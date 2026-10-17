#![deny(dead_code)]
//! `[[source: kitty-keyboard]]`'s progressive-enhancement **encoding**, under
//! all five flags: disambiguate, event types, alternate keys, all-keys and
//! associated text.
//!
//! The table of key numbers is gated against `scripts/extract-kitty-keys.py`
//! through the encoded output rather than by inspecting a private function: for
//! each functional key the test looks up the spec's number and final byte and
//! asserts the bytes `Term::key` produces are exactly that CSI-u form. A
//! transcription slip in the encoder then fails against the source, the same
//! discipline the base layer keeps against `extract-keys.py`.
//!
//! `Char` reports its unshifted codepoint; the keypad reports its dedicated
//! kitty numbers, navigation keys (`KP_LEFT` to `KP_BEGIN`) included, and the
//! Private Use Area keys -- the locks, F21-F35, the media and the individual
//! modifier keys -- theirs, all gated against the source, and
//! `every_row_of_the_table_is_gated` requires every row to be named by one of
//! them.

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
    let cases = functional_cases();
    for (code, name) in cases {
        let (number, fin) = table[name];
        let got = pressed(b"\x1b[=1u", KeyEvent::press(code, ctrl));
        let want = csi_u(number, fin, 5);
        assert_eq!(got, want, "{name}: encoded the wrong CSI-u form");
    }
    // Enter, Tab and Backspace keep their legacy bytes under disambiguate
    // alone, so their numbers are read under all-keys.
    for (code, name) in c0_cases() {
        let (number, fin) = table[name];
        let got = pressed(b"\x1b[=8u", KeyEvent::press(code, ctrl));
        assert_eq!(got, csi_u(number, fin, 5), "{name}: under all-keys");
    }
}

/// (KeyCode, its name in the functional table): Esc, the cursor and editing
/// keys and F1-F20.
fn functional_cases() -> Vec<(KeyCode, &'static str)> {
    let mut cases: Vec<(KeyCode, &'static str)> = vec![
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
    for (i, name) in fnames.into_iter().enumerate() {
        cases.push((
            KeyCode::Function(FnKey::new(u8::try_from(i + 1).unwrap()).unwrap()),
            name,
        ));
    }

    cases
}

/// The three keys disambiguate leaves on their legacy bytes.
fn c0_cases() -> [(KeyCode, &'static str); 3] {
    [
        (KeyCode::Enter, "ENTER"),
        (KeyCode::Tab, "TAB"),
        (KeyCode::Backspace, "BACKSPACE"),
    ]
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

/// **A key that produces text keeps its legacy bytes, and a repeat of it is a
/// press.** `[[source: kitty-keyboard]]`: "Key events that result in text are
/// reported as plain UTF-8 text, so events are not supported for them, unless
/// the application requests key report mode". So under disambiguation alone,
/// and with event types too, a plain `a` held down types `a` again, and its
/// release sends nothing. The same for Enter, which the spec gives no release
/// event without all-keys-as-escapes. A text key reaches the base encoder
/// here, which dropped every repeat -- so a held `a` typed once.
#[test]
fn a_text_keys_repeat_is_its_text_and_its_release_is_nothing() {
    let a = KeyEvent::press(KeyCode::Char(Codepoint::from_char('a')), Modifiers::NONE);
    let enter = KeyEvent::press(KeyCode::Enter, Modifiers::NONE);
    for setup in [&b"\x1b[=1u"[..], &b"\x1b[=3u"[..]] {
        for (key, text) in [(a, &b"a"[..]), (enter, &b"\r"[..])] {
            assert_eq!(pressed(setup, key), text, "a press");
            let repeat = KeyEvent {
                kind: KeyEventKind::Repeat,
                ..key
            };
            assert_eq!(pressed(setup, repeat), text, "a repeat is the press");
            let release = KeyEvent {
                kind: KeyEventKind::Release,
                ..key
            };
            assert!(pressed(setup, release).is_empty(), "no release");
        }
    }
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

/// **A release carries no associated text; a repeat does.** `[[source:
/// kitty-keyboard]]` embeds the text only for "key events that generate text"
/// (html 1663-1666), and divides key events into "those that produce text and
/// those that do not" (1382), the first sent as their text in the legacy
/// encoding, where "only key press and repeat events are sent" (1691). A
/// release is never sent there, so it produces no text, and the field is
/// absent even when the platform hands the release the character it would
/// have typed. The repeat is the control: the same key, the same flags, a
/// text-generating event that keeps its field.
#[test]
fn a_release_carries_no_associated_text_and_a_repeat_does() {
    // flags = event types | all-keys | associated text = 2 | 8 | 16 = 26.
    let key = text_key('a', Modifiers::NONE, Some('a'), None, None);
    let repeat = KeyEvent {
        kind: KeyEventKind::Repeat,
        ..key
    };
    let release = KeyEvent {
        kind: KeyEventKind::Release,
        ..key
    };
    assert_eq!(pressed(b"\x1b[=26u", key), b"\x1b[97;1;97u", "the press");
    assert_eq!(
        pressed(b"\x1b[=26u", repeat),
        b"\x1b[97;1:2;97u",
        "the repeat"
    );
    assert_eq!(
        pressed(b"\x1b[=26u", release),
        b"\x1b[97;1:3u",
        "the release"
    );
    // And with alternate keys and a modifier, only the text field goes.
    let shifted = text_key('a', Modifiers::SHIFT, Some('A'), Some('A'), None);
    let release = KeyEvent {
        kind: KeyEventKind::Release,
        ..shifted
    };
    assert_eq!(pressed(b"\x1b[=30u", release), b"\x1b[97:65;2:3u");
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
/// Under all-keys, with no modifier, that is `CSI <number> u`; `KP_BEGIN` is
/// the one letter-final row, `1 E`, whose number is omitted when nothing
/// else is carried. Under disambiguate alone with ctrl held, every row,
/// navigation included: "all non text keypad keys will be reported as
/// separate keys with CSI u encoding, using dedicated numbers from the
/// table".
#[test]
fn keypad_keys_match_the_source() {
    let table = functional_table();
    let cases = keypad_cases();
    for (k, name) in cases {
        let (number, fin) = table[name];
        let got = pressed(
            b"\x1b[=8u",
            KeyEvent::press(KeyCode::Keypad(k), Modifiers::NONE),
        );
        let mut want = vec![0x1b, b'['];
        if fin == b'u' || fin == b'~' {
            want.extend_from_slice(number.to_string().as_bytes());
        }
        want.push(fin);
        assert_eq!(got, want, "{name}");
        let ctrl = pressed(
            b"\x1b[=1u",
            KeyEvent::press(KeyCode::Keypad(k), Modifiers::CTRL),
        );
        assert_eq!(ctrl, csi_u(number, fin, 5), "{name} with ctrl");
    }
    // A release under event types carries the modifier field as `1`, so the
    // letter form writes its `1` too.
    let mut release = KeyEvent::press(KeyCode::Keypad(KeypadKey::Begin), Modifiers::NONE);
    release.kind = KeyEventKind::Release;
    assert_eq!(pressed(b"\x1b[=3u", release), b"\x1b[1;1:3E");

    // PF1-PF4 share F1-F4's forms from the table. With no modifier a letter
    // final omits its number, 1, so they are `CSI P`, `CSI Q`, `CSI 13 ~` and
    // `CSI S`; with ctrl held the number is written.
    let pf: [(KeypadKey, &str, &[u8]); 4] = [
        (KeypadKey::Pf1, "F1", b"\x1b[P"),
        (KeypadKey::Pf2, "F2", b"\x1b[Q"),
        (KeypadKey::Pf3, "F3", b"\x1b[13~"),
        (KeypadKey::Pf4, "F4", b"\x1b[S"),
    ];
    for (k, name, bare) in pf {
        let (number, fin) = table[name];
        assert_eq!(bare.last(), Some(&fin), "{name}'s final in the source");
        let key = |m| pressed(b"\x1b[=8u", KeyEvent::press(KeyCode::Keypad(k), m));
        assert_eq!(key(Modifiers::NONE), bare, "{k:?}");
        assert_eq!(
            key(Modifiers::CTRL),
            csi_u(number, fin, 5),
            "{k:?} with ctrl"
        );
    }
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

/// **Every PUA-only key encodes to its dedicated number from the source.**
/// These have no legacy form, so all-keys mode is where they appear:
/// `CSI <number> u`. This is the vocabulary flag 8's "report all keys,
/// including modifier keys" requires before the query may be answered.
#[test]
fn pua_keys_match_the_source() {
    let table = functional_table();
    let cases = pua_cases();
    for (k, name) in cases {
        let (number, _) = table[name];
        let got = pressed(
            b"\x1b[=8u",
            KeyEvent::press(KeyCode::Pua(k), Modifiers::NONE),
        );
        let mut want = vec![0x1b, b'['];
        want.extend_from_slice(number.to_string().as_bytes());
        want.push(b'u');
        assert_eq!(got, want, "{name}");
    }
}

/// (KeypadKey, its name in the functional table), `PF1`-`PF4` aside.
fn keypad_cases() -> Vec<(KeypadKey, &'static str)> {
    vec![
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
        (KeypadKey::Left, "KP_LEFT"),
        (KeypadKey::Right, "KP_RIGHT"),
        (KeypadKey::Up, "KP_UP"),
        (KeypadKey::Down, "KP_DOWN"),
        (KeypadKey::PageUp, "KP_PAGE_UP"),
        (KeypadKey::PageDown, "KP_PAGE_DOWN"),
        (KeypadKey::Home, "KP_HOME"),
        (KeypadKey::End, "KP_END"),
        (KeypadKey::Insert, "KP_INSERT"),
        (KeypadKey::Delete, "KP_DELETE"),
        (KeypadKey::Begin, "KP_BEGIN"),
    ]
}

/// (PuaKey, its name in the functional table).
fn pua_cases() -> Vec<(PuaKey, &'static str)> {
    vec![
        (PuaKey::CapsLock, "CAPS_LOCK"),
        (PuaKey::ScrollLock, "SCROLL_LOCK"),
        (PuaKey::NumLock, "NUM_LOCK"),
        (PuaKey::PrintScreen, "PRINT_SCREEN"),
        (PuaKey::Pause, "PAUSE"),
        (PuaKey::Menu, "MENU"),
        (PuaKey::F21, "F21"),
        (PuaKey::F22, "F22"),
        (PuaKey::F23, "F23"),
        (PuaKey::F24, "F24"),
        (PuaKey::F25, "F25"),
        (PuaKey::F26, "F26"),
        (PuaKey::F27, "F27"),
        (PuaKey::F28, "F28"),
        (PuaKey::F29, "F29"),
        (PuaKey::F30, "F30"),
        (PuaKey::F31, "F31"),
        (PuaKey::F32, "F32"),
        (PuaKey::F33, "F33"),
        (PuaKey::F34, "F34"),
        (PuaKey::F35, "F35"),
        (PuaKey::MediaPlay, "MEDIA_PLAY"),
        (PuaKey::MediaPause, "MEDIA_PAUSE"),
        (PuaKey::MediaPlayPause, "MEDIA_PLAY_PAUSE"),
        (PuaKey::MediaReverse, "MEDIA_REVERSE"),
        (PuaKey::MediaStop, "MEDIA_STOP"),
        (PuaKey::MediaFastForward, "MEDIA_FAST_FORWARD"),
        (PuaKey::MediaRewind, "MEDIA_REWIND"),
        (PuaKey::MediaTrackNext, "MEDIA_TRACK_NEXT"),
        (PuaKey::MediaTrackPrevious, "MEDIA_TRACK_PREVIOUS"),
        (PuaKey::MediaRecord, "MEDIA_RECORD"),
        (PuaKey::LowerVolume, "LOWER_VOLUME"),
        (PuaKey::RaiseVolume, "RAISE_VOLUME"),
        (PuaKey::MuteVolume, "MUTE_VOLUME"),
        (PuaKey::LeftShift, "LEFT_SHIFT"),
        (PuaKey::LeftControl, "LEFT_CONTROL"),
        (PuaKey::LeftAlt, "LEFT_ALT"),
        (PuaKey::LeftSuper, "LEFT_SUPER"),
        (PuaKey::LeftHyper, "LEFT_HYPER"),
        (PuaKey::LeftMeta, "LEFT_META"),
        (PuaKey::RightShift, "RIGHT_SHIFT"),
        (PuaKey::RightControl, "RIGHT_CONTROL"),
        (PuaKey::RightAlt, "RIGHT_ALT"),
        (PuaKey::RightSuper, "RIGHT_SUPER"),
        (PuaKey::RightHyper, "RIGHT_HYPER"),
        (PuaKey::RightMeta, "RIGHT_META"),
        (PuaKey::IsoLevel3Shift, "ISO_LEVEL3_SHIFT"),
        (PuaKey::IsoLevel5Shift, "ISO_LEVEL5_SHIFT"),
    ]
}

/// **Every row of the source's functional table is gated by a test above.**
///
/// The keypad's navigation rows sat ungated for as long as the protocol was
/// called complete, because each test named its own keys and nothing asked
/// which rows no test named. This asks: the names the three tables map,
/// with the keypad's `PF` keys counted under F1-F4, must be exactly the
/// extractor's rows.
#[test]
fn every_row_of_the_table_is_gated() {
    let table = functional_table();
    assert!(table.contains_key("ESCAPE"), "positive control");
    let mut gated: Vec<&str> = functional_cases().into_iter().map(|(_, n)| n).collect();
    gated.extend(c0_cases().into_iter().map(|(_, n)| n));
    gated.extend(keypad_cases().into_iter().map(|(_, n)| n));
    gated.extend(pua_cases().into_iter().map(|(_, n)| n));
    let mut missing: Vec<&str> = table
        .keys()
        .map(String::as_str)
        .filter(|n| !gated.contains(n))
        .collect();
    missing.sort_unstable();
    assert!(missing.is_empty(), "rows no test gates: {missing:?}");
    let mut extra: Vec<&str> = gated
        .into_iter()
        .filter(|n| !table.contains_key(*n))
        .collect();
    extra.sort_unstable();
    assert!(extra.is_empty(), "names not in the source: {extra:?}");
}
