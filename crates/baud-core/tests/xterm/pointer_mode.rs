//! XTSMPOINTER, `CSI > Ps p`: when the pointer hides as the user types.
//!
//! `[[source: ctlseqs]]`: "Set resource value pointerMode (XTSMPOINTER),
//! xterm. This is used by xterm to decide whether to hide the pointer cursor
//! as the user types. ... Ps = 0 -> never hide the pointer. Ps = 1 -> hide
//! if the mouse tracking mode is not enabled. Ps = 2 -> always hide the
//! pointer, except when leaving the window. Ps = 3 -> always hide the
//! pointer, even if leaving/entering the window. If no parameter is given,
//! xterm uses the default, which is 1." The pointer is the embedder's: the
//! core keeps the mode and answers whether a key hides it.

use crate::support;
use baud_core::*;

/// **`CSI > 2 p` is acted on, not counted.** The bytes alone, so the test
/// reads the same against a core that counted the control as unsupported.
#[test]
fn xtsmpointer_is_acted_on_not_counted() {
    support::with_corpus(|t| {
        let before = t.unsupported;
        support::feed(t, b"\x1b[>0p\x1b[>1p\x1b[>2p\x1b[>3p\x1b[>p");
        assert_eq!(t.unsupported, before, "the five forms ctlseqs gives");
    });
}

/// **Each value sets its mode, and the mode against the tracking answers a
/// key.** 1 hides unless a program tracks the mouse -- any of `?9`,
/// `?1000`-`?1003` or DEC's locator, xterm's `send_mouse_pos != MOUSE_OFF`;
/// 0 never hides; 2 and 3 hide whatever the tracking. No parameter is 1,
/// and xterm reads the first parameter alone.
#[test]
fn each_value_sets_when_a_key_hides_the_pointer() {
    support::with_corpus(|t| {
        assert_eq!(t.pointer_mode(), PointerMode::UnlessTracking, "power-on");
        assert_eq!(t.hides_pointer_on_key(), PointerHides::Yes);
        for on in [
            &b"\x1b[?9h"[..],
            b"\x1b[?1000h",
            b"\x1b[?1001h",
            b"\x1b[?1002h",
            b"\x1b[?1003h",
            b"\x1b[1'z",
        ] {
            support::feed(t, on);
            assert_eq!(
                t.hides_pointer_on_key(),
                PointerHides::No,
                "tracking {:?}",
                t.mouse_tracking()
            );
            support::feed(t, b"\x1b[>2p");
            assert_eq!(t.hides_pointer_on_key(), PointerHides::Yes, "2 hides");
            support::feed(t, b"\x1b[>3p");
            assert_eq!(t.hides_pointer_on_key(), PointerHides::Yes, "3 hides");
            support::feed(t, b"\x1b[>0p");
            assert_eq!(t.hides_pointer_on_key(), PointerHides::No, "0 never");
            support::feed(t, b"\x1b[>p");
            assert_eq!(t.pointer_mode(), PointerMode::UnlessTracking, "none is 1");
            support::feed(t, b"\x1b[?1000l\x1b[0'z");
            assert_eq!(t.mouse_tracking(), MouseTracking::Off);
            assert_eq!(t.hides_pointer_on_key(), PointerHides::Yes, "off again");
        }
        support::feed(t, b"\x1b[>0p");
        assert_eq!(t.hides_pointer_on_key(), PointerHides::No);
        support::feed(t, b"\x1b[>2;0p");
        assert_eq!(t.pointer_mode(), PointerMode::Always, "the first alone");
        support::feed(t, b"\x1b[>3p");
        assert_eq!(t.pointer_mode(), PointerMode::AlwaysEvenLeaving);
        support::feed(t, b"\x1b[>1p");
        assert_eq!(t.pointer_mode(), PointerMode::UnlessTracking);
    });
}

/// **A value past 3 is refused and counted, and changes nothing.** ctlseqs
/// lists 0-3 as the "Valid values for the parameter"; xterm's
/// `CASE_HIDE_POINTER` stores any value, where its neighbours range-check.
#[test]
fn a_value_past_three_is_refused_and_counted() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[>2p");
        let before = t.unsupported;
        support::feed(t, b"\x1b[>4p\x1b[>9p\x1b[>65535p");
        assert_eq!(t.unsupported, before + 3, "each counted");
        assert_eq!(t.pointer_mode(), PointerMode::Always, "and nothing changed");
    });
}

/// **`RIS` returns the embedder's start value and `DECSTR` keeps the mode**:
/// "Reset title and pointer modes per resources titleModes and pointerMode"
/// is in xterm's full-reset list (`[[source: xterm-man]]`), and its hard
/// branch restores `pointer_mode0` (`[[source: xterm-reset]]`'s file).
#[test]
fn ris_returns_the_embedders_pointer_mode_and_decstr_keeps_it() {
    support::with_corpus(|t| {
        t.set_initial_pointer_mode(PointerMode::Never);
        assert_eq!(t.pointer_mode(), PointerMode::Never, "set in force too");
        support::feed(t, b"\x1b[>2p\x1b[!p");
        assert_eq!(t.pointer_mode(), PointerMode::Always, "DECSTR keeps it");
        support::feed(t, b"\x1bc");
        assert_eq!(t.pointer_mode(), PointerMode::Never, "RIS: the embedder's");
        support::feed(t, b"\x1b[>p");
        assert_eq!(
            t.pointer_mode(),
            PointerMode::UnlessTracking,
            "no parameter is xterm's 1, not the start value"
        );
    });
}

/// **The same at every read size and every split into two reads.**
#[test]
fn xtsmpointer_is_the_same_at_every_split() {
    let bytes = b"\x1b[>3p\x1b[>4p\x1b[>0;2p";
    for split in 0..=bytes.len() {
        support::with_corpus(|t| {
            support::feed(t, &bytes[..split]);
            support::feed(t, &bytes[split..]);
            assert_eq!(t.pointer_mode(), PointerMode::Never, "split {split}");
        });
    }
}
