//! `cargo xtask publish`: what crates.io would receive, checked before
//! anything is sent.
//!
//! ```text
//! cargo xtask publish                  the manifests and the file lists
//! cargo xtask publish --size           and each .crate's size, from
//!                                      `cargo package --no-verify`
//! cargo xtask publish --verify         and each crate built from its own
//!                                      .crate, against the others'
//! cargo xtask publish --release        and no published crate ships a file
//!                                      whose licence is not settled
//! cargo xtask publish --write-notices  write each crate's NOTICE, and check
//!                                      the rest
//! cargo xtask publish --write-licences write each crate's `license` field,
//!                                      and check the rest
//! ```
//!
//! Exit 0 when everything passes, 1 on any FAIL, and 2 under `--release`
//! when the only thing left is a row whose licence is not settled. Other
//! arguments are ignored, as the script this replaced ignored them.
//!
//! `cargo install baud` builds the `baud` package and every crate it
//! depends on, so each of those is published and nothing else is. The
//! checks, each of which has to be able to fail:
//!
//! 1. the published set is `baud` and the workspace crates it reaches
//!    through its dependencies and build-dependencies, target-specific and
//!    optional ones included; every other member is in [`UNPUBLISHED`] and
//!    carries `publish = false`;
//! 2. each published manifest has a description, a readme that exists, the
//!    repository, a rust-version, and keywords and categories crates.io
//!    accepts;
//! 3. each published crate's `license` is what [`licence_expr`] gives from
//!    its own file list: CC-BY-NC-SA-4.0, baud's, AND the licence of every
//!    `docs/research/third-party.toml` entry with a file in the crate, an
//!    SPDX expression whose every term is one of [`SPDX_IDS`] or a
//!    LicenseRef- (`--write-licences` writes it); an entry whose `licence`
//!    is not such an expression says what it adds in `spdx`, "" when it
//!    adds nothing, or the check fails;
//! 4. each dependency of a published crate on a workspace crate names the
//!    version being released as well as its path: cargo refuses to package
//!    one that does not, and drops the path from what it uploads;
//! 5. each published crate's file list carries LICENSE, byte for byte the
//!    root's, and the notice every third-party file in it needs: for each
//!    sidecar entry with a file in the list, a notice in LICENSES/ must be
//!    in the crate's own LICENSES/, byte for byte the root's, with each text
//!    its [text] entry `needs`, and a notice that is another file must be in
//!    the list;
//! 6. no published crate ships a file of a sidecar entry whose status is one
//!    of [`UNSETTLED`]: without `--release` each is printed and the check
//!    still passes, since what is missing is a decision, not a change to
//!    the tree; with `--release` (run before `release-plz release`, before
//!    anything is uploaded) each one refuses the release. A file with no
//!    entry at all is not seen here;
//! 7. each published crate carries NOTICE, and it is what [`notice`] writes
//!    from the crate's own file list (`--write-notices` writes them);
//! 8. what a NOTICE prints of a settled entry is its `public` and `holder`
//!    and the URLs it came from, and none of it names a research note, an
//!    ISO date, who decided, a hyphenated source key of MANIFEST.toml or
//!    any key written in the repository's citation form, or a repository
//!    path the crate does not carry ([`wording`]); an entry whose status
//!    is one of [`UNSETTLED`] is reported on the awaiting path instead;
//! 9. `baud`, the package `cargo install` builds as its own root, carries
//!    the workspace's `[profile.release]` and cargo-features: cargo reads a
//!    profile only from the root manifest, so without the copy an install
//!    builds without LTO, in 16 codegen units and without the function
//!    alignment; with `--size` or `--verify`, the manifest inside baud's
//!    .crate is held to the same;
//! 10. with `--size` or `--verify`, every .crate is under crates.io's 10 MiB
//!     upload limit;
//! 11. with `--verify`, every crate builds from its .crate alone, as `cargo
//!     publish` builds it before uploading;
//! 12. with `--size` or `--verify`, each run packages and builds in a folder
//!     of its own ([`package`]), and a crate the build took from cargo's
//!     unpack of the other .crates must have been unpacked by this run;
//! 13. a packaged symlink is a copy of the file it points at (cargo archives
//!     the target's bytes), so it counts as that file for the notices, and
//!     it must point at a file in the repository.
//!
//! The lines it prints are the lines the Python gate it replaced printed,
//! but for the command its hints name.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use toml::{Table, Value};

use crate::html::Count;
use crate::textwrap::Wrap;
use crate::third_party::{self, STATUS};
use crate::{py, tarball, text};

pub const SIDECAR: &str = "docs/research/third-party.toml";
pub const LICENCE: &str = "CC-BY-NC-SA-4.0";
/// What crates.io takes in `license`, and so what [`licence_expr`] may
/// write. Cargo's reference ("The license and license-file fields",
/// [[source: cargo-1ef25ea-manifest-license]]) says crates.io reads the
/// field as an SPDX 2.3 licence expression, AND and OR joining licences,
/// that the name must be on the SPDX License List 3.20 (a version the
/// parser has since passed: Unicode-3.0 postdates it, so the parser's
/// 3.29.0 governs), and that a nonstandard licence may name a file in
/// `license-file` instead. crates.io's parser
/// ([[source: crates-io-b676909-licenses-rs]]) parses it with spdx =0.13.6
/// ([[source: crates-io-61edf7b-cargo-toml]]), which carries SPDX License
/// List 3.29.0, takes an identifier on that list or a LicenseRef- term,
/// and refuses any other term (`allow_unknown: false`;
/// [[source: spdx-0-13-6-crate]] lexer.rs); a `license-file` with no
/// `license` is recorded as "non-standard"
/// ([[source: crates-io-c7d5e23-publish-rs]]), which would hide baud's own
/// licence. So each crate sets `license`, and every term in it is one of
/// these identifiers, each on the SPDX License List 3.29.0
/// ([[source: spdx-license-list-3-29-0-json]]), or a LicenseRef-. An
/// identifier a row comes to use is added here once it is checked against
/// that list.
pub const SPDX_IDS: [&str; 15] = [
    "Apache-2.0",
    "CC-BY-4.0",
    "CC-BY-NC-SA-4.0",
    "CC-BY-ND-4.0",
    "HPND",
    "HPND-sell-variant",
    "IJG",
    "MIT",
    "MIT-Modern-Variant",
    "MIT-open-group",
    "OFL-1.1",
    "Unicode-3.0",
    "Unlicense",
    "X11",
    "Zlib",
];
/// The workspace members that are not published.
pub const UNPUBLISHED: [&str; 4] = ["baud-bench", "baud-difftest", "baud-sfte-difftest", "xtask"];
/// Statuses that say the licence of a file is not settled. A file under
/// one of these may sit in the repository, and must not go out in a
/// published crate: a crates.io version cannot be taken back. `copyleft`
/// is one, since keeping such a file or making it external is a choice
/// still open; `nc` is not, since CC BY-NC-SA 3.0 allows the verbatim copy
/// it is kept as.
pub const UNSETTLED: [&str; 4] = ["open", "unverified", "no-grant", "copyleft"];
/// crates.io's limit on an upload, in bytes.
pub const LIMIT: Bytes = Bytes(10 * 1024 * 1024);
/// The crates.io category slugs these manifests use, as
/// https://crates.io/api/v1/category_slugs listed them on 2026-10-28. A
/// slug outside the list is refused at upload.
pub const CATEGORIES: [&str; 12] = [
    "command-line-utilities",
    "encoding",
    "graphics",
    "gui",
    "hardware-support",
    "no-std",
    "no-std::no-alloc",
    "os::linux-apis",
    "os::macos-apis",
    "parser-implementations",
    "rendering",
    "text-processing",
];
pub const SOURCES: &str = "docs/research/sources/";
/// Fewer crates than this reached from `baud` means the walk stopped.
pub const CRATES_FLOOR: Count = Count(10);
/// Fewer hyphenated MANIFEST.toml keys than this means the key check
/// stopped working.
pub const KEYS_FLOOR: Count = Count(100);
const DEP_TABLES: [&str; 2] = ["dependencies", "build-dependencies"];
/// The command the failure lines tell a reader to run.
pub const COMMAND: &str = "cargo xtask publish";

/// A size in bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Bytes(pub u64);

// Python's `$` in `re.match` also matches before a final newline, and its
// `\s` on text is `str.isspace()`, which holds for `\x1c`-`\x1f` where the
// `regex` crate's does not: both are spelled out.
static KEYWORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_+-]{0,19}\n?\z").expect("KEYWORD"));
static LICENCE_REF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^LicenseRef-[A-Za-z0-9.-]+\n?\z").expect("LICENCE_REF"));
static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^https?://[^\s\x1c-\x1f]+\n?\z").expect("URL"));
static URL_IN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"https?://[^\s\x1c-\x1f]+").expect("URL_IN"));
static TOKENS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\(|\)|[^\s\x1c-\x1f()]+").expect("TOKENS"));
static ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9]").expect("ALNUM"));
static LICENSE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^license(\.workspace)?[\s\x1c-\x1f]*=").expect("LICENSE_LINE"));
/// What a NOTICE must not say. It goes to crates.io, where a version stays
/// as it was uploaded, to a reader who has only the crate: no research
/// note, no ISO date a decision was taken, no word about who decided what,
/// no hyphenated source key and no key in the repository's citation form. The
/// rows' `what`, which THIRD_PARTY.md shows in the repository, may say all
/// of these; a NOTICE prints a row's `public` instead.
static PROCESS: LazyLock<[Regex; 4]> = LazyLock::new(|| {
    [
        Regex::new(r"\b[Nn]ote [A-Z]{1,2}\b"),
        Regex::new(r"\b\d{4}-\d{2}-\d{2}\b"),
        // "lane" in lower case only: Thomas G. Lane is the IJG's holder.
        Regex::new(
            r"\b(?:[Dd]ecided|[Dd]ecision|[Rr]ecommendation|[Oo]wner|lane|[Rr]efuter|[Mm]anifest|MANIFEST|Jev|TODO)\b",
        ),
        // The repository's citation form, whatever key it names.
        Regex::new(r"\[\[source:"),
    ]
    .map(|r| r.expect("a PROCESS pattern"))
});

/// What the arguments ask for beside the checks of the tree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pack {
    /// No `cargo package`.
    No,
    /// `--size`: package without building.
    Size,
    /// `--verify`: package and build each crate from its .crate.
    Verify,
}

/// Whether to write a file before checking it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Writes {
    No,
    Yes,
}

/// Whether an unsettled row refuses the release.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Release {
    No,
    Yes,
}

/// The arguments, as the script read them: each flag by its presence,
/// anything else ignored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flags {
    pub pack: Pack,
    pub notices: Writes,
    pub licences: Writes,
    pub release: Release,
}

impl Flags {
    pub fn parse(args: &[&str]) -> Flags {
        let has = |f: &str| args.contains(&f);
        let yes = |f: &str| if has(f) { Writes::Yes } else { Writes::No };
        Flags {
            pack: if has("--verify") {
                Pack::Verify
            } else if has("--size") {
                Pack::Size
            } else {
                Pack::No
            },
            notices: yes("--write-notices"),
            licences: yes("--write-licences"),
            release: if has("--release") {
                Release::Yes
            } else {
                Release::No
            },
        }
    }
}

/// How the run ended, and the exit code it gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// 0.
    Passed,
    /// 1: a FAIL.
    Failed,
    /// 2: under `--release`, nothing failed and a row is not settled.
    Refused,
}

impl Exit {
    pub fn code(self) -> std::process::ExitCode {
        std::process::ExitCode::from(match self {
            Exit::Passed => 0,
            Exit::Failed => 1,
            Exit::Refused => 2,
        })
    }
}

/// Where cargo runs: its home, whose `registry/src` holds the unpacks, the
/// folder each packaging run makes its own folder in, and what else its
/// environment carries.
pub struct Cargo {
    pub home: PathBuf,
    pub target: PathBuf,
    pub env: Vec<(String, String)>,
}

impl Cargo {
    /// `$CARGO_HOME` or `~/.cargo`, and `$CARGO_TARGET_DIR` or `target`
    /// under `root`, as cargo itself takes them.
    pub fn from_env(root: &Path) -> Cargo {
        let var = |k: &str| env::var_os(k).filter(|v| !v.is_empty());
        let home = var("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(var("HOME").unwrap_or_default()).join(".cargo"));
        let target = root.join(var("CARGO_TARGET_DIR").unwrap_or_else(|| "target".into()));
        Cargo {
            home,
            target,
            env: Vec::new(),
        }
    }

    fn command(&self, root: &Path) -> Command {
        let mut c = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
        c.current_dir(root).env("CARGO_HOME", &self.home);
        for (k, v) in &self.env {
            c.env(k, v);
        }
        c
    }

    /// `cargo package --list` of one crate: its file list, or cargo's
    /// stderr.
    pub fn listed(&self, root: &Path, name: &str) -> Result<Vec<String>, String> {
        let out = self
            .command(root)
            .args(["package", "--list", "--allow-dirty", "-q", "-p", name])
            .output()
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(
                text::strip(&text::universal(&String::from_utf8_lossy(&out.stderr))).to_string(),
            );
        }
        let s = text::universal(&String::from_utf8_lossy(&out.stdout));
        Ok(text::splitlines(&s)
            .into_iter()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }
}

/// The folder a packaging run builds in.
pub enum Folder {
    /// A new folder under [`Cargo::target`], named for this run.
    Fresh,
    /// This one, made if it is not there: how a test runs twice in one
    /// place.
    At(PathBuf),
}

/// What [`package`] found.
pub struct Packed {
    pub errs: Vec<String>,
    /// Each crate's .crate size and the bytes of the Cargo.toml inside it.
    pub crates: BTreeMap<String, (Bytes, Vec<u8>)>,
    /// The unpacks, `<name>-<version>` directories under cargo's
    /// registry/src, that the build read.
    pub read: BTreeSet<PathBuf>,
}

/// `cargo package` of the crates `names` of the workspace at `root`, each
/// built from its own .crate under [`Pack::Verify`], as `cargo publish`
/// builds it.
///
/// Cargo puts the .crate of each crate it packages in a local registry,
/// `package/tmp-registry` under the build folder, and lays it over
/// crates.io for the crates after it
/// ([[source: cargo-eb94155-package-mod-rs]] lines 283-284 and
/// 1100-1102). A crate of one name and version is immutable to cargo in
/// two places, and each kept a crate's code from before a change that left
/// the version alone, so that the crates built on it failed, or passed, on
/// that old code:
///
/// - its unpack: the local registry's crates unpack into
///   `$CARGO_HOME/registry/src/-<hash>`, the hash of the registry's path as
///   a file URL, and an unpack whose .cargo-ok is written is used as it is
///   ([[source: cargo-eb94155-workspace-rs]] lines 2031-2035;
///   [[source: cargo-eb94155-source-id-rs]] lines 248-251 and 686-690;
///   [[source: cargo-eb94155-registry-mod-rs]] lines 459-469, 553 and
///   618-636);
/// - its compile: the verify builds in the build folder
///   ([[source: cargo-eb94155-package-verify-rs]] lines 72-77), where a
///   crate from the registry is fresh for as long as its version is
///   ([[source: cargo-eb94155-overlay-rs]] lines 117-119;
///   [[source: cargo-eb94155-fingerprint-mod-rs]] lines 336-338).
///
/// So each call builds in a folder made for it, set as the target folder
/// and as the build folder, which defaults to the target folder and may be
/// configured apart ([[source: cargo-eb94155-config-md]] lines 540-547): a
/// path no build used, so a registry URL and an unpack folder no build
/// used, and no compiled crate. A crate read from an unpack folder that
/// was there before the call is a FAIL. The folder is removed after, and
/// so are the unpacks of `names` in each unpack folder the call made,
/// found from the build's dep-info and errors, and that folder once it is
/// empty; nothing else under the cargo home is touched.
pub fn package(
    root: &Path,
    names: &[&str],
    version: &str,
    pack: Pack,
    cargo: &Cargo,
    folder: Folder,
) -> io::Result<Packed> {
    let mut p = Packed {
        errs: Vec::new(),
        crates: BTreeMap::new(),
        read: BTreeSet::new(),
    };
    let src = cargo.home.join("registry").join("src");
    fs::create_dir_all(&cargo.target)?;
    let fresh = match folder {
        Folder::Fresh => made_folder(&cargo.target)?,
        Folder::At(path) => {
            fs::create_dir_all(&path)?;
            path
        }
    };
    let alternatives: Vec<String> = names
        .iter()
        .map(|n| regex::escape(&format!("{n}-{version}")))
        .collect();
    let unpack = Regex::new(&format!(
        r"{}([^/\s\x1c-\x1f]+)/({})/",
        regex::escape(&format!("{}/", src.display())),
        alternatives.join("|")
    ))
    .map_err(io::Error::other)?;
    let mut args = vec!["package", "--allow-dirty", "-q"];
    if pack != Pack::Verify {
        args.push("--no-verify");
    }
    for n in names {
        args.extend(["-p", n]);
    }
    let before: HashSet<String> = match fs::read_dir(&src) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(_) => HashSet::new(),
    };
    let mut made: BTreeSet<String> = BTreeSet::new();
    let result = (|| -> io::Result<()> {
        let out = cargo
            .command(root)
            .args(&args)
            .env("CARGO_TARGET_DIR", &fresh)
            .env("CARGO_BUILD_BUILD_DIR", &fresh)
            .output()?;
        let stderr = text::universal(&String::from_utf8_lossy(&out.stderr));
        let mut said = vec![stderr.clone()];
        dep_info(&fresh, &mut said);
        let mut stale = BTreeSet::new();
        for m in unpack.captures_iter(&said.join("\n")) {
            p.read.insert(src.join(&m[1]).join(&m[2]));
            if before.contains(&m[1]) {
                stale.insert(m[1].to_string());
            } else {
                made.insert(m[1].to_string());
            }
        }
        for d in &stale {
            p.errs.push(format!(
                "cargo built a crate from {}, which was there before this run: \
                 what is unpacked there may be an older .crate's",
                src.join(d).display()
            ));
        }
        if !out.status.success() {
            let tail: Vec<char> = text::strip(&stderr).chars().collect();
            let from = tail.len().saturating_sub(2000);
            p.errs.push(format!(
                "cargo package failed: {}",
                tail[from..].iter().collect::<String>()
            ));
        } else {
            for n in names {
                let krate = fresh.join("package").join(format!("{n}-{version}.crate"));
                let gz = fs::read(&krate)?;
                let manifest = tarball::extract(&gz, &format!("{n}-{version}/Cargo.toml"))
                    .map_err(|e| io::Error::other(format!("{}: {e}", krate.display())))?;
                let size = Bytes(u64::try_from(gz.len()).map_err(io::Error::other)?);
                p.crates.insert(n.to_string(), (size, manifest));
            }
        }
        Ok(())
    })();
    for d in &made {
        for n in names {
            let _ = fs::remove_dir_all(src.join(d).join(format!("{n}-{version}")));
        }
        let _ = fs::remove_dir(src.join(d));
    }
    let _ = fs::remove_dir_all(&fresh);
    result.map(|()| p)
}

/// A folder no run used: `check-publish-` and a name made of the process
/// and the clock, made here so that no other run has it.
fn made_folder(parent: &Path) -> io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    for n in 0..1000 {
        let path = parent.join(format!("check-publish-{}-{stamp}-{n}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("no fresh folder could be made"))
}

/// Every `.d` file under `dir`, read with undecodable bytes replaced.
fn dep_info(dir: &Path, said: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => dep_info(&path, said),
            Ok(t) if t.is_file() && path.extension().is_some_and(|x| x == "d") => {
                if let Ok(b) = fs::read(&path) {
                    said.push(text::universal(&String::from_utf8_lossy(&b)));
                }
            }
            _ => {}
        }
    }
}

/// Python's truth of what `dict.get` returned.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Integer(n)) => *n != 0,
        Some(Value::Float(f)) => *f != 0.0,
        Some(Value::Boolean(b)) => *b,
        Some(Value::Datetime(_)) => true,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Table(t)) => !t.is_empty(),
    }
}

/// A number as Python compares it: a bool is an int.
enum Num {
    Int(i64),
    Float(f64),
}

fn number(v: &Value) -> Option<Num> {
    match v {
        Value::Boolean(b) => Some(Num::Int(i64::from(*b))),
        Value::Integer(n) => Some(Num::Int(*n)),
        Value::Float(f) => Some(Num::Float(*f)),
        _ => None,
    }
}

fn num_eq(a: Num, b: Num) -> bool {
    match (a, b) {
        (Num::Int(x), Num::Int(y)) => x == y,
        (Num::Float(x), Num::Float(y)) => x == y,
        // `int == float` is exact in Python: the float must be that
        // integer.
        (Num::Int(n), Num::Float(f)) | (Num::Float(f), Num::Int(n)) => {
            const TWO_63: f64 = 9_223_372_036_854_775_808.0;
            f.fract() == 0.0 && (-TWO_63..TWO_63).contains(&f) && f as i64 == n
        }
    }
}

/// `a == b` on what `tomllib` returned: a table compares without its
/// order, and `True == 1 == 1.0`.
fn py_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Integer(x), Value::Integer(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Datetime(x), Value::Datetime(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(x, y)| py_eq(x, y))
        }
        (Value::Table(x), Value::Table(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| py_eq(v, w)))
        }
        _ => match (number(a), number(b)) {
            (Some(x), Some(y)) => num_eq(x, y),
            _ => false,
        },
    }
}

fn opt_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => py_eq(a, b),
        _ => false,
    }
}

fn get<'a>(t: &'a Table, path: &[&str]) -> Option<&'a Value> {
    let (last, init) = path.split_last()?;
    let mut t = t;
    for k in init {
        t = t.get(*k)?.as_table()?;
    }
    t.get(*last)
}

/// `field()`: a `{ workspace = true }` value is the workspace's.
fn field<'a>(pkg: &'a Table, ws: &'a Table, key: &str) -> Option<&'a Value> {
    match pkg.get(key) {
        Some(Value::Table(t)) if truthy(t.get("workspace")) => {
            get(ws, &["workspace", "package", key])
        }
        v => v,
    }
}

fn string<'a>(t: &'a Table, key: &str) -> Result<&'a str, String> {
    match t.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(v) => Err(format!("TypeError: {key} is {}", py::repr(Some(v)))),
        None => Err(format!("KeyError: {}", py::repr_str(key))),
    }
}

fn strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// (table, name, spec) for every dependency a published manifest keeps.
fn deps(m: &Table) -> Vec<(String, &str, &Value)> {
    let mut out = Vec::new();
    for t in DEP_TABLES {
        if let Some(Value::Table(d)) = m.get(t) {
            out.extend(d.iter().map(|(n, s)| (t.to_string(), n.as_str(), s)));
        }
    }
    if let Some(Value::Table(targets)) = m.get("target") {
        for (target, tt) in targets {
            for t in DEP_TABLES {
                if let Some(Value::Table(d)) = tt.get(t) {
                    out.extend(
                        d.iter()
                            .map(|(n, s)| (format!("target.{target}.{t}"), n.as_str(), s)),
                    );
                }
            }
        }
    }
    out
}

/// The package a dependency names: its `package`, else its key.
fn dep_name<'a>(key: &'a str, spec: &'a Value) -> &'a str {
    match spec {
        Value::Table(t) => t.get("package").and_then(Value::as_str).unwrap_or(key),
        _ => key,
    }
}

/// A workspace member: its directory and its manifest.
pub struct Member {
    pub dir: String,
    pub manifest: Table,
}

/// The members `root` reaches through its dependencies, itself included.
pub fn closure(root: &str, mem: &BTreeMap<String, Member>) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut todo = vec![root.to_string()];
    while let Some(n) = todo.pop() {
        if !seen.insert(n.clone()) {
            continue;
        }
        let Some(m) = mem.get(&n) else { continue };
        for (_, d, s) in deps(&m.manifest) {
            let name = dep_name(d, s);
            if mem.contains_key(name) {
                todo.push(name.to_string());
            }
        }
    }
    seen
}

/// `os.path.normpath` of a relative path.
fn normpath(p: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let absolute = p.starts_with('/');
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            x => out.push(x),
        }
    }
    let joined = out.join("/");
    match (absolute, joined.is_empty()) {
        (true, _) => format!("/{joined}"),
        (false, true) => ".".to_string(),
        (false, false) => joined,
    }
}

/// `os.path.realpath` of an absolute path: each link resolved as it is
/// met, a part that does not exist kept as written, and a loop left as
/// the path it reached.
pub fn realpath(path: &Path) -> PathBuf {
    let mut out = PathBuf::from("/");
    let mut todo: Vec<PathBuf> = path
        .components()
        .rev()
        .map(|c| PathBuf::from(c.as_os_str()))
        .collect();
    let mut hops = 0;
    while let Some(part) = todo.pop() {
        match part.components().next() {
            Some(Component::RootDir) => out = PathBuf::from("/"),
            Some(Component::CurDir) | None => {}
            Some(Component::ParentDir) => {
                out.pop();
            }
            Some(Component::Prefix(_)) => {}
            Some(Component::Normal(n)) => {
                let next = out.join(n);
                let link = fs::symlink_metadata(&next).is_ok_and(|m| m.file_type().is_symlink());
                if link && hops < 40 {
                    hops += 1;
                    if let Ok(target) = fs::read_link(&next) {
                        todo.extend(
                            target
                                .components()
                                .rev()
                                .map(|c| PathBuf::from(c.as_os_str())),
                        );
                        continue;
                    }
                }
                out = next;
            }
        }
    }
    out
}

/// `os.path.relpath(path, start)` of two absolute paths.
fn relpath(path: &Path, start: &Path) -> String {
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .filter_map(|c| match c {
                Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    let (p, s) = (parts(path), parts(start));
    let common = p.iter().zip(&s).take_while(|(a, b)| a == b).count();
    let mut rel: Vec<String> = vec!["..".to_string(); s.len() - common];
    rel.extend(p[common..].iter().cloned());
    if rel.is_empty() {
        ".".to_string()
    } else {
        rel.join("/")
    }
}

fn is_path(notice: &str) -> bool {
    notice.contains('/') || notice == "LICENSE"
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    matches!((fs::read(a), fs::read(b)), (Ok(x), Ok(y)) if x == y)
}

/// One sidecar entry: a `[[tree]]` row, or an `[upstream.*]` group read as
/// one, with its files under [`SOURCES`].
pub struct Entry {
    pub what: String,
    pub paths: Vec<String>,
    pub notice: String,
    pub status: String,
    pub licence: String,
    pub source: Kind,
    pub upstream: String,
    pub public: Option<Value>,
    pub holder: Option<Value>,
    pub from: Option<Value>,
    pub spdx: Option<Value>,
}

/// Where an entry came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Tree,
    Source,
}

impl Entry {
    fn of(t: &Table, kind: Kind) -> Result<Entry, String> {
        let (what, paths, upstream) = match kind {
            Kind::Tree => (
                string(t, "what")?.to_string(),
                strings(t.get("paths")),
                string(t, "upstream")?.to_string(),
            ),
            Kind::Source => (
                string(t, "name")?.to_string(),
                strings(t.get("files"))
                    .into_iter()
                    .map(|f| format!("{SOURCES}{f}"))
                    .collect(),
                String::new(),
            ),
        };
        Ok(Entry {
            what,
            paths,
            notice: string(t, "notice")?.to_string(),
            status: string(t, "status")?.to_string(),
            licence: string(t, "licence")?.to_string(),
            source: kind,
            upstream,
            public: t.get("public").cloned(),
            holder: t.get("holder").cloned(),
            from: t.get("from").cloned(),
            spdx: t.get("spdx").cloned(),
        })
    }

    /// `e.get(key)` when it is truthy, as text.
    fn said(v: &Option<Value>) -> Option<String> {
        truthy(v.as_ref()).then(|| py::str_of(v.as_ref()))
    }
}

/// What the NOTICE of one crate is checked against beside its files.
pub struct Context<'a> {
    pub repo: &'a str,
    /// Directories of the repository no crate is packaged from.
    pub tops: Vec<String>,
    /// Each source file's URL, by its name under [`SOURCES`].
    pub url: HashMap<String, String>,
    /// The hyphenated source keys a NOTICE must not name bare. Only the
    /// hyphenated ones: a one-word key such as xterm or wayland is also the
    /// word, and a NOTICE may say it.
    pub keys: HashSet<String>,
    pub root: &'a Path,
}

/// The repository paths `text` names that the crate does not carry: a
/// token with a slash, URLs aside, under a directory that never ships, or
/// naming a file or directory of the repository that is not in the crate
/// under that name. An upstream project's own path, such as
/// OT/glyf/Glyph.hh, names nothing here and passes.
fn unshipped(text: &str, src: &HashMap<String, String>, cx: &Context) -> Vec<String> {
    let shipped: HashSet<&str> = src.values().map(String::as_str).collect();
    let mut bad = Vec::new();
    let cut = URL_IN.replace_all(text, " ");
    for tok in py::split(&cut) {
        let tok = tok.trim_matches(|c| ".,;:()'\"".contains(c));
        if !tok.contains('/') {
            continue;
        }
        let here = tok.trim_end_matches('/');
        if !ALNUM.is_match(here) {
            continue;
        }
        let prefix = format!("{here}/");
        let carried = shipped.contains(here) || shipped.iter().any(|f| f.starts_with(&prefix));
        let under = cx.tops.iter().any(|t| tok.starts_with(t.as_str()));
        if under || (!carried && cx.root.join(here).exists()) {
            bad.push(tok.to_string());
        }
    }
    bad
}

/// The hyphenated source keys `text` names as a word of its own, URLs
/// aside.
fn keys(text: &str, cx: &Context) -> Vec<String> {
    let cut = URL_IN.replace_all(text, " ");
    py::split(&cut)
        .into_iter()
        .map(|t| t.trim_matches(|c| ".,;:()[]'\"`".contains(c)))
        .filter(|t| cx.keys.contains(*t))
        .map(str::to_string)
        .collect()
}

/// What `text` says that a NOTICE must not: process wording, source keys
/// and repository-only paths.
pub fn wording(text: &str, src: &HashMap<String, String>, cx: &Context) -> Vec<String> {
    let mut found: Vec<String> = PROCESS
        .iter()
        .flat_map(|rx| rx.find_iter(text).map(|m| m.as_str().to_string()))
        .collect();
    found.extend(keys(text, cx));
    found.extend(unshipped(text, src, cx));
    found
}

fn meaning(status: &str) -> Result<&'static str, String> {
    STATUS
        .iter()
        .find(|(k, _)| *k == status)
        .map(|(_, m)| *m)
        .ok_or_else(|| format!("KeyError: {}", py::repr_str(status)))
}

/// A thing [`wording`] found in a NOTICE: the row's status and `what`,
/// and the finding.
pub type Finding = (String, String, String);

/// The NOTICE of crate `name`, and what is wrong with it: each sidecar
/// entry with a file in the crate, by the paths the crate gives its files.
/// `src` maps a repository path to the crate's path for it, both a file's
/// own path and the one a link resolves to. Every [[tree]] entry with a
/// file here is listed; a source group is left out when each of its files
/// here is already listed under a [[tree]] entry, as the target of a link
/// is. The second value is (status, what, finding) for each thing
/// [`wording`] finds in what the NOTICE prints of an entry, and for the
/// header ("own").
pub fn notice(
    name: &str,
    entries: &[Entry],
    src: &HashMap<String, String>,
    cx: &Context,
) -> Result<(String, Vec<Finding>), String> {
    let labelled = |label: &str, text: &str| {
        let initial = format!("  {label:<9}");
        Wrap {
            width: Count(76),
            initial_indent: &initial,
            subsequent_indent: "           ",
        }
        .fill(text)
    };
    let mut o = vec![
        format!("Third-party material in the {name} crate"),
        String::new(),
        Wrap::PLAIN.fill(&format!(
            "The licence of {name} is LICENSE. This file lists the material from other \
             projects that files in this crate carry: for each, the files by their paths \
             in this crate, the licence it keeps, and where the notice that licence asks \
             for is. It is generated from baud's record of third-party files, \
             in its repository at {repo}, whose THIRD_PARTY.md lists every such file \
             there.",
            repo = cx.repo
        )),
    ];
    let mut found: Vec<Finding> = wording(&o.join("\n"), src, cx)
        .into_iter()
        .map(|f| ("own".to_string(), "the header".to_string(), f))
        .collect();
    let mut shown: BTreeSet<String> = BTreeSet::new();
    let mut count = 0;
    for e in entries {
        let files: BTreeSet<String> = e.paths.iter().filter_map(|p| src.get(p).cloned()).collect();
        if files.is_empty() || (e.source == Kind::Source && files.is_subset(&shown)) {
            continue;
        }
        shown.extend(files.iter().cloned());
        count += 1;
        let text = Entry::said(&e.public).unwrap_or_else(|| e.what.clone());
        let origin: Vec<String> = match &e.from {
            v if truthy(v.as_ref()) => match v {
                Some(Value::Array(a)) => a.iter().map(|x| py::str_of(Some(x))).collect(),
                other => vec![py::str_of(other.as_ref())],
            },
            _ if !e.upstream.is_empty() => vec![e.upstream.clone()],
            _ => Vec::new(),
        };
        // A file of baud's sources is named by the URL it was copied from.
        let origin: Vec<String> = origin
            .into_iter()
            .map(|x| match x.strip_prefix(SOURCES) {
                Some(f) => cx.url.get(f).cloned().unwrap_or(x),
                None => x,
            })
            .collect();
        let note = e.notice.as_str();
        let lic = e.licence.as_str();
        let place = if note.starts_with("LICENSES/") || note == "LICENSE" {
            note.to_string()
        } else if is_path(note) {
            src.get(note).cloned().unwrap_or_else(|| note.to_string())
        } else {
            match note {
                "in file" => "in each file".to_string(),
                "uri" => match third_party::licence_uri(lic) {
                    Some(uri) => format!("the licence's URI, {uri}"),
                    None => "the licence's URI".to_string(),
                },
                "none needed" => "none needed".to_string(),
                "none" => "none".to_string(),
                other => other.to_string(),
            }
        };
        let files: Vec<String> = files.into_iter().collect();
        o.push(String::new());
        o.push(Wrap::PLAIN.fill(&text));
        o.push(labelled("Files:", &files.join(", ")));
        let holder = Entry::said(&e.holder);
        if let Some(h) = &holder {
            o.push(labelled("Holder:", h));
        }
        o.push(labelled("Licence:", lic));
        o.push(labelled("Notice:", &place));
        o.push(labelled(
            "Status:",
            &format!("{}: {}", e.status, meaning(&e.status)?),
        ));
        if !origin.is_empty() {
            o.push(labelled("From:", &origin.join(" and ")));
        }
        let holder_said = match &e.holder {
            Some(v) => py::str_of(Some(v)),
            None => String::new(),
        };
        let said = [text.as_str(), holder_said.as_str(), lic, place.as_str()].join("\n");
        found.extend(
            wording(&said, src, cx)
                .into_iter()
                .map(|f| (e.status.clone(), e.what.clone(), f)),
        );
        found.extend(
            origin
                .iter()
                .filter(|x| !URL.is_match(x))
                .map(|x| (e.status.clone(), e.what.clone(), format!("From: {x}"))),
        );
        // A settled row says in its own words what it is and whose.
        for (k, v) in [("public", &e.public), ("holder", &e.holder)] {
            if !truthy(v.as_ref()) {
                found.push((e.status.clone(), e.what.clone(), format!("no {k} field")));
            }
        }
    }
    if count == 0 {
        o.push(String::new());
        o.push(
            "No file in this crate carries material from another project that baud's".to_string(),
        );
        o.push("record of third-party files lists.".to_string());
    }
    Ok((format!("{}\n", o.join("\n")), found))
}

fn tokens(expr: &str) -> Vec<&str> {
    TOKENS.find_iter(expr).map(|m| m.as_str()).collect()
}

/// Whether `expr` is an SPDX licence expression crates.io takes: operands
/// of [`SPDX_IDS`] or LicenseRef- terms joined by AND and OR, in balanced
/// parentheses. WITH is refused, since no exception is listed.
pub fn spdx_ok(expr: &str) -> bool {
    let toks = tokens(expr);
    let mut depth = 0;
    let mut want_operand = true;
    for tok in &toks {
        match *tok {
            "(" => {
                if !want_operand {
                    return false;
                }
                depth += 1;
            }
            ")" => {
                if want_operand || depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            "AND" | "OR" => {
                if want_operand {
                    return false;
                }
                want_operand = true;
            }
            t => {
                if !want_operand || !(SPDX_IDS.contains(&t) || LICENCE_REF.is_match(t)) {
                    return false;
                }
                want_operand = false;
            }
        }
    }
    !toks.is_empty() && depth == 0 && !want_operand
}

/// What sidecar entry `e` adds to a crate's licence field: its `spdx` when
/// it has one ("" for facts and protocol data, which carry no licence),
/// else its `licence` when that is an expression crates.io takes, else
/// nothing.
fn row_spdx(e: &Entry) -> Option<String> {
    match &e.spdx {
        Some(v) => Some(py::str_of(Some(v))),
        None => spdx_ok(&e.licence).then(|| e.licence.clone()),
    }
}

/// The licence field of a crate whose third-party files add `terms`:
/// baud's own licence AND each of them. A term of ANDs alone is split into
/// its licences, so each is named once; a term with an OR is kept whole in
/// parentheses, since AND binds tighter than OR.
pub fn licence_expr(terms: &[String]) -> String {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for s in terms {
        if s.is_empty() {
            continue;
        }
        let toks = tokens(s);
        if toks.contains(&"OR") || toks.contains(&"(") {
            out.insert(if s.starts_with('(') && s.ends_with(')') {
                s.clone()
            } else {
                format!("({s})")
            });
        } else {
            out.extend(toks.into_iter().filter(|x| *x != "AND").map(str::to_string));
        }
    }
    out.remove(LICENCE);
    let mut all = vec![LICENCE.to_string()];
    all.extend(out);
    all.join(" AND ")
}

/// Set the `license` of the manifest at `path` to `expr`, in place of
/// whatever `license` line its [package] has.
pub fn write_licence(path: &Path, expr: &str) -> Result<(), String> {
    let shown = path.display();
    let body = text::read_universal(path).map_err(|e| format!("{shown}: {e}"))?;
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    let start = lines
        .iter()
        .position(|l| l == "[package]")
        .ok_or("ValueError: '[package]' is not in list")?;
    let end = (start + 1..lines.len())
        .find(|&i| lines[i].starts_with('['))
        .unwrap_or(lines.len());
    let hits: Vec<usize> = (start..end)
        .filter(|&i| LICENSE_LINE.is_match(&lines[i]))
        .collect();
    if hits.len() != 1 {
        return Err(format!(
            "{shown}: {} license lines in [package], not one",
            hits.len()
        ));
    }
    lines[hits[0]] = format!("license = \"{expr}\"");
    fs::write(path, lines.join("\n")).map_err(|e| format!("{shown}: {e}"))
}

/// What is wrong with `m`, the manifest of the package `cargo install`
/// builds as its own root, against the workspace's release settings.
pub fn root_settings(name: &str, m: &Table, ws: &Table) -> Vec<String> {
    let mut errs = Vec::new();
    let want = get(ws, &["profile", "release"]);
    let got = get(m, &["profile", "release"]);
    if !truthy(want) {
        errs.push(
            "the workspace has no [profile.release]: the profile check has stopped working"
                .to_string(),
        );
    } else if !opt_eq(got, want) {
        errs.push(format!(
            "{name}: [profile.release] is {}, not the workspace's {}",
            py::repr(got),
            py::repr(want)
        ));
    }
    let have = match m.get("cargo-features") {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    if let Some(Value::Array(fs)) = ws.get("cargo-features") {
        for f in fs {
            if !have.iter().any(|h| py_eq(h, f)) {
                errs.push(format!(
                    "{name}: cargo-features lacks {}, which its [profile.release] needs",
                    py::repr(Some(f))
                ));
            }
        }
    }
    errs
}

/// What failed, and the shipped files whose rows are not settled.
pub struct Report {
    pub errs: Vec<String>,
    pub awaiting: Vec<String>,
}

fn load(root: &Path, path: &str) -> Result<Table, String> {
    third_party::load(root, path)
}

/// The checks, with each crate's NOTICE written first under
/// `flags.notices` and its `license` under `flags.licences`. What is
/// printed on the way (the sizes, the count) goes to `out`.
pub fn check(
    root: &Path,
    flags: Flags,
    cargo: &Cargo,
    out: &mut impl Write,
) -> Result<Report, String> {
    let mut errs: Vec<String> = Vec::new();
    let mut awaiting: Vec<String> = Vec::new();
    let ws = load(root, "Cargo.toml")?;
    let version = match get(&ws, &["workspace", "package", "version"]) {
        Some(Value::String(v)) => v.clone(),
        v => return Err(format!("the workspace version is {}", py::repr(v))),
    };
    let empty = Table::new();
    let wdeps = get(&ws, &["workspace", "dependencies"])
        .and_then(Value::as_table)
        .unwrap_or(&empty);
    let mut mem: BTreeMap<String, Member> = BTreeMap::new();
    let dirs = strings(get(&ws, &["workspace", "members"]));
    for d in &dirs {
        let m = load(root, &format!("{d}/Cargo.toml"))?;
        let name = get(&m, &["package", "name"])
            .and_then(Value::as_str)
            .ok_or(format!("{d}/Cargo.toml: no package name"))?
            .to_string();
        mem.insert(
            name,
            Member {
                dir: d.clone(),
                manifest: m,
            },
        );
    }
    if !mem.contains_key("baud") {
        return Ok(Report {
            errs: vec![
                "no workspace member is named `baud`: `cargo install baud` would find nothing"
                    .to_string(),
            ],
            awaiting,
        });
    }
    let want = closure("baud", &mem);
    let published: BTreeSet<String> = mem
        .iter()
        .filter(|(_, m)| {
            !matches!(
                get(&m.manifest, &["package", "publish"]),
                Some(Value::Boolean(false))
            )
        })
        .map(|(n, _)| n.clone())
        .collect();
    for n in want.difference(&published) {
        errs.push(format!(
            "{n}: `baud` depends on it, so it must be published, and it says publish = false"
        ));
    }
    for n in published.difference(&want) {
        errs.push(format!("{n}: published, and `baud` does not depend on it"));
    }
    let unpublished: Vec<&str> = mem
        .keys()
        .filter(|n| !want.contains(*n))
        .map(String::as_str)
        .collect();
    let mut listed_unpub = unpublished.clone();
    listed_unpub.sort_unstable();
    let mut named = UNPUBLISHED.to_vec();
    named.sort_unstable();
    if listed_unpub != named {
        errs.push(format!(
            "unpublished members are {}, UNPUBLISHED says {}",
            py::repr_sorted(&unpublished),
            py::repr_sorted(&UNPUBLISHED)
        ));
    }
    if want.len() < CRATES_FLOOR.0 {
        errs.push(format!(
            "only {} crates reached from `baud`: the dependency walk has stopped working",
            want.len()
        ));
    }

    errs.extend(root_settings("baud", &mem["baud"].manifest, &ws));
    for n in &want {
        let m = &mem[n];
        let p = m
            .manifest
            .get("package")
            .and_then(Value::as_table)
            .unwrap_or(&empty);
        for key in ["description", "repository", "rust-version"] {
            if !truthy(field(p, &ws, key)) {
                errs.push(format!("{n}: no {key}"));
            }
        }
        let readme = field(p, &ws, "readme");
        let is_file = match readme {
            Some(Value::String(r)) if !r.is_empty() => root.join(&m.dir).join(r).is_file(),
            _ => false,
        };
        if !is_file {
            errs.push(format!("{n}: readme {} is not a file", py::repr(readme)));
        }
        let none = Value::Array(Vec::new());
        let kw = match field(p, &ws, "keywords") {
            v if truthy(v) => v.unwrap_or(&none),
            _ => &none,
        };
        let kw_list = kw.as_array().cloned().unwrap_or_default();
        let kw_ok = (1..=5).contains(&kw_list.len())
            && kw_list
                .iter()
                .all(|k| k.as_str().is_some_and(|k| KEYWORD.is_match(k)));
        if !kw_ok {
            errs.push(format!(
                "{n}: keywords {} are not one to five crates.io keywords",
                py::str_of(Some(kw))
            ));
        }
        let cats = match field(p, &ws, "categories") {
            v if truthy(v) => v.unwrap_or(&none),
            _ => &none,
        };
        let cat_list = cats.as_array().cloned().unwrap_or_default();
        let cats_ok = (1..=5).contains(&cat_list.len())
            && cat_list
                .iter()
                .all(|c| c.as_str().is_some_and(|c| CATEGORIES.contains(&c)));
        if !cats_ok {
            errs.push(format!(
                "{n}: categories {} are not one to five of CATEGORIES",
                py::str_of(Some(cats))
            ));
        }
        for (t, dn, s) in deps(&m.manifest) {
            let name = dep_name(dn, s);
            if !mem.contains_key(name) {
                continue;
            }
            let inherited = matches!(s, Value::Table(st) if truthy(st.get("workspace")));
            let spec = if inherited {
                wdeps.get(dn).cloned().unwrap_or(Value::Table(Table::new()))
            } else {
                s.clone()
            };
            let Some(spec) = spec.as_table().filter(|t| t.contains_key("path")) else {
                errs.push(format!(
                    "{n}: [{t}] {dn} is a workspace crate taken without a path"
                ));
                continue;
            };
            let v = Value::String(version.clone());
            if !opt_eq(spec.get("version"), Some(&v)) {
                errs.push(format!(
                    "{n}: [{t}] {dn} has version {}, not {}: \
                     cargo refuses to package a path dependency with none",
                    py::repr(spec.get("version")),
                    py::repr_str(&version)
                ));
            }
        }
    }

    let side = load(root, SIDECAR)?;
    let tree_rows: Vec<&Table> = match side.get("tree") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_table).collect(),
        _ => return Err("KeyError: 'tree'".to_string()),
    };
    let upstream = side
        .get("upstream")
        .and_then(Value::as_table)
        .ok_or("KeyError: 'upstream'")?;
    for e in tree_rows
        .iter()
        .copied()
        .chain(upstream.values().filter_map(Value::as_table))
    {
        if let Some(v) = e.get("spdx") {
            if truthy(Some(v)) && !spdx_ok(&py::str_of(Some(v))) {
                errs.push(format!(
                    "{SIDECAR}: spdx {} is no SPDX expression crates.io takes",
                    py::repr(Some(v))
                ));
            }
        }
    }
    let mut entries: Vec<Entry> = Vec::new();
    for t in &tree_rows {
        entries.push(Entry::of(t, Kind::Tree)?);
    }
    let mut groups: Vec<(&String, &Value)> = upstream.iter().collect();
    groups.sort_by(|a, b| a.0.cmp(b.0));
    for (id, g) in groups {
        let g = g.as_table().ok_or(format!("TypeError: upstream.{id}"))?;
        entries.push(Entry::of(g, Kind::Source)?);
    }
    let texts = side
        .get("text")
        .and_then(Value::as_table)
        .ok_or("KeyError: 'text'")?;
    let man = load(root, &format!("{SOURCES}MANIFEST.toml"))?;
    let sources = man
        .get("sources")
        .and_then(Value::as_table)
        .ok_or("KeyError: 'sources'")?;
    let mut url = HashMap::new();
    for s in sources.values().filter_map(Value::as_table) {
        if truthy(s.get("file")) {
            url.insert(py::str_of(s.get("file")), py::str_of(s.get("url")));
        }
    }
    let gkeys: HashSet<String> = sources
        .keys()
        .filter(|k| k.contains('-'))
        .cloned()
        .collect();
    if gkeys.len() < KEYS_FLOOR.0 {
        errs.push(format!(
            "only {} hyphenated source keys in MANIFEST.toml: the key check has stopped working",
            gkeys.len()
        ));
    }
    let repo = match get(&ws, &["workspace", "package", "repository"]) {
        Some(Value::String(r)) => r.clone(),
        v => return Err(format!("the workspace repository is {}", py::repr(v))),
    };
    let mut tops: Vec<String> = ["docs", "scripts", ".github", "nix", "packaging"]
        .iter()
        .map(|d| format!("{d}/"))
        .collect();
    tops.extend(dirs.iter().map(|d| format!("{d}/")));
    let cx = Context {
        repo: &repo,
        tops,
        url,
        keys: gkeys,
        root,
    };
    for n in &want {
        let d = mem[n].dir.as_str();
        let files = match cargo.listed(root, n) {
            Ok(f) => f,
            Err(e) => {
                errs.push(format!("{n}: cargo package --list failed: {e}"));
                continue;
            }
        };
        let has = |f: &str| files.iter().any(|x| x == f);
        if !has("LICENSE") {
            errs.push(format!("{n}: LICENSE is not in the package"));
        } else if !same_bytes(&root.join(d).join("LICENSE"), &root.join("LICENSE")) {
            errs.push(format!("{n}: its LICENSE is not the root's"));
        }
        let mut src: HashMap<String, String> = HashMap::new();
        for f in &files {
            let at = root.join(d).join(f);
            let r = relpath(&realpath(&at), root);
            let link = fs::symlink_metadata(&at).is_ok_and(|m| m.file_type().is_symlink());
            if link && (r.starts_with("..") || !root.join(&r).is_file()) {
                errs.push(format!(
                    "{n}: {f} links to {r}, which is not a file in the repository"
                ));
            }
            src.entry(r).or_insert_with(|| f.clone());
            src.entry(normpath(&format!("{d}/{f}")))
                .or_insert_with(|| f.clone());
        }
        let (want_notice, found) = notice(n, &entries, &src, &cx)?;
        let mut terms = Vec::new();
        for e in &entries {
            let hit: Vec<&str> = e
                .paths
                .iter()
                .filter_map(|x| src.get(x).map(String::as_str))
                .collect();
            if hit.is_empty() {
                continue;
            }
            match row_spdx(e) {
                Some(s) => terms.push(s),
                None => {
                    let line = format!(
                        "{n}: ships {} ({}...), whose licence {} \
                         is no SPDX expression crates.io takes, and the entry has no spdx field",
                        hit.join(", "),
                        py::head(&e.what, Count(50)),
                        py::repr_str(&e.licence)
                    );
                    if UNSETTLED.contains(&e.status.as_str()) {
                        awaiting.push(line);
                    } else {
                        errs.push(line);
                    }
                }
            }
        }
        let want_lic = licence_expr(&terms);
        let manifest_path = root.join(d).join("Cargo.toml");
        if flags.licences == Writes::Yes {
            write_licence(&manifest_path, &want_lic)?;
        }
        let fresh_manifest = load(root, &format!("{d}/Cargo.toml"))?;
        let got = get(&fresh_manifest, &["package", "license"]);
        let got = match got {
            Some(Value::Table(t)) if truthy(t.get("workspace")) => {
                get(&ws, &["workspace", "package", "license"])
            }
            v => v,
        };
        if !opt_eq(got, Some(&Value::String(want_lic.clone()))) {
            errs.push(format!(
                "{n}: license is {}, and its files give {}: run {COMMAND} --write-licences",
                py::repr(got),
                py::repr_str(&want_lic)
            ));
        }
        for (status, what, f) in &found {
            let line = format!(
                "{n}: NOTICE says {} ({}...)",
                py::repr_str(f),
                py::head(what, Count(50))
            );
            // A row awaiting a decision is refused at release anyway, and
            // is reworded when it is settled.
            if UNSETTLED.contains(&status.as_str()) {
                awaiting.push(line);
            } else {
                errs.push(line);
            }
        }
        let path = root.join(d).join("NOTICE");
        if flags.notices == Writes::Yes {
            fs::write(&path, &want_notice).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        if !path.is_file() {
            errs.push(format!("{n}: has no NOTICE: run {COMMAND} --write-notices"));
        } else if !has("NOTICE")
            && (flags.notices == Writes::No
                || !cargo
                    .listed(root, n)
                    .unwrap_or_default()
                    .iter()
                    .any(|x| x == "NOTICE"))
        {
            // A NOTICE written just now is listed only from here on.
            let done = if flags.notices == Writes::Yes {
                "NOTICE written, but "
            } else {
                ""
            };
            errs.push(format!(
                "{n}: {done}NOTICE is not in the package: add /NOTICE to its include"
            ));
        } else if text::read_universal(&path).map_err(|e| format!("{}: {e}", path.display()))?
            != want_notice
        {
            errs.push(format!(
                "{n}: NOTICE is not what its files' rows give: run {COMMAND} --write-notices"
            ));
        }
        for e in &entries {
            let what = py::head(&e.what, Count(50));
            let hit: Vec<&String> = e.paths.iter().filter(|p| src.contains_key(*p)).collect();
            if !hit.is_empty() && UNSETTLED.contains(&e.status.as_str()) {
                let shipped: Vec<&str> = hit.iter().map(|p| src[*p].as_str()).collect();
                awaiting.push(format!(
                    "{n}: ships {} ({what}...), status {}",
                    shipped.join(", "),
                    e.status
                ));
            }
            if hit.is_empty() || !is_path(&e.notice) {
                continue;
            }
            let first = &src[hit[0]];
            if let Some(name) = e.notice.strip_prefix("LICENSES/") {
                // A text another names as part of its notice goes with it.
                let needs = strings(
                    texts
                        .get(name)
                        .and_then(Value::as_table)
                        .and_then(|t| t.get("needs")),
                );
                for text in std::iter::once(e.notice.clone()).chain(needs) {
                    if !has(&text) {
                        errs.push(format!("{n}: ships {first} ({what}) without {text}"));
                    } else if !same_bytes(&root.join(d).join(&text), &root.join(&text)) {
                        errs.push(format!("{n}: {text} is not the root's"));
                    }
                }
            } else if e.notice != "LICENSE" && !src.contains_key(&e.notice) {
                errs.push(format!(
                    "{n}: ships {first} ({what}) without its notice {}",
                    e.notice
                ));
            }
        }
    }

    if flags.pack != Pack::No && errs.is_empty() {
        let names: Vec<&str> = want.iter().map(String::as_str).collect();
        let p = package(root, &names, &version, flags.pack, cargo, Folder::Fresh)
            .map_err(|e| e.to_string())?;
        let failed = !p.errs.is_empty();
        errs.extend(p.errs);
        if flags.pack == Pack::Verify && !failed && p.read.is_empty() {
            errs.push(
                "the verify build read no crate from cargo's unpack of the others: \
                 the check that it is this run's has stopped working"
                    .to_string(),
            );
        }
        errs.extend(sized(&p.crates, &ws, out)?);
    }
    if errs.is_empty() {
        writeln!(
            out,
            "checked: {} crates to publish, {} not",
            want.len(),
            mem.len() - want.len()
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(Report { errs, awaiting })
}

/// `{b / 1048576:.2f}`.
pub fn mib(b: Bytes) -> String {
    let whole = b.0 / 1_048_576;
    let part = b.0 % 1_048_576;
    // Exact: hundredths, rounded half to even, as Python's float format
    // rounds the exact binary value, which a quotient by 2^20 always is.
    let scaled = part * 100;
    let (mut hundredths, rem) = (scaled / 1_048_576, scaled % 1_048_576);
    let half = 1_048_576 / 2;
    if rem > half || (rem == half && hundredths % 2 == 1) {
        hundredths += 1;
    }
    let (whole, hundredths) = if hundredths == 100 {
        (whole + 1, 0)
    } else {
        (whole, hundredths)
    };
    format!("{whole}.{hundredths:02}")
}

/// Each .crate's size line, and what is wrong with the sizes and with the
/// manifest inside baud's.
pub fn sized(
    crates: &BTreeMap<String, (Bytes, Vec<u8>)>,
    ws: &Table,
    out: &mut impl Write,
) -> Result<Vec<String>, String> {
    let mut errs = Vec::new();
    for (n, (b, manifest)) in crates {
        writeln!(out, "  {n}: {} MiB compressed", mib(*b)).map_err(|e| e.to_string())?;
        if *b > LIMIT {
            errs.push(format!("{n}: {} bytes, over crates.io's {}", b.0, LIMIT.0));
        }
        if n == "baud" {
            let text = String::from_utf8(manifest.clone()).map_err(|e| format!("{n}: {e}"))?;
            let m: Table = text
                .parse()
                .map_err(|e| format!("{n}'s packaged Cargo.toml: {e}"))?;
            errs.extend(
                root_settings(n, &m, ws)
                    .into_iter()
                    .map(|e| format!("{e} in its .crate")),
            );
        }
    }
    Ok(errs)
}

/// The whole command: the checks, then each FAIL, each unsettled row, and
/// the verdict, as the script printed them.
pub fn run(root: &Path, flags: Flags, cargo: &Cargo, out: &mut impl Write) -> Result<Exit, String> {
    let r = check(root, flags, cargo, out)?;
    let w = |out: &mut dyn Write, s: String| writeln!(out, "{s}").map_err(|e| e.to_string());
    for e in &r.errs {
        w(out, format!("FAIL {e}"))?;
    }
    let label = match flags.release {
        Release::Yes => "REFUSED",
        Release::No => "awaiting the owner",
    };
    for a in &r.awaiting {
        w(out, format!("{label}: {a}"))?;
    }
    if !r.errs.is_empty() {
        return Ok(Exit::Failed);
    }
    if !r.awaiting.is_empty() && flags.release == Release::Yes {
        w(
            out,
            format!(
                "not releasable: {} shipped files' licences await the owner",
                r.awaiting.len()
            ),
        )?;
        return Ok(Exit::Refused);
    }
    w(out, "ok".to_string())?;
    Ok(Exit::Passed)
}
