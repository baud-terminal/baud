//! `CFF2`: the Compact Font Format version 2 and its charstrings, with the
//! variation operators (`[[source: ms-opentype-cff2]]`, which since
//! OpenType 1.9.1 carries the charstring format too; the separate
//! `cff2charstr` page is gone).
//!
//! **What is read.** The header, the Top DICT (CharStringINDEXOffset,
//! VariationStoreOffset, FontDICTINDEXOffset, FontDICTSelectOffset), the
//! global subroutines, each Font DICT's Private DICT -- its local
//! subroutines and its `vsindex` -- FDSelect in formats 0, 3 and 4, the
//! VariationStore, and the charstrings. FontMatrix is not read: `head`'s
//! unitsPerEm is the scale, and the specification requires the two to
//! agree. Hints are counted, for `hintmask`'s length, and discarded, as in
//! CFF -- with the CFF2 text's rules rather than CFF's: stem operands come in
//! pairs, there being no width to make them odd, and the first mask fixes the
//! masks' length, the stem hints coming first ("hs* vs* cm* hm*"). A stem
//! hint or `cntrmask` out of the text's order "hs* vs* cm* hm* {mt <path
//! constructing data>}*" -- a stem operator after the first mask or the
//! first path operator, an `hstem` or `hstemhm` after a vertical stem, a
//! `cntrmask` after a `hintmask` or a path operator -- operands before a
//! later mask (whose stack is "[ - ]"), a `hintmask` in a charstring with no
//! `hstemhm` or `vstemhm` stem or with an `hstem` or `vstem` one, and a
//! `vsindex` of other than one operand are forms the text does not have, and
//! each refuses its glyph.
//!
//! **A table whose structure does not read is refused whole** -- no INDEX,
//! a DICT past its stack, a DICT key with other than its number of operands
//! (the text's "If it is not empty, the subtable is invalid"), an FDSelect
//! where there is one Font DICT, none where there are more, or one that does
//! not map every glyph to a Font DICT there is, a CharStringINDEX whose
//! count is not `maxp`'s (the specification's "must match", which HarfBuzz
//! 12.3.0 also enforces, `[[source: harfbuzz-12-3-0-ot-cff2-table-hh]]`), a
//! Private DICT that names subroutines that are not there, a VariationStore
//! that does not parse, or a Private DICT `vsindex` that names no data set
//! -- so that the face can refuse at load rather than draw blanks
//! (`Face::parse`). A malformed charstring refuses only its glyph, as in
//! CFF.
//!
//! **The charstring dialect differs from CFF's** in what the specification
//! changed: there is no width operand, no `endchar` and no `return` -- the
//! end of a subroutine's data returns and the end of the glyph's ends the
//! glyph, closing the open contour (the specification's implicit lineto);
//! the stack holds 513 operands; an operator the dialect does not define
//! "is ignored and the stack is cleared", which takes in CFF's `endchar`
//! (14) and `return` (11) and every reserved byte; and `vsindex` and
//! `blend` vary the operands.
//!
//! **The arithmetic is `f64`, as HarfBuzz's** (`[[source:
//! harfbuzz-12-3-0-cff-interp-common]]`'s `number_t`): the variations
//! overview recommends that "at least 16 fractional bits of precision be
//! maintained" in scalars and interpolated values
//! (`[[source: ms-opentype-otvaroverview]]`), which an `f32` coordinate past
//! 256 units does not keep. A coordinate becomes `f32` only at the sink.
//! A `blend` adds each value's deltas, each times its region's `f32`
//! scalar, into a sum that starts at 0, and adds the sum to the default --
//! HarfBuzz's order (`[[source: harfbuzz-12-3-0-cff2-interp-cs]]`,
//! `blend_deltas`), where the specification says only that the products
//! "are then all added to the default value". The scalars are taken once,
//! at the first `blend`, for the data set the Private DICT or the
//! charstring's own `vsindex` names.

use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::cff::{
    Axis, CffIndex, Depth, DictLimit, DictOp, DictScan, FdFormats, FdSelect, Flex1, Path, Pen,
    Stack, bias, dict_scan, subr_index, usize_of,
};
use crate::ivs::Store;
use crate::outline::Sink;
use crate::var::Coords;
use baud_domain::GlyphId;

/// The CFF2 argument stack ("The maximum number of operands on the CFF2
/// stack is 513").
const STACK: usize = 513;
/// The most regions one `blend` of one value can use: `1 + k + 1 <= 513`.
const MAX_REGIONS: usize = STACK - 2;
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
    pub const VSINDEX: u8 = 15;
    pub const BLEND: u8 = 16;
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
    use crate::cff::DictOp;
    pub const CHAR_STRINGS: DictOp = DictOp(17);
    pub const PRIVATE: DictOp = DictOp(18);
    pub const SUBRS: DictOp = DictOp(19);
    pub const VSINDEX: DictOp = DictOp(22);
    pub const VSTORE: DictOp = DictOp(24);
    pub const FD_ARRAY: DictOp = DictOp(1236);
    pub const FD_SELECT: DictOp = DictOp(1237);
}

#[derive(Clone, Copy)]
pub struct Cff2<'a> {
    data: Bytes<'a>,
    global_subrs: CffIndex<'a>,
    char_strings: CffIndex<'a>,
    fd_array: CffIndex<'a>,
    fd_select: Option<FdSelect<'a>>,
    store: Option<Store<'a>>,
}

/// What a charstring needs of its Private DICT.
#[derive(Clone, Copy)]
struct Private<'a> {
    subrs: CffIndex<'a>,
    vsindex: Index,
}

/// A DICT key whose one operand is an offset or an index: `Some(None)` when
/// the key is absent, `None` when the DICT is malformed or the operand is
/// not a non-negative integer.
fn key(data: Bytes<'_>, want: DictOp) -> Option<Option<Offset>> {
    match dict_scan(data, want, DictLimit::Cff2) {
        // "the number of operands preceding a DICT key operator must be
        // exactly the number required ... If it is not empty, the subtable
        // is invalid."
        DictScan::Found(ops) if ops.len() != Count::of(1u32) => None,
        DictScan::Found(ops) => Some(Some(Offset::of(usize_of(ops.get(Index::ZERO)?)?))),
        DictScan::Absent => Some(None),
        DictScan::Malformed => None,
    }
}

impl<'a> Cff2<'a> {
    /// The table, or `None` when its structure does not read; `glyphs` is
    /// `maxp`'s count, which the CharStringINDEX must match.
    pub fn parse(data: Bytes<'a>, glyphs: Count) -> Option<Cff2<'a>> {
        let major: u8 = data.read(Offset::ZERO)?;
        if major != 2 {
            return None;
        }
        // The TopDICT is at headerSize, topDICTSize long, and the
        // GlobalSubrINDEX follows it.
        let header_size: u8 = data.read(Offset::of(2u32))?;
        let top_size: u16 = data.read(Offset::of(3u32))?;
        let top_at = Offset::of(header_size);
        let top_len = Len::of(u32::from(top_size));
        let top = data.slice(top_at, top_len)?;
        let (global_subrs, _) = CffIndex::parse_cff2(data, top_at.plus(top_len)?)?;

        let (char_strings, _) = CffIndex::parse_cff2(data, key(top, dict::CHAR_STRINGS)??)?;
        if char_strings.len() != glyphs {
            return None;
        }
        let (fd_array, _) = CffIndex::parse_cff2(data, key(top, dict::FD_ARRAY)??)?;
        if fd_array.len().is_zero() {
            return None;
        }
        // "If there is more than one FontDICT ... a FontDICTSelect table
        // must be used", and with one "the FontDICTSelectOffset operator
        // must not be used".
        let one_fd = fd_array.len() == Count::of(1u32);
        let fd_select = match key(top, dict::FD_SELECT)? {
            None if one_fd => None,
            Some(at) if !one_fd => {
                let s = FdSelect::parse(data, at, FdFormats::Cff2)?;
                s.covers(glyphs, fd_array.len())?;
                Some(s)
            }
            _ => return None,
        };
        // The VariationStore is a uint16 length and an ItemVariationStore
        // of that length.
        let store = match key(top, dict::VSTORE)? {
            None => None,
            Some(at) => {
                let len: u16 = data.read(at)?;
                let ivs = data.slice(at.plus(Len::of(2))?, Len::of(u32::from(len)))?;
                Some(Store::parse(ivs)?)
            }
        };
        let cff2 = Cff2 {
            data,
            global_subrs,
            char_strings,
            fd_array,
            fd_select,
            store,
        };
        let sets = store.map_or(Count::ZERO, |s| s.sets());
        let mut i = Index::ZERO;
        while fd_array.len().holds(i) {
            let private = cff2.private(i)?;
            // Data set 0 is the default whether or not there is a store; a
            // `vsindex` key past it names nothing.
            if private.vsindex != Index::ZERO && !sets.holds(private.vsindex) {
                return None;
            }
            i = i.next();
        }
        Some(cff2)
    }

    /// Font DICT `fd`'s Private DICT. A size of 0 is the empty Private
    /// DICT, "0 0" in the specification's words: no subroutines, data set 0.
    fn private(&self, fd: Index) -> Option<Private<'a>> {
        let font_dict = self.fd_array.get(fd)?;
        let DictScan::Found(p) = dict_scan(font_dict, dict::PRIVATE, DictLimit::Cff2) else {
            return None;
        };
        if p.len() != Count::of(2u32) {
            return None;
        }
        let size = usize_of(p.get(Index::ZERO)?)?;
        let start = usize_of(p.get(Index::of(1u32))?)?;
        if size == 0 {
            return Some(Private {
                subrs: CffIndex::EMPTY,
                vsindex: Index::ZERO,
            });
        }
        let pd = self.data.slice(Offset::of(start), Len::of(size))?;
        // LocalSubrINDEXOffset is from the Private DICT's start.
        let subrs = match key(pd, dict::SUBRS)? {
            None => CffIndex::EMPTY,
            Some(rel) => {
                let at = Offset::of(start).plus(rel.since_start())?;
                CffIndex::parse_cff2(self.data, at)?.0
            }
        };
        let vsindex = match key(pd, dict::VSINDEX)? {
            None => Index::ZERO,
            Some(ivs) => Index::of(ivs.since_start().value()),
        };
        Some(Private { subrs, vsindex })
    }
    /// Emits `g` at `coords`. `Some(())` on a well-formed charstring, `None`
    /// on a missing glyph or a malformed one.
    pub fn outline<S: Sink>(&self, g: GlyphId, coords: &Coords, sink: &mut S) -> Option<()> {
        let cs = self
            .char_strings
            .get(Index::of(u32::try_from(g.index()).ok()?))?;
        let fd = match self.fd_select {
            None => Index::ZERO,
            Some(s) => s.index_of(g)?,
        };
        let private = self.private(fd)?;
        let mut run = Run {
            cff2: self,
            coords,
            pen: Pen::new(sink),
            stack: Stack::new(),
            stems: Count::ZERO,
            local: private.subrs,
            vsindex: private.vsindex,
            variation: Variation::Unset,
            regions: Regions::Pending,
            mask: Mask::Unsized,
            hinting: Hinting::Unhinted,
            part: Part::Horizontal,
            scalars: [0.0; MAX_REGIONS],
        };
        run.exec(cs, Depth::GLYPH)?;
        run.pen.finish();
        Some(())
    }
}

/// Where a charstring is in the specification's order for its variation
/// operators: `vsindex` "may only be used once in a CharString and must
/// precede the first use of the blend operator".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Variation {
    Unset,
    Vsindex,
    Blended,
}

/// Whether the active data set's scalars have been taken -- once, at the
/// first `blend` -- and how many regions it has.
#[derive(Clone, Copy)]
enum Regions {
    Pending,
    Taken(Count),
}

/// Whether the first `hintmask` or `cntrmask` has fixed the masks' length.
///
/// The stem hints come first -- "hs* vs* cm* hm*" -- so the stems are all
/// defined by then. A mask's stack is "[ - ]", and the implied `vstemhm` is
/// the one "followed immediately by a cntrmask or hintmask", so operands
/// before a later mask are neither stems nor that form, and refuse the glyph.
#[derive(Clone, Copy)]
enum Mask {
    Unsized,
    Sized(Len),
}

/// Which stem operators a charstring has defined stems with: `hintmask`
/// "must not be used unless the CharString also defines stems using hstemhm
/// or vstemhm", and stems defined with `hstem` or `vstem` "cannot be used in
/// conjunction with the hintmask operator".
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hinting {
    /// No stem defined yet.
    Unhinted,
    /// Every stem by `hstemhm`, `vstemhm`, or implied before a mask.
    Hm,
    /// At least one stem by `hstem` or `vstem`.
    Plain,
}

/// Where a charstring is in the text's order, "hs* vs* cm* hm* {mt <path
/// constructing data>}*": horizontal stems, vertical stems, `cntrmask`s,
/// `hintmask`s, and then the path, which the pen's first `moveto` opens and
/// "within" which "there can be any number of hintmask operations" and
/// nothing else of these.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    /// Nothing but horizontal stems yet.
    Horizontal,
    /// A vertical stem has been defined.
    Vertical,
    /// A `cntrmask` has come.
    Counters,
    /// A `hintmask` has come.
    Hints,
}

/// The operator a run of stem operands was defined by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StemOp {
    /// `hstem` or `vstem`.
    Plain,
    /// `hstemhm` or `vstemhm`, or the vertical stems a mask implies: in a
    /// charstring that uses `hintmask` those must be `vstemhm`, and in one
    /// that does not, the kind changes nothing.
    Hm,
}

/// One charstring's interpretation: the pen, the stack, the stems so far and
/// the variation state.
struct Run<'s, 'a, S: Sink> {
    cff2: &'s Cff2<'a>,
    coords: &'s Coords,
    pen: Pen<'s, S, f64>,
    stack: Stack<f64, STACK>,
    stems: Count,
    local: CffIndex<'a>,
    vsindex: Index,
    variation: Variation,
    regions: Regions,
    mask: Mask,
    hinting: Hinting,
    part: Part,
    scalars: [f32; MAX_REGIONS],
}

/// A charstring operand that must be a non-negative integer: `blend`'s n,
/// `vsindex`'s data set.
fn whole(v: f64) -> Option<Count> {
    let i = v as i64;
    if i as f64 != v {
        return None;
    }
    u32::try_from(i).ok().map(Count::of)
}

impl<S: Sink> Run<'_, '_, S> {
    /// The active data set's region count, taking its scalars on the first
    /// call. A `blend` with no VariationStore is malformed: the
    /// specification forbids it.
    fn regions(&mut self) -> Option<Count> {
        if let Regions::Taken(k) = self.regions {
            return Some(k);
        }
        let store = self.cff2.store?;
        let k = store.scalars(self.vsindex, self.coords, &mut self.scalars)?;
        self.regions = Regions::Taken(k);
        Some(k)
    }

    /// `n default values, n * k deltas, n`: each default plus its deltas,
    /// each times its region's scalar.
    fn blend(&mut self) -> Option<()> {
        let n = whole(self.stack.pop()?)?;
        let k = self.regions()?;
        self.variation = Variation::Blended;
        let per = k.plus(Count::of(1u32))?;
        let take = Count::of(n.value().checked_mul(per.value())?);
        let have = Count::of(u32::try_from(self.stack.args().len()).ok()?);
        let start = have.minus(take)?;
        let args = self.stack.args_mut();
        let base = start.slot();
        let (n, k) = (n.slot(), k.slot());
        for i in 0..n {
            let deltas = base + n + i * k;
            let mut sum = 0.0f64;
            for (s, d) in self.scalars[..k].iter().zip(&args[deltas..deltas + k]) {
                sum += f64::from(*s) * d;
            }
            args[base + i] += sum;
        }
        self.stack
            .keep(start.plus(Count::of(u32::try_from(n).ok()?))?);
        Some(())
    }

    /// `vsindex`'s stack is "[ <integer> ]": one operand, and any other
    /// count is not that form and refuses the glyph.
    fn vsindex(&mut self) -> Option<()> {
        if self.variation != Variation::Unset || self.stack.args().len() != 1 {
            return None;
        }
        let ivs = whole(self.stack.pop()?)?.as_index();
        let sets = self.cff2.store?.sets();
        if !sets.holds(ivs) {
            return None;
        }
        self.vsindex = ivs;
        self.variation = Variation::Vsindex;
        self.stack.clear();
        Some(())
    }

    /// A stem operator's operands, "[ (dy1 dy2)+ ]": pairs, each a stem.
    ///
    /// An odd count is not that form, and refuses the glyph, as a path
    /// operator with the wrong count does; CFF's odd operand is its width,
    /// which CFF2 has none of.
    fn stems(&mut self, by: StemOp) -> Option<()> {
        let n = self.stack.args().len();
        if !n.is_multiple_of(2) {
            return None;
        }
        let pairs = u32::try_from(n / 2).ok()?;
        self.stems = self.stems.plus(Count::of(pairs))?;
        if pairs > 0 {
            self.hinting = match (self.hinting, by) {
                (Hinting::Plain, _) | (_, StemOp::Plain) => Hinting::Plain,
                _ => Hinting::Hm,
            };
        }
        self.stack.clear();
        Some(())
    }

    /// A stem operator's place in the text's order: none after the first
    /// mask or the first path operator, and no horizontal stem after a
    /// vertical one.
    fn stem_in_order(&mut self, axis: Axis) -> Option<()> {
        if matches!(self.mask, Mask::Sized(_)) || self.pen.path() == Path::Open {
            return None;
        }
        match axis {
            Axis::Horizontal if self.part == Part::Vertical => return None,
            Axis::Horizontal => {}
            Axis::Vertical => self.part = Part::Vertical,
        }
        Some(())
    }

    /// Calls the subroutine the top operand names in `subrs`, one deeper.
    fn call(&mut self, subrs: CffIndex<'_>, depth: Depth) -> Option<()> {
        let depth = depth.deeper()?;
        let i = subr_index(self.stack.pop()?, bias(subrs.len()))?;
        let cs = subrs.get(i)?;
        self.exec(cs, depth)
    }

    /// Runs `cs` to its end: the glyph's own charstring, or a subroutine,
    /// whose end returns to its caller.
    fn exec(&mut self, cs: Bytes<'_>, depth: Depth) -> Option<()> {
        let mut at = Offset::ZERO;
        while let Some(b) = cs.read::<u8>(at) {
            at = at.plus(Len::of(1))?;
            let a = self.stack.args();
            match b {
                op::HSTEM | op::VSTEM | op::HSTEMHM | op::VSTEMHM => {
                    let axis = match b {
                        op::HSTEM | op::HSTEMHM => Axis::Horizontal,
                        _ => Axis::Vertical,
                    };
                    self.stem_in_order(axis)?;
                    let by = match b {
                        op::HSTEM | op::VSTEM => StemOp::Plain,
                        _ => StemOp::Hm,
                    };
                    self.stems(by)?;
                }
                op::HINTMASK | op::CNTRMASK => {
                    // "cm*" comes before "hm*" and the path.
                    if b == op::CNTRMASK
                        && (self.part == Part::Hints || self.pen.path() == Path::Open)
                    {
                        return None;
                    }
                    let bytes = match self.mask {
                        Mask::Unsized => {
                            // Operands before the first mask are vstems, the
                            // implied vstemhm the hints "followed immediately
                            // by a cntrmask or hintmask" may leave out; the
                            // mask is a bit a stem, in whole bytes, from here
                            // on, as HarfBuzz sizes it once.
                            if !self.stack.args().is_empty() {
                                self.stem_in_order(Axis::Vertical)?;
                            }
                            self.stems(StemOp::Hm)?;
                            let bytes = Len::of(self.stems.value().div_ceil(8));
                            self.mask = Mask::Sized(bytes);
                            bytes
                        }
                        // A mask's stack is "[ - ]"; the implied stems are
                        // the first mask's alone.
                        Mask::Sized(_) if !a.is_empty() => return None,
                        Mask::Sized(bytes) => bytes,
                    };
                    if b == op::HINTMASK && self.hinting != Hinting::Hm {
                        return None;
                    }
                    self.part = if b == op::CNTRMASK {
                        Part::Counters
                    } else {
                        Part::Hints
                    };
                    at = at.plus(bytes)?;
                }
                op::RMOVETO => self.pen.rmoveto(a)?,
                op::HMOVETO => self.pen.hmoveto(a)?,
                op::VMOVETO => self.pen.vmoveto(a)?,
                op::RLINETO => self.pen.rlineto(a)?,
                op::HLINETO => self.pen.alternating_lineto(a, Axis::Horizontal)?,
                op::VLINETO => self.pen.alternating_lineto(a, Axis::Vertical)?,
                op::RRCURVETO => self.pen.rrcurveto(a)?,
                op::RCURVELINE => self.pen.rcurveline(a)?,
                op::RLINECURVE => self.pen.rlinecurve(a)?,
                op::VVCURVETO => self.pen.vvcurveto(a)?,
                op::HHCURVETO => self.pen.hhcurveto(a)?,
                op::VHCURVETO => self.pen.alternating_curveto(a, Axis::Vertical)?,
                op::HVCURVETO => self.pen.alternating_curveto(a, Axis::Horizontal)?,
                op::CALLSUBR => {
                    let subrs = self.local;
                    self.call(subrs, depth)?;
                    continue;
                }
                op::CALLGSUBR => {
                    let subrs = self.cff2.global_subrs;
                    self.call(subrs, depth)?;
                    continue;
                }
                op::BLEND => {
                    self.blend()?;
                    continue;
                }
                op::VSINDEX => {
                    self.vsindex()?;
                    continue;
                }
                TWO_BYTE => {
                    let b2: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    match b2 {
                        op::HFLEX => self.pen.hflex(a)?,
                        op::FLEX => self.pen.flex(a)?,
                        op::HFLEX1 => self.pen.hflex1(a)?,
                        op::FLEX1 => self.pen.flex1(a, Flex1::Summed)?,
                        // An operator CFF2 does not define.
                        _ => {}
                    }
                }
                op::SHORTINT => {
                    let n: i16 = cs.read(at)?;
                    at = at.plus(Len::of(2))?;
                    self.stack.push(f64::from(n))?;
                    continue;
                }
                32..=246 => {
                    self.stack.push(f64::from(i16::from(b) - 139))?;
                    continue;
                }
                247..=250 => {
                    let b1: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    self.stack
                        .push(f64::from((i16::from(b) - 247) * 256 + i16::from(b1) + 108))?;
                    continue;
                }
                251..=254 => {
                    let b1: u8 = cs.read(at)?;
                    at = at.plus(Len::of(1))?;
                    self.stack
                        .push(f64::from(-(i16::from(b) - 251) * 256 - i16::from(b1) - 108))?;
                    continue;
                }
                op::FIXED => {
                    let n: u32 = cs.read(at)?;
                    at = at.plus(Len::of(4))?;
                    let n = i32::from_ne_bytes(n.to_ne_bytes());
                    self.stack.push(f64::from(n) / 65536.0)?;
                    continue;
                }
                // Every other byte -- CFF's `endchar` (14) and `return`
                // (11) among them -- is an operator CFF2 does not define,
                // which "is ignored and the stack is cleared".
                _ => {}
            }
            // Every operator that reaches here clears the stack.
            self.stack.clear();
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::var::F2Dot14;
    extern crate alloc;
    use alloc::format;
    use alloc::string::String;
    use alloc::vec;
    use alloc::vec::Vec;

    /// What a charstring drew, a command a string.
    #[derive(Default)]
    struct Drawn(Vec<String>);

    impl Sink for Drawn {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("M {x} {y}"));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push(format!("L {x} {y}"));
        }
        fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
            self.0.push("Q".into());
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            self.0.push(format!("C {x1} {y1} {x2} {y2} {x} {y}"));
        }
        fn close(&mut self) {
            self.0.push("Z".into());
        }
    }

    /// A CFF2 INDEX: a `uint32` count, then four-byte offsets and the data.
    fn index(objects: &[&[u8]]) -> Vec<u8> {
        let mut out = u32::try_from(objects.len()).unwrap().to_be_bytes().to_vec();
        if objects.is_empty() {
            return out;
        }
        out.push(4);
        let mut at = 1u32;
        out.extend(at.to_be_bytes());
        for o in objects {
            at += u32::try_from(o.len()).unwrap();
            out.extend(at.to_be_bytes());
        }
        for o in objects {
            out.extend(*o);
        }
        out
    }

    /// A DICT integer in its five-byte form, so a DICT's length does not
    /// depend on the offsets it holds.
    fn int(v: Offset) -> Vec<u8> {
        let mut out = vec![29];
        out.extend(
            i32::try_from(v.since_start().value())
                .unwrap()
                .to_be_bytes(),
        );
        out
    }

    /// One axis; region 0 peaks at 1, region 1 at 4915 / 16384 (about
    /// 0.3), each from 0 to 1. `sets` lists each data set's regions.
    fn store(sets: &[&[u16]]) -> Vec<u8> {
        let n = sets.len();
        let regions_at = 8 + 4 * n;
        let mut data_at = regions_at + 16;
        let mut out = vec![0, 1];
        out.extend(u32::try_from(regions_at).unwrap().to_be_bytes());
        out.extend(u16::try_from(n).unwrap().to_be_bytes());
        let mut sets_bytes = Vec::new();
        for s in sets {
            out.extend(u32::try_from(data_at).unwrap().to_be_bytes());
            let mut d = vec![0, 0, 0, 0];
            d.extend(u16::try_from(s.len()).unwrap().to_be_bytes());
            for r in *s {
                d.extend(r.to_be_bytes());
            }
            data_at += d.len();
            sets_bytes.extend(d);
        }
        for v in [1i16, 2, 0, 16384, 16384, 0, 4915, 16384] {
            out.extend(v.to_be_bytes());
        }
        out.extend(sets_bytes);
        out
    }

    /// A CFF2 table of `glyphs`, one Font DICT whose Private DICT is
    /// `private` (empty: "0 0"), and `store` when there is one.
    fn table(glyphs: &[&[u8]], private: &[u8], store: Option<&[u8]>) -> Vec<u8> {
        let at = |n: usize| int(Offset::of(u32::try_from(n).unwrap()));
        let top_len = 6 + 7 + if store.is_some() { 6 } else { 0 };
        let cs_at = 5 + top_len + 4;
        let cs = index(glyphs);
        let fda_at = cs_at + cs.len();
        let private_at = fda_at + 4 + 1 + 8 + 11;
        let font_dict = if private.is_empty() {
            [int(Offset::ZERO), int(Offset::ZERO), vec![18]].concat()
        } else {
            [at(private.len()), at(private_at), vec![18]].concat()
        };
        let vs_at = private_at + private.len();
        let mut top = [at(cs_at), vec![17], at(fda_at), vec![12, 36]].concat();
        if store.is_some() {
            top.extend([at(vs_at), vec![24]].concat());
        }
        assert_eq!(top.len(), top_len);
        let mut out = vec![2, 0, 5];
        out.extend(u16::try_from(top_len).unwrap().to_be_bytes());
        out.extend(top);
        out.extend(index(&[]));
        out.extend(cs);
        out.extend(index(&[&font_dict]));
        out.extend(private);
        if let Some(s) = store {
            out.extend(u16::try_from(s.len()).unwrap().to_be_bytes());
            out.extend(s);
        }
        out
    }

    /// Glyph `g`'s commands at `coord` on the one axis, or `None` where the
    /// charstring is refused.
    fn draw(t: &[u8], g: GlyphId, coord: F2Dot14) -> Option<Vec<String>> {
        let cff2 = Cff2::parse(Bytes::new(t), Count::of(2u32)).expect("the table reads");
        let mut coords = Coords::default();
        coords.set_for_test(Index::ZERO, coord);
        let mut d = Drawn::default();
        cff2.outline(g, &coords, &mut d)?;
        Some(d.0)
    }

    // Numbers in charstring bytes: v + 139 for -107..=107.
    const N100: u8 = 239;
    const N50: u8 = 189;
    const N7: u8 = 146;
    const N2: u8 = 141;
    const N1: u8 = 140;
    const N0: u8 = 139;
    const DEFAULT: F2Dot14 = F2Dot14(0);
    const PEAK: F2Dot14 = F2Dot14(16384);

    /// "If an unrecognized operator is encountered while decoding, it is
    /// ignored and the stack is cleared": CFF's `endchar` (14) and `return`
    /// (11), a reserved one-byte operator (0) and a reserved two-byte one
    /// (12 0) each drop the 7s pushed before them, and drawing goes on.
    /// Ending the glyph at 14, refusing it, or keeping the 7s would each
    /// draw something else.
    #[test]
    fn an_operator_cff2_does_not_define_is_ignored_and_clears_the_stack() {
        let cs = [
            N100, N100, 21, // rmoveto 100 100
            N50, N0, 5, // rlineto 50 0
            N7, N7, 14, // endchar in CFF
            N7, 11, // return in CFF
            N7, 0, // reserved
            N7, 12, 0, // reserved, two bytes
            N0, N50, 5, // rlineto 0 50
        ];
        let t = table(&[&cs, &[]], &[], None);
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, DEFAULT).unwrap(),
            ["M 100 100", "L 150 100", "L 150 150", "Z"]
        );
    }

    /// `vsindex` "may only be used once in a CharString and must precede
    /// the first use of the blend operator"; one past the store's data sets
    /// names nothing; and with no VariationStore "the blend and vsindex
    /// operators must not be used". Each refuses its glyph; the control,
    /// one `vsindex` and then a `blend`, draws.
    #[test]
    fn the_variation_operators_keep_the_specifications_order() {
        let s = store(&[&[0], &[0, 1]]);
        // 1 vsindex, then rmoveto's dx of 100 blended by deltas 7 and 7.
        let ok = [N1, 15, N100, N7, N7, N1, 16, N100, 21, N50, N0, 5];
        let t = table(&[&ok, &[]], &[], Some(&s));
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, PEAK).unwrap(),
            ["M 107 100", "L 157 100", "Z"],
            "at region 0's peak and region 1's end"
        );
        let twice = [N1, 15, N1, 15, N100, N100, 21];
        let after = [N100, N7, N1, 16, N1, 15, N100, 21];
        let past = [N2, 15, N100, N100, 21];
        for (name, cs) in [
            ("twice", &twice[..]),
            ("after a blend", &after[..]),
            ("past the store", &past[..]),
        ] {
            let t = table(&[cs, &[]], &[], Some(&s));
            assert_eq!(draw(&t, GlyphId::NOTDEF, PEAK), None, "{name}");
        }
        let blend = [N100, N7, N1, 16, N100, 21];
        let t = table(&[&blend, &[]], &[], None);
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, PEAK),
            None,
            "a blend with no store"
        );
        let vsindex = [N0, 15, N100, N100, 21];
        let t = table(&[&vsindex, &[]], &[], None);
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, DEFAULT),
            None,
            "a vsindex with no store"
        );
    }

    /// A Private DICT `vsindex` past the store's data sets names nothing,
    /// and the table is refused; data set 1 of two reads.
    #[test]
    fn a_private_vsindex_past_the_store_refuses_the_table() {
        let s = store(&[&[0], &[0, 1]]);
        let read = |p: &[u8]| {
            let t = table(&[&[], &[]], p, Some(&s));
            Cff2::parse(Bytes::new(&t), Count::of(2u32)).is_some()
        };
        assert!(read(&[N1, 22]), "data set 1");
        assert!(!read(&[N2, 22]), "data set 2 of two");
    }

    /// A mask's stack is "[ - ]" and the implied `vstemhm` is the first
    /// mask's: operands before a later `hintmask` or `cntrmask` are not a
    /// form the text has, and refuse the glyph. The control, the same
    /// charstring without them, draws -- eight `hstemhm` stems, one mask
    /// byte each time. A later `hintmask` is tried inside the path and
    /// before it; a later `cntrmask` only before it, where "cm*" puts it.
    #[test]
    fn operands_before_a_later_mask_refuse_the_glyph() {
        for (mask, in_path) in [(19, true), (19, false), (20, false)] {
            let charstring = |operands: &[u8]| {
                let mut cs = Vec::new();
                for _ in 0..8 {
                    cs.extend([N0, N7]);
                }
                cs.extend([18, mask, 0xFF]); // hstemhm, the first mask
                if in_path {
                    cs.extend([N100, N100, 21]); // rmoveto 100 100
                }
                cs.extend(operands);
                cs.extend([mask, 0xFF]); // a later mask
                if !in_path {
                    cs.extend([N100, N100, 21]);
                }
                cs.extend([N50, N0, 5]); // rlineto 50 0
                cs
            };
            let (bare, two) = (charstring(&[]), charstring(&[N7, N7]));
            let t = table(&[&bare, &two], &[], None);
            assert_eq!(
                draw(&t, GlyphId::NOTDEF, DEFAULT).unwrap(),
                ["M 100 100", "L 150 100", "Z"],
                "the control, mask {mask}, in the path: {in_path}"
            );
            assert_eq!(
                draw(&t, GlyphId::from_shaper(1), DEFAULT),
                None,
                "two operands before a later mask {mask}, in the path: {in_path}"
            );
        }
    }

    /// `hintmask` "must not be used unless the CharString also defines stems
    /// using hstemhm or vstemhm", and stems from `hstem` or `vstem` "cannot
    /// be used in conjunction with the hintmask operator": a `hintmask` with
    /// no stems, with an `hstemhm` of no operands, or after an `hstem`,
    /// refuses the glyph. The controls draw: `hstemhm` stems, the implied
    /// `vstemhm` alone (the text's own shortened example), and `cntrmask`,
    /// which takes stems of either kind.
    #[test]
    fn a_hintmask_needs_hstemhm_or_vstemhm_stems() {
        let path = [N100, N100, 21, N50, N0, 5];
        let with = |hints: &[u8]| [hints, &path[..]].concat();
        let drawn = ["M 100 100", "L 150 100", "Z"];
        for (name, hints) in [
            ("hstemhm", &[N0, N7, 18, 19, 0x80][..]),
            ("the implied vstemhm", &[N0, N7, 19, 0x80]),
            ("cntrmask after hstem", &[N0, N7, 1, 20, 0x80]),
        ] {
            let t = table(&[&with(hints), &[]], &[], None);
            assert_eq!(
                draw(&t, GlyphId::NOTDEF, DEFAULT).unwrap(),
                drawn,
                "the control: {name}"
            );
        }
        for (name, hints) in [
            ("no stems", &[19][..]),
            ("an hstemhm of no operands", &[18, 19]),
            ("after hstem", &[N0, N7, 1, 19, 0x80]),
            (
                "after vstem and hstemhm",
                &[N0, N7, 18, N0, N7, 3, 19, 0x80],
            ),
        ] {
            let t = table(&[&with(hints), &[]], &[], None);
            assert_eq!(draw(&t, GlyphId::NOTDEF, DEFAULT), None, "{name}");
        }
    }

    /// "A complete CharString has the following structure: hs* vs* cm* hm*
    /// {mt <path constructing data>}*", and "within the path constructing
    /// data, there can be any number of hintmask operations". A stem
    /// operator after the first mask or the first path operator, the stems a
    /// first mask implies after a path operator, an `hstem` or `hstemhm`
    /// after a vertical stem, and a `cntrmask` after a `hintmask` or a path
    /// operator are out of that order, and each refuses the glyph. The
    /// controls draw: every part in order with masks repeated and a
    /// `hintmask` inside the path, `hstem`/`vstem` stems and a `cntrmask`,
    /// and the stems a first mask implies before the path.
    #[test]
    fn hints_out_of_the_texts_order_refuse_the_glyph() {
        let (mt, lt) = ([N100, N100, 21], [N50, N0, 5]);
        let (hs, vs) = ([N0, N7, 18], [N0, N7, 23]); // hstemhm, vstemhm
        let (hstem, vstem) = ([N0, N7, 1], [N0, N7, 3]);
        let drawn = ["M 100 100", "L 150 100", "Z"];
        let masks = [20, 0xC0, 20, 0x40, 19, 0xC0, 19, 0x80];
        for (name, cs) in [
            (
                "every part",
                [&hs[..], &vs, &masks, &mt, &[19, 0x40], &lt].concat(),
            ),
            (
                "hstem, vstem, cntrmask",
                [&hstem[..], &vstem, &[20, 0xC0], &mt, &lt].concat(),
            ),
            (
                "the implied vstemhm",
                [&hs[..], &[N0, N7, 19, 0xC0], &mt, &lt].concat(),
            ),
        ] {
            let t = table(&[&cs, &[]], &[], None);
            assert_eq!(
                draw(&t, GlyphId::NOTDEF, DEFAULT).unwrap(),
                drawn,
                "the control: {name}"
            );
        }
        let mut drew = Vec::new();
        for (name, cs) in [
            (
                "hstem after a hintmask",
                [&hs[..], &[19, 0x80], &hstem, &mt, &lt].concat(),
            ),
            (
                "vstem after a hintmask",
                [&hs[..], &[19, 0x80], &vstem, &mt, &lt].concat(),
            ),
            (
                "vstemhm after a cntrmask",
                [&hs[..], &[20, 0x80], &vs, &mt, &lt].concat(),
            ),
            (
                "hstem after a hintmask and a moveto",
                [&hs[..], &[19, 0x80], &mt, &hstem, &lt].concat(),
            ),
            ("hstemhm after a moveto", [&mt[..], &hs, &lt].concat()),
            (
                "the implied vstemhm after a moveto",
                [&hs[..], &mt, &[N0, N7, 19, 0xC0], &lt].concat(),
            ),
            ("hstem after vstem", [&vstem[..], &hstem, &mt, &lt].concat()),
            ("hstemhm after vstemhm", [&vs[..], &hs, &mt, &lt].concat()),
            (
                "cntrmask after a hintmask",
                [&hs[..], &[19, 0x80, 20, 0x80], &mt, &lt].concat(),
            ),
            (
                "cntrmask after a moveto",
                [&hs[..], &mt, &[20, 0x80], &lt].concat(),
            ),
        ] {
            let t = table(&[&cs, &[]], &[], None);
            if draw(&t, GlyphId::NOTDEF, DEFAULT).is_some() {
                drew.push(name);
            }
        }
        assert_eq!(drew, Vec::<&str>::new(), "drawn out of order");
    }

    /// `vsindex`'s stack is "[ <integer> ]": two operands, or none, refuse
    /// the glyph; one draws.
    #[test]
    fn a_vsindex_takes_exactly_one_operand() {
        let s = store(&[&[0], &[0, 1]]);
        let path = [N100, N100, 21, N50, N0, 5];
        let with = |v: &[u8]| [v, &path[..]].concat();
        let t = table(&[&with(&[N1, 15]), &[]], &[], Some(&s));
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, PEAK).unwrap(),
            ["M 100 100", "L 150 100", "Z"],
            "the control: one operand"
        );
        for (name, v) in [("two", &[N0, N1, 15][..]), ("none", &[15])] {
            let t = table(&[&with(v), &[]], &[], Some(&s));
            assert_eq!(draw(&t, GlyphId::NOTDEF, PEAK), None, "{name}");
        }
    }

    /// Stem operands come in pairs; an odd count refuses the glyph, before a
    /// stem operator and before the mask that implies one alike.
    #[test]
    fn an_odd_stem_operand_count_refuses_the_glyph() {
        let path = [N100, N100, 21, N50, 6];
        let draws = [&[N0, N7, 18][..], &path].concat();
        let odd = [&[N0, N7, N7, 18][..], &path].concat();
        let implied = [&[N0, N7, 18, N7, 19, 0x80][..], &path].concat();
        let t = table(&[&draws, &odd], &[], None);
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, DEFAULT).unwrap(),
            ["M 100 100", "L 150 100", "Z"],
            "the control: one pair"
        );
        assert_eq!(
            draw(&t, GlyphId::from_shaper(1), DEFAULT),
            None,
            "three operands"
        );
        let t = table(&[&implied, &[]], &[], None);
        assert_eq!(
            draw(&t, GlyphId::NOTDEF, DEFAULT),
            None,
            "one before the mask"
        );
    }

    /// A DICT key preceded by other than its one operand leaves the stack
    /// non-empty, and "the subtable is invalid"; the control, one operand,
    /// reads.
    #[test]
    fn a_dict_key_with_extra_operands_is_invalid() {
        let one = [N1, 17];
        let two = [N0, N1, 17];
        assert_eq!(
            key(Bytes::new(&one), dict::CHAR_STRINGS),
            Some(Some(Offset::of(1u32)))
        );
        assert_eq!(key(Bytes::new(&two), dict::CHAR_STRINGS), None);
    }

    /// FDSelect maps every glyph to a Font DICT there is: format 3 starts
    /// at glyph 0 and its sentinel is the glyph count; format 0 has an entry
    /// a glyph.
    #[test]
    fn an_fdselect_that_does_not_cover_the_glyphs_is_refused() {
        let covers = |b: &[u8], glyphs: u32, fds: u32| {
            FdSelect::parse(Bytes::new(b), Offset::ZERO, crate::cff::FdFormats::Cff2)
                .and_then(|s| s.covers(Count::of(glyphs), Count::of(fds)))
                .is_some()
        };
        // Format 3: glyphs 0-1 to Font DICT 0, glyph 2 to 1, sentinel 3.
        let ok = [3, 0, 2, 0, 0, 0, 0, 2, 1, 0, 3];
        assert!(covers(&ok, 3, 2), "the control");
        assert!(!covers(&ok, 4, 2), "a sentinel short of the glyph count");
        assert!(!covers(&ok, 3, 1), "a Font DICT past the FDArray");
        let late = [3, 0, 2, 0, 1, 0, 0, 2, 1, 0, 3];
        assert!(!covers(&late, 3, 2), "a first range past glyph 0");
        assert!(covers(&[0, 0, 1, 0], 3, 2), "format 0, three entries");
        assert!(!covers(&[0, 0, 1], 3, 2), "format 0, two entries for three");
        assert!(
            !covers(&[0, 0, 2, 0], 3, 2),
            "format 0, a Font DICT past it"
        );
        // Ranges (0, 0), (2, 1), (1, 0): not in increasing order.
        let unordered = [3, 0, 3, 0, 0, 0, 0, 2, 1, 0, 1, 0, 0, 3];
        assert!(!covers(&unordered, 3, 2), "ranges out of order");
        // Ranges (0, 0), (3, 1), sentinel 3: the last range is past the
        // glyphs, and not below the sentinel.
        let empty_last = [3, 0, 2, 0, 0, 0, 0, 3, 1, 0, 3];
        assert!(!covers(&empty_last, 3, 2), "a last range at the sentinel");
        // Format 4: glyphs 0-1 to Font DICT 0, glyph 2 to 1, sentinel 3.
        let four = [
            4, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 0, 0, 3,
        ];
        assert!(covers(&four, 3, 2), "the control, format 4");
        assert!(!covers(&four, 4, 2), "format 4, a sentinel short");
        assert!(!covers(&four, 3, 1), "format 4, a Font DICT past it");
    }

    /// A CFF2 table of two empty glyphs with `font_dicts` (each a whole Font
    /// DICT) and `fd_select` when there is one, no store.
    fn table_of(font_dicts: &[&[u8]], fd_select: Option<&[u8]>) -> Vec<u8> {
        let at = |n: usize| int(Offset::of(u32::try_from(n).unwrap()));
        let top_len = 6 + 7 + if fd_select.is_some() { 7 } else { 0 };
        let cs_at = 5 + top_len + 4;
        let cs = index(&[&[], &[]]);
        let fda_at = cs_at + cs.len();
        let fda = index(font_dicts);
        let fds_at = fda_at + fda.len();
        let mut top = [at(cs_at), vec![17], at(fda_at), vec![12, 36]].concat();
        if fd_select.is_some() {
            top.extend([at(fds_at), vec![12, 37]].concat());
        }
        assert_eq!(top.len(), top_len);
        let mut out = vec![2, 0, 5];
        out.extend(u16::try_from(top_len).unwrap().to_be_bytes());
        out.extend(top);
        out.extend(index(&[]));
        out.extend(cs);
        out.extend(fda);
        if let Some(f) = fd_select {
            out.extend(f);
        }
        out
    }

    /// "If there is more than one FontDICT ... a FontDICTSelect table must
    /// be used", and with one "the FontDICTSelectOffset operator must not
    /// be used"; the FDSelect, when there is one, must map both glyphs to a
    /// Font DICT there is. The controls read: one Font DICT and no FDSelect,
    /// two and a format 3 FDSelect that covers.
    #[test]
    fn an_fdselect_is_present_exactly_when_there_are_two_font_dicts() {
        let empty = [int(Offset::ZERO), int(Offset::ZERO), vec![18]].concat();
        let reads = |fds: &[&[u8]], sel: Option<&[u8]>| {
            Cff2::parse(Bytes::new(&table_of(fds, sel)), Count::of(2u32)).is_some()
        };
        // Format 3: glyph 0 to Font DICT 0, glyph 1 to 1, sentinel 2.
        let covers = [3, 0, 2, 0, 0, 0, 0, 1, 1, 0, 2];
        let short = [3, 0, 2, 0, 0, 0, 0, 1, 1, 0, 3];
        assert!(reads(&[&empty], None), "the control: one, none");
        assert!(reads(&[&empty, &empty], Some(&covers)), "the control: two");
        assert!(!reads(&[&empty, &empty], None), "two Font DICTs, none");
        // Format 3: both glyphs to Font DICT 0, sentinel 2.
        let to_zero = [3, 0, 1, 0, 0, 0, 0, 2];
        assert!(
            !reads(&[&empty], Some(&to_zero)),
            "one Font DICT, an FDSelect that maps every glyph to it"
        );
        assert!(
            !reads(&[&empty, &empty], Some(&short)),
            "two, an FDSelect whose sentinel is not the glyph count"
        );
    }

    /// PrivateDICTOffset is "[ size offset ]": a Font DICT whose key has
    /// one operand or three is invalid; two reads.
    #[test]
    fn a_private_dict_offset_takes_exactly_two_operands() {
        let reads =
            |fd: &[u8]| Cff2::parse(Bytes::new(&table_of(&[fd], None)), Count::of(2u32)).is_some();
        let z = || int(Offset::ZERO);
        assert!(reads(&[z(), z(), vec![18]].concat()), "the control: two");
        assert!(!reads(&[z(), vec![18]].concat()), "one");
        assert!(!reads(&[z(), z(), z(), vec![18]].concat()), "three");
    }

    /// The refusal says which table, in words a launch message can carry.
    #[test]
    fn a_refused_cff2_table_is_named() {
        let e = crate::Error::Malformed(crate::Tag::CFF2);
        assert_eq!(format!("{e}"), "its 'CFF2' table does not read");
        let e = crate::Error::MissingTable(crate::Tag::HEAD);
        assert_eq!(format!("{e}"), "no 'head' table");
    }

    /// An empty Private DICT ("0 0") has no subroutines: a glyph that calls
    /// one is refused, and one that does not draws.
    #[test]
    fn an_empty_private_dict_has_no_subroutines() {
        let calls = [N0, 10];
        let draws = [N100, N100, 21, N50, 6];
        let t = table(&[&calls, &draws], &[], None);
        assert_eq!(draw(&t, GlyphId::NOTDEF, DEFAULT), None);
        assert_eq!(
            draw(&t, GlyphId::from_shaper(1), DEFAULT).unwrap(),
            ["M 100 100", "L 150 100", "Z"]
        );
    }

    /// A blend sums a value's deltas, each times its scalar, from 0, and
    /// adds the sum to the default -- HarfBuzz's order. Adding each product
    /// to the default in turn rounds differently for these values, which
    /// the guard checks, so the order is what the assertion pins.
    #[test]
    fn a_blend_adds_its_summed_deltas_to_the_default() {
        let (a, d0, d1) = (
            -3000.0 / 65536.0 + 1000.123456,
            2147483633.0 / 65536.0,
            1073741819.0 / 65536.0,
        );
        let (s0, s1) = (292.0f32 / 16384.0, 292.0f32 / 4915.0);
        let summed = a + (0.0 + f64::from(s0) * d0 + f64::from(s1) * d1);
        let eager = (a + f64::from(s0) * d0) + f64::from(s1) * d1;
        assert_ne!(
            summed.to_bits(),
            eager.to_bits(),
            "the guard: the orders differ"
        );

        let s = store(&[&[0, 1]]);
        let t = table(&[&[], &[]], &[], Some(&s));
        let cff2 = Cff2::parse(Bytes::new(&t), Count::of(2u32)).unwrap();
        let coords = Coords::default();
        let mut sink = Drawn::default();
        let mut run = Run {
            cff2: &cff2,
            coords: &coords,
            pen: Pen::new(&mut sink),
            stack: Stack::new(),
            stems: Count::ZERO,
            local: CffIndex::EMPTY,
            vsindex: Index::ZERO,
            variation: Variation::Unset,
            regions: Regions::Taken(Count::of(2u32)),
            mask: Mask::Unsized,
            hinting: Hinting::Unhinted,
            part: Part::Horizontal,
            scalars: [0.0; MAX_REGIONS],
        };
        run.scalars[0] = s0;
        run.scalars[1] = s1;
        for v in [a, d0, d1, 1.0] {
            run.stack.push(v).unwrap();
        }
        run.blend().unwrap();
        assert_eq!(run.stack.args().len(), 1);
        assert_eq!(run.stack.args()[0].to_bits(), summed.to_bits());
    }
}
