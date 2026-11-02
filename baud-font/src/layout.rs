//! The OpenType Layout common tables (`[[source: ms-opentype-chapter2]]`)
//! and `GDEF` (`[[source: ms-opentype-gdef]]`): what `GSUB` and `GPOS`
//! share.
//!
//! **Scripts, features, lookups.** A layout table names its features under
//! a script and a language system, each feature lists lookups, and the
//! lookups are applied in `LookupList` order over the union of what the
//! selected features name. [`Layout::lookups_for`] does that selection for
//! a run's [`Script`] and yields the sorted, deduplicated lookup indices.
//!
//! **The Script table is the text's own, else `DFLT`, as the specification
//! directs:** "An application should use a DFLT script table if there is not
//! a script table associated with the specific script of the text being
//! formatted, or if the text does not have a specific script (for example,
//! it contains only symbols or punctuation)" (`[[source: ms-opentype-chapter2]]`,
//! ScriptList table). The script's registered tags are tried in order
//! ([`Script::tags`]), then `DFLT`, and its default language system is used;
//! no language is ever asked for.
//!
//! **Then `latn`, which no text asks for and which is kept on purpose.** A
//! face with neither the script's table nor `DFLT` gets `latn`'s features --
//! what baud gave every script before it read one, and what HarfBuzz does
//! too ("some old fonts put their features there even though they're really
//! trying to support Thai", `[[source: harfbuzz-hb-ot-layout]]`, its script
//! selection). The specification is silent on a face with no `DFLT`, so
//! today's behaviour stands. Two other HarfBuzz
//! choices are *not* made, and both are recorded as the open question Q6 of
//! `docs/research/notes/ab-right-to-left.md`: the misspelt `dflt` tried
//! before `latn`, and the Hebrew rule that turns `GPOS` off and places
//! marks by combining class when the `GPOS` table chosen is not `hebr`
//! (`[[source: harfbuzz-hb-ot-shape]]`, `disable_gpos`; the Hebrew shaper's
//! `gpos_tag`, `[[source: harfbuzz-hb-ot-shaper-hebrew]]`). Where `DFLT`
//! anchors Hebrew marks and `hebr` is absent, the specification sends the
//! text to `DFLT` and baud uses those anchors.
//!
//! **Coverage and classes** are the two ways a subtable names glyphs:
//! `Coverage` answers an index for a glyph it lists, in format 1 as a
//! sorted array and format 2 as sorted ranges; `ClassDef` answers a class
//! with 0 for everything unlisted.
//!
//! **Lookup flags and `GDEF`** decide which glyphs a lookup skips while it
//! matches: base glyphs, ligatures or marks by `GDEF` glyph class, marks
//! outside an attachment class, marks outside a mark filtering set. That is
//! [`Filter`], built once per lookup and asked once per glyph.

use crate::bytes::{Array, Bytes, Count, FromBe, Index, Len, Offset};
use crate::script::Script;
use baud_domain::{GlyphId, JoiningForm};

/// A four-byte OpenType script tag, as a `ScriptRecord` spells it:
/// `hebr`, `latn`, `DFLT` (`[[source: ms-opentype-scripttags]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScriptTag(pub [u8; 4]);

impl ScriptTag {
    /// The table for text with no specific script, and the fallback for a
    /// script the face has no table for.
    pub const DFLT: ScriptTag = ScriptTag(*b"DFLT");
    /// The last resort for a face with neither, as before scripts were read.
    pub const LATN: ScriptTag = ScriptTag(*b"latn");
}

/// A four-byte feature or script tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeatureTag(pub [u8; 4]);

impl FeatureTag {
    pub const CCMP: FeatureTag = FeatureTag(*b"ccmp");
    pub const CALT: FeatureTag = FeatureTag(*b"calt");
    pub const LIGA: FeatureTag = FeatureTag(*b"liga");
    pub const MARK: FeatureTag = FeatureTag(*b"mark");
    pub const MKMK: FeatureTag = FeatureTag(*b"mkmk");
    pub const ISOL: FeatureTag = FeatureTag(*b"isol");
    pub const FINA: FeatureTag = FeatureTag(*b"fina");
    pub const MEDI: FeatureTag = FeatureTag(*b"medi");
    pub const INIT: FeatureTag = FeatureTag(*b"init");
    pub const RLIG: FeatureTag = FeatureTag(*b"rlig");
    pub const FIN2: FeatureTag = FeatureTag(*b"fin2");
    pub const FIN3: FeatureTag = FeatureTag(*b"fin3");
    pub const MED2: FeatureTag = FeatureTag(*b"med2");
    pub const RCLT: FeatureTag = FeatureTag(*b"rclt");
    pub const LOCL: FeatureTag = FeatureTag(*b"locl");
    pub const NUKT: FeatureTag = FeatureTag(*b"nukt");
    pub const AKHN: FeatureTag = FeatureTag(*b"akhn");
    pub const RPHF: FeatureTag = FeatureTag(*b"rphf");
    pub const RKRF: FeatureTag = FeatureTag(*b"rkrf");
    pub const PREF: FeatureTag = FeatureTag(*b"pref");
    pub const BLWF: FeatureTag = FeatureTag(*b"blwf");
    pub const ABVF: FeatureTag = FeatureTag(*b"abvf");
    pub const HALF: FeatureTag = FeatureTag(*b"half");
    pub const PSTF: FeatureTag = FeatureTag(*b"pstf");
    pub const VATU: FeatureTag = FeatureTag(*b"vatu");
    pub const CJCT: FeatureTag = FeatureTag(*b"cjct");
    pub const PRES: FeatureTag = FeatureTag(*b"pres");
    pub const ABVS: FeatureTag = FeatureTag(*b"abvs");
    pub const BLWS: FeatureTag = FeatureTag(*b"blws");
    pub const PSTS: FeatureTag = FeatureTag(*b"psts");
    pub const HALN: FeatureTag = FeatureTag(*b"haln");
    pub const ABVM: FeatureTag = FeatureTag(*b"abvm");
    pub const BLWM: FeatureTag = FeatureTag(*b"blwm");
    pub const DIST: FeatureTag = FeatureTag(*b"dist");
    pub const KERN: FeatureTag = FeatureTag(*b"kern");

    /// The feature that draws a joining letter in `form`: OpenType's names
    /// for the Unicode Standard's Xn, Xr, Xm and Xl in a right-to-left run,
    /// and for alaph's Afn, Afx and its Xr inside a word
    /// (`[[source: ms-syriac-script-development]]`).
    pub fn of_form(form: JoiningForm) -> FeatureTag {
        match form {
            JoiningForm::Isolated => FeatureTag::ISOL,
            JoiningForm::Final => FeatureTag::FINA,
            JoiningForm::Medial => FeatureTag::MEDI,
            JoiningForm::Initial => FeatureTag::INIT,
            JoiningForm::Final2 => FeatureTag::FIN2,
            JoiningForm::Final3 => FeatureTag::FIN3,
            JoiningForm::Medial2 => FeatureTag::MED2,
        }
    }

    /// Whether this is one of the joining features: the seven forms, the
    /// required ligatures and the required contextual alternates, which
    /// [`Features::joining`] turns on.
    #[inline]
    pub fn is_joining(self) -> bool {
        matches!(
            self,
            FeatureTag::ISOL
                | FeatureTag::FINA
                | FeatureTag::FIN2
                | FeatureTag::FIN3
                | FeatureTag::MEDI
                | FeatureTag::MED2
                | FeatureTag::INIT
                | FeatureTag::RLIG
                | FeatureTag::RCLT
        )
    }
}

impl FromBe for FeatureTag {
    const SIZE: Len = Len::of(4);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(FeatureTag([
            u8::parse_be(b)?,
            u8::parse_be(b.get(1..)?)?,
            u8::parse_be(b.get(2..)?)?,
            u8::parse_be(b.get(3..)?)?,
        ]))
    }
}

/// A tag paired with a 16-bit offset: `ScriptRecord`, `LangSysRecord`,
/// `FeatureRecord`.
#[derive(Clone, Copy)]
struct TagRecord {
    tag: FeatureTag,
    offset: Offset,
}

impl FromBe for TagRecord {
    const SIZE: Len = Len::of(6);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(TagRecord {
            tag: FeatureTag::parse_be(b)?,
            offset: Offset::of(u16::parse_be(b.get(4..)?)?),
        })
    }
}

/// A glyph id as a table stores it.
fn glyph(b: &[u8]) -> Option<GlyphId> {
    Some(GlyphId::from_shaper(u16::parse_be(b)?))
}

/// A coverage table: which glyphs a subtable applies to, and each one's
/// index into the subtable's arrays.
#[derive(Clone, Copy)]
pub enum Coverage<'a> {
    /// Format 1: a sorted array of glyph ids; the index is the position.
    List(Array<'a, u16>),
    /// Format 2: sorted ranges each carrying the index of its first glyph.
    Ranges(Array<'a, Range>),
}

/// A coverage format 2 `RangeRecord`.
#[derive(Clone, Copy)]
pub struct Range {
    start: GlyphId,
    end: GlyphId,
    first_index: Index,
}

impl FromBe for Range {
    const SIZE: Len = Len::of(6);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Range {
            start: glyph(b)?,
            end: glyph(b.get(2..)?)?,
            first_index: Index::of(u16::parse_be(b.get(4..)?)?),
        })
    }
}

/// A summary of which glyphs a set may contain, in HarfBuzz's shape:
/// three 64-bit masks indexed by the glyph id shifted by 0, 4 and 9 bits.
/// `may_have` answers `false` only when the two sets are surely disjoint,
/// which is what lets a lookup be skipped without being opened.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Digest {
    words: [u64; 3],
}

impl Digest {
    const SHIFTS: [u32; 3] = [0, 4, 9];
    pub const EMPTY: Digest = Digest { words: [0; 3] };
    /// Everything: what a lookup whose coverage cannot be read gets, so it
    /// is never skipped.
    pub const ALL: Digest = Digest {
        words: [u64::MAX; 3],
    };

    /// The digest holding one glyph.
    pub fn of(g: GlyphId) -> Digest {
        let mut d = Digest::EMPTY;
        d.add(g);
        d
    }

    pub fn add(&mut self, g: GlyphId) {
        let id = u32::try_from(g.index()).unwrap_or(u32::MAX);
        for (w, shift) in self.words.iter_mut().zip(Digest::SHIFTS) {
            *w |= 1u64 << ((id >> shift) & 63);
        }
    }

    /// Every glyph from `a` to `b` inclusive; a range too wide for a word
    /// fills it.
    pub fn add_range(&mut self, a: GlyphId, b: GlyphId) {
        let (lo, hi) = (
            u32::try_from(a.index()).unwrap_or(u32::MAX),
            u32::try_from(b.index()).unwrap_or(u32::MAX),
        );
        for (w, shift) in self.words.iter_mut().zip(Digest::SHIFTS) {
            let (lo, hi) = (lo >> shift, hi >> shift);
            if hi.saturating_sub(lo) >= 63 {
                *w = u64::MAX;
                continue;
            }
            let mut i = lo;
            while i <= hi {
                *w |= 1u64 << (i & 63);
                i += 1;
            }
        }
    }

    pub fn union(self, o: Digest) -> Digest {
        Digest {
            words: [
                self.words[0] | o.words[0],
                self.words[1] | o.words[1],
                self.words[2] | o.words[2],
            ],
        }
    }

    /// Whether the two sets may share a glyph.
    pub fn may_have(self, o: Digest) -> bool {
        self.words.iter().zip(o.words).all(|(a, b)| a & b != 0)
    }
}

impl<'a> Coverage<'a> {
    /// The digest of every glyph this coverage names.
    pub fn digest(&self) -> Digest {
        let mut d = Digest::EMPTY;
        match self {
            Coverage::List(list) => {
                let mut i = Index::ZERO;
                while let Some(g) = list.get(i) {
                    d.add(GlyphId::from_shaper(g));
                    i = i.next();
                }
            }
            Coverage::Ranges(ranges) => {
                let mut i = Index::ZERO;
                while let Some(r) = ranges.get(i) {
                    d.add_range(r.start, r.end);
                    i = i.next();
                }
            }
        }
        d
    }

    pub fn parse(data: Bytes<'a>) -> Option<Coverage<'a>> {
        let format: u16 = data.read(Offset::ZERO)?;
        let count: u16 = data.read(Offset::of(2u32))?;
        match format {
            1 => Some(Coverage::List(
                data.array(Offset::of(4u32), Count::of(count))?,
            )),
            2 => Some(Coverage::Ranges(
                data.array(Offset::of(4u32), Count::of(count))?,
            )),
            _ => None,
        }
    }

    /// The coverage index of `g`, or `None` when it is not covered.
    pub fn index(&self, g: GlyphId) -> Option<Index> {
        let gid = u16::try_from(g.index()).ok()?;
        match self {
            Coverage::List(list) => {
                let i = list.partition_point(|x| x.cmp(&gid))?;
                (list.get(i)? == gid).then_some(i)
            }
            Coverage::Ranges(ranges) => {
                // The first range whose end is at or past the glyph.
                let i = ranges.partition_point(|r| r.end.index().cmp(&g.index()))?;
                let r = ranges.get(i)?;
                if g.index() < r.start.index() {
                    return None;
                }
                let within = Count::of(u32::try_from(g.index() - r.start.index()).ok()?);
                r.first_index.plus(within)
            }
        }
    }

    /// Every glyph the coverage names, in table order.
    pub fn glyphs(&self) -> impl Iterator<Item = GlyphId> + '_ {
        let (list, ranges) = match self {
            Coverage::List(a) => (Some(a), None),
            Coverage::Ranges(r) => (None, Some(r)),
        };
        let listed = list.into_iter().flat_map(|a| {
            (0..a.count().value())
                .filter_map(move |i| a.get(Index::of(i)).map(GlyphId::from_shaper))
        });
        let ranged = ranges.into_iter().flat_map(|r| {
            (0..r.count().value())
                .filter_map(move |i| r.get(Index::of(i)))
                .flat_map(|range| {
                    (range.start.index()..=range.end.index())
                        .filter_map(|g| u16::try_from(g).ok().map(GlyphId::from_shaper))
                })
        });
        listed.chain(ranged)
    }

    pub fn covers(&self, g: GlyphId) -> bool {
        self.index(g).is_some()
    }
}

/// A glyph class from a `ClassDef`; class 0 is every glyph not listed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Class(pub u16);

impl Class {
    pub const ZERO: Class = Class(0);
}

/// A class definition table.
#[derive(Clone, Copy)]
pub enum ClassDef<'a> {
    /// Format 1: classes for a run of glyphs starting at `first`.
    Run {
        first: GlyphId,
        classes: Array<'a, u16>,
    },
    /// Format 2: sorted glyph ranges each naming one class.
    Ranges(Array<'a, ClassRange>),
}

/// A class definition format 2 `ClassRangeRecord`.
#[derive(Clone, Copy)]
pub struct ClassRange {
    start: GlyphId,
    end: GlyphId,
    class: Class,
}

impl FromBe for ClassRange {
    const SIZE: Len = Len::of(6);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(ClassRange {
            start: glyph(b)?,
            end: glyph(b.get(2..)?)?,
            class: Class(u16::parse_be(b.get(4..)?)?),
        })
    }
}

impl<'a> ClassDef<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<ClassDef<'a>> {
        let format: u16 = data.read(Offset::ZERO)?;
        match format {
            1 => {
                let first: u16 = data.read(Offset::of(2u32))?;
                let count: u16 = data.read(Offset::of(4u32))?;
                Some(ClassDef::Run {
                    first: GlyphId::from_shaper(first),
                    classes: data.array(Offset::of(6u32), Count::of(count))?,
                })
            }
            2 => {
                let count: u16 = data.read(Offset::of(2u32))?;
                Some(ClassDef::Ranges(
                    data.array(Offset::of(4u32), Count::of(count))?,
                ))
            }
            _ => None,
        }
    }

    /// Every glyph of `class`. Class zero is every glyph the table does
    /// not name, which no rule can enumerate, so it answers nothing for it.
    pub fn glyphs_of(&self, class: Class) -> impl Iterator<Item = GlyphId> + '_ {
        let (run, ranges) = match self {
            ClassDef::Run { first, classes } => (Some((*first, classes)), None),
            ClassDef::Ranges(r) => (None, Some(r)),
        };
        let in_run = run.into_iter().flat_map(move |(first, classes)| {
            (0..classes.count().value()).filter_map(move |i| {
                let c = classes.get(Index::of(i))?;
                (class.0 != 0 && c == class.0)
                    .then(|| first.index().checked_add(usize::try_from(i).ok()?))
                    .flatten()
                    .and_then(|g| u16::try_from(g).ok())
                    .map(GlyphId::from_shaper)
            })
        });
        let in_ranges = ranges.into_iter().flat_map(move |r| {
            (0..r.count().value())
                .filter_map(move |i| r.get(Index::of(i)))
                .filter(move |cr| class.0 != 0 && cr.class.0 == class.0)
                .flat_map(|cr| {
                    (cr.start.index()..=cr.end.index())
                        .filter_map(|g| u16::try_from(g).ok().map(GlyphId::from_shaper))
                })
        });
        in_run.chain(in_ranges)
    }

    pub fn class(&self, g: GlyphId) -> Class {
        match self {
            ClassDef::Run { first, classes } => match g.index().checked_sub(first.index()) {
                Some(i) => u32::try_from(i)
                    .ok()
                    .and_then(|i| classes.get(Index::of(i)))
                    .map_or(Class::ZERO, Class),
                None => Class::ZERO,
            },
            ClassDef::Ranges(ranges) => {
                let Some(i) = ranges.partition_point(|r| r.end.index().cmp(&g.index())) else {
                    return Class::ZERO;
                };
                match ranges.get(i) {
                    Some(r) if g.index() >= r.start.index() => r.class,
                    _ => Class::ZERO,
                }
            }
        }
    }
}

/// `GDEF`'s glyph class: what a glyph is for the purpose of skipping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphClass {
    /// Class 0: unclassified, never skipped by a class flag.
    Other,
    Base,
    Ligature,
    Mark,
    Component,
}

/// The `GDEF` table: glyph classes, mark attachment classes and mark glyph
/// sets. Absent tables answer as if every glyph were unclassified, which is
/// what the specification says of a glyph a `GDEF` does not list.
#[derive(Clone, Copy, Default)]
pub struct Gdef<'a> {
    classes: Option<ClassDef<'a>>,
    mark_attach: Option<ClassDef<'a>>,
    mark_sets: Option<(Bytes<'a>, Array<'a, u32>)>,
    /// Version 1.3's item variation store, which a device table indexes:
    /// `None` when the table is older, the offset 0, or the store
    /// malformed -- every delta is then 0, the per-subtable rule this
    /// reader keeps for every part of `GDEF` (note Z step 4). HarfBuzz
    /// refuses the whole table on a malformed store
    /// (`[[source: harfbuzz-12-3-0-gdef]]`).
    store: Option<crate::ivs::Store<'a>>,
}

impl<'a> Gdef<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Gdef<'a>> {
        let major: u16 = data.read(Offset::ZERO)?;
        let minor: u16 = data.read(Offset::of(2u32))?;
        if major != 1 {
            return None;
        }
        let sub = |at: u32| -> Option<Bytes<'a>> {
            let off: u16 = data.read(Offset::of(at))?;
            if off == 0 {
                None
            } else {
                data.tail(Offset::of(u32::from(off)))
            }
        };
        let classes = sub(4).and_then(ClassDef::parse);
        let mark_attach = sub(10).and_then(ClassDef::parse);
        let mark_sets = if minor >= 2 {
            sub(12).and_then(|t| {
                let format: u16 = t.read(Offset::ZERO)?;
                if format != 1 {
                    return None;
                }
                let count: u16 = t.read(Offset::of(2u32))?;
                Some((t, t.array(Offset::of(4u32), Count::of(count))?))
            })
        } else {
            None
        };
        let store = if minor >= 3 {
            // A header too short for the offset is no store, not no GDEF.
            data.read::<u32>(Offset::of(14u32))
                .filter(|off| *off != 0)
                .and_then(|off| data.tail(Offset::of(off)))
                .and_then(crate::ivs::Store::parse)
        } else {
            None
        };
        Some(Gdef {
            classes,
            mark_attach,
            mark_sets,
            store,
        })
    }

    /// The item variation store, for a device table's delta.
    pub(crate) fn store(&self) -> Option<crate::ivs::Store<'a>> {
        self.store
    }

    pub fn class(&self, g: GlyphId) -> GlyphClass {
        match self.classes.map(|c| c.class(g)) {
            Some(Class(1)) => GlyphClass::Base,
            Some(Class(2)) => GlyphClass::Ligature,
            Some(Class(3)) => GlyphClass::Mark,
            Some(Class(4)) => GlyphClass::Component,
            _ => GlyphClass::Other,
        }
    }

    pub fn is_mark(&self, g: GlyphId) -> bool {
        self.class(g) == GlyphClass::Mark
    }

    /// Whether the table carries a glyph class definition at all. A face
    /// without one has its marks told apart by Unicode instead, which is
    /// what HarfBuzz synthesises in its place.
    pub fn has_classes(&self) -> bool {
        self.classes.is_some()
    }

    fn mark_attach_class(&self, g: GlyphId) -> Class {
        self.mark_attach.map_or(Class::ZERO, |c| c.class(g))
    }

    fn in_mark_set(&self, set: Index, g: GlyphId) -> bool {
        let Some((table, sets)) = self.mark_sets else {
            return false;
        };
        let Some(off) = sets.get(set) else {
            return false;
        };
        table
            .tail(Offset::of(off))
            .and_then(Coverage::parse)
            .is_some_and(|c| c.covers(g))
    }
}

/// What a lookup does with a glyph it is not looking at: the answer of
/// its lookup flags against `GDEF`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Skip {
    /// The glyph takes part in matching.
    No,
    /// The glyph is invisible to this lookup.
    Yes,
}

/// A lookup's `lookupFlag` word.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LookupFlags(u16);

impl LookupFlags {
    const IGNORE_BASE: u16 = 0x0002;
    const IGNORE_LIGATURES: u16 = 0x0004;
    const IGNORE_MARKS: u16 = 0x0008;
    const USE_MARK_FILTERING_SET: u16 = 0x0010;
    const MARK_ATTACHMENT_CLASS: u16 = 0xFF00;

    fn has(self, bit: u16) -> bool {
        self.0 & bit != 0
    }

    /// The mark attachment class the high byte names, if any.
    fn attachment_class(self) -> Option<Class> {
        let want = self.0 & LookupFlags::MARK_ATTACHMENT_CLASS;
        (want != 0).then_some(Class(want >> 8))
    }
}

/// A lookup's skipping rule, built from its flags once.
#[derive(Clone, Copy)]
pub struct Filter<'a> {
    gdef: Gdef<'a>,
    flags: LookupFlags,
    mark_set: Option<Index>,
    /// The syllabic shaper's stage this lookup runs in, if it runs in one
    /// ([`Stage`]); none for every other run.
    pub(crate) stage: Option<Stage>,
}

/// **How a lookup matches in a syllabic shaper's stage**: the masks its
/// features name, whether it matches within one syllable, and whether the
/// joiners are skipped. HarfBuzz's matcher in the same terms
/// (`[[source: harfbuzz-14-5-1-gsubgpos]]`, `matcher_t`): a glyph of the
/// input sequence must carry one of the masks, every glyph matched must be
/// in the first one's syllable, and ZWNJ is never skipped in the input nor
/// ZWJ where the joiners are manual, while the context skips ZWJ always and
/// ZWNJ where they are not. The Indic features are all manual: "the
/// presence of ZWJ will prevent GSUB substitution lookups from matching the
/// input glyph sequence" (`[[source: ms-devanagari-script-development]]`,
/// Effect of ZWJ, ZWNJ and NBSP on Consonant Shaping).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Stage {
    pub(crate) masks: crate::syllabic::Masks,
    pub(crate) syllables: PerSyllable,
    pub(crate) joiners: Joiners,
}

/// Whether a lookup matches within its first glyph's syllable only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum PerSyllable {
    Yes,
    No,
}

/// Whether ZWJ and ZWNJ are skipped where a pattern does not name them, as
/// a default-ignorable is (`Auto`), or must be named (`Manual`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Joiners {
    Auto,
    Manual,
}

impl Filter<'_> {
    /// The rule applied to a glyph whose class the buffer already holds;
    /// only a mark's set and attachment class are looked up here.
    pub fn skip_class(&self, class: GlyphClass, g: GlyphId) -> Skip {
        let flagged = match class {
            GlyphClass::Base => self.flags.has(LookupFlags::IGNORE_BASE),
            GlyphClass::Ligature => self.flags.has(LookupFlags::IGNORE_LIGATURES),
            GlyphClass::Mark => {
                if self.flags.has(LookupFlags::IGNORE_MARKS) {
                    true
                } else if let Some(set) = self.mark_set {
                    !self.gdef.in_mark_set(set, g)
                } else {
                    match self.flags.attachment_class() {
                        Some(want) => self.gdef.mark_attach_class(g) != want,
                        None => false,
                    }
                }
            }
            GlyphClass::Other | GlyphClass::Component => false,
        };
        if flagged { Skip::Yes } else { Skip::No }
    }
}

/// A lookup type, as `GSUB` and `GPOS` number them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Kind(u16);

impl Kind {
    pub const SINGLE: Kind = Kind(1);
    pub const MULTIPLE: Kind = Kind(2);
    pub const ALTERNATE: Kind = Kind(3);
    pub const CONTEXT: Kind = Kind(5);
    pub const REVERSE: Kind = Kind(8);
    pub const LIGATURE: Kind = Kind(4);
    pub const CHAINED: Kind = Kind(6);
    pub const GSUB_EXTENSION: Kind = Kind(7);
    pub const MARK_TO_BASE: Kind = Kind(4);
    pub const MARK_TO_LIGATURE: Kind = Kind(5);
    pub const MARK_TO_MARK: Kind = Kind(6);
    pub const GPOS_EXTENSION: Kind = Kind(9);
    pub const PAIR: Kind = Kind(2);
    pub const CHAINED_POS: Kind = Kind(8);
}

/// One lookup: its type, its filter, and its subtables.
#[derive(Clone, Copy)]
pub struct Lookup<'a> {
    pub kind: Kind,
    pub filter: Filter<'a>,
    table: Bytes<'a>,
    subtables: Array<'a, u16>,
}

impl<'a> Lookup<'a> {
    /// This lookup run in a syllabic shaper's `stage`.
    pub(crate) fn staged(mut self, stage: Option<Stage>) -> Lookup<'a> {
        self.filter.stage = stage;
        self
    }

    pub fn subtable_count(&self) -> Count {
        self.subtables.count()
    }

    /// Subtable `i`, with extension subtables (`ext_kind`: 7 in `GSUB`, 9
    /// in `GPOS`) resolved to the subtable they point at and its type.
    pub fn subtable(&self, i: Index, ext_kind: Kind) -> Option<(Kind, Bytes<'a>)> {
        let off = self.subtables.get(i)?;
        let sub = self.table.tail(Offset::of(u32::from(off)))?;
        if self.kind != ext_kind {
            return Some((self.kind, sub));
        }
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let kind = Kind(sub.read(Offset::of(2u32))?);
        let ext: u32 = sub.read(Offset::of(4u32))?;
        if kind == ext_kind {
            return None;
        }
        Some((kind, sub.tail(Offset::of(ext))?))
    }
}

/// The three lists a `GSUB` or `GPOS` header points at, over its bytes.
#[derive(Clone, Copy)]
pub struct Layout<'a> {
    scripts: Bytes<'a>,
    features: Bytes<'a>,
    lookups: Bytes<'a>,
    gdef: Gdef<'a>,
}

/// Which features to apply: the fixed set a grid wants, each on or off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Features {
    pub ccmp: Enabled,
    pub calt: Enabled,
    pub liga: Enabled,
    /// The joining forms, `isol`, `fina`, `fin2`, `fin3`, `medi`, `med2`
    /// and `init`, each applied only to a glyph the caller gave that form;
    /// the required ligatures, `rlig`; and the required contextual
    /// alternates, `rclt`.
    pub joining: Enabled,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Enabled {
    On,
    Off,
}

impl Features {
    /// `ccmp` and `calt` on, `liga` off: composition and programming
    /// ligatures without prose ligatures, which a grid does not want (D8).
    pub const GRID: Features = Features {
        ccmp: Enabled::On,
        calt: Enabled::On,
        liga: Enabled::Off,
        joining: Enabled::Off,
    };

    /// [`Features::GRID`] with the joining features on: what a renderer
    /// shapes with, so a letter given a joining form is drawn in it
    /// (`[[source: unicode-ch9]]` 9.2.2, "a display process must select an
    /// appropriate glyph to depict each Arabic letter according to its
    /// immediate joining context") and the lam-alef ligature forms (9.2.3).
    pub const JOINING: Features = Features {
        joining: Enabled::On,
        ..Features::GRID
    };

    /// These features with the joining ones off.
    pub fn without_joining(self) -> Features {
        Features {
            joining: Enabled::Off,
            ..self
        }
    }

    #[inline]
    pub fn wants(self, tag: FeatureTag) -> bool {
        self.wants_but_joining(tag) || (self.joining == Enabled::On && tag.is_joining())
    }

    /// [`Features::wants`] for every feature but the joining ones, which it
    /// answers no for: the test a run with no joining letter asks of each
    /// feature its language system names, as it did before joining existed.
    #[inline]
    pub fn wants_but_joining(self, tag: FeatureTag) -> bool {
        match tag {
            FeatureTag::CCMP => self.ccmp == Enabled::On,
            FeatureTag::CALT => self.calt == Enabled::On,
            FeatureTag::LIGA => self.liga == Enabled::On,
            FeatureTag::MARK | FeatureTag::MKMK => true,
            _ => false,
        }
    }
}

/// Whether a language system's required feature is added to the lookups a
/// set of features names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Required {
    Added,
    Left,
}

/// The highest lookup index a feature may name: a bitset of this many
/// bits is 256 bytes, and Fira Code's `calt` alone names lookups past 280.
pub const MAX_LOOKUPS: usize = 2048;

/// The lookup indices a feature set names, as a bitset so they come out in
/// `LookupList` order whatever feature named them. Two sets that are equal
/// shape alike, which is what lets a renderer keep one run across two
/// scripts that select the same lookups.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LookupSet {
    bits: [u64; MAX_LOOKUPS / 64],
    /// Lookups past [`MAX_LOOKUPS`] a feature named and this set could not
    /// hold; counted rather than dropped in silence.
    declined: Count,
}

impl LookupSet {
    const EMPTY: LookupSet = LookupSet {
        bits: [0; MAX_LOOKUPS / 64],
        declined: Count::ZERO,
    };

    /// The lookups in the set, in `LookupList` order: each word's set bits,
    /// lowest first, a word with none passed over in one test -- a run's
    /// joining stages are each a set, most of them empty or nearly.
    pub fn iter(&self) -> impl Iterator<Item = Index> + '_ {
        self.bits.iter().enumerate().flat_map(|(w, &word)| {
            let base = u32::try_from(w * 64).unwrap_or(0);
            let mut left = word;
            core::iter::from_fn(move || {
                let b = (left != 0).then(|| left.trailing_zeros())?;
                left &= left - 1;
                Some(Index::of(base + b))
            })
        })
    }

    pub fn declined(&self) -> Count {
        self.declined
    }

    /// The set naming no lookup.
    pub(crate) fn empty() -> LookupSet {
        LookupSet::EMPTY
    }

    /// Whether lookup `i` is in the set.
    pub(crate) fn contains(&self, i: Index) -> bool {
        let i = i.slot();
        self.bits
            .get(i / 64)
            .is_some_and(|w| w & (1u64 << (i % 64)) != 0)
    }

    /// The lookups either set names, and both sets' declined.
    pub(crate) fn union(&self, o: &LookupSet) -> LookupSet {
        let mut out = self.clone();
        for (a, b) in out.bits.iter_mut().zip(o.bits.iter()) {
            *a |= *b;
        }
        out.declined = Count::of(self.declined.value().saturating_add(o.declined.value()));
        out
    }

    fn insert(&mut self, i: Index) {
        let i = i.slot();
        match self.bits.get_mut(i / 64) {
            Some(word) => *word |= 1u64 << (i % 64),
            None => self.declined = Count::of(self.declined.value().saturating_add(1)),
        }
    }
}

impl<'a> Layout<'a> {
    pub fn parse(data: Bytes<'a>, gdef: Gdef<'a>) -> Option<Layout<'a>> {
        let major: u16 = data.read(Offset::ZERO)?;
        if major != 1 {
            return None;
        }
        let scripts: u16 = data.read(Offset::of(4u32))?;
        let features: u16 = data.read(Offset::of(6u32))?;
        let lookups: u16 = data.read(Offset::of(8u32))?;
        Some(Layout {
            scripts: data.tail(Offset::of(u32::from(scripts)))?,
            features: data.tail(Offset::of(u32::from(features)))?,
            lookups: data.tail(Offset::of(u32::from(lookups)))?,
            gdef,
        })
    }

    pub fn gdef(&self) -> Gdef<'a> {
        self.gdef
    }

    fn records(table: Bytes<'a>) -> Option<Array<'a, TagRecord>> {
        let count: u16 = table.read(Offset::ZERO)?;
        table.array(Offset::of(2u32), Count::of(count))
    }

    fn find(records: &Array<'a, TagRecord>, tag: [u8; 4]) -> Option<Offset> {
        // Records are sorted by tag, so a binary search answers.
        let i = records.partition_point(|r| r.tag.0.cmp(&tag))?;
        let r = records.get(i)?;
        (r.tag.0 == tag).then_some(r.offset)
    }

    /// The tag of the Script table text in `script` is shaped with: the
    /// script's own, else `DFLT`, else `latn`; `None` when the face has
    /// none of them.
    pub fn chosen(&self, script: Script) -> Option<ScriptTag> {
        self.record(script).map(|(tag, _)| tag)
    }

    /// The Script table [`Layout::chosen`] names, with its offset: one
    /// binary search per tag tried.
    fn record(&self, script: Script) -> Option<(ScriptTag, Offset)> {
        let scripts = Layout::records(self.scripts)?;
        script
            .tags()
            .chain([ScriptTag::DFLT, ScriptTag::LATN])
            .find_map(|t| Layout::find(&scripts, t.0).map(|off| (t, off)))
    }

    /// The default language system of the Script table [`Layout::chosen`]
    /// names.
    fn lang_sys(&self, script: Script) -> Option<Bytes<'a>> {
        let (_, off) = self.record(script)?;
        self.default_lang_sys(off)
    }

    /// The default language system of the Script table at `script_off`.
    fn default_lang_sys(&self, script_off: Offset) -> Option<Bytes<'a>> {
        let script = self.scripts.tail(script_off)?;
        let default: u16 = script.read(Offset::ZERO)?;
        if default == 0 {
            return None;
        }
        script.tail(Offset::of(u32::from(default)))
    }

    /// The lookups the selected features name for text in `script`, sorted.
    #[inline]
    pub fn lookups_for(&self, script: Script, wanted: Features) -> LookupSet {
        match wanted.joining {
            Enabled::Off => {
                self.lookups_where(script, Required::Added, |t| wanted.wants_but_joining(t))
            }
            Enabled::On => self.lookups_where(script, Required::Added, |t| wanted.wants(t)),
        }
    }

    /// The lookups the features `wanted` accepts name for text in `script`,
    /// sorted, with the language system's required feature added or not.
    pub fn lookups_where(
        &self,
        script: Script,
        required: Required,
        wanted: impl Fn(FeatureTag) -> bool,
    ) -> LookupSet {
        let mut set = LookupSet::EMPTY;
        if let Some(lang) = self.lang_sys(script) {
            self.add_lang_sys(lang, required, wanted, &mut set);
        }
        set
    }

    /// The lookups the selected features name for any script: the union over
    /// every Script table's default language system. What a lookup's digest,
    /// a pair prefilter or a context width must cover, since each is a
    /// property of a lookup and holds whatever script selected it.
    pub fn lookups_any(&self, wanted: Features) -> LookupSet {
        self.lookups_any_where(|t| wanted.wants(t))
    }

    /// [`Layout::lookups_any`] for the features `wanted` accepts.
    pub(crate) fn lookups_any_where(&self, wanted: impl Fn(FeatureTag) -> bool) -> LookupSet {
        let mut set = LookupSet::EMPTY;
        let Some(scripts) = Layout::records(self.scripts) else {
            return set;
        };
        let mut k = Index::ZERO;
        while let Some(r) = scripts.get(k) {
            if let Some(lang) = self.default_lang_sys(r.offset) {
                self.add_lang_sys(lang, Required::Added, &wanted, &mut set);
            }
            k = k.next();
        }
        set
    }

    /// Whether the language system text in `script` selects names the
    /// feature `tag`, among its features or as its required one.
    pub fn names(&self, script: Script, tag: FeatureTag) -> bool {
        let (Some(lang), Some(features)) = (self.lang_sys(script), Layout::records(self.features))
        else {
            return false;
        };
        let is = |fi: u16| features.get(Index::of(fi)).is_some_and(|r| r.tag == tag);
        let required: u16 = lang.read(Offset::of(2u32)).unwrap_or(0xFFFF);
        if required != 0xFFFF && is(required) {
            return true;
        }
        let count: u16 = lang.read(Offset::of(4u32)).unwrap_or(0);
        let Some(indices) = lang.array::<u16>(Offset::of(6u32), Count::of(count)) else {
            return false;
        };
        let mut k = Index::ZERO;
        while let Some(fi) = indices.get(k) {
            if is(fi) {
                return true;
            }
            k = k.next();
        }
        false
    }

    /// Adds what one language system's selected features name to `set`.
    fn add_lang_sys(
        &self,
        lang: Bytes<'a>,
        with_required: Required,
        wanted: impl Fn(FeatureTag) -> bool,
        set: &mut LookupSet,
    ) {
        let Some(features) = Layout::records(self.features) else {
            return;
        };
        let required: u16 = lang.read(Offset::of(2u32)).unwrap_or(0xFFFF);
        let count: u16 = lang.read(Offset::of(4u32)).unwrap_or(0);
        let Some(indices) = lang.array::<u16>(Offset::of(6u32), Count::of(count)) else {
            return;
        };
        let mut add_feature = |fi: u16| {
            let Some(rec) = features.get(Index::of(fi)) else {
                return;
            };
            let named = fi == required && with_required == Required::Added;
            if !named && !wanted(rec.tag) {
                return;
            }
            let Some(feature) = self.features.tail(rec.offset) else {
                return;
            };
            let n: u16 = feature.read(Offset::of(2u32)).unwrap_or(0);
            if let Some(lookups) = feature.array::<u16>(Offset::of(4u32), Count::of(n)) {
                let mut k = Index::ZERO;
                while let Some(l) = lookups.get(k) {
                    set.insert(Index::of(l));
                    k = k.next();
                }
            }
        };
        if required != 0xFFFF && with_required == Required::Added {
            add_feature(required);
        }
        let mut k = Index::ZERO;
        while let Some(fi) = indices.get(k) {
            add_feature(fi);
            k = k.next();
        }
    }

    /// Lookup `i` from the `LookupList`.
    pub fn lookup(&self, i: Index) -> Option<Lookup<'a>> {
        let count: u16 = self.lookups.read(Offset::ZERO)?;
        let offsets: Array<'a, u16> = self.lookups.array(Offset::of(2u32), Count::of(count))?;
        let table = self.lookups.tail(Offset::of(u32::from(offsets.get(i)?)))?;
        let kind = Kind(table.read(Offset::ZERO)?);
        let flags = LookupFlags(table.read(Offset::of(2u32))?);
        let n: u16 = table.read(Offset::of(4u32))?;
        let subtables = table.array(Offset::of(6u32), Count::of(n))?;
        let mark_set = if flags.has(LookupFlags::USE_MARK_FILTERING_SET) {
            let at = Offset::of(6u32).plus(Len::of(2).times(Count::of(n))?)?;
            Some(Index::of(table.read::<u16>(at)?))
        } else {
            None
        };
        Some(Lookup {
            kind,
            filter: Filter {
                gdef: self.gdef,
                flags,
                mark_set,
                stage: None,
            },
            table,
            subtables,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A GDEF of `minor` version whose glyph class definition (format 1,
    /// glyph 3 is a mark) sits at 18 and whose item variation store, when
    /// version 1.3 names one, is `store_at` from the table's start.
    fn gdef(version: [u8; 4], store_at: [u8; 4]) -> [u8; 60] {
        let mut t = [0u8; 60];
        t[..4].copy_from_slice(&version);
        t[4..6].copy_from_slice(&18u16.to_be_bytes());
        t[14..18].copy_from_slice(&store_at);
        // ClassDef format 1: start glyph 3, one class, class 3 (mark).
        t[18..26].copy_from_slice(&[0, 1, 0, 3, 0, 1, 0, 3]);
        // The store at 26: one empty data set, no regions.
        t[26..34].copy_from_slice(&[0, 1, 0, 0, 0, 0, 0, 1]);
        t[34..38].copy_from_slice(&12u32.to_be_bytes());
        t[38..44].copy_from_slice(&[0, 0, 0, 0, 0, 0]);
        t
    }

    const V1_2: [u8; 4] = [0, 1, 0, 2];
    const V1_3: [u8; 4] = [0, 1, 0, 3];

    fn mark(g: &Gdef<'_>) -> bool {
        g.is_mark(GlyphId::from_shaper(3))
    }

    /// Version 1.3 names a store, and a version below it cannot: the same
    /// bytes at 14 read as no store under 1.2.
    #[test]
    fn a_store_is_read_from_version_one_point_three() {
        let t = gdef(V1_3, 26u32.to_be_bytes());
        let g = Gdef::parse(Bytes::new(&t)).unwrap();
        assert!(g.store().is_some() && mark(&g));
        let t = gdef(V1_2, 26u32.to_be_bytes());
        let g = Gdef::parse(Bytes::new(&t)).unwrap();
        assert!(g.store().is_none() && mark(&g));
        let t = gdef(V1_3, 0u32.to_be_bytes());
        assert!(Gdef::parse(Bytes::new(&t)).unwrap().store().is_none());
    }

    /// A malformed store is no store and the rest of the table stands --
    /// the per-subtable rule this reader keeps, where HarfBuzz refuses the
    /// whole GDEF -- and so does a 1.3 header too short to hold the
    /// offset.
    #[test]
    fn a_malformed_store_leaves_the_classes() {
        let mut t = gdef(V1_3, 26u32.to_be_bytes());
        t[26..28].copy_from_slice(&[0, 2]);
        let g = Gdef::parse(Bytes::new(&t)).unwrap();
        assert!(g.store().is_none() && mark(&g), "format 2");
        let t = gdef(V1_3, 0xFFFF0u32.to_be_bytes());
        let g = Gdef::parse(Bytes::new(&t)).unwrap();
        assert!(g.store().is_none() && mark(&g), "past the table");
        let mut t = gdef(V1_3, 26u32.to_be_bytes());
        t[4..6].copy_from_slice(&0u16.to_be_bytes());
        let g = Gdef::parse(Bytes::new(&t[..16])).unwrap();
        assert!(g.store().is_none(), "a header cut at 16");
    }
}
