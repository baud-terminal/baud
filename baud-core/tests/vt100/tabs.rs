//! Tab stops: HT, HTS, TBC, CHT, CBT.
//!
//! One feature under five names. HT used fixed every-eight arithmetic and there
//! was no stop table at all, so HTS could not set one, TBC could not clear one,
//! and CHT/CBT did not exist. `docs/vt-coverage.md` records tab stops among the
//! operations the differential test against alacritty names as failing.
//!
//! `[[source: ctlseqs]]` § "Functions using CSI" gives CHT/CBT/TBC; TBC's
//! parameter values are there too -- xterm implements 0 and 3.

use crate::support;
use baud_core::*;

fn cursor(bytes: &[u8]) -> (usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

fn col(bytes: &[u8]) -> usize {
    cursor(bytes).1
}

// ── defaults ────────────────────────────────────────────────────────────────

/// A terminal powers on with a stop every eight columns.
#[test]
fn default_stops_are_every_eight_columns() {
    assert_eq!(col(b"\t"), 8);
    assert_eq!(col(b"\t\t"), 16);
    assert_eq!(col(b"\t\t\t"), 24);
}

/// A tab from just before a stop lands on it, not eight past it.
#[test]
fn ht_advances_to_the_next_stop_not_a_fixed_distance() {
    assert_eq!(col(b"\x1b[8G\t"), 8, "column 7 -> next stop is 8");
    assert_eq!(col(b"\x1b[9G\t"), 16, "column 8 is a stop; advance past it");
}

/// HT never leaves the screen.
#[test]
fn ht_stops_at_the_right_margin() {
    assert_eq!(col(b"\x1b[200G\t"), 199);
    assert_eq!(col(b"\x1b[200G\t\t\t\t"), 199);
}

// ── HTS and TBC ─────────────────────────────────────────────────────────────

/// HTS (`ESC H`) sets a stop at the cursor.
#[test]
fn hts_sets_a_stop_at_the_cursor() {
    // Clear everything, set one at column 5 (index 4), tab from column 1.
    assert_eq!(col(b"\x1b[3g\x1b[5G\x1bH\x1b[1G\t"), 4);
}

/// TBC 0 clears the stop under the cursor, and only that one.
#[test]
fn tbc_at_cursor_clears_one_stop() {
    // Stop at index 8 removed, so a tab from 0 runs on to index 16.
    assert_eq!(col(b"\x1b[9G\x1b[0g\x1b[1G\t"), 16);
    // index 16 still present
    assert_eq!(col(b"\x1b[9G\x1b[0g\x1b[1G\t\t"), 24);
}

/// TBC 3 clears every stop, so HT runs to the right margin.
#[test]
fn tbc_all_leaves_no_stops() {
    assert_eq!(col(b"\x1b[3g\t"), 199);
}

/// TBC 1 and 4 clear line stops, never a character stop.
///
/// Silently aliasing one value onto another is how a terminal ends up
/// clearing stops an application asked to keep. `tests/vt100/line_tabs.rs`
/// holds the line stops themselves, and TBC 2 and 5.
#[test]
fn tbc_line_forms_leave_the_character_stops() {
    assert_eq!(
        col(b"\x1b[1g\x1b[1G\t"),
        8,
        "TBC 1 clears no character stop"
    );
    assert_eq!(col(b"\x1b[4g\x1b[1G\t"), 8, "TBC 4 likewise");
}

// ── CHT and CBT ─────────────────────────────────────────────────────────────

/// CHT is HT n times.
#[test]
fn cht_advances_n_stops() {
    assert_eq!(col(b"\x1b[3I"), 24);
    assert_eq!(col(b"\x1b[I"), 8, "default 1");
    assert_eq!(col(b"\t\t\t"), col(b"\x1b[3I"), "CHT 3 == three tabs");
}

/// CBT walks back through the stops.
#[test]
fn cbt_moves_back_n_stops() {
    assert_eq!(
        col(b"\x1b[25G\x1b[Z"),
        16,
        "col 25 -> index 24 -> back to 16"
    );
    assert_eq!(col(b"\x1b[25G\x1b[2Z"), 8);
    assert_eq!(col(b"\x1b[25G\x1b[3Z"), 0);
}

/// CBT stops at column 0 rather than underflowing.
#[test]
fn cbt_stops_at_the_left_margin() {
    assert_eq!(col(b"\x1b[1G\x1b[Z"), 0);
    assert_eq!(col(b"\x1b[5G\x1b[99999Z"), 0);
}

/// CHT with a huge count clamps rather than looping to the margin repeatedly.
#[test]
fn cht_with_a_huge_count_clamps() {
    assert_eq!(col(b"\x1b[99999I"), 199);
}

// ── reset ───────────────────────────────────────────────────────────────────

/// RIS restores the default stops. Without this, a program that cleared them
/// leaves the next one with a terminal whose tabs never come back.
#[test]
fn ris_restores_default_stops() {
    assert_eq!(col(b"\x1b[3g\x1bc\t"), 8);
}

// ── HTJ ─────────────────────────────────────────────────────────────────────

const NARROW: WinSize = WinSize {
    ws_row: 4,
    ws_col: 24,
};

/// Row 0 as text, `.` for an erased position and `_` for a wide tail, and
/// the cursor's column.
fn justified(bytes: &[u8]) -> (String, usize) {
    support::render(&NARROW, bytes, |t| {
        let row: String = t.viewport().row(Row::ZERO)[..24]
            .iter()
            .map(|c| match c.content {
                x if x.is_empty() => '.',
                x if x.is_wide_tail() => '_',
                x => x
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('?'),
            })
            .collect();
        (row, t.cursor().1.index())
    })
}

/// "HTJ causes the contents of the active field ... to be shifted forward
/// so that it ends at the character position preceding the following
/// character tabulation stop. The active presentation position is moved to
/// that following character tabulation stop. The character positions which
/// precede the beginning of the shifted string are put into the erased
/// state" (`[[source: ecma48]]` 8.3.61): `ab` written at the stop at 8
/// ends at 15, before the stop at 16, and the cursor goes there.
#[test]
fn htj_shifts_the_field_to_end_before_the_next_stop() {
    assert_eq!(
        justified(b"\tab\x1bI"),
        ("..............ab........".into(), 16)
    );
    assert_eq!(
        justified(b"xy\x1bI"),
        ("......xy................".into(), 8),
        "the first field starts at the line's start"
    );
    assert_eq!(
        justified(b"xy\tab\x1bI"),
        ("xy............ab........".into(), 16),
        "the field before it stays"
    );
}

/// **The field's contents are its written positions, wherever the cursor
/// is in it** (Jev's lean, WRITTEN_EXTENT): with the cursor moved back over
/// `cd`, all of `abcd` is justified.
#[test]
fn htj_shifts_the_whole_written_field_with_the_cursor_inside_it() {
    assert_eq!(
        justified(b"\tabcd\x1b[2D\x1bI"),
        ("............abcd........".into(), 16)
    );
}

/// An empty field holds nothing to shift: the cursor still goes to the
/// following stop. A field already ending before its stop is left as is.
#[test]
fn htj_on_an_empty_or_full_field_only_moves_the_cursor() {
    assert_eq!(
        justified(b"\t\x1bI"),
        ("........................".into(), 16)
    );
    assert_eq!(
        justified(b"\tabcdefgh\x1b[3D\x1bI"),
        ("........abcdefgh........".into(), 16)
    );
}

/// With no stop following, the field ends at the line's end (`EF`'s field)
/// and the cursor goes where `HT` does, the last column.
#[test]
fn htj_with_no_following_stop_ends_the_field_at_the_line_end() {
    assert_eq!(
        justified(b"\t\t\x1b[3gab\x1bI"),
        ("......................ab".into(), 23)
    );
}

/// A wide character moves whole.
#[test]
fn htj_moves_a_wide_character_whole() {
    assert_eq!(
        justified("\t中\x1bI".as_bytes()),
        ("..............中_........".into(), 16)
    );
}

/// `89` under ISO 8859-1 is HTJ; in VT52 mode `ESC I` is still the
/// reverse line feed.
#[test]
fn htj_as_a_c1_under_latin1_and_not_in_vt52() {
    assert_eq!(
        justified(b"\x1b%@\tab\x89"),
        ("..............ab........".into(), 16)
    );
    support::render(&NARROW, b"\x1b[3;5H\x1b[?2l\x1bI", |t| {
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(4)));
    });
}
