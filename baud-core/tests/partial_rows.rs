//! **Every `**partial**` row is counted where the coverage files say how
//! many there are.**
//!
//! `docs/vt-coverage.md` states its count of unfinished rows twice, in the
//! legend ("N rows carry it") and in the paragraph under it ("N carry
//! `**partial**`"), and `docs/input-coverage.md` states its own once. A
//! status written in bold is invisible to a check that matches the bare
//! word, so the count was prose nothing held: the legend read twenty-eight
//! while twenty-seven rows carried the status, and it still named a row,
//! kitty graphics' file mediums, that had been done since 2026-10-16.
//!
//! The status cell is what is counted: a table cell that begins with the
//! word, bold or not, followed by nothing, a space or a dash. The legend's
//! own row defines the word and is not a row using it. Each stated count is
//! found by its sentence, and a sentence that is missing fails the test
//! rather than passing it, so the probe can always fire.

use std::fs;

/// A count written out in words, one to ninety-nine, as the coverage files
/// write them: "Two", "twenty-eight".
fn number(word: &str) -> Option<usize> {
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

/// Whether a table cell's text is the `partial` status, bold or bare.
fn is_partial(cell: &str) -> bool {
    let c = cell.trim();
    let c = c
        .strip_prefix("**partial**")
        .or_else(|| c.strip_prefix("partial"));
    match c {
        Some(rest) => {
            rest.is_empty()
                || rest.starts_with(' ')
                || rest.starts_with('—')
                || rest.starts_with('-')
        }
        None => false,
    }
}

/// The legend's row: the one whose first cell is the status itself.
fn is_legend(line: &str) -> bool {
    line.trim()
        .trim_start_matches('|')
        .trim_start()
        .starts_with("**partial** |")
}

/// The line numbers, one-based, of every table row with a `partial` status
/// cell, the legend's row left out.
fn partial_rows(src: &str) -> Vec<usize> {
    let mut rows = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let l = line.trim();
        if !l.starts_with('|') || is_legend(l) {
            continue;
        }
        if l.trim_matches('|').split('|').any(is_partial) {
            rows.push(n + 1);
        }
    }
    rows
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

fn doc(name: &str) -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/");
    fs::read_to_string(format!("{path}{name}")).expect("the coverage file is in the tree")
}

/// The legend's and the paragraph's counts in `vt-coverage.md` equal the
/// rows that carry the status.
#[test]
fn the_vt_coverage_partial_counts_equal_its_partial_rows() {
    let src = doc("vt-coverage.md");
    let rows = partial_rows(&src);
    let legend = stated(&src, "the gap is named in the row.", "rows carry it:");
    let paragraph = stated(
        &src,
        "**No row carries `parsed` or `todo`.",
        "carry `**partial**`",
    );
    assert_eq!(
        legend,
        Ok(rows.len()),
        "the legend's count; the rows are at lines {rows:?}"
    );
    assert_eq!(
        paragraph,
        Ok(rows.len()),
        "the paragraph's count; the rows are at lines {rows:?}"
    );
}

/// `input-coverage.md` states its count, and it equals its rows.
#[test]
fn the_input_coverage_partial_count_equals_its_partial_rows() {
    let src = doc("input-coverage.md");
    let rows = partial_rows(&src);
    let lead = "is an operation whose gap is named.";
    // "Two rows carry", or "One row carries" once one is left.
    let count = stated(&src, lead, "rows carry `**partial**`")
        .or_else(|_| stated(&src, lead, "row carries `**partial**`"));
    assert_eq!(
        count,
        Ok(rows.len()),
        "the stated count; the rows are at lines {rows:?}"
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
    let table = "\
| **partial** | the legend |
| a | **partial** — a gap |
| b | partial |
| c | done — partially drawn |
| d | **partial** |
prose that says **partial** is not a row
| e | done |
";
    assert_eq!(partial_rows(table), vec![2, 3, 5]);
    assert_eq!(
        stated("x. Three rows carry it: a", "x.", "rows carry it:"),
        Ok(3)
    );
    assert!(stated("x. Three rows", "x.", "rows carry it:").is_err());
    assert!(stated("y", "x.", "rows carry it:").is_err());
}
