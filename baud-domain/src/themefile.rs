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
//! (the slots at the top level beside `scheme`, `author` and
//! `description`), with `system` inferred from which slots are present when
//! it is missing. baud reads `base00`-`base17` as slots, `system` and
//! `variant`, and ignores every other key in either form. Values "MUST be in HTML hex format
//! and MAY be preceded by a `#`".

use crate::{Chan, Color, ColorScheme, ColorWord};

mod alacritty;
mod colour_names;
pub mod detect;
pub mod gamut;
mod iterm;
mod oklab;
mod terminals;
mod xresources;

pub use alacritty::{alacritty, alacritty_yaml};

/// A colour name in kitty's table (`[[source: kitty-color-names-1e1486e]]`),
/// case ignored: what kitty's colour control reads a name by too.
pub(crate) fn kitty_colour_name(name: &[u8]) -> Option<[u8; 3]> {
    use colour_names::{POOL, SEEDS, SLOTS};
    crate::colour_name::find(&SEEDS, &SLOTS, POOL, name)
}
pub use iterm::{iterm, windows_terminal};
pub use terminals::{Skipped, foot, kitty, plain_list};
pub use xresources::{Cpp, DEPTH as XRM_DEPTH, XresourcesError, xresources};

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
    Kitty,
    Foot,
    Alacritty,
    AlacrittyYaml,
    ITermPlist,
    WindowsTerminal,
    /// Sixteen `#rrggbb` lines.
    PlainList,
    /// X resources, as `xrdb` loads them.
    Xresources,
    /// A file with nothing in it but blank lines and comments: no format
    /// to name, and it sets nothing.
    Empty,
}

/// **A colour a scheme gives the cursor or the selection**: a colour, or the
/// colour of the cell it lands on. alacritty's `CellForeground` and
/// `CellBackground` "references the affected cell" (`[[source:
/// alacritty-0c405d5-man-5]]` 327-334, 398-405); ghostty's
/// `cell-foreground` and `cell-background` match "the cell foreground
/// color" and "the cell background color" (`[[source:
/// ghostty-config-8f0dd37]]` 707-716, 845-859, 905-911).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellPaint<C> {
    Colour(C),
    /// The text colour of the cell it lands on.
    Text,
    /// The page of the cell it lands on.
    Page,
}

impl<C> CellPaint<C> {
    /// The same paint, its colour mapped.
    pub fn map<D>(self, f: impl FnOnce(C) -> D) -> CellPaint<D> {
        match self {
            CellPaint::Colour(c) => CellPaint::Colour(f(c)),
            CellPaint::Text => CellPaint::Text,
            CellPaint::Page => CellPaint::Page,
        }
    }

    /// The colour, when it is one rather than a cell's.
    pub fn colour(self) -> Option<C> {
        match self {
            CellPaint::Colour(c) => Some(c),
            CellPaint::Text | CellPaint::Page => None,
        }
    }
}

/// **Under which cursor the text under a block is drawn**: every cursor,
/// or, after a kitty theme's `cursor none`, only a cursor with a colour of
/// its own. kitty.conf's cursor_text_color "is ignored" while cursor is
/// none, and a cursor colour set after it, the config's or a program's,
/// which "takes precedence", brings it back (`[[source:
/// kitty-options-definition]]` 353-379), as kitty's renderer has it
/// (`[[source: kitty-1e1486e-shaders-c]]` 937-954).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorText {
    #[default]
    UnderEveryCursor,
    UnderACursorColour,
}

/// **What a selection with no text colour of its own answers as its
/// text**: reverse video's, the page, as xterm answers, or after a kitty
/// theme's `selection_foreground none`, "the foreground color to be used
/// unchanged" (`[[source: kitty-options-definition]]` 2641-2657): each
/// cell's own text over a selection page of its own, and reverse video's
/// where the selection has none (`[[source: kitty-1e1486e-shaders-c]]`
/// 873-885). Either is drawn so; only `OSC 19 ; ?` tells them apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SelectionText {
    #[default]
    Reverse,
    Unchanged,
}

/// Where a scheme file's value lands: one of the colours a scheme holds as
/// a colour alone, or one of the four that may name a cell's.
pub(crate) enum Target<'s> {
    Colour(&'s mut Option<Color>),
    Paint(&'s mut Option<CellPaint<Color>>),
}

impl Target<'_> {
    /// Sets a colour, or unsets.
    pub(crate) fn set(self, c: Option<Color>) {
        match self {
            Target::Colour(s) => *s = c,
            Target::Paint(s) => *s = c.map(CellPaint::Colour),
        }
    }

    /// Sets a cell's colour where the slot takes one, and says whether it
    /// did.
    pub(crate) fn set_cell(self, p: CellPaint<Color>) -> bool {
        match self {
            Target::Colour(_) => false,
            Target::Paint(s) => {
                *s = Some(p);
                true
            }
        }
    }
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
    /// Entries 16-255: base16's and base24's 16-21, a ghostty file's
    /// `palette = N=` for N past 15. Parsed here; whether they are written
    /// is the config layer's.
    pub extended: [Option<Color>; 240],
    /// The cursor, the text under a block cursor, the selection's page and
    /// its text: each a colour or a cell's ([`CellPaint`]).
    pub cursor: Option<CellPaint<Color>>,
    pub cursor_text: Option<CellPaint<Color>>,
    pub selection: Option<CellPaint<Color>>,
    pub selection_text: Option<CellPaint<Color>>,
    /// Under which cursor [`Scheme::cursor_text`] is drawn.
    pub cursor_text_under: CursorText,
    /// What the selection's text answers where [`Scheme::selection_text`]
    /// is unset.
    pub unset_selection_text: SelectionText,
    pub format: Format,
    /// What the file says it is -- tinted's `variant` -- which is checked
    /// against [`Scheme::polarity`] and never used in its place.
    pub declared: Option<ColorScheme>,
}

impl Scheme {
    /// A scheme that sets nothing, read from `format`.
    pub const fn empty(format: Format) -> Scheme {
        Scheme {
            ansi: [None; 16],
            page: None,
            text: None,
            extended: [None; 240],
            cursor: None,
            cursor_text: None,
            selection: None,
            selection_text: None,
            cursor_text_under: CursorText::UnderEveryCursor,
            unset_selection_text: SelectionText::Reverse,
            format,
            declared: None,
        }
    }

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
    /// An X11 colour name in a ghostty file, which ghostty resolves and
    /// baud does not yet; kitty's names are read (by X.Org's `rgb.txt`,
    /// `[[source: xorg-rgb-749182c]]`, and kitty's one addition).
    ColourName,
    /// A dark and a light theme in one file (foot's two sections), where
    /// which half to read is undecided (note AR, O8): the line of the
    /// second.
    Pair,
    /// A Windows Terminal scheme without the `name` it requires.
    NoName,
    /// A Windows Terminal scheme without its sixteen table colours, or a
    /// list of colours that is not sixteen.
    Count,
    /// A C preprocessor construct baud's X resources reader does not run.
    Cpp(Cpp),
    /// A byte that is not UTF-8, in a format whose own text requires it:
    /// TOML, JSON and XML without another encoding declared. The line of
    /// the first such byte.
    NotUtf8,
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
        extended: {
            let mut e = [None; 240];
            e[..6].copy_from_slice(&[b(0x09), b(0x0F), b(0x01), b(0x02), b(0x04), b(0x06)]);
            e
        },
        cursor: None,
        cursor_text: None,
        selection: None,
        selection_text: None,
        cursor_text_under: CursorText::UnderEveryCursor,
        unset_selection_text: SelectionText::Reverse,
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

/// A ghostty theme file, read as ghostty d9840f3 reads one: a line trimmed of
/// spaces, tabs and CR, a comment only where the trimmed line's first byte is
/// `#` ([[source: ghostty-args-d9840f3]] 1450-1466), so a `#` in a value is
/// data; `key = value`, each side trimmed of spaces and tabs and one pair of
/// quotes off the value (1468-1482). The seven colour keys are read; every
/// other key is handed to `ignored` with its line, for the caller to report.
/// The cursor's and the selection's four take `cell-foreground` and
/// `cell-background` too, since ghostty 1.2.0 (`[[source:
/// ghostty-config-8f0dd37]]` 707-716, 845-859, 905-911, and `TerminalColor`'s parse,
/// 5770-5780): the cell's text colour and its page ([`CellPaint`]).
///
/// Lines apply in order, as ghostty applies them: a key set twice takes its
/// last value, and an empty value unsets -- one entry, or with `palette =`
/// all 256 (`[[source: ghostty-args-d9840f3]]` 719). A colour is every form
/// ghostty's `RGB.parse` reads (`[[source: ghostty-color-d9840f3]]` 660-790,
/// with `[[source: ghostty-fraction-d9840f3]]` and Zig's `parseUnsigned`,
/// `[[source: zig-fmt-0-16-0]]` 394-475) but an X11 name, which is refused as
/// [`SchemeErrorKind::ColourName`]: baud resolves names nowhere yet.
pub fn ghostty(text: &[u8], mut ignored: impl FnMut(Line, &[u8])) -> Result<Scheme, SchemeError> {
    let mut s = Scheme::empty(Format::Ghostty);
    let mut line_no = 0u32;
    for raw in text.split(|&b| b == b'\n') {
        line_no += 1;
        let line = Line(line_no);
        let err = |kind| SchemeError { line, kind };
        let body = trim_with(raw, b" \t\r");
        if body.is_empty() || body[0] == b'#' {
            continue;
        }
        let (key, value) = match body.iter().position(|&b| b == b'=') {
            Some(eq) => (
                trim_with(&body[..eq], b" \t"),
                Some(unquote(trim_with(&body[eq + 1..], b" \t"))),
            ),
            None => (body, None),
        };
        let slot = match key {
            b"background" => Target::Colour(&mut s.page),
            b"foreground" => Target::Colour(&mut s.text),
            b"cursor-color" => Target::Paint(&mut s.cursor),
            b"cursor-text" => Target::Paint(&mut s.cursor_text),
            b"selection-background" => Target::Paint(&mut s.selection),
            b"selection-foreground" => Target::Paint(&mut s.selection_text),
            b"palette" => {
                let value = value.ok_or_else(|| err(SchemeErrorKind::Syntax))?;
                if value.is_empty() {
                    s.ansi = [None; 16];
                    s.extended = [None; 240];
                    continue;
                }
                let eq = value
                    .iter()
                    .position(|&b| b == b'=')
                    .ok_or_else(|| err(SchemeErrorKind::Syntax))?;
                let n = palette_index(trim_with(&value[..eq], b" \t"))
                    .ok_or_else(|| err(SchemeErrorKind::PaletteIndex))?;
                let c = colour(&value[eq + 1..]).map_err(err)?;
                match n.checked_sub(16) {
                    None => s.ansi[usize::from(n)] = Some(c),
                    Some(high) => s.extended[usize::from(high)] = Some(c),
                }
                continue;
            }
            _ => {
                ignored(line, key);
                continue;
            }
        };
        let value = value.ok_or_else(|| err(SchemeErrorKind::Syntax))?;
        let cell = match value {
            b"cell-foreground" => Some(CellPaint::Text),
            b"cell-background" => Some(CellPaint::Page),
            _ => None,
        };
        match cell {
            Some(p) if matches!(slot, Target::Paint(_)) => {
                slot.set_cell(p);
            }
            _ if value.is_empty() => slot.set(None),
            _ => slot.set(Some(colour(value).map_err(err)?)),
        }
    }
    Ok(s)
}

/// `text` is UTF-8, or the line of its first byte that is not.
fn utf8(text: &[u8]) -> Result<(), SchemeError> {
    core::str::from_utf8(text).map(drop).map_err(|e| {
        let n = text[..e.valid_up_to()]
            .iter()
            .filter(|&&b| b == b'\n')
            .count();
        SchemeError {
            line: Line(u32::try_from(n).map_or(u32::MAX, |n| n.saturating_add(1))),
            kind: SchemeErrorKind::NotUtf8,
        }
    })
}

/// `t` without the bytes of `set` at either end.
fn trim_with<'a>(t: &'a [u8], set: &[u8]) -> &'a [u8] {
    let start = t.iter().position(|b| !set.contains(b)).unwrap_or(t.len());
    let end = t
        .iter()
        .rposition(|b| !set.contains(b))
        .map_or(start, |e| e + 1);
    &t[start..end]
}

/// One pair of surrounding double quotes off, as ghostty takes it.
fn unquote(v: &[u8]) -> &[u8] {
    match v {
        [b'"', inner @ .., b'"'] => inner,
        _ => v,
    }
}

/// Zig's `parseIntWithSign` for an unsigned result, positive sign
/// (`[[source: zig-fmt-0-16-0]]` 394-464): base 0 detects `0b`, `0o` or `0x`,
/// in either case, only when more than two bytes follow; `_` is skipped
/// between digits and refused first or last; `None` past `max`. The
/// more-than-two-bytes guard changes nothing a `u8` index can observe --
/// `0x`, `0b` and `0o` are refused either way -- and is kept to be Zig's.
fn zig_unsigned(buf: &[u8], base: u32, max: u32) -> Option<u32> {
    let (base, digits) = match buf {
        [b'0', p, _, ..] if base == 0 => match p.to_ascii_lowercase() {
            b'b' => (2, &buf[2..]),
            b'o' => (8, &buf[2..]),
            b'x' => (16, &buf[2..]),
            _ => (10, buf),
        },
        _ if base == 0 => (10, buf),
        _ => (base, buf),
    };
    if matches!(digits.first(), None | Some(b'_')) || digits.last() == Some(&b'_') {
        return None;
    }
    let mut n: u32 = 0;
    for &b in digits {
        if b == b'_' {
            continue;
        }
        n = n * base + (b as char).to_digit(base)?;
        if n > max {
            return None;
        }
    }
    Some(n)
}

/// A palette number as ghostty reads it, with Zig's `parseInt(u8, _, 0)`
/// (`[[source: ghostty-color-d9840f3]]` 62-75, `[[source: zig-fmt-0-16-0]]`
/// 322-331): a `+` may lead, and so may a `-`, which for a `u8` leaves only
/// zero.
fn palette_index(t: &[u8]) -> Option<u8> {
    let n = match t {
        [b'+', rest @ ..] => zig_unsigned(rest, 0, 255)?,
        [b'-', rest @ ..] => match zig_unsigned(rest, 0, 255)? {
            0 => 0,
            _ => return None,
        },
        _ => zig_unsigned(t, 0, 255)?,
    };
    u8::try_from(n).ok()
}

/// ghostty's `RGB.fromHex` (`[[source: ghostty-color-d9840f3]]` 656-677): one
/// to four hex digits by Zig's `parseUnsigned(u16, _, 16)`, `_` separators
/// included, scaled by `v * 255 / max` for the slice's length -- truncated,
/// where XParseColor rounds.
fn from_hex(s: &[u8]) -> Option<Chan> {
    let max = match s.len() {
        1 => 0xf,
        2 => 0xff,
        3 => 0xfff,
        4 => 0xffff,
        _ => return None,
    };
    let v = zig_unsigned(s, 16, 0xffff)?;
    u8::try_from(v * 255 / max).ok().map(Chan::from_u8)
}

/// ghostty's `fraction.parse` then `@intFromFloat(i * 255)`
/// (`[[source: ghostty-fraction-d9840f3]]` 23-80, `[[source:
/// ghostty-color-d9840f3]]` 641-650): an optional sign, decimal digits with
/// an optional point, at least one digit, in [0, 1] with `-0` allowed.
fn from_intensity(s: &[u8]) -> Option<Chan> {
    let (negative, v) = match s {
        [b'+', rest @ ..] => (false, rest),
        [b'-', rest @ ..] => (true, rest),
        _ => (false, s),
    };
    let point = v.iter().position(|&b| b == b'.').unwrap_or(v.len());
    let (int, frac) = (&v[..point], v.get(point + 1..).unwrap_or(&[]));
    if int.len() + frac.len() == 0 || !int.iter().chain(frac).all(u8::is_ascii_digit) {
        return None;
    }
    let mut whole = 0.0f64;
    for &d in int {
        whole = whole * 10.0 + f64::from(d - b'0');
    }
    let (mut f, mut scale) = (0u64, 1u64);
    for &d in frac {
        if scale < 1_000_000_000_000_000 {
            f = f * 10 + u64::from(d - b'0');
            scale *= 10;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let magnitude = whole + f as f64 / scale as f64;
    let i = if negative { -magnitude } else { magnitude };
    if !(0.0..=1.0).contains(&i) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(Chan::from_u8((i * 255.0) as u8))
}

/// A ghostty colour, `RGB.parse`'s branches in its order
/// (`[[source: ghostty-color-d9840f3]]` 704-790): `#` and 3, 6, 9 or 12
/// digits; an X11 name, refused; bare 3 or 6 digits; `rgb:` or `rgbi:`.
fn colour(v: &[u8]) -> Result<Color, SchemeErrorKind> {
    let v = trim_with(v, b" \t");
    let rgb = |[r, g, b]: [Option<Chan>; 3]| match (r, g, b) {
        (Some(r), Some(g), Some(b)) => Ok(Color::Rgb(r, g, b)),
        _ => Err(SchemeErrorKind::Hex(HexError::NotHex)),
    };
    let thirds = |d: &[u8]| {
        let w = d.len() / 3;
        [
            from_hex(&d[..w]),
            from_hex(&d[w..2 * w]),
            from_hex(&d[2 * w..]),
        ]
    };
    if let Some(d) = v.strip_prefix(b"#") {
        return match d.len() {
            0 => Err(SchemeErrorKind::Hex(HexError::Empty)),
            3 | 6 | 9 | 12 => rgb(thirds(d)),
            _ if d.iter().all(u8::is_ascii_hexdigit) => Err(SchemeErrorKind::Hex(HexError::Width)),
            _ => Err(SchemeErrorKind::Hex(HexError::NotHex)),
        };
    }
    let name = v.first().is_some_and(u8::is_ascii_alphabetic)
        && v.iter().all(|&b| b.is_ascii_alphanumeric() || b == b' ');
    if name && !v.iter().all(u8::is_ascii_hexdigit) {
        return Err(SchemeErrorKind::ColourName);
    }
    if matches!(v.len(), 3 | 6) {
        return rgb(thirds(v));
    }
    let rest = match v {
        _ if v.len() < b"rgb:a/a/a".len() => None,
        [b'r', b'g', b'b', b'i', b':', rest @ ..] => Some((true, rest)),
        [b'r', b'g', b'b', b':', rest @ ..] => Some((false, rest)),
        _ => None,
    };
    let Some((intensity, rest)) = rest else {
        return Err(SchemeErrorKind::Hex(match hex(v) {
            Err(e) => e,
            Ok(_) => HexError::NotHex,
        }));
    };
    let channel = if intensity { from_intensity } else { from_hex };
    let mut parts = rest.splitn(3, |&b| b == b'/');
    let (Some(r), Some(g), Some(b)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(SchemeErrorKind::Hex(HexError::NotHex));
    };
    rgb([channel(r), channel(g), channel(b)])
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
