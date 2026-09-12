//! The glyph atlas: one `A8` texture of coverage masks, skyline-packed,
//! grow-only, keyed by face and glyph.
//!
//! A mask is rasterised the first time a glyph is asked for and never
//! again while the atlas lives; a size or font change starts a new atlas.
//! When the skyline cannot place a mask the atlas doubles and empties,
//! and every glyph is rasterised again on demand -- simpler than repacking
//! and it happens a handful of times in a session.

use crate::stack::FaceIx;
use crate::units::{Tally, slots};
use baud_domain::GlyphId;
use baud_font::{Bounds, Canvas, Face, Fit, Px, Rasterizer, Scale};
use std::collections::HashMap;

/// A pixel position or size inside the atlas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tex(pub u16);

/// Where a glyph's mask sits in the atlas and where it goes on screen:
/// `left` pixels right of the glyph origin, `top` pixels below the
/// baseline (negative above it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub x: Tex,
    pub y: Tex,
    pub w: Tex,
    pub h: Tex,
    pub left: i32,
    pub top: i32,
}

/// A glyph's index in its face, hashable.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphIx(u16);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: FaceIx,
    glyph: GlyphIx,
}

/// One skyline segment: from `x` for `w` pixels the top edge is at `y`.
#[derive(Clone, Copy)]
struct Node {
    x: Px,
    y: Px,
    w: Px,
}

/// The atlas's side, a power of two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Side(u32);

impl Side {
    pub const FIRST: Side = Side(512);
    pub const LIMIT: Side = Side(8192);

    pub fn value(self) -> u32 {
        self.0
    }
}

/// What the atlas has done, for the frame report.
#[derive(Clone, Copy, Default, Debug)]
pub struct AtlasStats {
    pub rasterised: Tally,
    pub grown: Tally,
}

pub struct Atlas {
    side: Side,
    pixels: Vec<u8>,
    skyline: Vec<Node>,
    /// `None` records a glyph with no ink, so a space is looked up once.
    map: HashMap<GlyphKey, Option<Slot>>,
    scratch: Vec<f32>,
    coverage: Vec<u8>,
    stats: AtlasStats,
}

impl Default for Atlas {
    fn default() -> Self {
        Atlas::new()
    }
}

impl Atlas {
    pub fn new() -> Atlas {
        let mut a = Atlas {
            side: Side::FIRST,
            pixels: Vec::new(),
            skyline: Vec::new(),
            map: HashMap::new(),
            scratch: Vec::new(),
            coverage: Vec::new(),
            stats: AtlasStats::default(),
        };
        a.reset(Side::FIRST);
        a
    }

    fn reset(&mut self, side: Side) {
        self.side = side;
        let n = slots(Px::of(side.0));
        self.pixels.clear();
        self.pixels.resize(n * n, 0);
        self.skyline.clear();
        self.skyline.push(Node {
            x: Px::of(0),
            y: Px::of(0),
            w: Px::of(side.0),
        });
        self.map.clear();
    }

    pub fn side(&self) -> Side {
        self.side
    }

    /// The `A8` texels, `side` by `side`, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn stats(&self) -> AtlasStats {
        self.stats
    }

    /// The mask of `id` in `face`, rasterising it at `scale` on the first
    /// ask. `None` for a glyph that draws nothing.
    pub fn slot(&mut self, face: &Face<'_>, ix: FaceIx, id: GlyphId, scale: Scale) -> Option<Slot> {
        let key = GlyphKey {
            face: ix,
            glyph: GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX)),
        };
        if let Some(known) = self.map.get(&key) {
            return *known;
        }
        let slot = self.rasterise(face, id, scale);
        // A mask that would not fit even after growing is not recorded, so
        // a later, larger atlas may take it.
        if slot.is_some() || self.fits_nothing_anyway(face, id, scale) {
            self.map.insert(key, slot);
        }
        slot
    }

    /// Whether `id` has no ink at all, so `None` is its permanent answer.
    fn fits_nothing_anyway(&mut self, face: &Face<'_>, id: GlyphId, scale: Scale) -> bool {
        let mut b = Bounds::new();
        face.outline(id, &mut b);
        Fit::of(&b, scale).is_none()
    }

    fn rasterise(&mut self, face: &Face<'_>, id: GlyphId, scale: Scale) -> Option<Slot> {
        let mut b = Bounds::new();
        face.outline(id, &mut b);
        let fit = Fit::of(&b, scale)?;
        let need = fit.extent.scratch()?;
        if self.scratch.len() < need {
            self.scratch.resize(need, 0.0);
        }
        self.scratch.fill(0.0);
        let cells = fit.extent.cells()?;
        if self.coverage.len() < cells {
            self.coverage.resize(cells, 0);
        }
        let canvas = Canvas::new(fit.extent, &mut self.scratch)?;
        let mut r = Rasterizer::new(canvas, fit.transform);
        face.outline(id, &mut r);
        let canvas = r.finish();
        canvas.coverage(&mut self.coverage[..cells])?;
        self.stats.rasterised.bump();
        let (w, h) = (fit.extent.w, fit.extent.h);
        let (x, y) = loop {
            if let Some(at) = self.place(w, h) {
                break at;
            }
            let next = Side(self.side.0.checked_mul(2)?);
            if next.0 > Side::LIMIT.0 {
                return None;
            }
            self.stats.grown.bump();
            self.reset(next);
        };
        self.blit(x, y, w, h);
        Some(Slot {
            x: Tex(u16::try_from(x.value()).ok()?),
            y: Tex(u16::try_from(y.value()).ok()?),
            w: Tex(u16::try_from(w.value()).ok()?),
            h: Tex(u16::try_from(h.value()).ok()?),
            left: fit.left,
            top: fit.top,
        })
    }

    fn blit(&mut self, x: Px, y: Px, w: Px, h: Px) {
        let side = slots(Px::of(self.side.0));
        let (x, y, w, h) = (slots(x), slots(y), slots(w), slots(h));
        for row in 0..h {
            let src = &self.coverage[row * w..(row + 1) * w];
            let at = (y + row) * side + x;
            if let Some(dst) = self.pixels.get_mut(at..at + w) {
                dst.copy_from_slice(src);
            }
        }
    }

    /// Bottom-left skyline: the lowest top edge along which `w` fits, ties
    /// to the left. `None` when nothing fits.
    fn place(&mut self, w: Px, h: Px) -> Option<(Px, Px)> {
        let side = self.side.0;
        let (w, h) = (w.value(), h.value());
        if w > side || h > side {
            return None;
        }
        let mut best: Option<(usize, u32, u32)> = None;
        for i in 0..self.skyline.len() {
            let x = self.skyline[i].x.value();
            if x + w > side {
                break;
            }
            // The top edge over [x, x + w) is the highest node it crosses.
            let mut y = 0u32;
            let mut span = 0u32;
            let mut j = i;
            while span < w {
                let n = self.skyline.get(j)?;
                y = y.max(n.y.value());
                span += n.w.value();
                j += 1;
            }
            if y + h > side {
                continue;
            }
            if best.is_none_or(|(_, _, by)| y < by) {
                best = Some((i, x, y));
            }
        }
        let (i, x, y) = best?;
        // Insert the new top and trim the nodes it covers.
        self.skyline.insert(
            i,
            Node {
                x: Px::of(x),
                y: Px::of(y + h),
                w: Px::of(w),
            },
        );
        let j = i + 1;
        while j < self.skyline.len() {
            let n = self.skyline[j];
            let end = x + w;
            if n.x.value() < end {
                let shrink = end - n.x.value();
                if shrink >= n.w.value() {
                    self.skyline.remove(j);
                    continue;
                }
                self.skyline[j].x = Px::of(n.x.value() + shrink);
                self.skyline[j].w = Px::of(n.w.value() - shrink);
            }
            break;
        }
        // Merge equal neighbours.
        let mut k = 0;
        while k + 1 < self.skyline.len() {
            if self.skyline[k].y == self.skyline[k + 1].y {
                let w = self.skyline[k].w.value() + self.skyline[k + 1].w.value();
                self.skyline[k].w = Px::of(w);
                self.skyline.remove(k + 1);
            } else {
                k += 1;
            }
        }
        Some((Px::of(x), Px::of(y)))
    }
}
