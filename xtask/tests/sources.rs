//! Every check of `cargo xtask sources` fires: a tree that passes, then one
//! perturbation per check, each asserted to fail with its own line.

mod common;

use common::{Case, base, derived_cases, run, sources_cases, third_party_cases};
use xtask::third_party::Outcome;

fn fires(cases: &[Case]) {
    for case in cases {
        let (o, out) = run(case);
        assert_eq!(o, Outcome::Failed, "{}: the gate passed:\n{out}", case.name);
        assert!(
            out.lines()
                .any(|l| l.starts_with(case.expect) || l.contains(case.expect)),
            "{}: no line {:?} in:\n{out}",
            case.name,
            case.expect
        );
    }
}

#[test]
fn the_base_tree_passes() {
    let (o, out) = base().sources();
    assert_eq!(o, Outcome::Passed, "{out}");
    assert_eq!(
        out,
        "  ok   59 learn.microsoft.com pages carry their article text's sha256\n\
         \x20 ok   50 citations resolve, 61 sources in manifest\n\
         \x20 ok   1 sources and 8 other entries indexed, 1 licence texts, 55 fonts\n\
         \x20 ok   51 sources listed in docs/citations.md, 0 code items\n"
    );
}

#[test]
fn every_source_check_fires() {
    fires(&sources_cases());
}

#[test]
fn every_derived_from_check_fires() {
    fires(&derived_cases());
}

#[test]
fn a_parent_the_manifest_holds_passes() {
    let t = base();
    t.edit(
        "docs/research/sources/MANIFEST.toml",
        "group  = \"C\"\n\n[sources.gated-x]",
        "group  = \"C\"\nderived_from = [\"ms-page-00\", \"ms-page-01\"]\n\n[sources.gated-x]",
    );
    let (o, out) = t.sources();
    assert_eq!(o, Outcome::Passed, "{out}");
}

#[test]
fn the_citation_floor_stops_before_the_licence_check() {
    let case = sources_cases()
        .into_iter()
        .find(|c| c.name.starts_with("too few citations"))
        .expect("the floor case");
    let (o, out) = run(&case);
    assert_eq!(o, Outcome::Failed);
    assert!(
        !out.contains("licence texts"),
        "the licence check ran:\n{out}"
    );
    assert!(!out.contains("citations resolve"), "{out}");
}

#[test]
fn a_licence_failure_fails_the_sources_gate() {
    for case in third_party_cases() {
        let t = common::tree(&case);
        let (o, out) = t.sources();
        assert_eq!(
            o,
            Outcome::Failed,
            "{}: the sources gate passed:\n{out}",
            case.name
        );
    }
}

/// The key-set difference is printed as GNU diffutils 3.12's `diff`
/// printed it for the same lists.
#[test]
fn the_key_difference_reads_as_diff_printed_it() {
    let v = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    let got = xtask::sources::diff_sorted(&v("a b c e x y"), &v("a c d e f z"));
    assert_eq!(
        got,
        [
            "2d1", "< b", "3a3", "> d", "5,6c5,6", "< x", "< y", "---", "> f", "> z"
        ]
    );
    assert_eq!(
        xtask::sources::diff_sorted(&v("k"), &v("a k")),
        ["0a1", "> a"]
    );
    assert_eq!(
        xtask::sources::diff_sorted(&v("a k"), &v("k")),
        ["1d0", "< a"]
    );
}

/// The two readers of the manifest: the line reader's rules are the
/// shell's awk's, field for field.
#[test]
fn the_line_reader_reads_as_the_awk_did() {
    let m = "file = \"early\"\n[sources.a]\nfile = \"a.txt\"\nsha256 = \"ab\"\nstatus = \"retrieved\"\n\
             [sources.b]\nfile=bare\nsha256   =   \"cd\"\nstatus = \"gated\" # c\nfilename = \"no\"\n";
    let got: Vec<_> = xtask::sources::line_reader(m)
        .into_iter()
        .map(|e| format!("{}|{}|{}|{}", e.key, e.file, e.sha, e.status))
        .collect();
    assert_eq!(got, ["a|a.txt|ab|retrieved", "b|file=bare|cd|gated\" # c"]);
}
