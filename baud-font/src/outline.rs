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

/// The receiver of an outline. Every method is in font units.
pub trait Sink {
    fn move_to(&mut self, x: f32, y: f32);
    fn line_to(&mut self, x: f32, y: f32);
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32);
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32);
    fn close(&mut self);
}

/// Whether a glyph produced anything: the empty glyph -- a space, a `loca`
/// entry of zero length, a charstring with no path -- is not an error, and a
/// caller that wants to know must be told rather than left to count calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drawn {
    Outline,
    Nothing,
    /// A glyph whose outline varies at the face's coordinates, asked for
    /// without scratch or with too little: it needs this many points of
    /// [`crate::VarPoint`], and nothing was drawn -- never the default
    /// instance in its place.
    NeedsScratch(crate::bytes::Count),
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
