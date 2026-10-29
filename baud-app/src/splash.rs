//! **The launch splash, as both drivers time it** (note AJ): nothing of
//! it until the window is shown -- on macOS the first display-link tick at
//! which a frame has reached the layer, the plasma's pipeline is built and
//! AppKit reports the window visible, on Linux the compositor's first frame callback -- then baud's
//! colour lockup coming out of the plasma, left to right, and swept out of
//! it left to right, gone [`gone`] after it starts -- on the clock alone,
//! so a shell's output neither cuts it short nor holds it longer. Under reduced
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
//! frame it arrives, and the splash only owes frames while it plays or
//! fades, and the one the window's being shown starts.
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
    /// Coming out of the plasma and swept out of it; the clock it runs
    /// from, once a frame has had one.
    Playing(Option<f64>),
    /// Shown whole under reduced motion, which has no plasma, until the
    /// shell's first visible cell or [`CAP`]; the clock it runs from.
    Held(Option<f64>),
    /// The hold is over: this frame shows the logo still held, and the
    /// fade starts on the next -- so a frame that brings the shell's text
    /// costs what a held frame does and no more.
    Due,
    /// Fading since this time, under reduced motion.
    Fading(f64),
    /// Gone, or never shown.
    Gone,
}

/// **The splash's timing.**
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Splash {
    state: State,
}

/// Seconds from the splash's first frame to the frame it is gone on
/// (`baud_render::splash::GONE`).
pub(crate) fn gone() -> f64 {
    f64::from(baud_render::splash::GONE)
}

/// Seconds into the splash at which the sweep out starts
/// (`baud_render::splash::OUT`): reduced motion arriving before it shows
/// the logo whole, and after it takes the logo down.
fn out() -> f64 {
    f64::from(baud_render::splash::OUT)
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

    /// **The window is shown**: the plasma, or the reduced-motion hold,
    /// runs from the next frame with a clock, which this owes. Once.
    pub(crate) fn shown(&mut self) -> Owes {
        match self.state {
            State::Waiting => {
                self.state = State::Playing(None);
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
                State::Waiting | State::Playing(_) | State::Held(_) | State::Due | State::Fading(_),
            ) => {
                self.state = State::Gone;
                Owes::Frame
            }
            _ => Owes::Nothing,
        }
    }

    /// **The look for a frame at `now`**, which advances the state: nothing
    /// while it waits for the window (the logo whole under reduced
    /// motion); then the plasma from the first frame with a clock, gone
    /// [`gone`] into it; under reduced motion the hold and the fade from
    /// the frame after it ends; a fade that is over is gone. `None` for
    /// `now` -- a frame before the clock has a reading, the first one on
    /// macOS -- starts no clock. Reduced motion arriving while the logo
    /// comes out shows it whole at once, and once the sweep out has started
    /// takes it down, as it cuts a trail in flight.
    pub(crate) fn look(
        &mut self,
        now: Option<f64>,
        term: &Term<'_>,
        motion: SystemMotion,
    ) -> Option<SplashLook> {
        if motion == SystemMotion::Reduced {
            if let State::Playing(since) = self.state {
                let sweeping = since.zip(now).is_some_and(|(s, n)| n - s >= out());
                self.state = match sweeping {
                    true => State::Gone,
                    false => State::Held(since),
                };
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
                State::Playing(_) => Some(SplashLook {
                    phase: SplashPhase::Plasma(0.0),
                }),
                State::Held(_) | State::Due => held,
                _ => None,
            };
        };
        match self.state {
            State::Playing(since) => {
                let since = since.unwrap_or(now);
                if now - since >= gone() {
                    // Gone: this frame draws none, which repaints the
                    // screen once to clear it, and nothing more is owed.
                    self.state = State::Gone;
                    return None;
                }
                self.state = State::Playing(Some(since));
            }
            State::Due => self.state = State::Fading(now),
            State::Held(since) => {
                let since = since.unwrap_or(now);
                self.state = match drawn(term) || now - since >= CAP {
                    true => State::Due,
                    false => State::Held(Some(since)),
                };
            }
            State::Waiting | State::Fading(_) | State::Gone => {}
        }
        let phase = match self.state {
            State::Playing(since) => {
                SplashPhase::Plasma((now - since.unwrap_or(now)).max(0.0) as f32)
            }
            State::Held(_) | State::Due => SplashPhase::Hold,
            State::Waiting | State::Gone => return None,
            State::Fading(from) => {
                let p = (now - from).max(0.0) as f32 / baud_render::splash::FADE_S;
                if p >= 1.0 {
                    // Over: this frame draws none, which repaints the
                    // screen once to clear it, and nothing more is owed.
                    self.state = State::Gone;
                    return None;
                }
                SplashPhase::Fade(p)
            }
        };
        Some(SplashLook { phase })
    }

    /// **What a tick at `now` owes**: nothing while it waits for the
    /// window, a frame every tick while it plays or fades, and under
    /// reduced motion the one its cap runs out on -- the first frame after
    /// the screen gets a visible cell is the output's own.
    pub(crate) fn owes(&self, now: f64) -> Owes {
        match self.state {
            State::Playing(_) | State::Fading(_) | State::Due | State::Held(None) => Owes::Frame,
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
    /// draws with, so the plasma's first frame does not pay for it. The
    /// macOS GPU path's: the CPU path builds nothing.
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

    /// **The plasma waits for the window**: before it is shown no frame
    /// draws anything of the logo, with a clock or without, and nothing is
    /// owed, nor a deadline set, however long it takes; the plasma is built
    /// meanwhile. Being shown owes one frame, once, and the splash's clock
    /// starts on the first frame after it.
    #[test]
    fn the_plasma_waits_for_the_window_to_be_shown() {
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
            Some(SplashPhase::Plasma(0.0)),
            "the clock starts after the window is shown"
        );
        let Some(SplashPhase::Plasma(a)) = phase(s.look(Some(7.1), &t, full)) else {
            panic!("playing");
        };
        assert!((a - 0.1).abs() < 1e-4, "{a}");
    }

    /// **It plays on the display link's clock alone, and is gone at
    /// [`gone`]**: the plasma runs from the first frame with a clock after
    /// the window is shown, owing every frame -- output drawn meanwhile
    /// neither ends it nor holds it longer -- its phase the seconds since
    /// that frame; the frame [`gone`] into it draws none, and nothing is
    /// owed after.
    #[test]
    fn it_plays_on_the_clock_and_is_gone_at_its_end() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut s = Splash::new(LaunchSplash::On);
        let full = SystemMotion::Full;
        let _ = s.shown();
        assert_eq!(
            phase(s.look(None, &t, full)),
            Some(SplashPhase::Plasma(0.0)),
            "a frame before the clock"
        );
        assert_eq!(
            phase(s.look(Some(5.0), &t, full)),
            Some(SplashPhase::Plasma(0.0))
        );
        feed(&mut t, b"% ");
        assert_eq!(s.owes(5.1), Owes::Frame, "every frame while it plays");
        assert_eq!(s.deadline(), None);
        let Some(SplashPhase::Plasma(a)) = phase(s.look(Some(5.0 + gone() - 0.01), &t, full))
        else {
            panic!("output does not end it");
        };
        assert!((f64::from(a) - (gone() - 0.01)).abs() < 1e-4, "{a}");
        assert_eq!(s.owes(5.0 + gone() - 0.005), Owes::Frame);
        assert_eq!(s.look(Some(5.0 + gone()), &t, full), None, "gone");
        assert_eq!(s.owes(5.0 + gone() + 0.01), Owes::Nothing);
        assert_eq!(s.look(Some(5.0 + gone() + 0.02), &t, full), None);
    }

    /// **Its times are the renderer's**: gone half a second after it
    /// starts, the sweep out starting before, and both before the plasma's
    /// own end, which it does not wait for.
    #[test]
    fn it_is_gone_half_a_second_after_it_starts() {
        assert!((gone() - 0.5).abs() < 1e-6, "{}", gone());
        assert!(out() < gone(), "{} {}", out(), gone());
        assert!(gone() < f64::from(baud_render::splash::end()));
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

    /// **A shell that never draws does not hold it**, and one that draws
    /// at once does not cut it short: it is gone on the same frame either
    /// way, [`gone`] after the window is shown.
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
                let look = phase(s.look(Some(now), &t, full));
                if f > 30 && look.is_none() {
                    at = Some(now);
                    break;
                }
            }
            starts.push(at.expect("it goes"));
        }
        assert_eq!(starts[0], starts[1]);
        let shown = 30.0 / 120.0;
        assert!(
            (starts[0] - (shown + gone())).abs() <= 2.0 / 120.0,
            "{starts:?}"
        );
    }

    /// **Under reduced motion there is no plasma**: the logo whole from
    /// the first frame, as before; and reduced motion arriving while the
    /// logo comes out shows it whole at once, its plasma cut.
    #[test]
    fn reduced_motion_shows_it_whole_and_cuts_the_plasma_coming_out() {
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
            Some(SplashPhase::Plasma(0.0))
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
    /// after it; a plasma sweeping the logo out is gone on the first frame
    /// that reads reduced.
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
        let due = 1.0 + out();
        let _ = s.look(Some(1.0), &t, full);
        let Some(SplashPhase::Plasma(a)) = phase(s.look(Some(due), &t, full)) else {
            panic!("sweeping out");
        };
        assert!((f64::from(a) - out()).abs() < 1e-4, "{a}");
        assert_eq!(s.look(Some(due + 0.05), &t, reduced), None);
        assert_eq!(s.owes(due + 0.1), Owes::Nothing);
    }

    /// **Off draws nothing and owes nothing; turned off live it goes at
    /// once, waiting or playing; turned on after launch it stays gone.**
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
            Some(SplashPhase::Plasma(0.0))
        );
        assert_eq!(s.set(LaunchSplash::On), Owes::Nothing);
        assert_eq!(s.set(LaunchSplash::Off), Owes::Frame);
        assert_eq!(s.look(Some(1.1), &t, full), None);
        assert_eq!(s.set(LaunchSplash::Off), Owes::Nothing);
    }

    /// One launch's splash at `rate` frames a second, under full motion,
    /// as the driver runs it -- shown, then a frame on every tick it owes
    /// one, each look handed to the renderer and drawn on the CPU path, the
    /// pixels the GPU arms are held to, over a prompt and output under the
    /// logo -- from the first frame that draws the logo to the one that
    /// clears it: each frame's pixels and why its plasma carries the alpha
    /// it does.
    fn life(rate: f64) -> Vec<(Vec<u32>, baud_render::Held)> {
        use baud_domain::{Cols, Rows};
        use baud_render::{
            Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, Stack, Target, Theme,
        };
        let faces = [
            baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap(),
            baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap(),
        ];
        let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
        let ws = WinSize {
            ws_col: 64,
            ws_row: 20,
        };
        let mut r = Renderer::new(
            Stack::with_roles(&faces, &roles).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(usize::from(ws.ws_col)),
            Rows::of_index(usize::from(ws.ws_row)),
        )
        .unwrap();
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        // A prompt, and output in four colours under the logo, which a
        // capped pass would have to scan before it could draw.
        feed(
            &mut t,
            b"\x1b[1;34m~/code\x1b[0m on \x1b[35mmain\x1b[0m\r\n$ ",
        );
        for row in 8..=12 {
            let colours = "\x1b[31mab\x1b[32mcd\x1b[33mef\x1b[36mgh".repeat(5);
            feed(&mut t, format!("\x1b[{row};20H{colours}\x1b[0m").as_bytes());
        }
        let (w, h) = r.pixel_size();
        let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
        let mut s = Splash::new(LaunchSplash::On);
        assert_eq!(s.shown(), Owes::Frame);
        let mut out = Vec::new();
        for f in 0..2000u32 {
            let now = 3.0 + f64::from(f) / rate;
            if f > 0 && s.owes(now) == Owes::Nothing {
                break;
            }
            let look = s.look(Some(now), &t, SystemMotion::Full);
            r.set_splash(look);
            let mut target = Target::new(&mut px, w, w, h).unwrap();
            r.render(&mut t, &mut target, Damage::Full);
            out.push((px.clone(), r.flash_held()));
        }
        out
    }

    /// **No frame of the logo waits on a scan**: every frame from the first
    /// to the one that clears it draws its plasma as it is, or capped at a
    /// cap already known -- none draws it at nothing because a colour under
    /// it is still being scanned, which froze the vanish's first frame and
    /// cost it 5-7 ms. At 60 and 120 frames a second.
    #[test]
    fn no_splash_frame_waits_on_a_cap_scan() {
        for rate in [60.0, 120.0] {
            for (i, (_, held)) in life(rate).iter().enumerate() {
                assert!(
                    matches!(held, baud_render::Held::Full | baud_render::Held::Capped),
                    "{rate} Hz, frame {i}: {held:?}"
                );
            }
        }
    }

    /// **Every frame the logo owes moves**: from its first frame to the one
    /// that clears it, none is drawn the same as the frame before, so it
    /// never stands still while it holds the frame loop. At 60 and 120
    /// frames a second.
    #[test]
    fn every_frame_the_splash_owes_moves() {
        for rate in [60.0, 120.0] {
            let frames = life(rate);
            let same: Vec<usize> = (1..frames.len())
                .filter(|&i| frames[i].0 == frames[i - 1].0)
                .collect();
            assert!(
                same.is_empty(),
                "{rate} Hz: frames {same:?} of {}",
                frames.len()
            );
        }
    }

    /// **The logo is gone within half a second**: from its first frame to
    /// the one that clears it, at most 0.5 s and a frame, at 60 and 120
    /// frames a second.
    #[test]
    fn the_splash_is_gone_within_half_a_second() {
        for rate in [60.0, 120.0] {
            let frames = life(rate);
            let on = (frames.len() - 1) as f64 / rate;
            assert!(on <= 0.5 + 1.0 / rate + 1e-9, "{rate} Hz: {on} s");
        }
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
