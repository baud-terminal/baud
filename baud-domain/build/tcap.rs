//! xterm's XTGETTCAP table, `OUT_DIR/tcap_data.rs`: each of xterm's 319
//! names, terminfo and termcap, at the first row that names it, as
//! `lookupTcapByName` finds it (`[[source: xterm-xtermcap-c]]` 347-374),
//! with the key baud's encoder takes for that row; and the multiplier
//! `src/vt/tcap.rs` indexes the names by.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use super::py_random::PyRandom;
use super::xtermcap::{self, XTERMCAP};
use baud_tablegen::{Check, Emit, Errors, Input, Kind, Source};
use std::collections::HashSet;
use std::fmt::Write as _;

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 1] = [XTERMCAP];

/// How many distinct names the table holds.
const NAMES: usize = 319;

/// A name's length, in bytes: termcap's codes are two, and seven leaves
/// [`pack`] its length byte.
const NAME_LEN: std::ops::RangeInclusive<usize> = 2..=7;

/// A keysym the encoder has a key for, as the `KeyCode` it is.
const NAMED: [(&str, &str); 11] = [
    ("XK_Home", "KeyCode::Home"),
    ("XK_End", "KeyCode::End"),
    ("XK_Left", "KeyCode::Left"),
    ("XK_Right", "KeyCode::Right"),
    ("XK_Up", "KeyCode::Up"),
    ("XK_Down", "KeyCode::Down"),
    ("XK_Delete", "KeyCode::Delete"),
    ("XK_Insert", "KeyCode::Insert"),
    ("XK_Next", "KeyCode::PageDown"),
    ("XK_Prior", "KeyCode::PageUp"),
    ("XK_BackSpace", "KeyCode::Backspace"),
];

/// A keypad keysym, as the `KeypadKey` it is; `XK_KP_0` to `XK_KP_9` are
/// `Digit0` to `Digit9`.
const PAD: [(&str, &str); 11] = [
    ("XK_KP_Enter", "Enter"),
    ("XK_KP_Home", "Home"),
    ("XK_KP_End", "End"),
    ("XK_KP_Prior", "PageUp"),
    ("XK_KP_Next", "PageDown"),
    ("XK_KP_Add", "Add"),
    ("XK_KP_Separator", "Comma"),
    ("XK_KP_Divide", "Divide"),
    ("XK_KP_Decimal", "Period"),
    ("XK_KP_Multiply", "Multiply"),
    ("XK_KP_Subtract", "Minus"),
];

/// The keysyms baud has no key for: they send nothing, so they answer
/// empty.
const ABSENT: [&str; 5] = ["XK_Help", "XK_Find", "XK_Select", "XK_Clear", "XK_Undo"];

/// What a row answers, as `tcap.rs` spells it.
fn value(code: &str, param: u8) -> Option<String> {
    Some(match code {
        "XK_COLORS" => "TcapValue::Colors".into(),
        "XK_RGB" => "TcapValue::Rgb".into(),
        "XK_TCAPNAME" => "TcapValue::Name".into(),
        // The row's state is 0 and its keysym the shifted Tab; baud's key
        // is Tab with Shift (`[[source: xterm-input-c]]` 1146-1147,
        // "Shift-tab is often mapped to XK_ISO_Left_Tab").
        "XK_ISO_Left_Tab" => "tc_key(KeyCode::Tab, 2)".into(),
        c if ABSENT.contains(&c) => "TcapValue::Absent".into(),
        c => {
            if let Some(n) = xtermcap::function(c) {
                return Some(match n {
                    0..=20 => format!("tc_fn({n}, {param})"),
                    21..=35 => format!("tc_key(KeyCode::Pua(PuaKey::F{n}), {param})"),
                    _ => "TcapValue::Absent".into(),
                });
            }
            if let Some((_, k)) = NAMED.iter().find(|(x, _)| *x == c) {
                return Some(format!("tc_key({k}, {param})"));
            }
            if let Some((_, k)) = PAD.iter().find(|(x, _)| *x == c) {
                return Some(format!("tc_pad(KeypadKey::{k}, {param})"));
            }
            let d = c.strip_prefix("XK_KP_")?;
            if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() {
                return Some(format!("tc_pad(KeypadKey::Digit{d}, {param})"));
            }
            return None;
        }
    })
}

/// A name as one word, its length above its bytes, as `tcap.rs` packs it.
pub fn pack(name: &[u8]) -> u64 {
    name.iter()
        .fold(name.len() as u64, |v, &c| (v << 8) | u64::from(c))
}

/// The index's slots, `tcap.rs`'s `SLOTS`.
pub const SLOTS: usize = 1024;

/// The longest walk past its bucket any of `names` takes, open-addressed
/// over [`SLOTS`] in `names`' order, the bucket from the high ten bits of
/// `pack(name) * mul`.
pub fn walk(names: &[&[u8]], mul: u64) -> usize {
    let mut taken = vec![false; SLOTS];
    let mut worst = 0;
    for n in names {
        let mut i = (pack(n).wrapping_mul(mul) >> (64 - SLOTS.trailing_zeros())) as usize;
        let mut k = 0;
        while taken[i] {
            i = (i + 1) % SLOTS;
            k += 1;
        }
        taken[i] = true;
        worst = worst.max(k);
    }
    worst
}

/// How many odd multipliers the search tries, and from what seed.
pub const TRIES: usize = 3000;
pub const SEED: u32 = 1;

/// **The multiplier, searched**: the first of [`TRIES`] odd multipliers,
/// [`PyRandom`] seeded with [`SEED`], under which no name walks more than
/// one slot past its bucket; `tcap.rs` asserts that bound at compile time.
pub fn multiplier(names: &[&[u8]]) -> Option<u64> {
    let mut rng = PyRandom::new(SEED);
    (0..TRIES)
        .map(|_| rng.bits64() | 1)
        .find(|&m| walk(names, m) <= 1)
}

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let src = Source::load(&XTERMCAP, read)?;
    let rows = xtermcap::rows(&src)?;
    let path = src.path();
    let mut errs = Errors::default();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut names: Vec<(&str, String)> = Vec::new();
    for row in &rows {
        let Some(v) = value(row.code, row.param) else {
            errs.push(row.error(path, Kind::UnknownValue, format!("no key for {}", row.code)));
            continue;
        };
        for name in [row.ti.as_str(), row.tc] {
            if name.is_empty() || !seen.insert(name) {
                continue;
            }
            errs.require(Check::from(NAME_LEN.contains(&name.len())), || {
                row.error(
                    path,
                    Kind::Invariant,
                    format!("{name:?} is outside {NAME_LEN:?} bytes"),
                )
            });
            names.push((name, v.clone()));
        }
    }
    errs.count(path, "distinct names", names.len(), NAMES);
    if !errs.is_empty() {
        return Err(errs);
    }
    let keys: Vec<&[u8]> = names.iter().map(|(n, _)| n.as_bytes()).collect();
    let Some(mul) = multiplier(&keys) else {
        return Err(src
            .error(
                Kind::Invariant,
                format!("no multiplier of {TRIES} keeps every name within one slot"),
            )
            .into());
    };
    let mut rs = String::from(
        "use super::tcap::{TcapValue, tc_fn, tc_key, tc_pad};\n\
         use crate::{KeyCode, KeypadKey, PuaKey};\n\
         \n\
         /// The index's multiplier: the generator's search, which keeps every\n\
         /// name within one slot of its bucket.\n",
    );
    let _ = writeln!(rs, "pub(crate) const MUL: u64 = {mul:#018x};");
    let _ = writeln!(
        rs,
        "\npub(crate) const ROWS: [(&[u8], TcapValue); {}] = [",
        names.len()
    );
    for (name, v) in &names {
        let lit = name.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = writeln!(rs, "    (b\"{lit}\", {v}),");
    }
    rs.push_str("];\n");
    let mut out = Emit::new("tcap_data", "baud-domain/build.rs (build/tcap.rs)", &[&src]);
    out.code(&rs);
    Ok(out)
}
