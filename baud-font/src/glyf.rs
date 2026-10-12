//! `loca` and `glyf`: TrueType outlines (`[[source: ms-opentype-loca]]`,
//! `[[source: ms-opentype-glyf]]`).
//!
//! **The emission rule is the one every reader agrees on**, and it is what
//! makes an exact differential possible: start a contour at its first
//! on-curve point; two consecutive off-curve points imply an on-curve point at
//! their midpoint; a contour that starts off-curve starts at the midpoint of
//! its first two points if both are off-curve, or at the point after; and the
//! contour closes back to where it began, through the last control point if
//! one is pending. `ttf-parser` spells that rule the same way this file does,
//! and `tests/oracle.rs` holds the two to bit-identical output.
//!
//! **What is bounded.** Composite recursion at 32 deep -- the specification
//! sets no limit and a malformed font can point a component at itself; a
//! `loca` entry is used only if it is ascending and inside `glyf`; every
//! coordinate array's length is derived from the flags before a point is
//! read, so a lying flag cannot walk past the glyph.
//!
//! **How a component is placed** is the specification's
//! (`[[source: ms-opentype-glyf]]`, "Composite glyph description"). An
//! offset is added after the component's matrix, or, with
//! SCALED_COMPONENT_OFFSET alone, taken through it -- "deemed to be in the
//! component glyph's coordinate system". With neither flag the text lets a
//! rasteriser choose and recommends the unscaled offset, which Microsoft's,
//! Apple's (by that text), HarfBuzz's and FreeType's defaults take; Apple's
//! own reference (`[[source: apple-ttrm-glyf]]`) scales it by its `m` and
//! `n`, so the choice is recorded for Jev and the unscaled one kept, for
//! both flags set too, which the text calls invalid and gives the default.
//! A component whose arguments name points -- ARGS_ARE_XY_VALUES clear --
//! is aligned after its matrix: the parent's point, numbered across the
//! components before it and then the parent's own left and right
//! phantoms, onto the child's, its horizontal phantoms included. That
//! needs the parent's points, which a stream into a sink has given away,
//! so a glyph whose tree matches points is drawn only through scratch, by
//! the walk a varied glyph takes ([`Glyf::outline`] refuses it with the
//! need). What the text leaves open stays at offset 0, as every matched
//! component was before: a point past the parent's phantoms, a vertical
//! phantom of either glyph (no `vmtx` is read), and a first component that
//! is not an offset. A composite's own phantoms take the deltas
//! `[[source: ms-opentype-gvar]]` gives them -- X on the left and advance
//! ones, Y on the two vertical ones -- which a matched component sees once
//! the composite is its parent or its child. `ROUND_XY_TO_GRID` is a
//! grid-fitting rule, and nothing here fits to a grid.

use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::gvar::{Gvar, Referenced, Track};
use crate::outline::{Sink, Transform};
use crate::var::Coords as VarCoords;
use baud_domain::GlyphId;

const MAX_DEPTH: u8 = 32;

/// Glyphs an outline may visit, components counted each time they are
/// reached: HarfBuzz's `HB_MAX_GRAPH_EDGE_COUNT`
/// (`[[source: harfbuzz-12-3-0-limits]]`), on both paths. A fan-out of
/// shared components is exponential in the depth -- two components a level,
/// 32 levels deep, is 2^32 visits -- so depth alone does not bound a walk
/// over a hostile font. HarfBuzz checks the count before it counts a visit
/// (`Glyph::get_points` in `[[source: harfbuzz-12-3-0-glyph]]`), so 2,049
/// visits draw and the 2,050th refuses the glyph.
const MAX_EDGES: u32 = 2048;

/// One point of a varied glyph, in the caller's scratch: its position, its
/// flags, and the variation state `gvar` keeps per point while it applies a
/// glyph's tuples.
#[derive(Clone, Copy, Debug)]
pub struct VarPoint {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) on: OnCurve,
    pub(crate) last: LastPoint,
    pub(crate) emit: Emit,
    pub(crate) ox: f32,
    pub(crate) oy: f32,
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    pub(crate) referenced: Referenced,
}

impl VarPoint {
    pub const EMPTY: VarPoint = VarPoint {
        x: 0.0,
        y: 0.0,
        on: OnCurve::Yes,
        last: LastPoint::No,
        emit: Emit::No,
        ox: 0.0,
        oy: 0.0,
        dx: 0.0,
        dy: 0.0,
        referenced: Referenced::No,
    };

    fn at(x: f32, y: f32, on: OnCurve, last: LastPoint, emit: Emit) -> VarPoint {
        VarPoint {
            x,
            y,
            on,
            last,
            emit,
            ..VarPoint::EMPTY
        }
    }
}

/// Whether a point reaches the contour builder. Phantom points and a
/// component's offset point never do, and neither does the lone point of a
/// one-point glyph, which the default path draws nothing for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Emit {
    Yes,
    No,
}

/// The four phantom points, in HarfBuzz's order: left, right, top, bottom.
const PHANTOMS: u32 = 4;

/// What one glyph's walk needs, in points: what it appends to the flattened
/// list (its four phantom points included), and the most the component
/// stack holds. The list never grows past what it keeps: each child's
/// phantoms are dropped before the next child is walked, and the last one's
/// end exactly where the composite's own land.
#[derive(Clone, Copy)]
struct Reach {
    kept: Count,
    stack: Count,
    matched: Matched,
}

/// Whether a glyph's tree places a component by matched points anywhere,
/// so that it can be drawn only through scratch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Matched {
    No,
    Yes,
}

/// What [`Glyf::outline`] did with a glyph it parsed: streamed it, or
/// refused it because its tree matches points, with the scratch the walk
/// that draws it needs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Streamed {
    Drawn,
    NeedsScratch(Count),
}

/// A glyph's advance and left side bearing, from `hmtx`, which places its
/// first two phantom points.
pub type Hmtx<'h> = &'h dyn Fn(GlyphId) -> Option<(i32, i32)>;

/// The walk's shared state: the gvar table when the face is read at a
/// variation, the coordinates, `hmtx`, and how many glyphs it has visited.
struct Walk<'v, 'a> {
    gvar: Option<Gvar<'a>>,
    coords: &'v VarCoords,
    hmtx: Hmtx<'v>,
    edges: Edges,
}

/// How many glyphs a walk has visited, against [`MAX_EDGES`].
#[derive(Clone, Copy)]
struct Edges(u32);

impl Edges {
    /// Counts one more visit; `None` past the bound, checked before the
    /// visit is counted as HarfBuzz checks it.
    fn visit(&mut self) -> Option<()> {
        if self.0 > MAX_EDGES {
            return None;
        }
        self.0 += 1;
        Some(())
    }
}

/// `head.indexToLocFormat`: two-byte halved offsets, or four-byte offsets.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LocaFormat {
    Short,
    Long,
}

#[derive(Clone, Copy)]
pub struct Loca<'a> {
    data: Bytes<'a>,
    format: LocaFormat,
    entries: Count,
}

impl<'a> Loca<'a> {
    /// `numGlyphs + 1` entries, or as many as the table actually holds --
    /// a short table is read as far as it goes rather than refused.
    pub fn parse(data: Bytes<'a>, format: LocaFormat, glyphs: Count) -> Loca<'a> {
        let size = match format {
            LocaFormat::Short => 2u32,
            LocaFormat::Long => 4u32,
        };
        let available = Count::of(len_value(data.len()) / size);
        let wanted = glyphs.as_index().next();
        let entries = if available.holds(wanted) {
            Count::of(index_value(wanted))
        } else {
            available
        };
        Loca {
            data,
            format,
            entries,
        }
    }

    fn entry(&self, i: Index) -> Option<Offset> {
        match self.format {
            LocaFormat::Short => {
                let arr: Array<'a, u16> = self.data.array(Offset::ZERO, self.entries)?;
                Some(Offset::of(u32::from(arr.get(i)?).checked_mul(2)?))
            }
            LocaFormat::Long => {
                let arr: Array<'a, u32> = self.data.array(Offset::ZERO, self.entries)?;
                Some(Offset::of(arr.get(i)?))
            }
        }
    }

    /// Where glyph `g` lives in `glyf`, or `None` when the entry is absent,
    /// empty or descending.
    pub fn range(&self, g: GlyphId) -> Option<(Offset, Len)> {
        let i = Index::of(u32::try_from(g.index()).ok()?);
        let next = i.next();
        if !self.entries.holds(next) {
            return None;
        }
        let start = self.entry(i)?;
        let end = self.entry(next)?;
        let len = end.since(start)?;
        (!len.is_zero()).then_some((start, len))
    }

    /// As [`Loca::range`], but an empty glyph is an empty range rather than
    /// none, for the bake, which must tell it from an absent one.
    pub(crate) fn range_or_empty(&self, g: GlyphId) -> Option<(Offset, Len)> {
        let i = Index::of(u32::try_from(g.index()).ok()?);
        let next = i.next();
        if !self.entries.holds(next) {
            return None;
        }
        let start = self.entry(i)?;
        Some((start, self.entry(next)?.since(start)?))
    }
}

fn len_value(l: Len) -> u32 {
    l.value()
}

fn index_value(i: Index) -> u32 {
    i.value()
}

/// The contour-emission state of one simple glyph, or one component.
struct Contour<'s, S: Sink> {
    sink: &'s mut S,
    transform: Transform,
    first_on: Option<(f32, f32)>,
    first_off: Option<(f32, f32)>,
    last_off: Option<(f32, f32)>,
}

fn mid(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    // `a + 0.5 * (b - a)`, the reference's lerp, kept in that form so the
    // rounding is the same rounding.
    (a.0 + 0.5 * (b.0 - a.0), a.1 + 0.5 * (b.1 - a.1))
}

impl<S: Sink> Contour<'_, S> {
    fn tx(&self, p: (f32, f32)) -> (f32, f32) {
        if self.transform.is_identity() {
            p
        } else {
            self.transform.apply(p.0, p.1)
        }
    }

    fn move_to(&mut self, p: (f32, f32)) {
        let p = self.tx(p);
        self.sink.move_to(p.0, p.1);
    }

    fn line_to(&mut self, p: (f32, f32)) {
        let p = self.tx(p);
        self.sink.line_to(p.0, p.1);
    }

    fn quad_to(&mut self, c: (f32, f32), p: (f32, f32)) {
        let c = self.tx(c);
        let p = self.tx(p);
        self.sink.quad_to(c.0, c.1, p.0, p.1);
    }

    fn push(&mut self, p: (f32, f32), on_curve: OnCurve, last: LastPoint) {
        if self.first_on.is_none() {
            if on_curve == OnCurve::Yes {
                self.first_on = Some(p);
                self.move_to(p);
            } else if let Some(off) = self.first_off {
                let m = mid(off, p);
                self.first_on = Some(m);
                self.last_off = Some(p);
                self.move_to(m);
            } else {
                self.first_off = Some(p);
            }
        } else {
            match (self.last_off, on_curve) {
                (Some(off), OnCurve::Yes) => {
                    self.last_off = None;
                    self.quad_to(off, p);
                }
                (Some(off), OnCurve::No) => {
                    self.last_off = Some(p);
                    let m = mid(off, p);
                    self.quad_to(off, m);
                }
                (None, OnCurve::Yes) => self.line_to(p),
                (None, OnCurve::No) => self.last_off = Some(p),
            }
        }
        if last == LastPoint::Yes {
            self.finish();
        }
    }

    fn finish(&mut self) {
        if let (Some(off1), Some(off2)) = (self.first_off, self.last_off) {
            self.last_off = None;
            let m = mid(off2, off1);
            self.quad_to(off2, m);
        }
        if let (Some(p), Some(off1)) = (self.first_on, self.first_off) {
            self.quad_to(off1, p);
        } else if let (Some(p), Some(off2)) = (self.first_on, self.last_off) {
            self.quad_to(off2, p);
        } else if let Some(p) = self.first_on {
            self.line_to(p);
        }
        self.first_on = None;
        self.first_off = None;
        self.last_off = None;
        self.sink.close();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OnCurve {
    Yes,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LastPoint {
    Yes,
    No,
}

/// A simple glyph's flag byte.
#[derive(Clone, Copy)]
pub(crate) struct Flags(pub(crate) u8);

impl Flags {
    pub(crate) fn on_curve(self) -> OnCurve {
        if self.0 & 0x01 != 0 {
            OnCurve::Yes
        } else {
            OnCurve::No
        }
    }
    fn x_short(self) -> bool {
        self.0 & 0x02 != 0
    }
    fn y_short(self) -> bool {
        self.0 & 0x04 != 0
    }
    fn repeat(self) -> bool {
        self.0 & 0x08 != 0
    }
    fn x_same_or_positive(self) -> bool {
        self.0 & 0x10 != 0
    }
    fn y_same_or_positive(self) -> bool {
        self.0 & 0x20 != 0
    }
}

/// A composite record's flag word.
#[derive(Clone, Copy)]
pub(crate) struct Component(pub(crate) u16);

impl Component {
    pub(crate) fn words(self) -> bool {
        self.0 & 0x0001 != 0
    }
    pub(crate) fn xy_values(self) -> bool {
        self.0 & 0x0002 != 0
    }
    pub(crate) fn scale(self) -> bool {
        self.0 & 0x0008 != 0
    }
    fn more(self) -> bool {
        self.0 & 0x0020 != 0
    }
    pub(crate) fn xy_scale(self) -> bool {
        self.0 & 0x0040 != 0
    }
    pub(crate) fn two_by_two(self) -> bool {
        self.0 & 0x0080 != 0
    }
    fn use_my_metrics(self) -> bool {
        self.0 & 0x0200 != 0
    }
    /// SCALED_COMPONENT_OFFSET (bit 11) without UNSCALED_COMPONENT_OFFSET
    /// (bit 12): the offset is in the component's coordinates and goes
    /// through its matrix. Neither bit, or both, which the text calls
    /// invalid, is the default, the unscaled offset (open for Jev).
    pub(crate) fn scaled_offset(self) -> bool {
        self.0 & 0x1800 == 0x0800
    }
}

/// How a composite record places its component: by the offset in its
/// transform's `e` and `f`, or by aligning the parent's point onto the
/// child's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Place {
    Offset,
    Points { parent: Index, child: Index },
}

/// One composite record, decoded: its flags, its glyph, and its transform
/// with the x/y offset in `e` and `f` (zero for a component placed by
/// points, whose arguments are in `place`).
#[derive(Clone, Copy)]
pub(crate) struct Comp {
    pub(crate) flags: Component,
    pub(crate) glyph: GlyphId,
    pub(crate) t: Transform,
    pub(crate) place: Place,
    /// Where the record ends in the glyph's body: after the last one, a
    /// composite's instructions follow.
    pub(crate) end: Offset,
}

/// The records of a composite glyph, in order. The one decoder of them: the
/// default path composes each record's transform into its parent's, the
/// varied path takes the offset apart so a delta can move it.
pub(crate) struct Comps<'a> {
    pub(crate) body: Bytes<'a>,
    pub(crate) at: Option<Offset>,
}

impl<'a> Comps<'a> {
    /// The next record; `Some(None)` when the list is done, `None` when it
    /// is malformed.
    pub(crate) fn next(&mut self) -> Option<Option<Comp>> {
        let Some(mut at) = self.at else {
            return Some(None);
        };
        let body = self.body;
        let flags = Component(body.read(at)?);
        let glyph: u16 = body.read(at.plus(Len::of(2))?)?;
        at = at.plus(Len::of(4))?;
        let mut t = Transform::IDENTITY;
        let mut place = Place::Offset;
        if flags.words() {
            if flags.xy_values() {
                let e: i16 = body.read(at)?;
                let f: i16 = body.read(at.plus(Len::of(2))?)?;
                t.e = f32::from(e);
                t.f = f32::from(f);
            } else {
                let parent: u16 = body.read(at)?;
                let child: u16 = body.read(at.plus(Len::of(2))?)?;
                place = Place::Points {
                    parent: Index::of(parent),
                    child: Index::of(child),
                };
            }
            at = at.plus(Len::of(4))?;
        } else {
            if flags.xy_values() {
                let e: u8 = body.read(at)?;
                let f: u8 = body.read(at.plus(Len::of(1))?)?;
                t.e = f32::from(i8::from_ne_bytes([e]));
                t.f = f32::from(i8::from_ne_bytes([f]));
            } else {
                let parent: u8 = body.read(at)?;
                let child: u8 = body.read(at.plus(Len::of(1))?)?;
                place = Place::Points {
                    parent: Index::of(parent),
                    child: Index::of(child),
                };
            }
            at = at.plus(Len::of(2))?;
        }
        if flags.two_by_two() {
            let a: i16 = body.read(at)?;
            let b: i16 = body.read(at.plus(Len::of(2))?)?;
            let c: i16 = body.read(at.plus(Len::of(4))?)?;
            let d: i16 = body.read(at.plus(Len::of(6))?)?;
            t.a = f2dot14(a);
            t.b = f2dot14(b);
            t.c = f2dot14(c);
            t.d = f2dot14(d);
            at = at.plus(Len::of(8))?;
        } else if flags.xy_scale() {
            let a: i16 = body.read(at)?;
            let d: i16 = body.read(at.plus(Len::of(2))?)?;
            t.a = f2dot14(a);
            t.d = f2dot14(d);
            at = at.plus(Len::of(4))?;
        } else if flags.scale() {
            let a: i16 = body.read(at)?;
            t.a = f2dot14(a);
            t.d = t.a;
            at = at.plus(Len::of(2))?;
        }
        self.at = flags.more().then_some(at);
        Some(Some(Comp {
            flags,
            glyph: GlyphId::from_shaper(glyph),
            t,
            place,
            end: at,
        }))
    }
}

impl Comp {
    /// The transform that places the component in its parent when it is
    /// placed by an offset: the matrix, then the offset -- through the
    /// matrix first under SCALED_COMPONENT_OFFSET, so the offset is the
    /// matrix applied to it, `(a·e + c·f, b·e + d·f)`.
    fn offset_transform(&self) -> Transform {
        if !self.flags.scaled_offset() {
            return self.t;
        }
        let (e, f) = Transform {
            e: 0.0,
            f: 0.0,
            ..self.t
        }
        .apply(self.t.e, self.t.f);
        Transform { e, f, ..self.t }
    }
}

/// A 2.14 fixed-point value, as composite scales are stored.
fn f2dot14(v: i16) -> f32 {
    f32::from(v) / 16384.0
}

/// Walks the flags once to learn how long the two coordinate arrays are,
/// which the file does not say. Returns `(x bytes, y bytes, flag bytes)`.
fn coord_lengths(flags: Bytes<'_>, points: u32) -> Option<(Len, Len, Len)> {
    let mut left = points;
    let mut at = Offset::ZERO;
    let mut xs = 0u32;
    let mut ys = 0u32;
    while left > 0 {
        let f = Flags(flags.read(at)?);
        at = at.plus(Len::of(1))?;
        let repeats = if f.repeat() {
            let r: u8 = flags.read(at)?;
            at = at.plus(Len::of(1))?;
            u32::from(r).checked_add(1)?
        } else {
            1
        };
        if repeats > left {
            return None;
        }
        if f.x_short() {
            xs = xs.checked_add(repeats)?;
        } else if !f.x_same_or_positive() {
            xs = xs.checked_add(repeats.checked_mul(2)?)?;
        }
        if f.y_short() {
            ys = ys.checked_add(repeats)?;
        } else if !f.y_same_or_positive() {
            ys = ys.checked_add(repeats.checked_mul(2)?)?;
        }
        left -= repeats;
    }
    Some((Len::of(xs), Len::of(ys), at.since_start()))
}

/// One coordinate stream, decoded delta by delta.
struct Coords<'a> {
    data: Bytes<'a>,
    at: Offset,
    prev: i16,
}

/// One coordinate's encoding, from its two flag bits: one byte with the sign
/// in the flag, no bytes and the previous value repeated, or two bytes signed.
#[derive(Clone, Copy)]
enum Delta {
    Short(Sign),
    Same,
    Long,
}

#[derive(Clone, Copy)]
enum Sign {
    Positive,
    Negative,
}

impl Flags {
    fn x_delta(self) -> Delta {
        Delta::of(self.x_short(), self.x_same_or_positive())
    }
    fn y_delta(self) -> Delta {
        Delta::of(self.y_short(), self.y_same_or_positive())
    }
}

impl Delta {
    fn of(short: bool, same_or_positive: bool) -> Delta {
        match (short, same_or_positive) {
            (true, true) => Delta::Short(Sign::Positive),
            (true, false) => Delta::Short(Sign::Negative),
            (false, true) => Delta::Same,
            (false, false) => Delta::Long,
        }
    }
}

impl Coords<'_> {
    fn next(&mut self, delta: Delta) -> i16 {
        // Past the end reads as zero: the lengths were derived from the flags
        // and checked against the glyph, so this is the reference's fallback
        // and not a live path.
        let n: i16 = match delta {
            Delta::Short(sign) => {
                let b: u8 = self.data.read(self.at).unwrap_or(0);
                self.at = self.at.plus(Len::of(1)).unwrap_or(self.at);
                match sign {
                    Sign::Positive => i16::from(b),
                    Sign::Negative => -i16::from(b),
                }
            }
            Delta::Same => 0,
            Delta::Long => {
                let v: i16 = self.data.read(self.at).unwrap_or(0);
                self.at = self.at.plus(Len::of(2)).unwrap_or(self.at);
                v
            }
        };
        self.prev = self.prev.wrapping_add(n);
        self.prev
    }
}

/// A simple glyph's point data, located: the one decoder of it, which the
/// default path streams into a contour and the varied path copies into
/// scratch.
#[derive(Clone, Copy)]
pub(crate) struct Simple<'a> {
    pub(crate) ends: Array<'a, u16>,
    pub(crate) points: Count,
    /// The instruction bytes, which a writer copies through.
    pub(crate) instructions: Bytes<'a>,
    flags: Bytes<'a>,
    x_coords: Bytes<'a>,
    y_coords: Bytes<'a>,
}

impl<'a> Simple<'a> {
    pub(crate) fn parse(body: Bytes<'a>, contours: Count) -> Option<Simple<'a>> {
        let ends: Array<'a, u16> = body.array(Offset::ZERO, contours)?;
        let last_end = ends.get(contours.last()?)?;
        let points = Count::of(u32::from(last_end).checked_add(1)?);
        let after_ends = Offset::ZERO.at(contours.as_index(), Len::of(2))?;
        let instructions: u16 = body.read(after_ends)?;
        let flags_at = after_ends
            .plus(Len::of(2))?
            .plus(Len::of(u32::from(instructions)))?;
        let program = body.slice(
            after_ends.plus(Len::of(2))?,
            Len::of(u32::from(instructions)),
        )?;
        let flags = body.tail(flags_at)?;
        let (xs, ys, flag_bytes) = coord_lengths(flags, points.value())?;
        let flags = flags.slice(Offset::ZERO, flag_bytes)?;
        let x_at = flags_at.plus(flag_bytes)?;
        let y_at = x_at.plus(xs)?;
        Some(Simple {
            ends,
            points,
            instructions: program,
            flags,
            x_coords: body.slice(x_at, xs)?,
            y_coords: body.slice(y_at, ys)?,
        })
    }

    /// Every point, in order, with whether it is on the curve and whether it
    /// ends its contour.
    pub(crate) fn each(&self, mut f: impl FnMut((f32, f32), OnCurve, LastPoint)) {
        self.each_flag(|p, fl, last| f(p, fl.on_curve(), last));
    }

    /// Every point with its stored flag byte, which a writer carries the
    /// on-curve, overlap and cubic bits of.
    pub(crate) fn each_flag(&self, mut f: impl FnMut((f32, f32), Flags, LastPoint)) {
        let ends = self.ends;
        let mut xc = Coords {
            data: self.x_coords,
            at: Offset::ZERO,
            prev: 0,
        };
        let mut yc = Coords {
            data: self.y_coords,
            at: Offset::ZERO,
            prev: 0,
        };
        // Endpoints as a per-point "last" flag, the reference's iterator: the
        // first contour ends at endPts[0]; each later one at endPts[k], with
        // a malformed descending list read as an empty contour.
        let mut end_index = Index::ZERO;
        let mut left = ends.get(Index::ZERO).map_or(0, u32::from);
        let mut flag_at = Offset::ZERO;
        let mut repeats: u8 = 0;
        let mut fl = Flags(0);
        for _ in 0..self.points.value() {
            let last = if left == 0 {
                end_index = end_index.next();
                if let Some(end) = ends.get(end_index) {
                    let prev = end_index.prev().and_then(|p| ends.get(p)).unwrap_or(0);
                    left = u32::from(end.saturating_sub(prev).saturating_sub(1));
                }
                LastPoint::Yes
            } else {
                left -= 1;
                LastPoint::No
            };
            if repeats == 0 {
                fl = Flags(self.flags.read(flag_at).unwrap_or(0));
                flag_at = flag_at.plus(Len::of(1)).unwrap_or(flag_at);
                if fl.repeat() {
                    repeats = self.flags.read(flag_at).unwrap_or(0);
                    flag_at = flag_at.plus(Len::of(1)).unwrap_or(flag_at);
                }
            } else {
                repeats -= 1;
            }
            let x = xc.next(fl.x_delta());
            let y = yc.next(fl.y_delta());
            f((f32::from(x), f32::from(y)), fl, last);
        }
    }
}

enum Kind<'a> {
    Simple(Simple<'a>),
    Empty,
    Composite(Bytes<'a>),
}

/// A glyph as the phantom walk sees it: how many points of its own come
/// before its phantoms, and a composite's records.
enum Phantom<'a> {
    Empty,
    Points(Count),
    Composite(Bytes<'a>),
}

/// Four phantom points appended at `at`, where HarfBuzz starts them: left
/// at `(xMin - lsb, 0)` and right an advance along it, from `left` -- the
/// specification's "left sidebearing point" and "advance width point"
/// (`[[source: ms-opentype-tt-instructing-glyphs]]`) -- and the two
/// vertical ones at 0, since no `vmtx` is read. None is drawn and none is
/// a contour end; a matched component may align on the horizontal two, and
/// only the left one's delta moves the outline.
fn push_phantoms(pts: &mut [VarPoint], at: &mut usize, left: (i32, i32)) -> Option<()> {
    let (x, advance) = left;
    let place = [(x, 0), (x.checked_add(advance)?, 0), (0, 0), (0, 0)];
    for (px, py) in place {
        *pts.get_mut(*at)? =
            VarPoint::at(px as f32, py as f32, OnCurve::Yes, LastPoint::No, Emit::No);
        *at += 1;
    }
    Some(())
}

/// The table pair; `outline` is the entry point.
#[derive(Clone, Copy)]
pub struct Glyf<'a> {
    loca: Loca<'a>,
    data: Bytes<'a>,
}

impl<'a> Glyf<'a> {
    pub fn new(loca: Loca<'a>, data: Bytes<'a>) -> Glyf<'a> {
        Glyf { loca, data }
    }

    pub(crate) fn glyph(&self, g: GlyphId) -> Option<Bytes<'a>> {
        let (at, len) = self.loca.range(g)?;
        self.data.slice(at, len)
    }

    /// Glyph `g`'s bytes, empty for an empty glyph; `None` when `loca` has
    /// no entry for it or names bytes past `glyf`.
    pub(crate) fn glyph_or_empty(&self, g: GlyphId) -> Option<Bytes<'a>> {
        let (at, len) = self.loca.range_or_empty(g)?;
        self.data.slice(at, len)
    }

    /// Emits `g` into `sink`. `Some(Streamed::Drawn)` when the glyph exists
    /// and parsed, even if it drew nothing; `None` when it has no data or is
    /// malformed.
    ///
    /// A composite is walked once first, by the walk [`Glyf::need`] takes,
    /// which refuses one past [`MAX_EDGES`] before anything is drawn: the
    /// draw streams into `sink`, and a glyph refused half-way would leave
    /// half of it there, where HarfBuzz draws none of it. The same walk
    /// finds a component placed by points anywhere in the tree, and such a
    /// glyph is answered `Streamed::NeedsScratch` with nothing drawn: its
    /// alignment reads points a stream has already given away, and
    /// [`Glyf::outline_varied`] draws it.
    pub fn outline<S: Sink>(&self, g: GlyphId, sink: &mut S) -> Option<Streamed> {
        let data = self.glyph(g)?;
        let contours: i16 = data.read(Offset::ZERO)?;
        if contours < 0 {
            let r = self.reach(g, Count::ZERO, &mut Edges(0))?;
            if r.matched == Matched::Yes {
                return Some(Streamed::NeedsScratch(r.kept.plus(r.stack)?));
            }
        }
        self.outline_into(data, Transform::IDENTITY, 0, sink)?;
        Some(Streamed::Drawn)
    }

    /// [`Glyf::need`] for a glyph whose tree places a component by points,
    /// and so is drawn through scratch even at the default instance; `None`
    /// for any other glyph, or one the walk refuses.
    pub fn matched_need(&self, g: GlyphId) -> Option<Count> {
        let r = self.reach(g, Count::ZERO, &mut Edges(0))?;
        match r.matched {
            Matched::Yes => r.kept.plus(r.stack),
            Matched::No => None,
        }
    }

    /// How many [`VarPoint`]s the varied outline of `g` needs, or `None`
    /// when the glyph is malformed, nests past 32 or visits more than
    /// 2,049 glyphs. Walked the way [`Glyf::outline_varied`] walks, so the
    /// count is exact rather than a bound.
    pub fn need(&self, g: GlyphId) -> Option<Count> {
        let mut edges = Edges(0);
        let r = self.reach(g, Count::ZERO, &mut edges)?;
        r.kept.plus(r.stack)
    }

    fn reach(&self, g: GlyphId, depth: Count, edges: &mut Edges) -> Option<Reach> {
        edges.visit()?;
        if depth.value() >= u32::from(MAX_DEPTH) {
            return None;
        }
        let Some(data) = self.glyph(g) else {
            return Some(Reach {
                kept: Count::of(PHANTOMS),
                stack: Count::ZERO,
                matched: Matched::No,
            });
        };
        let contours: i16 = data.read(Offset::ZERO)?;
        let body = data.tail(Offset::of(10u32))?;
        if contours > 0 {
            let n = Simple::parse(body, Count::of(u16::try_from(contours).ok()?))?
                .points
                .plus(Count::of(PHANTOMS))?;
            return Some(Reach {
                kept: n,
                stack: Count::ZERO,
                matched: Matched::No,
            });
        }
        if contours == 0 {
            return Some(Reach {
                kept: Count::of(PHANTOMS),
                stack: Count::ZERO,
                matched: Matched::No,
            });
        }
        let mut comps = Comps {
            body,
            at: Some(Offset::ZERO),
        };
        let (mut k, mut kept, mut deepest) = (Count::ZERO, Count::ZERO, Count::ZERO);
        let mut matched = Matched::No;
        while let Some(c) = comps.next()? {
            k = k.plus(Count::of(1u32))?;
            let r = self.reach(c.glyph, Count::of(depth.value() + 1), edges)?;
            kept = kept.plus(r.kept.minus(Count::of(PHANTOMS))?)?;
            deepest = deepest.max(r.stack);
            if r.matched == Matched::Yes || c.place != Place::Offset {
                matched = Matched::Yes;
            }
        }
        Some(Reach {
            kept: kept.plus(Count::of(PHANTOMS))?,
            stack: k.plus(Count::of(PHANTOMS))?.plus(deepest)?,
            matched,
        })
    }

    /// The horizontal extent of `g`'s phantom points at `coords` -- right
    /// minus left, in font units, unrounded -- which is its advance when the
    /// face carries no `HVAR`. `hmtx` answers a glyph's default advance and
    /// left side bearing. `None` when the walk is refused: malformed data,
    /// nesting past 32, or one visit past HarfBuzz's bound.
    ///
    /// **HarfBuzz's `phantom_only` walk** (`glyf::get_advance_with_var_unscaled`
    /// and `Glyph::get_points` in `[[source: harfbuzz-12-3-0-glyph]]`,
    /// `[[source: harfbuzz-12-3-0-glyf-glyf]]`): a glyph's phantoms start at
    /// `xMin - lsb` and `advance + xMin - lsb`, move by its own explicit
    /// deltas (a phantom is never inferred), and a composite then takes the
    /// phantoms of each USE_MY_METRICS component -- the last one wins --
    /// walking no other component. A simple glyph's points are counted from
    /// `endPtsOfContours` and not decoded. No scratch: four points a level,
    /// on the stack.
    pub fn phantom_extent(
        &self,
        g: GlyphId,
        gvar: Gvar<'a>,
        coords: &VarCoords,
        hmtx: &dyn Fn(GlyphId) -> Option<(i32, i32)>,
    ) -> Option<f32> {
        let mut edges = Edges(0);
        let p = self.phantoms(g, Count::ZERO, &mut edges, gvar, coords, hmtx)?;
        Some(p[1].x - p[0].x)
    }

    fn phantoms(
        &self,
        g: GlyphId,
        depth: Count,
        edges: &mut Edges,
        gvar: Gvar<'a>,
        coords: &VarCoords,
        hmtx: &dyn Fn(GlyphId) -> Option<(i32, i32)>,
    ) -> Option<[VarPoint; 4]> {
        edges.visit()?;
        if depth.value() >= u32::from(MAX_DEPTH) {
            return None;
        }
        // A glyph with no data, or too short for its header, is HarfBuzz's
        // EMPTY: an xMin of 0 and no points of its own.
        let (x_min, kind) = match self.glyph(g).filter(|d| d.len() >= Len::of(10)) {
            None => (0i32, Phantom::Empty),
            Some(data) => {
                let contours: i16 = data.read(Offset::ZERO)?;
                let x_min: i16 = data.read(Offset::of(2u32))?;
                let body = data.tail(Offset::of(10u32))?;
                let kind = match contours {
                    0 => Phantom::Empty,
                    c if c > 0 => {
                        Phantom::Points(Self::counted(body, Count::of(u16::try_from(c).ok()?))?)
                    }
                    _ => Phantom::Composite(body),
                };
                (i32::from(x_min), kind)
            }
        };
        let own = match kind {
            Phantom::Empty => Count::ZERO,
            Phantom::Points(n) => n,
            Phantom::Composite(body) => {
                let mut records = Comps {
                    body,
                    at: Some(Offset::ZERO),
                };
                let mut n = Count::ZERO;
                while records.next()?.is_some() {
                    n = n.plus(Count::of(1u32))?;
                }
                n
            }
        };
        let (advance, lsb) = hmtx(g)?;
        let h = x_min.checked_sub(lsb)?;
        let at = |x: i32| VarPoint::at(x as f32, 0.0, OnCurve::Yes, LastPoint::No, Emit::No);
        let mut p = [at(h), at(advance.checked_add(h)?), at(0), at(0)];
        if let Some(store) = gvar.glyph(g) {
            let total = own.plus(Count::of(PHANTOMS))?;
            store.apply(coords, &mut p, Track::Phantoms(total));
        }
        if let Phantom::Composite(body) = kind {
            let mut records = Comps {
                body,
                at: Some(Offset::ZERO),
            };
            while let Some(c) = records.next()? {
                if c.flags.use_my_metrics() {
                    let next = Count::of(depth.value() + 1);
                    p = self.phantoms(c.glyph, next, edges, gvar, coords, hmtx)?;
                }
            }
        }
        Some(p)
    }

    /// A simple glyph's point count, from its last contour end, with the
    /// instruction length after it in range -- HarfBuzz's check before it
    /// counts, and all a phantom walk reads
    /// (`[[source: harfbuzz-12-3-0-simple-glyph]]`).
    fn counted(body: Bytes<'a>, n: Count) -> Option<Count> {
        let ends: Array<'a, u16> = body.array(Offset::ZERO, n)?;
        let last = ends.get(n.last()?)?;
        let _: u16 = body.read(Offset::ZERO.at(n.as_index(), Len::of(2))?)?;
        Some(Count::of(u32::from(last).checked_add(1)?))
    }

    /// The outline of `g` at `coords` into `sink`: its points moved by its
    /// `gvar` tuples when `gvar` is given, its components flattened and
    /// placed as HarfBuzz flattens them (`Glyph::get_points` in
    /// `[[source: harfbuzz-12-3-0-glyph]]`) and as the specification places
    /// them (the module's doc), and the whole shifted left by its left
    /// phantom point's delta. Without `gvar` it is the default instance of
    /// a glyph that matches points, which only this walk can draw. `scratch`
    /// must hold [`Glyf::need`] points; `None` when it does not, or when the
    /// glyph is malformed.
    ///
    /// **The shift is by the delta, not the position.** HarfBuzz shifts by
    /// the left phantom's final x, which is `xMin - lsb` plus its delta --
    /// 0 plus the delta in a font whose `xMin` is its left side bearing, as
    /// every glyph of JetBrains Mono and Fira Code is (`head` bit 1). The
    /// phantoms start where HarfBuzz starts them, so a matched component
    /// can align on them, and the shift is the left one's movement, so a
    /// glyph whose deltas are 0 is drawn where the default path draws it.
    pub fn outline_varied<S: Sink>(
        &self,
        g: GlyphId,
        gvar: Option<Gvar<'a>>,
        coords: &VarCoords,
        hmtx: Hmtx<'_>,
        scratch: &mut [VarPoint],
        sink: &mut S,
    ) -> Option<()> {
        let r = self.reach(g, Count::ZERO, &mut Edges(0))?;
        if scratch.len() < r.kept.plus(r.stack)?.slot() {
            return None;
        }
        let (all, stack) = scratch.split_at_mut(r.kept.slot());
        let mut walk = Walk {
            gvar,
            coords,
            hmtx,
            edges: Edges(0),
        };
        let mut used = Count::ZERO;
        let rest = self.varied(g, Count::ZERO, &mut walk, all, &mut used, stack)?;
        let end = used.slot();
        let pts = all.get_mut(..end)?;
        let (glyph, phantoms) = pts.split_at_mut(end.checked_sub(Count::of(PHANTOMS).slot())?);
        // HarfBuzz's "undocumented rasterizer behavior", at the top level.
        let shift = phantoms.first()?.x - rest as f32;
        let mut contour = Contour {
            sink,
            transform: Transform::IDENTITY,
            first_on: None,
            first_off: None,
            last_off: None,
        };
        for p in glyph.iter() {
            if p.emit == Emit::Yes {
                let x = if shift != 0.0 { p.x - shift } else { p.x };
                contour.push((x, p.y), p.on, p.last);
            }
        }
        Some(())
    }

    /// Where `g`'s left phantom starts, `xMin - lsb`, and its advance, the
    /// right one's distance from it: HarfBuzz's `init_phantom_points`, with
    /// `xMin` 0 for a glyph with no data and a missing `hmtx` entry read
    /// as 0, as HarfBuzz reads one.
    fn phantom_start(
        walk: &Walk<'_, 'a>,
        g: GlyphId,
        data: Option<Bytes<'a>>,
    ) -> Option<(i32, i32)> {
        let x_min: i16 = match data {
            Some(data) => data.read(Offset::of(2u32))?,
            None => 0,
        };
        let (advance, lsb) = (walk.hmtx)(g).unwrap_or((0, 0));
        Some((i32::from(x_min).checked_sub(lsb)?, advance))
    }

    /// Appends `g`'s points and its four phantom points to `all` at `used`,
    /// varied; a composite's own points -- one offset a component, then its
    /// phantoms -- live in `stack` while its components are walked. Answers
    /// where the left phantom started -- the composite's, or the last
    /// USE_MY_METRICS component's, whose phantoms it takes -- so the top
    /// level can shift by the movement alone.
    fn varied(
        &self,
        g: GlyphId,
        depth: Count,
        walk: &mut Walk<'_, 'a>,
        all: &mut [VarPoint],
        used: &mut Count,
        stack: &mut [VarPoint],
    ) -> Option<i32> {
        walk.edges.visit()?;
        if depth.value() >= u32::from(MAX_DEPTH) {
            return None;
        }
        let base = used.slot();
        let data = self.glyph(g);
        let body = match data {
            Some(data) => {
                let contours: i16 = data.read(Offset::ZERO)?;
                let body = data.tail(Offset::of(10u32))?;
                let kind = match contours {
                    c if c > 0 => {
                        Kind::Simple(Simple::parse(body, Count::of(u16::try_from(c).ok()?))?)
                    }
                    0 => Kind::Empty,
                    _ => Kind::Composite(body),
                };
                kind
            }
            None => Kind::Empty,
        };
        let start = Self::phantom_start(walk, g, data)?;
        let gvar = walk.gvar.and_then(|v| v.glyph(g));
        let comps = match body {
            Kind::Simple(simple) => {
                let one = simple.points == Count::of(1u32);
                let mut at = base;
                let mut out = Some(());
                simple.each(|(x, y), on, last| {
                    let emit = if one { Emit::No } else { Emit::Yes };
                    match all.get_mut(at) {
                        Some(p) => *p = VarPoint::at(x, y, on, last, emit),
                        None => out = None,
                    }
                    at += 1;
                });
                out?;
                push_phantoms(all, &mut at, start)?;
                *used = Count::of(u32::try_from(at).ok()?);
                if let Some(store) = gvar {
                    store.apply(walk.coords, all.get_mut(base..at)?, Track::All);
                }
                return Some(start.0);
            }
            Kind::Empty => {
                let mut at = base;
                push_phantoms(all, &mut at, start)?;
                *used = Count::of(u32::try_from(at).ok()?);
                if let Some(store) = gvar {
                    store.apply(walk.coords, all.get_mut(base..at)?, Track::All);
                }
                return Some(start.0);
            }
            Kind::Composite(body) => body,
        };
        // The composite's own points: every record's offset, a missing or
        // matched component's included, so the delta indices line up; each
        // is its own contour end, so nothing is inferred across components.
        let mut records = Comps {
            body: comps,
            at: Some(Offset::ZERO),
        };
        let mut k = 0usize;
        while let Some(c) = records.next()? {
            *stack.get_mut(k)? = VarPoint::at(c.t.e, c.t.f, OnCurve::Yes, LastPoint::Yes, Emit::No);
            k += 1;
        }
        let mut top = k;
        push_phantoms(stack, &mut top, start)?;
        let (mine, rest) = stack.split_at_mut(top);
        if let Some(store) = gvar {
            let &[l0, r0, t0, b0] = mine.get(k..)? else {
                return None;
            };
            store.apply(walk.coords, mine, Track::All);
            // "For AW and LSB points, apply the net X delta adjustments,
            // and ignore Y deltas. For AH and TSB points, apply the net Y
            // delta adjustments, and ignore X deltas"
            // (`[[source: ms-opentype-gvar]]`, "Point numbers and
            // processing for composite glyphs"). A matched component
            // aligns on them once this composite is its parent or child.
            let [l, r, t, b] = mine.get_mut(k..)? else {
                return None;
            };
            (l.y, r.y, t.x, b.x) = (l0.y, r0.y, t0.x, b0.x);
        }
        let mut left = start.0;
        let mut records = Comps {
            body: comps,
            at: Some(Offset::ZERO),
        };
        let mut j = 0usize;
        while let Some(c) = records.next()? {
            let old = used.slot();
            // The parent's own left and right phantoms as this component
            // finds them: an earlier USE_MY_METRICS component's, if one
            // came, and not yet its own, which the gvar text's order sets
            // after the component is placed.
            let own = [*mine.get(k)?, *mine.get(k.checked_add(1)?)?];
            let child_left =
                self.varied(c.glyph, Count::of(depth.value() + 1), walk, all, used, rest)?;
            let (parent, after) = all.split_at_mut(old);
            let child = after.get_mut(..used.slot().checked_sub(old)?)?;
            let n = child.len();
            if c.flags.use_my_metrics() {
                for (m, p) in mine
                    .get_mut(k..)?
                    .iter_mut()
                    .zip(child.get(n.checked_sub(4)?..)?)
                {
                    m.x = p.x;
                    m.y = p.y;
                }
                left = child_left;
            }
            let matrix = Transform {
                e: 0.0,
                f: 0.0,
                ..c.t
            };
            // An offset, moved by its delta, goes through the matrix with
            // the points under SCALED_COMPONENT_OFFSET and is added after it
            // otherwise; matched points are aligned after the matrix, and
            // their slot's delta is not read.
            let (before, after) = match c.place {
                Place::Offset => {
                    let o = mine.get(j)?;
                    if c.flags.scaled_offset() {
                        ((o.x, o.y), (0.0, 0.0))
                    } else {
                        ((0.0, 0.0), (o.x, o.y))
                    }
                }
                Place::Points { .. } => ((0.0, 0.0), (0.0, 0.0)),
            };
            for p in child.iter_mut() {
                p.x += before.0;
                p.y += before.1;
                if !matrix.is_identity() {
                    (p.x, p.y) = matrix.apply(p.x, p.y);
                }
                p.x += after.0;
                p.y += after.1;
            }
            // The first component "must always" be an offset, and one that
            // is not stays where its matrix put it (note Z, step 7).
            if let (
                Place::Points {
                    parent: on,
                    child: from,
                },
                1..,
            ) = (c.place, j)
            {
                if let Some(d) = Self::alignment(parent.get(base..)?, &own, child, on, from) {
                    for p in child.iter_mut() {
                        p.x += d.0;
                        p.y += d.1;
                    }
                }
            }
            // The component's phantoms go; its points stay.
            *used = Count::of(used.value().checked_sub(PHANTOMS)?);
            j += 1;
        }
        let mut at = used.slot();
        for p in mine.get(k..)? {
            *all.get_mut(at)? = *p;
            at += 1;
        }
        *used = Count::of(u32::try_from(at).ok()?);
        Some(left)
    }

    /// The translation that lays the child's point `from` -- its outline
    /// points numbered from 0, then its left and right phantoms -- on the
    /// parent's point `on`: the points incorporated from the components
    /// before it, numbered from 0, then the parent's own left and right
    /// phantoms, `own` ("Phantom points from the parent or the child may
    /// be referenced"; "Phantom point numbers follow after the last point
    /// number of the last contour", `[[source: ms-opentype-glyf]]`).
    /// `None` where the text leaves the alignment open and the component
    /// stays where its matrix put it: a parent point past the parent's
    /// horizontal phantoms, and either glyph's vertical phantoms, which
    /// need `vmtx` (note Z, step 7).
    fn alignment(
        parent: &[VarPoint],
        own: &[VarPoint; 2],
        child: &[VarPoint],
        on: Index,
        from: Index,
    ) -> Option<(f32, f32)> {
        let to = match on.slot().checked_sub(parent.len()) {
            None => parent.get(on.slot())?,
            Some(phantom) => own.get(phantom)?,
        };
        let horizontal = child.len().checked_sub(2)?;
        let from = child.get(..horizontal)?.get(from.slot())?;
        Some((to.x - from.x, to.y - from.y))
    }

    fn outline_into<S: Sink>(
        &self,
        data: Bytes<'a>,
        transform: Transform,
        depth: u8,
        sink: &mut S,
    ) -> Option<()> {
        if depth >= MAX_DEPTH {
            return None;
        }
        let contours: i16 = data.read(Offset::ZERO)?;
        // numberOfContours, then the bounding box, which is not trusted.
        let body = data.tail(Offset::of(10u32))?;
        if contours > 0 {
            self.simple(body, u16::try_from(contours).ok()?, transform, sink)
        } else if contours < 0 {
            self.composite(body, transform, depth, sink)
        } else {
            Some(())
        }
    }

    fn simple<S: Sink>(
        &self,
        body: Bytes<'a>,
        contours: u16,
        transform: Transform,
        sink: &mut S,
    ) -> Option<()> {
        let glyph = Simple::parse(body, Count::of(contours))?;
        if glyph.points == Count::of(1u32) {
            // A contour of one point draws nothing and is not an error.
            return Some(());
        }
        let mut contour = Contour {
            sink,
            transform,
            first_on: None,
            first_off: None,
            last_off: None,
        };
        glyph.each(|p, on, last| contour.push(p, on, last));
        Some(())
    }

    fn composite<S: Sink>(
        &self,
        body: Bytes<'a>,
        transform: Transform,
        depth: u8,
        sink: &mut S,
    ) -> Option<()> {
        let mut comps = Comps {
            body,
            at: Some(Offset::ZERO),
        };
        while let Some(c) = comps.next()? {
            if let Some(data) = self.glyph(c.glyph) {
                // [`Glyf::outline`] streams no tree that matches points, so
                // every record here is placed by its offset.
                let combined = Transform::combine(transform, c.offset_transform());
                self.outline_into(data, combined, depth + 1, sink)?;
            }
        }
        Some(())
    }
}
