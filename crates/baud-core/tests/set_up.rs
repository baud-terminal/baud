//! The four modes DEC STD 070 makes local Set-up features, "for receipt
//! from software application processes and as a local Set-up feature"
//! (`[[source: dec-std-070]]` 070-6 §6.2.1 d): DECARM `?8`, DECCKM `?1`,
//! the keypad (DECKPAM and DECKPNM, DECNKM `?66`) and LNM `20`, the
//! embedder's `[keyboard]` settings (`Term::set_up_at_launch`,
//! `Term::change_set_up`).
//!
//! **DECARM, DECCKM and LNM are saved values**, the power-up and `RIS`
//! state (070-4 Device Initialization; `[[source: vt510-ris]]`, "RIS
//! replaces all set-up features with their saved settings"). **The keypad
//! is not**: "The setting is not saved in nonvolatile memory"
//! (`[[source: vt420-uu-ch5]]`), and power-up and `RIS` select Numeric
//! (`[[source: vt510-deckpam]]`). DECSTR keeps 070-4's table: DECCKM
//! "reset (Cursor)", the keypad "reset (Numeric)", DECARM and LNM "NOT
//! affected".

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 5,
};

/// What DECRQM says of the four, as `(?8, ?1, ?66, 20)`: 1 set, 2 reset.
fn modes(t: &mut Term<'_>) -> (u8, u8, u8, u8) {
    let mut ask = |q: &[u8]| {
        t.clear_reply();
        support::feed(t, q);
        let r = t.reply().to_vec();
        t.clear_reply();
        let semi = r.iter().position(|&b| b == b';').expect("a DECRPM");
        r[semi + 1] - b'0'
    };
    (
        ask(b"\x1b[?8$p"),
        ask(b"\x1b[?1$p"),
        ask(b"\x1b[?66$p"),
        ask(b"\x1b[20$p"),
    )
}

/// Each mode away from its factory value.
const TURNED: SetUp = SetUp {
    auto_repeat: ModeChange::Reset,
    cursor_keys: ModeChange::Set,
    keypad: KeypadMode::Application,
    new_line: ModeChange::Set,
};

/// **Without a Set-up the factory settings**: repeat on, cursor keys,
/// Numeric, line feed, 070-4's `host_state_init`.
#[test]
fn the_factory_set_up_is_the_power_up_state() {
    support::with_term(&WS, |t| {
        assert_eq!(t.set_up(), SetUp::FACTORY);
        assert_eq!(modes(t), (1, 2, 2, 2));
    });
}

/// **The saved three at launch and at every `RIS`, and the keypad
/// Numeric**: launch with each turned, a program turning them all back,
/// then `RIS`.
#[test]
fn launch_and_ris_take_the_saved_values_and_select_numeric() {
    support::with_term(&WS, |t| {
        t.set_up_at_launch(TURNED);
        assert_eq!(t.set_up(), TURNED);
        assert_eq!(modes(t), (2, 1, 2, 1), "launch: the keypad is Numeric");
        assert!(t.cursor_keys_application());
        assert_eq!(t.keypad(), KeypadMode::Numeric);
        support::feed(t, b"\x1b[?8h\x1b[?1l\x1b=\x1b[20l");
        assert_eq!(modes(t), (1, 2, 1, 2));
        support::feed(t, b"\x1bc");
        assert_eq!(modes(t), (2, 1, 2, 1), "RIS");
    });
}

/// **LNM's Set-up value acts as LNM**: a line feed returns to the first
/// column, and Return sends CR LF.
#[test]
fn a_new_line_set_up_is_lnm_itself() {
    support::with_term(&WS, |t| {
        t.set_up_at_launch(SetUp {
            new_line: ModeChange::Set,
            ..SetUp::FACTORY
        });
        support::feed(t, b"ab\n");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::ZERO));
        t.clear_reply();
        assert!(t.key(KeyEvent::press(KeyCode::Enter, Modifiers::NONE)));
        assert_eq!(t.reply(), b"\r\n");
    });
}

/// **DECSTR and DECSCL keep 070-4's table under a Set-up**: DECCKM to
/// cursor and the keypad to Numeric whatever the Set-up says, DECARM and
/// LNM as the program left them.
#[test]
fn a_soft_reset_keeps_its_own_table() {
    for reset in [&b"\x1b[!p"[..], b"\x1b[64\"p"] {
        support::with_term(&WS, |t| {
            t.set_up_at_launch(TURNED);
            support::feed(t, b"\x1b[?8h\x1b=\x1b[20l");
            assert_eq!(modes(t), (1, 1, 1, 2));
            support::feed(t, reset);
            assert_eq!(modes(t), (1, 2, 2, 2), "{reset:?}");
        });
    }
}

/// **A change of the Set-up acts on the modes it changes, now, as a field
/// switched by hand**, and only on those: a program's DECCKM and keypad
/// survive a change of LNM and of DECARM, the keypad follows a change of
/// its own value, and `RIS` restores the new saved values.
#[test]
fn a_change_of_the_set_up_acts_on_what_it_changes() {
    support::with_term(&WS, |t| {
        support::feed(t, b"\x1b[?1h\x1b=");
        let lnm = SetUp {
            new_line: ModeChange::Set,
            ..SetUp::FACTORY
        };
        t.change_set_up(lnm);
        assert_eq!(modes(t), (1, 1, 1, 1), "LNM only");
        t.change_set_up(lnm);
        assert_eq!(modes(t), (1, 1, 1, 1), "the same Set-up again");
        let keypad = SetUp {
            keypad: KeypadMode::Application,
            ..lnm
        };
        t.change_set_up(keypad);
        support::feed(t, b"\x1b>");
        let quiet = SetUp {
            auto_repeat: ModeChange::Reset,
            ..keypad
        };
        t.change_set_up(quiet);
        assert_eq!(modes(t), (2, 1, 2, 1), "DECKPNM survives DECARM's change");
        support::feed(t, b"\x1b=");
        t.change_set_up(SetUp {
            keypad: KeypadMode::Numeric,
            ..quiet
        });
        assert_eq!(modes(t), (2, 1, 2, 1), "the keypad follows its own change");
        support::feed(t, b"\x1b[?8h\x1b[20l\x1bc");
        assert_eq!(modes(t), (2, 2, 2, 1), "RIS: the saved values");
    });
}
