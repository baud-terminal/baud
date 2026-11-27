//! `cargo xtask code-map --out DIR [--prune ROOT --keep N]`: a map of the
//! tree's Rust, written from the code alone, so the same tree always gives
//! the same bytes.
//!
//! **What it writes.** `index.md`, one line per tracked `.rs` file: its
//! crate, path, line count and the first sentence of its `//!` doc, or
//! "(no module doc)". `items.tsv`, one row per `pub`, `pub(crate)` or
//! `pub(super)` fn, struct, enum, trait, const, static, type alias and mod,
//! per exported `macro_rules!`, and per `impl` block, named by its target
//! type: the kind, the name, the path, `file:line`, the signature on one
//! line and the first sentence of its `///` doc. `crates.tsv`, one row per
//! workspace crate from `cargo metadata`: name, path, whether its library
//! is `no_std`, its features and the workspace crates it depends on.
//! `gates.tsv`: the perf gate's entries, the audit's budgets as
//! `audit.rs` declares them, the contract's witnessed rules and the paths
//! its `const _` lines pin. `MAP`, last: [`MARKER`], the generator's
//! version, the tree and commit it mapped, the file and item counts, and
//! one `error:` line when anything failed.
//!
//! **How Rust is read.** A line scanner, not a parser. Comments, strings
//! and character literals are blanked first, so an item inside a doc
//! example, a block comment or a string is never listed. An item head is
//! read at the top of a file, a module or an `impl` block, never inside a
//! function, trait or macro body, and its signature runs from the head to
//! the first `{` or `;` outside brackets, joined onto one line.
//!
//! **A failure never fails the run.** Whatever could not be read is named
//! on `MAP`'s `error:` line and the run exits 0, so a map never reddens a
//! gate. Only a bad command line exits 2.
//!
//! **Pruning** deletes directories directly under ROOT whose `MAP` begins
//! with [`MARKER`], all but the `--keep` newest by `MAP`'s modification
//! time, and never one a symbolic link in ROOT points at.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::features_since::Exit;

/// The first line of every `MAP` the generator writes, and how pruning
/// knows one.
pub const MARKER: &str = "baud code map";

/// The format of the files, raised when a column changes.
pub const VERSION: &str = "1";

/// The text `index.md` gives a file with no `//!` doc.
pub const NO_MODULE_DOC: &str = "(no module doc)";

/// A line of a file, from 1.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Line(pub usize);

/// A number of files or items.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Count(pub usize);

/// How many maps pruning keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Keep(pub usize);

/// What an item is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Fn,
    Struct,
    Enum,
    Trait,
    Const,
    Static,
    Type,
    Mod,
    Macro,
    Impl,
}

impl Kind {
    /// The word `items.tsv` gives it.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Fn => "fn",
            Kind::Struct => "struct",
            Kind::Enum => "enum",
            Kind::Trait => "trait",
            Kind::Const => "const",
            Kind::Static => "static",
            Kind::Type => "type",
            Kind::Mod => "mod",
            Kind::Macro => "macro",
            Kind::Impl => "impl",
        }
    }

    fn of(word: &str) -> Option<Kind> {
        Some(match word {
            "fn" => Kind::Fn,
            "struct" => Kind::Struct,
            "enum" => Kind::Enum,
            "trait" => Kind::Trait,
            "const" => Kind::Const,
            "static" => Kind::Static,
            "type" => Kind::Type,
            "mod" => Kind::Mod,
            _ => return None,
        })
    }
}

/// One row of `items.tsv`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Item {
    pub kind: Kind,
    pub name: String,
    /// The module path and the name, `::`-joined; an `impl` block's
    /// target type, or an exported macro at its crate's root.
    pub path: String,
    pub file: String,
    pub line: Line,
    pub signature: String,
    /// The first sentence of its `///` doc, or empty.
    pub doc: String,
}

/// What a run did: the counts `MAP` records and its error line.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    pub files: Count,
    pub items: Count,
    pub error: Option<String>,
}

/// The source with comments blanked (`shown`), and with string and
/// character contents blanked as well (`code`). Both keep every byte's
/// offset and every newline.
struct Masks {
    code: Vec<u8>,
    shown: Vec<u8>,
}

fn ident_byte(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// A byte offset into a file's source.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct At(usize);

/// Blanks `r` of `v`, its newlines kept.
fn blank(v: &mut [u8], r: Range<usize>) {
    let end = r.end.min(v.len());
    for b in &mut v[r.start.min(end)..end] {
        if *b != b'\n' {
            *b = b' ';
        }
    }
}

/// The end of a quoted string whose contents start at `at`: one past its
/// closing quotation mark.
fn string_end(b: &[u8], at: At) -> At {
    let mut j = at.0;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return At(j + 1),
            _ => j += 1,
        }
    }
    At(b.len())
}

/// A raw string at `at` (just past its `r`): its contents and one past its
/// end, or `None` when the `r` does not open one.
fn raw_string(b: &[u8], at: At) -> Option<(Range<usize>, At)> {
    let mut j = at.0;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    if b.get(j) != Some(&b'"') {
        return None;
    }
    let hashes = j - at.0;
    let start = j + 1;
    let mut k = start;
    while k < b.len() {
        if b[k] == b'"'
            && b[k + 1..]
                .iter()
                .take(hashes)
                .filter(|&&h| h == b'#')
                .count()
                == hashes
        {
            return Some((start..k, At(k + 1 + hashes)));
        }
        k += 1;
    }
    Some((start..b.len(), At(b.len())))
}

fn masks(src: &str) -> Masks {
    let b = src.as_bytes();
    let mut code = b.to_vec();
    let mut shown = b.to_vec();
    let mut i = 0;
    while i < b.len() {
        let next = b.get(i + 1).copied();
        let prev_ident = i > 0 && ident_byte(char::from(b[i - 1]));
        match b[i] {
            b'/' if next == Some(b'/') => {
                let end = b[i..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(b.len(), |p| i + p);
                blank(&mut code, i..end);
                blank(&mut shown, i..end);
                i = end;
            }
            b'/' if next == Some(b'*') => {
                let mut depth = 0usize;
                let mut j = i;
                while j < b.len() {
                    if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
                        depth += 1;
                        j += 2;
                    } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
                        depth -= 1;
                        j += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        j += 1;
                    }
                }
                blank(&mut code, i..j);
                blank(&mut shown, i..j);
                i = j;
            }
            b'"' => {
                let end = string_end(b, At(i + 1)).0;
                blank(&mut code, i + 1..end.saturating_sub(1));
                i = end;
            }
            b'r' | b'b' | b'c' if !prev_ident => {
                let mut j = i;
                if b[j] != b'r' {
                    j += 1;
                }
                if b.get(j) == Some(&b'r') {
                    if let Some((inside, end)) = raw_string(b, At(j + 1)) {
                        blank(&mut code, inside);
                        i = end.0;
                        continue;
                    }
                } else if b.get(j) == Some(&b'"') {
                    let end = string_end(b, At(j + 1)).0;
                    blank(&mut code, j + 1..end.saturating_sub(1));
                    i = end;
                    continue;
                }
                i += 1;
            }
            b'\'' => {
                // A character literal's length is its first character's
                // UTF-8 length; anything else after an apostrophe is a lifetime.
                let width = |lead| match lead {
                    0xf0..=0xff => 4,
                    0xe0..=0xef => 3,
                    0xc0..=0xdf => 2,
                    _ => 1,
                };
                let close = if next == Some(b'\\') {
                    let from = (i + 3).min(b.len());
                    b[from..].iter().position(|&c| c == b'\'').map(|p| from + p)
                } else {
                    next.map(|n| i + 1 + width(n))
                        .filter(|&k| b.get(k) == Some(&b'\''))
                };
                match close {
                    Some(k) => {
                        blank(&mut code, i + 1..k);
                        i = k + 1;
                    }
                    None => i += 1,
                }
            }
            _ => i += 1,
        }
    }
    Masks { code, shown }
}

/// The first sentence of a run of doc lines, their markers taken off: the
/// first paragraph up to the first `.`, `?` or `!` that a space or the
/// paragraph's end follows.
fn first_sentence<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let mut para = String::new();
    for l in lines {
        let l = l.trim();
        if l.is_empty() {
            if para.is_empty() {
                continue;
            }
            break;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(l);
    }
    let para = para.replace('\t', " ");
    let bytes = para.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if matches!(c, b'.' | b'?' | b'!') && bytes.get(i + 1).is_none_or(|&n| n == b' ') {
            return para[..=i].to_string();
        }
    }
    para
}

/// The first sentence of a file's leading `//!` doc, read past blank lines,
/// plain comments and inner attributes, or `None` when code comes first.
pub fn module_doc(src: &str) -> Option<String> {
    let mut doc = Vec::new();
    let mut attr = false;
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("//!") {
            doc.push(rest);
            continue;
        }
        if !doc.is_empty() {
            break;
        }
        if attr {
            attr = !t.ends_with(']');
            continue;
        }
        if t.starts_with("#![") {
            attr = !t.ends_with(']');
            continue;
        }
        if t.is_empty() || t.starts_with("//") {
            continue;
        }
        break;
    }
    let s = first_sentence(doc.into_iter());
    (!s.is_empty()).then_some(s)
}

/// Where each line starts, for turning an offset into a [`Line`].
struct Lines(Vec<usize>);

impl Lines {
    fn of(src: &str) -> Lines {
        let mut starts = vec![0];
        starts.extend(
            src.bytes()
                .enumerate()
                .filter(|&(_, b)| b == b'\n')
                .map(|(i, _)| i + 1),
        );
        Lines(starts)
    }

    fn at(&self, offset: At) -> Line {
        Line(self.0.partition_point(|&s| s <= offset.0))
    }
}

/// What a `{` opens, for knowing where item heads are read.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Ctx {
    Module(String),
    Impl(String),
    /// An `extern` block: its items are read, under the module's path.
    Foreign,
    Body,
}

/// The text after a leading run of attributes, and whether one of them is
/// `#[macro_export]`.
fn after_attributes(code: &str) -> (usize, Exported) {
    let b = code.as_bytes();
    let mut i = 0;
    let mut exported = Exported::No;
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let open = if code[i..].starts_with("#[") {
            i + 1
        } else if code[i..].starts_with("#![") {
            i + 2
        } else {
            return (i, exported);
        };
        let mut depth = 0usize;
        let mut j = open;
        while j < b.len() {
            match b[j] {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }
        if code[open..j.min(b.len())].contains("macro_export") {
            exported = Exported::Yes;
        }
        i = (j + 1).min(b.len());
    }
}

/// Whether a `macro_rules!` is exported to its crate's root.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Exported {
    Yes,
    No,
}

/// `s` with the word `w` taken off its front, when `w` is a whole word.
fn word<'a>(s: &'a str, w: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(w)?;
    if rest.chars().next().is_some_and(ident_byte) {
        return None;
    }
    Some(rest.trim_start())
}

fn ident(s: &str) -> &str {
    let s = s.strip_prefix("r#").unwrap_or(s);
    let end = s.find(|c| !ident_byte(c)).unwrap_or(s.len());
    &s[..end]
}

/// The head after `pub`, `pub(crate)` or `pub(super)`, or `None`.
fn visible(head: &str) -> Option<&str> {
    let rest = word(head, "pub")?;
    let Some(inner) = rest.strip_prefix('(') else {
        return Some(rest);
    };
    let close = inner.find(')')?;
    matches!(inner[..close].trim(), "crate" | "super").then(|| inner[close + 1..].trim_start())
}

/// An item's kind and name from the head after its visibility.
fn kind_and_name(mut rest: &str) -> Option<(Kind, String)> {
    loop {
        if let Some(r) = word(rest, "const") {
            if ["fn", "unsafe", "async", "extern"]
                .iter()
                .any(|w| word(r, w).is_some())
            {
                rest = r;
                continue;
            }
            break;
        }
        if let Some(r) = word(rest, "async")
            .or_else(|| word(rest, "unsafe"))
            .or_else(|| word(rest, "safe"))
        {
            rest = r;
            continue;
        }
        if let Some(r) = word(rest, "extern") {
            rest = match r.strip_prefix('"') {
                Some(abi) => abi.find('"').map_or("", |q| abi[q + 1..].trim_start()),
                None => r,
            };
            continue;
        }
        break;
    }
    let kw = ident(rest);
    let kind = Kind::of(kw)?;
    let mut after = rest[kw.len()..].trim_start();
    if kind == Kind::Static {
        after = word(after, "mut").unwrap_or(after);
    }
    let name = ident(after);
    (!name.is_empty()).then(|| (kind, name.to_string()))
}

/// The type an `impl` head is for: the last segment of the type after
/// `for`, or after the generics when there is no `for`.
fn impl_target(head: &str) -> Option<String> {
    let rest = word(head, "unsafe").unwrap_or(head);
    let mut rest = word(rest, "impl")?;
    if rest.starts_with('<') {
        let b = rest.as_bytes();
        let mut depth = 0usize;
        let mut end = b.len();
        for (i, &c) in b.iter().enumerate() {
            match c {
                b'<' => depth += 1,
                b'>' if i > 0 && b[i - 1] == b'-' => {}
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = rest[end..].trim_start();
    }
    let rest = rest.split(" where ").next().unwrap_or(rest);
    let mut depth = 0usize;
    let mut target = rest;
    for (i, c) in rest.char_indices() {
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && rest[i..].starts_with(" for ") {
            target = &rest[i + 5..];
            break;
        }
    }
    let mut t = target.trim();
    for p in ["!", "&", "mut ", "dyn "] {
        t = t.strip_prefix(p).unwrap_or(t).trim_start();
    }
    if t.starts_with('\'') {
        t = t.split_once(' ').map_or(t, |(_, r)| r.trim_start());
    }
    if t.starts_with(['(', '[']) {
        let mut depth = 0usize;
        for (i, c) in t.char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(t[..=i].to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let ty = t.split(['<', ' ', '{']).next().unwrap_or(t);
    let last = ty.rsplit("::").next().unwrap_or(ty);
    (!last.is_empty()).then(|| last.to_string())
}

/// A head's text on one line: its whitespace collapsed, and the spaces a
/// line break left inside brackets taken out.
fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut s = joined
        .replace("( ", "(")
        .replace("[ ", "[")
        .replace(" )", ")")
        .replace(" ]", "]")
        .replace(",)", ")")
        .replace(",]", "]");
    while s.ends_with(',') {
        s.pop();
    }
    s
}

/// The longest a const's or static's signature is shown with its value.
const VALUE_SHOWN: usize = 120;

/// An item's signature on one line. A const or static whose line would run
/// past [`VALUE_SHOWN`] is shown up to its `=`, its value elided.
fn signature(kind: Kind, shown: &str, code: &str) -> String {
    let line = one_line(shown);
    if !matches!(kind, Kind::Const | Kind::Static) || line.chars().count() <= VALUE_SHOWN {
        return line;
    }
    match code.find('=') {
        Some(eq) => format!("{} = …", one_line(&shown[..eq])),
        None => line,
    }
}

/// The doc lines of an item whose statement starts on line `first` and
/// whose head is on line `head`: the `///` lines directly above the
/// statement, past plain comments, and those among its attributes.
fn item_doc(lines: &[&str], first: Line, head: Line) -> String {
    let doc_line = |l: &str| {
        let t = l.trim_start();
        t.strip_prefix("///")
            .filter(|r| !r.starts_with('/'))
            .map(str::to_string)
    };
    let mut top = first.0 - 1;
    while top > 0 && lines[top - 1].trim_start().starts_with("//") {
        top -= 1;
    }
    let docs: Vec<String> = lines[top..head.0 - 1]
        .iter()
        .filter_map(|l| doc_line(l))
        .collect();
    first_sentence(docs.iter().map(String::as_str))
}
/// Every public item and `impl` block in one file's source. `module` is
/// the file's module path, its first segment the crate's name.
pub fn items(src: &str, file: &str, module: &str) -> Vec<Item> {
    let m = masks(src);
    let code = String::from_utf8_lossy(&m.code).into_owned();
    let shown = String::from_utf8_lossy(&m.shown).into_owned();
    let mut scan = Scan {
        code: &code,
        shown: &shown,
        starts: Lines::of(src),
        lines: src.split('\n').collect(),
        file,
        module,
        stack: Vec::new(),
        out: Vec::new(),
    };
    let mut begin: Option<usize> = None;
    let mut nest = 0usize;
    for (i, c) in code.bytes().enumerate() {
        if scan.stack.last() == Some(&Ctx::Body) {
            match c {
                b'{' => scan.stack.push(Ctx::Body),
                b'}' => {
                    scan.stack.pop();
                }
                _ => {}
            }
            continue;
        }
        match c {
            b'(' | b'[' => {
                nest += 1;
                begin.get_or_insert(i);
            }
            b')' | b']' => nest = nest.saturating_sub(1),
            b';' if nest == 0 => {
                if let Some(s) = begin.take() {
                    scan.head(s..i);
                }
            }
            b'{' if nest == 0 => {
                let ctx = begin.take().map_or(Ctx::Body, |s| scan.head(s..i));
                scan.stack.push(ctx);
            }
            b'}' if nest == 0 => {
                scan.stack.pop();
                begin = None;
            }
            c if !c.is_ascii_whitespace() => {
                begin.get_or_insert(i);
            }
            _ => {}
        }
    }
    scan.out
}

/// One file's scan: its masked texts, where its lines start, the blocks
/// open around the statement being read, and the rows found so far.
struct Scan<'a> {
    code: &'a str,
    shown: &'a str,
    starts: Lines,
    lines: Vec<&'a str>,
    file: &'a str,
    module: &'a str,
    stack: Vec<Ctx>,
    out: Vec<Item>,
}

impl Scan<'_> {
    /// Reads the head of the statement at `at`, its terminator excluded,
    /// adds its row when it is an item, and says what a `{` after it opens.
    fn head(&mut self, at: Range<usize>) -> Ctx {
        let (skip, exported) = after_attributes(&self.code[at.clone()]);
        let from = at.start + skip;
        let text = &self.code[from..at.end];
        let mut prefix = vec![self.module.to_string()];
        for c in &self.stack {
            match c {
                Ctx::Module(n) | Ctx::Impl(n) => prefix.push(n.clone()),
                Ctx::Foreign | Ctx::Body => {}
            }
        }
        let in_impl = matches!(self.stack.last(), Some(Ctx::Impl(_)));
        let krate = self.module.split("::").next().unwrap_or(self.module);
        let row = |kind: Kind, name: &str, path: String| Item {
            kind,
            name: name.to_string(),
            path,
            file: self.file.to_string(),
            line: self.starts.at(At(from)),
            signature: signature(kind, &self.shown[from..at.end], &self.code[from..at.end]),
            doc: item_doc(
                &self.lines,
                self.starts.at(At(at.start)),
                self.starts.at(At(from)),
            ),
        };
        if let Some(rest) = visible(text) {
            let Some((kind, name)) = kind_and_name(rest) else {
                return Ctx::Body;
            };
            let item = row(kind, &name, format!("{}::{name}", prefix.join("::")));
            self.out.push(item);
            return match kind {
                Kind::Mod => Ctx::Module(name),
                _ => Ctx::Body,
            };
        }
        if in_impl {
            return Ctx::Body;
        }
        if let Some(target) = impl_target(text) {
            prefix.push(target.clone());
            let item = row(Kind::Impl, &target, prefix.join("::"));
            self.out.push(item);
            return Ctx::Impl(target);
        }
        if let Some(rest) = word(text, "macro_rules").and_then(|r| r.strip_prefix('!')) {
            let name = ident(rest.trim_start());
            if exported == Exported::Yes && !name.is_empty() {
                let item = row(Kind::Macro, name, format!("{krate}::{name}"));
                self.out.push(item);
            }
            return Ctx::Body;
        }
        let abi = word(text, "unsafe").unwrap_or(text);
        if let Some(rest) = word(abi, "extern") {
            let rest = match rest.strip_prefix('"') {
                Some(r) => r.find('"').map_or(r, |q| r[q + 1..].trim()),
                None => rest,
            };
            if rest.is_empty() {
                return Ctx::Foreign;
            }
        }
        if let Some(rest) = word(text, "mod") {
            let name = ident(rest);
            if !name.is_empty() {
                return Ctx::Module(name.to_string());
            }
        }
        Ctx::Body
    }
}

/// The rows as `items.tsv` holds them, under its header.
pub fn items_tsv(items: &[Item]) -> String {
    let mut s = String::from("kind\tname\tpath\tat\tsignature\tdoc\n");
    for i in items {
        let _ = writeln!(
            s,
            "{}\t{}\t{}\t{}:{}\t{}\t{}",
            i.kind.word(),
            i.name,
            i.path,
            i.file,
            i.line.0,
            i.signature.replace('\t', " "),
            i.doc
        );
    }
    s
}

/// The package a file belongs to: its name and its directory, found from
/// the nearest `Cargo.toml` above it that has a `[package]`.
struct Packages {
    root: PathBuf,
    seen: BTreeMap<String, Option<(String, String)>>,
}

impl Packages {
    fn of(&mut self, dir: &str) -> Option<(String, String)> {
        if let Some(hit) = self.seen.get(dir) {
            return hit.clone();
        }
        let found = fs::read_to_string(self.root.join(dir).join("Cargo.toml"))
            .ok()
            .and_then(|t| toml::from_str::<toml::Table>(&t).ok())
            .and_then(|t| {
                t.get("package")?
                    .get("name")?
                    .as_str()
                    .map(|n| (n.to_string(), dir.to_string()))
            });
        let found = match found {
            Some(f) => Some(f),
            None if dir.is_empty() => None,
            None => {
                let up = dir.rsplit_once('/').map_or("", |(u, _)| u).to_string();
                self.of(&up)
            }
        };
        self.seen.insert(dir.to_string(), found.clone());
        found
    }
}

/// A file's module path, from its package's name and its path inside the
/// package.
fn module_path(package: Option<&(String, String)>, file: &str) -> String {
    let (krate, rel) = match package {
        Some((name, dir)) if !dir.is_empty() => (
            name.replace('-', "_"),
            file.strip_prefix(&format!("{dir}/")).unwrap_or(file),
        ),
        Some((name, _)) => (name.replace('-', "_"), file),
        None => (String::new(), file),
    };
    let stem = rel.strip_suffix(".rs").unwrap_or(rel);
    let mut parts: Vec<&str> = stem.split('/').collect();
    if parts.len() > 1 && matches!(parts.last(), Some(&"mod")) {
        parts.pop();
    }
    let segs: Vec<String> = match parts.as_slice() {
        ["src", "lib" | "main"] => vec![krate],
        ["src", "bin", rest @ ..] => {
            let mut r: Vec<&str> = rest.to_vec();
            if r.len() > 1 && matches!(r.last(), Some(&"main")) {
                r.pop();
            }
            r.iter().map(|s| s.replace('-', "_")).collect()
        }
        ["src", rest @ ..] => std::iter::once(krate)
            .chain(rest.iter().map(|s| (*s).to_string()))
            .collect(),
        ["tests" | "benches" | "examples", rest @ ..] if !rest.is_empty() => {
            let mut r: Vec<&str> = rest.to_vec();
            if r.len() > 1 && matches!(r.last(), Some(&"main")) {
                r.pop();
            }
            r.iter().map(|s| s.replace('-', "_")).collect()
        }
        all => all.iter().map(|s| s.replace('-', "_")).collect(),
    };
    segs.join("::")
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `crates.tsv` from `cargo metadata --no-deps`.
fn crates_tsv(root: &Path) -> Result<String, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let meta: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let base = meta["workspace_root"].as_str().unwrap_or_default();
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let names: BTreeSet<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    let mut rows = BTreeMap::new();
    for p in &packages {
        let name = p["name"].as_str().unwrap_or_default();
        let manifest = p["manifest_path"].as_str().unwrap_or_default();
        let dir = Path::new(manifest).parent().unwrap_or(Path::new(""));
        let rel = dir
            .strip_prefix(base)
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|_| dir.to_string_lossy().into_owned());
        let lib = p["targets"].as_array().and_then(|ts| {
            ts.iter().find(|t| {
                t["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "lib" || k == "proc-macro"))
            })
        });
        let no_std = match lib.and_then(|t| t["src_path"].as_str()) {
            None => "-",
            Some(src) => {
                let text = fs::read_to_string(src).unwrap_or_default();
                if text.lines().any(|l| l.trim() == "#![no_std]") {
                    "yes"
                } else if text
                    .lines()
                    .any(|l| l.trim_start().starts_with("#![cfg_attr(") && l.contains("no_std"))
                {
                    "cfg"
                } else {
                    "no"
                }
            }
        };
        let features: BTreeSet<&str> = p["features"]
            .as_object()
            .map(|f| f.keys().map(String::as_str).collect())
            .unwrap_or_default();
        let deps: BTreeSet<String> = p["dependencies"]
            .as_array()
            .map(|ds| {
                ds.iter()
                    .filter_map(|d| {
                        let n = d["name"].as_str()?;
                        if !names.contains(n) {
                            return None;
                        }
                        Some(match d["kind"].as_str() {
                            Some(k) => format!("{n}({k})"),
                            None => n.to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let join = |s: Vec<&str>| s.join(",");
        rows.insert(
            name.to_string(),
            format!(
                "{name}\t{rel}\t{no_std}\t{}\t{}",
                join(features.into_iter().collect()),
                join(deps.iter().map(String::as_str).collect())
            ),
        );
    }
    let mut s = String::from("name\tpath\tno_std\tfeatures\tdeps\n");
    for r in rows.values() {
        s.push_str(r);
        s.push('\n');
    }
    Ok(s)
}

/// A figure of the perf gate's budget table: `value`, or a `lo..hi` band.
fn figure(v: &toml::Value) -> Option<String> {
    let t = v.as_table()?;
    if let Some(x) = t.get("value") {
        return Some(x.to_string());
    }
    Some(format!("{}..{}", t.get("lo")?, t.get("hi")?))
}

fn perf_rows(prefix: &str, t: &toml::Table, rows: &mut Vec<String>) {
    let mut figures = Vec::new();
    for (k, v) in t {
        let path = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        if let Some(f) = figure(v) {
            figures.push(format!("{k}={f}"));
        } else if let Some(sub) = v.as_table() {
            if sub.values().all(|x| figure(x).is_some()) && !sub.is_empty() {
                for (fk, fv) in sub {
                    figures.push(format!("{k}.{fk}={}", figure(fv).unwrap_or_default()));
                }
            } else {
                perf_rows(&path, sub, rows);
            }
        }
    }
    if !figures.is_empty() {
        rows.push(format!(
            "perf-budget\t{prefix}\t{}\tperf-budgets.toml",
            figures.join(" ")
        ));
    }
}

fn perf(root: &Path, rows: &mut Vec<String>) -> Result<(), String> {
    let text = fs::read_to_string(root.join("tools/perf-budgets.toml"))
        .map_err(|e| format!("tools/perf-budgets.toml: {e}"))?;
    let t: toml::Table =
        toml::from_str(&text).map_err(|e| format!("tools/perf-budgets.toml: {e}"))?;
    let words = |v: Option<&toml::Value>| match v {
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    };
    if let Some(gates) = t.get("gate").and_then(toml::Value::as_table) {
        for (name, g) in gates {
            rows.push(format!(
                "perf-gate\t{name}\tfn={}; bench={}; inputs={}\tperf-budgets.toml",
                words(g.get("fn")),
                words(g.get("bench")),
                words(g.get("inputs"))
            ));
        }
    }
    for (k, v) in &t {
        if let (false, Some(sub)) = (k == "gate", v.as_table()) {
            perf_rows(k, sub, rows);
        }
    }
    Ok(())
}

static AUDIT: LazyLock<[(Regex, &str); 6]> = LazyLock::new(|| {
    let re = |p: &str| Regex::new(p).expect("an audit pattern");
    [
        (
            re(
                r#"krate\(\s*"([^"]+)"\s*,\s*&\[[^\]]*\]\s*,\s*Sites\((\d+)\)\s*,\s*Sites\((\d+)\)"#,
            ),
            "total={2} signature={3}",
        ),
        (
            re(r#"name:\s*"([^"]+)",[^}]*?total:\s*Sites\((\d+)\),\s*signature:\s*Sites\((\d+)\)"#),
            "total={2} signature={3}",
        ),
        (
            re(r#"name:\s*"([^"]+)",\s*root:\s*"[^"]*",\s*limit:\s*Sites\((\d+)\)"#),
            "limit={2}",
        ),
        (
            re(r#"\btree\(\s*"([^"]+)"\s*,\s*Sites\((\d+)\)\s*\)"#),
            "limit={2}",
        ),
        (
            re(r#"[\[,]\s*\(\s*"([^"]+)"\s*,\s*Sites\((\d+)\)\s*\)"#),
            "limit={2}",
        ),
        (
            re(r"pub const (\w+):\s*[\w:]*Limit\s*=\s*[\w:]*Limit\((\d+)\)"),
            "limit={2}",
        ),
    ]
});

static CONST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"pub const (\w+)").expect("CONST"));

const AUDIT_RS: &str = "crates/xtask/src/audit.rs";
const CONTRACT_RS: &str = "crates/baud-domain/tests/contract.rs";

fn audit(root: &Path, rows: &mut Vec<String>) -> Result<(), String> {
    let src = fs::read_to_string(root.join(AUDIT_RS)).map_err(|e| format!("{AUDIT_RS}: {e}"))?;
    let shown = String::from_utf8_lossy(&masks(&src).shown).into_owned();
    let starts = Lines::of(&src);
    let consts: Vec<(usize, &str)> = CONST
        .captures_iter(&shown)
        .filter_map(|c| c.get(1).map(|m| (m.start(), m.as_str())))
        .collect();
    let mut found = Vec::new();
    for (re, limit) in AUDIT.iter() {
        for c in re.captures_iter(&shown) {
            let (Some(name), Some(first)) = (c.get(1), c.get(2)) else {
                continue;
            };
            let owner = consts
                .iter()
                .rev()
                .find(|(at, _)| *at <= name.start())
                .map_or("-", |(_, n)| n);
            let text = limit
                .replace("{2}", first.as_str())
                .replace("{3}", c.get(3).map_or("", |m| m.as_str()));
            found.push((
                name.start(),
                format!(
                    "audit:{owner}\t{}\t{text}\t{AUDIT_RS}:{}",
                    name.as_str(),
                    starts.at(At(name.start())).0
                ),
            ));
        }
    }
    if found.is_empty() {
        return Err(format!("{AUDIT_RS}: no budget found"));
    }
    found.sort();
    rows.extend(found.into_iter().map(|(_, r)| r));
    Ok(())
}

static WITNESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)"#).expect("WITNESS"));

static PIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^const _\s*:").expect("PIN"));

fn contract(root: &Path, rows: &mut Vec<String>) -> Result<(), String> {
    let src =
        fs::read_to_string(root.join(CONTRACT_RS)).map_err(|e| format!("{CONTRACT_RS}: {e}"))?;
    let m = masks(&src);
    let shown = String::from_utf8_lossy(&m.shown).into_owned();
    let code = m.code;
    let starts = Lines::of(&src);
    let at = shown
        .find("const WITNESSED")
        .ok_or_else(|| format!("{CONTRACT_RS}: no WITNESSED"))?;
    let end = shown[at..].find("];").map_or(shown.len(), |e| at + e);
    for c in WITNESS.captures_iter(&shown[at..end]) {
        let (Some(ty), Some(code)) = (c.get(1), c.get(2)) else {
            continue;
        };
        rows.push(format!(
            "witnessed\t{}\t{}\t{CONTRACT_RS}:{}",
            ty.as_str(),
            code.as_str(),
            starts.at(At(at + ty.start())).0
        ));
    }
    for p in PIN.find_iter(&shown) {
        let mut nest = 0usize;
        let mut stop = code.len();
        let mut eq = None;
        for (i, &b) in code.iter().enumerate().skip(p.end()) {
            match b {
                b'(' | b'[' | b'{' | b'<' => nest += 1,
                b'>' if code[i - 1] == b'-' => {}
                b')' | b']' | b'}' | b'>' => nest = nest.saturating_sub(1),
                b'=' if nest == 0 && eq.is_none() => eq = Some(i),
                b';' if nest == 0 => {
                    stop = i;
                    break;
                }
                _ => {}
            }
        }
        let Some(eq) = eq else { continue };
        let ty = one_line(&shown[p.end()..eq]);
        let value = one_line(&shown[eq + 1..stop]);
        if !ty.contains("baud_domain::") && !value.contains("baud_domain::") {
            continue;
        }
        rows.push(format!(
            "pin\t{value}\t{ty}\t{CONTRACT_RS}:{}",
            starts.at(At(p.start())).0
        ));
    }
    Ok(())
}

fn gates_tsv(root: &Path, errors: &mut Vec<String>) -> String {
    let mut rows = Vec::new();
    for part in [perf, audit, contract] {
        if let Err(e) = part(root, &mut rows) {
            errors.push(e);
        }
    }
    let mut s = String::from("kind\tname\tlimit\twhere\n");
    for r in rows {
        s.push_str(&r);
        s.push('\n');
    }
    s
}

fn index_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Maps the tree at `root` into `out`. Whatever fails is named in the
/// report's error and on `MAP`'s `error:` line; `MAP` is written last and
/// always.
pub fn generate(root: &Path, out: &Path) -> Report {
    let mut errors = Vec::new();
    let mut report = Report::default();
    let tree = git(root, &["rev-parse", "HEAD^{tree}"]).map(|s| s.trim().to_string());
    let commit = git(root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string());
    let (tree, commit) = match (tree, commit) {
        (Ok(t), Ok(c)) => (t, c),
        (Err(e), _) | (_, Err(e)) => {
            errors.push(e);
            ("-".to_string(), "-".to_string())
        }
    };
    if let Err(e) = fs::create_dir_all(out) {
        errors.push(format!("{}: {e}", out.display()));
    }
    let files: Vec<String> = match git(root, &["ls-files", "-z", "--", "*.rs"]) {
        Ok(s) => {
            let mut f: Vec<String> = s
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect();
            f.sort();
            f
        }
        Err(e) => {
            errors.push(e);
            Vec::new()
        }
    };
    let mut packages = Packages {
        root: root.to_path_buf(),
        seen: BTreeMap::new(),
    };
    let mut index = format!(
        "# Code map of tree {tree}\n\nOne line per tracked `.rs` file: its crate, path, line count and the first sentence of its `//!` doc.\n\n| crate | path | lines | first sentence |\n|---|---|---|---|\n"
    );
    let mut all = Vec::new();
    let mut undocumented = Count(0);
    for file in &files {
        let src = match fs::read(root.join(file)) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                errors.push(format!("{file}: {e}"));
                continue;
            }
        };
        let dir = file.rsplit_once('/').map_or("", |(d, _)| d);
        let package = packages.of(dir);
        let module = module_path(package.as_ref(), file);
        let doc = module_doc(&src).unwrap_or_else(|| {
            undocumented.0 += 1;
            NO_MODULE_DOC.to_string()
        });
        let _ = writeln!(
            index,
            "| {} | {} | {} | {} |",
            package.as_ref().map_or("-", |(n, _)| n.as_str()),
            file,
            src.lines().count(),
            index_cell(&doc)
        );
        all.extend(items(&src, file, &module));
        report.files.0 += 1;
    }
    let _ = writeln!(
        index,
        "\n{} files, {} with {NO_MODULE_DOC}.",
        report.files.0, undocumented.0
    );
    report.items = Count(all.len());
    let crates = crates_tsv(root).unwrap_or_else(|e| {
        errors.push(e);
        String::from("name\tpath\tno_std\tfeatures\tdeps\n")
    });
    let gates = gates_tsv(root, &mut errors);
    for (name, body) in [
        ("index.md", index),
        ("items.tsv", items_tsv(&all)),
        ("crates.tsv", crates),
        ("gates.tsv", gates),
    ] {
        if let Err(e) = fs::write(out.join(name), body) {
            errors.push(format!("{name}: {e}"));
        }
    }
    if !errors.is_empty() {
        report.error = Some(errors.join("; ").replace('\n', " "));
    }
    let mut map = format!(
        "{MARKER}\nversion {VERSION}\ntree {tree}\ncommit {commit}\nfiles {}\nitems {}\nundocumented {}\n",
        report.files.0, report.items.0, undocumented.0
    );
    if let Some(e) = &report.error {
        let _ = writeln!(map, "error: {e}");
    }
    if let Err(e) = fs::create_dir_all(out).and_then(|()| fs::write(out.join("MAP"), map)) {
        let e = format!("MAP: {e}");
        report.error = Some(match report.error.take() {
            Some(prev) => format!("{prev}; {e}"),
            None => e,
        });
    }
    report
}

/// Deletes all but the `keep` newest maps directly under `root`, and
/// returns what it deleted, in order. A directory is a map when its `MAP`
/// begins with [`MARKER`]; one a symbolic link in `root` points at is
/// never deleted.
pub fn prune(root: &Path, keep: Keep) -> io::Result<Vec<PathBuf>> {
    let mut maps = Vec::new();
    let mut pointed = BTreeSet::new();
    for e in fs::read_dir(root)? {
        let e = e?;
        let path = e.path();
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            if let Ok(t) = fs::canonicalize(&path) {
                pointed.insert(t);
            }
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        let map = path.join("MAP");
        let first = fs::read_to_string(&map)
            .ok()
            .and_then(|t| t.lines().next().map(str::to_string));
        if first.as_deref() != Some(MARKER) {
            continue;
        }
        let modified = fs::metadata(&map)?.modified()?;
        maps.push((modified, path));
    }
    maps.sort_by(|a, b| b.cmp(a));
    let mut deleted = Vec::new();
    for (_, path) in maps.into_iter().skip(keep.0) {
        if pointed.contains(&fs::canonicalize(&path)?) {
            continue;
        }
        fs::remove_dir_all(&path)?;
        deleted.push(path);
    }
    deleted.sort();
    Ok(deleted)
}

/// `code-map --out DIR [--prune ROOT --keep N]`, or the prune alone.
pub fn run(args: &[&str], root: &Path, out: &mut impl Write) -> io::Result<Exit> {
    let mut dest = None;
    let mut prune_root = None;
    let mut keep = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let value = it.next();
        match (*flag, value) {
            ("--out", Some(v)) => dest = Some(PathBuf::from(v)),
            ("--prune", Some(v)) => prune_root = Some(PathBuf::from(v)),
            ("--keep", Some(v)) => match v.parse() {
                Ok(n) => keep = Some(Keep(n)),
                Err(_) => {
                    writeln!(out, "code-map: not a count: {v}")?;
                    return Ok(Exit(2));
                }
            },
            _ => {
                writeln!(out, "code-map: not an option: {flag}")?;
                return Ok(Exit(2));
            }
        }
    }
    if (dest.is_none() && prune_root.is_none()) || prune_root.is_some() != keep.is_some() {
        writeln!(
            out,
            "usage: cargo xtask code-map --out DIR [--prune ROOT --keep N]"
        )?;
        return Ok(Exit(2));
    }
    if let Some(dir) = dest {
        let r = generate(root, &dir);
        writeln!(
            out,
            "code-map: {} files, {} items in {}",
            r.files.0,
            r.items.0,
            dir.display()
        )?;
        if let Some(e) = r.error {
            writeln!(out, "code-map: error: {e}")?;
        }
    }
    if let (Some(p), Some(k)) = (prune_root, keep) {
        match prune(&p, k) {
            Ok(gone) => {
                for g in gone {
                    writeln!(out, "code-map: pruned {}", g.display())?;
                }
            }
            Err(e) => writeln!(out, "code-map: error: prune {}: {e}", p.display())?,
        }
    }
    Ok(Exit(0))
}
