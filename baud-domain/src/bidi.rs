//! **The bidirectional character properties** of the Unicode Character
//! Database 17.0.0, the ones UAX #9 revision 51 (`[[source: uax9-bidi]]`)
//! reads: Bidi_Class, Bidi_Mirrored, Bidi_Mirroring_Glyph,
//! Bidi_Paired_Bracket and Bidi_Paired_Bracket_Type. Each is one indexed
//! load from a table `scripts/gen-bidi-tables.py` compiles from the pinned
//! files (`bidi_data.rs`); `bidi_tables_match_the_pinned_data` holds the
//! committed table to them, and
//! `every_code_point_has_the_bidi_class_the_pinned_file_gives` reads every
//! code point back against `DerivedBidiClass.txt`.

mod algorithm;

pub use algorithm::*;

use crate::bidi_data::{BIDI_BLOCKS, BIDI_INDEX, MIRROR_BLOCKS, MIRROR_GLYPHS, MIRROR_INDEX};

/// A Bidi_Class value, in UAX #9 Table 4's order, which is the table's,
/// pinned by `the_bidi_trie_agrees_with_the_class_enum`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(clippy::upper_case_acronyms)]
pub enum BidiClass {
    /// Left-to-right.
    L,
    /// Right-to-left.
    R,
    /// Right-to-left Arabic.
    AL,
    /// European number.
    EN,
    /// European number separator.
    ES,
    /// European number terminator.
    ET,
    /// Arabic number.
    AN,
    /// Common number separator.
    CS,
    /// Nonspacing mark.
    NSM,
    /// Boundary neutral.
    BN,
    /// Paragraph separator.
    B,
    /// Segment separator.
    S,
    /// Whitespace.
    WS,
    /// Other neutrals.
    ON,
    /// Left-to-right embedding.
    LRE,
    /// Left-to-right override.
    LRO,
    /// Right-to-left embedding.
    RLE,
    /// Right-to-left override.
    RLO,
    /// Pop directional format.
    PDF,
    /// Left-to-right isolate.
    LRI,
    /// Right-to-left isolate.
    RLI,
    /// First strong isolate.
    FSI,
    /// Pop directional isolate.
    PDI,
}

use BidiClass::*;

impl BidiClass {
    /// Every class, in the table's order.
    pub const ALL: [BidiClass; 23] = [
        L, R, AL, EN, ES, ET, AN, CS, NSM, BN, B, S, WS, ON, LRE, LRO, RLE, RLO, PDF, LRI, RLI,
        FSI, PDI,
    ];

    /// The Bidi_Class of `c`.
    pub fn of(c: char) -> BidiClass {
        BidiProps::of(c).class
    }

    /// The class by its UCD short name, `"NSM"` for [`BidiClass::NSM`]:
    /// the spelling of `DerivedBidiClass.txt` and `BidiTest.txt`.
    pub fn from_name(name: &str) -> Option<BidiClass> {
        Some(match name {
            "L" => L,
            "R" => R,
            "AL" => AL,
            "EN" => EN,
            "ES" => ES,
            "ET" => ET,
            "AN" => AN,
            "CS" => CS,
            "NSM" => NSM,
            "BN" => BN,
            "B" => B,
            "S" => S,
            "WS" => WS,
            "ON" => ON,
            "LRE" => LRE,
            "LRO" => LRO,
            "RLE" => RLE,
            "RLO" => RLO,
            "PDF" => PDF,
            "LRI" => LRI,
            "RLI" => RLI,
            "FSI" => FSI,
            "PDI" => PDI,
            _ => return None,
        })
    }
}

/// Bidi_Mirrored: whether the character's glyph is mirrored where it is
/// displayed right to left (UAX #9 L4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BidiMirrored {
    Yes,
    No,
}

/// Bidi_Paired_Bracket_Type, but for its value None, which is the absence
/// of one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BracketKind {
    Open,
    Close,
}

/// A paired bracket: which way it faces, and the pair it belongs to. Two
/// brackets are one pair when their `pair` is equal, which is BD16's
/// comparison with its canonical equivalence folded in: U+2329 and U+232A
/// are in U+3008 and U+3009's pair.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairedBracket {
    pub kind: BracketKind,
    pub pair: BracketPair,
}

/// One of the 63 bracket pairs BidiBrackets.txt defines, numbered from 1.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BracketPair(u8);

impl BracketPair {
    /// The pair's number, 1 to 63.
    pub fn number(self) -> u8 {
        self.0
    }
}

/// What the class table says of one character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BidiProps {
    pub class: BidiClass,
    pub mirrored: BidiMirrored,
    /// Bidi_Paired_Bracket_Type; `None` for its value None.
    pub bracket: Option<BracketKind>,
}

impl BidiProps {
    /// The properties of `c`: one load from each stage of the table.
    pub fn of(c: char) -> BidiProps {
        let cp = u32::from(c);
        let block = BIDI_INDEX[usize::try_from(cp >> 8).unwrap_or(0)];
        let at = usize::from(block) * 256 + usize::try_from(cp & 0xFF).unwrap_or(0);
        let byte = BIDI_BLOCKS[at];
        BidiProps {
            class: BidiClass::ALL[usize::from(byte & 0x1F)],
            mirrored: match byte & 0x20 {
                0 => BidiMirrored::No,
                _ => BidiMirrored::Yes,
            },
            bracket: match byte >> 6 {
                1 => Some(BracketKind::Open),
                2 => Some(BracketKind::Close),
                _ => None,
            },
        }
    }
}

/// **Whether `cp` can make a left-to-right paragraph reorder**: a
/// character of class R, AL or AN, or an RLE, RLO or RLI. Without one, a
/// paragraph at level 0 resolves every character to an even level -- EN
/// becomes L by W7 with an L start of sequence, LRE, LRO and LRI raise to
/// even levels only, and FSI resolves left to right with no R or AL inside
/// it -- so L2 reverses nothing and L4 mirrors nothing. AN is in the set
/// because N1 resolves a neutral between two ANs to R. UAX #9 section 5.1
/// names the test: "An example of an effective optimization is to first
/// test for right-to-left characters and invoke the Bidirectional Algorithm
/// only if they are present."
///
/// **One load for nearly every character.** Below U+0590 nothing is in the
/// set, and above it a bit a 256-character block, compiled from the class
/// table ([`RTL_BLOCKS`]), says whether the block holds any; only a
/// character in such a block reads its class.
#[inline]
pub fn opens_rtl(cp: u32) -> bool {
    if !in_rtl_block(cp) {
        return false;
    }
    char::from_u32(cp).is_some_and(|c| {
        matches!(
            BidiClass::of(c),
            BidiClass::R
                | BidiClass::AL
                | BidiClass::AN
                | BidiClass::RLE
                | BidiClass::RLO
                | BidiClass::RLI
        )
    })
}

/// Whether `cp`'s 256-character block holds a character [`opens_rtl`]
/// answers yes for: one load from [`RTL_BLOCKS`], and no for any word past
/// U+10FFFF. A row whose every cell's block answers no needs no class read.
#[inline]
pub fn in_rtl_block(cp: u32) -> bool {
    let block = usize::try_from(cp >> 8).unwrap_or(usize::MAX);
    RTL_BLOCKS
        .get(block >> 6)
        .is_some_and(|w| w >> (block & 63) & 1 == 1)
}

/// A bit for each 256-character block of the code space, set where the
/// block holds a character [`opens_rtl`] answers yes for: compiled from the
/// class table at build time, one test of each distinct block's 256
/// entries.
pub static RTL_BLOCKS: [u64; 68] = rtl_blocks();

pub(crate) const fn rtl_blocks() -> [u64; 68] {
    let distinct = BIDI_BLOCKS.len() / 256;
    let mut rtl = [false; 256];
    let mut b = 0;
    while b < distinct {
        let mut i = 0;
        while i < 256 {
            let class = BIDI_BLOCKS[b * 256 + i] & 0x1F;
            // R, AL, AN, RLE, RLO and RLI in Table 4's order (`BidiClass`).
            if matches!(class, 1 | 2 | 6 | 16 | 17 | 20) {
                rtl[b] = true;
            }
            i += 1;
        }
        b += 1;
    }
    let mut out = [0u64; 68];
    let mut k = 0;
    while k < BIDI_INDEX.len() {
        if rtl[BIDI_INDEX[k] as usize] {
            out[k / 64] |= 1 << (k % 64);
        }
        k += 1;
    }
    out
}

/// The mirror table's entry for `c`: its glyph's index and its pair.
fn mirror_entry(c: char) -> u16 {
    let cp = u32::from(c);
    let block = MIRROR_INDEX[usize::try_from(cp >> 8).unwrap_or(0)];
    MIRROR_BLOCKS[usize::from(block) * 256 + usize::try_from(cp & 0xFF).unwrap_or(0)]
}

/// Bidi_Mirroring_Glyph: the character whose glyph is `c`'s mirrored, or
/// `None` where the UCD names none. A character can be Bidi_Mirrored with
/// no such glyph (U+2211 N-ARY SUMMATION), and then the mirrored glyph is
/// the font's to supply.
pub fn mirroring_glyph(c: char) -> Option<char> {
    match usize::from(mirror_entry(c) & 0x1FF) {
        0 => None,
        i => char::from_u32(MIRROR_GLYPHS[i]),
    }
}

/// `c` as a paired bracket, or `None` where Bidi_Paired_Bracket_Type is
/// None. Bidi_Paired_Bracket itself is [`mirroring_glyph`]'s answer for
/// every bracket, which the generator asserts.
pub fn paired_bracket(c: char) -> Option<PairedBracket> {
    let kind = BidiProps::of(c).bracket?;
    match mirror_entry(c) >> 9 {
        0 => None,
        n => Some(PairedBracket {
            kind,
            pair: BracketPair(u8::try_from(n).ok()?),
        }),
    }
}
