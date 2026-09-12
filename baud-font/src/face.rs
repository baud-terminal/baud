//! One face: the table directory and the metric tables every face carries.
//!
//! **A collection is a list of directories, and a directory is a list of
//! tables** (`[[source: ms-opentype-otff]]`). `'ttcf'` at offset 0 introduces
//! `numFonts` directory offsets; otherwise the file is one directory, whose
//! `sfntVersion` is `0x00010000` for TrueType outlines, `'OTTO'` for CFF, and
//! -- Apple's manual allows it, and this crate accepts it as the reference
//! implementations do -- `'true'` or `'typ1'`.
//!
//! **What a face requires and what it tolerates.** `head`, `maxp` and `hhea`
//! must be present and well-formed, because nothing can be drawn without
//! `unitsPerEm`, `numGlyphs` and the line metrics; `hmtx`, `cmap`, `OS/2` and
//! `post` are read if present and answer `None` otherwise. A table record
//! whose `offset + length` lies outside the file is treated as absent, the
//! rule `ttf-parser` applies and the differential in `tests/oracle.rs` holds
//! this crate to.

use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::cmap::{Cmap, Variation};
use crate::{Error, Tag};
use baud_domain::{Codepoint, GlyphId};

/// Font design units, the coordinate system `unitsPerEm` scales.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FontUnits(i16);

impl FontUnits {
    pub const fn value(self) -> i16 {
        self.0
    }
}

/// The em size in design units, 16 to 16384 per `head`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitsPerEm(u16);

impl UnitsPerEm {
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// A horizontal advance in design units, always non-negative in `hmtx`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Advance(u16);

impl Advance {
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// How many glyphs the face carries, per `maxp`; every [`GlyphId`] the face
/// answers is below it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphCount(u16);

impl GlyphCount {
    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn holds(self, g: GlyphId) -> bool {
        g.index() < usize::from(self.0)
    }
}

/// How many faces a file carries: one for an sfnt, `numFonts` for a `'ttcf'`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceCount(u32);

impl FaceCount {
    pub const fn value(self) -> u32 {
        self.0
    }
}

/// Which face of a collection; `FaceIndex::FIRST` for a plain sfnt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FaceIndex(u32);

impl FaceIndex {
    pub const FIRST: FaceIndex = FaceIndex(0);

    pub const fn of(i: u32) -> FaceIndex {
        FaceIndex(i)
    }
}

/// `hhea`'s line metrics, the ones Apple platforms use; `OS/2`'s typographic
/// set is [`Face::typo_metrics`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineMetrics {
    pub ascender: FontUnits,
    pub descender: FontUnits,
    pub line_gap: FontUnits,
}

/// Where and how thick `post` says an underline is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Underline {
    pub position: FontUnits,
    pub thickness: FontUnits,
}

/// Whether `post` declares every advance equal. **Declared, not verified**:
/// a monospace font with one proportional glyph still says fixed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pitch {
    Fixed,
    Proportional,
}

/// One table record: where the table is and how long. Not yet checked
/// against the file; [`Face::table`] does that when it slices.
#[derive(Clone, Copy)]
struct Record {
    tag: Tag,
    at: Offset,
    len: Len,
}

const RECORD: Len = Len::of(16);
const DIRECTORY_HEADER: Len = Len::of(12);
const SFNT_TRUETYPE: u32 = 0x0001_0000;
const SFNT_CFF: u32 = 0x4F54_544F; // 'OTTO'
const SFNT_APPLE_TRUE: u32 = 0x7472_7565; // 'true'
const SFNT_APPLE_TYP1: u32 = 0x7479_7031; // 'typ1'
const TTCF: u32 = 0x7474_6366; // 'ttcf'

/// A parsed face: the directory plus the tables read at construction.
///
/// Construction reads and validates the required tables once; every accessor
/// afterwards is a bounded read or a stored value, so a `Face` that exists
/// answers every question without a further error path.
pub struct Face<'a> {
    data: Bytes<'a>,
    records: Bytes<'a>,
    tables: Count,
    upem: UnitsPerEm,
    glyphs: GlyphCount,
    line: LineMetrics,
    hmetrics: Count,
    hmtx: Option<Bytes<'a>>,
    os2: Option<Bytes<'a>>,
    post: Option<Bytes<'a>>,
    cmap: Option<Cmap<'a>>,
}

/// How many faces `data` holds, without parsing any of them.
pub fn faces(data: &[u8]) -> Result<FaceCount, Error> {
    let b = Bytes::new(data);
    let version: u32 = b.read(Offset::ZERO).ok_or(Error::NotAFont)?;
    if version == TTCF {
        let n: u32 = b.read(Offset::of(8u32)).ok_or(Error::NotAFont)?;
        Ok(FaceCount(n))
    } else if is_sfnt(version) {
        Ok(FaceCount(1))
    } else {
        Err(Error::NotAFont)
    }
}

fn is_sfnt(version: u32) -> bool {
    matches!(
        version,
        SFNT_TRUETYPE | SFNT_CFF | SFNT_APPLE_TRUE | SFNT_APPLE_TYP1
    )
}

impl<'a> Face<'a> {
    /// The first face of `data`, which is the only face of a plain sfnt.
    pub fn parse(data: &'a [u8]) -> Result<Face<'a>, Error> {
        Face::parse_index(data, FaceIndex::FIRST)
    }

    /// Face `index` of a collection, or the one face of an sfnt when `index`
    /// is `FIRST`.
    pub fn parse_index(data: &'a [u8], index: FaceIndex) -> Result<Face<'a>, Error> {
        let b = Bytes::new(data);
        let version: u32 = b.read(Offset::ZERO).ok_or(Error::NotAFont)?;
        let dir = if version == TTCF {
            let n: u32 = b.read(Offset::of(8u32)).ok_or(Error::NotAFont)?;
            if index.0 >= n {
                return Err(Error::FaceOutOfRange);
            }
            // tableDirectoryOffsets[numFonts] follows the 12-byte header.
            let at = Offset::of(12u32)
                .at(Index::of(index.0), Len::of(4))
                .ok_or(Error::NotAFont)?;
            let off: u32 = b.read(at).ok_or(Error::NotAFont)?;
            Offset::of(off)
        } else if is_sfnt(version) {
            if index != FaceIndex::FIRST {
                return Err(Error::FaceOutOfRange);
            }
            Offset::ZERO
        } else {
            return Err(Error::NotAFont);
        };
        Face::at_directory(b, dir)
    }

    fn at_directory(data: Bytes<'a>, dir: Offset) -> Result<Face<'a>, Error> {
        let version: u32 = data.read(dir).ok_or(Error::NotAFont)?;
        if !is_sfnt(version) {
            return Err(Error::NotAFont);
        }
        let n: u16 = data
            .read(dir.plus(Len::of(4)).ok_or(Error::NotAFont)?)
            .ok_or(Error::NotAFont)?;
        let tables = Count::of(n);
        let first = dir.plus(DIRECTORY_HEADER).ok_or(Error::NotAFont)?;
        let records = data
            .slice(first, RECORD.times(tables).ok_or(Error::NotAFont)?)
            .ok_or(Error::NotAFont)?;

        let mut face = Face {
            data,
            records,
            tables,
            upem: UnitsPerEm(0),
            glyphs: GlyphCount(0),
            line: LineMetrics {
                ascender: FontUnits(0),
                descender: FontUnits(0),
                line_gap: FontUnits(0),
            },
            hmetrics: Count::ZERO,
            hmtx: None,
            os2: None,
            post: None,
            cmap: None,
        };

        // head: unitsPerEm at 18. The specification bounds it to 16..=16384;
        // outside that a face cannot be scaled, so it is malformed rather
        // than merely odd.
        let head = face
            .table(Tag::HEAD)
            .ok_or(Error::MissingTable(Tag::HEAD))?;
        let upem: u16 = head
            .read(Offset::of(18u32))
            .ok_or(Error::Malformed(Tag::HEAD))?;
        if !(16..=16384).contains(&upem) {
            return Err(Error::Malformed(Tag::HEAD));
        }
        face.upem = UnitsPerEm(upem);

        // maxp: numGlyphs at 4, in every version.
        let maxp = face
            .table(Tag::MAXP)
            .ok_or(Error::MissingTable(Tag::MAXP))?;
        let glyphs: u16 = maxp
            .read(Offset::of(4u32))
            .ok_or(Error::Malformed(Tag::MAXP))?;
        face.glyphs = GlyphCount(glyphs);

        // hhea: ascender 4, descender 6, lineGap 8, numberOfHMetrics 34.
        let hhea = face
            .table(Tag::HHEA)
            .ok_or(Error::MissingTable(Tag::HHEA))?;
        let ascender: i16 = hhea
            .read(Offset::of(4u32))
            .ok_or(Error::Malformed(Tag::HHEA))?;
        let descender: i16 = hhea
            .read(Offset::of(6u32))
            .ok_or(Error::Malformed(Tag::HHEA))?;
        let line_gap: i16 = hhea
            .read(Offset::of(8u32))
            .ok_or(Error::Malformed(Tag::HHEA))?;
        let hmetrics: u16 = hhea
            .read(Offset::of(34u32))
            .ok_or(Error::Malformed(Tag::HHEA))?;
        face.line = LineMetrics {
            ascender: FontUnits(ascender),
            descender: FontUnits(descender),
            line_gap: FontUnits(line_gap),
        };
        face.hmetrics = Count::of(hmetrics);

        face.hmtx = face.table(Tag::HMTX);
        face.os2 = face.table(Tag::OS2);
        face.post = face.table(Tag::POST);
        face.cmap = face.table(Tag::CMAP).and_then(Cmap::parse);
        Ok(face)
    }

    fn record(&self, i: Index) -> Option<Record> {
        let at = Offset::ZERO.at(i, RECORD)?;
        let tag: u32 = self.records.read(at)?;
        let off: u32 = self.records.read(at.plus(Len::of(8))?)?;
        let len: u32 = self.records.read(at.plus(Len::of(12))?)?;
        Some(Record {
            tag: Tag::from_bytes(tag.to_be_bytes()),
            at: Offset::of(off),
            len: Len::of(len),
        })
    }

    /// The bytes of table `tag`, or `None` when absent **or when its record
    /// points outside the file** -- a table the file does not actually hold
    /// is a table the face does not have.
    ///
    /// A linear scan: the directory is sorted by tag and could be searched,
    /// but a face has a few dozen tables and this runs once per table at
    /// construction.
    pub fn table(&self, tag: Tag) -> Option<Bytes<'a>> {
        let mut i = Index::ZERO;
        while self.tables.holds(i) {
            let r = self.record(i)?;
            if r.tag == tag {
                return self.data.slice(r.at, r.len);
            }
            i = i.next();
        }
        None
    }

    pub fn units_per_em(&self) -> UnitsPerEm {
        self.upem
    }

    pub fn glyph_count(&self) -> GlyphCount {
        self.glyphs
    }

    /// `hhea`'s ascender, descender and line gap.
    pub fn line_metrics(&self) -> LineMetrics {
        self.line
    }

    /// `OS/2`'s `sTypoAscender`, `sTypoDescender` and `sTypoLineGap`, the set
    /// Windows and new layout engines use; `None` without an `OS/2` table.
    pub fn typo_metrics(&self) -> Option<LineMetrics> {
        let os2 = self.os2?;
        Some(LineMetrics {
            ascender: FontUnits(os2.read(Offset::of(68u32))?),
            descender: FontUnits(os2.read(Offset::of(70u32))?),
            line_gap: FontUnits(os2.read(Offset::of(72u32))?),
        })
    }

    /// `OS/2`'s `usWinAscent` and `usWinDescent`, as a pair of design units;
    /// `None` when either does not fit the signed range every other metric
    /// uses.
    pub fn win_metrics(&self) -> Option<(FontUnits, FontUnits)> {
        let os2 = self.os2?;
        let asc: u16 = os2.read(Offset::of(74u32))?;
        let desc: u16 = os2.read(Offset::of(76u32))?;
        Some((
            FontUnits(i16::try_from(asc).ok()?),
            FontUnits(i16::try_from(desc).ok()?),
        ))
    }

    /// `post`'s underline position and thickness.
    pub fn underline(&self) -> Option<Underline> {
        let post = self.post?;
        Some(Underline {
            position: FontUnits(post.read(Offset::of(8u32))?),
            thickness: FontUnits(post.read(Offset::of(10u32))?),
        })
    }

    /// `post`'s `isFixedPitch` claim.
    pub fn pitch(&self) -> Option<Pitch> {
        let post = self.post?;
        let fixed: u32 = post.read(Offset::of(12u32))?;
        Some(if fixed == 0 {
            Pitch::Proportional
        } else {
            Pitch::Fixed
        })
    }

    fn hmtx_index(&self, g: GlyphId) -> Option<Index> {
        if !self.glyphs.holds(g) || self.hmetrics.is_zero() {
            return None;
        }
        Some(Index::of(u32::try_from(g.index()).ok()?))
    }

    /// The advance of `g` from `hmtx`, or `None` for a glyph the face does
    /// not have or a table too short to hold it.
    ///
    /// `hmtx` may carry fewer records than glyphs; past `numberOfHMetrics`
    /// every glyph takes the last record's advance
    /// (`[[source: ms-opentype-hmtx]]`).
    pub fn advance(&self, g: GlyphId) -> Option<Advance> {
        let i = self.hmtx_index(g)?;
        let slot = if self.hmetrics.holds(i) {
            i
        } else {
            self.hmetrics.last()?
        };
        let at = Offset::ZERO.at(slot, Len::of(4))?;
        self.hmtx?.read(at).map(Advance)
    }

    /// The left side bearing of `g` from `hmtx`: in the record for the first
    /// `numberOfHMetrics` glyphs, in the trailing `leftSideBearings` array
    /// for the rest.
    pub fn side_bearing(&self, g: GlyphId) -> Option<FontUnits> {
        let i = self.hmtx_index(g)?;
        let at = if self.hmetrics.holds(i) {
            Offset::ZERO.at(i, Len::of(4))?.plus(Len::of(2))?
        } else {
            let records = Len::of(4).times(self.hmetrics)?;
            let k = i.since(self.hmetrics.as_index())?;
            Offset::ZERO.plus(records)?.at(k.as_index(), Len::of(2))?
        };
        self.hmtx?.read(at).map(FontUnits)
    }

    /// The glyph `cmap` maps `c` to, or `None` for a character the face does
    /// not carry -- glyph 0 is never answered, so a caller can fall back to the
    /// next face rather than draw `.notdef`.
    pub fn glyph(&self, c: Codepoint) -> Option<GlyphId> {
        self.cmap.as_ref()?.glyph(c)
    }

    /// What `cmap` format 14 says about `c` followed by variation selector
    /// `vs`.
    pub fn variation(&self, c: Codepoint, vs: Codepoint) -> Variation {
        match &self.cmap {
            Some(cmap) => cmap.variation(c, vs),
            None => Variation::Absent,
        }
    }
}
