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
//!   [`GROUP_EVERY`]), each distinct group kept with the last line written
//!   under it ([`Probes::read_taken`]), and named by the worker at once,
//!   while it may still run. A batch carries them; a path printed only on
//!   rows up to a group named not local is not looked up, and those rows
//!   keep no mark (`Paths::written_under`). A group gone before it was named
//!   counts as not local.
//! * **No cloud file fetched to answer a lookup.** On macOS the worker sets
//!   `IOPOL_MATERIALIZE_DATALESS_FILES_OFF` on its own thread first
//!   (`sys::no_dataless_fetch`), so an `lstat` inside a File Provider folder
//!   that is only a placeholder does not fetch it. The call is accepted on
//!   this Mac; what it prevents is unverified.
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
use crate::lined::{Lines, Uris};
use crate::paths::{Candidate, Front, Links, Marks, Owed, Paths, Probe, Turned};
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

/// The distinct groups kept between two batches; past it, every line
/// written after counts as not local until the next.
pub const GROUPS: usize = 16;

/// The groups the worker keeps named, ahead of the batches that ask.
const NAMED: usize = 64;

/// The most symbolic links one walk follows by hand, as the kernel bounds its
/// own walk (`MAXSYMLINKS`, 32 on macOS): past it the answer is unknown.
pub const LINKS: usize = 32;

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
    /// Run on the worker's thread before anything else.
    fn prepare(&mut self) {}
}

/// **When the thread taking the reads asks for the foreground group**:
/// at a read [`GROUP_EVERY`] or more after it last did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sampler {
    last: Option<std::time::Instant>,
}

impl Sampler {
    /// Whether the read at `now` asks.
    pub fn due(&mut self, now: std::time::Instant) -> bool {
        let due = self
            .last
            .is_none_or(|l| now.duration_since(l) >= GROUP_EVERY);
        if due {
            self.last = Some(now);
        }
        due
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
    /// Each group seen at a read since the last batch, with the last line
    /// written under it, and the last line past [`GROUPS`] of them.
    pub seen: Vec<(Pid, AbsLine)>,
    pub unnamed: Option<AbsLine>,
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
    /// What each group seen since the last batch was, to its last line.
    pub seen: Vec<(Front, AbsLine)>,
    pub probes: Vec<(Vec<u8>, Probe)>,
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
        calls: 0,
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
            Some(e) => e.1 = e.1.max(*l),
            None => newer.seen.push((*g, *l)),
        }
    }
    newer.unnamed = newer.unnamed.max(older.unnamed);
    *slot = Some(newer);
    answers.send(left(&older)).is_ok()
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
/// newest screen batch, which a newer one replaces before it is taken and
/// ends between two paths. Every click and screen batch sent gets one
/// answer, a replaced one included.
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
    let mut clicks: std::collections::VecDeque<Ask> = std::collections::VecDeque::new();
    let mut groups: std::collections::VecDeque<Pid> = std::collections::VecDeque::new();
    let mut named: std::collections::HashMap<Pid, Vec<String>> = std::collections::HashMap::new();
    // The calls naming groups made, carried into the next answer's count.
    let mut naming = 0;
    // Where an ask goes; false once the driver is gone.
    let file = |a: Ask,
                screen: &mut Option<Ask>,
                clicks: &mut std::collections::VecDeque<Ask>,
                groups: &mut std::collections::VecDeque<Pid>|
     -> bool {
        match a.kind {
            Kind::Click => clicks.push_back(a),
            Kind::Name => groups.extend(a.group),
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
        if screen.is_none() && clicks.is_empty() && groups.is_empty() {
            match asks.recv() {
                Ok(a) => {
                    if !file(a, &mut screen, &mut clicks, &mut groups) {
                        return;
                    }
                }
                Err(_) => return,
            }
        }
        loop {
            match asks.try_recv() {
                Ok(a) => {
                    if !file(a, &mut screen, &mut clicks, &mut groups) {
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
            let n = fs.program(g);
            naming += 1;
            beat();
            named.insert(g, n);
        }
        let Some(ask) = clicks.pop_front().or_else(|| screen.take()) else {
            continue;
        };
        let mut gone = false;
        let mut stop = || loop {
            match asks.try_recv() {
                Ok(a) if a.kind == Kind::Screen => {
                    gone |= !file(a, &mut screen, &mut clicks, &mut groups);
                    return true;
                }
                Ok(a) => gone |= !file(a, &mut screen, &mut clicks, &mut groups),
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
    named: &mut std::collections::HashMap<Pid, Vec<String>>,
    beat: &dyn Fn(),
    stop: &mut dyn FnMut() -> bool,
) -> Answer {
    let mut calls = 0;
    // A group the worker named when it was seen, else named now: one gone by
    // then names nothing, which is not local.
    let mut front_of_group = |g: Pid, calls: &mut u64| -> Front {
        if let Some(n) = named.get(&g) {
            return front_of(n);
        }
        let n = fs.program(g);
        *calls += 1;
        beat();
        front_of(&n)
    };
    let mut seen: Vec<(Front, AbsLine)> = ask
        .seen
        .iter()
        .map(|&(g, l)| (front_of_group(g, &mut calls), l))
        .collect();
    seen.extend(ask.unnamed.map(|l| (Front::Elsewhere, l)));
    let front = match ask.group {
        Some(g) => front_of_group(g, &mut calls),
        None => Front::Elsewhere,
    };
    for (g, _) in &ask.seen {
        if Some(*g) != ask.group {
            named.remove(g);
        }
    }
    let through = |f: Front| seen.iter().filter(|s| s.0 == f).map(|s| s.1).max();
    let (elsewhere, multiplexed) = (through(Front::Elsewhere), through(Front::Multiplexer));
    let mut probes = Vec::new();
    if front != Front::Elsewhere && !ask.paths.is_empty() {
        let mounts = fs.mounts();
        calls += 1;
        beat();
        for c in ask.paths {
            // Printed only on rows a program not on this machine wrote.
            if elsewhere.is_some_and(|t| c.line <= t) {
                continue;
            }
            let muxed = front == Front::Multiplexer || multiplexed.is_some_and(|t| c.line <= t);
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
        calls,
    }
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
    /// Each group seen at a read since the last batch, with the last line
    /// written under it, and the last line written past [`GROUPS`] of them.
    seen: Vec<(Pid, AbsLine)>,
    unnamed: Option<AbsLine>,
    pub counts: Counts,
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
            seen: Vec::new(),
            unnamed: None,
            counts: Counts::default(),
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

    /// **A read was taken with `group` in front, and the cursor ended it on
    /// `line`**: the group kept with its last line, and a new one sent to
    /// be named at once, while it may still run. `group` is the
    /// [`Sampler`]'s: none for a read that did not ask, or a group the
    /// kernel no longer names.
    pub fn read_taken(&mut self, paths: &mut Paths, group: Option<Pid>, line: AbsLine) {
        let Some(g) = group else {
            return;
        };
        if paths.marks() != Marks::On || matches!(self.slot, Slot::Closed) {
            return;
        }
        if let Some(e) = self.seen.iter_mut().find(|e| e.0 == g) {
            e.1 = e.1.max(line);
            return;
        }
        if self.seen.len() >= GROUPS {
            self.unnamed = self.unnamed.max(Some(line));
            return;
        }
        self.seen.push((g, line));
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
                cursor: line,
                paths: Vec::new(),
                seen: Vec::new(),
                unnamed: None,
            });
        }
    }

    /// **The next instant a turn is owed by the clock**: the batch's
    /// deadline while one is owed, and the hang's while one is out. For the
    /// Linux loop's wait; macOS's display link ticks anyway.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn deadline(&self, paths: &Paths, now: T) -> Option<T> {
        if paths.marks() != Marks::On {
            return None;
        }
        // A frame's scan owes a batch before any turn has timed it: the next
        // turn, now, times it.
        let due = match paths.owed() {
            Owed::Yes => Some(self.pace.due_at().unwrap_or(now)),
            Owed::No => None,
        };
        let hang = self.flight.map(|f| f.at.after(HANG));
        [due, hang].into_iter().flatten().min()
    }

    /// **One turn, at `now`**: answers merged, the hang watched, and a batch
    /// sent once it is due. `group` reads the terminal's foreground group,
    /// a `TIOCGPGRP` on the master, only when a batch goes.
    pub fn turn(&mut self, now: T, s: Seen<'_, '_>, group: impl FnOnce() -> Option<Pid>) -> Turn {
        let mut turn = Turn::default();
        if s.paths.marks() != Marks::On {
            return turn;
        }
        self.pace.saw(Taken(s.term.amplification().0), now);
        // Answers first: a hang is a batch with none.
        let (answers, beats) = match &self.slot {
            Slot::Running(w) => {
                let got: Vec<Answer> = core::iter::from_fn(|| w.rx.try_recv().ok()).collect();
                (got, Some(w.beats()))
            }
            Slot::Unstarted | Slot::Closed => (Vec::new(), None),
        };
        if let Some(beats) = beats {
            for a in answers {
                self.counts.calls += a.calls;
                self.answered = self.answered.next();
                if self.answered == self.sent {
                    self.flight = None;
                }
                // A batch left for a newer one: its groups went with that.
                let Some(front) = a.front else {
                    continue;
                };
                match front {
                    Front::Elsewhere => self.elsewhere = a.group,
                    Front::Local | Front::Multiplexer => self.elsewhere = None,
                }
                let before = s.paths.generation();
                for &(f, line) in &a.seen {
                    s.paths.written_under(f, line);
                }
                s.paths.set_front(front, a.cursor);
                let clicked = (a.kind == Kind::Click && self.click == Some(a.id)).then(|| {
                    self.click = None;
                    match a.probes.first() {
                        Some((p, Probe::Exists)) => Clicked::Exists(p.clone()),
                        Some((_, Probe::Missing)) => Clicked::Missing,
                        Some((_, Probe::Unknown)) | None => Clicked::Unchecked,
                    }
                });
                let _ = s.paths.merge(a.probes);
                if s.paths.generation() != before {
                    s.lines.forget();
                    turn.frame = Owed::Yes;
                }
                if clicked.is_some() {
                    turn.clicked = clicked;
                }
            }
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
            // before it is covered.
            self.seen.clear();
            self.unnamed = None;
            return turn;
        }
        s.paths.front_moved(cursor);
        s.paths.begin_collect();
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
        turn.frame = Owed::Yes;
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

    /// On macOS, no File Provider placeholder fetched by this thread's
    /// lookups (`sys::no_dataless_fetch`).
    fn prepare(&mut self) {
        #[cfg(target_os = "macos")]
        if !crate::sys::no_dataless_fetch() {
            crate::warn!("paths: the worker could not turn cloud file fetches off for itself");
        }
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
    use crate::paths::Forms;
    use baud_core::Owned;
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
            self.groups
                .get(&group)
                .cloned()
                .unwrap_or_else(|| self.program.clone())
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
    /// looked up**; under a multiplexer only `./` is left out. Perturbation:
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
        let sent = turn(&mut pr, at(200), &mut paths, &mut lines, &mut uris);
        assert_eq!(sent.frame, Owed::Yes, "the batch went");
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
            let (mut lines, mut uris) = (Lines::default(), Uris::default());
            let s = Scrolled::START;
            let printed = s.line(Rows::ZERO, baud_domain::Row::ZERO);
            pr.read_taken(&mut paths, Some(at_read), printed);
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

    /// **A result arrives with no cell changed, and the lines read again once
    /// for it**: the batch's answer moves the lines, and a turn with no answer
    /// owes nothing. Perturbation: drop `lines.forget()` on an answer.
    #[test]
    fn an_answer_reads_the_lines_again_once() {
        let f = Fake::with(&[("/etc", Entry::Dir), ("/etc/hosts", Entry::File)]);
        let (mut pr, _) = probes(f);
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, b"/etc/hosts\r\n");
        t.feed(BytePos::ZERO, ReadLen::from_index(12));
        let mut paths = Paths::new(None, Forms::DEFAULT);
        paths.set(Turned::On);
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
        assert_eq!(
            pr.turn(t0 + Duration::from_millis(200), seen, || Some(1))
                .frame,
            Owed::Yes,
            "the batch went"
        );
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
        assert!(scan.rows > 0, "read again");
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
}
