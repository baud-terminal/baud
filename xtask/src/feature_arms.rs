//! `cargo xtask feature-arms [--since <rev>]`: every cargo feature of every
//! listed crate builds alone.
//!
//! An arm is built the way it is used, `--features <arm>` alone: building
//! them all at once lets one arm's code mask another's errors. Each arm is
//! `cargo check --release -q --all-targets` under `RUSTFLAGS="-D warnings"`,
//! by its crate's manifest, because a bare check covers the library and the
//! binaries only and a stubbed body that leaves a variable unused is a
//! warning the arm must not carry. A crate with default features has one
//! more arm, the build without them (`--no-default-features`), since cargo
//! builds a dependency once with every feature any crate in the build
//! enables on it.
//!
//! Before anything builds:
//!
//! - every top-level crate with a `[features]` block is in [`CRATES`], or
//!   the gate would check fewer crates than the workspace has;
//! - every declared feature has a reader, `feature = "name"` on a line of
//!   its crate's sources with `//` comments removed, or forwards to another
//!   crate (`dep/feat`), or is in [`DEAD_OK`] with its reason: a declared
//!   arm that nothing reads builds the base program and measures nothing;
//! - each crate's features, read from its `[features]` block, number at
//!   least its floor, so a parse that stopped matching fails rather than
//!   checks nothing. A crate whose manifest is missing or reads short stops
//!   the gate at its place in the list, after the crates before it are
//!   reported.
//!
//! **The arms run in parallel**, `CHECK_FEATURES_JOBS` workers (half the
//! CPUs, at most four, by default), each building in a target directory of
//! its own, `<CARGO_TARGET_DIR, or target>/arms-<i>`, because two cargo
//! runs in one target directory queue on its lock. An arm goes to the
//! worker its place in the full list names, modulo the count, so a
//! directory stays warm for the arms it built before. The results print in
//! the list's order as they come in.
//!
//! **`--since <rev>` checks only the crates a change can reach**, as
//! `cargo xtask features-since` names them; every floor is checked whether
//! its arms run or not, and each crate skipped is named.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::num::NonZeroUsize;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use regex::Regex;

use crate::features_since::{self, Exit};
use crate::third_party::Outcome;

/// The fewest features a crate's manifest must yield.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Floor(pub usize);

/// Every crate with a `[features]` block, by directory, each with its floor:
/// the fewest features its manifest may yield, so a parse that reads short
/// fails here.
pub const CRATES: [(&str, Floor); 14] = [
    ("baud-bench", Floor(53)),
    ("baud-core", Floor(54)),
    ("baud-domain", Floor(7)),
    ("baud-isa", Floor(1)),
    ("baud-isa-aarch64", Floor(1)),
    ("baud-isa-x86-64", Floor(1)),
    ("baud-font", Floor(0)),
    ("baud-render", Floor(4)),
    ("baud-present-macos", Floor(2)),
    ("baud-app", Floor(4)),
    ("baud-wayland", Floor(0)),
    ("baud-xkb", Floor(0)),
    ("baud-present-wayland", Floor(0)),
    ("baud-present-vulkan", Floor(1)),
];

/// Declared features with no reader, as `crate/name`, each allowed for a
/// reason written beside it. None is.
pub const DEAD_OK: [&str; 0] = [];

/// One arm: a crate's directory and a feature, or `None` for the build
/// without the defaults.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Arm {
    /// The crate's directory below the root.
    pub krate: String,
    /// The feature built alone.
    pub feature: Option<String>,
}

impl Arm {
    /// The arm as the report names it.
    pub fn label(&self) -> String {
        match &self.feature {
            Some(f) => format!("{}/{f}", self.krate),
            None => format!("{} --no-default-features", self.krate),
        }
    }
}

/// Checks one arm.
pub trait Check: Sync {
    /// Checks `arm`, building in `target`: whether it built, and what the
    /// build printed.
    fn check(&self, arm: &Arm, target: &Path) -> (Outcome, String);
}

/// The cargo that runs this program, from the repository's root.
pub struct Cargo {
    /// The repository's root.
    pub root: PathBuf,
}

impl Check for Cargo {
    fn check(&self, arm: &Arm, target: &Path) -> (Outcome, String) {
        let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut c = Command::new(cargo);
        c.args(["check", "--release", "-q", "--manifest-path"])
            .arg(format!("{}/Cargo.toml", arm.krate))
            .arg("--all-targets");
        match &arm.feature {
            Some(f) => c.args(["--features", f.as_str()]),
            None => c.arg("--no-default-features"),
        };
        c.env("RUSTFLAGS", "-D warnings")
            .env("CARGO_TARGET_DIR", target)
            .current_dir(&self.root)
            .stdin(Stdio::null());
        match c.output() {
            Ok(o) => {
                let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&o.stderr));
                let ok = if o.status.success() {
                    Outcome::Passed
                } else {
                    Outcome::Failed
                };
                (ok, text)
            }
            Err(e) => (Outcome::Failed, format!("error: cargo: {e}\n")),
        }
    }
}

/// What the environment gives a run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// `CHECK_FEATURES_JOBS`, when set and not empty.
    pub jobs: Option<String>,
    /// The CPUs this machine has online.
    pub cpus: NonZeroUsize,
    /// `CARGO_TARGET_DIR`, else `CARGO_BUILD_TARGET_DIR`, else `target`.
    pub target: PathBuf,
}

impl Settings {
    /// The settings this process's environment gives.
    pub fn from_env() -> Settings {
        let var = |k: &str| env::var(k).ok().filter(|v| !v.is_empty());
        Settings {
            jobs: var("CHECK_FEATURES_JOBS"),
            cpus: thread::available_parallelism().unwrap_or(NonZeroUsize::MIN),
            target: PathBuf::from(
                var("CARGO_TARGET_DIR")
                    .or_else(|| var("CARGO_BUILD_TARGET_DIR"))
                    .unwrap_or_else(|| "target".to_string()),
            ),
        }
    }
}

/// How many workers check arms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Jobs(pub NonZeroUsize);

/// The workers: `var` when given, which must be a number of workers, 1 or
/// more, with no leading zero; else half of `cpus`, at most four and at
/// least one. The line that refuses `var`, when it is refused.
pub fn jobs(var: Option<&str>, cpus: NonZeroUsize) -> Result<Jobs, String> {
    match var {
        None => {
            let half = (cpus.get() / 2).min(4);
            Ok(Jobs(NonZeroUsize::new(half).unwrap_or(NonZeroUsize::MIN)))
        }
        Some(v) => {
            let digits = !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit());
            match v.parse::<NonZeroUsize>() {
                Ok(n) if digits && !v.starts_with('0') => Ok(Jobs(n)),
                _ => Err(format!(
                    "FAIL: CHECK_FEATURES_JOBS is '{v}'; it takes a number of workers, 1 or more"
                )),
            }
        }
    }
}

/// The features a manifest declares: each line of its `[features]` block
/// that holds a `=`, by its first word, but a line whose first word is `#`
/// and the feature `default`.
pub fn declared(manifest: &str) -> Vec<String> {
    let mut inside = Section::Other;
    let mut out = Vec::new();
    for line in manifest.lines() {
        if line.starts_with("[features]") {
            inside = Section::Features;
            continue;
        }
        if line.starts_with('[') {
            inside = Section::Other;
        }
        if inside != Section::Features || !line.contains('=') {
            continue;
        }
        let first = line.split_whitespace().next();
        if let Some(word) = first.filter(|w| *w != "#" && *w != "default") {
            out.push(word.to_string());
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Features,
    Other,
}

/// Whether a manifest has default features: a line that starts
/// `default =`.
pub fn has_defaults(manifest: &str) -> bool {
    manifest.lines().any(|l| {
        l.strip_prefix("default")
            .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with('='))
    })
}

/// The directories directly below `root` that hold a `Cargo.toml`, in byte
/// order, hidden ones left out.
pub fn members(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(root)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| !n.starts_with('.'))
                .filter(|n| root.join(n).join("Cargo.toml").is_file())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

fn read(path: &Path) -> String {
    fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// The crates below `root` with a `[features]` block that `crates` does not
/// name.
pub fn unlisted(root: &Path, crates: &[(&str, Floor)]) -> Vec<String> {
    members(root)
        .into_iter()
        .filter(|c| {
            read(&root.join(c).join("Cargo.toml"))
                .lines()
                .any(|l| l.starts_with("[features]"))
        })
        .filter(|c| !crates.iter().any(|(n, _)| n == c))
        .collect()
}

/// Every `.rs` file at or below `path`, symbolic links not followed.
fn rust_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut names: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        names.sort();
        for p in names {
            rust_files(&p, out);
        }
    } else if path.extension().is_some_and(|x| x == "rs") {
        out.push(path.to_path_buf());
    }
}

/// The features a crate's own code reads: `feature = "name"` in its
/// `src`, `tests`, `benches`, `examples` and `build.rs`, each line read up
/// to its first `//`.
pub fn readers(dir: &Path) -> BTreeSet<String> {
    let re =
        Regex::new(r#"feature[[:space:]]*=[[:space:]]*"([^"]*)""#).expect("the reader pattern");
    let mut files = Vec::new();
    for sub in ["src", "tests", "benches", "examples", "build.rs"] {
        rust_files(&dir.join(sub), &mut files);
    }
    let mut out = BTreeSet::new();
    for f in files {
        for line in read(&f).lines() {
            let code = line.find("//").map_or(line, |at| &line[..at]);
            for c in re.captures_iter(code) {
                out.insert(c[1].to_string());
            }
        }
    }
    out
}

/// The declared features below `root` that nothing reads, as
/// `crate/name`: a feature whose first line in its manifest forwards to
/// another crate (`dep/feat`) needs no reader of its own.
pub fn dead(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for c in members(root) {
        let manifest = read(&root.join(&c).join("Cargo.toml"));
        let names = readers(&root.join(&c));
        for name in declared(&manifest) {
            let first = manifest.lines().find(|l| {
                l.strip_prefix(name.as_str())
                    .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with('='))
            });
            if first.is_some_and(|l| l.contains('/')) || names.contains(&name) {
                continue;
            }
            let label = format!("{c}/{name}");
            if !DEAD_OK.contains(&label.as_str()) {
                out.push(label);
            }
        }
    }
    out
}

/// The lines naming where a dead feature is forwarded from: each line of
/// each manifest below `root` holding `"<anything>/<leaf>"`.
fn forwarded(root: &Path, leaf: &str) -> Vec<String> {
    let re = Regex::new(&format!(r#""[^"]*/{}""#, regex::escape(leaf))).expect("a leaf pattern");
    let mut out = Vec::new();
    for c in members(root) {
        let text = read(&root.join(&c).join("Cargo.toml"));
        for (n, line) in text.lines().enumerate() {
            if re.is_match(line) {
                out.push(format!("      forwarded from {c}: {}:{line}", n + 1));
            }
        }
    }
    out
}

/// What `--since` found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Since {
    /// Every crate is checked, and why.
    All(String),
    /// Only these crates are, each with why.
    Reach(Vec<(String, String)>),
    /// The revision is refused: the lines to print.
    Refused(String),
    /// The scope could not be computed, so every crate is checked.
    Unknown,
}

/// `features-since`'s exit and output, read: `FAIL` with exit 1 is
/// refused, `all<TAB>why` with exit 0 checks every crate, any other
/// output with exit 0 names the crates reached, and anything else is
/// unknown.
pub fn interpret(exit: Exit, output: &str) -> Since {
    let text = output.trim_end_matches('\n');
    match exit {
        Exit(1) if text.starts_with("FAIL:") => Since::Refused(text.to_string()),
        Exit(0) => match text.strip_prefix("all\t") {
            Some(why) => Since::All(why.to_string()),
            None => Since::Reach(
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| {
                        let mut f = l.split('\t');
                        let c = f.next().unwrap_or("").to_string();
                        (c, f.next().unwrap_or("").to_string())
                    })
                    .collect(),
            ),
        },
        _ => Since::Unknown,
    }
}

/// `features-since <rev>`, run in this process.
pub fn since(rev: &str) -> Since {
    let mut out = Vec::new();
    match features_since::run(&[rev], &mut out) {
        Ok(exit) => interpret(exit, &String::from_utf8_lossy(&out)),
        Err(_) => Since::Unknown,
    }
}

/// Whether a crate's arms run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Runs {
    Checked,
    Skipped,
}

/// One crate of the plan.
struct Planned {
    name: String,
    features: Vec<String>,
    runs: Runs,
    arms: Range<usize>,
}

/// What a run of the gate is given.
pub struct Gate<'a> {
    /// The repository's root.
    pub root: &'a Path,
    /// The crates and their floors.
    pub crates: &'a [(&'a str, Floor)],
    /// The environment's settings.
    pub settings: &'a Settings,
    /// What checks an arm.
    pub check: &'a dyn Check,
    /// What `--since <rev>` reaches.
    pub since: &'a dyn Fn(&str) -> Since,
}

/// The command: `args` are what follows its name. Exit 0 when every arm
/// builds, 1 on a failure, 2 on a bad command line or worker count.
pub fn run(gate: &Gate, args: &[&str], out: &mut dyn Write) -> io::Result<Exit> {
    let rev = match args {
        [] => None,
        ["--since", rev] if !rev.is_empty() => Some(*rev),
        _ => {
            writeln!(out, "usage: cargo xtask feature-arms [--since <rev>]")?;
            return Ok(Exit(2));
        }
    };
    let jobs = match jobs(gate.settings.jobs.as_deref(), gate.settings.cpus) {
        Ok(Jobs(j)) => j.get(),
        Err(line) => {
            writeln!(out, "{line}")?;
            return Ok(Exit(2));
        }
    };
    let root = gate.root;

    let missing = unlisted(root, gate.crates);
    if !missing.is_empty() {
        writeln!(
            out,
            "FAIL: these crates have [features] and are not in CRATES:"
        )?;
        for c in &missing {
            writeln!(out, "  {c}")?;
        }
        writeln!(
            out,
            "  -- the gate would check fewer crates than the workspace has"
        )?;
        return Ok(Exit(1));
    }

    let unread = dead(root);
    if !unread.is_empty() {
        writeln!(
            out,
            "FAIL: these features are declared and no #[cfg] reads them:"
        )?;
        for d in &unread {
            writeln!(out, "  {d}")?;
            let leaf = d.rsplit('/').next().unwrap_or(d);
            for line in forwarded(root, leaf) {
                writeln!(out, "{line}")?;
            }
        }
        writeln!(
            out,
            "  -- enabling one builds the base program and measures nothing"
        )?;
        return Ok(Exit(1));
    }

    let mut reach: Option<BTreeSet<String>> = None;
    if let Some(rev) = rev {
        match (gate.since)(rev) {
            Since::Refused(text) => {
                writeln!(out, "{text}")?;
                return Ok(Exit(1));
            }
            Since::All(why) => writeln!(out, "since {rev}: {why}")?,
            Since::Reach(lines) => {
                let listed: Vec<String> = gate
                    .crates
                    .iter()
                    .flat_map(|(c, _)| {
                        lines
                            .iter()
                            .filter(move |(k, _)| k == c)
                            .map(move |(_, why)| format!("  {c}: {why}"))
                    })
                    .collect();
                if listed.is_empty() {
                    writeln!(
                        out,
                        "since {rev}: no change reaches a crate with feature arms"
                    )?;
                } else {
                    writeln!(out, "since {rev}: checking the crates a change can reach")?;
                    for l in &listed {
                        writeln!(out, "{l}")?;
                    }
                }
                reach = Some(lines.into_iter().map(|(c, _)| c).collect());
            }
            Since::Unknown => writeln!(
                out,
                "since {rev}: the scope command failed, so every crate is checked"
            )?,
        }
    }

    // The plan: every arm, in the order the report gives it.
    let mut planned: Vec<Planned> = Vec::new();
    let mut arms: Vec<(Arm, usize)> = Vec::new();
    let mut halt = None;
    let mut place = 0;
    for (name, Floor(min)) in gate.crates {
        let path = format!("{name}/Cargo.toml");
        let Ok(bytes) = fs::read(root.join(&path)) else {
            halt = Some(format!(
                "FAIL: {path} does not exist -- the gate would have read nothing for {name}"
            ));
            break;
        };
        let manifest = String::from_utf8_lossy(&bytes);
        let features = declared(&manifest);
        if features.len() < *min {
            halt = Some(format!(
                "FAIL: only {} features parsed from {path} (expected at least {min}) -- the gate is reading nothing",
                features.len()
            ));
            break;
        }
        let runs = match &reach {
            Some(r) if !r.contains(*name) => Runs::Skipped,
            _ => Runs::Checked,
        };
        let mut wanted: Vec<Option<String>> = features.iter().cloned().map(Some).collect();
        if has_defaults(&manifest) {
            wanted.push(None);
        }
        let first = arms.len();
        for feature in wanted {
            if runs == Runs::Checked {
                let arm = Arm {
                    krate: name.to_string(),
                    feature,
                };
                arms.push((arm, place % jobs));
            }
            place += 1;
        }
        planned.push(Planned {
            name: name.to_string(),
            features,
            runs,
            arms: first..arms.len(),
        });
    }

    let base = &gate.settings.target;
    let check = gate.check;
    let (fail, total, skipped) = thread::scope(|s| -> io::Result<(Outcome, usize, usize)> {
        let (tx, rx) = mpsc::channel::<(usize, String)>();
        if !arms.is_empty() {
            for w in 0..jobs {
                let tx = tx.clone();
                let arms = &arms;
                s.spawn(move || {
                    let target = base.join(format!("arms-{w}"));
                    for (k, (arm, slot)) in arms.iter().enumerate() {
                        if *slot != w {
                            continue;
                        }
                        let (ok, text) = check.check(arm, &target);
                        let lines = match ok {
                            Outcome::Passed => String::new(),
                            Outcome::Failed => broken(arm, &text),
                        };
                        if tx.send((k, lines)).is_err() {
                            return;
                        }
                    }
                });
            }
        }
        drop(tx);
        let mut got: BTreeMap<usize, String> = BTreeMap::new();
        let mut fail = Outcome::Passed;
        let (mut total, mut skipped) = (0, 0);
        for p in &planned {
            match p.runs {
                Runs::Skipped => {
                    let rev = rev.unwrap_or("");
                    writeln!(
                        out,
                        "{}: skipped, nothing it is built from changed since {rev}",
                        p.name
                    )?;
                    skipped += 1;
                }
                Runs::Checked => writeln!(
                    out,
                    "{}: checking {} feature arms individually (cargo check, -D warnings)",
                    p.name,
                    p.features.len()
                )?,
            }
            for k in p.arms.clone() {
                total += 1;
                while !got.contains_key(&k) {
                    match rx.recv() {
                        Ok((done, lines)) => {
                            got.insert(done, lines);
                        }
                        Err(_) => break,
                    }
                }
                match got.get(&k) {
                    None => {
                        fail = Outcome::Failed;
                        writeln!(out, "  BROKEN  {}", arms[k].0.label())?;
                        writeln!(out, "      no result: the worker checking it ended first")?;
                    }
                    Some(lines) if !lines.is_empty() => {
                        fail = Outcome::Failed;
                        write!(out, "{lines}")?;
                    }
                    Some(_) => {}
                }
            }
        }
        Ok((fail, total, skipped))
    })?;

    if let Some(line) = halt {
        writeln!(out, "{line}")?;
        return Ok(Exit(1));
    }
    if fail == Outcome::Failed {
        writeln!(
            out,
            "FAIL: at least one feature arm does not build -- a compiled arm that does not compile cannot be re-measured"
        )?;
        return Ok(Exit(1));
    }
    if skipped > 0 {
        let rev = rev.unwrap_or("");
        writeln!(
            out,
            "ok: the {total} feature arms a change since {rev} can reach build; {skipped} crates skipped"
        )?;
    } else {
        writeln!(out, "ok: all {total} feature arms build")?;
    }
    Ok(Exit(0))
}

/// The report of an arm that did not build: its name and the first two
/// lines of the build's output that start `error`.
pub fn broken(arm: &Arm, text: &str) -> String {
    let mut out = format!("  BROKEN  {}\n", arm.label());
    for l in text.lines().filter(|l| l.starts_with("error")).take(2) {
        out += &format!("      {l}\n");
    }
    out
}
