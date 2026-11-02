//! **What a blinking cursor shows on a frame**: how much of it is drawn,
//! and how bright the plasma glow over its cell is. The driver's clock
//! decides both -- `baud-app`'s `caret.rs` -- and hands them over as a
//! [`CursorPhase`]; this is what every path draws from them.
//!
//! * **The opacity is the cursor's own, taken part way.** A block cursor
//!   is the cell's two colours swapped, so a block at opacity `t` is each
//!   colour `t` of the way to its swapped one ([`Argb::mix`]): on an
//!   opaque cell exactly the cursor drawn as a layer at `t` over it, glyph
//!   edges included, because both are the same bilinear mix. On glass,
//!   where the cell's background carries the window's dim, the alphas are
//!   mixed straight and it is close to that layer, not equal. A bar or an
//!   underline is its shape at `t` of its alpha ([`Argb::scaled`]). The cell
//!   shader spells both in the same integers, so the pixel oracle holds the
//!   two GPU arms to the bit.
//! * **The glow is the plasma trail's sheath and tube on the cursor cell
//!   alone**: `crate::plasma`'s `TUBE`, `SHEATH` and `DECAY`, in the
//!   cursor's colour, as nested solids from the cell's edge in. Solids
//!   because every path draws a solid the same way -- the CPU, the instance
//!   arm and the cell arm's records -- where a trail's effect is drawn by
//!   its shader on the GPU and by its own per-pixel model on the CPU.
//! * **The fire burns the cursor down into embers and relights it**
//!   ([`Play::Fire`]): the fire trail's language (`crate::fire`) -- a flame
//!   in the cursor's colour, heat as opacity, embers that rise, flutter and
//!   cool -- on the cursor's own shape, in solids for the same reason, its
//!   times fractions of the hidden phase where the trail's are seconds.
//! * **The ufo lifts the cursor away on a beam and sets it back down**
//!   ([`Play::Ufo`]): the ufo trail's language (`crate::ufo`) -- a small
//!   saucer with a rim light, a translucent tractor beam at its `BEAM`
//!   opacity, brighter in the core, rings travelling along it -- inside the
//!   cursor's cell, the cursor's shape drawn up into the saucer and back.
//! * **Every blink is the same bits on every OS**: the glow's exponential
//!   and the fire's sines are [`crate::det`]'s, as the trails' are, not the
//!   platform's libm, whose answers differ between macOS and glibc
//!   (`every_blink_is_drawn_from_the_same_bits_on_every_os`).

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::det::Det;
use crate::effect::Hash;
use crate::instance::{Instance, Screen};
use crate::plasma::{DECAY, SHEATH, TUBE};
use crate::renderer::Blink;
use baud_domain::Chan;
use baud_domain::contrast::CurveRow;
use baud_font::Px;

/// A channel's top value, spelled as a constant: `Chan::from_offset` is the
/// one `const` constructor.
const FULL: Chan = Chan::from_offset(255, 0);
const NONE: Chan = Chan::from_offset(0, 0);

/// A blinking cursor on one frame: how much of it is drawn, and the glow
/// over its cell. A steady cursor ignores both and is drawn whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorPhase {
    /// 255 the whole cursor, 0 none of it: a block's swap, or a bar's or
    /// an underline's shape, taken this far.
    pub opacity: Chan,
    /// The plasma glow's pulse, 0 none: its brightness and how far its
    /// core has swelled across the cell, together.
    pub glow: Chan,
    /// A blink animation drawn as the cursor's own shape, and how far
    /// through the hidden phase it is.
    pub play: Play,
}

/// **A blink animation that moves the cursor's own shape**, at how far
/// through the hidden phase it is: 0 its start, 255 its end. Its frames are
/// solids over the cell ([`play`]); the cursor's own opacity, which hands
/// over to them and back, is the phase's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Play {
    /// None: the opacity and the glow are the whole of it.
    Still,
    /// The fire: the cursor burns down from the top into embers, and
    /// relights from the bottom.
    Fire(Chan),
    /// The ufo: a saucer's beam lifts the cursor away and sets it down.
    Ufo(Chan),
}

impl CursorPhase {
    /// The whole cursor and no glow: every frame of a cursor that does not
    /// blink, and a blink's on phase.
    pub const SHOWN: CursorPhase = CursorPhase {
        opacity: FULL,
        glow: NONE,
        play: Play::Still,
    };
    /// No cursor and no glow: a hard blink's off phase.
    pub const HIDDEN: CursorPhase = CursorPhase {
        opacity: NONE,
        glow: NONE,
        play: Play::Still,
    };

    /// **The fire blink `at` of the way through the hidden phase**: the
    /// cursor's opacity handing over to the flame's solids over the first
    /// [`HANDOVER`] and back over the last, gone between. Its two ends are
    /// the whole cursor and nothing else, so the flips into and out of the
    /// hidden phase meet it at rest.
    pub fn fire(at: Chan) -> CursorPhase {
        CursorPhase::handing(at, Play::Fire(at))
    }

    /// **The ufo blink `at` of the way through the hidden phase**, handed
    /// over and back as the fire is, so its ends are the whole cursor too.
    pub fn ufo(at: Chan) -> CursorPhase {
        CursorPhase::handing(at, Play::Ufo(at))
    }

    /// An effect that takes the cursor over at `at`: the cursor's own
    /// opacity [`handed`] to its solids, and none of it at either end.
    fn handing(at: Chan, play: Play) -> CursorPhase {
        if at == NONE || at == FULL {
            return CursorPhase::SHOWN;
        }
        CursorPhase {
            opacity: Chan::from_unit(handed(unit(at))),
            glow: NONE,
            play,
        }
    }
}

/// The share of the hidden phase an effect takes, at each end, to hand
/// the cursor over to its solids and back: a cross-fade, so a block's glyph
/// fades rather than vanishes.
pub const HANDOVER: f32 = 0.08;

/// How much of the cursor itself is drawn `u` through a hidden phase that
/// hands it to an effect: all of it at both ends, eased.
fn handed(u: f32) -> f32 {
    if u < HANDOVER {
        1.0 - ease(u / HANDOVER)
    } else if u >= 1.0 - HANDOVER {
        ease((u - (1.0 - HANDOVER)) / HANDOVER)
    } else {
        0.0
    }
}

/// Smoothstep from 0 to 1.
fn ease(s: f32) -> f32 {
    let s = s.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// Smoothstep from `e0` to `e1`.
fn step(e0: f32, e1: f32, x: f32) -> f32 {
    ease((x - e0) / (e1 - e0))
}

/// A channel as a unit fraction.
fn unit(c: Chan) -> f32 {
    f32::from(c.get()) / 255.0
}

impl Default for CursorPhase {
    fn default() -> CursorPhase {
        CursorPhase::SHOWN
    }
}

impl From<Blink> for CursorPhase {
    /// A hard blink's two phases.
    fn from(b: Blink) -> CursorPhase {
        match b {
            Blink::Shown => CursorPhase::SHOWN,
            // The caret's off phase has no faint look: the system's caret
            // settings, not Dim Flashing Lights, are what it follows.
            Blink::Hidden | Blink::Faint => CursorPhase::HIDDEN,
        }
    }
}

/// A count of pixels as an `f32`: every cell dimension fits a `u16`.
fn px_f32(v: Px) -> f32 {
    f32::from(u16::try_from(v.value()).unwrap_or(u16::MAX))
}

/// **The plasma blink's glow over a cell** `(left, top, w, h)` at pulse
/// `level`, in `fill`, appended to `out`.
///
/// The pulse `p` is `level` of 255. The core is the cell shrunk about its
/// centre to `p` of its size, at `p (TUBE + SHEATH)`; outside it the
/// sheath falls off as `p SHEATH e^(-DECAY d)`, `d` in cell heights, as the
/// trail's does past its quad -- and it stops at the cell's edge. So the
/// glow swells from a dim point to a cell-sized ghost of the cursor at 0.28
/// and back as `p` rises and falls.
///
/// One solid a pixel of depth from the edge, each covering everything
/// inside it and adding the opacity that brings the stack to the profile
/// there: `1 - (1 - A_i) / (1 - A_(i-1))` over the rings outside it. The
/// profile only rises inward, so no ring takes away. A solid whose share
/// rounds to nothing is left out, so a full core is one solid.
pub(crate) fn glow(cell: (Px, Px, Px, Px), level: Chan, fill: Argb, out: &mut Vec<Instance>) {
    let p = f32::from(level.get()) / 255.0;
    let (left, top, w, h) = (
        cell.0.value(),
        cell.1.value(),
        cell.2.value(),
        cell.3.value(),
    );
    if p <= 0.0 || w == 0 || h == 0 {
        return;
    }
    let rings = w.min(h).div_ceil(2);
    let core = (1.0 - p) * px_f32(Px::of(w.min(h))) / 2.0;
    let unit = px_f32(Px::of(h));
    let alpha = f32::from(fill.a().get()) / 255.0;
    let mut outside = 0.0f32;
    for i in 0..rings {
        let d = (core - px_f32(Px::of(i))).max(0.0);
        let tube = if d <= 0.0 { TUBE } else { 0.0 };
        let want = p * (SHEATH * (-DECAY * d / unit).det_exp() + tube);
        let share = 1.0 - (1.0 - want) / (1.0 - outside);
        outside = want;
        let v = share * alpha;
        #[cfg(test)]
        drawn::fold(&[v]);
        let a = Chan::from_unit(v);
        if a == NONE {
            continue;
        }
        let at = |v: u32| Screen(i16::try_from(v).unwrap_or(i16::MAX));
        let len = |v: u32| Tex(u16::try_from(v).unwrap_or(u16::MAX));
        out.push(Instance {
            x: at(left + i),
            y: at(top + i),
            w: len(w - 2 * i),
            h: len(h - 2 * i),
            u: Tex(0),
            v: Tex(0),
            color: fill.with_alpha(a),
            kind: Kind::Solid,
            curve: CurveRow::default(),
        });
    }
}

/// A cell or a cursor shape in pixels, edges as floats: left, top, right,
/// bottom.
#[derive(Clone, Copy, Debug)]
struct Box4 {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

impl Box4 {
    fn of(v: (Px, Px, Px, Px)) -> Box4 {
        let (l, t) = (px_f32(v.0), px_f32(v.1));
        Box4 {
            l,
            t,
            r: l + px_f32(v.2),
            b: t + px_f32(v.3),
        }
    }

    fn w(self) -> f32 {
        self.r - self.l
    }

    fn h(self) -> f32 {
        self.b - self.t
    }
}

/// **One solid of an effect**, edges rounded to whole pixels and cut to
/// `cell`, in `fill` at `a` of its alpha; nothing where either is empty.
fn solid(out: &mut Vec<Instance>, cell: Box4, edges: Box4, fill: Argb, a: f32) {
    let v = a * unit(fill.a());
    #[cfg(test)]
    drawn::fold(&[edges.l, edges.t, edges.r, edges.b, v]);
    let alpha = Chan::from_unit(v);
    let (l, t) = (edges.l.max(cell.l).round(), edges.t.max(cell.t).round());
    let (r, b) = (edges.r.min(cell.r).round(), edges.b.min(cell.b).round());
    if alpha == NONE || r <= l || b <= t {
        return;
    }
    // Whole pixels inside a cell, so every conversion holds.
    let at = |v: f32| Screen(i16::try_from(v as i32).unwrap_or(i16::MAX));
    let len = |v: f32| Tex(u16::try_from(v as i32).unwrap_or(u16::MAX));
    out.push(Instance {
        x: at(l),
        y: at(t),
        w: len(r - l),
        h: len(b - t),
        u: Tex(0),
        v: Tex(0),
        color: fill.with_alpha(alpha),
        kind: Kind::Solid,
        curve: CurveRow::default(),
    });
}

/// **The animation `play` over the cursor's `cell`**, its `shape` the cursor
/// as its style draws it, in `fill`, appended to `out`.
pub(crate) fn play(
    cell: (Px, Px, Px, Px),
    shape: (Px, Px, Px, Px),
    play: Play,
    fill: Argb,
    out: &mut Vec<Instance>,
) {
    let (cell, shape) = (Box4::of(cell), Box4::of(shape));
    if cell.w() <= 0.0 || cell.h() <= 0.0 {
        return;
    }
    match play {
        Play::Still => {}
        Play::Fire(at) => fire(cell, shape, unit(at), fill, out),
        Play::Ufo(at) => ufo(cell, shape, unit(at), fill, out),
    }
}

/// When, as shares of the hidden phase, the ufo has the cursor up -- it
/// lifts from the end of the handover to the first, holds, and sets it
/// down by `1 - HANDOVER`.
pub const LIFTED: (f32, f32) = (0.42, 0.58);

/// The rings travelling along the beam.
const RINGS: u16 = 3;

/// The beam's bands from the saucer down, each as wide as the beam is
/// half way down it.
const BANDS: u16 = 6;

/// **The ufo blink at `u` of the hidden phase.** A saucer -- a dome over a
/// hull, a rim light running along it -- at the top of the cell, lowering a
/// beam onto the cursor with rings rising along it; the cursor's shape,
/// handed over from the cursor, drawn up the beam into the saucer, shrinking
/// and fading, and back down with the rings falling, the beam closing as it
/// lands and the cursor handed back.
fn ufo(cell: Box4, shape: Box4, u: f32, fill: Argb, out: &mut Vec<Instance>) {
    use crate::ufo::BEAM;
    let c = cell.h();
    let mid = (cell.l + cell.r) / 2.0;
    // The saucer and its beam come and go at the phase's ends.
    let here = step(0.0, 0.12, u) * (1.0 - step(0.88, 1.0, u));
    let (dome, hull) = saucer(cell);
    // The beam: from under the hull to the cursor's foot, lowered at the
    // start and drawn up at the end, as wide as the trail's at both ends
    // and cut to the cell.
    let reach = step(0.02, 0.12, u) * (1.0 - step(0.9, 1.0, u));
    let (top, foot) = (hull.b, shape.b);
    let len = (foot - top) * reach;
    if len > 0.0 && here > 0.0 {
        let top_half = 0.14 * c * crate::ufo::SIZE;
        let foot_half = (0.5 * shape.w() + 0.08 * c).max(0.22 * c);
        let band = len / f32::from(BANDS);
        for k in 0..BANDS {
            let ff = (f32::from(k) + 0.5) / f32::from(BANDS);
            let half = top_half + (foot_half - top_half) * ff;
            let rows = Box4 {
                l: mid - half,
                r: mid + half,
                t: top + band * f32::from(k),
                b: top + band * f32::from(k + 1),
            };
            let a = BEAM * here * (0.6 + 0.4 * ff);
            solid(out, cell, rows, fill, 0.55 * a);
            let core = Box4 {
                l: mid - 0.4 * half,
                r: mid + 0.4 * half,
                ..rows
            };
            solid(out, cell, core, fill, 0.45 * a);
        }
        // Rings rising while it lifts and falling while it sets down.
        let travel = 2.0 * (0.5 - (u - 0.5).abs());
        let thick = thick_of(c);
        for i in 0..RINGS {
            let along = (f32::from(i) / f32::from(RINGS) + travel).fract();
            let y = foot - along * len;
            let ff = (y - top) / (foot - top).max(1.0);
            let half = top_half + (foot_half - top_half) * ff;
            let ring = Box4 {
                l: mid - half,
                r: mid + half,
                t: y - thick,
                b: y,
            };
            solid(out, cell, ring, fill, 0.9 * BEAM * here);
        }
    }
    let (body, a) = lifted(cell, shape, u);
    solid(out, cell, body, fill, a);
    // The saucer over all of it, its light running along the rim.
    if here > 0.0 {
        solid(out, cell, dome, fill, 0.5 * here);
        solid(out, cell, hull, fill, 0.8 * here);
        let at = hull.l + (2.0 * u).fract() * hull.w();
        let light = Box4 {
            l: at,
            r: at + thick_of(c),
            ..hull
        };
        solid(out, cell, light, fill, here);
    }
}

/// The saucer at the top of `cell`: its dome, and the hull under it,
/// the trail's `SIZE` across the cell.
fn saucer(cell: Box4) -> (Box4, Box4) {
    use crate::ufo::SIZE;
    let (c, mid) = (cell.h(), (cell.l + cell.r) / 2.0);
    let dome = Box4 {
        l: mid - 0.15 * SIZE * cell.w(),
        r: mid + 0.15 * SIZE * cell.w(),
        t: cell.t + 1.0,
        b: cell.t + 1.0 + (c / 16.0).max(1.0),
    };
    let hull = Box4 {
        l: mid - 0.35 * SIZE * cell.w(),
        r: mid + 0.35 * SIZE * cell.w(),
        t: dome.b,
        b: dome.b + (c / 12.0).max(1.0),
    };
    (dome, hull)
}

/// **The cursor as the beam holds it at `u`**, its edges and its share of
/// the alpha: handed over in place, its top drawn up to the saucer's hull
/// as it shrinks about its centre to 0.3 of its size and fades, held, and
/// set back down and handed back.
fn lifted(cell: Box4, shape: Box4, u: f32) -> (Box4, f32) {
    let hull = saucer(cell).1;
    let lift = if u < HANDOVER {
        0.0
    } else if u < LIFTED.0 {
        ease((u - HANDOVER) / (LIFTED.0 - HANDOVER))
    } else if u < LIFTED.1 {
        1.0
    } else {
        1.0 - ease((u - LIFTED.1) / (1.0 - HANDOVER - LIFTED.1))
    };
    let shown = if u < HANDOVER {
        ease(u / HANDOVER)
    } else if u >= 1.0 - HANDOVER {
        1.0 - ease((u - (1.0 - HANDOVER)) / HANDOVER)
    } else {
        1.0
    };
    let k = 1.0 - 0.7 * lift;
    let (w, h) = ((shape.w() * k).max(1.0), (shape.h() * k).max(1.0));
    let cx = (shape.l + shape.r) / 2.0;
    let t = shape.t + (hull.b - shape.t) * lift;
    let body = Box4 {
        l: cx - w / 2.0,
        r: cx + w / 2.0,
        t,
        b: t + h,
    };
    (body, shown * (1.0 - 0.85 * lift))
}

/// A line one pixel a 34-pixel cell thick, and never under one.
fn thick_of(c: f32) -> f32 {
    (c / 34.0).max(1.0)
}

/// When, as shares of the hidden phase, the fire's front leaves the top of
/// the cursor and reaches its bottom, and when the relit flame starts up
/// from the bottom; it is whole again at `1 - HANDOVER`.
pub const BURN: (f32, f32) = (HANDOVER, 0.45);
pub const RELIGHT: f32 = 0.62;

/// The flame's tongues above the front at their tallest, in cell heights:
/// the trail's `HEIGHT` of 1.6 cells, which a cell cannot hold, scaled to
/// it.
const TONGUE: f32 = 0.35;

/// The embers the burn sheds, and the times a tongue flickers in a phase.
const EMBERS: u32 = 8;
const FLICKER: f32 = 3.0;

/// The front's heat as it moves, `p` of the way: rising as it catches and
/// dying as it lands, so a tongue never jumps on or off.
fn heat(p: f32) -> f32 {
    step(0.0, 0.15, p) * (1.0 - step(0.8, 1.0, p))
}

/// **The fire blink at `u` of the hidden phase.** The cursor's shape as a
/// solid in the cursor's colour, handed over from the cursor itself; its
/// top burns down to its bottom with tongues of flame above the front,
/// shedding embers that rise, flutter and cool, as the fire trail's do; then
/// it grows back up from the bottom under the same tongues and hands back.
fn fire(cell: Box4, shape: Box4, u: f32, fill: Argb, out: &mut Vec<Instance>) {
    // The body: what of the cursor's shape is unburnt, and how much of it.
    let (top, a, p) = if u < BURN.0 {
        (shape.t, ease(u / BURN.0), None)
    } else if u < BURN.1 {
        let p = (u - BURN.0) / (BURN.1 - BURN.0);
        (shape.t + ease(p) * shape.h(), 1.0, Some(p))
    } else if u < RELIGHT {
        (shape.b, 0.0, None)
    } else if u < 1.0 - HANDOVER {
        let p = (u - RELIGHT) / (1.0 - HANDOVER - RELIGHT);
        (shape.b - ease(p) * shape.h(), 1.0, Some(p))
    } else {
        (shape.t, 1.0 - ease((u - (1.0 - HANDOVER)) / HANDOVER), None)
    };
    solid(out, cell, Box4 { t: top, ..shape }, fill, a);
    // The tongues over the front, a few across the shape, each flickering
    // on its own phase: a halo and a hotter core inside it.
    if let Some(p) = p {
        let hot = heat(p);
        let n = (shape.w() / 4.0).floor().clamp(1.0, 4.0);
        for j in (0..4u16).take_while(|&j| f32::from(j) < n) {
            let phase = Hash(0x0f1e_u32 ^ u32::from(j).wrapping_mul(0x9E37_79B9))
                .next()
                .unit();
            let s = (core::f32::consts::PI * (FLICKER * u + phase)).det_sin();
            let tall = TONGUE * cell.h() * hot * (0.55 + 0.45 * s * s);
            let w = shape.w() / n;
            let lane = Box4 {
                l: shape.l + w * f32::from(j),
                r: shape.l + w * f32::from(j + 1),
                t: top - tall,
                b: top,
            };
            solid(out, cell, lane, fill, 0.30);
            solid(
                out,
                cell,
                Box4 {
                    t: top - 0.55 * tall,
                    ..lane
                },
                fill,
                0.45,
            );
        }
    }
    // The embers, each born where the front passes during the burn and
    // gone before the cursor is whole.
    for k in 0..EMBERS {
        let h0 = Hash(0x00e6_b325 ^ k.wrapping_mul(0x9E37_79B9)).next();
        let (h1, h2) = (h0.next(), h0.next().next());
        let h3 = h2.next();
        let born = BURN.0 + (BURN.1 - BURN.0) * h0.unit();
        let life = 0.18 + 0.17 * h1.unit();
        let age = (u - born) / life;
        if !(0.0..1.0).contains(&age) {
            continue;
        }
        let at_birth = shape.t + ease((born - BURN.0) / (BURN.1 - BURN.0)) * shape.h();
        let cool = 1.0 - age;
        let rise = (0.25 + 0.35 * h3.unit()) * cell.h() * (1.0 - (1.0 - age) * (1.0 - age));
        let flutter =
            0.15 * cell.w() * (core::f32::consts::TAU * (1.2 * age + h1.unit())).det_sin();
        let x = shape.l + h2.unit() * shape.w() + flutter;
        let y = at_birth - rise;
        let size = (cell.h() / 14.0 * (0.6 + 0.4 * cool)).max(1.0);
        let ember = Box4 {
            l: x - size / 2.0,
            r: x + size / 2.0,
            t: y - size / 2.0,
            b: y + size / 2.0,
        };
        solid(out, cell, ember, fill, cool * (0.55 + 0.45 * cool));
    }
}

/// **The `f32`s a test's thread drew the blinks from**, folded FNV-1a: the
/// alpha of each ring of the glow, and each solid's edges and alpha, before
/// they are rounded to whole pixels and to a channel -- where a sine or an
/// exponential one ulp apart lands first, when rounding would hide it from
/// the frame. Tests only.
#[cfg(test)]
mod drawn {
    use std::cell::Cell;

    std::thread_local! {
        static BITS: Cell<u64> = const { Cell::new(0) };
    }

    pub(super) fn fold(vs: &[f32]) {
        BITS.with(|h| {
            let mut d = h.get();
            for v in vs {
                for b in v.to_bits().to_le_bytes() {
                    d = (d ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
                }
            }
            h.set(d);
        });
    }

    /// The digest of what `f` drew.
    pub(super) fn of(f: impl FnOnce()) -> u64 {
        BITS.with(|h| h.set(0xcbf2_9ce4_8422_2325));
        f();
        BITS.with(Cell::get)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: (Px, Px, Px, Px) = (Px::of(16), Px::of(34), Px::of(16), Px::of(34));

    fn stack(level: Chan) -> Vec<Instance> {
        let mut out = Vec::new();
        glow(CELL, level, Argb(0xFF_FF_FF_FF), &mut out);
        out
    }

    /// **The glow stays on its cell**: every solid is inside it, at every
    /// pulse, and nothing is drawn at a pulse of zero.
    #[test]
    fn the_glow_stays_on_the_cursor_cell() {
        assert!(stack(Chan::from_u8(0)).is_empty());
        for level in 1..=255u8 {
            for s in stack(Chan::from_u8(level)) {
                let (x, y) = (i32::from(s.x.0), i32::from(s.y.0));
                let (w, h) = (i32::from(s.w.0), i32::from(s.h.0));
                assert!(
                    x >= 16 && y >= 34 && x + w <= 32 && y + h <= 68,
                    "{level}: {s:?}"
                );
                assert_eq!(s.kind, Kind::Solid, "every path draws a solid alike");
            }
        }
    }

    /// The cursor's three shapes on [`CELL`]: the block, an underline and a
    /// bar two pixels thick, as `Renderer::cursor_px` places them.
    const SHAPES: [(Px, Px, Px, Px); 3] = [
        CELL,
        (Px::of(16), Px::of(66), Px::of(16), Px::of(2)),
        (Px::of(16), Px::of(34), Px::of(2), Px::of(34)),
    ];

    fn played(shape: (Px, Px, Px, Px), p: Play) -> Vec<Instance> {
        let mut out = Vec::new();
        play(CELL, shape, p, Argb(0xFF_FF_FF_FF), &mut out);
        out
    }

    fn inside(s: &Instance) -> bool {
        let (x, y) = (i32::from(s.x.0), i32::from(s.y.0));
        let (w, h) = (i32::from(s.w.0), i32::from(s.h.0));
        w > 0 && h > 0 && x >= 16 && y >= 34 && x + w <= 32 && y + h <= 68
    }

    /// **The fire stays on its cell**, for every shape at every level, all
    /// solids; and the phase's two ends are the whole cursor and nothing
    /// else, so the flips meet it at rest.
    #[test]
    fn the_fire_stays_on_the_cursor_cell_and_rests_at_its_ends() {
        for shape in SHAPES {
            for level in 0..=255u8 {
                for s in played(shape, Play::Fire(Chan::from_u8(level))) {
                    assert!(inside(&s), "{shape:?} {level}: {s:?}");
                    assert_eq!(s.kind, Kind::Solid, "every path draws a solid alike");
                }
            }
        }
        assert_eq!(CursorPhase::fire(Chan::from_u8(0)), CursorPhase::SHOWN);
        assert_eq!(CursorPhase::fire(Chan::from_u8(255)), CursorPhase::SHOWN);
        assert_eq!(CursorPhase::fire(Chan::from_u8(128)).opacity, NONE);
    }

    /// **The fire burns down, sheds embers, and relights from the bottom**:
    /// the unburnt body's top falls through the burn and rises through the
    /// relight; between them only embers, small and above where the front
    /// passed; and the cursor's own opacity hands over to the body at the
    /// start and back at the end.
    #[test]
    fn the_fire_burns_down_into_embers_and_relights() {
        let at = |u: f32| Play::Fire(Chan::from_unit(u));
        // The body is the one solid reaching the shape's bottom edge.
        let body_top = |u: f32| {
            played(CELL, at(u))
                .iter()
                .filter(|s| i32::from(s.y.0) + i32::from(s.h.0) == 68 && s.w.0 == 16)
                .map(|s| s.y.0)
                .min()
        };
        let burn: Vec<_> = [0.12, 0.2, 0.3, 0.4].map(body_top).to_vec();
        assert!(
            burn.windows(2).all(|w| w[0] < w[1]),
            "burning down: {burn:?}"
        );
        let relight: Vec<_> = [0.66, 0.72, 0.8, 0.88].map(body_top).to_vec();
        assert!(
            relight.windows(2).all(|w| w[0] > w[1]),
            "relighting up: {relight:?}"
        );
        let embers = played(CELL, at(0.53));
        assert!(!embers.is_empty(), "embers between");
        for e in &embers {
            assert!(e.w.0 <= 3 && e.h.0 <= 3, "an ember is small: {e:?}");
        }
        assert_eq!(body_top(0.53), None, "nothing unburnt between");
        let opacity = |u: f32| CursorPhase::fire(Chan::from_unit(u)).opacity.get();
        assert!(opacity(0.02) > 200 && opacity(0.06) < 60, "handed over");
        assert!(opacity(0.94) < 60 && opacity(0.99) > 200, "handed back");
        // A bar burns as a bar: its body is the bar's two pixels wide.
        assert!(
            played(SHAPES[2], at(0.2))
                .iter()
                .any(|s| s.w.0 == 2 && i32::from(s.y.0) + i32::from(s.h.0) == 68),
            "the bar's own body"
        );
    }

    /// **The ufo stays on its cell**, for every shape at every level, all
    /// solids; and its two ends are the whole cursor and nothing else.
    #[test]
    fn the_ufo_stays_on_the_cursor_cell_and_rests_at_its_ends() {
        for shape in SHAPES {
            for level in 0..=255u8 {
                for s in played(shape, Play::Ufo(Chan::from_u8(level))) {
                    assert!(inside(&s), "{shape:?} {level}: {s:?}");
                    assert_eq!(s.kind, Kind::Solid, "every path draws a solid alike");
                }
            }
        }
        assert_eq!(CursorPhase::ufo(Chan::from_u8(0)), CursorPhase::SHOWN);
        assert_eq!(CursorPhase::ufo(Chan::from_u8(255)), CursorPhase::SHOWN);
        assert_eq!(CursorPhase::ufo(Chan::from_u8(128)).opacity, NONE);
    }

    /// **The beam lifts the cursor away and sets it back down**: in each
    /// shape the lowest solid's foot -- the cursor's, under the beam's --
    /// rises through the lift and falls through the set-down, held up
    /// between; the beam reaches the cursor's foot while it lifts; the
    /// saucer sits at the cell's top; and the cursor's own opacity hands
    /// over and back as the fire's does.
    #[test]
    fn the_ufo_lifts_the_cursor_and_sets_it_down() {
        let at = |u: f32| Play::Ufo(Chan::from_unit(u));
        // The cursor's body, which the drawing holds as one solid.
        let body_foot = |shape: (Px, Px, Px, Px), u: f32| {
            let (b, _) = lifted(Box4::of(CELL), Box4::of(shape), unit(Chan::from_unit(u)));
            let (t, f) = (b.t.round(), b.b.round());
            let drawn = played(shape, at(u));
            assert!(
                drawn
                    .iter()
                    .any(|s| f32::from(s.y.0) == t && f32::from(s.y.0) + f32::from(s.h.0) == f),
                "{shape:?} at {u}: the body {b:?} is drawn"
            );
            f
        };
        for shape in SHAPES {
            let up: Vec<_> = [0.1, 0.2, 0.3, 0.4].map(|u| body_foot(shape, u)).to_vec();
            assert!(
                up.windows(2).all(|w| w[0] > w[1]),
                "{shape:?} lifting: {up:?}"
            );
            let down: Vec<_> = [0.62, 0.7, 0.8, 0.9].map(|u| body_foot(shape, u)).to_vec();
            assert!(
                down.windows(2).all(|w| w[0] < w[1]),
                "{shape:?} setting down: {down:?}"
            );
        }
        // The beam reaches the cursor's foot, at its own opacity's order.
        let lifting = played(CELL, at(0.25));
        assert!(
            lifting
                .iter()
                .any(|s| i32::from(s.y.0) + i32::from(s.h.0) == 68 && s.color.a().get() < 100),
            "the beam on the cursor: {lifting:?}"
        );
        // The saucer at the top, and nothing of it at the very ends.
        let held = played(CELL, at(0.5));
        assert!(
            held.iter()
                .any(|s| s.y.0 <= 38 && s.h.0 <= 3 && s.color.a().get() > 150),
            "the hull: {held:?}"
        );
        let opacity = |u: f32| CursorPhase::ufo(Chan::from_unit(u)).opacity.get();
        assert!(opacity(0.02) > 200 && opacity(0.06) < 60, "handed over");
        assert!(opacity(0.94) < 60 && opacity(0.99) > 200, "handed back");
    }

    /// **At the full pulse the core fills the cell at the trail's tube and
    /// sheath**, one solid at `TUBE + SHEATH`; a weaker pulse is dimmer at
    /// the centre and dimmer still at the edge.
    #[test]
    fn the_full_pulse_is_one_solid_at_tube_and_sheath() {
        let full = stack(Chan::from_u8(255));
        assert_eq!(full.len(), 1, "{full:?}");
        assert_eq!(full[0].color.a(), Chan::from_unit(TUBE + SHEATH));
        let half = stack(Chan::from_u8(128));
        let edge = half[0].color.a().get();
        let centre: f32 = half
            .iter()
            .map(|s| f32::from(s.color.a().get()) / 255.0)
            .fold(0.0, |acc, a| acc + a * (1.0 - acc));
        assert!(f32::from(edge) / 255.0 < centre, "{half:?}");
        assert!(centre < TUBE + SHEATH);
    }

    /// **Every blink is drawn from the same bits on every OS**, which its
    /// frames cannot show alone: a solid's edges round to whole pixels and
    /// its alpha to a channel, and most one-ulp differences round away.
    /// Here the glow at every pulse, and the fire and the ufo at every
    /// level in each of the cursor's three shapes, on three cell sizes,
    /// fold every `f32` they are drawn from (`drawn::fold`) into a digest
    /// pinned on macOS and required of every target. Their sine and
    /// exponential are `det`'s (`the_trails_call_no_platform_math`), whose
    /// bits are the same on every machine: with std's in their place, the
    /// glow on the largest cell differs between macOS and glibc 2.36 on
    /// aarch64, and with `det`'s none of the 21 does.
    #[test]
    fn every_blink_is_drawn_from_the_same_bits_on_every_os() {
        // Per cell, the 14-pixel font's, the 26's and the 48's: the glow,
        // then the fire and the ufo in the block, the underline and the bar.
        const PINNED: &[u64] = &[
            0x9e4486a2fa612295,
            0x8ed11bcd1d9ba6a2,
            0xc28c60de299cae28,
            0x3e8d431fd8991f54,
            0x7a176f6527cd9e3c,
            0x4024f43b496f6a0e,
            0x8786998edef8a528,
            0x4d6cf5702f3bc61a,
            0x05141f0edac879b3,
            0xbc2218a022050d6f,
            0xcf0ca5ab6c99e79d,
            0xddef64341a474d6a,
            0x6cc31d09b95d106f,
            0x31bbc83db0adea9e,
            0xd03109a0dfcf6a25,
            0x1e835d9042401d7d,
            0x86b314734dc1aaa3,
            0x73a728d00915773c,
            0x2fa94b1547a526c4,
            0x077c4126dc3934b7,
            0x1efdc36287b9fb6c,
        ];
        let mut got = Vec::new();
        for (w, h) in [(8u32, 18u32), (16, 34), (29, 63)] {
            let (l, t) = (Px::of(3 * w), Px::of(2 * h));
            let cell = (l, t, Px::of(w), Px::of(h));
            let shapes = [
                cell,
                (l, Px::of(2 * h + h - 2), Px::of(w), Px::of(2)),
                (l, t, Px::of(2), Px::of(h)),
            ];
            let fill = Argb(0xFF_E0_C0_A0);
            let mut out = Vec::new();
            got.push(drawn::of(|| {
                for level in 1..=255u8 {
                    glow(cell, Chan::from_u8(level), fill, &mut out);
                }
            }));
            for anim in [Play::Fire, Play::Ufo] {
                for shape in shapes {
                    got.push(drawn::of(|| {
                        for level in 0..=255u8 {
                            play(cell, shape, anim(Chan::from_u8(level)), fill, &mut out);
                        }
                    }));
                }
            }
            // The positive control: the arms drew solids.
            assert!(out.len() > 4 * 255, "{w}x{h}: {} solids", out.len());
        }
        // And every arm folded something.
        assert!(got.iter().all(|&d| d != 0xcbf2_9ce4_8422_2325), "{got:x?}");
        let table: String = got
            .iter()
            .map(|d| format!("            0x{d:016x},\n"))
            .collect();
        assert!(
            got == PINNED,
            "the blinks' bits moved; the table this target computes:\n{table}"
        );
    }
}
