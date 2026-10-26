//! The selection's text, extracted from a live terminal: a wide character
//! contributes once, a gap inside a row is a space and a trailing blank is
//! nothing, marks follow their base, a wrapped row joins the next without a
//! newline, and a range over the history reads the lines the ring kept.
//! `docs/research/notes/k-selection.md` is the spec.

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, ReadLen, Row, Rows, WinSize};
use baud_render::{AbsLine, Grain, Scrolled, Selection, Span};

const WS: WinSize = WinSize {
    ws_col: 10,
    ws_row: 4,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// A selection from `(row, col)` to `(row, col)` on the live screen, with
/// `scrolled` advances behind it.
fn select(t: &Term<'_>, scrolled: Scrolled, from: (usize, usize), to: (usize, usize)) -> Selection {
    let view = t.viewport();
    let at = |(r, c): (usize, usize)| {
        let row = Row::from_index(r);
        Span::cell(
            view.row(row),
            scrolled.line(Rows::ZERO, row),
            Col::at_index(c),
        )
    };
    let mut s = Selection::at(at(from), Grain::Cell);
    s.extend(at(to));
    s
}

/// The same corners as a block: Option held at the press.
fn block(t: &Term<'_>, from: (usize, usize), to: (usize, usize)) -> Selection {
    let view = t.viewport();
    let at = |(r, c): (usize, usize)| {
        let row = Row::from_index(r);
        Span::cell(
            view.row(row),
            Scrolled::START.line(Rows::ZERO, row),
            Col::at_index(c),
        )
    };
    let mut s = Selection::block(at(from), Grain::Cell);
    s.extend(at(to));
    s
}

#[test]
fn a_wide_character_a_gap_a_trailing_blank_and_a_mark_read_as_the_spec_says() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Row 0: `ab`, a wide `日`, then a gap the cursor moved over, then `e`
    // with a combining acute, then a blank never written.
    feed(&mut t, "ab\u{65e5}\x1b[7Ge\u{301}".as_bytes());
    let s = select(&t, Scrolled::START, (0, 0), (0, 9));
    assert_eq!(s.text(&t, Scrolled::START), "ab\u{65e5}  e\u{301}");
    // Starting on the tail and ending on the lead both take the whole
    // character.
    let s = select(&t, Scrolled::START, (0, 3), (0, 3));
    assert_eq!(s.text(&t, Scrolled::START), "\u{65e5}");
}

/// **A letter with a combining mark is a letter to a double click**: `cafe`
/// with an acute on its `e`, then a space, is one word from any of its four
/// cells, the marked one included. The marked cell holds its cluster's slot
/// rather than its letter, and the word's test reads the letter back from
/// the store; read as the cell's content it is no character, and the word
/// stopped before it.
#[test]
fn a_letter_with_a_combining_mark_is_part_of_its_word() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "cafe\u{301} au".as_bytes());
    let row = t.viewport().row(Row::ZERO);
    let line = Scrolled::START.line(Rows::ZERO, Row::ZERO);
    for c in 0..4 {
        let w = Span::word(&t, row, line, Col::at_index(c));
        assert_eq!(
            (w.start.col.index(), w.end.col.index()),
            (0, 3),
            "from column {c}"
        );
    }
    let mut s = Selection::at(Span::word(&t, row, line, Col::ZERO), Grain::Word);
    s.extend(Span::word(&t, row, line, Col::at_index(3)));
    assert_eq!(s.text(&t, Scrolled::START), "cafe\u{301}");
}

/// **A mark stored alone is part of its word**: after a control a mark is
/// a cluster of its own with no base (`Content::DEFECTIVE`), a character
/// the program wrote, neither whitespace nor a delimiter, so a double click
/// on the `e`, the mark or the `y` selects all three. The control is a
/// space in that cell, which ends the word.
#[test]
fn a_mark_stored_alone_is_part_of_its_word() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "e\x1b[m\u{301}y z".as_bytes());
    let row = t.viewport().row(Row::ZERO);
    let line = Scrolled::START.line(Rows::ZERO, Row::ZERO);
    for c in 0..3 {
        let w = Span::word(&t, row, line, Col::at_index(c));
        assert_eq!(
            (w.start.col.index(), w.end.col.index()),
            (0, 2),
            "from column {c}"
        );
    }
    let mut s = Selection::at(Span::word(&t, row, line, Col::ZERO), Grain::Word);
    s.extend(Span::word(&t, row, line, Col::at_index(2)));
    assert_eq!(s.text(&t, Scrolled::START), "e\u{301}y");
    let mut owned = Owned::new(&WS);
    let mut c = Term::new(owned.storage(), &WS);
    feed(&mut c, b"e y z");
    let row = c.viewport().row(Row::ZERO);
    let w = Span::word(&c, row, line, Col::ZERO);
    assert_eq!(
        (w.start.col.index(), w.end.col.index()),
        (0, 0),
        "the control"
    );
}

#[test]
fn a_wrapped_row_joins_the_next_and_a_newline_ends_a_row() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Twelve characters wrap at ten; then a newline; then a short row.
    feed(&mut t, b"0123456789AB\r\nnext");
    let s = select(&t, Scrolled::START, (0, 7), (2, 1));
    assert_eq!(s.text(&t, Scrolled::START), "789AB\nne");
}

#[test]
fn a_range_over_the_history_reads_the_lines_the_ring_kept() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Six lines on a four-row screen: two scroll into the history.
    feed(&mut t, b"one\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
    let scrolled = Scrolled::START.advanced(Rows::of_count(t.rotation().index()));
    assert_eq!(t.scrollback().len(), 2, "two lines retained");
    // `two` is absolute line 1: one above the live screen's top, line 2.
    let view = t.view(Rows::of_count(1));
    let start = Span::cell(
        view.row(Row::ZERO),
        AbsLine::at(Rows::of_count(1)),
        Col::at_index(1),
    );
    let live = t.viewport();
    let end = Span::cell(
        live.row(Row::from_index(1)),
        scrolled.line(Rows::ZERO, Row::from_index(1)),
        Col::at_index(1),
    );
    let mut s = Selection::at(start, Grain::Cell);
    s.extend(end);
    assert_eq!(s.text(&t, scrolled), "wo\nthree\nfo");
    // The window three lines up shows `two` at its top row, so the
    // highlight starts there.
    let h = s
        .highlight(
            scrolled.top(Rows::of_count(1)),
            Rows::of_count(4),
            live.cols(),
        )
        .unwrap();
    assert_eq!(h.from, (Row::ZERO, Col::at_index(1)));
    assert_eq!(h.to, (Row::from_index(2), Col::at_index(1)));
}

#[test]
fn a_block_reads_each_row_over_its_columns_and_cuts_a_wrapped_row() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Twelve characters wrap at ten; a newline; then `x`, a gap the cursor
    // moved over, and `yz`.
    feed(&mut t, b"0123456789AB\r\nx\x1b[4Gyz");
    // Columns 1..=3 on each row: the wrapped row is cut by a newline, the
    // second row's blanks never written are dropped, and the gap before
    // `y` on the third is two spaces.
    let s = block(&t, (0, 1), (2, 3));
    assert_eq!(s.text(&t, Scrolled::START), "123\nB\n  y");
    // Dragged from the bottom-left corner to the top-right, the same block.
    let s = block(&t, (2, 1), (0, 3));
    assert_eq!(s.text(&t, Scrolled::START), "123\nB\n  y");
    // The stream over the first pair of corners, for contrast: row-major,
    // and the wrapped row joins the next.
    let s = select(&t, Scrolled::START, (0, 1), (2, 3));
    assert_eq!(s.text(&t, Scrolled::START), "123456789AB\nx  y");
}

#[test]
fn a_block_edge_through_a_wide_character_keeps_it_whole_or_leaves_it_out() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // A one-column block at column 2. On row 1 a wide character's lead is
    // at 1 and its tail at 2, straddling the left edge; on row 2 its lead
    // is at 2 and its tail at 3, straddling the right. Rows 0 and 3 are
    // the narrow corners.
    feed(
        &mut t,
        "abcdef\r\na\u{65e5}bc\r\nab\u{65e5}c\r\nabcdef".as_bytes(),
    );
    let view = t.viewport();
    assert!(view.row(Row::from_index(1))[2].content.is_wide_tail());
    assert!(view.row(Row::from_index(2))[3].content.is_wide_tail());
    // Out on row 1, whole on row 2: a character belongs to the block its
    // lead is in.
    let s = block(&t, (0, 2), (3, 2));
    assert_eq!(s.text(&t, Scrolled::START), "c\n\n\u{65e5}\nc");
}

/// **`?96` DECRLCM copies each line from its right edge**: "if \"ABCD\" is
/// to be copied right-to-left ... the string is saved in the paste buffer
/// as \"DCBA.\"" (`[[source: vt510-decrlcm]]`). A copy over two lines reads
/// each in the order it is drawn from the right, the lines kept in order
/// (Jev's leans, DRAWN_FROM_RIGHT and EACH_LINE); the trailing blanks a
/// copy drops stay dropped, and a wide character and a letter with its
/// mark stay whole. Reset, the copy is as written.
#[test]
fn decrlcm_copies_each_line_from_its_right_edge() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "ABCD\r\nx\u{65e5}e\u{301}".as_bytes());
    let s = select(&t, Scrolled::START, (0, 0), (1, 9));
    assert_eq!(s.text(&t, Scrolled::START), "ABCD\nx\u{65e5}e\u{301}");
    feed(&mut t, b"\x1b[?96h");
    assert_eq!(s.text(&t, Scrolled::START), "DCBA\ne\u{301}\u{65e5}x");
}

/// **A Hebrew line UAX #9 draws right to left copies in the order it
/// reads** under `?96`, and an English word on it reversed: the cells as
/// drawn, read from the right. With BDSM reset nothing is reordered and
/// the whole line comes out reversed, as on the VT510.
#[test]
fn decrlcm_reads_a_reordered_line_as_it_is_drawn() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\u{5D0}\u{5D1} ok\x1b[?96h".as_bytes());
    let s = select(&t, Scrolled::START, (0, 0), (0, 9));
    assert_eq!(s.text(&t, Scrolled::START), "\u{5D0}\u{5D1} ko");
    feed(&mut t, b"\x1b[8l");
    assert_eq!(s.text(&t, Scrolled::START), "ko \u{5D1}\u{5D0}");
}

/// **A line in the history reads as its window draws it** under `?96`.
#[test]
fn decrlcm_reads_a_history_line_from_its_right_edge() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[?96hone\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix",
    );
    let scrolled = Scrolled::START.advanced(Rows::of_count(t.rotation().index()));
    let view = t.view(Rows::of_count(1));
    let start = Span::cell(
        view.row(Row::ZERO),
        AbsLine::at(Rows::of_count(1)),
        Col::at_index(0),
    );
    let live = t.viewport();
    let end = Span::cell(
        live.row(Row::ZERO),
        scrolled.line(Rows::ZERO, Row::ZERO),
        Col::at_index(9),
    );
    let mut s = Selection::at(start, Grain::Cell);
    s.extend(end);
    assert_eq!(s.text(&t, scrolled), "owt\neerht");
}
