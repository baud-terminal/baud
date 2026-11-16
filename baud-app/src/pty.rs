//! The pty: a login shell on the slave, the master read on a thread on
//! macOS and polled beside the compositor's socket on Linux.
//!
//! The one place this binary calls libc: `posix_openpt`, `grantpt`,
//! `unlockpt` and `fork` in place of `forkpty`, and `setsid`, `dup2` and
//! the `TIOCSCTTY` ioctl in the child as `login_tty` makes them,
//! `tcgetattr` and `tcsetattr` in the child and on the subsidiary
//! `ptsname` names,
//! `execve`, `read`, `write`, `poll`, `fcntl`, the `TIOCSWINSZ` ioctl,
//! `mmap` for the font files, CoreGraphics for the display's scale before
//! AppKit exists, and
//! CoreFoundation and `newlocale` for a shell whose environment names no
//! locale, and on Linux `newlocale` for the Compose search. On macOS the
//! reader thread
//! owns the master's read side for the life of the process and pushes
//! every `read(2)` -- 4096 bytes at a time, which is what `N_TTY_BUF_SIZE`
//! and the Darwin clist hand back anyway -- to the main thread over a
//! channel; on Linux the main loop waits on the master itself and takes
//! each read where it is polled.
//!
//! **The master is non-blocking**, because the main thread writes it. A
//! write the slave's input queue cannot hold -- Darwin's takes 1022 bytes
//! and no more until the child reads -- waited in `write(2)` until the child
//! read, which held the window's thread for as long as the child did not:
//! a stopped job, or one busy elsewhere. Now the kernel takes what fits, an
//! [`Outbox`] keeps the rest in order, and the next pump offers it again.
//! The read side waits in `poll` instead of in `read`, on whichever thread
//! reads.
#![deny(unsafe_op_in_unsafe_fn)]

use crate::child_env::{ChildEnv, EnvName, LANG, LC_ALL, LC_CTYPE, PWD, SHELL};
#[cfg(any(target_os = "macos", target_os = "linux"))]
use baud_domain::HostTime;
use baud_domain::{InputEncoding, WinSize};
use baud_font::Px;
use std::ffi::{CString, OsStr, OsString};
#[cfg(target_os = "linux")]
use std::os::fd::BorrowedFd;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "macos")]
use std::sync::Arc;
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "macos")]
use std::sync::mpsc::{Receiver, Sender, channel};
#[cfg(target_os = "linux")]
use std::time::Duration;

/// **A live terminal**: the master's file descriptor, owned here, and its
/// child.
pub struct Live {
    master: crate::sys::Fd,
    /// The process the pty was spawned with, the session's root: the
    /// shell, or what `BAUD_CMD` names. `None` for a test's pty over a
    /// child it forked itself.
    child: Option<crate::sys::Pid>,
    /// The size the kernel was last told, so a `resize` that changes nothing
    /// sends no `SIGWINCH`: the app calls it on every re-lay, including the
    /// ones where only the pixels could have moved.
    told: crate::sys::Winsize,
    /// `IUTF8`, kept with the input encoding after the spawn.
    line: LineDiscipline,
    #[cfg(target_os = "macos")]
    rx: Receiver<Read>,
    /// Set by the reader when it has woken the main thread and not yet
    /// been drained, so a flood of reads sends one wake, not one a read.
    #[cfg(target_os = "macos")]
    signalled: Arc<AtomicBool>,
    /// What the window's process watch saw between two reads, which the
    /// reader takes (`probe::WatchCell`).
    #[cfg(target_os = "macos")]
    watched: crate::probe::WatchCell,
    /// The write end of the pipe the reader waits on beside the master:
    /// dropped with the window's `Pty`, it ends the reader, which closes
    /// its copy of the master (`Reading`). `None` when the reader shares this
    /// `Pty`'s descriptor, which then stays open for the life of the
    /// process.
    #[cfg(target_os = "macos")]
    closing: Option<std::io::PipeWriter>,
    /// **How the window's child ends** (`held.rs`): the program a link's
    /// window runs, its start, and where its end goes once it is known.
    ending: ChildEnd,
    /// **The child as a descriptor** (`pidfd_open`), readable once it has
    /// ended, in the loop's poll until it is reaped; none once it is, or
    /// where the kernel has no such call ([`Pty::reap_owed`]).
    #[cfg(target_os = "linux")]
    pidfd: Option<std::os::fd::OwnedFd>,
    /// Whether the master has hung up: then it leaves the poll, which would
    /// answer its hangup at once on every wait after.
    #[cfg(target_os = "linux")]
    hung: crate::held::Stream,
    /// With no pidfd, when the hung-up master's child is looked for once
    /// more ([`Pty::reap_owed`]).
    #[cfg(target_os = "linux")]
    again: Option<std::time::Instant>,
    /// The token baud handed the shell, for the loop's scan of its marks
    /// (`probe::Sampler::for_shell`).
    #[cfg(target_os = "linux")]
    token: Option<crate::ran::ShellToken>,
}

/// **How a window's child ends, for its window** (`held::decide`): the
/// program a link's window ran, none for a shell window, which never
/// holds; when it was started; and how it ended, which its watch learns
/// (`fg::Watch` on macOS, the pidfd on Linux).
struct ChildEnd {
    program: Option<OsString>,
    born: std::time::Instant,
    slot: Slot,
}

impl ChildEnd {
    fn of(program: Option<OsString>) -> ChildEnd {
        ChildEnd {
            program,
            born: std::time::Instant::now(),
            slot: Slot::default(),
        }
    }
}

/// **Where a window's child's end goes** (`held::Reaped`), set once and
/// read by the window's turns and its watch. Anyone may read it; only this
/// file writes it, and only with what a `waitpid` of its own answered
/// ([`reap_into`]) or, on Linux, a window that stopped waiting for a child
/// still running ([`Pty::give_up`]). Its cell is private to this file, and
/// `a_slot_is_written_only_where_a_wait_answered` holds that.
#[derive(Clone, Default)]
pub struct Slot(std::sync::Arc<std::sync::OnceLock<crate::held::Reaped>>);

impl Slot {
    /// The child's end, once it is known.
    pub fn get(&self) -> Option<&crate::held::Reaped> {
        self.0.get()
    }

    /// The child's end, waiting until it is known.
    #[cfg(all(test, target_os = "macos"))]
    pub fn wait(&self) -> &crate::held::Reaped {
        self.0.wait()
    }

    /// A window that stopped waiting for its child, which still runs.
    #[cfg(target_os = "linux")]
    fn abandon(&self) {
        let at = std::time::Instant::now();
        let ending = crate::held::Ending::Abandoned;
        let _ = self.0.set(crate::held::Reaped { ending, at });
    }
}

/// **A window's close hangs up its shell**, as the process's exit always
/// has: the master's last close is a lost carrier, `ptcclose` calls the
/// line discipline's modem routine with the carrier off (XNU's
/// `tty_dev.c` 581, `[[source: xnu-tty-dev]]`), and `ttymodem` sends
/// `SIGHUP` to the session's leader, the shell (`tty.c` 2041-2042,
/// `[[source: xnu-tty]]`). On macOS the reader's copy is the other
/// reference, and the pipe this `Pty` holds ends it after this.
impl Drop for Live {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.closing.is_none() {
            return;
        }
        // SAFETY: the descriptor is this `Pty`'s, open since the fork, and
        // closed only here: the reader holds a copy of its own.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(self.master) });
    }
}

/// **A window's terminal**: a pty with its child, or the refusal of the
/// one the window asked for. A refused window opens all the same, over
/// [`NoTerminal`]'s words ([`say_refused`]), and stays until it is closed:
/// it has no child, so nothing is signalled, watched, reaped or polled.
pub enum Pty {
    Live(Live),
    Refused(Refused),
}

/// **A terminal the system would not open**, and the size the window
/// asked for, kept for a resize to change.
pub struct Refused {
    why: NoTerminal,
    told: crate::sys::Winsize,
}

/// A count of ptys: in use on the machine, or the most it allows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PtyCount(pub usize);

/// **Why a window has no terminal.** The system's limit on ptys, with the
/// count in use and the most allowed as they were read when the open
/// failed -- each left out when it could not be read, never guessed -- or
/// another failure, with its `errno`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NoTerminal {
    Limit {
        open: Option<PtyCount>,
        most: Option<PtyCount>,
    },
    Failed(crate::sys::Errno),
}

/// The `errno` a manager's open answers at the system's limit on ptys:
/// `ENXIO` on macOS, from `ptcopen` once `ptmx_get_ioctl` refuses
/// (`[[source: xnu-tty-dev]]` 493-494, `[[source: xnu-tty-ptmx-c]]`
/// 287-290), which it also does when the kernel could not allocate the
/// pty's state (299, 335). `EAGAIN` is not it: macOS answers that for a
/// subsidiary not yet unlocked (`[[source: xnu-tty-dev]]` 273).
#[cfg(target_os = "macos")]
const AT_THE_LIMIT: crate::sys::Errno = crate::sys::Errno(crate::sys::ENXIO);

/// `ENOSPC` on Linux, from `devpts_new_index` once `kernel/pty/nr` reaches
/// `kernel/pty/max` less the reserve (`[[source: linux-devpts-inode-c]]`
/// 466-472), which `ptmx_open` returns (`[[source: linux-pty]]` 792-795).
#[cfg(target_os = "linux")]
const AT_THE_LIMIT: crate::sys::Errno = crate::sys::Errno(crate::sys::ENOSPC);

impl NoTerminal {
    /// What a manager's open that failed with `e` means: the limit, with
    /// the counts `counts` reads, or `e`'s own failure.
    fn of(
        e: crate::sys::Errno,
        counts: impl FnOnce() -> (Option<PtyCount>, Option<PtyCount>),
    ) -> NoTerminal {
        if e == AT_THE_LIMIT {
            let (open, most) = counts();
            NoTerminal::Limit { open, most }
        } else {
            NoTerminal::Failed(e)
        }
    }
}

/// **The sysctl that holds the most ptys macOS allows**: read here and
/// named in the refused window's advice, which a test holds to it.
#[cfg(target_os = "macos")]
const PTMX_MAX: &core::ffi::CStr = c"kern.tty.ptmx_max";

/// **The ptys in use and the most the system allows**, read when an open
/// failed. On macOS, the `ttys` entries in `/dev`: each is made with its
/// pty and removed when the pty is freed (`[[source: xnu-tty-ptmx-c]]`
/// 379-382, 443); and `kern.tty.ptmx_max` (165-190).
#[cfg(target_os = "macos")]
fn pty_counts() -> (Option<PtyCount>, Option<PtyCount>) {
    #[cfg(test)]
    if let Some(counts) = COUNTED.with(core::cell::Cell::get) {
        return counts;
    }
    let open = std::fs::read_dir("/dev").ok().map(|d| {
        PtyCount(
            d.filter_map(Result::ok)
                .filter(|e| e.file_name().as_bytes().starts_with(b"ttys"))
                .count(),
        )
    });
    let mut most: core::ffi::c_int = 0;
    let mut size = crate::sys::MapLen(core::mem::size_of::<core::ffi::c_int>());
    // SAFETY: a NUL-terminated name, an int's buffer and its size, and no
    // new value.
    let read = unsafe {
        crate::sys::sysctlbyname(
            PTMX_MAX.as_ptr(),
            (&raw mut most).cast(),
            &raw mut size,
            core::ptr::null_mut(),
            crate::sys::MapLen(0),
        )
    };
    let most = (read == 0)
        .then(|| usize::try_from(most).ok())
        .flatten()
        .map(PtyCount);
    (open, most)
}

/// **Where Linux keeps the pty sysctls**, `kernel.pty.*`: read here and
/// named in the refused window's advice, which a test holds to it.
#[cfg(target_os = "linux")]
const PTY_SYSCTLS: &str = "/proc/sys/kernel/pty";

/// On Linux, `kernel/pty/nr` and `kernel/pty/max`
/// (`[[source: linux-devpts-inode-c]]` 43-70).
#[cfg(target_os = "linux")]
fn pty_counts() -> (Option<PtyCount>, Option<PtyCount>) {
    #[cfg(test)]
    if let Some(counts) = COUNTED.with(core::cell::Cell::get) {
        return counts;
    }
    let read = |name: &str| {
        std::fs::read_to_string(format!("{PTY_SYSCTLS}/{name}"))
            .ok()?
            .trim()
            .parse()
            .ok()
            .map(PtyCount)
    };
    (read("nr"), read("max"))
}

impl core::fmt::Display for NoTerminal {
    /// What the refused window says, each line within 80 columns: the
    /// cause, with the counts read, and what the user can do about it. The
    /// most a Mac allows is 999 (`PTMX_MAX_HARD`, `[[source: xnu-tty-ptmx-c]]`
    /// 166, 178-182), and `/etc/sysctl.conf` "is read in when the system goes
    /// into multi-user mode" (`[[source: macos-man-sysctl-conf-5]]`). Linux
    /// refuses at `kernel/pty/max` less the reserve, or at the maximum where
    /// the mount takes the reserve (`[[source: linux-devpts-inode-c]]`
    /// 470-471), so its line names the setting and the counts rather than
    /// a number it allows.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        const WHY: &str = "baud could not open a terminal:";
        let (open, most) = match *self {
            NoTerminal::Failed(e) => return write!(f, "{WHY} {e}."),
            NoTerminal::Limit { open, most } => (open, most),
        };
        #[cfg(target_os = "macos")]
        match (open, most) {
            (Some(o), Some(m)) => writeln!(
                f,
                "{WHY} macOS allows {} at once, and {} are open.",
                m.0, o.0
            )?,
            (None, Some(m)) => {
                writeln!(f, "{WHY} macOS allows {} at once, and all are in use.", m.0)?
            }
            (Some(o), None) => writeln!(f, "{WHY} macOS's limit is reached, with {} open.", o.0)?,
            (None, None) => writeln!(f, "{WHY} macOS's limit on terminals is reached.")?,
        }
        // Its own line for the counts: `kernel.pty.max` runs to seven
        // digits.
        #[cfg(target_os = "linux")]
        {
            writeln!(f, "{WHY} the system's limit on terminals is reached.")?;
            match (open, most) {
                (Some(o), Some(m)) => {
                    writeln!(f, "kernel.pty.max is {}, and {} are open.", m.0, o.0)?
                }
                (None, Some(m)) => writeln!(f, "kernel.pty.max is {}.", m.0)?,
                (Some(o), None) => writeln!(f, "{} are open.", o.0)?,
                (None, None) => {}
            }
        }
        writeln!(
            f,
            "Close terminal windows or tabs you no longer need, then open a new window."
        )?;
        #[cfg(target_os = "macos")]
        {
            writeln!(
                f,
                "Or raise the limit, up to 999: sudo sysctl kern.tty.ptmx_max=999"
            )?;
            write!(
                f,
                "To keep it after a restart, add kern.tty.ptmx_max=999 to /etc/sysctl.conf."
            )
        }
        #[cfg(target_os = "linux")]
        write!(
            f,
            "Or raise the limit: sudo sysctl kernel.pty.max=<a larger number>"
        )
    }
}

/// **A refused window says why**, written into its terminal as text when
/// the window opens, each line from the left edge; nothing for a live one.
pub fn say_refused(term: &mut baud_core::Term<'_>, pty: &Pty) {
    let Pty::Refused(r) = pty else {
        return;
    };
    let text = format!("{}\r\n", r.why)
        .replace('\n', "\r\n")
        .replace("\r\r\n", "\r\n");
    let bytes = text.as_bytes();
    term.write_at(baud_core::BytePos::ZERO, bytes);
    term.feed(
        baud_core::BytePos::ZERO,
        baud_core::ReadLen::from_index(bytes.len()),
    );
    while term.decode_step(baud_domain::DecodeBudget::ALL) == baud_domain::DecodeProgress::More {}
}

// **A test's refusal of the next opens on its thread**: each open of a
// manager fails with `REFUSING`'s `errno`, counted in `REFUSED_OPENS`, and
// the counts read are `COUNTED`'s, so no test needs the machine's ptys used
// up or its counts as they are.
#[cfg(test)]
thread_local! {
    static REFUSING: core::cell::Cell<Option<crate::sys::Errno>> = const { core::cell::Cell::new(None) };
    static REFUSED_OPENS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static COUNTED: core::cell::Cell<Option<(Option<PtyCount>, Option<PtyCount>)>> = const { core::cell::Cell::new(None) };
}

/// Refuses this thread's opens with `e`, the counts read `counts`, or
/// stops; and starts the count of refused opens again.
#[cfg(test)]
pub(crate) fn set_refusing(
    e: Option<crate::sys::Errno>,
    counts: Option<(Option<PtyCount>, Option<PtyCount>)>,
) {
    REFUSING.with(|r| r.set(e));
    COUNTED.with(|c| c.set(counts));
    REFUSED_OPENS.with(|n| n.set(0));
}

/// A count of opens tried.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Tries(pub(crate) usize);

/// How many opens this thread refused since [`set_refusing`].
#[cfg(test)]
pub(crate) fn refused_opens() -> Tries {
    Tries(REFUSED_OPENS.with(core::cell::Cell::get))
}

/// A manager's open, or this thread's refusal of it ([`REFUSING`]): -1
/// with `errno` set, as a failed `posix_openpt` leaves it.
fn open_manager(flags: core::ffi::c_int) -> crate::sys::Fd {
    #[cfg(test)]
    if let Some(e) = REFUSING.with(core::cell::Cell::get) {
        REFUSED_OPENS.with(|n| n.set(n.get() + 1));
        e.set();
        return -1;
    }
    // SAFETY: the call takes flags and returns a descriptor or -1.
    unsafe { crate::sys::posix_openpt(flags) }
}

impl Pty {
    /// Why the window has no terminal, if it has none.
    #[cfg(test)]
    pub(crate) fn refused(&self) -> Option<NoTerminal> {
        match self {
            Pty::Live(_) => None,
            Pty::Refused(r) => Some(r.why),
        }
    }

    /// The live terminal, for a test that reads its parts.
    #[cfg(test)]
    pub(crate) fn live(&self) -> &Live {
        match self {
            Pty::Live(l) => l,
            Pty::Refused(r) => panic!("a refused terminal: {:?}", r.why),
        }
    }

    /// The live terminal, for a test that changes its parts.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn live_mut(&mut self) -> &mut Live {
        match self {
            Pty::Live(l) => l,
            Pty::Refused(r) => panic!("a refused terminal: {:?}", r.why),
        }
    }

    /// The master, to wait on; a refused terminal has none.
    #[cfg(all(test, target_os = "linux"))]
    pub fn as_fd(&self) -> Option<BorrowedFd<'_>> {
        match self {
            Pty::Live(l) => Some(l.as_fd()),
            Pty::Refused(_) => None,
        }
    }

    /// The terminal's special characters; the defaults for a refused one.
    pub fn specials(&self) -> crate::paste::Specials {
        match self {
            Pty::Live(l) => l.specials(),
            Pty::Refused(_) => crate::paste::Specials::default(),
        }
    }

    /// The literal-next character; the default for a refused terminal.
    pub fn literal_next(&self) -> crate::readline::Quote {
        match self {
            Pty::Live(l) => l.literal_next(),
            Pty::Refused(_) => crate::readline::Quote::LNEXT,
        }
    }

    /// One read of the master; nothing, and never an end, for a refused
    /// terminal.
    #[cfg(target_os = "linux")]
    pub fn read_chunk(&self) -> Chunk {
        match self {
            Pty::Live(l) => l.read_chunk(),
            Pty::Refused(_) => Chunk::Bytes(Vec::new()),
        }
    }

    /// What the reader took; nothing, and the stream open, for a refused
    /// terminal, whose window stays until it is closed.
    #[cfg(target_os = "macos")]
    pub fn drain(&self) -> Drained {
        match self {
            Pty::Live(l) => l.drain(),
            Pty::Refused(_) => Drained {
                chunks: Vec::new(),
                stream: Stream::Open,
            },
        }
    }

    /// The bytes owed, written; a refused terminal has no child to take
    /// them, and they are let go.
    pub fn write(&self, out: &mut Outbox) -> Wrote {
        match self {
            Pty::Live(l) => l.write(out),
            Pty::Refused(_) => {
                out.bytes.clear();
                Wrote::All
            }
        }
    }

    /// The window's new size, told to the kernel; kept for a refused
    /// terminal.
    pub fn resize(&mut self, size: &WinSize, cell_w: Px, cell_h: Px) {
        match self {
            Pty::Live(l) => l.resize(size, cell_w, cell_h),
            Pty::Refused(r) => r.told = text_area(size, cell_w, cell_h),
        }
    }

    /// The session's root process; none for a refused terminal.
    pub fn child(&self) -> Option<crate::sys::Pid> {
        match self {
            Pty::Live(l) => l.child(),
            Pty::Refused(_) => None,
        }
    }

    /// Whether the window closes; never on its own for a refused terminal.
    pub fn decide(&self, stream: crate::held::Stream) -> crate::held::Decided {
        match self {
            Pty::Live(l) => l.decide(stream),
            Pty::Refused(_) => crate::held::Decided::Wait,
        }
    }

    /// The child the window's watch reaps; none for a refused terminal.
    #[cfg(all(test, target_os = "macos"))]
    pub fn reaps(&self) -> Option<crate::fg::Child> {
        match self {
            Pty::Live(l) => l.reaps(),
            Pty::Refused(_) => None,
        }
    }

    /// **The process watch for the window**, or none for a refused
    /// terminal, which has no process to watch.
    #[cfg(target_os = "macos")]
    pub fn watch(&self, wake: impl Fn() + Send + 'static) -> Option<crate::fg::Watch> {
        match self {
            Pty::Live(l) => crate::fg::Watch::start(l.reaps(), l.watched(), wake),
            Pty::Refused(_) => None,
        }
    }

    /// The master to poll; none for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn polled_master(&self) -> Option<BorrowedFd<'_>> {
        match self {
            Pty::Live(l) => l.polled_master(),
            Pty::Refused(_) => None,
        }
    }

    /// The master hung up; a refused terminal never does.
    #[cfg(target_os = "linux")]
    pub fn hung_up(&mut self) {
        if let Pty::Live(l) = self {
            l.hung_up();
        }
    }

    /// Whether the master is open; always for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn stream(&self) -> crate::held::Stream {
        match self {
            Pty::Live(l) => l.stream(),
            Pty::Refused(_) => crate::held::Stream::Open,
        }
    }

    /// The child's descriptor to poll; none for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn child_fd(&self) -> Option<BorrowedFd<'_>> {
        match self {
            Pty::Live(l) => l.child_fd(),
            Pty::Refused(_) => None,
        }
    }

    /// Reaps the child; nothing for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn reap(&mut self) {
        if let Pty::Live(l) = self {
            l.reap();
        }
    }

    /// When the child is to be asked again; never for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn reap_owed(&self) -> Option<std::time::Instant> {
        match self {
            Pty::Live(l) => l.reap_owed(),
            Pty::Refused(_) => None,
        }
    }

    /// Gives the child up; nothing for a refused terminal.
    #[cfg(target_os = "linux")]
    pub fn give_up(&mut self) {
        if let Pty::Live(l) = self {
            l.give_up();
        }
    }

    /// The token baud handed the shell; none for a refused terminal,
    /// which started no shell.
    #[cfg(target_os = "linux")]
    pub fn shell_token(&self) -> Option<&crate::ran::ShellToken> {
        match self {
            Pty::Live(l) => l.shell_token(),
            Pty::Refused(_) => None,
        }
    }

    /// The subsidiary's path; none for a refused terminal.
    pub fn tty(&self) -> Option<&std::path::Path> {
        match self {
            Pty::Live(l) => l.tty(),
            Pty::Refused(_) => None,
        }
    }

    /// What the window reads the foreground through; nothing for a
    /// refused terminal.
    pub fn reads(&self) -> crate::fg::Live {
        match self {
            Pty::Live(l) => l.reads(),
            Pty::Refused(_) => crate::fg::Live { master: None },
        }
    }

    /// The input encoding, followed on the line; nothing for a refused
    /// terminal.
    pub fn follow(&mut self, enc: InputEncoding) {
        if let Pty::Live(l) = self {
            l.follow(enc);
        }
    }
}

/// Whether the child is still on the other end of the pty.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stream {
    Open,
    /// The reader saw end of file: the child exited.
    Closed,
}

/// One `read(2)` of the master, and when it returned.
///
/// The stamp is taken on the reader thread the moment the read returns,
/// before the bytes are copied or the main thread is woken, so it is the
/// earliest moment this process knows the child answered -- the `echo` leg
/// of the keystroke instrument (`latency.rs`), which is why it rides in the
/// channel beside the bytes rather than being read on the main thread when
/// the chunk is drained. Named `Read` rather than `Chunk`, which is the Linux
/// loop's enum for the same read without the stamp.
#[cfg(target_os = "macos")]
pub struct Read {
    pub at: HostTime,
    pub bytes: Vec<u8>,
    /// The foreground process group just after the read and at the read
    /// before it, and what the process watch saw between, when the reader
    /// asked (`probe::Sampler::writers`): which program wrote these bytes,
    /// for the printed paths and a link's origin, read here because the
    /// program may be gone by the time anything else asks.
    pub writers: crate::probe::Writers,
    /// Whether the bytes carried an `OSC 8` link's introducer or a plain
    /// `file://` URI, which always asks for the group (`probe::Sampler::read`).
    pub linked: crate::probe::Linked,
}

/// What a drain found: the chunks that arrived, and whether more can.
#[cfg(target_os = "macos")]
pub struct Drained {
    pub chunks: Vec<Read>,
    pub stream: Stream,
}

/// What `read(2)` delivers at most per call, the tty's own quantum.
const READ_CHUNK: usize = 4096;

/// Whether a write reached the child whole, in part, or not at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrote {
    /// The kernel took every byte owed, and the outbox is empty.
    All,
    /// The kernel took some of it and not all: the slave's input queue
    /// filled because the child is not reading. The rest is still in the
    /// outbox, first in line for the next write. It was lost until
    /// 2026-10-09.
    Part,
    /// The kernel took none of it: the queue is full, or the child is gone,
    /// and everything owed is still owed. The activity lights' SD stays
    /// dark for it, since nothing was sent.
    Nothing,
}

/// Bytes owed to the child that the kernel has not taken yet, oldest first.
///
/// **Every write to the child goes through one**, so nothing overtakes what
/// is already owed: a key cannot land between the halves of a mouse report
/// the kernel split, and a paste cannot jump the tail of a wheel burst. It is
/// unbounded, and only what the user does fills it -- keys, a paste, the
/// wheel, and the core's answers to the child's own queries -- so a child
/// that stops reading costs memory in proportion to what was typed at it,
/// where it used to cost the window.
#[derive(Default)]
pub struct Outbox {
    bytes: Vec<u8>,
}

impl Outbox {
    /// Owe `b` too, after everything already owed.
    pub fn push(&mut self, b: &[u8]) {
        self.bytes.extend_from_slice(b);
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Whether bytes are owed that the kernel has not taken: a Linux wait
    /// then wakes for room to write, and on macOS the master's room to
    /// write is registered (`OutboxWatch::write`).
    pub fn owed(&self) -> Owed {
        match self.bytes.is_empty() {
            true => Owed::Nothing,
            false => Owed::Bytes,
        }
    }

    /// **Everything owed, let go**: the child's stream has closed, and
    /// nothing written now reaches it.
    #[cfg(target_os = "macos")]
    pub fn let_go(&mut self) {
        self.bytes.clear();
    }
}

/// What one `read(2)` of the master found.
enum Got<'a> {
    /// What the child wrote since the last read.
    Bytes(&'a [u8]),
    /// Nothing yet: the master is non-blocking and the child has written
    /// nothing since the last read, or a signal cut the read short. Neither
    /// is the end of the stream.
    Nothing,
    /// The child has gone: end of file on Darwin, `EIO` on Linux.
    Closed,
}

/// One `read(2)` of the master into `buf`, shared by the macOS reader thread
/// and the Linux loop so the two cannot disagree about which answer ends the
/// stream.
fn read_master(fd: crate::sys::Fd, buf: &mut [u8]) -> Got<'_> {
    // SAFETY: an open descriptor and a buffer valid for its length.
    let n = unsafe { crate::sys::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    match usize::try_from(n) {
        Ok(0) => Got::Closed,
        Ok(n) => Got::Bytes(&buf[..n.min(buf.len())]),
        // Every failed read was `Closed` while the master blocked, which
        // could not answer `EAGAIN`; a non-blocking one answers it whenever
        // the child is quiet.
        Err(_) => match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => Got::Nothing,
            _ => Got::Closed,
        },
    }
}

/// What a wait on the master for something to read ended with.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Waited {
    /// Something to read, or a hang-up -- the read that follows says which.
    Ready,
    /// `poll` answered `POLLNVAL`: it cannot watch this descriptor, and every
    /// wait would return at once, so waiting again would spin.
    Unwatchable,
    /// `poll` itself failed, which a retry would only repeat.
    Failed,
    /// The window's `Pty` dropped the pipe's write end: its window closed.
    #[cfg(target_os = "macos")]
    Closing,
}

/// Waits, as long as it takes, until the master has something to read or
/// has hung up: the macOS reader's wait, now that a read returns at once.
#[cfg(any(target_os = "macos", test))]
fn readable(fd: crate::sys::Fd) -> Waited {
    let mut p = crate::sys::PollFd {
        fd,
        events: crate::sys::POLLIN,
        revents: 0,
    };
    loop {
        // SAFETY: one initialised record and its count; the kernel writes
        // only `revents`, and the record outlives the call.
        if unsafe { crate::sys::poll(&mut p, 1, -1) } >= 0 {
            return match p.revents & crate::sys::POLLNVAL {
                0 => Waited::Ready,
                _ => Waited::Unwatchable,
            };
        }
        // A signal, or `EAGAIN` -- which Darwin's page gives for "Allocation
        // of internal data structures fails. A subsequent request may
        // succeed." -- is waited again; anything else would only repeat.
        match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock => {}
            _ => return Waited::Failed,
        }
    }
}

/// [`readable`], or until `closing` -- the read end of the pipe a
/// window's [`Pty`] holds the other end of -- is readable or hung up,
/// which only the window's close makes it.
#[cfg(target_os = "macos")]
fn readable_unless(fd: crate::sys::Fd, closing: Option<crate::sys::Fd>) -> Waited {
    let Some(closing) = closing else {
        return readable(fd);
    };
    let mut p = [
        crate::sys::PollFd {
            fd,
            events: crate::sys::POLLIN,
            revents: 0,
        },
        crate::sys::PollFd {
            fd: closing,
            events: crate::sys::POLLIN,
            revents: 0,
        },
    ];
    loop {
        // SAFETY: two initialised records and their count; the kernel
        // writes only `revents`, and the records outlive the call.
        if unsafe { crate::sys::poll(p.as_mut_ptr(), 2, -1) } >= 0 {
            return match (p[1].revents, p[0].revents & crate::sys::POLLNVAL) {
                (0, 0) => Waited::Ready,
                (0, _) => Waited::Unwatchable,
                _ => Waited::Closing,
            };
        }
        match std::io::Error::last_os_error().kind() {
            std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock => {}
            _ => return Waited::Failed,
        }
    }
}

/// Makes the master's reads and writes return rather than wait.
fn nonblocking(fd: crate::sys::Fd) -> std::io::Result<()> {
    // SAFETY: `F_GETFL` takes no third argument and `F_SETFL` an `int`; the
    // descriptor is open.
    let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above.
    if unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags | crate::sys::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Makes the master non-blocking, or says why it stays blocking -- in which
/// case a write the queue cannot hold waits in the kernel again, as every
/// write did before this.
fn make_nonblocking(fd: crate::sys::Fd) {
    if let Err(e) = nonblocking(fd) {
        crate::warn!("pty: the master stays blocking, so a full queue waits: {e}");
    }
}

/// Makes the master's reads and writes wait again: the reader's way back
/// when `poll` cannot watch the master, to the blocking reads and writes
/// every version before this one had.
#[cfg(target_os = "macos")]
fn blocking(fd: crate::sys::Fd) -> std::io::Result<()> {
    // SAFETY: as in `nonblocking`.
    let flags = unsafe { crate::sys::fcntl(fd, crate::sys::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: as above.
    if unsafe { crate::sys::fcntl(fd, crate::sys::F_SETFL, flags & !crate::sys::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Writes as much of `out` to `fd` as the kernel takes now, takes that off
/// the front, and says whether any is left.
fn write_some(fd: crate::sys::Fd, out: &mut Outbox) -> Wrote {
    let mut at = 0usize;
    while at < out.bytes.len() {
        let rest = &out.bytes[at..];
        // SAFETY: the slice is valid for its length and the fd is ours.
        let n = unsafe { crate::sys::write(fd, rest.as_ptr().cast(), rest.len()) };
        match usize::try_from(n) {
            Ok(0) => break,
            Ok(n) => at += n,
            Err(_) => match std::io::Error::last_os_error().kind() {
                // A signal cut it short: write again.
                std::io::ErrorKind::Interrupted => {}
                // `EAGAIN`: the queue is full until the child reads. Anything
                // else: the child is gone, and the reader is about to say so.
                _ => break,
            },
        }
    }
    out.bytes.drain(..at);
    match (out.bytes.is_empty(), at) {
        (true, _) => Wrote::All,
        (false, 0) => Wrote::Nothing,
        (false, _) => Wrote::Part,
    }
}

/// The shell when `$SHELL` is unset: the system's.
#[cfg(target_os = "macos")]
const DEFAULT_SHELL: &str = "/bin/zsh";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHELL: &str = "/bin/sh";

/// **The shell a window starts when `$SHELL` is unset**, given the
/// account's login shell as the password database has it: on Linux that
/// shell when it is absolute, else [`DEFAULT_SHELL`]; on macOS the system's,
/// [`DEFAULT_SHELL`], whatever the entry says.
fn system_shell(passwd: Option<&str>) -> String {
    #[cfg(not(target_os = "macos"))]
    if let Some(p) = passwd.filter(|p| p.starts_with('/')) {
        return p.to_string();
    }
    #[cfg(target_os = "macos")]
    let _ = passwd;
    DEFAULT_SHELL.to_string()
}

/// The account's login shell from the password database, where it is read.
fn passwd_shell() -> Option<String> {
    #[cfg(target_os = "linux")]
    return crate::sys::passwd_shell();
    #[cfg(not(target_os = "linux"))]
    None
}

/// The variables a terminal sets to name itself to what runs inside it,
/// so a program can pick the terminal's own features -- and the ones a
/// multiplexer sets to say it sits in between. baud's shell is in none of
/// them: launched from Terminal.app it would otherwise tell every child it
/// *is* Terminal.app, and on 2026-09-22 that is what happened. yazi read
/// the inherited `TERM_PROGRAM=Apple_Terminal`, decided the terminal was
/// Apple's -- a brand with no image support in its table -- and fell back
/// to chafa without ever reading baud's own device attributes; chafa read
/// the same variable and drew in 240 colours rather than truecolour.
///
/// Three entries are baud's own: shell integration's
/// [`crate::shell::HELPERS`].
///
/// The list is every variable yazi 26.5.6's `Brand::from_env` reads
/// (`yazi-emulator/src/brand.rs`), the rest of each named terminal's own
/// family, the multiplexers yazi's `Mux` consults, and the three that
/// describe the outer terminal to any program: `COLORFGBG` (its colours,
/// not baud's), `WINDOWID` (its X window) and `TERMINFO` (its terminfo
/// directory, which ncurses searches *alone* when the variable is set, so
/// a Ghostty- or kitty-provided one hides the `xterm-256color` entry baud
/// asks for). A variable a user sets in their own rc is set after this and
/// is theirs.
const OUTER: &[&str] = &[
    // Apple Terminal.
    "TERM_SESSION_ID",
    // iTerm2.
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "TERM_FEATURES",
    // kitty.
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_PUBLIC_KEY",
    "KITTY_INSTALLATION_DIR",
    "KITTY_LISTEN_ON",
    // WezTerm.
    "WEZTERM_EXECUTABLE",
    "WEZTERM_EXECUTABLE_DIR",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_CONFIG_DIR",
    "WEZTERM_CONFIG_FILE",
    // Ghostty.
    "GHOSTTY_RESOURCES_DIR",
    "GHOSTTY_BIN_DIR",
    "GHOSTTY_SHELL_FEATURES",
    // Alacritty.
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "ALACRITTY_LOG",
    // Konsole.
    "KONSOLE_VERSION",
    "KONSOLE_DBUS_SERVICE",
    "KONSOLE_DBUS_SESSION",
    "KONSOLE_DBUS_WINDOW",
    "SHELL_SESSION_ID",
    // VTE-based terminals.
    "VTE_VERSION",
    // Windows Terminal.
    "WT_SESSION",
    "WT_PROFILE_ID",
    // Warp.
    "WARP_HONOR_PS1",
    "WARP_IS_LOCAL_SHELL_SESSION",
    "WARP_USE_SSH_WRAPPER",
    // VS Code's integrated terminal.
    "VSCODE_INJECTION",
    "VSCODE_SHELL_INTEGRATION",
    // Tabby.
    "TABBY_CONFIG_DIRECTORY",
    // JetBrains' terminal.
    "TERMINAL_EMULATOR",
    // Multiplexers.
    "TMUX",
    "TMUX_PANE",
    "STY",
    "WINDOW",
    "ZELLIJ",
    "ZELLIJ_SESSION_NAME",
    "ZELLIJ_PANE_ID",
    // baud's own: what shell integration hands a shell's startup to put
    // back or to read, and its marker, none of them a parent's to pass on.
    crate::shell::HELPERS[0],
    crate::shell::HELPERS[1],
    crate::shell::HELPERS[2],
    crate::shell::HELPERS[3],
    crate::shell::HELPERS[4],
    // A filmed take's journal (`take.rs`): the window filmed writes it, and
    // a baud inside it, or any program, is never handed it.
    "BAUD_TAKE",
    // The outer terminal's colours, window and terminfo.
    "COLORFGBG",
    "WINDOWID",
    "TERMINFO",
];

/// What baud says about itself, replacing whatever the parent said.
/// `TERM_PROGRAM` is the convention every macOS terminal keeps --
/// `Apple_Terminal`, `iTerm.app`, `ghostty`, `WezTerm` -- and a name no
/// program's table knows is the honest answer: yazi then decides from
/// baud's own replies, and chafa from `COLORTERM`.
const OURS: [(EnvName, &str); 4] = [
    // The entry whose `flash` length the bell reads, spelled once there.
    (EnvName::fixed("TERM"), crate::bell::TERM),
    (EnvName::fixed("COLORTERM"), "truecolor"),
    (EnvName::fixed("TERM_PROGRAM"), "baud"),
    (EnvName::fixed("TERM_PROGRAM_VERSION"), env!("BAUD_VERSION")),
];

/// The variables a shell reads its locale from: `LC_ALL` outranks every
/// category, `LC_CTYPE` sets the one that decides how input bytes become
/// characters, and `LANG` stands under both.
const LOCALE: [EnvName; 3] = [LC_ALL, LC_CTYPE, LANG];

/// **Whether the parent names a locale**: one of [`LOCALE`] set and not
/// empty, as the child reads it, the first entry of its name -- POSIX reads
/// an empty one as unset. Other `LC_` names do not
/// count: each sets one category and leaves the character type to `LANG`,
/// and iTerm2's `LC_TERMINAL` is a terminal's name, not a locale's. The one
/// test both [`spawn_env`] and [`environment`] ask, so the two cannot
/// disagree about when a locale is filled.
fn names_a_locale(env: &ChildEnv) -> bool {
    LOCALE
        .iter()
        .any(|l| env.get(l).is_some_and(|v| !v.is_empty()))
}

/// [`environment`] of `parent` as `execve` takes it, for the tests that run
/// a program under the seat with the environment a window's child has.
#[cfg(test)]
pub(crate) fn child_env(parent: Vec<(OsString, OsString)>, locale: Option<&str>) -> Vec<CString> {
    environment(ChildEnv::of(parent), locale).into_envp()
}

/// The child's environment: the parent's, less every variable in
/// [`OUTER`] and every one baud sets, then baud's own in [`OURS`] -- and
/// `LANG` set to `locale` when the parent names no locale
/// ([`names_a_locale`]), replacing an empty one rather than adding a second
/// that `getenv`, which reads the first, would never see. Built in the
/// parent, before the fork, so the child runs no `putenv` between `fork`
/// and `exec`: it may reallocate the environment array in a process that
/// was multi-threaded a moment ago, and it is not async-signal-safe. The
/// child runs only [`utf8_erase`], the `chdir` [`start_in`] decided, a
/// test of what it returned, the exec and, if the exec fails, `_exit`.
/// Kept as a [`ChildEnv`] until the fork, so the spawn can still set what
/// the program and shell integration change.
fn environment(mut env: ChildEnv, locale: Option<&str>) -> ChildEnv {
    let fill = locale.filter(|_| !names_a_locale(&env));
    env.retain(|k| !OUTER.iter().any(|o| o.as_bytes() == k.as_bytes()));
    for (k, v) in &OURS {
        env.set(k, *v);
    }
    if let Some(l) = fill {
        env.set(&LANG, l);
    }
    env
}

/// **The environment a shell is spawned with**: [`environment`], with a
/// locale asked of `locale` only when the parent names none. A Dock launch
/// inherits launchd's environment, which has no locale, and zsh in the C
/// locale reads each byte of a UTF-8 character as a character of its own:
/// typed Hebrew came out as `U+FFFD` and `<0099>`. On macOS the answer is
/// CoreFoundation's, and a launch from a terminal, which passes its `LANG`
/// on, never pays for it.
fn spawn_env(
    parent: Vec<(OsString, OsString)>,
    locale: impl FnOnce() -> Option<String>,
) -> ChildEnv {
    let env = ChildEnv::of(parent);
    let found = (!names_a_locale(&env)).then(locale).flatten();
    environment(env, found.as_deref())
}

/// **The names a shell's locale is tried as**, in order, each as UTF-8 and
/// each once: the user's identifier without its `@` keywords (a region
/// override, `en_US@rg=ilzzzz`, is no part of a POSIX name), their language
/// and region (`zh_CN` for `zh-Hans_CN`), then `en_US`, then `C`. Language
/// and region can name a locale the system lacks -- English in Israel,
/// `en_IL` -- which is why each name is checked rather than trusted.
#[cfg(target_os = "macos")]
fn locale_candidates(
    identifier: Option<&str>,
    language: Option<&str>,
    region: Option<&str>,
) -> Vec<String> {
    fn given(s: Option<&str>) -> Option<&str> {
        s.filter(|s| !s.is_empty())
    }
    let own = given(identifier)
        .map(|id| id.split_once('@').map_or(id, |(base, _)| base))
        .filter(|base| !base.is_empty())
        .map(str::to_string);
    let pair = given(language)
        .zip(given(region))
        .map(|(l, r)| format!("{l}_{r}"));
    let mut names: Vec<String> = Vec::new();
    for base in own
        .into_iter()
        .chain(pair)
        .chain(["en_US".to_string(), "C".to_string()])
    {
        let name = format!("{base}.UTF-8");
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The first of `candidates` that `valid` accepts.
#[cfg(target_os = "macos")]
fn first_valid(candidates: &[String], valid: impl Fn(&str) -> bool) -> Option<String> {
    candidates.iter().find(|c| valid(c)).cloned()
}

/// **Whether the system has `name` as a UTF-8 locale**: `newlocale` loads
/// it for every category and its character set reads `UTF-8`. Every
/// category, because `LANG` feeds them all and a shell's
/// `setlocale(LC_ALL, "")` fails whole when one cannot load -- the
/// character type alone accepts `UTF-8`, which the whole mask refuses. The
/// character set, because `C`, `POSIX` and a bare `en_US` all load and are
/// not UTF-8. An empty name is refused first: `newlocale` reads it as the
/// environment's own locale, the one being replaced. Measured on macOS 27
/// (`scripts/locale/validate_probe.c`).
#[cfg(target_os = "macos")]
fn is_utf8_locale(name: &str) -> bool {
    use crate::sys::{CODESET, LC_ALL_MASK, freelocale, newlocale, nl_langinfo_l};
    if name.is_empty() {
        return false;
    }
    let Ok(name) = CString::new(name) else {
        return false;
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; a null base
    // asks for a new object.
    let locale = unsafe { newlocale(LC_ALL_MASK, name.as_ptr(), core::ptr::null_mut()) };
    if locale.is_null() {
        return false;
    }
    // SAFETY: `locale` is the live object just made, freed once here after
    // the string `nl_langinfo_l` returns for it -- which lives as long as
    // the object does -- has been read.
    unsafe {
        let set = nl_langinfo_l(CODESET, locale);
        let utf8 = !set.is_null() && std::ffi::CStr::from_ptr(set).to_bytes() == b"UTF-8";
        freelocale(locale);
        utf8
    }
}

/// **Whether the system has the locale `name`**: `newlocale` loads it
/// for every category, as libxkbcommon asks before it gives a locale its
/// Compose registry does not list the `en_US.UTF-8` table
/// (`[[source: libxkbcommon-compose-paths-c]]`), so a misspelt or
/// uninstalled one makes none.
#[cfg(target_os = "linux")]
pub fn locale_installed(name: &str) -> bool {
    use crate::sys::{LC_ALL_MASK, freelocale, newlocale};
    let Ok(name) = CString::new(name) else {
        return false;
    };
    // SAFETY: `name` is NUL-terminated and outlives the call; a null base
    // asks for a new object.
    let locale = unsafe { newlocale(LC_ALL_MASK, name.as_ptr(), core::ptr::null_mut()) };
    if locale.is_null() {
        return false;
    }
    // SAFETY: `locale` is the live object just made, freed once here.
    unsafe { freelocale(locale) };
    true
}

/// A CoreFoundation string as text: `None` for null, or for one longer than
/// 63 bytes -- a locale's identifier, language or region is a few letters.
#[cfg(target_os = "macos")]
pub(crate) fn cf_text(s: crate::sys::CfString) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut buf = [0u8; 64];
    let size = crate::sys::CfIndex::try_from(buf.len()).ok()?;
    // SAFETY: `s` is a live string, owned by a locale that outlives the
    // call or a framework constant, and `buf` is writable for the `size`
    // bytes passed.
    let ok = unsafe {
        crate::sys::CFStringGetCString(
            s,
            buf.as_mut_ptr().cast(),
            size,
            crate::sys::CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    Some(
        std::ffi::CStr::from_bytes_until_nul(&buf)
            .ok()?
            .to_str()
            .ok()?
            .to_string(),
    )
}

/// The user's locale identifier, language and region, as CoreFoundation
/// reads them from their preferences -- `en_US`, `en` and `US` on this Mac
/// -- whatever the environment says.
#[cfg(target_os = "macos")]
fn users_locale() -> (Option<String>, Option<String>, Option<String>) {
    use crate::sys::{
        CF_LOCALE_COUNTRY_CODE, CF_LOCALE_LANGUAGE_CODE, CFLocaleCopyCurrent,
        CFLocaleGetIdentifier, CFLocaleGetValue, CFRelease,
    };
    // SAFETY: `CFLocaleCopyCurrent` has no preconditions and returns a
    // reference this function owns, checked for null and released once;
    // the strings the `Get` calls return belong to the locale and are
    // copied out before it is released. The keys are framework constants,
    // read as the pointers they are.
    unsafe {
        let locale = CFLocaleCopyCurrent();
        if locale.is_null() {
            return (None, None, None);
        }
        let got = (
            cf_text(CFLocaleGetIdentifier(locale)),
            cf_text(CFLocaleGetValue(locale, CF_LOCALE_LANGUAGE_CODE)),
            cf_text(CFLocaleGetValue(locale, CF_LOCALE_COUNTRY_CODE)),
        );
        CFRelease(locale.object());
        got
    }
}

/// **The locale a shell is given when its environment names none**: the
/// user's, as CoreFoundation reads their preferences, if the system has it
/// as UTF-8, else the first fallback that is ([`locale_candidates`]);
/// `None` when none is, which leaves the environment as it was. What it
/// cost, and that it was asked at all, print under `BAUD_STARTUP`.
#[cfg(target_os = "macos")]
fn shell_locale() -> Option<String> {
    let asked = std::time::Instant::now();
    let (identifier, language, region) = users_locale();
    let found = first_valid(
        &locale_candidates(
            identifier.as_deref(),
            language.as_deref(),
            region.as_deref(),
        ),
        is_utf8_locale,
    );
    crate::host::startup_note(&format!(
        "locale: {} in {:.3} ms",
        found.as_deref().unwrap_or("none"),
        asked.elapsed().as_secs_f64() * 1e3
    ));
    found
}

/// On Linux the parent's environment passes on as it is, an unset `LANG`
/// included.
#[cfg(target_os = "linux")]
fn shell_locale() -> Option<String> {
    None
}

/// **A child forked onto a pty**: its pid, the master, and the token baud
/// handed its shell, which the window's marks are read against
/// (`ran::Commands::new`); none for a shell the integration does not mark.
type Forked = (
    crate::sys::Pid,
    std::os::fd::OwnedFd,
    Option<crate::ran::ShellToken>,
);

/// **What `[shell]` says about the shell a window starts**: whether its
/// prompts are marked (`integration`) and which program it is (`program`,
/// [`crate::config::shell_program`]; `None` starts `$SHELL`, else the
/// system's) -- or, for a window a link's `action = "window"` or `baud -e`
/// opened, the program it runs in place of any shell (`run`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellStart {
    pub integration: crate::shell::Integration,
    /// Whether the integration wraps `ssh` (`[shell] ssh_env`).
    pub ssh_env: crate::shell::SshEnv,
    pub program: Option<String>,
    /// The argv the window runs instead of a shell, with no shell between:
    /// none for a shell window.
    pub run: Option<crate::rules::Run>,
}

impl ShellStart {
    /// The shell the config names, marked as it says.
    pub fn of(cfg: &crate::config::Config) -> ShellStart {
        ShellStart {
            integration: cfg.shell_integration,
            ssh_env: cfg.shell_ssh_env,
            program: cfg.shell_program.clone(),
            run: None,
        }
    }

    /// This start, running `run` in place of the shell.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn running(self, run: crate::rules::Run) -> ShellStart {
        ShellStart {
            run: Some(run),
            ..self
        }
    }
}

impl From<crate::shell::Integration> for ShellStart {
    /// `$SHELL`, else the system's, marked as `integration` says, `ssh`
    /// as it is.
    fn from(integration: crate::shell::Integration) -> ShellStart {
        ShellStart {
            integration,
            ssh_env: crate::shell::SshEnv::Off,
            program: None,
            run: None,
        }
    }
}

/// **What the child runs**: the path `execve` is given and the argument
/// list, decided in the parent before the fork, with what shell
/// integration changes in the environment.
struct Exec {
    path: exec::Program,
    argv: Vec<CString>,
    injection: Option<crate::shell::Injection>,
    /// `SHELL=` the configured program, when `[shell] program` names one:
    /// the child's `SHELL`, in place of the parent's. POSIX has `SHELL`
    /// "represent a pathname of the user's preferred command language
    /// interpreter" (`[[source: posix-xbd-8]]`), and `login` sets it to the
    /// shell it starts; left at the parent's, which from the Dock is the
    /// account's, a multiplexer's or an editor's shell would start that
    /// one instead. Terminals differ: WezTerm sets `SHELL` from `$SHELL`
    /// or the account's entry and not from `default_prog`, and alacritty
    /// leaves the parent's; kitty's and Ghostty's handling was not read
    /// at source.
    shell_var: Option<OsString>,
}

impl Exec {
    /// **`run` itself, with no shell between**: its program as written when
    /// absolute, else found on the absolute entries of `parent`'s `PATH`
    /// ([`exec::ExecPath::resolve`]), else not found, so the child ends with
    /// 127 without an `execve` and the window says how it ended; its elements the child's argument list as they are, one
    /// argument each, never split or joined. Nothing of shell integration
    /// applies; `SHELL` is the configured `program` when `[shell]` names one,
    /// as a shell window's is, so an editor's shell-out starts the shell the
    /// config asks for. `None` when an element cannot be a C string.
    fn running(
        run: &crate::rules::Run,
        program: Option<&str>,
        parent: &[(OsString, OsString)],
    ) -> Option<Exec> {
        let path = exec::Program::of(run.program(), path_var(parent));
        let argv = run
            .argv()
            .iter()
            .map(|a| CString::new(a.as_bytes()).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(Exec {
            path,
            argv,
            injection: None,
            shell_var: program.map(OsString::from),
        })
    }

    /// `/bin/sh -c cmd` for a command, as `BAUD_CMD` runs one; otherwise
    /// a login shell, `program` when `[shell] program` names one, else
    /// `$SHELL` as `parent` has it (else the system's) ([`login_shell`]),
    /// with argv[0] its name after a `-`, as `login` starts one, and with
    /// whatever `integration` adds to its start, which its name picks
    /// ([`crate::shell::inject`]). A configured program is also the
    /// child's `SHELL`. `None` when an argument cannot be a C string.
    fn of(
        cmd: Option<String>,
        parent: &[(OsString, OsString)],
        integration: crate::shell::Integration,
        ssh_env: crate::shell::SshEnv,
        program: Option<&str>,
    ) -> Option<Exec> {
        if let Some(c) = cmd {
            return Some(Exec {
                path: exec::Program::At(exec::ExecPath::resolve(OsStr::new("/bin/sh"), None)?),
                argv: vec![
                    CString::new("sh").ok()?,
                    CString::new("-c").ok()?,
                    CString::new(c).ok()?,
                ],
                injection: None,
                shell_var: None,
            });
        }
        let (shell, refused) = login_shell(program, parent);
        if let Some(v) = refused {
            crate::warn!("SHELL={v:?} is not an absolute path, so the window starts {shell}");
        }
        let at = exec::ExecPath::resolve(OsStr::new(&shell), path_var(parent))
            .unwrap_or_else(exec::ExecPath::default_shell);
        let path = at.as_str().unwrap_or(DEFAULT_SHELL).to_string();
        let argv0 = CString::new(format!("-{}", shell.rsplit('/').next().unwrap_or("sh"))).ok()?;
        let injection = crate::shell::inject(integration, ssh_env, &shell, &path, parent);
        let argv = core::iter::once(argv0)
            .chain(injection.iter().flat_map(|i| i.args.iter().cloned()))
            .collect();
        let shell_var = program.map(OsString::from);
        Some(Exec {
            path: exec::Program::At(at),
            argv,
            injection,
            shell_var,
        })
    }
}

/// **The shell a session starts**: `program`, `[shell] program` when the
/// config names one ([`crate::config::shell_program`]), else `$SHELL` as
/// `parent` has it when it is absolute, else [`system_shell`]: what
/// [`Exec::of`] runs when no command is given. Beside it, a `$SHELL` that
/// was set and refused, for the caller to say.
///
/// **A `$SHELL` that is not absolute is unset.** POSIX has `SHELL`
/// "represent a pathname of the user's preferred command language
/// interpreter" (`[[source: posix-xbd-8]]`), login and launchd set it
/// absolute, and `[shell] program` refuses anything else. It is read again
/// for every window, and the child changes folder before it runs it, so a
/// relative one would make a folder's `bin/zsh` the shell of every window
/// opened there; a bare `zsh` is unset as well. Empty is unset and not said.
fn login_shell(program: Option<&str>, parent: &[(OsString, OsString)]) -> (String, Option<String>) {
    login_shell_of(program, parent, passwd_shell)
}

/// [`login_shell`], with the account's password-database shell read by
/// `account`, only when no `$SHELL` is taken.
fn login_shell_of(
    program: Option<&str>,
    parent: &[(OsString, OsString)],
    account: impl FnOnce() -> Option<String>,
) -> (String, Option<String>) {
    if let Some(p) = program {
        return (p.to_string(), None);
    }
    let set = parent
        .iter()
        .find(|(k, _)| k.as_bytes() == b"SHELL")
        .map(|(_, v)| v)
        .filter(|v| !v.is_empty());
    let system = || system_shell(account().as_deref());
    match set {
        Some(v) if v.as_bytes().first() == Some(&b'/') => match v.to_str() {
            Some(s) => (s.to_string(), None),
            None => (system(), None),
        },
        Some(v) => (system(), Some(v.to_string_lossy().into_owned())),
        None => (system(), None),
    }
}

/// **The program a window started for `run` executes**, resolved as
/// `Exec::running` resolves it on `parent`'s `PATH`
/// (`exec::Program::of`), or none when it is not there: what the film's
/// tap records for a link's window (`film/tap.rs`), which opens none.
#[cfg(all(test, target_os = "macos"))]
pub(crate) fn window_program(
    run: &crate::rules::Run,
    parent: &[(OsString, OsString)],
) -> Option<String> {
    exec::Program::of(run.program(), path_var(parent))
        .as_str()
        .map(str::to_string)
}

/// `PATH` as `parent` has it, the first entry of the name, as `getenv`
/// reads it.
fn path_var(parent: &[(OsString, OsString)]) -> Option<&OsStr> {
    parent
        .iter()
        .find(|(k, _)| k.as_bytes() == b"PATH")
        .map(|(_, v)| v.as_os_str())
}

/// **The name of the shell a session starts**, [`login_shell`]'s last
/// path component, `program` or this process's environment: the badge's
/// `shell`, as iTerm2 names the user's shell by its last path component
/// (`bestGuessAtUserShellWithPath:NO`, `[[source:
/// iterm2-964a75d-ptysession]]`).
pub(crate) fn shell_name(program: Option<&str>) -> String {
    let (shell, _) = login_shell(program, &std::env::vars_os().collect::<Vec<_>>());
    shell.rsplit('/').next().unwrap_or(&shell).to_string()
}

/// **What `execve` is given, made only here**, so the module boundary
/// holds the rule the compiler cannot see in a binary crate: nothing else
/// in this file can build an [`ExecPath`](exec::ExecPath).
mod exec {
    use std::ffi::{CStr, CString, OsStr};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};

    /// **An absolute path to a program**, the only kind the child is
    /// given. The child changes folder before it runs the program
    /// ([`super::StartIn`]), so a relative path would name a file in the
    /// folder it moved to, not the one it was typed in; a relative
    /// program is made absolute where it was typed (`run_arg::asked`), or
    /// it is not found.
    #[derive(Debug)]
    pub(super) struct ExecPath(CString);

    impl ExecPath {
        /// `prog` as `execve` can take it: itself when it is absolute; a
        /// bare name, the first executable of that name in an absolute
        /// entry of `path_var`, the search `execvp` would do, done here
        /// because the child must not allocate; anything else, a path
        /// relative to some folder, none.
        ///
        /// **An empty or relative `PATH` entry is skipped**, as
        /// `probe::command` skips one: POSIX reads a zero-length prefix as
        /// "the current working directory" (`[[source: posix-xbd-8]]`),
        /// which for the child is the folder it moved to.
        pub(super) fn resolve(prog: &OsStr, path_var: Option<&OsStr>) -> Option<ExecPath> {
            use std::os::unix::fs::PermissionsExt;
            let bytes = prog.as_bytes();
            if bytes.first() == Some(&b'/') {
                return Some(ExecPath(CString::new(bytes).ok()?));
            }
            if bytes.is_empty() || bytes.contains(&b'/') {
                return None;
            }
            std::env::split_paths(path_var?)
                .filter(|dir| dir.is_absolute())
                .map(|dir| dir.join(prog))
                .find(|p| {
                    std::fs::metadata(p)
                        .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                })
                .and_then(|p| CString::new(p.into_os_string().into_vec()).ok())
                .map(ExecPath)
        }

        /// The system's shell, [`super::DEFAULT_SHELL`], absolute.
        pub(super) fn default_shell() -> ExecPath {
            ExecPath(CString::new(super::DEFAULT_SHELL).expect("the default shell has no NUL"))
        }

        pub(super) fn as_c(&self) -> &CStr {
            &self.0
        }

        pub(super) fn as_str(&self) -> Option<&str> {
            self.0.to_str().ok()
        }
    }

    /// **What the child runs**: a program at an absolute path, or none
    /// found, for which the child ends with 127, what a shell answers for
    /// a command not found, without an `execve`.
    #[derive(Debug)]
    pub(super) enum Program {
        At(ExecPath),
        NotFound,
    }

    impl Program {
        /// `prog` resolved ([`ExecPath::resolve`]), else not found.
        pub(super) fn of(prog: &OsStr, path_var: Option<&OsStr>) -> Program {
            ExecPath::resolve(prog, path_var).map_or(Program::NotFound, Program::At)
        }

        /// The path `execve` is given, none when nothing was found.
        pub(super) fn as_c(&self) -> Option<&CStr> {
            match self {
                Program::At(p) => Some(p.as_c()),
                Program::NotFound => None,
            }
        }

        #[cfg(test)]
        pub(super) fn as_str(&self) -> Option<&str> {
            match self {
                Program::At(p) => p.as_str(),
                Program::NotFound => None,
            }
        }
    }
}

/// The `winsize` for a grid of `cells` whose cells are `cell_w` by `cell_h`
/// device pixels: the cell counts, and **the text area** in pixels.
///
/// The text area and not the window, because a child reads `ws_xpixel /
/// ws_col` as the width of a cell -- `[[source: yazi-dimension]]` does exactly
/// that -- and the window's width carries the padding and the part of a cell
/// the grid does not fill. `[[source: inband-resize]]` defines the same
/// quantity for the in-band report, "Text area does not include any padding
/// the terminal applies to the window", and `CSI 14 t` answers with it, so
/// all three agree. Device pixels, because that is what every render path
/// draws a Sixel pixel as. Saturating at the `u16` the kernel's field holds.
pub fn text_area(cells: &WinSize, cell_w: Px, cell_h: Px) -> crate::sys::Winsize {
    let span = |cell: Px, n: u16| {
        u16::try_from(u64::from(cell.value()) * u64::from(n)).unwrap_or(u16::MAX)
    };
    crate::sys::Winsize {
        ws_row: cells.ws_row,
        ws_col: cells.ws_col,
        ws_xpixel: span(cell_w, cells.ws_col),
        ws_ypixel: span(cell_h, cells.ws_row),
    }
}

/// Sends `ws` to the kernel through `master`, unless `told` says the kernel
/// already holds it -- a `TIOCSWINSZ` is a `SIGWINCH` to the child, and a
/// shell woken for a size it already has redraws its prompt for nothing.
fn tell(master: crate::sys::Fd, told: &mut crate::sys::Winsize, ws: crate::sys::Winsize) {
    if ws == *told {
        return;
    }
    // SAFETY: TIOCSWINSZ takes a pointer to a winsize.
    unsafe {
        crate::sys::ioctl(master, crate::sys::TIOCSWINSZ, &ws);
    }
    *told = ws;
}

/// **Backspace erases a whole UTF-8 character**: `IUTF8` set on the
/// terminal `fd`, which the kernel's default leaves clear on Darwin and on
/// Linux. Without it a line read in canonical mode -- `cat`, `read` -- kept
/// a Hebrew letter's lead byte `d7` after Backspace.
///
/// Called in the child, on descriptor 0, before the exec, so the shell's
/// first look at its terminal already has the flag. It changes that one
/// bit of the `c_iflag` it reads. A failed call leaves the kernel's default
/// and the shell still starts: the parent is not told (an open
/// question). After the spawn the parent keeps the bit with the input
/// encoding ([`LineDiscipline`]).
///
/// The attributes set are the ones `tcgetattr` read, with one bit changed,
/// and a failed read sets nothing: `tcsetattr`'s effect "is undefined if
/// the value of the termios structure pointed to by termios_p was not
/// derived from the result of a call to tcgetattr() on fildes"
/// (`[[source: posix-tcsetattr]]`).
///
/// # Safety
/// Only async-signal-safe calls, over the child's stack: `tcgetattr` and
/// `tcsetattr` are in XSH 2.4.3's table (`[[source: posix-xsh-chap02]]`).
/// It allocates, formats and panics nowhere, and reads no `errno`.
unsafe fn utf8_erase(fd: crate::sys::Fd) {
    let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
    let p = t.as_mut_ptr();
    // SAFETY: `p` is a termios-sized, termios-aligned block, which the
    // call fills when it succeeds.
    if unsafe { crate::sys::tcgetattr(fd, p) } != 0 {
        return;
    }
    // SAFETY: the call above succeeded, so `*p` is initialised; the flag
    // word is changed in place and nothing else is touched.
    unsafe {
        (*p).c_iflag |= crate::sys::IUTF8;
        crate::sys::tcsetattr(fd, crate::sys::TCSANOW, p);
    }
}

/// The child's terminal after `forkpty`: its `login_tty` makes the
/// subsidiary descriptors 0, 1 and 2, and 0 is the one Terminal.app asks.
const CHILD_TERMINAL: crate::sys::Fd = 0;

/// **Who started baud**, as far as where its shell starts goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Parent {
    /// launchd, which is the parent of an app `open` starts (measured), and
    /// so, inferred, of one the Dock or Finder starts, which LaunchServices
    /// starts the same way. Never read on Linux.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Launchd,
    /// A shell, a debugger, anything else.
    Other,
}

/// launchd's process ID. launchd(8): "During boot launchd is invoked by
/// the kernel to run as the first process on the system"
/// (`[[source: macos-man-launchd]]`).
#[cfg(target_os = "macos")]
const LAUNCHD: u32 = 1;

impl Parent {
    /// **This process's parent, read once, in the parent, at the spawn**:
    /// launchd when `getppid()` is 1 -- Ghostty's `getppid()` test on macOS
    /// (`[[source: ghostty-d9840f3-desktop]]`). An app `open` starts has
    /// parent 1, and one run from a shell has that shell: measured
    /// twice, with two probe apps each run both ways; the captures are not
    /// in this repository.
    ///
    /// One misreading is known and kept, as Ghostty keeps it: an exiting
    /// process's children are given to "an implementation-defined system
    /// process" (`[[source: posix-exit]]`), on macOS launchd
    /// (`a_shell_spawned_under_launchd_starts_home`'s orphan arm), so
    /// a baud started from a shell that exited before the spawn reads as
    /// launchd's too, and its shell starts home.
    #[cfg(target_os = "macos")]
    fn of_this_process() -> Parent {
        match std::os::unix::process::parent_id() {
            LAUNCHD => Parent::Launchd,
            _ => Parent::Other,
        }
    }

    /// Not on Linux, where process 1 is init and launchd does not exist:
    /// the shell starts where baud is.
    #[cfg(not(target_os = "macos"))]
    fn of_this_process() -> Parent {
        Parent::Other
    }
}

/// **Where the child starts.**
#[derive(Debug, PartialEq, Eq)]
enum StartIn {
    /// Where baud is: the exec keeps the working directory
    /// (`[[source: posix-exec]]`), and the child changes nothing.
    Inherited,
    /// A directory changed to in the child before the exec: `HOME`, or
    /// the directory a new window inherits (`windows::inherited`).
    Dir(CString),
}

/// **Where a shell baud spawns starts**: in `HOME` when baud's parent is
/// launchd, and otherwise where baud is -- so a Dock or Finder launch,
/// whose working directory is `/`, starts in the home directory, and a
/// launch from a shell starts in that shell's directory. `HOME` is the
/// first in `parent_env`, which is the one `getenv` would read, and is
/// used only when it is absolute: an empty one names nothing, and a
/// relative one would be read against `/`, so either leaves the child where
/// baud is. Decided in the parent, before the fork, so the child calls
/// `chdir` on a string built already.
fn start_in(started_by: Parent, parent_env: &[(OsString, OsString)]) -> StartIn {
    match started_by {
        Parent::Other => StartIn::Inherited,
        Parent::Launchd => parent_env
            .iter()
            .find(|(k, _)| k.as_bytes() == b"HOME")
            .map(|(_, v)| v.as_bytes())
            .filter(|v| v.first() == Some(&b'/'))
            .and_then(|v| CString::new(v).ok())
            .map_or(StartIn::Inherited, StartIn::Dir),
    }
}

/// **The directory the shell starts in**, [`start_in`]'s answer for this
/// process: `HOME` on a Dock or Finder launch, else baud's own working
/// directory. A program's log with no name, or a relative one, goes here
/// (`hostlog.rs`), from the shell's point of view at its start.
pub fn start_dir() -> std::path::PathBuf {
    let env: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    match start_in(Parent::of_this_process(), &env) {
        StartIn::Dir(home) => {
            std::path::PathBuf::from(std::ffi::OsStr::from_bytes(home.as_bytes()))
        }
        StartIn::Inherited => {
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
        }
    }
}

/// **The child's `PWD` names where it starts.** `PWD` "shall represent an
/// absolute pathname of the current working directory"
/// (`[[source: posix-xbd-8]]`). A child sent home would inherit a `PWD`
/// naming somewhere else -- `open` run from a shell passes that shell's on
/// to the app it starts, while the app's directory is `/`
/// (measured with `open -n`) -- so it is replaced with `HOME`. A
/// child that stays keeps its environment as it is.
fn with_pwd(mut env: ChildEnv, start: &StartIn) -> ChildEnv {
    if let StartIn::Dir(dir) = start {
        env.set(&PWD, OsStr::from_bytes(dir.as_bytes()));
    }
    env
}

/// **`IUTF8` follows the input encoding after the spawn.** Under `ESC % @`
/// the keyboard sends ISO 8859-1, one byte a character (`Term::key`), and
/// with `IUTF8` still set the line discipline's erase reads a byte
/// `80`-`BF` as a continuation and takes the character before it too: `a £`
/// then Backspace left an empty line, where ISO 8859-1 leaves `a`. So the
/// parent clears the bit when the core's encoding becomes ISO 8859-1 and
/// sets it when it becomes UTF-8 again, by whatever changed it -- `ESC % @`,
/// and `ESC % G` or `RIS`, which returns the core to UTF-8.
///
/// **Through the subsidiary, not the manager.** `tcgetattr` takes "an open
/// file descriptor associated with a terminal" (`[[source:
/// posix-tcgetattr]]`), which the subsidiary is; no text read says the
/// manager is one. So the parent opens the path `ptsname` gave at the
/// spawn, reads the attributes, changes one bit, writes them back and
/// closes it before returning: it never holds the subsidiary, so the
/// child's last close is still the one that hangs the manager up. Opened
/// with `O_NOCTTY`, so the parent never takes it as its controlling
/// terminal, and `O_NONBLOCK`, so the open "shall return without blocking
/// for the device to be ready or available" (`[[source: posix-open]]`).
/// The parent runs none of this between `fork` and `exec`, so it may
/// allocate, and it says a failure rather than swallowing it.
pub struct LineDiscipline {
    /// The subsidiary's path, from `ptsname` at the spawn; `None` if it
    /// gave none, and then no change can reach the line discipline.
    subsidiary: Option<std::path::PathBuf>,
    /// The encoding the bit was last set for: UTF-8 at the spawn, because
    /// the child set it ([`utf8_erase`]).
    told: InputEncoding,
}

/// What [`LineDiscipline::follow`] did.
#[derive(Debug)]
pub enum Followed {
    /// The encoding is the one the bit was last set for.
    Same,
    /// The bit now says the new encoding.
    Told,
    /// The change did not reach the line discipline, and why.
    Failed(std::io::Error),
}

impl LineDiscipline {
    /// The line discipline of the pty whose manager is `master`, the bit as
    /// the child set it.
    fn of(master: crate::sys::Fd) -> LineDiscipline {
        LineDiscipline {
            subsidiary: subsidiary_of(master),
            told: InputEncoding::Utf8,
        }
    }

    /// Sets `IUTF8` for `enc` when `enc` is not what it was last set for,
    /// and does nothing when it is. The encoding counts as told even when
    /// the change fails, so a failure is said once, not once a read.
    pub fn follow(&mut self, enc: InputEncoding) -> Followed {
        if enc == self.told {
            return Followed::Same;
        }
        self.told = enc;
        let Some(path) = &self.subsidiary else {
            return Followed::Failed(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "ptsname named no subsidiary at the spawn",
            ));
        };
        match utf8_erase_at(path, enc) {
            Ok(()) => Followed::Told,
            Err(e) => Followed::Failed(e),
        }
    }
}

/// The subsidiary's path for the manager `master`, or `None` if
/// `ptsname` gives none.
///
/// **Under a lock**, because `ptsname` answers in a buffer of libc's that
/// the next call overwrites, and two spawns -- or two tests, which
/// `cargo test` runs as threads of one process -- could otherwise read
/// each other's name. The lock covers every call baud makes; a library in
/// this process calling `ptsname` at the same moment is not covered, and
/// none in `baud-app`'s dependency tree does.
fn subsidiary_of(master: crate::sys::Fd) -> Option<std::path::PathBuf> {
    static PTSNAME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = PTSNAME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // SAFETY: `master` is a descriptor; the answer is null or a string
    // libc keeps until the next call, which the lock holds off until the
    // copy below is made.
    let name = unsafe { crate::sys::ptsname(master) };
    if name.is_null() {
        return None;
    }
    // SAFETY: non-null, so a NUL-terminated string, read before the guard
    // drops.
    let name = unsafe { std::ffi::CStr::from_ptr(name) };
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
        name.to_bytes(),
    )))
}

/// `IUTF8` set on the terminal at `subsidiary` for UTF-8 and cleared for
/// ISO 8859-1, every other attribute as `tcgetattr` read it; the
/// descriptor is closed before this returns, whatever happened.
fn utf8_erase_at(subsidiary: &std::path::Path, enc: InputEncoding) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(crate::sys::O_NOCTTY | crate::sys::O_NONBLOCK)
        .open(subsidiary)?;
    let fd = file.as_raw_fd();
    let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
    // SAFETY: a termios-sized, termios-aligned block, which the call fills
    // when it succeeds.
    if unsafe { crate::sys::tcgetattr(fd, t.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the call above succeeded, so the block is initialised.
    let mut t = unsafe { t.assume_init() };
    match enc {
        InputEncoding::Utf8 => t.c_iflag |= crate::sys::IUTF8,
        InputEncoding::Latin1 => t.c_iflag &= !crate::sys::IUTF8,
    }
    // SAFETY: `t` is what `tcgetattr` read on `fd`, one bit changed.
    if unsafe { crate::sys::tcsetattr(fd, crate::sys::TCSANOW, &t) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// **A forked child's first act is libSystem's, and a thread can be in the
/// middle of what it reads.** On macOS `fork` runs libnotify's handler in
/// the child, which reads the library's globals through a once; when
/// another thread of this process was making the process's first libnotify
/// call at the moment of the fork, the child waits on a once whose owner
/// did not cross the fork and is killed before it reaches `execve` -- the
/// shell never starts and the master reads nothing. `std::env::temp_dir()`
/// with `TMPDIR` unset is one such first call. Making one here, in the
/// parent, settles the once before every fork, or waits for the thread
/// that is settling it. Elsewhere `fork` runs no such handler.
fn settle_before_fork() {
    // SAFETY: the token check takes any int and reads libnotify's table
    // under its own lock.
    #[cfg(target_os = "macos")]
    unsafe {
        crate::sys::notify_is_valid_token(0);
    }
}

/// How many times a pty open is made before its failure stands. Each
/// `EREDRIVEOPEN` means another opener took the terminal first, and the
/// kernel has already redone the open ten times; measured with 64 threads
/// opening at once, no open took more than 4 calls. Each call that loses
/// waits out the kernel's sleeps, about 0.4 s.
const REDRIVES: usize = 16;

/// `open` -- the manager's `posix_openpt` -- made again, at once, while
/// it fails with [`crate::sys::EREDRIVEOPEN`]: the kernel's redrive
/// continued past its own ten, as its comment has the value mean, "we've
/// been raced" (`[[source: xnu-tty-ptmx-c]]`). The race is system-wide: four
/// processes of eight threads each lost it where one alone never did. No
/// wait of baud's own: the kernel sleeps inside the call before its sixth
/// redo. It is `ptcopen`, the manager's open, that returns it
/// (`[[source: xnu-tty-dev-c]]`), and any other failure stands. Never
/// around a fork, which would fork again.
fn redriven(mut open: impl FnMut() -> core::ffi::c_int) -> core::ffi::c_int {
    let mut calls = 1;
    loop {
        let got = open();
        if got >= 0 || calls == REDRIVES || !raced() {
            return got;
        }
        calls += 1;
    }
}

/// Whether the last call lost the kernel's pty allocation race.
#[cfg(target_os = "macos")]
fn raced() -> bool {
    std::io::Error::last_os_error().raw_os_error() == Some(crate::sys::EREDRIVEOPEN)
}

/// Linux's pty open has no such race to lose.
#[cfg(not(target_os = "macos"))]
fn raced() -> bool {
    false
}

/// What a test runs on its own thread in the moment a spawn's terminal is
/// open in this process and its child is not yet forked: the master and
/// the replica. Taken out while it runs, so a spawn the hook makes runs
/// none.
#[cfg(test)]
pub(crate) type Opened = Box<dyn FnMut(crate::sys::Fd, crate::sys::Fd)>;

#[cfg(test)]
thread_local! {
    static OPENED: core::cell::RefCell<Option<Opened>> = const { core::cell::RefCell::new(None) };
}

/// Sets this thread's [`OPENED`] hook, or clears it.
#[cfg(test)]
pub(crate) fn set_opened(hook: Option<Opened>) {
    OPENED.with(|h| *h.borrow_mut() = hook);
}

/// Runs this thread's [`OPENED`] hook, if a test set one.
#[cfg(test)]
fn opened(master: crate::sys::Fd, replica: crate::sys::Fd) {
    let hook = OPENED.with(|h| h.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook(master, replica);
        OPENED.with(|h| *h.borrow_mut() = Some(hook));
    }
}

/// **A fresh pty of size `ws`, both ends close-on-exec from the calls that
/// open them**: the manager by `posix_openpt` with `O_CLOEXEC`, made again
/// while it loses the kernel's race ([`redriven`]), then `grantpt`,
/// `unlockpt` and the subsidiary opened by its name through
/// [`std::fs::OpenOptions`], which opens close-on-exec, with `O_NOCTTY`,
/// and given the size, as `openpty` gives it
/// (`[[source: apple-libc-pty-c]]`, `[[source: glibc-2-42-openpty-c]]`).
///
/// **Not libc's `openpty`**: it opens both ends without close-on-exec, and
/// `forkpty` closes the parent's replica only after it forks, so a child
/// another thread of this process forks in between holds the ends after
/// its `exec`, a pty's master and replica held by an unrelated program.
/// Then closing a master hangs nothing up, and a replica's master never
/// reads its end. POSIX names the race on `posix_openpt`: "a file
/// descriptor intentionally inherited by child processes is similarly
/// leaked into an executed program if FD_CLOEXEC is not set atomically"
/// (`[[source: posix-posix-openpt]]`). Two vim tests deadlocked on it,
/// each child holding the other's master.
fn open_terminal(
    ws: crate::sys::Winsize,
) -> Result<(std::os::fd::OwnedFd, std::os::fd::OwnedFd), NoTerminal> {
    use std::os::fd::FromRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let flags = crate::sys::O_RDWR | crate::sys::O_NOCTTY | crate::sys::O_CLOEXEC;
    #[cfg(test)]
    let opening = budget::opening();
    let m = redriven(|| open_manager(flags));
    if m < 0 {
        // Read at once: the limit's counts are of this moment, and no
        // call between may change `errno`. Not tried again: a refusal is
        // not a lost race, which `redriven` alone retries.
        return Err(NoTerminal::of(crate::sys::Errno::last(), pty_counts));
    }
    // SAFETY: the descriptor just opened, owned here alone.
    let master = unsafe { std::os::fd::OwnedFd::from_raw_fd(m) };
    // SAFETY: an open manager descriptor.
    if unsafe { crate::sys::grantpt(m) } != 0 || unsafe { crate::sys::unlockpt(m) } != 0 {
        return Err(NoTerminal::Failed(crate::sys::Errno::last()));
    }
    let name = subsidiary_of(m).ok_or_else(|| NoTerminal::Failed(crate::sys::Errno::last()))?;
    let replica: std::os::fd::OwnedFd = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(crate::sys::O_NOCTTY)
        .open(name)
        .map_err(|e| NoTerminal::Failed(crate::sys::Errno(e.raw_os_error().unwrap_or(0))))?
        .into();
    // SAFETY: an open terminal, and the size live for the call.
    unsafe {
        crate::sys::ioctl(replica.as_raw_fd(), crate::sys::TIOCSWINSZ, &raw const ws);
    }
    #[cfg(test)]
    {
        drop(opening);
        budget::opened();
    }
    Ok((master, replica))
}

/// **What a fork of a fresh terminal leaves in each process**: the child,
/// which holds the terminal as its standard input, output and error and
/// has no master to hand back, or the parent, with the child's pid and the
/// master, owned, so a caller that fails lets the pty go.
enum Fork {
    Child,
    Parent(core::ffi::c_int, std::os::fd::OwnedFd),
}

/// **A child on a fresh pty of size `ws`** ([`open_terminal`]), as each
/// process has it ([`Fork`]), or why there is none: the open's refusal,
/// or the fork's `errno`. libnotify
/// is settled before the fork ([`settle_before_fork`]). The child does what
/// `login_tty` does, a new session, the terminal taken as its controlling
/// terminal and made its standard input, output and error, and, when it
/// cannot take the terminal, the copies alone, as Apple's `forkpty` falls
/// back (`[[source: apple-libc-pty-c]]`, `[[source: apple-libc-login-tty-c]]`).
/// Its copies of both ends close at its `exec`. The parent closes its
/// replica, the fork failed or not.
fn fork_terminal(ws: crate::sys::Winsize) -> Result<Fork, NoTerminal> {
    let (master, replica) = open_terminal(ws)?;
    // The child's bound, when it cannot list its own descriptors, looked
    // up here, where looking may allocate (`inherited.rs`).
    let highest = crate::inherited::Highest::scanned();
    let room = crate::inherited::list_room();
    #[cfg(test)]
    opened(master.as_raw_fd(), replica.as_raw_fd());
    settle_before_fork();
    // SAFETY: no preconditions; the child below calls only what POSIX
    // allows a child of a multi-threaded process before its `exec`.
    let pid = unsafe { crate::sys::fork() };
    if pid == 0 {
        let r = replica.as_raw_fd();
        // SAFETY: in the child, `setsid`, `ioctl`, `dup2`, `fcntl` and
        // `close` on descriptors it holds, `signal`, `sigprocmask`,
        // `getpid`, and nothing that allocates. POSIX has "the child process
        // only executes async-signal-safe operations until such time as one
        // of the exec functions is successful" (`[[source: posix-fork]]`),
        // and XSH 2.4.3's table lists `setsid`, `dup2`, `fcntl`, `close`,
        // `signal`, `sigprocmask` and `getpid` (`[[source:
        // posix-xsh-chap02]]`). `ioctl` is not on it; `TIOCSCTTY` is the
        // call `login_tty` makes in `forkpty`'s child on both platforms,
        // which every spawn made before. Nor is macOS's `proc_pidinfo`,
        // one `__proc_info` system call with no allocation or lock
        // (`[[source: xnu-libproc-c]]` 101-110), the same kind of call; nor
        // Linux's `syscall`, which makes `close_range` by its number.
        unsafe {
            crate::sys::setsid();
            crate::sys::ioctl(
                r,
                crate::sys::TIOCSCTTY,
                core::ptr::null::<core::ffi::c_void>(),
            );
            for fd in 0..3 {
                crate::sys::dup2(r, fd);
                // A replica numbered 0, 1 or 2 is not copied onto itself
                // and keeps its close-on-exec; the others never have it.
                crate::sys::fcntl(fd, crate::sys::F_SETFD, 0);
            }
            // **Every signal at its default action, and none blocked**,
            // whatever baud inherited, as every program baud starts begins
            // (`sys::child_signals`, which says why). Without it every shell
            // and what it ran would ignore `SIGPIPE`, as baud does.
            crate::sys::child_signals();
            // **Then nothing of baud's but the terminal.** A descriptor open
            // without close-on-exec at the fork -- one a library on another
            // thread held for a moment -- would cross the `exec` and stay
            // with the program for good, so every one from 3 is closed
            // here (`inherited.rs`). Its terminal's copies on 0, 1 and 2
            // are kept, and the replica and master, which close at the
            // `exec` anyway, go now.
            crate::inherited::close_from_three(highest, room);
        }
        // Closed at the `exec`, close-on-exec, and not here.
        core::mem::forget(replica);
        core::mem::forget(master);
        return Ok(Fork::Child);
    }
    let forked = crate::sys::Errno::last();
    drop(replica);
    if pid < 0 {
        return Err(NoTerminal::Failed(forked));
    }
    Ok(Fork::Parent(pid, master))
}

impl Pty {
    /// Forks a login shell -- `[shell] program`, else `$SHELL`, else the
    /// system's -- on a fresh
    /// pty of `size`, and returns the master. With `BAUD_CMD` set, forks
    /// `/bin/sh -c "$BAUD_CMD"` instead, which is how a benchmark drives
    /// the terminal: the window closes when the command ends.
    ///
    /// **The pixels go in at the fork**, not at the first resize: a program
    /// that asks the kernel how big a cell is before the window has settled
    /// -- yazi does, at startup -- would otherwise read zero, and the first
    /// resize after launch usually computes the grid the pty was spawned at,
    /// so it had nothing to send and the zero stood until the window changed.
    ///
    /// `at` is the directory a new window inherits, which wins over the
    /// launch rule ([`start_in`]) when it is absolute. `shell` is
    /// `[shell]`: its `integration`, which marks the shell's prompts
    /// (`shell.rs`), and its `program`.
    fn fork(
        ws: crate::sys::Winsize,
        at: Option<&std::path::Path>,
        shell: &ShellStart,
    ) -> Option<Result<Forked, NoTerminal>> {
        let cmd = std::env::var("BAUD_CMD").ok();
        Pty::fork_given(
            ws,
            cmd,
            std::env::vars_os().collect(),
            shell_locale,
            Parent::of_this_process(),
            at,
            shell,
        )
    }

    /// [`Pty::fork`] with every input given but the integration, which is
    /// off: the command, the parent's environment, where a locale comes
    /// from when the parent names none, and who started baud -- so a test
    /// runs the whole spawn, the locale and the start directory included.
    #[cfg(test)]
    fn fork_from(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        parent: Vec<(OsString, OsString)>,
        locale: impl FnOnce() -> Option<String>,
        started_by: Parent,
        at: Option<&std::path::Path>,
    ) -> Option<std::os::fd::OwnedFd> {
        Pty::fork_given(
            ws,
            cmd,
            parent,
            locale,
            started_by,
            at,
            &crate::shell::Integration::Off.into(),
        )
        .and_then(Result::ok)
        .map(|(_, master, _)| master)
    }

    /// [`Pty::fork`] with every input given, `[shell]` too.
    fn fork_given(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        parent: Vec<(OsString, OsString)>,
        locale: impl FnOnce() -> Option<String>,
        started_by: Parent,
        at: Option<&std::path::Path>,
        shell: &ShellStart,
    ) -> Option<Result<Forked, NoTerminal>> {
        let start = match at
            .filter(|d| d.is_absolute())
            .and_then(|d| CString::new(d.as_os_str().as_bytes()).ok())
        {
            Some(dir) => StartIn::Dir(dir),
            None => start_in(started_by, &parent),
        };
        let (exec, stay) = Pty::exec_and_env(cmd, parent, locale, shell)?;
        let home = with_pwd(stay.clone(), &start);
        let token = exec.injection.as_ref().and_then(|i| i.token.clone());
        Some(
            Pty::fork_in(ws, &exec, &home.into_envp(), &stay.into_envp(), &start)
                .map(|(child, master)| (child, master, token)),
        )
    }

    /// What the child runs, and its environment when it stays where baud
    /// is.
    fn exec_and_env(
        cmd: Option<String>,
        parent: Vec<(OsString, OsString)>,
        locale: impl FnOnce() -> Option<String>,
        shell: &ShellStart,
    ) -> Option<(Exec, ChildEnv)> {
        let exec = match &shell.run {
            Some(run) => Exec::running(run, shell.program.as_deref(), &parent)?,
            None => Exec::of(
                cmd,
                &parent,
                shell.integration,
                shell.ssh_env,
                shell.program.as_deref(),
            )?,
        };
        let mut stay = spawn_env(parent, locale);
        if let Some(v) = &exec.shell_var {
            stay.set(&SHELL, v.clone());
        }
        if let Some(i) = &exec.injection {
            i.apply(&mut stay);
        }
        Some((exec, stay))
    }

    /// [`Pty::fork_in`] where baud is, for the tests that hand the child a
    /// parent environment of their own and read back what it saw or the
    /// terminal it was given.
    #[cfg(test)]
    pub(crate) fn fork_with(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        env: Vec<CString>,
    ) -> Option<std::os::fd::OwnedFd> {
        Pty::fork_child(ws, cmd, env).map(|(_, master)| master)
    }

    /// [`Pty::fork_with`] with the child's pid beside the master, for a
    /// test that waits for that child: libtest runs the tests as threads of
    /// one process, so a wait for any child (`waitpid(-1)`) can take a
    /// child of a test running beside it, whose own wait then fails with
    /// `ECHILD`. The shell, when no command is given, is `$SHELL` as this
    /// process has it, started with integration off.
    #[cfg(test)]
    pub(crate) fn fork_child(
        ws: crate::sys::Winsize,
        cmd: Option<String>,
        env: Vec<CString>,
    ) -> Option<(core::ffi::c_int, std::os::fd::OwnedFd)> {
        let parent: Vec<(OsString, OsString)> = std::env::vars_os().collect();
        let exec = Exec::of(
            cmd,
            &parent,
            crate::shell::Integration::Off,
            crate::shell::SshEnv::Off,
            None,
        )?;
        Pty::fork_in(ws, &exec, &env, &env, &StartIn::Inherited).ok()
    }

    /// **baud's own start of a login shell, for a test**: `parent` as baud's
    /// environment, the shell it names started in `dir` with the shell
    /// integration on, as [`Pty::fork`] starts a window's -- the README
    /// session played without a window (`film::session`). The child's pid
    /// beside the master.
    ///
    /// **It is told no `PWD`, as the filmed window's shell is told none**:
    /// the demo launches baud in `dir` with no `PWD` in its environment, and
    /// baud under any parent but launchd leaves the child as it is
    /// ([`StartIn::Inherited`]), so the shell finds its own, the resolved
    /// path, and a `HOME` spelled through a symbolic link is not a prefix of
    /// it. Given `dir`'s spelling, as a new window is ([`with_pwd`]), the
    /// session's prompt read `~/baud` where the window's did not.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn fork_shell(
        ws: crate::sys::Winsize,
        mut parent: Vec<(OsString, OsString)>,
        dir: &std::path::Path,
    ) -> Option<(core::ffi::c_int, std::os::fd::OwnedFd)> {
        parent.retain(|(k, _)| k != "PWD");
        let (exec, env) =
            Pty::exec_and_env(None, parent, || None, &crate::shell::Integration::On.into())?;
        let env = env.into_envp();
        let start = StartIn::Dir(CString::new(dir.as_os_str().as_bytes()).ok()?);
        Pty::fork_in(ws, &exec, &env, &env, &start).ok()
    }

    /// [`Pty::fork`] with what the child runs ([`Exec`]), the environments
    /// and the start directory given rather than read: the child's pid and
    /// the master. `home` is the environment of a child whose `chdir` to
    /// `start`'s directory succeeded, and `stay` that of a child left where
    /// baud is -- because `start` said so, or because the `chdir` failed --
    /// so a `PWD` naming `HOME` reaches only a shell that is in it, as XBD 8
    /// has `PWD` name "the current working directory"
    /// (`[[source: posix-xbd-8]]`).
    fn fork_in(
        ws: crate::sys::Winsize,
        exec: &Exec,
        home: &[CString],
        stay: &[CString],
        start: &StartIn,
    ) -> Result<(core::ffi::c_int, std::os::fd::OwnedFd), NoTerminal> {
        let path_c = exec.path.as_c();
        let argv: Vec<*const core::ffi::c_char> = exec
            .argv
            .iter()
            .map(|a| a.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let envp_of = |env: &[CString]| -> Vec<*const core::ffi::c_char> {
            env.iter()
                .map(|e| e.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect()
        };
        let envp_home = envp_of(home);
        let envp_stay = envp_of(stay);
        // `fork_terminal` returns in the child having called only `fork`
        // and what `login_tty` calls; the redrive wraps the open alone.
        let Fork::Parent(pid, master) = fork_terminal(ws)? else {
            // SAFETY: in the child only `tcgetattr`, `tcsetattr`, `chdir`,
            // `execve` and `_exit` run, and a branch on `chdir`'s result,
            // over the child's stack and pointers into memory built before
            // the fork: after a multi-threaded process forks, POSIX has "the
            // child process only executes async-signal-safe operations until
            // such time as one of the exec functions is successful"
            // (`[[source: posix-fork]]`), and XSH 2.4.3's table lists all
            // five (`[[source: posix-xsh-chap02]]`). The rule was `execve`
            // and `_exit` alone until 2026-10-13, when it was widened for
            // the two attribute calls, and then for `chdir`, once the child
            // was to change directory.
            unsafe {
                utf8_erase(CHILD_TERMINAL);
                // A failed change leaves the directory as it was (`[[source:
                // posix-chdir]]`), and the shell still starts, as Ghostty's
                // does (`[[source: ghostty-d9840f3-command]]`), with the
                // environment of a child that stayed: the choice is a test
                // of the `int` `chdir` returns, and allocates nothing.
                let envp = match start {
                    StartIn::Dir(dir) if crate::sys::chdir(dir.as_ptr()) == 0 => &envp_home,
                    _ => &envp_stay,
                };
                // A program not found ends the child as a failed `execve`
                // does, without one: nothing is run from the folder it
                // moved to.
                if let Some(path) = path_c {
                    crate::sys::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                }
                crate::sys::_exit(127);
            }
        };
        // **The master closes on exec**, opened so ([`open_terminal`]): a
        // program this process starts -- the opener a link is handed to,
        // and what that opener starts -- never holds the terminal's other
        // end.
        Ok((pid, master))
    }

    /// The pty with a thread reading it: each chunk crosses a channel and
    /// `wake` runs once per batch, so a main thread that cannot wait on a
    /// descriptor -- the macOS run loop -- learns there is something to
    /// drain.
    ///
    /// `at` is the directory a new window's shell starts in (`fork`).
    #[cfg(target_os = "macos")]
    pub fn spawn(
        size: &WinSize,
        cell_w: Px,
        cell_h: Px,
        at: Option<&std::path::Path>,
        shell: &ShellStart,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        // A refused terminal still opens its window, over why (`Refused`).
        let (child, master, token) = match Pty::fork(told, at, shell)? {
            Ok(forked) => forked,
            Err(why) => return Some(Pty::Refused(Refused { why, told })),
        };
        // A link's program holds its window as it ends; a shell does not.
        let program = shell.run.as_ref().map(|r| r.program().to_os_string());
        Some(Pty::reading(
            master.into_raw_fd(),
            Some(child),
            told,
            program,
            token,
            wake,
        ))
    }

    /// The pty over `master`, a child's manager the fork told `told`,
    /// with the thread that reads it.
    #[cfg(target_os = "macos")]
    fn reading(
        master: crate::sys::Fd,
        child: Option<crate::sys::Pid>,
        told: crate::sys::Winsize,
        program: Option<OsString>,
        token: Option<crate::ran::ShellToken>,
        wake: impl Fn() + Send + 'static,
    ) -> Pty {
        // Before the reader starts, so it never sees the master block.
        make_nonblocking(master);
        let (tx, rx) = channel();
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        let (held, closing) = Reading::of(master);
        let watched = crate::probe::WatchCell::default();
        let taken = watched.clone();
        std::thread::spawn(move || reader(held, tx, flag, &taken, token.as_ref(), wake));
        Pty::Live(Live {
            master,
            child,
            told,
            line: LineDiscipline::of(master),
            rx,
            signalled,
            watched,
            closing,
            ending: ChildEnd::of(program),
        })
    }

    /// **The app's pty over a child a test names**: [`Pty::spawn`] with the
    /// command, the parent's environment and who started baud given rather
    /// than read from this process, the locale asked as a launch asks it
    /// ([`shell_locale`]), and the reader thread, which calls `wake` once a
    /// batch -- so a test runs a real program through the spawn a Dock
    /// launch runs.
    #[cfg(all(test, target_os = "macos"))]
    pub(crate) fn spawn_given(
        size: &WinSize,
        (cell_w, cell_h): (Px, Px),
        cmd: &str,
        parent: Vec<(OsString, OsString)>,
        started_by: Parent,
        wake: impl Fn() + Send + 'static,
    ) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let forked = Pty::fork_given(
            told,
            Some(cmd.into()),
            parent,
            shell_locale,
            started_by,
            None,
            &crate::shell::Integration::Off.into(),
        )?;
        Some(match forked {
            Ok((_, master, _)) => Pty::reading(master.into_raw_fd(), None, told, None, None, wake),
            Err(why) => Pty::Refused(Refused { why, told }),
        })
    }

    /// The pty with no thread: the caller waits on `as_fd` beside whatever
    /// else it waits on and takes each read with `read_chunk`, which is
    /// the Linux loop's shape -- one wait over the compositor's socket and
    /// the master, and nothing crossing a channel.
    #[cfg(target_os = "linux")]
    pub fn spawn_polled(size: &WinSize, cell_w: Px, cell_h: Px, shell: &ShellStart) -> Option<Pty> {
        let told = text_area(size, cell_w, cell_h);
        let (child, master, token) = match Pty::fork(told, None, shell)? {
            Ok(forked) => forked,
            Err(why) => return Some(Pty::Refused(Refused { why, told })),
        };
        let master = master.into_raw_fd();
        make_nonblocking(master);
        let program = shell.run.as_ref().map(|r| r.program().to_os_string());
        Some(Pty::polled(master, child, told, program, token))
    }

    /// The pty over `master` and its child `child`, polled by the caller:
    /// the child taken as a descriptor at once, so its end is an event in
    /// the same poll.
    #[cfg(target_os = "linux")]
    fn polled(
        master: crate::sys::Fd,
        child: crate::sys::Pid,
        told: crate::sys::Winsize,
        program: Option<OsString>,
        token: Option<crate::ran::ShellToken>,
    ) -> Pty {
        let pidfd = match pidfd_open(child) {
            Ok(fd) => Some(fd),
            Err(e) => {
                crate::info!(
                    "pty: no pidfd for the child ({e}); its end is asked at the hangup and once after"
                );
                None
            }
        };
        Pty::Live(Live {
            master,
            child: Some(child),
            told,
            line: LineDiscipline::of(master),
            ending: ChildEnd::of(program),
            pidfd,
            hung: crate::held::Stream::Open,
            again: None,
            token,
        })
    }
}

impl Live {
    /// The token baud handed the shell, none for one it does not mark.
    #[cfg(target_os = "linux")]
    pub fn shell_token(&self) -> Option<&crate::ran::ShellToken> {
        self.token.as_ref()
    }

    /// The master, to wait on.
    #[cfg(target_os = "linux")]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        // SAFETY: the master is open for as long as `self` is, and the
        // borrow cannot outlive `self`.
        unsafe { BorrowedFd::borrow_raw(self.master) }
    }

    /// **The tty's special characters**, which a paste replaces as xterm's
    /// `STTY` does (`paste::FILTER`): the `c_cc` entries
    /// [`crate::sys::STTY_CHARS`] names, as `tcgetattr` reads them on the
    /// manager; none when they cannot be read.
    pub fn specials(&self) -> crate::paste::Specials {
        let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
        // SAFETY: a termios-sized, termios-aligned block, which the call
        // fills when it succeeds.
        if unsafe { crate::sys::tcgetattr(self.master, t.as_mut_ptr()) } != 0 {
            return crate::paste::Specials::default();
        }
        // SAFETY: the call above succeeded, so the block is initialised.
        let t = unsafe { t.assume_init() };
        crate::paste::Specials::of(
            &t.c_cc,
            crate::sys::STTY_CHARS,
            crate::paste::Disabled(crate::sys::POSIX_VDISABLE),
        )
    }

    /// The tty's literal-next character, which a `?2005` paste puts
    /// before each byte: `c_cc[VLNEXT]` as `tcgetattr` reads it on the
    /// manager, or Control-V when it cannot be read or is switched off, as
    /// xterm's `get_tty_lnext` falls back to `XTERM_LNEXT`.
    pub fn literal_next(&self) -> crate::readline::Quote {
        let mut t = core::mem::MaybeUninit::<crate::sys::Termios>::uninit();
        // SAFETY: a termios-sized, termios-aligned block, which the call
        // fills when it succeeds.
        if unsafe { crate::sys::tcgetattr(self.master, t.as_mut_ptr()) } != 0 {
            return crate::readline::Quote::LNEXT;
        }
        // SAFETY: the call above succeeded, so the block is initialised.
        let t = unsafe { t.assume_init() };
        match t.c_cc[crate::sys::VLNEXT] {
            crate::sys::POSIX_VDISABLE => crate::readline::Quote::LNEXT,
            b => crate::readline::Quote(b),
        }
    }

    /// One read of the master, taken after a wait said it was ready: at
    /// most `READ_CHUNK` bytes, empty when a signal interrupted the read or
    /// the wait woke for room to write rather than for bytes, and `Closed`
    /// once the child has gone -- a Linux master answers a read with an
    /// error then, not with zero bytes.
    #[cfg(target_os = "linux")]
    pub fn read_chunk(&self) -> Chunk {
        let mut buf = vec![0u8; READ_CHUNK];
        match read_master(self.master, &mut buf) {
            Got::Bytes(b) => {
                let n = b.len();
                buf.truncate(n);
                Chunk::Bytes(buf)
            }
            Got::Nothing => Chunk::Bytes(Vec::new()),
            Got::Closed => Chunk::Closed,
        }
    }

    /// Chunks the shell wrote since the last call, in order.
    #[cfg(target_os = "macos")]
    pub fn drain(&self) -> Drained {
        // Cleared before draining, so a chunk that lands after the drain
        // begins earns its own wake.
        self.signalled.store(false, Ordering::Release);
        let mut chunks = Vec::new();
        let stream = loop {
            match self.rx.try_recv() {
                Ok(chunk) => chunks.push(chunk),
                Err(std::sync::mpsc::TryRecvError::Empty) => break Stream::Open,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break Stream::Closed,
            }
        };
        Drained { chunks, stream }
    }

    /// Writes what `out` owes the shell, as much as the kernel takes now,
    /// and says whether all of it went. **It never waits**: what the slave's
    /// input queue cannot hold stays in `out`, first in line, and the caller
    /// offers it again on its next turn. A write that left bytes owed asks
    /// for no echo, and the keystroke instrument must not wait for one.
    pub fn write(&self, out: &mut Outbox) -> Wrote {
        write_some(self.master, out)
    }

    /// Tells the kernel, and through it the shell, the geometry in cells
    /// and the text area in pixels -- **the cell size, not the window's**, so
    /// a caller cannot hand it the padding; see [`text_area`].
    ///
    /// Called on every re-lay, and a no-op when nothing moved: a zoom that
    /// keeps the grid still changes the pixels and is sent, a drag step that
    /// lands on the same grid and cell is not, so the shell is not woken
    /// for nothing.
    pub fn resize(&mut self, size: &WinSize, cell_w: Px, cell_h: Px) {
        tell(self.master, &mut self.told, text_area(size, cell_w, cell_h));
    }

    /// **The session's root process**, as the badge's `pid` names it:
    /// "The process ID of the root process in this session"
    /// (`[[source: iterm2-doc-variables]]`).
    pub fn child(&self) -> Option<crate::sys::Pid> {
        self.child
    }

    /// **What the window does about its child's end** (`held::decide`):
    /// waits until `stream` has closed and the child has been reaped, in
    /// either order, then closes or holds by how it ended.
    pub fn decide(&self, stream: crate::held::Stream) -> crate::held::Decided {
        let e = &self.ending;
        crate::held::decide(e.program.as_deref(), e.born, stream, e.slot.get())
    }

    /// **The window's child, for its watch to reap** (`fg::Watch`); none for
    /// a pty over a child a test forked itself.
    #[cfg(target_os = "macos")]
    pub fn reaps(&self) -> Option<crate::fg::Child> {
        Some(crate::fg::Child {
            pid: self.child?,
            slot: self.ending.slot.clone(),
        })
    }

    /// **Where the window's process watch leaves what it saw** for the
    /// reader to take (`fg::Watch`, `probe::WatchCell`).
    #[cfg(target_os = "macos")]
    pub fn watched(&self) -> crate::probe::WatchCell {
        self.watched.clone()
    }

    /// **The master to poll**, until it hangs up ([`Pty::hung_up`]): a
    /// hung-up master answers every poll at once, and would spin the loop.
    #[cfg(target_os = "linux")]
    pub fn polled_master(&self) -> Option<BorrowedFd<'_>> {
        match self.hung {
            crate::held::Stream::Open => Some(self.as_fd()),
            crate::held::Stream::Closed => None,
        }
    }

    /// The read said the child has gone: the master leaves the poll, and
    /// the child is asked after at once, for a kernel with no pidfd.
    #[cfg(target_os = "linux")]
    pub fn hung_up(&mut self) {
        self.hung = crate::held::Stream::Closed;
        if self.pidfd.is_none() {
            self.reap();
            if self.ending.slot.get().is_none() {
                self.again = Some(std::time::Instant::now() + REAP_AGAIN);
            }
        }
    }

    /// Whether the master has hung up.
    #[cfg(target_os = "linux")]
    pub fn stream(&self) -> crate::held::Stream {
        self.hung
    }

    /// **The child's descriptor to poll**, while it has not been reaped.
    #[cfg(target_os = "linux")]
    pub fn child_fd(&self) -> Option<BorrowedFd<'_>> {
        use std::os::fd::AsFd;
        match self.ending.slot.get() {
            Some(_) => None,
            None => self.pidfd.as_ref().map(AsFd::as_fd),
        }
    }

    /// **The child reaped, if it has ended**: on its pidfd's readiness, and
    /// with no pidfd at the hangup and once after ([`Pty::reap_owed`]). The
    /// pidfd closes once it is reaped, which it would otherwise answer on
    /// every poll after.
    #[cfg(target_os = "linux")]
    pub fn reap(&mut self) {
        let Some(pid) = self.child else {
            return;
        };
        if reap_into(pid, &self.ending.slot) == Recorded::Now {
            self.pidfd = None;
        }
    }

    /// **When the loop owes the child one more look**: with no pidfd, a
    /// hung-up master's child not yet reaped is asked once more
    /// [`REAP_AGAIN`] after the hangup's look, and if it is still running
    /// then the window closes without its status ([`Pty::give_up`]).
    #[cfg(target_os = "linux")]
    pub fn reap_owed(&self) -> Option<std::time::Instant> {
        self.again.filter(|_| self.ending.slot.get().is_none())
    }

    /// **The look after [`REAP_AGAIN`], with no pidfd**: reaped if it has
    /// ended; if not, the window stops waiting, with no status, and closes
    /// as a shell's does. Its process ends with the window's, and the
    /// child is then the system's to reap.
    #[cfg(target_os = "linux")]
    pub fn give_up(&mut self) {
        self.again = None;
        self.reap();
        self.ending.slot.abandon();
    }

    /// **The pty's device**, as the badge's `tty` names it: "The path to
    /// the local TTY device", `ptsname`'s answer at the spawn.
    pub fn tty(&self) -> Option<&std::path::Path> {
        self.line.subsidiary.as_deref()
    }

    /// What the derived title reads of the foreground (`fg.rs`).
    pub fn reads(&self) -> crate::fg::Live {
        crate::fg::Live {
            master: Some(self.master),
        }
    }

    /// Keeps `IUTF8` with the core's input encoding ([`LineDiscipline`]).
    /// Called after every feed and before any key is encoded, so a key
    /// `Term::key` sends as ISO 8859-1 reaches a line discipline with the
    /// bit clear. A change that fails is said once.
    pub fn follow(&mut self, enc: InputEncoding) {
        if let Followed::Failed(e) = self.line.follow(enc) {
            crate::warn!("the pty's IUTF8 stayed as it was for {enc:?}: {e}");
        }
    }
}

/// **What the reader holds of the master.** A copy of its own and the
/// read end of a pipe whose write end the window's [`Pty`] holds, so the
/// window can close without closing a descriptor the reader is waiting
/// on: a closed number can be handed to the next window's master, and the
/// reader would read that window's bytes. The pipe's close is the
/// reader's end, and the copy closes with it.
#[cfg(target_os = "macos")]
enum Reading {
    Own {
        copy: std::os::fd::OwnedFd,
        closing: std::io::PipeReader,
    },
    /// The window's own descriptor, when no copy or pipe could be made:
    /// it is never closed, as every master was before windows could close.
    Shared(crate::sys::Fd),
}

#[cfg(target_os = "macos")]
impl Reading {
    /// The reader's hold on `master`, and the write end the window keeps.
    fn of(master: crate::sys::Fd) -> (Reading, Option<std::io::PipeWriter>) {
        // SAFETY: open for as long as the window's `Pty`, which outlives
        // this call; the copy is a descriptor of its own.
        let copy = unsafe { std::os::fd::BorrowedFd::borrow_raw(master) }.try_clone_to_owned();
        match (copy, std::io::pipe()) {
            (Ok(copy), Ok((closing, close))) => (Reading::Own { copy, closing }, Some(close)),
            (Err(e), _) | (_, Err(e)) => {
                crate::warn!("pty: the master stays open until baud quits: {e}");
                (Reading::Shared(master), None)
            }
        }
    }

    fn fd(&self) -> crate::sys::Fd {
        match self {
            Reading::Own { copy, .. } => copy.as_raw_fd(),
            Reading::Shared(fd) => *fd,
        }
    }

    fn closing(&self) -> Option<crate::sys::Fd> {
        match self {
            Reading::Own { closing, .. } => Some(closing.as_raw_fd()),
            Reading::Shared(_) => None,
        }
    }
}

/// **How long after a hung-up master's look its child is looked for once
/// more, on a kernel with no pidfd** ([`Pty::reap_owed`]): a child that
/// closed its terminal on its way out has usually ended by then; one that
/// has not is left to the system once the window closes.
#[cfg(target_os = "linux")]
pub const REAP_AGAIN: Duration = Duration::from_millis(100);

/// **`pid` as a descriptor** (`pidfd_open(2)`): "the close-on-exec flag
/// is set on the file descriptor" (`[[source: linux-man-pidfd-open]]` 32),
/// so no program a link starts holds it; `ENOSYS` before 5.3, or where a
/// filter refuses the call.
#[cfg(target_os = "linux")]
fn pidfd_open(pid: crate::sys::Pid) -> std::io::Result<std::os::fd::OwnedFd> {
    let flags: core::ffi::c_uint = 0;
    // SAFETY: the call's two arguments, a pid and its flags, in the types
    // the kernel reads; it returns a new descriptor or -1.
    let fd = unsafe { crate::sys::syscall(crate::sys::SYS_PIDFD_OPEN, pid, flags) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let fd = crate::sys::Fd::try_from(fd).map_err(|_| std::io::Error::other("pidfd"))?;
    // SAFETY: a descriptor the call just made, owned here alone.
    Ok(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(fd) })
}

/// Whether a child's end is known yet ([`reap_now`]).
#[derive(Clone, Copy, Debug)]
enum Reap {
    /// It has not ended, or has not yet become a zombie to reap.
    Running,
    Reaped(crate::held::Reaped),
}

/// Whether a call put the child's end in its slot ([`reap_into`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Recorded {
    /// It was already there, or the child has not ended.
    Not,
    Now,
}

/// **`pid` reaped into `slot`, if it has ended and is not there yet**
/// ([`reap_now`]): the one way a slot learns a child's end from a wait,
/// called by the window's watch on macOS (`fg::Watch`) and by the Linux
/// loop.
pub(crate) fn reap_into(pid: crate::sys::Pid, slot: &Slot) -> Recorded {
    if slot.get().is_some() {
        return Recorded::Not;
    }
    match reap_now(pid) {
        Reap::Running => Recorded::Not,
        Reap::Reaped(r) => match slot.0.set(r) {
            Ok(()) => Recorded::Now,
            Err(_) => Recorded::Not,
        },
    }
}

/// **`pid` reaped, if it has ended**: `waitpid` on that pid alone with
/// [`crate::sys::WNOHANG`], which answers at once, asked only once an event
/// says the child ended -- the window's watch on macOS (`fg::Watch`), the
/// child's pidfd in the Linux loop's poll. Again when a signal cuts it
/// short ([`waited`]).
fn reap_now(pid: crate::sys::Pid) -> Reap {
    let mut status: core::ffi::c_int = 0;
    loop {
        // SAFETY: `status` is a valid out-pointer for the call's length.
        let got = unsafe { crate::sys::waitpid(pid, &raw mut status, crate::sys::WNOHANG) };
        let errno = (got < 0).then(crate::sys::Errno::last);
        if let Some(r) = waited(pid, got, status, errno) {
            return r;
        }
    }
}

/// **What one `waitpid(pid, .., WNOHANG)` answered**: `got`, the status it
/// wrote and, when it failed, its errno. None for `EINTR`, to be asked
/// again. `ECHILD` is the only failure that means the pid is no child of
/// this process to wait for -- one something else reaped -- and any other
/// is kept as itself: POSIX leaves only `EINVAL` for a pid above 0, which
/// would be a fault in baud and not a reaped child.
fn waited(
    pid: crate::sys::Pid,
    got: core::ffi::c_int,
    status: core::ffi::c_int,
    errno: Option<crate::sys::Errno>,
) -> Option<Reap> {
    use crate::held::Ending;
    use std::os::unix::process::ExitStatusExt;
    let ending = match (got, errno) {
        (got, _) if got == pid => Ending::Exited(std::process::ExitStatus::from_raw(status)),
        (0, _) => return Some(Reap::Running),
        (_, Some(e))
            if std::io::Error::from_raw_os_error(e.0).kind() == std::io::ErrorKind::Interrupted =>
        {
            return None;
        }
        (_, Some(crate::sys::Errno(crate::sys::ECHILD))) => Ending::NotOurs,
        (_, e) => Ending::WaitFailed(e.unwrap_or(crate::sys::Errno(0))),
    };
    let at = std::time::Instant::now();
    Some(Reap::Reaped(crate::held::Reaped { ending, at }))
}

#[cfg(target_os = "macos")]
fn reader(
    held: Reading,
    tx: Sender<Read>,
    signalled: Arc<AtomicBool>,
    watched: &crate::probe::WatchCell,
    token: Option<&crate::ran::ShellToken>,
    wake: impl Fn(),
) {
    let fd = held.fd();
    let mut buf = vec![0u8; READ_CHUNK];
    let mut sampler = crate::probe::Sampler::for_shell(token);
    // Debug tee: with BAUD_PTYLOG set, every byte the child writes is appended
    // to that file, so a scrambled Sixel can be captured as the exact input
    // baud received and replayed through a decoder test.
    let mut log = std::env::var_os("BAUD_PTYLOG").and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    loop {
        let bytes = match read_master(fd, &mut buf) {
            Got::Bytes(b) => b,
            // **The wait is here, in `poll`, not in the read.** The master is
            // non-blocking for the main thread's writes, and the flag is the
            // open file's, so this read returns at once with nothing. Read as
            // the end of the stream -- the rule every empty answer had while
            // the master blocked -- it would end the reader, and with it the
            // window, the first time the child paused.
            Got::Nothing => match readable_unless(fd, held.closing()) {
                Waited::Ready => continue,
                // The window closed: `held` drops here, and its copy of the
                // master with it.
                Waited::Closing => return,
                // Darwin's `poll(2)` says it does not support devices; it
                // watched the master on the Mac this was written on, and
                // where it cannot, the master goes back to blocking -- the
                // reads wait in `read` and the writes in `write`, as before
                // -- rather than this loop spinning on `POLLNVAL`.
                Waited::Unwatchable => match blocking(fd) {
                    Ok(()) => {
                        crate::warn!("pty: poll cannot watch the master; it blocks again");
                        continue;
                    }
                    Err(_) => return,
                },
                Waited::Failed => return,
            },
            // The child's end: a link's program is reaped first, so its
            // status is set before the channel says the stream closed.
            // The child let go of its terminal: the channel says so once
            // it has dropped, and the window is woken to hear it. Nothing
            // here waits for the child, which may go on without its
            // terminal; its watch reaps it when it ends (`fg::Watch`).
            Got::Closed => {
                drop(tx);
                wake();
                return;
            }
        };
        // Stamped before anything else happens to the bytes: this is the
        // moment the child's answer reached the process.
        let at = host_now();
        let (writers, linked) =
            crate::link_layer::read(&mut sampler, std::time::Instant::now(), bytes, fd, watched);
        if let Some(f) = log.as_mut() {
            use std::io::Write;
            let _ = f.write_all(bytes);
        }
        if tx
            .send(Read {
                at,
                bytes: bytes.to_vec(),
                writers,
                linked,
            })
            .is_err()
        {
            return;
        }
        // The chunk is in the channel; wake the main thread once for
        // however many arrive before it drains.
        if !signalled.swap(true, Ordering::AcqRel) {
            wake();
        }
    }
}

/// What a wait said about a descriptor. A hang-up counts as readable: the
/// read that follows is what reports it. So does room to write, when the
/// wait was told bytes are owed: the read that follows finds nothing, and
/// the flush after it writes.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ready {
    Readable,
    Idle,
}

/// Whether the second descriptor of a `wait` is owed bytes it has not
/// taken, so room to write should end the wait too: the retry is scheduled
/// on the event that makes it possible, not on a timer. On macOS, whether
/// a write registers the master's room to write (`OutboxWatch::write`),
/// the same event heard through a kqueue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owed {
    Nothing,
    Bytes,
}

/// One read of the master.
#[cfg(target_os = "linux")]
pub enum Chunk {
    Bytes(Vec<u8>),
    Closed,
}

/// What else a [`wait`] watches a descriptor for: a clipboard transfer's
/// pipe, read from or written to a piece at a time.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interest {
    Read,
    Write,
}

/// **The records one [`wait`] polls**: `a`, then `b` -- or, with no `b`, a
/// record POSIX's `poll` skips, "If the value of fd is less than 0, events
/// shall be ignored, and revents shall be set to 0" (`[[source:
/// posix-poll]]`), so the places of `also` stay where the caller counts
/// them -- then each of `also`. A hung-up master is left out this way:
/// polled, it would answer `POLLHUP` at once on every wait after.
#[cfg(target_os = "linux")]
fn poll_set(
    a: BorrowedFd<'_>,
    b: Option<BorrowedFd<'_>>,
    b_owed: Owed,
    also: &[(BorrowedFd<'_>, Interest)],
) -> Vec<crate::sys::PollFd> {
    let mut fds = vec![
        crate::sys::PollFd {
            fd: a.as_raw_fd(),
            events: crate::sys::POLLIN,
            revents: 0,
        },
        crate::sys::PollFd {
            fd: b.map_or(-1, |b| b.as_raw_fd()),
            events: match b_owed {
                Owed::Nothing => crate::sys::POLLIN,
                Owed::Bytes => crate::sys::POLLIN | crate::sys::POLLOUT,
            },
            revents: 0,
        },
    ];
    fds.extend(also.iter().map(|(fd, i)| crate::sys::PollFd {
        fd: fd.as_raw_fd(),
        events: match i {
            Interest::Read => crate::sys::POLLIN,
            Interest::Write => crate::sys::POLLOUT,
        },
        revents: 0,
    }));
    fds
}

/// Waits until either descriptor is readable, one of `also` is ready for
/// what it is watched for, or `timeout` passes -- `None` waits as long as
/// it takes -- and says which of the two, and which of `also`, in its
/// order: a descriptor the kernel says nothing of is one the caller need
/// not read. A signal that cuts the wait short answers `Idle` for all,
/// so the caller simply waits again; `poll` reports a descriptor that is
/// still ready on the next wait.
#[cfg(target_os = "linux")]
pub fn wait(
    a: BorrowedFd<'_>,
    b: Option<BorrowedFd<'_>>,
    b_owed: Owed,
    also: &[(BorrowedFd<'_>, Interest)],
    timeout: Option<Duration>,
) -> std::io::Result<(Ready, Ready, Vec<Ready>)> {
    let mut fds = poll_set(a, b, b_owed, also);
    // Rounded up to whole milliseconds, not down, from the nanoseconds: a
    // deadline 0.8 ms -- or 0.8 us -- ahead truncated to `poll(0)` returns
    // at once, and a caller waiting for that deadline asks again, spinning
    // until the clock reaches it.
    let ms = match timeout {
        None => -1,
        Some(d) => i32::try_from(d.as_nanos().div_ceil(1_000_000)).unwrap_or(i32::MAX),
    };
    // SAFETY: initialised records and their count; the kernel writes only
    // `revents`, and the vector outlives the call.
    let n = unsafe {
        crate::sys::poll(
            fds.as_mut_ptr(),
            crate::sys::Nfds::try_from(fds.len()).unwrap_or(2),
            ms,
        )
    };
    if n < 0 {
        let e = std::io::Error::last_os_error();
        return match e.kind() {
            std::io::ErrorKind::Interrupted => {
                Ok((Ready::Idle, Ready::Idle, vec![Ready::Idle; also.len()]))
            }
            _ => Err(e),
        };
    }
    let ready = |p: &crate::sys::PollFd| match p.revents {
        0 => Ready::Idle,
        _ => Ready::Readable,
    };
    let extra = fds[2..].iter().map(ready).collect();
    Ok((ready(&fds[0]), ready(&fds[1]), extra))
}

/// **The font bytes and parsed faces a stack is made of, held**: each file
/// mapping and each parsed slice lent out as `'static`, kept so a font
/// reload or a window's close can give them back once nothing borrows
/// them ([`Held::release`]). Dropped without that, they stay for the
/// process. It lives here beside [`map_readonly`], the mapping it gives
/// back, which is private to this file.
///
/// **A font file is mapped once for the process** ([`MAPS`]), however
/// many windows' stacks are made of it: a second [`Held::map`] of the
/// same file takes the first's mapping and counts itself, and only the
/// last [`Held::release`] of it unmaps it. Two windows drawing one font
/// share its pages rather than each holding a copy, and a release in one
/// window never unmaps what another still reads.
///
/// A parsed slice is kept as the pointer `Box::into_raw` gave, not as the
/// shared reference lent from it: freeing needs write permission, which a
/// pointer taken back from a shared reference does not carry.
#[derive(Default)]
pub struct Held {
    maps: Vec<&'static [u8]>,
    parsed: Vec<*mut [baud_font::Face<'static>]>,
}

/// What [`Held::release`] gave back: mappings the kernel unmapped, and
/// parsed slices freed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Released {
    pub unmapped: usize,
    pub freed: usize,
}

impl Held {
    /// `path` mapped ([`map_readonly`]) and held: the mapping the process
    /// already has of this file when it has one, counted once more.
    pub fn map(&mut self, path: &str) -> std::io::Result<&'static [u8]> {
        let bytes = map_shared(path)?;
        self.maps.push(bytes);
        Ok(bytes)
    }

    /// `faces` lent for as long as they are held.
    pub fn leak(
        &mut self,
        faces: Vec<baud_font::Face<'static>>,
    ) -> &'static [baud_font::Face<'static>] {
        let raw = Box::into_raw(faces.into_boxed_slice());
        self.parsed.push(raw);
        // SAFETY: a live allocation `Box::into_raw` just gave up, valid
        // until `release` frees it, whose caller says nothing borrows it
        // by then; nothing writes through `raw` before that.
        unsafe { &*raw }
    }

    /// **Every parsed slice freed, then every mapping given back**: each
    /// one's count in [`MAPS`] taken down, and the file unmapped when no
    /// other holder counts it.
    ///
    /// # Safety
    ///
    /// Nothing may borrow a slice this lent -- a face, or the bytes under
    /// one -- once it is called: every `Stack` and `Fonts` made of them
    /// dropped, and no frame drawing from them. Another holder's borrows
    /// of a shared mapping stay valid: it is unmapped only once no holder
    /// counts it.
    pub unsafe fn release(self) -> Released {
        let mut given = Released::default();
        for raw in self.parsed {
            // SAFETY: the pointer `Box::into_raw` gave `leak`, given back
            // once; the caller says nothing borrows the slice.
            drop(unsafe { Box::from_raw(raw) });
            given.freed += 1;
        }
        for bytes in self.maps {
            if let Gone::Unmapped = unshare(bytes) {
                given.unmapped += 1;
            }
        }
        given
    }
}

/// **The font files the process has mapped**, each with how many holders
/// count it: one entry a file, keyed by what the file is rather than by
/// its path ([`FileId`]), so a font replaced at the same path -- a reload
/// after an edit -- is mapped afresh rather than handed the old bytes.
static MAPS: std::sync::Mutex<Vec<Mapping>> = std::sync::Mutex::new(Vec::new());

/// What a font file is: its device, inode, length and last change, from
/// `fstat`. A file written in place keeps its inode and changes its time
/// and, mostly, its length; one replaced by a rename has another inode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileId {
    dev: Device,
    ino: Inode,
    len: FileBytes,
    changed: Changed,
}

/// The device a file is on, `st_dev`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Device(u64);

/// A file's inode number, `st_ino`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Inode(u64);

/// A file's length in bytes, `st_size`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FileBytes(u64);

/// When a file's data last changed, `st_mtime` and its nanoseconds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Changed(i64, i64);

impl FileId {
    fn of(m: &std::fs::Metadata) -> FileId {
        use std::os::unix::fs::MetadataExt;
        FileId {
            dev: Device(m.dev()),
            ino: Inode(m.ino()),
            len: FileBytes(m.len()),
            changed: Changed(m.mtime(), m.mtime_nsec()),
        }
    }
}

/// How many holders count a mapping: at least one while it is in the
/// table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Holders(usize);

impl Holders {
    pub const ONE: Holders = Holders(1);

    pub fn more(self) -> Holders {
        Holders(self.0 + 1)
    }

    /// One fewer, or `None` when this was the last.
    fn fewer(self) -> Option<Holders> {
        self.0.checked_sub(1).filter(|&n| n > 0).map(Holders)
    }
}

/// One mapped file: what it is, its bytes, and how many holders count it.
struct Mapping {
    id: FileId,
    bytes: &'static [u8],
    holders: Holders,
}

/// What [`unshare`] did with a mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gone {
    /// Another holder still counts it.
    Kept,
    /// It was the last, and the kernel unmapped it.
    Unmapped,
    /// It was the last and `munmap` failed, or it was never in the table.
    Failed,
}

/// How many holders count the mapping that begins at `at`, `None` when
/// the process has no such mapping.
#[cfg(test)]
pub fn holders(at: *const u8) -> Option<Holders> {
    MAPS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|m| core::ptr::eq(m.bytes.as_ptr(), at))
        .map(|m| m.holders)
}

/// **`path`'s bytes, mapped once for the process**: the mapping [`MAPS`]
/// has of the same file, counted once more, else a new one entered there.
fn map_shared(path: &str) -> std::io::Result<&'static [u8]> {
    let file = std::fs::File::open(path)?;
    let id = FileId::of(&file.metadata()?);
    let mut maps = MAPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(m) = maps.iter_mut().find(|m| m.id == id) {
        // It stays mapped while any holder counts it, this one now among
        // them.
        m.holders = m.holders.more();
        return Ok(m.bytes);
    }
    let bytes = map_readonly(file)?;
    maps.push(Mapping {
        id,
        bytes,
        holders: Holders::ONE,
    });
    Ok(bytes)
}

/// **One holder's count of `bytes`' mapping taken down**, and the file
/// unmapped when it was the last.
fn unshare(bytes: &'static [u8]) -> Gone {
    let mut maps = MAPS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(i) = maps
        .iter()
        .position(|m| core::ptr::eq(m.bytes.as_ptr(), bytes.as_ptr()))
    else {
        return Gone::Failed;
    };
    if let Some(left) = maps[i].holders.fewer() {
        maps[i].holders = left;
        return Gone::Kept;
    }
    let m = maps.swap_remove(i);
    // SAFETY: a whole mapping `map_readonly` made, unmapped once, when the
    // last holder that counted it gives it back: its caller says nothing
    // borrows it any more, and no other holder counts it.
    let r = unsafe {
        crate::sys::munmap(
            m.bytes.as_ptr().cast_mut().cast(),
            crate::sys::MapLen(m.bytes.len()),
        )
    };
    match r {
        0 => Gone::Unmapped,
        _ => Gone::Failed,
    }
}

/// An open font file as a read-only, private mapping, for the life of
/// the process unless the last [`Held`] counting it gives it back. A face is a bounds-checked
/// view over the file's bytes, so a mapping is the same thing a read into
/// memory is -- without the read: Apple Color Emoji is 192 MB, and reading
/// it whole was 17 ms of every launch for the few strikes a screen uses.
/// The pages a face touches are the pages that get paged in.
fn map_readonly(file: std::fs::File) -> std::io::Result<&'static [u8]> {
    // **`File` is `open`, `fstat` and `close`**, and it opens
    // close-on-exec, so none of those and none of their flags has to be
    // declared in `sys`. That is most of what used to make this function
    // the largest consumer of a `libc` dependency.
    let len = usize::try_from(file.metadata()?.len()).unwrap_or(0);
    if len == 0 {
        return Err(std::io::Error::other("an empty font file"));
    }
    // SAFETY: a private read-only mapping of the whole file at offset
    // zero; the kernel validates the length against the file. The mapping
    // is unmapped only by the last `Held::release` that counts it
    // (`unshare`), whose caller says nothing borrows the slice any more.
    let ptr = unsafe {
        crate::sys::mmap(
            std::ptr::null_mut(),
            len,
            crate::sys::PROT_READ,
            crate::sys::MAP_PRIVATE,
            file.as_raw_fd(),
            0,
        )
    };
    // The mapping holds its own reference to the file, so closing the
    // descriptor -- which dropping `file` does -- leaves the pages.
    drop(file);
    if ptr == crate::sys::MAP_FAILED {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `ptr` is a live, page-aligned, read-only mapping of exactly
    // `len` bytes that outlives every borrow of it.
    Ok(unsafe { std::slice::from_raw_parts(ptr.cast::<u8>(), len) })
}

#[cfg(target_os = "macos")]
/// A display, as CoreGraphics names one.
#[repr(transparent)]
struct DisplayId(u32);

#[cfg(target_os = "macos")]
/// A count of pixels, or of points, as a display mode reports a width.
#[repr(transparent)]
struct Pixels(usize);

#[cfg(target_os = "macos")]
/// A display mode, as CoreGraphics hands one out: an opaque reference the
/// caller releases.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct DisplayMode(*const core::ffi::c_void);

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> DisplayId;
    fn CGDisplayCopyDisplayMode(display: DisplayId) -> DisplayMode;
    fn CGDisplayModeGetPixelWidth(mode: DisplayMode) -> Pixels;
    fn CGDisplayModeGetWidth(mode: DisplayMode) -> Pixels;
    fn CGDisplayModeRelease(mode: DisplayMode);
}

/// The main display's backing scale -- pixels over points -- from
/// CoreGraphics, which answers without starting AppKit. Asking
/// `NSScreen` for the same number costs AppKit's whole initialisation,
/// 42 ms measured, and the grid the shell is spawned at needs it first.
/// `None` when the display reports no width, which is no display.
#[cfg(target_os = "macos")]
pub fn display_scale() -> Option<f64> {
    // `CGDisplayPixelsWide` answers in points on a Retina display, so the
    // first version of this read 1 on a 2x panel and the first frame was
    // built at half its size. The display *mode* carries both widths.
    // SAFETY: four CoreGraphics calls with no preconditions; the mode is a
    // reference this function owns until it releases it, and a null one
    // is checked before use.
    let (pixels, points) = unsafe {
        let mode = CGDisplayCopyDisplayMode(CGMainDisplayID());
        if mode.0.is_null() {
            return None;
        }
        let both = (
            CGDisplayModeGetPixelWidth(mode),
            CGDisplayModeGetWidth(mode),
        );
        CGDisplayModeRelease(mode);
        both
    };
    if points.0 == 0 || pixels.0 == 0 {
        return None;
    }
    // Counts of pixels and points a display has are exact in an f64.
    Some(pixels.0 as f64 / points.0 as f64)
}

/// **The system's non-blinking-caret switch**, macOS 15's "prefer
/// non-blinking cursor": `AXPrefersNonBlinkingTextInsertionIndicator`,
/// looked up rather than linked, since it does not exist before 15 (its
/// header's `API_AVAILABLE(macos(15.0))`). The caller asks only on 15 or
/// later, so an older system loads no framework for a symbol it lacks.
///
/// The framework is asked for with `RTLD_NOLOAD` first -- AppKit has
/// loaded it by the first tick -- and opened only if it has not. The
/// function is looked up once and called fresh on every read: Chromium
/// calls it fresh on each query too. Any failure reads as the switch off,
/// which draws the caret as it asks, as before the switch existed.
#[cfg(target_os = "macos")]
pub fn prefers_steady_caret() -> crate::caret::NonBlinking {
    use crate::caret::NonBlinking;
    match steady_caret_symbol() {
        Some(f) if f().as_bool() => NonBlinking::On,
        _ => NonBlinking::Off,
    }
}

/// `AXPrefersNonBlinkingTextInsertionIndicator`: `BOOL (void)`.
#[cfg(target_os = "macos")]
type AskSteady = extern "C" fn() -> macos_liquid_glass::objc2::runtime::Bool;

/// The switch's function, looked up once, or `None` where the framework or
/// the symbol is missing.
#[cfg(target_os = "macos")]
fn steady_caret_symbol() -> Option<AskSteady> {
    use crate::sys::{RTLD_LAZY, RTLD_LOCAL, RTLD_NOLOAD, dlopen, dlsym};
    use std::sync::OnceLock;
    static ASK: OnceLock<Option<AskSteady>> = OnceLock::new();
    *ASK.get_or_init(|| {
        let path = c"/System/Library/Frameworks/Accessibility.framework/Accessibility";
        // SAFETY: `dlopen` and `dlsym` take NUL-terminated strings, which
        // `c""` literals are, and answer null on any failure, which is
        // checked. A non-null symbol is the function the SDK header
        // declares, `BOOL (void)`, and `Bool` is `repr(transparent)` over
        // the platform's `BOOL`, so the pointer is that function's.
        unsafe {
            let mut handle = dlopen(path.as_ptr(), RTLD_NOLOAD | RTLD_LAZY);
            if handle.is_null() {
                handle = dlopen(path.as_ptr(), RTLD_LAZY | RTLD_LOCAL);
            }
            if handle.is_null() {
                return None;
            }
            let sym = dlsym(
                handle,
                c"AXPrefersNonBlinkingTextInsertionIndicator".as_ptr(),
            );
            (!sym.is_null()).then(|| core::mem::transmute::<*mut core::ffi::c_void, AskSteady>(sym))
        }
    })
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// libSystem's `clock_gettime_nsec_np`, which libc 0.2 does not bind
    /// (it binds the `CLOCK_UPTIME_RAW` constant and `clock_gettime`, whose
    /// `timespec` would need a multiply and an add to become nanoseconds).
    /// It answers a `uint64_t`, and [`HostTime`] is `repr(transparent)`
    /// over that width, so the return crosses the ABI as the type the
    /// instrument uses rather than as a bare integer -- the same shape as
    /// the CoreGraphics declarations above.
    fn clock_gettime_nsec_np(clock_id: core::ffi::c_uint) -> HostTime;
}

/// Now, on the clock every stamp in the keystroke instrument shares:
/// `CLOCK_UPTIME_RAW`, which is `mach_absolute_time` in nanoseconds -- the
/// base `NSEvent.timestamp`, `CADisplayLink.targetTimestamp` and Metal's
/// `GPUEndTime` are expressed in (see `baud_domain::time`). The one place
/// this binary reads it, so every caller agrees on the clock by
/// construction. Zero if the call fails, which it does not for a valid
/// clock id.
#[cfg(target_os = "macos")]
pub fn host_now() -> HostTime {
    // SAFETY: a libSystem call with no preconditions beyond a valid clock
    // id, which `CLOCK_UPTIME_RAW` is.
    unsafe { clock_gettime_nsec_np(crate::sys::CLOCK_UPTIME_RAW) }
}

/// Now, on Linux's `CLOCK_MONOTONIC` in nanoseconds: the clock
/// `wp_presentation` names here and the one the key-latency tool's poster
/// stamps a key on, so a key's legs and its frame's presentation are read
/// on one clock. The driver refuses the instrument's run where the
/// compositor names another. Zero if the call fails, which it does not
/// for a valid clock id.
#[cfg(target_os = "linux")]
pub fn host_now() -> HostTime {
    let mut ts = crate::sys::Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid clock id and a timespec the call fills.
    if unsafe { crate::sys::clock_gettime(crate::sys::CLOCK_MONOTONIC, &mut ts) } != 0 {
        return HostTime::ZERO;
    }
    let secs = u64::try_from(ts.tv_sec).unwrap_or(0);
    let nanos = u64::try_from(ts.tv_nsec).unwrap_or(0);
    HostTime::of_nanos(secs.saturating_mul(1_000_000_000).saturating_add(nanos))
}

#[cfg(test)]
mod budget;

#[cfg(target_os = "macos")]
mod outbox_watch;
#[cfg(target_os = "macos")]
pub use outbox_watch::OutboxWatch;

#[cfg(test)]
mod shell_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::tool;

    /// **A font file two windows' fonts map is mapped once, and unmapped
    /// only when the last gives it back**: the second map is the first's
    /// bytes counted twice, the first release keeps them mapped and
    /// readable, the second unmaps them. A file replaced at the same path
    /// is mapped afresh, so a reload after an edit reads the new bytes.
    #[test]
    fn a_font_file_is_mapped_once_and_unmapped_by_its_last_holder() {
        let dir = std::env::temp_dir().join(format!("baud-shared-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Face.ttf");
        std::fs::write(&path, b"first bytes").unwrap();
        let p = path.to_str().unwrap();
        let (mut a, mut b) = (Held::default(), Held::default());
        let one = a.map(p).unwrap();
        let two = b.map(p).unwrap();
        assert!(core::ptr::eq(one.as_ptr(), two.as_ptr()), "one mapping");
        let at = one.as_ptr();
        assert_eq!(holders(at), Some(Holders(2)));
        // SAFETY: nothing borrows `a`'s bytes but `one`, not read again.
        let given = unsafe { a.release() };
        assert_eq!(given.unmapped, 0, "b still counts it");
        assert_eq!(holders(at), Some(Holders(1)));
        assert_eq!(two, b"first bytes", "and reads it");
        // A rename over the path: another inode, so another mapping.
        let next = dir.join("Face.ttf.new");
        std::fs::write(&next, b"second, longer bytes").unwrap();
        std::fs::rename(&next, &path).unwrap();
        let mut c = Held::default();
        let three = c.map(p).unwrap();
        assert_eq!(three, b"second, longer bytes", "the file as it is now");
        assert!(!core::ptr::eq(two.as_ptr(), three.as_ptr()));
        // SAFETY: nothing borrows `b`'s bytes after this line.
        let given = unsafe { b.release() };
        assert_eq!(given.unmapped, 1, "the last holder unmaps it");
        assert_eq!(holders(at), None);
        // SAFETY: nothing borrows `c`'s bytes after this line.
        let given = unsafe { c.release() };
        assert_eq!(given.unmapped, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **`wait` says which of its extra descriptors is ready**, so the
    /// loop reads only those: one with a byte waiting is readable, one
    /// with none is idle, whichever end of the list each is at.
    #[cfg(target_os = "linux")]
    #[test]
    fn wait_says_which_extra_descriptor_is_ready() {
        use std::io::Write;
        use std::os::fd::AsFd;
        let (a, _a) = std::os::unix::net::UnixStream::pair().unwrap();
        let (b, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let (quiet, _q) = std::os::unix::net::UnixStream::pair().unwrap();
        let (heard, mut to_heard) = std::os::unix::net::UnixStream::pair().unwrap();
        to_heard.write_all(b"x").unwrap();
        for also in [
            [
                (quiet.as_fd(), Interest::Read),
                (heard.as_fd(), Interest::Read),
            ],
            [
                (heard.as_fd(), Interest::Read),
                (quiet.as_fd(), Interest::Read),
            ],
        ] {
            let (ra, rb, extra) = wait(
                a.as_fd(),
                Some(b.as_fd()),
                Owed::Nothing,
                &also,
                Some(Duration::ZERO),
            )
            .unwrap();
            assert_eq!((ra, rb), (Ready::Idle, Ready::Idle));
            let want: Vec<Ready> = also
                .iter()
                .map(|(fd, _)| match fd.as_raw_fd() == heard.as_raw_fd() {
                    true => Ready::Readable,
                    false => Ready::Idle,
                })
                .collect();
            assert_eq!(extra, want);
        }
    }

    /// **What the environment says baud is, XTVERSION says too.** A
    /// program reads `TERM_PROGRAM` and `TERM_PROGRAM_VERSION` before it
    /// starts and asks `CSI > q` once running (yazi does both); the two
    /// answers come from two crates, so this holds them to one text:
    /// `DCS > | baud(<version>+<build>) ST`, a real `Term` answering after
    /// the call both drivers make, so a build whose core stopped being told
    /// fails here with its environment still right.
    #[test]
    fn the_environment_names_what_xtversion_reports() {
        let get = |k: &str| {
            OURS.iter()
                .find(|(o, _)| o.as_bytes() == k.as_bytes())
                .map(|(_, v)| *v)
        };
        let (name, version) = (get("TERM_PROGRAM"), get("TERM_PROGRAM_VERSION"));
        let want = format!("\x1bP>|{}({})\x1b\\", name.unwrap(), version.unwrap());
        assert!(version.unwrap().contains('+'), "the build is in it");
        let ws = baud_domain::WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = baud_core::Owned::new(&ws);
        let mut t = baud_core::Term::new(s.storage(), &ws);
        crate::host::set_version_text(&mut t);
        let query = b"\x1b[>q";
        t.write_at(baud_domain::BytePos::ZERO, query);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(query.len()),
        );
        assert_eq!(
            String::from_utf8_lossy(t.reply()),
            want,
            "XTVERSION against TERM_PROGRAM and TERM_PROGRAM_VERSION"
        );
    }

    /// **The positive control for the switch.** A misspelt symbol, or a
    /// framework path that moved, reads as the switch off forever with every
    /// other test green. So the lookup is held to the OS version, asked of
    /// `sw_vers` -- not of `NSProcessInfo`, since this test binary links no
    /// Foundation (`-dead_strip_dylibs` keeps only what it names): the
    /// symbol resolves on macOS 15 and later and is absent before. Its value
    /// is the user's setting and is not asserted.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_steady_switch_resolves_on_macos_fifteen_and_later() {
        let out = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .expect("sw_vers runs");
        let version = String::from_utf8_lossy(&out.stdout);
        let major: u32 = version
            .trim()
            .split('.')
            .next()
            .and_then(|m| m.parse().ok())
            .unwrap_or_else(|| panic!("sw_vers said {version:?}"));
        assert_eq!(
            steady_caret_symbol().is_some(),
            major >= 15,
            "macOS {version}"
        );
        // And the read answers, whichever way the user has set it.
        let _ = prefers_steady_caret();
    }

    /// A parent environment as Terminal.app, kitty and tmux leave it, with
    /// two variables that are nobody's terminal and must pass through.
    fn outer_parent() -> Vec<(OsString, OsString)> {
        [
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM_PROGRAM_VERSION", "455"),
            ("TERM_SESSION_ID", "w0t0p0:0"),
            ("KITTY_WINDOW_ID", "3"),
            ("TMUX", "/tmp/tmux-501/default,1,0"),
            ("TERM", "dumb"),
            ("COLORTERM", "no"),
            ("HOME", "/nowhere"),
            ("BAUD_KEPT", "yes"),
        ]
        .into_iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect()
    }

    fn strings(env: &[CString]) -> Vec<String> {
        let mut v: Vec<String> = env
            .iter()
            .map(|e| e.to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn ours() -> Vec<String> {
        OURS.iter()
            .map(|(k, v)| format!("{}={v}", String::from_utf8_lossy(k.as_bytes())))
            .collect()
    }

    /// **The child names baud and no outer terminal**: every identity
    /// variable the parent carried is gone, each of baud's appears once
    /// with baud's value, and a variable that names no terminal survives.
    #[test]
    fn the_child_names_baud_and_no_outer_terminal() {
        let mut want = ours();
        want.push("BAUD_KEPT=yes".into());
        want.push("HOME=/nowhere".into());
        want.sort();
        assert_eq!(strings(&child_env(outer_parent(), None)), want);
    }

    /// The list is what it says: no entry is one of baud's own, which
    /// would drop a variable and then set it -- harmless, but a sign the
    /// two lists were not read together -- and none is repeated.
    #[test]
    fn the_outer_list_is_disjoint_from_ours_and_has_no_repeat() {
        for o in OUTER {
            assert!(
                !OURS.iter().any(|(k, _)| k.as_bytes() == o.as_bytes()),
                "{o} is in both lists"
            );
        }
        let mut seen: Vec<&str> = OUTER.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), OUTER.len(), "OUTER repeats an entry");
    }

    /// **The master closes on exec**, so a program the app starts never
    /// holds it. Perturbation: drop `O_CLOEXEC` from the open -- this fails.
    #[test]
    fn the_master_closes_on_exec() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("exit 0".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // SAFETY: an open descriptor; `F_GETFD` takes no argument.
        let flags = unsafe { crate::sys::fcntl(master.as_raw_fd(), crate::sys::F_GETFD) };
        let _ = read_all(master.as_raw_fd());
        assert!(flags >= 0);
        assert_eq!(flags & crate::sys::FD_CLOEXEC, crate::sys::FD_CLOEXEC);
    }

    /// The 80x24 terminal of 8x16-pixel cells the spawn-race tests run on.
    pub(super) fn grid_80x24() -> crate::sys::Winsize {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        text_area(&size, Px::of(8), Px::of(16))
    }

    /// A `/bin/sh` loop that prints `held N` for each descriptor of `list`
    /// the shell holds as a terminal after its `exec`, a pty's master or
    /// replica: `-t`, so a file another part of this process opened
    /// without close-on-exec and left at that number is not counted. Then
    /// it prints `control` once it has copied its own terminal to
    /// descriptor 7, so a run with no `held` line is one in which the probe
    /// could have fired.
    const HOLDS: &str = "for n in $list; do [ -t $n ] && echo held $n; done; \
        exec 7<&0; [ -t 7 ] && echo control";

    /// [`HOLDS`] over `fds`, the negative ones dropped.
    fn holding(fds: &[crate::sys::Fd]) -> String {
        let list: Vec<String> = fds
            .iter()
            .filter(|f| **f >= 0)
            .map(ToString::to_string)
            .collect();
        format!("list='{}'; {HOLDS}", list.join(" "))
    }

    /// What a probe's child reported, beside the master and the replica it
    /// was asked about.
    type Report = (crate::sys::Fd, crate::sys::Fd, String);

    /// **A child forked while a terminal opens holds neither of its
    /// ends.** In the moment a spawn's terminal is open in this process
    /// ([`OPENED`]), a second spawn's child reports, after its `exec`,
    /// which of the first's master and replica it holds: neither. Under
    /// `forkpty` with the close-on-exec set after it returned, the child
    /// held the master.
    #[test]
    fn a_child_forked_while_a_terminal_opens_holds_neither_end() {
        let _window = libc_window();
        let env = child_env(outer_parent(), None);
        let seen: std::rc::Rc<std::cell::RefCell<Option<Report>>> = std::rc::Rc::default();
        let into = std::rc::Rc::clone(&seen);
        let probe_env = env.clone();
        set_opened(Some(Box::new(move |master, replica| {
            let m = Pty::fork_with(
                grid_80x24(),
                Some(holding(&[master, replica])),
                probe_env.clone(),
            )
            .expect("the probe's spawn");
            let out = String::from_utf8_lossy(&read_all(m.as_raw_fd())).into_owned();
            drop(m);
            *into.borrow_mut() = Some((master, replica, out));
        })));
        let master = Pty::fork_with(grid_80x24(), Some("exit 0".into()), env).expect("forkpty");
        set_opened(None);
        let _ = read_all(master.as_raw_fd());
        drop(master);
        let (m, r, out) = seen.take().expect("the hook ran");
        assert!(out.contains("control"), "the probe can fire: {out:?}");
        assert!(
            !out.contains("held"),
            "the child holds the master {m} or the replica {r}: {out:?}"
        );
    }

    /// What a descriptor of this process names, or `None` when it is not
    /// open.
    fn named_by(fd: crate::sys::Fd) -> Option<std::fs::Metadata> {
        std::fs::metadata(format!("/dev/fd/{fd}")).ok()
    }

    /// A descriptor, and what it named while a spawn's terminal was open.
    type Named = (crate::sys::Fd, Option<std::fs::Metadata>);

    /// **A spawn leaves the parent no replica**: the descriptor the
    /// replica had while the terminal was open ([`OPENED`]) no longer
    /// names that terminal once the spawn returns. A number another
    /// thread reused names another file, so only the same device fails.
    #[test]
    fn a_spawn_leaves_the_parent_no_replica() {
        use std::os::unix::fs::MetadataExt;
        let env = child_env(outer_parent(), None);
        let seen: std::rc::Rc<std::cell::RefCell<Option<Named>>> = std::rc::Rc::default();
        let into = std::rc::Rc::clone(&seen);
        set_opened(Some(Box::new(move |_, replica| {
            *into.borrow_mut() = Some((replica, named_by(replica)));
        })));
        let (pid, master) =
            Pty::fork_child(grid_80x24(), Some("exit 0".into()), env).expect("a spawn");
        set_opened(None);
        let (replica, during) = seen.take().expect("the hook ran");
        let after = named_by(replica);
        let _ = read_all(master.as_raw_fd());
        drop(master);
        // SAFETY: this test's own child, waited for once.
        unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
        let during = during.expect("the control: the replica was open in the window");
        let held = after.is_some_and(|a| a.rdev() == during.rdev());
        assert!(!held, "the parent still holds the replica {replica}");
    }

    /// **A terminal opened beside another never keeps it from hanging
    /// up**: the vim tests' deadlock (`programs.rs`, `Under`'s drop), on
    /// purpose. A spawns `cat`, and in A's window ([`OPENED`]) B spawns
    /// `cat` too. A's master closed, A's `cat` hears the hangup and ends
    /// while B's runs. When B's child held A's master, A's `cat` heard
    /// nothing until B's ended, and a drop that waits for A before it
    /// closes B waited for ever. The wait here has a deadline; past it
    /// both children are killed, since each may hold the other's master,
    /// and then the assertion fails.
    #[test]
    fn a_terminal_opened_beside_another_never_keeps_it_from_hanging_up() {
        let _window = libc_window();
        let env = child_env(outer_parent(), None);
        let b: std::rc::Rc<std::cell::Cell<Option<(core::ffi::c_int, std::os::fd::OwnedFd)>>> =
            std::rc::Rc::default();
        let into = std::rc::Rc::clone(&b);
        let b_env = env.clone();
        set_opened(Some(Box::new(move |_, _| {
            into.set(Pty::fork_child(
                grid_80x24(),
                Some("exec cat".into()),
                b_env.clone(),
            ));
        })));
        let (a_pid, a_master) =
            Pty::fork_child(grid_80x24(), Some("exec cat".into()), env).expect("A's spawn");
        set_opened(None);
        let (b_pid, b_master) = b.take().expect("B, spawned in A's window");
        let (ended, a_ended) = std::sync::mpsc::channel();
        let waiter = std::thread::spawn(move || {
            // SAFETY: A's pid, this test's own child, waited for once.
            unsafe { crate::sys::waitpid(a_pid, std::ptr::null_mut(), 0) };
            let _ = ended.send(());
        });
        drop(a_master);
        let alone = a_ended
            .recv_timeout(std::time::Duration::from_secs(10))
            .is_ok();
        if !alone {
            // Each child may hold the other's master, as the vim tests'
            // did, and then neither hangs up: both are ended here.
            // SAFETY: this test's own children, not yet reaped.
            unsafe {
                crate::sys::kill(a_pid, crate::sys::SIGKILL);
                crate::sys::kill(b_pid, crate::sys::SIGKILL);
            }
        }
        drop(b_master);
        // SAFETY: B's pid, this test's own child, waited for once.
        unsafe { crate::sys::waitpid(b_pid, std::ptr::null_mut(), 0) };
        waiter.join().expect("A's waiter");
        assert!(
            alone,
            "A's child did not end on its hangup while B's ran: B's child held A's master"
        );
    }

    /// **Terminals spawned at once hold none of each other's ends.**
    /// Sixteen threads spawn four children each, all at once, every
    /// thread recording the ends of its spawns ([`OPENED`]). Each child
    /// then reads the whole list and reports which of them it holds after
    /// its `exec` ([`HOLDS`]): none, and every report carries its control.
    #[test]
    fn terminals_spawned_at_once_hold_none_of_each_others_ends() {
        let _heavy = budget::heavy();
        let _window = libc_window();
        const THREADS: usize = 16;
        const ROUNDS: usize = 4;
        let env = child_env(outer_parent(), None);
        let start = std::sync::Barrier::new(THREADS);
        let ends: std::sync::Mutex<Vec<crate::sys::Fd>> = std::sync::Mutex::default();
        let script = format!("read list; {HOLDS}");
        let spawned: Vec<(core::ffi::c_int, std::os::fd::OwnedFd)> = std::thread::scope(|s| {
            let threads: Vec<_> = (0..THREADS)
                .map(|_| {
                    s.spawn(|| {
                        let mine: std::rc::Rc<std::cell::RefCell<Vec<crate::sys::Fd>>> =
                            std::rc::Rc::default();
                        let into = std::rc::Rc::clone(&mine);
                        set_opened(Some(Box::new(move |m, r| {
                            into.borrow_mut().extend([m, r]);
                        })));
                        start.wait();
                        let kids: Vec<_> = (0..ROUNDS)
                            .map(|_| {
                                Pty::fork_child(grid_80x24(), Some(script.clone()), env.clone())
                                    .expect("a spawn")
                            })
                            .collect();
                        set_opened(None);
                        ends.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .extend(mine.borrow().iter().copied().filter(|f| *f >= 0));
                        kids
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|t| t.join().expect("a thread"))
                .collect()
        });
        let mut list = ends
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        list.sort_unstable();
        list.dedup();
        let line: Vec<String> = list.iter().map(ToString::to_string).collect();
        let line = format!("{}\n", line.join(" "));
        for (_, master) in &spawned {
            let mut b = line.as_bytes();
            while !b.is_empty() {
                // SAFETY: an open descriptor and a buffer valid for its length.
                let n =
                    unsafe { crate::sys::write(master.as_raw_fd(), b.as_ptr().cast(), b.len()) };
                b = &b[usize::try_from(n).expect("the list's write")..];
            }
        }
        let mut leaks = Vec::new();
        for (pid, master) in spawned {
            let out = String::from_utf8_lossy(&read_all(master.as_raw_fd())).into_owned();
            drop(master);
            // SAFETY: this test's own child, waited for once.
            unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
            assert!(out.contains("control"), "the probe can fire: {out:?}");
            leaks.extend(
                out.lines()
                    .filter(|l| l.starts_with("held"))
                    .map(str::to_string),
            );
        }
        assert!(
            leaks.is_empty(),
            "of the {} ends, children held {leaks:?}",
            list.len()
        );
    }

    /// **And the child a pty forks actually sees it**: the same parent
    /// environment handed to a real `forkpty`, with the child running
    /// `env`, read back off the master. This is the gate on the fork's
    /// plumbing -- the `execve` and the pointer array -- rather than on the
    /// filter, which the test above holds.
    #[test]
    fn a_forked_child_sees_the_environment_baud_gives_it() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("exec /usr/bin/env".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let out = read_all(master.as_raw_fd());
        let text = String::from_utf8_lossy(&out);
        let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
        assert!(
            lines.contains(&"BAUD_KEPT=yes"),
            "the child's env never arrived, so nothing below is evidence: {text:?}"
        );
        for want in ours() {
            assert_eq!(
                lines.iter().filter(|l| **l == want).count(),
                1,
                "{want} is not in the child's env exactly once: {text:?}"
            );
        }
        for gone in [
            "TERM_SESSION_ID=",
            "KITTY_WINDOW_ID=",
            "TMUX=",
            "TERM_PROGRAM=Apple",
        ] {
            assert!(
                !lines.iter().any(|l| l.starts_with(gone)),
                "the child inherited {gone}: {text:?}"
            );
        }
    }

    /// **Held over libc's `forkpty` and over every test that counts what a
    /// child holds.** libc opens the master without close-on-exec, the race
    /// [`open_terminal`] closes, and a child another test forks in that
    /// window holds it: so the window and those forks never overlap.
    static LIBC_WINDOW: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// [`LIBC_WINDOW`], held.
    fn libc_window() -> std::sync::MutexGuard<'static, ()> {
        LIBC_WINDOW
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// **libc's `forkpty`**, made again while it loses the kernel's race:
    /// the control beside [`fork_terminal`], libc's own terminal and
    /// `login_tty`. Under [`LIBC_WINDOW`], its master set close-on-exec
    /// before the lock is let go.
    fn libc_forkpty(ws: crate::sys::Winsize) -> Result<Fork, NoTerminal> {
        let held = libc_window();
        let opening = budget::opening();
        // Darwin's `forkpty` takes the size by mutable pointer and
        // glibc's by const, so the binding is only mutated on one.
        #[cfg_attr(target_os = "linux", allow(unused_mut))]
        let mut ws = ws;
        let mut master: crate::sys::Fd = -1;
        #[cfg(target_os = "linux")]
        let ws_arg: *const crate::sys::Winsize = &raw const ws;
        #[cfg(not(target_os = "linux"))]
        let ws_arg: *mut crate::sys::Winsize = &raw mut ws;
        settle_before_fork();
        // SAFETY: forkpty is given a valid out-pointer and window size,
        // both live for the call. Darwin's opens the pty before it forks
        // (`[[source: apple-libc-pty-c]]`), so a lost race has not forked.
        let pid = redriven(|| unsafe {
            crate::sys::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                ws_arg,
            )
        });
        if pid == 0 {
            // The child's copy of the lock is never let go: it runs only
            // `execve` and `_exit` after this.
            core::mem::forget(held);
            core::mem::forget(opening);
            return Ok(Fork::Child);
        } else if pid > 0 {
            // SAFETY: an open descriptor; `F_SETFD` takes the flags as an int.
            unsafe { crate::sys::fcntl(master, crate::sys::F_SETFD, crate::sys::FD_CLOEXEC) };
            drop(opening);
            budget::opened();
            drop(held);
        } else {
            return Err(NoTerminal::Failed(crate::sys::Errno::last()));
        }
        // SAFETY: the manager `forkpty` opened, owned here alone.
        Ok(Fork::Parent(pid, unsafe {
            <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(master)
        }))
    }

    /// A child of a bare `forkpty` that runs only `execve`, as every spawn
    /// did before 2026-10-13: libc's terminal, for a control.
    fn bare_fork(ws: crate::sys::Winsize, cmd: &str, env: &[CString]) -> std::os::fd::OwnedFd {
        bare_fork_by(libc_forkpty, ws, cmd, env)
    }

    /// A spawn's fork, [`fork_terminal`] or [`libc_forkpty`], as each process
    /// has it ([`Fork`]).
    type Forker = fn(crate::sys::Winsize) -> Result<Fork, NoTerminal>;

    /// A child of `fork`, [`fork_terminal`] or [`libc_forkpty`], that runs
    /// `/bin/sh -c cmd` and nothing before it.
    fn bare_fork_by(
        fork: Forker,
        ws: crate::sys::Winsize,
        cmd: &str,
        env: &[CString],
    ) -> std::os::fd::OwnedFd {
        bare_fork_in(fork, "/bin/sh", ws, cmd, env)
    }

    /// [`bare_fork_by`] with the shell at `shell` in place of `/bin/sh`.
    fn bare_fork_in(
        fork: Forker,
        shell: &str,
        ws: crate::sys::Winsize,
        cmd: &str,
        env: &[CString],
    ) -> std::os::fd::OwnedFd {
        let path = CString::new(shell).expect("a path");
        let argv0 = CString::new("sh").expect("argv0");
        let dash = CString::new("-c").expect("-c");
        let cmd = CString::new(cmd).expect("a command");
        let argv = [
            argv0.as_ptr(),
            dash.as_ptr(),
            cmd.as_ptr(),
            std::ptr::null(),
        ];
        let envp: Vec<*const core::ffi::c_char> = env
            .iter()
            .map(|e| e.as_ptr())
            .chain(std::iter::once(std::ptr::null()))
            .collect();
        let Fork::Parent(_, master) = fork(ws).expect("forkpty") else {
            // SAFETY: as in `fork_with`; the child runs only `execve` and
            // `_exit`.
            unsafe {
                crate::sys::execve(path.as_ptr(), argv.as_ptr(), envp.as_ptr());
                crate::sys::_exit(127);
            }
        };
        master
    }

    /// **The spawned child's terminal is its controlling terminal and its
    /// standard input, output and error**, as `login_tty` leaves
    /// `forkpty`'s: `/dev/tty` opens in it, and descriptors 0, 1 and 2 are
    /// the terminal. libc's `forkpty` is the control, the same probe and
    /// the same answer. Without the child's `TIOCSCTTY` it fails on Linux.
    /// On macOS it passes even then: `/bin/sh` runs bash, which opens the
    /// terminal itself before the probe, and the session leader's first
    /// open of a terminal without `O_NOCTTY` takes it as the controlling
    /// terminal (`[[source: xnu-vfs-syscalls-c]]`).
    /// [`a_spawned_child_opens_its_controlling_terminal_by_dev_tty`] is the
    /// probe that fails on macOS too.
    #[test]
    fn a_spawned_child_holds_its_terminal_as_login_tty_leaves_it() {
        let env = child_env(outer_parent(), None);
        let probe = "if : </dev/tty; then echo ctty; else echo none; fi; \
            for n in 0 1 2; do [ -t $n ] && echo tty $n; done";
        let forks: [(&str, Forker); 2] = [
            ("libc's forkpty", libc_forkpty),
            ("baud's spawn", fork_terminal),
        ];
        for (name, fork) in forks {
            let m = bare_fork_by(fork, grid_80x24(), probe, &env);
            let out = String::from_utf8_lossy(&read_all(m.as_raw_fd())).into_owned();
            drop(m);
            let lines: Vec<&str> = out.lines().map(|l| l.trim_end_matches('\r')).collect();
            assert_eq!(
                lines,
                ["ctty", "tty 0", "tty 1", "tty 2"],
                "{name}: {out:?}"
            );
        }
    }

    /// **The spawned child's controlling terminal is the one it was given,
    /// with no open of a terminal of its own**: the child's first open is of
    /// `/dev/tty`, which names the controlling terminal and fails with
    /// `ENXIO` when there is none, and is not a pty device, so the session
    /// leader's take of a terminal on its first open (`[[source:
    /// xnu-vfs-syscalls-c]]`) cannot stand in for the child's `TIOCSCTTY`.
    /// The shell is dash, which opens no terminal before its command:
    /// macOS's `/bin/sh` runs bash, and measured here, through bash the
    /// probe reads `ctty` with no `TIOCSCTTY` in the child, and through
    /// dash it fails with `ENXIO`. libc's `forkpty` is the control.
    #[test]
    fn a_spawned_child_opens_its_controlling_terminal_by_dev_tty() {
        let env = child_env(outer_parent(), None);
        let forks: [(&str, Forker); 2] = [
            ("libc's forkpty", libc_forkpty),
            ("baud's spawn", fork_terminal),
        ];
        for (name, fork) in forks {
            let probe = "exec 3</dev/tty && echo ctty";
            let m = bare_fork_in(fork, &tool("dash"), grid_80x24(), probe, &env);
            let out = String::from_utf8_lossy(&read_all(m.as_raw_fd())).into_owned();
            drop(m);
            let lines: Vec<&str> = out.lines().map(|l| l.trim_end_matches('\r')).collect();
            assert_eq!(lines, ["ctty"], "{name}: {out:?}");
        }
    }

    /// **Terminals opened at once all open.** Darwin's `/dev/ptmx` can hand
    /// two openers the same terminal, and the kernel redoes the loser's
    /// open ten times before it fails it with `EREDRIVEOPEN`; forty-eight
    /// threads forking at once lose that often enough to fail it. Each
    /// thread forks a child through [`fork_terminal`], as a window does,
    /// that exits at once, reads its master to the end and reaps it.
    #[test]
    fn terminals_opened_at_once_all_open() {
        let _heavy = budget::heavy();
        const THREADS: usize = 48;
        const ROUNDS: usize = 10;
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let ws = text_area(&size, Px::of(8), Px::of(16));
        let start = std::sync::Barrier::new(THREADS);
        let failed: Vec<String> = std::thread::scope(|s| {
            let opened: Vec<_> = (0..THREADS)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        let mut failed = Vec::new();
                        for _ in 0..ROUNDS {
                            let forked = match fork_terminal(ws) {
                                Ok(forked) => forked,
                                Err(why) => {
                                    failed.push(why.to_string());
                                    continue;
                                }
                            };
                            let Fork::Parent(pid, master) = forked else {
                                // SAFETY: the child calls only `_exit`.
                                unsafe { crate::sys::_exit(0) }
                            };
                            read_all(master.as_raw_fd());
                            drop(master);
                            // SAFETY: this thread's own child, waited for once.
                            unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
                        }
                        failed
                    })
                })
                .collect();
            opened
                .into_iter()
                .flat_map(|t| t.join().expect("a thread"))
                .collect()
        });
        assert!(
            failed.is_empty(),
            "of {} opens, {} failed: {failed:?}",
            THREADS * ROUNDS,
            failed.len()
        );
    }

    /// What `stty -a` printed on `master`, word by word, once the child
    /// has gone; the descriptor is closed after.
    fn stty_words(master: std::os::fd::OwnedFd) -> Vec<String> {
        let out = read_all(master.as_raw_fd());
        drop(master);
        String::from_utf8_lossy(&out)
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// **The child's terminal has `IUTF8`, and nothing else differs from
    /// the kernel's.** `stty -a` run through baud's spawn against the same
    /// command under a bare `forkpty`: the two print the same words but
    /// `iutf8` for `-iutf8`. The bare one printing `-iutf8` is the control
    /// -- `stty` names the flag on this system and the default leaves it
    /// clear, so the probe can fire -- and the comparison holds the route to
    /// one bit, every other flag, character and speed the kernel's.
    #[test]
    fn a_spawned_childs_terminal_has_iutf8_and_otherwise_the_kernels_default() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let ws = text_area(&size, Px::of(8), Px::of(16));
        let env = child_env(outer_parent(), None);
        let cmd = format!("exec {} -a", tool("stty"));
        let bare = stty_words(bare_fork(ws, &cmd, &env));
        assert!(
            bare.iter().any(|w| w == "-iutf8"),
            "the control: a bare forkpty's terminal reads -iutf8: {bare:?}"
        );
        let ours = stty_words(Pty::fork_with(ws, Some(cmd), env).expect("forkpty"));
        let want: Vec<String> = bare
            .iter()
            .map(|w| match w.as_str() {
                "-iutf8" => "iutf8".to_string(),
                _ => w.clone(),
            })
            .collect();
        assert_eq!(ours, want, "baud's spawn against the kernel's default");
    }

    /// The bytes a line read in canonical mode holds after `typed`, in hex:
    /// the child turns echo off, says it is ready -- after which its terminal
    /// is set, so the line discipline reads `typed` under the flags the
    /// spawn left -- reads one line and prints it between markers.
    fn line_after(typed: &[u8]) -> String {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                r#"{} -echo; echo ready; IFS= read -r l; printf 'GOT[%s]\n' "$(printf '%s' "$l" | {} -An -tx1 | {} -d ' \n')""#,
                tool("stty"),
                tool("od"),
                tool("tr")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master.as_raw_fd(), "ready\r\n");
        // SAFETY: `typed` is a live buffer of the length passed.
        let n =
            unsafe { crate::sys::write(master.as_raw_fd(), typed.as_ptr().cast(), typed.len()) };
        assert_eq!(
            usize::try_from(n).ok(),
            Some(typed.len()),
            "the pty took it"
        );
        let out = read_all(master.as_raw_fd());
        drop(master);
        let text = String::from_utf8_lossy(&out).into_owned();
        text.split_once("GOT[")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(line, _)| line.to_string())
            .unwrap_or_else(|| panic!("the child printed no line: {text:?}"))
    }

    /// **Backspace after a Hebrew letter leaves an empty line**, where the
    /// kernel's default leaves its lead byte `d7`
    /// (`the_kernels_erase_follows_iutf8_alone`). The control, `ab` then
    /// Backspace, leaves `61` with the flag or without it, so a probe that
    /// erased everything would fail it.
    #[test]
    fn backspace_after_a_hebrew_letter_erases_the_whole_letter() {
        assert_eq!(line_after(b"ab\x7f\n"), "61", "the control");
        assert_eq!(line_after(b"\xd7\x99\x7f\n"), "");
    }

    /// **One ERASE removes a whole UTF-8 character with `IUTF8` and one byte
    /// without it, and the kernel's default is without**: a bare `forkpty`,
    /// as baud's own is, the child setting the flag itself and printing the
    /// line it read and the flag `stty` then reports. `ab` then ERASE is the
    /// control: one byte goes under either flag, so a line discipline that
    /// erased everything, or a probe that read nothing, would fail it.
    #[test]
    fn the_kernels_erase_follows_iutf8_alone() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let ws = text_area(&size, Px::of(8), Px::of(16));
        let env: Vec<CString> = ["PATH=/usr/bin:/bin", "HOME=/nowhere"]
            .iter()
            .map(|e| CString::new(*e).expect("no NUL"))
            .collect();
        let (stty_at, od, tr, grep) = (tool("stty"), tool("od"), tool("tr"), tool("grep"));
        let line_and_flag = |stty: &str, typed: &[u8]| -> (String, String) {
            let script = format!(
                r#"{stty_at} -echo {stty}; echo ready; IFS= read -r l; printf 'GOT[%s]\n' "$(printf '%s' "$l" | {od} -An -tx1 | {tr} -d ' \n')"; {stty_at} -a | {tr} ' ' '\n' | {grep} -E '^-?iutf8$'"#
            );
            let master = bare_fork(ws, &script, &env);
            read_until(master.as_raw_fd(), "ready\r\n");
            send(master.as_raw_fd(), typed);
            let out = read_all(master.as_raw_fd());
            drop(master);
            let text = String::from_utf8_lossy(&out).into_owned();
            let (line, flag) = text
                .split_once("GOT[")
                .and_then(|(_, rest)| rest.split_once(']'))
                .unwrap_or_else(|| panic!("the child printed no line: {text:?}"));
            (line.to_string(), flag.trim().to_string())
        };
        let yod: &[u8] = b"\xd7\x99\x7f\n";
        let ab: &[u8] = b"ab\x7f\n";
        for (stty, yod_line, flag) in [
            ("", "d7", "-iutf8"),
            ("-iutf8", "d7", "-iutf8"),
            ("iutf8", "", "iutf8"),
        ] {
            assert_eq!(
                line_and_flag(stty, yod),
                (yod_line.to_string(), flag.to_string()),
                "a Hebrew letter under `stty {stty}`"
            );
            assert_eq!(
                line_and_flag(stty, ab),
                ("61".to_string(), flag.to_string()),
                "the control under `stty {stty}`"
            );
        }
    }

    /// Whether a test lets the line discipline follow the core's encoding,
    /// or leaves the bit as the child set it -- the control, and what the
    /// app did before the bit followed.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Follow {
        Encoding,
        Never,
    }

    /// A text key, as `key_encode.rs` spells one.
    fn ch(c: char) -> baud_core::KeyCode {
        baud_core::KeyCode::Char(baud_core::Codepoint::from_char(c))
    }

    /// The bytes `keys` send through `Term::key`, in the encoding `t` is in.
    fn typed(t: &mut baud_core::Term<'_>, keys: &[baud_core::KeyCode]) -> Vec<u8> {
        t.clear_reply();
        for &k in keys {
            assert!(
                t.key(baud_core::KeyEvent::press(k, baud_core::Modifiers::NONE)),
                "{k:?} sends something"
            );
        }
        let out = t.reply().to_vec();
        t.clear_reply();
        out
    }

    /// `bytes` into the core, as `reads::feed` hands it a read.
    fn feed(t: &mut baud_core::Term<'_>, bytes: &[u8]) {
        t.write_at(baud_domain::BytePos::ZERO, bytes);
        t.feed(
            baud_domain::BytePos::ZERO,
            baud_domain::ReadLen::from_index(bytes.len()),
        );
    }

    /// `bytes` to the child, all of them.
    fn send(master: crate::sys::Fd, bytes: &[u8]) {
        // SAFETY: `bytes` is a live buffer of the length passed.
        let n = unsafe { crate::sys::write(master, bytes.as_ptr().cast(), bytes.len()) };
        assert_eq!(
            usize::try_from(n).ok(),
            Some(bytes.len()),
            "the pty took it"
        );
    }

    /// The child's half of a line test: echo off, then for each marker, the
    /// marker (after which its terminal is as the parent leaves it), one
    /// line read in canonical mode, and that line in hex between `GOT[` and
    /// `]`. `before` is printed ahead of each marker -- `ESC % @` or
    /// `ESC % G`, which the core reads in the same output.
    fn line_script(steps: &[(&str, &str)]) -> String {
        let (od, tr) = (tool("od"), tool("tr"));
        let mut s = format!("{} -echo", tool("stty"));
        for (before, marker) in steps {
            s.push_str(&format!(
                r#"; printf '{before}{marker}\n'; IFS= read -r l; printf 'GOT[%s]\n' "$(printf '%s' "$l" | {od} -An -tx1 | {tr} -d ' \n')""#
            ));
        }
        s
    }

    /// The lines a child read in canonical mode after the keys of each
    /// step, typed through a `Term` that read everything the child wrote,
    /// with the line discipline following its encoding or not: the whole
    /// path from the child's `ESC % @` to the line discipline, but the
    /// driver's pump. Each step is the bytes printed before its marker and
    /// the keys typed after it.
    fn lines_typed(follow: Follow, steps: &[(&str, &[baud_core::KeyCode])]) -> Vec<String> {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let markers: Vec<(String, String)> = steps
            .iter()
            .enumerate()
            .map(|(i, (before, _))| (before.to_string(), format!("ready{i}")))
            .collect();
        let script = line_script(
            &markers
                .iter()
                .map(|(b, m)| (b.as_str(), m.as_str()))
                .collect::<Vec<_>>(),
        );
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(script),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master.as_raw_fd());
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        let mut out = Vec::new();
        for ((_, keys), (_, marker)) in steps.iter().zip(&markers) {
            let read = read_until(master.as_raw_fd(), &format!("{marker}\r\n"));
            out.extend_from_slice(&read);
            feed(&mut t, &read);
            if follow == Follow::Encoding {
                if let Followed::Failed(e) = line.follow(t.input_encoding()) {
                    panic!("the line discipline did not follow: {e}");
                }
            }
            let bytes = typed(&mut t, keys);
            send(master.as_raw_fd(), &bytes);
        }
        out.extend_from_slice(&read_all(master.as_raw_fd()));
        drop(master);
        let text = String::from_utf8_lossy(&out).into_owned();
        let got: Vec<String> = text
            .split("GOT[")
            .skip(1)
            .map(|rest| rest.split_once(']').expect("a closed GOT").0.to_string())
            .collect();
        assert_eq!(got.len(), steps.len(), "one line a step: {text:?}");
        got
    }

    /// **Under `ESC % @` Backspace erases one ISO 8859-1 byte.** The child
    /// switches the core to ISO 8859-1; `a`, `£`, Backspace and Return
    /// through `Term::key` send `61 a3 7f 0d`, and the line holds `61`.
    /// The control leaves the bit as the child set it: the erase reads
    /// `a3` as a continuation and takes the `a` with it, the empty line a
    /// spawn that only sets the bit in the child leaves, so the probe can
    /// fire.
    #[test]
    fn under_latin1_backspace_erases_one_byte() {
        use baud_core::KeyCode::{Backspace, Enter};
        let keys = [ch('a'), ch('£'), Backspace, Enter];
        {
            let size = WinSize {
                ws_col: 80,
                ws_row: 24,
            };
            let mut s = baud_core::Owned::new(&size);
            let mut t = baud_core::Term::new(s.storage(), &size);
            feed(&mut t, b"\x1b%@");
            assert_eq!(
                typed(&mut t, &keys),
                b"a\xa3\x7f\r",
                "the keyboard sends ISO 8859-1"
            );
        }
        let step: &[(&str, &[baud_core::KeyCode])] = &[(r"\033%%@", &keys)];
        assert_eq!(lines_typed(Follow::Never, step), [""], "the control");
        assert_eq!(lines_typed(Follow::Encoding, step), ["61"]);
    }

    /// **Back under `ESC % G`, Backspace erases the whole letter again.**
    /// The same child switches to ISO 8859-1, reads a line, switches back
    /// and reads another: `a £` Backspace leaves `61`, then `י` Backspace,
    /// `d7 99 7f`, leaves the line empty. Without the bit set again the
    /// erase would take `99` and leave `d7`.
    #[test]
    fn back_under_utf8_backspace_erases_the_whole_letter() {
        use baud_core::KeyCode::{Backspace, Enter};
        let latin1 = [ch('a'), ch('£'), Backspace, Enter];
        let utf8 = [ch('י'), Backspace, Enter];
        assert_eq!(
            lines_typed(
                Follow::Encoding,
                &[(r"\033%%@", &latin1), (r"\033%%G", &utf8)]
            ),
            ["61", ""]
        );
    }

    /// **The line discipline is told only when the encoding changes**: the
    /// app calls [`LineDiscipline::follow`] after every read, and an open,
    /// two attribute calls and a close a read would be work for nothing.
    /// UTF-8 at the spawn, as the child left the bit.
    #[test]
    fn the_line_discipline_is_told_only_when_the_encoding_changes() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some("IFS= read -r l".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master.as_raw_fd());
        let told = |f: Followed| match f {
            Followed::Same => false,
            Followed::Told => true,
            Followed::Failed(e) => panic!("{e}"),
        };
        assert!(!told(line.follow(InputEncoding::Utf8)), "as spawned");
        assert!(told(line.follow(InputEncoding::Latin1)));
        assert!(!told(line.follow(InputEncoding::Latin1)));
        assert!(told(line.follow(InputEncoding::Utf8)));
        assert!(!told(line.follow(InputEncoding::Utf8)));
        send(master.as_raw_fd(), b"\r");
        drop(read_all(master.as_raw_fd()));
        drop(master);
    }

    /// What `stty -a < /dev/tty` printed after the child switched the core
    /// to ISO 8859-1 and the parent did or did not follow: the child's
    /// controlling terminal, opened by that name after the parent's open,
    /// change and close.
    fn stty_after_latin1(follow: Follow) -> Vec<String> {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                r#"{stty} -echo; printf '\033%%@ready\n'; IFS= read -r l; exec {stty} -a < /dev/tty"#,
                stty = tool("stty")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        feed(&mut t, &read_until(master.as_raw_fd(), "ready\r\n"));
        assert_eq!(
            t.input_encoding(),
            InputEncoding::Latin1,
            "the core switched"
        );
        if follow == Follow::Encoding {
            let mut line = LineDiscipline::of(master.as_raw_fd());
            assert!(matches!(line.follow(t.input_encoding()), Followed::Told));
        }
        send(master.as_raw_fd(), b"\r");
        stty_words(master)
    }

    /// **The parent's change is one bit, on the child's own terminal.**
    /// `stty -a` on the child's `/dev/tty` -- which opens only while the
    /// child still has its controlling terminal -- prints the same words
    /// with and without the parent's change, but `-iutf8` for `iutf8`.
    #[test]
    fn the_parents_change_is_iutf8_alone_on_the_childs_terminal() {
        let before = stty_after_latin1(Follow::Never);
        assert!(
            before.iter().any(|w| w == "iutf8"),
            "the control: the child set the bit: {before:?}"
        );
        let want: Vec<String> = before
            .iter()
            .map(|w| match w.as_str() {
                "iutf8" => "-iutf8".to_string(),
                _ => w.clone(),
            })
            .collect();
        assert_eq!(stty_after_latin1(Follow::Encoding), want);
    }

    /// **A full reset sets `IUTF8` again.** The child switches the core to
    /// ISO 8859-1 and the parent follows, clearing the bit; then the child
    /// sends `RIS`, which returns the core to UTF-8, and the parent follows
    /// again. `stty -a` on the child's `/dev/tty` prints `iutf8`. Each
    /// switch arrives in a read of its own, so the parent follows both,
    /// as the app does after every read; the first is the control, which
    /// prints `-iutf8` once the bit is cleared.
    #[test]
    fn a_full_reset_sets_iutf8_again() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                r#"{stty} -echo; printf '\033%%@latin\n'; IFS= read -r l; {stty} -a < /dev/tty; printf 'cleared\n'; IFS= read -r l; printf '\033creset\n'; IFS= read -r l; exec {stty} -a < /dev/tty"#,
                stty = tool("stty")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        let mut line = LineDiscipline::of(master.as_raw_fd());
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        let told = |f: Followed| match f {
            Followed::Same => false,
            Followed::Told => true,
            Followed::Failed(e) => panic!("{e}"),
        };
        feed(&mut t, &read_until(master.as_raw_fd(), "latin\r\n"));
        assert_eq!(t.input_encoding(), InputEncoding::Latin1);
        assert!(told(line.follow(t.input_encoding())), "cleared");
        send(master.as_raw_fd(), b"\r");
        let cleared = read_until(master.as_raw_fd(), "cleared\r\n");
        let words = String::from_utf8_lossy(&cleared).into_owned();
        assert!(
            words.split_whitespace().any(|w| w == "-iutf8"),
            "the control: the bit is clear under ISO 8859-1: {words:?}"
        );
        send(master.as_raw_fd(), b"\r");
        feed(&mut t, &read_until(master.as_raw_fd(), "reset\r\n"));
        assert_eq!(t.input_encoding(), InputEncoding::Utf8, "RIS");
        assert!(told(line.follow(t.input_encoding())), "set again");
        send(master.as_raw_fd(), b"\r");
        let after = stty_words(master);
        assert!(
            after.iter().any(|w| w == "iutf8"),
            "the bit is set again: {after:?}"
        );
    }

    /// Whether `master` reads as hung up within two seconds -- a bound, so
    /// a subsidiary held open fails the test rather than hanging it.
    fn hung_up(master: crate::sys::Fd) -> bool {
        make_nonblocking(master);
        let mut p = crate::sys::PollFd {
            fd: master,
            events: crate::sys::POLLIN,
            revents: 0,
        };
        // SAFETY: one initialised record and its count.
        let ready = unsafe { crate::sys::poll(&mut p, 1, 2000) };
        let mut buf = [0u8; 64];
        ready > 0 && matches!(read_master(master, &mut buf), Got::Closed)
    }

    /// **The parent never holds the subsidiary, so the child's going still
    /// hangs the manager up.** The child prints `ESC % @` and exits; the
    /// parent sees the end, reads the switch and follows it after the child
    /// has gone, and the manager still reads as hung up. The control holds
    /// the subsidiary open across the same read, and the manager then reads
    /// as waiting, so the probe can fire.
    #[test]
    fn following_after_the_child_has_gone_leaves_the_manager_hung_up() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let spawn = || {
            Pty::fork_with(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(r"printf '\033%%@'".into()),
                child_env(outer_parent(), None),
            )
            .expect("forkpty")
        };
        let master = spawn();
        let mut line = LineDiscipline::of(master.as_raw_fd());
        let mut s = baud_core::Owned::new(&size);
        let mut t = baud_core::Term::new(s.storage(), &size);
        feed(&mut t, &read_all(master.as_raw_fd()));
        assert_eq!(
            t.input_encoding(),
            InputEncoding::Latin1,
            "the core switched"
        );
        let followed = line.follow(t.input_encoding());
        assert!(
            matches!(followed, Followed::Told),
            "the subsidiary opens after the child has gone: {followed:?}"
        );
        assert!(hung_up(master.as_raw_fd()), "the manager reads as hung up");
        drop(master);

        let master = spawn();
        drop(read_all(master.as_raw_fd()));
        let held = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags_noctty()
            .open(subsidiary_of(master.as_raw_fd()).expect("a subsidiary"))
            .expect("the subsidiary opens");
        assert!(
            !hung_up(master.as_raw_fd()),
            "the control: held open, it waits"
        );
        drop(held);
        drop(master);
    }

    /// `O_NOCTTY` on an `OpenOptions`, for the controls that open the
    /// subsidiary as the parent does.
    trait NoCtty {
        fn custom_flags_noctty(&mut self) -> &mut Self;
    }
    impl NoCtty for std::fs::OpenOptions {
        fn custom_flags_noctty(&mut self) -> &mut Self {
            use std::os::unix::fs::OpenOptionsExt;
            self.custom_flags(crate::sys::O_NOCTTY)
        }
    }

    /// The variable that turns [`ctty_helper`] on: the subsidiary's path,
    /// then `follow` or `plain` after a tab.
    const CTTY_HELPER: &str = "BAUD_TEST_CTTY_HELPER";

    /// **Run only by [`the_parent_never_takes_the_subsidiary_as_its_terminal`]**,
    /// as a process of its own; a no-op otherwise. It starts a session --
    /// no controlling terminal, the state of an app the Dock started --
    /// opens the subsidiary the way it is told, and says whether `/dev/tty`
    /// opens after: whether it took the subsidiary as its terminal.
    #[test]
    fn ctty_helper() {
        let Ok(arg) = std::env::var(CTTY_HELPER) else {
            return;
        };
        let (path, how) = arg.split_once('\t').expect("a path and a mode");
        // SAFETY: no arguments; a process that is not a group leader.
        assert!(unsafe { crate::sys::setsid() } >= 0, "a new session");
        assert!(
            std::fs::File::open("/dev/tty").is_err(),
            "a new session has no terminal"
        );
        match how {
            "follow" => {
                let mut line = LineDiscipline {
                    subsidiary: Some(path.into()),
                    told: InputEncoding::Utf8,
                };
                let followed = line.follow(InputEncoding::Latin1);
                assert!(matches!(followed, Followed::Told), "{followed:?}");
            }
            _ => drop(std::fs::File::open(path).expect("the subsidiary opens")),
        }
        let took = std::fs::File::open("/dev/tty").is_ok();
        println!("CTTY[{took}]");
    }

    fn env_of(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    /// **The child goes home only when launchd started baud, and only to
    /// an absolute `HOME`**: under any other parent it stays where baud
    /// is, and so it does under launchd when `HOME` is missing, empty or
    /// relative. The first `HOME` is the one used, as `getenv` reads it.
    #[test]
    fn the_child_starts_home_only_under_launchd_and_an_absolute_home() {
        let home = |v: &str| StartIn::Dir(CString::new(v).expect("no NUL"));
        let with_home = env_of(&[("HOME", "/Users/u"), ("HOME", "/second")]);
        assert_eq!(start_in(Parent::Launchd, &with_home), home("/Users/u"));
        assert_eq!(start_in(Parent::Other, &with_home), StartIn::Inherited);
        for env in [
            env_of(&[]),
            env_of(&[("HOME", "")]),
            env_of(&[("HOME", "relative/home")]),
        ] {
            assert_eq!(
                start_in(Parent::Launchd, &env),
                StartIn::Inherited,
                "{env:?}"
            );
        }
    }

    /// **A child sent home is told so by its `PWD`**, which replaces any
    /// it would inherit; a child that stays keeps its environment exactly.
    #[test]
    fn a_child_sent_home_has_home_as_its_pwd() {
        let env = || {
            ChildEnv::of(env_of(&[
                ("PWD", "/where/baud/is"),
                ("BAUD_KEPT", "yes"),
                ("OLDPWD", "/before"),
            ]))
        };
        let home = StartIn::Dir(CString::new("/Users/u").expect("no NUL"));
        assert_eq!(
            strings(&with_pwd(env(), &home).into_envp()),
            ["BAUD_KEPT=yes", "OLDPWD=/before", "PWD=/Users/u"]
        );
        assert_eq!(with_pwd(env(), &StartIn::Inherited), env());
    }

    /// **The spawn changes the child's directory**, through
    /// [`Pty::fork_from`] on any platform with the parent given: told that
    /// launchd started baud, the shell starts in the `HOME` it was handed;
    /// told otherwise, where this test is. The control is that the two
    /// directories differ. `HOME` is a symbolic link, so the `PWD` the
    /// shell reports tells baud's `PWD` (the link's spelling) from one the
    /// shell found itself (the resolved one): a POSIX shell keeps a `PWD`
    /// from its environment that is "an absolute pathname of the current
    /// working directory", and otherwise sets it as `pwd -P` would print
    /// it (`[[source: posix-xcu-2]]`, 2.5.3).
    #[test]
    fn the_spawn_starts_the_child_where_its_parent_says() {
        let real = scratch_dir("given-home");
        let link = scratch_dir("given-link").join("home");
        std::os::unix::fs::symlink(&real, &link).expect("a link to home");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        assert_ne!(real, here, "the control: two directories to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |started_by: Parent| -> Vec<String> {
            let parent = env_of(&[
                ("HOME", link.to_str().expect("a UTF-8 path")),
                ("PWD", "/where/baud/is"),
                ("LANG", "C"),
            ]);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(format!("{} -P; printf 'PWD=%s\\n' \"$PWD\"", tool("pwd"))),
                parent,
                || None,
                started_by,
                None,
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master.as_raw_fd()))
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        };
        assert_eq!(
            run(Parent::Launchd),
            [
                real.display().to_string(),
                format!("PWD={}", link.display())
            ]
        );
        assert_eq!(
            run(Parent::Other)[0],
            here.display().to_string(),
            "where this test is"
        );
        for d in [real, link.parent().expect("its directory").into()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **A new window's shell starts in the directory it inherits**
    /// (`windows::inherited`), whoever started baud, with `PWD` naming it;
    /// a relative one is not taken, and the launch rule stands. The
    /// directory is a symbolic link, so the `PWD` the shell reports tells
    /// baud's spelling from the one `pwd -P` resolves, and the control is
    /// that the inherited directory is not where this test is.
    #[test]
    fn a_new_windows_shell_starts_in_the_directory_it_inherits() {
        let real = scratch_dir("inherited");
        let link = scratch_dir("inherited-link").join("dir");
        std::os::unix::fs::symlink(&real, &link).expect("a link to it");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        assert_ne!(real, here, "the control: two directories to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |started_by: Parent, at: &std::path::Path| -> Vec<String> {
            let parent = env_of(&[
                ("HOME", "/nowhere/home"),
                ("PWD", "/where/baud/is"),
                ("LANG", "C"),
            ]);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(format!("{} -P; printf 'PWD=%s\\n' \"$PWD\"", tool("pwd"))),
                parent,
                || None,
                started_by,
                Some(at),
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master.as_raw_fd()))
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect()
        };
        let want = [
            real.display().to_string(),
            format!("PWD={}", link.display()),
        ];
        assert_eq!(run(Parent::Other, &link), want);
        assert_eq!(run(Parent::Launchd, &link), want, "over HOME");
        assert_eq!(
            run(Parent::Other, std::path::Path::new("relative"))[0],
            here.display().to_string(),
            "a relative directory is not inherited"
        );
        for d in [real, link.parent().expect("its directory").into()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **A window's close hangs up its shell**, as baud's exit does: the
    /// window's `Pty` drops, its descriptor closes and the pipe it holds
    /// ends the reader, which closes the other -- and the master's last
    /// close sends the shell `SIGHUP`. The shell traps it and writes to a
    /// FIFO, whose open waits until the trap runs: a pass is never timed,
    /// and the bound on each wait only turns a regression's hang into a
    /// failure. The control is the shell answering through the reader
    /// before the close.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_closed_windows_pty_hangs_up_its_shell() {
        let scratch = scratch_dir("hang-up");
        let fifo = scratch.join("hup");
        let made = std::process::Command::new(tool("mkfifo"))
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "a FIFO");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let told = text_area(&size, Px::of(8), Px::of(16));
        let master = Pty::fork_with(
            told,
            Some(format!(
                "{} -echo; trap 'printf hup > {}; exit 0' HUP; echo ready; while :; do read x; echo \"got $x\"; done",
                tool("stty"),
                fifo.display()
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master.as_raw_fd(), "ready\r\n");
        make_nonblocking(master.as_raw_fd());
        let (tx, rx) = channel();
        let signalled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&signalled);
        let (held, closing) = Reading::of(master.as_raw_fd());
        assert!(
            closing.is_some(),
            "the control: the reader has its own copy"
        );
        let (ended, reader_ended) = channel();
        std::thread::spawn(move || {
            reader(
                held,
                tx,
                flag,
                &crate::probe::WatchCell::default(),
                None,
                || {},
            );
            let _ = ended.send(());
        });
        let opened = fifo.clone();
        let (trapped, heard) = channel();
        std::thread::spawn(move || {
            let _ = trapped.send(std::fs::read_to_string(&opened));
        });
        let bound = std::time::Duration::from_secs(60);
        let master = master.into_raw_fd();
        let pty = Live {
            master,
            child: None,
            told,
            line: LineDiscipline::of(master),
            rx,
            signalled,
            watched: crate::probe::WatchCell::default(),
            closing,
            ending: ChildEnd::of(None),
        };
        // The control: the reader's copy alone keeps the line up, so the
        // shell still answers through it once a write reaches it.
        let mut out = Outbox::default();
        out.push(b"one\n");
        assert_eq!(write_some(pty.master, &mut out), Wrote::All);
        let mut echoed = Vec::new();
        while !echoed.ends_with(b"got one\r\n") {
            let read = pty
                .rx
                .recv_timeout(bound)
                .expect("the reader passes the answer on");
            echoed.extend_from_slice(&read.bytes);
        }
        assert_eq!(String::from_utf8_lossy(&echoed), "got one\r\n");
        drop(pty);
        reader_ended
            .recv_timeout(bound)
            .expect("the reader ends with the window");
        let heard = heard.recv_timeout(bound).expect("the shell was hung up");
        assert_eq!(heard.expect("the trap writes the FIFO"), "hup");
        let _ = std::fs::remove_dir_all(scratch);
    }

    /// **A child whose `chdir` fails keeps the environment of one that
    /// stayed**, so its `PWD` never names a `HOME` it is not in: told that
    /// launchd started baud, with an absolute `HOME` that does not exist,
    /// the shell starts where this test is and is handed the parent's
    /// `PWD`. That `PWD` is a symbolic link to this directory, and a POSIX
    /// shell keeps a `PWD` from its environment only when it is "an
    /// absolute pathname of the current working directory" (`[[source:
    /// posix-xcu-2]]`, 2.5.3), so the shell prints the link's spelling only
    /// if it was given the stay environment; given `PWD=$HOME` it would
    /// find its own and print the resolved one. The control is that the
    /// two spellings differ.
    #[test]
    fn a_failed_chdir_hands_the_shell_the_environment_of_one_that_stayed() {
        let scratch = scratch_dir("failed-chdir");
        let missing = scratch.join("missing-home");
        assert!(!missing.exists(), "the control: HOME names nothing");
        let here = std::env::current_dir()
            .expect("a working directory")
            .canonicalize()
            .expect("it resolves");
        let link = scratch.join("here");
        std::os::unix::fs::symlink(&here, &link).expect("a link to here");
        assert_ne!(link, here, "the control: two spellings to tell apart");
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let parent = env_of(&[
            ("HOME", missing.to_str().expect("a UTF-8 path")),
            ("PWD", link.to_str().expect("a UTF-8 path")),
            ("LANG", "C"),
        ]);
        let master = Pty::fork_from(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!("{} -P; printf 'PWD=%s\\n' \"$PWD\"", tool("pwd"))),
            parent,
            || None,
            Parent::Launchd,
            None,
        )
        .expect("forkpty");
        let lines: Vec<String> = String::from_utf8_lossy(&read_all(master.as_raw_fd()))
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        assert_eq!(
            lines,
            [
                here.display().to_string(),
                format!("PWD={}", link.display())
            ],
            "where baud is, with the PWD it would have had"
        );
        let _ = std::fs::remove_dir_all(scratch);
    }

    /// The variable that turns [`spawn_helper`] on: `orphan` to wait until
    /// launchd is its parent first, anything else to spawn at once.
    const SPAWN_HELPER: &str = "BAUD_TEST_SPAWN_HELPER";

    /// **Run only by [`a_shell_spawned_under_launchd_starts_home`]**, as a
    /// process of its own; a no-op otherwise. It spawns a shell the way
    /// the app does, [`Pty::fork`] with the process's own environment and
    /// parent, and prints its parent's ID and what the shell printed.
    ///
    /// As an orphan it first waits for its parent to be launchd: the `sh`
    /// that starts it exits at once, and POSIX gives the children of a
    /// process that exits to "an implementation-defined system process"
    /// (`[[source: posix-exit]]`), which on macOS is launchd, process 1
    /// (measured with `ps`).
    #[test]
    fn spawn_helper() {
        let Ok(how) = std::env::var(SPAWN_HELPER) else {
            return;
        };
        if how == "orphan" {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while std::os::unix::process::parent_id() != 1 {
                assert!(
                    std::time::Instant::now() < deadline,
                    "never adopted by launchd"
                );
                std::thread::yield_now();
            }
        }
        println!("PPID[{}]", std::os::unix::process::parent_id());
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let (_, master, _) = Pty::fork(
            text_area(&size, Px::of(8), Px::of(16)),
            None,
            &crate::shell::Integration::Off.into(),
        )
        .and_then(Result::ok)
        .expect("forkpty");
        for line in String::from_utf8_lossy(&read_all(master.as_raw_fd())).lines() {
            println!("CHILD[{}]", line.trim_end_matches('\r'));
        }
    }

    /// **The film's shell finds its own `PWD`, as the window's does**
    /// ([`Pty::fork_shell`]): a real login zsh started in a directory spelled
    /// through a symbolic link, with a `HOME` and a `PWD` in that spelling,
    /// reports the resolved path as `PWD`, not the spelling. The control is
    /// a new window's start of the same directory
    /// (`a_new_windows_shell_starts_in_the_directory_it_inherits`), which
    /// keeps the spelling. Perturbation: give it `with_pwd`, and `PWD` is the
    /// link's, and a headless take cannot go red where a window does.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_film_s_shell_finds_its_own_pwd() {
        let real = scratch_dir("film-pwd");
        let link = scratch_dir("film-pwd-link").join("home");
        std::os::unix::fs::symlink(&real, &link).expect("a link to it");
        std::fs::write(
            real.join(".zshrc"),
            "print -r -- \"PWD=$PWD\"\nprint -r -- \"HOME=$HOME\"\nexit 0\n",
        )
        .expect("the startup file");
        let spelled = link.to_str().expect("a UTF-8 path");
        let parent = env_of(&[
            ("HOME", spelled),
            ("ZDOTDIR", spelled),
            ("PWD", spelled),
            ("SHELL", "/bin/zsh"),
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C"),
            ("NOSYSZSHRC", "1"),
        ]);
        let size = WinSize {
            ws_col: 200,
            ws_row: 24,
        };
        let (_, master) = Pty::fork_shell(text_area(&size, Px::of(8), Px::of(16)), parent, &link)
            .expect("forkpty");
        let said = String::from_utf8_lossy(&read_all(master.as_raw_fd())).into_owned();
        let lines: Vec<&str> = said.lines().map(|l| l.trim_end_matches('\r')).collect();
        assert!(
            lines.contains(&format!("PWD={}", real.display()).as_str()),
            "{said:?}"
        );
        assert!(
            lines.contains(&format!("HOME={spelled}").as_str()),
            "the home keeps its spelling: {said:?}"
        );
        for d in [real, link.parent().expect("its directory").into()] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// An executable script at `p` holding `body`, written whole and
    /// closed before anything runs it.
    fn stand_in(p: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(p.parent().expect("a folder")).expect("its folder");
        std::fs::write(p, body).expect("the stand-in");
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).expect("executable");
    }

    /// **What reaches `execve` is absolute, or nothing is run**: for each
    /// kind of program, a bare name, `./x`, `a/b`, an absolute path and
    /// none, under every kind of `PATH` -- unset, empty, relative,
    /// absolute, and mixtures with empty entries -- over a folder holding
    /// an executable of each name, the path found is absolute or there is
    /// none. **A slash-relative program never resolves**, whatever `PATH`
    /// says: `./x`, `a/b`, `../x` and `x/` mean a file only against the
    /// folder they were typed in, which the child, having changed folder,
    /// is no longer in. A bare name is found only in an absolute entry,
    /// never through an empty or relative one; an absolute path is as
    /// written.
    #[test]
    fn a_slash_relative_program_never_resolves() {
        let dir = scratch_dir("exec-path");
        for name in ["x", "a/b"] {
            stand_in(&dir.join(name), "#!/bin/sh\n");
        }
        let abs = dir.to_str().expect("a UTF-8 path").to_string();
        // The same folder by a relative entry, from where the test runs: up
        // to the root and down again, so a relative entry that is searched
        // finds the executable and shows as a relative result.
        let here = std::env::current_dir().expect("a working directory");
        let up = "../".repeat(here.components().count());
        let rel = format!("{up}{}", abs.trim_start_matches('/'));
        assert!(
            std::path::Path::new(&rel).join("x").is_file(),
            "the control: {rel} reaches it"
        );
        let found = |prog: &str, path: Option<&str>| {
            exec::ExecPath::resolve(OsStr::new(prog), path.map(OsStr::new))
                .map(|p| p.as_str().expect("UTF-8").to_string())
        };
        let paths = [
            None,
            Some(String::new()),
            Some("rel".to_string()),
            Some(rel.clone()),
            Some(format!("{rel}:{abs}")),
            Some(abs.clone()),
            Some(format!("rel:{abs}")),
            Some(format!("::{abs}")),
            Some(format!("{abs}::")),
        ];
        let progs = ["x", "./x", "a/b", "../x", "x/", "./", "/bin/sh", ""];
        for path in &paths {
            for prog in progs {
                let got = found(prog, path.as_deref());
                assert!(
                    got.as_deref().is_none_or(|g| g.starts_with('/')),
                    "{prog:?} under PATH={path:?}: {got:?}"
                );
                if prog.contains('/') && !prog.starts_with('/') {
                    assert_eq!(got, None, "{prog:?} under PATH={path:?}");
                }
            }
        }
        assert_eq!(
            found("x", Some(&abs)),
            Some(format!("{abs}/x")),
            "the control"
        );
        assert_eq!(
            found("x", Some("")),
            None,
            "an empty entry is not the folder"
        );
        assert_eq!(
            found("x", Some(&rel)),
            None,
            "a relative entry is not searched"
        );
        assert_eq!(
            found("/bin/sh", None).as_deref(),
            Some("/bin/sh"),
            "as written"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **With no `$SHELL`, Linux starts the account's login shell**: the
    /// password database's shell for this user when it is absolute, else
    /// `/bin/sh`. macOS starts its own, `/bin/zsh`, whatever the entry
    /// says. With no `$SHELL` at all, a window given the entry
    /// `/opt/fish/bin/fish` starts that, and one given none starts
    /// `/bin/sh`; a `$SHELL` that is taken never reads the entry.
    #[test]
    fn with_no_shell_the_account_s_login_shell_starts() {
        let fish = || Some("/opt/fish/bin/fish".to_string());
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                system_shell(Some("/opt/fish/bin/fish")),
                "/opt/fish/bin/fish"
            );
            assert_eq!(system_shell(Some("fish")), "/bin/sh", "not absolute");
            assert_eq!(system_shell(Some("")), "/bin/sh", "empty");
            assert_eq!(system_shell(None), "/bin/sh", "no entry");
            assert_eq!(
                login_shell_of(None, &env_of(&[]), fish).0,
                "/opt/fish/bin/fish"
            );
            assert_eq!(
                login_shell_of(None, &env_of(&[("SHELL", "zsh")]), fish),
                ("/opt/fish/bin/fish".to_string(), Some("zsh".to_string())),
                "refused, and the entry starts"
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(system_shell(Some("/opt/fish/bin/fish")), "/bin/zsh");
            assert_eq!(login_shell_of(None, &env_of(&[]), fish).0, "/bin/zsh");
        }
        assert_eq!(login_shell_of(None, &env_of(&[]), || None).0, DEFAULT_SHELL);
        assert_eq!(
            login_shell_of(
                None,
                &env_of(&[("SHELL", "/bin/sh")]),
                || -> Option<String> { panic!("the entry is read though $SHELL was taken") }
            ),
            ("/bin/sh".to_string(), None)
        );
    }

    /// **The account's entry is the password database's**: what
    /// [`passwd_shell`] reads for this user is the seventh field
    /// `getent passwd` prints for `id -u`.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_account_s_entry_is_the_password_database_s() {
        let run = |p: &str, a: &[&str]| {
            let o = std::process::Command::new(p).args(a).output().expect(p);
            assert!(o.status.success(), "{p} {a:?}");
            String::from_utf8(o.stdout)
                .expect("UTF-8")
                .trim_end()
                .to_string()
        };
        let uid = run("id", &["-u"]);
        let entry = run("getent", &["passwd", &uid]);
        let shell = entry.split(':').nth(6).expect("a seventh field");
        assert_eq!(passwd_shell().as_deref(), Some(shell));
    }

    /// The shell a window with no usable `$SHELL` starts on this host,
    /// read without baud: `getent`'s entry when absolute, else `/bin/sh`;
    /// on macOS `/bin/zsh`.
    fn account_shell() -> String {
        if cfg!(target_os = "macos") {
            return "/bin/zsh".into();
        }
        let run = |p: &str, a: &[&str]| {
            let o = std::process::Command::new(p).args(a).output().expect(p);
            String::from_utf8(o.stdout)
                .expect("UTF-8")
                .trim_end()
                .to_string()
        };
        let entry = run("getent", &["passwd", &run("id", &["-u"])]);
        match entry.split(':').nth(6) {
            Some(s) if s.starts_with('/') => s.into(),
            _ => "/bin/sh".into(),
        }
    }

    /// **A `$SHELL` that is not absolute is unset**, so the system's shell
    /// starts: POSIX has `SHELL` "represent a pathname of the user's
    /// preferred command language interpreter" (`[[source: posix-xbd-8]]`),
    /// login and launchd set it absolute, and `[shell] program` refuses
    /// anything else ([`crate::config::shell_program`]). Read again for
    /// every window, a relative one would make a folder's `bin/zsh` the
    /// shell of every window opened there. A bare `zsh` is unset too. The
    /// control: an absolute one is the shell.
    #[test]
    fn a_shell_from_the_parent_that_is_not_absolute_is_unset() {
        for shell in ["bin/zsh", "./zsh", "zsh"] {
            assert_eq!(
                login_shell(None, &env_of(&[("SHELL", shell)])),
                (account_shell(), Some(shell.to_string())),
                "SHELL={shell}, refused and said"
            );
        }
        assert_eq!(
            login_shell(None, &env_of(&[("SHELL", "")])),
            (account_shell(), None),
            "empty is unset, and nothing to say"
        );
        assert_eq!(
            login_shell(None, &env_of(&[("SHELL", "/bin/sh")])),
            ("/bin/sh".to_string(), None),
            "the control"
        );
        let exec = Exec::of(
            None,
            &env_of(&[("SHELL", "bin/zsh")]),
            crate::shell::Integration::Off,
            crate::shell::SshEnv::Off,
            None,
        )
        .expect("an exec");
        assert_eq!(
            exec.path.as_str(),
            Some(account_shell().as_str()),
            "what the child runs"
        );
    }

    /// **A window whose parent says `SHELL=bin/zsh` never runs the
    /// `bin/zsh` in the folder it starts in**: a stand-in there would
    /// write a marker and print one. The window starts the system's shell
    /// instead, told a scratch `HOME` and `ZDOTDIR` so no one's own startup
    /// files run, and `exit` ends it.
    #[test]
    fn a_relative_shell_in_the_start_folder_never_runs() {
        let dir = scratch_dir("relative-shell");
        let marker = dir.join("stand-in-ran");
        stand_in(
            &dir.join("bin/zsh"),
            &format!("#!/bin/sh\n: > '{}'\necho STAND-IN\n", marker.display()),
        );
        // A startup file in the scratch home, so a zsh the account names
        // starts as it would and reads `exit`, not its first-run menu.
        std::fs::write(dir.join(".zshrc"), "").expect("a .zshrc");
        let home = dir.to_str().expect("a UTF-8 path");
        let parent = env_of(&[
            ("SHELL", "bin/zsh"),
            ("HOME", home),
            ("ZDOTDIR", home),
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C"),
        ]);
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let (pid, master, _) = Pty::fork_given(
            text_area(&size, Px::of(8), Px::of(16)),
            None,
            parent,
            || None,
            Parent::Other,
            Some(&dir),
            &crate::shell::Integration::Off.into(),
        )
        .and_then(Result::ok)
        .expect("forkpty");
        let typed = b"exit\n";
        // SAFETY: `typed` is a live buffer of the length passed.
        unsafe { crate::sys::write(master.as_raw_fd(), typed.as_ptr().cast(), typed.len()) };
        let said = String::from_utf8_lossy(&read_all(master.as_raw_fd())).to_string();
        // SAFETY: `pid` is this test's own child.
        unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
        assert!(!said.contains("STAND-IN"), "the stand-in ran: {said:?}");
        assert!(!marker.exists(), "the stand-in's marker");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A window told to run `./only-there.sh` runs nothing**, though the
    /// folder it starts in holds one: the path meant a file where it was
    /// typed, and a `ShellStart` carrying it relative is refused at the
    /// exec door, the child ending with 127, as a program not found does,
    /// without an `execve`. The script would write a marker.
    #[test]
    fn a_relative_program_in_the_start_folder_never_runs() {
        let dir = scratch_dir("relative-program");
        let marker = dir.join("only-there-ran");
        stand_in(
            &dir.join("only-there.sh"),
            &format!("#!/bin/sh\n: > '{}'\necho ONLY-THERE\n", marker.display()),
        );
        let here = std::env::current_dir().expect("a working directory");
        assert!(
            !here.join("only-there.sh").exists(),
            "the control: not where the test is"
        );
        let start = ShellStart {
            integration: crate::shell::Integration::Off,
            ssh_env: crate::shell::SshEnv::Off,
            program: None,
            run: crate::rules::Run::given(vec![OsString::from("./only-there.sh")]),
        };
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let (pid, master, _) = Pty::fork_given(
            text_area(&size, Px::of(8), Px::of(16)),
            None,
            env_of(&[("PATH", "/usr/bin:/bin"), ("LANG", "C")]),
            || None,
            Parent::Other,
            Some(&dir),
            &start,
        )
        .and_then(Result::ok)
        .expect("forkpty");
        let said = String::from_utf8_lossy(&read_all(master.as_raw_fd())).to_string();
        let mut status: core::ffi::c_int = 0;
        // SAFETY: `pid` is this test's own child, `status` a live int.
        unsafe { crate::sys::waitpid(pid, &raw mut status, 0) };
        assert!(!said.contains("ONLY-THERE"), "the script ran: {said:?}");
        assert!(!marker.exists(), "the script's marker");
        assert_eq!((status >> 8) & 0xff, 127, "ended as a program not found");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fresh directory of this test's own, by its resolved path.
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("baud-start-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("a scratch directory");
        d.canonicalize().expect("it resolves")
    }

    /// **A shell spawned under launchd starts in the home directory**, and one
    /// spawned under any other parent starts where baud was started: Ghostty's
    /// `getppid()` test. The whole spawn, [`Pty::fork`], runs in
    /// [`spawn_helper`] twice, from the same directory with the same `HOME`:
    /// once as this test's child, and once as an orphan, whose parent is then
    /// launchd for real -- which is what a Dock or Finder launch's parent is
    /// (measured: ppid 1 under `open`). The two
    /// directories differ, so either answer can be told from the other.
    ///
    /// `HOME` is a symbolic link to the directory, as `/var` is on macOS,
    /// so the shell's `PWD` -- which a POSIX shell keeps from its
    /// environment only when it names the working directory (`[[source:
    /// posix-xcu-2]]`, 2.5.3) -- says whether baud gave the shell `HOME` as
    /// its `PWD`, by the link's spelling, or the shell found its own, by
    /// the resolved one.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_shell_spawned_under_launchd_starts_home() {
        let real = scratch_dir("home");
        let link = scratch_dir("link").join("home");
        std::os::unix::fs::symlink(&real, &link).expect("a link to home");
        let launch = scratch_dir("launch");
        assert_ne!(real, launch, "the control: two directories to tell apart");
        let exe = std::env::current_exe().expect("this test");
        let run = |how: &str| -> (String, Vec<String>) {
            let mut cmd = match how {
                "orphan" => {
                    let mut c = std::process::Command::new("/bin/sh");
                    c.args([
                        "-c",
                        "\"$0\" --exact pty::tests::spawn_helper --nocapture & exit 0",
                    ])
                    .arg(&exe);
                    c
                }
                _ => {
                    let mut c = std::process::Command::new(&exe);
                    c.args(["--exact", "pty::tests::spawn_helper", "--nocapture"]);
                    c
                }
            };
            cmd.current_dir(&launch)
                .env("HOME", &link)
                .env("PWD", &launch)
                .env(
                    "BAUD_CMD",
                    format!("{} -P; printf 'PWD=%s\\n' \"$PWD\"", tool("pwd")),
                )
                .env(SPAWN_HELPER, how);
            let out = cmd.output().expect("the helper runs");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "the helper passed: {text}");
            let ppid = text
                .lines()
                .find_map(|l| l.strip_prefix("PPID[")?.strip_suffix(']'))
                .unwrap_or_else(|| panic!("the helper spawned nothing: {text}"))
                .to_string();
            let child = text
                .lines()
                .filter_map(|l| l.strip_prefix("CHILD[")?.strip_suffix(']'))
                .map(str::to_string)
                .collect();
            (ppid, child)
        };
        let (ppid, child) = run("child");
        assert_eq!(ppid, std::process::id().to_string(), "the control's parent");
        assert_eq!(
            child,
            [
                launch.display().to_string(),
                format!("PWD={}", launch.display())
            ],
            "under this test, the launch directory"
        );
        let (ppid, child) = run("orphan");
        assert_eq!(ppid, "1", "the orphan's parent is launchd");
        assert_eq!(
            child,
            [
                real.display().to_string(),
                format!("PWD={}", link.display())
            ],
            "under launchd, home"
        );
        for d in [real, link.parent().expect("its directory").into(), launch] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// **Both drivers follow the encoding straight after they feed the
    /// child's output.** The pumps are bound to AppKit and to Wayland, so
    /// the source is held: in each, the statement after the one
    /// `reads::feed` of a read is the follow, so no key can be encoded
    /// between a switch and the bit that follows it.
    #[test]
    fn both_drivers_follow_the_encoding_straight_after_the_feed() {
        let next_after = |src: &str, feed: &str, follow: &str| {
            assert_eq!(src.matches(feed).count(), 1, "one feed: {feed}");
            assert_eq!(src.matches(follow).count(), 1, "one follow: {follow}");
            let rest = &src[src.find(feed).expect("found") + feed.len()..];
            let next = rest
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty() && !l.starts_with("//"))
                .expect("a statement after the feed");
            assert_eq!(next, follow);
        };
        // Each callback ends taking the notifications, and the log is the
        // feed's last argument.
        let end = "t.clear_notices();\n            },\n";
        next_after(
            include_str!("app.rs"),
            &format!("{end}            hostlog,\n            &mut took,\n        );\n"),
            "self.app.pty.follow(self.app.term.input_encoding());",
        );
        next_after(
            include_str!("linux.rs"),
            &format!("{end}            &mut self.hostlog,\n            &mut took,\n        );\n"),
            "self.pty.follow(self.term.input_encoding());",
        );
    }

    /// A pseudo-terminal no session has held: its manager, owned. `posix_openpt` with `O_NOCTTY`, so this process does
    /// not take it either, and `O_CLOEXEC`, so no child another test forks
    /// holds it ([`HOLDS`]).
    fn unheld_pty() -> std::os::fd::OwnedFd {
        let flags = crate::sys::O_RDWR | crate::sys::O_NOCTTY | crate::sys::O_CLOEXEC;
        let opening = budget::opening();
        // SAFETY: plain calls on the descriptor the first returns.
        unsafe {
            let m = redriven(|| crate::sys::posix_openpt(flags));
            assert!(m >= 0, "posix_openpt");
            assert_eq!(crate::sys::grantpt(m), 0, "grantpt");
            assert_eq!(crate::sys::unlockpt(m), 0, "unlockpt");
            drop(opening);
            budget::opened();
            <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(m)
        }
    }

    /// Whether a descriptor of this process is the manager of the pty whose
    /// subsidiary is `name`: `ptsname` answers only for a manager.
    fn holds_manager_of(name: &std::path::Path) -> bool {
        crate::inherited::open_from_three()
            .into_iter()
            .any(|fd| subsidiary_of(fd).as_deref() == Some(name))
    }

    /// **A test that fails while it holds a pty lets the pty go.** Under
    /// `cargo test` every test is a thread of one process, so a manager a
    /// failed test leaves open stays open for the rest of the run, and
    /// counts against the machine's ptys (`kern.tty.ptmx_max`, 511 on a
    /// stock Mac) and the suite's budget (`pty/budget.rs`). Each helper
    /// that opens one hands it back owned, closed when it drops, and an
    /// unwinding test drops it. Run in a process of its own
    /// ([`lets_go_helper`]): a pty another test opens beside it can take
    /// the name of the one this one let go.
    #[test]
    fn a_test_that_fails_holding_a_pty_lets_it_go() {
        let out = std::process::Command::new(std::env::current_exe().expect("this test"))
            .args(["--exact", "pty::tests::lets_go_helper", "--nocapture"])
            .env(LETS_GO_HELPER, "1")
            .output()
            .expect("the helper runs");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        let errors = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "the helper passed: {text}{errors}");
        for what in LETS_GO_OPENERS {
            assert!(
                text.contains(&format!("LET GO[{what}]")),
                "{what} was checked: {text}"
            );
        }
    }

    const LETS_GO_HELPER: &str = "BAUD_TEST_LETS_GO_HELPER";
    const LETS_GO_OPENERS: [&str; 3] = ["a pty no child holds", "libc's forkpty", "baud's spawn"];

    /// **Run only by [`a_test_that_fails_holding_a_pty_lets_it_go`]**, as a
    /// process of its own; a no-op otherwise. Each helper opens a pty in a
    /// test that then fails, and after it no descriptor of this process is
    /// that pty's manager.
    #[test]
    fn lets_go_helper() {
        if std::env::var_os(LETS_GO_HELPER).is_none() {
            return;
        }
        let env = child_env(outer_parent(), None);
        let child = std::sync::Mutex::new(None);
        let openers: [&dyn Fn() -> std::os::fd::OwnedFd; 3] = [
            &unheld_pty,
            &|| bare_fork(grid_80x24(), "exit 0", &env),
            &|| {
                let (pid, master) =
                    Pty::fork_child(grid_80x24(), Some("exit 0".into()), env.clone())
                        .expect("a spawn");
                *child.lock().expect("the child") = Some(pid);
                master
            },
        ];
        for (what, open) in LETS_GO_OPENERS.into_iter().zip(openers) {
            // What the probe answered while the master was held, read out of
            // the failing test: an assertion inside it would fail as the
            // test is meant to, and pass here unseen.
            let named = std::sync::Mutex::new(None);
            let probed = std::sync::Mutex::new((false, true));
            let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let master = open();
                let name = subsidiary_of(master.as_raw_fd());
                *probed.lock().expect("the probe") = (
                    name.as_deref().is_some_and(holds_manager_of),
                    holds_manager_of(std::path::Path::new("/")),
                );
                *named.lock().expect("the name") = name;
                panic!("a test failing while it holds its pty");
            }));
            if let Some(pid) = child.lock().expect("the child").take() {
                // SAFETY: this test's own child, waited for once.
                unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
            }
            assert!(unwound.is_err(), "{what}: the test failed");
            let name = named.into_inner().expect("the name").expect("a subsidiary");
            let (found, stray) = probed.into_inner().expect("the probe");
            assert!(
                found,
                "{what}: the probe finds the manager while it is held"
            );
            assert!(!stray, "{what}: the probe can say no");
            assert!(
                !holds_manager_of(&name),
                "{what}: the manager of {} is still open after the test that held it failed",
                name.display()
            );
            println!("LET GO[{what}]");
        }
    }

    /// **The parent never takes the subsidiary as its controlling
    /// terminal.** A session leader with none -- an app the Dock started --
    /// takes a terminal it opens without `O_NOCTTY` when no session holds
    /// that terminal, and with it the terminal's hang-up. A process in a
    /// new session ([`ctty_helper`]) follows the encoding on the subsidiary
    /// of a pty no session holds and has no terminal after. The control
    /// opens the same path plainly and has it as its terminal after, so the
    /// probe can fire. This process keeps the subsidiary open across both,
    /// with `O_NOCTTY`, as the child keeps it in the app: on Darwin the
    /// helper's close is otherwise the last, and the last close lets the
    /// terminal go, so a control without the hold read nothing taken. Not
    /// a `forkpty` pty: its child's session holds the subsidiary from the
    /// start, and on Darwin that hold outlives the child -- a plain open
    /// after the child had exited, reaped or not, took nothing.
    #[test]
    fn the_parent_never_takes_the_subsidiary_as_its_terminal() {
        let took = |how: &str| {
            let master = unheld_pty();
            let path = subsidiary_of(master.as_raw_fd()).expect("a subsidiary");
            let held = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags_noctty()
                .open(&path)
                .expect("the subsidiary opens");
            let out = std::process::Command::new(std::env::current_exe().expect("this test"))
                .args(["--exact", "pty::tests::ctty_helper", "--nocapture"])
                .env(CTTY_HELPER, format!("{}\t{how}", path.display()))
                .output()
                .expect("the helper runs");
            drop(held);
            drop(master);
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            assert!(out.status.success(), "the helper passed: {text}");
            match (text.contains("CTTY[true]"), text.contains("CTTY[false]")) {
                (true, false) => (true, text),
                (false, true) => (false, text),
                _ => panic!("the helper ran once and said: {text}"),
            }
        };
        let (plain, said) = took("plain");
        assert!(plain, "the control: a plain open takes it: {said}");
        let (follow, said) = took("follow");
        assert!(!follow, "{said}");
    }

    /// **A parent's `LANG=` then `LANG=en_US.UTF-8` is an empty `LANG` to
    /// the child, so the locale is filled**: `getenv` reads the first entry
    /// of a name, and whether the parent names a locale is asked of that
    /// one. The child is given `LANG` once, the fill. The control: a first
    /// `LANG` that is set is kept, and nothing is filled.
    #[test]
    fn a_first_lang_that_is_empty_is_filled_though_a_later_one_is_set() {
        let parent = env_of(&[("LANG", ""), ("HOME", "/h"), ("LANG", "en_US.UTF-8")]);
        assert_eq!(
            langs(&child_env(parent.clone(), Some("he_IL.UTF-8"))),
            ["LANG=he_IL.UTF-8"]
        );
        let called = std::cell::Cell::new(false);
        let env = spawn_env(parent, || {
            called.set(true);
            Some("he_IL.UTF-8".into())
        });
        assert!(called.get(), "the spawn asks for a locale");
        assert_eq!(langs(&env.into_envp()), ["LANG=he_IL.UTF-8"]);
        let set = env_of(&[("LANG", "fr_FR.UTF-8"), ("LANG", "")]);
        assert_eq!(
            langs(&child_env(set, Some("he_IL.UTF-8"))),
            ["LANG=fr_FR.UTF-8"],
            "the control"
        );
    }

    /// **A name the parent repeats reaches the child once, as its first
    /// value**, the one `getenv` reads; an entry with an empty name, or a
    /// name holding `=`, no environment can carry, and it is dropped.
    #[test]
    fn a_repeated_name_reaches_the_child_once_as_its_first_value() {
        let mut parent = env_of(&[("A", "1"), ("B", "x"), ("A", "2")]);
        parent.push((OsString::new(), OsString::from("X")));
        parent.push((OsString::from("C=D"), OsString::from("e")));
        let got = strings(&child_env(parent, None));
        let named = |p: &str| {
            got.iter()
                .filter(|e| e.starts_with(p))
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(named("A="), ["A=1"]);
        assert_eq!(named("B="), ["B=x"]);
        assert!(
            !got.iter()
                .any(|e| e.starts_with('=') || e.starts_with("C=")),
            "{got:?}"
        );
    }

    /// **Every parent of up to four entries over the locale's names, a
    /// plain name and an empty or set value** -- 4,681 of them, each
    /// repeat and order -- gives a child with each name once, no name
    /// holding `=`, and the `LANG` the child would read: the parent's first
    /// when the first `LC_ALL`, `LC_CTYPE` or `LANG` names a locale, else
    /// the fill.
    #[test]
    fn every_small_parent_gives_each_name_once_and_the_lang_getenv_reads() {
        let entries: Vec<(&str, &str)> = ["LANG", "LC_ALL", "LC_CTYPE", "A"]
            .into_iter()
            .flat_map(|k| [(k, ""), (k, "en_US.UTF-8")])
            .collect();
        let mut parents: Vec<Vec<(&str, &str)>> = vec![Vec::new()];
        let mut cases = 0usize;
        for _ in 0..=4 {
            let mut longer = Vec::new();
            for parent in &parents {
                cases += 1;
                let first = |k: &str| parent.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
                let names = ["LC_ALL", "LC_CTYPE", "LANG"]
                    .iter()
                    .any(|l| first(l).is_some_and(|v| !v.is_empty()));
                let got = strings(&child_env(env_of(parent), Some("he_IL.UTF-8")));
                let mut seen: Vec<&str> = got
                    .iter()
                    .map(|e| e.split_once('=').expect("name=value").0)
                    .collect();
                let n = seen.len();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(seen.len(), n, "a name twice for {parent:?}: {got:?}");
                let want: Vec<String> = match (names, first("LANG")) {
                    (true, Some(v)) => vec![format!("LANG={v}")],
                    (true, None) => Vec::new(),
                    (false, _) => vec!["LANG=he_IL.UTF-8".to_string()],
                };
                assert_eq!(
                    langs(&child_env(env_of(parent), Some("he_IL.UTF-8"))),
                    want,
                    "{parent:?}"
                );
                for e in &entries {
                    let mut p = parent.clone();
                    p.push(*e);
                    longer.push(p);
                }
            }
            parents = longer;
        }
        assert_eq!(cases, 4681, "every parent of up to four entries");
    }

    /// `outer_parent()` with `extra` added.
    fn parent_with(extra: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        let mut parent = outer_parent();
        parent.extend(
            extra
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        );
        parent
    }

    fn langs(env: &[CString]) -> Vec<String> {
        strings(env)
            .into_iter()
            .filter(|e| e.starts_with("LANG="))
            .collect()
    }

    /// **A shell that inherited no locale is given the one baud found**,
    /// once, beside everything else as before. The name is `he_IL.UTF-8` so
    /// that neither this Mac's locale nor a test runner's own `LANG` can pass
    /// it by accident.
    #[test]
    fn a_child_with_no_locale_is_given_the_one_baud_found() {
        let mut want = ours();
        want.push("BAUD_KEPT=yes".into());
        want.push("HOME=/nowhere".into());
        want.push("LANG=he_IL.UTF-8".into());
        want.sort();
        assert_eq!(
            strings(&child_env(outer_parent(), Some("he_IL.UTF-8"))),
            want
        );
    }

    /// A locale the parent named wins: its `LANG`, or an `LC_ALL` or
    /// `LC_CTYPE`, which outrank `LANG`, leaves the environment as given.
    #[test]
    fn a_locale_the_parent_set_is_kept() {
        let env = child_env(parent_with(&[("LANG", "fr_FR.UTF-8")]), Some("he_IL.UTF-8"));
        assert_eq!(langs(&env), ["LANG=fr_FR.UTF-8"]);
        for (k, v) in [("LC_ALL", "he_IL.UTF-8"), ("LC_CTYPE", "UTF-8")] {
            let env = child_env(parent_with(&[(k, v)]), Some("he_IL.UTF-8"));
            assert!(
                langs(&env).is_empty(),
                "{k} named a locale: {:?}",
                strings(&env)
            );
            assert!(strings(&env).contains(&format!("{k}={v}")), "{k} kept");
        }
    }

    /// An empty variable is no locale: an empty `LANG` is replaced, never
    /// doubled -- `getenv` reads the first entry, which would be the empty
    /// one -- and empty `LC_ALL` and `LC_CTYPE` do not stop the fill.
    #[test]
    fn an_empty_locale_variable_is_replaced_not_duplicated() {
        let env = child_env(parent_with(&[("LANG", "")]), Some("he_IL.UTF-8"));
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
        let env = child_env(
            parent_with(&[("LC_ALL", ""), ("LC_CTYPE", "")]),
            Some("he_IL.UTF-8"),
        );
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
    }

    /// iTerm2's `LC_TERMINAL` and `LC_TERMINAL_VERSION` name a terminal, not
    /// a locale: they go as outer identity and do not stop the fill.
    #[test]
    fn lc_terminal_is_a_terminal_not_a_locale() {
        let env = child_env(
            parent_with(&[("LC_TERMINAL", "iTerm2"), ("LC_TERMINAL_VERSION", "3.5")]),
            Some("he_IL.UTF-8"),
        );
        assert_eq!(langs(&env), ["LANG=he_IL.UTF-8"]);
        assert!(!strings(&env).iter().any(|e| e.starts_with("LC_TERMINAL")));
    }

    /// The spawn asks for a locale only when the parent named none -- the
    /// answer is CoreFoundation's, read from the user's preferences -- fills
    /// it then, and leaves the environment as before when none is found. An
    /// empty `LANG` and `LC_TERMINAL` alone name none; `LC_ALL` and
    /// `LC_CTYPE` do.
    #[test]
    fn the_spawn_asks_for_a_locale_only_when_the_parent_named_none() {
        let asked = |parent: Vec<(OsString, OsString)>| {
            let called = std::cell::Cell::new(false);
            let env = spawn_env(parent, || {
                called.set(true);
                Some("he_IL.UTF-8".into())
            });
            (called.get(), langs(&env.into_envp()))
        };
        let filled = vec!["LANG=he_IL.UTF-8".to_string()];
        assert_eq!(
            asked(parent_with(&[("LANG", "fr_FR.UTF-8")])),
            (false, vec!["LANG=fr_FR.UTF-8".to_string()])
        );
        assert_eq!(
            asked(parent_with(&[("LC_ALL", "he_IL.UTF-8")])),
            (false, vec![])
        );
        assert_eq!(
            asked(parent_with(&[("LC_CTYPE", "UTF-8")])),
            (false, vec![])
        );
        assert_eq!(asked(outer_parent()), (true, filled.clone()));
        assert_eq!(asked(parent_with(&[("LANG", "")])), (true, filled.clone()));
        assert_eq!(
            asked(parent_with(&[("LC_TERMINAL", "iTerm2")])),
            (true, filled)
        );
        assert_eq!(
            strings(&spawn_env(outer_parent(), || None).into_envp()),
            strings(&child_env(outer_parent(), None))
        );
    }

    /// The names a shell's locale is tried as: the user's identifier
    /// without its `@` keywords, their language and region, then `en_US`
    /// and `C`, each as UTF-8 and each once. The first the system has wins;
    /// none, nothing.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_candidates_run_from_the_users_locale_to_the_fallbacks() {
        assert_eq!(
            locale_candidates(Some("he_IL"), Some("he"), Some("IL")),
            ["he_IL.UTF-8", "en_US.UTF-8", "C.UTF-8"]
        );
        assert_eq!(
            locale_candidates(Some("en_US@rg=ilzzzz"), Some("en"), Some("IL")),
            ["en_US.UTF-8", "en_IL.UTF-8", "C.UTF-8"]
        );
        assert_eq!(
            locale_candidates(None, None, None),
            ["en_US.UTF-8", "C.UTF-8"]
        );
        let he_us = locale_candidates(Some("he_US"), Some("he"), Some("US"));
        assert_eq!(
            first_valid(&he_us, |n| n != "he_US.UTF-8"),
            Some("en_US.UTF-8".into())
        );
        assert_eq!(first_valid(&he_us, |_| false), None);
    }

    /// Only a UTF-8 locale the system has is handed over. Refused: an empty
    /// name (which `newlocale` reads as the environment's), C and POSIX
    /// (ASCII), names the system lacks, a bare `en_US` (no codeset), and
    /// `UTF-8`, which the character type alone would accept. The positive
    /// controls are names every Mac has.
    #[cfg(target_os = "macos")]
    #[test]
    fn only_a_utf8_locale_the_system_has_is_accepted() {
        for bad in [
            "",
            "C",
            "POSIX",
            "bogus_XX.UTF-8",
            "en_IL.UTF-8",
            "en_US",
            "UTF-8",
            "en_US.utf8",
        ] {
            assert!(!is_utf8_locale(bad), "{bad:?} was accepted");
        }
        for good in ["en_US.UTF-8", "he_IL.UTF-8", "C.UTF-8"] {
            assert!(is_utf8_locale(good), "{good} was refused");
        }
        let he_us = locale_candidates(Some("he_US"), Some("he"), Some("US"));
        assert_eq!(
            first_valid(&he_us, is_utf8_locale),
            Some("en_US.UTF-8".into())
        );
    }

    /// On this Mac the user's locale is found, and it is one the system has.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_users_locale_is_found_and_is_utf8() {
        let found = shell_locale().expect("en_US.UTF-8 and C.UTF-8 are on every Mac");
        assert!(is_utf8_locale(&found), "{found}");
    }

    /// Everything a forked child writes, until it exits.
    fn read_all(master: crate::sys::Fd) -> Vec<u8> {
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            // SAFETY: `buf` is a live, writable buffer of the length passed.
            let n = unsafe { crate::sys::read(master, buf.as_mut_ptr().cast(), buf.len()) };
            let Ok(n) = usize::try_from(n) else { break };
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
        }
        out
    }

    /// **A shell a pty forks counts a Hebrew letter as one character**, and
    /// names the locale it was given -- through the whole spawn,
    /// [`Pty::fork_from`], from a parent that named none. Without a locale
    /// (the control) the same command counts the letter's two bytes, so the
    /// probe can fire.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_shell_spawned_without_a_locale_counts_a_hebrew_letter_as_one() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |locale: Option<&str>| {
            let locale = locale.map(str::to_string);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some(r#"printf '%s|' "$LANG"; printf '\327\231' | /usr/bin/wc -m"#.into()),
                outer_parent(),
                move || locale,
                Parent::Other,
                None,
            )
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master.as_raw_fd())).replace(char::is_whitespace, "")
        };
        assert_eq!(run(Some("he_IL.UTF-8")), "he_IL.UTF-8|1");
        assert_eq!(
            run(None),
            "|2",
            "the control: with no locale the letter is two bytes"
        );
    }

    /// The parent a `[shell] program` test's baud has: `SHELL` naming
    /// zsh, a scratch `HOME` and `TMPDIR` under `dir` (where integration
    /// writes its scripts), `HISTFILE` at `/dev/null`, and this `PATH`.
    fn program_parent(dir: &std::path::Path) -> Vec<(OsString, OsString)> {
        let os = |s: &str| OsString::from(s);
        for sub in ["home", "tmp"] {
            std::fs::create_dir_all(dir.join(sub)).expect("a scratch directory");
        }
        vec![
            (os("HOME"), dir.join("home").into_os_string()),
            (os("TMPDIR"), dir.join("tmp").into_os_string()),
            (os("HISTFILE"), os("/dev/null")),
            (os("PATH"), std::env::var_os("PATH").unwrap_or_default()),
            (os("LANG"), os("en_US.UTF-8")),
            (os("SHELL"), os("/bin/zsh")),
        ]
    }

    fn argv(e: &Exec) -> Vec<String> {
        e.argv
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    /// **A configured program is run as `$SHELL` is, in its place**: its
    /// path is what `execve` takes, argv[0] is its name after a `-`, the
    /// integration its name picks is added -- bash's `--posix`, fish's
    /// `-C`, zsh's `ZDOTDIR`, nothing for a shell baud does not mark --
    /// and it is the child's `SHELL`, while the parent's `SHELL` names zsh.
    #[test]
    fn a_configured_program_runs_in_place_of_shell() {
        let dir = std::env::temp_dir().join(format!("baud-program-exec-{}", std::process::id()));
        let parent = program_parent(&dir);
        let on = crate::shell::Integration::On;
        let bash = Exec::of(
            None,
            &parent,
            on,
            crate::shell::SshEnv::Off,
            Some("/opt/shells/bin/bash"),
        )
        .expect("an exec");
        assert_eq!(bash.path.as_str(), Some("/opt/shells/bin/bash"));
        assert_eq!(argv(&bash), ["-bash", "--posix"]);
        assert_eq!(
            bash.shell_var.as_deref(),
            Some(OsStr::new("/opt/shells/bin/bash"))
        );
        let fish = Exec::of(
            None,
            &parent,
            on,
            crate::shell::SshEnv::Off,
            Some("/opt/shells/bin/fish"),
        )
        .expect("an exec");
        assert_eq!(argv(&fish)[..2], ["-fish", "-C"]);
        let zsh = Exec::of(
            None,
            &parent,
            on,
            crate::shell::SshEnv::Off,
            Some("/opt/shells/bin/zsh"),
        )
        .expect("an exec");
        assert_eq!(argv(&zsh), ["-zsh"]);
        assert!(
            zsh.injection
                .as_ref()
                .is_some_and(|i| i.env.iter().any(|(k, _)| k.as_os_str() == "ZDOTDIR")),
            "zsh's integration"
        );
        let nu = Exec::of(
            None,
            &parent,
            on,
            crate::shell::SshEnv::Off,
            Some("/opt/shells/bin/nu"),
        )
        .expect("an exec");
        assert_eq!(argv(&nu), ["-nu"]);
        assert!(nu.injection.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **Unset, the shell is `$SHELL`, else the system's**, as before the
    /// setting, and the child's `SHELL` is the parent's, untouched. A
    /// command (`BAUD_CMD`) runs as it is, the program or not.
    #[test]
    fn unset_the_shell_is_shell_then_the_systems() {
        let dir = std::env::temp_dir().join(format!("baud-program-unset-{}", std::process::id()));
        let parent = program_parent(&dir);
        let off = crate::shell::Integration::Off;
        let e = Exec::of(None, &parent, off, crate::shell::SshEnv::Off, None).expect("an exec");
        assert_eq!(
            (e.path.as_str(), argv(&e)),
            (Some("/bin/zsh"), vec!["-zsh".to_string()])
        );
        assert!(e.shell_var.is_none());
        let bare: Vec<_> = parent
            .iter()
            .filter(|(k, _)| k != "SHELL")
            .cloned()
            .collect();
        let e = Exec::of(None, &bare, off, crate::shell::SshEnv::Off, None).expect("an exec");
        assert_eq!(e.path.as_str(), Some(account_shell().as_str()));
        let c = Exec::of(
            Some("true".into()),
            &parent,
            off,
            crate::shell::SshEnv::Off,
            Some("/opt/shells/bin/bash"),
        )
        .expect("an exec");
        assert_eq!(argv(&c), ["sh", "-c", "true"]);
        assert!(c.shell_var.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The configured program runs on a real pty and is its own
    /// `SHELL`**: a script at the configured path prints `$0` and
    /// `$SHELL`, while the parent's `SHELL` names another script that
    /// prints the same. Unset (the control), the parent's runs and its
    /// `SHELL` is the parent's, so the probe reads the variable it means.
    /// No shell's startup files run: both are `/bin/sh` scripts.
    #[test]
    fn the_configured_program_is_the_childs_shell() {
        let dir = std::env::temp_dir().join(format!("baud-program-pty-{}", std::process::id()));
        let mut parent = program_parent(&dir);
        // Written by `sh`, so no descriptor of the file is ever open here: a
        // file this process wrote stays open for writing in a child another
        // test thread forked meanwhile until that child execs, and Linux
        // refuses to run a file open for writing (ETXTBSY).
        let script = |name: &str| {
            let p = dir.join(name);
            let mut sh = std::process::Command::new("/bin/sh")
                .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
                .arg(&p)
                .stdin(std::process::Stdio::piped())
                .spawn()
                .expect("sh");
            let body = b"#!/bin/sh\nprintf 'ran=%s shell=%s\\n' \"$0\" \"$SHELL\"\n";
            std::io::Write::write_all(&mut sh.stdin.take().expect("its input"), body)
                .expect("the script");
            assert!(sh.wait().expect("sh").success(), "{}", p.display());
            p.to_str().expect("UTF-8").to_string()
        };
        let (path, theirs) = (script("myshell"), script("theirs"));
        for (k, v) in &mut parent {
            if k == "SHELL" {
                *v = OsString::from(&theirs);
            }
        }
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |parent: Vec<(OsString, OsString)>, program: Option<String>| {
            let start = ShellStart {
                integration: crate::shell::Integration::Off,
                ssh_env: crate::shell::SshEnv::Off,
                program,
                run: None,
            };
            let (_, master, _) = Pty::fork_given(
                text_area(&size, Px::of(8), Px::of(16)),
                None,
                parent,
                || None,
                Parent::Other,
                None,
                &start,
            )
            .and_then(Result::ok)
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master.as_raw_fd()))
                .trim()
                .to_string()
        };
        assert_eq!(
            run(parent.clone(), Some(path.clone())),
            format!("ran={path} shell={path}")
        );
        assert_eq!(
            run(parent, None),
            format!("ran={theirs} shell={theirs}"),
            "the control"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A window over `script` as a link's window or a shell's runs it**:
    /// the pty, its reader, and the window's watch reaping its child, each
    /// waking `wakes` as they wake the driver.
    #[cfg(target_os = "macos")]
    struct Window {
        pty: Pty,
        /// Held for its thread, which reaps the child; dropped with the
        /// window.
        _watch: crate::fg::Watch,
        wakes: std::sync::mpsc::Receiver<()>,
        pid: crate::sys::Pid,
    }

    #[cfg(target_os = "macos")]
    impl Window {
        fn of(script: &str, kind: Runs) -> Window {
            let (pid, master, told, program) = fork_script(script, kind);
            Window::over(master, pid, told, program)
        }

        /// **A window whose stream and whose child are two processes**:
        /// `stream` on the pty, and `child` beside it, the one the window
        /// reaps and decides on. Darwin keeps a session leader's
        /// controlling terminal open for as long as it lives, whatever
        /// descriptors it closes (measured: `exec 0<&- 1>&- 2>&-` then a
        /// wait, under `forkpty`, reads nothing and no end), and it revokes
        /// the terminal as the leader exits, so the two ends of one child
        /// cannot be put in either order by the child alone here.
        fn split(stream: &str, child: &str) -> Window {
            let (_, master, told, program) = fork_script(stream, Runs::Link);
            // Reaped by the window's watch, the thing under test.
            #[expect(clippy::zombie_processes, reason = "the watch reaps it")]
            let child = std::process::Command::new("/bin/sh")
                .args(["-c", child])
                .spawn()
                .expect("the window's child");
            let pid = crate::sys::Pid::try_from(child.id()).expect("a pid");
            Window::over(master, pid, told, program)
        }

        fn over(
            master: crate::sys::Fd,
            pid: crate::sys::Pid,
            told: crate::sys::Winsize,
            program: Option<OsString>,
        ) -> Window {
            let (woke, wakes) = channel();
            let woke_too = woke.clone();
            let pty = Pty::reading(master, Some(pid), told, program, None, move || {
                let _ = woke.send(());
            });
            let watch = pty
                .watch(move || {
                    let _ = woke_too.send(());
                })
                .expect("a kqueue");
            Window {
                pty,
                _watch: watch,
                wakes,
                pid,
            }
        }

        /// The stream as the driver's drain says it, and what the window
        /// decides on it.
        fn turn(&self) -> (Stream, crate::held::Decided) {
            let stream = self.pty.drain().stream;
            let held = match stream {
                Stream::Open => crate::held::Stream::Open,
                Stream::Closed => crate::held::Stream::Closed,
            };
            (stream, self.pty.decide(held))
        }

        /// Turns, each on a wake, until `done` says the window is where the
        /// test wants it.
        fn until(
            &self,
            what: &str,
            done: impl Fn(Stream, &crate::held::Decided) -> bool,
        ) -> crate::held::Decided {
            loop {
                let (stream, decided) = self.turn();
                if done(stream, &decided) {
                    return decided;
                }
                let got = self.wakes.recv_timeout(std::time::Duration::from_secs(60));
                assert!(got.is_ok(), "no wake before {what}");
            }
        }

        /// Whether the child's pid still names a process: a zombie does.
        fn alive(&self) -> bool {
            alive(self.pid)
        }
    }

    /// What a test's window runs: a link's program, which holds its window
    /// when it fails, or a shell, which never does.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Runs {
        Link,
        Shell,
    }

    /// **`script` forked on a pty as a link's window or a shell's runs
    /// it**: its pid, the master, the size told, and the program a link's
    /// window names.
    fn fork_script(
        script: &str,
        kind: Runs,
    ) -> (
        crate::sys::Pid,
        crate::sys::Fd,
        crate::sys::Winsize,
        Option<OsString>,
    ) {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let told = text_area(&size, Px::of(8), Px::of(16));
        let argv = ["/bin/sh", "-c", script].map(OsString::from).to_vec();
        let run = crate::rules::Run::given(argv).expect("a program");
        let start = ShellStart {
            integration: crate::shell::Integration::Off,
            ssh_env: crate::shell::SshEnv::Off,
            program: None,
            run: None,
        }
        .running(run.clone());
        let parent = vec![(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))];
        let (pid, master, _) =
            Pty::fork_given(told, None, parent, || None, Parent::Other, None, &start)
                .and_then(Result::ok)
                .expect("forkpty");
        let program = (kind == Runs::Link).then(|| run.program().to_os_string());
        (pid, master.into_raw_fd(), told, program)
    }

    /// **A slot is written only where a wait answered**: [`Slot`] is this
    /// file's own type, its cell private, and nothing reads it but
    /// [`Slot::get`] and [`Slot::wait`]; inside this file the cell is set at
    /// two sites, [`reap_into`] with what its `waitpid` answered and
    /// `Slot::abandon` for a window that stopped waiting. A binary crate
    /// runs no `compile_fail`, so the sources hold it. Named break: a
    /// public `set` on [`Slot`].
    #[test]
    fn a_slot_is_written_only_where_a_wait_answered() {
        const SRC: &str = include_str!("pty.rs");
        let def = concat!("pub struct ", "Slot(");
        let at = SRC.find(def).expect("Slot is pty.rs's own type");
        assert!(
            !SRC[at + def.len()..].starts_with("pub"),
            "its cell is private"
        );
        let open = concat!("impl ", "Slot {");
        let body = &SRC[SRC.find(open).expect("its impl")..];
        let body = &body[..body.find("\n}\n").expect("its impl's end")];
        let public: Vec<&str> = body
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                l.strip_prefix("pub fn ")
                    .or_else(|| l.strip_prefix("pub(crate) fn "))
            })
            .map(|l| l.split('(').next().unwrap_or(l))
            .collect();
        assert_eq!(public, ["get", "wait"], "readers only");
        let tests = SRC
            .find(concat!("#[cfg(test)]\nmod ", "tests"))
            .unwrap_or(SRC.len());
        let sets: Vec<usize> = SRC[..tests]
            .match_indices(concat!("self.0", ".set("))
            .chain(SRC[..tests].match_indices(concat!("slot.0", ".set(")))
            .map(|(i, _)| i)
            .collect();
        let named = |f: &str| SRC.find(f).expect(f);
        let abandon = named(concat!("fn ", "abandon("));
        let reap = named(concat!("fn ", "reap_into("));
        assert_eq!(sets.len(), 2, "two writes: {sets:?}");
        for i in sets {
            let in_abandon = i > abandon && i < abandon + 400;
            let in_reap = i > reap && i < reap + 600;
            assert!(in_abandon || in_reap, "a write outside the two: {i}");
        }
    }

    /// **One `waitpid`'s answer is read as it is**: the pid and its status
    /// is an exit; 0 is a child still running; `EINTR` is asked again;
    /// `ECHILD` alone is a pid that is no child of ours; any other errno is
    /// kept as itself. Named break: fold every errno into
    /// [`crate::held::Ending::NotOurs`].
    #[test]
    fn a_waits_answer_is_read_as_it_is() {
        use crate::held::Ending;
        use crate::sys::Errno;
        let pid = 4242;
        let ending = |r: Option<Reap>| match r {
            Some(Reap::Reaped(r)) => Some(r.ending),
            Some(Reap::Running) | None => None,
        };
        let exited = ending(waited(pid, pid, 3 << 8, None));
        assert!(
            matches!(exited, Some(Ending::Exited(s)) if s.code() == Some(3)),
            "{exited:?}"
        );
        assert!(matches!(waited(pid, 0, 0, None), Some(Reap::Running)));
        assert!(
            waited(pid, -1, 0, Some(Errno(crate::sys::EINTR))).is_none(),
            "EINTR asks again"
        );
        assert_eq!(
            ending(waited(pid, -1, 0, Some(Errno(crate::sys::ECHILD)))),
            Some(Ending::NotOurs)
        );
        assert_eq!(
            ending(waited(pid, -1, 0, Some(Errno(crate::sys::EINVAL)))),
            Some(Ending::WaitFailed(Errno(crate::sys::EINVAL)))
        );
    }

    /// **A pid that is no child of ours is reaped as not ours**: pid 1,
    /// whose real `waitpid` answers `ECHILD`, goes into its slot as
    /// [`crate::held::Ending::NotOurs`], once.
    #[test]
    fn a_pid_not_ours_is_reaped_as_not_ours() {
        let slot = Slot::default();
        assert_eq!(reap_into(1, &slot), Recorded::Now);
        assert_eq!(
            slot.get().map(|r| r.ending),
            Some(crate::held::Ending::NotOurs)
        );
        assert_eq!(reap_into(1, &slot), Recorded::Not, "set once");
    }

    /// Whether `pid` names a process, a zombie included: `kill(pid, 0)`,
    /// which answers `ESRCH` once it has been reaped.
    fn alive(pid: crate::sys::Pid) -> bool {
        // SAFETY: signal 0 is only the check.
        if unsafe { crate::sys::kill(pid, 0) } == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() != Some(crate::sys::ESRCH)
    }

    /// **`pid` once it has exited, still unreaped**: `waitid` with
    /// `WNOWAIT`, which waits for the exit and leaves the zombie.
    #[cfg(target_os = "macos")]
    fn exited_unreaped(pid: crate::sys::Pid) {
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
    }

    /// A fifo in a scratch directory of its own, removed by the caller.
    fn fifo(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("baud-held-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let f = dir.join("go");
        let made = std::process::Command::new(tool("mkfifo"))
            .arg(&f)
            .status()
            .expect("mkfifo runs");
        assert!(made.success());
        (dir, f)
    }

    /// Lets a child blocked on opening `f` read a line and go on.
    fn release(f: &std::path::Path) {
        use std::io::Write;
        let mut w = std::fs::OpenOptions::new()
            .write(true)
            .open(f)
            .expect("the fifo opens once its reader is there");
        w.write_all(b"go\n").expect("written");
    }

    /// **A window's child is reaped by its watch, and its window decides
    /// on how it ended**: real children on a real pty, run as a link's
    /// window runs them. `exit 3` at once holds with its line; `exit 0`
    /// closes; `kill -SEGV` of itself holds naming the signal; the same
    /// failure in a shell window closes; and each child is gone from the
    /// process table once its window has decided -- no zombie. Named break:
    /// never reap -- no window ever decides.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_windows_child_is_reaped_by_its_watch_and_decides() {
        use crate::held::Decided;
        let ended = |script: &str, kind: Runs| {
            let w = Window::of(script, kind);
            let decided = w.until("a decision", |_, d| *d != Decided::Wait);
            assert!(!w.alive(), "{script}: reaped, no zombie");
            decided
        };
        assert_eq!(
            ended("exit 3", Runs::Link),
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        assert_eq!(ended("exit 0", Runs::Link), Decided::Close);
        assert_eq!(
            ended("ulimit -c 0; kill -SEGV $$", Runs::Link),
            Decided::Hold("sh crashed (signal 11). Press any key to close.".into())
        );
        assert_eq!(
            ended("exit 3", Runs::Shell),
            Decided::Close,
            "a shell window"
        );
    }

    /// **A child that outlives its stream does not hang its window**: the
    /// stream closes -- its process ends -- while the window's child lives
    /// on, blocked on a fifo ([`Window::split`] for why two processes).
    /// The window waits, deciding nothing; once the fifo lets the child
    /// exit, its watch reaps it and the window holds. The order with the
    /// stream first. Named break: the reader waits for the child once the
    /// stream ends, as f721ba66's did -- the stream never closes while the
    /// child lives, and the first wait times out.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_that_outlives_its_stream_does_not_hang_its_window() {
        use crate::held::Decided;
        let (dir, f) = fifo("outlives");
        let w = Window::split("exit 0", &format!("read x < '{}'; exit 3", f.display()));
        let waiting = w.until("the stream closes", |s, _| s == Stream::Closed);
        assert_eq!(waiting, Decided::Wait, "the stream alone decides nothing");
        assert!(w.alive(), "the child lives on");
        release(&f);
        let decided = w.until("the child's end", |_, d| *d != Decided::Wait);
        assert_eq!(
            decided,
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        assert!(!w.alive(), "reaped");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The other order: the child is reaped while the stream stays
    /// open**, held by a process waiting on a fifo. The window sees the
    /// status first and waits on the stream; once the fifo lets the stream
    /// go, it closes and the window holds. Named break: decide on the
    /// status alone -- the window holds while the stream is open.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_reaped_before_its_stream_closes_waits_for_the_stream() {
        use crate::held::Decided;
        let (dir, f) = fifo("reaped-first");
        let w = Window::split(&format!("read x < '{}'", f.display()), "exit 3");
        // Reaped: the decision the stream's close would give is in.
        w.until("the child is reaped", |_, _| {
            w.pty.decide(crate::held::Stream::Closed) != Decided::Wait
        });
        let (stream, decided) = w.turn();
        assert_eq!(stream, Stream::Open, "the stream is held open");
        assert_eq!(decided, Decided::Wait, "the status alone decides nothing");
        assert!(!w.alive(), "the child is reaped");
        release(&f);
        let decided = w.until("the stream closes", |s, _| s == Stream::Closed);
        assert_eq!(
            decided,
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A window's watch reaps its own child and no other**: the opener's
    /// `Child` -- a program a link started -- exits first and is left a
    /// zombie for its own `try_wait`, while the window's child still runs;
    /// then the window's child ends. The window decides, and the opener's
    /// child is still there for `try_wait` to reap with its own status,
    /// though its `SIGCHLD` woke the window's watch first. Named break:
    /// `waitpid(-1, ..)` -- the watch reaps the opener's child on its
    /// signal, and its `try_wait` finds no such child.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_windows_watch_reaps_its_own_child_and_no_other() {
        use crate::held::Decided;
        let (dir, f) = fifo("own-child");
        let script = format!("read x < '{}'; exit 3", f.display());
        let w = Window::of(&script, Runs::Link);
        let mut opener = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 7"])
            .spawn()
            .expect("the opener's child");
        let pid = crate::sys::Pid::try_from(opener.id()).expect("a pid");
        exited_unreaped(pid);
        assert!(alive(pid), "the control: a zombie, not yet reaped");
        release(&f);
        let decided = w.until("the window's child ends", |_, d| *d != Decided::Wait);
        assert_eq!(
            decided,
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        let status = opener.try_wait().expect("still the opener's to reap");
        assert_eq!(status.and_then(|s| s.code()), Some(7));
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A child that exited before its watch was added is reaped at once**:
    /// its process watch answers `ESRCH`, its `SIGCHLD` was sent before the
    /// signal watch was, and the one look after them reaps it. Named break:
    /// drop that look -- nothing ever reaps it.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_child_gone_before_its_watch_is_reaped_at_once() {
        // Reaped by the window's watch, the thing under test.
        #[expect(clippy::zombie_processes, reason = "the watch reaps it")]
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 5"])
            .spawn()
            .expect("a child");
        let pid = crate::sys::Pid::try_from(child.id()).expect("a pid");
        exited_unreaped(pid);
        let slot = Slot::default();
        let watch = crate::fg::Watch::start(
            Some(crate::fg::Child {
                pid,
                slot: slot.clone(),
            }),
            crate::probe::WatchCell::default(),
            || {},
        )
        .expect("a kqueue");
        let got = slot.get().expect("reaped as the watch started");
        let crate::held::Ending::Exited(status) = got.ending else {
            panic!("reaped with its status: {:?}", got.ending);
        };
        assert_eq!(status.code(), Some(5));
        assert!(!alive(pid), "no zombie");
        drop(watch);
    }

    /// **A window closed before its child ended leaves no zombie**: a shell
    /// window waiting on its terminal is closed -- its pty dropped, which
    /// hangs the shell up, and its watch dropped -- and the watch's thread
    /// still reaps the shell once the hangup ends it. Named break: the
    /// watch's drop ends its thread at once -- the shell stays a zombie.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_window_closed_first_still_reaps_its_child() {
        let w = Window::of("read x", Runs::Shell);
        let slot = w.pty.live().ending.slot.clone();
        let pid = w.pid;
        assert!(w.alive(), "the control: the shell runs");
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = slot.wait();
            let _ = tx.send(());
        });
        drop(w);
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(60)).is_ok(),
            "the closed window's child is reaped"
        );
        assert!(!alive(pid), "no zombie");
    }

    /// **A window over `script` as the Linux loop runs one**: the pty
    /// polled, its child as a pidfd in the same poll.
    #[cfg(target_os = "linux")]
    struct Polled {
        pty: Pty,
        pid: crate::sys::Pid,
        /// A stand-in for the compositor's socket, which the loop's wait
        /// always has first.
        socket: (
            std::os::unix::net::UnixStream,
            std::os::unix::net::UnixStream,
        ),
    }

    #[cfg(target_os = "linux")]
    impl Polled {
        fn of(script: &str, kind: Runs) -> Polled {
            let (pid, master, told, program) = fork_script(script, kind);
            Polled::over(master, pid, told, program)
        }

        /// **A window whose stream and whose child are two processes**, as
        /// [`Window::split`] on macOS and for the same reason: Linux hangs
        /// the terminal up as its session leader exits (`disassociate_ctty`),
        /// so a grandchild holding it does not keep the stream open past the
        /// child.
        fn split(stream: &str, child: &str) -> Polled {
            let (_, master, told, program) = fork_script(stream, Runs::Link);
            // Reaped through its pidfd, the thing under test.
            #[expect(clippy::zombie_processes, reason = "the pidfd reaps it")]
            let child = std::process::Command::new("/bin/sh")
                .args(["-c", child])
                .spawn()
                .expect("the window's child");
            let pid = crate::sys::Pid::try_from(child.id()).expect("a pid");
            Polled::over(master, pid, told, program)
        }

        fn over(
            master: crate::sys::Fd,
            pid: crate::sys::Pid,
            told: crate::sys::Winsize,
            program: Option<OsString>,
        ) -> Polled {
            make_nonblocking(master);
            Polled {
                pty: Pty::polled(master, pid, told, program, None),
                pid,
                socket: std::os::unix::net::UnixStream::pair().expect("a pair"),
            }
        }

        /// **One turn of the loop's shape**: the wait over the master while
        /// it is polled and the pidfd while the child is unreaped, the read
        /// and the reap they say are ready, and the decision on both.
        fn turn(&mut self) -> crate::held::Decided {
            use std::os::fd::AsFd;
            let (master, child) = {
                let also: Vec<_> = self
                    .pty
                    .child_fd()
                    .map(|fd| (fd, Interest::Read))
                    .into_iter()
                    .collect();
                let (_, m, extra) = wait(
                    self.socket.0.as_fd(),
                    self.pty.polled_master(),
                    Owed::Nothing,
                    &also,
                    Some(Duration::from_secs(60)),
                )
                .expect("the wait");
                (m, extra.first().copied())
            };
            assert!(
                master == Ready::Readable || child == Some(Ready::Readable),
                "an event before the bound"
            );
            if master == Ready::Readable {
                if let Chunk::Closed = self.pty.read_chunk() {
                    self.pty.hung_up();
                }
            }
            if child == Some(Ready::Readable) {
                self.pty.reap();
            }
            self.pty.decide(self.pty.stream())
        }

        fn until(
            &mut self,
            done: impl Fn(&Polled, &crate::held::Decided) -> bool,
        ) -> crate::held::Decided {
            loop {
                let d = self.turn();
                if done(self, &d) {
                    return d;
                }
            }
        }
    }

    /// **How often a short ssh's link is read as local on Linux**, where no
    /// process watch sees a job that writes and ends between two reads
    /// (`probe::Writers`): `ssh localhost cat <file>` holding one `OSC 8`
    /// link, run `BAUD_SSH_RACE_N` times (200 by default) as a job of its
    /// own under a monitor-mode bash, a line of the shell's own before
    /// each, as a prompt is. Each read is taken as the Linux loop takes it
    /// -- the poll, [`Pty::read_chunk`], [`crate::probe::Sampler`] and the
    /// group's `TIOCGPGRP` -- and each sampled group named at once as the
    /// paths worker names it (`probe::front_of`, a group gone naming
    /// nothing, which is not local). It counts the links whose read had
    /// local groups at both samples: the race left. Needs an sshd on
    /// localhost that takes this user's key, as the VM container gives it;
    /// so it is ignored, and run there by hand.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an sshd on localhost that takes this user's key"]
    fn a_short_ssh_link_read_on_linux_is_counted() {
        use crate::probe::{Asks, Linked, Sampler, Watched};
        use std::os::fd::AsFd;
        let n: u32 = std::env::var("BAUD_SSH_RACE_N")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let dir = std::env::temp_dir().join(format!("baud-ssh-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        let link = dir.join("link");
        std::fs::write(
            &link,
            b"\x1b]8;;file://localhost/etc/hostname\x1b\\x\x1b]8;;\x1b\\\n",
        )
        .expect("the link");
        let script = dir.join("loop.sh");
        std::fs::write(
            &script,
            format!(
                "for i in $(seq {n}); do sleep 0.02; printf 'prompt\\n'; \
                 ssh -o BatchMode=yes localhost cat {}; done; printf 'end\\n'\n",
                link.display()
            ),
        )
        .expect("the script");
        let w = Polled::of(
            &format!("exec /bin/bash -m {}", script.display()),
            Runs::Shell,
        );
        let master = w.pty.reads().master;
        let mut sampler = Sampler::default();
        let mut named = std::collections::HashMap::new();
        let mut front = |g: Option<crate::sys::Pid>| -> Option<crate::paths::Front> {
            let g = g?;
            Some(*named.entry(g).or_insert_with(|| {
                let names: Vec<String> = [crate::fg::name(g), crate::fg::exe_name(g)]
                    .into_iter()
                    .flatten()
                    .collect();
                crate::probe::front_of(&names)
            }))
        };
        let (mut links, mut local, mut ssh_at) = (0u32, 0u32, 0u32);
        let mut all = Vec::new();
        loop {
            let (_, m, _) = wait(
                w.socket.0.as_fd(),
                w.pty.polled_master(),
                Owed::Nothing,
                &[],
                Some(Duration::from_secs(60)),
            )
            .expect("the wait");
            assert_eq!(m, Ready::Readable, "output before the bound");
            let bytes = match w.pty.read_chunk() {
                Chunk::Bytes(b) => b,
                Chunk::Closed => break,
            };
            let asks = sampler.read(std::time::Instant::now(), &bytes);
            let writers = match asks {
                Asks::Yes(_) => {
                    sampler.writers(master.and_then(crate::fg::group), Watched::Nothing)
                }
                Asks::No => sampler.unasked(),
            };
            all.extend_from_slice(&bytes);
            if asks != Asks::Yes(Linked::Yes) {
                continue;
            }
            let here = u32::try_from(bytes.windows(9).filter(|x| *x == b"\x1b]8;;file").count())
                .expect("a count");
            let (before, at) = (front(writers.before), front(writers.at));
            let elsewhere = Some(crate::paths::Front::Elsewhere);
            if at == elsewhere {
                ssh_at += here;
            }
            if before != elsewhere && at != elsewhere {
                local += here;
            }
            links += here;
            if holds(&all, b"end\r\n") {
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        println!("links {links}, ssh named at the read {ssh_at}, read as local {local}");
        assert_eq!(
            links,
            n,
            "every link was read: {:?}",
            String::from_utf8_lossy(&all)
        );
    }

    /// **How often a short ssh's link typed at a shell is read as local on
    /// Linux, by the samples and by the shell's marks**: an interactive
    /// login shell (`BAUD_SSH_RACE_SHELL`, bash by default) started as a
    /// window starts it, with baud's integration on unless
    /// `BAUD_SSH_RACE_INTEGRATION=off`, is typed `ssh localhost cat <file>`
    /// `BAUD_SSH_RACE_N` times (200 by default), the file holding one `OSC
    /// 8` link, each at its prompt. Each read is taken as the Linux loop
    /// takes it -- the poll, [`Pty::read_chunk`], the
    /// [`crate::probe::Sampler`] and its marks, `TIOCGPGRP` -- and each
    /// sampled group named as the paths worker names it. It counts the
    /// links read with local groups at both samples, and of those the ones
    /// no mark of the shell's made remote: the race left. Needs an sshd on
    /// localhost that takes this user's key, as the VM container gives it.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs an sshd on localhost that takes this user's key"]
    fn a_short_ssh_link_typed_at_a_shell_on_linux_is_counted() {
        use crate::probe::{Asks, Linked, Sampler, Watched};
        use std::os::fd::AsFd;
        let n: u32 = std::env::var("BAUD_SSH_RACE_N")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);
        let integration = match std::env::var("BAUD_SSH_RACE_INTEGRATION").as_deref() {
            Ok("off") => crate::shell::Integration::Off,
            _ => crate::shell::Integration::On,
        };
        let shell = std::env::var("BAUD_SSH_RACE_SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let dir = std::env::temp_dir().join(format!("baud-ssh-typed-{}", std::process::id()));
        let home = dir.join("home");
        std::fs::create_dir_all(&home).expect("a scratch home");
        for rc in [".bash_profile", ".bashrc", ".zshrc"] {
            std::fs::write(home.join(rc), "PS1='RACE> '\n").expect("a startup file");
        }
        let link = dir.join("link");
        std::fs::write(
            &link,
            b"\x1b]8;;file://localhost/etc/hostname\x1b\\x\x1b]8;;\x1b\\\n",
        )
        .expect("the link");
        let var = |k: &str| std::env::var_os(k).unwrap_or_default();
        let parent = vec![
            (OsString::from("HOME"), home.clone().into_os_string()),
            (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
            (OsString::from("SHELL"), OsString::from(&shell)),
            (OsString::from("USER"), var("USER")),
            (OsString::from("LOGNAME"), var("LOGNAME")),
        ];
        let size = WinSize {
            ws_col: 200,
            ws_row: 24,
        };
        let told = text_area(&size, Px::of(8), Px::of(16));
        let start = ShellStart {
            integration,
            ssh_env: crate::shell::SshEnv::Off,
            program: None,
            run: None,
        };
        let (pid, master, token) =
            Pty::fork_given(told, None, parent, || None, Parent::Other, None, &start)
                .and_then(Result::ok)
                .expect("forkpty");
        let master = master.into_raw_fd();
        let w = Polled::over(master, pid, told, None);
        let mut sampler = Sampler::for_shell(token.as_ref());
        let mut named = std::collections::HashMap::new();
        let elsewhere = Some(crate::paths::Front::Elsewhere);
        let (mut links, mut by_samples, mut left) = (0u32, 0u32, 0u32);
        let mut all = Vec::new();
        let prompts = |all: &[u8]| all.windows(6).filter(|x| *x == b"RACE> ").count();
        let mut out = Outbox::default();
        for i in 0..=n {
            if i > 0 {
                out.push(
                    format!("ssh -o BatchMode=yes localhost cat {}\r", link.display()).as_bytes(),
                );
                let _ = w.pty.write(&mut out);
            }
            let want = usize::try_from(i).expect("a count") + 1;
            while prompts(&all) < want {
                let (_, m, _) = wait(
                    w.socket.0.as_fd(),
                    w.pty.polled_master(),
                    Owed::Nothing,
                    &[],
                    Some(Duration::from_secs(60)),
                )
                .expect("the wait");
                assert_eq!(
                    m,
                    Ready::Readable,
                    "output before the bound: {:?}",
                    String::from_utf8_lossy(&all)
                );
                let bytes = match w.pty.read_chunk() {
                    Chunk::Bytes(b) => b,
                    Chunk::Closed => panic!("the shell ended: {:?}", String::from_utf8_lossy(&all)),
                };
                let asks = sampler.read(std::time::Instant::now(), &bytes);
                let writers = match asks {
                    Asks::Yes(_) => sampler.writers(crate::fg::group(master), Watched::Nothing),
                    Asks::No => sampler.unasked(),
                };
                all.extend_from_slice(&bytes);
                if asks != Asks::Yes(Linked::Yes) {
                    continue;
                }
                let mut front = |g: Option<crate::sys::Pid>| {
                    let g = g?;
                    Some(*named.entry(g).or_insert_with(|| {
                        let names: Vec<String> = [crate::fg::name(g), crate::fg::exe_name(g)]
                            .into_iter()
                            .flatten()
                            .collect();
                        crate::probe::front_of(&names)
                    }))
                };
                let here =
                    u32::try_from(bytes.windows(9).filter(|x| *x == b"\x1b]8;;file").count())
                        .expect("a count");
                let local = front(writers.before) != elsewhere && front(writers.at) != elsewhere;
                links += here;
                if local {
                    by_samples += here;
                    if writers.marked != crate::ran::Ran::Elsewhere {
                        left += here;
                    }
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        println!(
            "{shell}, integration {integration:?}: links {links}, read as local by the samples \
             {by_samples}, and with the shell's marks {left}"
        );
        assert_eq!(links, n, "every link was read");
    }

    /// **The Linux loop reaps a window's child by its pidfd, and the window
    /// decides on how it ended**: `exit 3` at once holds with its line,
    /// `exit 0` closes, `kill -SEGV` of itself holds naming the signal, the
    /// same failure in a shell window closes, and each child is gone from
    /// the process table once its window has decided. Named break: leave
    /// the pidfd out of the poll -- the window never decides, and the wait
    /// runs out.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_windows_child_is_reaped_by_its_pidfd_and_decides() {
        use crate::held::Decided;
        let ended = |script: &str, kind: Runs| {
            let mut w = Polled::of(script, kind);
            assert!(w.pty.child_fd().is_some(), "the control: a pidfd");
            let decided = w.until(|_, d| *d != Decided::Wait);
            assert!(!alive(w.pid), "{script}: reaped, no zombie");
            decided
        };
        assert_eq!(
            ended("exit 3", Runs::Link),
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        assert_eq!(ended("exit 0", Runs::Link), Decided::Close);
        assert_eq!(
            ended("ulimit -c 0; kill -SEGV $$", Runs::Link),
            Decided::Hold("sh crashed (signal 11). Press any key to close.".into())
        );
        assert_eq!(
            ended("exit 3", Runs::Shell),
            Decided::Close,
            "a shell window"
        );
    }

    /// **On Linux a child that closes its terminal and goes on outlives its
    /// stream, and its window waits for it**: the master hangs up while
    /// the child, blocked on a fifo, lives; the window decides nothing;
    /// once the fifo lets it exit, its pidfd says so and the window holds.
    /// The hung-up master is out of the poll by then. Named break: break
    /// the loop on the hangup, as before -- the window closes unheld with
    /// the child alive.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_child_that_outlives_its_stream_holds_its_window() {
        use crate::held::Decided;
        let (dir, f) = fifo("linux-outlives");
        let script = format!("exec 0<&- 1>&- 2>&-; read x < '{}'; exit 3", f.display());
        let mut w = Polled::of(&script, Runs::Link);
        let waiting = w.until(|w, _| w.pty.stream() == crate::held::Stream::Closed);
        assert_eq!(waiting, Decided::Wait, "the stream alone decides nothing");
        assert!(alive(w.pid), "the child lives on");
        assert!(w.pty.polled_master().is_none(), "the master left the poll");
        release(&f);
        let decided = w.until(|_, d| *d != Decided::Wait);
        assert_eq!(
            decided,
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The other order on Linux: the child is reaped while the stream
    /// stays open**, held by a process waiting on a fifo ([`Polled::split`]);
    /// the window waits on the stream, and holds once the fifo lets it go
    /// and the master hangs up. Named break: decide on the status alone --
    /// the window holds while the stream is open.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_linux_child_reaped_before_its_stream_closes_waits_for_the_stream() {
        use crate::held::Decided;
        let (dir, f) = fifo("linux-reaped-first");
        let mut w = Polled::split(&format!("read x < '{}'", f.display()), "exit 3");
        let waiting = w.until(|w, _| w.pty.child_fd().is_none());
        assert_eq!(waiting, Decided::Wait, "the status alone decides nothing");
        assert_eq!(w.pty.stream(), crate::held::Stream::Open);
        assert!(!alive(w.pid), "the child is reaped");
        release(&f);
        let decided = w.until(|_, d| *d != Decided::Wait);
        assert_eq!(
            decided,
            Decided::Hold("sh stopped with error 3. Press any key to close.".into())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A hung-up master leaves the poll, so the loop does not spin on
    /// it**: the control -- the hung-up master polled answers `POLLHUP` at
    /// once, with nothing asked of it -- and then the loop's own set over
    /// the same pty, which holds no record of it and waits out a timeout
    /// with nothing ready. Named break: keep polling the master after the
    /// hangup -- the wait answers at once on every turn.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_hung_up_master_leaves_the_poll() {
        use std::os::fd::AsFd;
        let mut w = Polled::of("exit 0", Runs::Shell);
        w.until(|w, _| w.pty.stream() == crate::held::Stream::Closed);
        let master = w.pty.as_fd().expect("a master").as_raw_fd();
        let mut hup = [crate::sys::PollFd {
            fd: master,
            events: 0,
            revents: 0,
        }];
        // SAFETY: one record and its count.
        let n = unsafe { crate::sys::poll(hup.as_mut_ptr(), 1, 0) };
        assert_eq!(n, 1, "the control: a hung-up master answers at once");
        let set = poll_set(
            w.socket.0.as_fd(),
            w.pty.polled_master(),
            Owed::Nothing,
            &[],
        );
        assert!(
            set.iter().all(|p| p.fd != master),
            "no record of the master"
        );
        let (_, m, _) = wait(
            w.socket.0.as_fd(),
            w.pty.polled_master(),
            Owed::Nothing,
            &[],
            Some(Duration::from_millis(50)),
        )
        .expect("the wait");
        assert_eq!(m, Ready::Idle, "nothing ready: the wait ran out");
    }

    /// **With no pidfd the hangup is looked at once and then once more**:
    /// a child that outlived its stream is not reaped at the hangup, a
    /// second look is owed [`REAP_AGAIN`] after it, and that look, the
    /// child still running, closes the window with no status -- the
    /// system's to reap once the window's process ends. Named break: no
    /// second look -- nothing is owed, and the window waits for ever.
    #[cfg(target_os = "linux")]
    #[test]
    fn with_no_pidfd_a_hung_up_childs_end_is_looked_at_twice() {
        use crate::held::Decided;
        let (dir, f) = fifo("linux-no-pidfd");
        let script = format!("exec 0<&- 1>&- 2>&-; read x < '{}'; exit 3", f.display());
        let mut w = Polled::of(&script, Runs::Link);
        w.pty.live_mut().pidfd = None;
        let before = std::time::Instant::now();
        let waiting = w.until(|w, _| w.pty.stream() == crate::held::Stream::Closed);
        assert_eq!(waiting, Decided::Wait);
        let owed = w.pty.reap_owed().expect("a second look is owed");
        assert!(owed >= before + REAP_AGAIN, "after the hangup's look");
        w.pty.give_up();
        assert_eq!(w.pty.decide(w.pty.stream()), Decided::Close);
        assert_eq!(w.pty.reap_owed(), None, "nothing more is owed");
        // Given up on while it runs, which its slot says, and not that
        // something else reaped it.
        assert_eq!(
            w.pty.live().ending.slot.get().map(|r| r.ending),
            Some(crate::held::Ending::Abandoned)
        );
        assert!(alive(w.pid), "the child given up on still runs");
        release(&f);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A window's program runs on a real pty as its argv, with no shell
    /// between**: each element one argument whatever it holds -- a space, a
    /// `--`, a `$`, a `;` and backquotes reach `printf` as written, where a
    /// shell would have split, expanded or run them -- in the environment a
    /// shell window gets, baud's `TERM` and `TERM_PROGRAM` in it and the
    /// outer terminal's gone, and `SHELL` the configured shell, as a shell
    /// window's is, with nothing of shell integration's start; a
    /// `BAUD_CMD`-style command and the configured shell given beside it are
    /// not run. Named breaks: build the exec as `/bin/sh -c` over the
    /// elements joined, `BAUD_CMD`'s way -- the arguments come out split and
    /// expanded; leave `SHELL` at the parent's -- the configured one is absent.
    #[test]
    fn a_windows_program_runs_its_argv_with_no_shell() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let run = |argv: &[&str]| {
            let given = argv.iter().map(OsString::from).collect();
            let start = ShellStart {
                integration: crate::shell::Integration::On,
                ssh_env: crate::shell::SshEnv::Off,
                program: Some("/bin/zsh".into()),
                run: None,
            }
            .running(crate::rules::Run::given(given).expect("a program"));
            let parent = vec![
                (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
                (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
                (
                    OsString::from("TERM_PROGRAM"),
                    OsString::from("Apple_Terminal"),
                ),
                (OsString::from("TERM_SESSION_ID"), OsString::from("outer")),
                (OsString::from("x"), OsString::from("expanded")),
            ];
            let (_, master, _) = Pty::fork_given(
                text_area(&size, Px::of(8), Px::of(16)),
                Some("echo a shell ran".into()),
                parent,
                || None,
                Parent::Other,
                None,
                &start,
            )
            .and_then(Result::ok)
            .expect("forkpty");
            String::from_utf8_lossy(&read_all(master.as_raw_fd())).replace("\r\n", "\n")
        };
        let out = run(&[&tool("printf"), "%s|", "a b", "--", "$x;`y`", "{url}"]);
        assert_eq!(out, "a b|--|$x;`y`|{url}|");
        let env = run(&[&tool("env")]);
        assert!(env.contains("TERM_PROGRAM=baud\n"), "{env}");
        assert!(
            env.contains(&format!("TERM={}\n", crate::bell::TERM)),
            "{env}"
        );
        assert!(
            !env.contains("Apple_Terminal") && !env.contains("TERM_SESSION_ID"),
            "{env}"
        );
        assert!(
            env.contains("SHELL=/bin/zsh\n"),
            "the configured shell is its SHELL: {env}"
        );
        assert!(
            !env.contains("ZDOTDIR="),
            "no shell integration's start: {env}"
        );
    }

    /// **A window's child holds no descriptor of baud's but its terminal**,
    /// though one is open in baud without close-on-exec when it forks: one
    /// baud was started with, or one a library on another thread holds for
    /// a moment -- macOS's C library opens each `LC_*` file of a locale it
    /// loads without close-on-exec (measured: every locale file seen open
    /// while `newlocale` ran on another thread lacked `FD_CLOEXEC`), so a
    /// fork inside that moment handed the shell, and all it ran, a
    /// read-only locale file for good. The child looks through `/dev/fd`:
    /// descriptor 0, its terminal, is held, the control that the look can
    /// see one; the descriptor without close-on-exec is not; nor is one
    /// with it, the second control.
    #[test]
    fn a_descriptor_open_without_close_on_exec_never_reaches_a_window_child() {
        use std::os::fd::AsRawFd;
        let bare = std::fs::File::open("/dev/null").expect("a descriptor");
        let kept = std::fs::File::open("/dev/null").expect("a second");
        // SAFETY: `bare` is open; clearing its flags leaves it open.
        unsafe { crate::sys::fcntl(bare.as_raw_fd(), crate::sys::F_SETFD, 0) };
        // SAFETY: as above, reading the flags.
        let flags = unsafe { crate::sys::fcntl(bare.as_raw_fd(), crate::sys::F_GETFD) };
        assert_eq!(
            flags & crate::sys::FD_CLOEXEC,
            0,
            "the control: no close-on-exec"
        );
        let (n, c) = (bare.as_raw_fd(), kept.as_raw_fd());
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_from(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                "for n in 0 {n} {c}; do if [ -e /dev/fd/$n ]; then echo \"$n held\"; \
                 else echo \"$n free\"; fi; done"
            )),
            env_of(&[("PATH", "/usr/bin:/bin"), ("LANG", "C")]),
            || None,
            Parent::Other,
            None,
        )
        .expect("forkpty");
        let said: Vec<String> = String::from_utf8_lossy(&read_all(master.as_raw_fd()))
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        drop((bare, kept));
        assert_eq!(
            said,
            [
                "0 held".to_string(),
                format!("{n} free"),
                format!("{c} free")
            ]
        );
    }

    /// **One test at a time makes a copy far past what it saw**: a copy one
    /// test holds while another's spawn looks raises that spawn's bound
    /// past the other's copy, and the two tests below would each pass or
    /// fail by the other's timing.
    static FAR_COPIES: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// **The number a far copy is made at**: a thousand past the highest
    /// descriptor `seen`, well past the bound [`MARGIN`] sets, so the
    /// descriptors other tests in the same process open meanwhile do not
    /// carry the bound up to it. [`room_for`] makes the limit fit it.
    ///
    /// [`MARGIN`]: crate::inherited::MARGIN
    fn far_past(seen: crate::sys::Fd) -> crate::sys::Fd {
        seen + 1000
    }

    /// **Room for a descriptor numbered `far`**: `dup2` refuses a number at
    /// or past the soft limit on open descriptors, 256 in a stock macOS
    /// shell, so it is raised to `far + 1` when it is under that, never
    /// past the hard limit and never lowered. Process-wide, which a higher
    /// limit leaves every other test as it was. Asserted to fit before the
    /// copy, so a limit the hard one holds down says so rather than
    /// failing as the copy.
    fn room_for(far: crate::sys::Fd) {
        use crate::sys::{RLIMIT_NOFILE, RlimT, Rlimit, getrlimit, setrlimit};
        let mut now = Rlimit::default();
        // SAFETY: a `struct rlimit` that lives for the call.
        assert_eq!(unsafe { getrlimit(RLIMIT_NOFILE, &mut now) }, 0, "read");
        let want = RlimT::try_from(far + 1).expect("a positive number");
        if now.rlim_cur < want {
            let raised = Rlimit {
                rlim_cur: want.min(now.rlim_max),
                rlim_max: now.rlim_max,
            };
            // SAFETY: as above; the soft value is no higher than the hard.
            assert_eq!(unsafe { setrlimit(RLIMIT_NOFILE, &raised) }, 0, "raised");
            // SAFETY: as above.
            assert_eq!(unsafe { getrlimit(RLIMIT_NOFILE, &mut now) }, 0, "read");
        }
        assert!(
            RlimT::try_from(far).is_ok_and(|f| f < now.rlim_cur),
            "{far} fits under the soft limit {}",
            now.rlim_cur
        );
    }

    /// What a window's child, forked by [`Pty::fork_from`], says of each
    /// descriptor in `fds` through `/dev/fd`: `held` or `free`.
    fn child_holds(fds: &[crate::sys::Fd]) -> Vec<String> {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let list = fds
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let master = Pty::fork_from(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                "for n in {list}; do if [ -e /dev/fd/$n ]; then echo \"$n held\"; \
                 else echo \"$n free\"; fi; done"
            )),
            env_of(&[("PATH", "/usr/bin:/bin"), ("LANG", "C")]),
            || None,
            Parent::Other,
            None,
        )
        .expect("forkpty");
        String::from_utf8_lossy(&read_all(master.as_raw_fd()))
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect()
    }

    /// **A descriptor opened after the parent looked, far past what it saw,
    /// never reaches a window's child**: in the moment the terminal is open
    /// and the child not yet forked ([`OPENED`]), after the parent found
    /// the highest descriptor it holds, a descriptor without close-on-exec
    /// is made at a thousand past the highest this test saw ([`far_past`])
    /// -- past the bound a child closes to when it cannot list its own --
    /// the soft limit raised to fit it ([`room_for`]). The child lists
    /// them and closes this one too; on Linux `close_range` does. The
    /// control: descriptor 0, its terminal, is held.
    #[test]
    fn a_descriptor_opened_past_the_parents_look_never_reaches_a_window_child() {
        use std::os::fd::AsRawFd;
        let _one = FAR_COPIES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let file = std::fs::File::open("/dev/null").expect("a descriptor");
        let from = file.as_raw_fd();
        let seen = crate::inherited::open_from_three()
            .into_iter()
            .max()
            .unwrap_or(2);
        let far = far_past(seen);
        room_for(far);
        assert!(
            far > seen + crate::inherited::MARGIN,
            "the control: past the bound"
        );
        set_opened(Some(Box::new(move |_, _| {
            // SAFETY: both descriptors are numbers this test owns; the copy
            // has no close-on-exec.
            assert_eq!(unsafe { crate::sys::dup2(from, far) }, far, "the copy");
        })));
        let said = child_holds(&[0, far]);
        set_opened(None);
        // SAFETY: the copy this test made.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(far) });
        assert_eq!(said, ["0 held".to_string(), format!("{far} free")]);
    }

    /// **With the child's list too small, the bound still closes what is
    /// under it**: a list of one entry, which fills, takes the bound, so a
    /// descriptor without close-on-exec opened before the fork, under the
    /// highest the parent saw, is closed. The control that the bound was
    /// taken: a descriptor made past it after the parent looked
    /// ([`OPENED`]) is held -- the bound alone does not reach it, the list
    /// does (the test above). On Linux a small list takes the bound too.
    #[test]
    fn with_the_list_too_small_the_bound_still_closes_what_is_under_it() {
        use std::os::fd::AsRawFd;
        let _one = FAR_COPIES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let bare = std::fs::File::open("/dev/null").expect("a descriptor");
        let n = bare.as_raw_fd();
        // SAFETY: `bare` is open; clearing its flags leaves it open.
        unsafe { crate::sys::fcntl(n, crate::sys::F_SETFD, 0) };
        let seen = crate::inherited::open_from_three()
            .into_iter()
            .max()
            .unwrap_or(2);
        let far = far_past(seen);
        room_for(far);
        set_opened(Some(Box::new(move |_, _| {
            // SAFETY: both descriptors are numbers this test owns.
            assert_eq!(unsafe { crate::sys::dup2(n, far) }, far, "the copy");
        })));
        crate::inherited::set_list_room(crate::inherited::Room::One);
        let said = child_holds(&[0, n, far]);
        crate::inherited::set_list_room(crate::inherited::Room::Full);
        set_opened(None);
        // SAFETY: the copy this test made.
        drop(unsafe { <std::os::fd::OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(far) });
        drop(bare);
        assert_eq!(
            said,
            [
                "0 held".to_string(),
                format!("{n} free"),
                format!("{far} held")
            ]
        );
    }

    /// **A window's child starts with every signal at its default
    /// action, and none blocked**, whatever baud inherited or set: each of
    /// `SIGPIPE`, which Rust's runtime ignores in this very process,
    /// `SIGINT`, `SIGHUP`, `SIGTERM` and `SIGQUIT`, which a runner can hand
    /// on ignored, and `SIGUSR2`, which this test blocks on its own thread
    /// before the spawn as a launcher's mask would, ends a shell the child
    /// starts that sends it to itself, and nothing says it survived; the
    /// control, `SIGUSR1` ignored by the child's own shell, does. Red
    /// without the resets in [`fork_terminal`]'s child: the `SIGPIPE` line
    /// always, the `SIGUSR2` line without the mask's, and the `SIGINT` line
    /// when the test binary runs with it ignored. Named breaks: reset
    /// nothing; leave the mask.
    #[test]
    fn a_windows_child_starts_with_its_signals_at_their_default() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        // Each probe is `kill`, exec'd by a shell so that it sends the
        // signal to itself with the shell's start as it was given: bash, as
        // `/bin/sh`, ignores `SIGQUIT` in its own process whatever it was
        // started with, and runs what it execs with the disposition it
        // inherited. A probe that exits 0 survived; one a signal ends prints
        // nothing. The control: a signal the shell ignores itself is ignored
        // by what it runs, and the line says so.
        let script = format!(
            "for s in PIPE INT HUP TERM QUIT USR2; do \
            /bin/sh -c \"exec {kill} -$s \\$\\$\"; [ $? -eq 0 ] && echo $s survived; done; \
            trap '' USR1; /bin/sh -c 'exec {kill} -USR1 $$'; [ $? -eq 0 ] && echo USR1 survived",
            kill = tool("kill")
        );
        let given = ["/bin/sh", "-c", script.as_str()]
            .map(OsString::from)
            .to_vec();
        let start = ShellStart {
            integration: crate::shell::Integration::Off,
            ssh_env: crate::shell::SshEnv::Off,
            program: Some("/bin/sh".into()),
            run: None,
        }
        .running(crate::rules::Run::given(given).expect("a program"));
        let parent = vec![(OsString::from("PATH"), OsString::from("/usr/bin:/bin"))];
        let usr2 = crate::sys::SigSet::default().with(crate::sys::SIGUSR2);
        let mut was = crate::sys::SigSet::default();
        // SAFETY: this thread's own mask, from a set and into one that
        // live for the call.
        let blocked =
            unsafe { crate::sys::pthread_sigmask(crate::sys::SIG_BLOCK, &usr2, &mut was) };
        assert_eq!(blocked, 0, "the control: SIGUSR2 blocked on this thread");
        let forked = Pty::fork_given(
            text_area(&size, Px::of(8), Px::of(16)),
            None,
            parent,
            || None,
            Parent::Other,
            None,
            &start,
        );
        // SAFETY: as above, the mask this thread had.
        unsafe {
            crate::sys::pthread_sigmask(crate::sys::SIG_SETMASK, &was, core::ptr::null_mut())
        };
        let (pid, master, _) = forked.and_then(Result::ok).expect("forkpty");
        let out = String::from_utf8_lossy(&read_all(master.as_raw_fd())).replace("\r\n", "\n");
        drop(master);
        // SAFETY: this test's own child, waited for once.
        unsafe { crate::sys::waitpid(pid, std::ptr::null_mut(), 0) };
        let survived: Vec<&str> = out.lines().filter(|l| l.ends_with(" survived")).collect();
        assert_eq!(
            survived,
            ["USR1 survived"],
            "only the control's: a signal the child started ignoring or blocking: {out:?}"
        );
    }

    /// **The tests that send signals pass whoever launched them**: run
    /// again from this binary as a runner launches them
    /// (`sys::as_a_runner_launches`: `SIGHUP` blocked, `SIGINT` and
    /// `SIGQUIT` ignored), each passes, and all of them ran. The control: a
    /// child started by a plain `Command` under the same launch survives its
    /// own `kill -HUP`, `kill -INT` and `kill -QUIT`, so the launch is what
    /// it says. Here because the window's test opens a pty, in this
    /// module's nextest group. Red with `Command::new` for `sys::child` in
    /// `held.rs` and in `rules::command_of`, and with the `sigprocmask` of
    /// `sys::child_signals` taken out.
    #[test]
    fn the_signal_tests_pass_as_a_runner_launches_them() {
        const TESTS: [&str; 3] = [
            "held::tests::a_status_is_decoded_as_waitpid_gives_it",
            "rules::tests::a_rules_command_starts_with_its_signals_at_their_default",
            "pty::tests::a_windows_child_starts_with_its_signals_at_their_default",
        ];
        let mut control = std::process::Command::new("/bin/sh");
        control.args([
            "-c",
            "ulimit -c 0; kill -HUP $$; kill -INT $$; kill -QUIT $$; exit 7",
        ]);
        let survived = crate::sys::as_a_runner_launches(&mut control)
            .status()
            .expect("sh");
        assert_eq!(survived.code(), Some(7), "the control: {survived:?}");
        let mut tests = std::process::Command::new(std::env::current_exe().expect("this binary"));
        tests.args(["--exact", "--test-threads=1"]).args(TESTS);
        let out = crate::sys::as_a_runner_launches(&mut tests)
            .output()
            .expect("the tests");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{said}");
        assert!(
            said.contains(&format!("{} passed; 0 failed", TESTS.len())),
            "each ran: {said}"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn holds(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    /// **zsh echoes a typed Hebrew letter whole** -- the reported symptom at
    /// its source. zsh runs on the pty with no rc file; once its line editor
    /// is reading (it turns bracketed paste on, `CSI ? 2004 h`, as it starts
    /// each line -- zsh takes no prompt from the environment, so the prompt
    /// cannot be the marker), the letter's two bytes are typed, then Return
    /// (zsh answers that no such command exists) and `exit`. With a locale
    /// the echo carries `d7 99` and no `<0099>`; without one (the control)
    /// zsh prints `<0099>`, as it did on the screen it was reported from.
    #[cfg(target_os = "macos")]
    #[test]
    fn zsh_echoes_a_hebrew_letter_whole() {
        let echo = |locale: Option<&str>| -> Vec<u8> {
            let size = WinSize {
                ws_col: 80,
                ws_row: 24,
            };
            let locale = locale.map(str::to_string);
            let master = Pty::fork_from(
                text_area(&size, Px::of(8), Px::of(16)),
                Some("exec /bin/zsh -f -i".into()),
                outer_parent(),
                move || locale,
                Parent::Other,
                None,
            )
            .expect("forkpty");
            // The reader owns the master and closes it when the shell ends;
            // the typing borrows its number until then.
            let raw = master.as_raw_fd();
            let (tx, rx) = channel::<Vec<u8>>();
            let reader = std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                loop {
                    // SAFETY: `buf` is a live, writable buffer of the length passed.
                    let n = unsafe {
                        crate::sys::read(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len())
                    };
                    let Ok(n) = usize::try_from(n) else { break };
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            });
            let mut seen = Vec::new();
            let wait_for = |seen: &mut Vec<u8>, needle: &[u8], from: usize| {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while !holds(&seen[from.min(seen.len())..], needle) {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    match rx.recv_timeout(left) {
                        Ok(chunk) => seen.extend(chunk),
                        Err(_) => panic!(
                            "zsh never wrote {:?}: {:?}",
                            String::from_utf8_lossy(needle),
                            String::from_utf8_lossy(seen)
                        ),
                    }
                }
            };
            let typed = |bytes: &[u8]| {
                // SAFETY: `bytes` is a live buffer of the length passed.
                let n = unsafe { crate::sys::write(raw, bytes.as_ptr().cast(), bytes.len()) };
                assert_eq!(
                    usize::try_from(n).ok(),
                    Some(bytes.len()),
                    "the pty took the bytes"
                );
            };
            const READING: &[u8] = b"\x1b[?2004h";
            wait_for(&mut seen, READING, 0);
            let first_line = seen.len();
            typed(b"\xd7\x99\r");
            wait_for(&mut seen, READING, first_line);
            typed(b"exit\r");
            for chunk in rx {
                seen.extend(chunk);
            }
            reader.join().expect("the reader");
            seen
        };
        let with = echo(Some("he_IL.UTF-8"));
        assert!(
            holds(&with, b"\xd7\x99"),
            "the letter's bytes: {:?}",
            String::from_utf8_lossy(&with)
        );
        assert!(
            !holds(&with, b"<0099>"),
            "zsh escaped the letter: {:?}",
            String::from_utf8_lossy(&with)
        );
        let without = echo(None);
        assert!(
            holds(&without, b"<0099>"),
            "the control: with no locale zsh prints <0099>: {:?}",
            String::from_utf8_lossy(&without)
        );
    }

    /// **The foreground is read off the master: the program and its
    /// folder**, from another session, as the app reads its own child
    /// (`fg.rs`). The child `cd`s into a fresh folder, says it is ready
    /// and waits for a line: the group is its own, `sh` by argv[0], in
    /// that folder -- canonical, as the kernel answers it. On macOS a
    /// line lets it exec `sleep`: the exec is a kqueue event (`fg::Watch`),
    /// waited on with a bound and never a sleep, after which the same
    /// group reads `sleep`, and a [`crate::title::Derive`] that heard the
    /// event reads the new name where one that did not keeps the old.
    #[test]
    fn the_foreground_reads_name_the_program_and_its_folder() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let made = std::env::temp_dir().join(format!("baud-fg-{}", std::process::id()));
        std::fs::create_dir_all(&made).expect("a folder");
        let dir = made.canonicalize().expect("canonical");
        let cmd = format!(
            "cd '{}' && {} -echo && echo ready && read x && exec {} 30",
            dir.display(),
            tool("stty"),
            tool("sleep")
        );
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(cmd),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master.as_raw_fd(), "ready\r\n");
        let g = crate::fg::group(master.as_raw_fd()).expect("the child's group");
        assert_eq!(crate::fg::name(g).as_deref(), Some("sh"));
        assert_eq!(crate::fg::dir(g).as_deref(), Some(dir.as_path()));
        let reads = crate::fg::Live {
            master: Some(master.as_raw_fd()),
        };
        let (mut heard, mut deaf) = (crate::title::Derive::new(), crate::title::Derive::new());
        heard.check(&reads);
        deaf.check(&reads);
        let home = Some(&b"/nowhere"[..]);
        let folder = crate::title::folder_text(
            std::os::unix::ffi::OsStrExt::as_bytes(dir.as_os_str()),
            home,
        );
        assert_eq!(
            heard.title(None, &[], home),
            Some(format!("{folder} \u{2014} sh"))
        );
        #[cfg(target_os = "macos")]
        {
            let (tx, rx) = std::sync::mpsc::channel();
            let w = crate::fg::Watch::start(None, crate::probe::WatchCell::default(), move || {
                let _ = tx.send(());
            })
            .expect("a kqueue");
            w.add(g);
            send(master.as_raw_fd(), b"\n");
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("the exec is an event");
            assert_eq!(w.take(), crate::fg::Moved::Moved);
            assert_eq!(
                crate::fg::group(master.as_raw_fd()),
                Some(g),
                "the same group"
            );
            assert_eq!(crate::fg::name(g).as_deref(), Some("sleep"));
            heard.moved();
            deaf.owe();
            assert_eq!(heard.check(&reads), crate::title::Changed::Yes);
            assert_eq!(
                heard.title(None, &[], home),
                Some(format!("{folder} \u{2014} sleep"))
            );
            deaf.check(&reads);
            assert_eq!(
                deaf.title(None, &[], home),
                Some(format!("{folder} \u{2014} sh")),
                "the control: no event, no new name"
            );
        }
        drop(master);
        let _ = std::fs::remove_dir(&made);
    }

    /// Blocking reads of `master` until what arrived ends with `want`, and
    /// what arrived. The master must still be blocking.
    fn read_until(master: crate::sys::Fd, want: &str) -> Vec<u8> {
        let mut seen = Vec::new();
        let mut buf = [0u8; 256];
        while !seen.ends_with(want.as_bytes()) {
            match read_master(master, &mut buf) {
                Got::Bytes(b) => seen.extend_from_slice(b),
                Got::Nothing => {}
                Got::Closed => panic!(
                    "the child left before {want:?}: {:?}",
                    String::from_utf8_lossy(&seen)
                ),
            }
        }
        seen
    }

    /// **An idle child's empty master is not the end of the stream.** The
    /// reader took a read that returned no bytes as the child gone. Once the
    /// master is non-blocking -- for the main thread's writes, and the flag
    /// is the open file's -- an idle child's master answers `EAGAIN` at once,
    /// and the reader, and with it the window, would end the first time the
    /// child paused. Then the wait that replaces the blocking read: `poll`
    /// returns when the child writes, and the read after it has the bytes.
    #[test]
    fn an_idle_childs_master_reads_as_nothing_and_its_next_write_wakes_the_wait() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                "{} -echo; echo ready; read x; echo hi",
                tool("stty")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // Parked in `read`, with nothing behind its `ready`.
        read_until(master.as_raw_fd(), "ready\r\n");
        nonblocking(master.as_raw_fd()).expect("O_NONBLOCK");
        let mut buf = [0u8; READ_CHUNK];
        assert!(
            matches!(read_master(master.as_raw_fd(), &mut buf), Got::Nothing),
            "an idle child's empty master read as the end of the stream"
        );
        let mut out = Outbox::default();
        out.push(b"go\n");
        assert_eq!(write_some(master.as_raw_fd(), &mut out), Wrote::All);
        let mut seen = Vec::new();
        loop {
            match read_master(master.as_raw_fd(), &mut buf) {
                Got::Bytes(b) => seen.extend_from_slice(b),
                Got::Nothing => assert_eq!(readable(master.as_raw_fd()), Waited::Ready),
                Got::Closed => break,
            }
        }
        assert_eq!(String::from_utf8_lossy(&seen), "hi\r\n");
        drop(master);
    }

    /// **A descriptor `poll` cannot watch is reported, not waited on again.**
    /// `POLLNVAL` returns at once, so a reader that took it as readiness
    /// would read, find nothing and wait again, forever, at full speed. A
    /// number no process opens is the descriptor here: `poll` answers it
    /// `POLLNVAL` on both platforms, and no other test thread can be handed
    /// it.
    #[test]
    fn a_descriptor_poll_cannot_watch_is_reported_rather_than_waited_on() {
        assert_eq!(readable(1 << 20), Waited::Unwatchable);
    }

    /// **A write the full queue takes nothing of says so**, so the activity
    /// lights' SD stays dark for bytes still owed: a
    /// raw child that never reads lets Darwin's master take 1022 bytes of
    /// 8192, a part, and the next write finds no room and takes none, every
    /// byte still owed.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_write_the_full_queue_takes_nothing_of_says_so() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                "{} raw -echo; echo ready; exec {} 30",
                tool("stty"),
                tool("sleep")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        read_until(master.as_raw_fd(), "ready\n");
        nonblocking(master.as_raw_fd()).expect("O_NONBLOCK");
        let mut out = Outbox::default();
        out.push(&[b'x'; 8192]);
        assert_eq!(write_some(master.as_raw_fd(), &mut out), Wrote::Part);
        let owed = out.bytes.len();
        assert!(owed > 0 && owed < 8192, "{owed} owed");
        assert_eq!(write_some(master.as_raw_fd(), &mut out), Wrote::Nothing);
        assert_eq!(out.bytes.len(), owed, "nothing taken, nothing lost");
        drop(master);
    }

    /// **A write the queue cannot hold returns, and what it could not take
    /// stays owed, in order.** A raw child that never reads let Darwin's
    /// master take 1022 bytes and then answered `EAGAIN` (measured
    /// 2026-10-09), and the blocking master waited in `write(2)` for the rest
    /// -- on the window's thread. Here 8192 bytes go to a raw child that
    /// echoes what it reads, through the outbox, offered again whenever
    /// `poll` says there is room: every byte comes back once and in order,
    /// and on Darwin the first write is short, so the owed path is the one
    /// taken.
    #[test]
    fn a_full_queue_keeps_the_rest_owed_in_order_and_never_waits() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let master = Pty::fork_with(
            text_area(&size, Px::of(8), Px::of(16)),
            Some(format!(
                "{} raw -echo; echo ready; exec {} -c 8192",
                tool("stty"),
                tool("head")
            )),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        // Raw: no output processing, so no carriage return.
        read_until(master.as_raw_fd(), "ready\n");
        nonblocking(master.as_raw_fd()).expect("O_NONBLOCK");
        // Printable, so no line discipline setting can read a byte as a
        // control.
        let payload: Vec<u8> = (b' '..=b'~').cycle().take(8192).collect();
        let mut out = Outbox::default();
        out.push(&payload);
        let first = write_some(master.as_raw_fd(), &mut out);
        #[cfg(target_os = "macos")]
        assert_eq!(
            first,
            Wrote::Part,
            "Darwin's queue holds 1022 bytes, so 8192 do not go in one write; {} were taken",
            payload.len() - out.bytes.len()
        );
        let _ = first;
        let mut echoed = Vec::new();
        let mut buf = [0u8; READ_CHUNK];
        loop {
            let mut p = crate::sys::PollFd {
                fd: master.as_raw_fd(),
                events: match out.is_empty() {
                    true => crate::sys::POLLIN,
                    false => crate::sys::POLLIN | crate::sys::POLLOUT,
                },
                revents: 0,
            };
            // A bound on a hang, not a timing: a pass never comes near it.
            // SAFETY: one initialised record and its count.
            let n = unsafe { crate::sys::poll(&mut p, 1, 20_000) };
            assert!(
                n > 0,
                "nothing moved in 20 s: {} echoed, {} owed",
                echoed.len(),
                out.bytes.len()
            );
            if p.revents & crate::sys::POLLOUT != 0 {
                let _ = write_some(master.as_raw_fd(), &mut out);
            }
            match read_master(master.as_raw_fd(), &mut buf) {
                Got::Bytes(b) => echoed.extend_from_slice(b),
                Got::Nothing => {}
                Got::Closed => break,
            }
        }
        assert!(out.is_empty(), "{} bytes still owed", out.bytes.len());
        assert_eq!(echoed.len(), payload.len());
        assert!(echoed == payload, "every byte, once, in order");
        drop(master);
    }

    /// What the kernel holds for the pty behind `master` -- read back rather
    /// than trusted from `told`, so the tests that use it are about the
    /// kernel.
    fn ask(master: crate::sys::Fd) -> crate::sys::Winsize {
        let mut ws = crate::sys::Winsize::default();
        // SAFETY: TIOCGWINSZ writes one winsize through the pointer.
        let rc = unsafe { crate::sys::ioctl(master, crate::sys::TIOCGWINSZ, &mut ws) };
        assert_eq!(rc, 0, "TIOCGWINSZ failed");
        ws
    }

    /// **The pixels are the text area, from the cell -- not the window.** A
    /// child reads `ws_xpixel / ws_col` as a cell's width, so a window's
    /// width, which carries the padding and the part of a cell the grid does
    /// not fill, would inflate it.
    #[test]
    fn the_winsize_is_the_text_area_in_pixels() {
        let ws = text_area(
            &WinSize {
                ws_col: 80,
                ws_row: 24,
            },
            Px::of(17),
            Px::of(34),
        );
        assert_eq!(
            ws,
            crate::sys::Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 1360,
                ws_ypixel: 816,
            }
        );
        // Saturating, not wrapping: 2048 columns of a 72-pixel cell is
        // 147,456 pixels, which the kernel's `u16` cannot hold.
        let wide = text_area(
            &WinSize {
                ws_col: 2048,
                ws_row: 10,
            },
            Px::of(72),
            Px::of(144),
        );
        assert_eq!((wide.ws_xpixel, wide.ws_ypixel), (u16::MAX, 1440));
    }

    /// **The kernel holds the pixels from the fork on, and hears a zoom that
    /// keeps the grid.** Before this, the fork carried zero pixels and the
    /// first resize after launch computed the grid the pty was spawned at,
    /// so nothing was sent and a child asking at startup -- yazi does --
    /// read zero until the window changed. Read back through `TIOCGWINSZ`
    /// on a real master, so it is the kernel's answer and not baud's
    /// bookkeeping.
    #[test]
    fn the_kernel_holds_the_text_area_from_the_fork_and_after_a_zoom() {
        let size = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let first = text_area(&size, Px::of(17), Px::of(34));
        let master = Pty::fork_with(
            first,
            Some("exec /bin/cat".into()),
            child_env(outer_parent(), None),
        )
        .expect("forkpty");
        assert_eq!(
            ask(master.as_raw_fd()),
            first,
            "the fork did not carry the pixels"
        );

        // A zoom: the same grid, a bigger cell. Told.
        let mut told = first;
        let zoomed = text_area(&size, Px::of(20), Px::of(40));
        tell(master.as_raw_fd(), &mut told, zoomed);
        assert_eq!(
            ask(master.as_raw_fd()),
            zoomed,
            "a zoom that kept the grid was not sent"
        );
        assert_eq!(told, zoomed);

        // **The same size again sends nothing**, which is the property that
        // lets the app call this on every re-lay. The kernel's size is moved
        // out of band first, so a `tell` that did send would put it back.
        let elsewhere = text_area(&size, Px::of(1), Px::of(1));
        // SAFETY: TIOCSWINSZ takes a pointer to a winsize.
        unsafe {
            crate::sys::ioctl(master.as_raw_fd(), crate::sys::TIOCSWINSZ, &elsewhere);
        }
        tell(master.as_raw_fd(), &mut told, zoomed);
        assert_eq!(
            ask(master.as_raw_fd()),
            elsewhere,
            "an unchanged size was sent again"
        );

        drop(master);
    }
}
#[cfg(test)]
mod refused_tests {
    use super::tests::grid_80x24;
    use super::*;
    use crate::sys::{EMFILE, ENOSPC, ENXIO, Errno};

    /// macOS's `EAGAIN`, a subsidiary's open before it is unlocked.
    #[cfg(target_os = "macos")]
    const EAGAIN: core::ffi::c_int = 35;

    const AT_511: (Option<PtyCount>, Option<PtyCount>) = (Some(PtyCount(511)), Some(PtyCount(511)));

    /// Refuses this thread's opens until dropped.
    struct Refusing;

    impl Refusing {
        fn with(e: core::ffi::c_int) -> Refusing {
            set_refusing(Some(Errno(e)), Some(AT_511));
            Refusing
        }
    }

    impl Drop for Refusing {
        fn drop(&mut self) {
            set_refusing(None, None);
        }
    }

    /// **The limit is the errno the kernel answers at it, and nothing
    /// else**: macOS's `ENXIO` from `ptcopen`, Linux's `ENOSPC` from
    /// `devpts_new_index`. macOS's `EAGAIN` is a subsidiary not yet
    /// unlocked, and `EMFILE` this process's descriptors: each its own
    /// failure, no count read for it.
    #[test]
    fn the_limit_is_the_kernels_answer_at_it_alone() {
        let counted = |e| NoTerminal::of(Errno(e), || AT_511);
        let limit = NoTerminal::Limit {
            open: Some(PtyCount(511)),
            most: Some(PtyCount(511)),
        };
        #[cfg(target_os = "macos")]
        {
            assert_eq!(counted(ENXIO), limit);
            assert_eq!(counted(ENOSPC), NoTerminal::Failed(Errno(ENOSPC)));
            assert_eq!(counted(EAGAIN), NoTerminal::Failed(Errno(EAGAIN)));
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(counted(ENOSPC), limit);
            assert_eq!(counted(ENXIO), NoTerminal::Failed(Errno(ENXIO)));
        }
        assert_eq!(counted(EMFILE), NoTerminal::Failed(Errno(EMFILE)));
    }

    /// **A refused open is tried once, and says why.** `redriven` asks
    /// again only for a lost race; the limit answers the same in
    /// microseconds, and is the window's to say.
    #[test]
    fn a_refused_open_is_tried_once_and_says_why() {
        let _refusing = Refusing::with(AT_THE_LIMIT.0);
        let refused = open_terminal(grid_80x24()).map(|_| ());
        assert_eq!(refused_opens(), Tries(1), "one open, no retry");
        assert_eq!(
            refused,
            Err(NoTerminal::Limit {
                open: Some(PtyCount(511)),
                most: Some(PtyCount(511)),
            })
        );
        drop(_refusing);
        let _other = Refusing::with(EMFILE);
        assert_eq!(
            open_terminal(grid_80x24()).map(|_| ()),
            Err(NoTerminal::Failed(Errno(EMFILE)))
        );
        assert_eq!(refused_opens(), Tries(1));
    }

    /// A refused window's terminal, at 80 by 24.
    fn refused() -> Pty {
        Pty::Refused(Refused {
            why: NoTerminal::Limit {
                open: Some(PtyCount(511)),
                most: Some(PtyCount(511)),
            },
            told: grid_80x24(),
        })
    }

    /// **A refused terminal has no child, so nothing is signalled,
    /// watched, reaped or polled, and its window stays.** Every answer a
    /// driver asks of the window's terminal, as the refused state gives it:
    /// no process, no descriptor to poll, the stream open and the window
    /// waiting, the bytes typed let go.
    #[test]
    fn a_refused_terminal_has_no_child_and_its_window_stays() {
        let mut pty = refused();
        assert_eq!(pty.child(), None, "no child to signal");
        assert_eq!(pty.reads().master, None, "no foreground to read");
        assert_eq!(pty.tty(), None);
        assert!(matches!(
            pty.decide(crate::held::Stream::Open),
            crate::held::Decided::Wait
        ));
        let mut out = Outbox::default();
        out.push(b"typed");
        assert_eq!(pty.write(&mut out), Wrote::All, "the bytes are let go");
        assert!(out.is_empty());
        pty.follow(InputEncoding::Latin1);
        pty.resize(
            &WinSize {
                ws_col: 100,
                ws_row: 30,
            },
            Px::of(8),
            Px::of(16),
        );
        #[cfg(target_os = "macos")]
        {
            assert!(pty.reaps().is_none(), "no child to reap");
            assert!(pty.watch(|| {}).is_none(), "no process watch");
            let drained = pty.drain();
            assert!(drained.chunks.is_empty());
            assert_eq!(drained.stream, Stream::Open, "the window stays");
        }
        #[cfg(target_os = "linux")]
        {
            assert!(pty.polled_master().is_none(), "nothing polled");
            assert!(pty.child_fd().is_none(), "no child's descriptor polled");
            assert!(pty.as_fd().is_none());
            pty.hung_up();
            pty.reap();
            pty.give_up();
            assert_eq!(pty.stream(), crate::held::Stream::Open, "the window stays");
            assert_eq!(pty.reap_owed(), None);
            assert!(matches!(pty.read_chunk(), Chunk::Bytes(b) if b.is_empty()));
        }
        assert!(pty.refused().is_some());
    }

    /// **What the refused window says, line for line, each within 80
    /// columns**, for each count that could be read.
    #[test]
    fn the_refused_window_says_the_limit_and_what_to_do() {
        let say = |open, most| NoTerminal::Limit { open, most }.to_string();
        let (o, m) = (Some(PtyCount(509)), Some(PtyCount(511)));
        #[cfg(target_os = "macos")]
        {
            let after = "\nClose terminal windows or tabs you no longer need, then open a new window.\
                \nOr raise the limit, up to 999: sudo sysctl kern.tty.ptmx_max=999\
                \nTo keep it after a restart, add kern.tty.ptmx_max=999 to /etc/sysctl.conf.";
            let first = "baud could not open a terminal:";
            assert_eq!(
                say(o, m),
                format!("{first} macOS allows 511 at once, and 509 are open.{after}")
            );
            assert_eq!(
                say(None, m),
                format!("{first} macOS allows 511 at once, and all are in use.{after}")
            );
            assert_eq!(
                say(o, None),
                format!("{first} macOS's limit is reached, with 509 open.{after}")
            );
            assert_eq!(
                say(None, None),
                format!("{first} macOS's limit on terminals is reached.{after}")
            );
        }
        #[cfg(target_os = "linux")]
        {
            let after = "\nClose terminal windows or tabs you no longer need, then open a new window.\
                \nOr raise the limit: sudo sysctl kernel.pty.max=<a larger number>";
            let first =
                "baud could not open a terminal: the system's limit on terminals is reached.";
            assert_eq!(
                say(o, m),
                format!("{first}\nkernel.pty.max is 511, and 509 are open.{after}")
            );
            assert_eq!(
                say(None, m),
                format!("{first}\nkernel.pty.max is 511.{after}")
            );
            assert_eq!(say(o, None), format!("{first}\n509 are open.{after}"));
            assert_eq!(say(None, None), format!("{first}{after}"));
        }
        let failed = NoTerminal::Failed(Errno(EMFILE)).to_string();
        assert!(
            failed.starts_with("baud could not open a terminal: "),
            "{failed}"
        );
        assert!(
            failed.contains(&std::io::Error::from_raw_os_error(EMFILE).to_string()),
            "{failed}"
        );
        // The most each system's count can be: macOS's `PTMX_MAX_HARD`, and
        // Linux's `NR_UNIX98_PTY_MAX`, 1 << 20.
        #[cfg(target_os = "macos")]
        let big = Some(PtyCount(999));
        #[cfg(target_os = "linux")]
        let big = Some(PtyCount(1 << 20));
        for open in [None, o, big] {
            for most in [None, m, big] {
                for line in say(open, most).lines() {
                    assert!(
                        line.chars().count() <= 80,
                        "{line:?} is {} columns",
                        line.chars().count()
                    );
                }
            }
        }
    }

    /// **The advice names the setting the counts read**: each `name=value`
    /// the refused window tells the user to set is the sysctl
    /// [`pty_counts`] reads, [`PTMX_MAX`] through `sysctlbyname` on macOS,
    /// on both its lines, and `max` under [`PTY_SYSCTLS`] on Linux, whose
    /// count line names it too.
    #[test]
    fn the_advice_names_the_setting_the_counts_read() {
        let said = NoTerminal::Limit {
            open: Some(PtyCount(509)),
            most: Some(PtyCount(511)),
        }
        .to_string();
        let set: Vec<&str> = said
            .split_whitespace()
            .filter_map(|word| word.split_once('=').map(|(name, _)| name))
            .collect();
        #[cfg(target_os = "macos")]
        let (read, lines) = (PTMX_MAX.to_str().expect("an ASCII name"), 2);
        #[cfg(target_os = "linux")]
        let read = PTY_SYSCTLS
            .strip_prefix("/proc/sys/")
            .expect("under /proc/sys")
            .replace('/', ".")
            + ".max";
        #[cfg(target_os = "linux")]
        let (read, lines) = (read.as_str(), 1);
        assert_eq!(set, vec![read; lines], "{said}");
        #[cfg(target_os = "linux")]
        assert!(
            said.lines()
                .any(|l| l.starts_with(&format!("{read} is 511"))),
            "{said}"
        );
    }

    /// **The refused window's terminal holds its words from the left edge,
    /// a line each**; a live one is left as it is.
    #[test]
    fn a_refused_window_writes_its_words_into_its_terminal() {
        let ws = WinSize {
            ws_col: 80,
            ws_row: 24,
        };
        let mut owned = baud_core::Owned::new(&ws);
        let mut term = baud_core::Term::new(owned.storage(), &ws);
        let pty = refused();
        say_refused(&mut term, &pty);
        let rows: Vec<String> = (0..5)
            .map(|y| {
                term.view(baud_domain::Rows::ZERO)
                    .row(baud_domain::Row::from_index(y))
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|p| char::from_u32(p.value()))
                            .unwrap_or(' ')
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect();
        let said: Vec<String> = pty
            .refused()
            .expect("refused")
            .to_string()
            .lines()
            .map(str::to_string)
            .chain([String::new()])
            .collect();
        assert_eq!(rows, said);
    }
}
