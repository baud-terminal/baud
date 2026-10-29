//! **The launch splash, as both drivers time it** (note AJ): nothing of
//! it until the window is shown -- on macOS the first display-link tick at
//! which a frame has reached the layer and AppKit reports the window
//! visible, on Linux the compositor's first frame callback -- then baud's
//! colour lockup coming out of the plasma, left to right; at [`turn`] into
//! the emerge, once what is left of its plasma is under a flash's change,
//! gone back into the plasma, left to right -- on the clock alone, so a
//! shell's output neither cuts it short nor holds it longer. Under reduced
//! motion there is no plasma: the logo is whole from the first frame, held
//! until the shell draws something or [`CAP`] after the window is shown,
//! whichever is first, then gone in a fade, one of the HIG's Reduce Motion
//! practices and no motion animation by WCAG's definition. The renderer
//! draws it (`baud_render::splash`); this says where it is on a clock, the
//! display link's timestamp on macOS and seconds since start on Linux, so
//! it is a pure function of the clock, the screen and the window's being
//! shown, and the tests run on every platform.
//!
//! **It waits on nothing.** The shell is spawned before the first frame,
//! keys go to it throughout, its output is drawn under the logo on the
//! frame it arrives, and the splash only owes frames while it emerges or
//! vanishes, and the one the window's being shown starts.
//!
//! **"The shell drew something" is a visible cell**, not a byte: a shell's
//! first bytes are often an `OSC 7` directory, a title or mode sets, which
//! draw nothing, and the splash would be gone before anything was drawn.

use crate::config::{LaunchSplash, SystemMotion};
use baud_core::Term;
use baud_domain::{Content, Row};
use baud_render::{SplashLook, SplashPhase};

/// **Seconds from the window's being shown after which the logo vanishes
/// though the shell has drawn nothing**, under reduced motion: a slow
/// login shell, a remote one, a command that prints nothing. Short, so a
/// launch feels quick.
pub(crate) const CAP: f64 = 0.5;

/// What the splash owes the frame loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    Frame,
}

/// Where the splash is.
#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    /// The window is not shown yet: nothing of the logo under full motion,
    /// the logo whole under reduced motion, and no clock.
    Waiting,
    /// Coming out of the plasma; the clock it runs from, once a frame has
    /// had one.
    Emerging(Option<f64>),
    /// Shown whole under reduced motion, which has no emerge, until the
    /// shell's first visible cell or [`CAP`]; the clock it runs from.
    Held(Option<f64>),
    /// The hold is over: this frame shows the logo still held, and the
    /// fade starts on the next -- so a frame that brings the shell's text
    /// costs what a held frame does and no more.
    Due,
    /// Vanishing since this time, in this form.
    Vanishing(f64, Form),
    /// Gone, or never shown.
    Gone,
}

/// How the logo leaves.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Form {
    Plasma,
    Fade,
}

/// **The splash's timing.**
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Splash {
    state: State,
}

/// Seconds into the emerge at which the vanish starts
/// (`baud_render::splash::TURN`).
pub(crate) fn turn() -> f64 {
    f64::from(baud_render::splash::TURN)
}

impl Splash {
    /// The splash a launch starts with: waiting for the window, or gone
    /// when the setting is off.
    pub(crate) fn new(setting: LaunchSplash) -> Splash {
        Splash {
            state: match setting {
                LaunchSplash::On => State::Waiting,
                LaunchSplash::Off => State::Gone,
            },
        }
    }

    /// **The window is shown**: the emerge, or the reduced-motion hold,
    /// runs from the next frame with a clock, which this owes. Once.
    pub(crate) fn shown(&mut self) -> Owes {
        match self.state {
            State::Waiting => {
                self.state = State::Emerging(None);
                Owes::Frame
            }
            _ => Owes::Nothing,
        }
    }

    /// Whether it waits for the window to be shown: what the macOS tick
    /// asks AppKit about.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn waiting(&self) -> bool {
        self.state == State::Waiting
    }

    /// **The setting read again**: `"off"` takes it down at once, which
    /// owes the one frame that clears it; `"on"` after launch shows
    /// nothing, the splash being the launch's.
    pub(crate) fn set(&mut self, setting: LaunchSplash) -> Owes {
        match (setting, self.state) {
            (
                LaunchSplash::Off,
                State::Waiting
                | State::Emerging(_)
                | State::Held(_)
                | State::Due
                | State::Vanishing(..),
            ) => {
                self.state = State::Gone;
                Owes::Frame
            }
            _ => Owes::Nothing,
        }
    }

    /// **The look for a frame at `now`**, which advances the state: nothing
    /// while it waits for the window (the logo whole under reduced
    /// motion); then the emerge from the first frame with a clock, and the
    /// vanish from [`turn`] into it; under reduced motion the hold and the
    /// fade from the frame after it ends; a vanish that is over is gone.
    /// `None` for `now` -- a frame before the clock has a reading, the
    /// first one on macOS -- starts no clock. Reduced motion arriving
    /// mid-emerge shows the logo whole at once, and mid-plasma cuts the
    /// vanish, as it cuts a trail in flight.
    pub(crate) fn look(
        &mut self,
        now: Option<f64>,
        term: &Term<'_>,
        motion: SystemMotion,
    ) -> Option<SplashLook> {
        if motion == SystemMotion::Reduced {
            match self.state {
                State::Emerging(since) => self.state = State::Held(since),
                State::Vanishing(_, Form::Plasma) => self.state = State::Gone,
                _ => {}
            }
        }
        let held = Some(SplashLook {
            phase: SplashPhase::Hold,
        });
        if self.state == State::Waiting {
            return match motion {
                SystemMotion::Full => None,
                SystemMotion::Reduced => held,
            };
        }
        let Some(now) = now else {
            return match self.state {
                State::Emerging(_) => Some(SplashLook {
                    phase: SplashPhase::Emerge(0.0),
                }),
                State::Held(_) | State::Due => held,
                _ => None,
            };
        };
        match self.state {
            State::Emerging(since) => {
                let since = since.unwrap_or(now);
                self.state = match now - since >= turn() {
                    true => State::Vanishing(now, Form::Plasma),
                    false => State::Emerging(Some(since)),
                };
            }
            State::Due => {
                self.state = State::Vanishing(
                    now,
                    match motion {
                        SystemMotion::Full => Form::Plasma,
                        SystemMotion::Reduced => Form::Fade,
                    },
                );
            }
            State::Held(since) => {
                let since = since.unwrap_or(now);
                self.state = match drawn(term) || now - since >= CAP {
                    true => State::Due,
                    false => State::Held(Some(since)),
                };
            }
            State::Waiting | State::Vanishing(..) | State::Gone => {}
        }
        let phase = match self.state {
            State::Emerging(since) => {
                SplashPhase::Emerge((now - since.unwrap_or(now)).max(0.0) as f32)
            }
            State::Held(_) | State::Due => SplashPhase::Hold,
            State::Waiting | State::Gone => return None,
            State::Vanishing(from, form) => {
                let age = (now - from).max(0.0) as f32;
                let (phase, over) = match form {
                    Form::Plasma => (SplashPhase::Vanish(age), age >= baud_render::splash::end()),
                    Form::Fade => {
                        let p = age / baud_render::splash::FADE_S;
                        (SplashPhase::Fade(p), p >= 1.0)
                    }
                };
                if over {
                    // Over: this frame draws none, which repaints the
                    // screen once to clear it, and nothing more is owed.
                    self.state = State::Gone;
                    return None;
                }
                phase
            }
        };
        Some(SplashLook { phase })
    }

    /// **What a tick at `now` owes**: nothing while it waits for the
    /// window, a frame every tick while it emerges or vanishes, and under
    /// reduced motion the one its cap runs out on -- the first frame after
    /// the screen gets a visible cell is the output's own.
    pub(crate) fn owes(&self, now: f64) -> Owes {
        match self.state {
            State::Emerging(_) | State::Vanishing(..) | State::Due | State::Held(None) => {
                Owes::Frame
            }
            State::Held(Some(since)) if now - since >= CAP => Owes::Frame,
            _ => Owes::Nothing,
        }
    }

    /// **When the reduced-motion hold ends** on the clock alone: what
    /// bounds the Linux loop's wait. The window's being shown is an event
    /// of its own, so a wait for it has no deadline.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn deadline(&self) -> Option<f64> {
        match self.state {
            State::Held(Some(since)) => Some(since + CAP),
            _ => None,
        }
    }

    /// Whether it stands still before its plasma -- waiting for the window,
    /// or held under reduced motion: the time to build what the plasma
    /// draws with, so neither the emerge's first frame nor a vanish after
    /// full motion returns pays for it. The macOS GPU path's: the CPU path
    /// builds nothing.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn held(&self) -> bool {
        matches!(self.state, State::Waiting | State::Held(_) | State::Due)
    }
}

/// **`BAUD_SPLASH`**: `on` and `off` fix the splash whatever the config
/// says, for checks -- a launch measured, a frame captured -- in the idiom
/// of `BAUD_MOTION`; anything else leaves the config's.
pub(crate) fn splash_override(env: Option<&str>, config: LaunchSplash) -> LaunchSplash {
    match env {
        Some("on") => LaunchSplash::On,
        Some("off") => LaunchSplash::Off,
        _ => config,
    }
}

/// Whether the screen shows anything: a cell holding a character other
/// than a space.
fn drawn(term: &Term<'_>) -> bool {
    let view = term.view(term.window_back());
    (0..view.rows().index()).any(|r| {
        view.row(Row::from_index(r)).iter().any(|c| {
            let k: Content = c.content;
            !k.is_empty() && k.as_codepoint().is_none_or(|p| p.value() != 0x20)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 10,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    fn phase(l: Option<SplashLook>) -> Option<SplashPhase> {
        l.map(|l| l.phase)
    }

    /// **The emerge waits for the window**: before it is shown no frame
    /// draws anything of the logo, with a clock or without, and nothing is
    /// owed, nor a deadline set, however long it takes; the plasma is built
    /// meanwhile. Being shown owes one frame, once, and the emerge's clock
    /// starts on the first frame after it.
    #[test]
    fn the_emerge_waits_for_the_window_to_be_shown() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let full = SystemMotion::Full;
        assert!(s.waiting());
        assert_eq!(s.look(None, &t, full), None, "nothing before the window");
        feed(&mut t, b"$ ");
        for now in [0.1, 1.0, 5.0] {
            assert_eq!(s.look(Some(now), &t, full), None, "{now}");
            assert_eq!(s.owes(now), Owes::Nothing, "{now}");
            assert_eq!(s.deadline(), None, "{now}");
        }
        assert!(s.held(), "the plasma is built while it waits");
        assert_eq!(s.shown(), Owes::Frame);
        assert!(!s.waiting());
        assert_eq!(s.shown(), Owes::Nothing, "once");
        assert_eq!(
            phase(s.look(Some(7.0), &t, full)),
            Some(SplashPhase::Emerge(0.0)),
            "the clock starts after the window is shown"
        );
        let Some(SplashPhase::Emerge(a)) = phase(s.look(Some(7.1), &t, full)) else {
            panic!("emerging");
        };
        assert!((a - 0.1).abs() < 1e-4, "{a}");
    }

    /// **It emerges, then vanishes at the turn**, on the display link's
    /// clock alone: the emerge runs from the first frame with a clock after
    /// the window is shown, owing every frame; the frame [`turn`] into it
    /// is the vanish's first, with no hold between -- output drawn
    /// meanwhile neither ends it nor holds it longer -- and the vanish is
    /// gone at the plasma's end, owing nothing after.
    #[test]
    fn it_emerges_then_vanishes_at_the_turn() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let full = SystemMotion::Full;
        let _ = s.shown();
        assert_eq!(
            phase(s.look(None, &t, full)),
            Some(SplashPhase::Emerge(0.0)),
            "a frame before the clock"
        );
        assert_eq!(
            phase(s.look(Some(5.0), &t, full)),
            Some(SplashPhase::Emerge(0.0))
        );
        feed(&mut t, b"% ");
        assert_eq!(s.owes(5.1), Owes::Frame, "every frame while it emerges");
        assert_eq!(s.deadline(), None);
        let Some(SplashPhase::Emerge(a)) = phase(s.look(Some(5.0 + turn() - 0.01), &t, full))
        else {
            panic!("output does not end the emerge");
        };
        assert!((f64::from(a) - (turn() - 0.01)).abs() < 1e-4, "{a}");
        assert_eq!(
            phase(s.look(Some(5.0 + turn()), &t, full)),
            Some(SplashPhase::Vanish(0.0)),
            "the vanish starts at the turn"
        );
        assert_eq!(s.owes(5.0 + turn()), Owes::Frame);
        let end = 5.0 + turn() + f64::from(baud_render::splash::end()) + 0.001;
        assert_eq!(s.look(Some(end), &t, full), None);
        assert_eq!(s.owes(end + 0.1), Owes::Nothing);
    }

    /// **The turn is the new length**: under half a second into the emerge,
    /// before the emerge's plasma is out; the logo no longer holds 0.4 s
    /// after it.
    #[test]
    fn the_vanish_starts_before_the_emerges_plasma_is_out() {
        let emerge = f64::from(baud_render::splash::end());
        assert!(turn() < emerge, "{} {emerge}", turn());
        assert!((turn() - 0.48).abs() < 1e-6, "{}", turn());
    }

    /// **Under reduced motion it holds until the shell draws a visible
    /// cell, then fades, and is gone at the fade's end**: whole from the
    /// first frame, before the window is shown, and held there whatever
    /// the shell drew; after it, an `OSC 7`, a title and spaces draw
    /// nothing and keep it held; the frame that shows the prompt holds it
    /// over the prompt, and the next starts the fade; the frame after its
    /// end draws none and owes nothing.
    #[test]
    fn under_reduced_motion_it_holds_until_the_shell_draws_then_fades() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let reduced = SystemMotion::Reduced;
        assert_eq!(phase(s.look(None, &t, reduced)), Some(SplashPhase::Hold));
        assert_eq!(
            phase(s.look(Some(9.0), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(9.0 + CAP), Owes::Nothing, "no cap before shown");
        assert_eq!(s.shown(), Owes::Frame);
        assert_eq!(
            phase(s.look(Some(10.0), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        feed(
            &mut t,
            b"\x1b]7;file:///tmp\x07\x1b]2;zsh\x07\x1b[?2004h   ",
        );
        assert_eq!(
            phase(s.look(Some(10.1), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(10.1), Owes::Nothing);
        assert_eq!(s.deadline(), Some(10.0 + CAP));
        assert!(s.held());
        feed(&mut t, b"% ");
        // The frame that shows the prompt holds the logo over it.
        assert_eq!(
            phase(s.look(Some(10.2), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(10.2), Owes::Frame);
        assert_eq!(s.deadline(), None);
        assert_eq!(
            phase(s.look(Some(10.25), &t, reduced)),
            Some(SplashPhase::Fade(0.0))
        );
        assert!(!s.held());
        let fade = f64::from(baud_render::splash::FADE_S);
        assert_eq!(s.look(Some(10.25 + fade + 0.001), &t, reduced), None);
        assert_eq!(s.owes(11.5), Owes::Nothing);
        assert_eq!(s.look(Some(11.6), &t, reduced), None);
    }

    /// **A shell that never draws does not hold it past the turn**, and
    /// one that draws at once does not cut it short: the vanish starts
    /// on the same frame either way, the turn after the window is shown.
    #[test]
    fn output_neither_ends_nor_holds_the_full_motion_splash() {
        let full = SystemMotion::Full;
        let mut starts = Vec::new();
        for prompt in [&b""[..], b"$ "] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, prompt);
            let mut s = Splash::new(LaunchSplash::On);
            let mut at = None;
            for f in 0..240 {
                let now = f64::from(f) / 120.0;
                if f == 30 {
                    let _ = s.shown();
                }
                if let Some(SplashPhase::Vanish(0.0)) = phase(s.look(Some(now), &t, full)) {
                    at = Some(now);
                    break;
                }
            }
            starts.push(at.expect("it vanishes"));
        }
        assert_eq!(starts[0], starts[1]);
        let shown = 30.0 / 120.0;
        assert!(
            (starts[0] - (shown + turn())).abs() <= 2.0 / 120.0,
            "{starts:?}"
        );
    }

    /// **Under reduced motion there is no emerge**: the logo whole from
    /// the first frame, as before; and reduced motion arriving mid-emerge
    /// shows it whole at once, its plasma cut.
    #[test]
    fn reduced_motion_shows_it_whole_and_cuts_an_emerge() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let reduced = SystemMotion::Reduced;
        let mut s = Splash::new(LaunchSplash::On);
        assert_eq!(phase(s.look(None, &t, reduced)), Some(SplashPhase::Hold));
        let _ = s.shown();
        assert_eq!(
            phase(s.look(Some(1.0), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        let mut s = Splash::new(LaunchSplash::On);
        let _ = s.shown();
        assert_eq!(
            phase(s.look(Some(1.0), &t, SystemMotion::Full)),
            Some(SplashPhase::Emerge(0.0))
        );
        assert_eq!(
            phase(s.look(Some(1.1), &t, reduced)),
            Some(SplashPhase::Hold),
            "whole at once"
        );
    }

    /// **The cap ends a hold the shell never ends**, under reduced
    /// motion, from the first frame with a clock after the window is
    /// shown, and owes the frame it runs out on.
    #[test]
    fn the_cap_ends_a_hold_the_shell_never_ends() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let reduced = SystemMotion::Reduced;
        assert_eq!(s.owes(0.0), Owes::Nothing, "waiting for the window");
        let _ = s.shown();
        assert_eq!(s.owes(0.0), Owes::Frame, "the first tick starts the clock");
        assert_eq!(
            phase(s.look(Some(5.0), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(5.0 + CAP - 0.01), Owes::Nothing);
        assert_eq!(s.owes(5.0 + CAP), Owes::Frame);
        assert_eq!(
            phase(s.look(Some(5.0 + CAP - 0.01), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(
            phase(s.look(Some(5.0 + CAP), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(5.0 + CAP), Owes::Frame);
        assert_eq!(
            phase(s.look(Some(5.0 + CAP + 0.016), &t, reduced)),
            Some(SplashPhase::Fade(0.0))
        );
    }

    /// **Under reduced motion it fades, and reduced motion arriving cuts
    /// the plasma**: the same trigger starts a fade over `FADE_S`, gone
    /// after it; a plasma in flight is gone on the first frame that reads
    /// reduced.
    #[test]
    fn reduced_motion_fades_it_and_cuts_a_plasma() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"$ ");
        let reduced = SystemMotion::Reduced;
        let mut s = Splash::new(LaunchSplash::On);
        let _ = s.shown();
        assert_eq!(
            phase(s.look(Some(0.9), &t, reduced)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(
            phase(s.look(Some(1.0), &t, reduced)),
            Some(SplashPhase::Fade(0.0))
        );
        let fade = f64::from(baud_render::splash::FADE_S);
        let Some(SplashPhase::Fade(p)) = phase(s.look(Some(1.0 + fade / 2.0), &t, reduced)) else {
            panic!("fading");
        };
        assert!((p - 0.5).abs() < 1e-4);
        assert_eq!(s.look(Some(1.0 + fade), &t, reduced), None);
        let mut s = Splash::new(LaunchSplash::On);
        let _ = s.shown();
        let full = SystemMotion::Full;
        let due = 1.0 + turn();
        let _ = s.look(Some(1.0), &t, full);
        assert_eq!(
            phase(s.look(Some(due), &t, full)),
            Some(SplashPhase::Vanish(0.0))
        );
        assert_eq!(s.look(Some(due + 0.05), &t, reduced), None);
        assert_eq!(s.owes(due + 0.1), Owes::Nothing);
    }

    /// **Off draws nothing and owes nothing; turned off live it goes at
    /// once, waiting or emerging; turned on after launch it stays gone.**
    #[test]
    fn off_draws_nothing_and_a_reload_to_off_cuts_it() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let full = SystemMotion::Full;
        let mut off = Splash::new(LaunchSplash::Off);
        assert_eq!(off.look(None, &t, full), None);
        assert_eq!(off.look(Some(1.0), &t, full), None);
        assert_eq!(off.owes(1.0), Owes::Nothing);
        assert_eq!(off.shown(), Owes::Nothing);
        assert_eq!(off.set(LaunchSplash::On), Owes::Nothing);
        assert_eq!(off.look(Some(1.1), &t, full), None);
        let mut waiting = Splash::new(LaunchSplash::On);
        assert_eq!(waiting.set(LaunchSplash::Off), Owes::Frame);
        assert_eq!(waiting.shown(), Owes::Nothing);
        assert_eq!(waiting.look(Some(1.0), &t, SystemMotion::Reduced), None);
        let mut s = Splash::new(LaunchSplash::On);
        let _ = s.shown();
        assert_eq!(
            phase(s.look(Some(1.0), &t, full)),
            Some(SplashPhase::Emerge(0.0))
        );
        assert_eq!(s.set(LaunchSplash::On), Owes::Nothing);
        assert_eq!(s.set(LaunchSplash::Off), Owes::Frame);
        assert_eq!(s.look(Some(1.1), &t, full), None);
        assert_eq!(s.set(LaunchSplash::Off), Owes::Nothing);
    }

    /// `BAUD_SPLASH`: `on` and `off` fix it, anything else is the config's.
    #[test]
    fn the_splash_override_reads_on_and_off() {
        use LaunchSplash::{Off, On};
        assert_eq!(splash_override(Some("off"), On), Off);
        assert_eq!(splash_override(Some("on"), Off), On);
        assert_eq!(splash_override(Some("yes"), Off), Off);
        assert_eq!(splash_override(None, On), On);
    }
}
