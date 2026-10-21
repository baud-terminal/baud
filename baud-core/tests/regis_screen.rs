//! **ReGIS's screen and writing controls from the Open Extensions, pixel
//! for pixel, against `[[source: dec-std-070]]` 070-8 § 8.8.**
//!
//! The clip rectangle (§ 8.8.1.4.6). Pixels are read back from the page and
//! compared to pixels worked out from the text, never from another
//! terminal's output; with no cell size the page is the VT340's 800 by 480,
//! a coordinate a pixel.

use baud_core::*;
use baud_domain::{ImageBudget, Pixel};

mod support;

const BLACK: u32 = 0xff00_0000;

fn run(bytes: &[u8], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    support::feed(&mut t, bytes);
    f(&mut t);
}

/// The one page: its pixels and width.
fn page(t: &Term<'_>) -> (Vec<u32>, usize) {
    let imgs = t.images();
    let mut placed = imgs.placements();
    let (_, p) = placed.next().expect("a page placed");
    assert!(placed.next().is_none(), "one page placed");
    let v = imgs.image(p.image).expect("its image");
    (
        v.pixels.iter().map(|px| px.bits()).collect(),
        v.width.index(),
    )
}

/// The pixels that are not clear, as `(x, y)`.
fn lit(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    run(bytes, |t| {
        let (px, w) = page(t);
        out = px
            .iter()
            .enumerate()
            .filter(|(_, &c)| c != Pixel::CLEAR.bits())
            .map(|(i, _)| (i % w, i / w))
            .collect();
    });
    out
}

fn row(y: usize, xs: impl IntoIterator<Item = usize>) -> Vec<(usize, usize)> {
    xs.into_iter().map(|x| (x, y)).collect()
}

/// "If <n> is greater than zero, drawing actions are limited to the
/// interior of the specified clip rectangle. The "interior" of the clip
/// rectangle is defined to include what would be drawn as the boundary"
/// (§ 8.8.1.4.6 b).
#[test]
fn a_clip_rectangle_limits_drawing_to_its_inside_and_boundary() {
    let v = lit(b"\x1bPpS(R[100,100][110,110]1)P[90,105]V[120,105]\x1b\\");
    assert_eq!(v, row(105, 100..=110));
    // A fill, a circle and text are drawing actions too.
    let f = lit(b"\x1bPpS(R[100,100][110,110]1)F(V[50,50][150,50][150,150][50,150])\x1b\\");
    assert_eq!(f.len(), 11 * 11);
    assert!(
        f.iter()
            .all(|&(x, y)| (100..=110).contains(&x) && (100..=110).contains(&y))
    );
    let c = lit(b"\x1bPpS(R[100,100][200,200]1)P[100,100]C[+50]\x1b\\");
    assert!(!c.is_empty());
    assert!(c.iter().all(|&(x, y)| x >= 100 && y >= 100));
}

/// "values of <n> less than zero indicate that drawing activity is limited
/// to the exterior of the clip rectangle ... the boundary is NOT part of
/// the exterior" (§ 8.8.1.4.6 c).
#[test]
fn shielding_draws_outside_and_not_on_the_boundary() {
    let v = lit(b"\x1bPpS(R[100,100][110,110]-1)P[90,105]V[120,105]\x1b\\");
    let mut want = row(105, 90..=99);
    want.extend(row(105, 111..=120));
    assert_eq!(v, want);
}

/// "The extent of the clip rectangle can be set by an instance of the
/// command without changing the enabled state ... A subsequent S(R1) will
/// enable clipping to this area, or S(R0) will disable clipping, without
/// changing the extent" (§ 8.8.1.4.6 d).
#[test]
fn the_extent_and_the_switch_are_set_apart() {
    let line = b"P[90,105]V[120,105]";
    let with = |pre: &[u8]| {
        let mut b = b"\x1bPp".to_vec();
        b.extend_from_slice(pre);
        b.extend_from_slice(line);
        b.extend_from_slice(b"\x1b\\");
        lit(&b)
    };
    // An extent alone: clipping still off.
    assert_eq!(with(b"S(R[100,100][110,110])"), row(105, 90..=120));
    // The switch after it: the extent kept.
    assert_eq!(with(b"S(R[100,100][110,110])S(R1)"), row(105, 100..=110));
    // Off again, the extent kept for the next on.
    assert_eq!(with(b"S(R[100,100][110,110]1)S(R0)"), row(105, 90..=120));
    assert_eq!(
        with(b"S(R[100,100][110,110]1)S(R0)S(R1)"),
        row(105, 100..=110)
    );
    // "If only one specifier is present, the extent of the clip rectangle
    // is not changed. If more than two are present, only the last two are
    // used."
    assert_eq!(
        with(b"S(R[100,100][110,110]1)S(R[0,0])"),
        row(105, 100..=110)
    );
    assert_eq!(
        with(b"S(R[0,0][1,1][100,100][110,110]1)"),
        row(105, 100..=110)
    );
}

/// "These are positional specifiers, and may use relative addressing"
/// (§ 8.8.1.4.6 a), "multiple specifiers have cumulative action" (§ 8.4.5.1):
/// the second corner's relative parts from the first. "S(R[ ][+200,+200])
/// sets the clip rectangle to an extent 200 square, starting at the current
/// drawing position" (d).
#[test]
fn relative_corners_accumulate_from_the_drawing_position() {
    assert_eq!(
        lit(b"\x1bPpP[100,100]S(R[][+10,+10]1)P[90,105]V[120,105]\x1b\\"),
        row(105, 100..=110)
    );
    // The second corner from the first: [110,110] to [120,120].
    assert_eq!(
        lit(b"\x1bPpP[100,100]S(R[+10,+10][+10,+10]1)P[90,115]V[130,115]\x1b\\"),
        row(115, 110..=120)
    );
    // An absent part is +0 from the corner before.
    assert_eq!(
        lit(b"\x1bPpP[100,100]S(R[,][+20]1)P[90,100]V[130,100]\x1b\\"),
        row(100, 100..=120)
    );
}

/// "Screen erase, S(E), always erases the entire display, regardless of the
/// clipping state. Erase mode writing, on the other hand, is bounded by the
/// clip rectangle" (§ 8.8.1.4.6 f).
#[test]
fn screen_erase_ignores_the_clip_and_erase_writing_keeps_it() {
    run(b"\x1bPpS(R[100,100][110,110]1)S(I2)S(E)\x1b\\", |t| {
        let (px, _) = page(t);
        assert!(px.iter().all(|&c| c == 0xffcc_2121), "every pixel red");
    });
    let e = lit(b"\x1bPpS(R[100,100][110,110]1)W(E)P[90,105]V[120,105]\x1b\\");
    assert_eq!(e, row(105, 100..=110));
}

/// "The default clip area is the extent defined by the last valid S(A) ...
/// The default clip state is disabled ... Both defaults are restored by
/// valid S(A) commands" (§ 8.8.1.4.6 g).
#[test]
fn a_valid_s_a_restores_the_default_clip() {
    assert_eq!(
        lit(b"\x1bPpS(R[100,100][110,110]1)S(A[0,0][799,479])P[90,105]V[120,105]\x1b\\"),
        row(105, 90..=120)
    );
    // The default extent is S(A)'s: on with no extent given clips to it.
    let v = lit(b"\x1bPpS(A[0,0][399,239])S(R1)P[390,100]V[410,100]\x1b\\");
    assert_eq!(v.last().map(|p| p.0), Some(799), "[399] is pixel 799");
    // An invalid S(A) changes nothing.
    assert_eq!(
        lit(b"\x1bPpS(R[100,100][110,110]1)S(A[0,0])P[90,105]V[120,105]\x1b\\"),
        row(105, 100..=110)
    );
}

/// The clip is built: it is no longer counted as unbuilt, and the command
/// display, the terminal's own line, is not clipped.
#[test]
fn the_clip_is_not_counted_and_does_not_clip_the_command_display() {
    run(b"\x1bPpS(R[0,0][10,10]1)S(R0)\x1b\\", |t| {
        assert_eq!(t.regis_counts().unbuilt, 0);
    });
    // A line feed erases the command display's line to the background,
    // the whole of it, the clip on.
    run(b"\x1bP2pS(R[0,0][10,10]1)\n\x1b\\", |t| {
        let (px, w) = page(t);
        assert!((460..480).all(|y| (0..800).all(|x| px[y * w + x] == BLACK)));
    });
}
