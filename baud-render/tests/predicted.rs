//! Row 9: a predicted echo is drawn as a record over a masked cell, and
//! the mask is how the overlay says a cell is already drawn. `walk` emits
//! records for the cells it takes off the shader -- a mark, a glyph
//! inside a ligature window -- and `emit` appends rather than replacing,
//! so a prediction landing on one of those cells would put two glyphs in
//! the same cell. It gives way instead, and the echo draws the cell a
//! frame later.

mod support;

use baud_domain::{CellFlags, Codepoint, Col, Cols, Content, PackedCell, Row};
use baud_font::Face;
use baud_render::{Argb, Curves, Overlay, Resolved, Shapes};
use support::{font, renderer};

fn styles() -> Vec<Resolved> {
    vec![
        Resolved {
            fg: Argb(0xFFFFFFFF),
            bg: Argb(0xFF000000),
            underline: Argb(0xFFFFFFFF),
            shapes: Shapes::default(),
            curves: Curves::default(),
            glass: Argb(0xFFFFFFFF),
            glass_curves: Curves::default(),
            line: Argb(0xFFFFFFFF),
            line_curves: Curves::default(),
        };
        8
    ]
}

fn cell(ch: char) -> PackedCell {
    PackedCell {
        content: Content::codepoint(Codepoint::new(u32::from(ch)).unwrap()),
        style: PackedCell::EMPTY.style,
        flags: CellFlags::NONE,
    }
}

#[test]
fn a_prediction_gives_way_to_a_cell_the_walk_already_drew() {
    let bytes = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&bytes).unwrap()];
    let mut r = renderer(&faces);
    let mut o = Overlay::new();
    let st = styles();
    let cols = Cols::of_index(20);
    let mut mask = vec![0u8; 20 * 4];
    let mut records = Vec::new();

    // A clean cell: the prediction draws, and says so in the mask.
    o.predicted(
        &mut r,
        (Row::from_index(1), Col::at_index(3)),
        cell('a'),
        &st,
        None,
        None,
        cols,
        &mut mask,
        &mut records,
    );
    let after_first = records.len();
    assert!(after_first > 0, "a prediction on a clean cell draws");
    assert_eq!(mask[20 + 3], 1, "and masks the cell it drew");

    // The same cell again -- which is what a cell `walk` already recorded
    // looks like from here: the bit is set and a record exists.
    o.predicted(
        &mut r,
        (Row::from_index(1), Col::at_index(3)),
        cell('b'),
        &st,
        None,
        None,
        cols,
        &mut mask,
        &mut records,
    );
    assert_eq!(
        records.len(),
        after_first,
        "a prediction over a cell already drawn appends nothing"
    );

    // The control: a different cell still draws, so the rule above is the
    // mask's doing and not the prediction path having stopped working.
    o.predicted(
        &mut r,
        (Row::from_index(1), Col::at_index(5)),
        cell('c'),
        &st,
        None,
        None,
        cols,
        &mut mask,
        &mut records,
    );
    assert!(
        records.len() > after_first,
        "a prediction on another clean cell still draws"
    );
    assert_eq!(mask[20 + 5], 1);
}

/// **A prediction in a concealed rendition draws nothing.** The echo takes
/// the rendition of the cell to its left, so a character typed after `SGR 8`
/// -- a prompt concealing what is typed into it -- is predicted concealed,
/// and drawing it would show on screen, for a frame, the character the
/// program asked the terminal to hide. The rendition is resolved from a real
/// terminal, so the concealment is the table's own and not the test's.
#[test]
fn a_prediction_in_concealed_text_draws_nothing() {
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, Rows};
    let bytes = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&bytes).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&support::WS);
    let mut t = Term::new(owned.storage(), &support::WS);
    let typed = b"pw: \x1b[8mx";
    t.write_at(BytePos::ZERO, typed);
    t.feed(BytePos::ZERO, ReadLen::from_index(typed.len()));
    let mut st = vec![Resolved::default(); t.styles().len()];
    r.resolve_styles_into(&t, &mut st);
    let cols = Cols::of_index(usize::from(support::WS.ws_col));
    let row = t.view(Rows::ZERO).row(Row::from_index(0)).to_vec();
    let concealed = row[4];
    let plain = row[0];
    let mut o = Overlay::new();
    let mut mask = vec![0u8; cols.index() * usize::from(support::WS.ws_row)];
    let mut records = Vec::new();
    // The control: the same prediction in the prompt's plain rendition draws.
    o.predicted(
        &mut r,
        (Row::from_index(0), Col::at_index(7)),
        PackedCell {
            style: plain.style,
            ..cell('y')
        },
        &st,
        None,
        None,
        cols,
        &mut mask,
        &mut records,
    );
    assert!(!records.is_empty(), "a plain prediction draws");
    records.clear();
    o.predicted(
        &mut r,
        (Row::from_index(0), Col::at_index(5)),
        PackedCell {
            style: concealed.style,
            ..cell('y')
        },
        &st,
        None,
        None,
        cols,
        &mut mask,
        &mut records,
    );
    assert!(
        records.is_empty(),
        "a concealed prediction drew {} records",
        records.len()
    );
}
