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
//!
//! **And each row's maps carry the generation that wrote them**
//! (`Orders::laid_after`): a row reads as laid out again after a seal only
//! where a layout or a rejoin wrote its maps otherwise than they were, and
//! every row does after another layout's seal; a row laid out again as it
//! was -- in its own order and unjoined, in the order it held, or under a
//! display made again the same -- does not; and an answer that every row's
//! joins be found again stays owed until a rejoin of every window row ends.

use baud_core::{BidiSupport, Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Col, Content, PackedCell, ReadLen, Row, Rows, WinSize, Wrap,
};
use baud_render::{BidiWork, Changes, Joins, LaidSeal, Rejoin, Reorder, Written};

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

/// The window's rows whose maps `o` wrote after `seal`.
fn laid_after(o: &Reorder, seal: &LaidSeal) -> Vec<usize> {
    (0..WS.ws_row)
        .map(usize::from)
        .filter(|&y| o.orders().laid_after(Row::from_index(y), seal))
        .collect()
}

/// **U8: a row reads as laid out again after a seal only where a layout
/// wrote its maps**: a frame with nothing written leaves every row and the
/// maps as the seal found them; a write in paragraph A's Hebrew row lays
/// out rows 2 and 3, which read as laid out again, and no other row does,
/// paragraph B's included. Named breaks: a row's stamp never read (no row
/// laid out again), and a layout that holds stamping the rows it keeps.
#[test]
fn a_row_reads_as_laid_out_again_only_where_a_layout_wrote_it() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let seal = o.orders().laid_seal();
    let _ = frame(&mut t, &mut o);
    assert_eq!(
        laid_after(&o, &seal),
        Vec::<usize>::new(),
        "nothing written"
    );
    assert!(!o.orders().moved_since(&seal), "and no map moved");
    feed(&mut t, "\x1b[4;1Habc de");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 2, "the control: paragraph A laid out");
    assert_eq!(laid_after(&o, &seal), vec![2, 3], "paragraph A's rows");
    assert!(o.orders().moved_since(&seal), "the maps moved");
}

/// **U9: a seal from another layout reads every row as laid out again**:
/// a layout written over twenty times, its seal taken, and a layout made
/// once of the same screen -- the leaving row's, swapped in for a slide, or
/// a renderer's made again -- whose stamps are all below that seal's
/// count. Every row of the second reads as laid out again after the
/// first's seal. Named break: the count compared without the epoch.
#[test]
fn a_seal_from_another_layout_reads_every_row_as_laid_out_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut a = Reorder::new();
    let _ = frame(&mut t, &mut a);
    for k in 0..20 {
        feed(
            &mut t,
            &format!("\x1b[4;1H{}", ["abc de", "\u{5d0}\u{5d1}\u{5d2}"][k % 2]),
        );
        let _ = frame(&mut t, &mut a);
    }
    let seal = a.orders().laid_seal();
    let mut b = Reorder::new();
    let _ = frame(&mut t, &mut b);
    let every: Vec<usize> = (0..WS.ws_row).map(usize::from).collect();
    assert_eq!(
        laid_after(&b, &seal),
        every,
        "every row of the other layout"
    );
    assert!(b.orders().moved_since(&seal), "and its maps moved");
}

/// **U10: a rejoin that finds a row's joins the same marks no row laid out
/// again** (`Reorder::rejoin`, with BDSM reset): Arabic on row 4, every row
/// joined after `Reorder::keep_displayed`, a seal taken, and every row
/// joined again with nothing written -- as a frame with a composition
/// shown joins every row. No row reads as laid out again and the maps did
/// not move. The control: row 4 holds joined cells. Named break: a rejoin
/// that stamps its row whatever it found.
#[test]
fn a_rejoin_that_finds_a_rows_joins_the_same_marks_no_row() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}");
    let mut o = Reorder::new();
    let written = Written::default();
    let join_every_row = |t: &mut Term<'_>, o: &mut Reorder| {
        let _ = t.seal_writes();
        let _ = o.keep_displayed(t, Rows::ZERO, Changes::Converted, &written);
        let vp = t.view(Rows::ZERO);
        for y in 0..usize::from(WS.ws_row) {
            o.rejoin(t, &vp, Row::from_index(y), &written);
        }
        o.rejoined();
    };
    join_every_row(&mut t, &mut o);
    assert!(
        o.orders().row_joins(Row::from_index(4)).is_some(),
        "the control: row 4 joined"
    );
    let seal = o.orders().laid_seal();
    join_every_row(&mut t, &mut o);
    assert_eq!(
        laid_after(&o, &seal),
        Vec::<usize>::new(),
        "no row's joins moved"
    );
    assert!(!o.orders().moved_since(&seal), "and no map moved");
}

/// A cell holding `c` in the default rendition.
fn cell(c: char) -> PackedCell {
    PackedCell {
        content: Content::codepoint(Codepoint::from_char(c)),
        ..PackedCell::EMPTY
    }
}

/// **U11: a row laid out again as it was, in its own order, reads as not
/// laid out again**: one paragraph over rows 2 and 3, row 2 all Latin
/// soft-wrapped onto row 3, Latin then Hebrew, so its first strong letter
/// is Latin and row 2 is in its own order while row 3 is drawn in another.
/// A write of Hebrew over row 3's first word lays out both rows again (the
/// control), and only row 3 reads as laid out again: its order came out
/// otherwise, and row 2's maps held none and hold none. Named break: every
/// row a relay lifted stamped at its end, whatever the layout wrote.
#[test]
fn a_row_laid_out_again_in_its_own_order_reads_as_not_laid_out_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("\x1b[3;1H{}xyz \u{5d0}\u{5d1}\u{5d2}", "abc ".repeat(10)),
    );
    let (two, three) = (Row::from_index(2), Row::from_index(3));
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    assert!(
        !o.orders().reordered(two) && o.orders().reordered(three),
        "the control: row 2 in its own order, row 3 in another"
    );
    let seal = o.orders().laid_seal();
    feed(&mut t, "\x1b[4;1H\u{5d3}\u{5d4}");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 2, "the control: rows 2 and 3 laid out");
    assert_eq!(laid_after(&o, &seal), vec![3], "row 3 alone");
    as_fresh(&t, &o, "a write in row 3");
}

/// **U12: a rejoin of every row with something written marks only the
/// rows whose joins it found otherwise** (`Reorder::keep_displayed` with
/// BDSM reset, its joins left in place for that rejoin): Arabic on rows 4
/// and 6, every row joined, then a composition over row 6's letters in
/// Latin, read as written. The answer is that every row's joins be found
/// again (the control), each against the joins it held, so row 6 alone
/// reads as laid out again: row 4's joins came out the same. Named break:
/// the joins forgotten before that answer, which reads every row as laid
/// out again.
#[test]
fn a_rejoin_of_every_row_marks_only_the_rows_whose_joins_moved() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}\x1b[7;1H\u{0628}\u{064A}\u{062A}",
    );
    let (four, six) = (Row::from_index(4), Row::from_index(6));
    let mut o = Reorder::new();
    let none = Written::default();
    let join = |t: &mut Term<'_>, o: &mut Reorder, written: &Written| {
        let _ = t.seal_writes();
        let how = o.keep_displayed(t, Rows::ZERO, Changes::Converted, written);
        let vp = t.view(Rows::ZERO);
        for y in 0..usize::from(WS.ws_row) {
            o.rejoin(t, &vp, Row::from_index(y), written);
        }
        o.rejoined();
        how
    };
    let _ = join(&mut t, &mut o, &none);
    assert_eq!(join(&mut t, &mut o, &none), Rejoin::Changed, "kept");
    assert!(
        o.orders().row_joins(four).is_some() && o.orders().row_joins(six).is_some(),
        "the control: rows 4 and 6 joined"
    );
    let seal = o.orders().laid_seal();
    let latin: Vec<PackedCell> = "abc".chars().map(cell).collect();
    let mut over = Written::default();
    over.write(&t.view(Rows::ZERO), six, Col::ZERO, &latin);
    assert_eq!(
        join(&mut t, &mut o, &over),
        Rejoin::Every,
        "the control: every row's joins found again"
    );
    assert!(o.orders().row_joins(six).is_none(), "row 6 reads the Latin");
    assert_eq!(laid_after(&o, &seal), vec![6], "row 6 alone");
}

/// **U13: an answer that every row's joins be found again stays owed until
/// that rejoin ends** (`Reorder::rejoined`): with BDSM reset, the first
/// frame answers `Rejoin::Every`; a frame that returns before its rejoin --
/// as a cell frame with no free set does -- leaves it owed, and the next
/// frame answers it again. Once a rejoin ends, the frame after keeps the
/// joins (the control). Named break: the key kept when the answer is
/// given, not when its rejoin ends.
#[test]
fn a_rejoin_of_every_row_not_ended_is_asked_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}");
    let none = Written::default();
    let mut o = Reorder::new();
    let keep = |t: &mut Term<'_>, o: &mut Reorder| {
        let _ = t.seal_writes();
        o.keep_displayed(t, Rows::ZERO, Changes::Copied, &none)
    };
    assert_eq!(keep(&mut t, &mut o), Rejoin::Every, "the first frame");
    assert_eq!(
        keep(&mut t, &mut o),
        Rejoin::Every,
        "the frame after one that returned before its rejoin"
    );
    let vp = t.view(Rows::ZERO);
    for y in 0..usize::from(WS.ws_row) {
        o.rejoin(&t, &vp, Row::from_index(y), &none);
    }
    o.rejoined();
    assert_eq!(
        keep(&mut t, &mut o),
        Rejoin::Changed,
        "the control: kept once a rejoin ended"
    );
}

/// **U14: the status line's maps stay while every window row's joins are
/// found again**: with BDSM reset, an Arabic status line, and a composition
/// over row 2 read as written, every frame answers `Rejoin::Every`; the
/// status line, whose plane is not written, is kept -- row 2's unit alone
/// is laid out again (`Reorder::work`), which is what shows it: a status
/// line laid out again as it was would mark nothing (`maps::relay_quietly`)
/// -- and its row of the maps -- the row after the window's -- reads as not
/// laid out again, nor does any window row. The controls: the status
/// line's row joined, and the answer. Named break: the status line laid
/// out again on every such answer.
#[test]
fn the_status_lines_maps_stay_while_every_row_is_rejoined() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}\x1b[2$~\x1b[1$}\u{0628}\u{064A}\u{062A}\x1b[0$}",
    );
    let status = Row::from_index(usize::from(WS.ws_row));
    let latin: Vec<PackedCell> = "abc".chars().map(cell).collect();
    let mut over = Written::default();
    over.write(&t.view(Rows::ZERO), Row::from_index(2), Col::ZERO, &latin);
    let mut o = Reorder::new();
    let join = |t: &mut Term<'_>, o: &mut Reorder| {
        let _ = t.seal_writes();
        let how = o.keep_displayed(t, Rows::ZERO, Changes::Converted, &over);
        let vp = t.view(Rows::ZERO);
        for y in 0..usize::from(WS.ws_row) {
            o.rejoin(t, &vp, Row::from_index(y), &over);
        }
        o.rejoined();
        how
    };
    let _ = join(&mut t, &mut o);
    let _ = join(&mut t, &mut o);
    assert!(
        o.orders().row_joins(status).is_some(),
        "the control: the status line's row joined"
    );
    let seal = o.orders().laid_seal();
    assert_eq!(
        join(&mut t, &mut o),
        Rejoin::Every,
        "the control: every row's joins found again"
    );
    assert_eq!(
        o.work().laid.value(),
        1,
        "the status line kept: row 2's unit alone laid out again"
    );
    assert!(
        !o.orders().laid_after(status, &seal),
        "the status line's row"
    );
    assert_eq!(laid_after(&o, &seal), Vec::<usize>::new(), "no window row");
    assert!(!o.orders().moved_since(&seal), "and no map moved");
}

/// **U15: an answer that every row's joins be found again is kept only
/// once every window row was rejoined** (`Reorder::rejoined`): with BDSM
/// reset, the first frame answers `Rejoin::Every`, and a caller that
/// rejoins rows 4 and 6 alone -- the rows it saw change, as under
/// `Rejoin::Changed` -- before the rejoin ends is answered it again on the
/// next frame. The control: once every row is rejoined, the frame after
/// keeps the joins. Named break: the key kept whatever rows were rejoined.
#[test]
fn a_rejoin_of_some_rows_under_every_is_asked_again() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, "\x1b[8l\x1b[5;1H\u{0628}\u{064A}\u{062A}");
    let none = Written::default();
    let mut o = Reorder::new();
    let keep = |t: &mut Term<'_>, o: &mut Reorder| {
        let _ = t.seal_writes();
        o.keep_displayed(t, Rows::ZERO, Changes::Copied, &none)
    };
    assert_eq!(keep(&mut t, &mut o), Rejoin::Every, "the first frame");
    let vp = t.view(Rows::ZERO);
    for y in [4, 6] {
        o.rejoin(&t, &vp, Row::from_index(y), &none);
    }
    o.rejoined();
    assert_eq!(
        keep(&mut t, &mut o),
        Rejoin::Every,
        "the frame after a rejoin of rows 4 and 6 alone"
    );
    let vp = t.view(Rows::ZERO);
    for y in 0..usize::from(WS.ws_row) {
        o.rejoin(&t, &vp, Row::from_index(y), &none);
    }
    o.rejoined();
    assert_eq!(
        keep(&mut t, &mut o),
        Rejoin::Changed,
        "the control: kept once every row was rejoined"
    );
}

/// **U16: a paragraph laid out again as it was marks no row**
/// (`maps::relay_quietly`): paragraph B's first letter written again as it
/// was lays out its rows 6 and 7 again (the control), row 6 drawn in
/// another order (the control), and neither reads as laid out again, nor
/// do the maps read as moved: each row's order and joins came out as they
/// were. Named break: every row a relay lifted stamped at its end,
/// whatever the layout wrote.
#[test]
fn a_paragraph_laid_out_again_as_it_was_marks_no_row() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &two_paragraphs());
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    assert!(
        o.orders().reordered(Row::from_index(6)),
        "the control: row 6 drawn in another order"
    );
    let seal = o.orders().laid_seal();
    feed(&mut t, "\x1b7\x1b[7;1H\u{5d0}\x1b8");
    let w = frame(&mut t, &mut o);
    assert_eq!(w.laid.value(), 2, "the control: paragraph B laid out again");
    assert_eq!(laid_after(&o, &seal), Vec::<usize>::new(), "no row");
    assert!(!o.orders().moved_since(&seal), "and no map moved");
    as_fresh(&t, &o, "paragraph B written as it was");
}

/// **U17: a display made again as it was marks no row**: word wrap on over
/// a right-to-left paragraph the display re-breaks, finished by a prompt;
/// its first letter written again as it was makes the display again,
/// another one (the control), lays the window out again (the control),
/// and no row reads as laid out again, nor do the maps read as moved: the
/// display, by value, and every row's order came out as they were. Named
/// break: every row stamped as the maps are made again over a display, as
/// a reset did.
#[test]
fn a_display_made_again_as_it_was_marks_no_row() {
    let words = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} ".repeat(10);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        &format!("\x1b[3;1H{words}\x1b[10;1H\x1b]133;A\x07$ "),
    );
    let mut o = Reorder::new();
    let _ = frame(&mut t, &mut o);
    let _ = frame(&mut t, &mut o);
    let held = o.orders().display().map(core::ptr::from_ref);
    assert!(held.is_some(), "the control: a display held");
    let seal = o.orders().laid_seal();
    feed(&mut t, "\x1b7\x1b[3;1H\u{5d0}\x1b8");
    let w = frame(&mut t, &mut o);
    let now = o.orders().display().map(core::ptr::from_ref);
    assert!(
        now.is_some() && now != held,
        "the control: another display held"
    );
    assert!(w.laid.value() > 0, "the control: the window laid out again");
    assert_eq!(laid_after(&o, &seal), Vec::<usize>::new(), "no row");
    assert!(!o.orders().moved_since(&seal), "and no map moved");
    as_fresh(&t, &o, "the display made again as it was");
}
