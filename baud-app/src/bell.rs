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
//!
//! **A `BEL` rings here too**, the owner's call of 2026-10-13,
//! "System sound + Dock bounce": `NSBeep` while the window is focused; while
//! it is not,
//! `?1042` asks for the user's attention -- the Dock icon bounces -- and
//! `?1043` raises the window; the config's `bell` picks the sound, the
//! visual bell or nothing. [`ring`] is that decision, a pure function of the
//! config, the motion preference, the window and the two modes, and the
//! macOS driver's shim makes the AppKit calls it names. [`Ringer`] reads
//! [`Term::bells`](baud_core::Term#structfield.bells) after each read and
//! rings once for the bells of one frame interval. A flash it decides is
//! started through [`Bell::ring`], so it is planned, committed and refused
//! as a `?5` pair's is.

use core::time::Duration;
use std::time::Instant;

#[cfg(any(target_os = "macos", test))]
use baud_core::{BellIsUrgent, PopOnBell};
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

    #[cfg(any(target_os = "macos", test))]
    fn next(self) -> Rung {
        Rung(self.0.wrapping_add(1))
    }
}

/// The bell: the reading the last drawn frame made, and the deadline of a
/// flash on screen.
pub struct Bell<T> {
    seen: Seen,
    until: Option<T>,
    length: Duration,
    /// The flashes asked for so far, of which `seen.rung` the last drawn
    /// frame had shown.
    rung: Rung,
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
            seen: Seen::of(term, Rung::FIRST),
            until: None,
            length,
            rung: Rung::FIRST,
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
        // A `BEL` is not screen content, so an update hides none: its flash
        // is shown by the first frame drawn after it, held or not.
        let rang = seen.rung != self.seen.rung;
        let until = if seen.mode != self.seen.mode {
            None
        } else if seen.shown != self.seen.shown || held_shown || rang {
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

    /// A `BEL` asked for the visual bell ([`ring`]): the next frame drawn
    /// shows it, from when it is drawn, unless that frame finds `?5` changed
    /// -- the mode is what a frame draws, and a flash of it would draw the
    /// opposite. Several rings before a frame are one flash.
    #[cfg(any(target_os = "macos", test))]
    pub fn ring(&mut self) {
        self.rung = self.rung.next();
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

/// The window as a `BEL` finds it. `Minimized` is unfocused too, and the
/// one state a raise must leave alone: `[[source: xterm-man]]` on
/// `popOnBell`, "If the window is iconified, this has no effect."
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    Focused,
    Unfocused,
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
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sound {
    Silent,
    Beep,
}

/// Whether the ring shows the visual bell ([`Bell::ring`]).
#[cfg(any(target_os = "macos", test))]
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
#[cfg(any(target_os = "macos", test))]
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
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Raise {
    Stay,
    Front,
}

/// What one ring does: each of the four, decided apart.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ring {
    pub sound: Sound,
    pub shown: Shown,
    pub attention: Attention,
    pub raise: Raise,
}

/// **What a `BEL` does**, the owner's call of 2026-10-13: "NSBeep when
/// focused; ?1042 bounces the Dock and ?1043 raises the window when
/// unfocused; config bell = sound|flash|none".
///
/// * Focused: the config's `bell` -- the sound, the visual bell, or nothing.
///   Neither mode acts: the window is the one the user is in.
/// * Not focused: `?1042` asks for attention, as xterm sets its urgency hint
///   only `if (enable && !(screen->select & FOCUS))` (`setXUrgency`,
///   `[[source: xterm-misc]]`); `?1043` raises the window, unless it is
///   minimized. **No sound**: the owner's "NSBeep when focused", read as
///   written; xterm rings whatever the focus, which is the alternative. A
///   flash is drawn whatever the focus, as xterm's `VisualBell` is, since
///   the decision limits only the sound to a focused window -- **but not in
///   a minimized one**, which "is removed from the screen" (`[[source:
///   apple-nswindow-isminiaturized]]`): its flash would wait for the next
///   drawn frame and show on the restore, long after the bell.
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
#[cfg(any(target_os = "macos", test))]
pub fn ring(
    style: crate::config::BellStyle,
    motion: crate::config::SystemMotion,
    window: Window,
    urgent: BellIsUrgent,
    pop: PopOnBell,
) -> Ring {
    use crate::config::{BellStyle, SystemMotion};
    let focused = window == Window::Focused;
    let sound = match (style, focused) {
        (BellStyle::Sound, true) => Sound::Beep,
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

/// How many `BEL`s the terminal had counted when the ringer last looked.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Heard(usize);

#[cfg(any(target_os = "macos", test))]
impl Heard {
    fn of(term: &Term<'_>) -> Heard {
        Heard(term.bells)
    }
}

/// Whether the bells a read brought ring.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rings {
    /// No new bell, or only bells inside the frame interval of the last ring.
    Quiet,
    Ring,
}

/// **Bells within one frame interval ring once.** The driver asks after
/// every read; the bells ring if the count moved and the last ring is at
/// least a frame interval old, and bells inside that interval are absorbed,
/// not put off -- so `yes $'\a'` rings at most once a frame, and a lone bell
/// rings on the read that brought it, drawn or not: a sound, a bounce and a
/// raise are not frames, and a minimized window draws none.
///
/// No text governs how bells coalesce. xterm suppresses for
/// `bellSuppressTime`, "Number of milliseconds after a bell command is sent
/// during which additional bells will be suppressed. Default is 200", and
/// until the server reports the first bell done (`[[source: xterm-man]]`;
/// `Bell`, `[[source: xterm-misc]]`). One frame is the bound built, and
/// xterm's 200 ms the open alternative. What Terminal.app does is not
/// known: its source is not published, no documentation of its bell is
/// cited here, and the rules forbid driving a GUI to find out.
#[cfg(any(target_os = "macos", test))]
pub struct Ringer<T> {
    heard: Heard,
    last: Option<T>,
}

#[cfg(any(target_os = "macos", test))]
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
        self.heard = heard;
        if self.last.is_some_and(|l| now.elapsed_since(l) < frame) {
            return Rings::Quiet;
        }
        self.last = Some(now);
        Rings::Ring
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

        /// One input to [`ring`].
        type Input = (BellStyle, SystemMotion, Window, BellIsUrgent, PopOnBell);

        /// Every input, with the ring it decides.
        fn every() -> Vec<(Input, Ring)> {
            let mut out = Vec::new();
            for s in STYLES {
                for m in MOTIONS {
                    for w in WINDOWS {
                        for u in URGENT {
                            for p in POP {
                                out.push(((s, m, w, u, p), ring(s, m, w, u, p)));
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

        /// The owner's rule in the focused window: the config's `bell`, and
        /// neither mode.
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
                    let f = |s, m| ring(s, m, Window::Focused, u, p);
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
                let r = |u, p| ring(s, SystemMotion::Full, Window::Unfocused, u, p);
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
            for ((s, m, w, u, p), r) in every() {
                let full = ring(s, SystemMotion::Full, w, u, p);
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
            assert_eq!(all.len(), 72, "the control: every input");
            for ((s, m, w, u, p), r) in all {
                let focused = w == Window::Focused;
                assert_eq!(
                    r.sound == Sound::Beep,
                    s == BellStyle::Sound && focused,
                    "{s:?} {m:?} {w:?} {u:?} {p:?}: sound"
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
                Rings::Ring,
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
                Rings::Ring,
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
