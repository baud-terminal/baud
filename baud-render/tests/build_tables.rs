//! **Each check the stress build makes on the pinned files fires**:
//! `build/stress.rs`, run here on its files with one row changed, reports
//! the failure that change must cause, and reports nothing on the files as
//! pinned: a code point the general categories leave out, a group or an
//! unknown value given as one, a file of another version, a decomposition
//! that is not code points, each positive control of the skip set and of
//! the combining set, and the separator and block counts. The module only
//! computes: it reads through the function it is given and writes
//! nothing, so running it here touches no `OUT_DIR`.

#[path = "../build/stress.rs"]
mod stress;

use baud_tablegen::Kind;

fn pinned(file: &str) -> std::io::Result<String> {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/sources");
    std::fs::read_to_string(format!("{dir}/{file}"))
}

/// The failures of the build when `file` has its first `find` replaced.
fn failures_of(file: &str, find: &str, replace: &str) -> Vec<(Kind, String)> {
    let read = |f: &str| {
        let text = pinned(f)?;
        if f != file {
            return Ok(text);
        }
        assert!(text.contains(find), "{file} has no {find:?}");
        Ok(text.replacen(find, replace, 1))
    };
    match stress::build(&read) {
        Ok(_) => Vec::new(),
        Err(errs) => errs
            .list()
            .iter()
            .map(|e| (e.kind, e.to_string()))
            .collect(),
    }
}

const GC: &str = "unicode-derivedgeneralcategory.txt";
const DATA: &str = "unicode-unicodedata.txt";

#[test]
fn the_pinned_files_build_the_stress_tables() {
    let emit = stress::build(&pinned).unwrap_or_else(|e| panic!("{e}"));
    let names: Vec<String> = emit.files().into_iter().map(|(f, _)| f).collect();
    assert_eq!(
        names,
        [
            "stress.rs",
            "stress_skip_index.bin",
            "stress_skip_blocks.bin",
            "combining_index.bin",
            "combining_blocks.bin",
        ]
    );
    let rs = emit.rust();
    assert!(rs.contains("pub static STRESS_SKIP_BLOCKS: &[u8; 4704]"));
    assert!(rs.contains("pub static SEPARATORS: [u32; 19] = ["));
    assert!(rs.contains("0x0020, 0x00A0, 0x1680,"));
    assert!(stress::INPUTS.iter().all(|i| pinned(i.file).is_ok()));
}

#[test]
fn each_check_the_stress_build_makes_fires() {
    let cases: &[(&str, &str, &str, Kind, &str)] = &[
        (
            GC,
            "002C          ; Po #       COMMA\n",
            "",
            Kind::Invariant,
            "U+002C has no value",
        ),
        (
            GC,
            "002C          ; Po #",
            "002C          ; P  #",
            Kind::UnknownValue,
            "group of categories",
        ),
        (
            GC,
            "002C          ; Po #",
            "002C          ; Qq #",
            Kind::UnknownValue,
            "Qq",
        ),
        (
            GC,
            "# DerivedGeneralCategory-17.0.0.txt",
            "# DerivedGeneralCategory-16.0.0.txt",
            Kind::Header,
            "line 1",
        ),
        (
            GC,
            "002C          ; Po #",
            "002C          ; Lo #",
            Kind::Control,
            "U+002C is skipped: false, true expected",
        ),
        (
            DATA,
            "FF03;FULLWIDTH NUMBER SIGN;Po;0;ET;<wide> 0023;",
            "FF03;FULLWIDTH NUMBER SIGN;Po;0;ET;<wide> 0024;",
            Kind::Control,
            "U+FF03 is skipped: true, false expected",
        ),
        (
            DATA,
            "FF03;FULLWIDTH NUMBER SIGN;Po;0;ET;<wide> 0023;",
            "FF03;FULLWIDTH NUMBER SIGN;Po;0;ET;<wide> 00ZZ;",
            Kind::Malformed,
            "data/unicode-unicodedata.txt:",
        ),
        (
            GC,
            "0300..036F    ; Mn #",
            "0300          ; Mn\n0301          ; Lo\n0302..036F    ; Mn #",
            Kind::Control,
            "U+0301 is combining: false, true expected",
        ),
        (
            GC,
            "200B..200F    ; Cf #",
            "200B..200C    ; Cf\n200D          ; Mn\n200E..200F    ; Cf #",
            Kind::Control,
            "U+200D is combining: true, false expected",
        ),
        (
            GC,
            "1680          ; Zs #",
            "1680          ; Po #",
            Kind::Count,
            "18 separators, 19 expected",
        ),
        (
            GC,
            "4E00..A014    ; Lo #",
            "4E00..5010    ; Lo\n5011          ; Po\n5012..50ED    ; Lo\n50EE          ; Po\n50EF..A014    ; Lo #",
            Kind::Count,
            "148 stress skip blocks, 147 expected",
        ),
        (
            GC,
            "4E00..A014    ; Lo #",
            "4E00..5010    ; Lo\n5011          ; Mn\n5012..50ED    ; Lo\n50EE          ; Mn\n50EF..A014    ; Lo #",
            Kind::Count,
            "76 combining blocks, 75 expected",
        ),
    ];
    assert!(failures_of(GC, "# ", "# ").is_empty(), "the pinned files");
    for (file, find, replace, kind, says) in cases {
        let got = failures_of(file, find, replace);
        assert!(
            got.iter().any(|(k, m)| k == kind && m.contains(says)),
            "{file}: {find:?} -> {replace:?} should report {kind:?} {says:?}, got {got:?}"
        );
    }
}

/// The bit trie reads back what it was built from, and numbers blocks in
/// the order they are first seen: an empty block, then the one with
/// U+0100 set.
#[test]
fn the_bit_trie_reads_back_its_set() {
    let mut set = vec![false; baud_tablegen::CODE_POINTS];
    set[0x100] = true;
    set[0x10_FF00] = true;
    let trie = stress::bit_trie("test", &set).expect("a trie");
    assert_eq!(trie.distinct, 2);
    assert_eq!(&trie.index[..3], &[0, 1, 0]);
    assert_eq!(trie.index[0x10FF], 1);
    assert_eq!(trie.blocks.len(), 64);
    assert_eq!(trie.blocks[32], 1, "U+0100 is bit 0 of its block's byte 0");
}
