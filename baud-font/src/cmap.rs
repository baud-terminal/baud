//! `cmap`: character to glyph (`[[source: ms-opentype-cmap]]`).
//!
//! **Which subtable answers.** The table is a list of encoding records, each
//! naming a platform, an encoding and a subtable. A face may carry several
//! Unicode subtables -- a BMP-only format 4 beside a full-repertoire format
//! 12 -- and this crate does what `ttf-parser` does so the differential can
//! hold it to the same answer: walk the records in order, skip any that is
//! not a Unicode encoding, and take the first subtable that maps the
//! character. Format 14 is consulted only for variation sequences.
//!
//! **Which formats.** 0, 4, 6, 12 and 13 for characters, 14 for variation
//! selectors. The specification says formats 4 and 12 are what new fonts
//! use and "application developers should anticipate that any of the formats
//! may be used"; 0 and 6 are a few lines each and a system font from 2005
//! still carries them, while format 2 (mixed 8/16-bit, for legacy CJK
//! encodings) and the platform-specific ones are not Unicode and are skipped
//! by the platform test before their format is ever read.
//!
//! **Every offset in here is hostile until checked.** The format 4
//! `idRangeOffset` trick addresses memory *relative to its own array slot*,
//! which is how a malformed font walks a parser out of the table; every such
//! read goes through [`Bytes`], whose end is the end of the `cmap` table.
//! Glyph 0 is never answered: it is `.notdef`, and a caller that gets `None`
//! can try the next face where one that got 0 would draw a box.

use crate::bytes::{Array, Bytes, Count, FromBe, Index, Len, Offset, U24};
use baud_domain::{Codepoint, GlyphId};
use core::cmp::Ordering;

/// What a format 14 subtable says about a base character followed by a
/// variation selector.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Variation {
    /// The sequence is a default one: draw the base character's own glyph.
    Default,
    /// The sequence selects this glyph instead.
    Glyph(GlyphId),
    /// The face says nothing about the pair.
    Absent,
}

/// An encoding record's platform id.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Platform(u16);

impl Platform {
    const UNICODE: Platform = Platform(0);
    const WINDOWS: Platform = Platform(3);
}

/// An encoding record's encoding id, whose meaning depends on the platform.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Encoding(u16);

impl Encoding {
    const WINDOWS_UNICODE_BMP: Encoding = Encoding(1);
    const WINDOWS_UNICODE_FULL: Encoding = Encoding(10);
}

/// A subtable format number.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Format(u16);

impl Format {
    const BYTE: Format = Format(0);
    const SEGMENTS: Format = Format(4);
    const TRIMMED: Format = Format(6);
    const GROUPS: Format = Format(12);
    const MANY_TO_ONE: Format = Format(13);
    const VARIATIONS: Format = Format(14);
}

/// A format 12/13 `SequentialMapGroup`: start, end, start glyph.
#[derive(Clone, Copy)]
struct Group(u32, u32, u32);

impl FromBe for Group {
    const SIZE: Len = Len::of(12);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Group(
            u32::parse_be(b)?,
            u32::parse_be(b.get(4..)?)?,
            u32::parse_be(b.get(8..)?)?,
        ))
    }
}

/// A format 14 `VariationSelector` record: selector, default UVS offset,
/// non-default UVS offset -- the offsets relative to the subtable, zero when
/// that half is absent.
#[derive(Clone, Copy)]
struct Selector(U24, u32, u32);

impl FromBe for Selector {
    const SIZE: Len = Len::of(11);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Selector(
            U24::parse_be(b)?,
            u32::parse_be(b.get(3..)?)?,
            u32::parse_be(b.get(7..)?)?,
        ))
    }
}

/// A default-UVS `UnicodeRange`: first codepoint and how many more follow.
#[derive(Clone, Copy)]
struct DefaultRange(U24, u8);

impl FromBe for DefaultRange {
    const SIZE: Len = Len::of(4);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(DefaultRange(U24::parse_be(b)?, u8::parse_be(b.get(3..)?)?))
    }
}

/// A non-default-UVS `UVSMapping`: codepoint and the glyph it selects.
#[derive(Clone, Copy)]
struct Mapping(U24, u16);

impl FromBe for Mapping {
    const SIZE: Len = Len::of(5);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Mapping(U24::parse_be(b)?, u16::parse_be(b.get(3..)?)?))
    }
}

const RECORD: Len = Len::of(8);

pub struct Cmap<'a> {
    data: Bytes<'a>,
    records: Count,
}

struct Subtable<'a> {
    platform: Platform,
    encoding: Encoding,
    format: Format,
    data: Bytes<'a>,
}

impl<'a> Cmap<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Cmap<'a>> {
        let n: u16 = data.read(Offset::of(2u32))?;
        Some(Cmap {
            data,
            records: Count::of(n),
        })
    }

    fn subtable(&self, i: Index) -> Option<Subtable<'a>> {
        let at = Offset::of(4u32).at(i, RECORD)?;
        let platform: u16 = self.data.read(at)?;
        let encoding: u16 = self.data.read(at.plus(Len::of(2))?)?;
        let off: u32 = self.data.read(at.plus(Len::of(4))?)?;
        // From the subtable offset to the end of the table, not to the
        // subtable's own declared length: a lying length must not extend a
        // read past the table, and a short one must not hide data every
        // other reader sees.
        let data = self.data.tail(Offset::of(off))?;
        let format: u16 = data.read(Offset::ZERO)?;
        Some(Subtable {
            platform: Platform(platform),
            encoding: Encoding(encoding),
            format: Format(format),
            data,
        })
    }

    pub fn glyph(&self, c: Codepoint) -> Option<GlyphId> {
        let mut i = Index::ZERO;
        while self.records.holds(i) {
            if let Some(sub) = self.subtable(i) {
                if sub.is_unicode() {
                    if let Some(g) = sub.glyph(c) {
                        return Some(g);
                    }
                }
            }
            i = i.next();
        }
        None
    }

    pub fn variation(&self, c: Codepoint, vs: Codepoint) -> Variation {
        let mut i = Index::ZERO;
        while self.records.holds(i) {
            if let Some(sub) = self.subtable(i) {
                if sub.format == Format::VARIATIONS {
                    if let Some(v) = sub.variation(c, vs) {
                        return v;
                    }
                }
            }
            i = i.next();
        }
        Variation::Absent
    }
}

fn glyph(n: u16) -> Option<GlyphId> {
    (n != 0).then(|| GlyphId::from_shaper(n))
}

impl<'a> Subtable<'a> {
    /// Whether the record's platform and encoding name Unicode codepoints --
    /// the rule the specification gives and `ttf-parser` implements: the
    /// Unicode platform always; Windows with the BMP encoding; Windows with
    /// the full-repertoire encoding only for a format 12 or 13 subtable,
    /// which is what that encoding id is defined to carry.
    fn is_unicode(&self) -> bool {
        match self.platform {
            Platform::UNICODE => true,
            Platform::WINDOWS => {
                self.encoding == Encoding::WINDOWS_UNICODE_BMP
                    || (self.encoding == Encoding::WINDOWS_UNICODE_FULL
                        && matches!(self.format, Format::GROUPS | Format::MANY_TO_ONE))
            }
            _ => false,
        }
    }

    fn glyph(&self, c: Codepoint) -> Option<GlyphId> {
        match self.format {
            Format::BYTE => self.format0(c),
            Format::SEGMENTS => self.format4(c),
            Format::TRIMMED => self.format6(c),
            Format::GROUPS => self.format12(c),
            Format::MANY_TO_ONE => self.format13(c),
            _ => None,
        }
    }

    /// Format 0: a 256-entry byte array for the first 256 codepoints.
    fn format0(&self, c: Codepoint) -> Option<GlyphId> {
        let i = Index::of(u8::try_from(c.value()).ok()?);
        let arr: Array<'a, u8> = self.data.array(Offset::of(6u32), Count::of(256u32))?;
        glyph(u16::from(arr.get(i)?))
    }

    /// Format 4: parallel arrays of segments over the BMP.
    fn format4(&self, c: Codepoint) -> Option<GlyphId> {
        let cp = u16::try_from(c.value()).ok()?;
        let seg_x2: u16 = self.data.read(Offset::of(6u32))?;
        let segs = Count::of(seg_x2 / 2);
        let ends: Array<'a, u16> = self.data.array(Offset::of(14u32), segs)?;
        // endCode[segCount], reservedPad, then three more arrays of segCount.
        let stride = Len::of(2).times(segs)?;
        let starts_at = Offset::of(16u32).plus(stride)?;
        let deltas_at = starts_at.plus(stride)?;
        let ranges_at = deltas_at.plus(stride)?;
        let starts: Array<'a, u16> = self.data.array(starts_at, segs)?;
        let deltas: Array<'a, u16> = self.data.array(deltas_at, segs)?;
        let ranges: Array<'a, u16> = self.data.array(ranges_at, segs)?;

        // "Search for the first endCode that is greater than or equal to the
        // character code you want to map."
        let i = ends.partition_point(|e| e.cmp(&cp))?;
        if !segs.holds(i) {
            return None;
        }
        let start = starts.get(i)?;
        if start > cp {
            return None;
        }
        let delta = deltas.get(i)?;
        let range = ranges.get(i)?;
        let raw = if range == 0 {
            cp.wrapping_add(delta)
        } else {
            // The offset is relative to the idRangeOffset entry itself, then
            // stepped by the character's distance into the segment. Bounded
            // by the table, never by trust.
            let entry = ranges_at.at(i, Len::of(2))?;
            let hop = Len::of(u32::from(range));
            let step = Len::of(2u32.checked_mul(u32::from(cp.checked_sub(start)?))?);
            let at = entry.plus(hop)?.plus(step)?;
            let g: u16 = self.data.read(at)?;
            if g == 0 {
                return None;
            }
            g.wrapping_add(delta)
        };
        glyph(raw)
    }

    /// Format 6: a dense array from `firstCode`.
    fn format6(&self, c: Codepoint) -> Option<GlyphId> {
        let cp = u16::try_from(c.value()).ok()?;
        let first: u16 = self.data.read(Offset::of(6u32))?;
        let count: u16 = self.data.read(Offset::of(8u32))?;
        let i = Index::of(cp.checked_sub(first)?);
        let arr: Array<'a, u16> = self.data.array(Offset::of(10u32), Count::of(count))?;
        glyph(arr.get(i)?)
    }

    /// The format 12/13 group holding `c`: the first group whose end is at or
    /// above it, if its start is at or below it.
    fn group(&self, c: Codepoint) -> Option<Group> {
        let cp = c.value();
        let n: u32 = self.data.read(Offset::of(12u32))?;
        let groups: Array<'a, Group> = self.data.array(Offset::of(16u32), Count::of(n))?;
        let i = groups.partition_point(|g| g.1.cmp(&cp))?;
        let g = groups.get(i)?;
        (g.0 <= cp && cp <= g.1).then_some(g)
    }

    /// Format 12: sorted groups of consecutive codepoints to consecutive
    /// glyphs, over the whole of Unicode.
    fn format12(&self, c: Codepoint) -> Option<GlyphId> {
        let g = self.group(c)?;
        let id = g.2.checked_add(c.value().checked_sub(g.0)?)?;
        glyph(u16::try_from(id).ok()?)
    }

    /// Format 13: the same groups, every codepoint in one mapping to the one
    /// glyph -- a last-resort font's shape.
    fn format13(&self, c: Codepoint) -> Option<GlyphId> {
        let g = self.group(c)?;
        glyph(u16::try_from(g.2).ok()?)
    }

    /// Format 14: a sorted list of variation selectors, each with a
    /// range-compressed default set and a sorted non-default mapping.
    fn variation(&self, c: Codepoint, vs: Codepoint) -> Option<Variation> {
        let n: u32 = self.data.read(Offset::of(6u32))?;
        let selectors: Array<'a, Selector> = self.data.array(Offset::of(10u32), Count::of(n))?;
        let sel = vs.value();
        let i = selectors.partition_point(|s| s.0.0.cmp(&sel))?;
        let s = selectors.get(i)?;
        if s.0.0 != sel {
            return None;
        }
        let cp = c.value();

        if s.1 != 0 {
            let base = Offset::of(s.1);
            let count: u32 = self.data.read(base)?;
            let ranges: Array<'a, DefaultRange> =
                self.data.array(base.plus(Len::of(4))?, Count::of(count))?;
            // The last range whose start is at or below the character: the
            // first with a greater start, stepped back one.
            let after = ranges.partition_point(|r| {
                if r.0.0 <= cp {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            })?;
            if let Some(last) = after.prev() {
                let r = ranges.get(last)?;
                if cp <= r.0.0.checked_add(u32::from(r.1))? {
                    return Some(Variation::Default);
                }
            }
        }

        if s.2 != 0 {
            let base = Offset::of(s.2);
            let count: u32 = self.data.read(base)?;
            let maps: Array<'a, Mapping> =
                self.data.array(base.plus(Len::of(4))?, Count::of(count))?;
            let i = maps.partition_point(|m| m.0.0.cmp(&cp))?;
            if let Some(m) = maps.get(i) {
                if m.0.0 == cp {
                    return glyph(m.1).map(Variation::Glyph);
                }
            }
        }
        None
    }
}
