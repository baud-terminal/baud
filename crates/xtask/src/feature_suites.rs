//! `cargo xtask feature-suites`: baud-core's suite under its feature arms.
//!
//! `cargo xtask feature-arms` builds every arm alone and runs none of them;
//! an arm that compiles can still be wrong. This runs baud-core's suite
//! through nextest (`--release`, `RUSTFLAGS="-D warnings"`) under each arm.
//!
//! **Every declared baud-core feature is in exactly one class, and the
//! classes are the gate.** A feature in none, or in two, fails before
//! anything builds, and the declared count must equal [`FLOOR`]:
//!
//! - [`BUILDS`]: one build a line, its arms joined by commas. Arms share a
//!   build only where none hides another's code from the suite.
//! - [`TOGETHER`]: additive probes, each running a second copy of some
//!   work into `black_box` or a scratch and leaving every counter and the
//!   screen as they were, in one build: a probe that is not additive turns
//!   the run red, and none removes work, so none can undo another's change.
//! - [`OFF_ARM64`]: built alone off aarch64 and skipped, with the reason
//!   printed, on aarch64, where `neon-scan` replaces the scan it reroutes
//!   and baud-core's dev-dependency on itself keeps `neon-scan` on.
//! - [`DEFAULT_OFF`]: default features the dev-dependency does not name,
//!   run off together in one build without the defaults
//!   (`--no-default-features --features neon-scan`).
//! - [`EXCLUDED`]: not run, each with its reason: on in every test build,
//!   covered by another gate, wrong by construction (naming a test it turns
//!   red), or not additive.
//!
//! No doc tests run: baud-core's doc fences are `text`. A run of an
//! excluded arm by hand fails its properties and appends their seeds to
//! the checked-in `.proptest-regressions` files.

use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cmd::Cmd;
use crate::feature_arms::{Floor, declared};
use crate::features_since::Exit;
use crate::third_party::Outcome;

/// baud-core's manifest, from the repository's root.
pub const MANIFEST: &str = "crates/baud-core/Cargo.toml";

/// The features the manifest declares.
pub const FLOOR: Floor = Floor(55);

/// One build a line, its arms joined by commas.
pub const BUILDS: [&str; 5] = [
    "two-pass-csi,probe-double-final-scan",
    "cell-table",
    "sgr-table",
    "diag",
    "param3,probe-no-fast-csi,colour-in-match",
];

/// The additive probes, built together.
pub const TOGETHER: [&str; 18] = [
    "probe-double-break",
    "probe-double-attach",
    "probe-double-classify",
    "probe-double-extent",
    "probe-double-decode",
    "probe-double-scan",
    "probe-warm-scroll",
    "probe-double-params",
    "probe-csi-scratch",
    "probe-double-text-scan",
    "probe-double-sgr",
    "probe-sgr-copy",
    "probe-double-fold",
    "probe-double-sgr-color",
    "probe-sgr-color-reborrow",
    "probe-double-intern",
    "probe-double-parse",
    "probe-double-put",
];

/// Built alone off aarch64, skipped on it.
pub const OFF_ARM64: [&str; 1] = ["route-stride"];

/// Default features run off together.
pub const DEFAULT_OFF: [&str; 1] = ["read-record"];

/// Not run: each name and why.
pub const EXCLUDED: [(&str, &str); 27] = [
    (
        "copy-counts",
        "covered by another gate: baud-present-wayland's copies_per_mb test builds baud-core with it and reads the counts",
    ),
    (
        "alloc",
        "on in every test build: the dev-dependency on baud-core itself enables it",
    ),
    (
        "record-test",
        "on in every test build: the dev-dependency on baud-core itself enables it",
    ),
    (
        "write-gen-probe",
        "on in every test build: the dev-dependency on baud-core itself enables it",
    ),
    ("neon-scan", "the default feature, so test:unit runs it"),
    (
        "avx2-scan",
        "inert off x86-64; test:x86-avx2 runs the workspace suite with it and +avx2",
    ),
    (
        "ceiling-no-write",
        "wrong by construction: the_cell_table_is_what_the_write_path_writes",
    ),
    (
        "ceiling-no-decode",
        "wrong by construction: the_cell_table_is_what_the_write_path_writes",
    ),
    (
        "ceiling-no-classify",
        "wrong by construction: every_line_of_the_suite_clusters_through_the_terminal",
    ),
    (
        "ceiling-no-run-gate",
        "wrong by construction: a_flag_is_one_cluster",
    ),
    (
        "ceiling-no-blank",
        "wrong by construction: a_row_reused_by_a_scroll_while_narrow_comes_back_clean",
    ),
    (
        "ceiling-no-scroll",
        "wrong by construction: a_taller_primary_screen_pulls_the_scrolled_lines_back_above_it",
    ),
    (
        "ceiling-no-store",
        "wrong by construction: the_cell_table_is_what_the_write_path_writes",
    ),
    (
        "ceiling-no-act",
        "wrong by construction, and three maximal_subparts tests do not finish under it",
    ),
    (
        "ceiling-no-text",
        "wrong by construction: every_recording_renders_to_its_pinned_digest",
    ),
    (
        "ceiling-implicit-tail",
        "wrong by construction: a_control_function_ends_the_cluster_before_it",
    ),
    (
        "ceiling-free-sgr",
        "wrong by construction: every_recording_renders_to_its_pinned_digest",
    ),
    (
        "ceiling-no-region-copy",
        "wrong by construction: a_copying_region_scroll_carries_a_hidden_tail",
    ),
    (
        "ceiling-no-region-repair",
        "wrong by construction: a_top_anchored_region_retires_its_line_whatever_its_height",
    ),
    (
        "ceiling-segment-cache",
        "wrong by construction: a_copy_is_the_same_at_every_read_size",
    ),
    (
        "probe-free-width",
        "wrong by construction: a_leading_mark_has_no_base_and_is_counted",
    ),
    (
        "probe-pinned-node",
        "wrong by construction: a_copy_is_the_same_at_every_read_size",
    ),
    (
        "probe-no-csi-op",
        "wrong by construction: every_recording_renders_to_its_pinned_digest",
    ),
    (
        "probe-no-csi-params",
        "wrong by construction: every_recording_renders_to_its_pinned_digest",
    ),
    (
        "ascii-only",
        "wrong by construction: every_recording_renders_to_its_pinned_digest",
    ),
    (
        "probe-double-intern-full",
        "not additive, it writes the bucket slot whatever holds it: a_hit_writes_nothing",
    ),
    (
        "probe-band-pin",
        "test-only: test:unit runs band_reserve under it in a step of its own (pinBuild), and so does the scoped gate's pin step",
    ),
];

/// The classes a feature can be in.
#[derive(Clone, Copy, Debug)]
pub struct Classes<'a> {
    /// One build a line, arms joined by commas.
    pub builds: &'a [&'a str],
    /// Built together.
    pub together: &'a [&'a str],
    /// Built alone off aarch64.
    pub off_arm64: &'a [&'a str],
    /// Run off without the defaults.
    pub default_off: &'a [&'a str],
    /// Not run, with why.
    pub excluded: &'a [(&'a str, &'a str)],
}

/// baud-core's classes.
pub const CLASSES: Classes<'static> = Classes {
    builds: &BUILDS,
    together: &TOGETHER,
    off_arm64: &OFF_ARM64,
    default_off: &DEFAULT_OFF,
    excluded: &EXCLUDED,
};

/// The `FAIL` lines for features in two classes, declared in none, or
/// classified and not declared; none when each declared feature is in
/// exactly one class.
pub fn classify(features: &[String], classes: &Classes) -> Vec<String> {
    let mut listed: Vec<&str> = classes
        .builds
        .iter()
        .flat_map(|b| b.split(','))
        .chain(classes.together.iter().copied())
        .chain(classes.off_arm64.iter().copied())
        .chain(classes.default_off.iter().copied())
        .chain(classes.excluded.iter().map(|(n, _)| *n))
        .filter(|n| !n.is_empty())
        .collect();
    listed.sort_unstable();
    let mut dups: Vec<&str> = listed
        .windows(2)
        .filter(|w| w[0] == w[1])
        .map(|w| w[0])
        .collect();
    dups.dedup();
    if !dups.is_empty() {
        return vec![format!("FAIL: in more than one class: {}", dups.join("\n"))];
    }
    let mut declared: Vec<&str> = features.iter().map(String::as_str).collect();
    declared.sort_unstable();
    let unclassified: Vec<&str> = declared
        .iter()
        .copied()
        .filter(|f| !listed.contains(f))
        .collect();
    let stale: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|f| !declared.contains(f))
        .collect();
    let mut out = Vec::new();
    if !unclassified.is_empty() {
        out.push(format!(
            "FAIL: declared in {MANIFEST} and in no class: {}",
            unclassified.join("\n")
        ));
    }
    if !stale.is_empty() {
        out.push(format!(
            "FAIL: classified here and not declared in {MANIFEST}: {}",
            stale.join("\n")
        ));
    }
    out
}

/// Runs baud-core's suite.
pub trait Suite {
    /// The host's target triple, or the line that says it is unknown.
    fn host(&self) -> Result<String, String>;
    /// Runs `cmd` with this process's output: whether it passed.
    fn run(&self, cmd: &Cmd) -> Outcome;
}

/// The cargo and the `rustc` (`RUSTC`, else `rustc`) of this machine.
pub struct Cargo {
    /// The repository's root.
    pub root: PathBuf,
}

impl Suite for Cargo {
    fn host(&self) -> Result<String, String> {
        let rustc = env::var("RUSTC")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "rustc".to_string());
        let triple = Command::new(&rustc)
            .arg("-vV")
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .find_map(|l| l.strip_prefix("host:"))
                    .map(|t| t.trim().to_string())
            })
            .filter(|t| !t.is_empty());
        triple.ok_or_else(|| format!("FAIL: no host triple from {rustc} -vV"))
    }

    fn run(&self, cmd: &Cmd) -> Outcome {
        cmd.status(&self.root)
    }
}

/// The suite under `features`, or under no defaults but `neon-scan` for
/// `None`.
pub fn suite(features: Option<&str>) -> Cmd {
    let cmd = Cmd::new(&[
        "cargo",
        "nextest",
        "run",
        "--release",
        "--cargo-quiet",
        "--no-fail-fast",
        "-p",
        "baud-core",
    ])
    .var("RUSTFLAGS", "-D warnings");
    match features {
        Some(f) => cmd.arg("--features").arg(f),
        None => cmd
            .arg("--no-default-features")
            .arg("--features")
            .arg("neon-scan"),
    }
}

/// The command, against the manifest under `root`: `args` are what
/// follows its name. Exit 0 when the suite passes under every build, 1 on
/// a failure, 2 on a bad command line.
pub fn run(
    root: &Path,
    args: &[&str],
    runner: &dyn Suite,
    out: &mut dyn Write,
) -> io::Result<Exit> {
    if !args.is_empty() {
        writeln!(out, "usage: cargo xtask feature-suites")?;
        return Ok(Exit(2));
    }
    let manifest = std::fs::read(root.join(MANIFEST))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let features = declared(&manifest);
    let Floor(floor) = FLOOR;
    if features.len() != floor {
        writeln!(
            out,
            "FAIL: {} features parsed from {MANIFEST}, the floor says {floor} -- classify the arm and move the floor with it",
            features.len()
        )?;
        return Ok(Exit(1));
    }
    let refused = classify(&features, &CLASSES);
    if !refused.is_empty() {
        for line in refused {
            writeln!(out, "{line}")?;
        }
        return Ok(Exit(1));
    }
    let host = match runner.host() {
        Ok(h) => h,
        Err(line) => {
            writeln!(out, "{line}")?;
            return Ok(Exit(1));
        }
    };
    let together = TOGETHER.join(",");
    let mut builds: Vec<&str> = BUILDS.to_vec();
    if host.starts_with("aarch64-") {
        for f in OFF_ARM64 {
            writeln!(
                out,
                "=== skip {f} on {host}: neon-scan replaces the scan it reroutes, and baud-core's dev-dependency on itself keeps neon-scan on"
            )?;
        }
    } else {
        builds.extend(OFF_ARM64);
    }
    builds.push(together.as_str());

    let mut fail = Outcome::Passed;
    let mut n = 0;
    for f in builds.iter().copied() {
        writeln!(out, "=== baud-core --features {f}")?;
        out.flush()?;
        if runner.run(&suite(Some(f))) == Outcome::Failed {
            fail = Outcome::Failed;
            writeln!(out, "  RED  baud-core --features {f}")?;
        }
        n += 1;
    }
    writeln!(
        out,
        "=== baud-core --no-default-features --features neon-scan (off: {})",
        DEFAULT_OFF.join(" ")
    )?;
    out.flush()?;
    if runner.run(&suite(None)) == Outcome::Failed {
        fail = Outcome::Failed;
        writeln!(out, "  RED  baud-core without the defaults")?;
    }
    n += 1;

    if fail == Outcome::Failed {
        writeln!(
            out,
            "FAIL: baud-core's suite is red under an arm that claims the default screen"
        )?;
        return Ok(Exit(1));
    }
    writeln!(
        out,
        "ok: baud-core's suite passes in {n} builds of its arms on {host}"
    )?;
    Ok(Exit(0))
}
