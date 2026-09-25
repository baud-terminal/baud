//! A visual bell no frame drew, shown for as long as the entry asks.
//!
//! **`tput flash` sends both halves in one read.** Under `xterm-256color` it
//! writes `\E[?5h`, waits the `$<100/>` the entry's `flash` names, and writes
//! `\E[?5l` -- and on macOS `tput` buffers across the wait, so the two arrive
//! together (a pty capture, 2026-09-25: 115.6 ms after it starts, both at
//! once). A frame after that read finds `?5` reset, as it was before, and the
//! reversed screen existed for no frame at all. xterm repaints inside each
//! flip (`ReverseVideo` calls `xtermRepaint`, in `[[source: xterm-scroll]]`'s
//! file), so both of its frames go to the display however close together they
//! come; baud draws frames, and nothing drew one between.
//!
//! **The core counts the flips and this reads the count.** A frame that finds
//! [`Term::reverse_flips`] moved since the last drawn frame, with `?5` reading
//! as it did then, knows the other video came and went unseen, and shows it:
//! the renderer's [`VideoFlash`], the other video, not reverse video, so a
//! flash over a reversed screen shows it normal. Its end is a deadline the
//! loop's own clock reaches -- the display link's timestamp on macOS, the
//! `poll` timeout on Linux -- so nothing sleeps, no frame is held, and output
//! keeps drawing under the flash. It costs two frames, its start and its end.
//!
//! **A synchronised update is honoured, and not trusted.** The core counts a
//! change made inside `?2026` apart ([`Term::reverse_flips_held`]). The driver
//! times each update from its opening ([`UpdateClock`], restarted by
//! [`Term::updates_opened`] so a close and a reopen in one read is a new
//! update), and while it holds frames for one it absorbs that count
//! ([`Bell::hold`]), so a pair the application hid stays hidden. A frame the
//! driver draws because the update open at that frame outlived its bound
//! ([`Update::Outlived`], which the driver says and this does not guess)
//! compares the held count too -- so a `tput flash` after a program died
//! mid-update still flashes. A pair made in an outlived update that closes
//! before the next frame is not shown: that frame reads the update closed.
//!
//! **The length is the entry's own**, read at build time from the pinned
//! file the child is told to use, so the number cannot be one thing in the
//! environment and another here: [`TERM`] is the one spelling of both. A
//! `flash` the reader does not understand stops the build.
//!
//! The rules, each a test below:
//!
//! * `?5` reads otherwise than it did: the frame draws the mode, and a flash
//!   in progress is over, whatever the counts say;
//! * `?5` reads as it did and a count the frame can show moved: a flash,
//!   started or restarted from now;
//! * neither: a flash in progress runs to its deadline.
//!
//! What the reading is compared against is the last *drawn* frame's. A frame
//! put off -- the GPU still busy (`Drawn::Busy`), no free set -- changes
//! nothing, and the next asks again; a frame held for an update changes the
//! held count only ([`Bell::hold`]). A frame whose present failed
//! ([`Bell::refused`]) is dropped, as the macOS driver drops any, so it takes
//! the reading and keeps a flash only where the screen shows one and the frame
//! would have kept it: a flash that never reached the screen is lost with the
//! frame, and an end that never did is drawn by the next frame drawn for any
//! reason, not forced again.

use core::time::Duration;
use std::time::Instant;

use baud_core::{ReverseFlips, Term, UpdatesOpened};
use baud_domain::HostTime;
use baud_domain::terminfo::{self, Flash};
use baud_render::VideoFlash;

/// The entry the child is told to use, spelled once for the environment and
/// for the file its flash is read from.
macro_rules! term_name {
    () => {
        "xterm-256color"
    };
}

/// `TERM`, for the child's environment (`pty.rs`).
pub const TERM: &str = term_name!();

/// The pinned entry, as `infocmp` wrote it.
const ENTRY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../docs/research/sources/terminfo/",
    term_name!(),
    ".ti"
));

/// How long a flash is shown: the entry's padding between `\E[?5h` and
/// `\E[?5l`, read at compile time. Anything else the entry could say stops
/// the build rather than guessing a length.
pub const LENGTH: Duration = match terminfo::flash(ENTRY) {
    Ok(Flash::Padded(p)) => p.delay.duration(),
    Ok(Flash::Unpadded) => panic!("the entry's flash has no padding: no length to show it for"),
    Ok(Flash::Absent) => panic!("the entry has no flash"),
    Err(_) => panic!("the entry's flash is not DECSCNM set, held and reset"),
};

/// `?5` as the frame reads it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Reverse,
}

impl Mode {
    pub fn of(term: &Term<'_>) -> Mode {
        if term.reverse_video() {
            Mode::Reverse
        } else {
            Mode::Normal
        }
    }
}

/// A clock a flash or an update can be timed on: the display link's host
/// time on macOS, an `Instant` on Linux.
pub trait Moment: Copy + Ord {
    fn after(self, d: Duration) -> Self;
    /// How long after `earlier` this is, or zero when it is not after it.
    fn elapsed_since(self, earlier: Self) -> Duration;
}

impl Moment for HostTime {
    fn after(self, d: Duration) -> HostTime {
        self.plus(d)
    }

    fn elapsed_since(self, earlier: HostTime) -> Duration {
        self.since(earlier).unwrap_or(Duration::ZERO)
    }
}

impl Moment for Instant {
    fn after(self, d: Duration) -> Instant {
        self + d
    }

    fn elapsed_since(self, earlier: Instant) -> Duration {
        self.saturating_duration_since(earlier)
    }
}

/// Whether a synchronised update is open, and if it is, whether the driver
/// still honours it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Update {
    /// No update is open.
    Closed,
    /// One is open and younger than the driver's bound: its frames are held.
    Honoured,
    /// One is open and older than the bound: the driver draws through it, and
    /// what it held is on screen.
    Outlived,
}

/// How long the update open now has been open, timed from its opening. A
/// close and a reopen in one read is a new update ([`Term::updates_opened`]),
/// and it is timed from when the clock first sees it, not from the update
/// before it.
pub struct UpdateClock<T> {
    opened: Option<(UpdatesOpened, T)>,
    max: Duration,
}

impl<T: Moment> UpdateClock<T> {
    /// A clock that honours an update for `max` from its opening.
    pub fn new(max: Duration) -> UpdateClock<T> {
        UpdateClock { opened: None, max }
    }

    /// What the update is at `now`, starting the clock on an opening it has
    /// not seen and forgetting one that closed. The driver's own hold check
    /// calls this after every read.
    pub fn observe(&mut self, term: &Term<'_>, now: T) -> Update {
        if !term.synchronized_output() {
            self.opened = None;
            return Update::Closed;
        }
        let n = term.updates_opened();
        if self.opened.is_none_or(|(m, _)| m != n) {
            self.opened = Some((n, now));
        }
        self.state(term, now)
    }

    /// The same answer without recording anything, for a frame drawn outside
    /// the loop's hold check -- a resize, a zoom. An opening the clock has not
    /// seen is younger than anything it has, so it is honoured.
    pub fn state(&self, term: &Term<'_>, now: T) -> Update {
        if !term.synchronized_output() {
            return Update::Closed;
        }
        match self.opened {
            Some((m, start)) if m == term.updates_opened() => {
                if now.elapsed_since(start) < self.max {
                    Update::Honoured
                } else {
                    Update::Outlived
                }
            }
            _ => Update::Honoured,
        }
    }

    /// When the update the clock is timing outlives its bound, for a loop
    /// that waits on a clock. The macOS driver ticks on the display link
    /// instead, so only the Linux `poll` timeout reads this.
    #[cfg(any(target_os = "linux", test))]
    pub fn deadline(&self) -> Option<T> {
        self.opened.map(|(_, start)| start.after(self.max))
    }
}

/// What a drawn frame read of the screen: both counts, and the mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Seen {
    shown: ReverseFlips,
    held: ReverseFlips,
    mode: Mode,
}

impl Seen {
    fn of(term: &Term<'_>) -> Seen {
        Seen {
            shown: term.reverse_flips(),
            held: term.reverse_flips_held(),
            mode: Mode::of(term),
        }
    }
}

/// The bell: the reading the last drawn frame made, and the deadline of a
/// flash on screen.
pub struct Bell<T> {
    seen: Seen,
    until: Option<T>,
    length: Duration,
}

/// What one frame shows, decided before it is built and kept by
/// [`Bell::commit`] if it is drawn, or in part by [`Bell::refused`] if its
/// present failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plan<T> {
    pub flash: VideoFlash,
    until: Option<T>,
    seen: Seen,
}

impl<T: Moment> Bell<T> {
    /// A bell whose first baseline is the screen as it stands, and whose
    /// flash lasts `length` -- [`LENGTH`] in a driver, anything in a test.
    pub fn new(term: &Term<'_>, length: Duration) -> Bell<T> {
        Bell {
            seen: Seen::of(term),
            until: None,
            length,
        }
    }

    /// What the frame about to be built shows, if it is shown at `now`,
    /// through `update` as the driver reads it. Changes nothing: a frame put
    /// off leaves the baseline where it was, so the next one asks the same
    /// question again.
    pub fn plan(&self, term: &Term<'_>, now: T, update: Update) -> Plan<T> {
        let seen = Seen::of(term);
        // Only a frame drawn through an outlived update shows what the update
        // held; any other either holds it or reads it closed.
        let held_shown = update == Update::Outlived && seen.held != self.seen.held;
        let until = if seen.mode != self.seen.mode {
            None
        } else if seen.shown != self.seen.shown || held_shown {
            Some(now.after(self.length))
        } else {
            self.until.filter(|&u| now < u)
        };
        Plan {
            flash: match until {
                Some(_) => VideoFlash::On,
                None => VideoFlash::Off,
            },
            until,
            seen,
        }
    }

    /// The frame `plan` was for was committed: its reading is the baseline
    /// now, and its flash, if any, runs to its deadline.
    pub fn commit(&mut self, plan: Plan<T>) {
        self.seen = plan.seen;
        self.until = plan.until;
    }

    /// The frame `plan` was for was built and its present failed, so it is
    /// dropped, as the macOS driver drops any frame whose present fails: the
    /// reading is the baseline now, and a flash stays only if one is on screen
    /// and this frame would have kept it. A flash it would have started is
    /// lost with it. An end it would have drawn -- or a change of mode, which
    /// makes a flash on screen mean the other video -- clears the deadline, so
    /// nothing forces the frame again while presents keep failing; the next
    /// frame drawn for any reason draws the video as it is. The Linux driver
    /// keeps a failed frame owed and draws it again, so there it is put off
    /// rather than dropped, and this has no caller.
    #[cfg(any(target_os = "macos", test))]
    pub fn refused(&mut self, plan: Plan<T>) {
        self.seen = plan.seen;
        self.until = self.until.and(plan.until);
    }

    /// The driver is holding frames for an open update: what the update
    /// changes is the application's to hide, so its count joins the baseline
    /// as it arrives. A driver that has stopped holding does not call this,
    /// and its next frame shows what the update held.
    pub fn hold(&mut self, term: &Term<'_>) {
        self.seen.held = term.reverse_flips_held();
    }

    /// When a flash on screen ends, for a loop that waits on a clock.
    pub fn deadline(&self) -> Option<T> {
        self.until
    }

    /// Whether a flash on screen has reached its end at `now`, so the loop
    /// owes a frame to take it down.
    pub fn is_due(&self, now: T) -> bool {
        self.deadline().is_some_and(|u| now >= u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 4,
        ws_col: 20,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    const MS: Duration = Duration::from_millis(1);

    /// A moment `d` after the clock's zero.
    fn at(d: Duration) -> HostTime {
        HostTime::ZERO.plus(d)
    }

    const LEN: Duration = Duration::from_millis(100);

    /// A frame at `now` with no update open, committed.
    fn draw(b: &mut Bell<HostTime>, t: &Term<'_>, now: HostTime) -> VideoFlash {
        draw_through(b, t, now, Update::Closed)
    }

    /// A frame at `now` through `update` as the driver reads it, committed.
    fn draw_through(
        b: &mut Bell<HostTime>,
        t: &Term<'_>,
        now: HostTime,
        update: Update,
    ) -> VideoFlash {
        let p = b.plan(t, now, update);
        b.commit(p);
        p.flash
    }

    #[test]
    fn the_length_is_the_pinned_entrys() {
        assert_eq!(TERM, "xterm-256color");
        assert_eq!(LENGTH, Duration::from_millis(100));
    }

    #[test]
    fn a_pair_in_one_read_flashes_until_its_deadline() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        assert_eq!(
            draw(&mut b, &t, at(Duration::ZERO)),
            VideoFlash::Off,
            "the control"
        );
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(MS * 10)), VideoFlash::On, "the start");
        assert_eq!(b.deadline(), Some(at(MS * 110)));
        assert!(!b.is_due(at(MS * 109)));
        feed(&mut t, b"output under the flash");
        assert_eq!(
            draw(&mut b, &t, at(MS * 50)),
            VideoFlash::On,
            "held while output draws"
        );
        assert!(b.is_due(at(MS * 110)), "due at its deadline");
        assert_eq!(draw(&mut b, &t, at(MS * 110)), VideoFlash::Off, "the end");
        assert_eq!(b.deadline(), None);
        assert!(!b.is_due(at(MS * 500)), "and nothing more is owed");
    }

    /// Halves drawn by separate frames were seen, so there is nothing to
    /// flash: the frames drew the mode.
    #[test]
    fn halves_drawn_apart_do_not_flash() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::Off);
        feed(&mut t, b"\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(MS * 100)), VideoFlash::Off);
    }

    /// The other direction: over a reversed screen, `?5l?5h` in one read
    /// flashes it normal.
    #[test]
    fn a_pair_over_a_reversed_screen_flashes_it_normal() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::Off);
        feed(&mut t, b"\x1b[?5l\x1b[?5h");
        assert_eq!(draw(&mut b, &t, at(MS * 10)), VideoFlash::On);
    }

    /// A real flip ends a flash: the mode is what the frame draws.
    #[test]
    fn a_flip_cancels_a_flash() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::On);
        feed(&mut t, b"\x1b[?5h");
        assert_eq!(
            draw(&mut b, &t, at(MS * 20)),
            VideoFlash::Off,
            "the mode, reversed"
        );
        assert_eq!(b.deadline(), None, "nothing is owed");
    }

    /// A second pair while one is showing restarts it from the second.
    #[test]
    fn a_second_pair_restarts_the_flash() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        draw(&mut b, &t, at(Duration::ZERO));
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(MS * 60)), VideoFlash::On);
        assert_eq!(b.deadline(), Some(at(MS * 160)), "from the second pair");
    }

    /// A frame that was not committed leaves the baseline, so the next one
    /// still sees the pair -- and starts the flash from when it is drawn.
    #[test]
    fn an_uncommitted_frame_moves_nothing() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        let refused = b.plan(&t, at(Duration::ZERO), Update::Closed);
        assert_eq!(refused.flash, VideoFlash::On);
        assert_eq!(b.deadline(), None, "plan changed nothing");
        assert_eq!(
            draw(&mut b, &t, at(MS * 8)),
            VideoFlash::On,
            "the next frame"
        );
        assert_eq!(b.deadline(), Some(at(MS * 108)));
    }

    /// Inside `?2026` the application asked for the frames between not to
    /// be seen, and the frame after the update does not look at what it
    /// held, so there is no flash -- even with the update opened and closed
    /// in the one read, so no pump ever held for it.
    #[test]
    fn a_pair_inside_a_synchronised_update_does_not_flash() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?2026h\x1b[?5h\x1b[?5l\x1b[?2026l");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::Off);
        // And the frame after absorbed the held count: a later frame drawn
        // inside an update left open does not show it either.
        feed(&mut t, b"\x1b[?2026h");
        assert_eq!(
            draw_through(&mut b, &t, at(MS * 50), Update::Outlived),
            VideoFlash::Off
        );
    }

    /// **An update left open is honoured, and then not trusted.** While the
    /// driver holds, a pair inside is the application's to hide; once it has
    /// stopped holding and draws with the update still open, a pair that
    /// arrives -- `tput flash` after a program died mid-update -- flashes.
    #[test]
    fn a_pair_inside_an_update_left_open_flashes_once_the_hold_is_over() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?2026h\x1b[?5h\x1b[?5l");
        b.hold(&t);
        assert_eq!(
            draw_through(&mut b, &t, at(MS * 40), Update::Outlived),
            VideoFlash::Off,
            "hidden while held"
        );
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(
            draw_through(&mut b, &t, at(MS * 60), Update::Outlived),
            VideoFlash::On,
            "the hold is over and the update is still open"
        );
        assert!(t.synchronized_output(), "the control: it never closed");
    }

    /// A frame drawn during an update the driver still honours -- a resize or
    /// a zoom draws one outside the hold check -- does not show what the
    /// update held, whatever the pump has absorbed so far.
    #[test]
    fn a_frame_drawn_through_an_honoured_update_hides_what_it_held() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?2026h\x1b[?5h\x1b[?5l");
        assert_eq!(
            draw_through(&mut b, &t, at(MS * 5), Update::Honoured),
            VideoFlash::Off
        );
    }

    /// A change of mode ends a flash whatever the counts say. First the
    /// change is made inside an update that closed, so no count the frame
    /// compares moved; then inside an outlived update, so the held count it
    /// compares did move -- a rule that read the counts first would start a
    /// flash there, against the reverse video the application asked for.
    #[test]
    fn a_change_of_mode_ends_a_flash_whatever_the_counts_say() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::On);
        feed(&mut t, b"\x1b[?2026h\x1b[?5h\x1b[?2026l");
        assert_eq!(
            draw(&mut b, &t, at(MS * 20)),
            VideoFlash::Off,
            "reverse, as asked"
        );
        assert_eq!(b.deadline(), None);

        feed(&mut t, b"\x1b[?2026h\x1b[?5l");
        assert_eq!(
            draw_through(&mut b, &t, at(MS * 80), Update::Outlived),
            VideoFlash::Off,
            "normal, as asked, with the held count moved and compared"
        );
    }

    /// A frame whose present failed is dropped: it takes the reading, loses
    /// a flash it would have started, and forces nothing again -- an end it
    /// would have drawn is drawn by the next frame drawn for any reason, and
    /// a tick does not ask for it while presents keep failing.
    #[test]
    fn a_refused_frame_loses_a_start_and_forces_no_end() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        let start = b.plan(&t, at(Duration::ZERO), Update::Closed);
        assert_eq!(start.flash, VideoFlash::On);
        b.refused(start);
        assert_eq!(b.deadline(), None, "nothing is on screen to end");
        assert_eq!(
            draw(&mut b, &t, at(MS * 10)),
            VideoFlash::Off,
            "the pair is spent"
        );

        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(MS * 20)), VideoFlash::On);
        let end = b.plan(&t, at(MS * 120), Update::Closed);
        assert_eq!(end.flash, VideoFlash::Off);
        b.refused(end);
        assert!(!b.is_due(at(MS * 128)), "no tick forces it again");
        assert_eq!(
            draw(&mut b, &t, at(MS * 500)),
            VideoFlash::Off,
            "the next frame drawn draws the mode"
        );
    }

    /// A refused frame that carried a change of mode ends the flash too: a
    /// flash is the other video *of the mode*, so one kept across a change
    /// would draw the opposite of what the application just set.
    #[test]
    fn a_refused_change_of_mode_ends_the_flash() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(draw(&mut b, &t, at(Duration::ZERO)), VideoFlash::On);
        feed(&mut t, b"\x1b[?5h");
        let flip = b.plan(&t, at(MS * 20), Update::Closed);
        assert_eq!(flip.flash, VideoFlash::Off);
        b.refused(flip);
        assert_eq!(b.deadline(), None);
        assert_eq!(
            draw(&mut b, &t, at(MS * 30)),
            VideoFlash::Off,
            "reverse video, as set"
        );
    }

    /// The update clock: honoured from its opening for its bound, outlived
    /// after, forgotten on a close, and restarted by a close and a reopen
    /// the driver never saw apart.
    #[test]
    fn the_update_clock_times_each_update_from_its_opening() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut c: UpdateClock<HostTime> = UpdateClock::new(MS * 33);
        assert_eq!(c.observe(&t, at(Duration::ZERO)), Update::Closed);
        assert_eq!(c.deadline(), None);
        feed(&mut t, b"\x1b[?2026h");
        assert_eq!(c.observe(&t, at(MS * 10)), Update::Honoured);
        assert_eq!(c.deadline(), Some(at(MS * 43)));
        assert_eq!(c.observe(&t, at(MS * 42)), Update::Honoured);
        assert_eq!(c.observe(&t, at(MS * 43)), Update::Outlived);
        feed(&mut t, b"\x1b[?2026l\x1b[?2026h");
        assert_eq!(
            c.state(&t, at(MS * 50)),
            Update::Honoured,
            "an opening the clock has not seen is fresh"
        );
        assert_eq!(c.observe(&t, at(MS * 50)), Update::Honoured, "restarted");
        assert_eq!(c.deadline(), Some(at(MS * 83)));
        feed(&mut t, b"\x1b[?2026l");
        assert_eq!(c.observe(&t, at(MS * 60)), Update::Closed);
        assert_eq!(c.deadline(), None, "forgotten");
    }

    /// The same machine on the Linux clock.
    #[test]
    fn it_runs_on_an_instant_too() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b: Bell<Instant> = Bell::new(&t, LEN);
        let start = Instant::now();
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        let p = b.plan(&t, start, Update::Closed);
        b.commit(p);
        assert_eq!(p.flash, VideoFlash::On);
        assert_eq!(b.deadline(), Some(start + LEN));
        assert!(b.is_due(start + LEN));
    }
}
