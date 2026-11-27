//! `cargo xtask paths`: every `path` a manifest names stays inside the
//! repository.
//!
//! A `path` dependency outside the repository builds on the machine it was
//! written on and nowhere else, so there is no exception. This reads every
//! `Cargo.toml` below the root -- not under `./target/` or `./.git/`, and not
//! through a symbolic link to a directory -- and fails on each
//! `path = "..."` value, on any line of the file, that:
//!
//! - is relative and does not name a directory, resolved from the
//!   manifest's own directory with symbolic links followed; or
//! - resolves, or is written absolute, outside the repository's root.
//!
//! A value ending in `.rs` names a target's source (`[lib]`, `[[bin]]`,
//! `[[test]]`, `[[bench]]`) rather than a dependency and is not read.
//!
//! **A positive control**: a walk that read no manifest would print nothing
//! and pass, so fewer than [`FLOOR`] manifests fails as well.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::third_party::Outcome;

/// The fewest manifests a walk of this repository can find.
pub const FLOOR: usize = 10;

/// A `path` key and its quoted value, as one match on one line.
static PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"path *= *"([^"]+)""#).expect("the path pattern"));

/// Where one `path` value leads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// The repository's root or a directory below it.
    Inside,
    /// A relative value that names no directory on this machine.
    Nowhere,
    /// A directory outside the repository.
    Outside,
}

/// Every file named `Cargo.toml` below `root`, spelled `./<path>`, in byte
/// order: `./target/` and `./.git/` are not read, and a symbolic link to a
/// directory is listed as itself and not entered.
pub fn manifests(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    descend(root, ".", &mut out);
    out.sort();
    out
}

fn descend(root: &Path, dir: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root.join(dir)) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = format!("{dir}/{name}");
        if path == "./target" || path == "./.git" {
            continue;
        }
        if name == "Cargo.toml" {
            out.push(path.clone());
        }
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            descend(root, &path, out);
        }
    }
}

/// Every `path` value in `text`, in order, each match on each line.
pub fn values(text: &str) -> Vec<String> {
    text.lines()
        .flat_map(|line| PATH.captures_iter(line))
        .map(|c| c[1].to_string())
        .filter(|v| !v.ends_with(".rs"))
        .collect()
}

/// Where `value`, named by the manifest at `manifest` below `root`, leads,
/// against `top`, the root with its symbolic links resolved.
pub fn verdict(root: &Path, top: &Path, manifest: &str, value: &str) -> Verdict {
    let named = Path::new(value);
    let resolved = if named.is_absolute() {
        named.to_path_buf()
    } else {
        let base = root.join(manifest);
        let dir = base.parent().unwrap_or(root);
        match fs::canonicalize(dir.join(named)) {
            Ok(p) if p.is_dir() => p,
            _ => return Verdict::Nowhere,
        }
    };
    if resolved.starts_with(top) {
        Verdict::Inside
    } else {
        Verdict::Outside
    }
}

/// `cargo xtask paths`, run against `root`.
pub fn run(root: &Path, out: &mut impl Write) -> io::Result<Outcome> {
    let top = fs::canonicalize(root)?;
    let found = manifests(root);
    let mut outcome = Outcome::Passed;
    for m in &found {
        let text = fs::read(root.join(m))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default();
        for v in values(&text) {
            match verdict(root, &top, m, &v) {
                Verdict::Inside => {}
                Verdict::Nowhere => {
                    writeln!(
                        out,
                        "FAIL {m}: path \"{v}\" resolves to nothing on this machine"
                    )?;
                    outcome = Outcome::Failed;
                }
                Verdict::Outside => {
                    writeln!(out, "FAIL {m}: \"{v}\" leaves the repository")?;
                    writeln!(
                        out,
                        "    That is a build that works on one machine. Vendor it, take it"
                    )?;
                    writeln!(out, "    from a registry, or make it a git dependency.")?;
                    outcome = Outcome::Failed;
                }
            }
        }
    }
    let n = found.len();
    if n < FLOOR {
        writeln!(
            out,
            "FAIL: only {n} manifests found -- this gate scanned almost nothing"
        )?;
        return Ok(Outcome::Failed);
    }
    if outcome == Outcome::Failed {
        writeln!(out, "FAIL: a path dependency leaves the repository")?;
        return Ok(Outcome::Failed);
    }
    writeln!(
        out,
        "ok: {n} manifests, every path dependency inside the repository"
    )?;
    Ok(Outcome::Passed)
}
