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
//! would never be asked about. Each batch is the window's screen as it is
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
use baud_domain::Rows;
use baud_render::{AbsLine, Scrolled};

use crate::bell::Moment;
use crate::fg::StartTime;
use crate::lined::{Lines, Uris};
use crate::paths::{
    Candidate, Front, Handler, LineSpan, Links, Marks, Moved, Owed, Paths, PathsAt, Probe, Program,
    ProgramDir, Turned,
};
use crate::sys::Pid;

/// How long after the program's last output a batch is sent.
pub const SETTLE: Duration = Duration::from_millis(150);

/// The fewest seconds between two batches, while output goes on or not.
pub const CEILING: Duration = Duration::from_secs(1);

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
/// started, and its names.
type Named = std::collections::HashMap<Pid, (Option<StartTime>, Vec<String>)>;

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
    /// The scan for what each read did to its cursor's line
    /// ([`crate::reach::Reads`]), and the last read's, with whether it
    /// carried a folder report.
    reads: crate::reach::Reads,
    reach: crate::reach::Reach,
    folder: crate::ran::Folder,
    reports: crate::ran::Reports,
    /// Whether the stream stood at ground as each read began
    /// ([`crate::cut::Stream`]), and where the last read is cut.
    stream: crate::cut::Stream,
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
    /// What the read did to the line its cursor ended on
    /// ([`crate::reach::Reach`]): whether that line is its.
    pub reach: crate::reach::Reach,
    /// Whether the read carried a folder report ([`crate::ran::Folder`]):
    /// the last one's verdict, or, once the read is cut to the part of it
    /// fed (`reads::Part`), the last that part fed.
    pub folder: crate::ran::Folder,
    /// Each folder report the read ended, with where
    /// ([`crate::ran::Reports`]).
    pub reports: crate::ran::Reports,
    /// The links the pen held as the part of it was fed
    /// ([`crate::paths::Pens`]).
    pub pens: crate::paths::Pens,
    /// Where the read is cut at the shell's `D`, or, for a part after it,
    /// where it was ([`crate::cut::Cut`]).
    pub cut: crate::cut::Cut,
}

/// **A read's writers, cut to a part of its bytes**: the folder verdict the
/// part fed, and the reports of a part held.
impl crate::reads::Part for Writers {
    fn part(self, from: baud_core::BytePos, to: baud_core::BytePos) -> Writers {
        Writers {
            folder: self.reports.part(from, to),
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
            reports: self.reports.rest(from),
            cut,
            ..self
        }
    }
    fn under(self, pens: crate::paths::Pens) -> Writers {
        Writers { pens, ..self }
    }
    fn plan(&self) -> Option<crate::cut::Plan> {
        match self.cut {
            crate::cut::Cut::Plan(p) => Some(p),
            crate::cut::Cut::None | crate::cut::Cut::After(_) => None,
        }
    }
    fn before_cut(self, reach: crate::reach::Reach) -> Writers {
        Writers {
            reach,
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
            reach: crate::reach::Reach::Anywhere,
            folder: crate::ran::Folder::Unreported,
            reports: crate::ran::Reports::default(),
            pens: crate::paths::Pens::default(),
            cut: crate::cut::Cut::None,
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
            reach: self.reach,
            folder: self.folder,
            reports: self.reports,
            pens: crate::paths::Pens::default(),
            cut: self.cut,
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
            reach: self.reach,
            folder: self.folder,
            reports: self.reports,
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
        let was = self.reads.encoding();
        self.reach = self.reads.read(bytes);
        // Under `ESC % @` an `OSC` can begin with 0x9D, which the marks
        // scan does not read: a read any byte of which was scanned so and
        // which holds one may report where no scan placed it, so every part
        // of it that feeds a byte is refused.
        let latin = self.reads.latin1_seen() == crate::reach::Latin1Seen::Yes;
        self.reports = match latin && bytes.contains(&0x9d) {
            true => crate::ran::Reports::unvouched(),
            false => self.commands.reports(),
        };
        self.folder = self.reports.last();
        // Where the read is cut at the shell's `D`: only from ground, and
        // under UTF-8 as the read began.
        let grounded = match was {
            crate::reach::Encoding::Utf8 => self.stream.grounded(),
            crate::reach::Encoding::Latin1 => crate::cut::Grounded::No,
        };
        self.stream.read(bytes);
        self.cut = self
            .commands
            .cut()
            .and_then(|(d, after)| crate::cut::Plan::of(bytes, d, after, grounded))
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
        match (self.due(now), linked) {
            (true, l) => Asks::Yes(l),
            (false, Linked::Yes) => Asks::Yes(Linked::Yes),
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
fn holds_link(b: &[u8]) -> bool {
    let mut from = 0;
    while let Some(p) = b[from..]
        .iter()
        .position(|&c| c == 0x1b || c == 0x9d || c == b':')
    {
        let at = from + p;
        let rest = &b[at + 1..];
        let hit = match b[at] {
            0x1b => rest.starts_with(b"]8;"),
            0x9d => rest.starts_with(b"8;"),
            _ => {
                let name = at
                    .checked_sub(4)
                    .map(|s| (&b[s..at], s.checked_sub(1).map(|q| b[q])));
                name.is_some_and(|(name, before)| {
                    name.eq_ignore_ascii_case(b"file") && before != Some(b';')
                }) && rest.starts_with(b"//")
            }
        };
        if hit {
            return true;
        }
        from = at + 1;
    }
    false
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
    pub cursor: AbsLine,
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
    pub cursor: AbsLine,
    /// What is in front now; none for a batch left for a newer one before
    /// it was taken, which names nothing.
    pub front: Option<Front>,
    /// What each group seen since the last batch was, with the lines it
    /// wrote: named, or gone before it could be named.
    pub seen: Vec<(crate::paths::Seen, LineSpan)>,
    pub probes: Vec<(Vec<u8>, Probe)>,
    /// For [`Kind::Programs`]: what each was found to be; none for a batch
    /// left for a newer one.
    pub programs: Option<Vec<(Program, Handler)>>,
    /// The folder of the group asked about, when it was named local and
    /// the folder could be read, and when its leader started, read with it:
    /// the key main compares, with no call of its own.
    pub folder: Option<ProgramDir>,
    pub started: Option<StartTime>,
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
            if named.len() >= NAMED {
                named.clear();
            }
            let t = fs.started(g);
            let n = fs.program(g);
            naming += 2;
            beat();
            named.insert(g, (t, n));
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
        let mut got = answer(&mut *fs, ask, &mut named, &beat, &mut stop);
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
    // none for one gone by then. A name kept for a leader that started at
    // another time than the group's leader now is another process's, the
    // group number reused, and the group is named again; a leader gone,
    // whose start no longer reads, keeps the name it was seen with.
    let mut names_of_group = |g: Pid, calls: &mut u64| -> Vec<String> {
        let now = fs.started(g);
        *calls += 1;
        if let Some((then, n)) = named.get(&g) {
            let reused = matches!((then, now), (Some(a), Some(b)) if *a != b);
            if !reused {
                return n.clone();
            }
        }
        let n = fs.program(g);
        *calls += 1;
        beat();
        n
    };
    // A group seen at a read and gone before it was named is not known,
    // and so are the lines written past the groups kept: their rows fail
    // closed and touch no other row (`Paths::written_under`). The group in
    // front with no name is not known to be local, as `front_of` says.
    let mut seen: Vec<(crate::paths::Seen, LineSpan)> = ask
        .seen
        .iter()
        .map(|&(g, l)| {
            let n = names_of_group(g, &mut calls);
            let wrote = match n.is_empty() {
                true => crate::paths::Seen::NotNamed,
                false => crate::paths::Seen::Named(front_of(&n)),
            };
            (wrote, l)
        })
        .collect();
    seen.extend(ask.unnamed.map(|l| (crate::paths::Seen::NotNamed, l)));
    let front = match ask.group {
        Some(g) => front_of(&names_of_group(g, &mut calls)),
        None => Front::Elsewhere,
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
        seen.iter().any(|&(x, s)| w(x) && s.holds(line))
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
        seen,
        probes,
        programs: None,
        folder,
        started,
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
    owed_since: Option<T>,
    last_batch: Option<T>,
}

impl<T: Moment> Default for Pace<T> {
    fn default() -> Pace<T> {
        Pace {
            bytes: None,
            last_read: None,
            owed_since: None,
            last_batch: None,
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

    /// A scan met a path with no answer.
    pub fn owe(&mut self, now: T) {
        if self.owed_since.is_none() {
            self.owed_since = Some(now);
        }
    }

    /// **The batch's deadline**: [`SETTLE`] after the last read, but no later
    /// than [`CEILING`] after the path was met, and never sooner than
    /// [`CEILING`] after the last batch. None while nothing is owed.
    pub fn due_at(&self) -> Option<T> {
        let since = self.owed_since?;
        let quiet = self.last_read.map_or(since, |r| r.after(SETTLE).max(since));
        let soon = quiet.min(since.after(CEILING));
        Some(match self.last_batch {
            Some(b) => soon.max(b.after(CEILING)),
            None => soon,
        })
    }

    /// A batch went at `now`.
    pub fn sent(&mut self, now: T) {
        self.last_batch = Some(now);
        self.owed_since = None;
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
    pub hangs: u64,
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

/// How a driver starts the worker.
pub type Start = Box<dyn FnMut() -> std::io::Result<Worker>>;

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
    /// Each group seen at a read since the last batch, with the lines it
    /// wrote, and the lines written past [`GROUPS`] of them.
    seen: Vec<(Pid, LineSpan)>,
    unnamed: Option<LineSpan>,
    /// What to find a program for, the driver's list from the config, and
    /// whether it is owed an ask.
    programs: Vec<Program>,
    programs_owed: Owed,
    /// What the rows drew before a batch's scan, kept between batches so
    /// the compare that decides its frame allocates nothing once grown.
    before: Before,
    pub counts: Counts,
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
            seen: Vec::new(),
            unnamed: None,
            programs: Vec::new(),
            programs_owed: Owed::No,
            before: Before::default(),
            counts: Counts::default(),
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

    /// **A drain of reads was taken as one span**, the cursor on `began`
    /// before the first and on `ended` after the last, as macOS feeds a
    /// drain: the span leaves the cursor's line out only when no read could
    /// have written on it ([`crate::reach::Reach::then`]), and every read
    /// is filed under the most cautious verdict of the drain's
    /// ([`crate::ran::Ran::or`]), so no read's verdict lowers rows another
    /// read in the span wrote: a local command's read lifts nothing a read
    /// naming nothing wrote, and the shell's own read drops no sample or
    /// watch's verdict of a command's. Each read's groups are kept as
    /// [`Probes::read_taken`] keeps them.
    ///
    /// **A drain a read was cut in at the shell's `D` is three spans**
    /// (`cut.rs`): the parts before the cut, from `began` to the cut's line,
    /// that line only if they could have written on it; the parts from the
    /// cut on over the cut's line alone, under the most cautious verdict of
    /// their own; and the lines below it to `ended`, which the shell's bytes
    /// moved over, under every part's verdict as it would have been uncut --
    /// a remote program can leave text below its cursor that no span filed,
    /// and the shell's lines feeds pass over it without writing it. So a
    /// prompt of two lines keeps its second line dark, failing closed. A
    /// cut whose line `term` cannot number -- the surface switched, the grid
    /// resized, lines taken back since, or the bytes after it ending above
    /// it -- is no cut: the drain is filed as it would have been without it.
    pub fn drain_taken(
        &mut self,
        paths: &mut Paths,
        term: &Term<'_>,
        began: AbsLine,
        ended: AbsLine,
        reads: impl Iterator<Item = (Writers, Linked)> + Clone,
    ) {
        let cut = reads
            .clone()
            .enumerate()
            .find_map(|(k, (w, _))| match w.cut {
                crate::cut::Cut::After(m) => Some((k, m)),
                crate::cut::Cut::None | crate::cut::Cut::Plan(_) => None,
            });
        let line = cut.and_then(|(_, m)| {
            m.spot
                .line(term, ended)
                .filter(|l| began <= *l && *l <= ended)
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
        // What a run of parts reached, drained one after another.
        let reach = |parts: &mut dyn Iterator<Item = (Writers, Linked)>| {
            let first = parts.next()?.0.reach;
            Some(parts.fold(first, |r, (w, _)| r.then(w.reach)))
        };
        match (cut, line) {
            (Some((k, _)), Some(line)) => {
                if let Some(r) = reach(&mut reads.clone().take(k)) {
                    self.drain_span(paths, LineSpan::read(began, line, r), reads.clone().take(k));
                }
                let Some(r) = reach(&mut reads.clone().skip(k)) else {
                    return;
                };
                let after = LineSpan::read(line, ended, r);
                self.drain_span(paths, LineSpan::of(line, line), reads.skip(k));
                if after.last() > line {
                    self.drain_span(paths, LineSpan::of(line.below(), after.last()), uncut);
                }
            }
            _ => {
                if let Some(r) = reach(&mut uncut.clone()) {
                    self.drain_span(paths, LineSpan::read(began, ended, r), uncut);
                }
            }
        }
    }

    /// Parts of a drain filed over `span`, under the most cautious verdict
    /// of them ([`Probes::drain_taken`]).
    fn drain_span(
        &mut self,
        paths: &mut Paths,
        span: LineSpan,
        reads: impl Iterator<Item = (Writers, Linked)> + Clone,
    ) {
        let Some(ran) = reads
            .clone()
            .map(|(w, _)| w.marked)
            .reduce(crate::ran::Ran::or)
        else {
            return;
        };
        for (w, linked) in reads {
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
        if groups == [None, None] && !remote && !here && !shell {
            return;
        }
        let kept = match paths.marks() {
            Marks::On => true,
            Marks::Off => linked == Linked::Yes,
            Marks::Closed => false,
        };
        if !kept || matches!(self.slot, Slot::Closed) {
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
        }
        for g in groups.into_iter().flatten() {
            self.group_wrote(paths, g, span);
        }
    }

    /// `g` wrote `span`: kept with its lines, and named now if it is new.
    /// Its rows join its last span only where they touch it: a group seen at
    /// two reads with another's rows between keeps two spans, so its verdict
    /// covers its own rows and not the rows between, and it is named once.
    fn group_wrote(&mut self, paths: &mut Paths, g: Pid, span: LineSpan) {
        let known = self.seen.iter().any(|e| e.0 == g);
        if let Some(e) = self.seen.iter_mut().rev().find(|e| e.0 == g) {
            if e.1.touches(span) {
                e.1 = e.1.joined(span);
                return;
            }
        }
        if self.seen.len() >= GROUPS {
            self.unnamed = joined(self.unnamed, Some(span));
            return;
        }
        self.seen.push((g, span));
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
                cursor: span.last(),
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
                let named = !self.seen.is_empty() || self.unnamed.is_some();
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
                let (before, origins) = (s.paths.generation(), s.paths.origins());
                for &(f, span) in &a.seen {
                    s.paths.written_under(f, span);
                }
                // A span gone from history is dropped as the next arrives,
                // on the primary screen, whose history the spans number.
                if !a.seen.is_empty() && s.term.screen() == baud_core::Screen::Normal {
                    s.paths.trim_spans(crate::keep::oldest(s.term, s.scrolled));
                }
                s.paths.set_front(front, a.cursor);
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
        let mut turn = Turn::default();
        match s.paths.marks() {
            Marks::On => {}
            Marks::Closed => return turn,
            // Paths off: only the groups a link's read saw, sent to be
            // named, with no screen read and nothing looked up.
            Marks::Off => {
                if (self.seen.is_empty() && self.unnamed.is_none())
                    || matches!(self.slot, Slot::Closed)
                {
                    return turn;
                }
                let ask = Ask {
                    id: self.next_id,
                    kind: Kind::Screen,
                    group: group(),
                    cursor: s.scrolled.line(Rows::ZERO, s.term.cursor().0),
                    paths: Vec::new(),
                    seen: core::mem::take(&mut self.seen),
                    unnamed: self.unnamed.take(),
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
                cursor: s.scrolled.line(Rows::ZERO, s.term.cursor().0),
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
        if s.paths.owed() == Owed::Yes {
            self.pace.owe(now);
        }
        let due = s.paths.owed() == Owed::Yes && self.pace.due_at().is_some_and(|d| now >= d);
        if !due || matches!(self.slot, Slot::Closed) {
            return turn;
        }
        self.pace.sent(now);
        let cursor = s.scrolled.line(Rows::ZERO, s.term.cursor().0);
        let g = group();
        if g.is_some() && g == self.elsewhere {
            // The same group, not local: its rows to here stay unmarked, and
            // nothing is asked.
            s.paths.set_front(Front::Elsewhere, cursor);
            s.paths.settled();
            // Every row to the cursor is now this group's: what was seen
            // before it is covered, and was written while it was in front.
            for (_, span) in self.seen.drain(..) {
                s.paths
                    .written_under(crate::paths::Seen::Named(Front::Elsewhere), span);
            }
            if let Some(span) = self.unnamed.take() {
                s.paths
                    .written_under(crate::paths::Seen::Named(Front::Elsewhere), span);
            }
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
        if queue.is_empty() && self.seen.is_empty() && self.unnamed.is_none() {
            return turn;
        }
        self.counts.asked += u64::try_from(queue.len()).unwrap_or(u64::MAX);
        self.counts.batches += 1;
        let ask = Ask {
            id: self.next_id,
            kind: Kind::Screen,
            group: g,
            cursor,
            paths: queue,
            seen: core::mem::take(&mut self.seen),
            unnamed: self.unnamed.take(),
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
        let cursor = s.scrolled.line(Rows::ZERO, s.term.cursor().0);
        let id = self.next_id;
        self.counts.asked += 1;
        let ask = Ask {
            id,
            kind: Kind::Click,
            group,
            cursor,
            paths: vec![path],
            seen: self.seen.clone(),
            unnamed: self.unnamed,
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
        paths.close();
        lines.forget();
        turn.frame = Owed::Yes;
        if self.click.take().is_some() {
            turn.clicked = Some(Clicked::Unchecked);
        }
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

/// **The Linux loop's wake**: a socket pair the worker writes a byte to after
/// each answer, which the loop's `poll` waits on beside the rest.
#[cfg(target_os = "linux")]
pub struct Woken {
    read: std::os::unix::net::UnixStream,
}

#[cfg(target_os = "linux")]
impl Woken {
    /// The pair, both ends non-blocking, and the worker's call.
    pub fn pair() -> std::io::Result<(Woken, impl Fn() + Send + Clone + 'static)> {
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
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

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
    }

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
                    cursor: AbsLine::at(Rows::ZERO),
                    paths: vec![
                        cand("/etc/hosts", crate::paths::Form::Absolute),
                        cand("/w/x", crate::paths::Form::Dot),
                    ],
                    seen: Vec::new(),
                    unnamed: None,
                    programs: Vec::new(),
                },
                &mut HashMap::new(),
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
        p.owe(ms(0));
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
        p.sent(ms(1000));
        p.owe(ms(1010));
        p.saw(Taken(99), ms(1020));
        assert_eq!(p.due_at(), Some(ms(2000)), "never two in a second");
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
        let r = turn(
            &mut pr,
            at(1_000 + 5_001),
            &mut paths,
            &mut lines,
            &mut uris,
        );
        assert_eq!(r.frame, Owed::Yes, "closing owes a frame");
        assert_eq!(paths.marks(), Marks::Closed);
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
        pr.read_taken(
            &mut paths,
            Writers::at(Some(read(0))),
            LineSpan::of(top, line),
            Linked::No,
        );
        let t0 = Instant::now();
        for step in 0..40 {
            let now = t0 + Duration::from_millis(100 * u64::from(step));
            // Another group in front writes: a read of its, on the last row.
            if step > 0 && read(step) != read(step - 1) {
                let b = b"\x1b[4;1Hx";
                t.write_at(BytePos::ZERO, b);
                t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
                let line = s.line(Rows::ZERO, t.cursor().0);
                pr.read_taken(
                    &mut paths,
                    Writers::at(Some(read(step))),
                    LineSpan::of(line, line),
                    Linked::No,
                );
            }
            if step_on(step, &mut t) == Moved::Yes {
                let line = s.line(Rows::ZERO, t.cursor().0);
                let top = s.line(Rows::ZERO, baud_domain::Row::ZERO);
                pr.read_taken(
                    &mut paths,
                    Writers::at(Some(read(step))),
                    LineSpan::of(top, line),
                    Linked::No,
                );
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
    /// a read without one, inside the pace, does not. Named breaks: ask by
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
                            cursor: scrolled.line(Rows::ZERO, t.cursor().0),
                            paths: queue,
                            seen: Vec::new(),
                            unnamed: None,
                            programs: Vec::new(),
                        },
                        named,
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
    }

    /// What the window does once a read's drain is taken: nothing, or a
    /// resize to this size, numbered as the drivers number one
    /// (`keep::kept_across_resize`).
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Then {
        Nothing,
        /// To this many columns and rows.
        Resize(u16, u16),
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
    /// and the watch's verdict, and each drain's reads given the drain's
    /// lines, from where the cursor began to where it ended
    /// (`LineSpan::read` of its last read), as the drivers give them. `f`
    /// names the groups.
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
        let (mut pr, _) = probes(f);
        let most = reads
            .iter()
            .fold((ws.ws_col, ws.ws_row), |(c, r), read| match read.then {
                Then::Resize(cols, rows) => (c.max(cols), r.max(rows)),
                Then::Nothing => (c, r),
            });
        let most = WinSize {
            ws_col: most.0,
            ws_row: most.1,
        };
        let mut owned = Owned::new(&most);
        let mut t = Term::new(owned.storage(), &ws);
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
        let (mut lines, mut uris) = (Lines::default(), Uris::default());
        let mut s = Scrolled::START;
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
            let began = s.line(Rows::ZERO, t.cursor().0);
            let before = (numbering == Numbering::Driven)
                .then(|| crate::keep::Before::watched(&mut t, None, s));
            let mut taken = Vec::new();
            let mut now = t0;
            for (k, r) in drain.iter().enumerate() {
                now = t0 + GROUP_EVERY * u32::try_from(i + k + 1).expect("a count");
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
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            if let Some(before) = before {
                s = match crate::keep::numbered(&t, before, None) {
                    crate::keep::Ring::Retired(k) => s.advanced(k),
                    crate::keep::Ring::Restored(k) => s.retreated(k).unwrap_or(Scrolled::START),
                };
            }
            let ended = s.line(Rows::ZERO, t.cursor().0);
            if !drain.iter().any(|r| r.sampled == Sampled::Unfiled) {
                pr.drain_taken(
                    &mut paths,
                    &t,
                    began,
                    ended,
                    took.into_iter().map(|w| (w, Linked::No)),
                );
            }
            if let Then::Resize(ws_col, ws_row) = drain[n - 1].then {
                let was = (t.history_mark(), t.geometry().0, t.screen());
                let _ = t.resize(&WinSize { ws_col, ws_row });
                if numbering == Numbering::Driven {
                    s = crate::keep::kept_across_resize(&t, was, None, s).1;
                }
            }
            i += n;
        }
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
        // Batches a second apart, each answer taken, until the screen has
        // been asked about as it stands: a row a later answer makes
        // readable is walked by the batch after it.
        let mut now = t0 + Duration::from_secs(60);
        for _ in 0..4 {
            now += Duration::from_secs(1);
            let _ = turn(&mut pr, now);
            while pr.flight.is_some() {
                std::thread::yield_now();
                let _ = turn(&mut pr, now);
            }
        }
        if let Some(n) = trim {
            paths.trim_spans(s.line(Rows::ZERO, baud_domain::Row::from_index(n)));
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
        (rows, origins, looked)
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
            let _ = sampler.read(t0 + GROUP_EVERY * k, bytes);
            let w = sampler.unasked();
            t.write_at(BytePos::ZERO, bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
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
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = s.line(Rows::ZERO, t.cursor().0);
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
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.drain_taken(
                &mut paths,
                &t,
                began,
                ended,
                took.into_iter().map(|w| (w, Linked::No)),
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
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = s.line(Rows::ZERO, t.cursor().0);
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
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.drain_taken(
                &mut paths,
                &t,
                began,
                ended,
                took.into_iter().map(|w| (w, Linked::No)),
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
        let t0 = Instant::now();
        let mut call = |reads: &[&[u8]], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = s.line(Rows::ZERO, t.cursor().0);
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
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.drain_taken(
                &mut paths,
                &t,
                began,
                ended,
                took.into_iter().map(|w| (w, Linked::No)),
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
        let t0 = Instant::now();
        let mut call = |read: &[u8], ms: u64| {
            let now = t0 + Duration::from_millis(ms);
            let began = s.line(Rows::ZERO, t.cursor().0);
            let _ = sampler.read(now, read);
            let tagged = [(read, sampler.unasked())];
            let mut took = Vec::new();
            let _ = crate::reads::paced(
                &mut t,
                &mut pace,
                now,
                tagged,
                |_| {},
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.drain_taken(
                &mut paths,
                &t,
                began,
                ended,
                took.into_iter().map(|w| (w, Linked::No)),
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
        let t0 = Instant::now();
        let prompt = [
            prompt_after("").as_slice(),
            b"\x1b]7;file://here.example/home/u\x07",
        ]
        .concat();
        let mut batches: Vec<Vec<(u64, Vec<u8>, Pid)>> = vec![
            vec![(0, ran("ssh h", ";cmdline=ssh\\ h"), 1)],
            vec![(50, b"remote$ exit\r\n".to_vec(), 9)],
        ];
        match drains {
            Drains::Each => {
                batches.push(vec![(55, b"logout\r\n".to_vec(), 9)]);
                batches.push(vec![(58, prompt, 1)]);
            }
            Drains::LastTwo => {
                batches.push(vec![(55, b"logout\r\n".to_vec(), 9), (58, prompt, 1)]);
            }
        }
        for (k, b) in b"./oct-commits.txt".iter().enumerate() {
            let at = 200 + 100 * u64::try_from(k).expect("a key");
            batches.push(vec![(at, vec![*b], 1)]);
        }
        let mut last = t0;
        for batch in &batches {
            let began = s.line(Rows::ZERO, t.cursor().0);
            let mut tagged = Vec::new();
            for (ms, bytes, front) in batch {
                let now = t0 + Duration::from_millis(*ms);
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
                |_| {},
                &mut crate::hostlog::NoLog,
                &mut took,
            );
            let ended = s.line(Rows::ZERO, t.cursor().0);
            pr.drain_taken(
                &mut paths,
                &t,
                began,
                ended,
                took.into_iter().map(|w| (w, Linked::No)),
            );
            let _ = paths.observe(&t, s);
            let _ = frame(&mut pr, &t, &mut lines, &mut uris, &mut paths, last);
        }
        // A frame each 10 ms after the last key, to the first batch the
        // pace sends, and the frame after it, which takes its answer.
        let mut k = 0u32;
        loop {
            k += 1;
            let now = last + Duration::from_millis(10) * k;
            if frame(&mut pr, &t, &mut lines, &mut uris, &mut paths, now) == Owed::Yes {
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
    const CLOSED: &[LinkStage] = &[];

    /// **A pending row's verdict**: while `stage` is open, `now` must fail
    /// exactly as `recorded`; red if it is `fixed` early, fails another
    /// way, or is still here once its stage has closed.
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

    /// **Pending on the core record (F5): a row a remote read wrote below
    /// its span, coming back up after, is filed over by the prompt that
    /// lands there, and lights.** [`below_and_back`]'s reads, in two
    /// shapes: `logout` in ssh's read and the prompt in a read of its own,
    /// filed as the shell's; and `logout`, the `D` and the prompt in one
    /// read, which the cut reaches the row through. Today the row lights in
    /// both; once the core records the lowest line each read reached, it
    /// stays dark. Expected-red ([`pending`]): red if either goes dark
    /// before the core record, lights another row, or outlives that stage.
    #[test]
    fn pending_core_record_a_row_written_below_a_remote_reads_span() {
        let [start, remote, logout, prompt] = below_and_back();
        let exit = [logout.as_slice(), &prompt].concat();
        let shapes = [
            lit_reads(
                named_groups(),
                &[
                    read(&start, 1),
                    read(&remote, 9),
                    read(&logout, 9),
                    read(&prompt, 1),
                ],
            ),
            lit_reads(
                named_groups(),
                &[read(&start, 1), read(&remote, 9), read(&exit, 1)],
            ),
        ];
        pending(
            LinkStage::CoreRecord,
            shapes,
            [vec![2], vec![2]],
            [Vec::new(), Vec::new()],
        );
    }

    /// **Pending on the core record (H1): after a full-screen program that
    /// scrolled, the primary's rows are looked up lines off, and a remote
    /// row reads a local row's writer.** `/etc/passwd` printed under `ssh`,
    /// then a local `less` on the alternate screen scrolls it two lines and
    /// quits, and the shell's prompt. The drivers number the window by one
    /// count that the alternate screen's scrolling moves too, and leaving
    /// it moves nothing back, so row 0 is looked up as a line `less` wrote,
    /// and lights. Once each screen keeps its own count it stays dark.
    /// Expected-red ([`pending`]).
    #[test]
    fn pending_core_record_a_remote_row_after_a_scrolled_pager() {
        let groups = || {
            let mut f = named_groups();
            f.groups.insert(3, vec!["less".into()]);
            f
        };
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
        pending(LinkStage::CoreRecord, lit, vec![0], Vec::new());
    }

    /// **Pending on the core record (H2): text a later local program moves
    /// keeps the writer of the row it lands on.** `ssh h` run, and
    /// `/etc/passwd` printed under it on row 1; it ends, and the shell's
    /// prompt follows on row 2, the shell's by its marks; then a local
    /// program inserts a line at row 1 (`CSI L`), which moves the remote
    /// text onto row 2, a row only the shell wrote, and it lights. Once the
    /// core records moved rows, filed with the most cautious writer they
    /// held, it stays dark. Expected-red ([`pending`]).
    #[test]
    fn pending_core_record_remote_text_moved_onto_a_local_row() {
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
        pending(
            LinkStage::CoreRecord,
            moved(b"\x1b[2;1H\x1b[L"),
            vec![2],
            Vec::new(),
        );
    }

    /// **Pending on the core record (H3): a line feed that cannot move the
    /// cursor still leaves its row to the next writer.** Under `ssh h`, a
    /// scrolling region over the top rows and the cursor on the row above
    /// the last; then a read of a line feed, a path on the last row, and a
    /// carriage return and line feed -- which, the cursor on the last row
    /// below the region, does not move it. The read is filed as if its
    /// final line feed had left the last row clean, so the shell's prompt
    /// written there next, the shell's by its marks, makes the row the
    /// shell's, and the path lights. Once the core records that the line
    /// feed did not move, the row stays the remote read's. Expected-red
    /// ([`pending`]).
    #[test]
    fn pending_core_record_a_line_feed_that_does_not_move() {
        let ws = WinSize {
            ws_col: 40,
            ws_row: 10,
        };
        let (start, prompt) = (ran("ssh h", ";cmdline=ssh\\ h"), prompt_after(""));
        let at = |region: &[u8]| {
            let ws = WinSize {
                ws_col: ws.ws_col,
                ws_row: ws.ws_row,
            };
            let reads = [
                read(&start, 1),
                read(region, 9),
                read(b"\n   /etc/passwd\r\n", 9),
                read(&prompt, 1),
            ];
            taken_on(ws, named_groups(), &reads).0
        };
        assert_eq!(
            at(b"\x1b[1;5r\x1b[8;1H"),
            Vec::<usize>::new(),
            "the control: a row higher, the last line feed moves"
        );
        pending(
            LinkStage::CoreRecord,
            at(b"\x1b[1;5r\x1b[9;1H"),
            vec![9],
            Vec::new(),
        );
    }

    /// **Pending on the core record (H4): a resize that re-wraps lines
    /// moves text whose writer baud could not name onto a row a local
    /// program wrote.** A local line wrapped over rows 0 and 1, then
    /// `/etc/passwd` on row 2 filed under no writer, then a local prompt;
    /// the window widened, the wrapped line takes one row and the path
    /// moves up to row 1, which is filed local, and lights. Once a
    /// renumbering never files a local writer over lines it did not write,
    /// it stays dark. Expected-red ([`pending`]).
    #[test]
    fn pending_core_record_a_resize_moves_unnamed_text_onto_a_local_row() {
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
        pending(
            LinkStage::CoreRecord,
            resized(Then::Resize(40, 6)),
            vec![1],
            Vec::new(),
        );
    }

    /// **Where F5 does not reach, the row written below stays dark**:
    /// [`below_and_back`]'s bytes all in one read, whose move up refuses
    /// the cut, and its reads taken in one drain, whose span reaches the
    /// prompt's row under the remote read. Named breaks: let any escape
    /// before the `D` through (`Forward::of`) and file the part before the
    /// cut by the plan's reach, not its own bytes' (`reads::paced_one`) --
    /// the one read lights; neither alone does, each holding it; a drain's
    /// reach the last read's alone (`Reach::then`) -- the drain lights.
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
}
