//! `cargo xtask scoped-test <from> <to>`: the gates a change from `<from>`
//! to `<to>` needs, run in the dev shell. `cargo xtask test-scope` names
//! them; this runs each, and exits 1 when any fails, 3 when the scope names
//! every gate (the caller then runs `devenv test`), 0 otherwise.
//!
//! Every run: formatting, sources, no Python, paths and the changelog,
//! which build nothing. A tree a try already ran clean (`tried`) stops
//! there when the scope names nothing after it, and runs what it names
//! otherwise; a `left-out` line runs nothing. Then, as the scope names
//! them: clippy of the packages a code
//! change reaches; the tests of whole packages and single binaries, in one
//! nextest run, and their doc tests, under the `release-probe` profile;
//! the feature arms of the crates the change reaches and the raw-integer
//! audit; the event rig, on macOS; the bare-metal builds; the Linux
//! clippy; and the README GIF's flash census and `--verify`.
//!
//! Each step prints `scoped-test: <name>`, runs with this process's
//! output, and prints `scoped-test: <name> passed (<n> s)` or
//! `scoped-test: <name> FAILED (<n> s)`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::cmd::Cmd;
use crate::features_since::Exit;
use crate::test_scope;
use crate::third_party::Outcome;

/// Whole seconds on a clock that only moves forward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Secs(pub u64);

/// The system the gates run on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Os {
    /// macOS, where the event rig runs.
    Darwin,
    /// Any other.
    Other,
}

impl Os {
    /// The system this program was built for.
    pub fn this() -> Os {
        if cfg!(target_os = "macos") {
            Os::Darwin
        } else {
            Os::Other
        }
    }
}

/// What the gates are run by.
pub trait Gates {
    /// `test-scope <from> <to>`'s lines, or `None` when it fails.
    fn plan(&self, from: &str, to: &str) -> Option<String>;
    /// Runs `cmd` with this process's output: whether it passed.
    fn run(&self, cmd: &Cmd) -> Outcome;
    /// The clock the steps are timed on.
    fn clock(&self) -> Secs;
}

/// The gates of the repository at `root`, timed from `start`.
pub struct Shell {
    /// The repository's root.
    pub root: PathBuf,
    /// When the run began.
    pub start: Instant,
}

impl Gates for Shell {
    fn plan(&self, from: &str, to: &str) -> Option<String> {
        let mut out = Vec::new();
        match test_scope::run(&[from, to], &mut out) {
            Ok(0) => Some(String::from_utf8_lossy(&out).into_owned()),
            _ => None,
        }
    }

    fn run(&self, cmd: &Cmd) -> Outcome {
        cmd.status(&self.root)
    }

    fn clock(&self) -> Secs {
        Secs(self.start.elapsed().as_secs())
    }
}

/// The steps run so far that failed, by name.
struct Steps<'a> {
    gates: &'a dyn Gates,
    failed: Vec<String>,
}

impl Steps<'_> {
    fn step(&mut self, out: &mut dyn Write, name: &str, cmd: &Cmd) -> io::Result<()> {
        let Secs(t0) = self.gates.clock();
        writeln!(out, "scoped-test: {name}")?;
        out.flush()?;
        let ok = self.gates.run(cmd);
        let Secs(t1) = self.gates.clock();
        let secs = t1.saturating_sub(t0);
        match ok {
            Outcome::Passed => writeln!(out, "scoped-test: {name} passed ({secs} s)")?,
            Outcome::Failed => {
                writeln!(out, "scoped-test: {name} FAILED ({secs} s)")?;
                self.failed.push(name.to_string());
            }
        }
        out.flush()
    }

    /// The last line and the exit, once every step has run.
    fn end(&self, out: &mut dyn Write, passed: &str) -> io::Result<Exit> {
        if self.failed.is_empty() {
            writeln!(out, "scoped-test: {passed}")?;
            return Ok(Exit(0));
        }
        writeln!(out, "scoped-test: FAILED: {}", self.failed.join(" "))?;
        Ok(Exit(1))
    }
}

/// The plan's lines, each split at its tabs.
struct Plan<'a>(Vec<Vec<&'a str>>);

impl<'a> Plan<'a> {
    /// Whether a line's first field is `step`.
    fn has(&self, step: &str) -> bool {
        self.0.iter().any(|f| f.first() == Some(&step))
    }

    /// The `n`th fields, from 0, of the lines whose first field is `step`,
    /// each split at its spaces.
    fn words(&self, step: &str, n: Field) -> Vec<&'a str> {
        let Field(n) = n;
        self.0
            .iter()
            .filter(|f| f.first() == Some(&step))
            .filter_map(|f| f.get(n).copied())
            .flat_map(str::split_whitespace)
            .collect()
    }
}

/// A field of a plan's line, from 0.
#[derive(Clone, Copy)]
struct Field(usize);

/// The steps of a plan's lines that run a gate after the ones that build
/// nothing.
const GATES: [&str; 8] = [
    "package", "binary", "clippy", "doc", "rig", "bare", "linux", "gif",
];

/// The bare-metal builds.
const BARE: &str = "cargo build --release --quiet -p baud-core --target x86_64-unknown-none && cargo build --release --quiet -p baud-font --target x86_64-unknown-none";

/// The command: `args` are what follows its name, `root` the repository's
/// root as an absolute path.
pub fn run(
    args: &[&str],
    root: &Path,
    os: Os,
    gates: &dyn Gates,
    out: &mut dyn Write,
) -> io::Result<Exit> {
    let [from, to] = args else {
        writeln!(out, "usage: cargo xtask scoped-test <from> <to>")?;
        return Ok(Exit(2));
    };
    let Some(text) = gates.plan(from, to) else {
        writeln!(out, "scoped-test: cargo xtask test-scope failed")?;
        return Ok(Exit(1));
    };
    let text = text.trim_end_matches('\n');
    writeln!(out, "scoped-test: {from}..{to}")?;
    if text.is_empty() {
        writeln!(out, "  (no test reads what changed)")?;
    } else {
        for line in text.split('\n') {
            writeln!(out, "  {line}")?;
        }
    }
    let lines: Vec<&str> = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    };
    if lines.iter().any(|l| l.starts_with("full")) {
        return Ok(Exit(3));
    }
    let tried = lines.iter().any(|l| l.starts_with("tried"));
    let plan = Plan(lines.iter().map(|l| l.split('\t').collect()).collect());

    let mut s = Steps {
        gates,
        failed: Vec::new(),
    };
    s.step(out, "fmt", &Cmd::new(&["treefmt", "--fail-on-change"]))?;
    s.step(out, "sources", &Cmd::new(&["cargo", "xtask", "sources"]))?;
    s.step(
        out,
        "no-python",
        &Cmd::new(&["cargo", "xtask", "no-python"]),
    )?;
    s.step(out, "paths", &Cmd::new(&["cargo", "xtask", "paths"]))?;
    s.step(
        out,
        "changelog",
        &Cmd::new(&["cargo", "xtask", "changelog"]),
    )?;
    if tried && !GATES.iter().any(|g| plan.has(g)) {
        return s.end(
            out,
            "tried clean, so no cargo gate runs; every gate that builds nothing passed",
        );
    }

    let clippy = plan.words("clippy", Field(1));
    if !clippy.is_empty() {
        let mut cmd = Cmd::new(&["cargo", "clippy", "--quiet"]);
        for p in &clippy {
            cmd = cmd.arg("-p").arg(*p);
        }
        let cmd = cmd.arg("--all-targets").arg("--").arg("-D").arg("warnings");
        s.step(out, "clippy", &cmd)?;
    }

    let packages = plan.words("package", Field(1));
    let binaries: Vec<(&str, &str)> = plan
        .0
        .iter()
        .filter(|f| f.first() == Some(&"binary"))
        .map(|f| {
            let at = |n| f.get(n).copied().unwrap_or("");
            (at(1), at(2))
        })
        .collect();
    if !packages.is_empty() || !binaries.is_empty() {
        let mut cmd = Cmd::new(&[
            "cargo",
            "nextest",
            "run",
            "--cargo-profile",
            "release-probe",
            "--cargo-quiet",
        ]);
        let mut filter: Vec<String> = Vec::new();
        for p in &packages {
            cmd = cmd.arg("-p").arg(*p);
            filter.push(format!("package({p})"));
        }
        for (p, id) in binaries.iter().filter(|(p, _)| !p.is_empty()) {
            cmd = cmd.arg("-p").arg(*p);
            filter.push(format!("binary_id({id})"));
        }
        let cmd = cmd.arg("-E").arg(filter.join(" | ")).arg("--no-fail-fast");
        s.step(out, "tests", &cmd)?;
    }

    let doc = plan.words("doc", Field(1));
    if !doc.is_empty() {
        let mut cmd = Cmd::new(&[
            "cargo",
            "test",
            "--profile",
            "release-probe",
            "--quiet",
            "--doc",
        ]);
        for p in &doc {
            cmd = cmd.arg("-p").arg(*p);
        }
        s.step(out, "doctests", &cmd)?;
    }

    if !clippy.is_empty() {
        let features = Cmd::new(&["cargo", "xtask", "feature-arms", "--since", *from]);
        s.step(out, "features", &features)?;
        s.step(out, "audit", &Cmd::new(&["sh", "./audit_all.sh"]))?;
    }

    if plan.has("rig") && os == Os::Darwin {
        let rig = Cmd::new(&[
            "cargo",
            "nextest",
            "run",
            "--cargo-profile",
            "release-probe",
            "--cargo-quiet",
            "-p",
            "baud",
            "--features",
            "effect-probe",
            "--test",
            "event_rig",
        ])
        .var("RUSTFLAGS", "-D warnings");
        s.step(out, "rig", &rig)?;
    }
    if plan.has("bare") {
        s.step(out, "bare", &Cmd::new(&["sh", "-c", BARE]))?;
    }
    if plan.has("linux") {
        let linux = Cmd::new(&[
            "cargo",
            "clippy",
            "--quiet",
            "-p",
            "baud",
            "--target",
            "aarch64-unknown-linux-gnu",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ]);
        s.step(out, "linux", &linux)?;
    }
    if plan.has("gif") {
        let gif = root.join("assets/readme/session.gif");
        let flashes = Cmd::new(&[
            "cargo",
            "test",
            "--quiet",
            "-p",
            "baud-bench",
            "--test",
            "trailer_flashes",
            "--",
            "the_trailer_is_below_the_flash_thresholds",
            "--ignored",
            "--exact",
        ])
        .var("BAUD_TRAILER_GIF", &gif.to_string_lossy());
        s.step(out, "gif-flashes", &flashes)?;
        let verify = Cmd::new(&[
            "cargo",
            "run",
            "--quiet",
            "--example",
            "demo",
            "--",
            "--verify",
            "assets/readme/session.gif",
        ]);
        s.step(out, "gif-verify", &verify)?;
    }

    s.end(out, "every scoped gate passed")
}
