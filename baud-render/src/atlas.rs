//! The glyph atlas: one `A8` texture of coverage masks and one `ARGB`
//! texture of colour glyphs, each carved into bands of uniform slots,
//! grow-only, keyed by face and glyph at each pixel size.
//!
//! A glyph is resolved the first time it is asked for at a size and never
//! again while the atlas holds that size: what was drawn at each size held
//! since the atlas last started over, at most [`SIZES`], stays on the
//! sheets, so a zoom back to one of them draws nothing ([`Atlas::at`]), and
//! a change of faces starts a new atlas. A face's bitmap strike (`sbix`,
//! `CBDT`) is tried first, then its `COLR` layers painted through palette
//! zero, then the outline as a mask -- the order Apple Color Emoji, Noto
//! Color Emoji and Bungee Tint each need.
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
//! which a change of faces asks for deliberately, as does a size past
//! [`SIZES`].
//!
//! Bands also admit per-slot eviction, since a freed slot is an index its
//! class can hand out again rather than a hole a packer would have to
//! repack around. Nothing frees a slot yet, so there is no free list to
//! consult: adding one before something fills it would be a mechanism
//! that cannot fire.

use crate::color::Argb;
use crate::stack::{FaceIx, Lean};
use crate::units::{Coverage, Tally, slots};
use baud_domain::{Chan, Codepoint, GlyphId};
use baud_font::bytes::Index;
use baud_font::{
    Bounds, Canvas, Drawn, Extent, Face, Fit, Mirror, Palette, PaletteEntry, Phase, Png, Ppem, Px,
    Rasterizer, Reach, Scale, Sink, Strike, Stroker, VarPoint,
};
use std::collections::HashMap;

/// The most scratch the atlas grows to for one outline, in points: a
/// memory budget, set at HarfBuzz's `HB_GLYF_MAX_POINTS`
/// (`[[source: harfbuzz-12-3-0-limits]]`), past which HarfBuzz refuses a
/// glyph whose flattened points exceed it. The need also counts the
/// component stack, so a glyph within HarfBuzz's bound and this close to
/// it can be refused here and not there; 5.6 MB at 28 bytes a point.
///
/// **A bound, because the walk's own bounds admit 3.5 GiB.** A walk
/// visits at most 2,049 glyphs (`glyf`'s edge bound), each simple one at
/// most 65,536 points, so one composite of 2,048 copies of a full simple
/// glyph -- a 210 KB font, measured -- needs 134,219,784 points: 3.76 GB
/// of scratch, held for the atlas's life. A glyph past the bound is not
/// a blank: the stack passes it over for the next face that carries its
/// character (`Stack`'s `drawing`), else the atlas draws the face's
/// `.notdef` in its place (`Atlas::slot`).
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
pub(crate) fn outline<S: Sink>(
    var: &mut Vec<VarPoint>,
    face: &Face<'_>,
    id: GlyphId,
    sink: &mut S,
) -> Drawn {
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

/// A scale's pixels a unit, its bits, hashable: the key a glyph drawn at
/// a size of its own is kept under.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ScaleKey(u32);

/// A glyph's index in its face, hashable.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphIx(u16);

/// How a glyph is drawn for the line it is on: at its size, or for a line
/// that is not single-width two cells wide, and for either half of a
/// double-height line two cells tall as well (`[[source: dec-std-070]]` §
/// 5.6.1). Rasterised from the outline at that size, not scaled from the
/// single mask, rather than repeating pixels or filtering linearly.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stretch {
    One,
    Wide,
    Tall,
}

impl Stretch {
    /// The stretch a line of rendition `r` draws its characters at.
    pub fn of(r: baud_domain::LineRendition) -> Stretch {
        match r {
            baud_domain::LineRendition::Single => Stretch::One,
            baud_domain::LineRendition::DoubleWidth => Stretch::Wide,
            baud_domain::LineRendition::DoubleTop | baud_domain::LineRendition::DoubleBottom => {
                Stretch::Tall
            }
        }
    }
    /// How many times as wide a character is drawn.
    pub fn x(self) -> i32 {
        match self {
            Stretch::One => 1,
            Stretch::Wide | Stretch::Tall => 2,
        }
    }
    /// How many times as tall.
    pub fn y(self) -> i32 {
        match self {
            Stretch::One | Stretch::Wide => 1,
            Stretch::Tall => 2,
        }
    }
    fn xf(self) -> f32 {
        f32::from(i16::try_from(self.x()).unwrap_or(1))
    }
    fn yf(self) -> f32 {
        f32::from(i16::try_from(self.y()).unwrap_or(1))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: FaceIx,
    glyph: GlyphIx,
    /// The size it is drawn at for its line: one key a stretch.
    stretch: Stretch,
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
    /// Every slot voided, by [`Atlas::reset`], or every slot handed out
    /// naming another size's image, by [`Atlas::at`]: either way what a
    /// cache of rectangles watches, and the only events that invalidate
    /// one.
    pub grown: Tally,
    /// A texture doubled with its rows copied across. Every rectangle
    /// survives, so nothing that cached one is told; the GPU still learns
    /// the texture itself was replaced, through [`Replaced`].
    pub enlarged: Tally,
    /// Glyphs whose outline needed more scratch than [`VAR_POINTS_MAX`],
    /// or that the face refuses ([`Drawn::Refused`]: a composite the `glyf`
    /// text says cannot be), drawn as the face's `.notdef`: refused where a
    /// reader sees it rather than taken for a glyph with no ink. Each is
    /// counted once when its answer is recorded; a refused glyph whose
    /// `.notdef` does not fit even after growing is not recorded, and is
    /// counted again on every ask. A `COLR` glyph
    /// refused for a layer falls to its base glyph's own outline, as a glyph
    /// with no `COLR` record does, and is counted twice when that outline is
    /// refused too, for either reason, once for each, and then draws the
    /// `.notdef` too. A glyph the stack passed over for another face is
    /// never asked for here; `FaceTable::refused` counts it.
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

/// **How many pixel sizes the atlas holds its glyphs at.** A slot is never
/// freed, so the sizes are bounded rather than evicted one at a time: a
/// rescale to a size past these starts the atlas over, as the badge's
/// third size does (`BADGE_SIZES`), and holds only the new one.
pub const SIZES: usize = 4;

/// A pixel size the atlas holds glyphs at: the size's bits, so two sizes
/// are one key exactly when they are the same `f32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeKey(u32);

impl SizeKey {
    /// No size yet: an atlas made by [`Atlas::new`] before
    /// [`Atlas::at`] names one.
    const NONE: SizeKey = SizeKey(u32::MAX);

    pub fn of(size: crate::PxSize) -> SizeKey {
        SizeKey(size.value().to_bits())
    }
}

/// What [`Atlas::at`] did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rescaled {
    /// The size was the one drawn at already.
    Same,
    /// The size was held, and its glyphs are drawn again from the sheets.
    Kept,
    /// A size not held, beside the others: its glyphs are drawn as asked.
    Added,
    /// A size past [`SIZES`], or the first one named over glyphs drawn
    /// under none: the atlas started over, as [`Atlas::reset`] does, and
    /// holds only this one.
    Restarted,
}

/// The glyphs drawn at one pixel size, by what they were asked for under:
/// every image here is a function of the size, so a size has its own.
struct Memo {
    size: SizeKey,
    /// `None` records a glyph with no ink, so a space is looked up once.
    map: HashMap<GlyphKey, Option<Slot>>,
    /// The procedural glyphs -- box drawing, block elements, braille --
    /// by codepoint, drawn once per size the way a font glyph is: a cell
    /// mask is a function of the cell, which is a function of the size.
    procedural: HashMap<u32, Option<Slot>>,
    /// The same for a line not single-width: drawn at the doubled cell,
    /// by codepoint, or a ring part's key, and stretch.
    procedural_stretched: HashMap<(u32, Stretch), Option<Slot>>,
    /// **The procedural glyphs of a character two cells wide**, by
    /// codepoint and stretch: a mask across both cells
    /// ([`crate::boxes::draw_across`]). A map of its own, so a one-cell
    /// glyph is looked up by the key it always was.
    procedural_wide: HashMap<(u32, Stretch), Option<Slot>>,
    /// **The same mirrored**, as `?2500` draws a box drawing character
    /// two cells wide right to left ([`crate::boxes::Facing`]): a map of
    /// its own, so the character as written keeps its key.
    procedural_mirrored: HashMap<(u32, Stretch), Option<Slot>>,
    /// **The glyphs drawn flipped left to right**, under the key their
    /// unflipped image is kept by: a Bidi_Mirrored character with no
    /// Bidi_Mirroring_Glyph resolved right to left (UAX #9 L4,
    /// [`Atlas::slot_flipped_at`]). A map of its own, so the key every other
    /// glyph is looked up by is no wider for it.
    flipped: HashMap<GlyphKey, Option<Slot>>,
    /// **The glyphs drawn sheared left**, flipped first or not, by the
    /// lean's bits: an italic SAPV 17 slants left ([`Atlas::slot_sheared_at`]).
    sheared: HashMap<(GlyphKey, Flip, u32), Option<Slot>>,
}

/// Whether a glyph's image is flipped left to right: a mirrored
/// character's that L4 flips (`Atlas::slot_flipped_at`), before a shear,
/// and an input method's cluster drawn as the cell it is written in is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Flip {
    Kept,
    Flipped,
}

impl Memo {
    fn new(size: SizeKey) -> Memo {
        Memo {
            size,
            map: HashMap::new(),
            procedural: HashMap::new(),
            procedural_stretched: HashMap::new(),
            procedural_wide: HashMap::new(),
            procedural_mirrored: HashMap::new(),
            flipped: HashMap::new(),
            sheared: HashMap::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.map.is_empty()
            && self.procedural.is_empty()
            && self.procedural_stretched.is_empty()
            && self.procedural_wide.is_empty()
            && self.procedural_mirrored.is_empty()
            && self.flipped.is_empty()
            && self.sheared.is_empty()
    }
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
    /// What has been drawn at the size the cells are drawn at now.
    held: Memo,
    /// What was drawn at the other sizes held, at most [`SIZES`] less one,
    /// their slots still on the sheets: a rescale back to one of them
    /// takes its memo up again and draws nothing.
    kept: Vec<Memo>,
    /// Glyphs drawn at a size of their own, the badge's, by face, glyph and
    /// scale -- the scale is in the key, so they stand at every cell size --
    /// and voided with the rest.
    sized: HashMap<(FaceIx, GlyphIx, ScaleKey), Option<Slot>>,
    /// The strokes of glyphs drawn at a size of their own, the badge's
    /// outline, by face, glyph, scale and the stroke's width, voided with
    /// the rest.
    stroked: HashMap<(FaceIx, GlyphIx, ScaleKey, ScaleKey), Option<Slot>>,
    /// Colour glyphs drawn at a size of their own inside an alpha, the
    /// badge's, by face, glyph, scale and the alpha, voided with the rest.
    faded: HashMap<(FaceIx, GlyphIx, ScaleKey, u8), Option<Slot>>,
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
            held: Memo::new(SizeKey::NONE),
            kept: Vec::new(),
            sized: HashMap::new(),
            stroked: HashMap::new(),
            faded: HashMap::new(),
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
        let other = |s: &Option<Slot>| {
            !matches!(
                s,
                Some(Slot {
                    kind: Kind::Mask,
                    ..
                })
            )
        };
        for memo in core::iter::once(&mut self.held).chain(&mut self.kept) {
            memo.map.retain(|_, s| other(s));
        }
    }

    fn clear_color(&mut self, side: Side) {
        self.color = Sheet::new(side);
        let n = slots(Px::of(side.0));
        self.argb.clear();
        self.argb.resize(n * n, 0);
        self.color_replaced = Replaced::Yes;
        self.dirty.color = None;
        let other = |s: &Option<Slot>| {
            !matches!(
                s,
                Some(Slot {
                    kind: Kind::Color,
                    ..
                })
            )
        };
        for memo in core::iter::once(&mut self.held).chain(&mut self.kept) {
            memo.map.retain(|_, s| other(s));
        }
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
    /// overlay rebuild what they kept. What a change of faces needs, and a
    /// size past [`SIZES`].
    pub fn reset(&mut self) {
        // **At the side the session has reached, not back at the first.**
        // Every bitmap is of faces or sizes no longer drawn, so every slot
        // has to go -- but how much room this terminal's repertoire needs
        // is a fact the session learned, and new faces or a new size need
        // about the same order of it. Starting at `Side::FIRST` guarantees
        // growing back through every doubling, re-rastering and re-uploading
        // the whole sheet at each one, on every step of a zoom.
        let (mask, color) = (self.mask.side, self.color.side);
        self.held = Memo::new(self.held.size);
        self.kept.clear();
        self.sized.clear();
        self.stroked.clear();
        self.faded.clear();
        self.clear_mask(mask);
        self.clear_color(color);
        self.generation = Generation(self.generation.0.wrapping_add(1));
        self.stats.grown.bump();
    }

    /// **The cells are drawn at `size` from now on.** A size already held
    /// takes up the glyphs drawn at it, still on the sheets, so a zoom
    /// back rasterises nothing and neither sheet is replaced; a size not
    /// held starts a memo of its own beside the others, filled as glyphs
    /// are asked for; a size past [`SIZES`] starts the atlas over. Unless
    /// the size is the same, every rectangle handed out names another
    /// size's image, so `grown` and the generation move, and whatever
    /// cached one rebuilds -- as after a reset, but over sheets that still
    /// hold every size's glyphs. The exception is a size it does not keep,
    /// asked while nothing is drawn under the size it holds (a new atlas,
    /// one just reset, or one just moved to a size not drawn at yet): its
    /// memo is only renamed, and since no rectangle names that size,
    /// neither `grown` nor the generation moves.
    pub fn at(&mut self, size: SizeKey) -> Rescaled {
        if self.held.size == size {
            return Rescaled::Same;
        }
        if let Some(i) = self.kept.iter().position(|m| m.size == size) {
            self.switched();
            let back = self.kept.swap_remove(i);
            let was = core::mem::replace(&mut self.held, back);
            if !was.is_empty() {
                self.kept.push(was);
            }
            return Rescaled::Kept;
        }
        if self.held.is_empty() {
            // Nothing drawn under the size held -- a new atlas, or one just
            // reset -- so no rectangle names it, and it is this size's.
            self.held.size = size;
            return Rescaled::Added;
        }
        if self.held.size == SizeKey::NONE || self.kept.len() + 1 >= SIZES {
            // Glyphs drawn before any size was named cannot be kept under
            // one, and a size past the bound starts over.
            self.reset();
            self.held.size = size;
            return Rescaled::Restarted;
        }
        self.switched();
        let was = core::mem::replace(&mut self.held, Memo::new(size));
        self.kept.push(was);
        Rescaled::Added
    }

    /// Every rectangle handed out names another size's image now.
    fn switched(&mut self) {
        self.generation = Generation(self.generation.0.wrapping_add(1));
        self.stats.grown.bump();
    }

    /// Which version of the sheets this is; every placement, reset and
    /// change of size moves it.
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
        self.slot_at(face, ix, id, scale, phase, Stretch::One)
    }

    /// **A glyph at a size of its own**, `scale`: the badge's text, drawn
    /// at a size the cells' are not. Its image is the face's bitmap, its
    /// colour layers or its outline, as [`Atlas::slot`]'s; memoised apart
    /// from the cells' glyphs under the scale, and voided with them.
    pub fn slot_sized(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
    ) -> Option<Slot> {
        let glyph = GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX));
        let key = (ix, glyph, ScaleKey(scale.px(1.0).to_bits()));
        if let Some(known) = self.sized.get(&key) {
            return *known;
        }
        let (phase, stretch) = (Phase::ZERO, Stretch::One);
        let slot = self
            .bitmap(face, id, scale, phase, stretch)
            .or_else(|| self.layered(face, id, scale, phase, stretch))
            .or_else(|| self.rasterise(face, id, scale, phase, stretch));
        if slot.is_some() || self.fits_nothing_anyway(face, id, scale, phase, stretch) {
            self.sized.insert(key, slot);
        }
        slot
    }

    /// **A glyph's stroke at a size of its own**, `scale`, the line `width`
    /// pixels wide and centred on the outline, as CoreGraphics strokes a
    /// glyph drawn with a stroke width (`baud_font::Stroker`): the badge's
    /// outline. A mask, memoised beside [`Atlas::slot_sized`]'s glyphs and
    /// voided with them; `None` for a glyph with no outline -- a bitmap or
    /// colour glyph, which CoreGraphics strokes no more than baud does --
    /// or one the face refuses.
    pub fn slot_stroked(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        width: f32,
    ) -> Option<Slot> {
        let glyph = GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX));
        let key = (
            ix,
            glyph,
            ScaleKey(scale.px(1.0).to_bits()),
            ScaleKey(width.to_bits()),
        );
        if let Some(known) = self.stroked.get(&key) {
            return *known;
        }
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(_) | Drawn::Refused = outline(&mut self.var, face, id, &mut b) {
            return None;
        }
        let slot = self.stroke(face, id, scale, &b, width);
        if slot.is_some() || Fit::of(&b, scale).is_none() {
            self.stroked.insert(key, slot);
        }
        slot
    }

    /// **A colour glyph at a size of its own, drawn inside `alpha`**: each
    /// texel of [`Atlas::slot_sized`]'s image with its alpha so scaled,
    /// rounded, in a slot of its own -- the badge's colour glyph, which
    /// iTerm2 draws inside the badge's alpha. A mask, and a whole alpha,
    /// is the sized slot itself.
    pub fn slot_faded(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        alpha: Chan,
    ) -> Option<Slot> {
        let own = self.slot_sized(face, ix, id, scale)?;
        if own.kind != Kind::Color || alpha.get() == u8::MAX {
            return Some(own);
        }
        let glyph = GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX));
        let key = (ix, glyph, ScaleKey(scale.px(1.0).to_bits()), alpha.get());
        if let Some(known) = self.faded.get(&key) {
            return *known;
        }
        let (w, h) = (
            slots(Px::of(u32::from(own.w.0))),
            slots(Px::of(u32::from(own.h.0))),
        );
        let (x0, y0) = (
            slots(Px::of(u32::from(own.x.0))),
            slots(Px::of(u32::from(own.y.0))),
        );
        let side = slots(Px::of(self.color.side.0));
        let k = u32::from(alpha.get());
        let mut image = Vec::with_capacity(w * h);
        for row in 0..h {
            let from = (y0 + row) * side + x0;
            image.extend(self.argb.get(from..from + w)?.iter().map(|&p| {
                let a = (p >> 24) * k;
                ((a + 127) / 255) << 24 | (p & 0x00FF_FFFF)
            }));
        }
        let at = Origin {
            left: own.left,
            top: own.top,
        };
        let slot = self.place_color(
            &image,
            Px::of(u32::from(own.w.0)),
            Px::of(u32::from(own.h.0)),
            at,
        );
        if slot.is_some() {
            self.faded.insert(key, slot);
        }
        slot
    }

    /// The stroke of `id`, whose outline lies in `b`, rasterised: the fit
    /// grown to what the stroke reaches, measured by a first pass.
    fn stroke(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        b: &Bounds,
        width: f32,
    ) -> Option<Slot> {
        let fit = Fit::of(b, scale)?;
        let mut reach = Stroker::new(Reach::default(), fit.transform, width);
        outline(&mut self.var, face, id, &mut reach);
        let fit = fit.holding(reach.finish().reached?)?;
        let cells = fit.extent.cells()?;
        if self.coverage.len() < cells {
            self.coverage.resize(cells, 0);
        }
        let need = fit.extent.scratch()?;
        if self.scratch.len() < need {
            self.scratch.resize(need, 0.0);
        }
        let canvas = Canvas::new(fit.extent, &mut self.scratch)?;
        let mut st = Stroker::new(canvas, fit.transform, width);
        outline(&mut self.var, face, id, &mut st);
        st.finish().coverage(&mut self.coverage[..cells])?;
        self.stats.rasterised.bump();
        self.place_coverage(&fit)
    }

    /// [`Atlas::slot`] for a line that draws its characters at `stretch`:
    /// the glyph rasterised at that size, `phase` in its pixels.
    pub fn slot_at(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
        stretch: Stretch,
    ) -> Option<Slot> {
        let glyph = GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX));
        let key = GlyphKey {
            face: ix,
            glyph,
            stretch,
            phase,
        };
        if let Some(known) = self.held.map.get(&key) {
            return *known;
        }
        let before = self.stats.refused;
        if let Some(s) = self
            .bitmap(face, id, scale, phase, stretch)
            .or_else(|| self.layered(face, id, scale, phase, stretch))
        {
            self.held.map.insert(key, Some(s));
            return Some(s);
        }
        let slot = self.rasterise(face, id, scale, phase, stretch);
        if slot.is_none() && id != GlyphId::NOTDEF && self.stats.refused != before {
            // A glyph that drew nothing after a refusal -- its outline or a
            // `COLR` layer refused by the face ([`Drawn::Refused`]) or past
            // [`VAR_POINTS_MAX`], each counted where it was met -- shows the
            // face's `.notdef` rather than a blank: TUS § 5.3 leaves a
            // character a face cannot render "some placeholder". The stack
            // passes such a glyph over for the next face that carries the
            // character (`Stack`'s `drawing`), so the face here is the primary
            // when it came from `cmap` -- but for a `COLR` glyph whose layer
            // cannot draw and whose own outline, which the stack judges by its
            // `glyf` header alone (all of a face without `glyf` as having one),
            // draws nothing: that stays on its face, an open question. A `COLR`
            // glyph whose layer cannot draw draws its own outline when that has
            // ink, as a glyph with no `COLR` record does, and reaches here only
            // when it has none or is refused too. Recorded when the `.notdef`'s
            // own answer is, so it is counted once then; a `.notdef` that does
            // not fit even after growing is not recorded, so neither is this
            // glyph, which every later ask draws and counts again.
            let notdef = self.slot_at(face, ix, GlyphId::NOTDEF, scale, phase, stretch);
            let own = GlyphKey {
                face: ix,
                glyph: GlyphIx(0),
                stretch,
                phase,
            };
            if self.held.map.contains_key(&own) {
                self.held.map.insert(key, notdef);
            }
            return notdef;
        }
        // An image that would not fit even after growing is not recorded,
        // so a later, larger atlas may take it.
        if slot.is_some() || self.fits_nothing_anyway(face, id, scale, phase, stretch) {
            self.held.map.insert(key, slot);
        }
        slot
    }

    /// **[`Atlas::slot_at`]'s image flipped left to right**, for a
    /// Bidi_Mirrored character with no Bidi_Mirroring_Glyph resolved right
    /// to left, for which BidiMirroring.txt leaves it "up to the rendering
    /// system to provide mirrored glyphs" (`[[source:
    /// unicode-bidimirroring]]`). Each row of texels reversed, on the same
    /// sheet, at the same `left` and `top`: the caller places it mirrored in
    /// its span (`InSpan::mirrored` in the renderer), so its pixels are the
    /// unflipped glyph's mirrored about the span's centre, at any phase.
    /// Drawn once and kept, as an unflipped glyph is.
    pub fn slot_flipped_at(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
        stretch: Stretch,
    ) -> Option<Slot> {
        let key = GlyphKey {
            face: ix,
            glyph: GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX)),
            stretch,
            phase,
        };
        if let Some(known) = self.held.flipped.get(&key) {
            return *known;
        }
        let Some(own) = self.slot_at(face, ix, id, scale, phase, stretch) else {
            // Nothing to flip: recorded only where the unflipped answer is.
            if self.held.map.contains_key(&key) {
                self.held.flipped.insert(key, None);
            }
            return None;
        };
        let flipped = self.flip(own);
        if flipped.is_some() {
            self.held.flipped.insert(key, flipped);
        }
        flipped
    }

    /// **[`Atlas::slot_at`]'s image sheared left by `lean`**, flipped first
    /// with `flip`, for a cell SAPV 17 slants left: each row of texels
    /// moved left by `lean` of its centre's height above the baseline, and
    /// right as far below it, so the baseline stays where it was and the
    /// image leans about it. A row moves by a fraction of a texel as well,
    /// each texel shared between the two it falls across, as a box filter
    /// shares it. On the same sheet, `left` moved by the row moved
    /// furthest left and `top` kept: the caller places it as it places the
    /// upright image. Drawn once and kept, as an upright glyph is.
    #[allow(clippy::too_many_arguments)]
    pub fn slot_sheared_at(
        &mut self,
        face: &Face<'_>,
        ix: FaceIx,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
        stretch: Stretch,
        (flip, lean): (Flip, Lean),
    ) -> Option<Slot> {
        let glyph = GlyphKey {
            face: ix,
            glyph: GlyphIx(u16::try_from(id.index()).unwrap_or(u16::MAX)),
            stretch,
            phase,
        };
        let key = (glyph, flip, lean.tan().to_bits());
        if let Some(known) = self.held.sheared.get(&key) {
            return *known;
        }
        let own = match flip {
            Flip::Kept => self.slot_at(face, ix, id, scale, phase, stretch),
            Flip::Flipped => self.slot_flipped_at(face, ix, id, scale, phase, stretch),
        }?;
        let sheared = self.shear(own, lean);
        if sheared.is_some() {
            self.held.sheared.insert(key, sheared);
        }
        sheared
    }

    /// `own`'s texels sheared left by `lean`, in a slot of their own on its
    /// sheet; `None` where the sheet cannot grow to hold them.
    fn shear(&mut self, own: Slot, lean: Lean) -> Option<Slot> {
        let (w, h) = (usize::from(own.w.0), usize::from(own.h.0));
        if w == 0 || h == 0 {
            return Some(own);
        }
        // A row's move, in 256ths of a texel: its centre's depth below the
        // baseline, `top` being the first row's, times the lean.
        let moved = |row: usize| {
            let depth = own.top as f32 + f32::from(u16::try_from(row).unwrap_or(u16::MAX)) + 0.5;
            (lean.tan() * depth * 256.0).round() as i32
        };
        let (a, b) = (moved(0), moved(h - 1));
        let least = a.min(b).div_euclid(256);
        let wide = w + usize::try_from(a.max(b).div_euclid(256) - least).ok()? + 1;
        let (x0, y0) = (usize::from(own.x.0), usize::from(own.y.0));
        let at = Origin {
            left: own.left + least,
            top: own.top,
        };
        // Output texel `x` of a row moved by `whole` texels and `part`
        // 256ths takes input texel `x - whole` for `256 - part` of its width
        // and the one before it for `part`.
        let split = |row: usize| {
            let m = moved(row) - least * 256;
            (
                usize::try_from(m.div_euclid(256)).unwrap_or(0),
                m.rem_euclid(256).unsigned_abs(),
            )
        };
        let mix =
            |here: u32, before: u32, part: u32| (here * (256 - part) + before * part + 128) >> 8;
        let tall = Px::of(u32::from(own.h.0));
        let across = Px::of(u32::try_from(wide).ok()?);
        match own.kind {
            Kind::Mask => {
                let side = slots(Px::of(self.mask.side.0));
                let mut image = vec![0u8; wide * h];
                for row in 0..h {
                    let from = (y0 + row) * side + x0;
                    let src = self.pixels.get(from..from + w)?;
                    let get =
                        |i: Option<usize>| i.and_then(|i| src.get(i)).map_or(0, |&v| u32::from(v));
                    let (whole, part) = split(row);
                    for (x, dst) in image[row * wide..(row + 1) * wide].iter_mut().enumerate() {
                        let here = get(x.checked_sub(whole));
                        let before = get(x.checked_sub(whole + 1));
                        *dst = u8::try_from(mix(here, before, part)).unwrap_or(u8::MAX);
                    }
                }
                self.place_mask(&image, across, tall, at)
            }
            Kind::Color => {
                let side = slots(Px::of(self.color.side.0));
                let mut image = vec![0u32; wide * h];
                for row in 0..h {
                    let from = (y0 + row) * side + x0;
                    let src = self.argb.get(from..from + w)?;
                    let get = |i: Option<usize>| {
                        i.and_then(|i| src.get(i))
                            .copied()
                            .unwrap_or(0)
                            .to_le_bytes()
                    };
                    let (whole, part) = split(row);
                    for (x, dst) in image[row * wide..(row + 1) * wide].iter_mut().enumerate() {
                        let (here, before) =
                            (get(x.checked_sub(whole)), get(x.checked_sub(whole + 1)));
                        // Each premultiplied channel's byte apart.
                        *dst = u32::from_le_bytes(core::array::from_fn(|c| {
                            let v = mix(u32::from(here[c]), u32::from(before[c]), part);
                            u8::try_from(v).unwrap_or(u8::MAX)
                        }));
                    }
                }
                self.place_color(&image, across, tall, at)
            }
            // A glyph's image is a mask or colour; any other has no texels.
            _ => Some(own),
        }
    }

    /// `own`'s texels, each row reversed, in a slot of their own on its
    /// sheet; `None` where the sheet cannot grow to hold them.
    fn flip(&mut self, own: Slot) -> Option<Slot> {
        let (w, h) = (
            slots(Px::of(u32::from(own.w.0))),
            slots(Px::of(u32::from(own.h.0))),
        );
        let (x0, y0) = (
            slots(Px::of(u32::from(own.x.0))),
            slots(Px::of(u32::from(own.y.0))),
        );
        let at = Origin {
            left: own.left,
            top: own.top,
        };
        match own.kind {
            Kind::Mask => {
                let side = slots(Px::of(self.mask.side.0));
                let mut image = Vec::with_capacity(w * h);
                for row in 0..h {
                    let from = (y0 + row) * side + x0;
                    image.extend(self.pixels.get(from..from + w)?.iter().rev());
                }
                self.place_mask(
                    &image,
                    Px::of(u32::from(own.w.0)),
                    Px::of(u32::from(own.h.0)),
                    at,
                )
            }
            Kind::Color => {
                let side = slots(Px::of(self.color.side.0));
                let mut image = Vec::with_capacity(w * h);
                for row in 0..h {
                    let from = (y0 + row) * side + x0;
                    image.extend(self.argb.get(from..from + w)?.iter().rev());
                }
                self.place_color(
                    &image,
                    Px::of(u32::from(own.w.0)),
                    Px::of(u32::from(own.h.0)),
                    at,
                )
            }
            // A glyph's image is a mask or colour; any other has no texels.
            _ => Some(own),
        }
    }

    /// Packs a coverage mask onto the mask sheet, growing it as needed:
    /// [`Atlas::place_color`]'s twin.
    fn place_mask(&mut self, image: &[u8], w: Px, h: Px, at: Origin) -> Option<Slot> {
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
        let side = slots(Px::of(self.mask.side.0));
        let (xs, ys, ws, hs) = (slots(x), slots(y), slots(w), slots(h));
        for row in 0..hs {
            let src = &image[row * ws..(row + 1) * ws];
            let at = (ys + row) * side + xs;
            if let Some(dst) = self.pixels.get_mut(at..at + ws) {
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
        Atlas::dirtied(&mut self.dirty.mask, rect);
        Some(Slot {
            kind: Kind::Mask,
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: rect.h,
            left: at.left,
            top: at.top,
        })
    }

    /// The glyph's image from a strike that holds it: among the strikes
    /// that hold it and decode, the smallest at or above the cell's size
    /// rounded half up, else the largest -- CoreText's order for the square
    /// it draws, which below 24 px on the system font is larger than the
    /// size and is not followed here -- then mirrored if a
    /// `'flip'` said so, and scaled to fit. A bitmap cannot be drawn at a
    /// fraction of a pixel, so only `phase`'s whole pixels move it.
    fn bitmap(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
        stretch: Stretch,
    ) -> Option<Slot> {
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
        let (fx, fy) = (factor * stretch.xf(), factor * stretch.yf());
        let dw = (sw as f32 * fx).round().max(1.0) as i64;
        let dh = (sh as f32 * fy).round().max(1.0) as i64;
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
                (f32::from(bitmap.placement.x) * fx).round() as i32,
                -((f32::from(bitmap.placement.y) * fy).round() as i32) - i32::try_from(dh).ok()?,
            ),
            Strike::Cbdt => (
                (f32::from(bitmap.placement.x) * fx).round() as i32,
                -((f32::from(bitmap.placement.y) * fy).round() as i32),
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
        stretch: Stretch,
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
        let fit = Fit::stretched(&b, scale, phase, stretch.xf(), stretch.yf())?;
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
        stretch: Stretch,
    ) -> bool {
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(_) | Drawn::Refused = outline(&mut self.var, face, id, &mut b) {
            return true;
        }
        Fit::stretched(&b, scale, phase, stretch.xf(), stretch.yf()).is_none()
    }

    fn rasterise(
        &mut self,
        face: &Face<'_>,
        id: GlyphId,
        scale: Scale,
        phase: Phase,
        stretch: Stretch,
    ) -> Option<Slot> {
        let mut b = Bounds::new();
        if let Drawn::NeedsScratch(_) | Drawn::Refused = outline(&mut self.var, face, id, &mut b) {
            self.stats.refused.bump();
            return None;
        }
        let fit = Fit::stretched(&b, scale, phase, stretch.xf(), stretch.yf())?;
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
        self.place_coverage(&fit)
    }

    /// The coverage just drawn for `fit` placed on the mask sheet, the
    /// sheet grown when it is full.
    fn place_coverage(&mut self, fit: &Fit) -> Option<Slot> {
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
    /// for a codepoint a font draws. Memoised per codepoint and size until
    /// `reset`, because the mask is a function of the cell and the cell
    /// changes only with the size.
    pub fn synthetic(
        &mut self,
        cp: Codepoint,
        cell: crate::boxes::Cell,
        baseline: Px,
    ) -> Option<Slot> {
        if let Some(slot) = self.held.procedural.get(&cp.value()) {
            return *slot;
        }
        let slot = self.synthesise(cp, cell, baseline);
        if slot.is_some() || !crate::boxes::covers(cp) {
            self.held.procedural.insert(cp.value(), slot);
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
        self.place_cell_mask(extent, baseline)
    }

    /// [`Atlas::synthetic`] for a line drawn at `stretch`: a double-height
    /// line's mask drawn at the doubled cell, two cells wide and two tall,
    /// with its baseline doubled, so the two halves cut from it join at the
    /// row boundary; a double-width line's drawn at the cell and each
    /// column repeated, which doubles the vertical strokes and keeps the
    /// horizontal ones, a stroke being whole pixels.
    pub fn synthetic_at(
        &mut self,
        cp: Codepoint,
        cell: crate::boxes::Cell,
        baseline: Px,
        stretch: Stretch,
    ) -> Option<Slot> {
        if stretch == Stretch::One {
            return self.synthetic(cp, cell, baseline);
        }
        if let Some(slot) = self.held.procedural_stretched.get(&(cp.value(), stretch)) {
            return *slot;
        }
        let slot = match stretch {
            Stretch::Tall => {
                let big = crate::boxes::Cell {
                    w: Px::of(cell.w.value() * 2),
                    h: Px::of(cell.h.value() * 2),
                };
                crate::boxes::draw(cp, big, &mut self.scratch, &mut self.coverage)
                    .and_then(|extent| self.place_cell_mask(extent, Px::of(baseline.value() * 2)))
            }
            _ => crate::boxes::draw(cp, cell, &mut self.scratch, &mut self.coverage).and_then(
                |extent| {
                    self.double_columns(extent);
                    self.place_cell_mask(
                        Extent::new(Px::of(extent.w.value() * 2), extent.h),
                        baseline,
                    )
                },
            ),
        };
        if slot.is_some() || !crate::boxes::covers(cp) {
            self.held
                .procedural_stretched
                .insert((cp.value(), stretch), slot);
        }
        slot
    }

    /// [`Atlas::synthetic_at`] for a character that takes `span` cells,
    /// facing as `facing` says: one cell is that, unchanged, or its
    /// counterpart's mirrored; two is the mask across both
    /// ([`crate::boxes::draw_across`]), its strokes at one cell's weight,
    /// then stretched as [`Atlas::synthetic_at`] stretches a one-cell mask
    /// -- drawn at the doubled cell on a double-height line, each column
    /// repeated on a double-width one. Memoised by codepoint and stretch in
    /// a map of its own for each facing.
    pub fn synthetic_across(
        &mut self,
        cp: Codepoint,
        cell: crate::boxes::Cell,
        (span, facing): (crate::cellmap::CellSpan, crate::boxes::Facing),
        baseline: Px,
        stretch: Stretch,
    ) -> Option<Slot> {
        use crate::boxes::Facing;
        if span == crate::cellmap::CellSpan::One {
            let cp = match facing {
                Facing::Written => cp,
                Facing::Mirrored => char::from_u32(cp.value())
                    .and_then(baud_domain::box_counterpart)
                    .map_or(cp, Codepoint::from_char),
            };
            return self.synthetic_at(cp, cell, baseline, stretch);
        }
        let held = match facing {
            Facing::Written => &self.held.procedural_wide,
            Facing::Mirrored => &self.held.procedural_mirrored,
        };
        if let Some(slot) = held.get(&(cp.value(), stretch)) {
            return *slot;
        }
        let (drawn, at) = match stretch {
            Stretch::Tall => (
                crate::boxes::Cell {
                    w: Px::of(cell.w.value() * 2),
                    h: Px::of(cell.h.value() * 2),
                },
                Px::of(baseline.value() * 2),
            ),
            Stretch::One | Stretch::Wide => (cell, baseline),
        };
        let slot = crate::boxes::draw_across(
            cp,
            drawn,
            span,
            facing,
            &mut self.scratch,
            &mut self.coverage,
        )
        .and_then(|extent| match stretch {
            Stretch::Wide => {
                self.double_columns(extent);
                self.place_cell_mask(Extent::new(Px::of(extent.w.value() * 2), extent.h), at)
            }
            Stretch::One | Stretch::Tall => self.place_cell_mask(extent, at),
        });
        if slot.is_some() || !crate::boxes::covers(cp) {
            match facing {
                Facing::Written => &mut self.held.procedural_wide,
                Facing::Mirrored => &mut self.held.procedural_mirrored,
            }
            .insert((cp.value(), stretch), slot);
        }
        slot
    }

    /// The mask for one part of an encircled run's outline (`SGR 52`),
    /// drawn at exactly `cell` with a stroke `th` wide and placed at the
    /// cell origin as a procedural glyph is, memoised the same way under
    /// the part's key ([`crate::boxes::Ring::key`]), above every codepoint.
    pub fn ring(
        &mut self,
        ring: crate::boxes::Ring,
        cell: crate::boxes::Cell,
        th: Px,
        baseline: Px,
    ) -> Option<Slot> {
        if let Some(slot) = self.held.procedural.get(&ring.key().0) {
            return *slot;
        }
        let slot = crate::boxes::draw_ring(ring, cell, th, &mut self.scratch, &mut self.coverage)
            .and_then(|extent| self.place_cell_mask(extent, baseline));
        self.held.procedural.insert(ring.key().0, slot);
        slot
    }

    /// **An exit-status mark's mask** ([`crate::status`]), the cell's box,
    /// placed as a procedural glyph is and memoised the same way under the
    /// mark's key, per size with them.
    pub fn status_mark(
        &mut self,
        mark: crate::status::Mark,
        cell: crate::boxes::Cell,
        baseline: Px,
    ) -> Option<Slot> {
        if let Some(slot) = self.held.procedural.get(&mark.key().0) {
            return *slot;
        }
        let slot = crate::status::draw(mark, cell.w, cell.h, &mut self.scratch, &mut self.coverage)
            .and_then(|extent| self.place_cell_mask(extent, baseline));
        self.held.procedural.insert(mark.key().0, slot);
        slot
    }

    /// [`Atlas::ring`] for a line drawn at `stretch`: the part's mask at
    /// the box a character there takes, built as box drawing is
    /// ([`Atlas::synthetic_at`]) -- at twice the cell with twice the stroke
    /// on a double-height line, each column doubled on a double-width one
    /// -- so its strokes meet the frame's lines, which the line's geometry
    /// stretches the same way. Memoised under the part's key and the
    /// stretch.
    pub fn ring_at(
        &mut self,
        ring: crate::boxes::Ring,
        cell: crate::boxes::Cell,
        th: Px,
        baseline: Px,
        stretch: Stretch,
    ) -> Option<Slot> {
        if stretch == Stretch::One {
            return self.ring(ring, cell, th, baseline);
        }
        let key = (ring.key().0, stretch);
        if let Some(slot) = self.held.procedural_stretched.get(&key) {
            return *slot;
        }
        let slot = match stretch {
            Stretch::Tall => {
                let big = crate::boxes::Cell {
                    w: Px::of(cell.w.value() * 2),
                    h: Px::of(cell.h.value() * 2),
                };
                let th = Px::of(th.value() * 2);
                crate::boxes::draw_ring(ring, big, th, &mut self.scratch, &mut self.coverage)
                    .and_then(|extent| self.place_cell_mask(extent, Px::of(baseline.value() * 2)))
            }
            _ => crate::boxes::draw_ring(ring, cell, th, &mut self.scratch, &mut self.coverage)
                .and_then(|extent| {
                    self.double_columns(extent);
                    self.place_cell_mask(
                        Extent::new(Px::of(extent.w.value() * 2), extent.h),
                        baseline,
                    )
                }),
        };
        self.held.procedural_stretched.insert(key, slot);
        slot
    }

    /// The mask in the first cells of `coverage`, `extent` in size, made
    /// twice as wide in place: each column drawn twice.
    fn double_columns(&mut self, extent: Extent) {
        let (w, h) = (slots(extent.w), slots(extent.h));
        let mut wide = vec![0u8; 2 * w * h];
        for y in 0..h {
            for x in 0..w {
                let v = self.coverage[y * w + x];
                wide[y * 2 * w + 2 * x] = v;
                wide[y * 2 * w + 2 * x + 1] = v;
            }
        }
        if self.coverage.len() < wide.len() {
            self.coverage.resize(wide.len(), 0);
        }
        self.coverage[..wide.len()].copy_from_slice(&wide);
    }

    /// Places the mask in the first cells of `coverage` on the mask sheet
    /// at the cell origin: `left` zero, `top` `-baseline`.
    fn place_cell_mask(&mut self, extent: Extent, baseline: Px) -> Option<Slot> {
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

    /// And a reset still voids everything, which is what `grown` means,
    /// with a change of size. The pair tells the two events apart, so a cache
    /// can watch the one that concerns it rather than both.
    #[test]
    fn a_reset_voids_what_growing_keeps() {
        let mut a = Atlas::new();
        let before = a.stats;
        a.reset();
        assert_eq!(a.stats.grown.value(), before.grown.value() + 1);
        assert_eq!(a.stats.enlarged.value(), before.enlarged.value());
    }

    /// **What a rescale does to the memos, size by size.** The first size
    /// is taken as the atlas's own; another is added beside it and the one
    /// before is kept; a held size comes back; the size past [`SIZES`]
    /// starts over, with nothing kept. Each change but the same size moves
    /// `grown`, and none replaces a sheet but the start over. A box-drawing
    /// mask is drawn at each size, at that size's cell, so each memo holds
    /// something.
    #[test]
    fn a_rescale_keeps_the_sizes_it_holds_and_starts_over_past_them() {
        let key = |px: f32| SizeKey::of(crate::PxSize::of(px).unwrap());
        let line = Codepoint::new(0x2500).unwrap();
        let draw = |a: &mut Atlas, k: usize| {
            let cell = crate::boxes::Cell {
                w: Px::of(8 + u32::try_from(k).unwrap()),
                h: Px::of(16 + 2 * u32::try_from(k).unwrap()),
            };
            a.synthetic(line, cell, Px::of(12))
                .expect("a box-drawing mask")
        };
        let mut a = Atlas::new();
        assert_eq!(a.at(key(14.0)), Rescaled::Added, "the first size is taken");
        let first = draw(&mut a, 0);
        a.take_dirty();
        let (grown, drawn) = (a.stats.grown, a.stats.synthesised);
        assert_eq!(a.at(key(14.0)), Rescaled::Same);
        assert_eq!(a.stats.grown, grown, "the same size moves nothing");
        for k in 1..SIZES {
            let px = 14.0 + k as f32;
            assert_eq!(a.at(key(px)), Rescaled::Added, "{px} px");
            draw(&mut a, k);
        }
        assert_eq!(a.kept.len(), SIZES - 1);
        a.take_dirty();
        assert_eq!(a.at(key(14.0)), Rescaled::Kept, "the first size is held");
        assert_eq!(draw(&mut a, 0), first, "and its mask is where it was");
        assert_eq!(
            a.stats.synthesised.value(),
            drawn.value() + SIZES as u64 - 1,
            "drawn once a size"
        );
        assert_eq!(
            a.take_dirty(),
            (Dirty::default(), Replaced::No, Replaced::No),
            "switching between held sizes replaces no sheet and writes nothing"
        );
        assert_eq!(
            a.stats.grown.value(),
            grown.value() + SIZES as u64,
            "every change of size tells what kept a rectangle"
        );
        assert_eq!(a.at(key(14.0 + SIZES as f32)), Rescaled::Restarted);
        assert!(a.kept.is_empty(), "a start over keeps no other size");
        assert_eq!(a.take_dirty().1, Replaced::Yes, "and replaces the sheets");
    }
}
