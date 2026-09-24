//! When the scrollbar's thumb is seen, and how wide: AppKit's overlay
//! scroller, measured by `scripts/scroller-probe.swift` and specified in
//! `docs/research/notes/l-scrollbar.md`. Pure -- the display link's time in,
//! a look out -- so the driver's four events and its tick are all this
//! module has to be told, and a test can walk a timeline without a window.
//!
//! A reveal holds the thumb at full, then fades it along Core Animation's
//! ease-in-ease-out curve to nothing; the pointer over the strip while scrolled back holds it, widened,
//! for as long as it stays; nothing else shows it. Output arriving under a
//! scrolled-back window reveals nothing, which is what AppKit measured for a
//! document that grows under a view that stays on its lines.

use baud_domain::{Chan, HostTime, Rows};
use baud_font::Px;
use baud_render::{Hover, Look, Place, Scroller, Widths};
use core::time::Duration;

/// How long a reveal holds the thumb at full: AppKit's after a wheel click,
/// where the fade's fit starts -- 23 ms before its last frame at 97% of the
/// peak, 524 ms after the click -- and 247 ms before it is gone at 748.
pub const HOLD: Duration = Duration::from_millis(500);

/// How long the fade takes: the ease-in-ease-out fit to a flash's 26 frames,
/// within 0.020 of each.
pub const FADE: Duration = Duration::from_millis(247);

/// Core Animation's `easeInEaseOut`, the cubic Bezier (0.42, 0) (0.58, 1):
/// how far along a fade is at `t` of its duration. The best fit of the two
/// curves the fade was compared against, which is what one would expect of an
/// AppKit animation. Solved for `t` by bisection on the curve's x; the curve
/// is monotonic, and 24 halvings are finer than a channel.
fn ease_in_out(t: f32) -> f32 {
    let bez = |u: f32, a: f32, b: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(mid, 0.42, 0.58) < t {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez((lo + hi) / 2.0, 0.0, 1.0)
}

/// AppKit's widths in points, measured: the knob at rest, the knob and its
/// track under the pointer (the legacy scroller's, which is what an overlay
/// one widens into), the inset from the edge, and the overlay scroller's
/// frame, which is the strip the pointer hovers.
const REST_PT: f64 = 6.0;
const HOVERED_PT: f64 = 11.0;
const INSET_PT: f64 = 3.0;
const STRIP_PT: f64 = 17.0;

/// The widths in pixels at `scale` pixels to the point.
pub fn widths(scale: Scale) -> Widths {
    let px = |pt: f64| Px::of(u32::try_from((pt * scale.0).round() as i64).unwrap_or(0));
    Widths {
        rest: px(REST_PT),
        hovered: px(HOVERED_PT),
        inset: px(INSET_PT),
        strip: px(STRIP_PT),
    }
}

/// The backing scale, pixels to the point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Scale(pub f64);

/// Where the pointer is, for the hover.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Over {
    /// Over the strip, with the window scrolled back: the thumb widens and
    /// holds. At the live screen the strip is cells -- the owner's call, so
    /// a terminal's rightmost columns stay its own.
    Strip,
    /// Anywhere else, or the strip at the live screen.
    Cells,
}

/// Where a pointer event is, for the hover: over the strip only while the
/// window is scrolled back and the pointer is not leaving the view. At the
/// live screen the strip is cells -- the owner's call, 2026-09-23, so a
/// terminal's rightmost columns keep their clicks, their selection and a
/// tracking program's reports.
pub fn over(place: Place, back: Rows, motion: Motion) -> Over {
    match (place, back.index() > 0, motion) {
        (Place::Strip, true, Motion::Within) => Over::Strip,
        _ => Over::Cells,
    }
}

/// Whether a pointer event leaves the view.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Motion {
    Within,
    Leaving,
}

/// Whether a thumb drag is in progress: a drag holds the hover itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Drag {
    Idle,
    Dragging,
}

/// Whether a press is on the knob.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Knob {
    On,
    Off,
}

/// Which button a press is: only the left drags or jumps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Other,
}

/// What a press does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Takes {
    /// On the knob: a drag starts.
    Drag,
    /// In the strip, off the knob: the thumb jumps to the pointer.
    Jump,
    /// In a legacy scroller's gutter with nothing for it to do -- no
    /// history, or a button that does not scroll: the gutter is never
    /// cells, so the press is taken and nothing happens.
    Swallow,
    /// Not the scrollbar's: the selection's, or the child's.
    Cells,
}

/// Whether a press's drags and its release belong to the scrollbar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grab {
    /// The scrollbar keeps the whole gesture, so no one else sees a
    /// release without its press.
    Bar,
    /// The cells', the selection's or the child's.
    Free,
}

impl Takes {
    /// Who holds the pointer until this press's release.
    pub fn grabs(self) -> Grab {
        match self {
            Takes::Drag | Takes::Jump | Takes::Swallow => Grab::Bar,
            Takes::Cells => Grab::Free,
        }
    }
}

/// A press in the strip, which is the knob's column under the overlay style
/// and the gutter under the legacy one. Under the overlay style it is the
/// scrollbar's while any of the thumb is seen -- a left press on the knob a
/// drag and off it a jump -- and with nothing drawn it is the cells', since
/// the strip lies over them. Under the legacy style the gutter is never
/// cells: a left press drags or jumps when there is a knob, and anything
/// else there is taken and does nothing. Outside the strip it is always the
/// cells'. `knob` is none when there is no history to scroll.
pub fn press(
    scroller: Scroller,
    place: Place,
    shown: Chan,
    knob: Option<Knob>,
    button: Button,
) -> Takes {
    match (place, scroller) {
        (Place::Grid, _) => Takes::Cells,
        (Place::Strip, Scroller::Overlay) => match (button, shown.get() > 0, knob) {
            (Button::Left, true, Some(Knob::On)) => Takes::Drag,
            (Button::Left, true, Some(Knob::Off)) => Takes::Jump,
            _ => Takes::Cells,
        },
        (Place::Strip, Scroller::Legacy) => match (button, knob) {
            (Button::Left, Some(Knob::On)) => Takes::Drag,
            (Button::Left, Some(Knob::Off)) => Takes::Jump,
            _ => Takes::Swallow,
        },
    }
}

/// A pointer motion that is not part of a press: under the legacy style
/// the gutter is not cells, so motion over it is taken and reported to no
/// one -- a program tracking every motion (`?1003`) would otherwise be told
/// the pointer is on the last column. The overlay strip lies over cells and
/// passes motion on.
pub fn motion(scroller: Scroller, place: Place) -> Takes {
    match (scroller, place) {
        (Scroller::Legacy, Place::Strip) => Takes::Swallow,
        _ => Takes::Cells,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Hidden,
    Held { until: HostTime },
    Fading { from: HostTime },
    Hovered,
}

/// The thumb's reveal: hidden, held, fading or hovered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Reveal {
    state: State,
}

impl Reveal {
    pub const HIDDEN: Reveal = Reveal {
        state: State::Hidden,
    };

    /// A scroll that moved the window, a jump, a snap to the live screen:
    /// full now, held, then faded. A hovered thumb stays hovered.
    pub fn reveal(&mut self, now: HostTime) {
        if self.state != State::Hovered {
            self.state = State::Held {
                until: now.plus(HOLD),
            };
        }
    }

    /// The pointer moved: over the strip it holds, widened, until it leaves,
    /// and leaving is a reveal. A drag in progress holds the same way.
    pub fn hover(&mut self, over: Over, now: HostTime) {
        match (over, self.state) {
            (Over::Strip, _) => self.state = State::Hovered,
            (Over::Cells, State::Hovered) => {
                self.state = State::Held {
                    until: now.plus(HOLD),
                }
            }
            (Over::Cells, _) => {}
        }
    }

    /// The display link ticked at `now`: a hold that ran out fades, from
    /// the moment it ran out, and a fade that finished is hidden.
    pub fn step(&mut self, now: HostTime) {
        if let State::Held { until } = self.state {
            if now.since(until).is_some() {
                self.state = State::Fading { from: until };
            }
        }
        if let State::Fading { from } = self.state {
            if now.since(from).is_some_and(|d| d >= FADE) {
                self.state = State::Hidden;
            }
        }
    }

    /// The display link ticked at `now`: step the hold and the fade, then
    /// re-evaluate the hover from where the pointer last was and where the
    /// window is now. The window moves without the pointer -- a wheel, a
    /// key, a paste or a jump bringing it to the live screen, or a wheel
    /// taking it off -- and whether the strip hovers depends on it. A drag
    /// holds its own.
    pub fn tick(&mut self, now: HostTime, pointer: Place, back: Rows, drag: Drag) {
        self.step(now);
        if drag == Drag::Idle {
            self.hover(over(pointer, back, Motion::Within), now);
        }
    }

    /// How the thumb is seen at `now` under `scroller`: this reveal's under
    /// the overlay style, and constant under the legacy one, where nothing
    /// fades or hovers -- so the tick, which forces a frame only when the
    /// look changes, forces none for it, and a wheel's reveal draws nothing
    /// new.
    pub fn look_under(&self, scroller: Scroller, now: HostTime) -> Look {
        match scroller {
            Scroller::Overlay => self.look(now),
            Scroller::Legacy => Look {
                shown: Chan::from_u8(255),
                hover: Hover::Hovered,
            },
        }
    }

    /// How the thumb is seen at `now`.
    pub fn look(&self, now: HostTime) -> Look {
        match self.state {
            State::Hidden => Look::HIDDEN,
            State::Held { .. } => Look {
                shown: Chan::from_u8(255),
                hover: Hover::Rest,
            },
            State::Hovered => Look {
                shown: Chan::from_u8(255),
                hover: Hover::Hovered,
            },
            State::Fading { from } => {
                let t = now
                    .since(from)
                    .map_or(0.0, |d| d.as_secs_f32() / FADE.as_secs_f32())
                    .clamp(0.0, 1.0);
                Look {
                    shown: Chan::from_unit(1.0 - ease_in_out(t)),
                    hover: Hover::Rest,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    fn at(d: Duration) -> HostTime {
        HostTime::of_secs(1000.0).unwrap().plus(d)
    }

    fn shown(r: &mut Reveal, d: Duration) -> Chan {
        r.step(at(d));
        r.look(at(d)).shown
    }

    #[test]
    fn a_reveal_holds_then_fades_along_the_measured_curve_to_nothing() {
        let mut r = Reveal::HIDDEN;
        assert_eq!(
            shown(&mut r, Duration::ZERO).get(),
            0,
            "hidden until revealed"
        );
        r.reveal(at(Duration::ZERO));
        assert_eq!(shown(&mut r, MS * 10).get(), 255);
        assert_eq!(
            shown(&mut r, MS * 499).get(),
            255,
            "held for the whole hold"
        );
        // A quarter, a half and three quarters of the way through the fade:
        // the ease-in-ease-out curve's 0.87, 0.50 and 0.13.
        assert_eq!(shown(&mut r, MS * (500 + 62)).get(), 222);
        assert_eq!(shown(&mut r, MS * (500 + 124)).get(), 127);
        assert_eq!(shown(&mut r, MS * (500 + 185)).get(), 33);
        assert_eq!(
            shown(&mut r, MS * (500 + 247)).get(),
            0,
            "gone when the fade ends"
        );
        assert_eq!(r, Reveal::HIDDEN);
    }

    #[test]
    fn a_reveal_during_the_fade_brings_it_back_to_full() {
        let mut r = Reveal::HIDDEN;
        r.reveal(at(Duration::ZERO));
        assert!(shown(&mut r, MS * 600).get() < 255, "fading");
        r.reveal(at(MS * 600));
        assert_eq!(shown(&mut r, MS * 610).get(), 255);
        assert_eq!(
            shown(&mut r, MS * 1099).get(),
            255,
            "a fresh hold from the new reveal"
        );
    }

    #[test]
    fn the_pointer_over_the_strip_widens_and_holds_until_it_leaves() {
        let mut r = Reveal::HIDDEN;
        r.hover(Over::Strip, at(Duration::ZERO));
        r.step(at(MS * 5_000));
        let l = r.look(at(MS * 5_000));
        assert_eq!((l.shown.get(), l.hover), (255, Hover::Hovered));
        // A scroll while hovered stays hovered.
        r.reveal(at(MS * 5_000));
        assert_eq!(r.look(at(MS * 5_000)).hover, Hover::Hovered);
        // Leaving is a reveal: narrow again, held, then faded.
        r.hover(Over::Cells, at(MS * 5_000));
        let l = r.look(at(MS * 5_010));
        assert_eq!((l.shown.get(), l.hover), (255, Hover::Rest));
        assert_eq!(shown(&mut r, MS * (5_000 + 500 + 247)).get(), 0);
        // The pointer over the cells never reveals anything.
        r.hover(Over::Cells, at(MS * 6_000));
        assert_eq!(shown(&mut r, MS * 6_010).get(), 0);
    }

    /// **Idle builds nothing.** The driver forces a frame only when the look
    /// changes from the last one drawn; over a reveal and a long idle after
    /// it, that is the fade's frames and nothing in the hold or once hidden.
    #[test]
    fn the_look_changes_only_while_it_fades() {
        let mut r = Reveal::HIDDEN;
        let mut last = r.look(at(Duration::ZERO));
        r.reveal(at(Duration::ZERO));
        let mut changed = Vec::new();
        // A 120 Hz tick for ten seconds.
        for tick in 1..=1200u64 {
            let now = at(Duration::from_millis(tick * 1000 / 120));
            r.step(now);
            let l = r.look(now);
            if l != last {
                changed.push(tick * 1000 / 120);
                last = l;
            }
        }
        // The reveal itself (the first tick), then every tick of the fade,
        // and none in the hold or after it ends.
        assert_eq!(changed.first(), Some(&8));
        assert!(
            changed[1..]
                .iter()
                .all(|&ms| (500..=500 + 247 + 9).contains(&ms))
        );
        assert!(
            (28..=32).contains(&(changed.len() - 1)),
            "{} fade frames",
            changed.len() - 1
        );
    }

    /// **Motion over the legacy gutter reaches no one**, where motion over
    /// the overlay strip is the cells', and motion over the grid always is.
    #[test]
    fn motion_over_the_legacy_gutter_reaches_no_one() {
        assert_eq!(motion(Scroller::Legacy, Place::Strip), Takes::Swallow);
        assert_eq!(motion(Scroller::Overlay, Place::Strip), Takes::Cells);
        assert_eq!(motion(Scroller::Legacy, Place::Grid), Takes::Cells);
    }

    /// **Under the legacy style the look never changes**, through a reveal,
    /// its hold, its fade and a hover -- so the tick forces no frame for it
    /// -- while under the overlay style it is the reveal's own.
    #[test]
    fn under_the_legacy_style_the_look_never_changes() {
        let mut r = Reveal::HIDDEN;
        let first = r.look_under(Scroller::Legacy, at(Duration::ZERO));
        assert_eq!(
            (first.shown.get(), first.hover),
            (255, Hover::Hovered),
            "a legacy knob is always drawn, and wide"
        );
        r.reveal(at(Duration::ZERO));
        for tick in 1..=240u64 {
            let now = at(Duration::from_millis(tick * 1000 / 120));
            if tick == 100 {
                r.hover(Over::Strip, now);
            }
            if tick == 110 {
                r.hover(Over::Cells, now);
            }
            r.step(now);
            assert_eq!(r.look_under(Scroller::Legacy, now), first, "tick {tick}");
            assert_eq!(r.look_under(Scroller::Overlay, now), r.look(now));
        }
    }

    /// **A press the scrollbar takes keeps its release.** A jump swallowed
    /// its press and let the release through to the cells, so a tracking
    /// program saw a release without its press and a selection saw an end
    /// it never began.
    #[test]
    fn a_press_the_scrollbar_takes_keeps_its_release() {
        assert_eq!(Takes::Drag.grabs(), Grab::Bar);
        assert_eq!(Takes::Jump.grabs(), Grab::Bar, "a jump's release leaks");
        assert_eq!(Takes::Swallow.grabs(), Grab::Bar);
        assert_eq!(Takes::Cells.grabs(), Grab::Free);
    }

    /// **Under the legacy style the gutter is never cells**: a left press
    /// drags or jumps when there is a knob, whatever the look, and with no
    /// history, or another button, it is taken and does nothing -- where the
    /// overlay strip, lying over cells, gives those presses to the cells.
    #[test]
    fn the_legacy_gutter_is_never_cells() {
        let gone = Chan::from_u8(0);
        let legacy = |knob, button| press(Scroller::Legacy, Place::Strip, gone, knob, button);
        assert_eq!(legacy(Some(Knob::On), Button::Left), Takes::Drag);
        assert_eq!(legacy(Some(Knob::Off), Button::Left), Takes::Jump);
        assert_eq!(legacy(None, Button::Left), Takes::Swallow);
        assert_eq!(legacy(Some(Knob::On), Button::Other), Takes::Swallow);
        let overlay = |knob, button| press(Scroller::Overlay, Place::Strip, gone, knob, button);
        assert_eq!(overlay(None, Button::Left), Takes::Cells);
        assert_eq!(overlay(Some(Knob::On), Button::Left), Takes::Cells);
        assert_eq!(
            press(
                Scroller::Legacy,
                Place::Grid,
                gone,
                Some(Knob::On),
                Button::Left
            ),
            Takes::Cells
        );
    }

    #[test]
    fn the_strip_hovers_only_while_scrolled_back_and_presses_only_while_seen() {
        let (live, up) = (Rows::ZERO, Rows::of_count(3));
        // The owner's call: at the live screen the strip is cells.
        assert_eq!(over(Place::Strip, live, Motion::Within), Over::Cells);
        assert_eq!(over(Place::Strip, up, Motion::Within), Over::Strip);
        assert_eq!(over(Place::Strip, up, Motion::Leaving), Over::Cells);
        assert_eq!(over(Place::Grid, up, Motion::Within), Over::Cells);
        let seen = Chan::from_u8(1);
        let gone = Chan::from_u8(0);
        let p = |place, shown, knob| press(Scroller::Overlay, place, shown, knob, Button::Left);
        assert_eq!(p(Place::Strip, seen, Some(Knob::On)), Takes::Drag);
        assert_eq!(p(Place::Strip, seen, Some(Knob::Off)), Takes::Jump);
        // Nothing drawn, or outside the strip: the cells'.
        assert_eq!(p(Place::Strip, gone, Some(Knob::On)), Takes::Cells);
        assert_eq!(p(Place::Grid, seen, Some(Knob::On)), Takes::Cells);
        // Another button in the strip is the cells', as it lies over them.
        assert_eq!(
            press(
                Scroller::Overlay,
                Place::Strip,
                seen,
                Some(Knob::On),
                Button::Other
            ),
            Takes::Cells
        );
    }

    /// **The window moves without the pointer, and the hover follows it.**
    /// A wheel, a key or a jump bringing the window to the live screen with
    /// the pointer still in the strip used to leave the thumb widened there,
    /// its track over the live screen, until the pointer moved; and a wheel
    /// taking it off the live screen under a still pointer left it narrow
    /// and fading while hovered.
    #[test]
    fn the_hover_follows_the_window_under_a_still_pointer() {
        let (live, up) = (Rows::ZERO, Rows::of_count(3));
        let mut r = Reveal::HIDDEN;
        r.hover(Over::Strip, at(Duration::ZERO));
        // The window reaches the live screen; the pointer has not moved.
        r.reveal(at(MS * 10));
        r.tick(at(MS * 16), Place::Strip, live, Drag::Idle);
        let l = r.look(at(MS * 16));
        assert_eq!((l.shown.get(), l.hover), (255, Hover::Rest), "held, narrow");
        r.tick(at(MS * (16 + 500 + 247)), Place::Strip, live, Drag::Idle);
        assert_eq!(
            r.look(at(MS * (16 + 500 + 247))).shown.get(),
            0,
            "and faded"
        );
        // The window leaves the live screen; the pointer is still in the strip.
        r.reveal(at(MS * 2_000));
        r.tick(at(MS * 2_008), Place::Strip, up, Drag::Idle);
        assert_eq!(r.look(at(MS * 2_008)).hover, Hover::Hovered);
        r.tick(at(MS * 9_000), Place::Strip, up, Drag::Idle);
        assert_eq!(
            r.look(at(MS * 9_000)).shown.get(),
            255,
            "held for as long as hovered"
        );
    }

    #[test]
    fn a_drag_holds_its_hover_wherever_the_pointer_goes() {
        let up = Rows::of_count(3);
        let mut r = Reveal::HIDDEN;
        r.hover(Over::Strip, at(Duration::ZERO));
        r.tick(at(MS * 5_000), Place::Grid, up, Drag::Dragging);
        let l = r.look(at(MS * 5_000));
        assert_eq!((l.shown.get(), l.hover), (255, Hover::Hovered));
        // Released away from the strip: the next tick lets the hover go.
        r.tick(at(MS * 5_008), Place::Grid, up, Drag::Idle);
        assert_eq!(r.look(at(MS * 5_008)).hover, Hover::Rest);
    }

    #[test]
    fn the_widths_are_appkits_points_at_the_backing_scale() {
        let w = widths(Scale(2.0));
        assert_eq!(
            (
                w.rest.value(),
                w.hovered.value(),
                w.inset.value(),
                w.strip.value()
            ),
            (12, 22, 6, 34)
        );
        let w = widths(Scale(1.0));
        assert_eq!((w.rest.value(), w.hovered.value()), (6, 11));
    }
}
