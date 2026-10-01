//! macOS font smoothing, reproduced: an outline dilation keyed on the text
//! colour (note J).
//!
//! **What CoreText does, measured.** With font smoothing on, CoreText grows
//! a glyph's outline before it rasterises it, by a distance that depends on
//! the size and on the luminance of the colour the text is drawn in, and on
//! nothing else -- not the background, not the font. The level is
//! `clamp(floor(4Y + 0.5), 0, 4)` for the fill's linear Rec.709 luminance
//! `Y`, and the growth per edge is `d = min((0.0058 + 0.0025 n) * ppem, 0.30)`
//! device pixels: `d` on the left and right, `2d` on the top, nothing on the
//! bottom, square at the corners -- a box, not a disc -- and toward device
//! up whatever the text matrix. Fitted per edge to the u8 step over 9-48 px
//! in `scratchpad/dil-oracle`, and held to CoreText's own masks by
//! `tests/smooth.rs`.
//!
//! **How baud builds it** (note J, Jev 0.97): the outline is drawn
//! [`SUPERSAMPLE`] times larger in each direction into one canvas, its
//! coverage is taken in `f32`, the max over the box is taken in two
//! separable passes with the window's fractional ends interpolated, and the
//! samples are averaged back down to one per pixel. That is the arm the
//! oracle measured at 0.002-0.003 mean error against CoreText; this module
//! is a port of it, not a second spelling, at the factor the owner took
//! for its cost (below).
//!
//! **Nothing is allocated**, as in the rasteriser: the caller hands over a
//! canvas of [`Smoother::canvas`], a line of [`Dilation::line`], and the
//! mask. Only outlines are dilated; a bitmap or a colour
//! layer is drawn as it is.

use crate::outline::{Sink, Transform};
use crate::raster::{Canvas, Contour, Depth, Extent, Fit, Point, Px};
use core::f32::math::{ceil, floor};

/// How many samples a pixel is cut into along each axis while dilating.
///
/// Eight, the owner's call (Jev 0.86): the smallest factor that holds
/// CoreText. Against sixteen -- the oracle's -- its masks differ by 0.34
/// codes on average and 11 at most over JetBrains Mono's ASCII at 11-32 px
/// and every level, and it costs a quarter as much: 8.5 us a mask at 13 px,
/// 31 us at 28 px, 97 us for a Han glyph at 32 px, against 37, 147 and
/// 442. Four fails `tests/smooth.rs`'s CoreText gate (mean 0.012 at 10 px).
pub const SUPERSAMPLE: Px = Px::of(8);

/// The supersample factor as a count, for the arithmetic below.
const K: usize = 8;

/// How heavily macOS draws a glyph: one of five levels, from the luminance
/// of the colour the text is drawn in (note J). `L0` is black text, `L4`
/// white; the background never enters.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum SmoothLevel {
    L0,
    L1,
    L2,
    L3,
    L4,
}

impl SmoothLevel {
    pub const ALL: [SmoothLevel; 5] = [
        SmoothLevel::L0,
        SmoothLevel::L1,
        SmoothLevel::L2,
        SmoothLevel::L3,
        SmoothLevel::L4,
    ];

    /// The level for a text colour of linear Rec.709 luminance `y`:
    /// `clamp(floor(4y + 0.5), 0, 4)`. A non-finite luminance is black's.
    pub fn of_luminance(y: f32) -> SmoothLevel {
        let n = floor(4.0 * y + 0.5);
        if n >= 4.0 {
            SmoothLevel::L4
        } else if n >= 3.0 {
            SmoothLevel::L3
        } else if n >= 2.0 {
            SmoothLevel::L2
        } else if n >= 1.0 {
            SmoothLevel::L1
        } else {
            SmoothLevel::L0
        }
    }

    /// The level as the `n` of the growth formula.
    fn steps(self) -> f32 {
        match self {
            SmoothLevel::L0 => 0.0,
            SmoothLevel::L1 => 1.0,
            SmoothLevel::L2 => 2.0,
            SmoothLevel::L3 => 3.0,
            SmoothLevel::L4 => 4.0,
        }
    }
}

/// How far font smoothing grows an outline: `d` device pixels on the left
/// and the right, `2d` on the top, nothing on the bottom.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dilation(f32);

impl Dilation {
    /// The most any edge but the top grows, at any size and level. The top
    /// grows twice this, which [`Fit`]'s pixel of margin holds.
    pub const CAP: f32 = 0.30;

    /// No growth: the supersample's own mask, which must be the
    /// rasteriser's for a straight edge; a curve takes CoreGraphics' chords here.
    pub const NONE: Dilation = Dilation(0.0);

    /// CoreText's growth for `level` at `ppem` pixels per em:
    /// `min((0.0058 + 0.0025 n) * ppem, 0.30)`. `None` for a size that is
    /// not a positive finite number.
    pub fn smoothing(level: SmoothLevel, ppem: f32) -> Option<Dilation> {
        if !(ppem.is_finite() && ppem > 0.0) {
            return None;
        }
        let d = (0.0058 + 0.0025 * level.steps()) * ppem;
        Some(Dilation(if d < Dilation::CAP { d } else { Dilation::CAP }))
    }

    /// The growth on each side, in device pixels.
    pub fn per_edge(self) -> f32 {
        self.0
    }

    /// The line buffer the dilation needs for a mask of `extent`, as an
    /// extent one row high: one supersampled row with zeros either side.
    /// Allocate its [`Extent::cells`].
    pub fn line(extent: Extent) -> Option<Extent> {
        let k = SUPERSAMPLE.value();
        let pad = u32::try_from(2 * PAD).ok()?;
        Some(Extent::new(
            Px::of(extent.w.value().checked_mul(k)?.checked_add(pad)?),
            Px::of(1),
        ))
    }
}

/// An outline drawn the way CoreText's font smoothing draws it, into a
/// canvas [`SUPERSAMPLE`] times the mask's size: a [`Sink`] that flattens
/// curves by CoreGraphics' rule in device pixels and draws the lines at
/// that factor. [`Smoother::finish`] grows and averages it into the mask.
///
/// **The flattening is CoreGraphics', not baud's own** (note J, owner's
/// call, Jev 0.93). CoreText cuts a curve into chords at flatness 1 device
/// pixel whatever the context says, and those chords are what macOS draws:
/// exact curves read 0.8-5.3% heavier on round letters at 1x. The rule was
/// recovered by matching masks, not from a source -- halve while
/// `n(n-1)/8` times the max-norm of the second difference exceeds 0.2 for a
/// curve of degree `n`, a quadratic into `2^k` equal pieces, a cubic
/// recursively -- and filled by CoreGraphics its polyline is CoreGraphics'
/// own fill byte for byte in 29 of 30 fonts and sizes measured. The
/// CoreText fixtures in `tests/smooth/` hold it there.
pub struct Smoother<'a> {
    canvas: Canvas<'a>,
    out: Extent,
    transform: Transform,
    start: Point,
    cur: Point,
    contour: Contour,
}

/// The threshold on a chord's bulge: 0.2 times CoreText's flatness of one
/// device pixel.
const FLAT: f32 = 0.2;

/// The most pieces a quadratic is cut into: as deep as a cubic is halved,
/// which CoreGraphics' own subdivision stops well before.
const QUAD_PIECES: u32 = 1 << 16;

impl<'a> Smoother<'a> {
    /// The canvas [`Smoother::new`] draws `fit` into: its extent at
    /// [`SUPERSAMPLE`] times. Allocate its [`Extent::scratch`].
    pub fn canvas(fit: &Fit) -> Option<Extent> {
        let k = SUPERSAMPLE.value();
        Some(Extent::new(
            Px::of(fit.extent.w.value().checked_mul(k)?),
            Px::of(fit.extent.h.value().checked_mul(k)?),
        ))
    }

    /// A cleared canvas for `fit` over `scratch`, or `None` when it is
    /// short. The samples of pixel `(x, y)` are the block at `(Kx, Ky)`,
    /// so the mask registers with the fit's own `left` and `top`.
    pub fn new(fit: &Fit, scratch: &'a mut [f32]) -> Option<Smoother<'a>> {
        Some(Smoother {
            canvas: Canvas::new(Smoother::canvas(fit)?, scratch)?,
            out: fit.extent,
            transform: fit.transform,
            start: Point::new(0.0, 0.0),
            cur: Point::new(0.0, 0.0),
            contour: Contour::Closed,
        })
    }

    /// The outline, grown by `dilation` and averaged into the first cells
    /// of `mask`; `line` holds the cells of [`Dilation::line`] of the fit's extent.
    pub fn finish(mut self, dilation: Dilation, line: &mut [f32], mask: &mut [u8]) -> Option<()> {
        self.close_contour();
        let out = self.out;
        self.canvas.dilated(out, dilation, line, mask)
    }

    fn at(&self, x: f32, y: f32) -> Point {
        let (px, py) = self.transform.apply(x, y);
        Point::new(px, py)
    }

    /// A line in device pixels, drawn at [`SUPERSAMPLE`] times.
    fn line(&mut self, p0: Point, p1: Point) {
        let k = K as f32;
        self.canvas.line(
            Point::new(p0.x * k, p0.y * k),
            Point::new(p1.x * k, p1.y * k),
        );
    }

    fn close_contour(&mut self) {
        if self.contour == Contour::Open && self.cur != self.start {
            self.line(self.cur, self.start);
        }
        self.contour = Contour::Closed;
    }

    /// A cubic, halved while `3/4` of the max-norm of its second
    /// differences exceeds [`FLAT`].
    fn cubic(&mut self, p: [Point; 4], depth: Depth) {
        let d = second(p[0], p[1], p[2]).max(second(p[1], p[2], p[3]));
        if !depth.may_split() || 0.75 * d <= FLAT {
            self.line(p[0], p[3]);
            return;
        }
        let mid = |a: Point, b: Point| Point::new(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
        let (a, b, c) = (mid(p[0], p[1]), mid(p[1], p[2]), mid(p[2], p[3]));
        let (d0, e) = (mid(a, b), mid(b, c));
        let f = mid(d0, e);
        self.cubic([p[0], a, d0, f], depth.deeper());
        self.cubic([f, e, c, p[3]], depth.deeper());
    }
}

/// The max-norm of `p0 - 2 p1 + p2`.
fn second(p0: Point, p1: Point, p2: Point) -> f32 {
    let (x, y) = (p0.x - 2.0 * p1.x + p2.x, p0.y - 2.0 * p1.y + p2.y);
    x.abs().max(y.abs())
}

impl Sink for Smoother<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_contour();
        self.start = self.at(x, y);
        self.cur = self.start;
        self.contour = Contour::Open;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.line(self.cur, p);
        self.cur = p;
    }
    /// A quadratic, cut into `2^k` equal pieces: the fewest for which a
    /// quarter of the max-norm of each piece's second difference is within
    /// [`FLAT`]. Halving a quadratic quarters its second difference.
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, c, p2) = (self.cur, self.at(x1, y1), self.at(x, y));
        let mut d = second(p0, c, p2) / 4.0;
        let mut n = 1u32;
        while d > FLAT && n < QUAD_PIECES {
            d /= 4.0;
            n *= 2;
        }
        let nf = n as f32;
        let mut prev = p0;
        for i in 1..=n {
            let s = i as f32 / nf;
            let a = Point::new(p0.x + s * (c.x - p0.x), p0.y + s * (c.y - p0.y));
            let b = Point::new(c.x + s * (p2.x - c.x), c.y + s * (p2.y - c.y));
            let q = Point::new(a.x + s * (b.x - a.x), a.y + s * (b.y - a.y));
            self.line(prev, q);
            prev = q;
        }
        self.cur = p2;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p = [self.cur, self.at(x1, y1), self.at(x2, y2), self.at(x, y)];
        self.cubic(p, Depth::ZERO);
        self.cur = p[3];
    }
    fn close(&mut self) {
        self.close_contour();
    }
}

impl Canvas<'_> {
    /// The mask of `out` -- `self` drawn at [`Fit::supersampled`] of it --
    /// grown by `dilation`: the coverage, the max over the box, and the
    /// average back to one sample a pixel, into the first `out.cells()`
    /// bytes of `mask`. `line` holds the cells of [`Dilation::line`]. `None` when
    /// `self` is not `out` supersampled or a buffer is short.
    fn dilated(
        self,
        out: Extent,
        dilation: Dilation,
        line: &mut [f32],
        mask: &mut [u8],
    ) -> Option<()> {
        debug_assert!(
            dilation.0 <= Dilation::CAP,
            "a growth past {} px leaves Fit's margin",
            Dilation::CAP
        );
        let (extent, a) = self.into_parts();
        let (w, h) = (
            usize::try_from(extent.w.value()).ok()?,
            usize::try_from(extent.h.value()).ok()?,
        );
        let (ow, oh) = (
            usize::try_from(out.w.value()).ok()?,
            usize::try_from(out.h.value()).ok()?,
        );
        if w != ow.checked_mul(K)? || h != oh.checked_mul(K)? {
            return None;
        }
        let a = a.get_mut(..w.checked_mul(h)?)?;
        let line = line.get_mut(..w.checked_add(2 * PAD)?)?;
        let mask = mask.get_mut(..out.cells()?)?;
        // Coverage in place: the prefix sum, |area| clamped to one. [`Fit`]
        // keeps the ink a pixel inside the canvas, so no line deposits past
        // a row's end and every row sums from zero on its own: four rows at
        // a time, so the sums do not wait on one another.
        let mut rows = a.chunks_exact_mut(w);
        loop {
            match (rows.next(), rows.next(), rows.next(), rows.next()) {
                (Some(r0), Some(r1), Some(r2), Some(r3)) => {
                    let mut acc = [0.0f32; 4];
                    for (((c0, c1), c2), c3) in r0
                        .iter_mut()
                        .zip(r1.iter_mut())
                        .zip(r2.iter_mut())
                        .zip(r3.iter_mut())
                    {
                        for (s, c) in acc.iter_mut().zip([c0, c1, c2, c3]) {
                            *s += *c;
                            let y = s.abs();
                            *c = if y < 1.0 { y } else { 1.0 };
                        }
                    }
                }
                (r0, r1, r2, _) => {
                    for r in [r0, r1, r2].into_iter().flatten() {
                        let mut acc = 0.0f32;
                        for c in r.iter_mut() {
                            acc += *c;
                            let y = acc.abs();
                            *c = if y < 1.0 { y } else { 1.0 };
                        }
                    }
                    break;
                }
            }
        }
        // The box: out(x) = max over [x - d, x + d], out(y) = max over
        // [y, y + 2d] -- the rows below, so the top edge grows. Each pass
        // is a few whole-row maxima over shifted copies, which vectorise.
        let d = dilation.0 * K as f32;
        let across = Window::new(-d, d);
        let down = Window::new(0.0, 2.0 * d);
        line.fill(0.0);
        for y in 0..h {
            let row = a.get_mut(y * w..(y + 1) * w)?;
            if row.iter().all(|&v| v == 0.0) {
                continue;
            }
            line.get_mut(PAD..PAD + w)?.copy_from_slice(row);
            across.apply(line, row)?;
        }
        // In place, top to bottom: row y reads rows y.. only, and none of
        // them has been written yet.
        for y in 0..h {
            let out = line.get_mut(..w)?;
            out.fill(0.0);
            for t in down.terms() {
                let r0 = y + t.k.0;
                if r0 >= h {
                    continue;
                }
                let lo = a.get(r0 * w..(r0 + 1) * w)?;
                if t.f == 0.0 || r0 + 1 >= h {
                    let g = 1.0 - t.f;
                    for (o, &v) in out.iter_mut().zip(lo) {
                        *o = o.max(v * g);
                    }
                } else {
                    let hi = a.get((r0 + 1) * w..(r0 + 2) * w)?;
                    for ((o, &u), &v) in out.iter_mut().zip(lo).zip(hi) {
                        *o = o.max(u + (v - u) * t.f);
                    }
                }
            }
            a.get_mut(y * w..(y + 1) * w)?
                .copy_from_slice(line.get(..w)?);
        }
        // Back to one sample a pixel: the block's mean, quantised as the
        // rasteriser quantises.
        let scale = 1.0 / (K * K) as f32;
        let sums = line.get_mut(..w)?;
        for (oy, mrow) in mask.chunks_exact_mut(ow).enumerate() {
            sums.fill(0.0);
            for sy in 0..K {
                let start = (oy * K + sy) * w;
                for (s, &v) in sums.iter_mut().zip(a.get(start..start + w)?) {
                    *s += v;
                }
            }
            for (m, block) in mrow.iter_mut().zip(sums.chunks_exact(K)) {
                let y = block.iter().sum::<f32>() * scale;
                let y = if y < 1.0 { y } else { 1.0 };
                *m = u8::try_from((255.0 * y) as i32).unwrap_or(u8::MAX);
            }
        }
        Some(())
    }
}

/// Zero samples on each side of a row in the line buffer: more than the
/// horizontal window reaches, `CAP * K` and one for the interpolation.
const PAD: usize = 8;

/// One point a window reads: `k` whole samples from the output, and the
/// fraction `f` of the way to the next. The coverage between samples is
/// linear, so its max over the window is the max over these points.
#[derive(Clone, Copy)]
struct Term {
    k: Samples,
    f: f32,
}

/// A whole number of samples along a row or down a column.
#[derive(Clone, Copy)]
struct Samples(usize);

/// A window `[lo, hi]` in samples, as the points it reads: its two ends
/// and every whole offset strictly between them. `k` is counted from
/// `lo`'s floor, which [`Window::apply`] shifts back.
struct Window {
    terms: [Option<Term>; WINDOW_MAX],
    base: isize,
}

/// The most points a window reads: the vertical one spans `2 * CAP * K` =
/// 4.8 samples at eight, so five whole offsets and two ends; twelve holds
/// a factor of sixteen too.
const WINDOW_MAX: usize = 12;

impl Window {
    fn new(lo: f32, hi: f32) -> Window {
        let base = floor(lo);
        let mut terms = [None; WINDOW_MAX];
        let mut slots = terms.iter_mut();
        let mut push = |t: f32| {
            if let Some(slot) = slots.next() {
                let k = floor(t - base);
                *slot = Some(Term {
                    k: Samples(usize::try_from(k as i32).unwrap_or(0)),
                    f: t - base - k,
                });
            }
        };
        if hi > lo {
            push(lo);
            let mut k = ceil(lo);
            while k < hi {
                if k > lo {
                    push(k);
                }
                k += 1.0;
            }
            push(hi);
        } else {
            push(lo);
        }
        Window {
            terms,
            base: base as isize,
        }
    }

    /// The points, with `k` counted from `lo`'s floor.
    fn terms(&self) -> impl Iterator<Item = Term> + '_ {
        self.terms.iter().flatten().copied()
    }

    /// `out[i] = max over the window of src(PAD + i + t)`: `src` is a row
    /// with [`PAD`] zeros either side.
    fn apply(&self, src: &[f32], out: &mut [f32]) -> Option<()> {
        let n = out.len();
        let from = usize::try_from(isize::try_from(PAD).ok()? + self.base).ok()?;
        out.fill(0.0);
        for t in self.terms() {
            let lo = src.get(from + t.k.0..from + t.k.0 + n)?;
            let hi = src.get(from + t.k.0 + 1..from + t.k.0 + 1 + n)?;
            for ((o, &u), &v) in out.iter_mut().zip(lo).zip(hi) {
                *o = o.max(u + (v - u) * t.f);
            }
        }
        Some(())
    }
}
