//! The CPU compositor draws a Sixel placement, and it moves with the scroll.
//!
//! A dedicated test rather than a row in `golden.rs`, deliberately: that
//! harness provisions no image memory (`Owned::new`), so a Sixel there would
//! pin the digest of a *refused* image -- a gate that passes while proving the
//! opposite of what it claims. This one provisions the store and asserts the
//! pixels drew before trusting any digest.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ImageAlpha, ImageBudget, ImagePx, Pixel, ReadLen, Rows};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::{Canvas, WS, font, renderer};

/// A 1x6 red Sixel, the smallest well-formed image, for the second-image case.
const TINY: &[u8] = b"\x1bP0;1;0q#0;2;100;0;0~\x1b\\";

const SWIRL: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/recordings/sixel/swirl.raw"
));

fn feed(t: &mut Term, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// How many pixels in the 64x48 rect at the top-left differ from the theme
/// background -- the image's own pixels, drawn over the cells.
fn painted_in_image_rect(c: &Canvas) -> usize {
    let bg = Theme::dark().bg.0;
    let mut n = 0;
    for y in 0..48 {
        for x in 0..64 {
            if c.pixels[y * c.width + x] != bg {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_sixel_is_drawn_over_the_grid() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }

    // The 64x48 swirl anchors at the cursor (row 0, col 0) and fills most of
    // its rect -- assert the pixels *before* any digest, so "the image drew"
    // is distinguished from "the frame is uniformly background".
    let painted = painted_in_image_rect(&c);
    assert!(
        painted > 64 * 48 / 2,
        "a gradient swirl paints most of its 3072 pixels, not {painted}"
    );
}

#[test]
fn a_sixel_scrolls_off_and_the_frame_changes() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);

    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let before = painted_in_image_rect(&c);
    let digest_before = c.digest();
    assert!(before > 64 * 48 / 2, "the image drew first, {before}");

    // Scroll the screen well past the image: move the cursor to the bottom and
    // scroll far enough that the image (anchored at line 0) is above the top.
    feed(&mut t, &[b'\n'; 200]);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let after = painted_in_image_rect(&c);
    let digest_after = c.digest();

    // The image scrolled off, so its rect is background again -- and the frame
    // changed, which a blit that silently drew nothing could not produce.
    assert_eq!(after, 0, "the image scrolled off its rect, {after} left");
    assert_ne!(
        digest_before, digest_after,
        "the frame changed with the scroll"
    );
}

/// The frame `swirl.raw` paints at home, as a digest -- `s-sixel` gate 2,
/// the regression net the painted-pixel count above cannot be: a count holds
/// through a flipped image, a register read one entry off or a band drawn a
/// row late, and a digest does not. Taken on the first run and not an
/// oracle, which is the decoder's own gate; regenerate with
/// `GOLDEN_PRINT=1`, and `GOLDEN_DUMP=<dir>` writes the frame to look at.
const SWIRL_FRAME: u64 = 0x22cfc4f36d316b7b;

/// Writes the frame as a binary PPM: `golden.rs`'s shape, its three bytes
/// taken whole rather than cast one at a time.
fn dump_ppm(c: &Canvas, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", c.width, c.height).into_bytes();
    for &p in &c.pixels {
        out.extend_from_slice(&p.to_be_bytes()[1..]);
    }
    std::fs::write(path, out).unwrap();
}

#[test]
fn a_sixel_paints_to_its_pinned_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    // The pixels before the digest, so a pinned frame of a refused image --
    // a background and a cursor -- cannot be what this pins.
    let painted = painted_in_image_rect(&c);
    assert!(painted > 64 * 48 / 2, "the swirl drew, {painted} pixels");
    if let Ok(dir) = std::env::var("GOLDEN_DUMP") {
        dump_ppm(&c, &format!("{dir}/sixel-swirl.ppm"));
    }
    let got = c.digest();
    if std::env::var("GOLDEN_PRINT").is_ok() {
        println!("const SWIRL_FRAME: u64 = 0x{got:016x};");
    } else {
        assert_eq!(got, SWIRL_FRAME, "the swirl's frame digest 0x{got:016x}");
    }
}

/// An image placed on row 5 and the screen scrolled one line draws one cell
/// row higher, pixel for pixel -- `s-sixel` gate 3's first clause. The test
/// above scrolls the image clean off, which a placement that ignored the
/// scroll by a row would also pass once enough lines went by; this is the
/// one-line case. The cursor is hidden and parked on the bottom row, so the
/// only thing in the image's rect is the image.
#[test]
fn a_sixel_scrolled_one_line_draws_one_row_higher() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[6;1H");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    let rect = |c: &Canvas, top: usize| -> Vec<u32> {
        (0..48)
            .flat_map(|y| c.pixels[(top + y) * c.width..][..64].to_vec())
            .collect()
    };
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let before = rect(&c, 5 * ch);
    let above_before = rect(&c, 4 * ch);
    let bg = Theme::dark().bg.0;
    let painted = before.iter().filter(|&&p| p != bg).count();
    assert!(
        painted > 64 * 48 / 2,
        "the swirl drew on row 5, {painted} pixels"
    );

    feed(&mut t, b"\n");
    assert_eq!(
        t.image_origin(Rows::ZERO).top.value(),
        1,
        "one line feed on the bottom row is one scroll"
    );
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    // A comparison that cannot fail by being the same frame twice: the rect
    // one row up held something else before the scroll.
    assert_ne!(
        above_before, before,
        "the rect one row up was not the image already"
    );
    assert_eq!(
        rect(&c, 4 * ch),
        before,
        "the image did not move exactly one cell row up with its line"
    );
}

/// The text of a view row, blanks trimmed.
fn row_text(t: &Term, back: Rows, y: usize) -> String {
    t.view(back)
        .row(baud_domain::Row::from_index(y))
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|p| char::from_u32(p.value()))
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// **A shorter screen retires the rows above the cursor into the history, and
/// a reader scrolled back to them finds the image on the row it was drawn
/// on.** The history here is deeper than the count of scrolls -- sixty scrolls,
/// then thirty rows retired by the shrink -- which is the case a renderer
/// computing its own `scrolled - back` put thirty rows off, because a shrink
/// retires rows without scrolling.
#[test]
fn a_scrolled_back_image_after_a_shrink_draws_on_its_row() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &[b'\n'; 60]);
    feed(&mut t, b"\x1b[1;1HMARK\r\n");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    let short = baud_domain::WinSize {
        ws_row: 20,
        ws_col: WS.ws_col,
    };
    assert_eq!(t.resize(&short), baud_core::Resized::Ok);
    let back = Rows::of_count(30);
    assert_eq!(
        row_text(&t, back, 0),
        "MARK",
        "the view 30 up starts at MARK"
    );
    r.set_images(&t, back);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the image is in that view");
    assert_eq!(i32::from(p[0].y), ch, "one row below MARK");
}

/// **Each screen draws only its own images**, and the editor's go when it
/// leaves: the shell's image is not drawn over a full-screen program, and is
/// back where it was when the program exits.
#[test]
fn the_shells_image_is_not_drawn_over_the_editor() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let ch = i32::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 128));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    r.set_images(&t, Rows::ZERO);
    assert_eq!(r.image_placements().len(), 1, "the shell's image");
    feed(&mut t, b"\x1b[?1049h");
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_placements().is_empty(),
        "the shell's image is not drawn on the editor's screen"
    );
    feed(&mut t, b"\x1b[10;1H");
    feed(&mut t, TINY);
    r.set_images(&t, Rows::ZERO);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the editor's own image");
    assert_eq!(i32::from(p[0].y), 9 * ch, "on the row the editor put it");
    feed(&mut t, b"\x1b[?1049l");
    r.set_images(&t, Rows::ZERO);
    let p = r.image_placements();
    assert_eq!(p.len(), 1, "the editor's image went with its screen");
    assert_eq!((p[0].y, p[0].w, p[0].h), (0, 64, 48), "the shell's is back");
}

#[test]
fn a_static_image_is_snapshotted_once_not_per_frame() {
    // `set_images` runs every frame; a large image's per-frame pixel snapshot
    // (a `Vec` allocation plus a per-pixel conversion) is real cost while it is
    // on screen. The snapshot cache is kept across frames and rebuilt only for
    // a new `(id, epoch)`, so an unchanged image on screen is snapshotted once
    // however many frames it draws -- which a clear-every-frame renderer is not.
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 128));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);

    for _ in 0..5 {
        r.set_images(&t, Rows::ZERO);
    }
    assert_eq!(
        r.image_snapshots().value(),
        1,
        "the static image was snapshotted once over five frames, not five times"
    );

    // A new image bumps the store epoch, which drops the snapshot cache, so the
    // next frame re-snapshots what is referenced -- the count rises.
    feed(&mut t, TINY);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_snapshots().value() > 1,
        "a store's epoch change re-snapshots, not {}",
        r.image_snapshots().value()
    );
    assert!(
        !r.image_uploads().is_empty(),
        "the images are cached on screen"
    );

    // Scroll both images off the top. A build counter cannot see a leak, so the
    // cache length is the observable: an off-screen image's snapshot is dropped
    // rather than held until the next epoch change, which on a terminal showing
    // one image is forever.
    feed(&mut t, &[b'\n'; 60]);
    r.set_images(&t, Rows::ZERO);
    assert!(
        r.image_placements().is_empty(),
        "the images scrolled off screen"
    );
    assert!(
        r.image_uploads().is_empty(),
        "and their snapshots were dropped, not retained"
    );
}

/// The frame an incremental render leaves, against a fresh full repaint of
/// the same terminal by a second renderer: the two must be pixel for pixel
/// equal, or incremental damage left something stale or drew something
/// twice. The rule `incremental_frames_equal_full_repaints_at_every_read_boundary`
/// holds the CPU path to, with an image.
fn equals_a_full_repaint(t: &mut Term, c: &Canvas, faces: &[Face]) -> bool {
    let mut fresh = renderer(faces);
    let mut f = Canvas::for_renderer(&fresh);
    {
        let mut target = f.target();
        fresh.render(t, &mut target, Damage::Full);
    }
    c.pixels == f.pixels
}

/// **An image that arrives mid-session is drawn on the next incremental
/// frame.** The CPU path drew images only on a full frame, and a live
/// terminal renders incrementally after its first frame -- so on Linux, whose
/// driver is CPU-only, and on the macOS CPU path, a Sixel a program printed
/// did not appear until a resize.
#[test]
fn an_image_that_arrives_mid_session_is_drawn_on_an_incremental_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    assert_eq!(painted_in_image_rect(&c), 0, "the control: nothing yet");
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental);
    }
    let painted = painted_in_image_rect(&c);
    assert!(
        painted > 64 * 48 / 2,
        "the swirl drew on the incremental frame, {painted} pixels"
    );
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

/// **And an image that moves or goes is repainted where it was.** Scrolling
/// the swirl up a row, then off the screen, on incremental frames: each frame
/// must equal a full repaint, so the rows it left are the cells again and the
/// rows it is on show it once, not composited over its last position.
#[test]
fn an_image_moving_on_incremental_frames_equals_a_full_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l\x1b[6;1Hunder\x1b[6;1H");
    feed(&mut t, SWIRL);
    feed(&mut t, b"\x1b[50;1H");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let bg = Theme::dark().bg.0;
    let drawn = (0..48)
        .flat_map(|y| c.pixels[(5 * ch + y) * c.width..][..64].to_vec())
        .filter(|&p| p != bg)
        .count();
    assert!(
        drawn > 64 * 48 / 2,
        "the control: the swirl drew on row 5 before it moves, {drawn} pixels"
    );
    for step in 0..12 {
        feed(&mut t, b"\n");
        {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental);
        }
        assert!(
            equals_a_full_repaint(&mut t, &c, &faces),
            "after {} line feeds the incremental frame is not the full one",
            step + 1
        );
    }
}

/// **A still image costs no repaint.** The rows under a placement are
/// repainted when it arrives, leaves or moves; a frame whose only change is
/// elsewhere repaints that change and not the image's rows, which is what a
/// keystroke's echo beside a preview is. The blit on the frame after is
/// idempotent, so the pixels stay equal to a full repaint.
#[test]
fn a_still_image_costs_no_repaint() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"\x1b[?25l");
    feed(&mut t, SWIRL);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    feed(&mut t, b"\x1b[40;1Hx");
    let frame = {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Incremental)
    };
    assert_eq!(
        frame.rows_dirty.value(),
        1,
        "only the row that changed, not the swirl's"
    );
    assert!(equals_a_full_repaint(&mut t, &c, &faces));
}

/// **And the CPU path keeps no upload snapshots.** It blits from the
/// terminal's own pixels, so a converted copy of each visible image, which a
/// GPU uploads, is a second copy it never reads.
#[test]
fn the_cpu_path_keeps_no_upload_snapshots() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64 * 48 + 64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, SWIRL);
    for damage in [Damage::Full, Damage::Incremental, Damage::Incremental] {
        let mut target = c.target();
        r.render(&mut t, &mut target, damage);
    }
    assert!(
        painted_in_image_rect(&c) > 64 * 48 / 2,
        "the control: it drew"
    );
    assert_eq!(r.image_snapshots().value(), 0);
    assert!(r.image_uploads().is_empty());
}

/// The partly transparent image the alpha tests place: 40x30, its alpha
/// running through every value from clear to opaque across the rect, its
/// colour varying with position, so a blend that read the alpha a step
/// wrong, or not at all, misses on most pixels.
const PW: usize = 40;
const PH: usize = 30;

fn partial_pixel(x: usize, y: usize) -> Pixel {
    let c = |v: usize| Chan::from_u8(u8::try_from(v % 256).unwrap());
    Pixel::straight(c(x * 7 + y * 13), c(x * 6), c(255 - y * 8), c(90 + x))
}

fn fill_partial(px: &mut [Pixel]) {
    for (i, p) in px.iter_mut().enumerate() {
        *p = partial_pixel(i % PW, i / PW);
    }
}

/// Coloured text on a set, opaque background under the image's rect, and
/// the cursor at row 3, column 4, where the image goes.
const UNDER: &[u8] = b"\x1b[?25l\x1b[48;2;20;40;160m\x1b[38;2;250;220;30m\
\x1b[2;1HHHHHHHHHHHHHHHHH\x1b[3;1HWWWWWWWWWWWWWWWW\x1b[4;1HMMMMMMMMMMMMMMMM\
\x1b[5;1H################\x1b[6;1H@@@@@@@@@@@@@@@@\x1b[0m\x1b[3;4H";

/// What straight-alpha OVER gives `src` over the opaque `dst`, in the sRGB
/// numbers `Blend::Native` blends in: `(s a + d (255 - a)) / 255`, rounded
/// to nearest -- which no exact half can tie, 255 being odd.
fn over(src: Pixel, dst: u32) -> u32 {
    let a = u32::from(src.alpha().get());
    let (r, g, b) = src.rgb();
    let mix = |s: Chan, shift: u32| {
        let d = (dst >> shift) & 0xFF;
        (u32::from(s.get()) * a + d * (255 - a) + 127) / 255
    };
    0xFF00_0000 | mix(r, 16) << 16 | mix(g, 8) << 8 | mix(b, 0)
}

/// **A partly transparent image blends by its alpha.** Every pixel of its
/// rect is the straight-alpha OVER of the image's pixel on the frame drawn
/// without it -- so a path that drew a partial pixel whole (the opaque-or-
/// clear blit), or premultiplied it twice, fails on most of them.
#[test]
fn a_partial_image_blends_by_its_alpha() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let (cw, ch) = (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.metrics().cell_h.value()).unwrap(),
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let under = c.pixels.clone();
    let placed = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    assert!(!placed.is_none(), "the image placed");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let (x0, y0) = (3 * cw, 2 * ch);
    let (mut wrong, mut mid, mut text) = (0, 0, 0);
    for y in 0..PH {
        for x in 0..PW {
            let i = (y0 + y) * c.width + x0 + x;
            assert_eq!(under[i] >> 24, 0xFF, "the frame under the image is opaque");
            let src = partial_pixel(x, y);
            if !matches!(src.alpha().get(), 0 | 0xFF) {
                mid += 1;
            }
            if under[i] != under[(y0 + y) * c.width + x0] {
                text += 1;
            }
            if c.pixels[i] != over(src, under[i]) {
                wrong += 1;
            }
        }
    }
    assert!(mid > PW * PH * 9 / 10, "the control: {mid} partial pixels");
    assert!(text > 0, "the control: glyphs lie under the image");
    assert_eq!(
        wrong, 0,
        "pixels not the OVER of the image on the frame under it"
    );
}

/// **A still partial image repaints its rows on every frame**, and only a
/// partial one: a blit over its own last blit would blend it twice, so the
/// rows it covers are dirty whatever else changed, and counted; the frame
/// equals a full repaint. `a_still_image_costs_no_repaint` is the opaque
/// side, which still repaints nothing.
#[test]
fn a_still_partial_image_repaints_its_rows_every_frame() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut c = Canvas::for_renderer(&r);
    let ch = usize::try_from(r.metrics().cell_h.value()).unwrap();
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    let _ = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let rows = u64::try_from((2 * ch + PH - 1) / ch - 2 + 1).unwrap();
    for step in 0..3u8 {
        feed(
            &mut t,
            &[0x1b, b'[', b'4', b'0', b';', b'1', b'H', b'a' + step],
        );
        let frame = {
            let mut target = c.target();
            r.render(&mut t, &mut target, Damage::Incremental)
        };
        assert_eq!(
            frame.rows_partial.value(),
            rows,
            "frame {step}: the rows under the partial image, counted"
        );
        assert_eq!(
            frame.rows_dirty.value(),
            1 + rows,
            "frame {step}: the row that changed and the image's"
        );
        assert!(
            equals_a_full_repaint(&mut t, &c, &faces),
            "frame {step}: the incremental frame is not the full one"
        );
    }
}

/// **The upload a GPU takes is the store's straight pixels**, unconverted:
/// the fragment premultiplies (`glyph_fragment`'s colour branch), so a
/// snapshot that premultiplied too would darken every partial pixel, and
/// one that forced the alpha opaque would draw it whole.
#[test]
fn the_upload_is_the_straight_pixels() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(PW * PH));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    let _ = t.put_image(
        ImagePx::of(u16::try_from(PW).unwrap()),
        ImagePx::of(u16::try_from(PH).unwrap()),
        ImageAlpha::Partial,
        fill_partial,
    );
    r.set_images(&t, Rows::ZERO);
    let ups = r.image_uploads();
    assert_eq!(ups.len(), 1, "one image on screen");
    let want: Vec<u32> = (0..PW * PH)
        .map(|i| partial_pixel(i % PW, i / PW).bits())
        .collect();
    assert_eq!(ups[0].pixels, want);
}

/// **A Sixel's clear pixels show the cells under them** on the CPU path,
/// under both blends, each its own opaque loop: an 8x6 Sixel whose data
/// sets only its second row draws that row red -- pure red, which `Linear`'s
/// tables carry through unchanged -- and leaves the five others as the
/// frame without it. The swirl paints every pixel, so nothing above sees a
/// clear one.
#[test]
fn a_sixels_clear_pixels_show_the_cells_under_them() {
    for blend in [Blend::Native, Blend::Linear] {
        clear_pixels_under(blend);
    }
}

fn clear_pixels_under(blend: Blend) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(support::PX).unwrap(),
        Theme::dark(),
        blend,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut c = Canvas::for_renderer(&r);
    let (cw, ch) = (
        usize::try_from(r.metrics().cell_w.value()).unwrap(),
        usize::try_from(r.metrics().cell_h.value()).unwrap(),
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64));
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, UNDER);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let under = c.pixels.clone();
    feed(&mut t, b"\x1bP0;1;0q\"1;1;8;6#0;2;100;0;0AAAAAAAA\x1b\\");
    assert_eq!(t.images().placements().count(), 1, "the Sixel placed");
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    let (x0, y0) = (3 * cw, 2 * ch);
    for y in 0..6 {
        for x in 0..8 {
            let i = (y0 + y) * c.width + x0 + x;
            let want = if y == 1 { 0xFFFF_0000 } else { under[i] };
            assert_eq!(c.pixels[i], want, "{blend:?}: pixel ({x}, {y})");
        }
    }
}
