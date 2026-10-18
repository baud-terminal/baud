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
use baud_render::{Blend, Damage, Metrics, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 1,
    ws_col: 8,
};

/// The canvas after `bytes` at `size` px, its width, and the metrics.
fn painted(bytes: &[u8], size: f32) -> (Vec<u32>, usize, Metrics) {
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

/// **The separators the mark skips are the Unicode Standard's Z\*
/// classes**, as `[[source: gc-data]]` lists them:
/// a hand-written range that drifted from the file would mark a space or
/// leave a letter bare.
#[test]
fn the_stress_mark_skips_exactly_the_separator_classes() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-derivedgeneralcategory.txt"
    );
    let ucd = std::fs::read_to_string(path).expect("the pinned UCD file");
    let mut want: Vec<(u32, u32)> = ucd
        .lines()
        .filter_map(|l| {
            let (field, rest) = l.split_once(';')?;
            let class = rest.split('#').next()?.trim();
            if !matches!(class, "Zs" | "Zl" | "Zp") {
                return None;
            }
            let field = field.trim();
            let (a, b) = field.split_once("..").unwrap_or((field, field));
            let hex = |s: &str| u32::from_str_radix(s, 16).expect("a hex codepoint");
            Some((hex(a), hex(b)))
        })
        .collect();
    want.sort_unstable();
    // Adjacent ranges, Zl's U+2028 beside Zp's U+2029, are one range.
    want.dedup_by(|b, a| {
        let joins = b.0 == a.1 + 1;
        if joins {
            a.1 = b.1;
        }
        joins
    });
    assert!(!want.is_empty(), "the positive control: the file names Z*");
    assert_eq!(baud_render::SEPARATORS.to_vec(), want);
    use baud_domain::{Codepoint, Content};
    for (cp, marks) in [
        (u32::from('o'), true),
        (0x4e2d, true),
        (0x20, false),
        (0x3000, false),
        (0x2029, false),
    ] {
        assert_eq!(
            baud_render::takes_stress_mark(Content::codepoint(Codepoint::new(cp).unwrap())),
            marks,
            "U+{cp:04X}"
        );
    }
    assert!(
        !baud_render::takes_stress_mark(Content::EMPTY),
        "a blank cell"
    );
    assert!(
        !baud_render::takes_stress_mark(Content::WIDE_TAIL),
        "a wide tail"
    );
}
