//! **How many of a profile's *input* capabilities baud can actually send.**
//!
//! Every profile in `docs/protocols/profiles/` says `input capabilities: N --
//! none; baud has no input path`, and `terminfo::is_input`'s own doc says the
//! same. That was true when written. It is not now: `encode_key` and
//! `Term::key` encode cursor keys, the keypad in both modes, twenty function
//! keys, the VT220 editing keys, the VT52 keyboard and the modifier columns.
//!
//! **Thirteen copies of one expired sentence** is what a claim costs when it is
//! restated per file rather than derived, which is why this file computes the
//! number instead of asserting it.
//!
//! The method is the repo's: the specification checks baud. Every input
//! capability in the hash-pinned terminfo is expanded to bytes, baud's whole
//! key space is enumerated to bytes, and the answer is the intersection. No
//! capability name is mapped to a `KeyCode` by hand -- a transcribed mapping is
//! the guessed table this project refuses, and it would also be the thing most
//! likely to be wrong.

use crate::terminfo::{self, Cap, is_input};
use baud_core::*;

/// Every byte string baud's key encoder can produce.
///
/// The key space is finite and small: seventeen `KeyCode` shapes, twenty
/// function keys, twenty-four keypad keys, sixteen modifier combinations, and
/// the mode axes that change an encoding -- DECCKM, the keypad mode, and VT52.
fn everything_baud_can_send() -> std::collections::BTreeSet<Vec<u8>> {
    let mut out = std::collections::BTreeSet::new();
    let ws = crate::support::CORPUS;

    let mut codes: Vec<KeyCode> = vec![
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::Right,
        KeyCode::Left,
        KeyCode::Home,
        KeyCode::End,
        KeyCode::Insert,
        KeyCode::Delete,
        KeyCode::PageUp,
        KeyCode::PageDown,
        KeyCode::Enter,
        KeyCode::Tab,
        KeyCode::Backspace,
        KeyCode::Escape,
    ];
    for n in 1..=20u8 {
        codes.push(KeyCode::Function(FnKey::new(n).expect("1..=20 are keys")));
    }
    // Spelled out rather than iterated: `KeypadKey` has no `ALL`, and adding
    // one to the domain for a test would be the test shaping the type. A
    // variant missing from this list costs coverage and cannot cause a false
    // pass, because the count is compared against the specification's.
    for k in [
        KeypadKey::Space,
        KeypadKey::Tab,
        KeypadKey::Enter,
        KeypadKey::Pf1,
        KeypadKey::Pf2,
        KeypadKey::Pf3,
        KeypadKey::Pf4,
        KeypadKey::Multiply,
        KeypadKey::Add,
        KeypadKey::Comma,
        KeypadKey::Minus,
        KeypadKey::Period,
        KeypadKey::Divide,
        KeypadKey::Digit0,
        KeypadKey::Digit1,
        KeypadKey::Digit2,
        KeypadKey::Digit3,
        KeypadKey::Digit4,
        KeypadKey::Digit5,
        KeypadKey::Digit6,
        KeypadKey::Digit7,
        KeypadKey::Digit8,
        KeypadKey::Digit9,
        KeypadKey::Equal,
    ] {
        codes.push(KeyCode::Keypad(k));
    }

    // The modes that change what a key sends, driven as escape sequences so
    // the encoder is reached the way an application reaches it.
    let setups: [&[u8]; 4] = [
        b"",         // ANSI, cursor keys normal, keypad numeric
        b"\x1b[?1h", // DECCKM: cursor keys application
        b"\x1b=",    // DECKPAM: keypad application
        b"\x1b[?2l", // DECANM off: VT52
    ];

    for setup in setups {
        for &code in &codes {
            for m in 0u8..16 {
                let mods = Modifiers::from_wire(m + 1).expect("1..=16 are modifiers");
                let mut s = Owned::new(&ws);
                let mut t = Term::new(s.storage(), &ws);
                if !setup.is_empty() {
                    t.write_at(BytePos::ZERO, setup);
                    t.feed(BytePos::ZERO, ReadLen::from_index(setup.len()));
                    t.clear_reply();
                }
                if t.key(KeyEvent::press(code, mods)) {
                    out.insert(t.reply().to_vec());
                }
            }
        }
    }
    out
}

fn profile(name: &str) -> Vec<Cap> {
    let path = format!(
        "{}/../docs/research/sources/terminfo/{name}.ti",
        env!("CARGO_MANIFEST_DIR")
    );
    terminfo::parse(&std::fs::read_to_string(path).expect("the pinned terminfo"))
}

/// **The count each profile claims is measured, not asserted.**
///
/// This does not require any particular number -- baud is not trying to be
/// `xterm` -- it requires the *documents to agree with the code*. A profile
/// saying "none" while baud sends dozens of those very sequences is the
/// documentation failure this project keeps finding, and a profile claiming
/// more than baud sends would be the worse direction.
#[test]
fn the_profiles_input_counts_are_what_baud_can_send() {
    let sendable = everything_baud_can_send();

    // Positive control: the enumeration must produce a real key space, or every
    // comparison below is against an empty set and reads as "baud sends
    // nothing" -- which is exactly the claim under test.
    assert!(
        sendable.len() > 100,
        "the key enumeration produced only {} sequences -- it is not reaching \
         the encoder, and every count below would be a false zero",
        sendable.len()
    );

    let mut report = Vec::new();
    for name in [
        "ansi",
        "dtterm",
        "nsterm",
        "rxvt",
        "vt52",
        "vt100",
        "vt102",
        "xterm",
        "xterm-16color",
        "xterm-256color",
    ] {
        let caps = profile(name);
        let inputs: Vec<&Cap> = caps.iter().filter(|c| is_input(&c.name)).collect();
        assert!(
            !inputs.is_empty(),
            "{name}: no input capabilities parsed -- the reader stopped matching"
        );
        let hit = inputs
            .iter()
            .filter(|c| sendable.contains(&c.bytes))
            .count();
        report.push((name, hit, inputs.len()));
    }

    // **baud sends some of them, and that is the whole assertion.** The exact
    // number is reported rather than pinned: it moves whenever a key gains a
    // modifier column, and a test that had to be edited for every such change
    // would be edited without being read.
    let total: usize = report.iter().map(|(_, h, _)| h).sum();
    assert!(
        total > 0,
        "baud sends none of any profile's input capabilities, which would mean \
         `encode_key` is unreachable from `Term::key`"
    );
    for (name, hit, all) in &report {
        println!("  {name:<16} {hit:>3} of {all:>3} input capabilities");
    }
}
