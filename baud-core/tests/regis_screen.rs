//! **ReGIS's screen and writing controls from the Open Extensions, pixel
//! for pixel, against `[[source: dec-std-070]]` 070-8 § 8.8.**
//!
//! The clip rectangle (§ 8.8.1.4.6), the line width (§ 8.8.3.2), the
//! screen's scroll (§ 8.8.1.1, § 8.8.1.2) and its scale (§ 8.8.1.4.7).
//! Pixels are read back from the page and compared to pixels worked out
//! from the text, never from another terminal's output; with no cell size
//! the page is the VT340's 800 by 480, a coordinate a pixel.

use baud_core::*;
use baud_domain::{
    Col, ImageBudget, ImagePx, LocatorAt, LocatorButton, LocatorButtons, LocatorInput, LocatorKind,
    Pixel, PointerPx, Row,
};

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

// ---------------------------------------------------------------------------
// `W(L<n>)`, 070-8 § 8.8.3.2: "the width or weight of lines and curves. The
// value <n> is a multiplier of the device's nominal line width."
// ---------------------------------------------------------------------------

/// The pixels a round pen `d` pixels across covers about `(x, y)`: a pixel
/// whose centre is within `d / 2` of the pen's centre, the pen centred on
/// the pixel for an odd `d` and on its upper-left corner for an even one.
fn pen(x: i64, y: i64, d: i64) -> Vec<(usize, usize)> {
    let c = (d + 1) % 2;
    let mut v = Vec::new();
    for dv in -(d / 2)..=(d - 1) / 2 {
        for du in -(d / 2)..=(d - 1) / 2 {
            if (2 * du + c).pow(2) + (2 * dv + c).pow(2) <= d * d {
                let at = |v: i64| usize::try_from(v).expect("on the page");
                v.push((at(x + du), at(y + dv)));
            }
        }
    }
    v
}

fn sorted(mut v: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    v.sort_by_key(|&(x, y)| (y, x));
    v.dedup();
    v
}

/// "W(L2) sets a width twice the nominal width, W(L3) three times": the
/// nominal width is a pixel, and a wider line is a round pen of that many
/// pixels swept along it.
#[test]
fn a_wide_line_is_a_round_pen_swept_along_it() {
    let got = sorted(lit(b"\x1bPpW(L5)P[100,100]V[120,100]\x1b\\"));
    let want = sorted((100..=120).flat_map(|x| pen(x, 100, 5)).collect());
    assert_eq!(got, want);
    // By hand: rows 99 to 101 from 98 to 122, rows 98 and 102 from 99 to
    // 121, the pen's corners off.
    assert_eq!(got.len(), 3 * 25 + 2 * 23);
    // A diagonal is as wide across as a horizontal: the pen at each pixel
    // of the one-pixel line.
    let got = sorted(lit(b"\x1bPpW(L4)P[100,100]V[110,110]\x1b\\"));
    let want = sorted((0..=10).flat_map(|k| pen(100 + k, 100 + k, 4)).collect());
    assert_eq!(got, want);
}

/// W(L1) is today's line, W(L0) "the minimum imageable line width", a
/// pixel, and a fraction rounds to the nearest: "Devices that do not
/// support fractions may truncate to the next lower integer or round to
/// the nearest integer".
#[test]
fn nominal_minimum_and_fractions() {
    let rows = |w: &str| {
        let b = format!("\x1bPpW(L{w})P[100,100]V[120,100]\x1b\\");
        let mut ys: Vec<usize> = lit(b.as_bytes()).iter().map(|p| p.1).collect();
        ys.sort();
        ys.dedup();
        ys.len()
    };
    let thin = lit(b"\x1bPpP[100,100]V[120,100]\x1b\\");
    assert_eq!(lit(b"\x1bPpW(L1)P[100,100]V[120,100]\x1b\\"), thin);
    assert_eq!(rows("0"), 1);
    assert_eq!(rows("0.4"), 1);
    assert_eq!(rows("1.5"), 2);
    assert_eq!(rows("2.4"), 2);
    assert_eq!(rows("3"), 3);
    // No limit short of the page: forty is forty.
    assert_eq!(rows("40"), 40);
}

/// "W(L2) sets a width twice the nominal width, W(L3) three times, and so
/// on": § 8.8.3.2 names no widest line, and baud sets none short of the
/// page. A pen wider than twice the page's diagonal covers the whole page
/// from any pixel of it.
#[test]
fn a_width_has_no_limit_short_of_the_page() {
    let got = sorted(lit(b"\x1bPpW(L40)P[200,200]V[260,200]\x1b\\"));
    let want = sorted((200..=260).flat_map(|x| pen(x, 200, 40)).collect());
    assert_eq!(got, want);
    for body in [
        &b"W(L65535)P[400,240]V[+1]"[..],
        &b"W(L99999)P[0,0]V[799,479]"[..],
        &b"W(L65535)P[400,240]C[+300]"[..],
    ] {
        let mut b = b"\x1bPp".to_vec();
        b.extend_from_slice(body);
        b.extend_from_slice(b"\x1b\\");
        assert_eq!(
            lit(&b).len(),
            800 * 480,
            "{}",
            String::from_utf8_lossy(body)
        );
    }
}

/// A width changed between two lines of a stroke: the second line's pen
/// begins at its own first step, the joint the first line's.
#[test]
fn a_width_changed_inside_a_stroke_draws_from_the_next_step() {
    let got = sorted(lit(b"\x1bPpW(L5)P[100,100]V[140,100]W(L9)V[180,100]\x1b\\"));
    let mut want: Vec<(usize, usize)> = (100..=140).flat_map(|x| pen(x, 100, 5)).collect();
    want.extend((141..=180).flat_map(|x| pen(x, 100, 9)));
    assert_eq!(got, sorted(want));
}

/// **A wide line writes what the round pen at each of its steps writes**,
/// each pixel at the first step that writes it (§ 8.7.2.4.1): with a line
/// pattern, overlay and complement write the pen at each step the pattern
/// has a one, and replace writes each pixel at the first step to cover it,
/// the foreground on a one and the background on a zero. The steps and
/// their pattern are the one-pixel line's, drawn alongside.
#[test]
fn a_patterned_wide_line_is_the_pen_at_each_step() {
    // P2 is 11110000, each element two steps long: on for eight, off for
    // eight.
    for (opts, path, d) in [
        ("P2", "P[100,100]V[160,100]", 5),
        ("P2", "P[100,100]V[150,140][220,90][100,100]", 6),
        ("C,P2", "P[300,200]C[+60]", 7),
        (
            "C,P2",
            "P[100,300]C(S)[+40,-40][+40,+40][+40,-40][+40,+40]",
            6,
        ),
    ] {
        let thin = lit(format!("\x1bPpW({opts}){path}\x1b\\").as_bytes());
        let want = sorted(
            thin.iter()
                .flat_map(|&(x, y)| pen(x as i64, y as i64, d))
                .collect(),
        );
        let got = sorted(lit(format!("\x1bPpW({opts},L{d}){path}\x1b\\").as_bytes()));
        assert!(thin.len() > 20);
        assert_eq!(got, want, "W({opts},L{d}){path}");
    }
    // Replace: the first step to cover a pixel writes it.
    let colours = |b: &[u8]| {
        let mut v = Vec::new();
        run(b, |t| {
            let (px, w) = page(t);
            v = px
                .iter()
                .enumerate()
                .filter(|(_, &c)| c != Pixel::CLEAR.bits())
                .map(|(i, &c)| ((i % w, i / w), c))
                .collect::<Vec<_>>();
        });
        v
    };
    let thin = colours(b"\x1bPpW(R,P2)P[100,100]V[160,100]\x1b\\");
    assert_eq!(thin.len(), 61);
    let mut want: Vec<((usize, usize), u32)> = Vec::new();
    for &((x, y), c) in &thin {
        for p in pen(x as i64, y as i64, 5) {
            if !want.iter().any(|&(q, _)| q == p) {
                want.push((p, c));
            }
        }
    }
    want.sort_by_key(|&((x, y), _)| (y, x));
    let mut got = colours(b"\x1bPpW(R,P2,L5)P[100,100]V[160,100]\x1b\\");
    got.sort_by_key(|&((x, y), _)| (y, x));
    assert!(want.iter().any(|&(_, c)| c == BLACK));
    assert_eq!(got, want);
}

/// **Each pixel of a wide stroke is written once**, a polyline's or a
/// curve's, so complement writing over clear makes every pixel of it
/// index 15, gray 75%, and none back to 0: "complement the contents of the
/// pixels present in the image" (§ 8.7.2.4.1.1).
#[test]
fn a_wide_stroke_complements_each_pixel_once() {
    for body in [
        &b"W(C,L7)P[200,200]V[300,200][250,280][200,200]"[..],
        &b"W(C,L9)P[300,200]C[+60]"[..],
        &b"W(C,L6)P[100,300]C(S)[+40,-40][+40,+40][+40,-40][+40,+40]"[..],
    ] {
        let mut b = b"\x1bPp".to_vec();
        b.extend_from_slice(body);
        b.extend_from_slice(b"\x1b\\");
        run(&b, |t| {
            let (px, _) = page(t);
            let lit: Vec<u32> = px
                .into_iter()
                .filter(|&c| c != Pixel::CLEAR.bits())
                .collect();
            assert!(lit.len() > 100);
            assert!(
                lit.iter().all(|&c| c == 0xffcc_cccc),
                "{}: a pixel complemented twice",
                String::from_utf8_lossy(body)
            );
        });
    }
}

/// A second stroke is a stroke of its own: drawn again over the first, it
/// complements every pixel back.
#[test]
fn a_new_stroke_writes_over_the_last() {
    run(
        b"\x1bPpW(C,L5)P[100,100]V[140,100]P[100,100]V[140,100]\x1b\\",
        |t| {
            let (px, _) = page(t);
            let lit: Vec<u32> = px
                .into_iter()
                .filter(|&c| c != Pixel::CLEAR.bits())
                .collect();
            assert!(lit.len() > 100);
            assert!(lit.iter().all(|&c| c == BLACK), "15 back to 0");
        },
    );
}

/// "Line width is set to nominal" by a valid S(A) (§ 8.8.1.4.1 b), a
/// temporary width lasts its command, and W(L) is no longer counted.
#[test]
fn the_width_resets_at_s_a_and_a_temporary_one_ends_with_its_command() {
    let thin = lit(b"\x1bPpP[100,100]V[120,100]\x1b\\");
    assert_eq!(
        lit(b"\x1bPpW(L5)S(A[0,0][799,479])P[100,100]V[120,100]\x1b\\"),
        thin
    );
    let t = lit(b"\x1bPpP[100,100]V(W(L3))[120,100]P[100,200]V[120,200]\x1b\\");
    assert_eq!(t.iter().filter(|p| p.1 == 200).count(), 21);
    // A pen three across covers its whole square: 99 to 121 on each row.
    assert_eq!(
        t.iter().filter(|p| (99..=101).contains(&p.1)).count(),
        3 * 23
    );
    run(b"\x1bPpW(L2)\x1b\\", |t| {
        assert_eq!(t.regis_counts().unbuilt, 0)
    });
}

/// **A stroke's marks do not move with the page's rows**: once lines move
/// the pixels away, a new stroke over them writes them again. A line
/// inserted above moves them down a cell; the window moving down a line
/// carries them up one onto the next drawing's page.
#[test]
fn moved_rows_keep_no_marks() {
    // 80 by 24 cells of 10 by 20: an 800 by 480 page, a coordinate a pixel.
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let cases: [(&[u8], usize); 2] = [(b"\x1b[1;1H\x1b[L", 50), (b"\x1b[24;1H\n", 10)];
    for (moves, y) in cases {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
        support::feed(&mut t, b"\x1bPpW(C,L5)P[100,30]V[140,30]\x1b\\");
        support::feed(&mut t, moves);
        let again = format!("\x1bPpW(C,L5)P[100,{y}]V[140,{y}]\x1b\\");
        support::feed(&mut t, again.as_bytes());
        let imgs = t.images();
        let (_, p) = imgs.placements().last().expect("a page");
        let v = imgs.image(p.image).expect("its image");
        let lit: Vec<u32> = v
            .pixels
            .iter()
            .map(|px| px.bits())
            .filter(|&c| c != Pixel::CLEAR.bits())
            .collect();
        assert!(lit.len() > 100);
        assert!(
            lit.iter().all(|&c| c == BLACK),
            "{}: a pixel the first stroke marked was passed over",
            String::from_utf8_lossy(moves)
        );
    }
}

// ---------------------------------------------------------------------------
// `S[x,y]` and `S<pv>`, 070-8 § 8.8.1.1: under the scrolling model "position
// arguments specify an address in the display address space which will
// become the new origin".
// ---------------------------------------------------------------------------

/// The page's pixel at `(x, y)` after `bytes`.
fn at(bytes: &[u8], pts: &[(usize, usize)]) -> Vec<u32> {
    let mut out = Vec::new();
    run(bytes, |t| {
        let (px, w) = page(t);
        out = pts.iter().map(|&(x, y)| px[y * w + x]).collect();
    });
    out
}

const LINE: &[u8] = b"\x1bPpP[100,100]V[110,100]";
const GRAY: u32 = 0xff87_8787;
const CLEAR: u32 = 0;

fn scrolled(scroll: &[u8]) -> Vec<(usize, usize)> {
    let mut b = LINE.to_vec();
    b.extend_from_slice(scroll);
    b.extend_from_slice(b"\x1b\\");
    let mut v = Vec::new();
    run(&b, |t| {
        let (px, w) = page(t);
        v = px
            .iter()
            .enumerate()
            .filter(|(_, &c)| c == GRAY)
            .map(|(i, _)| (i % w, i / w))
            .collect();
    });
    v
}

/// "The position you specify becomes the new upper-left corner of the
/// screen" (`[[source: vt3xx-screen-control]]`): the data move left and up
/// by the address's distance from the origin, and what they reveal is the
/// background, "moved off and erased from the display medium".
#[test]
fn a_scroll_moves_the_page_and_reveals_the_background() {
    assert_eq!(scrolled(b"S[+50,+20]"), row(80, 50..=60));
    let px = at(
        b"\x1bPpP[100,100]V[110,100]S[+50,+20]\x1b\\",
        &[
            (100, 100),
            (0, 0),
            (749, 459),
            (750, 0),
            (799, 459),
            (0, 460),
            (799, 479),
        ],
    );
    assert_eq!(px, vec![CLEAR, CLEAR, CLEAR, BLACK, BLACK, BLACK, BLACK]);
    // The other way: the data move right and down, the left and top
    // revealed.
    assert_eq!(scrolled(b"S[-50,-20]"), row(120, 150..=160));
    let px = at(
        b"\x1bPpP[100,100]V[110,100]S[-50,-20]\x1b\\",
        &[(49, 300), (50, 300), (300, 19), (300, 20)],
    );
    assert_eq!(px, vec![BLACK, CLEAR, BLACK, CLEAR]);
}

/// A relative part and an omitted one are measured from the origin, the
/// address at the screen's upper-left, so the drawing position does not
/// matter, an absolute part is the address itself, and an omitted part
/// moves nothing.
#[test]
fn a_scroll_is_measured_from_the_origin() {
    let want = row(80, 50..=60);
    assert_eq!(scrolled(b"S[50,20]"), want);
    assert_eq!(scrolled(b"P[300,300]S[+50,+20]"), want);
    assert_eq!(scrolled(b"S[,+20]"), row(80, 100..=110));
    assert_eq!(scrolled(b"S[+50]"), row(100, 50..=60));
    // In a space whose origin is [200,100], [250,120] is fifty units on.
    assert_eq!(
        scrolled(b"S(A[200,100][999,579])S[250,120]"),
        row(80, 50..=60)
    );
    // An inverted space: +X is leftward, so the origin fifty units on moves
    // the data right.
    assert_eq!(
        scrolled(b"S(A[799,0][0,479])S[+50,+0]"),
        row(100, 150..=160)
    );
}

/// "Numeric arguments to the Screen control command are treated as pixel
/// vector specifiers ... executed as a relative bracketed position
/// argument" (§ 8.8.1.2), at the multiplier `S(W(M))` sets, "the
/// granularity of the display motion" (§ 8.8.1.4.8); each digit a scroll.
#[test]
fn a_pixel_vector_scrolls_a_step_at_the_multiplier() {
    // Pixel vector 0 is to the right: the origin a pixel right, the data a
    // pixel left.
    assert_eq!(scrolled(b"S0"), row(100, 99..=109));
    assert_eq!(scrolled(b"S00"), row(100, 98..=108));
    assert_eq!(scrolled(b"S(W(M10))6"), row(90, 100..=110));
    // The temporary multiplier lasts its command.
    assert_eq!(scrolled(b"S(W(M10))6S6"), row(89, 100..=110));
    // The persistent one is W(M)'s.
    assert_eq!(scrolled(b"W(M5)S4"), row(100, 105..=115));
}

/// "complement the contents of the pixels present in the image": the
/// indices move with the pixels, so complement over the moved line turns
/// it back; the lit count is kept exactly; the drawing position does not
/// move ("Does not change the coordinate system"); the clip does not hold
/// the data back, "data that move out of the clip rectangle never
/// disappear just because of movement out of the clipping rectangle"
/// (§ 8.8.1.4.6 e).
#[test]
fn a_scroll_moves_the_indices_keeps_the_count_and_ignores_the_clip() {
    run(
        b"\x1bPpW(C)P[100,100]V[110,100]S[+50,+20]P[50,80]V[60,80]\x1b\\",
        |t| {
            let (px, w) = page(t);
            assert!((50..=60).all(|x| px[80 * w + x] == BLACK), "15 back to 0");
        },
    );
    run(
        b"\x1bPpF(V[0,0][100,0][100,100][0,100])S[+30,+30]\x1b\\",
        |t| {
            let imgs = t.images();
            let (_, p) = imgs.placements().next().unwrap();
            let v = imgs.image(p.image).unwrap();
            let n = v
                .pixels
                .iter()
                .filter(|px| px.bits() != Pixel::CLEAR.bits())
                .count();
            assert_eq!(imgs.lit(p.image), u32::try_from(n).ok());
        },
    );
    let mut r = Vec::new();
    support::with_corpus(|t| {
        support::feed(t, b"\x1bPpP[123,45]S[+50,+20]S3R(P)\x1b\\");
        r = t.reply().to_vec();
    });
    assert_eq!(r, b"[123,45]\r");
    assert_eq!(
        scrolled(b"S(R[90,90][120,110]1)S[+50,+20]"),
        row(80, 50..=60)
    );
    run(b"\x1bPpS[+5]S2\x1b\\", |t| {
        assert_eq!(t.regis_counts().unbuilt, 0)
    });
}

// ---------------------------------------------------------------------------
// `S(S<n>)`, 070-8 § 8.8.1.4.7: "This option is used for devices which can
// scale images after they are displayed. Taking the model of pixel
// replication, the subargument <n> is treated as a multiplicative scale
// factor for each pixel in the display."
// ---------------------------------------------------------------------------

fn block(x0: usize, x1: usize, y0: usize, y1: usize) -> Vec<(usize, usize)> {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .collect()
}

/// Each pixel shown `n` by `n` from the screen's upper-left, `X` and `Y`
/// apart, and the bitmap kept, so `S(S1)` shows it again: "The display
/// can be rescaled only by respecifying a scale factor of 1".
#[test]
fn scaling_magnifies_from_the_upper_left_and_s_s1_shows_the_bitmap_again() {
    let base = lit(b"\x1bPpP[100,100]V[110,100]\x1b\\");
    assert_eq!(base, row(100, 100..=110));
    assert_eq!(
        sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S2)\x1b\\")),
        block(200, 222, 200, 202)
    );
    assert_eq!(lit(b"\x1bPpP[100,100]V[110,100]S(S2)S(S1)\x1b\\"), base);
    assert_eq!(
        sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S(X2))\x1b\\")),
        block(200, 222, 100, 101)
    );
    assert_eq!(
        sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S(Y3))\x1b\\")),
        block(100, 111, 300, 303)
    );
}

/// A drawing that runs past the magnified window is shown up to the
/// page's edge and no further, its lit pixels counted.
#[test]
fn a_drawing_past_the_magnified_window_stops_at_the_edge() {
    // A first drawing places the page, then the one past the edge.
    run(b"\x1bPpS(S2)P[0,0]V[1,0]P[300,100]V[450,100]\x1b\\", |t| {
        let (px, w) = page(t);
        let mut lit: Vec<(usize, usize)> = px
            .iter()
            .enumerate()
            .filter(|(_, &c)| c != Pixel::CLEAR.bits())
            .map(|(i, _)| (i % w, i / w))
            .collect();
        lit.sort_by_key(|&(x, y)| (y, x));
        let mut want = block(0, 4, 0, 2);
        want.extend(block(600, 800, 200, 202));
        assert_eq!(lit, want);
        let imgs = t.images();
        let (_, p) = imgs.placements().next().unwrap();
        assert_eq!(imgs.lit(p.image), u32::try_from(lit.len()).ok());
    });
}

/// What is drawn while magnified is drawn on the bitmap, shown magnified;
/// and a scroll moves the bitmap through the window, "S(S) and S[] then
/// combine to move a small window through a large display space".
#[test]
fn drawing_and_scrolling_while_magnified_work_on_the_bitmap() {
    assert_eq!(
        sorted(lit(b"\x1bPpS(S2)P[10,10]V[20,10]\x1b\\")),
        block(20, 42, 20, 22)
    );
    assert_eq!(
        lit(b"\x1bPpS(S2)P[10,10]V[20,10]S(S1)\x1b\\"),
        row(10, 10..=20)
    );
    // [500,100] is past the window at twice; three hundred units on, it
    // is in it.
    assert_eq!(lit(b"\x1bPpP[500,100]V[510,100]S(S2)\x1b\\"), vec![]);
    assert_eq!(
        sorted(lit(b"\x1bPpP[500,100]V[510,100]S(S2)S[+300]\x1b\\")),
        block(400, 422, 200, 202)
    );
}

/// "Negative values for scaling factors are not allowed. A value which
/// exceeds the implementation limit in a device is mapped to the
/// implementation limit" -- sixteen -- and "The default value for
/// scaling factors is 1, and is restored by the execution of a valid
/// S(A)".
#[test]
fn the_limit_the_refusals_and_s_a() {
    assert_eq!(
        sorted(lit(b"\x1bPpP[10,10]V[11,10]S(S40)\x1b\\")),
        block(160, 192, 160, 176)
    );
    let two = sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S2)\x1b\\"));
    assert_eq!(
        sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S2)S(S-1)\x1b\\")),
        two
    );
    assert_eq!(
        sorted(lit(b"\x1bPpP[100,100]V[110,100]S(S2)S(S0)\x1b\\")),
        two
    );
    assert_eq!(
        lit(b"\x1bPpP[100,100]V[110,100]S(S2)S(A[0,0][799,479])\x1b\\"),
        row(100, 100..=110)
    );
    run(b"\x1bPpS(S2)S(S(X3)(Y2))\x1b\\", |t| {
        assert_eq!(t.regis_counts().unbuilt, 0)
    });
}

/// **A magnified page stays the bitmap shown magnified** through what the
/// terminal does to it: text written over it and erases clear the bitmap
/// under the cells they cover, a line's move and the window's move take
/// the bitmap's rows with the page's. After each, the lit count is a
/// recount, and repainting the whole page from the bitmap (a map entry no
/// pixel holds changed) changes nothing shown.
#[test]
fn a_magnified_page_stays_one_with_its_bitmap() {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
    let steps: &[&[u8]] = &[
        b"\x1bPpF(V[0,0][300,0][300,200][0,200])S(S2)\x1b\\",
        b"\x1b[2;3HHello",
        b"\x1b[4;1H\x1b[K",
        b"\x1b[1;5H\x1b[3X",
        b"\x1b[2;1H\x1b[M",
        b"\x1b[1;1H\x1b[L",
        b"\x1b[24;1H\n\x1bPpS(S3)P[200,100]V[201,100]\x1b\\",
        b"\x1b[2;1H\x1b[L",
        // Rows of the bitmap past the window come into it.
        b"\x1b[2;1H\x1b[2M",
    ];
    let shown = |t: &Term<'_>| -> (Vec<u32>, usize, Option<u32>) {
        let imgs = t.images();
        let (_, p) = imgs.placements().last().expect("a page");
        let v = imgs.image(p.image).expect("its image");
        let px: Vec<u32> = v.pixels.iter().map(|px| px.bits()).collect();
        let n = px.iter().filter(|&&c| c != Pixel::CLEAR.bits()).count();
        (px, n, imgs.lit(p.image))
    };
    for (i, step) in steps.iter().enumerate() {
        support::feed(&mut t, step);
        let (before, n, kept) = shown(&t);
        assert!(n > 0, "after step {i}: something lit");
        assert_eq!(kept, u32::try_from(n).ok(), "after step {i}: the lit count");
        support::feed(&mut t, b"\x1bPpS(M3(R))\x1b\\");
        let (after, _, _) = shown(&t);
        let differ = before.iter().zip(&after).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "after step {i}: the page left its bitmap");
    }
}

/// **A magnified page moves with its lines**: a line inserted above moves
/// the drawing down a line, the window moving down a line carries it up
/// one, the bitmap's rows with it.
#[test]
fn a_magnified_page_moves_with_its_lines() {
    let ws = WinSize {
        ws_row: 24,
        ws_col: 80,
    };
    // Device rows 20 to 29, shown twice over: page rows 40 to 59, the
    // text's third line.
    let cases: [(&[u8], std::ops::Range<usize>); 3] = [
        (b"", 40..60),
        (b"\x1b[1;1H\x1b[L", 60..80),
        (b"\x1b[24;1H\n\x1bPpP[390,230]V[390,230]\x1b\\", 20..40),
    ];
    for (moves, rows) in cases {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(8 << 20));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
        support::feed(&mut t, b"\x1bPpF(V[0,20][50,20][50,30][0,30])S(S2)\x1b\\");
        support::feed(&mut t, moves);
        let imgs = t.images();
        let (_, p) = imgs.placements().last().expect("a page");
        let v = imgs.image(p.image).expect("its image");
        let w = v.width.index();
        let gray: Vec<usize> = (0..v.height.index())
            .filter(|&y| v.pixels[y * w + 10].bits() == GRAY)
            .collect();
        assert_eq!(
            gray,
            rows.collect::<Vec<_>>(),
            "{:?}",
            String::from_utf8_lossy(moves)
        );
    }
}

/// The pointer on a magnified page names the bitmap's pixel under it.
#[test]
fn the_input_position_is_the_bitmap_s() {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_hold(RegisHold::On);
    support::feed(&mut t, b"\x1bPpS(S2)R(I1)");
    t.clear_reply();
    t.locator(LocatorInput {
        kind: LocatorKind::Press(LocatorButton::Left),
        at: Some(LocatorAt {
            col: Col::ZERO,
            row: Row::ZERO,
            px: Some(PointerPx {
                x: baud_domain::ImagePx::of(200),
                y: baud_domain::ImagePx::of(100),
            }),
        }),
        held: LocatorButtons::NONE.pressed(LocatorButton::Left),
    });
    assert_eq!(t.reply(), b"\x1b[241~[100,50]\r");
}

/// A stand-in for the embedder's font: every character a box round its
/// cell's edge, so where and how large it is drawn can be read back.
fn boxes(_: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    let (w, h) = (w.raw(), h.raw());
    for y in 0..h {
        for x in 0..w {
            if x == 0 || y == 0 || x + 1 == w || y + 1 == h {
                set(ImagePx::of(x), ImagePx::of(y));
            }
        }
    }
}

/// `reads` fed one after another to a terminal with the boxes font, then
/// `f`.
fn run_font(reads: &[&[u8]], f: impl FnOnce(&mut Term<'_>)) {
    let ws = support::CORPUS;
    let mut owned = Owned::new(&ws).with_images(ImageBudget::of(4 << 20));
    let mut t = Term::new(owned.storage(), &ws);
    t.set_regis_font(boxes);
    for r in reads {
        support::feed(&mut t, r);
    }
    f(&mut t);
}

/// The command display's line, the device's bottom twenty rows: its
/// pixels as `(x, y, colour)`, clear ones left out.
fn bottom_line(t: &Term<'_>) -> Vec<(usize, usize, u32)> {
    let (px, w) = page(t);
    (460..480)
        .flat_map(|y| (0..800).map(move |x| (x, y)))
        .filter(|&(x, y)| px[y * w + x] != CLEAR)
        .map(|(x, y)| (x, y, px[y * w + x]))
        .collect()
}

/// The boxes of columns `cols` on the command display's line, ten device
/// pixels by twenty each, in `colour`.
fn line_boxes(cols: std::ops::Range<usize>, colour: u32) -> Vec<(usize, usize, u32)> {
    let mut v: Vec<(usize, usize, u32)> = (460..480)
        .flat_map(|y| (cols.start * 10..cols.end * 10).map(move |x| (x, y)))
        .filter(|&(x, y)| x % 10 == 0 || x % 10 == 9 || y == 460 || y == 479)
        .map(|(x, y)| (x, y, colour))
        .collect();
    v.sort_by_key(|&(x, y, _)| (y, x));
    v
}

/// **Magnified, the command display stays on the screen's bottom line**,
/// its characters the size they are unmagnified: "The last line of ReGIS
/// commands received appears on the bottom line of the screen", and "The
/// appearance of the characters is based only on the output map settings
/// and the image that the characters are drawn over. Other ReGIS-settable
/// states have no effect" (`[[source: vt3xx-regis-intro]]`).
#[test]
fn magnified_the_command_display_stays_on_the_bottom_line() {
    // Unmagnified: `S(S1)P[1,1]`, eleven characters, eleven boxes.
    run_font(&[b"\x1bP3pS(S1)P[1,1]\x1b\\"], |t| {
        assert_eq!(bottom_line(t), line_boxes(0..11, GRAY));
    });
    // Magnified twice: the same eleven boxes on the same line.
    run_font(&[b"\x1bP3pS(S2)P[1,1]\x1b\\"], |t| {
        assert_eq!(bottom_line(t), line_boxes(0..11, GRAY));
    });
    // And sixteen times.
    run_font(&[b"\x1bP3pS(S16)P[1,1]\x1b\\"], |t| {
        assert_eq!(bottom_line(t), line_boxes(0..12, GRAY));
    });
    // With no font each character is counted once, magnified or not.
    run(b"\x1bP2pS(S2)P\x1b\\", |t| {
        assert_eq!(t.regis_counts().unfonted, 6);
    });
}

/// "A line feed character makes the terminal erase the ReGIS command
/// display line, along with any graphic image previously drawn there":
/// magnified, the drawing shown on the line is the plane's rows the scale
/// shows there.
#[test]
fn magnified_a_line_feed_erases_what_the_bottom_line_shows() {
    // Plane row 235, shown twice over at page rows 470 and 471.
    let draw: &[u8] = b"\x1bPpS(S2)W(I2)P[0,235]V[399,235]\x1b\\";
    run_font(&[draw], |t| {
        let line = bottom_line(t);
        assert_eq!(line.len(), 2 * 800);
        assert!(line.iter().all(|&(_, y, _)| y == 470 || y == 471));
    });
    run_font(&[draw, b"\x1bP2p\n\x1b\\"], |t| {
        let (px, w) = page(t);
        assert!((460..480).all(|y| (0..800).all(|x| px[y * w + x] == BLACK)));
    });
}

/// A line feed takes the magnified line's characters with it: after
/// `P[1,1]` puts six on the line and the line feed erases it, what is not
/// the erase's black is the boxes of the six drawn after it, and none of
/// the old line's.
#[test]
fn magnified_a_line_feed_clears_the_line_s_characters() {
    run_font(&[b"\x1bP3pS(S2)P[1,1]\nP[2,2]\x1b\\"], |t| {
        let grey: Vec<_> = bottom_line(t)
            .into_iter()
            .filter(|p| p.2 != BLACK)
            .collect();
        assert_eq!(grey, line_boxes(0..6, GRAY));
    });
}

/// The magnified line's characters stay over what is drawn after them,
/// recolour with the output map, and go with the magnification, the plane
/// holding only those drawn unmagnified.
#[test]
fn the_magnified_line_stays_over_later_drawing_and_goes_with_the_scale() {
    let line: &[u8] = b"\x1bP3pS(S2)P[1,1]\x1b\\";
    run_font(&[line, b"\x1bPpW(I2)P[0,235]V[399,235]\x1b\\"], |t| {
        let (px, w) = page(t);
        // A box's left edge over the drawing, the drawing in its middle.
        assert_eq!(px[470 * w], GRAY);
        assert_ne!(px[470 * w + 5], GRAY);
        assert_ne!(px[470 * w + 5], CLEAR);
        assert_eq!(px[470 * w + 200], px[470 * w + 5]);
    });
    run_font(&[line, b"\x1bPpP[10,10]V[20,10]S(M7(R))\x1b\\"], |t| {
        let (px, w) = page(t);
        let red = px[20 * w + 20];
        assert_ne!(red, GRAY);
        assert_eq!(bottom_line(t), line_boxes(0..11, red));
    });
    // `S(S2)` drawn unmagnified, the five characters before the scale.
    run_font(&[line, b"\x1bPpS(S1)\x1b\\"], |t| {
        assert_eq!(bottom_line(t), line_boxes(0..5, GRAY));
    });
    // `S(E)` erases them with the rest of the screen.
    run_font(&[line, b"\x1bPpS(E)\x1b\\"], |t| {
        let (px, w) = page(t);
        assert!((460..480).all(|y| (0..800).all(|x| px[y * w + x] == BLACK)));
    });
    // The output cursor drawn over the line while the envelope waits, at
    // plane pixel 3,233, and taken away at `ST`: the characters under it
    // drawn again.
    run_font(&[b"\x1bP3pS(S2)P[1,1]P[3,233]"], |t| {
        assert_ne!(bottom_line(t), line_boxes(0..19, GRAY), "the cursor");
        support::feed(t, b"\x1b\\");
        assert_eq!(bottom_line(t), line_boxes(0..19, GRAY));
    });
}
