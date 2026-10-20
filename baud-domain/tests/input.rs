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
        keys: KeyResources::default(),
        c1: C1Transmit::default(),
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::NONE,
        backarrow: BackarrowKey::default(),
        delete: DeleteKey::default(),
        num_lock: NumLockKeypad::default(),
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

/// Every `#define <prefix>Name 0x...` in a pinned keysym header.
fn keysym_defines(file: &str, prefix: &str) -> std::collections::HashMap<String, u32> {
    let src = std::fs::read_to_string(format!(
        "{}/../docs/research/sources/{file}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("the pinned header");
    let mut out = std::collections::HashMap::new();
    for line in src.lines() {
        let mut w = line.split_whitespace();
        if let (Some("#define"), Some(name), Some(v)) = (w.next(), w.next(), w.next()) {
            if let (Some(n), Some(hex)) = (name.strip_prefix(prefix), v.strip_prefix("0x")) {
                if let Ok(v) = u32::from_str_radix(hex, 16) {
                    out.insert(n.to_string(), v);
                }
            }
        }
    }
    out
}

/// **Every key's keysym is the header's**: what xterm classes and numbers
/// when a key-modifier resource is at 4 is the keysym, so each row of
/// `KeyCode::keysym` is read against `[[source: xorg-keysymdef]]` by name,
/// and the media keys against `[[source: xkbcommon-keysyms]]`'s `XF86`
/// names as kitty's keyboard layer maps them.
#[test]
fn keysyms_are_the_headers() {
    let xk = keysym_defines("xorg-keysymdef.h", "XK_");
    let xf = keysym_defines("xkbcommon-keysyms.h", "XKB_KEY_XF86");
    assert!(xk.len() > 1000 && xf.len() > 100, "the headers parsed");
    let none = Modifiers::NONE;
    let mut rows: Vec<(KeyCode, Modifiers, &str)> = vec![
        (KeyCode::Up, none, "Up"),
        (KeyCode::Down, none, "Down"),
        (KeyCode::Right, none, "Right"),
        (KeyCode::Left, none, "Left"),
        (KeyCode::Home, none, "Home"),
        (KeyCode::End, none, "End"),
        (KeyCode::Insert, none, "Insert"),
        (KeyCode::Delete, none, "Delete"),
        (KeyCode::PageUp, none, "Prior"),
        (KeyCode::PageDown, none, "Next"),
        (KeyCode::Enter, none, "Return"),
        (KeyCode::Tab, none, "Tab"),
        (KeyCode::Tab, Modifiers::SHIFT, "ISO_Left_Tab"),
        (KeyCode::Backspace, none, "BackSpace"),
        (KeyCode::Escape, none, "Escape"),
    ];
    let fnames: Vec<String> = (1..=20).map(|n| format!("F{n}")).collect();
    for (n, name) in (1..=20u8).zip(&fnames) {
        rows.push((KeyCode::Function(FnKey::new(n).expect("F")), none, name));
    }
    use KeypadKey as K;
    for (k, name) in [
        (K::Space, "KP_Space"),
        (K::Tab, "KP_Tab"),
        (K::Enter, "KP_Enter"),
        (K::Pf1, "KP_F1"),
        (K::Pf2, "KP_F2"),
        (K::Pf3, "KP_F3"),
        (K::Pf4, "KP_F4"),
        (K::Home, "KP_Home"),
        (K::Left, "KP_Left"),
        (K::Up, "KP_Up"),
        (K::Right, "KP_Right"),
        (K::Down, "KP_Down"),
        (K::PageUp, "KP_Prior"),
        (K::PageDown, "KP_Next"),
        (K::End, "KP_End"),
        (K::Begin, "KP_Begin"),
        (K::Insert, "KP_Insert"),
        (K::Delete, "KP_Delete"),
        (K::Multiply, "KP_Multiply"),
        (K::Add, "KP_Add"),
        (K::Comma, "KP_Separator"),
        (K::Minus, "KP_Subtract"),
        (K::Period, "KP_Decimal"),
        (K::Divide, "KP_Divide"),
        (K::Digit0, "KP_0"),
        (K::Digit1, "KP_1"),
        (K::Digit2, "KP_2"),
        (K::Digit3, "KP_3"),
        (K::Digit4, "KP_4"),
        (K::Digit5, "KP_5"),
        (K::Digit6, "KP_6"),
        (K::Digit7, "KP_7"),
        (K::Digit8, "KP_8"),
        (K::Digit9, "KP_9"),
        (K::Equal, "KP_Equal"),
    ] {
        rows.push((KeyCode::Keypad(k), none, name));
    }
    use PuaKey as P;
    let pua: [(P, &str); 41] = [
        (P::CapsLock, "Caps_Lock"),
        (P::ScrollLock, "Scroll_Lock"),
        (P::NumLock, "Num_Lock"),
        (P::PrintScreen, "Print"),
        (P::Pause, "Pause"),
        (P::Menu, "Menu"),
        (P::F21, "F21"),
        (P::F22, "F22"),
        (P::F23, "F23"),
        (P::F24, "F24"),
        (P::F25, "F25"),
        (P::F26, "F26"),
        (P::F27, "F27"),
        (P::F28, "F28"),
        (P::F29, "F29"),
        (P::F30, "F30"),
        (P::F31, "F31"),
        (P::F32, "F32"),
        (P::F33, "F33"),
        (P::F34, "F34"),
        (P::F35, "F35"),
        (P::LeftShift, "Shift_L"),
        (P::LeftControl, "Control_L"),
        (P::LeftAlt, "Alt_L"),
        (P::LeftSuper, "Super_L"),
        (P::LeftHyper, "Hyper_L"),
        (P::LeftMeta, "Meta_L"),
        (P::RightShift, "Shift_R"),
        (P::RightControl, "Control_R"),
        (P::RightAlt, "Alt_R"),
        (P::RightSuper, "Super_R"),
        (P::RightHyper, "Hyper_R"),
        (P::RightMeta, "Meta_R"),
        (P::IsoLevel3Shift, "ISO_Level3_Shift"),
        (P::IsoLevel5Shift, "ISO_Level5_Shift"),
        (P::MediaPlay, "XF86AudioPlay"),
        (P::MediaPause, "XF86AudioPause"),
        (P::MediaStop, "XF86AudioStop"),
        (P::MediaFastForward, "XF86AudioForward"),
        (P::MediaRewind, "XF86AudioRewind"),
        (P::MediaTrackNext, "XF86AudioNext"),
    ];
    for (k, name) in pua {
        rows.push((KeyCode::Pua(k), none, name));
    }
    for (k, name) in [
        (P::MediaTrackPrevious, "XF86AudioPrev"),
        (P::MediaRecord, "XF86AudioRecord"),
        (P::LowerVolume, "XF86AudioLowerVolume"),
        (P::RaiseVolume, "XF86AudioRaiseVolume"),
        (P::MuteVolume, "XF86AudioMute"),
    ] {
        rows.push((KeyCode::Pua(k), none, name));
    }
    for (code, mods, name) in &rows {
        let want = match name.strip_prefix("XF86") {
            Some(rest) => xf.get(rest),
            None => xk.get(*name),
        };
        assert_eq!(code.keysym(*mods), want.copied(), "{code:?} is {name}");
    }
    // Two kitty keys no keysym maps, and a text key, which has none here.
    for code in [
        KeyCode::Pua(P::MediaPlayPause),
        KeyCode::Pua(P::MediaReverse),
        KeyCode::Char(Codepoint::from_char('a')),
    ] {
        assert_eq!(code.keysym(none), None, "{code:?}");
    }
}
