//! UAX #14 line breaking, against Unicode's own conformance suite
//! (`[[source: unicode-linebreaktest]]`): every case is a code point
//! sequence with `÷` at each break opportunity and `×` where there is none,
//! the boundary after the last character included.

use baud_domain::{LineBreak, LineClass, LineProps, line_breaks};

struct Case {
    cps: Vec<char>,
    breaks: Vec<bool>,
    /// `line N: ` and the case as written.
    text: String,
}

fn cases() -> Vec<Case> {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-linebreaktest.txt"
    ))
    .expect("LineBreakTest.txt");
    let mut out = Vec::new();
    for (n, line) in src.lines().enumerate() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let (mut cps, mut breaks) = (Vec::new(), Vec::new());
        for tok in body.split_whitespace() {
            match tok {
                "÷" => breaks.push(true),
                "×" => breaks.push(false),
                hex => cps.push(
                    char::from_u32(u32::from_str_radix(hex, 16).expect("hex")).expect("a char"),
                ),
            }
        }
        assert_eq!(breaks.len(), cps.len() + 1, "line {}", n + 1);
        out.push(Case {
            cps,
            breaks,
            text: format!("line {}: {line}", n + 1),
        });
    }
    out
}

/// **Every case of `LineBreakTest.txt` 17.0.0 breaks where it says**: the
/// start is never a break (LB2), the end always is (LB3), and each
/// boundary between is `÷` exactly where `line_breaks` allows or requires
/// one. The positive control: the suite holds over nineteen thousand
/// cases, so a parser that stopped reading them would fail here.
#[test]
fn every_line_break_test_case_breaks_where_it_says() {
    let all = cases();
    assert!(all.len() > 19_000, "only {} cases read", all.len());
    let mut wrong = Vec::new();
    for c in &all {
        assert!(!c.breaks[0], "a break at the start, {}", c.text);
        let mut got = vec![false; c.cps.len() + 1];
        line_breaks(&c.cps, |i, b| got[i] = b != LineBreak::Prohibited);
        if got != c.breaks {
            wrong.push(format!("got {:?}\n  {}", got, c.text));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} cases differ:\n{}",
        wrong.len(),
        all.len(),
        wrong
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// **Hard line breaks are mandatory**, and only they: BK, CR, LF and NL
/// end a line, CR LF once, and the end of the text.
#[test]
fn a_hard_line_break_is_mandatory() {
    let text: Vec<char> = "ab\r\ncd\u{2028}e f".chars().collect();
    let mut got = Vec::new();
    line_breaks(&text, |i, b| got.push((i, b)));
    use LineBreak::{Allowed, Mandatory, Prohibited};
    assert_eq!(
        got,
        vec![
            (1, Prohibited),
            (2, Prohibited),
            (3, Prohibited),
            (4, Mandatory),
            (5, Prohibited),
            (6, Prohibited),
            (7, Mandatory),
            (8, Prohibited),
            (9, Allowed),
            (10, Mandatory),
        ]
    );
    line_breaks(&[], |_, _| panic!("an empty text has no boundaries"));
}

/// **The table's classes are the enum's, in order**, and resolved as LB1
/// resolves them: a Thai letter (SA, Lo) is AL, a Thai vowel sign (SA,
/// Mn) is CM, small kana (CJ) is NS, an ambiguous character (AI) and an
/// unassigned one (XX) are AL, and the quotation marks split by category.
#[test]
fn the_line_trie_agrees_with_the_class_enum() {
    for (ch, want) in [
        ('a', LineClass::AL),
        (' ', LineClass::SP),
        ('\u{0E01}', LineClass::AL),
        ('\u{0E31}', LineClass::CM),
        ('\u{3041}', LineClass::NS),
        ('\u{00A7}', LineClass::AL),
        ('\u{0378}', LineClass::AL),
        ('"', LineClass::QU),
        ('\u{201C}', LineClass::QuPi),
        ('\u{201D}', LineClass::QuPf),
        ('(', LineClass::OP),
        ('\u{4E00}', LineClass::ID),
        ('\u{200D}', LineClass::ZWJ),
        ('\u{1F1E6}', LineClass::RI),
    ] {
        assert_eq!(LineProps::of(ch).class, want, "U+{:04X}", u32::from(ch));
    }
    assert_eq!(
        LineProps::of('\u{4E00}').east_asian,
        baud_domain::EastAsian::Yes
    );
    assert_eq!(LineProps::of('a').east_asian, baud_domain::EastAsian::No);
}

/// **The committed table is the pinned data's**: the generator, run again,
/// writes the same file.
#[test]
fn linebreak_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-linebreak-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "linebreak_data.rs is stale -- rerun scripts/gen-linebreak-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
