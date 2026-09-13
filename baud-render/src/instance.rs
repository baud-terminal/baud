//! The per-glyph record and the per-cell background word: the shape every
//! GPU terminal uploads (D2), built on the CPU and, for now, consumed by
//! the CPU compositor.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;

/// A screen coordinate in pixels, signed so a glyph may hang off an edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Screen(pub i16);

impl Instance {
    /// The record's size in bytes, which the shader's struct must match.
    pub const BYTES: usize = 20;

    /// The record as the twenty bytes a shader reads, little-endian.
    pub fn to_bytes(self) -> [u8; Instance::BYTES] {
        let mut b = [0u8; Instance::BYTES];
        b[0..2].copy_from_slice(&self.x.0.to_le_bytes());
        b[2..4].copy_from_slice(&self.y.0.to_le_bytes());
        b[4..6].copy_from_slice(&self.w.0.to_le_bytes());
        b[6..8].copy_from_slice(&self.h.0.to_le_bytes());
        b[8..10].copy_from_slice(&self.u.0.to_le_bytes());
        b[10..12].copy_from_slice(&self.v.0.to_le_bytes());
        b[12..16].copy_from_slice(&self.color.0.to_le_bytes());
        b[16..20].copy_from_slice(&self.kind.code().0.to_le_bytes());
        b
    }
}

/// One drawn glyph: where on screen, which atlas rectangle on which
/// texture, what colour. Twenty bytes, `repr(C)` over `repr(transparent)`
/// newtypes and a `repr(u32)` kind, so a buffer of them is what a vertex
/// fetch reads: `i16 x, y; u16 w, h, u, v; u32 color; u32 kind`.
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
    pub kind: Kind,
}
