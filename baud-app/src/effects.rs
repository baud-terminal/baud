//! **The timed effects both drivers draw, as pure functions of a clock**:
//! blinking text's two phases and when either next flips, the cursor a
//! trail follows, and where the window sits for the scrollbar's thumb. The
//! macOS driver reads them off the display link's timestamp; the Linux
//! driver off seconds since it started, and wakes its one wait for the next
//! flip (`timed.rs`). Pure, so the tests run on every platform.

use baud_core::Term;
use baud_domain::{Col, RegisPhase, Row, Rows};
#[cfg(any(target_os = "linux", test))]
use baud_render::Blinking;
use baud_render::{Blink, BlinkPhases, Window};

/// Half a slow blink cycle, milliseconds: text with `SGR 5` shows for this
/// long, then hides for this long -- 75 blinks a minute. `[[source: ecma48]]`
/// § 8.3.117 gives 5 as "slowly blinking (less then 150 per minute)".
const BLINK_HALF_MS: u32 = 400;
/// Half a rapid blink cycle, milliseconds: text with `SGR 6` shows for this
/// long, then hides for this long -- 150 blinks a minute, 2.5 a second, the
/// floor of § 8.3.117's "rapidly blinking (150 per minute or more)", which
/// the owner chose on 2026-10-13. Under WCAG 2.3.1's "more than three times
/// in any one second period" (`[[source: wcag22]]`), whose flash is "a pair
/// of opposing changes": one shown-and-hidden cycle is one flash.
const RAPID_HALF_MS: u32 = 200;
// The two bounds, in integers: 150 a minute is exactly the rapid floor, so a
// float comparison would sit on a rounding edge.
const _: () = assert!(
    60_000 < 150 * 2 * BLINK_HALF_MS,
    "SGR 5 is slow blink: fewer than 150 a minute"
);
const _: () = assert!(
    60_000 >= 150 * 2 * RAPID_HALF_MS,
    "SGR 6 is rapid blink: 150 a minute or more"
);
/// The two halves in seconds, the display link's unit.
pub(crate) const BLINK_HALF: f64 = BLINK_HALF_MS as f64 / 1000.0;
pub(crate) const RAPID_HALF: f64 = RAPID_HALF_MS as f64 / 1000.0;

/// The phases blinking text draws with at display-link time `target`, one a
/// rate: the clock's, unless the system asks for reduced motion, when both
/// are held shown. Apple's HIG names blinking under Reduce Motion, and WCAG
/// 2.2.2 asks for a way to stop blinking that lasts past five seconds; this
/// is that way, and the reasons are on
/// `reduce_motion_holds_blinking_text_shown`. Held, no phase flips, so a
/// blinking cell owes no frame. Each rate shows for half its cycle, as
/// `SGR 5` always did: the text gives a rate and no duty cycle.
pub(crate) fn text_blink(target: f64, motion: crate::config::SystemMotion) -> BlinkPhases {
    use crate::config::SystemMotion;
    let at = |half: f64| match (target / half).rem_euclid(2.0) < 1.0 {
        true => Blink::Shown,
        false => Blink::Hidden,
    };
    match motion {
        SystemMotion::Reduced => BlinkPhases::SHOWN,
        SystemMotion::Full => BlinkPhases {
            slow: at(BLINK_HALF),
            rapid: at(RAPID_HALF),
        },
    }
}

/// How far past a flip's instant the wake for it lands, seconds: the phase
/// is read in floating point, where `k * 0.2 / 0.2` can fall a hair short
/// of `k`, and a wake that read the old phase would find its own deadline
/// already behind it and wait for nothing else. A millisecond is far under
/// a frame.
#[cfg(any(target_os = "linux", test))]
const PAST_FLIP: f64 = 0.001;

/// **When blinking text next changes, at or after `target`**: the earliest
/// flip of a rate `seen` says is on screen, a millisecond past it
/// ([`PAST_FLIP`]), or none -- nothing blinks on screen, or reduced motion
/// holds both phases shown, so no wait is bounded by a blink. A slow-only
/// screen wakes at the slow flips alone, as the macOS tick repaints only
/// for a rate on screen (`Blinking::flips`).
#[cfg(any(target_os = "linux", test))]
pub(crate) fn next_flip(
    target: f64,
    seen: Blinking,
    motion: crate::config::SystemMotion,
) -> Option<f64> {
    if motion == crate::config::SystemMotion::Reduced {
        return None;
    }
    let flip = |half: f64| {
        let mut k = (target / half).floor() + 1.0;
        while k * half + PAST_FLIP <= target {
            k += 1.0;
        }
        k * half + PAST_FLIP
    };
    [(Blinking::SLOW, BLINK_HALF), (Blinking::RAPID, RAPID_HALF)]
        .into_iter()
        .filter(|&(rate, _)| seen.with(rate) == seen)
        .map(|(_, half)| flip(half))
        .reduce(f64::min)
}

/// The phase a ReGIS page's blinking pixels show: text blink's slow one,
/// its faint off phase under dimmed flashing lights faint as the text's.
pub(crate) fn regis_phase(slow: Blink) -> RegisPhase {
    match slow {
        Blink::Shown => RegisPhase::Shown,
        Blink::Hidden => RegisPhase::Hidden,
        Blink::Faint => RegisPhase::Faint,
    }
}

/// How long the alternate-screen fade takes to reveal the new screen,
/// seconds. Only paid on a screen switch when the transition is on.
pub(crate) const TRANSITION_S: f64 = 0.15;

/// Where the window sits in the ring, for the scrollback's thumb: the
/// core's window as drawn (`Term::window_back`), which is the live screen on
/// the alternate screen, so the thumb, the cursor, a selection and the link
/// under a click agree with what is drawn.
pub(crate) fn window_of(term: &Term<'_>) -> Window {
    Window {
        // What the live surface can reach, by the same clamp: nothing on the
        // alternate screen, so no thumb is drawn there even at the bottom
        // of its travel, where a reveal would otherwise show one over the
        // primary's history.
        retained: term.looking_back(Rows::of_count(term.scrollback().len())),
        back: term.window_back(),
        rows: term.viewport().rows(),
    }
}

/// The cursor a trail follows, while the window is on the live screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TrailCursor {
    /// The cursor as it is drawn.
    Shown(Row, Col),
    /// Where a program put the cursor before it hid it
    /// (`Term::placed_cursor`): helix's selection, where helix draws its own
    /// block. Never drawn here -- the program hid it.
    Placed(Row, Col),
}

impl TrailCursor {
    pub(crate) fn cell(self) -> (Row, Col) {
        match self {
            TrailCursor::Shown(r, c) | TrailCursor::Placed(r, c) => (r, c),
        }
    }

    /// The shape the trail arrives at: the drawn cursor's own, and the whole
    /// cell for a placement, because the program draws a block there whatever
    /// shape the terminal's cursor is configured to -- the smear's hole has to
    /// be that block, or a bar-shaped hole leaves the trail over it.
    pub(crate) fn style(self, drawn: baud_core::CursorStyle) -> baud_core::CursorStyle {
        match self {
            TrailCursor::Shown(..) => drawn,
            TrailCursor::Placed(..) => baud_core::CursorStyle::SteadyBlock,
        }
    }
}

/// **The cursor a trail follows**: the one drawn, and while it is hidden,
/// where a program placed it before hiding it. Helix hides the terminal's
/// cursor for good and draws its own at the selection, moving the hidden one
/// there first on every frame, so a trail that followed only the drawn cursor
/// drew nothing on `gg` or `ge`. A hidden cursor no program placed is not
/// followed: the trail keeps where it last was (`trail.rs`).
pub(crate) fn trail_cursor(term: &Term<'_>) -> Option<TrailCursor> {
    if term.window_back().index() != 0 {
        return None;
    }
    match term.cursor_visible() {
        true => {
            let (r, c) = term.cursor();
            Some(TrailCursor::Shown(r, c))
        }
        false => term.placed_cursor().map(|(r, c)| TrailCursor::Placed(r, c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SystemMotion as M;

    /// **The wake for a flip reads the flipped phase**: from any moment,
    /// the deadline is ahead of it, the phase of each rate on screen has
    /// changed by then, and no flip of that rate lies between the two.
    /// Perturbation: wake a flip late, at `(k + 1) * half` -- the phase
    /// at the wake is the old one and this fails. Dropping `PAST_FLIP`
    /// fails `only_a_rate_on_screen_bounds_the_wait` and, through the
    /// clock's round trip to an instant, the wake loop of
    /// `blinking_text_wakes_at_its_own_flips_and_repaints_whole`.
    #[test]
    fn the_next_flip_is_ahead_and_reads_the_new_phase() {
        for (seen, rate) in [
            (
                Blinking::SLOW,
                (|p: &BlinkPhases| p.slow) as fn(&BlinkPhases) -> Blink,
            ),
            (Blinking::RAPID, |p: &BlinkPhases| p.rapid),
        ] {
            for i in 0..20_000u32 {
                let t = f64::from(i) * 0.000_731 + 1000.0;
                let next = next_flip(t, seen, M::Full).expect("a rate on screen");
                assert!(next > t, "{t}: {next}");
                let before = rate(&text_blink(t, M::Full));
                assert_ne!(rate(&text_blink(next, M::Full)), before, "{t} -> {next}");
                // Nothing flips back in between: half a millisecond before
                // the wake, the old phase or the new one, never a third.
                let just = rate(&text_blink(next - 0.0015, M::Full));
                assert_eq!(just, before, "{t} -> {next}: flipped early");
            }
        }
    }

    /// **Nothing on screen blinks, or reduced motion: no wake.** With both
    /// rates on screen the rapid one, which flips at every slow flip and
    /// between them, is the bound. Perturbation: return a flip for an
    /// unseen screen -- the first assertion fails.
    #[test]
    fn only_a_rate_on_screen_bounds_the_wait() {
        assert_eq!(next_flip(3.0, Blinking::UNSEEN, M::Full), None);
        assert_eq!(next_flip(3.0, Blinking::BOTH, M::Reduced), None);
        let both = next_flip(0.05, Blinking::BOTH, M::Full).expect("both");
        let rapid = next_flip(0.05, Blinking::RAPID, M::Full).expect("rapid");
        let slow = next_flip(0.05, Blinking::SLOW, M::Full).expect("slow");
        assert_eq!(both, rapid);
        assert!(slow > rapid, "{slow} {rapid}");
        assert!((rapid - (RAPID_HALF + PAST_FLIP)).abs() < 1e-9, "{rapid}");
        assert!((slow - (BLINK_HALF + PAST_FLIP)).abs() < 1e-9, "{slow}");
    }
}
