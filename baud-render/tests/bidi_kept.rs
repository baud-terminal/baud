//! **The bidirectional layout kept a paragraph at a time**
//! (`baud_render::Reorder`): a frame on which nothing was written lays out
//! no paragraph and reads no row's generation; a write lays out again
//! every row of each paragraph that read it and no other; a write outside
//! every paragraph reads each row's generation once and the frame after
//! none; a paragraph split by a row that ends lays out its lower part
//! though none of it was written; and a row written over by a composition
//! lays its paragraph out again while shown and the frame after; a new
//! BDSM, and with it reset the joins read, lay the window out again
//! though no row was written. Every frame, the kept maps equal a fresh
//! layout's. Each "frame" is a seal, one layout and its `Work` read.

use baud_core::{BidiSupport, Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Col, Content, PackedCell, ReadLen, Row, Rows, WinSize, Wrap,
};
use baud_render::{BidiWork, Joins, Reorder, Written};

const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};
const ROWS: u64 = 10;

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Forty columns of digits and spaces: no strong letter.
fn digits() -> String {
    format!("{}4", "12 ".repeat(13))
}

/// **The screen U1-U3 and U5 start from**: paragraph A on rows 2-3, row 2
/// all digits and spaces, row 3 Hebrew, so UAX #9 P2-P3 finds A's first
/// strong letter in row 3 and row 2 is laid out right to left; a Latin row;
/// paragraph B, Hebrew wrapped over rows 6-7. No status line.
fn two_paragraphs() -> String {
    let b = "\u{5d0}\u{5d1}\u{5d2}\u{5d3} ".repeat(9);
    format!(
        "\x1b[3;1H{}\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}\x1b[5;1Hplain latin\x1b[7;1H{b}",
        digits()
    )
}

/// One frame: the writes sealed, the window laid out, what it read.
fn frame(t: &mut Term<'_>, o: &mut Reorder) -> BidiWork {
    let _ = t.seal_writes();
    o.lay_out(t, Rows::ZERO);
    o.work()
}

/// The kept maps equal a fresh layout's, row for row.
fn as_fresh(t: &Term<'_>, o: &Reorder, what: &str) {
    let mut fresh = Reorder::new();
    fresh.lay_out(t, Rows::ZERO);
    assert_eq!(o.orders(), fresh.orders(), "{what}: the kept maps");
}

/// **U1: a frame with nothing written lays out no paragraph**: the first
/// frame lays out every row; the three after it lay out none, read no
/// generation and test no row. Named break: always laid out again.
#[test]
fn a_frame_with_nothing_written_lays_out_no_paragraph() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), ROWS, "the first frame lays out every row");
    assert!(
        o.orders().reordered(Row::from_index(2)),
        "the control: row 2 reversed"
    );
    for k in 2..=4 {
        let w = frame(&mut t, &mut o);
        assert_eq!(
            (w.laid.value(), w.gens.value(), w.tested.value()),
            (0, 0, 0),
            "frame {k}: nothing written"
        );
        as_fresh(&t, &o, "a frame with nothing written");
    }
}

/// **U2: a write in a paragraph lays out every row of it and no other**:
/// row 3's Hebrew written over in Latin. Paragraph A, rows 2 and 3, is laid
/// out again whole, and row 2, none of whose cells was written, is in its
/// own order now (the control: it was reversed); paragraph B is kept.
/// Named breaks: every paragraph laid out again (10 rows), or only the row
/// written (1, row 2 left reversed).
#[test]
fn a_write_in_a_paragraph_lays_out_every_row_of_it_and_no_other() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    assert!(
        o.orders().reordered(Row::from_index(2)),
        "the control: row 2 reversed"
    );
    feed(&mut t, "\x1b[4;1Habc de");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 2, "rows 2 and 3, paragraph A, alone");
    assert!(
        !o.orders().reordered(Row::from_index(2)),
        "row 2 is in its own order"
    );
    assert!(o.orders().reordered(Row::from_index(6)), "B is kept");
    as_fresh(&t, &o, "a write in paragraph A");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 0, "and the frame after lays out none");
}

/// **U3: a write outside every paragraph reads the generations once**:
/// the alternate screen entered, written and left between two frames.
/// The isolation: a surface was written since the seal, and no primary row
/// was. The next frame reads each row's generation once and lays out none;
/// the frame after reads none. Named break: the layout not kept under the
/// new seal, which reads every row's generation on both frames.
#[test]
fn a_write_outside_every_paragraph_reads_generations_once() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    let seal = t.sealed();
    feed(&mut t, "\x1b[?1049hthe alternate screen\x1b[?1049l");
    assert!(t.wrote_since(&seal), "the isolation: a surface was written");
    let g = t.row_gens(Rows::ZERO);
    assert!(
        (0..10).all(|y| !g.after(Row::from_index(y), &seal)),
        "the isolation: no primary row was"
    );
    let w = frame(&mut t, &mut o);
    assert_eq!(
        (w.gens.value(), w.laid.value()),
        (ROWS, 0),
        "each row's generation read once, nothing laid out"
    );
    as_fresh(&t, &o, "a write outside every paragraph");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.gens.value(), 0, "the frame after reads none");
}

/// **U4: a split paragraph lays out its lower part though none of it was
/// written**: rows 3-6 one paragraph whose only strong letter, Hebrew, is
/// in row 3, rows 4-6 digits and spaces, so rows 5 and 6 are laid out right
/// to left. A line feed from row 4 ends it (the positive control: row 4 no
/// longer continues, and it alone was written), which splits the paragraph
/// into rows 3-4 and 5-6; 5-6 has no strong letter, so it is in its own
/// order (the control: it was reversed). Both units are laid out again.
/// Named break: a unit laid out again only when a row of its own was
/// written, which keeps 5-6 reversed.
#[test]
fn a_split_paragraph_lays_out_its_lower_part_though_none_of_it_was_written() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let first = format!("\u{5d0}\u{5d1}\u{5d2} {}", "1 ".repeat(18));
    feed(
        &mut t,
        &format!("\x1b[4;1H{first}{}{}12 34", digits(), digits()),
    );
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    assert!(
        o.orders().reordered(Row::from_index(5)),
        "the control: row 5 reversed"
    );
    let seal = t.sealed();
    feed(&mut t, "\x1b[5;1H\n");
    let vp = t.view(Rows::ZERO);
    assert_eq!(vp.wrapped(Row::from_index(4)), Wrap::Ended, "row 4 ends");
    let g = t.row_gens(Rows::ZERO);
    let written: Vec<usize> = (0..10)
        .filter(|&y| g.after(Row::from_index(y), &seal))
        .collect();
    assert_eq!(written, vec![4], "row 4 alone was written");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 4, "rows 3-4 and 5-6");
    assert!(
        !o.orders().reordered(Row::from_index(5)),
        "rows 5-6 are in their own order"
    );
    as_fresh(&t, &o, "a split paragraph");
}

/// **U5: a composition over a paragraph lays it out while shown and the
/// frame after**: row 3 written over in Latin by a composition for one
/// frame, then none. Both frames lay out paragraph A again and equal a
/// fresh layout's; the third keeps it. Named break: the rows written over
/// at the last layout not laid out again, which keeps the composition's
/// order on the second frame.
#[test]
fn a_composition_over_a_paragraph_lays_it_out_while_shown_and_the_frame_after() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    let cell = |c: char| PackedCell {
        content: Content::codepoint(Codepoint::from_char(c)),
        ..PackedCell::EMPTY
    };
    let latin: Vec<PackedCell> = "abcdef".chars().map(cell).collect();
    let mut written = Written::default();
    written.write(&t.view(Rows::ZERO), Row::from_index(3), Col::ZERO, &latin);
    let _ = t.seal_writes();
    o.lay_out_written(&t, Rows::ZERO, Joins::Seen, &written);
    assert_eq!(o.work().laid.value(), 2, "A laid out while written over");
    assert!(
        !o.orders().reordered(Row::from_index(2)),
        "the control: row 2 follows the Latin written over row 3"
    );
    let mut fresh = Reorder::new();
    fresh.lay_out_written(&t, Rows::ZERO, Joins::Seen, &written);
    assert_eq!(o.orders(), fresh.orders(), "the written frame's maps");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 2, "A laid out again the frame after");
    assert!(
        o.orders().reordered(Row::from_index(2)),
        "row 2 reversed again"
    );
    as_fresh(&t, &o, "the frame after a composition");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 0, "and kept after that");
}

/// **U6: a new BDSM lays the window out again**: a one-row Hebrew
/// paragraph on row 9, laid out right to left with BDSM set, is in its own
/// order once BDSM is reset, and right to left again once it is set, no
/// window row written either time. Row 9 is a unit of the same rows cut
/// either way, so it is kept unless the key moved. The layout reads joins
/// in both modes (`Reorder::lay_out`), so BDSM is the one input of its key
/// that moved: on the renderer's own window a change of BDSM moves
/// whether joins are read or the pass with it. Named break: BDSM out of
/// the key.
#[test]
fn a_new_bdsm_lays_the_window_out_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("{}\x1b[10;1H\u{5d0}\u{5d1}\u{5d2} 12", two_paragraphs()),
    );
    let nine = Row::from_index(9);
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    assert!(
        o.orders().reordered(nine),
        "the control: row 9 right to left with BDSM set"
    );
    for (bytes, support, reordered) in [
        ("\x1b[8l", BidiSupport::Explicit, false),
        ("\x1b[8h", BidiSupport::Implicit, true),
    ] {
        let seal = t.sealed();
        feed(&mut t, bytes);
        assert_eq!(t.bidi_support(), support, "{bytes:?}: the operation");
        let g = t.row_gens(Rows::ZERO);
        assert!(
            (0..10).all(|y| !g.after(Row::from_index(y), &seal)),
            "{bytes:?}: the isolation, no window row written"
        );
        let _ = frame(&mut t, &mut o);
        assert_eq!(o.orders().reordered(nine), reordered, "{bytes:?}: row 9");
        as_fresh(&t, &o, "a new BDSM");
    }
}

/// **U7: with BDSM reset, joins read lay the window out again**: a row of
/// Arabic laid out while no row is read for its joins
/// (`Reorder::lay_out_as` with `Joins::Unseen`), then with joins read and
/// nothing written. The second layout lays out every row and reads row
/// 4's joins, as a fresh one does: whether joins are read is the one input
/// of its key that moved. On the renderer's own window the first joining
/// letter shaped moves the pass as well (`Reorder::keep_displayed`); the
/// leaving row's layout (`Reorder::lay_out_top` with what the renderer has
/// shaped) is where it moves alone. Named break: joins out of the key.
#[test]
fn with_bdsm_reset_joins_read_lay_the_window_out_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}");
    let four = Row::from_index(4);
    let mut o = Reorder::new();
    for _ in 0..2 {
        let _ = t.seal_writes();
        o.lay_out_as(&t, Rows::ZERO, Joins::Unseen);
    }
    assert!(
        o.orders().row_joins(four).is_none(),
        "the control: no row read for its joins"
    );
    let seal = t.sealed();
    o.lay_out_as(&t, Rows::ZERO, Joins::Seen);
    assert!(!t.wrote_since(&seal), "the isolation: nothing written");
    assert_eq!(o.work().laid.value(), ROWS, "every row laid out again");
    assert!(o.orders().row_joins(four).is_some(), "row 4's joins read");
    as_fresh(&t, &o, "joins read");
}
