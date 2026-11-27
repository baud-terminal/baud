//! **The launch splash**: baud's colour lockup, `baud ❯ █`, in the middle
//! of the window once it is shown -- drawn by the plasma from its left
//! edge to its right, whole at [`OUT`], 0.10 s in, and on that frame a
//! second plasma lights over it and carries it straight back out from its
//! left edge to its right, every glyph taken inside it, the logo gone at
//! [`LEFT`], 0.47 s in, and what is left of the plasma at [`GONE`], 0.55 s
//! in: one movement, with no frame where the logo stands still (note AJ).
//! The lockup is
//! `docs/assets/logo/baud-lockup-dark.svg`'s and `-light.svg`'s, colours and
//! geometry, as `tools/baud-pictures` draws them: the word's pen
//! at x 40, the ❯ in its cell from x 192, its top on the word's, placed as baud's
//! renderer places a glyph at 200 px and scaled to the cell, the block the
//! very next cell, and the whole moved [`DX`] to centre its ink across the
//! box -- without the dark one's baked ground, which would be a rectangle
//! on the glass; the half is the theme's by the rule `CSI ? 996 n` answers
//! with ([`crate::Theme::polarity`]). The word is drawn in the renderer's
//! badge face, JetBrains Mono Bold, where the files trace it at 500.
//!
//! **Its plasma is the colour of the glyph it is over** ([`paint`]): the
//! word's ink over `baud` -- near-white on the dark page, dark on the
//! light -- and the amber over `❯` and the block, stepping from the one
//! to the other across the gap between them. Every path draws it
//! source-over in its record's colour, so on the light page the word's
//! plasma is a dark glow by the same filaments and ions. The cursor
//! trails keep the cursor's colour. It goes where it stands rather than
//! flying its block to the shell's cursor. It is drawn as the lockup's
//! own pixels, not as cells.
//!
//! **It is the plasma trail's own model, at kitty's pace**,
//! [`crate::plasma`]: the lockup's block is a cursor, and the box from the
//! wordmark's left to the block collapses into it as kitty's quad
//! collapses into the cell a cursor reached ([`Segment`]), the filaments
//! braiding into the block and the ions peeling off, on the trail's
//! constants and clock, with no tube filling the box ([`Body::Bare`]).
//! **Where it is over time is the splash's own**: the first pass's box
//! spans the whole lockup for [`HOLD`] while an even edge crosses the
//! glyphs inside it, the block last, so the logo is drawn inside the plasma, which it stands faintly
//! in from its first frame ([`GHOST`]); the second pass lights over the
//! whole logo on the frame the block is drawn, its edge takes each letter
//! through its own [`HOLD`], and its box then collapses into the block on
//! kitty's curve, the block going as the box lands on it, and what is left
//! of it fades to nothing by [`GONE`]. Each frame has one effect, so the
//! second pass replaces the first, as it did when it lit at the first's
//! landing.
//! The two passes put a fourth flash into one second at a few pixels, an
//! area the general and red flash thresholds allow by a wide margin
//! (`crates/baud-bench/tests/splash_flashes.rs`); no pass is capped, so nothing
//! waits on a flash cap and no frame stands still. No new shader: every
//! path draws it by the effect it already draws, and the pixel oracle
//! holds it by the reference it already has.
//!
//! **Everything is a pure function of the phase**: the driver hands
//! [`SplashLook`] -- so many seconds into the splash, the logo held, or so
//! far into the fade -- computed from the display link's timestamp, and
//! this derives the records and the effect. No clock and no state, as
//! [`crate::effect`].

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::effect::{Body, Effect, Ignition, Seed, Vfx, smoothstep};
use crate::flash::Strength;
use crate::instance::{Instance, Screen};
use crate::trail::{Pt, Rect, Segment, Shape, Smear};
use crate::units::slots;
use baud_domain::Chan;
use baud_font::Px;
use core::ops::Range;

/// **Where the splash is**, as the driver hands it each frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SplashPhase {
    /// So many seconds into the splash under full motion, from its first
    /// frame: the logo coming out of the plasma, then swept out of it.
    Plasma(f32),
    /// The logo, whole: from the first frame under reduced motion.
    Hold,
    /// So far through the fade, 0 to 1: the form under reduced motion.
    Fade(f32),
}

/// The splash for a frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SplashLook {
    pub phase: SplashPhase,
}

/// The lockup's box, in the units of its SVG.
pub const BOX: (f32, f32) = (300.0, 130.0);

/// A line of the lockup's text: its pen's start, its baseline, its size
/// and the tracking between its characters, in the SVG's units.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Line {
    pub x: f32,
    pub baseline: f32,
    pub size: f32,
    pub tracking: f32,
}

/// **How far the logo files move every glyph to centre the lockup's ink
/// across its box**, in its units: `tools/baud-pictures`'s
/// `lockup_dx`, from the word's ink at 500 and the mark's, 10.362. The
/// generator holds this to its own (`brand_plasma`'s report), and
/// `tests/splash.rs` holds the drawn lockup to the file's.
pub const DX: f32 = 10.362;

/// **Lockup units a pixel of baud's cell at 200 px**: the mark's cell is
/// 46 units tall, and baud's cell at 200 px is 264 tall (the generator's
/// `CELL_H`).
pub const K: f32 = 46.0 / 264.0;

/// **The top of the mark's cell, in the lockup's units: the top of the
/// word's `b` and `d`.** The word's baseline 88 less their 0.73 em at size
/// 62, the files' `LOCKUP_CELL`, so the block's top sits on the word's top.
pub const MARK_TOP: f32 = 42.74;

/// `baud`: its pen at x 40 moved by [`DX`], its baseline 88, size 62 and
/// letter-spacing -2.8 -- the files' `WORD`.
pub const WORDMARK: Line = Line {
    x: 40.0 + DX,
    baseline: 88.0,
    size: 62.0,
    tracking: -2.8,
};

/// `❯`, placed in its cell as baud's renderer places a glyph: its pen at
/// the cell's left, x 192 moved by [`DX`], its baseline 204 of baud's
/// 200 px cell below the cell's top, [`MARK_TOP`], at 200 px, each scaled
/// by [`K`].
pub const PROMPT: Line = Line {
    x: 192.0 + DX,
    baseline: MARK_TOP + 204.0 * K,
    size: 200.0 * K,
    tracking: 0.0,
};

/// The block, the cell after the ❯'s: 120 of baud's 200 px cell wide from
/// the ❯'s cell's right edge, 46 units from [`MARK_TOP`] --
/// `<rect x="223.27" y="42.74" width="20.91" height="46">`.
pub const BLOCK: Rect = Rect {
    left: 192.0 + DX + 120.0 * K,
    top: MARK_TOP,
    right: 192.0 + DX + 240.0 * K,
    bottom: MARK_TOP + 46.0,
};

/// **The box the plasma fills at its start**: from the wordmark's pen to
/// the block's right edge, the block's height -- the lockup's cursor line,
/// from the top of the ascenders of `b` and `d` to just past the word's
/// foot.
pub const EMITTER: Rect = Rect {
    left: WORDMARK.x,
    top: BLOCK.top,
    right: BLOCK.right,
    bottom: BLOCK.bottom,
};

/// **The lockup's width as a share of the window's**, and the most of the
/// window's height it may take: 0.30, rather than 0.20 or 0.45. No text
/// gives a size.
pub const FRACTION: (f32, f32) = (0.30, 0.40);

/// The ignition's seed: one look, every launch.
pub const SEED: Seed = Seed(0xBA0D_276F);

/// Seconds the fade lasts: the screen transition's (`TRANSITION_S`).
pub const FADE_S: f32 = 0.15;

/// **Seconds each pass's plasma spans the whole lockup**: the box from the
/// wordmark's pen to the block holds while an even edge crosses the glyphs
/// inside it ([`reveal`]), so each is drawn, and later taken, under the
/// plasma; the second pass's box then lands on the block on kitty's curve.
/// 0.10 rather than 0.15 or none: none left the word's letters outside the
/// plasma as they were drawn, `b` under it for one 60 Hz frame and not at
/// all as it left (note AJ, section 15).
pub const HOLD: f32 = 0.10;

/// **Seconds the second pass's box takes from the end of its hold to land
/// on the block**: kitty's trailing corner, 0.4 s to close all but `2^-10` of its
/// gap, has brought the block to 0.986-0.995 of its opacity, from a
/// 6120-pixel frame to a 640.
pub const LAND: f32 = 0.27;

/// **Seconds into the splash at which the plasma that carries it out
/// lights**: the first pass's edge has drawn the last glyph, the block, in
/// its hold, and the second pass lights over the whole logo on that frame
/// and starts to take it on the next, so the logo is drawn and leaves in
/// one movement, never standing still between (note AJ, section 17).
pub const OUT: f32 = HOLD;

/// **Seconds into the splash at which the logo is gone**: the second
/// pass's edge has taken every letter in its hold, and its box, on kitty's
/// curve, has closed on the block, which is under 0.02 of its opacity
/// (0.014 at a 1600-pixel frame, 0.018 at 6120); what is left of it fades
/// with the plasma's remainder.
pub const LEFT: f32 = OUT + HOLD + LAND;

/// **Seconds into the splash at which it is gone**: the second pass fades
/// to nothing over [`DIM`] up to here, so its end is no step at any frame
/// rate. The plasma's own end is later ([`end`]), but what is left of it
/// two 60 Hz frames before here is within 0.10 of the bare page's
/// relative luminance at every pixel on the four grounds the census counts
/// (`crates/baud-bench/tests/splash_flashes.rs`).
pub const GONE: f32 = LEFT + DIM;

/// **Seconds the second pass takes to fade out**, ending at [`GONE`]: it
/// starts as the logo goes, the block last.
pub const DIM: f32 = 0.08;

/// The second pass's seed: its filaments and ions are its own, not the
/// first pass's played again.
pub const SEED_OUT: Seed = Seed(0x5EED_0A7F);

/// The lockup's two colours on a half: the wordmark's and the amber of the
/// prompt and the block.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Colours {
    pub wordmark: Argb,
    pub amber: Argb,
}

/// The dark lockup's, `#F3F4F6` and `#F4A32C`.
pub const DARK: Colours = Colours {
    wordmark: Argb(0xFFF3_F4F6),
    amber: Argb(0xFFF4_A32C),
};

/// **The light lockup's**: `#1E2128`, and the brand's amber, `#F4A32C`,
/// the dark lockup's and the light cursor's, so `❯█` is one colour.
pub fn light() -> Colours {
    Colours {
        wordmark: Argb(0xFF1E_2128),
        amber: DARK.amber,
    }
}

/// **Where the word's letters end and the mark's begin**, in the lockup's
/// units: the wordmark's pen plus four advances, each with its tracking,
/// and the prompt's pen. Nothing is drawn between them.
pub const SEAM: (f32, f32) = (
    WORDMARK.x + 4.0 * (WORDMARK.size * 0.6 + WORDMARK.tracking),
    PROMPT.x,
);

/// **How many slivers the plasma's colour turns in across the [`SEAM`]**,
/// each its own colour stepped from the word's ink to the amber: a blend
/// over the gap rather than a straight colour edge at its middle.
pub const SLIVERS: u8 = 8;

/// **The plasma's records in the colour of the glyph each is over**: a
/// band, a row of the plasma across the whole lockup, is split at the
/// [`SEAM`]'s slivers, each part in its colour -- the word's ink over
/// `baud`, near-white on the dark page and dark on the light, the amber
/// over `❯` and the block, and the steps between them over the gap -- and
/// an ion takes the colour where its centre is. Only the colour changes:
/// the light at each pixel is the effect's, whichever record draws it, so
/// the bands still cover the same pixels once each, in the same order. On
/// every path the plasma is drawn source-over in its record's colour, so
/// a dark ink is a dark glow on the light page.
pub fn paint(records: &mut Vec<Instance>, at: Pt, scale: f32, colours: Colours) {
    let n = f32::from(SLIVERS);
    let (l, r) = (at.x + SEAM.0 * scale, at.x + SEAM.1 * scale);
    // The pixel columns where the colour changes: the word's ink before
    // the first, the amber from the last, and a step between each two.
    let cuts: Vec<i32> = (0..=SLIVERS)
        .map(|i| (l + (r - l) * f32::from(i) / n).round() as i32)
        .collect();
    let tone = |x: i32| match cuts.iter().take_while(|&&c| c <= x).count() {
        0 => colours.wordmark,
        k if k >= cuts.len() => colours.amber,
        k => colours
            .amber
            .mix(colours.wordmark, Chan::from_unit((k as f32 - 0.5) / n)),
    };
    let mut out = Vec::with_capacity(records.len() + cuts.len() * 4);
    for rec in records.drain(..) {
        let x0 = i32::from(rec.x.0);
        let x1 = x0 + i32::from(rec.w.0);
        match rec.kind {
            Kind::Band => {
                let mut from = x0;
                for &c in cuts.iter().filter(|&&c| c > x0 && c < x1).chain([&x1]) {
                    out.push(Instance {
                        x: Screen(i16::try_from(from).unwrap_or(i16::MAX)),
                        w: Tex(u16::try_from(c - from).unwrap_or(0)),
                        color: tone(from).with_alpha(rec.color.a()),
                        ..rec
                    });
                    from = c;
                }
            }
            _ => out.push(Instance {
                color: tone(x0 + i32::from(rec.w.0) / 2).with_alpha(rec.color.a()),
                ..rec
            }),
        }
    }
    *records = out;
}

/// **Device pixels a lockup unit** over a frame `w` by `h`: the lockup
/// [`FRACTION`] of the window wide, or less where the window is short.
pub fn scale(frame: (f32, f32)) -> f32 {
    (frame.0 * FRACTION.0 / BOX.0)
        .min(frame.1 * FRACTION.1 / BOX.1)
        .max(0.0)
}

/// Where the lockup's box starts, centred in the frame, at `scale`.
pub fn origin(frame: (f32, f32), scale: f32) -> Pt {
    Pt {
        x: ((frame.0 - BOX.0 * scale) * 0.5).round(),
        y: ((frame.1 - BOX.1 * scale) * 0.5).round(),
    }
}

/// A lockup rectangle in frame pixels.
pub fn placed(r: Rect, at: Pt, scale: f32) -> Rect {
    Rect {
        left: at.x + r.left * scale,
        top: at.y + r.top * scale,
        right: at.x + r.right * scale,
        bottom: at.y + r.bottom * scale,
    }
}

/// **The ignition a pass is**: from the emitter's left, on its centre
/// line, to the block, its unit the block's height.
pub fn ignition(at: Pt, scale: f32) -> Ignition {
    lit(at, scale, SEED)
}

fn lit(at: Pt, scale: f32, seed: Seed) -> Ignition {
    lit_over(EMITTER, at, scale, seed)
}

/// [`lit`] from the left of `emitter`, in the lockup's units.
fn lit_over(emitter: Rect, at: Pt, scale: f32, seed: Seed) -> Ignition {
    let e = placed(emitter, at, scale);
    let block = placed(BLOCK, at, scale);
    Ignition::ignite(
        Pt {
            x: e.left,
            y: (e.top + e.bottom) * 0.5,
        },
        block,
        seed,
        (block.bottom - block.top).max(1.0),
    )
}

/// **The emitter's corners `t` seconds after a pass lights**: the box from
/// the wordmark's pen to the block for [`HOLD`], then collapsing into the
/// block on kitty's constants, its clock started where the hold ends.
fn corners(e: Rect, block: Rect, t: f32) -> crate::trail::Corners {
    match t < HOLD {
        true => e.corners(),
        false => Segment::new(e.corners(), block).at(t - HOLD),
    }
}

/// **One pass `t` seconds after it lights**: the emitter spanning the
/// lockup, then collapsing from its box into the block ([`corners`]), the
/// block its hole, the axis the move's, the ignition on kitty's clock from
/// the frame it lights, at full strength -- a cursor trail's plasma, as
/// the trail draws one, with no body ([`Body::Bare`]): its filaments and
/// ions over the lockup, and no tube filling the box nor sheath round it.
fn pass(at: Pt, scale: f32, seed: Seed, t: f32) -> Vfx {
    let v = pass_over(EMITTER, at, scale, seed, t);
    Vfx {
        ignition: Ignition {
            body: Body::Bare,
            ..v.ignition
        },
        ..v
    }
}

/// **A pass over part of the lockup**: [`vfx`]'s pass with `emitter`, in
/// the lockup's units, in place of [`EMITTER`] -- lit from its left on its
/// centre line towards the block, held for [`HOLD`], then collapsing into
/// the block. The organisation picture lights its word with one, the box
/// ending at the word, [`SEAM`]'s first column
/// (`crates/baud-render/examples/brand_plasma.rs`).
pub fn pass_over(emitter: Rect, at: Pt, scale: f32, seed: Seed, t: f32) -> Vfx {
    let e = placed(emitter, at, scale);
    let block = placed(BLOCK, at, scale);
    let t = t.max(0.0);
    let f = lit_over(emitter, at, scale, seed).at(t);
    Vfx {
        effect: Effect::Plasma,
        emitter: Smear {
            corners: corners(e, block, t),
            hole: block,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: f,
        axis: f.dir,
    }
}

/// **The plasma `age` seconds into the splash**: two passes of kitty's
/// plasma, each at its own clock from the frame it lights -- the first
/// drawing the logo from the first frame, the second, lit over it at
/// [`OUT`], taking it away.
pub fn vfx(at: Pt, scale: f32, age: f32) -> Vfx {
    match age < OUT {
        true => pass(at, scale, SEED, age),
        false => pass(at, scale, SEED_OUT, age - OUT),
    }
}

/// **Seconds from the splash's start to when nothing of its plasma would
/// be left**: the second pass's end -- its life, or its last ion's death,
/// on its own clock -- which [`GONE`] comes before. The ion count reads
/// the path's length in units, so this is the same at every size.
pub fn end() -> f32 {
    OUT + Effect::Plasma.end(&lit(Pt { x: 0.0, y: 0.0 }, 1.0, SEED_OUT))
}

/// **The logo's opacity before the emerge's edge reaches it**: drawn
/// faintly inside the plasma from the first frame, so the logo is on
/// screen at once and the plasma draws it whole.
pub const GHOST: f32 = 0.25;

/// **How much of the second pass's light is drawn `age` seconds in**: all
/// of it until [`DIM`] before [`GONE`], then less in step with the time
/// left, none at [`GONE`] -- so the frame before the splash is gone already
/// differs from the bare page by less than the remainder does, whenever it
/// falls. The logo's own last ink, the block's, fades with it.
pub fn glow(age: f32) -> f32 {
    ((GONE - age) / DIM).clamp(0.0, 1.0)
}

/// The edge of the plasma's quad the block follows: its trailing edge,
/// the corners' leftmost.
fn edge(v: &Vfx) -> f32 {
    v.emitter
        .corners
        .0
        .iter()
        .map(|p| p.x)
        .fold(f32::INFINITY, f32::min)
}

/// **The edge the glyphs follow `t` seconds into a pass**, in frame
/// pixels: from the wordmark's pen to the block's right edge at one speed
/// through the pass's [`HOLD`], while the plasma spans the lockup, and past
/// every glyph after it.
pub fn reveal(at: Pt, scale: f32, t: f32) -> f32 {
    match t < HOLD {
        true => at.x + (EMITTER.left + (EMITTER.right - EMITTER.left) * t.max(0.0) / HOLD) * scale,
        false => f32::INFINITY,
    }
}

/// **A glyph's opacity**, `left` to `right` its box in frame pixels: whole
/// while held; drawn as the first pass's [`reveal`] edge crosses it, from
/// [`GHOST`], and gone as the second pass's crosses it; and the fade's.
pub fn ink(look: SplashLook, at: Pt, scale: f32, left: f32, right: f32) -> f32 {
    let swept = |x: f32| smoothstep(left, right.max(left + 1.0), x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) if age < OUT => {
            GHOST + (1.0 - GHOST) * swept(reveal(at, scale, age))
        }
        SplashPhase::Plasma(age) => (1.0 - swept(reveal(at, scale, age - OUT))) * glow(age),
    }
}

/// **The block's opacity**: whole while held; drawn last, as the first
/// pass's edge crosses it, and gone last, as the second pass's box lands
/// on it; and the fade's.
pub fn block(look: SplashLook, at: Pt, scale: f32) -> f32 {
    let b = placed(BLOCK, at, scale);
    // Arrived within half a pixel, as a trail's corner is.
    let near = |x: f32| smoothstep(b.left - (b.right - b.left), b.left - 0.5, x);
    match look.phase {
        SplashPhase::Hold => 1.0,
        SplashPhase::Fade(p) => 1.0 - p.clamp(0.0, 1.0),
        SplashPhase::Plasma(age) if age < OUT => {
            GHOST + (1.0 - GHOST) * smoothstep(b.left, b.right, reveal(at, scale, age))
        }
        SplashPhase::Plasma(age) => (1.0 - near(edge(&vfx(at, scale, age)))) * glow(age),
    }
}

/// `c` at `alpha` of its own.
pub fn at_alpha(c: Argb, alpha: f32) -> Argb {
    c.with_alpha(Chan::from_unit(
        alpha.clamp(0.0, 1.0) * f32::from(c.a().get()) / 255.0,
    ))
}

/// **Where a [`Layer`] is drawn against the lockup's glyphs**: behind
/// them, where the splash's own plasma is, or in front of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    Behind,
    Front,
}

/// **A plasma layer drawn with the splash, by the CPU path alone**
/// ([`crate::Renderer::set_splash_layers`]): an effect built as a trail's,
/// drawn by the same records and compositor at its emitter's opacity,
/// behind the glyphs or in front of them. No driver sets one and the GPU
/// paths draw none: they are how `baud-app`'s logo film draws the README's
/// header. They take one colour ([`crate::Renderer::set_splash_ink`]),
/// not [`paint`]'s glyph colours, and in one colour source-over commutes
/// -- a pixel of glyph coverage `c` under a light `a` is `L (a + c - a c)
/// + G (1 - a)(1 - c)` either way round -- so over the word the depth reads
/// only through [`Slabs`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Layer {
    pub vfx: Vfx,
    pub depth: Depth,
}

/// **The knot cut**: the front layers' own light from `core[0]` to
/// `core[1]` taken as their core, the core grown by `grow` pixels, and
/// each glyph pixel under it taken `strength` of the way to the page
/// before the front layers are drawn, so a strand in front of a letter
/// reads as the over strand of a knot.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Knot {
    pub core: [f32; 2],
    pub grow: f32,
    pub strength: f32,
}

/// **The rim**: `band`, a weight on each pixel of the glyphs' inner edge;
/// on it the plasma's light -- the front layers' and 0.6 of the behind
/// layers', blurred over 6 pixels and scaled by `gain` -- takes the pixel
/// toward `ink`, at most 0.85 of the way. A `gain` of zero draws none.
#[derive(Clone, PartialEq, Debug)]
pub struct Rim {
    pub band: Vec<f32>,
    pub gain: f32,
    pub ink: Argb,
}

/// **The lockup's glyphs as slabs**: what the [`Layer`]s' depth does to
/// the pixels beyond drawing them under or over the glyphs, over the
/// frame-pixel box from `left`, `top`, `width` by `height`, each field a
/// weight for each pixel of it, row by row:
///
/// * `behind_cut`: how much of the light the behind layers drew is taken
///   back off a pixel -- the glyphs' mask grown by a keyline -- so a
///   letter in front of plasma stands clear of it by a gap of page;
/// * `glyph`: the glyphs' coverage, which the [`Knot`] cut takes toward
///   `page` under a strand in front;
/// * the [`Rim`] on the glyphs' inner edge.
#[derive(Clone, PartialEq, Debug)]
pub struct Slabs {
    pub left: Px,
    pub top: Px,
    pub width: Px,
    pub height: Px,
    pub page: Argb,
    pub behind_cut: Vec<f32>,
    pub glyph: Vec<f32>,
    pub knot: Knot,
    pub rim: Rim,
}

impl Slabs {
    /// Whether every field holds a weight for each of the box's pixels.
    pub fn fits(&self) -> bool {
        let n = slots(self.width) * slots(self.height);
        self.behind_cut.len() == n && self.glyph.len() == n && self.rim.band.len() == n
    }

    /// The box's columns and rows, as a target takes them.
    pub(crate) fn span(&self) -> (Range<usize>, Range<usize>) {
        let (x, y) = (slots(self.left), slots(self.top));
        (x..x + slots(self.width), y..y + slots(self.height))
    }

    /// **The knot cut over the box's pixels `px`**, `front` the front
    /// layers' own light at each: each glyph pixel taken toward the page by
    /// its coverage times the most of the core within the grown reach
    /// ([`Knot`]).
    pub(crate) fn cut_knot(&self, px: &mut [u32], front: &[f32]) {
        let (w, h) = (slots(self.width), slots(self.height));
        let k = self.knot;
        let core: Vec<f32> = front
            .iter()
            .map(|&a| smoothstep(k.core[0], k.core[1], a))
            .collect();
        let reach = usize::try_from(k.grow.max(0.0).ceil() as i64).unwrap_or(0) + 1;
        for y in 0..h {
            for x in 0..w {
                let gl = self.glyph[y * w + x];
                if gl <= 0.0 {
                    continue;
                }
                let mut best = 0.0f32;
                for yy in y.saturating_sub(reach)..(y + reach + 1).min(h) {
                    for xx in x.saturating_sub(reach)..(x + reach + 1).min(w) {
                        let (dx, dy) = (xx as f32 - x as f32, yy as f32 - y as f32);
                        let wgt = (k.grow + 1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                        if wgt > 0.0 {
                            best = best.max(core[yy * w + xx] * wgt);
                        }
                    }
                }
                let i = y * w + x;
                px[i] = mix(Argb(px[i]), self.page, gl * best * k.strength).0;
            }
        }
    }

    /// **The behind cut over the box's pixels `px`**: each taken back
    /// toward `before`, what it was before the behind layers were drawn,
    /// by its `behind_cut`.
    pub(crate) fn cut_behind(&self, px: &mut [u32], before: &[u32]) {
        for ((p, &b), &t) in px.iter_mut().zip(before).zip(&self.behind_cut) {
            *p = mix(Argb(*p), Argb(b), t).0;
        }
    }

    /// **The rim over the box's pixels `px`**, `front` and `behind` the
    /// layers' own light at each ([`Rim`]).
    pub(crate) fn light_rim(&self, px: &mut [u32], front: &[f32], behind: &[f32]) {
        const R: usize = 6;
        let (w, h) = (slots(self.width), slots(self.height));
        let mut light: Vec<f32> = front.iter().zip(behind).map(|(f, b)| f + 0.6 * b).collect();
        // A box blur of 6 pixels along the rows, then down the columns, each
        // the mean of the pixels it covers inside the box.
        for pass in [Pass::Rows, Pass::Cols] {
            let mut out = vec![0.0f32; w * h];
            for y in 0..h {
                for x in 0..w {
                    let (along, len) = match pass {
                        Pass::Rows => (x, w),
                        Pass::Cols => (y, h),
                    };
                    let (mut acc, mut n) = (0.0f32, 0.0f32);
                    for d in along.saturating_sub(R)..(along + R + 1).min(len) {
                        acc += match pass {
                            Pass::Rows => light[y * w + d],
                            Pass::Cols => light[d * w + x],
                        };
                        n += 1.0;
                    }
                    out[y * w + x] = acc / n;
                }
            }
            light = out;
        }
        for ((p, &band), &l) in px.iter_mut().zip(&self.rim.band).zip(&light) {
            if band > 0.0 {
                *p = mix(Argb(*p), self.rim.ink, band * (self.rim.gain * l).min(0.85)).0;
            }
        }
    }
}

/// A blur's direction.
#[derive(Clone, Copy)]
enum Pass {
    Rows,
    Cols,
}

/// `a` taken `t` of the way to `b`, each colour channel rounded, `a`'s
/// alpha kept.
fn mix(a: Argb, b: Argb, t: f32) -> Argb {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: Chan, y: Chan| {
        let (x, y) = (f32::from(x.get()), f32::from(y.get()));
        Chan::from_unit((x + (y - x) * t).round() / 255.0)
    };
    Argb::rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b())).with_alpha(a.a())
}

/// **A part of the frame the shell has drawn**, which the splash yields:
/// frame pixels, `left` and `top` in it, `right` and `bottom` past it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hole {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Hole {
    /// Whether any pixel of `r` is in this hole.
    pub fn meets(&self, r: &Instance) -> bool {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (x1, y1) = (x0 + i32::from(r.w.0), y0 + i32::from(r.h.0));
        x0 < self.right && self.left < x1 && y0 < self.bottom && self.top < y1
    }
}

/// **The splash's records with every [`Hole`] cut out of them**, so no
/// pixel the shell has drawn is under the logo or its plasma, and every
/// other pixel is drawn as it was: a glyph, a solid or a band of the
/// plasma is cut into the parts outside the holes -- a glyph's parts keep
/// their texels (`u` and `v` move with `x` and `y`, one texel a pixel), a
/// band's light is the effect's at each pixel whichever record draws it
/// ([`paint`] cuts them so too) -- and an ion or a quad, whose fragment
/// reads where in its own record a pixel is, is left out where it meets
/// one. A frame with no hole the records meet keeps them as they are.
pub fn yielded(records: &mut Vec<Instance>, holes: &[Hole]) {
    if !records.iter().any(|r| holes.iter().any(|h| h.meets(r))) {
        return;
    }
    let mut out = Vec::with_capacity(records.len());
    let mut parts = Vec::new();
    let mut next = Vec::new();
    for rec in records.drain(..) {
        if matches!(rec.kind, Kind::Particle | Kind::Quad) {
            if !holes.iter().any(|h| h.meets(&rec)) {
                out.push(rec);
            }
            continue;
        }
        parts.clear();
        parts.push(rec);
        for h in holes {
            next.clear();
            for p in &parts {
                cut(*p, h, &mut next);
            }
            core::mem::swap(&mut parts, &mut next);
        }
        out.extend_from_slice(&parts);
    }
    *records = out;
}

/// The parts of `rec` outside `h`, at most four: above it, below it, and
/// left and right of it on its rows.
fn cut(rec: Instance, h: &Hole, out: &mut Vec<Instance>) {
    if !h.meets(&rec) {
        out.push(rec);
        return;
    }
    let (x0, y0) = (i32::from(rec.x.0), i32::from(rec.y.0));
    let (x1, y1) = (x0 + i32::from(rec.w.0), y0 + i32::from(rec.h.0));
    let texels = matches!(rec.kind, Kind::Mask | Kind::Color);
    let part = |l: i32, t: i32, r: i32, b: i32| {
        let shift = |base: Tex, by: i32| match texels {
            true => Tex(u16::try_from(i32::from(base.0) + by).unwrap_or(base.0)),
            false => base,
        };
        Instance {
            x: Screen(i16::try_from(l).unwrap_or(rec.x.0)),
            y: Screen(i16::try_from(t).unwrap_or(rec.y.0)),
            w: Tex(u16::try_from(r - l).unwrap_or(0)),
            h: Tex(u16::try_from(b - t).unwrap_or(0)),
            u: shift(rec.u, l - x0),
            v: shift(rec.v, t - y0),
            ..rec
        }
    };
    let (top, bottom) = (h.top.max(y0), h.bottom.min(y1));
    for (l, t, r, b) in [
        (x0, y0, x1, top),
        (x0, bottom, x1, y1),
        (x0, top, h.left.min(x1), bottom),
        (h.right.max(x0), top, x1, bottom),
    ] {
        if l < r && t < b {
            out.push(part(l, t, r, b));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: (f32, f32) = (1600.0, 1000.0);

    /// **The lockup's box, colours and block are the SVG's**: the box and
    /// the two halves' colours, as `docs/assets/logo/baud-lockup-dark.svg` and
    /// `-light.svg` draw them -- the word in the one, the ❯ and the block in
    /// the other, each glyph an outline -- and the block's rect, to the
    /// file's two decimals. `tests/splash.rs` holds the drawn lockup to the
    /// file's ink.
    #[test]
    fn the_lockup_is_the_svgs() {
        let dark = include_str!("../../../docs/assets/logo/baud-lockup-dark.svg");
        let light = include_str!("../../../docs/assets/logo/baud-lockup-light.svg");
        for (svg, c) in [(dark, DARK), (light, super::light())] {
            assert!(svg.contains(r#"viewBox="0 0 300 130""#));
            let hex = |a: Argb| format!("#{:06X}", a.0 & 0xFF_FFFF);
            // The word's outline, then the ❯'s and the block, in the amber.
            let word = format!(r#"" fill="{}"/>"#, hex(c.wordmark));
            let amber = format!(r#"" fill="{}"/>"#, hex(c.amber));
            assert_eq!(svg.matches(&word).count(), 1, "the word, once");
            assert_eq!(svg.matches(&amber).count(), 2, "the ❯ and the block");
            let two = |v: f32| {
                format!("{:.2}", v)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            };
            assert!(svg.contains(&format!(
                r#"<rect x="{}" y="{}" width="{}" height="{}" fill="{}"/>"#,
                two(BLOCK.left),
                two(BLOCK.top),
                two(BLOCK.right - BLOCK.left),
                two(BLOCK.bottom - BLOCK.top),
                hex(c.amber)
            )));
            assert!(!svg.contains("<text"), "outlines, not text");
        }
        assert_eq!(BOX, (300.0, 130.0));
    }

    /// **A pass over the whole lockup is the splash's own**: [`pass_over`]
    /// with [`EMITTER`] is [`vfx`]'s pass in each of the two but for the
    /// splash's body, which is bare ([`Body::Bare`]), and one over the word
    /// alone lights from the same point with corners of its own.
    #[test]
    fn a_pass_over_the_emitter_is_the_splashs() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let bare = |v: Vfx| Vfx {
            ignition: Ignition {
                body: Body::Bare,
                ..v.ignition
            },
            ..v
        };
        for t in [0.0, 0.05, OUT - 1e-4] {
            assert_eq!(bare(pass_over(EMITTER, o, s, SEED, t)), vfx(o, s, t), "{t}");
        }
        for t in [OUT, OUT + HOLD + 0.1] {
            assert_eq!(
                bare(pass_over(EMITTER, o, s, SEED_OUT, t - OUT)),
                vfx(o, s, t),
                "{t}"
            );
        }
        let word = Rect {
            right: SEAM.0,
            ..EMITTER
        };
        let (w, whole) = (pass_over(word, o, s, SEED, 0.05), vfx(o, s, 0.05));
        assert_eq!(bare(w).ignition, whole.ignition);
        assert_ne!(w.emitter.corners, whole.emitter.corners);
    }

    /// **It is centred, and its share of the window**: as wide as the
    /// fraction says on a wide window, held by the height on a short one.
    #[test]
    fn it_is_centred_and_its_share_of_the_window() {
        let s = scale(FRAME);
        assert!((BOX.0 * s - FRAME.0 * FRACTION.0).abs() < 1e-3);
        let o = origin(FRAME, s);
        assert!((o.x * 2.0 + BOX.0 * s - FRAME.0).abs() <= 1.0);
        assert!((o.y * 2.0 + BOX.1 * s - FRAME.1).abs() <= 1.0);
        let short = (1600.0, 200.0);
        let t = scale(short);
        assert!((BOX.1 * t - short.1 * FRACTION.1).abs() < 1e-3);
        assert!(BOX.0 * t < short.0 * FRACTION.0);
    }

    /// **Both passes span the lockup, and the second lands on the block**:
    /// at each one's start, and through its [`HOLD`], the emitter is the box
    /// from the wordmark to the block, the hole the block, drawn at full
    /// strength, the second with a seed of its own; the first is the box on
    /// every frame before the second lights; half a second of the second's
    /// clock after its hold, it has landed on the block, within a pixel; and
    /// its own end, the same at every size, comes after the splash is gone.
    #[test]
    fn both_passes_span_the_lockup_and_the_second_lands_on_the_block() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let e = placed(EMITTER, o, s);
        let b = placed(BLOCK, o, s);
        for (start, seed) in [(0.0, SEED), (OUT, SEED_OUT)] {
            let first = vfx(o, s, start);
            let xs = first.emitter.corners.0.map(|p| p.x);
            assert!(xs.iter().any(|&x| (x - e.left).abs() < 1e-3), "{start}");
            assert!(xs.iter().any(|&x| (x - e.right).abs() < 1e-3), "{start}");
            assert_eq!(first.emitter.hole, b);
            assert_eq!(first.effect, Effect::Plasma);
            assert_eq!(first.emitter.strength, Strength::Full);
            assert_eq!(first.ignition.seed, seed);
            assert_eq!(first.ignition.age, 0.0, "lit at {start}");
            let held = vfx(o, s, start + HOLD - 1e-3);
            assert_eq!(held.emitter.corners, e.corners(), "held at {start}");
            assert!(held.ignition.age > 0.09, "its clock runs through the hold");
        }
        for ms in 0u8..100 {
            let age = f32::from(ms) / 1000.0;
            assert!(age >= OUT || vfx(o, s, age).emitter.corners == e.corners());
        }
        let late = vfx(o, s, OUT + HOLD + 0.5).emitter.corners.0;
        assert!(
            late.iter()
                .all(|p| p.x >= b.left - 1.0 && p.x <= b.right + 1.0),
            "{late:?}"
        );
        let out = end();
        assert!(GONE < out, "{GONE} {out}");
        assert_eq!(
            OUT + Effect::Plasma.end(&lit(o, s, SEED_OUT)),
            out,
            "the same at another size"
        );
    }

    /// **Each pass is kitty's plasma, at kitty's pace**: every 5 ms of the
    /// splash, its effect is the cursor trail's plasma spelled from the
    /// trail's own parts -- the emitter's box, held through [`HOLD`] and
    /// then collapsing into the block on kitty's curve from where the hold
    /// ends, and the ignition from the emitter's left aged at the pass's
    /// own clock, the seconds since it lit: the splash's for the first, the
    /// seconds since [`OUT`] for the second. So the plasma itself, its
    /// filaments and ions, runs at the trail's pace from the frame each
    /// pass lights, its body bare, and only where its box is changes.
    #[test]
    fn each_pass_is_kittys_plasma_at_kittys_pace() {
        for frame in [FRAME, (3400.0, 1850.0), (640.0, 400.0)] {
            let s = scale(frame);
            let o = origin(frame, s);
            let (e, b) = (placed(EMITTER, o, s), placed(BLOCK, o, s));
            let from = Pt {
                x: e.left,
                y: (e.top + e.bottom) * 0.5,
            };
            for ms in (0u16..).step_by(5) {
                let age = f32::from(ms) / 1000.0;
                if age >= GONE {
                    break;
                }
                let (seed, t) = match age < OUT {
                    true => (SEED, age),
                    false => (SEED_OUT, age - OUT),
                };
                let lit = Ignition {
                    body: Body::Bare,
                    ..Ignition::ignite(from, b, seed, b.bottom - b.top).at(t)
                };
                let v = vfx(o, s, age);
                assert_eq!(v.effect, Effect::Plasma, "{age}");
                let box_at = match t < HOLD {
                    true => e.corners(),
                    false => Segment::new(e.corners(), b).at(t - HOLD),
                };
                assert_eq!(v.emitter.corners, box_at, "{frame:?} at {age}");
                assert_eq!(v.ignition, lit, "{frame:?} at {age}");
                assert_eq!(v.axis, lit.dir, "{frame:?} at {age}");
            }
        }
    }

    /// **The second pass takes the logo, and the glow fades after it**: the
    /// logo whole at [`OUT`]; the wordmark's first glyph gone before the
    /// prompt; the letters gone by the end of the second pass's [`HOLD`];
    /// at [`LEFT`] nothing of the glyphs and under 0.02 of the block; the plasma's light whole until [`DIM`] before [`GONE`] and
    /// none at [`GONE`], nor anything of the block; the fade taking all of
    /// it together.
    #[test]
    fn the_second_pass_takes_the_logo_and_the_glow_fades_after_it() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let b = (o.x + WORDMARK.x * s, o.x + (WORDMARK.x + 35.0) * s);
        let p = (o.x + PROMPT.x * s, o.x + (PROMPT.x + 120.0 * K) * s);
        let held = SplashLook {
            phase: SplashPhase::Hold,
        };
        assert_eq!(ink(held, o, s, b.0, b.1), 1.0);
        assert_eq!(block(held, o, s), 1.0);
        let at = |t: f32| SplashLook {
            phase: SplashPhase::Plasma(t),
        };
        assert!(ink(at(OUT), o, s, b.0, b.1) > 1.0 - 1e-3);
        assert!(ink(at(OUT), o, s, p.0, p.1) > 1.0 - 1e-3);
        assert!(block(at(OUT), o, s) > 0.99, "{}", block(at(OUT), o, s));
        assert!(
            block(at(OUT - 1e-4), o, s) > 0.98,
            "whole as the first pass ends"
        );
        let t = OUT + HOLD / 2.0;
        assert!(ink(at(t), o, s, b.0, b.1) < ink(at(t), o, s, p.0, p.1));
        assert_eq!(ink(at(OUT + HOLD), o, s, p.0, p.1), 0.0);
        assert_eq!(ink(at(LEFT), o, s, b.0, b.1), 0.0);
        assert_eq!(ink(at(LEFT), o, s, p.0, p.1), 0.0);
        assert!(block(at(LEFT), o, s) < 0.02, "{}", block(at(LEFT), o, s));
        assert_eq!(block(at(GONE), o, s), 0.0);
        assert!(glow(GONE - DIM) > 1.0 - 1e-5, "{}", glow(GONE - DIM));
        assert!(glow(GONE - DIM / 2.0) > 0.4 && glow(GONE - DIM / 2.0) < 0.6);
        assert_eq!(glow(GONE), 0.0);
        let half = SplashLook {
            phase: SplashPhase::Fade(0.5),
        };
        assert_eq!(ink(half, o, s, b.0, b.1), 0.5);
        assert_eq!(block(half, o, s), 0.5);
    }

    /// The lockup's five glyphs' boxes, left to right, in frame pixels:
    /// the wordmark's four at its advance less its tracking, and the
    /// prompt's cell.
    fn glyphs(o: Pt, s: f32) -> [(f32, f32); 5] {
        let step = 62.0 * 0.6 + WORDMARK.tracking;
        let w = |i: f32| {
            let l = WORDMARK.x + i * step;
            (o.x + l * s, o.x + (l + step) * s)
        };
        [
            w(0.0),
            w(1.0),
            w(2.0),
            w(3.0),
            (o.x + PROMPT.x * s, o.x + (PROMPT.x + 120.0 * K) * s),
        ]
    }

    /// **Both passes run left to right, the block last**: at every age
    /// before the second pass each glyph is at least as drawn as the one to
    /// its right and the block the least; at every age after it lights,
    /// each is at least as gone, the block the last to go. The first frame
    /// shows the logo partly drawn, and [`OUT`] shows it whole.
    #[test]
    fn both_sweeps_run_left_to_right_the_block_last() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let g = glyphs(o, s);
        let ms = (0u16..)
            .map(|m| f32::from(m) / 1000.0)
            .take_while(|&a| a <= GONE);
        for age in ms {
            let look = SplashLook {
                phase: SplashPhase::Plasma(age),
            };
            let mut inks: Vec<f32> = g.iter().map(|&(l, r)| ink(look, o, s, l, r)).collect();
            inks.push(block(look, o, s));
            for w in inks.windows(2) {
                let ordered = match age < OUT {
                    true => w[0] + 1e-4 >= w[1],
                    false => w[0] <= w[1] + 1e-4,
                };
                assert!(ordered, "at {age}: {inks:?}");
            }
        }
        let first = SplashLook {
            phase: SplashPhase::Plasma(0.0),
        };
        let b = ink(first, o, s, g[0].0, g[0].1);
        assert!(b > 0.0 && b < 1.0, "the first frame, partly: {b}");
        let whole = SplashLook {
            phase: SplashPhase::Plasma(OUT),
        };
        for &(l, r) in &g {
            assert!(ink(whole, o, s, l, r) > 1.0 - 1e-3);
        }
        assert!(block(whole, o, s) > 0.99, "whole as the second pass lights");
        let midway = SplashLook {
            phase: SplashPhase::Plasma(HOLD / 2.0),
        };
        assert!(
            ink(midway, o, s, g[0].0, g[0].1) > ink(midway, o, s, g[4].0, g[4].1) + 0.2,
            "the left drawn before the right"
        );
    }

    /// **The logo leaves as it is drawn, with no wait between**: at 60 and
    /// 120 frames a second, no two frames in a row show every letter of
    /// `baud ❯` whole and still with the block unchanged by an 8-bit step,
    /// each letter stands whole for no more than
    /// a pass's [`HOLD`], and on the frame after the first that shows the
    /// whole logo, the block too, some of it has started to leave. Before,
    /// the letters stood whole and still from about 0.10 s to 0.37 s while
    /// the block landed. The control: a held logo is whole and still.
    #[test]
    fn the_logo_leaves_as_it_is_drawn() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let g = glyphs(o, s);
        let inks = |look: SplashLook| -> Vec<f32> {
            let mut v: Vec<f32> = g.iter().map(|&(l, r)| ink(look, o, s, l, r)).collect();
            v.push(block(look, o, s));
            v
        };
        let still = |a: &[f32], b: &[f32]| {
            a[..5]
                .iter()
                .zip(&b[..5])
                .all(|(&x, &y)| x > 1.0 - 1e-3 && (x - y).abs() < 1e-4)
                && (a[5] - b[5]).abs() < 1.0 / 255.0
        };
        let held = inks(SplashLook {
            phase: SplashPhase::Hold,
        });
        assert!(still(&held, &held), "the control");
        for rate in [60u16, 120] {
            let frames: Vec<Vec<f32>> = (0u16..)
                .map(|f| f32::from(f) / f32::from(rate))
                .take_while(|&a| a < GONE)
                .map(|a| {
                    inks(SplashLook {
                        phase: SplashPhase::Plasma(a),
                    })
                })
                .collect();
            for (f, p) in frames.windows(2).enumerate() {
                assert!(
                    !still(&p[0], &p[1]),
                    "{rate} Hz: frames {f} and {} stand whole and still: {:?}",
                    f + 1,
                    p[1]
                );
            }
            for i in 0..5 {
                let whole = frames.iter().filter(|v| v[i] > 1.0 - 1e-3).count();
                let secs = whole as f32 / f32::from(rate);
                assert!(secs <= HOLD + 1e-3, "{rate} Hz: letter {i} whole {secs} s");
            }
            let first = frames
                .iter()
                .position(|v| v.iter().all(|&x| x > 0.99))
                .expect("the logo whole on some frame");
            assert!(
                frames[first + 1]
                    .iter()
                    .zip(&frames[first])
                    .any(|(&n, &w)| n < w - 1e-3),
                "{rate} Hz: whole at frame {first}, and the next: {:?}",
                frames[first + 1]
            );
        }
    }

    /// **Every letter is drawn and taken under the plasma**: at 60 and 120
    /// frames a second, on every frame a letter of `baud ❯` grows more
    /// opaque coming in, or less going out, the plasma burns and its quad
    /// covers at least half the letter's width -- the filaments are
    /// inside the quad -- and each letter has the plasma over
    /// it for at least 0.08 s in each pass, so the plasma is on the word as
    /// well as the mark. With the box collapsing from its first frame, `b`
    /// turned whole outside it at 60 Hz and left with none over it. The
    /// control: on the first frame the quad covers every letter.
    #[test]
    fn every_letter_is_drawn_and_taken_under_the_plasma() {
        let s = scale(FRAME);
        let o = origin(FRAME, s);
        let g = glyphs(o, s);
        let over = |age: f32, (l, r): (f32, f32)| {
            let v = vfx(o, s, age);
            let xs = v.emitter.corners.0.map(|p| p.x);
            let (ql, qr) = (
                xs.iter().copied().fold(f32::INFINITY, f32::min),
                xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
            );
            v.ignition.age < crate::plasma::LIFE && (r.min(qr) - l.max(ql)) >= 0.5 * (r - l)
        };
        for &glyph in &g {
            assert!(over(0.0, glyph), "the control: {glyph:?}");
        }
        for rate in [60u16, 120] {
            let mut seen = [[0.0f32; 2]; 5];
            let mut last: Option<Vec<f32>> = None;
            for f in 0u16.. {
                let age = f32::from(f) / f32::from(rate);
                if age >= LEFT {
                    break;
                }
                let look = SplashLook {
                    phase: SplashPhase::Plasma(age),
                };
                let inks: Vec<f32> = g.iter().map(|&(l, r)| ink(look, o, s, l, r)).collect();
                let going = usize::from(age >= OUT);
                for (i, &glyph) in g.iter().enumerate() {
                    let moved = last.as_ref().is_some_and(|p| match going {
                        0 => inks[i] > p[i] + 1e-4,
                        _ => inks[i] < p[i] - 1e-4,
                    });
                    let lit = over(age, glyph);
                    assert!(
                        !moved || lit,
                        "{rate} Hz, {age} s: letter {i} {} with no plasma over it",
                        ["drawn", "taken"][going]
                    );
                    if lit {
                        seen[i][going] += 1.0 / f32::from(rate);
                    }
                }
                last = Some(inks);
            }
            for (i, both) in seen.iter().enumerate() {
                assert!(
                    both.iter().all(|&t| t >= 0.08),
                    "{rate} Hz: letter {i} under the plasma {both:?} s"
                );
            }
        }
    }
}
