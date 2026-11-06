//! `cargo xtask third-party [--check]`: THIRD_PARTY.md, the index of every
//! third-party file in the tree, written from docs/research/third-party.toml
//! and docs/research/sources/MANIFEST.toml.
//!
//! ```text
//! cargo xtask third-party           write THIRD_PARTY.md
//! cargo xtask third-party --check   fail if anything below is wrong,
//!                                   THIRD_PARTY.md included
//! ```
//!
//! The checks, each of which has to be able to fail:
//!
//! 1. every `retrieved` file in the manifest is in exactly one group, and
//!    no group names a file the manifest does not hold as `retrieved`;
//! 2. every file in LICENSES/ has a [text] entry and hashes to it, every
//!    entry's file exists, and every text is cited by some group or
//!    [[tree]] entry, or is in the `needs` of a text that is: a text
//!    another one names as part of its notice is cited through it, and a
//!    `needs` naming no text fails;
//! 3. every `notice` that is a path, and every [[tree]] path, exists;
//! 4. every status is one the sidecar defines;
//! 5. LICENSE is the Creative Commons text and nothing else, byte for byte
//!    (`LICENCE_SHA256`), and the READMEs say what it leaves to others
//!    (`README_SAYS`);
//! 6. every font in the tree has a row, baud's own under `own`; every file
//!    of a font package (a directory holding the OFL.txt, LICENSE or LICENCE
//!    a row names as its notice) has a row or is that notice; a third-party
//!    font's notice sits in the font's directory; and a notice in prose
//!    that names a file names one beside its row's files (see `rowed`);
//! 7. a [[tree]] entry whose notice is "in file", under a licence whose URI
//!    this module knows, names that URI in each of its files: the URI is
//!    what a Creative Commons licence asks a copy to carry, and a header
//!    the generator stopped writing would otherwise pass;
//! 8. every file the provenance table (`PROVENANCE`) classes (a),
//!    transcribed from another project, is in a [[tree]] entry whose notice
//!    is the text the table names; every file of `PROVENANCE_DIRS` has a
//!    class there, and every file the table names exists (see
//!    `provenance`);
//! 9. THIRD_PARTY.md is byte for byte what this module writes.
//!
//! The sidecar's header says what each field means. Nothing here decides a
//! licence: it renders what the sidecar records and refuses to let the
//! sidecar, the manifest and the tree drift apart.
//!
//! A sidecar whose groups name a file the manifest does not hold, or a
//! status it does not define, is reported line by line. The Python this
//! replaced checked both and then stopped on a `KeyError` while rendering,
//! before it printed either line.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use toml::{Table, Value};

use crate::html::Count;
use crate::{no_python, py, sha256, text, toml10};

pub const SIDECAR: &str = "docs/research/third-party.toml";
pub const MANIFEST: &str = "docs/research/sources/MANIFEST.toml";
pub const OUT: &str = "THIRD_PARTY.md";
pub const SRC: &str = "docs/research/sources";

/// Every status the sidecar defines, with what THIRD_PARTY.md says it
/// means.
pub const STATUS: [(&str, &str); 7] = [
    ("kept", "redistributable, and its notice is satisfied"),
    ("own", "the work of baud's author"),
    (
        "nc",
        "kept verbatim under CC BY-NC-SA 3.0, which allows a verbatim copy that carries its attribution and the licence's URI; its NonCommercial term binds every copy of this file, whatever the repository's licence",
    ),
    (
        "copyleft",
        "redistributable with its licence text, which is in LICENSES/; keeping it or making it `external` is a choice still open",
    ),
    (
        "no-grant",
        "no grant to redistribute was found; moving it out of the tree (`external`) is a choice still open",
    ),
    ("unverified", "licence not established; a choice still open"),
    ("open", "an open question, recorded where the file is"),
];
/// The order the summary and the sections list the statuses in.
pub const ORDER: [&str; 7] = [
    "no-grant",
    "unverified",
    "open",
    "copyleft",
    "nc",
    "kept",
    "own",
];

fn status_meaning(s: &str) -> Option<&'static str> {
    STATUS.iter().find(|(k, _)| *k == s).map(|(_, m)| *m)
}

fn status_names() -> String {
    let names: Vec<&str> = STATUS.iter().map(|(k, _)| *k).collect();
    py::repr_sorted(&names)
}

/// The table that classes each font file by where its code came from: (a)
/// transcribed from another project, (b) written from the specification
/// with behaviour matched, (c) neither. Its rows read
/// `` | `path` | a | `LICENSES/text` | `` or `` | `path` | b | -- | ``.
pub const PROVENANCE: &str = "baud-font/PROVENANCE.md";
pub const PROVENANCE_DIRS: [&str; 1] = ["baud-font/src"];
/// The table classes at least this many files: the control for a row
/// pattern that stopped matching, which would find no unrowed
/// transcription and pass.
pub const PROVENANCE_FLOOR: Count = Count(32);
/// The sha256 of LICENSE, the Creative Commons text alone. A third-party
/// file is named by its row, never there, so a line added to LICENSE is a
/// statement this gate has no way to read.
pub const LICENCE_SHA256: &str = "b56a26ec2fd167d676bec8e42f580b32193bc914219b54e2e453e378613b4601";
/// What each README has to say, its line breaks aside, now that it is where
/// this is said: a reader of LICENSE learns nothing about the fonts, and the
/// IJG's condition (2) wants its sentence in the documentation that goes
/// with a binary, which baud-app's README is for `cargo install`.
pub const README_SAYS: [(&str, &str); 4] = [
    (
        "README.md",
        "Fonts and other projects' files keep their own licences, which sit next to them",
    ),
    (
        "README.md",
        "[THIRD_PARTY.md](THIRD_PARTY.md) lists each one with its licence and source",
    ),
    (
        "README.md",
        "This software is based in part on the work of the Independent JPEG Group",
    ),
    (
        "baud-app/README.md",
        "This software is based in part on the work of the Independent JPEG Group",
    ),
];
/// What a font is, by its file's extension.
pub const FONT_EXTENSIONS: [&str; 5] = ["ttf", "otf", "ttc", "woff", "woff2"];
/// The names a font package's own licence has. A directory holding one that
/// a row names as its notice is someone's package, whole; a notice with
/// another name sits among other files (`baud-font/tests/fonts/`, whose
/// `<Family>-LICENSE.txt` share a directory with baud's own fonts).
pub const PACKAGE_NOTICES: [&str; 3] = ["OFL.txt", "LICENSE", "LICENCE"];
/// The tree holds at least this many fonts and this many font packages: the
/// control for a listing or a match that stopped working, which would find
/// no unrowed file and pass.
pub const FONTS_FLOOR: Count = Count(50);
pub const PACKAGES_FLOOR: Count = Count(5);

/// The URI a Creative Commons or document licence asks a copy to carry.
pub fn licence_uri(spdx: &str) -> Option<&'static str> {
    Some(match spdx {
        "CC-BY-4.0" => "https://creativecommons.org/licenses/by/4.0/",
        "CC-BY-SA-4.0" => "https://creativecommons.org/licenses/by-sa/4.0/",
        "CC-BY-ND-4.0" => "https://creativecommons.org/licenses/by-nd/4.0/",
        "CC-BY-NC-SA-3.0" => "https://creativecommons.org/licenses/by-nc-sa/3.0/",
        "CC0-1.0" => "https://creativecommons.org/publicdomain/zero/1.0/",
        "LicenseRef-W3C-Document" => "https://www.w3.org/copyright/document-license/",
        "LicenseRef-IETF-RFC" => "https://trustee.ietf.org/documents/trust-legal-provisions/",
        "LicenseRef-Ecma-Copyright" => {
            "https://www.ecma-international.org/policies/by-ipr/ecma-text-copyright-policy/"
        }
        _ => return None,
    })
}

fn is_path(notice: &str) -> bool {
    notice.contains('/') || notice == "LICENSE"
}

static NAMED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9_.-]+\.txt").expect("the named-file pattern"));

/// The files a notice in prose names, each beside its row's files:
/// "the two fonts' OFL texts beside it (A-LICENSE.txt, B-LICENSE.txt)".
pub fn named_beside(notice: &str) -> Vec<&str> {
    if is_path(notice) {
        return Vec::new();
    }
    NAMED.find_iter(notice).map(|m| m.as_str()).collect()
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn is_font(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| FONT_EXTENSIONS.iter().any(|f| f.eq_ignore_ascii_case(ext)))
}

/// A notice as THIRD_PARTY.md shows it: a link when it is a file.
fn notice_md(notice: &str) -> String {
    if is_path(notice) {
        return format!("[{notice}]({notice})");
    }
    match notice {
        "uri" => "the licence's URI, below",
        "none needed" => "none needed",
        "none" => "none",
        "in file" => "in the file",
        other => other,
    }
    .to_string()
}

/// A TOML file read as `tomllib.load` read it: TOML 1.0 only.
pub fn load(root: &Path, path: &str) -> Result<Table, String> {
    let raw = fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    let text = String::from_utf8(raw).map_err(|e| format!("{path}: {e}"))?;
    if let Some(r) = toml10::first_refusal(&text) {
        return Err(format!("{path}: {r}"));
    }
    text.parse::<Table>()
        .map_err(|e| format!("{path}: {}", toml10::described(&text, &e)))
}

/// `t[key]` as a string, or the `KeyError` Python stopped on.
fn s<'a>(t: &'a Table, key: &str) -> Result<&'a str, String> {
    match t.get(key) {
        Some(Value::String(v)) => Ok(v),
        Some(other) => Err(format!("TypeError: {key} is {}", py::repr(Some(other)))),
        None => Err(format!("KeyError: {}", py::repr_str(key))),
    }
}

fn strings<'a>(t: &'a Table, key: &str) -> Result<Vec<&'a str>, String> {
    match t.get(key) {
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or(format!("TypeError: {key} holds {}", py::repr(Some(v))))
            })
            .collect(),
        Some(other) => Err(format!("TypeError: {key} is {}", py::repr(Some(other)))),
        None => Err(format!("KeyError: {}", py::repr_str(key))),
    }
}

fn table<'a>(t: &'a Table, key: &str) -> Result<&'a Table, String> {
    t.get(key)
        .and_then(Value::as_table)
        .ok_or(format!("KeyError: {}", py::repr_str(key)))
}

/// One `[upstream.*]` group.
pub struct Group<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub licence: &'a str,
    pub notice: &'a str,
    pub status: &'a str,
    pub files: Vec<&'a str>,
}

/// One `[[tree]]` entry.
pub struct Tree<'a> {
    pub what: &'a str,
    pub licence: &'a str,
    pub notice: &'a str,
    pub upstream: &'a str,
    pub status: &'a str,
    pub paths: Vec<&'a str>,
}

/// One `[text.*]` licence text.
pub struct Text<'a> {
    pub name: &'a str,
    pub licence: &'a str,
    pub how: &'a str,
    pub from: &'a str,
    pub sha256: &'a str,
    pub needs: Vec<&'a str>,
}

/// The sidecar, read.
pub struct Side<'a> {
    pub upstream: Vec<Group<'a>>,
    pub tree: Vec<Tree<'a>>,
    pub text: Vec<Text<'a>>,
}

impl<'a> Side<'a> {
    pub fn of(side: &'a Table) -> Result<Self, String> {
        let upstream = table(side, "upstream")?
            .iter()
            .map(|(id, g)| {
                let g = g.as_table().ok_or(format!("TypeError: upstream.{id}"))?;
                Ok(Group {
                    id,
                    name: s(g, "name")?,
                    licence: s(g, "licence")?,
                    notice: s(g, "notice")?,
                    status: s(g, "status")?,
                    files: strings(g, "files")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let tree = match side.get("tree") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|t| {
                    let t = t.as_table().ok_or("TypeError: a [[tree]] entry")?;
                    Ok(Tree {
                        what: s(t, "what")?,
                        licence: s(t, "licence")?,
                        notice: s(t, "notice")?,
                        upstream: s(t, "upstream")?,
                        status: s(t, "status")?,
                        paths: strings(t, "paths")?,
                    })
                })
                .collect::<Result<_, String>>()?,
            _ => return Err("KeyError: 'tree'".to_string()),
        };
        let text = table(side, "text")?
            .iter()
            .map(|(name, t)| {
                let t = t.as_table().ok_or(format!("TypeError: text.{name}"))?;
                Ok(Text {
                    name,
                    licence: s(t, "licence")?,
                    how: s(t, "how")?,
                    from: s(t, "from")?,
                    sha256: s(t, "sha256")?,
                    needs: if t.contains_key("needs") {
                        strings(t, "needs")?
                    } else {
                        Vec::new()
                    },
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            upstream,
            tree,
            text,
        })
    }

    fn text(&self, name: &str) -> Option<&Text<'a>> {
        self.text.iter().find(|t| t.name == name)
    }
}

/// file -> the url of its copy, from the first key (in key order) naming
/// it.
pub fn urls(man: &Table) -> BTreeMap<String, String> {
    let mut keys: Vec<&String> = man.keys().collect();
    keys.sort();
    let mut url = BTreeMap::new();
    for key in keys {
        let Some(e) = man[key].as_table() else {
            continue;
        };
        let retrieved = e.get("status").and_then(Value::as_str) == Some("retrieved");
        let file = e.get("file").and_then(Value::as_str).unwrap_or("");
        if retrieved && !file.is_empty() {
            let u = e
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            url.entry(file.to_string()).or_insert(u);
        }
    }
    url
}

fn exists(root: &Path, p: &str) -> bool {
    root.join(p).exists()
}

/// What LICENSE and the READMEs say against what they must.
pub fn licence_and_readmes(root: &Path) -> io::Result<Vec<String>> {
    let mut errs = Vec::new();
    let Ok(licence) = fs::read(root.join("LICENSE")) else {
        return Ok(vec!["LICENSE is missing".to_string()]);
    };
    let got = sha256::hex(&licence);
    if got != LICENCE_SHA256 {
        errs.push(format!(
            "LICENSE is not the Creative Commons text alone: it hashes {got}, not {LICENCE_SHA256}; \
             a third-party file is named by its row in {SIDECAR}"
        ));
    }
    for (file, says) in README_SAYS {
        let text = fs::read_to_string(root.join(file)).unwrap_or_default();
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if !flat.contains(says) {
            errs.push(format!("{file} does not say {}", py::repr_str(says)));
        }
    }
    Ok(errs)
}

/// The third-party files the tree holds against the rows that name them:
/// `files` is what `git ls-files` lists. Returns what is wrong and how many
/// fonts were seen.
pub fn rowed(root: &Path, side: &Side, files: &[String]) -> (Vec<String>, Count) {
    let mut errs = Vec::new();
    let mut named: BTreeSet<String> = side
        .upstream
        .iter()
        .flat_map(|g| g.files.iter().map(|f| format!("{SRC}/{f}")))
        .collect();
    let mut notices: BTreeSet<String> = BTreeSet::new();
    let mut packages: BTreeSet<String> = BTreeSet::new();
    for (i, t) in side.tree.iter().enumerate() {
        named.extend(t.paths.iter().map(|p| p.to_string()));
        if is_path(t.notice) {
            notices.insert(t.notice.to_string());
            let file = t.notice.rsplit('/').next().unwrap_or("");
            if t.notice.contains('/')
                && !t.notice.starts_with("LICENSES/")
                && PACKAGE_NOTICES.contains(&file)
            {
                packages.insert(format!("{}/", parent(t.notice)));
            }
        }
        for name in named_beside(t.notice) {
            for dir in t.paths.iter().map(|p| parent(p)).collect::<BTreeSet<_>>() {
                let beside = format!("{dir}/{name}");
                if !exists(root, &beside) {
                    errs.push(format!(
                        "[[tree]] {i}: its notice names {name}, which is not beside its files in {dir}"
                    ));
                }
                notices.insert(beside);
            }
        }
        if t.status != "own" {
            for font in t.paths.iter().filter(|p| is_font(p)) {
                let beside = if is_path(t.notice) {
                    t.notice.contains('/') && parent(t.notice) == parent(font)
                } else {
                    !named_beside(t.notice).is_empty()
                };
                if !beside {
                    errs.push(format!(
                        "[[tree]] {i}: {font} is a third-party font and its notice, {}, is not beside it",
                        t.notice
                    ));
                }
            }
        }
    }
    let mut fonts = Count(0);
    for f in files {
        if is_font(f) {
            fonts.0 += 1;
            if !named.contains(f) {
                errs.push(format!(
                    "{f} is a font and no row of {SIDECAR} names it: a third-party font needs its \
                     row and its licence beside it, one of baud's own a row under `own`"
                ));
            }
        } else if packages.iter().any(|d| f.starts_with(d.as_str()))
            && !named.contains(f)
            && !notices.contains(f)
        {
            errs.push(format!(
                "{f} is in a font package and no row of {SIDECAR} names it"
            ));
        }
    }
    if fonts.0 < FONTS_FLOOR.0 {
        errs.push(format!(
            "the tree lists {} fonts, fewer than {}: the listing or the match has stopped working, \
             or the floor is stale",
            fonts.0, FONTS_FLOOR.0
        ));
    }
    if packages.len() < PACKAGES_FLOOR.0 {
        errs.push(format!(
            "the rows name {} font packages, fewer than {}: the match has stopped working, \
             or the floor is stale",
            packages.len(),
            PACKAGES_FLOOR.0
        ));
    }
    (errs, fonts)
}

static ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\| `([^`]+)` \| ([^|]*?) \| ([^|]*?) \|").expect("the row pattern")
});
static TEXTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`(LICENSES/[^`]+)`").expect("the text pattern"));

/// What the provenance table says against the sidecar: a file classed (a)
/// whose notice text is under no [[tree]] entry naming it, a file of
/// `PROVENANCE_DIRS` the table does not class, and a row naming a file
/// that is not there or a class the table does not define.
pub fn provenance(root: &Path, side: &Side) -> io::Result<Vec<String>> {
    let mut errs = Vec::new();
    let path = root.join(PROVENANCE);
    if !path.exists() {
        return Ok(vec![format!(
            "{PROVENANCE} is missing, so no transcribed file can be checked"
        )]);
    }
    let table = text::read_universal(&path)?;
    let mut classed: HashMap<String, String> = HashMap::new();
    for (n, line) in table.split_inclusive('\n').enumerate() {
        let n = n + 1;
        let Some(m) = ROW.captures(line) else {
            continue;
        };
        let p = &m[1];
        let cls = text::strip(&m[2]);
        let notice = text::strip(&m[3]);
        if !exists(root, p) {
            errs.push(format!(
                "{PROVENANCE}:{n} classes {p}, which does not exist"
            ));
            continue;
        }
        if classed.contains_key(p) {
            errs.push(format!("{PROVENANCE}:{n} classes {p} a second time"));
        }
        classed.insert(p.to_string(), cls.to_string());
        if !matches!(cls, "a" | "b" | "c") {
            errs.push(format!(
                "{PROVENANCE}:{n} gives {p} class {}, not a, b or c",
                py::repr_str(cls)
            ));
        } else if cls != "a" && notice != "--" {
            errs.push(format!(
                "{PROVENANCE}:{n}: {p} is ({cls}) and names a notice, {notice}"
            ));
        } else if cls == "a" {
            let texts: Vec<&str> = TEXTS
                .captures_iter(notice)
                .map(|c| c.get(1).map_or("", |g| g.as_str()))
                .collect();
            if texts.is_empty() {
                errs.push(format!(
                    "{PROVENANCE}:{n}: {p} is (a) and names no text in LICENSES/"
                ));
            }
            for t in texts {
                if !side
                    .tree
                    .iter()
                    .any(|e| e.notice == t && e.paths.contains(&p))
                {
                    errs.push(format!(
                        "{PROVENANCE}:{n}: {p} is transcribed (a), and no [[tree]] entry \
                         of {SIDECAR} lists it under {t}"
                    ));
                }
            }
        }
    }
    if classed.len() < PROVENANCE_FLOOR.0 {
        errs.push(format!(
            "{PROVENANCE} classes {} files, fewer than {}: \
             the row pattern has stopped matching, or the floor is stale",
            classed.len(),
            PROVENANCE_FLOOR.0
        ));
    }
    for d in PROVENANCE_DIRS {
        let mut names: Vec<String> = fs::read_dir(root.join(d))?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            let p = format!("{d}/{name}");
            if name.ends_with(".rs") && !classed.contains_key(&p) {
                errs.push(format!(
                    "{p} has no class in {PROVENANCE}: read it against what it cites and add its row"
                ));
            }
        }
    }
    Ok(errs)
}

/// Checks 1 to 8.
pub fn check(root: &Path, side: &Side, url: &BTreeMap<String, String>) -> io::Result<Vec<String>> {
    let mut errs = licence_and_readmes(root)?;
    errs.extend(rowed(root, side, &no_python::tracked(root)?).0);
    let mut seen: HashMap<&str, &str> = HashMap::new();
    for g in &side.upstream {
        if status_meaning(g.status).is_none() {
            errs.push(format!(
                "group {}: status {} is not one of {}",
                g.id,
                py::repr_str(g.status),
                status_names()
            ));
        }
        for f in &g.files {
            if let Some(other) = seen.get(f) {
                errs.push(format!("{f} is in two groups, {other} and {}", g.id));
            }
            seen.insert(f, g.id);
            if !url.contains_key(*f) {
                errs.push(format!(
                    "group {} names {f}, which the manifest does not hold as retrieved",
                    g.id
                ));
            }
        }
        if is_path(g.notice) && !exists(root, g.notice) {
            errs.push(format!(
                "group {}: notice {} does not exist",
                g.id, g.notice
            ));
        }
    }
    for f in url.keys() {
        if !seen.contains_key(f.as_str()) {
            errs.push(format!(
                "{SRC}/{f} is retrieved in the manifest and in no group of {SIDECAR}"
            ));
        }
    }

    let mut have: Vec<String> = fs::read_dir(root.join("LICENSES"))?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    have.sort();
    for name in &have {
        if side.text(name).is_none() {
            errs.push(format!("LICENSES/{name} has no [text] entry in {SIDECAR}"));
        }
    }
    let mut cited: BTreeSet<String> = side
        .upstream
        .iter()
        .map(|g| g.notice.to_string())
        .chain(side.tree.iter().map(|t| t.notice.to_string()))
        .collect();
    let mut texts: Vec<&Text> = side.text.iter().collect();
    texts.sort_by(|a, b| a.name.cmp(b.name));
    // A text that names another as part of its own notice cites it.
    for t in &texts {
        for need in &t.needs {
            let bare = need.strip_prefix("LICENSES/").unwrap_or(need);
            if side.text(bare).is_none() {
                errs.push(format!(
                    "[text] {} needs {need}, which has no [text] entry",
                    t.name
                ));
            }
        }
    }
    let mut todo: Vec<String> = cited
        .iter()
        .filter(|c| c.starts_with("LICENSES/"))
        .cloned()
        .collect();
    while let Some(c) = todo.pop() {
        let bare = c.strip_prefix("LICENSES/").unwrap_or(&c);
        for need in side.text(bare).map(|t| t.needs.clone()).unwrap_or_default() {
            if cited.insert(need.to_string()) {
                todo.push(need.to_string());
            }
        }
    }
    for t in &texts {
        let rel = format!("LICENSES/{}", t.name);
        let p = root.join(&rel);
        if !p.exists() {
            errs.push(format!(
                "[text] {} names LICENSES/{}, which does not exist",
                t.name, t.name
            ));
            continue;
        }
        let got = sha256::hex(&fs::read(&p)?);
        if got != t.sha256 {
            errs.push(format!("{rel} hashes {got}, {SIDECAR} says {}", t.sha256));
        }
        if !cited.contains(&rel) {
            errs.push(format!("{rel} is cited by no group and no [[tree]] entry"));
        }
    }

    for (i, t) in side.tree.iter().enumerate() {
        if status_meaning(t.status).is_none() {
            errs.push(format!(
                "[[tree]] {i}: status {} is not one of {}",
                py::repr_str(t.status),
                status_names()
            ));
        }
        for p in &t.paths {
            if !exists(root, p) {
                errs.push(format!(
                    "[[tree]] {i} ({}...): {p} does not exist",
                    py::head(t.what, Count(40))
                ));
            }
        }
        if is_path(t.notice) && !exists(root, t.notice) {
            errs.push(format!("[[tree]] {i}: notice {} does not exist", t.notice));
        }
        if let Some(uri) = licence_uri(t.licence).filter(|_| t.notice == "in file") {
            for p in &t.paths {
                let full = root.join(p);
                if full.is_file() {
                    let body = fs::read(&full)?;
                    let named = body.windows(uri.len()).any(|w| w == uri.as_bytes());
                    if !named {
                        errs.push(format!(
                            "[[tree]] {i}: {p} is {} with its notice in the file, and does not name {uri}",
                            t.licence
                        ));
                    }
                }
            }
        }
    }
    errs.extend(provenance(root, side)?);
    Ok(errs)
}

/// THIRD_PARTY.md, or the line Python stopped on when a group or entry
/// names a status the sidecar does not define or a file the manifest
/// does not hold.
pub fn render(side: &Side, url: &BTreeMap<String, String>) -> Result<String, String> {
    let mut n: HashMap<&str, Count> = STATUS.iter().map(|(k, _)| (*k, Count(0))).collect();
    for g in &side.upstream {
        let c = n
            .get_mut(g.status)
            .ok_or(format!("KeyError: {}", py::repr_str(g.status)))?;
        c.0 += g.files.len();
    }
    let mut t_n: HashMap<&str, Count> = STATUS.iter().map(|(k, _)| (*k, Count(0))).collect();
    for t in &side.tree {
        let c = t_n
            .get_mut(t.status)
            .ok_or(format!("KeyError: {}", py::repr_str(t.status)))?;
        c.0 += 1;
    }

    let mut o = String::new();
    o.push_str("# Third-party files in baud\n\n");
    o.push_str(HEADER);
    o.push_str(
        "baud's own work is under CC BY-NC-SA 4.0 ([LICENSE](LICENSE)). This file lists the files in the\n\
         tree that are someone else's work or carry some of it, as a licence audit of the tree (2026-10-10)\n\
         and a sweep of what scripts/gen-* and scripts/extract-* read and write found them: each file's\n\
         licence, where the notice that licence asks for is, and where it came from. Every file in\n\
         docs/research/sources/ is listed, and the check fails on one that is not. The licence texts the\n\
         files need are beside them or in\n\
         [LICENSES/](LICENSES/), and this file lists them; LICENSE is the Creative Commons text alone.\n\
         Each crate published to crates.io carries LICENSE, in its own LICENSES/ the texts its\n\
         files need, as links to these, and a NOTICE generated from the same rows that names its\n\
         third-party files by their paths in the crate, which `cargo xtask publish` checks. Nothing here is legal\n\
         advice; where a file's status is a choice still open, this file says so and decides nothing.\n\n",
    );
    o.push_str("## Summary\n\n");
    o.push_str("| status | research sources | other entries | meaning |\n|---|---|---|---|\n");
    for st in ORDER {
        let (a, b) = (n[st].0, t_n[st].0);
        if a > 0 || b > 0 {
            o.push_str(&format!(
                "| {st} | {a} | {b} | {} |\n",
                status_meaning(st).unwrap_or("")
            ));
        }
    }
    let total: usize = n.values().map(|c| c.0).sum();
    o.push_str(&format!("| total | {total} | {} | |\n\n", side.tree.len()));

    o.push_str("## Licence texts in LICENSES/\n\n");
    o.push_str(
        "Fetched texts are byte for byte what the URL served; built texts say in their first lines what\n\
         they were built from.\n\n",
    );
    o.push_str("| file | licence | from | sha256 |\n|---|---|---|---|\n");
    let mut texts: Vec<&Text> = side.text.iter().collect();
    texts.sort_by_key(|t| t.name.to_lowercase());
    for t in texts {
        o.push_str(&format!(
            "| [{}](LICENSES/{}) | {} | {}: {} | {} |\n",
            t.name,
            t.name,
            t.licence,
            t.how,
            t.from,
            py::head(t.sha256, Count(16))
        ));
    }
    o.push('\n');

    o.push_str("## In the tree outside docs/research/sources/\n\n");
    for st in ORDER {
        for t in side.tree.iter().filter(|t| t.status == st) {
            o.push_str(&format!("### {}\n\n", t.what));
            o.push_str(&format!(
                "- **Status:** {st} -- {}\n",
                status_meaning(st).unwrap_or("")
            ));
            o.push_str(&format!("- **Licence:** {}\n", t.licence));
            o.push_str(&format!("- **Notice:** {}\n", notice_md(t.notice)));
            o.push_str(&format!("- **From:** {}\n", t.upstream));
            let files: Vec<String> = t.paths.iter().map(|p| format!("[{p}]({p})")).collect();
            o.push_str(&format!("- **Files:** {}\n\n", files.join(", ")));
        }
    }

    o.push_str("## docs/research/sources/\n\n");
    o.push_str(
        "Copies of other people's documents and code that baud's research cites, pinned by sha256 in\n\
         [MANIFEST.toml](docs/research/sources/MANIFEST.toml). Each group gives the licence, where its\n\
         notice is, and the URL each file was copied from.\n\n",
    );
    let mut groups: Vec<&Group> = side.upstream.iter().collect();
    groups.sort_by_key(|g| g.name.to_lowercase());
    for st in ORDER {
        for g in groups.iter().filter(|g| g.status == st) {
            o.push_str(&format!("### {}\n\n", g.name));
            o.push_str(&format!(
                "- **Status:** {st} -- {}\n",
                status_meaning(st).unwrap_or("")
            ));
            let uri = licence_uri(g.licence).map_or(String::new(), |u| format!(" ({u})"));
            o.push_str(&format!("- **Licence:** {}{uri}\n", g.licence));
            o.push_str(&format!("- **Notice:** {}\n", notice_md(g.notice)));
            o.push_str(&format!("- **Files:** {}\n\n", g.files.len()));
            o.push_str("| file | copied from |\n|---|---|\n");
            let mut files = g.files.clone();
            files.sort();
            for f in files {
                let u = url.get(f).ok_or(format!("KeyError: {}", py::repr_str(f)))?;
                o.push_str(&format!("| [{f}]({SRC}/{f}) | {u} |\n"));
            }
            o.push('\n');
        }
    }
    Ok(o)
}

/// The comment under the title, naming what writes the file.
const HEADER: &str = "<!-- Generated by `cargo xtask third-party` from docs/research/third-party.toml\n\
and docs/research/sources/MANIFEST.toml. Do not edit: change the sidecar and\n\
regenerate. `cargo xtask sources` fails when this file is stale. -->\n\n";

/// What the command does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Write THIRD_PARTY.md.
    Write,
    /// Fail if anything is wrong, THIRD_PARTY.md included.
    Check,
}

/// Whether the command failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Passed,
    Failed,
}

/// `cargo xtask third-party [--check]`, run against `root`.
pub fn run(root: &Path, mode: Mode, out: &mut impl Write) -> io::Result<Outcome> {
    for need in [SIDECAR, MANIFEST, "LICENSES"] {
        if !exists(root, need) {
            writeln!(
                out,
                "  FAIL {need} is missing, so no third-party file can be checked"
            )?;
            return Ok(Outcome::Failed);
        }
    }
    let stop = |out: &mut dyn Write, e: String| -> io::Result<Outcome> {
        writeln!(out, "  FAIL {e}")?;
        Ok(Outcome::Failed)
    };
    let side_t = match load(root, SIDECAR) {
        Ok(t) => t,
        Err(e) => return stop(out, e),
    };
    let man_t = match load(root, MANIFEST) {
        Ok(t) => t,
        Err(e) => return stop(out, e),
    };
    let man = match table(&man_t, "sources") {
        Ok(m) => m,
        Err(e) => return stop(out, format!("{MANIFEST}: {e}")),
    };
    let side = match Side::of(&side_t) {
        Ok(s) => s,
        Err(e) => return stop(out, format!("{SIDECAR}: {e}")),
    };
    let url = urls(man);
    let mut errs = check(root, &side, &url)?;
    let rendered = render(&side, &url);
    match mode {
        Mode::Check => {
            let cur = text::read_universal(&root.join(OUT)).ok();
            let stale = match (&cur, &rendered) {
                (None, _) => Some("missing"),
                (Some(c), Ok(r)) if c != r => Some("stale"),
                _ => None,
            };
            if let Some(how) = stale {
                errs.push(format!("{OUT} is {how}: run {GENERATOR}"));
            }
            for e in &errs {
                writeln!(out, "  FAIL {e}")?;
            }
            if !errs.is_empty() {
                return Ok(Outcome::Failed);
            }
            let total: usize = side.upstream.iter().map(|g| g.files.len()).sum();
            writeln!(
                out,
                "  ok   {total} sources and {} other entries indexed, {} licence texts, {} fonts",
                side.tree.len(),
                side.text.len(),
                rowed(root, &side, &no_python::tracked(root)?).1.0
            )?;
            Ok(Outcome::Passed)
        }
        Mode::Write => {
            if !errs.is_empty() {
                for e in &errs {
                    writeln!(out, "  FAIL {e}")?;
                }
                return Ok(Outcome::Failed);
            }
            match rendered {
                Ok(r) => {
                    fs::write(root.join(OUT), r)?;
                    Ok(Outcome::Passed)
                }
                Err(e) => stop(out, e),
            }
        }
    }
}

/// The command the stale line tells a reader to run.
pub const GENERATOR: &str = "cargo xtask third-party";
