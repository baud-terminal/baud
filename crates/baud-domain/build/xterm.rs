//! xterm's `charsets.h` (`[[source: xterm-charsets]]`), which spells each
//! character set as a C macro: a `#define` and the lines its trailing
//! backslashes continue, one `MAP`, `UNI` or `XXX` entry a line. This
//! module finds a macro's body and reads the pieces every reader of it
//! shares; what a body may hold is each reader's own, in
//! `build/dec_sets.rs` and `build/nrcs.rs`, so neither widens the other.

use baud_tablegen::{Error, Input, Kind, Pos, Source};

pub const CHARSETS: Input = Input {
    file: "xterm-charsets.h",
    key: "xterm-charsets",
    header: "/*",
};

/// Line 2, the pinned revision's RCS identifier: line 1 is `/*`, which any
/// C file may start with, so the identifier is the header's control.
pub const ID: &str = " * $XTermId: charsets.h,v 1.36 2025/04/08 22:42:06 tom Exp $";

/// [`CHARSETS`], refused unless line 2 is [`ID`].
pub fn load(read: &dyn Fn(&str) -> std::io::Result<String>) -> Result<Source, Error> {
    let src = Source::load(&CHARSETS, read)?;
    if src.line(2) != ID {
        return Err(src
            .error(
                Kind::Header,
                format!("line 2 is not {ID:?}: is this the pinned file [xterm-charsets]?"),
            )
            .at(Pos { line: 2, col: 1 }, src.line(2)));
    }
    Ok(src)
}

/// One line of a macro's body: its number, from 1, and the line as written.
#[derive(Clone, Copy, Debug)]
pub struct Line<'a> {
    pub n: usize,
    pub raw: &'a str,
}

impl<'a> Line<'a> {
    /// What the line holds: trimmed, the backslash that continues it
    /// dropped, and trimmed again.
    pub fn text(&self) -> &'a str {
        let t = self.raw.trim();
        t.strip_suffix('\\').unwrap_or(t).trim()
    }

    /// A failure at this line, at the column its text starts.
    pub fn error(&self, path: &str, kind: Kind, message: impl Into<String>) -> Error {
        let col = self.raw.chars().take_while(|c| c.is_whitespace()).count() + 1;
        Error::new(kind, path, message).at(Pos { line: self.n, col }, self.raw)
    }
}

/// Each `#define <name>(` whose line a backslash continues, with its
/// line and its body: every line a trailing backslash continues, the last
/// one included. A `#define` that continues nothing has no body and is not
/// listed: `charsets.h` defines Greek, Hebrew, Turkish and the JIS sets a
/// second time, as `/* nothing */`, for a build without wide characters.
pub fn bodies<'a>(src: &'a Source, name: &str) -> Vec<(usize, Vec<Line<'a>>)> {
    let lines: Vec<&str> = src.text().lines().collect();
    let head = format!("#define {name}(");
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.starts_with(&head) {
            continue;
        }
        let mut body = Vec::new();
        let mut j = i;
        while lines[j].trim_end().ends_with('\\') && j + 1 < lines.len() {
            j += 1;
            body.push(Line {
                n: j + 1,
                raw: lines[j],
            });
        }
        if !body.is_empty() {
            out.push((i + 1, body));
        }
    }
    out
}

/// A cursor over an entry, which the readers match whole.
pub struct Cur<'a> {
    pub rest: &'a str,
}

/// Which hexadecimal digits a field allows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Digits {
    /// `0-9` and `A-F`.
    Upper,
    /// `0-9`, `A-F` and `a-f`.
    Either,
}

impl<'a> Cur<'a> {
    pub fn new(text: &'a str) -> Cur<'a> {
        Cur { rest: text }
    }

    /// Takes `p`, or nothing.
    pub fn lit(&mut self, p: &str) -> bool {
        match self.rest.strip_prefix(p) {
            Some(r) => {
                self.rest = r;
                true
            }
            None => false,
        }
    }

    /// Takes any whitespace.
    pub fn ws(&mut self) {
        self.rest = self.rest.trim_start();
    }

    /// `0x` and `min` to `max` hexadecimal digits, as many as there are.
    pub fn hex(&mut self, min: usize, max: usize, digits: Digits) -> Option<u32> {
        let r = self.rest.strip_prefix("0x")?;
        let ok = |c: char| match digits {
            Digits::Upper => c.is_ascii_digit() || ('A'..='F').contains(&c),
            Digits::Either => c.is_ascii_hexdigit(),
        };
        let n = r.chars().take(max).take_while(|&c| ok(c)).count();
        if n < min {
            return None;
        }
        let v = u32::from_str_radix(&r[..n], 16).ok()?;
        self.rest = &r[n..];
        Some(v)
    }

    /// A C identifier's characters, at least one, starting with `prefix`.
    pub fn word(&mut self, prefix: &str) -> Option<&'a str> {
        if !self.rest.starts_with(prefix) {
            return None;
        }
        let n = self.rest[prefix.len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .count();
        if n == 0 {
            return None;
        }
        let (w, r) = self.rest.split_at(prefix.len() + n);
        self.rest = r;
        Some(w)
    }

    /// The end of the entry: nothing, or blanks and one `/* ... */`
    /// comment closing the line. `Some` holds the comment's text, if any.
    pub fn end(self) -> Option<Option<&'a str>> {
        let r = self.rest.trim_start();
        if self.rest.is_empty() {
            return Some(None);
        }
        let inner = r.strip_prefix("/*")?.strip_suffix("*/")?;
        Some(Some(inner))
    }
}

/// Whether `text` holds `MAP(`, `UNI(` or `XXX(` as a word.
pub fn has_entry(text: &str) -> bool {
    ["MAP(", "UNI(", "XXX("].iter().any(|p| {
        text.match_indices(p).any(|(i, _)| {
            !text[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
        })
    })
}
