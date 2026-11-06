//! The termcap keyboard's table, `?1050`, `OUT_DIR/termcap_data.rs`.
//!
//! Under `?1050` a key sends the string the terminal description gives
//! it. xterm's `xtermcapString` (`[[source: xterm-xtermcap-c]]` 461-511)
//! finds the key's rows in its table (70-244) by keysym and modifier
//! parameter, in the table's order, and sends the string of the first one
//! the description has. The description is the pinned xterm-256color entry
//! (`[[source: terminfo-xterm-256color]]`), the `TERM` baud gives the
//! child, so the table is compiled from the two: for each key and
//! parameter, the first row whose terminfo name the entry defines.
//!
//! Rows no key reaches are left out: the keypad's Home, End, Prior and
//! Next, which xterm moves onto the main keys before it reads the table
//! (`[[source: xterm-input-c]]` 1185-1191); `kcbt`, whose row has no
//! modifier while a back tab carries Shift; Help, Find, Select, Clear and
//! Undo, and F36 up, which baud has no key for. No row names Begin, so the
//! entry's `kb2` is not in the table either: a key no row reaches keeps the
//! default keyboard's encoding, as in xterm.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use super::xtermcap::{self, XTERMCAP};
use baud_tablegen::{Emit, Error, Errors, Input, Kind, Pos, Source};
use std::collections::HashMap;
use std::fmt::Write as _;

/// The entry as `infocmp -x -1` wrote it: line 1 names the database file,
/// which any entry from that directory names, so line 2, the entry's names,
/// is the control.
pub const ENTRY: Input = Input {
    file: "terminfo-xterm-256color.ti",
    key: "terminfo-xterm-256color",
    header: "#\tReconstructed via infocmp from file: /usr/share/terminfo/78/xterm-256color",
};

/// [`ENTRY`]'s line 2.
pub const NAMES: &str = "xterm-256color|xterm with 256 colors,";

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [XTERMCAP, ENTRY];

/// How many keys and parameters the table gives a string.
const KEYS: usize = 107;

/// A keysym the encoder has a key for, as the `KeyCode` it is.
const NAMED: [(&str, &str); 18] = [
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
    ("XK_KP_Enter", "KeyCode::Keypad(KeypadKey::Enter)"),
    ("XK_KP_Add", "KeyCode::Keypad(KeypadKey::Add)"),
    ("XK_KP_Separator", "KeyCode::Keypad(KeypadKey::Comma)"),
    ("XK_KP_Divide", "KeyCode::Keypad(KeypadKey::Divide)"),
    ("XK_KP_Decimal", "KeyCode::Keypad(KeypadKey::Period)"),
    ("XK_KP_Multiply", "KeyCode::Keypad(KeypadKey::Multiply)"),
    ("XK_KP_Subtract", "KeyCode::Keypad(KeypadKey::Minus)"),
];

/// The rows no key reaches, and the three that answer a query rather than
/// name a key.
const UNREACHED: [&str; 13] = [
    "XK_KP_Home",
    "XK_KP_End",
    "XK_KP_Prior",
    "XK_KP_Next",
    "XK_ISO_Left_Tab",
    "XK_Help",
    "XK_Find",
    "XK_Select",
    "XK_Clear",
    "XK_Undo",
    "XK_COLORS",
    "XK_RGB",
    "XK_TCAPNAME",
];

/// A row's key: a function key's number, F1 to F20, or a `KeyCode`.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Function(u8),
    Code(String),
}

/// The row's key, `None` for a row no key reaches, `Err` for a keysym
/// this table does not know.
fn key(code: &str) -> Result<Option<Key>, ()> {
    if let Some(n) = xtermcap::function(code) {
        return Ok(match n {
            0..=20 => Some(Key::Function(n)),
            21..=35 => Some(Key::Code(format!("KeyCode::Pua(PuaKey::F{n})"))),
            _ => None,
        });
    }
    if UNREACHED.contains(&code) {
        return Ok(None);
    }
    if let Some((_, k)) = NAMED.iter().find(|(x, _)| *x == code) {
        return Ok(Some(Key::Code((*k).to_string())));
    }
    if let Some(d) = code.strip_prefix("XK_KP_") {
        if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() {
            return Ok(Some(Key::Code(format!(
                "KeyCode::Keypad(KeypadKey::Digit{d})"
            ))));
        }
    }
    Err(())
}

/// A string capability's value, terminfo(5)'s escapes undone: the ones a
/// key's string can carry, `\E` `\e` `\n` `\r` `\t` `\b` `\f` `\s` `\^` `\\`
/// `\,` `\:`, three octal digits, and `^X` for a control character, `^?`
/// for DEL. `Err` holds the byte offset of an escape it does not define.
fn unescape(v: &str) -> Result<Vec<u8>, usize> {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                let n = *b.get(i + 1).ok_or(i)?;
                let simple = match n {
                    b'E' | b'e' => Some(0x1b),
                    b'n' => Some(0x0a),
                    b'r' => Some(0x0d),
                    b't' => Some(0x09),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0c),
                    b's' => Some(0x20),
                    b'^' | b'\\' | b',' | b':' => Some(n),
                    _ => None,
                };
                if let Some(c) = simple {
                    out.push(c);
                    i += 2;
                } else if n.is_ascii_digit() {
                    let oct = v.get(i + 1..i + 4).ok_or(i)?;
                    out.push(u8::from_str_radix(oct, 8).map_err(|_| i)?);
                    i += 4;
                } else {
                    return Err(i);
                }
            }
            b'^' => {
                let n = *b.get(i + 1).ok_or(i)?;
                out.push(if n == b'?' { 0x7f } else { n & 0x1f });
                i += 2;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// A string as a Rust byte literal: printable ASCII as itself, but for `"`
/// and `\`, and every other byte `\xNN`.
fn lit(b: &[u8]) -> String {
    let mut s = String::from("b\"");
    for &c in b {
        if (0x20..0x7f).contains(&c) && c != b'"' && c != b'\\' {
            s.push(char::from(c));
        } else {
            let _ = write!(s, "\\x{c:02x}");
        }
    }
    s.push('"');
    s
}

/// One capability of the entry: its line and its value as written.
struct Cap<'a> {
    line: usize,
    raw: &'a str,
    value: &'a str,
}

/// The entry's string capabilities, `name=value,` one a line.
fn caps(src: &Source) -> Result<HashMap<&str, Cap<'_>>, Errors> {
    let mut errs = Errors::default();
    let mut out: HashMap<&str, Cap<'_>> = HashMap::new();
    for (i, raw) in src.text().lines().enumerate() {
        let line = i + 1;
        if !raw.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((name, rest)) = raw.trim_start().split_once('=') else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
            continue;
        }
        let Some(value) = rest.strip_suffix(',') else {
            continue;
        };
        let cap = Cap { line, raw, value };
        if let Some(first) = out.insert(name, cap) {
            errs.push(
                Error::new(
                    Kind::Duplicate,
                    src.path(),
                    format!("{name} is given already on line {}", first.line),
                )
                .at(Pos { line, col: 2 }, raw),
            );
        }
    }
    errs.or(out)
}

/// What a key and parameter send: the row's terminfo name, for the
/// comment, and the string.
type Sent<'a> = (&'a str, Vec<u8>);

pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let table = Source::load(&XTERMCAP, read)?;
    let entry = Source::load(&ENTRY, read)?;
    let mut errs = Errors::default();
    if entry.line(2) != NAMES {
        errs.push(
            entry
                .error(
                    Kind::Header,
                    format!(
                        "line 2 is not {NAMES:?}: is this the pinned entry [{}]?",
                        ENTRY.key
                    ),
                )
                .at(Pos { line: 2, col: 1 }, entry.line(2)),
        );
    }
    let rows = xtermcap::rows(&table);
    let caps = caps(&entry);
    let (rows, caps) = match (rows, caps) {
        (Ok(r), Ok(c)) => (r, c),
        (r, c) => {
            for e in [r.err(), c.err()].into_iter().flatten() {
                errs.extend(e);
            }
            return Err(errs);
        }
    };
    match caps.get("kcuu1").map(|c| c.value) {
        Some("\\EOA") => {}
        got => errs.push(entry.error(
            Kind::Control,
            format!("kcuu1 is {got:?}, \\EOA expected: the entry was not read"),
        )),
    }
    let mut keyed: Vec<((String, u8), Sent<'_>)> = Vec::new();
    let mut fns: Vec<((u8, u8), Sent<'_>)> = Vec::new();
    for row in &rows {
        let k = match key(row.code) {
            Ok(Some(k)) => k,
            Ok(None) => continue,
            Err(()) => {
                errs.push(row.error(
                    table.path(),
                    Kind::UnknownValue,
                    format!("no key for {}", row.code),
                ));
                continue;
            }
        };
        let Some(cap) = caps.get(row.ti.as_str()) else {
            continue;
        };
        let value = match unescape(cap.value) {
            Ok(v) => v,
            Err(at) => {
                let col = cap.raw.len() - cap.raw.trim_start().len() + row.ti.len() + 2 + at;
                errs.push(
                    Error::new(
                        Kind::Malformed,
                        entry.path(),
                        format!("an escape terminfo(5) does not define in {}", row.ti),
                    )
                    .at(
                        Pos {
                            line: cap.line,
                            col,
                        },
                        cap.raw,
                    ),
                );
                continue;
            }
        };
        let ti = row.ti.as_str();
        match k {
            Key::Function(n) => {
                if !fns.iter().any(|(at, _)| *at == (n, row.param)) {
                    fns.push(((n, row.param), (ti, value)));
                }
            }
            Key::Code(c) => {
                let at = (c, row.param);
                if !keyed.iter().any(|(a, _)| *a == at) {
                    keyed.push((at, (ti, value)));
                }
            }
        }
    }
    fns.sort_by_key(|(at, _)| *at);
    let kf13 = fns.iter().find(|(at, _)| *at == (13, 0)).map(|(_, v)| &v.1);
    if kf13.map(Vec::as_slice) != Some(b"\x1b[1;2P".as_slice()) {
        errs.push(table.error(
            Kind::Control,
            format!("kf13 is {kf13:?}, \\E[1;2P expected: the join went wrong"),
        ));
    }
    errs.count(
        entry.path(),
        "keys and parameters given a string",
        keyed.len() + fns.len(),
        KEYS,
    );
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut rs = String::from(
        "use crate::{KeyCode, KeypadKey, PuaKey};\n\
         \n\
         /// The string the entry gives `code` pressed with modifier parameter\n\
         /// `param` (0 for none, else xterm's `1 + mask`), or `None` for a key\n\
         /// and parameter it gives none.\n\
         pub(super) fn string(code: KeyCode, param: u8) -> Option<&'static [u8]> {\n    \
         Some(match (code, param) {\n",
    );
    for ((k, param), (ti, value)) in &keyed {
        let _ = writeln!(rs, "        // {ti}");
        let _ = writeln!(rs, "        ({k}, {param}) => {},", lit(value));
    }
    rs.push_str(
        "        (KeyCode::Function(f), p) => return function(f.number(), p),\n        \
         _ => return None,\n    \
         })\n\
         }\n\
         \n\
         /// The string for function key `n`, F1 to F20, with parameter `param`.\n\
         fn function(n: u8, param: u8) -> Option<&'static [u8]> {\n    \
         Some(match (n, param) {\n",
    );
    for ((n, param), (ti, value)) in &fns {
        let _ = writeln!(rs, "        // {ti}");
        let _ = writeln!(rs, "        ({n}, {param}) => {},", lit(value));
    }
    rs.push_str("        _ => return None,\n    })\n}\n");
    let mut out = Emit::new(
        "termcap_data",
        "baud-domain/build.rs (build/termcap_keys.rs)",
        &[&table, &entry],
    );
    out.code(&rs);
    Ok(out)
}
