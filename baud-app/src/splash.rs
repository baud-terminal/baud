//! **The launch splash, as both drivers time it** (note AJ): nothing of
//! it until the window is shown -- on macOS the first display-link tick at
//! which a frame has reached the layer and AppKit reports the window
//! visible, which it showed once the app finished launching, its first
//! frame already drawn, whole, with no animation, and the plasma's pipeline
//! built on a thread of its own since; on Linux the compositor's first
//! frame callback -- then baud's
//! colour lockup carried in by the plasma, left to right, and carried out
//! by a second, left to right, gone [`gone`] after it starts -- on the clock alone,
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
//! **Every window plays it**, the launch's and each one opened after it
//! -- Command-N, File > New Window and a Dock click on macOS
//! (`open_window`), Ctrl+Shift+N on Linux (`windows::spawn_window`) -- as
//! `[launch] splash`, `BAUD_SPLASH` and Reduce Motion say for the first.
//! The logo is baud's greeting for a window rather than a launch screen,
//! so the HIG's launch screen, a splash at the end of launching
//! (`[[source: apple-hig-launching]]`), is not what places it: that is
//! guidance, not a standard, and nothing of the splash delays a window.
//! Each window's splash is its own driver's, on its own clock from its own
//! first frame shown.
//!
//! **It waits on nothing, and nothing waits on it.** The shell is
//! spawned before the first frame, in every window, and keys go to it
//! throughout. Its output is drawn as it is with no logo on the frame it
//! arrives: the logo and its plasma yield every cell the shell has drawn
//! (`Renderer::splash_yields_to`, which each driver calls after handing
//! the renderer the splash), so the prompt is whole and readable from that
//! frame while the logo plays around it. Nothing the logo needs is on the
//! way to the window's first frame: the plasma's pipeline is built on a
//! thread of its own once the window is shown, and the logo starts on the
//! first tick after it is built (`App::plasma_ready`). The splash only
//! owes frames while it plays or fades, and the one the window's being
//! shown starts. Its look takes the terminal by shared reference: it
//! cannot write the grid, the cursor a report gives or the history.
//!
//! **"The shell drew something" is a visible cell**, not a byte: a shell's
//! first bytes are often an `OSC 7` directory, a title or mode sets, which
//! draw nothing, and the splash would be gone before anything was drawn.

use crate::config::{LaunchSplash, SystemMotion};
use baud_core::Term;
use baud_domain::{Content, Row};
#[cfg(any(target_os = "macos", test))]
use baud_domain::{HostTime, ReadLen};
use baud_render::{SplashLook, SplashPhase};
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

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
    /// Carried in by the plasma and out by a second; the clock it runs
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
/// (`baud_render::splash::GONE`), for the tests: [`Splash::look`] reads
/// the end in the renderer's own precision.
#[cfg(test)]
pub(crate) fn gone() -> f64 {
    f64::from(baud_render::splash::GONE)
}

/// Seconds into the splash at which the plasma that carries the logo out
/// lights (`baud_render::splash::OUT`): reduced motion arriving before it
/// shows the logo whole, and after it takes the logo down.
fn out() -> f64 {
    f64::from(baud_render::splash::OUT)
}

impl Splash {
    /// The splash a window starts with, the launch's and every one opened
    /// after it: waiting for the window, or gone when the setting is off.
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
    /// owes the one frame that clears it; `"on"` in a window already open
    /// shows nothing, a window being greeted only as it opens.
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
    /// comes out shows it whole at once, and once the plasma that carries
    /// it out has lit takes it down, as it cuts a trail in flight.
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
                // In the renderer's own precision, as the phase below hands
                // it: a frame whose age reads as the end is the end, rather
                // than a last frame of no light that repeats the clear one.
                if (now - since) as f32 >= baud_render::splash::GONE {
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

/// **What the launch logo's frames and the shell's first reads did**, for
/// `BAUD_STARTUP=1` (note AJ section 18): each frame committed while the
/// logo is drawn, the first frame with a visible cell, each read that
/// arrived before the logo ended and the frame that drew it, and the
/// longest display-link tick or wake on the main thread, said once, when
/// the logo is gone. Under Reduce Motion the launch's first frame, drawn
/// with the logo held before the window is shown, counts as one of the
/// logo's frames, so the first interval runs from it to the first tick. So
/// a launch shows whether the logo's frames kept their interval while the
/// shell started, and whether the shell's output waited for them. Off, it
/// records nothing.
#[cfg(any(target_os = "macos", test))]
pub(crate) struct LaunchLog {
    state: Logging,
    /// The commit of each frame that drew the logo.
    frames: Vec<HostTime>,
    /// The first frame committed with a visible cell, and the interval it
    /// ended.
    visible: Option<(HostTime, Option<HostTime>)>,
    /// Each read before the logo ended, at most [`LaunchLog::READS`]: when
    /// it arrived, its length, and the commit of the frame that drew it.
    reads: Vec<(HostTime, ReadLen, Option<HostTime>)>,
    /// The main thread's longest turn while the logo was drawn: how long,
    /// and when it began.
    turn: Option<(Duration, HostTime)>,
}

/// Whether a [`LaunchLog`] records, has said what it recorded, or never
/// records.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Logging {
    On,
    Said,
    Off,
}

/// Whether a committed frame drew the logo.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Logo {
    Drawn,
    None,
}

#[cfg(any(target_os = "macos", test))]
impl LaunchLog {
    /// The reads kept: a shell's start is a dozen.
    const READS: usize = 32;

    pub(crate) fn new(state: Logging) -> LaunchLog {
        LaunchLog {
            state,
            frames: Vec::new(),
            visible: None,
            reads: Vec::new(),
            turn: None,
        }
    }

    /// A read that arrived at `at`, `len` bytes long.
    pub(crate) fn read(&mut self, at: HostTime, len: ReadLen) {
        if self.state == Logging::On && self.reads.len() < Self::READS {
            self.reads.push((at, len, None));
        }
    }

    /// **A frame committed at `at`**: it drew every read before it, and
    /// `logo` says whether it drew the logo. The first frame without it
    /// after frames with it gives the lines to say ([`LaunchLog::ended`]).
    pub(crate) fn committed(
        &mut self,
        at: HostTime,
        logo: Logo,
        term: &Term<'_>,
    ) -> Option<Vec<String>> {
        if self.state != Logging::On {
            return None;
        }
        for r in self.reads.iter_mut().filter(|r| r.2.is_none()) {
            r.2 = Some(at);
        }
        if self.visible.is_none() && drawn(term) {
            self.visible = Some((at, self.frames.last().copied()));
        }
        match logo {
            Logo::Drawn => {
                self.frames.push(at);
                None
            }
            Logo::None => self.ended(),
        }
    }

    /// A turn of the main thread that began at `at` and took `took`, kept
    /// while the logo is drawn.
    pub(crate) fn turn(&mut self, at: HostTime, took: Duration) {
        if self.state == Logging::On
            && !self.frames.is_empty()
            && self.turn.is_none_or(|(t, _)| took > t)
        {
            self.turn = Some((took, at));
        }
    }

    /// **The lines to say once the logo is gone**, once; none before the
    /// logo has drawn a frame, or when off.
    fn ended(&mut self) -> Option<Vec<String>> {
        let (&first, &last) = self.frames.first().zip(self.frames.last())?;
        if self.state != Logging::On {
            return None;
        }
        self.state = Logging::Said;
        let ms = |a: HostTime, b: HostTime| (a.nanos() as f64 - b.nanos() as f64) / 1e6;
        let mut gaps: Vec<(f64, HostTime)> = self
            .frames
            .windows(2)
            .map(|w| (ms(w[1], w[0]), w[1]))
            .collect();
        let worst = gaps.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0));
        gaps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let median = gaps.get(gaps.len() / 2).map_or(0.0, |g| g.0);
        let mut lines = vec![match worst {
            Some((w, end)) => format!(
                "splash: {} frames over {:.1} ms; intervals {median:.1} ms median, {w:.1} worst, the worst ending {:.1} ms in",
                self.frames.len(),
                ms(last, first),
                ms(end, first)
            ),
            None => "splash: 1 frame".to_string(),
        }];
        lines.push(match self.visible {
            Some((at, since)) => format!(
                "splash: the shell's first visible cell drawn {:.1} ms in{}",
                ms(at, first),
                since.map_or(String::new(), |s| format!(
                    ", {:.1} ms after the frame before",
                    ms(at, s)
                ))
            ),
            None => "splash: no visible cell drawn while the logo played".to_string(),
        });
        let reads: Vec<String> = self
            .reads
            .iter()
            .map(|&(at, len, drawn)| match drawn {
                Some(d) => format!("{:+.1} {} B {:+.1}", ms(at, first), len.index(), ms(d, at)),
                None => format!("{:+.1} {} B undrawn", ms(at, first), len.index()),
            })
            .collect();
        let late = self
            .reads
            .iter()
            .filter_map(|&(at, _, d)| d.map(|d| ms(d, at)))
            .max_by(f64::total_cmp);
        lines.push(format!(
            "splash: reads (ms in, bytes, ms to their frame): {}{}",
            reads.join("; "),
            late.map_or(String::new(), |l| format!("; worst {l:.1}"))
        ));
        if let Some((took, at)) = self.turn {
            lines.push(format!(
                "splash: longest main-thread turn {:.1} ms, {:.1} ms in",
                took.as_secs_f64() * 1e3,
                ms(at, first)
            ));
        }
        Some(lines)
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
    use baud_domain::{BytePos, WinSize};

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

    /// **Its times are the renderer's**: gone 0.55 s after it starts, the
    /// plasma that carries the logo out lighting before, and both before
    /// that plasma's own end, which it does not wait for.
    #[test]
    fn it_is_gone_by_the_renderers_end() {
        assert!((gone() - 0.55).abs() < 1e-6, "{}", gone());
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
    /// that clears it, none repeats the frame before bit for bit -- a
    /// bit-identity check, not a visible-change threshold: the last frames
    /// change little as the remainder fades. At 60 and 120 frames a second.
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

    /// **The logo is gone at its end**: from its first frame to the one
    /// that clears it, at most 0.55 s and a frame, at 60 and 120 frames a
    /// second.
    #[test]
    fn the_splash_is_gone_at_its_end() {
        for rate in [60.0, 120.0] {
            let frames = life(rate);
            let on = (frames.len() - 1) as f64 / rate;
            assert!(on <= 0.55 + 1.0 / rate + 1e-9, "{rate} Hz: {on} s");
        }
    }

    /// **The shell's prompt is seen on the frame it arrives, with the
    /// logo or without it**: a window with the splash on and one with it
    /// off, frame by frame at 120 Hz in the drivers' order -- the read fed,
    /// the splash's look handed to the renderer and told what the shell
    /// drew, the frame drawn on the CPU path with incremental damage --
    /// take the same output, eight lines and a prompt that reach the
    /// middle of the window where the logo is, landing early, mid-way and
    /// late in the plasma. Each frame of the window with the splash draws
    /// every cell the shell inked as the window without it does, so the
    /// prompt is whole and readable on the same frame in both: the frame
    /// it arrives on. The control: the logo is drawn after the prompt
    /// lands, and the output reaches its box.
    #[test]
    fn the_prompt_is_seen_on_the_frame_it_arrives_with_the_splash_or_without() {
        use baud_domain::{Cols, Rows};
        use baud_font::Px;
        use baud_render::{
            Blend, Damage, FaceRole, FaceStyle, PxSize, Renderer, Stack, Target, Theme,
        };
        let faces = [
            baud_font::Face::parse(crate::embedded::REGULAR.bytes).unwrap(),
            baud_font::Face::parse(crate::embedded::BOLD.bytes).unwrap(),
        ];
        let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
        let ws = WinSize {
            ws_col: 80,
            ws_row: 20,
        };
        let renderer = || {
            Renderer::new(
                Stack::with_roles(&faces, &roles).unwrap(),
                PxSize::of(14.0).unwrap(),
                Theme::dark(),
                Blend::Native,
                Cols::of_index(usize::from(ws.ws_col)),
                Rows::of_index(usize::from(ws.ws_row)),
            )
            .unwrap()
        };
        let mut output: Vec<u8> = (0..8)
            .flat_map(|i| {
                format!("motd {i}: the first lines of the shell's start\r\n").into_bytes()
            })
            .collect();
        output
            .extend_from_slice(b"\x1b[1;34m~/code/projects/baud\x1b[0m on \x1b[35mmain\x1b[0m $ ");
        let rate = 120.0;
        for lands in [5_u32, 30, 60] {
            // Each window's frames: whether every inked cell is drawn as
            // the window with no splash draws it, and whether the logo was.
            let window = |setting: LaunchSplash| {
                let mut r = renderer();
                let mut owned = Owned::new(&ws);
                let mut t = Term::new(owned.storage(), &ws);
                let (w, h) = r.pixel_size();
                let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
                let mut s = Splash::new(setting);
                let _ = s.shown();
                let mut frames = Vec::new();
                for f in 0..80_u32 {
                    if f == lands {
                        feed(&mut t, &output);
                    }
                    let look = s.look(Some(f64::from(f) / rate), &t, SystemMotion::Full);
                    r.set_splash(look);
                    r.splash_yields_to(&t, Px::of(0));
                    let mut target = Target::new(&mut px, w, w, h).unwrap();
                    let damage = match f {
                        0 => Damage::Full,
                        _ => Damage::Incremental,
                    };
                    r.render(&mut t, &mut target, damage);
                    frames.push((px.clone(), look.is_some()));
                }
                let m = r.metrics();
                let n = |p: baud_font::Px| usize::try_from(p.value()).unwrap();
                let view = t.view(t.window_back());
                let inked: Vec<(usize, usize)> = (0..view.rows().index())
                    .flat_map(|y| {
                        view.row(Row::from_index(y))
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| {
                                !c.content.is_empty()
                                    && c.content.as_codepoint().is_none_or(|p| p.value() != 0x20)
                            })
                            .map(move |(x, _)| (y, x))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                (frames, inked, (n(m.cell_w), n(m.cell_h), n(w)))
            };
            let (on, inked, (cw, ch, w)) = window(LaunchSplash::On);
            let (off, _, _) = window(LaunchSplash::Off);
            let cell = |px: &[u32], (y, x): (usize, usize)| -> Vec<u32> {
                (0..ch)
                    .flat_map(|dy| px[(y * ch + dy) * w + x * cw..][..cw].to_vec())
                    .collect()
            };
            let seen = |frames: &[(Vec<u32>, bool)], end: &[u32]| {
                frames
                    .iter()
                    .position(|(px, _)| inked.iter().all(|&c| cell(px, c) == cell(end, c)))
            };
            let end = &off.last().unwrap().0;
            let (at_on, at_off) = (seen(&on, end), seen(&off, end));
            assert_eq!(
                at_off,
                usize::try_from(lands).ok(),
                "the control: drawn as it lands"
            );
            assert_eq!(at_on, at_off, "landing on frame {lands}");
            let from = usize::try_from(lands).unwrap();
            for (f, ((a, _), (b, _))) in on.iter().zip(&off).enumerate().skip(from) {
                for &c in &inked {
                    assert!(
                        cell(a, c) == cell(b, c),
                        "frame {f}: cell {c:?} under the logo"
                    );
                }
            }
            assert!(
                on[from..].iter().any(|(_, logo)| *logo),
                "the control: the logo plays after it lands"
            );
            let side = |v: usize| f32::from(u16::try_from(v).unwrap());
            let (bx, by) = {
                let frame = (side(w), side(ch * usize::from(ws.ws_row)));
                let sc = baud_render::splash::scale(frame);
                let o = baud_render::splash::origin(frame, sc);
                (o.x / side(cw), o.y / side(ch))
            };
            assert!(
                inked
                    .iter()
                    .any(|&(y, x)| side(y + 1) > by && side(x + 1) > bx),
                "the control: the output reaches the logo's box"
            );
        }
    }

    /// **Every driver tells the splash what the shell drew**: on macOS, on
    /// Linux and in the films, each hand of a splash to the renderer is
    /// followed by `splash_yields_to` over the terminal it draws, so no
    /// path draws the logo over the shell's cells. The drivers are bound to
    /// AppKit and a compositor, so the source is what can fail; read here
    /// so it is checked on every platform.
    #[test]
    fn every_driver_yields_the_splash_to_the_shell() {
        for (file, src, hand, yields) in [
            (
                "app.rs",
                include_str!("app.rs"),
                "self.renderer.set_splash(splash);",
                "self.renderer.splash_yields_to(&self.term, lift);",
            ),
            (
                "linux.rs",
                include_str!("linux.rs"),
                "self.renderer.set_splash(plan.splash);",
                "self.renderer.splash_yields_to(&self.term, Px::of(0));",
            ),
            (
                "film.rs",
                include_str!("film.rs"),
                "r.set_splash(plan.splash);",
                "r.splash_yields_to(&term, Px::of(0));",
            ),
        ] {
            let hands = src.matches(hand).count();
            assert!(hands > 0, "the control: {file} hands the splash");
            for (_, after) in src.match_indices(hand).map(|(i, _)| src.split_at(i)) {
                let next = after[hand.len()..]
                    .find("set_splash(")
                    .map_or(after.len(), |e| e + hand.len());
                assert!(
                    after[..next].contains(yields),
                    "{file}: a splash handed and not yielded: {}",
                    &after[..next.min(400)]
                );
            }
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

    /// **Every Linux window plays the logo, as the launch's does**: a new
    /// window is a new baud (`windows::spawn_window`), and its driver
    /// takes the splash from `[launch] splash` with `BAUD_SPLASH` over it,
    /// with no case for a process started as a window; its shell is
    /// spawned before the driver that draws its first frame is built, and
    /// nothing a key or a read does names the splash. The loop is Linux's,
    /// so the source is what can fail; read here so it is checked on every
    /// platform.
    #[test]
    fn every_linux_window_plays_the_logo_as_the_launchs_does() {
        const SRC: &str = include_str!("linux.rs");
        let given = concat!(".with_", "splash(");
        assert_eq!(SRC.matches(given).count(), 1, "one splash");
        let (before, after) = SRC.split_once(given).expect("the driver's splash");
        let arg = after
            .split_once("\n        bus,")
            .map(|(a, _)| a)
            .expect("its argument");
        assert!(
            arg.trim_start().starts_with(concat!(
                "crate::splash::splash_override(\n",
                "                std::env::var(\"BAUD_SPLASH\").ok().as_deref(),\n",
                "                cfg.launch_splash,"
            )),
            "the config's, BAUD_SPLASH over it: {arg}"
        );
        for named in ["LaunchSplash::Off", "Started", "NEW_WINDOW"] {
            assert!(!arg.contains(named), "{named} in the splash: {arg}");
        }
        assert!(
            before.contains(concat!("Pty::", "spawn_polled(")),
            "the shell is spawned before the driver is built"
        );
        let windows = include_str!("windows.rs");
        let spawn = windows
            .split_once(concat!(
                "pub fn spawn_",
                "window(dir: Option<&std::path::Path>) {"
            ))
            // Its body and the two it shares its start with, `new_baud` and
            // `running_window`, a link's window: all three, to the tests.
            .and_then(|(_, rest)| rest.split_once("\n#[cfg(test)]\nmod tests"))
            .map(|(b, _)| b)
            .expect("the new windows' spawns");
        assert!(
            spawn.contains(concat!("cmd.env_", "remove(\"WAYLAND_SOCKET\");")),
            "the control: the spawn's environment"
        );
        assert!(
            !spawn.contains("NEW_WINDOW"),
            "a window told apart: {spawn}"
        );
        for head in [
            concat!("    fn feed(&mut self, ", "bytes: &[u8]) {"),
            concat!(
                "    fn route_key(&mut self, ",
                "route: crate::pressed::Route) {"
            ),
        ] {
            let path = SRC
                .split_once(head)
                .and_then(|(_, rest)| rest.split_once("\n    }\n"))
                .map(|(b, _)| b)
                .unwrap_or_else(|| panic!("{head}"));
            assert!(!path.contains("splash"), "{head} reads the splash");
        }
    }

    /// **Each window's splash is its own**: two windows opened close
    /// together, the second shown a quarter of a second after the first,
    /// each play the whole plasma from their own first frame with a clock
    /// and are gone [`gone`] after it, the first's end leaving the
    /// second's playing. A window's splash is its own driver's
    /// (`App::splash`, `Timed::splash`), shown by its own window's first
    /// frame.
    #[test]
    fn each_window_plays_its_own_splash_on_its_own_clock() {
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let full = SystemMotion::Full;
        let rate = 120.0;
        let shown_at = [12_u32, 42];
        let mut windows = [Splash::new(LaunchSplash::On), Splash::new(LaunchSplash::On)];
        let mut first = [None; 2];
        let mut ends = [None; 2];
        for f in 0..240_u32 {
            let now = f64::from(f) / rate;
            for (w, s) in windows.iter_mut().enumerate() {
                if f == shown_at[w] {
                    assert_eq!(s.shown(), Owes::Frame, "window {w}");
                }
                match phase(s.look(Some(now), &t, full)) {
                    Some(SplashPhase::Plasma(a)) => {
                        let since = *first[w].get_or_insert(now);
                        assert!((f64::from(a) - (now - since)).abs() < 1e-4, "{w} {a}");
                    }
                    None if first[w].is_some() && ends[w].is_none() => ends[w] = Some(now),
                    None => {}
                    other => panic!("window {w}: {other:?}"),
                }
            }
        }
        for w in 0..2 {
            let since = first[w].expect("it plays");
            assert!(
                (since - f64::from(shown_at[w]) / rate).abs() < 1e-9,
                "window {w} starts on its own first frame"
            );
            let end = ends[w].expect("and goes");
            assert!((end - since - gone()).abs() <= 1.0 / rate, "{w}: {end}");
        }
        let (a, b) = (ends[0].expect("first"), first[1].expect("second"));
        assert!(
            b < a && a < ends[1].expect("second"),
            "the second still plays"
        );
    }

    /// **The Linux driver shows the window to the splash on the
    /// compositor's frame callback**: its event loop's `Event::Frame` arm
    /// calls `Timed::shown` and owes the frame its clock starts on, and no
    /// other arm does -- the first `Event::Frame` answers the first commit
    /// that attached a buffer (`baud-present-wayland`'s handshake test).
    /// The loop is Linux's and waits on a compositor, so the source is
    /// what can fail; read here so it is checked on every platform.
    #[test]
    fn the_linux_driver_shows_the_window_on_the_frame_callback() {
        const SRC: &str = include_str!("linux.rs");
        let start = concat!("d.timed.", "shown()");
        assert_eq!(SRC.matches(start).count(), 1, "one start");
        let (before, after) = SRC.split_once(start).expect("the start");
        let arm = before
            .rsplit_once("Event::")
            .map(|(_, arm)| arm)
            .expect("an event arm");
        assert!(arm.starts_with("Frame => {"), "the start's arm: {arm}");
        let body = after.split_once('}').map(|(b, _)| b).expect("its body");
        assert!(
            body.contains("d.pending = Pending::Dirty;"),
            "the frame it owes: {body}"
        );
        let timed = include_str!("timed.rs");
        let shown = timed
            .split_once(concat!("pub(crate) fn ", "shown(&mut self) -> Owes {"))
            .map(|(_, rest)| rest)
            .expect("Timed::shown");
        assert!(
            shown
                .trim_start()
                .starts_with("match self.splash.shown() {"),
            "Timed::shown starts the splash"
        );
    }

    /// **The launch log says what the logo's frames and the shell's reads
    /// did** (note AJ section 18): the frames and their worst interval and
    /// where it ended, the frame the shell's first visible cell was drawn
    /// on and the interval before it, each read's arrival and the wait for
    /// the frame that drew it, and the main thread's longest turn -- once,
    /// on the first frame after the logo, and nothing when off.
    #[test]
    fn the_launch_log_says_the_logos_frames_and_the_shells_reads() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let at = |ms: f64| HostTime::of_nanos((1e9 + ms * 1e6) as u64);
        let mut log = LaunchLog::new(Logging::On);
        log.read(at(-5.0), ReadLen::from_index(144));
        assert_eq!(log.committed(at(0.0), Logo::Drawn, &t), None);
        log.turn(at(1.0), Duration::from_micros(3_100));
        log.turn(at(2.0), Duration::from_micros(900));
        assert_eq!(log.committed(at(8.0), Logo::Drawn, &t), None);
        log.read(at(12.0), ReadLen::from_index(478));
        feed(&mut t, b"zsh ~");
        assert_eq!(log.committed(at(33.0), Logo::Drawn, &t), None);
        assert_eq!(log.committed(at(41.0), Logo::Drawn, &t), None);
        let lines = log.committed(at(49.0), Logo::None, &t).expect("said");
        assert_eq!(
            lines,
            [
                "splash: 4 frames over 41.0 ms; intervals 8.0 ms median, 25.0 worst, the worst ending 33.0 ms in",
                "splash: the shell's first visible cell drawn 33.0 ms in, 25.0 ms after the frame before",
                "splash: reads (ms in, bytes, ms to their frame): -5.0 144 B +5.0; +12.0 478 B +21.0; worst 21.0",
                "splash: longest main-thread turn 3.1 ms, 1.0 ms in",
            ]
        );
        assert_eq!(log.committed(at(57.0), Logo::None, &t), None, "once");
        let mut off = LaunchLog::new(Logging::Off);
        off.read(at(0.0), ReadLen::ONE);
        assert_eq!(off.committed(at(0.0), Logo::Drawn, &t), None);
        assert_eq!(
            off.committed(at(9.0), Logo::None, &t),
            None,
            "off says nothing"
        );
    }
}
