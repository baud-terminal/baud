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
