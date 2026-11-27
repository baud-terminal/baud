//! **The crates.io crates the `baud` binary links**, and the register of
//! their notices, `docs/research/crates.toml`, that `cargo xtask credits`
//! prints them from.
//!
//! **What the binary links is found, not listed.** For each target baud
//! ships, `cargo metadata --locked --offline --filter-platform` gives the
//! graph `Cargo.lock` resolves, and the walk from the `baud` package
//! follows normal dependency edges only: a build or dev dependency is not
//! in the program file, and neither is a proc-macro crate or anything only
//! a proc-macro uses. A crate with no registry source is one of baud's own
//! and is walked through, not credited.
//!
//! **The register is written by `cargo xtask credits --crates`**, the one
//! step that reads the crates' sources in the cargo registry: per crate and
//! version, the licence expression its manifest gives, the one it is
//! credited under (MIT, then BSD-2-Clause, then Apache-2.0: the first the
//! expression offers), the file in its source holding its notice, that
//! file's copyright lines word for word and its sha256, and the targets
//! that link it. A crate whose source carries no licence file takes its
//! copyright lines from its upstream repository's licence file, committed
//! under `docs/research/sources/` ([`UPSTREAM`]), or, with none listed, is
//! recorded with none and credited by the authors and repository its
//! manifest names.
//!
//! **The check** fails when the set `Cargo.lock` links differs from the
//! register's rows, when a notice file in the registry no longer hashes to
//! its row, and when a committed upstream licence file no longer hashes to
//! its manifest entry or its row. A box with no registry source for a row
//! says so and skips that row's registry hash rather than failing.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value as Json;
use toml::{Table, Value};

/// The register, from the repository root.
pub const REGISTER: &str = "docs/research/crates.toml";
/// The package the walk starts from: the program file.
pub const PROGRAM: &str = "baud";
/// The command that writes the register.
pub const WRITER: &str = "cargo xtask credits --crates";

/// A target baud ships, and the platform it builds for.
pub struct Target {
    pub triple: &'static str,
    pub platform: &'static str,
}

/// Every target baud ships.
pub const TARGETS: [Target; 3] = [
    Target {
        triple: "aarch64-apple-darwin",
        platform: "macOS",
    },
    Target {
        triple: "x86_64-unknown-linux-gnu",
        platform: "Linux",
    },
    Target {
        triple: "aarch64-unknown-linux-gnu",
        platform: "Linux",
    },
];

/// **The licence a crate offered under several is credited under**: the
/// first of these its expression offers. MIT's terms are met by its notice
/// alone and its text is printed already; BSD-2-Clause's are met the same
/// way; Apache-2.0 asks for its NOTICE file besides.
pub const PREFER: [&str; 3] = ["MIT", "BSD-2-Clause", "Apache-2.0"];

/// A crate and version, as `Cargo.lock` names it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Id {
    pub name: String,
    pub version: String,
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.name, self.version)
    }
}

/// Each linked crate and the targets that link it, in [`TARGETS`]' order.
pub type Linked = BTreeMap<Id, Vec<String>>;

/// One row of the register.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Row {
    pub name: String,
    pub version: String,
    pub targets: Vec<String>,
    /// The licence expression of the crate's manifest.
    pub expression: String,
    /// The licence it is credited under, one of [`PREFER`].
    pub licence: String,
    /// The file in the crate's source holding its notice, from the source's
    /// root; empty when the source carries no licence file.
    pub notice: String,
    /// The notice file's sha256, or the committed source's when
    /// `notice_source` names one; empty with neither.
    pub sha256: String,
    /// For a crate whose source carries no licence file: the key in
    /// `docs/research/sources/MANIFEST.toml` of the committed licence file
    /// of its upstream repository, which its copyright lines are read from.
    pub notice_source: String,
    /// The notice file's copyright lines, word for word.
    pub copyright: Vec<String>,
    /// The authors its manifest names, by name: an address the manifest
    /// gives beside a name (`Name <address>`) is left out.
    pub authors: Vec<String>,
    pub repository: String,
    /// For a crate credited under Apache-2.0 whose source ships a NOTICE
    /// file: that file, its sha256 and its text, printed whole.
    pub apache_notice: String,
    pub apache_notice_sha256: String,
    pub apache_notice_text: String,
}

impl Row {
    pub fn id(&self) -> Id {
        Id {
            name: self.name.clone(),
            version: self.version.clone(),
        }
    }
}

/// **The crates a target links**, from `cargo metadata`'s output: the walk
/// from `from` over normal edges, stopping at proc-macro crates, of every
/// crate with a registry source.
pub fn walk(metadata: &Json, from: &str) -> Result<BTreeSet<Id>, String> {
    let empty = Vec::new();
    let packages = metadata["packages"].as_array().unwrap_or(&empty);
    let by_id: BTreeMap<&str, &Json> = packages
        .iter()
        .filter_map(|p| p["id"].as_str().map(|i| (i, p)))
        .collect();
    let nodes: BTreeMap<&str, &Json> = metadata["resolve"]["nodes"]
        .as_array()
        .unwrap_or(&empty)
        .iter()
        .filter_map(|n| n["id"].as_str().map(|i| (i, n)))
        .collect();
    let start = packages
        .iter()
        .find(|p| p["name"] == from && p["source"].is_null())
        .and_then(|p| p["id"].as_str())
        .ok_or_else(|| format!("cargo metadata: no package {from} in the workspace"))?;
    let proc_macro = |p: &Json| {
        p["targets"].as_array().unwrap_or(&empty).iter().any(|t| {
            t["kind"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .any(|k| k == "proc-macro")
        })
    };
    let mut out = BTreeSet::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut todo = vec![start];
    while let Some(at) = todo.pop() {
        if !seen.insert(at) {
            continue;
        }
        let p = by_id
            .get(at)
            .ok_or_else(|| format!("cargo metadata: no package {at}"))?;
        if proc_macro(p) {
            continue;
        }
        if !p["source"].is_null() {
            out.insert(Id {
                name: p["name"].as_str().unwrap_or_default().to_owned(),
                version: p["version"].as_str().unwrap_or_default().to_owned(),
            });
        }
        let Some(node) = nodes.get(at) else {
            return Err(format!("cargo metadata: {at} is not in the resolve graph"));
        };
        for d in node["deps"].as_array().unwrap_or(&empty) {
            let normal = d["dep_kinds"]
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .any(|k| k["kind"].is_null());
            if let (true, Some(to)) = (normal, d["pkg"].as_str()) {
                todo.push(to);
            }
        }
    }
    Ok(out)
}

/// The licence of [`PREFER`] an expression offers first. Only a choice of
/// licences is read: an expression that joins two with `AND`, or adds an
/// exception, asks for both and is refused rather than half credited.
pub fn choose(expression: &str) -> Result<&'static str, String> {
    let words: Vec<&str> = expression
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')'))
        .filter(|w| !w.is_empty())
        .collect();
    if words.iter().any(|w| matches!(*w, "AND" | "WITH")) {
        return Err(format!(
            "the licence {expression:?} asks for more than one licence's terms"
        ));
    }
    let offered: Vec<&str> = words
        .iter()
        .filter(|w| **w != "OR")
        .flat_map(|w| w.split('/'))
        .collect();
    PREFER
        .iter()
        .find(|l| offered.contains(l))
        .copied()
        .ok_or_else(|| format!("the licence {expression:?} offers none of {PREFER:?}"))
}

/// **(a)**: every crate linked has a row, for the same targets, and every
/// row is a crate linked.
pub fn set_errors(linked: &Linked, rows: &[Row]) -> Vec<String> {
    let mut errs = Vec::new();
    let mut held: BTreeMap<Id, &Row> = BTreeMap::new();
    for r in rows {
        if held.insert(r.id(), r).is_some() {
            errs.push(format!("{REGISTER}: {} has two rows", r.id()));
        }
    }
    for (id, targets) in linked {
        match held.get(id) {
            None => errs.push(format!(
                "{id} is linked ({}) and has no row in {REGISTER}: run {WRITER}",
                targets.join(", ")
            )),
            Some(r) if r.targets != *targets => errs.push(format!(
                "{id} is linked on {} and its row in {REGISTER} says {}: run {WRITER}",
                targets.join(", "),
                r.targets.join(", ")
            )),
            Some(_) => {}
        }
    }
    for id in held.keys().filter(|id| !linked.contains_key(id)) {
        errs.push(format!(
            "{id} has a row in {REGISTER} and the program does not link it: run {WRITER}"
        ));
    }
    errs
}

/// What the notice check found: the rows whose files differ, and the rows
/// it could not read for want of their source.
#[derive(Default, Debug)]
pub struct Notices {
    pub errors: Vec<String>,
    pub skipped: Vec<String>,
}

/// **(b)**: each row's notice file in the registry still hashes to the row.
pub fn notice_errors(rows: &[Row], sources: &[PathBuf]) -> Notices {
    let mut n = Notices::default();
    for r in rows {
        let id = r.id();
        let Some(dir) = source_of(&id, sources) else {
            n.skipped.push(format!(
                "{id}: no source in the cargo registry, so its notice file is not hashed here"
            ));
            continue;
        };
        let files = licence_files(&dir);
        if r.notice.is_empty() {
            if !files.is_empty() {
                n.errors.push(format!(
                    "{id}: its row names no notice file and its source holds {}: run {WRITER}",
                    files.join(", ")
                ));
            }
        } else {
            let at = dir.join(&r.notice);
            match crate::sha256::file(&at) {
                Ok(h) if h == r.sha256 => {}
                Ok(_) => n.errors.push(format!(
                    "{id}: {} no longer hashes to its row in {REGISTER}: run {WRITER} and \
                     review the change",
                    r.notice
                )),
                Err(e) => n.errors.push(format!("{id}: {}: {e}", at.display())),
            }
        }
        if !r.apache_notice.is_empty() {
            let at = dir.join(&r.apache_notice);
            if crate::sha256::file(&at).ok().as_deref() != Some(r.apache_notice_sha256.as_str()) {
                n.errors.push(format!(
                    "{id}: {} no longer hashes to its row in {REGISTER}: run {WRITER}",
                    r.apache_notice
                ));
            }
        }
    }
    n
}

/// The files at the root of a crate's source that hold a licence or a
/// notice: `LICENSE*`, `LICENCE*`, `COPYING*`, `COPYRIGHT*` and `NOTICE*`.
fn licence_files(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|f| {
                    let u = f.to_ascii_uppercase();
                    ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"]
                        .iter()
                        .any(|p| u.starts_with(p))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The registry's source directories: `$CARGO_HOME/registry/src/*`.
pub fn registry_sources() -> Vec<PathBuf> {
    let home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".cargo")));
    let Some(home) = home else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = fs::read_dir(home.join("registry/src"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// The directory of `id`'s source under one of `sources`.
pub fn source_of(id: &Id, sources: &[PathBuf]) -> Option<PathBuf> {
    sources
        .iter()
        .map(|s| s.join(format!("{}-{}", id.name, id.version)))
        .find(|d| d.is_dir())
}

/// `cargo metadata` for one target, as `Cargo.lock` resolves it.
pub fn metadata(root: &Path, triple: &str) -> Result<Json, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--format-version",
            "1",
            "--filter-platform",
            triple,
        ])
        .current_dir(root)
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata --filter-platform {triple}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))
}

/// **Every crate the program links**, on every target it ships for.
pub fn linked(root: &Path) -> Result<Linked, String> {
    let mut all = Linked::new();
    for t in &TARGETS {
        for id in walk(&metadata(root, t.triple)?, PROGRAM)? {
            all.entry(id).or_default().push(t.triple.to_owned());
        }
    }
    Ok(all)
}

/// The rows of a register's text.
pub fn parse(text: &str) -> Result<Vec<Row>, String> {
    let t: Table = text.parse().map_err(|e| format!("{e}"))?;
    let Some(rows) = t.get("crate") else {
        return Ok(Vec::new());
    };
    let rows = rows
        .as_array()
        .ok_or_else(|| String::from("`crate` is not an array of tables"))?;
    let mut out = Vec::new();
    for (k, v) in rows.iter().enumerate() {
        let r = v
            .as_table()
            .ok_or_else(|| format!("row {k} is not a table"))?;
        let s = |key: &str| -> Result<String, String> {
            match r.get(key) {
                Some(Value::String(s)) => Ok(s.clone()),
                None => Ok(String::new()),
                Some(_) => Err(format!("row {k}: {key} is not a string")),
            }
        };
        let list = |key: &str| -> Result<Vec<String>, String> {
            match r.get(key) {
                None => Ok(Vec::new()),
                Some(Value::Array(a)) => a
                    .iter()
                    .map(|x| {
                        x.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("row {k}: {key} holds a non-string"))
                    })
                    .collect(),
                Some(_) => Err(format!("row {k}: {key} is not an array")),
            }
        };
        let row = Row {
            name: s("name")?,
            version: s("version")?,
            targets: list("targets")?,
            expression: s("expression")?,
            licence: s("licence")?,
            notice: s("notice")?,
            sha256: s("sha256")?,
            notice_source: s("notice_source")?,
            copyright: list("copyright")?,
            authors: list("authors")?,
            repository: s("repository")?,
            apache_notice: s("apache_notice")?,
            apache_notice_sha256: s("apache_notice_sha256")?,
            apache_notice_text: s("apache_notice_text")?,
        };
        if row.name.is_empty() || row.version.is_empty() || row.licence.is_empty() {
            return Err(format!(
                "row {k}: a name, a version and a licence are required"
            ));
        }
        out.push(row);
    }
    Ok(out)
}

/// The register's rows.
pub fn load(root: &Path) -> Result<Vec<Row>, String> {
    let text = fs::read_to_string(root.join(REGISTER)).map_err(|e| format!("{REGISTER}: {e}"))?;
    parse(&text).map_err(|e| format!("{REGISTER}: {e}"))
}

/// The register's head: what it is, what writes it and what checks it.
const HEAD: &str = "\
# baud -- the crates.io crates the `baud` binary links, and their notices.
#
# Written by `cargo xtask credits --crates` from Cargo.lock and the crates'
# sources in the cargo registry; `cargo xtask credits` prints each row in
# crates/baud-app/assets/CREDITS.txt. `cargo xtask credits --check` fails if the
# crates Cargo.lock links on a shipped target are not these rows, or a
# row's notice file in the registry no longer hashes to `sha256`.
#
# Fields:
#   targets     the shipped targets that link it
#   expression  the licence expression of its manifest
#   licence     the one it is credited under: MIT, then BSD-2-Clause, then
#               Apache-2.0, the first the expression offers
#   notice      the file in its source holding its notice; empty where the
#               source carries no licence file
#   sha256      that file's digest, or notice_source's
#   notice_source
#               for a crate whose source carries no licence file, the key
#               in docs/research/sources/MANIFEST.toml of the committed
#               licence file of its upstream repository, read in its place
#   copyright   that file's copyright lines, word for word
#   authors     the authors its manifest names
#   repository  the repository its manifest names
#   apache_notice, apache_notice_sha256, apache_notice_text
#               for a crate credited under Apache-2.0, the NOTICE file its
#               source ships, printed whole
";

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

/// The register's text, from its rows.
pub fn render(rows: &[Row]) -> String {
    let mut o = String::from(HEAD);
    let list = |v: &[String]| match v {
        [] => String::from("[]"),
        [one] => format!("[{}]", toml_string(one)),
        many => {
            let mut s = String::from("[\n");
            for x in many {
                s.push_str("  ");
                s.push_str(&toml_string(x));
                s.push_str(",\n");
            }
            s.push(']');
            s
        }
    };
    for r in rows {
        o.push_str("\n[[crate]]\n");
        for (k, v) in [
            ("name", &r.name),
            ("version", &r.version),
            ("expression", &r.expression),
            ("licence", &r.licence),
            ("notice", &r.notice),
            ("sha256", &r.sha256),
            ("notice_source", &r.notice_source),
        ] {
            o.push_str(&format!("{k} = {}\n", toml_string(v)));
        }
        for (k, v) in [
            ("targets", &r.targets),
            ("copyright", &r.copyright),
            ("authors", &r.authors),
        ] {
            o.push_str(&format!("{k} = {}\n", list(v)));
        }
        o.push_str(&format!("repository = {}\n", toml_string(&r.repository)));
        if !r.apache_notice.is_empty() {
            for (k, v) in [
                ("apache_notice", &r.apache_notice),
                ("apache_notice_sha256", &r.apache_notice_sha256),
                ("apache_notice_text", &r.apache_notice_text),
            ] {
                o.push_str(&format!("{k} = {}\n", toml_string(v)));
            }
        }
    }
    o
}

/// The words that say a file is the licence's, for each of [`PREFER`], and
/// the word its file's name carries when a crate ships one per licence.
fn marker(licence: &str) -> (&'static str, &'static str) {
    match licence {
        "MIT" => ("Permission is hereby granted", "MIT"),
        "BSD-2-Clause" => ("Redistribution and use in source and binary forms", "BSD"),
        _ => ("Apache License", "APACHE"),
    }
}

/// Whether a notice line is a copyright line.
fn is_copyright(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.starts_with("copyright") || l.starts_with("(c)") || line.starts_with('\u{a9}')
}

/// A manifest author by name: `Name <address>` gives `Name`, and an entry
/// that is only an address gives nothing.
pub fn author_name(author: &str) -> String {
    match author.split_once('<') {
        Some((name, _)) => name.trim().to_owned(),
        None if author.contains('@') => String::new(),
        None => author.trim().to_owned(),
    }
}

/// A crate's row, read from its source `dir`; `root` holds the licence
/// texts a notice is checked against.
pub fn read_row(root: &Path, id: &Id, targets: &[String], dir: &Path) -> Result<Row, String> {
    let at = dir.join("Cargo.toml");
    let manifest: Table = fs::read_to_string(&at)
        .map_err(|e| format!("{}: {e}", at.display()))?
        .parse()
        .map_err(|e| format!("{}: {e}", at.display()))?;
    let package = manifest
        .get("package")
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{}: no [package]", at.display()))?;
    let field = |k: &str| {
        package
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let expression = field("license");
    if expression.is_empty() {
        return Err(format!(
            "{id}: its manifest gives no licence expression, so its licence is read by a person \
             and credited by hand"
        ));
    }
    let licence = choose(&expression).map_err(|e| format!("{id}: {e}"))?;
    let (words, tag) = marker(licence);
    let files = licence_files(dir);
    let read = |f: &str| fs::read_to_string(dir.join(f)).unwrap_or_default();
    let mut theirs: Vec<&String> = files
        .iter()
        .filter(|f| !f.to_ascii_uppercase().starts_with("NOTICE"))
        .filter(|f| crate::credits::flat(&read(f)).contains(&crate::credits::flat(words)))
        .collect();
    theirs.sort_by_key(|f| !f.to_ascii_uppercase().contains(tag));
    let mut row = Row {
        name: id.name.clone(),
        version: id.version.clone(),
        targets: targets.to_vec(),
        expression,
        licence: licence.to_owned(),
        authors: package
            .get("authors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(author_name)
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        repository: field("repository"),
        ..Row::default()
    };
    match theirs.first() {
        Some(f) => {
            row.copyright = copyright_of(root, id, licence, f, &read(f))?;
            row.notice = (*f).clone();
            row.sha256 =
                crate::sha256::file(&dir.join(f)).map_err(|e| format!("{id}: {f}: {e}"))?;
        }
        None if files.is_empty() => {
            if let Some(u) = UPSTREAM.iter().find(|u| u.repository == row.repository) {
                let c = committed(root, u.key)?;
                let at = format!("{SOURCES}/{}", c.file);
                let text = fs::read_to_string(root.join(&at)).map_err(|e| format!("{at}: {e}"))?;
                row.copyright = copyright_of(root, id, licence, &at, &text)?;
                row.notice_source = u.key.to_owned();
                row.sha256 = c.sha256;
            }
        }
        None => {
            return Err(format!(
                "{id}: its source holds {} and none is its {licence} notice",
                files.join(", ")
            ));
        }
    }
    if licence == "Apache-2.0" {
        if let Some(f) = files
            .iter()
            .find(|f| f.to_ascii_uppercase().starts_with("NOTICE"))
        {
            row.apache_notice = f.clone();
            row.apache_notice_sha256 =
                crate::sha256::file(&dir.join(f)).map_err(|e| format!("{id}: {f}: {e}"))?;
            row.apache_notice_text = read(f).replace("\r\n", "\n").trim_end().to_owned();
        }
    }
    Ok(row)
}

/// **`--crates`**: the register's rows, read from the registry for every
/// crate the program links.
pub fn rows_from_registry(root: &Path) -> Result<Vec<Row>, Vec<String>> {
    let linked = linked(root).map_err(|e| vec![e])?;
    let sources = registry_sources();
    let mut rows = Vec::new();
    let mut errs = Vec::new();
    for (id, targets) in &linked {
        let Some(dir) = source_of(id, &sources) else {
            errs.push(format!(
                "{id}: no source in the cargo registry; `cargo fetch --locked` brings it"
            ));
            continue;
        };
        match read_row(root, id, targets, &dir) {
            Ok(r) => rows.push(r),
            Err(e) => errs.push(e),
        }
    }
    match errs.is_empty() {
        true => Ok(rows),
        false => Err(errs),
    }
}

/// **The copyright lines of a notice** `text` (from `what`) under
/// `licence`: the lines before its permission text that name a copyright.
/// An MIT text that is not the one the file prints is refused, for a person
/// to read before it is credited.
fn copyright_of(
    root: &Path,
    id: &Id,
    licence: &str,
    what: &str,
    text: &str,
) -> Result<Vec<String>, String> {
    let text_of = crate::credits::licence(licence).ok_or_else(|| {
        format!("{id}: is credited under {licence}, and crates/xtask/src/credits.rs prints no text of it")
    })?;
    let (words, _) = marker(licence);
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    let k = lines
        .iter()
        .position(|l| l.contains(words))
        .unwrap_or(lines.len());
    if licence == "MIT" {
        let printed = crate::credits::licence_text(root, text_of)?;
        let body = lines[k..].join("\n");
        if crate::credits::flat(&body) != crate::credits::flat(&printed) {
            return Err(format!(
                "{id}: {what} holds an MIT text other than the one printed, which a person \
                 reads before it is credited"
            ));
        }
    }
    Ok(lines[..k]
        .iter()
        .map(|l| l.trim())
        .filter(|l| is_copyright(l))
        .map(str::to_owned)
        .collect())
}

/// Where the committed sources are, from the repository root.
pub const SOURCES: &str = "docs/research/sources";

/// **A crate family whose crates.io sources carry no licence file**, and
/// the committed licence file of their upstream repository, by its key in
/// the sources manifest, that their copyright lines are read from. A crate
/// of the family is one whose manifest names `repository`.
pub struct Upstream {
    pub repository: &'static str,
    pub key: &'static str,
}

/// Every such family: the objc2 crates, whose repository's licence file at
/// the commit the manifest pins names their copyright holder.
pub const UPSTREAM: [Upstream; 1] = [Upstream {
    repository: "https://github.com/madsmtm/objc2",
    key: "objc2-license-mit",
}];

/// A committed source, as the sources manifest records it.
pub struct Committed {
    /// Its file under [`SOURCES`].
    pub file: String,
    pub sha256: String,
    pub url: String,
}

/// The source the manifest records under `key`.
pub fn committed(root: &Path, key: &str) -> Result<Committed, String> {
    let manifest = crate::third_party::MANIFEST;
    let man = crate::third_party::load(root, manifest)?;
    let entry = man
        .get("sources")
        .and_then(|s| s.get(key))
        .and_then(Value::as_table)
        .ok_or_else(|| format!("{manifest} holds no source {key}"))?;
    let s = |k: &str| {
        entry
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let c = Committed {
        file: s("file"),
        sha256: s("sha256"),
        url: s("url"),
    };
    match c.file.is_empty() || c.sha256.is_empty() {
        true => Err(format!(
            "{manifest}: source {key} names no committed file and digest"
        )),
        false => Ok(c),
    }
}

/// **(b) for a notice from a committed source**: the row names a source the
/// manifest holds, and the committed file hashes to the manifest and to the
/// row.
pub fn source_errors(root: &Path, rows: &[Row]) -> Vec<String> {
    let mut errs = Vec::new();
    for r in rows.iter().filter(|r| !r.notice_source.is_empty()) {
        let id = r.id();
        let c = match committed(root, &r.notice_source) {
            Ok(c) => c,
            Err(e) => {
                errs.push(format!("{id}: {e}"));
                continue;
            }
        };
        let at = format!("{SOURCES}/{}", c.file);
        match crate::sha256::file(&root.join(&at)) {
            Ok(h) if h != c.sha256 => errs.push(format!(
                "{id}: {at} no longer hashes to its entry in {}",
                crate::third_party::MANIFEST
            )),
            Ok(h) if h != r.sha256 => errs.push(format!(
                "{id}: {at} no longer hashes to its row in {REGISTER}: run {WRITER}"
            )),
            Ok(_) => {}
            Err(e) => errs.push(format!("{id}: {at}: {e}")),
        }
    }
    errs
}
