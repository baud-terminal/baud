//! Which format a theme file is in, read from its content (note AR,
//! `docs/research/notes/ar-theme-formats.md`, section 2).
//!
//! **One pass gives one verdict, and only that verdict's reader runs**:
//! baud never tries reader after reader and keeps whichever parses, which
//! makes the format depend on the order the readers are tried in. A verdict
//! comes from the file's own marks, in this order:
//!
//! 1. **A list of sixteen `#rrggbb` lines**, before any `#` line is read as
//!    a comment -- under ghostty's grammar every such line is one, and the
//!    file would read as empty.
//! 2. **The document's shape**: `<` is XML, `{` is JSON, and a `[section]`
//!    line anywhere is INI or TOML, whose section names decide.
//! 3. **The line grammar**, which must be the same on every line: `key: v`
//!    (YAML), `key = v`, `*.resource: v` (X resources) or `key v`. Two
//!    grammars in one file are [`Verdict::Mixed`], never a majority.
//! 4. **A key only one format has**, inside that grammar: `system`,
//!    `baseXX`, `scheme.system`, `color_NN` and `colors.primary` in YAML;
//!    `palette`, `regularN` and `colors.` in `key = v`; and so on.
//!
//! No text governs choosing between these families (note AR, section 4,
//! O1); inside tinted's own it is `[[source: tinted-builder]]` 30, which
//! makes the builder "use the provided colors in the palette to determine"
//! base16 or base24, and that choice stays the base16 reader's.
//!
//! Two refinements of note AR's design, each from the format's own text.
//! A `key = v` line with no `=` (a bare word) belongs with the `key = v`
//! lines, because ghostty takes a line without `=` as a key with no value
//! (`[[source: ghostty-args-d9840f3]]` 1468, the `=` is looked for and its
//! absence is not an error). And foot's colour keys outside a section are
//! not a foot theme: "The default section is usually unnamed"
//! (`[[source: foot-2705e36-foot-ini-5]]` 9-11), so they are `[main]`'s,
//! where foot has no colours.

use super::{Line, slot_of, trim_with, yaml_value};

/// A theme file's format, by what its content shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// base16 or base24 YAML (`[[source: tinted-builder]]`); which of the
    /// two is decided by the reader, as the builder text says.
    Tinted,
    /// A ghostty theme: a ghostty config file (`[[source:
    /// ghostty-config-d9840f3]]` 571-573).
    Ghostty,
    /// Sixteen `#rrggbb` lines, ANSI 0 to 15, as pywal's `colors` file.
    PlainList,
    /// kitty's `name value` config.
    Kitty,
    /// A foot `.ini` with a `[colors-dark]` or `[colors-light]` section.
    Foot,
    /// alacritty's TOML config, `[colors.*]`.
    Alacritty,
    /// alacritty's YAML config, before 0.13: `colors:` with `primary:`.
    AlacrittyYaml,
    /// An iTerm2 colour preset, an XML plist with `Ansi 0 Color`.
    ITermPlist,
    /// A Windows Terminal scheme: a JSON object with `purple`.
    WindowsTerminal,
    /// X resources: `*.color0: ...`.
    Xresources,
    /// tinted8, whose `scheme:` holds `system: "tinted8"`. Detected and
    /// refused (Jev, O6, decisions.jsonl 664/665).
    Tinted8,
    /// Gogh's and iTerm2-Color-Schemes' YAML, `color_01` to `color_16`.
    ColorNN,
    /// A wezterm TOML scheme, `[colors]` with `ansi` and `brights`.
    Wezterm,
}

/// The line that decided, and the key on it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark<'a> {
    pub line: Line,
    pub key: &'a [u8],
}

/// A line's grammar, from the byte after its key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grammar {
    /// `key: value`, and a YAML sequence entry `- ...`.
    Colon,
    /// `key = value`.
    Equals,
    /// `name value`.
    Space,
    /// `*.resource: value`: a key holding `*` or `.` before its `:`.
    Resource,
    /// A key and nothing after it.
    Bare,
    /// None of these: a line no theme grammar has.
    Other,
}

/// One side of a [`Verdict::Mixed`]: a grammar, or a format's own key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Grammar(Grammar),
    Kind(Kind),
}

/// A file whose shape baud saw but no format it reads has.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unknown<'a> {
    /// Only `#rrggbb` lines, and not sixteen of them.
    HexLines(Line),
    /// XML without `Ansi 0 Color`.
    Xml,
    /// JSON without the keys of a Windows Terminal scheme.
    Json,
    /// A Windows Terminal `settings.json`, holding several schemes; which
    /// one is the user's to say.
    JsonSchemes(Mark<'a>),
    /// INI or TOML sections no theme format uses; the first one.
    Sections(Mark<'a>),
    /// foot's `[colors]` or `[colors2]`, removed in foot 1.28.0.
    FootRemovedSection(Mark<'a>),
    /// foot's colour keys with no section around them.
    FootOutsideSection(Mark<'a>),
    /// YAML holding none of the keys a format has.
    Yaml(Mark<'a>),
    /// tinted's nested `scheme.system` naming a system other than
    /// tinted8.
    TintedSystem(Mark<'a>),
    /// tinted's slot names in `key = value` syntax (Jev, O10, refused: tinted
    /// is YAML, `[[source: tinted-builder]]` 30, 43).
    TintedSlotsInEquals(Mark<'a>),
    /// Lines of one grammar with no colour key any format reads.
    Lines(Grammar, Mark<'a>),
}

/// Bytes that are not a text theme file.
///
/// Bytes that are not UTF-8 are not this: the detector reads bytes, as
/// the formats' own programs do, and a format whose own text requires
/// UTF-8 refuses them in its reader.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotText {
    /// iTerm2's binary plist, which baud reads only as XML.
    BinaryPlist,
}

/// What a theme file is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict<'a> {
    Is(Kind, Mark<'a>),
    /// Nothing but blank lines and comments.
    Empty,
    /// Two grammars, or two formats' keys, in one file: the first line of
    /// each.
    Mixed(Shape, Mark<'a>, Shape, Mark<'a>),
    Unknown(Unknown<'a>),
    NotText(NotText),
}

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// The file as every reader sees it: without a leading UTF-8 BOM.
pub fn body(text: &[u8]) -> &[u8] {
    text.strip_prefix(BOM).unwrap_or(text)
}

/// Lines, 1-based, each without its CR.
fn lines(text: &[u8]) -> impl Iterator<Item = (Line, &[u8])> {
    text.split(|&b| b == b'\n')
        .zip(1u32..)
        .map(|(raw, n)| (Line(n), raw.strip_suffix(b"\r").unwrap_or(raw)))
}

fn trim(t: &[u8]) -> &[u8] {
    trim_with(t, b" \t")
}

/// What `text` is. Total: every byte string gives one verdict.
pub fn classify(text: &[u8]) -> Verdict<'_> {
    if text.starts_with(b"bplist00") {
        return Verdict::NotText(NotText::BinaryPlist);
    }
    let text = body(text);
    if let Some(v) = plain_list(text) {
        return v;
    }
    let first = lines(text)
        .map(|(_, raw)| trim(raw))
        .find(|b| !b.is_empty() && !matches!(b[0], b'#' | b'!' | b';'));
    let Some(first) = first else {
        return Verdict::Empty;
    };
    match first[0] {
        b'<' => return xml(text),
        b'{' => return json(text),
        b'[' if !is_header(first) => return json(text),
        _ => {}
    }
    if lines(text).any(|(_, raw)| is_header(trim(raw))) {
        return sections(text);
    }
    grammar(text)
}

/// `#rrggbb` and nothing else on the line.
fn is_hex_line(b: &[u8]) -> bool {
    matches!(b, [b'#', rest @ ..] if rest.len() == 6 && rest.iter().all(u8::is_ascii_hexdigit))
}

/// Step 1: a file of only `#rrggbb` lines.
fn plain_list(text: &[u8]) -> Option<Verdict<'_>> {
    let mut first = None;
    let mut n = 0u32;
    for (line, raw) in lines(text) {
        let b = trim(raw);
        if b.is_empty() {
            continue;
        }
        if !is_hex_line(b) {
            return None;
        }
        n += 1;
        first.get_or_insert(Mark { line, key: b });
    }
    let first = first?;
    Some(if n == 16 {
        Verdict::Is(Kind::PlainList, first)
    } else {
        Verdict::Unknown(Unknown::HexLines(Line(n)))
    })
}

/// The line `needle` first starts on.
fn find<'a>(text: &'a [u8], needle: &'static [u8]) -> Option<Mark<'a>> {
    let at = text.windows(needle.len()).position(|w| w == needle)?;
    let n = text[..at].iter().filter(|&&b| b == b'\n').count();
    Some(Mark {
        line: Line(u32::try_from(n).map_or(u32::MAX, |n| n.saturating_add(1))),
        key: &text[at..at + needle.len()],
    })
}

fn xml(text: &[u8]) -> Verdict<'_> {
    match find(text, b"<key>Ansi 0 Color</key>") {
        Some(m) => Verdict::Is(Kind::ITermPlist, m),
        None => Verdict::Unknown(Unknown::Xml),
    }
}

fn json(text: &[u8]) -> Verdict<'_> {
    if let Some(m) = find(text, b"\"schemes\"") {
        return Verdict::Unknown(Unknown::JsonSchemes(m));
    }
    match (
        find(text, b"\"purple\""),
        find(text, b"\"background\""),
        find(text, b"\"foreground\""),
    ) {
        (Some(m), Some(_), Some(_)) => Verdict::Is(Kind::WindowsTerminal, m),
        _ => Verdict::Unknown(Unknown::Json),
    }
}

/// A TOML or INI section header: `[name]` or `[[name]]`, a comment after
/// it allowed.
fn is_header(b: &[u8]) -> bool {
    let b = match b.iter().position(|&c| c == b'#') {
        Some(h) => trim(&b[..h]),
        None => b,
    };
    let inner = match b {
        [b'[', b'[', inner @ .., b']', b']'] | [b'[', inner @ .., b']'] => inner,
        _ => return false,
    };
    !inner.is_empty()
        && inner
            .iter()
            .all(|&c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'"' | b'-'))
}

/// The first of up to four marks, by line, and whether more than one kind
/// was seen: the decision every family makes.
#[derive(Clone, Copy)]
struct Seen<'a, const N: usize>([Option<(Shape, Mark<'a>)>; N]);

impl<'a, const N: usize> Seen<'a, N> {
    fn new() -> Self {
        Seen([None; N])
    }

    fn note(&mut self, slot: usize, shape: Shape, mark: Mark<'a>) {
        self.0[slot].get_or_insert((shape, mark));
    }

    fn get(&self, slot: usize) -> Option<Mark<'a>> {
        self.0[slot].map(|(_, m)| m)
    }

    /// The two earliest marks, when more than one slot holds one.
    fn mixed(&self) -> Option<Verdict<'a>> {
        let mut best: [Option<(Shape, Mark<'a>)>; 2] = [None, None];
        for &m in self.0.iter().flatten() {
            let at = m.1.line.0;
            best = match best {
                [None, _] => [Some(m), None],
                [Some(a), None] if at < a.1.line.0 => [Some(m), Some(a)],
                [Some(a), None] => [Some(a), Some(m)],
                [Some(a), Some(_)] if at < a.1.line.0 => [Some(m), Some(a)],
                [Some(a), Some(b)] if at < b.1.line.0 => [Some(a), Some(m)],
                kept => kept,
            };
        }
        match best {
            [Some(a), Some(b)] => Some(Verdict::Mixed(a.0, a.1, b.0, b.1)),
            _ => None,
        }
    }
}

/// The section header's name, without brackets or a comment.
fn header_name(b: &[u8]) -> &[u8] {
    let b = match b.iter().position(|&c| c == b'#') {
        Some(h) => trim(&b[..h]),
        None => b,
    };
    let b = b.strip_prefix(b"[").unwrap_or(b);
    let b = b.strip_prefix(b"[").unwrap_or(b);
    let b = b.strip_suffix(b"]").unwrap_or(b);
    b.strip_suffix(b"]").unwrap_or(b)
}

/// alacritty's `[colors]` children (`[[source: alacritty-0c405d5-man-5]]`
/// 297-475).
const ALACRITTY_COLORS: [&[u8]; 14] = [
    b"primary",
    b"cursor",
    b"vi_mode_cursor",
    b"search",
    b"hints",
    b"line_indicator",
    b"footer_bar",
    b"selection",
    b"normal",
    b"bright",
    b"dim",
    b"indexed_colors",
    b"transparent_background_colors",
    b"draw_bold_text_with_bright_colors",
];

/// wezterm's `[colors]` keys that alacritty does not have (note AR,
/// peers.md section 4).
const WEZTERM_COLORS: [&[u8]; 8] = [
    b"ansi",
    b"brights",
    b"indexed",
    b"tab_bar",
    b"cursor_bg",
    b"cursor_fg",
    b"selection_bg",
    b"selection_fg",
];

/// foot's colour keys: `regular0`-`7`, `bright0`-`7`, `dim0`-`7` and a
/// palette number `0`-`255` (`[[source: foot-2705e36-foot-ini-5]]`
/// 1053-1105).
fn foot_key(k: &[u8]) -> bool {
    let digit07 = |d: &[u8]| matches!(d, [b'0'..=b'7']);
    match k {
        [b'r', b'e', b'g', b'u', b'l', b'a', b'r', d @ ..]
        | [b'b', b'r', b'i', b'g', b'h', b't', d @ ..]
        | [b'd', b'i', b'm', d @ ..] => digit07(d),
        _ => palette_number(k),
    }
}

/// A palette number written in decimal without a leading zero, up to
/// `max`.
fn number_to(k: &[u8], max: u32) -> bool {
    !k.is_empty()
        && k.len() <= 3
        && k.iter().all(u8::is_ascii_digit)
        && (k.len() == 1 || k[0] != b'0')
        && k.iter().fold(0u32, |n, &d| n * 10 + u32::from(d - b'0')) <= max
}

/// `0` to `255`.
fn palette_number(k: &[u8]) -> bool {
    number_to(k, 255)
}

/// The key token at the start of a line: `[A-Za-z0-9_.*?-]`.
fn key_token(b: &[u8]) -> &[u8] {
    let n = b
        .iter()
        .take_while(|&&c| {
            c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'*' | b'?' | b'-')
        })
        .count();
    &b[..n]
}

/// Step 2 for INI and TOML: the section names decide.
fn sections(text: &[u8]) -> Verdict<'_> {
    const FOOT: usize = 0;
    const REMOVED: usize = 1;
    const ALACRITTY: usize = 2;
    const WEZTERM: usize = 3;
    let mut seen = Seen::<4>::new();
    let mut other: Option<Mark<'_>> = None;
    // Inside foot's old `[colors]`/`[colors2]` or a TOML `[colors]`.
    let mut in_colors = false;
    for (line, raw) in lines(text) {
        let b = trim(raw);
        if b.is_empty() || matches!(b[0], b'#' | b';') {
            continue;
        }
        if is_header(b) {
            let name = header_name(b);
            in_colors = false;
            let mark = Mark { line, key: name };
            if name == b"colors-dark" || name == b"colors-light" {
                seen.note(FOOT, Shape::Kind(Kind::Foot), mark);
            } else if name == b"colors" || name == b"colors2" {
                in_colors = true;
            } else if let Some(sub) = name.strip_prefix(b"colors.") {
                let sub = sub.split(|&c| c == b'.').next().unwrap_or(sub);
                if ALACRITTY_COLORS.contains(&sub) {
                    seen.note(ALACRITTY, Shape::Kind(Kind::Alacritty), mark);
                } else if WEZTERM_COLORS.contains(&sub) {
                    seen.note(WEZTERM, Shape::Kind(Kind::Wezterm), mark);
                }
            } else {
                other.get_or_insert(mark);
            }
            continue;
        }
        if !in_colors {
            continue;
        }
        let key = key_token(b);
        let first = key.split(|&c| c == b'.').next().unwrap_or(key);
        let mark = Mark { line, key };
        if foot_key(key) {
            seen.note(REMOVED, Shape::Kind(Kind::Foot), mark);
        } else if ALACRITTY_COLORS.contains(&first) {
            seen.note(ALACRITTY, Shape::Kind(Kind::Alacritty), mark);
        } else if WEZTERM_COLORS.contains(&first) {
            seen.note(WEZTERM, Shape::Kind(Kind::Wezterm), mark);
        }
    }
    if let Some(v) = seen.mixed() {
        return v;
    }
    if let Some(m) = seen.get(FOOT) {
        return Verdict::Is(Kind::Foot, m);
    }
    if let Some(m) = seen.get(REMOVED) {
        return Verdict::Unknown(Unknown::FootRemovedSection(m));
    }
    if let Some(m) = seen.get(ALACRITTY) {
        return Verdict::Is(Kind::Alacritty, m);
    }
    if let Some(m) = seen.get(WEZTERM) {
        return Verdict::Is(Kind::Wezterm, m);
    }
    let first = other.or_else(|| {
        lines(text)
            .find(|(_, raw)| is_header(trim(raw)))
            .map(|(line, raw)| Mark {
                line,
                key: header_name(trim(raw)),
            })
    });
    match first {
        Some(m) => Verdict::Unknown(Unknown::Sections(m)),
        None => Verdict::Unknown(Unknown::Json),
    }
}

/// A line's grammar and its key.
fn class(b: &[u8]) -> (Grammar, &[u8]) {
    if b == b"-" || b.starts_with(b"- ") {
        return (Grammar::Colon, &b[..1]);
    }
    let key = key_token(b);
    if key.is_empty() {
        return (Grammar::Other, b);
    }
    let rest = &b[key.len()..];
    let after = trim_with(rest, b" \t");
    let spaced = after.len() < rest.len();
    match after.first() {
        None if spaced => (Grammar::Other, key),
        None => (Grammar::Bare, key),
        Some(b'=') => (Grammar::Equals, key),
        Some(b':') if key.iter().any(|&c| matches!(c, b'*' | b'.' | b'?')) => {
            (Grammar::Resource, key)
        }
        Some(b':') => (Grammar::Colon, key),
        Some(_) if spaced => (Grammar::Space, key),
        Some(_) => (Grammar::Other, key),
    }
}

/// The lines a grammar's reader reads, each with its indentation and
/// grammar: blank lines and `#` and `!` comments are skipped, and so are a
/// YAML block scalar's lines (`description: >`) and a TOML array's
/// continuation lines, which carry no key of their own.
fn significant(text: &[u8]) -> impl Iterator<Item = (Line, usize, Grammar, &[u8], &[u8])> {
    let mut block: Option<usize> = None;
    let mut depth = 0i32;
    lines(text).filter_map(move |(line, raw)| {
        let indent = raw.iter().take_while(|&&c| c == b' ' || c == b'\t').count();
        let b = trim(&raw[indent..]);
        if let Some(k) = block {
            if b.is_empty() || indent > k {
                return None;
            }
            block = None;
        }
        if depth > 0 {
            depth += brackets(b);
            return None;
        }
        if b.is_empty() || matches!(b[0], b'#' | b'!') || b == b"---" {
            return None;
        }
        let (g, key) = class(b);
        let value = trim(&b[key.len()..]);
        match g {
            Grammar::Colon => {
                let v = trim(value.strip_prefix(b":").unwrap_or(value));
                if matches!(v.first(), Some(b'|' | b'>')) {
                    block = Some(indent);
                }
            }
            Grammar::Equals => {
                let v = trim(value.strip_prefix(b"=").unwrap_or(value));
                if v.first() == Some(&b'[') {
                    depth = brackets(v).max(0);
                }
            }
            _ => {}
        }
        Some((line, indent, g, key, value))
    })
}

/// `[` less `]` on a line: a TOML array still open after it.
fn brackets(b: &[u8]) -> i32 {
    b.iter().fold(0, |n, &c| match c {
        b'[' => n + 1,
        b']' => n - 1,
        _ => n,
    })
}

/// Steps 3 and 4 for line files: one grammar, then its keys.
fn grammar(text: &[u8]) -> Verdict<'_> {
    let mut seen = Seen::<6>::new();
    let slot = |g: Grammar| match g {
        Grammar::Colon => 0,
        Grammar::Equals => 1,
        Grammar::Space => 2,
        Grammar::Resource => 3,
        Grammar::Bare => 4,
        Grammar::Other => 5,
    };
    for (line, _, g, key, _) in significant(text) {
        seen.note(slot(g), Shape::Grammar(g), Mark { line, key });
    }
    // A bare key is ghostty's (args.zig 1468): one family with `key = v`.
    if seen.get(slot(Grammar::Equals)).is_some() {
        seen.0[slot(Grammar::Bare)] = None;
    }
    if let Some(v) = seen.mixed() {
        return v;
    }
    if let Some((Shape::Grammar(g), mark)) = seen.0.iter().flatten().next().copied() {
        return match g {
            Grammar::Colon => colon(text),
            Grammar::Equals => equals(text),
            Grammar::Space => space(text, mark),
            Grammar::Resource => resource(text, mark),
            Grammar::Bare | Grammar::Other => Verdict::Unknown(Unknown::Lines(g, mark)),
        };
    }
    Verdict::Empty
}

/// `color_01` to `color_16`, the 1-based keys of iTerm2-Color-Schemes'
/// YAML (`[[source: iterm2cs-gen-py]]` 390-405).
fn color_nn(k: &[u8]) -> bool {
    matches!(k, [b'c', b'o', b'l', b'o', b'r', b'_', a @ b'0'..=b'1', b @ b'0'..=b'9']
        if matches!((a, b), (b'0', b'1'..=b'9') | (b'1', b'0'..=b'6')))
}

/// YAML: tinted's keys, tinted8's nested system, `color_NN`, alacritty's
/// `colors:`.
fn colon(text: &[u8]) -> Verdict<'_> {
    const SYSTEM: usize = 0;
    const SLOT: usize = 1;
    const EIGHT: usize = 2;
    const NN: usize = 3;
    const ALACRITTY: usize = 4;
    const OTHER_SYSTEM: usize = 5;
    let mut seen = Seen::<6>::new();
    let mut parent: &[u8] = b"";
    let mut first = None;
    for (line, indent, g, key, rest) in significant(text) {
        if g != Grammar::Colon {
            continue;
        }
        let mark = Mark { line, key };
        first.get_or_insert(mark);
        let value = yaml_value(rest.strip_prefix(b":").unwrap_or(rest)).unwrap_or(b"");
        if indent == 0 {
            parent = if value.is_empty() { key } else { b"" };
            if key == b"system" {
                seen.note(SYSTEM, Shape::Kind(Kind::Tinted), mark);
            } else if slot_of(key).is_some() {
                seen.note(SLOT, Shape::Kind(Kind::Tinted), mark);
            } else if color_nn(key) {
                seen.note(NN, Shape::Kind(Kind::ColorNN), mark);
            }
            continue;
        }
        match parent {
            b"scheme" if key == b"system" => {
                if value == b"tinted8" {
                    seen.note(EIGHT, Shape::Kind(Kind::Tinted8), mark);
                } else {
                    seen.note(OTHER_SYSTEM, Shape::Kind(Kind::Tinted8), mark);
                }
            }
            b"palette" if slot_of(key).is_some() => {
                seen.note(SLOT, Shape::Kind(Kind::Tinted), mark);
            }
            b"colors" if matches!(key, b"primary" | b"normal" | b"bright") => {
                seen.note(ALACRITTY, Shape::Kind(Kind::AlacrittyYaml), mark);
            }
            _ => {}
        }
    }
    // tinted's own two marks are one format: `system` names it, else the
    // first slot does.
    let tinted = seen.get(SYSTEM).or(seen.get(SLOT));
    seen.0[SYSTEM] = None;
    if let Some(m) = tinted {
        seen.0[SLOT] = Some((Shape::Kind(Kind::Tinted), m));
    }
    if let Some(v) = seen.mixed() {
        return v;
    }
    if let Some(m) = tinted {
        return Verdict::Is(Kind::Tinted, m);
    }
    if let Some(m) = seen.get(EIGHT) {
        return Verdict::Is(Kind::Tinted8, m);
    }
    if let Some(m) = seen.get(OTHER_SYSTEM) {
        return Verdict::Unknown(Unknown::TintedSystem(m));
    }
    if let Some(m) = seen.get(NN) {
        return Verdict::Is(Kind::ColorNN, m);
    }
    if let Some(m) = seen.get(ALACRITTY) {
        return Verdict::Is(Kind::AlacrittyYaml, m);
    }
    match first {
        Some(m) => Verdict::Unknown(Unknown::Yaml(m)),
        None => Verdict::Empty,
    }
}

/// ghostty's colour keys (`[[source: ghostty-config-d9840f3]]` 711-906).
const GHOSTTY_COLOURS: [&[u8]; 7] = [
    b"palette",
    b"background",
    b"foreground",
    b"cursor-color",
    b"cursor-text",
    b"selection-background",
    b"selection-foreground",
];

/// `key = value`: ghostty, unless alacritty's dotted `colors.` keys, foot's
/// keys outside a section or tinted's slots say otherwise. `background`,
/// `foreground` and the two `selection-` keys are foot's names too, so only
/// `palette` and the cursor keys are ghostty's alone.
fn equals(text: &[u8]) -> Verdict<'_> {
    const GHOSTTY: usize = 0;
    const ALACRITTY: usize = 1;
    const FOOT: usize = 2;
    let mut seen = Seen::<3>::new();
    let (mut shared, mut tinted, mut first) = (None, None, None);
    for (line, _, g, key, _) in significant(text) {
        let mark = Mark { line, key };
        first.get_or_insert(mark);
        if g != Grammar::Equals {
            continue;
        }
        if matches!(key, b"palette" | b"cursor-color" | b"cursor-text") {
            seen.note(GHOSTTY, Shape::Kind(Kind::Ghostty), mark);
        } else if GHOSTTY_COLOURS.contains(&key) {
            shared.get_or_insert(mark);
        } else if key.starts_with(b"colors.") {
            seen.note(ALACRITTY, Shape::Kind(Kind::Alacritty), mark);
        } else if foot_key(key) {
            seen.note(FOOT, Shape::Kind(Kind::Foot), mark);
        } else if slot_of(key).is_some() {
            tinted.get_or_insert(mark);
        }
    }
    // A shared key beside alacritty's is a ghostty line in an alacritty
    // file: two formats.
    if let (Some(m), Some(_)) = (shared, seen.get(ALACRITTY)) {
        seen.note(GHOSTTY, Shape::Kind(Kind::Ghostty), m);
    }
    if let Some(v) = seen.mixed() {
        return v;
    }
    if let Some(m) = seen.get(ALACRITTY) {
        return Verdict::Is(Kind::Alacritty, m);
    }
    if let Some(m) = seen.get(FOOT) {
        return Verdict::Unknown(Unknown::FootOutsideSection(m));
    }
    // Beside ghostty's own keys a slot name is a key ghostty does not
    // have, and its reader reports it; alone, it is O10's refusal.
    match (seen.get(GHOSTTY).or(shared), tinted, first) {
        (Some(m), _, _) => Verdict::Is(Kind::Ghostty, m),
        (None, Some(m), _) => Verdict::Unknown(Unknown::TintedSlotsInEquals(m)),
        // Keys and no colour: a ghostty file that sets none (Jev, O12).
        (None, None, Some(m)) => Verdict::Is(Kind::Ghostty, m),
        (None, None, None) => Verdict::Empty,
    }
}

/// kitty's colour keys (`[[source: kitty-options-definition]]` 353, 369,
/// 2470, 2641, 2655, 2688).
fn kitty_key(k: &[u8]) -> bool {
    matches!(
        k,
        b"foreground"
            | b"background"
            | b"cursor"
            | b"cursor_text_color"
            | b"selection_foreground"
            | b"selection_background"
    ) || k.strip_prefix(b"color").is_some_and(palette_number)
}

fn space<'a>(text: &'a [u8], first: Mark<'a>) -> Verdict<'a> {
    significant(text)
        .find(|&(_, _, _, key, _)| kitty_key(key))
        .map_or(
            Verdict::Unknown(Unknown::Lines(Grammar::Space, first)),
            |(line, _, _, key, _)| Verdict::Is(Kind::Kitty, Mark { line, key }),
        )
}

/// xterm's colour resources (`[[source: xterm-man]]` 2663, 2912-2940,
/// 3233, 4014, 4075, 4180), the last component of a resource name.
fn xterm_resource(k: &[u8]) -> bool {
    let last = k.rsplit(|&c| c == b'.' || c == b'*').next().unwrap_or(k);
    matches!(
        last,
        b"foreground" | b"background" | b"cursorColor" | b"highlightColor" | b"highlightTextColor"
    ) || last
        .strip_prefix(b"color")
        .is_some_and(|n| number_to(n, 15))
}

fn resource<'a>(text: &'a [u8], first: Mark<'a>) -> Verdict<'a> {
    significant(text)
        .find(|&(_, _, _, key, _)| xterm_resource(key))
        .map_or(
            Verdict::Unknown(Unknown::Lines(Grammar::Resource, first)),
            |(line, _, _, key, _)| Verdict::Is(Kind::Xresources, Mark { line, key }),
        )
}
