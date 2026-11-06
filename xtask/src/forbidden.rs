//! `xtask::forbidden`: a session's bytes searched for what must never be
//! filmed, and `cargo xtask forbidden PATTERNS CONTROLS FILE...`.
//!
//! PATTERNS holds one `label::regex` a line, split at the first `::`, with
//! `#` lines and blank ones skipped; it is kept outside the repository,
//! since it names what it forbids. CONTROLS holds `label<TAB>example` lines,
//! the last of a label winning: before any FILE is read, every label must
//! have a control and every control must be found, by its own pattern and
//! by the patterns joined, or nothing is searched (exit 2). A search that
//! cannot fire looks exactly like a clean one.
//!
//! Each FILE is searched raw and with its terminal escapes taken out, so a
//! word a program coloured letter by letter is still found. A hit prints
//! the label and the file, never what matched (exit 1); none, exit 0. A
//! line with no label, a list with no pattern and a file that cannot be read
//! also exit 1.
//!
//! **A pattern is compiled by `regex::bytes` with `unicode(false)`**, so
//! `\w`, `\d`, `\s`, `\b` and `(?i)` are ASCII, as they are on a Python
//! bytes pattern, and `\xHH` is that byte. One that does not compile exits
//! 2, naming its label and the engine's reason, never its text. A byte above
//! 0x7f has to be written `\xHH`, because a raw one would repeat and sit in
//! a class as a whole character, not as a byte.
//!
//! **The engine has no look-around, and three forms stand in for it**, all
//! through a capture group named `hit`. When a pattern has that group, the
//! group is the match it reports; a match the group took no part in is no
//! hit, and the search goes on from the next byte. With `name` invented:
//!
//! | look-around | here |
//! |---|---|
//! | `(?<![-\w])name@` | `(?:^\|[^-\w])(?P<hit>name@)` |
//! | `@name(?![\w-])` | `(?P<hit>@name)(?:\z\|[^\w-])` |
//! | `/path/(?!qa/\|qb/)[a-z]+/` | `/path/(?:qa/\|qb/)\|(?P<hit>/path/[a-z]+/)` |
//!
//! The third puts what the look-ahead refused first, so at any one start it
//! wins, and the match it makes is no hit. [`Pattern::spans`] then reports
//! what `re.finditer` reports for the original. It starts its next search a
//! byte before the last hit's end, where the byte a look-behind only looked
//! at can be the last byte of that hit: `name@name@` is two hits.
//!
//! Where `re` and this engine still differ on a pattern both accept: `$`
//! here is the end of the text alone, where `re` also takes it before a
//! final newline, so write `\z` or `\r?\n`; `\<` and `\>` are word boundaries
//! here and literals there; `[[:alpha:]]`, `[a&&b]`, `[a--b]` and `[a~~b]` are
//! class operations here and sets of those bytes there. `(?i)` here holds to
//! its own pattern, which `re` refuses once the patterns are joined. A
//! possessive `a*+` or `a++` is a repeat of a repeat here; `[a[b]]` is a nested
//! class here and `[a[b]` then `]` there; `\B` here matches empty text, where
//! Python 3.13's does not. `a{,3}` and `(?>a)`, which `re` accepts, do not
//! compile here.
//!
//! One position cannot be looked at twice: the `^` of `(?:^|[...])` holds at
//! offset 0 alone, so when the candidate there is refused, or is a single
//! byte, a hit starting at offset 1 that needs byte 0 as its look-behind is
//! not reported. That takes a one-byte hit, or an exclusion with a look-behind
//! on a pattern that matches at both offsets.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::bytes::{Regex, RegexBuilder, RegexSet, RegexSetBuilder};

use crate::features_since::Exit;

/// The terminal's escapes: OSC to its BEL or ST, CSI, a charset, any other
/// ESC, and the C0 controls but `\t`, `\n`, `\x0b`, `\x0c` and `\r`.
static ESC: LazyLock<Regex> = LazyLock::new(|| {
    RegexBuilder::new(concat!(
        r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]|",
        r"\x1b[()*+][0-~]|\x1b[ -/]*[0-~]|[\x00-\x08\x0e-\x1f]",
    ))
    .unicode(false)
    .build()
    .expect("ESC")
});

/// The bytes as written, and with the terminal's escapes taken out.
pub fn views(data: &[u8]) -> [Cow<'_, [u8]>; 2] {
    [Cow::Borrowed(data), ESC.replace_all(data, &b""[..])]
}

/// One `label::regex` of the list.
pub struct Pattern {
    label: String,
    rx: Regex,
    /// The index of the group named `hit`, when the pattern has it.
    hit: Option<usize>,
}

impl Pattern {
    /// Compiled for bytes, with the reason it could not be and no more:
    /// the engine's own message quotes the pattern, which is not to be seen.
    pub fn compile(label: &str, rx: &str) -> Result<Pattern, String> {
        if !rx.is_ascii() {
            return Err("a byte above 0x7f, to be written \\xHH".to_string());
        }
        let compiled = RegexBuilder::new(rx)
            .unicode(false)
            .build()
            .map_err(|e| match e {
                regex::Error::Syntax(s) => s
                    .lines()
                    .last()
                    .unwrap_or_default()
                    .trim_start_matches("error: ")
                    .to_string(),
                other => other.to_string(),
            })?;
        let hit = compiled
            .capture_names()
            .position(|name| name == Some("hit"));
        Ok(Pattern {
            label: label.to_string(),
            rx: compiled,
            hit,
        })
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// Whether the pattern finds anything in `hay`.
    pub fn finds(&self, hay: &[u8]) -> bool {
        self.spans(hay).next().is_some()
    }

    /// What it finds, left to right, as `re.finditer` reports it for the
    /// look-around this pattern stands in for. An empty hit moves on a byte.
    pub fn spans<'p, 'h>(&'p self, hay: &'h [u8]) -> Spans<'p, 'h> {
        Spans {
            pattern: self,
            hay,
            from: Offset(0),
            past: Offset(0),
        }
    }
}

/// A byte offset into the text searched.
#[derive(Clone, Copy)]
struct Offset(usize);

/// The iterator of [`Pattern::spans`].
pub struct Spans<'p, 'h> {
    pattern: &'p Pattern,
    hay: &'h [u8],
    /// Where the next search begins.
    from: Offset,
    /// Where the next hit may begin: the end of the last.
    past: Offset,
}

impl Iterator for Spans<'_, '_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let Pattern { rx, hit, .. } = self.pattern;
        while self.from.0 <= self.hay.len() {
            let (whole, found) = match hit {
                None => {
                    let m = rx.find_at(self.hay, self.from.0)?;
                    (m.range(), Some(m.range()))
                }
                Some(group) => {
                    let c = rx.captures_at(self.hay, self.from.0)?;
                    (c.get(0)?.range(), c.get(*group).map(|m| m.range()))
                }
            };
            match found {
                Some(span) if span.start >= self.past.0 => {
                    self.past = Offset(span.end + usize::from(span.is_empty()));
                    self.from = match hit {
                        Some(_) => Offset(self.past.0.saturating_sub(1)),
                        None => self.past,
                    };
                    return Some(span);
                }
                _ => self.from = Offset(whole.start + 1),
            }
        }
        None
    }
}

/// How a search ended, for a caller that needs more than its exit code:
/// a list that could not be used also exits 1, as hits do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Every control found, and no pattern in any file.
    Clean,
    /// A pattern was found in a file.
    Hits,
    /// Nothing was searched, for the reason it wrote; the exit code to give.
    Refused(Exit),
}

impl Verdict {
    pub fn exit(self) -> Exit {
        match self {
            Verdict::Clean => Exit(0),
            Verdict::Hits => Exit(1),
            Verdict::Refused(e) => e,
        }
    }
}

/// What `bytes.strip()` removes, all of the line: a blank line.
fn blank(line: &[u8]) -> bool {
    line.iter()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
}

/// A file's lines that are neither blank nor a `#` line, split at `\n`
/// alone, which is what a line of a file opened in binary mode ends at.
fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|&b| b == b'\n')
        .filter(|l| !blank(l) && !l.starts_with(b"#"))
}

fn split_at<'a>(line: &'a [u8], sep: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    line.windows(sep.len())
        .position(|w| w == sep)
        .map(|i| (&line[..i], &line[i + sep.len()..]))
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    fs::read(path).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

fn utf8<'a>(bytes: &'a [u8], path: &Path) -> io::Result<&'a str> {
    std::str::from_utf8(bytes).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: a label: {e}", path.display()),
        )
    })
}

/// The list, or the exit code to give having said why not.
fn patterns(path: &Path, err: &mut impl Write) -> io::Result<Result<Vec<Pattern>, Verdict>> {
    let data = read(path)?;
    let mut out = Vec::new();
    for line in lines(&data) {
        let Some((name, rx)) = split_at(line, b"::") else {
            writeln!(
                err,
                "readme-forbidden: a line with no label in {}",
                path.display()
            )?;
            return Ok(Err(Verdict::Refused(Exit(1))));
        };
        let name = utf8(name, path)?;
        // A byte the text cannot carry is the same refusal as a raw one.
        let text = String::from_utf8_lossy(rx);
        match Pattern::compile(name, &text) {
            Ok(p) => out.push(p),
            Err(reason) => {
                writeln!(err, "readme-forbidden: {name}: does not compile: {reason}")?;
                return Ok(Err(Verdict::Refused(Exit(2))));
            }
        }
    }
    if out.is_empty() {
        writeln!(err, "readme-forbidden: {} holds no pattern", path.display())?;
        return Ok(Err(Verdict::Refused(Exit(1))));
    }
    Ok(Ok(out))
}

/// The label of each control and its example, in the order of the file.
fn controls(path: &Path) -> io::Result<Vec<(String, Vec<u8>)>> {
    let data = read(path)?;
    let mut out = Vec::new();
    for line in lines(&data) {
        let (k, v) = split_at(line, b"\t").unwrap_or((line, b""));
        out.push((utf8(k, path)?.to_string(), v.to_vec()));
    }
    Ok(out)
}

/// The search over `files`, printing as the command prints. An error is a
/// file that could not be read or a label that is not UTF-8: the command
/// exits 1, having printed no hit and no summary.
pub fn search(
    list: &Path,
    ctl: &Path,
    files: &[PathBuf],
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<Verdict> {
    let pats = match patterns(list, err)? {
        Ok(p) => p,
        Err(refused) => return Ok(refused),
    };
    // A set rather than `(?:p1)|(?:p2)` in text, since every pattern may
    // have its own group named `hit`. A pattern's own hit is always a match
    // of the set while the two read a pattern alike, and the check below
    // holds them to it.
    let joined = RegexSetBuilder::new(pats.iter().map(|p| p.rx.as_str()))
        .unicode(false)
        .build();
    let joined: RegexSet = match joined {
        Ok(set) => set,
        Err(_) => {
            writeln!(err, "readme-forbidden: the patterns joined do not compile")?;
            return Ok(Verdict::Refused(Exit(2)));
        }
    };
    let ctl = controls(ctl)?;
    let mut bad = Vec::new();
    for p in &pats {
        let name = p.label();
        match ctl.iter().rev().find(|(k, _)| k == name) {
            None => bad.push(format!("{name}: no control")),
            Some((_, v)) if !p.finds(v) => bad.push(format!("{name}: its control is not found")),
            Some((_, v)) if !joined.is_match(v) => {
                bad.push(format!("{name}: the joined patterns miss its control"));
            }
            Some(_) => {}
        }
    }
    if !bad.is_empty() {
        for b in &bad {
            writeln!(err, "readme-forbidden: {b}")?;
        }
        return Ok(Verdict::Refused(Exit(2)));
    }
    let mut hits: Vec<(&str, &Path)> = Vec::new();
    for path in files {
        let data = read(path)?;
        for view in views(&data) {
            if !joined.is_match(&view) {
                continue;
            }
            for p in &pats {
                let hit = (p.label(), path.as_path());
                if p.finds(&view) && !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
        }
    }
    for (name, path) in &hits {
        writeln!(err, "readme-forbidden: {name} in {}", path.display())?;
    }
    writeln!(
        out,
        "readme-forbidden: {} patterns, each found in its control; {} files, {} hits",
        pats.len(),
        files.len(),
        hits.len()
    )?;
    Ok(if hits.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Hits
    })
}

/// `cargo xtask forbidden PATTERNS CONTROLS FILE...`.
pub fn run(args: &[&str], out: &mut impl Write, err: &mut impl Write) -> io::Result<Exit> {
    match args {
        [list, ctl, files @ ..] if !files.is_empty() => {
            let files: Vec<PathBuf> = files.iter().map(PathBuf::from).collect();
            let verdict = search(Path::new(list), Path::new(ctl), &files, out, err)?;
            Ok(verdict.exit())
        }
        _ => {
            writeln!(
                err,
                "usage: cargo xtask forbidden PATTERNS CONTROLS FILE..."
            )?;
            Ok(Exit(1))
        }
    }
}
