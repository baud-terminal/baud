//! **The Arabic presentation forms**, read as a face with no `GSUB` forms
//! is drawn through them: a joining letter's form found as the character
//! UnicodeData.txt decomposes to that letter in that form
//! (`[[source: unicode-data]]`), and a lam-alef ligature as one of U+FEF5..
//! U+FEFC. The tables are generated (`presentation.rs`,
//! `scripts/gen-joining-tables.py`); this is their lookup.
//!
//! The characters are compatibility forms that "should not be used in
//! general interchange" (`[[source: unicode-ch9]]` 9.2.12): baud never
//! stores one for a letter it was sent. They are a key into the face's
//! `cmap`, for the glyph a legacy face draws a form with.

use crate::presentation::{FORMS, LAM_ALEF};
use baud_domain::{Codepoint, JoiningForm, LamAlef};

/// The presentation form of `letter` in `form`, if one is encoded: none
/// for alaph's three forms, whose script encodes none.
pub fn presentation_form(letter: Codepoint, form: JoiningForm) -> Option<Codepoint> {
    let code = match form {
        JoiningForm::Isolated => 0,
        JoiningForm::Final => 1,
        JoiningForm::Initial => 2,
        JoiningForm::Medial => 3,
        JoiningForm::Final2 | JoiningForm::Final3 | JoiningForm::Medial2 => return None,
    };
    let key = letter.value().checked_mul(4)? | code;
    let i = FORMS.partition_point(|&(k, _)| k < key);
    match FORMS.get(i) {
        Some(&(k, p)) if k == key => Codepoint::new(p),
        _ => None,
    }
}

/// The lam-alef ligature lam (U+0644) forms with `alef`, in `kind`'s form,
/// if one is encoded: (LAM-ALEF)n is the `<isolated>` character, (LAM-ALEF)r
/// the `<final>` one.
pub fn lam_alef_form(alef: Codepoint, kind: LamAlef) -> Option<Codepoint> {
    let &(_, nominal, joined) = LAM_ALEF.iter().find(|&&(a, _, _)| a == alef.value())?;
    Codepoint::new(match kind {
        LamAlef::Nominal => nominal,
        LamAlef::Joined => joined,
    })
}
