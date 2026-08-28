//! The base-layer key encoder, checked against `[[source: ctlseqs]]`.
//!
//! `docs/input-coverage.md` names three layers and this is the one every
//! terminal implements. An application that has not negotiated an enhancement
//! must receive what any other terminal would send, which is the same reasoning
//! that makes `DA` under-claim as a VT102.
//!
//! The expectations are read out of the pinned specification rather than
//! written here, for the reason `scripts/extract-keys.py` exists: a key table
//! transcribed by hand is a guessed table that looks like it worked.

use baud_core::*;

fn ctlseqs() -> String {
    std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-ctlseqs.txt"
    ))
    .expect("the pinned ctlseqs")
}

/// The `Key | Normal | Application` table at the given line, as
/// `(name, normal, application)`.
fn table_at(src: &str, line: usize) -> Vec<(String, String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        if t.starts_with('-') {
            continue;
        }
        let Some((name, rest)) = t.split_once('|') else {
            break;
        };
        let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
        if cells.len() < 2 {
            break;
        }
        out.push((
            name.trim().to_string(),
            cells[0].to_string(),
            cells[1].to_string(),
        ));
    }
    out
}

fn press(code: KeyCode, mods: Modifiers, app: bool) -> Vec<u8> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    if app {
        // DECCKM set: cursor keys go application.
        let b = b"\x1b[?1h";
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(b));
        t.feed(base, f.len());
        t.commit_read(f.len());
        t.clear_reply();
    }
    assert!(t.key(KeyEvent::press(code, mods)), "the key was encoded");
    t.reply().to_vec()
}

/// Turn the specification's spelling -- `CSI A`, `SS3 A` -- into bytes.
fn spec_bytes(form: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for tok in form.split_whitespace() {
        match tok {
            "CSI" => out.extend_from_slice(b"\x1b["),
            "SS3" => out.extend_from_slice(b"\x1bO"),
            other => out.extend_from_slice(other.as_bytes()),
        }
    }
    out
}

/// **Every cursor key matches the specification's table, in both modes.**
///
/// Six keys across two tables -- the four arrows at line 2473 and Home/End at
/// 2485 -- read from the source and compared against what baud sends.
#[test]
fn cursor_keys_match_the_specifications_table() {
    let src = ctlseqs();
    let rows: Vec<(String, String, String)> = table_at(&src, 2473)
        .into_iter()
        .chain(table_at(&src, 2485))
        .collect();
    assert_eq!(
        rows.len(),
        6,
        "expected four arrows and Home/End from ctlseqs; got {rows:?}. If the \
         source was re-pinned the line numbers moved, and an empty table would \
         pass every assertion below by having nothing to check."
    );

    for (name, normal, app) in rows {
        let code = match name.as_str() {
            "Cursor Up" => KeyCode::Up,
            "Cursor Down" => KeyCode::Down,
            "Cursor Right" => KeyCode::Right,
            "Cursor Left" => KeyCode::Left,
            "Home" => KeyCode::Home,
            "End" => KeyCode::End,
            other => panic!("unknown key {other:?} in the table"),
        };
        assert_eq!(
            press(code, Modifiers::NONE, false),
            spec_bytes(&normal),
            "{name} in normal mode should be {normal}"
        );
        assert_eq!(
            press(code, Modifiers::NONE, true),
            spec_bytes(&app),
            "{name} in application mode should be {app}"
        );
    }
}

/// **A modifier is appended as `1 ; mod`, and forces `CSI`.**
///
/// `SS3` takes no parameters, so there is nowhere to put the modifier. The
/// specification states this for F1-F4 at line 2564 -- "the SS3 sent before F1
/// through F4 is altered to CSI when sending a function key modifier" -- and
/// the same holds for the cursor keys. An encoder that kept `SS3` in
/// application mode would emit something no application parses.
#[test]
fn a_modified_cursor_key_uses_csi_even_in_application_mode() {
    assert_eq!(
        press(KeyCode::Up, Modifiers::SHIFT, false),
        b"\x1b[1;2A".to_vec(),
        "shift-Up is CSI 1 ; 2 A"
    );
    assert_eq!(
        press(KeyCode::Up, Modifiers::SHIFT, true),
        b"\x1b[1;2A".to_vec(),
        "and application mode does not change that -- SS3 has no parameters"
    );
    assert_eq!(
        press(KeyCode::Left, Modifiers::CTRL.with(Modifiers::SHIFT), false),
        b"\x1b[1;6D".to_vec(),
        "ctlseqs code 6 is Shift + Control"
    );
}

/// Releases and repeats are not sent on this layer.
///
/// Only `[[source: kitty-keyboard]]` reports them, and only once an
/// application has asked. A terminal that volunteered them would be sending
/// bytes nothing requested.
#[test]
fn only_presses_are_encoded() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        let e = KeyEvent {
            code: KeyCode::Up,
            mods: Modifiers::NONE,
            kind,
        };
        assert!(!t.key(e), "{kind:?} must not be encoded");
        assert!(t.reply().is_empty(), "and must write nothing");
    }
}

/// What is not implemented yet says so by returning false, rather than
/// sending something wrong.
#[test]
fn unimplemented_keys_send_nothing() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let e = KeyEvent::press(KeyCode::Tab, Modifiers::NONE);
    assert!(!t.key(e), "the text keys are not encoded yet");
    assert!(t.reply().is_empty());
}

/// The `Key | Escape Sequence` table at the given line.
fn seq_table_at(src: &str, line: usize) -> Vec<(String, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for l in lines.iter().skip(line) {
        let t = l.trim();
        if t.starts_with('-') {
            continue;
        }
        let Some((name, rest)) = t.split_once('|') else {
            break;
        };
        out.push((name.trim().to_string(), rest.trim().to_string()));
    }
    out
}

/// **Every function key matches the specification, gaps included.**
///
/// F5-F12 are 15, 17, 18, 19, 20, 21, 23, 24 and F13-F20 are 25, 26, 28, 29,
/// 31, 32, 33, 34. Sixteen and twenty-two are skipped, and so are twenty-seven
/// and thirty. A table written as "15 upward" is wrong from F6 onward and
/// sends a *neighbouring key* rather than nothing — the failure mode that is
/// hardest to notice, because something plausible arrives.
#[test]
fn function_keys_match_the_specifications_tables() {
    let src = ctlseqs();
    let rows: Vec<(String, String)> = seq_table_at(&src, 2529)
        .into_iter()
        .chain(seq_table_at(&src, 2685))
        .collect();
    assert_eq!(
        rows.len(),
        20,
        "expected F1-F12 and F13-F20 from ctlseqs; got {} rows. An empty table \
         would pass every comparison below by having nothing to compare.",
        rows.len()
    );

    for (name, form) in rows {
        let n: u8 = name.trim_start_matches('F').parse().expect("Fn");
        let key = KeyCode::Function(FnKey::new(n).expect("F1..F20"));
        assert_eq!(
            press(key, Modifiers::NONE, false),
            spec_bytes(&form),
            "F{n} should be {form}"
        );
    }
}

/// The editing keys, and that DECCKM does not touch them.
///
/// `[[source: ctlseqs]]` line 2672 gives the same sequence in the Normal and
/// Application columns, unlike the cursor keys two tables earlier.
#[test]
fn editing_keys_are_the_same_in_both_modes() {
    let src = ctlseqs();
    let rows = table_at(&src, 2672);
    assert_eq!(rows.len(), 6, "Insert, Delete, Home, End, PageUp, PageDown");

    for (name, normal, app) in rows {
        assert_eq!(
            normal, app,
            "{name}: DECCKM does not change the editing keys"
        );
        // Home and End are sent PC-style, from the table at 2485; the VT220
        // forms here are the `sunKeyboard` alternative, not an addition.
        let code = match name.as_str() {
            "Insert" => KeyCode::Insert,
            "Delete" => KeyCode::Delete,
            "PageUp" => KeyCode::PageUp,
            "PageDown" => KeyCode::PageDown,
            "Home" | "End" => continue,
            other => panic!("unknown editing key {other:?}"),
        };
        for app_mode in [false, true] {
            assert_eq!(
                press(code, Modifiers::NONE, app_mode),
                spec_bytes(&normal),
                "{name} should be {normal} in both modes"
            );
        }
    }
}

/// A modifier goes before the `~`, not after it.
///
/// `CSI 1 5 ; 2 ~` is shift-F5, which is the worked example the specification
/// gives at line 2586 for the modifier encoding.
#[test]
fn shift_f5_is_the_specifications_worked_example() {
    let src = ctlseqs();
    assert!(
        src.contains("shift-F5 would be sent as CSI 1 5 ; 2 ~"),
        "the worked example moved; this test quotes a source it no longer matches"
    );
    let f5 = KeyCode::Function(FnKey::new(5).unwrap());
    assert_eq!(press(f5, Modifiers::SHIFT, false), b"\x1b[15;2~".to_vec());

    // And F1-F4 take the cursor keys' shape instead.
    let f1 = KeyCode::Function(FnKey::new(1).unwrap());
    assert_eq!(press(f1, Modifiers::NONE, false), b"\x1bOP".to_vec());
    assert_eq!(press(f1, Modifiers::CTRL, false), b"\x1b[1;5P".to_vec());
}
