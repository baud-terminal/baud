//! Cursor positioning: CNL, CPL, CHA, HPA, VPA, HPR, VPR, SCOSC, SCORC.
//!
//! Every one of these has a default parameter and a clamping rule, and both are
//! where they usually go wrong. `[[source: ctlseqs]]` gives the defaults;
//! `[[source: ecma48]]` § 8.3 gives the coordinate conventions -- notably that
//! the wire is 1-based while every internal index here is 0-based, which is the
//! off-by-one these tests exist to pin.

use crate::support;
use baud_core::*;

/// The first row of an 8-column screen, so "the last column" has an index a
/// test can name.
fn narrow_row(bytes: &[u8]) -> Vec<PackedCell> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    // **Asserted, not discarded.** `Resized::Truncated` means the geometry
    // exceeded `MAX_COLS`/`MAX_ROWS`, and swallowing it is the silent-clamp
    // bug the return value was added to catch -- a test that clamped without
    // noticing would then measure the wrong grid.
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 4,
            ws_col: 8,
        }),
        Resized::Ok,
        "4x8 is well inside the fixed viewport"
    );
    support::feed(&mut t, bytes);
    t.viewport()[..8].to_vec()
}

fn cursor(bytes: &[u8]) -> (usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

// ── absolute positioning ────────────────────────────────────────────────────

/// CHA and HPA are the same operation under two names, and both are 1-based.
#[test]
fn cha_and_hpa_set_the_column_and_agree() {
    assert_eq!(
        cursor(b"\x1b[5;1H\x1b[9G"),
        (4, 8),
        "CHA: row kept, col 9 -> 8"
    );
    assert_eq!(cursor(b"\x1b[5;1H\x1b[9`"), (4, 8), "HPA must match CHA");
}

/// VPA sets the row and leaves the column alone. That is what distinguishes it
/// from CUP, and from CNL.
#[test]
fn vpa_sets_the_row_and_keeps_the_column() {
    assert_eq!(cursor(b"\x1b[3;7H\x1b[9d"), (8, 6));
}

/// An omitted parameter is 1, not 0. `CSI G` is column 1.
#[test]
fn absolute_moves_default_to_one() {
    assert_eq!(cursor(b"\x1b[5;9H\x1b[G"), (4, 0), "CHA default");
    assert_eq!(cursor(b"\x1b[5;9H\x1b[d"), (0, 8), "VPA default");
    assert_eq!(cursor(b"\x1b[5;9H\x1b[`"), (4, 0), "HPA default");
}

/// An explicit 0 is also 1: `[[source: ctlseqs]]` treats 0 and omitted alike
/// for cursor addressing.
#[test]
fn an_explicit_zero_behaves_as_one() {
    assert_eq!(cursor(b"\x1b[5;9H\x1b[0G"), (4, 0));
    assert_eq!(cursor(b"\x1b[5;9H\x1b[0d"), (0, 8));
}

/// Absolute moves clamp to the screen rather than wrapping or panicking.
#[test]
fn absolute_moves_clamp_to_the_screen() {
    assert_eq!(cursor(b"\x1b[99999d").0, 49, "row clamps to the last row");
    assert_eq!(
        cursor(b"\x1b[99999G").1,
        199,
        "column clamps to the last column"
    );
}

// ── relative positioning ────────────────────────────────────────────────────

/// HPR is CUF by another name: right n columns.
#[test]
fn hpr_moves_right() {
    assert_eq!(cursor(b"\x1b[1;5H\x1b[3a"), (0, 7));
    assert_eq!(cursor(b"\x1b[1;5H\x1b[a"), (0, 5), "default 1");
}

/// VPR moves down and keeps the column. CNL moves down and does not.
#[test]
fn vpr_keeps_the_column_and_cnl_does_not() {
    assert_eq!(cursor(b"\x1b[1;5H\x1b[3e"), (3, 4), "VPR");
    assert_eq!(cursor(b"\x1b[1;5H\x1b[3E"), (3, 0), "CNL");
}

/// CPL is CNL upwards.
#[test]
fn cpl_moves_up_to_column_one() {
    assert_eq!(cursor(b"\x1b[9;5H\x1b[3F"), (5, 0));
    assert_eq!(cursor(b"\x1b[9;5H\x1b[F"), (7, 0), "default 1");
}

/// Relative moves clamp at the edges rather than wrapping.
#[test]
fn relative_moves_clamp_at_the_edges() {
    assert_eq!(cursor(b"\x1b[1;1H\x1b[99999F"), (0, 0), "CPL at the top");
    assert_eq!(cursor(b"\x1b[1;1H\x1b[99999E").0, 49, "CNL at the bottom");
    assert_eq!(cursor(b"\x1b[1;1H\x1b[99999a").1, 199, "HPR at the right");
    assert_eq!(cursor(b"\x1b[1;1H\x1b[99999e").0, 49, "VPR at the bottom");
}

// ── save / restore ──────────────────────────────────────────────────────────

/// SCOSC/SCORC round-trip the position.
#[test]
fn scosc_and_scorc_round_trip() {
    assert_eq!(cursor(b"\x1b[7;13H\x1b[s\x1b[1;1H\x1b[u"), (6, 12));
}

/// SCORC with nothing saved must not move the cursor or panic.
#[test]
fn scorc_without_a_save_is_inert() {
    assert_eq!(cursor(b"\x1b[4;9H\x1b[u"), (3, 8));
}

/// `CSI s` and `ESC 7` write the same slot, so `ESC 8` restores either. That is
/// what xterm does when DECLRMM is disabled; with it set `CSI s` is DECSLRM
/// (`tests/vt420/side_margins.rs`), and with a parameter that is not a default it saves nothing
/// (`save_params.rs`).
#[test]
fn scosc_and_decsc_share_a_slot() {
    assert_eq!(cursor(b"\x1b[7;13H\x1b[s\x1b[1;1H\x1b8"), (6, 12));
    assert_eq!(cursor(b"\x1b[7;13H\x1b7\x1b[1;1H\x1b[u"), (6, 12));
}

// ── the cursor at the right edge, however the text arrived ──────────────────

/// **With DECAWM reset the cursor stops on the last column, not past it.**
///
/// The column one past the last is the *pending wrap* sentinel, and it is only
/// a legal place for the cursor when there is a wrap to pend. `baud-core2`'s
/// `put_run` knew that and its `put_glyph` did not, so text written a byte
/// at a time -- which on that core was every non-ASCII character -- left
/// the cursor one column further along than the same text taken by the run
/// scanner. Invisible in `cursor()`, which clamps; visible to the next
/// operation that acts at the cursor.
#[test]
fn autowrap_off_leaves_the_cursor_on_the_last_column() {
    // DECAWM off, cursor to the last column, more text than fits, then `ICH` --
    // which inserts a blank *at the cursor* and so reports where it actually
    // is. With the cursor one past the last column ICH acts off the row and
    // the `!` survives.
    let v = narrow_row(b"\x1b[?7l\x1b[1;999H!!!\x1b[@");
    let last = v.len() - 1;
    assert!(
        v[last].content.is_empty(),
        "ICH must blank the last column, which it can only do if the cursor \
         is on it rather than one past"
    );
}

/// `CUU`, `CUD` and `CUB` -- the three the coverage gate found nothing for.
///
/// All three are implemented and every ported suite *uses* them; none named
/// them, so `every_done_row_is_mentioned_by_a_test` counted three `done` rows
/// with no test behind them. That is the gate working: a row moves to **done**
/// only when a test exists that would fail if the operation regressed, and
/// "some other test happens to send the sequence" is not that test.
///
/// `[[source: ctlseqs]]` (lines 592-597): each defaults to 1. Each stops at
/// the edge rather than wrapping, and `CUU` stops at the top margin from at
/// or below it, `CUD` at the bottom margin from at or above it, DECOM or
/// not: `[[source: dec-std-070]]` CUU and CUD, notes 1 and 2 (pp. 5-41,
/// 5-43), which `tests/vt100/cuu_cud.rs` holds at every margin.
#[test]
fn cuu_cud_and_cub_move_and_stop_at_the_edges() {
    support::with_corpus(|t| {
        // Defaults are 1.
        support::feed(t, b"\x1b[10;10H\x1b[A");
        assert_eq!(t.cursor(), (Row::from_index(8), Col::at_index(9)), "CUU 1");
        support::feed(t, b"\x1b[B");
        assert_eq!(t.cursor(), (Row::from_index(9), Col::at_index(9)), "CUD 1");
        support::feed(t, b"\x1b[D");
        assert_eq!(t.cursor(), (Row::from_index(9), Col::at_index(8)), "CUB 1");

        // Counts, and the edges clamp rather than wrap.
        support::feed(t, b"\x1b[10;10H\x1b[3A");
        assert_eq!(t.cursor().0, Row::from_index(6), "CUU 3");
        support::feed(t, b"\x1b[5B");
        assert_eq!(t.cursor().0, Row::from_index(11), "CUD 5");
        support::feed(t, b"\x1b[4D");
        assert_eq!(t.cursor().1, Col::at_index(5), "CUB 4");

        support::feed(t, b"\x1b[1;1H\x1b[99A");
        assert_eq!(t.cursor().0, Row::ZERO, "CUU stops at the top");
        support::feed(t, b"\x1b[99D");
        assert_eq!(t.cursor().1, Col::ZERO, "CUB stops at the left");
        support::feed(t, b"\x1b[999B");
        assert_eq!(t.cursor().0, Row::from_index(49), "CUD stops at the bottom");
    });

    // **A scrolling region bounds them with DECOM reset too.** This
    // assertion read "without DECOM the region does not bound CUD", after
    // core2, whose `confine` clamped only under Origin mode, and no text was
    // read for it. `[[source: dec-std-070]]` CUD, note 1 (p. 5-43): from "at
    // or above the Bottom Margin ... The Active position will not move
    // beyond the Bottom Margin", and its algorithm tests no mode; CUU's note
    // 1 (p. 5-41) is the mirror.
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[5;20r\x1b[10;1H\x1b[99B");
        assert_eq!(
            t.cursor().0,
            Row::from_index(19),
            "without DECOM CUD stops at the region's bottom"
        );
        support::feed(t, b"\x1b[99A");
        assert_eq!(t.cursor().0, Row::from_index(4), "and CUU at its top");
    });
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[5;20r\x1b[?6h\x1b[6;1H\x1b[99B");
        assert_eq!(
            t.cursor().0,
            Row::from_index(19),
            "with DECOM it stops at the region's bottom"
        );
        support::feed(t, b"\x1b[99A");
        assert_eq!(t.cursor().0, Row::from_index(4), "and at its top");
    });
}
