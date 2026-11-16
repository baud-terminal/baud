//! `cargo xtask features-since`: which crates' feature arms a change can
//! break, for `cargo xtask feature-arms --since`.
//!
//! ```text
//! cargo xtask features-since <rev>
//! cargo xtask features-since --explain
//! ```
//!
//! With `<rev>`, the changed paths are those that differ between `<rev>`
//! and the working tree, committed or not, plus untracked files git does
//! not ignore: cargo checks the working tree, so an edit not yet committed
//! is a change. It prints `all<TAB><reason>` when every crate must be
//! checked, and otherwise one `<crate directory><TAB><reason>` line per
//! member a change can reach. A `<rev>` git cannot read as a commit prints
//! a FAIL line and exits 1; anything else that goes wrong prints `all`,
//! because a scope it cannot compute is no scope.
//!
//! A crate is reached when a changed path is in its directory, is a file
//! it reads at compile time from outside that directory, or is in a crate
//! it depends on, transitively and of every kind, since the arms build
//! `--all-targets`. `--explain` prints what the scan below finds for each
//! member.
//!
//! **The compile-time reads are scanned from the sources, not listed.** A
//! crate reads outside its own directory, so it does not depend only on
//! its dependencies: baud-bench compiles baud-app's `trail.rs` through
//! `#[path]` without depending on baud-app, and baud-domain, baud-render
//! and baud-app include files from the repository's tests/, assets/ and
//! docs/. So every Rust file of every member is read with comments and
//! literals told apart, and:
//!
//! - an `include!`, `include_bytes!` or `include_str!` names its file,
//!   relative to the file it is written in or, through
//!   `env!("CARGO_MANIFEST_DIR")`, to the crate; one through
//!   `env!("OUT_DIR")` reads what the build script wrote. One inside a
//!   `macro_rules!` body is resolved from every file that invokes the
//!   macro, and a piece given by a metavariable or by another macro is
//!   taken from the invocations or the macro's own arms, when every
//!   literal there stays below the known part of the path;
//! - a `#[path]` module, an `include!`d source and a target whose root is
//!   outside the crate are sources of the crate: the file, the module
//!   directory beside it, and their own reads, scanned the same way;
//! - any other macro or attribute given a literal with a `..` component,
//!   the form a file-reading procedural macro takes, names that path;
//! - the build script, and the modules it declares, name every literal
//!   with a `..` component, against the crate and the script's directory;
//! - a symbolic link in the crate's directory names what it points at.
//!
//! What the scan cannot resolve fails closed: an include it cannot spell,
//! and a build script that climbs with `.parent()` or `.ancestors()`, mark
//! the crate as reading anything outside the members, so any such change
//! reaches it. Reads at run time, a test opening docs/ with `std::fs`,
//! cannot break a `cargo check` and are not counted.
//!
//! **Some files reach every crate**: the lock file, the workspace
//! manifest, cargo's configuration, the toolchain's pins, the shell script
//! and the sources of this scan. The rest of xtask is not among them, so
//! other gates' lanes keep their scope: `main.rs` only dispatches here, and
//! an xtask that does not build exits nonzero, which checks every crate.
//!
//! **Order is output here.** The scan pops its queue from the end, the
//! first file to name a read gives its reason, and a change is matched
//! against the reads in the order they were found, as a plain string
//! prefix. Members keep the order `cargo metadata` lists them in.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::process::Command;
use std::rc::Rc;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde_json::Value;

use crate::py;
use crate::py_path::{basename, dirname, join, normpath, realpath, relpath, stem};
use crate::text;

const EVERYTHING_FILES: [&str; 15] = [
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    ".envrc",
    "devenv.nix",
    "devenv.lock",
    "devenv.yaml",
    "flake.nix",
    "flake.lock",
    "xtask/src/feature_arms.rs",
    "xtask/src/features_since.rs",
    "xtask/src/py.rs",
    "xtask/src/py_path.rs",
    "xtask/src/text.rs",
];
const EVERYTHING_DIRS: [&str; 2] = [".cargo/", "nix/"];

const INCLUDES: [&str; 3] = ["include", "include_bytes", "include_str"];
/// Macros whose arguments are evaluated at run time or are text: a literal
/// given to one names no file the compiler reads. A nested include is
/// found on its own.
const RUNTIME_MACROS: [&str; 28] = [
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "dbg",
    "eprint",
    "eprintln",
    "format",
    "format_args",
    "panic",
    "print",
    "println",
    "todo",
    "unimplemented",
    "unreachable",
    "vec",
    "write",
    "writeln",
    "matches",
    "concat",
    "stringify",
    "env",
    "option_env",
    "cfg",
    "compile_error",
    "macro_rules",
];

/// Python's `\s` on a `str`: `str.isspace()` also holds for the four
/// information separators, which this crate's `\s` leaves out.
const S: &str = r"[\s\x1c-\x1f]";

fn compile(pattern: &str) -> Regex {
    let spelled = pattern.replace("~", S);
    Regex::new(&spelled).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

macro_rules! pattern {
    ($name:ident, $p:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| compile($p));
    };
}

// Each is the script's pattern with `\s` spelled `~` for the class above,
// and `re.match` and `re.fullmatch` anchored. MACRO and RULES leave out the
// leading `\b`, which `bounded` checks.
pattern!(LIT, "\x01(\\d+)\x02");
pattern!(DIGIT, "^\\d$");
pattern!(MACRO, r"([A-Za-z_][A-Za-z0-9_]*)~*!~*([(\[{])");
pattern!(RULES, r"macro_rules~*!~*([A-Za-z_][A-Za-z0-9_]*)~*([(\[{])");
pattern!(ATTR, r"#~*!?~*\[");
pattern!(CLIMB, r"\.~*(?:parent|ancestors)~*\(");
pattern!(
    MOD,
    r"(?:^|[;{}\s\x1c-\x1f])mod~+([A-Za-z_][A-Za-z0-9_]*)~*;"
);
pattern!(
    ONLY_LITERALS,
    "^[\\s\\x1c-\\x1f,()\\[\\]{}]*(?:\x01\\d+\x02[\\s\\x1c-\\x1f,()\\[\\]{}]*)*$"
);
// `macro_export\b[^\]]*\]`, with Python's boundary: what follows the name
// is the bracket, or a character that is no word's and anything but it.
pattern!(
    EXPORT,
    r"#~*\[~*macro_export(?:[^\p{L}\p{N}_\]][^\]]*)?\]~*$"
);
pattern!(IS_PATH, r"(?s)^~*(?:cfg_attr~*\(.*,~*)?path~*=");
pattern!(EMPTY_CALL, r"^([A-Za-z_][A-Za-z0-9_]*)~*!~*[(\[{]~*[)\]}]$");
pattern!(ARM, r"=>~*[(\[{]");
pattern!(CONCAT, r"^concat~*!~*[(\[{]");
pattern!(ONE_LIT, "^\x01(\\d+)\x02$");
pattern!(ENV, "^env~*!~*[(\\[{]~*\x01(\\d+)\x02~*,?~*[)\\]}]$");
pattern!(ANY_CALL, r"(?s)^[A-Za-z_][A-Za-z0-9_]*~*!~*[(\[{].*$");

/// What stopped the scan, as `<exception>: <message>`. On a `<rev>` it
/// becomes the `all` line's reason; on `--explain`, the error it exits on.
#[derive(Debug)]
pub struct Failure(pub String);

type Scanned<T> = Result<T, Failure>;

/// `lits[k]`, for a `k` read back out of the code: bytes in a source that
/// spell a placeholder can name a literal that does not exist.
fn lit_at<'a>(lits: &'a [String], k: &str) -> Scanned<&'a str> {
    let at = k.chars().try_fold(0usize, |n, c| {
        n.checked_mul(10)?.checked_add(digit_value(c).0)
    });
    at.and_then(|k| lits.get(k))
        .map(String::as_str)
        .ok_or_else(|| Failure("IndexError: list index out of range".to_string()))
}

/// What `int()` reads a character of `\d` as. Each run of the category Nd
/// is whole sets of ten that start at zero, which holds for all 680 of them
/// in Python's table.
fn digit_value(c: char) -> Points {
    let mut at = u32::from(c);
    let mut past_zero = 0;
    while let Some(before) = at.checked_sub(1).and_then(char::from_u32) {
        let mut b = [0; 4];
        if !DIGIT.is_match(before.encode_utf8(&mut b)) {
            break;
        }
        at -= 1;
        past_zero += 1;
    }
    Points(past_zero % 10)
}

/// A source with its comments removed and each string literal replaced by
/// `\x01<n>\x02`, and the literals' values in order.
pub struct Lexed {
    /// The code.
    pub code: String,
    /// The values, escapes undone only for `\\` and `\"`.
    pub lits: Vec<String>,
}

/// A position in the text or a length, in code points.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Points(usize);

/// How many macro invocations a search has followed outward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Levels(usize);

/// What opens at a position of the text.
enum Token {
    Line,
    Block,
    Quote,
    Str,
    /// A raw string: where its text starts, and its hashes.
    Raw(Points, Points),
}

fn word_ascii(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `//|/\*|"|'|(?<![A-Za-z0-9_])[bc]?r#*"` at one position: the regex crate
/// has no look-behind, so the position is tried by hand, left to right.
fn token(t: &[char], i: Points) -> Option<Token> {
    let i = i.0;
    match t[i] {
        '/' => match t.get(i + 1) {
            Some('/') => Some(Token::Line),
            Some('*') => Some(Token::Block),
            _ => None,
        },
        '"' => Some(Token::Str),
        '\'' => Some(Token::Quote),
        'b' | 'c' | 'r' => {
            if i > 0 && word_ascii(t[i - 1]) {
                return None;
            }
            let r = if t[i] == 'r' { i } else { i + 1 };
            if t.get(r) != Some(&'r') {
                return None;
            }
            let hashes = t[r + 1..].iter().take_while(|c| **c == '#').count();
            (t.get(r + 1 + hashes) == Some(&'"'))
                .then(|| Token::Raw(Points(r + 2 + hashes), Points(hashes)))
        }
        _ => None,
    }
}

fn find(t: &[char], c: char, from: Points) -> Option<Points> {
    let found = t.get(from.0..)?.iter().position(|x| *x == c);
    found.map(|j| Points(j + from.0))
}

/// Reads the text as code points, as the script did: `'é'` is a character
/// literal because the quote two code points on closes it.
pub fn lex(text: &str) -> Lexed {
    let t: Vec<char> = text.chars().collect();
    let n = t.len();
    let mut code = String::with_capacity(text.len());
    let mut lits: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let Some(tok) = token(&t, Points(i)) else {
            code.push(t[i]);
            i += 1;
            continue;
        };
        match tok {
            Token::Line => i = find(&t, '\n', Points(i)).map_or(n, |j| j.0),
            Token::Block => {
                let mut depth = 1;
                i += 2;
                while i < n && depth > 0 {
                    if t[i] == '/' && t.get(i + 1) == Some(&'*') {
                        depth += 1;
                        i += 2;
                    } else if t[i] == '*' && t.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                code.push(' ');
            }
            // A character literal, which may hold a quote, or a lifetime.
            Token::Quote => {
                if t.get(i + 1) == Some(&'\\') {
                    i = find(&t, '\'', Points(i + 3)).map_or(n, |j| j.0 + 1);
                    code.push_str("' '");
                } else if t.get(i + 2) == Some(&'\'') {
                    i += 3;
                    code.push_str("' '");
                } else {
                    i += 1;
                    code.push('\'');
                }
            }
            Token::Str => {
                let mut j = i + 1;
                let mut value = String::new();
                while j < n && t[j] != '"' {
                    if t[j] == '\\' && j + 1 < n {
                        if !matches!(t[j + 1], '\\' | '"') {
                            value.push('\\');
                        }
                        value.push(t[j + 1]);
                        j += 2;
                    } else {
                        value.push(t[j]);
                        j += 1;
                    }
                }
                code.push_str(&format!("\x01{}\x02", lits.len()));
                lits.push(value);
                i = j + 1;
            }
            Token::Raw(Points(start), Points(hashes)) => {
                let closes = |j: usize| {
                    t[j] == '"'
                        && t.len() > j + hashes
                        && t[j + 1..=j + hashes].iter().all(|c| *c == '#')
                };
                let j = (start..n).find(|j| closes(*j)).unwrap_or(n);
                code.push_str(&format!("\x01{}\x02", lits.len()));
                lits.push(t[start..j].iter().collect());
                i = j + 1 + hashes;
            }
        }
    }
    Lexed { code, lits }
}

/// A byte offset into lexed code; the brackets it finds are ASCII.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct At(pub usize);

/// The index of the bracket closing the one at `open_at`, all three kinds
/// counted together, or the code's end.
pub fn closing(code: &str, open_at: At) -> At {
    let mut depth = 0;
    for (j, b) in code.bytes().enumerate().skip(open_at.0) {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return At(j);
                }
            }
            _ => {}
        }
    }
    At(code.len())
}

/// `args` split at its top-level commas, each piece stripped, the empty
/// left out.
pub fn split_top(args: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0, 0);
    for (j, b) in args.bytes().enumerate() {
        match b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(&args[start..j]);
                start = j + 1;
            }
            _ => {}
        }
    }
    parts.push(&args[start..]);
    parts
        .into_iter()
        .map(text::strip)
        .filter(|p| !p.is_empty())
        .collect()
}

fn climbs(lit: &str) -> bool {
    lit.replace('\\', "/").split('/').any(|c| c == "..")
}

fn under(path: &str, d: &str) -> bool {
    d.is_empty() || path == d || path.starts_with(&format!("{d}/"))
}

/// A repository path, `""` for its root, or `None` when it leaves it.
pub fn resolve(base: &str, lit: &str) -> Option<String> {
    let p = normpath(&join(base, lit));
    if p.starts_with('/') || p == ".." || p.starts_with("../") {
        return None;
    }
    Some(if p == "." { String::new() } else { p })
}

fn stem_of(path: &str) -> String {
    let s = path.strip_suffix(".rs").unwrap_or(path);
    if basename(s) == "mod" {
        dirname(s)
    } else {
        s.to_string()
    }
}

fn is_file(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file())
}

fn is_dir(path: &str) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_dir())
}

fn is_link(path: &str) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Every file under `d`, symbolic links to directories followed once and
/// listed, `target` and `.git` left out, as `os.walk` gives them: the
/// files of a directory in order, its linked directories, then each of its
/// directories in order.
pub fn walk(d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    descend(if d.is_empty() { "." } else { d }, &mut seen, &mut out);
    out
}

fn descend(top: &str, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(top) else {
        return;
    };
    let (mut dirs, mut files) = (Vec::new(), Vec::new());
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if is_dir(&join(top, &name)) {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    if !seen.insert(realpath(top)) {
        return;
    }
    dirs.retain(|x| x != "target" && x != ".git");
    dirs.sort();
    files.sort();
    let links = dirs.iter().filter(|x| is_link(&join(top, x)));
    for f in files.iter().chain(links) {
        out.push(normpath(&join(top, f)));
    }
    for x in &dirs {
        descend(&join(top, x), seen, out);
    }
}

/// `\b` before a match that starts on an ASCII word character, with
/// Python's word characters.
fn bounded(code: &str, at: At) -> bool {
    code[..at.0]
        .chars()
        .next_back()
        .is_none_or(|c| !py::word(c))
}

/// Every match of a pattern written without its leading `\b`, as Python's
/// `finditer` gives them with it. A start the boundary refuses is tried
/// again one character on, where it refuses again until the name is past.
fn finditer_bounded<'c>(re: &Regex, code: &'c str) -> Vec<Captures<'c>> {
    let (mut out, mut from) = (Vec::new(), 0);
    while let Some(c) = re.captures_at(code, from) {
        let whole = c.get(0).map_or(0..0, |m| m.range());
        if bounded(code, At(whole.start)) {
            from = whole.end;
            out.push(c);
        } else {
            from = whole.start + 1;
        }
    }
    out
}

/// Whether a file is scanned as a build script's.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Role {
    Plain,
    Build,
}

/// Whether a `macro_rules!` is `#[macro_export]`ed, so invocations this
/// crate cannot see can spell its pieces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exported {
    No,
    Yes,
}

struct Rule {
    path: String,
    start: At,
    end: At,
    exported: Exported,
}

struct Call {
    path: String,
    pos: At,
    args: String,
}

struct Include {
    path: String,
    pos: At,
    name: String,
    args: String,
}

/// How many times the includes have been resolved.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pass {
    First,
    Again,
}

/// One workspace member and what the scan found of it.
pub struct Member {
    dir: String,
    name: String,
    /// Member directories.
    deps: BTreeSet<String>,
    /// Repository path prefix, and the file that names it, in the order
    /// they were found.
    reads: Vec<(String, String)>,
    reads_seen: HashSet<String>,
    /// Why the scan could not resolve a read.
    anything: Vec<String>,
    files: HashMap<String, Rc<Lexed>>,
    /// Macro name and its definitions, in the order the names were found.
    rules: Vec<(String, Vec<Rule>)>,
    calls: HashMap<String, Vec<Call>>,
    /// Popped from the end.
    queue: Vec<(String, Role)>,
    done: HashSet<(String, Role)>,
    /// Includes, resolved once every file is read.
    later: Vec<Rc<Include>>,
    pass: Pass,
}

impl Member {
    fn new(dir: String, name: String) -> Self {
        Self {
            dir,
            name,
            deps: BTreeSet::new(),
            reads: Vec::new(),
            reads_seen: HashSet::new(),
            anything: Vec::new(),
            files: HashMap::new(),
            rules: Vec::new(),
            calls: HashMap::new(),
            queue: Vec::new(),
            done: HashSet::new(),
            later: Vec::new(),
            pass: Pass::First,
        }
    }

    /// A reason the scan cannot follow. The script's list held a file once
    /// per scan of it and so does this one; only a second pass, below,
    /// says nothing twice.
    fn cannot(&mut self, reason: String) {
        if self.pass == Pass::First || !self.anything.contains(&reason) {
            self.anything.push(reason);
        }
    }

    fn add_read(&mut self, target: String, why: String) {
        if self.reads_seen.insert(target.clone()) {
            self.reads.push((target, why));
        }
    }

    fn read(&mut self, target: Option<String>, why: &str) {
        let Some(target) = target else { return };
        if target.is_empty() {
            self.cannot(format!("{why} names the repository root"));
        } else if !under(&target, &self.dir) {
            self.add_read(target, why.to_string());
        }
    }

    /// A Rust file compiled into this crate, wherever it is.
    fn source(&mut self, target: Option<String>, why: &str) {
        let Some(target) = target else { return };
        if target.is_empty() {
            self.cannot(format!("{why} names the repository root"));
            return;
        }
        if !under(&target, &self.dir) {
            let s = stem_of(&target);
            self.add_read(s.clone(), why.to_string());
            if is_dir(&s) {
                let found = walk(&s).into_iter().filter(|f| f.ends_with(".rs"));
                self.queue.extend(found.map(|f| (f, Role::Plain)));
            }
        }
        self.queue.push((target, Role::Plain));
    }

    fn lexed(&mut self, path: &str) -> Scanned<Rc<Lexed>> {
        if let Some(l) = self.files.get(path) {
            return Ok(Rc::clone(l));
        }
        let raw = fs::read(path).map_err(|e| os_error(&e, path))?;
        let decoded = String::from_utf8_lossy(&raw);
        let l = Rc::new(lex(&text::universal(&decoded)));
        self.files.insert(path.to_string(), Rc::clone(&l));
        Ok(l)
    }

    fn rule_entry(&mut self, name: &str) -> &mut Vec<Rule> {
        let at = self.rules.iter().position(|(n, _)| n == name);
        let at = at.unwrap_or_else(|| {
            self.rules.push((name.to_string(), Vec::new()));
            self.rules.len() - 1
        });
        &mut self.rules[at].1
    }

    fn scan(&mut self, path: &str, role: Role) -> Scanned<()> {
        if self.done.contains(&(path.to_string(), role)) || !is_file(path) {
            return Ok(());
        }
        self.done.insert((path.to_string(), role));
        let lexed = self.lexed(path)?;
        let (code, lits) = (lexed.code.as_str(), &lexed.lits);
        let here = dirname(path);
        let dir = self.dir.clone();
        for r in finditer_bounded(&RULES, code) {
            let whole = r.get(0).map_or(0..0, |m| m.range());
            let open = r.get(2).map_or(0, |m| m.start());
            let exported = if EXPORT.is_match(&code[..whole.start]) {
                Exported::Yes
            } else {
                Exported::No
            };
            let rule = Rule {
                path: path.to_string(),
                start: At(whole.start),
                end: closing(code, At(open)),
                exported,
            };
            self.rule_entry(&r[1]).push(rule);
        }
        for mac in finditer_bounded(&MACRO, code) {
            let name = &mac[1];
            let start = mac.get(0).map_or(0, |m| m.start());
            let open = mac.get(2).map_or(0, |m| m.start());
            let args = &code[open + 1..closing(code, At(open)).0];
            self.calls.entry(name.to_string()).or_default().push(Call {
                path: path.to_string(),
                pos: At(start),
                args: args.to_string(),
            });
            if INCLUDES.contains(&name) {
                self.later.push(Rc::new(Include {
                    path: path.to_string(),
                    pos: At(start),
                    name: name.to_string(),
                    args: args.to_string(),
                }));
            } else if !RUNTIME_MACROS.contains(&name) {
                for k in LIT.captures_iter(args) {
                    let lit = lit_at(lits, &k[1])?;
                    if climbs(lit) {
                        for base in [here.as_str(), dir.as_str()] {
                            self.read(resolve(base, lit), &format!("{path} ({name}!)"));
                        }
                    }
                }
            }
        }
        for att in ATTR.find_iter(code) {
            let open = att.end() - 1;
            let body = &code[open + 1..closing(code, At(open)).0];
            let is_path = IS_PATH.is_match(body);
            for k in LIT.captures_iter(body) {
                let lit = lit_at(lits, &k[1])?;
                if is_path {
                    let inner = join(&here, stem(basename(path)));
                    for base in [here.as_str(), inner.as_str()] {
                        let target = resolve(base, lit);
                        let why = format!("{path} (#[path])");
                        match target {
                            Some(t) if is_file(&t) => self.source(Some(t), &why),
                            Some(t) if !under(&t, &self.dir) => self.read(Some(t), &why),
                            _ => {}
                        }
                    }
                } else if climbs(lit) {
                    for base in [here.as_str(), dir.as_str()] {
                        self.read(resolve(base, lit), &format!("{path} (attribute)"));
                    }
                }
            }
        }
        if role == Role::Build {
            if CLIMB.is_match(code) {
                self.cannot(format!("{path} climbs with .parent() or .ancestors()"));
            }
            for lit in lits.iter().filter(|l| climbs(l)) {
                for base in [dir.as_str(), here.as_str()] {
                    self.read(resolve(base, lit), &format!("{path} (build script)"));
                }
            }
            for m in MOD.captures_iter(code) {
                let module = &m[1];
                for f in [
                    join(&here, &format!("{module}.rs")),
                    join(&join(&here, module), "mod.rs"),
                ] {
                    self.queue.push((normpath(&f), Role::Build));
                }
            }
        }
        Ok(())
    }

    /// The `macro_rules!` definitions whose bodies hold `pos` in `path`:
    /// each one's name and whether it is exported.
    fn enclosing(&self, path: &str, pos: At) -> Vec<(String, Exported)> {
        let mut out = Vec::new();
        for (name, defs) in &self.rules {
            for d in defs {
                if d.path == path && d.start < pos && pos < d.end {
                    out.push((name.clone(), d.exported));
                }
            }
        }
        out
    }

    /// The directories of the files a macro is expanded from.
    fn callers(&self, name: &str, depth: Levels) -> BTreeSet<String> {
        let mut dirs = BTreeSet::new();
        for c in self.calls.get(name).map_or(&[][..], Vec::as_slice) {
            let outer = self.enclosing(&c.path, c.pos);
            if !outer.is_empty() && depth.0 < 8 {
                for (o, _) in outer {
                    dirs.extend(self.callers(&o, Levels(depth.0 + 1)));
                }
            } else {
                dirs.insert(dirname(&c.path));
            }
        }
        dirs
    }

    fn file(&self, path: &str) -> Scanned<&Lexed> {
        self.files
            .get(path)
            .map(Rc::as_ref)
            .ok_or_else(|| Failure(format!("KeyError: {}", py::repr_str(path))))
    }

    /// Whether what a metavariable or macro call spells has no `..`.
    fn pieces_stay(&self, piece: &str, path: &str, pos: At) -> Scanned<Stays> {
        if piece.starts_with('$') {
            let outer = self.enclosing(path, pos);
            for (name, exported) in &outer {
                if *exported == Exported::Yes {
                    return Ok(Stays::No);
                }
                for c in self.calls.get(name).map_or(&[][..], Vec::as_slice) {
                    let lits = &self.file(&c.path)?.lits;
                    if stays(&c.args, lits)? == Stays::No {
                        return Ok(Stays::No);
                    }
                }
            }
            return Ok(if outer.is_empty() {
                Stays::No
            } else {
                Stays::Yes
            });
        }
        let Some(call) = EMPTY_CALL.captures(piece) else {
            return Ok(Stays::No);
        };
        let Some((_, defs)) = self.rules.iter().find(|(n, _)| n == &call[1]) else {
            return Ok(Stays::No);
        };
        for d in defs {
            let lexed = self.file(&d.path)?;
            let code = lexed.code.as_str();
            for arm in ARM.find_iter(&code[d.start.0..d.end.0]) {
                let open = d.start.0 + arm.end() - 1;
                let body = &code[open + 1..closing(code, At(open)).0];
                if stays(body, &lexed.lits)? == Stays::No {
                    return Ok(Stays::No);
                }
            }
        }
        Ok(Stays::Yes)
    }

    fn include(&mut self, inc: &Include) -> Scanned<()> {
        let lexed = Rc::clone(
            self.files
                .get(&inc.path)
                .ok_or_else(|| Failure(format!("KeyError: {}", py::repr_str(&inc.path))))?,
        );
        let lits = &lexed.lits;
        let why = format!("{} ({}!)", inc.path, inc.name);
        let mut parts = split_top(&inc.args);
        while parts.len() == 1 && CONCAT.is_match(parts[0]) {
            parts = split_top(inside(parts[0]));
        }
        let mut flat = Vec::new();
        for part in parts {
            if CONCAT.is_match(part) {
                flat.extend(split_top(inside(part)));
            } else {
                flat.push(part);
            }
        }
        let (mut known, mut unknown, mut tail_climbs, mut env_base) =
            (String::new(), Vec::new(), Tail::Stays, Base::File);
        for (i, part) in flat.iter().enumerate() {
            let lit = ONE_LIT.captures(part);
            if let Some(env) = ENV.captures(part) {
                let var = lit_at(lits, &env[1])?;
                if var == "OUT_DIR" && i == 0 {
                    return Ok(());
                }
                if var != "CARGO_MANIFEST_DIR" || i != 0 {
                    self.cannot(format!("{why} spells its path with env!({var})"));
                    return Ok(());
                }
                env_base = Base::Crate;
            } else if let Some(lit) = &lit {
                let value = lit_at(lits, &lit[1])?;
                if unknown.is_empty() {
                    known.push_str(value);
                } else if climbs(value) {
                    tail_climbs = Tail::Climbs;
                }
            } else if part.starts_with('$') || ANY_CALL.is_match(part) {
                unknown.push(*part);
            } else {
                let spelled: String = py::split(part).join(" ").chars().take(40).collect();
                self.cannot(format!("{why} spells its path with {spelled}"));
                return Ok(());
            }
        }
        if !unknown.is_empty() {
            let mut stays = tail_climbs == Tail::Stays;
            for u in &unknown {
                if !stays {
                    break;
                }
                stays = self.pieces_stay(u, &inc.path, inc.pos)? == Stays::Yes;
            }
            if !stays {
                let spelled = unknown.join(" ");
                self.cannot(format!("{why} spells its path with {spelled}"));
                return Ok(());
            }
        }
        let bases: BTreeSet<String> = if env_base == Base::Crate {
            known = known.trim_start_matches('/').to_string();
            BTreeSet::from([self.dir.clone()])
        } else {
            let mut b = BTreeSet::from([dirname(&inc.path)]);
            for (o, _) in self.enclosing(&inc.path, inc.pos) {
                b.extend(self.callers(&o, Levels(0)));
            }
            b
        };
        for base in &bases {
            let target = resolve(base, &known);
            if inc.name == "include" && unknown.is_empty() {
                self.source(target, &why);
            } else {
                self.read(target, &why);
            }
        }
        Ok(())
    }

    /// Until no include brings in a file not yet read, since a file read
    /// late can hold an invocation an earlier include is resolved from.
    ///
    /// **It stops when nothing new is queued, where the script's loop did
    /// not stop while anything was**: an `include!` whose file is found
    /// queues that file on every pass, so the script never returned on a
    /// tree holding one. On a tree it returned on, this is one pass.
    fn run(&mut self) -> Scanned<()> {
        loop {
            while let Some((path, role)) = self.queue.pop() {
                self.scan(&path, role)?;
            }
            for inc in self.later.clone() {
                self.include(&inc)?;
            }
            let done = &self.done;
            self.queue
                .retain(|(p, r)| !done.contains(&(p.clone(), *r)) && is_file(p));
            if self.queue.is_empty() {
                break;
            }
            self.pass = Pass::Again;
        }
        for f in walk(&self.dir) {
            if is_link(&f) {
                if let Ok(to) = fs::read_link(&f) {
                    let to = to.to_string_lossy();
                    self.read(resolve(&dirname(&f), &to), &format!("{f} (symbolic link)"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stays {
    No,
    Yes,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tail {
    Stays,
    Climbs,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Base {
    File,
    Crate,
}

/// Whether every literal of macro arguments stays below the known part.
fn stays(args: &str, lits: &[String]) -> Scanned<Stays> {
    if !ONLY_LITERALS.is_match(args) {
        return Ok(Stays::No);
    }
    for k in LIT.captures_iter(args) {
        if climbs(lit_at(lits, &k[1])?) {
            return Ok(Stays::No);
        }
    }
    Ok(Stays::Yes)
}

/// What a call's brackets hold: `s[s.index("!") + 1:].strip()[1:-1]`.
fn inside(call: &str) -> &str {
    let after = call.find('!').map_or(call, |i| &call[i + 1..]);
    let mut chars = text::strip(after).chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// An `OSError` as Python prints it.
fn os_error(e: &io::Error, name: &str) -> Failure {
    let kind = match e.kind() {
        io::ErrorKind::NotFound => "FileNotFoundError",
        io::ErrorKind::PermissionDenied => "PermissionError",
        io::ErrorKind::IsADirectory => "IsADirectoryError",
        io::ErrorKind::NotADirectory => "NotADirectoryError",
        _ => "OSError",
    };
    let shown = e.to_string();
    let errno = e.raw_os_error().unwrap_or(0);
    let plain = shown
        .strip_suffix(&format!(" (os error {errno})"))
        .unwrap_or(&shown);
    Failure(format!(
        "{kind}: [Errno {errno}] {plain}: {}",
        py::repr_str(name)
    ))
}

/// A command's standard output, or what `subprocess.run(check=True)`
/// raised on it.
fn output(cmd: &str, args: &[&str]) -> Scanned<Vec<u8>> {
    let out = Command::new(cmd)
        .args(args)
        .output()
        .map_err(|e| os_error(&e, cmd))?;
    if out.status.success() {
        return Ok(out.stdout);
    }
    let spelled: Vec<String> = std::iter::once(cmd)
        .chain(args.iter().copied())
        .map(py::repr_str)
        .collect();
    let status = out
        .status
        .code()
        .map_or_else(|| "died of a signal".to_string(), |c| c.to_string());
    Err(Failure(format!(
        "CalledProcessError: Command '[{}]' returned non-zero exit status {status}.",
        spelled.join(", ")
    )))
}

fn key<'a>(v: &'a Value, k: &str) -> Scanned<&'a Value> {
    v.get(k)
        .ok_or_else(|| Failure(format!("KeyError: {}", py::repr_str(k))))
}

fn text_of<'a>(v: &'a Value, k: &str) -> Scanned<&'a str> {
    key(v, k)?
        .as_str()
        .ok_or_else(|| Failure(format!("TypeError: {k} is not a string")))
}

fn list_of<'a>(v: &'a Value, k: &str) -> Scanned<&'a [Value]> {
    key(v, k)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| Failure(format!("TypeError: {k} is not a list")))
}

fn load() -> Scanned<Vec<Member>> {
    let raw = output("cargo", &["metadata", "--no-deps", "--format-version", "1"])?;
    let meta: Value =
        serde_json::from_slice(&raw).map_err(|e| Failure(format!("JSONDecodeError: {e}")))?;
    let root = realpath(text_of(&meta, "workspace_root")?);
    if root != realpath(".") {
        return Err(Failure(format!(
            "RuntimeError: cargo's workspace root {root} is not the checkout"
        )));
    }
    let rel = |p: &str| relpath(&realpath(p), &root);
    let packages = list_of(&meta, "packages")?;
    let mut members = Vec::new();
    let mut index = HashMap::new();
    for p in packages {
        let d = rel(&dirname(text_of(p, "manifest_path")?));
        index.insert(d.clone(), members.len());
        members.push(Member::new(d, text_of(p, "name")?.to_string()));
    }
    for p in packages {
        let d = rel(&dirname(text_of(p, "manifest_path")?));
        let m = &mut members[index[&d]];
        for dep in list_of(p, "dependencies")? {
            let Some(path) = dep.get("path").and_then(Value::as_str) else {
                continue;
            };
            if path.is_empty() {
                continue;
            }
            let to = rel(path);
            if index.contains_key(&to) {
                m.deps.insert(to);
            } else {
                let why = format!("{}/Cargo.toml (path dependency)", m.dir);
                m.read(Some(to), &why);
            }
        }
        for t in list_of(p, "targets")? {
            let src = rel(text_of(t, "src_path")?);
            let build = list_of(t, "kind")?
                .iter()
                .any(|k| k.as_str() == Some("custom-build"));
            if build {
                m.queue.push((src, Role::Build));
            } else if !under(&src, &m.dir) {
                let why = format!("{}/Cargo.toml (target {})", m.dir, text_of(t, "name")?);
                m.source(Some(src), &why);
            }
        }
        let found = walk(&m.dir).into_iter().filter(|f| f.ends_with(".rs"));
        let found: Vec<_> = found.map(|f| (f, Role::Plain)).collect();
        m.queue.extend(found);
        m.run()?;
    }
    Ok(members)
}

/// Where a byte of a path that is not UTF-8 is kept, as `os.fsdecode` keeps
/// it in a lone surrogate: a `char` cannot be one, so the last 256 code
/// points of the private use plane stand in.
const ESCAPED: u32 = 0x10_ff00;

fn fsdecode(path: &[u8]) -> String {
    let mut out = String::new();
    for chunk in path.utf8_chunks() {
        out.push_str(chunk.valid());
        out.extend(
            chunk
                .invalid()
                .iter()
                .filter_map(|b| char::from_u32(ESCAPED + u32::from(*b))),
        );
    }
    out
}

/// Whether a line holds such a byte. Python's `print` cannot encode a
/// surrogate, so the script died there, at exit 1, with the lines before it
/// printed; where no such path is printed, it ran on.
fn escaped(line: &str) -> bool {
    line.chars().any(|c| u32::from(c) >= ESCAPED)
}

fn changed(rev: &str) -> Scanned<Vec<String>> {
    let mut raw = output(
        "git",
        &["diff", "--name-only", "--no-renames", "-z", rev, "--"],
    )?;
    raw.extend(output(
        "git",
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?);
    let mut paths = BTreeSet::new();
    for p in raw.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        paths.insert(fsdecode(p));
    }
    Ok(paths.into_iter().collect())
}

/// What is reached, as `(directory or "all", reason)`.
fn scope(rev: &str) -> Scanned<Vec<(String, String)>> {
    let paths = changed(rev)?;
    if let Some(p) = everything(&paths) {
        return Ok(vec![(
            "all".to_string(),
            format!("{p} changed, so every crate is checked"),
        )]);
    }
    let members = load()?;
    Ok(reach_in(&paths, &members)
        .into_iter()
        .map(|(dir, _, why)| (dir, why))
        .collect())
}

/// The first of `paths` that reaches every crate.
pub fn everything(paths: &[String]) -> Option<&String> {
    paths.iter().find(|p| {
        EVERYTHING_FILES.contains(&p.as_str()) || EVERYTHING_DIRS.iter().any(|d| p.starts_with(d))
    })
}

/// The workspace scanned once, to ask what several changes reach.
pub struct Scan(Vec<Member>);

impl Scan {
    /// Scans the workspace whose root is the current directory; `None`
    /// when the scan cannot be computed.
    pub fn load() -> Option<Self> {
        load().ok().map(Scan)
    }

    /// The members a change to `paths` reaches, as `(directory, package,
    /// reason)` sorted by directory: those a path is in, those that read
    /// one at compile time (`reads <path> at compile time, in <file>`), and
    /// every member that builds from one of them. `None` when a path
    /// reaches every crate.
    pub fn reach(&self, paths: &[String]) -> Option<Vec<(String, String, String)>> {
        if everything(paths).is_some() {
            return None;
        }
        Some(reach_in(paths, &self.0))
    }
}

fn reach_in(paths: &[String], members: &[Member]) -> Vec<(String, String, String)> {
    let outside: Vec<&String> = paths
        .iter()
        .filter(|p| !members.iter().any(|m| under(p, &m.dir)))
        .collect();
    let mut why: HashMap<String, String> = HashMap::new();
    for m in members {
        for p in paths {
            if under(p, &m.dir) {
                why.insert(m.dir.clone(), format!("changed: {p}"));
                break;
            }
            if let Some((_, src)) = m.reads.iter().find(|(r, _)| p.starts_with(r.as_str())) {
                why.insert(
                    m.dir.clone(),
                    format!("reads {p} at compile time, in {src}"),
                );
                break;
            }
        }
        if !why.contains_key(&m.dir) && !m.anything.is_empty() && !outside.is_empty() {
            why.insert(
                m.dir.clone(),
                format!(
                    "{} changed, and {}, which the scan cannot follow",
                    outside[0], m.anything[0]
                ),
            );
        }
    }
    let mut grew = true;
    while grew {
        grew = false;
        for m in members {
            if why.contains_key(&m.dir) {
                continue;
            }
            if let Some(dep) = m.deps.iter().find(|x| why.contains_key(*x)) {
                why.insert(m.dir.clone(), format!("builds from {dep}"));
                grew = true;
            }
        }
    }
    let mut lines: Vec<_> = why
        .into_iter()
        .map(|(dir, why)| {
            let name = members
                .iter()
                .find(|m| m.dir == dir)
                .map(|m| m.name.clone())
                .unwrap_or_default();
            (dir, name, why)
        })
        .collect();
    lines.sort();
    lines
}

/// What a run exits with: 0, 1 on a FAIL line or an `--explain` that could
/// not be computed, 2 on a bad command line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exit(pub u8);

fn explain(out: &mut impl Write) -> io::Result<Exit> {
    let mut members = match load() {
        Ok(m) => m,
        Err(Failure(e)) => {
            eprintln!("xtask: {e}");
            return Ok(Exit(1));
        }
    };
    members.sort_by(|a, b| a.dir.cmp(&b.dir));
    for m in &mut members {
        writeln!(out, "{} ({})", m.dir, m.name)?;
        let deps: Vec<&str> = m.deps.iter().map(String::as_str).collect();
        let deps = if deps.is_empty() {
            "-".to_string()
        } else {
            deps.join(" ")
        };
        writeln!(out, "  depends on: {deps}")?;
        m.reads.sort();
        for (r, src) in &m.reads {
            writeln!(out, "  reads {r}  <- {src}")?;
        }
        for a in &m.anything {
            writeln!(out, "  reads anything: {a}")?;
        }
    }
    Ok(Exit(0))
}

/// The command: `args` are what follows its name.
pub fn run(args: &[&str], out: &mut impl Write) -> io::Result<Exit> {
    let rev = match args {
        ["--explain"] => return explain(out),
        [rev] => *rev,
        _ => {
            writeln!(out, "usage: cargo xtask features-since <rev> | --explain")?;
            return Ok(Exit(2));
        }
    };
    let spec = format!("{rev}^{{commit}}");
    let known = Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", &spec])
        .output()?;
    if !known.status.success() {
        writeln!(out, "FAIL: --since {rev}: git knows no such commit")?;
        return Ok(Exit(1));
    }
    let lines = scope(rev).unwrap_or_else(|Failure(e)| {
        vec![(
            "all".to_string(),
            format!("the scope could not be computed ({e}), so every crate is checked"),
        )]
    });
    for (d, why) in lines {
        if escaped(&why) {
            eprintln!("xtask: a path that is not UTF-8 cannot be printed");
            return Ok(Exit(1));
        }
        writeln!(out, "{d}\t{why}")?;
    }
    Ok(Exit(0))
}
