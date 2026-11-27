//! Every budget, every control and every gate of `cargo xtask audit` fires.
//!
//! Each test builds a tree outside the repository holding exactly what the
//! budget tables allow, checks that it passes, then adds one site to one
//! row and checks that row, and only that row, fails. The rows come from
//! the tables themselves, so a row added to them is perturbed here with no
//! edit to this file. The sites are spelled in pieces so this file's own
//! text matches no pattern the audit counts.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::audit::{
    ADDRESS, ADDRESSES, Audit, BOOL_DIR, CORE, CRATES, CROSS_CONTROL, CROSS_CRATE, Cross, Empty,
    FILE_LIMITS, FILES, Failed, GLOB, ISA, SCAN_LIMIT, SUITES, TREES,
};
use xtask::impl_audit::Sites;
use xtask::scan_audit;

/// One raw-integer site.
const CAST: &str = concat!("// 1u8 a", "s u8\n");
/// One signature primitive.
const SIG: &str = concat!("// fn _p(x:", " u32) {}\n");
/// One address operation.
const ADDR: &str = concat!("// a.as_p", "tr()\n");
/// One `bool` field.
const FLAG: &str = concat!("struct X {\n    on:", " bool,\n}\n");

/// A directory under the system's temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn scratch() -> Scratch {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("xtask-audit-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    Scratch(dir)
}

fn put(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
    let mut old = fs::read(&path).unwrap_or_default();
    old.extend_from_slice(text.as_bytes());
    fs::write(&path, old).expect("a write");
}

fn sites(text: &str, n: Sites) -> String {
    text.repeat(n.0)
}

/// A tree holding exactly what every table allows.
fn at_budget() -> Scratch {
    let s = scratch();
    let root = s.0.as_path();
    let mut body = String::from("fn run_of_text() {\n");
    body += &"    while x\n".repeat(SCAN_LIMIT.0);
    body += "}\n";
    put(root, scan_audit::PATH, &body);
    for c in [&ISA, &CORE].into_iter().chain(&CRATES) {
        let file = format!("{}/a.rs", c.roots[0]);
        put(
            root,
            &file,
            &(sites(CAST, c.total) + &sites(SIG, c.signature)),
        );
    }
    for t in &TREES {
        put(root, &format!("{}/a.rs", t.dir), &sites(CAST, t.limit));
    }
    put(
        root,
        &format!("{}/sub/a.rs", SUITES.root),
        &sites(CAST, SUITES.limit),
    );
    put(root, &format!("{GLOB}/zero.rs"), "");
    for (f, limit) in FILE_LIMITS {
        put(root, f, &sites(CAST, limit));
    }
    for f in FILES {
        put(root, f, "");
    }
    put(root, &format!("{BOOL_DIR}lib.rs"), "");
    for (krate, limit) in ADDRESSES {
        put(root, &format!("{krate}/src/addr.rs"), &sites(ADDR, limit));
    }
    s
}

/// What a stand-in cross-compiler answers for one package.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Builds,
    Fails,
}

/// A cross-compiler whose answers are given: the control's, then the
/// crate's.
struct Builds(Answer, Answer);

impl Cross for Builds {
    fn builds(&self, package: &str) -> bool {
        let answer = match package {
            p if p == CROSS_CONTROL => self.0,
            p if p == CROSS_CRATE => self.1,
            p => panic!("asked to build {p}"),
        };
        answer == Answer::Builds
    }
}

const BUILDS: Builds = Builds(Answer::Builds, Answer::Builds);

fn audit_with(root: &Path, cross: &Builds) -> (String, Failed) {
    let mut out = Vec::new();
    let failed = Audit::new(root, &mut out)
        .run(cross)
        .expect("the audit runs");
    (String::from_utf8(out).expect("a UTF-8 report"), failed)
}

fn audit(root: &Path) -> (String, Failed) {
    audit_with(root, &BUILDS)
}

fn fails(report: &str) -> Vec<&str> {
    report.lines().filter(|l| l.starts_with("  FAIL")).collect()
}

/// Exactly one FAIL, starting with `want`, and the run failed.
fn only_fail(report: &str, failed: Failed, want: &str) {
    let f = fails(report);
    assert_eq!(failed, Failed::Yes, "{report}");
    assert_eq!(f.len(), 1, "want one FAIL, {want:?}:\n{report}");
    assert!(f[0].starts_with(want), "want {want:?}, got {:?}", f[0]);
}

#[test]
fn a_tree_at_every_budget_passes() {
    let s = at_budget();
    let (report, failed) = audit(&s.0);
    assert_eq!(failed, Failed::No, "{report}");
    assert!(fails(&report).is_empty(), "{report}");
    assert!(report.contains("  ok   baud-isa-x86-64: cross-compiles\n"));
}

#[test]
fn one_more_site_fails_each_aggregate_on_each_sum() {
    for c in [&ISA, &CORE].into_iter().chain(&CRATES) {
        for (what, site) in [("raw", CAST), ("signature", SIG)] {
            let s = at_budget();
            put(&s.0, &format!("{}/b.rs", c.roots[c.roots.len() - 1]), site);
            let (report, failed) = audit(&s.0);
            only_fail(&report, failed, &format!("  FAIL {}: ", c.name));
            assert!(report.contains("raw-integer sites"), "{what} {}", c.name);
        }
    }
}

#[test]
fn one_more_site_fails_each_tree() {
    for t in &TREES {
        let s = at_budget();
        put(&s.0, &format!("{}/deeper/b.rs", t.dir), CAST);
        let (report, failed) = audit(&s.0);
        only_fail(&report, failed, &format!("  FAIL {}: ", t.label));
    }
}

/// A tree's signature primitives are not its budget's: only the total is.
#[test]
fn a_tree_does_not_count_signatures() {
    let s = at_budget();
    put(&s.0, &format!("{}/b.rs", TREES[0].dir), SIG);
    let (report, failed) = audit(&s.0);
    assert_eq!(failed, Failed::No, "{report}");
}

#[test]
fn one_more_site_fails_the_suites_and_a_loose_file_is_not_one() {
    let s = at_budget();
    put(&s.0, &format!("{}/other/deep/b.rs", SUITES.root), CAST);
    let (report, failed) = audit(&s.0);
    only_fail(&report, failed, &format!("  FAIL {}: ", SUITES.name));

    let s = at_budget();
    put(&s.0, &format!("{GLOB}/zero.rs"), CAST);
    let (report, failed) = audit(&s.0);
    only_fail(
        &report,
        failed,
        &format!("  FAIL {GLOB}/zero.rs: 1 raw-integer"),
    );
}

#[test]
fn one_more_site_fails_each_named_file() {
    let named = FILE_LIMITS.iter().map(|(f, _)| *f).chain(FILES);
    for f in named {
        let s = at_budget();
        put(&s.0, f, CAST);
        let (report, failed) = audit(&s.0);
        only_fail(&report, failed, &format!("  FAIL {f}: "));
    }
}

#[test]
fn one_more_line_fails_each_address_budget() {
    for (krate, limit) in ADDRESSES {
        let s = at_budget();
        put(&s.0, &format!("{krate}/tests/b.rs"), ADDR);
        let (report, failed) = audit(&s.0);
        only_fail(
            &report,
            failed,
            &format!(
                "  FAIL {krate}: {} lines with an address operation (limit {limit})",
                limit.0 + 1
            ),
        );
        assert!(report.contains(&format!("{krate}/tests/b.rs:1:// a.as_p")));
    }
}

/// Lines are counted, not matches: two operations on one line are one.
#[test]
fn an_address_line_counts_once() {
    let s = at_budget();
    let (krate, _) = ADDRESSES[2];
    put(
        &s.0,
        &format!("{krate}/src/addr.rs"),
        "// a.as_ptr() b.addr()\n",
    );
    let (report, failed) = audit(&s.0);
    only_fail(&report, failed, &format!("  FAIL {krate}: 1 lines"));
}

#[test]
fn the_address_pattern_is_checked_on_its_sample() {
    let s = at_budget();
    let mut out = Vec::new();
    let mut a = Audit::new(&s.0, &mut out);
    a.addresses(&ADDRESS.replace(r"\.addr\(\)|", ""))
        .expect("runs");
    let failed = a.failed;
    let report = String::from_utf8(out).expect("UTF-8");
    only_fail(
        &report,
        failed,
        "  FAIL address operations: the pattern misses its own sample -- this gate counts nothing",
    );
}

#[test]
fn a_bool_field_fails_in_any_file_of_the_directory() {
    for file in ["zz.rs", "notes.txt", "deep/er.rs"] {
        let s = at_budget();
        put(&s.0, &format!("{BOOL_DIR}{file}"), FLAG);
        let (report, failed) = audit(&s.0);
        only_fail(
            &report,
            failed,
            "  FAIL crates/baud-domain/src: 1 bool fields (limit 0) -- use a named two-state enum",
        );
        assert!(
            report.contains(&format!(
                "\ncrates/baud-domain/src/{file}:2:    on: bool,\n"
            )),
            "{report}"
        );
    }
}

/// `struct X(bool)` is a newtype, and a field on the line with its type's
/// brace is not one to a line: neither is the shape the gate is for.
#[test]
fn a_newtype_over_bool_is_not_a_field() {
    let s = at_budget();
    put(
        &s.0,
        &format!("{BOOL_DIR}zz.rs"),
        concat!("struct X(bo", "ol);\nstruct Y { on: bo", "ol, }\n"),
    );
    assert_eq!(audit(&s.0).1, Failed::No);
}

#[test]
fn the_cross_compile_fails_on_the_crate_and_on_the_control() {
    let s = at_budget();
    let (report, failed) = audit_with(&s.0, &Builds(Answer::Builds, Answer::Fails));
    only_fail(
        &report,
        failed,
        "  FAIL baud-isa-x86-64: does not compile for x86_64 -- the trait moved and this impl did not",
    );
    let (report, failed) = audit_with(&s.0, &Builds(Answer::Fails, Answer::Builds));
    only_fail(
        &report,
        failed,
        "  FAIL x86_64-unknown-linux-gnu: the control crate does not build, so this gate checked nothing",
    );
}

#[test]
fn a_sixth_scan_fails() {
    let s = at_budget();
    let src = fs::read_to_string(s.0.join(scan_audit::PATH)).expect("write.rs");
    fs::write(
        s.0.join(scan_audit::PATH),
        src.replace("}\n", "    while y\n}\n"),
    )
    .expect("a write");
    let (report, failed) = audit(&s.0);
    only_fail(
        &report,
        failed,
        "  FAIL ingest scans: 6 (limit 5, target 0)",
    );
}

#[test]
fn every_root_with_no_sources_fails() {
    for c in [&ISA, &CORE].into_iter().chain(&CRATES) {
        let s = at_budget();
        for r in c.roots {
            let _ = fs::remove_dir_all(s.0.join(r));
        }
        let (report, _) = audit(&s.0);
        let want = match c.empty {
            Empty::Report(what) => {
                format!("  FAIL {what}: no sources found -- this gate audited nothing")
            }
            Empty::Stop => format!(
                "  FAIL {}: no sources found -- this gate audited nothing",
                c.name
            ),
        };
        assert!(report.lines().any(|l| l == want), "{want}\n{report}");
        let reported = report
            .lines()
            .any(|l| l.starts_with(&format!("  ok   {}: 0 and 0", c.name)));
        assert_eq!(reported, matches!(c.empty, Empty::Report(_)), "{report}");
    }
    for t in &TREES {
        let s = at_budget();
        fs::remove_dir_all(s.0.join(t.dir)).expect("a tree");
        let (report, failed) = audit(&s.0);
        only_fail(
            &report,
            failed,
            &format!(
                "  FAIL {}: no sources found -- this gate audited nothing",
                t.label
            ),
        );
    }
    let s = at_budget();
    fs::remove_dir_all(s.0.join(SUITES.root).join("sub")).expect("the suites");
    let (report, failed) = audit(&s.0);
    only_fail(
        &report,
        failed,
        &format!(
            "  FAIL {}: no sources found -- this gate audited nothing",
            SUITES.name
        ),
    );
}

#[test]
fn a_named_file_that_is_missing_or_unreadable_fails() {
    let s = at_budget();
    fs::remove_file(s.0.join(FILES[0])).expect("a file");
    let (report, failed) = audit(&s.0);
    only_fail(
        &report,
        failed,
        &format!(
            "  FAIL {}: no such file -- this gate audited nothing",
            FILES[0]
        ),
    );

    let s = at_budget();
    for e in fs::read_dir(s.0.join(GLOB))
        .expect("the glob's directory")
        .flatten()
    {
        if e.path().is_file() {
            fs::remove_file(e.path()).expect("a file");
        }
    }
    let (report, _) = audit(&s.0);
    let want = format!("  FAIL {GLOB}/*.rs: no such file -- this gate audited nothing");
    assert!(report.lines().any(|l| l == want), "{report}");

    let s = at_budget();
    fs::write(s.0.join(GLOB).join("zero.rs"), b"\xff\n").expect("a write");
    let (report, failed) = audit(&s.0);
    only_fail(
        &report,
        failed,
        &format!("  FAIL {GLOB}/zero.rs: the audit produced no count"),
    );
}

/// The repository itself, every gate but the cross-compile, which
/// `cargo xtask audit` runs.
#[test]
fn the_repository_is_within_every_budget() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut a = Audit::new(&root, &mut out);
    a.scans(SCAN_LIMIT).expect("the scans");
    for c in [&ISA, &CORE].into_iter().chain(&CRATES) {
        a.krate(c).expect("a crate");
    }
    for t in &TREES {
        a.tree(t).expect("a tree");
    }
    a.suites(&SUITES).expect("the suites");
    a.files().expect("the files");
    a.bools().expect("the bools");
    a.addresses(ADDRESS).expect("the addresses");
    let failed = a.failed;
    let report = String::from_utf8(out).expect("UTF-8");
    assert_eq!(failed, Failed::No, "{report}");
}
