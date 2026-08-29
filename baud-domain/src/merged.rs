//! One lookup for everything the per-character path asks about a codepoint.
//!
//! **Two questions, one answer.** `Width::of` and
//! `GraphemeClass::of_with_lookback` are two two-stage tries over the *same*
//! `cp >> 8` stage-1 shape, so every non-ASCII character paid four loads and
//! 44,032 bytes of table to learn three small facts about one codepoint.
//!
//! Packing them into a single leaf byte does not work: the grapheme leaf
//! already spends all eight bits on four of class and four of lookback, and
//! width needs two more. A two-byte leaf grows the table to 67,328 -- 53%
//! *worse* -- because the block count rises as well.
//!
//! What works is a palette. Only **28** distinct `(class, lookback, width)`
//! combinations occur across all 1,114,112 codepoints, out of a possible 672,
//! so the leaf is a one-byte index into a table of 28 entries. Two loads,
//! 37,204 bytes, and the palette itself is 84 bytes and always hot.
//!
//! The width stored is the *code*, not a resolved `Width`: East Asian
//! Ambiguous depends on a policy the embedder owns, so a table cannot bake it
//! in. `[[source: uax11]]` § 5 is why that policy exists at all.

use crate::merged_data::{MERGED_BLOCKS, MERGED_INDEX, MERGED_PALETTE};
use crate::{AmbiguousWidth, Codepoint, GraphemeClass, Lookback, Width};

/// Everything the per-character path needs to know about one codepoint.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Classified {
    pub class: GraphemeClass,
    pub lookback: Lookback,
    /// 0 narrow, 1 zero, 2 wide, 3 East Asian Ambiguous.
    code: u8,
}

impl Classified {
    /// Both answers, from one two-stage lookup and a palette read.
    #[inline(always)]
    pub fn of(c: Codepoint) -> Classified {
        let cp = c.value();
        // ASCII never reaches the per-character path -- `put_ascii_run` takes
        // it -- but `Codepoint` allows it and the tries answer for it, so this
        // must too.
        let block = MERGED_INDEX[(cp >> 8) as usize] as usize;
        let id = MERGED_BLOCKS[block * 256 + (cp as usize & 0xff)] as usize;
        Classified {
            class: GraphemeClass::FROM_INDEX[MERGED_PALETTE[id * 3] as usize],
            lookback: Lookback::from_bits(MERGED_PALETTE[id * 3 + 1]),
            code: MERGED_PALETTE[id * 3 + 2],
        }
    }

    /// The column count, once the embedder's ambiguous-width policy is known.
    #[inline(always)]
    pub fn width(self, aw: AmbiguousWidth) -> Width {
        match self.code {
            1 => Width::ZERO,
            2 => Width::WIDE,
            3 => match aw {
                AmbiguousWidth::Wide => Width::WIDE,
                AmbiguousWidth::Narrow => Width::NARROW,
            },
            _ => Width::NARROW,
        }
    }
}
