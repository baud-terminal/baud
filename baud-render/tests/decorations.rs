//! `SGR 53` overlined, `51` framed, the ideogram lines `60-63` and `64`'s
//! stress mark reach the pixels, and their cancels take them away.
//!
//! `[[source: ecma48]]` § 8.3.117 names them: "51 framed", "53 overlined",
//! "54 not framed, not encircled", "55 not overlined", and "60 ideogram
//! underline or right side line", "61 ideogram double underline or double
//! line on the right side", "62 ideogram overline or left side line", "63
//! ideogram double overline or double line on the left side", "65 cancels
//! the effect of the rendition aspects established by parameter values 60
//! to 64". baud's text is horizontal, so the under and over lines are the
//! ones drawn. Every line's scene is spaces, so a lit pixel is a
//! decoration and never a glyph; the stress mark's scenes are text,
//! compared with the same text unmarked.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Blink, BlinkPhases, Damage, Metrics, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 1,
    ws_col: 8,
};

/// The canvas after `bytes` at `size` px, its width, and the metrics.
fn painted(bytes: &[u8], size: f32) -> (Vec<u32>, usize, Metrics) {
    painted_in(bytes, size, BlinkPhases::SHOWN)
}

/// [`painted`] in the blink phases `blink`.
fn painted_in(bytes: &[u8], size: f32, blink: BlinkPhases) -> (Vec<u32>, usize, Metrics) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(size).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let all = [b"\x1b[?25l".as_slice(), bytes].concat();
    t.write_at(BytePos::ZERO, &all);
    t.feed(BytePos::ZERO, ReadLen::from_index(all.len()));
    r.set_blink(blink);
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let m = r.metrics();
    let w = c.width;
    (c.pixels, w, m)
}

fn px(v: baud_font::Px) -> usize {
    usize::try_from(v.value()).unwrap()
}

/// Whether the pixel at `(x, y)` is lit, that is not the default background.
fn lit(p: &[u32], w: usize, x: usize, y: usize) -> bool {
    p[y * w + x] != Theme::dark().bg.0
}

/// The rows of pixels lit across the whole of columns `c0..c1`.
fn full_rows(p: &[u32], w: usize, m: Metrics, c0: usize, c1: usize) -> Vec<usize> {
    let cw = px(m.cell_w);
    (0..px(m.cell_h))
        .filter(|&y| (c0 * cw..c1 * cw).all(|x| lit(p, w, x, y)))
        .collect()
}

/// **An overline is a line across the top of every cell it covers**, one
/// underline thickness tall, and nothing else of those cells is lit; the
/// cells after `55` carry none. The top of the cell, as ghostty places it
/// (`[[source: ghostty-metrics-d9840f3]]`, `overline_position = 0`).
#[test]
fn an_overline_is_a_line_across_the_top_of_its_cells() {
    let (p, w, m) = painted(b"\x1b[53m   \x1b[55m   ", 14.0);
    let th = px(m.thickness).max(1);
    assert_eq!(
        full_rows(&p, w, m, 0, 3),
        (0..th).collect::<Vec<_>>(),
        "the top {th} rows of the three overlined cells, and only they"
    );
    let cw = px(m.cell_w);
    let after = (3 * cw..6 * cw).any(|x| (0..px(m.cell_h)).any(|y| lit(&p, w, x, y)));
    assert!(!after, "55 cleared it: nothing is lit after it");
}

/// **The overline is drawn in the text's colour, as the strike is, and
/// not in `SGR 58`'s.** `[[source: kitty-underlines]]` defines 58 as "the
/// underline color", and an overline is not an underline; ghostty draws
/// its overline in the foreground (`[[source: ghostty-generic-d9840f3]]`,
/// `addOverline(.., fg, ..)`).
#[test]
fn the_overline_is_the_text_colour_and_not_the_underline_colour() {
    let sgr = b"\x1b[38;2;40;220;90;58;2;230;30;30m";
    let (over, w, m) = painted(&[sgr.as_slice(), b"\x1b[53m  "].concat(), 14.0);
    let (strike, ..) = painted(&[sgr.as_slice(), b"\x1b[9m  "].concat(), 14.0);
    let (under, ..) = painted(&[sgr.as_slice(), b"\x1b[4m  "].concat(), 14.0);
    let text = strike[px(m.strike) * w];
    let underline = under[px(m.underline) * w];
    assert_ne!(text, underline, "the premise: the two colours differ");
    assert_eq!(over[0], text, "the overline takes the strike's colour");
}

/// **`60` is an underline and `62` an overline**, in horizontal text: the
/// same pixels as `4` and `53`, and different from each other.
#[test]
fn ideogram_lines_are_the_under_and_over_lines_of_horizontal_text() {
    let (i60, ..) = painted(b"\x1b[60m  ", 14.0);
    let (i62, ..) = painted(b"\x1b[62m  ", 14.0);
    let (u4, ..) = painted(b"\x1b[4m  ", 14.0);
    let (o53, ..) = painted(b"\x1b[53m  ", 14.0);
    let (none, ..) = painted(b"  ", 14.0);
    assert!(i60 != none, "60 is drawn");
    assert!(i60 == u4, "60 draws what 4 draws");
    assert!(i62 == o53, "62 draws what 53 draws");
    assert!(i60 != i62, "and the two are different lines");
}

/// **`61` and `63` are double lines**: 61 the double underline `21` draws,
/// 63 two lines at the top, one thickness and a pixel apart. At 28 px,
/// where the cell has room for the gap to show.
#[test]
fn ideogram_double_lines_are_two_lines() {
    let (i61, ..) = painted(b"\x1b[61m  ", 28.0);
    let (d21, ..) = painted(b"\x1b[21m  ", 28.0);
    assert!(i61 == d21, "61 draws what 21 draws");
    let (i63, w, m) = painted(b"\x1b[63m  ", 28.0);
    let th = px(m.thickness).max(1);
    let gap = th + 1;
    let want: Vec<usize> = (0..th).chain(gap..gap + th).collect();
    assert_eq!(
        full_rows(&i63, w, m, 0, 2),
        want,
        "two lines at the top, {th} thick, the second at {gap}"
    );
}

/// **`65` cancels every ideogram line, and `55` the overline alone.**
#[test]
fn sixty_five_and_fifty_five_cancel_their_lines() {
    let (none, ..) = painted(b"  ", 14.0);
    assert!(
        painted(b"\x1b[53m  ", 14.0).0 != none,
        "the premise: 53 is drawn"
    );
    for (bytes, name) in [
        (&b"\x1b[60;61;62;63;65m  "[..], "65 after 60-63"),
        (b"\x1b[53;55m  ", "55 after 53"),
        (b"\x1b[51;54m  ", "54 after 51"),
        (b"\x1b[52;54m  ", "54 after 52"),
    ] {
        assert!(painted(bytes, 14.0).0 == none, "{name} leaves nothing");
    }
    let (o, ..) = painted(b"\x1b[62;53;55m  ", 14.0);
    let (o53, ..) = painted(b"\x1b[53m  ", 14.0);
    assert!(o == o53, "55 leaves 62's overline");
}

/// **A framed run is one box**: a line across the top and the bottom of
/// every framed cell, a side at the first cell's left and the last cell's
/// right, and nothing between two framed cells.
#[test]
fn a_framed_run_is_one_box() {
    let (p, w, m) = painted(b" \x1b[51m   \x1b[54m  ", 14.0);
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    let th = px(m.thickness).max(1);
    let want: Vec<usize> = (0..th).chain(ch - th..ch).collect();
    assert_eq!(
        full_rows(&p, w, m, 1, 4),
        want,
        "top and bottom of the run's three cells"
    );
    let column = |x: usize| (0..ch).all(|y| lit(&p, w, x, y));
    assert!(
        (cw..cw + th).all(column),
        "the left side, at the first cell"
    );
    assert!(
        (4 * cw - th..4 * cw).all(column),
        "the right side, at the last cell"
    );
    let mid = ch / 2;
    for edge in [2 * cw, 3 * cw] {
        assert!(
            !(edge - th..edge + th).any(|x| lit(&p, w, x, mid)),
            "nothing between two framed cells at x {edge}"
        );
    }
    let outside = (0..cw)
        .chain(4 * cw..8 * cw)
        .any(|x| (0..ch).any(|y| lit(&p, w, x, y)));
    assert!(!outside, "nothing outside the run");
}

/// The coverage of `ring`'s mask at the metrics' cell, as the renderer
/// draws it into the atlas.
fn ring_mask(m: Metrics, ring: baud_render::boxes::Ring) -> Vec<u8> {
    let cell = baud_render::boxes::Cell {
        w: m.cell_w,
        h: m.cell_h,
    };
    let th = baud_font::Px::of(m.thickness.value().max(1));
    let (mut scratch, mut cov) = (Vec::new(), Vec::new());
    baud_render::boxes::draw_ring(ring, cell, th, &mut scratch, &mut cov).expect("a mask");
    cov
}

/// Whether cell `col` of the canvas is lit exactly where `mask` covers:
/// dark where it is zero, lit where it is at least half.
fn follows(p: &[u32], w: usize, m: Metrics, col: usize, mask: &[u8]) -> bool {
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    (0..ch).all(|y| {
        (0..cw).all(|x| {
            let c = mask[y * cw + x];
            let on = lit(p, w, col * cw + x, y);
            (c != 0 || !on) && (c < 128 || on)
        })
    })
}

/// **An encircled run is one enclosure**: `[[source: ecma48]]` § 8.3.117
/// says only "52 encircled", and baud draws it as `SGR 51`'s frame is
/// drawn, one outline a run -- the frame's top and bottom lines across
/// the cells between the ends, and at each end a side between two
/// rounded corners: a lone cell is a stadium; a run of two cells is a
/// rectangle with rounded corners of radius half a cell's width.
/// In the text's colour, as the frame is.
#[test]
fn an_encircled_run_is_one_enclosure() {
    use baud_render::boxes::Ring;
    let (p, w, m) = painted(b" \x1b[52m   \x1b[54m  ", 14.0);
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    let th = px(m.thickness).max(1);
    let want: Vec<usize> = (0..th).chain(ch - th..ch).collect();
    assert_eq!(
        full_rows(&p, w, m, 2, 3),
        want,
        "top and bottom of the cell between the ends"
    );
    assert!(
        follows(&p, w, m, 1, &ring_mask(m, Ring::Starts)),
        "the left end"
    );
    assert!(
        follows(&p, w, m, 3, &ring_mask(m, Ring::Ends)),
        "the right end"
    );
    let outside = (0..cw)
        .chain(4 * cw..8 * cw)
        .any(|x| (0..ch).any(|y| lit(&p, w, x, y)));
    assert!(!outside, "nothing outside the run");
    let (alone, ..) = painted(b" \x1b[52m \x1b[54m", 14.0);
    assert!(
        follows(&alone, w, m, 1, &ring_mask(m, Ring::Alone)),
        "a lone cell"
    );
    let (two, ..) = painted(b" \x1b[52m  \x1b[54m", 14.0);
    let mirrored =
        (0..ch).all(|y| (0..2 * cw).all(|x| two[y * w + cw + x] == two[y * w + 3 * cw - 1 - x]));
    assert!(
        mirrored,
        "a two-cell run is one rounded rectangle, its halves mirrored"
    );
    assert!(
        (0..ch).any(|y| lit(&two, w, cw, y)) && (0..ch).any(|y| lit(&two, w, 3 * cw - 1, y)),
        "the premise: the two-cell run is drawn"
    );
    let sgr = b"\x1b[38;2;40;220;90;58;2;230;30;30m";
    let (ring, ..) = painted(&[sgr.as_slice(), b"\x1b[52m   "].concat(), 14.0);
    let (strike, ..) = painted(&[sgr.as_slice(), b"\x1b[9m   "].concat(), 14.0);
    assert_eq!(
        ring[cw + cw / 2],
        strike[px(m.strike) * w + cw],
        "the line between the ends is the text colour"
    );
}

/// **An encircled run's ends keep to the corners**, at 9-13 px and 18-26
/// px, 9-13 pt at 1x and 2x: between the rounded corners an end is the
/// stroke at the cell's outer edge and nothing further in, and its corner
/// is round, not the frame's square. A half ellipse filling the end cell
/// crossed most end glyphs' ink at every one of those sizes; The rounded
/// corners are chosen over the ellipse and over the frame's square box.
#[test]
fn an_encircled_end_keeps_to_its_corners() {
    use baud_render::boxes::Ring;
    for size in [9.0, 10.0, 11.0, 12.0, 13.0, 18.0, 20.0, 22.0, 24.0, 26.0] {
        let (_, _, m) = painted(b"", size);
        let (cw, ch) = (px(m.cell_w), px(m.cell_h));
        let th = px(m.thickness).max(1);
        // The rows between the corners: below the top corner's radius,
        // half the cell's width, and above the bottom one's. The side is
        // whole there, but for where a corner's curve meets it.
        let r = cw.div_ceil(2);
        for (ring, sides) in [
            (Ring::Starts, [0..th, 0..0]),
            (Ring::Ends, [cw - th..cw, 0..0]),
            (Ring::Alone, [0..th, cw - th..cw]),
        ] {
            let mask = ring_mask(m, ring);
            for y in r..ch - r {
                for x in 0..cw {
                    let side = sides.iter().any(|s| s.contains(&x));
                    let c = mask[y * cw + x];
                    assert!(
                        if side { c >= 250 } else { c == 0 },
                        "{size} px, {ring:?}: row {y}, column {x} covers {c}"
                    );
                }
            }
            let corner = if ring == Ring::Ends { cw - 1 } else { 0 };
            assert!(
                mask[corner] < 255,
                "{size} px, {ring:?}: the corner is round"
            );
        }
    }
}

/// **An encircled run closes beside hidden text**, as the frame does.
#[test]
fn an_encircled_run_closes_beside_a_hidden_cell() {
    let (got, ..) = painted(b" \x1b[52mab\x1b[8mcd", 14.0);
    let (want, ..) = painted(b" \x1b[52mab\x1b[0;8mcd", 14.0);
    let (open, ..) = painted(b" \x1b[52mabcd", 14.0);
    let (bare, ..) = painted(b" ab\x1b[8mcd", 14.0);
    assert!(want != open, "the premise: the hidden cells hide something");
    assert!(got != bare, "the premise: the enclosure is drawn");
    assert!(
        got == want,
        "the visible cells close the enclosure at the seam"
    );
}

/// **A framed run closes beside a framed cell whose text is hidden**: a
/// concealed cell (`SGR 8`, "concealed characters", `[[source: ecma48]]`
/// § 8.3.117) or one in its blink's off phase draws its background and
/// nothing of its own, frame included, so the visible cells draw a side
/// at the seam -- the same pixels as the hidden cells never framed.
#[test]
fn a_framed_run_closes_beside_a_hidden_cell() {
    let off = BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Hidden,
    };
    let shown = BlinkPhases::SHOWN;
    let mut open_at = Vec::new();
    for (seam, closed, blink, name) in [
        (
            " \x1b[51mab\x1b[8mcd",
            " \x1b[51mab\x1b[0;8mcd",
            shown,
            "concealed after",
        ),
        (
            " \x1b[51;8mab\x1b[28mcd",
            " \x1b[8mab\x1b[0;51mcd",
            shown,
            "concealed before",
        ),
        (
            " \x1b[51mab\x1b[5mcd",
            " \x1b[51mab\x1b[0;5mcd",
            off,
            "slow blink off",
        ),
        (
            " \x1b[51mab\x1b[6mcd",
            " \x1b[51mab\x1b[0;6mcd",
            off,
            "rapid blink off",
        ),
    ] {
        let (got, ..) = painted_in(seam.as_bytes(), 14.0, blink);
        let (want, ..) = painted_in(closed.as_bytes(), 14.0, blink);
        let (open, ..) = painted_in(b" \x1b[51mabcd", 14.0, blink);
        assert!(
            want != open,
            "{name}: the premise, the hidden cells hide something"
        );
        if got != want {
            open_at.push(name);
        }
    }
    assert!(
        open_at.is_empty(),
        "the visible cells leave the box open at the seam: {open_at:?}"
    );
}

/// The top-left corner and the side of the stress mark over a character
/// that starts in column `col` and spans `span` cells: a filled square two
/// line thicknesses on a side, centred across the character, one side's
/// length below the top of the cell and clamped inside it.
fn stress_dot(m: Metrics, col: usize, span: usize) -> (usize, usize, usize) {
    let (cw, ch) = (px(m.cell_w), px(m.cell_h));
    let d = 2 * px(m.thickness).max(1);
    let x = col * cw + (span * cw).saturating_sub(d) / 2;
    (x, d.min(ch.saturating_sub(d)), d)
}

/// **`SGR 64` puts a filled dot over each character, in the text colour,
/// and none over a separator.** `[[source: ecma48]]` § 8.3.117 names "64
/// ideogram stress marking" and gives no shape or side; the web's
/// emphasis marks (`[[source: css-text-decor-3]]`) give text with no
/// language both: `text-emphasis-position` "Initial: over right", a
/// filled dot "U+2022", `text-emphasis-color` "Initial: currentcolor",
/// "drawn once for each typographic character unit", and "not drawn for
/// ... the Unicode separator classes (Z*)". So a wide character takes one
/// mark centred across its two cells, and a space and an ideographic space
/// take none. `SGR 58` is the underline colour and does not reach it.
#[test]
fn a_stress_mark_is_a_dot_over_each_character() {
    let sgr = "\x1b[38;2;40;220;90;58;2;230;30;30m";
    let text = "ox \u{3000}\u{4e2d}";
    let (marked, w, m) = painted(format!("{sgr}\x1b[64m{text}").as_bytes(), 14.0);
    let (plain, ..) = painted(format!("{sgr}{text}").as_bytes(), 14.0);
    let fg = 0xff28_dc5a;
    let dots = [
        stress_dot(m, 0, 1),
        stress_dot(m, 1, 1),
        stress_dot(m, 5, 2),
    ];
    let inside = |x: usize, y: usize| {
        dots.iter()
            .any(|&(x0, y0, d)| (x0..x0 + d).contains(&x) && (y0..y0 + d).contains(&y))
    };
    for y in 0..px(m.cell_h) {
        for x in 0..w {
            let i = y * w + x;
            if inside(x, y) {
                assert_eq!(marked[i], fg, "the mark at ({x}, {y}) is the text colour");
            } else {
                assert_eq!(marked[i], plain[i], "nothing else changes, at ({x}, {y})");
            }
        }
    }
    assert!(marked != plain, "the premise: the marks are drawn");
}

/// **`65` cancels the stress mark**, as it cancels "the rendition aspects
/// established by parameter values 60 to 64".
#[test]
fn sixty_five_cancels_the_stress_mark() {
    let (none, ..) = painted(b"ox", 14.0);
    let (marked, ..) = painted(b"\x1b[64mox", 14.0);
    assert!(marked != none, "the premise: 64 is drawn");
    assert!(painted(b"\x1b[64;65mox", 14.0).0 == none, "65 after 64");
}

/// **The mark skips what the web's emphasis marks leave bare**, at every
/// codepoint: `[[source: css-text-decor-3]]` draws none for "the Unicode
/// separator classes (Z*)", for "any characters that belong to the
/// Unicode P* general category and do not NFKD normalize [UAX15] to any
/// of the following symbols" -- `#`, `%`, U+2030, U+2031, U+066A,
/// U+0609, U+060A, `&`, U+204A, `@`, U+00A7, U+00B6, U+204B, U+2053,
/// U+303D -- and for "the Unicode classes for control codes and
/// unassigned characters (Cc, Cf, Cn)". The categories are
/// `[[source: gc-data]]`'s; NFKD of one codepoint is its compatibility
/// decomposition (`[[source: unicode17-ch3]]` D65), `[[source:
/// unicode-data]]`'s mappings applied until nothing decomposes. This is
/// the set computed a second way, beside the generator's.
#[test]
fn the_stress_mark_skips_what_css_leaves_bare() {
    use baud_domain::{Codepoint, Content};
    use std::collections::HashMap;
    let read = |f: &str| {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources/");
        std::fs::read_to_string(format!("{dir}{f}")).expect("a pinned UCD file")
    };
    let hex = |s: &str| u32::from_str_radix(s.trim(), 16).expect("a hex codepoint");
    let mut gc: HashMap<u32, String> = HashMap::new();
    for l in read("unicode-derivedgeneralcategory.txt").lines() {
        let l = l.split('#').next().unwrap_or("");
        let Some((field, class)) = l.split_once(';') else {
            continue;
        };
        let field = field.trim();
        let (a, b) = field.split_once("..").unwrap_or((field, field));
        for cp in hex(a)..=hex(b) {
            gc.insert(cp, class.trim().to_string());
        }
    }
    assert_eq!(gc.len(), 0x11_0000, "the file lists every codepoint");
    let mut dec: HashMap<u32, Vec<u32>> = HashMap::new();
    for l in read("unicode-unicodedata.txt").lines() {
        let f: Vec<&str> = l.split(';').collect();
        if f[5].is_empty() {
            continue;
        }
        let to = f[5]
            .split_whitespace()
            .filter(|t| !t.starts_with('<'))
            .map(hex)
            .collect();
        dec.insert(hex(f[0]), to);
    }
    fn nfkd(cp: u32, dec: &HashMap<u32, Vec<u32>>, out: &mut Vec<u32>) {
        match dec.get(&cp) {
            Some(to) => to.iter().for_each(|&c| nfkd(c, dec, out)),
            None => out.push(cp),
        }
    }
    let keeps = [
        0x23, 0x25, 0x2030, 0x2031, 0x66a, 0x609, 0x60a, 0x26, 0x204a, 0x40, 0xa7, 0xb6, 0x204b,
        0x2053, 0x303d,
    ];
    let mut wrong = Vec::new();
    for cp in 0..0x11_0000u32 {
        let class = gc[&cp].as_str();
        let skips = if class.starts_with('Z') || matches!(class, "Cc" | "Cf" | "Cn") {
            true
        } else if class.starts_with('P') {
            let mut n = Vec::new();
            nfkd(cp, &dec, &mut n);
            !(n.len() == 1 && keeps.contains(&n[0]))
        } else {
            false
        };
        let Some(c) = Codepoint::new(cp) else {
            continue;
        };
        if baud_render::takes_stress_mark(Content::codepoint(c), []) == skips {
            wrong.push(cp);
        }
    }
    assert!(
        wrong.is_empty(),
        "{} codepoints marked against the rule, the first U+{:04X}",
        wrong.len(),
        wrong.first().copied().unwrap_or(0)
    );
    for (cp, marks) in [
        (u32::from('o'), true),
        (0x4e2d, true),
        (u32::from('#'), true),
        (0xff03, true),
        (0xe000, true),
        (u32::from(','), false),
        (0x3001, false),
        (0xad, false),
        (0x378, false),
        (0x20, false),
        (0x3000, false),
    ] {
        let c = Content::codepoint(Codepoint::new(cp).unwrap());
        assert_eq!(baud_render::takes_stress_mark(c, []), marks, "U+{cp:04X}");
    }
    assert!(
        !baud_render::takes_stress_mark(Content::EMPTY, []),
        "a blank cell"
    );
    assert!(
        !baud_render::takes_stress_mark(Content::WIDE_TAIL, []),
        "a wide tail"
    );
    // A cluster is classed by its base, and a separator base takes the
    // mark only with a combining character -- gc M* (`[[source:
    // unicode17-ch3]]` D52) -- attached: every codepoint as the one mark
    // on a space, against the categories above.
    let space = Content::codepoint(Codepoint::new(0x20).unwrap());
    let mut wrong = Vec::new();
    for cp in 0..0x11_0000u32 {
        if baud_render::takes_stress_mark(space, [cp]) != gc[&cp].starts_with('M') {
            wrong.push(cp);
        }
    }
    assert!(
        wrong.is_empty(),
        "{} marks on a space judged against D52, the first U+{:04X}",
        wrong.len(),
        wrong.first().copied().unwrap_or(0)
    );
}

/// **A cluster takes the mark as its base character does**: CSS classes
/// a typographic character unit by its base, and its one exception for a
/// bare base is the separators' -- "emphasis marks are drawn for a space
/// that combines with any combining characters" (`[[source:
/// css-text-decor-3]]`). So `e` with an acute accent is marked, a comma
/// with one is not, nor `‼` with its emoji selector (P*, whose NFKD is
/// `!!`, off CSS's list), and a space with an acute accent is marked while
/// a space with only a zero width joiner (Cf, not a combining character)
/// is not. Each is painted with and without `SGR 64`.
#[test]
fn a_cluster_takes_the_mark_as_its_base_does() {
    for (text, marks) in [
        ("e\u{301}", true),
        ("a\u{308}\u{301}", true),
        (",\u{301}", false),
        ("\u{203c}\u{fe0f}", false),
        ("#\u{fe0f}\u{20e3}", true),
        (" \u{301}", true),
        (" \u{200d}", false),
        ("\u{3001}\u{3099}", false),
    ] {
        let (none, ..) = painted(format!("x{text}").as_bytes(), 14.0);
        let (marked, ..) = painted(format!("x\x1b[64m{text}").as_bytes(), 14.0);
        assert_eq!(marked != none, marks, "{text:?}");
    }
    // The controls: each base alone, so the cluster's answer is not the
    // base's by accident where the two differ.
    for (text, marks) in [("e", true), (",", false), ("\u{203c}", false), (" ", false)] {
        let (none, ..) = painted(format!("x{text}").as_bytes(), 14.0);
        let (marked, ..) = painted(format!("x\x1b[64m{text}").as_bytes(), 14.0);
        assert_eq!(marked != none, marks, "{text:?} alone");
    }
}
