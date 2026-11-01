//! **The visual bell a `BEL` rings**: what a frame shows of it, in the
//! colour a program set with kitty's `OSC 21 ; visual_bell=` or, with none
//! set, as the other video; and how often it may flash.
//!
//! **A colour is drawn as kitty draws its visual bell** (`[[source:
//! kitty-6d5d0c4-shaders]]` 1111-1169): one solid over the whole frame, in
//! the bell's colour at an alpha of the flash's intensity times 0.4, or
//! times 0.6 when the colour's largest channel is above 0.45 in linear light
//! ("light color"). The intensity rises from 0 to 1 and falls back to 0,
//! each half under CSS's `ease-in-out`, cubic-bezier(0.42, 0, 0.58, 1)
//! (`[[source: kitty-6d5d0c4-animation-h]]` 20, the curve kitty's
//! `get_visual_bell_intensity()` builds), over the bell's length: "The flash
//! is animated, fading in and out over the specified duration"
//! (`[[source: kitty-6d5d0c4-options-definition]]` 1450-1464). kitty's code
//! runs the whole fade inside half of its configured duration (`duration =
//! OPT(visual_bell_duration) / 2`, and the bell over once the progress
//! passes it), a departure from its own text not followed. The length is
//! the one baud's other-video flash is shown for, the entry's `flash`
//! padding (`baud-app`'s `bell::LENGTH`), since kitty's own default length
//! is zero, its bell off.
//!
//! **The solid is mixed as the frame mixes every solid**, in the frame's
//! [`crate::Blend`]: in linear light under [`crate::Blend::Linear`], as
//! kitty's layered path blends its tint, and in encoded numbers under the
//! default, a departure in how the colour mixes, not in its alpha.
//!
//! **A bell rung faster than WCAG 2.3.1 allows flashes no faster**: "no
//! more than three general flashes and / or no more than three red flashes
//! within any one-second period" (`[[source: wcag22]]`), and a bell is one
//! flash, a pair of opposing changes, wherever its colour and the page
//! differ enough. kitty restarts the fade at each `BEL` (`screen_bell()`,
//! `[[source: kitty-6d5d0c4-screen]]` 3146-3158), so a bell rung every
//! frame dips to nothing and rises again each frame: a flash a frame
//! interval. Here **a ring while the flash rises or holds its peak holds it
//! there** from the ring for half the length, so bells closer than that are
//! one flash however long they last; **a ring after the peak and through
//! the [`quiet`] after the flash ends is absorbed**, as xterm suppresses a
//! bell inside `bellSuppressTime` (`[[source: xterm-man]]`). A flash and
//! the quiet after it last at least [`SPAN`], half a second, except where a
//! change of `?5` ends an other-video flash early: then the quiet runs from
//! that change ([`BellFlash::video_ended`]), and the change itself shows
//! the flash's video, so no extra transition is drawn. Either way no second
//! holds more than four of the bell's changes and no reading of the rule
//! counts more than three flashes in one -- the census's verdict and its
//! strict reading both (`baud-bench/tests/bell_flashes.rs`). The same rule
//! holds the other-video flash a `BEL` shows with no colour set.

use core::time::Duration;

use crate::color::Argb;
use crate::renderer::VideoFlash;
use baud_core::{PaintRole, ProgramPaint, Term};
use baud_domain::Chan;

/// A flash and the quiet after it, at least: two in a second at most.
pub const SPAN: Duration = Duration::from_millis(500);

/// How a bell that rang shows: the colour a program set, or the other
/// video, fixed when its flash starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BellKind {
    /// The other video, kitty's "Automatic color selection based on current
    /// screen colors" (`[[source: kitty-6d5d0c4-color-stack]]`): no colour
    /// set, or the automatic one.
    Video,
    /// A colour of the program's, opaque.
    Tint(Argb),
}

impl BellKind {
    /// How a bell rung now shows: in the colour a program set with
    /// `OSC 21 ; visual_bell=` ([`Term::program_paint`]), else -- none
    /// set, or the automatic one -- as the other video.
    pub fn of(term: &Term<'_>) -> BellKind {
        match term.program_paint(PaintRole::VisualBell) {
            ProgramPaint::Colour((r, g, b)) => BellKind::Tint(Argb::rgb(
                Chan::from_u8(r),
                Chan::from_u8(g),
                Chan::from_u8(b),
            )),
            ProgramPaint::Automatic | ProgramPaint::Unset => BellKind::Video,
        }
    }
}

/// What one frame draws of the bell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BellShown {
    Dark,
    /// The other video, [`crate::VideoFlash::On`].
    Video,
    /// The solid over the frame ([`crate::Renderer::set_bell_tint`]).
    Tint(BellTint),
}

impl BellShown {
    /// Its half of the frame's [`VideoFlash`].
    pub fn video(self) -> VideoFlash {
        match self {
            BellShown::Video => VideoFlash::On,
            BellShown::Dark | BellShown::Tint(_) => VideoFlash::Off,
        }
    }

    /// The solid it draws, if any ([`crate::Renderer::set_bell_tint`]).
    pub fn tint(self) -> Option<BellTint> {
        match self {
            BellShown::Tint(t) => Some(t),
            BellShown::Dark | BellShown::Video => None,
        }
    }
}

/// The bell's solid over the whole frame: its colour, and the alpha the
/// fade is at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BellTint {
    pub colour: Argb,
    pub alpha: Chan,
}

/// A flash on screen: how it shows, when it started, and the last ring it
/// holds its peak for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Lit {
    kind: BellKind,
    start: Duration,
    held: Duration,
}

/// Whether a frame comes with a ring the last drawn frame had not shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rang {
    No,
    Yes,
}

/// **The bell's flash on a clock**, as a value: [`BellFlash::at`] says what
/// a frame drawn at a moment shows and returns the state that frame
/// leaves, which a driver keeps once the frame is drawn. Moments are the
/// time since any fixed origin the caller keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BellFlash {
    lit: Option<Lit>,
    /// No flash starts before this: the [`quiet`] after the last one ended.
    quiet: Duration,
    length: Duration,
}

/// The quiet after a flash of `length`, so that the two last [`SPAN`].
pub fn quiet(length: Duration) -> Duration {
    SPAN.saturating_sub(length)
}

impl BellFlash {
    /// No flash, none ever shown, each to last `length`.
    pub fn new(length: Duration) -> BellFlash {
        BellFlash {
            lit: None,
            quiet: Duration::ZERO,
            length,
        }
    }

    /// The frame drawn at `now`: what it shows, and the state it leaves.
    /// `kind` is how a flash this frame starts shows. A frame earlier than
    /// one already drawn is read as at that frame.
    pub fn at(self, now: Duration, rang: Rang, kind: BellKind) -> (BellFlash, BellShown) {
        let half = self.length / 2;
        let mut next = self;
        if let Some(mut lit) = self.lit {
            let fall = match lit.kind {
                BellKind::Video => lit.held + self.length,
                BellKind::Tint(_) => lit.held + half,
            };
            if rang == Rang::Yes && now < fall {
                lit.held = now.max(lit.held);
            }
            let end = lit.held + self.length;
            if now < end {
                next.lit = Some(lit);
                return (next, self.show(lit, now));
            }
            next.lit = None;
            next.quiet = end + quiet(self.length);
        }
        if rang == Rang::Yes && now >= next.quiet {
            let lit = Lit {
                kind,
                start: now,
                held: now,
            };
            next.lit = Some(lit);
            return (next, self.show(lit, now));
        }
        (next, BellShown::Dark)
    }

    /// Whether a flash is on screen, and how.
    pub fn lit(&self) -> Option<BellKind> {
        self.lit.map(|l| l.kind)
    }

    /// When a flash on screen ends, if nothing holds it longer.
    pub fn end(&self) -> Option<Duration> {
        self.lit.map(|l| l.held + self.length)
    }

    /// The flash a frame's mode change ends: one of the other video, since
    /// the mode is what a frame draws, starting the quiet from `now`; a
    /// tint is not of the video and runs on.
    pub fn video_ended(self, now: Duration) -> BellFlash {
        match self.lit {
            Some(Lit {
                kind: BellKind::Video,
                ..
            }) => BellFlash {
                lit: None,
                quiet: now + quiet(self.length),
                ..self
            },
            _ => self,
        }
    }

    fn show(&self, lit: Lit, now: Duration) -> BellShown {
        match lit.kind {
            BellKind::Video => BellShown::Video,
            BellKind::Tint(colour) => BellShown::Tint(BellTint {
                colour,
                alpha: tint_alpha(colour, self.intensity(lit, now)),
            }),
        }
    }

    /// The fade at `now`: up over the first half from the start, held from
    /// there until half the length after the last ring, down over the
    /// second half.
    fn intensity(&self, lit: Lit, now: Duration) -> f64 {
        let half = (self.length / 2).as_secs_f64();
        if half <= 0.0 {
            return 0.0;
        }
        let up = (now.saturating_sub(lit.start)).as_secs_f64() / half;
        if up < 1.0 {
            return ease_in_out(up);
        }
        let down = (now.saturating_sub(lit.held + self.length / 2)).as_secs_f64() / half;
        if down <= 0.0 {
            return 1.0;
        }
        1.0 - ease_in_out(down.min(1.0))
    }
}

/// **The solid's alpha at `intensity`**: kitty's `draw_visual_bell_flash()`,
/// the intensity times its attenuation, 0.4, or 0.6 for a colour whose
/// largest channel in linear light is above 0.45, rounded to a byte.
pub fn tint_alpha(colour: Argb, intensity: f64) -> Chan {
    let lin = |c: Chan| {
        let c = f64::from(c.get()) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            crate::det::Det::det_powf((c + 0.055) / 1.055, 2.4)
        }
    };
    let most = lin(colour.r()).max(lin(colour.g())).max(lin(colour.b()));
    let attenuation = if most > 0.45 { 0.6 } else { 0.4 };
    let a = (intensity.clamp(0.0, 1.0) * attenuation * 255.0).round();
    Chan::from_u8(u8::try_from(a as i64).unwrap_or(u8::MAX))
}

/// CSS's `ease-in-out`, cubic-bezier(0.42, 0, 0.58, 1), at `x` in 0 to 1:
/// the curve's y where its x is `x`, the parameter found by bisection,
/// which the curve's x rising monotonically from 0 to 1 makes exact to the
/// 52 steps taken.
pub fn ease_in_out(x: f64) -> f64 {
    const P1: (f64, f64) = (0.42, 0.0);
    const P2: (f64, f64) = (0.58, 1.0);
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bez = |t: f64, a: f64, b: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..52 {
        let mid = (lo + hi) * 0.5;
        if bez(mid, P1.0, P2.0) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) * 0.5, P1.1, P2.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);
    const RED: Argb = Argb(0xFFFF_0000);

    fn tint(s: BellShown) -> Option<Chan> {
        match s {
            BellShown::Tint(t) => Some(t.alpha),
            _ => None,
        }
    }

    /// The curve's ends and middle, and its symmetry about the middle.
    #[test]
    fn ease_in_out_is_css_curve() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-9);
        for k in 1..10 {
            let x = f64::from(k) / 10.0;
            assert!((ease_in_out(x) + ease_in_out(1.0 - x) - 1.0).abs() < 1e-9);
        }
        assert!(ease_in_out(0.1) < 0.1, "slow at the start");
    }

    /// kitty's attenuation: 0.4 for a dark colour, 0.6 for a light one,
    /// the line at 0.45 in linear light.
    #[test]
    fn a_light_colour_is_attenuated_less() {
        let grey = |v: u8| Argb::rgb(Chan::from_u8(v), Chan::from_u8(v), Chan::from_u8(v));
        assert_eq!(tint_alpha(grey(0x40), 1.0).get(), 102);
        assert_eq!(tint_alpha(RED, 1.0).get(), 153);
        // 0.45 in linear light is about 178 encoded.
        assert_eq!(tint_alpha(grey(178), 1.0).get(), 102);
        assert_eq!(tint_alpha(grey(179), 1.0).get(), 153);
    }

    /// One ring: up over the first half, down over the second, gone at the
    /// length.
    #[test]
    fn one_ring_fades_in_and_out_over_the_length() {
        let b = BellFlash::new(MS * 100);
        let (b, s) = b.at(MS * 1000, Rang::Yes, BellKind::Tint(RED));
        assert_eq!(
            tint(s).map(Chan::get),
            Some(0),
            "the first frame is the fade's start"
        );
        let (b2, s) = b.at(MS * 1050, Rang::No, BellKind::Video);
        assert_eq!(tint(s).map(Chan::get), Some(153), "the peak");
        let (_, s) = b.at(MS * 1025, Rang::No, BellKind::Video);
        assert!(tint(s).map(Chan::get).unwrap() > 50 && tint(s).map(Chan::get).unwrap() < 103);
        let (b3, s) = b2.at(MS * 1100, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "over at the length");
        assert_eq!(b3.lit(), None);
    }

    /// Rings closer than half the length hold the peak; the fall starts
    /// half the length after the last.
    #[test]
    fn rings_while_it_rises_or_holds_hold_the_peak() {
        let mut b = BellFlash::new(MS * 100);
        let mut shown = vec![];
        for f in 0..30u32 {
            let rang = if f < 12 { Rang::Yes } else { Rang::No };
            let (n, s) = b.at(MS * (1000 + f * 16), rang, BellKind::Tint(RED));
            b = n;
            shown.push(tint(s).map(Chan::get));
        }
        // Rising, then held at the peak while the rings come and for half
        // the length after the last, at 1176 ms.
        assert_eq!(shown[0], Some(0));
        assert!(shown[4..15].iter().all(|&a| a == Some(153)), "{shown:?}");
        assert!(shown[15].unwrap() < 153 && shown[15].unwrap() > 0);
        assert_eq!(shown[18], None, "over 100 ms after the last ring");
        // Monotone: up, then down, never up again.
        let a: Vec<u8> = shown.iter().map(|s| s.unwrap_or(0)).collect();
        let peak = a.iter().position(|&v| v == 153).unwrap();
        assert!(a[..=peak].windows(2).all(|w| w[0] <= w[1]));
        assert!(a[peak..].windows(2).all(|w| w[0] >= w[1]));
    }

    /// A ring after the peak, and through the quiet after the end, is
    /// absorbed; the first after the quiet starts a flash.
    #[test]
    fn a_ring_in_the_fall_or_the_quiet_is_absorbed() {
        let b = BellFlash::new(MS * 100);
        let (b, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Tint(RED));
        let (b, s) = b.at(MS * 60, Rang::Yes, BellKind::Tint(RED));
        assert!(tint(s).map(Chan::get).unwrap() < 153, "falling, not held");
        let (b, s) = b.at(MS * 100, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (b, s) = b.at(MS * 499, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "inside the quiet");
        let (_, s) = b.at(MS * 500, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Video, "half a second after the start");
    }

    /// The other video holds while rings come closer than the length, as
    /// a second `?5` pair restarts its flash.
    #[test]
    fn the_other_video_holds_for_the_length_after_each_ring() {
        let b = BellFlash::new(MS * 100);
        let (b, s) = b.at(Duration::ZERO, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Video);
        let (b, _) = b.at(MS * 90, Rang::Yes, BellKind::Video);
        let (b, s) = b.at(MS * 180, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Video, "held from the second ring");
        assert_eq!(b.end(), Some(MS * 190));
        let (b, s) = b.at(MS * 190, Rang::No, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (_, s) = b.at(MS * 589, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark, "the quiet runs from the end");
    }

    /// A mode change ends a flash of the other video and starts the quiet;
    /// a tint runs on.
    #[test]
    fn a_mode_change_ends_the_other_video_and_not_a_tint() {
        let b = BellFlash::new(MS * 100);
        let (v, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Video);
        let v = v.video_ended(MS * 10);
        assert_eq!(v.lit(), None);
        let (_, s) = v.at(MS * 409, Rang::Yes, BellKind::Video);
        assert_eq!(s, BellShown::Dark);
        let (t, _) = b.at(Duration::ZERO, Rang::Yes, BellKind::Tint(RED));
        assert_eq!(t.video_ended(MS * 10), t);
    }

    #[test]
    fn the_quiet_and_the_length_make_the_span() {
        assert_eq!(quiet(MS * 100), MS * 400);
        assert_eq!(quiet(MS * 100) + MS * 100, SPAN);
    }
}
