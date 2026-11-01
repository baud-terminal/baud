//! **The timed effects both drivers draw, as pure functions of a clock**:
//! blinking text's two phases and when either next flips, the cursor a
//! trail follows, and where the window sits for the scrollbar's thumb. The
//! macOS driver reads them off the display link's timestamp; the Linux
//! driver off seconds since it started, and wakes its one wait for the next
//! flip (`timed.rs`). Pure, so the tests run on every platform.

use baud_core::Term;
use baud_domain::{Chan, Col, RegisPhase, Row, Rows};
#[cfg(any(target_os = "linux", test))]
use baud_render::Blinking;
use baud_render::{Blink, BlinkPhases, GuideLook, Window};

/// Half a slow blink cycle, milliseconds: text with `SGR 5` shows for this
/// long, then hides for this long -- 75 blinks a minute. `[[source: ecma48]]`
/// § 8.3.117 gives 5 as "slowly blinking (less then 150 per minute)".
const BLINK_HALF_MS: u32 = 400;
/// Half a rapid blink cycle, milliseconds: text with `SGR 6` shows for this
/// long, then hides for this long -- 150 blinks a minute, 2.5 a second, the
/// floor of § 8.3.117's "rapidly blinking (150 per minute or more)". Under
/// WCAG 2.3.1's "more than three times in any one second period"
/// (`[[source: wcag22]]`), whose flash is "a pair of opposing changes": one
/// shown-and-hidden cycle is one flash.
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

/// **How the cursor guide is drawn**, `None` while it is hidden: iTerm2's
/// default colour, and its one-point line as `scale` device pixels
/// rounded, from one to four.
pub(crate) fn guide_look(guide: baud_domain::CursorGuide, scale: f64) -> Option<GuideLook> {
    let line = (1..=4u32)
        .rev()
        .find(|&n| f64::from(n) <= scale.round())
        .unwrap_or(1);
    match guide {
        baud_domain::CursorGuide::Shown => Some(GuideLook {
            colour: baud_render::CURSOR_GUIDE,
            line: baud_font::Px::of(line),
        }),
        baud_domain::CursorGuide::Hidden => None,
    }
}

/// **How the badge is drawn**, `None` while there is none to draw: the
/// format a program set (`Term::badge_format`) evaluated -- its text and
/// escapes, each user variable's value (baud keeps none yet, so each is
/// empty, as iTerm2 draws an undefined variable), every other expression
/// nothing -- in iTerm2's default colour or the one `SetColors=badge=`
/// set, opaque as iTerm2 makes it, in iTerm2's box and margins at
/// `scale` device pixels a point, in bold. Nothing while `[screen] badge`
/// is off, and nothing for a text of white space alone.
pub(crate) fn badge_look(
    term: &Term<'_>,
    shown: crate::config::BadgeShown,
    scale: f64,
) -> Option<baud_render::BadgeLook> {
    if shown == crate::config::BadgeShown::Off {
        return None;
    }
    let format = term.badge_format()?;
    let mut text = String::new();
    baud_domain::interpolate(
        format,
        |e, _w| {
            // A user variable's value would be written here; none is kept.
            let _ = baud_domain::user_variable(e);
        },
        |c| text.push(c),
    );
    if text.chars().all(char::is_whitespace) {
        return None;
    }
    let colour = match term.badge_colour().and_then(|c| c.as_rgb()) {
        Some((r, g, b)) => {
            baud_render::Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        }
        None => baud_render::BADGE,
    };
    let s = scale as f32;
    let px =
        |points: f32| baud_font::Px::of(u32::try_from((points * s).round() as i64).unwrap_or(0));
    Some(baud_render::BadgeLook {
        text,
        colour,
        fraction: baud_render::BOX,
        top: px(baud_render::MARGINS.0),
        right: px(baud_render::MARGINS.1),
        scale: s,
        style: baud_render::FaceStyle::Bold,
    })
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

/// How long the alternate-screen veil takes to reveal the new screen,
/// seconds: the switch's form under Reduce Motion. Only paid on a screen
/// switch when the transition is on.
pub(crate) const TRANSITION_S: f64 = 0.15;

/// **How a screen switch is drawn**: the new screen sliding in over the
/// old one toward the screen it brought up, or, under Reduce Motion, the
/// veil -- a dissolve in place of the motion.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum SwitchForm {
    Slide(baud_render::Toward),
    Veil,
}

/// The form a switch to `screen` takes under `motion`.
pub(crate) fn switch_form(
    screen: baud_core::Screen,
    motion: crate::config::SystemMotion,
) -> SwitchForm {
    match motion {
        crate::config::SystemMotion::Reduced => SwitchForm::Veil,
        crate::config::SystemMotion::Full => SwitchForm::Slide(match screen {
            baud_core::Screen::Alternate => baud_render::Toward::Alternate,
            baud_core::Screen::Normal => baud_render::Toward::Primary,
        }),
    }
}

/// **A switch's frame at `secs`** on the display link's clock, or `None`
/// once it has settled. The slide: for [`baud_render::SLIDE_S`] the new
/// screen slides in under the faint veil, and then the veil fades off over
/// [`TRANSITION_S`]. The veil alone, under Reduce Motion: the fade over
/// [`TRANSITION_S`].
pub(crate) fn switch_at(start: f64, form: SwitchForm, secs: f64) -> Option<SwitchFrame> {
    let t = (secs - start).max(0.0);
    let slide_s = f64::from(baud_render::SLIDE_S);
    match form {
        SwitchForm::Slide(toward) if t < slide_s => Some(SwitchFrame {
            slide: Some(baud_render::ScreenSlide {
                progress: (t / slide_s) as f32,
                toward,
            }),
            veil: Some(baud_render::Transition::faint(0.0)),
        }),
        SwitchForm::Slide(_) if t < slide_s + TRANSITION_S => Some(SwitchFrame {
            slide: None,
            veil: Some(baud_render::Transition::faint(
                ((t - slide_s) / TRANSITION_S) as f32,
            )),
        }),
        SwitchForm::Veil if t < TRANSITION_S => Some(SwitchFrame {
            slide: None,
            veil: Some(baud_render::Transition::fade((t / TRANSITION_S) as f32)),
        }),
        SwitchForm::Slide(_) | SwitchForm::Veil => None,
    }
}

/// Whether a CPU frame was composed from a slide's two frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Composed {
    No,
    Yes,
}

/// A switch's frame: where the slide is, if it is still sliding, and the
/// veil over the new screen.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct SwitchFrame {
    pub(crate) slide: Option<baud_render::ScreenSlide>,
    pub(crate) veil: Option<baud_render::Transition>,
}

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
/// followed: the trail keeps where it last was, and one drawing fades out
/// there (`trail.rs`).
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

    /// **The guide's look**: iTerm2's colour while shown, none while
    /// hidden, and its one-point line one device pixel at a backing scale
    /// of 1 and two at 2, from one to four.
    #[test]
    fn the_guide_line_is_a_point_in_device_pixels() {
        use baud_domain::CursorGuide;
        assert_eq!(guide_look(CursorGuide::Hidden, 2.0), None);
        for (scale, line) in [(1.0, 1), (2.0, 2), (3.0, 3), (0.5, 1), (9.0, 4)] {
            assert_eq!(
                guide_look(CursorGuide::Shown, scale),
                Some(GuideLook {
                    colour: baud_render::CURSOR_GUIDE,
                    line: baud_font::Px::of(line),
                }),
                "scale {scale}"
            );
        }
    }

    /// **The badge is the program's format evaluated**: its text and
    /// escapes, an undefined user variable empty, in iTerm2's colour, box
    /// and margins at the backing scale, in bold; the colour `SetColors`
    /// set opaque; nothing while the setting is off, before a program sets
    /// one, or for white space.
    #[test]
    fn the_badge_is_the_programs_format_evaluated() {
        use crate::config::BadgeShown;
        let ws = baud_domain::WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(owned.storage(), &ws);
        let feed = |t: &mut baud_core::Term<'_>, b: &[u8]| {
            t.write_at(baud_domain::BytePos::ZERO, b);
            t.feed(
                baud_domain::BytePos::ZERO,
                baud_domain::ReadLen::from_index(b.len()),
            );
        };
        assert_eq!(badge_look(&t, BadgeShown::On, 2.0), None, "none set");
        // `on \(user.host)\tx`, base64.
        feed(
            &mut t,
            b"\x1b]1337;SetBadgeFormat=b24gXCh1c2VyLmhvc3QpXHR4\x07",
        );
        let look = badge_look(&t, BadgeShown::On, 2.0).expect("a badge");
        assert_eq!(look.text, "on \tx");
        assert_eq!(look.colour, baud_render::BADGE);
        assert_eq!(
            (look.top, look.right),
            (baud_font::Px::of(60), baud_font::Px::of(20))
        );
        assert_eq!(look.fraction, baud_render::BOX);
        assert_eq!(look.style, baud_render::FaceStyle::Bold);
        assert_eq!(
            badge_look(&t, BadgeShown::Off, 2.0),
            None,
            "the setting off"
        );
        feed(&mut t, b"\x1b]1337;SetColors=badge=00ff00\x07");
        assert_eq!(
            badge_look(&t, BadgeShown::On, 1.0).map(|l| l.colour),
            Some(baud_render::Argb::rgb(
                Chan::from_u8(0),
                Chan::from_u8(255),
                Chan::from_u8(0)
            ))
        );
        // `   `, base64.
        feed(&mut t, b"\x1b]1337;SetBadgeFormat=ICAg\x07");
        assert_eq!(
            badge_look(&t, BadgeShown::On, 1.0),
            None,
            "white space alone"
        );
        let src = [include_str!("app.rs"), include_str!("linux.rs")];
        for s in src {
            assert!(s.contains(concat!("set_badge(crate::", "effects::badge_look(")));
            assert!(s.contains(concat!("&self.term, self.", "badge, ")));
        }
    }

    /// **Both apps draw a program's guide over the setting**: each frame
    /// hands the renderer what `Term::cursor_guide` says, else `[cursor]
    /// guide`, through `guide_look`.
    #[test]
    fn both_apps_draw_a_programs_guide_over_the_setting() {
        let read = concat!("self.term.cursor_guide().", "unwrap_or(self.cursor_guide)");
        for (name, src) in [
            ("app.rs", include_str!("app.rs")),
            ("linux.rs", include_str!("linux.rs")),
        ] {
            assert_eq!(src.matches(read).count(), 1, "{name}: {read}");
            assert!(
                src.contains(concat!(
                    "set_cursor_guide(crate::effects::",
                    "guide_look(guide"
                )),
                "{name}"
            );
        }
    }

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
