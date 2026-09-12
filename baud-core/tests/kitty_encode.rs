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
//! `Char` and the keypad are not here: their kitty number is the *unshifted*
//! codepoint, which the `KeyEvent` does not yet carry, so the encoder sends
//! nothing for them and one test pins that so the gap is visible.

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
    };
    let repeat = KeyEvent {
        code: up,
        mods: Modifiers::NONE,
        kind: KeyEventKind::Repeat,
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

/// **The deferred gap, pinned.** A `Char` under kitty needs its unshifted
/// codepoint, which the `KeyEvent` does not yet carry, so the encoder sends
/// nothing rather than a wrong number. When the `KeyEvent` extension lands this
/// test flips, which is the reminder to update it.
#[test]
fn a_char_under_kitty_sends_nothing_yet() {
    let a = KeyCode::Char(Codepoint::from_ascii(StreamByte::from_stream(b'a')));
    // Under all-keys, a plain char would be `CSI 97 u` -- once the codepoint is
    // carried. For now it is dropped.
    assert!(
        pressed(b"\x1b[=8u", KeyEvent::press(a, Modifiers::NONE)).is_empty(),
        "Char encoding is deferred to the KeyEvent extension"
    );
}
