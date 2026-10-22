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

use crate::bitmap::{Bitmap, Cbdt, Ppem, ReadStrike, Records, Sbix, StrikeState};
use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::cff::Cff;
use crate::cff2::Cff2;
use crate::cmap::{Cmap, Variation};
use crate::colr::{Colr, Cpal, Layers, Palette, PaletteEntry, Rgba};
use crate::glyf::{Glyf, Instance, Loca, LocaFormat, Metrics, Need, Streamed};
use crate::gpos::Gpos;
use crate::gsub::Gsub;
use crate::layout::Gdef;
use crate::morx::Morx;
use crate::outline::{Drawn, Sink};
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

/// HarfBuzz's `roundf`, which is its own `floorf (x + .5f)`
/// (`[[source: harfbuzz-12-3-0-hb]]`): a half rounds up, -0.5 to 0.
pub(crate) fn round_half_up(x: f32) -> f32 {
    core::f32::math::floor(x + 0.5)
}

impl Advance {
    pub const fn value(self) -> u16 {
        self.0
    }

    /// A rounded varied advance, floored at 0 and saturating at 65,535.
    fn saturating(v: f32) -> Advance {
        if v <= 0.0 || v.is_nan() {
            return Advance(0);
        }
        let v = if v >= 65535.0 { 65535.0 } else { v };
        Advance(u16::try_from(v as i32).unwrap_or(u16::MAX))
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
    /// `vmtx`'s long records and top side bearings, as many as the table
    /// holds, and the table; `None` for a face without vertical metrics.
    vmetrics: Count,
    vbearings: Count,
    vmtx: Option<Bytes<'a>>,
    os2: Option<Bytes<'a>>,
    post: Option<Bytes<'a>>,
    cmap: Option<Cmap<'a>>,
    glyf: Option<Glyf<'a>>,
    cff: Option<Cff<'a>>,
    cff2: Option<Cff2<'a>>,
    sbix: Option<Sbix<'a>>,
    cbdt: Option<Cbdt<'a>>,
    colr: Option<Colr<'a>>,
    cpal: Option<Cpal<'a>>,
    gdef: Gdef<'a>,
    gsub: Option<Gsub<'a>>,
    gpos: Option<Gpos<'a>>,
    morx: Option<Morx<'a>>,
    fvar: Option<crate::var::Fvar<'a>>,
    avar: Option<crate::var::Avar<'a>>,
    gvar: Option<crate::gvar::Gvar<'a>>,
    hvar: Option<crate::hvar::Hvar<'a>>,
    coords: crate::var::Coords,
}

/// Counts what passes through to the real sink, so [`Face::outline`] can
/// say whether anything was drawn without the caller counting.
struct Counting<'s, S: Sink> {
    inner: &'s mut S,
    drawn: Drawn,
}

impl<S: Sink> Sink for Counting<'_, S> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.drawn = Drawn::Outline;
        self.inner.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.drawn = Drawn::Outline;
        self.inner.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.drawn = Drawn::Outline;
        self.inner.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.drawn = Drawn::Outline;
        self.inner.curve_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.inner.close();
    }
    /// Passed on whole, so the sink underneath decides how it is drawn.
    fn flex(&mut self, f: &crate::outline::Flex) {
        self.drawn = Drawn::Outline;
        self.inner.flex(f);
    }
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

/// **A face's table directory and nothing more**: where each table is,
/// for a reader that wants a few tables of many faces -- discovery reads
/// `name`, `OS/2`, `head`, `post` and `fvar` of every installed face and
/// parses none of them ([`crate::Described`]).
#[derive(Clone, Copy)]
pub(crate) struct Directory<'a> {
    data: Bytes<'a>,
    records: Bytes<'a>,
    tables: Count,
}

impl<'a> Directory<'a> {
    /// Face `index`'s directory in `data`: the one face of an sfnt, or
    /// face `index` of a `'ttcf'` collection.
    pub(crate) fn of(data: &'a [u8], index: FaceIndex) -> Result<Directory<'a>, Error> {
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
        let version: u32 = b.read(dir).ok_or(Error::NotAFont)?;
        if !is_sfnt(version) {
            return Err(Error::NotAFont);
        }
        let n: u16 = b
            .read(dir.plus(Len::of(4)).ok_or(Error::NotAFont)?)
            .ok_or(Error::NotAFont)?;
        let tables = Count::of(n);
        let first = dir.plus(DIRECTORY_HEADER).ok_or(Error::NotAFont)?;
        let records = b
            .slice(first, RECORD.times(tables).ok_or(Error::NotAFont)?)
            .ok_or(Error::NotAFont)?;
        Ok(Directory {
            data: b,
            records,
            tables,
        })
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

    /// The bytes of table `tag`, or `None` when absent or when its record
    /// points outside the file. A linear scan: the directory is sorted by
    /// tag and could be searched, but a face has a few dozen tables.
    pub(crate) fn table(&self, tag: Tag) -> Option<Bytes<'a>> {
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
        let dir = Directory::of(data, index)?;
        Face::at_directory(dir)
    }

    fn at_directory(dir: Directory<'a>) -> Result<Face<'a>, Error> {
        let Directory {
            data,
            records,
            tables,
        } = dir;

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
            vmetrics: Count::ZERO,
            vbearings: Count::ZERO,
            vmtx: None,
            os2: None,
            post: None,
            cmap: None,
            glyf: None,
            cff: None,
            cff2: None,
            sbix: None,
            cbdt: None,
            colr: None,
            cpal: None,
            gdef: Gdef::default(),
            gsub: None,
            gpos: None,
            morx: None,
            fvar: None,
            avar: None,
            gvar: None,
            hvar: None,
            coords: crate::var::Coords::default(),
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
        // vhea: numOfLongVerMetrics at 34, which says how many of `vmtx`'s
        // records are long (`[[source: ms-opentype-vhea]]`,
        // `[[source: ms-opentype-vmtx]]`). The long records are clamped to
        // the table, and the bearings to the glyph count and what is left
        // of the table, as HarfBuzz's accelerator clamps them
        // (`[[source: harfbuzz-12-3-0-hmtx]]`). No long record, or no
        // `vhea`, is read as no vertical metrics: the text lays `vmtx` out
        // by `vhea`'s count, which then names no record. HarfBuzz departs
        // in both cases: it reads every tsb as 0, as this does, but where
        // the table holds more two-byte entries than the face has glyphs it
        // takes the surplus's count as its number of advance heights and
        // reads that many u16s from the table's start, glyph g reading entry
        // min(g, count - 1).
        if let (Some(vhea), Some(vmtx)) = (face.table(Tag::VHEA), face.table(Tag::VMTX)) {
            let long: Option<u16> = vhea.read(Offset::of(34u32));
            let len = vmtx.len().value() & !1;
            let long = long.map_or(0, u32::from).min(len / 4);
            let bearings = u32::from(glyphs).max(long).min(long + (len - long * 4) / 2);
            if long > 0 {
                face.vmetrics = Count::of(long);
                face.vbearings = Count::of(bearings);
                face.vmtx = Some(vmtx);
            }
        }
        face.os2 = face.table(Tag::OS2);
        face.post = face.table(Tag::POST);
        face.cmap = face.table(Tag::CMAP).and_then(Cmap::parse);

        // Outlines: `loca` + `glyf` for TrueType, `CFF ` otherwise. The loca
        // format is `head.indexToLocFormat` at 50; any value but 0 or 1
        // means the pair cannot be read and the face has no outlines.
        let loca_format: Option<i16> = head.read(Offset::of(50u32));
        let format = match loca_format {
            Some(0) => Some(LocaFormat::Short),
            Some(1) => Some(LocaFormat::Long),
            _ => None,
        };
        // flags at 16 are not read: bit 1, "Left sidebearing point at x=0",
        // does not move where a TrueType glyph is drawn from (`glyf.rs`'s
        // doc).
        if let (Some(format), Some(loca), Some(glyf)) =
            (format, face.table(Tag::LOCA), face.table(Tag::GLYF))
        {
            let loca = Loca::parse(loca, format, Count::of(glyphs));
            face.glyf = Some(Glyf::new(loca, glyf));
        }
        face.cff = face.table(Tag::CFF).and_then(Cff::parse);
        // `CFF2`, when it is there, is read whole or the face is refused --
        // if it is the face's outlines. A face whose only outlines did not
        // read would answer every glyph with nothing, and a terminal would
        // draw blanks for it without a word; HarfBuzz tries `glyf`, then
        // `CFF2`, then `CFF ` (`[[source: harfbuzz-12-3-0-ot-font]]`), so a
        // face with another outline table falls back to it instead.
        if let Some(t) = face.table(Tag::CFF2) {
            face.cff2 = Cff2::parse(t, Count::of(glyphs));
            if face.cff2.is_none() && face.glyf.is_none() && face.cff.is_none() {
                return Err(Error::Malformed(Tag::CFF2));
            }
        }
        face.gvar = face.table(Tag::GVAR).and_then(crate::gvar::Gvar::parse);
        face.hvar = face.table(Tag::HVAR).and_then(crate::hvar::Hvar::parse);
        // Bitmap strikes and layered colour are optional in every sense: a
        // face without them answers `None` and draws its outlines.
        face.sbix = face
            .table(Tag::SBIX)
            .and_then(|t| Sbix::parse(t, Count::of(u32::from(face.glyphs.0))));
        face.cbdt = match (face.table(Tag::CBLC), face.table(Tag::CBDT)) {
            (Some(cblc), Some(cbdt)) => {
                Cbdt::parse(cblc, cbdt, Count::of(u32::from(face.glyphs.0)))
            }
            _ => None,
        };
        face.colr = face.table(Tag::COLR).and_then(Colr::parse);
        face.cpal = face.table(Tag::CPAL).and_then(Cpal::parse);
        // Layout: `GDEF` is optional and every lookup consults it; `GSUB`
        // is what a ligature or emoji font shapes with.
        face.gdef = face
            .table(Tag::GDEF)
            .and_then(Gdef::parse)
            .unwrap_or_default();
        face.gsub = face
            .table(Tag::GSUB)
            .and_then(|t| Gsub::parse(t, face.gdef));
        face.gpos = face
            .table(Tag::GPOS)
            .and_then(|t| Gpos::parse(t, face.gdef));
        face.morx = face.table(Tag::MORX).and_then(Morx::parse);
        face.fvar = face.table(Tag::FVAR).and_then(crate::var::Fvar::parse);
        face.avar = face.table(Tag::AVAR).and_then(crate::var::Avar::parse);
        Ok(face)
    }

    /// The variation axes the face offers, in `fvar`'s order; none for a
    /// static face.
    pub fn axis(&self, i: Index) -> Option<crate::var::Axis> {
        self.fvar.and_then(|f| f.axis(i))
    }

    /// Reads the face at `settings`: each named axis normalised, the rest
    /// left where they were (at the default on a fresh face). A static face
    /// ignores them.
    pub fn set_variations(&mut self, settings: &[crate::var::Variation]) {
        if let Some(fvar) = self.fvar {
            crate::var::apply(fvar, self.avar, settings, &mut self.coords);
        }
    }

    /// How many tables the directory lists.
    pub(crate) fn table_count(&self) -> Count {
        self.tables
    }

    /// Directory record `i`: its tag, and its bytes when the record lies
    /// inside the file.
    pub(crate) fn table_at(&self, i: Index) -> Option<(Tag, Option<Bytes<'a>>)> {
        let r = self.directory().record(i)?;
        Some((r.tag, self.data.slice(r.at, r.len)))
    }

    fn directory(&self) -> Directory<'a> {
        Directory {
            data: self.data,
            records: self.records,
            tables: self.tables,
        }
    }

    pub(crate) fn glyf(&self) -> Option<Glyf<'a>> {
        self.glyf
    }

    pub(crate) fn gvar(&self) -> Option<crate::gvar::Gvar<'a>> {
        self.gvar
    }

    /// The normalised coordinates the face is read at.
    pub fn coords(&self) -> &crate::var::Coords {
        &self.coords
    }

    /// The bytes of table `tag`, or `None` when absent **or when its record
    /// points outside the file** -- a table the file does not actually hold
    /// is a table the face does not have.
    ///
    /// A linear scan: the directory is sorted by tag and could be searched,
    /// but a face has a few dozen tables and this runs once per table at
    /// construction.
    pub fn table(&self, tag: Tag) -> Option<Bytes<'a>> {
        self.directory().table(tag)
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
    ///
    /// This is the default instance's advance whatever the face's
    /// coordinates; [`Face::advance`] answers at them.
    pub fn default_advance(&self, g: GlyphId) -> Option<Advance> {
        let i = self.hmtx_index(g)?;
        let slot = if self.hmetrics.holds(i) {
            i
        } else {
            self.hmetrics.last()?
        };
        let at = Offset::ZERO.at(slot, Len::of(4))?;
        self.hmtx?.read(at).map(Advance)
    }

    /// The advance of `g` at the face's coordinates, in font units, as
    /// HarfBuzz 12.3.0 answers it (`[[source: harfbuzz-12-3-0-ot-font]]`,
    /// `[[source: harfbuzz-12-3-0-hmtx]]`):
    /// at the default instance, [`Face::default_advance`]; with `HVAR`, the
    /// `hmtx` advance plus its delta rounded half up, floored at 0; with no `HVAR` but `gvar`, the phantom points' extent rounded and
    /// floored at 0 -- `upem / 2` when the glyph cannot be walked, and 0 past
    /// the glyph count; with neither, the `hmtx` advance. No scratch in any
    /// case. An advance past 65,535 saturates there, where HarfBuzz's is an
    /// unsigned int; no real font comes near it.
    pub fn advance(&self, g: GlyphId) -> Option<Advance> {
        if self.coords.is_default() {
            return self.default_advance(g);
        }
        // HarfBuzz's `hmtx` answers 0 for a glyph id past the face, where
        // [`Face::default_advance`] -- ttf-parser's rule -- answers nothing.
        let past = !self.glyphs.holds(g);
        if let Some(hvar) = self.hvar {
            let base = if past {
                0.0
            } else {
                f32::from(self.default_advance(g)?.0)
            };
            return Some(Advance::saturating(
                base + round_half_up(hvar.delta(g, &self.coords)),
            ));
        }
        let (Some(glyf), Some(gvar)) = (self.glyf, self.gvar) else {
            return self.default_advance(g);
        };
        if past {
            return Some(Advance(0));
        }
        let hmtx = |g: GlyphId| self.metrics(g);
        Some(match glyf.phantom_extent(g, gvar, &self.coords, &hmtx) {
            Some(w) => Advance::saturating(round_half_up(w)),
            None => Advance(self.upem.0 / 2),
        })
    }

    /// `g`'s advance and left side bearing from `hmtx`, which place its
    /// first two phantom points: `None` without an advance, and a bearing
    /// of 0 where the table has none.
    fn metrics(&self, g: GlyphId) -> Option<(i32, i32)> {
        let advance = i32::from(self.default_advance(g)?.0);
        let lsb = self.side_bearing(g).map_or(0, |b| i32::from(b.value()));
        Some((advance, lsb))
    }

    /// `g`'s advance height and top side bearing from `vmtx`, which place
    /// its two vertical phantom points, as HarfBuzz reads them: the bearing
    /// from the long records, then the trailing array, and 0 past both; the
    /// advance from its record, or the last long record's for a glyph past
    /// them, and 0 past the glyph count. A face without vertical metrics
    /// answers HarfBuzz's `(unitsPerEm, 0)` for every glyph, which the text
    /// leaves open (`glyf.rs`'s `push_phantoms`).
    fn vertical(&self, g: GlyphId) -> (i32, i32) {
        let Some(vmtx) = self.vmtx else {
            return (i32::from(self.upem.0), 0);
        };
        let Some(i) = u32::try_from(g.index()).ok().map(Index::of) else {
            return (0, 0);
        };
        let record = |k: Index| Offset::ZERO.at(k, Len::of(4));
        let advance = |k: Index| -> i32 {
            record(k)
                .and_then(|at| vmtx.read::<u16>(at))
                .map_or(0, i32::from)
        };
        let Some(last) = self.vmetrics.last() else {
            return (0, 0);
        };
        if !self.vbearings.holds(i) {
            let height = if self.glyphs.holds(g) {
                advance(last)
            } else {
                0
            };
            return (height, 0);
        }
        let tsb = if self.vmetrics.holds(i) {
            record(i).and_then(|at| at.plus(Len::of(2)))
        } else {
            Len::of(4)
                .times(self.vmetrics)
                .zip(i.since(self.vmetrics.as_index()))
                .and_then(|(records, k)| Offset::ZERO.plus(records)?.at(k.as_index(), Len::of(2)))
        };
        let tsb = tsb.and_then(|at| vmtx.read::<i16>(at)).map_or(0, i32::from);
        let slot = if self.vmetrics.holds(i) { i } else { last };
        (advance(slot), tsb)
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

    /// Whether the face refuses `g` ([`Drawn::Refused`]): a `glyf`
    /// composite whose first component, at any depth of its tree, is placed
    /// by matched points. The same at every instance, since it reads only the
    /// components' flags. A caller that caps its scratch asks
    /// [`Face::refuses_past`], which answers this and the budget at once.
    pub fn refuses(&self, g: GlyphId) -> bool {
        self.glyf.is_some_and(|glyf| glyf.refuses(g))
    }

    /// Whether [`Face::outline_at`] cannot draw `g` with `budget` points of
    /// scratch: the face refuses it ([`Face::refuses`]), or its
    /// [`Face::outline_need`] is past `budget`. A caller that caps its
    /// scratch asks this to pass the glyph over as it passes over a refused
    /// one (baud-render's `Stack`). Exactly `refuses(g) ||
    /// outline_need(g) > budget`, from one walk of a composite and none of
    /// a simple glyph while `budget` holds the most one can need.
    pub fn refuses_past(&self, g: GlyphId, budget: Count) -> bool {
        let at = match self.varying() {
            Some(_) => Instance::Varied,
            None => Instance::Default,
        };
        self.glyf
            .is_some_and(|glyf| glyf.refuses_past(g, budget, at))
    }

    /// Whether `g` has an outline of its own to draw, as far as a `glyf`
    /// header says: false for a glyph with no data or no contours; true
    /// for any glyph of a face without `glyf`, which only drawing tells.
    pub fn has_outline(&self, g: GlyphId) -> bool {
        self.glyf.is_none_or(|glyf| glyf.has_outline(g))
    }

    /// Every codepoint range `cmap` could map -- candidates for
    /// [`Face::glyph`], a superset of what it answers (`Cmap::candidates`).
    pub fn candidates(&self, f: impl FnMut(Codepoint, Codepoint)) {
        if let Some(cmap) = &self.cmap {
            cmap.candidates(f);
        }
    }

    /// What `cmap` format 14 says about `c` followed by variation selector
    /// `vs`.
    pub fn variation(&self, c: Codepoint, vs: Codepoint) -> Variation {
        match &self.cmap {
            Some(cmap) => cmap.variation(c, vs),
            None => Variation::Absent,
        }
    }

    /// How many bitmap strikes the face carries, from `sbix` first and
    /// `CBLC` otherwise -- a face carries one or the other in practice, and
    /// a strike index below this count names one of them.
    pub fn strikes(&self) -> Count {
        if let Some(s) = &self.sbix {
            s.strikes()
        } else if let Some(c) = &self.cbdt {
            c.strikes()
        } else {
            Count::ZERO
        }
    }

    /// The pixel size strike `i` was rendered at.
    pub fn strike_ppem(&self, i: Index) -> Option<Ppem> {
        if let Some(s) = &self.sbix {
            s.ppem(i)
        } else {
            self.cbdt.as_ref()?.ppem(i)
        }
    }

    /// Whether strike `i` is read and how, or why it is refused: `None` past
    /// [`Face::strikes`] or where the strike's `ppem` does not read. An `sbix`
    /// strike is always read; a `CBLC` size is refused when its index subtable
    /// records run past the table or their first glyphs decrease.
    pub fn strike(&self, i: Index) -> Option<StrikeState> {
        if let Some(s) = &self.sbix {
            Some(StrikeState::Read(ReadStrike::new(
                i,
                s.ppem(i)?,
                Records::Disjoint,
            )))
        } else {
            self.cbdt.as_ref()?.state(i)
        }
    }

    /// Glyph `g`'s PNG in a strike already checked by this face's
    /// [`Face::strike`]; a [`ReadStrike`] from another face can answer wrongly.
    pub fn bitmap_in(&self, s: ReadStrike, g: GlyphId) -> Option<Bitmap<'a>> {
        if let Some(sb) = &self.sbix {
            sb.bitmap(s.index(), g)
        } else {
            self.cbdt.as_ref()?.bitmap_in(s, g)
        }
    }

    /// Glyph `g`'s PNG in strike `i`, still encoded, with its placement:
    /// [`Face::strike`] checked on every call, then [`Face::bitmap_in`]. A
    /// renderer that asks a strike for many glyphs keeps the [`ReadStrike`].
    pub fn bitmap(&self, i: Index, g: GlyphId) -> Option<Bitmap<'a>> {
        match self.strike(i)? {
            StrikeState::Read(s) => self.bitmap_in(s, g),
            StrikeState::Refused(_) => None,
        }
    }

    /// The `GDEF` table's view of the face: glyph classes and mark sets, or
    /// the empty view when the face has none.
    pub fn gdef(&self) -> Gdef<'a> {
        self.gdef
    }

    /// The substitution table, when the face has one.
    pub fn gsub(&self) -> Option<&Gsub<'a>> {
        self.gsub.as_ref()
    }

    /// The positioning table, when the face has one.
    pub fn gpos(&self) -> Option<&Gpos<'a>> {
        self.gpos.as_ref()
    }

    /// The `morx` table, when the face carries one.
    pub fn morx(&self) -> Option<&Morx<'a>> {
        self.morx.as_ref()
    }

    /// Whether `g` draws in colour: it has `COLR` layers, or a bitmap in
    /// any strike. A face can carry both kinds -- a `COLR` font's unlayered
    /// glyphs are outlines -- so this asks about the glyph, not the face.
    ///
    /// **Any strike, not the first.** A glyph can be empty in some strikes
    /// and present in others, and the specifications require it be drawn
    /// from one that has it (`[[source: ms-opentype-sbix]]`,
    /// `[[source: apple-ttrm-sbix]]`): Apple Color Emoji's first strike, 20
    /// ppem, lacks 96 glyphs the others carry, the pink heart U+1FA77 among
    /// them, and asking it alone answered that the heart was not colour. The
    /// renderer's atlas falls back across strikes, so the two ask nearly the
    /// same question: the atlas also needs the PNG to decode, so a glyph whose
    /// every image is unreadable is colour here and draws nothing there
    /// (`a_glyph_whose_every_image_is_unreadable_is_colour_and_draws_nothing`).
    pub fn is_colour(&self, g: GlyphId) -> bool {
        self.layers(g).is_some() || self.has_bitmap(g)
    }

    /// Whether some strike holds an image for `g`, through any `dupe` or
    /// `flip` on the way: the reference walk over every strike, which a
    /// refused `CBLC` size answers nothing in.
    pub fn has_bitmap(&self, g: GlyphId) -> bool {
        let strikes = self.strikes();
        let mut i = Index::ZERO;
        while strikes.holds(i) {
            if self.bitmap(i, g).is_some() {
                return true;
            }
            i = i.next();
        }
        false
    }

    /// The `COLR` v0 layers of `g`, bottom first, or `None` for a glyph the
    /// table does not describe.
    pub fn layers(&self, g: GlyphId) -> Option<Layers<'a>> {
        self.colr.as_ref()?.layers(g)
    }

    /// How many palettes `CPAL` carries; zero without the table.
    pub fn palettes(&self) -> Count {
        self.cpal.as_ref().map_or(Count::ZERO, Cpal::palettes)
    }

    /// Entry `e` of palette `p`, or `None` for an entry the table lacks --
    /// including [`PaletteEntry::FOREGROUND`], which is the caller's colour
    /// rather than the font's.
    pub fn color(&self, p: Palette, e: PaletteEntry) -> Option<Rgba> {
        self.cpal.as_ref()?.color(p, e)
    }

    /// The `gvar` table when the face is read at a variation: every
    /// TrueType glyph then goes through the varied path, as HarfBuzz sends
    /// every glyph through `accelerator_t::apply_deltas_to_points` once any coordinate is
    /// set -- a composite with no data of its own can have components that
    /// move.
    fn varying(&self) -> Option<(Glyf<'a>, crate::gvar::Gvar<'a>)> {
        if self.coords.is_default() {
            return None;
        }
        Some((self.glyf?, self.gvar?))
    }

    /// How many [`crate::VarPoint`]s [`Face::outline_at`] needs for `g`:
    /// at a variation, what the varied walk needs; at the default instance,
    /// what a glyph that places a component by matched points needs, since
    /// only that walk can align it; and 0 for everything else -- a glyph
    /// that draws without scratch, a face with no `glyf` or `gvar` to vary,
    /// or a glyph the walk refuses.
    pub fn outline_need(&self, g: GlyphId) -> Count {
        match self.varying() {
            Some((glyf, _)) => glyf.need(g),
            None => self.glyf.and_then(|glyf| glyf.matched_need(g)),
        }
        .unwrap_or(Count::ZERO)
    }

    /// The outline of `g` into `sink`, in font units, unflattened: quadratic
    /// curves from `glyf`, cubics from `CFF2` and CFF. `glyf` is tried
    /// first, `CFF2` second and CFF third, which is HarfBuzz's order.
    /// `Drawn::Nothing` for a glyph with no contours, no data, or a
    /// malformed description. A `glyf` glyph is drawn from its left side
    /// bearing point, `xMin - lsb`, whatever `head` flags bit 1 says
    /// (`glyf.rs`'s doc); CFF and `CFF2` outlines carry their bearing in
    /// their coordinates.
    ///
    /// **A glyph that needs scratch is refused here**, answered
    /// `Drawn::NeedsScratch` with nothing drawn, and [`Face::outline_at`]
    /// draws it: at a variation, every TrueType glyph whose outline varies,
    /// rather than the default instance in its place; at the default
    /// instance, a composite that places a component by matched points,
    /// whose alignment reads points a stream has given away.
    /// A `CFF2` glyph needs no scratch: its `blend` operators are applied at
    /// the face's coordinates as it is drawn.
    pub fn outline<S: Sink>(&self, g: GlyphId, sink: &mut S) -> Drawn {
        if let Some((glyf, _)) = self.varying() {
            return match glyf.walk_need(g) {
                Some(Need::Points(need)) => Drawn::NeedsScratch(need),
                Some(Need::Refused) => Drawn::Refused,
                None => Drawn::Nothing,
            };
        }
        let mut counting = Counting {
            inner: sink,
            drawn: Drawn::Nothing,
        };
        let parsed = if let Some(glyf) = &self.glyf {
            let hmtx = |g: GlyphId| self.metrics(g);
            match glyf.outline(g, &hmtx, &mut counting) {
                Some(Streamed::Drawn) => Some(()),
                Some(Streamed::NeedsScratch(need)) => return Drawn::NeedsScratch(need),
                Some(Streamed::Refused) => return Drawn::Refused,
                None => None,
            }
        } else if let Some(cff2) = &self.cff2 {
            cff2.outline(g, &self.coords, &mut counting)
        } else if let Some(cff) = &self.cff {
            cff.outline(g, &mut counting)
        } else {
            None
        };
        match parsed {
            Some(()) => counting.drawn,
            None => Drawn::Nothing,
        }
    }

    /// The outline of `g` at the face's coordinates, with `scratch` for a
    /// glyph that needs it: [`Face::outline`] for one that does not, else
    /// the walk that flattens the glyph's components -- with its `gvar`
    /// deltas at a variation (HarfBuzz's flattening, drawn from the left
    /// phantom where it ends), without them at the default instance
    /// for a glyph that matches points; the rules are on
    /// `Glyf::outline_varied`. `Drawn::NeedsScratch` when `scratch` holds
    /// fewer points than [`Face::outline_need`] says.
    pub fn outline_at<S: Sink>(
        &self,
        g: GlyphId,
        scratch: &mut [crate::VarPoint],
        sink: &mut S,
    ) -> Drawn {
        let (glyf, gvar, need) = match self.varying() {
            Some((glyf, gvar)) => match glyf.walk_need(g) {
                Some(Need::Points(need)) => (glyf, Some(gvar), need),
                Some(Need::Refused) => return Drawn::Refused,
                None => return Drawn::Nothing,
            },
            None => match (self.outline(g, sink), self.glyf) {
                (Drawn::NeedsScratch(need), Some(glyf)) => (glyf, None, need),
                (drawn, _) => return drawn,
            },
        };
        if scratch.len() < need.slot() {
            return Drawn::NeedsScratch(need);
        }
        let mut counting = Counting {
            inner: sink,
            drawn: Drawn::Nothing,
        };
        let hmtx = |g: GlyphId| self.metrics(g);
        let vmtx = |g: GlyphId| self.vertical(g);
        let metrics = Metrics {
            hmtx: &hmtx,
            vmtx: &vmtx,
        };
        match glyf.outline_varied(g, gvar, &self.coords, metrics, scratch, &mut counting) {
            Some(()) => counting.drawn,
            None => Drawn::Nothing,
        }
    }
}
