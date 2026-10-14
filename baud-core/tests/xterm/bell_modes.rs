//! `?1042` and `?1043`, the bell's two modes.
//!
//! `[[source: ctlseqs]]`, DECSET: "Ps = 1 0 4 2 -> Enable Urgency window
//! manager hint when Control-G is received, xterm. This enables the
//! bellIsUrgent resource." and "Ps = 1 0 4 3 -> Enable raising of the window
//! when Control-G is received, xterm. This enables the popOnBell resource."
//! DECRST disables each. The window is the embedder's, so the core acts on
//! them by exposing them (`Term::bell_is_urgent`, `Term::pop_on_bell`) beside
//! the count of `BEL`s they act on (`Term::bells`); the macOS app reads all
//! three (`baud-app/src/bell.rs`, `ring`).
//!
//! Off at power-on, as xterm's resources default (`Bres(XtNbellIsUrgent, ...,
//! False)`, `Bres(XtNpopOnBell, ..., False)`, `[[source: xterm-reset]]`'s
//! file). **`RIS` resets both** -- ECMA-48 § 8.3.105, "set the modes into the
//! reset state" (`[[source: ecma48]]`) -- where xterm keeps them, no
//! `ReallyReset` path in that file touching either; `DECSTR` keeps them.

use crate::support;
use baud_core::*;

fn feed(t: &mut Term, b: &[u8]) {
    t.write_at(BytePos::ZERO, b);
    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
}

/// The two modes as the embedder reads them, the `DECRQM` answers for both,
/// and how many parameters were counted as unsupported, after `program`
/// arrives in reads of `size` bytes.
fn after(program: &[u8], size: usize) -> (BellIsUrgent, PopOnBell, Vec<u8>, usize) {
    let mut s = Owned::new(&support::CORPUS);
    let mut t = Term::new(s.storage(), &support::CORPUS);
    for chunk in program.chunks(size.max(1)) {
        feed(&mut t, chunk);
    }
    let counted = t.unsupported_param;
    t.clear_reply();
    feed(&mut t, b"\x1b[?1042$p\x1b[?1043$p");
    (
        t.bell_is_urgent(),
        t.pop_on_bell(),
        t.reply().to_vec(),
        counted,
    )
}

fn whole(program: &[u8]) -> (BellIsUrgent, PopOnBell, Vec<u8>, usize) {
    after(program, program.len())
}

const BOTH_RESET: &[u8] = b"\x1b[?1042;2$y\x1b[?1043;2$y";

/// **Off at power-on, and `DECRQM` says so** -- `2`, reset, not `0`: the
/// modes are recognised and acted on.
#[test]
fn both_are_reset_at_power_on() {
    assert_eq!(
        whole(b""),
        (BellIsUrgent::Off, PopOnBell::Off, BOTH_RESET.to_vec(), 0)
    );
}

/// Each mode sets and resets alone: the other is not touched, nothing is
/// counted, and `DECRQM` answers what the accessor reads.
#[test]
fn each_mode_sets_and_resets_alone() {
    assert_eq!(
        whole(b"\x1b[?1042h"),
        (
            BellIsUrgent::On,
            PopOnBell::Off,
            b"\x1b[?1042;1$y\x1b[?1043;2$y".to_vec(),
            0
        ),
        "?1042 set"
    );
    assert_eq!(
        whole(b"\x1b[?1043h"),
        (
            BellIsUrgent::Off,
            PopOnBell::On,
            b"\x1b[?1042;2$y\x1b[?1043;1$y".to_vec(),
            0
        ),
        "?1043 set"
    );
    assert_eq!(
        whole(b"\x1b[?1042;1043h\x1b[?1042l"),
        (
            BellIsUrgent::Off,
            PopOnBell::On,
            b"\x1b[?1042;2$y\x1b[?1043;1$y".to_vec(),
            0
        ),
        "?1042 reset beside a set ?1043"
    );
    assert_eq!(
        whole(b"\x1b[?1042;1043h\x1b[?1043l"),
        (
            BellIsUrgent::On,
            PopOnBell::Off,
            b"\x1b[?1042;1$y\x1b[?1043;2$y".to_vec(),
            0
        ),
        "?1043 reset beside a set ?1042"
    );
}

/// **`RIS` resets both and `DECSTR` keeps them.** ECMA-48 § 8.3.105: RIS
/// "causes a device to be reset to its initial state ... set the modes into
/// the reset state"; xterm keeps both, a departure not followed. The
/// control: the same program without a reset reads set, so the `RIS`
/// assertion is about the reset and not about the set never landing.
#[test]
fn ris_resets_both_and_decstr_keeps_them() {
    let set = (
        BellIsUrgent::On,
        PopOnBell::On,
        b"\x1b[?1042;1$y\x1b[?1043;1$y".to_vec(),
        0,
    );
    assert_eq!(whole(b"\x1b[?1042;1043h"), set, "the control");
    assert_eq!(
        whole(b"\x1b[?1042;1043h\x1bc"),
        (BellIsUrgent::Off, PopOnBell::Off, BOTH_RESET.to_vec(), 0),
        "RIS"
    );
    assert_eq!(whole(b"\x1b[?1042;1043h\x1b[!p"), set, "DECSTR");
}

/// **XTSAVE keeps them and XTRESTORE brings them back**, "Ps values are
/// the same as for DECSET" (`[[source: ctlseqs]]`).
#[test]
fn xtsave_and_xtrestore_carry_both() {
    assert_eq!(
        whole(b"\x1b[?1042;1043h\x1b[?1042;1043s\x1b[?1042;1043l\x1b[?1042;1043r"),
        (
            BellIsUrgent::On,
            PopOnBell::On,
            b"\x1b[?1042;1$y\x1b[?1043;1$y".to_vec(),
            0
        )
    );
}

/// **The modes land the same at every read size and every split into two
/// reads.** The program leaves both away from power-on by different paths.
#[test]
fn the_bell_modes_land_the_same_at_every_read_split() {
    let program: &[u8] = b"\x1b[?1043h\x1b[?1042;1043h\x1b[?1043l\x1b[?1042h";
    let want = whole(program);
    assert_eq!(
        want,
        (
            BellIsUrgent::On,
            PopOnBell::Off,
            b"\x1b[?1042;1$y\x1b[?1043;2$y".to_vec(),
            0
        ),
        "the whole read"
    );
    for size in 1..=program.len() {
        assert_eq!(after(program, size), want, "read size {size}");
    }
    for cut in 1..program.len() {
        let mut s = Owned::new(&support::CORPUS);
        let mut t = Term::new(s.storage(), &support::CORPUS);
        feed(&mut t, &program[..cut]);
        feed(&mut t, &program[cut..]);
        assert_eq!(
            (t.bell_is_urgent(), t.pop_on_bell(), t.unsupported_param),
            (want.0, want.1, 0),
            "split at {cut}"
        );
    }
}

/// **The modes ring nothing in the core; the count is what an embedder
/// rings from.** A `BEL` counts one with either mode in either state, and
/// a mode change counts none.
#[test]
fn a_bel_is_counted_whatever_the_modes_say() {
    for setup in [
        &b""[..],
        b"\x1b[?1042h",
        b"\x1b[?1043h",
        b"\x1b[?1042;1043h",
    ] {
        let mut s = Owned::new(&support::CORPUS);
        let mut t = Term::new(s.storage(), &support::CORPUS);
        feed(&mut t, setup);
        assert_eq!(t.bells, 0, "{setup:?}: a mode is not a bell");
        feed(&mut t, b"\x07a\x07");
        assert_eq!(t.bells, 2, "{setup:?}");
    }
}
