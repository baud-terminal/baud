//! `cargo xtask scoped-test`: the gates that build nothing run every time,
//! a tried tree stops after them, a plan that names every gate exits 3,
//! and each step the plan names runs the command it names, timed and
//! reported, with any failure named at the end and exit 1.

use std::cell::{Cell, RefCell};
use std::path::Path;

use xtask::cmd::Cmd;
use xtask::features_since::Exit;
use xtask::scoped_test::{self, Gates, Os, Secs};
use xtask::third_party::Outcome;

/// Gates that answer `plan` for the scope, record each command, fail
/// those whose line contains one of `red`, and tick one second a reading.
struct Fake {
    plan: Option<String>,
    red: Vec<String>,
    ran: RefCell<Vec<String>>,
    tick: Cell<u64>,
}

impl Fake {
    fn new(plan: Option<&str>, red: &[&str]) -> Fake {
        Fake {
            plan: plan.map(str::to_string),
            red: red.iter().map(|r| r.to_string()).collect(),
            ran: RefCell::new(Vec::new()),
            tick: Cell::new(0),
        }
    }

    fn ran(&self) -> Vec<String> {
        self.ran.borrow().clone()
    }
}

impl Gates for Fake {
    fn plan(&self, from: &str, to: &str) -> Option<String> {
        assert_eq!((from, to), ("A", "B"));
        self.plan.clone()
    }

    fn run(&self, cmd: &Cmd) -> Outcome {
        let line = cmd.to_string();
        self.ran.borrow_mut().push(line.clone());
        if self.red.iter().any(|r| line.contains(r.as_str())) {
            Outcome::Failed
        } else {
            Outcome::Passed
        }
    }

    fn clock(&self) -> Secs {
        let t = self.tick.get();
        self.tick.set(t + 1);
        Secs(t)
    }
}

fn run(fake: &Fake, os: Os, args: &[&str]) -> (Exit, String) {
    let mut out = Vec::new();
    let exit = scoped_test::run(args, Path::new("/repo"), os, fake, &mut out).expect("the report");
    (exit, String::from_utf8(out).expect("UTF-8"))
}

/// The lines of steps that each pass, in order.
fn passed(names: &[&str]) -> String {
    names
        .iter()
        .map(|n| format!("scoped-test: {n}\nscoped-test: {n} passed (1 s)\n"))
        .collect()
}

const CHEAP: [&str; 5] = [
    "treefmt --fail-on-change",
    "cargo xtask sources",
    "cargo xtask no-python",
    "cargo xtask paths",
    "cargo xtask changelog",
];

const CHEAP_NAMES: [&str; 5] = ["fmt", "sources", "no-python", "paths", "changelog"];

#[test]
fn a_wrong_command_line_prints_usage_and_exits_2() {
    let f = Fake::new(Some(""), &[]);
    for args in [&[][..], &["A"][..], &["A", "B", "C"][..]] {
        assert_eq!(
            run(&f, Os::Darwin, args),
            (
                Exit(2),
                "usage: cargo xtask scoped-test <from> <to>\n".to_string()
            ),
            "{args:?}"
        );
    }
    assert!(f.ran().is_empty());
}

#[test]
fn a_scope_that_cannot_be_computed_fails() {
    let f = Fake::new(None, &[]);
    assert_eq!(
        run(&f, Os::Darwin, &["A", "B"]),
        (
            Exit(1),
            "scoped-test: cargo xtask test-scope failed\n".to_string()
        )
    );
    assert!(f.ran().is_empty());
}

#[test]
fn a_plan_that_names_every_gate_exits_3_and_runs_nothing() {
    let f = Fake::new(Some("full\tdevenv.nix changed, so every gate runs\n"), &[]);
    assert_eq!(
        run(&f, Os::Darwin, &["A", "B"]),
        (
            Exit(3),
            "scoped-test: A..B\n  full\tdevenv.nix changed, so every gate runs\n".to_string()
        )
    );
    assert!(f.ran().is_empty());
}

#[test]
fn an_empty_plan_runs_the_gates_that_build_nothing() {
    let f = Fake::new(Some(""), &[]);
    let (exit, out) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(0));
    assert_eq!(
        out,
        format!(
            "scoped-test: A..B\n  (no test reads what changed)\n{}\
             scoped-test: every scoped gate passed\n",
            passed(&CHEAP_NAMES)
        )
    );
    assert_eq!(f.ran(), CHEAP);
}

#[test]
fn a_tried_tree_stops_after_the_gates_that_build_nothing() {
    let plan = "tried\ttree 1234 was tried clean: 1234 r1\n";
    let f = Fake::new(Some(plan), &[]);
    let (exit, out) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(0));
    assert_eq!(
        out,
        format!(
            "scoped-test: A..B\n  tried\ttree 1234 was tried clean: 1234 r1\n{}\
             scoped-test: tried clean, so no cargo gate runs; every gate that builds nothing passed\n",
            passed(&CHEAP_NAMES)
        )
    );
    assert_eq!(f.ran(), CHEAP);

    let f = Fake::new(Some(plan), &["xtask paths"]);
    let (exit, out) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(1));
    assert!(
        out.ends_with(
            "scoped-test: paths\nscoped-test: paths FAILED (1 s)\n\
             scoped-test: changelog\nscoped-test: changelog passed (1 s)\n\
             scoped-test: FAILED: paths\n"
        ),
        "{out}"
    );
}

const PLAN: &str = "package\tbaud-core\tchanged: baud-core/src/x.rs\n\
                    package\tbaud-bench\tdepends on baud-core\n\
                    binary\txtask\txtask::docs\tchanged: docs/x.md\n\
                    clippy\tbaud-bench\n\
                    clippy\tbaud-core\n\
                    doc\tbaud-core\n\
                    rig\tbaud is reached by x\n\
                    bare\tbaud-core is reached by x\n\
                    linux\tbaud is reached by x\n\
                    gif\tassets/readme/session.gif changed\n";

const BARE: &str = "sh -c cargo build --release --quiet -p baud-core --target x86_64-unknown-none \
                    && cargo build --release --quiet -p baud-font --target x86_64-unknown-none";

fn every_step(rig: &[&str]) -> Vec<String> {
    let mut want: Vec<String> = CHEAP.iter().map(|c| c.to_string()).collect();
    for c in [
        "cargo clippy --quiet -p baud-bench -p baud-core --all-targets -- -D warnings",
        "cargo nextest run --cargo-profile release-probe --cargo-quiet -p baud-core -p baud-bench \
         -p xtask -E package(baud-core) | package(baud-bench) | binary_id(xtask::docs) --no-fail-fast",
        "cargo test --profile release-probe --quiet --doc -p baud-core",
        "cargo xtask feature-arms --since A",
        "sh ./audit_all.sh",
    ] {
        want.push(c.to_string());
    }
    want.extend(rig.iter().map(|r| r.to_string()));
    for c in [
        BARE,
        "cargo clippy --quiet -p baud --target aarch64-unknown-linux-gnu --all-targets -- -D warnings",
        "BAUD_TRAILER_GIF=/repo/assets/readme/session.gif cargo test --quiet -p baud-bench \
         --test trailer_flashes -- the_trailer_is_below_the_flash_thresholds --ignored --exact",
        "cargo run --quiet --example demo -- --verify assets/readme/session.gif",
    ] {
        want.push(c.to_string());
    }
    want
}

const RIG: &str = "RUSTFLAGS=-D warnings cargo nextest run --cargo-profile release-probe \
                   --cargo-quiet -p baud --features effect-probe --test event_rig";

#[test]
fn each_step_the_plan_names_runs_its_command_in_order() {
    let f = Fake::new(Some(PLAN), &[]);
    let (exit, out) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(0), "{out}");
    assert_eq!(f.ran(), every_step(&[RIG]));
    let listed: String = PLAN.lines().map(|l| format!("  {l}\n")).collect();
    let names = [
        "fmt",
        "sources",
        "no-python",
        "paths",
        "changelog",
        "clippy",
        "tests",
        "doctests",
        "features",
        "audit",
        "rig",
        "bare",
        "linux",
        "gif-flashes",
        "gif-verify",
    ];
    assert_eq!(
        out,
        format!(
            "scoped-test: A..B\n{listed}{}scoped-test: every scoped gate passed\n",
            passed(&names)
        )
    );
}

#[test]
fn the_rig_runs_on_macos_only() {
    let f = Fake::new(Some(PLAN), &[]);
    let (exit, out) = run(&f, Os::Other, &["A", "B"]);
    assert_eq!(exit, Exit(0), "{out}");
    assert_eq!(f.ran(), every_step(&[]));
    assert!(!out.contains("scoped-test: rig"), "{out}");
}

#[test]
fn every_failed_step_is_named_at_the_end_and_the_rest_still_run() {
    let f = Fake::new(
        Some(PLAN),
        &[
            "treefmt",
            "nextest run --cargo-profile release-probe --cargo-quiet -p baud-core",
            "audit_all",
        ],
    );
    let (exit, out) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(1));
    assert_eq!(f.ran(), every_step(&[RIG]));
    assert!(out.contains("scoped-test: fmt FAILED (1 s)\n"), "{out}");
    assert!(out.contains("scoped-test: tests FAILED (1 s)\n"), "{out}");
    assert!(
        out.ends_with("scoped-test: FAILED: fmt tests audit\n"),
        "{out}"
    );
}

#[test]
fn features_and_audit_run_only_with_clippy() {
    let plan = "binary\tbaud-render\tbaud-render::cells\tchanged: baud-render/tests/cells.rs\n";
    let f = Fake::new(Some(plan), &[]);
    let (exit, _) = run(&f, Os::Darwin, &["A", "B"]);
    assert_eq!(exit, Exit(0));
    let mut want: Vec<String> = CHEAP.iter().map(|c| c.to_string()).collect();
    want.push(
        "cargo nextest run --cargo-profile release-probe --cargo-quiet -p baud-render \
         -E binary_id(baud-render::cells) --no-fail-fast"
            .to_string(),
    );
    assert_eq!(f.ran(), want);
}

#[test]
fn a_cargo_xtask_command_is_this_program() {
    let c = Cmd::new(&["cargo", "xtask", "paths"]);
    let me = std::env::current_exe().expect("this test");
    let p = c.command(Path::new("/")).expect("a command");
    assert_eq!(p.get_program(), me.as_os_str());
    assert_eq!(p.get_args().collect::<Vec<_>>(), ["paths"]);
    assert!(Cmd::default().command(Path::new("/")).is_none());
}
