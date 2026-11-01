//! **An outline's stroke**, a line of a width centred on every contour, as
//! CoreGraphics strokes the text iTerm2's badge draws "with stroke and
//! fill" (`[[source: apple-ct-stroke-width]]`: the width "interpreted as a
//! percentage of font point size"). Quartz's own text says how a path is
//! stroked (`[[source: apple-quartz2d-paths]]`): "The line straddles the
//! path, with half of the total width on either side"; "The default style
//! is miter join", where "Quartz extends the outer edges of the strokes for
//! the two segments until they meet at an angle", and "a bevel join is
//! used instead" when "the length of the miter divided by the line width
//! is greater than the miter limit"; and "A closed subpath treats the
//! starting point as a junction between connected line segments", so a
//! glyph's contours, all closed, have joins and no caps. The limit is 10,
//! as `CGContext` strokes with none set, measured (a wedge mitred at 9.57
//! and bevelled at 10.43).
//!
//! **The stroke is a union of pieces, rasterised as a fill is.** Each
//! contour is flattened to lines; each line gives the rectangle the width
//! sweeps about it, and each corner the wedge its join adds on the turn's
//! outer side, a curve's ends joined by its tangents there. Every piece is wound the same way onto one [`Edges`], and a
//! [`Canvas`]'s coverage clamps the accumulated area at one, so the mask
//! is the pieces' union. Nothing is allocated and nothing is buffered: a
//! contour's first direction and its last are all its closing join needs.
//! The code is baud's.

use crate::outline::{Flex, FlexDrawn, Sink, Transform};
use crate::raster::{Canvas, Point};
use core::f32::math::{ceil, sqrt};

/// "If the result is greater than the miter limit, the style is converted
/// to a bevel": CoreGraphics' limit, the miter's length over the line's
/// width, while none is set.
pub const MITER_LIMIT: f32 = 10.0;

/// How far a flattened curve may stray from the curve, pixels.
const FLAT: f32 = 0.05;

/// The most lines a curve is flattened to.
const LINES_MAX: f32 = 64.0;

/// Where a stroke's edges go: a [`Canvas`] to fill, or a [`Reach`] to
/// measure.
pub trait Edges {
    fn edge(&mut self, from: Point, to: Point);
}

impl Edges for Canvas<'_> {
    fn edge(&mut self, from: Point, to: Point) {
        self.line(from, to);
    }
}

/// **How far a stroke reaches**: the box every edge it would draw lies
/// in, `None` until one is.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Reach {
    pub reached: Option<[f32; 4]>,
}

impl Edges for Reach {
    fn edge(&mut self, from: Point, to: Point) {
        for p in [from, to] {
            self.reached = Some(match self.reached {
                None => [p.x, p.y, p.x, p.y],
                Some([x0, y0, x1, y1]) => [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)],
            });
        }
    }
}

/// The contour being stroked: where it started, where it is, and the
/// direction of its first line and of its last, each a unit vector,
/// `None` before the first.
#[derive(Clone, Copy)]
struct Run {
    start: Point,
    cur: Point,
    first: Option<Point>,
    last: Option<Point>,
}

/// **A [`Sink`] that strokes an outline** onto `E` through a
/// [`Transform`], the line `width` pixels wide.
pub struct Stroker<E> {
    edges: E,
    transform: Transform,
    half: f32,
    run: Option<Run>,
}

fn sub(a: Point, b: Point) -> Point {
    Point::new(a.x - b.x, a.y - b.y)
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn times(a: Point, s: f32) -> Point {
    Point::new(a.x * s, a.y * s)
}

fn dot(a: Point, b: Point) -> f32 {
    a.x * b.x + a.y * b.y
}

fn cross(a: Point, b: Point) -> f32 {
    a.x * b.y - a.y * b.x
}

fn length(a: Point) -> f32 {
    sqrt(dot(a, a))
}

/// `d` turned a quarter, to one side of a path running along it.
fn normal(d: Point) -> Point {
    Point::new(-d.y, d.x)
}

impl<E: Edges> Stroker<E> {
    pub fn new(edges: E, transform: Transform, width: f32) -> Stroker<E> {
        Stroker {
            edges,
            transform,
            half: width / 2.0,
            run: None,
        }
    }

    fn at(&self, x: f32, y: f32) -> Point {
        let (px, py) = self.transform.apply(x, y);
        Point::new(px, py)
    }

    /// A polygon onto the edges, wound as every piece is: in the order
    /// given when its signed area is positive, else reversed.
    fn piece(&mut self, p: &[Point]) {
        let n = p.len();
        let area: f32 = (0..n).map(|i| cross(p[i], p[(i + 1) % n])).sum();
        for i in 0..n {
            let (a, b) = (p[i], p[(i + 1) % n]);
            match area >= 0.0 {
                true => self.edges.edge(a, b),
                false => self.edges.edge(b, a),
            }
        }
    }

    /// The join at `v` from a line running along `a` to one along `b`,
    /// each a unit vector, on the outer side of the turn: a miter to where
    /// the two outer edges meet, or past [`MITER_LIMIT`] a bevel across
    /// the corner. A line running straight on needs none.
    fn join(&mut self, v: Point, a: Point, b: Point) {
        let turn = cross(a, b);
        if turn.abs() <= f32::EPSILON && dot(a, b) > 0.0 {
            return;
        }
        let side = match turn > 0.0 {
            true => -1.0,
            false => 1.0,
        };
        let (na, nb) = (times(normal(a), side), times(normal(b), side));
        let (p, q) = (add(v, times(na, self.half)), add(v, times(nb, self.half)));
        // The miter over the width is 1 / sin(phi / 2), phi the angle the
        // two lines make, and sin(phi / 2) is the root of (1 + a.b) / 2.
        let cos = 1.0 + dot(a, b);
        if cos * MITER_LIMIT * MITER_LIMIT >= 2.0 {
            let m = add(v, times(add(na, nb), self.half / cos));
            self.piece(&[v, p, m, q]);
        } else {
            self.piece(&[v, p, q]);
        }
    }

    /// One straight line of the contour to `to`, the join before it with
    /// it.
    fn segment(&mut self, to: Point) {
        let Some(mut run) = self.run else {
            return;
        };
        let d = sub(to, run.cur);
        let len = length(d);
        if len <= f32::EPSILON {
            return;
        }
        let u = times(d, 1.0 / len);
        match run.last {
            Some(last) => self.join(run.cur, last, u),
            None => run.first = Some(u),
        }
        let n = times(normal(u), self.half);
        let from = run.cur;
        self.piece(&[add(from, n), add(to, n), sub(to, n), sub(from, n)]);
        run.last = Some(u);
        run.cur = to;
        self.run = Some(run);
    }

    /// The contour turned at its current point to run along `towards`,
    /// joined there as a corner is: a curve's tangent where it leaves and
    /// where it arrives, so a curve meets its neighbours as its own
    /// direction does, not its first and last line's.
    fn turn(&mut self, towards: Point) {
        let Some(mut run) = self.run else {
            return;
        };
        let len = length(towards);
        if len <= f32::EPSILON {
            return;
        }
        let u = times(towards, 1.0 / len);
        match run.last {
            Some(last) => self.join(run.cur, last, u),
            None => run.first = Some(u),
        }
        run.last = Some(u);
        self.run = Some(run);
    }

    /// The first of `ds` that is not a point, the direction a curve
    /// leaves or arrives in when its control points lie on its ends.
    fn direction(ds: [Point; 3]) -> Point {
        ds.into_iter()
            .find(|&d| length(d) > f32::EPSILON)
            .unwrap_or(Point::new(0.0, 0.0))
    }

    /// The contour closed: the line back to its start, and the join there.
    fn close_contour(&mut self) {
        let Some(run) = self.run else {
            return;
        };
        self.segment(run.start);
        if let Some(Run {
            start,
            first: Some(first),
            last: Some(last),
            ..
        }) = self.run
        {
            self.join(start, last, first);
        }
        self.run = None;
    }

    /// A curve whose control polygon is `polygon` pixels long, as lines
    /// to the points `at` gives along it, from just past its start to its
    /// end: enough that none strays past [`FLAT`] from the curve, at most
    /// [`LINES_MAX`].
    fn curve(&mut self, polygon: f32, at: impl Fn(f32) -> Point) {
        let n = ceil(sqrt(polygon / FLAT) / 2.0).clamp(1.0, LINES_MAX);
        let mut i = 1.0;
        while i <= n {
            self.segment(at(i / n));
            i += 1.0;
        }
    }

    /// Closes any open contour and hands the edges back.
    pub fn finish(mut self) -> E {
        self.close_contour();
        self.edges
    }
}

impl<E: Edges> Sink for Stroker<E> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close_contour();
        let p = self.at(x, y);
        self.run = Some(Run {
            start: p,
            cur: p,
            first: None,
            last: None,
        });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.segment(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(run) = self.run else { return };
        let (p0, c, p1) = (run.cur, self.at(x1, y1), self.at(x, y));
        let polygon = length(sub(c, p0)) + length(sub(p1, c));
        let chord = sub(p1, p0);
        self.turn(Stroker::<E>::direction([sub(c, p0), chord, chord]));
        self.curve(polygon, |t| {
            let s = 1.0 - t;
            add(
                add(times(p0, s * s), times(c, 2.0 * s * t)),
                times(p1, t * t),
            )
        });
        self.turn(Stroker::<E>::direction([sub(p1, c), chord, chord]));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let Some(run) = self.run else { return };
        let (p0, c1, c2, p1) = (run.cur, self.at(x1, y1), self.at(x2, y2), self.at(x, y));
        let polygon = length(sub(c1, p0)) + length(sub(c2, c1)) + length(sub(p1, c2));
        let chord = sub(p1, p0);
        self.turn(Stroker::<E>::direction([sub(c1, p0), sub(c2, p0), chord]));
        self.curve(polygon, |t| {
            let s = 1.0 - t;
            add(
                add(times(p0, s * s * s), times(c1, 3.0 * s * s * t)),
                add(times(c2, 3.0 * s * t * t), times(p1, t * t * t)),
            )
        });
        self.turn(Stroker::<E>::direction([sub(p1, c2), sub(p1, c1), chord]));
    }
    fn close(&mut self) {
        self.close_contour();
    }
    fn flex(&mut self, f: &Flex) {
        let [x1, y1, x2, y2, x, y] = f.first;
        let [x3, y3, x4, y4, x5, y5] = f.second;
        match f.rendered(&self.transform) {
            FlexDrawn::Straight => self.line_to(x5, y5),
            FlexDrawn::Curved => {
                self.curve_to(x1, y1, x2, y2, x, y);
                self.curve_to(x3, y3, x4, y4, x5, y5);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{Extent, Px};
    extern crate alloc;
    use alloc::vec;
    use alloc::vec::Vec;

    const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// The closed polygon `points`, stroked `width` wide on a `side`
    /// square canvas: its coverage, row by row.
    fn stroked(points: &[(f32, f32)], width: f32, side: u32) -> Vec<u8> {
        let extent = Extent::new(Px::of(side), Px::of(side));
        let mut scratch = vec![0f32; extent.scratch().unwrap()];
        let canvas = Canvas::new(extent, &mut scratch).unwrap();
        let mut s = Stroker::new(canvas, IDENTITY, width);
        s.move_to(points[0].0, points[0].1);
        for &(x, y) in &points[1..] {
            s.line_to(x, y);
        }
        s.close();
        let mut out = vec![0u8; extent.cells().unwrap()];
        s.finish().coverage(&mut out).unwrap();
        out
    }

    /// How far above `apex_y` the stroke of a closed wedge with the apex
    /// angle `phi` degrees and the line `width` wide reaches.
    fn reach_above_apex(phi: f32, width: f32) -> f32 {
        let half = phi.to_radians() / 2.0;
        let (ax, ay, len) = (200.0f32, 300.0f32, 150.0f32);
        let mut s = Stroker::new(Reach::default(), IDENTITY, width);
        s.move_to(ax, ay);
        s.line_to(ax - len * half.sin(), ay + len * half.cos());
        s.line_to(ax + len * half.sin(), ay + len * half.cos());
        s.close();
        let [_, y0, _, _] = s.finish().reached.unwrap();
        ay - y0
    }

    /// **The line straddles the path, half its width either side, and a
    /// right-angled corner is mitred**: a 10 by 10 square stroked 2 wide is
    /// the ring from 4 to 16 less the square from 6 to 14, 80 pixels whole,
    /// its outer corners -- the contour's start among them, joined as a
    /// junction -- covered, its inside empty; the pieces overlapping at
    /// each corner count once.
    #[test]
    fn a_stroke_straddles_the_outline_and_a_square_corner_is_mitred() {
        let m = stroked(
            &[(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0)],
            2.0,
            20,
        );
        let at = |x: usize, y: usize| m[y * 20 + x];
        let area: u32 = m.iter().map(|&c| u32::from(c)).sum();
        assert_eq!(area, 80 * 255);
        for (x, y) in [(4, 4), (15, 4), (15, 15), (4, 15)] {
            assert_eq!(at(x, y), 255, "the corner at ({x}, {y})");
        }
        for (x, y) in [(10, 4), (10, 5), (4, 10), (5, 10)] {
            assert_eq!(at(x, y), 255, "the band at ({x}, {y})");
        }
        for (x, y) in [(10, 10), (6, 6), (3, 10), (10, 16)] {
            assert_eq!(at(x, y), 0, "outside the band at ({x}, {y})");
        }
    }

    /// **A corner past the miter limit is bevelled**: "a bevel join is used
    /// instead" when "the length of the miter divided by the line width is
    /// greater than the miter limit", 10 -- the wedge CoreGraphics mitred
    /// at 12 degrees (a ratio of 9.57) reaches 19 pixels past its apex 4
    /// wide, and the one it bevelled at 11 degrees (10.43) under 2, as
    /// measured there.
    #[test]
    fn a_corner_past_the_miter_limit_is_bevelled() {
        let mitred = reach_above_apex(12.0, 4.0);
        assert!(
            (mitred - 2.0 / 6.0f32.to_radians().sin()).abs() < 0.01,
            "{mitred}"
        );
        assert!(mitred > 18.0, "{mitred}");
        let bevelled = reach_above_apex(11.0, 4.0);
        assert!(bevelled < 2.0, "{bevelled}");
        assert!(
            reach_above_apex(16.0, 4.0) > 14.0,
            "mitred well inside the limit"
        );
    }

    /// **A contour that turns both ways is joined on each corner's outer
    /// side, every piece adding**: an L, five corners one way and one the
    /// other, stroked 2 wide is the L 1 out less the L 1 in, 80 pixels
    /// whole, its inner corner's miter at (10, 10) to (11, 11) covered.
    #[test]
    fn a_contour_turning_both_ways_is_joined_outside_each_corner() {
        let l = [
            (5.0, 5.0),
            (15.0, 5.0),
            (15.0, 10.0),
            (10.0, 10.0),
            (10.0, 15.0),
            (5.0, 15.0),
        ];
        let m = stroked(&l, 2.0, 20);
        let area: u32 = m.iter().map(|&c| u32::from(c)).sum();
        assert_eq!(area, 80 * 255);
        assert_eq!(m[10 * 20 + 10], 255, "the inner corner's miter");
        assert_eq!(m[12 * 20 + 12], 0, "past it");
    }

    /// **Where pieces overlap they add, never cancel**: one square laid
    /// twice, once each way round -- as a join on a turn one way and a
    /// line's rectangle can come -- is covered, where pieces wound against
    /// each other would leave it empty.
    #[test]
    fn overlapping_pieces_add() {
        let extent = Extent::new(Px::of(10), Px::of(10));
        let mut scratch = vec![0f32; extent.scratch().unwrap()];
        let canvas = Canvas::new(extent, &mut scratch).unwrap();
        let mut s = Stroker::new(canvas, IDENTITY, 2.0);
        let p = |x, y| Point::new(x, y);
        s.piece(&[p(2.0, 2.0), p(8.0, 2.0), p(8.0, 8.0), p(2.0, 8.0)]);
        s.piece(&[p(2.0, 2.0), p(2.0, 8.0), p(8.0, 8.0), p(8.0, 2.0)]);
        let mut m = vec![0u8; extent.cells().unwrap()];
        s.finish().coverage(&mut m).unwrap();
        assert_eq!(m[5 * 10 + 5], 255);
        assert_eq!(m.iter().map(|&c| u32::from(c)).sum::<u32>(), 36 * 255);
    }

    /// **Every piece is wound one way, whichever way the contour runs**:
    /// the square drawn the other way round strokes the same mask, as a
    /// glyph's holes, wound against its outer contours, are stroked like
    /// them.
    #[test]
    fn a_contour_strokes_alike_either_way_round() {
        let one = stroked(
            &[(5.0, 5.0), (15.0, 5.0), (15.0, 15.0), (5.0, 15.0)],
            2.0,
            20,
        );
        let other = stroked(
            &[(5.0, 5.0), (5.0, 15.0), (15.0, 15.0), (15.0, 5.0)],
            2.0,
            20,
        );
        assert_eq!(one, other);
    }
}
