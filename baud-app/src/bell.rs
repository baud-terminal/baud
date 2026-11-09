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
//! times each hold from the opening that began it ([`UpdateClock`]: a close
//! and a reopen in one read is a new update, [`Term::updates_opened`], with
//! its own bound after a drawn frame and the hold's while frames are held),
//! and while it holds frames for one it absorbs that count
//! ([`Bell::hold`]), so a pair the application hid stays hidden. A frame the
//! driver draws because the hold at that frame outlived its bound
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
//! * neither: a flash in progress runs to its deadline;
//! * **and every change of the whole screen a frame draws -- the mode's, a
//!   flash's, a `BEL`'s -- shares one budget**, four in any second, so no
//!   second shows more than three flashes however fast a program toggles
//!   `?5` (`baud_render::bell`, WCAG 2.3.1). A change back toward the mode
//!   past it is held, the frame drawing the other video, and drawn on a
//!   deadline at most a second later; a flash starts only with room for its
//!   return. The core's `?5` and `DECRQM`'s answer change at once.
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
//!
//! **A `BEL` rings here too**, as the system sound and a Dock bounce:
//! `NSBeep` while the window is focused; while it is not, `?1042` asks for
//! the user's attention -- the Dock icon bounces -- and `?1043` raises the
//! window; the config's `bell` picks the sound, the visual bell or nothing.
//! [`ring`] is that decision, a pure function of the config, the motion
//! preference, the window and the two modes, and the macOS driver's shim
//! makes the AppKit calls it names. The Linux driver asks it too, its window
//! focused while the keyboard is its: the sound is the compositor's system
//! bell where it offers `xdg-system-bell-v1`, and nothing where it does not.
//! There the two modes are one request, `xdg-activation-v1`'s `activate`
//! with a token the window asks for, as foot marks a window urgent
//! (`[[source: foot-2705e36-wayland]]` 2630-2658): Wayland has no request
//! that tells urgency from a raise, and the compositor decides what it shows
//! (`[[source: xdg-activation-v1]]`). [`Ringer`] reads the bells the
//! terminal counted at each volume ([`Term::bells_rung`]) after each read
//! and rings once for the bells of one frame interval. A flash it decides is
//! started through [`Bell::ring`], so it is planned, committed and refused
//! as a `?5` pair's is.

use core::time::Duration;
use std::time::Instant;

#[cfg(any(target_os = "macos", target_os = "linux", test))]
use baud_core::{BellIsUrgent, BellVolume, BellsRung, PopOnBell};
use baud_core::{ReverseFlips, Term, UpdatesOpened};
use baud_domain::HostTime;
use baud_domain::terminfo::{self, Flash};
use baud_render::{
    BellKind, BellTint, PairFlash, Rang, ScreenFlash, Video, VideoFlash, VideoMoved,
};

/// The entry the child is told to use, spelled once for the environment and
/// for the file its flash is read from.
macro_rules! term_name {
    () => {
        "xterm-256color"
    };
}

/// `TERM`, for the child's environment (`pty.rs`).
pub const TERM: &str = term_name!();

/// The pinned entry, as `infocmp` wrote it, read through
/// `baud-app/terminfo/`, whose link to `docs/research/sources/terminfo/`'s
/// copy the published crate carries as the file.
const ENTRY: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/terminfo/",
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

    /// The video the mode names.
    fn video(self) -> Video {
        match self {
            Mode::Normal => Video::Normal,
            Mode::Reverse => Video::Reverse,
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
    /// One is open and its hold younger than the driver's bound: its frames
    /// are held.
    Honoured,
    /// One is open and its hold older than the bound: the driver draws
    /// through it, and what it held is on screen.
    Outlived,
}

/// **Whether a sampling of the activity lights sees a synchronised
/// update**, for SYN: open now, or opened since the last sampling
/// ([`Term::updates_opened`]). A program behind ssh sends its frames in
/// reads microseconds apart, so `?2026h` and `?2026l` can both arrive
/// between two samplings a frame apart, and the mode alone, read at each,
/// is reset at both. The first reading is only the baseline: an update
/// before it is not counted.
pub(crate) struct Synced {
    opened: Option<UpdatesOpened>,
}

impl Synced {
    pub(crate) fn new() -> Synced {
        Synced { opened: None }
    }

    /// **SYN at this sampling of `term`**: the update open now, or one
    /// opened since the last reading, which this one replaces.
    pub(crate) fn seen(&mut self, term: &Term<'_>) -> bool {
        let opened = term.updates_opened();
        let since = self.opened.is_some_and(|was| was != opened);
        self.opened = Some(opened);
        term.synchronized_output() || since
    }
}

/// How long the frames held now have been held, timed from the opening of
/// the update that began the hold. A close and a reopen in one read is a
/// new update ([`Term::updates_opened`]). Found after the clock last
/// answered Closed or Outlived, an answer that lets the driver draw (the
/// update before it closed, or outlived its bound), it is timed from when
/// the clock first sees it, so it has the whole bound. Found while frames are held, it keeps the
/// hold's start: the close in that read was owed a frame, "implicitly
/// updating the render state" (`[[source: contour-synchronized-output]]`),
/// and a chain of updates, each opened in the read that closed the last,
/// is held for one bound, not for as long as it runs.
pub struct UpdateClock<T> {
    opened: Option<(UpdatesOpened, T)>,
    /// What the hold check was last answered.
    last: Update,
    max: Duration,
}

impl<T: Moment> UpdateClock<T> {
    /// A clock that holds frames for `max` from the opening that began the
    /// hold.
    pub fn new(max: Duration) -> UpdateClock<T> {
        UpdateClock {
            opened: None,
            last: Update::Closed,
            max,
        }
    }

    /// What the update is at `now`, starting the clock on an opening it has
    /// not seen unless frames are held, and forgetting one that closed. The
    /// driver's own hold check calls this after every read.
    pub fn observe(&mut self, term: &Term<'_>, now: T) -> Update {
        if !term.synchronized_output() {
            self.opened = None;
            self.last = Update::Closed;
            return Update::Closed;
        }
        self.opened = Some(self.timed(term.updates_opened(), now));
        self.last = self.state(term, now);
        self.last
    }

    /// Opening `n`, with the moment its hold is timed from at `now`: the
    /// hold's start while the clock holds frames or `n` is the opening it
    /// times, else `now`.
    fn timed(&self, n: UpdatesOpened, now: T) -> (UpdatesOpened, T) {
        match self.opened {
            Some((m, start)) if m == n || self.last == Update::Honoured => (n, start),
            _ => (n, now),
        }
    }

    /// The same answer without recording anything, for a frame drawn outside
    /// the loop's hold check -- a resize, a zoom. An opening the clock has not
    /// seen is timed as the hold check will time it: fresh, or from the
    /// hold's start while frames are held.
    pub fn state(&self, term: &Term<'_>, now: T) -> Update {
        if !term.synchronized_output() {
            return Update::Closed;
        }
        let (_, start) = self.timed(term.updates_opened(), now);
        if now.elapsed_since(start) < self.max {
            Update::Honoured
        } else {
            Update::Outlived
        }
    }

    /// When the hold the clock is timing outlives its bound, for a loop
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
    rung: Rung,
}

impl Seen {
    fn of(term: &Term<'_>, rung: Rung) -> Seen {
        Seen {
            shown: term.reverse_flips(),
            held: term.reverse_flips_held(),
            mode: Mode::of(term),
            rung,
        }
    }
}

/// How many flashes a `BEL` has asked for ([`Bell::ring`]): a count the
/// frame compares, as it compares the `?5` counts, so a ring that lands
/// between a frame's plan and its commit is still seen by the next frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rung(u32);

impl Rung {
    const FIRST: Rung = Rung(0);

    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    fn next(self) -> Rung {
        Rung(self.0.wrapping_add(1))
    }
}

/// The bell: the reading the last drawn frame made, the deadline of a
/// `?5` pair's flash on screen, and the flash a `BEL` rang with the
/// budget every change of the whole screen shares ([`ScreenFlash`]),
/// timed from the first frame planned.
pub struct Bell<T> {
    seen: Seen,
    until: Option<T>,
    length: Duration,
    /// The flashes asked for so far, of which `seen.rung` the last drawn
    /// frame had shown.
    rung: Rung,
    /// A `BEL`'s flash, in the colour a program set or the other video,
    /// and the video drawn, held to WCAG 2.3.1's rate together
    /// (`baud_render::bell`).
    screen: ScreenFlash,
    /// The moment `screen`'s clock counts from, the first frame planned,
    /// and the last drawn frame on it.
    origin: Option<T>,
    drawn: Duration,
}

/// What one frame shows, decided before it is built and kept by
/// [`Bell::commit`] if it is drawn, or in part by [`Bell::refused`] if its
/// present failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Plan<T> {
    /// The other video: a `?5` pair's, a `BEL`'s with no colour set, or
    /// the video a change the budget holds leaves on screen.
    pub flash: VideoFlash,
    /// A `BEL`'s flash in the colour a program set: the solid the frame
    /// draws over the screen ([`baud_render::Renderer::set_bell_tint`]).
    ///
    /// Never a `?5` pair's, which is reverse video and not the bell.
    pub tint: Option<BellTint>,
    until: Option<T>,
    seen: Seen,
    screen: ScreenFlash,
    origin: T,
    at: Duration,
}

impl<T: Moment> Bell<T> {
    /// A bell whose first baseline is the screen as it stands, and whose
    /// flash lasts `length` -- [`LENGTH`] in a driver, anything in a test.
    pub fn new(term: &Term<'_>, length: Duration) -> Bell<T> {
        Bell {
            seen: Seen::of(term, Rung::FIRST),
            until: None,
            length,
            rung: Rung::FIRST,
            screen: ScreenFlash::new(length, Mode::of(term).video()),
            origin: None,
            drawn: Duration::ZERO,
        }
    }

    /// What the frame about to be built shows, if it is shown at `now`,
    /// through `update` as the driver reads it. Changes nothing: a frame put
    /// off leaves the baseline where it was, so the next one asks the same
    /// question again.
    pub fn plan(&self, term: &Term<'_>, now: T, update: Update) -> Plan<T> {
        let seen = Seen::of(term, self.rung);
        // Only a frame drawn through an outlived update shows what the update
        // held; any other either holds it or reads it closed.
        let held_shown = update == Update::Outlived && seen.held != self.seen.held;
        let moved = seen.mode != self.seen.mode;
        let until = if moved {
            None
        } else if seen.shown != self.seen.shown || held_shown {
            Some(now.after(self.length))
        } else {
            self.until.filter(|&u| now < u)
        };
        // **A `BEL`'s flash is its own** (`baud_render::bell`): not screen
        // content, so an update hides none and it is shown by the first
        // frame drawn after it, held or not; in the colour a program set,
        // else as the other video, which a change of `?5` in the same frame
        // ends or refuses, as it ends a pair's, since the mode is what a
        // frame draws. A tint is not of the video and runs on. **The video
        // and the tint share one budget**, at most four changes in a
        // second, so whatever mix of `?5`, pairs and bells a program
        // drives, the screen flashes no faster than WCAG 2.3.1 has it.
        let origin = self.origin.unwrap_or(now);
        let at = now.elapsed_since(origin).max(self.drawn);
        let mode = seen.mode.video();
        let (screen, shows) = self.screen.at(
            at,
            mode,
            if moved {
                VideoMoved::Yes
            } else {
                VideoMoved::No
            },
            if until.is_some() {
                PairFlash::On
            } else {
                PairFlash::Off
            },
            if seen.rung == self.seen.rung {
                Rang::No
            } else {
                Rang::Yes
            },
            BellKind::of(term),
        );
        let flash = if shows.video == mode {
            VideoFlash::Off
        } else {
            VideoFlash::On
        };
        Plan {
            flash,
            tint: shows.tint,
            until,
            seen,
            screen,
            origin,
            at,
        }
    }

    /// The frame `plan` was for was committed: its reading is the baseline
    /// now, and its flash, if any, runs to its deadline.
    pub fn commit(&mut self, plan: Plan<T>) {
        self.seen = plan.seen;
        self.until = plan.until;
        self.screen = plan.screen;
        self.origin = Some(plan.origin);
        self.drawn = plan.at;
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
        self.origin = Some(plan.origin);
        self.drawn = plan.at;
        // A `BEL`'s flash the frame would have started is lost with it, and
        // starts no quiet: nothing was shown. One on screen runs on as the
        // frame left it, and the video drawn is the last shown.
        self.screen = self.screen.refused(plan.screen);
    }

    /// The driver is holding frames for an open update: what the update
    /// changes is the application's to hide, so its count joins the baseline
    /// as it arrives. A driver that has stopped holding does not call this,
    /// and its next frame shows what the update held.
    pub fn hold(&mut self, term: &Term<'_>) {
        self.seen.held = term.reverse_flips_held();
    }

    /// A `BEL` asked for the visual bell ([`ring`]): the next frame drawn
    /// shows it, from when it is drawn, unless that frame finds `?5` changed
    /// -- the mode is what a frame draws, and a flash of it would draw the
    /// opposite. Several rings before a frame are one flash.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn ring(&mut self) {
        self.rung = self.rung.next();
    }

    /// **A copy's pulse asks for its two changes**, from `now` for `length`
    /// (`copied.rs`): true where the budget every change of the whole screen
    /// shares has room for both, which it then counts, so a person's copies
    /// and a program's flashes together stay inside WCAG 2.3.1's rate
    /// (`baud_render::bell`); false, and the pulse does not run.
    #[cfg(any(target_os = "macos", target_os = "linux", test))]
    pub fn pulse(&mut self, now: T, length: Duration) -> bool {
        let origin = *self.origin.get_or_insert(now);
        let at = now.elapsed_since(origin).max(self.drawn);
        match self.screen.spend_pair(at, length) {
            Some(screen) => {
                self.screen = screen;
                true
            }
            None => false,
        }
    }

    /// When the next frame the bell owes is due, for a loop that waits on
    /// a clock: a flash's end, the moment the budget allows a change it
    /// holds, or while a `BEL`'s tint fades, the last drawn frame's moment
    /// -- one each frame interval, paced by the display link on macOS and
    /// the compositor's frame events on Linux, until it is gone.
    pub fn deadline(&self) -> Option<T> {
        let origin = self.origin?;
        let rang = match self.screen.lit() {
            Some(BellKind::Tint(_)) => Some(origin.after(self.drawn)),
            Some(BellKind::Video) => self.screen.end().map(|e| origin.after(e)),
            None => None,
        };
        let owed = self.screen.owed().map(|o| origin.after(o));
        [self.until, rang, owed].into_iter().flatten().min()
    }

    /// Whether a flash on screen has reached its end at `now`, so the loop
    /// owes a frame to take it down.
    pub fn is_due(&self, now: T) -> bool {
        self.deadline().is_some_and(|u| now >= u)
    }
}

/// The window as a `BEL` finds it. `Minimized` is unfocused too, and the
/// one state a raise must leave alone: `[[source: xterm-man]]` on
/// `popOnBell`, "If the window is iconified, this has no effect."
/// xdg-shell tells a Linux client nothing of a minimized window, so there
/// a window is focused or not.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    Focused,
    Unfocused,
    #[cfg_attr(all(target_os = "linux", not(test)), allow(dead_code))]
    Minimized,
}

/// Whether AppKit has the window as the key window: "A Boolean value that
/// indicates whether the window is the key window for the application"
/// (`[[source: apple-nswindow-iskeywindow]]`).
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Key,
    NotKey,
}

/// Whether AppKit has the window minimized: "A Boolean value that
/// indicates whether the window is minimized" (`[[source:
/// apple-nswindow-isminiaturized]]`).
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Miniaturized {
    Minimized,
    NotMinimized,
}

#[cfg(any(target_os = "macos", test))]
impl Window {
    /// **The window from AppKit's two answers**, so the shim only asks.
    /// The key window is the focused one, minimized or not -- AppKit does
    /// not make a minimized window key, and a window the user is typing in
    /// is the one the decision calls focused. Not key, minimized is
    /// [`Window::Minimized`]; not key and not minimized is
    /// [`Window::Unfocused`] -- **a hidden application's window too**
    /// (Cmd-H), which neither answer tells apart, so `?1043` asks it to the
    /// front: the open counterpart of xterm's "If the window is iconified,
    /// this has no effect" (`[[source: xterm-man]]`).
    pub fn of(key: Key, mini: Miniaturized) -> Window {
        match (key, mini) {
            (Key::Key, Miniaturized::NotMinimized) => Window::Focused,
            (Key::Key, Miniaturized::Minimized) => Window::Focused,
            (Key::NotKey, Miniaturized::Minimized) => Window::Minimized,
            (Key::NotKey, Miniaturized::NotMinimized) => Window::Unfocused,
        }
    }
}

/// Whether the ring plays the system's alert sound, `NSBeep`: "Plays the
/// system beep. Users can select a sound to be played as the system beep"
/// (`[[source: apple-nsbeep]]`).
///
/// **Low plays it too**, as high does: `NSBeep` takes no volume, NSSound's
/// own documentation sends the system beep to it, "If you want to play the
/// system beep sound, use the NSBeep ... function"
/// (`[[source: apple-nssound]]`), and the system sound services play "at
/// the current system audio volume, with no programmatic volume control
/// available" (`[[source: apple-audioservicesplaysystemsound]]`). No
/// documented API names the user's alert sound or plays it below the
/// user's alert volume, so on macOS low and high sound alike, an open gap;
/// off is silence.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    Silent,
    Beep,
}

/// Whether the ring shows the visual bell ([`Bell::ring`]).
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Nothing,
    Flash,
}

/// Whether the ring asks for the user's attention: `?1042`, "Enable Urgency
/// window manager hint" (`[[source: ctlseqs]]`), which on macOS is the
/// application's `requestUserAttention` of the informational kind: "The dock
/// icon will bounce for one second. The request, though, remains active
/// until either the app becomes active or the request is canceled"
/// (`[[source: apple-nsapplication-informationalrequest]]`). "Sending
/// requestUserAttention(_:) to an app that is already active has no effect"
/// (`[[source: apple-nsapplication-requestuserattention]]`), so a window that
/// is not key while baud is active asks and nothing bounces.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Attention {
    None,
    Request,
}

/// Whether the ring raises the window: `?1043`, "Enable raising of the
/// window" (`[[source: ctlseqs]]`). xterm's raise is `XRaiseWindow`
/// (`Bell`, `[[source: xterm-misc]]`), a change of stacking that asks for no
/// focus, so the shim calls `orderFrontRegardless`, which "Moves the window
/// to the front of its level, even if its application isn't active, without
/// changing either the key window or the main window" (`[[source:
/// apple-nswindow-orderfrontregardless]]`).
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Raise {
    Stay,
    Front,
}

/// What one ring does: each of the four, decided apart.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ring {
    pub sound: Sound,
    pub shown: Shown,
    pub attention: Attention,
    pub raise: Raise,
}

/// **What a `BEL` does**: "NSBeep when focused; ?1042 bounces the Dock
/// and ?1043 raises the window when unfocused; config bell =
/// sound|flash|none".
///
/// * Focused: the config's `bell` -- the sound, the visual bell, or nothing.
///   Neither mode acts: the window is the one the user is in.
/// * Not focused: `?1042` asks for attention, as xterm sets its urgency hint
///   only `if (enable && !(screen->select & FOCUS))` (`setXUrgency`,
///   `[[source: xterm-misc]]`); `?1043` raises the window, unless it is
///   minimized. **No sound**: `NSBeep` sounds only in the focused window;
///   xterm rings whatever the focus, which is the alternative. A flash is
///   drawn whatever the focus, as xterm's `VisualBell` is, since the
///   decision limits only the sound to a focused window -- **but not in a
///   minimized one**, which "is removed from the screen"
///   (`[[source: apple-nswindow-isminiaturized]]`): its flash would wait for
///   the next drawn frame and show on the restore, long after the bell.
/// * **The program's volume silences the sound alone**: DECSWBV's for a
///   `BEL`, DECSMBV's for the margin bell. Off, nothing is heard, and the
///   flash, the attention request and the raise act as they would, as
///   xterm's volume reaches only `xtermBell` and its `Bell` sets the
///   urgency, shows the visual bell and raises whatever it says
///   (`[[source: xterm-misc]]`). The config is the user's and the volume
///   the program's: a `none` stays silent whatever the volume.
/// * The config does not silence the modes: an application that set one is
///   told by `DECRQM` that it is set, so a `none` that dropped it would be a
///   mode recorded and not acted on. xterm sets the hint and raises whatever
///   its bell's volume or `visualBell` says (`Bell`, `[[source:
///   xterm-misc]]`).
/// * **Reduce Motion: no flash.** Apple's HIG (`[[source:
///   apple-hig-accessibility]]`): "Be cautious with fast-moving and blinking
///   animations ... When this setting is active, ensure your app or game
///   responds by reducing automatic and repetitive animations", and a flash
///   inverts the screen and puts it back whenever a program writes a `BEL`.
///   Nothing takes its place, as nothing takes blinking text's: the user
///   picked no sound.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub fn ring(
    style: crate::config::BellStyle,
    motion: crate::config::SystemMotion,
    window: Window,
    urgent: BellIsUrgent,
    pop: PopOnBell,
    volume: BellVolume,
) -> Ring {
    use crate::config::{BellStyle, SystemMotion};
    let focused = window == Window::Focused;
    let sound = match (style, focused, volume) {
        (BellStyle::Sound, true, BellVolume::Low | BellVolume::High) => Sound::Beep,
        _ => Sound::Silent,
    };
    let shown = match (style, motion, window) {
        (BellStyle::Flash, SystemMotion::Full, Window::Focused | Window::Unfocused) => Shown::Flash,
        _ => Shown::Nothing,
    };
    let attention = match (urgent, focused) {
        (BellIsUrgent::On, false) => Attention::Request,
        _ => Attention::None,
    };
    let raise = match (pop, window) {
        (PopOnBell::On, Window::Unfocused) => Raise::Front,
        _ => Raise::Stay,
    };
    Ring {
        sound,
        shown,
        attention,
        raise,
    }
}

/// How many `BEL`s and margin bells the terminal had counted at each
/// volume when the ringer last looked: the core counts each at the volume
/// it rang at, so a read that changes the volume around a bell sounds it
/// at its own.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Heard {
    bells: BellsRung,
    margin: BellsRung,
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
impl Heard {
    fn of(term: &Term<'_>) -> Heard {
        Heard {
            bells: term.bells_rung(),
            margin: term.margin_bells_rung(),
        }
    }
}

/// Whether the bells a read brought ring, and how loud.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rings {
    /// No new bell, or only bells inside the frame interval of the last ring.
    Quiet,
    /// The loudest volume a bell rang at: DECSWBV's when a `BEL` rang,
    /// DECSMBV's when the margin bell did.
    Ring(BellVolume),
}

/// **Bells within one frame interval ring once.** A `BEL` and the margin
/// bell are one bell here, as xterm rings both through its `Bell`
/// (`[[source: xterm-misc]]`), each at its own volume. The driver asks after
/// every read; the bells ring if the count moved and the last ring is at
/// least a frame interval old, and bells inside that interval are absorbed,
/// not put off -- so `yes $'\a'` rings at most once a frame, and a lone bell
/// rings on the read that brought it, drawn or not: a sound, a bounce and a
/// raise are not frames, and a minimized window draws none.
///
/// **DEC STD 070 governs it, and this departs from it**: "Closely spaced
/// bell codes will be sounded individually at a rate of 4-5 per second.
/// As bell characters arrive, they are stored in a bell buffer until they
/// can be sounded. The bell buffer will have 10 entries" (070-6 § 6.6.5.1,
/// p. 6-45, `[[source: dec-std-070]]`), and the rest are discarded. One
/// ring a frame is an open departure from that rate and that buffer, found
/// after it was built. xterm suppresses for
/// `bellSuppressTime`, "Number of milliseconds after a bell command is sent
/// during which additional bells will be suppressed. Default is 200", and
/// until the server reports the first bell done (`[[source: xterm-man]]`;
/// `Bell`, `[[source: xterm-misc]]`). One frame is the bound built, and
/// xterm's 200 ms the open alternative. What Terminal.app does is not
/// known: its source is not published, no documentation of its bell is
/// cited here, and the rules forbid driving a GUI to find out.
#[cfg(any(target_os = "macos", target_os = "linux", test))]
pub struct Ringer<T> {
    heard: Heard,
    last: Option<T>,
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
impl<T: Moment> Ringer<T> {
    /// A ringer that has heard every bell the terminal has counted so far.
    pub fn new(term: &Term<'_>) -> Ringer<T> {
        Ringer {
            heard: Heard::of(term),
            last: None,
        }
    }

    /// Whether the bells since the last call ring at `now`, a frame being
    /// `frame` long.
    pub fn hear(&mut self, term: &Term<'_>, now: T, frame: Duration) -> Rings {
        let heard = Heard::of(term);
        if heard == self.heard {
            return Rings::Quiet;
        }
        let loudest = heard
            .bells
            .loudest_since(self.heard.bells)
            .max(heard.margin.loudest_since(self.heard.margin));
        self.heard = heard;
        if self.last.is_some_and(|l| now.elapsed_since(l) < frame) {
            return Rings::Quiet;
        }
        self.last = Some(now);
        Rings::Ring(loudest.unwrap_or(BellVolume::Off))
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

    /// **A copy's pulse shares the flash budget**: two pulses take its four
    /// changes, the third in that second is refused and spends nothing, and a
    /// second later a pulse runs again. Perturbation: return true from
    /// `Bell::pulse` with no check, and the third runs.
    #[test]
    fn a_pulse_shares_the_flash_budget() {
        let mut owned = Owned::new(&WS);
        let term = Term::new(owned.storage(), &WS);
        let mut b: Bell<HostTime> = Bell::new(&term, LEN);
        let len = Duration::from_millis(150);
        assert!(b.pulse(at(MS), len), "the first");
        assert!(b.pulse(at(MS * 200), len), "the second");
        assert!(!b.pulse(at(MS * 400), len), "the third in that second");
        assert!(
            !b.pulse(at(MS * 600), len),
            "and it spent nothing to wait on"
        );
        assert!(b.pulse(at(MS * 1500), len), "a second on");
    }

    /// **The Linux driver rings as [`ring`] decides, and only so**: its
    /// body compared whole with the spaces taken out, so a sound outside
    /// its condition, the flash on the sound's, the window state swapped,
    /// the config's style or the motion preference not passed, or an
    /// activation on neither mode's condition each change the text; and
    /// the bells are heard after the read that brought them. The window is
    /// focused while the keyboard is the window's, the sound is the
    /// compositor's system bell, and `?1042`'s attention and `?1043`'s
    /// raise are one activation asked of the compositor.
    #[test]
    fn the_linux_driver_rings_as_ring_decides() {
        let src = include_str!("linux.rs");
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let body = src
            .split_once("fn ring_bell(&mut self, volume: baud_core::BellVolume) {")
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| b)
            .expect("the ring");
        const RING: &str = r#"
            use crate::bell::{Attention, Raise, Shown, Sound, Window};
            let state = match self.focus {
                Focus::Gained => Window::Focused,
                Focus::Lost => Window::Unfocused,
            };
            let ring = crate::bell::ring(
                self.bell_style, self.motion, state,
                self.term.bell_is_urgent(), self.term.pop_on_bell(), volume,
            );
            if ring.sound == Sound::Beep {
                let _ = self.window.ring_bell(&mut self.conn.sender());
            }
            if ring.shown == Shown::Flash {
                self.bell.ring();
                self.pending = Pending::Dirty;
            }
            if ring.attention == Attention::Request || ring.raise == Raise::Front {
                let _ = self.window.activate(&mut self.conn.sender());
            }
        "#;
        assert_eq!(bare(body), bare(RING), "the ring is this, and only this");
        let feed = src
            .split_once("fn feed(&mut self, bytes: &[u8], tag: Taken) -> Vec<Taken> {")
            .map(|(_, rest)| rest)
            .expect("the feed");
        let read = feed.find("crate::reads::paced(").expect("the read");
        let heard = feed
            .find("self.ringer.hear(&self.term, Instant::now(), interval)")
            .expect("the bells heard");
        let rung = feed.find("self.ring_bell(volume);").expect("and rung");
        assert!(read < heard && heard < rung, "the read, then the ring");
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

    /// A frame at `now`, committed: both halves of what it draws.
    fn drawn(b: &mut Bell<HostTime>, t: &Term<'_>, now: HostTime) -> (VideoFlash, Option<u8>) {
        let p = b.plan(t, now, Update::Closed);
        b.commit(p);
        (p.flash, p.tint.map(|k| k.alpha.get()))
    }

    /// **A `BEL` with a colour set draws the colour and not the other
    /// video**: kitty's fade, owed a frame each interval while it runs and
    /// none once it is over.
    #[test]
    fn a_ring_in_a_colour_draws_the_tint_and_not_the_other_video() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b]21;visual_bell=#ff0000\x07");
        assert_eq!(
            drawn(&mut b, &t, at(Duration::ZERO)),
            (VideoFlash::Off, None)
        );
        b.ring();
        assert_eq!(drawn(&mut b, &t, at(MS * 5)), (VideoFlash::Off, Some(0)));
        assert!(b.is_due(at(MS * 5)), "the fade owes the next frame");
        assert_eq!(drawn(&mut b, &t, at(MS * 55)), (VideoFlash::Off, Some(153)));
        assert!(b.is_due(at(MS * 70)));
        assert_eq!(drawn(&mut b, &t, at(MS * 105)), (VideoFlash::Off, None));
        assert_eq!(b.deadline(), None, "nothing more is owed");
    }

    /// **A `?5` pair is reverse video, not the bell**: with a colour set it
    /// still draws the other video, and a `BEL` in the same frame draws its
    /// colour over it.
    #[test]
    fn a_pair_is_the_other_video_whatever_the_bell_colour() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b]21;visual_bell=#ff0000\x07");
        assert_eq!(
            drawn(&mut b, &t, at(Duration::ZERO)),
            (VideoFlash::Off, None)
        );
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        assert_eq!(drawn(&mut b, &t, at(MS * 5)), (VideoFlash::On, None));
        feed(&mut t, b"\x1b[?5h\x1b[?5l");
        b.ring();
        let (flash, tint) = drawn(&mut b, &t, at(MS * 60));
        assert_eq!(flash, VideoFlash::On, "the pair's");
        assert!(tint.is_some(), "and the bell's colour");
    }

    /// **With no colour, or the automatic one, a `BEL` draws the other
    /// video**, as before a colour could be set.
    #[test]
    fn an_automatic_ring_draws_the_other_video() {
        for set in [
            &b""[..],
            b"\x1b]21;visual_bell=\x07",
            b"\x1b]21;visual_bell=red;visual_bell\x07",
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            feed(&mut t, set);
            drawn(&mut b, &t, at(Duration::ZERO));
            b.ring();
            assert_eq!(drawn(&mut b, &t, at(MS * 5)), (VideoFlash::On, None));
            assert_eq!(b.deadline(), Some(at(MS * 105)));
        }
    }

    /// **A change of `?5` ends the other video and not a tint**, which is
    /// not of the video.
    #[test]
    fn a_change_of_mode_ends_no_tint() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b]21;visual_bell=#00ff00\x07");
        drawn(&mut b, &t, at(Duration::ZERO));
        b.ring();
        drawn(&mut b, &t, at(MS * 5));
        feed(&mut t, b"\x1b[?5h");
        assert_eq!(drawn(&mut b, &t, at(MS * 55)), (VideoFlash::Off, Some(153)));
    }

    /// **Bells rung faster than WCAG 2.3.1 allows flash no faster**: a
    /// `BEL` each frame holds one flash; one each 200 ms, xterm's
    /// `bellSuppressTime`, flashes once each 600 ms, the rings inside the
    /// quiet absorbed -- for the other video and for a colour alike.
    #[test]
    fn bells_rung_fast_flash_at_most_twice_a_second() {
        for set in [&b""[..], b"\x1b]21;visual_bell=#ffffff\x07"] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            feed(&mut t, set);
            drawn(&mut b, &t, at(Duration::ZERO));
            let mut lit = vec![];
            for f in 1..120u32 {
                if f % 12 == 0 {
                    b.ring();
                }
                let (v, k) = drawn(&mut b, &t, at(Duration::from_micros(u64::from(f) * 16_667)));
                lit.push(v == VideoFlash::On || k.is_some_and(|a| a > 0));
            }
            let starts = lit.windows(2).filter(|w| !w[0] && w[1]).count();
            assert_eq!(starts, 3, "rings at 0.2 s, 0.8 s and 1.4 s: {lit:?}");
            let (mut owned, mut lit) = (Owned::new(&WS), vec![]);
            let t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            for f in 0..60u32 {
                b.ring();
                let (v, _) = drawn(&mut b, &t, at(Duration::from_micros(u64::from(f) * 16_667)));
                lit.push(v == VideoFlash::On);
            }
            assert!(lit.iter().all(|&l| l), "a ring a frame holds one flash");
        }
    }

    /// A sixtieth of a second, a frame.
    const FRAME: Duration = Duration::from_micros(16_667);

    /// The video a committed frame drew: the mode, or under a flash the
    /// other one.
    fn video(t: &Term<'_>, flash: VideoFlash) -> Mode {
        match (Mode::of(t), flash) {
            (m, VideoFlash::Off) => m,
            (Mode::Normal, VideoFlash::On) => Mode::Reverse,
            (Mode::Reverse, VideoFlash::On) => Mode::Normal,
        }
    }

    /// The frames, of `seen` -- the video before the first frame, then one
    /// a frame -- on which the drawn video changed.
    fn changes(seen: &[Mode]) -> Vec<usize> {
        (1..seen.len())
            .filter(|&i| seen[i] != seen[i - 1])
            .collect()
    }

    /// **No 60 frames hold more than four changes**: a second holds at
    /// most three flashes on the chained reading, every adjacent opposing
    /// pair a flash, and two on the disjoint one (`baud_bench::flash`).
    fn at_most_four_a_second(at: &[usize], what: &str) {
        eprintln!("{what}: changes at {at:?}");
        for w in at.windows(5) {
            assert!(w[4] - w[0] >= 60, "{what}: five changes at {w:?} of {at:?}");
        }
    }

    /// **A program's `?5` toggled at 10 Hz is drawn at most four changes a
    /// second**, each change in a read of its own, 50 ms apart, for two
    /// seconds, with a frame every sixtieth: three spellings of the flip,
    /// `DECSCNM` alone, against `RIS` and against XTRESTORE, which all
    /// flip through the one path. The mode is set at once, so every read
    /// leaves `?5` as written, and once the toggles stop the screen draws
    /// the mode they left.
    #[test]
    fn reverse_video_toggled_at_10_hz_draws_at_most_four_changes_a_second() {
        for (name, set, reset) in [
            ("DECSCNM", &b"\x1b[?5h"[..], &b"\x1b[?5l"[..]),
            ("RIS", b"\x1b[?5h", b"\x1bc"),
            ("XTRESTORE", b"\x1b[?5h", b"\x1b[?5r"),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"\x1b[?5s");
            let mut b = Bell::new(&t, LEN);
            let mut seen = vec![Mode::of(&t)];
            for f in 0..240u32 {
                if f < 120 && f.is_multiple_of(3) {
                    let on = (f / 3).is_multiple_of(2);
                    feed(&mut t, if on { set } else { reset });
                    let want = if on { Mode::Reverse } else { Mode::Normal };
                    assert_eq!(Mode::of(&t), want, "{name} {f}: the mode at once");
                }
                seen.push(video(&t, draw(&mut b, &t, at(FRAME * f))));
            }
            let at = changes(&seen);
            assert!(at.len() >= 4, "{name}: the toggles are drawn: {at:?}");
            at_most_four_a_second(&at, name);
            assert_eq!(seen[240], Mode::of(&t), "{name}: the mode it ends in");
        }
    }

    /// **A `tput flash` every 200 ms is held the same way**: each pair in
    /// one read is the other video's flash, and the flashes are drawn at
    /// most four changes a second, as many pairs as fit shown whole.
    #[test]
    fn pairs_every_200_ms_draw_at_most_four_changes_a_second() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        let mut seen = vec![Mode::of(&t)];
        for f in 0..240u32 {
            if f < 120 && f.is_multiple_of(12) {
                feed(&mut t, b"\x1b[?5h\x1b[?5l");
            }
            seen.push(video(&t, draw(&mut b, &t, at(FRAME * f))));
        }
        let at = changes(&seen);
        assert!(at.len() >= 4, "the first flashes are shown: {at:?}");
        at_most_four_a_second(&at, "a pair every 200 ms");
        assert!(at.len().is_multiple_of(2), "each flash ends: {at:?}");
        assert_eq!(seen[240], Mode::Normal);
    }

    /// **Frames drawn through an outlived update are held the same way**:
    /// an update left open, its hold over, and a pair in it every 200 ms,
    /// each shown by the frame drawn through it.
    #[test]
    fn pairs_drawn_through_an_outlived_update_are_held_the_same_way() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        feed(&mut t, b"\x1b[?2026h");
        let mut seen = vec![Mode::of(&t)];
        for f in 0..240u32 {
            if f < 120 && f.is_multiple_of(12) {
                feed(&mut t, b"\x1b[?5h\x1b[?5l");
            }
            let flash = draw_through(&mut b, &t, at(FRAME * f), Update::Outlived);
            seen.push(video(&t, flash));
        }
        assert!(t.synchronized_output(), "the control: it never closed");
        let at = changes(&seen);
        assert!(at.len() >= 4, "the first flashes are shown: {at:?}");
        at_most_four_a_second(&at, "through an outlived update");
    }

    /// **A change held past the budget is owed a frame**, and the mode it
    /// holds is the program's at once: `DECRQM` answers it set while the
    /// screen still draws it reset. Drawn only when a frame is owed, the
    /// screen draws the mode at the deadline, a second after the first of
    /// the four changes before it.
    #[test]
    fn a_held_change_is_owed_a_frame_and_the_mode_answers_at_once() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        for (k, half) in [b"\x1b[?5h", b"\x1b[?5l", b"\x1b[?5h", b"\x1b[?5l"]
            .into_iter()
            .enumerate()
        {
            feed(&mut t, half);
            let f = u32::try_from(k).unwrap() * 3;
            let want = Mode::of(&t);
            assert_eq!(video(&t, draw(&mut b, &t, at(FRAME * f))), want, "{k}");
        }
        feed(&mut t, b"\x1b[?5h\x1b[?5$p");
        assert_eq!(t.reply(), b"\x1b[?5;1$y", "set, at once");
        assert_eq!(
            video(&t, draw(&mut b, &t, at(FRAME * 12))),
            Mode::Normal,
            "the fifth change is held"
        );
        let second = at(Duration::from_secs(1));
        assert_eq!(b.deadline(), Some(second), "owed a second after the first");
        assert!(!b.is_due(at(FRAME * 59)));
        assert!(b.is_due(at(FRAME * 60)));
        assert_eq!(video(&t, draw(&mut b, &t, at(FRAME * 60))), Mode::Reverse);
        assert_eq!(b.deadline(), None, "nothing more is owed");
    }

    /// **A flash's end dropped with its frame is drawn by the next frame
    /// drawn**: `?5` set and reset, then a pair, whose flash is the third
    /// change and has its return counted from its start; the frame that
    /// would end it is refused, and every frame after draws the mode. The
    /// budget is full for most of a second after that frame, so an end
    /// that lost its reservation with it would hold the flash until frame
    /// 60.
    #[test]
    fn a_flash_end_dropped_with_its_frame_is_drawn_by_the_next() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut b = Bell::new(&t, LEN);
        for f in 0..12u32 {
            match f {
                0 => feed(&mut t, b"\x1b[?5h"),
                3 => feed(&mut t, b"\x1b[?5l"),
                6 => feed(&mut t, b"\x1b[?5h\x1b[?5l"),
                _ => {}
            }
            let flash = draw(&mut b, &t, at(FRAME * f));
            let lit = (6..12).contains(&f);
            assert_eq!(flash == VideoFlash::On, lit, "frame {f}: the pair's flash");
        }
        let end = b.plan(&t, at(FRAME * 12), Update::Closed);
        assert_eq!(end.flash, VideoFlash::Off, "the frame would end it");
        b.refused(end);
        let lit: Vec<u32> = (13..80u32)
            .filter(|&f| draw(&mut b, &t, at(FRAME * f)) == VideoFlash::On)
            .collect();
        assert!(lit.is_empty(), "frames still showing the flash: {lit:?}");
        assert_eq!(Mode::of(&t), Mode::Normal);
    }

    /// **A change of the mode dropped with its frame is drawn by the next
    /// frame drawn** too, the slot it spent its reservation: the fourth
    /// change of a second, which fits, and a fifth, held and dropped at
    /// its deadline. Each time the budget is full for most of a second
    /// after the dropped frame, so a change that had to find room again
    /// would hold the old video until frame 60 or frame 111.
    #[test]
    fn a_change_of_mode_dropped_with_its_frame_is_drawn_by_the_next() {
        let mut shown = vec![];
        for (name, halves, dropped, held) in [
            ("the fourth", &[0u32, 3, 6, 9][..], 9u32, &[][..]),
            ("the fifth", &[0, 51, 54, 57, 58], 60, &[58u32, 59]),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            let mut before = vec![];
            for f in 0..=dropped {
                if let Some(k) = halves.iter().position(|&h| h == f) {
                    let half = if k.is_multiple_of(2) { "h" } else { "l" };
                    feed(&mut t, format!("\x1b[?5{half}").as_bytes());
                }
                if f < dropped && draw(&mut b, &t, at(FRAME * f)) == VideoFlash::On {
                    before.push(f);
                }
            }
            assert_eq!(before, held, "{name}: the frames the budget holds it");
            let plan = b.plan(&t, at(FRAME * dropped), Update::Closed);
            assert_eq!(plan.flash, VideoFlash::Off, "{name}: the frame draws it");
            b.refused(plan);
            let lit: Vec<u32> = (dropped + 1..dropped + 70)
                .filter(|&f| draw(&mut b, &t, at(FRAME * f)) == VideoFlash::On)
                .collect();
            shown.push((name, lit));
        }
        assert!(
            shown.iter().all(|(_, lit)| lit.is_empty()),
            "frames showing the old video: {shown:?}"
        );
    }

    /// **Frames dropped anywhere leave the screen at most four changes a
    /// second**: `?5` toggled every 50 ms, and a pair every 200 ms, for
    /// two seconds, with a run of one, two or three frames dropped
    /// starting at each frame, and with every second, third or fourth
    /// frame dropped. A dropped frame shows what the last drawn one drew,
    /// and a change it spent a slot on is drawn later without room of its
    /// own, so what the screen shows is counted, not what was planned.
    #[test]
    fn frames_dropped_anywhere_draw_at_most_four_changes_a_second() {
        let runs = (0..240u32).flat_map(|d| (1..=3).map(move |n| (d, n, 0)));
        let strides = (2..=4u32).flat_map(|n| (0..n).map(move |r| (r, 1, n)));
        let drops: Vec<(u32, u32, u32)> = runs.chain(strides).collect();
        for (name, every) in [("?5 every 50 ms", 3u32), ("a pair every 200 ms", 12)] {
            for &(from, run, stride) in &drops {
                let dropped = |f: u32| match stride {
                    0 => (from..from + run).contains(&f),
                    n => f % n == from,
                };
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut b = Bell::new(&t, LEN);
                let mut seen = vec![Mode::of(&t)];
                let mut lost = 0;
                for f in 0..240u32 {
                    if f < 120 && f.is_multiple_of(every) {
                        feed(
                            &mut t,
                            match (every, (f / every).is_multiple_of(2)) {
                                (12, _) => b"\x1b[?5h\x1b[?5l",
                                (_, true) => b"\x1b[?5h",
                                (_, false) => b"\x1b[?5l",
                            },
                        );
                    }
                    let p = b.plan(&t, at(FRAME * f), Update::Closed);
                    let last = seen[seen.len() - 1];
                    if dropped(f) {
                        lost += usize::from(video(&t, p.flash) != last);
                        b.refused(p);
                        seen.push(last);
                    } else {
                        b.commit(p);
                        seen.push(video(&t, p.flash));
                    }
                }
                let at = changes(&seen);
                let how = format!("{name}, dropped {from} {run} {stride}");
                for w in at.windows(5) {
                    assert!(w[4] - w[0] >= 60, "{how}: five changes at {w:?} of {at:?}");
                }
                assert_eq!(seen[240], Mode::of(&t), "{how}: the mode at the end");
                if stride == 0 && from == 9 && every == 3 {
                    assert!(lost > 0, "{how}: the control drops a change");
                }
            }
        }
    }

    /// **A `BEL` in a colour shares the budget with `?5`**: toggles every
    /// 250 ms are four changes a second alone, and a white bell rung every
    /// half second, two flashes of its own, may not add to them. A
    /// change of the bell's solid is counted where it completes: its rise
    /// on the frame it first reaches its peak, its fall on the first frame
    /// without it.
    #[test]
    fn a_bell_in_a_colour_and_toggles_share_the_budget() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b]21;visual_bell=#ffffff\x07");
        let mut b = Bell::new(&t, LEN);
        let (mut seen, mut solid) = (vec![Mode::of(&t)], vec![0]);
        for f in 0..240u32 {
            if f < 120 && f.is_multiple_of(15) {
                let on = (f / 15).is_multiple_of(2);
                feed(&mut t, if on { b"\x1b[?5h" } else { b"\x1b[?5l" });
            }
            if f < 120 && f % 30 == 7 {
                b.ring();
            }
            let p = b.plan(&t, at(FRAME * f), Update::Closed);
            b.commit(p);
            seen.push(video(&t, p.flash));
            solid.push(p.tint.map_or(0, |k| k.alpha.get()));
        }
        let peak = solid.iter().copied().max().unwrap_or(0);
        let mut at = changes(&seen);
        for i in 1..solid.len() {
            let rise = solid[i] == peak && solid[i - 1] < peak && peak > 0;
            let fall = solid[i] == 0 && solid[i - 1] > 0;
            if rise || fall {
                at.push(i);
            }
        }
        at.sort_unstable();
        assert!(peak > 0, "the bell is shown");
        at_most_four_a_second(&at, "toggles and a bell");
    }

    /// The update clock: honoured from its opening for its bound, outlived
    /// after, forgotten on a close, and restarted by a close and a reopen
    /// the driver never saw apart, once the update before it was drawn
    /// through.
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

    /// **A chain of updates is held for one bound, not for ever**: a
    /// program that closes each update and opens the next in one read,
    /// every 10 ms, leaves one open at every read's end, and the clock
    /// never sees one closed. Restarted at each opening, it held every
    /// frame for as long as the chain ran. The close is the update's end,
    /// "implicitly updating the render state by reading out the latest
    /// grid buffer state" (`[[source: contour-synchronized-output]]`), so
    /// an opening found while frames are held keeps the hold's start: the
    /// read past 33 ms draws through, and the next opening, after that
    /// frame, has its own bound.
    #[test]
    fn a_chain_of_reopened_updates_is_held_for_one_bound() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut c: UpdateClock<HostTime> = UpdateClock::new(MS * 33);
        feed(&mut t, b"\x1b[?2026h");
        let mut answers = vec![c.observe(&t, at(Duration::ZERO))];
        let mut deadlines = vec![c.deadline()];
        for k in 1..=9u32 {
            feed(&mut t, b"x\x1b[?2026l\x1b[?2026h");
            let now = at(MS * 10 * k);
            // A frame drawn outside the hold check reads what the check
            // will.
            let unseen = c.state(&t, now);
            answers.push(c.observe(&t, now));
            assert_eq!(Some(&unseen), answers.last(), "{k}");
            deadlines.push(c.deadline());
        }
        use Update::{Honoured as H, Outlived as O};
        assert_eq!(answers, [H, H, H, H, O, H, H, H, H, O]);
        let (first, next) = (Some(at(MS * 33)), Some(at(MS * 83)));
        assert_eq!(deadlines, [[first; 5], [next; 5]].concat());
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

    mod bel {
        //! A `BEL`: what it does ([`ring`]), how often ([`Ringer`]), and
        //! its flash ([`Bell::ring`]).

        use super::super::*;
        use super::{LEN, MS, WS, at, draw, draw_through, feed};
        use crate::config::{BellStyle, SystemMotion};
        use baud_core::Owned;

        const STYLES: [BellStyle; 3] = [BellStyle::Sound, BellStyle::Flash, BellStyle::Nothing];
        const MOTIONS: [SystemMotion; 2] = [SystemMotion::Full, SystemMotion::Reduced];
        const WINDOWS: [Window; 3] = [Window::Focused, Window::Unfocused, Window::Minimized];
        const URGENT: [BellIsUrgent; 2] = [BellIsUrgent::Off, BellIsUrgent::On];
        const POP: [PopOnBell; 2] = [PopOnBell::Off, PopOnBell::On];
        const VOLUMES: [BellVolume; 3] = [BellVolume::Off, BellVolume::Low, BellVolume::High];
        /// The volume at power-on, where a test is not about the volume.
        const HIGH: BellVolume = BellVolume::High;

        /// One input to [`ring`].
        type Input = (
            BellStyle,
            SystemMotion,
            Window,
            BellIsUrgent,
            PopOnBell,
            BellVolume,
        );

        /// Every input, with the ring it decides.
        fn every() -> Vec<(Input, Ring)> {
            let mut out = Vec::new();
            for s in STYLES {
                for m in MOTIONS {
                    for w in WINDOWS {
                        for u in URGENT {
                            for p in POP {
                                for v in VOLUMES {
                                    out.push(((s, m, w, u, p, v), ring(s, m, w, u, p, v)));
                                }
                            }
                        }
                    }
                }
            }
            out
        }

        /// **The window is AppKit's two answers, all four of them**: key
        /// is focused whatever else it reads, and a window that is not key
        /// is minimized or merely unfocused.
        #[test]
        fn the_window_is_classed_from_both_answers() {
            assert_eq!(
                Window::of(Key::Key, Miniaturized::NotMinimized),
                Window::Focused
            );
            assert_eq!(
                Window::of(Key::Key, Miniaturized::Minimized),
                Window::Focused,
                "key wins"
            );
            assert_eq!(
                Window::of(Key::NotKey, Miniaturized::Minimized),
                Window::Minimized
            );
            assert_eq!(
                Window::of(Key::NotKey, Miniaturized::NotMinimized),
                Window::Unfocused
            );
        }

        /// The rule in the focused window: the config's `bell`, and neither
        /// mode.
        #[test]
        fn a_focused_window_does_what_the_config_says() {
            let quiet = Ring {
                sound: Sound::Silent,
                shown: Shown::Nothing,
                attention: Attention::None,
                raise: Raise::Stay,
            };
            for u in URGENT {
                for p in POP {
                    let f = |s, m| ring(s, m, Window::Focused, u, p, HIGH);
                    assert_eq!(
                        f(BellStyle::Sound, SystemMotion::Full),
                        Ring {
                            sound: Sound::Beep,
                            ..quiet
                        },
                        "{u:?} {p:?}: sound"
                    );
                    assert_eq!(
                        f(BellStyle::Flash, SystemMotion::Full),
                        Ring {
                            shown: Shown::Flash,
                            ..quiet
                        },
                        "{u:?} {p:?}: flash"
                    );
                    assert_eq!(
                        f(BellStyle::Nothing, SystemMotion::Full),
                        quiet,
                        "{u:?} {p:?}: none"
                    );
                }
            }
        }

        /// **Unfocused, the modes act and the sound does not**: `?1042`
        /// asks for attention, `?1043` raises, each alone, whatever the
        /// config says -- `none` included.
        #[test]
        fn an_unfocused_window_does_what_the_modes_say() {
            for s in STYLES {
                let r = |u, p| ring(s, SystemMotion::Full, Window::Unfocused, u, p, HIGH);
                assert_eq!(r(BellIsUrgent::Off, PopOnBell::Off).sound, Sound::Silent);
                assert_eq!(
                    r(BellIsUrgent::Off, PopOnBell::Off).attention,
                    Attention::None
                );
                assert_eq!(r(BellIsUrgent::Off, PopOnBell::Off).raise, Raise::Stay);
                assert_eq!(
                    r(BellIsUrgent::On, PopOnBell::Off).attention,
                    Attention::Request,
                    "{s:?}: ?1042"
                );
                assert_eq!(r(BellIsUrgent::On, PopOnBell::Off).raise, Raise::Stay);
                assert_eq!(
                    r(BellIsUrgent::Off, PopOnBell::On).raise,
                    Raise::Front,
                    "{s:?}: ?1043"
                );
                assert_eq!(
                    r(BellIsUrgent::Off, PopOnBell::On).attention,
                    Attention::None
                );
            }
        }

        /// **A minimized window is not raised**, "If the window is
        /// iconified, this has no effect", and still asks for attention.
        #[test]
        fn a_minimized_window_is_not_raised_and_still_asks() {
            for s in STYLES {
                let r = ring(
                    s,
                    SystemMotion::Full,
                    Window::Minimized,
                    BellIsUrgent::On,
                    PopOnBell::On,
                    HIGH,
                );
                assert_eq!(r.raise, Raise::Stay, "{s:?}");
                assert_eq!(r.attention, Attention::Request, "{s:?}");
                assert_eq!(r.sound, Sound::Silent, "{s:?}");
                assert_eq!(
                    r.shown,
                    Shown::Nothing,
                    "{s:?}: no flash waits for a restore"
                );
            }
        }

        /// **Reduce Motion: no flash, and nothing in its place.** The
        /// control: the same inputs under full motion flash.
        #[test]
        fn reduce_motion_shows_no_flash_and_rings_nothing_instead() {
            for ((s, m, w, u, p, v), r) in every() {
                let full = ring(s, SystemMotion::Full, w, u, p, v);
                match (s, m) {
                    (BellStyle::Flash, SystemMotion::Reduced) => {
                        assert_eq!(
                            full.shown == Shown::Flash,
                            w != Window::Minimized,
                            "the control, {w:?}"
                        );
                        assert_eq!(
                            r,
                            Ring {
                                shown: Shown::Nothing,
                                ..full
                            },
                            "{w:?} {u:?} {p:?}"
                        );
                    }
                    _ => assert_eq!(r, full, "{s:?} {m:?} {w:?}: motion moves nothing else"),
                }
            }
        }

        /// The whole table, each of the four outputs a function of only
        /// what the rule says it reads: the sound of the style and the
        /// focus, the flash of the style, the motion and whether the window
        /// is minimized, the attention of
        /// `?1042` and the focus, the raise of `?1043` and the window.
        #[test]
        fn each_output_reads_only_its_own_inputs() {
            let all = every();
            assert_eq!(all.len(), 216, "the control: every input");
            for ((s, m, w, u, p, v), r) in all {
                let focused = w == Window::Focused;
                assert_eq!(
                    r.sound == Sound::Beep,
                    s == BellStyle::Sound && focused && v != BellVolume::Off,
                    "{s:?} {m:?} {w:?} {u:?} {p:?} {v:?}: sound"
                );
                assert_eq!(
                    r.shown == Shown::Flash,
                    s == BellStyle::Flash && m == SystemMotion::Full && w != Window::Minimized,
                    "{s:?} {m:?} {w:?}: flash"
                );
                assert_eq!(
                    r.attention == Attention::Request,
                    u == BellIsUrgent::On && !focused,
                    "{w:?} {u:?}: attention"
                );
                assert_eq!(
                    r.raise == Raise::Front,
                    p == PopOnBell::On && w == Window::Unfocused,
                    "{w:?} {p:?}: raise"
                );
            }
        }

        /// **The program's volume reaches the sound alone**: off is silence
        /// with the flash, the request and the raise as they were; low
        /// sounds as high does, `NSBeep`.
        #[test]
        fn the_volume_silences_the_sound_and_nothing_else() {
            for ((s, m, w, u, p, v), r) in every() {
                let high = ring(s, m, w, u, p, BellVolume::High);
                match v {
                    BellVolume::Off => assert_eq!(
                        r,
                        Ring {
                            sound: Sound::Silent,
                            ..high
                        },
                        "{s:?} {m:?} {w:?} {u:?} {p:?}"
                    ),
                    _ => assert_eq!(r, high, "{s:?} {m:?} {w:?} {u:?} {p:?} {v:?}"),
                }
            }
            assert_eq!(
                ring(
                    BellStyle::Sound,
                    SystemMotion::Full,
                    Window::Focused,
                    BellIsUrgent::Off,
                    PopOnBell::Off,
                    BellVolume::Off,
                )
                .sound,
                Sound::Silent,
                "the control: off is heard nowhere"
            );
        }

        /// **The ringer hears both bells, each at its own volume**: a
        /// `BEL` at DECSWBV's, the margin bell at DECSMBV's, the louder
        /// when both rang in one interval.
        #[test]
        fn the_ringer_hears_each_bell_at_its_own_volume() {
            let frame = MS * 8;
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut r: Ringer<HostTime> = Ringer::new(&t);
            feed(&mut t, b"\x1b[1 t\x07");
            assert_eq!(
                r.hear(&t, at(MS), frame),
                Rings::Ring(BellVolume::Off),
                "DECSWBV 1"
            );
            feed(&mut t, b"\x1b[3 t\x07");
            assert_eq!(
                r.hear(&t, at(MS * 20), frame),
                Rings::Ring(BellVolume::Low),
                "DECSWBV 3"
            );
            // The margin bell: on at low, the cursor on the 20-column
            // line's twelfth column (RIGHT MARGIN - 8), a key typed and
            // echoed.
            feed(&mut t, b"\x1b[?44h\x1b[2 u\x1b[1 t\x1b[1;12H");
            assert!(t.key(baud_core::KeyEvent::press(
                baud_core::KeyCode::Char(baud_core::Codepoint::from_char('a')),
                baud_core::Modifiers::NONE,
            )));
            t.clear_reply();
            feed(&mut t, b"a");
            assert_eq!(t.margin_bells, 1, "the control: the margin bell rang");
            assert_eq!(
                r.hear(&t, at(MS * 40), frame),
                Rings::Ring(BellVolume::Low),
                "the margin bell at DECSMBV's, not DECSWBV's off"
            );
            feed(&mut t, b"\x1b[8 t\x07");
            assert_eq!(
                r.hear(&t, at(MS * 60), frame),
                Rings::Ring(BellVolume::High)
            );
        }

        /// **A bell rings at the volume it was sent at**, not at the one a
        /// read leaves: DECSWBV around a `BEL` in one read, either order,
        /// and `RIS` after it.
        #[test]
        fn a_bell_rings_at_the_volume_it_was_sent_at() {
            let frame = MS * 8;
            for (bytes, want) in [
                (&b"\x1b[1 t\x07\x1b[5 t"[..], BellVolume::Off),
                (b"\x07\x1b[1 t", BellVolume::High),
                (b"\x1b[3 t\x07\x1b[1 t", BellVolume::Low),
                (b"\x1b[1 t\x07\x1bc", BellVolume::Off),
            ] {
                let mut owned = Owned::new(&WS);
                let mut t = Term::new(owned.storage(), &WS);
                let mut r: Ringer<HostTime> = Ringer::new(&t);
                feed(&mut t, bytes);
                assert_eq!(r.hear(&t, at(MS), frame), Rings::Ring(want), "{bytes:?}");
            }
            // The margin bell, rung at low and turned up in the same read.
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"\x1b[?44h\x1b[2 u\x1b[1 t\x1b[1;12H");
            let mut r: Ringer<HostTime> = Ringer::new(&t);
            assert!(t.key(baud_core::KeyEvent::press(
                baud_core::KeyCode::Char(baud_core::Codepoint::from_char('a')),
                baud_core::Modifiers::NONE,
            )));
            t.clear_reply();
            feed(&mut t, b"a\x1b[8 u");
            assert_eq!(t.margin_bells, 1, "the control: the margin bell rang");
            assert_eq!(r.hear(&t, at(MS), frame), Rings::Ring(BellVolume::Low));
        }

        /// **Bells within one frame ring once; a lone bell rings on the
        /// read that brought it.**
        #[test]
        fn bells_within_one_frame_ring_once() {
            let frame = MS * 8;
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"\x07");
            let mut r: Ringer<HostTime> = Ringer::new(&t);
            assert_eq!(
                r.hear(&t, at(Duration::ZERO), frame),
                Rings::Quiet,
                "a bell from before the ringer is not this one's"
            );
            feed(&mut t, b"a");
            assert_eq!(r.hear(&t, at(MS), frame), Rings::Quiet, "no bell");
            feed(&mut t, b"\x07\x07\x07");
            assert_eq!(
                r.hear(&t, at(MS * 2), frame),
                Rings::Ring(BellVolume::High),
                "three in one read"
            );
            feed(&mut t, b"\x07");
            assert_eq!(
                r.hear(&t, at(MS * 9), frame),
                Rings::Quiet,
                "inside the frame"
            );
            assert_eq!(
                r.hear(&t, at(MS * 10), frame),
                Rings::Quiet,
                "absorbed, not put off: a frame on, it does not ring"
            );
            feed(&mut t, b"\x07");
            assert_eq!(
                r.hear(&t, at(MS * 10), frame),
                Rings::Ring(BellVolume::High),
                "a bell a frame after the last ring"
            );
        }

        /// **A ring's flash is planned and committed as a `?5` pair's**:
        /// shown by the next frame from when it is drawn, run to its
        /// deadline, and several rings before a frame are one flash.
        #[test]
        fn a_ring_flashes_from_the_next_frame() {
            let mut owned = Owned::new(&WS);
            let t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            assert_eq!(
                draw(&mut b, &t, at(Duration::ZERO)),
                VideoFlash::Off,
                "the control"
            );
            b.ring();
            b.ring();
            assert_eq!(b.deadline(), None, "a ring draws nothing by itself");
            assert_eq!(draw(&mut b, &t, at(MS * 5)), VideoFlash::On, "the start");
            assert_eq!(
                b.deadline(),
                Some(at(MS * 105)),
                "one flash, from the frame"
            );
            assert_eq!(draw(&mut b, &t, at(MS * 50)), VideoFlash::On, "held");
            assert_eq!(draw(&mut b, &t, at(MS * 105)), VideoFlash::Off, "the end");
            assert_eq!(b.deadline(), None, "nothing more is owed");
        }

        /// A ring that lands between a frame's plan and its commit is shown
        /// by the next frame: the count, not a deadline set from the side.
        #[test]
        fn a_ring_between_plan_and_commit_is_not_lost() {
            let mut owned = Owned::new(&WS);
            let t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            let p = b.plan(&t, at(Duration::ZERO), Update::Closed);
            b.ring();
            b.commit(p);
            assert_eq!(p.flash, VideoFlash::Off, "the plan was made before");
            assert_eq!(draw(&mut b, &t, at(MS * 8)), VideoFlash::On);
        }

        /// A `BEL` is not screen content: a frame drawn through an honoured
        /// update shows its flash, where it hides a `?5` pair; and a change
        /// of `?5` in the same frame wins, as it does over a pair.
        #[test]
        fn an_update_hides_no_ring_and_a_change_of_mode_wins() {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            feed(&mut t, b"\x1b[?2026h");
            b.ring();
            assert_eq!(
                draw_through(&mut b, &t, at(MS * 5), Update::Honoured),
                VideoFlash::On
            );
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let mut b = Bell::new(&t, LEN);
            b.ring();
            feed(&mut t, b"\x1b[?5h");
            assert_eq!(
                draw(&mut b, &t, at(MS * 5)),
                VideoFlash::Off,
                "reverse, as set"
            );
        }
    }
}
