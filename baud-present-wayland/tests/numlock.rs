//! **With `?1035` set, NumLock takes the whole keypad out of DECKPAM, its
//! digits as well as its operators.** Played on the pinned US keymap through
//! `baud-xkb` and `keys::press`, so the modifiers are the driver's: the
//! keymap's `KEYPAD` type consumes NumLock for the digits, and xterm still
//! tests NumLock against the event's whole state (`[[source:
//! xterm-input-c]]`, the `num_lock` override in `Input`).

use baud_domain::{
    BackarrowKey, C1Transmit, DeleteKey, InputEncoding, KeyState, KeypadMode, KittyFlags, Modes,
    NumLockKeypad, OtherKeys, OtherKeysFormat, TerminalMode, encode_key,
};
use baud_present_wayland::keys;
use baud_xkb::{Keymap, Mods, State};

/// DECKPAM, with `?1035` as given and every other mode at power-on.
fn application(num_lock: NumLockKeypad) -> KeyState {
    KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::Application,
        other_keys: OtherKeys::default(),
        other_keys_format: OtherKeysFormat::default(),
        c1: C1Transmit::SevenBit,
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::NONE,
        backarrow: BackarrowKey::default(),
        delete: DeleteKey::default(),
        num_lock,
    }
}

/// (key name, bytes with `?1035` set, bytes with it reset), NumLock on.
/// Reset, a key sends its Application column of `[[source: ctlseqs]]`'s
/// Sun/PC keypad table, the digit's row named for its navigation key.
const KEYS: [(&str, &[u8], &[u8]); 5] = [
    ("KP1", b"1", b"\x1bOF"),
    ("KP5", b"5", b"\x1b[E"),
    ("KPDL", b".", b"\x1b[3~"),
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
