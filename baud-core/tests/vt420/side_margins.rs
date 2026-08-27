//! DECSLRM left/right margins, and the column operations that ride on them.
//!
//! * `[[source: vt510-decslrm]]`: "sets the left and right margins to define
//!   the scrolling region ... DECSLRM only works when vertical split screen
//!   mode (DECLRMM) is set ... The value of the left margin (Pl) must be less
//!   than the right margin (Pr) ... DECSLRM moves the cursor to column 1,
//!   line 1 of the page."
//! * `[[source: vt510-decic]]`: "As columns are inserted, the columns between
//!   the cursor and the right margin move to the right ... DECIC has no effect
//!   outside the scrolling margins."
//! * `[[source: vt510-decdc]]`: "the remaining columns between the cursor and
//!   the right margin move to the left. The terminal adds blank columns ... at
//!   the right margin."
//!
//! `CSI s` is **ambiguous**: SCOSC normally, DECSLRM when DECLRMM is set. That
//! ambiguity is in the protocol, so the parser is told which mode is in force
//! rather than guessing.

use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, (usize, usize), Vec<u8>) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    let (r, c) = t.cursor();
    (
        t.viewport().to_vec(),
        (r.index(), c.index()),
        t.reply().to_vec(),
    )
}

fn grid(b: &[u8]) -> Vec<PackedCell> {
    run(b).0
}
fn cursor(b: &[u8]) -> (usize, usize) {
    run(b).1
}
fn replies(b: &[u8]) -> Vec<u8> {
    run(b).2
}

/// Enable DECLRMM and set margins at columns 5..10 (1-based).
const LRM: &[u8] = b"\x1b[?69h\x1b[5;10s";

// ── the ambiguity ───────────────────────────────────────────────────────────

/// Without DECLRMM, `CSI s` is SCOSC and saves the cursor.
#[test]
fn csi_s_is_scosc_without_declrmm() {
    // Save at 3;3, move, restore: back to 3;3.
    assert_eq!(cursor(b"\x1b[3;3H\x1b[s\x1b[9;9H\x1b[u"), (2, 2));
}

/// With DECLRMM, the same bytes are DECSLRM and do **not** save the cursor.
#[test]
fn csi_s_is_decslrm_with_declrmm() {
    // `CSI 5;10 s` sets margins and homes the cursor, per the spec.
    assert_eq!(cursor(b"\x1b[?69h\x1b[9;9H\x1b[5;10s"), (0, 0));
}

/// Turning DECLRMM off restores full-width margins.
///
/// Leaving them set while the mode that gives them meaning is off would confine
/// writes with nothing to explain why.
#[test]
fn disabling_declrmm_restores_full_width() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[?69l");
    v.extend(std::iter::repeat(b'x').take(20));
    // With margins gone, 20 characters occupy columns 0..20.
    assert_eq!(cursor(&v), (0, 20));
}

// ── setting them ────────────────────────────────────────────────────────────

/// DECSLRM homes the cursor. `[[source: vt510-decslrm]]`: "moves the cursor to
/// column 1, line 1 of the page."
#[test]
fn decslrm_homes_the_cursor() {
    assert_eq!(cursor(LRM), (0, 0));
}

/// An invalid range leaves the margins alone rather than clamping into
/// something the application did not ask for.
///
/// `[[source: vt510-decslrm]]`: "The value of the left margin (Pl) must be less
/// than the right margin (Pr)."
#[test]
fn an_inverted_range_is_rejected() {
    let mut v = b"\x1b[?69h\x1b[10;5s".to_vec();
    v.extend(std::iter::repeat(b'x').take(20));
    assert_eq!(cursor(&v), (0, 20), "margins unchanged, so full width");
}

/// Equal margins are rejected too: the minimum region is two columns.
#[test]
fn a_zero_width_region_is_rejected() {
    let mut v = b"\x1b[?69h\x1b[5;5s".to_vec();
    v.extend(std::iter::repeat(b'x').take(20));
    assert_eq!(cursor(&v), (0, 20));
}

// ── what the margins confine ────────────────────────────────────────────────

/// Text wraps at the right margin and resumes at the **left** margin.
///
/// Wrapping to column 0 would leave the scrolling region entirely, which is the
/// whole thing side margins exist to prevent.
#[test]
fn text_wraps_within_the_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[1;5H"); // to the left margin
    v.extend(std::iter::repeat(b'x').take(7)); // 6 fit (cols 5..10), 1 wraps
    let (_, (r, c), _) = run(&v);
    assert_eq!(r, 1, "wrapped to the next row");
    assert_eq!(c, 5, "and to the left margin, not column 0");
}

/// With autowrap off the cursor pins to the right margin, not the last column.
#[test]
fn no_wrap_pins_to_the_right_margin() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1b[?7l\x1b[1;5H");
    v.extend(std::iter::repeat(b'x').take(50));
    assert_eq!(cursor(&v), (0, 9), "the right margin is column index 9");
}

/// ICH does not push text past the right margin.
#[test]
fn ich_stops_at_the_right_margin() {
    // Fill the row, set margins, then insert inside them.
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat(b'a').take(20));
    let mut v = base.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b[2@");
    let after = grid(&v);
    let before = grid(&base);
    // Columns *outside* the margins are untouched. The margins are 1-based
    // 5..10, so index 4 is the left margin and is inside -- it gets blanked by
    // the insert, which is the operation working, not a leak.
    for x in [0usize, 1, 2, 3, 10, 11, 19] {
        assert_eq!(after[x], before[x], "column {x} is outside the margins");
    }
    let blank = grid(b"")[0];
    assert_eq!(after[4], blank, "the left margin is inside, and was opened");
    assert_eq!(after[5], blank);
    assert_eq!(after[6], before[4], "old column 4 shifted right by two");
}

/// DCH does not pull text in from beyond the right margin.
#[test]
fn dch_does_not_pull_from_outside_the_margins() {
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat(b'a').take(20));
    let mut v = base.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b[2P");
    let after = grid(&v);
    let blank = grid(b"")[0];
    // The two columns at the right margin become blank, not copies of what is
    // beyond it.
    assert_eq!(after[8], blank, "column 8 blanked at the margin");
    assert_eq!(after[9], blank, "column 9 blanked at the margin");
    assert_eq!(after[10], grid(&base)[10], "column 10 untouched");
}

// ── DECIC / DECDC ───────────────────────────────────────────────────────────

/// DECIC inserts columns within the margins, on every row of the region.
#[test]
fn decic_inserts_columns_within_the_margins() {
    let mut v = b"".to_vec();
    v.extend(std::iter::repeat(b'a').take(20));
    v.extend_from_slice(b"\r\n");
    v.extend(std::iter::repeat(b'b').take(20));
    let base = v.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b['}");
    let after = grid(&v);
    let before = grid(&base);
    let blank = grid(b"")[0];
    assert_eq!(after[5 - 1 + 1 - 1], blank, "a blank column opened at 5");
    // Row 1 moved too: the region is the rectangle, not one line.
    assert_eq!(after[2048 + 4], blank, "row 1 shifted as well");
    assert_eq!(after[0], before[0], "outside the margins, unchanged");
}

/// DECDC deletes columns and fills at the right margin.
#[test]
fn decdc_deletes_columns_and_fills_at_the_margin() {
    let mut v = b"".to_vec();
    v.extend(std::iter::repeat(b'a').take(20));
    let base = v.clone();
    v.extend_from_slice(LRM);
    v.extend_from_slice(b"\x1b[1;5H\x1b['~");
    let after = grid(&v);
    let blank = grid(b"")[0];
    assert_eq!(after[9], blank, "filled at the right margin");
    assert_eq!(after[10], grid(&base)[10], "beyond it, untouched");
}

/// Both are no-ops with the cursor outside the margins.
///
/// `[[source: vt510-decic]]`: "DECIC has no effect outside the scrolling
/// margins."
#[test]
fn the_column_operations_are_inert_outside_the_margins() {
    let mut base = b"".to_vec();
    base.extend(std::iter::repeat(b'a').take(20));
    let mut set = base.clone();
    set.extend_from_slice(LRM);
    let mut ic = set.clone();
    ic.extend_from_slice(b"\x1b[1;1H\x1b[5'}");
    let mut dc = set.clone();
    dc.extend_from_slice(b"\x1b[1;1H\x1b[5'~");
    assert_eq!(grid(&ic), grid(&set), "DECIC outside the margins");
    assert_eq!(grid(&dc), grid(&set), "DECDC outside the margins");
}

// ── DECRQSS ─────────────────────────────────────────────────────────────────

/// DECRQSS reports the margins in a form that can be sent straight back.
#[test]
fn decrqss_reports_the_side_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1bP$qs\x1b\\");
    assert_eq!(replies(&v), b"\x1bP1$r5;10s\x1b\\");
}

/// And the scrolling region.
#[test]
fn decrqss_reports_the_scroll_region() {
    assert_eq!(replies(b"\x1b[3;20r\x1bP$qr\x1b\\"), b"\x1bP1$r3;20r\x1b\\");
}

/// RIS resets the margins.
#[test]
fn ris_resets_the_side_margins() {
    let mut v = LRM.to_vec();
    v.extend_from_slice(b"\x1bc");
    v.extend(std::iter::repeat(b'x').take(20));
    assert_eq!(cursor(&v), (0, 20));
}

/// Margins behave identically however the reads are split.
#[test]
fn side_margins_are_chunk_invariant() {
    let mut seq = LRM.to_vec();
    seq.extend_from_slice(b"\x1b[1;5Habcdefghij\x1b['}");
    let whole = grid(&seq);
    for chunk in 1..seq.len() {
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&seq[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
        assert_eq!(t.viewport().to_vec(), whole, "differed at chunk {chunk}");
    }
}
