//! **With `?1035` set, NumLock takes the whole keypad out of DECKPAM, its
//! digits as well as its operators.** Played on the pinned US keymap through
//! `baud-xkb` and `keys::press`, so the modifiers are the driver's: the
//! keymap's `KEYPAD` type consumes NumLock for the digits, and xterm still
//! tests NumLock against the event's whole state (`[[source:
//! xterm-input-c]]`, the `num_lock` override in `Input`).

use baud_domain::{
    BackarrowKey, C1Transmit, ConformanceLevel, DeleteKey, InputEncoding, KeyResources, KeyState,
    KeypadMode, KittyFlags, Modes, NumLockKeypad, TerminalMode, encode_key,
};
use baud_present_wayland::keys;
use baud_xkb::{Keymap, Mods, State};

/// DECKPAM, with `?1035` as given and every other mode at power-on.
fn application(num_lock: NumLockKeypad) -> KeyState {
    KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::Application,
        keys: KeyResources::default(),
        c1: C1Transmit::SevenBit,
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::NONE,
        backarrow: BackarrowKey::default(),
        delete: DeleteKey::default(),
        num_lock,
        level: ConformanceLevel::Four,
    }
}

/// (key name, bytes with `?1035` set, bytes with it reset), NumLock on.
/// Reset, a digit and `.` send the VT220 keypad's application codes,
/// `SS3 p`-`y` and `SS3 n`, as xterm's code sends them (Jev, VT220_SS3,
/// 0.94/0.94), and the operators their Application column.
const KEYS: [(&str, &[u8], &[u8]); 5] = [
    ("KP1", b"1", b"\x1bOq"),
    ("KP5", b"5", b"\x1bOu"),
    ("KPDL", b".", b"\x1bOn"),
    ("KPAD", b"+", b"\x1bOk"),
    ("KPEN", b"\r", b"\x1bOM"),
];

#[test]
fn num_lock_takes_every_keypad_key_out_of_the_application_keypad() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let keymap = Keymap::from_text(&text).expect("it compiles");
    let mut state = State::new(&keymap);
    let numlock = keymap.effective(1 << keymap.mod_index("NumLock").expect("NumLock"));
    state.update(
        Mods {
            locked: numlock,
            ..Mods::default()
        },
        0,
    );
    for (name, set, reset) in KEYS {
        let kc = keymap.keycode_named(name).expect(name);
        let e = keys::press(&state, kc).expect(name);
        let sent = |s| encode_key(e, s).map(|k| k.as_bytes().to_vec());
        assert_eq!(
            sent(application(NumLockKeypad::Overrides)).as_deref(),
            Some(set),
            "{name}, ?1035 set"
        );
        assert_eq!(
            sent(application(NumLockKeypad::Ignored)).as_deref(),
            Some(reset),
            "{name}, ?1035 reset"
        );
    }
}

/// **Under kitty's "Report all keys as escape codes" a keypad digit carries
/// `num_lock`**, as `KP_Add` already did: "To get lock modifiers for all
/// keys use the Report all keys as escape codes enhancement"
/// (`[[source: kitty-keyboard]]`), and `num_lock` is 128, so `1 + 128`.
#[test]
fn a_keypad_digit_reports_num_lock_to_kitty_as_its_operators_do() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let keymap = Keymap::from_text(&text).expect("it compiles");
    let mut state = State::new(&keymap);
    let numlock = keymap.effective(1 << keymap.mod_index("NumLock").expect("NumLock"));
    state.update(
        Mods {
            locked: numlock,
            ..Mods::default()
        },
        0,
    );
    let all = KeyState {
        kitty: KittyFlags::ALL_AS_ESCAPES,
        ..application(NumLockKeypad::default())
    };
    for (name, sent) in [("KP1", b"\x1b[57400;129u"), ("KPAD", b"\x1b[57413;129u")] {
        let kc = keymap.keycode_named(name).expect(name);
        let e = keys::press(&state, kc).expect(name);
        let got = encode_key(e, all).map(|k| k.as_bytes().to_vec());
        assert_eq!(got.as_deref(), Some(sent.as_slice()), "{name}");
    }
}

/// **Under kitty's disambiguate flag alone a keypad key that types a
/// character sends the character, its lock unreported**: "all non text
/// keypad keys will be reported as separate keys with CSI u", and "the Lock
/// modifiers are not reported for text producing keys, to keep them usable
/// in legacy programs" (`[[source: kitty-keyboard]]`). The driver gives a
/// keypad key the text it types, as kitty's own keyboard layer does -- none
/// while Control, Alt or Super is held, none for a control code -- and a key
/// with no text is still its own number. A digit went out as `CSI 57400 ;
/// 129 u`, which a program that negotiated only disambiguation read as no
/// key it knew.
#[test]
fn a_keypad_key_that_types_is_its_text_under_disambiguate_alone() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let keymap = Keymap::from_text(&text).expect("it compiles");
    let mut state = State::new(&keymap);
    let numlock = keymap.effective(1 << keymap.mod_index("NumLock").expect("NumLock"));
    let control = keymap.effective(1 << keymap.mod_index("Control").expect("Control"));
    let disambiguate = KeyState {
        kitty: KittyFlags::DISAMBIGUATE,
        ..application(NumLockKeypad::default())
    };
    let sent = |state: &State<'_>, name: &str| {
        let kc = keymap.keycode_named(name).expect(name);
        let e = keys::press(state, kc).expect(name);
        encode_key(e, disambiguate).map(|k| k.as_bytes().to_vec())
    };
    state.update(
        Mods {
            locked: numlock,
            ..Mods::default()
        },
        0,
    );
    for (name, want) in [
        ("KP1", &b"1"[..]),
        ("KPAD", b"+"),
        ("KPDL", b"."),
        ("KPEN", b"\x1b[57414;129u"),
    ] {
        assert_eq!(
            sent(&state, name).as_deref(),
            Some(want),
            "{name}, NumLock on"
        );
    }
    // Control types nothing, so the digit is its number, the lock with it.
    state.update(
        Mods {
            locked: numlock,
            depressed: control,
            ..Mods::default()
        },
        0,
    );
    assert_eq!(
        sent(&state, "KP1").as_deref(),
        Some(&b"\x1b[57400;133u"[..]),
        "Control with NumLock"
    );
    // Alt types the digit in the keymap, and kitty's layer gives no text.
    let alt = keymap.effective(1 << keymap.mod_index("Mod1").expect("Mod1"));
    state.update(
        Mods {
            locked: numlock,
            depressed: alt,
            ..Mods::default()
        },
        0,
    );
    assert_eq!(
        sent(&state, "KP1").as_deref(),
        Some(&b"\x1b[57400;131u"[..]),
        "Alt with NumLock"
    );
    // NumLock off: the navigation key types nothing.
    state.update(Mods::default(), 0);
    assert_eq!(
        sent(&state, "KP1").as_deref(),
        Some(&b"\x1b[57424u"[..]),
        "KP_End"
    );
}
