//! Double-width and double-height lines, drawn: each character over two
//! columns, and on a double-height pair the top half on the first line and
//! the bottom half on the second (`[[source: dec-std-070]]` § 5.6.1). The
//! glyph is rasterised at the doubled size from its outline, and box drawing
//! drawn at the doubled cell, so a frame of line drawing joins across the
//! pair.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, ReadLen};
use baud_font::Face;
use baud_render::{Damage, Renderer};
use support::*;

fn draw(r: &mut Renderer<'_, '_>, bytes: &[u8]) -> Canvas {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = Canvas::for_renderer(r);
    paint(r, &mut t, &mut c, Damage::Full);
    c
}

/// The box around every pixel that differs from the page, within rows
/// `y0..y1` of pixels: `(x0, y0, x1, y1)`, exclusive at the far ends.
fn ink(c: &Canvas, y0: usize, y1: usize) -> Option<(usize, usize, usize, usize)> {
    let page = c.pixels[c.pixels.len() - 1];
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for y in y0..y1 {
        for x in 0..c.width {
            if c.pixels[y * c.width + x] != page {
                b = Some(match b {
                    None => (x, y, x + 1, y + 1),
                    Some((a, bb, cc, d)) => (a.min(x), bb.min(y), cc.max(x + 1), d.max(y + 1)),
                });
            }
        }
    }
    b
}

/// The columns inked on pixel row `y`.
fn inked(c: &Canvas, y: usize) -> Vec<usize> {
    let page = c.pixels[c.pixels.len() - 1];
    (0..c.width)
        .filter(|&x| c.pixels[y * c.width + x] != page)
        .collect()
}

fn faces() -> Vec<u8> {
    font("JetBrainsMono-Regular.ttf")
}

/// The cell's width and height in pixels, as indices.
fn cell(r: &Renderer<'_, '_>) -> (usize, usize) {
    let m = r.metrics();
    (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    )
}

/// **A double-width line draws its character over two columns, as wide
/// again and as tall**: the `M`'s ink starts in the same place in a box
/// twice the cell's width, and is about twice as wide.
#[test]
fn a_double_width_line_draws_each_character_over_two_columns() {
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let (cw, ch) = cell(&r);
    let one = draw(&mut r, b"\x1b[?25l\x1b[2;3HM");
    let two = draw(&mut r, b"\x1b[?25l\x1b[2;3H\x1b#6M");
    let (a0, ay0, a1, ay1) = ink(&one, ch, 2 * ch).expect("the single M");
    let (b0, by0, b1, by1) = ink(&two, ch, 2 * ch).expect("the double M");
    assert!(
        a0 >= 2 * cw && a1 <= 3 * cw,
        "the single M is in its cell: {a0}..{a1}"
    );
    assert!(
        b0 >= 4 * cw && b1 <= 6 * cw,
        "the double M is in its two: {b0}..{b1}"
    );
    let (w1, w2) = (a1 - a0, b1 - b0);
    assert!(
        w2 + 2 >= 2 * w1 && w2 <= 2 * w1 + 2,
        "twice as wide: {w1} and {w2}"
    );
    assert_eq!((ay0, ay1), (by0, by1), "as tall");
}

/// **A double-height pair draws the top half on the first line and the
/// bottom half on the second**: the `M`'s ink is about twice as tall as
/// the single one's and twice as wide, it crosses the boundary between the
/// two lines, and the strokes meet there.
#[test]
fn a_double_height_pair_draws_the_top_and_the_bottom_half() {
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let (_, ch) = cell(&r);
    let one = draw(&mut r, b"\x1b[?25l\x1b[2;3HM");
    let tall = draw(&mut r, b"\x1b[?25l\x1b[2;3H\x1b#3M\x1b[3;3H\x1b#4M");
    let (_, ay0, _, ay1) = ink(&one, ch, 2 * ch).expect("the single M");
    let (_, by0, _, by1) = ink(&tall, ch, 3 * ch).expect("the tall M");
    let (h1, h2) = (ay1 - ay0, by1 - by0);
    assert!(
        h2 + 2 >= 2 * h1 && h2 <= 2 * h1 + 2,
        "twice as tall: {h1} and {h2}"
    );
    assert!(
        by0 < 2 * ch && by1 > 2 * ch,
        "it crosses the boundary: {by0}..{by1}"
    );
    let (above, below) = (inked(&tall, 2 * ch - 1), inked(&tall, 2 * ch));
    assert!(!above.is_empty(), "the top half reaches the boundary");
    assert_eq!(above, below, "the strokes meet at the boundary");
}

/// **Line drawing joins across a double-height pair**: DEC graphics `x`
/// on both halves draws one vertical stroke, the same columns on either
/// side of the boundary, twice the single stroke's width.
#[test]
fn line_drawing_joins_across_a_double_height_pair() {
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let (_, ch) = cell(&r);
    let one = draw(&mut r, b"\x1b[?25l\x1b(0\x1b[2;3Hx");
    let tall = draw(&mut r, b"\x1b[?25l\x1b(0\x1b[2;3H\x1b#3x\x1b[3;3H\x1b#4x");
    let single = inked(&one, ch + ch / 2);
    let (above, below) = (inked(&tall, 2 * ch - 1), inked(&tall, 2 * ch));
    assert!(!single.is_empty() && !above.is_empty());
    assert_eq!(above, below, "the stroke is unbroken at the boundary");
    assert_eq!(above.len(), 2 * single.len(), "twice the width");
    // And from the top of the pair to its bottom.
    for y in [ch, 3 * ch - 1] {
        assert_eq!(inked(&tall, y), above, "pixel row {y}");
    }
}

/// **A rendition that changes no cell repaints its row**: DECDWL on a line
/// it leaves its cells, then DECSWL, then DECDHL, each an incremental frame
/// equal to a full one, and each a frame unlike the last.
#[test]
fn a_rendition_change_repaints_its_row() {
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut inc = renderer(&faces);
    let mut full = renderer(&faces);
    let mut a = Canvas::for_renderer(&inc);
    let mut b = Canvas::for_renderer(&full);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut was: Option<Vec<u32>> = None;
    for step in [
        "\x1b[?25l\x1b[3;1Hab",
        "\x1b[3;1H\x1b#6",
        "\x1b#5",
        "\x1b#3",
        "\x1b[4;1H\x1b#4ab",
        "\x1b[2J",
    ] {
        t.write_at(BytePos::ZERO, step.as_bytes());
        t.feed(BytePos::ZERO, ReadLen::from_index(step.len()));
        paint(&mut inc, &mut t, &mut a, Damage::Incremental);
        paint(&mut full, &mut t, &mut b, Damage::Full);
        assert!(a.pixels == b.pixels, "{step:?}: incremental against full");
        if let Some(w) = &was {
            assert!(*w != a.pixels, "{step:?}: the frame changed");
        }
        was = Some(a.pixels.clone());
    }
}

/// **vttest's frame of line drawing on double-height pairs joins at every
/// boundary**: replayed from vttest's bytes to the hold where it asks for a
/// frame with blinking bold text in double size, the frame's left side is
/// one stroke from its top corner to its bottom one.
#[test]
fn vttest_s_double_height_frame_is_unbroken() {
    let bytes = std::fs::read(format!(
        "{}/../tests/vttest/doublesize.raw",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the recording");
    let hold = b"all in double-height double-width size. Push <RETURN>";
    let end = bytes
        .windows(hold.len())
        .position(|w| w == hold)
        .expect("the frame's hold")
        + hold.len();
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let (cw, ch) = cell(&r);
    let c = draw(&mut r, &bytes[..end]);
    // The left side, column 1: the top corner's stroke runs down from the
    // middle of its doubled cell -- the boundary between lines 8 and 9 --
    // and the bottom corner's up to the middle of its own, the boundary
    // between lines 16 and 17; between them every row draws the same two
    // cells' worth of stroke.
    let side =
        |y: usize| -> Vec<usize> { inked(&c, y).into_iter().filter(|&x| x < 2 * cw).collect() };
    let at = side(9 * ch + ch / 2);
    assert!(!at.is_empty(), "the side's stroke on line 10");
    // Twice the width of the same stroke on a single-width line.
    let one = draw(&mut r, b"\x1b[?25l\x1b(0\x1b[2;1Hx");
    let single = inked(&one, ch + ch / 2);
    assert_eq!(at.len(), 2 * single.len(), "the side is drawn double");
    for line in 9..16 {
        let y = line * ch;
        assert_eq!(side(y - 1), side(y), "the boundary below line {line}");
        assert_eq!(side(y), at, "line {}", line + 1);
    }
}

/// **A selection on a sized line reads its characters**: the pointer's
/// display column is the line's character under it, half of it on a sized
/// line (`baud_render::char_col`), so a drag over the double-width word's
/// ten columns copies the word, and a word on a single-width line below
/// copies as before.
#[test]
fn a_selection_on_a_sized_line_copies_its_characters() {
    use baud_domain::{Col, Row, Rows};
    use baud_render::{Grain, Scrolled, Selection, Span, char_col};
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = b"\x1b[2;1H\x1b#6hello world\x1b[3;1Hhello world";
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let view = t.viewport();
    let pick = |y: usize, x: usize| {
        let row = Row::from_index(y);
        let col = char_col(&view, row, Col::at_index(x));
        Span::cell(view.row(row), Scrolled::START.line(Rows::ZERO, row), col)
    };
    assert_eq!(
        char_col(&view, Row::from_index(1), Col::at_index(9)),
        Col::at_index(4)
    );
    assert_eq!(
        char_col(&view, Row::from_index(2), Col::at_index(9)),
        Col::at_index(9)
    );
    let mut s = Selection::at(pick(1, 0), Grain::Cell);
    s.extend(pick(1, 9));
    assert_eq!(s.text(&t, Scrolled::START), "hello");
    let mut s = Selection::at(pick(2, 0), Grain::Cell);
    s.extend(pick(2, 9));
    assert_eq!(s.text(&t, Scrolled::START), "hello worl");
}

/// **An encircled character's ring is drawn at its character's box**
/// (`SGR 52`): on a double-width line the ring around a lone character is
/// about twice as wide as the single one's and as tall, and on a
/// double-height pair twice as wide and twice as tall, crossing the
/// boundary between the two lines.
#[test]
fn an_encircled_ring_is_drawn_at_the_line_s_size() {
    let data = faces();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let (cw, ch) = cell(&r);
    // A space: the ring is all the ink.
    let one = draw(&mut r, b"\x1b[?25l\x1b[2;3H\x1b[52m \x1b[m");
    let wide = draw(&mut r, b"\x1b[?25l\x1b[2;3H\x1b#6\x1b[52m \x1b[m");
    let tall = draw(
        &mut r,
        b"\x1b[?25l\x1b[2;3H\x1b#3\x1b[52m \x1b[m\x1b[3;3H\x1b#4\x1b[52m \x1b[m",
    );
    let (a0, ay0, a1, ay1) = ink(&one, ch, 2 * ch).expect("the single ring");
    let (b0, by0, b1, by1) = ink(&wide, ch, 2 * ch).expect("the wide ring");
    let (c0, cy0, c1, cy1) = ink(&tall, ch, 3 * ch).expect("the tall ring");
    let (w1, h1) = (a1 - a0, ay1 - ay0);
    assert!(
        b0 >= 4 * cw && b1 <= 6 * cw,
        "the wide ring is in its two columns: {b0}..{b1}"
    );
    assert!(
        b1 - b0 + 2 >= 2 * w1 && b1 - b0 <= 2 * w1 + 2,
        "twice as wide: {w1} and {}",
        b1 - b0
    );
    assert_eq!((ay0, ay1), (by0, by1), "as tall");
    assert!(
        c1 - c0 + 2 >= 2 * w1 && c1 - c0 <= 2 * w1 + 2,
        "the tall ring twice as wide: {w1} and {}",
        c1 - c0
    );
    assert!(
        cy1 - cy0 + 2 >= 2 * h1 && cy1 - cy0 <= 2 * h1 + 2,
        "twice as tall: {h1} and {}",
        cy1 - cy0
    );
    assert!(cy0 < 2 * ch && cy1 > 2 * ch, "it crosses the boundary");
}
