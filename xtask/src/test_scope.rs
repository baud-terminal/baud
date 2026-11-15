//! `cargo xtask test-scope`: the tests and gates a change needs, for the
//! push gate (`scripts/scoped-test.sh`).
//!
//! ```text
//! cargo xtask test-scope <from> <to>
//! cargo xtask test-scope <rev>
//! ```
//!
//! The changed paths are those that differ between `<from>` and `<to>`, or
//! between `<rev>` and the working tree, untracked files git does not
//! ignore included. It prints one tab-separated line per step:
//!
//! - `full<TAB><reason>`, alone: every gate runs. A path that reaches every
//!   crate (`features-since`'s list), the test runner's or the hooks'
//!   configuration, and a scope that cannot be computed print it.
//! - `package<TAB><name><TAB><reason>`: every test of the package and its
//!   doc tests. A changed path in a member reaches that member and every
//!   member that builds from it (`features-since`'s reach); so does a file
//!   outside the members that a library other members build from reads at
//!   compile time.
//! - `binary<TAB><package><TAB><binary id><TAB><reason>`: one test binary,
//!   for any other file outside the members. The binary reads it: its
//!   source names the file in a string literal (the path, a directory above
//!   it, a tail of it from a `/`, or its extension, as a walker of `.md`
//!   files names `".md"`), or includes a source that does. A source that
//!   belongs to no test binary names its whole package.
//! - `doc<TAB><name>`: the doc tests of each package a `package` line
//!   names that has a library.
//! - `clippy<TAB><name>`: the clippy of each package a `package` line
//!   names, unless every changed path is documentation.
//! - `rig`, `bare` and `linux`, each with its reason: the event rig and the
//!   Linux clippy when a `package` line names `baud`, the bare-metal builds
//!   when one names `baud-core` or `baud-font`, unless every changed path is
//!   documentation.
//! - `gif<TAB><reason>`: the README's GIF or its provenance changed, so its
//!   flash census and `--verify` run.
//!
//! The gates that build nothing (formatting, sources, paths, the
//! changelog, no Python) are the runner's to run every time. A file that no
//! member holds and no source names is read by no test and adds nothing.
//! Documentation is a `.md` file or anything under `assets/readme/`.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::process::Command;

use serde_json::Value;

use crate::features_since::{Scan, everything, lex, walk};

/// Paths whose change reaches every gate, on top of `features-since`'s:
/// the test runner's configuration, the hooks', and this scope's own code.
const FULL_PREFIXES: [&str; 5] = [
    ".config/",
    ".pre-commit-config.yaml",
    "scripts/pre-push.sh",
    "scripts/scoped-test.sh",
    "xtask/src/test_scope.rs",
];

/// The package whose code the event rig and the Linux clippy build.
const RIG: &str = "baud";
/// The packages built for a bare-metal target.
const BARE: [&str; 2] = ["baud-core", "baud-font"];
/// The README's GIF and its provenance.
const GIF: [&str; 2] = [
    "assets/readme/session.gif",
    "assets/readme/session.gif.provenance.toml",
];

/// One target of a package.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Target {
    /// Its name.
    pub name: String,
    /// Its first kind: `lib`, `bin`, `test`, `example`, `bench`,
    /// `custom-build` or another library kind.
    pub kind: String,
    /// Its root source, relative to the workspace root.
    pub src: String,
}

/// A workspace member, its targets, and whether a member builds from it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Package {
    /// Its name.
    pub name: String,
    /// Its directory, relative to the workspace root.
    pub dir: String,
    /// Its targets.
    pub targets: Vec<Target>,
    /// Whether another member depends on it.
    pub depended_on: Depended,
}

/// Whether another member depends on a package.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depended {
    /// No member does.
    No,
    /// One does, so what its library compiles in reaches that member too.
    Yes,
}

/// What a change reaches, from `features-since`'s scan: `None` when every
/// crate is.
pub type Reach<'a> = &'a dyn Fn(&[String]) -> Option<Vec<(String, String, String)>>;

/// What a change needs, step by step.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Plan {
    /// Every gate, and why.
    pub full: Option<String>,
    /// Packages tested whole, by name, with the reason.
    pub packages: BTreeMap<String, String>,
    /// Single binaries, `(package, binary id)`, with the reason.
    pub binaries: BTreeMap<(String, String), String>,
    /// Packages whose clippy runs.
    pub clippy: BTreeSet<String>,
    /// Packages tested whole that have a library, whose doc tests run.
    pub doc: BTreeSet<String>,
    /// The event rig, and why.
    pub rig: Option<String>,
    /// The bare-metal builds, and why.
    pub bare: Option<String>,
    /// The Linux clippy, and why.
    pub linux: Option<String>,
    /// The GIF's census and verify, and why.
    pub gif: Option<String>,
}

impl Plan {
    fn everything(why: String) -> Self {
        Plan {
            full: Some(why),
            ..Plan::default()
        }
    }

    /// The plan as the lines `test-scope` prints.
    pub fn lines(&self) -> Vec<String> {
        if let Some(why) = &self.full {
            return vec![format!("full\t{why}")];
        }
        let mut out = Vec::new();
        for (name, why) in &self.packages {
            out.push(format!("package\t{name}\t{why}"));
        }
        for ((pkg, id), why) in &self.binaries {
            out.push(format!("binary\t{pkg}\t{id}\t{why}"));
        }
        for name in &self.clippy {
            out.push(format!("clippy\t{name}"));
        }
        for name in &self.doc {
            out.push(format!("doc\t{name}"));
        }
        for (step, why) in [
            ("rig", &self.rig),
            ("bare", &self.bare),
            ("linux", &self.linux),
            ("gif", &self.gif),
        ] {
            if let Some(why) = why {
                out.push(format!("{step}\t{why}"));
            }
        }
        out
    }
}

/// Whether a changed path is documentation.
pub fn is_doc(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("assets/readme/")
}

/// Whether a string literal in a source names `path`: the path itself
/// (leading `./` and `../` taken off), a directory above it, a tail of it
/// that starts at a `/`, or, for a literal such as `".md"`, its extension.
pub fn names(lit: &str, path: &str) -> bool {
    let mut l = lit;
    loop {
        if let Some(r) = l.strip_prefix("../") {
            l = r;
        } else if let Some(r) = l.strip_prefix("./") {
            l = r;
        } else {
            break;
        }
    }
    let l = l.trim_end_matches('/');
    if l.is_empty() || l == "." || l == ".." || l.chars().any(|c| c.is_whitespace() || c == '{') {
        return false;
    }
    if l.starts_with('.') && !l.contains('/') && l.len() >= 3 {
        return path.ends_with(l);
    }
    path == l || path.starts_with(&format!("{l}/")) || path.ends_with(&format!("/{l}"))
}

fn under(path: &str, dir: &str) -> bool {
    dir.is_empty() || path.starts_with(&format!("{dir}/"))
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The nextest binary id of a target, or `None` for one nextest does not
/// run (a build script, a bench).
pub fn binary_id(pkg: &str, t: &Target) -> Option<String> {
    match t.kind.as_str() {
        "bin" => Some(format!("{pkg}::bin/{}", t.name)),
        "test" => Some(format!("{pkg}::{}", t.name)),
        "example" => Some(format!("{pkg}::example/{}", t.name)),
        "bench" | "custom-build" => None,
        _ => Some(pkg.to_string()),
    }
}

/// The test binaries a source file of `p` is compiled into: the target it
/// is the root of, or else those whose root's directory is the deepest one
/// above it (none for a bench's or a build script's). `None` when it is
/// under no target's directory, so the package is tested whole.
pub fn binaries_of(p: &Package, file: &str) -> Option<Vec<String>> {
    if let Some(t) = p.targets.iter().find(|t| t.src == file) {
        return Some(binary_id(&p.name, t).into_iter().collect());
    }
    let depth = p
        .targets
        .iter()
        .map(|t| parent(&t.src))
        .filter(|d| under(file, d) && *d != p.dir)
        .map(str::len)
        .max()?;
    Some(
        p.targets
            .iter()
            .filter(|t| parent(&t.src).len() == depth && under(file, parent(&t.src)))
            .filter_map(|t| binary_id(&p.name, t))
            .collect(),
    )
}

/// The file a `features-since` reason says reads a path at compile time.
fn reader_in(why: &str) -> Option<&str> {
    let (_, rest) = why.split_once(" at compile time, in ")?;
    Some(rest.split_once(" (").map_or(rest, |(f, _)| f))
}

/// What the plan is built from.
pub struct Inputs<'a> {
    /// The changed paths.
    pub paths: &'a [String],
    /// What a change reaches.
    pub reach: Reach<'a>,
    /// The members.
    pub packages: &'a [Package],
    /// Each member source file with its string literals.
    pub sources: &'a [(String, Vec<String>)],
}

struct Builder<'a> {
    i: &'a Inputs<'a>,
    plan: Plan,
    /// Packages reached as code, whose clippy and rig run.
    code: BTreeSet<String>,
}

impl Builder<'_> {
    fn package_of(&self, file: &str) -> Option<&Package> {
        self.i
            .packages
            .iter()
            .filter(|k| under(file, &k.dir))
            .max_by_key(|k| k.dir.len())
    }

    fn whole(&mut self, reached: &[(String, String, String)]) {
        for (_, name, why) in reached {
            self.plan
                .packages
                .entry(name.clone())
                .or_insert_with(|| why.clone());
            self.code.insert(name.clone());
        }
    }

    /// Adds the binaries `file` is compiled into, and those of the sources
    /// that include it by name, for `why`.
    fn binaries(&mut self, file: &str, why: &str) {
        let mut files = vec![file.to_string()];
        for (g, lits) in self.i.sources {
            if g != file && lits.iter().any(|l| names(l, file)) {
                files.push(g.clone());
            }
        }
        for f in files {
            let Some(pkg) = self.package_of(&f).cloned() else {
                continue;
            };
            match binaries_of(&pkg, &f) {
                Some(ids) => {
                    for id in ids {
                        self.plan
                            .binaries
                            .entry((pkg.name.clone(), id))
                            .or_insert_with(|| why.to_string());
                    }
                }
                None => {
                    self.plan
                        .packages
                        .entry(pkg.name.clone())
                        .or_insert_with(|| why.to_string());
                }
            }
        }
    }

    /// A file outside the members: the whole reach when a library other
    /// members build from reads it at compile time, else the binaries that
    /// read it at compile time or name it. `false` when it reaches every
    /// crate.
    fn outside(&mut self, path: &str) -> bool {
        let Some(reached) = (self.i.reach)(&[path.to_string()]) else {
            return false;
        };
        let readers: Vec<String> = reached
            .iter()
            .filter_map(|(_, _, why)| reader_in(why).map(str::to_string))
            .collect();
        let lib = readers.iter().any(|f| {
            self.package_of(f).is_some_and(|p| {
                p.depended_on == Depended::Yes
                    && binaries_of(p, f).is_none_or(|ids| ids.contains(&p.name))
            })
        });
        if lib {
            self.whole(&reached);
            return true;
        }
        for f in &readers {
            self.binaries(f, &format!("{f} reads {path} at compile time"));
        }
        let namers: Vec<String> = self
            .i
            .sources
            .iter()
            .filter(|(_, lits)| lits.iter().any(|l| names(l, path)))
            .map(|(f, _)| f.clone())
            .collect();
        for f in namers {
            if let Some(pkg) = self.package_of(&f).cloned() {
                let why = format!("{f} names {path}");
                match binaries_of(&pkg, &f) {
                    Some(ids) => {
                        for id in ids {
                            self.plan
                                .binaries
                                .entry((pkg.name.clone(), id))
                                .or_insert_with(|| why.clone());
                        }
                    }
                    None => {
                        self.plan.packages.entry(pkg.name.clone()).or_insert(why);
                    }
                }
            }
        }
        true
    }
}

/// The plan for a change.
pub fn plan(i: &Inputs) -> Plan {
    let paths = i.paths;
    if let Some(p) = paths
        .iter()
        .find(|p| FULL_PREFIXES.iter().any(|f| p.starts_with(f)))
    {
        return Plan::everything(format!("{p} changed, so every gate runs"));
    }
    if let Some(p) = everything(paths) {
        return Plan::everything(format!("{p} changed, so every crate is reached"));
    }
    let mut b = Builder {
        i,
        plan: Plan::default(),
        code: BTreeSet::new(),
    };
    let (inside, outside): (Vec<String>, Vec<String>) = paths
        .iter()
        .cloned()
        .partition(|p| i.packages.iter().any(|k| under(p, &k.dir)));
    if !inside.is_empty() {
        let Some(reached) = (i.reach)(&inside) else {
            return Plan::everything("the reach could not be computed".to_string());
        };
        b.whole(&reached);
    }
    for p in &outside {
        if !b.outside(p) {
            return Plan::everything(format!("{p} reaches every crate"));
        }
    }
    let mut plan = b.plan;
    plan.binaries
        .retain(|(pkg, _), _| !plan.packages.contains_key(pkg));
    for p in i.packages {
        if plan.packages.contains_key(&p.name)
            && p.targets
                .iter()
                .any(|t| binary_id(&p.name, t).as_deref() == Some(p.name.as_str()))
        {
            plan.doc.insert(p.name.clone());
        }
    }
    if let Some(code) = paths.iter().find(|p| !is_doc(p)) {
        plan.clippy = b.code.clone();
        if b.code.contains(RIG) {
            let why = format!("{RIG} is reached by {code}");
            plan.rig = Some(why.clone());
            plan.linux = Some(why);
        }
        if let Some(name) = BARE.iter().find(|n| b.code.contains(**n)) {
            plan.bare = Some(format!("{name} is reached by {code}"));
        }
    }
    if let Some(g) = paths.iter().find(|p| GIF.contains(&p.as_str())) {
        plan.gif = Some(format!("{g} changed"));
    }
    plan
}

fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

fn split_z(raw: &[u8], into: &mut BTreeSet<String>) -> Result<(), String> {
    for p in raw.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let p = std::str::from_utf8(p).map_err(|_| "a changed path is not UTF-8".to_string())?;
        into.insert(p.to_string());
    }
    Ok(())
}

/// The changed paths: between two commits, or between one and the working
/// tree with untracked files.
pub fn changed(from: &str, to: Option<&str>) -> Result<Vec<String>, String> {
    let mut paths = BTreeSet::new();
    let mut args = vec!["diff", "--name-only", "--no-renames", "-z", from];
    if let Some(to) = to {
        args.push(to);
    }
    args.push("--");
    split_z(&git(&args)?, &mut paths)?;
    if to.is_none() {
        split_z(
            &git(&["ls-files", "--others", "--exclude-standard", "-z"])?,
            &mut paths,
        )?;
    }
    Ok(paths.into_iter().collect())
}

fn text_of<'a>(v: &'a Value, k: &str) -> Result<&'a str, String> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("cargo metadata: no {k}"))
}

fn list_of<'a>(v: &'a Value, k: &str) -> Result<&'a Vec<Value>, String> {
    v.get(k)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("cargo metadata: no {k}"))
}

/// The members and their targets, from `cargo metadata` run here, the
/// workspace root.
pub fn packages() -> Result<Vec<Package>, String> {
    let out = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err("cargo metadata failed".to_string());
    }
    let meta: Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let root = format!("{}/", text_of(&meta, "workspace_root")?);
    let rel = |p: &str| p.strip_prefix(&root).unwrap_or(p).to_string();
    let raw = list_of(&meta, "packages")?;
    let mut used = BTreeSet::new();
    for p in raw {
        for d in list_of(p, "dependencies")? {
            if d.get("path").and_then(Value::as_str).is_some() {
                used.insert(text_of(d, "name")?.to_string());
            }
        }
    }
    let mut out = Vec::new();
    for p in raw {
        let manifest = rel(text_of(p, "manifest_path")?);
        let mut targets = Vec::new();
        for t in list_of(p, "targets")? {
            let kind = list_of(t, "kind")?
                .first()
                .and_then(Value::as_str)
                .ok_or("cargo metadata: a target without a kind")?;
            targets.push(Target {
                name: text_of(t, "name")?.to_string(),
                kind: kind.to_string(),
                src: rel(text_of(t, "src_path")?),
            });
        }
        let name = text_of(p, "name")?.to_string();
        out.push(Package {
            depended_on: if used.contains(&name) {
                Depended::Yes
            } else {
                Depended::No
            },
            name,
            dir: parent(&manifest).to_string(),
            targets,
        });
    }
    Ok(out)
}

/// Every Rust source under the members, with its string literals.
pub fn sources(packages: &[Package]) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for p in packages {
        for f in walk(&p.dir) {
            let f = f.strip_prefix("./").unwrap_or(&f).to_string();
            if !f.ends_with(".rs") || !seen.insert(f.clone()) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&f) {
                out.push((f, lex(&text).lits));
            }
        }
    }
    out
}

/// The command: prints the plan, exit 0; a bad command line, exit 2.
pub fn run(args: &[&str], out: &mut impl Write) -> io::Result<u8> {
    let (from, to) = match args {
        [from, to] => (*from, Some(*to)),
        [rev] => (*rev, None),
        _ => {
            writeln!(out, "usage: cargo xtask test-scope <from> <to> | <rev>")?;
            return Ok(2);
        }
    };
    let computed = (|| -> Result<Plan, String> {
        let paths = changed(from, to)?;
        let packages = packages()?;
        let scan = Scan::load().ok_or("the workspace could not be scanned")?;
        let reach = |p: &[String]| scan.reach(p);
        let sources = sources(&packages);
        Ok(plan(&Inputs {
            paths: &paths,
            reach: &reach,
            packages: &packages,
            sources: &sources,
        }))
    })();
    let plan = computed
        .unwrap_or_else(|e| Plan::everything(format!("the scope could not be computed ({e})")));
    for line in plan.lines() {
        writeln!(out, "{line}")?;
    }
    Ok(0)
}
