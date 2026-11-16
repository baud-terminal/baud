//! `cargo xtask comments`: each public rule and each private term planted in
//! an added comment line is found at its line, the forms each rule passes
//! stay green, and a notice, a credit and a protocol name pass even when
//! they hold a term, so that each of those controls could fail. Every term
//! here is invented.

use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use xtask::comments::{self, Found, Hit, LineNo, Rule, TermLine, Terms};
use xtask::features_since::Exit;

/// Line 2 refuses the invented terminal's name and allows its keyboard
/// protocol; line 3 is a second term, so a hit has to name the right line.
const TERMS: &str = "# invented\n\
(?i)\\bquillterm(?:'s)? keyboard protocol|(?P<hit>\\bquillterm\\b)\tthe quillterm way\n\
(?i)\\bmossvt\\b\tmossvt\n";

/// What a line that passes finds.
const NONE: [Found; 0] = [];

fn terms() -> Terms {
    Terms::parse(TERMS).expect("the invented terms")
}

/// A diff adding `lines` to `path` from line 1.
fn diff(path: &str, lines: &[&str]) -> String {
    let mut d = format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for l in lines {
        d.push('+');
        d.push_str(l);
        d.push('\n');
    }
    d
}

fn found(path: &str, lines: &[&str]) -> Vec<Found> {
    comments::check(&diff(path, lines), &terms())
        .into_iter()
        .map(|h| h.found)
        .collect()
}

fn one(line: &str) -> Vec<Found> {
    found("src/a.rs", &[line])
}

#[test]
fn an_iso_date_is_found() {
    assert_eq!(
        one("/// Set to four on 2031-02-03."),
        [Found::Public(Rule::Date)]
    );
}

#[test]
fn a_month_before_a_year_is_found() {
    assert_eq!(
        one("// Drawn this way from March 2031."),
        [Found::Public(Rule::Date)]
    );
}

#[test]
fn since_a_year_is_found() {
    assert_eq!(
        one("//! The default since 2031."),
        [Found::Public(Rule::Date)]
    );
}

#[test]
fn a_year_before_two_thousand_is_an_epoch_and_passes() {
    assert_eq!(one("/// Seconds from 1904-01-01 to 1970-01-01."), NONE);
}

#[test]
fn a_former_state_is_found() {
    assert_eq!(
        one("// This used to return early."),
        [Found::Public(Rule::Former)]
    );
    assert_eq!(
        one("// Used to panic on a shrink."),
        [Found::Public(Rule::Former)]
    );
}

#[test]
fn a_purpose_after_be_or_get_passes() {
    assert_eq!(one("/// The table is used to look up a width."), NONE);
    assert_eq!(one("/// Both are used to place the caret."), NONE);
    assert_eq!(one("/// It gets used to size the atlas."), NONE);
}

#[test]
fn a_commit_hash_is_found() {
    assert_eq!(
        one("// Measured at 9fd41b2 and kept."),
        [Found::Public(Rule::Hash)]
    );
    assert_eq!(
        one("// See 4f0e3c1a9b8d7e6f5a4b3c2d1e0f9a8b7c6d5e4f."),
        [Found::Public(Rule::Hash)]
    );
}

#[test]
fn hex_that_is_no_hash_passes() {
    // A colour, a binary literal, a part of a name, digits alone, letters
    // alone, uppercase, a word too short, and a digest too long.
    assert_eq!(one("// The tint is #ff00ffaa, the mask 0b110101."), NONE);
    assert_eq!(one("// The key glyph-a1b2c3d and v_1a2b3c4d stay."), NONE);
    assert_eq!(one("// 1234567 cells, a facade, 1A2B3C4D, ab12cd."), NONE);
    let digest = "a1".repeat(32);
    assert_eq!(one(&format!("// sha256 {digest}")), NONE);
}

#[test]
fn a_decision_story_is_found() {
    assert_eq!(
        one("// We decided to keep the ring."),
        [Found::Public(Rule::Story)]
    );
    assert_eq!(
        one("// The default was reverted."),
        [Found::Public(Rule::Story)]
    );
    assert_eq!(
        one("// It is the owner's call."),
        [Found::Public(Rule::Story)]
    );
}

#[test]
fn a_citation_key_with_a_date_and_a_revision_passes() {
    assert_eq!(
        one(&format!(
            "/// ([[source: {dated}]] and [[source: {revision}]])",
            dated = "fonts-wd-2031-02-03",
            revision = "vt-9fd41b2-terminal",
        )),
        NONE
    );
}

#[test]
fn a_quotation_from_a_specification_passes_the_public_rules() {
    assert_eq!(one("//! \"the mode used to be set\" says the text."), NONE);
    assert_eq!(one("//! \u{201c}since 2031\u{201d}, the text says."), NONE);
}

#[test]
fn a_private_term_is_found_and_named_by_its_line() {
    assert_eq!(one("// As quillterm draws it."), [Found::Term(TermLine(2))]);
    assert_eq!(one("// As MossVT draws it."), [Found::Term(TermLine(3))]);
}

#[test]
fn a_term_inside_quotes_is_still_found() {
    assert_eq!(one("// \"as quillterm does\""), [Found::Term(TermLine(2))]);
}

#[test]
fn a_protocol_name_the_term_file_allows_passes() {
    assert_eq!(one("/// The quillterm keyboard protocol's flags."), NONE);
    assert_eq!(one("/// quillterm's keyboard protocol, level 2."), NONE);
}

#[test]
fn a_licence_notice_holding_a_term_passes() {
    assert_eq!(one("// Copyright (c) 2031 The quillterm Authors"), NONE);
    assert_eq!(one("// SPDX-License-Identifier: MIT"), NONE);
}

#[test]
fn a_line_that_only_mentions_a_licence_is_searched() {
    assert_eq!(
        one("// The licence table used to be sorted."),
        [Found::Public(Rule::Former)]
    );
    assert_eq!(
        one("// quillterm's licence is MIT."),
        [Found::Term(TermLine(2))]
    );
    assert_eq!(
        one("// The copyright line of quillterm."),
        [Found::Term(TermLine(2))]
    );
}

#[test]
fn a_port_credit_holding_a_term_passes() {
    assert_eq!(one("// Ported from mossvt's wcwidth.c at 9fd41b2."), NONE);
}

#[test]
fn the_controls_hold_a_term_so_they_could_fail() {
    // Each control above, with its notice or protocol wording taken out,
    // is found: the pass is the exemption's, not a term that missed.
    assert_eq!(one("// The quillterm Authors"), [Found::Term(TermLine(2))]);
    assert_eq!(
        one("// From mossvt's wcwidth.c at 9fd41b2."),
        [Found::Public(Rule::Hash), Found::Term(TermLine(3))]
    );
    assert_eq!(one("/// The quillterm flags."), [Found::Term(TermLine(2))]);
}

#[test]
fn only_added_whole_line_comments_are_read() {
    let d = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n\
@@ -3,2 +3,3 @@\n\
-// This used to return early.\n\
-// quillterm\n\
+let s = \"2031-02-03 quillterm\";\n\
+let t = 1; // used to be two\n\
+    // indented, at 9fd41b2\n";
    let hits = comments::check(d, &terms());
    assert_eq!(
        hits,
        [Hit {
            path: "src/a.rs".to_string(),
            line: LineNo(5),
            found: Found::Public(Rule::Hash),
        }]
    );
}

#[test]
fn line_numbers_follow_each_hunk_and_file() {
    let d = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n\
@@ -10 +10 @@\n-x\n+// quillterm\n\
@@ -40,0 +41,2 @@\n+code();\n+// mossvt\n\
diff --git a/s.sh b/s.sh\nnew file mode 100644\n--- /dev/null\n+++ b/s.sh\n\
@@ -0,0 +1,3 @@\n+#!/bin/sh\n+# since 2031\n++++ b/still-a-line\n";
    let got: Vec<String> = comments::check(d, &terms())
        .iter()
        .map(|h| h.to_string())
        .collect();
    assert_eq!(
        got,
        [
            "a.rs:10: term file line 2",
            "a.rs:42: term file line 3",
            "s.sh:2: a date",
        ]
    );
}

#[test]
fn a_line_that_reads_like_a_file_header_is_still_a_line() {
    let lines = comments::added(&diff("a.rs", &["++ b/x", "// mossvt"]));
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].line, LineNo(2));
}

#[test]
fn files_with_no_comment_syntax_here_are_not_read() {
    assert_eq!(found("README.md", &["// quillterm on 2031-02-03"]), NONE);
    assert_eq!(
        found("Cargo.toml", &["# quillterm"]),
        [Found::Term(TermLine(2))]
    );
}

#[test]
fn a_hit_names_neither_the_term_nor_the_comment() {
    let hits = comments::check(&diff("a.rs", &["// what quillterm does"]), &terms());
    let printed = hits[0].to_string();
    assert_eq!(printed, "a.rs:1: term file line 2");
    assert!(!printed.to_lowercase().contains("quillterm"));
}

#[test]
fn a_term_file_that_cannot_fire_is_refused() {
    let line = |e: Result<Terms, String>| e.err().expect("refused");
    assert_eq!(
        line(Terms::parse("quillterm\n")),
        "terms line 1: no control after a tab"
    );
    assert_eq!(
        line(Terms::parse("# c\n\\bquillterm\\b\tquilltermish\n")),
        "terms line 2: its control is not found"
    );
    assert_eq!(
        line(Terms::parse("# only a comment\n\n")),
        "the term file holds no term"
    );
    let bad = line(Terms::parse("quill(term\tquillterm\n"));
    assert!(bad.starts_with("terms line 1: "), "{bad}");
    assert!(!bad.contains("quill"), "{bad}");
}

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Repo(PathBuf);

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Repo {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("xtask-comments-{}-{n}", std::process::id()));
        fs::create_dir_all(&d).expect("scratch");
        let r = Self(d);
        r.git(&["init", "-q"]);
        r
    }

    fn git(&self, args: &[&str]) {
        let mut c = Command::new("git");
        c.args(["-c", "user.name=x", "-c", "user.email=x@example.org"])
            .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
            .args(args)
            .current_dir(&self.0)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"] {
            c.env_remove(v);
        }
        let out = c.output().expect("git");
        assert!(out.status.success(), "git {args:?}: {out:?}");
    }

    fn commit(&self, path: &str, text: &str) {
        fs::write(self.0.join(path), text).expect("the file");
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "c"]);
    }

    fn run(&self, args: &[&str], terms: Option<&OsStr>) -> (Exit, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let e = comments::run(args, &self.0, terms, &mut out, &mut err).expect("the gate");
        let s = |b: Vec<u8>| String::from_utf8(b).expect("UTF-8");
        (e, s(out), s(err))
    }
}

#[test]
fn the_command_reads_the_change_against_its_base() {
    let r = Repo::new();
    r.commit("a.rs", "// This used to return early.\nfn f() {}\n");
    r.git(&["tag", "base"]);
    r.commit("a.rs", "fn f() {}\n// As quillterm draws it.\n");
    let tf = r.0.join("terms.txt");
    fs::write(&tf, TERMS).expect("terms");
    let terms = Some(tf.as_os_str());

    let (e, out, _) = r.run(&["--check", "base"], terms);
    assert_eq!(e, Exit(1));
    assert_eq!(out, "a.rs:2: term file line 2\n1 comment finding(s)\n");

    let (e, out, _) = r.run(&["--check", "HEAD"], terms);
    assert_eq!((e, out.as_str()), (Exit(0), "0 comment finding(s)\n"));

    let (e, out, _) = r.run(&["--check", "base", "--", "other.rs"], terms);
    assert_eq!((e, out.as_str()), (Exit(0), "0 comment finding(s)\n"));
}

#[test]
fn the_command_searches_nothing_without_a_usable_term_file() {
    let r = Repo::new();
    r.commit("a.rs", "fn f() {}\n");
    let (e, out, err) = r.run(&["--check", "HEAD"], None);
    assert_eq!((e, out.as_str()), (Exit(2), ""));
    assert_eq!(err, "BAUD_COMMENT_TERMS is not set: nothing was searched\n");

    let missing = r.0.join("absent.txt");
    let (e, out, _) = r.run(&["--check", "HEAD"], Some(missing.as_os_str()));
    assert_eq!((e, out.as_str()), (Exit(2), ""));

    let tf = r.0.join("terms.txt");
    fs::write(&tf, TERMS).expect("terms");
    let (e, out, err) = r.run(&["--check", "no-such-ref"], Some(tf.as_os_str()));
    assert_eq!((e, out.as_str()), (Exit(2), ""));
    assert!(
        err.ends_with("git diff failed: nothing was searched\n"),
        "{err}"
    );

    let (e, _, _) = r.run(&["--check"], Some(tf.as_os_str()));
    assert_eq!(e, Exit(2));
}
