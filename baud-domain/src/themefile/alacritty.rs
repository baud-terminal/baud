//! alacritty's theme files: its TOML config (0.13 and later), and the YAML
//! one before it, each read for its `colors` onto the same [`Scheme`].
//!
//! **The keys** are alacritty's (`[[source: alacritty-0c405d5-man-5]]`
//! 297-475; `[[source: alacritty-config-color-0c405d5]]` 9-24, 115-121,
//! 171-245): `colors.primary` `background` and `foreground` are the page
//! and the text, `colors.normal` and `colors.bright` `black` to `white` are
//! 0-7 and 8-15, `colors.cursor` and `colors.selection` take `text` (or its
//! name `foreground`) and `cursor`/`background`, and `colors.indexed_colors`
//! holds `{ index, color }` for 16-255.
//!
//! **A colour** is alacritty's `Rgb::from_str`
//! (`[[source: alacritty-display-color-0c405d5]]` 276-299): a string of
//! `#` and six hex digits or `0x` and six, read by `u32::from_str_radix`,
//! which takes a leading `+`; or the struct form `{ r, g, b }` (223-269).
//! `CellForeground` and `CellBackground` name the cell's own colours, which
//! a scheme cannot hold; one leaves the colour baud's own and is said.
//!
//! alacritty reports a field it cannot read and keeps its default, so a bad
//! value here is skipped and said, and the rest read; a file that is not
//! TOML (or YAML) at all is refused, as alacritty refuses it.

use super::terminals::Skipped;
use super::{Format, Line, Scheme, SchemeError, SchemeErrorKind, rgb};
use crate::Color;

/// A key path, at most six deep: `colors.indexed_colors.color` and the
/// element of an array of tables it is in.
#[derive(Clone, Copy)]
struct Path<'a> {
    seg: [&'a [u8]; 6],
    len: usize,
    /// Which element of an array (`indexed_colors`) the path is in; 0 for
    /// none, so each element counts from 1.
    elem: u32,
}

impl<'a> Path<'a> {
    const ROOT: Path<'a> = Path {
        seg: [b""; 6],
        len: 0,
        elem: 0,
    };

    fn push(mut self, s: &'a [u8]) -> Option<Self> {
        *self.seg.get_mut(self.len)? = s;
        self.len += 1;
        Some(self)
    }

    fn segs(&self) -> &[&'a [u8]] {
        &self.seg[..self.len]
    }
}

/// A value, as far as a colour needs it.
#[derive(Clone, Copy)]
enum Value<'a> {
    /// A string's contents; `escaped` holds a backslash escape, which no
    /// colour has.
    Str(&'a [u8], Escapes),
    Int(i64),
    /// `{ r, g, b }`, the struct form, once all three are in.
    Struct([Option<i64>; 3]),
    /// A boolean, a float, a date, an array of scalars.
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Escapes {
    None,
    Some,
}

/// alacritty's `Rgb::from_str`: `#rrggbb` or `0xrrggbb`.
fn rgb_from_str(s: &[u8]) -> Option<Color> {
    let chars = match s {
        [b'0', b'x', rest @ ..] if s.len() == 8 => rest,
        [b'#', rest @ ..] if s.len() == 7 => rest,
        _ => return None,
    };
    let digits = chars.strip_prefix(b"+").unwrap_or(chars);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let n = digits
        .iter()
        .fold(0u32, |n, &d| n * 16 + (d as char).to_digit(16).unwrap_or(0));
    let [_, r, g, b] = n.to_be_bytes();
    Some(rgb(r, g, b))
}

/// A colour, or a cell-relative name, or neither.
enum Cell {
    Colour(Color),
    Relative,
    Bad,
}

fn cell_rgb(v: Value<'_>, cell_relative: CellNames) -> Cell {
    match v {
        Value::Str(s, Escapes::None) => {
            if cell_relative == CellNames::Allowed
                && matches!(s, b"CellForeground" | b"CellBackground")
            {
                return Cell::Relative;
            }
            rgb_from_str(s).map_or(Cell::Bad, Cell::Colour)
        }
        Value::Struct([Some(r), Some(g), Some(b)]) => {
            match (u8::try_from(r), u8::try_from(g), u8::try_from(b)) {
                (Ok(r), Ok(g), Ok(b)) => Cell::Colour(rgb(r, g, b)),
                _ => Cell::Bad,
            }
        }
        _ => Cell::Bad,
    }
}

/// Whether a field is alacritty's `CellRgb`, which takes the cell's own
/// colours by name, or a plain `Rgb`, which does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CellNames {
    Allowed,
    Refused,
}

/// The eight names `normal` and `bright` take, in ANSI order.
const NAMES: [&[u8]; 8] = [
    b"black", b"red", b"green", b"yellow", b"blue", b"magenta", b"cyan", b"white",
];

/// What the readers lay their key paths onto.
struct Builder<'a, F> {
    s: Scheme,
    ignored: F,
    /// The `indexed_colors` element being read: its number, line, index
    /// and colour, laid down when the next element starts.
    pending: (u32, Line, Option<i64>, Option<Color>, &'a [u8]),
}

impl<'a, F: FnMut(Line, &[u8], Skipped)> Builder<'a, F> {
    fn new(format: Format, ignored: F) -> Self {
        Builder {
            s: Scheme::empty(format),
            ignored,
            pending: (0, Line(0), None, None, b""),
        }
    }

    /// The pending element, laid down: `index` 16-255, as alacritty's
    /// `ColorIndex` refuses one below 16.
    fn flush(&mut self) {
        let (elem, line, index, colour, key) = self.pending;
        self.pending = (0, Line(0), None, None, b"");
        if elem == 0 {
            return;
        }
        match (index.and_then(|i| u8::try_from(i).ok()), colour) {
            (Some(i), Some(c)) if i >= 16 => self.s.extended[usize::from(i - 16)] = Some(c),
            _ => (self.ignored)(line, key, Skipped::Refused),
        }
    }

    fn set(&mut self, line: Line, key: &'a [u8], path: Path<'a>, v: Value<'a>) {
        if path.elem != 0 && path.elem != self.pending.0 {
            self.flush();
        }
        let segs = path.segs();
        let [b"colors", rest @ ..] = segs else {
            (self.ignored)(line, key, Skipped::NotAColour);
            return;
        };
        let slot = match rest {
            [b"primary", b"background"] => (&mut self.s.page, CellNames::Refused),
            [b"primary", b"foreground"] => (&mut self.s.text, CellNames::Refused),
            [b"normal" | b"bright", name] if NAMES.contains(name) => {
                let i = NAMES.iter().position(|n| n == name).unwrap_or(0);
                let i = if rest[0] == b"bright" { i + 8 } else { i };
                (&mut self.s.ansi[i], CellNames::Refused)
            }
            [b"cursor", b"text" | b"foreground"] => (&mut self.s.cursor_text, CellNames::Allowed),
            [b"cursor", b"cursor" | b"background"] => (&mut self.s.cursor, CellNames::Allowed),
            [b"selection", b"text" | b"foreground"] => {
                (&mut self.s.selection_text, CellNames::Allowed)
            }
            [b"selection", b"cursor" | b"background"] => {
                (&mut self.s.selection, CellNames::Allowed)
            }
            [b"indexed_colors", field] if path.elem != 0 => {
                if self.pending.0 == 0 {
                    self.pending = (path.elem, line, None, None, key);
                }
                match (*field, v) {
                    (b"index", Value::Int(i)) => self.pending.2 = Some(i),
                    (b"color", v) => match cell_rgb(v, CellNames::Refused) {
                        Cell::Colour(c) => self.pending.3 = Some(c),
                        _ => self.pending.3 = None,
                    },
                    _ => {}
                }
                return;
            }
            [
                b"primary" | b"dim" | b"vi_mode_cursor" | b"search" | b"hints" | b"line_indicator"
                | b"footer_bar",
                ..,
            ] => {
                (self.ignored)(line, key, Skipped::Unheld);
                return;
            }
            _ => {
                (self.ignored)(line, key, Skipped::NotAColour);
                return;
            }
        };
        let (slot, names) = slot;
        match cell_rgb(v, names) {
            Cell::Colour(c) => *slot = Some(c),
            Cell::Relative => {
                *slot = None;
                (self.ignored)(line, key, Skipped::CellRelative);
            }
            Cell::Bad => (self.ignored)(line, key, Skipped::Refused),
        }
    }

    fn finish(mut self) -> Scheme {
        self.flush();
        self.s
    }
}

/// A TOML subset: what an alacritty config uses, from TOML 1.0.0
/// (`[[source: toml-1-0-0]]`): `#` comments; `[table]` and `[[array of
/// tables]]` headers; bare, quoted and dotted keys; basic, literal and
/// multi-line strings; inline tables, on one line; arrays, across lines;
/// integers in the four bases; and the other scalars read as tokens. Anything
/// else is refused at its line, as alacritty refuses a config TOML cannot
/// parse.
struct Toml<'a> {
    t: &'a [u8],
    at: usize,
    line: u32,
    elems: u32,
}

type Step<T> = Result<T, Line>;

impl<'a> Toml<'a> {
    fn peek(&self) -> Option<u8> {
        self.t.get(self.at).copied()
    }

    fn bump(&mut self) {
        if self.peek() == Some(b'\n') {
            self.line += 1;
        }
        self.at += 1;
    }

    fn here(&self) -> Line {
        Line(self.line)
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.peek() == Some(b) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Spaces and tabs.
    fn ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.bump();
        }
    }

    /// Spaces, tabs, comments and newlines.
    fn ws_nl(&mut self) {
        loop {
            self.ws();
            match self.peek() {
                Some(b'#') => self.comment(),
                Some(b'\n') => self.bump(),
                Some(b'\r') if self.t.get(self.at + 1) == Some(&b'\n') => self.bump(),
                _ => return,
            }
        }
    }

    fn comment(&mut self) {
        while !matches!(self.peek(), None | Some(b'\n')) {
            self.bump();
        }
    }

    /// The end of a key/value line: a comment and a newline, or the end.
    fn end_of_line(&mut self) -> Step<()> {
        self.ws();
        if self.peek() == Some(b'#') {
            self.comment();
        }
        self.eat(b'\r');
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => {
                self.bump();
                Ok(())
            }
            Some(_) => Err(self.here()),
        }
    }

    fn simple_key(&mut self) -> Step<&'a [u8]> {
        match self.peek() {
            Some(q @ (b'"' | b'\'')) => match self.string(q)? {
                Value::Str(s, Escapes::None) => Ok(s),
                _ => Err(self.here()),
            },
            _ => {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                {
                    self.bump();
                }
                if self.at == start {
                    return Err(self.here());
                }
                Ok(&self.t[start..self.at])
            }
        }
    }

    /// A dotted key, onto `base`; the key's own text for a report.
    fn key(&mut self, base: Path<'a>) -> Step<(Path<'a>, &'a [u8])> {
        let start = self.at;
        let mut path = base;
        loop {
            let k = self.simple_key()?;
            path = path.push(k).ok_or(self.here())?;
            let end = self.at;
            self.ws();
            if !self.eat(b'.') {
                return Ok((path, &self.t[start..end]));
            }
            self.ws();
        }
    }

    /// A string whose opening quote is next.
    fn string(&mut self, q: u8) -> Step<Value<'a>> {
        let line = self.here();
        let multi = self.t[self.at..].starts_with(&[q, q, q]);
        let open = if multi { 3 } else { 1 };
        for _ in 0..open {
            self.bump();
        }
        if multi && self.peek() == Some(b'\n') {
            self.bump();
        }
        let start = self.at;
        let mut escapes = Escapes::None;
        loop {
            match self.peek() {
                None => return Err(line),
                Some(b'\n') if !multi => return Err(line),
                Some(b'\\') if q == b'"' => {
                    escapes = Escapes::Some;
                    self.bump();
                    self.bump();
                }
                Some(c) if c == q => {
                    if !multi {
                        let s = &self.t[start..self.at];
                        self.bump();
                        return Ok(Value::Str(s, escapes));
                    }
                    if self.t[self.at..].starts_with(&[q, q, q]) {
                        let s = &self.t[start..self.at];
                        for _ in 0..3 {
                            self.bump();
                        }
                        return Ok(Value::Str(s, escapes));
                    }
                    self.bump();
                }
                Some(_) => self.bump(),
            }
        }
    }

    /// A scalar that is not a string: an integer is read, the rest are
    /// tokens -- booleans, floats, dates -- that no colour is.
    fn scalar(&mut self) -> Step<Value<'a>> {
        let start = self.at;
        while self.peek().is_some_and(|c| {
            c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'_' | b'.' | b':')
        }) {
            self.bump();
        }
        let tok = &self.t[start..self.at];
        if tok.is_empty() {
            return Err(self.here());
        }
        Ok(integer(tok).map_or(Value::Other, Value::Int))
    }

    /// A value at `path`, laid onto `b` (an inline table's or array's
    /// members by their own paths).
    fn value<F: FnMut(Line, &[u8], Skipped)>(
        &mut self,
        b: &mut Builder<'a, F>,
        path: Path<'a>,
        key: &'a [u8],
        line: Line,
    ) -> Step<()> {
        match self.peek() {
            Some(q @ (b'"' | b'\'')) => {
                let v = self.string(q)?;
                b.set(line, key, path, v);
            }
            Some(b'{') => self.inline_table(b, path, key, line)?,
            Some(b'[') => {
                self.bump();
                loop {
                    self.ws_nl();
                    if self.eat(b']') {
                        break;
                    }
                    if self.peek() == Some(b'{') {
                        let mut elem = path;
                        self.elems += 1;
                        elem.elem = self.elems;
                        let at = self.here();
                        self.value(b, elem, key, at)?;
                    } else {
                        self.skip_value()?;
                    }
                    self.ws_nl();
                    if self.eat(b',') {
                        continue;
                    }
                    self.ws_nl();
                    if !self.eat(b']') {
                        return Err(self.here());
                    }
                    break;
                }
                if path.elem == 0 && !is_indexed(&path) {
                    b.set(line, key, path, Value::Other);
                }
            }
            _ => {
                let v = self.scalar()?;
                b.set(line, key, path, v);
            }
        }
        Ok(())
    }

    /// A value that holds no colour -- an array's string, number or inner
    /// array -- read past.
    fn skip_value(&mut self) -> Step<()> {
        match self.peek() {
            Some(q @ (b'"' | b'\'')) => self.string(q).map(|_| ()),
            Some(b'[') => {
                self.bump();
                loop {
                    self.ws_nl();
                    if self.eat(b']') {
                        return Ok(());
                    }
                    self.skip_value()?;
                    self.ws_nl();
                    if !self.eat(b',') {
                        self.ws_nl();
                        return if self.eat(b']') {
                            Ok(())
                        } else {
                            Err(self.here())
                        };
                    }
                }
            }
            _ => self.scalar().map(|_| ()),
        }
    }

    /// `{ k = v, ... }`, on one line; `{ r, g, b }` read as one colour.
    fn inline_table<F: FnMut(Line, &[u8], Skipped)>(
        &mut self,
        b: &mut Builder<'a, F>,
        path: Path<'a>,
        key: &'a [u8],
        line: Line,
    ) -> Step<()> {
        self.bump();
        let mut rgb_parts: [Option<i64>; 3] = [None; 3];
        let mut others = 0u32;
        self.ws();
        if self.eat(b'}') {
            return Ok(());
        }
        // Members are buffered as the struct form needs all three first.
        let mut members: [Option<(Path<'a>, &'a [u8], Value<'a>)>; 16] = [None; 16];
        let mut n = 0usize;
        loop {
            self.ws();
            let (p, k) = self.key(path)?;
            self.ws();
            if !self.eat(b'=') {
                return Err(self.here());
            }
            self.ws();
            let leaf = p.segs().last().copied().unwrap_or(b"");
            match self.peek() {
                Some(b'{' | b'[') => {
                    others += 1;
                    self.value(b, p, k, line)?;
                }
                Some(q @ (b'"' | b'\'')) => {
                    let v = self.string(q)?;
                    others += 1;
                    *members.get_mut(n).ok_or(self.here())? = Some((p, k, v));
                    n += 1;
                }
                _ => {
                    let v = self.scalar()?;
                    match (leaf, v, p.len == path.len + 1) {
                        (b"r", Value::Int(i), true) => rgb_parts[0] = Some(i),
                        (b"g", Value::Int(i), true) => rgb_parts[1] = Some(i),
                        (b"b", Value::Int(i), true) => rgb_parts[2] = Some(i),
                        _ => others += 1,
                    }
                    *members.get_mut(n).ok_or(self.here())? = Some((p, k, v));
                    n += 1;
                }
            }
            self.ws();
            if self.eat(b',') {
                continue;
            }
            if self.eat(b'}') {
                break;
            }
            return Err(self.here());
        }
        if others == 0 && rgb_parts.iter().all(Option::is_some) {
            b.set(line, key, path, Value::Struct(rgb_parts));
            return Ok(());
        }
        for (p, k, v) in members.into_iter().flatten() {
            b.set(line, k, p, v);
        }
        Ok(())
    }

    fn header(&mut self) -> Step<Path<'a>> {
        let array = self.t[self.at..].starts_with(b"[[");
        self.bump();
        if array {
            self.bump();
        }
        self.ws();
        let (mut path, _) = self.key(Path::ROOT)?;
        self.ws();
        if !self.eat(b']') || (array && !self.eat(b']')) {
            return Err(self.here());
        }
        if array {
            self.elems += 1;
            path.elem = self.elems;
        }
        self.end_of_line()?;
        Ok(path)
    }
}

/// A TOML integer: decimal with a sign, or `0x`, `0o`, `0b`; `_` between
/// digits.
fn integer(tok: &[u8]) -> Option<i64> {
    let (neg, body) = match tok {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, tok),
    };
    let (base, digits) = match body {
        [b'0', b'x', rest @ ..] => (16, rest),
        [b'0', b'o', rest @ ..] => (8, rest),
        [b'0', b'b', rest @ ..] => (2, rest),
        _ => (10, body),
    };
    if digits.is_empty() || digits[0] == b'_' || digits.last() == Some(&b'_') {
        return None;
    }
    let mut n: i64 = 0;
    for &d in digits {
        if d == b'_' {
            continue;
        }
        n = n
            .checked_mul(i64::from(base))?
            .checked_add(i64::from((d as char).to_digit(base)?))?;
    }
    Some(if neg { -n } else { n })
}

fn is_indexed(p: &Path<'_>) -> bool {
    p.segs() == [&b"colors"[..], b"indexed_colors"]
}

/// An alacritty TOML config, or a theme written as one
/// (`[[source: alacritty-0c405d5-man-5]]` 30-47: themes come in through
/// `[general] import`, each a TOML file of the same keys). "A TOML file
/// must be a valid UTF-8 encoded Unicode document" (`[[source: toml-1-0-0]]`
/// 44), so other bytes refuse it.
pub fn alacritty(
    text: &[u8],
    ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    super::utf8(text)?;
    let mut b = Builder::new(Format::Alacritty, ignored);
    let mut p = Toml {
        t: text,
        at: 0,
        line: 1,
        elems: 0,
    };
    let mut table = Path::ROOT;
    let syntax = |line| SchemeError {
        line,
        kind: SchemeErrorKind::Syntax,
    };
    loop {
        p.ws_nl();
        let Some(c) = p.peek() else { break };
        if c == b'[' {
            table = p.header().map_err(syntax)?;
            continue;
        }
        let line = p.here();
        let (path, key) = p.key(table).map_err(syntax)?;
        p.ws();
        if !p.eat(b'=') {
            return Err(syntax(p.here()));
        }
        p.ws();
        p.value(&mut b, path, key, line).map_err(syntax)?;
        p.end_of_line().map_err(syntax)?;
    }
    Ok(b.finish())
}

/// A YAML scalar's text: single-quoted (`''` its quote), double-quoted (a
/// backslash marking an escape), or plain up to a ` #` comment.
fn yaml_scalar(v: &[u8]) -> Option<(Value<'_>, &[u8])> {
    match v.first()? {
        b'\'' => {
            let mut i = 1;
            loop {
                match v.get(i)? {
                    b'\'' if v.get(i + 1) == Some(&b'\'') => i += 2,
                    b'\'' => break,
                    _ => i += 1,
                }
            }
            let esc = if v[1..i].windows(2).any(|w| w == b"''") {
                Escapes::Some
            } else {
                Escapes::None
            };
            Some((Value::Str(&v[1..i], esc), &v[i + 1..]))
        }
        b'"' => {
            let mut i = 1;
            let mut esc = Escapes::None;
            loop {
                match v.get(i)? {
                    b'\\' => {
                        esc = Escapes::Some;
                        i += 2;
                    }
                    b'"' => break,
                    _ => i += 1,
                }
            }
            Some((Value::Str(&v[1..i], esc), &v[i + 1..]))
        }
        _ => {
            let mut end = v.len();
            for i in 0..v.len() {
                if matches!(v[i], b',' | b'}') || (v[i] == b'#' && i > 0 && v[i - 1] == b' ') {
                    end = i;
                    break;
                }
            }
            let s = trim(&v[..end]);
            // A plain `0x1d1f21` or `16` is a YAML integer, not a string.
            Some((
                integer(s).map_or(Value::Str(s, Escapes::None), Value::Int),
                &v[end..],
            ))
        }
    }
}

fn trim(t: &[u8]) -> &[u8] {
    super::trim_with(t, b" \t")
}

/// `{ k: v, ... }` on one line, each member laid at its own path.
fn yaml_flow<'a, F: FnMut(Line, &[u8], Skipped)>(
    b: &mut Builder<'a, F>,
    line: Line,
    path: Path<'a>,
    v: &'a [u8],
) -> Option<()> {
    let mut rest = trim(v.strip_prefix(b"{")?);
    let mut members: [Option<(&'a [u8], Value<'a>)>; 16] = [None; 16];
    let mut n = 0;
    loop {
        rest = trim(rest);
        if let Some(r) = rest.strip_prefix(b"}") {
            if !trim(r).is_empty() && trim(r)[0] != b'#' {
                return None;
            }
            break;
        }
        let colon = rest.iter().position(|&c| c == b':')?;
        let k = trim(&rest[..colon]);
        let (val, after) = yaml_scalar(trim(&rest[colon + 1..]))?;
        *members.get_mut(n)? = Some((k, val));
        n += 1;
        rest = trim(after);
        rest = rest.strip_prefix(b",").unwrap_or(rest);
    }
    let parts = [b"r", b"g", b"b"].map(|c| {
        members
            .iter()
            .flatten()
            .find(|(k, _)| *k == &c[..])
            .and_then(|(_, v)| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
    });
    if n == 3 && parts.iter().all(Option::is_some) {
        b.set(
            line,
            path.segs().last().copied().unwrap_or(b""),
            path,
            Value::Struct(parts),
        );
        return Some(());
    }
    for (k, val) in members.into_iter().flatten() {
        b.set(line, k, path.push(k)?, val);
    }
    Some(())
}

/// alacritty's YAML config before 0.13 (`[[source: alacritty-0-12-3-yml]]`
/// 200-340, the same keys under `colors:`), a YAML subset: block mappings
/// by indentation, `- { index: 16, color: '#ff00ff' }` sequence entries,
/// flow mappings on one line, and quoted or plain scalars. Lines outside
/// `colors:` are passed over by indentation; inside it, a line that is no
/// part of the subset refuses the file.
pub fn alacritty_yaml(
    text: &[u8],
    ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    let mut b = Builder::new(Format::AlacrittyYaml, ignored);
    // The open mappings: each key and its indentation.
    let mut stack: [(usize, &[u8]); 6] = [(0, b""); 6];
    let mut depth = 0usize;
    let mut elems = 0u32;
    for (raw, n) in text.split(|&c| c == b'\n').zip(1u32..) {
        let line = Line(n);
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let indent = raw.iter().take_while(|&&c| c == b' ').count();
        let body = trim(&raw[indent..]);
        if body.is_empty() || body[0] == b'#' || body == b"---" {
            continue;
        }
        // A block sequence may sit at its key's own indentation.
        let entry = body.starts_with(b"-");
        while depth > 0 && (stack[depth - 1].0 > indent || (!entry && stack[depth - 1].0 == indent))
        {
            depth -= 1;
        }
        let in_colors =
            depth > 0 && stack[0].1 == b"colors" || (depth == 0 && body.starts_with(b"colors:"));
        let syntax = SchemeError {
            line,
            kind: SchemeErrorKind::Syntax,
        };
        let mut path = Path::ROOT;
        for &(_, k) in &stack[..depth] {
            path = path.push(k).ok_or(syntax)?;
        }
        if let Some(entry) = body.strip_prefix(b"-") {
            if !in_colors {
                continue;
            }
            elems += 1;
            path.elem = elems;
            let entry = trim(entry);
            if entry.starts_with(b"{") {
                yaml_flow(&mut b, line, path, entry).ok_or(syntax)?;
                continue;
            }
            return Err(syntax);
        }
        let Some(colon) = body.iter().position(|&c| c == b':') else {
            if in_colors {
                return Err(syntax);
            }
            continue;
        };
        let key = trim(&body[..colon]);
        let rest = trim(&body[colon + 1..]);
        if depth == 0 && key != b"colors" {
            b.set(line, key, path.push(key).ok_or(syntax)?, Value::Other);
        }
        if rest.is_empty() || rest[0] == b'#' {
            if depth < stack.len() {
                stack[depth] = (indent, key);
                depth += 1;
            }
            continue;
        }
        if !in_colors {
            continue;
        }
        let path = path.push(key).ok_or(syntax)?;
        if rest.starts_with(b"{") {
            yaml_flow(&mut b, line, path, rest).ok_or(syntax)?;
            continue;
        }
        if rest == b"[]" {
            continue;
        }
        let (v, after) = yaml_scalar(rest).ok_or(syntax)?;
        let after = trim(after);
        if !after.is_empty() && after[0] != b'#' {
            return Err(syntax);
        }
        b.set(line, key, path, v);
    }
    Ok(b.finish())
}
