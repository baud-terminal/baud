//! **A PC keypad names a navigation key with NumLock off and a digit with
//! it on, and both reach the core as keypad keys.** Played on the pinned
//! US keymap through `baud-xkb`, so the keysym each key produces is the
//! keymap's (`key <KP4> { [ KP_Left, KP_4 ] }`), not a value typed here.

use baud_domain::{KeyCode, KeypadKey};
use baud_present_wayland::keys;
use baud_xkb::{Keymap, Keysym, Mods, State};

/// (key name, NumLock-off keysym, navigation key, NumLock-on digit).
const KEYS: [(&str, &str, KeypadKey, KeypadKey); 11] = [
    ("KP7", "KP_Home", KeypadKey::Home, KeypadKey::Digit7),
    ("KP8", "KP_Up", KeypadKey::Up, KeypadKey::Digit8),
    ("KP9", "KP_Prior", KeypadKey::PageUp, KeypadKey::Digit9),
    ("KP4", "KP_Left", KeypadKey::Left, KeypadKey::Digit4),
    ("KP5", "KP_Begin", KeypadKey::Begin, KeypadKey::Digit5),
    ("KP6", "KP_Right", KeypadKey::Right, KeypadKey::Digit6),
    ("KP1", "KP_End", KeypadKey::End, KeypadKey::Digit1),
    ("KP2", "KP_Down", KeypadKey::Down, KeypadKey::Digit2),
    ("KP3", "KP_Next", KeypadKey::PageDown, KeypadKey::Digit3),
    ("KP0", "KP_Insert", KeypadKey::Insert, KeypadKey::Digit0),
    ("KPDL", "KP_Delete", KeypadKey::Delete, KeypadKey::Period),
];

#[test]
fn numlock_chooses_between_the_navigation_key_and_the_digit() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let keymap = Keymap::from_text(&text).expect("it compiles");
    let mut state = State::new(&keymap);
    // The virtual NumLock as the real modifier a compositor locks.
    let numlock = keymap.effective(1 << keymap.mod_index("NumLock").expect("NumLock"));
    for (name, sym, nav, digit) in KEYS {
        let kc = keymap.keycode_named(name).expect(name);
        state.update(Mods::default(), 0);
        assert_eq!(state.sym(kc), Keysym::named(sym).expect(sym), "{name}");
        let off = keys::press(&state, kc).expect(name);
        assert_eq!(off.code, KeyCode::Keypad(nav), "{name}, NumLock off");
        state.update(
            Mods {
                locked: numlock,
                ..Mods::default()
            },
            0,
        );
        let on = keys::press(&state, kc).expect(name);
        assert_eq!(on.code, KeyCode::Keypad(digit), "{name}, NumLock on");
    }
}

/// **`KP_Equal` is the keypad's `=`**, `XKB_KEY_KP_Equal` 0xffbd
/// (`[[source: xkbcommon-keysyms]]` line 312), which the pinned keymap binds to
/// `<KPEQ>` alone, so DECKPAM's `SS3 X` and kitty's `KP_EQUAL` reach it.
#[test]
fn kp_equal_is_the_keypads_equal() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let keymap = Keymap::from_text(&text).expect("it compiles");
    let mut state = State::new(&keymap);
    let kc = keymap.keycode_named("KPEQ").expect("KPEQ");
    state.update(Mods::default(), 0);
    assert_eq!(state.sym(kc), Keysym::named("KP_Equal").expect("KP_Equal"));
    let e = keys::press(&state, kc).expect("KPEQ");
    assert_eq!(e.code, KeyCode::Keypad(KeypadKey::Equal));
}

/// **With NumLock off a keypad key is its navigation key, though Shift
/// typed its digit**, as kitty's keyboard layer names a key by the keysym
/// it gives with no modifier and takes the lookup's only while NumLock is
/// on (`[[source: kitty-xkb-glfw-c]]`), as the protocol has a text key's
/// code "the lower-case (or more technically, un-shifted) version of the
/// key" (`[[source: kitty-keyboard]]`). The keymap
/// is the pinned US one with the XKB protocol's own `KEYPAD` type, which
/// xkeyboard-config ships as `numpad(pc)`, "Shift selects the digits and
/// cancels NumLock" (`[[source: xkeyboard-config-numpad-193cc6c]]`), in
/// place of the default, which consumes NumLock alone. Shift with the
/// keypad's 4 is `KP_LEFT` with Shift, `CSI 57417 ; 2 u` under "report all
/// keys", where it went out as `KP_4`'s `CSI 57403 ; 2 u`, and its text
/// `4` rides with associated text; disambiguate alone, the numeric keypad
/// and DECKPAM send what they sent, the `4` and `KP_4`'s application row,
/// as xterm sends the keysym the lookup gave. Perturbation: name the key
/// by the lookup's keysym again and the kitty rows read 57403; take the
/// navigation key on the base layer as well and the legacy row reads the
/// left arrow.
#[test]
fn shift_on_a_keypad_digit_with_numlock_off_names_the_navigation_key() {
    use baud_domain::{
        BackarrowKey, C1Transmit, Codepoint, ConformanceLevel, DeleteKey, InputEncoding,
        KeyResources, KeyState, KeypadMode, KittyFlags, KittySetMode, Modes, Modifiers,
        NumLockKeypad, TerminalMode, encode_key,
    };
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-xkb/tests/fixtures/keymap-us.xkb"
    ))
    .expect("the pinned keymap");
    let default = "\ttype \"KEYPAD\" {\n\t\tmodifiers= NumLock;\n\t\tmap[NumLock]= 2;\n";
    assert_eq!(text.matches(default).count(), 1, "the default KEYPAD type");
    let pc = text.replace(
        default,
        "\ttype \"KEYPAD\" {\n\t\tmodifiers= Shift+NumLock;\n\t\tmap[Shift]= 2;\n\t\tmap[NumLock]= 2;\n",
    );
    let keymap = Keymap::from_text(&pc).expect("it compiles");
    let numlock = keymap.effective(1 << keymap.mod_index("NumLock").expect("NumLock"));
    let kp4 = keymap.keycode_named("KP4").expect("KP4");
    let press = |depressed, locked| {
        let mut state = State::new(&keymap);
        state.update(
            Mods {
                depressed,
                locked,
                ..Mods::default()
            },
            0,
        );
        (state.sym(kp4), keys::press(&state, kp4).expect("KP4"))
    };
    let sent = |e, mode: &str| {
        let mut s = KeyState {
            modes: Modes::default(),
            keypad: KeypadMode::Numeric,
            keys: KeyResources::default(),
            c1: C1Transmit::SevenBit,
            terminal: TerminalMode::default(),
            encoding: InputEncoding::default(),
            kitty: KittyFlags::NONE,
            backarrow: BackarrowKey::default(),
            delete: DeleteKey::default(),
            num_lock: NumLockKeypad::default(),
            level: ConformanceLevel::Four,
        };
        match mode {
            "legacy" => {}
            "deckpam" => s.keypad = KeypadMode::Application,
            n => {
                for f in [
                    KittyFlags::DISAMBIGUATE,
                    KittyFlags::ALL_AS_ESCAPES,
                    KittyFlags::ASSOCIATED_TEXT,
                ] {
                    if n.parse::<u8>().expect("flags") & f.bits() != 0 {
                        s.kitty = s.kitty.apply(f, KittySetMode::Or);
                    }
                }
            }
        }
        encode_key(e, s).map_or_else(Vec::new, |k| k.as_bytes().to_vec())
    };
    // Shift, NumLock off: the keymap types the digit.
    let (sym, shifted) = press(1, 0);
    assert_eq!(sym, Keysym::named("KP_4").expect("KP_4"));
    assert_eq!(shifted.code, KeyCode::Keypad(KeypadKey::Left));
    assert_eq!(shifted.text, Some(Codepoint::from_char('4')));
    assert_eq!(shifted.mods, Modifiers::SHIFT);
    for (mode, want) in [
        ("legacy", &b"4"[..]),
        ("deckpam", b"\x1bO2t"),
        ("1", b"4"),
        ("8", b"\x1b[57417;2u"),
        ("24", b"\x1b[57417;2;52u"),
    ] {
        assert_eq!(sent(shifted, mode), want, "Shift+KP4 under {mode}");
    }
    // The controls: NumLock on, the digit, and with Shift as well the
    // navigation key, which the lookup gives then too.
    let (_, digit) = press(0, numlock);
    assert_eq!(digit.code, KeyCode::Keypad(KeypadKey::Digit4));
    assert_eq!(sent(digit, "8"), b"\x1b[57403;129u");
    let (sym, both) = press(1, numlock);
    assert_eq!(sym, Keysym::named("KP_Left").expect("KP_Left"));
    assert_eq!(
        (both.code, both.text),
        (KeyCode::Keypad(KeypadKey::Left), None)
    );
    assert_eq!(sent(both, "8"), b"\x1b[57417;130u");
    let (_, plain) = press(0, 0);
    assert_eq!(plain.code, KeyCode::Keypad(KeypadKey::Left));
    assert_eq!(sent(plain, "legacy"), b"\x1b[D");
}
