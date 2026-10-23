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

use baud_domain::{
    AmbiguousWidth, Classified, Codepoint, GraphemeClass, UnicodeWidths, Width, WidthPolicy,
};

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
            let now = WidthPolicy::of(aw, UnicodeWidths::Current);
            assert!(
                m.width(now) == Width::of(c, aw),
                "width at U+{cp:04X} under {aw:?}: merged {} tries {}",
                m.width(now).cells(),
                Width::of(c, aw).cells()
            );
            assert!(
                m.width_current(WidthPolicy::of(aw, UnicodeWidths::Eight)) == Width::of(c, aw),
                "the current width at U+{cp:04X} under {aw:?}, Unicode 8 named"
            );
            let eight = WidthPolicy::of(aw, UnicodeWidths::Eight);
            assert!(
                m.width(eight) == Width::of_unicode8(c, aw),
                "Unicode 8 width at U+{cp:04X} under {aw:?}: merged {} tries {}",
                m.width(eight).cells(),
                Width::of_unicode8(c, aw).cells()
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
                m.width(WidthPolicy::DEFAULT).cells(),
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

/// The byte-indexed form must agree with the codepoint-indexed one for every
/// three-byte sequence, which is the case it exists for and 81% of cat-cjk.
#[test]
fn the_byte_indexed_form_agrees_for_every_three_byte_sequence() {
    let mut n = 0usize;
    for b0 in 0xe0u8..=0xef {
        for b1 in 0x80u8..=0xbf {
            for b2 in 0x80u8..=0xbf {
                let cp = (u32::from(b0 & 0x0f) << 12)
                    | (u32::from(b1 & 0x3f) << 6)
                    | u32::from(b2 & 0x3f);
                let Some(c) = Codepoint::new(cp) else {
                    continue;
                };
                let a = Classified::of(c);
                let b = Classified::of_utf8_three(b0, b1, b2);
                assert_eq!(a.class, b.class, "class for {b0:02x} {b1:02x} {b2:02x}");
                assert_eq!(
                    a.lookback, b.lookback,
                    "lookback for {b0:02x} {b1:02x} {b2:02x}"
                );
                assert!(
                    a.width(WidthPolicy::DEFAULT) == b.width(WidthPolicy::DEFAULT),
                    "width for {b0:02x} {b1:02x} {b2:02x}"
                );
                n += 1;
            }
        }
    }
    assert!(n > 60_000, "only checked {n} sequences");
}
