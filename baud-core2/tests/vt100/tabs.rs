//! Tab stops: HT, HTS, TBC, CHT, CBT.
//!
//! One feature under five names. HT used fixed every-eight arithmetic and there
//! was no stop table at all, so HTS could not set one, TBC could not clear one,
//! and CHT/CBT did not exist. `docs/vt-coverage.md` records tab stops among the
//! operations the differential test against alacritty names as failing.
//!
//! `[[source: ctlseqs]]` § "Functions using CSI" gives CHT/CBT/TBC; TBC's
//! parameter values are there too -- xterm implements 0 and 3.

use baud_core2::*;

fn cursor(bytes: &[u8]) -> (usize, usize) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
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

/// A TBC value xterm does not implement is ignored rather than treated as 0.
///
/// Silently aliasing an unknown parameter onto a known one is how a terminal
/// ends up clearing stops an application asked to keep.
#[test]
fn tbc_with_an_unimplemented_parameter_is_inert() {
    assert_eq!(col(b"\x1b[1g\x1b[1G\t"), 8, "TBC 1 must not clear anything");
    assert_eq!(col(b"\x1b[5g\x1b[1G\t"), 8, "TBC 5 likewise");
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
