//! SM and RM take a list: `CSI Pm h` and `CSI Pm l`.
//!
//! `[[source: ecma48]]` § 8.3.125 SM, "Notation: (Ps...) ... SM causes the
//! modes of the receiving device to be set as specified by the parameter
//! values", and § 8.3.106 RM, the same for reset. `[[source: ctlseqs]]`
//! spells both `CSI Pm h` / `CSI Pm l`. xterm's `ansi_modes` walks every
//! parameter (`[[source: xterm-charproc-c-bccd466]]`). baud read only the
//! first, so `CSI 20 ; 4 h` set LNM and left IRM off without a word.
//!
//! DECSET, DECRST, XTSAVE and XTRESTORE already walked the list; their tests
//! here hold that, a mode list in an order that matters.

use crate::support;
use baud_core::*;

fn run(bytes: &[u8]) -> (Vec<PackedCell>, Vec<u8>, u64) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    (
        support::screen(&t),
        t.reply().to_vec(),
        t.unsupported_param as u64,
    )
}

/// The first row's first eight cells, where every case here writes.
fn grid(bytes: &[u8]) -> Vec<PackedCell> {
    run(bytes).0[..8].to_vec()
}

/// The DECRQM answers for IRM and LNM after `bytes`.
fn irm_lnm(bytes: &[u8]) -> Vec<u8> {
    let mut b = bytes.to_vec();
    b.extend_from_slice(b"\x1b[4$p\x1b[20$p");
    let (_, reply, _) = run(&b);
    reply
}

/// The second mode of `CSI 20 ; 4 h` is set: IRM inserts, and DECRQM says
/// both are set.
#[test]
fn sm_sets_every_mode_in_its_list() {
    assert_eq!(
        grid(b"abcd\x1b[1;1H\x1b[20;4hXY"),
        grid(b"XYabcd"),
        "IRM, the second parameter, must insert"
    );
    assert_eq!(irm_lnm(b"\x1b[20;4h"), b"\x1b[4;1$y\x1b[20;1$y");
    assert_eq!(irm_lnm(b"\x1b[4;20h"), b"\x1b[4;1$y\x1b[20;1$y");
}

/// And `CSI 20 ; 4 l` resets both.
#[test]
fn rm_resets_every_mode_in_its_list() {
    assert_eq!(
        irm_lnm(b"\x1b[4h\x1b[20h\x1b[20;4l"),
        b"\x1b[4;2$y\x1b[20;2$y"
    );
    assert_eq!(
        grid(b"\x1b[4h\x1b[20habcd\x1b[1;1H\x1b[20;4lXY"),
        grid(b"XYcd"),
        "IRM reset by the second parameter must overwrite"
    );
}

/// A mode baud does not act on is counted, a parameter at a time, and the
/// ones after it still apply.
#[test]
fn an_unrecognised_mode_in_the_list_is_counted_and_the_rest_apply() {
    let (_, _, base) = run(b"");
    let (_, _, n) = run(b"\x1b[4;99;20h");
    assert_eq!(n - base, 1, "99 alone is counted");
    assert_eq!(irm_lnm(b"\x1b[4;99;20h"), b"\x1b[4;1$y\x1b[20;1$y");
    let (_, _, n) = run(b"\x1b[99;98l");
    assert_eq!(n - base, 2, "each unrecognised parameter is counted");
}

/// A hole is an omitted parameter, which SM has no default for: counted, and
/// the parameters around it apply.
#[test]
fn a_hole_in_the_list_is_counted_and_the_rest_apply() {
    let (_, _, base) = run(b"");
    let (_, _, n) = run(b"\x1b[4;;20h");
    assert_eq!(n - base, 1);
    assert_eq!(irm_lnm(b"\x1b[4;;20h"), b"\x1b[4;1$y\x1b[20;1$y");
}

/// A bare `CSI h` names no mode and is counted once, as before.
#[test]
fn a_bare_sm_is_counted_once() {
    let (_, _, base) = run(b"");
    let (_, _, n) = run(b"\x1b[h");
    assert_eq!(n - base, 1);
    assert_eq!(irm_lnm(b"\x1b[h"), b"\x1b[4;2$y\x1b[20;2$y");
}

/// DECSET walks its list in order: the last tracking level named wins, so
/// the two orders differ.
#[test]
fn decset_applies_its_list_in_order() {
    let ask = b"\x1b[?1000$p\x1b[?1002$p";
    let mut a = b"\x1b[?1000;1002h".to_vec();
    a.extend_from_slice(ask);
    let mut b = b"\x1b[?1002;1000h".to_vec();
    b.extend_from_slice(ask);
    assert_eq!(run(&a).1, b"\x1b[?1000;2$y\x1b[?1002;1$y");
    assert_eq!(run(&b).1, b"\x1b[?1000;1$y\x1b[?1002;2$y");
}

/// XTSAVE and XTRESTORE walk theirs: both modes saved set, reset by one
/// DECRST, come back.
#[test]
fn xtsave_and_xtrestore_walk_their_lists() {
    let (_, reply, _) = run(b"\x1b[?7;25s\x1b[?25;7l\x1b[?7;25r\x1b[?7$p\x1b[?25$p");
    assert_eq!(reply, b"\x1b[?7;1$y\x1b[?25;1$y");
}
