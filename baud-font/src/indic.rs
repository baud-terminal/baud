//! **The Indic shaper**, built for Devanagari: the shaping engine Microsoft's
//! "Developing OpenType Fonts for Devanagari Script" describes
//! (`[[source: ms-devanagari-script-development]]`), over the encoding
//! model of the Unicode Standard's 12.1 (`[[source: unicode-ch12]]`).
//!
//! **Why reordering is the shaper's.** Text is stored in logical order:
//! "In a text sequence, these characters are stored in logical (phonetic)
//! order", and "In modern usage the Devanagari script has only one
//! character denoting a left-side dependent vowel sign: U+093F" (12.1), so
//! कि is KA then VOWEL SIGN I and draws the sign first. The specification
//! leaves that to the engine: "The OpenType lookups in an Indic font must
//! be written to match glyph sequences after re-ordering has occurred.
//! OpenType fonts should not have substitutions that attempt to perform the
//! re-ordering" (Reorder characters). A font shaped as a simple script
//! draws the sign after its consonant and every conjunct as dead
//! consonants side by side.
//!
//! **The engine's stages, in the specification's order**
//! (Shaping Engine): "Analyze the text sequence; breaking it into syllable
//! clusters; Reorder the characters as necessary; Apply OpenType GSUB font
//! features to get the correct glyph shape; Apply OpenType GPOS features to
//! position glyphs or marks".
//! [`substitute`] runs them: [`find_syllables`] by the grammar; `locl` (and
//! `ccmp`); the initial reordering, which finds each syllable's base
//! consonant, moves a pre-base matra to the front and marks which glyphs
//! each masked feature may form; the basic shaping forms one feature at a
//! time, `nukt`, `akhn`, `rphf`, `rkrf`, `blwf`, `half`, `vatu` and `cjct`
//! in the specification's order (with `pref`, `abvf` and `pstf` where a face
//! names them, as the engine's dynamic properties list them: "Reph 'rphf',
//! Half forms 'half', Pre-base-reordering forms of Ra/Rra 'pref',
//! Below-base forms 'blwf', Post-base forms 'pstf'"); the final reordering,
//! which moves a pre-base matra closer to its main consonant and reph to
//! its place; then "the features for presentation forms are applied to the
//! entire cluster simultaneously" -- `pres`, `abvs`, `blws`,
//! `psts`, `haln`, with `calt` and the required `rlig` and `rclt` any
//! script may carry. Positioning (`kern`, `dist`, `abvm`, `blwm`, `mark`,
//! `mkmk`) is the shaper's caller's, after.
//!
//! **Practice where the text is silent, from HarfBuzz 14.5.1's Indic
//! shaper** (`[[source: harfbuzz-14-5-1-shaper-indic]]`, read as reference;
//! the fixtures in `tests/shape/devanagari.txt` are its hb-shape's output):
//! the grammar's finer classes, the masks each feature is applied with, the
//! cluster bookkeeping and the old specification's differences, each named
//! where it is done. Two choices the text leaves open are taken so:
//!
//! - **The grammar is HarfBuzz's** ([`scan`]), wider than the
//!   specification's three patterns: more than one matra of a class, a
//!   nukta after a matra (बडे़, in the corpus), a stand-alone cluster
//!   anywhere and not only "at the start of the word", digits and dashes as
//!   stand-alone bases beside NBSP, and the symbol cluster. The text allows
//!   it: "Shaping engine implementations may vary somewhat with regard to
//!   what sequences are or are not considered valid" (Invalid Combining
//!   Marks).
//! - **`init` is not applied.** The Devanagari list does not name it;
//!   HarfBuzz applies it to a word-initial left matra where a face carries
//!   it, which none of the Devanagari faces read does.
//!
//! **The open departures from the text**, where baud follows HarfBuzz:
//!
//! 1. **A pre-base matra stays before a halant that ZWJ follows.** The text
//!    says "If ZWJ or ZWNJ follow this halant, position is moved after it"
//!    (Reorder matras); HarfBuzz moves it after neither, from testing the
//!    engine the text describes, Uniscribe, with Nirmala UI
//!    (`U+091F,U+094D,U+200D,U+092F,U+093F`: "Matra does NOT move"), and a
//!    ZWNJ after a halant ends the syllable before any matra. ZWJ there
//!    asks for the half form, before which the matra stays.
//! 2. **Reph's step d.** Devanagari's reph goes before post-base
//!    consonants ("0930 (reph) BeforePostscript"). With no such consonant,
//!    the text says "the target position should be before the first
//!    matra, syllable modifier sign or vedic sign". baud, as HarfBuzz
//!    does, puts reph after the matras and before the syllable modifiers
//!    and Vedic signs, which is step e's reading followed by step f, so
//!    र्वा is VA, AA, reph. HarfBuzz's own step 4, which it says fails
//!    "BADLY", is for a reph after the subjoined forms and is never
//!    reached here.
//! 3. **`ccmp` runs first, with `locl`**, which the Devanagari list does
//!    not name; the feature registry makes it every script's: "This
//!    feature should always be applied", "Script/language sensitivity:
//!    None", "This feature needs to be implemented prior to any other
//!    feature" (`[[source: ms-opentype-features-ae]]`, ccmp).

use crate::buffer::{Buffer, ClusterLevel, Glyph, Ignorable, Mark};
use crate::bytes::{Count, Index};
use crate::face::Face;
use crate::gsub::{Context, Gsub};
use crate::layout::{
    Digest, Enabled, FeatureTag, Features, Joiners, LookupSet, PerSyllable, Required, ScriptTag,
    Stage,
};
use crate::script::Script;
use crate::shape::{CombiningClass, Substitution};
use crate::syllabic::{
    At, Categories, Category, Formed, Kind, Masks, Position, Serial, Syllabic, Syllable,
};
use baud_domain::{Codepoint, GlyphId};
use core::ops::Range;

/// How reph is written in a script.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RephMode {
    /// RA and halant at the start of a syllable become reph.
    Implicit,
}

/// Where the below-base forms feature applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BlwfMode {
    /// Before the base and after it.
    PreAndPost,
}

/// What differs from one Indic script to the next: its virama, where its
/// reph goes and how it is written, and where `blwf` applies. Devanagari's
/// is the only one built; Bengali, Gurmukhi, Gujarati, Oriya, Tamil,
/// Telugu, Kannada and Malayalam each need their own row and the branches
/// their specifications add (HarfBuzz's `indic_configs` table).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Config {
    virama: char,
    reph: Position,
    reph_mode: RephMode,
    blwf: BlwfMode,
}

/// Devanagari's: "Possible positions for reph, depending on the script, are;
/// after main, before post-base consonant forms, and after post-base
/// consonant forms", and its Reorder Class table gives "0930 (reph)
/// BeforePostscript" (`[[source: ms-devanagari-script-development]]`).
const DEVANAGARI: Config = Config {
    virama: '\u{094D}',
    reph: Position::BEFORE_POST,
    reph_mode: RephMode::Implicit,
    blwf: BlwfMode::PreAndPost,
};

/// The configuration of `script`'s Indic shaper, if it has one built.
pub(crate) fn config(script: Script) -> Option<Config> {
    match &script.code() {
        b"Deva" => Some(DEVANAGARI),
        _ => None,
    }
}

/// Whether text in `c`'s script is shaped by an Indic shaper here.
pub(crate) fn is_indic(c: Codepoint) -> bool {
    config(Script::of(c)).is_some()
}

/// Which Indic specification a face was built for: the new, under the
/// `dev2` tag, or the old, under `deva`. "Note for new Indic shaping
/// implementation 'dev2' is used (old-behavior implementations used
/// 'deva')" (Appendix A).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Spec {
    New,
    Old,
}

/// **What shaping a run in one script on one face needs to know of the
/// face**: its tables, its virama and dotted circle, which masked features
/// it names and the lookups the consonant classes are read from.
pub(crate) struct Plan<'a> {
    config: Config,
    script: Script,
    gsub: Option<Gsub<'a>>,
    spec: Spec,
    virama: Option<GlyphId>,
    dotted: Option<GlyphId>,
    /// The masked features the language system names.
    names: Masks,
    rphf: LookupSet,
    pref: LookupSet,
    blwf: LookupSet,
    pstf: LookupSet,
    vatu: LookupSet,
    context: Context,
    gdef: crate::layout::Gdef<'a>,
    /// Which cell a glyph moved across cells keeps ([`merge`]).
    clusters: ClusterLevel,
}

/// The masked features, each with its mask, in the order the basic stages
/// run them.
const MASKED: [(FeatureTag, Masks); 5] = [
    (FeatureTag::RPHF, Masks::RPHF),
    (FeatureTag::PREF, Masks::PREF),
    (FeatureTag::BLWF, Masks::BLWF),
    (FeatureTag::ABVF, Masks::ABVF),
    (FeatureTag::HALF, Masks::HALF),
];

/// The basic shaping forms, in the specification's order with the masked
/// ones HarfBuzz adds between them (`pref` and `abvf` after `rkrf` and
/// `blwf`, `pstf` after `half`): each its own stage, "applied one at a time
/// to the cluster or a relevant portion of the cluster".
const BASIC: [(FeatureTag, Masks); 11] = [
    (FeatureTag::NUKT, Masks::GLOBAL),
    (FeatureTag::AKHN, Masks::GLOBAL),
    (FeatureTag::RPHF, Masks::RPHF),
    (FeatureTag::RKRF, Masks::GLOBAL),
    (FeatureTag::PREF, Masks::PREF),
    (FeatureTag::BLWF, Masks::BLWF),
    (FeatureTag::ABVF, Masks::ABVF),
    (FeatureTag::HALF, Masks::HALF),
    (FeatureTag::PSTF, Masks::PSTF),
    (FeatureTag::VATU, Masks::GLOBAL),
    (FeatureTag::CJCT, Masks::GLOBAL),
];

/// Whether one of the Indic engine's `GSUB` stages names `tag`: what a
/// lookup digest must cover for the stages to be accelerated.
pub(crate) fn stages_name(tag: FeatureTag) -> bool {
    tag == FeatureTag::LOCL
        || tag == FeatureTag::CCMP
        || tag == FeatureTag::RLIG
        || tag == FeatureTag::RCLT
        || tag == FeatureTag::CALT
        || BASIC.iter().any(|&(t, _)| t == tag)
        || PRESENTATION.contains(&tag)
}

/// The presentation forms, "mandatory for software implementations".
const PRESENTATION: [FeatureTag; 5] = [
    FeatureTag::PRES,
    FeatureTag::ABVS,
    FeatureTag::BLWS,
    FeatureTag::PSTS,
    FeatureTag::HALN,
];

/// Whether text in `script` on `face` is shaped by an Indic shaper: the
/// test [`Plan::new`] makes before it reads anything else.
pub(crate) fn takes(face: &Face<'_>, script: Script) -> bool {
    config(script).is_some()
        && Substitution::of(face) != Substitution::Morx
        && !matches!(
            face.gsub().and_then(|g| g.chosen(script)),
            Some(ScriptTag::DFLT | ScriptTag::LATN)
        )
}

impl<'a> Plan<'a> {
    /// The plan for text in `script` on `face`, or `None` where no Indic
    /// shaper applies: the script has none built, the face shapes through
    /// `morx`, or its `GSUB` gives the script only `DFLT` or `latn`, a face
    /// "designed for the 'DFLT' script", which HarfBuzz shapes as a simple
    /// script (`[[source: harfbuzz-14-5-1-shaper]]`). A face with no
    /// `GSUB`, or none for the script, is reordered all the same.
    pub(crate) fn new(face: &Face<'a>, script: Script) -> Option<Plan<'a>> {
        if !takes(face, script) {
            return None;
        }
        let config = config(script)?;
        let gsub = face.gsub().copied();
        let chosen = gsub.and_then(|g| g.chosen(script));
        // HarfBuzz reads a face with no table for the script as the old
        // specification's: its tag is none, and none ends in '2'.
        let spec = match chosen {
            Some(ScriptTag([_, _, _, b'2'])) => Spec::New,
            _ => Spec::Old,
        };
        let set = |tag: FeatureTag| match gsub {
            Some(g) => g.lookups_where(script, Required::Left, |t| t == tag),
            None => LookupSet::empty(),
        };
        let mut names = Masks::NONE;
        for (tag, mask) in MASKED.into_iter().chain([(FeatureTag::PSTF, Masks::PSTF)]) {
            if gsub.is_some_and(|g| g.names(script, tag)) {
                names = names.or(mask);
            }
        }
        Some(Plan {
            config,
            script,
            gsub,
            spec,
            virama: face.glyph(Codepoint::from_char(config.virama)),
            dotted: face.glyph(Codepoint::from_char('\u{25CC}')),
            names,
            rphf: set(FeatureTag::RPHF),
            pref: set(FeatureTag::PREF),
            blwf: set(FeatureTag::BLWF),
            pstf: set(FeatureTag::PSTF),
            vatu: set(FeatureTag::VATU),
            // "The new-spec for all dual-spec scripts says zero-context
            // matching happens"; the old matched in context.
            context: match spec {
                Spec::New => Context::Zero,
                Spec::Old => Context::Any,
            },
            gdef: face.gdef(),
            clusters: ClusterLevel::Monotone,
        })
    }

    /// This plan with the glyphs it reorders keeping their cells as
    /// `level` says.
    pub(crate) fn keeping(self, level: ClusterLevel) -> Plan<'a> {
        Plan {
            clusters: level,
            ..self
        }
    }

    fn would(&self, set: &LookupSet, glyphs: &[GlyphId]) -> bool {
        self.gsub
            .is_some_and(|g| g.would_substitute(set, glyphs, self.context))
    }

    /// **The position a consonant takes from the face**: below the base
    /// where `blwf` or `vatu` would form it with a halant, after it where
    /// `pstf` or `pref` would, else a base. "Each of the features above is
    /// applied together with 'locl' feature to input sequences consisting
    /// of two characters ... for 'pref', 'blwf' and 'pstf', features are
    /// applied to Halant + Consonant combinations ... If these two glyphs
    /// form a ligature, with no additional glyphs in context, this means
    /// the consonant has the corresponding form" (Retrieving dynamic
    /// character properties). Both orders are tried, as HarfBuzz tries them
    /// for faces that copied the old specification's lookups into the new
    /// (`consonant_position_from_face()`).
    fn consonant_position(&self, consonant: GlyphId, virama: GlyphId) -> Position {
        let both = [virama, consonant, virama];
        let (vc, cv) = (&both[..2], &both[1..]);
        if self.would(&self.blwf, vc)
            || self.would(&self.blwf, cv)
            || self.would(&self.vatu, vc)
            || self.would(&self.vatu, cv)
        {
            return Position::BELOW_C;
        }
        if self.would(&self.pstf, vc)
            || self.would(&self.pstf, cv)
            || self.would(&self.pref, vc)
            || self.would(&self.pref, cv)
        {
            return Position::POST_C;
        }
        Position::BASE_C
    }
}

/// **Shapes the buffer's run through `GSUB` as the Indic engine does**,
/// in the stages the module comment lists, and answers how many named
/// lookups lay past what a lookup set holds.
pub(crate) fn substitute(
    plan: &Plan<'_>,
    buf: &mut Buffer<'_>,
    features: Features,
    digests: &[Digest],
) -> Count {
    find_syllables(buf.as_mut_slice());
    let mut declined = Count::ZERO;
    let script = plan.script;
    let staged = |set: &LookupSet, stage: Stage, buf: &mut Buffer<'_>, declined: &mut Count| {
        if let Some(g) = plan.gsub {
            g.apply_staged(buf, set, |_| Some(stage), digests);
        }
        *declined = Count::of(declined.value().saturating_add(set.declined().value()));
    };
    // Localized forms, and `ccmp` beside them, with the required feature.
    if let Some(g) = plan.gsub {
        let set = g.lookups_where(script, Required::Added, |t| {
            t == FeatureTag::LOCL || (t == FeatureTag::CCMP && features.ccmp == Enabled::On)
        });
        let stage = Stage {
            masks: Masks::GLOBAL,
            syllables: PerSyllable::Yes,
            joiners: Joiners::Auto,
        };
        staged(&set, stage, buf, &mut declined);
    }
    initial_reordering(plan, buf);
    for (tag, masks) in BASIC {
        let Some(g) = plan.gsub else {
            break;
        };
        let set = g.lookups_where(script, Required::Left, |t| t == tag);
        let stage = Stage {
            masks,
            syllables: PerSyllable::Yes,
            joiners: Joiners::Manual,
        };
        staged(&set, stage, buf, &mut declined);
    }
    final_reordering(plan, buf.as_mut_slice());
    // The presentation forms with the features every script takes, one
    // stage in lookup order. A lookup two features name takes both
    // stages' properties as HarfBuzz merges them: either mask, within a
    // syllable if either keeps to one, manual joiners if either is
    // (`[[source: harfbuzz-14-5-1-ot-map]]`).
    if let Some(g) = plan.gsub {
        let presentation = g.lookups_where(script, Required::Left, |t| PRESENTATION.contains(&t));
        let common = g.lookups_where(script, Required::Left, |t| {
            t == FeatureTag::RLIG
                || t == FeatureTag::RCLT
                || (t == FeatureTag::CALT && features.calt == Enabled::On)
        });
        let union = presentation.union(&common);
        g.apply_staged(
            buf,
            &union,
            |i| match (presentation.contains(i), common.contains(i)) {
                (true, _) => Some(Stage {
                    masks: Masks::GLOBAL,
                    syllables: PerSyllable::Yes,
                    joiners: Joiners::Manual,
                }),
                (false, true) => Some(Stage {
                    masks: Masks::GLOBAL,
                    syllables: PerSyllable::No,
                    joiners: Joiners::Auto,
                }),
                (false, false) => None,
            },
            digests,
        );
        declined = Count::of(declined.value().saturating_add(union.declined().value()));
    }
    declined
}

/// The longest window the grammar reads from a syllable's start: a
/// position set is one 64-bit word.
const WINDOW: usize = 63;

/// A set of end positions, relative to where a syllable starts.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Ends(u64);

impl Ends {
    const START: Ends = Ends(1);
    const NONE: Ends = Ends(0);

    fn or(self, o: Ends) -> Ends {
        Ends(self.0 | o.0)
    }

    /// The longest match, or `None` for none past the start.
    fn longest(self) -> Option<At> {
        let top = self.0.checked_ilog2()?;
        usize::try_from(top).ok().filter(|&n| n > 0).map(At)
    }
}

/// **The syllable grammar as sets of positions**: each production maps the
/// positions a match may have reached to the positions it reaches after
/// the production, so alternatives, options and repetitions are unions and
/// fixpoints and the longest match is the highest position reached. The
/// productions are HarfBuzz's (`[[source:
/// harfbuzz-14-5-1-shaper-indic-machine]]`); the specification's three
/// patterns, "Consonant syllable", "Vowel-based syllable" and "Stand Alone
/// cluster", are the first three, widened as the module comment says.
struct Scan<'c> {
    cats: &'c [Category],
}

/// The grammar's terminal classes.
mod set {
    use crate::syllabic::{Categories, Category as C};
    pub(super) const CONSONANT: Categories = C::C.set().or(C::RA);
    pub(super) const JOINER: Categories = C::ZWJ.set().or(C::ZWNJ);
    pub(super) const SM: Categories = C::SM.set().or(C::SMPST);
    pub(super) const REPHA_CS: Categories = C::REPHA.set().or(C::CS);
    pub(super) const MATRA: Categories = C::M.set().or(C::MPST);
}

impl Scan<'_> {
    /// One glyph of the class `of`.
    fn one(&self, s: Ends, of: Categories) -> Ends {
        let mut out = Ends::NONE;
        let mut left = s.0;
        while left != 0 {
            let i = usize::try_from(left.trailing_zeros()).unwrap_or(usize::MAX);
            left &= left - 1;
            if self.cats.get(i).is_some_and(|&c| of.has(c)) {
                out = out.or(Ends(
                    1u64.checked_shl(u32::try_from(i + 1).unwrap_or(64))
                        .unwrap_or(0),
                ));
            }
        }
        out
    }

    fn cat(&self, s: Ends, c: Category) -> Ends {
        self.one(s, c.set())
    }

    /// `f` once or not at all.
    fn opt(s: Ends, f: impl Fn(Ends) -> Ends) -> Ends {
        s.or(f(s))
    }

    /// `f` any number of times.
    fn star(s: Ends, f: impl Fn(Ends) -> Ends) -> Ends {
        let mut r = s;
        loop {
            let next = r.or(f(r));
            if next == r {
                return r;
            }
            r = next;
        }
    }

    /// `n = (ZWNJ? RS)? (N N?)?`
    fn n(&self, s: Ends) -> Ends {
        let rs = Scan::opt(s, |t| {
            self.cat(Scan::opt(t, |u| self.cat(u, Category::ZWNJ)), Category::RS)
        });
        Scan::opt(rs, |t| {
            Scan::opt(self.cat(t, Category::N), |u| self.cat(u, Category::N))
        })
    }

    /// `cn = c ZWJ? n?`
    fn cn(&self, s: Ends) -> Ends {
        self.n(Scan::opt(self.one(s, set::CONSONANT), |t| {
            self.cat(t, Category::ZWJ)
        }))
    }

    /// `reph = Ra H | Repha`
    fn reph(&self, s: Ends) -> Ends {
        self.cat(self.cat(s, Category::RA), Category::H)
            .or(self.cat(s, Category::REPHA))
    }

    /// `matra_group = z* (M | sm? MPst) N? H?`
    fn matra_group(&self, s: Ends) -> Ends {
        let z = Scan::star(s, |t| self.one(t, set::JOINER));
        let m = self
            .cat(z, Category::M)
            .or(self.cat(Scan::opt(z, |t| self.one(t, set::SM)), Category::MPST));
        Scan::opt(Scan::opt(m, |t| self.cat(t, Category::N)), |t| {
            self.cat(t, Category::H)
        })
    }

    /// `syllable_tail = (z? sm sm? ZWNJ?)? (A | VD)*`
    fn syllable_tail(&self, s: Ends) -> Ends {
        let sm = Scan::opt(s, |t| {
            let a = self.one(Scan::opt(t, |u| self.one(u, set::JOINER)), set::SM);
            Scan::opt(Scan::opt(a, |u| self.one(u, set::SM)), |u| {
                self.cat(u, Category::ZWNJ)
            })
        });
        Scan::star(sm, |t| self.cat(t, Category::A))
    }

    /// `halant_group = z? H (ZWJ N?)?`
    fn halant_group(&self, s: Ends) -> Ends {
        let h = self.cat(Scan::opt(s, |t| self.one(t, set::JOINER)), Category::H);
        Scan::opt(h, |t| {
            Scan::opt(self.cat(t, Category::ZWJ), |u| self.cat(u, Category::N))
        })
    }

    /// `final_halant_group = halant_group | H ZWNJ`
    fn final_halant_group(&self, s: Ends) -> Ends {
        self.halant_group(s)
            .or(self.cat(self.cat(s, Category::H), Category::ZWNJ))
    }

    /// `halant_or_matra_group = final_halant_group | matra_group*`
    fn halant_or_matra_group(&self, s: Ends) -> Ends {
        self.final_halant_group(s)
            .or(Scan::star(s, |t| self.matra_group(t)))
    }

    /// `complex_syllable_tail = (halant_group cn)* CM? halant_or_matra_group
    /// syllable_tail`
    fn complex_syllable_tail(&self, s: Ends) -> Ends {
        let conjuncts = Scan::star(s, |t| self.cn(self.halant_group(t)));
        let medial = Scan::opt(conjuncts, |t| self.cat(t, Category::CM));
        self.syllable_tail(self.halant_or_matra_group(medial))
    }

    fn consonant_syllable(&self, s: Ends) -> Ends {
        self.complex_syllable_tail(self.cn(Scan::opt(s, |t| self.one(t, set::REPHA_CS))))
    }

    fn vowel_syllable(&self, s: Ends) -> Ends {
        let v = self.n(self.cat(Scan::opt(s, |t| self.reph(t)), Category::V));
        self.cat(v, Category::ZWJ).or(self.complex_syllable_tail(v))
    }

    fn standalone_cluster(&self, s: Ends) -> Ends {
        let placeholder = self.cat(
            Scan::opt(s, |t| self.one(t, set::REPHA_CS)),
            Category::PLACEHOLDER,
        );
        let dotted = self.cat(Scan::opt(s, |t| self.reph(t)), Category::DOTTED_CIRCLE);
        self.complex_syllable_tail(self.n(placeholder.or(dotted)))
    }

    fn symbol_cluster(&self, s: Ends) -> Ends {
        let symbol = Scan::opt(self.cat(s, Category::SYMBOL), |t| self.cat(t, Category::N));
        self.syllable_tail(symbol)
    }

    fn broken_cluster(&self, s: Ends) -> Ends {
        self.complex_syllable_tail(self.n(Scan::opt(s, |t| self.reph(t))))
    }
}

/// **The syllable at the start of `cats`**: its length and kind, the
/// longest of the grammar's patterns, the first listed on a tie, and one
/// glyph of no syllable when none matches -- a scanner's longest match
/// (`[[source: harfbuzz-14-5-1-shaper-indic-machine]]`, `main`). A broken
/// cluster is marks with no base, which the engine will put on a dotted
/// circle.
fn scan(cats: &[Category]) -> (usize, Kind) {
    let s = Scan { cats };
    let start = Ends::START;
    let candidates = [
        (s.consonant_syllable(start), Kind::Consonant),
        (s.vowel_syllable(start), Kind::Vowel),
        (s.standalone_cluster(start), Kind::Standalone),
        (s.symbol_cluster(start), Kind::Symbol),
        (s.cat(start, Category::SMPST), Kind::NonIndic),
        (s.broken_cluster(start), Kind::Broken),
    ];
    let mut best = (1usize, Kind::NonIndic);
    let mut longest = 0usize;
    for (ends, kind) in candidates {
        if let Some(At(n)) = ends.longest() {
            if n > longest {
                longest = n;
                best = (n, kind);
            }
        }
    }
    best
}

/// **Breaks the run into syllables** (Find next syllable in the input): each
/// glyph takes its syllable's serial and kind.
pub(crate) fn find_syllables(glyphs: &mut [Glyph]) {
    let mut p = 0usize;
    let mut serial = Serial::FIRST;
    let mut cats = [Category::X; WINDOW];
    while p < glyphs.len() {
        let w = (glyphs.len() - p).min(WINDOW);
        for (slot, g) in cats.iter_mut().zip(glyphs.iter().skip(p).take(w)) {
            *slot = g.syllabic.category;
        }
        let (len, kind) = scan(cats.get(..w).unwrap_or(&[]));
        let len = len.clamp(1, w);
        for g in glyphs.iter_mut().skip(p).take(len) {
            g.syllabic.syllable = Syllable::new(serial, kind);
        }
        serial = serial.next();
        p += len;
    }
}

/// "If it ligated, all bets are off": a ligature is no longer what its
/// first component was.
fn is_one_of(g: &Glyph, of: Categories) -> bool {
    !g.syllabic.formed.has(Formed::LIGATED) && of.has(g.syllabic.category)
}

/// The classes the engine treats as consonants: "We treat Vowels and
/// placeholders as if they were consonants" (HarfBuzz), which lets a vowel
/// syllable and a stand-alone cluster go through the consonant
/// syllable's reordering.
const CONSONANTS: Categories = Category::C
    .set()
    .or(Category::CS)
    .or(Category::RA)
    .or(Category::CM)
    .or(Category::V)
    .or(Category::PLACEHOLDER)
    .or(Category::DOTTED_CIRCLE);

fn is_consonant(g: &Glyph) -> bool {
    is_one_of(g, CONSONANTS)
}

fn is_joiner(g: &Glyph) -> bool {
    is_one_of(g, set::JOINER)
}

fn is_halant(g: &Glyph) -> bool {
    is_one_of(g, Category::H.set())
}

/// **Merges the cells of `glyphs[start..end]`**, as a ligature spans the
/// cells of its components: each takes the first cell and spans to the
/// last, and a glyph beside the range in one of its edge cells goes with
/// them (HarfBuzz's `merge_clusters()`). Reordering a glyph out of its cell
/// draws it over the cells it moved across. At
/// [`ClusterLevel::Characters`] nothing merges and each glyph keeps its
/// cell, as `merge_clusters()` at a level that is not monotone only marks
/// the range unsafe to break (`[[source: harfbuzz-14-5-1-buffer-hh]]`).
fn merge(level: ClusterLevel, glyphs: &mut [Glyph], span: Range<usize>) {
    let Range { start, end } = span;
    if level == ClusterLevel::Characters || end <= start + 1 || end > glyphs.len() {
        return;
    }
    let range = glyphs.get(start..end).unwrap_or(&[]);
    let first = range
        .iter()
        .map(|g| g.cluster)
        .min()
        .unwrap_or(glyphs[start].cluster);
    let (mut start, mut end) = (start, end);
    if first != glyphs[end - 1].cluster {
        while end < glyphs.len() && glyphs[end - 1].cluster == glyphs[end].cluster {
            end += 1;
        }
    }
    if first != glyphs[start].cluster {
        while start > 0 && glyphs[start - 1].cluster == glyphs[start].cluster {
            start -= 1;
        }
    }
    let last = glyphs[start..end]
        .iter()
        .map(|g| g.last)
        .max()
        .unwrap_or(first);
    for g in &mut glyphs[start..end] {
        g.cluster = first;
        g.last = last;
    }
}

/// A small memo of consonant positions by glyph, so a run asks the face
/// once per consonant glyph rather than once per occurrence.
struct Positions {
    slots: [Option<(GlyphId, Position)>; 64],
}

impl Positions {
    fn get(&mut self, plan: &Plan<'_>, consonant: GlyphId, virama: GlyphId) -> Position {
        let slot = consonant.index() % self.slots.len();
        if let Some(Some((g, p))) = self.slots.get(slot) {
            if *g == consonant {
                return *p;
            }
        }
        let p = plan.consonant_position(consonant, virama);
        if let Some(s) = self.slots.get_mut(slot) {
            *s = Some((consonant, p));
        }
        p
    }
}

/// **The initial reordering** (Reorder characters, a to c): each
/// consonant's position read from the face, a dotted circle put on each
/// broken cluster, then each syllable's base found, its glyphs put in
/// their visual order and the masked features' masks set.
fn initial_reordering(plan: &Plan<'_>, buf: &mut Buffer<'_>) {
    if let Some(virama) = plan.virama {
        let mut memo = Positions { slots: [None; 64] };
        for g in buf.as_mut_slice() {
            if g.syllabic.position == Position::BASE_C {
                g.syllabic.position = memo.get(plan, g.id, virama);
            }
        }
    }
    insert_dotted_circles(plan, buf);
    let glyphs = buf.as_mut_slice();
    let mut start = 0usize;
    while start < glyphs.len() {
        let syllable = glyphs[start].syllabic.syllable;
        let end = glyphs
            .iter()
            .skip(start)
            .position(|g| g.syllabic.syllable != syllable)
            .map_or(glyphs.len(), |n| start + n);
        match syllable.kind() {
            Kind::Consonant | Kind::Vowel | Kind::Standalone | Kind::Broken => {
                reorder_syllable(plan, glyphs, start..end);
            }
            Kind::Symbol | Kind::NonIndic => {}
        }
        start = end;
    }
}

/// **A dotted circle on each broken cluster**: "a shaping engine
/// implementation might treat an invalid mark as a separate cluster and
/// display the stand-alone mark positioned on some default base glyph, such
/// as a dotted circle", and "it is recommended that a Devanagari OT font
/// contain a glyph for the dotted circle character, U+25CC. If this
/// character is not supported in the font, such implementations will
/// display invalid signs on the missing glyph shape" (Invalid Combining
/// Marks). It goes after a repha the cluster starts with, in the cell of
/// the glyph it goes before; a face with no dotted circle gets none, as
/// HarfBuzz inserts none (`[[source: harfbuzz-14-5-1-shaper-syllabic]]`).
fn insert_dotted_circles(plan: &Plan<'_>, buf: &mut Buffer<'_>) {
    let Some(dotted) = plan.dotted else {
        return;
    };
    let mut last = Syllable::NONE;
    let mut at = 0usize;
    while let Some(g) = buf.get(Index::of(u32::try_from(at).unwrap_or(u32::MAX))) {
        let syllable = g.syllabic.syllable;
        if syllable == last || syllable.kind() != Kind::Broken {
            at += 1;
            continue;
        }
        last = syllable;
        while buf
            .get(Index::of(u32::try_from(at).unwrap_or(u32::MAX)))
            .is_some_and(|h| h.syllabic.syllable == last && h.syllabic.category == Category::REPHA)
        {
            at += 1;
        }
        let Some(cur) = buf.get(Index::of(u32::try_from(at).unwrap_or(u32::MAX))) else {
            break;
        };
        let mut circle = cur;
        circle.id = dotted;
        circle.class = plan.gdef.class(dotted);
        circle.mark = match plan.gdef.has_classes() && plan.gdef.is_mark(dotted) {
            true => Mark::Yes,
            false => Mark::No,
        };
        circle.ignorable = Ignorable::No;
        circle.ccc = CombiningClass::STARTER;
        circle.syllabic = Syllabic {
            category: Category::DOTTED_CIRCLE,
            position: Position::END,
            masks: cur.syllabic.masks,
            syllable,
            formed: Formed::NONE,
        };
        if buf
            .put(Index::of(u32::try_from(at).unwrap_or(u32::MAX)), circle)
            .is_none()
        {
            break;
        }
        at += 2;
    }
}

/// HarfBuzz's initial reordering of one consonant syllable, `glyphs[start..
/// end]`, which a vowel syllable, a stand-alone cluster and a broken one
/// share (`initial_reordering_consonant_syllable()`), for the configured
/// script.
fn reorder_syllable(plan: &Plan<'_>, g: &mut [Glyph], span: Range<usize>) {
    let Range { start, end } = span;
    if end <= start || end > g.len() {
        return;
    }
    // 1. Find base consonant: "starting from the end of the syllable, move
    // backwards until a consonant is found that does not have a below-base
    // or post-base form (post-base forms have to follow below-base forms),
    // or that is not a pre-base reordering Ra, or arrive at the first
    // consonant. The consonant stopped at will be the base." And "If the
    // syllable starts with Ra + Halant (in a script that has Reph) and has
    // more than one consonant, Ra is excluded from candidates for base
    // consonants."
    let mut base = end;
    let mut has_reph = false;
    let mut limit = start;
    if plan.names.meets(Masks::RPHF)
        && start + 3 <= end
        && plan.config.reph_mode == RephMode::Implicit
        && !is_joiner(&g[start + 2])
        && plan.would(&plan.rphf, &[g[start].id, g[start + 1].id])
    {
        limit += 2;
        while limit < end && is_joiner(&g[limit]) {
            limit += 1;
        }
        base = start;
        has_reph = true;
    }
    {
        let mut i = end;
        let mut seen_below = false;
        loop {
            i -= 1;
            if is_consonant(&g[i]) {
                let p = g[i].syllabic.position;
                if p != Position::BELOW_C && (p != Position::POST_C || seen_below) {
                    base = i;
                    break;
                }
                if p == Position::BELOW_C {
                    seen_below = true;
                }
                base = i;
            } else if start < i
                && g[i].syllabic.category == Category::ZWJ
                && g[i - 1].syllabic.category == Category::H
            {
                // "A ZWJ after a Halant stops the base search, and requests
                // an explicit half form."
                break;
            }
            if i <= limit {
                break;
            }
        }
    }
    // A reph with no other consonant is no reph: RA is the base.
    if has_reph && base == start && limit - base <= 2 {
        has_reph = false;
    }

    // 2. Decompose and reorder matras: normalisation took the two- and
    // three-part matras apart, and the sort below moves each to its
    // class's position. 3. Reorder marks to canonical order: "Adjacent
    // nukta and halant or nukta and vedic sign are always repositioned if
    // necessary, so that the nukta is first" -- the canonical order
    // normalisation gave them.
    for x in &mut g[start..base] {
        x.syllabic.position = x.syllabic.position.min(Position::PRE_C);
    }
    if base < end {
        g[base].syllabic.position = Position::BASE_C;
    }
    if has_reph {
        g[start].syllabic.position = Position::RA_TO_BECOME_REPH;
    }
    // The old specification's fonts expect the first halant after the base
    // after the last consonant (HarfBuzz, from Uniscribe).
    if plan.spec == Spec::Old {
        if let Some(i) = (base + 1..end).find(|&i| g[i].syllabic.category == Category::H) {
            let j = (i + 1..end)
                .rev()
                .find(|&j| is_consonant(&g[j]))
                .unwrap_or(i);
            if g[j].syllabic.category != Category::H && j > i {
                g[i..=j].rotate_left(1);
            }
        }
    }
    // Attach the marks that follow a glyph to it, so they move with it.
    {
        let movers = set::JOINER
            .or(Category::N)
            .or(Category::RS)
            .or(Category::CM)
            .or(Category::H);
        let mut last_pos = Position::START;
        for i in start..end {
            let c = g[i].syllabic.category;
            if movers.has(c) {
                g[i].syllabic.position = last_pos;
                if c == Category::H && last_pos == Position::PRE_M {
                    // "Uniscribe doesn't move the Halant with Left Matra."
                    if let Some(j) = (start..i)
                        .rev()
                        .find(|&j| g[j].syllabic.position != Position::PRE_M)
                    {
                        g[i].syllabic.position = g[j].syllabic.position;
                    }
                }
            } else if g[i].syllabic.position != Position::SMVD {
                if c == Category::MPST && i > start && g[i - 1].syllabic.category == Category::SM {
                    g[i - 1].syllabic.position = g[i].syllabic.position;
                }
                last_pos = g[i].syllabic.position;
            }
        }
    }
    // A post-base consonant owns what comes before it since the last
    // consonant or matra.
    {
        let mut last = base;
        for i in base + 1..end {
            if is_consonant(&g[i]) {
                for j in last + 1..i {
                    if g[j].syllabic.position < Position::SMVD {
                        g[j].syllabic.position = g[i].syllabic.position;
                    }
                }
                last = i;
            } else if set::MATRA.has(g[i].syllabic.category) {
                last = i;
            }
        }
    }
    // Sort by position, stably, each glyph remembering where it was.
    let syllable = g[start].syllabic.syllable;
    let len = end - start;
    let tracked = len <= 127;
    if tracked {
        for (k, x) in g[start..end].iter_mut().enumerate() {
            x.syllabic.syllable = Syllable::scratch(At(k));
        }
    }
    for i in start + 1..end {
        let mut j = i;
        while j > start && g[j - 1].syllabic.position > g[j].syllabic.position {
            g.swap(j - 1, j);
            j -= 1;
        }
    }
    // Find the base again, and put a run of left matras back in their
    // logical order (HarfBuzz issue 3863).
    let mut first_left = end;
    let mut last_left = end;
    base = end;
    for (i, x) in g.iter().enumerate().take(end).skip(start) {
        let p = x.syllabic.position;
        if p == Position::BASE_C {
            base = i;
            break;
        }
        if p == Position::PRE_M {
            if first_left == end {
                first_left = i;
            }
            last_left = i;
        }
    }
    if first_left < last_left {
        g[first_left..=last_left].reverse();
        let mut i = first_left;
        for j in first_left..=last_left {
            if set::MATRA.has(g[j].syllabic.category) {
                g[i..=j].reverse();
                i = j + 1;
            }
        }
    }
    // What moved from after the base to before it, or around after it,
    // merges cells with the base; what is before it merges in the final
    // reordering.
    if plan.spec == Spec::Old || !tracked {
        merge(plan.clusters, g, base..end);
    } else {
        for i in base..end {
            if g[i].syllabic.syllable.is_visited() {
                continue;
            }
            let (mut lo, mut hi) = (i, i);
            let mut j = start + g[i].syllabic.syllable.scratch_index().0;
            while j != i && j < end {
                lo = lo.min(j);
                hi = hi.max(j);
                let next = start + g[j].syllabic.syllable.scratch_index().0;
                g[j].syllabic.syllable = Syllable::VISITED;
                j = next;
            }
            merge(plan.clusters, g, base.max(lo)..hi + 1);
        }
    }
    for x in &mut g[start..end] {
        x.syllabic.syllable = syllable;
    }

    // The masks: reph's, the pre-base forms', the post-base forms'.
    let names = plan.names;
    for x in g[start..end]
        .iter_mut()
        .take_while(|x| x.syllabic.position == Position::RA_TO_BECOME_REPH)
    {
        x.syllabic.masks = x.syllabic.masks.or(Masks::RPHF.and(names));
    }
    let mut pre = Masks::HALF;
    if plan.spec == Spec::New && plan.config.blwf == BlwfMode::PreAndPost {
        pre = pre.or(Masks::BLWF);
    }
    for x in &mut g[start..base.min(end)] {
        x.syllabic.masks = x.syllabic.masks.or(pre.and(names));
    }
    let post = Masks::BLWF.or(Masks::ABVF).or(Masks::PSTF);
    for x in g.iter_mut().take(end).skip(base + 1) {
        x.syllabic.masks = x.syllabic.masks.or(post.and(names));
    }
    // The old specification's eyelash RA: RA and halant before the base,
    // not followed by ZWJ, take the below-base forms.
    if plan.spec == Spec::Old {
        for i in start..base.saturating_sub(1) {
            if g[i].syllabic.category == Category::RA
                && g[i + 1].syllabic.category == Category::H
                && (i + 2 == base || g[i + 2].syllabic.category != Category::ZWJ)
            {
                for x in &mut g[i..i + 2] {
                    x.syllabic.masks = x.syllabic.masks.or(Masks::BLWF.and(names));
                }
            }
        }
    }
    // A halant and RA after the base that `pref` would form are marked for
    // pre-base reordering.
    if names.meets(Masks::PREF) && base + 2 < end {
        if let Some(i) =
            (base + 1..end - 1).find(|&i| plan.would(&plan.pref, &[g[i].id, g[i + 1].id]))
        {
            for x in &mut g[i..i + 2] {
                x.syllabic.masks = x.syllabic.masks.or(Masks::PREF);
            }
        }
    }
    // "The main intent of using ZWNJ is to prevent conjunct ligature or
    // half forms from forming ... The shaping engine must take specific
    // actions to prevent half forms for a sequence of Consonant + Halant +
    // ZWNJ" (Effect of ZWJ, ZWNJ and NBSP): a ZWNJ takes the half mask
    // from the glyphs back to the consonant before it. ZWJ needs nothing:
    // the manual joiners keep a conjunct from matching across it.
    for i in start + 1..end {
        if !is_joiner(&g[i]) || g[i].syllabic.category != Category::ZWNJ {
            continue;
        }
        let mut j = i;
        loop {
            j -= 1;
            g[j].syllabic.masks = g[j].syllabic.masks.without(Masks::HALF);
            if j <= start || is_consonant(&g[j]) {
                break;
            }
        }
    }
}

/// **The final reordering** of each syllable (Final reordering): after the
/// basic shaping forms, the pre-base matra moves to its base, reph to its
/// place and a pre-base reordering consonant before the base.
fn final_reordering(plan: &Plan<'_>, glyphs: &mut [Glyph]) {
    let mut start = 0usize;
    while start < glyphs.len() {
        let syllable = glyphs[start].syllabic.syllable;
        let end = glyphs
            .iter()
            .skip(start)
            .position(|g| g.syllabic.syllable != syllable)
            .map_or(glyphs.len(), |n| start + n);
        final_syllable(plan, glyphs, start..end);
        start = end;
    }
}

/// HarfBuzz's final reordering of one syllable
/// (`final_reordering_syllable_indic()`), for the configured script.
fn final_syllable(plan: &Plan<'_>, g: &mut [Glyph], span: Range<usize>) {
    let Range { start, end } = span;
    if end <= start || end > g.len() {
        return;
    }
    // A virama a ligature took apart again is a halant once more.
    if let Some(v) = plan.virama {
        for x in &mut g[start..end] {
            let f = x.syllabic.formed;
            if x.id == v && f.has(Formed::LIGATED) && f.has(Formed::MULTIPLIED) {
                x.syllabic.category = Category::H;
                x.syllabic.formed = f.without(Formed::LIGATED.or(Formed::MULTIPLIED));
            }
        }
    }
    let mut try_pref = plan.names.meets(Masks::PREF);

    // Find the base again.
    let mut base = start;
    while base < end {
        if g[base].syllabic.position >= Position::BASE_C {
            if try_pref && base + 1 < end {
                if let Some(i) = (base + 1..end).find(|&i| g[i].syllabic.masks.meets(Masks::PREF)) {
                    let f = g[i].syllabic.formed;
                    if !(f.has(Formed::SUBSTITUTED) && f.ligated_alone()) {
                        // A `pref` candidate that formed nothing: the base
                        // is around here.
                        base = i;
                        while base < end && is_halant(&g[base]) {
                            base += 1;
                        }
                        if base < end {
                            g[base].syllabic.position = Position::BASE_C;
                        }
                        try_pref = false;
                    }
                }
                if base == end {
                    break;
                }
            }
            if start < base && g[base].syllabic.position > Position::BASE_C {
                base -= 1;
            }
            break;
        }
        base += 1;
    }
    if base == end && start < base && is_one_of(&g[base - 1], Category::ZWJ.set()) {
        base -= 1;
    }
    if base < end {
        while start < base && is_one_of(&g[base], Category::N.set().or(Category::H)) {
            base -= 1;
        }
    }

    // Reorder matras: "If a pre-base matra character had been reordered
    // before applying basic features, the glyph can be moved closer to the
    // main consonant based on whether half-forms had been formed. Actual
    // position for the matra is defined as 'after last standalone halant
    // glyph, after initial matra position and before the main consonant'."
    if start + 1 < end && start < base {
        let mut new_pos = if base == end { base - 2 } else { base - 1 };
        let stops = set::MATRA.or(Category::H);
        loop {
            while new_pos > start && !is_one_of(&g[new_pos], stops) {
                new_pos -= 1;
            }
            if is_halant(&g[new_pos]) && g[new_pos].syllabic.position != Position::PRE_M {
                // The first open departure: a ZWJ after the halant keeps the
                // matra searching left.
                if new_pos + 1 < end
                    && g[new_pos + 1].syllabic.category == Category::ZWJ
                    && new_pos > start
                {
                    new_pos -= 1;
                    continue;
                }
            } else {
                new_pos = start;
            }
            break;
        }
        if start < new_pos && g[new_pos].syllabic.position != Position::PRE_M {
            let mut i = new_pos;
            while i > start {
                if g[i - 1].syllabic.position == Position::PRE_M {
                    let old_pos = i - 1;
                    if old_pos < base && base <= new_pos {
                        base -= 1;
                    }
                    g[old_pos..=new_pos].rotate_left(1);
                    merge(plan.clusters, g, new_pos..end.min(base + 1));
                    new_pos -= 1;
                }
                i -= 1;
            }
        } else if let Some(i) = (start..base).find(|&i| g[i].syllabic.position == Position::PRE_M) {
            merge(plan.clusters, g, i..end.min(base + 1));
        }
    }

    // Reorder reph: "Reph's original position is always at the beginning of
    // the syllable ... However, it will be reordered according to the
    // basic-forms shaping results." RA and halant move only where they
    // ligated to the reph form.
    if start + 1 < end
        && g[start].syllabic.position == Position::RA_TO_BECOME_REPH
        && ((g[start].syllabic.category == Category::REPHA)
            ^ g[start].syllabic.formed.ligated_alone())
    {
        let At(new_reph_pos) = reph_target(plan, g, start..end, At(base));
        merge(plan.clusters, g, start..new_reph_pos + 1);
        g[start..=new_reph_pos].rotate_left(1);
        if start < base && base <= new_reph_pos {
            base -= 1;
        }
    }

    // Reorder pre-base reordering consonants: "Only reorder a glyph
    // produced by substitution during application of the 'pref' feature
    // ... Try to find a target position the same way as for pre-base
    // matra. If it is found, reorder pre-base consonant glyph. If position
    // is not found, reorder immediately before main consonant."
    if try_pref && base + 1 < end {
        if let Some(i) = (base + 1..end).find(|&i| g[i].syllabic.masks.meets(Masks::PREF)) {
            if g[i].syllabic.formed.ligated_alone() {
                let mut new_pos = base;
                let stops = set::MATRA.or(Category::H);
                while new_pos > start && !is_one_of(&g[new_pos - 1], stops) {
                    new_pos -= 1;
                }
                if new_pos > start
                    && is_halant(&g[new_pos - 1])
                    && new_pos < end
                    && is_joiner(&g[new_pos])
                {
                    new_pos += 1;
                }
                let old_pos = i;
                merge(plan.clusters, g, new_pos..old_pos + 1);
                g[new_pos..=old_pos].rotate_right(1);
            }
        }
    }
}

/// Where reph goes: the steps of "Reorder reph", for a script whose reph
/// goes before the post-base forms. Step 2: "target position is after the
/// first explicit halant glyph between the first post-reph consonant and
/// last main consonant. If ZWJ or ZWNJ are following this halant, position
/// is moved after it." Step 5 repeats it; step 6: "Otherwise, reorder reph
/// to the end of the syllable", before the syllable modifiers and Vedic
/// signs, and before a halant that ends a matra's sequence.
fn reph_target(plan: &Plan<'_>, g: &[Glyph], span: Range<usize>, At(base): At) -> At {
    let Range { start, end } = span;
    let after_halant = || {
        let mut p = start + 1;
        while p < base && !is_halant(&g[p]) {
            p += 1;
        }
        (p < base && is_halant(&g[p])).then(|| match p + 1 < base && is_joiner(&g[p + 1]) {
            true => p + 1,
            false => p,
        })
    };
    if plan.config.reph != Position::AFTER_POST {
        if let Some(p) = after_halant() {
            return At(p);
        }
        if plan.config.reph == Position::AFTER_MAIN {
            let mut p = base;
            while p + 1 < end && g[p + 1].syllabic.position <= Position::AFTER_MAIN {
                p += 1;
            }
            if p < end {
                return At(p);
            }
        }
        if plan.config.reph == Position::AFTER_SUB {
            let mut p = base;
            while p + 1 < end
                && !matches!(
                    g[p + 1].syllabic.position,
                    Position::POST_C | Position::AFTER_POST | Position::SMVD
                )
            {
                p += 1;
            }
            if p < end {
                return At(p);
            }
        }
    }
    if let Some(p) = after_halant() {
        return At(p);
    }
    let mut p = end - 1;
    while p > start && g[p].syllabic.position == Position::SMVD {
        p -= 1;
    }
    if is_halant(&g[p]) {
        let mut i = base + 1;
        while i < p {
            if set::MATRA.has(g[i].syllabic.category) {
                p -= 1;
            }
            i += 1;
        }
    }
    At(p)
}

/// **A dotted circle in a vowel sequence the Unicode Standard says not to
/// use**: Table 12-1 shows "vowel letters that can be analyzed, the single
/// code point that should be used to represent them in text, and the
/// sequence of code points resulting from analysis that should not be
/// used" (`[[source: unicode-ch12]]` 12.1). Such a sequence is drawn as
/// the vowel letter and the sign on a dotted circle, so it does not pass
/// for the letter it imitates; HarfBuzz does the same
/// (`[[source: harfbuzz-14-5-1-vowel-constraints]]`), on a face its Indic
/// shaper takes ([`takes`]). Fed one character at a time, in logical
/// order.
#[derive(Clone, Copy, Default)]
pub(crate) struct Vowels {
    /// The two characters before, each `None` once a match has used it.
    prev: Option<Codepoint>,
    prev2: Option<Codepoint>,
}

impl Vowels {
    /// Whether a dotted circle goes before `c`, the character after the
    /// ones fed so far; `c` is fed.
    pub(crate) fn before(&mut self, c: Codepoint) -> bool {
        let v = c.value();
        let three = self.prev2.map(|p| p.value()) == Some(0x0930)
            && self.prev.map(|p| p.value()) == Some(0x094D)
            && v == 0x0907;
        let two = self.prev.is_some_and(|p| Vowels::analysed(p, c));
        if three || two {
            self.prev = None;
            self.prev2 = None;
            return true;
        }
        self.prev2 = self.prev;
        self.prev = Some(c);
        false
    }

    /// Table 12-1's two-character sequences: a vowel letter and the sign
    /// that would make it another.
    fn analysed(a: Codepoint, b: Codepoint) -> bool {
        let b = b.value();
        match a.value() {
            0x0905 => matches!(
                b,
                0x093A
                    | 0x093B
                    | 0x093E
                    | 0x0945
                    | 0x0946
                    | 0x0949
                    | 0x094A
                    | 0x094B
                    | 0x094C
                    | 0x094F
                    | 0x0956
                    | 0x0957
            ),
            0x0906 => matches!(b, 0x093A | 0x0945 | 0x0946 | 0x0947 | 0x0948),
            0x0909 => b == 0x0941,
            0x090F => matches!(b, 0x0945..=0x0947),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use crate::syllabic::Category as C;
    use alloc::vec::Vec;

    fn scan_all(cats: &[Category]) -> Vec<(usize, Kind)> {
        let mut out = Vec::new();
        let mut p = 0usize;
        while p < cats.len() {
            let (n, k) = scan(&cats[p..]);
            out.push((n, k));
            p += n.max(1);
        }
        out
    }

    /// The specification's patterns, and the cluster of marks with no base
    /// the grammar calls broken.
    #[test]
    fn the_grammar_finds_the_specification_s_syllables() {
        // कि: a consonant and its matra.
        assert_eq!(scan_all(&[C::C, C::M]), [(2, Kind::Consonant)]);
        // क्ष: C H C.
        assert_eq!(scan_all(&[C::C, C::H, C::C]), [(3, Kind::Consonant)]);
        // र्कि: Ra H C M, one syllable.
        assert_eq!(scan_all(&[C::RA, C::H, C::C, C::M]), [(4, Kind::Consonant)]);
        // क्‌ष: a halant and ZWNJ end a syllable.
        assert_eq!(
            scan_all(&[C::C, C::H, C::ZWNJ, C::C]),
            [(3, Kind::Consonant), (1, Kind::Consonant)]
        );
        // क्‍ष: ZWJ keeps the conjunct one syllable.
        assert_eq!(
            scan_all(&[C::C, C::H, C::ZWJ, C::C]),
            [(4, Kind::Consonant)]
        );
        // आं: a vowel and a syllable modifier.
        assert_eq!(scan_all(&[C::V, C::SM]), [(2, Kind::Vowel)]);
        // NBSP and a matra: a stand-alone cluster.
        assert_eq!(scan_all(&[C::PLACEHOLDER, C::M]), [(2, Kind::Standalone)]);
        // A matra alone, and after a halant: broken.
        assert_eq!(scan_all(&[C::M]), [(1, Kind::Broken)]);
        assert_eq!(
            scan_all(&[C::C, C::H, C::M]),
            [(2, Kind::Consonant), (1, Kind::Broken)]
        );
        // A space, and ZWJ alone: no syllable.
        assert_eq!(scan_all(&[C::X]), [(1, Kind::NonIndic)]);
        assert_eq!(scan_all(&[C::ZWJ]), [(1, Kind::NonIndic)]);
        // बडे़: a nukta after a matra stays in the syllable.
        assert_eq!(
            scan_all(&[C::C, C::C, C::M, C::N]),
            [(1, Kind::Consonant), (3, Kind::Consonant)]
        );
    }

    /// Table 12-1's sequences take a dotted circle before their sign, and
    /// a sign the table does not pair with the letter takes none.
    #[test]
    fn a_vowel_letter_and_the_sign_that_imitates_another_take_a_dotted_circle() {
        let feed = |cps: &[u32]| -> Vec<bool> {
            let mut v = Vowels::default();
            cps.iter()
                .map(|&c| v.before(Codepoint::new(c).unwrap()))
                .collect()
        };
        assert_eq!(feed(&[0x0905, 0x093E]), [false, true]);
        assert_eq!(feed(&[0x0930, 0x094D, 0x0907]), [false, false, true]);
        assert_eq!(feed(&[0x0909, 0x0941]), [false, true]);
        assert_eq!(feed(&[0x0915, 0x093E]), [false, false]);
        assert_eq!(feed(&[0x0905, 0x0947]), [false, false]);
        // A sign the match used cannot begin another.
        assert_eq!(feed(&[0x0905, 0x093E, 0x093E]), [false, true, false]);
    }
}
