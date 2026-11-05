//! The cursor's blink: when it shows, when it hides, what it does while it
//! is hidden, and which frames that owes.
//!
//! **The timing is even, 250 ms shown and 250 hidden**: two blinks a
//! second, symmetric, where AppKit's unset caret is 650/350.
//! `[cursor] blink_interval_ms` sets each phase and wins over everything;
//! without it the system's `NSTextInsertionPointBlinkPeriodOn` and `...Off`
//! are followed where the user set them, each read on its own as AppKit
//! reads it, and 250 where not ([`rate`]). Linux reads no desktop setting,
//! so it is the config's interval or 250.
//!
//! **The default is a plain switch**: shown, then gone, with nothing drawn
//! in between. An edge is one even beat; the plasma glow, the default
//! before it, pulsed in the middle of the hidden phase, and that pulse
//! read as a second blink. Two blinks a second are four flips, four
//! luminance changes at the cursor's cells: two flashes in WCAG 2.3.1's
//! sense, under its three (`[[source: wcag22]]`;
//! `baud-bench/tests/blink_flashes.rs` counts them).
//!
//! **The hidden phase is the animation's** (`[cursor] blink_animation`):
//! none by default -- the hard switch -- or the plasma glow, a fade, the
//! fire or the ufo. Each is a function of how far into the hidden phase it
//! is, not of a length of its own, so it scales with any interval and ends
//! inside the phase ([`BlinkAnimation::at`]). Reduce Motion means none.
//!
//! **It blinks only while the window has focus**, as foot arms its blink
//! timer (`term_cursor_blink_update`, whose `activate` needs
//! `term->visual_focus`, in `[[source: foot-term-reset]]`'s file), so a window
//! in the background holds a steady cursor.
//!
//! **The cycle restarts, shown, on input or on a change of the cursor a frame
//! presents** ([`Caret::present`]), and on nothing else. Input is a key sent,
//! a paste, an input method's commit, a focus gain: a key the child does not
//! echo, a password, still shows the cursor. The presented cursor is the one
//! a frame puts on screen -- its cell, whether it is drawn, its shape and
//! blink -- compared with the last frame's, so a cursor that moves as a shell
//! prints is shown while it moves and not hidden half the time. Output that
//! leaves it where, how and whether it was keeps the cycle running: a
//! multiplexer redrawing a spinner in another pane about nine times a second, each
//! frame a synchronised update that hides the cursor, draws elsewhere and puts
//! it back, presents the same cursor every frame, and it blinks. foot restarts
//! the cycle on every read of the child's output instead -- "Prevent blinking
//! while typing" (the same file, 254-258) -- which holds the cursor steady
//! while any pane of a multiplexer draws; baud departs from that.
//!
//! **A cursor that is not blinking draws [`CursorPhase::SHOWN`], and the
//! frame that stops it blinking is owed.** Losing focus, a steady style,
//! `DECTCEM`, a window scrolled back, the system's non-blinking switch, the
//! stop: any of them in the off phase would otherwise leave the cursor
//! hidden until something else drew a frame.
//!
//! **It stops only if asked**, `[cursor] stop_blinking_after`: after that long
//! without input -- a key, a click, a paste, a focus gain -- it stops, shown,
//! and output does not hold it off, so `tail -f` stops blinking (kitty's
//! "keyboard inactivity"). WCAG 2.2's SC 2.2.2 asks that blinking which
//! lasts more than five seconds can be stopped (`[[source: wcag22]]`), and
//! `blink = false` and the stop are that.
//!
//! **What an idle blinking cursor costs is a frame each time what it shows
//! changes**: two a cycle switched hard, the default's four a second, and
//! one a display tick while an animation moves -- counted per animation by
//! a test below. Nothing in a
//! fade's fully hidden middle, and nothing in the shown phase.
//!
//! The clock is the driver's -- the display link's timestamp on macOS, the
//! `poll` timeout on Linux ([`Caret::next_change`]) -- so nothing sleeps and
//! no frame is held.

use core::time::Duration;

use baud_core::{CursorStyle, Focus, Term};
use baud_domain::{Chan, Col, Row};
use baud_render::det::Det;
use baud_render::{CursorLook, CursorPhase};

use crate::bell::{Moment, Update};
pub use crate::config::BlinkAnimation;
use crate::config::{BlinkChoice, BlinkInterval, StopAfter};

/// The shortest phase: below a sixtieth of a second a phase cannot be shown,
/// and every tick would owe a frame. Also the Linux driver's wakeup while an
/// animation moves: one 60 Hz frame.
pub const FLOOR: Duration = Duration::from_nanos(16_666_667);
/// The longest: Chromium's `kMaximumReasonableIntervalMs`. An on phase past it
/// is a steady cursor, as AppKit's is; an off phase past it is clamped, where
/// AppKit hides the caret for good.
pub const CEILING: Duration = Duration::from_secs(60);

/// Each phase where nothing sets it: 250 ms, two blinks a second.
pub const EVEN: Duration = Duration::from_millis(250);

/// The share of the hidden phase each of the fade's two ramps takes: the
/// amendment's "30% of the phase each", 75 ms at 250.
const FADE: f64 = 0.3;

/// One phase of the blink, [`FLOOR`] to [`CEILING`] by construction, so a
/// cycle is never zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Phase(Duration);

impl Phase {
    pub fn get(self) -> Duration {
        self.0
    }

    /// A phase `d` long, clamped into [`FLOOR`]..=[`CEILING`].
    pub fn of(d: Duration) -> Phase {
        Phase(d.clamp(FLOOR, CEILING))
    }

    /// A phase `ms` long, from a value already known positive, finite and
    /// at most [`CEILING`]: clamped up to the floor.
    #[cfg(any(target_os = "macos", test))]
    fn of_ms(ms: f64) -> Phase {
        Duration::try_from_secs_f64(ms / 1000.0).map_or(Phase(FLOOR), Phase::of)
    }
}

/// How the cursor blinks: shown `on`, hidden `off`, or not at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cycle {
    Blinks {
        on: Phase,
        off: Phase,
    },
    #[cfg_attr(
        not(any(target_os = "macos", test)),
        expect(dead_code, reason = "nothing on Linux reads the desktop's caret yet")
    )]
    Steady,
}

impl Cycle {
    /// The cycle where nothing sets it: [`EVEN`] shown and [`EVEN`] hidden.
    pub const DEFAULT: Cycle = Cycle::Blinks {
        on: Phase(EVEN),
        off: Phase(EVEN),
    };

    /// Each phase `d` long, clamped as every phase is: what
    /// `blink_interval_ms` sets.
    pub fn even(d: Duration) -> Cycle {
        Cycle::Blinks {
            on: Phase::of(d),
            off: Phase::of(d),
        }
    }

    /// The cycle the config sets, if it does, else [`Cycle::DEFAULT`]: the
    /// whole rule on Linux, where no desktop setting is read.
    pub fn of_config(interval: BlinkInterval) -> Cycle {
        match interval {
            BlinkInterval::Set(d) => Cycle::even(d),
            BlinkInterval::Default => Cycle::DEFAULT,
        }
    }

    /// The cycle two per-phase defaults describe, each in milliseconds and
    /// read on its own, as AppKit reads them; `unset` gives a phase that is
    /// not set. Total: NaN, zero or negative is unset; an on phase past the
    /// ceiling is steady; an off phase past it is the ceiling; below the
    /// floor is the floor.
    #[cfg(any(target_os = "macos", test))]
    pub fn of_defaults(on_ms: f64, off_ms: f64, unset: Cycle) -> Cycle {
        let (on0, off0) = match unset.or(Cycle::DEFAULT) {
            Cycle::Blinks { on, off } => (on, off),
            Cycle::Steady => unreachable!("or() never answers steady"),
        };
        let ceiling = CEILING.as_secs_f64() * 1000.0;
        let off = match off_ms {
            ms if unset_ms(ms) => off0,
            ms if ms > ceiling => Phase(CEILING),
            ms => Phase::of_ms(ms),
        };
        match on_ms {
            ms if unset_ms(ms) => Cycle::Blinks { on: on0, off },
            ms if ms > ceiling => Cycle::Steady,
            ms => Cycle::Blinks {
                on: Phase::of_ms(ms),
                off,
            },
        }
    }

    /// This cycle, or `fallback` if it is steady.
    #[cfg(any(target_os = "macos", test))]
    fn or(self, fallback: Cycle) -> Cycle {
        match self {
            Cycle::Steady => fallback,
            c => c,
        }
    }
}

/// **The rate, in this precedence**: the config's interval if it sets
/// one, whatever the system says; else the system's two keys, each
/// where the user set it; else [`EVEN`], 250 ms a phase. `on_ms` and `off_ms` are
/// `NSUserDefaults`' answers, 0 for a key that is not set.
#[cfg(any(target_os = "macos", test))]
pub fn rate(interval: BlinkInterval, on_ms: f64, off_ms: f64) -> Cycle {
    match interval {
        BlinkInterval::Set(d) => Cycle::even(d),
        BlinkInterval::Default => Cycle::of_defaults(on_ms, off_ms, Cycle::DEFAULT),
    }
}

impl core::fmt::Display for Cycle {
    /// `250/250 ms`, or `steady`, for the read's log line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Cycle::Blinks { on, off } => write!(
                f,
                "{}/{} ms",
                on.get().as_secs_f64() * 1000.0,
                off.get().as_secs_f64() * 1000.0
            ),
            Cycle::Steady => f.write_str("steady"),
        }
    }
}

/// Whether a default's value leaves its phase unset: NaN, zero or negative.
#[cfg(any(target_os = "macos", test))]
fn unset_ms(ms: f64) -> bool {
    ms.is_nan() || ms <= 0.0
}

/// Smoothstep, `3 s^2 - 2 s^3`: the fade's ease, flat at both ends so a
/// ramp leaves and lands without a jolt.
fn ease(s: f64) -> f64 {
    let s = s.clamp(0.0, 1.0);
    s * s * (3.0 - 2.0 * s)
}

/// A unit fraction as a channel, rounded.
fn chan(v: f64) -> Chan {
    Chan::from_unit(v as f32)
}

impl BlinkAnimation {
    /// **What the cursor shows `h` into a hidden phase `off` long.** A
    /// function of `h / off` alone, so every animation scales with the
    /// phase:
    ///
    /// * `None`: nothing.
    /// * `Fade`: the cursor eased out over the first [`FADE`] of the phase,
    ///   gone for the middle, eased back in over the last -- so it is whole
    ///   at both ends and meets the shown phase with no step.
    /// * `Plasma`: the cursor gone and the glow pulsing `sin^2(pi h / off)`,
    ///   out from nothing and back to nothing at the phase's end, the sine
    ///   `baud_render::det`'s so the pulse is the same on every OS.
    /// * `Fire`: the cursor handed to a flame that burns it down into
    ///   embers and relights it, whole at both ends ([`CursorPhase::fire`]).
    /// * `Ufo`: the cursor handed to a saucer's beam that lifts it away and
    ///   sets it back down, whole at both ends ([`CursorPhase::ufo`]).
    pub fn at(self, h: Duration, off: Duration) -> CursorPhase {
        let u = (h.as_secs_f64() / off.as_secs_f64().max(f64::MIN_POSITIVE)).clamp(0.0, 1.0);
        match self {
            BlinkAnimation::None => CursorPhase::HIDDEN,
            BlinkAnimation::Fire => CursorPhase::fire(chan(u)),
            BlinkAnimation::Ufo => CursorPhase::ufo(chan(u)),
            BlinkAnimation::Fade => {
                let opacity = if u < FADE {
                    1.0 - ease(u / FADE)
                } else if u >= 1.0 - FADE {
                    ease((u - (1.0 - FADE)) / FADE)
                } else {
                    0.0
                };
                CursorPhase {
                    opacity: chan(opacity),
                    ..CursorPhase::HIDDEN
                }
            }
            BlinkAnimation::Plasma => {
                let s = (core::f64::consts::PI * u).det_sin();
                CursorPhase {
                    glow: chan(s * s),
                    ..CursorPhase::HIDDEN
                }
            }
        }
    }

    /// Where, from `h` into a hidden phase `off` long, the drawing next
    /// starts to move or the phase ends: `h` itself while it moves.
    #[cfg(any(target_os = "linux", test))]
    fn next_motion(self, h: Duration, off: Duration) -> Duration {
        match self {
            BlinkAnimation::None => off,
            BlinkAnimation::Plasma | BlinkAnimation::Fire | BlinkAnimation::Ufo => h,
            BlinkAnimation::Fade => {
                let (out, back) = (off.mul_f64(FADE), off.mul_f64(1.0 - FADE));
                if h < out || h >= back { h } else { back }
            }
        }
    }
}

/// The system's non-blinking-caret switch: macOS 15's "prefer non-blinking
/// cursor". Absent -- an older macOS, Linux -- reads off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NonBlinking {
    Off,
    #[cfg_attr(
        not(any(target_os = "macos", test)),
        expect(dead_code, reason = "nothing on Linux reads the desktop's switch yet")
    )]
    On,
}

/// Whether `[cursor] blink = true` overrides the system's non-blinking
/// switch, which is on ([`Caret::read_system`]).
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overridden {
    Yes,
    No,
}

/// Whether the drawing is held steady whatever the style asks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    Free,
    Steady,
}

impl Gate {
    /// The switch steadies the drawn caret unless the config says `true`:
    /// the system's setting is honoured unless the config says otherwise,
    /// and `false` does not say otherwise.
    pub fn of(choice: BlinkChoice, switch: NonBlinking) -> Gate {
        match (switch, choice) {
            (NonBlinking::On, BlinkChoice::Auto | BlinkChoice::Off) => Gate::Steady,
            _ => Gate::Free,
        }
    }
}

/// Whether the cursor blinks this frame at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Blinks {
    Yes,
    No,
}

/// Whether a cursor of `style`, in a window with `focus`, blinks: it has to
/// ask to, the window has to have focus, and there has to be a cursor on the
/// screen being drawn -- `cursor` is the driver's, `None` when `DECTCEM` hid it
/// or the window is looking back.
pub fn blinks<T>(style: CursorStyle, focus: Focus, cursor: Option<T>) -> Blinks {
    if style.blinks() && focus == Focus::Gained && cursor.is_some() {
        Blinks::Yes
    } else {
        Blinks::No
    }
}

/// The cursor to draw: the live screen's, while the window is on it, and
/// on the status line's row below the grid while DECSASD selects the line
/// (`Term::status_line`), where the main display's is not drawn.
pub fn live_cursor(term: &Term<'_>) -> Option<(Row, Col)> {
    if term.window_back().index() != 0 {
        return None;
    }
    match term.status_line().and_then(|l| l.cursor) {
        Some(c) => Some((Row::from_index(term.geometry().1.index()), c)),
        None => term.cursor_visible().then(|| term.cursor()),
    }
}

/// **The cursor as a frame presents it**: its cell, `None` where it is not
/// drawn ([`live_cursor`]), and its style, the shape with the blink `?12`
/// folds in (`Term::cursor_style`). What [`Caret::present`] compares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PresentedCursor {
    at: Option<(Row, Col)>,
    style: CursorStyle,
}

impl PresentedCursor {
    /// The cursor a frame of `term` drawn now presents.
    pub fn of(term: &Term<'_>) -> PresentedCursor {
        PresentedCursor {
            at: live_cursor(term),
            style: term.cursor_style(),
        }
    }
}

/// **How the cursor is drawn in a window with `focus`**: in its shape while
/// the window has the keyboard, and `unfocused` -- `[cursor]
/// unfocused_hollow`'s look -- while another window has it. The one
/// spelling both drivers hand the renderer.
pub fn look(focus: Focus, unfocused: CursorLook) -> CursorLook {
    match focus {
        Focus::Gained => CursorLook::Shaped,
        Focus::Lost => unfocused,
    }
}

/// Whether the blink owes a frame: what a frame drawn now would show is
/// not what the last drawn one showed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owes {
    Frame,
    Nothing,
}

/// The blink's clock, what the last drawn frame showed, and the system's say.
pub struct Caret<M> {
    /// When the current cycle began, shown.
    epoch: M,
    /// Whether the cursor blinked at the last reading, so a cursor that starts
    /// blinking again starts shown -- foot's `CURSOR_BLINK_ON` when it arms.
    was: Blinks,
    /// What the last drawn frame showed.
    drawn: CursorPhase,
    /// The rate, the switch as the config lets it, and the stop.
    cycle: Cycle,
    gate: Gate,
    stop: StopAfter,
    /// What the hidden phase draws, Reduce Motion folded in.
    animation: BlinkAnimation,
    /// The last input; output never moves it.
    quiet_since: M,
    /// The cursor the last frame on screen presented, and the one the frame
    /// being planned presents, which [`Caret::commit`] makes the last.
    presented: Option<PresentedCursor>,
    planned: Option<PresentedCursor>,
}

impl<M: Moment> Caret<M> {
    pub fn new(
        now: M,
        cycle: Cycle,
        gate: Gate,
        stop: StopAfter,
        animation: BlinkAnimation,
    ) -> Caret<M> {
        Caret {
            epoch: now,
            was: Blinks::No,
            drawn: CursorPhase::SHOWN,
            cycle,
            gate,
            stop,
            animation,
            quiet_since: now,
            presented: None,
            planned: None,
        }
    }

    /// **A frame about to present `cursor`, at `now`**, before its
    /// [`Caret::plan`]: the cycle restarts from `now`, shown, when the cursor
    /// differs from the one the last frame on screen presented -- another
    /// cell, shown or hidden, another shape or blink. It does not hold off
    /// the stop. Called only for a frame that is drawn, after the driver's
    /// synchronised-update hold, so what a held update hid and showed again,
    /// or moved away and back, is never seen.
    pub fn present(&mut self, now: M, cursor: PresentedCursor) {
        if self.presented != Some(cursor) {
            self.epoch = now;
        }
        self.planned = Some(cursor);
    }

    /// Input -- a key sent, a paste, a focus gain: the cycle restarts shown,
    /// and the stop is a whole quiet time away again.
    pub fn input(&mut self, now: M) {
        self.epoch = now;
        self.quiet_since = now;
    }

    /// A click or the wheel: the stop is a whole quiet time away again, and
    /// the phase is left alone -- unless the blink had stopped, when it
    /// starts again, shown.
    pub fn quiet(&mut self, now: M) {
        self.quiet_since = now;
    }

    /// The system's rate and switch, read again -- or on Linux, where no
    /// system caret is read, the config's read again (`reload.rs`). No
    /// restart: a blink that resumes restarts at the next plan anyway.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn set_system(&mut self, cycle: Cycle, gate: Gate) {
        self.cycle = cycle;
        self.gate = gate;
    }

    /// **The system's caret read into the blink**: the rate the driver
    /// read, and the non-blinking switch asked of `switch` -- fresh at each
    /// call -- folded with `[cursor] blink` into the gate ([`Gate::of`]).
    /// The macOS driver runs it with the system's readers on its first
    /// tick, at every focus gain and when the config is read again, so a
    /// switch turned while baud was in the background lands at the gain.
    /// It also says whether `blink = true` is blinking a caret the switch
    /// asks to hold steady, which the driver says once.
    #[cfg(any(target_os = "macos", test))]
    pub fn read_system(
        &mut self,
        cycle: Cycle,
        choice: crate::config::BlinkChoice,
        switch: impl FnOnce() -> NonBlinking,
    ) -> (NonBlinking, Overridden) {
        let switch = switch();
        self.set_system(cycle, Gate::of(choice, switch));
        let overridden = match (switch, choice) {
            (NonBlinking::On, crate::config::BlinkChoice::On) => Overridden::Yes,
            _ => Overridden::No,
        };
        (switch, overridden)
    }

    /// `[cursor] stop_blinking_after`, read again with the config
    /// (`reload.rs`): the next plan stops on the new time.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn set_stop(&mut self, stop: StopAfter) {
        self.stop = stop;
    }

    /// The animation, after the motion preference moved. The next plan
    /// draws it from where the cycle is.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn set_animation(&mut self, animation: BlinkAnimation) {
        self.animation = animation;
    }

    /// Whether the cursor blinks at `now`, with the system's say folded in.
    fn blinking(&self, now: M, blinks: Blinks) -> Blinks {
        let stopped = match self.stop {
            StopAfter::After(d) => now.elapsed_since(self.quiet_since) >= d,
            StopAfter::Never => false,
        };
        match (self.gate, self.cycle, stopped) {
            (Gate::Free, Cycle::Blinks { .. }, false) => blinks,
            _ => Blinks::No,
        }
    }

    /// What a frame drawn at `now` shows. A cursor that does not blink
    /// shows; one that starts blinking again restarts its cycle, shown.
    pub fn plan(&mut self, now: M, blinks: Blinks) -> CursorPhase {
        let blinks = self.blinking(now, blinks);
        if blinks == Blinks::Yes && self.was == Blinks::No {
            self.epoch = now;
        }
        self.was = blinks;
        match (blinks, self.cycle) {
            (Blinks::Yes, Cycle::Blinks { on, off }) => {
                let into = cycle_offset(now.elapsed_since(self.epoch), on.get(), off.get());
                match into.checked_sub(on.get()) {
                    None => CursorPhase::SHOWN,
                    Some(h) => self.animation.at(h, off.get()),
                }
            }
            _ => CursorPhase::SHOWN,
        }
    }

    /// **What the blink owes a tick at `now`, under `update`**: nothing
    /// while a synchronised update holds the frame, else [`Caret::due`].
    /// A held update's cursor is the program's half-drawn one -- a
    /// multiplexer hides it while it draws another pane -- and no frame can
    /// show it, so reading it would only restart the cycle when the update
    /// closes on the cursor it opened on.
    pub fn tick(&mut self, now: M, blinks: Blinks, update: Update) -> Owes {
        match update {
            Update::Honoured => Owes::Nothing,
            Update::Closed | Update::Outlived => self.due(now, blinks),
        }
    }

    /// Whether a frame drawn at `now` would show other than the last drawn
    /// frame did: the tick owes a frame when it would.
    pub fn due(&mut self, now: M, blinks: Blinks) -> Owes {
        if self.plan(now, blinks) == self.drawn {
            Owes::Nothing
        } else {
            Owes::Frame
        }
    }

    /// A frame showing `phase`, and the cursor [`Caret::present`] was told
    /// of, reached the screen. One put off or refused does not call this, so
    /// the next asks again.
    pub fn commit(&mut self, phase: CursorPhase) {
        self.drawn = phase;
        if let Some(cursor) = self.planned.take() {
            self.presented = Some(cursor);
        }
    }

    /// When the drawing next changes, for a `poll` timeout: the next flip,
    /// the next start of an animation's motion, a frame's time while it
    /// moves, or the stop if it comes first; `None` while the cursor does
    /// not blink, so a steady cursor adds no wakeup. The macOS driver reads
    /// the display link instead and has no use for it.
    #[cfg(any(target_os = "linux", test))]
    pub fn next_change(&self, now: M, blinks: Blinks) -> Option<M> {
        if self.blinking(now, blinks) == Blinks::No {
            return None;
        }
        let Cycle::Blinks { on, off } = self.cycle else {
            return None;
        };
        let (on, off) = (on.get(), off.get());
        let into = cycle_offset(now.elapsed_since(self.epoch), on, off);
        let next = match into.checked_sub(on) {
            None => on - into,
            Some(h) => {
                let at = self.animation.next_motion(h, off);
                if at > h { at - h } else { FLOOR.min(off - h) }
            }
        };
        let left = match self.stop {
            StopAfter::After(d) => next.min(d.saturating_sub(now.elapsed_since(self.quiet_since))),
            StopAfter::Never => next,
        };
        Some(now.after(left))
    }
}

/// How far into its cycle a cursor is, `elapsed` after the cycle began. The
/// cycle is at least two floors, so the remainder is never taken by zero.
fn cycle_offset(elapsed: Duration, on: Duration, off: Duration) -> Duration {
    let cycle = (on + off).as_nanos().max(1);
    let into = elapsed.as_nanos() % cycle;
    // Below one cycle, which is at most two minutes, so it fits.
    Duration::from_nanos(u64::try_from(into).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, WinSize};
    use baud_render::Play;
    use std::time::Instant;

    const MS: Duration = Duration::from_millis(1);
    const SHOWN: CursorPhase = CursorPhase::SHOWN;
    const HIDDEN: CursorPhase = CursorPhase::HIDDEN;

    /// **The blink calls no platform math**: the plasma's pulse is a pixel
    /// input on every OS, so its sine is `det`'s, not the host's libm's.
    /// Perturbation: std's sine back in the plasma's pulse fails it.
    #[test]
    fn the_blink_calls_no_platform_math() {
        let src = include_str!("caret.rs");
        for call in baud_render::det::PLATFORM {
            assert!(!src.contains(call), "caret.rs calls the platform's {call}");
        }
        let control = concat!(".det_", "sin(");
        assert!(
            src.contains(control),
            "the control: the scan reads the pulse"
        );
    }

    /// A rate the user set in the system's keys, 650 ms shown and 350
    /// hidden -- what AppKit's caret is when they are unset, which baud no
    /// longer takes -- switched hard: the clock's rules are the same at any
    /// rate, and these tests pin them at this one.
    const SET: Cycle = Cycle::Blinks {
        on: Phase(Duration::from_millis(650)),
        off: Phase(Duration::from_millis(350)),
    };

    /// **A hollow cursor only without the keyboard, and only when asked**:
    /// in front the shape, behind it the setting's look.
    #[test]
    fn the_cursor_is_hollow_only_behind_another_window_and_only_when_asked() {
        for unfocused in [CursorLook::Shaped, CursorLook::Hollow] {
            assert_eq!(
                look(Focus::Gained, unfocused),
                CursorLook::Shaped,
                "{unfocused:?}"
            );
            assert_eq!(look(Focus::Lost, unfocused), unfocused, "{unfocused:?}");
        }
    }

    /// A blinking block presented on the first row at column `col`: a
    /// cursor output moved there.
    fn at_col(col: Col) -> PresentedCursor {
        PresentedCursor {
            at: Some((Row::from_index(0), col)),
            style: CursorStyle::BlinkingBlock,
        }
    }

    fn caret(t0: Instant) -> Caret<Instant> {
        Caret::new(t0, SET, Gate::Free, StopAfter::Never, BlinkAnimation::None)
    }

    fn cycle(on: Duration, off: Duration) -> Cycle {
        Cycle::Blinks {
            on: Phase(on),
            off: Phase(off),
        }
    }

    /// The frames a 120 Hz display link draws from `from` to `to` ms after
    /// `t0`, as the driver ticks: a frame when one is owed, of a blinking
    /// block in a window with `focus`; whether any showed it hidden.
    fn hides(c: &mut Caret<Instant>, t0: Instant, (from, to): (u64, u64), focus: Focus) -> bool {
        let style = CursorStyle::of(
            baud_core::CursorShape::Block,
            baud_core::CursorBlink::Blinking,
        );
        let mut hid = false;
        for tick in from * 120 / 1000..to * 120 / 1000 {
            let now = t0 + Duration::from_nanos(tick * 1_000_000_000 / 120);
            let b = blinks(style, focus, Some(()));
            if c.due(now, b) == Owes::Frame {
                let p = c.plan(now, b);
                c.commit(p);
                hid |= p != SHOWN;
            }
        }
        hid
    }

    /// **The system's switch turned while baud is in the background lands
    /// at the focus gain**, the driver's path (`Caret::read_system`, run by
    /// `read_caret_system` at every gain): blinking with the switch off; no
    /// blink and no read behind another window, where the switch is turned
    /// on; steady from the gain that reads it; and blinking again from the
    /// gain after it is turned off. The switch is a stub the test turns, as
    /// System Settings would, and `[cursor] blink` is the default, `auto`.
    /// Perturbations: a read that ignores the switch blinks after the
    /// second gain; one that reads nothing at a gain stays steady after the
    /// third.
    #[test]
    fn a_switch_turned_in_the_background_lands_at_the_focus_gain() {
        use crate::config::BlinkChoice;
        let t0 = Instant::now();
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        let rate = cycle(MS * 333, MS * 333);
        let mut c = caret(t0);
        let system = std::cell::Cell::new(NonBlinking::Off);
        let gain = |c: &mut Caret<Instant>, ms| {
            let read = c.read_system(rate, BlinkChoice::Auto, || system.get());
            c.input(at(ms));
            read
        };
        assert_eq!(gain(&mut c, 0), (NonBlinking::Off, Overridden::No));
        assert!(
            hides(&mut c, t0, (0, 2000), Focus::Gained),
            "blinks with the switch off"
        );
        assert!(
            !hides(&mut c, t0, (2000, 4000), Focus::Lost),
            "behind another window"
        );
        system.set(NonBlinking::On);
        assert!(
            !hides(&mut c, t0, (4000, 6000), Focus::Lost),
            "turned on, unread"
        );
        assert_eq!(gain(&mut c, 6000), (NonBlinking::On, Overridden::No));
        assert!(
            !hides(&mut c, t0, (6000, 9000), Focus::Gained),
            "steady from the gain"
        );
        assert!(!hides(&mut c, t0, (9000, 10000), Focus::Lost));
        system.set(NonBlinking::Off);
        gain(&mut c, 10000);
        assert!(
            hides(&mut c, t0, (10000, 12000), Focus::Gained),
            "blinks again"
        );
    }

    /// **`[cursor] blink = true` overrides the switch, and says so; `auto`
    /// and `false` honour it**: `true` "blinks whatever that switch says,
    /// with a warning" (`config.toml.example`), and `false`, which starts
    /// the cursor steady, holds a blink a program asks for steady under the
    /// switch as `auto` does (`Gate::of`). Perturbation: a gate that lets `true`
    /// honour the switch fails the first arm, and one that lets `false`
    /// override it the third.
    #[test]
    fn blink_true_overrides_the_switch_and_auto_and_false_honour_it() {
        use crate::config::BlinkChoice;
        let rate = cycle(MS * 333, MS * 333);
        for (choice, hid, said) in [
            (BlinkChoice::On, true, Overridden::Yes),
            (BlinkChoice::Auto, false, Overridden::No),
            (BlinkChoice::Off, false, Overridden::No),
        ] {
            let t0 = Instant::now();
            let mut c = caret(t0);
            let read = c.read_system(rate, choice, || NonBlinking::On);
            assert_eq!(read, (NonBlinking::On, said), "{choice:?}");
            assert_eq!(
                hides(&mut c, t0, (0, 2000), Focus::Gained),
                hid,
                "{choice:?}"
            );
            let read = c.read_system(rate, choice, || NonBlinking::Off);
            assert_eq!(read.1, Overridden::No, "{choice:?} with the switch off");
            assert!(
                hides(&mut c, t0, (2000, 4000), Focus::Gained),
                "{choice:?} off"
            );
        }
    }

    /// What happens at a tick, before the frame it may owe.
    #[derive(Clone, Copy, PartialEq)]
    enum Event {
        None,
        Output,
        Key,
        Click,
    }

    /// The 120 Hz display's ticks over `c` for `over`, each drawing a frame
    /// when one is owed, with `event` saying what arrived at each tick
    /// first: the tick and phase of every frame the blink cost drew.
    fn ticks(
        c: &mut Caret<Instant>,
        t0: Instant,
        over: Duration,
        blinks: Blinks,
        event: impl Fn(u64) -> Event,
    ) -> Vec<(u64, CursorPhase)> {
        let mut drawn = Vec::new();
        for tick in 1.. {
            let at = Duration::from_nanos(tick * 1_000_000_000 / 120);
            if at > over {
                break;
            }
            let now = t0 + at;
            match event(tick) {
                // Output that moves the cursor: shell output, `tail -f`.
                Event::Output => {
                    c.present(now, at_col(Col::at_index(usize::try_from(tick).unwrap())));
                }
                Event::Key => c.input(now),
                Event::Click => c.quiet(now),
                Event::None => {}
            }
            if c.due(now, blinks) == Owes::Frame {
                let phase = c.plan(now, blinks);
                c.commit(phase);
                drawn.push((tick, phase));
            }
        }
        drawn
    }

    /// Blinking and focused, a frame at every flip: two a 1.0 s cycle. The
    /// first plan, at tick 1, restarts the cycle, so the hides fall on ticks
    /// 79 + 120k and the shows on 121 + 120k: ten hides and nine shows in
    /// 1200 ticks, the tenth show at 1201 being past them. 600/300 counted
    /// 22. Not blinking, none -- an idle steady cursor builds no frame.
    #[test]
    fn a_blinking_cursor_costs_two_frames_a_cycle_and_a_steady_one_none() {
        let t0 = Instant::now();
        let drawn = ticks(
            &mut caret(t0),
            t0,
            Duration::from_secs(10),
            Blinks::Yes,
            |_| Event::None,
        );
        assert_eq!(drawn.len(), 19);
        assert_eq!(drawn[0], (79, HIDDEN));
        assert_eq!(drawn[1], (121, SHOWN));
        // Each frame shows the other phase from the one before: a frame per
        // flip and none between.
        assert!(drawn.windows(2).all(|w| w[0].1 != w[1].1), "{drawn:?}");
        let steady = ticks(
            &mut caret(t0),
            t0,
            Duration::from_secs(10),
            Blinks::No,
            |_| Event::None,
        );
        assert!(steady.is_empty());
    }

    /// A rate the user set is followed: shown for 650 ms, hidden for 350.
    #[test]
    fn a_set_rate_is_followed_six_fifty_on_and_three_fifty_off() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(0), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(649), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(650), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(999), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(1000), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(1650), Blinks::Yes), HIDDEN);
    }

    /// Each default is read on its own, as AppKit reads them -- Chromium
    /// averages the two and baud does not -- and the parse is total.
    #[test]
    fn each_key_is_read_on_its_own_as_appkit_reads_it() {
        let a = SET;
        assert_eq!(Cycle::of_defaults(0.0, 0.0, a), a, "both unset");
        assert_eq!(Cycle::of_defaults(400.0, 0.0, a), cycle(MS * 400, MS * 350));
        assert_eq!(Cycle::of_defaults(0.0, 400.0, a), cycle(MS * 650, MS * 400));
        assert_eq!(
            Cycle::of_defaults(1000.0, 200.0, a),
            cycle(MS * 1000, MS * 200)
        );
        // Unset, negative and NaN fall back per phase, not to AppKit's 250
        // ms for a stored NaN.
        assert_eq!(Cycle::of_defaults(-5.0, f64::NAN, a), a);
        assert_eq!(Cycle::of_defaults(f64::NAN, -1.0, a), a);
        // A fraction of a millisecond is kept.
        assert_eq!(
            Cycle::of_defaults(400.7, 0.0, a),
            Cycle::Blinks {
                on: Phase(Duration::from_micros(400_700)),
                off: Phase(MS * 350),
            }
        );
        // Past the ceiling an on phase is steady, as AppKit's is, and an off
        // phase is the ceiling, where AppKit would hide the caret for good.
        assert_eq!(Cycle::of_defaults(1e308, 0.0, a), Cycle::Steady);
        assert_eq!(Cycle::of_defaults(f64::INFINITY, 0.0, a), Cycle::Steady);
        assert_eq!(
            Cycle::of_defaults(0.0, 1e308, a),
            Cycle::Blinks {
                on: Phase(MS * 650),
                off: Phase(CEILING),
            }
        );
        assert_eq!(
            Cycle::of_defaults(0.0, f64::INFINITY, a),
            Cycle::of_defaults(0.0, 1e308, a)
        );
        // Below the floor is the floor.
        assert_eq!(
            Cycle::of_defaults(1e-12, 1e-12, a),
            Cycle::Blinks {
                on: Phase(FLOOR),
                off: Phase(FLOOR),
            }
        );
        // The ceiling itself is kept, and a hair past it is not.
        assert_eq!(
            Cycle::of_defaults(60_000.0, 0.0, a),
            Cycle::Blinks {
                on: Phase(CEILING),
                off: Phase(MS * 350),
            }
        );
        assert_eq!(Cycle::of_defaults(60_000.001, 0.0, a), Cycle::Steady);
        // Both unset against the default is the default.
        assert_eq!(
            Cycle::of_defaults(0.0, 0.0, Cycle::DEFAULT),
            cycle(MS * 250, MS * 250)
        );
    }

    /// **Unset, the cycle is 250 ms shown and 250 hidden**, on macOS and on
    /// Linux alike -- not AppKit's unset 650/350 nor xterm's 600/300 -- and
    /// a caret on it shows for 250 and hides for 250: two blinks a second.
    /// Perturbation: `EVEN` back at 333 fails every line.
    #[test]
    fn the_unset_cycle_is_two_fifty_each_phase() {
        let even = cycle(MS * 250, MS * 250);
        assert_eq!(EVEN, MS * 250);
        assert_eq!(Cycle::DEFAULT, even);
        assert_eq!(rate(BlinkInterval::Default, 0.0, 0.0), even, "macOS");
        assert_eq!(Cycle::of_config(BlinkInterval::Default), even, "Linux");
        let t0 = Instant::now();
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        let mut c = Caret::new(
            t0,
            Cycle::of_config(BlinkInterval::Default),
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::default(),
        );
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(249), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(250), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(375), Blinks::Yes), HIDDEN, "a hard switch");
        assert_eq!(c.plan(at(499), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(500), Blinks::Yes), SHOWN);
    }

    /// **The default blink is the hard switch**: the hidden phase is
    /// [`HIDDEN`] from its first instant to its last, and nothing between
    /// -- no glow, no fade. Perturbation: the plasma back as the default
    /// fails the first line, and its glow the sweep.
    #[test]
    fn the_default_blink_is_a_plain_switch() {
        assert_eq!(BlinkAnimation::default(), BlinkAnimation::None);
        let a = BlinkAnimation::default();
        for k in 0..2500u32 {
            let h = Duration::from_micros(100) * k;
            assert_eq!(a.at(h, EVEN), HIDDEN, "{h:?}");
        }
        assert_eq!(a.next_motion(Duration::ZERO, EVEN), EVEN, "nothing moves");
    }

    /// **The default's idle cost: two frames a cycle, four a second**, on
    /// 120 Hz ticks over ten seconds -- the flips and nothing else
    /// ([`FRAMES_NONE`]), where the plasma glow, the default before, drew a
    /// frame each tick of its hidden phase at 333/333: 613 in ten seconds.
    /// Perturbation: the plasma back as the default fails both assertions,
    /// and draws [`FRAMES_PLASMA`].
    #[test]
    fn the_default_blink_draws_only_its_flips() {
        let t0 = Instant::now();
        let mut c = Caret::new(
            t0,
            Cycle::of_config(BlinkInterval::Default),
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::default(),
        );
        let drawn = ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
            Event::None
        });
        assert!(
            drawn.iter().all(|d| d.1 == SHOWN || d.1 == HIDDEN),
            "{drawn:?}"
        );
        assert_eq!(drawn.len(), FRAMES_NONE, "the flips alone");
    }

    /// **The system's keys are followed only where the user set them**, each
    /// on its own; a key unset is that phase's 250, and a key set wins
    /// over it.
    #[test]
    fn the_system_keys_are_followed_only_where_set() {
        let d = BlinkInterval::Default;
        assert_eq!(rate(d, 650.0, 350.0), cycle(MS * 650, MS * 350));
        assert_eq!(rate(d, 400.0, 0.0), cycle(MS * 400, MS * 250));
        assert_eq!(rate(d, 0.0, 200.0), cycle(MS * 250, MS * 200));
        assert_eq!(rate(d, f64::NAN, -1.0), cycle(MS * 250, MS * 250));
        assert_eq!(rate(d, 1e308, 0.0), Cycle::Steady, "the system's steady");
    }

    /// **`blink_interval_ms` wins over the system's keys**, set or not, and
    /// is clamped as every phase is. It is what the read at a focus gain
    /// sets, so a gain does not hand the rate back to the system.
    #[test]
    fn the_config_interval_wins_over_the_system_keys() {
        let set = |ms: u64| BlinkInterval::Set(MS * u32::try_from(ms).unwrap());
        assert_eq!(rate(set(400), 0.0, 0.0), cycle(MS * 400, MS * 400));
        assert_eq!(rate(set(400), 650.0, 350.0), cycle(MS * 400, MS * 400));
        assert_eq!(rate(set(250), 0.0, 0.0), cycle(MS * 250, MS * 250));
        assert_eq!(rate(set(250), 1000.0, 200.0), cycle(MS * 250, MS * 250));
        assert_eq!(rate(set(250), 1e308, 0.0), cycle(MS * 250, MS * 250));
        assert_eq!(
            rate(BlinkInterval::Set(Duration::from_micros(10)), 0.0, 0.0),
            cycle(FLOOR, FLOOR)
        );
        assert_eq!(
            rate(BlinkInterval::Set(Duration::from_secs(600)), 0.0, 0.0),
            cycle(CEILING, CEILING)
        );
        assert_eq!(Cycle::of_config(set(500)), cycle(MS * 500, MS * 500));
        // The gain's read: the system says 1000/200, the config 250.
        let t0 = Instant::now();
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        let mut c = Caret::new(
            t0,
            Cycle::of_config(set(250)),
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::None,
        );
        c.set_system(rate(set(250), 1000.0, 200.0), Gate::Free);
        c.input(at(10));
        c.plan(at(10), Blinks::Yes);
        assert_eq!(c.plan(at(259), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(260), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(510), Blinks::Yes), SHOWN);
    }

    const ANIMATIONS: [BlinkAnimation; 5] = [
        BlinkAnimation::Plasma,
        BlinkAnimation::Fade,
        BlinkAnimation::Fire,
        BlinkAnimation::Ufo,
        BlinkAnimation::None,
    ];

    /// **Every animation ends inside the hidden phase, at any interval**:
    /// its length, measured as the last sample whose drawing differs from
    /// the one before, is inside the phase and scales with it -- at least
    /// nine tenths of it for the fade and the plasma, nothing for a hard
    /// switch; the shown phase is the whole cursor and nothing else; the
    /// fade leaves and lands whole and the plasma's glow rises from nothing
    /// and returns to it, so the flip into the shown phase meets the drawing
    /// at rest. Sampled every tenth of a millisecond across a whole cycle at
    /// 100, 250, 333, 500 and 1000 ms.
    #[test]
    fn every_animation_ends_inside_the_hidden_phase_at_any_interval() {
        for ms in [100u32, 250, 333, 500, 1000] {
            let off = MS * ms;
            for a in ANIMATIONS {
                let t0 = Instant::now();
                let mut c = Caret::new(t0, Cycle::even(off), Gate::Free, StopAfter::Never, a);
                c.plan(t0, Blinks::Yes);
                let step = Duration::from_micros(100);
                let mut h_peak = (Duration::ZERO, 0u8);
                let (mut last, mut length) = (a.at(Duration::ZERO, off), Duration::ZERO);
                for k in 0..(2 * ms * 10) {
                    let into = step * k;
                    let p = c.plan(t0 + into, Blinks::Yes);
                    match into.checked_sub(off) {
                        None => assert_eq!(p, SHOWN, "{a:?} at {ms}: {into:?}"),
                        Some(h) => {
                            assert_eq!(p, a.at(h, off), "{a:?} at {ms}: {h:?}");
                            if p != last {
                                (last, length) = (p, h);
                            }
                            if p.glow.get() > h_peak.1 {
                                h_peak = (h, p.glow.get());
                            }
                        }
                    }
                }
                assert!(length < off, "{a:?} at {ms}: moves until {length:?}");
                let share = length.as_secs_f64() / off.as_secs_f64();
                match a {
                    BlinkAnimation::None => assert_eq!(length, Duration::ZERO, "{ms}"),
                    _ => assert!(share >= 0.9, "{a:?} at {ms}: {share} of the phase"),
                }
                let start = a.at(Duration::ZERO, off);
                let end = a.at(off - Duration::from_micros(1), off);
                match a {
                    BlinkAnimation::Fade => {
                        assert_eq!((start, end), (SHOWN, SHOWN), "{ms}: whole at both ends");
                        let mid = a.at(off / 2, off);
                        assert_eq!(mid, HIDDEN, "{ms}: gone in the middle");
                        // 30% each side: whole-ish ramps, gone from 30% to 70%.
                        assert_eq!(a.at(off.mul_f64(0.30), off), HIDDEN, "{ms}");
                        assert_eq!(a.at(off.mul_f64(0.6999), off), HIDDEN, "{ms}");
                        assert!(a.at(off.mul_f64(0.15), off).opacity.get() > 100, "{ms}");
                        assert!(a.at(off.mul_f64(0.85), off).opacity.get() > 100, "{ms}");
                        // Symmetric: the ramp in mirrors the ramp out.
                        for f in [0.05, 0.1, 0.2, 0.25] {
                            assert_eq!(
                                a.at(off.mul_f64(f), off),
                                a.at(off.mul_f64(1.0 - f), off),
                                "{ms} at {f}"
                            );
                        }
                    }
                    BlinkAnimation::Fire => {
                        assert_eq!((start, end), (SHOWN, SHOWN), "{ms}: whole at both ends");
                        // Burnt down and not yet relit: only embers, the
                        // cursor itself gone.
                        let embers = a.at(off.mul_f64(0.5), off);
                        assert_eq!(embers.opacity.get(), 0, "{ms}: {embers:?}");
                        assert!(matches!(embers.play, Play::Fire(_)), "{ms}: {embers:?}");
                        // The cursor hands over to the flame in the first 8%
                        // and back in the last.
                        assert_eq!(a.at(off.mul_f64(0.08), off).opacity.get(), 0, "{ms}");
                        assert!(a.at(off.mul_f64(0.04), off).opacity.get() > 100, "{ms}");
                        assert!(a.at(off.mul_f64(0.96), off).opacity.get() > 100, "{ms}");
                    }
                    BlinkAnimation::Ufo => {
                        assert_eq!((start, end), (SHOWN, SHOWN), "{ms}: whole at both ends");
                        // Up in the saucer: the cursor itself gone, the
                        // effect drawing it.
                        let up = a.at(off.mul_f64(0.5), off);
                        assert_eq!(up.opacity.get(), 0, "{ms}: {up:?}");
                        assert!(matches!(up.play, Play::Ufo(_)), "{ms}: {up:?}");
                        assert_eq!(a.at(off.mul_f64(0.08), off).opacity.get(), 0, "{ms}");
                        assert!(a.at(off.mul_f64(0.04), off).opacity.get() > 100, "{ms}");
                        assert!(a.at(off.mul_f64(0.96), off).opacity.get() > 100, "{ms}");
                    }
                    BlinkAnimation::Plasma => {
                        assert_eq!((start, end), (HIDDEN, HIDDEN), "{ms}: no glow at the ends");
                        assert_eq!(h_peak.1, 255, "{ms}: the pulse reaches its height");
                        let mid = off / 2;
                        assert!(h_peak.0.abs_diff(mid) <= off / 20, "{ms}: {h_peak:?}");
                    }
                    BlinkAnimation::None => assert_eq!((start, end), (HIDDEN, HIDDEN)),
                }
            }
        }
    }

    /// **Reduce Motion means none**: the plasma, under it, is the hard
    /// switch -- no glow, no fade, two frames a cycle.
    #[test]
    fn reduce_motion_draws_a_hard_blink() {
        use crate::config::SystemMotion;
        let t0 = Instant::now();
        let mut c = Caret::new(
            t0,
            Cycle::DEFAULT,
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::Plasma.effective(SystemMotion::Reduced),
        );
        let drawn = ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
            Event::None
        });
        assert!(
            drawn.iter().all(|d| d.1 == SHOWN || d.1 == HIDDEN),
            "{drawn:?}"
        );
        assert_eq!(
            drawn.len(),
            FRAMES_NONE,
            "two frames a 500 ms cycle over ten seconds"
        );
        let mut c = Caret::new(
            t0,
            Cycle::DEFAULT,
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::Plasma,
        );
        c.plan(t0, Blinks::Yes);
        let mid = t0 + EVEN + EVEN / 2;
        assert_ne!(
            c.plan(mid, Blinks::Yes),
            HIDDEN,
            "the control: plasma glows"
        );
        c.set_animation(BlinkAnimation::Plasma.effective(SystemMotion::Reduced));
        assert_eq!(c.plan(mid, Blinks::Yes), HIDDEN, "and stops at the switch");
    }

    /// **What an idle blinking cursor costs, per animation, on 120 Hz
    /// ticks over ten seconds at the default 250/250**: a frame each time
    /// the drawing changes, which is only while an animation moves. The
    /// hard switch, the default, draws two a cycle; the fade a frame a tick
    /// through its two 75 ms ramps and nothing through its gone middle; the
    /// plasma a
    /// frame a tick through the whole hidden phase. Nothing in a shown
    /// phase but the flip into it.
    #[test]
    fn frames_a_cycle_for_each_animation() {
        let t0 = Instant::now();
        let mut counts = Vec::new();
        for a in ANIMATIONS {
            let mut c = Caret::new(t0, Cycle::DEFAULT, Gate::Free, StopAfter::Never, a);
            let drawn = ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
                Event::None
            });
            // Every frame drawn is in a hidden phase, or the flip into a
            // shown one, which shows it whole: the cycle restarts at tick 1.
            let at = |tick: u64| Duration::from_nanos(tick * 1_000_000_000 / 120);
            for &(tick, p) in &drawn {
                let into = cycle_offset(at(tick) - at(1), EVEN, EVEN);
                match into.checked_sub(EVEN) {
                    None => assert_eq!(p, SHOWN, "{a:?}: {tick}"),
                    Some(h) if a == BlinkAnimation::Fade => assert!(
                        h < EVEN.mul_f64(0.3) + MS * 9 || h >= EVEN.mul_f64(0.7),
                        "the fade draws nothing while the cursor is gone: {tick} {p:?}"
                    ),
                    Some(_) => {}
                }
            }
            counts.push(drawn.len());
        }
        // Ten seconds is 20 cycles of 500 ms; the counts per cycle are
        // these over 20.
        assert_eq!(
            counts,
            [
                FRAMES_PLASMA,
                FRAMES_FADE,
                FRAMES_FIRE,
                FRAMES_UFO,
                FRAMES_NONE
            ],
            "{counts:?}"
        );
    }

    /// About 31 a cycle: each of the hidden phase's 30 ticks, and the flip
    /// back; sixteen times the hard switch's. The default at 333/333 drew
    /// 613 in ten seconds, about 41 a 666 ms cycle.
    const FRAMES_PLASMA: usize = 619;
    /// About 18 a cycle: the two 75 ms ramps' 9 ticks each, give or take
    /// the tick a ramp's first and last value round alike.
    const FRAMES_FADE: usize = 359;
    /// About 30 a cycle: the flame moves through the whole hidden phase,
    /// and its last tick rounds to the whole cursor the flip shows, so one
    /// fewer than the plasma.
    const FRAMES_FIRE: usize = 599;
    /// The same for the ufo: its rings and rim light move while the cursor
    /// is held up.
    const FRAMES_UFO: usize = 599;
    /// The hard switch, the default: the flips alone, 20 hides and 19
    /// shows. The cycle restarts at tick 1, so the twentieth show falls a
    /// tick past the ten seconds.
    const FRAMES_NONE: usize = 39;

    /// **On Linux the `poll` timeout follows the animation**: the flip in
    /// the shown phase, a frame's time while the drawing moves, and across
    /// a fade's gone middle the start of its ramp back.
    #[test]
    fn the_next_change_follows_the_animation() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        let mut c = Caret::new(
            t0,
            Cycle::even(MS * 1000),
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::Fade,
        );
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.next_change(at(100), Blinks::Yes), Some(at(1000)), "shown");
        assert_eq!(
            c.next_change(at(1100), Blinks::Yes),
            Some(at(1100) + FLOOR),
            "ramp out"
        );
        assert_eq!(c.next_change(at(1500), Blinks::Yes), Some(at(1700)), "gone");
        assert_eq!(
            c.next_change(at(1800), Blinks::Yes),
            Some(at(1800) + FLOOR),
            "ramp in"
        );
        assert_eq!(
            c.next_change(at(1995), Blinks::Yes),
            Some(at(2000)),
            "the flip"
        );
        let mut p = Caret::new(
            t0,
            Cycle::even(MS * 1000),
            Gate::Free,
            StopAfter::Never,
            BlinkAnimation::Plasma,
        );
        p.plan(t0, Blinks::Yes);
        assert_eq!(
            p.next_change(at(1500), Blinks::Yes),
            Some(at(1500) + FLOOR),
            "glowing"
        );
        p.set_animation(BlinkAnimation::Fire);
        assert_eq!(
            p.next_change(at(1500), Blinks::Yes),
            Some(at(1500) + FLOOR),
            "burning"
        );
        p.set_animation(BlinkAnimation::Ufo);
        assert_eq!(
            p.next_change(at(1500), Blinks::Yes),
            Some(at(1500) + FLOOR),
            "held up"
        );
    }

    /// No input the defaults can hold gives a phase of zero, so a cycle is
    /// never taken modulo zero.
    #[test]
    fn no_phase_is_zero() {
        let floor = FLOOR.as_secs_f64() * 1000.0;
        let ceiling = CEILING.as_secs_f64() * 1000.0;
        let edges = [
            0.0,
            -0.0,
            f64::MIN_POSITIVE / 2.0,
            f64::MIN_POSITIVE,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
            floor,
            floor.next_down(),
            floor.next_up(),
            ceiling,
            ceiling.next_down(),
            ceiling.next_up(),
            1e-300,
            1e300,
        ];
        for on in edges {
            for off in edges {
                for unset in [SET, Cycle::DEFAULT, Cycle::Steady] {
                    if let Cycle::Blinks { on: p, off: q } = Cycle::of_defaults(on, off, unset) {
                        assert!(p.get() >= FLOOR && q.get() >= FLOOR, "{on} {off}");
                        assert!(p.get() <= CEILING && q.get() <= CEILING, "{on} {off}");
                    }
                }
            }
        }
    }

    /// Input restarts the cycle shown, and a frame is owed if the drawn one
    /// had the cursor hidden; the next hide is a whole on phase later.
    /// Output that moves the presented cursor restarts it the same way.
    #[test]
    fn input_and_output_restart_the_cycle() {
        for restart in [Event::Key, Event::Output] {
            let t0 = Instant::now();
            let mut c = caret(t0);
            let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
            c.plan(t0, Blinks::Yes);
            let hidden = c.plan(at(700), Blinks::Yes);
            assert_eq!(hidden, HIDDEN);
            c.commit(hidden);
            match restart {
                Event::Key => c.input(at(750)),
                _ => c.present(at(750), at_col(Col::at_index(1))),
            }
            assert_eq!(
                c.due(at(751), Blinks::Yes),
                Owes::Frame,
                "the restart owes no frame"
            );
            assert_eq!(c.plan(at(751), Blinks::Yes), SHOWN);
            assert_eq!(c.plan(at(1399), Blinks::Yes), SHOWN);
            assert_eq!(c.plan(at(1400), Blinks::Yes), HIDDEN);
        }
    }

    /// A cursor that stops blinking in the off phase -- focus lost, a steady
    /// style, hidden, looked away from -- is owed the frame that shows it.
    #[test]
    fn stopping_in_the_off_phase_owes_the_frame_that_shows_it() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        let hidden = c.plan(at(700), Blinks::Yes);
        c.commit(hidden);
        assert_eq!(c.due(at(710), Blinks::No), Owes::Frame);
        assert_eq!(c.plan(at(710), Blinks::No), SHOWN);
    }

    /// A cursor that starts blinking again starts shown, whatever the old
    /// cycle says -- foot re-arms its timer with the state on.
    #[test]
    fn blinking_again_restarts_the_cycle_shown() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::No);
        // 700 ms into a cycle that began at t0 would be hidden.
        assert_eq!(c.plan(at(700), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(1349), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(1350), Blinks::Yes), HIDDEN);
    }

    /// A frame that was put off draws nothing, so the tick after it is owed
    /// the frame again.
    #[test]
    fn a_frame_put_off_is_owed_again() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.due(at(650), Blinks::Yes), Owes::Frame);
        assert_eq!(
            c.due(at(658), Blinks::Yes),
            Owes::Frame,
            "no commit, and nothing owed"
        );
        let p = c.plan(at(666), Blinks::Yes);
        c.commit(p);
        assert_eq!(c.due(at(674), Blinks::Yes), Owes::Nothing);
    }

    /// The `poll` timeout's deadline: the next flip while blinking, and none
    /// for a steady cursor.
    #[test]
    fn the_next_change_is_the_next_flip() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.next_change(at(100), Blinks::Yes), Some(at(650)));
        assert_eq!(c.next_change(at(700), Blinks::Yes), Some(at(1000)));
        assert_eq!(c.next_change(at(1000), Blinks::Yes), Some(at(1650)));
        assert_eq!(c.next_change(at(100), Blinks::No), None);
    }

    /// Under a steady gate a blinking style is drawn steady, and a steady
    /// cycle -- an on phase past the ceiling -- is the same.
    #[test]
    fn a_steady_gate_steadies_a_blinking_style() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        for (cycle, gate) in [(SET, Gate::Steady), (Cycle::Steady, Gate::Free)] {
            let mut c = Caret::new(t0, cycle, gate, StopAfter::Never, BlinkAnimation::None);
            c.plan(t0, Blinks::Yes);
            for ms in [0, 649, 650, 999, 1650] {
                assert_eq!(c.plan(at(ms), Blinks::Yes), SHOWN, "{ms}");
            }
            assert_eq!(c.next_change(at(100), Blinks::Yes), None);
        }
    }

    /// The switch steadies the drawn caret unless the config says `true`:
    /// `false` does not say otherwise, and `auto` defers to it.
    #[test]
    fn the_gate_is_the_switch_unless_the_config_says_true() {
        assert_eq!(Gate::of(BlinkChoice::Auto, NonBlinking::Off), Gate::Free);
        assert_eq!(Gate::of(BlinkChoice::On, NonBlinking::Off), Gate::Free);
        assert_eq!(Gate::of(BlinkChoice::Off, NonBlinking::Off), Gate::Free);
        assert_eq!(Gate::of(BlinkChoice::Auto, NonBlinking::On), Gate::Steady);
        assert_eq!(Gate::of(BlinkChoice::On, NonBlinking::On), Gate::Free);
        assert_eq!(Gate::of(BlinkChoice::Off, NonBlinking::On), Gate::Steady);
    }

    /// A steady switch costs an idle screen nothing: no frame over ten
    /// seconds, through the tick's own `due`.
    #[test]
    fn a_steady_switch_costs_no_frames_over_ten_seconds() {
        let t0 = Instant::now();
        let mut c = Caret::new(
            t0,
            SET,
            Gate::Steady,
            StopAfter::Never,
            BlinkAnimation::None,
        );
        assert!(
            ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
                Event::None
            })
            .is_empty()
        );
    }

    fn stopping(t0: Instant) -> Caret<Instant> {
        Caret::new(
            t0,
            SET,
            Gate::Free,
            StopAfter::After(MS * 5000),
            BlinkAnimation::None,
        )
    }

    /// After the quiet time the blink stops shown, owing the one frame that
    /// shows it if the last drawn was hidden, and nothing after.
    #[test]
    fn a_blink_stops_shown_after_the_quiet_time_and_owes_one_frame() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let drawn = ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
            Event::None
        });
        // The last frame before the stop, at 5.0 s (tick 600), hid the
        // cursor: the hide at tick 559 (4.65 s after the cycle's restart at
        // tick 1).
        let (before, after): (Vec<_>, Vec<_>) = drawn.into_iter().partition(|d| d.0 < 600);
        assert_eq!(before.last(), Some(&(559, HIDDEN)));
        assert_eq!(after, [(600, SHOWN)], "one frame, at the stop");
        assert_eq!(c.next_change(t0 + MS * 6000, Blinks::Yes), None);
    }

    /// `tail -f`: output every 2 s and no key. Output does not hold off the
    /// stop, so the blink stops at 5 s. The hides and shows before it are
    /// ticks 79, 121, 199, then the restart at 240, 318, 360, 438, the
    /// restart at 480, and 558; then the frame that shows it at 600, and
    /// nothing in the rest of ten seconds.
    #[test]
    fn output_does_not_hold_off_the_stop() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let every_two = |tick: u64| {
            if tick.is_multiple_of(240) {
                Event::Output
            } else {
                Event::None
            }
        };
        let drawn = ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, every_two);
        let ticks_drawn: Vec<u64> = drawn.iter().map(|d| d.0).collect();
        assert_eq!(
            ticks_drawn,
            [79, 121, 199, 240, 318, 360, 438, 480, 558, 600],
            "{drawn:?}"
        );
        assert_eq!(drawn.last().map(|d| d.1), Some(SHOWN));
    }

    /// A key after the stop starts the blink again, shown, and a whole
    /// quiet time is left before it stops again.
    #[test]
    fn a_key_after_the_stop_blinks_again_from_shown() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(5700), Blinks::Yes), SHOWN, "stopped");
        c.input(at(6000));
        // The frame the key draws: the first plan after the blink resumes
        // is where its cycle restarts.
        assert_eq!(c.plan(at(6000), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(6649), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(6650), Blinks::Yes), HIDDEN, "blinking");
        assert_eq!(c.plan(at(10_999), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(11_000), Blinks::Yes), SHOWN, "stopped again");
        assert_eq!(c.plan(at(11_700), Blinks::Yes), SHOWN);
    }

    /// A click before the stop moves the deadline and leaves the phase: the
    /// cursor goes on blinking on the cycle it was on.
    #[test]
    fn a_click_before_the_stop_moves_the_deadline_and_not_the_phase() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        c.quiet(at(3000));
        assert_eq!(c.plan(at(3649), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(3650), Blinks::Yes), HIDDEN, "the old phase");
        assert_eq!(c.plan(at(7650), Blinks::Yes), HIDDEN, "past 5 s");
        assert_eq!(c.plan(at(8000), Blinks::Yes), SHOWN, "stopped at 8 s");
        assert_eq!(c.plan(at(8650), Blinks::Yes), SHOWN);
    }

    /// Clicks every four seconds hold off a five-second stop and leave the
    /// phase alone: the ten seconds draw exactly the frames a blink that
    /// never stops draws.
    #[test]
    fn clicks_hold_off_the_stop_without_moving_the_phase() {
        let t0 = Instant::now();
        let every_four = |tick: u64| {
            if tick.is_multiple_of(480) {
                Event::Click
            } else {
                Event::None
            }
        };
        let clicked = ticks(
            &mut stopping(t0),
            t0,
            Duration::from_secs(10),
            Blinks::Yes,
            every_four,
        );
        let never = ticks(
            &mut caret(t0),
            t0,
            Duration::from_secs(10),
            Blinks::Yes,
            |_| Event::None,
        );
        assert_eq!(clicked, never);
        assert_eq!(clicked.len(), 19);
    }

    /// **A tick under a held synchronised update owes nothing and restarts
    /// nothing**: a multiplexer's frame read in two parts leaves the cursor
    /// hidden at a tick between them, and reading it would stop the blink
    /// there and restart the cycle when the update closes on the cursor it
    /// opened on. Held, the cursor 700 ms in is hidden, on the cycle it was
    /// on; the control, the same tick with no update open, restarts it.
    /// Perturbation: `tick` asking `due` under a held update hides nothing.
    #[test]
    fn a_held_update_owes_the_tick_nothing_and_restarts_nothing() {
        for (update, want) in [
            (Update::Honoured, HIDDEN),
            (Update::Closed, SHOWN),
            (Update::Outlived, SHOWN),
        ] {
            let t0 = Instant::now();
            let mut c = caret(t0);
            let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
            let p = c.plan(t0, Blinks::Yes);
            c.commit(p);
            let owes = c.tick(at(300), Blinks::No, update);
            if update == Update::Honoured {
                assert_eq!(owes, Owes::Nothing, "held");
            }
            assert_eq!(c.plan(at(700), Blinks::Yes), want, "{update:?}");
        }
    }

    /// **Every driver tells the caret the cursor a frame presents, just
    /// before it plans the frame's blink, and asks the tick through the held
    /// update**: the macOS and Linux drivers, which no headless test runs,
    /// and the film, which the blink's film tests run. Each plan is the line
    /// after its present, each tick is `tick` and never `due`, and no read
    /// restarts the cycle. The control: every pinned line is found once.
    /// Perturbations: drop either driver's `present`, or give either tick
    /// back `due`, and it fails.
    #[test]
    fn every_driver_presents_before_it_plans_and_ticks_through_the_hold() {
        let plans = [
            (
                "app.rs",
                include_str!("app.rs"),
                "self.caret.plan(seen_at, blinks);",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "self.caret.plan(seen_at, blinks);",
            ),
            (
                "film.rs",
                include_str!("film.rs"),
                "caret.plan(now, blinks);",
            ),
        ];
        for (name, src, plan) in plans {
            let lines: Vec<&str> = src.lines().map(str::trim).collect();
            let at: Vec<usize> = (0..lines.len())
                .filter(|&i| lines[i].ends_with(plan))
                .collect();
            assert_eq!(at.len(), 1, "{name}: one plan of the frame's blink");
            assert!(
                at[0] > 0 && lines[at[0] - 1].contains("PresentedCursor::of("),
                "{name}: the cursor presented just before the plan"
            );
            assert!(
                !src.contains(concat!("caret.", "restart(")),
                "{name}: a read restarts the cycle"
            );
        }
        for (name, src) in [
            ("app.rs", include_str!("app.rs")),
            ("linux.rs", include_str!("linux.rs")),
        ] {
            assert_eq!(
                src.matches(concat!("caret.", "tick(")).count(),
                1,
                "{name}: the tick"
            );
            assert!(
                !src.contains(concat!("caret.", "due(")),
                "{name}: a tick asks `due`"
            );
        }
    }

    /// After the stop a click is input enough to start it again, shown:
    /// GTK's rule.
    #[test]
    fn a_click_after_the_stop_restarts_the_cycle_shown() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(5700), Blinks::Yes), SHOWN);
        c.quiet(at(6000));
        assert_eq!(c.plan(at(6100), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(6749), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(6750), Blinks::Yes), HIDDEN);
    }

    /// The deadline is the stop when it comes before the next flip. The stop
    /// is 4.4 s, inside a shown phase, so the two answers differ: at 4.2 s
    /// the next flip is the hide at 4.65 s and the stop comes first; at
    /// 3.9 s, hidden, the next flip is the show at 4.0 s, before it.
    #[test]
    fn the_next_change_is_the_stop_when_it_comes_first() {
        let t0 = Instant::now();
        let mut c = Caret::new(
            t0,
            SET,
            Gate::Free,
            StopAfter::After(MS * 4400),
            BlinkAnimation::None,
        );
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.next_change(at(4200), Blinks::Yes), Some(at(4400)));
        assert_eq!(c.next_change(at(3900), Blinks::Yes), Some(at(4000)));
    }

    /// `Never` is never: a whole minute of ticks keeps flipping.
    #[test]
    fn never_is_never() {
        let t0 = Instant::now();
        let drawn = ticks(
            &mut caret(t0),
            t0,
            Duration::from_secs(60),
            Blinks::Yes,
            |_| Event::None,
        );
        assert_eq!(drawn.len(), 119);
        assert!(drawn.last().unwrap().0 > 7100);
    }

    /// A new cycle read at a focus gain takes effect from the first plan
    /// after it, which restarts the cycle shown.
    #[test]
    fn a_new_cycle_takes_effect_at_the_first_plan_after_the_gain() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        c.plan(at(300), Blinks::No);
        c.set_system(cycle(MS * 1000, MS * 200), Gate::Free);
        c.input(at(400));
        c.plan(at(410), Blinks::Yes);
        assert_eq!(c.plan(at(1409), Blinks::Yes), SHOWN);
        assert_eq!(c.plan(at(1410), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(1609), Blinks::Yes), HIDDEN);
        assert_eq!(c.plan(at(1610), Blinks::Yes), SHOWN);
    }

    /// A focus round trip under 120 Hz ticks: blinking at 650/350, steady
    /// and drawn shown while unfocused, then blinking at the new rate from
    /// shown when focus returns.
    #[test]
    fn a_focus_round_trip_with_a_new_cycle() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let focused = |tick: u64| !(300..600).contains(&tick);
        let mut drawn = Vec::new();
        for tick in 1..=900u64 {
            let now = t0 + Duration::from_nanos(tick * 1_000_000_000 / 120);
            if tick == 600 {
                c.set_system(cycle(MS * 400, MS * 400), Gate::Free);
                c.input(now);
            }
            let b = if focused(tick) {
                Blinks::Yes
            } else {
                Blinks::No
            };
            if c.due(now, b) == Owes::Frame {
                let p = c.plan(now, b);
                c.commit(p);
                drawn.push((tick, p));
            }
        }
        assert_eq!(
            drawn,
            [
                (79, HIDDEN),
                (121, SHOWN),
                (199, HIDDEN),
                (241, SHOWN),
                // Lost at 300: already shown, so nothing owed until 600.
                (648, HIDDEN),
                (696, SHOWN),
                (744, HIDDEN),
                (792, SHOWN),
                (840, HIDDEN),
                (888, SHOWN),
            ]
        );
    }

    const WS: WinSize = WinSize {
        ws_row: 4,
        ws_col: 20,
    };

    /// A pin, not a red-first test: the switch is the drawing's, and the
    /// terminal answers the application's style whatever it says.
    #[test]
    fn the_switch_steadies_the_drawing_and_not_the_answer() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_initial_cursor_style(CursorStyle::of(
            baud_core::CursorShape::Block,
            BlinkChoice::Auto.power_on(),
        ));
        let q = b"\x1bP$q q\x1b\\";
        t.write_at(BytePos::ZERO, q);
        t.feed(BytePos::ZERO, ReadLen::from_index(q.len()));
        assert_eq!(t.reply(), b"\x1bP1$r1 q\x1b\\");
        let t0 = Instant::now();
        let mut c = Caret::new(
            t0,
            SET,
            Gate::Steady,
            StopAfter::Never,
            BlinkAnimation::None,
        );
        let b = blinks(t.cursor_style(), Focus::Gained, Some(()));
        assert_eq!(b, Blinks::Yes, "the style still blinks");
        c.plan(t0, b);
        assert_eq!(c.plan(t0 + MS * 700, b), SHOWN);
    }

    /// Blinking needs all three: a style that blinks, focus, and a cursor
    /// on the screen being drawn.
    #[test]
    fn a_cursor_blinks_only_focused_visible_and_asked_to() {
        let on = Some(());
        let none: Option<()> = None;
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Gained, on),
            Blinks::Yes
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBar, Focus::Gained, on),
            Blinks::Yes
        );
        assert_eq!(
            blinks(CursorStyle::SteadyBlock, Focus::Gained, on),
            Blinks::No
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Lost, on),
            Blinks::No
        );
        assert_eq!(
            blinks(CursorStyle::BlinkingBlock, Focus::Gained, none),
            Blinks::No
        );
    }
}
