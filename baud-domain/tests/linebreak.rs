//! UAX #14 line breaking, against Unicode's own conformance suite
//! (`[[source: unicode-linebreaktest]]`): every case is a code point
//! sequence with `÷` at each break opportunity and `×` where there is none,
//! the boundary after the last character included.

use baud_domain::{LineBreak, LineClass, LineProps, line_breaks, line_breaks_in_paths};
use proptest::prelude::*;

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

/// Each code point's value in a pinned UCD file of `<range> ; <value>`
/// rows, `default` where it lists none, read here and not by the build's
/// reader, so a fault in one is not a fault in both.
fn ucd_values(file: &str, default: &str) -> Vec<String> {
    let path = format!(
        "{}/../docs/research/sources/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut out = vec![default.to_string(); 0x11_0000];
    for line in text.lines() {
        let body = line.split('#').next().unwrap_or("").trim();
        if body.is_empty() {
            continue;
        }
        let (range, value) = body.split_once(';').expect("a range and a value");
        let (range, value) = (range.trim(), value.trim());
        let (lo, hi) = range.split_once("..").unwrap_or((range, range));
        let hex = |s: &str| u32::from_str_radix(s, 16).expect("hex");
        for cp in hex(lo)..=hex(hi) {
            let i = usize::try_from(cp).expect("index");
            // emoji-data.txt lists several properties over one code point:
            // keep Extended_Pictographic, the one read here.
            if out[i] != "Extended_Pictographic" {
                out[i] = value.to_string();
            }
        }
    }
    out
}

/// **Every code point has the class the pinned files give**, through
/// `LineProps::of`: LineBreak.txt's class, resolved as LB1 resolves it
/// with no other criteria (AI, SG and XX to AL; SA to CM for gc Mn and Mc,
/// else AL; CJ to NS), a QU of gc Pi or Pf as `QuPi` or `QuPf`;
/// East_Asian_Width F, W or H as East Asian; and Extended_Pictographic of
/// gc Cn as an unassigned pictographic. Read with this file's own reader,
/// and a class matched by its name, not the table's order.
#[test]
fn every_code_point_has_the_line_class_the_pinned_files_give() {
    let lb = ucd_values("unicode-linebreak.txt", "XX");
    let gc = ucd_values("unicode-derivedgeneralcategory.txt", "Cn");
    let ea = ucd_values("unicode-eastasianwidth.txt", "N");
    let emoji = ucd_values("unicode-emoji-data.txt", "");
    let mut wrong = Vec::new();
    for cp in 0..0x11_0000u32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let i = usize::try_from(cp).expect("index");
        let g = gc[i].as_str();
        let class = match (lb[i].as_str(), g) {
            ("AI" | "SG" | "XX", _) => "AL",
            ("SA", "Mn" | "Mc") => "CM",
            ("SA", _) => "AL",
            ("CJ", _) => "NS",
            ("QU", "Pi") => "QuPi",
            ("QU", "Pf") => "QuPf",
            (other, _) => other,
        };
        let east = if ["F", "W", "H"].contains(&ea[i].as_str()) {
            baud_domain::EastAsian::Yes
        } else {
            baud_domain::EastAsian::No
        };
        let pictographic = if emoji[i] == "Extended_Pictographic" && g == "Cn" {
            baud_domain::Unassigned::Pictographic
        } else {
            baud_domain::Unassigned::No
        };
        let got = LineProps::of(c);
        if format!("{:?}", got.class) != class
            || got.east_asian != east
            || got.pictographic != pictographic
        {
            wrong.push(format!(
                "U+{cp:04X} {got:?}, {class} {east:?} {pictographic:?} expected"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong: {:?}",
        wrong.len(),
        &wrong[..wrong.len().min(20)]
    );
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

/// `text` with `÷` at each opportunity the breaker gives between its
/// characters, the end left out.
fn marked(text: &str, breaker: fn(&[char], &mut dyn FnMut(usize, LineBreak))) -> String {
    let cps: Vec<char> = text.chars().collect();
    let mut at = vec![false; cps.len() + 1];
    breaker(&cps, &mut |i, b| at[i] = b != LineBreak::Prohibited);
    let mut out = String::new();
    for (i, c) in cps.iter().enumerate() {
        if i > 0 && at[i] {
            out.push('÷');
        }
        out.push(*c);
    }
    out
}

fn default(cps: &[char], f: &mut dyn FnMut(usize, LineBreak)) {
    line_breaks(cps, f);
}

fn paths(cps: &[char], f: &mut dyn FnMut(usize, LineBreak)) {
    line_breaks_in_paths(cps, f);
}

/// **A path breaks before its separators, and moves whole after a
/// space** (`linebreak.rs`' header, T1-T4 and B1-B2): `/home` then
/// `/jordan`, `cd ÷/home`, a dated log path at each separator where the
/// default glues its digits (LB25), a Windows path at its backslashes
/// where the default has no opportunity, `https://` and `C:\` whole, `~/`,
/// `./` and `../` whole. Each row's default is beside it, the positive
/// control that the tailoring changed it.
#[test]
fn a_path_breaks_before_its_separators() {
    for (text, want, was) in [
        ("ls /home/jordan", "ls ÷/home÷/jordan", "ls /÷home/÷jordan"),
        ("cd /home", "cd ÷/home", "cd /÷home"),
        (
            "/var/log/2026/11/04.log",
            "/var÷/log÷/2026÷/11÷/04.log",
            "/÷var/÷log/÷2026/11/04.log",
        ),
        (
            r"C:\Users\jordan\Code",
            r"C:\Users÷\jordan÷\Code",
            r"C:÷\Users\jordan\Code",
        ),
        (
            r"\\server\share\a.txt",
            r"\\server÷\share÷\a.txt",
            r"\÷\server\share\a.txt",
        ),
        (
            "https://github.com/jordan",
            "https://÷github.com÷/jordan",
            "https://÷github.com/÷jordan",
        ),
        (
            "file:///Users/jordan",
            "file:///÷Users÷/jordan",
            "file:///÷Users/÷jordan",
        ),
        (
            "cfg ~/.config/baud",
            "cfg ÷~/.config÷/baud",
            "cfg ÷~/.config/÷baud",
        ),
        (
            "cd ../../etc/ssh",
            "cd ÷../../etc÷/ssh",
            "cd ../../÷etc/÷ssh",
        ),
        ("cp dir/ /tmp/", "cp ÷dir/ ÷/tmp/", "cp ÷dir/ /÷tmp/"),
        ("ls /", "ls ÷/", "ls /"),
        ("(/usr/lib)", "(/usr÷/lib)", "(/÷usr/÷lib)"),
        ("see ( /usr)", "see ÷( /usr)", "see ÷( /÷usr)"),
    ] {
        assert_eq!(marked(text, paths), want, "{text}");
        assert_eq!(marked(text, default), was, "{text}: the default");
    }
}

/// **Outside a path run the tailoring is the default**: a fraction and a
/// date (LB25, "so as to not break "1/2" or "06/07/99""), the
/// abbreviations the SY paragraph names, a relative path of one slash and
/// text with none break as UAX #14 breaks them.
#[test]
fn outside_a_path_the_default_rules_hold() {
    for text in [
        "half 1/2 and 06/07/99",
        "-1/12 2026/11/04",
        "w/o A/S and/or",
        "see src/main.rs now",
        "the quick brown fox",
        "a-b c.d e:f",
    ] {
        assert_eq!(marked(text, paths), marked(text, default), "{text}");
    }
    // The positive control: each would differ were it a path run.
    assert_ne!(marked("/w/o", paths), marked("/w/o", default));
}

/// Whether Section 6.1 (LB4-LB12a) decides the boundary before `cps[b]`,
/// which no tailoring may change.
fn decided_by_six_one(cps: &[char], b: usize) -> bool {
    use LineClass::*;
    let raw = |i: usize| LineProps::of(cps[i]).class;
    let (a, rb) = (raw(b - 1), raw(b));
    if matches!(a, BK | CR | LF | NL) || matches!(rb, BK | CR | LF | NL | SP | ZW) {
        return true;
    }
    let mut j = Some(b - 1);
    while let Some(k) = j.filter(|&k| raw(k) == SP) {
        j = k.checked_sub(1);
    }
    if j.is_some_and(|k| raw(k) == ZW) || a == ZWJ {
        return true;
    }
    if matches!(rb, CM | ZWJ) && !matches!(a, BK | CR | LF | NL | SP | ZW) {
        return true;
    }
    matches!(a, WJ | GL) || rb == WJ || (rb == GL && !matches!(a, SP | BA | HY | HH))
}

fn atoms() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec![
        "/", "//", "\\", "~", ".", "..", ":", "://", "C:\\", "\\\\", "home", "jordan", "2026",
        "11", "x", "שלום", " ", "  ", "(", ")", "\"", "\u{201C}", "-", "_", "\u{2060}", "\u{00A0}",
        "\u{200B}", "\u{200D}", "\u{0301}", "\n", "?", "=", "%",
    ])
}

proptest! {
    /// **No decision of Section 6.1 moves** (UAX14-C1's first two
    /// subconditions), over texts built from path pieces and the
    /// characters 6.1 names: WJ, GL, ZW, ZWJ, a combining mark, a space,
    /// a hard break.
    #[test]
    fn the_tailoring_keeps_every_six_one_decision(parts in prop::collection::vec(atoms(), 1..24)) {
        let text: String = parts.concat();
        let cps: Vec<char> = text.chars().collect();
        let (mut d, mut t) = (vec![None; cps.len() + 1], vec![None; cps.len() + 1]);
        line_breaks(&cps, |i, b| d[i] = Some(b));
        line_breaks_in_paths(&cps, |i, b| t[i] = Some(b));
        for b in 1..cps.len() {
            if decided_by_six_one(&cps, b) {
                prop_assert_eq!(d[b], t[b], "{:?} at {}", text, b);
            }
        }
        prop_assert_eq!(d[cps.len()], t[cps.len()]);
    }

    /// **Text with no separator holds no path run**, so it breaks as the
    /// default does at every boundary.
    #[test]
    fn text_without_a_separator_breaks_as_the_default(parts in prop::collection::vec(atoms(), 1..24)) {
        let text: String = parts.concat().replace(['/', '\\'], "");
        prop_assert_eq!(marked(&text, paths), marked(&text, default));
    }
}
