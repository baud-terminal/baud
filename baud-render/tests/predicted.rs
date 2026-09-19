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
use baud_render::{Argb, Overlay, Resolved, Shapes};
use support::{font, renderer};

fn styles() -> Vec<Resolved> {
    vec![
        Resolved {
            fg: Argb(0xFFFFFFFF),
            bg: Argb(0xFF000000),
            underline: Argb(0xFFFFFFFF),
            shapes: Shapes::default(),
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
