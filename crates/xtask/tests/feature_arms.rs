//! `cargo xtask feature-arms`: every arm of every listed crate is checked
//! alone in its worker's target directory and reported in the list's
//! order; an unlisted crate, an unread feature, a short or missing manifest
//! and a bad worker count each fail; `--since` checks only the crates a
//! change reaches; and the repository's own crates pass every check that
//! builds nothing.

use std::collections::BTreeSet;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::feature_arms::{
    self, Arm, CRATES, Check, Floor, Gate, Jobs, Settings, Since, dead, declared, has_defaults,
    interpret, jobs, unlisted,
};
use xtask::features_since::Exit;
use xtask::third_party::Outcome;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A scratch tree, removed on drop: crate `a` with a default and two
/// features its source reads, crate `b` with one feature that forwards to
/// `a`, and crate `c` with no features.
struct Tree(PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const A: &str =
    "[package]\nname = \"a\"\n\n[features]\ndefault = []\nx = []\ny = []\n\n[dependencies]\n";
const A_SRC: &str = "#[cfg(feature = \"x\")]\nfn x() {}\n#[cfg(feature=\"y\")]\nfn y() {}\n";
const B: &str = "[package]\nname = \"b\"\n\n[features]\nz = [\"a/x\"]\n";
const C: &str = "[package]\nname = \"c\"\n";

const LIST: [(&str, Floor); 2] = [("a", Floor(2)), ("b", Floor(1))];

impl Tree {
    fn new() -> Tree {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let t = Tree(std::env::temp_dir().join(format!("xtask-arms-{}-{n}", std::process::id())));
        t.write("a/Cargo.toml", A)
            .write("a/src/lib.rs", A_SRC)
            .write("b/Cargo.toml", B)
            .write("c/Cargo.toml", C);
        t
    }

    fn write(&self, path: &str, text: &str) -> &Tree {
        let p = self.0.join(path);
        fs::create_dir_all(p.parent().expect("a parent")).expect("the parent");
        fs::write(p, text).expect("the file");
        self
    }
}

/// A check that records each arm and the directory it built in, and fails
/// the arms named in `broken` with `text`.
struct Fake {
    broken: Vec<String>,
    text: String,
    seen: Mutex<Vec<(String, PathBuf)>>,
}

impl Fake {
    fn passing() -> Fake {
        Fake::failing(&[], "")
    }

    fn failing(broken: &[&str], text: &str) -> Fake {
        Fake {
            broken: broken.iter().map(|b| b.to_string()).collect(),
            text: text.to_string(),
            seen: Mutex::new(Vec::new()),
        }
    }

    fn seen(&self) -> BTreeSet<(String, PathBuf)> {
        self.seen
            .lock()
            .expect("the record")
            .iter()
            .cloned()
            .collect()
    }
}

impl Check for Fake {
    fn check(&self, arm: &Arm, target: &Path) -> (Outcome, String) {
        let label = arm.label();
        self.seen
            .lock()
            .expect("the record")
            .push((label.clone(), target.to_path_buf()));
        if self.broken.contains(&label) {
            (Outcome::Failed, self.text.clone())
        } else {
            (Outcome::Passed, String::new())
        }
    }
}

fn two() -> Settings {
    Settings {
        jobs: Some("2".to_string()),
        cpus: NonZeroUsize::new(8).expect("eight"),
        target: PathBuf::from("/t"),
    }
}

fn no_since(_: &str) -> Since {
    panic!("no --since was given")
}

/// The gate over `root` and `crates`: its exit and its report.
fn gate(
    root: &Path,
    crates: &[(&str, Floor)],
    settings: &Settings,
    check: &dyn Check,
    since: &dyn Fn(&str) -> Since,
    args: &[&str],
) -> (Exit, String) {
    let g = Gate {
        root,
        crates,
        settings,
        check,
        since,
    };
    let mut out = Vec::new();
    let exit = feature_arms::run(&g, args, &mut out).expect("the report");
    (exit, String::from_utf8(out).expect("UTF-8"))
}

fn seen(list: &[(&str, &str)]) -> BTreeSet<(String, PathBuf)> {
    list.iter()
        .map(|(a, t)| (a.to_string(), PathBuf::from(t)))
        .collect()
}

#[test]
fn every_arm_is_checked_alone_in_its_workers_directory() {
    let t = Tree::new();
    let f = Fake::passing();
    let got = gate(&t.0, &LIST, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(0),
            "a: checking 2 feature arms individually (cargo check, -D warnings)\n\
             b: checking 1 feature arms individually (cargo check, -D warnings)\n\
             ok: all 4 feature arms build\n"
                .to_string()
        )
    );
    assert_eq!(
        f.seen(),
        seen(&[
            ("a/x", "/t/arms-0"),
            ("a/y", "/t/arms-1"),
            ("a --no-default-features", "/t/arms-0"),
            ("b/z", "/t/arms-1"),
        ])
    );
}

#[test]
fn a_broken_arm_prints_its_first_two_error_lines_and_fails() {
    let t = Tree::new();
    let text = "warning: unused\nerror[E0425]: one\n  note\nerror: two\nerror: three\n";
    let f = Fake::failing(&["a/y", "a --no-default-features"], text);
    let got = gate(&t.0, &LIST, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(1),
            "a: checking 2 feature arms individually (cargo check, -D warnings)\n  \
             BROKEN  a/y\n      error[E0425]: one\n      error: two\n  \
             BROKEN  a --no-default-features\n      error[E0425]: one\n      error: two\n\
             b: checking 1 feature arms individually (cargo check, -D warnings)\n\
             FAIL: at least one feature arm does not build -- a compiled arm that does not compile cannot be re-measured\n"
                .to_string()
        )
    );
}

#[test]
fn the_worker_count_is_half_the_cpus_at_most_four_or_the_variable() {
    let n = |k| NonZeroUsize::new(k).expect("non-zero");
    for (cpus, want) in [(1, 1), (2, 1), (5, 2), (8, 4), (18, 4)] {
        assert_eq!(jobs(None, n(cpus)), Ok(Jobs(n(want))), "{cpus} CPUs");
    }
    assert_eq!(jobs(Some("6"), n(2)), Ok(Jobs(n(6))));
    for bad in ["0", "04", "x", "+2", "-1", "2 "] {
        assert_eq!(
            jobs(Some(bad), n(8)),
            Err(format!(
                "FAIL: CHECK_FEATURES_JOBS is '{bad}'; it takes a number of workers, 1 or more"
            )),
            "{bad}"
        );
    }
}

#[test]
fn a_bad_worker_count_exits_2_before_anything_is_checked() {
    let t = Tree::new();
    let f = Fake::passing();
    let s = Settings {
        jobs: Some("0".to_string()),
        ..two()
    };
    let got = gate(&t.0, &LIST, &s, &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(2),
            "FAIL: CHECK_FEATURES_JOBS is '0'; it takes a number of workers, 1 or more\n"
                .to_string()
        )
    );
    assert!(f.seen().is_empty());
}

#[test]
fn a_wrong_command_line_prints_usage_and_exits_2() {
    let t = Tree::new();
    for args in [
        &["--since"][..],
        &["HEAD"][..],
        &["--since", ""][..],
        &["-s", "x"][..],
    ] {
        let got = gate(&t.0, &LIST, &two(), &Fake::passing(), &no_since, args);
        assert_eq!(
            got,
            (
                Exit(2),
                "usage: cargo xtask feature-arms [--since <rev>]\n".to_string()
            ),
            "{args:?}"
        );
    }
}

#[test]
fn a_crate_with_features_the_list_does_not_name_fails() {
    let t = Tree::new();
    t.write(
        "d/Cargo.toml",
        "[package]\nname = \"d\"\n[features]\nq = []\n",
    );
    t.write(".hidden/Cargo.toml", "[features]\nq = []\n");
    let f = Fake::passing();
    let got = gate(&t.0, &LIST, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(1),
            "FAIL: these crates have [features] and are not in CRATES:\n  d\n  \
             -- the gate would check fewer crates than the workspace has\n"
                .to_string()
        )
    );
    assert!(f.seen().is_empty());
}

#[test]
fn a_feature_nothing_reads_fails_and_names_where_it_is_forwarded_from() {
    let t = Tree::new();
    t.write("a/Cargo.toml", &A.replace("y = []\n", "y = []\nw = []\n"));
    t.write(
        "a/src/lib.rs",
        &format!("{A_SRC}// #[cfg(feature = \"w\")]\n"),
    );
    t.write("b/Cargo.toml", &B.replace("z = [\"a/x\"]", "z = [\"a/w\"]"));
    assert_eq!(dead(&t.0), ["a/w"]);
    let f = Fake::passing();
    let got = gate(&t.0, &LIST, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(1),
            "FAIL: these features are declared and no #[cfg] reads them:\n  \
             a/w\n      forwarded from b: 5:z = [\"a/w\"]\n  \
             -- enabling one builds the base program and measures nothing\n"
                .to_string()
        )
    );
    assert!(f.seen().is_empty());
}

#[test]
fn a_reader_in_tests_benches_examples_or_build_rs_counts() {
    for path in [
        "tests/t.rs",
        "benches/deep/b.rs",
        "examples/e.rs",
        "build.rs",
    ] {
        let t = Tree::new();
        t.write("a/Cargo.toml", &A.replace("y = []\n", "y = []\nw = []\n"));
        t.write(&format!("a/{path}"), "#[cfg(feature = \"w\")]\nfn w() {}\n");
        assert!(dead(&t.0).is_empty(), "{path}");
    }
}

#[test]
fn a_short_or_missing_manifest_stops_the_gate_after_the_crates_before_it() {
    let t = Tree::new();
    let f = Fake::passing();
    let short = [("b", Floor(1)), ("a", Floor(3))];
    let got = gate(&t.0, &short, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(1),
            "b: checking 1 feature arms individually (cargo check, -D warnings)\n\
             FAIL: only 2 features parsed from a/Cargo.toml (expected at least 3) -- the gate is reading nothing\n"
                .to_string()
        )
    );
    let missing = [("b", Floor(1)), ("e", Floor(0)), ("a", Floor(2))];
    let got = gate(&t.0, &missing, &two(), &f, &no_since, &[]);
    assert_eq!(
        got,
        (
            Exit(1),
            "b: checking 1 feature arms individually (cargo check, -D warnings)\n\
             FAIL: e/Cargo.toml does not exist -- the gate would have read nothing for e\n"
                .to_string()
        )
    );
}

#[test]
fn since_checks_only_the_crates_a_change_reaches_in_the_same_slots() {
    let t = Tree::new();
    let f = Fake::passing();
    let reach = |rev: &str| -> Since {
        Since::Reach(vec![("b".to_string(), format!("changed since {rev}"))])
    };
    let got = gate(&t.0, &LIST, &two(), &f, &reach, &["--since", "HEAD"]);
    assert_eq!(
        got,
        (
            Exit(0),
            "since HEAD: checking the crates a change can reach\n  \
             b: changed since HEAD\n\
             a: skipped, nothing it is built from changed since HEAD\n\
             b: checking 1 feature arms individually (cargo check, -D warnings)\n\
             ok: the 1 feature arms a change since HEAD can reach build; 1 crates skipped\n"
                .to_string()
        )
    );
    assert_eq!(f.seen(), seen(&[("b/z", "/t/arms-1")]));
}

#[test]
fn since_reports_every_other_answer_of_the_scope() {
    let t = Tree::new();
    let run = |s: Since| {
        let f = Fake::passing();
        let answer = move |_: &str| s.clone();
        gate(&t.0, &LIST, &two(), &f, &answer, &["--since", "v1"])
    };
    let all = "a: checking 2 feature arms individually (cargo check, -D warnings)\n\
               b: checking 1 feature arms individually (cargo check, -D warnings)\n\
               ok: all 4 feature arms build\n";
    assert_eq!(
        run(Since::All(
            "Cargo.lock changed, so every crate is checked".to_string()
        )),
        (
            Exit(0),
            format!("since v1: Cargo.lock changed, so every crate is checked\n{all}")
        )
    );
    assert_eq!(
        run(Since::Unknown),
        (
            Exit(0),
            format!("since v1: the scope command failed, so every crate is checked\n{all}")
        )
    );
    assert_eq!(
        run(Since::Refused(
            "FAIL: --since v1: git knows no such commit".to_string()
        )),
        (
            Exit(1),
            "FAIL: --since v1: git knows no such commit\n".to_string()
        )
    );
    assert_eq!(
        run(Since::Reach(Vec::new())),
        (
            Exit(0),
            "since v1: no change reaches a crate with feature arms\n\
             a: skipped, nothing it is built from changed since v1\n\
             b: skipped, nothing it is built from changed since v1\n\
             ok: the 0 feature arms a change since v1 can reach build; 2 crates skipped\n"
                .to_string()
        )
    );
}

#[test]
fn the_scope_commands_answers_are_read_as_the_gate_reads_them() {
    assert_eq!(
        interpret(Exit(1), "FAIL: --since x: git knows no such commit\n"),
        Since::Refused("FAIL: --since x: git knows no such commit".to_string())
    );
    assert_eq!(
        interpret(
            Exit(0),
            "all\tCargo.lock changed, so every crate is checked\n"
        ),
        Since::All("Cargo.lock changed, so every crate is checked".to_string())
    );
    assert_eq!(
        interpret(
            Exit(0),
            "baud-core\tchanged: x\nbaud-bench\tdepends on baud-core\n"
        ),
        Since::Reach(vec![
            ("baud-core".to_string(), "changed: x".to_string()),
            ("baud-bench".to_string(), "depends on baud-core".to_string()),
        ])
    );
    assert_eq!(interpret(Exit(0), ""), Since::Reach(Vec::new()));
    assert_eq!(interpret(Exit(1), "xtask: a path\n"), Since::Unknown);
    assert_eq!(interpret(Exit(2), "usage\n"), Since::Unknown);
}

#[test]
fn a_feature_is_a_line_of_the_features_block_by_its_first_word() {
    let manifest = "[package]\nname = \"p\"\n[features]\n# off = []\ndefault = [\"on\"]\n\
                    on = []\n\n  spaced   =  [ \"dep/x\" ]\nno-equals\n[dependencies]\nd = \"1\"\n";
    assert_eq!(declared(manifest), ["on", "spaced"]);
    assert!(has_defaults(manifest));
    assert!(!has_defaults("[features]\ndefaults = []\n"));
    assert!(has_defaults("[features]\ndefault\t= []\n"));
}

/// The positive control: the repository's crates pass every check made
/// before anything builds.
#[test]
fn the_repository_passes_every_check_that_builds_nothing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(unlisted(&root, &CRATES), Vec::<String>::new());
    assert_eq!(dead(&root), Vec::<String>::new());
    for (name, Floor(min)) in CRATES {
        let text =
            fs::read_to_string(root.join("crates").join(name).join("Cargo.toml")).expect(name);
        let n = declared(&text).len();
        assert!(n >= min, "{name}: {n} features, floor {min}");
    }
}
