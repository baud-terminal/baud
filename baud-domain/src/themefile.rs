//! Colour scheme files: bytes to a [`Scheme`], refusing what a format leaves
//! unsettled (note Y, section 4; the pattern `terminfo` follows).
//!
//! **Each format lands on sixteen ANSI slots by its own published mapping**,
//! plus a page and a text colour held apart from the slots, because every
//! format has a page and a text colour and not every format puts them in
//! slots 0 and 7:
//!
//! - **base16** by tinted-theming's table (`[[source: tinted-styling-base16]]`
//!   v0.4.2): base00 is 0 and the page, base05 is 7 and the text, base08 is
//!   "1 and 9", base0B 2 and 10, base0A 3 and 11, base0D 4 and 12, base0E 5
//!   and 13, base0C 6 and 14, base03 8, base07 15; base09, 0F, 01, 02, 04 and
//!   06 are 16-21.
//! - **base24** (`[[source: base24-styling]]`): base16's 0-8 and 15, and its
//!   brights from base12 (9), base14 (10), base13 (11), base16 (12), base17
//!   (13), base15 (14); base10 and base11 have no ANSI number.
//!
//! Entries 16-21 are parsed and kept, and baud does not write them: entries
//! 16-255 stay xterm's, which programs addressing the cube by number expect.
//! The file format is `[[source: tinted-builder]]` 0.11.2: the current nested
//! form (`system`, `palette:` and its children) and the legacy flat form
//! (`scheme`, and every other key a slot), with `system` inferred from which
//! slots are present when it is missing. Values "MUST be in HTML hex format
//! and MAY be preceded by a `#`".

use crate::{Chan, Color, ColorScheme, ColorWord};

/// Why a colour value was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HexError {
    /// Nothing there.
    Empty,
    /// A character that is not a hex digit: an X11 name (`red`), a keyword
    /// (`none`, `background`) or an `rgb:` form, none of which a scheme file
    /// in these formats may use.
    NotHex,
    /// Hex digits, but not three or six of them: `#rrggbbaa` is eight.
    Width,
}

/// `#rrggbb`, `#rgb` (each digit doubled, as HTML hex has it) or either
/// without the `#`.
pub fn hex(text: &[u8]) -> Result<Color, HexError> {
    let t = trim(text);
    let t = t.strip_prefix(b"#").unwrap_or(t);
    if t.is_empty() {
        return Err(HexError::Empty);
    }
    if !t.iter().all(u8::is_ascii_hexdigit) {
        return Err(HexError::NotHex);
    }
    let d = |b: u8| match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        _ => b - b'A' + 10,
    };
    match *t {
        [r1, r2, g1, g2, b1, b2] => Ok(rgb(
            d(r1) * 16 + d(r2),
            d(g1) * 16 + d(g2),
            d(b1) * 16 + d(b2),
        )),
        [r, g, b] => Ok(rgb(d(r) * 17, d(g) * 17, d(b) * 17)),
        _ => Err(HexError::Width),
    }
}

fn trim(t: &[u8]) -> &[u8] {
    let start = t
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(t.len());
    let end = t
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |e| e + 1);
    &t[start..end]
}

/// Which format a [`Scheme`] was read from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Base16,
    Base24,
    Ghostty,
}

/// A colour scheme, landed on the sixteen ANSI slots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scheme {
    /// Entries 0-15. `None` only where a format lets a file leave one unset
    /// (a ghostty file); base16 and base24 set all sixteen.
    pub ansi: [Option<Color>; 16],
    /// The page and the text, as `OSC 11` and `OSC 10` would answer.
    pub page: Option<Color>,
    pub text: Option<Color>,
    /// base16's and base24's entries 16-21, parsed and not written.
    pub extended: [Option<Color>; 6],
    pub cursor: Option<Color>,
    pub cursor_text: Option<Color>,
    pub selection: Option<Color>,
    pub selection_text: Option<Color>,
    pub format: Format,
    /// What the file says it is -- tinted's `variant` -- which is checked
    /// against [`Scheme::polarity`] and never used in its place.
    pub declared: Option<ColorScheme>,
}

impl Scheme {
    /// Dark or light by the rule `CSI ? 996 n` answers with (note Y,
    /// decision 5): the page's relative luminance against the text's.
    pub fn polarity(&self) -> Option<ColorScheme> {
        ColorScheme::of(
            ColorWord::from_color(self.text?),
            ColorWord::from_color(self.page?),
        )
    }
}

/// A line of a scheme file, 1-based, for a complaint to name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line(pub u32);

/// Why a scheme file was refused, and where.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemeError {
    pub line: Line,
    pub kind: SchemeErrorKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SchemeErrorKind {
    /// A colour value that did not read.
    Hex(HexError),
    /// A slot the format requires is missing: `base0x`/`base1x` by its
    /// number.
    Missing(Slot),
    /// The same key twice, which YAML does not allow.
    Duplicate,
    /// `system` names a scheme system baud does not read.
    UnknownSystem,
    /// A line that is not `key: value` in a place a key must be.
    Syntax,
    /// A ghostty `palette` entry whose number is not 0-255.
    PaletteIndex,
}

/// A base16/base24 slot number, `0x00` to `0x17`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot(pub u8);

/// A base16 or base24 scheme file, current or legacy form.
///
/// **The YAML is a subset**, the shape the spec and every file in the tinted
/// corpus use: `key: value` lines, the value double- or single-quoted or
/// bare, a `#` comment on its own line or after a value, `palette:` opening
/// the indented slots, and a block scalar (`description: >`) skipped whole --
/// its lines are text, and a slot name inside one is not a slot. A tab in the
/// indentation, which YAML forbids, is refused.
pub fn base16(text: &[u8]) -> Result<Scheme, SchemeError> {
    let mut slots: [Option<Color>; 24] = [None; 24];
    let mut system: Option<Format> = None;
    let mut declared = None;
    let (mut seen_system, mut seen_variant) = (false, false);
    // The indentation of a block scalar's key, while its lines are skipped.
    let mut block: Option<usize> = None;
    let mut in_palette = false;
    let mut line_no = 0u32;
    for raw in text.split(|&b| b == b'\n') {
        line_no += 1;
        let line = Line(line_no);
        let err = |kind| SchemeError { line, kind };
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let indent = raw.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
        if raw[..indent].contains(&b'\t') {
            return Err(err(SchemeErrorKind::Syntax));
        }
        let body = trim(&raw[indent..]);
        if let Some(k) = block {
            if body.is_empty() || indent > k {
                continue;
            }
            block = None;
        }
        if body.is_empty() || body[0] == b'#' {
            continue;
        }
        if body == b"---" {
            continue;
        }
        let Some(colon) = body.iter().position(|&b| b == b':') else {
            return Err(err(SchemeErrorKind::Syntax));
        };
        let key = trim(&body[..colon]);
        let value = yaml_value(&body[colon + 1..]).ok_or_else(|| err(SchemeErrorKind::Syntax))?;
        if matches!(value.first(), Some(b'|' | b'>')) {
            block = Some(indent);
            continue;
        }
        if indent == 0 {
            in_palette = false;
        } else if !in_palette {
            // A nested map baud does not read.
            continue;
        }
        if let Some(n) = slot_of(key) {
            let c = hex(value).map_err(|e| err(SchemeErrorKind::Hex(e)))?;
            let s = &mut slots[usize::from(n)];
            if s.is_some() {
                return Err(err(SchemeErrorKind::Duplicate));
            }
            *s = Some(c);
            continue;
        }
        if indent > 0 {
            continue;
        }
        match key {
            b"palette" if value.is_empty() => in_palette = true,
            b"system" => {
                if seen_system {
                    return Err(err(SchemeErrorKind::Duplicate));
                }
                seen_system = true;
                system = Some(match value {
                    b"base16" => Format::Base16,
                    b"base24" => Format::Base24,
                    _ => return Err(err(SchemeErrorKind::UnknownSystem)),
                });
            }
            b"variant" => {
                if seen_variant {
                    return Err(err(SchemeErrorKind::Duplicate));
                }
                seen_variant = true;
                // "Currently only light and dark are used, but this value
                // could be anything": anything else declares nothing.
                declared = match value {
                    b"light" => Some(ColorScheme::Light),
                    b"dark" => Some(ColorScheme::Dark),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    // `system` missing: "determine ... base16 or base24 format depending on
    // which colors are provided".
    let format = system.unwrap_or(if slots[0x10..].iter().any(Option::is_some) {
        Format::Base24
    } else {
        Format::Base16
    });
    let needed = if format == Format::Base24 { 24 } else { 16 };
    // The last line of the file, not the empty piece after its final newline.
    let eof = Line(if text.ends_with(b"\n") {
        line_no - 1
    } else {
        line_no
    });
    let mut got = [rgb(0, 0, 0); 24];
    for n in 0..needed {
        got[n] = slots[n].ok_or(SchemeError {
            line: eof,
            kind: SchemeErrorKind::Missing(Slot(u8::try_from(n).unwrap_or(u8::MAX))),
        })?;
    }
    let b = |n: usize| Some(got[n]);
    // The brights: base16 repeats the normals; base24 has its own.
    let bright = if format == Format::Base24 {
        [b(0x12), b(0x14), b(0x13), b(0x16), b(0x17), b(0x15)]
    } else {
        [b(0x08), b(0x0B), b(0x0A), b(0x0D), b(0x0E), b(0x0C)]
    };
    Ok(Scheme {
        ansi: [
            b(0x00),
            b(0x08),
            b(0x0B),
            b(0x0A),
            b(0x0D),
            b(0x0E),
            b(0x0C),
            b(0x05),
            b(0x03),
            bright[0],
            bright[1],
            bright[2],
            bright[3],
            bright[4],
            bright[5],
            b(0x07),
        ],
        page: b(0x00),
        text: b(0x05),
        extended: [b(0x09), b(0x0F), b(0x01), b(0x02), b(0x04), b(0x06)],
        cursor: None,
        cursor_text: None,
        selection: None,
        selection_text: None,
        format,
        declared,
    })
}

/// The value after `key:`: quoted -- its inside, with only a comment allowed
/// after the closing quote -- or bare up to a comment, which YAML starts at a
/// `#` that begins the value or follows whitespace. `None` for a quote left
/// open.
fn yaml_value(v: &[u8]) -> Option<&[u8]> {
    let v = trim(v);
    if let Some(&q @ (b'"' | b'\'')) = v.first() {
        let close = v[1..].iter().position(|&b| b == q)? + 1;
        let rest = trim(&v[close + 1..]);
        return (rest.is_empty() || rest[0] == b'#').then_some(&v[1..close]);
    }
    let mut end = v.len();
    for i in 0..v.len() {
        if v[i] == b'#' && (i == 0 || v[i - 1].is_ascii_whitespace()) {
            end = i;
            break;
        }
    }
    Some(trim(&v[..end]))
}

/// `base00` to `base17` as a slot number; anything else is not a slot.
fn slot_of(key: &[u8]) -> Option<u8> {
    let [b'b', b'a', b's', b'e', hi, lo] = *key else {
        return None;
    };
    let d = |x: u8| (x as char).to_digit(16).and_then(|v| u8::try_from(v).ok());
    let n = d(hi)? * 16 + d(lo)?;
    (n <= 0x17).then_some(n)
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}
