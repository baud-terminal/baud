//! `cargo xtask feature-suites`: baud-core's suite runs once per build of
//! its classes, `route-stride` only off aarch64, then without the
//! defaults; a red build fails the gate; a feature in no class or in two,
//! or a count off the floor, fails before anything runs; and the
//! repository's baud-core is classified whole.

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use xtask::cmd::Cmd;
use xtask::feature_arms::{Floor, declared};
use xtask::feature_suites::{self, CLASSES, Classes, FLOOR, MANIFEST, Suite, classify, suite};
use xtask::features_since::Exit;
use xtask::third_party::Outcome;

/// A runner on `host` that records each command and fails those whose
/// line contains one of `red`.
struct Fake {
    host: Result<String, String>,
    red: Vec<String>,
    ran: RefCell<Vec<String>>,
}

impl Fake {
    fn on(host: &str, red: &[&str]) -> Fake {
        Fake {
            host: Ok(host.to_string()),
            red: red.iter().map(|r| r.to_string()).collect(),
            ran: RefCell::new(Vec::new()),
        }
    }
}

impl Suite for Fake {
    fn host(&self) -> Result<String, String> {
        self.host.clone()
    }

    fn run(&self, cmd: &Cmd) -> Outcome {
        let line = cmd.to_string();
        self.ran.borrow_mut().push(line.clone());
        if self.red.iter().any(|r| line.ends_with(r.as_str())) {
            Outcome::Failed
        } else {
            Outcome::Passed
        }
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(fake: &Fake, args: &[&str]) -> (Exit, String) {
    let mut out = Vec::new();
    let exit = feature_suites::run(&root(), args, fake, &mut out).expect("the report");
    (exit, String::from_utf8(out).expect("UTF-8"))
}

const PREFIX: &str =
    "RUSTFLAGS=-D warnings cargo nextest run --release --cargo-quiet --no-fail-fast -p baud-core";

fn together() -> String {
    feature_suites::TOGETHER.join(",")
}

fn header(f: &str) -> String {
    format!("=== baud-core --features {f}\n")
}

const DEFAULTS_OFF: &str =
    "=== baud-core --no-default-features --features neon-scan (off: read-record)\n";

#[test]
fn the_suite_runs_once_per_build_and_skips_route_stride_on_aarch64() {
    let f = Fake::on("aarch64-apple-darwin", &[]);
    let (exit, out) = run(&f, &[]);
    let mut want = String::from(
        "=== skip route-stride on aarch64-apple-darwin: neon-scan replaces the scan it \
         reroutes, and baud-core's dev-dependency on itself keeps neon-scan on\n",
    );
    for b in feature_suites::BUILDS {
        want += &header(b);
    }
    want += &header(&together());
    want += DEFAULTS_OFF;
    want += "ok: baud-core's suite passes in 7 builds of its arms on aarch64-apple-darwin\n";
    assert_eq!((exit, out), (Exit(0), want));
    let mut ran: Vec<String> = feature_suites::BUILDS
        .iter()
        .map(|b| format!("{PREFIX} --features {b}"))
        .collect();
    ran.push(format!("{PREFIX} --features {}", together()));
    ran.push(format!(
        "{PREFIX} --no-default-features --features neon-scan"
    ));
    assert_eq!(*f.ran.borrow(), ran);
}

#[test]
fn off_aarch64_route_stride_is_a_build_of_its_own() {
    let f = Fake::on("x86_64-unknown-linux-gnu", &[]);
    let (exit, out) = run(&f, &[]);
    let mut want = String::new();
    for b in feature_suites::BUILDS {
        want += &header(b);
    }
    want += &header("route-stride");
    want += &header(&together());
    want += DEFAULTS_OFF;
    want += "ok: baud-core's suite passes in 8 builds of its arms on x86_64-unknown-linux-gnu\n";
    assert_eq!((exit, out), (Exit(0), want));
}

#[test]
fn a_red_build_is_named_and_fails_the_gate_after_every_build_runs() {
    let f = Fake::on(
        "aarch64-apple-darwin",
        &[
            "--features diag",
            "--no-default-features --features neon-scan",
        ],
    );
    let (exit, out) = run(&f, &[]);
    assert_eq!(exit, Exit(1));
    assert!(
        out.contains("=== baud-core --features diag\n  RED  baud-core --features diag\n"),
        "{out}"
    );
    assert!(
        out.ends_with(&format!(
            "{DEFAULTS_OFF}  RED  baud-core without the defaults\n\
             FAIL: baud-core's suite is red under an arm that claims the default screen\n"
        )),
        "{out}"
    );
    assert_eq!(f.ran.borrow().len(), 7);
}

#[test]
fn no_host_triple_fails_before_anything_runs() {
    let f = Fake {
        host: Err("FAIL: no host triple from rustc -vV".to_string()),
        red: Vec::new(),
        ran: RefCell::new(Vec::new()),
    };
    assert_eq!(
        run(&f, &[]),
        (Exit(1), "FAIL: no host triple from rustc -vV\n".to_string())
    );
    assert!(f.ran.borrow().is_empty());
}

#[test]
fn an_argument_prints_usage_and_exits_2() {
    let f = Fake::on("aarch64-apple-darwin", &[]);
    assert_eq!(
        run(&f, &["diag"]),
        (Exit(2), "usage: cargo xtask feature-suites\n".to_string())
    );
    assert!(f.ran.borrow().is_empty());
}

const SMALL: Classes = Classes {
    builds: &["a,b", "c"],
    together: &["d", "e"],
    off_arm64: &["f"],
    default_off: &["g"],
    excluded: &[("h", "why")],
};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|n| n.to_string()).collect()
}

#[test]
fn each_feature_must_be_in_exactly_one_class() {
    let all = ["a", "b", "c", "d", "e", "f", "g", "h"];
    assert_eq!(classify(&names(&all), &SMALL), Vec::<String>::new());
    let twice = Classes {
        together: &["d", "e", "c", "b"],
        ..SMALL
    };
    assert_eq!(
        classify(&names(&all), &twice),
        ["FAIL: in more than one class: b\nc"]
    );
    assert_eq!(
        classify(
            &names(&["a", "b", "c", "d", "e", "f", "g", "h", "new"]),
            &SMALL
        ),
        ["FAIL: declared in crates/baud-core/Cargo.toml and in no class: new"]
    );
    assert_eq!(
        classify(&names(&["b", "c", "d", "e", "f", "g"]), &SMALL),
        ["FAIL: classified here and not declared in crates/baud-core/Cargo.toml: a\nh"]
    );
}

/// The positive control: baud-core declares exactly the floor's count, and
/// every feature is in one class.
#[test]
fn the_repository_baud_core_is_classified_whole() {
    let text = fs::read_to_string(root().join(MANIFEST)).expect(MANIFEST);
    let features = declared(&text);
    let Floor(floor) = FLOOR;
    assert_eq!(features.len(), floor);
    assert_eq!(classify(&features, &CLASSES), Vec::<String>::new());
}

#[test]
fn the_suite_command_is_nextest_over_baud_core_under_deny_warnings() {
    assert_eq!(
        suite(Some("diag")).to_string(),
        format!("{PREFIX} --features diag")
    );
    assert_eq!(
        suite(None).to_string(),
        format!("{PREFIX} --no-default-features --features neon-scan")
    );
}
