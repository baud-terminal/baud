//! **SAPV's presentation variants**: `CSI Ps... SP ]`, "SAPV is used to
//! specify one or more variants for the presentation of subsequent text"
//! (`[[source: ecma48]]` 8.3.108).
//!
//! **Carried by each character, in its rendition**: the variants in force
//! when a character is written ride in its [`crate::StyleWord`], as `SGR`'s
//! attributes do, so text written before a change keeps its own -- "the
//! presentation of subsequent text" -- and the cell, eight bytes, does not
//! grow: the word's two spare bytes hold them (Jev's lean, PER_CHARACTER,
//! 0.82/0.96). `SGR 0` leaves them, being no `SGR`.
//!
//! **Zero is the default presentation**, "implementation-defined" for
//! value 0: digits as stored, paired characters and mathematical operators
//! mirrored where a line runs right to left (values 3 and 4, UAX #9 L4 and
//! terminal-wg's explicit RTL), vowels above or below their letter (11),
//! Arabic joined with the lam-alef ligature and no other (13), the decimal
//! mark as stored, italics as their face leans. A zeroed rendition table
//! starts every slot there.

use crate::{JoiningForm, JoiningType};
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// The variants a character was written under: two bytes of its
/// rendition. Bit 15 is never set by any value, which keeps
/// [`Variants::TOMBSTONE`] out of every word a program can make.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct Variants(pub(crate) u16);

/// How the decimal digits are drawn: values 1 and 20 (as stored; 20's
/// "device dependent" symbols are baud's stored ones, Jev's lean
/// AS_STORED, 0.86/0.82) and 2, "the graphic symbols used in the Arabic
/// script, i.e. the Hindi symbols", U+0660-U+0669 (`[[source:
/// unicode-ch9]]` 9.2: the digits of the Arabic world are "widely known
/// there as \"Hindi numbers\"").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Digits {
    Stored,
    ArabicIndic,
}

/// Which FULL STOP a decimal number's mark is drawn as: values 9 (as
/// stored) and 10, a COMMA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecimalMark {
    FullStop,
    Comma,
}

/// Whether vowels are drawn: value 11, above or below the letter before
/// them, or 16, not at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vowels {
    Marked,
    Hidden,
}

/// How Arabic is shaped: 13, joined with the lam-alef ligature and no
/// other; 14, joined with no ligature; 18, not joined and the digits as
/// stored too; 19, not joined, the digits as values 1 and 2 say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shaping {
    Joined,
    Unligated,
    Stored,
    StoredButDigits,
}

/// A joining form a character is drawn in whatever its neighbours give
/// it: values 5 to 8.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Form {
    Isolated,
    Initial,
    Medial,
    Final,
}

/// Whether one class of mirrored characters is drawn mirrored where a line
/// runs right to left: value 3 the paired characters, 4 the mathematical
/// operators, 15 neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirroring {
    Mirrored,
    Cancelled,
}

/// How italics lean: value 17, "when the string direction is
/// right-to-left, the italicized characters are slanted to the left; when
/// the string direction is left-to-right, the italicized characters are
/// slanted to the right", or as the italic face leans, the default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slant {
    AsFace,
    ByDirection,
}

/// What one SAPV parameter value does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sapv {
    /// 0: the default presentation, cancelling every earlier SAPV.
    Default,
    Digits(Digits),
    /// 3: the paired characters mirrored again.
    Paired,
    /// 4: the mathematical operators mirrored again.
    Mathematical,
    /// 15: cancels 3 and 4.
    Unmirrored,
    /// 5 to 8: the following graphic character's form.
    Form(Form),
    DecimalMark(DecimalMark),
    Vowels(Vowels),
    /// 12: vowels after the character before them, refused: a vowel is
    /// stored on its letter's cell and the grid has no column for it to
    /// follow in (Jev's lean, REFUSED_COUNTED, 0.59/0.61).
    VowelsAfter,
    Shaping(Shaping),
    /// 17: italics slanted toward the string direction, left where it
    /// runs right to left (Jev's lean, LEFT_SHEAR, 0.82/0.80).
    Slant,
    /// 21: forms held for the following characters until cancelled.
    FormsHeld,
    /// 22: a form for the next single character only.
    FormsOnce,
    /// A value 8.3.108 does not define.
    Undefined,
}

impl Sapv {
    /// Parameter value `v`; an omitted one is 0, SAPV's default.
    pub fn of(v: usize) -> Sapv {
        match v {
            0 => Sapv::Default,
            1 | 20 => Sapv::Digits(Digits::Stored),
            2 => Sapv::Digits(Digits::ArabicIndic),
            3 => Sapv::Paired,
            4 => Sapv::Mathematical,
            5 => Sapv::Form(Form::Isolated),
            6 => Sapv::Form(Form::Initial),
            7 => Sapv::Form(Form::Medial),
            8 => Sapv::Form(Form::Final),
            9 => Sapv::DecimalMark(DecimalMark::FullStop),
            10 => Sapv::DecimalMark(DecimalMark::Comma),
            11 => Sapv::Vowels(Vowels::Marked),
            12 => Sapv::VowelsAfter,
            13 => Sapv::Shaping(Shaping::Joined),
            14 => Sapv::Shaping(Shaping::Unligated),
            15 => Sapv::Unmirrored,
            16 => Sapv::Vowels(Vowels::Hidden),
            17 => Sapv::Slant,
            18 => Sapv::Shaping(Shaping::Stored),
            19 => Sapv::Shaping(Shaping::StoredButDigits),
            21 => Sapv::FormsHeld,
            22 => Sapv::FormsOnce,
            _ => Sapv::Undefined,
        }
    }
}

impl Variants {
    const DIGITS: u16 = 1;
    const PAIRED_OFF: u16 = 1 << 1;
    const MATH_OFF: u16 = 1 << 2;
    const COMMA: u16 = 1 << 3;
    const VOWELS_HIDDEN: u16 = 1 << 4;
    const SHAPING: u16 = 3 << 5;
    const SLANT: u16 = 1 << 7;
    const FORM: u16 = 7 << 8;
    /// The default presentation.
    pub const DEFAULT: Variants = Variants(0);
    /// What a freed rendition-table slot holds ([`crate::StyleWord::TOMBSTONE`]):
    /// bit 15, which no value sets.
    pub const TOMBSTONE: Variants = Variants(1 << 15);

    pub fn bits(self) -> u16 {
        self.0
    }

    pub fn is_default(self) -> bool {
        self.0 == 0
    }

    fn with(self, mask: u16, bits: u16) -> Variants {
        Variants((self.0 & !mask) | (bits & mask))
    }

    pub fn digits(self) -> Digits {
        match self.0 & Self::DIGITS {
            0 => Digits::Stored,
            _ => Digits::ArabicIndic,
        }
    }
    pub fn with_digits(self, d: Digits) -> Variants {
        self.with(Self::DIGITS, u16::from(d == Digits::ArabicIndic))
    }

    /// Whether the paired characters (value 3) are drawn mirrored.
    pub fn paired(self) -> Mirroring {
        match self.0 & Self::PAIRED_OFF {
            0 => Mirroring::Mirrored,
            _ => Mirroring::Cancelled,
        }
    }
    /// Whether the mathematical operators (value 4) are drawn mirrored.
    pub fn mathematical(self) -> Mirroring {
        match self.0 & Self::MATH_OFF {
            0 => Mirroring::Mirrored,
            _ => Mirroring::Cancelled,
        }
    }
    pub fn with_paired(self, m: Mirroring) -> Variants {
        let off = match m {
            Mirroring::Mirrored => 0,
            Mirroring::Cancelled => Self::PAIRED_OFF,
        };
        self.with(Self::PAIRED_OFF, off)
    }
    pub fn with_mathematical(self, m: Mirroring) -> Variants {
        let off = match m {
            Mirroring::Mirrored => 0,
            Mirroring::Cancelled => Self::MATH_OFF,
        };
        self.with(Self::MATH_OFF, off)
    }

    pub fn decimal_mark(self) -> DecimalMark {
        match self.0 & Self::COMMA {
            0 => DecimalMark::FullStop,
            _ => DecimalMark::Comma,
        }
    }
    pub fn with_decimal_mark(self, m: DecimalMark) -> Variants {
        self.with(
            Self::COMMA,
            u16::from(m == DecimalMark::Comma) * Self::COMMA,
        )
    }

    pub fn vowels(self) -> Vowels {
        match self.0 & Self::VOWELS_HIDDEN {
            0 => Vowels::Marked,
            _ => Vowels::Hidden,
        }
    }
    pub fn with_vowels(self, v: Vowels) -> Variants {
        self.with(
            Self::VOWELS_HIDDEN,
            u16::from(v == Vowels::Hidden) * Self::VOWELS_HIDDEN,
        )
    }

    pub fn shaping(self) -> Shaping {
        match (self.0 & Self::SHAPING) >> 5 {
            0 => Shaping::Joined,
            1 => Shaping::Unligated,
            2 => Shaping::Stored,
            _ => Shaping::StoredButDigits,
        }
    }
    pub fn with_shaping(self, s: Shaping) -> Variants {
        let n: u16 = match s {
            Shaping::Joined => 0,
            Shaping::Unligated => 1,
            Shaping::Stored => 2,
            Shaping::StoredButDigits => 3,
        };
        self.with(Self::SHAPING, n << 5)
    }

    pub fn slant(self) -> Slant {
        match self.0 & Self::SLANT {
            0 => Slant::AsFace,
            _ => Slant::ByDirection,
        }
    }
    pub fn with_slant(self, s: Slant) -> Variants {
        self.with(
            Self::SLANT,
            u16::from(s == Slant::ByDirection) * Self::SLANT,
        )
    }

    pub fn form(self) -> Option<Form> {
        match (self.0 & Self::FORM) >> 8 {
            1 => Some(Form::Isolated),
            2 => Some(Form::Initial),
            3 => Some(Form::Medial),
            4 => Some(Form::Final),
            _ => None,
        }
    }
    pub fn with_form(self, f: Option<Form>) -> Variants {
        let n: u16 = match f {
            None => 0,
            Some(Form::Isolated) => 1,
            Some(Form::Initial) => 2,
            Some(Form::Medial) => 3,
            Some(Form::Final) => 4,
        };
        self.with(Self::FORM, n << 8)
    }

    /// **How decimal digit `d` is drawn** under these variants: as stored,
    /// or its Arabic-Indic counterpart under value 2 -- unless value 18
    /// draws every character, the digits included, as stored.
    pub fn digit(self, d: char) -> char {
        match (self.digits(), self.shaping(), d) {
            (Digits::ArabicIndic, s, '0'..='9') if s != Shaping::Stored => {
                char::from_u32(0x0660 + (d as u32 - '0' as u32)).unwrap_or(d)
            }
            _ => d,
        }
    }

    /// Whether Arabic is joined at all: not under values 18 and 19.
    pub fn joins(self) -> bool {
        matches!(self.shaping(), Shaping::Joined | Shaping::Unligated)
    }

    /// Whether a lam and an alef form their ligature: value 13 alone.
    pub fn ligates(self) -> bool {
        self.shaping() == Shaping::Joined
    }
}

/// **The form a letter of joining type `t` is drawn in when `f` is asked
/// of it**: the form the joins asked for would give it, as the Unicode
/// Standard asks a form through context, a joiner beside the letter
/// (`[[source: unicode18-ch23]]` 23.2, "they indicate that the rendering
/// process should select a different joining glyph"): a right-joining
/// letter asked for its initial form, which joins on the left it cannot,
/// is drawn isolated, and asked for its medial form drawn final; a
/// left-joining one the mirror of that. A character that takes no form is
/// drawn as stored (`None`). Jev's lean, NEAREST, 0.22/0.40.
pub fn forced_form(f: Form, t: JoiningType) -> Option<JoiningForm> {
    use JoiningForm as J;
    Some(match (t, f) {
        (JoiningType::DualJoining, Form::Isolated) => J::Isolated,
        (JoiningType::DualJoining, Form::Initial) => J::Initial,
        (JoiningType::DualJoining, Form::Medial) => J::Medial,
        (JoiningType::DualJoining, Form::Final) => J::Final,
        (JoiningType::RightJoining, Form::Isolated | Form::Initial) => J::Isolated,
        (JoiningType::RightJoining, Form::Medial | Form::Final) => J::Final,
        (JoiningType::LeftJoining, Form::Isolated | Form::Final) => J::Isolated,
        (JoiningType::LeftJoining, Form::Medial | Form::Initial) => J::Initial,
        _ => return None,
    })
}

/// **Whether `c` is a vowel** for values 11 and 16: the Arabic marks the
/// Unicode Standard names as vowels, tanwin and harakat U+064B-U+0650
/// (`[[source: unicode-ch9]]` 9.2, "harakat (short vowel marks), tanwin
/// (postnasalized or long vowel marks)"), and the Hebrew points it classes
/// as vowels, U+05B0-U+05BB and U+05C7 (9.1, "dagesh, shin dot and sin
/// dot, vowels, and other pronunciation marks") -- Jev's lean,
/// ARABIC_AND_HEBREW_VOWELS, 0.40/0.23. Shadda and sukun are not vowels,
/// nor dagesh and the shin and sin dots.
pub fn is_vowel(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{0650}' | '\u{05B0}'..='\u{05BB}' | '\u{05C7}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_defined_value_parses_and_the_rest_are_undefined() {
        assert!((0..=40).all(|v| (Sapv::of(v) != Sapv::Undefined) == (v <= 22)));
    }

    #[test]
    fn each_field_round_trips_and_leaves_the_others() {
        let v = Variants::DEFAULT
            .with_digits(Digits::ArabicIndic)
            .with_paired(Mirroring::Cancelled)
            .with_decimal_mark(DecimalMark::Comma)
            .with_vowels(Vowels::Hidden)
            .with_shaping(Shaping::StoredButDigits)
            .with_slant(Slant::ByDirection)
            .with_form(Some(Form::Medial));
        assert_eq!(v.digits(), Digits::ArabicIndic);
        assert_eq!(v.paired(), Mirroring::Cancelled);
        assert_eq!(v.mathematical(), Mirroring::Mirrored);
        assert_eq!(v.decimal_mark(), DecimalMark::Comma);
        assert_eq!(v.vowels(), Vowels::Hidden);
        assert_eq!(v.shaping(), Shaping::StoredButDigits);
        assert_eq!(v.form(), Some(Form::Medial));
        assert_eq!(v.slant(), Slant::ByDirection);
        assert_eq!(v.with_slant(Slant::AsFace).slant(), Slant::AsFace);
        assert_eq!(
            v.with_slant(Slant::AsFace).shaping(),
            Shaping::StoredButDigits
        );
        assert_eq!(v.bits() & Variants::TOMBSTONE.bits(), 0);
        assert_eq!(v.with_form(None).form(), None);
    }

    #[test]
    fn a_form_a_letter_has_not_got_is_the_nearest_it_has() {
        let beh = JoiningType::of(0x0628);
        let alef = JoiningType::of(0x0627);
        assert_eq!(forced_form(Form::Medial, beh), Some(JoiningForm::Medial));
        assert_eq!(
            forced_form(Form::Initial, alef),
            Some(JoiningForm::Isolated)
        );
        assert_eq!(forced_form(Form::Medial, alef), Some(JoiningForm::Final));
        assert_eq!(
            forced_form(Form::Final, JoiningType::of(u32::from('a'))),
            None
        );
    }

    #[test]
    fn value_18_draws_the_digits_as_stored_and_19_does_not() {
        let v = Variants::DEFAULT.with_digits(Digits::ArabicIndic);
        assert_eq!(v.digit('3'), '\u{0663}');
        assert_eq!(v.with_shaping(Shaping::Stored).digit('3'), '3');
        assert_eq!(
            v.with_shaping(Shaping::StoredButDigits).digit('3'),
            '\u{0663}'
        );
        assert_eq!(Variants::DEFAULT.digit('3'), '3');
    }
}
