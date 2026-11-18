//! `cargo xtask test-scope`: the tests and gates a change needs, for the
//! push gate (`cargo xtask scoped-test`).
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
//!   crate (`features-since`'s list), the hooks' configuration, a file
//!   under `.config/` other than `.config/nextest.toml`, a changed line of
//!   `.config/nextest.toml` whose filter selects tests by more than a
//!   binary's or a package's name, and a scope that cannot be computed
//!   print it. On a tried tip whose tries name what they covered, all but
//!   a scope that cannot be computed give way to `tried` and the gates
//!   `full` stands for.
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
//! - A change to `.config/nextest.toml` plans, beside the tests that read
//!   it, each binary a `binary(<name>)` on a line it adds or removes names
//!   (a `binary` line for every member target of that name) and each member
//!   a `package(<name>)` there names (a `package` line). A changed line
//!   that sets a value and names no binary and no package adds nothing.
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
//! - `tried<TAB><reason>`, first: a try already ran `<to>`'s tree clean,
//!   by the list the file `BAUD_TRIED_TREES` names, one line a try: the
//!   tree's hash, its lane, its commit, its time, and after them, when the
//!   try names them, `covered=<gate>,<gate>,...`, the gates it ran. The
//!   lines that name `<to>`'s tree cover the union of their `covered=`
//!   gates. What follows `tried`:
//!   - a line without `covered=` among them, on a range that does not run
//!     every gate: nothing, so only the gates that build nothing run;
//!   - no line with `covered=`, on a range that runs every gate: no
//!     `tried` line, and `full`;
//!   - else every line the range plans whose gate no tried line covers, as
//!     this list prints it, with `full` standing for `clippy` of every
//!     member, `bare` and `linux`; the event rig, the whole suite and the
//!     publish verify are never planned there and print `left-out` lines.
//!
//!   A gate in `covered=` is `package:<member>` (which covers that
//!   member's `binary` lines too), `binary:<binary id>`,
//!   `clippy:<member>`, `doc:<member>`, `rig`, `bare`, `linux`, `gif`,
//!   `suite` (every `package`, `binary` and `doc` line, and the whole
//!   suite), `publish`, or one of `fmt`, `sources`, `no-python`, `paths`
//!   and `changelog`, which run on every push and change nothing here. A
//!   name that is none of these covers nothing.
//! - `left-out<TAB><gate><TAB><reason>`: on a tried tip, a gate the range
//!   would run that the tries did not cover and the push does not run:
//!   `rig`, `suite` (every member's tests and doc tests) and `publish`.
//! - `from-tried<TAB><tree> <lane>`, first, before the rest: the plan is
//!   over the paths that differ between `<to>` and a tried tree, named by
//!   its first twelve hex digits and its lane, in place of `<from>`. Of the
//!   listed trees this repository holds, it is the one whose difference
//!   touches the fewest paths (the later line among equals), and it is used
//!   when that difference touches fewer paths than `<from>`'s and does not
//!   run every gate.
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
/// the configuration under `.config/` but [`NEXTEST`], and the hooks'.
const FULL_PREFIXES: [&str; 2] = [".config/", ".pre-commit-config.yaml"];

/// The test runner's configuration: a change to it plans what the filters
/// on its changed lines name, and the tests that read it.
pub const NEXTEST: &str = ".config/nextest.toml";

/// What a filter on a changed line of [`NEXTEST`] names.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Named {
    /// A test binary, `binary(<name>)` or `binary(=<name>)`.
    Binary(String),
    /// A package, `package(<name>)` or `package(=<name>)`.
    Package(String),
    /// A selection whose tests the plan does not list, as written: a
    /// `binary` or `package` matcher that is a glob, a regular expression
    /// or a substring, any predicate but those two and `test`, or a `test`
    /// on a line that names no binary and no package.
    Unbounded(String),
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// What the filter predicates on the lines a diff of [`NEXTEST`] adds or
/// removes name, in the order they appear. Only the lines after the first
/// hunk header are read, so the file headers are not, and a comment line
/// names nothing.
pub fn named(diff: &str) -> Vec<Named> {
    let mut out = Vec::new();
    let lines = diff
        .lines()
        .skip_while(|l| !l.starts_with("@@"))
        .filter(|l| l.starts_with('+') || l.starts_with('-'))
        .map(|l| &l[1..])
        .filter(|l| !l.trim_start().starts_with('#'));
    for line in lines {
        let b = line.as_bytes();
        let (mut i, mut bounded, mut tests) = (0, false, false);
        while i < b.len() {
            if !b[i].is_ascii_lowercase() || (i > 0 && is_word(char::from(b[i - 1]))) {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && (b[i].is_ascii_lowercase() || b[i] == b'_') {
                i += 1;
            }
            if b.get(i) != Some(&b'(') {
                continue;
            }
            let pred = &line[start..i];
            let close = line[i..].find(')').map(|n| i + n);
            let written = &line[start..close.map_or(line.len(), |c| c + 1)];
            let arg = close.map(|c| line[i + 1..c].trim());
            i = close.map_or(line.len(), |c| c + 1);
            let name = arg.map(|a| a.strip_prefix('=').unwrap_or(a));
            let plain = name.filter(|n| !n.is_empty() && n.chars().all(is_word));
            out.push(match (pred, plain) {
                ("binary", Some(n)) => Named::Binary(n.to_string()),
                ("package", Some(n)) => Named::Package(n.to_string()),
                ("test", _) if arg.is_some() => {
                    tests = true;
                    continue;
                }
                _ => Named::Unbounded(written.to_string()),
            });
            bounded = true;
        }
        if tests && !bounded {
            out.push(Named::Unbounded(line.trim().to_string()));
        }
    }
    out
}

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

/// Whether a path of the member at `dir` is a test, a bench or an
/// example: compiled into that member's own binaries only, so nothing that
/// builds from the member is reached.
pub fn is_test_only(dir: &str, path: &str) -> bool {
    let rel = if dir.is_empty() {
        path
    } else {
        path.strip_prefix(&format!("{dir}/")).unwrap_or(path)
    };
    ["tests/", "benches/", "examples/"]
        .iter()
        .any(|d| rel.starts_with(d))
}

/// Whether a changed path is documentation.
pub fn is_doc(path: &str) -> bool {
    path.ends_with(".md") || path.starts_with("assets/readme/")
}

/// Whether a string literal in a source names `path`: the path itself
/// (leading `./` and `../` taken off, and the `/` before them of a literal
/// joined to a directory, as `"/../x"` is), a directory above it, a tail of
/// it that starts at a `/`, or, for a literal such as `".md"`, its
/// extension.
pub fn names(lit: &str, path: &str) -> bool {
    let mut l = lit;
    if l.starts_with("/../") || l.starts_with("/./") {
        l = &l[1..];
    }
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
    /// What the filters on the changed lines of [`NEXTEST`] name.
    pub named: &'a [Named],
}

struct Builder<'a> {
    i: &'a Inputs<'a>,
    plan: Plan,
    /// Packages reached as code, whose clippy and rig run.
    code: BTreeSet<String>,
    /// Packages whose tests, benches or examples alone changed: their clippy
    /// runs, and nothing that builds from them.
    own: BTreeSet<String>,
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

    /// Adds the binaries and the members the changed lines of [`NEXTEST`]
    /// name: every member target of a binary's name, every member of a
    /// package's. An error holding the selection as written when one
    /// selects tests by more than a binary's or a package's name.
    fn filters(&mut self) -> Result<(), String> {
        for n in self.i.named {
            match n {
                Named::Binary(name) => {
                    let why = format!("a changed line of {NEXTEST} names binary({name})");
                    for p in self.i.packages {
                        for t in p.targets.iter().filter(|t| &t.name == name) {
                            if let Some(id) = binary_id(&p.name, t) {
                                self.plan
                                    .binaries
                                    .entry((p.name.clone(), id))
                                    .or_insert_with(|| why.clone());
                            }
                        }
                    }
                }
                Named::Package(name) => {
                    if self.i.packages.iter().any(|p| &p.name == name) {
                        self.plan.packages.entry(name.clone()).or_insert_with(|| {
                            format!("a changed line of {NEXTEST} names package({name})")
                        });
                    }
                }
                Named::Unbounded(what) => return Err(what.clone()),
            }
        }
        Ok(())
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
        .find(|p| *p != NEXTEST && FULL_PREFIXES.iter().any(|f| p.starts_with(f)))
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
        own: BTreeSet::new(),
    };
    let (inside, outside): (Vec<String>, Vec<String>) = paths
        .iter()
        .cloned()
        .partition(|p| i.packages.iter().any(|k| under(p, &k.dir)));
    let (tests_only, code): (Vec<String>, Vec<String>) = inside
        .into_iter()
        .partition(|p| b.package_of(p).is_some_and(|k| is_test_only(&k.dir, p)));
    if !code.is_empty() {
        let Some(reached) = (i.reach)(&code) else {
            return Plan::everything("the reach could not be computed".to_string());
        };
        b.whole(&reached);
    }
    for p in &tests_only {
        if let Some(k) = b.package_of(p).map(|k| k.name.clone()) {
            b.binaries(p, &format!("changed: {p}"));
            b.own.insert(k);
        }
        if !b.outside(p) {
            return Plan::everything(format!("{p} reaches every crate"));
        }
    }
    for p in &outside {
        if p == NEXTEST {
            if let Err(what) = b.filters() {
                return Plan::everything(format!(
                    "a changed line of {p} selects {what}, which the plan does not list, so \
                     every gate runs"
                ));
            }
        }
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
        plan.clippy = b.code.union(&b.own).cloned().collect();
        if b.code.contains(RIG) {
            let why = format!("{RIG} is reached by {code}");
            plan.rig = Some(why.clone());
            plan.linux = Some(why);
        } else if b.own.contains(RIG) {
            plan.linux = Some(format!("{RIG}'s tests changed ({code})"));
            if plan
                .binaries
                .contains_key(&(RIG.to_string(), format!("{RIG}::event_rig")))
            {
                plan.rig = Some(format!("the rig's own test changed ({code})"));
            }
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

/// The diff of [`NEXTEST`] between `from` and `to`, or between `from` and
/// the working tree. An untracked file is all added lines.
fn nextest_diff(from: &str, to: Option<&str>) -> Result<String, String> {
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "-U0", from];
    if let Some(to) = to {
        args.push(to);
    }
    args.extend(["--", NEXTEST]);
    let diff = String::from_utf8_lossy(&git(&args)?).into_owned();
    if to.is_some() || !diff.is_empty() {
        return Ok(diff);
    }
    let Ok(text) = fs::read_to_string(NEXTEST) else {
        return Ok(diff);
    };
    let mut added = String::from("@@\n");
    for l in text.lines() {
        added.push('+');
        added.push_str(l);
        added.push('\n');
    }
    Ok(added)
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
    let computed = (|| -> Result<Scope, String> {
        let paths = changed(from, to)?;
        let packages = packages()?;
        let scan = Scan::load().ok_or("the workspace could not be scanned")?;
        let reach = |p: &[String]| scan.reach(p);
        let sources = sources(&packages);
        let plan_of = |base: &str, paths: &[String]| -> Result<Plan, String> {
            let named = if paths.iter().any(|p| p == NEXTEST) {
                named(&nextest_diff(base, to)?)
            } else {
                Vec::new()
            };
            Ok(plan(&Inputs {
                paths,
                reach: &reach,
                packages: &packages,
                sources: &sources,
                named: &named,
            }))
        };
        let whole = plan_of(from, &paths)?;
        let Some(to) = to else {
            return Ok(Scope::Plan(whole));
        };
        if let Some(t) = tried(to) {
            match (&t.covered, &whole.full) {
                (_, None) if t.plain == Plain::Yes => return Ok(Scope::Tried(t.why, Vec::new())),
                (None, Some(_)) => {}
                (Some(c), Some(why)) => {
                    let every = everything_of(why, &packages);
                    return Ok(Scope::Tried(t.why, uncovered(every, c, Range::Every)));
                }
                (c, None) => {
                    let c = c.clone().unwrap_or_default();
                    return Ok(Scope::Tried(t.why, uncovered(whole, &c, Range::Scoped)));
                }
            }
        }
        if whole.full.is_some() {
            return Ok(Scope::Plan(whole));
        }
        if let Some(n) = nearest(to, &paths) {
            let from_tree = plan_of(&n.hash, &n.paths)?;
            if from_tree.full.is_none() {
                return Ok(Scope::FromTried(n.tree, from_tree));
            }
        }
        Ok(Scope::Plan(whole))
    })();
    let plan = match computed {
        Ok(Scope::Tried(why, lines)) => {
            writeln!(out, "tried\t{why}")?;
            for line in lines {
                writeln!(out, "{line}")?;
            }
            return Ok(0);
        }
        Ok(Scope::FromTried(tree, p)) => {
            writeln!(out, "from-tried\t{tree}")?;
            p
        }
        Ok(Scope::Plan(p)) => p,
        Err(e) => Plan::everything(format!("the scope could not be computed ({e})")),
    };
    for line in plan.lines() {
        writeln!(out, "{line}")?;
    }
    Ok(0)
}

/// What the command prints for a scope it computed.
enum Scope {
    /// The plan over the changed paths.
    Plan(Plan),
    /// A try ran the tree clean: the reason, and the lines of the gates
    /// the tries did not cover.
    Tried(String, Vec<String>),
    /// The plan over the paths that differ from a tried tree, which is
    /// named by its first twelve hex digits and its lane.
    FromTried(String, Plan),
}

/// A tried tree and the paths that differ between it and the tip.
struct Nearest {
    /// The tree's first twelve hex digits and its lane.
    tree: String,
    /// The tree's hash.
    hash: String,
    /// The paths that differ.
    paths: Vec<String>,
}

/// The tried tree whose difference from `to` touches the fewest paths,
/// the later line of the file `BAUD_TRIED_TREES` names among equals, when
/// it touches fewer than `whole`. A line names a tree, its lane, its
/// commit and its time; a tree this repository does not hold is skipped.
fn nearest(to: &str, whole: &[String]) -> Option<Nearest> {
    let list = std::env::var_os("BAUD_TRIED_TREES")?;
    let text = fs::read_to_string(list).ok()?;
    let mut best: Option<Nearest> = None;
    for l in text.lines() {
        let mut fields = l.split_whitespace();
        let Some(tree) = fields.next() else {
            continue;
        };
        if !tree.bytes().all(|b| b.is_ascii_hexdigit()) || git(&["cat-file", "-e", tree]).is_err() {
            continue;
        }
        let Ok(paths) = changed(tree, Some(to)) else {
            continue;
        };
        if best.as_ref().is_none_or(|b| paths.len() <= b.paths.len()) {
            let lane = fields.next().unwrap_or("");
            let short = tree.get(..12).unwrap_or(tree);
            best = Some(Nearest {
                tree: format!("{short} {lane}"),
                hash: tree.to_string(),
                paths,
            });
        }
    }
    best.filter(|n| n.paths.len() < whole.len())
}

/// The lines of the file `BAUD_TRIED_TREES` names that start with a tree's
/// hash.
struct Tried {
    /// The tree and the first such line.
    why: String,
    /// Whether one of the lines carries no `covered=`.
    plain: Plain,
    /// The union of the lines' `covered=` gates, when one carries them.
    covered: Option<Covered>,
}

/// Whether a tried line among those of a tree carries no `covered=`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Plain {
    /// None does.
    No,
    /// One does.
    Yes,
}

/// What the range plans before the tried lines are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Range {
    /// Every gate, `full`.
    Every,
    /// The gates its paths reach.
    Scoped,
}

/// The gates the tries of a tree ran, by their `covered=` names.
#[derive(Clone, Default)]
struct Covered(BTreeSet<String>);

impl Covered {
    fn has(&self, gate: &str) -> bool {
        self.0.contains(gate)
    }

    /// Whether the tries ran the line `step` prints for `name`.
    fn line(&self, step: &str, name: &str) -> bool {
        self.has(&format!("{step}:{name}"))
    }
}

/// Why the event rig is not run on a tried tip.
const RIG_LEFT: &str = "the event rig opens windows and is run by hand, never on a tried tree";

/// Why the whole suite is not run on a tried tip.
const SUITE_LEFT: &str =
    "every member's tests and doc tests run on their own schedule, not on the push of a tried tree";

/// Why the publish verify is not run on a tried tip.
const PUBLISH_LEFT: &str = "the publish verify has no scoped step";

/// The gates `full` stands for that a push of a tried tree can run:
/// `clippy` of every member, `bare` and `linux`, for `why`; and the rig,
/// which [`uncovered`] leaves out.
fn everything_of(why: &str, packages: &[Package]) -> Plan {
    Plan {
        clippy: packages.iter().map(|p| p.name.clone()).collect(),
        rig: Some(why.to_string()),
        bare: Some(why.to_string()),
        linux: Some(why.to_string()),
        ..Plan::default()
    }
}

/// The lines of `plan` whose gate `c` does not cover, then a `left-out`
/// line for the rig when the plan names it and for the whole suite and the
/// publish verify when the range runs every gate, each unless `c` covers it.
fn uncovered(mut plan: Plan, c: &Covered, range: Range) -> Vec<String> {
    let every = range == Range::Every;
    let suite = c.has("suite");
    plan.packages
        .retain(|name, _| !suite && !c.line("package", name));
    plan.binaries
        .retain(|(pkg, id), _| !suite && !c.line("package", pkg) && !c.line("binary", id));
    plan.doc.retain(|name| !suite && !c.line("doc", name));
    plan.clippy.retain(|name| !c.line("clippy", name));
    let rig = plan.rig.take();
    for (gate, slot) in [
        ("bare", &mut plan.bare),
        ("linux", &mut plan.linux),
        ("gif", &mut plan.gif),
    ] {
        if c.has(gate) {
            *slot = None;
        }
    }
    let mut out = plan.lines();
    if every && !suite {
        out.push(format!("left-out\tsuite\t{SUITE_LEFT}"));
    }
    if rig.is_some() && !c.has("rig") {
        out.push(format!("left-out\trig\t{RIG_LEFT}"));
    }
    if every && !c.has("publish") {
        out.push(format!("left-out\tpublish\t{PUBLISH_LEFT}"));
    }
    out
}

/// Whether a try already ran `to`'s tree clean: a line of the file
/// `BAUD_TRIED_TREES` names starts with its tree hash. What those lines
/// cover, when one does.
fn tried(to: &str) -> Option<Tried> {
    let list = std::env::var_os("BAUD_TRIED_TREES")?;
    let text = fs::read_to_string(list).ok()?;
    let spec = format!("{to}^{{tree}}");
    let tree = git(&["rev-parse", "--verify", "--quiet", &spec]).ok()?;
    let tree = std::str::from_utf8(&tree).ok()?.trim().to_string();
    if tree.is_empty() {
        return None;
    }
    let mut found: Option<Tried> = None;
    for l in text.lines() {
        let mut fields = l.split_whitespace();
        if fields.next() != Some(tree.as_str()) {
            continue;
        }
        let gates = fields.find_map(|f| f.strip_prefix("covered="));
        let t = found.get_or_insert_with(|| Tried {
            why: format!("tree {tree} was tried clean: {l}"),
            plain: Plain::No,
            covered: None,
        });
        match gates {
            None => t.plain = Plain::Yes,
            Some(g) => {
                let c = t.covered.get_or_insert_with(Covered::default);
                c.0.extend(g.split(',').filter(|n| !n.is_empty()).map(str::to_string));
            }
        }
    }
    found
}
