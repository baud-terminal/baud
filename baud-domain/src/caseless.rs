//! **Default caseless matching**, as a font family name is compared.
//!
//! CSS Fonts 3 section 5.1 (`[[source: css-fonts-3]]`): "User agents must
//! match these names case insensitively, using the \"Default Caseless
//! Matching\" algorithm ... Specifically, the algorithm must be applied
//! without normalizing the strings involved and without applying any
//! language-specific tailorings", with "the case mappings with status field
//! 'C' or 'F' in the CaseFolding.txt file". That is D144 of
//! `[[source: unicode17-ch3]]`, toCasefold(X) = toCasefold(Y), R4's
//! toCasefold mapping each character to its full case folding
//! (`[[source: unicode-casefolding]]`). So `ß` matches `ss`, the Turkic rows
//! (status T) are not applied, and a precomposed `å` does not match `a`
//! followed by U+030A -- the text's own example.
//!
//! Not the search's comparison ([`crate::Needle`]), which folds and then
//! decomposes (D145) and so would match that pair. One index-only table,
//! `caseless_data`, compiled from the pinned file; nothing searches it.

use crate::caseless_data::{CASELESS_BLOCKS, CASELESS_INDEX, CASELESS_POOL, CASELESS_SHIFT};

/// The characters `c` folds to: one, or up to three for a full folding.
pub fn case_fold(c: char) -> impl Iterator<Item = char> {
    let cp = u32::from(c);
    let block = usize::from(CASELESS_INDEX[(cp >> CASELESS_SHIFT) as usize]);
    let at = (block << CASELESS_SHIFT) | (cp & ((1 << CASELESS_SHIFT) - 1)) as usize;
    let ix = usize::from(CASELESS_BLOCKS[at]);
    let seq: &'static [u32] = match ix {
        0 => &[],
        _ => &CASELESS_POOL[ix + 1..ix + 1 + CASELESS_POOL[ix] as usize],
    };
    let own = seq.is_empty().then_some(c);
    own.into_iter()
        .chain(seq.iter().filter_map(|&v| char::from_u32(v)))
}

/// Whether `a` and `b` are a default caseless match: their full case
/// foldings are the same characters.
pub fn caseless_eq<A, B>(a: A, b: B) -> bool
where
    A: IntoIterator<Item = char>,
    B: IntoIterator<Item = char>,
{
    a.into_iter()
        .flat_map(case_fold)
        .eq(b.into_iter().flat_map(case_fold))
}
