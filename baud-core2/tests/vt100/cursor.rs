//! Cursor positioning: CNL, CPL, CHA, HPA, VPA, HPR, VPR, SCOSC, SCORC.
//!
//! Every one of these has a default parameter and a clamping rule, and both are
//! where they usually go wrong. `[[source: ctlseqs]]` gives the defaults;
//! `[[source: ecma48]]` § 8.3 gives the coordinate conventions -- notably that
//! the wire is 1-based while every internal index here is 0-based, which is the
//! off-by-one these tests exist to pin.

use baud_core2::*;

/// The first row of an 8-column screen, so "the last column" has an index a
/// test can name.
fn narrow_row(bytes: &[u8]) -> Vec<PackedCell> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    t.resize(&WinSize {
        ws_row: 4,
        ws_col: 8,
    });
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
    t.viewport()[..8].to_vec()
}

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
/// what xterm does when DECLRMM is disabled, which is baud's only mode since it
/// has no left/right margins.
#[test]
fn scosc_and_decsc_share_a_slot() {
    assert_eq!(cursor(b"\x1b[7;13H\x1b[s\x1b[1;1H\x1b8"), (6, 12));
    assert_eq!(cursor(b"\x1b[7;13H\x1b7\x1b[1;1H\x1b[u"), (6, 12));
}

// ── the cursor at the right edge, however the text arrived ──────────────────

/// **With DECAWM reset the cursor stops on the last column, not past it.**
///
/// The column one past the last is the *pending wrap* sentinel, and it is only
/// a legal place for the cursor when there is a wrap to pend. `put_run` knew
/// that and `put_glyph` did not, so text written a byte at a time -- which is
/// every non-ASCII character -- left the cursor one column further along than
/// the same text taken by the run scanner. Invisible in `cursor()`, which
/// clamps; visible to the next operation that acts at the cursor.
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
