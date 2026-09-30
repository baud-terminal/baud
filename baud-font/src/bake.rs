//! The build-time bake: one static TrueType instance of a variable font,
//! written as fontTools' instancer writes it. Note Z, step 5.
//!
//! **What it is for.** baud embeds JetBrains Mono upright and italic at
//! `wght` 400 and 700. The shipped files are the two variable ones; the four
//! static instances are baked from them when baud is built, so the runtime
//! reads four ordinary fonts and pays nothing for the variation.
//!
//! **The oracle is fontTools 4.63.0's `instantiateVariableFont`**, with its
//! defaults, and `tests/bake.rs` holds every glyph, metric and anchor to
//! its output. The rules are its, read at source:
//!
//! * a tuple's scalar is `supportScalar` of its tent at the location, in
//!   `f64`, and an ill-formed tent is dropped (`changeTupleVariationAxisLimit`
//!   -- refused here);
//! * the explicit deltas are scaled, **then** inferred (`iup_delta`, its
//!   `iup_segment` formula), then summed over the tuples in order;
//! * a simple glyph's points are the default plus the sum, rounded
//!   `floor(x + 0.5)` (`otRound`) when written;
//! * a composite keeps its components, each offset moved and rounded;
//! * a glyph's bounds are `recalcBounds`: a composite whose offsets are all
//!   whole before rounding takes the union of its components' bounds, else
//!   it is flattened -- leaf points rounded, float offsets added -- and the
//!   extremes rounded;
//! * the advance is the phantom points' extent rounded and floored at 0,
//!   the side bearing the new `xMin` less the left phantom, rounded -- the
//!   points are not shifted, so the difference lands in `hmtx`;
//! * a GPOS anchor's device is a variation index: its delta is rounded and
//!   added, and the anchor becomes format 1;
//! * `head`'s bounds, `hhea`'s extents, `OS/2`'s weight class and average
//!   advance are recomputed, the overlap flags set.
//!
//! **Where it writes something else, it says so and the tests say so**:
//! the coordinates are packed by this encoder rather than fontTools', `STAT`
//! is dropped where fontTools instances it, and `name` is copied whole where
//! fontTools prunes the names only the dropped tables used -- nothing in
//! baud reads either. None of these changes a glyph, a metric or an anchor.
//!
//! **What it refuses** rather than guess: CFF outlines, an axis other than
//! `wght`, `cvar`, `MVAR`, `VVAR`, `vmtx`, `VARC`, a second `avar` version,
//! feature variations, a component anchored by point numbers or carrying a
//! scale, a device on a value record or a caret, and malformed data
//! anywhere. JetBrains Mono uses none of them.
//!
//! `no_std` and allocation-free, like the rest of the crate: the caller
//! lends the point scratch and the output buffer, and a buffer too small is
//! answered with the size it needs.

use crate::Tag;
use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::face::Face;
use crate::glyf::{Comps, Glyf, LastPoint, LocaFormat, Simple};
use crate::gvar::{Points, Store as Tuples, TupleData};
use crate::ivs::{Store as ItemStore, VarIndex};
use crate::var::{Avar, Axis, F2Dot14, Fixed};
use baud_domain::GlyphId;

/// Why a font was not baked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Not a font `Face::parse` reads.
    NotAFont,
    /// Not a single TrueType-outline face.
    NotTrueType,
    /// No `fvar`, or an axis other than `wght`.
    Axes,
    /// A table this bake does not instance: `cvar`, `MVAR`, `VVAR`, `vmtx`,
    /// `VARC`, CFF, a second `avar` version, feature variations.
    Table(Tag),
    /// A table the bake reads or rewrites is malformed.
    Malformed(Tag),
    /// Glyph data that is malformed, nests too deep, or would not fit its
    /// fields once varied.
    Glyph(GlyphId),
    /// A component anchored by point numbers, or carrying a scale.
    Component(GlyphId),
    /// A metric past its field.
    Metrics,
    /// A device table that is not a variation index: on a value record, a
    /// caret, or an anchor.
    Device,
    /// The scratch holds fewer points than a glyph's walk needs.
    Scratch,
    /// The output buffer needs this many bytes: the font, and the room its
    /// `hmtx` and `loca` take at full width while the glyphs are written
    /// and before they are compacted. The font answered is no longer.
    Out(Len),
}

/// One point of the bake's scratch: its default position, this tuple's
/// delta and the running sum, in `f64` as fontTools keeps them.
#[derive(Clone, Copy, Debug)]
pub struct BakePoint {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    sx: f64,
    sy: f64,
    given: Given,
    last: LastPoint,
    flag: PointFlag,
}

/// A simple glyph's stored flag byte for the point, which the writer
/// carries the on-curve and overlap bits of.
#[derive(Clone, Copy, Debug)]
struct PointFlag(u8);

impl BakePoint {
    pub const ZERO: BakePoint = BakePoint {
        x: 0.0,
        y: 0.0,
        dx: 0.0,
        dy: 0.0,
        sx: 0.0,
        sy: 0.0,
        given: Given::No,
        last: LastPoint::No,
        flag: PointFlag(0),
    };

    fn at(x: f64, y: f64, last: LastPoint) -> BakePoint {
        BakePoint {
            x,
            y,
            last,
            ..BakePoint::ZERO
        }
    }
}

/// Whether the tuple being read named the point, which decides whether it
/// is inferred.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Given {
    No,
    Yes,
}

const WGHT: Tag = Tag::from_bytes(*b"wght");
const STAT: Tag = Tag::from_bytes(*b"STAT");
const DSIG: Tag = Tag::from_bytes(*b"DSIG");
const REFUSED: [&[u8; 4]; 8] = [
    b"cvar", b"MVAR", b"VVAR", b"vmtx", b"VARC", b"CFF ", b"CFF2", b"vhea",
];

/// fontTools' `otRound`.
fn ot_round(x: f64) -> f64 {
    core::f64::math::floor(x + 0.5)
}

fn whole(x: f64) -> bool {
    x == core::f64::math::floor(x)
}

/// A whole value as a 16-bit field, or `None` past it.
fn short(x: f64) -> Option<i16> {
    if (-32768.0..=32767.0).contains(&x) {
        Some(x as i16)
    } else {
        None
    }
}

/// `supportScalar` of one axis's tent at `v`, the tent in 2.14 units:
/// `None` for an ill-formed one, which fontTools drops.
fn tent(lower: F2Dot14, peak: F2Dot14, upper: F2Dot14, v: F2Dot14) -> Option<f64> {
    let (l, p, u, v) = (
        i32::from(lower.0),
        i32::from(peak.0),
        i32::from(upper.0),
        i32::from(v.0),
    );
    if p == 0 {
        return Some(1.0);
    }
    if !(l <= p && p <= u) || (l < 0 && u > 0) {
        return None;
    }
    if v == p {
        return Some(1.0);
    }
    if v <= l || v >= u {
        return Some(0.0);
    }
    // Differences of 2.14 values are exact in `f64`, so the ratio of the
    // integers is the ratio fontTools divides.
    Some(if v < p {
        f64::from(v - l) / f64::from(p - l)
    } else {
        f64::from(v - u) / f64::from(p - u)
    })
}

/// fontTools' `normalize`: the user value clamped to the axis and mapped
/// to -1..1 (`normalizeValue`), bent through `avar`'s map
/// (`piecewiseLinearMap`, over the 2.14 values as floats, the map a
/// dictionary so a repeated key keeps its last value), then quantized to
/// 2.14 (`floatToFixedToFloat`). **Not HarfBuzz's rule**, which the runtime
/// follows: HarfBuzz divides in 16.16 integers, and the two land a 2.14
/// unit apart where the bent value falls near a half -- the case font's
/// wght 700 does. `None` for an axis whose default is outside its range.
fn normalize(axis: Axis, map: Option<(Bytes<'_>, Count)>, user: Fixed) -> Option<F2Dot14> {
    let f = |x: Fixed| f64::from(x.0) / 65536.0;
    let (lo, def, hi) = (f(axis.min), f(axis.default), f(axis.max));
    if !(lo <= def && def <= hi) {
        return None;
    }
    let v = f(user).min(hi).max(lo);
    let n = if v == def || lo == hi {
        0.0
    } else if (v < def && lo != def) || (v > def && hi == def) {
        (v - def) / (def - lo)
    } else {
        (v - def) / (hi - def)
    };
    let n = match map {
        Some((records, count)) if !count.is_zero() => piecewise(n, records, count)?,
        _ => n,
    };
    let q = ot_round(n * 16384.0);
    short(q).map(F2Dot14)
}

/// `piecewiseLinearMap(v, mapping)` over an `AxisValueMap` list.
fn piecewise(v: f64, records: Bytes<'_>, count: Count) -> Option<f64> {
    let (mut exact, mut min, mut max, mut below, mut above) = (
        None,
        None::<(f64, f64)>,
        None::<(f64, f64)>,
        None::<(f64, f64)>,
        None::<(f64, f64)>,
    );
    let mut i = Index::ZERO;
    while count.holds(i) {
        let at = Offset::ZERO.at(i, Len::of(4))?;
        let from: F2Dot14 = records.read(at)?;
        let to: F2Dot14 = records.read(at.plus(Len::of(2))?)?;
        let (k, t) = (f64::from(from.0) / 16384.0, f64::from(to.0) / 16384.0);
        if k == v {
            exact = Some(t);
        }
        if min.is_none_or(|(m, _)| k <= m) {
            min = Some((k, t));
        }
        if max.is_none_or(|(m, _)| k >= m) {
            max = Some((k, t));
        }
        if k < v && below.is_none_or(|(m, _)| k >= m) {
            below = Some((k, t));
        }
        if k > v && above.is_none_or(|(m, _)| k <= m) {
            above = Some((k, t));
        }
        i = i.next();
    }
    if let Some(t) = exact {
        return Some(t);
    }
    let ((kmin, tmin), (kmax, tmax)) = (min?, max?);
    if v < kmin {
        return Some(v + tmin - kmin);
    }
    if v > kmax {
        return Some(v + tmax - kmax);
    }
    let ((a, va), (b, vb)) = (below?, above?);
    Some(va + (vb - va) * (v - a) / (b - a))
}

/// The scalar of a gvar tuple on the one axis.
fn tuple_scalar(t: &TupleData<'_>, v: F2Dot14) -> Option<f64> {
    let peak = t.peak.at(Index::ZERO);
    let (lower, upper) = match t.region {
        Some((a, b)) => (a.at(Index::ZERO), b.at(Index::ZERO)),
        None => (F2Dot14(peak.0.min(0)), F2Dot14(peak.0.max(0))),
    };
    tent(lower, peak, upper, v)
}

/// fontTools' `iup_segment` on one axis: `x` between the references `a`
/// and `b`, each a position and a delta.
fn iup(x: f64, a: (f64, f64), b: (f64, f64)) -> f64 {
    let ((mut x1, mut d1), (mut x2, mut d2)) = (a, b);
    if x1 == x2 {
        return if d1 == d2 { d1 } else { 0.0 };
    }
    if x1 > x2 {
        (x1, x2) = (x2, x1);
        (d1, d2) = (d2, d1);
    }
    let scale = (d2 - d1) / (x2 - x1);
    if x <= x1 {
        d1
    } else if x >= x2 {
        d2
    } else {
        d1 + (x - x1) * scale
    }
}

/// Fills the deltas of the points one tuple left out, contour by contour,
/// from the named points on either side -- fontTools' `iup_contour`, in one
/// walk: from the first named point round to itself, each gap filled from
/// the named points that bound it, every point visited once.
fn infer(pts: &mut [BakePoint]) {
    let mut start = 0usize;
    while start < pts.len() {
        let mut end = start;
        while end + 1 < pts.len() && pts[end].last != LastPoint::Yes {
            end += 1;
        }
        let contour = &mut pts[start..=end];
        let n = contour.len();
        match contour.iter().position(|p| p.given == Given::Yes) {
            None => {
                for p in contour.iter_mut() {
                    p.dx = 0.0;
                    p.dy = 0.0;
                }
            }
            Some(first) => {
                let mut prev = first;
                for k in 1..=n {
                    let j = (first + k) % n;
                    if contour[j].given != Given::Yes {
                        continue;
                    }
                    let (a, b) = (contour[prev], contour[j]);
                    let mut i = (prev + 1) % n;
                    while i != j {
                        let p = contour[i];
                        contour[i].dx = iup(p.x, (a.x, a.dx), (b.x, b.dx));
                        contour[i].dy = iup(p.y, (a.y, a.dy), (b.y, b.dy));
                        i = (i + 1) % n;
                    }
                    prev = j;
                }
            }
        }
        start = end + 1;
    }
}

/// Moves `pts` -- the glyph's own points, then its four phantoms -- by the
/// glyph's tuples at `v`, as fontTools' instancer does. `None` refuses the
/// glyph: a malformed store, an ill-formed tent, a point past the glyph.
fn vary(store: Option<Tuples<'_>>, v: F2Dot14, pts: &mut [BakePoint]) -> Option<()> {
    for p in pts.iter_mut() {
        p.sx = 0.0;
        p.sy = 0.0;
    }
    if let Some(store) = store {
        let total = Count::of(u32::try_from(pts.len()).ok()?);
        store.tuples(total, |mut t| {
            let s = tuple_scalar(&t, v)?;
            if s == 0.0 {
                return Some(());
            }
            for p in pts.iter_mut() {
                p.dx = 0.0;
                p.dy = 0.0;
                p.given = Given::No;
            }
            let scaled = |d: i32| {
                if s == 1.0 {
                    f64::from(d)
                } else {
                    f64::from(d) * s
                }
            };
            let mut list = t.points.map(|p: Points<'_>| p.iter());
            let mut k = Count::ZERO;
            while k != t.n {
                let i = match list.as_mut() {
                    Some(it) => Points::next_from(it),
                    None => k.as_index(),
                };
                k = k.plus(Count::of(1u32))?;
                let (x, y) = (t.xs.next(), t.ys.next());
                // A point named twice keeps its last delta, as fontTools
                // reads the list.
                let p = pts.get_mut(i.slot())?;
                p.dx = scaled(x);
                p.dy = scaled(y);
                p.given = Given::Yes;
            }
            if t.points.is_some() {
                infer(pts);
            }
            for p in pts.iter_mut() {
                p.sx += p.dx;
                p.sy += p.dy;
            }
            Some(())
        })?;
    }
    for p in pts.iter_mut() {
        p.x += p.sx;
        p.y += p.sy;
    }
    Some(())
}

/// A glyph's bounds, as its header stores them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Bbox {
    x_min: i32,
    y_min: i32,
    x_max: i32,
    y_max: i32,
}

impl Bbox {
    const ZERO: Bbox = Bbox {
        x_min: 0,
        y_min: 0,
        x_max: 0,
        y_max: 0,
    };

    fn degenerate(self) -> bool {
        self.x_min == self.x_max && self.y_min == self.y_max
    }

    fn union(self, o: Bbox) -> Bbox {
        Bbox {
            x_min: self.x_min.min(o.x_min),
            y_min: self.y_min.min(o.y_min),
            x_max: self.x_max.max(o.x_max),
            y_max: self.y_max.max(o.y_max),
        }
    }
}

/// A flattened composite's extremes before the final rounding.
#[derive(Clone, Copy)]
struct Span {
    x_min: f64,
    y_min: f64,
    x_max: f64,
    y_max: f64,
}

impl Span {
    fn union(self, o: Span) -> Span {
        Span {
            x_min: self.x_min.min(o.x_min),
            y_min: self.y_min.min(o.y_min),
            x_max: self.x_max.max(o.x_max),
            y_max: self.y_max.max(o.y_max),
        }
    }

    fn moved(self, x: f64, y: f64) -> Span {
        Span {
            x_min: self.x_min + x,
            y_min: self.y_min + y,
            x_max: self.x_max + x,
            y_max: self.y_max + y,
        }
    }
}

fn join<T>(a: Option<T>, b: T, f: impl Fn(T, T) -> T) -> T {
    match a {
        Some(a) => f(a, b),
        None => b,
    }
}

/// What a glyph's data is.
#[derive(Clone, Copy)]
enum Shape<'f> {
    Empty,
    Simple(Simple<'f>),
    Composite(Bytes<'f>),
}

/// A glyph as the bake reads it: its data, its contour count and its
/// stored `xMin`, which its left phantom starts from.
#[derive(Clone, Copy)]
struct Parsed<'f> {
    shape: Shape<'f>,
    contours: i16,
    x_min: i16,
}

/// The most glyphs one top-level walk may visit, HarfBuzz's bound plus
/// one; past it the glyph is refused.
const BUDGET: u32 = 2049;
const MAX_DEPTH: u32 = 32;

/// How deep a walk has nested.
#[derive(Clone, Copy)]
struct Depth(u32);

impl Depth {
    fn next(self) -> Depth {
        Depth(self.0 + 1)
    }
}

/// How many glyphs one top-level walk has visited.
struct Budget(u32);

struct Ctx<'f> {
    face: &'f Face<'f>,
    glyf: Glyf<'f>,
    gvar: Option<crate::gvar::Gvar<'f>>,
    v: F2Dot14,
}

impl<'f> Ctx<'f> {
    fn parse(&self, g: GlyphId) -> Result<Parsed<'f>, Refused> {
        let bad = Refused::Glyph(g);
        let data = self.glyf.glyph_or_empty(g).ok_or(bad)?;
        if data.is_empty() {
            return Ok(Parsed {
                shape: Shape::Empty,
                contours: 0,
                x_min: 0,
            });
        }
        let contours: i16 = data.read(Offset::ZERO).ok_or(bad)?;
        let x_min: i16 = data.read(Offset::of(2u32)).ok_or(bad)?;
        let body = data.tail(Offset::of(10u32)).ok_or(bad)?;
        let shape = match contours {
            0 => Shape::Empty,
            c if c > 0 => {
                let n = Count::of(u16::try_from(c).map_err(|_| bad)?);
                let s = Simple::parse(body, n).ok_or(bad)?;
                // fontTools asserts the ends ascend; a list that does not is
                // a glyph it would not read.
                let mut prev: Option<u16> = None;
                let mut i = Index::ZERO;
                while n.holds(i) {
                    let e = s.ends.get(i).ok_or(bad)?;
                    if prev.is_some_and(|p| e <= p) {
                        return Err(bad);
                    }
                    prev = Some(e);
                    i = i.next();
                }
                Shape::Simple(s)
            }
            _ => Shape::Composite(body),
        };
        Ok(Parsed {
            shape,
            contours,
            x_min,
        })
    }

    /// The glyph's own points -- a simple glyph's outline or a composite's
    /// component offsets -- then its four phantoms, varied, in `pts`.
    /// Answers how many.
    fn points(&self, g: GlyphId, p: Parsed<'f>, pts: &mut [BakePoint]) -> Result<usize, Refused> {
        let bad = Refused::Glyph(g);
        let mut n = 0usize;
        let mut put = |pt: BakePoint, pts: &mut [BakePoint]| -> Result<(), Refused> {
            *pts.get_mut(n).ok_or(Refused::Scratch)? = pt;
            n += 1;
            Ok(())
        };
        match p.shape {
            Shape::Empty => {}
            Shape::Simple(s) => {
                let mut out = Ok(());
                s.each_flag(|(x, y), fl, last| {
                    if out.is_ok() && fl.0 & CUBIC != 0 {
                        out = Err(bad);
                    }
                    if out.is_ok() {
                        let mut q = BakePoint::at(f64::from(x), f64::from(y), last);
                        q.flag = PointFlag(fl.0);
                        out = put(q, pts);
                    }
                });
                out?;
            }
            Shape::Composite(body) => {
                let mut records = Comps {
                    body,
                    at: Some(Offset::ZERO),
                };
                while let Some(c) = records.next().ok_or(bad)? {
                    if !c.flags.xy_values() {
                        return Err(Refused::Component(g));
                    }
                    if c.flags.scale() || c.flags.xy_scale() || c.flags.two_by_two() {
                        return Err(Refused::Component(g));
                    }
                    put(
                        BakePoint::at(f64::from(c.t.e), f64::from(c.t.f), LastPoint::Yes),
                        pts,
                    )?;
                }
            }
        }
        let advance = self
            .face
            .default_advance(g)
            .ok_or(Refused::Malformed(Tag::HMTX))?;
        let lsb = self
            .face
            .side_bearing(g)
            .ok_or(Refused::Malformed(Tag::HMTX))?;
        let left = f64::from(p.x_min) - f64::from(lsb.value());
        let right = left + f64::from(advance.value());
        for x in [left, right, 0.0, 0.0] {
            put(BakePoint::at(x, 0.0, LastPoint::Yes), pts)?;
        }
        let store = self.gvar.and_then(|v| v.glyph(g));
        vary(store, self.v, pts.get_mut(..n).ok_or(Refused::Scratch)?).ok_or(bad)?;
        Ok(n)
    }

    /// `recalcBounds`: the glyph's bounds once varied.
    fn bounds(
        &self,
        g: GlyphId,
        depth: Depth,
        budget: &mut Budget,
        pts: &mut [BakePoint],
    ) -> Result<Bbox, Refused> {
        let p = self.visit(g, depth, budget)?;
        let n = self.points(g, p, pts)?;
        let (own, rest) = pts.split_at_mut(n);
        self.bounds_of(g, p, own, rest, depth, budget)
    }

    fn visit(&self, g: GlyphId, depth: Depth, budget: &mut Budget) -> Result<Parsed<'f>, Refused> {
        budget.0 += 1;
        if budget.0 > BUDGET || depth.0 >= MAX_DEPTH {
            return Err(Refused::Glyph(g));
        }
        self.parse(g)
    }

    /// The bounds of a glyph whose own points `own` are already varied.
    fn bounds_of(
        &self,
        g: GlyphId,
        p: Parsed<'f>,
        own: &[BakePoint],
        rest: &mut [BakePoint],
        depth: Depth,
        budget: &mut Budget,
    ) -> Result<Bbox, Refused> {
        let pts = &own[..own.len() - 4];
        match p.shape {
            Shape::Empty => Ok(Bbox::ZERO),
            Shape::Simple(_) => {
                let mut b: Option<Bbox> = None;
                for q in pts {
                    let (x, y) = (ot_round(q.x) as i32, ot_round(q.y) as i32);
                    let one = Bbox {
                        x_min: x,
                        y_min: y,
                        x_max: x,
                        y_max: y,
                    };
                    b = Some(join(b, one, Bbox::union));
                }
                Ok(b.unwrap_or(Bbox::ZERO))
            }
            Shape::Composite(body) => {
                if pts.iter().all(|q| whole(q.x) && whole(q.y)) {
                    let mut b: Option<Bbox> = None;
                    let mut records = Comps {
                        body,
                        at: Some(Offset::ZERO),
                    };
                    for q in pts {
                        let c = records.next().flatten().ok_or(Refused::Glyph(g))?;
                        let child = self.bounds(c.glyph, depth.next(), budget, rest)?;
                        if child.degenerate() {
                            continue;
                        }
                        let (x, y) = (q.x as i32, q.y as i32);
                        let moved = Bbox {
                            x_min: child.x_min + x,
                            y_min: child.y_min + y,
                            x_max: child.x_max + x,
                            y_max: child.y_max + y,
                        };
                        b = Some(join(b, moved, Bbox::union));
                    }
                    Ok(b.unwrap_or(Bbox::ZERO))
                } else {
                    let s = self.flat_of(g, p, own, rest, depth, budget)?;
                    Ok(match s {
                        None => Bbox::ZERO,
                        Some(s) => Bbox {
                            x_min: ot_round(s.x_min) as i32,
                            y_min: ot_round(s.y_min) as i32,
                            x_max: ot_round(s.x_max) as i32,
                            y_max: ot_round(s.y_max) as i32,
                        },
                    })
                }
            }
        }
    }

    /// `getCoordinates(round=otRound)`'s extremes: every leaf point rounded,
    /// every offset added unrounded; `None` for a glyph with no points.
    fn flat(
        &self,
        g: GlyphId,
        depth: Depth,
        budget: &mut Budget,
        pts: &mut [BakePoint],
    ) -> Result<Option<Span>, Refused> {
        let p = self.visit(g, depth, budget)?;
        let n = self.points(g, p, pts)?;
        let (own, rest) = pts.split_at_mut(n);
        self.flat_of(g, p, own, rest, depth, budget)
    }

    fn flat_of(
        &self,
        g: GlyphId,
        p: Parsed<'f>,
        own: &[BakePoint],
        rest: &mut [BakePoint],
        depth: Depth,
        budget: &mut Budget,
    ) -> Result<Option<Span>, Refused> {
        let pts = &own[..own.len() - 4];
        match p.shape {
            Shape::Empty => Ok(None),
            Shape::Simple(_) => {
                let mut s: Option<Span> = None;
                for q in pts {
                    let (x, y) = (ot_round(q.x), ot_round(q.y));
                    let one = Span {
                        x_min: x,
                        y_min: y,
                        x_max: x,
                        y_max: y,
                    };
                    s = Some(join(s, one, Span::union));
                }
                Ok(s)
            }
            Shape::Composite(body) => {
                let mut s: Option<Span> = None;
                let mut records = Comps {
                    body,
                    at: Some(Offset::ZERO),
                };
                for q in pts {
                    let c = records.next().flatten().ok_or(Refused::Glyph(g))?;
                    if let Some(child) = self.flat(c.glyph, depth.next(), budget, rest)? {
                        s = Some(join(s, child.moved(q.x, q.y), Span::union));
                    }
                }
                Ok(s)
            }
        }
    }
}

const CUBIC: u8 = 0x80;
const OVERLAP_SIMPLE: u8 = 0x40;
const ON_CURVE: u8 = 0x01;
const X_SHORT: u8 = 0x02;
const Y_SHORT: u8 = 0x04;
const REPEAT: u8 = 0x08;
const X_SAME: u8 = 0x10;
const Y_SAME: u8 = 0x20;

const ARG_WORDS: u16 = 0x0001;
const XY_VALUES: u16 = 0x0002;
const MORE: u16 = 0x0020;
const HAVE_INSTRUCTIONS: u16 = 0x0100;
const OVERLAP_COMPOUND: u16 = 0x0400;
/// The flags fontTools carries through: ROUND_XY_TO_GRID, the obsolete
/// NON_OVERLAPPING, USE_MY_METRICS, OVERLAP_COMPOUND and the two
/// component-offset scalings. It recomputes the rest.
const KEPT: u16 = 0x0004 | 0x0010 | 0x0200 | 0x0400 | 0x0800 | 0x1000;

/// A big-endian value a writer puts.
pub(crate) trait ToBe: Copy {
    const LEN: Len;
    fn put(self, out: &mut [u8]);
}

macro_rules! to_be {
    ($t:ty, $n:literal) => {
        impl ToBe for $t {
            const LEN: Len = Len::of($n);
            fn put(self, out: &mut [u8]) {
                out.copy_from_slice(&self.to_be_bytes());
            }
        }
    };
}
to_be!(u8, 1);
to_be!(u16, 2);
to_be!(i16, 2);
to_be!(u32, 4);

/// A 32-bit word of the output: a checksum, or a tag as the directory
/// sorts it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Word(u32);

impl ToBe for Word {
    const LEN: Len = Len::of(4);
    fn put(self, out: &mut [u8]) {
        out.copy_from_slice(&self.0.to_be_bytes());
    }
}

/// `at` and `len` as a slice range, or `None` past `usize`.
fn range(at: Offset, len: Len) -> Option<core::ops::Range<usize>> {
    let s = usize::try_from(at.since_start().value()).ok()?;
    let n = usize::try_from(len.value()).ok()?;
    Some(s..s.checked_add(n)?)
}

/// The output: every write past its end is skipped, so a buffer too small
/// still measures what the font needs.
struct Out<'o> {
    buf: &'o mut [u8],
}

impl Out<'_> {
    fn put<T: ToBe>(&mut self, at: Offset, v: T) {
        if let Some(d) = range(at, T::LEN).and_then(|r| self.buf.get_mut(r)) {
            v.put(d);
        }
    }

    fn copy(&mut self, at: Offset, src: &[u8]) {
        let Ok(len) = len_of(src) else {
            return;
        };
        if let Some(d) = range(at, len).and_then(|r| self.buf.get_mut(r)) {
            d.copy_from_slice(src);
        }
    }

    /// Moves `len` bytes from `from` to `to`, the two allowed to overlap.
    fn shift(&mut self, from: Offset, len: Len, to: Offset) {
        if let (Some(r), Some(t)) = (range(from, len), range(to, Len::of(0))) {
            if r.end <= self.buf.len() && t.start + (r.end - r.start) <= self.buf.len() {
                self.buf.copy_within(r, t.start);
            }
        }
    }

    /// Zeroes the bytes from `from` up to `to`.
    fn zero(&mut self, from: Offset, to: Offset) {
        if let Some(r) = to.since(from).and_then(|l| range(from, l)) {
            if let Some(d) = self.buf.get_mut(r) {
                d.fill(0);
            }
        }
    }

    /// The word at `at`, a byte past the buffer read as 0.
    fn word(&self, at: Offset) -> Word {
        let mut b = [0u8; 4];
        let mut o = at;
        for v in b.iter_mut() {
            *v = range(o, Len::of(1))
                .and_then(|r| self.buf.get(r))
                .and_then(|x| x.first().copied())
                .unwrap_or(0);
            o = o.plus(Len::of(1)).unwrap_or(o);
        }
        Word(u32::from_be_bytes(b))
    }
}

/// A cursor into [`Out`].
struct Cursor<'s, 'o> {
    out: &'s mut Out<'o>,
    at: Offset,
}

impl Cursor<'_, '_> {
    fn put<T: ToBe>(&mut self, v: T) -> Result<(), Refused> {
        self.out.put(self.at, v);
        self.at = self.at.plus(T::LEN).ok_or(Refused::Metrics)?;
        Ok(())
    }

    fn copy(&mut self, src: &[u8]) -> Result<(), Refused> {
        self.out.copy(self.at, src);
        self.at = self.at.plus(len_of(src)?).ok_or(Refused::Metrics)?;
        Ok(())
    }
}

fn len_of(src: &[u8]) -> Result<Len, Refused> {
    u32::try_from(src.len())
        .map(Len::of)
        .map_err(|_| Refused::Metrics)
}

/// One point's move from the one before, as the file stores it.
#[derive(Clone, Copy)]
struct Step {
    flag: PointFlag,
    dx: i16,
    dy: i16,
}

/// A move between two points, in font units.
#[derive(Clone, Copy)]
struct Move {
    dx: i32,
    dy: i32,
}

fn step(Move { dx, dy }: Move) -> Option<Step> {
    let axis = |d: i32, short: u8, same: u8| -> u8 {
        if d == 0 {
            same
        } else if (-255..=255).contains(&d) {
            short | if d > 0 { same } else { 0 }
        } else {
            0
        }
    };
    Some(Step {
        flag: PointFlag(axis(dx, X_SHORT, X_SAME) | axis(dy, Y_SHORT, Y_SAME)),
        dx: i16::try_from(dx).ok()?,
        dy: i16::try_from(dy).ok()?,
    })
}

/// A simple glyph's points once rounded, and the flags carried from the
/// input; the header's bounds are the caller's.
fn write_simple(
    g: GlyphId,
    s: Simple<'_>,
    pts: &[BakePoint],
    w: &mut Cursor<'_, '_>,
) -> Result<(), Refused> {
    let bad = Refused::Glyph(g);
    let mut i = Index::ZERO;
    while s.ends.count().holds(i) {
        w.put(s.ends.get(i).ok_or(bad)?)?;
        i = i.next();
    }
    let program = s.instructions.as_slice();
    w.put(u16::try_from(program.len()).map_err(|_| bad)?)?;
    w.copy(program)?;
    let steps = |k: usize| -> Result<Step, Refused> {
        let q = pts.get(k).ok_or(bad)?;
        let (x, y) = (ot_round(q.x), ot_round(q.y));
        let (px, py) = match k.checked_sub(1).and_then(|j| pts.get(j)) {
            Some(p) => (ot_round(p.x), ot_round(p.y)),
            None => (0.0, 0.0),
        };
        short(x).ok_or(bad)?;
        short(y).ok_or(bad)?;
        step(Move {
            dx: (x - px) as i32,
            dy: (y - py) as i32,
        })
        .ok_or(bad)
    };
    // The flags: the input's on-curve, overlap and cubic bits, the overlap
    // bit set on the first point as fontTools sets it; equal flags in a run
    // written once with a repeat count.
    let mut run: Option<(u8, u32)> = None;
    let flush = |run: (u8, u32), w: &mut Cursor<'_, '_>| -> Result<(), Refused> {
        let (f, mut n) = run;
        while n > 0 {
            let chunk = n.min(256);
            match chunk {
                1 => w.put(f)?,
                2 => {
                    w.put(f)?;
                    w.put(f)?;
                }
                _ => {
                    w.put(f | REPEAT)?;
                    w.put(u8::try_from(chunk - 1).map_err(|_| bad)?)?;
                }
            }
            n -= chunk;
        }
        Ok(())
    };
    for k in 0..pts.len() {
        let kept = pts.get(k).ok_or(bad)?.flag.0 & (ON_CURVE | OVERLAP_SIMPLE);
        let kept = if k == 0 { kept | OVERLAP_SIMPLE } else { kept };
        let f = steps(k)?.flag.0 | kept;
        run = match run {
            Some((r, n)) if r == f => Some((r, n + 1)),
            Some(r) => {
                flush(r, w)?;
                Some((f, 1))
            }
            None => Some((f, 1)),
        };
    }
    if let Some(r) = run {
        flush(r, w)?;
    }
    for k in 0..pts.len() {
        let st = steps(k)?;
        if st.flag.0 & X_SHORT != 0 {
            w.put(st.dx.unsigned_abs().to_be_bytes()[1])?;
        } else if st.flag.0 & X_SAME == 0 {
            w.put(st.dx)?;
        }
    }
    for k in 0..pts.len() {
        let st = steps(k)?;
        if st.flag.0 & Y_SHORT != 0 {
            w.put(st.dy.unsigned_abs().to_be_bytes()[1])?;
        } else if st.flag.0 & Y_SAME == 0 {
            w.put(st.dy)?;
        }
    }
    Ok(())
}

/// A composite's records with their offsets moved and rounded, and its
/// instructions carried through.
fn write_composite(
    g: GlyphId,
    body: Bytes<'_>,
    offsets: &[BakePoint],
    w: &mut Cursor<'_, '_>,
) -> Result<(), Refused> {
    let bad = Refused::Glyph(g);
    let mut records = Comps {
        body,
        at: Some(Offset::ZERO),
    };
    let mut instructions = false;
    let n = offsets.len();
    for (k, q) in offsets.iter().enumerate() {
        let c = records.next().flatten().ok_or(bad)?;
        instructions |= c.flags.0 & HAVE_INSTRUCTIONS != 0;
        let end = c.end;
        let (x, y) = (ot_round(q.x), ot_round(q.y));
        let (x, y) = (short(x).ok_or(bad)?, short(y).ok_or(bad)?);
        let bytes = (-128..=127).contains(&x) && (-128..=127).contains(&y);
        let mut f = (c.flags.0 & KEPT) | XY_VALUES;
        if !bytes {
            f |= ARG_WORDS;
        }
        if k == 0 {
            f |= OVERLAP_COMPOUND;
        }
        if k + 1 < n {
            f |= MORE;
        }
        let program = if k + 1 == n && instructions {
            let len: u16 = body.read(end).ok_or(bad)?;
            let at = end.plus(Len::of(2)).ok_or(bad)?;
            body.slice(at, Len::of(u32::from(len)))
                .ok_or(bad)?
                .as_slice()
        } else {
            &[]
        };
        if !program.is_empty() {
            f |= HAVE_INSTRUCTIONS;
        }
        w.put(f)?;
        w.put(u16::try_from(c.glyph.index()).map_err(|_| bad)?)?;
        if bytes {
            w.put(x.to_be_bytes()[1])?;
            w.put(y.to_be_bytes()[1])?;
        } else {
            w.put(x)?;
            w.put(y)?;
        }
        if !program.is_empty() {
            w.put(u16::try_from(program.len()).map_err(|_| bad)?)?;
            w.copy(program)?;
        }
    }
    Ok(())
}

/// What a glyph left for `head`, `hhea` and `OS/2`.
struct Tally {
    head: Option<Bbox>,
    lsb_is_xmin: LsbIsXmin,
    advance_max: i32,
    lsb_min: Option<i32>,
    rsb_min: Option<i32>,
    extent_max: Option<i32>,
    /// The nonzero advances' sum and count, exact in `f64` far past any
    /// font's.
    advance_sum: f64,
    advances: f64,
}

/// `head` flags bit 1: whether every drawn glyph's `xMin` is its left side
/// bearing, which maxp's recalculation keeps true or clears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum LsbIsXmin {
    Every,
    NotEvery,
}

/// `loca`'s format for a `glyf` this long: short while every offset
/// halves into 16 bits -- they are all even, each glyph padded to four --
/// as fontTools chooses it.
fn loca_format(glyf: Len) -> LocaFormat {
    if glyf.value() < 0x2_0000 {
        LocaFormat::Short
    } else {
        LocaFormat::Long
    }
}

/// A table of the output: where it lies and how long it is.
#[derive(Clone, Copy)]
struct Placed {
    tag: Tag,
    at: Offset,
    len: Len,
}

fn at_of(placed: &[Placed], t: Tag) -> Option<Offset> {
    placed.iter().find(|p| p.tag == t).map(|p| p.at)
}

fn tag_key(t: Tag) -> Word {
    Word(u32::from_be_bytes(t.as_bytes()))
}

fn pad4(l: Len) -> Result<Len, Refused> {
    let v = l.value().checked_add(3).ok_or(Refused::Metrics)? & !3;
    Ok(Len::of(v))
}

/// The sum of the words from `at` over `len`, a partial last word padded
/// with zeros -- the format's table checksum.
fn checksum(out: &Out<'_>, at: Offset, len: Len) -> Word {
    let mut sum = 0u32;
    let mut k = Len::of(0);
    while k < len {
        let o = at.plus(k).unwrap_or(at);
        sum = sum.wrapping_add(out.word(o).0);
        k = Len::of(k.value().saturating_add(4));
    }
    Word(sum)
}

const MAX_TABLES: usize = 64;

/// Bakes `font` at `wght` into `out`, answering the font's length. `scratch`
/// holds the points one glyph walk needs at once: the most points of any
/// simple glyph plus four, with a composite's own offsets on top for each
/// level it nests; 65,536 is ample for any font this bake accepts.
pub fn bake(
    font: &[u8],
    wght: Fixed,
    scratch: &mut [BakePoint],
    out: &mut [u8],
) -> Result<Len, Refused> {
    let face = Face::parse(font).map_err(|_| Refused::NotAFont)?;
    let version: u32 = Bytes::new(font)
        .read(Offset::ZERO)
        .ok_or(Refused::NotAFont)?;
    if version != 0x0001_0000 {
        return Err(Refused::NotTrueType);
    }
    let glyf = face.glyf().ok_or(Refused::NotTrueType)?;
    // One axis, and it is `wght`.
    let axis = face.axis(Index::ZERO).ok_or(Refused::Axes)?;
    if axis.tag != WGHT || face.axis(Index::of(1u32)).is_some() {
        return Err(Refused::Axes);
    }
    let avar = match face.table(Tag::AVAR) {
        Some(t) => Some(Avar::parse(t).ok_or(Refused::Malformed(Tag::AVAR))?),
        None => None,
    };
    let v = normalize(axis, avar.and_then(|a| a.map(Index::ZERO)), wght)
        .ok_or(Refused::Malformed(Tag::FVAR))?;
    let gvar = face.gvar();
    if gvar.is_some_and(|g| g.axes() != Count::of(1u32)) {
        return Err(Refused::Malformed(Tag::GVAR));
    }

    // The directory: every table the input lists, less the ones instancing
    // drops, in tag order; `glyf` placed last, since only it grows.
    let mut placed = [Placed {
        tag: Tag::HEAD,
        at: Offset::ZERO,
        len: Len::of(0),
    }; MAX_TABLES];
    let mut kept = 0usize;
    let mut i = Index::ZERO;
    while face.table_count().holds(i) {
        let (tag, bytes) = face.table_at(i).ok_or(Refused::NotAFont)?;
        i = i.next();
        if REFUSED.iter().any(|r| **r == tag.as_bytes()) {
            return Err(Refused::Table(tag));
        }
        if [Tag::FVAR, Tag::GVAR, Tag::AVAR, Tag::HVAR, STAT, DSIG].contains(&tag) {
            continue;
        }
        let bytes = bytes.ok_or(Refused::Malformed(tag))?;
        let slot = placed.get_mut(kept).ok_or(Refused::NotAFont)?;
        *slot = Placed {
            tag,
            at: Offset::ZERO,
            len: bytes.len(),
        };
        kept += 1;
    }
    let placed = &mut placed[..kept];
    placed.sort_unstable_by_key(|p| tag_key(p.tag));
    if placed.windows(2).any(|w| w[0].tag == w[1].tag) {
        return Err(Refused::NotAFont);
    }
    if let Some(avar) = face.table(Tag::AVAR) {
        let major: u16 = avar
            .read(Offset::ZERO)
            .ok_or(Refused::Malformed(Tag::AVAR))?;
        if major != 1 {
            return Err(Refused::Table(Tag::AVAR));
        }
    }
    let glyphs = face.glyph_count();
    let n = u32::from(glyphs.value());
    if n == 0 {
        return Err(Refused::Malformed(Tag::MAXP));
    }
    let table = |t: Tag| face.table(t).ok_or(Refused::Malformed(t));
    let n_tables = u16::try_from(kept).map_err(|_| Refused::NotAFont)?;
    let dir = Len::of(12)
        .value()
        .checked_add(16 * u32::from(n_tables))
        .ok_or(Refused::Metrics)?;
    // Every table but the three the glyphs write, in tag order; then `hmtx`,
    // `loca` and `glyf`, in full width while the glyphs are written, and
    // compacted after as fontTools compacts them -- `hmtx` trimmed, `loca`
    // short when `glyf` fits -- in one move rather than a second pass.
    let mut at = Offset::of(dir);
    for p in placed
        .iter_mut()
        .filter(|p| ![Tag::GLYF, Tag::LOCA, Tag::HMTX].contains(&p.tag))
    {
        p.at = at;
        at = at.plus(pad4(p.len)?).ok_or(Refused::Metrics)?;
    }
    let hmtx_at = at;
    let full = |w: u32, k: u32| Len::of(w).times(Count::of(k)).ok_or(Refused::Metrics);
    let loca_at = hmtx_at.plus(pad4(full(4, n)?)?).ok_or(Refused::Metrics)?;
    let glyf_at = loca_at
        .plus(pad4(full(4, n + 1)?)?)
        .ok_or(Refused::Metrics)?;

    // Every byte the writes skip -- padding, a table's tail -- is zero,
    // whatever the caller's buffer held.
    out.fill(0);
    let mut out = Out { buf: out };

    // Everything copied through, then the tables rewritten in place.
    for p in placed.iter() {
        if [Tag::GLYF, Tag::LOCA, Tag::HMTX].contains(&p.tag) {
            continue;
        }
        out.copy(p.at, table(p.tag)?.as_slice());
    }
    for t in [
        Tag::HEAD,
        Tag::HHEA,
        Tag::MAXP,
        Tag::LOCA,
        Tag::HMTX,
        Tag::GLYF,
    ] {
        at_of(placed, t).ok_or(Refused::Malformed(t))?;
    }
    for t in [Tag::GSUB, Tag::GPOS] {
        if let Some(b) = face.table(t) {
            no_feature_variations(t, b)?;
        }
    }
    let item_store = match face.table(Tag::GDEF) {
        Some(gdef) => {
            let store = gdef_downgrade(
                gdef,
                &mut out,
                at_of(placed, Tag::GDEF).ok_or(Refused::Metrics)?,
            )?;
            Some(store)
        }
        None => None,
    }
    .flatten();
    if let (Some(gpos), Some(dst)) = (face.table(Tag::GPOS), at_of(placed, Tag::GPOS)) {
        Anchors {
            src: gpos,
            dst,
            store: item_store,
            v,
        }
        .walk(&mut out)?;
    }

    // The glyphs.
    let ctx = Ctx {
        face: &face,
        glyf,
        gvar,
        v,
    };
    // Where the trailing run of equal advances begins, which is where
    // fontTools' `hmtx` stops writing records.
    let mut run_start = 0u32;
    let mut last_advance: Option<u16> = None;
    let mut tally = Tally {
        head: None,
        lsb_is_xmin: LsbIsXmin::Every,
        advance_max: 0,
        lsb_min: None,
        rsb_min: None,
        extent_max: None,
        advance_sum: 0.0,
        advances: 0.0,
    };
    let mut glyf_len = Len::of(0);
    let mut k = 0u32;
    while k < n {
        let g = GlyphId::from_shaper(u16::try_from(k).map_err(|_| Refused::Metrics)?);
        let bad = Refused::Glyph(g);
        out.put(
            loca_at.plus(Len::of(4 * k)).ok_or(Refused::Metrics)?,
            glyf_len.value(),
        );
        let mut budget = Budget(0);
        let p = ctx.visit(g, Depth(0), &mut budget)?;
        let count = ctx.points(g, p, scratch)?;
        let (own, rest) = scratch.split_at_mut(count);
        let bbox = ctx.bounds_of(g, p, own, rest, Depth(0), &mut budget)?;
        let ph = &own[count - 4..];
        let (left, right) = (ph[0].x, ph[1].x);
        let advance = ot_round(right - left).max(0.0);
        let advance = u16::try_from(advance as i32).map_err(|_| Refused::Metrics)?;
        let lsb = short(ot_round(f64::from(bbox.x_min) - left)).ok_or(Refused::Metrics)?;
        let hm = hmtx_at.plus(Len::of(4 * k)).ok_or(Refused::Metrics)?;
        out.put(hm, advance);
        if last_advance != Some(advance) {
            run_start = k;
            last_advance = Some(advance);
        }
        out.put(hm.plus(Len::of(2)).ok_or(Refused::Metrics)?, lsb);
        tally.advance_max = tally.advance_max.max(i32::from(advance));
        if advance > 0 {
            tally.advance_sum += f64::from(advance);
            tally.advances += 1.0;
        }
        let start = glyf_at.plus(glyf_len).ok_or(Refused::Metrics)?;
        let mut w = Cursor {
            out: &mut out,
            at: start,
        };
        if !matches!(p.shape, Shape::Empty) {
            w.put(p.contours)?;
            for c in [bbox.x_min, bbox.y_min, bbox.x_max, bbox.y_max] {
                w.put(i16::try_from(c).map_err(|_| bad)?)?;
            }
            match p.shape {
                Shape::Simple(s) => write_simple(g, s, &own[..count - 4], &mut w)?,
                Shape::Composite(body) => write_composite(g, body, &own[..count - 4], &mut w)?,
                Shape::Empty => {}
            }
            tally.head = Some(join(tally.head, bbox, Bbox::union));
            if i32::from(lsb) != bbox.x_min {
                tally.lsb_is_xmin = LsbIsXmin::NotEvery;
            }
            let width = bbox.x_max - bbox.x_min;
            let (lsb, adv) = (i32::from(lsb), i32::from(advance));
            tally.lsb_min = Some(join(tally.lsb_min, lsb, i32::min));
            tally.rsb_min = Some(join(tally.rsb_min, adv - lsb - width, i32::min));
            tally.extent_max = Some(join(tally.extent_max, lsb + width, i32::max));
            let written = w.at.since(start).ok_or(Refused::Metrics)?;
            glyf_len = glyf_len
                .value()
                .checked_add(pad4(written)?.value())
                .map(Len::of)
                .ok_or(Refused::Metrics)?;
        }
        k += 1;
    }
    out.put(
        loca_at.plus(Len::of(4 * n)).ok_or(Refused::Metrics)?,
        glyf_len.value(),
    );
    // The compaction. `hmtx` keeps records to the start of the trailing run
    // of equal advances (at least one), side bearings after; `loca` is short
    // while every offset halves into 16 bits, and they are all even.
    let long_metrics = run_start + 1;
    let hmtx_len = full(4, long_metrics)?
        .value()
        .checked_add(full(2, n - long_metrics)?.value())
        .map(Len::of)
        .ok_or(Refused::Metrics)?;
    let short_loca = loca_format(glyf_len) == LocaFormat::Short;
    let loca_len = full(if short_loca { 2 } else { 4 }, n + 1)?;
    let new_loca = hmtx_at.plus(pad4(hmtx_len)?).ok_or(Refused::Metrics)?;
    let new_glyf = new_loca.plus(pad4(loca_len)?).ok_or(Refused::Metrics)?;
    let need = glyf_at
        .plus(glyf_len)
        .ok_or(Refused::Metrics)?
        .since_start();
    let total = new_glyf
        .plus(glyf_len)
        .ok_or(Refused::Metrics)?
        .since_start();
    if usize::try_from(need.value()).map_or(true, |t| t > out.buf.len()) {
        return Err(Refused::Out(need));
    }
    let mut k = long_metrics;
    while k < n {
        let from = hmtx_at.plus(Len::of(4 * k + 2)).ok_or(Refused::Metrics)?;
        let to = hmtx_at
            .plus(Len::of(4 * long_metrics + 2 * (k - long_metrics)))
            .ok_or(Refused::Metrics)?;
        out.shift(from, Len::of(2), to);
        k += 1;
    }
    out.zero(hmtx_at.plus(hmtx_len).ok_or(Refused::Metrics)?, new_loca);
    let mut i = 0u32;
    while i <= n {
        let v = out
            .word(loca_at.plus(Len::of(4 * i)).ok_or(Refused::Metrics)?)
            .0;
        if short_loca {
            let half = u16::try_from(v / 2).map_err(|_| Refused::Metrics)?;
            out.put(new_loca.plus(Len::of(2 * i)).ok_or(Refused::Metrics)?, half);
        } else {
            out.put(new_loca.plus(Len::of(4 * i)).ok_or(Refused::Metrics)?, v);
        }
        i += 1;
    }
    out.zero(new_loca.plus(loca_len).ok_or(Refused::Metrics)?, new_glyf);
    out.shift(glyf_at, glyf_len, new_glyf);
    for p in placed.iter_mut() {
        if p.tag == Tag::GLYF {
            (p.at, p.len) = (new_glyf, glyf_len);
        } else if p.tag == Tag::LOCA {
            (p.at, p.len) = (new_loca, loca_len);
        } else if p.tag == Tag::HMTX {
            (p.at, p.len) = (hmtx_at, hmtx_len);
        }
    }
    let head_at = at_of(placed, Tag::HEAD).ok_or(Refused::Malformed(Tag::HEAD))?;
    let hhea_at = at_of(placed, Tag::HHEA).ok_or(Refused::Malformed(Tag::HHEA))?;
    let at = |base: Offset, k: u32| base.plus(Len::of(k)).ok_or(Refused::Metrics);
    if table(Tag::HEAD)?.len() < Len::of(54) || table(Tag::HHEA)?.len() < Len::of(36) {
        return Err(Refused::Malformed(Tag::HEAD));
    }
    let fit = |v: i32| i16::try_from(v).map_err(|_| Refused::Metrics);
    let b = tally.head.unwrap_or(Bbox::ZERO);
    for (k, c) in [(36, b.x_min), (38, b.y_min), (40, b.x_max), (42, b.y_max)] {
        out.put(at(head_at, k)?, fit(c)?);
    }
    let flags: u16 = table(Tag::HEAD)?
        .read(Offset::of(16u32))
        .ok_or(Refused::Malformed(Tag::HEAD))?;
    out.put(
        at(head_at, 16)?,
        match tally.lsb_is_xmin {
            LsbIsXmin::Every => flags | 2,
            LsbIsXmin::NotEvery => flags & !2,
        },
    );
    out.put(at(head_at, 50)?, if short_loca { 0i16 } else { 1i16 });
    out.put(at(head_at, 8)?, 0u32);
    out.put(
        at(hhea_at, 10)?,
        u16::try_from(tally.advance_max).map_err(|_| Refused::Metrics)?,
    );
    out.put(at(hhea_at, 12)?, fit(tally.lsb_min.unwrap_or(0))?);
    out.put(at(hhea_at, 14)?, fit(tally.rsb_min.unwrap_or(0))?);
    out.put(at(hhea_at, 16)?, fit(tally.extent_max.unwrap_or(0))?);
    out.put(
        at(hhea_at, 34)?,
        u16::try_from(long_metrics).map_err(|_| Refused::Metrics)?,
    );
    if let Some(os2) = at_of(placed, Tag::OS2) {
        if table(Tag::OS2)?.len() < Len::of(6) {
            return Err(Refused::Malformed(Tag::OS2));
        }
        let avg = if tally.advances == 0.0 {
            0.0
        } else {
            ot_round(tally.advance_sum / tally.advances)
        };
        out.put(at(os2, 2)?, short(avg).ok_or(Refused::Metrics)?);
        let user = f64::from(wght.0) / 65536.0;
        let min = f64::from(axis.min.0) / 65536.0;
        let max = f64::from(axis.max.0) / 65536.0;
        let weight = ot_round(user.clamp(min, max).clamp(1.0, 1000.0));
        out.put(
            at(os2, 4)?,
            u16::try_from(weight as i32).map_err(|_| Refused::Metrics)?,
        );
    }

    // The directory, the checksums, then the whole file's.
    out.put(Offset::ZERO, 0x0001_0000u32);
    out.put(Offset::of(4u32), n_tables);
    let mut pow = 1u16;
    let mut sel = 0u16;
    while pow.checked_mul(2).is_some_and(|p| p <= n_tables) {
        pow *= 2;
        sel += 1;
    }
    let range = pow * 16;
    out.put(Offset::of(6u32), range);
    out.put(Offset::of(8u32), sel);
    out.put(Offset::of(10u32), n_tables * 16 - range);
    for (k, p) in placed.iter().enumerate() {
        let rec = Offset::of(12u32 + 16 * u32::try_from(k).map_err(|_| Refused::Metrics)?);
        let sum = checksum(&out, p.at, p.len);
        out.put(rec, tag_key(p.tag));
        out.put(at(rec, 4)?, sum);
        out.put(at(rec, 8)?, p.at.since_start().value());
        out.put(at(rec, 12)?, p.len.value());
    }
    let whole_sum = checksum(&out, Offset::ZERO, total);
    out.put(at(head_at, 8)?, 0xB1B0_AFBAu32.wrapping_sub(whole_sum.0));
    Ok(total)
}

fn no_feature_variations(t: Tag, b: Bytes<'_>) -> Result<(), Refused> {
    let minor: u16 = b.read(Offset::of(2u32)).ok_or(Refused::Malformed(t))?;
    if minor >= 1 {
        let fv: u32 = b.read(Offset::of(10u32)).ok_or(Refused::Malformed(t))?;
        if fv != 0 {
            return Err(Refused::Table(t));
        }
    }
    Ok(())
}

/// GDEF: a version 1.3 header loses its item variation store -- to 1.2 when
/// it names mark glyph sets, to 1.0 when it does not, as fontTools
/// downgrades it -- and the store is answered for the anchors. A caret
/// with a device is refused.
fn gdef_downgrade<'g>(
    gdef: Bytes<'g>,
    out: &mut Out<'_>,
    dst: Offset,
) -> Result<Option<ItemStore<'g>>, Refused> {
    let bad = Refused::Malformed(Tag::GDEF);
    let minor: u16 = gdef.read(Offset::of(2u32)).ok_or(bad)?;
    let carets: u16 = gdef.read(Offset::of(8u32)).ok_or(bad)?;
    if carets != 0 {
        let list = gdef.tail(Offset::of(carets)).ok_or(bad)?;
        let n: u16 = list.read(Offset::of(2u32)).ok_or(bad)?;
        for k in 0..u32::from(n) {
            let o: u16 = list.read(Offset::of(4 + 2 * k)).ok_or(bad)?;
            let glyph = list.tail(Offset::of(o)).ok_or(bad)?;
            let m: u16 = glyph.read(Offset::ZERO).ok_or(bad)?;
            for j in 0..u32::from(m) {
                let c: u16 = glyph.read(Offset::of(2 + 2 * j)).ok_or(bad)?;
                let format: u16 = glyph.read(Offset::of(c)).ok_or(bad)?;
                if format == 3 {
                    return Err(Refused::Device);
                }
            }
        }
    }
    if minor < 3 {
        return Ok(None);
    }
    let at: u32 = gdef.read(Offset::of(14u32)).ok_or(bad)?;
    if at == 0 {
        return Ok(None);
    }
    let sets: u16 = gdef.read(Offset::of(12u32)).ok_or(bad)?;
    out.put(
        dst.plus(Len::of(2)).ok_or(Refused::Metrics)?,
        if sets != 0 { 2u16 } else { 0u16 },
    );
    let store = ItemStore::parse(gdef.tail(Offset::of(at)).ok_or(bad)?).ok_or(bad)?;
    Ok(Some(store))
}

/// The GPOS walk: every anchor any lookup reaches, patched from the input's
/// bytes into the output's, so an anchor two subtables share is written
/// the same twice.
struct Anchors<'g> {
    src: Bytes<'g>,
    dst: Offset,
    store: Option<ItemStore<'g>>,
    v: F2Dot14,
}

/// A position in `GPOS`, wide enough that no sum of offsets and counts a
/// table can name overflows it; a position past the table is malformed.
#[derive(Clone, Copy)]
struct At(u64);

impl At {
    fn plus(self, n: At) -> At {
        At(self.0 + n.0)
    }
}

/// A lookup's type, as the lookup list stores it.
#[derive(Clone, Copy)]
struct LookupKind(u64);

impl Anchors<'_> {
    fn bad() -> Refused {
        Refused::Malformed(Tag::GPOS)
    }

    fn offset(at: At) -> Result<Offset, Refused> {
        u32::try_from(at.0).map(Offset::of).map_err(|_| Self::bad())
    }

    fn u16_at(&self, at: At) -> Result<u64, Refused> {
        let v: u16 = self.src.read(Self::offset(at)?).ok_or(Self::bad())?;
        Ok(u64::from(v))
    }

    /// The position an offset stored at `field` names, from `base`.
    fn follow(&self, base: At, field: At) -> Result<At, Refused> {
        Ok(base.plus(At(self.u16_at(field)?)))
    }

    fn walk(&self, out: &mut Out<'_>) -> Result<(), Refused> {
        let lookups = self.u16_at(At(8))?;
        if lookups == 0 {
            return Ok(());
        }
        let lookups = At(lookups);
        let count = self.u16_at(lookups)?;
        for k in 0..count {
            let lookup = self.follow(lookups, lookups.plus(At(2 + 2 * k)))?;
            let kind = self.u16_at(lookup)?;
            let subs = self.u16_at(lookup.plus(At(4)))?;
            for j in 0..subs {
                let sub = self.follow(lookup, lookup.plus(At(6 + 2 * j)))?;
                if kind == 9 {
                    let real = self.u16_at(sub.plus(At(2)))?;
                    let off: u32 = self
                        .src
                        .read(Self::offset(sub.plus(At(4)))?)
                        .ok_or(Self::bad())?;
                    self.subtable(LookupKind(real), sub.plus(At(u64::from(off))), out)?;
                } else {
                    self.subtable(LookupKind(kind), sub, out)?;
                }
            }
        }
        Ok(())
    }

    fn subtable(&self, kind: LookupKind, sub: At, out: &mut Out<'_>) -> Result<(), Refused> {
        const DEVICES: u64 = 0x00F0;
        match kind.0 {
            1 => {
                if self.u16_at(sub.plus(At(4)))? & DEVICES != 0 {
                    return Err(Refused::Device);
                }
            }
            2 => {
                if (self.u16_at(sub.plus(At(4)))? | self.u16_at(sub.plus(At(6)))?) & DEVICES != 0 {
                    return Err(Refused::Device);
                }
            }
            3 => {
                let n = self.u16_at(sub.plus(At(4)))?;
                for r in 0..n {
                    for e in [0, 2] {
                        self.anchor_at(sub, sub.plus(At(6 + 4 * r + e)), out)?;
                    }
                }
            }
            4 | 6 => {
                let classes = self.u16_at(sub.plus(At(6)))?;
                self.marks(self.follow(sub, sub.plus(At(8)))?, out)?;
                let arr = self.follow(sub, sub.plus(At(10)))?;
                let n = self.u16_at(arr)?;
                for r in 0..n * classes {
                    self.anchor_at(arr, arr.plus(At(2 + 2 * r)), out)?;
                }
            }
            5 => {
                let classes = self.u16_at(sub.plus(At(6)))?;
                self.marks(self.follow(sub, sub.plus(At(8)))?, out)?;
                let arr = self.follow(sub, sub.plus(At(10)))?;
                let n = self.u16_at(arr)?;
                for l in 0..n {
                    let attach = self.follow(arr, arr.plus(At(2 + 2 * l)))?;
                    let comps = self.u16_at(attach)?;
                    for r in 0..comps * classes {
                        self.anchor_at(attach, attach.plus(At(2 + 2 * r)), out)?;
                    }
                }
            }
            7 | 8 => {}
            _ => return Err(Self::bad()),
        }
        Ok(())
    }

    /// A MarkArray at `arr`: each record's class, then its anchor.
    fn marks(&self, arr: At, out: &mut Out<'_>) -> Result<(), Refused> {
        let n = self.u16_at(arr)?;
        for r in 0..n {
            self.anchor_at(arr, arr.plus(At(2 + 4 * r + 2)), out)?;
        }
        Ok(())
    }

    /// The anchor whose offset, from `base`, lies at `field`; a null offset
    /// names none.
    fn anchor_at(&self, base: At, field: At, out: &mut Out<'_>) -> Result<(), Refused> {
        let off = self.u16_at(field)?;
        if off == 0 {
            return Ok(());
        }
        let a = base.plus(At(off));
        match self.u16_at(a)? {
            1 | 2 => Ok(()),
            3 => {
                let x: i16 = self
                    .src
                    .read(Self::offset(a.plus(At(2)))?)
                    .ok_or(Self::bad())?;
                let y: i16 = self
                    .src
                    .read(Self::offset(a.plus(At(4)))?)
                    .ok_or(Self::bad())?;
                let dx = self.delta(a, At(self.u16_at(a.plus(At(6)))?))?;
                let dy = self.delta(a, At(self.u16_at(a.plus(At(8)))?))?;
                let x = short(f64::from(x) + dx).ok_or(Refused::Metrics)?;
                let y = short(f64::from(y) + dy).ok_or(Refused::Metrics)?;
                let at = |k: u64| -> Result<Offset, Refused> {
                    let o = Self::offset(a.plus(At(k)))?;
                    self.dst.plus(o.since_start()).ok_or(Refused::Metrics)
                };
                out.put(at(0)?, 1u16);
                out.put(at(2)?, x);
                out.put(at(4)?, y);
                Ok(())
            }
            _ => Err(Self::bad()),
        }
    }

    /// A device's rounded delta: 0 for none, the item store's for a
    /// variation index, refused for a hinting device.
    fn delta(&self, anchor: At, off: At) -> Result<f64, Refused> {
        if off.0 == 0 {
            return Ok(0.0);
        }
        let dev = self
            .src
            .tail(Self::offset(anchor.plus(off))?)
            .ok_or(Self::bad())?;
        let format: u16 = dev.read(Offset::of(4u32)).ok_or(Self::bad())?;
        if format != 0x8000 {
            return Err(Refused::Device);
        }
        let index = VarIndex::of_device(dev).ok_or(Self::bad())?;
        if index.is_none() {
            return Ok(0.0);
        }
        let store = self.store.ok_or(Refused::Malformed(Tag::GDEF))?;
        let mut sum: Option<f64> = None;
        store
            .columns(index, |region, d| {
                let s = tent(region[0], region[1], region[2], self.v)?;
                if s != 0.0 {
                    let c = if s == 1.0 {
                        f64::from(d)
                    } else {
                        f64::from(d) * s
                    };
                    sum = Some(sum.map_or(c, |a| a + c));
                }
                Some(())
            })
            .ok_or(Refused::Malformed(Tag::GDEF))?;
        Ok(ot_round(sum.unwrap_or(0.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The golden values are fontTools' own, from `iup_contour` and
    /// `supportScalar` run on the same inputs; each input was found by
    /// searching for one where the alternative order gives another double.
    #[test]
    fn the_inference_is_fonttools_segment_formula() {
        // HarfBuzz's form, (x - x1) / (x2 - x1) * (d2 - d1), gives
        // 12.787500000000023 here.
        let d = iup(243.0, (-68.0, -271.0), (492.0, 240.0));
        assert_eq!(d.to_bits(), 12.787_499_999_999_966_f64.to_bits());
        assert_eq!(iup(5.0, (3.0, 7.0), (3.0, 7.0)), 7.0);
        assert_eq!(iup(5.0, (3.0, 7.0), (3.0, 8.0)), 0.0);
        assert_eq!(iup(1.0, (3.0, 7.0), (9.0, 8.0)), 7.0);
        assert_eq!(iup(10.0, (9.0, 8.0), (3.0, 7.0)), 8.0);
    }

    /// One glyph's variation data by hand: a peak at wght 1.0, point 0 and
    /// point `second` named with x deltas 15 and -43, the rest inferred.
    fn one_tuple(second: Index) -> [u8; 42] {
        let mut t = [0u8; 42];
        let header: [u16; 10] = [1, 0, 1, 0, 0, 24, 1, 0, 0, 24];
        for (k, v) in header.iter().enumerate() {
            t[2 * k..2 * k + 2].copy_from_slice(&v.to_be_bytes());
        }
        // sharedTuplesOffset and the array offset are 32 bits: 24 each.
        t[8..12].copy_from_slice(&24u32.to_be_bytes());
        t[12..14].copy_from_slice(&1u16.to_be_bytes());
        t[14..16].copy_from_slice(&0u16.to_be_bytes());
        t[16..20].copy_from_slice(&24u32.to_be_bytes());
        t[20..22].copy_from_slice(&0u16.to_be_bytes());
        t[22..24].copy_from_slice(&9u16.to_be_bytes());
        let glyph: [u8; 18] = [
            0x00,
            0x01,
            0x00,
            0x0A, // one tuple, data at 10
            0x00,
            0x08,
            0xA0,
            0x00,
            0x40,
            0x00, // 8 bytes; embedded peak, private points; peak 1.0
            0x02,
            0x01,
            0x00,
            u8::try_from(second.value()).unwrap(), // points 0 and `second`, as a difference
            0x01,
            15,
            0xD5, // x: 15, -43
            0x81, // y: two zeros
        ];
        t[24..].copy_from_slice(&glyph);
        t
    }

    #[test]
    fn a_tuple_is_scaled_before_it_is_inferred() {
        let table = one_tuple(Index::of(2u32));
        let gvar = crate::gvar::Gvar::parse(Bytes::new(&table)).unwrap();
        let store = gvar.glyph(GlyphId::from_shaper(0));
        assert!(store.is_some());
        let mut pts = [BakePoint::ZERO; 7];
        for (p, x) in pts.iter_mut().zip([-327.0, -131.0, 355.0]) {
            *p = BakePoint::at(x, 0.0, LastPoint::No);
        }
        pts[2].last = LastPoint::Yes;
        for p in &mut pts[3..] {
            p.last = LastPoint::Yes;
        }
        vary(store, F2Dot14(9928), &mut pts).unwrap();
        // Inferred after scaling; scaling the inferred delta instead gives
        // -1.0111130521444278.
        assert_eq!(pts[1].sx.to_bits(), 0xbff0_2d84_e138_4e18);
        assert_eq!(pts[1].x, -131.0 + f64::from_bits(0xbff0_2d84_e138_4e18));
        assert_eq!(pts[0].x, -327.0 + 15.0 * 9928.0 / 16384.0);
        assert!(pts[3..].iter().all(|p| p.x == 0.0 && p.sx == 0.0));
    }

    #[test]
    fn an_intermediate_scalar_is_kept_in_f64() {
        let s = tent(F2Dot14(3277), F2Dot14(8192), F2Dot14(16384), F2Dot14(5000));
        // Through f32 it would be 0x3fd66f9120000000.
        assert_eq!(s.map(f64::to_bits), Some(0x3fd6_6f91_2983_195e));
        assert_eq!(
            tent(F2Dot14(-8192), F2Dot14(8192), F2Dot14(16384), F2Dot14(1)),
            None
        );
        assert_eq!(
            tent(F2Dot14(0), F2Dot14(0), F2Dot14(0), F2Dot14(9)),
            Some(1.0)
        );
        assert_eq!(
            tent(F2Dot14(0), F2Dot14(8192), F2Dot14(16384), F2Dot14(0)),
            Some(0.0)
        );
        assert_eq!(
            tent(F2Dot14(0), F2Dot14(8192), F2Dot14(16384), F2Dot14(8192)),
            Some(1.0)
        );
    }

    #[test]
    fn a_half_rounds_up() {
        assert_eq!(ot_round(2.5), 3.0);
        assert_eq!(ot_round(-2.5), -2.0);
        assert_eq!(ot_round(-0.5), 0.0);
        assert_eq!(ot_round(-3.5000001), -4.0);
    }

    /// A point list that names point 0 twice: fontTools keeps the last
    /// delta, and the other points of the contour are inferred from it.
    #[test]
    fn a_point_named_twice_keeps_its_last_delta() {
        let table = one_tuple(Index::ZERO);
        let gvar = crate::gvar::Gvar::parse(Bytes::new(&table)).unwrap();
        let mut pts = [BakePoint::ZERO; 7];
        for (p, x) in pts.iter_mut().zip([-327.0, -131.0, 355.0]) {
            *p = BakePoint::at(x, 0.0, LastPoint::No);
        }
        pts[2].last = LastPoint::Yes;
        for p in &mut pts[3..] {
            p.last = LastPoint::Yes;
        }
        vary(
            gvar.glyph(GlyphId::from_shaper(0)),
            F2Dot14(16384),
            &mut pts,
        )
        .unwrap();
        assert_eq!(pts[0].sx, -43.0);
        assert_eq!(pts[1].sx, -43.0);
        assert_eq!(pts[2].sx, -43.0);
    }

    /// `piecewiseLinearMap` reads the map as a dictionary: a repeated key
    /// keeps its last value wherever the key is used -- as the exact
    /// answer, the smallest, the largest, or a side of an interval.
    #[test]
    fn avar_is_read_as_a_dictionary() {
        fn map(records: &[(i16, i16)]) -> ([u8; 32], Count) {
            let mut b = [0u8; 32];
            for (k, (f, t)) in records.iter().enumerate() {
                b[4 * k..4 * k + 2].copy_from_slice(&f.to_be_bytes());
                b[4 * k + 2..4 * k + 4].copy_from_slice(&t.to_be_bytes());
            }
            (b, Count::of(u32::try_from(records.len()).unwrap()))
        }
        let at = |records: &[(i16, i16)], v: f64| {
            let (b, n) = map(records);
            piecewise(v, Bytes::new(&b), n).unwrap()
        };
        let half = 8192;
        // The smallest key twice, below it.
        let r = [(-half, -13107), (-half, -6554), (0, 0), (16384, 16384)];
        assert_eq!(at(&r, -0.75), -0.75 + -6554.0 / 16384.0 + 0.5);
        // The key itself.
        assert_eq!(at(&r, -0.5), -6554.0 / 16384.0);
        // A side of an interval.
        let r = [(0, 0), (half, 4000), (half, 12000), (16384, 16384)];
        assert_eq!(at(&r, 0.25), 12000.0 / 16384.0 * 0.5);
        assert_eq!(
            at(&r, 0.75),
            12000.0 / 16384.0 + (1.0 - 12000.0 / 16384.0) * 0.5
        );
        // The largest key twice, above it.
        let r = [(0, 0), (half, 4000), (half, 12000)];
        assert_eq!(at(&r, 0.75), 0.75 + 12000.0 / 16384.0 - 0.5);
    }

    #[test]
    fn loca_is_short_while_every_offset_halves_into_sixteen_bits() {
        assert!(loca_format(Len::of(0x1_FFFC)) == LocaFormat::Short);
        assert!(loca_format(Len::of(0x1_FFFE)) == LocaFormat::Short);
        assert!(loca_format(Len::of(0x2_0000)) == LocaFormat::Long);
    }
}
