//! The bulk collector's shortcut past the Unicode tries, checked against them.
//!
//! `Codepoint::is_wide_ordinary` lets the wide collector skip `Width::of` and
//! `GraphemeClass::of_with_lookback` for four ranges of codepoints, on the
//! claim that all three properties are constant across them. A hand-written
//! range list asserting that is exactly the "guessed table" this repo refuses,
//! so it is not asserted -- it is enumerated against the same generated tables
//! the slow path consults.
//!
//! **The direction matters.** The predicate is allowed to be incomplete: a
//! codepoint it declines merely takes the slow path and is written correctly.
//! It must never be wrong, because a codepoint it wrongly admits skips both
//! tables and is written with the wrong width and no cluster breaking. So the
//! test is over every codepoint three bytes can encode, and it checks the
//! implication in the direction that can do damage.

use baud_domain::{AmbiguousWidth, Codepoint, GraphemeClass, Width};

fn agrees(v: u32) -> bool {
    match Codepoint::new(v) {
        Some(cp) => {
            let (class, lb) = GraphemeClass::of_with_lookback(cp);
            Width::of(cp, AmbiguousWidth::Narrow) == Width::WIDE
                && class == GraphemeClass::Other
                && lb.is_none()
        }
        None => false,
    }
}

#[test]
fn the_wide_ordinary_ranges_agree_with_the_tables() {
    let mut admitted = 0usize;
    for v in 0x0800u32..=0xffffu32 {
        let Some(cp) = Codepoint::new(v) else {
            continue;
        };
        if cp.is_wide_ordinary() {
            admitted += 1;
            assert!(
                agrees(v),
                "U+{v:04X} is admitted by is_wide_ordinary but the tables \
                 disagree -- it would be written with the wrong width and \
                 skip cluster breaking"
            );
        }
    }
    // A positive control. Without it the test passes for a predicate that
    // admits nothing, which is the shape of a probe that cannot fire.
    assert!(
        admitted > 29_000,
        "only {admitted} codepoints admitted; the predicate has narrowed and \
         the fast path it exists for is no longer being taken"
    );
}

#[test]
fn the_two_kana_sound_marks_are_excluded() {
    // U+3099 and U+309A are combining marks sitting between hiragana and
    // katakana, and they are the reason those are two ranges rather than one.
    // If they were ever admitted, a `か` followed by one would render as two
    // cells instead of a single voiced `が`.
    for v in [0x3099u32, 0x309a] {
        let cp = Codepoint::new(v).expect("valid");
        assert!(
            !cp.is_wide_ordinary(),
            "U+{v:04X} is a combining sound mark and must take the slow path"
        );
    }
    // And the characters on both sides of them are admitted, so the exclusion
    // is a two-codepoint hole rather than a range that quietly stops early.
    for v in [0x3096u32, 0x309b] {
        let cp = Codepoint::new(v).expect("valid");
        assert!(cp.is_wide_ordinary(), "U+{v:04X} should be admitted");
    }
}
