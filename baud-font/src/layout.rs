//! The OpenType Layout common tables (`[[source: ms-opentype-chapter2]]`)
//! and `GDEF` (`[[source: ms-opentype-gdef]]`): what `GSUB` and `GPOS`
//! share.
//!
//! **Scripts, features, lookups.** A layout table names its features under
//! a script and a language system, each feature lists lookups, and the
//! lookups are applied in `LookupList` order over the union of what the
//! selected features name. [`Layout::lookups_for`] does that selection --
//! script `latn` with its default language system, or `DFLT` -- and yields
//! the sorted, deduplicated lookup indices. A terminal shapes one script's
//! worth of Latin, symbols and emoji, so that is the whole of the script
//! machinery it needs.
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
use baud_domain::GlyphId;

/// A four-byte feature or script tag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeatureTag(pub [u8; 4]);

impl FeatureTag {
    pub const CCMP: FeatureTag = FeatureTag(*b"ccmp");
    pub const CALT: FeatureTag = FeatureTag(*b"calt");
    pub const LIGA: FeatureTag = FeatureTag(*b"liga");
    pub const MARK: FeatureTag = FeatureTag(*b"mark");
    pub const MKMK: FeatureTag = FeatureTag(*b"mkmk");
    const LATN: FeatureTag = FeatureTag(*b"latn");
    const DFLT: FeatureTag = FeatureTag(*b"DFLT");
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
        Some(Gdef {
            classes,
            mark_attach,
            mark_sets,
        })
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
}

impl Filter<'_> {
    pub fn skip(&self, g: GlyphId) -> Skip {
        let class = self.gdef.class(g);
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
    pub const LIGATURE: Kind = Kind(4);
    pub const CHAINED: Kind = Kind(6);
    pub const GSUB_EXTENSION: Kind = Kind(7);
    pub const MARK_TO_BASE: Kind = Kind(4);
    pub const MARK_TO_MARK: Kind = Kind(6);
    pub const GPOS_EXTENSION: Kind = Kind(9);
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
    };

    fn wants(self, tag: FeatureTag) -> bool {
        match tag {
            FeatureTag::CCMP => self.ccmp == Enabled::On,
            FeatureTag::CALT => self.calt == Enabled::On,
            FeatureTag::LIGA => self.liga == Enabled::On,
            FeatureTag::MARK | FeatureTag::MKMK => true,
            _ => false,
        }
    }
}

/// The highest lookup index a feature may name: a bitset of this many
/// bits is 256 bytes, and Fira Code's `calt` alone names lookups past 280.
pub const MAX_LOOKUPS: usize = 2048;

/// The lookup indices a feature set names, as a bitset so they come out in
/// `LookupList` order whatever feature named them.
pub struct LookupSet {
    bits: [u64; MAX_LOOKUPS / 64],
    /// Lookups past [`MAX_LOOKUPS`] a feature named and this set could not
    /// hold; counted rather than dropped in silence.
    declined: Count,
}

impl LookupSet {
    pub fn iter(&self) -> impl Iterator<Item = Index> + '_ {
        self.bits.iter().enumerate().flat_map(|(w, &word)| {
            (0..64u32)
                .filter(move |b| word & (1u64 << b) != 0)
                .map(move |b| Index::of(u32::try_from(w * 64).unwrap_or(0) + b))
        })
    }

    pub fn declined(&self) -> Count {
        self.declined
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

    fn find(records: &Array<'a, TagRecord>, tag: FeatureTag) -> Option<Offset> {
        // Records are sorted by tag, so a binary search answers.
        let i = records.partition_point(|r| r.tag.0.cmp(&tag.0))?;
        let r = records.get(i)?;
        (r.tag == tag).then_some(r.offset)
    }

    /// The default language system of `latn`, else of `DFLT`.
    fn lang_sys(&self) -> Option<Bytes<'a>> {
        let scripts = Layout::records(self.scripts)?;
        let script_off = Layout::find(&scripts, FeatureTag::LATN)
            .or_else(|| Layout::find(&scripts, FeatureTag::DFLT))?;
        let script = self.scripts.tail(script_off)?;
        let default: u16 = script.read(Offset::ZERO)?;
        if default == 0 {
            return None;
        }
        script.tail(Offset::of(u32::from(default)))
    }

    /// The lookups the selected features name for the script, sorted.
    pub fn lookups_for(&self, wanted: Features) -> LookupSet {
        let mut set = LookupSet {
            bits: [0; MAX_LOOKUPS / 64],
            declined: Count::ZERO,
        };
        let Some(lang) = self.lang_sys() else {
            return set;
        };
        let Some(features) = Layout::records(self.features) else {
            return set;
        };
        let required: u16 = lang.read(Offset::of(2u32)).unwrap_or(0xFFFF);
        let count: u16 = lang.read(Offset::of(4u32)).unwrap_or(0);
        let Some(indices) = lang.array::<u16>(Offset::of(6u32), Count::of(count)) else {
            return set;
        };
        let mut add_feature = |fi: u16| {
            let Some(rec) = features.get(Index::of(fi)) else {
                return;
            };
            if fi != required && !wanted.wants(rec.tag) {
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
        if required != 0xFFFF {
            add_feature(required);
        }
        let mut k = Index::ZERO;
        while let Some(fi) = indices.get(k) {
            add_feature(fi);
            k = k.next();
        }
        set
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
            },
            table,
            subtables,
        })
    }
}
