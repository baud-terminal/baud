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
//! **What is deliberately not done.** Point-matching composites (arguments
//! that name points rather than offsets) are read and placed at zero offset
//! rather than matched -- FreeType matches them, the oracle does not read them
//! at all, and no face on this Mac uses them; `SCALED_COMPONENT_OFFSET` and
//! `ROUND_XY_TO_GRID` are ignored as every reference reader ignores them.

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
}

/// The walk's shared state: the gvar table, the coordinates, and how many
/// glyphs it has visited.
struct Walk<'v, 'a> {
    gvar: Gvar<'a>,
    coords: &'v VarCoords,
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
struct Flags(u8);

impl Flags {
    fn on_curve(self) -> OnCurve {
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
struct Component(u16);

impl Component {
    fn words(self) -> bool {
        self.0 & 0x0001 != 0
    }
    fn xy_values(self) -> bool {
        self.0 & 0x0002 != 0
    }
    fn scale(self) -> bool {
        self.0 & 0x0008 != 0
    }
    fn more(self) -> bool {
        self.0 & 0x0020 != 0
    }
    fn xy_scale(self) -> bool {
        self.0 & 0x0040 != 0
    }
    fn two_by_two(self) -> bool {
        self.0 & 0x0080 != 0
    }
    fn use_my_metrics(self) -> bool {
        self.0 & 0x0200 != 0
    }
}

/// One composite record, decoded: its flags, its glyph, and its transform
/// with the x/y offset in `e` and `f` (zero for an anchored component, whose
/// arguments name points rather than an offset).
#[derive(Clone, Copy)]
struct Comp {
    flags: Component,
    glyph: GlyphId,
    t: Transform,
}

/// The records of a composite glyph, in order. The one decoder of them: the
/// default path composes each record's transform into its parent's, the
/// varied path takes the offset apart so a delta can move it.
struct Comps<'a> {
    body: Bytes<'a>,
    at: Option<Offset>,
}

impl Comps<'_> {
    /// The next record; `Some(None)` when the list is done, `None` when it
    /// is malformed.
    fn next(&mut self) -> Option<Option<Comp>> {
        let Some(mut at) = self.at else {
            return Some(None);
        };
        let body = self.body;
        let flags = Component(body.read(at)?);
        let glyph: u16 = body.read(at.plus(Len::of(2))?)?;
        at = at.plus(Len::of(4))?;
        let mut t = Transform::IDENTITY;
        if flags.words() {
            if flags.xy_values() {
                let e: i16 = body.read(at)?;
                let f: i16 = body.read(at.plus(Len::of(2))?)?;
                t.e = f32::from(e);
                t.f = f32::from(f);
            }
            at = at.plus(Len::of(4))?;
        } else {
            if flags.xy_values() {
                let e: u8 = body.read(at)?;
                let f: u8 = body.read(at.plus(Len::of(1))?)?;
                t.e = f32::from(i8::from_ne_bytes([e]));
                t.f = f32::from(i8::from_ne_bytes([f]));
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
        }))
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
struct Simple<'a> {
    ends: Array<'a, u16>,
    points: Count,
    flags: Bytes<'a>,
    x_coords: Bytes<'a>,
    y_coords: Bytes<'a>,
}

impl<'a> Simple<'a> {
    fn parse(body: Bytes<'a>, contours: Count) -> Option<Simple<'a>> {
        let ends: Array<'a, u16> = body.array(Offset::ZERO, contours)?;
        let last_end = ends.get(contours.last()?)?;
        let points = Count::of(u32::from(last_end).checked_add(1)?);
        let after_ends = Offset::ZERO.at(contours.as_index(), Len::of(2))?;
        let instructions: u16 = body.read(after_ends)?;
        let flags_at = after_ends
            .plus(Len::of(2))?
            .plus(Len::of(u32::from(instructions)))?;
        let flags = body.tail(flags_at)?;
        let (xs, ys, flag_bytes) = coord_lengths(flags, points.value())?;
        let flags = flags.slice(Offset::ZERO, flag_bytes)?;
        let x_at = flags_at.plus(flag_bytes)?;
        let y_at = x_at.plus(xs)?;
        Some(Simple {
            ends,
            points,
            flags,
            x_coords: body.slice(x_at, xs)?,
            y_coords: body.slice(y_at, ys)?,
        })
    }

    /// Every point, in order, with whether it is on the curve and whether it
    /// ends its contour.
    fn each(&self, mut f: impl FnMut((f32, f32), OnCurve, LastPoint)) {
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
            f((f32::from(x), f32::from(y)), fl.on_curve(), last);
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

/// Four phantom points at 0, appended at `at`: none is drawn, none is a
/// contour end, and only the left one's delta reaches the outline.
fn push_phantoms(pts: &mut [VarPoint], at: &mut usize) -> Option<()> {
    for _ in 0..PHANTOMS {
        *pts.get_mut(*at)? = VarPoint::at(0.0, 0.0, OnCurve::Yes, LastPoint::No, Emit::No);
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

    fn glyph(&self, g: GlyphId) -> Option<Bytes<'a>> {
        let (at, len) = self.loca.range(g)?;
        self.data.slice(at, len)
    }

    /// Emits `g` into `sink`. `Some(())` when the glyph exists and parsed,
    /// even if it drew nothing; `None` when it has no data or is malformed.
    ///
    /// A composite is walked once first, by [`Glyf::need`], which refuses
    /// one past [`MAX_EDGES`] before anything is drawn: the draw streams
    /// into `sink`, and a glyph refused half-way would leave half of it
    /// there, where HarfBuzz draws none of it.
    pub fn outline<S: Sink>(&self, g: GlyphId, sink: &mut S) -> Option<()> {
        let data = self.glyph(g)?;
        let contours: i16 = data.read(Offset::ZERO)?;
        if contours < 0 {
            self.need(g)?;
        }
        self.outline_into(data, Transform::IDENTITY, 0, sink)
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
            });
        }
        if contours == 0 {
            return Some(Reach {
                kept: Count::of(PHANTOMS),
                stack: Count::ZERO,
            });
        }
        let mut comps = Comps {
            body,
            at: Some(Offset::ZERO),
        };
        let (mut k, mut kept, mut deepest) = (Count::ZERO, Count::ZERO, Count::ZERO);
        while let Some(c) = comps.next()? {
            k = k.plus(Count::of(1u32))?;
            let r = self.reach(c.glyph, Count::of(depth.value() + 1), edges)?;
            kept = kept.plus(r.kept.minus(Count::of(PHANTOMS))?)?;
            deepest = deepest.max(r.stack);
        }
        Some(Reach {
            kept: kept.plus(Count::of(PHANTOMS))?,
            stack: k.plus(Count::of(PHANTOMS))?.plus(deepest)?,
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
    /// `gvar` tuples, its components flattened as HarfBuzz flattens them
    /// (`Glyph::get_points` in `[[source: harfbuzz-12-3-0-glyph]]`), and the
    /// whole shifted left by its left phantom point's delta. `scratch` must
    /// hold [`Glyf::need`] points; `None` when it does not, or when the
    /// glyph is malformed.
    ///
    /// **The shift is by the delta, not the position.** HarfBuzz shifts by
    /// the left phantom's final x, which is `xMin - lsb` plus its delta --
    /// 0 plus the delta in a font whose `xMin` is its left side bearing, as
    /// every glyph of JetBrains Mono and Fira Code is (`head` bit 1). Phantom
    /// points start at 0 here, so the shift is the delta alone and a glyph
    /// whose deltas are 0 is drawn where the default path draws it.
    ///
    /// **Two component rules follow the default path, not HarfBuzz**:
    /// an anchored component sits at offset 0 without point matching, its
    /// offset's delta read and not applied; `SCALED_COMPONENT_OFFSET` is
    /// ignored. None of the fixture fonts' 3,599 components uses either.
    pub fn outline_varied<S: Sink>(
        &self,
        g: GlyphId,
        gvar: Gvar<'a>,
        coords: &VarCoords,
        scratch: &mut [VarPoint],
        sink: &mut S,
    ) -> Option<()> {
        let need = self.need(g)?;
        let mut edges = Edges(0);
        let r = self.reach(g, Count::ZERO, &mut edges)?;
        if scratch.len() < need.slot() {
            return None;
        }
        let (all, stack) = scratch.split_at_mut(r.kept.slot());
        let mut walk = Walk {
            gvar,
            coords,
            edges: Edges(0),
        };
        let mut used = Count::ZERO;
        self.varied(g, Count::ZERO, &mut walk, all, &mut used, stack)?;
        let end = used.slot();
        let pts = all.get_mut(..end)?;
        let (glyph, phantoms) = pts.split_at_mut(end.checked_sub(Count::of(PHANTOMS).slot())?);
        // HarfBuzz's "undocumented rasterizer behavior", at the top level.
        let shift = phantoms.first()?.x;
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

    /// Appends `g`'s points and its four phantom points to `all` at `used`,
    /// varied; a composite's own points -- one offset a component, then its
    /// phantoms -- live in `stack` while its components are walked.
    fn varied(
        &self,
        g: GlyphId,
        depth: Count,
        walk: &mut Walk<'_, 'a>,
        all: &mut [VarPoint],
        used: &mut Count,
        stack: &mut [VarPoint],
    ) -> Option<()> {
        walk.edges.visit()?;
        if depth.value() >= u32::from(MAX_DEPTH) {
            return None;
        }
        let base = used.slot();
        let body = match self.glyph(g) {
            Some(data) => {
                let contours: i16 = data.read(Offset::ZERO)?;
                let body = data.tail(Offset::of(10u32))?;
                match contours {
                    c if c > 0 => {
                        Kind::Simple(Simple::parse(body, Count::of(u16::try_from(c).ok()?))?)
                    }
                    0 => Kind::Empty,
                    _ => Kind::Composite(body),
                }
            }
            None => Kind::Empty,
        };
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
                push_phantoms(all, &mut at)?;
                *used = Count::of(u32::try_from(at).ok()?);
                if let Some(store) = walk.gvar.glyph(g) {
                    store.apply(walk.coords, all.get_mut(base..at)?, Track::All);
                }
                return Some(());
            }
            Kind::Empty => {
                let mut at = base;
                push_phantoms(all, &mut at)?;
                *used = Count::of(u32::try_from(at).ok()?);
                if let Some(store) = walk.gvar.glyph(g) {
                    store.apply(walk.coords, all.get_mut(base..at)?, Track::All);
                }
                return Some(());
            }
            Kind::Composite(body) => body,
        };
        // The composite's own points: every record's offset, a missing or
        // anchored component's included, so the delta indices line up; each
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
        push_phantoms(stack, &mut top)?;
        let (mine, rest) = stack.split_at_mut(top);
        if let Some(store) = walk.gvar.glyph(g) {
            store.apply(walk.coords, mine, Track::All);
        }
        let mut records = Comps {
            body: comps,
            at: Some(Offset::ZERO),
        };
        let mut j = 0usize;
        while let Some(c) = records.next()? {
            let old = used.slot();
            self.varied(c.glyph, Count::of(depth.value() + 1), walk, all, used, rest)?;
            let child = all.get_mut(old..used.slot())?;
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
            }
            let matrix = Transform {
                e: 0.0,
                f: 0.0,
                ..c.t
            };
            let offset = if c.flags.xy_values() {
                let o = mine.get(j)?;
                (o.x, o.y)
            } else {
                (0.0, 0.0)
            };
            for p in child.iter_mut() {
                if !matrix.is_identity() {
                    (p.x, p.y) = matrix.apply(p.x, p.y);
                }
                p.x += offset.0;
                p.y += offset.1;
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
        Some(())
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
                let combined = Transform::combine(transform, c.t);
                self.outline_into(data, combined, depth + 1, sink)?;
            }
        }
        Some(())
    }
}
