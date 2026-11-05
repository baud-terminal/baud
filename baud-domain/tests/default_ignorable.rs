//! **Every code point is default-ignorable exactly when the pinned file
//! says so**: `is_default_ignorable` against `Default_Ignorable_Code_Point`
//! in DerivedCoreProperties.txt (`[[source: dcp-data]]`), read here with a
//! reader of its own rather than the build's, at all 1,114,112 code points
//! but the surrogates, which no `Codepoint` is.

use baud_domain::{Codepoint, is_default_ignorable};

#[test]
fn every_code_point_is_default_ignorable_as_the_pinned_file_says() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/research/sources/unicode-derivedcoreproperties.txt"
    );
    let text = std::fs::read_to_string(path).expect("the pinned DerivedCoreProperties.txt");
    let mut has = vec![false; 0x11_0000];
    let mut rows = 0;
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let mut f = line.split(';').map(str::trim);
        let (Some(cps), Some("Default_Ignorable_Code_Point")) = (f.next(), f.next()) else {
            continue;
        };
        rows += 1;
        let (lo, hi) = cps.split_once("..").unwrap_or((cps, cps));
        let (lo, hi) = (
            usize::from_str_radix(lo, 16).unwrap(),
            usize::from_str_radix(hi, 16).unwrap(),
        );
        has[lo..=hi].iter_mut().for_each(|h| *h = true);
    }
    assert_eq!(rows, 27, "the file's Default_Ignorable_Code_Point rows");
    // The file's own total, `# Total code points: 4174`.
    assert_eq!(has.iter().filter(|&&h| h).count(), 4174);
    let wrong: Vec<u32> = (0u32..)
        .zip(&has)
        .filter_map(|(v, &want)| Codepoint::new(v).map(|c| (v, c, want)))
        .filter(|&(_, c, want)| is_default_ignorable(c) != want)
        .map(|(v, _, _)| v)
        .collect();
    assert!(
        wrong.is_empty(),
        "{} wrong, first {:X?}",
        wrong.len(),
        &wrong[..wrong.len().min(8)]
    );
}
