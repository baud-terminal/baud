//! BDSM, `SM 8` / `RM 8`: whether lines are drawn in their bidirectional
//! order.
//!
//! `[[source: ecma48]]` 7.2.1: EXPLICIT, the reset state, "Control
//! functions are performed in the data component or in the presentation
//! component, depending on the setting of the DEVICE COMPONENT SELECT MODE
//! (DCSM)"; IMPLICIT, the set state, "All bi-directional aspects of data
//! are handled by the device itself". The mode is the renderer's to act on
//! (the reordering is display-only); the core records it, answers DECRQM
//! from it, and returns it to the reset state on `RIS`. Every test but the
//! controls fails on a BDSM that is counted and dropped, which is what it
//! was.
mod support;
use baud_core::{BidiSupport, Owned, Term, WinSize};

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 20,
};

/// The mode, the dropped-parameter count and DECRQM's answer for mode 8
/// after `bytes`.
fn after(bytes: &[u8]) -> (BidiSupport, usize, Vec<u8>) {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, bytes);
    let (mode, dropped) = (t.bidi_support(), t.unsupported_param);
    t.clear_reply();
    support::feed(&mut t, b"\x1b[8$p");
    (mode, dropped, t.reply().to_vec())
}

#[test]
fn bdsm_starts_explicit_and_decrqm_answers_reset() {
    assert_eq!(
        after(b""),
        (BidiSupport::Explicit, 0, b"\x1b[8;2$y".to_vec())
    );
}

#[test]
fn sm_8_sets_implicit_and_rm_8_resets_it() {
    assert_eq!(
        after(b"\x1b[8h"),
        (BidiSupport::Implicit, 0, b"\x1b[8;1$y".to_vec())
    );
    assert_eq!(
        after(b"\x1b[8h\x1b[8l"),
        (BidiSupport::Explicit, 0, b"\x1b[8;2$y".to_vec())
    );
}

/// A parameter list sets each mode it names: BDSM beside IRM.
#[test]
fn bdsm_in_a_list_is_set_with_its_neighbours() {
    assert_eq!(after(b"\x1b[4;8h").0, BidiSupport::Implicit);
}

/// `RIS` returns it to the reset state, ECMA-48 8.3.105's "set the modes
/// into the reset state"; `DECSTR` keeps it, as its DEC table names no
/// BDSM.
#[test]
fn ris_resets_bdsm_and_decstr_keeps_it() {
    assert_eq!(after(b"\x1b[8h\x1bc").0, BidiSupport::Explicit);
    assert_eq!(after(b"\x1b[8h\x1b[!p").0, BidiSupport::Implicit);
}

/// The control beside it: DCSM, mode 9, is not built, so it is counted and
/// DECRQM says it is not recognised rather than answering a state.
#[test]
fn dcsm_stays_counted_and_not_recognised() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1b[9h");
    assert_eq!(t.unsupported_param, 1);
    t.clear_reply();
    support::feed(&mut t, b"\x1b[9$p");
    assert_eq!(t.reply(), b"\x1b[9;0$y");
}
