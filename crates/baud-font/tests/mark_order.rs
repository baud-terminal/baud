//! The order the shaper hands a cluster's marks to a face: Unicode's
//! canonical order (`[[source: unicode-ch3]]` D109) over HarfBuzz's
//! modified combining classes, which permute Hebrew's fixed-position
//! classes 10-26 and leave every other class as it is
//! (`[[source: harfbuzz-hb-unicode]]`).
//!
//! The documents check the code here: the classes are read from the pinned
//! UnicodeData.txt and the permutation from the pinned hb-unicode.hh, so a
//! table in baud that drifted from either cannot agree with itself.

use baud_domain::Codepoint;
use baud_font::{Face, Normalised};
use std::collections::HashMap;

fn source(name: &str) -> String {
    let path = format!(
        "{}/../../docs/research/sources/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Field 3 of UnicodeData.txt, the canonical combining class, for every
/// mark of the Hebrew block and U+FB1E (`[[source: unicode-data]]`).
fn hebrew_marks() -> Vec<(u32, u8)> {
    source("unicode-unicodedata.txt")
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(';').collect();
            let cp = u32::from_str_radix(f[0], 16).ok()?;
            let class: u8 = f[3].parse().ok()?;
            let hebrew = (0x0591..=0x05C7).contains(&cp) || cp == 0xFB1E;
            (hebrew && class != 0).then_some((cp, class))
        })
        .collect()
}

/// `HB_MODIFIED_COMBINING_CLASS_CCC<n> <m>` for n in 10-26, the Hebrew
/// block of hb-unicode.hh (lines 333-349 at 64e2b53).
fn harfbuzz_hebrew_keys() -> HashMap<u8, u8> {
    source("harfbuzz-64e2b53-hb-unicode.hh")
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("#define HB_MODIFIED_COMBINING_CLASS_CCC")?;
            let mut w = rest.split_whitespace();
            let class: u8 = w.next()?.parse().ok()?;
            let key: u8 = w.next()?.parse().ok()?;
            (10..=26).contains(&class).then_some((class, key))
        })
        .collect()
}

fn built_regular() -> Vec<u8> {
    let path = format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn cps(v: &[u32]) -> Vec<Codepoint> {
    v.iter().map(|&c| Codepoint::new(c).unwrap()).collect()
}

fn normalised(face: &Face<'_>, v: &[u32]) -> Vec<u32> {
    Normalised::of(face, &cps(v))
        .as_slice()
        .iter()
        .map(|c| c.value())
        .collect()
}

/// **HarfBuzz's Hebrew classes are a permutation of 10-26**, which is what
/// keeps the order canonically equivalent: the Unicode FAQ's rule that the
/// values "can only be permuted, not combined or split"
/// (`[[source: unicode-faq-normalization]]`). A positive control for the
/// readers too: seventeen keys, and of the UCD's 52 Hebrew marks with a
/// class, the 19 points in 10-26 (U+05B0-U+05BD, U+05BF, U+05C1, U+05C2,
/// U+05C7 and U+FB1E).
#[test]
fn harfbuzzs_hebrew_classes_are_a_permutation_of_ten_to_twenty_six() {
    let keys = harfbuzz_hebrew_keys();
    assert_eq!(keys.len(), 17, "{keys:?}");
    let mut values: Vec<u8> = keys.values().copied().collect();
    values.sort_unstable();
    assert_eq!(values, (10..=26).collect::<Vec<u8>>());
    // Shin dot is first and dagesh before qamats, the swap that matters to
    // Courier New; sheva and hiriq come after qamats.
    assert_eq!((keys[&24], keys[&21], keys[&18]), (10, 12, 21));
    let marks = hebrew_marks();
    let points = marks
        .iter()
        .filter(|&&(_, c)| (10..=26).contains(&c))
        .count();
    assert_eq!((marks.len(), points), (52, 19));
}

/// **Every pair of Hebrew marks after a letter sorts by HarfBuzz's key**,
/// whichever order it was typed in: the canonical class where HarfBuzz
/// leaves it (the accents, 220 and 230), its modified class for 10-26. So
/// two canonically equivalent spellings normalise to one sequence (C6,
/// `[[source: unicode-ch3]]`), and it is HarfBuzz's. Marks of one class
/// keep the order they were typed in, as the Canonical Ordering Algorithm
/// keeps them. The built JetBrains Mono's glyphs are not consulted for a
/// Hebrew point -- none decomposes and none composes with shin
/// (U+FB2A-FB49 are composition exclusions) -- so this holds on every
/// platform, and it counts the pairs where the two orders differ, which
/// canonical order alone would get wrong.
#[test]
fn every_pair_of_hebrew_marks_sorts_by_harfbuzzs_key() {
    let data = built_regular();
    let face = Face::parse(&data).unwrap();
    let keys = harfbuzz_hebrew_keys();
    let marks = hebrew_marks();
    let key = |class: u8| keys.get(&class).copied().unwrap_or(class);
    let (mut pairs, mut differ) = (0usize, 0usize);
    for &(a, ca) in &marks {
        for &(b, cb) in &marks {
            if a == b {
                continue;
            }
            let want = if key(cb) < key(ca) {
                [0x05E9, b, a]
            } else {
                [0x05E9, a, b]
            };
            assert_eq!(
                normalised(&face, &[0x05E9, a, b]),
                want,
                "U+{a:04X} ({ca}) then U+{b:04X} ({cb})"
            );
            pairs += 1;
            // Canonical order would have put them the other way round.
            if (cb < ca) != (key(cb) < key(ca)) {
                differ += 1;
            }
        }
    }
    eprintln!("{pairs} ordered pairs, {differ} where canonical order differs");
    // 52 marks, every ordered pair of two different ones.
    assert_eq!((pairs, differ), (52 * 51, 184));
}

/// **shin + qamats + shin dot, and every spelling of it with a dagesh**,
/// normalise to shin, shin dot, dagesh, qamats: the order Courier New's
/// `ccmp` forms the shin-dot ligature in, from all six
/// spellings of the three points.
#[test]
fn every_spelling_of_shin_with_three_points_normalises_alike() {
    let data = built_regular();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        normalised(&face, &[0x05E9, 0x05B8, 0x05C1]),
        [0x05E9, 0x05C1, 0x05B8]
    );
    let points = [0x05B8, 0x05BC, 0x05C1];
    for p in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let typed = [0x05E9, points[p[0]], points[p[1]], points[p[2]]];
        assert_eq!(
            normalised(&face, &typed),
            [0x05E9, 0x05C1, 0x05BC, 0x05B8],
            "{typed:04X?}"
        );
    }
}

/// **Outside Hebrew the order is canonical**: grave over dot-below sorts
/// the dot first, as D109 orders 220 before 230, and the marks of one
/// class keep their typed order. A face that composes neither is needed so
/// the sequence comes back as sorted; the built face has no `a` with both.
#[test]
fn every_other_mark_keeps_its_canonical_order() {
    let data = built_regular();
    let face = Face::parse(&data).unwrap();
    // x + grave (230) + dot below (220): the dot sorts first.
    assert_eq!(
        normalised(&face, &[0x0078, 0x0300, 0x0323]),
        [0x0078, 0x0323, 0x0300]
    );
    // x + acute + grave, both 230: kept as typed.
    assert_eq!(
        normalised(&face, &[0x0078, 0x0301, 0x0300]),
        [0x0078, 0x0301, 0x0300]
    );
}
