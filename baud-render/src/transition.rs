//! The alternate-screen transition: a veil in the default background colour
//! over the new screen, fading to reveal it, when the terminal switches
//! between the primary and alternate screen. Off by default; sfte's
//! `SFTE_TERM_ANIMATE_SCREEN`, built to baud's blink standard.
//!
//! It is a fade *in from the background* rather than a crossfade: the new
//! screen is already drawn, and the veil ramps to nothing over it, so no
//! capture of the outgoing frame is needed -- one translucent full-screen
//! solid, the scrollbar's shape, driven by a `progress` the driver computes
//! from the display link's timestamp the way [`crate::Blink`] is. A
//! crossfade of the old screen into the new would need the outgoing frame
//! held and blended, which is a present-path change this is not.
//!
//! A screen switch changes every cell, so unlike the cursor trail the veil
//! is not bounded to a band -- it covers the frame, and every row is dirty
//! while it fades. That is honest: the whole screen is what is changing.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::Chan;
use baud_font::Px;

/// A screen switch being animated: how far along the fade the driver has
/// advanced it -- 0.0 fully veiled, 1.0 clear and the new screen settled.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Transition {
    pub progress: f32,
}

/// The veil for a frame: a full-screen rectangle at the alpha the fade is
/// at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Veil {
    pub w: Px,
    pub h: Px,
    pub alpha: Chan,
}

/// The alpha the veil starts at, before the fade.
const PEAK_ALPHA: u8 = 0xE0;

/// The veil for `t` over a frame `w` by `h`: full-screen, at an alpha that
/// fades from the peak to zero as the new screen settles.
pub fn veil(t: Transition, w: Px, h: Px) -> Veil {
    let p = t.progress.clamp(0.0, 1.0);
    let fade = (f32::from(PEAK_ALPHA) * (1.0 - p)).round().max(0.0) as i64;
    Veil {
        w,
        h,
        alpha: Chan::from_u8(u8::try_from(fade).unwrap_or(0)),
    }
}

/// The veil as one translucent solid every path draws, in the default
/// background `bg` at its faded alpha, at the frame origin.
pub fn instance(v: Veil, bg: Argb) -> Instance {
    Instance {
        x: Screen(0),
        y: Screen(0),
        w: Tex(u16::try_from(v.w.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(v.h.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color: bg.with_alpha(v.alpha),
        kind: Kind::Solid,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Px = Px::of(800);
    const H: Px = Px::of(500);

    /// At the start the veil is nearly opaque and covers the frame; at the
    /// end it is gone; the size is the frame's throughout.
    #[test]
    fn the_veil_fades_from_the_peak_to_nothing() {
        let start = veil(Transition { progress: 0.0 }, W, H);
        assert_eq!(start.alpha.get(), 0xE0);
        assert_eq!((start.w.value(), start.h.value()), (800, 500));

        let mid = veil(Transition { progress: 0.5 }, W, H);
        assert_eq!(mid.alpha.get(), 0x70);

        let end = veil(Transition { progress: 1.0 }, W, H);
        assert_eq!(end.alpha.get(), 0);
        assert_eq!((end.w.value(), end.h.value()), (800, 500));
    }

    /// Progress is clamped, so a driver that overshoots draws the settled,
    /// invisible veil rather than a negative alpha.
    #[test]
    fn progress_is_clamped() {
        let over = veil(Transition { progress: 1.5 }, W, H);
        let end = veil(Transition { progress: 1.0 }, W, H);
        assert_eq!(over, end);
    }
}
