//! Where an outline goes, and how a composite moves it.
//!
//! **Font units, `f32`, no flattening.** A [`Sink`] receives the outline
//! exactly as the file describes it -- quadratic curves from `glyf`, cubics
//! from CFF -- in design units, and flattening to the pixel grid is the
//! rasteriser's job (register D6). `f32` because that is what both formats'
//! arithmetic is specified in for composites and charstrings, and because the
//! differential against the oracle is bit-exact only if the operations are
//! the same operations.
//!
//! **The composite transform is the specification's 2x2 plus offset**
//! (`[[source: ms-opentype-glyf]]`): `x' = a·x + c·y + e`, `y' = b·x + d·y + f`,
//! and nesting composes by ordinary matrix multiplication with the outer
//! transform applied last.
//!
//! **A flex is the one segment whose shape depends on the device.** Both
//! charstring texts define it as two curves that "must be rendered as a
//! straight line when the flex depth is less than" a threshold in device
//! pixels (`[[source: adobe-5177-type2]]` § 4.1; `[[source:
//! ms-opentype-cff2]]`, "Flex mechanism operators"), so the reader cannot
//! decide it in font units: it hands the sink a [`Flex`], and a sink that
//! knows its device -- [`crate::Rasterizer`] -- decides. Every other sink
//! receives the two curves, which is the outline in font units, the form the
//! oracles compare.

/// The receiver of an outline. Every method is in font units.
pub trait Sink {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32);
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    fn close(&mut self);
    /// A flex from the current point. A sink with no device draws its two
    /// curves; one that has a device draws [`Flex::rendered`] there.
    fn flex(&mut self, f: &Flex) {
        let [x1, y1, x2, y2, x, y] = f.first;
        self.curve_to(x1, y1, x2, y2, x, y);
        let [x1, y1, x2, y2, x, y] = f.second;
        self.curve_to(x1, y1, x2, y2, x, y);
    }
}

/// A flex: two cubics from the current point through the joining point to
/// the end, its flex depth, and the threshold below which the text says it
/// "must be rendered as a straight line".
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Flex {
    /// The first curve, to the joining point: `[x1, y1, x2, y2, x, y]`.
    pub first: [f32; 6],
    /// The second curve, to the end: `[x1, y1, x2, y2, x, y]`.
    pub second: [f32; 6],
    /// The flex depth, as a vector in font units across the flex: `[0, dy]`
    /// for a horizontal flex and `[dx, 0]` for a vertical one. Where the
    /// start and the end share a `y` (or an `x`), it is the joining point's
    /// offset from that line; where they share neither, or both, its offset
    /// from the end point's own line, as the text's figure draws it (the
    /// owner's decision of 2026-10-13; the record, "A flex shallower than
    /// half a pixel draws straight", question 6).
    pub depth: [f32; 2],
    /// The threshold in device pixels: `fd / 100` for `flex`, and the
    /// implicit 0.5 ("threshold fd = 50 is implicit") for `hflex`, `hflex1`
    /// and `flex1`.
    pub threshold: f32,
}

/// How a flex is drawn on a device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlexDrawn {
    /// One line from the start to the end: the depth is "less than" the
    /// threshold.
    Straight,
    /// The two curves: the depth is "greater than or equal to" it.
    Curved,
}

impl Flex {
    /// How the flex is drawn under `t`, the transform from font units to
    /// device pixels: the depth vector is carried by `t`'s linear part, and
    /// its length is the depth in device pixels.
    pub fn rendered(&self, t: &Transform) -> FlexDrawn {
        let [dx, dy] = self.depth;
        let (px, py) = (t.a * dx + t.c * dy, t.b * dx + t.d * dy);
        if core::f32::math::sqrt(px * px + py * py) < self.threshold {
            FlexDrawn::Straight
        } else {
            FlexDrawn::Curved
        }
    }
}

/// Whether a glyph produced anything: the empty glyph -- a space, a `loca`
/// entry of zero length, a charstring with no path -- is not an error, and a
/// caller that wants to know must be told rather than left to count calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drawn {
    Outline,
    Nothing,
    /// A glyph that can be drawn only through scratch, asked for without it
    /// or with too little: one whose outline varies at the face's
    /// coordinates, or, at any instance, a composite that places a
    /// component by matched points. It needs this many points of
    /// [`crate::VarPoint`], and nothing was drawn -- never the default
    /// instance, or the component at offset 0, in its place.
    NeedsScratch(crate::bytes::Count),
    /// A glyph the face has but whose description the text says cannot be:
    /// a `glyf` composite whose first component, at any depth of its tree,
    /// is placed by matched points ("cannot be used for the first component
    /// glyph", `[[source: ms-opentype-glyf]]`). Nothing was drawn, and a
    /// caller counts it rather than take it for a glyph with no ink, and
    /// shows a placeholder in its cell, never a blank (TUS § 5.3): another
    /// face's glyph for the character, or a `.notdef` ([`crate::Face::refuses`]
    /// asks before drawing). A colour glyph whose own outline is refused
    /// still draws from its strike image or `COLR` layers.
    Refused,
}

/// An affine transform, `[a b c d e f]` in the order the specifications
/// write it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Transform {
    pub const IDENTITY: Transform = Transform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn is_identity(&self) -> bool {
        *self == Transform::IDENTITY
    }

    /// `outer ∘ inner`: apply `inner` first, then `outer`.
    pub fn combine(outer: Transform, inner: Transform) -> Transform {
        Transform {
            a: outer.a * inner.a + outer.c * inner.b,
            b: outer.b * inner.a + outer.d * inner.b,
            c: outer.a * inner.c + outer.c * inner.d,
            d: outer.b * inner.c + outer.d * inner.d,
            e: outer.a * inner.e + outer.c * inner.f + outer.e,
            f: outer.b * inner.e + outer.d * inner.f + outer.f,
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `upem` 1024 at `px` pixels per em, `y` flipped as the rasteriser
    /// places an outline.
    fn at(px: f32) -> Transform {
        let s = px / 1024.0;
        Transform {
            a: s,
            b: 0.0,
            c: 0.0,
            d: -s,
            e: 0.0,
            f: 0.0,
        }
    }

    fn flex(depth: [f32; 2], threshold: f32) -> Flex {
        Flex {
            first: [0.0; 6],
            second: [0.0; 6],
            depth,
            threshold,
        }
    }

    /// "Rendered as a straight line when the flex depth is less than"
    /// the threshold, "and as curves when the flex depth is greater than
    /// or equal to" it: 32 units of 1024 is exactly half a pixel at 16 px,
    /// so 16 px is curved, 15 straight, 17 curved -- across and along, and
    /// at `flex`'s own threshold of `fd` 30, which 32 units first passes
    /// between 9 px and 10.
    #[test]
    fn a_flex_is_straight_below_its_threshold_and_curved_at_it() {
        for depth in [[0.0, 32.0], [32.0, 0.0], [0.0, -32.0]] {
            let f = flex(depth, 0.5);
            assert_eq!(f.rendered(&at(15.0)), FlexDrawn::Straight, "{depth:?}");
            assert_eq!(f.rendered(&at(16.0)), FlexDrawn::Curved, "{depth:?} at 0.5");
            assert_eq!(f.rendered(&at(17.0)), FlexDrawn::Curved, "{depth:?}");
        }
        let fd30 = flex([0.0, 32.0], 0.3);
        assert_eq!(fd30.rendered(&at(9.0)), FlexDrawn::Straight);
        assert_eq!(fd30.rendered(&at(10.0)), FlexDrawn::Curved);
    }
}
