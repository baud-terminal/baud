//! xterm's XTGETTCAP table, `[[source: xterm-xtermcap-c]]` lines 70-244,
//! which `build/tcap.rs` and `build/termcap_keys.rs` both compile: the
//! rows of `static const TCAPINFO table[]` in source order, from its `{` to
//! `#undef DATA`. A row is a `DATA(tc, ti, code, state)` or one of the
//! seven a `DGRP(name, code)` group spells, `BASE #name` with state 2 and
//! `BASE #name #parm` with 3 to 8 (lines 189-198), `BASE` being whichever
//! `#define BASE` is in force. Every conditional row is read, as compiled
//! with `USE_EXTENDED_NAMES`, `XK_ISO_Left_Tab`, `OPT_ISO_COLORS` and
//! `OPT_DIRECT_COLOR`. A line that starts `DATA(` or `DGRP(` and is not
//! read whole is refused; the table's other lines are its preprocessor
//! conditions and its comments.

use baud_tablegen::{Error, Errors, Input, Kind, Pos, Source};

pub const XTERMCAP: Input = Input {
    file: "xterm-xtermcap.c",
    key: "xterm-xtermcap-c",
    header: "/* $XTermId: xtermcap.c,v 1.70 2026/04/07 23:05:27 tom Exp $ */",
};

/// The table's `DATA` rows and `DGRP` groups (`[[source: xterm-xtermcap-c]]`
/// 72-243).
pub const DATA_ROWS: usize = 110;
pub const GROUPS: usize = 17;

const START: &str = "static const TCAPINFO table[] = {";
const END: &str = "#undef DATA";

/// One row: its line, its termcap code (empty for a group's), its
/// terminfo name, its keysym expression as written, and its modifier
/// parameter, 0 for none or xterm's `1 + mask`: `SHIFT` is `MOD_NONE +
/// MOD_SHIFT`, 2 (line 60).
#[derive(Clone, Debug)]
pub struct Row<'a> {
    pub line: usize,
    pub raw: &'a str,
    pub tc: &'a str,
    pub ti: String,
    pub code: &'a str,
    pub param: u8,
}

impl Row<'_> {
    /// A failure at this row, at the column its text starts.
    pub fn error(&self, path: &str, kind: Kind, message: impl Into<String>) -> Error {
        at(path, self.line, self.raw, kind, message)
    }
}

fn at(path: &str, line: usize, raw: &str, kind: Kind, message: impl Into<String>) -> Error {
    let col = raw.chars().take_while(|c| c.is_whitespace()).count() + 1;
    Error::new(kind, path, message).at(Pos { line, col }, raw)
}

/// `"..."` and what follows it.
fn quoted(s: &str) -> Option<(&str, &str)> {
    let r = s.strip_prefix('"')?;
    let end = r.find('"')?;
    Some((&r[..end], &r[end + 1..]))
}

/// A C identifier's characters, at least one, and what follows them.
fn word(s: &str) -> Option<(&str, &str)> {
    let n = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .count();
    (n > 0).then(|| s.split_at(n))
}

/// `"tc", "ti", code, state)`: the fields of a `DATA(` line.
fn data(s: &str) -> Option<(&str, &str, &str, &str)> {
    let (tc, r) = quoted(s.trim_start())?;
    let (ti, r) = quoted(r.strip_prefix(',')?.trim_start())?;
    let r = r.strip_prefix(',')?;
    let comma = r.find(',')?;
    let code = r[..comma].trim();
    let (state, r) = word(r[comma + 1..].trim_start())?;
    r.trim_start().strip_prefix(')')?;
    (!code.is_empty()).then_some((tc, ti, code, state))
}

/// `name, code)`: the fields of a `DGRP(` line.
fn group(s: &str) -> Option<(&str, &str)> {
    let (name, r) = word(s)?;
    let (code, r) = word(r.strip_prefix(',')?.trim_start())?;
    r.strip_prefix(')')?;
    Some((name, code))
}

/// The table's rows in source order.
pub fn rows(src: &Source) -> Result<Vec<Row<'_>>, Errors> {
    let path = src.path();
    let text = src.text();
    let Some(start) = text.find(START) else {
        return Err(src
            .error(
                Kind::Control,
                format!("no {START:?}: the table was not found"),
            )
            .into());
    };
    let Some(len) = text[start..].find(END) else {
        return Err(src
            .error(Kind::Control, format!("no {END:?} after the table"))
            .into());
    };
    // The line `START` is on, counted from 1.
    let first = text[..start].matches('\n').count() + 1;
    let mut errs = Errors::default();
    let mut out = Vec::new();
    let mut base: Option<&str> = None;
    let (mut datas, mut groups) = (0, 0);
    for (i, raw) in text[start..start + len].lines().enumerate() {
        let line = first + i;
        let s = raw.trim();
        if let Some(r) = s.strip_prefix("#define BASE") {
            match quoted(r.trim()) {
                Some((b, "")) => base = Some(b),
                _ => errs.push(at(path, line, raw, Kind::Malformed, "a BASE not read")),
            }
        } else if s.starts_with("#undef BASE") {
            base = None;
        } else if let Some(r) = s.strip_prefix("DATA(") {
            let Some((tc, ti, code, state)) = data(r) else {
                errs.push(at(path, line, raw, Kind::Malformed, "a DATA row not read"));
                continue;
            };
            let param = match state {
                "0" => 0,
                "SHIFT" => 2,
                _ => {
                    errs.push(at(
                        path,
                        line,
                        raw,
                        Kind::UnknownValue,
                        format!("state {state}, neither 0 nor SHIFT"),
                    ));
                    continue;
                }
            };
            datas += 1;
            out.push(Row {
                line,
                raw,
                tc,
                ti: ti.to_string(),
                code,
                param,
            });
        } else if let Some(r) = s.strip_prefix("DGRP(") {
            let Some((name, code)) = group(r) else {
                errs.push(at(
                    path,
                    line,
                    raw,
                    Kind::Malformed,
                    "a DGRP group not read",
                ));
                continue;
            };
            let Some(b) = base else {
                errs.push(at(path, line, raw, Kind::Malformed, "a DGRP with no BASE"));
                continue;
            };
            groups += 1;
            out.push(Row {
                line,
                raw,
                tc: "",
                ti: format!("{b}{name}"),
                code,
                param: 2,
            });
            for p in 3..=8u8 {
                out.push(Row {
                    line,
                    raw,
                    tc: "",
                    ti: format!("{b}{name}{p}"),
                    code,
                    param: p,
                });
            }
        }
    }
    errs.count(path, "DATA rows", datas, DATA_ROWS);
    errs.count(path, "DGRP groups", groups, GROUPS);
    errs.or(out)
}

/// `XK_Fn(n)` or `-n`: function key `n`. A negative code is the function
/// key of that number, which has no keysym: `xtermcapKeycode` reads it as
/// `XK_Fn((-code))` (`[[source: xterm-xtermcap-c]]` 412-414), and the
/// table gives F36 to F63 that way (131-158).
pub fn function(code: &str) -> Option<u8> {
    let n = match code.strip_prefix('-') {
        Some(n) => n,
        None => code.strip_prefix("XK_Fn(")?.strip_suffix(')')?,
    };
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}
