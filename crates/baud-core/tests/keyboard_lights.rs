//! DECLL, `CSI Ps q`: the keyboard lights a program loads, drawn by the
//! embedder and never the keyboard's own lock lights.
//!
//! `[[source: vt100-ug-ch3]]`: "Load the four programmable LEDs on the
//! keyboard according to the parameter(s)", and in its table, `ESC [
//! Ps;Ps;...Ps q`, whose "Ps are selective parameters separated by
//! semicolons (0738) and executed in order", "0 or None All LEDs Off", and
//! "Any other parameter values are ignored". `[[source: ctlseqs]]` gives 1-3
//! Num Lock, Caps Lock and Scroll Lock and 21-23 extinguishing them. No
//! DECKLHIM gate: it is the VT510's, for lights that are also the lock
//! lights, which drawn ones are not.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 6,
    ws_col: 80,
};

/// Num, Caps and Scroll as lit after `bytes`, with the two unsupported
/// counts, fed at read sizes 1, 2 and whole, which must agree.
fn after(bytes: &[u8]) -> ([LedState; 3], usize, usize) {
    let mut seen = None;
    for chunk in [1, 2, bytes.len().max(1)] {
        let got = support::with_term(&WS, |t| {
            for piece in bytes.chunks(chunk) {
                support::feed(t, piece);
            }
            let l = t.keyboard_lights();
            (
                KeyLed::ALL.map(|k| l.state(k)),
                t.unsupported,
                t.unsupported_param,
            )
        });
        if let Some(prev) = seen {
            assert_eq!(got, prev, "{bytes:02x?} at read size {chunk}");
        }
        seen = Some(got);
    }
    seen.unwrap()
}

use LedState::{Off, On};

#[test]
fn decll_lights_and_extinguishes_each_light() {
    assert_eq!(after(b""), ([Off, Off, Off], 0, 0), "none lit at power-on");
    assert_eq!(after(b"\x1b[1q"), ([On, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[2q"), ([Off, On, Off], 0, 0));
    assert_eq!(after(b"\x1b[3q"), ([Off, Off, On], 0, 0));
    assert_eq!(after(b"\x1b[1q\x1b[21q"), ([Off, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[1;2;3q\x1b[22q"), ([On, Off, On], 0, 0));
    assert_eq!(after(b"\x1b[1;2;3q\x1b[23q"), ([On, On, Off], 0, 0));
    assert_eq!(after(b"\x1b[1;2;3q\x1b[0q"), ([Off, Off, Off], 0, 0));
}

/// "0 or None All LEDs Off": a bare `CSI q`, and an omitted parameter
/// in a list.
#[test]
fn decll_with_no_parameter_puts_every_light_out() {
    assert_eq!(after(b"\x1b[1;2;3q\x1b[q"), ([Off, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[1;2;3q\x1b[;3q"), ([Off, Off, On], 0, 0));
}

/// "executed in order": a later parameter overrides an earlier one.
#[test]
fn decll_parameters_execute_in_order() {
    assert_eq!(after(b"\x1b[1q\x1b[0;2q"), ([Off, On, Off], 0, 0));
    assert_eq!(after(b"\x1b[1;21q"), ([Off, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[21;1q"), ([On, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[2;0;3q"), ([Off, Off, On], 0, 0));
}

/// "Any other parameter values are ignored", the VT100's fourth light
/// among them, and each is counted.
#[test]
fn a_value_no_text_gives_a_light_is_ignored_and_counted() {
    assert_eq!(after(b"\x1b[4q"), ([Off, Off, Off], 0, 1));
    assert_eq!(after(b"\x1b[2;4;24q"), ([Off, On, Off], 0, 2));
    assert_eq!(after(b"\x1b[1q\x1b[20q"), ([On, Off, Off], 0, 1));
}

/// `RIS` returns the terminal to "the state it has after it is powered on":
/// none lit. `DECSTR`'s list names no light, so it keeps them.
#[test]
fn ris_puts_the_lights_out_and_decstr_keeps_them() {
    assert_eq!(after(b"\x1b[1;3q\x1bc"), ([Off, Off, Off], 0, 0));
    assert_eq!(after(b"\x1b[1;3q\x1b[!p"), ([On, Off, On], 0, 0));
}

/// The VT100's own control, at every conformance level.
#[test]
fn decll_acts_at_level_one() {
    assert_eq!(after(b"\x1b[61\"p\x1b[2q"), ([Off, On, Off], 0, 0));
}

/// The other `q` finals keep their meanings and light nothing: DECSCUSR's
/// `SP q`, DECSCA's `" q` and XTVERSION's `> q`.
#[test]
fn the_other_q_finals_light_nothing() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[5 q");
        assert_eq!(t.cursor_style(), CursorStyle::BlinkingBar);
        support::feed(t, b"\x1b[1\"q");
        support::feed(t, b"\x1b[>0q");
        assert!(!t.reply().is_empty(), "XTVERSION answers");
        assert_eq!(t.keyboard_lights(), KeyLeds::NONE);
        assert_eq!(t.unsupported, 0);
    });
}

/// DECAC and DECATC, the VT525's colour assignments, are out: each is
/// counted, and nothing changes, the keyboard lights that share no byte
/// with them included.
#[test]
fn decac_and_decatc_are_counted() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[1;7;0,|");
        assert_eq!(t.unsupported, 1, "DECAC");
        support::feed(t, b"\x1b[0;7;0,}");
        assert_eq!(t.unsupported, 2, "DECATC");
        assert_eq!(t.keyboard_lights(), KeyLeds::NONE);
    });
}
