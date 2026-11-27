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
        KeypadKey::Left,
        KeypadKey::Right,
        KeypadKey::Up,
        KeypadKey::Down,
        KeypadKey::PageUp,
        KeypadKey::PageDown,
        KeypadKey::Home,
        KeypadKey::End,
        KeypadKey::Insert,
        KeypadKey::Delete,
        KeypadKey::Begin,
    ] {
        codes.push(KeyCode::Keypad(k));
    }

    // The modes that change what a key sends, driven as escape sequences so
    // the encoder is reached the way an application reaches it.
    // The fourth is every profile's `smkx` that sets both, xterm's and
    // vt100's: a key capability holds after the profile's `smkx`, which
    // sends the two modes at once.
    let setups: [&[u8]; 5] = [
        b"",              // ANSI, cursor keys normal, keypad numeric
        b"\x1b[?1h",      // DECCKM: cursor keys application
        b"\x1b=",         // DECKPAM: keypad application
        b"\x1b[?1h\x1b=", // both, the `smkx` of xterm, vt100, vt102 and nsterm
        b"\x1b[?2l",      // DECANM off: VT52
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
    // **A mouse report is input too**: `kmous` is the prefix of the
    // reports baud sends under `?1000` with no extended encoding, so the
    // report itself is enumerated and `kmous` is matched as its prefix.
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    t.write_at(BytePos::ZERO, b"\x1b[?1000h");
    t.feed(BytePos::ZERO, ReadLen::from_index(8));
    t.clear_reply();
    let press = MouseEvent {
        button: MouseButton::Left,
        mods: MouseMods::NONE,
        kind: MouseKind::Press,
        col: Col::at_index(0),
        row: Row::from_index(0),
        px: None,
    };
    if t.mouse_report(press) {
        out.insert(t.reply().to_vec());
    }
    out
}

/// Whether baud sends `cap`: one of its sequences, or for `kmous` the start
/// of one, since the capability names only the report's prefix.
fn sends(sendable: &std::collections::BTreeSet<Vec<u8>>, cap: &Cap) -> bool {
    match cap.name.as_str() {
        "kmous" => sendable.iter().any(|s| s.starts_with(&cap.bytes)),
        _ => sendable.contains(&cap.bytes),
    }
}

fn profile(name: &str) -> Vec<Cap> {
    let path = format!(
        "{}/../../docs/research/sources/terminfo/{name}.ti",
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
        let hit = inputs.iter().filter(|c| sends(&sendable, c)).count();
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

/// The profiles `docs/input-coverage.md`'s table names, each with the
/// capabilities it names for it.
fn documented_unsent() -> std::collections::BTreeMap<String, std::collections::BTreeSet<String>> {
    let path = format!(
        "{}/../../docs/input-coverage.md",
        env!("CARGO_MANIFEST_DIR")
    );
    let doc = std::fs::read_to_string(path).expect("input-coverage.md");
    let start = doc
        .find("## Terminfo input capabilities baud does not send")
        .expect("the section");
    let names = |cell: &str| -> Vec<String> {
        cell.split('`')
            .skip(1)
            .step_by(2)
            .map(str::to_string)
            .collect()
    };
    let mut out = std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    for line in doc[start..].lines().skip(1) {
        if line.starts_with("## ") {
            break;
        }
        let cells: Vec<&str> = line.split('|').collect();
        if cells.len() < 5 || !cells[1].contains('`') {
            continue;
        }
        assert!(!cells[3].trim().is_empty(), "a row with no reason: {line}");
        for p in names(cells[1]) {
            out.entry(p).or_default().extend(names(cells[2]));
        }
    }
    out
}

/// **Every input capability baud does not send is named, with its
/// reason**, in `docs/input-coverage.md`, and each profile's count in
/// `docs/protocols/profiles/` is the measured one: the document checks the
/// code, so neither list can drift from what the encoder produces.
#[test]
fn the_unsent_input_capabilities_are_named() {
    let sendable = everything_baud_can_send();
    let named = documented_unsent();
    // Positive control: the table was found and read.
    assert!(
        named.get("xterm").is_some_and(|s| s.contains("kb2")),
        "the table did not parse: {named:?}"
    );
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
        let unsent: std::collections::BTreeSet<String> = inputs
            .iter()
            .filter(|c| !sends(&sendable, c))
            .map(|c| c.name.clone())
            .collect();
        let doc = named.get(name).cloned().unwrap_or_default();
        assert_eq!(
            unsent, doc,
            "{name}: unsent (left) against the table (right)"
        );
        let path = format!(
            "{}/../../docs/protocols/profiles/{name}.md",
            env!("CARGO_MANIFEST_DIR")
        );
        let md = std::fs::read_to_string(path).expect("every profile has a page");
        let want = format!(
            "| input capabilities | {} — **{} sent**",
            inputs.len(),
            inputs.len() - unsent.len()
        );
        assert!(md.contains(&want), "{name}.md does not say {want:?}");
    }
}
