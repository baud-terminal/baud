//! **The read record against the core itself, read by read** (E2): generated
//! programs, cut into reads at generated places, each read's record checked
//! against what the read did to the screen.
//!
//! Four properties, over one generator, from a terminal whose every line of
//! both screens holds a marker of its own ([`marked_lines`]):
//!
//! - **Every line a read changed is one its record names**: a line of the
//!   primary (screen or history) or of the alternate screen, the screen up
//!   or the one parked, whose cells -- text, rendition and link id -- differ
//!   after the read is on the read's forward path (from where it began to
//!   where it ended, each end on its own surface), in its off-path span, or
//!   where one of its moves took text. So a read that switches screens is
//!   read on both. A record that renumbered is read against nothing, and
//!   one that cleared the alternate screen not against that screen's lines,
//!   which it numbers again.
//! - **Every line a read moved text onto is one its moves name**: a marker
//!   found on a line it was not on before the read stood, before it, on a
//!   line [`ReadRecord::carried_from`] gives for where it stands now: the
//!   one composition of the moves, which an embedder files by too. A line
//!   named only as written would be filed with the read's origin, so a move
//!   missing or taken from the wrong lines would pass the first property
//!   and not this one.
//! - **A byte run under ISO 8859-1 makes the read `Other`**, read from a
//!   shadow terminal fed the same reads a byte at a time, the encoding noted
//!   before each byte: one that switches and comes back inside the read is
//!   `Other` too.
//! - **The record does not depend on how a read reached the core**: the
//!   shadow's records, a byte at a time, equal the records of the reads fed
//!   whole.
//!
//! The generator here is the hand-written one; `read_record_corpus.rs` draws
//! from the core's own dispatch.
#![cfg(feature = "read-record")]
mod record_check;

use baud_core::*;
use baud_domain::{BytePos, InputEncoding, LineIdx, ReadLen, Rows};
use proptest::prelude::*;
use record_check::*;

/// A number from 0 to `n`, as a parameter's digits (0 often means default).
fn num(n: u16) -> impl Strategy<Value = String> {
    (0..=n).prop_map(|v| v.to_string())
}

/// **The alphabet**: text, the C0 controls, cursor motion up and down,
/// erases and edits on and off the cursor's row, region and side margins,
/// scrolls, the rectangle operations, column shifts, the screens, the
/// encoding switch, folder reports, marks, links and resets.
fn atom() -> impl Strategy<Value = Vec<u8>> {
    let csi = |s: String| format!("\x1b[{s}").into_bytes();
    prop_oneof![
        Just(b"text".to_vec()),
        Just(b"/etc/hosts".to_vec()),
        Just("\u{6f22}\u{5b57}".as_bytes().to_vec()),
        Just(b"\r".to_vec()),
        Just(b"\n".to_vec()),
        Just(b"\r\n".to_vec()),
        Just(b"\x08".to_vec()),
        Just(b"\t".to_vec()),
        Just(b"\x0b".to_vec()),
        Just(b"\x0c".to_vec()),
        num(9).prop_map(move |n| csi(format!("{n}A"))),
        num(9).prop_map(move |n| csi(format!("{n}B"))),
        num(9).prop_map(move |n| csi(format!("{n}C"))),
        num(9).prop_map(move |n| csi(format!("{n}D"))),
        num(4).prop_map(move |n| csi(format!("{n}E"))),
        num(4).prop_map(move |n| csi(format!("{n}F"))),
        (num(9), num(13)).prop_map(move |(r, c)| csi(format!("{r};{c}H"))),
        num(9).prop_map(move |n| csi(format!("{n}d"))),
        num(13).prop_map(move |n| csi(format!("{n}G"))),
        (0u8..=3).prop_map(move |n| csi(format!("{n}J"))),
        (0u8..=2).prop_map(move |n| csi(format!("{n}K"))),
        (0u8..=3).prop_map(move |n| csi(format!("?{n}J"))),
        num(5).prop_map(move |n| csi(format!("{n}X"))),
        num(5).prop_map(move |n| csi(format!("{n}@"))),
        num(5).prop_map(move |n| csi(format!("{n}P"))),
        num(4).prop_map(move |n| csi(format!("{n}L"))),
        num(4).prop_map(move |n| csi(format!("{n}M"))),
        num(4).prop_map(move |n| csi(format!("{n}S"))),
        num(4).prop_map(move |n| csi(format!("{n}T"))),
        num(3).prop_map(move |n| csi(format!("{n} @"))),
        num(3).prop_map(move |n| csi(format!("{n} A"))),
        num(3).prop_map(move |n| csi(format!("{n}'}}"))),
        num(3).prop_map(move |n| csi(format!("{n}'~"))),
        num(4).prop_map(move |n| csi(format!("{n}b"))),
        num(3).prop_map(move |n| csi(format!("{n}Z"))),
        (num(8), num(8)).prop_map(move |(t, b)| csi(format!("{t};{b}r"))),
        Just(b"\x1b[?69h".to_vec()),
        Just(b"\x1b[?69l".to_vec()),
        (num(12), num(12)).prop_map(move |(l, r)| csi(format!("{l};{r}s"))),
        Just(b"\x1b[?6h".to_vec()),
        Just(b"\x1b[?6l".to_vec()),
        Just(b"\x1b[?7l".to_vec()),
        Just(b"\x1b[?7h".to_vec()),
        (num(8), num(8), num(8), num(8), num(8), num(12))
            .prop_map(move |(t, l, b, r, dt, dl)| csi(format!("{t};{l};{b};{r};1;{dt};{dl};1$v"))),
        (num(8), num(8), num(8), num(8))
            .prop_map(move |(t, l, b, r)| csi(format!("88;{t};{l};{b};{r}$x"))),
        (num(8), num(8), num(8), num(8))
            .prop_map(move |(t, l, b, r)| csi(format!("{t};{l};{b};{r}$z"))),
        (num(8), num(8), num(8), num(8))
            .prop_map(move |(t, l, b, r)| csi(format!("{t};{l};{b};{r};1$r"))),
        (num(8), num(8), num(8), num(8))
            .prop_map(move |(t, l, b, r)| csi(format!("{t};{l};{b};{r};1$t"))),
        Just(b"\x1b6".to_vec()),
        Just(b"\x1b9".to_vec()),
        Just(b"\x1bM".to_vec()),
        Just(b"\x1bD".to_vec()),
        Just(b"\x1bE".to_vec()),
        Just(b"\x1b7".to_vec()),
        Just(b"\x1b8".to_vec()),
        Just(b"\x1b#8".to_vec()),
        Just(b"\x1b#6".to_vec()),
        Just(b"\x1b#3".to_vec()),
        Just(b"\x1b%@".to_vec()),
        Just(b"\x1b%G".to_vec()),
        Just(b"\x9b2A".to_vec()),
        Just(b"\x1b]7;file:///tmp\x07".to_vec()),
        Just(b"\x1b]7;file://elsewhere.example/x\x1b\\".to_vec()),
        Just(b"\x1b]1337;CurrentDir=/srv\x07".to_vec()),
        (0..6usize).prop_map(|i| format!("\x1b]133;{}\x07", char::from(b"ABCDLN"[i])).into_bytes()),
        Just(b"\x1b]8;;file:///etc/passwd\x07".to_vec()),
        Just(b"\x1b]8;;\x07".to_vec()),
        Just(b"\x1b[1;31m".to_vec()),
        Just(b"\x1b[0m".to_vec()),
        Just(b"\x1b[?1049h".to_vec()),
        Just(b"\x1b[?1049l".to_vec()),
        Just(b"\x1b[?47h".to_vec()),
        Just(b"\x1b[?47l".to_vec()),
        Just(b"\x1b[?1047h".to_vec()),
        Just(b"\x1b[?1047l".to_vec()),
        Just(b"\x1b[?40h\x1b[?3h".to_vec()),
        Just(b"\x1b[?3l".to_vec()),
        Just(b"\x1b[!p".to_vec()),
        Just(b"\x1bc".to_vec()),
        Just(b"\x1b[2$~".to_vec()),
        Just(b"\x1b[1$}status\x1b[0$}".to_vec()),
        Just(b"\x1b[0$~".to_vec()),
        Just(b"\x1bPq#0;2;100;0;0#0~~~~-~~\x1b\\".to_vec()),
        Just(b"\x1b_Ga=T,f=24,s=1,v=1;AAAA\x1b\\".to_vec()),
        Just(b"\x1b[?45h".to_vec()),
        Just(b"\x1b[?1045h".to_vec()),
        Just(b"\x1b[1\"q".to_vec()),
        Just(b"\x1b]133;A;redraw=1\x07".to_vec()),
        Just(b"\x1b]133;P;k=s\x07".to_vec()),
        // An APC G left open, as mpv's exit leaves one, and the marks
        // after it: whatever the core reads of them, the record says.
        Just(b"\x1b_Ga=d;".to_vec()),
        Just(b"\x1b]133;D\x07".to_vec()),
        Just(b"\x1b]133;D\x1b\\".to_vec()),
        Just(b"\x1b[?2004h".to_vec()),
        Just(b"\x1b[".to_vec()),
        Just(b"\x1b".to_vec()),
    ]
}

/// A program and where its reads end.
fn program() -> impl Strategy<Value = (Vec<u8>, Vec<usize>)> {
    prop::collection::vec(atom(), 1..40).prop_flat_map(|atoms| {
        let bytes: Vec<u8> = atoms.concat();
        let n = bytes.len();
        (Just(bytes), prop::collection::vec(0..=n, 0..6))
    })
}

/// The reads: `bytes` cut at `cuts`, none empty.
fn reads(bytes: &[u8], cuts: &[usize]) -> Vec<Vec<u8>> {
    let mut at: Vec<usize> = cuts.to_vec();
    at.push(0);
    at.push(bytes.len());
    at.sort_unstable();
    at.dedup();
    at.windows(2).map(|w| bytes[w[0]..w[1]].to_vec()).collect()
}

/// One byte, fed alone where it stands in its read.
fn feed_byte(t: &mut Term<'_>, at: usize, b: u8) {
    t.write_at(BytePos::from_index(at), &[b]);
    t.feed(BytePos::from_index(at), ReadLen::ONE);
}

/// Each read's record, the read fed whole, with what it leaves unsaid.
fn whole(reads: &[Vec<u8>]) -> Vec<(ReadRecord, Unsaid)> {
    with(|t| reads.iter().map(|r| checked(t, r)).collect())
}

/// Each read's record, the read fed a byte at a time, with whether a byte
/// of it ran under ISO 8859-1.
fn bytewise(reads: &[Vec<u8>]) -> Vec<(ReadRecord, bool)> {
    with(|t| {
        reads
            .iter()
            .map(|r| {
                let mut latin1 = false;
                for (i, &b) in r.iter().enumerate() {
                    latin1 |= t.input_encoding() != InputEncoding::Utf8;
                    feed_byte(t, i, b);
                }
                (t.take_record(), latin1)
            })
            .collect()
    })
}

proptest! {
    /// Every line a read changed is named by its record.
    #[test]
    fn every_line_a_read_changed_is_one_its_record_names((bytes, cuts) in program()) {
        let reads = reads(&bytes, &cuts);
        for (i, (rec, unsaid)) in whole(&reads).iter().enumerate() {
            prop_assert!(unsaid.unnamed.is_empty(), "read {i} {:?}: {:?} not in {rec:?}", String::from_utf8_lossy(&reads[i]), unsaid.unnamed);
        }
    }

    /// Every marker a read moved off its line was moved by a move its
    /// record holds.
    #[test]
    fn every_line_a_read_moved_text_onto_is_one_its_moves_name((bytes, cuts) in program()) {
        let reads = reads(&bytes, &cuts);
        for (i, (rec, unsaid)) in whole(&reads).iter().enumerate() {
            prop_assert!(unsaid.displaced.is_empty(), "read {i} {:?}: {:?} unexplained by {rec:?}", String::from_utf8_lossy(&reads[i]), unsaid.displaced);
        }
    }

    /// A byte run under ISO 8859-1 makes the read `Other`, read a byte at
    /// a time.
    #[test]
    fn a_byte_run_under_latin1_makes_the_read_other((bytes, cuts) in program()) {
        let reads = reads(&bytes, &cuts);
        for (i, (rec, latin1)) in bytewise(&reads).iter().enumerate() {
            if *latin1 {
                prop_assert_eq!(rec.encoding(), ReadEncoding::Other, "read {}", i);
            }
        }
    }

    /// **Every mark a record holds is one the core acted on**, and every
    /// one the core acted on is in the record: per read, the record's marks
    /// against the core's own count of the prompt marks it acted on, fed
    /// whole and a byte at a time. Inside an APC G left open, a mark the
    /// core takes as payload is in neither, whichever way the core reads
    /// that string.
    #[test]
    fn the_record_s_marks_are_the_ones_the_core_acted_on((bytes, cuts) in program()) {
        let reads = reads(&bytes, &cuts);
        for bytewise in [false, true] {
            let counts: Vec<(usize, usize, Fill)> = with(|t| {
                reads
                    .iter()
                    .map(|r| {
                        let before = t.semantic_marks;
                        match bytewise {
                            false => {
                                t.write_at(BytePos::ZERO, r);
                                t.feed(BytePos::ZERO, ReadLen::from_index(r.len()));
                            }
                            true => r.iter().enumerate().for_each(|(i, &b)| feed_byte(t, i, b)),
                        }
                        let rec = t.take_record();
                        (rec.marks().iter().count(), t.semantic_marks - before, rec.marks().fill())
                    })
                    .collect()
            });
            for (i, (recorded, acted, fill)) in counts.into_iter().enumerate() {
                match fill {
                    Fill::Room => prop_assert_eq!(recorded, acted, "read {} {:?} bytewise {}", i, String::from_utf8_lossy(&reads[i]), bytewise),
                    Fill::Overflowed => prop_assert!(recorded < acted, "read {} bytewise {}", i, bytewise),
                }
            }
        }
    }

    /// The record of a read fed a byte at a time is the record of it fed
    /// whole.
    #[test]
    fn the_record_does_not_depend_on_how_the_read_arrived((bytes, cuts) in program()) {
        let reads = reads(&bytes, &cuts);
        let whole: Vec<ReadRecord> = whole(&reads).into_iter().map(|(r, _)| r).collect();
        let bytewise: Vec<ReadRecord> = bytewise(&reads).into_iter().map(|(r, _)| r).collect();
        prop_assert_eq!(whole, bytewise);
    }
}

/// **A line a rectangle copy wrote keeps its own text, and lines inserted
/// after carry it on**: `DECCRA` copies rows 1-4 one row down from the
/// second column, so rows 2-5 keep their markers in the first; `IL 3` on
/// row 2 then moves rows 2-5 down three. The record holds the two moves,
/// the copy's replacing part of each line it wrote and the `IL`'s whole
/// (`Replaced`), and a marker now on row 5 came from row 2, which only the
/// `IL` names.
/// The read the property found, by hand to these two operations (the
/// watcher's own bytes are the next test). Break: `DECCRA` recorded as
/// replacing its lines whole (`copy_area`), which folds the two to one move
/// from rows 1-4 down four.
#[test]
fn a_line_a_copy_wrote_keeps_its_text_through_the_lines_inserted_after() {
    let (rec, unsaid) = with(|t| checked(t, b"\x1b[1;2;4;12;1;2;2;1$v\n\x1b[3L"));
    assert!(
        unsaid.displaced.is_empty(),
        "{:?} by {rec:?}",
        unsaid.displaced
    );
    assert!(unsaid.unnamed.is_empty(), "{:?} by {rec:?}", unsaid.unnamed);
    let line = |i: usize| LineIdx::FIRST.plus(Rows::of_count(i));
    let moves: Vec<Move> = rec.moves().iter().collect();
    assert_eq!(
        moves,
        [
            Move::Exact {
                screen: Screen::Normal,
                from: Span::of(line(6), line(9)),
                by: LineShift::Down(Rows::of_count(1)),
                replaced: Replaced::Part,
            },
            Move::Exact {
                screen: Screen::Normal,
                from: Span::of(line(7), line(10)),
                by: LineShift::Down(Rows::of_count(3)),
                replaced: Replaced::Whole,
            },
        ]
    );
}

/// **Lines inserted, then a copy onto them, name where each line's text
/// came from**: `IL 2` on row 2 of a region of rows 1-6 moves rows 2-4 to
/// 4-6, and `DECCRA` copies rows 4-6 one down from the second column, so
/// row 5 keeps the marker the `IL` brought from row 3. Break: as above.
#[test]
fn a_copy_onto_lines_inserted_names_the_lines_they_came_from() {
    let (rec, unsaid) = with(|t| checked(t, b"\x1b[1;6r\x1b[2H\x1b[2L\x1b[4;2;6;12;1;5;2;1$v"));
    assert!(
        unsaid.displaced.is_empty(),
        "{:?} by {rec:?}",
        unsaid.displaced
    );
}

/// **The read the watcher's run of the property found**, on the alternate
/// screen, its bytes as the property printed them: the copy and the lines
/// inserted under it, with a `DECSTBM` of top past bottom, which sets no
/// region and does not move the cursor (`Paint::set_scroll_region`), and
/// an `APC G` left open at the end. Break: as above.
#[test]
fn the_read_the_property_found_names_every_line_its_moves_wrote() {
    let bytes = b"\x1b[?1047h\x1b[0;4;4;8;1;2;9;1$v\r\n\x1b[3L\x1b[?7l\x1b[?7l\x1bD\x1b[8;1r\x1b[13G\x1b]133;D\x1b\\\x1b[2$~\x1b[2'~\x1bPq#0;2;100;0;0#0~~~~-~~\x1b\\\x1b]8;;file:///etc/passwd\x07\xe6\xbc\xa2\xe5\xad\x97\x1b_Ga=d;\x1b[?1049l\x1b[2X";
    let (rec, unsaid) = with(|t| checked(t, bytes));
    assert!(
        unsaid.displaced.is_empty(),
        "{:?} by {rec:?}",
        unsaid.displaced
    );
}

/// **Clearing the alternate screen's moves leaves nothing to chain onto**:
/// a copy on the primary as above, then on the alternate screen an `IL`,
/// which replaces its lines whole, and `?1047 l`, which clears that screen
/// and with it the `IL`'s move. The copy is the last move again, and the
/// `IL 3` on the primary after it stays a move of its own: the markers it
/// carries from rows 2-5 are named by it alone: the copy says itself that
/// it replaced part of each line (`Move::Exact`'s `replaced`), whatever
/// move was cleared after it. Break: a chain folded whatever its moves
/// replaced (`Move::then`).
#[test]
fn a_move_cleared_with_the_alternate_screen_leaves_no_chain_onto_a_copy() {
    let (rec, unsaid) = with(|t| {
        checked(
            t,
            b"\x1b[1;2;4;12;1;2;2;1$v\x1b[?1047h\x1b[2H\x1b[L\x1b[?1047l\x1b[2H\x1b[3L",
        )
    });
    assert!(
        unsaid.displaced.is_empty(),
        "{:?} by {rec:?}",
        unsaid.displaced
    );
    assert_eq!(rec.cleared(), Cleared::Alternate, "the control: {rec:?}");
}

/// The two shrunk cases of `the_record_does_not_depend_on_how_the_read_arrived`
/// (lines 18 and 19 of read_record_props.proptest-regressions), each fed whole and a byte at a time.
fn the_record_is_the_same_bytewise(bytes: &[u8], cuts: &[usize]) {
    let reads = reads(bytes, cuts);
    let whole: Vec<ReadRecord> = whole(&reads).into_iter().map(|(r, _)| r).collect();
    let bytewise: Vec<ReadRecord> = bytewise(&reads).into_iter().map(|(r, _)| r).collect();
    assert_eq!(whole, bytewise);
}

/// **A region move on the alternate screen names the same lines whether
/// its read arrives whole or a byte at a time**: `CSI 0;5 r` on the primary,
/// `?1047h`, `RI` at the region's top and `SU 4` on the alternate screen,
/// then `?1049l`, `DL` on the primary and shifts around wide text -- line
/// 18 of the regression file.
///
/// Named break: copy the first region move of a visit and rotate the
/// rest -- the byte-a-read run rotates where the whole read copied, and
/// the records name other lines.
#[test]
fn a_region_move_after_the_alternate_screen_comes_up_records_alike_bytewise() {
    the_record_is_the_same_bytewise(
        &[
            27, 91, 48, 59, 53, 114, 27, 91, 63, 49, 48, 52, 55, 104, 27, 77, 27, 91, 52, 83, 27,
            91, 55, 68, 27, 91, 63, 49, 48, 52, 57, 108, 27, 91, 48, 65, 27, 91, 49, 77, 27, 91,
            27, 91, 63, 52, 53, 104, 27, 91, 52, 68, 27, 91, 48, 32, 64, 230, 188, 162, 229, 173,
            151, 27, 91, 48, 32, 65, 27, 93, 55, 59, 102, 105, 108, 101, 58, 47, 47, 47, 116, 109,
            112, 7,
        ],
        &[],
    );
}

/// **The prompt's block moved by `IL` on the alternate screen records
/// alike bytewise**: `?47h`, a prompt mark, `IL 1`, a second mark with a
/// redraw, and `?1045h` -- line 19 of the regression file, cut at byte 26; and the same
/// with `CSI 2;6 r` after `?47h`, so the `IL` turns the index.
///
/// Named break: erase the prompt's block by consecutive slots
/// (`Paint::replace_prompt_block`) -- through a turned index the erase
/// lands on other rows, and the records differ.
#[test]
fn a_prompt_block_moved_on_the_alternate_screen_records_alike_bytewise() {
    let seed: &[u8] = &[
        27, 91, 63, 52, 55, 104, 27, 91, 51, 66, 27, 93, 49, 51, 51, 59, 80, 59, 107, 61, 115, 7,
        27, 91, 49, 76, 27, 93, 49, 51, 51, 59, 65, 59, 114, 101, 100, 114, 97, 119, 61, 49, 7,
        116, 101, 120, 116, 12, 27, 91, 63, 49, 48, 52, 53, 104, 27, 91, 52, 59, 49, 59, 52, 59,
        53, 59, 49, 36, 116, 27, 91, 50, 68, 27, 80, 113, 35, 48, 59, 50, 59, 49, 48, 48, 59, 48,
        59, 48, 35, 48, 126, 126, 126, 126, 45, 126, 126, 27, 92,
    ];
    the_record_is_the_same_bytewise(seed, &[26]);
    let mut turned = seed[..6].to_vec();
    turned.extend_from_slice(b"\x1b[2;6r");
    turned.extend_from_slice(&seed[6..]);
    the_record_is_the_same_bytewise(&turned, &[32]);
}
