//! `cargo xtask sources`: the source-provenance gate, after zebu's
//! tools/scripts/check-sources.sh.
//!
//! Fails if the research corpus and its manifest have drifted apart:
//!
//! 1. a manifest entry marked `retrieved` whose file is missing;
//! 2. a file whose contents no longer hash to the recorded sha256, or an
//!    entry whose sha256 is not 64 lowercase hex;
//! 3. a file in docs/research/sources/ with no manifest entry;
//! 4. a note citing a key the manifest does not carry, or carries as
//!    `gated`;
//! 5. a committed source with no licence row (`cargo xtask third-party
//!    --check`);
//! 6. a learn.microsoft.com page with no article-text pin: its bytes move
//!    with the site's chrome, so its check is the text (`crate::page_text`);
//! 7. a `derived_from` naming a key the manifest does not carry, carries as
//!    `gated`, or the entry itself: a file transcribed from a source is
//!    pinned with its own sha256 and names the source it was read from;
//! 8. docs/citations.md not what `cargo xtask citations` writes, or a row
//!    of docs/data/citations-code.tsv naming a key the manifest does not
//!    carry or carries as `gated`, or a path that is not in the tree.
//!
//! (4) is the one that matters: it stops a design document resting on a
//! citation nobody could check. Notes cite with `[[source: <key>]]`.
//!
//! **No list of what to scan, because every list here has gone stale.**
//! This gate has carried two, and both were narrower than the sentence
//! above them. `DOCS="docs"` sat under a comment reading "Every doc ... a
//! gate that only scanned one directory would have let those citations rot
//! unchecked" -- and left **the root guide's 39 citations** out, along with
//! README.md's and MEASUREMENTS.md's, because those are at the repository
//! root rather than under docs/. `CODE` named nine crates and omitted the
//! three ISA ones, where `baud-isa-aarch64` cites `arm-neon-bitmask` in the
//! doc comment this project's NEON work was reasoned from. So: every
//! markdown file and every Rust file in the working tree, minus the build
//! and VCS directories. A new crate, a new document at any depth, and a
//! crate the workspace excludes are all covered without anyone remembering
//! to add them.
//!
//! **Two readers of one file, and they must agree.** The line reader below
//! reads four fields by line prefix and never parses the file, so a title
//! with unescaped quotes -- `"VTE #2919, discussion -- "resizing-rewrapping
//! is an atomic step""`, which note U's commit added -- left the manifest
//! unreadable by any TOML parser while every check passed. So the file is
//! parsed too, and the parser's `[sources.*]` keys must be exactly the
//! line reader's: a header the line reader misreads, or one only a parser
//! sees, is the same drift between two readers of one file. The parse is
//! held to TOML 1.0 (`crate::toml10`), which is what the file was written
//! for.
//!
//! The line reader is the shell's awk, rule for rule, and the field
//! splitting each check does is the shell's: `read`'s for (1) and (2),
//! where a run of tabs is one separator and an empty field moves the rest
//! left, so a retrieved entry with no file or no sha256 reads a status
//! that is not `retrieved` and only the well-formedness check sees it; and
//! `cut`'s for that check, which keeps empty fields. The first version of
//! that check used `read`, read `status` as the digest and reported
//! apple-cpu-guide's perfectly good 64 hex chars as "8 chars, want 64" --
//! a probe firing on the wrong data, which is the failure mode this repo
//! already records twice.
//!
//! The lists sort in byte order, where the shell's `sort -u` sorted keys
//! and citations in the locale's collation; only the order of the failure
//! lines can differ.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use regex::bytes::Regex;
use toml::{Table, Value};

use crate::html::Count;
use crate::third_party::{self, Mode, Outcome};
use crate::{citations, page_text, py, sha256, toml10};

pub const SRC: &str = "docs/research/sources";
pub const MANIFEST: &str = "docs/research/sources/MANIFEST.toml";

/// The citation floor. **A gate that scanned nothing read as a gate that
/// passed.** The count was printed and never asserted, so a broken `find`,
/// a renamed directory or a changed citation syntax would have reported
/// "ok 0 citations resolve" and exited 0 -- the failure this repository
/// records under `inf x amplification`, the stale `scan path: AVX2` flag
/// and `fast_hit`, in the gate written to stop design documents resting on
/// unreadable sources. The floor is well under the real count and exists
/// to catch a scan that found nothing, not to pin a number that grows with
/// every note.
pub const CITATION_FLOOR: Count = Count(50);

/// What the citation scan leaves out, from the repository's root.
/// `.claude/worktrees` holds other branches' checkouts, each with its own
/// manifest: their citations are theirs, and a lane's new key reads as
/// missing here.
pub const PRUNE: [&str; 5] = [
    "target",
    ".git",
    ".devenv",
    "node_modules",
    ".claude/worktrees",
];

static CITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[source: ([a-z0-9_-]+)\]\]").expect("the citation pattern"));

/// One `[sources.*]` block as the line reader reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub file: String,
    pub sha: String,
    pub status: String,
}

impl Entry {
    /// `key<TAB>file<TAB>sha256<TAB>status`, the line the shell's awk
    /// printed.
    fn line(&self) -> String {
        format!("{}\t{}\t{}\t{}", self.key, self.file, self.sha, self.status)
    }
}

/// `sub(/^name *= *"/, "", v); sub(/"$/, "", v)` on a line that matched
/// `/^name *=/`.
fn field(line: &str, name: &str) -> Option<String> {
    let rest = line.strip_prefix(name)?;
    let after = rest.trim_start_matches(' ');
    let value = after.strip_prefix('=')?;
    let v = match value.trim_start_matches(' ').strip_prefix('"') {
        Some(v) => v,
        None => line,
    };
    Some(v.strip_suffix('"').unwrap_or(v).to_string())
}

/// The manifest read as the shell's awk read it: by line prefix, one
/// entry per `[sources.*]` header.
pub fn line_reader(text: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut cur = Entry::default();
    for line in text.split_terminator('\n') {
        if let Some(rest) = line.strip_prefix("[sources.") {
            if !cur.key.is_empty() {
                out.push(cur.clone());
            }
            cur = Entry {
                key: rest.strip_suffix(']').unwrap_or(rest).to_string(),
                ..Entry::default()
            };
        } else if let Some(v) = field(line, "file") {
            cur.file = v;
        } else if let Some(v) = field(line, "sha256") {
            cur.sha = v;
        } else if let Some(v) = field(line, "status") {
            cur.status = v;
        }
    }
    if !cur.key.is_empty() {
        out.push(cur);
    }
    out
}

/// `diff old new` in its normal format, for two sorted lists: the longest
/// common subsequence of two sorted lists is the elements they share, so
/// the hunks are the runs between those.
pub fn diff_sorted(old: &[String], new: &[String]) -> Vec<String> {
    let range = |a: usize, b: usize| {
        if a == b {
            a.to_string()
        } else {
            format!("{a},{b}")
        }
    };
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    loop {
        let (i0, j0) = (i, j);
        while i < old.len() || j < new.len() {
            match (old.get(i), new.get(j)) {
                (Some(a), Some(b)) if a == b => break,
                (Some(a), Some(b)) if a < b => i += 1,
                (Some(_), Some(_)) => j += 1,
                (Some(_), None) => i += 1,
                (None, _) => j += 1,
            }
        }
        match (i > i0, j > j0) {
            (true, true) => {
                out.push(format!("{}c{}", range(i0 + 1, i), range(j0 + 1, j)));
                out.extend(old[i0..i].iter().map(|l| format!("< {l}")));
                out.push("---".to_string());
                out.extend(new[j0..j].iter().map(|l| format!("> {l}")));
            }
            (true, false) => {
                out.push(format!("{}d{}", range(i0 + 1, i), j0));
                out.extend(old[i0..i].iter().map(|l| format!("< {l}")));
            }
            (false, true) => {
                out.push(format!("{}a{}", i0, range(j0 + 1, j)));
                out.extend(new[j0..j].iter().map(|l| format!("> {l}")));
            }
            (false, false) => {}
        }
        if i >= old.len() && j >= new.len() {
            return out;
        }
        i += 1;
        j += 1;
    }
}

/// Every regular file under `dir`, as `find <dir> -type f` lists them:
/// directories in the order they list, entered as they are met, symbolic
/// links neither listed nor followed.
fn find_files(root: &Path, dir: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(t) = e.file_type() else { continue };
        let path = format!("{dir}/{}", e.file_name().to_string_lossy());
        if t.is_file() {
            out.push(path);
        } else if t.is_dir() {
            find_files(root, &path, out);
        }
    }
}

/// Every `[[source: <key>]]` in the tree's `*.md` and `*.rs` files.
pub fn citations(root: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    cite_dir(root, "", &mut found);
    found
}

fn cite_dir(root: &Path, rel: &str, found: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(root.join(rel)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        if PRUNE.contains(&path.as_str()) {
            continue;
        }
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            cite_dir(root, &path, found);
        } else if t.is_file() && (name.ends_with(".md") || name.ends_with(".rs")) {
            let Ok(bytes) = fs::read(e.path()) else {
                continue;
            };
            for c in CITE.captures_iter(&bytes) {
                found.insert(String::from_utf8_lossy(&c[1]).into_owned());
            }
        }
    }
}

/// The parsed manifest's `[sources.*]` table, or why it could not be read.
pub fn parse(text: &str) -> Result<Table, String> {
    if let Some(r) = toml10::first_refusal(text) {
        return Err(r.to_string());
    }
    let mut table: Table = text
        .parse()
        .map_err(|e: toml::de::Error| toml10::described(text, &e))?;
    match table.remove("sources") {
        Some(Value::Table(t)) => Ok(t),
        _ => Err("KeyError: 'sources'".to_string()),
    }
}

/// Check 7: every `derived_from` names another entry the manifest holds
/// and has read.
pub fn derived_problems(sources: &Table) -> Vec<String> {
    let mut out = Vec::new();
    for (key, e) in sources {
        let Some(d) = e.as_table().and_then(|t| t.get("derived_from")) else {
            continue;
        };
        let parents: Option<Vec<&str>> = match d {
            Value::String(s) => Some(vec![s.as_str()]),
            Value::Array(a) if !a.is_empty() => a.iter().map(Value::as_str).collect(),
            _ => None,
        };
        let Some(parents) = parents else {
            out.push(format!(
                "{key}: derived_from is {}, want a key or a list of keys",
                py::repr(Some(d))
            ));
            continue;
        };
        for p in parents {
            let parent = sources.get(p).and_then(Value::as_table);
            if p == key {
                out.push(format!("{key}: derived_from names the entry itself"));
            } else if let Some(parent) = parent {
                if parent.get("status").and_then(Value::as_str) == Some("gated") {
                    out.push(format!(
                        "{key}: derived_from names {p}, recorded as gated \u{2014} never read"
                    ));
                }
            } else {
                out.push(format!(
                    "{key}: derived_from names {p}, which the manifest does not carry"
                ));
            }
        }
    }
    out
}

/// `IFS=$'\t' read -r key file sha status`.
fn read_fields(line: &str) -> (&str, &str, &str, &str) {
    let mut rest = line.trim_matches('\t');
    let mut fields = [""; 3];
    for f in &mut fields {
        let (head, tail) = rest.split_once('\t').unwrap_or((rest, ""));
        *f = head;
        rest = tail.trim_start_matches('\t');
    }
    (fields[0], fields[1], fields[2], rest)
}

/// A field of an entry's line, as `cut -f` numbers them from 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Key,
    File,
    Sha,
    Status,
}

/// `cut -f<n>`: a line with no tab is every field of itself.
fn cut(line: &str, f: Field) -> &str {
    let cols: Vec<&str> = line.split('\t').collect();
    if cols.len() == 1 {
        return line;
    }
    let n = match f {
        Field::Key => 0,
        Field::File => 1,
        Field::Sha => 2,
        Field::Status => 3,
    };
    cols.get(n).copied().unwrap_or("")
}

/// `cargo xtask sources` against `root`.
pub fn run(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    let mut fail = false;
    let say = |out: &mut dyn Write, s: String| writeln!(out, "{s}");
    let Ok(raw) = fs::read(root.join(MANIFEST)) else {
        say(out, format!("  FAIL {MANIFEST} is missing"))?;
        return Ok(Outcome::Failed);
    };
    let text = String::from_utf8_lossy(&raw).into_owned();
    let entries = line_reader(&text);

    // The manifest is TOML, and both readers see the same keys.
    let parsed = match std::str::from_utf8(&raw) {
        Ok(t) => parse(t),
        Err(e) => Err(e.to_string()),
    };
    let sources = match parsed {
        Ok(s) => s,
        Err(e) => {
            say(out, format!("  FAIL {MANIFEST} is not valid TOML: {e}"))?;
            return Ok(Outcome::Failed);
        }
    };
    let mut line_keys: Vec<String> = entries.iter().map(|e| e.key.clone()).collect();
    line_keys.sort();
    let mut toml_keys: Vec<String> = sources.keys().cloned().collect();
    toml_keys.sort();
    if line_keys != toml_keys {
        say(
            out,
            "  FAIL the awk and a TOML parser read different [sources.*] keys:".to_string(),
        )?;
        for l in diff_sorted(&line_keys, &toml_keys).iter().take(5) {
            say(out, format!("       {l}"))?;
        }
        fail = true;
    }

    // A learn.microsoft.com page is pinned by its article text. Closed:
    // only the line that says every page passed lets this one pass.
    let problems = page_text::offline_problems(&sources);
    for p in &problems {
        say(out, format!("  FAIL {p}"))?;
    }
    if problems.is_empty() {
        say(
            out,
            format!(
                "  ok   {} learn.microsoft.com pages carry their article text's sha256",
                page_text::pages(&sources).0
            ),
        )?;
    } else {
        fail = true;
    }

    // A derived file names the source it was read from.
    for p in derived_problems(&sources) {
        say(out, format!("  FAIL {p}"))?;
        fail = true;
    }

    let lines: Vec<String> = entries.iter().map(Entry::line).collect();

    // 1 & 2: retrieved entries must exist and still hash.
    for line in &lines {
        let (key, file, sha, status) = read_fields(line);
        if status != "retrieved" {
            continue;
        }
        let path = format!("{SRC}/{file}");
        if !root.join(&path).is_file() {
            say(
                out,
                format!("  FAIL {key}: status=retrieved but {path} is missing"),
            )?;
            fail = true;
            continue;
        }
        let got = sha256::file(&root.join(&path)).unwrap_or_else(|e| {
            eprintln!(
                "check-sources: {path}: {}",
                page_text::os_error(&e, Path::new(&path))
            );
            String::new()
        });
        if got != sha {
            say(
                out,
                format!("  FAIL {key}: {file} hashes {got}, manifest says {sha}"),
            )?;
            fail = true;
        }
    }

    // 2b: every sha256 is well-formed. An `external` entry names a file
    // this repo does not hold, so (2) cannot re-hash it and a malformed
    // digest there is invisible. This check caught a literal placeholder
    // -- `sha256 = "9c07f2a0a0a0a0a0"` -- in the commit that added it, on
    // an entry a note already cited. It is the cheap half of a
    // verification that cannot be done in full.
    for line in &lines {
        let (key, sha, status) = (
            cut(line, Field::Key),
            cut(line, Field::Sha),
            cut(line, Field::Status),
        );
        // `gated` means never obtained, so there is nothing to have hashed.
        if key.is_empty() || status == "gated" {
            continue;
        }
        // The shell's `[0-9a-f]*`: the first character only.
        if sha.starts_with(|c: char| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
            let n = sha.chars().count();
            if n != 64 {
                say(out, format!("  FAIL {key}: sha256 is {n} chars, want 64"))?;
                fail = true;
            }
        } else {
            say(
                out,
                format!("  FAIL {key}: sha256 is not lowercase hex: '{sha}'"),
            )?;
            fail = true;
        }
    }

    // 3: every source file is accounted for. Recursive: sources are
    // grouped into subdirectories, so a top-level-only walk would see a
    // directory it could not account for and never check the files inside
    // it.
    let known: BTreeSet<&str> = lines
        .iter()
        .map(|l| cut(l, Field::File))
        .filter(|f| !f.is_empty())
        .collect();
    let mut found = Vec::new();
    find_files(root, SRC, &mut found);
    for f in &found {
        if f.rsplit('/').next() == Some("MANIFEST.toml") {
            continue;
        }
        let f = f.trim_matches([' ', '\t']);
        let b = f.strip_prefix(&format!("{SRC}/")).unwrap_or(f);
        if !known.contains(b) {
            say(
                out,
                format!("  FAIL {b} is in {SRC}/ with no manifest entry"),
            )?;
            fail = true;
        }
    }

    // 4: every citation resolves to a readable source.
    let cited = citations(root);
    let keys: BTreeSet<&str> = lines.iter().map(|l| cut(l, Field::Key)).collect();
    let gated: BTreeSet<&str> = lines
        .iter()
        .filter(|l| l.split('\t').nth(3) == Some("gated"))
        .map(|l| l.split('\t').next().unwrap_or(""))
        .collect();
    if cited.len() < CITATION_FLOOR.0 {
        say(
            out,
            format!(
                "  FAIL only {} citations found; this gate scans the whole tree and should see far more",
                cited.len()
            ),
        )?;
        return Ok(Outcome::Failed);
    }
    for c in &cited {
        if !keys.contains(c.as_str()) {
            say(
                out,
                format!(
                    "  FAIL a doc or doc comment cites [[source: {c}]], which the manifest does not carry"
                ),
            )?;
            fail = true;
        }
        if gated.contains(c.as_str()) {
            say(
                out,
                format!(
                    "  FAIL a doc or doc comment cites [[source: {c}]], recorded as gated \u{2014} never read"
                ),
            )?;
            fail = true;
        }
    }
    say(
        out,
        format!(
            "  ok   {} citations resolve, {} sources in manifest",
            cited.len(),
            keys.len()
        ),
    )?;

    // 5: every committed source has a licence, and the index says so.
    // Committing a file is distributing it, so a source in the manifest is
    // also a licence obligation.
    if third_party::run(root, Mode::Check, out)? == Outcome::Failed {
        fail = true;
    }

    // 8: the citations list is what the manifest and the citations give.
    if citations::run(root, citations::Mode::Check, out)? == Outcome::Failed {
        fail = true;
    }
    Ok(if fail {
        Outcome::Failed
    } else {
        Outcome::Passed
    })
}
