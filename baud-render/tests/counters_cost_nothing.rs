//! **Nothing is computed for a counter in a build without the `counters`
//! feature** (`src/counters.rs`): `counters::add` is an empty function
//! there, but its argument is still evaluated, so an argument that walks
//! or accumulates -- an `iter`, a `filter`, a `count`, a `fold` -- is work
//! a shipping frame would do for nothing. Every such call sits under
//! `#[cfg(feature = "counters")]`, and a plain length or `Tally::of(n)`
//! need not. A source check, so it runs in every build.
//!
//! Its controls: it reads every counter call in baud-render's sources, the
//! copy of the screen's (`src/screens.rs`) among them, at least as many as
//! were there when it was written, and it flags a walking argument with no
//! attribute and passes the same one under the attribute.

use std::path::{Path, PathBuf};

/// What an argument may not call outside the feature.
const WALKS: [&str; 4] = [".iter(", ".filter(", ".count(", ".fold("];

/// The attribute that takes a call out of a shipping build.
const GATE: &str = "#[cfg(feature = \"counters\")]";

/// How a counter call is spelled: through the module, or by the name a
/// file imported.
const CALLS: [&str; 2] = ["counters::add(", "add(Counter::"];

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source directory") {
        let path = entry.expect("an entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The text of the parenthesised argument list opening at `open`.
fn arguments(src: &str, open: usize) -> &str {
    let mut depth = 0;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..=open + i];
                }
            }
            _ => {}
        }
    }
    &src[open..]
}

/// Each counter call in `src` whose argument walks with no gate before
/// it, as the text of the call; and how many calls there are.
fn ungated(src: &str) -> (Vec<String>, usize) {
    let (mut bad, mut calls) = (Vec::new(), 0);
    for call in CALLS {
        for (at, _) in src.match_indices(call) {
            // `counters::add(` also ends `crate::counters::add(`, and
            // `add(Counter::` never starts inside it: one call, one match.
            if call == CALLS[1] && src[..at].ends_with("counters::") {
                continue;
            }
            calls += 1;
            let args = arguments(src, at + call.len() - 1);
            if !WALKS.iter().any(|w| args.contains(w)) {
                continue;
            }
            let start = src[..at].rfind(['\n', ';', '{', '}']).map_or(0, |i| i + 1);
            let before = src[..start].trim_end();
            if !before.ends_with(GATE) {
                bad.push(
                    src[start..at + call.len() - 1 + args.len()]
                        .trim()
                        .to_string(),
                );
            }
        }
    }
    (bad, calls)
}

#[test]
fn no_counter_argument_walks_outside_the_feature() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    let mut bad = Vec::new();
    let mut calls = 0;
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let (b, n) = ungated(&src);
        calls += n;
        bad.extend(b.into_iter().map(|b| format!("{}: {b}", f.display())));
    }
    assert!(calls >= 25, "the counter calls are read: {calls}");
    assert!(
        bad.is_empty(),
        "computed for a counter in a shipping build: {bad:#?}"
    );
}

#[test]
fn the_check_flags_a_walking_argument_and_passes_it_gated() {
    let walks =
        "    let x = 1;\n    crate::counters::add(Counter::A, Tally::of(v.iter().count()));\n";
    assert_eq!(ungated(walks).0.len(), 1, "a walk with no gate is flagged");
    let gated = "    let x = 1;\n    #[cfg(feature = \"counters\")]\n    crate::counters::add(Counter::A, Tally::of(v.iter().count()));\n";
    assert_eq!(
        ungated(gated),
        (Vec::new(), 1),
        "the same walk gated passes"
    );
    let plain = "    add(Counter::A, Tally::of(v.len()));\n";
    assert_eq!(ungated(plain), (Vec::new(), 1), "a length needs no gate");
}
