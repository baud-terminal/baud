//! `cargo xtask citations [--check | --merge <fragment>...]`: docs/citations.md,
//! the list of the primary sources the tree cites, written from
//! docs/research/sources/MANIFEST.toml, the source citations in the tracked
//! documents and Rust files, and docs/data/citations-code.tsv.
//!
//! ```text
//! cargo xtask citations                     write docs/citations.md
//! cargo xtask citations --check             fail if it is stale or an input is wrong
//! cargo xtask citations --merge <tsv>...    add the fragments' rows to
//!                                           docs/data/citations-code.tsv, then write
//! ```
//!
//! An entry is listed when a tracked `.md` or `.rs` file cites its key, when
//! its copy is committed under docs/research/sources/, or when a row of
//! citations-code.tsv names it. Each entry carries its title, URL, sha256,
//! where its copy is, the date its entry's comments give for the day it was
//! read, the files that cite it and the code items it supports, under an
//! anchor that is its key.
//!
//! citations-code.tsv has one row per code item and key, `item<TAB>key<TAB>what`,
//! where `item` is `path::name` and `what` says what the source supports in
//! it. A fragment has the columns `item<TAB>key<TAB>what`, or
//! `item<TAB>key<TAB>lines<TAB>what`, and its item may leave out the path;
//! `--merge` drops the line range, gives each item the path of the tracked
//! `.rs` file that defines it, adds each row the file does not hold and keeps
//! the file sorted. `--check` fails on a row whose key the manifest does not
//! carry or carries as `gated`, whose path is not in the tree, or that
//! repeats another.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml::{Table, Value};

use crate::third_party::Outcome;
use crate::{no_python, sources, text};

pub const OUT: &str = "docs/citations.md";
pub const CODE: &str = "docs/data/citations-code.tsv";
/// The command the stale line tells a reader to run.
pub const GENERATOR: &str = "cargo xtask citations";

static CITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[source: ([a-z0-9_-]+)\]\]").expect("the citation pattern"));
static READ: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[Ff]etched|[Rr]ead|[Rr]etrieved|[Cc]opied)(?: on)? (\d{4}-\d{2}-\d{2})")
        .expect("the read-date pattern")
});

/// What the command does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Write docs/citations.md.
    Write,
    /// Fail if docs/citations.md is stale or an input is wrong.
    Check,
}

/// One row of citations-code.tsv.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Code {
    pub key: String,
    pub item: String,
    pub what: String,
}

/// The rows of a citations-code.tsv text, or the line that is malformed.
/// A fragment's four columns are read as its item, key and what.
pub fn rows(text: &str, origin: &str) -> Result<Vec<Code>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let (item, key, what) = match cols.as_slice() {
            [item, key, what] => (*item, *key, *what),
            [item, key, _lines, what] => (*item, *key, *what),
            _ => {
                return Err(format!(
                    "{origin}:{}: {} columns, want item, key and what (a fragment adds lines)",
                    n + 1,
                    cols.len()
                ));
            }
        };
        if item.is_empty() || key.is_empty() || what.is_empty() {
            return Err(format!("{origin}:{}: an empty column", n + 1));
        }
        out.push(Code {
            key: key.to_string(),
            item: item.to_string(),
            what: what.to_string(),
        });
    }
    Ok(out)
}

/// citations-code.tsv as `rows` gives it, written back: its header, then
/// the rows by key and item.
pub fn code_file(rows: &[Code]) -> String {
    let mut o = String::from(CODE_HEADER);
    let mut sorted: Vec<&Code> = rows.iter().collect();
    sorted.sort();
    for r in sorted {
        o.push_str(&format!("{}\t{}\t{}\n", r.item, r.key, r.what));
    }
    o
}

const CODE_HEADER: &str = "# The code items each source supports, one row per item and key:\n\
# item (path::name) <TAB> key <TAB> what the source supports in it.\n\
# `cargo xtask citations --merge <fragment>...` adds rows; docs/citations.md\n\
# lists them under each key.\n";

/// Each key's comments in the manifest: the comment lines after an entry's
/// header up to the next blank line, and those right above its header.
pub fn comments(manifest: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut pending: Vec<&str> = Vec::new();
    let mut cur: Option<String> = None;
    for line in manifest.lines() {
        if line.trim().is_empty() {
            pending.clear();
            cur = None;
        } else if let Some(c) = line.strip_prefix('#') {
            let c = c.trim();
            match &cur {
                Some(k) => {
                    let e = out.entry(k.clone()).or_default();
                    e.push(' ');
                    e.push_str(c);
                }
                None => pending.push(c),
            }
        } else if let Some(rest) = line.strip_prefix("[sources.") {
            let key = rest.strip_suffix(']').unwrap_or(rest).to_string();
            let e = out.entry(key.clone()).or_default();
            for c in pending.drain(..) {
                e.push(' ');
                e.push_str(c);
            }
            cur = Some(key);
        }
    }
    out
}

/// The day an entry's comments say its source was read, where they say.
pub fn read_date(comment: &str) -> Option<String> {
    READ.captures(comment).map(|c| c[1].to_string())
}

/// Each cited key and the tracked files that cite it.
pub fn sites(root: &Path) -> io::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let skip = format!("{}/", sources::SRC);
    for path in no_python::tracked(root)? {
        if !(path.ends_with(".md") || path.ends_with(".rs"))
            || path == OUT
            || path.starts_with(&skip)
        {
            continue;
        }
        let full = root.join(&path);
        if !fs::symlink_metadata(&full).is_ok_and(|m| m.is_file()) {
            continue;
        }
        let Ok(bytes) = fs::read(&full) else {
            continue;
        };
        let body = String::from_utf8_lossy(&bytes);
        for c in CITE.captures_iter(&body) {
            out.entry(c[1].to_string())
                .or_default()
                .insert(path.clone());
        }
    }
    Ok(out)
}

/// Text as markdown shows it: no character in it starts a link, a code
/// span, emphasis or a tag.
pub fn escaped(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '`' => o.push_str("&#96;"),
            '[' => o.push_str("&#91;"),
            ']' => o.push_str("&#93;"),
            '*' | '_' | '\\' => {
                o.push('\\');
                o.push(c);
            }
            _ => o.push(c),
        }
    }
    o
}

/// A URL as a link, or, where the entry records how the copy was made
/// rather than a URL, that text.
pub fn link(u: &str) -> String {
    let web = u.starts_with("https://") || u.starts_with("http://");
    if web && !u.contains(char::is_whitespace) {
        format!("<{u}>")
    } else {
        escaped(u)
    }
}

fn field<'t>(e: &'t Table, name: &str) -> Option<&'t str> {
    e.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
}

/// The inputs, read: the manifest's entries, its comments, and the code rows.
pub struct Inputs {
    pub sources: Table,
    pub comments: BTreeMap<String, String>,
    pub code: Vec<Code>,
    pub sites: BTreeMap<String, BTreeSet<String>>,
}

impl Inputs {
    pub fn read(root: &Path) -> Result<Inputs, String> {
        let raw = text::read_universal(&root.join(sources::MANIFEST))
            .map_err(|e| format!("{}: {e}", sources::MANIFEST))?;
        let parsed = sources::parse(&raw).map_err(|e| format!("{}: {e}", sources::MANIFEST))?;
        let code = match fs::read_to_string(root.join(CODE)) {
            Ok(t) => rows(&t, CODE)?,
            Err(e) => return Err(format!("{CODE}: {e}")),
        };
        Ok(Inputs {
            sources: parsed,
            comments: comments(&raw),
            code,
            sites: sites(root).map_err(|e| e.to_string())?,
        })
    }

    /// What is wrong with the code rows.
    pub fn problems(&self, root: &Path) -> Vec<String> {
        let mut errs = Vec::new();
        let mut seen = BTreeSet::new();
        for r in &self.code {
            match self.sources.get(&r.key).and_then(Value::as_table) {
                None => errs.push(format!(
                    "{CODE}: {} names {}, which the manifest does not carry",
                    r.item, r.key
                )),
                Some(e) if e.get("status").and_then(Value::as_str) == Some("gated") => {
                    errs.push(format!(
                        "{CODE}: {} names {}, recorded as gated \u{2014} never read",
                        r.item, r.key
                    ));
                }
                Some(_) => {}
            }
            let path = r.item.split("::").next().unwrap_or("");
            if !root.join(path).is_file() {
                errs.push(format!(
                    "{CODE}: {} names {path}, which is not in the tree",
                    r.item
                ));
            }
            if !seen.insert((r.key.as_str(), r.item.as_str())) {
                errs.push(format!("{CODE}: {} under {} twice", r.item, r.key));
            }
        }
        errs
    }

    /// The keys the list holds, in byte order.
    pub fn listed(&self) -> BTreeSet<String> {
        let mut keys: BTreeSet<String> = self.sites.keys().cloned().collect();
        keys.extend(self.code.iter().map(|r| r.key.clone()));
        for (k, e) in &self.sources {
            let committed = e.as_table().is_some_and(|t| {
                t.get("status").and_then(Value::as_str) == Some("retrieved")
                    && field(t, "file").is_some()
            });
            if committed {
                keys.insert(k.clone());
            }
        }
        keys.retain(|k| self.sources.contains_key(k));
        keys
    }
}

/// docs/citations.md for these inputs.
pub fn render(inputs: &Inputs) -> String {
    let mut o = String::from(HEAD);
    for key in inputs.listed() {
        let Some(e) = inputs.sources.get(&key).and_then(Value::as_table) else {
            continue;
        };
        o.push_str(&format!("<a id=\"{key}\"></a>\n### {key}\n\n"));
        let mut line: Vec<String> = Vec::new();
        if let Some(t) = field(e, "title") {
            line.push(escaped(t));
        }
        let year = e.get("year").map(|y| match y {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        let by: Vec<String> = [
            field(e, "author").map(str::to_string),
            year,
            field(e, "venue").map(str::to_string),
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .map(|s| escaped(&s))
        .collect();
        if !by.is_empty() {
            line.push(by.join(", "));
        }
        if !line.is_empty() {
            o.push_str(&format!("{}.\n\n", line.join(". ")));
        }
        if let Some(u) = field(e, "url") {
            o.push_str(&format!("- **URL:** {}\n", link(u)));
        }
        if let Some(u) = field(e, "archive") {
            o.push_str(&format!("- **Archive:** {}\n", link(u)));
        }
        if let Some(sha) = field(e, "sha256") {
            match e.get("bytes").and_then(Value::as_integer) {
                Some(b) => o.push_str(&format!("- **sha256:** `{sha}` ({b} bytes)\n")),
                None => o.push_str(&format!("- **sha256:** `{sha}`\n")),
            }
        }
        let status = field(e, "status").unwrap_or("");
        match field(e, "file") {
            Some(f) if status == "retrieved" => o.push_str(&format!(
                "- **Copy:** [{}/{f}](research/sources/{f})\n",
                sources::SRC
            )),
            _ => o.push_str("- **Copy:** not committed; cited by URL and sha256\n"),
        }
        if let Some(d) = inputs
            .comments
            .get(&key)
            .map(String::as_str)
            .and_then(read_date)
        {
            o.push_str(&format!("- **Read:** {d}\n"));
        }
        if let Some(files) = inputs.sites.get(&key) {
            let list: Vec<String> = files.iter().map(|f| format!("`{f}`")).collect();
            o.push_str(&format!("- **Cited in:** {}\n", list.join(", ")));
        }
        let mut code: Vec<&Code> = inputs.code.iter().filter(|r| r.key == key).collect();
        code.sort();
        if !code.is_empty() {
            o.push_str("- **Supports:**\n");
            for r in code {
                o.push_str(&format!("  - `{}`: {}\n", r.item, escaped(&r.what)));
            }
        }
        o.push('\n');
    }
    o
}

const HEAD: &str = "# Citations\n\n\
<!-- Generated by `cargo xtask citations` from docs/research/sources/MANIFEST.toml, the source\n\
citations in the tree's documents and code, and docs/data/citations-code.tsv. Do not edit:\n\
change those and regenerate. `cargo xtask sources` fails when this file is stale. -->\n\n\
Every protocol claim traces to a primary source recorded in\n\
[MANIFEST.toml](research/sources/MANIFEST.toml) with a SHA-256: committed where a build or test\n\
reads it, otherwise cited by URL and hash. This page lists, by key, each source a document or a\n\
source file cites, each one committed under docs/research/sources/ and each one a code item\n\
rests on: its title, URL and digest, where its copy is, the day it was read where its entry\n\
says, the files that cite it and the code items it supports. A citation of a key resolves to\n\
its entry here, at `docs/citations.md#<key>`.\n\n";

/// `cargo xtask citations [--check]`, run against `root`.
pub fn run(root: &Path, mode: Mode, out: &mut impl Write) -> io::Result<Outcome> {
    let inputs = match Inputs::read(root) {
        Ok(i) => i,
        Err(e) => {
            writeln!(out, "  FAIL {e}")?;
            return Ok(Outcome::Failed);
        }
    };
    let mut errs = inputs.problems(root);
    let rendered = render(&inputs);
    if mode == Mode::Check {
        match text::read_universal(&root.join(OUT)) {
            Ok(cur) if cur == rendered => {}
            Ok(_) => errs.push(format!("{OUT} is stale: run {GENERATOR}")),
            Err(_) => errs.push(format!("{OUT} is missing: run {GENERATOR}")),
        }
    }
    for e in &errs {
        writeln!(out, "  FAIL {e}")?;
    }
    if !errs.is_empty() {
        return Ok(Outcome::Failed);
    }
    match mode {
        Mode::Write => fs::write(root.join(OUT), rendered)?,
        Mode::Check => writeln!(
            out,
            "  ok   {} sources listed in {OUT}, {} code items",
            inputs.listed().len(),
            inputs.code.len()
        )?,
    }
    Ok(Outcome::Passed)
}

/// Each name a tracked `.rs` file outside docs/research/sources/ defines
/// at the start of a line, and the files that define it.
pub fn definitions(root: &Path) -> io::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let skip = format!("{}/", sources::SRC);
    for path in no_python::tracked(root)? {
        if !path.ends_with(".rs") || path.starts_with(&skip) {
            continue;
        }
        let Ok(body) = fs::read_to_string(root.join(&path)) else {
            continue;
        };
        for c in DEFINES.captures_iter(&body) {
            out.entry(c[1].to_string())
                .or_default()
                .insert(path.clone());
        }
    }
    Ok(out)
}

static DEFINES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^[ \t]*(?:pub(?:\([^)]*\))?[ \t]+)?(?:(?:const|async|unsafe)[ \t]+)*(?:extern[ \t]+"[^"]*"[ \t]+)?(?:fn|struct|enum|union|mod|trait|type|const|static|macro_rules!)[ \t]+(?:mut[ \t]+)?([A-Za-z_][A-Za-z0-9_]*)"#,
    )
    .expect("the definition pattern")
});

/// A fragment's item as `path::name`. An item whose first segment is a
/// path is kept; otherwise the path is the one file that defines its last
/// segment and whose text holds every other segment as a word.
pub fn resolve(
    root: &Path,
    item: &str,
    defs: &BTreeMap<String, BTreeSet<String>>,
) -> Result<String, String> {
    let segs: Vec<&str> = item.split("::").collect();
    let Some((&last, outer)) = segs.split_last() else {
        return Err(format!("{item}: an empty item"));
    };
    if segs[0].contains('/') || segs[0].ends_with(".rs") {
        return Ok(item.to_string());
    }
    let mut files: Vec<&String> = defs
        .get(last)
        .map(|s| s.iter().collect())
        .unwrap_or_default();
    if !outer.is_empty() {
        files.retain(|f| {
            let body = fs::read_to_string(root.join(f)).unwrap_or_default();
            outer.iter().all(|s| holds_word(&body, s))
        });
    }
    match files.as_slice() {
        [one] => Ok(format!("{one}::{item}")),
        [] if outer.is_empty() => Err(format!(
            "{item}: no tracked .rs file defines {last}; write the row as path::{item}"
        )),
        [] => Err(format!(
            "{item}: no tracked .rs file defines {last} and names {}; write the row as path::{item}",
            outer.join("::")
        )),
        several => Err(format!(
            "{item}: {} define {last}; write the row as path::{item}",
            several
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Whether `word` occurs in `text` with no identifier character either side.
pub fn holds_word(text: &str, word: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    !word.is_empty()
        && text.match_indices(word).any(|(i, _)| {
            let before = text[..i].chars().next_back();
            let after = text[i + word.len()..].chars().next();
            !before.is_some_and(ident) && !after.is_some_and(ident)
        })
}

/// `cargo xtask citations --merge <fragment>...`: the fragments' rows added
/// to citations-code.tsv, each item given its path, then docs/citations.md
/// written. Nothing is written when an item's path cannot be found.
pub fn merge(root: &Path, fragments: &[&str], out: &mut impl Write) -> io::Result<Outcome> {
    let mut all = match fs::read_to_string(root.join(CODE)) {
        Ok(t) => match rows(&t, CODE) {
            Ok(r) => r,
            Err(e) => {
                writeln!(out, "  FAIL {e}")?;
                return Ok(Outcome::Failed);
            }
        },
        Err(_) => Vec::new(),
    };
    let defs = definitions(root)?;
    let mut added = 0usize;
    let mut unresolved = Vec::new();
    for f in fragments {
        let t = fs::read_to_string(f).map_err(|e| io::Error::other(format!("{f}: {e}")))?;
        let new = match rows(&t, f) {
            Ok(r) => r,
            Err(e) => {
                writeln!(out, "  FAIL {e}")?;
                return Ok(Outcome::Failed);
            }
        };
        for mut r in new {
            match resolve(root, &r.item, &defs) {
                Ok(item) => r.item = item,
                Err(e) => {
                    unresolved.push(format!("{f}: {e}"));
                    continue;
                }
            }
            if !all.iter().any(|a| a.key == r.key && a.item == r.item) {
                all.push(r);
                added += 1;
            }
        }
    }
    if !unresolved.is_empty() {
        for e in &unresolved {
            writeln!(out, "  FAIL {e}")?;
        }
        return Ok(Outcome::Failed);
    }
    if let Some(dir) = root.join(CODE).parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(root.join(CODE), code_file(&all))?;
    writeln!(out, "  ok   {added} rows added to {CODE}")?;
    run(root, Mode::Write, out)
}
