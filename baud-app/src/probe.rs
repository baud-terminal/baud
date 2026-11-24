//! **Whether a printed path exists, asked of one worker thread** that the
//! frame never waits on. `paths.rs` finds the paths and draws what this
//! found; this asks, and decides when.
//!
//! **One thread a window, started when the first program is seen writing**
//! -- never with `[link] paths = "off"` -- and never another: it names that
//! program, takes a batch, the newest one sent, answers it, and waits for
//! the next. The frame path only reads
//! answers already sent (`Receiver::try_recv`), so a call that never returns
//! -- a lookup on a dead mount -- stops nothing but the worker.
//!
//! **When it is asked** ([`Pace`]): once a scan has met a path it has no
//! answer for, [`SETTLE`] after the last read of the program's output, and at
//! most once each [`CEILING`] while output goes on -- a spinner or a
//! program's repaint never goes quiet, and without the ceiling its paths
//! would never be asked about. A batch that asked about a path holds the
//! next for [`CEILING`]; one that only named the programs that wrote holds
//! nothing, so a listing's paths, readable once its writer is named, are
//! asked at once. Batches that only name are bounded by the quiet instead:
//! each waits for output after the last batch and [`SETTLE`] after it, so
//! two are never closer than [`SETTLE`] -- at most 1 + 1 s / [`SETTLE`],
//! 7, in any second, where batches that ask about a path go once a second.
//!
//! **Where the window's own shell says its command ended**, the quiet is
//! not waited for: its `D`, "End of current command"
//! (`[[source: semantic-prompts]]`), carrying the token baud handed that
//! shell, in a part the core read alike, with nothing open after it
//! (`vouch::Ended`), makes the next batch due at once, until a batch asks
//! about a path or [`SETTLE`] after the drain that read it. The shell
//! prints it once its job has ended, so every byte the job wrote is
//! before it. It moves when a batch goes and nothing a batch reads: a row
//! is filed by its writers at its read, whatever the pace. **The grant
//! serves the chain to one asking batch**: it lifts the floor after the last
//! batch that asked for at most two scans (`Pace::scanned`), the one naming
//! the listing's writer and the one asking about its paths, and the asking
//! one spends it and lays the floor again. So per tokened `D` at most two
//! scans pass the floor, at most one of them asks about a path, each at
//! least [`GRANT_GAP`] after the scan before; one grant scans at most 20
//! times where no floor is live; and across `D`s, a key held down, batches
//! go at most once each [`GRANT_GAP`]. **It hastens only a batch owed for a
//! path** (`paths::Owes`): one with no answer, or one on a row whose writer
//! is still to be named, which the naming makes readable -- and drawable,
//! where its answer is kept. Rows that changed alone, a prompt with no path
//! at a held key, take the quiet and the ceiling. Output with no such `D`
//! -- a flood, a spinner, a remote shell's or a nested one's marks, which
//! carry no token or another -- is paced by the quiet and held by the
//! floor.
//!
//! Each batch is the window's screen as it is
//! then: one scan of every row (`Paths::begin_collect`), so a flood costs
//! one scan of the last screen, and a batch sent replaces one not yet taken.
//! The worker leaves a batch for a newer one between two paths.
//!
//! **A hang closes it, for that window.** While a batch is out, every call
//! the worker makes that returns counts a beat. No beat for [`HANG`] is a
//! hang: path marks go for the rest of the window's life, no batch is sent
//! again, and the stuck thread is left where it is -- no second one is
//! started. The deadline is checked where the driver's clock already runs:
//! the display link's tick on macOS, the `poll` timeout on Linux. Nothing
//! sleeps.
//!
//! **What it calls, and nothing else**: the mount table, then a walk of the
//! path a component at a time ([`walk`]), each by `std::fs::symlink_metadata`
//! and, for a link, `std::fs::read_link`. No candidate is ever opened, no
//! directory read, and no `statfs` asked of one. In the pinned toolchain's
//! `std` (its 1.99.1 source, read: the nightly's own source is not installed),
//! `symlink_metadata` is `lstat(2)` on macOS, which "returns information
//! about the link" rather than what it names (`[[source: macos-man-stat-2]]`),
//! and `statx(2)` with
//! `AT_SYMLINK_NOFOLLOW`, falling back to `lstat64(2)`, on Linux, and
//! `read_link` is `readlink(2)` on both.
//!
//! * **The mount table before any lookup.** macOS: `getfsstat(2)` with
//!   `MNT_NOWAIT`, which "will directly return the information retained in
//!   the kernel to avoid delays caused by waiting for updated information
//!   from a file system that is perhaps temporarily unable to respond"
//!   (`[[source: macos-man-getfsstat-2]]`); a file system without
//!   `MNT_LOCAL`, "stored locally" (`[[source: macos-sdk-sys-mount-h]]`), or
//!   of a network, FUSE or automounter type is refused. Linux:
//!   `/proc/self/mountinfo` (`[[source: linux-man-proc-pid-mountinfo]]`),
//!   which carries no flag for local, so only the file system types on
//!   [`LINUX_LOCAL`] are taken and every other is refused.
//! * **A component at a time**, because a path's text is not where it leads:
//!   on macOS `/home` is a link to `/System/Volumes/Data/home`, an
//!   automounter map, so a check of the printed text misses `/home/x`, and a
//!   lookup of the whole path follows the link and mounts it. Each component
//!   is checked against the table before it is looked up, and a link is
//!   followed by hand, at most [`LINKS`] of them. RFC 8089 warns that
//!   "merely testing for the existence of such a name will cause the
//!   operating system to pause or invoke unrelated system calls"
//!   (`[[source: rfc8089]]` § 5).
//! * **The program in front first** ([`front_of`]): under `ssh` and its kin a
//!   path names another machine's file, and nothing is looked up.
//! * **And every program that wrote since the last batch.** A one-shot
//!   `ssh host ls /etc` prints and exits before the batch, which then finds
//!   the shell in front. So the foreground group is read at the read
//!   ([`Sampler`]: `TIOCGPGRP` on the master, at most once each
//!   [`GROUP_EVERY`]), each distinct group kept with the lines written
//!   under it ([`Probes::read_taken`]), and named by the worker at once,
//!   while it may still run. A batch carries them; a path printed on a row
//!   a group named not local wrote is not looked up, and that row keeps no
//!   mark, nor does one a group gone before it was named wrote, whose
//!   writer is not known: those rows only, and the rows around them keep
//!   theirs (`Paths::written_under`).
//! * **No cloud file fetched to answer a lookup.** On macOS the worker sets
//!   `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` on its own thread first
//!   (`sys::no_dataless_fetch`), so an `lstat` inside a File Provider folder
//!   that is only a placeholder does not fetch it. The call is accepted on
//!   this Mac; what it prevents is unverified.
//!
//! **Whether a program opens a link, asked of the same worker**
//! ([`Program`], `paths.rs` says what is struck for it): a kind of link's
//! app by its scheme alone, at a fixed address ([`crate::paths::address`]),
//! so no byte a program printed reaches LaunchServices; a `[link.*]` rule's
//! `command[0]` by the walk above, along `PATH`. The list is the driver's,
//! from the config, never the output's, and it is asked when a window
//! starts, when its config is read again, when it comes to the front, when
//! the pointer comes over a link struck for it and, on macOS while one was
//! found missing, when a job in front ends -- never on a timer. On macOS the app is
//! `NSWorkspace.URLForApplicationToOpenURL`, which opens nothing; the
//! class carries no main-actor mark in the SDK's header, where
//! `NSResponder` carries `NS_SWIFT_UI_ACTOR`
//! (`[[source: sdk27-nsworkspace-h]]`), and Apple's summary of AppKit's
//! threading names it in neither its thread-unsafe nor its main-thread-only
//! list (`[[source: apple-thread-safety-summary]]`), so it is called here,
//! off the main thread, inside an autorelease pool of its own. That is the
//! documentation's word: no run has tested it off the main thread. **On
//! Linux no kind is asked**: `xdg-open` falls back to a browser for a
//! scheme with no handler of its own, so an empty answer would not mean
//! nothing opens, and nothing is struck for a scheme there until that is
//! tested.
//!
//! **What is counted, never named**: paths are private, so no path is logged
//! by this or by anything it answers. The five-second report counts the
//! paths asked about and the calls made (`report::Links::checks_line`), at
//! `BAUD_LOG=info`, which is what a check of the calls against what a test
//! expects reads.
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use baud_core::Term;
use baud_domain::{PendingGen, Rows, Tracked};
use baud_render::Scrolled;

use crate::bell::Moment;
use crate::fg::StartTime;
use crate::lined::{Lines, Uris};
use crate::paths::{
    Candidate, CoreLines, Front, Handler, LineOn, LineSpan, Links, Marks, Moved, Owed, Owes,
    PartLines, Paths, PathsAt, Probe, Program, ProgramDir, Turned,
};
use crate::sys::Pid;

/// How long after the program's last output a batch is sent.
pub const SETTLE: Duration = Duration::from_millis(150);

/// The longest a path met waits for its batch while output goes on, and
/// the shortest between two batches that ask about a path.
pub const CEILING: Duration = Duration::from_secs(1);

/// **The shortest between a batch the shell's `D` made due and the scan
/// before it, sent or not**: a 120 Hz display's frame is 8.33 ms, so a
/// grant scans at most once a frame there, each a scan of the screen on the
/// main thread, and one grant, live for [`SETTLE`] from its drain, at most
/// 20 times (`Pace::scanned`); and a listing's two batches, the one naming
/// its writer and the one asking about its paths, still fit in two frames
/// with the worker's answer between. The pace knows no display's rate, so
/// it is fixed here, not read from one.
pub const GRANT_GAP: Duration = Duration::from_millis(8);

/// **How long the worker may go with a batch out and no call returning
/// before path marks close for the window.** A local disk that has spun
/// down can take seconds to answer its first lookup; a lookup on a mount
/// that has gone may never return. Five seconds is past the first, and the
/// cost of being past it is only that marks wait: nothing else waits on the
/// worker.
pub const HANG: Duration = Duration::from_secs(5);

/// **The fewest milliseconds between two reads of the foreground group**,
/// on the thread taking the reads: `TIOCGPGRP` measured 3.1 to 5.6 µs a
/// call on this Mac, under load, so asked at every 4 KB read of a flood it
/// would cost the reader a sixth of its time. A program's first read after
/// a pause is always read, so a one-shot remote command is seen; a program
/// in front for less than this, with every read inside it, is not.
pub const GROUP_EVERY: Duration = Duration::from_millis(10);

/// The groups' spans kept between two batches, a group's rows that touch
/// being one; past it, every line written after counts as not named until
/// the next.
pub const GROUPS: usize = 16;

/// The groups the worker keeps named, ahead of the batches that ask.
const NAMED: usize = 64;

/// The most symbolic links one walk follows by hand, as the kernel bounds its
/// own walk (`MAXSYMLINKS`, 32 on macOS): past it the answer is unknown.
pub const LINKS: usize = 32;

/// What the worker named a group as when it first saw it: when its leader
/// started, its names, and the session of the ssh leading it, read then
/// ([`ssh_session`]), which the rows it writes are filed as printed by.
type Named = std::collections::HashMap<
    Pid,
    (
        Option<StartTime>,
        Vec<String>,
        Option<Arc<crate::ssh::Session>>,
    ),
>;

/// **The ssh session the worker last read in front**, by its leader's pid
/// and start: read once while that process leads the group in front, its
/// `ssh -G` run once, and dropped when any other group is. A leader whose
/// start cannot be read has none, so a pid another process took never
/// reads as the session it had.
#[derive(Default)]
struct Sessions(Option<SessionRead>);

/// A session read, by the leader it was read for.
struct SessionRead {
    group: Pid,
    at: StartTime,
    session: Option<Arc<crate::ssh::Session>>,
}

impl Sessions {
    /// **The session of the ssh leading `group`**, read through `fs`
    /// unless it was for this process.
    fn of(
        &mut self,
        fs: &mut dyn Fs,
        group: Pid,
        calls: &mut u64,
        beat: &dyn Fn(),
    ) -> Option<Arc<crate::ssh::Session>> {
        let started = fs.started(group);
        *calls += 1;
        let Some(at) = started else {
            self.0 = None;
            return None;
        };
        match &self.0 {
            Some(k) if k.group == group && k.at == at => return k.session.clone(),
            _ => {}
        }
        let argv = fs.argv(group);
        *calls += 1;
        beat();
        let read = argv.and_then(|argv| {
            let mut asked = 0;
            let read = crate::ssh::session(&argv, |a| {
                asked += 1;
                fs.resolve(group, a)
            });
            *calls += asked;
            read.ok().map(Arc::new)
        });
        beat();
        self.0 = Some(SessionRead {
            group,
            at,
            session: read.clone(),
        });
        read
    }

    /// Another program in front, or none: the session read goes.
    fn drop_kept(&mut self) {
        self.0 = None;
    }
}

/// **`group` named as soon as it is seen**, while it may still run: its
/// leader's start, its names and, for an ssh, its session, kept for the
/// batch that names the rows it wrote ([`answer`]).
fn name_group(
    fs: &mut dyn Fs,
    named: &mut Named,
    sessions: &mut Sessions,
    group: Pid,
    calls: &mut u64,
    beat: &dyn Fn(),
) {
    if named.len() >= NAMED {
        named.clear();
    }
    let t = fs.started(group);
    let n = fs.program(group);
    *calls += 2;
    beat();
    let s = ssh_session(fs, sessions, group, &n, calls, beat);
    named.insert(group, (t, n, s));
}

/// **The session of the ssh leading `group`, named `names`**, read through
/// `sessions` ([`Sessions::of`]) while it leads the group: none for a
/// program that is not ssh, or is named on this machine.
fn ssh_session(
    fs: &mut dyn Fs,
    sessions: &mut Sessions,
    group: Pid,
    names: &[String],
    calls: &mut u64,
    beat: &dyn Fn(),
) -> Option<Arc<crate::ssh::Session>> {
    let ssh = names.iter().any(|n| spelled(n) == "ssh");
    match ssh && front_of(names) == Front::Elsewhere {
        true => sessions.of(fs, group, calls, beat),
        false => None,
    }
}

/// What one `lstat` found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stat {
    Dir,
    Link,
    /// A file, or anything that is not a folder or a link.
    File,
    /// `ENOENT` or `ENOTDIR`.
    Missing,
    /// Anything else: `EACCES`, `EPERM`, `EIO`, a name the call refused.
    Other,
}

/// **The worker's view of the system**: the seam the tests fake, with a
/// file system and a mount table of their own and a call that never
/// returns.
pub trait Fs: Send {
    /// `lstat` of `path`.
    fn lstat(&mut self, path: &[u8]) -> Stat;
    /// What the link at `path` holds.
    fn readlink(&mut self, path: &[u8]) -> Option<Vec<u8>>;
    /// The mount table, or none when it cannot be read.
    fn mounts(&mut self) -> Option<Mounts>;
    /// The names of the program leading the group in front: argv[0]'s and
    /// its executable's (`fg.rs`).
    fn program(&mut self, group: Pid) -> Vec<String>;
    /// **The folder `group`'s leader is in**, an absolute path, or none
    /// when it cannot be read (`fg::dir`): what a relative path on the
    /// alternate screen is read against.
    fn folder(&mut self, _group: Pid) -> Option<Vec<u8>> {
        None
    }
    /// **When `group`'s leader started**, or none when it cannot be read
    /// (`fg::started`): what tells a reused group number from the process
    /// that had it.
    fn started(&mut self, _group: Pid) -> Option<StartTime> {
        None
    }
    /// Run on the worker's thread before anything else.
    fn prepare(&mut self) {}
    /// **Whether an app opens `address`**, a fixed address of one kind of
    /// link ([`crate::paths::address`]), asked without opening anything.
    /// Unknown where the system cannot be asked: on Linux, by design.
    fn handler(&mut self, _address: &str) -> Handler {
        Handler::Unknown
    }
    /// `PATH`, which a command's program is looked up along.
    fn path_var(&mut self) -> Option<std::ffi::OsString> {
        None
    }
    /// **Every argument of `group`'s leader** (`fg::argv`), or none when
    /// they cannot be read: what the ssh in front connects with.
    fn argv(&mut self, _group: Pid) -> Option<Vec<std::ffi::OsString>> {
        None
    }
    /// **`ssh -G`'s printout for `asked`** ([`crate::ssh::resolve_argv`]),
    /// run as `group`'s leader's own program; none when it does not run, or
    /// ends in a failure.
    fn resolve(&mut self, _group: Pid, _asked: &[std::ffi::OsString]) -> Option<Vec<u8>> {
        None
    }
}

/// **When the thread taking the reads asks for the foreground group**:
/// at a read [`GROUP_EVERY`] or more after it last did, and at every read
/// carrying an `OSC 8` link's introducer or a `file://` URI in its text,
/// whatever the pace says: a file link takes its origin from who printed
/// it ([`crate::paths::Origin`]), asked at the read because the program may
/// be gone by the time the bytes are parsed.
#[derive(Clone, Debug, Default)]
pub struct Sampler {
    last: Option<std::time::Instant>,
    /// The last bytes of the reads so far, at most [`CARRY`]: an
    /// introducer or a `file://` split across two reads is still seen.
    tail: [Option<u8>; CARRY],
    /// The group the last read that asked was taken with, and what the
    /// process watch had named by then: the next read's "before"
    /// ([`Sampler::writers`]).
    previous: Option<Pid>,
    previous_watched: Watched,
    /// The shell's command marks, scanned at every read
    /// ([`crate::ran::Commands`]), and what the last read was taken as.
    commands: crate::ran::Commands,
    marked: crate::ran::Ran,
    /// The marks the last read carried, where each ended
    /// ([`crate::ran::Timeline`]).
    marks: crate::ran::Timeline,
    /// Where the last read is cut ([`crate::cut::Plan`]).
    cut: crate::cut::Cut,
    /// Whether the last read carried the shell's own `D`, which ends the
    /// command the group before was taken under once that read's writers
    /// are built ([`Sampler::writers`], [`Sampler::unasked`]).
    ended: Ended,
}

/// **Whether the last read ended a command at the shell's `D`**, to be
/// forgotten after its own writers are built: the `D` read keeps the group
/// before and the watch's verdict for its own rows, and the reads after
/// it are not that program's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Ended {
    #[default]
    No,
    AtD,
}

/// **Whether the process watch named a program not on this machine** since
/// the reader last took it (`fg::Watch`, macOS): a job that started, wrote
/// and ended between two reads, which no sample of the group in front saw.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Watched {
    #[default]
    Nothing,
    Elsewhere,
}

impl Watched {
    /// Either's verdict: not local if either is.
    fn or(self, other: Watched) -> Watched {
        match (self, other) {
            (Watched::Nothing, Watched::Nothing) => Watched::Nothing,
            _ => Watched::Elsewhere,
        }
    }
}

/// **Where the process watch leaves its verdict for the reader** (macOS's
/// `fg::Watch`): raised on its thread when an exec, or a child found at a
/// fork, is named not local, and taken -- read and cleared -- by the
/// thread taking the reads, at a read that asks for the group. One flag and
/// no pid: the watch has already named the program, and a pid handed on
/// could be gone, or another program's, by the time it was named again.
#[derive(Clone, Default, Debug)]
pub struct WatchCell(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl WatchCell {
    /// A program not on this machine was seen.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn raise(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }

    /// What was seen since the last take, cleared.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn take(&self) -> Watched {
        match self.0.swap(false, std::sync::atomic::Ordering::AcqRel) {
            true => Watched::Elsewhere,
            false => Watched::Nothing,
        }
    }
}

/// **Which surfaces' lines a part of a drain files** ([`part_spans`]): both;
/// the primary's alone, where a later part's clear took the alternate
/// screen's text; or both, where its own clear did, the alternate screen's
/// from the clear on -- not the line it began on, numbered before it.
/// **Where a run of a drain's parts ends** (`Probes::drain_segment`): at a
/// page-width request's re-cut that is followed, or with the batch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RunEnds {
    AtRecut,
    AtBatch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Keeps {
    Both,
    Primary,
    ClearedHere,
}

/// **The line a part began on, here, where it wrote no cell of it**
/// (`ReadRecord::start_row`) and its record names it nowhere off the path:
/// a climb back onto it, an erase or an edit reaching it from another row.
/// Fail closed: only where nothing else the record holds could have reached
/// it either -- no move of text on its screen at all, however exact the
/// band (a scroll that copies rows, a region's, the prompt's pin), no clear
/// of the alternate screen, no change of screen, and no renumbering.
fn untouched_start(paths: &Paths, lines: &PartLines, w: &Writers) -> Option<LineSpan> {
    let rec = w.record.filter(|r| r.renumbered().is_none())?;
    let start = rec.start();
    let off = rec.off_path(start.screen);
    let named = off.is_some_and(|s| s.first() <= start.line && start.line <= s.last());
    let moved = rec.moves().fill() == baud_core::Fill::Overflowed
        || rec.moves().iter().any(|m| m.screen() == start.screen);
    let cleared = rec.cleared() != baud_core::Cleared::No;
    let switched = rec.end().screen != start.screen;
    if rec.start_row() == baud_core::StartRow::Wrote || named || moved || cleared || switched {
        return None;
    }
    let line = lines.line(start.screen, start.line)?;
    Some(LineSpan::on(paths.surface(start.screen), line, line))
}

/// **The lines a part names on each surface, here** (N1): the hull of what
/// its record says it wrote off its forward path there
/// (`ReadRecord::off_path`) and the lines it began and ended on there,
/// placed on the app's numbers by `lines`, and each band of lines its moves
/// read and wrote there (C2), apart. The line it ended on is left out where
/// its record vouches that line holds nothing it wrote
/// ([`crate::vouch::Below`]): what a command's last line feed leaves to the
/// next writer. None for a part with no record, or one the core says it
/// renumbered -- its lines are on numbers no count says -- nor for a
/// surface whose lines cannot be placed.
fn part_spans(paths: &Paths, lines: &PartLines, w: &Writers, keeps: Keeps) -> Vec<LineSpan> {
    use baud_core::Screen;
    let Some(rec) = w.record.filter(|r| r.renumbered().is_none()) else {
        return Vec::new();
    };
    let below = w
        .end_row
        .zip(w.drew)
        .zip(w.matched)
        .map(|((e, d), m)| crate::vouch::Below::of(&rec, e, d, m));
    let mut out = Vec::new();
    for screen in [Screen::Normal, Screen::Alternate] {
        let alternate = screen == Screen::Alternate;
        if alternate && keeps == Keeps::Primary {
            continue;
        }
        let (start, end) = (rec.start(), rec.end());
        let begun = (start.screen == screen && !(alternate && keeps == Keeps::ClearedHere))
            .then_some(start.line);
        let ended = (end.screen == screen).then_some(end.line);
        let off = rec.off_path(screen);
        let named = [begun, ended]
            .into_iter()
            .flatten()
            .chain(off.into_iter().flat_map(|s| [s.first(), s.last()]));
        let Some((first, mut last)) = named.fold(None, |h, l| match h {
            None => Some((l, l)),
            Some((a, b)) => Some((l.min(a), l.max(b))),
        }) else {
            continue;
        };
        let clean =
            matches!(below, Some(crate::vouch::Vouch::Yes(b)) if b.line() == (screen, last));
        if clean && ended == Some(last) && last > first {
            last = last.minus(Rows::of_count(1)).unwrap_or(first);
        }
        if let (Some(a), Some(b)) = (lines.line(screen, first), lines.line(screen, last)) {
            out.push(LineSpan::on(paths.surface(screen), a, b));
        }
        // The lines its moves read and wrote, each band apart (C2).
        for m in rec.moves().iter() {
            let (baud_core::Move::Exact { screen: s, .. }
            | baud_core::Move::Within { screen: s, .. }) = m;
            let band = m.band();
            if let (true, Some(a), Some(b)) = (
                s == screen,
                lines.line(screen, band.first()),
                lines.line(screen, band.last()),
            ) {
                out.push(LineSpan::on(paths.surface(screen), a, b));
            }
        }
    }
    out
}

/// **Who may have written a read**: the group in front at the sample before
/// it, the group in front just after it, and whether the process watch
/// named a program not on this machine in between. The read is local only
/// if every one of them is ([`Probes::read_taken`]): a program that wrote
/// and ended before the sample after its read is still named by the one
/// before, or by the watch.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Writers {
    pub before: Option<Pid>,
    pub at: Option<Pid>,
    pub watched: Watched,
    /// What the shell's marks say of the read ([`crate::ran::Ran`]): a
    /// command named not local, one named here, or the shell's own.
    pub marked: crate::ran::Ran,
    /// Whether the part fed a folder report the core acted on
    /// ([`crate::ran::Folder`]), judged once its record is taken by what
    /// was open as the last of them came: none before.
    pub folder: crate::ran::Folder,
    /// The shell's marks the read carried, where each ended
    /// ([`crate::ran::Timeline`]), cut to the part fed (`reads::Part`).
    pub marks: crate::ran::Timeline,
    /// Whether the part's marks are the core's (`vouch::Matched`), read
    /// with its record, and then whether the window's are
    /// (`vouch::Latch`, [`Probes::drain_taken`]): none before.
    pub matched: Option<crate::vouch::Vouch<crate::vouch::Matched>>,
    /// The links the pen held as the part of it was fed
    /// ([`crate::paths::Pens`]).
    pub pens: crate::paths::Pens,
    /// Where the read is cut at the shell's `D`, or, for a part after it,
    /// where it was ([`crate::cut::Cut`]).
    pub cut: crate::cut::Cut,
    /// What the core did as it ran the part fed, from `Term::take_record`:
    /// none on a read before it is fed, or on a part held.
    pub record: Option<baud_core::ReadRecord>,
    /// The primary's window-top line as the part fed ended, read with its
    /// record (`reads::Taken`): what its primary lines are placed against.
    pub top: Option<baud_domain::LineIdx>,
    /// Whether the cursor's row held cells as the part fed ended, read with
    /// its record: what the line it ended on is judged by
    /// (`vouch::Below`).
    pub end_row: Option<crate::vouch::EndRow>,
    /// Whether the part fed placed an image, read with its record.
    pub drew: Option<crate::vouch::Drew>,
    /// **The re-cuts a page-width request in the part made**, told line by
    /// line (`reads::Taken`): what [`Probes::drain_recut`] carries the
    /// lines through between the parts before and after it.
    pub recut: Option<crate::paths::RecutAt>,
    /// **The line the part began on, where it wrote no cell of it**
    /// (`ReadRecord::start_row`) and its record names it nowhere off the
    /// path: placed here by [`Probes::drain_taken`], none before. Filed
    /// with the part's other lines, as a link opened there needs; never
    /// held for it (G1), so a command whose first read only feeds off the
    /// prompt line leaves a path typed there lit.
    pub untouched: Option<LineSpan>,
}

/// **The folder verdict of a part** (`ran::Folder`): none if the core
/// acted on no folder report in it; the last one's, judged by what the
/// shell's marks left open as it came, where the marks are the core's
/// (`vouch::Matched`) and the part ran under UTF-8 with no report past
/// what the record keeps; refused otherwise, as a report sent while a
/// command named not local ran is.
fn folder_of(rec: &baud_core::ReadRecord, marks: &crate::ran::Timeline) -> crate::ran::Folder {
    use crate::ran::{Folder, Left};
    let Some(last) = rec.folders().iter().last() else {
        return Folder::Unreported;
    };
    let vouched = rec.folders().fill() == baud_core::Fill::Room
        && matches!(
            crate::vouch::Matched::of(rec, marks),
            crate::vouch::Vouch::Yes(_)
        );
    match (vouched, marks.open_at(last.at)) {
        (true, Left::Closed | Left::Here) => Folder::Reported,
        (true, Left::Remote) | (false, _) => Folder::FromRemote,
    }
}

/// **A read's writers, cut to a part of its bytes**: the marks the part
/// fed, and those of a part held.
impl crate::reads::Part for Writers {
    fn part(self, from: baud_core::BytePos, to: baud_core::BytePos) -> Writers {
        Writers {
            marks: self.marks.part(from, to),
            ..self
        }
    }
    fn rest(self, from: baud_core::BytePos) -> Writers {
        let cut = match self.cut {
            crate::cut::Cut::Plan(p) => p
                .rest(from)
                .map_or(crate::cut::Cut::None, crate::cut::Cut::Plan),
            c => c,
        };
        Writers {
            marks: self.marks.rest(from),
            cut,
            ..self
        }
    }
    fn under(self, pens: crate::paths::Pens) -> Writers {
        Writers { pens, ..self }
    }
    fn recorded(self, taken: Option<crate::reads::Taken>) -> Writers {
        Writers {
            folder: taken.map_or(crate::ran::Folder::Unreported, |t| {
                folder_of(&t.record, &self.marks)
            }),
            matched: taken.map(|t| crate::vouch::Matched::of(&t.record, &self.marks)),
            record: taken.map(|t| t.record),
            top: taken.map(|t| t.top),
            end_row: taken.map(|t| t.end_row),
            drew: taken.map(|t| t.drew),
            recut: taken.and_then(|t| t.recut),
            ..self
        }
    }
    fn matched(&self, rec: &baud_core::ReadRecord) -> crate::vouch::Vouch<crate::vouch::Matched> {
        crate::vouch::Matched::of(rec, &self.marks)
    }
    fn plan(&self) -> Option<crate::cut::Plan> {
        match self.cut {
            crate::cut::Cut::Plan(p) => Some(p),
            crate::cut::Cut::None | crate::cut::Cut::After(_) => None,
        }
    }
    fn before_cut(self) -> Writers {
        Writers {
            cut: crate::cut::Cut::None,
            ..self
        }
    }
    fn pre_cut(self, vouch: crate::vouch::Vouch<crate::vouch::CutAt>) -> Writers {
        match self.cut {
            crate::cut::Cut::Plan(p) => Writers {
                cut: crate::cut::Cut::Plan(p.vouched(vouch)),
                ..self
            },
            crate::cut::Cut::None | crate::cut::Cut::After(_) => self,
        }
    }
    fn uncut(self) -> Writers {
        Writers {
            cut: crate::cut::Cut::None,
            ..self
        }
    }
    /// The shell's from the cut on: what the bytes after the `D` say, no
    /// group sampled before the read and no watch's verdict, the uncut
    /// writers kept for a cut whose line cannot be numbered.
    fn after_cut(self, spot: crate::cut::Spot) -> Writers {
        let Some(plan) = self.plan() else {
            return self;
        };
        Writers {
            before: None,
            watched: Watched::Nothing,
            marked: plan.after(),
            cut: crate::cut::Cut::After(crate::cut::Mark {
                spot,
                uncut: crate::cut::Uncut {
                    marked: self.marked,
                    before: self.before,
                    watched: self.watched,
                },
            }),
            ..self
        }
    }
}

impl Writers {
    /// One group, sampled at the read, and nothing before it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn at(group: Option<Pid>) -> Writers {
        Writers {
            before: None,
            at: group,
            watched: Watched::Nothing,
            marked: crate::ran::Ran::Unmarked,
            folder: crate::ran::Folder::Unreported,
            marks: crate::ran::Timeline::default(),
            matched: None,
            pens: crate::paths::Pens::default(),
            cut: crate::cut::Cut::None,
            record: None,
            top: None,
            end_row: None,
            drew: None,
            untouched: None,
            recut: None,
        }
    }
}

/// Whether a read carried an `OSC 8` link's introducer or a plain `file://` URI.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Linked {
    #[default]
    No,
    Yes,
}

/// Whether a read asks for the foreground group, and whether it carried a
/// link.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Asks {
    No,
    Yes(Linked),
}

impl Sampler {
    /// **The sampler for a window whose shell baud handed `token`**: the
    /// marks are read against that token alone ([`crate::ran::Commands::new`]).
    pub fn for_shell(token: Option<&crate::ran::ShellToken>) -> Sampler {
        Sampler {
            commands: crate::ran::Commands::new(token),
            ..Sampler::default()
        }
    }

    /// **The writers of a read that asked**, from `at`, the group sampled
    /// just after it, and `watched`, the process watch's verdict taken at
    /// it: the group before is the one the last read that asked was taken
    /// with, a group the kernel names, and a verdict counts for the next
    /// read that asks as well, so a read the pace asked for cannot use it up
    /// before the link's read comes.
    pub fn writers(&mut self, at: Option<Pid>, watched: Watched) -> Writers {
        let w = Writers {
            before: self.previous,
            at,
            watched: watched.or(self.previous_watched),
            marked: self.marked,
            folder: crate::ran::Folder::Unreported,
            marks: self.marks,
            matched: None,
            pens: crate::paths::Pens::default(),
            cut: self.cut,
            record: None,
            top: None,
            end_row: None,
            drew: None,
            untouched: None,
            recut: None,
        };
        self.forget_at_d();
        if at.is_some() {
            self.previous = at;
        }
        self.previous_watched = watched;
        w
    }

    /// **The writers of a read that did not ask**: the group the last read
    /// that asked was taken with, as the one before it, which a read inside
    /// the pace is taken to be until the next sample says otherwise, no
    /// group after it and no watch's verdict, which wait for a read that
    /// asks; and what the shell's marks say of it, what it did to its
    /// cursor's line and whether it reported a folder, which are this
    /// read's alone. So every read's rows are filed under some writer.
    pub fn unasked(&mut self) -> Writers {
        let w = Writers {
            before: self.previous,
            marked: self.marked,
            marks: self.marks,
            cut: self.cut,
            ..Writers::default()
        };
        self.forget_at_d();
        w
    }

    /// After a read that carried the shell's own `D` has its writers: the
    /// group before and the watch's verdict are that command's, and no
    /// read after the `D` is taken with them.
    fn forget_at_d(&mut self) {
        if self.ended == Ended::AtD {
            self.previous = None;
            self.previous_watched = Watched::Nothing;
            self.ended = Ended::No;
        }
    }

    /// Whether the read at `now` asks, by the pace alone.
    pub fn due(&mut self, now: std::time::Instant) -> bool {
        let due = self
            .last
            .is_none_or(|l| now.duration_since(l) >= GROUP_EVERY);
        if due {
            self.last = Some(now);
        }
        due
    }

    /// **Whether the read of `bytes` at `now` asks**: when [`Sampler::due`]
    /// says so, or when the bytes carry `ESC ] 8 ;` or its 8-bit form `9D 8
    /// ;`, or `file://` in any case, with the bytes the reads before them
    /// ended on.
    pub fn read(&mut self, now: std::time::Instant, bytes: &[u8]) -> Asks {
        self.marked = self.commands.read(bytes);
        // Where each mark ended: a part's are checked against the core's
        // record of it, and a folder report it fed is judged by what was
        // open as it came (`Writers::recorded`).
        self.marks = self.commands.marks();
        // Where the read is cut at the shell's `D`: taken as it is fed,
        // where the core's records vouch for it (`reads::paced`).
        self.cut = self
            .commands
            .cut()
            .and_then(|(d, after)| crate::cut::Plan::of(bytes, d, after))
            .map_or(crate::cut::Cut::None, crate::cut::Cut::Plan);
        // The shell's own `D` ends the command the group sampled before was
        // taken under: what the reads after it wrote is not that program's,
        // though what this read wrote before it may be, so it is forgotten
        // once this read's writers are built.
        self.ended = match self.commands.shells_d() {
            crate::ran::ShellsD::Yes => Ended::AtD,
            crate::ran::ShellsD::No => Ended::No,
        };
        // The tail, then the read's last bytes: what the next read follows.
        let mut seam = [0u8; 2 * CARRY];
        let mut n = 0;
        for &b in self.tail.iter().flatten() {
            seam[n] = b;
            n += 1;
        }
        let linked = match linked(&seam[..n], bytes) {
            true => Linked::Yes,
            false => Linked::No,
        };
        for &b in &bytes[bytes.len() - bytes.len().min(CARRY)..] {
            seam[n] = b;
            n += 1;
        }
        self.tail = [None; CARRY];
        let kept = n.min(CARRY);
        for (slot, &b) in self.tail[CARRY - kept..].iter_mut().zip(&seam[n - kept..n]) {
            *slot = Some(b);
        }
        // A read with no group to be taken under -- the shell's `D` forgot
        // the one sampled before, and the pace has not asked since -- asks
        // all the same: one sample, where none would leave its lines named
        // by nothing and filed not known (W3).
        match (self.due(now), linked) {
            (true, l) => Asks::Yes(l),
            (false, Linked::Yes) => Asks::Yes(Linked::Yes),
            (false, Linked::No) if self.previous.is_none() => Asks::Yes(Linked::No),
            (false, Linked::No) => Asks::No,
        }
    }
}

/// The bytes a read's scan carries to the next: one fewer than `;file://`,
/// the longest thing it looks at.
const CARRY: usize = 7;

/// Whether `bytes`, after `tail`, hold an `OSC 8` introducer or a
/// `file://` URI: what one read holds, or what crosses from the tail into
/// its first bytes.
fn linked(tail: &[u8], bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(CARRY)];
    let mut seam = [0u8; 2 * CARRY];
    seam[..tail.len()].copy_from_slice(tail);
    seam[tail.len()..tail.len() + head.len()].copy_from_slice(head);
    let seam = &seam[..tail.len() + head.len()];
    holds_link(seam) || holds_link(bytes)
}

/// **Whether `b` holds an `OSC 8` introducer -- `ESC ] 8 ;` or `9D 8 ;` --
/// or a `file://` URI in any case**, in one pass over the bytes: a plain
/// `file` URI is read as the text's links are ([`crate::link::scheme_at`]),
/// and a click takes its line's origin as it does a marked one's
/// ([`crate::paths::PathsAt::printed`]). A `file://` is found by its
/// `:`, then the four bytes before and the two after, so `Makefile:12:`
/// and `profile:` ask nothing. One after a `;` is a control string's
/// parameter, `OSC 7`'s folder at every prompt, which no click reads, and
/// asks nothing either: `OSC 8`'s is asked for by its introducer.
///
/// Sixteen bytes a step: one compare finds the lanes holding `ESC`, `9D` or
/// `:`, and only those are looked at. The bytes past the last whole block
/// are looked at one by one.
fn holds_link(b: &[u8]) -> bool {
    use baud_domain::{LANE_COUNT, Printable, StreamByte, Vectorized};
    let s = StreamByte::slice_from(b);
    let mut n = 0;
    while n + LANE_COUNT <= b.len() {
        let mut lanes = Vectorized::<Printable>::load(&s[n..]).any_of_three(
            StreamByte::from_stream(0x1b),
            StreamByte::from_stream(0x9d),
            StreamByte::from_stream(b':'),
        );
        while let Some(l) = lanes.first() {
            if link_at(b.split_at(n + l.index() + 1)) {
                return true;
            }
            lanes = lanes.after_first();
        }
        n += LANE_COUNT;
    }
    (n..b.len()).any(|at| matches!(b[at], 0x1b | 0x9d | b':') && link_at(b.split_at(at + 1)))
}

/// Whether the `ESC`, `9D` or `:` that ends `head` begins an `OSC 8`
/// introducer or is a `file://` URI's colon, `rest` being what follows it.
fn link_at((head, rest): (&[u8], &[u8])) -> bool {
    let Some((&mark, before)) = head.split_last() else {
        return false;
    };
    match mark {
        0x1b => rest.starts_with(b"]8;"),
        0x9d => rest.starts_with(b"8;"),
        _ => {
            let name = before
                .len()
                .checked_sub(4)
                .map(|s| (&before[s..], s.checked_sub(1).map(|q| before[q])));
            name.is_some_and(|(name, before)| {
                name.eq_ignore_ascii_case(b"file") && before != Some(b';')
            }) && rest.starts_with(b"//")
        }
    }
}

/// How mount points are compared to a path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fold {
    /// Byte for byte: Linux.
    #[cfg(any(target_os = "linux", test))]
    Exact,
    /// As a file system that ignores case may match them: ASCII without
    /// case, and two names that both hold bytes outside ASCII taken as the
    /// same, since a fold of those is not done here. Wider than any one
    /// volume, so it refuses more, never less: macOS.
    #[cfg(any(target_os = "macos", test))]
    Ascii,
}

/// **The mount points a walk must not step onto**: every one not on this
/// machine's own storage.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mounts {
    refused: Vec<Vec<u8>>,
    fold: Fold,
}

/// Where the automounter's network maps sit by convention, refused whatever
/// the table says: a printed `/net/host/x` must never reach a lookup, even
/// before the automounter has put its mount there.
const NEVER: [&[u8]; 2] = [b"/net", b"/Network"];

impl Mounts {
    pub fn new(fold: Fold) -> Mounts {
        Mounts {
            refused: NEVER.iter().map(|m| m.to_vec()).collect(),
            fold,
        }
    }

    /// A mount point refused, and everything under it.
    pub fn refuse(&mut self, on: &[u8]) {
        self.refused.push(on.to_vec());
    }

    /// Whether `path` is a refused mount point or under one, compared a
    /// component at a time: `/network2` is not under `/net`.
    pub fn refuses(&self, path: &[u8]) -> bool {
        self.refused.iter().any(|m| under(path, m, self.fold))
    }
}

fn components(p: &[u8]) -> impl Iterator<Item = &[u8]> {
    p.split(|&b| b == b'/').filter(|c| !c.is_empty())
}

fn same(a: &[u8], b: &[u8], fold: Fold) -> bool {
    match fold {
        #[cfg(any(target_os = "linux", test))]
        Fold::Exact => a == b,
        #[cfg(any(target_os = "macos", test))]
        Fold::Ascii => a.eq_ignore_ascii_case(b) || (!a.is_ascii() && !b.is_ascii()),
    }
}

/// Whether `path` is `mount` or under it.
fn under(path: &[u8], mount: &[u8], fold: Fold) -> bool {
    let mut p = components(path);
    components(mount).all(|m| p.next().is_some_and(|c| same(c, m, fold)))
}

/// Whether a file system says it is on this machine's own storage:
/// `MNT_LOCAL` on macOS.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Storage {
    Local,
    Elsewhere,
}

/// **Whether a macOS file system is refused**: not `MNT_LOCAL`, or of a type
/// that reaches another machine or a process -- the automounter's, NFS, SMB,
/// AFP, WebDAV, FTP and every FUSE type -- whatever it says of itself.
#[cfg(any(target_os = "macos", test))]
pub fn darwin_refuses(local: Storage, fstype: &[u8]) -> bool {
    const ELSEWHERE: [&[u8]; 8] = [
        b"autofs", b"nfs", b"smbfs", b"afpfs", b"webdav", b"ftp", b"cifs", b"fuse",
    ];
    local == Storage::Elsewhere
        || ELSEWHERE
            .iter()
            .any(|t| fstype.windows(t.len()).any(|w| w.eq_ignore_ascii_case(t)))
}

/// **The Linux file system types taken as this machine's own storage**:
/// disks, memory and the overlay a container's root is. Every other type is
/// refused -- NFS, CIFS, FUSE (`fuse.sshfs` among them), the automounter,
/// `proc` and `sysfs` -- because mountinfo says nothing of where a file
/// system is, and a type not on this list may be anywhere.
#[cfg(any(target_os = "linux", test))]
pub const LINUX_LOCAL: [&str; 21] = [
    "ext2", "ext3", "ext4", "xfs", "btrfs", "f2fs", "vfat", "exfat", "ntfs", "ntfs3", "tmpfs",
    "overlay", "squashfs", "erofs", "iso9660", "zfs", "bcachefs", "jfs", "reiserfs", "nilfs2",
    "devtmpfs",
];

/// **`/proc/self/mountinfo`'s mount points refused**, from its lines: "36 35
/// 98:0 /mnt1 /mnt2 rw,noatime master:1 - ext3 /dev/root rw", the mount point
/// the fifth field and the type the first after the `-` that ends the
/// optional fields (`[[source: linux-man-proc-pid-mountinfo]]`). A mount
/// point's space, tab, newline and backslash are written as octal escapes,
/// `\040` and the rest (`[[source: linux-man-getmntent-3]]`). None for a
/// table with no root, or none at all, which refuses every path.
#[cfg(any(target_os = "linux", test))]
pub fn mountinfo(text: &[u8]) -> Option<Mounts> {
    let mut m = Mounts::new(Fold::Exact);
    let mut root = false;
    for line in text.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
        let fields: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
        let point = unescape(fields.get(4)?);
        let dash = fields.iter().position(|f| *f == b"-")?;
        let fstype = fields.get(dash + 1)?;
        let main = fstype.split(|&b| b == b'.').next().unwrap_or(fstype);
        let local = fstype.len() == main.len() && LINUX_LOCAL.iter().any(|t| t.as_bytes() == main);
        if point == b"/" {
            root = true;
        }
        if !local {
            m.refuse(&point);
        }
    }
    root.then_some(m)
}

/// A mount point's octal escapes undone.
#[cfg(any(target_os = "linux", test))]
fn unescape(f: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(f.len());
    let mut i = 0;
    while i < f.len() {
        let oct = |b: u8| (b'0'..=b'7').contains(&b);
        if f[i] == b'\\' && f.len() >= i + 4 && f[i + 1..i + 4].iter().all(|&b| oct(b)) {
            let v = f[i + 1..i + 4]
                .iter()
                .fold(0u32, |a, &b| a * 8 + u32::from(b - b'0'));
            out.push(u8::try_from(v).unwrap_or(b'?'));
            i += 4;
        } else {
            out.push(f[i]);
            i += 1;
        }
    }
    out
}

/// **One path walked**, a component at a time from `/`: each next component
/// checked against `mounts` before it is looked up, a link read and followed
/// by hand, `..` taken back over what was walked, and the calls made counted
/// in `calls`. `beat` is called as each call returns.
pub fn walk(
    fs: &mut dyn Fs,
    mounts: &Mounts,
    path: &[u8],
    calls: &mut u64,
    beat: &dyn Fn(),
) -> Probe {
    if path.first() != Some(&b'/') {
        return Probe::Unknown;
    }
    // What is left, last first; an empty name is a `/` (a trailing one says
    // the path names a folder).
    let mut rest: Vec<Vec<u8>> = path[1..]
        .split(|&b| b == b'/')
        .rev()
        .map(<[u8]>::to_vec)
        .collect();
    let mut done: Vec<Vec<u8>> = Vec::new();
    let mut followed = 0;
    let mut next = Vec::new();
    while let Some(c) = rest.pop() {
        if c.is_empty() || c == b"." {
            continue;
        }
        if c == b".." {
            done.pop();
            continue;
        }
        next.clear();
        for d in &done {
            next.push(b'/');
            next.extend_from_slice(d);
        }
        next.push(b'/');
        next.extend_from_slice(&c);
        if mounts.refuses(&next) {
            return Probe::Unknown;
        }
        let stat = fs.lstat(&next);
        *calls += 1;
        beat();
        match stat {
            Stat::Missing => return Probe::Missing,
            Stat::Other => return Probe::Unknown,
            Stat::Dir => done.push(c),
            Stat::File => {
                // A file with more to walk is `ENOTDIR`, a trailing `/` too.
                return match rest.is_empty() {
                    true => Probe::Exists,
                    false => Probe::Missing,
                };
            }
            Stat::Link => {
                followed += 1;
                if followed > LINKS {
                    return Probe::Unknown;
                }
                let target = fs.readlink(&next);
                *calls += 1;
                beat();
                let Some(target) = target.filter(|t| !t.is_empty()) else {
                    return Probe::Unknown;
                };
                if target.first() == Some(&b'/') {
                    done.clear();
                }
                rest.extend(target.split(|&b| b == b'/').rev().map(<[u8]>::to_vec));
            }
        }
    }
    Probe::Exists
}

/// **A command's program, looked up along `PATH` as `execvp` looks**, each
/// candidate by [`walk`]: a name with a `/` is the path it names, if
/// absolute; a bare name is each folder of `path_var` in turn.
/// Found at the first file of the name; missing only when every folder's
/// walk said gone; unknown where any walk could not tell, a folder is not
/// absolute, or the name is relative. `prog` and `path_var` are what a
/// `[link.*]` rule's program and its `PATH` are read as.
pub fn command(
    fs: &mut dyn Fs,
    mounts: &Mounts,
    (prog, path_var): (&str, Option<&std::ffi::OsStr>),
    calls: &mut u64,
    beat: &dyn Fn(),
) -> Handler {
    use std::os::unix::ffi::OsStrExt;
    let argv0 = prog.as_bytes();
    let path = path_var.map(std::ffi::OsStr::as_bytes);
    let found = |p: Probe| match p {
        Probe::Exists => Handler::Found,
        Probe::Missing => Handler::Missing,
        Probe::Unknown => Handler::Unknown,
    };
    if argv0.is_empty() {
        return Handler::Unknown;
    }
    if argv0.contains(&b'/') {
        return match argv0.first() {
            Some(b'/') => found(walk(fs, mounts, argv0, calls, beat)),
            _ => Handler::Unknown,
        };
    }
    let Some(path) = path else {
        return Handler::Unknown;
    };
    let mut answer = Handler::Missing;
    for dir in path.split(|&b| b == b':') {
        if dir.first() != Some(&b'/') {
            answer = Handler::Unknown;
            continue;
        }
        let mut full = dir.to_vec();
        if full.last() != Some(&b'/') {
            full.push(b'/');
        }
        full.extend_from_slice(argv0);
        match walk(fs, mounts, &full, calls, beat) {
            Probe::Exists => return Handler::Found,
            Probe::Missing => {}
            Probe::Unknown => answer = Handler::Unknown,
        }
    }
    answer
}

/// Programs whose output is another machine's: a path they print names a
/// file there, so nothing is looked up while one is in front.
pub const ELSEWHERE: [&str; 24] = [
    // A shell on another machine.
    "ssh",
    "slogin",
    "autossh",
    "mosh",
    "mosh-client",
    "telnet",
    "rsh",
    "rlogin",
    // Eternal Terminal's client: a shell on another machine.
    "et",
    // Teleport's `tsh ssh`: a shell on another machine.
    "tsh",
    // kitty's `kitten ssh`; its other subcommands print no path of this
    // machine's that a shell would not.
    "kitten",
    // A container's own file system: `docker run`, `docker exec`.
    "docker",
    "podman",
    // A pod's file system, on another machine.
    "kubectl",
    // A Lima virtual machine's shell, whose root is not this machine's.
    "limactl",
    "lima",
    // An OrbStack machine's shell.
    "orb",
    "orbctl",
    // A Multipass virtual machine's shell.
    "multipass",
    // `vagrant ssh`: a virtual machine's shell.
    "vagrant",
    // `gcloud compute ssh`, `gcloud cloud-shell ssh`: another machine.
    "gcloud",
    // A container that shares the home folder and not the rest: its `/usr`
    // and `/etc` are the image's.
    "distrobox",
    "toolbox",
    // `virsh console`: a virtual machine's console.
    "virsh",
];

/// **Terminal multiplexers**: each runs its panes on ptys of its own and
/// draws them into this one, so the folder `OSC 7` gave baud is the shell's
/// that started it, not a pane's, and a `./` gets no line. herdr keeps each
/// pane's `OSC 7` to itself and writes none to this terminal (read in its
/// source, `src/pane/osc.rs`); tmux, screen, zellij and the rest keep theirs
/// too. A path a pane prints may be another machine's if the pane runs ssh,
/// which baud cannot see: `/` and `~/` mean "on this machine", and the bar
/// says so.
pub const MULTIPLEXERS: [&str; 8] = [
    "herdr", "tmux", "tmate", "screen", "zellij", "abduco", "dtach", "byobu",
];

/// A program's name as the lists spell it: a wrapper's leading `.` and
/// `-wrapped` (nix's), and tmux's `: client`, taken off.
fn spelled(name: &str) -> &str {
    let n = name.strip_prefix('.').unwrap_or(name);
    let n = n.strip_suffix("-wrapped").unwrap_or(n);
    n.split(':').next().unwrap_or(n)
}

/// **What the program in front is**, by its names: none read is not known
/// to be local.
pub fn front_of(names: &[String]) -> Front {
    if names.is_empty() {
        return Front::Elsewhere;
    }
    let any = |list: &[&str]| names.iter().any(|n| list.contains(&spelled(n)));
    if any(&ELSEWHERE) {
        Front::Elsewhere
    } else if any(&MULTIPLEXERS) {
        Front::Multiplexer
    } else {
        Front::Local
    }
}

/// What a batch is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Screen,
    Click,
    /// Name a group now, while it may still run; answered by no answer.
    Name,
    /// The programs a link opens with ([`Program`]).
    Programs,
}

/// A batch to the worker.
#[derive(Debug)]
pub struct Ask {
    pub id: Id,
    pub kind: Kind,
    pub group: Option<Pid>,
    /// The cursor's line when it was sent: rows to it were written under the
    /// program the worker names.
    pub cursor: LineOn,
    pub paths: Vec<Candidate>,
    /// Each group seen at a read since the last batch, with the lines it
    /// wrote, and the lines written past [`GROUPS`] of them.
    pub seen: Vec<(Pid, LineSpan)>,
    pub unnamed: Option<LineSpan>,
    /// For [`Kind::Programs`]: what to find a program for.
    pub programs: Vec<Program>,
}

/// The worker's answer to one batch.
#[derive(Debug)]
pub struct Answer {
    pub id: Id,
    pub kind: Kind,
    pub group: Option<Pid>,
    pub cursor: LineOn,
    /// What is in front now; none for a batch left for a newer one before
    /// it was taken, which names nothing.
    pub front: Option<Front>,
    /// What each group seen since the last batch was: named, or gone
    /// before it could be named; none for the lines past the groups kept.
    ///
    /// No lines: the worker's are the lines as they were read, and main
    /// files each verdict on its own copy, carried with the text since
    /// (`Probes::file_answered`), so a verdict has no lines as read to be
    /// filed on.
    pub seen: Vec<(Option<Pid>, crate::paths::Seen)>,
    pub probes: Vec<(Vec<u8>, Probe)>,
    /// For [`Kind::Programs`]: what each was found to be; none for a batch
    /// left for a newer one.
    pub programs: Option<Vec<(Program, Handler)>>,
    /// The folder of the group asked about, when it was named local and
    /// the folder could be read, and when its leader started, read with it:
    /// the key main compares, with no call of its own.
    pub folder: Option<ProgramDir>,
    pub started: Option<StartTime>,
    /// **The session of the ssh leading the group in front**, as the
    /// worker read it ([`Sessions`]): none when no ssh leads it, or one
    /// that is no session a file can be opened in.
    pub session: Option<Arc<crate::ssh::Session>>,
    /// **The session of each group in `seen` an ssh leads**, as the worker
    /// read it when it named the group, gone since or not: what the rows
    /// the group wrote are filed as printed by (`Paths::written_by`).
    pub sessions: Vec<(Pid, Arc<crate::ssh::Session>)>,
    pub calls: u64,
}

/// The answer to a batch a newer one replaced before it was taken: it
/// names nothing and looks nothing up, and it is answered so that every
/// batch sent has one.
fn left(ask: &Ask) -> Answer {
    Answer {
        id: ask.id,
        kind: ask.kind,
        group: ask.group,
        cursor: ask.cursor,
        front: None,
        seen: Vec::new(),
        probes: Vec::new(),
        programs: None,
        folder: None,
        started: None,
        session: None,
        sessions: Vec::new(),
        calls: 0,
    }
}

/// Two groups' lines as one, either none.
fn joined(a: Option<LineSpan>, b: Option<LineSpan>) -> Option<LineSpan> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.joined(b)),
        (a, b) => a.or(b),
    }
}

/// `h` held, where it is not already: the holds move only when it is new.
fn hold(held: &mut Tracked<Vec<(Option<Pid>, LineSpan)>>, h: (Option<Pid>, LineSpan)) {
    if !held.get().contains(&h) {
        held.edit().push(h);
    }
}

/// A newer screen batch in `newer`'s place: the groups `older` carried go
/// with it, so a batch left untaken drops none.
fn replace(slot: &mut Option<Ask>, mut newer: Ask, answers: &Sender<Answer>) -> bool {
    let Some(older) = slot.take() else {
        *slot = Some(newer);
        return true;
    };
    for (g, l) in &older.seen {
        match newer.seen.iter_mut().find(|e| e.0 == *g) {
            Some(e) => e.1 = e.1.joined(*l),
            None => newer.seen.push((*g, *l)),
        }
    }
    newer.unnamed = joined(newer.unnamed, older.unnamed);
    *slot = Some(newer);
    answers.send(left(&older)).is_ok()
}

/// A newer programs batch in `newer`'s place, the older answered as left.
fn replace_programs(slot: &mut Option<Ask>, newer: Ask, answers: &Sender<Answer>) -> bool {
    match slot.replace(newer) {
        Some(older) => answers.send(left(&older)).is_ok(),
        None => true,
    }
}

/// A batch's number: a newer one is a greater one.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Id(u64);

impl Id {
    fn next(self) -> Id {
        Id(self.0.wrapping_add(1))
    }
}

/// The worker's count of calls that have returned: one unchanged for
/// [`HANG`] with a batch out is a hang.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Beats(u64);

/// The core's count of bytes taken, the link reader's: a new one is a
/// read.
pub use crate::link::Taken;

/// **The worker**: its thread's two ends and its beat.
pub struct Worker {
    tx: Sender<Ask>,
    rx: Receiver<Answer>,
    beats: Arc<AtomicU64>,
}

impl Worker {
    /// The thread, started, with `fs` and `wake`, called after each answer
    /// is sent so a loop waiting on a descriptor hears of it.
    pub fn start(fs: Box<dyn Fs>, wake: Box<dyn Fn() + Send>) -> std::io::Result<Worker> {
        let (tx, asks) = mpsc::channel::<Ask>();
        let (answers, rx) = mpsc::channel::<Answer>();
        let beats = Arc::new(AtomicU64::new(0));
        let beat = Arc::clone(&beats);
        std::thread::Builder::new()
            .name("paths".into())
            .spawn(move || run(fs, &asks, &answers, &beat, &*wake))?;
        Ok(Worker { tx, rx, beats })
    }

    fn post(&self, ask: Ask) -> bool {
        self.tx.send(ask).is_ok()
    }

    fn beats(&self) -> Beats {
        Beats(self.beats.load(Ordering::Relaxed))
    }
}

/// The worker's loop: groups to name first, then clicks, in order, then the
/// newest programs batch, then the newest screen batch, which a newer one
/// replaces before it is taken and ends between two paths. Every click,
/// programs and screen batch sent gets one answer, a replaced one included.
fn run(
    mut fs: Box<dyn Fs>,
    asks: &Receiver<Ask>,
    answers: &Sender<Answer>,
    beats: &AtomicU64,
    wake: &dyn Fn(),
) {
    fs.prepare();
    let beat = || {
        beats.fetch_add(1, Ordering::Relaxed);
    };
    let mut screen: Option<Ask> = None;
    let mut programs: Option<Ask> = None;
    let mut clicks: std::collections::VecDeque<Ask> = std::collections::VecDeque::new();
    let mut groups: std::collections::VecDeque<Pid> = std::collections::VecDeque::new();
    let mut named: Named = Named::new();
    let mut sessions = Sessions::default();
    // The calls naming groups made, carried into the next answer's count.
    let mut naming = 0;
    // Where an ask goes; false once the driver is gone.
    let file = |a: Ask,
                screen: &mut Option<Ask>,
                programs: &mut Option<Ask>,
                clicks: &mut std::collections::VecDeque<Ask>,
                groups: &mut std::collections::VecDeque<Pid>|
     -> bool {
        match a.kind {
            Kind::Click => clicks.push_back(a),
            Kind::Name => groups.extend(a.group),
            Kind::Programs => {
                let answered = programs.is_some();
                if !replace_programs(programs, a, answers) {
                    return false;
                }
                if answered {
                    wake();
                }
            }
            Kind::Screen => {
                let answered = screen.is_some();
                if !replace(screen, a, answers) {
                    return false;
                }
                if answered {
                    wake();
                }
            }
        }
        true
    };
    loop {
        if screen.is_none() && programs.is_none() && clicks.is_empty() && groups.is_empty() {
            match asks.recv() {
                Ok(a) => {
                    if !file(a, &mut screen, &mut programs, &mut clicks, &mut groups) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        loop {
            match asks.try_recv() {
                Ok(a) => {
                    if !file(a, &mut screen, &mut programs, &mut clicks, &mut groups) {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        // A group is named as soon as it is seen: one that exits before the
        // batch could not be named then.
        while let Some(g) = groups.pop_front() {
            name_group(&mut *fs, &mut named, &mut sessions, g, &mut naming, &beat);
        }
        let Some(ask) = clicks
            .pop_front()
            .or_else(|| programs.take())
            .or_else(|| screen.take())
        else {
            continue;
        };
        let mut gone = false;
        let mut stop = || loop {
            match asks.try_recv() {
                Ok(a) if a.kind == Kind::Screen => {
                    gone |= !file(a, &mut screen, &mut programs, &mut clicks, &mut groups);
                    return true;
                }
                Ok(a) => gone |= !file(a, &mut screen, &mut programs, &mut clicks, &mut groups),
                Err(_) => return false,
            }
        };
        let mut got = answer(&mut *fs, ask, &mut named, &mut sessions, &beat, &mut stop);
        got.calls += core::mem::take(&mut naming);
        if gone || answers.send(got).is_err() {
            return;
        }
        wake();
    }
}

/// One batch answered: the program in front named, then each path walked
/// unless that program's output is not this machine's, stopping where
/// `stop` says a newer batch came.
fn answer(
    fs: &mut dyn Fs,
    ask: Ask,
    named: &mut Named,
    sessions: &mut Sessions,
    beat: &dyn Fn(),
    stop: &mut dyn FnMut() -> bool,
) -> Answer {
    let mut calls = 0;
    if ask.kind == Kind::Programs {
        let base = left(&ask);
        let found = programs(fs, ask.programs, &mut calls, beat);
        return Answer {
            programs: Some(found),
            calls,
            ..base
        };
    }
    // A group the worker named when it was seen, else named now: its names,
    // none for one gone by then, and the session of the ssh leading it. A
    // name kept for a leader that started at another time than the group's
    // leader now is another process's, the group number reused, and the
    // group is named again; a leader gone, whose start no longer reads,
    // keeps the name and the session it was seen with.
    let mut names_of_group = |g: Pid, calls: &mut u64| {
        let now = fs.started(g);
        *calls += 1;
        if let Some((then, n, s)) = named.get(&g) {
            let reused = matches!((then, now), (Some(a), Some(b)) if *a != b);
            if !reused {
                return (n.clone(), s.clone());
            }
        }
        let n = fs.program(g);
        *calls += 1;
        beat();
        let s = ssh_session(&mut *fs, &mut *sessions, g, &n, calls, beat);
        (n, s)
    };
    // A group seen at a read and gone before it was named is not known,
    // and so are the lines written past the groups kept: their rows fail
    // closed and touch no other row (`Paths::written_under`). The group in
    // front with no name is not known to be local, as `front_of` says.
    let mut printed_by = Vec::new();
    let mut seen: Vec<(Option<Pid>, crate::paths::Seen, LineSpan)> = ask
        .seen
        .iter()
        .map(|&(g, l)| {
            let (n, s) = names_of_group(g, &mut calls);
            printed_by.extend(s.map(|s| (g, s)));
            let wrote = match n.is_empty() {
                true => crate::paths::Seen::NotNamed,
                false => crate::paths::Seen::Named(front_of(&n)),
            };
            (Some(g), wrote, l)
        })
        .collect();
    seen.extend(ask.unnamed.map(|l| (None, crate::paths::Seen::NotNamed, l)));
    let front_names = ask.group.map(|g| names_of_group(g, &mut calls).0);
    let front = match &front_names {
        Some(n) => front_of(n),
        None => Front::Elsewhere,
    };
    // The ssh leading the group in front, by its names: its argv and
    // `ssh -G` are read here, on the worker's thread, and nowhere else.
    let ssh = front_names
        .as_ref()
        .is_some_and(|n| n.iter().any(|n| spelled(n) == "ssh"));
    let session = match ask.group {
        Some(g) if front == Front::Elsewhere && ssh => sessions.of(fs, g, &mut calls, beat),
        _ => {
            sessions.drop_kept();
            None
        }
    };
    for (g, _) in &ask.seen {
        if Some(*g) != ask.group {
            named.remove(g);
        }
    }
    // The folder of a group named local, read here on the worker's thread:
    // what a relative path on its full screen is read against. Its leader's
    // start time is read before and after it, and the folder is answered
    // only when both read and agree: a folder read across another process
    // taking the group's number, or under a start that cannot be read, is
    // no one's.
    let (folder, started) = match (front, ask.group) {
        (Front::Local, Some(g)) => {
            calls += 3;
            beat();
            let before = fs.started(g);
            let folder = fs.folder(g).map(ProgramDir);
            let after = fs.started(g);
            match (before, after) {
                (Some(b), Some(a)) if b == a => (folder, Some(a)),
                _ => (None, after),
            }
        }
        _ => (None, None),
    };
    // Whether this batch saw a writer `w` takes write the row `line`.
    let wrote_on = |line, w: &dyn Fn(crate::paths::Seen) -> bool| {
        seen.iter().any(|&(_, x, s)| w(x) && s.holds(line))
    };
    let not_local = |x: crate::paths::Seen| {
        matches!(
            x,
            crate::paths::Seen::NotNamed | crate::paths::Seen::Named(Front::Elsewhere)
        )
    };
    let muxer = |x: crate::paths::Seen| x == crate::paths::Seen::Named(Front::Multiplexer);
    let mut probes = Vec::new();
    if front != Front::Elsewhere && !ask.paths.is_empty() {
        let mounts = fs.mounts();
        calls += 1;
        beat();
        for c in ask.paths {
            // Printed on a row a program not on this machine, or not known,
            // wrote: that row only.
            if wrote_on(c.line, &not_local) {
                continue;
            }
            let muxed = front == Front::Multiplexer || wrote_on(c.line, &muxer);
            if muxed && c.form.relative() {
                continue;
            }
            let p = match &mounts {
                Some(m) => walk(fs, m, &c.path, &mut calls, beat),
                None => Probe::Unknown,
            };
            probes.push((c.path, p));
            if ask.kind == Kind::Screen && stop() {
                break;
            }
        }
    }
    Answer {
        id: ask.id,
        kind: ask.kind,
        group: ask.group,
        cursor: ask.cursor,
        front: Some(front),
        seen: seen.into_iter().map(|(g, f, _)| (g, f)).collect(),
        probes,
        programs: None,
        folder,
        started,
        session,
        sessions: printed_by,
        calls,
    }
}

/// **Each program found or not**: a kind of link at its fixed address, a
/// command along `PATH`, the mount table read once for the commands.
fn programs(
    fs: &mut dyn Fs,
    asked: Vec<Program>,
    calls: &mut u64,
    beat: &dyn Fn(),
) -> Vec<(Program, Handler)> {
    let mut mounts: Option<Option<Mounts>> = None;
    let mut path: Option<Option<std::ffi::OsString>> = None;
    let mut out = Vec::with_capacity(asked.len());
    for p in asked {
        let h = match &p {
            Program::Scheme(s) => match crate::paths::address(*s) {
                Some(a) => {
                    let h = fs.handler(a);
                    *calls += 1;
                    beat();
                    h
                }
                None => Handler::Unknown,
            },
            Program::Command(prog) => {
                let m = mounts.get_or_insert_with(|| {
                    let m = fs.mounts();
                    *calls += 1;
                    beat();
                    m
                });
                let v = path.get_or_insert_with(|| fs.path_var());
                match m {
                    Some(m) => command(fs, m, (prog, v.as_deref()), calls, beat),
                    None => Handler::Unknown,
                }
            }
        };
        out.push((p, h));
    }
    out
}

/// **When the next batch is due**, on the driver's clock.
#[derive(Clone, Copy, Debug)]
pub struct Pace<T> {
    bytes: Option<Taken>,
    last_read: Option<T>,
    /// When a batch was first owed, for anything.
    owed_since: Option<T>,
    /// When a batch was first owed for a path ([`Owes::Path`]): the only
    /// owing the shell's `D` hastens.
    path_since: Option<T>,
    /// The last batch that asked about a path: the floor's stamp.
    last_batch: Option<T>,
    /// The last screen scan a deadline met, sent or not, asking or only
    /// naming: what [`GRANT_GAP`] is kept after.
    last_sent: Option<T>,
    /// The window's shell's `D` the next batch is due at once for.
    grant: Grant<T>,
}

/// **The shell's `D` the pace holds** ([`Pace::ended`]): none, or one
/// minted by the drain whose clock this is, and how much of the floor it
/// may still lift.
#[derive(Clone, Copy, Debug)]
enum Grant<T> {
    None,
    Since(T, Lift),
}

/// **How many more scans a grant may pass the floor with**: two, the one
/// naming a listing's writer and the one asking about its paths, since
/// one batch names every writer the screen holds. Each scan a deadline
/// meets under the grant steps it ([`Pace::scanned`]); once spent, the
/// floor after the last batch that asked holds the grant's deadline as it
/// holds any other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lift {
    /// No scan yet: the one naming the writer may pass.
    Open,
    /// One scan passed: the one asking about the paths may.
    Named,
    /// Two passed: the floor holds the rest.
    Spent,
}

impl Lift {
    /// The lift after one more scan under it.
    fn step(self) -> Lift {
        match self {
            Lift::Open => Lift::Named,
            Lift::Named | Lift::Spent => Lift::Spent,
        }
    }
}

/// **A batch's deadline and why it is due**: no deadline exists without a
/// reason ([`Pace::due`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Due<T> {
    pub at: T,
    pub by: By,
    /// Whether the shell's `D` set the deadline the floor held, or set
    /// this one: a floor-held batch under a grant is counted apart.
    pub granted: Granted,
}

/// **Why a batch is due.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum By {
    /// The window's shell's `D` (`vouch::Ended`): at once.
    Ended,
    /// [`SETTLE`] after the last read: the quiet of output with no mark.
    Quiet,
    /// [`CEILING`] after the path was met, while output goes on.
    Ceiling,
    /// [`CEILING`] after the last batch that asked about a path.
    Floor,
}

/// Whether a live grant of the shell's `D` set a deadline before the floor
/// was laid over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Granted {
    Yes,
    No,
}

impl<T: Moment> Default for Pace<T> {
    fn default() -> Pace<T> {
        Pace {
            bytes: None,
            last_read: None,
            owed_since: None,
            path_since: None,
            last_batch: None,
            last_sent: None,
            grant: Grant::None,
        }
    }
}

impl<T: Moment> Pace<T> {
    /// The core's count of bytes taken, at `now`: a new byte is a read.
    pub fn saw(&mut self, bytes: Taken, now: T) {
        if self.bytes != Some(bytes) {
            self.bytes = Some(bytes);
            self.last_read = Some(now);
        }
    }

    /// **A batch is owed, for `owes`**, from `now` if it was not already:
    /// a row changed ([`Owes::Rows`]), or a scan met a path with no answer,
    /// or one on a row still to be named ([`Owes::Path`]).
    pub fn owe(&mut self, now: T, owes: Owes) {
        if self.owed_since.is_none() {
            self.owed_since = Some(now);
        }
        if owes == Owes::Path && self.path_since.is_none() {
            self.path_since = Some(now);
        }
    }

    /// **The window's shell ended its command**, in the drain whose clock
    /// is `now` (`Probes::drain_recut`): the next batch is due at once,
    /// not [`SETTLE`] after the last read, until a batch asks about a path
    /// or [`SETTLE`] after `now`. Stamped here, by the drain, and never by
    /// a turn after it: a `D` that owed nothing would otherwise be live
    /// from the next key's turn, and send its echo's partial path at once.
    /// A second `D` before this one is spent replaces it, its lift whole
    /// again: one chain to an asking batch per `D`.
    pub fn ended(&mut self, _proof: crate::vouch::Ended, now: T) {
        self.grant = Grant::Since(now, Lift::Open);
    }

    /// **A command opened after the shell's `D`, or a part's marks were
    /// not trusted**: the grant ends.
    pub fn lapse(&mut self) {
        self.grant = Grant::None;
    }

    /// **The batch's deadline**: [`Pace::due`]'s instant.
    pub fn due_at(&self) -> Option<T> {
        self.due().map(|d| d.at)
    }

    /// **The batch's deadline and why**: [`SETTLE`] after the last read
    /// ([`By::Quiet`]), but no later than [`CEILING`] after the path was
    /// met ([`By::Ceiling`]); at once while the window's shell's `D` holds
    /// ([`By::Ended`]), never sooner than [`GRANT_GAP`] after the last
    /// scan; and never sooner than [`CEILING`] after the last batch that
    /// asked about a path ([`By::Floor`]) -- but for a grant's deadline
    /// while its [`Lift`] is not spent, which passes the floor. None while
    /// nothing is owed.
    pub fn due(&self) -> Option<Due<T>> {
        let soon = self.soon()?;
        let lifts = soon.granted == Granted::Yes
            && matches!(self.grant, Grant::Since(_, Lift::Open | Lift::Named));
        Some(match self.last_batch.map(|b| b.after(CEILING)) {
            Some(floor) if floor > soon.at && !lifts => Due {
                at: floor,
                by: By::Floor,
                ..soon
            },
            Some(_) | None => soon,
        })
    }

    /// The deadline with no floor: the grant's while it holds and a path
    /// is owed, else [`SETTLE`] after the last read, but no later than
    /// [`CEILING`] after the batch was first owed. **A grant hastens only a
    /// batch owed for a path** -- one with no answer, or on a row its
    /// naming makes readable -- and judges its lapse by when that path was
    /// met: rows that changed alone, a prompt with no path at a held key,
    /// take the plain pace, and a row changed early does not carry a path
    /// met past the lapse into the grant.
    fn soon(&self) -> Option<Due<T>> {
        let since = self.owed_since?;
        let quiet = self.last_read.map_or(since, |r| r.after(SETTLE).max(since));
        let ceiling = since.after(CEILING);
        let plain = match quiet <= ceiling {
            true => Due {
                at: quiet,
                by: By::Quiet,
                granted: Granted::No,
            },
            false => Due {
                at: ceiling,
                by: By::Ceiling,
                granted: Granted::No,
            },
        };
        Some(match self.path_since.and_then(|p| self.granted(p)) {
            Some(at) if at < plain.at => Due {
                at,
                by: By::Ended,
                granted: Granted::Yes,
            },
            Some(_) | None => plain,
        })
    }

    /// **The grant's deadline, for a path met at `since`**: at once, but
    /// not before the drain that minted it nor sooner than [`GRANT_GAP`]
    /// after the last scan; none once lapsed, a path met [`SETTLE`] or
    /// more after the mint being the next output's and paced by its quiet.
    fn granted(&self, since: T) -> Option<T> {
        let Grant::Since(minted, _) = self.grant else {
            return None;
        };
        if since >= minted.after(SETTLE) {
            return None;
        }
        let at = since.max(minted);
        Some(self.last_sent.map_or(at, |s| at.max(s.after(GRANT_GAP))))
    }

    /// **The deadline was met at `now`**: the screen is scanned for the
    /// batch, and nothing is owed until a scan meets a path with no answer
    /// again. The scan is the last, which a grant's next keeps
    /// [`GRANT_GAP`] after, and it steps the grant's [`Lift`], whether a
    /// batch goes or the scan finds nothing to send: stamped where the
    /// floor is passed, not where a batch is sent, so a scan that sends
    /// nothing is spaced and capped like one that does. The grant is kept:
    /// a batch that only names its writer makes a listing's rows readable,
    /// and the batch after asks about them. **The bound, per tokened `D`**:
    /// at most two scans pass the floor, at most one of them asking about a
    /// path ([`Pace::sent`] spends the grant), each at least [`GRANT_GAP`]
    /// after the scan before; and one grant scans at most 20 times where no
    /// floor is live, [`SETTLE`] over [`GRANT_GAP`] after its drain.
    pub fn scanned(&mut self, now: T) {
        self.owed_since = None;
        self.path_since = None;
        self.last_sent = Some(now);
        if let Grant::Since(minted, lift) = self.grant {
            self.grant = Grant::Since(minted, lift.step());
        }
    }

    /// **A batch went at `now`, asking about `paths`**: only a batch that
    /// asked about a path holds the next one to [`CEILING`] after it, and
    /// spends the shell's `D`, its lift with it, so the next asking batch
    /// with no new `D` is held by the floor this one lays. One that only
    /// names the programs that wrote -- the first after a listing, whose
    /// rows are read once their writer is named, or a typed key's echo --
    /// holds nothing and spends nothing, so the batch its answer makes
    /// readable goes at its own deadline and not a second later. Under
    /// output that goes on, [`CEILING`] after the path was met still sends
    /// batches about once a second; batches that only name are never closer
    /// than [`SETTLE`], at most 7 in a second, where no grant holds, and
    /// never closer than [`GRANT_GAP`] where one does ([`Pace::scanned`]).
    pub fn sent(&mut self, now: T, paths: &[Candidate]) {
        if !paths.is_empty() {
            self.last_batch = Some(now);
            self.grant = Grant::None;
        }
    }
}

/// Batches out, and the last beat seen with the time it was seen.
#[derive(Clone, Copy, Debug)]
struct Flight<T> {
    beats: Beats,
    at: T,
}

/// The worker, as the driver has it.
enum Slot {
    Unstarted,
    Running(Worker),
    Closed,
}

/// **What a click on a printed path comes to**, once the worker has asked
/// again.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Clicked {
    /// There: reveal it, through `admit`.
    Exists(Vec<u8>),
    /// Gone: say so, and open nothing.
    Missing,
    /// It could not be asked: the worker closed, or not on this machine.
    Unchecked,
}

/// What a turn owes the driver.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Turn {
    /// A frame, for lines that changed.
    pub frame: Owed,
    pub clicked: Option<Clicked>,
}

/// What the five-second report counts.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts {
    /// Paths sent to be asked about, clicks included.
    pub asked: u64,
    /// The calls the worker made: lookups, link reads, the mount table and
    /// the program in front.
    pub calls: u64,
    pub batches: u64,
    /// Batches that asked about a path later than their own deadline, held
    /// by the floor after the last that did ([`By::Floor`]).
    pub held: u64,
    /// **Batches of either kind the floor held whose own deadline the
    /// window's shell's `D` set** ([`Granted::Yes`]): the grant's [`Lift`]
    /// spent, two scans having passed the floor under it with no batch
    /// asking. A listing, a listing again on its marks' frame, and a path
    /// typed then a listing within a second each pass the floor in at
    /// most two scans, so each reads 0 here: the witness the rig's rows for
    /// them hold to 0.
    pub held_granted: u64,
    /// The screen batches sent, by why each was due ([`By`]).
    pub by_ended: u64,
    pub by_quiet: u64,
    pub by_ceiling: u64,
    pub by_floor: u64,
    pub hangs: u64,
    /// **Reads whose record held more whole-line erasures than it has room
    /// for** (`ReadRecord::erased`): their lines kept every fact, failing
    /// closed (`Probes::erased`).
    pub erase_overflowed: u64,
    /// Runs of lines a read erased whole that its part's lines could not
    /// place here, which kept their facts.
    pub erase_unplaced: u64,
}

impl Counts {
    /// A screen batch sent, due `by`.
    fn sent_by(&mut self, by: By) {
        let n = match by {
            By::Ended => &mut self.by_ended,
            By::Quiet => &mut self.by_quiet,
            By::Ceiling => &mut self.by_ceiling,
            By::Floor => &mut self.by_floor,
        };
        *n += 1;
    }
}

/// What a turn reads and changes: the window's view, its lines and readers.
pub struct Seen<'a, 't> {
    pub term: &'a Term<'t>,
    pub scrolled: Scrolled,
    pub lines: &'a mut Lines,
    pub uris: &'a mut Uris,
    pub urls: Turned,
    pub paths: &'a mut Paths,
}

impl Seen<'_, '_> {
    /// The cursor's line, on the surface up.
    fn cursor_on(&self) -> LineOn {
        let line = self.scrolled.line(Rows::ZERO, self.term.cursor().0);
        self.paths.line_on(self.term.screen(), line)
    }
}

/// **The group whose verdict each line's facts came from** (G1): what
/// tells a write over a line another group lit from a group's rewrite of
/// its own. Set where a verdict is filed -- a batch's answer on its lines,
/// a hold released, the shell's own read filed at once -- the source a
/// line had before giving way to the new one; carried with the text
/// through every part's moves, as the lines still to be named are; and
/// dropped above the window, where no write reaches. Spans of one source
/// that touch are one. `Paths`'s facts keep no group, and a fact carried
/// keeps none of where it came from, so the group lives here.
#[derive(Debug, Default)]
struct Lit {
    spans: Vec<(Option<Pid>, LineSpan)>,
}

impl Lit {
    /// **`span` lit by `by`'s verdict**: whatever lit its lines before
    /// gives way there, and keeps the rest of its span.
    fn set(&mut self, by: Option<Pid>, span: LineSpan) {
        let mut i = 0;
        while let Some(&(g, s)) = self.spans.get(i) {
            if s.meet(span).is_none() {
                i += 1;
                continue;
            }
            self.spans.swap_remove(i);
            self.spans.extend(s.outside(span).map(|rest| (g, rest)));
        }
        self.spans.push((by, span));
        self.merge();
    }

    /// Spans of one source that touch, joined.
    fn merge(&mut self) {
        let mut i = 0;
        while i < self.spans.len() {
            let (g, s) = self.spans[i];
            let next = (i + 1..self.spans.len())
                .find(|&j| self.spans[j].0 == g && self.spans[j].1.touches(s));
            match next {
                Some(j) => {
                    let (_, t) = self.spans.swap_remove(j);
                    self.spans[i].1 = s.joined(t);
                }
                None => i += 1,
            }
        }
    }

    /// Every span carried where `carry` says its text went.
    fn carry(&mut self, carry: impl Fn(LineSpan) -> LineSpan) {
        for e in &mut self.spans {
            e.1 = carry(e.1);
        }
        self.merge();
    }

    /// `erased`'s lines out of every span: what lit them was filed over
    /// text that is gone (`Probes::erased`).
    fn cut(&mut self, erased: LineSpan) {
        self.spans = core::mem::take(&mut self.spans)
            .into_iter()
            .flat_map(|(g, s)| s.outside(erased).map(move |p| (g, p)))
            .collect();
    }

    /// **Only what a write can reach**: the lines on `surfaces`, and on
    /// the first of them none above `top`, the window's first line.
    fn keep_from(
        &mut self,
        surfaces: [crate::paths::Surface; 2],
        top: Option<baud_render::AbsLine>,
    ) {
        self.spans.retain_mut(|(_, s)| {
            if !surfaces.contains(&s.surface()) {
                return false;
            }
            match top {
                Some(top) => match s.below_or_at(surfaces[0], top) {
                    Some(kept) => {
                        *s = kept;
                        true
                    }
                    None => false,
                },
                None => true,
            }
        });
    }

    /// **The lines of `lit` that `g`'s own verdict did not light**: each
    /// span less the lines this names `g` for. A line it names no source
    /// for is among them.
    fn not_of(&self, g: Option<Pid>, lit: Vec<LineSpan>) -> Vec<LineSpan> {
        let mut out = lit;
        for &(_, own) in self.spans.iter().filter(|e| e.0 == g) {
            out = out.into_iter().flat_map(|s| s.outside(own)).collect();
        }
        out
    }

    fn clear(&mut self) {
        self.spans.clear();
    }
}

/// **The groups to hold a moved line under** (F3): those of the spans
/// still to be named that covered its source, or, where none is found --
/// `Paths`'s copy and the driver's no longer agreeing -- the lines past
/// the groups kept's, which the next batch's answer files not known.
fn holders(found: Vec<Option<Pid>>) -> Vec<Option<Pid>> {
    match found.is_empty() {
        true => vec![None],
        false => found,
    }
}

/// How a driver starts the worker.
pub type Start = Box<dyn FnMut() -> std::io::Result<Worker>>;

/// **The lines whose writer is still to be named**, kept to send and sent
/// ([`Probes`]'s `kept`), each in the order it came.
#[derive(Default, Debug)]
struct Kept {
    /// Each group seen at a read after the last batch, with the lines it
    /// wrote, and the lines written past [`GROUPS`] of them.
    seen: Vec<(Pid, LineSpan)>,
    unnamed: Option<LineSpan>,
    /// **The lines of each group a batch sent is to name**, by the batch,
    /// the group (none for the lines past the groups kept) and the lines,
    /// carried with the text through every part's moves until the answer
    /// files on them (B10): the worker's copy is the lines as they were
    /// read.
    inflight: Vec<(Id, Option<Pid>, LineSpan)>,
}

impl Kept {
    /// Every span kept, in no order.
    fn spans(&self) -> impl Iterator<Item = LineSpan> + Clone + '_ {
        let seen = self.seen.iter().map(|e| e.1).chain(self.unnamed);
        seen.chain(self.inflight.iter().map(|e| e.2))
    }
}

/// **The driver's side of the worker**: when to ask, the answers merged, the
/// hang watched, a click's second look.
pub struct Probes<T> {
    pace: Pace<T>,
    slot: Slot,
    start: Start,
    /// How many threads were started: one at most, which a test reads.
    started: baud_render::Tally,
    flight: Option<Flight<T>>,
    next_id: Id,
    /// Clicks and screen batches sent, and answers taken: one is out while
    /// they differ. Answers come out of order -- a click is answered before
    /// a screen batch sent ahead of it -- so no id says which is the last.
    sent: Id,
    answered: Id,
    click: Option<Id>,
    /// The group in front when the worker last named it not local.
    elsewhere: Option<Pid>,
    /// The group last seen in front, at a read or a batch, and the group
    /// the program's folder in [`Paths`] was read for: a folder answered
    /// for another group than the one in front is not taken, and one kept
    /// goes when another group comes in front, so a reused pid or a later
    /// program never reads against an earlier one's folder.
    front_group: Option<Pid>,
    dir_key: Option<(Pid, StartTime)>,
    /// A read sampled another group in front than the folder's: the folder
    /// goes at the next turn, which reads the lines again.
    dir_dropped: Owed,
    /// **The lines still to be named, kept to send and sent**: every change
    /// moves its generation, and [`Probes::publish`] hands them on only
    /// when it or `held`'s moved.
    kept: Tracked<Kept>,
    /// **The probe runs** (`Probes::drain_recut`): one a drain, none while
    /// nothing is read.
    probe_runs: baud_render::Tally,
    /// **The cursor's line each batch out was sent with**, by the batch,
    /// carried with its text through a re-cut (`Probes::recut`): the line
    /// its answer's program in front is named from, where it is now, not
    /// on the numbers it was sent on.
    cursors: Vec<(Id, LineOn)>,
    /// **The lines still to be named that a local answer for another
    /// writer was filed over**, by the group still to name them (none for
    /// the lines past the groups kept): a span carried imprecisely -- a
    /// hull, a band -- may cover another writer's text, which has no fact
    /// of its own until named, so those lines read as not known
    /// (`Paths::set_pending`) until the answer naming their own group is
    /// filed on them as well (`Probes::file_answered`).
    held: Tracked<Vec<(Option<Pid>, LineSpan)>>,
    /// The group whose verdict each line's facts came from (G1): a group
    /// still to be named writing over lines another lit holds them.
    lit: Lit,
    /// **The lines reads not kept wrote while paths were off**, carried
    /// with their text (W3, F1): no group was asked for them, so their
    /// facts are still those of the text before, and the turn that finds
    /// paths on again files them not known (`Probes::floor_off`). A link's
    /// read kept meanwhile takes its lines out, its own answer filed there.
    off: Vec<LineSpan>,
    /// The shell's group, as sampled at its own last read: what the lines
    /// its read filed at once were lit by ([`Paths::written_by_shell`]).
    shell_group: Option<Pid>,
    /// What to find a program for, the driver's list from the config, and
    /// whether it is owed an ask.
    programs: Vec<Program>,
    programs_owed: Owed,
    /// What the rows drew before a batch's scan, kept between batches so
    /// the compare that decides its frame allocates nothing once grown.
    before: Before,
    pub counts: Counts,
    /// **The text of every path a batch asked about**, in order, for the
    /// frame rig under the probe only: [`Counts::asked`] says how many, and
    /// a row about one path must see that path among them.
    #[cfg(feature = "effect-probe")]
    pub asked_texts: Vec<Vec<u8>>,
    /// **The worker's verdicts that a writer seen at a read was not
    /// named**, gone before it was named or past the groups a batch keeps,
    /// each filed over its lines (`Probes::file_answered`), for the frame
    /// rig under the probe only: a row whose marks never came says whether
    /// a writer of its lines went unnamed.
    #[cfg(feature = "effect-probe")]
    pub not_named: u64,
    /// **The screen and click answers that named the program in front not
    /// on this machine**, or not known to be (`front_of`: none read is not
    /// known to be local), each leaving the rows from its cursor's line
    /// unread while it stays in front, for the frame rig under the probe
    /// only: a group gone and still in front when a batch goes is named so.
    #[cfg(feature = "effect-probe")]
    pub elsewhere_fronts: u64,
    /// Whether the window's marks, as the reader's scan reads them, are the
    /// core's (`vouch::Latch`): stepped part by part as each drain is
    /// filed.
    latch: crate::vouch::Latch,
    /// **Every `publish` made and the nanoseconds it took**, read by
    /// `measure_publish_over_lines_held_by_a_re_cut`.
    #[cfg(test)]
    publishes: AtomicU64,
    #[cfg(test)]
    publish_ns: AtomicU64,
}

/// **The runs the rows drew before a batch's scan read them again**: the
/// scan rebuilds the lines and the struck runs in place, so telling whether
/// it drew them otherwise needs the runs as they were; a count of runs
/// added or taken cannot, since a batch reads every row and takes and adds
/// every run. Held here and refilled each batch, never reallocated once
/// grown.
#[derive(Default)]
struct Before {
    lines: Vec<baud_render::LinkLine>,
    struck: Vec<baud_render::LinkLine>,
}

impl Before {
    /// The runs `lines` draws now, kept.
    fn keep(&mut self, lines: &Lines) {
        self.lines.clear();
        self.lines.extend_from_slice(lines.lines());
        self.struck.clear();
        self.struck.extend_from_slice(lines.struck());
    }

    /// Whether `lines` draws other runs than those kept.
    fn moved(&self, lines: &Lines) -> bool {
        lines.lines() != &self.lines[..] || lines.struck() != &self.struck[..]
    }
}

impl<T: Moment> Probes<T> {
    pub fn new(start: Start) -> Probes<T> {
        Probes {
            pace: Pace::default(),
            slot: Slot::Unstarted,
            start,
            started: baud_render::Tally::default(),
            flight: None,
            next_id: Id::default(),
            sent: Id::default(),
            answered: Id::default(),
            click: None,
            elsewhere: None,
            front_group: None,
            dir_key: None,
            dir_dropped: Owed::No,
            kept: Tracked::new(Kept::default()),
            probe_runs: baud_render::Tally::default(),
            cursors: Vec::new(),
            held: Tracked::new(Vec::new()),
            lit: Lit::default(),
            off: Vec::new(),
            shell_group: None,
            programs: Vec::new(),
            programs_owed: Owed::No,
            before: Before::default(),
            counts: Counts::default(),
            #[cfg(feature = "effect-probe")]
            asked_texts: Vec::new(),
            #[cfg(feature = "effect-probe")]
            not_named: 0,
            #[cfg(feature = "effect-probe")]
            elsewhere_fronts: 0,
            latch: crate::vouch::Latch::Synced,
            #[cfg(test)]
            publishes: AtomicU64::new(0),
            #[cfg(test)]
            publish_ns: AtomicU64::new(0),
        }
    }

    /// **What to find a program for**, from the config: the kinds of link
    /// `admit` opens on macOS while `missing = "strike"`, and the `[link.*]`
    /// rules' `command[0]`s. A list that changed is asked at the next turn.
    pub fn set_programs(&mut self, programs: Vec<Program>) {
        if self.programs != programs {
            self.programs = programs;
            self.owe_programs();
        }
    }

    /// **Ask about the programs again at the next turn**: the window came to
    /// the front, a job ended, the config was read again, or the pointer came
    /// over a link struck for one.
    pub fn owe_programs(&mut self) {
        if !self.programs.is_empty() {
            self.programs_owed = Owed::Yes;
        }
    }

    /// The real worker: `std::fs` and the system's mount table.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn system(wake: impl Fn() + Send + Clone + 'static) -> Probes<T> {
        Probes::new(Box::new(move || {
            Worker::start(Box::new(System), Box::new(wake.clone()))
        }))
    }

    /// Threads started so far.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn started(&self) -> baud_render::Tally {
        self.started
    }

    /// The probe runs so far: one a drain.
    #[cfg(test)]
    pub fn probe_runs(&self) -> baud_render::Tally {
        self.probe_runs
    }

    /// **A drain of reads was taken**, the cursor on `began` before the first
    /// and on `ended` after the last, as macOS feeds a drain: each part is
    /// filed over the lines the core's record of it names on each surface
    /// ([`part_spans`]) -- what it wrote off its forward path and where it
    /// began and ended there -- each leaving the line it ended on out where
    /// its record vouches that line holds nothing it wrote
    /// ([`crate::vouch::Below`]), and every read under the most
    /// cautious verdict of the drain's ([`crate::ran::Ran::or`]), so no
    /// read's verdict lowers rows another read in it wrote: a local
    /// command's read lifts nothing a read naming nothing wrote, and the
    /// shell's own read drops no sample or watch's verdict of a command's.
    /// Each read's groups are kept as [`Probes::read_taken`] keeps them. A
    /// read that left one surface for the other files the lines it wrote
    /// on each, a remote one left on the alternate screen as it went
    /// included. `lines` places the core's line numbers on the app's
    /// (`keep::Counts`); a part the core says it renumbered, or recorded
    /// nothing of, files nothing. Before any of it, lines renumbered in a
    /// way no count says are left not known -- each part's renumbering up
    /// to the window's last the numbers reached by its end
    /// (`Paths::part_renumbered`), a take-back past the count's first line
    /// up to the batch's (`Paths::taken_back_past_start`) -- and the text
    /// each part moved is carried to the lines it went to
    /// (`Paths::carried`), part by part.
    ///
    /// **A drain a read was cut in at the shell's `D` is three spans**
    /// (`cut.rs`): the parts before the cut, over their own lines; the
    /// parts from the cut on over the cut's line alone, under the most
    /// cautious verdict of their own; and every other line those parts
    /// named, which the shell's bytes moved over, under every part's
    /// verdict as it would have been uncut -- a remote program can leave
    /// text below its cursor that no span filed, and the shell's line feeds
    /// pass over it without writing it. So a prompt of two lines keeps its
    /// second line dark, failing closed. A cut whose line `term` cannot
    /// number -- the surface switched, the grid resized, lines taken back
    /// since, or the bytes after it ending above it -- is no cut: the drain
    /// is filed as it would have been without it.
    ///
    /// **A clear of the alternate screen goes first**: where a part's record
    /// says the core cleared it, its facts go (`Paths::alternate_cleared`),
    /// and only the lines the parts from the last clear on name on it are
    /// filed -- the text the parts before it wrote there is gone.
    ///
    /// **`now` is the turn's clock, the drain's own**: a part in which the
    /// window's shell ended its command, trusted and leaving nothing open
    /// (`vouch::Ended`), makes the next batch due at once, from `now`
    /// (`Pace::ended`); a part that opens a command, or is not trusted,
    /// ends that (`Pace::lapse`). It decides when a batch goes, and files
    /// no line.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn drain_taken(
        &mut self,
        paths: &mut Paths,
        term: &Term<'_>,
        on: (LineOn, LineOn),
        lines: CoreLines,
        reads: impl Iterator<Item = (Writers, Linked)>,
        now: T,
    ) {
        self.drain_recut(
            paths,
            term,
            on,
            lines,
            reads,
            &crate::paths::Recuts::NONE,
            now,
        );
    }

    /// **[`Probes::drain_taken`], each page-width request's re-cut among
    /// the parts followed** (`recuts`, the call's `reads::Pace::recuts`):
    /// the parts up to the resize -- a part of its own after the bytes that
    /// asked for it (`reads::paced`), which wrote none -- are filed first,
    /// on the numbers before it, the bytes that asked included; the re-cut
    /// then carries every line's facts and every span kept of the lines to
    /// where its text went ([`Probes::recut`]); then the parts after it are
    /// filed on the numbers it made. A resize whose re-cut was not told, or
    /// is not held in `recuts`, or a batch whose count was clamped, or one
    /// filed with `[link] paths` not on (`Probes::resized`), renumbers what
    /// it reached as a re-cut no map follows does
    /// (`Paths::part_renumbered`).
    #[allow(clippy::too_many_arguments)]
    pub fn drain_recut(
        &mut self,
        paths: &mut Paths,
        term: &Term<'_>,
        (began, ended): (LineOn, LineOn),
        lines: CoreLines,
        reads: impl Iterator<Item = (Writers, Linked)>,
        recuts: &crate::paths::Recuts,
        now: T,
    ) {
        // Every change to the lines still to be named was handed on.
        #[cfg(test)]
        assert!(self.published(paths), "pending not published");
        self.probe_runs.bump();
        self.floor_off(paths);
        // Each part's marks against the core's, and the window's latch
        // stepped over them in order: a part whose marks are not trusted
        // is filed as the shell's marks had said nothing of it.
        let stepped: Vec<(Writers, Linked)> = reads
            .map(|(w, linked)| {
                let w = self.latched(w, now);
                let untouched = untouched_start(paths, &lines.part(w.top), &w);
                (Writers { untouched, ..w }, linked)
            })
            .collect();
        // The window taken back past the session's first line (N2, P1):
        // every line up to the window's last the batch's numbers reached.
        let tops = |run: &[(Writers, Linked)]| -> Vec<baud_domain::LineIdx> {
            run.iter().filter_map(|(w, _)| w.top).collect()
        };
        if lines.numbers() == crate::paths::Numbers::Clamped {
            if let (Some(last), _) = lines.lasts(tops(&stepped).into_iter()) {
                paths.taken_back_past_start(last);
            }
        }
        // Each part's re-cut, where every line of it was told on a count
        // that followed the core's top: a part that sealed resizes takes
        // the next sealed ones (`Recuts::sealed`), whether it follows them
        // or not, so each later part takes its own.
        let mut sealed = recuts.sealed();
        let follows: Vec<_> = stepped
            .iter()
            .map(|(w, _)| {
                let at = w.recut?;
                let told: Vec<_> = sealed.next()?.collect();
                let resized =
                    w.record.and_then(|r| r.renumbered()) == Some(baud_core::Renumbered::Resize);
                let all = told
                    .iter()
                    .all(|t| t.following == baud_core::Following::Followed);
                let on = paths.marks() == Marks::On;
                (resized && all && on && lines.numbers() == crate::paths::Numbers::Followed)
                    .then_some((at, told))
            })
            .collect();
        let mut from = 0;
        for (k, follow) in follows.iter().enumerate() {
            let Some((at, told)) = follow else {
                continue;
            };
            let (before, run) = stepped[..k + 1].split_at(from);
            self.drain_segment(
                paths,
                term,
                ((began, ended), lines),
                (before, run, &follows[from..k + 1]),
                RunEnds::AtRecut,
            );
            from = k + 1;
            let w = stepped[k].0;
            let placed = lines.part(w.top);
            let reached = tops(&stepped[..k + 1]).into_iter().chain([at.pre()]);
            let (Some(last), _) = lines.lasts(reached) else {
                continue;
            };
            for t in told {
                self.recut(paths, t.map, &placed, last);
            }
        }
        let (before, run) = stepped.split_at(from);
        self.drain_segment(
            paths,
            term,
            ((began, ended), lines),
            (before, run, &follows[from..]),
            RunEnds::AtBatch,
        );
        // What lit each line, kept to the lines a write can reach once this
        // drain's parts have read it (G1): a line one wrote over and then
        // scrolled into history is held first.
        let top = stepped
            .iter()
            .filter_map(|(w, _)| w.top)
            .next_back()
            .and_then(|top| lines.part(Some(top)).line(baud_core::Screen::Normal, top));
        let surfaces =
            [baud_core::Screen::Normal, baud_core::Screen::Alternate].map(|sc| paths.surface(sc));
        self.lit.keep_from(surfaces, top);
    }

    /// **A run of a drain's parts, filed** ([`Probes::drain_recut`]):
    /// `run`, the parts `before` it in the drain ahead of it, and `follows`
    /// each one of the run's re-cut where it is followed -- a part between
    /// two followed re-cuts is filed on the numbers the first made. The cut
    /// is taken only in the run the batch ends with, the one the cursor
    /// ended in; one before a re-cut is no cut, as one whose grid resized
    /// since is (`cut::Spot::line`).
    #[allow(clippy::type_complexity)]
    fn drain_segment(
        &mut self,
        paths: &mut Paths,
        term: &Term<'_>,
        ((began, ended), lines): ((LineOn, LineOn), CoreLines),
        (before, run, follows): (
            &[(Writers, Linked)],
            &[(Writers, Linked)],
            &[Option<(crate::paths::RecutAt, Vec<crate::paths::Told<'_>>)>],
        ),
        ends: RunEnds,
    ) {
        let reads = run.iter().copied();
        let clears = reads
            .clone()
            .filter(|(w, _)| {
                w.record
                    .is_some_and(|r| r.cleared() == baud_core::Cleared::Alternate)
            })
            .count();
        for _ in 0..clears {
            paths.alternate_cleared();
        }
        // Each part the core says renumbered the lines in a way no count
        // says (N2, P1) -- a page-width request's re-cut no map follows, or
        // a core that recorded nothing -- up to the window's last the
        // numbers reached by the end of that part, not the lines the parts
        // after it wrote.
        for (k, (w, _)) in reads.clone().enumerate() {
            let Some(why) = w.record.and_then(|r| r.renumbered()) else {
                continue;
            };
            if follows[k].is_some() {
                continue;
            }
            let reached = before.iter().chain(&run[..k + 1]);
            if let (Some(primary), alternate) = lines.lasts(reached.filter_map(|(w, _)| w.top)) {
                paths.part_renumbered(why, (primary, alternate));
            }
        }
        // The parts before the last clear name lines of a text it took.
        let last_clear = reads
            .clone()
            .enumerate()
            .filter(|(_, (w, _))| {
                w.record
                    .is_some_and(|r| r.cleared() == baud_core::Cleared::Alternate)
            })
            .map(|(k, _)| k)
            .next_back();
        // A cut whose parts a re-cut came among -- one before this run, or
        // a renumbering in it from the cut on -- is no cut: the line it was
        // taken on was renumbered since, and a grid restored to its size
        // (132 columns, then 80) would pass `Spot::line`'s checks.
        let after = |w: &Writers| matches!(w.cut, crate::cut::Cut::After(_));
        let whole = !before.iter().any(|(w, _)| after(w));
        let cut = reads
            .clone()
            .enumerate()
            .filter(|_| ends == RunEnds::AtBatch && whole)
            .find_map(|(k, (w, _))| match w.cut {
                crate::cut::Cut::After(m) => Some((k, m)),
                crate::cut::Cut::None | crate::cut::Cut::Plan(_) => None,
            })
            .filter(|&(k, _)| {
                run[k..]
                    .iter()
                    .all(|(w, _)| w.record.and_then(|r| r.renumbered()).is_none())
            });
        let one_surface =
            began.surface == ended.surface && paths.surface(term.screen()) == ended.surface;
        let line = cut.filter(|_| one_surface).and_then(|(_, m)| {
            m.spot
                .line(term, ended.line)
                .filter(|l| began.line <= *l && *l <= ended.line)
        });
        // Each part after the cut as it would have been filed without it.
        let uncut = reads.clone().map(|(w, linked)| match w.cut {
            crate::cut::Cut::After(m) => (
                Writers {
                    marked: m.uncut.marked,
                    before: m.uncut.before,
                    watched: m.uncut.watched,
                    ..w
                },
                linked,
            ),
            crate::cut::Cut::None | crate::cut::Cut::Plan(_) => (w, linked),
        });
        // What each part keeps of what its record names: the parts before
        // the last clear name lines of a text it took.
        let keep = |k| match last_clear {
            Some(c) if k < c => Keeps::Primary,
            Some(c) if k == c => Keeps::ClearedHere,
            _ => Keeps::Both,
        };
        // The text each part moved, carried to where it went, part by part
        // from the facts as the parts before left them (`Paths::carried`).
        for (k, (w, _)) in reads.clone().enumerate() {
            let Some(rec) = w.record.filter(|r| r.renumbered().is_none()) else {
                continue;
            };
            let screens = match keep(k) {
                Keeps::Primary => &[baud_core::Screen::Normal][..],
                Keeps::Both | Keeps::ClearedHere => {
                    &[baud_core::Screen::Normal, baud_core::Screen::Alternate][..]
                }
            };
            let at = lines.part(w.top);
            let moved = paths.carried(&rec, &at, screens);
            // Each line moved text a span still to be named covered may
            // have gone to is held under that span's group: its text has
            // no fact until named, and what was filed over the line was
            // another text's. Found on the spans as they were, held after
            // they are carried, where the line already is.
            let mut held = Vec::new();
            for (line, by) in moved {
                let found: Vec<_> = self.groups_of(by).collect();
                debug_assert!(!found.is_empty(), "a covering span with no group");
                held.extend(holders(found).into_iter().map(|g| (g, line)));
            }
            self.carry_pending(paths, &rec, &at, screens);
            for h in held {
                hold(&mut self.held, h);
            }
            // What it erased whole, forgotten after its moves and before
            // any of the drain is filed.
            self.erased(paths, &rec, &at, screens);
            self.publish(paths);
        }
        let kept: Vec<_> = reads
            .clone()
            .enumerate()
            .map(|(k, (w, linked))| (keep(k), w, linked))
            .collect();
        // Each part of a run with the lines its record names.
        let spanned = |paths: &Paths, run: &[(Keeps, Writers, Linked)]| {
            let mut out = Vec::new();
            for &(keeps, w, linked) in run {
                let at = lines.part(w.top);
                for span in part_spans(paths, &at, &w, keeps) {
                    out.push((w, linked, span));
                }
            }
            out
        };
        match (cut, line) {
            (Some((k, _)), Some(line)) => {
                let before = spanned(paths, &kept[..k]);
                self.drain_parts(paths, &before);
                let at_cut = LineSpan::on(ended.surface, line, line);
                let after: Vec<_> = reads
                    .clone()
                    .skip(k)
                    .map(|(w, linked)| (w, linked, at_cut))
                    .collect();
                self.drain_parts(paths, &after);
                // Every other line the parts from the cut on named, under
                // every part's verdict uncut.
                let named = spanned(paths, &kept[k..]);
                let rest = named
                    .iter()
                    .map(|&(_, _, s)| s)
                    .reduce(LineSpan::joined)
                    .into_iter()
                    .flat_map(|s| s.without(line));
                for span in rest {
                    let all: Vec<_> = uncut.clone().map(|(w, linked)| (w, linked, span)).collect();
                    self.drain_parts(paths, &all);
                }
            }
            _ => {
                let uncut: Vec<_> = uncut
                    .enumerate()
                    .map(|(k, (w, linked))| (keep(k), w, linked))
                    .collect();
                let all = spanned(paths, &uncut);
                self.drain_parts(paths, &all);
            }
        }
    }

    /// **A part's writers, the window's latch stepped over it**
    /// (`vouch::Latch`): as they are where its marks are trusted; else as
    /// they would have been with the read uncut, at least a command naming
    /// nothing's verdict, its folder report refused and its marks unmatched,
    /// so no line of it is vouched clean.
    fn latched(&mut self, w: Writers, now: T) -> Writers {
        use crate::vouch::Vouch;
        let (latch, trusted, ended) = match w.record {
            Some(rec) => self.latch.step(&rec, &w.marks),
            None => (
                crate::vouch::Latch::Drifted,
                Vouch::No(crate::vouch::Why::Unrecorded),
                None,
            ),
        };
        self.latch = latch;
        // The shell ended its command here, with nothing open after: the
        // next batch is due at once. Else a command opened, or the marks
        // are not trusted, and what was granted ends. A command whose `C`
        // and `D` share the part ended in it: `Resync` already refuses a
        // `C` after the `D`.
        let opens = w.marks.marks().any(|m| m.letter().raw_byte() == b'C');
        match (ended, trusted) {
            (Some(proof), _) => self.pace.ended(proof, now),
            (None, Vouch::No(_)) => self.pace.lapse(),
            (None, Vouch::Yes(_)) if opens => self.pace.lapse(),
            (None, Vouch::Yes(_)) => {}
        }
        let Vouch::No(why) = trusted else {
            return Writers {
                matched: Some(trusted),
                ..w
            };
        };
        let w = match w.cut {
            crate::cut::Cut::After(m) => Writers {
                marked: m.uncut.marked,
                before: m.uncut.before,
                watched: m.uncut.watched,
                cut: crate::cut::Cut::None,
                ..w
            },
            crate::cut::Cut::None | crate::cut::Cut::Plan(_) => w,
        };
        Writers {
            marked: w.marked.or(crate::ran::Ran::Command),
            folder: match w.folder {
                crate::ran::Folder::Reported => crate::ran::Folder::FromRemote,
                f => f,
            },
            matched: Some(Vouch::No(why)),
            ..w
        }
    }

    /// **Every line whose writer is still to be named**: kept to send, and
    /// sent and not yet answered.
    fn pending(&self) -> impl Iterator<Item = LineSpan> + Clone + '_ {
        let held = self.held.get().iter().map(|e| e.1);
        self.kept.get().spans().chain(held)
    }

    /// Where the lines still to be named and the holds stand: the pair
    /// [`Probes::publish`] hands on with them.
    fn gens(&self) -> (PendingGen, PendingGen) {
        (self.kept.generation(), self.held.generation())
    }

    /// **The lines still to be named, and those held, handed to `paths`**
    /// where they are now (`Paths::set_pending`). After everything that
    /// changes them -- a read kept, a part's moves, a batch answered, the
    /// same-group shortcut, a close. Nothing is handed on, and no span
    /// compared, while neither has moved from where `paths` last took them.
    fn publish(&self, paths: &mut Paths) {
        #[cfg(test)]
        let began = std::time::Instant::now();
        if paths.pending_at() != Some(self.gens()) {
            let held = self.held.get().iter().map(|e| e.1);
            paths.set_pending(self.gens(), self.pending(), held);
        }
        #[cfg(test)]
        {
            let ns = u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX);
            self.publishes.fetch_add(1, Ordering::Relaxed);
            self.publish_ns.fetch_add(ns, Ordering::Relaxed);
        }
    }

    /// Whether every change to the lines still to be named was handed on:
    /// `paths` took them where they stand, or took none and none is
    /// pending.
    #[cfg(test)]
    fn published(&self, paths: &Paths) -> bool {
        match paths.pending_at() {
            Some(at) => at == self.gens(),
            None => self.pending().next().is_none(),
        }
    }

    /// **The group of each line still to be named that is `span`**, as
    /// handed to `Paths` (`Probes::publish`): none for the lines past the
    /// groups kept.
    fn groups_of(&self, span: LineSpan) -> impl Iterator<Item = Option<Pid>> + '_ {
        let kept = self.kept.get();
        let seen = kept.seen.iter().map(|e| (Some(e.0), e.1));
        seen.chain(kept.unnamed.map(|l| (None, l)))
            .chain(kept.inflight.iter().map(|e| (e.1, e.2)))
            .chain(self.held.get().iter().copied())
            .filter(move |e| e.1 == span)
            .map(|e| e.0)
    }

    /// `span` written by a read not kept with paths off, joined to what
    /// overlaps it on its surface.
    fn off_wrote(&mut self, span: LineSpan) {
        let mut span = span;
        self.off.retain(|&o| match o.meet(span) {
            Some(_) => {
                span = o.joined(span);
                false
            }
            None => true,
        });
        self.off.push(span);
    }

    /// `span` written by a link's read kept with paths off: its own answer
    /// is filed there, so no floor is.
    fn off_kept(&mut self, span: LineSpan) {
        let off = core::mem::take(&mut self.off);
        self.off = off.into_iter().flat_map(|o| o.outside(span)).collect();
    }

    /// **Paths on again: what reads not kept wrote while they were off,
    /// filed not known** (`Probes::off`); none kept once the worker is
    /// closed, which lights nothing again.
    fn floor_off(&mut self, paths: &mut Paths) {
        match paths.marks() {
            Marks::On => {
                for span in self.off.drain(..) {
                    paths.written_unnamed(span);
                }
            }
            Marks::Closed => self.off.clear(),
            Marks::Off => {}
        }
    }

    /// **Holds no group still to be named will release, filed `f` and
    /// dropped**: a hold waits for the answer naming its group, and with
    /// none of that group's lines left to name -- a close, the same-group
    /// shortcut, a batch whose answer did not name it -- none will come.
    fn drop_orphan_holds(&mut self, paths: &mut Paths, f: crate::paths::Seen) {
        let kept = self.kept.get();
        let named = |h: Option<Pid>| match h {
            Some(g) => kept.seen.iter().any(|e| e.0 == g),
            None => kept.unnamed.is_some(),
        } || kept.inflight.iter().any(|e| e.1 == h);
        if self.held.get().iter().all(|&(h, _)| named(h)) {
            return;
        }
        self.held.edit().retain(|&(h, span)| {
            let waits = named(h);
            if !waits {
                paths.written_under(f, span);
            }
            waits
        });
    }

    /// **Every line whose writer is still to be named, carried through a
    /// part's moves** (`Paths::carried_span`, B10).
    fn carry_pending(
        &mut self,
        paths: &mut Paths,
        rec: &baud_core::ReadRecord,
        lines: &crate::paths::PartLines,
        screens: &[baud_core::Screen],
    ) {
        self.carry_spans(|span| paths.carried_span(rec, lines, screens, span));
        self.publish(paths);
    }

    /// **What a part's record says it erased whole, forgotten**
    /// (`Paths::forget_erased`) on each of `screens`, and cut out of every
    /// span this keeps of the lines ([`Probes::cut`]): once a part, in the
    /// parts' order, after the part's moves are carried and before any of
    /// the drain is filed -- so text moved onto a line and then erased goes
    /// with the line, and a read that erases and then writes ends the
    /// earlier facts before it files its own. Each part's own record: one
    /// for the whole drain would say only what its last part erased. A
    /// record that held more than it has room for forgets nothing, and a
    /// run the part's lines cannot place keeps its facts, each counted.
    /// The only caller of `Paths::forget_erased`.
    fn erased(
        &mut self,
        paths: &mut Paths,
        rec: &baud_core::ReadRecord,
        lines: &crate::paths::PartLines,
        screens: &[baud_core::Screen],
    ) {
        use crate::paths::{Erased, ErasedLines};
        for &screen in screens {
            match Erased::of(rec, lines, paths, screen) {
                ErasedLines::Overflowed => {
                    self.counts.erase_overflowed += 1;
                    return;
                }
                ErasedLines::Placed(erased, unplaced) => {
                    self.counts.erase_unplaced += unplaced;
                    for e in erased {
                        paths.forget_erased(e);
                        self.cut(e.span());
                    }
                }
            }
        }
    }

    /// **`erased`'s lines out of every span this keeps of the lines**:
    /// those still to be named, sent, held, written with paths off, and
    /// lit -- the text they were kept for is gone, and an answer about it
    /// is filed over none of them. A span cut in two keeps its batch and
    /// group in both; the lines past the groups kept, one span, keep the
    /// hull of what is left of them, failing closed.
    fn cut(&mut self, erased: LineSpan) {
        // A span `erased` does not meet is left whole by `outside`, so only
        // one it meets moves a generation.
        let meets = |s: LineSpan| s.meet(erased).is_some();
        if self.kept.get().spans().any(meets) {
            let kept = self.kept.edit();
            kept.seen = core::mem::take(&mut kept.seen)
                .into_iter()
                .flat_map(|(g, s)| s.outside(erased).map(move |p| (g, p)))
                .collect();
            kept.unnamed = kept
                .unnamed
                .and_then(|u| u.outside(erased).reduce(LineSpan::joined));
            kept.inflight = core::mem::take(&mut kept.inflight)
                .into_iter()
                .flat_map(|(i, g, s)| s.outside(erased).map(move |p| (i, g, p)))
                .collect();
        }
        if self.held.get().iter().any(|e| meets(e.1)) {
            let held = self.held.edit();
            *held = core::mem::take(held)
                .into_iter()
                .flat_map(|(g, s)| s.outside(erased).map(move |p| (g, p)))
                .collect();
        }
        self.off = core::mem::take(&mut self.off)
            .into_iter()
            .flat_map(|o| o.outside(erased))
            .collect();
        self.lit.cut(erased);
    }

    /// Every span this keeps of the lines, carried where `carry` says their
    /// text went: those still to be named, held, written with paths off,
    /// and lit.
    fn carry_spans(&mut self, carry: impl Fn(LineSpan) -> LineSpan) {
        // A carry is a function of the span alone, so a generation moves
        // only where some span is carried elsewhere.
        let moves = |s: LineSpan| carry(s) != s;
        if self.kept.get().spans().any(moves) {
            let kept = self.kept.edit();
            for e in &mut kept.seen {
                e.1 = carry(e.1);
            }
            kept.unnamed = kept.unnamed.map(&carry);
            for e in &mut kept.inflight {
                e.2 = carry(e.2);
            }
        }
        if self.held.get().iter().any(|e| moves(e.1)) {
            for e in self.held.edit() {
                e.1 = carry(e.1);
            }
        }
        for o in &mut self.off {
            *o = carry(*o);
        }
        self.lit.carry(carry);
    }

    /// **A resize the driver just made, filed at once** (`rec`, the core's
    /// record of it, taken once for this and the counts, which
    /// `keep::kept_across_resize` has moved; `told`, what the resize said
    /// of its re-cut, `paths::Recuts::resize`), before any hover, click or
    /// frame reads a line. Where the record says it renumbered the
    /// primary's lines and every one it re-cut was told, on a count that
    /// followed the core's top, each line's facts and spans go where its
    /// text went ([`Probes::recut`]); else every line up to the window's
    /// last before the resize (`was_last`) or after it is not known
    /// (`Paths::resized`): a re-cut nothing told, a count clamped at the
    /// session's first line, a core that recorded nothing -- or with
    /// `[link] paths` not on: reads not kept then file nothing, so a line
    /// nothing was filed over is not a line nothing wrote, and its text
    /// carries no fact it should. A change of height alone re-cuts nothing
    /// and keeps them.
    pub fn resized(
        &mut self,
        paths: &mut Paths,
        (rec, told): (&baud_core::ReadRecord, Option<crate::paths::Told<'_>>),
        term: &Term<'_>,
        was_last: baud_render::AbsLine,
        counts: &crate::keep::Counts,
    ) {
        let lines = counts.core_lines(term);
        let (primary, alternate) = (counts.primary(), counts.alternate());
        let last = was_last.max(crate::paths::last_line(term, primary));
        match (rec.renumbered(), told, lines.numbers(), paths.marks()) {
            (None, _, _, _) => {}
            (
                Some(baud_core::Renumbered::Resize),
                Some(crate::paths::Told {
                    map,
                    following: baud_core::Following::Followed,
                }),
                crate::paths::Numbers::Followed,
                Marks::On,
            ) => {
                let at = lines.part(Some(term.history_mark().line()));
                self.recut(paths, map, &at, last);
            }
            (Some(_), _, _, _) => paths.resized(rec, term, was_last, primary, alternate),
        }
    }

    /// **A re-cut followed, on the lines and on every span kept of them**
    /// (`Paths::reflowed`, `Paths::recut_span`): each new line whose text a
    /// span still to be named covered is held under that span's group,
    /// found on the spans as they were and held once they are carried, as
    /// for a part's moves -- so a line holding two writers' text lights
    /// only once both are named.
    ///
    /// This hold is the second guard on the rule, behind `carry_spans`: every
    /// case built is held by the carried spans as well, so no test goes red
    /// without it. It stays as the backstop for a span kind still to be named
    /// that is added without a carry of its own.
    fn recut(
        &mut self,
        paths: &mut Paths,
        map: &[baud_core::LineRecut],
        at: &crate::paths::PartLines,
        last: baud_render::AbsLine,
    ) {
        let moved = paths.reflowed(map, at, last);
        let mut held = Vec::new();
        for (line, by) in moved {
            let found: Vec<_> = self.groups_of(by).collect();
            held.extend(holders(found).into_iter().map(|g| (g, line)));
        }
        self.carry_spans(|span| paths.recut_span(span));
        // Each batch out's cursor line, where its text is now, as the
        // program in front's is (`Paths::recut_line`).
        for e in &mut self.cursors {
            e.1 = paths.recut_line(e.1);
        }
        for h in held {
            hold(&mut self.held, h);
        }
        self.publish(paths);
    }

    /// **Each verdict of a batch's answer filed on its group's lines as
    /// they are now** (B10): on the lines kept for it and for every batch it
    /// replaced, carried with the text since they were read, and any of
    /// those lines its answer did not name left not known. A group the
    /// answer names twice -- its lines kept in two spans, another's between
    /// -- is filed at the first, which takes every copy of its lines. A
    /// click's answer files nothing: the lines it was asked with are the
    /// ones still kept to send, which their own batch names where they are
    /// then, or which a batch sent while the click was out, or the
    /// same-group shortcut, has taken -- never the lines as asked, whose
    /// text may have moved. Nor do a batch to name a group, which is
    /// answered by no answer, or one for programs, which names no group.
    ///
    /// **A local verdict holds the lines of another writer's still to be
    /// named that it was filed over** (`Probes::held`): a span carried
    /// imprecisely may cover that writer's text, which has no fact of its
    /// own yet, and the local fact alone would light it. Each hold is
    /// released by the verdict naming its own group, filed on it too, and
    /// one whose group is no longer to be named is left not known.
    fn file_answered(
        &mut self,
        paths: &mut Paths,
        (id, kind): (Id, Kind),
        seen: &[(Option<Pid>, crate::paths::Seen)],
        sessions: &[(Pid, Arc<crate::ssh::Session>)],
    ) {
        use crate::paths::Seen;
        if kind != Kind::Screen {
            return;
        }
        for &(g, f) in seen {
            // The ssh session the worker read for this group, filed with its
            // verdict as what its rows were printed under.
            let by = g
                .and_then(|g| sessions.iter().find(|e| e.0 == g))
                .map(|e| &e.1);
            #[cfg(feature = "effect-probe")]
            {
                self.not_named += u64::from(f == Seen::NotNamed);
            }
            // The holds this group's verdict releases, lit by it now.
            let lit = &mut self.lit;
            self.held.edit().retain(|&(h, span)| {
                if h == g {
                    paths.written_by(f, by, span);
                    lit.set(g, span);
                }
                h != g
            });
            let mine = |&(i, h, _): &(Id, Option<Pid>, LineSpan)| i <= id && h == g;
            // A verdict that lights holds every other writer's lines it
            // lands on.
            if matches!(f, Seen::Named(Front::Local | Front::Multiplexer)) {
                let kept = self.kept.get();
                let others = kept.seen.iter().map(|e| (Some(e.0), e.1));
                let others = others
                    .chain(kept.unnamed.map(|l| (None, l)))
                    .chain(kept.inflight.iter().map(|e| (e.1, e.2)))
                    .filter(|e| e.0 != g);
                for &(_, _, span) in kept.inflight.iter().filter(|e| mine(e)) {
                    for (h, other) in others.clone() {
                        if let Some(both) = span.meet(other) {
                            self.held.edit().push((h, both));
                        }
                    }
                }
            }
            let lit = &mut self.lit;
            self.kept.edit().inflight.retain(|e| {
                if mine(e) {
                    paths.written_by(f, by, e.2);
                    lit.set(g, e.2);
                }
                !mine(e)
            });
        }
        self.kept.edit().inflight.retain(|&(i, _, span)| {
            let answered = i <= id;
            if answered {
                paths.written_under(Seen::NotNamed, span);
            }
            !answered
        });
        self.drop_orphan_holds(paths, Seen::NotNamed);
        self.publish(paths);
    }

    /// Parts of a drain, each filed over its own span, under the most
    /// cautious verdict of them all ([`Probes::drain_taken`]).
    fn drain_parts(&mut self, paths: &mut Paths, parts: &[(Writers, Linked, LineSpan)]) {
        let Some(ran) = parts
            .iter()
            .map(|(w, _, _)| w.marked)
            .reduce(crate::ran::Ran::or)
        else {
            return;
        };
        for &(w, linked, span) in parts {
            // Every read that opened a link or wrote under it is filed
            // against its slot, whatever its rows are filed as.
            for pen in w.pens.each() {
                paths.slot_wrote(pen, span);
            }
            self.read_taken(paths, Writers { marked: ran, ..w }, span, linked);
        }
    }

    /// **A read was taken with `writers` in front, writing `span`**, from
    /// the line the cursor was on as it began to the line it ended on: each
    /// group sampled for it, before the read and just after it, kept with
    /// its lines, and a new one sent to be named at once, while it may still
    /// run. So a read is local only when both samples are, and a program
    /// that wrote and ended before the sample after its read is named by the
    /// one before it -- unless the shell's marks say the read is the shell's
    /// own ([`crate::ran::Ran::Shell`]), its prompt after a command, which
    /// that program did not write. A read the process watch, or the shell's
    /// marks, say a program not on this machine wrote is remote outright
    /// ([`Paths::written_remote`]) -- the watch's verdict not on the
    /// shell's own -- and one the shell, with its token, says a command
    /// named here wrote lifts a writer gone before it was named
    /// ([`Paths::written_here`]). Each of these touches the read's own rows
    /// only. `writers` is the [`Sampler`]'s: none for a read that did not
    /// ask, or a group the kernel no longer names. With `[link] paths =
    /// "off"` only a read that `linked` an `OSC 8` link is kept, since only
    /// a link's click reads who printed it ([`Paths::origin`]).
    pub fn read_taken(
        &mut self,
        paths: &mut Paths,
        writers: Writers,
        span: LineSpan,
        linked: Linked,
    ) {
        use crate::ran::Ran;
        // A folder report sent while a command named not local ran is that
        // program's, which can name this machine as well as the shell can.
        match writers.folder {
            crate::ran::Folder::Unreported => {}
            crate::ran::Folder::Reported => paths.folder_reported(crate::paths::Report::Taken),
            crate::ran::Folder::FromRemote => paths.folder_reported(crate::paths::Report::Refused),
        }
        if let (Ran::Shell, Some(g)) = (writers.marked, writers.at) {
            self.shell_group = Some(g);
        }
        if let Some(g) = writers.at {
            self.front_group = Some(g);
            // yazi handing straight to hx: the folder read for one is never
            // read against for the other, even before the next batch.
            if self.dir_key.is_some_and(|(d, _)| d != g) {
                self.dir_dropped = Owed::Yes;
            }
        }
        let before = match writers.marked {
            Ran::Shell => None,
            Ran::Unmarked | Ran::Here | Ran::Elsewhere | Ran::Command => writers.before,
        };
        let groups = match (before, writers.at) {
            (Some(b), Some(a)) if b == a => [Some(a), None],
            (b, a) => [b, a],
        };
        // The watch names a program, not its rows: what it saw is filed on
        // the reads of the command it ran in, and not on the shell's own,
        // its prompt after `git push` or `git pull`.
        let watched = writers.watched == Watched::Elsewhere && writers.marked != Ran::Shell;
        let remote = watched || writers.marked == Ran::Elsewhere;
        let here = writers.marked == Ran::Here;
        // The shell's own read with no group to name is filed as the
        // shell's, which is on this machine.
        let shell = writers.marked == Ran::Shell && groups == [None, None];
        let kept = match paths.marks() {
            Marks::On => true,
            Marks::Off => linked == Linked::Yes,
            Marks::Closed => false,
        };
        // A read not kept with paths off wrote its lines all the same:
        // they are floored not known when paths come on (`Probes::off`).
        if !kept || matches!(self.slot, Slot::Closed) {
            if paths.marks() == Marks::Off && !matches!(self.slot, Slot::Closed) {
                self.off_wrote(span);
            }
            return;
        }
        // A link's read kept with paths off is named as with them on.
        if paths.marks() == Marks::Off {
            self.off_kept(span);
        }
        // A read nothing names -- no group sampled, no marks -- wrote its
        // lines all the same: they are not known, and never a line with
        // nothing filed over it, which a move carries as nothing written
        // (W3).
        if groups == [None, None] && !remote && !here && !shell {
            paths.written_unnamed(span);
            return;
        }
        if remote {
            paths.written_remote(span);
        }
        if here {
            paths.written_here(span);
        }
        if shell {
            paths.written_by_shell(span);
            self.lit.set(self.shell_group, span);
        }
        // A group still to be named writing over lines that read lit holds
        // them until its own answer (G1): what lit them was another text's,
        // and its own has no fact yet. Only a line whose lit fact its own
        // verdict filed, as `Lit` names it, keeps lighting -- a group's
        // rewrite of its own -- and a line `Lit` names no group for holds:
        // one the window trimmed and a resize brought back.
        for g in groups.into_iter().flatten() {
            for over in self.lit.not_of(Some(g), paths.filed_lit(span)) {
                // Less the line it began on, where it wrote no cell of it.
                let mut written = [Some(over), None];
                if let Some(start) = writers.untouched {
                    written = [None, None];
                    for (slot, part) in written.iter_mut().zip(over.outside(start)) {
                        *slot = Some(part);
                    }
                }
                for over in written.into_iter().flatten() {
                    hold(&mut self.held, (Some(g), over));
                }
            }
        }
        for g in groups.into_iter().flatten() {
            self.group_wrote(paths, g, span);
        }
        self.publish(paths);
    }

    /// `g` wrote `span`: kept with its lines, and named now if it is new.
    /// Its rows join its last span only where they touch it: a group seen at
    /// two reads with another's rows between keeps two spans, so its verdict
    /// covers its own rows and not the rows between, and it is named once.
    fn group_wrote(&mut self, paths: &mut Paths, g: Pid, span: LineSpan) {
        let kept = self.kept.get();
        let known = kept.seen.iter().any(|e| e.0 == g);
        // The lines kept move only where the join is another span.
        if let Some(k) = kept.seen.iter().rposition(|e| e.0 == g) {
            let was = kept.seen[k].1;
            if was.touches(span) {
                if was.joined(span) != was {
                    self.kept.edit().seen[k].1 = was.joined(span);
                }
                return;
            }
        }
        if kept.seen.len() >= GROUPS {
            let both = joined(kept.unnamed, Some(span));
            if both != kept.unnamed {
                self.kept.edit().unnamed = both;
            }
            return;
        }
        self.kept.edit().seen.push((g, span));
        if known {
            return;
        }
        if matches!(self.slot, Slot::Unstarted) {
            self.started.bump();
            match (self.start)() {
                Ok(w) => self.slot = Slot::Running(w),
                Err(e) => {
                    // No mark has been drawn yet: none was ever answered.
                    crate::warn!("paths: no worker thread ({e}); printed paths get no line");
                    self.slot = Slot::Closed;
                    paths.close();
                    return;
                }
            }
        }
        if let Slot::Running(w) = &self.slot {
            let _ = w.post(Ask {
                id: self.next_id,
                kind: Kind::Name,
                group: Some(g),
                cursor: LineOn {
                    surface: span.surface(),
                    line: span.last(),
                },
                paths: Vec::new(),
                seen: Vec::new(),
                unnamed: None,
                programs: Vec::new(),
            });
        }
    }

    /// **The next instant a turn is owed by the clock**: the batch's
    /// deadline while one is owed, and the hang's while one is out. The
    /// Linux loop's wait, and the wake the macOS driver schedules after a
    /// turn's last scan, so no tick polls for it. None once the worker is
    /// closed: its turn would do nothing, and a deadline of `now` that no
    /// turn clears is a wake on every turn.
    pub fn deadline(&self, paths: &Paths, now: T) -> Option<T> {
        if matches!(self.slot, Slot::Closed) {
            return None;
        }
        match paths.marks() {
            Marks::On => {}
            Marks::Closed => return None,
            // A link's read is named at the next turn, now.
            Marks::Off => {
                let kept = self.kept.get();
                let named = !kept.seen.is_empty() || kept.unnamed.is_some();
                return named.then_some(now);
            }
        }
        // A frame's scan owes a batch before any turn has timed it: the next
        // turn, now, times it.
        let due = match paths.owed() {
            Owed::Yes => Some(self.pace.due_at().unwrap_or(now)),
            Owed::No => None,
        };
        let hang = self.flight.map(|f| f.at.after(HANG));
        let programs = (self.programs_owed == Owed::Yes).then_some(now);
        [due, hang, programs].into_iter().flatten().min()
    }

    /// **One turn, at `now`**: answers merged ([`Probes::answers`]), then
    /// the clock's half ([`Probes::tick`]). Linux's loop, whose turn is
    /// every wake, and the tests take both at once.
    #[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
    pub fn turn(&mut self, now: T, s: Seen<'_, '_>, group: impl FnOnce() -> Option<Pid>) -> Turn {
        let mut s = s;
        let mut turn = self.answers(&mut s);
        let clock = self.tick(now, s, group);
        turn.frame = if turn.frame == Owed::Yes {
            Owed::Yes
        } else {
            clock.frame
        };
        turn
    }

    /// **The worker's answers, merged**: what its wake says came. A click's
    /// answer is [`Turn::clicked`]. Nothing here waits or asks, so macOS
    /// takes it on the view's `wake`, which the worker sends after each
    /// answer, and never on the display link's tick.
    pub fn answers(&mut self, s: &mut Seen<'_, '_>) -> Turn {
        // Every change to the lines still to be named was handed on.
        #[cfg(test)]
        assert!(self.published(s.paths), "pending not published");
        self.floor_off(s.paths);
        let mut turn = Turn::default();
        if self.dir_dropped == Owed::Yes {
            self.dir_dropped = Owed::No;
            self.dir_key = None;
            if s.paths.set_program_dir(None) == Moved::Yes {
                s.lines.forget();
                turn.frame = Owed::Yes;
            }
        }
        // With paths off, a link's read was named all the same.
        if s.paths.marks() == Marks::Closed {
            return turn;
        }
        if let Slot::Running(w) = &self.slot {
            let answers: Vec<Answer> = core::iter::from_fn(|| w.rx.try_recv().ok()).collect();
            for a in answers {
                self.counts.calls += a.calls;
                self.answered = self.answered.next();
                // The cursor's line as the batch was sent, where its text
                // is now.
                let cursor = match self.cursors.iter().position(|e| e.0 == a.id) {
                    Some(i) => self.cursors.swap_remove(i).1,
                    None => a.cursor,
                };
                if self.answered == self.sent {
                    self.flight = None;
                }
                if a.kind == Kind::Programs {
                    // A batch left for a newer one answers nothing.
                    if let Some(found) = a.programs {
                        if s.paths.set_programs(found) == Moved::Yes {
                            s.lines.forget();
                            turn.frame = Owed::Yes;
                        }
                    }
                    continue;
                }
                // A batch left for a newer one: its groups went with that.
                let Some(front) = a.front else {
                    continue;
                };
                match front {
                    Front::Elsewhere => self.elsewhere = a.group,
                    Front::Local | Front::Multiplexer => self.elsewhere = None,
                }
                // The session of the ssh in front, as the worker read it:
                // what the rows its group writes while it stays in front
                // are filed as printed by, with no batch to name them.
                s.paths.set_session(a.session);
                #[cfg(feature = "effect-probe")]
                {
                    self.elsewhere_fronts += u64::from(front == Front::Elsewhere);
                }
                let (before, origins) = (s.paths.generation(), s.paths.origins());
                self.file_answered(s.paths, (a.id, a.kind), &a.seen, &a.sessions);
                // A span gone from the surface up is dropped as the next
                // arrives: the primary's history, and on the alternate
                // screen, which keeps none, what scrolled off its top.
                if !a.seen.is_empty() {
                    let screen = s.term.screen();
                    let oldest = match screen {
                        baud_core::Screen::Normal => crate::keep::oldest(s.term, s.scrolled),
                        baud_core::Screen::Alternate => s.scrolled.top(Rows::ZERO),
                    };
                    s.paths.trim_spans(screen, oldest);
                }
                s.paths.set_front(front, cursor);
                // The program's folder, for the group still in front; on
                // the normal screen it is none, which `Paths::observe` keeps.
                // The key it was read under is the group and its leader's
                // start, both in the answer: a folder kept under another
                // start is another process's, and goes.
                if a.group.is_some() && a.group == self.front_group {
                    let key = a.group.zip(a.started);
                    let dir = match s.term.screen() {
                        baud_core::Screen::Alternate => a.folder,
                        baud_core::Screen::Normal => None,
                    };
                    if self.dir_key.is_some() && self.dir_key != key {
                        self.dir_key = None;
                        let _ = s.paths.set_program_dir(None);
                    }
                    self.dir_key = dir.as_ref().and(key);
                    let _ = s.paths.set_program_dir(dir);
                }
                let clicked = (a.kind == Kind::Click && self.click == Some(a.id)).then(|| {
                    self.click = None;
                    match a.probes.first() {
                        Some((p, Probe::Exists)) => Clicked::Exists(p.clone()),
                        Some((_, Probe::Missing)) => Clicked::Missing,
                        Some((_, Probe::Unknown)) | None => Clicked::Unchecked,
                    }
                });
                // What moved before the answers reads every row again; the
                // answers say themselves what they redraw (`merge_into`).
                let moved = s.paths.generation() != before;
                if moved {
                    s.lines.forget();
                }
                if s.paths.merge_into(a.probes, s.lines) == Moved::Yes || moved {
                    turn.frame = Owed::Yes;
                }
                // A program named may join or part two rows: the scan asks
                // again on its own (`Lines::scan`), and the look under a
                // still pointer reads again (`Hover`), on this frame.
                if s.paths.origins() != origins {
                    turn.frame = Owed::Yes;
                }
                if clicked.is_some() {
                    turn.clicked = clicked;
                }
            }
        }
        turn
    }

    /// **The clock's half of a turn, at `now`**: the hang watched, and a
    /// batch sent once it is due. `group` reads the terminal's foreground
    /// group, a `TIOCGPGRP` on the master, only when a batch goes. No
    /// answer is taken here.
    pub fn tick(&mut self, now: T, s: Seen<'_, '_>, group: impl FnOnce() -> Option<Pid>) -> Turn {
        // Every change to the lines still to be named was handed on.
        #[cfg(test)]
        assert!(self.published(s.paths), "pending not published");
        self.floor_off(s.paths);
        let mut turn = Turn::default();
        match s.paths.marks() {
            Marks::On => {}
            Marks::Closed => return turn,
            // Paths off: only the groups a link's read saw, sent to be
            // named, with no screen read and nothing looked up.
            Marks::Off => {
                let kept = self.kept.get();
                if (kept.seen.is_empty() && kept.unnamed.is_none())
                    || matches!(self.slot, Slot::Closed)
                {
                    return turn;
                }
                let (seen, unnamed) = self.take_seen();
                let ask = Ask {
                    id: self.next_id,
                    kind: Kind::Screen,
                    group: group(),
                    cursor: s.cursor_on(),
                    paths: Vec::new(),
                    seen,
                    unnamed,
                    programs: Vec::new(),
                };
                self.counts.batches += 1;
                self.send(ask, now, s.paths, s.lines, &mut turn);
                return turn;
            }
        }
        self.pace.saw(Taken(s.term.amplification().0), now);
        // A hang is a batch out with no beat since.
        if let Slot::Running(w) = &self.slot {
            let beats = w.beats();
            if let Some(f) = &mut self.flight {
                if beats != f.beats {
                    f.beats = beats;
                    f.at = now;
                } else if now >= f.at.after(HANG) {
                    self.close(s.paths, s.lines, &mut turn);
                    return turn;
                }
            }
        }
        if self.programs_owed == Owed::Yes && !matches!(self.slot, Slot::Closed) {
            self.programs_owed = Owed::No;
            let ask = Ask {
                id: self.next_id,
                kind: Kind::Programs,
                group: None,
                cursor: s.cursor_on(),
                paths: Vec::new(),
                seen: Vec::new(),
                unnamed: None,
                programs: self.programs.clone(),
            };
            self.counts.batches += 1;
            if !self.send(ask, now, s.paths, s.lines, &mut turn) {
                return turn;
            }
        }
        if let Some(owes) = s.paths.owes() {
            self.pace.owe(now, owes);
        }
        // The one door to a screen batch: a deadline met, and why it was
        // due, read before the scan clears what is owed.
        let due = match s.paths.owed() {
            Owed::Yes => self.pace.due().filter(|d| now >= d.at),
            Owed::No => None,
        };
        let Some(due) = due else {
            return turn;
        };
        if matches!(self.slot, Slot::Closed) {
            return turn;
        }
        self.pace.scanned(now);
        let cursor = s.cursor_on();
        let g = group();
        if g.is_some() && g == self.elsewhere {
            // The same group, not local: its rows to here stay unmarked, and
            // nothing is asked.
            s.paths.set_front(Front::Elsewhere, cursor);
            s.paths.settled();
            // Every row to the cursor is now this group's: what was seen
            // before it is covered, and was written while it was in front.
            // Its own rows were printed under the session in front.
            let front = s.paths.front_session();
            let (seen, unnamed) = self.take_seen();
            for (h, span) in seen {
                let by = front.as_ref().filter(|_| Some(h) == g);
                s.paths
                    .written_by(crate::paths::Seen::Named(Front::Elsewhere), by, span);
            }
            if let Some(span) = unnamed {
                s.paths
                    .written_under(crate::paths::Seen::Named(Front::Elsewhere), span);
            }
            self.drop_orphan_holds(s.paths, crate::paths::Seen::Named(Front::Elsewhere));
            self.publish(s.paths);
            return turn;
        }
        s.paths.front_moved(cursor);
        s.paths.begin_collect();
        // What the rows drew before the batch's scan: a frame is owed only
        // where the scan drew them otherwise, so a batch over a still screen
        // draws nothing.
        self.before.keep(s.lines);
        s.lines.forget();
        let _ = s.lines.scan(
            s.term,
            s.scrolled,
            &mut Links {
                uris: &mut *s.uris,
                urls: s.urls,
                paths: &mut *s.paths,
            },
        );
        let queue = s.paths.end_collect();
        if self.before.moved(s.lines) {
            turn.frame = Owed::Yes;
        }
        // With no path to ask about, the groups seen still go, to be named
        // and answered with nothing looked up, so the next batch's are its
        // own.
        let kept = self.kept.get();
        if queue.is_empty() && kept.seen.is_empty() && kept.unnamed.is_none() {
            return turn;
        }
        self.counts.asked += u64::try_from(queue.len()).unwrap_or(u64::MAX);
        #[cfg(feature = "effect-probe")]
        self.asked_texts
            .extend(queue.iter().map(|c| c.path.clone()));
        self.counts.batches += 1;
        self.counts.sent_by(due.by);
        if due.by == By::Floor && !queue.is_empty() {
            self.counts.held += 1;
        }
        if due.by == By::Floor && due.granted == Granted::Yes {
            self.counts.held_granted += 1;
        }
        // Only now is it known whether this batch asks about a path, the
        // one kind that holds the next.
        self.pace.sent(now, &queue);
        let (seen, unnamed) = self.take_seen();
        let ask = Ask {
            id: self.next_id,
            kind: Kind::Screen,
            group: g,
            cursor,
            paths: queue,
            seen,
            unnamed,
            programs: Vec::new(),
        };
        self.send(ask, now, s.paths, s.lines, &mut turn);
        turn
    }

    /// **A click on a printed path, asked again off the frame**: its answer
    /// comes in a later turn's [`Turn::clicked`]. Unchecked at once with no
    /// worker to ask.
    pub fn click(
        &mut self,
        now: T,
        s: Seen<'_, '_>,
        group: Option<Pid>,
        path: Candidate,
    ) -> Option<Clicked> {
        if s.paths.marks() != Marks::On || matches!(self.slot, Slot::Closed) {
            return Some(Clicked::Unchecked);
        }
        let cursor = s.cursor_on();
        let id = self.next_id;
        self.counts.asked += 1;
        #[cfg(feature = "effect-probe")]
        self.asked_texts.push(path.path.clone());
        let ask = Ask {
            id,
            kind: Kind::Click,
            group,
            cursor,
            paths: vec![path],
            seen: self.kept.get().seen.clone(),
            unnamed: self.kept.get().unnamed,
            programs: Vec::new(),
        };
        let mut turn = Turn::default();
        if !self.send(ask, now, s.paths, s.lines, &mut turn) {
            return Some(Clicked::Unchecked);
        }
        self.click = Some(id);
        None
    }

    /// The batch to the worker, started if it is not; whether it went.
    fn send(
        &mut self,
        ask: Ask,
        now: T,
        paths: &mut Paths,
        lines: &mut Lines,
        turn: &mut Turn,
    ) -> bool {
        // A batch's lines are kept here too, carried with the text until
        // its answer (B10); a click's are the ones still kept to send.
        self.cursors.push((ask.id, ask.cursor));
        // The lines `take_seen` took out of those kept to send are in
        // `inflight` again here, and handed on as they stand.
        if ask.kind == Kind::Screen {
            let lines = ask.seen.iter().map(|&(g, l)| (Some(g), l));
            let inflight = &mut self.kept.edit().inflight;
            for (g, l) in lines.chain(ask.unnamed.map(|l| (None, l))) {
                inflight.push((ask.id, g, l));
            }
            self.publish(paths);
        }
        if matches!(self.slot, Slot::Unstarted) {
            self.started.bump();
            self.slot = match (self.start)() {
                Ok(w) => Slot::Running(w),
                Err(e) => {
                    crate::warn!("paths: no worker thread ({e}); printed paths get no line");
                    self.close(paths, lines, turn);
                    return false;
                }
            };
        }
        // Another group in front than the folder's: the folder goes, and
        // the rows read against it are read again.
        if let Some(g) = ask.group {
            self.front_group = Some(g);
            if self.dir_key.is_some_and(|(d, _)| d != g) {
                self.dir_key = None;
                if paths.set_program_dir(None) == Moved::Yes {
                    lines.forget();
                    turn.frame = Owed::Yes;
                }
            }
        }
        let Slot::Running(w) = &self.slot else {
            return false;
        };
        self.next_id = self.next_id.next();
        if !w.post(ask) {
            self.close(paths, lines, turn);
            return false;
        }
        self.sent = self.sent.next();
        let beats = w.beats();
        if self.flight.is_none() {
            self.flight = Some(Flight { beats, at: now });
        }
        true
    }

    /// Marks off for this window: the worker left where it is.
    fn close(&mut self, paths: &mut Paths, lines: &mut Lines, turn: &mut Turn) {
        if matches!(self.slot, Slot::Running(_)) {
            self.counts.hangs += 1;
            crate::warn!(
                "paths: the file checks stopped answering; printed paths get no line in this window"
            );
        }
        self.slot = Slot::Closed;
        self.flight = None;
        // Lines whose writer is still to be named never will be.
        for span in self.pending() {
            paths.written_under(crate::paths::Seen::NotNamed, span);
        }
        *self.kept.edit() = Kept::default();
        self.cursors.clear();
        self.held.edit().clear();
        self.lit.clear();
        self.off.clear();
        self.publish(paths);
        paths.close();
        lines.forget();
        turn.frame = Owed::Yes;
        if self.click.take().is_some() {
            turn.clicked = Some(Clicked::Unchecked);
        }
    }

    /// **The groups kept to send, taken for a batch or the same-group
    /// shortcut**: their lines go to `inflight` as the batch goes
    /// (`Probes::send`), or are filed where the shortcut takes them, and
    /// each hands them on after.
    fn take_seen(&mut self) -> (Vec<(Pid, LineSpan)>, Option<LineSpan>) {
        let kept = self.kept.edit();
        (core::mem::take(&mut kept.seen), kept.unnamed.take())
    }
}

/// **The system the worker asks**: `std::fs` and the mount table.
#[cfg(any(target_os = "macos", target_os = "linux"))]
pub struct System;

#[cfg(any(target_os = "macos", target_os = "linux"))]
impl Fs for System {
    fn lstat(&mut self, path: &[u8]) -> Stat {
        use std::os::unix::ffi::OsStrExt;
        match std::fs::symlink_metadata(std::ffi::OsStr::from_bytes(path)) {
            Ok(m) if m.file_type().is_symlink() => Stat::Link,
            Ok(m) if m.is_dir() => Stat::Dir,
            Ok(_) => Stat::File,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                Stat::Missing
            }
            Err(_) => Stat::Other,
        }
    }

    fn readlink(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        std::fs::read_link(std::ffi::OsStr::from_bytes(path))
            .ok()
            .map(|p| p.into_os_string().into_vec())
    }

    #[cfg(target_os = "macos")]
    fn mounts(&mut self) -> Option<Mounts> {
        let mut m = Mounts::new(Fold::Ascii);
        let mut root = false;
        for fs in crate::sys::mounted()? {
            let local = match fs.f_flags & crate::sys::MNT_LOCAL {
                0 => Storage::Elsewhere,
                _ => Storage::Local,
            };
            let on = c_field(&fs.f_mntonname);
            if on == b"/" {
                root = true;
            }
            if darwin_refuses(local, &c_field(&fs.f_fstypename)) {
                m.refuse(&on);
            }
        }
        root.then_some(m)
    }

    #[cfg(target_os = "linux")]
    fn mounts(&mut self) -> Option<Mounts> {
        mountinfo(&std::fs::read("/proc/self/mountinfo").ok()?)
    }

    fn program(&mut self, group: Pid) -> Vec<String> {
        [crate::fg::name(group), crate::fg::exe_name(group)]
            .into_iter()
            .flatten()
            .collect()
    }

    fn started(&mut self, group: Pid) -> Option<StartTime> {
        crate::fg::started(group)
    }

    fn folder(&mut self, group: Pid) -> Option<Vec<u8>> {
        use std::os::unix::ffi::OsStringExt;
        crate::fg::dir(group).map(|p| p.into_os_string().into_vec())
    }

    fn argv(&mut self, group: Pid) -> Option<Vec<std::ffi::OsString>> {
        crate::fg::argv(group)
    }

    /// The leader's own executable, so the ssh asked is the one in front
    /// and not the one `PATH` finds first, in the leader's folder, so a
    /// relative `-F` or `-i` reads as the session's did; with no input,
    /// its errors dropped.
    fn resolve(&mut self, group: Pid, asked: &[std::ffi::OsString]) -> Option<Vec<u8>> {
        let (_, args) = asked.split_first()?;
        let mut cmd = crate::sys::child(crate::fg::exe(group)?);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if let Some(dir) = crate::fg::dir(group) {
            cmd.current_dir(dir);
        }
        let out = cmd.output().ok()?;
        out.status.success().then_some(out.stdout)
    }

    /// On macOS, no File Provider placeholder fetched by this thread's
    /// lookups (`sys::no_dataless_fetch`).
    fn prepare(&mut self) {
        #[cfg(target_os = "macos")]
        if !crate::sys::no_dataless_fetch() {
            crate::warn!("paths: the worker could not turn cloud file fetches off for itself");
        }
    }

    /// LaunchServices, by the fixed address: found, none, or unknown for an
    /// address it could not parse.
    #[cfg(target_os = "macos")]
    fn handler(&mut self, address: &str) -> Handler {
        match baud_present_macos::link::has_handler(address) {
            Some(true) => Handler::Found,
            Some(false) => Handler::Missing,
            None => Handler::Unknown,
        }
    }

    // Linux takes the trait's answer, unknown: `xdg-open` falls back to a
    // browser, so no answer here would mean nothing opens.

    fn path_var(&mut self) -> Option<std::ffi::OsString> {
        std::env::var_os("PATH")
    }
}

/// A C string field's bytes, to its first NUL.
#[cfg(target_os = "macos")]
fn c_field(f: &[core::ffi::c_char]) -> Vec<u8> {
    f.iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c.to_ne_bytes()[0])
        .collect()
}

/// **The Linux loop's wake**: a socket pair a worker writes a byte to after
/// each answer -- the printed paths' worker, a kitty file read's thread --
/// which the loop's `poll` waits on beside the rest.
#[cfg(target_os = "linux")]
pub struct Woken {
    read: std::os::unix::net::UnixStream,
}

#[cfg(target_os = "linux")]
impl Woken {
    /// The pair, both ends non-blocking, and the worker's call.
    pub fn pair() -> std::io::Result<(Woken, impl Fn() + Send + Sync + Clone + 'static)> {
        let (read, write) = std::os::unix::net::UnixStream::pair()?;
        read.set_nonblocking(true)?;
        write.set_nonblocking(true)?;
        let write = Arc::new(write);
        let wake = move || {
            use std::io::Write;
            let _ = (&*write).write(&[1]);
        };
        Ok((Woken { read }, wake))
    }

    pub fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        use std::os::fd::AsFd;
        self.read.as_fd()
    }

    /// What the worker wrote, read and dropped, never waiting.
    pub fn drain(&self) {
        use std::io::Read;
        let mut buf = [0u8; 64];
        while matches!((&self.read).read(&mut buf), Ok(n) if n > 0) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::{Forms, Origin};
    use baud_core::Owned;
    use baud_core::Screen;
    use baud_domain::{BytePos, ReadLen, WinSize};
    use baud_render::AbsLine;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    /// `holds_link` one byte at a time: what the block scan must answer.
    fn holds_link_bytewise(b: &[u8]) -> bool {
        (0..b.len()).any(|at| matches!(b[at], 0x1b | 0x9d | b':') && link_at(b.split_at(at + 1)))
    }

    #[test]
    fn the_link_scan_answers_as_a_byte_at_a_time_scan_does_at_every_offset() {
        let marks: [&[u8]; 8] = [
            b"\x1b]8;",
            b"\x9d8;",
            b"file://",
            b"FILE://",
            b"profile:12",
            b";file://",
            b"\x1b]9;",
            b"Makefile:12:",
        ];
        let mut asked = 0;
        for mark in marks {
            for len in mark.len()..80usize {
                for at in 0..=len - mark.len() {
                    let mut b = vec![b'a'; len];
                    b[at..at + mark.len()].copy_from_slice(mark);
                    assert_eq!(
                        holds_link(&b),
                        holds_link_bytewise(&b),
                        "{mark:?} at {at} of {len}"
                    );
                    asked += 1;
                }
            }
        }
        // The marks that are links are found at every offset, in a block or
        // in the tail past the last whole one.
        for len in [16, 17, 31, 32, 33, 47, 63, 79] {
            let mut b = vec![b'a'; len];
            b[len - 4..].copy_from_slice(b"\x1b]8;");
            assert!(holds_link(&b), "an introducer ending {len} bytes");
        }
        // A lane that is no link does not end its block's search: the link
        // after it, in the same sixteen bytes, is found.
        assert!(holds_link(b"a:b \x1b]9; file://h/aaaaaaaaaaaaaaaaaaaa"));
        assert!(asked > 2000);
    }

    /// What a stuck lookup waits on until the test lets it go.
    type Gate = Arc<Mutex<Receiver<()>>>;

    /// A file system of entries, a mount table, a call log and, optionally,
    /// a path whose lookup never returns until the test lets it.
    #[derive(Clone, Default)]
    struct Fake {
        entries: HashMap<Vec<u8>, Entry>,
        mounts: Vec<Vec<u8>>,
        program: Vec<String>,
        /// A group's own names, where it has some; else `program`.
        groups: HashMap<Pid, Vec<String>>,
        log: Arc<Mutex<Vec<Vec<u8>>>>,
        stuck: Option<(Vec<u8>, Gate)>,
        /// Set once the last copy is dropped: the worker's thread has ended.
        ends: Arc<Ends>,
        /// What the system says opens each address, and every address asked.
        handlers: HashMap<String, Handler>,
        asked: Arc<Mutex<Vec<String>>>,
        /// The folder each group's leader is in, where it has one.
        folders: HashMap<Pid, Vec<u8>>,
        /// What a group's leader is now and when it started, changed while
        /// the worker runs: a group number another process took.
        live: Live,
        /// Whether a group not in `live` has a start time that reads.
        starts: Starts,
        /// Each group leader's argv, where it has one, and every group
        /// whose argv was asked.
        argvs: HashMap<Pid, Vec<std::ffi::OsString>>,
        argv_asked: Arc<Mutex<Vec<Pid>>>,
        /// What `ssh -G` prints, and every argv it was run with, by group.
        printout: Option<Vec<u8>>,
        resolves: Resolves,
        /// Where the terminal the test feeds ends an `APC G`.
        string_end: baud_domain::ControlStringEnd,
    }

    /// Every `ssh -G` a [`Fake`] ran, as the group and its argv.
    type Resolves = Arc<Mutex<Vec<(Pid, Vec<std::ffi::OsString>)>>>;

    /// What a group's leader is now and when it started, if that reads, by
    /// group.
    type Live = Arc<Mutex<HashMap<Pid, (Vec<String>, Option<StartTime>)>>>;

    /// Whether a leader's start time reads, for a group not in `live`.
    #[derive(Clone, Copy, Default)]
    enum Starts {
        /// It reads, the same at every read.
        #[default]
        Read,
        /// It does not, as for a leader the user may not read.
        Unread,
    }

    #[derive(Default)]
    struct Ends(Arc<std::sync::atomic::AtomicBool>);

    impl Drop for Ends {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    #[derive(Clone)]
    enum Entry {
        Dir,
        File,
        Link(&'static [u8]),
    }

    impl Fake {
        fn with(entries: &[(&str, Entry)]) -> Fake {
            Fake {
                entries: entries
                    .iter()
                    .map(|(p, e)| (p.as_bytes().to_vec(), e.clone()))
                    .collect(),
                program: vec!["zsh".into()],
                ..Fake::default()
            }
        }

        /// The same, its terminal ending an `APC G` only at `BEL` or
        /// `ESC \\` (`[images] kitty_string_end = "kitty"`), where a
        /// picture's body can hold a mark the core never reads.
        fn kitty_ending(self) -> Fake {
            Fake {
                string_end: baud_domain::ControlStringEnd::Kitty,
                ..self
            }
        }

        fn logged(&self) -> Vec<String> {
            self.log
                .lock()
                .unwrap()
                .iter()
                .map(|p| String::from_utf8_lossy(p).into_owned())
                .collect()
        }
    }

    impl Fs for Fake {
        fn lstat(&mut self, path: &[u8]) -> Stat {
            self.log.lock().unwrap().push(path.to_vec());
            if let Some((p, gate)) = &self.stuck {
                if p == path {
                    let _ = gate.lock().unwrap().recv();
                }
            }
            match self.entries.get(path) {
                Some(Entry::Dir) => Stat::Dir,
                Some(Entry::File) => Stat::File,
                Some(Entry::Link(_)) => Stat::Link,
                None => Stat::Missing,
            }
        }

        fn readlink(&mut self, path: &[u8]) -> Option<Vec<u8>> {
            self.log
                .lock()
                .unwrap()
                .push([b"readlink ".as_slice(), path].concat());
            match self.entries.get(path) {
                Some(Entry::Link(t)) => Some(t.to_vec()),
                _ => None,
            }
        }

        fn mounts(&mut self) -> Option<Mounts> {
            let mut m = Mounts::new(Fold::Ascii);
            for p in &self.mounts {
                m.refuse(p);
            }
            Some(m)
        }

        fn program(&mut self, group: Pid) -> Vec<String> {
            if let Some((names, _)) = self.live.lock().unwrap().get(&group) {
                return names.clone();
            }
            self.groups
                .get(&group)
                .cloned()
                .unwrap_or_else(|| self.program.clone())
        }

        fn started(&mut self, group: Pid) -> Option<StartTime> {
            match (self.live.lock().unwrap().get(&group), self.starts) {
                (Some((_, t)), _) => *t,
                (None, Starts::Read) => Some(StartTime::AT[0]),
                (None, Starts::Unread) => None,
            }
        }

        fn folder(&mut self, group: Pid) -> Option<Vec<u8>> {
            self.log
                .lock()
                .unwrap()
                .push(format!("folder {group}").into_bytes());
            self.folders.get(&group).cloned()
        }

        fn handler(&mut self, address: &str) -> Handler {
            self.asked.lock().unwrap().push(address.to_string());
            self.handlers
                .get(address)
                .copied()
                .unwrap_or(Handler::Unknown)
        }

        fn argv(&mut self, group: Pid) -> Option<Vec<std::ffi::OsString>> {
            self.argv_asked.lock().unwrap().push(group);
            self.argvs.get(&group).cloned()
        }

        fn resolve(&mut self, group: Pid, asked: &[std::ffi::OsString]) -> Option<Vec<u8>> {
            self.resolves.lock().unwrap().push((group, asked.to_vec()));
            self.printout.clone()
        }
    }

    fn walked(fs: &mut Fake, path: &str) -> Probe {
        let mounts = fs.clone().mounts().unwrap();
        walk(fs, &mounts, path.as_bytes(), &mut 0, &|| {})
    }

    /// The shape of this Mac's `/home`: a link at the root to a folder on
    /// the data volume, where the automounter holds the map.
    fn darwin_home() -> Fake {
        let mut f = Fake::with(&[
            ("/home", Entry::Link(b"/System/Volumes/Data/home")),
            ("/System", Entry::Dir),
            ("/System/Volumes", Entry::Dir),
            ("/System/Volumes/Data", Entry::Dir),
            ("/System/Volumes/Data/home", Entry::Dir),
            ("/System/Volumes/Data/home/user", Entry::Dir),
            ("/System/Volumes/Data/home/user/x", Entry::File),
            ("/etc", Entry::Dir),
            ("/etc/hosts", Entry::File),
        ]);
        f.mounts = vec![b"/System/Volumes/Data/home".to_vec()];
        f
    }

    /// **A link in the middle of a path is followed by hand, and the mount it
    /// leads to is checked before it is looked up**: `/home/user/x` on this
    /// Mac's shape reaches the automounter's map by the link at `/home`, and
    /// no lookup is made at or under it; so is the map's own spelling in
    /// another case. A plain path is walked to its file. Perturbations: check
    /// the mount table against the printed path only; look the whole path up
    /// at once; compare mount points byte for byte on macOS.
    #[test]
    fn a_link_into_an_automount_is_refused_before_any_lookup_under_it() {
        let mut f = darwin_home();
        assert_eq!(walked(&mut f, "/etc/hosts"), Probe::Exists, "the control");
        assert_eq!(f.logged(), ["/etc", "/etc/hosts"]);
        let mut f = darwin_home();
        assert_eq!(walked(&mut f, "/home/user/x"), Probe::Unknown);
        let log = f.logged();
        assert!(
            log.iter()
                .all(|p| !p.starts_with("/System/Volumes/Data/home")),
            "{log:?}"
        );
        assert!(
            log.contains(&"readlink /home".to_string()),
            "followed by hand: {log:?}"
        );
        let mut m = Mounts::new(Fold::Ascii);
        m.refuse(b"/System/Volumes/Data/home");
        assert!(
            m.refuses(b"/SYSTEM/volumes/DATA/Home/user/x"),
            "a volume that ignores case"
        );
        let mut e = Mounts::new(Fold::Exact);
        e.refuse(b"/System/Volumes/Data/home");
        assert!(
            !e.refuses(b"/SYSTEM/volumes/DATA/Home/user/x"),
            "Linux names are bytes"
        );
    }

    /// **`/net/...` never reaches a lookup**, with the automounter's mount in
    /// the table or not, and neither does a link that leads there; a name
    /// that only starts the same is not under it. Perturbation: drop `NEVER`;
    /// compare mount points as text prefixes.
    #[test]
    fn net_never_reaches_a_lookup() {
        let mut f = Fake::with(&[
            ("/data", Entry::Link(b"/net/attacker.example/x")),
            ("/network2", Entry::Dir),
        ]);
        assert_eq!(walked(&mut f, "/net/attacker.example/x"), Probe::Unknown);
        assert!(f.logged().is_empty(), "{:?}", f.logged());
        assert_eq!(walked(&mut f, "/data/y"), Probe::Unknown);
        assert_eq!(f.logged(), ["/data", "readlink /data"]);
        let mut f = Fake::with(&[("/network2", Entry::Dir)]);
        assert_eq!(walked(&mut f, "/network2"), Probe::Exists, "not under /net");
    }

    /// **A walk says gone only where the file system did**: a component
    /// missing, or a file named as a folder; a link loop is unknown after
    /// `LINKS`, and `..` walks back over what the links reached. Perturbation:
    /// answer `Exists` for a file with more to walk; follow links without a
    /// bound.
    #[test]
    fn a_walk_says_gone_only_where_the_file_system_did() {
        let mut f = Fake::with(&[
            ("/a", Entry::Dir),
            ("/a/f", Entry::File),
            ("/loop", Entry::Link(b"/loop")),
            ("/l", Entry::Link(b"/a/b")),
            ("/a/b", Entry::Dir),
            ("/a/g", Entry::File),
        ]);
        assert_eq!(walked(&mut f, "/a/nope"), Probe::Missing);
        assert_eq!(walked(&mut f, "/a/f/x"), Probe::Missing, "ENOTDIR");
        assert_eq!(walked(&mut f, "/a/f/"), Probe::Missing, "a trailing slash");
        assert_eq!(walked(&mut f, "/loop/x"), Probe::Unknown);
        assert_eq!(walked(&mut f, "/l/../g"), Probe::Exists, "/a/b/.. is /a");
        assert_eq!(walked(&mut f, "/a/./f"), Probe::Exists);
    }

    /// **Linux's mountinfo is read by its fields**: the root's type local,
    /// a network and a FUSE mount refused with an escaped space undone, and
    /// a table with no root refuses everything. Perturbation: take a type
    /// off the allowlist's absence as local; leave `\040` as it is.
    #[test]
    fn mountinfo_refuses_every_type_not_known_local() {
        let text = b"22 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw\n\
                     30 22 0:40 / /mnt/my\\040share rw - cifs //srv/s rw\n\
                     31 22 0:41 / /home/u/remote rw - fuse.sshfs u@h: rw\n\
                     32 22 0:42 / /run/user/1000 rw - tmpfs tmpfs rw\n";
        let m = mountinfo(text).expect("a root");
        assert!(m.refuses(b"/mnt/my share/x"));
        assert!(m.refuses(b"/home/u/remote/f"));
        assert!(!m.refuses(b"/home/u/f"));
        assert!(!m.refuses(b"/run/user/1000/x"));
        assert!(m.refuses(b"/net/h/x"), "NEVER on Linux too");
        assert!(mountinfo(b"30 22 0:40 / /mnt rw - ext4 /dev/x rw\n").is_none());
        assert!(darwin_refuses(Storage::Elsewhere, b"apfs"));
        assert!(darwin_refuses(Storage::Local, b"autofs"));
        assert!(darwin_refuses(Storage::Local, b"macfuse"));
        assert!(!darwin_refuses(Storage::Local, b"apfs"));
    }

    fn os_args(v: &[&str]) -> Vec<std::ffi::OsString> {
        v.iter().map(std::ffi::OsString::from).collect()
    }

    /// One screen batch with `group` in front, answered on `f` with the
    /// worker's `sessions`.
    fn answered_for(f: &mut Fake, group: Pid, sessions: &mut Sessions) -> Answer {
        answer(
            f,
            Ask {
                id: Id::default(),
                kind: Kind::Screen,
                group: Some(group),
                cursor: LineOn {
                    surface: crate::paths::Surface::Primary,
                    line: AbsLine::at(Rows::ZERO),
                },
                paths: Vec::new(),
                seen: Vec::new(),
                unnamed: None,
                programs: Vec::new(),
            },
            &mut HashMap::new(),
            sessions,
            &|| {},
            &mut || false,
        )
    }

    /// An ssh leading group 7, to `demo` on port 2222 with a forward, and
    /// what its `ssh -G` prints.
    fn ssh_in_front() -> Fake {
        let mut f = Fake::with(&[]);
        f.groups.insert(7, vec!["ssh".into()]);
        f.argvs
            .insert(7, os_args(&["ssh", "-p", "2222", "-L", "1:h:1", "demo"]));
        f.printout = Some(b"host demo\nhostname demo.example\nport 2222\n".to_vec());
        f
    }

    /// The `ssh -G` runs `f` made, each as the group and its argv.
    fn resolves(f: &Fake) -> Vec<(Pid, Vec<std::ffi::OsString>)> {
        f.resolves.lock().unwrap().clone()
    }

    /// **The ssh in front is read once, on the worker**: its argv, and
    /// `ssh -G` with its identity options only, and every answer while it
    /// leads the group in front carries its session. Named break: no cache
    /// -- the second batch runs `ssh -G` again.
    #[test]
    fn the_ssh_in_front_is_read_once_on_the_worker() {
        let mut f = ssh_in_front();
        let mut sessions = Sessions::default();
        for n in 0..2 {
            let a = answered_for(&mut f, 7, &mut sessions);
            assert_eq!(a.front, Some(Front::Elsewhere), "batch {n}");
            let s = a.session.expect("its session");
            assert_eq!(s.ssh().dest().arg(), "demo");
            assert_eq!(
                s.resolved().hostname().map(crate::ssh::HostName::as_str),
                Some("demo.example"),
                "what ssh -G resolved"
            );
        }
        assert_eq!(
            resolves(&f),
            [(7, os_args(&["ssh", "-G", "-p", "2222", "demo"]))],
            "one ssh -G, with no forward"
        );
        assert_eq!(*f.argv_asked.lock().unwrap(), [7], "one argv read");
    }

    /// **A group number another process took is read again**: the session
    /// is kept by the leader's pid and its start, and a leader whose start
    /// cannot be read has none and asks nothing. Named breaks: key the
    /// session by the pid alone -- the second leader's `ssh -G` never runs;
    /// keep a session with no start -- the unread one answers a session.
    #[test]
    fn a_reused_group_number_is_read_again() {
        let mut f = ssh_in_front();
        let live = |f: &Fake, at| {
            f.live
                .lock()
                .unwrap()
                .insert(7, (vec!["ssh".to_string()], at));
        };
        let mut sessions = Sessions::default();
        live(&f, Some(StartTime::AT[0]));
        assert!(answered_for(&mut f, 7, &mut sessions).session.is_some());
        live(&f, Some(StartTime::AT[1]));
        assert!(answered_for(&mut f, 7, &mut sessions).session.is_some());
        assert_eq!(resolves(&f).len(), 2, "read again for the new leader");
        live(&f, None);
        assert!(
            answered_for(&mut f, 7, &mut sessions).session.is_none(),
            "no start, no session"
        );
        assert_eq!(resolves(&f).len(), 2, "and nothing asked");
    }

    /// **Only an ssh leading the group in front is read, and its session
    /// goes when another program is in front**: a kitten, also not local,
    /// has its argv asked by no one, a local shell has no session, and the
    /// ssh in front again is read again. Named breaks: read every program
    /// named not local -- the kitten's argv is asked; keep the session
    /// across another program in front -- the third batch runs no `ssh -G`.
    #[test]
    fn only_the_ssh_in_front_is_read_and_its_session_goes_with_it() {
        let mut f = ssh_in_front();
        f.groups.insert(8, vec!["kitten".into()]);
        f.argvs.insert(8, os_args(&["kitten", "ssh", "demo"]));
        f.groups.insert(9, vec!["zsh".into()]);
        let mut sessions = Sessions::default();
        let kitten = answered_for(&mut f, 8, &mut sessions);
        assert_eq!(
            kitten.front,
            Some(Front::Elsewhere),
            "the control: not local"
        );
        assert!(kitten.session.is_none());
        assert!(answered_for(&mut f, 7, &mut sessions).session.is_some());
        let shell = answered_for(&mut f, 9, &mut sessions);
        assert_eq!(shell.front, Some(Front::Local));
        assert!(shell.session.is_none());
        assert!(answered_for(&mut f, 7, &mut sessions).session.is_some());
        assert_eq!(*f.argv_asked.lock().unwrap(), [7, 7], "the ssh's only");
        assert_eq!(resolves(&f).len(), 2, "read again once back in front");
    }

    /// **An ssh whose argv is no session has none, and asks `ssh -G`
    /// nothing**: one that runs a command of its own. Named break: run
    /// `ssh -G` before reading the argv.
    #[test]
    fn an_ssh_that_is_no_session_has_none() {
        let mut f = ssh_in_front();
        f.argvs.insert(7, os_args(&["ssh", "demo", "uptime"]));
        let a = answered_for(&mut f, 7, &mut Sessions::default());
        assert!(a.session.is_none());
        assert!(resolves(&f).is_empty(), "nothing asked");
    }

    /// **A group keeps the session it was named with, gone or not**: an
    /// ssh named while it led the group in front, gone by the batch that
    /// names its rows with a shell in front, answers its session with the
    /// group's verdict, and `ssh -G` ran once, at the naming. The control:
    /// the same group gone before anything named it answers no session,
    /// and its rows are not named. Named break: keep no session in the
    /// naming (`name_group`) -- the gone group's rows answer none.
    #[test]
    fn a_group_keeps_the_session_it_was_named_with() {
        let mut f = ssh_in_front();
        f.groups.insert(9, vec!["zsh".into()]);
        let top = AbsLine::at(Rows::ZERO);
        let span = LineSpan::on(crate::paths::Surface::Primary, top, top);
        // A batch with the shell in front and the ssh's rows seen.
        let batch = |f: &mut Fake, named: &mut Named, sessions: &mut Sessions| {
            answer(
                f,
                Ask {
                    id: Id::default(),
                    kind: Kind::Screen,
                    group: Some(9),
                    cursor: LineOn {
                        surface: crate::paths::Surface::Primary,
                        line: top,
                    },
                    paths: Vec::new(),
                    seen: vec![(7, span)],
                    unnamed: None,
                    programs: Vec::new(),
                },
                named,
                sessions,
                &|| {},
                &mut || false,
            )
        };
        let gone = |f: &Fake| {
            f.live.lock().unwrap().insert(7, (Vec::new(), None));
        };
        let (mut named, mut sessions) = (Named::new(), Sessions::default());
        name_group(&mut f, &mut named, &mut sessions, 7, &mut 0, &|| {});
        gone(&f);
        let a = batch(&mut f, &mut named, &mut sessions);
        assert_eq!(
            a.seen,
            [(Some(7), crate::paths::Seen::Named(Front::Elsewhere))]
        );
        let [(g, s)] = &a.sessions[..] else {
            panic!("one session: {:?}", a.sessions);
        };
        assert_eq!(*g, 7);
        assert_eq!(s.ssh().dest().arg(), "demo");
        assert_eq!(resolves(&f).len(), 1, "ssh -G at the naming only");
        // The control: gone before it was named.
        let mut f = ssh_in_front();
        f.groups.insert(9, vec!["zsh".into()]);
        gone(&f);
        let a = batch(&mut f, &mut Named::new(), &mut Sessions::default());
        assert_eq!(a.seen, [(Some(7), crate::paths::Seen::NotNamed)]);
        assert!(a.sessions.is_empty(), "{:?}", a.sessions);
    }

    /// **Main reads no argv and runs no ssh**: `answers` takes the sessions
    /// from the worker's answer and calls nothing of its own: the one in
    /// front goes to the paths (`Paths::set_session`), what its group's rows
    /// are filed as printed by while it stays in front, and each seen
    /// group's is filed with its verdict (`Probes::file_answered`), what a
    /// remote program's `file://` link on those rows opens through. Named
    /// breaks: read the session in `answers`; file the verdicts without the
    /// groups' sessions.
    #[test]
    fn main_takes_the_session_with_no_call_of_its_own() {
        let src = include_str!("probe.rs");
        let from = src.find("    pub fn answers(").expect("answers");
        let to = from + src[from..].find("\n    }\n").expect("its end");
        let body = &src[from..to];
        for call in [
            concat!("ssh", "::"),
            concat!("resolve", "("),
            concat!("argv", "("),
            concat!("child", "("),
        ] {
            assert!(!body.contains(call), "answers calls {call}");
        }
        assert!(
            body.contains(concat!("a.", "session")),
            "the positive control: the session is taken"
        );
        assert!(
            body.contains("s.paths.set_session(a.session);"),
            "the session in front reaches the paths"
        );
        assert!(
            body.contains("(a.id, a.kind), &a.seen, &a.sessions);"),
            "each seen group's session is filed with its verdict"
        );
    }

    /// **The program in front is named first, and under ssh nothing is
    /// looked up**, its folder included; a local one's folder is read
    /// before its paths; under a multiplexer only `./` is left out. Perturbation:
    /// walk before `front_of`; drop the relative test.
    #[test]
    fn ssh_in_front_looks_nothing_up() {
        let cand = |p: &str, form| Candidate {
            path: p.as_bytes().to_vec(),
            form,
            line: AbsLine::at(Rows::ZERO),
        };
        let ask = |names: &[&str]| {
            let mut f = Fake::with(&[
                ("/etc", Entry::Dir),
                ("/etc/hosts", Entry::File),
                ("/w", Entry::Dir),
                ("/w/x", Entry::File),
            ]);
            f.program = names.iter().map(|s| s.to_string()).collect();
            let a = answer(
                &mut f,
                Ask {
                    id: Id::default(),
                    kind: Kind::Screen,
                    group: Some(7),
                    cursor: LineOn {
                        surface: crate::paths::Surface::Primary,
                        line: AbsLine::at(Rows::ZERO),
                    },
                    paths: vec![
                        cand("/etc/hosts", crate::paths::Form::Absolute),
                        cand("/w/x", crate::paths::Form::Dot),
                    ],
                    seen: Vec::new(),
                    unnamed: None,
                    programs: Vec::new(),
                },
                &mut HashMap::new(),
                &mut Sessions::default(),
                &|| {},
                &mut || false,
            );
            (a.front, a.probes.len(), f.logged())
        };
        assert_eq!(
            ask(&["zsh"]),
            (
                Some(Front::Local),
                2,
                vec![
                    "folder 7".into(),
                    "/etc".into(),
                    "/etc/hosts".into(),
                    "/w".into(),
                    "/w/x".into()
                ]
            )
        );
        let (front, n, log) = ask(&["ssh"]);
        assert_eq!((front, n), (Some(Front::Elsewhere), 0));
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(
            ask(&["unreadable"; 0]).0,
            Some(Front::Elsewhere),
            "none read: not known local"
        );
        assert_eq!(ask(&[".herdr-wrapped"]).1, 1, "a multiplexer: no ./");
        assert_eq!(
            front_of(&["tmux: client (/tmp/x)".into()]),
            Front::Multiplexer
        );
    }

    /// **The batch is due 150 ms after the last read, at most a second after
    /// the path was met while output goes on, and never two in a second.**
    /// Perturbation: drop the ceiling from `due_at`; drop the floor after the
    /// last batch.
    #[test]
    fn a_repaint_loop_still_gets_a_batch_a_second() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        assert_eq!(p.due_at(), None, "nothing owed");
        p.owe(ms(0), Owes::Path);
        assert_eq!(p.due_at(), Some(ms(150)), "quiet after the read");
        // A repaint every 50 ms.
        for k in 1..40u64 {
            p.saw(Taken(usize::try_from(k).unwrap()), ms(k * 50));
        }
        assert_eq!(
            p.due_at(),
            Some(ms(1000)),
            "the ceiling, while output goes on"
        );
        p.scanned(ms(1000));
        p.sent(ms(1000), &[asked_path()]);
        p.owe(ms(1010), Owes::Path);
        p.saw(Taken(99), ms(1020));
        assert_eq!(p.due_at(), Some(ms(2000)), "never two in a second");
    }

    /// A path a batch asks about.
    fn asked_path() -> Candidate {
        Candidate {
            path: b"/w/file_01.txt".to_vec(),
            form: crate::paths::Form::Absolute,
            line: AbsLine::at(Rows::ZERO),
        }
    }

    /// **A batch that only named the writer holds no floor**: a listing's
    /// rows are read once the batch that names its writer is answered, and
    /// the batch that asks about their paths then goes at its own deadline
    /// -- the frame that met them, long past the listing's [`SETTLE`] --
    /// and not [`CEILING`] after the naming one. So too after a typed key's
    /// echo: `l` and `s` echoed, each named [`SETTLE`] after it, then the
    /// listing. Break: the floor armed by any batch (`Pace::sent` setting
    /// the last batch whatever it asked), under which each path batch is
    /// held to a second after the naming batch before it.
    #[test]
    fn a_batch_that_only_named_its_writer_holds_no_floor() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        // Cold: the listing read, its rows not yet readable.
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        assert_eq!(p.due_at(), Some(ms(150)), "quiet after the listing");
        assert_eq!(p.due().map(|d| d.by), Some(By::Quiet));
        p.scanned(ms(150));
        p.sent(ms(150), &[]);
        // Its answer filed the rows; the next frame's scan met the paths.
        p.owe(ms(152), Owes::Path);
        assert_eq!(
            p.due().map(|d| d.by),
            Some(By::Quiet),
            "nothing asked: no floor"
        );
        assert_eq!(p.due_at(), Some(ms(152)), "at once, not at 1150 ms");
        // Typed: `l` at 0, `s` at 120, each echo named once quiet, then the
        // listing at 420 and its paths met once the naming is answered.
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        p.saw(Taken(2), ms(120));
        assert_eq!(p.due_at(), Some(ms(270)));
        p.scanned(ms(270));
        p.sent(ms(270), &[]);
        p.saw(Taken(40), ms(420));
        p.owe(ms(420), Owes::Path);
        assert_eq!(p.due_at(), Some(ms(570)), "the listing's own quiet");
        p.scanned(ms(570));
        p.sent(ms(570), &[]);
        p.owe(ms(572), Owes::Path);
        assert_eq!(p.due().map(|d| d.by), Some(By::Quiet));
        assert_eq!(p.due_at(), Some(ms(572)), "at once, not at 1570 ms");
        // A batch that asked about a path still holds the next.
        p.scanned(ms(572));
        p.sent(ms(572), &[asked_path()]);
        p.owe(ms(900), Owes::Path);
        assert_eq!(p.due().map(|d| d.by), Some(By::Floor));
        assert_eq!(p.due_at(), Some(ms(1572)), "a second after the asking");
    }

    /// **Batches that ask about a path go about once a second while output
    /// goes on, a flood's or a spinner's**: every read rewrites a path, met
    /// by the frame after it, and each batch asks about it. A flood, a read
    /// every 10 ms, never goes quiet, and [`CEILING`] after the path was met
    /// sends each; a spinner, a read every 200 ms, goes quiet [`SETTLE`]
    /// after each, and the floor after the last batch that asked holds the
    /// next. The control for [`a_batch_that_only_named_its_writer_holds_no_floor`]:
    /// green with the floor armed by any batch as well. Perturbations: drop
    /// the ceiling from `due_at`, and the flood is never asked about; drop
    /// the floor, and the spinner is asked about five times a second.
    #[test]
    fn a_flood_or_a_spinner_asks_about_paths_once_a_second() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        for (name, every) in [("flood", 10_u64), ("spinner", 200)] {
            let mut p: Pace<Instant> = Pace::default();
            let mut sent: Vec<u64> = Vec::new();
            let mut taken = 0_usize;
            // Ten seconds of turns 5 ms apart.
            for now in (0..10_000_u64).step_by(5) {
                if now % every == 0 {
                    taken += 1;
                    p.saw(Taken(taken), ms(now));
                }
                p.owe(ms(now), Owes::Path);
                if p.due_at().is_some_and(|d| ms(now) >= d) {
                    p.scanned(ms(now));
                    p.sent(ms(now), &[asked_path()]);
                    sent.push(now);
                }
            }
            assert!(
                (9..=10).contains(&sent.len()),
                "{name}: {} batches in ten seconds: {sent:?}",
                sent.len()
            );
            for w in sent.windows(2) {
                assert!(w[1] - w[0] >= 1000, "{name}: two in a second: {sent:?}");
            }
        }
    }

    /// **A spinner that prints no path is named at each pause, and never
    /// more often than [`SETTLE`]**: a read every 160 ms for ten seconds,
    /// each met by the frame after it, and every batch only names the
    /// writer. It owes only at a read, where the flood control owes every
    /// turn: a batch's collection clears what is owed, and only a changed
    /// row owes again, which is the bound's premise. A naming batch holds
    /// no floor, so each goes [`SETTLE`] after its read, one a read: 62 of
    /// the 63 reads, the last one's quiet falling past ten seconds. Two are
    /// never closer than [`SETTLE`] -- the next is owed only by a read
    /// after this one and waits out the quiet after it -- so ten seconds
    /// hold at most 1 + 10 s / [`SETTLE`] = 67, and no second more than
    /// 1 + 1 s / [`SETTLE`] = 7. Under the floor armed by any batch
    /// (`Pace::sent` setting the last batch whatever it asked) the same
    /// spinner is named about once a second, 10.
    #[test]
    fn a_spinner_that_prints_no_path_is_named_never_closer_than_settle() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let settle = u64::try_from(SETTLE.as_millis()).unwrap();
        let mut p: Pace<Instant> = Pace::default();
        let mut sent: Vec<u64> = Vec::new();
        let mut taken = 0_usize;
        // Ten seconds of turns 5 ms apart.
        for now in (0..10_000_u64).step_by(5) {
            if now % 160 == 0 {
                taken += 1;
                p.saw(Taken(taken), ms(now));
                p.owe(ms(now), Owes::Rows);
            }
            if p.due_at().is_some_and(|d| ms(now) >= d) {
                p.scanned(ms(now));
                p.sent(ms(now), &[]);
                sent.push(now);
            }
        }
        for w in sent.windows(2) {
            assert!(w[1] - w[0] >= settle, "closer than SETTLE: {sent:?}");
        }
        assert!(
            sent.len() <= usize::try_from(1 + 10_000 / settle).unwrap(),
            "{} batches in ten seconds: {sent:?}",
            sent.len()
        );
        for (i, &s) in sent.iter().enumerate() {
            let second = sent[i..].iter().take_while(|&&t| t < s + 1000).count();
            assert!(
                second <= usize::try_from(1 + 1000 / settle).unwrap(),
                "{second} batches in a second from {s} ms"
            );
        }
        let quiet_in_time = (0..10_000_u64)
            .step_by(160)
            .filter(|r| r + settle < 10_000)
            .count();
        assert_eq!(
            sent.len(),
            quiet_in_time,
            "one a read, SETTLE after it: {sent:?}"
        );
    }

    /// **The proof the window's shell ended its command**, made the one
    /// way there is: the shell's `D` with [`TOKEN`] and its `A`, fed to a
    /// terminal and read by the window's scan, the latch stepped over the
    /// part (`vouch::Latch::step`).
    fn shells_d() -> crate::vouch::Ended {
        let bytes =
            format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07").into_bytes();
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let rec = t.take_record();
        let mut scan = crate::ran::Commands::new(Some(&crate::ran::ShellToken::given(TOKEN)));
        let _ = scan.read(&bytes);
        let (_, _, ended) = crate::vouch::Latch::Synced.step(&rec, &scan.marks());
        ended.expect("the shell's D ends its command")
    }

    /// A deadline, due `by`, a grant having set it or not.
    fn due(at: Instant, by: By, granted: Granted) -> Option<Due<Instant>> {
        Some(Due { at, by, granted })
    }

    /// **T1: the shell's `D` makes the batch due at once**: a read and a
    /// path met at 0, and the shell's `D` in the drain at 0, due at 0 by
    /// the `D`. The control, no `D`: [`SETTLE`] after the read, by the
    /// quiet. Breaks: `due` blind to the grant -- 150 ms, by the quiet;
    /// the grant's deadline reported as the quiet's -- by the quiet.
    #[test]
    fn the_shells_d_makes_the_batch_due_at_once() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        assert_eq!(p.due(), due(ms(150), By::Quiet, Granted::No), "the control");
        p.ended(shells_d(), ms(0));
        assert_eq!(p.due(), due(ms(0), By::Ended, Granted::Yes));
        assert_eq!(p.due_at(), Some(ms(0)), "due_at is due's instant");
    }

    /// **T2: a batch that only names keeps the grant; one that asks spends
    /// it**: due at once by the `D`; a batch naming the listing's writer at
    /// 0; its answer makes the rows readable, met at 1 ms: due by the `D`
    /// again, [`GRANT_GAP`] after the batch before. That batch asks about a
    /// path and spends it: met at 9 ms, the floor a second after it.
    /// Breaks: any batch spends the grant -- the second is due 150 ms
    /// after the read, by the quiet; no gap after the last batch -- due at
    /// 1 ms.
    #[test]
    fn a_naming_batch_keeps_the_grant() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        p.ended(shells_d(), ms(0));
        assert_eq!(p.due(), due(ms(0), By::Ended, Granted::Yes));
        p.scanned(ms(0));
        p.sent(ms(0), &[]);
        p.owe(ms(1), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(gap), By::Ended, Granted::Yes),
            "kept, a gap after the naming batch"
        );
        p.scanned(ms(gap));
        p.sent(ms(gap), &[asked_path()]);
        p.owe(ms(gap + 1), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(gap + 1000), By::Floor, Granted::No),
            "spent: the floor"
        );
    }

    /// A pace whose last batch asked about a path at `t0`, so the floor
    /// holds to a second after, and whose shell's `D` came at `d`.
    fn floored_then_d(t0: Instant, d: Instant) -> Pace<Instant> {
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), t0);
        p.owe(t0, Owes::Path);
        p.scanned(t0);
        p.sent(t0, &[asked_path()]);
        p.saw(Taken(2), d);
        p.ended(shells_d(), d);
        p
    }

    /// **T3: the grant's one asking batch lifts the floor, once**: a batch
    /// asked about a path at 0; the shell's `D` at 200 ms, a path met then:
    /// due at once, by the `D`, the floor a second after the asking passed.
    /// The batch naming the listing's writer goes at 200, the one asking
    /// about its paths [`GRANT_GAP`] after, and that one spends the grant
    /// and lays the floor again: a path met at 300 with no new `D` is due a
    /// second after it, by the floor and not the `D`. The same where one
    /// batch both names and asks, the typed listing's shape: met at 300, the
    /// floor. Breaks: no lift -- due at 1000, by the floor; the lift
    /// outliving its asking batch (`Pace::sent` keeping the grant) -- the
    /// one-batch path met at 300 due at once, by the `D`, and the two-batch
    /// one held with the `D`'s mark on it.
    #[test]
    fn the_grants_one_asking_batch_lifts_the_floor_once() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut p = floored_then_d(t0, ms(200));
        p.owe(ms(200), Owes::Path);
        assert_eq!(p.due(), due(ms(200), By::Ended, Granted::Yes), "lifted");
        p.scanned(ms(200));
        p.sent(ms(200), &[]);
        p.owe(ms(201), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(200 + gap), By::Ended, Granted::Yes),
            "the asking batch, a gap after the naming one"
        );
        p.scanned(ms(200 + gap));
        p.sent(ms(200 + gap), &[asked_path()]);
        p.saw(Taken(3), ms(300));
        p.owe(ms(300), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(1200 + gap), By::Floor, Granted::No),
            "spent: the floor the asking batch laid"
        );
        let mut p = floored_then_d(t0, ms(200));
        p.owe(ms(200), Owes::Path);
        p.scanned(ms(200));
        p.sent(ms(200), &[asked_path()]);
        p.saw(Taken(3), ms(300));
        p.owe(ms(300), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(1200), By::Floor, Granted::No),
            "one batch named and asked: spent"
        );
    }

    /// **W1: a grant passes the floor with two scans, and no third**: the
    /// floor live to a second; the shell's `D` at 200 ms; two batches pass
    /// it [`GRANT_GAP`] apart, neither asking; a path met after them, the
    /// grant still live, is due at the floor, the `D`'s mark on it, and
    /// counted held under it (`Counts::held_granted`). Breaks: no cap (a
    /// [`Lift`] never spent) -- the third due at once; no lift -- the first
    /// due at the floor.
    #[test]
    fn a_grant_lifts_the_floor_for_two_batches() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut p = floored_then_d(t0, ms(200));
        p.owe(ms(200), Owes::Path);
        assert_eq!(p.due(), due(ms(200), By::Ended, Granted::Yes), "first");
        p.scanned(ms(200));
        p.sent(ms(200), &[]);
        p.owe(ms(201), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(200 + gap), By::Ended, Granted::Yes),
            "second"
        );
        p.scanned(ms(200 + gap));
        p.sent(ms(200 + gap), &[]);
        p.owe(ms(201 + gap), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(1000), By::Floor, Granted::Yes),
            "the third, held"
        );
    }

    /// **A scan that sends nothing spends its lift, and is spaced like
    /// one that does**: the floor live; the shell's `D` at 200 ms; a scan
    /// at 200 that finds nothing to send; a path met after it is due
    /// [`GRANT_GAP`] after that scan; a second such scan, and the third is
    /// held by the floor. Breaks: the gap and the step stamped in
    /// `Pace::sent` alone -- the second due at once; no cap -- the third due
    /// a gap after the second; no lift -- the second due at the floor.
    #[test]
    fn a_scan_that_sends_nothing_spends_its_lift() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut p = floored_then_d(t0, ms(200));
        p.owe(ms(200), Owes::Path);
        p.scanned(ms(200));
        p.owe(ms(201), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(200 + gap), By::Ended, Granted::Yes),
            "spaced after the empty scan"
        );
        p.scanned(ms(200 + gap));
        p.owe(ms(201 + gap), Owes::Path);
        assert_eq!(
            p.due(),
            due(ms(1000), By::Floor, Granted::Yes),
            "two empty scans spent it"
        );
    }

    /// **W1 under a held key: each `D` passes the floor with two scans**:
    /// a batch asked about a path at 0, so the floor holds to a second;
    /// then twenty of the shell's `D`s 33 ms apart from 100 ms, as Enter
    /// repeats, with a turn every 4 ms, a read and a path owed at each,
    /// and every scan sending a batch that only names -- the most a held
    /// key can owe, a path on a new row at every frame. Between one `D` and
    /// the next exactly two scans pass the floor, by the `D` and
    /// [`GRANT_GAP`] apart, and the rest wait. Breaks: no cap (a [`Lift`]
    /// never spent) -- five between one `D` and the next, one each
    /// [`GRANT_GAP`]; no lift -- none.
    #[test]
    fn each_d_of_a_held_key_passes_the_floor_with_two_scans() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        p.scanned(ms(0));
        p.sent(ms(0), &[asked_path()]);
        let mut read = 1;
        for k in 0..20_u64 {
            let d = 100 + 33 * k;
            p.ended(shells_d(), ms(d));
            let mut passed = Vec::new();
            for now in (d..d + 33).step_by(4) {
                read += 1;
                p.saw(Taken(read), ms(now));
                p.owe(ms(now), Owes::Path);
                if let Some(due) = p.due().filter(|due| ms(now) >= due.at) {
                    p.scanned(ms(now));
                    p.sent(ms(now), &[]);
                    passed.push((now - d, due.by));
                }
            }
            assert_eq!(
                passed,
                [(0, By::Ended), (gap, By::Ended)],
                "between D {k} and the next"
            );
        }
    }

    /// **T4: a grant lapses [`SETTLE`] after its drain**: the `D` at 0,
    /// nothing owed; a read at 200 ms and a path met then: due 150 ms
    /// after that read, by the quiet. Break: no lapse -- due at 200 ms, by
    /// the `D`. Without the second read the two would agree.
    #[test]
    fn a_grant_lapses_after_settle() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.ended(shells_d(), ms(0));
        p.saw(Taken(2), ms(200));
        p.owe(ms(200), Owes::Path);
        assert_eq!(p.due(), due(ms(350), By::Quiet, Granted::No));
    }

    /// **T5: a command opened or a part not trusted ends the grant**
    /// (`Pace::lapse`): due by the quiet. Break: `lapse` does nothing --
    /// due at once, by the `D`.
    #[test]
    fn a_c_or_an_untrusted_part_ends_the_grant() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.owe(ms(0), Owes::Path);
        p.ended(shells_d(), ms(0));
        p.lapse();
        assert_eq!(p.due(), due(ms(150), By::Quiet, Granted::No));
    }

    /// **T6: a grant is stamped when its drain mints it, not when a turn
    /// first sees it**: the `D` at 0 owing nothing, then ten seconds on an
    /// echo's read holding a path, met then: due [`SETTLE`] after it, by
    /// the quiet, as with no `D`. Break: the grant stamped by the first
    /// path met after it (the macOS pump's tick runs before its drain) --
    /// due at once, and a partial path typed asked about early.
    #[test]
    fn a_grant_is_stamped_when_it_is_minted() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.ended(shells_d(), ms(0));
        p.saw(Taken(2), ms(10_000));
        p.owe(ms(10_000), Owes::Path);
        assert_eq!(p.due(), due(ms(10_150), By::Quiet, Granted::No));
    }

    /// The `D` at 0, a read every millisecond so the quiet never comes, a
    /// batch owed for `owes` at each turn, a turn each millisecond for 400
    /// ms, and every batch only naming, so none spends the grant: each
    /// batch sent, and why it was due.
    fn a_grant_under_turns(owes: Owes) -> Vec<(u64, By)> {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.ended(shells_d(), ms(0));
        let mut sent: Vec<(u64, By)> = Vec::new();
        for (k, now) in (0..400_u64).enumerate() {
            p.saw(Taken(k + 1), ms(now));
            p.owe(ms(now), owes);
            if let Some(d) = p.due().filter(|d| ms(now) >= d.at) {
                p.scanned(ms(now));
                p.sent(ms(now), &[]);
                sent.push((now, d.by));
            }
        }
        sent
    }

    /// **One grant scans at most 20 times, none closer than
    /// [`GRANT_GAP`]**: [`a_grant_under_turns`], a path owed at each turn.
    /// Batches go at 0, 8, ... 144 ms, and at 152, met at 145, the last met
    /// before the grant lapses at [`SETTLE`]: 20, the bound
    /// `Pace::scanned` and [`GRANT_GAP`] state. After that only the
    /// ceiling, a second after the path was met. Break: no gap after the
    /// last scan -- 150.
    #[test]
    fn one_grant_sends_at_most_twenty_batches_a_frame_apart() {
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let sent = a_grant_under_turns(Owes::Path);
        let by_d: Vec<u64> = sent
            .iter()
            .filter(|s| s.1 == By::Ended)
            .map(|s| s.0)
            .collect();
        assert_eq!(by_d.len(), 20, "{sent:?}");
        assert_eq!(by_d.last(), Some(&152), "{sent:?}");
        for w in sent.windows(2) {
            assert!(w[1].0 - w[0].0 >= gap, "closer than GRANT_GAP: {sent:?}");
        }
        assert_eq!(sent.len(), 20, "nothing else within 400 ms: {sent:?}");
    }

    /// **Rows that changed alone take the plain pace under a grant**: a
    /// read and a row changed at 0, the `D` at 0: due [`SETTLE`] after the
    /// read, by the quiet; then a path met at 1 ms: due at once, by the
    /// `D`. And [`a_grant_under_turns`] with only rows owed -- a prompt
    /// with no path at a held key -- sends nothing within 400 ms, the
    /// quiet never coming and the ceiling a second off. Break: the grant
    /// hastening any owing -- due at 0, by the `D`, and 20 batches.
    #[test]
    fn rows_alone_take_the_plain_pace_under_a_grant() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.saw(Taken(1), ms(0));
        p.ended(shells_d(), ms(0));
        p.owe(ms(0), Owes::Rows);
        assert_eq!(p.due(), due(ms(150), By::Quiet, Granted::No), "rows");
        p.owe(ms(1), Owes::Path);
        assert_eq!(p.due(), due(ms(1), By::Ended, Granted::Yes), "a path");
        let sent = a_grant_under_turns(Owes::Rows);
        assert_eq!(sent, Vec::new(), "rows alone under turns");
    }

    /// **A path met past the grant's lapse is paced by the quiet, whatever
    /// was owed before it**: the `D` at 0, a row changed at 1 ms, reads
    /// every 33 ms, and a path met at 200 ms, [`SETTLE`] past the mint: due
    /// [`SETTLE`] after the last read, by the quiet. Break: the grant's
    /// lapse judged by when anything was first owed -- due at 1 ms, by the
    /// `D`, the row's owing carrying the later path into the grant.
    #[test]
    fn a_path_met_past_the_lapse_is_paced_by_the_quiet() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut p: Pace<Instant> = Pace::default();
        p.ended(shells_d(), ms(0));
        p.owe(ms(1), Owes::Rows);
        for (k, at) in (0..=198_u64).step_by(33).enumerate() {
            p.saw(Taken(k + 1), ms(at));
        }
        p.owe(ms(200), Owes::Path);
        assert_eq!(p.due(), due(ms(198 + 150), By::Quiet, Granted::No));
    }

    /// A probe set for a window with `fs`, the clock at `t0`.
    fn probes(fs: Fake) -> (Probes<Instant>, Arc<Mutex<usize>>) {
        let starts = Arc::new(Mutex::new(0));
        let counted = Arc::clone(&starts);
        let mut fs = Some(fs);
        let p = Probes::new(Box::new(move || {
            *counted.lock().unwrap() += 1;
            Worker::start(Box::new(fs.take().expect("started once")), Box::new(|| {}))
        }));
        (p, starts)
    }

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 4,
    };

    /// **A stuck lookup stops nothing but the worker**: the turn that sends
    /// it returns, turns after it return with drawing owed, no answer comes,
    /// and at `HANG` with no beat marks close for the window; later turns
    /// send nothing and start no second thread, and the thread ends once its
    /// call returns. Time is the test's clock, not a sleep. Perturbations:
    /// wait for the answer in `turn`; start a worker per batch; never close.
    ///
    /// Under the probe, the fed control of the frame rig's count of hangs,
    /// read through the probe as the rig reads it (`app::record_unnamed`,
    /// `probe::paths_hangs`): 0 before the hang, 1 once marks close. Named
    /// break: the count's increment in `Probes::close` removed -- it reads
    /// 0 after the close too.
    #[test]
    fn a_stuck_lookup_closes_marks_and_starts_no_second_thread() {
        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let mut f = Fake::with(&[("/etc", Entry::Dir)]);
        f.stuck = Some((b"/etc/hosts".to_vec(), Arc::new(Mutex::new(gate_rx))));
        let log = Arc::clone(&f.log);
        let ended = Arc::clone(&f.ends.0);
        let (mut pr, starts) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.all_written_here();
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let turn = |pr: &mut Probes<Instant>,
                    now: Instant,
                    paths: &mut Paths,
                    lines: &mut Lines,
                    uris: &mut Uris| {
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut *uris,
                    urls: Turned::On,
                    paths: &mut *paths,
                },
            );
            pr.turn(
                now,
                Seen {
                    term: &t,
                    scrolled: s,
                    lines,
                    uris,
                    urls: Turned::On,
                    paths,
                },
                || Some(1),
            )
        };
        let _ = turn(&mut pr, at(0), &mut paths, &mut lines, &mut uris);
        let batches = pr.counts.batches;
        let _ = turn(&mut pr, at(200), &mut paths, &mut lines, &mut uris);
        assert_eq!(pr.counts.batches, batches + 1, "the batch went");
        assert_eq!(*starts.lock().unwrap(), 1);
        // The worker reaches the stuck lookup; the test waits for that call.
        while !log.lock().unwrap().iter().any(|p| p == b"/etc/hosts") {
            std::thread::yield_now();
        }
        let r = turn(&mut pr, at(1_000), &mut paths, &mut lines, &mut uris);
        assert_eq!(r.clicked, None);
        assert_eq!(paths.marks(), Marks::On, "not yet");
        #[cfg(all(target_os = "macos", feature = "effect-probe"))]
        let from = {
            crate::app::record_unnamed(&pr);
            let from = baud_present_macos::probe::paths_hangs();
            assert_eq!(from.0, 0, "the control: no hang counted before it");
            from
        };
        let r = turn(
            &mut pr,
            at(1_000 + 5_001),
            &mut paths,
            &mut lines,
            &mut uris,
        );
        assert_eq!(r.frame, Owed::Yes, "closing owes a frame");
        assert_eq!(paths.marks(), Marks::Closed);
        #[cfg(all(target_os = "macos", feature = "effect-probe"))]
        {
            crate::app::record_unnamed(&pr);
            assert_eq!(
                baud_present_macos::probe::paths_hangs()
                    .0
                    .saturating_sub(from.0),
                1,
                "the hang, read through the probe as the frame rig reads it"
            );
        }
        assert_eq!(pr.counts.hangs, 1);
        for k in 0..5 {
            let _ = turn(
                &mut pr,
                at(7_000 + k * 2_000),
                &mut paths,
                &mut lines,
                &mut uris,
            );
        }
        assert_eq!(*starts.lock().unwrap(), 1, "no second thread");
        let clicked = pr.click(
            at(20_000),
            Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
            Some(1),
            Candidate {
                path: b"/etc/hosts".to_vec(),
                form: crate::paths::Form::Absolute,
                line: AbsLine::at(Rows::ZERO),
            },
        );
        assert_eq!(
            clicked,
            Some(Clicked::Unchecked),
            "a click says it could not check"
        );
        // Let the call return: dropping the probes drops the sender, and the
        // thread ends, dropping its file system.
        assert!(!ended.load(Ordering::SeqCst), "the control: still stuck");
        gate_tx.send(()).unwrap();
        drop(pr);
        while !ended.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
    }

    /// **A click asks again and answers in a later turn**: found, it hands
    /// back the path to reveal; gone since the line, it says so. The click
    /// itself waits on nothing. Perturbation: answer a click from the
    /// cached result.
    #[test]
    fn a_click_asks_again_off_the_frame() {
        let f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let mut ask = |pr: &mut Probes<Instant>, path: &str| {
            let c = Candidate {
                path: path.as_bytes().to_vec(),
                form: crate::paths::Form::Absolute,
                line: AbsLine::at(Rows::ZERO),
            };
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            assert_eq!(pr.click(t0, seen, Some(1), c), None, "asked, not answered");
            loop {
                let seen = Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                };
                if let Some(c) = pr.turn(t0, seen, || Some(1)).clicked {
                    return c;
                }
                std::thread::yield_now();
            }
        };
        assert_eq!(
            ask(&mut pr, "/etc/hosts"),
            Clicked::Exists(b"/etc/hosts".to_vec())
        );
        assert_eq!(ask(&mut pr, "/etc/gone"), Clicked::Missing);
    }

    /// **A frame that owes a batch before any turn has timed it bounds the
    /// wait at once**, so a loop that waits on its deadline never sleeps past
    /// it; once timed, the deadline is the settle's, and with nothing owed
    /// there is none. Perturbation: answer `due_at()` alone in `deadline`.
    #[test]
    fn the_first_owed_batch_bounds_the_wait() {
        let (mut pr, _) = probes(Fake::default());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let t0 = Instant::now();
        assert_eq!(pr.deadline(&paths, t0), None, "nothing owed");
        let _ = lines.scan(
            &t,
            Scrolled::START,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        assert_eq!(pr.deadline(&paths, t0), Some(t0), "owed, untimed: now");
        let seen = Seen {
            term: &t,
            scrolled: Scrolled::START,
            lines: &mut lines,
            uris: &mut uris,
            urls: Turned::On,
            paths: &mut paths,
        };
        let _ = pr.turn(t0, seen, || Some(1));
        assert_eq!(
            pr.deadline(&paths, t0),
            Some(t0 + SETTLE),
            "timed: the settle"
        );
    }

    /// **A kind of link is asked about by its fixed address, once a window
    /// and again only when owed**: a screen of a thousand URLs a program
    /// printed asks the three addresses once each, none holding a byte of
    /// the screen's; more turns ask nothing; `owe_programs` asks them once
    /// more. The answer strikes the kind with no app. Perturbations: ask
    /// with the URI found in the text; ask on every turn; drop the
    /// `programs_owed` reset in `turn`.
    #[test]
    fn a_kind_of_link_is_asked_at_its_fixed_address_once() {
        use crate::link::Scheme;
        let mut f = Fake::with(&[]);
        f.handlers
            .insert("https://example.invalid/".into(), Handler::Missing);
        f.handlers
            .insert("http://example.invalid/".into(), Handler::Found);
        let asked = Arc::clone(&f.asked);
        let (mut pr, _) = probes(f);
        let kinds = [Scheme::Https, Scheme::Http, Scheme::Mailto];
        pr.set_programs(kinds.into_iter().map(Program::Scheme).collect());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let flood = "https://evil.example/q?x=1 ".repeat(1000);
        for c in flood.as_bytes().chunks(4096) {
            t.write_at(BytePos::ZERO, c);
            t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        }
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let turn = |pr: &mut Probes<Instant>,
                    after: Duration,
                    paths: &mut Paths,
                    lines: &mut Lines,
                    uris: &mut Uris| {
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut *uris,
                    urls: Turned::On,
                    paths: &mut *paths,
                },
            );
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines,
                uris,
                urls: Turned::On,
                paths,
            };
            pr.turn(t0 + after, seen, || Some(1))
        };
        let https = Program::Scheme(Scheme::Https);
        while paths.program(&https) == Handler::Unknown {
            let _ = turn(&mut pr, Duration::ZERO, &mut paths, &mut lines, &mut uris);
            std::thread::yield_now();
        }
        let fixed = [
            "https://example.invalid/",
            "http://example.invalid/",
            "mailto:nobody@example.invalid",
        ];
        assert_eq!(*asked.lock().unwrap(), fixed, "each fixed address, once");
        assert!(
            asked.lock().unwrap().iter().all(|a| !a.contains("evil")),
            "no byte of the screen asked"
        );
        for k in 1..6 {
            let _ = turn(
                &mut pr,
                Duration::from_secs(k * 2),
                &mut paths,
                &mut lines,
                &mut uris,
            );
        }
        assert_eq!(asked.lock().unwrap().len(), 3, "not asked again unowed");
        pr.owe_programs();
        while asked.lock().unwrap().len() < 6 {
            let _ = turn(
                &mut pr,
                Duration::from_secs(14),
                &mut paths,
                &mut lines,
                &mut uris,
            );
            std::thread::yield_now();
        }
        assert_eq!(asked.lock().unwrap()[3..], fixed, "owed: asked once more");
        lines.forget();
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        assert!(!lines.struck().is_empty(), "https, no app: struck");
        assert!(lines.lines().is_empty(), "and none underlined");
    }

    /// **A command's program is looked up along `PATH` by the walk**: found
    /// in a later folder, missing only when every folder said gone, unknown
    /// for a relative folder, no `PATH`, a relative name, an empty one or a
    /// folder under a refused mount; an absolute name is its own walk.
    /// Perturbations: answer missing on a relative folder; skip the mount
    /// table for a command.
    #[test]
    fn a_commands_program_is_looked_up_along_path() {
        let mut f = Fake::with(&[
            ("/usr", Entry::Dir),
            ("/usr/bin", Entry::Dir),
            ("/usr/bin/vi", Entry::File),
            ("/opt", Entry::Dir),
        ]);
        let look = |f: &mut Fake, prog: &str, path: Option<&str>| {
            let m = f.clone().mounts().unwrap();
            command(
                f,
                &m,
                (prog, path.map(std::ffi::OsStr::new)),
                &mut 0,
                &|| {},
            )
        };
        assert_eq!(look(&mut f, "vi", Some("/opt:/usr/bin")), Handler::Found);
        assert_eq!(
            look(&mut f, "nano", Some("/opt:/usr/bin")),
            Handler::Missing
        );
        assert_eq!(look(&mut f, "nano", Some("bin:/usr/bin")), Handler::Unknown);
        assert_eq!(look(&mut f, "nano", None), Handler::Unknown, "no PATH");
        assert_eq!(look(&mut f, "./vi", Some("/usr/bin")), Handler::Unknown);
        assert_eq!(look(&mut f, "", Some("/usr/bin")), Handler::Unknown);
        assert_eq!(look(&mut f, "/usr/bin/vi", None), Handler::Found);
        assert_eq!(look(&mut f, "/usr/bin/nano", None), Handler::Missing);
        f.mounts = vec![b"/opt".to_vec()];
        assert_eq!(
            look(&mut f, "nano", Some("/opt:/usr/bin")),
            Handler::Unknown,
            "a refused mount on the way: can't tell"
        );
        // Asked of the worker with the rest: the mount table read once.
        let mut f = Fake::with(&[("/usr", Entry::Dir), ("/usr/bin", Entry::Dir)]);
        let mut calls = 0;
        let got = programs(
            &mut f,
            vec![
                Program::Command("/usr/bin/nano".to_owned()),
                Program::Command("/usr/bin/ed".to_owned()),
            ],
            &mut calls,
            &|| {},
        );
        assert!(got.iter().all(|(_, h)| *h == Handler::Missing), "{got:?}");
        assert_eq!(calls, 1 + 3 + 3, "one mount table, three lookups each");
    }

    /// **`paths = "off"` starts no thread and asks nothing.** Perturbation:
    /// start the worker in `new`.
    #[test]
    fn paths_off_starts_no_thread() {
        let (mut pr, starts) = probes(Fake::default());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let t0 = Instant::now();
        for k in 0..5u64 {
            let _ = lines.scan(
                &t,
                Scrolled::START,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
            let seen = Seen {
                term: &t,
                scrolled: Scrolled::START,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(t0 + Duration::from_secs(k * 2), seen, || Some(1));
        }
        assert_eq!(*starts.lock().unwrap(), 0);
        assert_eq!(pr.started().value(), 0);
        assert_eq!(pr.counts, Counts::default());
    }

    /// **`[link.path] action = "off"` is `[link] paths = "off"`**: the config
    /// reads it as paths off, and a window set from it reads, asks and
    /// starts nothing, as `paths_off_starts_no_thread` holds for the key.
    /// Perturbation: leave `paths_read` on for the rule -- the config
    /// assertion fails, and the worker starts.
    #[test]
    fn off_on_the_path_rule_starts_no_worker() {
        let mut c = crate::config::Config::defaults();
        assert!(c.apply("[link.path]\naction = \"off\"\n").is_empty());
        assert_eq!(c.link.paths_read(), Turned::Off);
        assert_eq!(
            crate::config::Config::defaults().link.paths_read(),
            Turned::On,
            "the control"
        );
        let (mut pr, starts) = probes(Fake::default());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, c.link.rules.forms());
        paths.set_rules(c.link.rules.clone());
        paths.set(c.link.paths_read());
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let t0 = Instant::now();
        for k in 0..5u64 {
            let _ = lines.scan(
                &t,
                Scrolled::START,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
            let seen = Seen {
                term: &t,
                scrolled: Scrolled::START,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(t0 + Duration::from_secs(k * 2), seen, || Some(1));
        }
        assert_eq!(*starts.lock().unwrap(), 0);
        assert_eq!(pr.started().value(), 0);
    }

    /// **A one-shot ssh's rows keep no mark, and its paths are not looked
    /// up, though the shell is in front when the batch goes**: ssh's group
    /// is read at the read that printed `/etc/hosts`, and named before the
    /// batch; the batch, with the shell in front, walks nothing printed to
    /// that line, and the row draws no line. With the shell's group at the
    /// read instead, the same path is looked up and drawn, the control.
    /// Perturbations: send the batch's `seen` empty in `turn`; drop the
    /// `elsewhere` test in `answer`; drop `written_under` from `turn`.
    #[test]
    fn a_one_shot_ssh_keeps_its_rows_unmarked() {
        let run = |at_read: Pid| {
            let mut f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
            f.groups.insert(9, vec!["ssh".into()]);
            f.groups.insert(1, vec!["zsh".into()]);
            let log = Arc::clone(&f.log);
            let (mut pr, _) = probes(f);
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
            t.feed(BytePos::ZERO, ReadLen::from_index(12));
            let mut paths = Paths::new(None, Forms::DEFAULT);
            paths.set(Turned::On);
            paths.all_written_here();
            // Lit by the shell's verdict, as its own reads light them (G1).
            let all = AbsLine::at(Rows::of_count(usize::MAX));
            pr.lit
                .set(Some(1), LineSpan::of(AbsLine::at(Rows::ZERO), all));
            let (mut lines, mut uris) = (Lines::default(), Uris::default());
            let s = Scrolled::START;
            let printed = s.line(Rows::ZERO, baud_domain::Row::ZERO);
            let span = crate::paths::LineSpan::of(printed, printed);
            pr.read_taken(&mut paths, Writers::at(Some(at_read)), span, Linked::No);
            let t0 = Instant::now();
            let mut turn = |pr: &mut Probes<Instant>, now: Instant| {
                let _ = lines.scan(
                    &t,
                    s,
                    &mut Links {
                        uris: &mut uris,
                        urls: Turned::On,
                        paths: &mut paths,
                    },
                );
                let seen = Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                };
                // The shell is in front by the batch.
                pr.turn(now, seen, || Some(1))
            };
            let _ = turn(&mut pr, t0);
            let _ = turn(&mut pr, t0 + Duration::from_millis(200));
            while turn(&mut pr, t0 + Duration::from_millis(300)).frame != Owed::Yes {
                std::thread::yield_now();
            }
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
            let walked = log.lock().unwrap().iter().any(|p| p == b"/etc/hosts");
            (walked, lines.lines().len())
        };
        assert_eq!(run(1), (true, 1), "the control: the shell's output");
        assert_eq!(run(9), (false, 0), "ssh's: not looked up, not marked");
    }

    /// **The worker names the turn it next owes** (FS-A14), which the window
    /// schedules a wake for: nothing owed, none; a path met and not yet due,
    /// the batch's deadline; a batch out, its hang; the answer in, none.
    /// Perturbation: no hang in `deadline` -- the third fails.
    #[test]
    fn the_worker_names_its_next_turn() {
        let f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.all_written_here();
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        assert_eq!(pr.deadline(&paths, t0), None, "nothing owed");
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let mut turn = |pr: &mut Probes<Instant>, paths: &mut Paths, lines: &mut Lines, at| {
            pr.turn(
                at,
                Seen {
                    term: &t,
                    scrolled: s,
                    lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths,
                },
                || Some(1),
            )
        };
        let _ = turn(&mut pr, &mut paths, &mut lines, t0);
        assert_eq!(
            pr.deadline(&paths, t0),
            Some(t0 + SETTLE),
            "the batch's deadline"
        );
        let sent = t0 + Duration::from_millis(200);
        let _ = turn(&mut pr, &mut paths, &mut lines, sent);
        assert_eq!(
            pr.deadline(&paths, sent),
            Some(sent + HANG),
            "a batch out: its hang"
        );
        while turn(&mut pr, &mut paths, &mut lines, sent).frame != Owed::Yes {
            std::thread::yield_now();
        }
        assert_eq!(pr.deadline(&paths, sent), None, "answered, nothing owed");
    }

    /// **A closed worker names no turn**, whatever is owed: its turn would
    /// do nothing, and a deadline of `now` that no turn clears would be a
    /// wake on every turn. The control: the programs owed to a worker not
    /// yet started, at once. Perturbation: no closed guard in `deadline` --
    /// the second fails.
    #[test]
    fn a_closed_worker_names_no_turn() {
        let (mut pr, _) = probes(Fake::default());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let t0 = Instant::now();
        pr.set_programs(vec![Program::Command("/usr/bin/ed".to_owned())]);
        assert_eq!(pr.deadline(&paths, t0), Some(t0), "the control: owed");
        pr.slot = Slot::Closed;
        assert_eq!(pr.deadline(&paths, t0), None, "closed");
    }

    /// **A batch whose scan draws the rows as they were owes no frame**: a
    /// row of plain words changed, which owes a batch; at its deadline the
    /// batch's scan reads the rows again, finds no line, and asks nothing,
    /// and no frame is owed for lines that did not change. Perturbation: a
    /// frame owed by every batch -- the last assertion fails.
    #[test]
    fn a_batch_over_rows_drawn_as_they_were_owes_no_frame() {
        let (mut pr, _) = probes(Fake::with(&[]));
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"plain words\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(13));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.all_written_here();
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let mut turn = |pr: &mut Probes<Instant>, paths: &mut Paths, at| {
            pr.turn(
                at,
                Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths,
                },
                || Some(1),
            )
        };
        let _ = turn(&mut pr, &mut paths, t0);
        assert_eq!(
            pr.deadline(&paths, t0),
            Some(t0 + SETTLE),
            "the control: a batch owed"
        );
        let at = t0 + Duration::from_millis(200);
        let still = turn(&mut pr, &mut paths, at);
        assert_eq!(pr.deadline(&paths, at), None, "the batch's turn came");
        assert_eq!(still.frame, Owed::No, "no line changed, no frame");
    }

    /// **A result arrives with no cell changed, and the lines are drawn
    /// again once for it, no row read**: the batch's answer redraws the path
    /// the collection kept, and a turn with no answer owes nothing.
    /// Perturbation: merge the answer without telling the lines
    /// (`merge_into` as `merge`) -- the path is never drawn.
    #[test]
    fn an_answer_redraws_the_lines_once() {
        let f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.all_written_here();
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let seen = Seen {
            term: &t,
            scrolled: s,
            lines: &mut lines,
            uris: &mut uris,
            urls: Turned::On,
            paths: &mut paths,
        };
        assert_eq!(
            pr.turn(t0, seen, || Some(1)).frame,
            Owed::No,
            "met, not yet due"
        );
        let seen = Seen {
            term: &t,
            scrolled: s,
            lines: &mut lines,
            uris: &mut uris,
            urls: Turned::On,
            paths: &mut paths,
        };
        let batches = pr.counts.batches;
        let _ = pr.turn(t0 + Duration::from_millis(200), seen, || Some(1));
        assert_eq!(pr.counts.batches, batches + 1, "the batch went");
        let r = loop {
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let r = pr.turn(t0 + Duration::from_millis(300), seen, || Some(1));
            if r.frame == Owed::Yes {
                break r;
            }
            std::thread::yield_now();
        };
        assert_eq!(r.frame, Owed::Yes);
        let scan = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        assert_eq!((scan.rows, scan.remarks), (0, 1), "redrawn, no row read");
        assert_eq!(lines.lines().len(), 1, "and drawn");
        let seen = Seen {
            term: &t,
            scrolled: s,
            lines: &mut lines,
            uris: &mut uris,
            urls: Turned::On,
            paths: &mut paths,
        };
        assert_eq!(
            pr.turn(t0 + Duration::from_millis(400), seen, || Some(1))
                .frame,
            Owed::No,
            "nothing more"
        );
        assert_eq!(pr.counts.asked, 1);
        assert!(
            pr.counts.calls >= 3,
            "the program, the table and the walk: {}",
            pr.counts.calls
        );
    }

    /// **A program named local owes a frame, and its rows are read again**:
    /// a row nothing was filed over reads as not known, so a naming moves
    /// what a link on it looks like, and may join two rows the writer ended
    /// or part them (`PathsAt::joins`); the line at rest and the look under
    /// a still pointer are read again only on a frame. The shell's group,
    /// read at a line holding a link with printed paths off, named local: no
    /// answer and no front moves, the rows are read again, and the turn that
    /// brings the naming owes the frame. Named break: file a writer named
    /// after its read without moving the generation -- the rows are not read
    /// again.
    #[test]
    fn a_program_named_owes_a_frame() {
        let mut f = Fake::with(&[]);
        f.groups.insert(1, vec!["zsh".into()]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"plain\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(7));
        // Paths off, the read linked: its group is named for a link's
        // origin alone, nothing looked up.
        let mut paths = Paths::new(None, Forms::DEFAULT);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let printed = s.line(Rows::ZERO, baud_domain::Row::ZERO);
        pr.read_taken(
            &mut paths,
            Writers::at(Some(1)),
            crate::paths::LineSpan::of(printed, printed),
            Linked::Yes,
        );
        assert_eq!(
            paths.origin(printed),
            crate::paths::Origin::Unknown,
            "the control"
        );
        let t0 = Instant::now();
        let generation = paths.generation();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut step = 0;
        let frame = loop {
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            // Met at the first, the batch sent at the second, then waited for.
            let r = pr.turn(t0 + Duration::from_millis(step), seen, || Some(1));
            step = match step {
                0 => 200,
                _ => 300,
            };
            if paths.origin(printed) == crate::paths::Origin::Local {
                break r.frame;
            }
            assert!(Instant::now() < deadline, "never named");
            std::thread::yield_now();
        };
        assert_ne!(
            paths.generation(),
            generation,
            "the naming moves what the row reads"
        );
        assert_eq!(frame, Owed::Yes);
    }

    /// **A README on a full-screen program's screen, as `hx README.md`
    /// leaves it**: the program's group seen at its read, and `text` on the
    /// alternate screen (or the normal one, `alt` off), with relative paths
    /// read. The turns run over 4 s of the test's clock, each batch's answer
    /// waited for by the worker's wake, so nothing races; `front` is the
    /// group the terminal names in front at each 100 ms step. Returns the
    /// lines drawn and the worker's calls.
    fn on_screen(
        fs: Fake,
        alt: Screen,
        text: &[u8],
        front: impl Fn(u32) -> Pid,
    ) -> (usize, Vec<String>, Option<Vec<u8>>) {
        on_screen_with(fs, alt, text, &front, &front, |_, _| Moved::No, None)
    }

    /// [`on_screen`] with the group a read samples (`read`) apart from the
    /// one a batch reads (`batch`), `step_on` run on the terminal before
    /// each step's turn -- what it prints read as the group sampled then
    /// wrote it, as the reader files a read -- and at the end, with `last` given, one more read
    /// sampling that group and the worker's answers merged, with no batch.
    fn on_screen_with(
        fs: Fake,
        alt: Screen,
        text: &[u8],
        batch: &dyn Fn(u32) -> Pid,
        read: &dyn Fn(u32) -> Pid,
        step_on: impl Fn(u32, &mut Term<'_>) -> Moved,
        last: Option<Pid>,
    ) -> (usize, Vec<String>, Option<Vec<u8>>) {
        let log = fs.clone();
        let (woke, wakes) = std::sync::mpsc::channel();
        let mut fs = Some(fs);
        let mut pr: Probes<Instant> = Probes::new(Box::new(move || {
            let woke = woke.clone();
            Worker::start(
                Box::new(fs.take().expect("started once")),
                Box::new(move || {
                    let _ = woke.send(());
                }),
            )
        }));
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut bytes = match alt {
            Screen::Alternate => b"\x1b[?1049h".to_vec(),
            Screen::Normal => Vec::new(),
        };
        bytes.extend_from_slice(text);
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let mut paths = Paths::new(
            None,
            Forms {
                bare: Turned::On,
                ..Forms::DEFAULT
            },
        );
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let line = s.line(Rows::ZERO, t.cursor().0);
        // The program's first read wrote the screen from its top.
        let top = s.line(Rows::ZERO, baud_domain::Row::ZERO);
        let span = LineSpan::on(paths.surface(t.screen()), top, line);
        pr.read_taken(&mut paths, Writers::at(Some(read(0))), span, Linked::No);
        let t0 = Instant::now();
        for step in 0..40 {
            let now = t0 + Duration::from_millis(100 * u64::from(step));
            // Another group in front writes: a read of its, on the last row.
            if step > 0 && read(step) != read(step - 1) {
                let b = b"\x1b[4;1Hx";
                t.write_at(BytePos::ZERO, b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
                let line = s.line(Rows::ZERO, t.cursor().0);
                let span = LineSpan::on(paths.surface(t.screen()), line, line);
                pr.read_taken(&mut paths, Writers::at(Some(read(step))), span, Linked::No);
            }
            if step_on(step, &mut t) == Moved::Yes {
                let line = s.line(Rows::ZERO, t.cursor().0);
                let top = s.line(Rows::ZERO, baud_domain::Row::ZERO);
                let span = LineSpan::on(paths.surface(t.screen()), top, line);
                pr.read_taken(&mut paths, Writers::at(Some(read(step))), span, Linked::No);
            }
            let before = pr.counts.batches;
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(now, seen, || Some(batch(step)));
            let _ = paths.observe(&t, s);
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
            for _ in before..pr.counts.batches {
                wakes
                    .recv_timeout(Duration::from_secs(60))
                    .expect("the worker answers every batch");
            }
        }
        if let Some(g) = last {
            let line = s.line(Rows::ZERO, t.cursor().0);
            pr.read_taken(
                &mut paths,
                Writers::at(Some(g)),
                LineSpan::of(line, line),
                Linked::No,
            );
            let mut seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.answers(&mut seen);
            let _ = paths.observe(&t, s);
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
        }
        // What a hover over the path on the first row reads, as a click does.
        // The first row as its cells hold it, as the driver reads it.
        let row = crate::programs::screen(&t).swap_remove(0);
        let units: Vec<crate::link::Unit> = row.chars().map(crate::link::Unit::Char).collect();
        let at = row[..row.find("docs/").expect("the path")].chars().count();
        let first = s.line(Rows::ZERO, baud_domain::Row::from_index(0));
        let edges = (crate::link::Edge::Line, crate::link::Edge::Line);
        let hovered = crate::paths::PathsAt::at(
            &paths,
            &units,
            crate::link::Ix(at),
            edges,
            (first, first),
            alt,
        )
        .map(|p| p.path);
        (lines.lines().len(), log.logged(), hovered)
    }

    /// The README's tree, under `/work/hx`, and an empty folder beside it.
    fn readme_tree() -> Fake {
        let mut f = Fake::with(&[
            ("/work", Entry::Dir),
            ("/work/hx", Entry::Dir),
            ("/work/hx/docs", Entry::Dir),
            ("/work/hx/docs/guide", Entry::Dir),
            ("/work/hx/docs/guide/features.md", Entry::File),
            ("/work/empty", Entry::Dir),
        ]);
        f.groups.insert(7, vec!["hx".into()]);
        f.folders.insert(7, b"/work/hx".to_vec());
        f
    }

    const README: &[u8] = b"see [guide](docs/guide/features.md)\r\n";
    const FEATURES: &str = "/work/hx/docs/guide/features.md";

    /// **A relative path on a full-screen program's screen is read against
    /// the folder the program in front is in**: helix's README,
    /// its link `docs/guide/features.md`, gets a line once the worker has
    /// read helix's folder and found the file there, with no byte fed
    /// between, and a hover or a click over it reads it there; the folder
    /// is read on the worker's thread, in its answer.
    /// Named breaks: answer no folder -- no line; take the program's folder
    /// on the normal screen too, at each of its three refusals -- the
    /// control below draws one.
    #[test]
    fn a_relative_path_on_the_alternate_screen_reads_against_the_programs_folder() {
        let (drawn, log, hovered) = on_screen(readme_tree(), Screen::Alternate, README, |_| 7);
        assert!(log.iter().any(|l| l == "folder 7"), "{log:?}");
        assert!(log.iter().any(|l| l == FEATURES), "looked up: {log:?}");
        assert_eq!(drawn, 1, "a line, with no byte fed: {log:?}");
        assert_eq!(
            hovered.as_deref(),
            Some(FEATURES.as_bytes()),
            "a hover and a click read it there"
        );
        // As helix draws it, with its own underline: that line is the
        // program's and baud draws none over it (`lined.rs`), and the path
        // is found all the same, for a hover and a click.
        let helix = b"see [guide](\x1b[4mdocs/guide/features.md\x1b[24m)\r\n";
        let (drawn, log, hovered) = on_screen(readme_tree(), Screen::Alternate, helix, |_| 7);
        assert_eq!(drawn, 0, "helix's own line: {log:?}");
        assert_eq!(hovered.as_deref(), Some(FEATURES.as_bytes()));
        // The normal screen reads a relative path against the shell's
        // reported folder alone, and none was reported.
        let (drawn, log, _) = on_screen(readme_tree(), Screen::Normal, README, |_| 7);
        assert_eq!(drawn, 0, "{log:?}");
        assert!(!log.iter().any(|l| l == FEATURES), "{log:?}");
    }

    /// **A wrong folder draws no line, and nothing is read against a
    /// folder that is not this machine's or cannot be read**: the folder
    /// empty -- the path is looked up there and found gone; no folder read;
    /// a multiplexer in front, whose pane's folder baud cannot see; `ssh`
    /// in front. Named breaks: draw a path the walk found gone; read a
    /// folder whatever is in front -- the multiplexer's is looked under.
    #[test]
    fn a_relative_path_gets_no_line_from_a_wrong_or_unknown_folder() {
        let mut empty = readme_tree();
        empty.folders.insert(7, b"/work/empty".to_vec());
        let (drawn, log, _) = on_screen(empty, Screen::Alternate, README, |_| 7);
        assert!(
            log.iter().any(|l| l == "/work/empty/docs"),
            "looked up: {log:?}"
        );
        assert_eq!(drawn, 0, "gone, so no line");
        let mut unread = readme_tree();
        unread.folders.clear();
        let (drawn, log, _) = on_screen(unread, Screen::Alternate, README, |_| 7);
        assert_eq!(drawn, 0, "{log:?}");
        for (front, names) in [("a multiplexer", "tmux"), ("ssh", "ssh")] {
            let mut f = readme_tree();
            f.groups.insert(7, vec![names.into()]);
            let (drawn, log, _) = on_screen(f, Screen::Alternate, README, |_| 7);
            assert_eq!(drawn, 0, "{front}: {log:?}");
            assert!(
                !log.iter().any(|l| l.starts_with("folder")),
                "{front}: no folder read: {log:?}"
            );
        }
    }

    /// **A folder read for one group is never read against for another**:
    /// helix's folder is taken while helix is in front, and once another
    /// group is in front at a batch it goes, and its line with it, before
    /// any answer for the new group. Named break: keep the folder whoever
    /// is in front -- the line stays under a program whose folder no one
    /// read.
    #[test]
    fn a_programs_folder_goes_when_another_group_comes_in_front() {
        let (drawn, _, _) = on_screen(readme_tree(), Screen::Alternate, README, |_| 7);
        assert_eq!(drawn, 1, "the control: helix's");
        // Group 8, a program with no folder to read, in front from 2 s.
        let mut f = readme_tree();
        f.groups.insert(8, vec!["less".into()]);
        let (drawn, log, _) =
            on_screen(
                f,
                Screen::Alternate,
                README,
                |step| {
                    if step < 20 { 7 } else { 8 }
                },
            );
        assert!(log.iter().any(|l| l == "folder 8"), "{log:?}");
        assert_eq!(drawn, 0, "{log:?}");
    }

    /// **The system's folder read is this machine's own answer**: this
    /// process's folder, as the kernel resolves it (`fg::dir`), and none for
    /// a pid no process has. And it is made only in the worker's answer, on
    /// its thread: the one call outside the tests is in `answer`, so no
    /// frame, tick or callback reads a folder. Named breaks: answer the
    /// folder as the bytes of something else -- the first assertion; call
    /// it anywhere else, even on the worker -- the count.
    #[test]
    fn a_programs_folder_is_read_by_the_system_on_the_worker_only() {
        use std::os::unix::ffi::OsStrExt;
        let me = Pid::try_from(std::process::id()).expect("a pid");
        let here = std::env::current_dir()
            .and_then(std::fs::canonicalize)
            .expect("this folder");
        assert_eq!(
            System.folder(me).as_deref(),
            Some(here.as_os_str().as_bytes())
        );
        assert_eq!(System.folder(Pid::MAX), None, "no such process");
        let src = include_str!("probe.rs");
        let code = &src[..src.find("#[cfg(test)]\nmod tests").expect("the tests")];
        let calls: Vec<usize> = code
            .match_indices(concat!("fs.", "folder("))
            .map(|(i, _)| i)
            .collect();
        let answer = code.find("\nfn answer(").expect("answer");
        let after = answer + code[answer..].find("\n}\n").expect("its end");
        assert_eq!(calls.len(), 1, "one call");
        assert!(calls.iter().all(|&c| answer < c && c < after), "in answer");
    }

    /// **No path reaches a log line**: the worker's and the drivers' log
    /// calls name counts only. Perturbation: log a candidate.
    #[test]
    fn no_path_is_logged() {
        for (name, src) in [
            ("probe.rs", include_str!("probe.rs")),
            ("paths.rs", include_str!("paths.rs")),
        ] {
            for line in src.lines().filter(|l| {
                l.contains(concat!("crate::", "warn!")) || l.contains(concat!("crate::", "info!"))
            }) {
                assert!(
                    !line.contains("{p") && !line.contains("path:?") && !line.contains("{shown"),
                    "{name}: {line}"
                );
            }
        }
    }

    /// **A read carrying an `OSC 8` introducer asks for the group even
    /// inside the pace**, its 8-bit form too and one split across reads;
    /// a read without one, inside the pace and with a group taken before
    /// it, does not. Named breaks: ask by
    /// the pace alone -- the link's read inside the pace asks nothing; drop
    /// the tail -- a split introducer asks nothing.
    #[test]
    fn a_links_read_always_asks_who_wrote_it() {
        let t0 = Instant::now();
        let soon = t0 + Duration::from_millis(1);
        let fresh = || {
            let mut s = Sampler::default();
            assert_eq!(
                s.read(t0, b"$ "),
                Asks::Yes(Linked::No),
                "the pace's first read"
            );
            // Its group taken, as the drivers take it after every ask.
            let _ = s.writers(Some(1), Watched::Nothing);
            s
        };
        let mut s = fresh();
        assert_eq!(
            s.read(soon, b"plain text\r\n"),
            Asks::No,
            "no link, inside the pace"
        );
        let mut s = fresh();
        let link = b"\x1b]8;;file:///etc/hosts\x1b\\hosts\x1b]8;;\x1b\\";
        assert_eq!(s.read(soon, link), Asks::Yes(Linked::Yes), "a link");
        let mut s = fresh();
        assert_eq!(
            s.read(soon, b"\x9d8;;file:///x\x9c"),
            Asks::Yes(Linked::Yes),
            "C1"
        );
        // Only the opening introducer, cut inside it: the second read holds
        // no `ESC` at all.
        let open = b"\x1b]8;;file:///etc/hosts\x07hosts";
        for cut in 1..4 {
            let mut s = fresh();
            let (a, b) = open.split_at(cut);
            let _ = s.read(soon, a);
            assert_eq!(s.read(soon, b), Asks::Yes(Linked::Yes), "split after {cut}");
        }
        let mut s = fresh();
        assert_eq!(
            s.read(soon, b"\x1b]7;file://h/w\x07\x1b]8"),
            Asks::No,
            "OSC 7, no 8;"
        );
    }

    /// **With `[link] paths = "off"`, a read carrying a link is named all
    /// the same, and one without is not**: the link's lines take the origin
    /// of the program that wrote them, `ssh` here, through the worker as a
    /// printed path's would; a read with no link starts nothing. Named
    /// break: name the group only when paths are on -- the link's line stays
    /// unknown.
    #[test]
    fn with_paths_off_a_links_read_is_named() {
        let run = |linked: Linked| {
            let mut f = Fake::with(&[]);
            f.groups.insert(9, vec!["ssh".into()]);
            let (mut pr, starts) = probes(f);
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            t.write_at(BytePos::ZERO, b"x\r\n");
            t.feed(BytePos::ZERO, ReadLen::from_index(3));
            let mut paths = Paths::new(None, Forms::DEFAULT);
            assert_eq!(paths.marks(), Marks::Off);
            let (mut lines, mut uris) = (Lines::default(), Uris::default());
            let s = Scrolled::START;
            let line = s.line(Rows::ZERO, baud_domain::Row::ZERO);
            pr.read_taken(
                &mut paths,
                Writers::at(Some(9)),
                LineSpan::of(line, line),
                linked,
            );
            let t0 = Instant::now();
            for i in 0..1_000_000u32 {
                let seen = Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                };
                let _ = pr.turn(t0 + Duration::from_millis(u64::from(i)), seen, || Some(9));
                if paths.origin(line) != Origin::Unknown || *starts.lock().unwrap() == 0 {
                    break;
                }
                std::thread::yield_now();
            }
            let started = *starts.lock().unwrap();
            (started, paths.origin(line))
        };
        assert_eq!(
            run(Linked::Yes),
            (1, Origin::Remote),
            "a link's read, named ssh's"
        );
        assert_eq!(
            run(Linked::No),
            (0, Origin::Unknown),
            "no link: nothing started"
        );
    }

    /// The origin a link's line takes from a read taken with `writers`,
    /// with `[link] paths = "off"`: ssh is group 9, zsh group 1.
    fn origin_of(writers: Writers) -> Origin {
        let mut f = Fake::with(&[]);
        f.groups.insert(9, vec!["ssh".into()]);
        f.groups.insert(1, vec!["zsh".into()]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"x\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(3));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let line = s.line(Rows::ZERO, baud_domain::Row::ZERO);
        pr.read_taken(&mut paths, writers, LineSpan::of(line, line), Linked::Yes);
        let t0 = Instant::now();
        for i in 0..1_000_000u32 {
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(t0 + Duration::from_millis(u64::from(i)), seen, || Some(1));
            if paths.origin(line) != Origin::Unknown {
                break;
            }
            std::thread::yield_now();
        }
        paths.origin(line)
    }

    /// **A link's read is local only if the sample before it and the one
    /// at it both are**: ssh at the sample before the read and the shell
    /// at the one after -- a short ssh that wrote and ended in between --
    /// makes the line remote, and so does ssh at the read; the shell at
    /// both, the control, is local. Named break: take the group at the read
    /// alone in `read_taken` -- the first case reads local.
    #[test]
    fn a_read_is_remote_if_the_sample_before_it_is() {
        let w = |before, at| Writers {
            before: Some(before),
            at: Some(at),
            ..Writers::default()
        };
        assert_eq!(origin_of(w(9, 1)), Origin::Remote, "ssh before, zsh at");
        assert_eq!(origin_of(w(1, 9)), Origin::Remote, "zsh before, ssh at");
        assert_eq!(
            origin_of(w(1, 1)),
            Origin::Local,
            "the control: zsh at both"
        );
        let gone = Writers {
            before: Some(9),
            at: None,
            ..Writers::default()
        };
        assert_eq!(
            origin_of(gone),
            Origin::Remote,
            "ssh before, and no group the kernel names at the read"
        );
    }

    /// **A program the process watch named not local between two local
    /// samples makes the read remote**: `ssh host 'printf <link>'` started,
    /// wrote and ended between two reads that each found the shell in
    /// front, and only its exec, named by the watch, says it ran. Through
    /// the reader's half too: the verdict taken from the cell between the
    /// two samples reaches the link's read, though a read the pace asked
    /// for took it first. Named breaks: ignore the watch in `read_taken` --
    /// the line reads local; keep no verdict for the next read that asks
    /// (`Sampler::writers`) -- the link's read carries none.
    #[test]
    fn a_job_the_watch_named_between_two_local_samples_is_remote() {
        let cell = WatchCell::default();
        let mut s = Sampler::default();
        let first = s.writers(Some(1), cell.take());
        assert_eq!(origin_of(first), Origin::Local, "the control: the shell");
        // The watch names ssh's exec, and a read the pace asked for takes
        // the cell before the link's read comes.
        cell.raise();
        let paced = s.writers(Some(1), cell.take());
        assert_eq!(paced.watched, Watched::Elsewhere);
        assert_eq!(cell.take(), Watched::Nothing, "the cell is taken");
        let link = s.writers(Some(1), cell.take());
        assert_eq!(
            (link.before, link.at, link.watched),
            (Some(1), Some(1), Watched::Elsewhere),
            "the shell at both samples, the watch's verdict carried"
        );
        assert_eq!(origin_of(link), Origin::Remote, "the link's read");
        let after = s.writers(Some(1), cell.take());
        assert_eq!(
            after.watched,
            Watched::Nothing,
            "carried once, not for ever"
        );
        assert_eq!(origin_of(after), Origin::Local, "the read after it");
    }

    /// **The group before a read is the one the last read that asked was
    /// taken with**: none at the first, and a read whose group the kernel
    /// no longer names leaves the last one named as the next read's
    /// before. Named break: keep no previous sample -- `before` is none.
    #[test]
    fn the_sample_before_a_read_is_the_last_one_taken() {
        let mut s = Sampler::default();
        assert_eq!(s.writers(Some(1), Watched::Nothing).before, None);
        assert_eq!(s.writers(Some(9), Watched::Nothing).before, Some(1));
        assert_eq!(s.writers(None, Watched::Nothing).before, Some(9));
        assert_eq!(s.writers(Some(1), Watched::Nothing).before, Some(9));
    }

    /// **The watch's verdict makes a link's line remote and leaves the
    /// printed paths above it marked**: a job the watch named not local --
    /// `git pull`'s ssh, a child of the shell's job -- wrote row 2, and
    /// `/etc/hosts` on row 0, which the shell wrote, is still looked up and
    /// marked, while row 2 takes a remote origin. With the shell alone at
    /// row 2, the control, row 0 is marked too and row 2 is local. Named
    /// break: file the verdict with the lines written past `GROUPS`
    /// (`unnamed`), which the worker names not local and the printed
    /// paths' watermark goes by -- row 0 loses its mark.
    #[test]
    fn the_watchs_verdict_leaves_the_paths_above_it_marked() {
        let run = |watched: Watched| {
            let mut f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
            f.groups.insert(1, vec!["zsh".into()]);
            let log = Arc::clone(&f.log);
            let (mut pr, _) = probes(f);
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let bytes = b"/etc/hosts\r\n\r\nx";
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let mut paths = Paths::new(None, Forms::DEFAULT);
            paths.set(Turned::On);
            paths.all_written_here();
            // Lit by the shell's verdict, as its own reads light them (G1).
            let all = AbsLine::at(Rows::of_count(usize::MAX));
            pr.lit
                .set(Some(1), LineSpan::of(AbsLine::at(Rows::ZERO), all));
            let (mut lines, mut uris) = (Lines::default(), Uris::default());
            let s = Scrolled::START;
            let row = |r: usize| s.line(Rows::ZERO, baud_domain::Row::from_index(r));
            pr.read_taken(
                &mut paths,
                Writers::at(Some(1)),
                LineSpan::of(row(0), row(0)),
                Linked::No,
            );
            let later = Writers {
                before: Some(1),
                at: Some(1),
                watched,
                ..Writers::default()
            };
            pr.read_taken(&mut paths, later, LineSpan::of(row(2), row(2)), Linked::No);
            let t0 = Instant::now();
            let mut turn = |pr: &mut Probes<Instant>, now: Instant| {
                let _ = lines.scan(
                    &t,
                    s,
                    &mut Links {
                        uris: &mut uris,
                        urls: Turned::On,
                        paths: &mut paths,
                    },
                );
                let seen = Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                };
                pr.turn(now, seen, || Some(1))
            };
            let _ = turn(&mut pr, t0);
            let _ = turn(&mut pr, t0 + Duration::from_millis(200));
            while turn(&mut pr, t0 + Duration::from_millis(300)).frame != Owed::Yes {
                std::thread::yield_now();
            }
            let _ = lines.scan(
                &t,
                s,
                &mut Links {
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                },
            );
            let walked = log.lock().unwrap().iter().any(|p| p == b"/etc/hosts");
            (walked, lines.lines().len(), paths.origin(row(2)))
        };
        assert_eq!(
            run(Watched::Nothing),
            (true, 1, Origin::Local),
            "the control: the shell at row 2"
        );
        assert_eq!(
            run(Watched::Elsewhere),
            (true, 1, Origin::Remote),
            "the watch's verdict at row 2"
        );
    }

    /// **A read carrying `file://` as plain text asks for the group even
    /// inside the pace**, in any case and split across two reads at every
    /// cut; `Makefile:12:`, `profile:` and the folder `OSC 7` reports at a
    /// prompt (`ESC ] 7 ; file://...`) ask nothing. Named break: look for
    /// the `OSC 8` introducer alone -- the plain URI's read asks nothing.
    #[test]
    fn a_plain_file_uris_read_always_asks_who_wrote_it() {
        let t0 = Instant::now();
        let soon = t0 + Duration::from_millis(1);
        let fresh = || {
            let mut s = Sampler::default();
            assert_eq!(
                s.read(t0, b"$ "),
                Asks::Yes(Linked::No),
                "the pace's first read"
            );
            // Its group taken, as the drivers take it after every ask.
            let _ = s.writers(Some(1), Watched::Nothing);
            s
        };
        for text in [
            &b"see file:///etc/hosts\r\n"[..],
            b"FILE://localhost/x",
            b"(File://h/x)",
        ] {
            assert_eq!(fresh().read(soon, text), Asks::Yes(Linked::Yes), "{text:?}");
        }
        let uri = b"x file://localhost/etc/hosts";
        for cut in 1..uri.len() {
            let mut s = fresh();
            let (a, b) = uri.split_at(cut);
            let first = s.read(soon, a);
            let second = s.read(soon, b);
            assert!(
                first == Asks::Yes(Linked::Yes) || second == Asks::Yes(Linked::Yes),
                "split after {cut}: {first:?} {second:?}"
            );
        }
        for text in [
            &b"Makefile:12: recipe for target\r\n"[..],
            b"profile: ok",
            b"\x1b]7;file://h/home/x\x07$ ",
            b"\x9d7;file://h/home/x\x9c$ ",
        ] {
            assert_eq!(fresh().read(soon, text), Asks::No, "{text:?}");
        }
    }

    /// **With `[link] paths = "off"`, a `file://` URI printed as plain text
    /// is named as a marked link is, and its click goes by who printed
    /// it**: the shell's is revealed, ssh's is refused. The read goes
    /// through the [`Sampler`] as the reader takes it, then the worker, and
    /// the link is read back from the cells and clicked as both drivers
    /// click it. Named break: look for the `OSC 8` introducer alone in the
    /// [`Sampler`] -- the read is not kept, the line's origin stays
    /// unknown, and the shell's link is refused too.
    #[test]
    fn with_paths_off_a_plain_file_uri_is_named_and_clicked_by_its_origin() {
        use crate::link::{Click, Refused, Spot, Under, hovered};
        let run = |g: Pid| -> Result<Click, Refused> {
            let mut f = Fake::with(&[]);
            f.groups.insert(9, vec!["ssh".into()]);
            f.groups.insert(1, vec!["zsh".into()]);
            let (mut pr, starts) = probes(f);
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            let bytes = b"file://localhost/etc/hosts\r\n";
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let mut paths = Paths::new(None, Forms::DEFAULT);
            assert_eq!(paths.marks(), Marks::Off);
            let (mut lines, mut uris) = (Lines::default(), Uris::default());
            let s = Scrolled::START;
            let line = s.line(Rows::ZERO, baud_domain::Row::ZERO);
            let t0 = Instant::now();
            let mut sampler = Sampler::default();
            let _ = sampler.read(t0, b"$ ");
            let _ = sampler.writers(Some(1), Watched::Nothing);
            let (writers, linked) = match sampler.read(t0 + Duration::from_millis(1), bytes) {
                Asks::Yes(l) => (sampler.writers(Some(g), Watched::Nothing), l),
                Asks::No => (sampler.unasked(), Linked::No),
            };
            pr.read_taken(&mut paths, writers, LineSpan::of(line, line), linked);
            for i in 0..1_000_000u32 {
                let seen = Seen {
                    term: &t,
                    scrolled: s,
                    lines: &mut lines,
                    uris: &mut uris,
                    urls: Turned::On,
                    paths: &mut paths,
                };
                let _ = pr.turn(t0 + Duration::from_millis(u64::from(i)), seen, || Some(1));
                if paths.origin(line) != Origin::Unknown || *starts.lock().unwrap() == 0 {
                    break;
                }
                std::thread::yield_now();
            }
            let spot = Spot {
                scrolled: s,
                back: Rows::ZERO,
                row: baud_domain::Row::ZERO,
                col: baud_domain::Col::at_index(1),
            };
            let under = hovered(&t, spot).expect("a link under the cell");
            let Under::Bare(bare) = &under else {
                panic!("read from the text: {under:?}");
            };
            let uri = bare.uri().expect("its URI").to_vec();
            let _ = uri;
            crate::link::clicked(
                &under,
                &t,
                &crate::link::MachineNames::NONE,
                uris.rules(),
                &paths,
            )
        };
        assert_eq!(run(1), Ok(Click::Reveal), "the shell's");
        assert_eq!(run(9), Err(Refused::Remote), "ssh's");
    }

    /// **A link on row 0 as both drivers click it, after `reads` went
    /// through the reader as it takes them**: each fed to the core, scanned
    /// by the [`Sampler`], and taken with the shell (group 1) in front at
    /// every sample -- a short job that came and went between them -- then
    /// named by the worker.
    fn clicked_after_reads(reads: &[&[u8]]) -> Result<crate::link::Click, crate::link::Refused> {
        clicked_after_reads_turning(reads, Turned::Off)
    }

    /// [`clicked_after_reads`], with paths turned `on` once the link's
    /// answer is filed, as a reload of the config turns them, and one turn
    /// taken after.
    fn clicked_after_reads_turning(
        reads: &[&[u8]],
        on: Turned,
    ) -> Result<crate::link::Click, crate::link::Refused> {
        use crate::link::{Spot, Under, hovered};
        let mut f = Fake::with(&[]);
        f.groups.insert(1, vec!["zsh".into()]);
        let (mut pr, starts) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let t0 = Instant::now();
        let mut sampler = Sampler::default();
        for (k, bytes) in reads.iter().enumerate() {
            let began = s.line(Rows::ZERO, t.cursor().0);
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let ended = s.line(Rows::ZERO, t.cursor().0);
            let now = t0 + Duration::from_millis(u64::try_from(k).expect("a count"));
            let (writers, linked) = match sampler.read(now, bytes) {
                Asks::Yes(l) => (sampler.writers(Some(1), Watched::Nothing), l),
                Asks::No => (sampler.unasked(), Linked::No),
            };
            pr.read_taken(&mut paths, writers, LineSpan::of(began, ended), linked);
        }
        let line = s.line(Rows::ZERO, baud_domain::Row::ZERO);
        for i in 0..1_000_000u32 {
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(t0 + Duration::from_millis(u64::from(i)), seen, || Some(1));
            if paths.origin(line) != Origin::Unknown || *starts.lock().unwrap() == 0 {
                break;
            }
            std::thread::yield_now();
        }
        if on == Turned::On {
            paths.set(Turned::On);
            lines.forget();
            let seen = Seen {
                term: &t,
                scrolled: s,
                lines: &mut lines,
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            };
            let _ = pr.turn(t0, seen, || Some(1));
        }
        let spot = Spot {
            scrolled: s,
            back: Rows::ZERO,
            row: baud_domain::Row::ZERO,
            col: baud_domain::Col::at_index(1),
        };
        let under = hovered(&t, spot).expect("a link under the cell");
        let uri = match &under {
            Under::Osc8(l) => l.uri(&t).expect("its URI").to_vec(),
            Under::Bare(b) => b.uri().expect("its URI").to_vec(),
            Under::Path(_) => panic!("a path, not a link"),
        };
        let _ = uri;
        crate::link::clicked(
            &under,
            &t,
            &crate::link::MachineNames::NONE,
            uris.rules(),
            &paths,
        )
    }

    /// **The shell's mark that its command is `ssh` makes the link that
    /// command printed remote, though the shell was in front at every
    /// sample**: `133;C;cmdline_url=ssh%20localhost`, a `file://localhost`
    /// link, then `133;D`, in one read and in three, is refused; the same
    /// with `ls`, and with no mark at all, is revealed -- the samples alone
    /// cannot see a job that short. Named break: ignore the mark in
    /// `read_taken` -- the ssh's link is revealed.
    #[test]
    fn the_shells_mark_names_a_short_ssh_that_no_sample_saw() {
        use crate::link::{Click, Refused};
        let link: &[u8] = b"\x1b]8;;file://localhost/etc/passwd\x1b\\passwd\x1b]8;;\x1b\\\r\n";
        let c = |line: &str| format!("\x1b]133;C;baud_token=t;cmdline_url={line}\x07").into_bytes();
        let d: &[u8] = b"\x1b]133;D;0;baud_token=t\x07";
        for (cmd, want) in [
            ("ssh%20localhost", Err(Refused::Remote)),
            ("ls%20--hyperlink", Ok(Click::Reveal)),
        ] {
            let mark = c(cmd);
            let one = [mark.as_slice(), link, d].concat();
            assert_eq!(clicked_after_reads(&[&one]), want, "{cmd}, one read");
            assert_eq!(
                clicked_after_reads(&[&mark, link, d]),
                want,
                "{cmd}, three reads"
            );
        }
        assert_eq!(
            clicked_after_reads(&[link]),
            Ok(Click::Reveal),
            "no mark: the shell"
        );
    }

    /// **A line written over while paths were off is not known once they
    /// are on again, though nothing moves it** (W3, F1): `ls` lights
    /// `/etc/passwd` on row 0; paths off; `ssh` writes `/etc/hosts` over
    /// it, opening no link; paths on -- dark. The control: `ls` writes it
    /// with paths on -- lit. Named break: no floor at the turn
    /// (`Probes::floor_off` filing nothing) -- `ls`'s fact stays on the
    /// line and `ssh`'s path lights.
    #[test]
    fn a_line_written_over_while_paths_were_off_is_not_known_after() {
        let run = |over: Read<'static>, off: Turned| {
            lit_driven(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Paths(Turned::On),
                        ..read(b"", 2)
                    },
                    Read {
                        then: Then::Answered,
                        ..read(b"/etc/passwd", 2)
                    },
                    Read {
                        then: Then::Paths(off),
                        ..read(b"\x1b[m", 2)
                    },
                    over,
                    Read {
                        then: Then::Paths(Turned::On),
                        ..read(b"\x1b[m", 2)
                    },
                ],
            )
        };
        assert_eq!(
            run(read(b"\r/etc/hosts\x1b[K", 9), Turned::Off),
            Vec::<usize>::new()
        );
        assert_eq!(
            run(read(b"\r/etc/hosts\x1b[K", 2), Turned::On),
            [0],
            "the control"
        );
    }

    /// **A link printed with paths off keeps its own answer when they come
    /// on** (W3, F1): a read not kept leaves the cursor's line for the
    /// floor, a link's read then writes over it and is kept, and its answer
    /// is filed; paths on, the floor leaves the link's lines out, and its
    /// click is revealed. Named
    /// break: a kept read leaving the floor whole (`Probes::off_kept`
    /// taking nothing out) -- the link's line is floored not known and the
    /// click refused.
    #[test]
    fn a_link_printed_with_paths_off_keeps_its_answer_when_they_come_on() {
        let link: &[u8] = b"\x1b]8;;file://localhost/etc/passwd\x1b\\passwd\x1b]8;;\x1b\\\r\n";
        assert_eq!(
            clicked_after_reads_turning(&[b"\x1b[m", link], Turned::On),
            Ok(crate::link::Click::Reveal)
        );
    }

    /// **A folder goes when a read samples another group, before any batch
    /// says so**: yazi (group 7) in front, its folder read and the README's
    /// line drawn, hands straight to hx (group 8); the read that samples
    /// hx's group drops the folder at the next turn, with no batch sent,
    /// and the line goes. A read sampling yazi's group again, the control,
    /// keeps it. Named break: keep the folder in `read_taken` -- the line
    /// stays until a batch.
    #[test]
    fn a_programs_folder_goes_when_a_read_samples_another_group() {
        let run = |last: Pid| {
            let mut f = readme_tree();
            f.groups.insert(8, vec!["hx".into()]);
            on_screen_with(
                f,
                Screen::Alternate,
                README,
                &|_| 7,
                &|_| 7,
                |_, _| Moved::No,
                Some(last),
            )
        };
        let (drawn, log, _) = run(7);
        assert_eq!(drawn, 1, "the control: the same group: {log:?}");
        let (drawn, log, _) = run(8);
        assert_eq!(drawn, 0, "another group: {log:?}");
    }

    /// **Leaving the alternate screen drops the folder**: the README's line,
    /// drawn while helix is up, is gone once the screen it was read for is,
    /// with helix's group still in front, and a relative path is read
    /// against no folder of the program's after it. Named break: keep the
    /// program's folder on the normal screen (`Paths::observe`) -- the
    /// hover on the README's row still finds the file.
    #[test]
    fn a_programs_folder_goes_with_the_alternate_screen() {
        let leave = |step: u32, t: &mut Term<'_>| {
            if step == 25 {
                // Back to the primary screen, and the README's text written
                // there as plain output, for the read against the folder.
                let b = [b"\x1b[?1049l".as_slice(), README].concat();
                t.write_at(BytePos::ZERO, &b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
                return Moved::Yes;
            }
            Moved::No
        };
        let (drawn, log, hovered) = on_screen_with(
            readme_tree(),
            Screen::Alternate,
            README,
            &|_| 7,
            &|_| 7,
            leave,
            None,
        );
        assert_eq!(drawn, 0, "{log:?}");
        assert_eq!(hovered, None, "no folder to read against: {log:?}");
    }

    /// **A group number another process took is named again, and the
    /// folder read for the one before it goes**: helix, group 7 started at
    /// time one, is named local and its folder read; from 2 s group 7 is
    /// `ssh`, started at time two, and prints a path, and the worker, whose
    /// name for 7 was helix's, sees the start time differ, names it again
    /// as not local and answers no folder, so neither line is drawn. The
    /// control: helix itself, its start the same, prints the path and the
    /// lines stay. A leader gone, whose start no longer reads, leaves no
    /// folder either. Named breaks: key the worker's names by the group
    /// alone -- 7 stays helix's, and the lines stay under ssh; answer the
    /// folder when the two start reads are equal Options -- none and none
    /// agree, and the lines stay with helix gone.
    #[test]
    fn a_reused_group_number_is_named_again_by_its_start_time() {
        let run = |name: &'static str, at: Option<StartTime>| {
            let f = readme_tree();
            let live = Arc::clone(&f.live);
            live.lock()
                .unwrap()
                .insert(7, (vec!["hx".into()], Some(StartTime::AT[1])));
            // At 2 s group 7 is another process, helix still, or no one,
            // and what it prints owes a batch.
            let prints = move |step: u32, t: &mut Term<'_>| {
                if step == 20 {
                    let names = if name.is_empty() {
                        Vec::new()
                    } else {
                        vec![name.into()]
                    };
                    live.lock().unwrap().insert(7, (names, at));
                    let b = b"\x1b[3;1Hdocs/guide/new.md";
                    t.write_at(BytePos::ZERO, b);
                    t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
                    return Moved::Yes;
                }
                Moved::No
            };
            on_screen_with(f, Screen::Alternate, README, &|_| 7, &|_| 7, prints, None)
        };
        let (drawn, log, _) = run("ssh", Some(StartTime::AT[2]));
        assert_eq!(drawn, 0, "another process under 7: {log:?}");
        let (drawn, log, _) = run("hx", Some(StartTime::AT[1]));
        assert!(drawn >= 1, "the control: helix still: {log:?}");
        let (drawn, log, _) = run("", None);
        assert_eq!(drawn, 0, "helix gone, no start to key by: {log:?}");
    }

    /// **A folder is answered only when its leader's start reads**: with no
    /// start time that reads for group 7, its folder is read and not
    /// answered, so the README's line is not drawn; the control, the same
    /// screen with the start reading, draws it. Named break: answer the
    /// folder when the two start reads are equal Options -- none and none
    /// agree, and the line is drawn under a key that tells nothing apart.
    #[test]
    fn a_folder_is_answered_only_when_its_leaders_start_reads() {
        let mut unread = readme_tree();
        unread.starts = Starts::Unread;
        let (drawn, log, _) = on_screen(unread, Screen::Alternate, README, |_| 7);
        assert!(log.iter().any(|l| l == "folder 7"), "read: {log:?}");
        assert_eq!(drawn, 0, "not answered: {log:?}");
        let (drawn, log, _) = on_screen(readme_tree(), Screen::Alternate, README, |_| 7);
        assert_eq!(drawn, 1, "the control: {log:?}");
    }

    /// **Main takes no call of its own for the folder's key**: `answers`,
    /// where a folder is taken or dropped, reads the group and the start
    /// time from the answer and nothing from the system. Named break: read
    /// the group or the start time in `answers`.
    #[test]
    fn the_folders_key_is_compared_with_no_call_on_main() {
        let src = include_str!("probe.rs");
        let from = src.find("    pub fn answers(").expect("answers");
        let to = from + src[from..].find("\n    }\n").expect("its end");
        let body = &src[from..to];
        for call in [
            concat!("fs", "."),
            concat!("fg", "::"),
            concat!("sys", "::"),
            concat!("started", "("),
            concat!("TIOC", "GPGRP"),
        ] {
            assert!(!body.contains(call), "answers calls {call}");
        }
        assert!(
            body.contains(concat!("a.", "started")),
            "the positive control: the key is read"
        );
    }

    /// **What single names cost the worker**, for the record rather than a
    /// gate, with `"name"` read, on this machine's file system through
    /// [`System`]: a 200x50
    /// window after `OSC 7` names a scratch folder of 2,000 entries, its
    /// collections answered by [`answer`] until none is owed, for `ls -l`
    /// and `ls -C` of the folder, a 2,000-line log of dotted identifiers,
    /// and a 10,000-line one fed 1,000 lines before each batch, the
    /// worker's ceiling of one a second, for ten. Prints each batch's paths
    /// and names asked, `lstat`/`readlink` calls and time, and the kept
    /// answers after; asserts every batch keeps [`crate::paths::NAMES_MAX`]
    /// and [`crate::paths::BATCH_MAX`] and the kept answers stay within one
    /// screen. Run with `--run-ignored only --no-capture`.
    #[test]
    #[ignore = "a measurement, printed"]
    fn names_cost_the_worker() {
        use crate::paths::{BATCH_MAX, Form, NAMES_MAX};
        use std::os::unix::ffi::OsStrExt;
        let ws = WinSize {
            ws_col: 200,
            ws_row: 50,
        };
        let dir = std::env::temp_dir().join(format!("baud-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the folder");
        for n in 0..2000 {
            let name = match n % 10 {
                9 => format!("dir{n:04}"),
                k => format!("f{n:04}.{}", ["rs", "md", "toml", "txt", "tar.gz"][k % 5]),
            };
            let p = dir.join(&name);
            match n % 10 {
                9 => std::fs::create_dir(&p).expect("a folder"),
                _ => std::fs::write(&p, b"").expect("a file"),
            }
        }
        let dir = std::fs::canonicalize(&dir).expect("the folder's path");
        let ls = |args: &[&str]| -> Vec<u8> {
            let out = std::process::Command::new("/bin/ls")
                .args(args)
                .current_dir(&dir)
                .env("LC_ALL", "C")
                .env("COLUMNS", "200")
                .output()
                .expect("ls");
            out.stdout
        };
        let log = |lines: core::ops::Range<u32>| -> Vec<u8> {
            lines
                .map(|n| {
                    format!(
                        "2026-11-08T10:{:02}:{:02}.{:03}Z INFO svc.handler.Request id=req.{n} \
                         path=api.v1.items took {}.{}ms\n",
                        n / 3600 % 60,
                        n / 60 % 60,
                        n % 1000,
                        n % 7,
                        n % 10
                    )
                })
                .collect::<String>()
                .into_bytes()
        };
        let crlf = |b: &[u8]| -> Vec<u8> {
            b.iter()
                .flat_map(|&c| match c {
                    b'\n' => vec![b'\r', b'\n'],
                    c => vec![c],
                })
                .collect()
        };
        let feed = |t: &mut Term<'_>, bytes: &[u8]| {
            for c in bytes.chunks(4096) {
                t.write_at(BytePos::ZERO, c);
                t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
            }
        };
        let me = Pid::try_from(std::process::id()).expect("a pid");
        let folder = dir.as_os_str().as_bytes().to_vec();
        let mut fs = System;
        let mut named = HashMap::new();
        /// Batches until none is owed, or one a chunk, as the ceiling sends
        /// while output goes on.
        #[derive(Clone, Copy, PartialEq)]
        enum Batches {
            Settle,
            OneAChunk,
        }
        let run = |label: &str,
                   chunks: Vec<Vec<u8>>,
                   batches: Batches,
                   fs: &mut System,
                   named: &mut HashMap<_, _>| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let mut osc = b"\x1b]7;file://bench.example".to_vec();
            osc.extend_from_slice(&folder);
            osc.push(0x07);
            feed(&mut t, &osc);
            let names = Forms {
                name: Turned::On,
                ..Forms::DEFAULT
            };
            let mut p = Paths::new(None, names);
            p.set(Turned::On);
            p.set_names(crate::link::MachineNames::of(vec![
                "bench.example".to_string(),
            ]));
            let scrolled = Scrolled::START.advanced(Rows::of_count(1 << 40));
            let _ = p.observe(&t, scrolled);
            let (mut l, mut u) = (Lines::default(), Uris::default());
            let mut id = Id::default();
            for (i, chunk) in chunks.iter().enumerate() {
                feed(&mut t, &crlf(chunk));
                let _ = p.observe(&t, scrolled);
                for _ in 0..20 {
                    p.begin_collect();
                    l.forget();
                    let _ = l.scan(
                        &t,
                        scrolled,
                        &mut Links {
                            uris: &mut u,
                            urls: Turned::On,
                            paths: &mut p,
                        },
                    );
                    let queue = p.end_collect();
                    let names = queue
                        .iter()
                        .filter(|c| matches!(c.form, Form::Name | Form::Word))
                        .count();
                    assert!(names <= NAMES_MAX && queue.len() <= BATCH_MAX, "{label}");
                    let asked = queue.len();
                    id = id.next();
                    let t0 = Instant::now();
                    let got = answer(
                        fs,
                        Ask {
                            id,
                            kind: Kind::Screen,
                            group: Some(me),
                            cursor: LineOn {
                                surface: crate::paths::Surface::Primary,
                                line: scrolled.line(Rows::ZERO, t.cursor().0),
                            },
                            paths: queue,
                            seen: Vec::new(),
                            unnamed: None,
                            programs: Vec::new(),
                        },
                        named,
                        &mut Sessions::default(),
                        &|| {},
                        &mut || false,
                    );
                    let took = t0.elapsed();
                    let found = got
                        .probes
                        .iter()
                        .filter(|(_, pr)| *pr == Probe::Exists)
                        .count();
                    let _ = p.merge(got.probes);
                    let (n, bytes) = p.known_size();
                    eprintln!(
                        "{label} [{i}]: asked {asked} (names {names}), found {found}, calls {}, \
                         {:.2} ms; known {n} entries, {bytes} bytes",
                        got.calls,
                        took.as_secs_f64() * 1e3
                    );
                    assert!(n <= 200 * 50 / 2, "{label}: known stays within a screen");
                    if batches == Batches::OneAChunk || p.owed() == Owed::No {
                        break;
                    }
                }
            }
        };
        run(
            "ls -l",
            vec![ls(&["-l"])],
            Batches::Settle,
            &mut fs,
            &mut named,
        );
        run(
            "ls -C",
            vec![ls(&["-C"])],
            Batches::Settle,
            &mut fs,
            &mut named,
        );
        run(
            "log 2000",
            vec![log(0..2000)],
            Batches::Settle,
            &mut fs,
            &mut named,
        );
        let scrolling = (0..10).map(|s| log(s * 1000..(s + 1) * 1000)).collect();
        run(
            "log 10000 / 10 s",
            scrolling,
            Batches::OneAChunk,
            &mut fs,
            &mut named,
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One read for [`lit_reads`]: its bytes, the group the kernel names at
    /// it, the process watch's verdict taken at it, and whether it is taken
    /// with the read before it in one drain, as the macOS pump takes them.
    #[derive(Clone, Copy)]
    struct Read<'a> {
        bytes: &'a [u8],
        group: Pid,
        watched: Watched,
        drain: Drain,
        sampled: Sampled,
        then: Then,
        /// The group in front when the batches its drain's `then` sends
        /// go (the shell, 1, unless a row says otherwise).
        front: Pid,
    }

    /// What the window does once a read's drain is taken: nothing, or a
    /// resize to this size, numbered as the drivers number one
    /// (`keep::kept_across_resize`).
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Then {
        Nothing,
        /// To this many columns and rows.
        Resize(u16, u16),
        /// The worker asked about the screen as it stands and every answer
        /// taken, before the next read: a later read seconds after.
        Answered,
        /// The worker asked about the screen as it stands, its answer not
        /// yet taken when the next read is filed.
        Sent,
        /// The batch still out answered and taken, nothing sent, and the
        /// rows that draw a line then noted ([`lit_midway`]).
        Took,
        /// `[link] paths` turned on or off, as a reload of the config does.
        Paths(Turned),
    }

    /// **How the window's lines are numbered across the reads**: held at
    /// the start, as most rows here read them, or moved after each drain by
    /// what the live surface retired or took back, as the drivers move
    /// theirs (`seat::Seat::after_read`, `keep::numbered`).
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Numbering {
        Held,
        Driven,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Drain {
        New,
        Same,
    }

    /// Whether a read asks for the group in front, or is one inside the
    /// pace, which asks for none (`Sampler::unasked`).
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Sampled {
        Yes,
        No,
        /// Fed and filed under nothing: text on the screen whose writer no
        /// span names, as a read before the paths were on.
        Unfiled,
    }

    /// A read of its own, as the Linux loop takes each, with nothing from
    /// the watch.
    fn read(bytes: &[u8], group: Pid) -> Read<'_> {
        Read {
            bytes,
            group,
            watched: Watched::Nothing,
            drain: Drain::New,
            sampled: Sampled::Yes,
            then: Then::Nothing,
            front: 1,
        }
    }

    /// A read inside the pace: no group asked for.
    fn quiet(bytes: &[u8]) -> Read<'_> {
        Read {
            sampled: Sampled::No,
            ..read(bytes, 0)
        }
    }

    /// **The rows that draw a line once `chunks` are taken and a batch is
    /// answered with the shell (group 1) in front**: [`lit_reads`] with
    /// each read alone, as the Linux loop takes reads.
    fn lit_after(f: Fake, chunks: &[(&[u8], Pid)]) -> Vec<usize> {
        let reads: Vec<Read<'_>> = chunks.iter().map(|&(b, g)| read(b, g)).collect();
        lit_reads(f, &reads)
    }

    /// **The rows that draw a line once `reads` are taken and a batch is
    /// answered with the shell (group 1) in front**: each fed to a terminal
    /// through a [`Sampler`] -- its marks scan included -- with its group
    /// and the watch's verdict, and each drain's reads filed over the lines
    /// each one's record names (`Probes::drain_taken`), as the drivers file
    /// them. `f` names the groups.
    fn lit_reads(f: Fake, reads: &[Read<'_>]) -> Vec<usize> {
        taken(f, reads).0
    }

    /// [`lit_reads`], and the origin of each of the first rows, to the
    /// cursor's.
    fn taken(f: Fake, reads: &[Read<'_>]) -> (Vec<usize>, Vec<Origin>) {
        let (rows, origins, ()) = taken_then(f, reads, None, |_, _| ());
        (rows, origins)
    }

    /// [`taken`], the spans of the lines above `trim` dropped once the reads
    /// are filed and answered, as when they leave history, and then `look`
    /// at the terminal and the paths as they stand.
    fn taken_then<R>(
        f: Fake,
        reads: &[Read<'_>],
        trim: Option<usize>,
        look: impl FnOnce(&Term<'_>, &Paths) -> R,
    ) -> (Vec<usize>, Vec<Origin>, R) {
        taken_then_on(WS, f, reads, trim, look)
    }

    /// [`taken`] on a screen of `ws`.
    fn taken_on(ws: WinSize, f: Fake, reads: &[Read<'_>]) -> (Vec<usize>, Vec<Origin>) {
        let (rows, origins, ()) = taken_then_on(ws, f, reads, None, |_, _| ());
        (rows, origins)
    }

    /// [`taken_then`] on a screen of `ws`.
    fn taken_then_on<R>(
        ws: WinSize,
        f: Fake,
        reads: &[Read<'_>],
        trim: Option<usize>,
        look: impl FnOnce(&Term<'_>, &Paths) -> R,
    ) -> (Vec<usize>, Vec<Origin>, R) {
        taken_numbered(ws, f, reads, trim, Numbering::Held, look)
    }

    /// **[`lit_reads`] on a screen of `ws`, the lines numbered as the
    /// drivers number them** ([`Numbering::Driven`]).
    fn lit_driven(ws: WinSize, f: Fake, reads: &[Read<'_>]) -> Vec<usize> {
        taken_numbered(ws, f, reads, None, Numbering::Driven, |_, _| ()).0
    }

    /// **Batches a second apart from `now`, each answer taken, until the
    /// screen has been asked about as it stands**: a row a later answer
    /// makes readable is walked by the batch after it.
    /// **A batch sent after `now` and its answer left untaken**: the
    /// clock's half of a turn alone (`Probes::tick`), a second apart until
    /// the pace sends one.
    fn sent(
        pr: &mut Probes<Instant>,
        (t, s): (&Term<'_>, Scrolled),
        (lines, uris, paths): (&mut Lines, &mut Uris, &mut Paths),
        mut now: Instant,
        front: Pid,
    ) {
        for _ in 0..4 {
            now += Duration::from_secs(1);
            let _ = lines.scan(
                t,
                s,
                &mut Links {
                    uris: &mut *uris,
                    urls: Turned::On,
                    paths: &mut *paths,
                },
            );
            let seen = Seen {
                term: t,
                scrolled: s,
                lines: &mut *lines,
                uris: &mut *uris,
                urls: Turned::On,
                paths: &mut *paths,
            };
            let _ = pr.tick(now, seen, || Some(front));
            if pr.flight.is_some() {
                return;
            }
        }
        panic!("no batch went");
    }

    /// **The batch still out answered and taken**, with nothing sent
    /// meanwhile.
    fn took_out(
        pr: &mut Probes<Instant>,
        (t, s): (&Term<'_>, Scrolled),
        (lines, uris, paths): (&mut Lines, &mut Uris, &mut Paths),
    ) {
        while pr.flight.is_some() {
            std::thread::yield_now();
            let mut seen = Seen {
                term: t,
                scrolled: s,
                lines: &mut *lines,
                uris: &mut *uris,
                urls: Turned::On,
                paths: &mut *paths,
            };
            let _ = pr.answers(&mut seen);
        }
    }

    fn answered(
        pr: &mut Probes<Instant>,
        (t, s): (&Term<'_>, Scrolled),
        (lines, uris, paths): (&mut Lines, &mut Uris, &mut Paths),
        mut now: Instant,
        front: Pid,
    ) {
        // A batch still out from before (`Then::Sent`) is answered before
        // the clock moves past its hang: the worker's answer is taken as
        // it comes, with nothing sent meanwhile.
        took_out(pr, (t, s), (&mut *lines, &mut *uris, &mut *paths));
        let mut turn = |pr: &mut Probes<Instant>, now: Instant| {
            let _ = lines.scan(
                t,
                s,
                &mut Links {
                    uris: &mut *uris,
                    urls: Turned::On,
                    paths: &mut *paths,
                },
            );
            let seen = Seen {
                term: t,
                scrolled: s,
                lines: &mut *lines,
                uris: &mut *uris,
                urls: Turned::On,
                paths: &mut *paths,
            };
            pr.turn(now, seen, || Some(front))
        };
        for _ in 0..4 {
            now += Duration::from_secs(1);
            let _ = turn(pr, now);
            while pr.flight.is_some() {
                std::thread::yield_now();
                let _ = turn(pr, now);
            }
        }
    }

    /// [`taken_then_on`], the lines numbered by `numbering`, the storage
    /// sized to the largest window any read resizes to.
    fn taken_numbered<R>(
        ws: WinSize,
        f: Fake,
        reads: &[Read<'_>],
        trim: Option<usize>,
        numbering: Numbering,
        look: impl FnOnce(&Term<'_>, &Paths) -> R,
    ) -> (Vec<usize>, Vec<Origin>, R) {
        let t = taken_midway(ws, f, reads, trim, numbering, look);
        (t.lit, t.origins, t.looked)
    }

    /// **The rows that draw a line at each [`Then::Took`], and at the
    /// end**, the lines numbered as the drivers number them.
    fn lit_midway(ws: WinSize, f: Fake, reads: &[Read<'_>]) -> (Vec<Vec<usize>>, Vec<usize>) {
        let t = taken_midway(ws, f, reads, None, Numbering::Driven, |_, _| ());
        (t.midway, t.lit)
    }

    /// **The origin of each of the primary's first sixteen lines at each
    /// [`Then::Took`]**, the lines numbered as the drivers number them:
    /// what a click there would be judged by before any batch after it
    /// names a group.
    fn origins_midway(ws: WinSize, f: Fake, reads: &[Read<'_>]) -> Vec<Vec<Origin>> {
        taken_midway(ws, f, reads, None, Numbering::Driven, |_, _| ()).midway_origins
    }

    /// What [`taken_midway`] found: the rows that draw a line and each
    /// row's origin at the end, what `look` read there, and the rows that
    /// draw a line and the first lines' origins at each [`Then::Took`].
    struct Taken<R> {
        lit: Vec<usize>,
        origins: Vec<Origin>,
        looked: R,
        midway: Vec<Vec<usize>>,
        midway_origins: Vec<Vec<Origin>>,
    }

    /// [`taken_numbered`], with the rows that draw a line at each
    /// [`Then::Took`].
    fn taken_midway<R>(
        ws: WinSize,
        f: Fake,
        reads: &[Read<'_>],
        trim: Option<usize>,
        numbering: Numbering,
        look: impl FnOnce(&Term<'_>, &Paths) -> R,
    ) -> Taken<R> {
        let mut midway = Vec::new();
        let mut midway_origins = Vec::new();
        let string_end = f.string_end;
        let (mut pr, _) = probes(f);
        let most = reads
            .iter()
            .fold((ws.ws_col, ws.ws_row), |(c, r), read| match read.then {
                Then::Resize(cols, rows) => (c.max(cols), r.max(rows)),
                Then::Nothing | Then::Answered | Then::Sent | Then::Took | Then::Paths(_) => (c, r),
            });
        // A page-width request (`DECSCPP`, `DECCOLM`) is resized to between
        // the read's parts, as the app resizes to one: room for 132.
        let asks = reads.iter().any(|r| {
            r.bytes.windows(2).any(|w| w == b"$|")
                || r.bytes.windows(3).any(|w| w == b"?3h" || w == b"?3l")
        });
        let most = WinSize {
            ws_col: match asks {
                true => most.0.max(132),
                false => most.0,
            },
            ws_row: most.1,
        };
        let mut owned = Owned::new(&most).with_images(baud_domain::ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_control_string_end(string_end);
        if asks {
            t.set_width_hold(baud_core::WidthHold::On);
        }
        // A log's start or end (`?46`) stops the read there, as the app's
        // does with logging on: each side a part of its own.
        if reads
            .iter()
            .any(|r| r.bytes.windows(3).any(|w| w == b"?46"))
        {
            t.set_logging(baud_domain::Logging::On);
        }
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let mut s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        // The shell is handed [`TOKEN`] where the session's marks carry a
        // token, as baud hands a window's shell its own at the spawn.
        let marked = reads
            .iter()
            .any(|r| r.bytes.windows(11).any(|w| w == b"baud_token="));
        let mut sampler = match marked {
            true => shells_sampler(),
            false => Sampler::default(),
        };
        let t0 = Instant::now();
        let mut shift = Duration::ZERO;
        // Fed as the macOS pump feeds a drain, ReGIS delays held
        // (`reads::paced`): each call files the reads it fed, a held one's
        // own verdict when its bytes go on.
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = crate::reads::Pace::new();
        let mut i = 0;
        while i < reads.len() {
            let n = 1 + reads[i + 1..]
                .iter()
                .take_while(|r| r.drain == Drain::Same)
                .count();
            let drain = &reads[i..i + n];
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let mut taken = Vec::new();
            let mut now = t0;
            for (k, r) in drain.iter().enumerate() {
                now = t0 + shift + GROUP_EVERY * u32::try_from(i + k + 1).expect("a count");
                let _ = sampler.read(now, r.bytes);
                taken.push(match r.sampled {
                    Sampled::Yes => sampler.writers(Some(r.group), r.watched),
                    Sampled::No | Sampled::Unfiled => sampler.unasked(),
                });
            }
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                drain.iter().map(|r| r.bytes).zip(taken),
                |_| {},
                |t, r| page_width(t, &ws, r),
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let last = took.iter().rev().find_map(|w| w.record);
            let numbered = counts.numbered(&t, last.as_ref()).scrolled();
            let core = match numbering {
                Numbering::Driven => {
                    s = numbered;
                    counts.core_lines(&t)
                }
                Numbering::Held => counts.core_lines_at(s, &t),
            };
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            if !drain.iter().any(|r| r.sampled == Sampled::Unfiled) {
                pr.drain_recut(
                    &mut paths,
                    &t,
                    (began, ended),
                    core,
                    took.into_iter().map(|w| (w, Linked::No)),
                    &pace.recuts,
                    now,
                );
            }
            if let Then::Resize(ws_col, ws_row) = drain[n - 1].then {
                let was = (t.geometry().0, t.screen());
                let was_last = crate::paths::last_line(&t, counts.primary());
                pace.recuts.clear();
                let _ = pace.recuts.resize(&mut t, &WinSize { ws_col, ws_row });
                if numbering == Numbering::Driven {
                    let rec = t.take_record();
                    s = crate::keep::kept_across_resize(&t, was, None, &mut counts, &rec).1;
                    let told = pace.recuts.last();
                    pr.resized(&mut paths, (&rec, told), &t, was_last, &counts);
                }
            }
            if drain[n - 1].then == Then::Sent {
                shift += Duration::from_secs(60);
                sent(
                    &mut pr,
                    (&t, s),
                    (&mut lines, &mut uris, &mut paths),
                    t0 + shift,
                    drain[n - 1].front,
                );
                shift += Duration::from_secs(60);
            }
            if let Then::Paths(on) = drain[n - 1].then {
                paths.set(on);
                lines.forget();
            }
            if drain[n - 1].then == Then::Took {
                took_out(&mut pr, (&t, s), (&mut lines, &mut uris, &mut paths));
                let _ = lines.scan(
                    &t,
                    s,
                    &mut Links {
                        uris: &mut uris,
                        urls: Turned::On,
                        paths: &mut paths,
                    },
                );
                let mut rows: Vec<usize> = lines.lines().iter().map(|l| l.row.index()).collect();
                rows.dedup();
                midway.push(rows);
                midway_origins.push(
                    (0..16)
                        .map(|n| paths.origin(AbsLine::at(Rows::of_count(n))))
                        .collect(),
                );
            }
            if drain[n - 1].then == Then::Answered {
                shift += Duration::from_secs(60);
                answered(
                    &mut pr,
                    (&t, s),
                    (&mut lines, &mut uris, &mut paths),
                    t0 + shift,
                    drain[n - 1].front,
                );
                shift += Duration::from_secs(60);
            }
            i += n;
        }
        answered(
            &mut pr,
            (&t, s),
            (&mut lines, &mut uris, &mut paths),
            t0 + shift + Duration::from_secs(60),
            1,
        );
        if let Some(n) = trim {
            paths.trim_spans(
                t.screen(),
                s.line(Rows::ZERO, baud_domain::Row::from_index(n)),
            );
            lines.forget();
        }
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let mut rows: Vec<usize> = lines.lines().iter().map(|l| l.row.index()).collect();
        rows.dedup();
        let origins = (0..=t.cursor().0.index())
            .map(|y| paths.origin(s.line(Rows::ZERO, baud_domain::Row::from_index(y))))
            .collect();
        let looked = look(&t, &paths);
        // The worker's counts the frame rig reads, set as the driver sets
        // them, for the rows that read them through the probe.
        #[cfg(all(target_os = "macos", feature = "effect-probe"))]
        crate::app::record_unnamed(&pr);
        Taken {
            lit: rows,
            origins,
            looked,
            midway,
            midway_origins,
        }
    }

    /// The groups the tests below name: the shell, a local `ls`, `ssh`, and
    /// two groups gone before they were named, which name nothing.
    fn named_groups() -> Fake {
        let mut f = Fake::with(&[
            ("/etc", Entry::Dir),
            ("/etc/hosts", Entry::File),
            ("/etc/passwd", Entry::File),
        ]);
        f.groups.insert(1, vec!["zsh".into()]);
        f.groups.insert(2, vec!["ls".into()]);
        f.groups.insert(9, vec!["ssh".into()]);
        f.groups.insert(7, Vec::new());
        f.groups.insert(6, Vec::new());
        f
    }

    /// **A short ssh's printed path gets no line, and the local row above
    /// it keeps its own**: `ls` printed `/etc/hosts` on row 0 and `ssh`
    /// printed `/etc/passwd` on row 1, each named. The control: both local,
    /// both lit. Named break: the high-water mark (`elsewhere_through`) --
    /// row 0, written before the ssh, goes dark with it.
    #[test]
    fn a_short_ssh_darkens_only_its_own_rows() {
        let local = lit_after(
            named_groups(),
            &[(b"/etc/hosts\r\n", 2), (b"/etc/passwd\r\n", 2)],
        );
        assert_eq!(local, [0, 1], "the control: both printed here");
        let ssh = lit_after(
            named_groups(),
            &[(b"/etc/hosts\r\n", 2), (b"/etc/passwd\r\n", 9)],
        );
        assert_eq!(ssh, [0], "ssh's row dark, the local row above lit");
    }

    /// **A program gone before it was named darkens only its own rows, and
    /// those fail closed**: its `/etc/passwd` gets no line, as its writer is
    /// not known, and `ls`'s `/etc/hosts` above keeps its line. Under a
    /// command the shell, with its token, named a program on this machine,
    /// the same gone writer is that command's, and its row lights. Named
    /// breaks: the high-water mark -- row 0 goes dark too; a gone group
    /// named not local -- the row under the shell's local command stays
    /// dark.
    #[test]
    fn a_gone_writer_darkens_only_its_own_rows() {
        let rows = lit_after(
            named_groups(),
            &[(b"/etc/hosts\r\n", 2), (b"/etc/passwd\r\n", 7)],
        );
        assert_eq!(rows, [0], "the gone writer's row dark, the row above lit");
        let marked = lit_after(
            named_groups(),
            &[
                (
                    b"/etc/hosts\r\n\x1b]133;C;baud_token=0123456789abcdef;cmdline=cat\\ x\x07",
                    2,
                ),
                (b"/etc/passwd\r\n", 7),
            ],
        );
        assert_eq!(marked, [0, 1], "the shell named its command here");
    }

    /// **The event rig's `SHELLS_D_WARM` act as the terminal reads it**:
    /// two commands, each one `printf` writing the shell's `C` with the
    /// token naming `printf`, `/usr/bin`, a move back onto its row, the `D`
    /// with the token and the `A`, in one read; and the groups the tests
    /// below name, group 7 gone: no names and no start that reads.
    fn warm_act() -> (Fake, Vec<u8>, Vec<u8>) {
        let mut f = named_groups();
        for p in ["/usr", "/usr/bin"] {
            f.entries.insert(p.as_bytes().to_vec(), Entry::Dir);
        }
        f.live.lock().unwrap().insert(7, (Vec::new(), None));
        let command = |tail: &str| {
            format!(
                "\x1b]133;C;baud_token={TOKEN};cmdline=printf\x07/usr/bin\r\n\x1b[A\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07\r\n{tail}"
            )
            .into_bytes()
        };
        (f, command(""), command("\x1b]52;c;!\x07"))
    }

    /// **A tokened command's writer gone before it was named lights its
    /// row, the same path printed again** ([`warm_act`]): the second
    /// command's group is gone before the worker names it (7), with the
    /// first command's batch still out when its read is filed
    /// (`Then::Sent`: asked, the writer gone, the next ask) or answered
    /// before it (`Then::Answered`). Its row is filed not named, and the
    /// command its own marks named here lifts it (`Facts::provenance`), so
    /// both rows light, as when both writers are named local. The control
    /// that a gone writer darkens: the same paths with no marks, the
    /// second row dark.
    ///
    /// **A guard, not a reproduction**: it passes as the lift stands, and
    /// it holds that lift while the front race is fixed (a gone group in
    /// front at a batch, which this row does not have), so a fix that
    /// reaches the writer's verdict as well as the front's shows here.
    /// Named break: no lift (`(Unnamed::Yes, Here::Yes)` resolved
    /// `NotNamed`) -- the second row dark under the marks too.
    #[test]
    fn a_tokened_commands_writer_gone_before_it_was_named_lights_its_row() {
        let (f, first, second) = warm_act();
        let run = |reads: &[Read<'_>]| lit_reads(f.clone(), reads);
        let control = run(&[read(&first, 2), read(&second, 2)]);
        assert_eq!(control, [0, 1], "the control: both writers named local");
        for (then, batch) in [(Then::Sent, "out"), (Then::Answered, "answered")] {
            let before = Read {
                then,
                ..read(&first, 2)
            };
            assert_eq!(
                run(&[before, read(&second, 7)]),
                control,
                "the second writer gone before it was named, the first batch {batch}"
            );
        }
        let bare = b"/usr/bin\r\n".as_slice();
        assert_eq!(
            run(&[read(bare, 2), read(bare, 7)]),
            [0],
            "the control: with no marks, the gone writer's row dark"
        );
    }

    /// **The frame rig's count of programs in front named not local moves
    /// on the front race, and not without it**: the fed control of
    /// `Probes::elsewhere_fronts`, read through the probe as the rig reads
    /// it (`app::record_unnamed`, `probe::paths_elsewhere_fronts`).
    /// [`warm_act`] with the first command's group gone and still in front
    /// when the batch its `D` makes due goes (7), the order the front race
    /// needs, moves it; so does a live group in front whose names do not
    /// read and whose start does (6), as another user's program, which
    /// the count holds whatever a gone group is named; the control, the
    /// shell in front at that batch (1), leaves it at 0 and lights both
    /// rows. Each run is a worker of its own, so what it sets is that
    /// run's count. A change that names a gone group otherwise than not
    /// local moves the gone arm, and changes its assert with it. Named
    /// break: the count's increment in `Probes::answers` removed -- the
    /// gone arm reads 0 and the test stops at its assert, so the unread
    /// arm after it is not reached; once a gone group is named otherwise,
    /// the same break is run again and reds at the unread arm's assert.
    #[cfg(all(target_os = "macos", feature = "effect-probe"))]
    #[test]
    fn the_front_race_moves_the_rigs_count_of_fronts_named_not_local() {
        use baud_present_macos::probe::paths_elsewhere_fronts;
        let (f, first, second) = warm_act();
        let run = |front: Pid| {
            let before = Read {
                then: Then::Answered,
                front,
                ..read(&first, 2)
            };
            let rows = lit_reads(f.clone(), &[before, read(&second, 2)]);
            (rows, paths_elsewhere_fronts().0)
        };
        assert_eq!(
            run(1),
            (vec![0, 1], 0),
            "the control: the shell in front at the batch"
        );
        let (_, gone) = run(7);
        assert!(
            gone >= 1,
            "the gone group in front at its batch, named not local {gone} times"
        );
        let (_, unread) = run(6);
        assert!(
            unread >= 1,
            "a live group in front whose names do not read, named not local {unread} times"
        );
    }

    /// **The frame rig's count of writers not named moves on an untokened
    /// writer gone before it was named, and not without one**: the fed
    /// control of `Probes::not_named`, read through the probe as the rig
    /// reads it (`app::record_unnamed`, `probe::paths_not_named`). `ls`
    /// prints `/etc/hosts`, and a group gone before the worker names it
    /// (7) prints `/etc/passwd` with no marks to lift it, so its verdict is
    /// not named and its row dark; the control, both writers `ls` (2),
    /// leaves it at 0. Each run is a worker of its own, so what it sets is
    /// that run's count. Named break: the count's increment in
    /// `Probes::file_answered` removed -- the gone arm reads 0.
    #[cfg(all(target_os = "macos", feature = "effect-probe"))]
    #[test]
    fn a_writer_gone_before_it_was_named_moves_the_rigs_count_of_writers_not_named() {
        use baud_present_macos::probe::paths_not_named;
        let run = |second: Pid| {
            let rows = lit_after(
                named_groups(),
                &[(b"/etc/hosts\r\n", 2), (b"/etc/passwd\r\n", second)],
            );
            (rows, paths_not_named().0)
        };
        assert_eq!(
            run(2),
            (vec![0, 1], 0),
            "the control: both writers named local"
        );
        let (rows, gone) = run(7);
        assert_eq!(rows, [0], "the gone writer's row dark, the row above lit");
        assert!(
            gone >= 1,
            "the gone writer's verdict, filed not named {gone} times"
        );
    }

    /// **R1: a line erased whole takes the writer filed after the erase**
    /// (diag-hang2's shape): a program gone before it was named prints
    /// `/etc/passwd` on row 0 and its batch is answered; after the shell's
    /// prompt, a local read clears the screen and prints `/etc/passwd`
    /// there again. Row 0
    /// lights: the gone writer's fact went with its text. So it does with
    /// the clear before the gone writer's answer is taken, an answer about
    /// text the clear erased, which is then filed over none of its lines.
    /// The control: the local read homes and prints over the line with no
    /// erase, and the row stays dark. Named breaks: facts kept across an
    /// erase (`Paths::forget_erased` doing nothing) -- the row stays dark;
    /// no cut (`Probes::cut` doing nothing) -- the answer taken after the
    /// clear darkens it.
    #[test]
    fn a_line_erased_whole_takes_the_writer_filed_after_it() {
        let clear = read(b"\x1b[2J\x1b[H/etc/passwd\r\n", 2);
        let gone = |then| Read {
            then,
            ..read(b"/etc/passwd\r\n", 7)
        };
        let prompt = read(b"$ ", 1);
        assert_eq!(
            lit_reads(named_groups(), &[gone(Then::Answered), prompt, clear]),
            [0]
        );
        assert_eq!(
            lit_reads(named_groups(), &[gone(Then::Sent), prompt, clear]),
            [0],
            "the clear before the answer"
        );
        let over = read(b"\x1b[H/etc/passwd\r\n", 2);
        assert_eq!(
            lit_reads(named_groups(), &[gone(Then::Answered), prompt, over]),
            Vec::<usize>::new(),
            "the control: no erase"
        );
    }

    /// **R2: after an ssh session, its exit and a `clear`, a local listing
    /// on the same lines lights** -- the shell integration's own bytes:
    /// `ssh h`, named not local by the shell's marks, prints `/etc/passwd`
    /// on row 1; the shell's `D`, then `clear`, run with its `C`, whose
    /// program prints `ED 2` and `ED 3`; the `D`, then `ls`, whose
    /// `/etc/passwd` lands on row 1 again. So it does where Ctrl-L clears
    /// instead, the shell redrawing with no marks around it. The control:
    /// `clear`'s program homes and erases nothing, and the ssh's row stays
    /// dark under the listing. Named break: facts kept across an erase
    /// (`Paths::forget_erased` doing nothing) -- row 1 stays dark.
    #[test]
    fn after_ssh_its_exit_and_a_clear_a_local_listing_lights() {
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let clear = [d.as_slice(), &ran("clear", ";cmdline=clear")].concat();
        let ls = [d.as_slice(), &ran("ls", ";cmdline=ls")].concat();
        let by_clear = |wipe: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(&ssh, 1),
                    read(b"/etc/passwd\r\n", 9),
                    read(&clear, 1),
                    read(wipe, 7),
                    read(&ls, 1),
                    read(b"/etc/passwd\r\n", 2),
                ],
            )
        };
        assert_eq!(by_clear(b"\x1b[H\x1b[2J\x1b[3J"), [1], "clear");
        assert_eq!(
            by_clear(b"\x1b[H"),
            Vec::<usize>::new(),
            "the control: no erase"
        );
        let prompt = prompt_after("");
        let ls_bare = ran("ls", ";cmdline=ls");
        let ctrl_l = lit_reads(
            named_groups(),
            &[
                read(&ssh, 1),
                read(b"/etc/passwd\r\n", 9),
                read(&prompt, 1),
                read(b"\x1b[H\x1b[2J", 1),
                read(&ls_bare, 1),
                read(b"/etc/passwd\r\n", 2),
            ],
        );
        assert_eq!(ctrl_l, [1], "Ctrl-L");
    }

    /// **E2: `ED 3` after `ED 2` leaves no fact of the history it took**:
    /// on a ten-row window `ssh h` prints fifteen `/etc/passwd` lines, the
    /// first of them into the history; `clear` erases the screen and the
    /// history; `ls` prints fifteen more. Line 10, `ssh`'s and then the
    /// listing's, is this machine's; line 2, in the history the clear
    /// erased, has no fact left: the screen's lines the ssh wrote were
    /// forgotten, so its run ends above the window, and the next answer
    /// trims it there (`Paths::trim_spans`). Named break: facts kept across
    /// an erase (`Paths::forget_erased` doing nothing) -- line 2 still
    /// answers remote, and line 10 too.
    #[test]
    fn ed_3_after_ed_2_leaves_no_fact_of_the_history_it_took() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let clear = [d.as_slice(), &ran("clear", ";cmdline=clear")].concat();
        let ls = [d.as_slice(), &ran("ls", ";cmdline=ls")].concat();
        let lines = b"/etc/passwd\r\n".repeat(15);
        let reads = [
            read(&ssh, 1),
            read(&lines, 9),
            read(&clear, 1),
            read(b"\x1b[H\x1b[2J\x1b[3J", 7),
            read(&ls, 1),
            read(&lines, 2),
        ];
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let (_, _, (old, new)) = taken_numbered(
            ws,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |_, paths| (paths.origin(at(2)), paths.origin(at(10))),
        );
        assert_eq!(new, Origin::Local, "the listing's line");
        assert_eq!(old, Origin::Unknown, "the history the clear took");
    }

    /// **S1: a remote writer after an erase leaves its line remote**: `ls`
    /// prints `/etc/hosts` on row 0, lit; then `ssh` clears the screen and
    /// prints it again in one read, or after a local clear. The row is dark
    /// and remote. Named break: forget after the drain's filing -- the
    /// remote read's own lines are cut, and it reads not known.
    #[test]
    fn a_remote_writer_after_an_erase_leaves_its_line_remote() {
        let lit = Read {
            then: Then::Answered,
            ..read(b"/etc/hosts\r\n", 2)
        };
        assert_eq!(lit_reads(named_groups(), &[lit]), [0], "the control");
        let (rows, origins) = taken(
            named_groups(),
            &[lit, read(b"\x1b[2J\x1b[H/etc/hosts\r\n", 9)],
        );
        assert_eq!(rows, Vec::<usize>::new(), "one read");
        assert_eq!(origins[0], Origin::Remote, "one read");
        let (rows, origins) = taken(
            named_groups(),
            &[lit, read(b"\x1b[2J\x1b[H", 2), read(b"/etc/hosts\r\n", 9)],
        );
        assert_eq!(rows, Vec::<usize>::new(), "after a local clear");
        assert_eq!(origins[0], Origin::Remote, "after a local clear");
    }

    /// **S2: a writer not named after an erase leaves its line not known**:
    /// `ls`'s lit row 0, a clear, and a program gone before it was named
    /// prints there, in the clear's read or after it. The row is dark: the
    /// local fact went with the text before.
    ///
    /// No wrong forget or cut can light it, because the gone writer's own
    /// verdict, not known, is filed over the row by its answer
    /// (`Probes::file_answered`), which comes after the erase in every
    /// order. Forgetting nothing leaves `ls`'s local fact under it, and the
    /// most cautious fact decides. Forgetting more can only take the local
    /// fact. Forgetting and cutting after the drain's filing cuts the gone
    /// writer's own span, so its answer files nothing there, and a line
    /// with no fact reads not known (`Paths::wrote`), never local. What
    /// lights it is a wrong writer for the read. Named break: an erasing
    /// read's writer taken from the sample before it alone
    /// (`Probes::read_taken`) -- in one read, `ls` is named for the gone
    /// writer's text.
    #[test]
    fn a_writer_not_named_after_an_erase_leaves_its_line_dark() {
        let lit = Read {
            then: Then::Answered,
            ..read(b"/etc/hosts\r\n", 2)
        };
        let after =
            |reads: &[Read<'_>]| lit_reads(named_groups(), &[[lit].as_slice(), reads].concat());
        assert_eq!(
            after(&[read(b"\x1b[2J\x1b[H/etc/hosts\r\n", 7)]),
            Vec::<usize>::new(),
            "one read"
        );
        assert_eq!(
            after(&[read(b"\x1b[2J\x1b[H", 2), read(b"/etc/hosts\r\n", 7)]),
            Vec::<usize>::new(),
            "after a local clear"
        );
    }

    /// **S3: a local write over part of a line not known leaves it not
    /// known**: a program gone before it was named prints `/etc/hosts` on
    /// row 0; then a local read returns and prints it over, or prints it
    /// over and erases to the line's end from column 11 (`EL 0`, a part of
    /// the line). The row stays dark: the old text may still be there, and
    /// nothing erased the line. The control: `EL 2` first, the whole line,
    /// and the local text lights. Named break: every erase recorded whole
    /// -- the `EL 0` lights.
    #[test]
    fn a_local_write_over_part_of_a_line_not_known_leaves_it_dark() {
        let gone = Read {
            then: Then::Answered,
            ..read(b"/etc/hosts", 7)
        };
        let over = |bytes: &'static [u8]| {
            lit_reads(named_groups(), &[gone, read(b"\r\n$ ", 1), read(bytes, 2)])
        };
        assert_eq!(over(b"\x1b[A\r/etc/hosts"), Vec::<usize>::new(), "over");
        assert_eq!(
            over(b"\x1b[A\r/etc/hosts\x1b[K"),
            Vec::<usize>::new(),
            "over, EL 0 from column 11"
        );
        assert_eq!(over(b"\x1b[A\r\x1b[2K/etc/hosts"), [0], "the control: EL 2");
    }

    /// **S4: a read that erases and then writes files over what it wrote
    /// alone**: `ssh` prints `/etc/passwd` on row 0; then one local read
    /// clears the screen and prints it again. Row 0 lights. Named breaks:
    /// facts kept across an erase -- the row stays dark; forget after the
    /// drain's filing -- the local read's own lines are cut, and the row
    /// reads not known.
    #[test]
    fn a_read_that_erases_then_writes_files_over_what_it_wrote_alone() {
        let ssh = Read {
            then: Then::Answered,
            ..read(b"/etc/passwd\r\n", 9)
        };
        let local = read(b"\x1b[2J\x1b[H/etc/passwd\r\n", 2);
        assert_eq!(
            lit_reads(named_groups(), &[ssh, read(b"$ ", 1), local]),
            [0]
        );
    }

    /// **S5: an erase that spares a guarded area leaves its line's facts**:
    /// a program gone before it was named prints `/etc/hosts` in a guarded
    /// area (`SPA`, `EPA`) on row 0; a local `ED 2` under ERM's PROTECT
    /// leaves it, and the row stays dark. The control: under ALL the same
    /// erase takes the text, and the local `/etc/hosts` printed after it
    /// lights. Named break: every erase recorded whole -- the guarded row
    /// lights.
    #[test]
    fn an_erase_that_spares_a_guarded_area_leaves_its_lines_facts() {
        let gone = Read {
            then: Then::Answered,
            ..read(b"\x1bV/etc/hosts\x1bW", 7)
        };
        let after = |bytes: &'static [u8]| {
            lit_reads(named_groups(), &[gone, read(b"\r\n$ ", 1), read(bytes, 2)])
        };
        assert_eq!(after(b"\x1b[2J"), Vec::<usize>::new(), "PROTECT");
        assert_eq!(
            after(b"\x1b[6h\x1b[2J\x1b[H/etc/hosts"),
            [0],
            "the control: ALL"
        );
    }

    /// **One rule on both screens** (the alternate screen within one
    /// clear's epoch): `ssh`'s `/etc/passwd` on the alternate screen's row
    /// 2, then a local program's `ED 2` and `/etc/passwd` there again --
    /// row 2 lights; the reverse, a local line and `ssh`'s redraw, stays
    /// dark. The control: local both, lit. Named break: facts kept across
    /// an erase -- the local redraw over the remote line stays dark.
    #[test]
    fn an_alternate_screen_redraw_takes_the_writer_after_its_erase() {
        let run = |first: Pid, then: Pid| {
            lit_driven(
                TALL,
                named_groups(),
                &[
                    read(b"\x1b[?1049h", first),
                    Read {
                        then: Then::Answered,
                        ..read(b"\x1b[3;1H/etc/passwd", first)
                    },
                    read(b"\x1b[8;1H", 1),
                    read(b"\x1b[2J\x1b[3;1H/etc/passwd", then),
                ],
            )
        };
        assert_eq!(run(2, 2), [2], "the control: local both");
        assert_eq!(run(9, 2), [2], "a local redraw over remote lines");
        assert_eq!(
            run(2, 9),
            Vec::<usize>::new(),
            "a remote redraw over local lines"
        );
    }

    /// **E5: each part of a drain forgets what its own record erased**: a
    /// remote line, then one drain of two local parts, the first clearing
    /// the screen and the second printing `/etc/passwd` on row 0. It
    /// lights. Named break: one record for the whole drain, its last part's
    /// -- the clear is missed and the row stays dark.
    #[test]
    fn each_part_of_a_drain_forgets_what_its_own_record_erased() {
        let reads = [
            Read {
                then: Then::Answered,
                ..read(b"/etc/passwd\r\n", 9)
            },
            read(b"$ ", 1),
            read(b"\x1b[2J\x1b[H", 2),
            Read {
                drain: Drain::Same,
                ..read(b"/etc/passwd\r\n", 2)
            },
        ];
        assert_eq!(lit_reads(named_groups(), &reads), [0]);
    }

    /// **A remote `clear` before `exit` leaves the lines it blanked remote,
    /// pinned as it is**: `ssh h` prints `/etc/passwd` on row 1 and clears
    /// the screen itself; it exits and `ls` prints `/etc/passwd` on row 1.
    /// The row is dark: the remote clear's read is filed over every line
    /// it blanked, and the listing erases nothing. An under-mark, never an
    /// over-mark: a change that forgets the lines still blank at the end
    /// of the read that erased them flips this to lit.
    #[test]
    fn a_remote_clear_before_exit_leaves_its_blank_lines_remote_an_under_mark() {
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let ls = [d.as_slice(), &ran("ls", ";cmdline=ls")].concat();
        let rows = lit_reads(
            named_groups(),
            &[
                read(&ssh, 1),
                read(b"/etc/passwd\r\n", 9),
                read(b"\x1b[H\x1b[2J", 9),
                read(&ls, 1),
                read(b"/etc/passwd\r\n", 2),
            ],
        );
        assert_eq!(rows, Vec::<usize>::new());
    }

    /// **E1: an erasure the record could not hold, or the part's lines
    /// could not place, forgets nothing and is counted**: every line filed
    /// remote, then one read naming nothing erases. Four `EL 2` apart
    /// forget row 0, which reads not known; five overflow the record, are
    /// counted, and row 0 stays remote; `ED 2` on a part whose primary
    /// lines are not placed is counted, and row 0 stays remote. Named
    /// break: an overflow not counted -- the count reads 0.
    #[test]
    fn an_erasure_not_held_or_not_placed_forgets_nothing_and_is_counted() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let run = |bytes: &[u8], placed: bool| -> (u64, u64, Origin) {
            let (mut pr, _) = probes(named_groups());
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let mut paths = Paths::new(None, Forms::DEFAULT);
            paths.set(Turned::On);
            let mut counts = crate::keep::Counts::new(&t);
            paths.written_remote(LineSpan::of(
                AbsLine::at(Rows::ZERO),
                AbsLine::at(Rows::of_count(9)),
            ));
            let began = paths.line_on(t.screen(), AbsLine::at(Rows::ZERO));
            let mut pace = crate::reads::Pace::new();
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                Instant::now(),
                [(bytes, Sampler::default().unasked())],
                |_| {},
                |t, r| page_width(t, &ws, r),
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            let lines = match placed {
                true => counts.core_lines(&t),
                false => crate::paths::CoreLines::of(None),
            };
            pr.drain_taken(
                &mut paths,
                &t,
                (began, began),
                lines,
                took.into_iter().map(|w| (w, Linked::No)),
                Instant::now(),
            );
            (
                pr.counts.erase_overflowed,
                pr.counts.erase_unplaced,
                paths.origin(AbsLine::at(Rows::ZERO)),
            )
        };
        let apart = |n: usize| -> Vec<u8> {
            (0..n)
                .flat_map(|k| format!("\x1b[{};1H\x1b[2K", 1 + 2 * k).into_bytes())
                .collect()
        };
        assert_eq!(
            run(&apart(4), true),
            (0, 0, Origin::Unknown),
            "the control: four"
        );
        assert_eq!(run(&apart(5), true), (1, 0, Origin::Remote), "five");
        assert_eq!(run(b"\x1b[2J", false), (0, 1, Origin::Remote), "not placed");
    }

    /// **Only a read's record ends a line's facts**: `Store::forget_lines`
    /// is called once, by `Paths::forget_erased`; that is called once in
    /// the app's code, by `Probes::erased`; an `Erased` is made only by
    /// `Erased::of`, which reads the core's record of the read and nothing
    /// of the screen's cells; and nothing in `probe.rs` makes one or reads
    /// the record's erasures itself. Perturbation: call `forget_erased`
    /// from anywhere else -- the count.
    #[test]
    fn only_a_reads_record_ends_a_lines_facts() {
        use crate::no_appkit::{bare_code, fn_body, without_tests};
        let paths = without_tests(&bare_code(include_str!("paths.rs")));
        let probe = without_tests(&bare_code(include_str!("probe.rs")));
        assert_eq!(paths.matches("forget_lines(").count(), 2, "one call");
        assert!(fn_body(&paths, "pub fn forget_erased(").contains(".forget_lines("));
        assert_eq!(paths.matches("forget_erased(").count(), 1, "defined");
        assert_eq!(probe.matches("forget_erased(").count(), 1, "one call");
        assert!(fn_body(&probe, "fn erased(").contains(".forget_erased("));
        assert_eq!(paths.matches("Erased(LineSpan").count(), 2, "one maker");
        let of = fn_body(&paths, "impl Erased ");
        assert!(of.contains("Erased(LineSpan::on("), "made in Erased::of");
        assert!(of.contains("rec.erased(screen)"), "from the record");
        assert!(!of.contains("term") && !of.contains("row("), "no cell read");
        assert_eq!(probe.matches("Erased(").count(), 0, "none made here");
        assert_eq!(probe.matches("rec.erased(").count(), 0, "nor read here");
    }

    /// **`bytes` fed to a screen of [`WS`], and the lines its record says
    /// it erased whole forgotten and cut out of every span `pr` keeps**
    /// ([`Probes::erased`]), on the primary's lines as a screen with no
    /// history numbers them, from 0 -- and nothing of the read filed: what
    /// the erase leaves before any writer is filed after it.
    fn erased_by(pr: &mut Probes<Instant>, paths: &mut Paths, bytes: &[u8]) {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut counts = crate::keep::Counts::new(&t);
        let mut pace = crate::reads::Pace::new();
        let mut took = Vec::new();
        let _ = crate::reads::paced(
            &mut t,
            &mut pace,
            Instant::now(),
            [(bytes, Sampler::default().unasked())],
            |_| {},
            |t, r| page_width(t, &WS, r),
            &mut crate::hostlog::NoLog,
            &mut took,
        );
        let last = took.iter().rev().find_map(|w| w.record);
        let _ = counts.numbered(&t, last.as_ref());
        let lines = counts.core_lines(&t);
        for w in &took {
            if let Some(rec) = w.record {
                pr.erased(paths, &rec, &lines.part(w.top), &[Screen::Normal]);
            }
        }
        pr.publish(paths);
    }

    /// **A read that moved nothing hands nothing on and compares no span**
    /// (`Probes::publish`): sixteen groups kept to send and 64 holds,
    /// handed on; then a read by a kept group over lines its span already
    /// holds, and a drain of text whose record moved no line. Neither
    /// `Paths::publishes` nor `Paths::pending_steps` moves. The controls:
    /// the drain is one probe run, and a read that grows a kept group's
    /// span by a line is handed on once and compares. Named break:
    /// `publish` without the generation check -- the unchanged read and
    /// drain hand on three times, each comparing every span.
    #[test]
    fn an_unchanged_read_compares_no_span() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut counts = crate::keep::Counts::new(&t);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n: usize| AbsLine::at(Rows::of_count(n));
        let group = |g: usize| Pid::try_from(100 + g).expect("a pid");
        for g in 0..GROUPS {
            let first = line(1_000 + 10 * g);
            pr.kept
                .edit()
                .seen
                .push((group(g), LineSpan::of(first, first.below())));
        }
        for h in 0..64 {
            let at = line(2_000 + 2 * h);
            pr.held
                .edit()
                .push((Some(group(h % GROUPS)), LineSpan::of(at, at)));
        }
        pr.publish(&mut paths);
        let count = |paths: &Paths| (paths.publishes().value(), paths.pending_steps().value());
        let before = count(&paths);
        // A kept group's read over a line its span holds.
        pr.read_taken(
            &mut paths,
            Writers::at(Some(group(0))),
            LineSpan::of(line(1_000), line(1_000)),
            Linked::No,
        );
        // A drain of text that moved no line: no scroll, no erase.
        let began = paths.line_on(t.screen(), line(0));
        let mut pace = crate::reads::Pace::new();
        let mut took = Vec::new();
        let _ = crate::reads::paced(
            &mut t,
            &mut pace,
            Instant::now(),
            [(&b"plain text"[..], Sampler::default().unasked())],
            |_| {},
            |t, r| page_width(t, &ws, r),
            &mut crate::hostlog::NoLog,
            &mut took,
        );
        assert!(
            took.iter().any(|w| w.record.is_some()),
            "the control: the drain's part has a record"
        );
        let rec = took.iter().rev().find_map(|w| w.record);
        let _ = counts.numbered(&t, rec.as_ref());
        let runs = pr.probe_runs().value();
        pr.drain_taken(
            &mut paths,
            &t,
            (began, began),
            counts.core_lines(&t),
            took.into_iter().map(|w| (w, Linked::No)),
            Instant::now(),
        );
        assert_eq!(
            pr.probe_runs().value() - runs,
            1,
            "the control: the drain is one probe run"
        );
        assert_eq!(
            count(&paths),
            before,
            "(publishes, pending_steps) after a read and a drain that moved nothing"
        );
        // The control: the same group's read one line past its span.
        pr.read_taken(
            &mut paths,
            Writers::at(Some(group(0))),
            LineSpan::of(line(1_002), line(1_002)),
            Linked::No,
        );
        let (publishes, steps) = count(&paths);
        assert_eq!(publishes - before.0, 1, "the control: one hand-on");
        assert!(steps > before.1, "the control: the moved span compared");
    }

    /// **A hold over a line erased whole is let go** (`Probes::cut`,
    /// `held`): `ls`'s lit text on rows 0 and 1; a program gone before it
    /// was named wrote over both, so they are held under its group, its
    /// batch out; row 1 is erased whole, and `ls` prints on it again, its
    /// own batch out. The gone writer's answer is about text the erase
    /// took on row 1, and releases its hold on row 0 alone: row 0 stays not
    /// known, and row 1 is `ls`'s. Named break: no cut of the holds -- the
    /// gone writer's verdict is filed over row 1 as it releases the hold
    /// there, and `ls`'s text reads not known.
    #[test]
    fn a_hold_over_a_line_erased_whole_is_let_go() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        let both = LineSpan::of(line(0), line(1));
        let id = Id::default();
        paths.written_under(crate::paths::Seen::Named(Front::Local), both);
        pr.held.edit().push((Some(7), both));
        pr.kept.edit().inflight.push((id, Some(7), both));
        pr.publish(&mut paths);
        erased_by(&mut pr, &mut paths, b"\x1b[2;1H\x1b[2K");
        pr.kept
            .edit()
            .inflight
            .push((id, Some(2), LineSpan::of(line(1), line(1))));
        pr.publish(&mut paths);
        pr.file_answered(
            &mut paths,
            (id, Kind::Screen),
            &[
                (Some(2), crate::paths::Seen::Named(Front::Local)),
                (Some(7), crate::paths::Seen::NotNamed),
            ],
            &[],
        );
        assert_eq!(
            [0, 1].map(|n| paths.origin(line(n))),
            [Origin::Unknown, Origin::Local]
        );
    }

    /// **A line erased whole names no group as what lit it** (`Probes::cut`,
    /// `Lit`; G1): `ls`'s verdict lit rows 0 and 1, the shell's row 2; row
    /// 1 is erased whole; a move carries the shell's line onto row 1, its
    /// local fact and what lit it with it (`Paths::carried`, `Lit::carry`);
    /// then `ls`, still to be named, writes over rows 0 and 1. Row 0 is
    /// `ls`'s rewrite of its own and stays local. Row 1 holds the shell's
    /// fact, which no verdict of `ls`'s filed, so it is held until `ls`'s
    /// answer: not known. Named break: no cut of `Lit` -- `ls` is still
    /// named for row 1, its write there holds nothing, and its text, not
    /// yet named, reads local on the shell's fact.
    #[test]
    fn a_line_erased_whole_names_no_group_as_what_lit_it() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        let one = |n| LineSpan::of(line(n), line(n));
        let local = crate::paths::Seen::Named(Front::Local);
        paths.written_under(local, LineSpan::of(line(0), line(2)));
        pr.lit.set(Some(2), LineSpan::of(line(0), line(1)));
        pr.lit.set(Some(1), one(2));
        erased_by(&mut pr, &mut paths, b"\x1b[2;1H\x1b[2K");
        paths.written_under(local, one(1));
        pr.lit.carry(|s| match s == one(2) {
            true => one(1),
            false => s,
        });
        pr.read_taken(
            &mut paths,
            Writers::at(Some(2)),
            LineSpan::of(line(0), line(1)),
            Linked::No,
        );
        assert_eq!(
            [0, 1].map(|n| paths.origin(line(n))),
            [Origin::Local, Origin::Unknown]
        );
    }

    /// **A line erased whole leaves the lines written past the groups
    /// kept** (`Probes::cut`, `unnamed`): writers past the groups kept
    /// wrote rows 0 to 2, still to be named; row 2, the last of them, is
    /// erased whole, and `ls`'s text printed there after is named local;
    /// then the worker closes, and every line still to be named is filed
    /// not known, as the worker's answer files those lines
    /// (`answer`). Rows 0 and 1 are not known, and row 2 is `ls`'s. Named
    /// break: no cut of `unnamed` -- row 2 is filed not known over `ls`'s
    /// text.
    #[test]
    fn a_line_erased_whole_leaves_the_lines_past_the_groups_kept() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        pr.kept.edit().unnamed = Some(LineSpan::of(line(0), line(2)));
        pr.publish(&mut paths);
        erased_by(&mut pr, &mut paths, b"\x1b[3;1H\x1b[2K");
        paths.written_under(
            crate::paths::Seen::Named(Front::Local),
            LineSpan::of(line(2), line(2)),
        );
        let (mut lines, mut turn) = (Lines::default(), Turn::default());
        pr.close(&mut paths, &mut lines, &mut turn);
        assert_eq!(
            [0, 1, 2].map(|n| paths.origin(line(n))),
            [Origin::Unknown, Origin::Unknown, Origin::Local]
        );
    }

    /// **An erase inside the lines written past the groups kept leaves
    /// them all not known, and its own line dark** (`Probes::cut`, which
    /// keeps `unnamed` one span): `ls`'s verdict lit rows 0 to 4; `ls`,
    /// past the groups kept, rewrites them, which holds none of them, its
    /// rewrite of its own (G1); row 2 is erased whole, and `ssh`, still to
    /// be named, prints a path on it, dark; then the worker closes, and
    /// every line still to be named is filed not known, as the worker's
    /// answer files those lines (`answer`). Every row is: what `ls` wrote
    /// past the groups kept was never named, and row 2 holds only `ssh`'s
    /// text. Row 2 cannot light before `ssh` is named: the erase left it
    /// no fact, and the lines past the groups kept are answered not known,
    /// never local, so the span still covering row 2 after the erase can
    /// only darken it -- an under-mark once `ssh`'s line is named local,
    /// never an over-mark. Named break: the span cut to its piece above the
    /// erase (`outside(..).next()`) -- rows 3 and 4 read local.
    #[test]
    fn an_erase_inside_the_lines_past_the_groups_kept_leaves_them_all_not_known() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        let rows = LineSpan::of(line(0), line(4));
        paths.written_under(crate::paths::Seen::Named(Front::Local), rows);
        pr.lit.set(Some(2), rows);
        pr.kept.edit().unnamed = Some(rows);
        pr.publish(&mut paths);
        erased_by(&mut pr, &mut paths, b"\x1b[3;1H\x1b[2K");
        pr.read_taken(
            &mut paths,
            Writers::at(Some(9)),
            LineSpan::of(line(2), line(2)),
            Linked::No,
        );
        assert_eq!(paths.origin(line(2)), Origin::Unknown, "before the close");
        let (mut lines, mut turn) = (Lines::default(), Turn::default());
        pr.close(&mut paths, &mut lines, &mut turn);
        assert_eq!(
            [0, 1, 2, 3, 4].map(|n| paths.origin(line(n))),
            [Origin::Unknown; 5]
        );
    }

    /// **An erase inside the lines written past the groups kept leaves the
    /// line its writer named local dark, pinned as it is**: the shell's
    /// verdict lit rows 0 to 4; writers past the groups kept wrote them
    /// again, still to be named; row 2 is erased whole, and `ls` prints on
    /// it; one batch asks for both, and the answer files them in the
    /// worker's order (`answer`), `ls` named local first and then the lines
    /// past the groups kept, not known. Before the answer row 2 is not
    /// known: the erase took the shell's fact with its text, and no other
    /// group's verdict lights `ls`'s line. After it every row is not known,
    /// row 2 too: the hull still covers row 2 (`Probes::cut`), `ls`'s
    /// local verdict holds it under the lines past the groups kept, and
    /// their answer files it not known. An under-mark, never an over-mark.
    /// Named breaks: the hull cut to its piece above the erase
    /// (`outside(..).next()`) -- row 2 reads local after the answer; the
    /// erase forgetting no fact (`Paths::forget_erased` doing nothing) --
    /// row 2 reads local before it.
    #[test]
    fn an_erase_inside_the_lines_past_the_groups_kept_then_named_local_is_an_under_mark() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        let rows = LineSpan::of(line(0), line(4));
        let row2 = LineSpan::of(line(2), line(2));
        paths.written_under(crate::paths::Seen::Named(Front::Local), rows);
        pr.lit.set(Some(1), rows);
        pr.kept.edit().unnamed = Some(rows);
        pr.publish(&mut paths);
        erased_by(&mut pr, &mut paths, b"\x1b[3;1H\x1b[2K");
        assert_eq!(
            pr.kept.get().unnamed,
            Some(rows),
            "the hull after the erase"
        );
        pr.read_taken(&mut paths, Writers::at(Some(2)), row2, Linked::No);
        assert_eq!(paths.origin(line(2)), Origin::Unknown, "before the answer");
        let id = Id::default();
        let seen = core::mem::take(&mut pr.kept.edit().seen);
        assert_eq!(seen, [(2, row2)], "ls kept with its line");
        let hull = pr.kept.edit().unnamed.take();
        let asked = seen.iter().map(|&(g, l)| (Some(g), l));
        for (g, l) in asked.chain(hull.map(|l| (None, l))) {
            pr.kept.edit().inflight.push((id, g, l));
        }
        pr.publish(&mut paths);
        pr.file_answered(
            &mut paths,
            (id, Kind::Screen),
            &[
                (Some(2), crate::paths::Seen::Named(Front::Local)),
                (None, crate::paths::Seen::NotNamed),
            ],
            &[],
        );
        assert_eq!(
            [0, 1, 2, 3, 4].map(|n| paths.origin(line(n))),
            [Origin::Unknown; 5]
        );
    }

    /// **A line written while paths were off, floored when they come on and
    /// then erased whole, takes the writer after the erase**: with paths
    /// off, `ls` prints `/etc/passwd` on row 0, a read not kept; paths on
    /// again; `ls` erases row 0 whole and prints `/etc/hosts` there. The
    /// drain floors row 0 not known (`Probes::floor_off`) before its erase,
    /// which ends that fact with the text it was about, and the row lights.
    /// The control: `ls` prints `/etc/passwd` over it with no erase, and
    /// the floor leaves it dark. Named break: facts kept across an erase
    /// (`Paths::forget_erased` doing nothing) -- the floor's fact stays
    /// under `ls`'s text.
    #[test]
    fn a_line_floored_when_paths_come_on_and_then_erased_whole_lights() {
        let run = |after: &'static [u8]| {
            lit_reads(
                named_groups(),
                &[
                    Read {
                        then: Then::Paths(Turned::Off),
                        ..read(b"\x1b[m", 2)
                    },
                    Read {
                        then: Then::Paths(Turned::On),
                        ..read(b"/etc/passwd", 2)
                    },
                    read(after, 2),
                ],
            )
        };
        assert_eq!(run(b"\r\x1b[2K/etc/hosts"), [0]);
        assert_eq!(
            run(b"\r/etc/passwd"),
            Vec::<usize>::new(),
            "the control: no erase"
        );
    }

    /// **A line erased whole leaves the lines written while paths were
    /// off** (`Probes::cut`, `off`): rows 0 to 2 written by reads not kept
    /// while paths were off; row 2 is erased whole, and `ls`'s text written
    /// there after is named local; then the floor files what is left of
    /// them not known (`Probes::floor_off`). Rows 0 and 1 are not known,
    /// and row 2 is `ls`'s. No drain reaches this order today, so the cut
    /// is a second guard: with paths on, `Probes::drain_recut` floors
    /// before any part's erase, which leaves nothing to cut; with paths
    /// off, the erasing read's own lines, every line it erased among them,
    /// are written off again after the cut (`Probes::read_taken`,
    /// `off_wrote`). Named break: no cut of `off` -- row 2 is floored not
    /// known over `ls`'s text.
    #[test]
    fn a_line_erased_whole_leaves_the_lines_written_while_paths_were_off() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        pr.off.push(LineSpan::of(line(0), line(2)));
        erased_by(&mut pr, &mut paths, b"\x1b[3;1H\x1b[2K");
        paths.written_under(
            crate::paths::Seen::Named(Front::Local),
            LineSpan::of(line(2), line(2)),
        );
        pr.floor_off(&mut paths);
        assert_eq!(
            [0, 1, 2].map(|n| paths.origin(line(n))),
            [Origin::Unknown, Origin::Unknown, Origin::Local]
        );
    }

    /// **The prompt after a quick command is the shell's, whoever wrote the
    /// command's output**: the shell's marks (`OSC 133`, with its token)
    /// around a command whose line it did not send, as bash's with history
    /// off; the child, gone before it was named, prints `/etc/passwd`; the
    /// next prompt, sampled with the child before it, has `/etc/hosts`
    /// typed on it, not run. The typed path lights and the child's does
    /// not. The control: the same with the child named local lights both.
    /// Named breaks: the output's span running onto the line the cursor
    /// rests on; the prompt's read given to the group sampled before it.
    #[test]
    fn the_prompt_after_a_quick_command_keeps_its_lines() {
        let run = |child: Pid| {
            lit_after(
                named_groups(),
                &[
                    (
                        b"\x1b]133;A;aid=1\x07% /bin/echo\r\n\x1b]133;C;baud_token=0123456789abcdef\x07",
                        1,
                    ),
                    (b"/etc/passwd\r\n", child),
                    (
                        b"\x1b]133;D;0;aid=1;baud_token=0123456789abcdef\x07\x1b]133;A;aid=1\x07% /etc/hosts",
                        1,
                    ),
                ],
            )
        };
        assert_eq!(run(2), [1, 2], "the control: a local child");
        assert_eq!(run(7), [2], "the typed path lit, the gone child's dark");
    }

    /// The shell's token in the tests' marks.
    const TOKEN: &str = "0123456789abcdef";

    /// The sampler for a window whose shell baud handed [`TOKEN`].
    fn shells_sampler() -> Sampler {
        Sampler::for_shell(Some(&crate::ran::ShellToken::given(TOKEN)))
    }

    /// The shell's prompt with `line` typed and run, and its `C` with
    /// `fields` after the token.
    fn ran(line: &str, fields: &str) -> Vec<u8> {
        format!("\x1b]133;A;aid=1\x07% {line}\r\n\x1b]133;C;baud_token={TOKEN}{fields}\x07")
            .into_bytes()
    }

    /// The command's end and the next prompt, `typed` at it.
    fn prompt_after(typed: &str) -> Vec<u8> {
        format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07% {typed}")
            .into_bytes()
    }

    /// **The process watch's verdict is filed on the reads of the command
    /// it saw a program for, not on the prompt after it**: `git push`, a
    /// command the shell named here, whose ssh the watch names at the
    /// read that printed `/etc/passwd`; the verdict carries to the next
    /// read, the shell's `D` and prompt, where `/etc/hosts` is typed. The
    /// typed path lights and the command's row stays dark. The control: no
    /// verdict, both lit. Named break: file the watch's verdict on the
    /// shell's own read -- the prompt goes dark.
    #[test]
    fn the_watch_files_into_its_commands_rows_not_the_prompt() {
        let (start, end) = (
            ran("git push", ";cmdline=git\\ push"),
            prompt_after("/etc/hosts"),
        );
        let run = |watched: Watched| {
            lit_reads(
                named_groups(),
                &[
                    read(&start, 1),
                    Read {
                        watched,
                        ..read(b"/etc/passwd\r\n", 2)
                    },
                    read(&end, 1),
                ],
            )
        };
        assert_eq!(run(Watched::Nothing), [1, 2], "the control: no verdict");
        assert_eq!(
            run(Watched::Elsewhere),
            [2],
            "the prompt lit, the command dark"
        );
    }

    /// **In one drain, as the macOS pump takes the reads, the prompt after
    /// a gone child rests on the shell's marks**: the child's
    /// `/etc/passwd`, its `D` and the next prompt with `/etc/hosts` typed
    /// share the drain. Under a command the shell named here both light,
    /// the gone child its command's; under a `C` naming nothing, as bash's
    /// with history off, the child's row stays dark -- the residual,
    /// failing closed -- and the prompt, cut from it at the shell's `D`
    /// (`cut.rs`), lights. Named breaks: no lift for a command the shell
    /// named here -- the first case's row 1 goes dark; no cut -- the second
    /// case's prompt goes dark.
    #[test]
    fn in_one_drain_the_prompt_rests_on_the_shells_marks() {
        let end = prompt_after("/etc/hosts");
        let run = |start: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(start, 1),
                    read(b"/etc/passwd\r\n", 7),
                    Read {
                        drain: Drain::Same,
                        ..read(&end, 1)
                    },
                ],
            )
        };
        assert_eq!(run(&ran("cat x", ";cmdline=cat\\ x")), [1, 2], "named here");
        assert_eq!(
            run(&ran("cat x", "")),
            [2],
            "named nothing: the prompt alone"
        );
    }
    /// The shell's `D` for the command before, then its prompt with `line`
    /// typed and run, and its `C` with `fields` after the token.
    fn then_ran(line: &str, fields: &str) -> Vec<u8> {
        let mut b = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        b.extend_from_slice(&ran(line, fields));
        b
    }

    /// **A read inside the pace carries the shell's marks**: most reads ask
    /// for no group, and each still carries what the marks say of it. After
    /// `ssh h`'s `C`, its `/etc/passwd` on the second row of a read that
    /// asked for nothing, a row no read before it reached, is remote; after `ls`'s, the same read is the group last sampled's, the
    /// shell's, and lights. Named break: a read that asks for nothing gets
    /// `Writers::default()` -- the remote row is not known, and the local
    /// one is dark.
    #[test]
    fn a_read_inside_the_pace_carries_the_shells_marks() {
        let run = |line: &str, fields: &str| {
            let start = ran(line, fields);
            taken(
                named_groups(),
                &[read(&start, 1), quiet(b"x\r\n/etc/passwd\r\n")],
            )
        };
        let (lit, origins) = run("ssh h", ";cmdline=ssh\\ h");
        assert_eq!(lit, Vec::<usize>::new(), "ssh's");
        assert_eq!(origins[2], Origin::Remote, "ssh's row: {origins:?}");
        let (lit, origins) = run("ls", ";cmdline=ls");
        assert_eq!(lit, [2], "the control: ls's");
        assert_eq!(origins[2], Origin::Local, "ls's row: {origins:?}");
    }

    /// **R1: a remote program's own `D` ends nothing**: after `ssh h`'s
    /// `C`, the program prints a `D` without the shell's token in the
    /// middle of a read, then `/etc/passwd`, and `/etc/hosts` in the next
    /// read, both asking for no group: every row stays remote, none goes
    /// local, the second read included. Named break: let a `D` with no
    /// token end a command the shell named not local -- the second read is
    /// the shell's, and `/etc/hosts` lights.
    #[test]
    fn a_remote_programs_own_d_ends_nothing() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let (lit, origins) = taken(
            named_groups(),
            &[
                read(&start, 1),
                quiet(b"x\x1b]133;D;0\x07/etc/passwd\r\n"),
                quiet(b"/etc/hosts\r\n"),
            ],
        );
        assert_eq!(lit, Vec::<usize>::new(), "no row lit");
        assert_eq!(origins[1..3], [Origin::Remote; 2], "{origins:?}");
    }

    /// **C1: the line a remote read moved back to stays remote**: under
    /// `git push`, a command the shell named here, the process watch names
    /// the read that writes `aaa` on one line and `bbb /etc/passwd` on the
    /// next, then `CR`, `CSI A` and `LF`, ending at the start of the second
    /// line with its text still on it; the shell's `D` and prompt follow
    /// there, the shell's own read. The line keeps the remote origin and its
    /// path no line. The control: a plain `/etc/passwd` and a line feed,
    /// after which the prompt's line is the shell's, its typed path lit.
    /// Named break: leave the cursor's line out of any read ending in a line
    /// feed -- the remote text's line goes local.
    #[test]
    fn the_line_a_remote_read_moved_back_to_stays_remote() {
        let start = ran("git push", ";cmdline=git\\ push");
        let run = |output: &[u8], end: &[u8]| {
            taken(
                named_groups(),
                &[
                    read(&start, 1),
                    Read {
                        watched: Watched::Elsewhere,
                        ..read(output, 2)
                    },
                    read(end, 1),
                ],
            )
        };
        let (lit, origins) = run(b"aaa\r\nbbb /etc/passwd\r\x1b[A\n", &prompt_after(""));
        assert_eq!(origins[2], Origin::Remote, "{origins:?}");
        assert_eq!(lit, Vec::<usize>::new(), "no line on the remote text");
        let (lit, origins) = run(b"/etc/passwd\r\n", &prompt_after("/etc/hosts"));
        assert_eq!(
            origins[1..3],
            [Origin::Remote, Origin::Local],
            "the control"
        );
        assert_eq!(lit, [2], "the control: the prompt's typed path");
    }

    /// **C2: a drain is filed under its most cautious read, cut at the
    /// shell's `D`**: as the macOS pump takes them, `ssh`'s output, the
    /// shell's `D`, and its prompt in a read of its own, in one drain, make
    /// `ssh`'s rows remote, and the prompt from the `D` on is the shell's
    /// and the samples' (`cut.rs`); and a read under a command naming
    /// nothing, then the shell's next command named here, in one drain,
    /// lift none of the gone writer's rows. Named breaks: file every read
    /// under the drain's last -- the shell's own, and the command named
    /// here -- `ssh`'s row reads local; no cut -- the prompt stays dark.
    #[test]
    fn a_drain_is_filed_under_its_most_cautious_read() {
        let (start, end) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after("/etc/hosts"));
        let a = end
            .windows(7)
            .position(|w| w == b"\x1b]133;A")
            .expect("the prompt's A");
        let (ended, prompt) = end.split_at(a);
        let (lit, origins) = taken(
            named_groups(),
            &[
                read(&start, 1),
                read(b"x\r\n/etc/passwd\r\n", 1),
                Read {
                    drain: Drain::Same,
                    ..read(ended, 1)
                },
                Read {
                    drain: Drain::Same,
                    ..read(prompt, 1)
                },
            ],
        );
        assert_eq!(lit, [3], "the prompt, cut from ssh's drain");
        assert_eq!(
            origins[2..4],
            [Origin::Remote, Origin::Local],
            "{origins:?}"
        );
        let (first, next) = (ran("x", ""), then_ran("cat y", ";cmdline=cat\\ y"));
        let lit = lit_reads(
            named_groups(),
            &[
                read(&first, 1),
                read(b"/etc/passwd\r\n", 7),
                Read {
                    drain: Drain::Same,
                    ..read(&[next.as_slice(), b"/etc/hosts"].concat(), 7)
                },
            ],
        );
        assert_eq!(
            lit,
            Vec::<usize>::new(),
            "a command naming nothing in the drain"
        );
    }

    /// **R4: a command the shell named here lifts only its own rows**: a
    /// command whose `C` named nothing, its child gone before it was named
    /// printing `/etc/passwd`; then `cat y`, named here with the shell's
    /// token, its child gone too printing `/etc/hosts`. `cat`'s row lights,
    /// its writer that command's, and the row before stays dark. Named
    /// break: lift every row a writer not named wrote -- the first row
    /// lights too.
    #[test]
    fn a_command_named_here_lifts_only_its_own_rows() {
        let (first, next) = (ran("x", ""), then_ran("cat y", ";cmdline=cat\\ y"));
        let lit = lit_after(
            named_groups(),
            &[
                (&first, 1),
                (b"/etc/passwd\r\n", 7),
                (&next, 1),
                (b"/etc/hosts", 6),
            ],
        );
        assert_eq!(lit, [3], "cat's row lit, the row before dark");
    }

    /// The click on the link at `row`, column 0, as `paths` judges it.
    fn click_at(t: &Term<'_>, paths: &Paths, row: baud_domain::Row) -> bool {
        let spot = crate::link::Spot {
            scrolled: Scrolled::START,
            back: Rows::ZERO,
            row,
            col: baud_domain::Col::ZERO,
        };
        let under = crate::link::hovered(t, spot).expect("the link");
        crate::link::clicked(
            &under,
            t,
            &crate::link::MachineNames::NONE,
            paths.rules(),
            paths,
        )
        .is_ok()
    }

    /// Who opens the link [`left_open`]'s reads write under.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Opener {
        Remote,
        Shell,
    }

    /// The reads of a remote program that leaves an `OSC 8` link open as it
    /// exits, writing no cell under it, and of the shell's prompt after it,
    /// which writes `/etc/hosts` under the link on row 3; or, the control,
    /// of the shell opening the link itself there.
    fn left_open(opener: Opener) -> [Vec<u8>; 4] {
        let remote = opener == Opener::Remote;
        let open = b"\x1b]8;;file:///etc/passwd\x1b\\".as_slice();
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let exits = match remote {
            true => [open, b"\r\n"].concat(),
            false => b"\r\n".to_vec(),
        };
        let prompt = match remote {
            true => [d.as_slice(), b"/etc/hosts"].concat(),
            false => [d.as_slice(), open, b"/etc/hosts"].concat(),
        };
        [
            ran("ssh h", ";cmdline=ssh\\ h"),
            exits,
            [d.as_slice(), b"\r\n"].concat(),
            prompt,
        ]
    }

    /// **B1: a link a remote program left open is its, whoever writes
    /// under it later**: under `ssh h`, the remote program opens an `OSC 8`
    /// link to `/etc/passwd` and exits writing no cell under it; the shell's
    /// own read then writes `/etc/hosts` under it on row 3, a row filed as
    /// this machine's. The row draws no line and its click is refused: the
    /// read that opened the link is filed against its slot. The control:
    /// the shell opens the same link there itself, lit and opened. Named
    /// break: file no read against its slot -- row 3 lights and opens.
    #[test]
    fn a_link_a_remote_program_left_open_is_its() {
        for opener in [Opener::Remote, Opener::Shell] {
            let reads = left_open(opener);
            let (lit, origins, click) = taken_then(
                named_groups(),
                &[
                    read(&reads[0], 1),
                    read(&reads[1], 9),
                    read(&reads[2], 1),
                    read(&reads[3], 1),
                ],
                None,
                |t, paths| click_at(t, paths, baud_domain::Row::from_index(3)),
            );
            assert_eq!(
                origins[3],
                Origin::Local,
                "row 3 is the shell's: {origins:?}"
            );
            let opens = opener == Opener::Shell;
            assert_eq!((lit.contains(&3), click), (opens, opens), "{opener:?}");
        }
    }

    /// **B1, D1: a link whose opening rows left history stays dark**: as
    /// [`a_link_a_remote_program_left_open_is_its`], with the spans of the
    /// rows above row 3 dropped as history drops them: the slot's record
    /// names rows nothing is filed over now, which read as not known, so
    /// row 3 draws no line and its click is refused. The cost, named: a
    /// link a local program left open past history stays dark too. Named
    /// break: skip the rows nothing is filed over -- row 3 lights and
    /// opens.
    #[test]
    fn a_link_whose_opening_rows_left_history_stays_dark() {
        let reads = left_open(Opener::Remote);
        let (lit, _, click) = taken_then(
            named_groups(),
            &[
                read(&reads[0], 1),
                read(&reads[1], 9),
                read(&reads[2], 1),
                read(&reads[3], 1),
            ],
            Some(3),
            |t, paths| click_at(t, paths, baud_domain::Row::from_index(3)),
        );
        assert_eq!((lit.contains(&3), click), (false, false));
    }

    /// **A link a remote program opened behind a ReGIS delay is its**: as
    /// [`a_link_a_remote_program_left_open_is_its`], with an `S(T1)` delay
    /// before the opener, so the opener is held and fed past the deadline,
    /// a part of its own read, in a drain before the prompt's: row 3 draws
    /// no line and its click is refused. Named break: the pen read only as each part begins
    /// (`Pens::of(pen, pen)`) -- row 3 lights and opens.
    #[test]
    fn a_link_opened_behind_a_delay_is_its_openers() {
        let reads = left_open(Opener::Remote);
        let delayed = [b"\x1bPpS(T1)\x1b\\".as_slice(), &reads[1]].concat();
        let (lit, _, click) = taken_then(
            named_groups(),
            &[
                read(&reads[0], 1),
                read(&delayed, 9),
                read(&reads[2], 1),
                read(b"", 1),
                read(&reads[3], 1),
            ],
            None,
            |t, paths| click_at(t, paths, baud_domain::Row::from_index(3)),
        );
        assert_eq!((lit.contains(&3), click), (false, false));
    }

    /// **A folder report in a read a remote command wrote is not taken**: a
    /// program on the other end of `ssh h` can name this machine as well as
    /// the shell can, so its `OSC 7` naming `/tmp` leaves the folder as the
    /// shell last reported it; the shell's own report after the command's
    /// `D` is taken, though it shares a read with that `D`. Named breaks: take
    /// every report -- the folder is `/tmp` while `ssh` runs; judge a report
    /// by its read's verdict -- the shell's after the `D` is refused.
    #[test]
    fn a_folder_report_from_a_remote_command_is_not_taken() {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let t0 = Instant::now();
        let mut feed = |bytes: &[u8], k: u32| {
            let began = s.line(Rows::ZERO, t.cursor().0);
            use crate::reads::Part;
            let _ = sampler.read(t0 + GROUP_EVERY * k, bytes);
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            // The part's verdict, read with its record (`Writers::recorded`).
            let w = sampler
                .unasked()
                .part(BytePos::ZERO, BytePos::from_index(bytes.len()))
                .recorded(Some(crate::reads::Taken {
                    record: t.take_record(),
                    top: t.history_mark().line(),
                    end_row: crate::vouch::EndRow::of(&t),
                    drew: crate::vouch::Drew::Nothing,
                    recut: None,
                }));
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.read_taken(&mut paths, w, LineSpan::of(began, ended), Linked::No);
            let _ = paths.observe(&t, s);
            paths.folder_now().map(<[u8]>::to_vec)
        };
        let home = Some(b"/home/u".to_vec());
        let shells = b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ";
        assert_eq!(feed(shells, 1), home, "the shell's");
        assert_eq!(feed(&ran("ssh h", ";cmdline=ssh\\ h"), 2), home);
        let forged = b"\x1b]7;file://here.example/tmp\x07remote$ ";
        assert_eq!(feed(forged, 3), home, "the remote program's, not taken");
        let after = [
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/tmp\x07",
        ]
        .concat();
        assert_eq!(feed(&after, 4), Some(b"/tmp".to_vec()), "the shell's again");
    }

    /// **A group seen at two reads apart keeps its two spans**: a writer
    /// gone before it was named wrote row 0 and row 3, and `ls` wrote rows
    /// 1 and 2 between, sampled twice; the gone writer's verdict darkens
    /// its own rows and row 1, whose read it was sampled before, and `ls`'s
    /// `/etc/hosts` on row 2 keeps its line. Named break: join every span
    /// of a group into one -- row 2 goes dark under the gone writer.
    #[test]
    fn a_group_seen_at_two_reads_apart_keeps_two_spans() {
        let lit = lit_after(
            named_groups(),
            &[
                (b"x\r\n", 7),
                (b"y\r\n", 2),
                (b"/etc/hosts\r\n", 2),
                (b"z", 7),
            ],
        );
        assert_eq!(lit, [2], "ls's row between keeps its line");
    }

    /// **B4: a read a ReGIS delay held is filed under its own verdict when
    /// it is fed**: under `ssh h`, a read that writes an `S(T1)` delay and
    /// then `/etc/passwd` on a row of its own, and the shell's `D` in the
    /// read after it, are held; the shell's prompt, in a read past the
    /// deadline, releases them, and `/etc/passwd`'s row is remote, filed
    /// under the reads that wrote it. The control: under `ls`, the same
    /// row lights. Named break: file a call's own reads, held or not, and
    /// no held read when it is fed -- the row is filed under the prompt's
    /// read, the shell's, and lights.
    #[test]
    fn a_held_read_is_filed_under_its_own_verdict() {
        let ended = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let delayed = b"\x1bPpS(T1)\x1b\\\r\n/etc/passwd";
        let prompt = b"\r\n\x1b]133;A;aid=1\x07% ";
        let run = |line: &str, fields: &str| {
            let start = ran(line, fields);
            taken(
                named_groups(),
                &[
                    read(&start, 1),
                    read(delayed, 1),
                    read(&ended, 1),
                    read(prompt, 1),
                ],
            )
        };
        let (lit, origins) = run("ssh h", ";cmdline=ssh\\ h");
        assert_eq!(lit, Vec::<usize>::new(), "ssh's row");
        assert_eq!(origins[2], Origin::Remote, "{origins:?}");
        let (lit, origins) = run("ls", ";cmdline=ls");
        assert_eq!(lit, [2], "the control: ls's row: {origins:?}");
    }

    /// **B4: a forged folder report a ReGIS delay held is judged when it is
    /// fed**: under `ssh h`, a read that delays, reports `/tmp` from the
    /// other end and delays again is fed a part at a time; the shell's `D`,
    /// prompt and own report of `/srv`, held behind it, are filed only once
    /// their bytes go on, so when the forged report reaches the terminal,
    /// at the first deadline, it is the remote program's and not taken, and
    /// the shell's own is taken once fed. Named break: file a call's own
    /// reads, held or not, and no held read when it is fed -- the shell's
    /// report, filed as it arrived, lets the forged one in.
    #[test]
    fn a_forged_report_behind_a_delay_is_judged_when_fed() {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = crate::reads::Pace::new();
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let tagged: Vec<(&[u8], Writers)> = reads
                .iter()
                .map(|b| {
                    let _ = sampler.read(now, b);
                    (*b, sampler.unasked())
                })
                .collect();
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                tagged,
                |_| {},
                |_, _| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            pr.drain_taken(
                &mut paths,
                &t,
                (began, ended),
                counts.core_lines_at(s, &t),
                took.into_iter().map(|w| (w, Linked::No)),
                now,
            );
            let _ = paths.observe(&t, s);
            paths.folder_now().map(<[u8]>::to_vec)
        };
        let home = Some(b"/home/u".to_vec());
        let shells = b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ";
        assert_eq!(call(&[shells], 0), home, "the shell's");
        assert_eq!(call(&[&ran("ssh h", ";cmdline=ssh\\ h")], 10), home);
        let forged =
            b"\x1bPpS(T1)\x1b\\\x1b]7;file://here.example/tmp\x07\x1bPpS(T1)\x1b\\remote$ ";
        assert_eq!(call(&[forged], 20), home, "fed to its first delay");
        let own = [
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/srv\x07",
        ]
        .concat();
        assert_eq!(call(&[&own], 30), home, "held");
        assert_eq!(call(&[], 40), home, "the forged report fed, not taken");
        assert_eq!(call(&[], 60), Some(b"/srv".to_vec()), "the shell's, fed");
    }

    /// **B4: a folder verdict is filed with the part of its read that fed
    /// the report**: under `ssh h`, one read holds the remote program's
    /// forged report of `/tmp`, an `S(T1)` delay, and the shell's `D`, `A`
    /// and own report of `/srv`. The part fed before the delay feeds only
    /// the forged report, refused, and the shell's is taken once the rest
    /// is fed past the deadline. Named break: file the read's last
    /// verdict with its first part -- the forged `/tmp` is read against
    /// until the deadline.
    #[test]
    fn a_report_is_filed_with_the_part_that_fed_it() {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = crate::reads::Pace::new();
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let tagged: Vec<(&[u8], Writers)> = reads
                .iter()
                .map(|b| {
                    let _ = sampler.read(now, b);
                    (*b, sampler.unasked())
                })
                .collect();
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                tagged,
                |_| {},
                |_, _| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            pr.drain_taken(
                &mut paths,
                &t,
                (began, ended),
                counts.core_lines_at(s, &t),
                took.into_iter().map(|w| (w, Linked::No)),
                now,
            );
            let _ = paths.observe(&t, s);
            paths.folder_now().map(<[u8]>::to_vec)
        };
        let home = Some(b"/home/u".to_vec());
        let shells = b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ";
        assert_eq!(call(&[shells], 0), home, "the shell's");
        assert_eq!(call(&[&ran("ssh h", ";cmdline=ssh\\ h")], 10), home);
        let one = [
            b"\x1b]7;file://here.example/tmp\x07\x1bPpS(T1)\x1b\\".as_slice(),
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/srv\x07",
        ]
        .concat();
        assert_eq!(call(&[&one], 20), home, "the forged report fed, not taken");
        assert_eq!(call(&[], 30), home, "the rest held");
        assert_eq!(call(&[], 60), Some(b"/srv".to_vec()), "the shell's, fed");
    }

    /// **B4: a read under `ESC % @` holding 0x9D refuses every part it
    /// feeds**: an `OSC` begun with 0x9D is one no scan places, so the
    /// remote program's forged 8-bit report of `/tmp`, fed before an `S(T1)`
    /// delay, is refused, and so is the rest of that read once fed, the
    /// shell's own report in it included. Named break: such a read files
    /// only the reports the 7-bit scan placed -- the part fed before the
    /// delay files nothing, and `/tmp` is read against.
    #[test]
    fn a_read_holding_an_8_bit_osc_refuses_every_part() {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = crate::reads::Pace::new();
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let tagged: Vec<(&[u8], Writers)> = reads
                .iter()
                .map(|b| {
                    let _ = sampler.read(now, b);
                    (*b, sampler.unasked())
                })
                .collect();
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                tagged,
                |_| {},
                |_, _| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            pr.drain_taken(
                &mut paths,
                &t,
                (began, ended),
                counts.core_lines_at(s, &t),
                took.into_iter().map(|w| (w, Linked::No)),
                now,
            );
            let _ = paths.observe(&t, s);
            paths.folder_now().map(<[u8]>::to_vec)
        };
        let home = Some(b"/home/u".to_vec());
        let shells = b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ";
        assert_eq!(call(&[shells], 0), home, "the shell's");
        assert_eq!(call(&[&ran("ssh h", ";cmdline=ssh\\ h")], 10), home);
        let one = [
            b"\x1b%@\x9d7;file://here.example/tmp\x9c\x1bPpS(T1)\x1b\\".as_slice(),
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/srv\x07",
        ]
        .concat();
        assert_eq!(call(&[&one], 20), home, "the forged report fed, refused");
        assert_eq!(call(&[], 30), home, "the rest held");
        assert_eq!(call(&[], 60), home, "the rest fed, refused");
    }

    /// **A read any byte of which ran under `ESC % @` and holding 0x9D is
    /// refused, though it began and ended under UTF-8**: under `ssh h`, one
    /// read selects ISO 8859-1, forges an 8-bit report of `/tmp`, selects
    /// UTF-8 again, and carries the shell's `D` and prompt. The folder
    /// stays the shell's. Named break: judge the encoding only as the read
    /// began and ended -- `/tmp` is taken.
    #[test]
    fn a_read_that_ran_under_latin_1_between_is_refused() {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut pace = crate::reads::Pace::new();
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let t0 = Instant::now();
        let mut call = |read: &[u8], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let _ = sampler.read(now, read);
            let tagged = [(read, sampler.unasked())];
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                tagged,
                |_| {},
                |_, _| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            pr.drain_taken(
                &mut paths,
                &t,
                (began, ended),
                counts.core_lines_at(s, &t),
                took.into_iter().map(|w| (w, Linked::No)),
                now,
            );
            let _ = paths.observe(&t, s);
            paths.folder_now().map(<[u8]>::to_vec)
        };
        let home = Some(b"/home/u".to_vec());
        let shells = b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ";
        assert_eq!(call(shells, 0), home, "the shell's");
        assert_eq!(call(&ran("ssh h", ";cmdline=ssh\\ h"), 10), home);
        let one = [
            b"\x1b%@\x9d7;file://here.example/tmp\x9c\x1b%G".as_slice(),
            prompt_after("").as_slice(),
        ]
        .concat();
        assert_eq!(call(&one, 20), home, "the forged report refused");
    }

    /// A screen of 40 columns and 8 rows, and one of 80, the tour's width.
    const TALL: WinSize = WinSize {
        ws_col: 40,
        ws_row: 8,
    };
    const WIDE: WinSize = WinSize {
        ws_col: 80,
        ws_row: 8,
    };

    /// zsh's default `PROMPT_SP` run with `blanks` blanks, as zsh writes it.
    fn prompt_sp(blanks: ReadLen) -> Vec<u8> {
        [
            b"\x1b[1m\x1b[7m%\x1b[27m\x1b[1m\x1b[0m".as_slice(),
            &b" ".repeat(blanks.index()),
            b"\r \r",
        ]
        .concat()
    }

    /// Whether the reads before a typed path are fed in drains of their
    /// own, or the last two in one, as the macOS pump takes them.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Drains {
        Each,
        LastTwo,
    }

    /// **A frame at `now`**, as the app draws one each tick: the lines at
    /// rest scanned, the worker's turn, and its answers taken; whether a
    /// batch went.
    fn frame(
        pr: &mut Probes<Instant>,
        t: &Term<'_>,
        lines: &mut Lines,
        uris: &mut Uris,
        paths: &mut Paths,
        now: Instant,
    ) -> Owed {
        let s = Scrolled::START;
        let mut sent = Owed::No;
        loop {
            let _ = lines.scan(
                t,
                s,
                &mut Links {
                    uris,
                    urls: Turned::On,
                    paths,
                },
            );
            let seen = Seen {
                term: t,
                scrolled: s,
                lines,
                uris,
                urls: Turned::On,
                paths,
            };
            if pr.turn(now, seen, || Some(1)).frame == Owed::Yes {
                sent = Owed::Yes;
            }
            if pr.flight.is_none() {
                return sent;
            }
            std::thread::yield_now();
        }
    }

    /// **A path typed at the prompt after `ssh` exits**, as the app files
    /// it: the shell's `C` for `ssh h`, ssh's output with ssh in front,
    /// `exit`'s `logout` and the shell's tokened `D`, its folder report and
    /// prompt arriving within the pace, then `./oct-commits.txt` typed and
    /// echoed one key a read, 100 ms apart, the shell in front, a frame
    /// after each drain as the app draws one; the rows lit by the first
    /// batch the pace sends after the last key -- [`SETTLE`] after it, or
    /// [`CEILING`] after the batch before -- and its answer.
    fn typed_after_ssh(drains: Drains) -> Vec<usize> {
        let mut f = Fake::with(&[
            ("/home", Entry::Dir),
            ("/home/u", Entry::Dir),
            ("/home/u/oct-commits.txt", Entry::File),
        ]);
        f.groups.insert(1, vec!["zsh".into()]);
        f.groups.insert(9, vec!["ssh".into()]);
        let prompt = [
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/home/u\x07",
        ]
        .concat();
        let mut batches: Vec<Vec<(Duration, Vec<u8>, Pid)>> = vec![
            vec![(
                Duration::from_millis(0),
                ran("ssh h", ";cmdline=ssh\\ h"),
                1,
            )],
            vec![(Duration::from_millis(50), b"remote$ exit\r\n".to_vec(), 9)],
        ];
        match drains {
            Drains::Each => {
                batches.push(vec![(Duration::from_millis(55), b"logout\r\n".to_vec(), 9)]);
                batches.push(vec![(Duration::from_millis(58), prompt, 1)]);
            }
            Drains::LastTwo => {
                batches.push(vec![
                    (Duration::from_millis(55), b"logout\r\n".to_vec(), 9),
                    (Duration::from_millis(58), prompt, 1),
                ]);
            }
        }
        for (k, b) in b"./oct-commits.txt".iter().enumerate() {
            let at = 200 + 100 * u64::try_from(k).expect("a key");
            batches.push(vec![(Duration::from_millis(at), vec![*b], 1)]);
        }
        lit_on_a_timeline(f, &batches)
    }

    /// **The rows lit once `batches` are fed as the drivers feed them**:
    /// each batch a drain, each read `(ms, bytes, group in front)` at its
    /// time, asking by the pace, a frame after each drain, then frames 10
    /// ms apart to the first batch the pace sends and the frame that takes
    /// its answer ([`typed_after_ssh`]'s run).
    fn lit_on_a_timeline(f: Fake, batches: &[Vec<(Duration, Vec<u8>, Pid)>]) -> Vec<usize> {
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let mut sampler = shells_sampler();
        let mut pace = crate::reads::Pace::new();
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let t0 = Instant::now();
        let mut last = t0;
        for batch in batches {
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let mut tagged = Vec::new();
            for (after, bytes, front) in batch {
                let now = t0 + *after;
                last = now;
                let w = match sampler.read(now, bytes) {
                    Asks::Yes(_) => sampler.writers(Some(*front), Watched::Nothing),
                    Asks::No => sampler.unasked(),
                };
                tagged.push((bytes.as_slice(), w));
            }
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                last,
                tagged,
                |_| {},
                |_, _| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let rec = took.iter().rev().find_map(|w| w.record);
            let _ = counts.numbered(&t, rec.as_ref());
            pr.drain_taken(
                &mut paths,
                &t,
                (began, ended),
                counts.core_lines_at(s, &t),
                took.into_iter().map(|w| (w, Linked::No)),
                last,
            );
            let _ = paths.observe(&t, s);
            let _ = frame(&mut pr, &t, &mut lines, &mut uris, &mut paths, last);
        }
        // A frame each 10 ms after the last key, to the first batch the
        // pace sends, and the frame after it, which takes its answer.
        let mut k = 0u32;
        let before = pr.counts.batches;
        loop {
            k += 1;
            let now = last + Duration::from_millis(10) * k;
            let owed = frame(&mut pr, &t, &mut lines, &mut uris, &mut paths, now);
            if owed == Owed::Yes || pr.counts.batches > before {
                let now = now + Duration::from_millis(10);
                let _ = frame(&mut pr, &t, &mut lines, &mut uris, &mut paths, now);
                break;
            }
            assert!(k < 100, "no batch within a second of the last key");
        }
        let _ = lines.scan(
            &t,
            Scrolled::START,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let mut rows: Vec<usize> = lines.lines().iter().map(|l| l.row.index()).collect();
        rows.dedup();
        rows
    }

    /// **The read that carries the shell's `D` keeps the group before and
    /// the watch's verdict for its own rows**: a command wrapping ssh --
    /// `make deploy`, named here, or one naming nothing, with ssh sampled
    /// in front at the read before; or `make deploy` with the watch naming
    /// ssh at that read -- whose last line, `/etc/passwd`, comes in one read
    /// with the shell's `D` and its prompt. The line before the `D` stays
    /// dark and the prompt's typed path lights. Named break: forget the
    /// group before and the watch's verdict at the `D` as the read is
    /// scanned, before its writers are built -- the line before the `D`
    /// lights.
    #[test]
    fn the_d_reads_own_rows_keep_the_group_before() {
        let tail = [b"/etc/passwd\r\n".as_slice(), &prompt_after("/etc/hosts")].concat();
        let here = ran("make deploy", ";cmdline=make\\ deploy");
        let unnamed = ran("x", "");
        let ssh = |wrapper: &[u8]| {
            lit_reads(
                named_groups(),
                &[read(wrapper, 1), read(b"out\r\n", 9), read(&tail, 1)],
            )
        };
        assert_eq!(ssh(&here), [3], "a wrapper named here");
        assert_eq!(ssh(&unnamed), [3], "a wrapper naming nothing");
        let watched = lit_reads(
            named_groups(),
            &[
                read(&here, 1),
                Read {
                    watched: Watched::Elsewhere,
                    ..read(b"out\r\n", 2)
                },
                read(&tail, 1),
            ],
        );
        assert_eq!(watched, [3], "the watch naming ssh");
    }

    /// **The cut's red 1: a path typed after `ssh` exits lights** by the
    /// first batch after the last key and its answer ([`typed_after_ssh`]): the read carrying
    /// the shell's `D` is cut there, and the prompt from it is the shell's,
    /// whether `logout` came in a drain of its own or in the `D`'s; and the
    /// shell's `D` ends the command the group sampled before was taken
    /// under. Named breaks: no cut (`cut::Plan::of` none) -- the typed path
    /// stays dark; the sample before kept across the shell's `D` -- the
    /// first key's read is ssh's, and the path stays dark.
    #[test]
    fn a_path_typed_after_ssh_exits_lights() {
        assert_eq!(typed_after_ssh(Drains::Each), [3], "a drain a read");
        assert_eq!(
            typed_after_ssh(Drains::LastTwo),
            [3],
            "logout and the D in one"
        );
    }

    /// **The cut's red 2: a remote link below the shell's `D` stays dark**:
    /// under `ssh h`, the remote program prints `/etc/passwd` a row down and
    /// moves back up; then the shell's `D`, `A` and a prompt of two lines,
    /// its line feeds passing over the link's row. In a read before the
    /// `D`'s, the lines below the cut keep the uncut verdict, ssh's; in the
    /// `D`'s own read, the move up refuses the cut. Named breaks: the lines
    /// below the cut filed under the shell's verdict -- the earlier read's
    /// link lights; that and the cut taken whatever the bytes before the
    /// `D` -- the same read's link lights too.
    #[test]
    fn a_remote_link_below_the_shells_d_stays_dark() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let remote: &[u8] = b"x\r\n/etc/passwd\r\n\x1b[2A";
        let prompt = [prompt_after("").as_slice(), b"\r\n\r\n> "].concat();
        let (lit, origins) = taken_on(
            TALL,
            named_groups(),
            &[read(&start, 1), read(remote, 9), read(&prompt, 1)],
        );
        assert!(!lit.contains(&2), "the earlier read's link: {lit:?}");
        assert_ne!(origins[2], Origin::Local, "{origins:?}");
        let one = [remote, prompt.as_slice()].concat();
        let (lit, origins) = taken_on(TALL, named_groups(), &[read(&start, 1), read(&one, 9)]);
        assert!(!lit.contains(&2), "the same read's link: {lit:?}");
        assert_ne!(origins[2], Origin::Local, "{origins:?}");
    }

    /// The tour's exit, as run9 recorded it: the remote shell's tail --
    /// `exit`, its own `C`, `logout`, an iTerm badge reset -- then the local
    /// zsh's `PROMPT_SP` run of 79 blanks, its `D`, carrying the token as
    /// the integration sends it now, and its prompt, `/etc/hosts` typed.
    fn tour_exit() -> (Vec<u8>, Vec<u8>) {
        let tail = b"exit\r\n\x1b]133;C\x07logout\r\n\x1b]1337;SetBadgeFormat=\x07".to_vec();
        let prompt = format!(
            "\x1b]133;D;0;aid=2688;baud_token={TOKEN}\x07\x1b]133;A;aid=2688\x07\r\x1b[0m\
             \x1b[27m\x1b[24m\x1b[J\x1b]133;P;k=i\x07~/baud \x1b[33m25s\x1b[0m \
             \x1b[38;2;244;163;44m\u{276f}\x1b[0m \x1b]133;B\x07/etc/hosts"
        );
        let local = [
            prompt_sp(ReadLen::from_index(79)).as_slice(),
            prompt.as_bytes(),
        ]
        .concat();
        (tail, local)
    }

    /// **The cut's red 4: the tour's prompt after `ssh demo` exits lights**,
    /// on run9's bytes: the remote tail and the local shell's read in one
    /// drain, zsh's run beginning at the first column with the columns less
    /// one of blanks, so the cut moves to it; and the same bytes in one
    /// read, where the badge's `OSC 1337` refuses the cut, failing closed.
    /// Named break: no `PROMPT_SP` exception -- the prompt stays dark.
    #[test]
    fn the_tours_prompt_after_ssh_exits_lights() {
        let start = ran("ssh demo", ";cmdline=ssh\\ demo");
        let (tail, local) = tour_exit();
        let (lit, origins) = taken_on(
            WIDE,
            named_groups(),
            &[
                read(&start, 1),
                read(&tail, 9),
                Read {
                    drain: Drain::Same,
                    ..read(&local, 1)
                },
            ],
        );
        assert_eq!(lit, [3], "the prompt's typed path: {origins:?}");
        let one = [tail, local].concat();
        let (lit, _) = taken_on(WIDE, named_groups(), &[read(&start, 1), read(&one, 9)]);
        assert_eq!(lit, Vec::<usize>::new(), "one read: the badge refuses");
    }

    /// **Text no read filed before zsh's run keeps its row dark**: under
    /// `ssh h`, with autowrap off, `x /etc/passwd ` on the screen with no
    /// span filed over it, the cursor after it; then zsh's run, mid-row,
    /// its `D` and a prompt: the run moves nothing, and the row, the run's
    /// bytes on it, stays ssh's. Named break: the run taken with the
    /// cursor anywhere -- the text lights.
    #[test]
    fn text_before_a_run_begun_mid_row_stays_dark() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let local = [prompt_sp(ReadLen::from_index(39)), prompt_after("")].concat();
        let (lit, origins) = taken_on(
            TALL,
            named_groups(),
            &[
                read(&start, 1),
                Read {
                    sampled: Sampled::Unfiled,
                    ..read(b"\x1b[?7lx /etc/passwd ", 9)
                },
                read(&local, 1),
            ],
        );
        assert_eq!(lit, Vec::<usize>::new(), "{origins:?}");
    }

    /// **A run short of the columns less one moves nothing**: under `ssh
    /// h`, `/etc/passwd` at column 28 with no span filed over it and the
    /// cursor back at the first column; zsh's run of 20 blanks leaves the
    /// text, and the row stays ssh's; the control, a run of 39, blanks the
    /// whole row and the prompt's typed path lights. Named break: the run
    /// taken whatever its blanks -- the text lights.
    #[test]
    fn a_short_run_moves_nothing() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let text = [b" ".repeat(28).as_slice(), b"/etc/passwd\r"].concat();
        let run = |blanks: usize| {
            let local = [
                prompt_sp(ReadLen::from_index(blanks)),
                prompt_after("/etc/hosts"),
            ]
            .concat();
            taken_on(
                TALL,
                named_groups(),
                &[
                    read(&start, 1),
                    Read {
                        sampled: Sampled::Unfiled,
                        ..read(&text, 9)
                    },
                    read(&local, 1),
                ],
            )
        };
        let (lit, origins) = run(20);
        assert_eq!(lit, Vec::<usize>::new(), "short: {origins:?}");
        let (lit, origins) = run(39);
        assert_eq!(lit, [1], "the control: {origins:?}");
    }

    /// **The link layer's stages still to land, each clearing the pending
    /// rows that name it**: the core's record of what each read wrote
    /// (the core-record commit) closes [`LinkStage::CoreRecord`].
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum LinkStage {
        CoreRecord,
    }

    /// The stages closed: the commit that closes one adds it here and turns
    /// every row pending on it green, or the row goes red.
    const CLOSED: &[LinkStage] = &[LinkStage::CoreRecord];

    /// **A pending row's verdict**: while `stage` is open, `now` must fail
    /// exactly as `recorded`; red if it is `fixed` early, fails another
    /// way, or is still here once its stage has closed.
    #[allow(dead_code, reason = "every stage is closed: no row is pending")]
    fn pending<T: PartialEq + core::fmt::Debug>(stage: LinkStage, now: T, recorded: T, fixed: T) {
        verdict(CLOSED, stage, now, recorded, fixed);
    }

    /// [`pending`] with `closed` the stages closed.
    fn verdict<T: PartialEq + core::fmt::Debug>(
        closed: &[LinkStage],
        stage: LinkStage,
        now: T,
        recorded: T,
        fixed: T,
    ) {
        assert!(
            !closed.contains(&stage),
            "{stage:?} closed with this row pending: make it its green row"
        );
        assert_ne!(
            now, fixed,
            "fixed while {stage:?} is pending: make it a green row"
        );
        assert_eq!(now, recorded, "pending on {stage:?}, failing another way");
    }

    /// Under `ssh h`, a remote read that writes `/etc/passwd` a row down and
    /// moves back up, so its span ends where it began; `logout` and its
    /// line feed; and the shell's `D` and a bash prompt.
    fn below_and_back() -> [Vec<u8>; 4] {
        [
            ran("ssh h", ";cmdline=ssh\\ h"),
            [
                b"$ \r\n".as_slice(),
                &b" ".repeat(20),
                b"/etc/passwd\x1b[A\r",
            ]
            .concat(),
            b"logout\r\n".to_vec(),
            format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07$ ").into_bytes(),
        ]
    }

    /// **A read is filed on the primary before any record has ended there**
    /// (`HistoryMark::line`, taken with each part's record): `ls` prints
    /// `/etc/hosts` on row 0 and a program it starts switches to the
    /// alternate screen in the same read, and leaves it in the next. Row 0
    /// lights. Named break: place no primary line until a record ends on
    /// the primary, as a top worked back from where a record's cursor
    /// ended did -- row 0 stays dark.
    #[test]
    fn a_read_is_filed_on_the_primary_before_a_record_ends_there() {
        let reads = [
            read(b"/etc/hosts\r\n\x1b[?1049h", 2),
            read(b"\x1b[?1049l", 2),
        ];
        assert_eq!(lit_driven(TALL, named_groups(), &reads), [0]);
    }

    /// **A row a remote read reached below its span, and left, stays dark
    /// under the shell's prompt that moves onto it, by the core's record of
    /// the lowest line the read reached alone** (F5, E5's `sank`): under
    /// `ssh h`, a read that feeds a line down, writes `/etc/passwd` there,
    /// moves back up and ends writing `bye` on the row it began on -- its
    /// last write and its cursor above the path's row; the shell's `D` in a
    /// read of its own; then a prompt read that feeds a line onto the
    /// path's row and draws `$ ` there. Row 2 stays dark. Every other layer
    /// is off, each shown by a control in this test: the remote's own span
    /// ends above the row, and the prompt read is the shell's alone -- no
    /// group sampled before it and no cut -- so the same session, its
    /// remote read writing nothing below, lights `/etc/hosts` typed at the
    /// prompt on that row; and the same bytes under a command named here
    /// light the path. Named break: no record at the climb (the core's
    /// `record_climb` a no-op) -- row 2 lights.
    #[test]
    fn a_row_a_remote_read_reached_below_is_dark_by_its_record_alone() {
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let below = [
            b"\r\n".as_slice(),
            &b" ".repeat(20),
            b"/etc/passwd\x1b[A\rbye",
        ]
        .concat();
        let prompt = |typed: &str| format!("\r\n\x1b]133;A;aid=1\x07$ {typed}").into_bytes();
        let run = |start: &[u8], remote: &[u8], g: Pid, prompt: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(start, 1),
                    read(remote, g),
                    read(&d, 1),
                    read(prompt, 1),
                ],
            )
        };
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        assert_eq!(
            run(&ssh, b"bye", 9, &prompt("/etc/hosts")),
            [2],
            "the control: the prompt read is the shell's alone"
        );
        assert_eq!(
            run(&ran("cat x", ";cmdline=cat\\ x"), &below, 2, &prompt("")),
            [2],
            "the control: written here, the path lights"
        );
        assert_eq!(run(&ssh, &below, 9, &prompt("")), Vec::<usize>::new());
    }

    /// **A row a remote read wrote below its span, coming back up after,
    /// stays dark under the prompt that lands there** (F5,
    /// [`below_and_back`]): in two shapes, `logout` in ssh's read and the
    /// prompt in a read of its own; and `logout`, the `D` and the prompt in
    /// one read, behind the cut. A paired-layer row: two layers each hold
    /// it alone. The remote read's record of the lowest line it reached
    /// (the core's `sank`) files it over the row below; and `logout`'s line
    /// feed ends its read on that row, which holds the path, so its record
    /// vouches nothing clean there (`vouch::Below`, its end row) and the
    /// row is that read's. Named break: both -- no record at the climb (the
    /// core's `record_climb` a no-op) and the end line vouched clean
    /// whatever its row holds -- row 2 lights in both shapes; either alone
    /// leaves it dark. Each layer is pinned alone elsewhere:
    /// [`a_row_a_remote_read_reached_below_is_dark_by_its_record_alone`]
    /// and `read_record.rs`'s climb tests for the record,
    /// [`text_after_a_remote_reads_last_line_feed_stays_the_reads`] for the
    /// end row.
    #[test]
    fn a_row_written_below_a_remote_reads_span_stays_dark() {
        let [start, remote, logout, prompt] = below_and_back();
        let exit = [logout.as_slice(), &prompt].concat();
        let own = lit_reads(
            named_groups(),
            &[
                read(&start, 1),
                read(&remote, 9),
                read(&logout, 9),
                read(&prompt, 1),
            ],
        );
        assert_eq!(own, Vec::<usize>::new(), "the prompt in a read of its own");
        let cut = lit_reads(
            named_groups(),
            &[read(&start, 1), read(&remote, 9), read(&exit, 1)],
        );
        assert_eq!(cut, Vec::<usize>::new(), "the prompt behind the cut");
    }

    /// **A read that leaves the alternate screen is filed on the lines it
    /// wrote there** (N1): under `ssh`, a remote read switches to the
    /// alternate screen (`?47h`, which keeps the cursor on row 1), and the
    /// next writes `/etc/passwd` on its row 1 and switches back (`?47l`) in
    /// the same read; the shell then shows the alternate screen again, its
    /// text kept, and moves to the top. Row 1 stays dark. Named break: file
    /// a read that ends on another screen than it began on nowhere, as one
    /// span from line to line could not -- row 1 lights.
    #[test]
    fn a_read_that_leaves_the_alternate_screen_files_what_it_wrote_there() {
        let reads = [
            read(b"$ ssh h\r\n", 1),
            read(b"\x1b[?47h", 9),
            read(b"/etc/passwd\r\n\x1b[?47l", 9),
            read(b"\x1b[?47h", 1),
            read(b"\x1b[1;1H", 1),
        ];
        assert_eq!(
            lit_driven(TALL, named_groups(), &reads),
            Vec::<usize>::new()
        );
    }

    /// **After a full-screen program that scrolled, the primary's rows are
    /// looked up on their own lines** (H1): `/etc/passwd` printed under
    /// `ssh`, then a local `less` on the alternate screen scrolls it two
    /// lines and quits, and the shell's prompt; row 0 stays dark, numbered
    /// as the drivers number it (`keep::Counts`). So it does with two local
    /// lines under it before `less`, the second the one two lines down.
    /// Named break: the primary's count moved by the alternate screen's
    /// scrolling too, as one count for both screens was, and never moved
    /// back -- the second shape's row 0 is looked up two lines down, on
    /// ` x`, and lights.
    #[test]
    fn a_remote_row_after_a_scrolled_pager_stays_dark() {
        let groups = || {
            let mut f = named_groups();
            f.groups.insert(3, vec!["less".into()]);
            f
        };
        let pager = [
            read(b"\x1b[?1049h", 3),
            read(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\n", 3),
            read(b"\x1b[?1049l", 3),
            read(b"$ ", 1),
        ];
        let under = [
            read(b"/etc/passwd\r\n", 9),
            read(b"$ ls\r\n", 1),
            read(b"$ less x\r\n", 1),
        ];
        let reads = [under.as_slice(), &pager].concat();
        assert_eq!(
            lit_driven(TALL, groups(), &reads),
            Vec::<usize>::new(),
            "two local lines under it"
        );
        let reads = [
            read(b"/etc/passwd\r\n", 9),
            read(b"$ less x\r\n", 1),
            read(b"\x1b[?1049h", 3),
            read(b"a\r\nb\r\nc\r\nd\r\ne\r\nf\r\ng\r\n", 3),
            read(b"\x1b[?1049l", 3),
            read(b"$ ", 1),
        ];
        let (held, _, ()) =
            taken_numbered(TALL, groups(), &reads, None, Numbering::Held, |_, _| ());
        assert_eq!(
            held,
            Vec::<usize>::new(),
            "the control: a count that does not drift"
        );
        let lit = lit_driven(TALL, groups(), &reads);
        assert_eq!(lit, Vec::<usize>::new(), "row 0 after the pager");
    }

    /// **A clear of the alternate screen takes its facts, and an answer
    /// about the text it took is filed nowhere**: a local `less` (group 3)
    /// writes `/etc/passwd` on the alternate screen's row 2, its group's
    /// answer still to come; it leaves and enters again (`?1049l ?1049h`,
    /// which clears the screen and starts its count again); then text no
    /// read filed lands on row 2, and the answer naming group 3 local
    /// arrives -- the row stays dark, though its number is the one the
    /// answer's span names. The control: no clear, the cursor moved
    /// instead, and the row keeps its local writer. Named breaks: no clear
    /// filed (`Paths::alternate_cleared` not called) -- the row lights; an
    /// answer of an earlier epoch filed in the store as it is now
    /// (`Paths::store_mut` reading no epoch) -- the row lights.
    #[test]
    fn an_answer_about_alternate_text_a_clear_took_is_filed_nowhere() {
        let groups = || {
            let mut f = named_groups();
            f.groups.insert(3, vec!["less".into()]);
            f
        };
        let then = |between: &'static [u8]| {
            let reads = [
                read(b"$ less x\r\n", 1),
                read(b"\x1b[?1049h", 3),
                read(b"\x1b[3;1H/etc/passwd", 3),
                read(between, 3),
                Read {
                    sampled: Sampled::Unfiled,
                    ..read(b"\x1b[3;1H/etc/passwd", 9)
                },
            ];
            lit_driven(TALL, groups(), &reads)
        };
        assert_eq!(then(b"\x1b[1;1H"), [2], "the control: no clear");
        assert_eq!(
            then(b"\x1b[?1049l\x1b[?1049h"),
            Vec::<usize>::new(),
            "after a clear"
        );
    }

    /// **Each alternate-screen row is looked up by its own text across a
    /// change of height**: a local `less` (group 3) writes `/etc/passwd` on
    /// row 3, `ssh` (group 9) writes `/etc/hosts` on row 4 and puts the
    /// cursor on the bottom row; the window shrinks to five rows, then grows
    /// back to eight. Both rows fit five whether the core cuts the bottom
    /// rows or scrolls the top ones away, and whichever it does, the rows
    /// lit are the rows holding the local text: the resize's record moves
    /// the alternate screen's count (`keep::kept_across_resize`), with no
    /// read after the resize to move it. The core cuts the bottom and moves
    /// no row today, so the named break -- the counts moved at the resize
    /// with no record (`Counts::numbered(term, None)`) -- turns this red only
    /// under a core that scrolls the top rows away instead: perturbed so,
    /// the remote row is looked up on the number the local one had, and
    /// lights.
    #[test]
    fn alternate_rows_are_looked_up_by_their_text_across_a_change_of_height() {
        let groups = || {
            let mut f = named_groups();
            f.groups.insert(3, vec!["less".into()]);
            f
        };
        let reads = [
            read(b"$ less x\r\n", 1),
            read(b"\x1b[?1049h", 3),
            read(b"\x1b[4;1H", 3),
            read(b"/etc/passwd\r\n", 3),
            Read {
                then: Then::Resize(40, 5),
                ..read(b"/etc/hosts\x1b[8;1H", 9)
            },
            // No byte: nothing but the resize moves the count.
            Read {
                then: Then::Resize(40, 8),
                ..read(b"", 9)
            },
        ];
        let holding = |t: &Term<'_>, what: &str| -> Vec<usize> {
            (0..t.geometry().1.index())
                .filter(|&y| {
                    let row = t.row(baud_domain::Row::from_index(y)).expect("a row");
                    let text: String = row
                        .iter()
                        .filter_map(|c| c.content.as_codepoint())
                        .filter_map(|c| char::from_u32(c.value()))
                        .collect();
                    text.contains(what)
                })
                .collect()
        };
        let (lit, _, (local, remote)) =
            taken_numbered(TALL, groups(), &reads, None, Numbering::Driven, |t, _| {
                (holding(t, "/etc/passwd"), holding(t, "/etc/hosts"))
            });
        assert!(
            !local.is_empty() && !remote.is_empty(),
            "both texts on the screen: {local:?} {remote:?}"
        );
        assert_eq!(lit, local, "the local rows, and only they, light");
    }

    /// **Text a later local program moves keeps the writer of the row it
    /// came from** (H2): `ssh h` run, and `/etc/passwd` printed under it on
    /// row 1; it ends, and the shell's prompt follows on row 2, the shell's
    /// by its marks; then a local program inserts a line at row 1 (`CSI
    /// L`), which moves the remote text onto row 2, a row only the shell
    /// wrote. The row stays dark: the line it landed on takes the most
    /// cautious writer of the lines its text came from (`Paths::carried`).
    /// Named break: carry nothing -- row 2 lights.
    #[test]
    fn remote_text_moved_onto_a_local_row_stays_dark() {
        let (start, prompt) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after(""));
        let moved = |local: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(&start, 1),
                    read(b"/etc/passwd\r\n", 9),
                    read(&prompt, 1),
                    read(local, 2),
                ],
            )
        };
        assert_eq!(
            moved(b"\x1b[2;1H"),
            Vec::<usize>::new(),
            "the control: nothing moved"
        );
        assert_eq!(moved(b"\x1b[2;1H\x1b[L"), Vec::<usize>::new(), "moved");
    }

    /// **Each line a read's moves wrote is filed from the lines as they
    /// stood before the read** (C2, P4). Under `IL`: `ssh h`'s
    /// `/etc/passwd` on row 1, the shell's `% ls` on row 2 and `ls`'s
    /// `/etc/hosts` on row 3; a local `IL` at row 1 moves each down a row,
    /// and `/etc/hosts` lights on row 4, its text and the row's own both
    /// local before the read. Under `DL`, the mirror: `/etc/hosts` on row
    /// 1, `% ssh h` on row 2 and `/etc/passwd` on row 3, and a `DL` at row
    /// 0 moves each up a row, `/etc/hosts` lighting on row 0. A local
    /// region over rows 1-5 and an `SD` in it, the cursor below it, lights
    /// the path on row 4 too, and the same `SD` by `ssh` leaves it dark.
    /// Named break: file each line as it is found, so a line the move
    /// filled is read as the next one's source with what the move put
    /// there -- the path goes dark under `IL` walked down or under `DL`
    /// walked up.
    #[test]
    fn moved_lines_are_filed_from_the_lines_before_the_read() {
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let after = |line: &str, fields: &str| [d.as_slice(), &ran(line, fields)].concat();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let ls = ran("ls", ";cmdline=ls");
        let ssh_after = after("ssh h", ";cmdline=ssh\\ h");
        let ls_after = after("ls", ";cmdline=ls");
        let prompt = prompt_after("");
        let by = |reads: [(&[u8], Pid); 4], (set, edit): (&[u8], &[u8]), group: Pid| {
            let mut all: Vec<_> = reads.iter().map(|&(b, g)| read(b, g)).collect();
            all.push(read(&prompt, 1));
            all.push(Read {
                then: Then::Answered,
                ..read(set, 2)
            });
            all.push(read(edit, group));
            lit_driven(TALL, named_groups(), &all)
        };
        let at = |reads, edit| by(reads, (b"", edit), 2);
        let il = [
            (ssh.as_slice(), 1),
            (b"/etc/passwd\r\n".as_slice(), 9),
            (ls_after.as_slice(), 1),
            (b"/etc/hosts\r\n".as_slice(), 2),
        ];
        assert_eq!(at(il, b"\x1b[9;1H"), [3], "the control: nothing moved");
        assert_eq!(at(il, b"\x1b[2;1H\x1b[L"), [4], "IL");
        let region = b"\x1b[1;5r\x1b[6;1H".as_slice();
        assert_eq!(by(il, (region, b"\x1b[T"), 2), [4], "SD in a region");
        assert_eq!(
            by(il, (region, b"\x1b[T"), 9),
            Vec::<usize>::new(),
            "SD in a region by ssh"
        );
        let dl = [
            (ls.as_slice(), 1),
            (b"/etc/hosts\r\n".as_slice(), 2),
            (ssh_after.as_slice(), 1),
            (b"/etc/passwd\r\n".as_slice(), 9),
        ];
        assert_eq!(at(dl, b"\x1b[9;1H"), [1], "the control: nothing moved");
        assert_eq!(at(dl, b"\x1b[1;1H\x1b[M"), [0], "DL");
    }

    /// **The rows under a region anchored at the top keep their writers as
    /// it scrolls** (H5): a scroll of a region at the top retires its first
    /// line into the history, and the rows under it keep their text and
    /// take new numbers, each the number of the row under it. Under `ssh
    /// h`, `/etc/passwd` on row 7; the shell's `% ls` on row 8 and `ls`'s
    /// `/etc/hosts` on row 9; then a local program sets a region over rows
    /// 1-5 and leaves the cursor on its last row, and a local line feed
    /// there scrolls it a line. Row 7 stays dark and row 9 stays lit; the
    /// same line feed by `ssh` darkens row 9, a move filed under its own
    /// writer as well. Named breaks: carry nothing -- row 7, numbered as row 8 was,
    /// lights; file no move's lines under the read's own writer -- `ssh`'s
    /// line feed leaves row 9 lit.
    #[test]
    fn rows_under_a_top_region_keep_their_writers_as_it_scrolls() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 12,
        };
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let ls_after = [d.as_slice(), &ran("ls", ";cmdline=ls")].concat();
        let prompt = prompt_after("");
        let by = |edit: &[u8], group: Pid| {
            let ws = WinSize {
                ws_col: ws.ws_col,
                ws_row: ws.ws_row,
            };
            let reads = [
                read(&ssh, 1),
                read(b"\n\n\n\n\n\n/etc/passwd\r\n", 9),
                read(&ls_after, 1),
                read(b"/etc/hosts\r\n", 2),
                read(&prompt, 1),
                Read {
                    then: Then::Answered,
                    ..read(b"\x1b[1;5r\x1b[5;1H", 2)
                },
                read(edit, group),
            ];
            let (lit, origins, ()) = taken_numbered(
                ws,
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |_, _| (),
            );
            (lit, origins)
        };
        let at = |edit| by(edit, 2);
        assert_eq!(at(b"").0, [9], "the control: nothing moved");
        let (lit, origins) = at(b"\n");
        assert_eq!(lit, [9], "{origins:?}");
        let (lit, origins) = by(b"\n", 9);
        assert_eq!(lit, Vec::<usize>::new(), "by ssh: {origins:?}");
    }

    /// **A local read of many lines under a region anchored at the top
    /// keeps the history it makes lit, and a remote row it moves dark**
    /// (M1): `ssh h`'s `/etc/passwd` on row 3 of a 10-row screen, then a
    /// local program sets a region over rows 1-9 and prints `/etc/hosts`
    /// on fifty lines at its last row, each line feed retiring the
    /// region's top into the history and moving the row under it to the
    /// next number. The rows `/etc/hosts` holds on the screen light, the
    /// lines it left in the history are this machine's, and `ssh`'s line
    /// in the history stays remote. Named break: carry each move as text
    /// moved anywhere in its band (`Move::Within`), as a fold of every
    /// move into one would -- the screen's rows go dark.
    #[test]
    fn many_lines_under_a_top_region_keep_their_writers() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let d = format!("\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07").into_bytes();
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let ls_after = [d.as_slice(), &ran("ls", ";cmdline=ls")].concat();
        let prompt = prompt_after("");
        let flood = [
            b"\x1b[1;9r\x1b[9;1H".as_slice(),
            &b"/etc/hosts\r\n".repeat(50),
        ]
        .concat();
        let reads = [
            read(&ssh, 1),
            read(b"\n\n/etc/passwd\r\n", 9),
            Read {
                then: Then::Answered,
                ..read(&prompt, 1)
            },
            read(&ls_after, 1),
            read(&flood, 2),
        ];
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let (lit, origins, history) = taken_numbered(
            ws,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |_, paths| (paths.origin(at(3)), paths.origin(at(30))),
        );
        assert_eq!(lit, (0..8).collect::<Vec<_>>(), "{origins:?}");
        assert_eq!(history, (Origin::Remote, Origin::Local));
    }

    /// **A page-width request resized to**, as the app's `after` resizes
    /// to one (`width::Widths::poll`): the columns asked for, the rows of
    /// `ws` kept.
    fn page_width(t: &mut Term<'_>, ws: &WinSize, recuts: &mut crate::paths::Recuts) {
        if let Some(w) = t.requested_width() {
            if usize::from(w.ws_col()) != t.geometry().0.index() {
                let _ = recuts.resize(
                    t,
                    &WinSize {
                        ws_col: w.ws_col(),
                        ws_row: ws.ws_row,
                    },
                );
            }
        }
    }

    /// Where [`renumbered_by`]'s count starts: before the history it
    /// feeds, or after it.
    #[derive(Clone, Copy)]
    enum CountFrom {
        Before,
        After,
    }

    /// **One part of a drain on a terminal of `ws` in storage of `room`,
    /// filed as the drivers file it**: the history fed first, `lines` of
    /// it, and every line filed local (`Paths::all_written_here`), the
    /// count started before the history or, `late`, after it; then
    /// `bytes`, under a writer no one named, a page-width request resized
    /// to as the app resizes to one (`WidthHold::On`). What the drain
    /// leaves line 0, in the history, to read as.
    fn renumbered_by(
        (ws, room): (&WinSize, &WinSize),
        (lines, late): (Rows, CountFrom),
        bytes: &[u8],
    ) -> Origin {
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(room);
        let mut t = Term::new(owned.storage(), ws);
        t.set_width_hold(baud_core::WidthHold::On);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let mut counts = crate::keep::Counts::new(&t);
        for n in 0..lines.index() {
            let line = format!("line {n:02}\r\n");
            t.write_at(BytePos::ZERO, line.as_bytes());
            t.feed(BytePos::ZERO, ReadLen::from_index(line.len()));
        }
        let _ = t.take_record();
        match late {
            CountFrom::After => counts = crate::keep::Counts::new(&t),
            CountFrom::Before => {
                let _ = counts.numbered(&t, None);
            }
        }
        paths.all_written_here();
        let began = paths.line_on(t.screen(), AbsLine::at(Rows::ZERO));
        let mut pace = crate::reads::Pace::new();
        let mut took = Vec::new();
        let _ = crate::reads::paced(
            &mut t,
            &mut pace,
            Instant::now(),
            [(bytes, Sampler::default().unasked())],
            |_| {},
            |t, r| page_width(t, ws, r),
            &mut crate::hostlog::NoLog,
            &mut took,
        );
        let rec = took.iter().rev().find_map(|w| w.record);
        let _ = counts.numbered(&t, rec.as_ref());
        pr.drain_taken(
            &mut paths,
            &t,
            (began, began),
            counts.core_lines(&t),
            took.into_iter().map(|w| (w, Linked::No)),
            Instant::now(),
        );
        paths.origin(AbsLine::at(Rows::ZERO))
    }

    /// **A page-width request among a read's parts leaves every line its
    /// re-cut renumbered not known** (N2, P1): twenty lines of history on
    /// a 40-column window, every line filed local, then a read that asks
    /// for 132 columns -- by `DECSCPP`, or by `DECCOLM` under `?40` --
    /// which the app resizes to between the read's parts. Line 0, in the
    /// history, is not known, a local fact filed over it after included.
    /// The control: a read of text alone leaves it local. Named break: file
    /// nothing for a part the core says renumbered the lines -- line 0
    /// stays local, under either request.
    #[test]
    fn a_page_width_request_among_a_reads_parts_darkens_what_it_renumbered() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let room = WinSize {
            ws_col: 132,
            ws_row: 6,
        };
        assert_eq!(
            renumbered_by((&ws, &room), (Rows::of_count(20), CountFrom::Before), b"x"),
            Origin::Local,
            "the control: no request"
        );
        assert_eq!(
            renumbered_by(
                (&ws, &room),
                (Rows::of_count(20), CountFrom::Before),
                b"one\r\n\x1b[132$|two"
            ),
            Origin::Unknown,
            "DECSCPP"
        );
        assert_eq!(
            renumbered_by(
                (&ws, &room),
                (Rows::of_count(20), CountFrom::Before),
                b"one\r\n\x1b[?40h\x1b[?3htwo"
            ),
            Origin::Unknown,
            "DECCOLM"
        );
    }

    /// **The parts of one drain are each placed by their own top, across
    /// a screen switch and a re-cut between them** (D1): history, then one
    /// drain of three reads: a switch to the alternate screen and back, a
    /// page-width request the harness resizes to (`DECSCPP`), and eight
    /// lines and `/etc/passwd`, which retire lines past the window the
    /// re-cut renumbered and land on lines it numbered. `/etc/passwd`
    /// lights where it is, and written by `ssh` it stays dark. Two re-cuts:
    /// to 132 columns from 40 over `ls`'s `/etc/hosts`, which leaves the
    /// window's top where it was, and to 80 from 132 over ten lines of 100
    /// columns, which wraps each and moves the top by its own renumbering
    /// -- checked first on a terminal of its own. The control: the same
    /// drain with no request. Named break: number a part's top by the
    /// batch's count as it began -- `/etc/passwd`'s line is filed off its
    /// own, and it goes dark, after either re-cut.
    #[test]
    fn the_parts_of_a_drain_are_placed_across_a_switch_and_a_re_cut() {
        let wide = [[b'x'; 100].as_slice(), b"\r\n"].concat().repeat(10);
        // The re-cut to 80 columns moves the window's top.
        let (ws, narrow) = (
            WinSize {
                ws_col: 132,
                ws_row: 6,
            },
            WinSize {
                ws_col: 80,
                ws_row: 6,
            },
        );
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        t.write_at(BytePos::ZERO, &wide);
        t.feed(BytePos::ZERO, ReadLen::from_index(wide.len()));
        let mark = t.history_mark();
        let _ = t.resize(&narrow);
        assert!(
            matches!(t.history_moved_since(mark), baud_core::Moved::Retired(n) if n.index() > 0),
            "the re-cut moved the top: {:?}",
            t.history_moved_since(mark)
        );
        let flood = [b"a\r\n".repeat(8).as_slice(), b"/etc/passwd\r\n"].concat();
        let lit = |(cols, history): (u16, &[u8]), request: &[u8], group: Pid| {
            let ws = WinSize {
                ws_col: cols,
                ws_row: 6,
            };
            let reads = [
                read(history, 2),
                read(b"\x1b[?1049hx\x1b[?1049l", 2),
                Read {
                    drain: Drain::Same,
                    ..read(request, 2)
                },
                Read {
                    drain: Drain::Same,
                    ..read(&flood, group)
                },
            ];
            lit_driven(ws, named_groups(), &reads)
        };
        for (from, request) in [
            ((40, b"/etc/hosts\r\n".as_slice()), b"\x1b[132$|".as_slice()),
            ((132, wide.as_slice()), b"\x1b[80$|".as_slice()),
        ] {
            let cols = from.0;
            assert_eq!(lit(from, b"", 2), [4], "{cols}: the control, no request");
            assert_eq!(
                lit(from, request, 2),
                [4],
                "{cols}: a re-cut between the parts"
            );
            assert_eq!(lit(from, request, 9), Vec::<usize>::new(), "{cols}: by ssh");
        }
    }

    /// **A full-screen scroll down that takes lines back keeps every fact
    /// on its line, unless it takes back past the count's first line**
    /// (Restored): ten lines on a six-row window, filed local, then `SD 3`
    /// (`CSI 3 T`), which takes three lines back from the history. With
    /// the count started before the history, it follows, and line 0 stays
    /// local. With the count started over the history, at its first line,
    /// it clamps there (`PastStart`), and every line up to the window's
    /// last before the clamp is not known. Named break: file nothing for a
    /// clamp -- line 0 stays local.
    #[test]
    fn a_take_back_keeps_its_facts_unless_it_passes_the_first_line() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        assert_eq!(
            renumbered_by(
                (&ws, &ws),
                (Rows::of_count(10), CountFrom::Before),
                b"\x1b[3T"
            ),
            Origin::Local,
            "followed"
        );
        assert_eq!(
            renumbered_by(
                (&ws, &ws),
                (Rows::of_count(10), CountFrom::After),
                b"\x1b[3T"
            ),
            Origin::Unknown,
            "past the first line"
        );
    }

    /// **A read is cut at the shell's `D` only where the records of the
    /// parts before it vouch they wrote forward** (`vouch::CutAt`): under
    /// `ssh h`, one read, sampled with the shell in front, of `ssh`'s last
    /// bytes, the shell's `D` and a prompt with `/etc/hosts` typed at it.
    /// After `logout` and a line feed the cut is taken, and the prompt's
    /// path lights; after `logout`, a climb (`CSI A`) and two line feeds,
    /// which leave the prompt's row below every row the climb reached, the
    /// cut is refused and the read filed whole under `ssh`'s verdict, the
    /// prompt with it, dark. Named break: take the cut whatever the records
    /// vouch -- the prompt's path lights after the climb.
    #[test]
    fn a_read_is_cut_only_where_its_parts_before_vouch() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let prompt = prompt_after("/etc/hosts");
        let lit = |remote: &[u8]| {
            let read_ = [remote, &prompt].concat();
            let reads = [read(&start, 1), read(b"x\r\n", 9), read(&read_, 1)];
            !lit_reads(named_groups(), &reads).is_empty()
        };
        assert!(lit(b"logout\r\n"), "the control: forward only, cut");
        assert!(!lit(b"logout\x1b[A\r\n\r\n"), "a climb");
    }

    /// **Text after a remote read's last line feed stays the read's**
    /// (`vouch::Below`, its end row): under `ssh h`, a read of a line and
    /// `/etc/passwd` after its line feed, with none after it, then the
    /// shell's `D` and prompt on the same row, in a read of its own or
    /// behind the cut in the same read. The row stays dark: the line the
    /// read ended on holds what it wrote, so it is the read's. The control:
    /// the read ending on its line feed, the path on the row above, dark,
    /// and the prompt's row the shell's. Named break: vouch the end line
    /// clean whatever its row holds -- the path lights after the prompt's
    /// own read.
    #[test]
    fn text_after_a_remote_reads_last_line_feed_stays_the_reads() {
        let (start, prompt) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after(""));
        let lit = |remote: &[u8]| {
            let both = [remote, &prompt].concat();
            [
                lit_reads(
                    named_groups(),
                    &[read(&start, 1), read(remote, 9), read(&prompt, 1)],
                ),
                lit_reads(named_groups(), &[read(&start, 1), read(&both, 9)]),
            ]
        };
        assert_eq!(
            lit(b"x\r\n/etc/passwd\r\n"),
            [Vec::<usize>::new(), Vec::new()],
            "the control"
        );
        assert_eq!(lit(b"x\r\n/etc/passwd"), [Vec::<usize>::new(), Vec::new()]);
    }

    /// Whether a terminal fed `bytes` whole records a `C`.
    fn core_reads_c(end: baud_domain::ControlStringEnd, bytes: &[u8]) -> bool {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_control_string_end(end);
        let _ = t.take_record();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        t.take_record().marks().iter().any(|m| m.letter == b'C')
    }

    /// The core's reading under which a picture's body holds a mark.
    const KITTY: baud_domain::ControlStringEnd = baud_domain::ControlStringEnd::Kitty;

    /// The shell's `C` naming `cat y` here, with the window's token, ended
    /// by `end`, `BEL` or `ST`.
    fn cat_here(end: &[u8]) -> Vec<u8> {
        [
            format!("\x1b]133;C;baud_token={TOKEN};cmdline=cat\\ y").as_bytes(),
            end,
        ]
        .concat()
    }

    /// **After a mark the app read and the core did not, the window's marks
    /// are not trusted** (`vouch::Latch`), on a terminal set to end an
    /// `APC G` only at `BEL` or `ESC \\` ([`Fake::kitty_ending`]), the
    /// one reading under which a control string's body can hold a mark: an
    /// unterminated kitty graphics command (`ESC _ G a=d;`), then the
    /// shell's `C` naming `cat y` here with its token, and a `ST` -- which
    /// the core reads inside the command's body, so it runs no `C`, where
    /// the app's scan reads one and believes `cat` runs here -- cut into
    /// two reads at every byte, the `C` ended by `BEL` and by `ST`; then a
    /// writer gone before it was named prints `/etc/hosts`. The row lights
    /// only where the core read the `C` too: what the core reads decides,
    /// so this holds under any reading of the command's body. The control:
    /// no graphics command, the `C` read by both, and the row lit under
    /// `cat`. Named break: no latch (`Probes::latched` trusting every part)
    /// -- the gone writer is lifted under the `C` the core never ran.
    #[test]
    fn a_mark_the_core_did_not_read_leaves_the_window_untrusted() {
        let lit = |first: &[u8], cut: usize| {
            !lit_reads(
                named_groups().kitty_ending(),
                &[
                    read(&first[..cut], 1),
                    read(&first[cut..], 1),
                    read(b"/etc/hosts", 6),
                ],
            )
            .is_empty()
        };
        for end in [b"\x07".as_slice(), b"\x1b\\"] {
            let st = end.len() == 2;
            let control = cat_here(end);
            assert!(lit(&control, control.len()), "the control, st {st}");
            let first = [b"\x1b_Ga=d;".as_slice(), &cat_here(end), b"\x1b\\"].concat();
            let core = core_reads_c(KITTY, &first);
            for cut in 0..=first.len() {
                assert_eq!(lit(&first, cut), core, "st {st}, cut at {cut}");
            }
        }
    }

    /// **A mark both sides read does not re-sync the window unless it is the
    /// shell's `D`** (`vouch::Resync`): after the drift above, a `C` with
    /// no token naming `cat y` -- read by the core and the scan alike,
    /// which leaves the scan's belief that `cat` runs here as it was --
    /// then a writer gone before it was named prints `/etc/hosts`: dark.
    /// Named break: re-sync on any part whose marks match and hold one --
    /// the gone writer is lifted under the drifted `C`.
    #[test]
    fn a_matched_mark_that_is_not_the_shells_d_leaves_the_window_untrusted() {
        let drift = [b"\x1b_Ga=d;".as_slice(), &cat_here(b"\x07"), b"\x1b\\"].concat();
        assert!(!core_reads_c(KITTY, &drift), "the core runs no C here");
        let tokenless = b"\x1b]133;C;cmdline=cat\\ y\x07";
        let lit = lit_reads(
            named_groups().kitty_ending(),
            &[read(&drift, 1), read(tokenless, 1), read(b"/etc/hosts", 6)],
        );
        assert_eq!(lit, Vec::<usize>::new());
    }

    /// **The shell's `D` with its token re-syncs the window** (`vouch::Resync`):
    /// after the drift above and a line feed, the shell's `D` and a prompt,
    /// then its `C` naming `cat y` here with its token, then a writer gone
    /// before it was named printing `/etc/hosts`, which lights under `cat`
    /// as it does with no drift. The rows the drifted bytes reached stay
    /// theirs: the line they ended on is no prompt's to vouch clean. Named
    /// break: no part re-syncs (`Resync::of` always none) -- the gone
    /// writer is not lifted.
    #[test]
    fn the_shells_d_re_syncs_the_window() {
        let lit = |first: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(&[first, b"\r\n"].concat(), 6),
                    read(&prompt_after("x\r\n"), 1),
                    read(&cat_here(b"\x07"), 1),
                    read(b"/etc/hosts", 6),
                ],
            )
        };
        let control = lit(&cat_here(b"\x07"));
        assert_eq!(control, [2], "the control: no drift");
        let drift = [b"\x1b_Ga=d;".as_slice(), &cat_here(b"\x07"), b"\x1b\\"].concat();
        assert_eq!(lit(&drift), control);
    }

    /// **The shell's `D` inside a picture's body does not re-sync the
    /// window** (`vouch::Resync`, its match first), on a terminal set to
    /// end an `APC G` only at `BEL` or `ESC \\`: after the drift above,
    /// the shell's `D` with its token and its prompt inside a kitty
    /// graphics command left open, where the core runs no mark and the
    /// scan reads the `D` and the token, and a `ST`; then its `C` naming
    /// `cat y` with the token, and a writer gone before it was named
    /// printing `/etc/hosts`: dark, the window still drifted. The control:
    /// no graphics command around the `D`, lit. Named breaks, together:
    /// `Resync::of` without its match and `Latch::step` without its own
    /// arm for a part that does not match -- the payload's `D` re-syncs;
    /// either alone is held by the other. Each layer is pinned alone
    /// elsewhere: the match by `vouch`'s
    /// `a_tokened_d_in_a_pictures_body_does_not_re_sync`, and the latch's
    /// arm by `a_mark_the_core_did_not_read_leaves_the_window_untrusted`.
    #[test]
    fn the_shells_d_in_a_pictures_body_does_not_re_sync() {
        let drift = [b"\x1b_Ga=d;".as_slice(), &cat_here(b"\x07"), b"\x1b\\\r\n"].concat();
        let lit = |d: &[u8]| {
            !lit_reads(
                named_groups().kitty_ending(),
                &[
                    read(&drift, 6),
                    read(d, 1),
                    read(&cat_here(b"\x07"), 1),
                    read(b"/etc/hosts", 6),
                ],
            )
            .is_empty()
        };
        let d = prompt_after("x\r\n");
        assert!(lit(&d), "the control");
        let inside = [b"\x1b_Ga=d;".as_slice(), &d, b"\x1b\\"].concat();
        assert!(!core_reads_c(KITTY, &inside) && !lit(&inside));
    }

    /// **A row written below and climbed back from is off the read's
    /// path** (E5, the climb's own line): under `ssh h`, a read that writes
    /// `/etc/passwd` a row down, past where the prompt will draw, climbs
    /// back (`CSI A`), and writes `logout` and a line feed onto the
    /// `/etc/passwd` row, then the shell's `D` and prompt there -- in a
    /// read of its own, and in the same read behind the cut. The row stays
    /// dark: the climb recorded the row it left, so the line the read
    /// ended on is off its path, and the cut is refused. The control: no
    /// climb, the prompt's path lights. Named breaks, together: a climb
    /// records only the row it lands on (`Paint::record_climb`) and
    /// `Below::of` blind to the end row -- the `/etc/passwd` row lights
    /// after the prompt's own read. Each alone is held by the other: the
    /// end row holds `/etc/passwd`; and behind the cut, the climb's
    /// landing row is off the path still, which refuses it. Each layer is
    /// pinned alone elsewhere: the climb's record by the core's
    /// `a_climb_records_down_to_the_lowest_row_the_cursor_reached`
    /// (`read_record.rs`), and the end row by `vouch`'s
    /// `below_vouches_only_where_the_record_does` (its `foo\r\nbar` case).
    #[test]
    fn a_row_climbed_back_from_stays_the_reads() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let lit = |remote: &[u8], typed: &str| {
            let prompt = prompt_after(typed);
            let both = [remote, &prompt].concat();
            [
                !lit_reads(
                    named_groups(),
                    &[read(&start, 1), read(remote, 9), read(&prompt, 1)],
                )
                .is_empty(),
                !lit_reads(named_groups(), &[read(&start, 1), read(&both, 1)]).is_empty(),
            ]
        };
        assert_eq!(
            lit(b"logout\r\n", "/etc/hosts"),
            [true, true],
            "the control"
        );
        assert_eq!(
            lit(b"\n\x1b[5G/etc/passwd\x1b[A\rlogout\r\n", ""),
            [false, false]
        );
    }

    /// **A column inserted across the region writes every row of it**
    /// (H6, `DECBI` at the left margin): `ls` prints `/etc/hosts` on the
    /// first row; then, under `ssh h`, the remote program's `ESC 6` at the
    /// left margin of the last row shifts every row of the region right, the
    /// first included. The first row goes dark: the remote program moved
    /// its text. The control: the remote program writes on its own row,
    /// and the first row keeps its line. Named break: `DECBI` records no
    /// rows (`Paint::shift_columns_at` without `wrote_rows`) -- the first
    /// row stays lit.
    #[test]
    fn a_remote_back_index_at_the_margin_writes_every_row() {
        let lit = |remote: &[u8]| {
            lit_reads(
                named_groups(),
                &[
                    read(b"/etc/hosts\r\n", 2),
                    read(&ran("ssh h", ";cmdline=ssh\\ h"), 1),
                    read(remote, 9),
                ],
            )
        };
        assert_eq!(lit(b"\x1b[5;1Hx"), [0], "the control");
        assert_eq!(lit(b"\x1b[5;1H\x1b6"), Vec::<usize>::new());
    }

    /// **Text a region scroll moved before its writer was named is filed
    /// where it went** (B10): `less` (named here) writes two lines; a
    /// writer writes a path on the third row; `less`, sampled in front
    /// again, scrolls a region of the second and third rows by one inside
    /// the batch's settle, so the
    /// path moves to the second row; then the batch is answered -- or a
    /// batch is sent before the scroll and its answer taken after it.
    /// Written by `ls`, the path lights on its new row; written by `ssh`,
    /// it stays dark there. Named breaks: carry no line whose writer is
    /// still to be named (`Probes::carry_pending`), so each answer files
    /// on its read-time span -- `ssh`'s path lights on the row it moved
    /// to; and a moved line whose writer is still to be named read as no
    /// writer named (`Paths::worst_over`) -- `ls`'s path stays dark. The
    /// answer carries no lines as read to file on (`Answer::seen`), so the
    /// sent batch's answer is filed where its lines were carried or
    /// nowhere.
    #[test]
    fn text_moved_before_its_writer_is_named_is_filed_where_it_went() {
        let lit = |group: Pid, path: &[u8]| {
            lit_driven(
                WS,
                named_groups(),
                &[
                    read(b"a\r\nb\r\n", 2),
                    read(path, group),
                    read(b"\x1b[m", 2),
                    read(b"\x1b[2;3r\x1b[3;1H\nz", 2),
                ],
            )
        };
        assert_eq!(lit(2, b"/etc/hosts"), [1], "ls's path, moved");
        assert_eq!(
            lit(9, b"/etc/passwd"),
            Vec::<usize>::new(),
            "ssh's path, moved"
        );
        // The batch sent before the scroll, its answer taken after it.
        let sent = |group: Pid, path: &[u8]| {
            lit_driven(
                WS,
                named_groups(),
                &[
                    read(b"a\r\nb\r\n", 2),
                    read(path, group),
                    Read {
                        then: Then::Sent,
                        ..read(b"\x1b[m", 2)
                    },
                    read(b"\x1b[2;3r\x1b[3;1H\nz", 2),
                ],
            )
        };
        assert_eq!(
            sent(2, b"/etc/hosts"),
            [1],
            "ls's, answered after the scroll"
        );
        assert_eq!(
            sent(9, b"/etc/passwd"),
            Vec::<usize>::new(),
            "ssh's, answered after the scroll"
        );
    }

    /// **The row text left before its writer was named does not take that
    /// writer's answer** (B10, the open side): `ls` prints a path and `ssh`
    /// one under it, both still to be named; a region scroll moves both up
    /// a row inside the batch's settle, so the row `ls`'s path was read on
    /// holds `ssh`'s path now; then the batch is answered -- or sent before
    /// the scroll and answered after it. `ls`'s path lights on the row it
    /// moved to, and `ssh`'s stays dark on the row it moved into, which
    /// nothing but its own writer's answer files. Named breaks: carry no
    /// line whose writer is still to be named (`Probes::carry_pending`) --
    /// `ls`'s answer is filed on its read-time row and lights `ssh`'s path
    /// there; and a moved line whose writer is still to be named read as no
    /// writer named (`Paths::worst_over`) -- `ls`'s path stays dark. The
    /// answer carries no lines as read to file on (`Answer::seen`).
    #[test]
    fn the_row_text_left_does_not_take_its_writers_answer() {
        let left = |then: Then| {
            lit_driven(
                WS,
                named_groups(),
                &[
                    read(b"a\r\nb\r\n", 2),
                    read(b"/etc/hosts\r\n", 2),
                    read(b"/etc/passwd", 9),
                    Read {
                        then,
                        ..read(b"\x1b[m", 2)
                    },
                    read(b"\x1b[2;4r\x1b[4;1H\n", 2),
                ],
            )
        };
        assert_eq!(left(Then::Nothing), [1], "answered after the scroll");
        assert_eq!(left(Then::Sent), [1], "sent before the scroll");
    }

    /// **A local answer filed over another writer's lines still to be named
    /// does not light them** (B10, the hull): `ls` prints `/etc/passwd`,
    /// named and looked up; `ls` writes a row and its batch goes; `ssh`
    /// prints `/etc/passwd` on the row under it and, sampled in front again,
    /// scrolls a region that takes that row and the next down one, so
    /// `ls`'s lines, carried, are
    /// the hull of where they were and where their last went, and cover
    /// `ssh`'s path. `ls`'s answer, taken alone, lights its own path and
    /// not `ssh`'s; `ssh`'s answer after it leaves `ssh`'s dark. The
    /// control: the second writer `ls` too, its path lit once both are
    /// answered. Named break: a held line read by its facts alone
    /// (`Paths::wrote`, `Paths::origin_over`) -- `ssh`'s path lights on
    /// `ls`'s answer.
    #[test]
    fn a_local_answer_over_another_writers_lines_does_not_light_them() {
        let run = |group: Pid| {
            lit_midway(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Answered,
                        ..read(b"\x1b[3;6r/etc/passwd\r\n", 2)
                    },
                    Read {
                        then: Then::Sent,
                        ..read(b"x\r\ny", 2)
                    },
                    read(b"\r\n/etc/passwd\r\n\r\n", group),
                    read(b"\x1b[m", group),
                    Read {
                        then: Then::Took,
                        ..read(b"\x1b[3;1H\x1bM\x1b[6;1H\n", group)
                    },
                ],
            )
        };
        assert_eq!(run(2), (vec![vec![0, 3]], vec![0, 3]), "the control");
        assert_eq!(run(9), (vec![vec![0]], vec![0]));
    }

    /// **Text still to be named that a move puts on a line filed before
    /// takes nothing from that line** (B10, F3): `ls` prints `/etc/passwd`
    /// and three rows, named and looked up; `ssh`, its writer still to be
    /// named and nothing marking it, prints `/etc/passwd` under them and,
    /// sampled in front again, scrolls the region up a row, which moves its
    /// path onto a row `ls` wrote. Before `ssh`'s answer the path is dark,
    /// and after it. The control: the second writer `ls`, dark until its
    /// answer and lit after it. Named break: a moved line whose source is
    /// still to be named held under no group (`Probes::groups_of` naming
    /// none) -- `ssh`'s path lights before its answer on `ls`'s fact.
    #[test]
    fn moved_text_still_to_be_named_takes_nothing_from_where_it_lands() {
        let run = |group: Pid| {
            lit_midway(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Answered,
                        ..read(b"\x1b[3;5r/etc/passwd\r\na\r\nb\r\nc", 2)
                    },
                    read(b"\r\n/etc/passwd", group),
                    read(b"\x1b[m", group),
                    Read {
                        then: Then::Took,
                        ..read(b"\n", group)
                    },
                ],
            )
        };
        assert_eq!(run(9), (vec![vec![0]], vec![0]));
        assert_eq!(run(2), (vec![vec![0]], vec![0, 3]), "the control");
    }

    /// **A group still to be named writing over lines another group lit
    /// does not light them** (G1): `ls` prints `/etc/passwd` and three
    /// rows, named and looked up; `ssh`, unmarked and still to be named,
    /// climbs back to the path's row, prints `/etc/passwd` over it, and
    /// returns below, its read ending under the rows it wrote. Before
    /// `ssh`'s answer the path is dark, and after it. The control: `ls`
    /// rewriting its own rows, lit before and after its answer. Named
    /// breaks: no hold on a write (`Probes::read_taken` without
    /// `Lit::others`) -- `ssh`'s path lights before its answer on `ls`'s
    /// fact; and the lines a read wrote taken without what its record says
    /// it wrote off its path (`part_spans` without `off_path`) -- the same,
    /// since the read began and ended under the row it climbed to.
    #[test]
    fn a_write_over_lines_another_group_lit_does_not_light_them() {
        let run = |group: Pid| {
            lit_midway(
                WinSize {
                    ws_col: 40,
                    ws_row: 8,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Answered,
                        ..read(b"/etc/passwd\r\na\r\nb\r\nc\r\n", 2)
                    },
                    Read {
                        then: Then::Took,
                        ..read(b"\x1b[4A/etc/passwd\x1b[4B", group)
                    },
                ],
            )
        };
        assert_eq!(run(9), (vec![vec![]], vec![]));
        assert_eq!(run(2), (vec![vec![0]], vec![0]), "the control");
    }

    /// **A group still to be named writing over a line that reads lit holds
    /// it, whoever lit it** (G1): `ls` prints `/etc/hosts` and four lines,
    /// named, so the path's line goes into the history; `ls`'s next read
    /// trims what lit each line to the window (`Lit::keep_from`); the window
    /// grows to eight rows and the line comes back into it with its local
    /// fact and nothing naming what lit it; then `ssh`, still to be named,
    /// moves to it and prints `/etc/passwd` over it. The line is not known
    /// before `ssh`'s answer. The control: `ls` printing over its own lit
    /// line, with nothing trimmed, keeps it lit before its answer;
    /// [`a_command_that_feeds_off_the_prompt_leaves_its_path_lit`] keeps a
    /// path typed on the shell's prompt lit, the prompt's read lighting it
    /// at once. Named break: hold only the lines another group is named
    /// for (`Lit`'s spans of other groups, met with the span) -- the line
    /// the window brought back reads local.
    #[test]
    fn a_line_that_reads_lit_is_held_whoever_lit_it() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 4,
        };
        let back = [
            Read {
                then: Then::Answered,
                ..read(b"/etc/hosts\r\na\r\nb\r\nc\r\nd", 2)
            },
            read(b"\x1b[m", 2),
            Read {
                then: Then::Resize(40, 8),
                ..read(b"", 1)
            },
            Read {
                then: Then::Took,
                ..read(b"\x1b[1;1H/etc/passwd", 9)
            },
        ];
        assert_eq!(
            origins_midway(ws, named_groups(), &back)[0][0],
            Origin::Unknown,
            "ssh's write over the line the window brought back"
        );
        let own = [
            Read {
                then: Then::Answered,
                ..read(b"/etc/hosts\r\n", 2)
            },
            Read {
                then: Then::Took,
                ..read(b"\x1b[1;1H/etc/hosts", 2)
            },
        ];
        let ws = WinSize {
            ws_col: 40,
            ws_row: 4,
        };
        assert_eq!(
            origins_midway(ws, named_groups(), &own)[0][0],
            Origin::Local,
            "the control: ls over its own lit line"
        );
    }

    /// **A command whose first read only feeds off the prompt line leaves
    /// a path typed there lit** (G1, `ReadRecord::start_row`): the shell's
    /// prompt with `/etc/passwd` typed on it, named and looked up; then
    /// `ls`'s first read, a carriage return and line feeds off that line
    /// and its output under it -- the typed path lit before `ls`'s answer
    /// and after. Where `ls`'s read returns and prints over the line
    /// instead, the line is held until its answer: dark before, lit after.
    /// Named break: hold the line a part began on whether it wrote it or
    /// not (`Writers::untouched` ignored) -- the typed path dark before
    /// `ls`'s answer.
    #[test]
    fn a_command_that_feeds_off_the_prompt_leaves_its_path_lit() {
        let run = |first: &[u8]| {
            lit_midway(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Answered,
                        ..read(b"$ /etc/passwd", 1)
                    },
                    Read {
                        then: Then::Took,
                        ..read(first, 2)
                    },
                ],
            )
        };
        assert_eq!(run(b"\r\nfoo\r\n"), (vec![vec![0]], vec![0]));
        assert_eq!(run(b"\r$ /etc/passwd x"), (vec![vec![]], vec![0]));
    }

    /// **A line a scroll copied text onto is held, though the read began on
    /// it and wrote no cell of it itself** (StartRow, B2, F-G1a): the shell
    /// lights `/etc/hosts` and the prompt's `/etc/passwd` in a region of
    /// rows 2 to 5; `ssh`'s first read feeds off the prompt, prints
    /// `file:///etc/hosts` on the bottom margin and feeds again, and the
    /// region's scroll copies that text onto the prompt's row -- not known
    /// before `ssh` is named. The control: no region, so no scroll moves
    /// text and the prompt keeps its row -- lit. Named break: a line taken
    /// as untouched whatever its screen's moves (`untouched_start` without
    /// `moved`) -- the row reads local before `ssh` is named.
    #[test]
    fn a_scroll_that_copies_onto_the_start_line_holds_it() {
        let run = |region: &'static [u8]| {
            let first = [
                region,
                b"\x1b[1;1H/etc/hosts\x1b[4;1H$ /etc/passwd".as_slice(),
            ]
            .concat();
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(&first, 1)
                },
                Read {
                    then: Then::Took,
                    ..read(b"\r\nfile:///etc/hosts\r\n", 9)
                },
            ];
            origins_midway(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &reads,
            )[0][3]
        };
        assert_eq!(run(b"\x1b[2;5r"), Origin::Unknown);
        assert_eq!(run(b""), Origin::Local, "the control: no region");
    }

    /// **The line a part began on is taken as untouched only where nothing
    /// else its record holds could have reached it** (StartRow): a read
    /// feeding off the line is; one that leaves it and switches to the
    /// alternate screen (`?47`, which clears nothing), or clears the
    /// alternate screen and comes back (`?1049`), is
    /// not -- the core says the line is not written, and the record holds
    /// a change it cannot place on it. Named breaks: the switch ignored
    /// (`untouched_start` without `switched`), and the clear ignored
    /// (without `cleared`) -- that line is taken as untouched.
    #[test]
    fn the_start_line_is_untouched_only_where_nothing_else_reached_it() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let untouched = |bytes: &[u8]| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let prompt = b"$ /etc/passwd";
            t.write_at(BytePos::ZERO, prompt);
            t.feed(BytePos::ZERO, ReadLen::from_index(prompt.len()));
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let w = Writers {
                record: Some(t.take_record()),
                top: Some(t.history_mark().line()),
                ..Writers::at(None)
            };
            let counts = crate::keep::Counts::new(&t);
            let paths = Paths::new(None, Forms::DEFAULT);
            let start = w.record.map(|r| r.start_row());
            let lines = counts.core_lines_at(Scrolled::START, &t).part(w.top);
            (start, untouched_start(&paths, &lines, &w).is_some())
        };
        let core_untouched = Some(baud_core::StartRow::Untouched);
        assert_eq!(
            untouched(b"\r\nfoo\r\n"),
            (core_untouched, true),
            "feeds off it"
        );
        assert_eq!(
            untouched(b"\n\x1b[?47h"),
            (core_untouched, false),
            "a switch"
        );
        assert_eq!(
            untouched(b"\n\x1b[?1049h\x1b[?1049l"),
            (core_untouched, false),
            "the alternate screen cleared"
        );
    }

    /// **A command that leaves the prompt line and comes back to print on it
    /// is held there** (StartRow, S1): the shell's prompt, named and looked
    /// up; `ls`'s first read prints on the line under it, moves the cursor
    /// back to the prompt line and prints `/etc/passwd` there. The line
    /// was not written while the cursor was still on it, but the move back
    /// is a climb the record names off the path, so the line is held: dark
    /// before `ls`'s answer, lit after. Named breaks: a cursor move up
    /// recorded as no climb (`Paint::set_row` without `record_climb`), and
    /// the line taken as untouched whatever the record names off the path
    /// (`untouched_start` without `named`) -- either way the path lights
    /// before `ls`'s answer.
    #[test]
    fn a_command_that_comes_back_to_the_prompt_line_is_held_there() {
        let lit = lit_midway(
            WinSize {
                ws_col: 40,
                ws_row: 6,
            },
            named_groups(),
            &[
                Read {
                    then: Then::Answered,
                    ..read(b"$ /etc/passwd", 1)
                },
                Read {
                    then: Then::Took,
                    ..read(b"\r\nfoo\x1b[1;1H/etc/passwd x", 2)
                },
            ],
        );
        assert_eq!(lit, (vec![vec![]], vec![0]));
    }

    /// **A move of lines nothing wrote carries nothing** (W3): on a fresh
    /// screen `ls` scrolls a region of never-written rows down a row, and
    /// then prints `/etc/passwd` on one of them -- lit. The control: `ssh`
    /// writes the region's top row first, so the move carries a remote
    /// writer onto the row `ls` then prints on -- dark. Named break: a line
    /// nothing was filed over read as no writer named (`Paths::worst_over`)
    /// -- `ls`'s path stays dark.
    #[test]
    fn a_move_of_lines_nothing_wrote_carries_nothing() {
        let run = |first: &[u8], group: Pid| {
            lit_driven(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &[
                    Read {
                        then: Then::Answered,
                        ..read(first, group)
                    },
                    read(b"\x1b[2;5r\x1b[2;1H\x1bM", 2),
                    read(b"\x1b[m", 2),
                    read(b"\x1b[3;1H/etc/passwd", 2),
                ],
            )
        };
        assert_eq!(run(b"\x1b[m", 2), [2]);
        assert_eq!(run(b"\x1b[2;1Hx", 9), Vec::<usize>::new(), "the control");
    }

    /// **A read nothing names leaves its own lines not known** (W3): no
    /// group sampled for it and no marks; it rewrites a line a local fact
    /// lit, which reads not known after, and leaves the line under it, which
    /// it did not write, local. Named break: such a read filed nowhere (the
    /// early return in `Probes::read_taken`) -- its line keeps the local
    /// fact.
    #[test]
    fn a_read_nothing_names_leaves_its_own_lines_not_known() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        paths.written_by_shell(LineSpan::of(line(0), line(1)));
        pr.publish(&mut paths);
        let span = LineSpan::of(line(0), line(0));
        pr.read_taken(&mut paths, Writers::at(None), span, Linked::No);
        let origins = [0, 1].map(|n| paths.origin(line(n)));
        assert_eq!(origins, [Origin::Unknown, Origin::Local]);
    }

    /// **A prompt redrawn inside the pace after a fast command's `D` keeps
    /// a path typed on it lit** (W3, F2): `ls`'s last output read asks;
    /// the shell's `D` and prompt come within the pace, unasked, forgetting
    /// the group sampled before; a read that redraws the prompt line comes
    /// within the pace too, unasked, with no group sampled; then
    /// `/etc/passwd` typed and echoed one key a read, the shell in front,
    /// and the batch after the last key answered -- the typed path lit.
    /// Named break: a read inside the pace with no group taken before it
    /// asks nothing (`Sampler::read`) -- the redraw is filed under no
    /// writer, and the path stays dark.
    #[test]
    fn a_prompt_redrawn_inside_the_pace_keeps_its_typed_path_lit() {
        let mut f = Fake::with(&[("/etc", Entry::Dir), ("/etc/passwd", Entry::File)]);
        f.groups.insert(1, vec!["zsh".into()]);
        f.groups.insert(2, vec!["ls".into()]);
        let mut batches: Vec<Vec<(Duration, Vec<u8>, Pid)>> = vec![
            vec![(Duration::from_millis(0), ran("ls", ";cmdline=ls"), 1)],
            vec![(Duration::from_millis(50), b"out\r\n".to_vec(), 2)],
            vec![(Duration::from_millis(55), prompt_after(""), 1)],
            vec![(Duration::from_millis(58), b"\r\x1b[K% ".to_vec(), 1)],
        ];
        for (k, b) in b"/etc/passwd".iter().enumerate() {
            let at = 200 + 100 * u64::try_from(k).expect("a key");
            batches.push(vec![(Duration::from_millis(at), vec![*b], 1)]);
        }
        assert_eq!(lit_on_a_timeline(f, &batches), [2]);
    }

    /// **A line written while paths were off is not known once they are on
    /// again** (W3, F1): `ls` prints four lines, lit; paths off; `ssh`
    /// prints `/etc/passwd` over the second, opening no link, so no answer
    /// is kept for it; paths on; `ls` scrolls a region that moves the path a
    /// row down, onto a line its own text lit -- dark. The control: the same
    /// path printed by `ls` with paths on, and the same move -- lit. Named
    /// break: no floor at the turn (`Probes::floor_off` filing nothing) --
    /// the path lights.
    #[test]
    fn a_line_written_while_paths_were_off_is_not_known_after() {
        let run = |printed: &[Read<'static>]| {
            let mut reads = vec![
                Read {
                    then: Then::Paths(Turned::On),
                    ..read(b"", 2)
                },
                read(b"a\r\nb\r\nc\r\nd", 2),
            ];
            reads.extend_from_slice(printed);
            reads.extend([
                read(b"\x1b[m", 2),
                read(b"\x1b[2;4r\x1b[2;1H\x1bM\x1b[r\x1b[6;1H", 2),
                read(b"\x1b[m", 2),
            ]);
            lit_driven(
                WinSize {
                    ws_col: 40,
                    ws_row: 6,
                },
                named_groups(),
                &reads,
            )
        };
        let ssh_while_off = [
            Read {
                then: Then::Paths(Turned::Off),
                ..read(b"\x1b[m", 2)
            },
            read(b"\x1b[2;1H/etc/passwd\x1b[6;1H", 9),
            Read {
                then: Then::Paths(Turned::On),
                ..read(b"\x1b[m", 2)
            },
        ];
        assert_eq!(run(&ssh_while_off), Vec::<usize>::new());
        // The control: `ls` prints the path, paths on throughout.
        let ls = [read(b"\x1b[2;1H/etc/passwd\x1b[6;1H", 2)];
        assert_eq!(run(&ls), [2], "the control");
    }

    /// **A line a still-unnamed writer overwrote and scrolled into history
    /// in one read is held there** (G1, F-G1b): the shell's `/etc/passwd`
    /// lit on line 1; `ssh` writes `file:///etc/hosts` over it and seven
    /// line feeds scroll it off the window -- not known until `ssh` is
    /// named, then remote. The control: the window not scrolled, the same
    /// line not known as well. Named break: what lit each line kept to the
    /// window before the drain's parts read it (`Lit::keep_from` ahead of
    /// `drain_parts`) -- line 1 reads local in history.
    #[test]
    fn a_line_overwritten_and_scrolled_away_in_one_read_is_held() {
        let ws = || WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let run = |over: &'static [u8]| {
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(b"/etc/hosts\r\n$ /etc/passwd", 1)
                },
                Read {
                    then: Then::Took,
                    ..read(over, 9)
                },
            ];
            let (_, origins, ()) = taken_numbered(
                ws(),
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |_, _| (),
            );
            (origins_midway(ws(), named_groups(), &reads)[0][1], origins)
        };
        let (midway, _) = run(b"\rfile:///etc/hosts      \r\n\r\n\r\n\r\n\r\n\r\n\r\n");
        assert_eq!(midway, Origin::Unknown, "scrolled into history");
        let (midway, after) = run(b"\rfile:///etc/hosts      ");
        assert_eq!(midway, Origin::Unknown, "the control: on the window");
        assert_eq!(after[1], Origin::Remote, "named");
    }

    /// **A read nothing names is not lifted by a command the shell named
    /// here writing its line after** (W3, F-W3b): `ls` runs and ends, the
    /// shell's `D` read inside the pace forgetting its group; a read inside
    /// the pace, sampled for no group and marked by nothing, prints
    /// `file:///etc/hosts` -- which the drivers reach only where the group
    /// sample after an ask names none ([`Sampler::read`] asks there);
    /// `ls` runs again from that line and is named local -- the line stays
    /// not known. The control: the same read sampled with `ls` in front --
    /// local. Named break: a read nothing names filed as a writer not named
    /// (`Paths::written_unnamed` filing `Fact::NotNamed`, which the
    /// command's lift reaches) -- local.
    #[test]
    fn a_read_nothing_names_is_not_lifted_by_a_local_command_after_it() {
        let ws = || WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let (ls, end) = (ran("ls", ";cmdline=ls"), prompt_after(""));
        let run = |third: Read<'_>| {
            origins_midway(
                ws(),
                named_groups(),
                &[
                    read(&ls, 2),
                    quiet(&end),
                    third,
                    Read {
                        then: Then::Answered,
                        ..read(&ls, 2)
                    },
                    Read {
                        then: Then::Took,
                        ..read(b"\x1b[m", 2)
                    },
                ],
            )[0][2]
        };
        assert_eq!(run(quiet(b"\r\nfile:///etc/hosts")), Origin::Unknown);
        assert_eq!(
            run(read(b"\r\nfile:///etc/hosts", 2)),
            Origin::Local,
            "the control: sampled"
        );
    }

    /// **Text a still-unnamed writer left in a scroll under side margins
    /// stays not known** (W3, F-W3a): `ls` lights row 0; `ssh` prints
    /// `file:///etc/hosts` on row 2; `ls` scrolls rows 1 to 5 within the
    /// left margins, the text to row 1 -- not known before `ssh` is named,
    /// remote after. The rows `ssh` passed and the rows below it nothing
    /// wrote are one gap the band carries from, longer than `ssh`'s span.
    /// The control: rows 1 to 4, a gap inside `ssh`'s span -- not known as
    /// well. Named break: only a span that holds the whole gap counted
    /// (`Paths::worst_over`'s gap check by containment) -- row 1 reads
    /// local with the taller region.
    #[test]
    fn a_scroll_under_side_margins_keeps_an_unnamed_writers_text_dark() {
        let ws = || WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let run = |scroll: &'static [u8]| {
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(b"\x1b[1;22H/etc/hosts /etc/passwd", 2)
                },
                read(b"\r\n\r\nfile:///etc/hosts", 9),
                read(b"\x1b[m", 2),
                Read {
                    then: Then::Took,
                    ..read(scroll, 2)
                },
            ];
            let midway = origins_midway(ws(), named_groups(), &reads)[0][1];
            let (_, origins, ()) = taken_numbered(
                ws(),
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |_, _| (),
            );
            (midway, origins[1])
        };
        assert_eq!(
            run(b"\x1b[?69h\x1b[1;20s\x1b[1;5r\x1b[5;1H\n"),
            (Origin::Unknown, Origin::Remote)
        );
        assert_eq!(
            run(b"\x1b[?69h\x1b[1;20s\x1b[1;4r\x1b[4;1H\n"),
            (Origin::Unknown, Origin::Remote),
            "the control: a gap inside the span"
        );
    }

    /// **A link a writer left open is judged by that writer whatever the
    /// shell writes under it** (F-Below, fails closed): `ssh` opens
    /// `OSC 8` to `file:///etc/passwd` at the end of its read and the
    /// shell's prompt is written under it -- the link is remote; with the
    /// opener gone before it was named, not known; the control, `ls`
    /// opening it, local. Two guards hold it, each enough alone: the rows
    /// the link is drawn on carry the opener's verdict, and the slot's
    /// record keeps the line the opener's read began on. Named break: both
    /// dropped (`Paths::link_origin` reading neither) -- the remote link
    /// is local.
    #[test]
    fn a_link_left_open_is_judged_by_its_opener() {
        let ws = || WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let link = |opener: Pid| {
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(b"/etc/hosts", 1)
                },
                read(b"\r\n\x1b]8;;file:///etc/passwd\x1b\\", opener),
                read(b"$ /etc/hosts", 1),
            ];
            let look = |t: &Term<'_>, p: &Paths| {
                let spot = crate::link::Spot {
                    scrolled: Scrolled::START,
                    back: Rows::ZERO,
                    row: baud_domain::Row::from_index(1),
                    col: baud_domain::Col::at_index(1),
                };
                let under = crate::link::hovered(t, spot).expect("the link");
                match p.printed(t, &under) {
                    crate::link::Printed::Link { origin } => origin.origin(),
                    crate::link::Printed::Path => panic!("a path"),
                }
            };
            taken_numbered(ws(), named_groups(), &reads, None, Numbering::Driven, look).2
        };
        assert_eq!(link(9), Origin::Remote, "ssh's");
        assert_eq!(link(7), Origin::Unknown, "gone before it was named");
        assert_eq!(link(2), Origin::Local, "the control: ls's");
    }

    /// **A hold is released by its own group's answer, its verdict filed
    /// there** (G1): a line held under `ssh`'s group reads not known; the
    /// answer naming `ssh` not local files remote on it and drops the
    /// hold, and one naming it local files local; either way its group is
    /// what lit the line now. Named break: a release that files nothing -- the held
    /// line reads not known after the answer.
    #[test]
    fn a_hold_is_released_and_filed_by_its_own_groups_answer() {
        use crate::paths::Seen;
        let line = |n| AbsLine::at(Rows::of_count(n));
        for (verdict, origin) in [
            (Seen::Named(Front::Elsewhere), Origin::Remote),
            (Seen::Named(Front::Local), Origin::Local),
        ] {
            let (mut pr, _) = probes(named_groups());
            let mut paths = Paths::new(None, Forms::DEFAULT);
            paths.set(Turned::On);
            let id = Id::default();
            pr.kept
                .edit()
                .inflight
                .push((id, Some(9), LineSpan::of(line(5), line(5))));
            pr.held
                .edit()
                .push((Some(9), LineSpan::of(line(3), line(3))));
            pr.publish(&mut paths);
            assert_eq!(paths.origin(line(3)), Origin::Unknown, "held");
            pr.file_answered(&mut paths, (id, Kind::Screen), &[(Some(9), verdict)], &[]);
            assert_eq!((paths.origin(line(3)), pr.held.get().len()), (origin, 0));
            let three = vec![LineSpan::of(line(3), line(3))];
            assert!(pr.lit.not_of(Some(9), three).is_empty(), "lit by its group");
        }
    }

    /// **A moved line whose covering span names no group is held all the
    /// same** (F3's guard): with no group found, under the lines past the
    /// groups kept, which the next batch's answer files not known. The
    /// control: a group found is the group held under. Named break:
    /// `holders` returning what it found -- no group, no hold.
    #[test]
    fn a_covering_span_with_no_group_still_holds() {
        assert_eq!(holders(Vec::new()), [None]);
        assert_eq!(holders(vec![Some(9)]), [Some(9)]);
    }

    /// **What lit each line splits, merges, moves with the text, and
    /// keeps to the window** (G1, `Lit`): `ssh` lighting rows 2-3 of
    /// `ls`'s 0-5 splits `ls`'s in two; `ls` lighting them again makes one
    /// span; a region scroll carries a span exactly and a fold of two
    /// takes its band; and lines above the window's first, or on a surface
    /// gone, are dropped. Named breaks, one each: no split -- `ls`'s span
    /// still covers rows 2-3; no merge -- three spans of `ls`; no carry --
    /// the span stays on row 3; keep everything -- rows 0-1 stay.
    #[test]
    fn what_lit_each_line_splits_merges_carries_and_drops() {
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let at = |n: usize| AbsLine::at(Rows::of_count(n));
        let span = |a, b| LineSpan::of(at(a), at(b));
        let sorted = |lit: &Lit| {
            let mut v = lit.spans.clone();
            v.sort_by_key(|e| e.1.last());
            v
        };
        let mut lit = Lit::default();
        lit.set(Some(2), span(0, 5));
        lit.set(Some(9), span(2, 3));
        assert_eq!(
            sorted(&lit),
            [
                (Some(2), span(0, 1)),
                (Some(9), span(2, 3)),
                (Some(2), span(4, 5))
            ],
            "split"
        );
        lit.set(Some(2), span(2, 3));
        assert_eq!(sorted(&lit), [(Some(2), span(0, 5))], "merged");
        let carried = |bytes: &[u8], s: LineSpan| {
            let mut owned = Owned::new(&ws);
            let mut t = Term::new(owned.storage(), &ws);
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let rec = t.take_record();
            let lines =
                crate::paths::CoreLines::of(Some((t.history_mark().line(), Scrolled::START)))
                    .part(Some(t.history_mark().line()));
            let mut p = Paths::new(None, Forms::DEFAULT);
            p.set(Turned::On);
            let screens = [baud_core::Screen::Normal, baud_core::Screen::Alternate];
            p.carried_span(&rec, &lines, &screens, s)
        };
        let mut lit = Lit::default();
        lit.set(Some(2), span(3, 3));
        lit.carry(|s| carried(b"\x1b[3;5r\x1b[5;1H\n", s));
        assert_eq!(sorted(&lit), [(Some(2), span(2, 2))], "exact");
        let mut lit = Lit::default();
        lit.set(Some(2), span(3, 3));
        lit.carry(|s| carried(b"\x1b[3;5r\x1b[5;1H\n\x1b[3;1H\x1bM", s));
        assert_eq!(sorted(&lit), [(Some(2), span(2, 4))], "within");
        let p = Paths::new(None, Forms::DEFAULT);
        let alt = |n| LineSpan::on(p.surface(baud_core::Screen::Alternate), at(n), at(n));
        let mut lit = Lit::default();
        lit.set(Some(2), span(0, 1));
        lit.set(Some(9), span(2, 3));
        lit.set(Some(2), alt(1));
        let mut gone = Paths::new(None, Forms::DEFAULT);
        gone.alternate_cleared();
        let surfaces =
            [baud_core::Screen::Normal, baud_core::Screen::Alternate].map(|sc| gone.surface(sc));
        lit.keep_from(surfaces, Some(at(2)));
        assert_eq!(sorted(&lit), [(Some(9), span(2, 3))], "dropped");
    }

    /// **An answer naming a group twice files its verdict where the
    /// group's text is, never where it was asked** (B10): the batch named
    /// `ls` with two spans, another writer's rows between, and both have
    /// moved up a row since they were asked (rows 2 and 5, now 1 and 4).
    /// `ls`'s answer names it twice; its verdict is filed on both copies at
    /// the first, and the rows they were asked on, which hold other text
    /// now, stay not known. The answer carries no lines (`Answer::seen`),
    /// so the second naming has none of the worker's to file on. Named
    /// break: a verdict filed only on the spans of batches before its own
    /// (`mine` taking `i < id`) -- rows 1 and 4 are not known.
    #[test]
    fn a_group_named_twice_is_filed_where_its_text_is() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        let id = Id::default();
        pr.kept
            .edit()
            .inflight
            .push((id, Some(2), LineSpan::of(line(1), line(1))));
        pr.kept
            .edit()
            .inflight
            .push((id, Some(2), LineSpan::of(line(4), line(4))));
        pr.publish(&mut paths);
        let local = crate::paths::Seen::Named(Front::Local);
        pr.file_answered(
            &mut paths,
            (id, Kind::Screen),
            &[(Some(2), local), (Some(2), local)],
            &[],
        );
        let origins = [1, 2, 4, 5].map(|n| paths.origin(line(n)));
        assert_eq!(
            origins,
            [
                Origin::Local,
                Origin::Unknown,
                Origin::Local,
                Origin::Unknown
            ]
        );
    }

    /// **A click's answer files nothing** (B10): the lines it was asked
    /// with were kept to send, and a batch sent while it was out took them,
    /// carried with the text since -- asked on row 1, on row 2 now. Row 1,
    /// holding nothing filed, stays not known when the click's local
    /// verdict comes, and row 2 waits for its batch's answer. The answer
    /// carries no lines (`Answer::seen`), so none as the click asked them
    /// are filed. Named break: a click's answer taken as a batch's and
    /// filed on every span of its group, whatever batch kept it -- row 2
    /// reads local.
    #[test]
    fn a_clicks_answer_files_nothing_where_its_lines_were() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        pr.kept.edit().inflight.push((
            Id::default().next(),
            Some(2),
            LineSpan::of(line(2), line(2)),
        ));
        pr.publish(&mut paths);
        pr.file_answered(
            &mut paths,
            (Id::default(), Kind::Click),
            &[(Some(2), crate::paths::Seen::Named(Front::Local))],
            &[],
        );
        let origins = [1, 2].map(|n| paths.origin(line(n)));
        assert_eq!(origins, [Origin::Unknown, Origin::Unknown]);
    }

    /// **A worker that closes leaves every line it was still to name not
    /// known** (B10): a line kept to send and a line a sent batch was to
    /// name, both carried with their text, read as not known once the
    /// worker is closed, where the facts filed before them stay. A line
    /// whose writer is still to be named carries nothing when its text
    /// moves, so its span is the only thing that will ever file on where
    /// it went. Named break: close without filing them -- both read local
    /// under the facts filed over them.
    #[test]
    fn a_closed_worker_leaves_the_lines_it_never_named_not_known() {
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let line = |n| AbsLine::at(Rows::of_count(n));
        paths.written_under(
            crate::paths::Seen::Named(Front::Local),
            LineSpan::of(line(0), line(2)),
        );
        pr.kept
            .edit()
            .seen
            .push((9, LineSpan::of(line(1), line(1))));
        pr.kept
            .edit()
            .inflight
            .push((Id::default(), Some(9), LineSpan::of(line(2), line(2))));
        let (mut lines, mut turn) = (Lines::default(), Turn::default());
        pr.close(&mut paths, &mut lines, &mut turn);
        let origins: Vec<Origin> = (0..3).map(|n| paths.origin(line(n))).collect();
        assert_eq!(origins, [Origin::Local, Origin::Unknown, Origin::Unknown]);
    }

    /// **A read nothing names leaves the line it ended on out, where its
    /// record vouches it wrote nothing there** (W3, `vouch::Below`): a read
    /// with no group sampled and no marks prints `/etc/hosts` and a line
    /// feed, and is filed not known over its own line only -- the next
    /// line, where the cursor ended and the next read begins, takes the
    /// shell's local fact filed after. The control: the line it printed
    /// on stays not known, the same fact filed over it after included.
    /// Named break: the end line kept whatever the record vouches
    /// (`part_spans` taking nothing as clean) -- line 1 stays not known.
    #[test]
    fn a_read_nothing_names_leaves_its_clean_end_line_out() {
        use crate::reads::Part;
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let mut counts = crate::keep::Counts::new(&t);
        let bytes = b"/etc/hosts\r\n";
        let _ = t.take_record();
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        let w = Writers::default()
            .part(BytePos::ZERO, BytePos::from_index(bytes.len()))
            .recorded(Some(crate::reads::Taken {
                record: t.take_record(),
                top: t.history_mark().line(),
                end_row: crate::vouch::EndRow::of(&t),
                drew: crate::vouch::Drew::Nothing,
                recut: None,
            }));
        assert_eq!(w.before.or(w.at), None, "the control: no group");
        let rec = w.record;
        let _ = counts.numbered(&t, rec.as_ref());
        let line = |n| AbsLine::at(Rows::of_count(n));
        let on = paths.line_on(t.screen(), line(0));
        let end = paths.line_on(t.screen(), line(1));
        pr.drain_taken(
            &mut paths,
            &t,
            (on, end),
            counts.core_lines(&t),
            [(w, Linked::No)].into_iter(),
            Instant::now(),
        );
        for n in [0, 1] {
            paths.written_by_shell(LineSpan::of(line(n), line(n)));
        }
        assert_eq!(
            paths.origin(line(0)),
            Origin::Unknown,
            "the line it printed on"
        );
        assert_eq!(paths.origin(line(1)), Origin::Local, "the line it ended on");
    }

    /// The folder `paths` reads `./` against after each of `reads` is fed
    /// and filed with its record, as the drivers file a part
    /// (`Writers::recorded`).
    fn folders_after(reads: &[&[u8]]) -> Vec<Option<Vec<u8>>> {
        use crate::reads::Part;
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        paths.set_names(crate::link::MachineNames::of(vec!["here.example".into()]));
        let mut sampler = shells_sampler();
        let s = Scrolled::START;
        let t0 = Instant::now();
        let mut out = Vec::new();
        for (k, bytes) in (1..).zip(reads) {
            let began = s.line(Rows::ZERO, t.cursor().0);
            let _ = sampler.read(t0 + GROUP_EVERY * k, bytes);
            let _ = t.take_record();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let w = sampler
                .unasked()
                .part(BytePos::ZERO, BytePos::from_index(bytes.len()))
                .recorded(Some(crate::reads::Taken {
                    record: t.take_record(),
                    top: t.history_mark().line(),
                    end_row: crate::vouch::EndRow::of(&t),
                    drew: crate::vouch::Drew::Nothing,
                    recut: None,
                }));
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.read_taken(&mut paths, w, LineSpan::of(began, ended), Linked::No);
            let _ = paths.observe(&t, s);
            out.push(paths.folder_now().map(<[u8]>::to_vec));
        }
        out
    }

    /// **A report the core refuses after a remote program's forged one
    /// leaves no folder, never the forged one**: the shell reports
    /// `/home/u`; under `ssh h` a remote program reports `/tmp`, which the
    /// core keeps and the app refuses; then, after the shell's `D`, the
    /// shell's report the core cannot read (`file://` with no path), which
    /// clears the core's. The app's folder is the shell's, then still the
    /// shell's, then none -- what the core holds after its refusal, never
    /// the forged one it held before. Named break: a report the core
    /// refused filed as the app's refusal (`folder_of` judging the record's
    /// `Refused` remote) -- the app keeps `/home/u`, a folder the shell
    /// has since said it left.
    #[test]
    fn a_refused_report_after_a_forged_one_leaves_no_folder() {
        let home = Some(b"/home/u".to_vec());
        let got = folders_after(&[
            b"\x1b]133;A;aid=1\x07\x1b]7;file://here.example/home/u\x07% ",
            &ran("ssh h", ";cmdline=ssh\\ h"),
            b"\x1b]7;file://here.example/tmp\x07remote$ ",
            &[prompt_after("").as_slice(), b"\x1b]7;file://\x07"].concat(),
        ]);
        assert_eq!(got, [home.clone(), home.clone(), home, None]);
    }

    /// **A printed path reaches the system only through `lstat` and
    /// `readlink`**, as SECURITY.md says: of the worker's seam (`Fs`), the
    /// only methods that take a path are those two, and the real one
    /// (`System`) answers them with `std::fs::symlink_metadata` and
    /// `std::fs::read_link` -- neither opens the file nor follows a link --
    /// and calls no other `std::fs` function but the read of Linux's mount
    /// table. The worker is a thread no test drives against the real file
    /// system, so the source is what can fail. Named breaks: a method
    /// taking a path added to `Fs`; `lstat` answered by
    /// `std::fs::metadata`, which follows a link.
    #[test]
    fn a_printed_path_reaches_the_system_only_through_lstat_and_readlink() {
        let src = include_str!("probe.rs");
        let body = |open: &str| {
            let rest = src.split_once(open).expect(open).1;
            &rest[..rest.find("\n}\n").expect("its end")]
        };
        let path_fns = |b: &str| -> Vec<String> {
            b.split(concat!("\n    fn ", ""))
                .skip(1)
                .filter(|f| {
                    f.split_once(')')
                        .is_some_and(|(sig, _)| sig.contains(": &[u8]"))
                })
                .map(|f| f.split('(').next().unwrap_or_default().to_string())
                .collect()
        };
        let seam = body(concat!("pub trait ", "Fs: Send {"));
        assert_eq!(
            path_fns(seam),
            ["lstat", "readlink"],
            "the seam's path methods"
        );
        let real = body(concat!("impl Fs ", "for System {"));
        assert_eq!(path_fns(real), ["lstat", "readlink"], "the real one's");
        let calls: Vec<&str> = real
            .match_indices(concat!("std::", "fs::"))
            .map(|(at, _)| {
                let call = &real[at..];
                &call[..call.find('(').unwrap_or(call.len())]
            })
            .collect();
        assert_eq!(
            calls,
            [
                concat!("std::", "fs::symlink_metadata"),
                concat!("std::", "fs::read_link"),
                concat!("std::", "fs::read"),
            ],
            "the calls the real one makes"
        );
        assert!(
            real.contains(concat!("std::", "fs::read(\"/proc/self/mountinfo\")")),
            "the one read: the mount table"
        );
    }

    /// **iTerm2's `CurrentDir` is a folder report like `OSC 7`**, taken from
    /// the shell and refused from a remote command, which the scan of `OSC
    /// 7` alone never saw: the core records it (`ReadRecord::folders`). The
    /// shell's `CurrentDir=/srv/w`, on the host its `RemoteHost` names, is
    /// taken; under `ssh h` a remote program's `CurrentDir=/tmp` is not. Named break: judge every report
    /// as the shell's (`folder_of` ignoring what was open) -- the remote
    /// one is taken.
    #[test]
    fn an_iterm_current_dir_is_judged_as_osc_7_is() {
        let home = Some(b"/srv/w".to_vec());
        let got = folders_after(&[
            b"\x1b]133;A;aid=1\x07\x1b]1337;RemoteHost=here.example\x07\x1b]1337;CurrentDir=/srv/w\x07% ",
            &ran("ssh h", ";cmdline=ssh\\ h"),
            b"\x1b]1337;RemoteHost=here.example\x07\x1b]1337;CurrentDir=/tmp\x07remote$ ",
        ]);
        assert_eq!(got, [home.clone(), home.clone(), home]);
    }

    /// **An image a remote read placed after its last line feed leaves its
    /// row the read's** (`vouch::Drew`): under `ssh h`, a read of a line,
    /// its line feed and a Sixel there, the Sixel in a part of its own
    /// between a log's start and end (`?46`), whose record is quiet -- the
    /// image moves no cursor here and leaves the row's mark at 0 -- then
    /// the shell's `D` and a prompt with `/etc/hosts` typed on that row.
    /// The row stays dark. The control: no Sixel, the prompt's path lights.
    /// Named breaks: a quiet part that drew pushes no tag of its own --
    /// only the part before, which vouched the row clean, is filed; and
    /// `Drew::of` always `Nothing`.
    #[test]
    fn an_image_after_a_remote_reads_last_line_feed_keeps_its_row() {
        let (start, prompt) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after("/etc/hosts"));
        let lit = |remote: &[u8]| {
            !lit_reads(
                named_groups(),
                &[read(&start, 1), read(remote, 9), read(&prompt, 1)],
            )
            .is_empty()
        };
        assert!(lit(b"x\r\n\x1b[?46h\x1b[?46l"), "the control");
        assert!(!lit(
            b"x\r\n\x1b[?46h\x1bPq#0;2;100;0;0#0~~~~\x1b\\\x1b[?46l"
        ));
    }

    /// **A line feed that cannot move the cursor leaves its row the
    /// read's** (H3): under `ssh h`, a scrolling region over the top rows
    /// and the cursor on the row above the last; then a read of a line
    /// feed, a path on the last row, and a carriage return and line feed --
    /// which, the cursor on the last row below the region, does not move it
    /// (`Feeds::Stuck`), and leaves the path on the row it ends on. The row
    /// stays the remote read's, so the shell's prompt written there next
    /// does not light the path; nor do the shell's `D` and prompt in the
    /// same read, sampled with the shell in front, where the stuck feed
    /// refuses the cut. The control: a row higher, the last line feed
    /// moves. Named breaks: vouch the line a read ended on clean whatever
    /// its feeds and its row (`vouch::Below`) -- the row lights after the
    /// prompt's own read; and with it, a cut whatever its feeds
    /// (`vouch::CutAt`) -- the row lights behind the cut. Either verdict's
    /// stuck feed alone is held by the row the read left the path on.
    #[test]
    fn a_line_feed_that_does_not_move_leaves_its_row_the_reads() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let (start, prompt) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after(""));
        let path = b"\n   /etc/passwd\r\n".as_slice();
        let at = |region: &[u8], one: bool| {
            let ws = WinSize {
                ws_col: ws.ws_col,
                ws_row: ws.ws_row,
            };
            let both = [path, &prompt].concat();
            let mut reads = vec![read(&start, 1), read(region, 9)];
            match one {
                false => reads.extend([read(path, 9), read(&prompt, 1)]),
                true => reads.push(read(&both, 1)),
            }
            taken_on(ws, named_groups(), &reads).0
        };
        for one in [false, true] {
            assert_eq!(
                at(b"\x1b[1;5r\x1b[8;1H", one),
                Vec::<usize>::new(),
                "the control: a row higher, the last line feed moves ({one})"
            );
            assert_eq!(
                at(b"\x1b[1;5r\x1b[9;1H", one),
                Vec::<usize>::new(),
                "stuck ({one})"
            );
        }
    }

    /// **A resize that re-wraps lines leaves text whose writer baud could
    /// not name dark** (H4): a local line wrapped over rows 0 and 1, then
    /// `/etc/passwd` on row 2 filed under no writer, then a local prompt;
    /// the window widened, the wrapped line takes one row and the path
    /// moves up to row 1, which was filed local -- and the re-cut, filed at
    /// the resize as the drivers file it (`Probes::resized`), carries each
    /// line's facts with its text, and the path's line had none. Named
    /// break: the spans still to be named left where they were
    /// (`Probes::recut` carrying none) -- after the resize, a row lights.
    #[test]
    fn a_resize_moves_unnamed_text_and_it_stays_dark() {
        let narrow = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let wrapped = [[b'x'; 30].as_slice(), b"\r\n"].concat();
        let resized = |then: Then| {
            let ws = WinSize {
                ws_col: narrow.ws_col,
                ws_row: narrow.ws_row,
            };
            let reads = [
                read(&wrapped, 1),
                Read {
                    sampled: Sampled::Unfiled,
                    ..read(b"/etc/passwd\r\n", 1)
                },
                Read {
                    then,
                    ..read(b"$ ", 1)
                },
            ];
            lit_driven(ws, named_groups(), &reads)
        };
        assert_eq!(
            resized(Then::Nothing),
            Vec::<usize>::new(),
            "the control: no resize"
        );
        assert_eq!(
            resized(Then::Resize(40, 6)),
            Vec::<usize>::new(),
            "the path the re-cut moved"
        );
    }

    /// **A widen that brings a lit history line back into the window
    /// leaves an unnamed writer's overwrite of it held** (G1 over a
    /// re-cut): `ls` prints `/etc/hosts` and two lines wrapped over two
    /// rows each at twenty columns, named, so the path's line is in the
    /// history; `ls`'s next read trims what lit each line to the window;
    /// the window widens to forty, each wrapped line takes one row, and the
    /// path's line is back in the window with its local fact carried and
    /// nothing naming what lit it; then `ssh`, still to be named, moves to
    /// it and prints `/etc/passwd`. The line is not known before `ssh`'s
    /// answer. The control: the widen brought the line back -- `ssh`'s move
    /// to the first row lands on it. Named break: hold only the lines
    /// another group is named for (`Lit`'s spans of other groups) -- the
    /// line reads local.
    #[test]
    fn a_widen_that_brings_a_lit_line_back_holds_an_unnamed_overwrite() {
        let narrow = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let wrapped = [
            b"/etc/hosts\r\n".as_slice(),
            &[b'x'; 30],
            b"\r\n",
            &[b'y'; 30],
        ]
        .concat();
        let reads = [
            Read {
                then: Then::Answered,
                ..read(&wrapped, 2)
            },
            Read {
                then: Then::Resize(40, 4),
                ..read(b"\x1b[m", 2)
            },
            Read {
                then: Then::Took,
                ..read(b"\x1b[1;1H/etc/passwd", 9)
            },
        ];
        let t = taken_midway(
            narrow,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |t, _| holding(t, "/etc/passwd"),
        );
        assert_eq!(t.looked, [0], "the control: ssh's path on the first row");
        assert_eq!(
            t.midway_origins[0][0],
            Origin::Unknown,
            "before ssh's answer"
        );
    }

    /// **A program's own page-width request does not light its overwrite
    /// of a history line the widen brings back** (G1 over a re-cut, the
    /// one-read form): `ls` prints `/etc/hosts`, a line wrapped over three
    /// rows at forty columns and `/etc/group`, named, so the first path's
    /// line is in the history; `ls`'s next read trims what lit each line to
    /// the window; then `ssh`, still to be named, in one read asks for 132
    /// columns -- the wrapped line takes one row and the path's line is back
    /// in the window -- moves to the first row and prints `/etc/passwd` over
    /// it.
    /// The line is not known before `ssh`'s answer. The control: the
    /// request resized and `ssh`'s path is on the first row. Named break:
    /// hold only the lines another group is named for (`Lit`'s spans of
    /// other groups) -- the line reads local.
    #[test]
    fn a_programs_own_widen_does_not_light_its_overwrite() {
        let wrapped = [
            b"/etc/hosts\r\n".as_slice(),
            &[b'x'; 100],
            b"\r\n/etc/group",
        ]
        .concat();
        let reads = [
            Read {
                then: Then::Answered,
                ..read(&wrapped, 2)
            },
            read(b"\x1b[m", 2),
            Read {
                then: Then::Took,
                ..read(b"\x1b[132$|\x1b[1;1H/etc/passwd", 9)
            },
        ];
        let t = taken_midway(
            WS,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |t, _| {
                (
                    t.geometry().0.index(),
                    holding(t, "/etc/passwd"),
                    holding(t, "/etc/group"),
                )
            },
        );
        assert_eq!(
            t.looked,
            (132, vec![0], vec![2]),
            "the control: resized, ssh's path on row 0"
        );
        assert_eq!(
            t.midway_origins[0][0],
            Origin::Unknown,
            "before ssh's answer"
        );
    }

    /// **A re-cut while `[link] paths` is off is followed by nothing**:
    /// `ls` prints a full row ending `fi`, named local; paths go off; `ssh`
    /// writes `le:///etc/passwd` on, wrapped onto the next row -- a read not
    /// kept, which files nothing -- and the window widens, re-cutting both
    /// rows into one line. That line is not known: under paths off, a line
    /// nothing was filed over may be one a read not kept wrote, so the
    /// re-cut floors as one no map follows does. The control: before the
    /// widen, the continuation's row is not known and the row above it
    /// local. Named break: follow the re-cut whatever the paths are
    /// (`Probes::resized` taking any `Marks`) -- the joined line reads
    /// local.
    #[test]
    fn a_re_cut_with_paths_off_is_followed_by_nothing() {
        let narrow = WinSize {
            ws_col: 20,
            ws_row: 4,
        };
        let row = [[b'x'; 17].as_slice(), b" fi"].concat();
        let origins = |then: Then| {
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(&row, 2)
                },
                Read {
                    then: Then::Paths(Turned::Off),
                    ..read(b"\x1b[m", 2)
                },
                Read {
                    then,
                    ..read(b"le:///etc/passwd", 9)
                },
            ];
            let narrow = WinSize { ..narrow };
            taken_numbered(
                narrow,
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |_, p| [0, 1].map(|n| p.origin(AbsLine::at(Rows::of_count(n)))),
            )
            .2
        };
        assert_eq!(
            origins(Then::Nothing),
            [Origin::Local, Origin::Unknown],
            "the control: no widen"
        );
        assert_eq!(
            origins(Then::Resize(40, 4))[0],
            Origin::Unknown,
            "re-cut into one line"
        );
    }

    /// **A re-cut carries each line's writer to where its text went** (C):
    /// a local line wrapped over two rows at twenty columns, `ls`'s
    /// `/etc/passwd` below it and `ssh`'s `/etc/hosts` below that, each
    /// named, then the window widened to forty -- the wrapped line takes one
    /// row and both paths move up one -- and narrowed back to twenty. The
    /// local path lights on its row each time, and the remote one stays
    /// dark. The control: no resize. Named breaks: `Probes::resized` floors
    /// the lines as a re-cut no map follows does (`Paths::resized`) --
    /// nothing lights; every new line filed local whatever its text's
    /// writer (`Fact::carried`) -- the remote path lights.
    #[test]
    fn a_re_cut_carries_each_lines_writer_to_where_its_text_went() {
        let narrow = || WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let wrapped = [[b'x'; 30].as_slice(), b"\r\n"].concat();
        let lit = |thens: &[Then]| {
            let mut reads = vec![
                read(&wrapped, 2),
                read(b"/etc/passwd\r\n", 2),
                Read {
                    then: Then::Answered,
                    ..read(b"/etc/hosts\r\n", 9)
                },
            ];
            reads.extend(thens.iter().map(|&then| Read {
                then,
                ..read(b"", 1)
            }));
            lit_driven(narrow(), named_groups(), &reads)
        };
        assert_eq!(lit(&[]), [2], "the control: no resize");
        assert_eq!(lit(&[Then::Resize(40, 6)]), [1], "widened");
        assert_eq!(
            lit(&[Then::Resize(40, 6), Then::Resize(20, 6)]),
            [2],
            "widened and narrowed back"
        );
    }

    /// **An answer still out across a re-cut is filed where the re-cut took
    /// its lines** (C): [`a_re_cut_carries_each_lines_writer_to_where_its_text_went`]'s
    /// reads with no answer taken before the window widens, so every group
    /// is still to be named as the lines move. The local path lights on the
    /// row it moved to and the remote one stays dark: each new line is held
    /// under the group whose span covered its text (`Paths::reflowed`), and
    /// the answer files on its holds as on its spans. Named break:
    /// `Probes::resized` floors the lines as a re-cut no map follows does
    /// -- the path stays dark.
    #[test]
    fn an_answer_still_out_is_filed_where_the_re_cut_took_its_lines() {
        let narrow = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let wrapped = [[b'x'; 30].as_slice(), b"\r\n"].concat();
        let reads = [
            read(&wrapped, 2),
            read(b"/etc/passwd\r\n", 2),
            Read {
                then: Then::Resize(40, 6),
                ..read(b"/etc/hosts\r\n", 9)
            },
        ];
        assert_eq!(lit_driven(narrow, named_groups(), &reads), [1]);
    }

    /// **What following a re-cut costs, over a full ring** (C, M2): the
    /// app's ring -- 2,000 lines of history, `Depth::DEFAULT`, behind a
    /// window of 160x50 in storage of 400x150 -- full of lines of 150
    /// columns, each line's writer alternating local and remote so every
    /// line is a run of its own, eight spans still to be named; the window
    /// narrowed to 100 columns and widened back, 11 rounds, the first not
    /// counted. Timed apart: the core's resize telling nothing
    /// (`Term::resize`), the same resize telling every line
    /// (`Recuts::resize`, its buffer reused), and the app's following of
    /// it (`Probes::resized`: `Paths::reflowed` and every span's carry).
    /// Run by hand (`--ignored --nocapture`) for the record; it asserts the
    /// counts.
    #[test]
    #[ignore]
    fn measure_following_a_re_cut_over_a_full_ring() {
        let room = WinSize {
            ws_col: 400,
            ws_row: 150,
        };
        let wide = WinSize {
            ws_col: 160,
            ws_row: 50,
        };
        let narrow = WinSize {
            ws_col: 100,
            ws_row: 50,
        };
        let mut owned = Owned::with_history(
            &room,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::DEFAULT,
        );
        let mut t = Term::new(owned.storage(), &wide);
        let text: Vec<u8> = (0..3_000)
            .flat_map(|i| format!("{i:05}{}\r\n", "z".repeat(145)).into_bytes())
            .collect();
        t.write_at(BytePos::ZERO, &text);
        t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
        let _ = t.take_record();
        let mut counts = crate::keep::Counts::new(&t);
        let _ = counts.numbered(&t, None);
        let (mut pr, _) = probes(named_groups());
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        for n in 0..3_000usize {
            let at = AbsLine::at(Rows::of_count(n));
            match n % 2 {
                0 => paths.written_by_shell(LineSpan::of(at, at)),
                _ => paths.written_remote(LineSpan::of(at, at)),
            }
        }
        for g in 0..8usize {
            let at = AbsLine::at(Rows::of_count(2_000 + g * 100));
            let span = LineSpan::of(at, at.below());
            pr.kept
                .edit()
                .seen
                .push((Pid::try_from(10 + g).expect("a pid"), span));
        }
        pr.publish(&mut paths);
        let mut recuts = crate::paths::Recuts::default();
        let mut times = [Vec::new(), Vec::new(), Vec::new()];
        let mut told = 0;
        let mut sizes = Vec::new();
        for round in 0..11 {
            for ws in [&narrow, &wide] {
                // The core alone, telling nothing: a copy to resize.
                let mut plain_owned = Owned::with_history(
                    &room,
                    &baud_core::CacheGeometry::UNKNOWN,
                    baud_core::Depth::DEFAULT,
                );
                let mut plain = Term::new(plain_owned.storage(), &wide);
                plain.write_at(BytePos::ZERO, &text);
                plain.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
                if ws.ws_col == wide.ws_col {
                    let _ = plain.resize(&narrow);
                }
                let t0 = Instant::now();
                let _ = plain.resize(ws);
                let plain_took = t0.elapsed();
                let was = (t.geometry().0, t.screen());
                let was_last = crate::paths::last_line(&t, counts.primary());
                recuts.clear();
                let t1 = Instant::now();
                let _ = recuts.resize(&mut t, ws);
                let told_took = t1.elapsed();
                let rec = t.take_record();
                let _ = crate::keep::kept_across_resize(&t, was, None, &mut counts, &rec);
                let map = recuts.last().expect("held");
                told = map.map.len();
                let t2 = Instant::now();
                pr.resized(&mut paths, (&rec, Some(map)), &t, was_last, &counts);
                let followed_took = t2.elapsed();
                if round > 0 {
                    sizes.push((told, rec.renumbered().is_some()));
                    times[0].push(plain_took);
                    times[1].push(told_took);
                    times[2].push(followed_took);
                }
            }
        }
        // At 100 columns each line takes two rows, so about half as many
        // lines as the ring has rows are left to tell.
        assert!(told > 1_000, "the control: a full ring told, {told} lines");
        assert_eq!(pr.kept.get().seen.len(), 8, "the control: the spans kept");
        assert!(
            sizes.iter().all(|&(n, re_cut)| n > 1_000 && re_cut),
            "the control: every resize re-cut a full ring: {sizes:?}"
        );
        for (name, mut ts) in ["Term::resize", "Recuts::resize", "Probes::resized"]
            .into_iter()
            .zip(times)
        {
            ts.sort();
            let (lo, mid, hi) = (ts[0], ts[ts.len() / 2], ts[ts.len() - 1]);
            println!(
                "{name}: median {mid:?}, range {lo:?}..{hi:?}, {} resizes",
                ts.len()
            );
        }
    }

    /// **What handing the lines still to be named to `paths` costs while a
    /// re-cut's holds stand** (`Probes::publish`, `Paths::set_pending`):
    /// `cat-scroll.raw` fed at 200x50 in reads of 4096 bytes through the
    /// app's ring -- 2,000 lines of history, `Depth::DEFAULT`, storage of
    /// 400x150 -- every read the same group's and no batch sent, so one
    /// span still to be named covers every line written; after half the
    /// recording the window narrows to 100 columns, and the re-cut holds
    /// each new line that span covered under its group (`Probes::recut`).
    /// Then `FRAMES` more reads: each drain's own publishes, counted and
    /// timed inside `publish`, with the span still to be named grown by the
    /// read and the holds unchanged. Then `PROBE_PUBLISH_REPS` more (50
    /// when unset) over the last state, both lists unchanged. Printed
    /// beside the times: the publishes, the lines the re-cut told, and at
    /// each frame the holds and the spans handed (still to be named, holds
    /// included). `set_pending`'s compares are not counted. Only `publish`
    /// is timed: `drain_recut` checks `pending_is` in a test build, which
    /// the app does not. `PROBE_BENCH_ARM` labels the summary line, so arms
    /// run in turn read apart. Run by hand (`--release --ignored
    /// --nocapture`); it asserts the counts.
    #[test]
    #[ignore]
    fn measure_publish_over_lines_held_by_a_re_cut() {
        const FRAMES: usize = 20;
        const CHUNK: usize = 4096;
        let room = WinSize {
            ws_col: 400,
            ws_row: 150,
        };
        let ws = WinSize {
            ws_col: 200,
            ws_row: 50,
        };
        let narrow = WinSize {
            ws_col: 100,
            ws_row: 50,
        };
        let reps = std::env::var("PROBE_PUBLISH_REPS").map_or(50, |v| {
            v.parse::<usize>().expect("PROBE_PUBLISH_REPS is a count")
        });
        let arm = std::env::var("PROBE_BENCH_ARM").unwrap_or_else(|_| "-".to_string());
        let recording = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/recordings/ascii/cat-scroll.raw"
        ))
        .expect("the recording");
        let chunks: Vec<&[u8]> = recording.chunks(CHUNK).collect();
        let half = chunks.len() / 2;
        assert!(half > FRAMES, "the control: reads past the re-cut");
        let mut owned = Owned::with_history(
            &room,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::DEFAULT,
        );
        let mut t = Term::new(owned.storage(), &ws);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let mut s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let mut sampler = Sampler::default();
        let (mut pr, _) = probes(Fake::default());
        let mut pace = crate::reads::Pace::new();
        let t0 = Instant::now();
        let mut told = 0;
        // Each frame's publishes, their nanoseconds, the holds and the
        // spans handed after it.
        let mut frames = Vec::with_capacity(FRAMES);
        for (i, &bytes) in chunks.iter().take(half + FRAMES).enumerate() {
            let now = t0 + GROUP_EVERY * u32::try_from(i + 1).expect("a count");
            let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let _ = sampler.read(now, bytes);
            let taken = sampler.writers(Some(2), Watched::Nothing);
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                [(bytes, taken)],
                |_| {},
                |t, r| page_width(t, &ws, r),
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let last = took.iter().rev().find_map(|w| w.record);
            s = counts.numbered(&t, last.as_ref()).scrolled();
            let core = counts.core_lines(&t);
            let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
            let calls = pr.publishes.load(Ordering::Relaxed);
            let ns = pr.publish_ns.load(Ordering::Relaxed);
            pr.drain_recut(
                &mut paths,
                &t,
                (began, ended),
                core,
                took.into_iter().map(|w| (w, Linked::No)),
                &pace.recuts,
                now,
            );
            if i >= half {
                frames.push((
                    pr.publishes.load(Ordering::Relaxed) - calls,
                    pr.publish_ns.load(Ordering::Relaxed) - ns,
                    pr.held.get().len(),
                    pr.pending().count(),
                ));
            }
            if i + 1 == half {
                let was = (t.geometry().0, t.screen());
                let was_last = crate::paths::last_line(&t, counts.primary());
                pace.recuts.clear();
                let _ = pace.recuts.resize(&mut t, &narrow);
                let rec = t.take_record();
                s = crate::keep::kept_across_resize(&t, was, None, &mut counts, &rec).1;
                let map = pace.recuts.last();
                told = map.as_ref().map_or(0, |m| m.map.len());
                pr.resized(&mut paths, (&rec, map), &t, was_last, &counts);
                assert!(
                    pr.held.get().len() > 1_000,
                    "the control: the re-cut held its lines, {} held of {told} told",
                    pr.held.get().len()
                );
            }
        }
        let mut still = Vec::with_capacity(reps);
        for _ in 0..reps {
            let ns = pr.publish_ns.load(Ordering::Relaxed);
            pr.publish(&mut paths);
            still.push(pr.publish_ns.load(Ordering::Relaxed) - ns);
        }
        assert_eq!(frames.len(), FRAMES, "the control: every frame read");
        assert!(
            frames.iter().all(|f| f.0 > 0),
            "the control: every drain published"
        );
        for (k, (calls, ns, held, spans)) in frames.iter().enumerate() {
            println!("frame {k}: {calls} publishes, {ns} ns, {held} held, {spans} spans handed");
        }
        let spread = |ns: &mut [u64]| {
            ns.sort_unstable();
            match (ns.first(), ns.get(ns.len() / 2), ns.last()) {
                (Some(&lo), Some(&mid), Some(&hi)) => (lo, mid, hi),
                _ => (0, 0, 0),
            }
        };
        let mut per_frame: Vec<u64> = frames.iter().map(|f| f.1).collect();
        let (f_lo, f_mid, f_hi) = spread(&mut per_frame);
        let (s_lo, s_mid, s_hi) = spread(&mut still);
        let calls: u64 = frames.iter().map(|f| f.0).sum();
        let (held_first, (_, _, held_last, spans_last)) = (frames[0].2, frames[FRAMES - 1]);
        println!(
            "publish_held arm={arm} told={told} frames={FRAMES} frame_publishes={calls} \
             held_first={held_first} held_last={held_last} spans_last={spans_last} \
             frame_ns_p50={f_mid} frame_ns_min={f_lo} frame_ns_max={f_hi} still_reps={reps} \
             still_ns_p50={s_mid} still_ns_min={s_lo} still_ns_max={s_hi}"
        );
    }

    /// **A re-cut carries every hold with its text** (C): `ls` prints a
    /// line of thirty columns, `/etc/hosts` and `/etc/passwd` at forty
    /// columns, named; `ssh`, still to be named, writes `/etc/group` over
    /// the path's row, which holds it; the window narrows to twenty, the
    /// long line takes two rows, and `/etc/hosts` moves down onto the
    /// number the held row had. Before `ssh`'s answer, `/etc/hosts` reads
    /// local where it is now, and `ssh`'s row, one further down, is held.
    /// Named break: the spans kept of the lines left where they were
    /// (`Probes::recut` carrying none) -- the hold stays on its old number
    /// and `/etc/hosts` reads not known.
    #[test]
    fn a_re_cut_carries_every_hold_with_its_text() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let printed = [[b'x'; 30].as_slice(), b"\r\n/etc/hosts\r\n/etc/passwd\r\n"].concat();
        let reads = [
            Read {
                then: Then::Answered,
                ..read(&printed, 2)
            },
            Read {
                then: Then::Resize(20, 6),
                ..read(b"\x1b[3;1H/etc/group", 9)
            },
            Read {
                then: Then::Took,
                ..read(b"", 1)
            },
        ];
        let t = taken_midway(
            ws,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |t, _| (holding(t, "/etc/hosts"), holding(t, "/etc/group")),
        );
        assert_eq!(t.looked, (vec![2], vec![3]), "the control: both moved down");
        assert_eq!(
            [t.midway_origins[0][2], t.midway_origins[0][3]],
            [Origin::Local, Origin::Unknown]
        );
    }

    /// **A quiet part after a page-width request is not lost to the
    /// dedupe** (D4): `ls` prints a line wrapped over two rows at forty
    /// columns and `$ /etc/hosts` after it, leaving the cursor on it,
    /// named; then `ssh`, still to be named, in one read asks for 132
    /// columns -- the wrapped line takes one row and the cursor's line
    /// moves up one -- and returns to the start of the line to print
    /// `/etc/passwd` over it. That last part's record is quiet, its cursor
    /// on the line it began on, and its tag the resize's part's, so it is
    /// not a part of its own and the resize's part, whose record says it
    /// renumbered, names no line. It is not lost all the same: a quiet part
    /// writes only on the cursor's line, which the asking part named as
    /// the line it began on, and the re-cut carries that span to where the
    /// cursor's text went. Not known before `ssh`'s answer. The control:
    /// the request resized and `ssh`'s path is on the cursor's line, moved
    /// up. Named break: the spans kept of the lines left where they were
    /// (`Probes::recut` carrying none) -- the asking part's span stays on
    /// the old number and the line reads local.
    #[test]
    fn a_quiet_part_after_a_page_width_request_is_filed() {
        let printed = [[b'x'; 60].as_slice(), b"\r\n$ /etc/hosts"].concat();
        let reads = [
            Read {
                then: Then::Answered,
                ..read(&printed, 2)
            },
            Read {
                then: Then::Took,
                ..read(b"\x1b[132$|\r/etc/passwd ", 9)
            },
        ];
        let t = taken_midway(
            WS,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |t, _| (t.geometry().0.index(), holding(t, "/etc/passwd")),
        );
        assert_eq!(
            t.looked,
            (132, vec![1]),
            "the control: resized, ssh's path on row 1"
        );
        assert_eq!(
            t.midway_origins[0][1],
            Origin::Unknown,
            "before ssh's answer"
        );
    }

    /// **A cut a re-cut came among is no cut** (D4, `cut::Spot`): under
    /// `ssh`, run from the shell, which is in front by the read, a read of
    /// `/etc/hosts`, the shell's `D` and prompt, and then
    /// requests for 132 columns and back to 80, with `/etc/passwd` typed
    /// after them. The cut's line was renumbered twice, and the grid ends
    /// the size it was, which `Spot::line`'s checks would pass: the parts
    /// from the cut on are filed as they would be uncut, under the remote
    /// command's verdict, so no row lights. The control: the same read with no
    /// requests lights the typed path. Named break: the cut taken in the
    /// run the batch ends with whatever came before it
    /// (`Probes::drain_segment` taking any cut there) -- a row lights.
    #[test]
    fn a_cut_a_re_cut_came_among_is_no_cut() {
        let ws = WinSize {
            ws_col: 80,
            ws_row: 6,
        };
        let lit = |requests: &[u8]| {
            let remote = [
                b"/etc/hosts\r\n".as_slice(),
                &prompt_after(""),
                requests,
                b"/etc/passwd",
            ]
            .concat();
            let start = ran("ssh h", ";cmdline=ssh\\ h");
            let reads = [read(&start, 1), read(&remote, 1)];
            let ws = WinSize { ..ws };
            taken_numbered(
                ws,
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |t, _| t.geometry().0.index(),
            )
        };
        let (plain, _, cols) = lit(b"");
        assert_eq!(
            (plain.len(), cols),
            (1, 80),
            "the control: the typed path lights"
        );
        let (recut, _, cols) = lit(b"\x1b[132$|\x1b[80$|");
        assert_eq!(
            (recut, cols),
            (vec![], 80),
            "re-cut twice, the grid restored"
        );
    }

    /// The rows of the screen up whose text holds `what`.
    fn holding(t: &Term<'_>, what: &str) -> Vec<usize> {
        (0..t.geometry().1.index())
            .filter(|&y| {
                let row = t.row(baud_domain::Row::from_index(y)).expect("a row");
                let text: String = row
                    .iter()
                    .filter_map(|c| c.content.as_codepoint())
                    .filter_map(|c| char::from_u32(c.value()))
                    .collect();
                text.contains(what)
            })
            .collect()
    }

    /// **A page-width request among a read's parts is followed by what its
    /// re-cut told** (C, D4): a line wrapped over two rows filed local,
    /// `ssh`'s `/etc/hosts` and the shell's prompt after it, named; then
    /// `ls` printing `/etc/passwd`, asking for 132 columns and printing
    /// `/etc/passwd` again. The part before the
    /// request is filed on the numbers before the re-cut and carried, the
    /// part after it on the numbers it made: both paths light where they
    /// are drawn, the remote one stays dark, and line 0, the wrapped line,
    /// keeps its local writer. Named break: the drain filed with no re-cut
    /// held (`Recuts::NONE`) -- nothing lights, and line 0 is not known, as
    /// a re-cut no map follows leaves them.
    #[test]
    fn a_page_width_request_among_a_reads_parts_is_followed() {
        let wrapped = [[b'x'; 60].as_slice(), b"\r\n"].concat();
        let reads = [
            read(&wrapped, 2),
            read(b"/etc/hosts\r\n", 9),
            Read {
                then: Then::Answered,
                ..read(b"$ \r\n", 1)
            },
            read(b"/etc/passwd\r\n\x1b[132$|/etc/passwd", 2),
        ];
        let (lit, _, (wide, local, remote, first)) = taken_numbered(
            WS,
            named_groups(),
            &reads,
            None,
            Numbering::Driven,
            |t, p| {
                (
                    t.geometry().0.index(),
                    holding(t, "/etc/passwd"),
                    holding(t, "/etc/hosts"),
                    p.origin(AbsLine::at(Rows::ZERO)),
                )
            },
        );
        assert_eq!(wide, 132, "the control: the request resized");
        assert_eq!(local.len(), 2, "the control: both paths up: {local:?}");
        assert_eq!(remote.len(), 1, "the control: the remote one up");
        assert_eq!(lit, local, "the local paths, and only they");
        assert_eq!(first, Origin::Local, "line 0, carried");
    }

    /// **The bytes that ask for a page width are filed on the numbers they
    /// were written at** (C, D4): `ls` prints `/etc/passwd` on row 0, named
    /// local; then `ssh` writes `/etc/hosts` over it and asks for 132
    /// columns in the same part. Its record is taken before the resize, so
    /// the remote writer is filed over row 0 before the re-cut carries it:
    /// the row stays dark. The control: the same bytes with no request
    /// lights nothing either. Named break: the part's record taken after
    /// the resize, as one record (`reads::paced_one`) -- it says the lines
    /// were renumbered and names none, and row 0 carries the local writer
    /// and lights.
    #[test]
    fn the_bytes_that_ask_for_a_page_width_are_filed_before_its_re_cut() {
        let lit = |ask: &[u8]| {
            let over = [b"\x1b[1;1H/etc/hosts  ".as_slice(), ask].concat();
            let reads = [
                Read {
                    then: Then::Answered,
                    ..read(b"/etc/passwd\r\n", 2)
                },
                read(&over, 9),
            ];
            taken_numbered(
                WS,
                named_groups(),
                &reads,
                None,
                Numbering::Driven,
                |t, _| t.geometry().0.index(),
            )
        };
        let (plain, _, cols) = lit(b"");
        assert_eq!((plain, cols), (vec![], 40), "the control: no request");
        let (asked, _, cols) = lit(b"\x1b[132$|");
        assert_eq!((asked, cols), (vec![], 132), "the request");
    }

    /// **What each of the primary's first twelve lines reads as after a
    /// re-cut told as `edit` leaves the map** (`Probes::resized`): six
    /// lines of thirty columns filed local on a 40x4 window, narrowed to
    /// twenty, so each takes two lines, then the shell's prompt filed local
    /// over line `prompt`, as the shell redraws it after the resize.
    fn after_recut(
        edit: impl FnOnce(&mut Vec<baud_core::LineRecut>) -> baud_core::Following,
        prompt: Rows,
    ) -> Vec<Origin> {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 4,
        };
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let mut counts = crate::keep::Counts::new(&t);
        let text: Vec<u8> = (0..6)
            .flat_map(|i| format!("{i}{}\r\n", "y".repeat(29)).into_bytes())
            .collect();
        t.write_at(BytePos::ZERO, &text);
        t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
        let _ = t.take_record();
        let _ = counts.numbered(&t, None);
        paths.all_written_here();
        let was = (t.geometry().0, t.screen());
        let was_last = crate::paths::last_line(&t, counts.primary());
        let mut recuts = crate::paths::Recuts::default();
        let _ = recuts.resize(
            &mut t,
            &WinSize {
                ws_col: 20,
                ws_row: 4,
            },
        );
        let rec = t.take_record();
        let _ = crate::keep::kept_across_resize(&t, was, None, &mut counts, &rec);
        let mut map = recuts.last().expect("the resize, held").map.to_vec();
        assert_eq!(map.len(), 7, "the control: six lines and the cursor's");
        let following = edit(&mut map);
        let told = crate::paths::Told {
            map: &map,
            following,
        };
        pr.resized(&mut paths, (&rec, Some(told)), &t, was_last, &counts);
        let at = AbsLine::at(prompt);
        paths.written_by_shell(LineSpan::of(at, at));
        (0..12)
            .map(|n| paths.origin(AbsLine::at(Rows::of_count(n))))
            .collect()
    }

    /// **A re-cut that drops lines from the history keeps every line's
    /// writer on its own text** (D3 in the app): ten lines of thirty
    /// columns, each numbered at its start, on a 40x4 window over a history
    /// of six lines, the even ones filed local and the odd remote; the
    /// window narrows to twenty, each line takes two rows, and the ring
    /// drops the oldest. Every row of every line left reads its own line's
    /// writer, found by the number its text starts with. The control: the
    /// re-cut dropped lines -- the oldest left is past the oldest before.
    /// A widen re-cuts each line into fewer rows and drops none. Named
    /// break: each line's new lines one later than the core says
    /// (`LineRecut::new` shifted in the re-cut's hook) -- a row reads the
    /// line before's writer.
    #[test]
    fn a_re_cut_that_drops_lines_keeps_each_lines_writer_on_its_text() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 4,
        };
        let (mut pr, _) = probes(named_groups());
        let mut owned = Owned::with_history(
            &ws,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(6),
        );
        let mut t = Term::new(owned.storage(), &ws);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let mut counts = crate::keep::Counts::new(&t);
        let text: Vec<u8> = (0..10)
            .flat_map(|i| format!("{i}{}\r\n", "z".repeat(29)).into_bytes())
            .collect();
        t.write_at(BytePos::ZERO, &text);
        t.feed(BytePos::ZERO, ReadLen::from_index(text.len()));
        let _ = t.take_record();
        let _ = counts.numbered(&t, None);
        for n in 0..10 {
            let at = AbsLine::at(Rows::of_count(n));
            match n % 2 {
                0 => paths.written_by_shell(LineSpan::of(at, at)),
                _ => paths.written_remote(LineSpan::of(at, at)),
            }
        }
        let first_before = crate::keep::oldest(&t, counts.primary());
        let was = (t.geometry().0, t.screen());
        let was_last = crate::paths::last_line(&t, counts.primary());
        let mut recuts = crate::paths::Recuts::default();
        let _ = recuts.resize(
            &mut t,
            &WinSize {
                ws_col: 20,
                ws_row: 4,
            },
        );
        let rec = t.take_record();
        let s = crate::keep::kept_across_resize(&t, was, None, &mut counts, &rec).1;
        pr.resized(&mut paths, (&rec, recuts.last()), &t, was_last, &counts);
        let first = crate::keep::oldest(&t, s);
        assert!(
            first > first_before,
            "the control: the re-cut dropped lines"
        );
        // Each row with text, by the line its text belongs to.
        let mut writer = None;
        let mut rows = 0;
        let mut line = first;
        while let Some((cells, _)) = baud_render::row_of(&t, s, line) {
            let lead = cells.first().and_then(|c| c.content.as_codepoint());
            match lead.map(|c| c.value()) {
                Some(d) if (u32::from(b'0')..=u32::from(b'9')).contains(&d) => {
                    writer = Some(match (d - u32::from(b'0')) % 2 {
                        0 => Origin::Local,
                        _ => Origin::Remote,
                    });
                }
                Some(_) => {}
                None => writer = None,
            }
            if let Some(w) = writer {
                assert_eq!(paths.origin(line), w, "{line:?}");
                rows += 1;
            }
            line = line.below();
        }
        assert!(rows >= 6, "the control: the rows compared, {rows}");
    }

    /// **A resize that re-cut lines it did not tell is followed by
    /// nothing** (D1): the map the resize told, said not followed, as a
    /// resize through `Term::resize` or a core without the record says --
    /// every line it renumbered is not known, the prompt the shell redraws
    /// on line 3 after it included. The control: followed, every line
    /// keeps its local writer. Named break: `Probes::resized` reading the
    /// map whatever `Following` says -- every line local.
    #[test]
    fn a_re_cut_not_followed_is_followed_by_nothing() {
        let followed = after_recut(|_| baud_core::Following::Followed, Rows::of_count(3));
        assert_eq!(followed, vec![Origin::Local; 12], "the control: followed");
        let not = after_recut(|_| baud_core::Following::NotFollowed, Rows::of_count(3));
        assert_eq!(not, vec![Origin::Unknown; 12], "not followed");
    }

    /// **A line no line told covers is not known, a local fact filed over it
    /// after included** (D2): the map less the line whose text the re-cut
    /// put on lines 4 and 5 -- both are not known, the prompt the shell
    /// redraws on line 4 after it included, and every other line keeps its
    /// local writer. Named break: the lines no line told left with nothing
    /// filed (`Paths::reflowed`'s uncovered lines) -- line 4 lights once
    /// the prompt is filed over it.
    #[test]
    fn a_line_no_line_told_covers_is_not_known() {
        let origins = after_recut(
            |map| {
                map.remove(2);
                baud_core::Following::Followed
            },
            Rows::of_count(4),
        );
        let want: Vec<_> = (0..12)
            .map(|n| match n {
                4 | 5 => Origin::Unknown,
                _ => Origin::Local,
            })
            .collect();
        assert_eq!(origins, want);
    }

    /// **The row written below stays dark in one read or one drain**:
    /// [`below_and_back`]'s bytes all in one read, and its reads taken in
    /// one drain. A paired-layer row, held by the same two layers as
    /// [`a_row_written_below_a_remote_reads_span_stays_dark`]: the remote
    /// part's record of the lowest line it reached, and `logout`'s part
    /// ending on the path's row, which its record vouches nothing clean
    /// on. Named break: both -- no record at the climb (the core's
    /// `record_climb` a no-op) and the end line vouched clean whatever its
    /// row holds -- the one read and the drain light; either alone leaves
    /// them dark.
    #[test]
    fn a_row_written_below_stays_dark_in_one_read_or_one_drain() {
        let [start, remote, logout, prompt] = below_and_back();
        let exit = [logout.as_slice(), &prompt].concat();
        let all = [remote.as_slice(), &exit].concat();
        let one_read = lit_reads(named_groups(), &[read(&start, 1), read(&all, 1)]);
        assert_eq!(one_read, Vec::<usize>::new(), "one read");
        let one_drain = lit_reads(
            named_groups(),
            &[
                read(&start, 1),
                read(&remote, 9),
                Read {
                    drain: Drain::Same,
                    ..read(&exit, 1)
                },
            ],
        );
        assert_eq!(one_drain, Vec::<usize>::new(), "one drain");
    }

    /// **A pending row's verdict holds in each state**: failing as recorded
    /// while its stage is open passes; fixed early, failing another way,
    /// or still pending once its stage has closed is red.
    #[test]
    fn a_pending_rows_verdict_holds_in_each_state() {
        let stage = LinkStage::CoreRecord;
        verdict(&[], stage, 1, 1, 0);
        for (closed, now, why) in [
            (&[][..], 0, "fixed early"),
            (&[][..], 2, "failing another way"),
            (&[LinkStage::CoreRecord][..], 1, "its stage closed"),
        ] {
            let red = std::panic::catch_unwind(|| verdict(closed, stage, now, 1, 0));
            assert!(red.is_err(), "{why} is red");
        }
    }

    /// A screen with room for every prompt [`timed`]'s rows draw: nothing
    /// scrolls, so the lines are numbered as they were written.
    const ROOMY: WinSize = WinSize {
        ws_col: 60,
        ws_row: 50,
    };

    /// One step of [`timed`], at a time in milliseconds from its start.
    enum Step<'a> {
        /// A drain of reads, each with the group read at it (none for a
        /// read inside the pace, which asks for none), fed and filed at
        /// `at`, the drain's own clock.
        Drain(u64, Vec<(&'a [u8], Option<Pid>)>),
        /// A turn every `every` milliseconds from `from` up to `to`.
        Turns { from: u64, to: u64, every: u64 },
        /// The pace's deadline as it stands, noted.
        Look,
    }

    /// What [`timed`] found: the rows that draw a line and the origin of
    /// each row to the cursor's, at the end; the counts; each turn that
    /// sent a batch, with the paths asked so far and why it was due; every path the worker
    /// looked up; and each [`Step::Look`]'s deadline, from the start.
    struct Timed {
        lit: Vec<usize>,
        origins: Vec<Origin>,
        counts: Counts,
        sent: Vec<(u64, u64, By)>,
        walked: Vec<Vec<u8>>,
        looked: Vec<Option<(Duration, By)>>,
    }

    /// **Drains and turns, each at its own clock, as the drivers take
    /// them**: each drain's reads fed through `sampler` and the core
    /// (`reads::paced`) and filed at its time (`Probes::drain_recut`, the
    /// drain's clock its own); each turn at its time a frame's scan, then
    /// the clock's half (`Probes::tick`), with the shell (group 1) in
    /// front, a batch it sends answered, merged and scanned before the next
    /// turn, however long the worker's thread took. So what is due and
    /// what is asked follow the clock passed alone, never the thread's
    /// pace.
    fn timed(f: Fake, mut sampler: Sampler, steps: &[Step<'_>]) -> Timed {
        let log = Arc::clone(&f.log);
        let string_end = f.string_end;
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&ROOMY);
        let mut t = Term::new(owned.storage(), &ROOMY);
        t.set_control_string_end(string_end);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let s = Scrolled::START;
        let mut counts = crate::keep::Counts::new(&t);
        let mut pace = crate::reads::Pace::new();
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let (mut sent, mut looked) = (Vec::new(), Vec::new());
        for step in steps {
            match step {
                Step::Drain(at, reads) => {
                    let began = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
                    let mut tagged = Vec::new();
                    for (k, &(bytes, group)) in reads.iter().enumerate() {
                        let now = ms(*at) + Duration::from_micros(u64::try_from(k).unwrap());
                        let _ = sampler.read(now, bytes);
                        let w = match group {
                            Some(g) => sampler.writers(Some(g), Watched::Nothing),
                            None => sampler.unasked(),
                        };
                        tagged.push((bytes, w));
                    }
                    let mut took = Vec::new();
                    let _ = crate::reads::paced(
                        &mut t,
                        &mut pace,
                        ms(*at),
                        tagged,
                        |_| {},
                        |_, _| {},
                        &mut crate::hostlog::NoLog,
                        &mut took,
                    );
                    let ended = paths.line_on(t.screen(), s.line(Rows::ZERO, t.cursor().0));
                    let rec = took.iter().rev().find_map(|w| w.record);
                    let _ = counts.numbered(&t, rec.as_ref());
                    pr.drain_recut(
                        &mut paths,
                        &t,
                        (began, ended),
                        counts.core_lines_at(s, &t),
                        took.into_iter().map(|w| (w, Linked::No)),
                        &pace.recuts,
                        ms(*at),
                    );
                }
                Step::Turns { from, to, every } => {
                    for at in (*from..*to).step_by(usize::try_from(*every).unwrap()) {
                        let mut links = Links {
                            uris: &mut uris,
                            urls: Turned::On,
                            paths: &mut paths,
                        };
                        let _ = lines.scan(&t, s, &mut links);
                        let before = pr.counts;
                        let seen = Seen {
                            term: &t,
                            scrolled: s,
                            lines: &mut lines,
                            uris: &mut uris,
                            urls: Turned::On,
                            paths: &mut paths,
                        };
                        let _ = pr.tick(ms(at), seen, || Some(1));
                        let c = pr.counts;
                        let by = [
                            (c.by_ended, before.by_ended, By::Ended),
                            (c.by_quiet, before.by_quiet, By::Quiet),
                            (c.by_ceiling, before.by_ceiling, By::Ceiling),
                            (c.by_floor, before.by_floor, By::Floor),
                        ]
                        .into_iter()
                        .find_map(|(now, was, by)| (now != was).then_some(by));
                        if let Some(by) = by {
                            sent.push((at, c.asked, by));
                        }
                        while pr.flight.is_some() {
                            std::thread::yield_now();
                            let mut seen = Seen {
                                term: &t,
                                scrolled: s,
                                lines: &mut lines,
                                uris: &mut uris,
                                urls: Turned::On,
                                paths: &mut paths,
                            };
                            let _ = pr.answers(&mut seen);
                        }
                    }
                }
                Step::Look => looked.push(
                    pr.pace
                        .due()
                        .map(|d| (d.at.saturating_duration_since(t0), d.by)),
                ),
            }
        }
        let _ = lines.scan(
            &t,
            s,
            &mut Links {
                uris: &mut uris,
                urls: Turned::On,
                paths: &mut paths,
            },
        );
        let mut lit: Vec<usize> = lines.lines().iter().map(|l| l.row.index()).collect();
        lit.dedup();
        let origins = (0..=t.cursor().0.index())
            .map(|y| paths.origin(s.line(Rows::ZERO, baud_domain::Row::from_index(y))))
            .collect();
        let walked = log.lock().unwrap().clone();
        Timed {
            lit,
            origins,
            counts: pr.counts,
            sent,
            walked,
            looked,
        }
    }

    /// Turns every millisecond from `from` to `to`.
    /// **P1: a listing is asked about on the shell's `D`, well inside
    /// [`SETTLE`]**: zsh's reads -- the prompt with `ls` run and its `C`
    /// with the token, `ls`'s two rows of paths, then the `D` with the
    /// token and the next prompt -- each a drain a millisecond apart, then
    /// a turn each millisecond to 40 ms. By then two batches went by the
    /// `D` -- the one naming `ls`, and, [`GRANT_GAP`] after, the one asking
    /// about its paths -- both were asked about and both rows are lit. The
    /// control: the same reads in a window baud handed no token, where the
    /// marks are no shell's -- nothing asked and nothing lit by 40 ms, and
    /// both lit by the quiet before a second. Breaks: `Latch::step` mints
    /// no proof; any batch spends the grant (`Pace::sent`); a frame that
    /// reads no line still to be named (`Reads::Pending`); no path owed
    /// once the listing is named (`Awaiting::Unanswered`); a path on a row
    /// still to be named owed only for the rows -- each leaves the paths
    /// unasked at 40 ms.
    #[test]
    fn a_listing_is_asked_on_the_shells_d() {
        let start = ran("ls", ";cmdline=ls");
        let end = prompt_after("");
        let listing: &[u8] = b"/etc/hosts\r\n/etc/passwd\r\n";
        let drains = || {
            vec![
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(listing, Some(2))]),
                Step::Drain(2, vec![(end.as_slice(), Some(1))]),
            ]
        };
        let mut steps = drains();
        steps.push(Step::Turns {
            from: 3,
            to: 40,
            every: 1,
        });
        let got = timed(named_groups(), shells_sampler(), &steps);
        assert_eq!(got.lit, [1, 2], "lit by 40 ms: {:?}", got.sent);
        assert_eq!(got.counts.asked, 2, "{:?}", got.counts);
        assert_eq!(
            (
                got.counts.by_ended,
                got.counts.by_quiet,
                got.counts.by_floor
            ),
            (2, 0, 0),
            "{:?}",
            got.counts
        );
        let mut steps = drains();
        steps.push(Step::Turns {
            from: 3,
            to: 40,
            every: 1,
        });
        let control = timed(named_groups(), Sampler::default(), &steps);
        assert_eq!(control.lit, Vec::<usize>::new(), "the control by 40 ms");
        assert_eq!(control.counts.batches, 0, "{:?}", control.counts);
        let mut steps = drains();
        steps.push(Step::Turns {
            from: 3,
            to: 1000,
            every: 1,
        });
        let quiet = timed(named_groups(), Sampler::default(), &steps);
        assert_eq!(quiet.lit, [1, 2], "the control by the quiet");
        assert_eq!(quiet.counts.by_ended, 0, "{:?}", quiet.counts);
    }

    /// **A hyperlinked listing is asked about on the shell's `D`**: P1's
    /// reads with `ls`'s rows printed as `OSC 8` `file://` links whose text
    /// names no path, as `ls --hyperlink` prints them. By 40 ms the batch
    /// naming `ls` and the one asking about the links' files went by the
    /// `D`, and both files were asked about. Break: a `file://` link on a
    /// row still to be named owing nothing -- only the rows are owed, and
    /// nothing is asked by 40 ms.
    #[test]
    fn a_hyperlinked_listing_is_asked_on_the_shells_d() {
        let start = ran("ls", ";cmdline=ls");
        let end = prompt_after("");
        let listing: &[u8] = b"\x1b]8;;file:///etc/hosts\x1b\\hosts\x1b]8;;\x1b\\\r\n\x1b]8;;file:///etc/passwd\x1b\\passwd\x1b]8;;\x1b\\\r\n";
        let got = timed(
            named_groups(),
            shells_sampler(),
            &[
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(listing, Some(2))]),
                Step::Drain(2, vec![(end.as_slice(), Some(1))]),
                Step::Turns {
                    from: 3,
                    to: 40,
                    every: 1,
                },
            ],
        );
        assert_eq!(got.counts.asked, 2, "{:?} {:?}", got.counts, got.sent);
        assert!(got.sent.iter().all(|s| s.2 == By::Ended), "{:?}", got.sent);
    }

    /// **P2: the grant lowers no row of a one-shot ssh**: the shell's `C`
    /// naming `ssh h`; then one read, the shell in front at its sample:
    /// ssh's `x` and `/etc/hosts`, a move back up two rows, which refuses
    /// the cut at the `D`, and the shell's `D` and a prompt of two lines,
    /// whose line feeds pass over the `/etc/hosts` row. By 40 ms a batch
    /// went by the `D`, and the row stays dark, `/etc/hosts` never looked
    /// up. The control: `cat` in place of `ssh`, lit. A provenance guard,
    /// green with the grant and without it. Break: the grant files the
    /// drain that minted it as the shell's (`drain_recut` raising every part
    /// to the shell's where its `D` granted) -- the row lights and is looked
    /// up. Raising the `D`'s own part alone lights nothing: a row filed
    /// remote is never lowered by a later fact (`Paths::file`).
    #[test]
    fn a_grant_lowers_no_row_of_a_one_shot_ssh() {
        let run = |line: &str| {
            let start = ran(line, &format!(";cmdline={}", line.replace(' ', "\\ ")));
            let out = [
                b"x\r\n/etc/hosts\r\n\x1b[2A".as_slice(),
                &prompt_after(""),
                b"\r\n\r\n> ",
            ]
            .concat();
            timed(
                named_groups(),
                shells_sampler(),
                &[
                    Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                    Step::Drain(1, vec![(out.as_slice(), Some(1))]),
                    Step::Turns {
                        from: 2,
                        to: 40,
                        every: 1,
                    },
                ],
            )
        };
        let ssh = run("ssh h");
        assert!(!ssh.lit.contains(&2), "ssh's row dark: {:?}", ssh.lit);
        assert!(
            !ssh.walked.iter().any(|p| p == b"/etc/hosts"),
            "{:?}",
            ssh.walked
        );
        assert!(
            ssh.counts.by_ended >= 1,
            "the grant fired: {:?}",
            ssh.counts
        );
        let cat = run("cat x");
        assert!(
            cat.lit.contains(&2),
            "the control: cat's row lit: {:?}",
            cat.lit
        );
    }

    /// **P3: rows a remote program wrote stay dark when the shell's `D`
    /// sends the batch at once**: the remote shapes of
    /// `a_remote_link_below_the_shells_d_stays_dark` -- a link a row below
    /// the cut, in a read of its own and in the `D`'s -- and of
    /// `a_remote_programs_own_d_ends_nothing` with the shell's own `D` after
    /// it, each with turns a millisecond apart to 40 ms after the last
    /// drain, not a second: a batch went by the `D` in each, and no remote
    /// row goes local. Break: P2's, which lights the link where its read
    /// and the `D` share the drain.
    #[test]
    fn remote_rows_stay_dark_when_the_shells_d_sends_the_batch() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let remote: &[u8] = b"x\r\n/etc/passwd\r\n\x1b[2A";
        let prompt = [prompt_after("").as_slice(), b"\r\n\r\n> "].concat();
        let one = [remote, prompt.as_slice()].concat();
        let run = |drains: Vec<Vec<(&[u8], Option<Pid>)>>| {
            let n = u64::try_from(drains.len()).unwrap();
            let mut steps: Vec<Step<'_>> = drains
                .into_iter()
                .zip(0..)
                .map(|(d, at)| Step::Drain(at, d))
                .collect();
            steps.push(Step::Turns {
                from: n,
                to: n + 40,
                every: 1,
            });
            timed(named_groups(), shells_sampler(), &steps)
        };
        for got in [
            run(vec![
                vec![(start.as_slice(), Some(1))],
                vec![(remote, Some(9))],
                vec![(prompt.as_slice(), Some(1))],
            ]),
            run(vec![
                vec![(start.as_slice(), Some(1))],
                vec![(one.as_slice(), Some(9))],
            ]),
        ] {
            assert!(!got.lit.contains(&2), "the link's row: {:?}", got.lit);
            assert_ne!(got.origins[2], Origin::Local, "{:?}", got.origins);
            assert!(got.counts.by_ended >= 1, "{:?}", got.counts);
        }
        let end = prompt_after("");
        let got = run(vec![
            vec![(start.as_slice(), Some(1))],
            vec![(b"x\x1b]133;D;0\x07/etc/passwd\r\n".as_slice(), None)],
            vec![(b"/etc/hosts\r\n".as_slice(), None)],
            vec![(end.as_slice(), Some(1))],
        ]);
        assert_eq!(got.lit, Vec::<usize>::new(), "no remote row lit");
        assert_eq!(got.origins[1..3], [Origin::Remote; 2], "{:?}", got.origins);
        assert!(got.counts.by_ended >= 1, "{:?}", got.counts);
    }

    /// **P4: a remote shell's own marks grant nothing**: inside `ssh h`'s
    /// command, the remote prints a `D` with no token, one with another
    /// token, an `A` and its prompt, then `/etc/hosts` on the next row in a
    /// read of its own. By 40 ms no batch went, and the deadline is the
    /// quiet's, [`SETTLE`] after the read; at a second the row is still
    /// dark. Breaks: a grant on any `D` the scan read, the latch's verdict
    /// aside -- a batch by the `D` at once; a `D` with no token ending the
    /// command (`Commands::ends`) -- the row lights.
    #[test]
    fn a_remote_shells_own_marks_grant_nothing() {
        let start = ran("ssh h", ";cmdline=ssh\\ h");
        let remote: &[u8] =
            b"\x1b]133;D;0;aid=7\x07\x1b]133;D;0;baud_token=fedcba9876543210\x07\x1b]133;A;aid=7\x07$ \r\n";
        let got = timed(
            named_groups(),
            shells_sampler(),
            &[
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(remote, None)]),
                Step::Drain(2, vec![(b"/etc/hosts\r\n".as_slice(), Some(1))]),
                Step::Turns {
                    from: 3,
                    to: 40,
                    every: 1,
                },
                Step::Look,
                Step::Turns {
                    from: 40,
                    to: 1000,
                    every: 10,
                },
            ],
        );
        let at_40 = got.sent.iter().filter(|s| s.0 < 40).count();
        assert_eq!(at_40, 0, "no batch by 40 ms: {:?}", got.sent);
        let Some(Some((at, by))) = got.looked.first().copied() else {
            panic!("nothing owed: {:?}", got.looked);
        };
        assert_eq!(by, By::Quiet, "{:?}", got.looked);
        assert!(at >= SETTLE, "{at:?}");
        assert_eq!(got.lit, Vec::<usize>::new(), "dark at a second");
        assert_eq!(got.counts.by_ended, 0, "{:?}", got.counts);
    }

    /// **P5: the shell's `D` inside a picture's body grants nothing**, on a
    /// terminal set to end an `APC G` only at `BEL` or `ESC \\`
    /// ([`Fake::kitty_ending`]): after a drift (a picture's body left open
    /// over the shell's `C`),
    /// the `D` with its token and the prompt inside another picture's body,
    /// where the core reads no mark and the scan reads the `D`, then
    /// `/etc/hosts` typed after the body, a path the prompt's naming makes
    /// readable. By 40 ms no batch went by the `D`. The control: the same
    /// `D`, prompt and path outside it, a part the latch trusts -- a batch
    /// by the `D`. Break: a grant on the `D` the scan read, the core's marks
    /// aside (`latched` minting where the part carries the shell's `D`) --
    /// a batch at once. A drifted part that re-syncs later than its first
    /// mark is refused by the latch (`vouch`'s
    /// `a_part_that_re_syncs_after_its_first_mark_ends_nothing`), a shape
    /// no drain here reaches: a read is cut into parts at the shell's
    /// first `D` (`reads::paced`), so a part with the `D` begins at it.
    #[test]
    fn a_d_in_a_pictures_body_grants_nothing() {
        let drift = [b"\x1b_Ga=d;".as_slice(), &cat_here(b"\x07"), b"\x1b\\\r\n"].concat();
        let d = prompt_after("");
        let inside = [b"\x1b_Ga=d;".as_slice(), &d, b"\x1b\\/etc/hosts"].concat();
        let outside = [d.as_slice(), b"/etc/hosts"].concat();
        let run = |part: &[u8]| {
            timed(
                named_groups().kitty_ending(),
                shells_sampler(),
                &[
                    Step::Drain(0, vec![(drift.as_slice(), Some(6))]),
                    Step::Drain(1, vec![(part, Some(1))]),
                    Step::Turns {
                        from: 2,
                        to: 40,
                        every: 1,
                    },
                ],
            )
            .counts
        };
        let control = run(&outside);
        assert!(control.by_ended >= 1, "the control: {control:?}");
        let inside = run(&inside);
        assert_eq!(inside.by_ended, 0, "inside a picture's body: {inside:?}");
    }

    /// **P6: a command after the `D` ends the grant**: `ls` run and its
    /// rows; then in one drain the shell's `D` and prompt, `cat x` typed
    /// and its `C` with the token, and the start of `cat`'s output. No
    /// batch went by the `D` by 40 ms: the command is still running.
    /// Break: no lapse on a `C` -- a batch by the `D` at once.
    #[test]
    fn a_command_after_the_d_ends_the_grant() {
        let start = ran("ls", ";cmdline=ls");
        let end = prompt_after("cat x\r\n");
        let c = format!("\x1b]133;C;baud_token={TOKEN};cmdline=cat\\ x\x07").into_bytes();
        let got = timed(
            named_groups(),
            shells_sampler(),
            &[
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(b"/etc/hosts\r\n".as_slice(), Some(2))]),
                Step::Drain(
                    2,
                    vec![
                        (end.as_slice(), Some(1)),
                        (c.as_slice(), Some(1)),
                        (b"/etc/passwd".as_slice(), Some(2)),
                    ],
                ),
                Step::Turns {
                    from: 3,
                    to: 40,
                    every: 1,
                },
            ],
        );
        assert_eq!(got.counts.batches, 0, "{:?} {:?}", got.counts, got.sent);
    }

    /// **P7: Enter held at a prompt holding a path asks at most once a
    /// `D`**: forty of the shell's `D`s, each a cancelled line and a new
    /// prompt row holding a path of its own, 33 ms apart, as a key repeats,
    /// and turns every 4 ms through them and two seconds after. Every `D`
    /// is trusted and grants, and its chain passes the floor: between one
    /// `D` and the next, at most two batches by the `D` and at most one
    /// batch asking about a path; none the `D`s sent closer than
    /// [`GRANT_GAP`] to the batch before it; and no batch is held under a
    /// grant (`Counts::held_granted`). Break: no lift -- the floor holds
    /// batches the `D`s made due, counted held under them.
    #[test]
    fn held_enter_asks_at_most_once_a_d() {
        let enters: Vec<Vec<u8>> = (0..40)
            .map(|k| {
                format!(
                    "\r\n\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07/etc/p{k} % "
                )
                .into_bytes()
            })
            .collect();
        let mut steps = Vec::new();
        for (k, enter) in (0..40_u64).zip(&enters) {
            steps.push(Step::Drain(33 * k, vec![(enter.as_slice(), Some(1))]));
            steps.push(Step::Turns {
                from: 33 * k + 1,
                to: 33 * (k + 1),
                every: 4,
            });
        }
        steps.push(Step::Turns {
            from: 33 * 40,
            to: 33 * 40 + 2000,
            every: 4,
        });
        let got = timed(named_groups(), shells_sampler(), &steps);
        let gap = u64::try_from(GRANT_GAP.as_millis()).unwrap();
        let mut asked_at = Vec::new();
        let mut before = 0;
        for &(at, asked, _) in &got.sent {
            if asked != before {
                asked_at.push(at);
                before = asked;
            }
        }
        assert!(asked_at.len() >= 2, "{:?}", got.sent);
        for k in 0..40_u64 {
            let within = |at: u64| 33 * k <= at && at < 33 * (k + 1);
            let asking = asked_at.iter().filter(|&&at| within(at)).count();
            assert!(asking <= 1, "{asking} asking after D {k}: {asked_at:?}");
            let by_d = got
                .sent
                .iter()
                .filter(|s| within(s.0) && s.2 == By::Ended)
                .count();
            assert!(by_d <= 2, "{by_d} by D {k}: {:?}", got.sent);
        }
        for w in got.sent.windows(2) {
            if w[1].2 == By::Ended {
                assert!(
                    w[1].0 - w[0].0 >= gap,
                    "closer than GRANT_GAP: {:?}",
                    got.sent
                );
            }
        }
        assert!(got.counts.by_ended >= 1, "{:?}", got.counts);
        assert_eq!(got.counts.held_granted, 0, "{:?}", got.counts);
        println!(
            "held Enter: {} batches in {} ms, {} asking at {asked_at:?}, {:?}",
            got.sent.len(),
            33 * 40 + 2000,
            asked_at.len(),
            got.counts
        );
    }

    /// **Enter held at a prompt with no path is named by the ceiling, not
    /// at each `D`**: P7's shape for five seconds, 150 of the shell's `D`s
    /// 33 ms apart, each a cancelled line and a new prompt row `% `, which
    /// holds no path, and turns every 4 ms through them and a second after.
    /// Every `D` is trusted and grants, and only rows are owed: the quiet
    /// never comes, and the ceiling sends a batch a second -- at most 6 in
    /// the six seconds, none by the `D`. Break: the grant hastening rows
    /// that changed alone -- a batch by the `D` at each.
    #[test]
    fn held_enter_at_a_prompt_with_no_path_is_named_by_the_ceiling() {
        let enter = format!("\r\n\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07% ")
            .into_bytes();
        let mut steps = Vec::new();
        for k in 0..150_u64 {
            steps.push(Step::Drain(33 * k, vec![(enter.as_slice(), Some(1))]));
            steps.push(Step::Turns {
                from: 33 * k + 1,
                to: 33 * (k + 1),
                every: 4,
            });
        }
        steps.push(Step::Turns {
            from: 33 * 150,
            to: 33 * 150 + 1000,
            every: 4,
        });
        let got = timed(named_groups(), shells_sampler(), &steps);
        assert!(got.sent.len() <= 6, "{:?}", got.sent);
        assert!(!got.sent.is_empty(), "named at all: {:?}", got.counts);
        assert_eq!(got.counts.by_ended, 0, "{:?}", got.counts);
        assert_eq!(got.counts.asked, 0, "{:?}", got.counts);
    }

    /// **Enter held at a prompt that shows a path names each new row and
    /// asks once**: P7's shape with the same path on every prompt row, as a
    /// prompt that shows its folder: forty of the shell's `D`s 33 ms apart,
    /// each a cancelled line and a new prompt row `/etc/hosts % `, and
    /// turns every 4 ms through them and two seconds after. The first `D`'s
    /// chain names its row and asks about the path, laying the floor; each
    /// later `D`'s row has its writer still to be named, so a batch names
    /// it, and the answer kept since draws its mark. In all, at most one
    /// batch naming a `D` and one batch asking; between one `D` and the
    /// next, at most two batches passing the floor the asking one laid; and
    /// no batch held under a grant (`Counts::held_granted`). The control
    /// that the floor bound is live: `D`s inside the floor pass it. Breaks:
    /// a kept answer asked again by each collection -- a batch asking at
    /// every `D`; no lift -- none passing the floor. The cap itself, which
    /// one naming batch a `D` never reaches here, is
    /// [`each_d_of_a_held_key_passes_the_floor_with_two_scans`]'s.
    #[test]
    fn held_enter_at_a_path_prompt_names_each_row_and_asks_once() {
        let enter = format!(
            "\r\n\x1b]133;D;0;aid=1;baud_token={TOKEN}\x07\x1b]133;A;aid=1\x07/etc/hosts % "
        )
        .into_bytes();
        let mut steps = Vec::new();
        for k in 0..40_u64 {
            steps.push(Step::Drain(33 * k, vec![(enter.as_slice(), Some(1))]));
            steps.push(Step::Turns {
                from: 33 * k + 1,
                to: 33 * (k + 1),
                every: 4,
            });
        }
        steps.push(Step::Turns {
            from: 33 * 40,
            to: 33 * 40 + 2000,
            every: 4,
        });
        let ds: Vec<u64> = steps
            .iter()
            .filter_map(|s| match s {
                Step::Drain(at, _) => Some(*at),
                _ => None,
            })
            .collect();
        let got = timed(named_groups(), shells_sampler(), &steps);
        let (mut asking, mut naming) = (Vec::new(), Vec::new());
        let mut before = 0;
        for &(at, asked, _) in &got.sent {
            match asked == before {
                true => naming.push(at),
                false => asking.push(at),
            }
            before = asked;
        }
        assert_eq!(asking.len(), 1, "asking at {asking:?}: {:?}", got.sent);
        assert!(
            naming.len() <= ds.len(),
            "{} naming for {} Ds: {:?}",
            naming.len(),
            ds.len(),
            got.sent
        );
        let ceiling = u64::try_from(CEILING.as_millis()).unwrap();
        let floor = |at: u64| asking[0] < at && at < asking[0] + ceiling;
        let mut passed = 0;
        for (k, &d) in ds.iter().enumerate() {
            let next = ds.get(k + 1).copied().unwrap_or(u64::MAX);
            let lifted = got
                .sent
                .iter()
                .filter(|s| d <= s.0 && s.0 < next && s.2 == By::Ended && floor(s.0))
                .count();
            assert!(
                lifted <= 2,
                "{lifted} past the floor after D {k}: {:?}",
                got.sent
            );
            passed += lifted;
        }
        assert!(passed >= 20, "the control: {passed} passed the floor");
        assert_eq!(got.counts.held_granted, 0, "{:?}", got.counts);
        println!(
            "held Enter at a path prompt: {} batches, {} naming, asking at {asking:?}, {passed} past the floor, {:?}",
            got.sent.len(),
            naming.len(),
            got.counts
        );
    }

    /// `ls` listing `/etc/hosts` on its `D`, its batches answered by
    /// 100 ms, then `ls` again at 100 ms listing `second`, and turns a
    /// millisecond apart to 150 ms: within a second of the first listing's
    /// asking batch, under the floor it laid.
    fn a_listing_again(second: &[u8]) -> Timed {
        let start = ran("ls", ";cmdline=ls");
        let end = prompt_after("");
        timed(
            named_groups(),
            shells_sampler(),
            &[
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(b"/etc/hosts\r\n".as_slice(), Some(2))]),
                Step::Drain(2, vec![(end.as_slice(), Some(1))]),
                Step::Turns {
                    from: 3,
                    to: 100,
                    every: 1,
                },
                Step::Drain(100, vec![(start.as_slice(), Some(1))]),
                Step::Drain(101, vec![(second, Some(2))]),
                Step::Drain(102, vec![(end.as_slice(), Some(1))]),
                Step::Turns {
                    from: 103,
                    to: 150,
                    every: 1,
                },
            ],
        )
    }

    /// **A listing within a second of a batch that asked goes on its
    /// `D`**: `ls` lists `/etc/hosts`, asked about on its `D`; a tenth of a
    /// second later `ls` lists `/etc/passwd`. Its `D`'s grant passes the
    /// floor the first listing's asking batch laid: the batch naming the
    /// second `ls` and the one asking about its path both go by the `D`,
    /// [`GRANT_GAP`] apart, and both listings are lit by 150 ms. None held:
    /// `held` and `held_granted` stay 0. Break: no lift -- the second
    /// listing's batch goes by the floor, a second after the first asking,
    /// and its row is dark at 150 ms; the floor passed only by a batch that
    /// asks -- the naming batch is never sent, and the same.
    #[test]
    fn a_listing_within_a_second_of_an_asking_batch_goes_on_its_d() {
        let got = a_listing_again(b"/etc/passwd\r\n");
        let second: Vec<(u64, u64, By)> = got.sent.iter().copied().filter(|s| s.0 >= 100).collect();
        assert_eq!(
            second.iter().map(|s| s.2).collect::<Vec<_>>(),
            [By::Ended, By::Ended],
            "{:?}",
            got.sent
        );
        assert_eq!(got.counts.asked, 2, "{:?}", got.counts);
        assert_eq!(
            (
                got.counts.held,
                got.counts.held_granted,
                got.counts.by_floor
            ),
            (0, 0, 0),
            "{:?}",
            got.counts
        );
        assert_eq!(got.lit, [1, 3], "both listings lit by 150 ms");
    }

    /// **The same listing again, on the first one's marks, goes on its
    /// `D`**: `ls` lists `/etc/hosts`, asked about on its `D`; a tenth of a
    /// second later the same `ls` again. Every path it prints has its
    /// answer kept, so its one batch only names its writer, which makes
    /// the kept answer drawable: that batch goes by the `D`, past the floor
    /// the first listing's asking batch laid, nothing is asked again, and
    /// both rows are lit by 150 ms. `held_granted` stays 0. Breaks: no lift
    /// -- the naming batch goes by the floor and the row is dark at 150 ms;
    /// the floor passed only by a batch that asks -- the same.
    #[test]
    fn the_same_listing_again_goes_on_its_d() {
        let got = a_listing_again(b"/etc/hosts\r\n");
        let second: Vec<(u64, u64, By)> = got.sent.iter().copied().filter(|s| s.0 >= 100).collect();
        assert_eq!(
            second.first().map(|s| s.2),
            Some(By::Ended),
            "{:?}",
            got.sent
        );
        assert_eq!(got.counts.asked, 1, "nothing asked again: {:?}", got.counts);
        assert_eq!(
            (
                got.counts.held,
                got.counts.held_granted,
                got.counts.by_floor
            ),
            (0, 0, 0),
            "{:?}",
            got.counts
        );
        assert_eq!(got.lit, [1, 3], "both listings lit by 150 ms");
    }

    /// **A `D` that is not the window's shell's passes no floor**: `ls`
    /// lists `/etc/hosts`, asked about on its `D`, which lays the floor;
    /// a tenth of a second later, inside `ssh h`'s command, the remote
    /// prints a `D` with no token, one with another token, an `A` and its
    /// prompt, then `/etc/hosts` again on a row of its own. No batch goes
    /// by a `D`, and the deadline is the floor's, a second after the
    /// asking. Break: the proof minted from any `D` the core read
    /// (`vouch::Latch::step`) -- a batch by the `D`, past the floor.
    #[test]
    fn a_foreign_d_lifts_nothing() {
        let start = ran("ls", ";cmdline=ls");
        let end = prompt_after("");
        let ssh = ran("ssh h", ";cmdline=ssh\\ h");
        let remote: &[u8] =
            b"\x1b]133;D;0;aid=7\x07\x1b]133;D;0;baud_token=fedcba9876543210\x07\x1b]133;A;aid=7\x07$ \r\n";
        let got = timed(
            named_groups(),
            shells_sampler(),
            &[
                Step::Drain(0, vec![(start.as_slice(), Some(1))]),
                Step::Drain(1, vec![(b"/etc/hosts\r\n".as_slice(), Some(2))]),
                Step::Drain(2, vec![(end.as_slice(), Some(1))]),
                Step::Turns {
                    from: 3,
                    to: 100,
                    every: 1,
                },
                Step::Drain(100, vec![(ssh.as_slice(), Some(1))]),
                Step::Drain(101, vec![(remote, None)]),
                Step::Drain(102, vec![(b"/etc/passwd\r\n".as_slice(), Some(1))]),
                Step::Turns {
                    from: 103,
                    to: 300,
                    every: 1,
                },
                Step::Look,
            ],
        );
        let first_ask = got
            .sent
            .iter()
            .find(|s| s.1 > 0)
            .expect("an asking batch")
            .0;
        assert!(
            got.sent.iter().all(|s| s.0 < 100),
            "no batch after the foreign D by 300 ms: {:?}",
            got.sent
        );
        let Some(Some((at, by))) = got.looked.first().copied() else {
            panic!("nothing owed: {:?}", got.looked);
        };
        assert_eq!(by, By::Floor, "{:?}", got.looked);
        assert_eq!(
            at,
            Duration::from_millis(first_ask + 1000),
            "{:?}",
            got.sent
        );
    }

    /// The source of `probe.rs` before its tests, its comment lines blank.
    fn code_of(src: &str) -> String {
        let code = &src[..src.find("#[cfg(test)]\nmod tests").expect("the tests")];
        code.lines()
            .map(|l| match l.trim_start().starts_with("//") {
                true => "",
                false => l,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The body of the item whose first line is `head`, to its closing
    /// brace at `indent`.
    fn body_of<'a>(code: &'a str, head: &str, indent: &str) -> &'a str {
        let from = code.find(head).unwrap_or_else(|| panic!("{head}"));
        let end = format!("\n{indent}}}\n");
        let to = from
            + code[from..]
                .find(&end)
                .unwrap_or_else(|| panic!("{head}'s end"));
        &code[from..to]
    }

    /// **S1: one door to a screen batch, through the pace**: [`SETTLE`],
    /// [`CEILING`] and [`GRANT_GAP`] are used only inside `impl Pace`, past
    /// their own definitions; a `Pace` is built only in its `default`; and
    /// `tick` sends a screen batch only after the line that binds a
    /// deadline and its reason from `Pace::due`, counting that reason --
    /// the two `kind: Kind::Screen` asks in the code are `tick`'s, and the
    /// first is the paths-off branch, exempt by name: it only names the
    /// groups a link's read saw, needs no path, looks nothing up and is
    /// sent at the turn the read owes. Named break: a `pace_bypass` that
    /// sends a screen batch from `answers` on a naming answer -- a third
    /// screen ask, outside `tick`.
    #[test]
    fn a_screen_batch_goes_through_the_pace_alone() {
        let code = code_of(include_str!("probe.rs"));
        let pace = body_of(&code, "impl<T: Moment> Pace<T> {", "");
        for name in ["SETTLE", "CEILING", "GRANT_GAP"] {
            let def = format!("pub const {name}: Duration");
            for (at, _) in code.match_indices(name) {
                let line = code[..at].rfind('\n').map_or(0, |n| n + 1);
                let line = &code[line..at + code[at..].find('\n').unwrap_or(0)];
                let in_pace = code
                    .find(pace)
                    .is_some_and(|p| p <= at && at < p + pace.len());
                assert!(
                    line.contains(&def) || in_pace,
                    "{name} used outside impl Pace: {line}"
                );
            }
        }
        let built: Vec<usize> = code.match_indices("Pace {").map(|(i, _)| i).collect();
        let default = body_of(&code, "impl<T: Moment> Default for Pace<T> {", "");
        let at = code.find(default).expect("default");
        assert_eq!(built.len(), 1, "a Pace built once");
        assert!(
            built.iter().all(|&b| at <= b && b < at + default.len()),
            "in default"
        );
        let asks: Vec<usize> = code
            .match_indices("kind: Kind::Screen,")
            .map(|(i, _)| i)
            .collect();
        let tick = body_of(&code, "    pub fn tick(", "    ");
        let t = code.find(tick).expect("tick");
        assert_eq!(asks.len(), 2, "two screen asks in the code");
        assert!(
            asks.iter().all(|&a| t <= a && a < t + tick.len()),
            "both in tick"
        );
        let off = t + tick.find("Marks::Off => {").expect("the paths-off branch");
        let off_end = off + code[off..].find("\n            }\n").expect("its end");
        assert!(off < asks[0] && asks[0] < off_end, "the first, paths off");
        let door = t + tick.find("self.pace.due()").expect("the door");
        let counted = t + tick
            .find("self.counts.sent_by(due.by)")
            .expect("its reason");
        assert!(
            door < counted && counted < asks[1],
            "the second, through the door"
        );
    }

    /// **S2: the proof stays out of filing**: `vouch::Ended` is named, in
    /// the code, in `vouch.rs` and in `probe.rs` only inside `impl Pace`
    /// and `latched`, and in none of `paths.rs`, `ran.rs`, `cut.rs`,
    /// `reads.rs` or `link_layer.rs`; no `written_` method of `Paths` takes
    /// anything of `vouch`'s; and `latched` raises no part: the only
    /// verdict it names is `Ran::Command`, the one it lowers an untrusted
    /// part to, and it files nothing. Named break: `latched` filing the
    /// part where the shell ended its command as the shell's -- a
    /// `Ran::Shell` in `latched`.
    #[test]
    fn the_shells_d_files_no_row() {
        let code = code_of(include_str!("probe.rs"));
        let pace = body_of(&code, "impl<T: Moment> Pace<T> {", "");
        let latched = body_of(&code, "    fn latched(", "    ");
        let (p, l) = (
            code.find(pace).expect("Pace"),
            code.find(latched).expect("latched"),
        );
        for (at, _) in code.match_indices("vouch::Ended") {
            assert!(
                (p <= at && at < p + pace.len()) || (l <= at && at < l + latched.len()),
                "vouch::Ended outside Pace and latched"
            );
        }
        assert!(
            latched.contains("self.pace.ended("),
            "the control: minted there"
        );
        for (name, src) in [
            ("paths.rs", include_str!("paths.rs")),
            ("ran.rs", include_str!("ran.rs")),
            ("cut.rs", include_str!("cut.rs")),
            ("reads.rs", include_str!("reads.rs")),
            ("link_layer.rs", include_str!("link_layer.rs")),
        ] {
            assert!(!src.contains("vouch::Ended"), "{name} names the proof");
        }
        let paths = include_str!("paths.rs");
        for (at, _) in paths.match_indices("pub fn written_") {
            let sig = &paths[at..at + paths[at..].find('{').expect("a body")];
            assert!(!sig.contains("vouch"), "{sig}");
        }
        assert!(paths.contains("pub fn written_by_shell("), "the control");
        let named: Vec<&str> = latched
            .match_indices("Ran::")
            .map(|(at, _)| {
                let rest = &latched[at + 5..];
                &rest[..rest
                    .find(|c: char| !c.is_alphanumeric())
                    .unwrap_or(rest.len())]
            })
            .collect();
        assert_eq!(named, ["Command"], "latched raises no part");
        assert!(!latched.contains("written_"), "latched files nothing");
    }
}
