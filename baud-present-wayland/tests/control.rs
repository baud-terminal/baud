//! **Control on a non-Latin layout, and the base layout key**, played on
//! the pinned keymaps through `baud-xkb`: the US keymap the PC-101 table
//! is held to, the US, German and Russian keymap with each group locked,
//! and the German keymap alone, which has no other group to fall back to.

use baud_domain::{Codepoint, KeyCode, KeyEvent};
use baud_present_wayland::keys;
use baud_xkb::{Keymap, Mods, State};

fn keymap(name: &str) -> Keymap {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../baud-xkb/tests/fixtures")
            .join(name),
    )
    .expect("the pinned keymap");
    Keymap::from_text(&text).expect("it compiles")
}

/// **The PC-101 table is the US keymap's first level**: every key it names
/// types that character unshifted on the pinned US keymap, and every other
/// key that types a printable ASCII character there is one a 101-key board
/// lacks -- the ISO key between Shift and Z, and the two parenthesis keys
/// evdev names 179 and 180 -- or a keypad key.
#[test]
fn pc101_is_the_us_keymaps_first_level() {
    let km = keymap("keymap-us.xkb");
    let st = State::new(&km);
    let mut named = 0;
    for code in (8..256).map(baud_xkb::Keycode) {
        let Some(l) = st.lookup(code) else { continue };
        let typed = l
            .sym
            .to_char()
            .filter(|c| c.is_ascii() && !c.is_ascii_control());
        match (keys::pc101(code), typed) {
            (Some(want), Some(got)) => {
                assert_eq!(want, got, "keycode {code:?}");
                named += 1;
            }
            (Some(want), None) => {
                panic!("keycode {code:?}: the table says {want:?}, the keymap nothing")
            }
            (None, Some(got)) => {
                let name = km.key(code).map(|k| k.name.as_str()).unwrap_or("?");
                assert!(
                    ["LSGT", "I187", "I188"].contains(&name) || l.sym.is_keypad(),
                    "{name} types {got:?} and is not in the table"
                );
            }
            (None, None) => {}
        }
    }
    assert_eq!(named, 48, "the table's every key");
}

/// A press of the key named `key` with `mods` held and `group` locked.
const PRESS: fn(&Keymap, usize, baud_xkb::ModMask, &str) -> KeyEvent = |km, group, mods, key| {
    let mut st = State::new(km);
    st.update(
        Mods {
            depressed: mods,
            ..Mods::default()
        },
        group,
    );
    keys::press(&st, km.keycode_named(key).expect(key)).expect(key)
};

const CTRL: baud_xkb::ModMask = 1 << 2;
const ALT: baud_xkb::ModMask = 1 << 3;

fn cp(c: char) -> Option<Codepoint> {
    Some(Codepoint::from_char(c))
}

/// **Ctrl on a Russian layout takes the key's Latin character, as xterm's
/// does**: with US, German and Russian listed and Russian locked, Ctrl+С
/// names `c` for Control, the first other group's, and `c` as its base
/// layout key; Ctrl+Ж the US `;`. On German, Ctrl+Ö takes the US `;` the
/// same way, and the Z key, which types `z` there, needs no fallback and
/// reports the PC-101 `y` as its base layout key. Alt alone is not
/// Control. On US nothing changes, Shift held or not. On the German
/// keymap alone Ctrl+Ö has no other group, and keeps its letter.
#[test]
fn control_falls_back_to_another_layouts_ascii_key() {
    let km = keymap("keymap-us_de_ru__grp_win_space_toggle.xkb");
    let es = PRESS(&km, 2, CTRL, "AB03");
    assert_eq!(es.code, KeyCode::Char(Codepoint::from_char('\u{441}')));
    assert_eq!((es.control, es.base_layout), (cp('c'), cp('c')));
    let zhe = PRESS(&km, 2, CTRL, "AC10");
    assert_eq!((zhe.control, zhe.base_layout), (cp(';'), cp(';')));
    let es_alt = PRESS(&km, 2, ALT, "AB03");
    assert_eq!((es_alt.control, es_alt.base_layout), (None, cp('c')));
    let oe = PRESS(&km, 1, CTRL, "AC10");
    assert_eq!(oe.code, KeyCode::Char(Codepoint::from_char('ö')));
    assert_eq!(oe.control, cp(';'));
    let z = PRESS(&km, 1, CTRL, "AD06");
    assert_eq!(z.code, KeyCode::Char(Codepoint::from_char('z')));
    assert_eq!((z.control, z.base_layout), (None, cp('y')));
    let c = PRESS(&km, 0, CTRL, "AB03");
    assert_eq!((c.control, c.base_layout), (None, None));
    let shifted = PRESS(&km, 0, CTRL | 1, "AB03");
    assert_eq!(
        shifted.base_layout, None,
        "Shift does not make the key another"
    );
    let de = keymap("keymap-de.xkb");
    let alone = PRESS(&de, 0, CTRL, "AC10");
    assert_eq!(alone.code, KeyCode::Char(Codepoint::from_char('ö')));
    assert_eq!(alone.control, None, "no other group");
}

/// **A keymap with a Hebrew layout says so, for DECHEBM**: a group whose
/// first level types a letter alef to tav, here SI-1452's shin on `AC01`
/// beside the US group; the pinned US keymap, and the US, German and
/// Russian one, whose third group types Cyrillic, have none.
#[test]
fn a_keymap_with_a_hebrew_layout_says_so() {
    use baud_domain::HebrewLayout;
    let us_il = "xkb_keymap {\n\
        xkb_keycodes \"t\" { minimum = 8; maximum = 255; <AC01> = 38; };\n\
        xkb_types \"t\" {\n\
            type \"TWO_LEVEL\" { modifiers = Shift; map[Shift] = Level2;\n\
                level_name[Level1] = \"Base\"; level_name[Level2] = \"Shift\"; };\n\
            type \"ALPHABETIC\" { modifiers = Shift+Lock; map[Shift] = Level2;\n\
                map[Lock] = Level2; level_name[Level1] = \"Base\";\n\
                level_name[Level2] = \"Caps\"; };\n\
        };\n\
        xkb_compat \"t\" { };\n\
        xkb_symbols \"t\" {\n\
            name[1] = \"English (US)\"; name[2] = \"Hebrew\";\n\
            key <AC01> { symbols[1] = [ a, A ], symbols[2] = [ hebrew_shin, A ] };\n\
        };\n\
    };\n";
    let km = Keymap::from_text(us_il).expect("it compiles");
    assert_eq!(keys::hebrew_layout(&km), HebrewLayout::Present);
    for name in ["keymap-us.xkb", "keymap-us_de_ru__grp_win_space_toggle.xkb"] {
        assert_eq!(
            keys::hebrew_layout(&keymap(name)),
            HebrewLayout::Absent,
            "{name}"
        );
    }
}
