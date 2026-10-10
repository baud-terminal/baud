//! For every key name in xterm's XTGETTCAP table (xtermcap.c 70-244) that
//! baud's KeyCode can name, encode it with baud's real encoder in two states
//! and compare with the pinned xterm-256color entry and the -x capture.
//!
//! Then, for each entry value that differs from the key xterm's table names,
//! encode the key ncurses' convention names (shift-F1..F8 for kf13-kf20,
//! Ctrl-H for ^H): which key, if any, baud sends that value for.
//!
//! Run from the repository root, as keyprobe-Cargo.toml says. The two
//! entries are read at the paths they are committed at, relative to the
//! directory the program runs in, unless two paths are given as arguments:
//! the pinned entry, then the -x capture.
use baud_domain::*;
use std::collections::BTreeMap;

fn parse(path: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for l in std::fs::read_to_string(path).unwrap().lines() {
        if !l.starts_with('\t') {
            continue;
        }
        let t = l.trim().trim_end_matches(',');
        if let Some((k, v)) = t.split_once('=') {
            m.insert(k.to_string(), v.to_string());
        }
    }
    m
}
fn decode(v: &str) -> Vec<u8> {
    let b = v.as_bytes();
    let mut o = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'^' => {
                o.push(if b[i + 1] == b'?' {
                    0x7f
                } else {
                    b[i + 1] & 0x1f
                });
                i += 2;
            }
            b'\\' => {
                o.push(match b[i + 1] {
                    b'E' | b'e' => 0x1b,
                    b's' => b' ',
                    c => c,
                });
                i += 2;
            }
            c => {
                o.push(c);
                i += 1;
            }
        }
    }
    o
}
fn show(b: &[u8]) -> String {
    b.iter()
        .map(|&c| match c {
            0x1b => "\\E".into(),
            0x7f => "^?".into(),
            c if c < 32 => format!("^{}", (c + 64) as char),
            c => (c as char).to_string(),
        })
        .collect()
}
fn mods_of(p: u8) -> Modifiers {
    // xterm's modifier parameter: 1 + bitmask (shift 1, alt 2, ctrl 4)
    let m = p - 1;
    let mut r = Modifiers::NONE;
    if m & 1 != 0 {
        r = r.with(Modifiers::SHIFT);
    }
    if m & 2 != 0 {
        r = r.with(Modifiers::ALT);
    }
    if m & 4 != 0 {
        r = r.with(Modifiers::CTRL);
    }
    r
}
fn main() {
    let mut args = std::env::args().skip(1);
    let pin = parse(
        &args
            .next()
            .unwrap_or_else(|| "docs/research/sources/terminfo/xterm-256color.ti".into()),
    );
    let cap = parse(
        &args
            .next()
            .unwrap_or_else(|| "docs/research/sources/terminfo/xterm-256color-x.ti".into()),
    );
    let base: KeyState = KeyState {
        modes: Modes::default(),
        keypad: KeypadMode::default(),
        other_keys: OtherKeys::default(),
        other_keys_format: OtherKeysFormat::default(),
        c1: C1Transmit::default(),
        terminal: TerminalMode::default(),
        encoding: InputEncoding::default(),
        kitty: KittyFlags::NONE,
    };
    let mut smkx = base;
    smkx.modes.set(ModeBit::CursorKeys);
    smkx.keypad = KeypadMode::Application;
    let s = Modifiers::SHIFT;
    let n = Modifiers::NONE;
    let mut rows: Vec<(String, KeyCode, Modifiers)> = vec![
        ("khome".into(), KeyCode::Home, n),
        ("kHOM".into(), KeyCode::Home, s),
        ("kend".into(), KeyCode::End, n),
        ("kEND".into(), KeyCode::End, s),
        ("kcub1".into(), KeyCode::Left, n),
        ("kcuf1".into(), KeyCode::Right, n),
        ("kcuu1".into(), KeyCode::Up, n),
        ("kcud1".into(), KeyCode::Down, n),
        ("kLFT".into(), KeyCode::Left, s),
        ("kRIT".into(), KeyCode::Right, s),
        ("kind".into(), KeyCode::Down, s),
        ("kri".into(), KeyCode::Up, s),
        ("kcbt".into(), KeyCode::Tab, s),
        ("kdch1".into(), KeyCode::Delete, n),
        ("kich1".into(), KeyCode::Insert, n),
        ("knp".into(), KeyCode::PageDown, n),
        ("kpp".into(), KeyCode::PageUp, n),
        ("kNXT".into(), KeyCode::PageDown, s),
        ("kPRV".into(), KeyCode::PageUp, s),
        ("kbs".into(), KeyCode::Backspace, n),
        ("kent".into(), KeyCode::Keypad(KeypadKey::Enter), n),
        ("kUP".into(), KeyCode::Up, s),
        ("kDN".into(), KeyCode::Down, s),
        ("kDC".into(), KeyCode::Delete, s),
        ("kIC".into(), KeyCode::Insert, s),
    ];
    for f in 1..=20u8 {
        rows.push((
            format!("kf{f}"),
            KeyCode::Function(FnKey::new(f).unwrap()),
            n,
        ));
    }
    let groups = [
        ("DN", KeyCode::Down),
        ("LFT", KeyCode::Left),
        ("RIT", KeyCode::Right),
        ("UP", KeyCode::Up),
        ("DC", KeyCode::Delete),
        ("END", KeyCode::End),
        ("HOM", KeyCode::Home),
        ("IC", KeyCode::Insert),
        ("NXT", KeyCode::PageDown),
        ("PRV", KeyCode::PageUp),
    ];
    for (g, k) in groups {
        for p in 3..=8u8 {
            rows.push((format!("k{g}{p}"), k, mods_of(p)));
        }
    }
    let (mut agree, mut differ, mut absent) = (0, 0, 0);
    let mut longest = 0usize;
    for (name, code, mods) in &rows {
        let e = KeyEvent::press(*code, *mods);
        let b0 = encode_key(e, base)
            .map(|k| k.as_bytes().to_vec())
            .unwrap_or_default();
        let b1 = encode_key(e, smkx)
            .map(|k| k.as_bytes().to_vec())
            .unwrap_or_default();
        longest = longest.max(b0.len()).max(b1.len());
        let entry = pin.get(name).or_else(|| cap.get(name));
        let src = if pin.contains_key(name) {
            "pin"
        } else if cap.contains_key(name) {
            "-x"
        } else {
            "--"
        };
        let verdict = match entry {
            None => {
                absent += 1;
                "not in either entry".to_string()
            }
            Some(v) => {
                let d = decode(v);
                if d == b1 {
                    agree += 1;
                    "agrees (smkx)".into()
                } else {
                    differ += 1;
                    format!("DIFFERS: entry {}", show(&d))
                }
            }
        };
        println!(
            "{name:7} {src:3} reset {:12} smkx {:12} {verdict}",
            show(&b0),
            show(&b1)
        );
    }
    println!(
        "rows {} agree {agree} differ {differ} absent {absent}; comparable {}; longest encoded {longest} B",
        rows.len(),
        agree + differ
    );

    // The differing values, encoded from the key ncurses' convention names.
    println!(
        "-- the differing entry values: which key baud sends each for (smkx state; reset is the same for these)"
    );
    let mut conv: Vec<(String, KeyCode, Modifiers, &str)> = Vec::new();
    for f in 1..=8u8 {
        conv.push((
            format!("kf{}", f + 12),
            KeyCode::Function(FnKey::new(f).unwrap()),
            s,
            "shift-F",
        ));
    }
    conv.push((
        "kbs".into(),
        KeyCode::Char(Codepoint::from_char('h')),
        Modifiers::CTRL,
        "ctrl-h",
    ));
    conv.push(("kbs".into(), KeyCode::Backspace, n, "Backspace"));
    let mut same = 0;
    for (name, code, mods, what) in &conv {
        let e = KeyEvent::press(*code, *mods);
        let b0 = encode_key(e, base)
            .map(|k| k.as_bytes().to_vec())
            .unwrap_or_default();
        let b1 = encode_key(e, smkx)
            .map(|k| k.as_bytes().to_vec())
            .unwrap_or_default();
        let d = decode(pin.get(name).unwrap());
        let label = if *what == "shift-F" {
            format!("shift-F{}", name[2..].parse::<u8>().unwrap() - 12)
        } else {
            what.to_string()
        };
        let v = if d == b1 && d == b0 {
            same += 1;
            "same bytes as the entry"
        } else {
            "differs from the entry"
        };
        println!(
            "{name:7} entry {:10} {label:10} reset {:10} smkx {:10} {v}",
            show(&d),
            show(&b0),
            show(&b1)
        );
    }
    println!(
        "convention keys probed {}, sending the entry's bytes {same}",
        conv.len()
    );
}
