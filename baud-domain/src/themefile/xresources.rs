//! X resources: a theme as `xrdb` loads one (`[[source: xrdb-man-aaf6f3d]]`).
//!
//! **xrdb passes the file through the C preprocessor first**: it runs
//! `cpp -P` with its `-D` symbols (`[[source: xrdb-c-aaf6f3d]]` 1362-1373,
//! 1431-1433), `COLOR` among them on a colour visual (664-672). So a
//! tinted file's `#define base05 #fcfcfc` and `*foreground: base05` set the
//! text to `#fcfcfc`. baud runs the subset every theme in the 1503-file
//! corpus of note AR uses, and refuses the rest by name rather than reading
//! the file as something cpp would not make of it:
//!
//! - **Run**: C comments (`/* */` and `//`, outside quotes) and spliced
//!   lines; object-like `#define` and `#undef`; `#ifdef`, `#ifndef`, `#else`
//!   and `#endif`, with `COLOR` defined as 1. A display's own symbols
//!   (`WIDTH`, `PLANES`, `CLASS`, ...) describe an X server baud is not, and
//!   are not defined.
//! - **Followed**: `#include "f"`, relative to the including file, nested at
//!   most 100 deep -- Xrm's `MAXDBDEPTH` (`[[source: libx11-xrm-b8d5a70]]`
//!   245, 1191-1208, 1621-1650) -- with the macros carried across; one that
//!   cannot be read is passed over and named, as Xrm skips it.
//! - **Refused by name** ([`Cpp`]): `#if`, `#elif`, a macro with arguments
//!   and `#include <f>`. Any other directive is passed over and named, as
//!   cpp prints nothing for it and xrdb reads on (its `pclose` status is
//!   not looked at, `[[source: xrdb-c-aaf6f3d]]` 1448-1453).
//!
//! **Then each line is xrdb's**: `!` starts a comment, a line without `:` is
//! passed over, the name and the value are trimmed of blanks before and the
//! name after (`[[source: xrdb-c-aaf6f3d]]` 330-395), and a later line wins.
//! That is xrdb's rule for one spelling of a name; `xrdb -load` keeps a
//! repeated name at its first line (260-287), so where `*color0` and
//! `*.color0` alternate the X server's database can hold the other value,
//! which note AR lists open.
//! The name is Xrm's: components bound by `.` or `*`, two separators with
//! nothing between them one binding, loose if either is `*`
//! (`[[source: libx11-xrm-b8d5a70]]` 1270-1295). **Only a name every client
//! matches is read** -- one loose component, `*color0` or `*.color0` -- and
//! a program's own entry (`URxvt*color0`) is passed over naming the program
//! (Jev's lean, decisions.jsonl 714/715).
//!
//! **A colour is XParseColor's, read by the parser `OSC 4` reads with**,
//! [`crate::XColor::parse`], so a resource file and an application's escape
//! read one spec alike: `#` and 3, 6, 9 or 12 hex digits, the most
//! significant bits of each 16-bit component; `rgb:` with one to four
//! digits a component, scaled to 16 bits, in any case; `rgbi:` and the CIE
//! and TekHVC forms, converted to sRGB; and the X server's names, case
//! ignored (`[[source: xlib-ch06-b8d5a70]]` 484-695,
//! `[[source: libx11-parsecol-b8d5a70]]` 52-139). A 16-bit component is
//! drawn as its high byte, `v >> 8`, as xterm computes it on TrueColor
//! (`[[source: xterm-misc-9489b20]]` 2725-2733), as libXft does
//! (`[[source: libxft-xftcolor-6b333a3]]` 81-95), and as the X server's
//! `fbResolveColor` gives it on an 8-bit visual, `miResolveColor`'s
//! `(v >> 8) * 257` (`[[source: xserver-fbscreen-98aed9d]]` 135,
//! `[[source: xserver-fbcmap-mi-98aed9d]]` 57-61,
//! `[[source: xserver-micmap-98aed9d]]` 95-112): so `#fff` is `#f0f0f0`.
//! A value the parser refuses -- a name the server lacks, a spec off the
//! manual's grammar such as `rgb:1/2/3/4` -- is passed over and named, as
//! xterm skips a colour XParseColor refuses.
//!
//! The owner took Jev's leans on all four choices (2026-10-17;
//! decisions.jsonl 714/715, none at 0.9 on both asks): the subset 0.72 /
//! 0.72, every client's names 0.14 / 0.61, includes followed 0.76 / 0.60,
//! the high byte 0.84 / 0.86. On the corpus note AR measured -- 575
//! tinted-xresources, 815 iTerm2-Color-Schemes and 113 xcolors files -- this
//! reader gives the same scheme from each file as from clang's
//! `cpp -P -DCOLOR` output of it, 1503 of 1503.

use super::terminals::Skipped;
use super::{Format, Line, Scheme, SchemeError, SchemeErrorKind, rgb};
use crate::{Color, XColor};
use core::marker::PhantomData;

/// A C preprocessor construct baud's subset does not run, which refuses
/// the file at its line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cpp {
    /// `#if`, whose expression baud does not evaluate.
    If,
    /// `#elif`.
    Elif,
    /// A function-like `#define NAME(...)`.
    MacroArguments,
    /// `#include <f>`, whose search path is the system's.
    AngleInclude,
    /// `#include` naming no quoted file.
    IncludeName,
    /// `#define`, `#undef`, `#ifdef` or `#ifndef` without a name.
    NoName,
    /// `#else` or `#endif` with no open `#ifdef`, or a second `#else`.
    Unmatched,
    /// A file that ends inside an `#ifdef`, or inside a `/*` comment.
    Unterminated,
    /// Conditionals nested deeper than [`NEST`].
    Nesting,
    /// More `#define` text than [`MACRO_BYTES`], or more than [`MACROS`]
    /// definitions.
    Macros,
    /// A directive line longer than [`LINE`] bytes.
    Long,
}

/// The bytes of a line baud holds, once its comments are removed and its
/// macros replaced; a resource line past it is read as far as it goes.
pub const LINE: usize = 4096;
/// The bytes of macro names and bodies baud holds.
pub const MACRO_BYTES: usize = 16384;
/// The definitions baud holds, `COLOR` among them.
pub const MACROS: usize = 512;
/// Conditionals open at once.
pub const NEST: usize = 64;
/// Xrm's `MAXDBDEPTH`: files nested past it are not read.
pub const DEPTH: u32 = 100;

/// Whether a write fitted its buffer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fit {
    Whole,
    Cut,
}

/// A bounded byte buffer.
struct Buf {
    b: [u8; LINE],
    len: usize,
    fit: Fit,
}

impl Buf {
    const fn new() -> Buf {
        Buf {
            b: [0; LINE],
            len: 0,
            fit: Fit::Whole,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.fit = Fit::Whole;
    }

    fn push(&mut self, c: u8) {
        match self.b.get_mut(self.len) {
            Some(slot) => {
                *slot = c;
                self.len += 1;
            }
            None => self.fit = Fit::Cut,
        }
    }

    fn bytes(&self) -> &[u8] {
        &self.b[..self.len]
    }
}

/// Whether a space is owed before the next token cpp prints.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Gap {
    None,
    Owed,
}

/// A line as cpp prints it: tokens, one space where any whitespace was.
struct Out {
    buf: Buf,
    gap: Gap,
}

impl Out {
    fn token(&mut self, t: &[u8]) {
        if self.gap == Gap::Owed && self.buf.len > 0 {
            self.buf.push(b' ');
        }
        self.gap = Gap::None;
        for &c in t {
            self.buf.push(c);
        }
    }
}

/// One definition: its name and body in the arena, or removed.
#[derive(Clone, Copy)]
struct Def {
    at: u16,
    name: u16,
    body: u16,
    state: Defined,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Defined {
    Yes,
    Removed,
}

/// The macro table: names and bodies in one arena, definitions in order.
struct Macros {
    arena: [u8; MACRO_BYTES],
    used: usize,
    defs: [Def; MACROS],
    count: usize,
}

impl Macros {
    fn new() -> Macros {
        let mut m = Macros {
            arena: [0; MACRO_BYTES],
            used: 0,
            defs: [Def {
                at: 0,
                name: 0,
                body: 0,
                state: Defined::Removed,
            }; MACROS],
            count: 0,
        };
        // xrdb's -DCOLOR, which cpp defines as 1.
        let _ = m.define(b"COLOR", b"1");
        m
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        (0..self.count).rev().find(|&i| {
            let d = self.defs[i];
            let at = usize::from(d.at);
            d.state == Defined::Yes && &self.arena[at..at + usize::from(d.name)] == name
        })
    }

    fn body(&self, i: usize) -> &[u8] {
        let d = self.defs[i];
        let at = usize::from(d.at) + usize::from(d.name);
        &self.arena[at..at + usize::from(d.body)]
    }

    fn undef(&mut self, name: &[u8]) {
        if let Some(i) = self.find(name) {
            self.defs[i].state = Defined::Removed;
        }
    }

    /// A definition, replacing one of the same name, as cpp does.
    fn define(&mut self, name: &[u8], body: &[u8]) -> Result<(), Cpp> {
        self.undef(name);
        let end = self.used + name.len() + body.len();
        if end > MACRO_BYTES || self.count == MACROS {
            return Err(Cpp::Macros);
        }
        let len = |n: usize| u16::try_from(n).map_err(|_| Cpp::Macros);
        self.defs[self.count] = Def {
            at: len(self.used)?,
            name: len(name.len())?,
            body: len(body.len())?,
            state: Defined::Yes,
        };
        // Byte by byte, here and in `Reader::file`'s copy of a directive:
        // with `copy_from_slice` at those two sites, ingest2's
        // `put_narrow_run`, whose source this does not touch, compiled 64
        // bytes shorter; without them, ingest2's text is byte-identical.
        for (slot, &b) in self.arena[self.used..end]
            .iter_mut()
            .zip(name.iter().chain(body))
        {
            *slot = b;
        }
        self.used = end;
        self.count += 1;
        Ok(())
    }
}

/// One open `#ifdef`: whether its lines are read now, and whether one of
/// its branches has been.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    /// Read, and no `#else` seen.
    Reading,
    /// Not read, and no `#else` seen.
    Waiting,
    /// After `#else`, read.
    ElseReading,
    /// After `#else`, not read.
    ElseSkipping,
}

impl Group {
    fn reads(self) -> bool {
        matches!(self, Group::Reading | Group::ElseReading)
    }
}

/// The conditionals: the groups whose parent is read, and how many are
/// nested inside one that is not, which cpp only counts.
struct Conds {
    open: [Group; NEST],
    depth: usize,
    dead: u32,
}

impl Conds {
    fn reading(&self) -> bool {
        self.dead == 0 && (self.depth == 0 || self.open[self.depth - 1].reads())
    }
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\x0b' | b'\x0c' | b'\r')
}

fn ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'$'
}

fn ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

/// The bytes of a file with its spliced lines (`\` before a newline)
/// joined, as cpp's second translation phase joins them.
struct Src<'t> {
    t: &'t [u8],
    i: usize,
    /// The physical line `i` is on.
    line: u32,
}

impl Src<'_> {
    /// The splices at `i`, skipped.
    fn splice(&mut self) {
        loop {
            match self.t.get(self.i..) {
                Some([b'\\', b'\n', ..]) => self.i += 2,
                Some([b'\\', b'\r', b'\n', ..]) => self.i += 3,
                _ => return,
            }
            self.line += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.splice();
        self.t.get(self.i).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.i += 1;
        if c == b'\n' {
            self.line += 1;
        }
        Some(c)
    }
}

/// The next logical line into `buf`, each comment one space: the line it
/// starts on, or `None` at the end. A `/*` left open is
/// [`Cpp::Unterminated`] at its line.
fn logical(src: &mut Src<'_>, buf: &mut Buf) -> Option<Result<Line, SchemeError>> {
    buf.clear();
    src.peek()?;
    let start = Line(src.line);
    while let Some(c) = src.bump() {
        match c {
            b'\n' => break,
            b'"' | b'\'' => {
                // A literal runs to its closing quote or to the line's end.
                buf.push(c);
                while let Some(d) = src.peek() {
                    if d == b'\n' {
                        break;
                    }
                    src.bump();
                    buf.push(d);
                    if d == b'\\' {
                        if let Some(e) = src.peek().filter(|&e| e != b'\n') {
                            src.bump();
                            buf.push(e);
                        }
                    } else if d == c {
                        break;
                    }
                }
            }
            b'/' if src.peek() == Some(b'*') => {
                let open = Line(src.line);
                src.bump();
                buf.push(b' ');
                let mut star = false;
                loop {
                    match src.bump() {
                        None => {
                            return Some(Err(SchemeError {
                                line: open,
                                kind: SchemeErrorKind::Cpp(Cpp::Unterminated),
                            }));
                        }
                        Some(b'/') if star => break,
                        Some(d) => star = d == b'*',
                    }
                }
            }
            b'/' if src.peek() == Some(b'/') => {
                while src.peek().is_some_and(|d| d != b'\n') {
                    src.bump();
                }
            }
            _ => buf.push(c),
        }
    }
    Some(Ok(start))
}

/// A C pp-number's length: a digit, or `.` and a digit, then digits,
/// letters, `_`, `.` and an exponent's sign.
fn pp_number(t: &[u8]) -> usize {
    let mut n = 1;
    while let Some(&c) = t.get(n) {
        let sign = matches!(c, b'+' | b'-') && matches!(t[n - 1], b'e' | b'E' | b'p' | b'P');
        if !(sign || c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.')) {
            break;
        }
        n += 1;
    }
    n
}

/// The definitions being replaced, so none is replaced inside itself.
struct Active {
    at: [u16; MACROS],
    len: usize,
}

impl Active {
    fn holds(&self, i: usize) -> bool {
        self.at[..self.len].iter().any(|&a| usize::from(a) == i)
    }
}

/// `text` with every defined name replaced and the result rescanned, as
/// cpp prints it.
fn expand(text: &[u8], m: &Macros, active: &mut Active, out: &mut Out) {
    let mut i = 0;
    while i < text.len() {
        let c = text[i];
        if is_space(c) {
            out.gap = Gap::Owed;
            i += 1;
        } else if ident_start(c) {
            let n = text[i..].iter().take_while(|&&d| ident_char(d)).count();
            let word = &text[i..i + n];
            match m.find(word).filter(|&d| !active.holds(d)) {
                Some(d) if active.len < MACROS => {
                    active.at[active.len] = u16::try_from(d).unwrap_or(u16::MAX);
                    active.len += 1;
                    expand(m.body(d), m, active, out);
                    active.len -= 1;
                }
                _ => out.token(word),
            }
            i += n;
        } else if c.is_ascii_digit()
            || (c == b'.' && text.get(i + 1).is_some_and(u8::is_ascii_digit))
        {
            let n = pp_number(&text[i..]);
            out.token(&text[i..i + n]);
            i += n;
        } else if c == b'"' || c == b'\'' {
            let mut n = 1;
            while let Some(&d) = text.get(i + n) {
                n += 1;
                if d == b'\\' {
                    n += 1;
                } else if d == c {
                    break;
                }
            }
            let n = n.min(text.len() - i);
            out.token(&text[i..i + n]);
            i += n;
        } else {
            out.token(&text[i..=i]);
            i += 1;
        }
    }
}

/// What an X resource name names, by its last component.
enum Name<'a> {
    /// One component, bound loosely: every client's.
    AnyClient(&'a [u8]),
    /// A program's entry, or a path through a program's widgets; a single
    /// component bound tightly names a program too, the one so named.
    Program(&'a [u8]),
}

/// An X resource name by Xrm's grammar.
fn name(tag: &[u8]) -> Name<'_> {
    let sep = |c: &u8| matches!(c, b'.' | b'*');
    let last = tag.rsplit(sep).next().unwrap_or(b"");
    // The separators before the last component: loose if any is `*`.
    let run = &tag[..tag.len() - last.len()];
    let run = &run[run.len() - run.iter().rev().take_while(|c| sep(c)).count()..];
    let parts = tag.split(sep).filter(|c| !c.is_empty()).count();
    if parts == 1 && !last.is_empty() && run.contains(&b'*') {
        Name::AnyClient(last)
    } else {
        Name::Program(last)
    }
}

/// Where a resource lands in a [`Scheme`]; `None` for one that is not a
/// colour baud reads.
fn slot<'s>(s: &'s mut Scheme, r: &[u8]) -> Option<&'s mut Option<Color>> {
    match r {
        b"foreground" => Some(&mut s.text),
        b"background" => Some(&mut s.page),
        b"cursorColor" => Some(&mut s.cursor),
        b"highlightColor" => Some(&mut s.selection),
        b"highlightTextColor" => Some(&mut s.selection_text),
        _ => {
            let d = r.strip_prefix(b"color")?;
            if d.is_empty()
                || d.len() > 3
                || (d.len() > 1 && d[0] == b'0')
                || !d.iter().all(u8::is_ascii_digit)
            {
                return None;
            }
            let n = d.iter().fold(0u32, |n, &c| n * 10 + u32::from(c - b'0'));
            let n = u8::try_from(n).ok()?;
            Some(match n.checked_sub(16) {
                None => &mut s.ansi[usize::from(n)],
                Some(high) => &mut s.extended[usize::from(high)],
            })
        }
    }
}

/// An X colour by XParseColor: `Some` a colour, `None` a value X refuses
/// (xterm skips it). A `?` is no colour here: it is a query only in an
/// escape.
fn x_colour(v: &[u8]) -> Option<Color> {
    match XColor::parse(v) {
        XColor::Rgb(r, g, b) => Some(rgb(r.get(), g.get(), b.get())),
        XColor::Query | XColor::Unresolved(_) => None,
    }
}

/// The reader's state across the files it reads.
struct Reader<C, B, F, G> {
    /// The file context and bytes `include` hands back.
    files: PhantomData<fn(&C) -> B>,
    macros: Macros,
    conds: Conds,
    line: Buf,
    out: Out,
    active: Active,
    scheme: Scheme,
    include: F,
    ignored: G,
}

/// Why reading stopped: the error, and the file it is in, `None` for the
/// first.
pub type XresourcesError<C> = (Option<C>, SchemeError);

impl<C, B, F, G> Reader<C, B, F, G>
where
    B: AsRef<[u8]>,
    F: FnMut(&C, &[u8]) -> Option<(C, B)>,
    G: FnMut(&C, Line, &[u8], Skipped),
{
    /// One file: its lines, and the files it includes.
    fn file(&mut self, at: &C, text: &[u8], depth: u32) -> Result<(), XresourcesError<C>> {
        let mut src = Src {
            t: text,
            i: 0,
            line: 1,
        };
        let (open, dead) = (self.conds.depth, self.conds.dead);
        let fail = |line, c| {
            Err((
                None,
                SchemeError {
                    line,
                    kind: SchemeErrorKind::Cpp(c),
                },
            ))
        };
        let mut last = Line(1);
        while let Some(start) = logical(&mut src, &mut self.line) {
            let line = start.map_err(|e| (None, e))?;
            last = line;
            let b = self.line.bytes();
            let body = &b[b.iter().take_while(|&&c| is_space(c)).count()..];
            if body.first() == Some(&b'#') {
                if self.line.fit == Fit::Cut {
                    return fail(line, Cpp::Long);
                }
                let mut d = [0; LINE];
                let n = body.len() - 1;
                for (slot, &b) in d.iter_mut().zip(&body[1..]) {
                    *slot = b;
                }
                self.directive(at, line, &d[..n], depth)?;
                continue;
            }
            if self.conds.reading() {
                self.text(at, line).map_err(|e| (None, e))?;
            }
        }
        if self.conds.depth != open || self.conds.dead != dead {
            return fail(last, Cpp::Unterminated);
        }
        Ok(())
    }

    /// A directive, `d` the bytes after its `#`.
    fn directive(
        &mut self,
        at: &C,
        line: Line,
        d: &[u8],
        depth: u32,
    ) -> Result<(), XresourcesError<C>> {
        let fail = |c| {
            Err((
                None,
                SchemeError {
                    line,
                    kind: SchemeErrorKind::Cpp(c),
                },
            ))
        };
        let d = &d[d.iter().take_while(|&&c| is_space(c)).count()..];
        let n = d.iter().take_while(|&&c| ident_char(c)).count();
        let (word, rest) = d.split_at(n);
        let arg = &rest[rest.iter().take_while(|&&c| is_space(c)).count()..];
        let name = &arg[..arg.iter().take_while(|&&c| ident_char(c)).count()];
        let c = &mut self.conds;
        match word {
            b"ifdef" | b"ifndef" | b"if" if !c.reading() => c.dead += 1,
            b"elif" | b"else" if c.dead > 0 => {}
            b"endif" if c.dead > 0 => c.dead -= 1,
            b"if" => return fail(Cpp::If),
            b"elif" => return fail(Cpp::Elif),
            b"ifdef" | b"ifndef" => {
                if name.is_empty() {
                    return fail(Cpp::NoName);
                }
                if c.depth == NEST {
                    return fail(Cpp::Nesting);
                }
                let defined = self.macros.find(name).is_some();
                c.open[c.depth] = if defined == (word == b"ifdef") {
                    Group::Reading
                } else {
                    Group::Waiting
                };
                c.depth += 1;
            }
            b"else" => {
                let Some(g) = c.depth.checked_sub(1).map(|i| &mut c.open[i]) else {
                    return fail(Cpp::Unmatched);
                };
                *g = match *g {
                    Group::Reading => Group::ElseSkipping,
                    Group::Waiting => Group::ElseReading,
                    Group::ElseReading | Group::ElseSkipping => return fail(Cpp::Unmatched),
                };
            }
            b"endif" => match c.depth.checked_sub(1) {
                Some(d) => c.depth = d,
                None => return fail(Cpp::Unmatched),
            },
            // cpp reads nothing else in a group it skips.
            _ if !c.reading() => {}
            // The null directive.
            b"" if arg.is_empty() => {}
            b"define" => {
                if name.is_empty() {
                    return fail(Cpp::NoName);
                }
                let after = &arg[name.len()..];
                if after.first() == Some(&b'(') {
                    return fail(Cpp::MacroArguments);
                }
                let body = super::trim_with(after, b" \t\x0b\x0c\r");
                self.macros.define(name, body).or_else(fail)?;
            }
            b"undef" => {
                if name.is_empty() {
                    return fail(Cpp::NoName);
                }
                self.macros.undef(name);
            }
            b"include" => {
                let file = match arg {
                    [b'"', rest @ ..] => match rest.iter().position(|&c| c == b'"') {
                        Some(end) => &rest[..end],
                        None => return fail(Cpp::IncludeName),
                    },
                    [b'<', ..] => return fail(Cpp::AngleInclude),
                    _ => return fail(Cpp::IncludeName),
                };
                if depth >= DEPTH {
                    (self.ignored)(at, line, file, Skipped::Nested);
                    return Ok(());
                }
                match (self.include)(at, file) {
                    Some((inner, bytes)) => match self.file(&inner, bytes.as_ref(), depth + 1) {
                        Ok(()) => {}
                        Err((None, e)) => return Err((Some(inner), e)),
                        Err(e) => return Err(e),
                    },
                    None => (self.ignored)(at, line, file, Skipped::NoFile),
                }
            }
            _ => (self.ignored)(at, line, word, Skipped::Directive),
        }
        Ok(())
    }

    /// A line cpp prints: xrdb's `name: value`, its value XParseColor's.
    fn text(&mut self, at: &C, line: Line) -> Result<(), SchemeError> {
        self.out.buf.clear();
        self.out.gap = Gap::None;
        self.active.len = 0;
        expand(
            self.line.bytes(),
            &self.macros,
            &mut self.active,
            &mut self.out,
        );
        let b = self.out.buf.bytes();
        if b.is_empty() || matches!(b[0], b'!' | b'#') {
            return Ok(());
        }
        // xrdb's FindFirst: a `\` hides the byte after it.
        let mut colon = None;
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'\\' => i += 1,
                b':' => {
                    colon = Some(i);
                    break;
                }
                _ => {}
            }
            i += 1;
        }
        let Some(colon) = colon else {
            (self.ignored)(at, line, b, Skipped::Syntax);
            return Ok(());
        };
        let tag = super::trim_with(&b[..colon], b" \t");
        let value = &b[colon + 1..];
        let value = &value[value
            .iter()
            .take_while(|&&c| matches!(c, b' ' | b'\t'))
            .count()..];
        let resource = match name(tag) {
            Name::AnyClient(r) => r,
            Name::Program(r) => {
                let why = if slot(&mut self.scheme, r).is_some() {
                    Skipped::Program
                } else {
                    Skipped::NotAColour
                };
                (self.ignored)(at, line, tag, why);
                return Ok(());
            }
        };
        let fit = self.out.buf.fit;
        let Some(slot) = slot(&mut self.scheme, resource) else {
            let colourish =
                resource.starts_with(b"color") || resource.windows(5).any(|w| w == b"Color");
            let why = if colourish {
                Skipped::Unheld
            } else {
                Skipped::NotAColour
            };
            (self.ignored)(at, line, tag, why);
            return Ok(());
        };
        match (fit, x_colour(value)) {
            (Fit::Whole, Some(c)) => *slot = Some(c),
            _ => (self.ignored)(at, line, tag, Skipped::Refused),
        }
        Ok(())
    }
}

/// An X resources file, `root` naming it to `include` and `ignored`.
///
/// `include` is handed the including file and the name between the quotes,
/// and returns the included file and its bytes, or `None` for one that
/// cannot be read, which is passed over to `ignored` as
/// [`Skipped::NoFile`]. `ignored` is handed every line passed over, with
/// the file it is in.
pub fn xresources<C, B>(
    text: &[u8],
    root: &C,
    include: impl FnMut(&C, &[u8]) -> Option<(C, B)>,
    ignored: impl FnMut(&C, Line, &[u8], Skipped),
) -> Result<Scheme, XresourcesError<C>>
where
    B: AsRef<[u8]>,
{
    let mut r = Reader {
        files: PhantomData,
        macros: Macros::new(),
        conds: Conds {
            open: [Group::Reading; NEST],
            depth: 0,
            dead: 0,
        },
        line: Buf::new(),
        out: Out {
            buf: Buf::new(),
            gap: Gap::None,
        },
        active: Active {
            at: [0; MACROS],
            len: 0,
        },
        scheme: Scheme::empty(Format::Xresources),
        include,
        ignored,
    };
    r.file(root, text, 0)?;
    Ok(r.scheme)
}
