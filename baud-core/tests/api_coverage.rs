#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! Every public function in `baud-core` is called by a test.
//!
//! This is the check that would have found the resize crash. `Term::resize`
//! had **no caller anywhere** — not in a test, not in the benchmark — because
//! the property generator emits bytes and `resize` is an API call. Scroll a
//! screenful, shrink the window, read the grid, and `viewport`'s `rotate_left`
//! panicked. Three bugs came out of that one entry point once it was reached.
//!
//! It is deliberately *not* line coverage. Most of what this session found was
//! in code the tests already executed — DECSTR's soft reset ran on every
//! relevant test and was still wrong about the alternate screen. Line coverage
//! would have been high throughout and would have said nothing. "Is there a
//! test that calls this at all" is the weaker question, but it is the one that
//! was actually being answered wrongly.

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
/// impossible rather than merely awkward — and "awkward" is what `resize` was.
const NOT_CALLABLE: [(&str, &str); 0] = [];

#[test]
fn every_public_function_is_called_by_a_test() {
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
    assert!(
        pubs.len() > 30,
        "only {} public functions found — the scanner stopped matching",
        pubs.len()
    );

    let tests = read_all(&["baud-core/tests", "baud-domain/tests", "baud-bench/src"]);
    let missing: Vec<&String> = pubs
        .iter()
        .filter(|n| !NOT_CALLABLE.iter().any(|(e, _)| *e == n.as_str()))
        // A call is the name followed by `(`; `t.resize(&ws)` and
        // `Term::resize(...)` both match, and a mention in prose does not.
        .filter(|n| !tests.contains(&format!("{n}(")))
        .collect();

    assert!(
        missing.is_empty(),
        "{} public function(s) in baud-core that no test calls:\n  {}\n\
         Each is an entry point whose behaviour nothing checks. `resize` was \
         in this list and had a panic in it.",
        missing.len(),
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}
