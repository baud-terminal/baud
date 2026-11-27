//! BDSM, `SM 8` / `RM 8`: whether lines are drawn in their bidirectional
//! order.
//!
//! `[[source: ecma48]]` 7.2.1: EXPLICIT, the reset state, "Control
//! functions are performed in the data component or in the presentation
//! component, depending on the setting of the DEVICE COMPONENT SELECT MODE
//! (DCSM)"; IMPLICIT, the set state, "All bi-directional aspects of data
//! are handled by the device itself". The mode is the renderer's to act on
//! (the reordering is display-only); the core records it, answers DECRQM
//! from it, and returns it to its initial state, set, on `RIS`. Every test
//! but the controls fails on a BDSM that is counted and dropped, which is
//! what it was.
//!
//! **Set at power-on**. `[[source: ecma48]]` 7.1 recommends the reset
//! state as the initial one; Annex D item 11 leaves "the initial state of
//! a device upon power up including the settings of the modes" to the
//! implementation.
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
fn bdsm_starts_implicit_and_decrqm_answers_set() {
    assert_eq!(
        after(b""),
        (BidiSupport::Implicit, 0, b"\x1b[8;1$y".to_vec())
    );
}

#[test]
fn sm_8_sets_implicit_and_rm_8_resets_it() {
    assert_eq!(
        after(b"\x1b[8l\x1b[8h"),
        (BidiSupport::Implicit, 0, b"\x1b[8;1$y".to_vec())
    );
    assert_eq!(
        after(b"\x1b[8l"),
        (BidiSupport::Explicit, 0, b"\x1b[8;2$y".to_vec())
    );
}

/// A parameter list sets and resets each mode it names: BDSM beside IRM.
#[test]
fn bdsm_in_a_list_is_set_with_its_neighbours() {
    assert_eq!(after(b"\x1b[8l\x1b[4;8h").0, BidiSupport::Implicit);
    assert_eq!(after(b"\x1b[4;8l").0, BidiSupport::Explicit);
}

/// `RIS` returns it to its initial state, set: ECMA-48 8.3.105, "reset to
/// its initial state, i.e. the state it has after it is made operational",
/// where setting the modes into the reset state is one of what that "may
/// imply, if applicable". `DECSTR` keeps it either way, as its DEC table
/// names no BDSM.
#[test]
fn ris_returns_bdsm_to_set_and_decstr_keeps_it() {
    assert_eq!(after(b"\x1b[8l\x1bc").0, BidiSupport::Implicit);
    assert_eq!(after(b"\x1b[8l\x1b[!p").0, BidiSupport::Explicit);
    assert_eq!(after(b"\x1b[!p").0, BidiSupport::Implicit);
}

/// **DCSM, mode 9, beside it**: acted on and not counted, and DECRQM
/// answers DATA's state while BDSM is set ("it is considered to be set to
/// DATA", `[[source: ecma48]]` 7.3.4 c), the state set once BDSM is reset:
/// PRESENTATION, the reset state, at power-on. Until now it was counted and
/// answered `0`. The rest is `crates/baud-core/tests/device_component.rs`.
#[test]
fn dcsm_answers_data_while_bdsm_is_set() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let ask = |t: &mut Term<'_>| {
        t.clear_reply();
        support::feed(t, b"\x1b[9$p");
        t.reply().to_vec()
    };
    assert_eq!(ask(&mut t), b"\x1b[9;1$y", "BDSM set: DATA");
    support::feed(&mut t, b"\x1b[8l");
    assert_eq!(ask(&mut t), b"\x1b[9;2$y", "PRESENTATION at power-on");
    support::feed(&mut t, b"\x1b[9h");
    assert_eq!(ask(&mut t), b"\x1b[9;1$y");
    support::feed(&mut t, b"\x1b[9l\x1b[8h");
    assert_eq!(ask(&mut t), b"\x1b[9;1$y", "BDSM set again");
    support::feed(&mut t, b"\x1b[8l");
    assert_eq!(ask(&mut t), b"\x1b[9;2$y", "the state kept");
    assert_eq!(t.unsupported_param, 0);
}
