//! **Cursive joining**: which form each letter of a cursive script takes,
//! by the Unicode Standard's rules R1-R7 and its obligatory lam-alef
//! ligature, L1-L3 (`[[source: unicode-ch9]]` 9.2.2 and 9.2.3).
//!
//! **The data is compiled, the rules are a walk.** Joining_Type and the two
//! joining groups the ligature rules name come from ArabicShaping.txt
//! (`[[source: unicode-arabicshaping]]`) through a two-stage trie generated
//! at build time (`joining_data.rs`); a code point it does not list is T if
//! its General Category is Mn, Me or Cf and U otherwise, as the file's
//! header says. [`join`] walks one run and gives each joining character its
//! form.
//!
//! **In logical order.** The standard states R1-R7 "in terms of their
//! display (visual) order", and allows them restated in logical order,
//! "the words right and left ... would become preceding and following":
//! [`join`] takes them so, which is how a right-to-left run is stored. A
//! caller with text already in display order, left to right, hands it over
//! reversed.

use crate::joining_data::{JOINING_BLOCKS, JOINING_INDEX};

/// Joining_Type (`[[source: unicode-ch9]]` Table 9-3), in the order the
/// table's low three bits number them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoiningType {
    /// U: spaces, digits, punctuation, other scripts' letters, ZWNJ.
    NonJoining,
    /// T: nonspacing marks and most format controls; skipped by R1.
    Transparent,
    /// C: ZWJ and tatweel, which cause joining and take no form.
    JoinCausing,
    /// D: beh, teh, jeem: four forms.
    DualJoining,
    /// R: alef, dal, reh: joins only to its right, two forms.
    RightJoining,
    /// L: joins only to its left (Phags-pa, Manichaean).
    LeftJoining,
}

/// The joining groups the obligatory ligature rules name (9.2.3): "any
/// character in the LAM joining group followed by any character from the
/// ALEF joining group".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ligating {
    Neither,
    Lam,
    Alef,
}

fn entry(cp: u32) -> u8 {
    let Some(&block) = JOINING_INDEX.get(usize::try_from(cp >> 8).unwrap_or(usize::MAX)) else {
        return 0;
    };
    JOINING_BLOCKS
        .get(usize::from(block) * 256 + usize::try_from(cp & 0xFF).unwrap_or(0))
        .copied()
        .unwrap_or(0)
}

impl JoiningType {
    /// The Joining_Type of `cp`; U past U+10FFFF.
    pub fn of(cp: u32) -> JoiningType {
        match entry(cp) & 7 {
            1 => JoiningType::Transparent,
            2 => JoiningType::JoinCausing,
            3 => JoiningType::DualJoining,
            4 => JoiningType::RightJoining,
            5 => JoiningType::LeftJoining,
            _ => JoiningType::NonJoining,
        }
    }

    /// Whether a character of this type on a character's right causes it
    /// to join right: "Right join-causing: Superset of dual-joining,
    /// left-joining, and join-causing" (Table 9-4). In logical order, a
    /// preceding character.
    fn causes_from_preceding(self) -> bool {
        matches!(
            self,
            JoiningType::DualJoining | JoiningType::LeftJoining | JoiningType::JoinCausing
        )
    }

    /// "Left join-causing: Superset of dual-joining, right-joining, and
    /// join-causing" (Table 9-4); in logical order, a following character.
    fn causes_from_following(self) -> bool {
        matches!(
            self,
            JoiningType::DualJoining | JoiningType::RightJoining | JoiningType::JoinCausing
        )
    }
}

impl Ligating {
    /// Which of the two groups `cp` is in, if either.
    pub fn of(cp: u32) -> Ligating {
        match entry(cp) >> 3 & 3 {
            1 => Ligating::Lam,
            2 => Ligating::Alef,
            _ => Ligating::Neither,
        }
    }
}

/// The glyph form a joining character takes (Table 9-5), named for the
/// OpenType feature that draws it in a right-to-left run.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum JoiningForm {
    /// Xn, the non-joining form: `isol`.
    Isolated,
    /// Xr, joined on its right, to the preceding character: `fina`.
    Final,
    /// Xl, joined on its left, to the following character: `init`.
    Initial,
    /// Xm, joined on both sides: `medi`.
    Medial,
}

/// Whether `cp`'s 256-character block holds a character of type D, R or
/// L, the three that take a form: one load from [`JOIN_BLOCKS`]. A row
/// whose every cell's block answers no has nothing to join.
#[inline]
pub fn in_joining_block(cp: u32) -> bool {
    let block = usize::try_from(cp >> 8).unwrap_or(usize::MAX);
    JOIN_BLOCKS
        .get(block >> 6)
        .is_some_and(|w| w >> (block & 63) & 1 == 1)
}

/// Whether `cp` takes a form: Joining_Type D, R or L. A code point below
/// the first block that holds one -- every ASCII, Latin, Greek, Cyrillic
/// and Hebrew one -- is answered by one comparison.
#[inline]
pub fn takes_form(cp: u32) -> bool {
    cp >= FIRST_JOINING_BLOCK
        && in_joining_block(cp)
        && matches!(
            JoiningType::of(cp),
            JoiningType::DualJoining | JoiningType::RightJoining | JoiningType::LeftJoining
        )
}

/// A bit for each 256-character block, set where the block holds a
/// character of type D, R or L: compiled from the joining table at build
/// time.
pub static JOIN_BLOCKS: [u64; 68] = join_blocks();

/// The first code point of the first block [`JOIN_BLOCKS`] sets, U+0600 in
/// Unicode 17: no code point below it takes a form. Found at build time.
pub const FIRST_JOINING_BLOCK: u32 = first_joining_block();

const fn first_joining_block() -> u32 {
    let blocks = join_blocks();
    let mut b = 0;
    while b < 68 * 64 {
        if blocks[b / 64] >> (b % 64) & 1 == 1 {
            return (b as u32) << 8;
        }
        b += 1;
    }
    u32::MAX
}

/// **A bit for each 256-character block that holds a character that can
/// make its paragraph reorder or a letter that takes a joining form**:
/// [`crate::RTL_BLOCKS`] and [`JOIN_BLOCKS`] together, compiled at build
/// time, so a row's one pass with one load a cell asks what its layout
/// reads for both. A right-to-left block that joins is in both; Mongolian
/// and Phags-pa, which join and run left to right, are in the second only.
pub static LAYOUT_BLOCKS: [u64; 68] = layout_blocks();

/// Whether `cp`'s 256-character block holds a character that can make its
/// paragraph reorder or one that takes a joining form: one load from
/// [`LAYOUT_BLOCKS`], and no for any word past U+10FFFF.
#[inline]
pub fn in_layout_block(cp: u32) -> bool {
    let block = usize::try_from(cp >> 8).unwrap_or(usize::MAX);
    LAYOUT_BLOCKS
        .get(block >> 6)
        .is_some_and(|w| w >> (block & 63) & 1 == 1)
}

const fn layout_blocks() -> [u64; 68] {
    let (rtl, joins) = (crate::bidi::rtl_blocks(), join_blocks());
    let mut out = [0u64; 68];
    let mut k = 0;
    while k < 68 {
        out[k] = rtl[k] | joins[k];
        k += 1;
    }
    out
}

const fn join_blocks() -> [u64; 68] {
    let distinct = JOINING_BLOCKS.len() / 256;
    let mut joins = [false; 256];
    let mut b = 0;
    while b < distinct {
        let mut i = 0;
        while i < 256 {
            // D, R and L in `JoiningType`'s order.
            if matches!(JOINING_BLOCKS[b * 256 + i] & 7, 3..=5) {
                joins[b] = true;
            }
            i += 1;
        }
        b += 1;
    }
    let mut out = [0u64; 68];
    let mut k = 0;
    while k < JOINING_INDEX.len() {
        if joins[JOINING_INDEX[k] as usize] {
            out[k / 64] |= 1 << (k % 64);
        }
        k += 1;
    }
    out
}

/// **Rules R1-R7 over one run, in logical order**: `forms[i]` is the form
/// of the character of type `types[i]`, `None` for one of type U, T or C,
/// which take none. A run is what shaping may join across: UAX #9 limits it
/// to "characters within the same level run" (`[[source: uax9-bidi]]` 3.5),
/// and a caller ends one wherever that or the line does. Only as many
/// positions as both slices hold are read and written.
///
/// R1 skips a transparent character: each character's neighbours are the
/// nearest characters either side that are not T.
pub fn join(types: &[JoiningType], forms: &mut [Option<JoiningForm>]) {
    let n = types.len().min(forms.len());
    // The nearest preceding character that is not T, as the walk passes.
    let mut preceding: Option<JoiningType> = None;
    for i in 0..n {
        let t = types[i];
        if t == JoiningType::Transparent {
            forms[i] = None;
            continue;
        }
        let following = types[i + 1..n]
            .iter()
            .copied()
            .find(|&f| f != JoiningType::Transparent);
        let right = preceding.is_some_and(JoiningType::causes_from_preceding);
        let left = following.is_some_and(JoiningType::causes_from_following);
        forms[i] = match t {
            // R2.
            JoiningType::RightJoining if right => Some(JoiningForm::Final),
            // R3.
            JoiningType::LeftJoining if left => Some(JoiningForm::Initial),
            // R4, R5, R6.
            JoiningType::DualJoining => Some(match (right, left) {
                (true, true) => JoiningForm::Medial,
                (true, false) => JoiningForm::Final,
                (false, true) => JoiningForm::Initial,
                (false, false) => JoiningForm::Isolated,
            }),
            // R7.
            JoiningType::RightJoining | JoiningType::LeftJoining => Some(JoiningForm::Isolated),
            _ => None,
        };
        preceding = Some(t);
    }
}

/// The lam-alef ligature a lam forms (9.2.3, L2 and L3), by the lam's form:
/// with a medial lam, L2's right-joining (LAM-ALEF)r; with an initial one,
/// L3's nominal (LAM-ALEF)n.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LamAlef {
    Nominal,
    Joined,
}

/// **The obligatory ligatures of one run, after [`join`]**: for each lam
/// that ligates, its position, its alef's and the ligature's form. A LAM
/// whose form joins it to the following character (Xl or Xm) and whose
/// following character, skipping T (L1), is an ALEF -- which the rules
/// have then made Xr -- forms one. `cps` are the run's code points, the
/// same length as `forms`.
pub fn lam_alefs(
    cps: &[u32],
    forms: &[Option<JoiningForm>],
    mut each: impl FnMut(usize, usize, LamAlef),
) {
    let n = cps.len().min(forms.len());
    for i in 0..n {
        let kind = match forms[i] {
            Some(JoiningForm::Medial) => LamAlef::Joined,
            Some(JoiningForm::Initial) => LamAlef::Nominal,
            _ => continue,
        };
        if Ligating::of(cps[i]) != Ligating::Lam {
            continue;
        }
        let Some(j) = (i + 1..n).find(|&j| JoiningType::of(cps[j]) != JoiningType::Transparent)
        else {
            continue;
        };
        if Ligating::of(cps[j]) == Ligating::Alef && forms[j] == Some(JoiningForm::Final) {
            each(i, j, kind);
        }
    }
}
