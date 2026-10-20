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
