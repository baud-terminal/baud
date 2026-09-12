//! The per-glyph record and the per-cell background word: the shape every
//! GPU terminal uploads (D2), built on the CPU and, for now, consumed by
//! the CPU compositor.

use crate::atlas::Tex;
use crate::color::Argb;

/// A screen coordinate in pixels, signed so a glyph may hang off an edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Screen(pub i16);

/// One drawn glyph: where on screen, which atlas rectangle, what colour.
/// Sixteen bytes, `repr(C)`, so a buffer of them is what a vertex fetch
/// reads.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Instance {
    pub x: Screen,
    pub y: Screen,
    pub w: Tex,
    pub h: Tex,
    pub u: Tex,
    pub v: Tex,
    pub color: Argb,
}
