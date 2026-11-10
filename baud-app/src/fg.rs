//! **The program in the foreground and the folder it is in**, read from
//! the kernel for the window's title when a program sets none (note AK,
//! section 2; `title.rs`).
//!
//! No standard covers any of it: POSIX's `tcgetpgrp` "shall fail if ... the
//! file is not the controlling terminal" (`[[source: posix-tcgetpgrp]]`),
//! and both kernels answer it on the master anyway (`sys.rs`). What is
//! read, and why that and not something cheaper:
//!
//! * **The group**: `TIOCGPGRP` on the master, the job the shell gave the
//!   terminal to, or the shell itself at its prompt. XNU's master answers 0
//!   when there is none, which is no group here.
//! * **Its name**: argv[0]'s last component, a login shell's leading `-`
//!   dropped -- what `ps` shows and what the user typed. macOS's
//!   `proc_name` is cheaper and answered `coreutils` for nix's `sleep`, a
//!   symlink to a multi-call binary (note AK 2.3, measured), where Linux's
//!   `comm` says `sleep`; argv[0] is the same on both. When argv[0] cannot
//!   be read -- a root process under `sudo` -- the executable's file name
//!   stands in.
//! * **Its directory**: macOS's `PROC_PIDVNODEPATHINFO`, Linux's
//!   `/proc/pid/cwd` (`[[source: linux-man-proc-pid-cwd]]`), each refused
//!   for a process the user cannot trace.
//!
//! On macOS the reads are Apple's "private interfaces ... subject to change
//! in future releases" (`[[source: macos-sdk-libproc-h]]` 41-44), the only
//! ones there are; a call that fails reads as nothing, and the title says
//! what it still knows.

use crate::sys::{Fd, Pid};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

/// The pty's foreground process group, `None` when the kernel names none.
pub fn group(master: Fd) -> Option<Pid> {
    let mut g: Pid = 0;
    // SAFETY: `TIOCGPGRP` writes one `pid_t` through the pointer.
    let r = unsafe { crate::sys::ioctl(master, crate::sys::TIOCGPGRP, &raw mut g) };
    (r == 0 && g > 0).then_some(g)
}

/// The program `pid` runs, by argv[0], else by its executable's name.
pub fn name(pid: Pid) -> Option<String> {
    argv0(pid)
        .and_then(|a| program(&a))
        .or_else(|| exe(pid).and_then(|p| program(p.as_os_str().as_bytes())))
}

/// The program `pid` runs, by its executable's file name alone: what a
/// program that rewrote its arguments -- tmux's client, `tmux: client
/// (...)` on Linux -- is still known by (`probe::front_of`).
pub fn exe_name(pid: Pid) -> Option<String> {
    exe(pid).and_then(|p| program(p.as_os_str().as_bytes()))
}

/// argv[0]'s last path component, a login shell's leading `-` dropped
/// (`fork_in` starts the shell as `-zsh`, as `login` does).
pub fn program(argv0: &[u8]) -> Option<String> {
    let last = argv0.rsplit(|&b| b == b'/').next()?;
    let last = last.strip_prefix(b"-").unwrap_or(last);
    (!last.is_empty()).then(|| String::from_utf8_lossy(last).into_owned())
}

/// The first argument out of a `KERN_PROCARGS2` answer: "argc as the
/// first word", then "the bare executable path", which "may have multiple
/// null bytes after it for alignment", then argv (`[[source:
/// xnu-kern-sysctl]]` 1619-1640, 1706-1709).
#[cfg(any(target_os = "macos", test))]
pub fn procargs2_argv0(buf: &[u8]) -> Option<&[u8]> {
    let (argc, rest) = buf.split_first_chunk::<4>()?;
    if core::ffi::c_int::from_ne_bytes(*argc) < 1 {
        return None;
    }
    let path_end = rest.iter().position(|&b| b == 0)?;
    let after = &rest[path_end..];
    let start = after.iter().position(|&b| b != 0)?;
    let arg = &after[start..];
    let end = arg.iter().position(|&b| b == 0).unwrap_or(arg.len());
    Some(&arg[..end])
}

/// The bytes before the first NUL: a C string out of a buffer.
fn c_string(buf: &[u8]) -> &[u8] {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    &buf[..end]
}

/// argv[0] of `pid`: `KERN_PROCARGS2` asked twice, once for the size it
/// needs and once with a buffer that size, because a shorter buffer gets
/// "copied out `buflen` bytes from the end" -- the environment's end, not
/// argv's (`[[source: xnu-kern-sysctl]]` 1532-1536 and 1661-1693).
#[cfg(target_os = "macos")]
fn argv0(pid: Pid) -> Option<Vec<u8>> {
    use crate::sys::{CTL_KERN, KERN_PROCARGS2, MapLen, sysctl};
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut len = MapLen(0);
    let null = std::ptr::null_mut();
    // SAFETY: a three-name MIB and an out-length; no buffer, so the call
    // writes only the length.
    if unsafe { sysctl(mib.as_mut_ptr(), 3, null, &raw mut len, null, MapLen(0)) } != 0 {
        return None;
    }
    let mut buf = vec![0u8; len.0];
    // SAFETY: a buffer of the length passed beside it.
    if unsafe {
        sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            null,
            MapLen(0),
        )
    } != 0
    {
        return None;
    }
    buf.truncate(len.0);
    procargs2_argv0(&buf).map(<[u8]>::to_vec)
}

/// argv[0] of `pid`: `/proc/pid/cmdline`'s "set of strings separated by
/// null bytes", up to the first (`[[source: linux-man-proc-pid-cmdline]]`).
/// A process that rewrote its arguments as one line with no NULs (sshd's
/// `sshd: user@pts/0`) gives that whole line, and its last `/` component
/// is shown.
#[cfg(target_os = "linux")]
fn argv0(pid: Pid) -> Option<Vec<u8>> {
    let all = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    Some(c_string(&all).to_vec())
}

/// The executable `pid` runs: `proc_pidpath`, which still answers for a
/// root process (note AK 2.5, measured against pid 1).
#[cfg(target_os = "macos")]
fn exe(pid: Pid) -> Option<PathBuf> {
    use crate::sys::{PROC_PIDPATHINFO_MAXSIZE, PathSize, proc_pidpath};
    let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    let size = PathSize(u32::try_from(buf.len()).ok()?);
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe { proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
    let path = c_string(&buf[..usize::try_from(n).ok()?]);
    (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// The executable `pid` runs: `/proc/pid/exe`, "a symbolic link containing
/// the actual pathname of the executed command" (`[[source:
/// linux-man-proc-pid-exe]]`).
#[cfg(target_os = "linux")]
fn exe(pid: Pid) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// The directory `pid` is in: `pvi_cdir`'s path out of
/// `PROC_PIDVNODEPATHINFO`, symlinks resolved by the kernel (`/tmp` reads
/// `/private/tmp`). `proc_pidinfo` answers the bytes it wrote, 0 on a
/// failure (`[[source: xnu-libproc-c]]` 100-110).
#[cfg(target_os = "macos")]
pub fn dir(pid: Pid) -> Option<PathBuf> {
    use crate::sys::{
        MAXPATHLEN, PROC_PIDVNODEPATHINFO, ProcArg, VNODEPATHINFO_CDIR_PATH, VNODEPATHINFO_SIZE,
        proc_pidinfo,
    };
    let mut buf = vec![0u8; VNODEPATHINFO_SIZE];
    let size = core::ffi::c_int::try_from(buf.len()).ok()?;
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDVNODEPATHINFO,
            ProcArg(0),
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let path = c_string(&buf[VNODEPATHINFO_CDIR_PATH..VNODEPATHINFO_CDIR_PATH + MAXPATHLEN]);
    (!path.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(path)))
}

/// The directory `pid` is in: `/proc/pid/cwd`, "a symbolic link to the
/// current working directory of the process".
#[cfg(target_os = "linux")]
pub fn dir(pid: Pid) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// **When a process started**, as the kernel keeps it ([`started`]):
/// microseconds on macOS, clock ticks since boot on Linux. For equality
/// only: the same pid with another start time is another process, and the
/// value orders nothing and means nothing across the two systems, so it is
/// made and read only here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartTime(u64);

#[cfg(test)]
impl StartTime {
    /// Three start times for tests, told apart only by equality.
    pub(crate) const AT: [Self; 3] = [Self(0), Self(1), Self(2)];
}

/// **When `pid` started**, as the kernel keeps it: `PROC_PIDTBSDINFO`'s
/// `pbi_start_tvsec` and `pbi_start_tvusec`, as microseconds. None for a
/// process gone or one the user may not read.
#[cfg(target_os = "macos")]
pub fn started(pid: Pid) -> Option<StartTime> {
    use crate::sys::{BSDINFO_SIZE, BSDINFO_START_SEC, BSDINFO_START_USEC, PROC_PIDTBSDINFO};
    let mut buf = [0u8; BSDINFO_SIZE];
    let size = core::ffi::c_int::try_from(buf.len()).ok()?;
    // SAFETY: a buffer of the size passed beside it.
    let n = unsafe {
        crate::sys::proc_pidinfo(
            pid,
            PROC_PIDTBSDINFO,
            crate::sys::ProcArg(0),
            buf.as_mut_ptr().cast(),
            size,
        )
    };
    if n != size {
        return None;
    }
    let word = |at: usize| buf.get(at..at + 8)?.try_into().ok().map(u64::from_ne_bytes);
    let (sec, usec) = (word(BSDINFO_START_SEC)?, word(BSDINFO_START_USEC)?);
    Some(StartTime(sec.checked_mul(1_000_000)?.checked_add(usec)?))
}

/// **When `pid` started**: field 22 of `/proc/pid/stat`, "the time the
/// process started after system boot", in clock ticks, read past the
/// command's closing parenthesis, which a name with blanks or a `)` of its
/// own cannot move. None for a process gone.
#[cfg(target_os = "linux")]
pub fn started(pid: Pid) -> Option<StartTime> {
    let stat = std::fs::read(format!("/proc/{pid}/stat")).ok()?;
    stat_started(&stat)
}

/// Field 22 of a `/proc/pid/stat` line: the twentieth field after the
/// command's closing parenthesis, which ends field 2.
#[cfg(any(target_os = "linux", test))]
fn stat_started(stat: &[u8]) -> Option<StartTime> {
    let close = stat.iter().rposition(|&b| b == b')')?;
    let field = stat[close + 1..]
        .split(|&b| b == b' ')
        .filter(|f| !f.is_empty())
        .nth(19)?;
    std::str::from_utf8(field).ok()?.parse().ok().map(StartTime)
}

/// The live reads, for [`crate::title::Derive::check`].
pub struct Live {
    pub master: Option<Fd>,
}

impl crate::title::Reads for Live {
    fn group(&self) -> Option<Pid> {
        self.master.and_then(group)
    }

    fn name(&self, pid: Pid) -> Option<String> {
        name(pid)
    }

    fn dir(&self, pid: Pid) -> Option<PathBuf> {
        dir(pid)
    }
}

/// Whether a watched process forked, exec'd or exited since the last look.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moved {
    Still,
    Moved,
}

/// **The process events a job starts and ends with** (note AK 2.7): a
/// kqueue watching the shell and each foreground leader for
/// `NOTE_FORK`, `NOTE_EXEC` and `NOTE_EXIT`, waited on by a thread of its
/// own, as the pty's reader waits on the master, which wakes the main
/// thread once per batch. A job that prints nothing, `sleep 60`, starts
/// and ends with no read, and these are what say it did; nothing is
/// checked on a timer. "NOTE_TRACK, NOTE_TRACKERR, and NOTE_CHILD are no
/// longer supported as of 10.5" (`[[source: macos-sdk-sys-event-h]]`
/// 357), so a fork names no child: after one the shell's children are
/// listed and watched, before the group is read, so a child that takes
/// the terminal and execs after the read is seen by its `NOTE_EXEC`.
///
/// **It reaps the window's own child** ([`Child`]), on the same kqueue and
/// thread: `waitpid` on that pid alone, never `-1`, which would take the
/// opener's children from under their own `try_wait`, and with `WNOHANG`,
/// asked only on an event, so nothing waits in it. Two events, because the
/// first can come too soon: XNU posts the child's `NOTE_EXIT` before it
/// marks the child a zombie (`[[source: xnu-kern-exit]]` 2703-2704, then
/// `p->p_stat = SZOMB` at 2773), so a `waitpid` on it can find nothing yet,
/// and then sends the parent `SIGCHLD` (2779), whose `EVFILT_SIGNAL` event
/// is posted before any check of the signal's action (`[[source:
/// xnu-kern-sig]]` 2125-2127). Registered in that order -- the signal,
/// then the process, then one `waitpid` -- every interleaving reaps: a
/// child already gone when its process watch is added (`ESRCH`) or before
/// either is, by the `waitpid` after them; one that ends after, by the
/// signal. The signal comes for every child of the process, the opener's
/// and the other windows' too, so it only asks after this one, and wakes
/// nothing unless it was reaped.
///
/// **It names what a job runs, for a link's origin** ([`crate::probe::
/// WatchCell`]): a job that starts, writes a link and ends between two of
/// the reader's reads -- `ssh host 'printf <link>'` -- is in front at no
/// read, and only these events see it. At a `NOTE_FORK` the thread lists
/// the forking process's children itself, watches each new one and names
/// it at once, since its exec may already have happened by then; at a
/// `NOTE_EXEC` it names the process. One named not on this machine
/// (`probe::front_of`) raises the cell, which the reader takes at its next
/// read that asks. A child the thread watched itself is marked by its
/// event's `udata`, and its events wake no one: the title and the paths'
/// re-checks see the processes they saw before, those the main thread
/// adds ([`Watch::add`]). A process gone before it is named -- one that
/// forked, ran and ended before the thread listed it -- names nothing and
/// raises nothing: the race that is left.
///
/// **Dropped with its window, it takes its thread and kqueue with it**: a
/// user event registered at the start is raised, the thread returns on
/// it, the drop joins it, and the kqueue closes with the last of the two
/// that hold it. A window closed before its child ended -- the hangup its
/// close sends the shell takes a moment to end it -- leaves the thread to
/// reap it first, waking no one, so a closed window leaves no zombie; a
/// child that outlives its hangup keeps that thread and its kqueue until
/// it ends.
#[cfg(target_os = "macos")]
pub struct Watch {
    kq: std::sync::Arc<std::os::fd::OwnedFd>,
    moved: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    child: Option<Child>,
    commands: Commands,
}

/// **The window's own child, for its watch to reap** (`Watch`): its pid,
/// and where its status goes.
#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct Child {
    pub pid: Pid,
    pub slot: crate::pty::Slot,
}

#[cfg(target_os = "macos")]
impl Child {
    /// Reaped now if it has ended and is not yet; whether this call did.
    fn reap(&self) -> Reaped {
        match crate::pty::reap_into(self.pid, &self.slot) {
            crate::pty::Recorded::Not => Reaped::Not,
            crate::pty::Recorded::Now => Reaped::Now,
        }
    }

    fn done(&self) -> bool {
        self.slot.get().is_some()
    }
}

/// Whether a call reaped the child.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reaped {
    Not,
    Now,
}

/// The ident of the user event a [`Watch`]'s drop raises; the process
/// filter's idents are pids, under another filter.
#[cfg(target_os = "macos")]
const WAKE: core::ffi::c_ulong = 0;

/// One change to `kq`, with no event list and a zero timeout.
#[cfg(target_os = "macos")]
fn change(kq: Fd, change: &crate::sys::Kevent) -> core::ffi::c_int {
    let zero = crate::sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: one change, no event list, a zero timeout.
    unsafe {
        crate::sys::kevent(
            kq,
            core::ptr::from_ref(change),
            1,
            std::ptr::null_mut(),
            0,
            &raw const zero,
        )
    }
}

/// The user event [`WAKE`], registered (`EV_ADD`) or raised
/// (`NOTE_TRIGGER`).
#[cfg(target_os = "macos")]
fn wake_event(flags: core::ffi::c_ushort, fflags: core::ffi::c_uint) -> crate::sys::Kevent {
    crate::sys::Kevent {
        ident: WAKE,
        filter: crate::sys::EVFILT_USER,
        flags,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}

/// Who watches a process: the main thread, whose events are the title's
/// and wake it, the watch's own thread, whose are only named, or a
/// command's reaper, whose exits only reap ([`Watch::reap_command`]).
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum By {
    Main,
    Thread,
    Command,
}

#[cfg(target_os = "macos")]
impl By {
    /// The mark an event carries back in its `udata`: none for the main
    /// thread's, and a pointer never read for the others', one address
    /// each.
    fn udata(self) -> *mut core::ffi::c_void {
        match self {
            By::Main => std::ptr::null_mut(),
            By::Thread => std::ptr::without_provenance_mut(1),
            By::Command => std::ptr::without_provenance_mut(2),
        }
    }

    fn of(e: &crate::sys::Kevent) -> By {
        match e.udata.addr() {
            0 => By::Main,
            2 => By::Command,
            _ => By::Thread,
        }
    }
}

/// **The commands a `[link.*]` rule started, for the watch to reap**
/// ([`Watch::reap_command`]): each until it has ended. A `Mutex`, as the
/// main thread hands them over and the watch's thread reaps them.
#[cfg(target_os = "macos")]
type Commands = std::sync::Arc<std::sync::Mutex<Vec<std::process::Child>>>;

/// **The commands in `commands` that have ended, reaped**, never waiting:
/// each marked for the frame rig under the probe. Whether any is left.
#[cfg(target_os = "macos")]
fn reap_commands(commands: &Commands) -> Left {
    let Ok(mut held) = commands.lock() else {
        return Left::None;
    };
    held.retain_mut(|c| match c.try_wait() {
        Ok(None) => true,
        Ok(Some(_)) | Err(_) => {
            #[cfg(feature = "effect-probe")]
            A_COMMAND_REAPED.store(true, std::sync::atomic::Ordering::Relaxed);
            false
        }
    });
    match held.is_empty() {
        true => Left::None,
        false => Left::Some,
    }
}

/// Whether any window's watch has reaped a command, for the frame rig:
/// the watch's thread marks it, so a static and not the main thread's.
#[cfg(all(target_os = "macos", feature = "effect-probe"))]
static A_COMMAND_REAPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// **Whether a command has been reaped yet**, in this process, for the
/// frame rig.
#[cfg(all(target_os = "macos", feature = "effect-probe"))]
pub(crate) fn a_command_reaped() -> Reaped {
    match A_COMMAND_REAPED.load(std::sync::atomic::Ordering::Relaxed) {
        true => Reaped::Now,
        false => Reaped::Not,
    }
}

/// **What a batch of the watch's events reaped, and whether the thread
/// has still to wait**: the window's child reaped at a batch that holds an
/// event, and the commands at every batch, an empty one too -- a `kevent`
/// the system interrupted (`EINTR`) -- so a closed window's thread with
/// nothing left to reap returns at it, rather than waiting again until an
/// event no one sends. It waits while the child or a command has not
/// ended. Reaping wakes no one.
#[cfg(target_os = "macos")]
fn after_batch(
    got: &[crate::sys::Kevent],
    child: Option<&Child>,
    commands: &Commands,
) -> (Reaped, Waits) {
    let reaped = match (child, got.is_empty()) {
        (Some(c), false) => c.reap(),
        _ => Reaped::Not,
    };
    let left = reap_commands(commands);
    let waits = match child.is_some_and(|c| !c.done()) || left == Left::Some {
        true => Waits::Yes,
        false => Waits::No,
    };
    (reaped, waits)
}

/// Whether the watch's thread has still to wait, for its window's child
/// or a command.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Waits {
    No,
    Yes,
}

/// Whether a command is left to reap.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Left {
    None,
    Some,
}

/// `SIGCHLD`'s signal event, added: posted for every child of the process
/// as it becomes a zombie, after its `NOTE_EXIT` (see [`Watch`]).
#[cfg(target_os = "macos")]
fn sigchld_event() -> crate::sys::Kevent {
    use crate::sys::{EV_ADD, EV_CLEAR, EVFILT_SIGNAL, Kevent, SIGCHLD};
    Kevent {
        ident: SIGCHLD.unsigned_abs().into(),
        filter: EVFILT_SIGNAL,
        flags: EV_ADD | EV_CLEAR,
        fflags: 0,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}

/// A process watch on `pid` for `fflags`.
#[cfg(target_os = "macos")]
fn proc_event(pid: Pid, fflags: core::ffi::c_uint) -> Option<crate::sys::Kevent> {
    proc_event_by(pid, fflags, By::Main)
}

/// A process watch on `pid` for `fflags`, marked with who added it.
#[cfg(target_os = "macos")]
fn proc_event_by(pid: Pid, fflags: core::ffi::c_uint, by: By) -> Option<crate::sys::Kevent> {
    use crate::sys::{EV_ADD, EV_CLEAR, EVFILT_PROC, Kevent};
    Some(Kevent {
        ident: core::ffi::c_ulong::try_from(pid).ok()?,
        filter: EVFILT_PROC,
        flags: EV_ADD | EV_CLEAR,
        fflags,
        data: 0,
        udata: by.udata(),
    })
}

/// **Whether `pid` runs a program not on this machine**, by the names the
/// paths worker reads (`probe::front_of`); a process gone, which names
/// nothing, is not counted.
#[cfg(target_os = "macos")]
fn named_elsewhere(pid: Pid) -> bool {
    let names: Vec<String> = [name(pid), exe_name(pid)].into_iter().flatten().collect();
    !names.is_empty() && crate::probe::front_of(&names) == crate::paths::Front::Elsewhere
}

/// The children of `parent`, at most 64: `proc_listchildpids` answers a
/// count of pids (`[[source: xnu-libproc-c]]` 87-97).
#[cfg(target_os = "macos")]
fn children(parent: Pid) -> impl Iterator<Item = Pid> {
    let mut kids = [0 as Pid; 64];
    let n = match core::ffi::c_int::try_from(core::mem::size_of_val(&kids)) {
        // SAFETY: a buffer of the size passed beside it.
        Ok(size) => unsafe {
            crate::sys::proc_listchildpids(parent, kids.as_mut_ptr().cast(), size)
        },
        Err(_) => 0,
    };
    kids.into_iter().take(usize::try_from(n).unwrap_or(0))
}

/// **What the thread makes of one batch of events**: each new child of a
/// process that forked watched, by the thread, and each process that
/// exec'd or was just found named; `cell` raised when one is not on this
/// machine. `known` holds the children the thread watches, until they
/// exit, so a busy parent's children are named once each.
#[cfg(target_os = "macos")]
fn name_moves(
    kq: Fd,
    got: &[crate::sys::Kevent],
    known: &mut std::collections::HashSet<Pid>,
    cell: &crate::probe::WatchCell,
) {
    use crate::sys::{EVFILT_PROC, NOTE_EXEC, NOTE_EXIT, NOTE_FORK};
    for e in got.iter().filter(|e| e.filter == EVFILT_PROC) {
        let Ok(pid) = Pid::try_from(e.ident) else {
            continue;
        };
        if e.fflags & NOTE_EXEC != 0 && named_elsewhere(pid) {
            cell.raise();
        }
        if e.fflags & NOTE_FORK != 0 {
            for kid in children(pid) {
                if !known.insert(kid) {
                    continue;
                }
                if let Some(ev) = proc_event_by(kid, NOTE_FORK | NOTE_EXEC | NOTE_EXIT, By::Thread)
                {
                    let _ = change(kq, &ev);
                }
                if named_elsewhere(kid) {
                    cell.raise();
                }
            }
        }
        if e.fflags & NOTE_EXIT != 0 {
            known.remove(&pid);
        }
    }
}

#[cfg(target_os = "macos")]
impl Watch {
    /// A kqueue and the thread that waits on it; `wake` runs after each
    /// batch of events that moved a process the main thread watches or
    /// reaped `child`, and `cell` is raised when a job is named not local.
    pub fn start(
        child: Option<Child>,
        cell: crate::probe::WatchCell,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Watch> {
        use crate::sys::{EV_ADD, EV_CLEAR, EVFILT_PROC, Kevent, NOTE_EXEC, NOTE_EXIT, NOTE_FORK};
        use std::sync::atomic::{AtomicBool, Ordering};
        // SAFETY: no arguments; a descriptor or -1.
        let raw = unsafe { crate::sys::kqueue() };
        if raw < 0 {
            return None;
        }
        // SAFETY: the descriptor `kqueue` just gave, owned here and closed
        // once, with the last holder of it.
        let owned = unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(raw) };
        let kq = std::sync::Arc::new(owned);
        let fd = std::os::fd::AsRawFd::as_raw_fd(&*kq);
        if change(fd, &wake_event(EV_ADD | EV_CLEAR, 0)) < 0 {
            return None;
        }
        // The child's two events, the signal first, then one look: see the
        // type's doc for why every order of the child's end reaps.
        if let Some(c) = &child {
            if change(fd, &sigchld_event()) < 0 {
                return None;
            }
            // `ESRCH` for a child already gone, which the look reaps. Its
            // forks and execs from the start too: the first job is named
            // before any title check has watched the shell.
            if let Some(exit) = proc_event(c.pid, NOTE_FORK | NOTE_EXEC | NOTE_EXIT) {
                let _ = change(fd, &exit);
            }
            let _ = c.reap();
        }
        let moved = std::sync::Arc::new(AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&moved);
        let held = std::sync::Arc::clone(&kq);
        let reaps = child.clone();
        let commands = Commands::default();
        let reaping = std::sync::Arc::clone(&commands);
        let spawned = std::thread::Builder::new()
            .name("baud-fg-watch".into())
            .spawn(move || {
                let kq = std::os::fd::AsRawFd::as_raw_fd(&*held);
                let mut wake = Some(wake);
                let mut known = std::collections::HashSet::new();
                let mut events = [Kevent {
                    ident: 0,
                    filter: 0,
                    flags: 0,
                    fflags: 0,
                    data: 0,
                    udata: std::ptr::null_mut(),
                }; 8];
                loop {
                    // SAFETY: no changes; an event list of its length; no
                    // timeout, so the call waits for an event.
                    let n = unsafe {
                        crate::sys::kevent(
                            kq,
                            std::ptr::null(),
                            0,
                            events.as_mut_ptr(),
                            8,
                            std::ptr::null(),
                        )
                    };
                    if n < 0
                        && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        return;
                    }
                    let got = &events[..usize::try_from(n).unwrap_or(0)];
                    let (reaped, waits) = after_batch(got, reaps.as_ref(), &reaping);
                    let waits = waits == Waits::Yes;
                    if got.iter().any(|e| e.filter == crate::sys::EVFILT_USER) {
                        // The window has gone: its child and its commands,
                        // those not ended, are still reaped here, and no
                        // one is woken.
                        drop(wake.take());
                        match waits {
                            true => continue,
                            false => return,
                        }
                    }
                    if wake.is_none() {
                        // Closed, and waiting only for the child and the
                        // commands.
                        if !waits {
                            return;
                        }
                        continue;
                    }
                    name_moves(kq, got, &mut known, &cell);
                    let moves = got
                        .iter()
                        .any(|e| e.filter == EVFILT_PROC && By::of(e) == By::Main);
                    if moves {
                        flag.store(true, Ordering::Release);
                    }
                    if moves || reaped == Reaped::Now {
                        if let Some(w) = &wake {
                            w();
                        }
                    }
                }
            });
        let thread = spawned.ok()?;
        Some(Watch {
            kq,
            moved,
            thread: Some(thread),
            child,
            commands,
        })
    }

    fn fd(&self) -> Fd {
        std::os::fd::AsRawFd::as_raw_fd(&*self.kq)
    }

    /// Watch `pid` for a fork, an exec or an exit. A process gone, or one
    /// the user may not watch, is not watched, and says nothing.
    pub fn add(&self, pid: Pid) {
        use crate::sys::{NOTE_EXEC, NOTE_EXIT, NOTE_FORK};
        if let Some(e) = proc_event(pid, NOTE_FORK | NOTE_EXEC | NOTE_EXIT) {
            let _ = change(self.fd(), &e);
        }
    }

    /// Watch every child of `parent`: the job a fork made, before it
    /// takes the terminal and execs. `proc_listchildpids` answers a count
    /// of pids (`[[source: xnu-libproc-c]]` 87-97).
    pub fn add_children(&self, parent: Pid) {
        for kid in children(parent) {
            self.add(kid);
        }
    }

    /// **A `[link.*]` command's child, reaped once it ends**, on the
    /// watch's thread, by its exit (`NOTE_EXIT`) or the `SIGCHLD` after it,
    /// so nothing polls for it. Held first, then watched, then looked at
    /// once: a child that ended before its watch was added (`ESRCH`) is
    /// reaped by that look, one that ends after by its events, and the
    /// lock keeps the two from reaping it twice.
    pub fn reap_command(&self, child: std::process::Child) {
        use crate::sys::NOTE_EXIT;
        let pid = Pid::try_from(child.id()).ok();
        if let Ok(mut held) = self.commands.lock() {
            held.push(child);
        }
        let _ = change(self.fd(), &sigchld_event());
        if let Some(e) = pid.and_then(|p| proc_event_by(p, NOTE_EXIT, By::Command)) {
            let _ = change(self.fd(), &e);
        }
        let _ = reap_commands(&self.commands);
    }

    /// Whether a command is held, not yet reaped.
    #[cfg(test)]
    fn commands_left(&self) -> Left {
        match self.commands.lock().map_or(true, |h| h.is_empty()) {
            true => Left::None,
            false => Left::Some,
        }
    }

    /// Whether anything watched moved since the last call.
    pub fn take(&self) -> Moved {
        match self.moved.swap(false, std::sync::atomic::Ordering::AcqRel) {
            true => Moved::Moved,
            false => Moved::Still,
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for Watch {
    /// The thread woken by the user event, and joined unless it still has
    /// the window's child to reap; the kqueue closes with the last of the
    /// two that hold it. A closed window leaves no thread and no
    /// descriptor once its child has been reaped.
    fn drop(&mut self) {
        let raised = change(self.fd(), &wake_event(0, crate::sys::NOTE_TRIGGER));
        let Some(t) = self.thread.take() else {
            return;
        };
        // A thread the event could not reach is left waiting rather than
        // joined forever, its kqueue kept open for it; so is one still to
        // reap the child, which returns once it has.
        let commands = self.commands.lock().map_or(0, |h| h.len());
        if raised < 0 || self.child.as_ref().is_some_and(|c| !c.done()) || commands > 0 {
            return;
        }
        let _ = t.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A process watch dropped ends its thread and closes its kqueue**:
    /// the drop returns only once the thread has, so the waker the thread
    /// held is gone, and the last holder of the kqueue's descriptor with
    /// it, so the descriptor is closed. The control: before the drop the
    /// thread holds the waker and the kqueue has two holders, the watch
    /// and its thread.
    ///
    /// The descriptor is followed by its holders, not by its number: a
    /// closed descriptor's number is the next `open`'s anywhere in the
    /// process, and under `cargo test` another test's thread takes it at
    /// once. Read by number, an open here right after the drop -- the
    /// stand-in for that thread, kept below -- made "closed" read open.
    /// Named break: a drop that keeps a holder of the kqueue (a clone of
    /// its `Arc` forgotten) leaves it open.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_dropped_process_watch_ends_its_thread_and_closes_its_kqueue() {
        let held = std::sync::Arc::new(());
        let in_thread = std::sync::Arc::clone(&held);
        let watch = Watch::start(None, crate::probe::WatchCell::default(), move || {
            let _ = &in_thread;
        })
        .expect("a kqueue and its thread");
        let kq = std::sync::Arc::downgrade(&watch.kq);
        assert_eq!(
            std::sync::Arc::strong_count(&held),
            2,
            "the control: the thread holds the waker"
        );
        assert_eq!(
            kq.strong_count(),
            2,
            "the control: the watch and its thread hold the kqueue"
        );
        drop(watch);
        // Another test's open, which takes the closed descriptor's number.
        let other = std::fs::File::open("/dev/null").expect("an open");
        assert_eq!(
            std::sync::Arc::strong_count(&held),
            1,
            "the thread returned and dropped the waker before the drop did"
        );
        assert!(kq.upgrade().is_none(), "the kqueue is closed");
        drop(other);
    }

    /// **A closed watch with nothing to reap stops at an empty batch**
    /// (`after_batch`): a `kevent` the system interrupted returns no event,
    /// and with no child and no command held the thread need not wait
    /// again. The control: a command still running keeps it waiting, at an
    /// empty batch too. Named break: an empty batch taken as a command
    /// left -- the first assertion fails.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_empty_batch_with_nothing_held_ends_the_wait() {
        use std::process::{Command, Stdio};
        let commands = Commands::default();
        assert_eq!(
            after_batch(&[], None, &commands).1,
            Waits::No,
            "nothing held"
        );
        let mut run = Command::new("/bin/sh");
        run.args(["-c", "read x"]).stdin(Stdio::piped());
        let mut running = run.spawn().expect("a child");
        let stdin = running.stdin.take().expect("its input");
        commands.lock().expect("the commands' lock").push(running);
        assert_eq!(
            after_batch(&[], None, &commands).1,
            Waits::Yes,
            "the control: a command running"
        );
        drop(stdin);
        let start = std::time::Instant::now();
        while after_batch(&[], None, &commands).1 == Waits::Yes
            && start.elapsed() < std::time::Duration::from_secs(2)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            after_batch(&[], None, &commands).1,
            Waits::No,
            "reaped, done"
        );
    }

    /// Whether `w` holds a command, once its thread has had up to two
    /// seconds to reap them all: the thread reaps them, so the test looks
    /// until it has or the bound passes.
    #[cfg(target_os = "macos")]
    fn left_after(w: &Watch) -> Left {
        let start = std::time::Instant::now();
        while w.commands_left() == Left::Some && start.elapsed() < std::time::Duration::from_secs(2)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        w.commands_left()
    }

    /// **A command handed to the watch is reaped on its thread at its
    /// exit, and a running one is held**: one that has ended before it is
    /// handed over is reaped by the look after its watch is added; one
    /// that runs is held while it runs and reaped once it is ended, with
    /// nothing asked of the watch between; and the reap is the kernel's, no
    /// zombie left. Named breaks: the exit not watched -- the running one,
    /// ended, is held; no look after the watch -- the ended one is held.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_command_is_reaped_at_its_exit_and_held_while_it_runs() {
        use std::process::{Command, Stdio};
        let w = Watch::start(None, crate::probe::WatchCell::default(), || {}).expect("a kqueue");
        let mut done = Command::new("/bin/sh");
        done.args(["-c", "exit 0"]);
        let ended = done.spawn().expect("a child");
        // Ended before it is handed over: `waitid` with `WNOWAIT` waits for
        // the exit and leaves the zombie.
        let pid = crate::sys::Pid::try_from(ended.id()).expect("a pid");
        let mut info = [0u8; 256];
        let id = core::ffi::c_uint::try_from(pid).expect("a pid");
        // SAFETY: a buffer larger than either system's `siginfo_t`.
        let r = unsafe {
            crate::sys::waitid(
                crate::sys::P_PID,
                id,
                info.as_mut_ptr().cast(),
                crate::sys::WEXITED | crate::sys::WNOWAIT,
            )
        };
        assert_eq!(r, 0, "waitid: {}", std::io::Error::last_os_error());
        w.reap_command(ended);
        assert_eq!(left_after(&w), Left::None, "the ended one, by the look");
        // SAFETY: signal 0 names no signal; it asks whether `pid` exists.
        let gone = unsafe { crate::sys::kill(pid, 0) } < 0
            && std::io::Error::last_os_error().raw_os_error() == Some(crate::sys::ESRCH);
        assert!(gone, "reaped, not left a zombie");
        let mut run = Command::new("/bin/sh");
        run.args(["-c", "read x"]).stdin(Stdio::piped());
        let mut running = run.spawn().expect("a child");
        let stdin = running.stdin.take().expect("its input");
        w.reap_command(running);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(
            w.commands_left(),
            Left::Some,
            "the control: held while it runs"
        );
        // Its input closed, `read` ends and so does the shell.
        drop(stdin);
        assert_eq!(left_after(&w), Left::None, "reaped at its exit");
    }

    /// A fresh directory holding `ssh` and `true`, each a link to
    /// `/bin/sleep`: argv[0] names the first not local and the second
    /// local, and either lives as long as it is told.
    #[cfg(target_os = "macos")]
    fn programs(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("baud-fg-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch directory");
        for n in ["ssh", "true"] {
            std::os::unix::fs::symlink("/bin/sleep", d.join(n)).expect("a link");
        }
        d
    }

    /// One `EVFILT_PROC` event, as the kernel returns it.
    #[cfg(target_os = "macos")]
    fn event(pid: Pid, fflags: core::ffi::c_uint) -> crate::sys::Kevent {
        crate::sys::Kevent {
            ident: core::ffi::c_ulong::try_from(pid).expect("a pid"),
            filter: crate::sys::EVFILT_PROC,
            flags: 0,
            fflags,
            data: 0,
            udata: std::ptr::null_mut(),
        }
    }

    /// **The watch's thread names a forking process's new children and a
    /// process that exec'd, and raises the cell only for one not on this
    /// machine**: this test's own child run as `true`, found at a fork or
    /// named at its exec, raises nothing; one run as `ssh` raises it, either
    /// way; and a child already listed is not named again. Named breaks:
    /// list no children at a fork -- the fork's case raises nothing; name
    /// nothing at an exec -- the exec's case raises nothing. The fork's case
    /// watches a parent of the test's own whose one child is the program, so
    /// it holds in a process shared with other tests, as `cargo test` runs
    /// them; three children of this process held beside it make that so on
    /// every run. Break: watch this process's own fork, as it did -- they
    /// are listed too.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_fork_or_exec_raises_the_cell_only_for_a_remote_program() {
        use crate::sys::{NOTE_EXEC, NOTE_FORK};
        let dir = programs("names");
        // SAFETY: no arguments; a descriptor or -1.
        let raw = unsafe { crate::sys::kqueue() };
        assert!(raw >= 0, "a kqueue");
        // SAFETY: the descriptor `kqueue` just gave, closed once, here.
        let kq = unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(raw) };
        let fd = std::os::fd::AsRawFd::as_raw_fd(&kq);
        // Other tests of this process run beside this one under `cargo
        // test`, and their children are this process's too: three held
        // here, so the interference is there on every run.
        let mut others: Vec<_> = (0..3)
            .map(|_| {
                std::process::Command::new("/bin/sleep")
                    .arg("30")
                    .spawn()
                    .expect("a sibling's child")
            })
            .collect();
        // The fork's case watches a parent of the test's own, `sh`, whose one
        // child is the program: its children are the program alone, whatever
        // else this process has started. Its child is waited for to exist
        // and to have exec'd, named the program, before the event is read.
        let run = |name: &str, fflags| {
            let program = dir.join(name);
            let (mut parent, pid) = match fflags {
                NOTE_FORK => {
                    let parent = std::process::Command::new("/bin/sh")
                        .args(["-c", "\"$0\" 5 & wait"])
                        .arg(&program)
                        .spawn()
                        .expect("spawned");
                    let sh = Pid::try_from(parent.id()).expect("a pid");
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
                    loop {
                        let kids: Vec<Pid> = children(sh).collect();
                        if let [kid] = kids[..] {
                            if super::name(kid).as_deref() == Some(name) {
                                break;
                            }
                        }
                        assert!(std::time::Instant::now() < deadline, "{name} never started");
                        std::thread::yield_now();
                    }
                    (parent, sh)
                }
                _ => {
                    let child = std::process::Command::new(&program)
                        .arg("5")
                        .spawn()
                        .expect("spawned");
                    let pid = Pid::try_from(child.id()).expect("a pid");
                    (child, pid)
                }
            };
            let cell = crate::probe::WatchCell::default();
            let mut known = std::collections::HashSet::new();
            name_moves(fd, &[event(pid, fflags)], &mut known, &cell);
            let got = cell.take();
            let listed: Vec<Pid> = known.iter().copied().collect();
            name_moves(fd, &[event(pid, fflags)], &mut known, &cell);
            let again = cell.take();
            for kid in &listed {
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-9", &kid.to_string()])
                    .status();
            }
            let _ = parent.kill();
            let _ = parent.wait();
            (got, listed.len(), again)
        };
        use crate::probe::Watched::{Elsewhere, Nothing};
        assert_eq!(
            run("true", NOTE_FORK),
            (Nothing, 1, Nothing),
            "the control, forked"
        );
        assert_eq!(
            run("true", NOTE_EXEC),
            (Nothing, 0, Nothing),
            "the control, exec'd"
        );
        assert_eq!(
            run("ssh", NOTE_FORK),
            (Elsewhere, 1, Nothing),
            "found at the fork, and named once"
        );
        assert_eq!(
            run("ssh", NOTE_EXEC),
            (Elsewhere, 0, Elsewhere),
            "named at its exec"
        );
        for o in &mut others {
            let _ = o.kill();
            let _ = o.wait();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A short ssh started by a watched shell raises the cell, through
    /// the kernel's own events**, the main thread having watched only the
    /// shell, as its title check does: one exec'd at once and one exec'd
    /// from a subshell after a pause. The watch's thread lists the shell's
    /// children at its fork and finds each as `sh`, named at the exec it
    /// then watches, or already `ssh`, named as it is found: the ending is
    /// the same either way the race falls. Named breaks: leave the children
    /// to the main thread (no listing in `name_moves`), or name nothing at
    /// an exec -- neither raises.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_short_ssh_under_a_watched_shell_raises_the_cell() {
        use std::io::Write;
        let dir = programs("events");
        let ssh = dir.join("ssh");
        let ssh = ssh.to_str().expect("a UTF-8 path");
        for script in [
            format!("read x; {ssh} 0.2"),
            format!("read x; ( /bin/sleep 0.3; exec {ssh} 0.2 ); :"),
        ] {
            let cell = crate::probe::WatchCell::default();
            let w = Watch::start(None, cell.clone(), || {}).expect("a kqueue");
            let mut sh = std::process::Command::new("/bin/sh")
                .args(["-c", &script])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .expect("a shell");
            w.add(Pid::try_from(sh.id()).expect("a pid"));
            assert_eq!(cell.take(), crate::probe::Watched::Nothing, "the control");
            sh.stdin
                .take()
                .expect("its input")
                .write_all(b"\n")
                .expect("written");
            let mut got = crate::probe::Watched::Nothing;
            let bound = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while got == crate::probe::Watched::Nothing && std::time::Instant::now() < bound {
                got = cell.take();
                if got == crate::probe::Watched::Nothing {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
            }
            let _ = sh.wait();
            assert_eq!(got, crate::probe::Watched::Elsewhere, "{script}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A process's start time is read as the kernel keeps it**: this
    /// process's, the same at two reads and none for a pid no process has;
    /// and Linux's field 22 is the twentieth after the command's closing
    /// parenthesis, a command holding blanks and a `)` of its own included.
    /// Named break: count the fields from the line's start -- the command
    /// with a blank moves them.
    #[test]
    fn a_processs_start_time_is_the_kernels() {
        let me = Pid::try_from(std::process::id()).expect("a pid");
        let first = started(me);
        assert!(first.is_some(), "this process's");
        assert_eq!(started(me), first, "the same at a second read");
        assert_eq!(started(Pid::MAX), None, "no such process");
        let fields: Vec<String> = (3..=25).map(|n| n.to_string()).collect();
        for comm in ["sleep", "a b) c"] {
            let line = format!("4242 ({comm}) {}\n", fields.join(" "));
            assert_eq!(stat_started(line.as_bytes()), Some(StartTime(22)), "{line}");
        }
        assert_eq!(stat_started(b"4242 (x) S 1"), None, "short");
    }

    #[test]
    fn a_program_is_argv0s_last_part_without_a_login_dash() {
        assert_eq!(program(b"-zsh").as_deref(), Some("zsh"));
        assert_eq!(program(b"/bin/sleep").as_deref(), Some("sleep"));
        assert_eq!(program(b"vim").as_deref(), Some("vim"));
        assert_eq!(program(b""), None);
        assert_eq!(program(b"/usr/bin/"), None);
    }

    #[test]
    fn procargs2_gives_argv0_past_the_path_and_its_padding() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/nix/store/x/bin/coreutils\0\0\0\0sleep\x0060\0PATH=/bin\0");
        assert_eq!(procargs2_argv0(&buf), Some(&b"sleep"[..]));
        let mut none = 0i32.to_ne_bytes().to_vec();
        none.extend_from_slice(b"/bin/x\0\0y\0");
        assert_eq!(procargs2_argv0(&none), None, "no argc, no argv");
        assert_eq!(procargs2_argv0(&[1, 0]), None, "short");
    }
}
