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
//! implicitly; every curve is a cubic; flex operators emit two cubics; the
//! arithmetic is `f32` in the order the reference performs it.
//!
//! **`seac`** -- `endchar` with four or five arguments -- composes a base and
//! an accent named by Standard Encoding codes, which the generated
//! `standard_encoding` table maps to SIDs and the charset maps to glyphs.

use crate::bytes::{Bytes, Count, Index, Len, Offset, U24};
use crate::outline::Sink;
use crate::standard_encoding::STANDARD_ENCODING;
use baud_domain::GlyphId;

const MAX_ARGS: usize = 48;
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
    pub const CHARSET: u16 = 15;
    pub const CHAR_STRINGS: u16 = 17;
    pub const PRIVATE: u16 = 18;
    pub const SUBRS: u16 = 19;
    pub const ROS: u16 = 1230;
    pub const FD_ARRAY: u16 = 1236;
    pub const FD_SELECT: u16 = 1237;
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
struct CffIndex<'a> {
    offsets: Bytes<'a>,
    size: OffSize,
    count: Count,
    data: Bytes<'a>,
}

impl<'a> CffIndex<'a> {
    const EMPTY: CffIndex<'static> = CffIndex {
        offsets: Bytes::new(&[]),
        size: OffSize::One,
        count: Count::ZERO,
        data: Bytes::new(&[]),
    };

    /// The INDEX at `at`, and the offset just past it.
    fn parse(b: Bytes<'a>, at: Offset) -> Option<(CffIndex<'a>, Offset)> {
        let count: u16 = b.read(at)?;
        let after_count = at.plus(Len::of(2))?;
        if count == 0 {
            return Some((CffIndex::EMPTY, after_count));
        }
        let size = OffSize::parse(b.read(after_count)?)?;
        let offsets_at = after_count.plus(Len::of(1))?;
        let entries = Count::of(u32::from(count).checked_add(1)?);
        let offsets_len = size.len().times(entries)?;
        let offsets = b.slice(offsets_at, offsets_len)?;
        let data_at = offsets_at.plus(offsets_len)?;
        let index = CffIndex {
            offsets,
            size,
            count: Count::of(count),
            data: Bytes::new(&[]),
        };
        let last = index.offset(Count::of(count).as_index())?;
        let data = b.slice(data_at, last)?;
        let end = data_at.plus(last)?;
        Some((CffIndex { data, ..index }, end))
    }

    fn len(&self) -> Count {
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

    fn get(&self, i: Index) -> Option<Bytes<'a>> {
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
/// because a 32-bit integer operand does not fit an `f32`.
struct Operands {
    vals: [f64; MAX_ARGS],
    n: Count,
}

impl Operands {
    fn get(&self, i: Index) -> Option<f64> {
        self.n.holds(i).then(|| self.vals[i.slot()])
    }
}

/// Scans a DICT for operator `want` and returns its operands.
fn dict_operands(data: Bytes<'_>, want: u16) -> Option<Operands> {
    let mut ops = Operands {
        vals: [0.0; MAX_ARGS],
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
                1200 + u16::from(b2)
            } else {
                u16::from(b)
            };
            if operator == want {
                return Some(ops);
            }
            ops.n = Count::ZERO;
            continue;
        }
        let (v, next) = dict_number(data, b, at)?;
        at = next;
        if ops.n.as_index().slot() >= MAX_ARGS {
            return None;
        }
        ops.vals[ops.n.as_index().slot()] = v;
        ops.n = Count::of(ops.n.value() + 1);
    }
    None
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

/// CID-keyed fonts: which Font DICT a glyph uses.
#[derive(Clone, Copy)]
enum FdSelect<'a> {
    Format0(Bytes<'a>),
    Format3(Bytes<'a>),
}

impl FdSelect<'_> {
    fn parse(b: Bytes<'_>, at: Offset) -> Option<FdSelect<'_>> {
        let format: u8 = b.read(at)?;
        let body = b.tail(at.plus(Len::of(1))?)?;
        match format {
            0 => Some(FdSelect::Format0(body)),
            3 => Some(FdSelect::Format3(body)),
            _ => None,
        }
    }

    fn index_of(&self, g: GlyphId) -> Option<u8> {
        match *self {
            FdSelect::Format0(data) => data.read(Offset::of(u32::try_from(g.index()).ok()?)),
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
                        return Some(prev_fd);
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
fn usize_of(v: f64) -> Option<u32> {
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
            let fd_select = FdSelect::parse(data, Offset::of(fds))?;
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
        let font_dict = fd_array.get(Index::of(fd))?;
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
            sink,
            stack: Stack {
                vals: [0.0; MAX_ARGS],
                n: 0,
            },
            x: 0.0,
            y: 0.0,
            path: Path::NotStarted,
            moveto: Moveto::First,
            width: Width::Pending,
            stems: 0,
            end: End::Running,
            glyph: g,
            local,
        };
        run.exec(cs, 0)?;
        (run.end != End::Running).then_some(())
    }
}

fn bias(count: Count) -> i32 {
    let n = count.value();
    if n < 1240 {
        107
    } else if n < 33900 {
        1131
    } else {
        32768
    }
}

fn subr_index(v: f32, bias: i32) -> Option<Index> {
    let i = v as i32;
    if f64::from(i) != f64::from(v) {
        return None;
    }
    Index::of(u32::try_from(i.checked_add(bias)?).ok()?).into()
}

/// The argument stack, 48 deep per the specification.
struct Stack {
    vals: [f32; MAX_ARGS],
    n: usize,
}

impl Stack {
    fn args(&self) -> &[f32] {
        &self.vals[..self.n]
    }
    fn push(&mut self, v: f32) -> Option<()> {
        if self.n >= MAX_ARGS {
            return None;
        }
        self.vals[self.n] = v;
        self.n += 1;
        Some(())
    }
    fn pop(&mut self) -> f32 {
        self.n -= 1;
        self.vals[self.n]
    }
    fn clear(&mut self) {
        self.n = 0;
    }
    fn reverse(&mut self) {
        self.vals[..self.n].reverse();
    }
}

/// Whether a path has been opened by a `moveto`: drawing before one is a
/// malformed charstring.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
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

/// Which axis an alternating operator (`hlineto`, `hvcurveto`, ...) starts on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
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

struct Run<'s, 'a, S: Sink> {
    cff: &'s Cff<'a>,
    sink: &'s mut S,
    stack: Stack,
    x: f32,
    y: f32,
    path: Path,
    moveto: Moveto,
    width: Width,
    stems: u32,
    end: End,
    glyph: GlyphId,
    local: Option<CffIndex<'a>>,
}

impl<S: Sink> Run<'_, '_, S> {
    fn moved(&mut self) {
        match self.moveto {
            Moveto::First => self.moveto = Moveto::Later,
            Moveto::Later => self.sink.close(),
        }
        self.path = Path::Open;
    }

    fn need_move(&self) -> Option<()> {
        (self.path == Path::Open).then_some(())
    }

    fn rmoveto(&mut self, from: usize) -> Option<()> {
        if self.stack.n != from + 2 {
            return None;
        }
        self.moved();
        self.x += self.stack.args()[from];
        self.y += self.stack.args()[from + 1];
        self.sink.move_to(self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn hmoveto(&mut self, from: usize) -> Option<()> {
        if self.stack.n != from + 1 {
            return None;
        }
        self.moved();
        self.x += self.stack.args()[from];
        self.sink.move_to(self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn vmoveto(&mut self, from: usize) -> Option<()> {
        if self.stack.n != from + 1 {
            return None;
        }
        self.moved();
        self.y += self.stack.args()[from];
        self.sink.move_to(self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn rlineto(&mut self) -> Option<()> {
        self.need_move()?;
        if !self.stack.n.is_multiple_of(2) {
            return None;
        }
        let mut i = 0;
        while i < self.stack.n {
            self.x += self.stack.args()[i];
            self.y += self.stack.args()[i + 1];
            self.sink.line_to(self.x, self.y);
            i += 2;
        }
        self.stack.clear();
        Some(())
    }

    /// `hlineto` and `vlineto`: alternating single deltas, starting on the
    /// axis named.
    fn alternating_lineto(&mut self, first: Axis) -> Option<()> {
        self.need_move()?;
        if self.stack.n == 0 {
            return None;
        }
        let mut axis = first;
        let mut i = 0;
        while i < self.stack.n {
            match axis {
                Axis::Horizontal => self.x += self.stack.args()[i],
                Axis::Vertical => self.y += self.stack.args()[i],
            }
            i += 1;
            self.sink.line_to(self.x, self.y);
            axis = axis.flip();
        }
        self.stack.clear();
        Some(())
    }

    fn curve(&mut self, i: usize) {
        let a = self.stack.args();
        let x1 = self.x + a[i];
        let y1 = self.y + a[i + 1];
        let x2 = x1 + a[i + 2];
        let y2 = y1 + a[i + 3];
        self.x = x2 + a[i + 4];
        self.y = y2 + a[i + 5];
        self.sink.curve_to(x1, y1, x2, y2, self.x, self.y);
    }

    fn rrcurveto(&mut self) -> Option<()> {
        self.need_move()?;
        if !self.stack.n.is_multiple_of(6) {
            return None;
        }
        let mut i = 0;
        while i < self.stack.n {
            self.curve(i);
            i += 6;
        }
        self.stack.clear();
        Some(())
    }

    fn rcurveline(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n < 8 || !(self.stack.n - 2).is_multiple_of(6) {
            return None;
        }
        let mut i = 0;
        while i < self.stack.n - 2 {
            self.curve(i);
            i += 6;
        }
        self.x += self.stack.args()[i];
        self.y += self.stack.args()[i + 1];
        self.sink.line_to(self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn rlinecurve(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n < 8 || !(self.stack.n - 6).is_multiple_of(2) {
            return None;
        }
        let mut i = 0;
        while i < self.stack.n - 6 {
            self.x += self.stack.args()[i];
            self.y += self.stack.args()[i + 1];
            self.sink.line_to(self.x, self.y);
            i += 2;
        }
        self.curve(i);
        self.stack.clear();
        Some(())
    }

    fn hhcurveto(&mut self) -> Option<()> {
        self.need_move()?;
        let mut i = 0;
        if self.stack.n % 2 == 1 {
            self.y += self.stack.args()[0];
            i += 1;
        }
        if !(self.stack.n - i).is_multiple_of(4) {
            return None;
        }
        while i < self.stack.n {
            let a = self.stack.args();
            let x1 = self.x + a[i];
            let y1 = self.y;
            let x2 = x1 + a[i + 1];
            let y2 = y1 + a[i + 2];
            self.x = x2 + a[i + 3];
            self.y = y2;
            self.sink.curve_to(x1, y1, x2, y2, self.x, self.y);
            i += 4;
        }
        self.stack.clear();
        Some(())
    }

    fn vvcurveto(&mut self) -> Option<()> {
        self.need_move()?;
        let mut i = 0;
        if self.stack.n % 2 == 1 {
            self.x += self.stack.args()[0];
            i += 1;
        }
        if !(self.stack.n - i).is_multiple_of(4) {
            return None;
        }
        while i < self.stack.n {
            let a = self.stack.args();
            let x1 = self.x;
            let y1 = self.y + a[i];
            let x2 = x1 + a[i + 1];
            let y2 = y1 + a[i + 2];
            self.x = x2;
            self.y = y2 + a[i + 3];
            self.sink.curve_to(x1, y1, x2, y2, self.x, self.y);
            i += 4;
        }
        self.stack.clear();
        Some(())
    }

    /// `hvcurveto` and `vhcurveto`: alternating tangent curves, with an
    /// optional last delta on the closing axis.
    fn alternating_curveto(&mut self, first: Axis) -> Option<()> {
        self.need_move()?;
        if self.stack.n < 4 {
            return None;
        }
        self.stack.reverse();
        let mut axis = first;
        while self.stack.n != 0 {
            if self.stack.n < 4 {
                return None;
            }
            if axis == Axis::Horizontal {
                let x1 = self.x + self.stack.pop();
                let y1 = self.y;
                let x2 = x1 + self.stack.pop();
                let y2 = y1 + self.stack.pop();
                self.y = y2 + self.stack.pop();
                self.x = x2;
                if self.stack.n == 1 {
                    self.x += self.stack.pop();
                }
                self.sink.curve_to(x1, y1, x2, y2, self.x, self.y);
            } else {
                let x1 = self.x;
                let y1 = self.y + self.stack.pop();
                let x2 = x1 + self.stack.pop();
                let y2 = y1 + self.stack.pop();
                self.x = x2 + self.stack.pop();
                self.y = y2;
                if self.stack.n == 1 {
                    self.y += self.stack.pop();
                }
                self.sink.curve_to(x1, y1, x2, y2, self.x, self.y);
            }
            axis = axis.flip();
        }
        Some(())
    }

    fn flex(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n != 13 {
            return None;
        }
        let a = self.stack.args();
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
        self.x = dx5 + a[10];
        self.y = dy5 + a[11];
        self.sink.curve_to(dx1, dy1, dx2, dy2, dx3, dy3);
        self.sink.curve_to(dx4, dy4, dx5, dy5, self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn flex1(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n != 11 {
            return None;
        }
        let a = self.stack.args();
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
        if abs(dx5 - self.x) > abs(dy5 - self.y) {
            self.x = dx5 + a[10];
        } else {
            self.y = dy5 + a[10];
        }
        self.sink.curve_to(dx1, dy1, dx2, dy2, dx3, dy3);
        self.sink.curve_to(dx4, dy4, dx5, dy5, self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn hflex(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n != 7 {
            return None;
        }
        let a = self.stack.args();
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
        self.sink.curve_to(dx1, dy1, dx2, dy2, dx3, dy3);
        self.sink.curve_to(dx4, dy4, dx5, dy5, self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn hflex1(&mut self) -> Option<()> {
        self.need_move()?;
        if self.stack.n != 9 {
            return None;
        }
        let a = self.stack.args();
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
        self.sink.curve_to(dx1, dy1, dx2, dy2, dx3, dy3);
        self.sink.curve_to(dx4, dy4, dx5, dy5, self.x, self.y);
        self.stack.clear();
        Some(())
    }

    fn call(&mut self, subrs: CffIndex<'_>, depth: u8) -> Option<()> {
        if self.stack.n == 0 || depth == MAX_DEPTH {
            return None;
        }
        let i = subr_index(self.stack.pop(), bias(subrs.len()))?;
        let cs = subrs.get(i)?;
        self.exec(cs, depth + 1)
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

    fn exec(&mut self, cs: Bytes<'_>, depth: u8) -> Option<()> {
        let mut at = Offset::ZERO;
        while let Some(b) = cs.read::<u8>(at) {
            at = at.plus(Len::of(1))?;
            match b {
                0 | 2 | 9 | 13 | 15 | 16 | 17 => return None,
                op::HSTEM | op::VSTEM | op::HSTEMHM | op::VSTEMHM => {
                    let len = if self.stack.n % 2 == 1 && self.width == Width::Pending {
                        self.width = Width::Taken;
                        self.stack.n - 1
                    } else {
                        self.stack.n
                    };
                    self.stems += u32::try_from(len).ok()? >> 1;
                    self.stack.clear();
                }
                op::VMOVETO => {
                    let mut i = 0;
                    if self.stack.n == 2 {
                        i += 1;
                        self.width = Width::Taken;
                    }
                    self.vmoveto(i)?;
                }
                op::RLINETO => self.rlineto()?,
                op::HLINETO => self.alternating_lineto(Axis::Horizontal)?,
                op::VLINETO => self.alternating_lineto(Axis::Vertical)?,
                op::RRCURVETO => self.rrcurveto()?,
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
                    match b2 {
                        op::HFLEX => self.hflex()?,
                        op::FLEX => self.flex()?,
                        op::HFLEX1 => self.hflex1()?,
                        op::FLEX1 => self.flex1()?,
                        _ => return None,
                    }
                }
                op::ENDCHAR => {
                    if self.stack.n == 4 || (self.width == Width::Pending && self.stack.n == 5) {
                        let accent_code = self.stack.pop();
                        let base_code = self.stack.pop();
                        let accent = self.seac_glyph(accent_code)?;
                        let base = self.seac_glyph(base_code)?;
                        let dy = self.stack.pop();
                        let dx = self.stack.pop();
                        if self.width == Width::Pending && self.stack.n != 0 {
                            self.stack.pop();
                            self.width = Width::Taken;
                        }
                        self.end = End::Seac;
                        if depth == MAX_DEPTH {
                            return None;
                        }
                        let base_cs = self
                            .cff
                            .char_strings
                            .get(Index::of(u32::try_from(base.index()).ok()?))?;
                        self.exec(base_cs, depth + 1)?;
                        self.x = dx;
                        self.y = dy;
                        let accent_cs = self
                            .cff
                            .char_strings
                            .get(Index::of(u32::try_from(accent.index()).ok()?))?;
                        self.exec(accent_cs, depth + 1)?;
                    } else if self.stack.n == 1 && self.width == Width::Pending {
                        self.stack.pop();
                        self.width = Width::Taken;
                    }
                    if self.moveto == Moveto::Later {
                        self.moveto = Moveto::First;
                        self.sink.close();
                    }
                    if cs.read::<u8>(at).is_some() {
                        return None;
                    }
                    if self.end == End::Running {
                        self.end = End::Reached;
                    }
                    break;
                }
                op::HINTMASK | op::CNTRMASK => {
                    let mut len = self.stack.n;
                    self.stack.clear();
                    if len % 2 == 1 {
                        len -= 1;
                        self.width = Width::Taken;
                    }
                    self.stems += u32::try_from(len).ok()? >> 1;
                    at = at.plus(Len::of((self.stems + 7) >> 3))?;
                }
                op::RMOVETO => {
                    let mut i = 0;
                    if self.stack.n == 3 {
                        i += 1;
                        self.width = Width::Taken;
                    }
                    self.rmoveto(i)?;
                }
                op::HMOVETO => {
                    let mut i = 0;
                    if self.stack.n == 2 {
                        i += 1;
                        self.width = Width::Taken;
                    }
                    self.hmoveto(i)?;
                }
                op::RCURVELINE => self.rcurveline()?,
                op::RLINECURVE => self.rlinecurve()?,
                op::VVCURVETO => self.vvcurveto()?,
                op::HHCURVETO => self.hhcurveto()?,
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
                op::VHCURVETO => self.alternating_curveto(Axis::Vertical)?,
                op::HVCURVETO => self.alternating_curveto(Axis::Horizontal)?,
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

fn abs(v: f32) -> f32 {
    if v.is_sign_negative() { -v } else { v }
}
