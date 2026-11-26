//! baud's compiled xterm-256color entry, `OUT_DIR/terminfo_caps_data.rs`
//! and `xterm_256color.bin`, and the predefined capabilities in the order
//! a compiled entry stores them.
//!
//! **The order.** Each capability line of `[[source: ncurses-caps]]` is its
//! variable name, its capname and its type, `bool`, `num` or `str`, and a
//! type's lines in the file's order are its positions in a compiled entry's
//! boolean, number and string sections (`[[source: term-man5]]`: "The
//! capabilities are in the same order as the file <term.h>", which the
//! build of ncurses writes from this file). Lines starting `#` are
//! comments, and `capalias` and `infoalias` lines name termcap and terminfo
//! aliases, which hold no position.
//!
//! **The entry** is the pinned xterm-256color source
//! (`[[source: terminfo-xterm-256color]]`) with one capability changed:
//! [`SE`], the string that resets the cursor's style, is `\E[0 q`, which
//! returns the cursor to the style baud is configured with, where the
//! pinned entry's `\E[2 q` sets a steady block. Every other capability is
//! the pinned one's. It is compiled to `[[source: term-man5]]`'s format:
//! six little-endian short integers (the magic number 0432, the names'
//! size, the counts of booleans, numbers and string offsets, the string
//! table's size), the names ended by a NUL, one byte a boolean, a NUL when
//! that leaves the numbers on an odd byte, a short a number, a short a
//! string offset (-1 for one the entry does not have) and the string
//! table, each string ended by a NUL. Each predefined section stops after
//! the last capability the entry has. The capabilities `<term.h>` does not
//! name follow in the extended format: five shorts (the extended booleans,
//! numbers and strings, the offsets after them, the table's size), the
//! booleans, a NUL to an even byte, the numbers, the offsets of the
//! strings' values and then of every extended capability's name, booleans,
//! numbers and strings in that order, and the table holding the values
//! and then the names. The names' offsets count from the first name. Each
//! type's extended capabilities are in byte order of their names, and a
//! NUL keeps the extended header on an even byte. The page calls the
//! header's fourth short the table's size; it is the count of offsets
//! that follow, which is how ncurses reads it back.
//!
//! A string's escapes are terminfo(5)'s (`[[source: terminfo-man5]]`), and
//! a number is decimal, octal or hexadecimal, as C writes it, no larger
//! than the format's short. A cancelled capability (`name@`), a `use=` and
//! anything else the source could mean more than one way stops the build.
//!
//! A pure function of its inputs: `main` reads them and writes what
//! [`build`] returns.

use super::termcap_keys::{ENTRY, NAMES};
use baud_tablegen::{Check, Emit, Error, Errors, Input, Kind, PerLine, Pos, Source};

/// The pinned `include/Caps`, whose line 1 is a rule of `#`.
pub const CAPS: Input = Input {
    file: "ncurses-6.0-Caps.txt",
    key: "ncurses-caps",
    header: "##############################################################################",
};

/// The files, under `data/`, in the order [`build`] reads them.
pub const INPUTS: [Input; 2] = [CAPS, ENTRY];

/// Line 2: the copyright of the 6.0 release, which line 1 does not name.
const VERSION: &str = "# Copyright (c) 1998-2011,2015 Free Software Foundation, Inc.";

/// How many of each type the file defines: ncurses 6.0's `BOOLCOUNT`,
/// `NUMCOUNT` and `STRCOUNT`.
const COUNTS: [(&str, usize); 3] = [("bool", 44), ("num", 39), ("str", 414)];

/// Positions the file's order must give, as `<term.h>` numbers them:
/// `lpix` is the last SVr4 boolean and `OTbs` the first obsolete termcap
/// one, `colors` is number 13, and `meml`, `memu` and `box1` end the strings.
const PINNED: [(&str, usize, &str); 7] = [
    ("bool", 36, "lpix"),
    ("bool", 37, "OTbs"),
    ("num", 13, "colors"),
    ("str", 394, "OTi2"),
    ("str", 411, "meml"),
    ("str", 412, "memu"),
    ("str", 413, "box1"),
];

/// One capability baud's entry gives a value of its own: its name, the
/// value the pinned entry gives it, and baud's, both as the source
/// writes them.
pub struct Change {
    pub name: &'static str,
    pub pinned: &'static str,
    pub ours: &'static str,
}

/// `Se`, the cursor style's reset: DECSCUSR 0, the configured style.
pub const SE: Change = Change {
    name: "Se",
    pinned: "\\E[2 q",
    ours: "\\E[0 q",
};

/// The compiled format's magic number, octal 0432.
const MAGIC: i16 = 0o432;

/// One capability of the entry, its value decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Flag,
    Number(i16),
    Text(Vec<u8>),
}

/// The capability order and the compiled entry.
pub fn build(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Emit, Errors> {
    let mut errs = Errors::default();
    let caps = Source::load(&CAPS, read)?;
    let entry = Source::load(&ENTRY, read)?;
    let names = match order(&caps) {
        Ok(n) => Some(n),
        Err(e) => {
            errs.extend(e);
            None
        }
    };
    let fields = match parse(&entry) {
        Ok(f) => Some(f),
        Err(e) => {
            errs.extend(e);
            None
        }
    };
    let (Some(names), Some((title, mut fields))) = (names, fields) else {
        return Err(errs);
    };
    match fields.iter_mut().find(|(n, _)| n == SE.name) {
        Some((_, value)) if *value == Value::Text(unescape(SE.pinned).unwrap_or_default()) => {
            *value = Value::Text(unescape(SE.ours).unwrap_or_default());
        }
        found => errs.push(entry.error(
            Kind::Control,
            format!(
                "{} is {found:?}, {:?} expected before baud's {:?}",
                SE.name, SE.pinned, SE.ours
            ),
        )),
    }
    let bytes = errs.take(compile(&entry, &title, &fields, &names));
    let Some(bytes) = bytes else {
        return Err(errs);
    };
    if !errs.is_empty() {
        return Err(errs);
    }
    let mut out = Emit::new(
        "terminfo_caps_data",
        "baud-domain/build.rs (build/terminfo_caps.rs)",
        &[&caps, &entry],
    );
    out.bytes(
        "XTERM_256COLOR",
        &[
            "baud's xterm-256color entry in the compiled format: the pinned",
            "entry with `Se` set to `\\E[0 q`.",
        ],
        &bytes,
    );
    for ((ident, what), list) in [
        ("BOOLEANS", "boolean"),
        ("NUMBERS", "number"),
        ("STRINGS", "string"),
    ]
    .iter()
    .zip(&names)
    {
        out.array(
            ident,
            "&str",
            &[&format!(
                "Each predefined {what} capability's capname, at its position in a compiled entry."
            )],
            list,
            PerLine(8),
            |n| format!("{n:?}"),
        );
    }
    Ok(out)
}

/// The predefined capabilities' names, booleans, numbers and strings, in
/// [`CAPS`]'s order.
fn order(src: &Source) -> Result<[Vec<String>; 3], Errors> {
    let mut errs = Errors::default();
    errs.require(Check::from(src.line(2).starts_with(VERSION)), || {
        src.error(
            Kind::Header,
            format!("line 2 is {:?}, {VERSION:?} expected", src.line(2)),
        )
    });
    let mut names: [Vec<String>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (n, line) in src.text().lines().enumerate() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if matches!(fields.first(), Some(&("capalias" | "infoalias"))) {
            continue;
        }
        let Some(ty) = fields.get(2) else {
            errs.push(src.error(
                Kind::FieldCount,
                format!("line {}: fewer than three fields", n + 1),
            ));
            continue;
        };
        let Some(at) = COUNTS.iter().position(|(t, _)| t == ty) else {
            errs.push(src.error(
                Kind::UnknownValue,
                format!("line {}: type {ty:?} is not bool, num or str", n + 1),
            ));
            continue;
        };
        let name = fields[1];
        if names
            .iter()
            .any(|list| list.iter().any(|x| x.as_str() == name))
        {
            errs.push(src.error(
                Kind::Duplicate,
                format!("line {}: {name} is defined already", n + 1),
            ));
            continue;
        }
        names[at].push(name.to_string());
    }
    for ((ty, want), list) in COUNTS.iter().zip(&names) {
        errs.count(src.path(), ty, list.len(), *want);
    }
    for (ty, i, want) in PINNED {
        let at = COUNTS.iter().position(|(t, _)| *t == ty).expect("a type");
        let got = names[at].get(i).map(String::as_str);
        errs.require(Check::from(got == Some(want)), || {
            src.error(
                Kind::Control,
                format!("{ty} {i} is {got:?}, {want} expected"),
            )
        });
    }
    errs.or(names)
}

/// The entry's names line, its trailing `,` dropped, and its capabilities
/// in the source's order. The source is `infocmp -1`'s: a line of names,
/// then one capability a line, indented, each ended by a `,`.
fn parse(src: &Source) -> Result<(String, Vec<(String, Value)>), Errors> {
    let mut errs = Errors::default();
    let mut title = None;
    let mut fields: Vec<(String, Value)> = Vec::new();
    for (i, raw) in src.text().lines().enumerate() {
        let line = i + 1;
        let fail = |kind: Kind, message: String| {
            Error::new(kind, src.path(), message).at(Pos { line, col: 1 }, raw)
        };
        if raw.starts_with('#') || raw.trim().is_empty() {
            continue;
        }
        if title.is_none() {
            errs.require(Check::from(raw == NAMES), || {
                fail(
                    Kind::Control,
                    format!("the names are {raw:?}, {NAMES:?} expected"),
                )
            });
            title = Some(raw.trim_end_matches(',').to_string());
            continue;
        }
        let Some(field) = raw.strip_prefix('\t').and_then(|f| f.strip_suffix(',')) else {
            errs.push(fail(
                Kind::FieldCount,
                "not one capability ended by `,`".into(),
            ));
            continue;
        };
        let (name, value) = match field.find(['=', '#', '@']) {
            Some(k) if field.as_bytes()[k] == b'=' => match unescape(&field[k + 1..]) {
                Ok(v) => (&field[..k], Value::Text(v)),
                Err(off) => {
                    errs.push(fail(
                        Kind::Malformed,
                        format!(
                            "an escape terminfo(5) does not define, at byte {off} of the value"
                        ),
                    ));
                    continue;
                }
            },
            Some(k) if field.as_bytes()[k] == b'#' => match number(&field[k + 1..]) {
                Some(v) => (&field[..k], Value::Number(v)),
                None => {
                    errs.push(fail(
                        Kind::Malformed,
                        "a number that is not one short in C's notation".into(),
                    ));
                    continue;
                }
            },
            Some(_) => {
                errs.push(fail(Kind::UnknownValue, "a cancelled capability".into()));
                continue;
            }
            None => (field, Value::Flag),
        };
        if name.is_empty() || name == "use" || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
            errs.push(fail(
                Kind::UnknownValue,
                format!("{name:?} is no capability name"),
            ));
            continue;
        }
        if fields.iter().any(|(n, _)| n == name) {
            errs.push(fail(Kind::Duplicate, format!("{name} is given already")));
            continue;
        }
        fields.push((name.to_string(), value));
    }
    let Some(title) = title else {
        errs.push(src.error(Kind::FieldCount, "no names line"));
        return Err(errs);
    };
    errs.or((title, fields))
}

/// A number as C writes one, `0x` hexadecimal, a leading `0` octal, else
/// decimal; `None` past the format's short or for anything else.
fn number(text: &str) -> Option<i16> {
    let hex = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"));
    let (digits, radix) = if let Some(h) = hex {
        (h, 16)
    } else if text.len() > 1 && text.starts_with('0') {
        (&text[1..], 8)
    } else {
        (text, 10)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    i16::from_str_radix(digits, radix).ok()
}

/// A string capability's value, terminfo(5)'s escapes undone: `\E` and `\e`
/// are ESC, `^x` control-x and `^?` DEL; `\n \l \r \t \b \f \s` are newline,
/// line feed, return, tab, backspace, form feed and space; `\^ \\ \, \:` are
/// the byte itself; a backslash before three octal digits is that byte; and
/// `\0` is `\200`, "which does not terminate a string". A compiled string
/// holds no NUL, so three octal digits naming zero are `\200` too.
/// Padding, `$<..>`, and the parameter language, `%`, are kept as written.
/// `Err` holds the byte offset of an escape the page does not define.
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
                    b'n' | b'l' => Some(0x0a),
                    b'r' => Some(0x0d),
                    b't' => Some(0x09),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0c),
                    b's' => Some(0x20),
                    b'^' | b'\\' | b',' | b':' => Some(n),
                    _ => None,
                };
                let octal = b
                    .get(i + 1..i + 4)
                    .filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)) && d[0] <= b'3');
                if let Some(c) = simple {
                    out.push(c);
                    i += 2;
                } else if let Some(d) = octal {
                    let byte = d.iter().fold(0u8, |acc, c| acc * 8 + (c - b'0'));
                    out.push(if byte == 0 { 0x80 } else { byte });
                    i += 4;
                } else if n == b'0' {
                    out.push(0x80);
                    i += 2;
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

/// `values` as little-endian shorts.
fn shorts(out: &mut Vec<u8>, values: &[i16]) {
    for v in values {
        out.extend_from_slice(&v.to_le_bytes());
    }
}

/// A NUL when `len` is odd, so what follows starts on an even byte.
fn even(out: &mut Vec<u8>, len: usize) {
    if len % 2 == 1 {
        out.push(0);
    }
}

/// Each of `strings`' offset in a table of them laid end to end, each
/// ended by a NUL, -1 for one absent; and the table.
fn table<'a>(strings: impl Iterator<Item = Option<&'a [u8]>>) -> (Vec<i16>, Vec<u8>) {
    let mut offsets = Vec::new();
    let mut text = Vec::new();
    for s in strings {
        match s {
            Some(s) => {
                offsets.push(i16::try_from(text.len()).unwrap_or(i16::MAX));
                text.extend_from_slice(s);
                text.push(0);
            }
            None => offsets.push(-1),
        }
    }
    (offsets, text)
}

/// A count or size as the header's short.
fn short(src: &Source, what: &str, n: usize) -> Result<i16, Error> {
    i16::try_from(n).map_err(|_| {
        src.error(
            Kind::Invariant,
            format!("{what} is {n}, past the format's short"),
        )
    })
}

/// The entry `title` and `fields` in the compiled format, `names` the
/// predefined capabilities' order.
fn compile(
    src: &Source,
    title: &str,
    fields: &[(String, Value)],
    names: &[Vec<String>; 3],
) -> Result<Vec<u8>, Error> {
    let value = |n: &str| fields.iter().find(|(f, _)| f == n).map(|(_, v)| v);
    let flags: Vec<bool> = names[0]
        .iter()
        .map(|n| value(n) == Some(&Value::Flag))
        .collect();
    let numbers: Vec<i16> = names[1]
        .iter()
        .map(|n| match value(n) {
            Some(Value::Number(v)) => *v,
            _ => -1,
        })
        .collect();
    let strings: Vec<Option<&[u8]>> = names[2]
        .iter()
        .map(|n| match value(n) {
            Some(Value::Text(v)) => Some(v.as_slice()),
            _ => None,
        })
        .collect();
    for (name, v) in fields {
        let fits = matches!(
            (names.iter().position(|list| list.contains(name)), v),
            (None, _)
                | (Some(0), Value::Flag)
                | (Some(1), Value::Number(_))
                | (Some(2), Value::Text(_))
        );
        if !fits {
            return Err(src.error(
                Kind::UnknownValue,
                format!("{name} is given as another type than <term.h>'s"),
            ));
        }
    }
    let flags = &flags[..flags.iter().rposition(|f| *f).map_or(0, |i| i + 1)];
    let numbers = &numbers[..numbers.iter().rposition(|v| *v != -1).map_or(0, |i| i + 1)];
    let strings = &strings[..strings
        .iter()
        .rposition(Option::is_some)
        .map_or(0, |i| i + 1)];
    let (offsets, text) = table(strings.iter().copied());
    let mut title = title.as_bytes().to_vec();
    title.push(0);
    let mut out = Vec::new();
    shorts(
        &mut out,
        &[
            MAGIC,
            short(src, "the names' size", title.len())?,
            short(src, "the booleans", flags.len())?,
            short(src, "the numbers", numbers.len())?,
            short(src, "the strings", strings.len())?,
            short(src, "the string table", text.len())?,
        ],
    );
    out.extend_from_slice(&title);
    out.extend(flags.iter().map(|f| u8::from(*f)));
    even(&mut out, title.len() + flags.len());
    shorts(&mut out, numbers);
    shorts(&mut out, &offsets);
    out.extend_from_slice(&text);
    even(&mut out, text.len());
    let extended = |want: fn(&Value) -> bool| {
        let mut list: Vec<&(String, Value)> = fields
            .iter()
            .filter(|(n, v)| want(v) && !names.iter().any(|l| l.contains(n)))
            .collect();
        list.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        list
    };
    let ext_flags = extended(|v| matches!(v, Value::Flag));
    let ext_numbers = extended(|v| matches!(v, Value::Number(_)));
    let ext_strings = extended(|v| matches!(v, Value::Text(_)));
    let (value_offsets, value_text) = table(ext_strings.iter().map(|(_, v)| match v {
        Value::Text(t) => Some(t.as_slice()),
        _ => None,
    }));
    let (name_offsets, name_text) = table(
        ext_flags
            .iter()
            .chain(&ext_numbers)
            .chain(&ext_strings)
            .map(|(n, _)| Some(n.as_bytes())),
    );
    shorts(
        &mut out,
        &[
            short(src, "the extended booleans", ext_flags.len())?,
            short(src, "the extended numbers", ext_numbers.len())?,
            short(src, "the extended strings", ext_strings.len())?,
            short(
                src,
                "the extended offsets",
                value_offsets.len() + name_offsets.len(),
            )?,
            short(
                src,
                "the extended table",
                value_text.len() + name_text.len(),
            )?,
        ],
    );
    out.extend(ext_flags.iter().map(|_| 1));
    even(&mut out, ext_flags.len());
    let ext_values: Vec<i16> = ext_numbers
        .iter()
        .map(|(_, v)| match v {
            Value::Number(n) => *n,
            _ => -1,
        })
        .collect();
    shorts(&mut out, &ext_values);
    shorts(&mut out, &value_offsets);
    shorts(&mut out, &name_offsets);
    out.extend_from_slice(&value_text);
    out.extend_from_slice(&name_text);
    Ok(out)
}
