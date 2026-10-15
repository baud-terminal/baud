//! **What a blinking cursor shows on a frame**: how much of it is drawn,
//! and how bright the plasma glow over its cell is. The driver's clock
//! decides both -- `baud-app`'s `caret.rs`, the owner's picks of
//! 2026-10-15 -- and hands them over as a [`CursorPhase`]; this is what
//! every path draws from them.
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
//!   arm and the cell arm's records -- where the trail's own effect
//!   pipeline is the GPU's alone.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
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
}

impl CursorPhase {
    /// The whole cursor and no glow: every frame of a cursor that does not
    /// blink, and a blink's on phase.
    pub const SHOWN: CursorPhase = CursorPhase {
        opacity: FULL,
        glow: NONE,
    };
    /// No cursor and no glow: a hard blink's off phase.
    pub const HIDDEN: CursorPhase = CursorPhase {
        opacity: NONE,
        glow: NONE,
    };
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
            Blink::Hidden => CursorPhase::HIDDEN,
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
        let want = p * (SHEATH * (-DECAY * d / unit).exp() + tube);
        let share = 1.0 - (1.0 - want) / (1.0 - outside);
        outside = want;
        let a = Chan::from_unit(share * alpha);
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
}
