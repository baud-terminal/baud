//! `CFF`: Type 2 charstrings (`[[source: adobe-5176-cff]]` for the container,
//! `[[source: adobe-5177-type2]]` for the charstring language,
//! `[[source: ms-opentype-cff]]` for its place in an OpenType file).
//!
//! **What is read.** The header, the Top DICT (charset, CharStrings,
//! Private, and for CID-keyed fonts ROS, FDArray and FDSelect), the Private
//! DICT's local subroutines, the global subroutines, the charset only as far
//! as `seac` needs it, and the charstrings themselves. Sixty-odd of the faces
//! on this Mac are CFF -- the Noto Indic fallbacks, STIX, LastResort -- and
//! JetBrains Mono's OTF cousin is one, so this is not an optional path.
//!
//! **The interpreter is the specification's, with the specification's
//! limits** (5177 Appendix B): an argument stack of 48, subroutine nesting
//! of 10, and the transient array unused because no shipping font uses it.
//! Hints are counted and discarded; `hintmask`'s data bytes are skipped by
//! the stem count, which is why the count is kept. The glyph width, which
//! rides in the first stack slot of the first stack-clearing operator, is
//! recognised so the argument counts come out right, and then ignored --
//! `hmtx` is the advance.
//!
//! **The emission conventions are the oracle's**, so the differential can be
//! exact: a contour is closed by the next `moveto` and by `endchar`, never
//! implicitly; every curve is a cubic; a flex reaches the sink as its two
//! cubics with its depth and threshold ([`crate::outline::Flex`]), which a
//! sink with no device draws as the two cubics the oracle emits and the
//! rasteriser draws straight when the text says so; the arithmetic is `f32`
//! in the order the reference performs it.
//!
//! **`seac`** -- `endchar` with four or five arguments -- composes a base and
//! an accent named by Standard Encoding codes, which the generated
//! `standard_encoding` table maps to SIDs and the charset maps to glyphs.
//!
//! **What `CFF2` shares with it** (`crate::cff2`): the INDEX, the DICT
//! scan, FDSelect, the subroutine bias, the argument stack and the path
//! operators, which [`Pen`] holds over either number type -- `f32` here,
//! where the oracle computes in `f32`, and `f64` there.

use crate::bytes::{Bytes, Count, Index, Len, Offset, U24};
use crate::outline::{Flex, Sink};
use crate::standard_encoding::STANDARD_ENCODING;
use baud_domain::GlyphId;
use core::ops::{Add, Div, Mul, Neg, Sub};

const MAX_ARGS: usize = 48;
/// The threshold of `hflex`, `hflex1` and `flex1` in device pixels: "0.5
/// (that is, fd is 50) device pixels" (`[[source: adobe-5177-type2]]`), "threshold
/// fd = 50 is implicit" (`[[source: ms-opentype-cff2]]`).
const IMPLICIT_FLEX_THRESHOLD: f32 = 0.5;
const MAX_DEPTH: u8 = 10;
const TWO_BYTE: u8 = 12;

mod op {
    pub const HSTEM: u8 = 1;
    pub const VSTEM: u8 = 3;
    pub const VMOVETO: u8 = 4;
    pub const RLINETO: u8 = 5;
    pub const HLINETO: u8 = 6;
    pub const VLINETO: u8 = 7;
    pub const RRCURVETO: u8 = 8;
    pub const CALLSUBR: u8 = 10;
    pub const RETURN: u8 = 11;
    pub const ENDCHAR: u8 = 14;
    pub const HSTEMHM: u8 = 18;
    pub const HINTMASK: u8 = 19;
    pub const CNTRMASK: u8 = 20;
    pub const RMOVETO: u8 = 21;
    pub const HMOVETO: u8 = 22;
    pub const VSTEMHM: u8 = 23;
    pub const RCURVELINE: u8 = 24;
    pub const RLINECURVE: u8 = 25;
    pub const VVCURVETO: u8 = 26;
    pub const HHCURVETO: u8 = 27;
    pub const SHORTINT: u8 = 28;
    pub const CALLGSUBR: u8 = 29;
    pub const VHCURVETO: u8 = 30;
    pub const HVCURVETO: u8 = 31;
    pub const HFLEX: u8 = 34;
    pub const FLEX: u8 = 35;
    pub const HFLEX1: u8 = 36;
    pub const FLEX1: u8 = 37;
    pub const FIXED: u8 = 255;
}

mod dict {
    use super::DictOp;
    pub const CHARSET: DictOp = DictOp(15);
    pub const CHAR_STRINGS: DictOp = DictOp(17);
    pub const PRIVATE: DictOp = DictOp(18);
    pub const SUBRS: DictOp = DictOp(19);
    pub const ROS: DictOp = DictOp(1230);
    pub const FD_ARRAY: DictOp = DictOp(1236);
    pub const FD_SELECT: DictOp = DictOp(1237);
}

/// The width of an INDEX's offsets.
#[derive(Clone, Copy)]
enum OffSize {
    One,
    Two,
    Three,
    Four,
}

impl OffSize {
    fn parse(b: u8) -> Option<OffSize> {
        match b {
            1 => Some(OffSize::One),
            2 => Some(OffSize::Two),
            3 => Some(OffSize::Three),
            4 => Some(OffSize::Four),
            _ => None,
        }
    }
    fn len(self) -> Len {
        match self {
            OffSize::One => Len::of(1),
            OffSize::Two => Len::of(2),
            OffSize::Three => Len::of(3),
            OffSize::Four => Len::of(4),
        }
    }
}

/// A CFF INDEX: `count` objects, `count + 1` one-based offsets, the data.
#[derive(Clone, Copy)]
pub(crate) struct CffIndex<'a> {
    offsets: Bytes<'a>,
    size: OffSize,
    count: Count,
    data: Bytes<'a>,
}

impl<'a> CffIndex<'a> {
    pub(crate) const EMPTY: CffIndex<'static> = CffIndex {
        offsets: Bytes::new(&[]),
        size: OffSize::One,
        count: Count::ZERO,
        data: Bytes::new(&[]),
    };

    /// The CFF INDEX at `at`, its count a `uint16`, and the offset just
    /// past it.
    fn parse(b: Bytes<'a>, at: Offset) -> Option<(CffIndex<'a>, Offset)> {
        let count: u16 = b.read(at)?;
        CffIndex::counted(b, Count::of(count), at.plus(Len::of(2))?)
    }

    /// The CFF2 INDEX at `at`: the same layout with its count a `uint32`,
    /// so an empty one is four bytes (`[[source: ms-opentype-cff2]]`, "INDEX
    /// data").
    pub(crate) fn parse_cff2(b: Bytes<'a>, at: Offset) -> Option<(CffIndex<'a>, Offset)> {
        let count: u32 = b.read(at)?;
        CffIndex::counted(b, Count::of(count), at.plus(Len::of(4))?)
    }

    /// The rest of an INDEX of `count` objects whose count field ends at
    /// `after_count`.
    fn counted(b: Bytes<'a>, count: Count, after_count: Offset) -> Option<(CffIndex<'a>, Offset)> {
        if count.is_zero() {
            return Some((CffIndex::EMPTY, after_count));
        }
        let size = OffSize::parse(b.read(after_count)?)?;
        let offsets_at = after_count.plus(Len::of(1))?;
        let entries = count.plus(Count::of(1u32))?;
        let offsets_len = size.len().times(entries)?;
        let offsets = b.slice(offsets_at, offsets_len)?;
        let data_at = offsets_at.plus(offsets_len)?;
        let index = CffIndex {
            offsets,
            size,
            count,
            data: Bytes::new(&[]),
        };
        let last = index.offset(count.as_index())?;
        let data = b.slice(data_at, last)?;
        let end = data_at.plus(last)?;
        Some((CffIndex { data, ..index }, end))
    }

    pub(crate) fn len(&self) -> Count {
        self.count
    }

    /// Offset `i`, zero-based (the file stores them one-based).
    fn offset(&self, i: Index) -> Option<Len> {
        let at = Offset::ZERO.at(i, self.size.len())?;
        let n: u32 = match self.size {
            OffSize::One => u32::from(self.offsets.read::<u8>(at)?),
            OffSize::Two => u32::from(self.offsets.read::<u16>(at)?),
            OffSize::Three => self.offsets.read::<U24>(at)?.0,
            OffSize::Four => self.offsets.read::<u32>(at)?,
        };
        n.checked_sub(1).map(Len::of)
    }

    pub(crate) fn get(&self, i: Index) -> Option<Bytes<'a>> {
        if !self.count.holds(i) {
            return None;
        }
        let start = self.offset(i)?;
        let end = self.offset(i.next())?;
        let len = end.value().checked_sub(start.value())?;
        self.data.slice(Offset::of(start.value()), Len::of(len))
    }
}

/// The operands of one DICT entry, as the reference stores them: `f64`,
/// because a 32-bit integer operand does not fit an `f32`. The first four
/// are kept -- no key either reader looks up takes more than two -- and the
/// rest only counted, against the dialect's limit.
pub(crate) struct Operands {
    vals: [f64; KEPT],
    n: Count,
}

/// How many of a DICT entry's operands are kept.
const KEPT: usize = 4;

impl Operands {
    /// How many operands preceded the operator, kept or only counted.
    pub(crate) fn len(&self) -> Count {
        self.n
    }

    pub(crate) fn get(&self, i: Index) -> Option<f64> {
        if !self.n.holds(i) {
            return None;
        }
        self.vals.get(i.slot()).copied()
    }
}

/// How many operands may precede one DICT operator.
#[derive(Clone, Copy)]
pub(crate) enum DictLimit {
    /// 48, the Type 2 argument stack (5177 Appendix B).
    Cff,
    /// 513, the CFF2 stack: a `blend` before a key leaves `n + n * k + 1`
    /// operands on it (`[[source: ms-opentype-cff2]]`, "Stack-based CFF2
    /// decoding").
    Cff2,
}

impl DictLimit {
    fn count(self) -> Count {
        match self {
            DictLimit::Cff => Count::of(48u32),
            DictLimit::Cff2 => Count::of(513u32),
        }
    }
}

/// A DICT operator: one byte, or `12` and a second byte, spelled as
/// `1200` plus the second.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct DictOp(pub(crate) u16);

/// What a DICT scan for one operator found.
pub(crate) enum DictScan {
    /// The operator, with the operands before it.
    Found(Operands),
    /// The DICT read to its end without the operator.
    Absent,
    /// A number that does not decode, a truncated operator, or more
    /// operands before one operator than the dialect allows.
    Malformed,
}

/// Scans a CFF DICT for operator `want` and returns its operands: `None`
/// when it is absent or the DICT is malformed.
fn dict_operands(data: Bytes<'_>, want: DictOp) -> Option<Operands> {
    match dict_scan(data, want, DictLimit::Cff) {
        DictScan::Found(ops) => Some(ops),
        DictScan::Absent | DictScan::Malformed => None,
    }
}

/// Scans a DICT for operator `want` under `limit`.
pub(crate) fn dict_scan(data: Bytes<'_>, want: DictOp, limit: DictLimit) -> DictScan {
    match dict_walk(data, want, limit) {
        Some(Some(ops)) => DictScan::Found(ops),
        Some(None) => DictScan::Absent,
        None => DictScan::Malformed,
    }
}

/// [`dict_scan`] as an `Option` of an `Option`: `None` for a malformed
/// DICT, `Some(None)` for an absent operator.
fn dict_walk(data: Bytes<'_>, want: DictOp, limit: DictLimit) -> Option<Option<Operands>> {
    let mut ops = Operands {
        vals: [0.0; KEPT],
        n: Count::ZERO,
    };
    let mut at = Offset::ZERO;
    while let Some(b) = data.read::<u8>(at) {
        at = at.plus(Len::of(1))?;
        let is_operator = matches!(b, 0..=27 | 31 | 255);
        if is_operator {
            let operator = if b == TWO_BYTE {
                let b2: u8 = data.read(at)?;
                at = at.plus(Len::of(1))?;
                DictOp(1200 + u16::from(b2))
            } else {
                DictOp(u16::from(b))
            };
            if operator == want {
                return Some(Some(ops));
            }
            ops.n = Count::ZERO;
            continue;
        }
        let (v, next) = dict_number(data, b, at)?;
        at = next;
        if !limit.count().holds(ops.n.as_index()) {
            return None;
        }
        if let Some(slot) = ops.vals.get_mut(ops.n.as_index().slot()) {
            *slot = v;
        }
        ops.n = ops.n.plus(Count::of(1u32))?;
    }
    Some(None)
}

/// One DICT operand starting with byte `b0` (5177 Table 3, 5176 Table 5).
fn dict_number(data: Bytes<'_>, b0: u8, at: Offset) -> Option<(f64, Offset)> {
    match b0 {
        28 => {
            let n: i16 = data.read(at)?;
            Some((f64::from(n), at.plus(Len::of(2))?))
        }
        29 => {
            let n: u32 = data.read(at)?;
            Some((
                f64::from(i32::from_ne_bytes(n.to_ne_bytes())),
                at.plus(Len::of(4))?,
            ))
        }
        30 => dict_real(data, at),
        32..=246 => Some((f64::from(i32::from(b0) - 139), at)),
        247..=250 => {
            let b1: u8 = data.read(at)?;
            Some((
                f64::from((i32::from(b0) - 247) * 256 + i32::from(b1) + 108),
                at.plus(Len::of(1))?,
            ))
        }
        251..=254 => {
            let b1: u8 = data.read(at)?;
            Some((
                f64::from(-(i32::from(b0) - 251) * 256 - i32::from(b1) - 108),
                at.plus(Len::of(1))?,
            ))
        }
        _ => None,
    }
}

/// A real number as nibbles: digits, `.`, `E`, `E-`, `-`, terminated by `f`.
fn dict_real(data: Bytes<'_>, mut at: Offset) -> Option<(f64, Offset)> {
    let mut buf = [0u8; 64];
    let mut n = 0usize;
    'outer: loop {
        let b: u8 = data.read(at)?;
        at = at.plus(Len::of(1))?;
        for nibble in [b >> 4, b & 15] {
            if nibble == 0xf {
                break 'outer;
            }
            let piece: &[u8] = match nibble {
                0..=9 => &[b'0' + nibble],
                0xa => b".",
                0xb => b"E",
                0xc => b"E-",
                0xe => b"-",
                _ => return None,
            };
            if n + piece.len() > buf.len() {
                return None;
            }
            buf[n..n + piece.len()].copy_from_slice(piece);
            n += piece.len();
        }
    }
    let s = core::str::from_utf8(&buf[..n]).ok()?;
    Some((s.parse().ok()?, at))
}

/// The charset, only as far as `seac` needs it: a string id to a glyph.
#[derive(Clone, Copy)]
enum Charset<'a> {
    IsoAdobe,
    Expert,
    ExpertSubset,
    /// Format 0: one SID per glyph after `.notdef`.
    Format0(Bytes<'a>, Count),
    /// Format 1: ranges of `(first SID, u8 more)`.
    Format1(Bytes<'a>, Count),
    /// Format 2: ranges of `(first SID, u16 more)`.
    Format2(Bytes<'a>, Count),
}

impl<'a> Charset<'a> {
    fn parse(b: Bytes<'a>, at: Offset, glyphs: Count) -> Option<Charset<'a>> {
        let format: u8 = b.read(at)?;
        let body = b.tail(at.plus(Len::of(1))?)?;
        // `.notdef` is omitted, so one fewer than the glyph count.
        let mut left = glyphs.value().checked_sub(1)?;
        match format {
            0 => Some(Charset::Format0(body, Count::of(left))),
            1 | 2 => {
                let size = if format == 1 { Len::of(3) } else { Len::of(4) };
                let mut ranges = 0u32;
                let mut at = Offset::ZERO;
                while left > 0 {
                    let more = if format == 1 {
                        u32::from(body.read::<u8>(at.plus(Len::of(2))?)?)
                    } else {
                        u32::from(body.read::<u16>(at.plus(Len::of(2))?)?)
                    };
                    left = left.checked_sub(more.checked_add(1)?)?;
                    ranges = ranges.checked_add(1)?;
                    at = at.plus(size)?;
                }
                let data = body.slice(Offset::ZERO, size.times(Count::of(ranges))?)?;
                Some(if format == 1 {
                    Charset::Format1(data, Count::of(ranges))
                } else {
                    Charset::Format2(data, Count::of(ranges))
                })
            }
            _ => None,
        }
    }

    fn sid_to_gid(&self, sid: u16) -> Option<GlyphId> {
        if sid == 0 {
            return Some(GlyphId::NOTDEF);
        }
        match *self {
            Charset::IsoAdobe | Charset::Expert | Charset::ExpertSubset => None,
            Charset::Format0(data, count) => {
                let mut i = Index::ZERO;
                while count.holds(i) {
                    let s: u16 = data.read(Offset::ZERO.at(i, Len::of(2))?)?;
                    if s == sid {
                        return Some(GlyphId::from_shaper(
                            u16::try_from(i.value().checked_add(1)?).ok()?,
                        ));
                    }
                    i = i.next();
                }
                None
            }
            Charset::Format1(data, count) | Charset::Format2(data, count) => {
                let wide = matches!(self, Charset::Format2(..));
                let size = if wide { Len::of(4) } else { Len::of(3) };
                let mut gid: u16 = 1;
                let mut i = Index::ZERO;
                while count.holds(i) {
                    let at = Offset::ZERO.at(i, size)?;
                    let first: u16 = data.read(at)?;
                    let more: u16 = if wide {
                        data.read(at.plus(Len::of(2))?)?
                    } else {
                        u16::from(data.read::<u8>(at.plus(Len::of(2))?)?)
                    };
                    let last = u32::from(first) + u32::from(more);
                    if first <= sid && u32::from(sid) <= last {
                        return Some(GlyphId::from_shaper(gid.wrapping_add(sid - first)));
                    }
                    gid = gid.wrapping_add(more).wrapping_add(1);
                    i = i.next();
                }
                None
            }
        }
    }
}

/// CID-keyed fonts and CFF2: which Font DICT a glyph uses. Formats 0 and 3
/// are CFF's (5176 section 19); format 4, `uint32` ranges naming `uint16`
/// Font DICTs, is CFF2's (`[[source: ms-opentype-cff2]]`, "FontDICTSelect").
#[derive(Clone, Copy)]
pub(crate) enum FdSelect<'a> {
    Format0(Bytes<'a>),
    Format3(Bytes<'a>),
    Format4(Bytes<'a>),
}

/// Which FDSelect formats a dialect reads: CFF knows 0 and 3, CFF2 adds 4.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FdFormats {
    Cff,
    Cff2,
}

impl FdSelect<'_> {
    pub(crate) fn parse(b: Bytes<'_>, at: Offset, formats: FdFormats) -> Option<FdSelect<'_>> {
        let format: u8 = b.read(at)?;
        let body = b.tail(at.plus(Len::of(1))?)?;
        match format {
            0 => Some(FdSelect::Format0(body)),
            3 => Some(FdSelect::Format3(body)),
            4 if formats == FdFormats::Cff2 => Some(FdSelect::Format4(body)),
            _ => None,
        }
    }

    /// `Some` when every one of `glyphs` maps to one of `fds` Font DICTs,
    /// as `[[source: ms-opentype-cff2]]`, "FontDICTSelect", requires:
    /// format 0 holds an entry a glyph; formats 3 and 4 start at glyph 0,
    /// ascend, and end at a sentinel equal to the glyph count.
    pub(crate) fn covers(&self, glyphs: Count, fds: Count) -> Option<()> {
        let names = |fd: u32| fds.holds(Index::of(fd)).then_some(());
        match *self {
            FdSelect::Format0(data) => {
                let mut g = Index::ZERO;
                while glyphs.holds(g) {
                    names(u32::from(data.read::<u8>(Offset::of(g.value()))?))?;
                    g = g.next();
                }
                Some(())
            }
            FdSelect::Format3(data) => {
                let ranges = u32::from(data.read::<u16>(Offset::ZERO)?);
                let first = |i: u32| -> Option<u32> {
                    data.read::<u16>(Offset::of(2 + 3 * i)).map(u32::from)
                };
                let mut prev = None;
                for i in 0..ranges {
                    let f = first(i)?;
                    if prev.map_or(f != 0, |p| f <= p) {
                        return None;
                    }
                    names(u32::from(data.read::<u8>(Offset::of(2 + 3 * i + 2))?))?;
                    prev = Some(f);
                }
                (ranges > 0 && first(ranges)? == glyphs.value() && prev? < glyphs.value())
                    .then_some(())
            }
            FdSelect::Format4(data) => {
                let ranges = data.read::<u32>(Offset::ZERO)?;
                let first = |i: u32| -> Option<u32> {
                    data.read::<u32>(Offset::of(4u32.checked_add(i.checked_mul(6)?)?))
                };
                let mut prev = None;
                for i in 0..ranges {
                    let f = first(i)?;
                    if prev.map_or(f != 0, |p| f <= p) {
                        return None;
                    }
                    let at = 4u32.checked_add(i.checked_mul(6)?)?.checked_add(4)?;
                    names(u32::from(data.read::<u16>(Offset::of(at))?))?;
                    prev = Some(f);
                }
                (ranges > 0 && first(ranges)? == glyphs.value() && prev? < glyphs.value())
                    .then_some(())
            }
        }
    }

    /// The Font DICT glyph `g` uses, or `None` past the ranges.
    pub(crate) fn index_of(&self, g: GlyphId) -> Option<Index> {
        match *self {
            FdSelect::Format0(data) => data
                .read::<u8>(Offset::of(u32::try_from(g.index()).ok()?))
                .map(Index::of),
            FdSelect::Format4(data) => {
                let ranges = Count::of(data.read::<u32>(Offset::ZERO)?);
                let gid = u32::try_from(g.index()).ok()?;
                // Records of (first glyph u32, fd u16), then a sentinel
                // glyph: range i runs from its first glyph to the next's.
                let record = |i: Index| -> Option<(u32, u16)> {
                    let at = Offset::of(4u32).at(i, Len::of(6))?;
                    Some((data.read(at)?, data.read(at.plus(Len::of(4))?)?))
                };
                let mut i = Index::ZERO;
                while ranges.holds(i) {
                    let (first, fd) = record(i)?;
                    let end: u32 = data.read(Offset::of(4u32).at(i.next(), Len::of(6))?)?;
                    if (first..end).contains(&gid) {
                        return Some(Index::of(fd));
                    }
                    i = i.next();
                }
                None
            }
            FdSelect::Format3(data) => {
                let ranges: u16 = data.read(Offset::ZERO)?;
                if ranges == 0 {
                    return None;
                }
                let gid = u16::try_from(g.index()).ok()?;
                // Records of (first glyph u16, fd u8), then a sentinel glyph.
                let mut at = Offset::of(2u32);
                let mut prev_first: u16 = data.read(at)?;
                let mut prev_fd: u8 = data.read(at.plus(Len::of(2))?)?;
                at = at.plus(Len::of(3))?;
                for _ in 1..=ranges {
                    let first: u16 = data.read(at)?;
                    if (prev_first..first).contains(&gid) {
                        return Some(Index::of(prev_fd));
                    }
                    prev_fd = data.read(at.plus(Len::of(2))?).unwrap_or(prev_fd);
                    prev_first = first;
                    at = at.plus(Len::of(3))?;
                }
                None
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    Sid {
        local_subrs: CffIndex<'a>,
    },
    Cid {
        fd_array: CffIndex<'a>,
        fd_select: FdSelect<'a>,
    },
}

#[derive(Clone, Copy)]
pub struct Cff<'a> {
    data: Bytes<'a>,
    global_subrs: CffIndex<'a>,
    char_strings: CffIndex<'a>,
    charset: Charset<'a>,
    kind: Kind<'a>,
}

/// The Private DICT's local subroutines: `Subrs` is relative to the Private
/// DICT's own start.
fn local_subrs<'a>(data: Bytes<'a>, private: &Operands) -> Option<CffIndex<'a>> {
    let size = usize_of(private.get(Index::ZERO)?)?;
    let start = usize_of(private.get(Index::of(1u32))?)?;
    let pd = data.slice(Offset::of(start), Len::of(size))?;
    let subrs = dict_operands(pd, dict::SUBRS)?;
    let rel = usize_of(subrs.get(Index::ZERO)?)?;
    let at = Offset::of(start.checked_add(rel)?);
    CffIndex::parse(data, at).map(|(i, _)| i)
}

/// An operand as an offset or size: an integer in `u32` range.
pub(crate) fn usize_of(v: f64) -> Option<u32> {
    let i = v as i32;
    if f64::from(i) != v {
        return None;
    }
    u32::try_from(i).ok()
}

impl<'a> Cff<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Cff<'a>> {
        let major: u8 = data.read(Offset::ZERO)?;
        if major != 1 {
            return None;
        }
        let header_size: u8 = data.read(Offset::of(2u32))?;
        let at = Offset::of(u32::from(header_size));
        let (_names, at) = CffIndex::parse(data, at)?;
        let (top_index, at) = CffIndex::parse(data, at)?;
        let top = top_index.get(Index::ZERO)?;
        let (_strings, at) = CffIndex::parse(data, at)?;
        let (global_subrs, _) = CffIndex::parse(data, at)?;

        let cs_off = usize_of(dict_operands(top, dict::CHAR_STRINGS)?.get(Index::ZERO)?)?;
        if cs_off == 0 {
            return None;
        }
        let (char_strings, _) = CffIndex::parse(data, Offset::of(cs_off))?;
        let glyphs = char_strings.len();
        if glyphs.is_zero() || u16::try_from(glyphs.value()).is_err() {
            return None;
        }

        let charset = match dict_operands(top, dict::CHARSET).and_then(|o| o.get(Index::ZERO)) {
            None => Charset::IsoAdobe,
            Some(v) => match usize_of(v)? {
                0 => Charset::IsoAdobe,
                1 => Charset::Expert,
                2 => Charset::ExpertSubset,
                off => Charset::parse(data, Offset::of(off), glyphs)?,
            },
        };

        let kind = if dict_operands(top, dict::ROS).is_some() {
            let fda = usize_of(dict_operands(top, dict::FD_ARRAY)?.get(Index::ZERO)?)?;
            let fds = usize_of(dict_operands(top, dict::FD_SELECT)?.get(Index::ZERO)?)?;
            if matches!(
                charset,
                Charset::IsoAdobe | Charset::Expert | Charset::ExpertSubset
            ) {
                // "There are no predefined charsets for CID fonts."
                return None;
            }
            let (fd_array, _) = CffIndex::parse(data, Offset::of(fda))?;
            let fd_select = FdSelect::parse(data, Offset::of(fds), FdFormats::Cff)?;
            Kind::Cid {
                fd_array,
                fd_select,
            }
        } else {
            let local = dict_operands(top, dict::PRIVATE)
                .and_then(|p| local_subrs(data, &p))
                .unwrap_or(CffIndex::EMPTY);
            Kind::Sid { local_subrs: local }
        };

        Some(Cff {
            data,
            global_subrs,
            char_strings,
            charset,
            kind,
        })
    }

    /// The local subroutines for `g` in a CID font: FDSelect names the Font
    /// DICT, whose Private DICT names the Subrs.
    fn cid_local_subrs(&self, g: GlyphId) -> Option<CffIndex<'a>> {
        let Kind::Cid {
            fd_array,
            fd_select,
        } = self.kind
        else {
            return None;
        };
        let fd = fd_select.index_of(g)?;
        let font_dict = fd_array.get(fd)?;
        let private = dict_operands(font_dict, dict::PRIVATE)?;
        local_subrs(self.data, &private)
    }

    /// Emits `g`. `Some(())` on a well-formed charstring, `None` on a
    /// missing glyph or a malformed one -- which the oracle also refuses.
    pub fn outline<S: Sink>(&self, g: GlyphId, sink: &mut S) -> Option<()> {
        let cs = self
            .char_strings
            .get(Index::of(u32::try_from(g.index()).ok()?))?;
        let local = match self.kind {
            Kind::Sid { local_subrs } => Some(local_subrs),
            Kind::Cid { .. } => None,
        };
        let mut run = Run {
            cff: self,
            pen: Pen::new(sink),
            stack: Stack::new(),
            width: Width::Pending,
            stems: 0,
            end: End::Running,
            glyph: g,
            local,
        };
        run.exec(cs, Depth::GLYPH)?;
        (run.end != End::Running).then_some(())
    }
}

pub(crate) fn bias(count: Count) -> i32 {
    let n = count.value();
    if n < 1240 {
        107
    } else if n < 33900 {
        1131
    } else {
        32768
    }
}

/// A subroutine operand plus its INDEX's bias: `None` unless the operand is
/// an integer and the sum is not negative.
pub(crate) fn subr_index(v: f64, bias: i32) -> Option<Index> {
    let i = v as i32;
    if f64::from(i) != v {
        return None;
    }
    Index::of(u32::try_from(i.checked_add(bias)?).ok()?).into()
}

/// How deep a charstring has called: 0 for the glyph's own, one more per
/// subroutine, and the specification's limit of 10 (5177 Appendix B; the
/// same 10 in `[[source: ms-opentype-cff2]]`, "Subroutines").
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Depth(u8);

impl Depth {
    pub(crate) const GLYPH: Depth = Depth(0);
    const LIMIT: Depth = Depth(MAX_DEPTH);

    /// One call deeper, or `None` at the limit.
    pub(crate) fn deeper(self) -> Option<Depth> {
        (self != Depth::LIMIT).then_some(Depth(self.0 + 1))
    }
}

/// A charstring operand and coordinate: `f32` for CFF, which its oracle
/// computes in, and `f64` for CFF2, which keeps the sixteen fractional bits
/// the variations overview recommends. Either reaches the [`Sink`] as `f32`.
pub(crate) trait Num:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    const ZERO: Self;
    fn to_sink(self) -> f32;
}

impl Num for f32 {
    const ZERO: f32 = 0.0;
    fn to_sink(self) -> f32 {
        self
    }
}

impl Num for f64 {
    const ZERO: f64 = 0.0;
    fn to_sink(self) -> f32 {
        self as f32
    }
}

fn abs<N: Num>(v: N) -> N {
    if v < N::ZERO { -v } else { v }
}

/// The argument stack: 48 deep for CFF, 513 for CFF2.
pub(crate) struct Stack<N, const CAP: usize> {
    vals: [N; CAP],
    n: usize,
}

impl<N: Num, const CAP: usize> Stack<N, CAP> {
    pub(crate) fn new() -> Stack<N, CAP> {
        Stack {
            vals: [N::ZERO; CAP],
            n: 0,
        }
    }
    pub(crate) fn args(&self) -> &[N] {
        &self.vals[..self.n]
    }
    pub(crate) fn args_mut(&mut self) -> &mut [N] {
        &mut self.vals[..self.n]
    }
    pub(crate) fn push(&mut self, v: N) -> Option<()> {
        *self.vals.get_mut(self.n)? = v;
        self.n += 1;
        Some(())
    }
    pub(crate) fn pop(&mut self) -> Option<N> {
        self.n = self.n.checked_sub(1)?;
        Some(self.vals[self.n])
    }
    /// Drops all but the first `keep` operands.
    pub(crate) fn keep(&mut self, keep: Count) {
        self.n = self.n.min(keep.slot());
    }
    pub(crate) fn clear(&mut self) {
        self.n = 0;
    }
}

/// Whether a path has been opened by a `moveto`: drawing before one is a
/// malformed charstring.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Path {
    NotStarted,
    Open,
}

/// Whether the next `moveto` is the glyph's first (no contour to close) or
/// a later one (the previous contour closes first).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Moveto {
    First,
    Later,
}

/// Which axis an alternating operator (`hlineto`, `hvcurveto`, ...) starts on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    fn flip(self) -> Axis {
        match self {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        }
    }
}

/// How `flex1` decides whether its last operand is a dx or a dy. Both
/// specifications say the same thing -- the vector from the start to the
/// fifth point -- and the two oracles compute it two ways, which differ
/// only in the last bit of a rounded `f32`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flex1 {
    /// The fifth point less the start, as ttf-parser computes it for CFF.
    EndPoint,
    /// The five deltas summed, as `[[source: ms-opentype-cff2]]` words it
    /// and HarfBuzz computes it for CFF2.
    Summed,
}

/// The path operators of both charstring dialects over one number type:
/// the current point, whether a contour is open, and the sink. Each
/// operator takes exactly its operands -- the caller has stripped a CFF
/// width -- and refuses any other count. Each coordinate is the previous
/// one plus a delta, in the order the specification lists the deltas.
pub(crate) struct Pen<'s, S: Sink, N: Num> {
    sink: &'s mut S,
    x: N,
    y: N,
    path: Path,
    moveto: Moveto,
}

impl<'s, S: Sink, N: Num> Pen<'s, S, N> {
    pub(crate) fn new(sink: &'s mut S) -> Pen<'s, S, N> {
        Pen {
            sink,
            x: N::ZERO,
            y: N::ZERO,
            path: Path::NotStarted,
            moveto: Moveto::First,
        }
    }

    /// Moves the current point without drawing: `seac`'s accent origin.
    fn place(&mut self, x: N, y: N) {
        self.x = x;
        self.y = y;
    }

    fn moved(&mut self) {
        match self.moveto {
            Moveto::First => self.moveto = Moveto::Later,
            Moveto::Later => self.sink.close(),
        }
        self.path = Path::Open;
    }

    /// Closes the open contour at the end of a glyph: `endchar` in CFF, the
    /// end of the charstring in CFF2.
    pub(crate) fn finish(&mut self) {
        if self.moveto == Moveto::Later {
            self.moveto = Moveto::First;
            self.sink.close();
        }
    }

    /// Whether a `moveto` has opened the path. Every other path operator
    /// needs one, so in a charstring that draws, the path is open from its
    /// first path operator on.
    pub(crate) fn path(&self) -> Path {
        self.path
    }

    fn need_move(&self) -> Option<()> {
        (self.path == Path::Open).then_some(())
    }

    fn emit_move(&mut self) {
        self.sink.move_to(self.x.to_sink(), self.y.to_sink());
    }

    fn emit_line(&mut self) {
        self.sink.line_to(self.x.to_sink(), self.y.to_sink());
    }

    /// A cubic to the current point.
    fn emit_curve(&mut self, x1: N, y1: N, x2: N, y2: N) {
        self.emit_curve_to([x1, y1, x2, y2, self.x, self.y]);
    }

    fn emit_curve_to(&mut self, p: [N; 6]) {
        let [x1, y1, x2, y2, x, y] = p;
        self.sink.curve_to(
            x1.to_sink(),
            y1.to_sink(),
            x2.to_sink(),
            y2.to_sink(),
            x.to_sink(),
            y.to_sink(),
        );
    }

    pub(crate) fn rmoveto(&mut self, a: &[N]) -> Option<()> {
        let [dx, dy] = *a else { return None };
        self.moved();
        self.x = self.x + dx;
        self.y = self.y + dy;
        self.emit_move();
        Some(())
    }

    pub(crate) fn hmoveto(&mut self, a: &[N]) -> Option<()> {
        let [dx] = *a else { return None };
        self.moved();
        self.x = self.x + dx;
        self.emit_move();
        Some(())
    }

    pub(crate) fn vmoveto(&mut self, a: &[N]) -> Option<()> {
        let [dy] = *a else { return None };
        self.moved();
        self.y = self.y + dy;
        self.emit_move();
        Some(())
    }

    pub(crate) fn rlineto(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if !a.len().is_multiple_of(2) {
            return None;
        }
        for d in a.chunks_exact(2) {
            self.x = self.x + d[0];
            self.y = self.y + d[1];
            self.emit_line();
        }
        Some(())
    }

    /// `hlineto` and `vlineto`: alternating single deltas, starting on the
    /// axis named.
    pub(crate) fn alternating_lineto(&mut self, a: &[N], first: Axis) -> Option<()> {
        self.need_move()?;
        if a.is_empty() {
            return None;
        }
        let mut axis = first;
        for &d in a {
            match axis {
                Axis::Horizontal => self.x = self.x + d,
                Axis::Vertical => self.y = self.y + d,
            }
            self.emit_line();
            axis = axis.flip();
        }
        Some(())
    }

    /// One `rrcurveto` segment from six deltas.
    fn curve(&mut self, a: &[N]) {
        let x1 = self.x + a[0];
        let y1 = self.y + a[1];
        let x2 = x1 + a[2];
        let y2 = y1 + a[3];
        self.x = x2 + a[4];
        self.y = y2 + a[5];
        self.emit_curve(x1, y1, x2, y2);
    }

    pub(crate) fn rrcurveto(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if !a.len().is_multiple_of(6) {
            return None;
        }
        for c in a.chunks_exact(6) {
            self.curve(c);
        }
        Some(())
    }

    pub(crate) fn rcurveline(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if a.len() < 8 || !(a.len() - 2).is_multiple_of(6) {
            return None;
        }
        let (curves, line) = a.split_at(a.len() - 2);
        for c in curves.chunks_exact(6) {
            self.curve(c);
        }
        self.x = self.x + line[0];
        self.y = self.y + line[1];
        self.emit_line();
        Some(())
    }

    pub(crate) fn rlinecurve(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if a.len() < 8 || !(a.len() - 6).is_multiple_of(2) {
            return None;
        }
        let (lines, curve) = a.split_at(a.len() - 6);
        for d in lines.chunks_exact(2) {
            self.x = self.x + d[0];
            self.y = self.y + d[1];
            self.emit_line();
        }
        self.curve(curve);
        Some(())
    }

    pub(crate) fn hhcurveto(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        let mut a = a;
        if a.len() % 2 == 1 {
            self.y = self.y + a[0];
            a = &a[1..];
        }
        if !a.len().is_multiple_of(4) {
            return None;
        }
        for c in a.chunks_exact(4) {
            let x1 = self.x + c[0];
            let y1 = self.y;
            let x2 = x1 + c[1];
            let y2 = y1 + c[2];
            self.x = x2 + c[3];
            self.y = y2;
            self.emit_curve(x1, y1, x2, y2);
        }
        Some(())
    }

    pub(crate) fn vvcurveto(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        let mut a = a;
        if a.len() % 2 == 1 {
            self.x = self.x + a[0];
            a = &a[1..];
        }
        if !a.len().is_multiple_of(4) {
            return None;
        }
        for c in a.chunks_exact(4) {
            let x1 = self.x;
            let y1 = self.y + c[0];
            let x2 = x1 + c[1];
            let y2 = y1 + c[2];
            self.x = x2;
            self.y = y2 + c[3];
            self.emit_curve(x1, y1, x2, y2);
        }
        Some(())
    }

    /// `hvcurveto` and `vhcurveto`: alternating tangent curves, with an
    /// optional last delta on the closing axis.
    pub(crate) fn alternating_curveto(&mut self, a: &[N], first: Axis) -> Option<()> {
        self.need_move()?;
        if a.len() < 4 {
            return None;
        }
        let mut axis = first;
        let mut rest = a;
        while !rest.is_empty() {
            if rest.len() < 4 {
                return None;
            }
            // Exactly one operand after this curve's four: the last delta.
            let last = (rest.len() == 5).then(|| rest[4]);
            let x1;
            let y1;
            let x2;
            let y2;
            if axis == Axis::Horizontal {
                x1 = self.x + rest[0];
                y1 = self.y;
                x2 = x1 + rest[1];
                y2 = y1 + rest[2];
                self.y = y2 + rest[3];
                self.x = x2;
                if let Some(d) = last {
                    self.x = self.x + d;
                }
            } else {
                x1 = self.x;
                y1 = self.y + rest[0];
                x2 = x1 + rest[1];
                y2 = y1 + rest[2];
                self.x = x2 + rest[3];
                self.y = y2;
                if let Some(d) = last {
                    self.y = self.y + d;
                }
            }
            self.emit_curve(x1, y1, x2, y2);
            rest = &rest[if last.is_some() { 5 } else { 4 }..];
            axis = axis.flip();
        }
        Some(())
    }

    /// The two curves of a flex, from their five intermediate points and
    /// the current point, which the caller has moved to the sixth, with the
    /// flex depth and the threshold in device pixels below which the sink is
    /// to draw them as one line. A `depth` of `None` is a `flex` back at its
    /// start, which has no line to take a depth from (`Pen::axis_depth`): its
    /// two curves go to the sink as curves.
    fn flex_curves(&mut self, p: [N; 10], depth: Option<[N; 2]>, threshold: f32) {
        let [x1, y1, x2, y2, x3, y3, x4, y4, x5, y5] = p;
        let Some([dx, dy]) = depth else {
            self.emit_curve_to([x1, y1, x2, y2, x3, y3]);
            self.emit_curve(x4, y4, x5, y5);
            return;
        };
        let f = |v: N| v.to_sink();
        self.sink.flex(&Flex {
            first: [f(x1), f(y1), f(x2), f(y2), f(x3), f(y3)],
            second: [f(x4), f(y4), f(x5), f(y5), f(self.x), f(self.y)],
            depth: [f(dx), f(dy)],
            threshold,
        });
    }

    /// The depth of a `flex` from `(x0, y0)` to the current point whose
    /// joining point is `(x3, y3)`, `sum` being its summed `dx1..dx5` and
    /// `dy1..dy5`.
    ///
    /// Where the start and the end share a `y` (horizontal) or an `x`
    /// (vertical), every reading of the text gives the joining point's
    /// offset across that line. Where they share neither, the text's prose
    /// and figure read three ways (the record's question 6), and the deepest
    /// is carried, so that the flex is straight only where every reading is
    /// under the threshold. The perpendicular to the start-end line is never
    /// the deepest, which leaves the longer of the start-end line's distance
    /// from the joining point along the minor axis and the joining point's
    /// distance across to the end point's own line, the orientation by
    /// flex1's test: horizontal where `|sum dx| > |sum dy|`. A `flex` back at
    /// its start has no line: `None`, and it is drawn as its two curves.
    fn axis_depth(&self, x0: N, y0: N, joint: [N; 2], sum: [N; 2]) -> Option<[N; 2]> {
        let ([x3, y3], (x, y)) = (joint, (self.x, self.y));
        let deeper = |line: N, end: N| if abs(end) > abs(line) { end } else { line };
        match (x == x0, y == y0) {
            (false, true) => Some([N::ZERO, y3 - y0]),
            (true, false) => Some([x3 - x0, N::ZERO]),
            (true, true) => None,
            (false, false) if abs(sum[0]) > abs(sum[1]) => {
                let line = y3 - (y0 + (y - y0) * (x3 - x0) / (x - x0));
                Some([N::ZERO, deeper(line, y3 - y)])
            }
            (false, false) => {
                let line = x3 - (x0 + (x - x0) * (y3 - y0) / (y - y0));
                Some([deeper(line, x3 - x), N::ZERO])
            }
        }
    }

    pub(crate) fn flex(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if a.len() != 13 {
            return None;
        }
        let dx1 = self.x + a[0];
        let dy1 = self.y + a[1];
        let dx2 = dx1 + a[2];
        let dy2 = dy1 + a[3];
        let dx3 = dx2 + a[4];
        let dy3 = dy2 + a[5];
        let dx4 = dx3 + a[6];
        let dy4 = dy3 + a[7];
        let dx5 = dx4 + a[8];
        let dy5 = dy4 + a[9];
        let (x0, y0) = (self.x, self.y);
        self.x = dx5 + a[10];
        self.y = dy5 + a[11];
        // "The operand fd specifies a flex depth threshold in 1/100ths of a
        // device pixel." A flex whose start and end share neither
        // coordinate has its depth by "a method that depends on whether the
        // curve is more horizontal or more vertical, as illustrated in the
        // following figure", which the figure and the prose read three
        // ways: a question for Jev, and the deepest of them until it is
        // answered (the record, "A flex shallower than half a pixel draws
        // straight").
        let sum = [
            a[0] + a[2] + a[4] + a[6] + a[8],
            a[1] + a[3] + a[5] + a[7] + a[9],
        ];
        let depth = self.axis_depth(x0, y0, [dx3, dy3], sum);
        let threshold = a[12].to_sink() / 100.0;
        self.flex_curves(
            [dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5],
            depth,
            threshold,
        );
        Some(())
    }

    pub(crate) fn flex1(&mut self, a: &[N], test: Flex1) -> Option<()> {
        self.need_move()?;
        if a.len() != 11 {
            return None;
        }
        let dx1 = self.x + a[0];
        let dy1 = self.y + a[1];
        let dx2 = dx1 + a[2];
        let dy2 = dy1 + a[3];
        let dx3 = dx2 + a[4];
        let dy3 = dy2 + a[5];
        let dx4 = dx3 + a[6];
        let dy4 = dy3 + a[7];
        let dx5 = dx4 + a[8];
        let dy5 = dy4 + a[9];
        let horizontal = match test {
            Flex1::EndPoint => abs(dx5 - self.x) > abs(dy5 - self.y),
            Flex1::Summed => {
                let dx = a[0] + a[2] + a[4] + a[6] + a[8];
                let dy = a[1] + a[3] + a[5] + a[7] + a[9];
                abs(dx) > abs(dy)
            }
        };
        // The end shares the start's `y` when horizontal and its `x`
        // otherwise, so the depth is the joining point's offset across.
        let depth = if horizontal {
            self.x = dx5 + a[10];
            [N::ZERO, dy3 - self.y]
        } else {
            self.y = dy5 + a[10];
            [dx3 - self.x, N::ZERO]
        };
        self.flex_curves(
            [dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5],
            Some(depth),
            IMPLICIT_FLEX_THRESHOLD,
        );
        Some(())
    }

    pub(crate) fn hflex(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if a.len() != 7 {
            return None;
        }
        let dx1 = self.x + a[0];
        let dy1 = self.y;
        let dx2 = dx1 + a[1];
        let dy2 = dy1 + a[2];
        let dx3 = dx2 + a[3];
        let dy3 = dy2;
        let dx4 = dx3 + a[4];
        let dy4 = dy2;
        let dx5 = dx4 + a[5];
        let dy5 = self.y;
        self.x = dx5 + a[6];
        // Horizontal: the end keeps the start's `y`.
        let depth = [N::ZERO, dy3 - self.y];
        self.flex_curves(
            [dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5],
            Some(depth),
            IMPLICIT_FLEX_THRESHOLD,
        );
        Some(())
    }

    pub(crate) fn hflex1(&mut self, a: &[N]) -> Option<()> {
        self.need_move()?;
        if a.len() != 9 {
            return None;
        }
        let dx1 = self.x + a[0];
        let dy1 = self.y + a[1];
        let dx2 = dx1 + a[2];
        let dy2 = dy1 + a[3];
        let dx3 = dx2 + a[4];
        let dy3 = dy2;
        let dx4 = dx3 + a[5];
        let dy4 = dy2;
        let dx5 = dx4 + a[6];
        let dy5 = dy4 + a[7];
        self.x = dx5 + a[8];
        // Horizontal: the end keeps the start's `y`.
        let depth = [N::ZERO, dy3 - self.y];
        self.flex_curves(
            [dx1, dy1, dx2, dy2, dx3, dy3, dx4, dy4, dx5, dy5],
            Some(depth),
            IMPLICIT_FLEX_THRESHOLD,
        );
        Some(())
    }
}

/// Whether the optional leading width argument has been consumed. It rides
/// in the first stack slot of the first stack-clearing operator and is
/// recognised so the argument counts come out right.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Width {
    Pending,
    Taken,
}

/// Whether `endchar` has been reached, and whether it was a `seac`
/// composition, which nests two more charstrings under it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Running,
    Reached,
    Seac,
}

struct Run<'s, 'a, S: Sink> {
    cff: &'s Cff<'a>,
    pen: Pen<'s, S, f32>,
    stack: Stack<f32, MAX_ARGS>,
    width: Width,
    stems: u32,
    end: End,
    glyph: GlyphId,
    local: Option<CffIndex<'a>>,
}

/// A moveto's operands: all of them, or all but the first when there are
/// `WITH_WIDTH`, one more than the operator takes -- the width. As the
/// reference reads it, whether or not a width was already taken.
fn moveto_args<'s, const WITH_WIDTH: usize>(
    stack: &'s Stack<f32, MAX_ARGS>,
    width: &mut Width,
) -> &'s [f32] {
    if stack.args().len() == WITH_WIDTH {
        *width = Width::Taken;
        &stack.args()[1..]
    } else {
        stack.args()
    }
}

impl<S: Sink> Run<'_, '_, S> {
    fn call(&mut self, subrs: CffIndex<'_>, depth: Depth) -> Option<()> {
        let depth = depth.deeper()?;
        let i = subr_index(f64::from(self.stack.pop()?), bias(subrs.len()))?;
        let cs = subrs.get(i)?;
        self.exec(cs, depth)
    }

    fn seac_glyph(&self, code: f32) -> Option<GlyphId> {
        let c = code as i32;
        if f64::from(c) != f64::from(code) {
            return None;
        }
        let code = u8::try_from(c).ok()?;
        let sid = u16::from(STANDARD_ENCODING[usize::from(code)]);
        match self.cff.charset {
            // The reference bounds by the code, not the SID; the two agree
            // for every code Standard Encoding maps.
            Charset::IsoAdobe => (code <= 228).then(|| GlyphId::from_shaper(sid)),
            Charset::Expert | Charset::ExpertSubset => None,
            _ => self.cff.charset.sid_to_gid(sid),
        }
    }

    fn exec(&mut self, cs: Bytes<'_>, depth: Depth) -> Option<()> {
        let mut at = Offset::ZERO;
        while let Some(b) = cs.read::<u8>(at) {
            at = at.plus(Len::of(1))?;
            match b {
                0 | 2 | 9 | 13 | 15 | 16 | 17 => return None,
                op::HSTEM | op::VSTEM | op::HSTEMHM | op::VSTEMHM => {
                    let n = self.stack.args().len();
                    let len = if n % 2 == 1 && self.width == Width::Pending {
                        self.width = Width::Taken;
                        n - 1
                    } else {
                        n
                    };
                    self.stems += u32::try_from(len).ok()? >> 1;
                    self.stack.clear();
                }
                op::VMOVETO => {
                    let a = moveto_args::<2>(&self.stack, &mut self.width);
                    self.pen.vmoveto(a)?;
                    self.stack.clear();
                }
                op::RLINETO => {
                    self.pen.rlineto(self.stack.args())?;
                    self.stack.clear();
                }
                op::HLINETO => {
                    self.pen
                        .alternating_lineto(self.stack.args(), Axis::Horizontal)?;
                    self.stack.clear();
                }
                op::VLINETO => {
                    self.pen
                        .alternating_lineto(self.stack.args(), Axis::Vertical)?;
                    self.stack.clear();
                }
                op::RRCURVETO => {
                    self.pen.rrcurveto(self.stack.args())?;
                    self.stack.clear();
                }
                op::CALLSUBR => {
                    if self.local.is_none() {
                        self.local = self.cff.cid_local_subrs(self.glyph);
                    }
                    let subrs = self.local?;
                    self.call(subrs, depth)?;
                    if self.end == End::Reached {
                        if cs.read::<u8>(at).is_some() {
                            return None;
                        }
                        break;
                    }
                }
                op::RETURN => break,
                TWO_BYTE => {
                    let b2: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    let a = self.stack.args();
                    match b2 {
                        op::HFLEX => self.pen.hflex(a)?,
                        op::FLEX => self.pen.flex(a)?,
                        op::HFLEX1 => self.pen.hflex1(a)?,
                        op::FLEX1 => self.pen.flex1(a, Flex1::EndPoint)?,
                        _ => return None,
                    }
                    self.stack.clear();
                }
                op::ENDCHAR => {
                    let n = self.stack.args().len();
                    if n == 4 || (self.width == Width::Pending && n == 5) {
                        let accent_code = self.stack.pop()?;
                        let base_code = self.stack.pop()?;
                        let accent = self.seac_glyph(accent_code)?;
                        let base = self.seac_glyph(base_code)?;
                        let dy = self.stack.pop()?;
                        let dx = self.stack.pop()?;
                        if self.width == Width::Pending && !self.stack.args().is_empty() {
                            self.stack.pop()?;
                            self.width = Width::Taken;
                        }
                        self.end = End::Seac;
                        let deeper = depth.deeper()?;
                        let base_cs = self
                            .cff
                            .char_strings
                            .get(Index::of(u32::try_from(base.index()).ok()?))?;
                        self.exec(base_cs, deeper)?;
                        self.pen.place(dx, dy);
                        let accent_cs = self
                            .cff
                            .char_strings
                            .get(Index::of(u32::try_from(accent.index()).ok()?))?;
                        self.exec(accent_cs, deeper)?;
                    } else if n == 1 && self.width == Width::Pending {
                        self.stack.pop()?;
                        self.width = Width::Taken;
                    }
                    self.pen.finish();
                    if cs.read::<u8>(at).is_some() {
                        return None;
                    }
                    if self.end == End::Running {
                        self.end = End::Reached;
                    }
                    break;
                }
                op::HINTMASK | op::CNTRMASK => {
                    let mut len = self.stack.args().len();
                    self.stack.clear();
                    if len % 2 == 1 {
                        len -= 1;
                        self.width = Width::Taken;
                    }
                    self.stems += u32::try_from(len).ok()? >> 1;
                    at = at.plus(Len::of((self.stems + 7) >> 3))?;
                }
                op::RMOVETO => {
                    let a = moveto_args::<3>(&self.stack, &mut self.width);
                    self.pen.rmoveto(a)?;
                    self.stack.clear();
                }
                op::HMOVETO => {
                    let a = moveto_args::<2>(&self.stack, &mut self.width);
                    self.pen.hmoveto(a)?;
                    self.stack.clear();
                }
                op::RCURVELINE => {
                    self.pen.rcurveline(self.stack.args())?;
                    self.stack.clear();
                }
                op::RLINECURVE => {
                    self.pen.rlinecurve(self.stack.args())?;
                    self.stack.clear();
                }
                op::VVCURVETO => {
                    self.pen.vvcurveto(self.stack.args())?;
                    self.stack.clear();
                }
                op::HHCURVETO => {
                    self.pen.hhcurveto(self.stack.args())?;
                    self.stack.clear();
                }
                op::SHORTINT => {
                    let n: i16 = cs.read(at)?;
                    at = at.plus(Len::of(2))?;
                    self.stack.push(f32::from(n))?;
                }
                op::CALLGSUBR => {
                    let subrs = self.cff.global_subrs;
                    self.call(subrs, depth)?;
                    if self.end == End::Reached {
                        if cs.read::<u8>(at).is_some() {
                            return None;
                        }
                        break;
                    }
                }
                op::VHCURVETO => {
                    self.pen
                        .alternating_curveto(self.stack.args(), Axis::Vertical)?;
                    self.stack.clear();
                }
                op::HVCURVETO => {
                    self.pen
                        .alternating_curveto(self.stack.args(), Axis::Horizontal)?;
                    self.stack.clear();
                }
                32..=246 => self.stack.push(f32::from(i16::from(b) - 139))?,
                247..=250 => {
                    let b1: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    self.stack
                        .push(f32::from((i16::from(b) - 247) * 256 + i16::from(b1) + 108))?;
                }
                251..=254 => {
                    let b1: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    self.stack
                        .push(f32::from(-(i16::from(b) - 251) * 256 - i16::from(b1) - 108))?;
                }
                op::FIXED => {
                    let n: u32 = cs.read(at)?;
                    at = at.plus(Len::of(4))?;
                    let n = i32::from_ne_bytes(n.to_ne_bytes());
                    self.stack.push(n as f32 / 65536.0)?;
                }
            }
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::vec::Vec;

    /// The flexes a pen hands its sink whole, and the curves it draws.
    #[derive(Default)]
    struct Seen {
        flexes: Vec<Flex>,
        curves: Vec<[f32; 6]>,
    }

    impl Sink for Seen {
        fn move_to(&mut self, _: f32, _: f32) {}
        fn line_to(&mut self, _: f32, _: f32) {}
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.curves.push([x1, y1, x2, y2, x, y]);
        }
        fn close(&mut self) {}
        fn flex(&mut self, f: &Flex) {
            self.flexes.push(*f);
        }
    }

    /// What `op` hands the sink from (100, 100).
    fn from_start(op: impl FnOnce(&mut Pen<'_, Seen, f32>) -> Option<()>) -> Seen {
        let mut s = Seen::default();
        let mut p = Pen::new(&mut s);
        p.rmoveto(&[100.0, 100.0]).unwrap();
        op(&mut p).unwrap();
        s
    }

    /// The one flex `op` hands the sink: its depth and threshold.
    fn one(op: impl FnOnce(&mut Pen<'_, Seen, f32>) -> Option<()>) -> ([f32; 2], f32) {
        let s = from_start(op);
        assert_eq!((s.flexes.len(), s.curves.len()), (1, 0));
        (s.flexes[0].depth, s.flexes[0].threshold)
    }

    /// Each flex operator hands the sink its depth -- the joining point's
    /// offset across the line its start and end share -- and its threshold
    /// in device pixels: `fd`/100 for `flex`, 0.5 for the other three. A
    /// `flex` whose start and end share neither coordinate carries the
    /// deeper of two readings, along the minor axis of flex1's test: the
    /// distance across the start-end line, and the distance across to the
    /// end point's own line. Each slanted pair below has one case where each
    /// is the deeper, horizontal and vertical. A `flex` back at its start
    /// has no line, and draws its two curves.
    #[test]
    fn each_flex_operator_carries_its_depth_and_threshold() {
        let flex = |a: [f32; 13]| move |p: &mut Pen<'_, Seen, f32>| p.flex(&a);
        let h = [10., 5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -5., 30.];
        assert_eq!(one(flex(h)), ([0.0, 10.0], 0.3), "flex, horizontal, fd 30");
        let v = [5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -5., 10., 50.];
        assert_eq!(one(flex(v)), ([10.0, 0.0], 0.5), "flex, vertical");
        // From (100, 100) through (130, 110): ending at (160, 101), the line
        // passes 100.5 under the joining point and the end's line 101, so
        // 9.5 against 9; ending at (160, 99), 99.5 against 99, so 10.5
        // against 11. The vertical pair is the same, transposed.
        let slant = [10., 5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -4., 50.];
        assert_eq!(one(flex(slant)), ([0.0, 9.5], 0.5), "slanted, the line");
        let below = [10., 5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -6., 50.];
        assert_eq!(
            one(flex(below)),
            ([0.0, 11.0], 0.5),
            "slanted, the end's line"
        );
        let v_slant = [5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -4., 10., 50.];
        assert_eq!(
            one(flex(v_slant)),
            ([9.5, 0.0], 0.5),
            "vertical slant, the line"
        );
        let v_below = [5., 10., 5., 10., 0., 10., 0., 10., -5., 10., -6., 10., 50.];
        assert_eq!(
            one(flex(v_below)),
            ([11.0, 0.0], 0.5),
            "vertical slant, the end's line"
        );
        let fifth = [
            10., 5., 10., 5., 10., 0., 10., -5., 10., -5., 10., 100., 50.,
        ];
        assert_eq!(
            one(flex(fifth)),
            ([0.0, -90.0], 0.5),
            "slanted, oriented by the fifth point"
        );
        let back = [
            10., 5., 10., 5., 10., 0., -10., 0., -10., -5., -10., -5., 50.,
        ];
        let s = from_start(flex(back));
        assert_eq!(
            (s.flexes.len(), s.curves.len()),
            (0, 2),
            "flex, back to its start"
        );
        let flex1 = |a: [f32; 11]| move |p: &mut Pen<'_, Seen, f32>| p.flex1(&a, Flex1::EndPoint);
        let h1 = [10., 5., 10., 5., 10., 0., 10., 0., 10., -5., 7.];
        assert_eq!(one(flex1(h1)), ([0.0, 10.0], 0.5), "flex1, horizontal");
        let v1 = [5., 10., 5., 10., 0., 10., 0., 10., -5., 10., 7.];
        assert_eq!(one(flex1(v1)), ([10.0, 0.0], 0.5), "flex1, vertical");
        let hflex = [10., 10., 6., 10., 10., 10., 10.];
        assert_eq!(one(|p| p.hflex(&hflex)), ([0.0, 6.0], 0.5), "hflex");
        let hflex1 = [10., 2., 10., 4., 10., 10., 10., -6., 10.];
        let s = from_start(|p| p.hflex1(&hflex1));
        assert_eq!(s.flexes[0].depth, [0.0, 6.0], "hflex1");
        assert_eq!(s.flexes[0].first, [110., 102., 120., 106., 130., 106.]);
        assert_eq!(s.flexes[0].second, [140., 106., 150., 100., 160., 100.]);
    }
}
