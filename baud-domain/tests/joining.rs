//! **Cursive joining, held to the Unicode Standard's own examples**: rules
//! R1-R7 and L1-L3 (`[[source: unicode-ch9]]` 9.2.2 and 9.2.3) over the
//! types ArabicShaping.txt gives (`[[source: unicode-arabicshaping]]`).
//!
//! The rules are stated in display order; [`join`] takes them in logical
//! order, which the text allows ("right and left ... would become preceding
//! and following"). Each example below is the text's, written in the order
//! it is stored, and its expected forms are the text's read right to left.

use baud_domain::{
    FIRST_JOINING_BLOCK, JOIN_BLOCKS, JoiningForm, JoiningType, LamAlef, Ligating,
    in_joining_block, in_layout_block, in_rtl_block, join, lam_alefs,
};

const MEEM: u32 = 0x0645;
const SHADDA: u32 = 0x0651;
const LAM: u32 = 0x0644;
const ALEF: u32 = 0x0627;
const TATWEEL: u32 = 0x0640;
const FATHA: u32 = 0x064E;
const BEH: u32 = 0x0628;
const ZWJ: u32 = 0x200D;
const ZWNJ: u32 = 0x200C;

fn forms(cps: &[u32]) -> Vec<Option<JoiningForm>> {
    let types: Vec<JoiningType> = cps.iter().map(|&c| JoiningType::of(c)).collect();
    let mut out = vec![None; cps.len()];
    join(&types, &mut out);
    out
}

use JoiningForm::{Final, Initial, Isolated, Medial};

/// The generator regenerates both tables from the pinned files and
/// compares: `joining_data.rs` and baud-font's `presentation.rs` cannot
/// drift from ArabicShaping.txt, DerivedGeneralCategory.txt and
/// UnicodeData.txt, and its Joining_Type is held to DerivedJoiningType.txt
/// at every code point.
#[test]
fn joining_tables_match_the_pinned_data() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = std::process::Command::new("python3")
        .arg(format!("{root}/scripts/gen-joining-tables.py"))
        .arg("--check")
        .current_dir(root)
        .output()
        .expect("python3 to run the generator");
    assert!(
        out.status.success(),
        "the joining tables are stale -- rerun scripts/gen-joining-tables.py\n{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Table 9-3's examples, each type from the trie: and the header's
/// defaults, T for an unlisted mark or format control (fatha, ALM) and U
/// for an unlisted letter of another script; the isolates U, as UAX #9 3.5
/// says they are defined.
#[test]
fn each_type_is_the_table_s() {
    use JoiningType::*;
    let cases = [
        (0x0627, RightJoining, "alef"),
        (0x062F, RightJoining, "dal"),
        (0x0631, RightJoining, "reh"),
        (0x0628, DualJoining, "beh"),
        (0x062C, DualJoining, "jeem"),
        (0x200D, JoinCausing, "ZWJ"),
        (0x0640, JoinCausing, "tatweel"),
        (0x200C, NonJoining, "ZWNJ"),
        (0x0621, NonJoining, "hamza"),
        (0x0674, NonJoining, "high hamza"),
        (0x0600, NonJoining, "number sign"),
        (0x06DD, NonJoining, "end of ayah"),
        (0x064B, Transparent, "fathatan"),
        (0x0655, Transparent, "hamza below"),
        (0x0670, Transparent, "superscript alef"),
        (0x070F, Transparent, "Syriac abbreviation mark"),
        (0x061C, Transparent, "ALM, Cf and unlisted"),
        (0x0301, Transparent, "acute, Mn and unlisted"),
        (0x0041, NonJoining, "A"),
        (0x0020, NonJoining, "space"),
        (0x2066, NonJoining, "LRI"),
        (0x2069, NonJoining, "PDI"),
        (0xA872, LeftJoining, "Phags-pa superfixed letter RA"),
        (0x11_0000, NonJoining, "past the code space"),
    ];
    for (cp, want, name) in cases {
        assert_eq!(JoiningType::of(cp), want, "{name} U+{cp:04X}");
    }
}

/// The two groups the ligature rules name, from field 3: lam and its
/// variants, alef and its, and a letter in neither.
#[test]
fn the_lam_and_alef_groups_are_the_file_s() {
    for cp in [0x0644, 0x06B5, 0x06B8, 0x076A, 0x08A6] {
        assert_eq!(Ligating::of(cp), Ligating::Lam, "U+{cp:04X}");
    }
    for cp in [0x0622, 0x0623, 0x0625, 0x0627, 0x0671, 0x0870] {
        assert_eq!(Ligating::of(cp), Ligating::Alef, "U+{cp:04X}");
    }
    for cp in [0x0628, 0x0645, 0x0041, 0x0640] {
        assert_eq!(Ligating::of(cp), Ligating::Neither, "U+{cp:04X}");
    }
}

/// The block bits are the table's: every code point that takes a form is in
/// a set block, and a block with none is clear -- Latin, Hebrew and Han.
#[test]
fn the_block_bits_are_the_types() {
    for cp in 0..0x11_0000u32 {
        if matches!(
            JoiningType::of(cp),
            JoiningType::DualJoining | JoiningType::RightJoining | JoiningType::LeftJoining
        ) {
            assert!(in_joining_block(cp), "U+{cp:04X}");
        }
    }
    for cp in [0x0041, 0x05D0, 0x4E00, 0x1F600] {
        assert!(!in_joining_block(cp), "U+{cp:04X}");
    }
    assert!(in_joining_block(BEH) && JOIN_BLOCKS.iter().any(|&w| w != 0));
    // No code point below the first set block takes a form, and that block
    // is Arabic's.
    assert_eq!(FIRST_JOINING_BLOCK, 0x0600);
    for cp in 0..FIRST_JOINING_BLOCK {
        assert!(!in_joining_block(cp), "U+{cp:04X}");
    }
}

/// R1: "MEEMn + SHADDAn + LAMn -> MEEMr + SHADDAn + LAMl". Read right to
/// left, the meem comes first: it joins its following lam across the
/// shadda, so it is Xl (initial) and the lam Xr (final).
#[test]
fn r1_a_transparent_mark_does_not_break_the_join() {
    assert_eq!(
        forms(&[MEEM, SHADDA, LAM]),
        [Some(Initial), None, Some(Final)]
    );
}

/// R2: "ALEFn + TATWEELn -> ALEFr + TATWEELn": in display order the tatweel
/// is on the alef's right, so it precedes it: the alef joins it, and the
/// tatweel, join-causing, takes no form.
#[test]
fn r2_a_right_joining_letter_after_a_join_causer_is_final() {
    assert_eq!(forms(&[TATWEEL, ALEF]), [None, Some(Final)]);
}

/// R4: "TATWEELn + MEEMn + TATWEELn -> TATWEELn + MEEMm + TATWEELn".
#[test]
fn r4_a_dual_joining_letter_between_join_causers_is_medial() {
    assert_eq!(forms(&[TATWEEL, MEEM, TATWEEL]), [None, Some(Medial), None]);
}

/// R5: "MEEMn + TATWEELn -> MEEMr + TATWEELn" (the tatweel on its right,
/// before it) and R6: "TATWEELn + MEEMn -> TATWEELn + MEEMl" (on its left,
/// after it).
#[test]
fn r5_and_r6_one_side_joined() {
    assert_eq!(forms(&[TATWEEL, MEEM]), [None, Some(Final)]);
    assert_eq!(forms(&[MEEM, TATWEEL]), [Some(Initial), None]);
}

/// R7: a letter with nothing to join is Xn; a right-joining letter before a
/// letter does not join it, so the beh after an alef stands alone.
#[test]
fn r7_otherwise_non_joining() {
    assert_eq!(forms(&[BEH]), [Some(Isolated)]);
    assert_eq!(
        forms(&[BEH, BEH, BEH]),
        [Some(Initial), Some(Medial), Some(Final)]
    );
    assert_eq!(forms(&[ALEF, BEH]), [Some(Isolated), Some(Isolated)]);
    assert_eq!(
        forms(&[BEH, ALEF, BEH]),
        [Some(Initial), Some(Final), Some(Isolated)]
    );
}

/// ZWJ causes joining and ZWNJ prevents it (9.2.2, "the ZERO WIDTH
/// NON-JOINER may be used to prevent joining"): a beh followed by ZWJ is
/// initial, one before ZWNJ stands alone, and a ZWNJ between two behs
/// breaks the word in two.
#[test]
fn zwj_joins_and_zwnj_breaks() {
    assert_eq!(forms(&[BEH, ZWJ]), [Some(Initial), None]);
    assert_eq!(forms(&[ZWJ, BEH]), [None, Some(Final)]);
    assert_eq!(
        forms(&[BEH, ZWNJ, BEH]),
        [Some(Isolated), None, Some(Isolated)]
    );
    // The positive control for the ZWNJ: without it the two join.
    assert_eq!(forms(&[BEH, BEH]), [Some(Initial), Some(Final)]);
}

/// L1-L3: a lam joined to its following alef forms the ligature, across a
/// mark (L1); with a medial lam it is (LAM-ALEF)r (L2), with an initial one
/// (LAM-ALEF)n (L3). A lam not joined forward forms none, nor does a lam
/// before a letter that is not an alef.
#[test]
fn the_lam_alef_ligatures_are_l1_to_l3() {
    let found = |cps: &[u32]| {
        let f = forms(cps);
        let mut out = Vec::new();
        lam_alefs(cps, &f, |i, j, k| out.push((i, j, k)));
        out
    };
    assert_eq!(found(&[LAM, ALEF]), [(0, 1, LamAlef::Nominal)]);
    assert_eq!(found(&[BEH, LAM, ALEF]), [(1, 2, LamAlef::Joined)]);
    assert_eq!(found(&[LAM, FATHA, ALEF]), [(0, 2, LamAlef::Nominal)]);
    // "salam": seen, lam, alef, meem -- the lam is medial.
    assert_eq!(found(&[0x0633, LAM, ALEF, MEEM]), [(1, 2, LamAlef::Joined)]);
    assert_eq!(found(&[LAM, ZWNJ, ALEF]), []);
    assert_eq!(found(&[LAM, BEH]), []);
    assert_eq!(found(&[ALEF, LAM]), []);
}

/// The layout's blocks are the two tables' together: a code point's block
/// is set exactly when its right-to-left bit or its joining bit is, over
/// the whole code space and past it -- Arabic in both, Mongolian joining
/// alone, Hebrew right to left alone, Latin, Han and box drawing in
/// neither.
#[test]
fn the_layout_blocks_are_both_tables() {
    for cp in (0..0x11_0000u32).chain([0x11_0000, 0x7FFF_FFFF, u32::MAX]) {
        assert_eq!(
            in_layout_block(cp),
            in_rtl_block(cp) || in_joining_block(cp),
            "U+{cp:04X}"
        );
    }
    for cp in [0x0628, 0x1820, 0x05D0] {
        assert!(in_layout_block(cp), "U+{cp:04X}");
    }
    assert!(in_joining_block(0x1820) && !in_rtl_block(0x1820));
    assert!(in_rtl_block(0x05D0) && !in_joining_block(0x05D0));
    for cp in [0x0041, 0x4E00, 0x2500, 0x2800] {
        assert!(!in_layout_block(cp), "U+{cp:04X}");
    }
}
