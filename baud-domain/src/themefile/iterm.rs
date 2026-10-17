//! iTerm2's colour presets (`.itermcolors`) and Windows Terminal's colour
//! schemes, each read as its own program reads it onto the same
//! [`Scheme`].

use super::gamut::{Fit, Space, to_srgb};
use super::terminals::Skipped;
use super::{Format, HexError, Line, Scheme, SchemeError, SchemeErrorKind, rgb};
use crate::Color;

/// The line a byte offset is on.
fn line_at(text: &[u8], at: usize) -> Line {
    let n = text[..at.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count();
    Line(u32::try_from(n).map_or(u32::MAX, |n| n.saturating_add(1)))
}

/// The `encoding` an XML declaration at the start of `text` names.
///
/// An entity in UTF-8 "contains any ill-formed code unit sequences", or
/// one with "no encoding declaration and its content is not legal UTF-8",
/// is a fatal error (`[[source: xml-1-0-5e]]` 4.3.3), so a preset that
/// declares no encoding, or UTF-8, must be UTF-8. Under another declared
/// encoding it is read as bytes: a plist's tags, keys and numbers are
/// ASCII, one byte each in any encoding that spells `<?xml` in single
/// bytes.
fn declared_encoding(text: &[u8]) -> Option<&[u8]> {
    let decl = text.strip_prefix(b"<?xml")?;
    let decl = &decl[..decl.windows(2).position(|w| w == b"?>")?];
    let at = decl.windows(8).position(|w| w == b"encoding")?;
    let rest = trim_start(&decl[at + 8..]).strip_prefix(b"=")?;
    let rest = trim_start(rest);
    let (&q, rest) = rest.split_first()?;
    if q != b'"' && q != b'\'' {
        return None;
    }
    Some(&rest[..rest.iter().position(|&b| b == q)?])
}

fn trim_start(t: &[u8]) -> &[u8] {
    let n = t
        .iter()
        .take_while(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        .count();
    &t[n..]
}

/// An XML token, as far as a property list needs one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag<'a> {
    Open(&'a [u8]),
    Close(&'a [u8]),
    /// `<true/>`, `<false/>`, `<dict/>`.
    Empty(&'a [u8]),
    Text(&'a [u8]),
}

/// A property list's XML, token by token: the declaration, `<!DOCTYPE>`
/// and comments are passed over, and so is whitespace between elements.
struct Xml<'a> {
    t: &'a [u8],
    at: usize,
}

impl<'a> Xml<'a> {
    fn starts(&self, s: &[u8]) -> bool {
        self.t[self.at..].starts_with(s)
    }

    fn skip_past(&mut self, end: &[u8]) -> Option<()> {
        let rest = &self.t[self.at..];
        let i = rest.windows(end.len()).position(|w| w == end)?;
        self.at += i + end.len();
        Some(())
    }

    /// The next token, `None` at the end, `Err` at a malformed one.
    fn next(&mut self) -> Result<Option<(usize, Tag<'a>)>, usize> {
        loop {
            let start = self.at;
            if self.at >= self.t.len() {
                return Ok(None);
            }
            if self.starts(b"<?") {
                self.skip_past(b"?>").ok_or(start)?;
                continue;
            }
            if self.starts(b"<!--") {
                self.skip_past(b"-->").ok_or(start)?;
                continue;
            }
            if self.starts(b"<!") {
                self.skip_past(b">").ok_or(start)?;
                continue;
            }
            if self.t[self.at] == b'<' {
                self.skip_past(b">").ok_or(start)?;
                let inner = &self.t[start + 1..self.at - 1];
                let name_of = |s: &'a [u8]| {
                    let n = s
                        .iter()
                        .take_while(|c| !c.is_ascii_whitespace() && **c != b'/')
                        .count();
                    &s[..n]
                };
                return Ok(Some(match inner {
                    [b'/', rest @ ..] => (start, Tag::Close(name_of(rest))),
                    [.., b'/'] => (start, Tag::Empty(name_of(inner))),
                    _ => (start, Tag::Open(name_of(inner))),
                }));
            }
            let n = self.t[self.at..].iter().take_while(|&&c| c != b'<').count();
            self.at += n;
            let text = &self.t[start..self.at];
            if text.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            return Ok(Some((start, Tag::Text(text))));
        }
    }

    /// The text of an element whose open tag was just read, through its
    /// close tag.
    fn text_of(&mut self, name: &[u8]) -> Result<&'a [u8], usize> {
        let start = self.at;
        match self.next()? {
            Some((_, Tag::Close(n))) if n == name => Ok(b""),
            Some((at, Tag::Text(t))) => match self.next()? {
                Some((_, Tag::Close(n))) if n == name => Ok(t),
                _ => Err(at),
            },
            _ => Err(start),
        }
    }

    /// Past one value, whatever it is: an element and all inside it.
    fn skip_value(&mut self, first: Tag<'a>) -> Result<(), usize> {
        let Tag::Open(name) = first else {
            return Ok(());
        };
        let mut depth = 1u32;
        while depth > 0 {
            match self.next()? {
                Some((_, Tag::Open(_))) => depth += 1,
                Some((_, Tag::Close(n))) => {
                    depth -= 1;
                    if depth == 0 && n != name {
                        return Err(self.at);
                    }
                }
                Some(_) => {}
                None => return Err(self.at),
            }
        }
        Ok(())
    }
}

/// A preset key iTerm2 reads as one of a [`Scheme`]'s colours
/// (`[[source: iterm2cs-gen-py]]` 387-416, iTerm2's own key names).
fn iterm_slot<'s>(s: &'s mut Scheme, key: &[u8]) -> Option<&'s mut Option<Color>> {
    Some(match key {
        b"Background Color" => &mut s.page,
        b"Foreground Color" => &mut s.text,
        b"Cursor Color" => &mut s.cursor,
        b"Cursor Text Color" => &mut s.cursor_text,
        b"Selection Color" => &mut s.selection,
        b"Selected Text Color" => &mut s.selection_text,
        _ => {
            let n = key.strip_prefix(b"Ansi ")?.strip_suffix(b" Color")?;
            let i = match *n {
                [d] if d.is_ascii_digit() => d - b'0',
                [b'1', d @ b'0'..=b'5'] => 10 + d - b'0',
                _ => return None,
            };
            &mut s.ansi[usize::from(i)]
        }
    })
}

/// An iTerm2 colour preset: an XML property list whose dictionary maps a
/// key such as `Ansi 0 Color` to a colour dictionary of `Red Component`,
/// `Green Component` and `Blue Component`, reals, and an optional `Color
/// Space`. iTerm2 reads `sRGB` as sRGB, `P3` as Display P3 and anything
/// else -- `Calibrated`, or no key -- as macOS's calibrated RGB, and a
/// dictionary of fewer than three entries as black
/// (`[[source: iterm2-nsdictionary-9e29604]]` 214-243); each is converted
/// to sRGB here (Jev, decisions.jsonl 682/683, for `Calibrated`), and a
/// colour outside sRGB is clipped and said.
///
/// **A preset with `Foreground Color (Light)`** holds a light and a dark
/// scheme, iTerm2's own test (`[[source: iterm2-color-presets-9e29604]]`
/// 329-331); which one a baud half reads is undecided (note AR, O8), so it
/// is refused. Keys iTerm2 has and a scheme does not (`Bold Color`,
/// `Link Color`, ...) are said, and so is every other key.
pub fn iterm(
    text: &[u8],
    mut ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    if declared_encoding(text).is_none_or(|e| e.eq_ignore_ascii_case(b"UTF-8")) {
        super::utf8(text)?;
    }
    let mut s = Scheme::empty(Format::ITermPlist);
    let syntax = |at| SchemeError {
        line: line_at(text, at),
        kind: SchemeErrorKind::Syntax,
    };
    let mut x = Xml { t: text, at: 0 };
    // Into `<plist>` and its one dictionary.
    loop {
        match x.next().map_err(syntax)? {
            Some((_, Tag::Open(b"plist"))) => {}
            Some((_, Tag::Open(b"dict"))) => break,
            Some((at, _)) => return Err(syntax(at)),
            None => return Err(syntax(x.at)),
        }
    }
    loop {
        let (at, tok) = x.next().map_err(syntax)?.ok_or_else(|| syntax(x.at))?;
        let key = match tok {
            Tag::Close(b"dict") => break,
            Tag::Open(b"key") => x.text_of(b"key").map_err(syntax)?,
            _ => return Err(syntax(at)),
        };
        let line = line_at(text, at);
        if key == b"Foreground Color (Light)" {
            return Err(SchemeError {
                line,
                kind: SchemeErrorKind::Pair,
            });
        }
        let (vat, value) = x.next().map_err(syntax)?.ok_or_else(|| syntax(x.at))?;
        let Some(slot) = iterm_slot(&mut s, key) else {
            let why = if key.ends_with(b" Color") || key.ends_with(b")") {
                Skipped::Unheld
            } else {
                Skipped::NotAColour
            };
            x.skip_value(value).map_err(syntax)?;
            ignored(line, key, why);
            continue;
        };
        if value != Tag::Open(b"dict") {
            x.skip_value(value).map_err(syntax)?;
            ignored(line, key, Skipped::Refused);
            continue;
        }
        let mut parts: [Option<f64>; 3] = [None; 3];
        let mut space = Space::Calibrated;
        let mut entries = 0u32;
        loop {
            let (kat, tok) = x.next().map_err(syntax)?.ok_or_else(|| syntax(vat))?;
            let k = match tok {
                Tag::Close(b"dict") => break,
                Tag::Open(b"key") => x.text_of(b"key").map_err(syntax)?,
                _ => return Err(syntax(kat)),
            };
            entries += 1;
            let (eat, v) = x.next().map_err(syntax)?.ok_or_else(|| syntax(kat))?;
            let Tag::Open(kind) = v else {
                continue;
            };
            let body = x.text_of(kind).map_err(|_| syntax(eat))?;
            let number = || {
                core::str::from_utf8(body)
                    .ok()
                    .and_then(|t| t.trim().parse::<f64>().ok())
            };
            match (k, kind) {
                (b"Red Component", b"real" | b"integer") => parts[0] = number(),
                (b"Green Component", b"real" | b"integer") => parts[1] = number(),
                (b"Blue Component", b"real" | b"integer") => parts[2] = number(),
                (b"Color Space", b"string") => {
                    space = match body {
                        b"sRGB" => Space::Srgb,
                        b"P3" => Space::DisplayP3,
                        _ => Space::Calibrated,
                    };
                }
                _ => {}
            }
        }
        if entries < 3 {
            *slot = Some(rgb(0, 0, 0));
            continue;
        }
        let [Some(r), Some(g), Some(b)] = parts else {
            ignored(line, key, Skipped::Refused);
            continue;
        };
        let ([r, g, b], fit) = to_srgb(space, [r, g, b]);
        *slot = Some(rgb(r, g, b));
        if fit == Fit::Clipped {
            ignored(line, key, Skipped::Clipped);
        }
    }
    Ok(s)
}

/// A JSON value, as far as a scheme needs one.
#[derive(Clone, Copy)]
enum Json<'a> {
    Str(&'a [u8]),
    Other,
}

/// Whether a JSON value is the root, whose members are the scheme's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Depth {
    Root,
    Inner,
}

/// JSON as Windows Terminal's jsoncpp 1.9.6 reads it by default
/// (`[[source: jsoncpp-reader-1-9-6]]`, `setDefaults`): `//` and `/* */`
/// comments and trailing commas allowed, a key given twice taking its last
/// value, and nothing read after the root.
struct JsonParser<'a> {
    t: &'a [u8],
    at: usize,
}

impl<'a> JsonParser<'a> {
    fn ws(&mut self) -> Result<(), usize> {
        loop {
            match self.t.get(self.at) {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.at += 1,
                Some(b'/') if self.t.get(self.at + 1) == Some(&b'/') => {
                    while !matches!(self.t.get(self.at), None | Some(b'\n')) {
                        self.at += 1;
                    }
                }
                Some(b'/') if self.t.get(self.at + 1) == Some(&b'*') => {
                    let rest = &self.t[self.at + 2..];
                    let end = rest.windows(2).position(|w| w == b"*/").ok_or(self.at)?;
                    self.at += 2 + end + 2;
                }
                _ => return Ok(()),
            }
        }
    }

    fn string(&mut self) -> Result<&'a [u8], usize> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.t.get(self.at) {
                None | Some(b'\n') => return Err(start),
                Some(b'\\') => self.at += 2,
                Some(b'"') => {
                    self.at += 1;
                    return Ok(&self.t[start + 1..self.at - 1]);
                }
                Some(_) => self.at += 1,
            }
        }
    }

    /// One value; the root object's members are handed to `member`.
    fn value(
        &mut self,
        member: &mut impl FnMut(usize, &'a [u8], usize, Json<'a>),
        depth: Depth,
    ) -> Result<Json<'a>, usize> {
        self.ws()?;
        let start = self.at;
        match self.t.get(self.at) {
            Some(b'"') => self.string().map(Json::Str),
            Some(b'{') => {
                self.at += 1;
                loop {
                    self.ws()?;
                    if self.t.get(self.at) == Some(&b'}') {
                        self.at += 1;
                        return Ok(Json::Other);
                    }
                    if self.t.get(self.at) != Some(&b'"') {
                        return Err(self.at);
                    }
                    let kat = self.at;
                    let k = self.string()?;
                    self.ws()?;
                    if self.t.get(self.at) != Some(&b':') {
                        return Err(self.at);
                    }
                    self.at += 1;
                    self.ws()?;
                    let vat = self.at;
                    let v = self.value(member, Depth::Inner)?;
                    if depth == Depth::Root {
                        member(kat, k, vat, v);
                    }
                    self.ws()?;
                    match self.t.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b'}') => {}
                        _ => return Err(self.at),
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                loop {
                    self.ws()?;
                    if self.t.get(self.at) == Some(&b']') {
                        self.at += 1;
                        return Ok(Json::Other);
                    }
                    self.value(member, Depth::Inner)?;
                    self.ws()?;
                    match self.t.get(self.at) {
                        Some(b',') => self.at += 1,
                        Some(b']') => {}
                        _ => return Err(self.at),
                    }
                }
            }
            Some(_) => {
                let n = self.t[self.at..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
                    .count();
                if n == 0 {
                    return Err(start);
                }
                self.at += n;
                Ok(Json::Other)
            }
            None => Err(start),
        }
    }
}

/// Windows Terminal's `ColorFromHexString` behind its `CanConvert`
/// (`[[source: windows-terminal-utils-9fd41b2]]`,
/// `[[source: windows-terminal-jsonutils-9fd41b2]]` 786-809): `#rgb`,
/// each digit doubled, or `#rrggbb`; each pair read by `std::stoul` in base
/// 16, which stops at the first byte that is not a digit.
fn wt_colour(v: &[u8]) -> Option<Color> {
    let pair = |p: [u8; 2]| -> Option<u8> {
        let d = |b: u8| (b as char).to_digit(16).and_then(|v| u8::try_from(v).ok());
        let hi = d(p[0])?;
        Some(match d(p[1]) {
            Some(lo) => hi * 16 + lo,
            None => hi,
        })
    };
    match *v {
        [b'#', r, g, b] => Some(rgb(pair([r, r])?, pair([g, g])?, pair([b, b])?)),
        [b'#', r1, r2, g1, g2, b1, b2] => {
            Some(rgb(pair([r1, r2])?, pair([g1, g2])?, pair([b1, b2])?))
        }
        _ => None,
    }
}

/// Windows Terminal's table names in its own order, and the entry each is
/// (`[[source: windows-terminal-colorscheme-9fd41b2]]` 24-45): `magenta`
/// and `brightMagenta` are the alternates of `purple` and `brightPurple`.
const TABLE: [(&[u8], usize); 18] = [
    (b"black", 0),
    (b"red", 1),
    (b"green", 2),
    (b"yellow", 3),
    (b"blue", 4),
    (b"purple", 5),
    (b"cyan", 6),
    (b"white", 7),
    (b"brightBlack", 8),
    (b"brightRed", 9),
    (b"brightGreen", 10),
    (b"brightYellow", 11),
    (b"brightBlue", 12),
    (b"brightPurple", 13),
    (b"brightCyan", 14),
    (b"brightWhite", 15),
    (b"magenta", 5),
    (b"brightMagenta", 13),
];

/// A Windows Terminal colour scheme: one JSON object of `name`, the
/// optional `foreground`, `background`, `cursorColor` and
/// `selectionBackground`, and the table colours, read as Windows Terminal's
/// `ColorScheme::_layerJson` reads them: the table in [`TABLE`]'s order,
/// counting until sixteen are found; a scheme without a `name` or without
/// sixteen is invalid, and a colour Windows Terminal cannot convert refuses
/// it, as `GetValueForKey` throws (`[[source:
/// windows-terminal-jsonutils-9fd41b2]]` 140-180). Every other key is said.
/// A theme file is JSON "exchanged between systems", which "MUST be encoded
/// using UTF-8" (`[[source: rfc8259-json]]` 485-486), so other bytes refuse
/// it.
pub fn windows_terminal(
    text: &[u8],
    mut ignored: impl FnMut(Line, &[u8], Skipped),
) -> Result<Scheme, SchemeError> {
    super::utf8(text)?;
    let mut s = Scheme::empty(Format::WindowsTerminal);
    let mut name = false;
    let mut bad: Option<usize> = None;
    let mut table: [Option<(usize, Color)>; 18] = [None; 18];
    let mut p = JsonParser { t: text, at: 0 };
    let mut seen = |kat: usize, k: &[u8], vat: usize, v: Json<'_>| {
        let colour = |v: Json<'_>| match v {
            Json::Str(c) => wt_colour(c),
            Json::Other => None,
        };
        let slot = match k {
            b"name" => {
                name = matches!(v, Json::Str(_));
                return;
            }
            b"foreground" => &mut s.text,
            b"background" => &mut s.page,
            b"cursorColor" => &mut s.cursor,
            b"selectionBackground" => &mut s.selection,
            _ => {
                if let Some(i) = TABLE.iter().position(|(n, _)| *n == k) {
                    match colour(v) {
                        Some(c) => table[i] = Some((kat, c)),
                        None => {
                            bad.get_or_insert(vat);
                        }
                    }
                } else {
                    ignored(line_at(text, kat), k, Skipped::NotAColour);
                }
                return;
            }
        };
        match colour(v) {
            Some(c) => *slot = Some(c),
            None => {
                bad.get_or_insert(vat);
            }
        }
    };
    p.value(&mut seen, Depth::Root).map_err(|at| SchemeError {
        line: line_at(text, at),
        kind: SchemeErrorKind::Syntax,
    })?;
    if let Some(at) = bad {
        return Err(SchemeError {
            line: line_at(text, at),
            kind: SchemeErrorKind::Hex(HexError::NotHex),
        });
    }
    let mut count = 0;
    for (i, found) in table.iter().enumerate() {
        if count == 16 {
            break;
        }
        if let Some((_, c)) = found {
            s.ansi[TABLE[i].1] = Some(*c);
            count += 1;
        }
    }
    let eof = line_at(text, text.len());
    if !name {
        return Err(SchemeError {
            line: eof,
            kind: SchemeErrorKind::NoName,
        });
    }
    if count < 16 {
        return Err(SchemeError {
            line: eof,
            kind: SchemeErrorKind::Count,
        });
    }
    Ok(s)
}
