//! The modifier encoding, checked against `[[source: ctlseqs]]`'s own table.
//!
//! This is the direction `tests/conformance/spec_inventory.rs` established for
//! the receive side: the document checks the code. A modifier table written
//! into a test from memory would agree with a modifier table written into the
//! source from the same memory, and neither would have been read.

use baud_domain::*;

fn ctlseqs() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs")
}

/// Parse the `Code | Modifiers` table into `(code, [names])`.
fn modifier_table(src: &str) -> Vec<(u8, Vec<String>)> {
    let mut out = Vec::new();
    let mut seen_header = false;
    for line in src.lines() {
        if line.contains("Code") && line.contains("Modifiers") {
            seen_header = true;
            continue;
        }
        if !seen_header {
            continue;
        }
        let t = line.trim();
        if t.starts_with('-') || t.is_empty() {
            if out.is_empty() {
                continue;
            }
            break;
        }
        let Some((code, names)) = t.split_once('|') else {
            break;
        };
        let Ok(code) = code.trim().parse::<u8>() else {
            break;
        };
        out.push((
            code,
            names
                .split('+')
                .map(|n| n.trim().to_ascii_lowercase())
                .collect(),
        ));
    }
    out
}

/// **Every row of xterm's modifier table round-trips through `Modifiers`.**
///
/// Fifteen rows, 2 through 16. The encoding is `1 + mask`, and the table is
/// the only place that says so — the prose does not.
#[test]
fn the_modifier_codes_match_the_specifications_table() {
    let src = ctlseqs();
    let rows = modifier_table(&src);
    assert_eq!(
        rows.len(),
        15,
        "expected codes 2..=16 from ctlseqs; got {rows:?}. If the source was \
         re-pinned this parser needs updating — an empty or short table would \
         otherwise pass every assertion below by having nothing to check."
    );

    for (code, names) in &rows {
        let mut m = Modifiers::NONE;
        for n in names {
            m = m.with(match n.as_str() {
                "shift" => Modifiers::SHIFT,
                "alt" => Modifiers::ALT,
                "control" | "ctrl" => Modifiers::CTRL,
                "meta" => Modifiers::META,
                other => panic!("unknown modifier name {other:?} in the table"),
            });
        }
        assert_eq!(m.wire(), *code, "code {code} is {names:?}");
        assert_eq!(
            Modifiers::from_wire(*code),
            Some(m),
            "code {code} must read back as what it encoded"
        );
    }
}

/// The worked example the specification gives, verbatim.
///
/// "For example, shift-F5 would be sent as CSI 1 5 ; 2 ~". The `2` is the
/// whole claim being tested — that an unmodified F5 is `CSI 1 5 ~` and the
/// modifier is appended, not folded into the key number.
#[test]
fn shift_f5_carries_modifier_two() {
    let src = ctlseqs();
    assert!(
        src.contains("shift-F5 would be sent as CSI 1 5 ; 2 ~"),
        "the worked example moved; this test is quoting a source it no longer \
         matches"
    );
    assert_eq!(Modifiers::SHIFT.wire(), 2);
    assert!(
        Modifiers::NONE.is_none(),
        "so no parameter is appended at all"
    );
}

/// `NONE` encodes as 1, and an encoder is expected to omit it.
#[test]
fn no_modifier_is_one_and_is_omitted_rather_than_sent() {
    assert_eq!(Modifiers::NONE.wire(), 1);
    assert!(Modifiers::NONE.is_none());
    assert!(!Modifiers::SHIFT.is_none());
}

/// Outside 1..=16 is not a modifier parameter, and is refused rather than
/// clamped.
///
/// Clamping would invent a keypress the application never made — the same
/// reason `DA` under-claims and `DECRQM` answers 0 for a mode it does not
/// implement.
#[test]
fn a_parameter_outside_the_table_is_refused() {
    assert_eq!(Modifiers::from_wire(0), None);
    assert_eq!(Modifiers::from_wire(17), None);
    assert_eq!(Modifiers::from_wire(255), None);
    assert_eq!(Modifiers::from_wire(16), Some(Modifiers::ALL));
}

/// **F1-F4 change introducer when modified, and the rest do not.**
///
/// `[[source: ctlseqs]]`: "As a special case, the SS3 sent before F1 through
/// F4 is altered to CSI when sending a function key modifier as a parameter."
/// Unmodified F1 is `SS3 P`; shift-F1 is `CSI 1 ; 2 P`. An encoder that
/// appends the modifier without changing the introducer sends something no
/// application parses.
#[test]
fn only_f1_to_f4_change_introducer_when_modified() {
    let src = ctlseqs();
    assert!(
        src.contains("SS3  sent before F1 through F4 is altered to CSI"),
        "the special case moved in the source"
    );
    for n in 1..=20u8 {
        let f = FnKey::new(n).expect("F1..F20");
        assert_eq!(f.changes_introducer_when_modified(), n <= 4, "F{n}");
    }
}

/// `FnKey` refuses a number outside the twenty the tables define.
#[test]
fn function_keys_outside_the_tables_are_refused() {
    assert!(FnKey::new(0).is_none());
    assert!(FnKey::new(21).is_none());
    assert_eq!(FnKey::new(1).map(FnKey::number), Some(1));
    assert_eq!(FnKey::new(20).map(FnKey::number), Some(20));
}

/// The set operations, including the one an encoder needs and nothing else
/// does.
///
/// `without` is here because releasing a modifier is a real event on
/// `[[source: kitty-keyboard]]`'s event-type layer, not because symmetry is
/// tidy: a key held while Ctrl is let go reports with Ctrl cleared, and the
/// encoder computes that rather than being handed it.
#[test]
fn modifiers_compose_and_decompose() {
    let both = Modifiers::CTRL.with(Modifiers::SHIFT);
    assert_eq!(both.wire(), 6, "ctlseqs code 6 is Shift + Control");
    assert!(both.has(Modifiers::CTRL) && both.has(Modifiers::SHIFT));

    let dropped = both.without(Modifiers::CTRL);
    assert_eq!(dropped, Modifiers::SHIFT);
    assert!(!dropped.has(Modifiers::CTRL));

    // Removing something that was never set is not an error and not a change.
    assert_eq!(Modifiers::SHIFT.without(Modifiers::META), Modifiers::SHIFT);
    assert_eq!(Modifiers::ALL.without(Modifiers::ALL), Modifiers::NONE);
}

/// A `KeyEvent` built by `press` is a press, which is every event on the base
/// layer.
#[test]
fn a_press_is_the_default_kind() {
    let f5 = FnKey::new(5).expect("F5");
    assert_eq!(f5.number(), 5);

    let e = KeyEvent::press(KeyCode::Function(f5), Modifiers::SHIFT);
    assert_eq!(e.kind, KeyEventKind::Press);
    assert_eq!(
        e.mods.wire(),
        2,
        "the shift-F5 the specification works through"
    );
    assert_eq!(e.code, KeyCode::Function(f5));

    // Only kitty's event-type layer reports anything else, so `Press` is what
    // a terminal that reports presses alone produces -- and is the default.
    assert_eq!(KeyEventKind::default(), KeyEventKind::Press);
}

/// **The encoder treats a repeat as a press and never sends a release, on the
/// base layer.** `[[source: kitty-keyboard]]`: "Normally only key press events
/// are reported and key repeat events are treated as key press events." This is
/// the encoder itself, with a fresh terminal's state and no enhancement; the
/// core's `Term::key` tests reach it through the mode state they build.
#[test]
fn the_base_encoder_sends_a_repeat_as_a_press_and_no_release() {
    let s = KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::default(),
        other_keys: OtherKeys::default(),
        other_keys_format: OtherKeysFormat::default(),
        c1: C1Transmit::default(),
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::NONE,
    };
    let bytes = |e: KeyEvent| encode_key(e, s).map(|k| k.as_bytes().to_vec());
    for code in [
        KeyCode::Up,
        KeyCode::Backspace,
        KeyCode::Char(Codepoint::from_char('x')),
    ] {
        let press = KeyEvent::press(code, Modifiers::NONE);
        let pressed = bytes(press);
        assert!(pressed.is_some(), "{code:?} has a legacy encoding");
        let repeat = KeyEvent {
            kind: KeyEventKind::Repeat,
            ..press
        };
        assert_eq!(bytes(repeat), pressed, "{code:?}: a repeat is the press");
        let release = KeyEvent {
            kind: KeyEventKind::Release,
            ..press
        };
        assert_eq!(bytes(release), None, "{code:?}: no release");
    }
}
