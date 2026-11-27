//! What a syllabic shaper keeps of each glyph: the class its character has
//! in a syllable, where in the syllable its glyph goes, which features may
//! substitute it, which syllable it belongs to and what substitution made
//! it. [`crate::indic`] writes and reads these; every other run carries
//! them unread.
//!
//! **They travel with the glyph** because the shaper reads them again after
//! substitution has replaced, joined and multiplied glyphs, as the
//! specification's final reordering does: "After the localized forms and
//! basic shaping forms GSUB features have been applied ... the shaping
//! engine performs some final glyph reordering"
//! (`[[source: ms-devanagari-script-development]]`, Reorder characters). A
//! ligature keeps its first component's, as a glyph's cells do, and each
//! glyph of a multiple substitution its source's.

use crate::indic_data::{DENSE, DENSE_FIRST, LATIN1, SPARSE, SPARSE_BLOCKS};
use baud_domain::Codepoint;

/// A character's class in an Indic syllable: the specification's C, V, N,
/// H, ZWNJ, ZWJ, M, SM, VD and A, and the finer ones a shaper needs beside
/// them (`[[source: harfbuzz-14-5-1-shaper-indic-machine]]`). The numbers
/// are the compiled table's (`build/indic.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Category(u8);

impl Category {
    /// Not part of an Indic syllable.
    pub const X: Category = Category(0);
    /// A consonant.
    pub const C: Category = Category(1);
    /// An independent vowel.
    pub const V: Category = Category(2);
    /// A nukta.
    pub const N: Category = Category(3);
    /// A halant (virama).
    pub const H: Category = Category(4);
    pub const ZWNJ: Category = Category(5);
    pub const ZWJ: Category = Category(6);
    /// A matra (dependent vowel sign).
    pub const M: Category = Category(7);
    /// A syllable modifier: a bindu, a visarga.
    pub const SM: Category = Category(8);
    /// A cantillation or Vedic sign.
    pub const A: Category = Category(9);
    /// A base that is no letter: NO-BREAK SPACE, a digit, a dash.
    pub const PLACEHOLDER: Category = Category(10);
    /// U+25CC DOTTED CIRCLE.
    pub const DOTTED_CIRCLE: Category = Category(11);
    /// A register shifter.
    pub const RS: Category = Category(12);
    /// A matra that follows a syllable modifier.
    pub const MPST: Category = Category(13);
    /// A repha encoded on its own.
    pub const REPHA: Category = Category(14);
    /// The letter RA, which can become reph.
    pub const RA: Category = Category(15);
    /// A medial consonant.
    pub const CM: Category = Category(16);
    /// A symbol that takes marks, as an avagraha does.
    pub const SYMBOL: Category = Category(17);
    /// A consonant with a stacker.
    pub const CS: Category = Category(18);
    /// A syllable modifier with no position of its own.
    pub const SMPST: Category = Category(19);

    /// The set holding this class alone.
    pub(crate) const fn set(self) -> Categories {
        Categories(1 << self.0)
    }
}

/// A set of [`Category`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Categories(u32);

impl Categories {
    pub(crate) const fn or(self, c: Category) -> Categories {
        Categories(self.0 | c.set().0)
    }

    pub(crate) const fn has(self, c: Category) -> bool {
        self.0 & c.set().0 != 0
    }
}

/// Where a glyph goes in its syllable, left to right: the order the
/// initial reordering sorts by (`[[source: harfbuzz-14-5-1-shaper-indic-h]]`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Position(u8);

impl Position {
    pub const START: Position = Position(0);
    pub const RA_TO_BECOME_REPH: Position = Position(1);
    pub const PRE_M: Position = Position(2);
    pub const PRE_C: Position = Position(3);
    pub const BASE_C: Position = Position(4);
    pub const AFTER_MAIN: Position = Position(5);
    pub const ABOVE_C: Position = Position(6);
    pub const BEFORE_SUB: Position = Position(7);
    pub const BELOW_C: Position = Position(8);
    pub const AFTER_SUB: Position = Position(9);
    pub const BEFORE_POST: Position = Position(10);
    pub const POST_C: Position = Position(11);
    pub const AFTER_POST: Position = Position(12);
    pub const SMVD: Position = Position(13);
    pub const END: Position = Position(14);
}

/// Which of the masked features may substitute a glyph: the shaper sets
/// these per glyph, and a feature's lookups start only on a glyph that
/// carries its mask. Every glyph carries [`Masks::GLOBAL`], which the
/// features applied to all glyphs name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Masks(u8);

impl Masks {
    pub const NONE: Masks = Masks(0);
    pub const RPHF: Masks = Masks(1);
    pub const PREF: Masks = Masks(2);
    pub const BLWF: Masks = Masks(4);
    pub const ABVF: Masks = Masks(8);
    pub const HALF: Masks = Masks(16);
    pub const PSTF: Masks = Masks(32);
    pub const INIT: Masks = Masks(64);
    pub const GLOBAL: Masks = Masks(128);

    pub const fn or(self, o: Masks) -> Masks {
        Masks(self.0 | o.0)
    }

    pub const fn without(self, o: Masks) -> Masks {
        Masks(self.0 & !o.0)
    }

    /// The masks both hold.
    pub const fn and(self, o: Masks) -> Masks {
        Masks(self.0 & o.0)
    }

    /// Whether the two share a mask.
    pub const fn meets(self, o: Masks) -> bool {
        self.0 & o.0 != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// What substitution made a glyph: any, a ligature, a multiple
/// substitution -- HarfBuzz's substituted, ligated and multiplied glyph
/// properties, which its final reordering reads
/// (`[[source: harfbuzz-14-5-1-shaper-indic]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Formed(u8);

impl Formed {
    pub const NONE: Formed = Formed(0);
    pub const SUBSTITUTED: Formed = Formed(1);
    pub const LIGATED: Formed = Formed(2);
    pub const MULTIPLIED: Formed = Formed(4);

    pub const fn or(self, o: Formed) -> Formed {
        Formed(self.0 | o.0)
    }

    pub const fn without(self, o: Formed) -> Formed {
        Formed(self.0 & !o.0)
    }

    pub const fn has(self, o: Formed) -> bool {
        self.0 & o.0 == o.0 && o.0 != 0
    }

    /// A ligature that no multiple substitution has since taken apart.
    pub const fn ligated_alone(self) -> bool {
        self.has(Formed::LIGATED) && !self.has(Formed::MULTIPLIED)
    }
}

/// Which syllable a glyph is in and what kind it is: a serial that differs
/// from its neighbours' in the high four bits, the kind in the low four,
/// zero outside any syllable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Syllable(u8);

/// The kinds of syllable the grammar finds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Consonant,
    Vowel,
    Standalone,
    Symbol,
    Broken,
    NonIndic,
}

/// A place in a run's glyphs: the index of a glyph in the slice a
/// syllabic shaper reorders.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(crate) struct At(pub(crate) usize);

impl Syllable {
    pub const NONE: Syllable = Syllable(0);

    /// Syllable `serial`, of 1 to 15, of `kind`.
    pub(crate) fn new(serial: Serial, kind: Kind) -> Syllable {
        let code = match kind {
            Kind::Consonant => 0,
            Kind::Vowel => 1,
            Kind::Standalone => 2,
            Kind::Symbol => 3,
            Kind::Broken => 4,
            Kind::NonIndic => 5,
        };
        Syllable(serial.0 << 4 | code)
    }

    pub(crate) fn kind(self) -> Kind {
        match self.0 & 15 {
            0 => Kind::Consonant,
            1 => Kind::Vowel,
            2 => Kind::Standalone,
            3 => Kind::Symbol,
            4 => Kind::Broken,
            _ => Kind::NonIndic,
        }
    }

    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// A glyph's place in its syllable before a sort, held here for the
    /// sort's length, as HarfBuzz holds it
    /// (`initial_reordering_consonant_syllable()`); at most 127.
    pub(crate) fn scratch(k: At) -> Syllable {
        Syllable(u8::try_from(k.0.min(127)).unwrap_or(127))
    }

    /// The place [`Syllable::scratch`] held.
    pub(crate) fn scratch_index(self) -> At {
        At(usize::from(self.0))
    }

    /// A place already followed round its cycle.
    pub(crate) const VISITED: Syllable = Syllable(255);

    pub(crate) fn is_visited(self) -> bool {
        self == Syllable::VISITED
    }
}

/// A syllable's serial: 1 to 15, then 1 again, so two neighbouring
/// syllables never share one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Serial(u8);

impl Serial {
    pub(crate) const FIRST: Serial = Serial(1);

    pub(crate) fn next(self) -> Serial {
        match self.0 {
            15.. => Serial(1),
            n => Serial(n + 1),
        }
    }
}

/// A glyph's syllabic properties.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Syllabic {
    pub category: Category,
    pub position: Position,
    pub masks: Masks,
    pub syllable: Syllable,
    pub formed: Formed,
}

impl Syllabic {
    /// A glyph's before any syllabic shaper has looked at it.
    pub const NONE: Syllabic = Syllabic {
        category: Category::X,
        position: Position::END,
        masks: Masks::GLOBAL,
        syllable: Syllable::NONE,
        formed: Formed::NONE,
    };

    /// The class and position the generated table gives `c`
    /// (`build/indic.rs`), X at the end for a character it does not class.
    pub fn of(c: Codepoint) -> Syllabic {
        let v = c.value();
        // Two bytes a character in the dense tables.
        let dense = |first: u32, table: &'static [u8]| {
            let i = usize::try_from(v.checked_sub(first)?).ok()?;
            Some((*table.get(2 * i)?, *table.get(2 * i + 1)?))
        };
        // Most characters of most runs are in no block the sparse table
        // holds: a Han or Latin letter is answered by a block test.
        let found = dense(0, LATIN1)
            .or_else(|| dense(DENSE_FIRST, DENSE))
            .or_else(|| {
                SPARSE_BLOCKS
                    .contains(&(v >> 8))
                    .then(|| Syllabic::sparse(c))
                    .flatten()
            });
        match found {
            Some((c, p)) => Syllabic {
                category: Category(c),
                position: Position(p),
                ..Syllabic::NONE
            },
            None => Syllabic::NONE,
        }
    }

    /// `v`'s class and position in [`SPARSE`]'s five-byte records, by
    /// binary search.
    fn sparse(c: Codepoint) -> Option<(u8, u8)> {
        let v = c.value();
        let record = |k: usize| SPARSE.get(5 * k..5 * k + 5);
        let at = |k: usize| record(k).map(|r| u32::from_be_bytes([0, r[0], r[1], r[2]]));
        let (mut lo, mut hi) = (0usize, SPARSE.len() / 5);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match at(mid)?.cmp(&v) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => return record(mid).map(|r| (r[3], r[4])),
            }
        }
        None
    }
}
