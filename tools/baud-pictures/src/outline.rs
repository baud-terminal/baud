//! **Text to SVG path data, as fontTools 4.63.0 draws it**: a glyf glyph
//! through `TransformPen` into `SVGPathPen`, and through `BoundsPen` for
//! its ink, in `f64` and in fontTools' order of operations, so every
//! coordinate the SVGs carry is the one the generator wrote before.
//!
//! What is ported, each from its fontTools source:
//!
//! * `Glyph.draw` (`ttLib/tables/_g_l_y_f.py`): each contour rotated to
//!   begin after its first on-curve point, `moveTo` the last point, a
//!   `lineTo` per on-curve point but the closing one, a `qCurveTo` per run
//!   of off-curve points; a contour of off-curve points alone is one
//!   `qCurveTo` ending in `None`. A composite draws each component through
//!   `addComponent`, its offset composed onto the pen's transform by
//!   `Transform.transform`, so a component's points are `k*x + 0*y +
//!   (k*dx + 0*dy + cx)` and not `k*(x + dx) + cx`.
//! * `_TTGlyphGlyf.draw`: the top-level glyph moved by `lsb - xMin`.
//! * `BasePen.qCurveTo`: the implied on-curve point `0.5 * (a + b)`, taken
//!   after the transform, since `TransformPen` transforms the points
//!   first.
//! * `SVGPathPen`: `M`, then `H` or `V` for a line along an axis, ` x y`
//!   for a line straight after a move, `L` otherwise, `Q` and `Z`; a line
//!   to the current point is dropped.
//! * `BoundsPen`: a quadratic's extremum is added only when its control
//!   point lies outside the bounds so far (`calcQuadraticBounds`, roots
//!   `-b / 2a` kept for `0 <= t < 1`).
//!
//! The faces' glyphs here are quadratic, and their components offsets
//! alone; a cubic contour or a scaled or point-anchored component is
//! refused rather than drawn some other way.

use read_fonts::tables::glyf::{Anchor, Glyph};
use read_fonts::types::GlyphId;
use read_fonts::{FontRef, TableProvider};

/// A point, `f64` as Python's floats.
pub type Pt = (f64, f64);

/// fontTools' `Transform`: `(xx, xy, yx, yy, dx, dy)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform(pub [f64; 6]);

impl Transform {
    const IDENTITY: Transform = Transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `transformPoint`: `(xx*x + yx*y + dx, xy*x + yy*y + dy)`.
    #[must_use]
    pub fn point(&self, (x, y): Pt) -> Pt {
        let [xx, xy, yx, yy, dx, dy] = self.0;
        (xx * x + yx * y + dx, xy * x + yy * y + dy)
    }

    /// `self.transform(other)`.
    #[must_use]
    pub fn then(&self, other: &Transform) -> Transform {
        let [xx1, xy1, yx1, yy1, dx1, dy1] = other.0;
        let [xx2, xy2, yx2, yy2, dx2, dy2] = self.0;
        Transform([
            xx1 * xx2 + xy1 * yx2,
            xx1 * xy2 + xy1 * yy2,
            yx1 * xx2 + yy1 * yx2,
            yx1 * xy2 + yy1 * yy2,
            xx2 * dx1 + yx2 * dy1 + dx2,
            xy2 * dx1 + yy2 * dy1 + dy2,
        ])
    }
}

/// The pen calls a glyph makes.
pub trait Pen {
    fn move_to(&mut self, p: Pt);
    fn line_to(&mut self, p: Pt);
    /// A run of off-curve points and its end: `None` for a contour of
    /// off-curve points alone.
    fn q_curve_to(&mut self, pts: &[Pt], end: Option<Pt>);
    fn close_path(&mut self);
}

/// `BasePen.qCurveTo`'s split into single quadratic segments, calling
/// `one(control, end)` with the current point updated by the caller.
fn decompose(pts: &[Pt], end: Option<Pt>, mut start: impl FnMut(Pt), mut one: impl FnMut(Pt, Pt)) {
    let mut pts = pts.to_vec();
    let end = match end {
        Some(e) => e,
        None => {
            let (x, y) = pts[pts.len() - 1];
            let (nx, ny) = pts[0];
            let implied = (0.5 * (x + nx), 0.5 * (y + ny));
            start(implied);
            implied
        }
    };
    pts.push(end);
    let n = pts.len() - 1;
    for i in 0..n - 1 {
        let (x, y) = pts[i];
        let (nx, ny) = pts[i + 1];
        one(pts[i], (0.5 * (x + nx), 0.5 * (y + ny)));
    }
    one(pts[n - 1], pts[n]);
}

/// `fontTools.pens.svgPathPen.SVGPathPen` with `ntos` given.
pub struct SvgPath<'n> {
    commands: Vec<String>,
    last: Option<char>,
    at: Option<Pt>,
    ntos: &'n dyn Fn(f64) -> String,
}

impl<'n> SvgPath<'n> {
    #[must_use]
    pub fn new(ntos: &'n dyn Fn(f64) -> String) -> SvgPath<'n> {
        SvgPath {
            commands: Vec::new(),
            last: None,
            at: None,
            ntos,
        }
    }

    fn pt(&self, (x, y): Pt) -> String {
        format!("{} {}", (self.ntos)(x), (self.ntos)(y))
    }

    fn moved(&mut self, p: Pt) {
        if self.last == Some('M') {
            self.commands.pop();
        }
        let t = format!("M{}", self.pt(p));
        self.commands.push(t);
        self.last = Some('M');
        self.at = Some(p);
    }

    fn q_one(&mut self, c: Pt, e: Pt) {
        let t = format!("Q{} {}", self.pt(c), self.pt(e));
        self.commands.push(t);
        self.last = Some('Q');
        self.at = Some(e);
    }

    /// `getCommands()`.
    #[must_use]
    pub fn commands(&self) -> String {
        self.commands.concat()
    }
}

impl Pen for SvgPath<'_> {
    fn move_to(&mut self, p: Pt) {
        self.moved(p);
    }

    fn line_to(&mut self, (x, y): Pt) {
        let (lx, ly) = match self.at {
            Some((lx, ly)) => (Some(lx), Some(ly)),
            None => (None, None),
        };
        let t = if Some(x) == lx && Some(y) == ly {
            return;
        } else if Some(x) == lx {
            self.last = Some('V');
            format!("V{}", (self.ntos)(y))
        } else if Some(y) == ly {
            self.last = Some('H');
            format!("H{}", (self.ntos)(x))
        } else if self.last == Some('M') {
            format!(" {}", self.pt((x, y)))
        } else {
            self.last = Some('L');
            format!("L{}", self.pt((x, y)))
        };
        self.commands.push(t);
        self.at = Some((x, y));
    }

    fn q_curve_to(&mut self, pts: &[Pt], end: Option<Pt>) {
        let me = core::cell::RefCell::new(self);
        decompose(
            pts,
            end,
            |p| me.borrow_mut().moved(p),
            |c, e| me.borrow_mut().q_one(c, e),
        );
    }

    fn close_path(&mut self) {
        self.commands.push("Z".to_string());
        self.last = Some('Z');
        self.at = None;
    }
}

/// `fontTools.pens.boundsPen.BoundsPen`.
#[derive(Default)]
pub struct Bounds {
    pub bounds: Option<[f64; 4]>,
    start: Option<Pt>,
    at: Option<Pt>,
}

/// Python's `min(a, b)`: `a` unless `b` is less.
fn pmin(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Python's `max(a, b)`: `a` unless `b` is greater.
fn pmax(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

fn update(b: Option<[f64; 4]>, (x, y): Pt) -> [f64; 4] {
    match b {
        None => [x, y, x, y],
        Some([x0, y0, x1, y1]) => [pmin(x0, x), pmin(y0, y), pmax(x1, x), pmax(y1, y)],
    }
}

fn inside((x, y): Pt, [x0, y0, x1, y1]: [f64; 4]) -> bool {
    (x0 <= x && x <= x1) && (y0 <= y && y <= y1)
}

/// `calcQuadraticBounds(pt1, pt2, pt3)`.
fn quadratic_bounds(p1: Pt, p2: Pt, p3: Pt) -> [f64; 4] {
    let ((x2, y2), (x3, y3), (cx, cy)) = (p2, p3, p1);
    let (bx, by) = ((x2 - cx) * 2.0, (y2 - cy) * 2.0);
    let (ax, ay) = (x3 - cx - bx, y3 - cy - by);
    let (ax2, ay2) = (ax * 2.0, ay * 2.0);
    let mut roots = Vec::new();
    if ax2 != 0.0 {
        roots.push(-bx / ax2);
    }
    if ay2 != 0.0 {
        roots.push(-by / ay2);
    }
    let mut pts: Vec<Pt> = roots
        .into_iter()
        .filter(|t| (0.0..1.0).contains(t))
        .map(|t| (ax * t * t + bx * t + cx, ay * t * t + by * t + cy))
        .collect();
    pts.extend([p1, p3]);
    // calcBounds: min and max of each axis over the list, in order.
    let mut b = [pts[0].0, pts[0].1, pts[0].0, pts[0].1];
    for &(x, y) in &pts[1..] {
        b = [pmin(b[0], x), pmin(b[1], y), pmax(b[2], x), pmax(b[3], y)];
    }
    b
}

impl Bounds {
    fn add_move(&mut self) {
        if let Some(p) = self.start.take() {
            self.bounds = Some(update(self.bounds, p));
        }
    }

    fn q_one(&mut self, c: Pt, e: Pt) {
        self.add_move();
        let mut b = update(self.bounds, e);
        if !inside(c, b) {
            let q = quadratic_bounds(self.at.expect("a current point"), c, e);
            b = [
                pmin(b[0], q[0]),
                pmin(b[1], q[1]),
                pmax(b[2], q[2]),
                pmax(b[3], q[3]),
            ];
        }
        self.bounds = Some(b);
        self.at = Some(e);
    }
}

impl Pen for Bounds {
    fn move_to(&mut self, p: Pt) {
        self.start = Some(p);
        self.add_move();
        self.at = Some(p);
    }

    fn line_to(&mut self, p: Pt) {
        self.add_move();
        self.bounds = Some(update(self.bounds, p));
        self.at = Some(p);
    }

    fn q_curve_to(&mut self, pts: &[Pt], end: Option<Pt>) {
        let me = core::cell::RefCell::new(self);
        decompose(
            pts,
            end,
            |p| {
                // BasePen.qCurveTo sets the current point, then _moveTo.
                let mut m = me.borrow_mut();
                m.at = Some(p);
                m.start = Some(p);
                m.add_move();
            },
            |c, e| me.borrow_mut().q_one(c, e),
        );
    }

    fn close_path(&mut self) {
        self.at = None;
    }
}

/// A pen behind a transform, `TransformPen(pen, t)`, or the pen itself.
struct Through<'p> {
    pen: &'p mut dyn Pen,
    t: Option<Transform>,
}

impl Through<'_> {
    fn point(&self, p: Pt) -> Pt {
        self.t.map_or(p, |t| t.point(p))
    }
}

impl Pen for Through<'_> {
    fn move_to(&mut self, p: Pt) {
        let p = self.point(p);
        self.pen.move_to(p);
    }
    fn line_to(&mut self, p: Pt) {
        let p = self.point(p);
        self.pen.line_to(p);
    }
    fn q_curve_to(&mut self, pts: &[Pt], end: Option<Pt>) {
        let pts: Vec<Pt> = pts.iter().map(|&p| self.point(p)).collect();
        let end = end.map(|e| self.point(e));
        self.pen.q_curve_to(&pts, end);
    }
    fn close_path(&mut self) {
        self.pen.close_path();
    }
}

/// One face: its glyphs, advances and side bearings.
pub struct Face<'a> {
    font: FontRef<'a>,
    pub upem: u16,
}

impl<'a> Face<'a> {
    /// # Errors
    /// When the font cannot be read.
    pub fn new(data: &'a [u8]) -> Result<Face<'a>, String> {
        let font = FontRef::new(data).map_err(|e| e.to_string())?;
        let upem = font.head().map_err(|e| e.to_string())?.units_per_em();
        Ok(Face { font, upem })
    }

    /// The glyph the best cmap gives `c`.
    #[must_use]
    pub fn glyph(&self, c: char) -> Option<GlyphId> {
        self.font.cmap().ok()?.map_codepoint(c)
    }

    /// The advance width, `hmtx[name][0]`.
    ///
    /// # Panics
    /// When the face has no hmtx.
    #[must_use]
    pub fn advance(&self, g: GlyphId) -> u16 {
        self.font
            .hmtx()
            .expect("an hmtx")
            .advance(g)
            .expect("an advance")
    }

    fn lsb(&self, g: GlyphId) -> i16 {
        self.font
            .hmtx()
            .expect("an hmtx")
            .side_bearing(g)
            .expect("a side bearing")
    }

    fn outline(&self, g: GlyphId) -> Option<Glyph<'a>> {
        let loca = self.font.loca(None).expect("a loca");
        let glyf = self.font.glyf().expect("a glyf");
        loca.get_glyf(g, &glyf).expect("a readable glyph")
    }

    /// `glyphSet[name].draw(pen)` at the top level, through
    /// `TransformPen(pen, t)` when `t` is given.
    pub fn draw(&self, g: GlyphId, pen: &mut dyn Pen, t: Option<Transform>) {
        self.draw_at(g, pen, t, true);
    }

    /// A glyph drawn into `pen` through `t`. A component's transform is
    /// composed onto `t` (`TransformPen.addComponent`), and the base pen
    /// draws through the result unless it is the identity
    /// (`DecomposingPen.addComponent`).
    fn draw_at(&self, g: GlyphId, pen: &mut dyn Pen, t: Option<Transform>, top: bool) {
        let Some(glyph) = self.outline(g) else {
            return;
        };
        match glyph {
            Glyph::Composite(c) => {
                for comp in c.components() {
                    let (x, y) = match comp.anchor {
                        Anchor::Offset { x, y } => (f64::from(x), f64::from(y)),
                        Anchor::Point { .. } => panic!("a point-anchored component in glyph {g}"),
                    };
                    let m = comp.transform;
                    let one = read_fonts::types::F2Dot14::from_f32(1.0);
                    let zero = read_fonts::types::F2Dot14::from_f32(0.0);
                    assert!(
                        m.xx == one && m.yy == one && m.xy == zero && m.yx == zero,
                        "a scaled component in glyph {g}"
                    );
                    let own = Transform([1.0, 0.0, 0.0, 1.0, x, y]);
                    let composed = t.map_or(own, |t| t.then(&own));
                    let composed = (composed != Transform::IDENTITY).then_some(composed);
                    self.draw_at(comp.glyph, pen, composed, false);
                }
            }
            Glyph::Simple(s) => {
                let offset = if top {
                    f64::from(self.lsb(g)) - f64::from(s.x_min())
                } else {
                    0.0
                };
                let pts: Vec<_> = s.points().collect();
                let ends: Vec<usize> = s
                    .end_pts_of_contours()
                    .iter()
                    .map(|e| usize::from(e.get()))
                    .collect();
                let mut start = 0;
                for end in ends {
                    let contour: Vec<(Pt, bool)> = pts[start..=end]
                        .iter()
                        .map(|p| {
                            assert!(!p.cubic, "a cubic contour in glyph {g}");
                            ((f64::from(p.x) + offset, f64::from(p.y)), p.on_curve)
                        })
                        .collect();
                    start = end + 1;
                    contour_to(&mut Through { pen: &mut *pen, t }, contour);
                }
            }
        }
    }
}

/// One contour, as `Glyph.draw` walks it.
fn contour_to(pen: &mut dyn Pen, contour: Vec<(Pt, bool)>) {
    let Some(first) = contour.iter().position(|&(_, on)| on) else {
        let pts: Vec<Pt> = contour.iter().map(|&(p, _)| p).collect();
        if !pts.is_empty() {
            pen.q_curve_to(&pts, None);
            pen.close_path();
        }
        return;
    };
    let mut c = contour[first + 1..].to_vec();
    c.extend_from_slice(&contour[..=first]);
    pen.move_to(c[c.len() - 1].0);
    let mut rest = &c[..];
    while !rest.is_empty() {
        let next = rest
            .iter()
            .position(|&(_, on)| on)
            .expect("an on-curve point")
            + 1;
        if next == 1 {
            if rest.len() > 1 {
                pen.line_to(rest[0].0);
            }
        } else {
            let pts: Vec<Pt> = rest[..next - 1].iter().map(|&(p, _)| p).collect();
            pen.q_curve_to(&pts, Some(rest[next - 1].0));
        }
        rest = &rest[next..];
    }
    pen.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `calcQuadraticBounds`'s own examples (fontTools 4.63.0's
    /// docstring), and `BoundsPen` against `ControlBoundsPen` on the first.
    #[test]
    fn the_bounds_are_the_curves_not_its_control_points() {
        assert_eq!(
            quadratic_bounds((0.0, 0.0), (50.0, 100.0), (100.0, 0.0)),
            [0.0, 0.0, 100.0, 50.0]
        );
        assert_eq!(
            quadratic_bounds((0.0, 0.0), (100.0, 0.0), (100.0, 100.0)),
            [0.0, 0.0, 100.0, 100.0]
        );
        let mut b = Bounds::default();
        b.move_to((0.0, 0.0));
        b.q_curve_to(&[(50.0, 100.0)], Some((100.0, 0.0)));
        b.close_path();
        assert_eq!(
            b.bounds,
            Some([0.0, 0.0, 100.0, 50.0]),
            "the curve's top, not the control point's"
        );
    }

    /// `Transform.transform`'s own example, and a component's offset
    /// composed onto the pen's transform: the offset is scaled before the
    /// pen's translation is added, `k*x + (k*dx + cx)`, which at x 182 is
    /// one ulp from `k*(x + dx) + cx`, the control.
    #[test]
    fn a_component_is_drawn_through_the_composed_transform() {
        let t = Transform([2.0, 0.0, 0.0, 3.0, 1.0, 6.0]);
        assert_eq!(
            t.then(&Transform([4.0, 3.0, 2.0, 1.0, 5.0, 6.0])).0,
            [8.0, 9.0, 4.0, 3.0, 11.0, 24.0]
        );
        let (k, cx, x) = (0.038, 560.0, 182.0);
        let c = Transform([k, 0.0, 0.0, -k, cx, 0.0])
            .then(&Transform([1.0, 0.0, 0.0, 1.0, 615.0, 0.0]));
        assert_eq!(c.point((x, 0.0)).0, k * x + (k * 615.0 + cx));
        assert_eq!(
            c.point((x, 0.0)).0,
            590.286_000_000_000_1,
            "fontTools' value"
        );
        assert_ne!(c.point((x, 0.0)).0, k * (x + 615.0) + cx, "the control");
    }

    /// `SVGPathPen`'s own examples: a line along an axis is `H` or `V`, a
    /// line straight after a move is bare, a line to the current point is
    /// dropped, and a move after a move replaces it.
    #[test]
    fn the_path_commands_are_svgpathpens() {
        let ntos = |v: f64| crate::py::num(v);
        let mut p = SvgPath::new(&ntos);
        p.move_to((0.0, 0.0));
        p.move_to((10.0, 10.0));
        p.line_to((10.0, 10.0));
        p.line_to((20.0, 20.0));
        p.line_to((20.0, 0.0));
        p.line_to((0.0, 0.0));
        p.line_to((70.0, 80.0));
        p.q_curve_to(&[(3.0, 3.0), (7.0, 5.0)], Some((11.0, 4.0)));
        p.close_path();
        assert_eq!(p.commands(), "M10 10 20 20V0H0L70 80Q3 3 5 4Q7 5 11 4Z");
    }
}
