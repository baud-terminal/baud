//! **The launch splash, as both drivers time it** (note AJ): baud's colour
//! lockup from the first frame, held until the shell draws something or
//! [`CAP`] passes, whichever is first, then gone in the plasma -- or,
//! under reduced motion, in a fade, one of the HIG's Reduce Motion
//! practices and no motion animation by WCAG's definition. The renderer
//! draws it (`baud_render::splash`); this says where it is on a clock, the
//! display link's timestamp on macOS and seconds since start on Linux, so
//! it is a pure function of the clock and the screen, and the tests run on
//! every platform.
//!
//! **It waits on nothing.** The shell is spawned before the first frame,
//! keys go to it throughout, and the splash only owes frames while it
//! vanishes; while it is held it owes one frame, at the cap.
//!
//! **"The shell drew something" is a visible cell**, not a byte: a shell's
//! first bytes are often an `OSC 7` directory, a title or mode sets, which
//! draw nothing, and the splash would be gone before anything was drawn.

use crate::config::{LaunchSplash, SystemMotion};
use baud_core::Term;
use baud_domain::{Content, Row};
use baud_render::{SplashLook, SplashPhase};

/// **Seconds from the first frame after which the logo vanishes though the
/// shell has drawn nothing**: a slow login shell, a remote one, a command
/// that prints nothing. Short, so a launch feels quick.
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
    /// Shown whole; the clock the cap runs from, once a frame has had one.
    Held(Option<f64>),
    /// The shell drew, or the cap passed: this frame shows the text under
    /// the logo, still held, and the vanish starts on the next -- so the
    /// first frame of the shell's text costs what a held frame does and no
    /// more, the plasma's first frame landing on the frame after it.
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

impl Splash {
    /// The splash a launch starts with: held, or gone when the setting is
    /// off.
    pub(crate) fn new(setting: LaunchSplash) -> Splash {
        Splash {
            state: match setting {
                LaunchSplash::On => State::Held(None),
                LaunchSplash::Off => State::Gone,
            },
        }
    }

    /// **The setting read again**: `"off"` takes it down at once, which
    /// owes the one frame that clears it; `"on"` after launch shows
    /// nothing, the splash being the launch's.
    pub(crate) fn set(&mut self, setting: LaunchSplash) -> Owes {
        match (setting, self.state) {
            (LaunchSplash::Off, State::Held(_) | State::Due | State::Vanishing(..)) => {
                self.state = State::Gone;
                Owes::Frame
            }
            _ => Owes::Nothing,
        }
    }

    /// **The look for a frame at `now`**, which advances the state: the
    /// vanish starts on the frame after the first whose screen has a
    /// visible cell, or after the cap, in the form the motion preference
    /// gives; a vanish that
    /// is over is gone. `None` for `now` -- a frame before the clock has a
    /// reading, the first one on macOS -- holds the logo and starts no
    /// clock. Reduced motion arriving mid-plasma cuts it, as it cuts a
    /// trail in flight.
    pub(crate) fn look(
        &mut self,
        now: Option<f64>,
        term: &Term<'_>,
        motion: SystemMotion,
    ) -> Option<SplashLook> {
        let Some(now) = now else {
            return match self.state {
                State::Held(_) | State::Due => Some(SplashLook {
                    phase: SplashPhase::Hold,
                }),
                _ => None,
            };
        };
        match self.state {
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
            State::Vanishing(..) | State::Gone => {}
        }
        if let State::Vanishing(_, Form::Plasma) = self.state {
            if motion == SystemMotion::Reduced {
                self.state = State::Gone;
            }
        }
        let phase = match self.state {
            State::Held(_) | State::Due => SplashPhase::Hold,
            State::Gone => return None,
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

    /// **What a tick at `now` owes**: a frame every tick while it vanishes,
    /// the one the cap runs out on while it is held, and the first frame
    /// after the screen gets a visible cell -- which the output's own frame
    /// draws anyway.
    pub(crate) fn owes(&self, now: f64) -> Owes {
        match self.state {
            State::Vanishing(..) | State::Due => Owes::Frame,
            State::Held(Some(since)) if now - since >= CAP => Owes::Frame,
            State::Held(None) => Owes::Frame,
            _ => Owes::Nothing,
        }
    }

    /// **When the cap runs out**, while held: what bounds the Linux loop's
    /// wait.
    #[cfg(any(target_os = "linux", test))]
    pub(crate) fn deadline(&self) -> Option<f64> {
        match self.state {
            State::Held(Some(since)) => Some(since + CAP),
            _ => None,
        }
    }

    /// Whether it is held, still to vanish: the time to build what the
    /// vanish draws with, before it starts. The macOS GPU path's: the CPU
    /// path builds nothing.
    #[cfg(any(target_os = "macos", test))]
    pub(crate) fn held(&self) -> bool {
        matches!(self.state, State::Held(_) | State::Due)
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

    /// **It holds until the shell draws a visible cell, then vanishes in
    /// the plasma, and is gone at the plasma's end**: an `OSC 7`, a title
    /// and spaces draw nothing and keep it held; the frame that shows the
    /// prompt holds it over the prompt, and the next starts the vanish;
    /// the frame after its end draws none and owes nothing.
    #[test]
    fn it_holds_until_the_shell_draws_then_vanishes() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let full = SystemMotion::Full;
        assert_eq!(phase(s.look(None, &t, full)), Some(SplashPhase::Hold));
        assert_eq!(phase(s.look(Some(10.0), &t, full)), Some(SplashPhase::Hold));
        feed(
            &mut t,
            b"\x1b]7;file:///tmp\x07\x1b]2;zsh\x07\x1b[?2004h   ",
        );
        assert_eq!(phase(s.look(Some(10.1), &t, full)), Some(SplashPhase::Hold));
        assert_eq!(s.owes(10.1), Owes::Nothing);
        assert_eq!(s.deadline(), Some(10.0 + CAP));
        assert!(s.held());
        feed(&mut t, b"% ");
        // The frame that shows the prompt holds the logo over it.
        assert_eq!(phase(s.look(Some(10.2), &t, full)), Some(SplashPhase::Hold));
        assert_eq!(s.owes(10.2), Owes::Frame);
        assert_eq!(s.deadline(), None);
        assert_eq!(
            phase(s.look(Some(10.25), &t, full)),
            Some(SplashPhase::Vanish(0.0))
        );
        assert!(!s.held());
        let Some(SplashPhase::Vanish(a)) = phase(s.look(Some(10.35), &t, full)) else {
            panic!("vanishing");
        };
        assert!((a - 0.1).abs() < 1e-4, "{a}");
        let end = f64::from(baud_render::splash::end());
        assert_eq!(s.look(Some(10.25 + end + 0.001), &t, full), None);
        assert_eq!(s.owes(11.5), Owes::Nothing);
        assert_eq!(s.look(Some(11.6), &t, full), None);
    }

    /// **The cap ends a hold the shell never ends**, from the first frame
    /// with a clock, and owes the frame it runs out on.
    #[test]
    fn the_cap_ends_a_hold_the_shell_never_ends() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let full = SystemMotion::Full;
        assert_eq!(s.owes(0.0), Owes::Frame, "the first tick starts the clock");
        assert_eq!(phase(s.look(Some(5.0), &t, full)), Some(SplashPhase::Hold));
        assert_eq!(s.owes(5.0 + CAP - 0.01), Owes::Nothing);
        assert_eq!(s.owes(5.0 + CAP), Owes::Frame);
        assert_eq!(
            phase(s.look(Some(5.0 + CAP - 0.01), &t, full)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(
            phase(s.look(Some(5.0 + CAP), &t, full)),
            Some(SplashPhase::Hold)
        );
        assert_eq!(s.owes(5.0 + CAP), Owes::Frame);
        assert_eq!(
            phase(s.look(Some(5.0 + CAP + 0.016), &t, full)),
            Some(SplashPhase::Vanish(0.0))
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
        let _ = s.look(Some(0.9), &t, SystemMotion::Full);
        assert_eq!(
            phase(s.look(Some(1.0), &t, SystemMotion::Full)),
            Some(SplashPhase::Vanish(0.0))
        );
        assert_eq!(s.look(Some(1.05), &t, reduced), None);
        assert_eq!(s.owes(1.1), Owes::Nothing);
    }

    /// **Off draws nothing and owes nothing; turned off live it goes at
    /// once; turned on after launch it stays gone.**
    #[test]
    fn off_draws_nothing_and_a_reload_to_off_cuts_it() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let full = SystemMotion::Full;
        let mut off = Splash::new(LaunchSplash::Off);
        assert_eq!(off.look(None, &t, full), None);
        assert_eq!(off.look(Some(1.0), &t, full), None);
        assert_eq!(off.owes(1.0), Owes::Nothing);
        assert_eq!(off.set(LaunchSplash::On), Owes::Nothing);
        assert_eq!(off.look(Some(1.1), &t, full), None);
        let mut s = Splash::new(LaunchSplash::On);
        assert_eq!(phase(s.look(Some(1.0), &t, full)), Some(SplashPhase::Hold));
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
