//! The cursor's blink: when it shows, when it hides, and which frames that
//! owes.
//!
//! **The timing is the system's caret**, the owner's call of 2026-09-25: on
//! macOS the per-phase `NSTextInsertionPointBlinkPeriodOn` and `...Off`
//! defaults, each read on its own as AppKit reads it, and AppKit's own 650 ms
//! shown and 350 hidden where they are unset. AppKit ramps its caret's
//! opacity in four 37.5 ms steps; baud switches hard at the same mean opacity,
//! two frames a cycle, the owner's call of 2026-09-27. Linux keeps xterm's
//! `cursorOnTime` and `cursorOffTime`, 600 and 300 (`[[source:
//! xterm-reset]]`'s file, 577-578), until the desktop's are read.
//!
//! **It blinks where foot blinks it, and nowhere else.** foot arms its blink
//! timer only while the window has focus (`term_cursor_blink_update`, whose
//! `activate` needs `term->visual_focus`, in `[[source: foot-term-reset]]`'s
//! file), so a window in the background holds a steady cursor. And it restarts
//! the cycle shown on every read of the child's output -- "Prevent blinking
//! while typing" (the same file, 254-258) -- so a cursor that is moving is not
//! hidden half the time. baud restarts it on a key sent as well: a key the
//! child does not echo, a password, reads nothing back to restart it.
//!
//! **A cursor that is not blinking draws [`Blink::Shown`], and the frame that
//! stops it blinking is owed.** Losing focus, a steady style, `DECTCEM`, a
//! window scrolled back, the system's non-blinking switch, the stop: any of
//! them in the off phase would otherwise leave the cursor hidden until
//! something else drew a frame.
//!
//! **It stops only if asked**, `[cursor] stop_blinking_after`: after that long
//! without input -- a key, a click, a paste, a focus gain -- it stops, shown,
//! and output does not hold it off, so `tail -f` stops blinking (kitty's
//! "keyboard inactivity"). What a blinking cursor costs an idle screen is two
//! frames a cycle, counted by a test below.
//!
//! The clock is the driver's -- the display link's timestamp on macOS, the
//! `poll` timeout on Linux ([`Caret::next_change`]) -- so nothing sleeps and
//! no frame is held.

use core::time::Duration;

use baud_core::{CursorStyle, Focus};
use baud_render::Blink;

use crate::bell::Moment;
use crate::config::{BlinkChoice, StopAfter};

/// The shortest phase: below a sixtieth of a second a phase cannot be shown,
/// and every tick would owe a frame.
#[cfg(any(target_os = "macos", test))]
pub const FLOOR: Duration = Duration::from_nanos(16_666_667);
/// The longest: Chromium's `kMaximumReasonableIntervalMs`. An on phase past it
/// is a steady cursor, as AppKit's is; an off phase past it is clamped, where
/// AppKit hides the caret for good.
#[cfg(any(target_os = "macos", test))]
pub const CEILING: Duration = Duration::from_secs(60);

/// One phase of the blink, [`FLOOR`] to [`CEILING`] by construction, so a
/// cycle is never zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Phase(Duration);

impl Phase {
    pub fn get(self) -> Duration {
        self.0
    }

    /// A phase `ms` long, from a value already known positive, finite and
    /// at most [`CEILING`]: clamped up to the floor.
    #[cfg(any(target_os = "macos", test))]
    fn of_ms(ms: f64) -> Phase {
        Duration::try_from_secs_f64(ms / 1000.0).map_or(Phase(FLOOR), |d| Phase(d.max(FLOOR)))
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
    /// AppKit's caret where the defaults are unset: 650 ms shown, 350 hidden.
    #[cfg(any(target_os = "macos", test))]
    pub const APPKIT: Cycle = Cycle::Blinks {
        on: Phase(Duration::from_millis(650)),
        off: Phase(Duration::from_millis(350)),
    };
    /// xterm's `cursorOnTime` and `cursorOffTime`, which Linux keeps.
    #[cfg(any(target_os = "linux", test))]
    pub const XTERM: Cycle = Cycle::Blinks {
        on: Phase(Duration::from_millis(600)),
        off: Phase(Duration::from_millis(300)),
    };

    /// The cycle two per-phase defaults describe, each in milliseconds and
    /// read on its own, as AppKit reads them; `unset` gives a phase that is
    /// not set. Total: NaN, zero or negative is unset; an on phase past the
    /// ceiling is steady; an off phase past it is the ceiling; below the
    /// floor is the floor.
    #[cfg(any(target_os = "macos", test))]
    pub fn of_defaults(on_ms: f64, off_ms: f64, unset: Cycle) -> Cycle {
        let (on0, off0) = match unset.or(Cycle::APPKIT) {
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

impl core::fmt::Display for Cycle {
    /// `650/350 ms`, or `steady`, for the read's log line.
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

/// Whether the drawing is held steady whatever the style asks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    Free,
    Steady,
}

impl Gate {
    /// The switch steadies the drawn caret unless the config says `true`:
    /// the owner's "honoured unless the config says otherwise", and `false`
    /// does not say otherwise.
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

/// Whether the blink owes a frame: the phase a frame drawn now would show is
/// not the one last drawn.
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
    /// The phase the last drawn frame showed.
    drawn: Blink,
    /// The system's rate, the switch as the config lets it, and the stop.
    cycle: Cycle,
    gate: Gate,
    stop: StopAfter,
    /// The last input; output never moves it.
    quiet_since: M,
}

impl<M: Moment> Caret<M> {
    pub fn new(now: M, cycle: Cycle, gate: Gate, stop: StopAfter) -> Caret<M> {
        Caret {
            epoch: now,
            was: Blinks::No,
            drawn: Blink::Shown,
            cycle,
            gate,
            stop,
            quiet_since: now,
        }
    }

    /// Output: the cursor shows, and the cycle restarts from `now`. It does
    /// not hold off the stop.
    pub fn restart(&mut self, now: M) {
        self.epoch = now;
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
    #[cfg(any(target_os = "macos", test))]
    pub fn quiet(&mut self, now: M) {
        self.quiet_since = now;
    }

    /// The system's rate and switch, read again. No restart: a blink that
    /// resumes restarts at the next plan anyway.
    #[cfg(any(target_os = "macos", test))]
    pub fn set_system(&mut self, cycle: Cycle, gate: Gate) {
        self.cycle = cycle;
        self.gate = gate;
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

    /// The phase a frame drawn at `now` shows. A cursor that does not blink
    /// shows; one that starts blinking again restarts its cycle, shown.
    pub fn plan(&mut self, now: M, blinks: Blinks) -> Blink {
        let blinks = self.blinking(now, blinks);
        if blinks == Blinks::Yes && self.was == Blinks::No {
            self.epoch = now;
        }
        self.was = blinks;
        match (blinks, self.cycle) {
            (Blinks::Yes, Cycle::Blinks { on, off }) => {
                phase_at(now.elapsed_since(self.epoch), on.get(), off.get())
            }
            _ => Blink::Shown,
        }
    }

    /// Whether a frame drawn at `now` would show another phase than the last
    /// drawn frame did: the tick owes a frame when it would.
    pub fn due(&mut self, now: M, blinks: Blinks) -> Owes {
        if self.plan(now, blinks) == self.drawn {
            Owes::Nothing
        } else {
            Owes::Frame
        }
    }

    /// A frame showing `phase` reached the screen. One put off or refused
    /// does not call this, so the next asks again.
    pub fn commit(&mut self, phase: Blink) {
        self.drawn = phase;
    }

    /// When the phase next changes, for a `poll` timeout: the next flip, or
    /// the stop if it comes first; `None` while the cursor does not blink,
    /// so a steady cursor adds no wakeup. The macOS driver reads the display
    /// link instead and has no use for it.
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
        let flip = if into < on {
            on - into
        } else {
            on + off - into
        };
        let left = match self.stop {
            StopAfter::After(d) => flip.min(d.saturating_sub(now.elapsed_since(self.quiet_since))),
            StopAfter::Never => flip,
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

/// The phase `elapsed` into a cycle: shown for `on`, then hidden.
fn phase_at(elapsed: Duration, on: Duration, off: Duration) -> Blink {
    if cycle_offset(elapsed, on, off) < on {
        Blink::Shown
    } else {
        Blink::Hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, Term};
    use baud_domain::{BytePos, ReadLen, WinSize};
    use std::time::Instant;

    const MS: Duration = Duration::from_millis(1);

    fn caret(t0: Instant) -> Caret<Instant> {
        Caret::new(t0, Cycle::APPKIT, Gate::Free, StopAfter::Never)
    }

    fn cycle(on: Duration, off: Duration) -> Cycle {
        Cycle::Blinks {
            on: Phase(on),
            off: Phase(off),
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
    ) -> Vec<(u64, Blink)> {
        let mut drawn = Vec::new();
        for tick in 1.. {
            let at = Duration::from_nanos(tick * 1_000_000_000 / 120);
            if at > over {
                break;
            }
            let now = t0 + at;
            match event(tick) {
                Event::Output => c.restart(now),
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
        assert_eq!(drawn[0], (79, Blink::Hidden));
        assert_eq!(drawn[1], (121, Blink::Shown));
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

    /// AppKit's unset caret: shown for 650 ms, hidden for 350.
    #[test]
    fn the_cycle_is_appkits_six_fifty_on_and_three_fifty_off() {
        let t0 = Instant::now();
        let mut c = caret(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(0), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(649), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(650), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(999), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(1000), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1650), Blinks::Yes), Blink::Hidden);
    }

    /// Each default is read on its own, as AppKit reads them -- Chromium
    /// averages the two and baud does not -- and the parse is total.
    #[test]
    fn each_key_is_read_on_its_own_as_appkit_reads_it() {
        let a = Cycle::APPKIT;
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
        // Linux's unset cycle is xterm's.
        assert_eq!(
            Cycle::of_defaults(0.0, 0.0, Cycle::XTERM),
            cycle(MS * 600, MS * 300)
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
                for unset in [Cycle::APPKIT, Cycle::XTERM, Cycle::Steady] {
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
    /// Output restarts it the same way.
    #[test]
    fn input_and_output_restart_the_cycle() {
        for restart in [Event::Key, Event::Output] {
            let t0 = Instant::now();
            let mut c = caret(t0);
            let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
            c.plan(t0, Blinks::Yes);
            let hidden = c.plan(at(700), Blinks::Yes);
            assert_eq!(hidden, Blink::Hidden);
            c.commit(hidden);
            match restart {
                Event::Key => c.input(at(750)),
                _ => c.restart(at(750)),
            }
            assert_eq!(
                c.due(at(751), Blinks::Yes),
                Owes::Frame,
                "the restart owes no frame"
            );
            assert_eq!(c.plan(at(751), Blinks::Yes), Blink::Shown);
            assert_eq!(c.plan(at(1399), Blinks::Yes), Blink::Shown);
            assert_eq!(c.plan(at(1400), Blinks::Yes), Blink::Hidden);
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
        assert_eq!(c.plan(at(710), Blinks::No), Blink::Shown);
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
        assert_eq!(c.plan(at(700), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1349), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1350), Blinks::Yes), Blink::Hidden);
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
        for (cycle, gate) in [(Cycle::APPKIT, Gate::Steady), (Cycle::Steady, Gate::Free)] {
            let mut c = Caret::new(t0, cycle, gate, StopAfter::Never);
            c.plan(t0, Blinks::Yes);
            for ms in [0, 649, 650, 999, 1650] {
                assert_eq!(c.plan(at(ms), Blinks::Yes), Blink::Shown, "{ms}");
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
        let mut c = Caret::new(t0, Cycle::APPKIT, Gate::Steady, StopAfter::Never);
        assert!(
            ticks(&mut c, t0, Duration::from_secs(10), Blinks::Yes, |_| {
                Event::None
            })
            .is_empty()
        );
    }

    fn stopping(t0: Instant) -> Caret<Instant> {
        Caret::new(t0, Cycle::APPKIT, Gate::Free, StopAfter::After(MS * 5000))
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
        assert_eq!(before.last(), Some(&(559, Blink::Hidden)));
        assert_eq!(after, [(600, Blink::Shown)], "one frame, at the stop");
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
        assert_eq!(drawn.last().map(|d| d.1), Some(Blink::Shown));
    }

    /// A key after the stop starts the blink again, shown, and a whole
    /// quiet time is left before it stops again.
    #[test]
    fn a_key_after_the_stop_blinks_again_from_shown() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(5700), Blinks::Yes), Blink::Shown, "stopped");
        c.input(at(6000));
        // The frame the key draws: the first plan after the blink resumes
        // is where its cycle restarts.
        assert_eq!(c.plan(at(6000), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(6649), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(6650), Blinks::Yes), Blink::Hidden, "blinking");
        assert_eq!(c.plan(at(10_999), Blinks::Yes), Blink::Hidden);
        assert_eq!(
            c.plan(at(11_000), Blinks::Yes),
            Blink::Shown,
            "stopped again"
        );
        assert_eq!(c.plan(at(11_700), Blinks::Yes), Blink::Shown);
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
        assert_eq!(c.plan(at(3649), Blinks::Yes), Blink::Shown);
        assert_eq!(
            c.plan(at(3650), Blinks::Yes),
            Blink::Hidden,
            "the old phase"
        );
        assert_eq!(c.plan(at(7650), Blinks::Yes), Blink::Hidden, "past 5 s");
        assert_eq!(
            c.plan(at(8000), Blinks::Yes),
            Blink::Shown,
            "stopped at 8 s"
        );
        assert_eq!(c.plan(at(8650), Blinks::Yes), Blink::Shown);
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

    /// After the stop a click is input enough to start it again, shown:
    /// GTK's rule.
    #[test]
    fn a_click_after_the_stop_restarts_the_cycle_shown() {
        let t0 = Instant::now();
        let mut c = stopping(t0);
        let at = |ms: u64| t0 + MS * u32::try_from(ms).unwrap();
        c.plan(t0, Blinks::Yes);
        assert_eq!(c.plan(at(5700), Blinks::Yes), Blink::Shown);
        c.quiet(at(6000));
        assert_eq!(c.plan(at(6100), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(6749), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(6750), Blinks::Yes), Blink::Hidden);
    }

    /// The deadline is the stop when it comes before the next flip. The stop
    /// is 4.4 s, inside a shown phase, so the two answers differ: at 4.2 s
    /// the next flip is the hide at 4.65 s and the stop comes first; at
    /// 3.9 s, hidden, the next flip is the show at 4.0 s, before it.
    #[test]
    fn the_next_change_is_the_stop_when_it_comes_first() {
        let t0 = Instant::now();
        let mut c = Caret::new(t0, Cycle::APPKIT, Gate::Free, StopAfter::After(MS * 4400));
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
        assert_eq!(c.plan(at(1409), Blinks::Yes), Blink::Shown);
        assert_eq!(c.plan(at(1410), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(1609), Blinks::Yes), Blink::Hidden);
        assert_eq!(c.plan(at(1610), Blinks::Yes), Blink::Shown);
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
                (79, Blink::Hidden),
                (121, Blink::Shown),
                (199, Blink::Hidden),
                (241, Blink::Shown),
                // Lost at 300: already shown, so nothing owed until 600.
                (648, Blink::Hidden),
                (696, Blink::Shown),
                (744, Blink::Hidden),
                (792, Blink::Shown),
                (840, Blink::Hidden),
                (888, Blink::Shown),
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
        let mut c = Caret::new(t0, Cycle::APPKIT, Gate::Steady, StopAfter::Never);
        let b = blinks(t.cursor_style(), Focus::Gained, Some(()));
        assert_eq!(b, Blinks::Yes, "the style still blinks");
        c.plan(t0, b);
        assert_eq!(c.plan(t0 + MS * 700, b), Blink::Shown);
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
