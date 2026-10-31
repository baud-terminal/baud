//! **Shift and Caps Lock from a Linux keyboard, as kitty reports them**,
//! played on the pinned keymaps through `baud-xkb` and `keys::press`: a
//! key the keymap shifted is the key Shift is on, with Shift beside it and
//! what it typed as its shifted key (`[[source: kitty-keyboard]]`: the key
//! is "the lower-case (or more technically, un-shifted) version of the
//! key", and "if a modifier is *active* when the key event occurs ... the
//! key event must have the bit for that modifier set"). The rows both
//! drivers share are `tests/keys/shift.txt`.

use baud_domain::{
    BackarrowKey, C1Transmit, Codepoint, ConformanceLevel, DeleteKey, InputEncoding, KeyCode,
    KeyEvent, KeyResource, KeyResources, KeySet, KeyState, KeypadMode, KittyFlags, KittySetMode,
    Modes, Modifiers, NumLockKeypad, TerminalMode, encode_key,
};
use baud_present_wayland::keys;
use baud_xkb::{Keycode, Keymap, ModMask, Mods, State};

fn keymap(name: &str) -> Keymap {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../baud-xkb/tests/fixtures")
            .join(name),
    )
    .expect("the pinned keymap");
    Keymap::from_text(&text).expect("it compiles")
}

const SHIFT: ModMask = 1 << 0;
const LOCK: ModMask = 1 << 1;
const CTRL: ModMask = 1 << 2;
const ALT: ModMask = 1 << 3;
/// LevelThree, AltGr, is Mod5 on the pinned keymaps (`baud-xkb`'s oracle
/// holds it so).
const ALTGR: ModMask = 1 << 7;

/// The key event for `key` with `held` depressed, `locked` locked and
/// `group` the layout.
fn press(km: &Keymap, group: usize, held: ModMask, locked: ModMask, key: Keycode) -> KeyEvent {
    let mut st = State::new(km);
    st.update(
        Mods {
            depressed: held,
            locked,
            ..Mods::default()
        },
        group,
    );
    keys::press(&st, key).expect("a key")
}

/// The keyboard state `mode` names: `legacy`, `deckpam` (the application
/// keypad), `kitty=N` or `mok=N`.
fn state(mode: &str) -> KeyState {
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
    match mode.split_once('=') {
        None if mode == "deckpam" => s.keypad = KeypadMode::Application,
        None => assert_eq!(mode, "legacy"),
        Some(("kitty", n)) => {
            let n: u8 = n.parse().expect("flags");
            for f in [
                KittyFlags::DISAMBIGUATE,
                KittyFlags::EVENT_TYPES,
                KittyFlags::ALTERNATE_KEYS,
                KittyFlags::ALL_AS_ESCAPES,
                KittyFlags::ASSOCIATED_TEXT,
            ] {
                if n & f.bits() != 0 {
                    s.kitty = s.kitty.apply(f, KittySetMode::Or);
                }
            }
            assert_eq!(s.kitty.bits(), n, "{mode}");
        }
        Some(("mok", n)) => {
            let v = KeyResource::Other
                .value(n.parse().expect("level"))
                .expect("a level");
            s.keys
                .apply(KeySet::Set(KeyResource::Other, v, Modifiers::NONE));
        }
        _ => panic!("{mode}"),
    }
    s
}

/// What `e` sends under `mode`.
fn sent(e: KeyEvent, mode: &str) -> Vec<u8> {
    encode_key(e, state(mode)).map_or_else(Vec::new, |k| k.as_bytes().to_vec())
}

/// `\e` and `\xNN` read as the bytes they name.
fn unescape(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1)) {
            (b'\\', Some(b'e')) => {
                out.push(0x1b);
                i += 2;
            }
            (b'\\', Some(b'x')) => {
                let hex = s.get(i + 2..i + 4).expect("two hex digits");
                out.push(u8::from_str_radix(hex, 16).expect("hex"));
                i += 4;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// A table character: itself, or the word `space`.
fn character(s: &str) -> char {
    match s {
        "space" => ' ',
        _ => {
            let mut cs = s.chars();
            let c = cs.next().expect("a character");
            assert!(cs.next().is_none(), "{s}");
            c
        }
    }
}

/// The key that types `c` with no modifier on the pinned US keymap.
fn us_key(km: &Keymap, c: char) -> Keycode {
    let st = State::new(km);
    (8..256)
        .map(Keycode)
        .find(|&k| {
            st.lookup(k).and_then(|l| l.sym.to_char()) == Some(c) && keys::pc101(k) == Some(c)
        })
        .expect("the US key")
}

/// **Every row of the table both drivers share is what a Linux keyboard
/// sends**: a key typed with Shift on the US keymap is the key Shift is
/// on, Shift beside it and what it typed its shifted key, so Shift+a is
/// `CSI 97 ; 2 u` under "report all keys", where it went out as
/// `CSI 65 u`, Control+Shift+2 the `2` key, `CSI 50 ; 6 u`, and under
/// `modifyOtherKeys` 2 Shift+a is `CSI 27 ; 2 ; 65 ~`, where it sent `A`.
/// Perturbation: take a consumed Shift out of the modifiers again and every
/// kitty row loses its `2`; name the key by the character it typed and
/// Shift+a reads `65`.
#[test]
fn the_shift_table_is_what_the_wayland_driver_sends() {
    let km = keymap("keymap-us.xkb");
    let table = include_str!("../../tests/keys/shift.txt");
    let mut rows = 0;
    for line in table
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let [keyed, mode, want] = line.split(" | ").collect::<Vec<_>>()[..] else {
            panic!("{line}")
        };
        let [chord, key, typed] = keyed.split(' ').collect::<Vec<_>>()[..] else {
            panic!("{line}")
        };
        let held = match chord {
            "shift" => SHIFT,
            "ctrl+shift" => CTRL | SHIFT,
            "ctrl" => CTRL,
            _ => panic!("{chord}"),
        };
        let (key, typed) = (character(key), character(typed));
        let e = press(&km, 0, held, 0, us_key(&km, key));
        assert_eq!(e.code, KeyCode::Char(Codepoint::from_char(key)), "{line}");
        assert_eq!(e.mods.has(Modifiers::SHIFT), held & SHIFT != 0, "{line}");
        assert_eq!(
            e.shifted,
            Some(Codepoint::from_char(typed)).filter(|_| typed != key),
            "{line}"
        );
        assert_eq!(sent(e, mode), unescape(want), "{line}");
        rows += 1;
    }
    assert_eq!(rows, 28, "the table's every row");
}

/// One key and what it sends under each mode.
type Row<'a> = (KeyEvent, &'a [(&'a str, &'a [u8])]);

fn check(name: &str, (e, rows): Row<'_>) {
    for (mode, want) in rows {
        assert_eq!(
            sent(e, mode),
            *want,
            "{name} under {mode}: {:?}",
            String::from_utf8_lossy(&sent(e, mode))
        );
    }
}

/// **Caps Lock is a modifier kitty reports where it asks for locks**:
/// Caps Lock on, `a` is the `a` key with `caps_lock` (64) set, `CSI 97 ;
/// 65 u` under "report all keys" and its text `A` under disambiguate alone,
/// as "the Lock modifiers are not reported for text producing keys"; with
/// Control it is `CSI 97 ; 69 u`, where it went out as `CSI 65 ; 5 u`.
/// With Shift as well the keymap types `a`, which is the key's own, so no
/// shifted key rides and the legacy layer sends `a`. Perturbation: take a
/// consumed Lock out of the modifiers again and the kitty rows read `65 u`
/// and `65 ; 5 u`.
#[test]
fn caps_lock_is_reported_where_kitty_asks_for_locks() {
    let km = keymap("keymap-us.xkb");
    let a = km.keycode_named("AC01").expect("AC01");
    let caps = press(&km, 0, 0, LOCK, a);
    assert_eq!(caps.code, KeyCode::Char(Codepoint::from_char('a')));
    assert_eq!(caps.text, Some(Codepoint::from_char('A')));
    assert_eq!(caps.mods, Modifiers::CAPS_LOCK);
    check(
        "Caps+a",
        (
            caps,
            &[
                ("legacy", b"A"),
                ("kitty=1", b"A"),
                ("mok=2", b"A"),
                ("kitty=8", b"\x1b[97;65u"),
                ("kitty=12", b"\x1b[97;65u"),
                ("kitty=24", b"\x1b[97;65;65u"),
            ],
        ),
    );
    check(
        "Caps+Ctrl+a",
        (
            press(&km, 0, CTRL, LOCK, a),
            &[("legacy", b"\x01"), ("kitty=1", b"\x1b[97;69u")],
        ),
    );
    let both = press(&km, 0, SHIFT, LOCK, a);
    assert_eq!(
        (both.shifted, both.text),
        (None, Some(Codepoint::from_char('a')))
    );
    check(
        "Caps+Shift+a",
        (
            both,
            &[
                ("legacy", b"a"),
                ("kitty=8", b"\x1b[97;66u"),
                ("kitty=12", b"\x1b[97;66u"),
                ("mok=2", b"\x1b[27;2;97~"),
            ],
        ),
    );
}

/// **A functional key the keymap shifts keeps its Shift**: Tab with Shift
/// is `ISO_Left_Tab` on the US keymap, and goes out as `CSI Z` (CBT) as
/// xterm sends it, `CSI 9 ; 2 u` under "report all keys" as kitty's clean
/// keysym names it; it went out as a plain tab. Backspace with Shift,
/// whose two levels are both `BackSpace`, is `CSI 127 ; 2 u` there, where
/// it was `CSI 127 u`. F1 and the keypad's `+`, whose `CTRL+ALT` type
/// consumes Shift, carry it: Shift+F1 is `CSI 1 ; 2 P`, where it went out
/// as F1, and Shift with the keypad's `+` `SS3 2 k` on the application
/// keypad, as xterm's `MODIFIER_PARM` writes it, and `CSI 57413 ; 2 u`.
/// Alt+Shift+a is `ESC A` on the legacy layer and the `a` key under
/// disambiguate, `CSI 97 ; 4 u`.
#[test]
fn a_functional_key_the_keymap_shifts_keeps_its_shift() {
    let km = keymap("keymap-us.xkb");
    let key = |n: &str| km.keycode_named(n).expect(n);
    let tab = press(&km, 0, SHIFT, 0, key("TAB"));
    assert_eq!((tab.code, tab.mods), (KeyCode::Tab, Modifiers::SHIFT));
    check(
        "Shift+Tab",
        (
            tab,
            &[
                ("legacy", b"\x1b[Z"),
                ("kitty=1", b"\x1b[Z"),
                ("kitty=8", b"\x1b[9;2u"),
                ("mok=2", b"\x1b[27;2;9~"),
            ],
        ),
    );
    check(
        "Shift+Backspace",
        (
            press(&km, 0, SHIFT, 0, key("BKSP")),
            &[("legacy", b"\x7f"), ("kitty=8", b"\x1b[127;2u")],
        ),
    );
    check(
        "Shift+F1",
        (
            press(&km, 0, SHIFT, 0, key("FK01")),
            &[("legacy", b"\x1b[1;2P"), ("kitty=8", b"\x1b[1;2P")],
        ),
    );
    let add = press(&km, 0, SHIFT, 0, key("KPAD"));
    assert_eq!(add.mods, Modifiers::SHIFT);
    check(
        "Shift+KP_Add",
        (
            add,
            &[
                ("legacy", b"+"),
                ("deckpam", b"\x1bO2k"),
                ("kitty=8", b"\x1b[57413;2u"),
            ],
        ),
    );
    check(
        "Alt+Shift+a",
        (
            press(&km, 0, ALT | SHIFT, 0, key("AC01")),
            &[
                ("legacy", b"\x1bA"),
                ("kitty=1", b"\x1b[97;4u"),
                ("kitty=12", b"\x1b[97:65;4u"),
            ],
        ),
    );
}

/// **On another layout the key is that layout's own, and AltGr's
/// character stays the key**: German Shift+Z is the `z` key with `Z`
/// shifted and the PC-101 `y` as its base layout key, `CSI 122 : 90 : 121
/// ; 2 u`; Russian Shift+С is `CSI 1089 : 1057 : 99 ; 2 u`. AltGr+Shift+1
/// on German types `¡`, and AltGr is no modifier the protocol carries, so
/// the key is `¡` with Shift, `CSI 161 ; 2 u`, as kitty keeps the keysym
/// when a modifier it does not report was consumed and foot its level-0
/// key only when none is held (`[[source: kitty-xkb-glfw-c]]`,
/// `[[source: foot-2705e36-input]]`). Perturbation: take the clean key
/// whatever was consumed and AltGr+Shift+1 reads `CSI 49 ; 2 u`, the `1`
/// key with Shift, AltGr lost.
#[test]
fn another_layouts_key_is_its_own_and_altgr_keeps_its_character() {
    let de = keymap("keymap-de.xkb");
    let key = |km: &Keymap, n: &str| km.keycode_named(n).expect(n);
    let z = press(&de, 0, SHIFT, 0, key(&de, "AD06"));
    assert_eq!(z.code, KeyCode::Char(Codepoint::from_char('z')));
    check(
        "German Shift+Z",
        (
            z,
            &[
                ("legacy", b"Z"),
                ("kitty=8", b"\x1b[122;2u"),
                ("kitty=12", b"\x1b[122:90:121;2u"),
            ],
        ),
    );
    let one = press(&de, 0, ALTGR | SHIFT, 0, key(&de, "AE01"));
    assert_eq!(one.code, KeyCode::Char(Codepoint::from_char('¡')));
    assert_eq!(one.shifted, None);
    check(
        "German AltGr+Shift+1",
        (
            one,
            &[("legacy", "¡".as_bytes()), ("kitty=8", b"\x1b[161;2u")],
        ),
    );
    let ru = keymap("keymap-us_de_ru__grp_win_space_toggle.xkb");
    check(
        "Russian Shift+С",
        (
            press(&ru, 2, SHIFT, 0, key(&ru, "AB03")),
            &[
                ("legacy", "С".as_bytes()),
                ("kitty=12", b"\x1b[1089:1057:99;2u"),
            ],
        ),
    );
}

/// Super, Mod4 on the pinned keymaps.
const SUPER: ModMask = 1 << 6;

/// **A Control, Alt or Super chord carries no associated text**, as
/// kitty's keyboard layer gives a key none while any of them is held
/// ("xkb returns text even if alt and/or super are pressed", `[[source:
/// kitty-xkb-glfw-c]]`) and foot counts no text where a modifier it
/// reports was not consumed (`[[source: foot-2705e36-input]]`): Control+a
/// is `CSI 97 ; 5 u` under "report associated text", where it went out as
/// `CSI 97 ; 5 ; 97 u`, and Alt+a `CSI 97 ; 3 u`. The legacy layer sends
/// what it sent, the character the keymap's lookup gives: Alt+a `ESC a`,
/// Control+1 `1`, Caps Lock and Alt+a `ESC A`. Shift alone, Caps Lock and
/// AltGr keep the text, which is what they typed. Perturbation: give a
/// chord its text again and every chord row reads its codepoint third;
/// send the lookup's character nowhere and Alt+a sends nothing.
#[test]
fn a_control_alt_or_super_chord_carries_no_associated_text() {
    let km = keymap("keymap-us.xkb");
    let key = |km: &Keymap, n: &str| km.keycode_named(n).expect(n);
    let a = key(&km, "AC01");
    let ctrl = press(&km, 0, CTRL, 0, a);
    assert_eq!(ctrl.text, None);
    check(
        "Ctrl+a",
        (
            ctrl,
            &[
                ("legacy", b"\x01"),
                ("kitty=1", b"\x1b[97;5u"),
                ("kitty=24", b"\x1b[97;5u"),
                ("kitty=28", b"\x1b[97;5u"),
                ("mok=2", b"\x1b[27;5;97~"),
            ],
        ),
    );
    check(
        "Alt+a",
        (
            press(&km, 0, ALT, 0, a),
            &[
                ("legacy", b"\x1ba"),
                ("kitty=1", b"\x1b[97;3u"),
                ("kitty=24", b"\x1b[97;3u"),
            ],
        ),
    );
    check(
        "Super+a",
        (
            press(&km, 0, SUPER, 0, a),
            &[("legacy", b"a"), ("kitty=24", b"\x1b[97;9u")],
        ),
    );
    check(
        "Ctrl+1",
        (
            press(&km, 0, CTRL, 0, key(&km, "AE01")),
            &[("legacy", b"1"), ("kitty=24", b"\x1b[49;5u")],
        ),
    );
    check(
        "Ctrl+Shift+a",
        (
            press(&km, 0, CTRL | SHIFT, 0, a),
            &[("legacy", b"\x01"), ("kitty=24", b"\x1b[97;6u")],
        ),
    );
    check(
        "Alt+Shift+a",
        (
            press(&km, 0, ALT | SHIFT, 0, a),
            &[
                ("legacy", b"\x1bA"),
                ("kitty=24", b"\x1b[97;4u"),
                ("kitty=28", b"\x1b[97:65;4u"),
            ],
        ),
    );
    check(
        "Caps+Alt+a",
        (
            press(&km, 0, ALT, LOCK, a),
            &[("legacy", b"\x1bA"), ("kitty=24", b"\x1b[97;67u")],
        ),
    );
    // What Shift, Caps Lock and AltGr typed is the text.
    check(
        "Shift+a",
        (
            press(&km, 0, SHIFT, 0, a),
            &[("kitty=24", b"\x1b[97;2;65u")],
        ),
    );
    check(
        "Caps+a",
        (
            press(&km, 0, 0, LOCK, a),
            &[("kitty=24", b"\x1b[97;65;65u")],
        ),
    );
    let de = keymap("keymap-de.xkb");
    check(
        "German AltGr+q",
        (
            press(&de, 0, ALTGR, 0, key(&de, "AD01")),
            &[("legacy", b"@"), ("kitty=24", b"\x1b[64;1;64u")],
        ),
    );
}
