//! The glyph atlas: one `A8` texture of coverage masks and one `ARGB`
//! texture of colour glyphs, each skyline-packed, grow-only, keyed by face
//! and glyph.
//!
//! A glyph is resolved the first time it is asked for and never again
//! while the atlas lives; a size or font change starts a new atlas. A
//! face's bitmap strike (`sbix`, `CBDT`) is tried first at the strike
//! nearest the cell, then its `COLR` layers painted through palette zero,
//! then the outline as a mask -- the order Apple Color Emoji, Noto Color
//! Emoji and Bungee Tint each need. When a skyline cannot place an image
//! its texture doubles and empties, and every glyph on it is resolved
//! again on demand -- simpler than repacking and it happens a handful of
//! times in a session.

use crate::color::Argb;
use crate::stack::FaceIx;
use crate::units::{Coverage, Tally, slots};
use baud_domain::{Chan, GlyphId};
use baud_font::bytes::Index;
use baud_font::{
    Bounds, Canvas, Face, Fit, Palette, PaletteEntry, Png, Px, Rasterizer, Scale, Strike,
};
use std::collections::HashMap;

/// A pixel position or size inside the atlas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tex(pub u16);

/// Which texture an image lives on, and so how it composites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// An `A8` coverage mask, painted in the instance's colour.
    Mask,
    /// An `ARGB` image, painted as it is; the instance's colour is unused.
    Color,
    /// No texels at all: every pixel fully covered, in the instance's
    /// colour. Underlines, strikes and cursor shapes.
    Solid,
}

/// Where an image goes on screen relative to the glyph origin: `left`
/// pixels to the right, `top` pixels below the baseline (negative above).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Origin {
    pub left: i32,
    pub top: i32,
}

/// Where a glyph's image sits in the atlas and where it goes on screen:
/// `left` pixels right of the glyph origin, `top` pixels below the
/// baseline (negative above it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub kind: Kind,
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
    /// Bitmap strikes decoded and `COLR` glyphs painted.
    pub coloured: Tally,
    pub grown: Tally,
}

/// One texture and its skyline.
struct Sheet {
    side: Side,
    skyline: Vec<Node>,
}

impl Sheet {
    fn new(side: Side) -> Sheet {
        Sheet {
            side,
            skyline: vec![Node {
                x: Px::of(0),
                y: Px::of(0),
                w: Px::of(side.0),
            }],
        }
    }
}

pub struct Atlas {
    mask: Sheet,
    pixels: Vec<u8>,
    color: Sheet,
    argb: Vec<u32>,
    /// `None` records a glyph with no ink, so a space is looked up once.
    map: HashMap<GlyphKey, Option<Slot>>,
    scratch: Vec<f32>,
    coverage: Vec<u8>,
    raw: Vec<u8>,
    rgba: Vec<u8>,
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
            mask: Sheet::new(Side::FIRST),
            pixels: Vec::new(),
            color: Sheet::new(Side::FIRST),
            argb: Vec::new(),
            map: HashMap::new(),
            scratch: Vec::new(),
            coverage: Vec::new(),
            raw: Vec::new(),
            rgba: Vec::new(),
            stats: AtlasStats::default(),
        };
        a.reset_mask(Side::FIRST);
        a.reset_color(Side::FIRST);
        a
    }

    fn reset_mask(&mut self, side: Side) {
        self.mask = Sheet::new(side);
        let n = slots(Px::of(side.0));
        self.pixels.clear();
        self.pixels.resize(n * n, 0);
        self.map.retain(|_, s| {
            !matches!(
                s,
                Some(Slot {
                    kind: Kind::Mask,
                    ..
                })
            )
        });
    }

    fn reset_color(&mut self, side: Side) {
        self.color = Sheet::new(side);
        let n = slots(Px::of(side.0));
        self.argb.clear();
        self.argb.resize(n * n, 0);
        self.map.retain(|_, s| {
            !matches!(
                s,
                Some(Slot {
                    kind: Kind::Color,
                    ..
                })
            )
        });
    }

    /// The mask texture's side.
    pub fn side(&self) -> Side {
        self.mask.side
    }

    /// The colour texture's side.
    pub fn color_side(&self) -> Side {
        self.color.side
    }

    /// The `A8` texels, `side` by `side`, row-major.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The `ARGB` texels, `color_side` by `color_side`, row-major.
    pub fn argb(&self) -> &[u32] {
        &self.argb
    }

    pub fn stats(&self) -> AtlasStats {
        self.stats
    }

    /// The image of `id` in `face`, resolved on the first ask: its bitmap
    /// strike nearest `scale`, else its `COLR` layers, else its outline as
    /// a mask. `None` for a glyph that draws nothing.
    pub fn slot(&mut self, face: &Face<'_>, ix: FaceIx, id: GlyphId, scale: Scale) -> Option<Slot> {
        let key = GlyphKey {
            face: ix,
            glyph: GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX)),
        };
        if let Some(known) = self.map.get(&key) {
            return *known;
        }
        let slot = self
            .bitmap(face, id, scale)
            .or_else(|| self.layered(face, id, scale))
            .or_else(|| self.rasterise(face, id, scale));
        // An image that would not fit even after growing is not recorded,
        // so a later, larger atlas may take it.
        if slot.is_some() || self.fits_nothing_anyway(face, id, scale) {
            self.map.insert(key, slot);
        }
        slot
    }

    /// The strike whose pixels per em is nearest the cell, preferring the
    /// first at or above it, and the glyph's image on it scaled to fit.
    fn bitmap(&mut self, face: &Face<'_>, id: GlyphId, scale: Scale) -> Option<Slot> {
        let strikes = face.strikes();
        if strikes.is_zero() {
            return None;
        }
        let target = scale.px(f32::from(face.units_per_em().value()));
        let mut best: Option<(Index, f32)> = None;
        let mut i = Index::ZERO;
        while strikes.holds(i) {
            if let Some(ppem) = face.strike_ppem(i) {
                let p = f32::from(ppem.value());
                let score = if p >= target {
                    p - target
                } else {
                    (target - p) * 4.0
                };
                if best.is_none_or(|(_, s)| score < s) {
                    best = Some((i, score));
                }
            }
            i = i.next();
        }
        let (strike, _) = best?;
        let bitmap = face.bitmap(strike, id)?;
        let png = Png::parse(bitmap.png.as_slice()).ok()?;
        let (raw_len, rgba_len) = (png.raw_len()?, png.rgba_len()?);
        if self.raw.len() < raw_len {
            self.raw.resize(raw_len, 0);
        }
        if self.rgba.len() < rgba_len {
            self.rgba.resize(rgba_len, 0);
        }
        let size = png.decode(&mut self.raw, &mut self.rgba).ok()?;
        let factor = target / f32::from(bitmap.ppem.value());
        let (sw, sh) = (size.width, size.height);
        let dw = (sw as f32 * factor).round().max(1.0) as i64;
        let dh = (sh as f32 * factor).round().max(1.0) as i64;
        let (dw, dh) = (u32::try_from(dw).ok()?, u32::try_from(dh).ok()?);
        if dw > Side::LIMIT.0 || dh > Side::LIMIT.0 {
            return None;
        }
        // Box-filter the RGBA down to the cell's scale, straight alpha.
        let mut scaled = vec![0u32; slots(Px::of(dw)) * slots(Px::of(dh))];
        let (sw_u, sh_u) = (slots(Px::of(sw)), slots(Px::of(sh)));
        for dy in 0..slots(Px::of(dh)) {
            let y0 = dy * sh_u / slots(Px::of(dh));
            let y1 = ((dy + 1) * sh_u / slots(Px::of(dh))).max(y0 + 1).min(sh_u);
            for dx in 0..slots(Px::of(dw)) {
                let x0 = dx * sw_u / slots(Px::of(dw));
                let x1 = ((dx + 1) * sw_u / slots(Px::of(dw))).max(x0 + 1).min(sw_u);
                let (mut r, mut g, mut b, mut a, mut n) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let at = (y * sw_u + x) * 4;
                        let px = &self.rgba[at..at + 4];
                        let alpha = u32::from(px[3]);
                        // Weight colour by alpha so transparent texels do
                        // not darken the edge.
                        r += u32::from(px[0]) * alpha;
                        g += u32::from(px[1]) * alpha;
                        b += u32::from(px[2]) * alpha;
                        a += alpha;
                        n += 1;
                    }
                }
                let out = if a == 0 {
                    Argb(0)
                } else {
                    let ch = |v: u32| Chan::from_u8(u8::try_from(v / a).unwrap_or(255));
                    Argb::rgb(ch(r), ch(g), ch(b))
                        .with_alpha(Chan::from_u8(u8::try_from(a / n.max(1)).unwrap_or(255)))
                };
                scaled[dy * slots(Px::of(dw)) + dx] = out.0;
            }
        }
        // Placement: `sbix` offsets the image's bottom-left corner from the
        // origin, `CBDT` bears its top-left; both in the strike's pixels.
        let (left, top) = match bitmap.strike {
            Strike::Sbix => (
                (f32::from(bitmap.placement.x) * factor).round() as i32,
                -((f32::from(bitmap.placement.y) * factor).round() as i32)
                    - i32::try_from(dh).ok()?,
            ),
            Strike::Cbdt => (
                (f32::from(bitmap.placement.x) * factor).round() as i32,
                -((f32::from(bitmap.placement.y) * factor).round() as i32),
            ),
        };
        self.stats.coloured.bump();
        self.place_color(&scaled, Px::of(dw), Px::of(dh), Origin { left, top })
    }

    /// `COLR` v0: every layer's outline painted in its palette colour into
    /// one image, bottom layer first.
    fn layered(&mut self, face: &Face<'_>, id: GlyphId, scale: Scale) -> Option<Slot> {
        let layers = face.layers(id)?;
        // The union box of every layer decides the image.
        let mut b = Bounds::new();
        for layer in layers {
            face.outline(layer.glyph, &mut b);
        }
        let fit = Fit::of(&b, scale)?;
        let cells = fit.extent.cells()?;
        let need = fit.extent.scratch()?;
        if self.scratch.len() < need {
            self.scratch.resize(need, 0.0);
        }
        if self.coverage.len() < cells {
            self.coverage.resize(cells, 0);
        }
        let mut image = vec![0u32; cells];
        for layer in face.layers(id)? {
            let color = match face.color(Palette::of(0), layer.color) {
                Some(c) => Argb::rgb(Chan::from_u8(c.r), Chan::from_u8(c.g), Chan::from_u8(c.b))
                    .with_alpha(Chan::from_u8(c.a)),
                // The foreground entry is the text colour, which a
                // shared image cannot carry: white, so it still shows.
                None if layer.color == PaletteEntry::FOREGROUND => Argb(0xFFFF_FFFF),
                None => continue,
            };
            self.scratch.fill(0.0);
            let canvas = Canvas::new(fit.extent, &mut self.scratch)?;
            let mut r = Rasterizer::new(canvas, fit.transform);
            face.outline(layer.glyph, &mut r);
            r.finish().coverage(&mut self.coverage[..cells])?;
            for (px, &cov) in image.iter_mut().zip(&self.coverage[..cells]) {
                if cov == 0 {
                    continue;
                }
                *px = over_straight(color, Argb(*px), Coverage(cov)).0;
            }
        }
        self.stats.coloured.bump();
        self.place_color(
            &image,
            fit.extent.w,
            fit.extent.h,
            Origin {
                left: fit.left,
                top: fit.top,
            },
        )
    }

    /// Packs an `ARGB` image onto the colour sheet, growing it as needed.
    fn place_color(&mut self, image: &[u32], w: Px, h: Px, at: Origin) -> Option<Slot> {
        let (x, y) = loop {
            if let Some(at) = place(&mut self.color, w, h) {
                break at;
            }
            let next = Side(self.color.side.0.checked_mul(2)?);
            if next.0 > Side::LIMIT.0 {
                return None;
            }
            self.stats.grown.bump();
            self.reset_color(next);
        };
        let side = slots(Px::of(self.color.side.0));
        let (xs, ys, ws, hs) = (slots(x), slots(y), slots(w), slots(h));
        for row in 0..hs {
            let src = &image[row * ws..(row + 1) * ws];
            let at = (ys + row) * side + xs;
            if let Some(dst) = self.argb.get_mut(at..at + ws) {
                dst.copy_from_slice(src);
            }
        }
        Some(Slot {
            kind: Kind::Color,
            x: Tex(u16::try_from(x.value()).ok()?),
            y: Tex(u16::try_from(y.value()).ok()?),
            w: Tex(u16::try_from(w.value()).ok()?),
            h: Tex(u16::try_from(h.value()).ok()?),
            left: at.left,
            top: at.top,
        })
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
            if let Some(at) = place(&mut self.mask, w, h) {
                break at;
            }
            let next = Side(self.mask.side.0.checked_mul(2)?);
            if next.0 > Side::LIMIT.0 {
                return None;
            }
            self.stats.grown.bump();
            self.reset_mask(next);
        };
        self.blit(x, y, w, h);
        Some(Slot {
            kind: Kind::Mask,
            x: Tex(u16::try_from(x.value()).ok()?),
            y: Tex(u16::try_from(y.value()).ok()?),
            w: Tex(u16::try_from(w.value()).ok()?),
            h: Tex(u16::try_from(h.value()).ok()?),
            left: fit.left,
            top: fit.top,
        })
    }

    fn blit(&mut self, x: Px, y: Px, w: Px, h: Px) {
        let side = slots(Px::of(self.mask.side.0));
        let (x, y, w, h) = (slots(x), slots(y), slots(w), slots(h));
        for row in 0..h {
            let src = &self.coverage[row * w..(row + 1) * w];
            let at = (y + row) * side + x;
            if let Some(dst) = self.pixels.get_mut(at..at + w) {
                dst.copy_from_slice(src);
            }
        }
    }
}

/// Bottom-left skyline: the lowest top edge along which `w` fits, ties
/// to the left. `None` when nothing fits.
fn place(sheet: &mut Sheet, w: Px, h: Px) -> Option<(Px, Px)> {
    let side = sheet.side.0;
    let (w, h) = (w.value(), h.value());
    if w > side || h > side {
        return None;
    }
    let mut best: Option<(usize, u32, u32)> = None;
    for i in 0..sheet.skyline.len() {
        let x = sheet.skyline[i].x.value();
        if x + w > side {
            break;
        }
        // The top edge over [x, x + w) is the highest node it crosses.
        let mut y = 0u32;
        let mut span = 0u32;
        let mut j = i;
        while span < w {
            let n = sheet.skyline.get(j)?;
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
    sheet.skyline.insert(
        i,
        Node {
            x: Px::of(x),
            y: Px::of(y + h),
            w: Px::of(w),
        },
    );
    let j = i + 1;
    while j < sheet.skyline.len() {
        let n = sheet.skyline[j];
        let end = x + w;
        if n.x.value() < end {
            let shrink = end - n.x.value();
            if shrink >= n.w.value() {
                sheet.skyline.remove(j);
                continue;
            }
            sheet.skyline[j].x = Px::of(n.x.value() + shrink);
            sheet.skyline[j].w = Px::of(n.w.value() - shrink);
        }
        break;
    }
    // Merge equal neighbours.
    let mut k = 0;
    while k + 1 < sheet.skyline.len() {
        if sheet.skyline[k].y == sheet.skyline[k + 1].y {
            let w = sheet.skyline[k].w.value() + sheet.skyline[k + 1].w.value();
            sheet.skyline[k].w = Px::of(w);
            sheet.skyline.remove(k + 1);
        } else {
            k += 1;
        }
    }
    Some((Px::of(x), Px::of(y)))
}

/// `src` over `dst`, straight alpha, with `cov` scaling the source: what
/// a `COLR` layer does to the layers below it.
fn over_straight(src: Argb, dst: Argb, cov: Coverage) -> Argb {
    let a = u32::from(cov.0) * u32::from(src.a().get()) / 255;
    let da = u32::from(dst.a().get());
    let out_a = a + da * (255 - a) / 255;
    if out_a == 0 {
        return Argb(0);
    }
    let mix = |s: Chan, d: Chan| -> Chan {
        let v = (u32::from(s.get()) * a + u32::from(d.get()) * da * (255 - a) / 255) / out_a;
        Chan::from_u8(u8::try_from(v).unwrap_or(255))
    };
    Argb::rgb(
        mix(src.r(), dst.r()),
        mix(src.g(), dst.g()),
        mix(src.b(), dst.b()),
    )
    .with_alpha(Chan::from_u8(u8::try_from(out_a).unwrap_or(255)))
}
