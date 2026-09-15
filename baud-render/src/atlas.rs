//! The glyph atlas: one `A8` texture of coverage masks and one `ARGB`
//! texture of colour glyphs, each carved into bands of uniform slots,
//! grow-only, keyed by face and glyph.
//!
//! A glyph is resolved the first time it is asked for and never again
//! while the atlas lives; a size or font change starts a new atlas. A
//! face's bitmap strike (`sbix`, `CBDT`) is tried first at the strike
//! nearest the cell, then its `COLR` layers painted through palette zero,
//! then the outline as a mask -- the order Apple Color Emoji, Noto Color
//! Emoji and Bungee Tint each need.
//!
//! **A slot's position is arithmetic, and growing keeps every one of
//! them.** The sheet is a stack of bands; a band is a strip `h` texels
//! tall carved into slots `w` texels wide, and an image's extent is
//! rounded up to a step so the classes stay few. So a slot is a band and
//! an index, its texel is `(index * w, band.y)`, and a sheet that fills
//! doubles and **copies its rows into the wider one at the same
//! coordinates** -- every rectangle already handed out stays valid and
//! nothing that cached one is told anything.
//!
//! That is the whole reason the bands are here. The skyline packer this
//! replaced answered a full sheet by doubling and *emptying*, so one
//! glyph that did not fit discarded every glyph in the atlas, re-rastered
//! everything still on screen and cleared the 384 KB glyph map -- mid
//! frame, while the GPU was sampling the texture it rewrote. A band never
//! moves, so the only operation that voids a rectangle is [`Atlas::reset`],
//! which a font size change asks for deliberately.
//!
//! Bands also admit per-slot eviction, since a freed slot is an index its
//! class can hand out again rather than a hole a packer would have to
//! repack around. Nothing frees a slot yet, so there is no free list to
//! consult: adding one before something fills it would be a mechanism
//! that cannot fire.

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
#[repr(transparent)]
pub struct Tex(pub u16);

/// Which texture an image lives on, and so how it composites. `repr(u32)`
/// with fixed discriminants, because the value rides in the instance
/// record a shader reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Kind {
    /// An `A8` coverage mask, painted in the instance's colour.
    Mask = 0,
    /// An `ARGB` image, painted as it is; the instance's colour is unused.
    Color = 1,
    /// No texels at all: every pixel fully covered, in the instance's
    /// colour. Underlines, strikes and cursor shapes.
    Solid = 2,
}

/// A kind's discriminant as the shader reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Code(pub u32);

impl Kind {
    /// The discriminant a shader switches on: the same numbers the enum
    /// declares, spelled once more here because reading them back needs a
    /// cast, and `tests/instance.rs` pins that the two agree.
    pub const fn code(self) -> Code {
        Code(match self {
            Kind::Mask => 0,
            Kind::Color => 1,
            Kind::Solid => 2,
        })
    }
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

/// One band: a strip of the sheet `h` texels tall starting at row `y`,
/// carved into `cap` slots of `w` texels each, of which `used` have been
/// handed out. Every slot in a band is the same size, which is what makes
/// a slot's texel arithmetic rather than a search.
#[derive(Clone, Copy)]
struct Band {
    y: Px,
    w: Px,
    h: Px,
    used: SlotIx,
    cap: SlotIx,
}

/// A band's place in the sheet's stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct BandIx(usize);

/// A slot's place along its band. A band's slots are uniform, so this
/// and the band are the whole of a slot's address -- which is what makes
/// the texel arithmetic rather than a search.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct SlotIx(u32);

impl SlotIx {
    const FIRST: SlotIx = SlotIx(0);

    fn next(self) -> SlotIx {
        SlotIx(self.0.saturating_add(1))
    }

    /// The texel this slot starts at along a band of `w`-wide slots.
    fn along(self, w: Px) -> Option<Px> {
        Some(Px::of(self.0.checked_mul(w.value())?))
    }
}

/// A slot: which band, and which index within it.
#[derive(Clone, Copy)]
struct Spot {
    band: BandIx,
    ix: SlotIx,
}

/// The step a slot's width and height are rounded up to, in texels. It
/// bounds both the number of size classes and the waste: a slot is at
/// most one step wider and one step taller than the image in it.
const STEP: u32 = 8;

fn step_up(v: Px) -> Option<Px> {
    Some(Px::of(
        v.value().max(1).checked_add(STEP - 1)? / STEP * STEP,
    ))
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
    /// Every slot voided: [`Atlas::reset`], which a font size change asks
    /// for. What a cache of rectangles watches, and the only event that
    /// invalidates one.
    pub grown: Tally,
    /// A texture doubled with its rows copied across. Every rectangle
    /// survives, so nothing that cached one is told; the GPU still learns
    /// the texture itself was replaced, through [`Replaced`].
    pub enlarged: Tally,
}

/// One texture and the bands carved out of it.
struct Sheet {
    side: Side,
    bands: Vec<Band>,
    /// The first row no band has claimed.
    bump: Px,
}

impl Sheet {
    fn new(side: Side) -> Sheet {
        Sheet {
            side,
            bands: Vec::new(),
            bump: Px::of(0),
        }
    }

    /// The texel a slot at least `w` by `h` starts at, or `None` when the
    /// sheet is full and the caller has to grow it.
    fn take(&mut self, w: Px, h: Px) -> Option<(Px, Px)> {
        let (cw, ch) = (step_up(w)?, step_up(h)?);
        if cw.value() > self.side.value() || ch.value() > self.side.value() {
            return None;
        }
        let spot = self.claim(cw, ch)?;
        let b = self.bands.get(spot.band.0)?;
        Some((spot.ix.along(b.w)?, b.y))
    }

    /// A slot of this class: the first band of the class with room, else
    /// a fresh band off the bump pointer.
    fn claim(&mut self, cw: Px, ch: Px) -> Option<Spot> {
        for (i, b) in self.bands.iter_mut().enumerate() {
            if b.w.value() == cw.value() && b.h.value() == ch.value() && b.used < b.cap {
                let ix = b.used;
                b.used = b.used.next();
                return Some(Spot {
                    band: BandIx(i),
                    ix,
                });
            }
        }
        let after = self.bump.value().checked_add(ch.value())?;
        if after > self.side.value() {
            return None;
        }
        let cap = SlotIx(self.side.value() / cw.value());
        if cap == SlotIx::FIRST {
            return None;
        }
        let band = BandIx(self.bands.len());
        self.bands.push(Band {
            y: self.bump,
            w: cw,
            h: ch,
            used: SlotIx::FIRST.next(),
            cap,
        });
        self.bump = Px::of(after);
        Some(Spot {
            band,
            ix: SlotIx::FIRST,
        })
    }

    /// A wider sheet holds more slots in every band it already has, since
    /// a band's slots run along a row that just got longer.
    fn widened(&mut self, side: Side) {
        self.side = side;
        for b in &mut self.bands {
            b.cap = SlotIx(side.value() / b.w.value());
        }
    }
}

/// Counts every change to either sheet, so a copy of the atlas elsewhere
/// -- a GPU texture -- knows when it is stale.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Generation(u64);

/// A rectangle of texels on a sheet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TexRect {
    pub x: Tex,
    pub y: Tex,
    pub w: Tex,
    pub h: Tex,
}

impl TexRect {
    /// The smallest rectangle covering both.
    fn union(self, o: TexRect) -> TexRect {
        let x0 = self.x.0.min(o.x.0);
        let y0 = self.y.0.min(o.y.0);
        let x1 = (self.x.0 + self.w.0).max(o.x.0 + o.w.0);
        let y1 = (self.y.0 + self.h.0).max(o.y.0 + o.h.0);
        TexRect {
            x: Tex(x0),
            y: Tex(y0),
            w: Tex(x1 - x0),
            h: Tex(y1 - y0),
        }
    }
}

/// What a copy of the sheets elsewhere has to fetch since it last asked:
/// the texels written on each, or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Dirty {
    pub mask: Option<TexRect>,
    pub color: Option<TexRect>,
}

/// Whether a sheet was replaced -- grown and emptied -- since the last
/// [`Atlas::take_dirty`], so a copy has to be made afresh at the new side.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Replaced {
    #[default]
    No,
    Yes,
}

pub struct Atlas {
    generation: Generation,
    mask: Sheet,
    pixels: Vec<u8>,
    color: Sheet,
    argb: Vec<u32>,
    dirty: Dirty,
    mask_replaced: Replaced,
    color_replaced: Replaced,
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
            generation: Generation::default(),
            mask: Sheet::new(Side::FIRST),
            pixels: Vec::new(),
            color: Sheet::new(Side::FIRST),
            argb: Vec::new(),
            dirty: Dirty::default(),
            mask_replaced: Replaced::No,
            color_replaced: Replaced::No,
            map: HashMap::new(),
            scratch: Vec::new(),
            coverage: Vec::new(),
            raw: Vec::new(),
            rgba: Vec::new(),
            stats: AtlasStats::default(),
        };
        a.clear_mask(Side::FIRST);
        a.clear_color(Side::FIRST);
        a
    }

    fn clear_mask(&mut self, side: Side) {
        self.mask = Sheet::new(side);
        let n = slots(Px::of(side.0));
        self.pixels.clear();
        self.pixels.resize(n * n, 0);
        self.mask_replaced = Replaced::Yes;
        self.dirty.mask = None;
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

    fn clear_color(&mut self, side: Side) {
        self.color = Sheet::new(side);
        let n = slots(Px::of(side.0));
        self.argb.clear();
        self.argb.resize(n * n, 0);
        self.color_replaced = Replaced::Yes;
        self.dirty.color = None;
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

    /// A bigger mask texture with every slot still where it was. The old
    /// rows are copied into the wider sheet at the same coordinates, so
    /// every rectangle already handed out addresses the same texels and
    /// the glyph map keeps every entry it holds. Only the texture is
    /// replaced, which the GPU learns from [`Replaced`].
    fn grow_mask(&mut self, side: Side) -> Option<()> {
        let (old, new) = (slots(Px::of(self.mask.side.0)), slots(Px::of(side.0)));
        let mut next = vec![0u8; new.checked_mul(new)?];
        for row in 0..old {
            let (from, to) = (row.checked_mul(old)?, row.checked_mul(new)?);
            next.get_mut(to..to.checked_add(old)?)?
                .copy_from_slice(self.pixels.get(from..from.checked_add(old)?)?);
        }
        self.pixels = next;
        self.mask.widened(side);
        self.mask_replaced = Replaced::Yes;
        self.dirty.mask = None;
        self.stats.enlarged.bump();
        Some(())
    }

    /// The colour sheet's counterpart, texel for texel the same move.
    fn grow_color(&mut self, side: Side) -> Option<()> {
        let (old, new) = (slots(Px::of(self.color.side.0)), slots(Px::of(side.0)));
        let mut next = vec![0u32; new.checked_mul(new)?];
        for row in 0..old {
            let (from, to) = (row.checked_mul(old)?, row.checked_mul(new)?);
            next.get_mut(to..to.checked_add(old)?)?
                .copy_from_slice(self.argb.get(from..from.checked_add(old)?)?);
        }
        self.argb = next;
        self.color.widened(side);
        self.color_replaced = Replaced::Yes;
        self.dirty.color = None;
        self.stats.enlarged.bump();
        Some(())
    }

    /// Which version of the sheets this is; every placement and reset
    /// moves it.
    /// Start over at the first side: every slot is gone, the sheets are
    /// replaced whole, and the grow count moves so the glyph map and the
    /// overlay rebuild what they kept. What a change of font size needs.
    pub fn reset(&mut self) {
        // **At the side the session has reached, not back at the first.**
        // Every bitmap is at the wrong pixel size after a rescale, so every
        // slot has to go -- but how much room this terminal's repertoire
        // needs is a fact the session learned, and a new font size needs
        // about the same order of it. Starting at `Side::FIRST` guarantees
        // growing back through every doubling, re-rastering and re-uploading
        // the whole sheet at each one, on every step of a zoom.
        let (mask, color) = (self.mask.side, self.color.side);
        self.map.clear();
        self.clear_mask(mask);
        self.clear_color(color);
        self.generation = Generation(self.generation.0.wrapping_add(1));
        self.stats.grown.bump();
    }

    pub fn generation(&self) -> Generation {
        self.generation
    }

    fn touched(&mut self) {
        self.generation = Generation(self.generation.0.wrapping_add(1));
    }

    /// The texels written since the last call, and whether either sheet
    /// was replaced since -- in which case its rectangle is the whole new
    /// sheet's worth of texels a copy has to fetch. Clears both.
    pub fn take_dirty(&mut self) -> (Dirty, Replaced, Replaced) {
        let out = (
            self.dirty,
            core::mem::take(&mut self.mask_replaced),
            core::mem::take(&mut self.color_replaced),
        );
        self.dirty = Dirty::default();
        out
    }

    fn dirtied(slot: &mut Option<TexRect>, r: TexRect) {
        *slot = Some(slot.map_or(r, |d| d.union(r)));
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
            if let Some(at) = self.color.take(w, h) {
                break at;
            }
            let next = Side(self.color.side.0.checked_mul(2)?);
            if next.0 > Side::LIMIT.0 {
                return None;
            }
            self.grow_color(next)?;
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
        self.touched();
        let rect = TexRect {
            x: Tex(u16::try_from(x.value()).ok()?),
            y: Tex(u16::try_from(y.value()).ok()?),
            w: Tex(u16::try_from(w.value()).ok()?),
            h: Tex(u16::try_from(h.value()).ok()?),
        };
        Atlas::dirtied(&mut self.dirty.color, rect);
        Some(Slot {
            kind: Kind::Color,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
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
            if let Some(at) = self.mask.take(w, h) {
                break at;
            }
            let next = Side(self.mask.side.0.checked_mul(2)?);
            if next.0 > Side::LIMIT.0 {
                return None;
            }
            self.grow_mask(next)?;
        };
        self.blit(x, y, w, h);
        self.touched();
        let rect = TexRect {
            x: Tex(u16::try_from(x.value()).ok()?),
            y: Tex(u16::try_from(y.value()).ok()?),
            w: Tex(u16::try_from(w.value()).ok()?),
            h: Tex(u16::try_from(h.value()).ok()?),
        };
        Atlas::dirtied(&mut self.dirty.mask, rect);
        Some(Slot {
            kind: Kind::Mask,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A slot's texel is its index times its band's width, on its band's
    /// row. It is arithmetic, not a search, and it does not depend on
    /// what was allocated before it.
    #[test]
    fn a_slot_is_its_index_times_its_width_on_its_band() {
        let mut sheet = Sheet::new(Side::FIRST);
        let mut at = || {
            let (x, y) = sheet
                .take(Px::of(10), Px::of(20))
                .expect("a fresh sheet has room");
            (x.value(), y.value())
        };
        // Ten by twenty rounds up to the sixteen by twenty-four class.
        assert_eq!(at(), (0, 0));
        assert_eq!(at(), (16, 0));
        assert_eq!(at(), (32, 0));
    }

    /// A second class opens its own band, below the first and not
    /// overlapping it.
    #[test]
    fn a_second_class_takes_a_band_of_its_own() {
        let mut sheet = Sheet::new(Side::FIRST);
        let (_, first) = sheet.take(Px::of(10), Px::of(20)).expect("room");
        let (x, y) = sheet.take(Px::of(10), Px::of(40)).expect("room");
        assert_eq!(first.value(), 0);
        assert_eq!(x.value(), 0);
        assert_eq!(
            y.value(),
            24,
            "below the twenty-four tall band the first class opened"
        );
    }

    /// **The whole reason the bands are here.** A sheet that fills grows
    /// and keeps every rectangle it has already handed out, texel for
    /// texel, so nothing that cached one is invalidated. `grown` -- what
    /// the glyph map watches -- does not move, and only `enlarged` does.
    /// The skyline packer this replaced answered a full sheet by
    /// discarding every glyph on it, mid frame, while the GPU sampled the
    /// texture it rewrote.
    #[test]
    fn growing_keeps_every_rectangle_and_every_texel() {
        let mut a = Atlas::new();
        let (x, y) = a
            .mask
            .take(Px::of(10), Px::of(20))
            .expect("room on a fresh sheet");
        let narrow = slots(Px::of(a.mask.side.0));
        let was = slots(y) * narrow + slots(x);
        a.pixels[was] = 0xAB;
        a.pixels[was + 1] = 0xCD;
        let before = a.stats;

        let twice = Side(a.mask.side.0 * 2);
        a.grow_mask(twice).expect("the sheet doubles");

        let wide = slots(Px::of(a.mask.side.0));
        let now = slots(y) * wide + slots(x);
        assert_eq!(a.pixels[now], 0xAB, "the texel is where it was");
        assert_eq!(a.pixels[now + 1], 0xCD);
        assert_eq!(
            a.stats.grown.value(),
            before.grown.value(),
            "growing voids no slot, so nothing that cached one is told"
        );
        assert_eq!(a.stats.enlarged.value(), before.enlarged.value() + 1);
    }

    /// And a font size change still voids everything, which is what
    /// `grown` means now. The pair tells the two events apart, so a cache
    /// can watch the one that concerns it rather than both.
    #[test]
    fn a_reset_voids_what_growing_keeps() {
        let mut a = Atlas::new();
        let before = a.stats;
        a.reset();
        assert_eq!(a.stats.grown.value(), before.grown.value() + 1);
        assert_eq!(a.stats.enlarged.value(), before.enlarged.value());
    }
}
