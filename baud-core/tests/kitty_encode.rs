#![deny(dead_code)]
//! `[[source: kitty-keyboard]]`'s progressive-enhancement **encoding** for the
//! functional keys, under the disambiguate, event-type and all-keys flags.
//!
//! The table of key numbers is gated against `scripts/extract-kitty-keys.py`
//! through the encoded output rather than by inspecting a private function: for
//! each functional key the test looks up the spec's number and final byte and
//! asserts the bytes `Term::key` produces are exactly that CSI-u form. A
//! transcription slip in the encoder then fails against the source, the same
//! discipline the base layer keeps against `extract-keys.py`.
//!
//! `Char` reports its unshifted codepoint and the keypad its dedicated kitty
//! numbers; the remaining gap is the media and individual-modifier keys, which
//! `KeyCode` does not model yet.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use std::collections::HashMap;

/// Set the given kitty flags, press one key, and return the bytes it produced.
fn pressed(setup: &[u8], e: KeyEvent) -> Vec<u8> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, setup);
    t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
    // The set/push sequence writes nothing to the reply, so what remains is the
    // key's own bytes.
    assert!(t.reply().is_empty(), "negotiation must not write a reply");
    t.key(e);
    t.reply().to_vec()
}

/// name -> (number, final byte), read live from the pinned source.
fn functional_table() -> HashMap<String, (u32, u8)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/extract-kitty-keys.py"))
        .current_dir(root)
        .output()
        .expect("python3 to run the extractor");
    assert!(out.status.success(), "extract-kitty-keys.py failed");
    let text = String::from_utf8(out.stdout).unwrap();
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut f = line.split('\t');
        let (name, num, fin) = (f.next().unwrap(), f.next().unwrap(), f.next().unwrap());
        map.insert(name.to_string(), (num.parse().unwrap(), fin.as_bytes()[0]));
    }
    map
}

/// The CSI-u form for a functional key with one modifier present (so the
/// parameters are never omitted): `CSI <number|1> ; <mods> <final>`.
fn csi_u(number: u32, final_byte: u8, mods_wire: u16) -> Vec<u8> {
    let mut v = vec![0x1b, b'['];
    let letter = final_byte != b'u' && final_byte != b'~';
    let n = if letter { 1 } else { number };
    v.extend_from_slice(n.to_string().as_bytes());
    v.push(b';');
    v.extend_from_slice(mods_wire.to_string().as_bytes());
    v.push(final_byte);
    v
}

/// **Every functional key encodes to the number and final the source gives.**
/// Pressed with ctrl held so the number is always in the output.
#[test]
fn functional_keys_match_the_source() {
    let table = functional_table();
    // Positive control: the source parsed, and the keys we bridge are in it.
    assert!(table.len() >= 80, "extractor returned too few rows");

    let ctrl = Modifiers::CTRL; // kitty_wire = 5
    // (KeyCode, its name in the functional table)
    let mut cases: Vec<(KeyCode, &str)> = vec![
        (KeyCode::Escape, "ESCAPE"),
        (KeyCode::Up, "UP"),
        (KeyCode::Down, "DOWN"),
        (KeyCode::Left, "LEFT"),
        (KeyCode::Right, "RIGHT"),
        (KeyCode::Home, "HOME"),
        (KeyCode::End, "END"),
        (KeyCode::Insert, "INSERT"),
        (KeyCode::Delete, "DELETE"),
        (KeyCode::PageUp, "PAGE_UP"),
        (KeyCode::PageDown, "PAGE_DOWN"),
    ];
    // F1..F20 by their names.
    let fnames = [
        "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14",
        "F15", "F16", "F17", "F18", "F19", "F20",
    ];
    for (i, name) in fnames.iter().enumerate() {
        cases.push((
            KeyCode::Function(FnKey::new(u8::try_from(i + 1).unwrap()).unwrap()),
            name,
        ));
    }

    for (code, name) in cases {
        let (number, fin) = table[name];
        let got = pressed(b"\x1b[=1u", KeyEvent::press(code, ctrl));
        let want = csi_u(number, fin, 5);
        assert_eq!(got, want, "{name}: encoded the wrong CSI-u form");
    }
}

/// The worked forms the spec names in prose, spelled out.
#[test]
fn the_worked_forms_are_what_baud_sends() {
    // Escape, disambiguated, no modifiers: `CSI 27 u`.
    assert_eq!(
        pressed(
            b"\x1b[=1u",
            KeyEvent::press(KeyCode::Escape, Modifiers::NONE)
        ),
        b"\x1b[27u"
    );
    // An unmodified arrow keeps the legacy letter form -- number 1 omitted.
    assert_eq!(
        pressed(b"\x1b[=1u", KeyEvent::press(KeyCode::Up, Modifiers::NONE)),
        b"\x1b[A"
    );
    // ctrl+Home: `CSI 1 ; 5 H`.
    assert_eq!(
        pressed(b"\x1b[=1u", KeyEvent::press(KeyCode::Home, Modifiers::CTRL)),
        b"\x1b[1;5H"
    );
    // F1 unmodified: `CSI P`, and shift+F1: `CSI 1 ; 2 P`.
    assert_eq!(
        pressed(
            b"\x1b[=1u",
            KeyEvent::press(KeyCode::Function(FnKey::new(1).unwrap()), Modifiers::NONE)
        ),
        b"\x1b[P"
    );
    assert_eq!(
        pressed(
            b"\x1b[=1u",
            KeyEvent::press(KeyCode::Function(FnKey::new(1).unwrap()), Modifiers::SHIFT)
        ),
        b"\x1b[1;2P"
    );
}

/// Event types add a `:2` (repeat) or `:3` (release) sub-field, and the
/// modifier field defaults to `1` to carry it when nothing is held. Without the
/// enhancement, a release is not reported at all.
#[test]
fn event_types_are_reported_only_when_enabled() {
    let up = KeyCode::Up;
    let release = KeyEvent {
        code: up,
        mods: Modifiers::NONE,
        kind: KeyEventKind::Release,
        text: None,
        shifted: None,
        base_layout: None,
    };
    let repeat = KeyEvent {
        code: up,
        mods: Modifiers::NONE,
        kind: KeyEventKind::Repeat,
        text: None,
        shifted: None,
        base_layout: None,
    };

    // Disambiguate only: a release is silent, a repeat looks like a press.
    assert!(
        pressed(b"\x1b[=1u", release).is_empty(),
        "no release without event types"
    );
    assert_eq!(
        pressed(b"\x1b[=1u", repeat),
        b"\x1b[A",
        "repeat is a press without event types"
    );

    // Event types on (flags = disambiguate|event-types = 3).
    assert_eq!(pressed(b"\x1b[=3u", release), b"\x1b[1;1:3A");
    assert_eq!(pressed(b"\x1b[=3u", repeat), b"\x1b[1;1:2A");
}

/// Enter/Tab/Backspace keep their legacy bytes under disambiguate, and become
/// CSI-u only under `Report all keys as escape codes`.
#[test]
fn the_three_exceptions_stay_legacy_until_all_keys() {
    // Disambiguate: Enter is still CR.
    assert_eq!(
        pressed(
            b"\x1b[=1u",
            KeyEvent::press(KeyCode::Enter, Modifiers::NONE)
        ),
        b"\r"
    );
    // All-keys (flag 8 = value 8): Enter is `CSI 13 u`.
    assert_eq!(
        pressed(
            b"\x1b[=8u",
            KeyEvent::press(KeyCode::Enter, Modifiers::NONE)
        ),
        b"\x1b[13u"
    );
}

/// **A `Char`'s number is its unshifted codepoint, which is the whole point of
/// modelling `KeyCode::Char` as the identity.** `[[source: kitty-keyboard]]`:
/// "the codepoint used is always the un-shifted version"; `ctrl+shift+a` "must
/// not be `CSI 65`". So shift is carried in the modifier field while the number
/// stays 97.
#[test]
fn a_char_encodes_its_unshifted_codepoint() {
    let a = KeyCode::Char(Codepoint::from_ascii(StreamByte::from_stream(b'a')));

    // All-keys: a plain char is `CSI 97 u` -- text is not sent.
    assert_eq!(
        pressed(b"\x1b[=8u", KeyEvent::press(a, Modifiers::NONE)),
        b"\x1b[97u"
    );

    // ctrl+a under disambiguate: `CSI 97 ; 5 u`, the unshifted 97 with ctrl.
    assert_eq!(
        pressed(b"\x1b[=1u", KeyEvent::press(a, Modifiers::CTRL)),
        b"\x1b[97;5u"
    );

    // shift+a: the number is still 97 (unshifted), shift in the modifier field
    // -- `CSI 97 ; 2 u`, never `CSI 65`. `KeyEvent::press` gives Char the
    // identity `a`; a real embedder would also set `text = 'A'`, unused here
    // because the associated-text flag is off.
    assert_eq!(
        pressed(b"\x1b[=8u", KeyEvent::press(a, Modifiers::SHIFT)),
        b"\x1b[97;2u"
    );
}

fn text_key(
    ch: char,
    mods: Modifiers,
    text: Option<char>,
    shifted: Option<char>,
    base: Option<char>,
) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(Codepoint::from_char(ch)),
        mods,
        kind: KeyEventKind::Press,
        text: text.map(Codepoint::from_char),
        shifted: shifted.map(Codepoint::from_char),
        base_layout: base.map(Codepoint::from_char),
    }
}

/// **Associated text (flag 16): the spec's worked example.** `shift+a` with the
/// produced text `A` reports `CSI 97 ; 2 ; 65 u` -- unshifted 97, shift in the
/// modifier field, text 65. And the modifier field is an explicit `1`, never
/// empty, when text is carried with no modifier held.
#[test]
fn associated_text_matches_the_worked_example() {
    // flags = all-keys | associated-text = 8 | 16 = 24.
    assert_eq!(
        pressed(
            b"\x1b[=24u",
            text_key('a', Modifiers::SHIFT, Some('A'), None, None)
        ),
        b"\x1b[97;2;65u"
    );
    // Plain 'a' with text: the modifier field is `1`, not empty.
    assert_eq!(
        pressed(
            b"\x1b[=24u",
            text_key('a', Modifiers::NONE, Some('a'), None, None)
        ),
        b"\x1b[97;1;97u"
    );
    // Without the text flag, all-keys reports the key event only -- no text.
    assert_eq!(
        pressed(
            b"\x1b[=8u",
            text_key('a', Modifiers::SHIFT, Some('A'), None, None)
        ),
        b"\x1b[97;2u"
    );
}

/// **Alternate keys (flag 4): shifted only under shift, base as an empty-slot
/// third sub-field.** `[[source: kitty-keyboard]]`: the shifted key "must be
/// present only if shift is also present", and a base-only report uses an
/// empty shifted slot, `CSI code::base`.
#[test]
fn alternate_keys_attach_to_the_number() {
    // shift+a is a text key -- not routed under disambiguate alone -- so the
    // alternate-key case needs all-keys to route it: flags = all | alt = 12.
    // shift+a with shifted 'A': CSI 97:65 ; 2 u.
    assert_eq!(
        pressed(
            b"\x1b[=12u",
            text_key('a', Modifiers::SHIFT, Some('A'), Some('A'), None)
        ),
        b"\x1b[97:65;2u"
    );
    // flags = disambiguate | alternate-keys = 1 | 4 = 5 for the ctrl cases,
    // which route on ctrl.
    // ctrl+a, base-layout 'c', no shift: empty shifted slot -> CSI 97::99 ; 5 u.
    assert_eq!(
        pressed(
            b"\x1b[=5u",
            text_key('a', Modifiers::CTRL, None, Some('A'), Some('c'))
        ),
        b"\x1b[97::99;5u"
    );
    // ctrl+shift+a with both: CSI 97:65:99 ; 6 u.
    assert_eq!(
        pressed(
            b"\x1b[=5u",
            text_key(
                'a',
                Modifiers::CTRL.with(Modifiers::SHIFT),
                Some('A'),
                Some('A'),
                Some('c')
            )
        ),
        b"\x1b[97:65:99;6u"
    );
    // The shifted key is dropped when shift is not held, even if supplied.
    assert_eq!(
        pressed(
            b"\x1b[=5u",
            text_key('a', Modifiers::CTRL, None, Some('A'), None)
        ),
        b"\x1b[97;5u"
    );
    // Alternate keys are off without the flag: ctrl+a is just CSI 97 ; 5 u.
    assert_eq!(
        pressed(
            b"\x1b[=1u",
            text_key('a', Modifiers::CTRL, None, Some('A'), Some('c'))
        ),
        b"\x1b[97;5u"
    );
}

/// **Every keypad key encodes to its dedicated kitty number from the source.**
/// Under all-keys, with no modifier, that is `CSI <number> u`.
#[test]
fn keypad_keys_match_the_source() {
    let table = functional_table();
    let cases: [(KeypadKey, &str); 18] = [
        (KeypadKey::Digit0, "KP_0"),
        (KeypadKey::Digit1, "KP_1"),
        (KeypadKey::Digit2, "KP_2"),
        (KeypadKey::Digit3, "KP_3"),
        (KeypadKey::Digit4, "KP_4"),
        (KeypadKey::Digit5, "KP_5"),
        (KeypadKey::Digit6, "KP_6"),
        (KeypadKey::Digit7, "KP_7"),
        (KeypadKey::Digit8, "KP_8"),
        (KeypadKey::Digit9, "KP_9"),
        (KeypadKey::Period, "KP_DECIMAL"),
        (KeypadKey::Divide, "KP_DIVIDE"),
        (KeypadKey::Multiply, "KP_MULTIPLY"),
        (KeypadKey::Minus, "KP_SUBTRACT"),
        (KeypadKey::Add, "KP_ADD"),
        (KeypadKey::Enter, "KP_ENTER"),
        (KeypadKey::Equal, "KP_EQUAL"),
        (KeypadKey::Comma, "KP_SEPARATOR"),
    ];
    for (k, name) in cases {
        let (number, _) = table[name];
        let got = pressed(
            b"\x1b[=8u",
            KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE),
        );
        let mut want = vec![0x1b, b'['];
        want.extend_from_slice(number.to_string().as_bytes());
        want.push(b'u');
        assert_eq!(got, want, "{name}");
    }

    // PF1 shares F1's form -- `CSI P`, number omitted with no modifiers.
    assert_eq!(
        pressed(
            b"\x1b[=8u",
            KeyEvent::press(KeyCode::Keypad(KeypadKey::Pf1), Modifiers::NONE)
        ),
        b"\x1b[P"
    );
    // The keypad Space and Tab have no form in the table, so they send nothing.
    for k in [KeypadKey::Space, KeypadKey::Tab] {
        assert!(
            pressed(
                b"\x1b[=8u",
                KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE)
            )
            .is_empty(),
            "keypad {k:?} has no kitty form"
        );
    }
}
