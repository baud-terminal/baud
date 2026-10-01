//! Signed-area rasterisation of an outline into an 8-bit coverage mask
//! (`[[source: font-rs-raster]]`, `[[source: font-rs-accumulate]]`,
//! `[[source: ab-glyph-raster]]`).
//!
//! **The algorithm, in one paragraph.** Every segment of the outline is
//! flattened to lines; each line walks the scanlines it crosses and deposits
//! the signed area it sweeps into the one or two cells its span touches,
//! with the sign of its direction; a prefix sum along each row then turns
//! those deltas into the winding-weighted area under the outline at every
//! cell, and `min(|area|, 1) * 255` is the coverage. That is the font-rs
//! form the register (D6) chose as the smallest correct exact-area
//! rasteriser, and it is the form `ab_glyph_rasterizer` carries: this
//! module is held **byte-identical** to it in `tests/raster.rs`, which is
//! why the quad count, the cubic subdivision and the quantisation are
//! spelled exactly as that reference spells them rather than as a paper
//! would. FreeType's `ftgrays.c` (`[[source: freetype-ftgrays]]`) computes
//! the same area by a cell sweep and is the tolerance oracle.
//!
//! **No hinting, linear coverage** (D7): the outline is scaled and placed by
//! a [`Transform`] and nothing snaps it to the grid. Gamma is the
//! compositor's.
//!
//! **Nothing is allocated.** The caller provides an `f32` scratch of
//! [`Extent::scratch`] cells and the `u8` mask of [`Extent::cells`]; a glyph
//! at a terminal size is a few kilobytes of each and the renderer keeps one
//! of both. The scratch has four cells of slack past the last row because
//! the line splat may deposit one cell past the right edge of a row it
//! touches exactly, and the accumulator's contract is that the outline lies
//! **inside** the canvas -- [`Fit`] leaves a pixel of margin on every side
//! so it does. A caller that rasterises a larger outline into a smaller
//! canvas gets the reference's behaviour: deposits past a row's end land at
//! the start of the next row, and deposits past the buffer are dropped.

use crate::face::{Advance, UnitsPerEm};
use crate::outline::{Sink, Transform};
use core::f32::math::{ceil, floor, round, sqrt};

/// A count of pixels along one axis. `repr(transparent)`, so a uniform
/// block a shader reads may carry it where it means a pixel count.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Px(u32);

impl Px {
    pub const fn of(v: u32) -> Px {
        Px(v)
    }

    pub const fn value(self) -> u32 {
        self.0
    }

    fn index(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

/// A mask's size in pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Extent {
    pub w: Px,
    pub h: Px,
}

impl Extent {
    pub const fn new(w: Px, h: Px) -> Extent {
        Extent { w, h }
    }

    /// Cells in the mask: `w * h`.
    pub fn cells(self) -> Option<usize> {
        self.w.index()?.checked_mul(self.h.index()?)
    }

    /// `f32` cells the accumulator needs: one per mask cell plus the four
    /// of slack the line splat may run into on the last row.
    pub fn scratch(self) -> Option<usize> {
        self.cells()?.checked_add(4)
    }
}

/// A point in pixels, `y` downward.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    fn lerp(t: f32, p0: Point, p1: Point) -> Point {
        Point::new(p0.x + t * (p1.x - p0.x), p0.y + t * (p1.y - p0.y))
    }

    fn distance_to(self, other: Point) -> f32 {
        let (dx, dy) = (other.x - self.x, other.y - self.y);
        sqrt(dx * dx + dy * dy)
    }
}

/// How far a cubic has been subdivided; the reference stops at sixteen.
#[derive(Clone, Copy)]
struct Depth(u8);

impl Depth {
    const ZERO: Depth = Depth(0);
    const LIMIT: Depth = Depth(16);

    fn deeper(self) -> Depth {
        Depth(self.0.saturating_add(1))
    }
}

/// A row index from a pixel coordinate, the way the reference takes it:
/// truncation with negatives saturating to the first row.
fn row(y: f32) -> usize {
    usize::try_from(y as i32).unwrap_or(0)
}

/// The accumulation buffer for one mask.
pub struct Canvas<'a> {
    extent: Extent,
    a: &'a mut [f32],
}

impl<'a> Canvas<'a> {
    /// A cleared canvas over the first [`Extent::scratch`] cells of
    /// `scratch`, or `None` when it is too short.
    pub fn new(extent: Extent, scratch: &'a mut [f32]) -> Option<Canvas<'a>> {
        let a = scratch.get_mut(..extent.scratch()?)?;
        a.fill(0.0);
        Some(Canvas { extent, a })
    }

    pub fn extent(&self) -> Extent {
        self.extent
    }

    fn deposit(&mut self, at: i64, v: f32) -> Option<()> {
        *self.a.get_mut(usize::try_from(at).ok()?)? += v;
        Some(())
    }

    /// One scanline of a line's splat. `None` is the reference's
    /// `continue`: a deposit that fell outside the buffer abandons the rest
    /// of the row, including the advance of `x`.
    #[allow(clippy::too_many_arguments)]
    fn splat(&mut self, linestart: usize, x: f32, xnext: f32, d: f32) -> Option<()> {
        let (x0, x1) = if x < xnext { (x, xnext) } else { (xnext, x) };
        let x0floor = floor(x0);
        let x0i = x0floor as i32;
        let x1ceil = ceil(x1);
        let x1i = x1ceil as i32;
        let start = i64::try_from(linestart).ok()? + i64::from(x0i);
        if start < 0 {
            return None;
        }
        if x1i <= x0i + 1 {
            let xmf = 0.5 * (x + xnext) - x0floor;
            self.deposit(start, d - d * xmf)?;
            self.deposit(start + 1, d * xmf)?;
        } else {
            let s = (x1 - x0).recip();
            let x0f = x0 - x0floor;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = x1 - x1ceil + 1.0;
            let am = 0.5 * s * x1f * x1f;
            self.deposit(start, d * a0)?;
            if x1i == x0i + 2 {
                self.deposit(start + 1, d * (1.0 - a0 - am))?;
            } else {
                let a1 = s * (1.5 - x0f);
                self.deposit(start + 1, d * (a1 - a0))?;
                for xi in x0i + 2..x1i - 1 {
                    self.deposit(i64::try_from(linestart).ok()? + i64::from(xi), d * s)?;
                }
                let a2 = a1 + (x1i - x0i - 3) as f32 * s;
                self.deposit(
                    i64::try_from(linestart).ok()? + i64::from(x1i - 1),
                    d * (1.0 - a2 - am),
                )?;
            }
            self.deposit(i64::try_from(linestart).ok()? + i64::from(x1i), d * am)?;
        }
        Some(())
    }

    /// A straight edge from `p0` to `p1`.
    pub fn line(&mut self, p0: Point, p1: Point) {
        if (p0.y - p1.y).abs() <= f32::EPSILON {
            return;
        }
        let (dir, p0, p1) = if p0.y < p1.y {
            (1.0, p0, p1)
        } else {
            (-1.0, p1, p0)
        };
        let dxdy = (p1.x - p0.x) / (p1.y - p0.y);
        let mut x = p0.x;
        let y0 = row(p0.y);
        if p0.y < 0.0 {
            x -= p0.y * dxdy;
        }
        let (Some(w), Some(h)) = (self.extent.w.index(), self.extent.h.index()) else {
            return;
        };
        for y in y0..h.min(row(ceil(p1.y))) {
            let linestart = y * w;
            let dy = ((y + 1) as f32).min(p1.y) - (y as f32).max(p0.y);
            let xnext = x + dxdy * dy;
            let d = dy * dir;
            if self.splat(linestart, x, xnext, d).is_none() {
                continue;
            }
            x = xnext;
        }
    }

    /// A quadratic from `p0` to `p2` about `p1`, flattened to a count of
    /// lines fixed by the control point's deviation: `1 + ⌊(3·|p0 − 2p1 +
    /// p2|²)^¼⌋`, known before any is emitted.
    pub fn quad(&mut self, p0: Point, p1: Point, p2: Point) {
        let devx = p0.x - 2.0 * p1.x + p2.x;
        let devy = p0.y - 2.0 * p1.y + p2.y;
        let devsq = devx * devx + devy * devy;
        if devsq < 0.333 {
            self.line(p0, p2);
            return;
        }
        let tol = 3.0;
        let n = 1 + usize::try_from(floor(sqrt(sqrt(tol * devsq))) as i32).unwrap_or(0);
        let mut p = p0;
        let nrecip = (n as f32).recip();
        let mut t = 0.0;
        for _ in 0..n - 1 {
            t += nrecip;
            let pn = Point::lerp(t, Point::lerp(t, p0, p1), Point::lerp(t, p1, p2));
            self.line(p, pn);
            p = pn;
        }
        self.line(p, p2);
    }

    /// A cubic from `p0` to `p3` about `p1` and `p2`, split at the midpoint
    /// until the control polygon is within 0.35 px of the chord (the
    /// stb_truetype criterion the reference adopted) or sixteen deep.
    pub fn cubic(&mut self, p0: Point, p1: Point, p2: Point, p3: Point) {
        self.split_cubic(p0, p1, p2, p3, Depth::ZERO);
    }

    fn split_cubic(&mut self, p0: Point, p1: Point, p2: Point, p3: Point, depth: Depth) {
        const FLATNESS: f32 = 0.35;
        let longlen = p0.distance_to(p1) + p1.distance_to(p2) + p2.distance_to(p3);
        let shortlen = p0.distance_to(p3);
        let flatness_squared = longlen * longlen - shortlen * shortlen;
        if depth.0 < Depth::LIMIT.0 && flatness_squared > FLATNESS * FLATNESS {
            let p01 = Point::lerp(0.5, p0, p1);
            let p12 = Point::lerp(0.5, p1, p2);
            let p23 = Point::lerp(0.5, p2, p3);
            let pa = Point::lerp(0.5, p01, p12);
            let pb = Point::lerp(0.5, p12, p23);
            let mp = Point::lerp(0.5, pa, pb);
            self.split_cubic(p0, p01, pa, mp, depth.deeper());
            self.split_cubic(mp, pb, p23, p3, depth.deeper());
        } else {
            self.line(p0, p3);
        }
    }

    /// The mask: a prefix sum along the buffer, `min(|acc|, 1) * 255` per
    /// cell, into the first [`Extent::cells`] bytes of `out`. `None` when
    /// `out` is too short.
    pub fn coverage(&self, out: &mut [u8]) -> Option<()> {
        let n = self.extent.cells()?;
        let out = out.get_mut(..n)?;
        let mut acc = 0.0f32;
        for (o, c) in out.iter_mut().zip(self.a.get(..n)?) {
            acc += c;
            let y = acc.abs();
            let y = if y < 1.0 { y } else { 1.0 };
            *o = u8::try_from((255.0 * y) as i32).unwrap_or(u8::MAX);
        }
        Some(())
    }
}

/// Pixels per font unit.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scale(f32);

impl Scale {
    /// `px` pixels per em over a face of `upem` units per em.
    pub fn of(px: f32, upem: UnitsPerEm) -> Scale {
        Scale(px / f32::from(upem.value()))
    }

    /// `units` font units in pixels at this scale.
    pub fn px(self, units: f32) -> f32 {
        units * self.0
    }

    /// The transform that scales font units to pixels, flips `y` to run
    /// downward, and moves the result by `(dx, dy)` pixels.
    pub fn placing(self, dx: f32, dy: f32) -> Transform {
        Transform {
            a: self.0,
            b: 0.0,
            c: 0.0,
            d: -self.0,
            e: dx,
            f: dy,
        }
    }
}

/// Where a glyph sits horizontally inside its span: an offset to the
/// right, in 1/256 px so it compares exactly.
///
/// A cell is a whole number of pixels and the font's advance is not, so
/// rasterising at the cell's left edge leaves all the rounding slack on the
/// right. [`Phase::centring`] is half of it, the one offset ghostty's
/// CoreText path draws every glyph at (note J, decision 7): text is centred
/// in the block cursor at every size, and spacing does not change, because
/// every glyph moves by the same amount. A CJK fallback face is centred the
/// same way in its two cells, which can take more than a pixel; the whole
/// pixels only move the mask, and the fraction is what is rasterised.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Phase(i16);

impl Phase {
    /// No offset: the glyph origin on the span's left edge.
    pub const ZERO: Phase = Phase(0);

    /// Half the slack between a span `span` pixels wide and an `advance`
    /// at `scale`, rounded to 1/256 px; `None` when the advance is not
    /// within half the span of it, which says the face does not belong in
    /// that span at all.
    pub fn centring(scale: Scale, advance: Advance, span: Px) -> Option<Phase> {
        let span = f32::from(u16::try_from(span.0).ok()?);
        let slack = span - scale.px(f32::from(advance.value()));
        let q = round(slack * 128.0);
        (q.is_finite() && slack.abs() < span / 2.0).then_some(Phase(q as i16))
    }

    fn px(self) -> f32 {
        f32::from(self.0) / 256.0
    }
}

/// Whether a [`Bounds`] has seen a point yet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seen {
    Nothing,
    Points,
}

/// An outline's control-point box in whole font units.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InkBox {
    pub left: i32,
    pub right: i32,
    pub bottom: i32,
    pub top: i32,
}

/// A finite font-unit coordinate as an integer, truncated toward zero.
fn units(v: f32) -> i32 {
    v as i32
}

/// A [`Sink`] that records the outline's control-point box in font units.
/// Control points bound the curve, so the box bounds the outline.
pub struct Bounds {
    seen: Seen,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds::new()
    }
}

impl Bounds {
    pub const fn new() -> Bounds {
        Bounds {
            seen: Seen::Nothing,
            min_x: 0.0,
            min_y: 0.0,
            max_x: 0.0,
            max_y: 0.0,
        }
    }

    /// The box in whole font units, or `None` for an empty outline. Every
    /// `glyf` coordinate is an integer, so the truncation is exact there;
    /// a CFF path can carry fractions and loses them, as HarfBuzz's own
    /// extents do.
    pub fn ink(&self) -> Option<InkBox> {
        if self.seen == Seen::Nothing {
            return None;
        }
        let all = [self.min_x, self.max_x, self.min_y, self.max_y];
        if !all.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(InkBox {
            left: units(self.min_x),
            right: units(self.max_x),
            bottom: units(self.min_y),
            top: units(self.max_y),
        })
    }

    fn see(&mut self, x: f32, y: f32) {
        match self.seen {
            Seen::Nothing => {
                self.seen = Seen::Points;
                self.min_x = x;
                self.max_x = x;
                self.min_y = y;
                self.max_y = y;
            }
            Seen::Points => {
                self.min_x = self.min_x.min(x);
                self.max_x = self.max_x.max(x);
                self.min_y = self.min_y.min(y);
                self.max_y = self.max_y.max(y);
            }
        }
    }
}

impl Sink for Bounds {
    fn move_to(&mut self, x: f32, y: f32) {
        self.see(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.see(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.see(x1, y1);
        self.see(x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.see(x1, y1);
        self.see(x2, y2);
        self.see(x, y);
    }
    fn close(&mut self) {}
}

/// Where a glyph's mask goes: its size, the transform that draws the
/// outline into it, and the canvas origin relative to the glyph origin in
/// pixels -- `left` to the right of the origin, `top` above it.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    pub extent: Extent,
    pub transform: Transform,
    pub left: i32,
    pub top: i32,
}

impl Fit {
    /// A mask is never larger than this on a side. A glyph at a terminal
    /// size is tens of pixels; a font whose outline asks for more at 24 px
    /// is asking for a buffer, not a glyph (E-fontformat hazard 7).
    pub const LIMIT: Px = Px(4096);

    /// The fit for `bounds` at `scale`, with a pixel of margin on every
    /// side; `None` for an empty outline, a non-finite one, or one past
    /// [`Fit::LIMIT`].
    pub fn of(bounds: &Bounds, scale: Scale) -> Option<Fit> {
        Fit::at(bounds, scale, Phase::ZERO)
    }

    /// [`Fit::of`] with the outline moved `phase` to the right, so the
    /// rasteriser draws it at that fraction of a pixel; `left` still counts
    /// whole pixels from the glyph origin.
    pub fn at(bounds: &Bounds, scale: Scale, phase: Phase) -> Option<Fit> {
        if bounds.seen == Seen::Nothing {
            return None;
        }
        let s = scale.0;
        let dx = phase.px();
        // y flips: the outline's top is the canvas's smallest row.
        let (x0, x1) = (bounds.min_x * s + dx, bounds.max_x * s + dx);
        let (y0, y1) = (-bounds.max_y * s, -bounds.min_y * s);
        if !(x0.is_finite() && x1.is_finite() && y0.is_finite() && y1.is_finite()) {
            return None;
        }
        let limit = f32::from(u16::try_from(Fit::LIMIT.0).ok()?);
        if x0.abs() > limit || x1.abs() > limit || y0.abs() > limit || y1.abs() > limit {
            return None;
        }
        let left = floor(x0) as i32 - 1;
        let top = floor(y0) as i32 - 1;
        let right = ceil(x1) as i32 + 1;
        let bottom = ceil(y1) as i32 + 1;
        let w = u32::try_from(right - left).ok()?;
        let h = u32::try_from(bottom - top).ok()?;
        if w > Fit::LIMIT.0 || h > Fit::LIMIT.0 {
            return None;
        }
        Some(Fit {
            extent: Extent::new(Px(w), Px(h)),
            transform: scale.placing(dx - left as f32, -(top as f32)),
            left,
            top,
        })
    }
}

/// Whether the current contour has been started and not yet closed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Contour {
    Closed,
    Open,
}

/// A [`Sink`] that draws an outline onto a [`Canvas`] through a
/// [`Transform`]. Contours close themselves: a `move_to` or `close` on an
/// open contour draws the edge back to its start, so a shape the font left
/// open is still filled as the specification requires.
pub struct Rasterizer<'a> {
    canvas: Canvas<'a>,
    transform: Transform,
    start: Point,
    cur: Point,
    contour: Contour,
}

impl<'a> Rasterizer<'a> {
    pub fn new(canvas: Canvas<'a>, transform: Transform) -> Rasterizer<'a> {
        Rasterizer {
            canvas,
            transform,
            start: Point::new(0.0, 0.0),
            cur: Point::new(0.0, 0.0),
            contour: Contour::Closed,
        }
    }

    fn at(&self, x: f32, y: f32) -> Point {
        let (px, py) = self.transform.apply(x, y);
        Point::new(px, py)
    }

    fn close_contour(&mut self) {
        if self.contour == Contour::Open && self.cur != self.start {
            self.canvas.line(self.cur, self.start);
        }
        self.contour = Contour::Closed;
    }

    /// Closes any open contour and hands the canvas back for
    /// [`Canvas::coverage`].
    pub fn finish(mut self) -> Canvas<'a> {
        self.close_contour();
        self.canvas
    }
}

impl Sink for Rasterizer<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_contour();
        self.start = self.at(x, y);
        self.cur = self.start;
        self.contour = Contour::Open;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.canvas.line(self.cur, p);
        self.cur = p;
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.at(x1, y1), self.at(x, y));
        self.canvas.quad(self.cur, c, p);
        self.cur = p;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        self.canvas.cubic(self.cur, c1, c2, p);
        self.cur = p;
    }
    fn close(&mut self) {
        self.close_contour();
    }
}
