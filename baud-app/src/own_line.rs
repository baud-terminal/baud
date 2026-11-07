//! **The activity lights on a line of their own, fed by a driver with no
//! display link**: the Linux driver's, whose window has no title bar, so
//! the lights' own line in the frame is its default (`[window]
//! activity_lights`, `baud_render::lights`).
//!
//! The stamps are the macOS driver's (`lights.rs` there): RD each read of
//! the program's output and HS each one as long as the platform's tty hands
//! back (`baud_render::lights::READ_CEILING`, Linux's 4095), AA a reply the
//! core queued while it read that output, SD a write the kernel took any of
//! and CS dark for one it refused any of and while bytes are owed, SYN and
//! FAX from the core's state, and OH
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
//!
//! **DEC's indicator line's OH is the same read** ([`FrontRead`]), shared
//! with the lights at one `ioctl` a frame (`line_lamps.rs`). There the
//! wait is bounded while the line shows the words, since 070-14 gives an
//! indicator 80 ms and a job that prints nothing makes no turn: the group
//! is read again 80 ms less three frames after the last read
//! ([`FrontRead::line_wait`]), every 30 ms at 60 Hz.

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

/// **What who is in front shows as DEC's line's OH**: lit for a job, dark
/// for the shell at its prompt and for nobody.
pub(crate) fn lamp(front: InFront) -> baud_core::ReadLamp {
    use baud_core::{LampState, ReadLamp};
    match front {
        InFront::Job => ReadLamp::Read(LampState::Lit),
        InFront::Shell | InFront::Nobody => ReadLamp::Read(LampState::Dark),
    }
}

/// **The last read of who is in front, and when**: one read of the
/// terminal's foreground group for the lights on their line and DEC's
/// line's words alike, at most once every [`SAMPLE`].
pub(crate) struct FrontRead {
    last: Option<(HostTime, InFront)>,
}

impl FrontRead {
    pub(crate) fn new() -> FrontRead {
        FrontRead { last: None }
    }

    /// **Who is in front at `now`**: `read` asked the first time and once
    /// [`SAMPLE`] has gone by since it last was, else its last answer.
    pub(crate) fn at(&mut self, now: HostTime, read: impl FnOnce() -> InFront) -> InFront {
        match self.last {
            Some((at, front)) if now < at.plus(SAMPLE) => front,
            _ => {
                let front = read();
                self.last = Some((now, front));
                front
            }
        }
    }

    /// **How long from `now` the wait may last while DEC's line shows the
    /// words**, the display refreshing every `frame`: until the group is
    /// read again 070-14's 80 ms less three frames after the last read --
    /// a frame already in flight, the one that draws the word and the
    /// refresh that shows it -- and never sooner than [`SAMPLE`] after it,
    /// so the loop does not spin. Below 37.5 Hz that is every [`SAMPLE`]
    /// and the bound is not met. None before a first read.
    pub(crate) fn line_wait(&self, now: HostTime, frame: Duration) -> Option<Duration> {
        let (at, _) = self.last?;
        let every = crate::line_lamps::BOUND
            .saturating_sub(frame * 3)
            .max(SAMPLE);
        Some(at.plus(every).since(now).unwrap_or(Duration::ZERO))
    }
}

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
    synced: crate::bell::Synced,
}

impl LineLights {
    pub(crate) fn new() -> LineLights {
        LineLights {
            activity: Activity::new(),
            shown: None,
            sampled: None,
            synced: crate::bell::Synced::new(),
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
    /// dark for any it refused.
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
    /// program holds synchronized output open or opened it since the last
    /// turn ([`crate::bell::Synced`]), FAX while a picture is placed, and
    /// CS dark while `outbox` holds bytes the kernel has not taken.
    pub(crate) fn states(
        &mut self,
        front: InFront,
        term: &baud_core::Term<'_>,
        outbox: &crate::pty::Outbox,
        now: HostTime,
    ) {
        if front == InFront::Job {
            self.activity.seen(Led::Oh, now);
        }
        crate::line_lamps::feed_cs(&mut self.activity, outbox, now);
        if self.synced.seen(term) {
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

    /// **A turn with the lights off their line**: SYN's reading taken all
    /// the same, so the line shown again lights nothing opened while it was
    /// away, as the macOS tick takes it.
    pub(crate) fn away(&mut self, term: &baud_core::Term<'_>) {
        let _ = self.synced.seen(term);
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
    use crate::pty::Outbox;
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
    /// stamps RD and HS, each write SD and a refused one CS, each sampled
    /// turn the states and CS from the outbox, every turn steps the lights
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
            "self.lights.states(front, &self.term, &self.outbox, now);",
            "d.flush_reply();\n        d.tick_lights();\n",
            "report, lights, words,\n        ]",
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

    /// **DEC's line's OH is lit for a job and dark otherwise, from one
    /// read a frame**: a job in front reads lit, the shell and nobody
    /// dark, never unknown; a second ask within [`SAMPLE`] is answered
    /// from the last read, one after reads again. Perturbation: read on
    /// every ask -- an `ioctl` a read of output again -- and this goes red.
    #[test]
    fn dec_lines_oh_is_who_is_in_front_read_once_a_frame() {
        use baud_core::{LampState, ReadLamp};
        assert_eq!(lamp(InFront::Job), ReadLamp::Read(LampState::Lit));
        assert_eq!(lamp(InFront::Shell), ReadLamp::Read(LampState::Dark));
        assert_eq!(lamp(InFront::Nobody), ReadLamp::Read(LampState::Dark));
        let mut f = FrontRead::new();
        let mut reads = 0;
        let mut read = |who| {
            reads += 1;
            who
        };
        assert_eq!(
            f.at(at(Duration::ZERO), || read(InFront::Shell)),
            InFront::Shell
        );
        assert_eq!(f.at(at(MS * 16), || read(InFront::Job)), InFront::Shell);
        assert_eq!(f.at(at(SAMPLE), || read(InFront::Job)), InFront::Job);
        assert_eq!(reads, 2, "one read a frame");
    }

    /// **While DEC's line shows the words, a job that prints nothing is
    /// drawn within 80 ms** (070-14 p. 14-6): the wait ends 80 ms less
    /// three frames after the last read of the group, and the job that
    /// takes the terminal just after it is read at that wake, leaving a
    /// frame in flight, the one that draws the word and the refresh that
    /// shows it. At 60 Hz the wait is 30 ms and at 120 Hz 55; below 37.5
    /// Hz it is a 60 Hz frame, so the loop never spins, and the bound is
    /// not met. Nothing is waited for before a first read. Perturbation:
    /// leave the three frames out -- the wake at 80 ms -- and this goes
    /// red.
    #[test]
    fn a_silent_jobs_oh_is_drawn_within_80_ms() {
        let us = Duration::from_micros;
        assert_eq!(
            FrontRead::new().line_wait(at(Duration::ZERO), us(16_667)),
            None
        );
        for frame in [us(16_667), us(8_333), us(6_944), us(11_111)] {
            let mut f = FrontRead::new();
            f.at(at(Duration::ZERO), || InFront::Shell);
            let joined = us(1);
            let wait = f.line_wait(at(Duration::ZERO), frame).expect("a bound");
            assert!(wait >= SAMPLE, "{frame:?}: {wait:?}");
            assert_eq!(
                f.at(at(wait), || InFront::Job),
                InFront::Job,
                "read at the wake"
            );
            let shown = wait + frame * 3 - joined;
            assert!(shown <= crate::line_lamps::BOUND, "{frame:?}: {shown:?}");
            assert!(
                shown > crate::line_lamps::BOUND - frame,
                "{frame:?}: read too often"
            );
            assert_eq!(f.line_wait(at(wait + MS), frame), Some(wait - MS));
            assert_eq!(
                f.line_wait(at(wait * 3), frame),
                Some(Duration::ZERO),
                "past due"
            );
        }
        let mut f = FrontRead::new();
        f.at(at(Duration::ZERO), || InFront::Shell);
        assert_eq!(
            f.line_wait(at(Duration::ZERO), us(16_667)),
            Some(us(29_999))
        );
        assert_eq!(f.line_wait(at(Duration::ZERO), us(8_333)), Some(us(55_001)));
        assert_eq!(f.line_wait(at(Duration::ZERO), us(33_333)), Some(SAMPLE));
    }

    /// **Each stamp lights its own light, a step owes a frame only when the
    /// line moved, and the wait ends when a light goes back to rest**: a
    /// read lights RD, one at the ceiling HS too, a reply AA, a write SD, a
    /// job in front OH, and a refused write darkens CS, lit while the line
    /// is clear; the step that shows them owes a frame, the next with
    /// nothing new does not; the wait is the hold, and at its end the
    /// lights are at rest, CS lit again, and owe one more; with two lights
    /// held to different ends, the wait is the sooner. Bytes owed at a
    /// turn keep CS dark, and an empty outbox leaves it lit. Perturbation: wake for the
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
        l.states(InFront::Job, &term, &Outbox::default(), at(Duration::ZERO));
        let (shown, _) = l
            .step(at(Duration::ZERO), Pace::Full, Glow::Full)
            .expect("a frame owed");
        for led in [Led::Rd, Led::Hs, Led::Aa, Led::Sd, Led::Oh] {
            assert_eq!(*shown.get(led), Light::Lit, "{led:?}");
        }
        for led in [Led::Cd, Led::Cs, Led::Fax, Led::Syn] {
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
            .expect("at rest now");
        assert_eq!(shown, Shown::rest());
        assert_eq!(shown.cs, Light::Lit, "the line clear again");
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
            Some((Shown::rest(), Glow::Dimmed))
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
        // A write the kernel took whole lights SD and leaves CS lit; one
        // it took none of leaves SD dark and darkens CS.
        let mut l = LineLights::new();
        l.wrote(Wrote::All, at(Duration::ZERO));
        let (s, _) = l.step(at(Duration::ZERO), Pace::Full, Glow::Full).unwrap();
        assert_eq!((s.sd, s.cs), (Light::Lit, Light::Lit));
        let mut l = LineLights::new();
        l.wrote(Wrote::Nothing, at(Duration::ZERO));
        let (s, _) = l.step(at(Duration::ZERO), Pace::Full, Glow::Full).unwrap();
        assert_eq!((s.sd, s.cs), (Light::Dark, Light::Dark));
        // Bytes still owed at each turn keep CS dark past the hold; the
        // outbox emptied, it lights once the hold is over.
        let mut owed = Outbox::default();
        owed.push(b"x");
        for m in [16, 100, 190, 300] {
            l.states(InFront::Shell, &term, &owed, at(MS * m));
            let _ = l.step(at(MS * m), Pace::Full, Glow::Full);
            assert_eq!(l.activity.shown().cs, Light::Dark, "owed at {m}");
        }
        for m in [316, 499] {
            l.states(InFront::Shell, &term, &Outbox::default(), at(MS * m));
            let _ = l.step(at(MS * m), Pace::Full, Glow::Full);
        }
        assert_eq!(
            l.activity.shown().cs,
            Light::Dark,
            "the hold after the last owed"
        );
        let _ = l.step(at(MS * 500), Pace::Full, Glow::Full);
        assert_eq!(l.activity.shown().cs, Light::Lit, "clear");
    }

    /// **An update opened and closed between two samplings lights SYN**:
    /// `?2026h`, a frame and `?2026l` in one read leave the mode reset at
    /// both samplings, as btop's frames through ssh do, and the opening
    /// read since the last lights SYN all the same. The control: two
    /// samplings with no update between them, the first a fresh
    /// terminal's, leave it dark. Perturbations: SYN from the mode alone
    /// (the update missed) and the first reading counted as an opening
    /// (lit with none) each go red.
    #[test]
    fn an_update_between_two_samplings_lights_syn() {
        let grid = baud_core::WinSize {
            ws_col: 10,
            ws_row: 2,
        };
        let mut owned = baud_core::Owned::new(&grid);
        let mut term = baud_core::Term::new(owned.storage(), &grid);
        let mut l = LineLights::new();
        let syn = |l: &mut LineLights, t: Duration| {
            l.stale();
            let (shown, _) = l.step(at(t), Pace::Full, Glow::Full).expect("a frame");
            *shown.get(Led::Syn)
        };
        l.states(
            InFront::Shell,
            &term,
            &Outbox::default(),
            at(Duration::ZERO),
        );
        l.states(InFront::Shell, &term, &Outbox::default(), at(SAMPLE));
        assert_eq!(syn(&mut l, SAMPLE), Light::Dark, "no update, no SYN");
        crate::reads::feed(
            &mut term,
            [&b"\x1b[?2026hframe\x1b[?2026l"[..]],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert!(!term.synchronized_output(), "closed by the sampling");
        l.states(InFront::Shell, &term, &Outbox::default(), at(SAMPLE * 2));
        assert_eq!(syn(&mut l, SAMPLE * 2), Light::Lit, "opened since");
        let mut fresh = crate::bell::Synced::new();
        assert!(!fresh.seen(&term), "a first reading is the baseline");
        assert!(!fresh.seen(&term), "and nothing opened since it");
    }

    /// **An update while the line was away lights nothing when it is back**:
    /// the turns with the lights off their line take SYN's reading, so the
    /// line shown again starts from it.
    #[test]
    fn an_update_while_the_line_was_away_lights_nothing() {
        let grid = baud_core::WinSize {
            ws_col: 10,
            ws_row: 2,
        };
        let mut owned = baud_core::Owned::new(&grid);
        let mut term = baud_core::Term::new(owned.storage(), &grid);
        let mut l = LineLights::new();
        l.states(
            InFront::Shell,
            &term,
            &Outbox::default(),
            at(Duration::ZERO),
        );
        l.states(InFront::Shell, &term, &Outbox::default(), at(SAMPLE));
        crate::reads::feed(
            &mut term,
            [&b"\x1b[?2026hframe\x1b[?2026l"[..]],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        l.away(&term);
        l.states(InFront::Shell, &term, &Outbox::default(), at(SAMPLE * 2));
        l.stale();
        let (shown, _) = l
            .step(at(SAMPLE * 2), Pace::Full, Glow::Full)
            .expect("a frame");
        assert_eq!(*shown.get(Led::Syn), Light::Dark, "opened while away");
    }
}
