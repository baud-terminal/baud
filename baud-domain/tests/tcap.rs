//! xterm's XTGETTCAP table and the answer built from it
//! (`baud_domain::encode_tcap_reply`).

use baud_domain::*;
use std::collections::BTreeSet;

/// The generated table is what the generator writes from the pinned source.
#[test]
fn the_tcap_table_is_the_pinned_source() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-tcap-table.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "tcap_data.rs is stale -- rerun scripts/gen-tcap-table.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// **The termcap keyboard's table is the pinned entry's**, joined to
/// xterm's table: `termcap_data.rs` is exactly what
/// `scripts/gen-termcap-keys.py` writes from `terminfo/xterm-256color.ti`
/// and `xterm-xtermcap.c`.
#[test]
fn the_termcap_table_is_the_pinned_entry() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-termcap-keys.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "termcap_data.rs is stale -- rerun scripts/gen-termcap-keys.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The names of xterm's table, read from the pinned source here rather
/// than by the generator: every quoted name of a `DATA` row, and the seven
/// names each `DGRP` group spells (xtermcap.c 189-243).
fn xterm_names() -> BTreeSet<String> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/xterm-xtermcap.c"
    ))
    .expect("the pinned xtermcap.c");
    let start = src
        .find("static const TCAPINFO table[] = {")
        .expect("the table");
    let end = start + src[start..].find("#undef DATA").expect("its end");
    let mut names = BTreeSet::new();
    let mut base = "k";
    for line in src[start..end].lines() {
        let l = line.trim();
        if l.starts_with("#define BASE \"kp\"") {
            base = "kp";
        }
        if let Some(rest) = l.strip_prefix("DATA(") {
            for name in rest.split('"').skip(1).step_by(2).take(2) {
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        } else if let Some(rest) = l.strip_prefix("DGRP(") {
            let g = rest.split(',').next().expect("a group name").trim();
            names.insert(format!("{base}{g}"));
            for p in 3..=8 {
                names.insert(format!("{base}{g}{p}"));
            }
        }
    }
    names
}

/// **Every one of xterm's 319 names is found, and nothing else is.** The
/// count is the pinned xtermcap.c table's, `DATA` rows and `DGRP` groups
/// read as `xterm_names` reads them.
#[test]
fn every_name_in_xterms_table_is_found_and_no_other() {
    let names = xterm_names();
    assert_eq!(names.len(), 319);
    for n in &names {
        assert!(tcap_lookup(n.as_bytes()).is_some(), "{n}");
    }
    for n in [
        "cup", "Ms", "Tc", "RGBX", "KCUU1", "kcuu", "", "kpZRO88", "kf64",
    ] {
        assert_eq!(tcap_lookup(n.as_bytes()), None, "{n}");
    }
    // Two names, one row: `colors` and its termcap code, and `Co` is the
    // first row that names it, not `RGB`'s.
    assert_eq!(tcap_lookup(b"Co"), Some(TcapValue::Colors));
    assert_eq!(tcap_lookup(b"RGB"), Some(TcapValue::Rgb));
    assert_eq!(
        tcap_lookup(b"kcbt"),
        Some(TcapValue::Key(KeyCode::Tab, Modifiers::SHIFT))
    );
    assert_eq!(tcap_lookup(b"kf36"), Some(TcapValue::Absent));
    assert_eq!(tcap_lookup(b"kund"), Some(TcapValue::Absent));
}

fn state() -> KeyState {
    KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::Numeric,
        keys: KeyResources::default(),
        c1: C1Transmit::SevenBit,
        terminal: TerminalMode::Ansi,
        encoding: InputEncoding::Utf8,
        kitty: KittyFlags::NONE,
        backarrow: BackarrowKey::Delete,
        delete: DeleteKey::Remove,
        num_lock: NumLockKeypad::Overrides,
        level: ConformanceLevel::Four,
        keyboard: Keyboard::Default,
    }
}

fn hex(bytes: &[u8]) -> Vec<u8> {
    bytes
        .iter()
        .flat_map(|b| format!("{b:02X}").into_bytes())
        .collect()
}

/// **The bound holds for the list that answers most**: as many of the
/// shortest name as a list holds, each answering the longest name `TN`
/// can be. And the answer is whole or nothing: one byte short of room,
/// none of it.
#[test]
fn the_longest_answer_is_within_its_bound_and_whole_or_nothing() {
    let name = TermName::new(&[b'x'; TERM_NAME_MAX]).expect("a name");
    let one = hex(b"TN");
    let mut list = one.clone();
    while list.len() + 1 + one.len() <= OSC_LIST {
        list.push(b';');
        list.extend_from_slice(&one);
    }
    let mut out = vec![0u8; tcap_reply_max(OSC_LIST)];
    let n = encode_tcap_reply(&list, C1Transmit::SevenBit, state(), name, &mut out)
        .expect("the bound holds it")
        .index();
    assert_eq!(list.len(), 509);
    assert!(n <= tcap_reply_max(list.len()));
    assert!(out[..n].starts_with(b"\x1bP1+r544E="));
    let mut short = vec![0u8; n - 1];
    assert!(encode_tcap_reply(&list, C1Transmit::SevenBit, state(), name, &mut short).is_none());
    let mut exact = vec![0u8; n];
    assert!(encode_tcap_reply(&list, C1Transmit::SevenBit, state(), name, &mut exact).is_some());
}

/// A list the core's string buffer holds after its `q`.
const OSC_LIST: usize = 511;
