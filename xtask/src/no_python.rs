//! `cargo xtask no-python`: the repository holds no Python and runs none.
//!
//! Every generator, gate and oracle it had is Rust now, and a script or a
//! recipe that quietly calls an interpreter again is a dependency nothing
//! declares. This reads every file `git ls-files` lists and exits 1 on:
//!
//! - **a source or bytecode file**: `.py`, `.pyw`, `.pyi`, `.pyx`, `.pyc`,
//!   `.pyo`, `.pyd`, or anything under a `__pycache__/`;
//! - **a shebang** naming `python`, `python3`, `python3.N` or `pip`, as a
//!   word's last path component, in any file;
//! - **a command**, in a shell file (`.sh`, `.bash`, `.zsh`, `Dockerfile*`,
//!   `.github/workflows/*`, a shell shebang) and in a Markdown fence labelled
//!   for a shell or not labelled: the same words where a command starts, at
//!   the start of a line or after `|`, `&&`, `||`, `;`, `&`, `(`, `$(`, a
//!   backquote, a shell keyword, `exec`, `env`, `sudo`, `nice`, `xargs`,
//!   `sh -c`, `RUN`, `run:` or a variable assignment;
//! - **a package**, in the same places: the words as an `apt-get`, `apt`,
//!   `apk`, `brew`, `dnf`, `yum`, `pacman`, `nix-env`, `nix-shell` or
//!   `nix shell` package, `-`-suffixed names included;
//! - **a nix attribute**: outside a comment in `.nix`, the word `python`
//!   with its digits, so `pkgs.python3`, `python3Packages` and a bare
//!   `python3` in a list;
//! - **a spawn**: `Command::new` given a string naming the words, in `.rs`;
//! - **an invocation**, in any text file: the words followed by what only a
//!   command line has, which is a flag, `<<` or a `.py` path, or `pip`
//!   followed by a flag or a verb such as `install`;
//! - **a run in a Markdown code span**: a span whose command is the words
//!   and which carries more than the word.
//!
//! **What tells a use from the word.** The match is case-sensitive and whole:
//! the character before it cannot be a letter, digit, `_`, `.`, `-`, `$` or
//! `[`, and what follows has to be one of the forms above. So `Python 3.14`,
//! a `# Python` heading or comment, `pythonic`, `libpython3.14`,
//! `lib/python3.14/os.py`, `[python]`, `$python`, `pipefail` and `pipes.sh`
//! are prose, and so is a span holding the word alone, which names the
//! program rather than running it. A shell comment is taken off before the
//! command and package forms read the line, and a fence labelled for another
//! language is not read as commands.
//!
//! **What is not read, by name, and nothing else.**
//!
//! - [`OWN`], this file and its tests, by exact path: they have to spell the
//!   forms they refuse. The same text at any other path fires.
//! - Every file `docs/research/sources/MANIFEST.toml` pins, such as the
//!   third-party `.py.txt` sources: text that is read and never run, whose
//!   SHA-256 `cargo xtask sources` holds, so nothing can be added to one. A
//!   file there the manifest does not name is read as any other.
//! - `*.raw` under `tests/`: what a program wrote to a terminal, committed as
//!   evidence and fed to the parser, which no recording can be edited out of.
//! - A file with a NUL in its first 8000 bytes, git's test for binary.
//!
//! The path check holds for all of them, so a `.py` is refused anywhere.
//!
//! **History is one thing, and narrow.** In `MEASUREMENTS.md` alone, below
//! its first `## ` heading and outside a fence, a run in a code span whose
//! preceding word is `ran` says what a record's arm did then. The same
//! span after any other word, in a fence, without the backquotes or in any
//! other file fires. `xtask/tests/no_python.rs` holds each of those.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::ops::Range;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

use crate::scan_audit::Line;
use crate::sources;
use crate::third_party::Outcome;

/// The gate and its tests, which spell what they refuse.
pub const OWN: [&str; 2] = ["xtask/src/no_python.rs", "xtask/tests/no_python.rs"];
/// The only file history is allowed in.
pub const RECORD: &str = "MEASUREMENTS.md";
const MANIFEST: &str = "docs/research/sources/MANIFEST.toml";
const SOURCES: &str = "docs/research/sources/";

const EXTENSIONS: [&str; 7] = [".py", ".pyw", ".pyi", ".pyx", ".pyc", ".pyo", ".pyd"];
/// A word before the command, which leaves the command still to come.
const WRAPPERS: &[&str] = &[
    "exec",
    "env",
    "command",
    "sudo",
    "nice",
    "nohup",
    "time",
    "xargs",
    "then",
    "do",
    "else",
    "elif",
    "if",
    "while",
    "until",
    "!",
    "RUN",
    "CMD",
    "ENTRYPOINT",
    "run:",
    "-",
    "$",
    "%",
];
const SHELLS: [&str; 5] = ["sh", "bash", "zsh", "dash", "ksh"];
const FENCES: [&str; 8] = [
    "",
    "sh",
    "bash",
    "zsh",
    "shell",
    "console",
    "shell-session",
    "dockerfile",
];

/// git's test for binary: a NUL in this many leading bytes.
const BINARY_PROBE: usize = 8000;

/// The words followed by what only a command line has. Group 1 starts where
/// the word's path does.
static INVOCATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:^|[^A-Za-z0-9_.$\[/-])",
        r"((?:[A-Za-z0-9_./~-]*/)?",
        r"(?:python(?:[0-9]+(?:\.[0-9]+)*)?[ \t]+",
        r"(?:-[A-Za-z]|-[ \t]*<<|<<|[A-Za-z0-9_./-]+\.py(?:[^A-Za-z0-9_]|$))",
        r"|pip[0-9]*[ \t]+(?:install|uninstall|download|wheel|freeze|-[A-Za-z])))",
    ))
    .expect("the invocation pattern")
});
/// `${...}` taken out, so a variable's name is not read as a command.
static EXPANSION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$\{[^}]*\}").expect("the expansion pattern"));
static SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\|\||&&|\$\(|[|;&(){}`]").expect("the separator pattern"));
static INSTALLER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:^|[^A-Za-z0-9_-])",
        r"(?:apt-get|apt|dnf|yum|brew)[ \t]+(?:[^ \t&|;]+[ \t]+)*?install\b",
        r"|(?:^|[^A-Za-z0-9_-])apk[ \t]+add\b",
        r"|(?:^|[^A-Za-z0-9_-])pacman[ \t]+-S[A-Za-z]*",
        r"|(?:^|[^A-Za-z0-9_-])nix-env[ \t]+-[iA]",
        r"|(?:^|[^A-Za-z0-9_-])nix-shell[ \t]+(?:[^ \t&|;]+[ \t]+)*?-p\b",
        r"|(?:^|[^A-Za-z0-9_-])nix[ \t]+(?:shell|profile[ \t]+install)\b",
    ))
    .expect("the installer pattern")
});
static NIX_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[^A-Za-z0-9_-])python[0-9]*(?:$|[^a-z-])").expect("the nix pattern")
});
static SPAWN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"Command::new\(\s*"(?:[^"]*/)?(?:python[0-9.]*|pip[0-9]*)""#)
        .expect("the spawn pattern")
});

/// Which rule a line broke.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    File,
    Shebang,
    Command,
    Package,
    Nix,
    Spawn,
    Invocation,
    Span,
}

impl Form {
    fn name(self) -> &'static str {
        match self {
            Self::File => "a Python file",
            Self::Shebang => "a shebang",
            Self::Command => "a command",
            Self::Package => "a package",
            Self::Nix => "a nix attribute",
            Self::Spawn => "a spawn",
            Self::Invocation => "an invocation",
            Self::Span => "a run in a code span",
        }
    }
}

/// One use, at a one-based line; 0 for the file's path.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Finding {
    pub path: String,
    pub line: usize,
    pub form: Form,
    pub text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Shell,
    Markdown,
    Nix,
    Rust,
    Other,
}

fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// `python`, `python3`, `python3.14`, `pip` or `pip3`, with or without a
/// path before it.
fn is_python(word: &str) -> bool {
    let name = base(word);
    if let Some(rest) = name.strip_prefix("python") {
        return rest.is_empty()
            || (rest.starts_with(|c: char| c.is_ascii_digit())
                && rest.chars().all(|c| c.is_ascii_digit() || c == '.'));
    }
    name.strip_prefix("pip")
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
}

fn is_assignment(word: &str) -> bool {
    match word.split_once('=') {
        Some((name, _)) => {
            !name.is_empty()
                && !name.starts_with(|c: char| c.is_ascii_digit())
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// The first word of one command is the words, once what can stand before a
/// command is skipped.
fn segment_runs_python(segment: &str) -> bool {
    let mut words = segment.split_whitespace().peekable();
    let mut wrapped = false;
    while let Some(raw) = words.next() {
        let word = raw.trim_matches(|c| matches!(c, '\'' | '"' | '\\' | '[' | ']' | ','));
        if word.is_empty() || is_assignment(word) {
            continue;
        }
        if WRAPPERS.contains(&base(word)) {
            wrapped = true;
            continue;
        }
        if SHELLS.contains(&base(word))
            && words
                .peek()
                .is_some_and(|n| n.starts_with('-') && n.contains('c'))
        {
            words.next();
            wrapped = true;
            continue;
        }
        if wrapped && (word.starts_with('-') || word.chars().all(|c| c.is_ascii_digit())) {
            continue;
        }
        let command = word.split(['<', '>']).next().unwrap_or(word);
        return is_python(command);
    }
    false
}

fn runs_python(line: &str) -> bool {
    let plain = EXPANSION.replace_all(line, "$$V");
    SEPARATOR.split(&plain).any(segment_runs_python)
}

/// The words as a package after an installer.
fn installs_python(line: &str) -> bool {
    INSTALLER.find_iter(line).any(|m| {
        let rest = &line[m.end()..];
        let args = rest.split(['&', '|', ';']).next().unwrap_or(rest);
        args.split_whitespace().any(|raw| {
            let word = raw.trim_matches(|c| matches!(c, '\'' | '"' | '\\'));
            let word = word.rsplit('#').next().unwrap_or(word);
            let stem = word.split(['-', '=']).next().unwrap_or(word);
            let set = word.starts_with("python") && word.contains("Packages");
            !word.starts_with('-') && !stem.contains('/') && (is_python(stem) || set)
        })
    })
}

/// `line` up to a `#` that starts a word.
fn uncommented(line: &str) -> &str {
    let mut before = ' ';
    for (i, c) in line.char_indices() {
        if c == '#' && before.is_whitespace() {
            return &line[..i];
        }
        before = c;
    }
    line
}

/// Physical lines joined at a trailing `\`, comments off, each with the
/// number of the line it starts on.
fn logical<'a>(lines: impl Iterator<Item = (usize, &'a str)>) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in lines {
        let code = uncommented(line).trim_end();
        let (start, mut text) = open.take().unwrap_or((n, String::new()));
        match code.strip_suffix('\\') {
            Some(head) => {
                text.push_str(head);
                text.push(' ');
                open = Some((start, text));
            }
            None => {
                text.push_str(code);
                out.push((start, text));
            }
        }
    }
    out.extend(open);
    out
}

/// The code spans of one line: the range of each, backquotes included.
fn spans(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let open = i;
        while i < bytes.len() && bytes[i] == b'`' {
            i += 1;
        }
        let run = i - open;
        let mut j = i;
        let mut closed = None;
        while j < bytes.len() {
            if bytes[j] == b'`' {
                let from = j;
                while j < bytes.len() && bytes[j] == b'`' {
                    j += 1;
                }
                if j - from == run {
                    closed = Some(j);
                    break;
                }
            } else {
                j += 1;
            }
        }
        if let Some(end) = closed {
            out.push(open..end);
            i = end;
        }
    }
    out
}

/// A fence's opening or closing: its character, length and label.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let t = line.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = t.chars().take_while(|x| *x == c).count();
    (len >= 3).then(|| (c, len, t[len..].trim()))
}

struct Scan<'a> {
    path: &'a str,
    found: Vec<Finding>,
}

impl Scan<'_> {
    fn push(&mut self, Line(line): Line, form: Form, text: &str) {
        self.found.push(Finding {
            path: self.path.to_string(),
            line,
            form,
            text: text.trim().chars().take(100).collect(),
        });
    }

    fn invocations(&mut self, n: Line, line: &str, history: &[Range<usize>]) {
        let hit = INVOCATION.captures_iter(line).any(|c| {
            let start = c.get(1).map_or(0, |m| m.start());
            !history.iter().any(|h| h.contains(&start))
        });
        if hit {
            self.push(n, Form::Invocation, line);
        }
    }

    fn shell_lines<'a>(&mut self, lines: impl Iterator<Item = (usize, &'a str)>) {
        for (n, text) in logical(lines) {
            if runs_python(&text) {
                self.push(Line(n), Form::Command, &text);
            } else if installs_python(&text) {
                self.push(Line(n), Form::Package, &text);
            }
        }
    }

    fn shell(&mut self, text: &str) {
        self.shell_lines(text.lines().enumerate().map(|(i, l)| (i + 1, l)));
        self.other(text);
    }

    fn markdown(&mut self, text: &str) {
        let record = self.path == RECORD;
        let mut section = false;
        let mut open: Option<(char, usize, bool)> = None;
        let mut block: Vec<(usize, &str)> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            let n = i + 1;
            if let Some((c, len, shell)) = open {
                match fence(line) {
                    Some((d, l, "")) if d == c && l >= len => {
                        if shell {
                            self.shell_lines(block.drain(..));
                        }
                        block.clear();
                        open = None;
                    }
                    _ => {
                        self.invocations(Line(n), line, &[]);
                        block.push((n, line));
                    }
                }
                continue;
            }
            if let Some((c, len, label)) = fence(line) {
                let word = label.split_whitespace().next().unwrap_or("");
                let lower = word.to_ascii_lowercase();
                open = Some((c, len, FENCES.contains(&lower.as_str())));
                continue;
            }
            section = section || line.starts_with("## ");
            let all = spans(line);
            let history: Vec<Range<usize>> = all
                .iter()
                .filter(|s| record && section && after_ran(&line[..s.start]))
                .cloned()
                .collect();
            self.invocations(Line(n), line, &history);
            for s in &all {
                if history.contains(s) {
                    continue;
                }
                let inner = line[s.clone()].trim_matches('`').trim();
                if inner.split_whitespace().count() >= 2 && runs_python(inner) {
                    self.push(Line(n), Form::Span, line);
                }
            }
        }
        if let Some((_, _, true)) = open {
            self.shell_lines(block.drain(..));
        }
    }

    fn nix(&mut self, text: &str) {
        for (i, line) in text.lines().enumerate() {
            self.invocations(Line(i + 1), line, &[]);
            if NIX_WORD.is_match(uncommented(line)) {
                self.push(Line(i + 1), Form::Nix, line);
            }
        }
    }

    fn rust(&mut self, text: &str) {
        for m in SPAWN.find_iter(text) {
            let n = 1 + text[..m.start()].matches('\n').count();
            self.push(Line(n), Form::Spawn, m.as_str());
        }
        self.other(text);
    }

    fn other(&mut self, text: &str) {
        for (i, line) in text.lines().enumerate() {
            self.invocations(Line(i + 1), line, &[]);
        }
    }
}

/// The word before a span is `ran`.
fn after_ran(before: &str) -> bool {
    before.split_whitespace().next_back() == Some("ran") && before.ends_with(char::is_whitespace)
}

fn shebang_words(first: &str) -> Vec<&str> {
    first
        .strip_prefix("#!")
        .map_or_else(Vec::new, |rest| rest.split_whitespace().collect())
}

fn kind(path: &str, first: &str) -> Kind {
    let name = base(path);
    if name.ends_with(".md") {
        return Kind::Markdown;
    }
    if name.ends_with(".nix") {
        return Kind::Nix;
    }
    if name.ends_with(".rs") {
        return Kind::Rust;
    }
    let shell_file = [".sh", ".bash", ".zsh", ".Dockerfile"]
        .iter()
        .any(|e| name.ends_with(e))
        || name.starts_with("Dockerfile")
        || path.starts_with(".github/workflows/");
    let shell_shebang = shebang_words(first).iter().any(|w| {
        SHELLS.contains(&base(w))
            || ["bash", "zsh", "dash", "ksh"]
                .iter()
                .any(|s| w.ends_with(s))
    });
    if shell_file || shell_shebang {
        Kind::Shell
    } else {
        Kind::Other
    }
}

fn is_python_file(path: &str) -> bool {
    EXTENSIONS.iter().any(|e| path.ends_with(e)) || path.split('/').any(|c| c == "__pycache__")
}

/// The files the manifest pins, as repository paths.
pub fn pinned(manifest: &str) -> BTreeSet<String> {
    sources::line_reader(manifest)
        .into_iter()
        .filter(|e| !e.file.is_empty())
        .map(|e| format!("{SOURCES}{}", e.file))
        .collect()
}

/// Every use in one file. A file that is exempt from the reading is still
/// held to the path check.
pub fn scan(path: &str, bytes: &[u8], pinned: &BTreeSet<String>) -> Vec<Finding> {
    let mut scan = Scan {
        path,
        found: Vec::new(),
    };
    if is_python_file(path) {
        scan.push(Line(0), Form::File, path);
        return scan.found;
    }
    let recording = path.starts_with("tests/") && path.ends_with(".raw");
    let binary = bytes[..bytes.len().min(BINARY_PROBE)].contains(&0);
    if OWN.contains(&path) || pinned.contains(path) || recording || binary {
        return scan.found;
    }
    let text = String::from_utf8_lossy(bytes);
    let first = text.lines().next().unwrap_or("");
    if shebang_words(first).iter().any(|w| {
        let w = w.trim_matches(|c| matches!(c, '\'' | '"'));
        is_python(w.rsplit("\\_").next().unwrap_or(w))
    }) {
        scan.push(Line(1), Form::Shebang, first);
    }
    match kind(path, first) {
        Kind::Shell => scan.shell(&text),
        Kind::Markdown => scan.markdown(&text),
        Kind::Nix => scan.nix(&text),
        Kind::Rust => scan.rust(&text),
        Kind::Other => scan.other(&text),
    }
    // One finding a line: the first rule found to break it.
    scan.found.sort_by_key(|f| f.line);
    scan.found.dedup_by_key(|f| f.line);
    scan.found
}

/// What `git ls-files` lists at `root`, whatever repository a hook's
/// environment names.
pub fn tracked(root: &Path) -> io::Result<Vec<String>> {
    let mut git = Command::new("git");
    git.current_dir(root).args(["ls-files", "-z"]);
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
        git.env_remove(v);
    }
    let out = git.output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git ls-files: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .split_terminator('\0')
        .map(str::to_string)
        .collect())
}

/// Every tracked file read; 1 on any use, each printed with its line.
pub fn run(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    let files = tracked(root)?;
    let manifest = fs::read(root.join(MANIFEST)).unwrap_or_default();
    let pinned = pinned(&String::from_utf8_lossy(&manifest));
    let mut uses = 0usize;
    let mut read = 0usize;
    for path in &files {
        let full = root.join(path);
        // A path git lists can be a link, a submodule or deleted but not yet
        // staged: only a regular file has lines to read. Its name still does.
        let regular = fs::symlink_metadata(&full).is_ok_and(|m| m.is_file());
        let bytes = if regular {
            read += 1;
            fs::read(&full)?
        } else {
            Vec::new()
        };
        for f in scan(path, &bytes, &pinned) {
            uses += 1;
            writeln!(
                out,
                "  FAIL {}:{}: {}: {}",
                f.path,
                f.line,
                f.form.name(),
                f.text
            )?;
        }
    }
    if uses == 0 {
        writeln!(
            out,
            "no-python: none in {read} files read of {} tracked",
            files.len()
        )?;
        Ok(Outcome::Passed)
    } else {
        writeln!(out, "no-python: {uses} found")?;
        Ok(Outcome::Failed)
    }
}
