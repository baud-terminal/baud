//! **A family name is compared by default caseless match**: CSS Fonts 3
//! section 5.1 (`[[source: css-fonts-3]]`), D144 of
//! `[[source: unicode17-ch3]]`, the full case folding of
//! `[[source: unicode-casefolding]]` and no normalisation.

use baud_domain::{case_fold, caseless_eq};

/// Every C and F row of the pinned file folds to what the row says, and a
/// code point no row names folds to itself.
#[test]
fn every_folding_is_the_pinned_files() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let text = std::fs::read_to_string(format!(
        "{root}/docs/research/sources/unicode-casefolding.txt"
    ))
    .expect("the pinned CaseFolding.txt");
    let mut rows = 0;
    let mut named = std::collections::HashSet::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(';').map(str::trim).collect();
        if f[1] != "C" && f[1] != "F" {
            continue;
        }
        let cp = char::from_u32(u32::from_str_radix(f[0], 16).unwrap()).unwrap();
        let want: Vec<char> = f[2]
            .split_whitespace()
            .map(|x| char::from_u32(u32::from_str_radix(x, 16).unwrap()).unwrap())
            .collect();
        assert_eq!(
            case_fold(cp).collect::<Vec<_>>(),
            want,
            "U+{:04X}",
            u32::from(cp)
        );
        named.insert(cp);
        rows += 1;
    }
    assert!(rows > 1400, "{rows} rows: the file was not read");
    for c in [
        'a',
        '0',
        ' ',
        '\u{0131}',
        '\u{05D0}',
        '\u{4E2D}',
        '\u{1F600}',
    ] {
        assert!(!named.contains(&c));
        assert_eq!(case_fold(c).collect::<Vec<_>>(), vec![c], "{c:?}");
    }
}

/// **The texts' own cases.** Full folding, so `ß` is `ss` and `ﬁ` is
/// `fi`; status T is left out, so `I` folds to `i` and dotted `İ` to `i`
/// with U+0307, which `i` alone is not; and nothing is normalised, so the
/// precomposed `å` does not match `a` followed by U+030A -- CSS's own
/// example of a pair that "will not match".
#[test]
fn the_texts_cases() {
    let eq = |a: &str, b: &str| caseless_eq(a.chars(), b.chars());
    assert!(eq("Menlo", "MENLO"));
    assert!(eq("JetBrains Mono", "jetbrains mono"));
    assert!(eq("STRASSE", "Straße"));
    assert!(eq("\u{FB01}x", "FIX"));
    assert!(eq("\u{212A}", "k"), "KELVIN SIGN");
    assert!(eq("I", "i"));
    assert!(!eq("\u{130}", "i"), "dotted capital I keeps its dot");
    assert!(eq("\u{130}", "i\u{307}"));
    assert!(!eq("\u{E5}", "a\u{30A}"), "no normalisation");
    assert!(eq("\u{C5}", "\u{E5}"));
    assert!(!eq("Menlo", "Menl"));
    assert!(!eq("Menl", "Menlo"));
    assert!(eq("", ""));
    assert!(!eq("ss", "s"));
}
