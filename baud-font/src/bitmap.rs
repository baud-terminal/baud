//! Bitmap strikes: `sbix` (`[[source: ms-opentype-sbix]]`,
//! `[[source: apple-ttrm-sbix]]`) and `CBLC`/`CBDT`
//! (`[[source: ms-opentype-cblc]]`, `[[source: ms-opentype-cbdt]]`).
//!
//! **What a strike is.** A set of pre-rendered glyph images at one pixel
//! size. `sbix` is Apple's form: per strike, a `ppem`, a `ppi` and one
//! offset per glyph into a run of `(originOffsetX, originOffsetY,
//! graphicType, data)` records. `CBLC` is the locator for `CBDT`: per
//! `BitmapSize`, line metrics, a glyph range, a `ppem` and an array of index
//! subtables that map a glyph range to an image location and format.
//!
//! **What is read and what is refused.** `sbix`: `'png '` records,
//! `'dupe'` redirections and Apple's undocumented `'flip'`, bounded together
//! at ten records read -- nine hops -- because either can point at itself;
//! `'jpg '` and
//! `'tiff'` answer `None`. `CBLC`: index formats 1 through 5 and image formats
//! 17, 18 and 19 -- the PNG ones; the byte- and bit-aligned mono formats
//! belong to `EBDT` and are not colour emoji. Every offset is bounded by its
//! table.
//!
//! **`'flip'` is a `'dupe'` that mirrors.** Its payload is a glyph id, like
//! a dupe's, and the glyph draws as that glyph's image mirrored left to
//! right within the image's own width, at that glyph's offsets. Apple Color
//! Emoji carries 108 such glyphs in every strike, the people facing right
//! (`U+1F3C3 U+200D U+27A1`). Measured against CoreText on that face at
//! native strike sizes, the mirror within the image matches at zero pixel
//! error and a mirror about the origin, a vertical one or a rotation do not;
//! on modified copies, neither glyph's advance moves the axis and the
//! flip's own offsets are ignored. FreeType 2.14 agrees on all of that
//! (its sbix image loader in `ttsbit.c`).
//!
//! **A chain of flips mirrors once.** The two disagree only here, on a chain
//! no known font contains: FreeType toggles per flip, so a flip of a flip
//! draws upright, while CoreText, Apple's own renderer of its own format,
//! draws any chain holding a flip mirrored exactly once. baud follows
//! CoreText (Jev 0.91, confidence 0.81, under the bar, so the owner's to
//! overturn).
//!
//! **A `CBLC` size is read only when its records are sorted.** `CBLC`'s
//! formats are "downward compatible extensions of the EBDT and EBLC tables"
//! (`[[source: ms-opentype-cblc]]`), and EBLC's index subtable records "must
//! be sorted by firstGlyphIndex, and records should not have overlapping glyph
//! ID ranges" (`[[source: ms-opentype-eblc]]`). A size whose records run past
//! the table, or whose first glyphs decrease, is refused whole, and the rest
//! of the table still draws ([`StrikeState`]). Overlap is a "should not", so
//! an overlapping size is read and a glyph answers from the first record that
//! holds it, as FreeType, ttf-parser and HarfBuzz do: by binary search when no
//! record starts inside another or when each record also ends no earlier
//! than the one before, and by walking the records only on a size where one
//! nests inside another ([`Records`]). A glyph at or past the face's
//! `numGlyphs` has no bitmap, as in `sbix`. Format 4's pairs are searched
//! among the real ones; the extra pair only marks where the last image ends.
//! The owner's calls of 2026-10-02 (sorted records) and 2026-10-03 (format 4,
//! and the first record on an overlap); note J has the research.
//!
//! **Strikes are enumerated, not chosen.** Which strike a renderer wants for
//! a cell size is a policy the compositor owns; this module answers what the
//! face has, so the oracle differential can walk every strike rather than
//! agree with a heuristic. A glyph can be present in some strikes and empty
//! in others -- Apple Color Emoji has 177 such glyphs -- so "the face has a
//! bitmap for this glyph" is a question about every strike, which
//! [`Sbix::bitmap`] answers one strike at a time.

use crate::bytes::{Array, Bytes, Count, FromBe, Index, Len, Offset};
use baud_domain::GlyphId;
use core::cmp::Ordering;

/// Pixels per em a strike was rendered at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Ppem(u16);

impl Ppem {
    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Where a bitmap sits relative to the glyph origin, in pixels, as the
/// table states it: `sbix` gives the offset of the image's bottom-left
/// corner; `CBDT` gives the bearing of the image's top-left corner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    pub x: i16,
    pub y: i16,
}

/// Which table the bitmap came from, which decides how [`Placement`] reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Strike {
    Sbix,
    Cbdt,
}

/// Whether a bitmap draws as stored or mirrored left to right within its
/// own width: an `sbix` `'flip'` record anywhere on the way to its image.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirror {
    AsStored,
    LeftRight,
}

/// One glyph's image: PNG bytes, still encoded, with where to put them and
/// whether to mirror them.
#[derive(Clone, Copy)]
pub struct Bitmap<'a> {
    pub png: Bytes<'a>,
    pub placement: Placement,
    pub ppem: Ppem,
    pub strike: Strike,
    pub mirror: Mirror,
}

/// A strike whose records were checked: the only key [`crate::Face::bitmap_in`]
/// takes, so a renderer checks a strike once and then asks it per glyph. It
/// answers only for the face that issued it: asked of another face, it reads
/// that face's size [`ReadStrike::index`] with the first face's search, which
/// can answer from a size the second face refuses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReadStrike {
    index: Index,
    ppem: Ppem,
    records: Records,
}

impl ReadStrike {
    pub(crate) fn new(index: Index, ppem: Ppem, records: Records) -> ReadStrike {
        ReadStrike {
            index,
            ppem,
            records,
        }
    }

    pub fn index(self) -> Index {
        self.index
    }

    pub fn ppem(self) -> Ppem {
        self.ppem
    }

    pub fn records(self) -> Records {
        self.records
    }
}

/// How a read `CBLC` size's index subtable records are searched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Records {
    /// No record starts inside an earlier one: a binary search over the firsts.
    Disjoint,
    /// Records overlap, but each also ends no earlier than the one before: a
    /// binary search over the lasts finds the first record that holds a glyph.
    Staircase,
    /// A record ends before one in front of it: the records are walked.
    Nested,
}

/// Why a `CBLC` size is not read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrikeRefusal {
    /// Its index subtable records do not lie wholly in the table -- a list
    /// that starts past the table's end, even an empty one, included.
    PastTable,
    /// Record `at` starts before the record listed ahead of it.
    Unsorted { at: Index },
}

/// What a strike is: read, with its search, or refused, with the reason.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrikeState {
    Read(ReadStrike),
    Refused(StrikeRefusal),
}

/// Records read through `'dupe'` and `'flip'` together, the image's own
/// included, before a glyph is refused: ten records is nine hops. A chain
/// can loop.
const DUPE_DEPTH: u8 = 10;

/// The most strikes a table may claim before it is refused whole. Every
/// question about a glyph walks the strikes -- the atlas per glyph and size,
/// [`crate::Face::is_colour`] per codepoint of a face-table block -- and a
/// count is the font's to state: a 4.2 MB `sbix` of 1,048,576 offsets that
/// fit it made each face-table block fill take about 0.65 s. The colour
/// faces measured carry one (Noto Color Emoji) and nine (Apple Color Emoji),
/// and no colour font on the development Mac carries more. 255 is Jev's pick
/// (0.38 at confidence 0.08, against 64 at 0.35 and no bound at 0.27): a
/// recommendation, not settled.
const MAX_STRIKES: u32 = 255;

/// `sbix`: a version, flags, and `numStrikes` offsets to strikes.
#[derive(Clone, Copy)]
pub struct Sbix<'a> {
    data: Bytes<'a>,
    strikes: Count,
    glyphs: Count,
}

impl<'a> Sbix<'a> {
    pub fn parse(data: Bytes<'a>, glyphs: Count) -> Option<Sbix<'a>> {
        let version: u16 = data.read(Offset::ZERO)?;
        if version != 1 {
            return None;
        }
        let n: u32 = data.read(Offset::of(4u32))?;
        // A count past `MAX_STRIKES`, or whose offsets run past the table,
        // is refused whole: every renderer walks the strikes per glyph.
        if n > MAX_STRIKES {
            return None;
        }
        let _: Array<'a, u32> = data.array(Offset::of(8u32), Count::of(n))?;
        Some(Sbix {
            data,
            strikes: Count::of(n),
            glyphs,
        })
    }

    pub fn strikes(&self) -> Count {
        self.strikes
    }

    fn strike_at(&self, i: Index) -> Option<Offset> {
        let offsets: Array<'a, u32> = self.data.array(Offset::of(8u32), self.strikes)?;
        Some(Offset::of(offsets.get(i)?))
    }

    /// The `ppem` of strike `i`.
    pub fn ppem(&self, i: Index) -> Option<Ppem> {
        let at = self.strike_at(i)?;
        self.data.read(at).map(Ppem)
    }

    /// Glyph `g`'s PNG in strike `i`, following `dupe` and `flip`
    /// redirections within the strike; `None` where the strike holds
    /// nothing for it.
    pub fn bitmap(&self, i: Index, g: GlyphId) -> Option<Bitmap<'a>> {
        let strike = self.strike_at(i)?;
        let ppem = Ppem(self.data.read(strike)?);
        let mut g = g;
        let mut mirror = Mirror::AsStored;
        let mut depth = 0u8;
        loop {
            if depth == DUPE_DEPTH {
                return None;
            }
            depth += 1;
            // glyphDataOffsets[numGlyphs + 1] follows ppem and ppi.
            let entries = Count::of(self.glyphs.value().checked_add(1)?);
            let offsets: Array<'a, u32> = self.data.array(strike.plus(Len::of(4))?, entries)?;
            let gi = Index::of(u32::try_from(g.index()).ok()?);
            let start = offsets.get(gi)?;
            let end = offsets.get(gi.next())?;
            if start == end {
                return None;
            }
            let len = end.checked_sub(start)?.checked_sub(8)?;
            let rec = strike.plus(Len::of(start))?;
            let x: i16 = self.data.read(rec)?;
            let y: i16 = self.data.read(rec.plus(Len::of(2))?)?;
            let kind: u32 = self.data.read(rec.plus(Len::of(4))?)?;
            let payload = self.data.slice(rec.plus(Len::of(8))?, Len::of(len))?;
            match &kind.to_be_bytes() {
                b"png " => {
                    return Some(Bitmap {
                        png: payload,
                        placement: Placement { x, y },
                        ppem,
                        strike: Strike::Sbix,
                        mirror,
                    });
                }
                b"dupe" => {
                    let target: u16 = payload.read(Offset::ZERO)?;
                    g = GlyphId::from_shaper(target);
                }
                b"flip" => {
                    let target: u16 = payload.read(Offset::ZERO)?;
                    g = GlyphId::from_shaper(target);
                    mirror = Mirror::LeftRight;
                }
                _ => return None,
            }
        }
    }
}

/// How a `CBLC` index subtable locates its images: `indexFormat` 1 to 5.
#[derive(Clone, Copy)]
enum IndexFormat {
    /// 1: one `u32` offset per glyph, plus a sentinel.
    Offsets32,
    /// 2: every glyph the same size, metrics shared, dense.
    Constant,
    /// 3: as 1 with `u16` offsets.
    Offsets16,
    /// 4: `(glyph, offset)` pairs for the glyphs present, plus a sentinel.
    Sparse,
    /// 5: as 2, over a sorted list of the glyphs present.
    SparseConstant,
}

impl IndexFormat {
    fn parse(v: u16) -> Option<IndexFormat> {
        match v {
            1 => Some(IndexFormat::Offsets32),
            2 => Some(IndexFormat::Constant),
            3 => Some(IndexFormat::Offsets16),
            4 => Some(IndexFormat::Sparse),
            5 => Some(IndexFormat::SparseConstant),
            _ => None,
        }
    }
}

/// How a `CBDT` image is laid out: the three PNG formats. The mono formats
/// 1 to 9 are `EBDT`'s and answer `None` here.
#[derive(Clone, Copy)]
enum ImageFormat {
    /// 17: `smallGlyphMetrics`, a length, the PNG.
    Small,
    /// 18: `bigGlyphMetrics`, a length, the PNG.
    Big,
    /// 19: a length and the PNG; the metrics live in the index subtable.
    Shared,
}

impl ImageFormat {
    fn parse(v: u16) -> Option<ImageFormat> {
        match v {
            17 => Some(ImageFormat::Small),
            18 => Some(ImageFormat::Big),
            19 => Some(ImageFormat::Shared),
            _ => None,
        }
    }
}

/// A `CBLC` index subtable header: how glyphs are indexed and how the
/// images are laid out.
#[derive(Clone, Copy)]
struct IndexHeader {
    index_format: IndexFormat,
    image_format: ImageFormat,
    image_data: Offset,
}

/// `BigGlyphMetrics` (8 bytes): height, width, then the horizontal bearings
/// this reader keeps -- the PNG carries its own size, so height and width
/// are the image's and a renderer reads them from the IHDR.
#[derive(Clone, Copy)]
struct BigMetrics {
    bearing_x: i8,
    bearing_y: i8,
}

impl FromBe for BigMetrics {
    const SIZE: Len = Len::of(8);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(BigMetrics {
            bearing_x: i8::from_ne_bytes([u8::parse_be(b.get(2..)?)?]),
            bearing_y: i8::from_ne_bytes([u8::parse_be(b.get(3..)?)?]),
        })
    }
}

/// An index subtable record: its first and last glyph, and its subtable's
/// offset from the start of the record list.
#[derive(Clone, Copy)]
struct Range(u16, u16, u32);

impl FromBe for Range {
    const SIZE: Len = Len::of(8);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Range(
            u16::parse_be(b)?,
            u16::parse_be(b.get(2..)?)?,
            u32::parse_be(b.get(4..)?)?,
        ))
    }
}

/// A format-4 pair: a glyph and its image's offset.
#[derive(Clone, Copy)]
struct Pair(u16, u16);

impl FromBe for Pair {
    const SIZE: Len = Len::of(4);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Pair(u16::parse_be(b)?, u16::parse_be(b.get(2..)?)?))
    }
}

/// `CBLC` and `CBDT` together, and the face's glyph count, which bounds
/// every glyph a size can answer.
#[derive(Clone, Copy)]
pub struct Cbdt<'a> {
    cblc: Bytes<'a>,
    cbdt: Bytes<'a>,
    sizes: Count,
    glyphs: Count,
}

const SIZE_RECORD: Len = Len::of(48);

impl<'a> Cbdt<'a> {
    pub fn parse(cblc: Bytes<'a>, cbdt: Bytes<'a>, glyphs: Count) -> Option<Cbdt<'a>> {
        // The version is not checked: the reference readers skip it because
        // Noto Color Emoji shipped with 2.0 before the specification settled
        // on 3.0, and the layout is the same.
        let n: u32 = cblc.read(Offset::of(4u32))?;
        // As for `sbix`: too many sizes, or records past the table, refuse it.
        if n > MAX_STRIKES {
            return None;
        }
        cblc.slice(Offset::of(8u32), SIZE_RECORD.times(Count::of(n))?)?;
        Some(Cbdt {
            cblc,
            cbdt,
            sizes: Count::of(n),
            glyphs,
        })
    }

    pub fn strikes(&self) -> Count {
        self.sizes
    }

    fn size_at(&self, i: Index) -> Option<Offset> {
        if !self.sizes.holds(i) {
            return None;
        }
        Offset::of(8u32).at(i, SIZE_RECORD)
    }

    /// The horizontal `ppem` of strike `i`.
    pub fn ppem(&self, i: Index) -> Option<Ppem> {
        let at = self.size_at(i)?;
        let ppem: u8 = self.cblc.read(at.plus(Len::of(44))?)?;
        Some(Ppem(u16::from(ppem)))
    }

    /// Size `i`'s index subtable records, when every one lies in the table:
    /// where the record list starts, and the records.
    fn records(&self, size: Offset) -> Option<(Offset, Array<'a, Range>)> {
        let list = Offset::of(self.cblc.read::<u32>(size)?);
        let count: u32 = self.cblc.read(size.plus(Len::of(8))?)?;
        Some((list, self.cblc.array(list, Count::of(count))?))
    }

    /// Whether size `i` is read, and how its records are searched, or why it
    /// is refused: one pass over the records. Refused when they run past the
    /// table or a record starts before the one listed ahead of it; read
    /// otherwise, disjoint when every record starts after the last glyph of
    /// the one before, a staircase when each also ends no earlier, nested
    /// when one ends before a record in front of it.
    pub fn state(&self, i: Index) -> Option<StrikeState> {
        let size = self.size_at(i)?;
        let ppem = self.ppem(i)?;
        let Some((_, records)) = self.records(size) else {
            return Some(StrikeState::Refused(StrikeRefusal::PastTable));
        };
        let (mut disjoint, mut staircase) = (true, true);
        let mut before: Option<Range> = None;
        let mut j = Index::ZERO;
        while records.count().holds(j) {
            let r = records.get(j)?;
            if let Some(p) = before {
                if r.0 < p.0 {
                    return Some(StrikeState::Refused(StrikeRefusal::Unsorted { at: j }));
                }
                disjoint &= r.0 > p.1;
                staircase &= r.1 >= p.1;
            }
            before = Some(r);
            j = j.next();
        }
        let records = if disjoint {
            Records::Disjoint
        } else if staircase {
            Records::Staircase
        } else {
            Records::Nested
        };
        Some(StrikeState::Read(ReadStrike::new(i, ppem, records)))
    }

    /// Glyph `g`'s PNG in a read size: below the face's glyph count, inside
    /// the size's start..end range, from the first record that holds it. That
    /// record is the last of those starting at or before `g` when no record
    /// starts inside another; on a staircase, the first ending at or after
    /// `g`; and on a nested size, found by walking the records that start at
    /// or before `g`.
    pub fn bitmap_in(&self, s: ReadStrike, g: GlyphId) -> Option<Bitmap<'a>> {
        let size = self.size_at(s.index())?;
        let gid = u16::try_from(g.index()).ok()?;
        if !self.glyphs.holds(Index::of(gid)) {
            return None;
        }
        let first: u16 = self.cblc.read(size.plus(Len::of(40))?)?;
        let last: u16 = self.cblc.read(size.plus(Len::of(42))?)?;
        if !(first..=last).contains(&gid) {
            return None;
        }
        let (list, records) = self.records(size)?;
        // How many records start at or before the glyph: the disjoint and the
        // nested search stop there.
        let starting = || {
            records.partition_point(|r: &Range| {
                if r.0 <= gid {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            })
        };
        let r = match s.records() {
            Records::Disjoint => records.get(starting()?.prev()?).filter(|r| gid <= r.1)?,
            Records::Staircase => {
                let ends = |r: &Range| {
                    if r.1 < gid {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                };
                records
                    .get(records.partition_point(ends)?)
                    .filter(|r| r.0 <= gid)?
            }
            Records::Nested => {
                let k = starting()?;
                let mut j = Index::ZERO;
                loop {
                    if j >= k {
                        return None;
                    }
                    let r = records.get(j)?;
                    if r.0 <= gid && gid <= r.1 {
                        break r;
                    }
                    j = j.next();
                }
            }
        };
        self.image(s.ppem(), list, r, g)
    }

    /// The image record `r` locates for glyph `g`, with its metrics from
    /// wherever the image format keeps them.
    fn image(&self, ppem: Ppem, list: Offset, r: Range, g: GlyphId) -> Option<Bitmap<'a>> {
        let gid = u16::try_from(g.index()).ok()?;
        let (sub_first, header_at) = (r.0, list.plus(Len::of(r.2))?);
        let header = IndexHeader {
            index_format: IndexFormat::parse(self.cblc.read(header_at)?)?,
            image_format: ImageFormat::parse(self.cblc.read(header_at.plus(Len::of(2))?)?)?,
            image_data: Offset::of(self.cblc.read::<u32>(header_at.plus(Len::of(4))?)?),
        };
        let body = header_at.plus(Len::of(8))?;
        let diff = Index::of(gid.checked_sub(sub_first)?);
        let mut shared: Option<BigMetrics> = None;
        let image = match header.index_format {
            IndexFormat::Offsets32 => {
                let off: u32 = self.cblc.read(body.at(diff, Len::of(4))?)?;
                header.image_data.plus(Len::of(off))?
            }
            IndexFormat::Constant => {
                let image_size: u32 = self.cblc.read(body)?;
                shared = Some(self.cblc.read(body.plus(Len::of(4))?)?);
                header
                    .image_data
                    .plus(Len::of(image_size.checked_mul(diff.value())?))?
            }
            IndexFormat::Offsets16 => {
                let off: u16 = self.cblc.read(body.at(diff, Len::of(2))?)?;
                header.image_data.plus(Len::of(u32::from(off)))?
            }
            IndexFormat::Sparse => {
                // The n + 1 pairs must lie in the table, but only the first n
                // name glyphs: the last marks where the last image ends. The
                // pairs are in glyph order, so a glyph is where a binary search
                // puts it, or absent.
                let n: u32 = self.cblc.read(body)?;
                let pairs_at = body.plus(Len::of(4))?;
                let _: Array<'a, Pair> = self.cblc.array(pairs_at, Count::of(n.checked_add(1)?))?;
                let pairs: Array<'a, Pair> = self.cblc.array(pairs_at, Count::of(n))?;
                let p = pairs.get(pairs.partition_point(|p| p.0.cmp(&gid))?)?;
                if p.0 != gid {
                    return None;
                }
                header.image_data.plus(Len::of(u32::from(p.1)))?
            }
            IndexFormat::SparseConstant => {
                let image_size: u32 = self.cblc.read(body)?;
                shared = Some(self.cblc.read(body.plus(Len::of(4))?)?);
                let n: u32 = self.cblc.read(body.plus(Len::of(12))?)?;
                let ids: Array<'a, u16> = self.cblc.array(body.plus(Len::of(16))?, Count::of(n))?;
                let k = ids.partition_point(|id| id.cmp(&gid))?;
                if ids.get(k)? != gid {
                    return None;
                }
                header
                    .image_data
                    .plus(Len::of(image_size.checked_mul(k.value())?))?
            }
        };
        let (metrics_len, metrics): (u32, BigMetrics) = match header.image_format {
            ImageFormat::Small => {
                // smallGlyphMetrics: height, width, bearingX, bearingY, advance.
                let m = BigMetrics {
                    bearing_x: i8::from_ne_bytes([self.cbdt.read(image.plus(Len::of(2))?)?]),
                    bearing_y: i8::from_ne_bytes([self.cbdt.read(image.plus(Len::of(3))?)?]),
                };
                (5, m)
            }
            ImageFormat::Big => (8, self.cbdt.read(image)?),
            ImageFormat::Shared => (0, shared?),
        };
        let len_at = image.plus(Len::of(metrics_len))?;
        let len: u32 = self.cbdt.read(len_at)?;
        let png = self.cbdt.slice(len_at.plus(Len::of(4))?, Len::of(len))?;
        Some(Bitmap {
            png,
            placement: Placement {
                x: i16::from(metrics.bearing_x),
                y: i16::from(metrics.bearing_y),
            },
            ppem,
            strike: Strike::Cbdt,
            mirror: Mirror::AsStored,
        })
    }
}
