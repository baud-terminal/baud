//! The glyph atlas: one `A8` texture of coverage masks and one `ARGB`
//! texture of colour glyphs, each carved into bands of uniform slots,
//! grow-only, keyed by face and glyph.
//!
//! A glyph is resolved the first time it is asked for and never again
//! while the atlas lives; a size or font change starts a new atlas. A
//! face's bitmap strike (`sbix`, `CBDT`) is tried first, then its `COLR`
//! layers painted through palette zero, then the outline as a mask -- the
//! order Apple Color Emoji, Noto Color Emoji and Bungee Tint each need.
//!
//! **A strike is chosen per glyph, among the strikes that hold it**: the
//! smallest at or above the cell's size rounded half up to a whole pixel,
//! else the largest, skipping any where the glyph is empty or its PNG will
//! not decode. The specifications require a glyph with an image in any
//! strike be drawn from one (`[[source: ms-opentype-sbix]]`,
//! `[[source: apple-ttrm-sbix]]`), and CoreText was measured taking this
//! order on Apple Color Emoji for the square it draws, whose strikes leave
//! 177 glyphs empty in some sizes and not others: the melting face is empty
//! at 40, 48 and 52 ppem and CoreText draws it from 64 there. That square
//! is the size from 24 px up, and on a copy of the face opened from its
//! bytes; below 24 px CoreText enlarges the system font's square and takes
//! a larger strike, which baud does not follow.
//! One strike a face, the rule this replaced, drew those glyphs as nothing,
//! 99 of them at the app's default 26 px.
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
use baud_domain::{Chan, Codepoint, GlyphId};
use baud_font::bytes::Index;
use baud_font::{
    Bounds, Canvas, Drawn, Face, Fit, Mirror, Palette, PaletteEntry, Phase, Png, Ppem, Px,
    Rasterizer, Scale, Sink, Strike, VarPoint,
};
use std::collections::HashMap;

/// The most scratch the atlas grows to for one outline, in points: a
/// memory budget, set at HarfBuzz's `HB_GLYF_MAX_POINTS`
/// (`[[source: harfbuzz-12-3-0-limits]]`), past which HarfBuzz refuses a
/// glyph whose flattened points exceed it. The need also counts the
/// component stack, so a glyph within HarfBuzz's bound and this close to
/// it can be refused here and not there; about 5.6 MB at 28 bytes a point.
pub const VAR_POINTS_MAX: u32 = 200_000;

/// `id`'s outline at the face's coordinates into `sink`. A glyph that
/// needs scratch -- a face read at a variation draws only through
/// [`Face::outline_at`], and so does a composite that places a component
/// by matched points, at any instance -- answers [`Drawn::NeedsScratch`]
/// with nothing drawn, and `var` grows to exactly that need and the glyph
/// is drawn again, unless the need is past [`VAR_POINTS_MAX`]: then the
/// refusal is the answer, for the caller to count. A glyph the face
/// refuses outright answers [`Drawn::Refused`], for the caller to count as
/// well and to draw the face's `.notdef` for (`Atlas::slot`). Any other
/// glyph of a face at its default instance never asks.
fn outline<S: Sink>(var: &mut Vec<VarPoint>, face: &Face<'_>, id: GlyphId, sink: &mut S) -> Drawn {
    match face.outline_at(id, var, sink) {
        Drawn::NeedsScratch(need) if need.value() <= VAR_POINTS_MAX => {
            var.resize(
                usize::try_from(need.value()).unwrap_or(usize::MAX),
                VarPoint::EMPTY,
            );
            face.outline_at(id, var, sink)
        }
        drawn => drawn,
    }
}

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
    /// The cursor trail's quad: the record is its bounds, and the pixels
    /// inside the quad are covered, anti-aliased at its edges and bare in
    /// the cursor's own rectangle (`trail::coverage`). Drawn by its own
    /// fragment function, `quad_fragment`, which reads the corners.
    Quad = 3,
    /// A band of an effect trail's reach: the record covers pixels the
    /// effect's fragment function may draw in, and the effect itself comes
    /// from the uniform it reads (`effect::Vfx`).
    Band = 4,
    /// A particle of an effect trail, the fire's embers among them: the
    /// record is its bounds, and `glyph_vertex` hands the effect's fragment
    /// function where in it a pixel is, -1 to 1 on each axis.
    Particle = 5,
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
            Kind::Quad => 3,
            Kind::Band => 4,
            Kind::Particle => 5,
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
    /// Where the glyph sits in its span: the grid phase for one cell, or
    /// the cluster's centring in two. One glyph is drawn at each phase it
    /// is asked at -- a one-cell text glyph in a two-cell span, a cluster's
    /// mark at its base's offset.
    phase: Phase,
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
    /// Procedural masks drawn: box drawing, block elements and braille,
    /// once per codepoint per size.
    pub synthesised: Tally,
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
    /// Glyphs whose outline needed more scratch than [`VAR_POINTS_MAX`],
    /// drawn as nothing, or that the face refuses ([`Drawn::Refused`]: a
    /// composite the `glyf` text says cannot be), drawn as the face's
    /// `.notdef`: refused where a reader sees it rather than taken for a
    /// glyph with no ink. Each is counted once when its answer is recorded;
    /// a refused glyph whose `.notdef` does not fit even after growing is
    /// not recorded, and is counted again on every ask. A `COLR` glyph
    /// refused for a layer falls to its base glyph's own outline, as a glyph
    /// with no `COLR` record does, and is counted twice when that outline is
    /// refused too, for either reason, once for each. A refused glyph the
    /// stack passed over for another face, one that is not colour, is never
    /// asked for here; `FaceTable::refused` counts it.
    pub refused: Tally,
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
    /// The procedural glyphs -- box drawing, block elements, braille --
    /// by codepoint, drawn once per size the way a font glyph is; a cell
    /// mask is a function of the cell, so `reset` voids it with `map`.
    procedural: HashMap<u32, Option<Slot>>,
    scratch: Vec<f32>,
    /// The points of a varied outline ([`outline`]), grown to the largest
    /// need asked so far and kept.
    var: Vec<VarPoint>,
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
            procedural: HashMap::new(),
            scratch: Vec::new(),
            var: Vec::new(),
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
        self.procedural.clear();
        self.clear_mask(mask);
        self.clear_color(color);
        self.generation = Generation(self.generation.0.wrapping_add(1));
        self.stats.grown.bump();
    }

    /// Which version of the sheets this is; every placement and reset
    /// moves it.
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
    /// from a strike that holds it (see [`Atlas::bitmap`]), else its `COLR`
    /// layers, else its outline as
    /// an exact-coverage mask. `None` for a glyph that draws nothing; a
    /// glyph the face refuses draws the face's `.notdef` in its place. One
    /// raster a glyph and phase: the text-contrast curve is applied when the
    /// mask is composited (`baud_domain::contrast`), not here.
    pub fn slot(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
    ) -> Option<Slot> {
        let glyph = GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX));
        let key = GlyphKey {
            face: ix,
            glyph,
            phase,
        };
        if let Some(known) = self.map.get(&key) {
            return *known;
        }
        if let Some(s) = self
            .bitmap(face, id, scale, phase)
            .or_else(|| self.layered(face, id, scale, phase))
        {
            self.map.insert(key, Some(s));
            return Some(s);
        }
        let slot = self.rasterise(face, id, scale, phase);
        if slot.is_none() && id != GlyphId::NOTDEF && face.refuses(id) {
            // A glyph the face refuses ([`Drawn::Refused`]), counted by
            // `rasterise`, shows the face's `.notdef` rather than a blank:
            // TUS § 5.3 leaves a character a face cannot render "some
            // placeholder". The stack passes a refused glyph that is not
            // colour over for the next face that carries the character, so
            // the face here is the primary when such a glyph came from
            // `cmap`; a colour glyph is kept on its face and reaches here
            // only when its strike image and its layers drew nothing. Recorded
            // when the `.notdef`'s own answer is, so it is counted once
            // then; a `.notdef` that does not fit even after growing is not
            // recorded, so neither is this glyph, which every later ask
            // draws and counts again.
            let notdef = self.slot(face, ix, GlyphId::NOTDEF, scale, phase);
            let own = GlyphKey {
                face: ix,
                glyph: GlyphIx(0),
                phase,
            };
            if self.map.contains_key(&own) {
                self.map.insert(key, notdef);
            }
            return notdef;
        }
        // An image that would not fit even after growing is not recorded,
        // so a later, larger atlas may take it.
        if slot.is_some() || self.fits_nothing_anyway(face, id, scale, phase) {
            self.map.insert(key, slot);
        }
        slot
    }

    /// The glyph's image from a strike that holds it: among the strikes
    /// that hold it and decode, the smallest at or above the cell's size
    /// rounded half up, else the largest -- CoreText's order for the square
    /// it draws, which below 24 px on the system font is larger than the
    /// size and is not followed here -- then mirrored if a
    /// `'flip'` said so, and scaled to fit. A bitmap cannot be drawn at a
    /// fraction of a pixel, so only `phase`'s whole pixels move it.
    fn bitmap(&mut self, face: &Face<'_>, id: GlyphId, scale: Scale, phase: Phase) -> Option<Slot> {
        let strikes = face.strikes();
        if strikes.is_zero() {
            return None;
        }
        let target = scale.px(f32::from(face.units_per_em().value()));
        // CoreText compares a strike with the side of the square it draws,
        // the size rounded half up: 40.5 px takes 48, 40.4 takes 40. The
        // target comes back through the scale in f32, which turns 26.5 px on
        // an 800-unit em into 26.499998; snapped to 1/1024 px first, a half
        // stays a half.
        let whole = ((target * 1024.0).round() / 1024.0 + 0.5).floor();
        let below = |p: Ppem| f32::from(p.value()) < whole;
        let mut order: Vec<(Ppem, Index)> = Vec::new();
        let mut i = Index::ZERO;
        while strikes.holds(i) {
            if let Some(p) = face.strike_ppem(i) {
                order.push((p, i));
            }
            i = i.next();
        }
        // At or above first, smallest first; then below, largest first.
        order.sort_by(|a, b| {
            let (x, y) = (below(a.0), below(b.0));
            x.cmp(&y)
                .then(if x { b.0.cmp(&a.0) } else { a.0.cmp(&b.0) })
        });
        let (bitmap, size) = order.iter().find_map(|&(_, strike)| {
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
            Some((bitmap, size))
        })?;
        if bitmap.mirror == Mirror::LeftRight {
            // Left to right within the image's own width, before scaling:
            // the image the font names, then drawn as any other. A row is
            // never empty: the decoder refuses a zero side (PNG 11.2.2).
            let w = slots(Px::of(size.width));
            for row in self.rgba[..w * slots(Px::of(size.height)) * 4].chunks_exact_mut(w * 4) {
                for x in 0..w / 2 {
                    for c in 0..4 {
                        row.swap(x * 4 + c, (w - 1 - x) * 4 + c);
                    }
                }
            }
        }
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
        self.place_color(
            &scaled,
            Px::of(dw),
            Px::of(dh),
            Origin {
                left: left + phase.whole(),
                top,
            },
        )
    }

    /// `COLR` v0: every layer's outline painted in its palette colour into
    /// one image, bottom layer first.
    fn layered(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
    ) -> Option<Slot> {
        let layers = face.layers(id)?;
        // The union box of every layer decides the image. A layer refused
        // refuses the glyph, counted once, rather than paint it partly.
        let mut b = Bounds::new();
        for layer in layers {
            if let Drawn::NeedsScratch(_) | Drawn::Refused =
                outline(&mut self.var, face, layer.glyph, &mut b)
            {
                self.stats.refused.bump();
                return None;
            }
        }
        let fit = Fit::at(&b, scale, phase)?;
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
            // The scratch the bounds pass grew answers this pass too.
            outline(&mut self.var, face, layer.glyph, &mut r);
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

    /// Whether `id` has no ink at all, so `None` is its permanent answer --
    /// or was refused, which [`Atlas::rasterise`] has already counted, and
    /// whose answer is as permanent: the budget it was refused against
    /// does not change.
    fn fits_nothing_anyway(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
    ) -> bool {
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(_) | Drawn::Refused = outline(&mut self.var, face, id, &mut b) {
            return true;
        }
        Fit::at(&b, scale, phase).is_none()
    }

    fn rasterise(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
    ) -> Option<Slot> {
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(_) | Drawn::Refused = outline(&mut self.var, face, id, &mut b) {
            self.stats.refused.bump();
            return None;
        }
        let fit = Fit::at(&b, scale, phase)?;
        let cells = fit.extent.cells()?;
        if self.coverage.len() < cells {
            self.coverage.resize(cells, 0);
        }
        let need = fit.extent.scratch()?;
        if self.scratch.len() < need {
            self.scratch.resize(need, 0.0);
        }
        let canvas = Canvas::new(fit.extent, &mut self.scratch)?;
        let mut r = Rasterizer::new(canvas, fit.transform);
        outline(&mut self.var, face, id, &mut r);
        r.finish().coverage(&mut self.coverage[..cells])?;
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

    /// The mask for a procedural codepoint -- box drawing, a block element,
    /// braille -- drawn at exactly `cell` and placed at the cell origin:
    /// `left` is zero and `top` is `-baseline`, so every path's placement
    /// formula lands it on the cell whatever face the row is on. `None`
    /// for a codepoint a font draws. Memoised per codepoint until `reset`,
    /// because the mask is a function of the cell and the cell changes
    /// only with the size.
    pub fn synthetic(
        &mut self,
        cp: Codepoint,
        cell: crate::boxes::Cell,
        baseline: Px,
    ) -> Option<Slot> {
        if let Some(slot) = self.procedural.get(&cp.value()) {
            return *slot;
        }
        let slot = self.synthesise(cp, cell, baseline);
        if slot.is_some() || !crate::boxes::covers(cp) {
            self.procedural.insert(cp.value(), slot);
        }
        slot
    }

    fn synthesise(
        &mut self,
        cp: Codepoint,
        cell: crate::boxes::Cell,
        baseline: Px,
    ) -> Option<Slot> {
        let extent = crate::boxes::draw(cp, cell, &mut self.scratch, &mut self.coverage)?;
        self.stats.synthesised.bump();
        let (w, h) = (extent.w, extent.h);
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
            left: 0,
            top: -i32::try_from(baseline.value()).unwrap_or(0),
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
