#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. The compiler's own dead-code
// analysis is the check that catches it.
//! Every public function in `baud-core` is called by a test.
//!
//! `baud-core2` had this gate for a while before `baud-core` did, which is the
//! same asymmetry that left CI running `cargo test -p baud-core --test parser`
//! against a package with no such target: a check written for the old core,
//! and a new core beside it that nothing pointed at.
//!
//! It is deliberately *not* line coverage. "Is there a test that calls this at
//! all" is the weaker question, and it is the one that was being answered
//! wrongly. Its first run here named four entry points, three of them accessors
//! added the same day -- `mouse_tracking`, `highlight_tracking` and
//! `highlight_region` -- and `Term::cursor`, which nothing had ever read.
//!
//! In `baud-core2` the equivalent found `Term::resize` with **no caller
//! anywhere**, and reaching it found three bugs including a panic on shrink.
//! `baud-core` had no `resize` when this was written; it has one now, and this
//! is what makes sure something calls it.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if name != "target" {
                rust_files(&p, out);
            }
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

fn read_all(dirs: &[&str]) -> String {
    let mut files = Vec::new();
    for d in dirs {
        rust_files(&root().join(d), &mut files);
    }
    files
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Public functions that no test calls, with the reason each is exempt.
///
/// Empty on purpose. A name belongs here only when calling it from a test is
/// impossible rather than merely awkward -- and "awkward" is what `resize` was
/// in the other core.
const NOT_CALLABLE: [(&str, &str); 0] = [];

#[test]
fn every_public_function_in_the_new_core_is_called_by_a_test() {
    let src = read_all(&["baud-core/src"]);
    let mut pubs = BTreeSet::new();
    for line in src.lines() {
        let t = line.trim_start();
        let rest = t
            .strip_prefix("pub fn ")
            .or_else(|| t.strip_prefix("pub unsafe fn "));
        if let Some(rest) = rest {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                pubs.insert(name);
            }
        }
    }
    // The positive control, because a scanner that matches nothing passes.
    assert!(
        pubs.len() > 20,
        "only {} public functions found -- the scanner stopped matching",
        pubs.len()
    );

    // `baud-bench/benches` is where the measurement programs live: they were
    // `baud-bench/src/bin/*.rs` until they became `[[bench]]` targets, and
    // this list is what noticed -- `amplification`, `named_acts` and
    // `named_nodes` are called by a bench and by nothing else, so the move
    // made three entry points read as uncovered. A path list is the thing
    // that goes stale when files move, which this repo has recorded for
    // `docs.rs`'s ROOTS and for `audit_all.sh`'s budgets.
    let tests = read_all(&[
        "baud-core/tests",
        "baud-bench/tests",
        "baud-bench/benches",
        "baud-bench/src",
        "baud-domain/tests",
    ]);
    let missing: Vec<&String> = pubs
        .iter()
        .filter(|n| !NOT_CALLABLE.iter().any(|(e, _)| *e == n.as_str()))
        // A call is the name followed by `(`; `t.key(e)` and `Term::key(...)`
        // both match, and a mention in prose does not.
        .filter(|n| !tests.contains(&format!("{n}(")))
        .collect();

    assert!(
        missing.is_empty(),
        "{} public function(s) in baud-core that no test calls:\n  {}\n\
         Each is an entry point whose behaviour nothing checks.",
        missing.len(),
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
