//! `docs/vt-coverage.md` is checked here, not merely asserted.
//!
//! That document says a row reaches **done** only when a test exists that would
//! fail if the operation regressed, and it says so in a section headed "How
//! this file stays true". For most of this project's life that sentence named
//! a file — `tests/coverage.rs` — **that did not exist**. The document whose
//! stated purpose is preventing "documentation that claims coverage it does not
//! have" was doing exactly that, in the paragraph where it said so.
//!
//! This is the mechanism. Be precise about what it does and does not prove:
//!
//! * It **does** prove that every `done` row names an operation some test
//!   mentions. Marking a row done with no test at all now fails here.
//! * It **cannot** prove the test "would fail if the operation regressed".
//!   Nothing mechanical can. That remains a review obligation, and
//!   `vt-coverage.md` now says so in those words rather than implying a
//!   guarantee this file cannot deliver.
//!
//! A weaker check that is real beats a stronger claim that is not.

use std::collections::BTreeSet;
use std::fs;

/// Mnemonics that are deliberately not test-visible, with the reason.
///
/// Kept explicit and tiny. A growing list here means the check is being worked
/// around rather than the coverage being fixed.
const EXEMPT: &[(&str, &str)] = &[
    // Tier 0 parser states are exercised by every test that feeds bytes; they
    // are not operations with names an application sends.
    ("CSI", "the introducer, not an operation"),
    (
        "DCS",
        "the introducer; DECRQSS is the operation and is covered",
    ),
    ("SOS", "consumed as a string; covered as sos_pm_apc_string"),
    ("PM", "as SOS"),
    ("APC", "as SOS"),
];

fn mnemonics(cell: &str) -> BTreeSet<String> {
    // ALL-CAPS runs of >= 2 letters: DECSCA, ICH, BEL, DECSTBM.
    let mut out = BTreeSet::new();
    let mut cur = String::new();
    for ch in cell.chars() {
        if ch.is_ascii_uppercase() || (!cur.is_empty() && ch.is_ascii_digit()) {
            cur.push(ch);
        } else {
            if cur.len() >= 2 {
                out.insert(cur.clone());
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.insert(cur);
    }
    out
}

#[test]
fn every_done_row_is_mentioned_by_a_test() {
    let doc = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/vt-coverage.md"
    ))
    .expect("docs/vt-coverage.md must be readable from the test");

    // All test sources, concatenated once.
    // **Recursive.** Tests live in per-protocol directories now
    // (`tests/vt100/`, `tests/xterm/`, ...), so a flat `read_dir` would see
    // four cross-cutting files and report almost every row uncovered. It did
    // exactly that when the directories landed — which is the check working:
    // it failed loudly rather than quietly measuring nothing.
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    let mut corpus = String::new();
    let mut stack = vec![std::path::PathBuf::from(dir)];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap_or_else(|err| panic!("{}: {err}", d.display())) {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                corpus.push_str(&fs::read_to_string(&p).expect("test source"));
            }
        }
    }
    let corpus = corpus.to_ascii_uppercase();

    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(m, _)| *m).collect();
    let mut missing: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for line in doc.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        if cells.len() < 2 {
            continue;
        }
        // The legend row defines the statuses; it is not a row about an operation.
        if cells[0].contains("**done**") {
            continue;
        }
        if !cells
            .iter()
            .any(|c| c.split_whitespace().next() == Some("done"))
        {
            continue;
        }
        // Mnemonics from the identity columns only.
        let mut ms = mnemonics(cells[0]);
        if cells.len() > 1 {
            ms.extend(mnemonics(cells[1]));
        }
        let ms: Vec<String> = ms
            .into_iter()
            .filter(|m| !exempt.contains(m.as_str()))
            .collect();
        if ms.is_empty() {
            continue; // e.g. `ground`, `csi_param` — states, not operations
        }
        checked += 1;
        if !ms.iter().any(|m| corpus.contains(m.as_str())) {
            missing.push(format!("{}  ({})", cells[0], ms.join("/")));
        }
    }

    assert!(
        checked > 40,
        "only {checked} rows checked -- the parser stopped matching the table, \
         which would make this test vacuous"
    );
    assert!(
        missing.is_empty(),
        "{} done row(s) in docs/vt-coverage.md have no test mentioning them:\n  {}",
        missing.len(),
        missing.join("\n  ")
    );
}

/// Numbers quoted in prose must match the table they describe.
///
/// **This exists because the numbers went stale twice.** README, CLAUDE.md and
/// MEASUREMENTS.md each quote "N rows at done" and README quotes a test count;
/// after two rounds of protocol work they said 92 and 321 while the truth was
/// 96 and 355. A number in prose that nothing checks is the same failure as a
/// coverage claim that nothing checks — this file's whole subject.
///
/// The fix is to gate the count, not to remember to update it.
#[test]
fn quoted_counts_match_reality() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let doc = fs::read_to_string(format!("{root}/docs/vt-coverage.md")).expect("vt-coverage");

    let done = doc
        .lines()
        .filter(|l| l.trim_start().starts_with('|'))
        .filter(|l| !l.contains("**done**")) // the legend row
        .filter(|l| {
            l.trim_matches('|')
                .split('|')
                .any(|c| c.trim().split_whitespace().next() == Some("done"))
        })
        .count();

    let mut wrong = Vec::new();
    for f in ["README.md", "CLAUDE.md", "MEASUREMENTS.md"] {
        let Ok(src) = fs::read_to_string(format!("{root}/{f}")) else {
            continue;
        };
        // Any "<n> rows at **done**" / "<n> operations" claim.
        for pat in [" rows at **done**", " operations,", " operations."] {
            let mut rest = src.as_str();
            while let Some(i) = rest.find(pat) {
                let head = &rest[..i];
                let n: String = head
                    .chars()
                    .rev()
                    .take_while(char::is_ascii_digit)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                if let Ok(claimed) = n.parse::<usize>() {
                    if claimed != done {
                        wrong.push(format!("{f}: claims {claimed}, table has {done}"));
                    }
                }
                rest = &rest[i + pat.len()..];
            }
        }
    }
    assert!(
        done > 80,
        "only {done} done rows parsed -- the parser stopped matching the table"
    );
    assert!(wrong.is_empty(), "stale counts:\n  {}", wrong.join("\n  "));
}

/// **No row is marked `partial`, and this fails if one ever is.**
///
/// `partial` means the table claims an operation baud does not fully do. That
/// is a weaker failure than `parsed` — the row is at least honest — but it is
/// still a row saying "not finished", and the standing rule for this project is
/// that the protocol surface has none of those.
///
/// Two rows carried it for most of this project's life and neither was benign:
///
/// * `csi_entry` ("folded into `CsiParams`") was a real state-machine gap. The
///   fold dropped Williams' rule that a private marker is legal only in first
///   position, so `CSI 2 ? l` — malformed, and required to be discarded —
///   instead executed as DECRST and switched the terminal into VT52 mode.
/// * `CSI m` was `partial` on the strength of the SGR font group, which is
///   `out` rather than missing. That one was a vocabulary error: it made the
///   table under-claim finished work, which is its own kind of untrue.
///
/// The distinction the check enforces: `out` is a scope decision with a reason
/// in the row, `partial` is unfinished work. Do not relabel one as the other to
/// make this pass — the point is that reaching for `partial` is a signal the
/// operation is not done, and the fix is the operation.
#[test]
fn no_row_is_marked_partial() {
    let mut bad = Vec::new();
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs");
    let mut files = vec![format!("{root}/vt-coverage.md")];
    if let Ok(rd) = fs::read_dir(format!("{root}/protocols")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "md") {
                files.push(p.display().to_string());
            }
        }
    }
    for f in &files {
        let Ok(src) = fs::read_to_string(f) else {
            continue;
        };
        for (n, line) in src.lines().enumerate() {
            let l = line.trim();
            if !l.starts_with('|') {
                continue; // prose may discuss the word; only rows assert it
            }
            for cell in l.trim_matches('|').split('|') {
                let c = cell.trim();
                // `**partial**` is the legend defining the vocabulary, not a
                // row using it.
                if c == "partial" || c.starts_with("partial ") || c.starts_with("partial—") {
                    let name = f.rsplit('/').next().unwrap_or(f);
                    bad.push(format!("{name}:{}: {l}", n + 1));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "rows marked partial -- finish the operation, do not relabel it:\n  {}",
        bad.join("\n  ")
    );
}
