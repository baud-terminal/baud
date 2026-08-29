//! The merged table must answer exactly what the two tries answer.
//!
//! It is generated *from* them, so this is not independent evidence that the
//! properties are right -- `width_tables_match_the_pinned_data` and the
//! `GraphemeBreakTest.txt` suite are. What it rules out is the merge itself
//! going wrong: a palette id colliding, a block deduplicating two blocks that
//! differ, the ambiguous-width code being resolved the wrong way round.
//!
//! Exhaustive rather than sampled, because a trie is exactly the shape where a
//! sampled test passes and one block is wrong.

use baud_domain::{AmbiguousWidth, Classified, Codepoint, GraphemeClass, Width};

#[test]
fn the_merged_table_agrees_with_both_tries() {
    let mut checked = 0usize;
    for cp in 0..0x110000u32 {
        let Some(c) = Codepoint::new(cp) else {
            continue;
        };
        let m = Classified::of(c);
        let (cl, lb) = GraphemeClass::of_with_lookback(c);
        assert_eq!(m.class, cl, "class at U+{cp:04X}");
        assert_eq!(m.lookback, lb, "lookback at U+{cp:04X}");
        for aw in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
            assert!(
                m.width(aw) == Width::of(c, aw),
                "width at U+{cp:04X} under {aw:?}: merged {} tries {}",
                m.width(aw).cells(),
                Width::of(c, aw).cells()
            );
        }
        checked += 1;
    }
    assert!(checked > 1_100_000, "only checked {checked}");
}

/// The palette is small enough to stay hot, which is the whole reason the
/// merge is smaller rather than larger.
#[test]
fn the_palette_is_small_enough_to_stay_resident() {
    let mut seen = std::collections::HashSet::new();
    for cp in 0..0x110000u32 {
        if let Some(c) = Codepoint::new(cp) {
            let m = Classified::of(c);
            seen.insert((
                m.class as u8,
                m.lookback.bits(),
                m.width(AmbiguousWidth::Narrow).cells(),
            ));
        }
    }
    assert!(
        seen.len() <= 32,
        "palette grew to {} distinct combinations; the one-byte leaf assumes \
         it stays under 256 and the size win assumes it stays tiny",
        seen.len()
    );
}
