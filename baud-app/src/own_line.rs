//! **The activity lights on a line of their own, fed by a driver with no
//! display link**: the Linux driver's, whose window has no title bar, so
//! the lights' own line in the frame is its default (`[window]
//! activity_lights`, `baud_render::lights`).
//!
//! The stamps are the macOS driver's (`lights.rs` there): RD each read of
//! the program's output and HS each one as long as the platform's tty hands
//! back (`baud_render::lights::READ_CEILING`, Linux's 4095), AA a reply the
//! core queued while it read that output, SD a write the kernel took any of
//! and CS one it refused any of, SYN and FAX from the core's state, and OH
//! while a job other than the shell is in front. **OH reads the terminal's
//! foreground group** (`TIOCGPGRP` on the master, `fg::group`), which both
//! kernels answer, and the shell `forkpty` started leads its own group
//! (`login_tty` makes it a session's leader), so a group in front other
//! than its pid is another job's. **CD is not built on Linux**: the ssh
//! member and its established socket are read there through Darwin's
//! process interface (`front.rs`), and the Linux reads of the same --
//! the group's members out of `/proc`, each ssh's socket inodes and
//! `/proc/net/tcp` -- are not written, so CD stays dark there.
//!
//! **Each change is scheduled, never waited for**: [`LineLights::wait`] is
//! the soonest moment a light can change, which bounds the driver's poll,
//! so a light goes dark when its hold ends and not at the next event.
//!
//! **The states are sampled at a frame's pace, not a read's**: the driver's
//! loop turns once a read, and a flood of output would otherwise make the
//! foreground group's `ioctl` once a chunk. [`LineLights::states`] reads
//! at most once every [`SAMPLE`], a 60 Hz frame, about the pace the macOS
//! display link samples them at, on the turns that happen. Nothing wakes
//! the loop only to sample. While a light is lit, though, the wait bound
//! wakes it at that light's hold's end: every 200 ms, or 2 s under Reduce
//! Motion. So with a job in front, an otherwise idle window reads the
//! foreground group at that pace. With every light dark and holding, it
//! makes no read.

use crate::pty::Wrote;
use crate::sys::Pid;
use baud_domain::HostTime;
use baud_render::{Activity, Glow, Led, Pace, Shown};
use core::time::Duration;

/// Who the terminal's foreground group is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum InFront {
    /// The shell itself, at its prompt.
    Shell,
    /// A job the shell started.
    Job,
    /// No group, or no shell to compare it with.
    Nobody,
}

/// **Who `group`, the terminal's foreground group, is**, `shell` the
/// session's root: the shell `forkpty` started leads its own group, so a
/// group other than its pid is a job's.
pub(crate) fn in_front(group: Option<Pid>, shell: Option<Pid>) -> InFront {
    match (group, shell) {
        (Some(g), Some(s)) if g == s => InFront::Shell,
        (Some(_), Some(_)) => InFront::Job,
        _ => InFront::Nobody,
    }
}

/// The least time between two samplings of OH, SYN and FAX: a 60 Hz
/// frame.
pub(crate) const SAMPLE: Duration = Duration::from_micros(16_667);

/// Whether a turn samples the states.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Sampled {
    /// Sampled now.
    Now,
    /// Sampled less than [`SAMPLE`] ago: nothing read.
    Recently,
}

/// The lights' clocks, what the line last showed, and when the states were
/// last sampled.
pub(crate) struct LineLights {
    activity: Activity,
    shown: Option<(Shown, Glow)>,
    sampled: Option<HostTime>,
}

impl LineLights {
    pub(crate) fn new() -> LineLights {
        LineLights {
            activity: Activity::new(),
            shown: None,
            sampled: None,
        }
    }

    /// **Whether OH, SYN and FAX are due a sampling at `now`**: the first
    /// time, and once [`SAMPLE`] has gone by since the last; marked sampled
    /// when they are.
    pub(crate) fn due(&mut self, now: HostTime) -> Sampled {
        match self.sampled {
            Some(at) if now < at.plus(SAMPLE) => Sampled::Recently,
            _ => {
                self.sampled = Some(now);
                Sampled::Now
            }
        }
    }

    /// One read of the program's output, at `now`: RD for any, HS for one
    /// at the platform's ceiling.
    pub(crate) fn read(&mut self, bytes: &[u8], now: HostTime) {
        if bytes.is_empty() {
            return;
        }
        self.activity.seen(Led::Rd, now);
        if baud_render::lights::full(bytes) {
            self.activity.seen(Led::Hs, now);
        }
    }

    /// A reply the core queued while it read the program's output, which
    /// nobody typed: AA.
    pub(crate) fn answered(&mut self, now: HostTime) {
        self.activity.seen(Led::Aa, now);
    }

    /// What a write to the child did: SD for any byte the kernel took, CS
    /// for any it refused.
    pub(crate) fn wrote(&mut self, w: Wrote, now: HostTime) {
        match w {
            Wrote::All => self.activity.seen(Led::Sd, now),
            Wrote::Part => {
                self.activity.seen(Led::Sd, now);
                self.activity.seen(Led::Cs, now);
            }
            Wrote::Nothing => self.activity.seen(Led::Cs, now),
        }
    }

    /// **The states at a turn**: OH from who is in front, SYN while the
    /// program holds synchronized output open, FAX while a picture is
    /// placed.
    pub(crate) fn states(&mut self, front: InFront, term: &baud_core::Term<'_>, now: HostTime) {
        if front == InFront::Job {
            self.activity.seen(Led::Oh, now);
        }
        if term.synchronized_output() {
            self.activity.seen(Led::Syn, now);
        }
        if term.images().any_placed() {
            self.activity.seen(Led::Fax, now);
        }
    }

    /// **The clocks stepped at `now`**: what the line shows now, at `glow`,
    /// when that is not what it showed last, which owes a frame; none when
    /// nothing moved.
    pub(crate) fn step(&mut self, now: HostTime, pace: Pace, glow: Glow) -> Option<(Shown, Glow)> {
        let shown = (self.activity.step(now, pace), glow);
        (self.shown != Some(shown)).then(|| {
            self.shown = Some(shown);
            shown
        })
    }

    /// The line drawn afresh: the next step owes a frame whatever moved.
    pub(crate) fn stale(&mut self) {
        self.shown = None;
    }

    /// **How long from `now` until a light can next change** at `pace`, the
    /// bound the driver's poll takes; none while every light holds.
    pub(crate) fn wait(&self, now: HostTime, pace: Pace) -> Option<Duration> {
        self.activity
            .next_change(now, pace)
            .and_then(|t| t.since(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_render::Light;

    const MS: Duration = Duration::from_millis(1);

    fn at(after: Duration) -> HostTime {
        HostTime::ZERO.plus(Duration::from_secs(10) + after)
    }

    /// **The group in front names OH's job**: the shell's own pid is the
    /// shell at its prompt, another group a job, and no group or no shell
    /// nobody.
    /// **The Linux driver feeds, schedules and lays out the line**, held in
    /// its source since the driver runs only under Wayland: each read
    /// stamps RD and HS, each write SD and CS, every turn steps the lights
    /// before the frame, the wait is bounded by the next change, the grid is
    /// laid out from the window's height less the line, and the setting's
    /// reload gives the window the line's pixels. Perturbation: take
    /// `lights` out of the wait's bounds -- a light would stay lit until the
    /// next event -- and this goes red.
    #[test]
    fn the_linux_driver_feeds_schedules_and_lays_out_the_line() {
        const SRC: &str = include_str!("linux.rs");
        for want in [
            "self.lights.read(bytes, now);",
            "self.lights.answered(now);",
            "self.lights.wrote(wrote, now);",
            "d.tick_lights();\n        d.present();",
            "report, lights,\n        ]",
            "let lights = d.lights_wait(now);",
            "let rows = (text_h / m.cell_h.value().max(1))",
            ".saturating_sub(self.renderer.lights_height().value());",
            ".set_lights_line(self.activity_lights.line(), scale.factor() as f32);",
            "self.lights_line(was);",
            "self.renderer.set_lights(shown, glow);",
        ] {
            assert_eq!(SRC.matches(want).count(), 1, "{want}");
        }
    }

    /// **The states are sampled at most once a frame**: the first turn
    /// samples, a turn within 16.7 ms does not, and one after does -- so a
    /// flood of reads makes one `ioctl` a frame, not one a read.
    /// Perturbation: answer `Now` every turn -- the reads' pace again --
    /// and this goes red.
    #[test]
    fn the_states_are_sampled_once_a_frame_not_once_a_read() {
        let mut l = LineLights::new();
        assert_eq!(l.due(at(Duration::ZERO)), Sampled::Now);
        for us in [1, 4_000, 16_666] {
            assert_eq!(
                l.due(at(Duration::from_micros(us))),
                Sampled::Recently,
                "{us}"
            );
        }
        assert_eq!(l.due(at(SAMPLE)), Sampled::Now);
        assert_eq!(l.due(at(SAMPLE + MS)), Sampled::Recently);
    }

    #[test]
    fn a_group_other_than_the_shells_is_a_job_in_front() {
        assert_eq!(in_front(Some(200), Some(200)), InFront::Shell);
        assert_eq!(in_front(Some(311), Some(200)), InFront::Job);
        assert_eq!(in_front(None, Some(200)), InFront::Nobody);
        assert_eq!(in_front(Some(311), None), InFront::Nobody);
    }

    /// **Each stamp lights its own light, a step owes a frame only when the
    /// line moved, and the wait ends when a light goes dark**: a read lights
    /// RD, one at the ceiling HS too, a reply AA, a write SD and a refused
    /// one CS, a job in front OH; the step that shows them owes a frame,
    /// the next with nothing new does not; the wait is the hold, and at its
    /// end the lights go dark and owe one more; with two lights held to
    /// different ends, the wait is the sooner. Perturbation: wake for the
    /// latest change rather than the soonest (`Activity::next_change`) --
    /// SD would stay lit until RD's hold ends -- and this goes red.
    #[test]
    fn each_stamp_lights_its_light_and_the_wait_ends_its_hold() {
        let mut l = LineLights::new();
        let full = vec![0u8; baud_render::READ_CEILING];
        l.read(b"x", at(Duration::ZERO));
        l.read(&full, at(Duration::ZERO));
        l.read(&[], at(Duration::ZERO));
        l.answered(at(Duration::ZERO));
        l.wrote(Wrote::Part, at(Duration::ZERO));
        let mut term_owned = baud_core::Owned::new(&baud_core::WinSize {
            ws_col: 10,
            ws_row: 2,
        });
        let term = baud_core::Term::new(
            term_owned.storage(),
            &baud_core::WinSize {
                ws_col: 10,
                ws_row: 2,
            },
        );
        l.states(InFront::Job, &term, at(Duration::ZERO));
        let (shown, _) = l
            .step(at(Duration::ZERO), Pace::Full, Glow::Full)
            .expect("a frame owed");
        for led in [Led::Rd, Led::Hs, Led::Aa, Led::Sd, Led::Cs, Led::Oh] {
            assert_eq!(*shown.get(led), Light::Lit, "{led:?}");
        }
        for led in [Led::Cd, Led::Fax, Led::Syn] {
            assert_eq!(*shown.get(led), Light::Dark, "{led:?}");
        }
        assert_eq!(
            l.step(at(MS * 50), Pace::Full, Glow::Full),
            None,
            "nothing moved"
        );
        assert_eq!(
            l.step(at(MS * 50), Pace::Full, Glow::Dimmed)
                .map(|(_, g)| g),
            Some(Glow::Dimmed)
        );
        let wait = l.wait(at(MS * 50), Pace::Full).expect("a light to go dark");
        assert_eq!(wait, MS * 150, "the hold's end");
        let (shown, _) = l
            .step(at(MS * 200), Pace::Full, Glow::Dimmed)
            .expect("dark now");
        assert_eq!(shown, Shown::DARK);
        assert_eq!(
            l.wait(at(MS * 200), Pace::Full),
            Some(Duration::from_millis(200)),
            "the dwell"
        );
        assert_eq!(l.step(at(MS * 400), Pace::Full, Glow::Dimmed), None);
        assert_eq!(l.wait(at(MS * 400), Pace::Full), None, "every light holds");
        // A line drawn afresh owes a frame though nothing moved.
        l.stale();
        assert_eq!(
            l.step(at(MS * 450), Pace::Full, Glow::Dimmed),
            Some((Shown::DARK, Glow::Dimmed))
        );
        // The soonest light to change bounds the wait, not the last: SD
        // seen at 0 goes dark at 200, RD seen again at 120 at 320.
        let mut l = LineLights::new();
        l.wrote(Wrote::All, at(Duration::ZERO));
        l.read(b"x", at(Duration::ZERO));
        let _ = l.step(at(Duration::ZERO), Pace::Full, Glow::Full);
        l.read(b"x", at(MS * 120));
        assert_eq!(l.step(at(MS * 150), Pace::Full, Glow::Full), None);
        assert_eq!(l.wait(at(MS * 150), Pace::Full), Some(MS * 50), "SD's hold");
        // A write the kernel took whole is SD alone; one it took none of
        // is CS alone.
        let mut l = LineLights::new();
        l.wrote(Wrote::All, at(Duration::ZERO));
        let (s, _) = l.step(at(Duration::ZERO), Pace::Full, Glow::Full).unwrap();
        assert_eq!((s.sd, s.cs), (Light::Lit, Light::Dark));
        let mut l = LineLights::new();
        l.wrote(Wrote::Nothing, at(Duration::ZERO));
        let (s, _) = l.step(at(Duration::ZERO), Pace::Full, Glow::Full).unwrap();
        assert_eq!((s.sd, s.cs), (Light::Dark, Light::Lit));
    }
}
