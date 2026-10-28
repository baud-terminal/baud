//! A zoom keeps the glyphs it has drawn: a rescale back to a size the
//! atlas holds draws no glyph again, and every frame at every size is the
//! frame a renderer drawing that size afresh paints.

mod support;
use baud_domain::Codepoint;
use baud_font::Face;
use baud_render::{Damage, PxSize, Renderer, SIZES};
use support::*;

/// btop's last screen -- box drawing, block elements, braille -- with a
/// double-height pair of lines over it carrying text, box drawing and a
/// colour glyph, and an encircled word: every memo the atlas keeps per
/// size, the stretched ones included.
fn scene(emoji: char) -> Vec<u8> {
    let mut bytes = recording("utf8/btop.raw");
    for row in [1, 2] {
        let half = if row == 1 { 3 } else { 4 };
        bytes.extend_from_slice(
            format!("\x1b[{row};1H\x1b#{half}\x1b[2K┼╋▒⣿ zoom {emoji} ").as_bytes(),
        );
    }
    bytes.extend_from_slice("\x1b[4;1H\x1b[52mring\x1b[54m ┼▒⣿".as_bytes());
    bytes
}

/// A codepoint the colour face maps.
fn an_emoji(face: &Face<'_>) -> char {
    (0x80..=0x10FFFF)
        .find(|&cp| Codepoint::new(cp).is_some_and(|c| face.glyph(c).is_some()))
        .and_then(char::from_u32)
        .expect("the CBDT subset maps something")
}

/// The scene's frame from a renderer made at `px` that has drawn nothing
/// before.
fn fresh(faces: &[Face<'_>], bytes: &[u8], px: f32) -> Vec<u32> {
    let mut r = Renderer::new(
        baud_render::Stack::new(faces).unwrap(),
        PxSize::of(px).unwrap(),
        baud_render::Theme::dark(),
        baud_render::Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut out = Vec::new();
    replay_final(bytes, |t| {
        let mut c = Canvas::for_renderer(&r);
        paint(&mut r, t, &mut c, Damage::Full);
        out = c.pixels;
    });
    out
}

/// Rescales to `px` and paints the frame that follows, returning it and
/// the glyphs the atlas drew for it: outlines, colour images and masks.
fn zoom(r: &mut Renderer<'_, '_>, t: &mut baud_core::Term<'_>, px: f32) -> (Vec<u32>, u64) {
    let drawn = |r: &Renderer<'_, '_>| {
        let s = r.atlas_stats();
        s.rasterised.value() + s.coloured.value() + s.synthesised.value()
    };
    let before = drawn(r);
    r.rescale(PxSize::of(px).unwrap()).unwrap();
    let mut c = Canvas::for_renderer(r);
    paint(r, t, &mut c, Damage::Full);
    (c.pixels, drawn(r) - before)
}

fn differing(a: &[u32], b: &[u32]) -> usize {
    assert_eq!(a.len(), b.len(), "the frames are one size");
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

#[test]
fn a_zoom_back_draws_no_glyph_again_and_paints_a_fresh_frame() {
    let (jbm, noto) = (
        font("JetBrainsMono-Regular.ttf"),
        font("NotoColorEmoji-subset.ttf"),
    );
    let faces = [Face::parse(&jbm).unwrap(), Face::parse(&noto).unwrap()];
    let bytes = scene(an_emoji(&faces[1]));
    let mut r = renderer(&faces);
    replay_final(&bytes, |t| {
        let mut c = Canvas::for_renderer(&r);
        paint(&mut r, t, &mut c, Damage::Full);
        assert_eq!(differing(&c.pixels, &fresh(&faces, &bytes, PX)), 0);
        // 15 and 20 px put a cell's glyph at the same phase in its cell
        // (JetBrains Mono's advance is 0.6 em, 9 and 12 whole pixels), so
        // only the size tells their glyphs apart.
        for (px, step, again) in [
            (15.0, "the first zoom", false),
            (20.0, "the second zoom", false),
            (15.0, "the zoom back", true),
            (20.0, "the zoom in again", true),
            (PX, "the zoom back to the start", true),
        ] {
            let grown = r.atlas_stats().grown;
            let (pixels, drawn) = zoom(&mut r, t, px);
            assert_ne!(
                r.atlas_stats().grown.value(),
                grown.value(),
                "{step}: what kept a rectangle is told to rebuild"
            );
            assert_eq!(
                differing(&pixels, &fresh(&faces, &bytes, px)),
                0,
                "{step}: the frame at {px} px is not a fresh one's"
            );
            if again {
                assert_eq!(drawn, 0, "{step} drew {drawn} glyphs again");
            } else {
                // btop's last screen and the scene's lines draw 90 glyphs at
                // the first size.
                assert!(drawn > 80, "{step} draws the screen's glyphs: {drawn}");
            }
        }
    });
}

/// **The atlas holds [`SIZES`] sizes and starts over at the next.** Four
/// zooms a point apart and back to the first draw nothing again; a fifth
/// size starts the atlas over, so the first size is drawn afresh after it.
/// Every frame is a fresh renderer's at its size.
#[test]
fn a_size_past_the_held_ones_starts_the_atlas_over() {
    let jbm = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&jbm).unwrap()];
    let bytes = recording("utf8/btop.raw");
    let mut r = renderer(&faces);
    let held: Vec<f32> = (0..SIZES).map(|k| PX + k as f32).collect();
    replay_final(&bytes, |t| {
        let mut c = Canvas::for_renderer(&r);
        paint(&mut r, t, &mut c, Damage::Full);
        for &px in &held[1..] {
            let (pixels, drawn) = zoom(&mut r, t, px);
            assert_eq!(differing(&pixels, &fresh(&faces, &bytes, px)), 0, "{px} px");
            assert!(drawn > 0, "{px} px is new and draws its glyphs");
        }
        for &px in held.iter().rev() {
            let (pixels, drawn) = zoom(&mut r, t, px);
            assert_eq!(differing(&pixels, &fresh(&faces, &bytes, px)), 0, "{px} px");
            assert_eq!(drawn, 0, "{px} px is held and drew {drawn} glyphs");
        }
        let past = PX + SIZES as f32;
        let (pixels, drawn) = zoom(&mut r, t, past);
        assert_eq!(differing(&pixels, &fresh(&faces, &bytes, past)), 0);
        assert!(drawn > 0, "{past} px is new");
        let (pixels, drawn) = zoom(&mut r, t, PX);
        assert_eq!(differing(&pixels, &fresh(&faces, &bytes, PX)), 0);
        assert!(
            drawn > 0,
            "the atlas started over, so {PX} px is drawn afresh"
        );
    });
}

/// The same feed in reads, for a renderer whose size moved mid-stream:
/// the frames after a zoom back still track the screen as it changes.
#[test]
fn frames_after_a_zoom_back_follow_the_screen() {
    let jbm = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&jbm).unwrap()];
    let bytes = recording("utf8/vim-unicode.raw");
    let (mut zoomed, mut plain) = (renderer(&faces), renderer(&faces));
    let mut reads = 0usize;
    replay(&bytes, |t, i| {
        if i == 1 {
            zoomed.rescale(PxSize::of(PX * 2.0).unwrap()).unwrap();
            let mut c = Canvas::for_renderer(&zoomed);
            paint(&mut zoomed, t, &mut c, Damage::Incremental);
            zoomed.rescale(PxSize::of(PX).unwrap()).unwrap();
        }
        let (mut a, mut b) = (Canvas::for_renderer(&zoomed), Canvas::for_renderer(&plain));
        paint(&mut zoomed, t, &mut a, Damage::Full);
        paint(&mut plain, t, &mut b, Damage::Full);
        assert_eq!(differing(&a.pixels, &b.pixels), 0, "read {i}");
        reads += 1;
    });
    assert!(reads > 2, "the recording takes several reads: {reads}");
}

/// **Another stack of faces starts the atlas over at its own size.** A
/// program's `OSC 50` restacks the renderer at whatever size is in force;
/// a rescale back to the size before it must draw that size afresh, not
/// take up glyphs the restack drew at another.
#[test]
fn a_restack_at_another_size_holds_its_glyphs_under_that_size() {
    let jbm = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&jbm).unwrap()];
    let bytes = recording("utf8/btop.raw");
    let mut r = renderer(&faces);
    replay_final(&bytes, |t| {
        let mut c = Canvas::for_renderer(&r);
        paint(&mut r, t, &mut c, Damage::Full);
        r.restack(
            baud_render::Stack::new(&faces).unwrap(),
            PxSize::of(20.0).unwrap(),
        )
        .unwrap();
        let mut c = Canvas::for_renderer(&r);
        paint(&mut r, t, &mut c, Damage::Full);
        assert_eq!(
            differing(&c.pixels, &fresh(&faces, &bytes, 20.0)),
            0,
            "20 px"
        );
        let (pixels, drawn) = zoom(&mut r, t, PX);
        assert_eq!(
            differing(&pixels, &fresh(&faces, &bytes, PX)),
            0,
            "back at {PX} px"
        );
        assert!(
            drawn > 0,
            "the restack voided {PX} px, so it is drawn afresh"
        );
    });
}
