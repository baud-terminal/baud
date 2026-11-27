//! baud-font's canonical tables against UAX #15's conformance file,
//! NormalizationTest.txt (`[[source: unicode-normalizationtest]]`), at the
//! Unicode version of the tables (`[[source: unicode-data]]`, 17.0.0).
//!
//! [`decompose`] gives one level of a character's canonical decomposition
//! and [`compose`] the primary composite of a pair, Hangul by arithmetic;
//! [`combining_class`] gives the canonical combining class. NFD and NFC
//! are built here from those three alone, as `[[source: unicode-ch3]]`
//! defines them: the full canonical decomposition (D68), put in canonical
//! order (D109), and for NFC the canonical composition algorithm (D117),
//! where a character composes with the last starter unless it is blocked
//! from it (D115). Every disagreement with the file is then a disagreement
//! of the tables.
//!
//! The file's columns are c1 to c5, and its canonical invariants are the
//! ones checked:
//!
//! * c2 == NFC(c1) == NFC(c2) == NFC(c3) and c4 == NFC(c4) == NFC(c5);
//! * c3 == NFD(c1) == NFD(c2) == NFD(c3) and c5 == NFD(c4) == NFD(c5).
//!
//! A code point that Part 1 does not list is its own NFD and NFC, which
//! is checked as `decompose` giving nothing for it.

use baud_domain::Codepoint;
use baud_font::{combining_class, compose, decompose};
use std::collections::HashSet;

/// Where the file is kept, beside the other pinned Unicode data.
const FILE: &str = "unicode-normalizationtest.txt";

fn source(name: &str) -> String {
    let path = format!(
        "{}/../../docs/research/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn class(c: Codepoint) -> u8 {
    combining_class(c).0
}

/// The full canonical decomposition of `c`, appended to `out`.
fn decompose_fully(c: Codepoint, out: &mut Vec<Codepoint>) {
    match decompose(c) {
        Some((a, b)) => {
            decompose_fully(a, out);
            if let Some(b) = b {
                decompose_fully(b, out);
            }
        }
        None => out.push(c),
    }
}

/// NFD: the full canonical decomposition, each run of non-starters then
/// sorted by combining class and kept in order within a class.
fn nfd(s: &[Codepoint]) -> Vec<Codepoint> {
    let mut out = Vec::with_capacity(s.len() * 2);
    for &c in s {
        decompose_fully(c, &mut out);
    }
    for i in 1..out.len() {
        let mut j = i;
        while j > 0 && class(out[j]) != 0 && class(out[j - 1]) > class(out[j]) {
            out.swap(j - 1, j);
            j -= 1;
        }
    }
    out
}

/// NFC: NFD, then each character joined to the last starter when nothing
/// between them is a starter or has a class at or above its own.
fn nfc(s: &[Codepoint]) -> Vec<Codepoint> {
    let mut out: Vec<Codepoint> = Vec::with_capacity(s.len());
    let mut starter: Option<usize> = None;
    let mut between: Option<u8> = None;
    for c in nfd(s) {
        let cc = class(c);
        let blocked = between.is_some_and(|b| b == 0 || b >= cc);
        if let (Some(at), false) = (starter, blocked) {
            if let Some(joined) = compose(out[at], c) {
                out[at] = joined;
                continue;
            }
        }
        if cc == 0 {
            starter = Some(out.len());
            between = None;
        } else {
            between = Some(cc);
        }
        out.push(c);
    }
    out
}

fn field(f: &str) -> Vec<Codepoint> {
    f.split_whitespace()
        .map(|h| {
            let v = u32::from_str_radix(h, 16).unwrap_or_else(|_| panic!("{h:?} is not hex"));
            Codepoint::new(v).unwrap_or_else(|| panic!("{v:04X} is not a scalar value"))
        })
        .collect()
}

fn hex(s: &[Codepoint]) -> String {
    s.iter()
        .map(|c| format!("{:04X}", c.value()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A normalization form: a string to its normalized string.
type Form = fn(&[Codepoint]) -> Vec<Codepoint>;

/// One invariant: the form's name, the form, the column it must yield and
/// the columns it is applied to.
type Rule = (&'static str, Form, usize, &'static [usize]);

/// The canonical invariants one line of the file states that the tables
/// break, one string each; empty when the line holds.
fn check(line: &str) -> Vec<String> {
    let f: Vec<&str> = line.split(';').collect();
    assert!(f.len() >= 5, "{line:?}: fewer than five columns");
    let c: Vec<Vec<Codepoint>> = f[..5].iter().copied().map(field).collect();
    let rules: [Rule; 4] = [
        ("NFC", nfc, 1, &[0, 1, 2]),
        ("NFC", nfc, 3, &[3, 4]),
        ("NFD", nfd, 2, &[0, 1, 2]),
        ("NFD", nfd, 4, &[3, 4]),
    ];
    let mut failed = Vec::new();
    for (name, form, want, from) in rules {
        for &i in from {
            let got = form(&c[i]);
            if got != c[want] {
                failed.push(format!(
                    "{line}: {name}(c{}) is {}, c{} is {}",
                    i + 1,
                    hex(&got),
                    want + 1,
                    hex(&c[want]),
                ));
            }
        }
    }
    failed
}

/// The data lines of `text` with their part, comments and blank lines
/// left out.
fn lines(text: &str) -> Vec<(u8, &str)> {
    let mut part = None;
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(p) = line.strip_prefix("@Part") {
            part = Some(p.trim().parse::<u8>().unwrap_or_else(|_| panic!("{raw:?}")));
            continue;
        }
        out.push((
            part.unwrap_or_else(|| panic!("{raw:?} before any @Part")),
            line,
        ));
    }
    out
}

/// The positive control: a letter with a dot above, which composes, and a
/// Hangul syllable, which composes by arithmetic, both hold.
#[test]
fn a_line_that_holds_reports_nothing() {
    assert_eq!(
        check("1E0A;1E0A;0044 0307;1E0A;0044 0307;"),
        Vec::<String>::new()
    );
    assert_eq!(
        check("AC01;AC01;1100 1161 11A8;AC01;1100 1161 11A8;"),
        Vec::<String>::new()
    );
}

/// The negative control: the same line with its composed and decomposed
/// columns exchanged breaks every invariant that names c2 or c3.
#[test]
fn a_planted_wrong_expectation_is_reported() {
    let failed = check("1E0A;0044 0307;1E0A;1E0A;0044 0307;");
    assert_eq!(failed.len(), 6, "{failed:#?}");
    assert!(
        failed
            .iter()
            .all(|f| f.contains("(c1)") || f.contains("(c2)") || f.contains("(c3)"))
    );
}

/// A mark composes past a mark of a lower class and not past one of its
/// own: the blocking rule is live in `nfc`.
#[test]
fn a_blocked_mark_stays_apart() {
    // A, grave below (220), acute (230): the grave below has no composite
    // with A, and the acute composes past it.
    assert_eq!(hex(&nfc(&field("0041 0316 0301"))), "00C1 0316");
    // A, bridge above (230), acute (230): the bridge has no composite with
    // A and blocks the acute, which would otherwise make U+00C1.
    assert_eq!(hex(&nfc(&field("0041 0346 0301"))), "0041 0346 0301");
}

/// Every line of every part holds every canonical invariant it states.
#[test]
fn every_line_of_the_conformance_file_holds() {
    let text = source(FILE);
    let all = lines(&text);
    let parts: HashSet<u8> = all.iter().map(|&(p, _)| p).collect();
    assert_eq!(
        parts,
        HashSet::from([0, 1, 2, 3, 4, 5]),
        "the file's six parts"
    );
    assert!(
        all.iter()
            .any(|&(_, l)| l.starts_with("1E0A;1E0A;0044 0307;")),
        "the reader sees a line the file is known to carry"
    );
    let failed: Vec<String> = all.iter().flat_map(|&(_, l)| check(l)).collect();
    assert!(
        failed.is_empty(),
        "{} of {} lines' invariants fail:\n{}",
        failed.len(),
        all.len(),
        failed.join("\n")
    );
}

/// Every scalar value Part 1 does not list is its own NFD and NFC, so the
/// tables decompose none of them.
#[test]
fn a_character_part_1_does_not_list_does_not_decompose() {
    let text = source(FILE);
    let listed: HashSet<u32> = lines(&text)
        .into_iter()
        .filter(|&(p, _)| p == 1)
        .map(|(_, l)| {
            let c1 = l.split(';').next().unwrap_or("");
            u32::from_str_radix(c1.trim(), 16).unwrap_or_else(|_| panic!("{l:?}"))
        })
        .collect();
    assert!(listed.contains(&0x1E0A), "Part 1 is read");
    let failed: Vec<String> = (0..=0x10FFFF)
        .filter(|v| !listed.contains(v))
        .filter_map(Codepoint::new)
        .filter(|&c| decompose(c).is_some())
        .map(|c| format!("U+{:04X} decomposes and Part 1 does not list it", c.value()))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}
