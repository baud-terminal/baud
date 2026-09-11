//! One lookup for everything the per-character path asks about a codepoint.
//!
//! **Two questions, one answer.** `Width::of` and
//! `GraphemeClass::of_with_lookback` are two two-stage tries over the *same*
//! `cp >> 8` stage-1 shape, so every non-ASCII character paid four loads and
//! 44,480 bytes of table to learn three small facts about one codepoint.
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

use crate::merged_data::{BMP_DIRECT, MERGED_BLOCKS, MERGED_INDEX, MERGED_PALETTE};
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
    /// **One load for the Basic Multilingual Plane.**
    ///
    /// Every other table here is two dependent loads -- a stage-1 index, then
    /// a block -- and this session measured three separate changes that removed
    /// work and lost because they *lengthened* the chain. The flat table is the
    /// opposite trade: 128 KB of rodata to make the lookup depth one.
    ///
    /// The size is affordable because only touched pages cost anything.
    /// `cat-cjk.raw` uses 3,175 distinct BMP codepoints, which is **113 cache
    /// lines -- 7.1 KB** -- and real text clusters by script, so this is the
    /// usual shape rather than a lucky one.
    ///
    /// Astral codepoints keep the two-stage path: they are 4% of `cat-cjk` and
    /// a flat table for them would be 2 MB.
    #[inline(always)]
    pub fn of(c: Codepoint) -> Classified {
        let cp = c.value();
        if cp < 0x10000 {
            let i = (cp as usize) << 1;
            let v = u16::from_le_bytes([BMP_DIRECT[i], BMP_DIRECT[i + 1]]);
            return Classified {
                class: GraphemeClass::FROM_INDEX[(v & 0xf) as usize],
                lookback: Lookback::from_bits(((v >> 4) & 0xf) as u8),
                code: (v >> 8) as u8,
            };
        }
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

    /// The same answer, from the *bytes*, so the first load issues sooner.
    ///
    /// **Not a micro-optimisation of the index arithmetic -- a shortening of
    /// the dependency chain.** For a three-byte sequence the stage-1 index is
    /// `cp >> 8`, and
    ///
    /// ```text
    /// cp >> 8   = (b0 & 0x0f) << 4 | (b1 & 0x3f) >> 2      only b0, b1
    /// cp & 0xff = ((b1 & 0x3f) & 3) << 6 | (b2 & 0x3f)     needs b2
    /// ```
    ///
    /// so the **table load does not depend on the third byte**. Taking a
    /// codepoint forces it to: `cp` is a function of all three, and the
    /// compiler must prove `(x | y) >> 8 == x >> 8` for `y < 256` to recover
    /// it. Handing over the parts says it outright, and the load can issue a
    /// step earlier.
    ///
    /// This matters because every measurement on this path says it is
    /// latency-bound: a vectorised decoder, a block decoder and a merged table
    /// each removed work and each lost, all by lengthening the chain.
    #[inline(always)]
    pub fn of_utf8_three(b0: u8, b1: u8, b2: u8) -> Classified {
        let hi = ((b0 & 0x0f) << 4) | ((b1 & 0x3f) >> 2);
        let lo = ((b1 & 0x03) << 6) | (b2 & 0x3f);
        let block = MERGED_INDEX[hi as usize] as usize;
        let id = MERGED_BLOCKS[block * 256 + lo as usize] as usize;
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
