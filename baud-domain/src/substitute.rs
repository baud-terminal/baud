//! **What a `SUB` draws where it arrives, when the embedder asks for it**:
//! DEC's error character, or another the user names, or nothing.
//!
//! DEC's terminals draw one: `[[source: dec-std-070]]` p. 5-132, SUB "is
//! processed by placing a reversed question mark in the display at the
//! Active Position" through `insert_or_replace_character()`, the routine
//! every graphic character is written by (pp. 5-140-5-141); the VT220's
//! "Causes a reverse question mark to be displayed" (`[[source:
//! vt220-rm-ch4]]`). baud draws nothing by default, the owner's call of
//! 2026-10-23 ("i would say keep A as default and add config option and
//! maybe also which char to show? and default is the reverse question
//! mark"), and [`SubstituteMark::Drawn`] is the setting turned on.

use crate::{AmbiguousWidth, Classified, Codepoint, GraphemeClass, UnicodeWidths, Width};
use crate::{DEC_ERROR_CHARACTER, WidthPolicy};

/// **A character a `SUB` can draw**: one cell, a cluster of its own, under
/// every width the terminal can measure by -- grapheme class Other, one
/// column whichever ambiguous width and Unicode version are chosen. So
/// the mark takes the one cell DEC's does, whatever the session sets
/// later, and nothing after it joins it. Held as a non-zero word -- no
/// such character is `NUL` -- so a [`SubstituteMark`] is four bytes and
/// fits where the core keeps it without growing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrorCharacter(core::num::NonZeroU32);

/// Why a character cannot be the error character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnfitMark {
    /// A control, a mark, a joiner or another character that is not a
    /// cluster of its own (its grapheme class is not Other).
    NotAlone,
    /// Not one column under every width policy: zero, two, or East Asian
    /// Ambiguous.
    NotOneColumn,
}

impl ErrorCharacter {
    /// DEC's, U+2426 SYMBOL FOR SUBSTITUTE FORM TWO (note AE's AE8.1).
    pub const DEC: ErrorCharacter = match core::num::NonZeroU32::new(DEC_ERROR_CHARACTER.0) {
        Some(v) => ErrorCharacter(v),
        None => panic!("U+2426 is not NUL"),
    };

    /// `c`, if it can be drawn in one cell of its own.
    pub fn of(c: Codepoint) -> Result<ErrorCharacter, UnfitMark> {
        let cl = Classified::of(c);
        if cl.class != GraphemeClass::Other {
            return Err(UnfitMark::NotAlone);
        }
        let policies = [
            WidthPolicy::of(AmbiguousWidth::Narrow, UnicodeWidths::Current),
            WidthPolicy::of(AmbiguousWidth::Wide, UnicodeWidths::Current),
            WidthPolicy::of(AmbiguousWidth::Narrow, UnicodeWidths::Eight),
            WidthPolicy::of(AmbiguousWidth::Wide, UnicodeWidths::Eight),
        ];
        if !policies.iter().all(|&p| cl.width(p) == Width::NARROW) {
            return Err(UnfitMark::NotOneColumn);
        }
        // Class Other is never `NUL`, a Control.
        core::num::NonZeroU32::new(c.0)
            .map(ErrorCharacter)
            .ok_or(UnfitMark::NotAlone)
    }

    /// The character drawn.
    pub fn codepoint(self) -> Codepoint {
        Codepoint(self.0.get())
    }
}

/// **Whether a `SUB` draws a mark**: `Off`, the default -- it cancels what
/// it interrupts and writes nothing -- or `Drawn`, written at the cursor in
/// the current rendition as a graphic character is, the cursor moving on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SubstituteMark {
    #[default]
    Off,
    Drawn(ErrorCharacter),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cp(c: char) -> Codepoint {
        Codepoint::from_char(c)
    }

    /// **DEC's character, and other one-cell characters, are fit; a
    /// control, a mark, a wide or an ambiguous one is not**: U+2426 is
    /// East Asian Width N, so one column under every policy; U+2592, the
    /// VT100's blotch, is Ambiguous, so two under a wide ambiguous width.
    /// Perturbation: test only the narrow policy -- U+00A7 passes.
    #[test]
    fn the_error_character_is_one_cell_of_its_own() {
        assert_eq!(
            ErrorCharacter::of(DEC_ERROR_CHARACTER),
            Ok(ErrorCharacter::DEC)
        );
        assert_eq!(ErrorCharacter::DEC.codepoint(), cp('\u{2426}'));
        for c in ['?', '\u{2E2E}', '#', '\u{E0B0}'] {
            assert_eq!(
                ErrorCharacter::of(cp(c)).map(ErrorCharacter::codepoint),
                Ok(cp(c)),
                "{c:?}"
            );
        }
        for c in [
            '\u{1A}', '\u{7}', '\u{AD}', '\u{301}', '\u{200D}', '\u{FE0F}', '\u{600}',
        ] {
            assert_eq!(ErrorCharacter::of(cp(c)), Err(UnfitMark::NotAlone), "{c:?}");
        }
        for c in ['\u{4E00}', '\u{1F600}', '\u{A7}', '\u{2592}'] {
            assert_eq!(
                ErrorCharacter::of(cp(c)),
                Err(UnfitMark::NotOneColumn),
                "{c:?}"
            );
        }
        assert_eq!(SubstituteMark::default(), SubstituteMark::Off);
        assert_eq!(
            core::mem::size_of::<SubstituteMark>(),
            4,
            "the niche holds Off"
        );
    }
}
