//! IRM — Insert/Replace Mode, `CSI 4 h` / `CSI 4 l`.
//!
//! `[[source: ecma48]]` § 8.3.64: with IRM set, a graphic character is
//! *inserted* at the cursor and the characters from there to the end of the
//! line move right; with it reset the character replaces what is there.
//!
//! **This was a live bug, not a missing feature.** `ModeBit::InsertReplace` was
//! set by SM/RM and reported by DECRQM, and nothing in the write path read it.
//! An application sent `CSI 4 h`, wrote text expecting insertion, got
//! overwrite — and DECRQM confirmed the mode was on, so it had no way to find
//! out. That is two of the three rules `docs/vt-coverage.md` calls
//! non-negotiable broken at once: *never record a mode you do not act on*, and
//! *never answer a query with the wrong shape*.

use crate::support;
use baud_core::*;

fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    support::screen(&t)
}

fn replies(bytes: &[u8]) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));
    t.reply().to_vec()
}

/// The whole point: text written under IRM pushes what follows to the right.
#[test]
fn insert_mode_shifts_the_rest_of_the_line() {
    // Write "abcd", home, then insert "XY".
    assert_eq!(
        grid(b"abcd\x1b[1;1H\x1b[4hXY"),
        grid(b"XYabcd"),
        "the tail must move right, not be overwritten"
    );
}

/// Reset is overwrite, which is the default and the behaviour before this fix.
#[test]
fn replace_mode_overwrites() {
    assert_eq!(grid(b"abcd\x1b[1;1H\x1b[4lXY"), grid(b"XYcd"));
    assert_eq!(grid(b"abcd\x1b[1;1HXY"), grid(b"XYcd"), "reset by default");
}

/// `CSI 4 l` restores overwrite after `CSI 4 h`.
#[test]
fn the_mode_can_be_turned_off_again() {
    assert_eq!(grid(b"abcd\x1b[1;1H\x1b[4h\x1b[4lXY"), grid(b"XYcd"));
}

/// Inserting mid-line moves only the tail, and leaves the head alone.
#[test]
fn inserting_mid_line_moves_only_the_tail() {
    assert_eq!(grid(b"abcd\x1b[1;3H\x1b[4hZ"), grid(b"abZcd"));
}

/// At end of line there is nothing to shift, so insert and replace agree.
#[test]
fn inserting_at_the_end_matches_replace() {
    assert_eq!(grid(b"ab\x1b[4hcd"), grid(b"abcd"));
}

/// Text pushed past the right margin falls off; it does not wrap to row 2.
///
/// This is the property that separates IRM from a naive "shift everything":
/// the line is a fixed width and the far end is lost.
#[test]
fn text_pushed_past_the_end_is_discarded() {
    let long: Vec<u8> = std::iter::repeat_n(b'x', 200).collect();
    let mut v = long.clone();
    v.extend_from_slice(b"\x1b[1;1H\x1b[4hZ");
    let after = grid(&v);
    // Row 1 must still be blank — nothing wrapped onto it.
    let blank = grid(b"")[0];
    // Row 1 starts one stride in, and the stride is `Term::stride` here
    // rather than `MAX_COLS`. Reading `after[2048]` -- which is what
    // `baud-core2`'s suite did -- lands on row 10 of this grid.
    let stride = support::with_corpus(|t| t.stride().index());
    assert_eq!(after[stride], blank, "nothing may spill onto the next row");
}

/// **IRM respects DECSLRM: the shift stops at the right margin.** On
/// `abcdefghijklmnopqrst` with the margins at columns 5 to 10, a `Z`
/// written at column 5 moves `e` to `i` one right and `j`, shifted past
/// the right margin, is lost; columns 11 to 20, and the blank column 21
/// past the text, keep what they held (`[[source: dec-std-070]]`
/// pp. 5-139, 5-140: "Data shifted past the Right Margin within the
/// Scroll Area ... is lost"). The cursor goes on to column 6.
#[test]
fn insert_stops_at_the_right_margin() {
    let bytes = b"abcdefghijklmnopqrst\x1b[?69h\x1b[5;10s\x1b[1;5H\x1b[4hZ";
    assert_eq!(
        grid(bytes),
        grid(b"abcdZefghiklmnopqrst"),
        "inside the margins the line shifts right and `j` is lost; outside them nothing moves"
    );
    let cursor = support::with_corpus(|t| {
        support::feed(t, bytes);
        t.cursor()
    });
    assert_eq!(cursor, (Row::from_index(0), Col::at_index(5)));
}

/// It applies to the bulk path too, not only to single characters.
///
/// Plain ASCII runs never reach the per-codepoint path, so implementing IRM
/// only there would leave it broken for essentially all real text.
#[test]
fn insert_mode_applies_to_bulk_runs() {
    assert_eq!(grid(b"abcdef\x1b[1;1H\x1b[4hXYZ"), grid(b"XYZabcdef"));
}

/// A double-width character opens a two-cell gap, not one.
#[test]
fn a_wide_character_inserts_two_cells() {
    let mut v = "ab\x1b[1;1H\x1b[4h".as_bytes().to_vec();
    v.extend_from_slice("日".as_bytes());
    assert_eq!(grid(&v), grid("日ab".as_bytes()));
}

/// DECRQM agreed with the broken behaviour before; it must agree with the
/// working one now.
#[test]
fn decrqm_still_reports_the_mode() {
    assert_eq!(replies(b"\x1b[4$p"), b"\x1b[4;2$y", "reset at power-on");
    assert_eq!(replies(b"\x1b[4h\x1b[4$p"), b"\x1b[4;1$y");
    assert_eq!(replies(b"\x1b[4h\x1b[4l\x1b[4$p"), b"\x1b[4;2$y");
}

/// RIS and DECSTR return to replace mode, so a crashed program cannot leave the
/// next one inserting.
#[test]
fn resets_restore_replace_mode() {
    assert_eq!(
        grid(b"abcd\x1b[1;1H\x1b[4h\x1bcabcd\x1b[1;1HXY"),
        grid(b"XYcd")
    );
    assert_eq!(grid(b"abcd\x1b[1;1H\x1b[4h\x1b[!pXY"), grid(b"XYcd"));
}

/// Insert mode parses the same however the reads are split.
#[test]
fn insert_mode_is_chunk_invariant() {
    let seq = b"abcdef\x1b[1;1H\x1b[4hXYZ\x1b[4lQ";
    let whole = grid(seq);
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole, "differed at chunk {chunk}");
    }
}

/// **ICH leaves the cursor where it is**: DEC STD 070 p. 5-142 note 3,
/// "The Active Position is not affected by execution of this control", and
/// xterm, where `[[source: ecma48]]` 8.3.64 moves it to the line home; a
/// curses program sends `ich1` "just before sending the character to be
/// inserted" (terminfo(5)). DEC's reading is built, and recorded as a
/// departure.
#[test]
fn ich_leaves_the_cursor_where_it_is() {
    let at = |bytes: &[u8]| {
        support::render(&support::CORPUS, bytes, |t| {
            let (r, c) = t.cursor();
            (r.index(), c.index())
        })
    };
    assert_eq!(at(b"abcdef\x1b[1;3H\x1b[2@"), (0, 2));
    assert_eq!(
        at(b"\x1b[?69h\x1b[3;10s\x1b[1;5H\x1b[@"),
        (0, 4),
        "inside margins"
    );
    assert_eq!(
        at(b"\x1b[10 Uabcdef\x1b[1;3H\x1b[2@"),
        (0, 2),
        "with a line home SLH set"
    );
}
