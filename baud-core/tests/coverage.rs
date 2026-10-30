#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
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
                .any(|c| c.split_whitespace().next() == Some("done"))
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

/// An unfinished status, as a table cell can begin with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unfinished {
    Partial,
    Parsed,
    Todo,
}

/// The unfinished status a table cell begins with, in any spelling -- bold,
/// italic or code, any case -- and the text after the status word.
///
/// `**partial** — a gap` is `Partial` with `** — a gap` after it. A word
/// that only begins with one of the three, such as "partially", is not the
/// status.
fn unfinished(cell: &str) -> Option<(Unfinished, &str)> {
    let c = cell.trim().trim_start_matches(['*', '_', '`']);
    for (word, status) in [
        ("partial", Unfinished::Partial),
        ("parsed", Unfinished::Parsed),
        ("todo", Unfinished::Todo),
    ] {
        let Some(head) = c.get(..word.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(word) {
            continue;
        }
        let rest = &c[word.len()..];
        let after = rest.trim_start_matches(['*', '_', '`']);
        let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
        if after.starts_with(word_char) {
            continue;
        }
        return Some((status, rest));
    }
    None
}

/// What a `partial` cell says after its status: the gap, with the closing
/// emphasis and the dash that introduces it taken off.
fn gap(rest: &str) -> &str {
    rest.trim_start_matches(['*', '_', '`'])
        .trim()
        .trim_start_matches(['—', '-', ':'])
        .trim()
}

/// The zero-based lines of the first table in a file. In `vt-coverage.md`
/// that table is the legend, the one row per status that defines them.
fn first_table(src: &str) -> std::ops::Range<usize> {
    let lines: Vec<&str> = src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with('|'))
        .unwrap_or(lines.len());
    let len = lines[start..]
        .iter()
        .take_while(|l| l.trim_start().starts_with('|'))
        .count();
    start..start + len
}

/// A sentence that states a count: the text it begins with, and the texts
/// one of which follows the count.
type Sentence = (&'static str, &'static [&'static str]);

/// The sentences in which a coverage file states how many of its rows are
/// at `partial`: the text each begins with, and the texts one of which
/// follows the count. A file not listed states no count, and so may carry
/// no `partial` row.
const STATED: &[(&str, &[Sentence])] = &[
    (
        "vt-coverage.md",
        &[
            ("the gap is named in the row.", &["rows carry it:"]),
            (
                "**No row carries `parsed` or `todo`.",
                &["carry `**partial**`"],
            ),
        ],
    ),
    (
        "input-coverage.md",
        &[(
            "is an operation whose gap is named.",
            // "Two rows carry", or "One row carries" while one is left.
            &["rows carry `**partial**`", "row carries `**partial**`"],
        )],
    ),
];

/// The words for the numbers below twenty, and for the tens.
const UNITS: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 8] = [
    "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

/// A count written out in words, zero to ninety-nine, as the coverage files
/// write them: "Two", "twenty-eight".
fn number(word: &str) -> Option<usize> {
    let w = word.to_ascii_lowercase();
    if let Some(n) = UNITS.iter().position(|u| *u == w) {
        return Some(n);
    }
    let (tens, unit) = match w.split_once('-') {
        Some((t, u)) => (t, Some(u)),
        None => (w.as_str(), None),
    };
    let t = TENS.iter().position(|x| *x == tens)?;
    let u = match unit {
        Some(u) => match UNITS[1..10].iter().position(|x| *x == u) {
            Some(i) => i + 1,
            None => return None,
        },
        None => 0,
    };
    Some((t + 2) * 10 + u)
}

/// `n` in the words [`number`] reads, zero to ninety-nine.
fn spelled(n: usize) -> String {
    match n {
        0..20 => UNITS[n].to_string(),
        _ if n.is_multiple_of(10) => TENS[n / 10 - 2].to_string(),
        _ => format!("{}-{}", TENS[n / 10 - 2], UNITS[n % 10]),
    }
}

/// The count stated by the word just before `after` in the sentence that
/// `lead` begins, or a description of what is missing.
fn stated(src: &str, lead: &str, after: &str) -> Result<usize, String> {
    let i = src
        .find(lead)
        .ok_or_else(|| format!("no sentence beginning {lead:?}"))?;
    let rest = &src[i + lead.len()..];
    let j = rest
        .find(after)
        .ok_or_else(|| format!("{lead:?} is not followed by {after:?}"))?;
    let word = rest[..j].trim();
    number(word).ok_or_else(|| format!("{word:?} after {lead:?} is not a count"))
}

/// What one coverage file's statuses come to.
struct Statuses {
    /// One-based lines of the rows at `partial`.
    partial: Vec<usize>,
    /// Every way the file breaks the rules, one line each.
    wrong: Vec<String>,
}

/// Reads every table row of one coverage file, `name` being its path under
/// `docs/`.
///
/// A row at `parsed` or `todo` is wrong. A row at `partial` is counted, and
/// is wrong unless the cell names its gap. The counts the file states, by
/// [`STATED`], must equal the rows at `partial`. In `vt-coverage.md` the
/// first table is the legend and is passed over by its place; it must hold
/// the five statuses and nothing else, so that place cannot pass over a row
/// about an operation.
fn statuses(name: &str, src: &str) -> Statuses {
    let mut partial = Vec::new();
    let mut wrong = Vec::new();
    let legend = if name == "vt-coverage.md" {
        let legend = first_table(src);
        let defined: Vec<String> = src
            .lines()
            .skip(legend.start)
            .take(legend.len())
            .filter_map(|l| l.trim().trim_start_matches('|').split('|').next())
            .map(|c| c.trim().trim_matches('*').to_string())
            .filter(|c| !c.is_empty() && !c.chars().all(|ch| ch == '-'))
            .collect();
        if defined != ["done", "partial", "parsed", "todo", "out"] {
            wrong.push(format!(
                "{name}: the first table is not the legend of the five statuses: {defined:?}"
            ));
        }
        legend
    } else {
        0..0
    };
    for (n, line) in src.lines().enumerate() {
        let l = line.trim();
        if !l.starts_with('|') || legend.contains(&n) {
            continue; // prose may discuss the words; only rows assert them
        }
        let mut at_partial = false;
        for cell in l.trim_matches('|').split('|') {
            match unfinished(cell) {
                Some((Unfinished::Partial, rest)) => {
                    at_partial = true;
                    // As an `out` row's reason is held to, by the same length.
                    if gap(rest).len() < 12 {
                        wrong.push(format!("{name}:{}: partial, no gap named: {l}", n + 1));
                    }
                }
                Some((status, _)) => wrong.push(format!("{name}:{}: {status:?}: {l}", n + 1)),
                None => {}
            }
        }
        if at_partial {
            partial.push(n + 1);
        }
    }
    let sentences = STATED
        .iter()
        .find(|(file, _)| *file == name)
        .map_or(&[][..], |(_, s)| *s);
    if sentences.is_empty() && !partial.is_empty() {
        wrong.push(format!(
            "{name}: rows at partial, lines {partial:?}, in a file that states no count"
        ));
    }
    for (lead, afters) in sentences {
        let count = afters
            .iter()
            .map(|after| stated(src, lead, after))
            .find(Result::is_ok)
            .unwrap_or_else(|| stated(src, lead, afters[0]));
        if count != Ok(partial.len()) {
            wrong.push(format!(
                "{name}: the sentence beginning {lead:?} states {count:?}, \
                 and {} rows are at partial, lines {partial:?}",
                partial.len()
            ));
        }
    }
    Statuses { partial, wrong }
}

/// The coverage files, by their path under `docs/`: `vt-coverage.md`,
/// `input-coverage.md`, and every `.md` under `docs/protocols`, its
/// profiles included. A file that cannot be read fails the test rather
/// than being passed over.
fn coverage_files() -> Vec<(String, String)> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs");
    let mut names = vec![
        "vt-coverage.md".to_string(),
        "input-coverage.md".to_string(),
    ];
    let mut found = Vec::new();
    let mut stack = vec![std::path::PathBuf::from(format!("{root}/protocols"))];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap_or_else(|err| panic!("{}: {err}", d.display())) {
            let p = e.expect("dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                let rel = p.strip_prefix(root).expect("a file under docs");
                found.push(rel.display().to_string());
            }
        }
    }
    found.sort();
    names.extend(found);
    names
        .into_iter()
        .map(|n| {
            let src = fs::read_to_string(format!("{root}/{n}"))
                .unwrap_or_else(|err| panic!("docs/{n}: {err}"));
            (n, src)
        })
        .collect()
}

/// **No row is `parsed` or `todo`, every `partial` row names its gap, and
/// the rows at `partial` are as many as the files say.**
///
/// The statuses are read in any spelling, bold or bare and in any case,
/// from `vt-coverage.md`, `input-coverage.md` and every file under
/// `docs/protocols`. Until 2026-10-30 this test matched only a status cell
/// that began with the bare word, and every `partial` row writes it in bold:
/// it passed with twenty-six rows of `vt-coverage.md` at `**partial**` while
/// its own comment said none was, it would have passed a `**parsed**` row,
/// and it did not read `input-coverage.md` at all.
///
/// `parsed` is a bug: `vt-coverage.md` says so in its own words, "a `parsed`
/// row is a bug". `todo` is in scope and unbuilt. `partial` is implemented
/// and known to be incomplete, which is a row saying "not finished", and
/// it is held to two things: the cell names what is missing, and the row
/// is counted where the file states its count, so a row that gains or loses
/// the status moves the count or this fails.
///
/// Two rows once at `partial` show why the status is not benign:
///
/// * `csi_entry` ("folded into `CsiParams`") was a real state-machine gap. The
///   fold dropped Williams' rule that a private marker is legal only in first
///   position, so `CSI 2 ? l` — malformed, and required to be discarded —
///   instead executed as DECRST and switched the terminal into VT52 mode.
/// * `CSI m` was `partial` on the strength of the SGR font group, which is
///   `out` rather than missing. That one was a vocabulary error: it made the
///   table under-claim finished work, which is its own kind of untrue.
///
/// The distinction: `out` is a scope decision with a reason in the row,
/// `partial` is unfinished work. Do not relabel one as the other to make a
/// check pass — reaching for `partial` is a signal the operation is not
/// done, and the fix is the operation.
#[test]
fn no_row_is_unfinished() {
    let files = coverage_files();
    let mut wrong = Vec::new();
    let mut partial = 0usize;
    for (name, src) in &files {
        let s = statuses(name, src);
        partial += s.partial.len();
        wrong.extend(s.wrong);
    }
    // The floors guard the scanner: the protocol files exist, and the rows
    // the files say are at `partial` are found.
    assert!(files.len() > 3, "only {} coverage files read", files.len());
    assert!(
        partial > 0,
        "no row at partial found -- the scanner stopped matching"
    );
    assert!(
        wrong.is_empty(),
        "rows at parsed or todo, partial rows naming no gap, or counts that \
         disagree with the rows -- finish the operation, do not relabel it:\n  {}",
        wrong.join("\n  ")
    );
}

/// The legend's and the paragraph's counts in `vt-coverage.md` equal the
/// rows that carry the status.
#[test]
fn the_vt_coverage_partial_counts_equal_its_partial_rows() {
    let (name, src) = &coverage_files()[0];
    assert_eq!(name, "vt-coverage.md");
    let s = statuses(name, src);
    assert!(s.wrong.is_empty(), "{}", s.wrong.join("\n"));
    assert_eq!(
        stated(src, "the gap is named in the row.", "rows carry it:"),
        Ok(s.partial.len()),
        "the legend's count; the rows are at lines {:?}",
        s.partial
    );
}

/// `input-coverage.md` states its count, and it equals its rows.
#[test]
fn the_input_coverage_partial_count_equals_its_partial_rows() {
    let (name, src) = &coverage_files()[1];
    assert_eq!(name, "input-coverage.md");
    let s = statuses(name, src);
    assert!(s.wrong.is_empty(), "{}", s.wrong.join("\n"));
    let lead = "is an operation whose gap is named.";
    let count = stated(src, lead, "rows carry `**partial**`")
        .or_else(|_| stated(src, lead, "row carries `**partial**`"));
    assert_eq!(
        count,
        Ok(s.partial.len()),
        "the stated count; the rows are at lines {:?}",
        s.partial
    );
}

/// The probes fire: the counting and the parsing, on text whose answer is
/// known.
#[test]
fn the_partial_count_probes_fire() {
    assert_eq!(number("Twenty-eight"), Some(28));
    assert_eq!(number("two"), Some(2));
    assert_eq!(number("Ninety-nine"), Some(99));
    assert_eq!(number("twenty-"), None);
    assert_eq!(number("many"), None);
    assert_eq!(
        stated("x. Three rows carry it: a", "x.", "rows carry it:"),
        Ok(3)
    );
    assert!(stated("x. Three rows", "x.", "rows carry it:").is_err());
    assert!(stated("y", "x.", "rows carry it:").is_err());

    // Every spelling of a status, and the words that only begin like one.
    for (cell, want) in [
        ("**partial** — a gap", Some(Unfinished::Partial)),
        ("partial", Some(Unfinished::Partial)),
        ("_Partial_: a gap", Some(Unfinished::Partial)),
        ("**parsed**", Some(Unfinished::Parsed)),
        ("parsed — then dropped", Some(Unfinished::Parsed)),
        ("`todo`", Some(Unfinished::Todo)),
        ("TODO: later", Some(Unfinished::Todo)),
        ("done — partially drawn", None),
        ("partially drawn", None),
        ("**partially** drawn", None),
        ("parsed_args", None),
        ("todos", None),
        ("**done**", None),
    ] {
        assert_eq!(unfinished(cell).map(|(s, _)| s), want, "{cell:?}");
    }

    // The legend is passed over by its place: the same row further down
    // is a row like any other.
    let table = "\
| | meaning |
|---|---|
| **done** | finished |
| **partial** | the legend |
| **parsed** | dropped |
| **todo** | unbuilt |
| **out** | not in scope |

the gap is named in the row. Two rows carry it: a, b.
**No row carries `parsed` or `todo`. Two carry `**partial**`.

| a | **partial** — a gap a reader can find |
| b | partial: and another one, bare |
| c | done — partially drawn |
prose that says **parsed** is not a row
| **partial** | pasted below the legend |
";
    let s = statuses("vt-coverage.md", table);
    assert_eq!(s.partial, vec![12, 13, 16]);
    assert_eq!(s.wrong.len(), 3, "{:#?}", s.wrong);
    assert!(s.wrong[0].contains(":16: partial, no gap named"));
    let moved = table.replacen("| **done** | finished |", "| **partial** | finished |", 1);
    assert_ne!(moved, table);
    let s = statuses("vt-coverage.md", &moved);
    assert!(
        s.wrong[0].contains("the first table is not the legend"),
        "{:#?}",
        s.wrong
    );
}

/// Each way a coverage file can break the rules fails the check, on the
/// real files changed in memory: an unfinished row in each spelling, a
/// `partial` row with no gap, a legend row pasted below the legend, a new
/// `partial` row the counts were not moved for, and each stated count one
/// off either way.
#[test]
fn the_unfinished_check_fails_each_way_a_file_is_wrong() {
    let files = coverage_files();
    let (_, vt) = &files[0];
    let n = statuses("vt-coverage.md", vt).partial.len();
    let row = "| `ESC 7` | DECSC save cursor |";
    assert!(vt.contains(row), "the anchor row moved");
    for (added, want) in [
        ("**parsed** — recognised and dropped", "Parsed:"),
        ("parsed", "Parsed:"),
        ("**todo**", "Todo:"),
        ("todo — unbuilt", "Todo:"),
        ("**partial**", "partial, no gap named"),
        ("partial —", "partial, no gap named"),
        ("**partial** — the gap, named in full", "states Ok("),
    ] {
        let src = vt.replacen(
            row,
            &format!("| `ESC 9` | an operation | {added} |\n{row}"),
            1,
        );
        let s = statuses("vt-coverage.md", &src);
        assert!(
            s.wrong.iter().any(|w| w.contains(want)),
            "{added:?}: {:#?}",
            s.wrong
        );
    }
    let src = vt.replacen(row, &format!("| **partial** | the legend |\n{row}"), 1);
    let s = statuses("vt-coverage.md", &src);
    assert_eq!(
        s.partial.len(),
        n + 1,
        "a legend row below the legend is a row"
    );
    assert!(s.wrong.iter().any(|w| w.contains("partial, no gap named")));

    for (name, src) in &files[..2] {
        let n = statuses(name, src).partial.len();
        assert!(statuses(name, src).wrong.is_empty());
        let (_, sentences) = STATED.iter().find(|(f, _)| f == name).expect("stated");
        for (lead, afters) in *sentences {
            let after = afters
                .iter()
                .find(|a| stated(src, lead, a).is_ok())
                .expect("the sentence is there");
            let i = src.find(lead).expect("the lead") + lead.len();
            let j = i + src[i..].find(after).expect("the count's end");
            // A file at zero states "Zero rows carry", and has no count
            // below it to try.
            for off in [n.checked_sub(1), Some(n + 1)].into_iter().flatten() {
                let changed = format!("{} {} {}", &src[..i], spelled(off), &src[j..]);
                let s = statuses(name, &changed);
                assert!(
                    s.wrong.iter().any(|w| w.contains(lead)),
                    "{name}: {lead:?} at {off} passed"
                );
            }
        }
    }
}

/// **Every `out` row gives a reason, in the row.**
///
/// `out` is the only status that means "not implemented", and it is what keeps
/// "nothing is partial" from being a word game: a row can always be moved to
/// `out` to make the other checks pass. What stops that is the requirement the
/// legend states — "deliberately not in scope; **the reason is in the row**" —
/// which was prose nothing enforced.
///
/// A bare `**out**` with no reason is now a test failure. This cannot judge
/// whether a reason is *good*; that is review. It can insist one exists, and
/// that a scope decision therefore has to be argued somewhere a reader will
/// find it.
#[test]
fn every_out_row_states_a_reason() {
    let mut bare = Vec::new();
    let mut checked = 0usize;
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
            if !l.starts_with("| `") && !l.starts_with("| ") {
                continue;
            }
            for cell in l.trim_matches('|').split('|') {
                let c = cell.trim();
                if !c.starts_with("**out**") {
                    continue;
                }
                // The legend row defines the vocabulary rather than using it.
                if l.contains("deliberately not in scope") {
                    continue;
                }
                checked += 1;
                let reason = c.trim_start_matches("**out**").trim();
                let reason = reason
                    .trim_start_matches('—')
                    .trim_start_matches('-')
                    .trim();
                if reason.len() < 12 {
                    let name = f.rsplit('/').next().unwrap_or(f);
                    bare.push(format!("{name}:{}: {l}", n + 1));
                }
            }
        }
    }
    // The floor guards the *scanner*, not the count. `out` rows are supposed
    // to become `done` rows over time -- four did this session -- so a
    // shrinking population is the system working, and only zero would mean the
    // parser had stopped matching the table.
    assert!(
        checked > 2,
        "only {checked} out rows checked -- the scanner stopped matching"
    );
    assert!(
        bare.is_empty(),
        "{} `out` row(s) with no reason -- say why, or implement it:\n  {}",
        bare.len(),
        bare.join("\n  ")
    );
}

/// **`docs/input-coverage.md`'s row counts still match the source.**
///
/// The key tables in that document are extracted from `[[source: ctlseqs]]` by
/// `scripts/extract-keys.py`, not transcribed. A key table written from memory
/// is the "guessed table that would be unrecoverable and would look like it
/// worked" this project refused for DEC line drawing, so the extraction has to
/// stay live rather than having been run once.
///
/// This is the positive control as much as the staleness check: if the
/// extractor stops matching -- a regex that no longer fires, a source that
/// reflowed -- it reports zero tables, and zero would otherwise look exactly
/// like agreement with a document nobody updated.
#[test]
fn the_input_key_tables_are_still_extractable() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/extract-keys.py"))
        .arg("--count")
        .current_dir(root)
        .output()
        .expect("python3 to run the extractor");
    assert!(out.status.success(), "extract-keys.py failed");
    let got = String::from_utf8_lossy(&out.stdout);
    let got = got.trim();

    assert_eq!(
        got, "10 tables, 112 key rows",
        "the extraction changed. If `ctlseqs` was re-pinned this is expected \
         and docs/input-coverage.md needs regenerating; if not, the extractor \
         stopped matching and is reporting agreement it did not verify."
    );

    let doc = std::fs::read_to_string(format!("{root}/docs/input-coverage.md"))
        .expect("docs/input-coverage.md");
    assert!(
        doc.contains("**112 key rows**"),
        "the document's total disagrees with the extractor"
    );
    for line in [
        "2473", "2485", "2498", "2529", "2552", "2628", "2672", "2685", "2703", "2713",
    ] {
        assert!(
            doc.contains(line),
            "table at ctlseqs line {line} is extracted but not listed in the document"
        );
    }
}
