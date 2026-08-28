#![deny(dead_code)]
//! **The alternate screen must not write the primary's bookkeeping.**
//!
//! `Storage` holds one run table (`runs`, `nrun`) and one `promoted` flag
//! array, and both belong to the *primary* screen. `head` is the primary's
//! ring position and does not move while the alternate screen is showing, so
//! `head.plus(y)` names a primary line no matter which screen is current, and
//! `promoted[y]` is a primary row. Writing either during an alternate-screen
//! session lands on whatever the primary is holding underneath.
//!
//! Three separate bugs came from that in one session, and none was found by a
//! behavioural test:
//!
//! * every copying line-move zeroed a block of the primary's run counts;
//! * `IND` zeroed one per line scrolled;
//! * `erase_line`'s span-truncation fast path read `promoted[y]` raw, so on
//!   the alternate screen it took the span path and returned **without
//!   erasing** -- a live, user-visible defect where `CSI K` did nothing.
//!
//! The behavioural tests for those live in `vt100/scrolling.rs`. This file
//! catches the *class*: a new operation that touches the run table and forgets
//! which screen it is on. That is worth a structural check rather than another
//! example, because the state is invisible to every public accessor -- in
//! `OpMode::Runs` nothing renders spans, so a zeroed run count cannot be
//! observed through `Term::row` or `Term::viewport` at all. An attempt to
//! write the failing test instead reproduced the harness trap `CLAUDE.md`
//! records: a `Runs` viewport reports representation, so every case came back
//! empty, including one that did nothing.
//!
//! The same shape as `every_public_function_is_called_by_a_test` and the
//! `docs.rs` gates: where behaviour cannot be asserted, assert the code.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Functions allowed to touch `s.nrun` / `s.promoted` without a screen test,
/// each with the reason it is safe.
const EXEMPT: &[(&str, &str)] = &[
    (
        "discard_row",
        "the screen-aware accessor itself: it returns early on the alternate \
         screen and every other writer goes through it",
    ),
    (
        "row_is_promoted",
        "the screen-aware reader itself: `alt || promoted[y]`",
    ),
    (
        "scroll_region_up",
        "the raw writes sit inside the `Screen::Normal` arm of the ring fast \
         path, which the alternate screen never enters; its own arm returns \
         before them",
    ),
];

/// Every `fn` in `baud-core/src` that mentions the run table, and whether its
/// body also mentions the screen.
#[test]
fn the_alt_screen_never_writes_the_primarys_run_table() {
    let mut offenders = BTreeSet::new();
    let mut checked = 0usize;

    for entry in fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("baud-core/src is readable")
    {
        let path = entry.expect("a readable dir entry").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let src = fs::read_to_string(&path).expect("a readable source file");
        let file = path
            .file_name()
            .expect("a named file")
            .to_string_lossy()
            .to_string();

        // Split on `fn ` at a function-definition indent. Crude on purpose:
        // a parser here would be a second thing to keep correct, and an
        // over-wide split can only produce a false *pass* for a helper that
        // happens to sit in the same block -- which the exemption reasons are
        // there to make visible.
        let mut name = String::from("<file scope>");
        let mut body = String::new();
        let flush = |name: &str, body: &str, offenders: &mut BTreeSet<String>| {
            if !(body.contains("s.nrun[") || body.contains("s.promoted[")) {
                return;
            }
            if EXEMPT.iter().any(|(n, _)| *n == name) {
                return;
            }
            // Calling either accessor counts as being screen-aware, and is
            // the *preferred* way to be: `row_is_promoted` answers "is this
            // row materialised" with the alternate screen folded in, and
            // `discard_row` returns early there. A body that names
            // `Screen::Alternate` directly is the fallback for the one site
            // that clears a single run count rather than a row.
            let aware = ["Screen::Alternate", "row_is_promoted", "discard_row"];
            if !aware.iter().any(|a| body.contains(a)) {
                offenders.insert(format!("{file}: {name}"));
            }
        };

        for line in src.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("fn ").or_else(|| {
                t.strip_prefix("pub fn ")
                    .or_else(|| t.strip_prefix("pub(crate) fn "))
            }) {
                flush(&name, &body, &mut offenders);
                checked += 1;
                name = rest
                    .split(['(', '<'])
                    .next()
                    .unwrap_or(rest)
                    .trim()
                    .to_string();
                body.clear();
                continue;
            }
            // **Code only.** A doc comment that says "and not on the
            // alternate screen" is not a guard, and counting it as one makes
            // this gate agree with whatever the author believed. Sabotaging
            // `index`'s guard left the test green for exactly that reason --
            // the comment above the guard named `discard_row`, so the body
            // still matched. That is the same failure as a coverage document
            // asserting its own coverage.
            let code = match line.split_once("//") {
                Some((before, _)) => before,
                None => line,
            };
            body.push_str(code);
            body.push('\n');
        }
        flush(&name, &body, &mut offenders);
    }

    assert!(
        checked > 50,
        "only {checked} functions scanned -- the splitter stopped matching, \
         which is the `inf x amplification` failure: a gate that cannot fire \
         looks exactly like a clean result"
    );
    assert!(
        offenders.is_empty(),
        "{} function(s) write or read the primary's run table without \
         mentioning `Screen::Alternate`:\n  {}\n\nEither route the write \
         through `Term::discard_row` and the read through \
         `Term::row_is_promoted`, or add it to EXEMPT with the reason it is \
         safe.",
        offenders.len(),
        offenders.iter().cloned().collect::<Vec<_>>().join("\n  "),
    );
}

/// The exemption list must not rot: a name that no longer exists is a reason
/// nobody can check, and it silently widens the gate.
#[test]
fn every_exemption_names_a_function_that_exists() {
    let mut src = String::new();
    for entry in fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("baud-core/src is readable")
    {
        let path = entry.expect("a readable dir entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            src.push_str(&fs::read_to_string(&path).expect("a readable source file"));
        }
    }
    let stale: Vec<&str> = EXEMPT
        .iter()
        .filter(|(n, _)| !src.contains(&format!("fn {n}(")))
        .map(|(n, _)| *n)
        .collect();
    assert!(
        stale.is_empty(),
        "EXEMPT names {stale:?}, which no longer exist in baud-core/src"
    );
    assert!(
        EXEMPT.iter().all(|(_, why)| why.len() > 20),
        "every exemption carries a reason, not a bare name"
    );
}
