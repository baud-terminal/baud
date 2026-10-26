//! **SAPV, `CSI Ps... SP ]`**: "SAPV is used to specify one or more
//! variants for the presentation of subsequent text" (`[[source: ecma48]]`
//! 8.3.108).
//!
//! **Each value folds into the rendition being built**, as an `SGR`
//! attribute does, so the characters written after it carry it in their
//! rendition and the renderer draws them by it ([`baud_domain::Variants`]).
//! Value 12, "vowels are presented after the preceding character", is
//! refused and counted ([`Term::variants_refused`]): a vowel is stored on
//! its letter's cell and the grid has no column for it to follow in (Jev's
//! lean, REFUSED_COUNTED, 0.59/0.61). A value 8.3.108 does not define is
//! counted in `unsupported_param`.
//!
//! **A form for "the following graphic character"** (5 to 8) is in the
//! rendition until a character takes it: a letter that takes a form keeps
//! it for its write, and the next character takes it out before its own
//! ([`Paint::form_ends`]); a character that takes none spends it unstored,
//! since no form draws it otherwise ([`baud_domain::forced_form`]). 21
//! holds forms "for the following graphic characters until cancelled"; 22
//! cancels 21, and a form in force then is the next character's alone. `DECSTR` and `RIS` return the
//! variants to the default presentation (Jev's lean, RESET, 0.32/0.22, as
//! they return SCP's path); `SGR 0` leaves them, being no `SGR`.
//!
//! **What it costs a program that never sends it**: the writers test two
//! more bits ([`ModeBit::FormPending`], [`ModeBit::FormEnd`]) in the test of
//! the mode word they make for IRM, and `run` makes one test a run; they go
//! out of line while a form waits or is spent. The out-of-line branches
//! still cost code: `ingest2` retires 1.0-1.6% more instructions on every
//! recording, `cat` included, with no throughput result (docs/record.md,
//! "SAPV: a program chooses how the text after it is drawn").

use baud_domain::{Mirroring, ModeBit, Sapv, Slant, Variants};

use crate::paint::Paint;
use crate::term::Term;

/// Whether any SAPV has selected a presentation other than the default.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum VariantsSeen {
    No,
    Yes,
}

/// A count of the SAPV values refused, as a reading to compare: 12, whose
/// vowels after their letter a cell grid has no column for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VariantsRefused(u32);

impl VariantsRefused {
    const FIRST: VariantsRefused = VariantsRefused(0);

    fn next(self) -> VariantsRefused {
        VariantsRefused(self.0.wrapping_add(1))
    }
}

/// SAPV's state beside the rendition's variants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct State {
    pub(crate) forms: Forms,
    /// Values not acted on ([`Term::variants_refused`]).
    pub(crate) refused: VariantsRefused,
    pub(crate) seen: VariantsSeen,
}

impl State {
    pub(crate) const POWER_ON: State = State {
        forms: Forms::Once,
        refused: VariantsRefused::FIRST,
        seen: VariantsSeen::No,
    };
}

/// Whether forms (values 5 to 8) are held for every following character
/// (21) or for the next one only (22, and at power-on).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Forms {
    Once,
    Held,
}

impl Paint<'_, '_> {
    /// SAPV: each parameter value folded into the rendition being built,
    /// in order; an omitted one is 0.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_variants(&mut self, csi: &crate::write::Csi) {
        self.form_ends();
        let mut v = self.style_raw.variants();
        let mut refused = self.strs.variants.refused;
        let mut undefined = 0usize;
        let mut forms = self.strs.variants.forms;
        csi.for_each_param(|p| match Sapv::of(p.map_or(0, |x| x.index())) {
            Sapv::Default => {
                v = Variants::DEFAULT;
                forms = Forms::Once;
            }
            Sapv::Digits(d) => v = v.with_digits(d),
            Sapv::Paired => v = v.with_paired(Mirroring::Mirrored),
            Sapv::Mathematical => v = v.with_mathematical(Mirroring::Mirrored),
            Sapv::Unmirrored => {
                v = v
                    .with_paired(Mirroring::Cancelled)
                    .with_mathematical(Mirroring::Cancelled)
            }
            Sapv::Form(f) => v = v.with_form(Some(f)),
            Sapv::DecimalMark(m) => v = v.with_decimal_mark(m),
            Sapv::Vowels(x) => v = v.with_vowels(x),
            Sapv::VowelsAfter => refused = refused.next(),
            Sapv::Shaping(s) => v = v.with_shaping(s),
            Sapv::Slant => v = v.with_slant(Slant::ByDirection),
            Sapv::FormsHeld => forms = Forms::Held,
            Sapv::FormsOnce => forms = Forms::Once,
            Sapv::Undefined => undefined += 1,
        });
        self.strs.variants.refused = refused;
        self.unsupported_param += undefined;
        self.strs.variants.forms = forms;
        if !v.is_default() {
            self.strs.variants.seen = VariantsSeen::Yes;
        }
        self.style_raw = self.style_raw.with_variants(v);
        self.style = self.intern_style(self.style_raw);
        self.cold.modes.apply(
            ModeBit::FormPending,
            v.form().is_some() && forms == Forms::Once,
        );
    }

    /// **The form the last character took leaves the rendition being
    /// built**, if one did: before the next character, a SAPV, a change of
    /// display, or anything else that reads the rendition.
    pub(crate) fn form_ends(&mut self) {
        if self.cold.modes.is_set(ModeBit::FormEnd) {
            self.form_written();
        }
    }

    /// **The rendition being built as a save keeps it**: a form the last
    /// character took left out, as it leaves before the next.
    pub(crate) fn kept_rendition(&self) -> baud_domain::StyleWord {
        match self.cold.modes.is_set(ModeBit::FormEnd) {
            true => self
                .style_raw
                .with_variants(self.style_raw.variants().with_form(None)),
            false => self.style_raw,
        }
    }

    /// **The form leaves the rendition being built**: spent, or ended.
    #[cold]
    #[inline(never)]
    pub(crate) fn form_written(&mut self) {
        self.cold.modes.reset(ModeBit::FormPending);
        self.cold.modes.reset(ModeBit::FormEnd);
        let v = self.style_raw.variants().with_form(None);
        self.style_raw = self.style_raw.with_variants(v);
        self.style = self.intern_style(self.style_raw);
    }

    /// **The rendition being built was put back** (`DECRC`, `XTPOPSGR`,
    /// `DECRSPS`, a change of display): a form it holds waits for the
    /// following character unless forms are held.
    pub(crate) fn rendition_changed(&mut self) {
        self.cold.modes.reset(ModeBit::FormEnd);
        let pending =
            self.style_raw.variants().form().is_some() && self.strs.variants.forms == Forms::Once;
        self.cold.modes.apply(ModeBit::FormPending, pending);
    }

    /// The variants' state at power-on, which `DECSTR` and `RIS` return
    /// to; the rendition's own are reset with it.
    pub(crate) fn reset_variants(&mut self) {
        self.cold.modes.reset(ModeBit::FormPending);
        self.cold.modes.reset(ModeBit::FormEnd);
        self.strs.variants.forms = Forms::Once;
    }
}

impl Term<'_> {
    /// **SAPV values refused**: value 12, "vowels are presented after the
    /// preceding character", a cell grid having no column for it that the
    /// data holds.
    pub fn variants_refused(&self) -> VariantsRefused {
        self.store.strs.variants.refused
    }

    /// **Whether any SAPV has selected a presentation other than the
    /// default**, since power-on: until one has, no cell carries a variant
    /// and a renderer reads none.
    pub fn variants_seen(&self) -> bool {
        self.store.strs.variants.seen == VariantsSeen::Yes
    }
}
