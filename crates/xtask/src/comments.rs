//! `xtask::comments`: the comment lines a change adds, held to the rule
//! that a comment says only what baud's code does, and
//! `cargo xtask comments --check BASE [-- PATHSPEC...]`.
//!
//! **What is read.** The lines `git diff -U0 BASE HEAD` adds, and of those
//! only a whole-line comment: one that begins, after its indent, with `//`
//! in a file whose comments are spelled that way (`.rs`, `.metal`, `.c`,
//! `.h`, `.m`, `.js`, `.zig`), or with `#` in one whose comments are spelled
//! that way (`.sh`, `.bash`, `.toml`, `.nix`, `.yml`, `.yaml`), a `#!` line
//! excepted. A comment after code on the same line is not read: telling it
//! from a `//` inside a string literal takes a lexer, and every URL in a
//! string has one. A removed line is never read, so a change that takes a
//! breach out passes.
//!
//! **What is passed before anything is searched.** A line that is a
//! notice -- one that opens with "Copyright", "(c)", the copyright sign, an
//! SPDX identifier, "Licensed under" or "Permission is hereby granted" -- or
//! that carries a credit, "ported from", "transcribed from" or "adapted
//! from" anywhere, is passed whole; a line that only mentions a licence is
//! searched like any other,
//! and a `[[source: KEY]]` citation is taken out of every line, since a key
//! may carry a version, a date or a revision.
//!
//! **The public rules**, [`Rule`], are searched with the text between
//! straight or curly double quotes also taken out, because a quotation from
//! a specification says what the specification says. Each rule is written
//! where it is defined, with the forms it passes. They are not searched in
//! [`SOURCES_MANIFEST`], whose comments record when and where each source
//! was fetched; the private terms still are.
//!
//! **The private terms** are read from the file the environment variable
//! [`TERMS_VAR`] names, which the repository never holds. Each line is
//! `REGEX<TAB>CONTROL`, with `#` lines and blank ones skipped; the regex is
//! compiled as [`forbidden::Pattern`] compiles one, so it is ASCII, written
//! for bytes, and a group named `hit` marks the part that counts: with that
//! group, a form the file allows can be written before the form it refuses,
//! as `forbidden`'s documentation shows. Every line's control must be found
//! by its own regex before any comment is searched, or nothing is. A term is
//! searched in the line with only the citations taken out. **A hit names
//! the term file's line number, never the term and never the comment.**
//!
//! **Exits.** 0 when nothing is found; 1 when something is, one line each,
//! `PATH:LINE: WHAT`; 2 when nothing was searched: the variable unset, its
//! file unreadable, a line with no control, a control not found, a regex
//! that does not compile, no term at all, `git` failing, or a bad command
//! line.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

use crate::features_since::Exit;
use crate::forbidden::Pattern;

/// The environment variable naming the private term file.
pub const TERMS_VAR: &str = "BAUD_COMMENT_TERMS";

/// The sources manifest, by its path from the repository root: a record of
/// each source's fetch, whose comments the public rules pass and the
/// private terms still read.
pub const SOURCES_MANIFEST: &str = "docs/research/sources/MANIFEST.toml";

/// A line number in a file as the change leaves it, from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineNo(pub usize);

/// A line number in the private term file, from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TermLine(pub usize);

/// A line the change adds.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Added {
    pub path: String,
    pub line: LineNo,
    pub text: String,
}

/// The public rules, each a shape of text a comment that says what the
/// code does has no use for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rule {
    /// A calendar date: `YYYY-MM-DD` with a year from 2000, a month's name
    /// before a year, or "since", "until" or "as of" before a year from 2000
    /// to 2039. A year before 2000, an epoch's for instance, passes.
    Date,
    /// The phrase "used to" after anything but a form of "be" or "get",
    /// which make it a purpose: "the table is used to look up" passes.
    Former,
    /// A lowercase hexadecimal word of 7 to 40 digits, holding a digit and
    /// a letter both. One after `#`, `-`, `_`, `.` or `+`, or before `-` or
    /// `_`, is part of a colour or a name, and a binary literal is neither.
    Hash,
    /// A decision told as one: "we" with a verb of choosing, a decision
    /// "was" or "were" made, or a call attributed to the project's owner.
    Story,
}

/// Every public rule, in the order a line is searched.
pub const RULES: [Rule; 4] = [Rule::Date, Rule::Former, Rule::Hash, Rule::Story];

impl Rule {
    /// What a hit prints.
    pub fn label(self) -> &'static str {
        match self {
            Rule::Date => "a date",
            Rule::Former => "a former state of the code",
            Rule::Hash => "a commit hash",
            Rule::Story => "a decision story",
        }
    }

    /// Whether the rule finds anything in a comment's text.
    pub fn finds(self, text: &str) -> bool {
        match self {
            Rule::Date => DATE.is_match(text),
            Rule::Former => former(text),
            Rule::Hash => hash(text),
            Rule::Story => STORY.is_match(text),
        }
    }
}

static DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b20[0-9]{2}-(?:0[1-9]|1[0-2])-(?:0[1-9]|[12][0-9]|3[01])\b",
        r"|(?i:\b(?:jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|june?|july?",
        r"|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\.?",
        r"\s+(?:[0-9]{1,2},?\s+)?20[0-3][0-9]\b)",
        r"|(?i:\b(?:since|until|as of)\s+20[0-3][0-9]\b)",
    ))
    .expect("DATE")
});

static USED_TO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bused\s+to\b").expect("USED_TO"));

static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[0-9a-f]{7,40}\b").expect("HEX"));

static STORY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\bwe\s+(?:decided|chose|tried|picked|switched|reverted|settled\s+on)\b",
        r"|(?i)\b(?:was|were)\s+(?:decided|reverted|withdrawn)\b",
        r"|(?i)\bthe\s+owner(?:'s)?\s+",
        r"(?:call|decision|choice|asked|said|chose|decided|wants|wanted)\b",
    ))
    .expect("STORY")
});

/// The words before "used to" that make it the present's purpose.
const BE: [&str; 10] = [
    "is", "are", "be", "been", "being", "was", "were", "get", "gets", "got",
];

fn former(text: &str) -> bool {
    USED_TO.find_iter(text).any(|m| {
        let before = text[..m.start()].trim_end();
        let word = before
            .rsplit(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        !BE.contains(&word.as_str())
    })
}

fn hash(text: &str) -> bool {
    HEX.find_iter(text).any(|m| {
        let word = m.as_str();
        let digit = word.bytes().any(|b| b.is_ascii_digit());
        let letter = word.bytes().any(|b| b.is_ascii_alphabetic());
        let before = text[..m.start()].chars().next_back();
        let after = text[m.end()..].chars().next();
        let binary = word.starts_with("0b") && word[2..].bytes().all(|b| b == b'0' || b == b'1');
        digit
            && letter
            && !binary
            && !matches!(before, Some('#' | '-' | '_' | '.' | '+'))
            && !matches!(after, Some('-' | '_'))
    })
}

static NOTICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^\s*(?:copyright\b|\(c\)\s|\x{a9}|spdx-license-identifier:",
        r"|licensed under\b|permission is hereby granted\b)",
        r"|(?i)\b(?:ported|transcribed|adapted) from\b",
    ))
    .expect("NOTICE")
});

static CITATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[source:[^\]]*\]\]").expect("CITATION"));

static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\"[^\"]*\"|\u{201c}[^\u{201d}]*\u{201d}").expect("QUOTED"));

/// How a file spells a whole-line comment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Marker {
    Slashes,
    Hash,
}

fn marker(path: &str) -> Option<Marker> {
    let ext = Path::new(path).extension().and_then(OsStr::to_str)?;
    match ext {
        "rs" | "metal" | "c" | "h" | "m" | "js" | "zig" => Some(Marker::Slashes),
        "sh" | "bash" | "toml" | "nix" | "yml" | "yaml" => Some(Marker::Hash),
        _ => None,
    }
}

/// Whether `path` is a source whose whole-line comments `comment` reads.
pub fn commented(path: &str) -> bool {
    marker(path).is_some()
}

/// The text of a whole-line comment in `path`, its marker taken off, or
/// `None` when the line is not one.
pub fn comment<'a>(path: &str, line: &'a str) -> Option<&'a str> {
    let t = line.trim_start();
    match marker(path)? {
        Marker::Slashes => {
            let rest = t.strip_prefix("//")?;
            Some(rest.strip_prefix(['/', '!']).unwrap_or(rest))
        }
        Marker::Hash => {
            if t.starts_with("#!") {
                return None;
            }
            Some(t.strip_prefix('#')?.trim_start_matches('#'))
        }
    }
}

/// The lines `git diff -U0` output adds, with the line numbers the change
/// gives them. A hunk's lines are counted from its header, so an added line
/// that reads like a file header is still a line.
pub fn added(diff: &str) -> Vec<Added> {
    let mut found = Vec::new();
    let mut path: Option<String> = None;
    let mut next = LineNo(0);
    let (mut old_left, mut new_left) = (0usize, 0usize);
    for raw in diff.lines() {
        if old_left > 0 || new_left > 0 {
            match raw.as_bytes().first() {
                Some(b'+') => {
                    if let Some(p) = &path {
                        found.push(Added {
                            path: p.clone(),
                            line: next,
                            text: raw[1..].to_string(),
                        });
                    }
                    next = LineNo(next.0 + 1);
                    new_left = new_left.saturating_sub(1);
                }
                Some(b'-') => old_left = old_left.saturating_sub(1),
                Some(b' ') => {
                    next = LineNo(next.0 + 1);
                    old_left = old_left.saturating_sub(1);
                    new_left = new_left.saturating_sub(1);
                }
                _ => {}
            }
            continue;
        }
        if raw.starts_with("diff --git ") {
            path = None;
        } else if let Some(p) = raw.strip_prefix("+++ ") {
            path = p.strip_prefix("b/").map(str::to_string);
        } else if let Some(h) = raw.strip_prefix("@@ ") {
            if let Some((old, new)) = hunk(h) {
                old_left = old.1;
                new_left = new.1;
                next = LineNo(new.0);
            }
        }
    }
    found
}

/// A hunk header's `-a,b +c,d`, a count left out being 1.
fn hunk(h: &str) -> Option<((usize, usize), (usize, usize))> {
    let mut parts = h.split(' ');
    let range = |s: &str| -> Option<(usize, usize)> {
        let (start, count) = s.split_once(',').unwrap_or((s, "1"));
        Some((start.parse().ok()?, count.parse().ok()?))
    };
    let old = range(parts.next()?.strip_prefix('-')?)?;
    let new = range(parts.next()?.strip_prefix('+')?)?;
    Some((old, new))
}

/// The private terms, each with its line in the file.
pub struct Terms(Vec<(TermLine, Pattern)>);

impl Terms {
    /// The file's text read, every control found by its own regex. The
    /// error names a line by its number and never quotes it.
    pub fn parse(text: &str) -> Result<Terms, String> {
        let mut terms = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let n = TermLine(i + 1);
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((rx, control)) = line.split_once('\t') else {
                return Err(format!("terms line {}: no control after a tab", n.0));
            };
            let p = Pattern::compile(&n.0.to_string(), rx)
                .map_err(|e| format!("terms line {}: {e}", n.0))?;
            if !p.finds(control.as_bytes()) {
                return Err(format!("terms line {}: its control is not found", n.0));
            }
            terms.push((n, p));
        }
        if terms.is_empty() {
            return Err("the term file holds no term".to_string());
        }
        Ok(Terms(terms))
    }
}

/// What a line was found to hold.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Found {
    Public(Rule),
    Term(TermLine),
}

/// One finding, printed `PATH:LINE: WHAT`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hit {
    pub path: String,
    pub line: LineNo,
    pub found: Found,
}

impl fmt::Display for Hit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: ", self.path, self.line.0)?;
        match self.found {
            Found::Public(r) => f.write_str(r.label()),
            Found::Term(n) => write!(f, "term file line {}", n.0),
        }
    }
}

/// Every finding in the comment lines `diff` adds, in the diff's order.
pub fn check(diff: &str, terms: &Terms) -> Vec<Hit> {
    let mut hits = Vec::new();
    for a in added(diff) {
        let Some(text) = comment(&a.path, &a.text) else {
            continue;
        };
        if NOTICE.is_match(text) {
            continue;
        }
        let cited = CITATION.replace_all(text, " ");
        let unquoted = QUOTED.replace_all(&cited, " ");
        // A Markdown blockquote (`> `) quotes a source as quotation marks
        // do, and the sources manifest's comments record each fetch: the
        // public rules pass both, the private terms still read both.
        let blockquote = text.trim_start().starts_with('>');
        let public = !blockquote && a.path != SOURCES_MANIFEST;
        let mut hit = |found| {
            hits.push(Hit {
                path: a.path.clone(),
                line: a.line,
                found,
            })
        };
        for r in RULES {
            if public && r.finds(&unquoted) {
                hit(Found::Public(r));
            }
        }
        for (n, p) in &terms.0 {
            if p.finds(cited.as_bytes()) {
                hit(Found::Term(*n));
            }
        }
    }
    hits
}

/// `cargo xtask comments --check BASE [-- PATHSPEC...]`, in the repository
/// at `root`, with the term file `terms` names.
pub fn run(
    args: &[&str],
    root: &Path,
    terms: Option<&OsStr>,
    out: &mut impl Write,
    err: &mut impl Write,
) -> io::Result<Exit> {
    let (base, paths) = match args {
        ["--check", base] => (*base, &[][..]),
        ["--check", base, "--", paths @ ..] => (*base, paths),
        _ => {
            writeln!(
                err,
                "usage: cargo xtask comments --check BASE [-- PATHSPEC...]"
            )?;
            return Ok(Exit(2));
        }
    };
    let Some(file) = terms else {
        writeln!(err, "{TERMS_VAR} is not set: nothing was searched")?;
        return Ok(Exit(2));
    };
    let terms = fs::read_to_string(file)
        .map_err(|e| format!("the term file: {e}"))
        .and_then(|text| Terms::parse(&text));
    let terms = match terms {
        Ok(t) => t,
        Err(e) => {
            writeln!(err, "{e}: nothing was searched")?;
            return Ok(Exit(2));
        }
    };
    let mut git = Command::new("git");
    git.arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=off", "diff", "-U0", "--no-color"])
        .args(["--no-ext-diff", "--no-textconv", "--no-relative", "-M"])
        .args(["--src-prefix=a/", "--dst-prefix=b/", base, "HEAD", "--"])
        .args(paths);
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
        git.env_remove(v);
    }
    let git = git.output()?;
    if !git.status.success() {
        err.write_all(&git.stderr)?;
        writeln!(err, "git diff failed: nothing was searched")?;
        return Ok(Exit(2));
    }
    let diff = String::from_utf8_lossy(&git.stdout);
    let hits = check(&diff, &terms);
    for h in &hits {
        writeln!(out, "{h}")?;
    }
    writeln!(out, "{} comment finding(s)", hits.len())?;
    Ok(Exit(u8::from(!hits.is_empty())))
}
