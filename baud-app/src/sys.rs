//! **The calls the pty and the font mapping need and `std` does not
//! expose**, declared as the extern lines they are rather than through a
//! `libc` crate -- the way `baud-wayland`'s `sys` declares `sendmsg` and
//! `baud-isa-aarch64` declares `sysctlbyname`.
//!
//! What is *not* here is as deliberate as what is. `open`, `fstat`,
//! `close`, `O_RDONLY` and `O_CLOEXEC` went to [`std::fs::File`], which
//! opens close-on-exec, answers the length from `metadata` and closes on
//! drop; that took the surface from twenty-five items to ten. Everything
//! left is something `std` has no API for: a pty, an ioctl, a poll, a
//! descriptor's non-blocking flag, a terminal's attributes, a mapping,
//! Darwin's raw uptime clock, a
//! symbol looked up at run time because older systems do not have it, and
//! the user's locale -- read from CoreFoundation and checked with
//! `newlocale` -- for a shell whose environment names none, and on macOS
//! one libnotify call a fork makes first, for what it initialises.
//!
//! **Every layout and constant below is checked against the platform's own
//! headers** by this file's tests, which compile a C probe with `cc` and
//! compare what it prints. A wrong *signature* usually fails to link; a
//! wrong *field order* compiles, links, and quietly reads the wrong
//! member, so the struct half cannot be left to reading.

#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
use core::ffi::c_uint;
use core::ffi::{c_char, c_int, c_ulong, c_void};

/// A file descriptor, as the kernel numbers them.
pub type Fd = c_int;

/// `poll`'s count of descriptors: `unsigned int` on Darwin and
/// `unsigned long` on glibc, which `sizeof.nfds_t` pins.
///
/// Both drivers poll the master. The Linux loop waits on it beside the
/// compositor's socket; the macOS reader thread waits on it alone, when a
/// read of the master -- non-blocking since 2026-10-09, because the main
/// thread writes it (`pty.rs`) -- finds nothing.
#[cfg(target_os = "macos")]
pub type Nfds = c_uint;
#[cfg(target_os = "linux")]
pub type Nfds = c_ulong;

/// The terminal's size, as `TIOCSWINSZ` takes it. Four `u16`s, in this
/// order, on both platforms.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// One descriptor `poll` watches.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PollFd {
    pub fd: Fd,
    pub events: i16,
    pub revents: i16,
}

/// Set the terminal's window size. The number differs per platform --
/// Darwin encodes the direction and the payload's size into it, glibc
/// numbers it in a flat space -- which is the clearest case in this file
/// for reading a header rather than remembering a constant.
#[cfg(target_os = "macos")]
pub const TIOCSWINSZ: c_ulong = 0x8008_7467;
#[cfg(not(target_os = "macos"))]
pub const TIOCSWINSZ: c_ulong = 0x5414;

/// **Take the terminal as the session's controlling terminal**: the
/// spawned child's second call, after `setsid`, as Apple's and glibc's
/// `login_tty` make it (`[[source: apple-libc-login-tty-c]]`,
/// `[[source: glibc-2-42-login-tty-c]]`). Darwin's `_IO('t', 97)`, glibc's
/// flat number.
#[cfg(target_os = "macos")]
pub const TIOCSCTTY: c_ulong = 0x2000_7461;
#[cfg(not(target_os = "macos"))]
pub const TIOCSCTTY: c_ulong = 0x540e;

/// Read the terminal's window size back. Only a test asks: it forks a pty and
/// reads what the kernel holds, which is the one reading of "the child was
/// told" that does not trust baud's own bookkeeping.
#[cfg(all(test, target_os = "macos"))]
pub const TIOCGWINSZ: c_ulong = 0x4008_7468;
#[cfg(all(test, not(target_os = "macos")))]
pub const TIOCGWINSZ: c_ulong = 0x5413;

pub const POLLIN: i16 = 0x1;
/// Room to write. The Linux loop asks for it while the pty owes bytes it
/// has not taken. The macOS driver asks no `poll` for it: a write that
/// leaves bytes owed registers `EVFILT_WRITE` on the master in a kqueue
/// heard on the main queue, whose event wakes the window to write the rest
/// (`pty::OutboxWatch`). Only a test asks for it there.
#[cfg(any(target_os = "linux", test))]
pub const POLLOUT: i16 = 0x4;
/// The descriptor cannot be watched. Darwin's `poll(2)` says under BUGS
/// that it "currently does not support devices", and a pty master is one;
/// measured on this Mac it watches the master as it should, so this is the
/// reader's way to notice a system where the page is right.
#[cfg(any(target_os = "macos", test))]
pub const POLLNVAL: i16 = 0x20;

/// `fcntl`'s commands to read and to set a descriptor's status flags, and
/// the flag that makes a `read` or a `write` that would wait return
/// `EAGAIN` instead. The flag's bit differs per platform.
pub const F_GETFL: c_int = 3;
pub const F_SETFL: c_int = 4;

/// **What a pty open returns when the kernel's own redrive ran out.**
/// Darwin's `/dev/ptmx` hands an opener the first free terminal without
/// claiming it, so two openers at once can be given the same one; the
/// one that comes second gets `EREDRIVEOPEN` and the kernel redoes the
/// open, ten times, and then returns it: "-6 ... reserved for kernel
/// internal use" (`[[source: xnu-errno-h]]`, `[[source: xnu-tty-ptmx-c]]`,
/// `[[source: xnu-vfs-vnops-c]]`). Linux has no such value. Not in the
/// header test: the SDK's `errno.h` does not carry it, the kernel's has it
/// under `KERNEL_PRIVATE`, so the pinned source holds the value.
#[cfg(target_os = "macos")]
pub const EREDRIVEOPEN: c_int = -6;

/// `fcntl`'s commands and flag for a descriptor's own flags: the spawned
/// child clears close-on-exec on its standard descriptors, `main` sets it
/// on every descriptor baud was started with (`inherited.rs`), and the
/// tests read that the pty's master and what `openat` opened for a kitty
/// file close on exec. The same values on both platforms.
pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const FD_CLOEXEC: c_int = 1;

/// **`RLIMIT_NOFILE`, the limit on how many descriptors a process holds**,
/// and its `struct rlimit`: the tests that make a descriptor far past the
/// highest open raise the soft value toward the hard one (`pty.rs`), which
/// is 256 in a stock macOS shell. Darwin's resource is 8 and glibc's 7;
/// `rlim_t` is eight bytes on both.
#[cfg(all(test, target_os = "macos"))]
pub const RLIMIT_NOFILE: c_int = 8;
#[cfg(all(test, not(target_os = "macos")))]
pub const RLIMIT_NOFILE: c_int = 7;
#[cfg(all(test, target_os = "macos"))]
pub type RlimT = core::ffi::c_ulonglong;
#[cfg(all(test, not(target_os = "macos")))]
pub type RlimT = c_ulong;

/// `struct rlimit`: the soft value, which a process may raise to the hard
/// one.
#[cfg(test)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rlimit {
    pub rlim_cur: RlimT,
    pub rlim_max: RlimT,
}

#[cfg(test)]
unsafe extern "C" {
    /// Reads `resource`'s soft and hard values into `rlp`; 0 on success.
    pub fn getrlimit(resource: c_int, rlp: *mut Rlimit) -> c_int;

    /// Sets `resource`'s values from `rlp`: the soft value no higher than
    /// the hard one; 0 on success.
    pub fn setrlimit(resource: c_int, rlp: *const Rlimit) -> c_int;
}
#[cfg(target_os = "macos")]
pub const O_NONBLOCK: c_int = 0x4;
#[cfg(not(target_os = "macos"))]
pub const O_NONBLOCK: c_int = 0o4000;
pub const PROT_READ: c_int = 0x1;
pub const MAP_PRIVATE: c_int = 0x2;

/// What `mmap` answers with on failure: `(void *) -1`, not null.
pub const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;

/// Darwin's monotonic clock that does not stop while the machine sleeps,
/// which is the one the keystroke instrument stamps on.
#[cfg(target_os = "macos")]
pub const CLOCK_UPTIME_RAW: c_uint = 8;

/// `dlopen`'s modes: resolve symbols when first used, keep them out of the
/// global namespace, and -- with `RTLD_NOLOAD` -- answer only a library
/// already loaded. The accessibility switch's read asks for a framework
/// AppKit has loaded before it asks for one it may not have.
#[cfg(target_os = "macos")]
pub const RTLD_LAZY: c_int = 0x1;
#[cfg(target_os = "macos")]
pub const RTLD_LOCAL: c_int = 0x4;
#[cfg(target_os = "macos")]
pub const RTLD_NOLOAD: c_int = 0x10;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// **libnotify's token check, called before a fork for its first line.**
    /// It reads libnotify's globals through `_notify_globals()`, an
    /// `os_alloc_once` (`[[source: libnotify-348-notify-client]]` lines
    /// 475-477 and 748-754), and libSystem's fork handler in the child reads
    /// them the same way (lines 1221-1223). A fork while another thread is
    /// inside that one-time initialisation leaves the child waiting on a once
    /// whose owner did not cross the fork, and libplatform aborts it: "os_once_t
    /// is corrupt", "crashed on child side of fork pre-exec". Called first, the
    /// initialisation is over -- or this call waits for it -- before the fork.
    /// The answer is not read.
    pub fn notify_is_valid_token(val: c_int) -> bool;
}

// `forkpty` is in libutil on glibc and in libSystem on Darwin.
#[cfg(test)]
#[cfg_attr(not(target_os = "macos"), link(name = "util"))]
unsafe extern "C" {
    /// Fork with a fresh pty; the parent gets the master in `amaster`. Only a
    /// test calls it, as the control beside baud's own spawn: libc opens
    /// both ends without close-on-exec and forks after, so a child another
    /// thread forks between the two holds them (`pty.rs`, `open_terminal`).
    ///
    /// The window size is `const` on glibc and mutable on Darwin. The
    /// struct is the same either way, which is why only the pointer
    /// differs here.
    #[cfg(target_os = "macos")]
    pub fn forkpty(
        amaster: *mut Fd,
        name: *mut c_char,
        termp: *const c_void,
        winp: *mut Winsize,
    ) -> c_int;
    #[cfg(not(target_os = "macos"))]
    pub fn forkpty(
        amaster: *mut Fd,
        name: *mut c_char,
        termp: *const c_void,
        winp: *const Winsize,
    ) -> c_int;
}

unsafe extern "C" {
    pub fn read(fd: Fd, buf: *mut c_void, count: usize) -> isize;
    pub fn write(fd: Fd, buf: *const c_void, count: usize) -> isize;
    pub fn poll(fds: *mut PollFd, nfds: Nfds, timeout: c_int) -> c_int;

    /// Variadic in the header, as `ioctl` is -- and it has to be declared
    /// so: on Apple's arm64 a variadic argument is passed on the stack, so a
    /// fixed third parameter would hand `F_SETFL` whatever sat in the
    /// register.
    pub fn fcntl(fd: Fd, cmd: c_int, ...) -> c_int;
    pub fn execve(
        path: *const c_char,
        argv: *const *const c_char,
        envp: *const *const c_char,
    ) -> c_int;
    pub fn mmap(
        addr: *mut c_void,
        len: usize,
        prot: c_int,
        flags: c_int,
        fd: Fd,
        offset: i64,
    ) -> *mut c_void;

    /// Variadic in the header, and the third argument is the only one any
    /// caller here passes.
    pub fn ioctl(fd: Fd, request: c_ulong, ...) -> c_int;

    /// Exit without running atexit handlers or flushing: the child between
    /// `fork` and a failed `exec`, where only async-signal-safe calls are
    /// allowed.
    pub fn _exit(status: c_int) -> !;

    /// Closes `fd`. A window's child calls it between `fork` and `exec` on
    /// every descriptor it holds past its terminal (`pty.rs`), which XSH
    /// 2.4.3's table of async-signal-safe functions allows (`[[source:
    /// posix-xsh-chap02]]`).
    pub fn close(fd: Fd) -> c_int;

    /// This process's ID: a window's child asks the kernel for its own
    /// descriptors by it (`pty.rs`). On XSH 2.4.3's table.
    #[cfg(target_os = "macos")]
    pub fn getpid() -> Pid;

    /// Changes the working directory to `path`; on failure "the current
    /// working directory shall remain unchanged" (`[[source:
    /// posix-chdir]]`). The child calls it between `fork` and `exec`, which
    /// XSH 2.4.3's table of async-signal-safe functions allows (`[[source:
    /// posix-xsh-chap02]]`).
    pub fn chdir(path: *const c_char) -> c_int;

    /// A terminal's attributes, read into `t`. The child calls it on its
    /// terminal between `fork` and `exec`, which XSH 2.4.3's table of
    /// async-signal-safe functions allows, and the parent on the
    /// subsidiary it opens when the input encoding changes.
    pub fn tcgetattr(fd: Fd, t: *mut Termios) -> c_int;

    /// A terminal's attributes, set from `t`. Async-signal-safe, as
    /// [`tcgetattr`] is; POSIX leaves the effect undefined unless `t` came
    /// from `tcgetattr` on the same descriptor.
    pub fn tcsetattr(fd: Fd, when: c_int, t: *const Termios) -> c_int;

    /// The subsidiary's path for the manager `fd`, or null. The answer
    /// lives in a buffer of libc's that the next call overwrites -- POSIX
    /// says `ptsname` "need not be thread-safe" -- so the one caller takes a
    /// process-wide lock and copies it out (`pty.rs`, `subsidiary_of`).
    /// Not `ptsname_r`: its `size_t` length would be a signature primitive
    /// past `audit_all.sh`'s budget for this crate.
    pub fn ptsname(fd: Fd) -> *mut c_char;

    /// A new session with no controlling terminal: the spawned child's
    /// first call, before it takes its terminal, as `login_tty` makes it
    /// (`pty.rs`, `fork_terminal`), and the test that holds [`O_NOCTTY`]
    /// to its job, since only such a process could take one. A `pid_t` is
    /// an `int` on both platforms.
    pub fn setsid() -> c_int;

    /// **A new pty's manager, close-on-exec from the call that opens it**,
    /// with `O_RDWR | O_NOCTTY | O_CLOEXEC`: POSIX Issue 8 adds
    /// `O_CLOEXEC`, "necessary to avoid a data race in multi-threaded
    /// applications" (`[[source: posix-posix-openpt]]`). Apple's and glibc's
    /// hand the flags to `open("/dev/ptmx")` unchanged
    /// (`[[source: apple-libc-grantpt-c]]`, `[[source: glibc-2-42-getpt-c]]`),
    /// and xnu sets the descriptor's close-on-exec in `open1`
    /// (`[[source: xnu-vfs-syscalls-c]]`). Then `grantpt` and `unlockpt`
    /// before the subsidiary is opened.
    pub fn posix_openpt(flags: c_int) -> Fd;
    pub fn grantpt(fd: Fd) -> c_int;
    pub fn unlockpt(fd: Fd) -> c_int;

    /// A child of this process, `0` in the child and its pid in the parent:
    /// the spawn's own fork, once both ends of its terminal are open
    /// close-on-exec (`pty.rs`, `fork_terminal`). A `pid_t` is an `int` on
    /// both platforms.
    pub fn fork() -> c_int;

    /// `fd` copied onto `to`, the copy without close-on-exec: the child's
    /// terminal made its standard input, output and error, as `login_tty`
    /// does. On XSH 2.4.3's list of async-signal-safe functions
    /// (`[[source: posix-xsh-chap02]]`).
    pub fn dup2(fd: Fd, to: Fd) -> c_int;

    /// Reaps the child `pid`: with [`WNOHANG`] a window's watch, once its
    /// child's end is an event (`pty::reap_now`), and with no options the
    /// test that forks many at once and must not leave them behind. Never
    /// `-1`, which would reap the opener's children from under their own
    /// `try_wait`. A `pid_t` is an `int` on both platforms, and so is the
    /// status.
    pub fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;

    /// Sends `sig` to `pid`; the tests send 0, which checks only whether a
    /// process of that number exists -- a zombie still does -- and answer
    /// [`ESRCH`] once it was reaped, and [`SIGKILL`] to end children a
    /// failed test would otherwise leave waiting on each other.
    #[cfg(test)]
    pub fn kill(pid: c_int, sig: c_int) -> c_int;

    /// Waits for `id` to change state as `options` say, for the tests that
    /// need a child that has exited and not been reaped: [`WNOWAIT`] "Keep
    /// the process whose status is returned in infop in a waitable state"
    /// (`[[source: posix-waitid]]`). The record it fills is only a place to write to
    /// here, so it is taken as bytes, larger than either system's.
    #[cfg(all(test, target_os = "macos"))]
    pub fn waitid(
        idtype: c_int,
        id: core::ffi::c_uint,
        infop: *mut c_void,
        options: c_int,
    ) -> c_int;
}

/// `waitpid`'s option to answer at once, 0, when the child has not ended
/// yet: a window's watch asks only once an event says the child has
/// (`pty::reap_now`). The same value on both platforms.
pub const WNOHANG: c_int = 1;

/// A signal's disposition as `signal` takes and answers it, a pointer to a
/// handler or one of the header's sentinels.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct SigHandler(pub usize);

/// `signal`'s disposition that restores a signal's default action:
/// `SIG_DFL`, 0 on both platforms; the probe holds it.
pub const SIG_DFL: SigHandler = SigHandler(0);

/// **One past the highest signal number**, `NSIG`: 32 on Darwin, 65 under
/// glibc, whose numbers run on into the real-time signals. The probe holds
/// it. A child baud starts sets every number below it to its default
/// action ([`child_signals`]); `signal` refuses the two whose action cannot
/// change, and glibc the ones it keeps for itself, and nothing else moves.
#[cfg(target_os = "macos")]
pub const NSIG: c_int = 32;
#[cfg(not(target_os = "macos"))]
pub const NSIG: c_int = 65;

unsafe extern "C" {
    pub fn signal(sig: c_int, handler: SigHandler) -> SigHandler;
}

/// **A set of signals, `sigset_t`**: a 32-bit word on Darwin, a bit a
/// signal from the lowest; 128 bytes under glibc, `unsigned long`s with
/// the same bit order. The probe holds the size, and the bytes of a set
/// holding one signal against `sigaddset`'s.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SigSet(u32);

/// **A set of signals, `sigset_t`**: see Darwin's.
#[cfg(not(target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SigSet([c_ulong; 16]);

impl SigSet {
    /// The set with `sig` added, as `sigaddset` adds it.
    #[cfg(test)]
    pub fn with(mut self, sig: c_int) -> SigSet {
        let Ok(bit) = u32::try_from(sig - 1) else {
            return self;
        };
        #[cfg(target_os = "macos")]
        {
            self.0 |= 1 << bit;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let word = c_ulong::BITS;
            let at = usize::try_from(bit / word).ok();
            if let Some(w) = at.and_then(|i| self.0.get_mut(i)) {
                *w |= 1 << (bit % word);
            }
        }
        self
    }
}

/// `sigprocmask`'s and `pthread_sigmask`'s `how` that replaces the mask
/// with the set given: 3 on Darwin, 2 under glibc; the probe holds both.
#[cfg(target_os = "macos")]
pub const SIG_SETMASK: c_int = 3;
#[cfg(not(target_os = "macos"))]
pub const SIG_SETMASK: c_int = 2;

/// Their `how` that adds the set given to the mask: 1 on Darwin, 0 under
/// glibc.
#[cfg(all(test, target_os = "macos"))]
pub const SIG_BLOCK: c_int = 1;
#[cfg(all(test, not(target_os = "macos")))]
pub const SIG_BLOCK: c_int = 0;

/// `SIGUSR2`, a signal no one sends baud: 31 on Darwin, 12 on Linux.
#[cfg(all(test, target_os = "macos"))]
pub const SIGUSR2: c_int = 31;
#[cfg(all(test, not(target_os = "macos")))]
pub const SIGUSR2: c_int = 12;

/// `SIGHUP`, `SIGINT` and `SIGQUIT`: 1, 2 and 3 on Darwin and on Linux;
/// the probe holds them. What [`as_a_runner_launches`] blocks and ignores.
#[cfg(test)]
pub const SIGHUP: c_int = 1;
#[cfg(test)]
pub const SIGINT: c_int = 2;
#[cfg(test)]
pub const SIGQUIT: c_int = 3;

/// `signal`'s disposition that ignores a signal: `SIG_IGN`, 1 on both
/// platforms; the probe holds it.
#[cfg(test)]
pub const SIG_IGN: SigHandler = SigHandler(1);

// **The signal mask**: `sigprocmask` in a child, single-threaded after the
// fork, which empties the mask the forking thread had ([`child_signals`]);
// `pthread_sigmask` in a test, which blocks a signal on its own thread
// alone.
unsafe extern "C" {
    pub fn sigprocmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
    #[cfg(test)]
    pub fn pthread_sigmask(how: c_int, set: *const SigSet, old: *mut SigSet) -> c_int;
}

/// **Every signal at its default action, then none blocked**: what every
/// program baud starts begins with, whatever baud inherited or set -- a
/// window's shell (`pty::fork_terminal`) and every other ([`child`]).
///
/// A signal ignored stays ignored across the `exec` ("signals set to be
/// ignored (SIG_IGN) by the calling process image shall be set to be
/// ignored by the new process image", `[[source: posix-exec]]`), and a
/// shell cannot undo one ignored on its entry. baud ignores `SIGPIPE` from
/// its start, as Rust's runtime does in every program it starts
/// (`[[source: rust-std-unix-sigpipe]]` 33-34, 168); a baud started with
/// `SIGINT` ignored, as a background job of a non-interactive shell is,
/// would hand what it starts a Control-C that does nothing. The mask
/// crosses the `exec` too, and the forking thread's may hold signals a
/// launcher blocked -- cargo-watch hands its command `SIGHUP` blocked --
/// and `std::process::Command` resets neither: on macOS a child it starts,
/// by `posix_spawn` or by `fork`, from a process with `SIGHUP` blocked and
/// `SIGINT` ignored survives a `kill -HUP` and a `kill -INT` of itself.
///
/// **The actions first and the mask last**: a signal held by the mask and
/// delivered once it is emptied -- a `SIGWINCH` from a resize's
/// `TIOCSWINSZ` in the parent, say -- then finds its default action, never
/// a handler inherited from baud, which emptying the mask first would
/// leave a moment to run; baud installs none today, so the order costs
/// nothing and closes that moment. `signal` refuses the two numbers whose
/// action cannot change, and glibc the ones it keeps for itself.
///
/// # Safety
///
/// Only in a child between its `fork` and its `exec`, which has one
/// thread: it changes every signal's action for the whole process.
/// `signal` and `sigprocmask` are on XSH 2.4.3's table of
/// async-signal-safe functions (`[[source: posix-xsh-chap02]]`), all a
/// child of a multi-threaded process may call before its `exec`
/// (`[[source: posix-fork]]`).
pub unsafe fn child_signals() {
    // SAFETY: the caller's: a child before its `exec`; the set lives for
    // the call.
    unsafe {
        for sig in 1..NSIG {
            signal(sig, SIG_DFL);
        }
        sigprocmask(SIG_SETMASK, &SigSet::default(), core::ptr::null_mut());
    }
}

/// **A program baud starts, as a [`std::process::Command`] whose child
/// begins with [`child_signals`]**: every one baud starts outside a
/// window's terminal -- a `[link.*]` rule's command, `xdg-open`, a new
/// window's baud on Linux -- is made here, and nowhere else
/// (`a_program_baud_starts_is_made_only_here`). The closure makes `std`
/// start it by `fork` and `exec` rather than `posix_spawn`.
pub fn child(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(program);
    // SAFETY: between the fork and the exec the closure calls only
    // `child_signals`, which calls only what XSH 2.4.3's table lists.
    unsafe {
        cmd.pre_exec(|| {
            child_signals();
            Ok(())
        })
    };
    cmd
}

/// **`cmd` started as a runner can start a test**: `SIGHUP` blocked, as
/// cargo-watch hands its command, and `SIGINT` and `SIGQUIT` ignored, as a
/// background job of a non-interactive shell has them. A test that sends
/// signals runs under this to show its result does not depend on who
/// launched it.
#[cfg(test)]
pub fn as_a_runner_launches(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::unix::process::CommandExt;
    // SAFETY: between the fork and the exec the closure calls only
    // `sigprocmask` and `signal`, on XSH 2.4.3's table (`[[source:
    // posix-xsh-chap02]]`), over a set that lives for the call.
    unsafe {
        cmd.pre_exec(|| {
            let hup = SigSet::default().with(SIGHUP);
            sigprocmask(SIG_BLOCK, &hup, core::ptr::null_mut());
            signal(SIGINT, SIG_IGN);
            signal(SIGQUIT, SIG_IGN);
            Ok(())
        })
    }
}

/// **`f` run with `sig` blocked on this thread alone**, as a launcher's
/// mask would hold it, and the thread's mask as it was after: a test's way
/// to start a child from a blocked mask without blocking it for the
/// process.
#[cfg(test)]
pub fn blocking_on_this_thread<T>(sig: c_int, f: impl FnOnce() -> T) -> T {
    let set = SigSet::default().with(sig);
    let mut was = SigSet::default();
    // SAFETY: this thread's own mask, from a set and into one that live for
    // the call.
    let blocked = unsafe { pthread_sigmask(SIG_BLOCK, &set, &mut was) };
    assert_eq!(blocked, 0, "{sig} blocked on this thread");
    let out = f();
    // SAFETY: as above, the mask this thread had.
    unsafe { pthread_sigmask(SIG_SETMASK, &was, core::ptr::null_mut()) };
    out
}

/// **An `errno` value** as a call left it, for a failure kept past the
/// call: shown through the platform's own message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Errno(pub c_int);

impl Errno {
    /// The calling thread's `errno` now.
    pub fn last() -> Errno {
        Errno(std::io::Error::last_os_error().raw_os_error().unwrap_or(0))
    }

    /// Sets the calling thread's `errno` to this, as a call that failed
    /// with it would: a test's stand-in for a failing call.
    #[cfg(test)]
    pub fn set(self) {
        // SAFETY: the thread's own `errno`, which the call names for it
        // alone, written as an int.
        unsafe { *errno_location() = self.0 };
    }
}

#[cfg(all(test, target_os = "macos"))]
unsafe extern "C" {
    #[link_name = "__error"]
    fn errno_location() -> *mut c_int;
}

#[cfg(all(test, target_os = "linux"))]
unsafe extern "C" {
    #[link_name = "__errno_location"]
    fn errno_location() -> *mut c_int;
}

impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        std::io::Error::from_raw_os_error(self.0).fmt(f)
    }
}

/// "No child processes": what `waitpid` answers for a pid that is no
/// child of the caller's to wait for -- one something else reaped. The
/// same value on both platforms; the probe holds it.
pub const ECHILD: c_int = 10;

/// "Device not configured": what macOS's `posix_openpt` answers once
/// the ptys in use reach `kern.tty.ptmx_max` -- `ptmx_get_ioctl` returns
/// none (`[[source: xnu-tty-ptmx-c]]` 287-290, 314-319) and `ptcopen`
/// turns that into `ENXIO` (`[[source: xnu-tty-dev]]` 493-494). The same
/// value on both platforms.
#[cfg(any(target_os = "macos", test))]
pub const ENXIO: c_int = 6;

/// "No space left on device": what Linux's `posix_openpt` answers once
/// `kernel/pty/nr` reaches `kernel/pty/max` less the reserve --
/// `devpts_new_index` (`[[source: linux-devpts-inode-c]]` 466-472), which
/// `ptmx_open` returns as its error (`[[source: linux-pty]]` 792-795). The
/// same value on both platforms.
#[cfg(any(target_os = "linux", test))]
pub const ENOSPC: c_int = 28;

/// "Too many open files": this process's descriptor limit, which no
/// count of ptys explains. The same value on both platforms.
#[cfg(test)]
pub const EMFILE: c_int = 24;

/// "Invalid argument": what `waitpid` answers for an options value it
/// does not take, the one other failure POSIX gives it for a pid above 0
/// besides `ECHILD` and `EINTR`. The same value on both platforms.
#[cfg(test)]
pub const EINVAL: c_int = 22;

/// "Interrupted system call": a call a signal cut short, asked again. The
/// same value on both platforms.
#[cfg(test)]
pub const EINTR: c_int = 4;

/// "No such process": what `kill(pid, 0)` answers once a reaped child is
/// gone from the process table, and what `kevent` answers for a process
/// watch on a child that has already exited (`[[source:
/// macos-man-kqueue]]`). The same value on both platforms.
#[cfg(test)]
pub const ESRCH: c_int = 3;

/// `waitid`'s arguments for one child that has exited, left waitable:
/// `P_PID`, `WEXITED` and `WNOWAIT` (`[[source: posix-waitid]]`).
#[cfg(test)]
pub const P_PID: c_int = 1;
#[cfg(test)]
pub const WEXITED: c_int = 4;
#[cfg(all(test, target_os = "macos"))]
pub const WNOWAIT: c_int = 0x20;
#[cfg(all(test, target_os = "linux"))]
pub const WNOWAIT: c_int = 0x0100_0000;

/// `SIGCHLD`, the signal a parent is sent once its child is a zombie, and
/// so the event a window's watch reaps on (`fg::Watch`). The number
/// differs per platform.
#[cfg(target_os = "macos")]
pub const SIGCHLD: c_int = 20;

/// The signal that cannot be caught: a test ends with it the children
/// its failure left deadlocked. The same number on both platforms.
#[cfg(test)]
pub const SIGKILL: c_int = 9;

// **POSIX shared memory, for the kitty `t=s` medium**
// (`kitty_files.rs`): open an object read-only, and unlink its name once
// read, as the text requires ("read the data from the memory object and
// then unlink and close it"). `shm_open` is variadic in Darwin's
// `<sys/mman.h>`, with the mode as its variadic argument, and takes a
// fixed `mode_t` in glibc's; a read-only open passes no mode on Darwin
// and zero on glibc. `munmap` releases what `mmap` mapped of an object
// on Darwin, where a shared-memory descriptor cannot be read.
unsafe extern "C" {
    #[cfg(target_os = "macos")]
    pub fn shm_open(name: *const c_char, oflag: c_int, ...) -> Fd;
    #[cfg(not(target_os = "macos"))]
    pub fn shm_open(name: *const c_char, oflag: c_int, mode: core::ffi::c_uint) -> Fd;
    pub fn shm_unlink(name: *const c_char) -> c_int;
    pub fn munmap(addr: *mut c_void, len: MapLen) -> c_int;
}

/// `open`'s read-and-write access mode, which `posix_openpt` takes; the
/// same value on both platforms.
pub const O_RDWR: c_int = 2;

/// `open`'s flag that keeps a terminal from becoming the caller's
/// controlling terminal. The parent opens the subsidiary with it, through
/// [`std::fs::OpenOptions`]: a session leader with no controlling terminal
/// takes a terminal no session holds when it opens one without the flag
/// (measured on Darwin, `pty.rs`'s
/// `the_parent_never_takes_the_subsidiary_as_its_terminal`). Whether an
/// app the Dock started is such a leader was not checked. The bit differs
/// per platform.
#[cfg(target_os = "macos")]
pub const O_NOCTTY: c_int = 0x2_0000;
#[cfg(not(target_os = "macos"))]
pub const O_NOCTTY: c_int = 0o400;

/// `open`'s flag that fails on a symbolic link as the last component. A
/// kitty file medium's path is resolved first -- the text says "the
/// terminal emulator must follow symlinks" -- and the resolved path, which
/// has none, is opened a component at a time from `/` with this, each
/// directory and then the file, so a link swapped in after the check is
/// refused rather than followed (`kitty_files.rs`). Darwin's bit, glibc's
/// on arm64 (`asm/fcntl.h` gives aarch64 its own) and glibc's elsewhere;
/// the probe below checks the one it runs on.
#[cfg(target_os = "macos")]
pub const O_NOFOLLOW: c_int = 0x100;
#[cfg(all(not(target_os = "macos"), target_arch = "aarch64"))]
pub const O_NOFOLLOW: c_int = 0o100_000;
#[cfg(all(not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const O_NOFOLLOW: c_int = 0o400_000;

/// `open`'s flag that fails unless the path is a directory: each
/// directory above a kitty file medium's file, opened so. Darwin's bit,
/// glibc's on arm64 and glibc's elsewhere, as [`O_NOFOLLOW`].
#[cfg(target_os = "macos")]
pub const O_DIRECTORY: c_int = 0x10_0000;
#[cfg(all(not(target_os = "macos"), target_arch = "aarch64"))]
pub const O_DIRECTORY: c_int = 0o40_000;
#[cfg(all(not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const O_DIRECTORY: c_int = 0o200_000;

/// `open`'s close-on-exec flag, which `std` sets on its own opens: a
/// descriptor `openat` makes must carry it too, or the shell baud forks
/// inherits it.
#[cfg(target_os = "macos")]
pub const O_CLOEXEC: c_int = 0x100_0000;
#[cfg(not(target_os = "macos"))]
pub const O_CLOEXEC: c_int = 0o2_000_000;

/// **How a directory above a kitty file is opened: to search it, not to
/// read it**, so a directory the user may pass through and not list --
/// as the path's own resolution may -- is opened as it resolved: Darwin's
/// `O_SEARCH` (`O_EXEC | O_DIRECTORY`), glibc's `O_PATH` beside
/// [`O_DIRECTORY`]. Either descriptor serves `openat`, `fstat` and
/// `unlinkat`.
#[cfg(target_os = "macos")]
pub const O_SEARCH: c_int = 0x4010_0000;
#[cfg(not(target_os = "macos"))]
pub const O_PATH: c_int = 0o10_000_000;

/// What `openat` reads a relative path against when given no directory:
/// the working directory. Only `/` is opened against it, which is
/// absolute and so reads against nothing.
#[cfg(target_os = "macos")]
pub const AT_FDCWD: Fd = -2;
#[cfg(not(target_os = "macos"))]
pub const AT_FDCWD: Fd = -100;

// **A path walked a component at a time, and a file deleted from the
// directory it was read from** (`kitty_files.rs`): `openat` opens a name
// in a directory held open, so no link and no rename above it can move
// what is opened, and `unlinkat` deletes a name there, so a `t=t` file is
// deleted from the directory it was read from. `openat` is variadic in
// both platforms' `<fcntl.h>`, its mode the variadic argument, and these
// opens create nothing, so they pass none.
unsafe extern "C" {
    pub fn openat(dir: Fd, path: *const c_char, oflag: c_int, ...) -> Fd;
    pub fn unlinkat(dir: Fd, path: *const c_char, flag: c_int) -> c_int;
}

/// `open`'s read-only access mode, which `shm_open` takes; zero on both
/// platforms.
pub const O_RDONLY: c_int = 0;

/// A mapping that shares the object's pages: a kitty `t=s` object on
/// Darwin, whose shared memory cannot be `read` and is mapped instead.
#[cfg(target_os = "macos")]
pub const MAP_SHARED: c_int = 0x1;

/// The longest POSIX shared-memory name, its leading `/` included:
/// Darwin's `PSHMNAMLEN`, and `NAME_MAX` on glibc, whose `shm_open` opens
/// the name under `/dev/shm`. The text: names "must be no longer than the
/// maximum SHM name size supported by the OS".
#[cfg(target_os = "macos")]
pub const SHM_NAME_MAX: usize = 31;
#[cfg(not(target_os = "macos"))]
pub const SHM_NAME_MAX: usize = 255;

/// A mapping's length, as `munmap`, `madvise` and `mincore` take it: a
/// `size_t`.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct MapLen(pub usize);

/// `open`'s flag that creates the object, and `mmap`'s that lets a
/// mapping be written: the tests make the shared-memory objects a `t=s`
/// read takes.
#[cfg(all(test, target_os = "macos"))]
pub const O_CREAT: c_int = 0x200;
#[cfg(all(test, not(target_os = "macos")))]
pub const O_CREAT: c_int = 0o100;
#[cfg(any(target_os = "macos", test))]
pub const PROT_WRITE: c_int = 0x2;
#[cfg(all(test, not(target_os = "macos")))]
pub const MAP_SHARED: c_int = 0x1;

/// `tcflag_t`, the type of each of a termios's four flag words: `unsigned
/// long` on Darwin and `unsigned int` on glibc.
#[cfg(target_os = "macos")]
pub type TcFlag = c_ulong;
#[cfg(not(target_os = "macos"))]
pub type TcFlag = core::ffi::c_uint;

/// **`struct termios`, opaque but for `c_iflag` and `c_cc`.** What the
/// kernel hands out is handed back with one bit of `c_iflag` set or
/// cleared, and `c_cc` is read for the literal-next character a `?2005`
/// paste quotes with; nothing else is read or built. The size, the
/// alignment and both offsets are the header's, which the tests below hold:
/// a block smaller than the header's would let `tcgetattr` write past it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: TcFlag,
    flags: [TcFlag; 3],
    #[cfg(not(target_os = "macos"))]
    c_line: [u8; 1],
    pub c_cc: [u8; NCCS],
    rest: [u8; TERMIOS_SIZE - CC_AT - NCCS],
}

/// Where `c_cc` starts: after the four flag words, and on glibc the line
/// discipline's byte.
#[cfg(target_os = "macos")]
const CC_AT: usize = 4 * core::mem::size_of::<TcFlag>();
#[cfg(not(target_os = "macos"))]
const CC_AT: usize = 4 * core::mem::size_of::<TcFlag>() + 1;

/// `NCCS`, the control characters a termios holds.
#[cfg(target_os = "macos")]
pub const NCCS: usize = 20;
#[cfg(not(target_os = "macos"))]
pub const NCCS: usize = 32;

/// `VLNEXT`, the literal-next character's index in `c_cc`, which a
/// `?2005` paste reads on both systems; the header probe holds both.
#[cfg(target_os = "macos")]
pub const VLNEXT: usize = 14;
#[cfg(not(target_os = "macos"))]
pub const VLNEXT: usize = 15;

/// **The `c_cc` entries xterm's paste filter reads for `STTY`**, in
/// `removeControls`' order (`[[source: xterm-button]]` 2609-2655): VEOF,
/// VEOL, VERASE, VINTR, VKILL, VQUIT, VSTART, VSTOP, VSUSP, then those the
/// system has of VDISCARD, VDSUSP, VEOL2, VLNEXT, VREPRINT, VSTATUS, VSWTC
/// and VWERASE. Darwin's from its header, which the probe holds; glibc's
/// from its header (`[[source: glibc-2-40-termios-c-cc]]`), which the
/// probe holds where it runs on Linux.
#[cfg(target_os = "macos")]
pub const STTY_CHARS: &[usize] = &[0, 1, 3, 8, 5, 9, 12, 13, 10, 15, 11, 2, 14, 6, 18, 4];
#[cfg(not(target_os = "macos"))]
pub const STTY_CHARS: &[usize] = &[4, 11, 2, 0, 3, 1, 8, 9, 10, 13, 16, 15, 12, 7, 14];

/// `_POSIX_VDISABLE`, a control character that is switched off.
#[cfg(target_os = "macos")]
pub const POSIX_VDISABLE: u8 = 0xff;
#[cfg(not(target_os = "macos"))]
pub const POSIX_VDISABLE: u8 = 0;

/// Four `unsigned long` flag words, twenty control characters, padding and
/// two `unsigned long` speeds on Darwin; four `unsigned int` flag words,
/// the line discipline, thirty-two control characters, padding and two
/// `unsigned int` speeds on glibc.
#[cfg(target_os = "macos")]
const TERMIOS_SIZE: usize = 72;
#[cfg(not(target_os = "macos"))]
const TERMIOS_SIZE: usize = 60;

/// **Backspace erases a whole UTF-8 character**: with it set, the line
/// discipline's erase takes the continuation bytes before the cursor and
/// the byte that leads them (`[[source: xnu-tty]]` `ttyinput`,
/// `[[source: linux-n-tty]]` `eraser`). Not in POSIX's `<termios.h>`;
/// the same bit in Darwin's header and in Linux's.
pub const IUTF8: TcFlag = 0x4000;

/// `tcsetattr`'s "now", rather than after the output drains.
pub const TCSANOW: c_int = 0;

// **A symbol looked up rather than linked**, because it exists only from
// macOS 15: `AXPrefersNonBlinkingTextInsertionIndicator`. Linking it would
// fail to load on an older system; looking it up answers null there.
/// A locale object as `newlocale` hands one out: opaque, freed by
/// [`freelocale`]. Used to check a locale name before a shell is given it
/// on macOS, and on Linux before the Compose search falls back for a
/// locale its registry does not list.
pub type Locale = *mut c_void;

/// `newlocale`'s mask for every category. `LANG` feeds them all, and a
/// shell's `setlocale(LC_ALL, "")` fails as a whole when one cannot load,
/// so a name is checked against all of them, not the character type alone
/// (which accepts `UTF-8`, a name the whole mask refuses).
#[cfg(target_os = "macos")]
pub const LC_ALL_MASK: c_int = 63;

/// glibc's mask for every category: one bit for each of its twelve, at
/// the category's number, which skips `__LC_ALL`'s 6 -- the same all
/// libxkbcommon's `newlocale` asks for.
#[cfg(not(target_os = "macos"))]
pub const LC_ALL_MASK: c_int = 0x1fbf;

/// `nl_langinfo`'s item, an `int`.
#[cfg(target_os = "macos")]
pub type NlItem = c_int;

/// The item that names a locale's character set.
#[cfg(target_os = "macos")]
pub const CODESET: NlItem = 0;

unsafe extern "C" {
    pub fn newlocale(mask: c_int, name: *const c_char, base: Locale) -> Locale;
    pub fn freelocale(locale: Locale);
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    pub fn nl_langinfo_l(item: NlItem, locale: Locale) -> *const c_char;
}

// **The keyboard layouts the user has enabled**, for DECHEBM, which acts
// only with a Hebrew one (`layouts.rs`): HIToolbox's Text Input Sources,
// and the array calls that read their list.
#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// A `CFArrayRef` the caller releases: the enabled sources when
    /// `include_all_installed` is 0 and `properties` null.
    pub fn TISCreateInputSourceList(
        properties: *const c_void,
        include_all_installed: CfBoolean,
    ) -> CfObject;
    /// A property of a source, which the source owns; a `CFStringRef` for
    /// the category, a `CFArrayRef` of `CFStringRef` for the languages.
    pub fn TISGetInputSourceProperty(source: *const c_void, key: CfString) -> *const c_void;
    #[link_name = "kTISPropertyInputSourceLanguages"]
    pub static TIS_PROPERTY_INPUT_SOURCE_LANGUAGES: CfString;
    #[link_name = "kTISPropertyInputSourceCategory"]
    pub static TIS_PROPERTY_INPUT_SOURCE_CATEGORY: CfString;
    #[link_name = "kTISCategoryKeyboardInputSource"]
    pub static TIS_CATEGORY_KEYBOARD_INPUT_SOURCE: CfString;
}

// **What a key types with no modifier on the active layout**, kitty's key
// identity (`layouts::unshifted`): the layout's own table, read through
// `UCKeyTranslate` as kitty's macOS layer reads it.
#[cfg(target_os = "macos")]
#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    /// The keyboard layout in use, a `TISInputSourceRef` the caller
    /// releases, or null.
    pub fn TISCopyCurrentKeyboardLayoutInputSource() -> CfObject;
    /// The physical keyboard's type, which `UCKeyTranslate` is asked for.
    pub fn LMGetKbdType() -> PhysicalKeyboard;
    /// The characters `code` types under `modifiers` with `layout`'s
    /// `uchr` table: `noErr`, 0, or an error.
    pub fn UCKeyTranslate(
        layout: *const c_void,
        code: VirtualKeyCode,
        action: KeyAction,
        modifiers: ModifierKeyState,
        keyboard: KeyboardType,
        options: OptionBits,
        dead: *mut DeadKeyState,
        max: UniCharCount,
        actual: *mut UniCharCount,
        chars: *mut UniChar,
    ) -> OsStatus;
    /// A layout's `uchr` table, a `CFDataRef` the source owns.
    #[link_name = "kTISPropertyUnicodeKeyLayoutData"]
    pub static TIS_PROPERTY_UNICODE_KEY_LAYOUT_DATA: CfString;
    /// A source's identifier, a `CFStringRef` such as
    /// `com.apple.keylayout.US`: the tests find the US layout by it.
    #[cfg(test)]
    #[link_name = "kTISPropertyInputSourceID"]
    pub static TIS_PROPERTY_INPUT_SOURCE_ID: CfString;
}

/// A Mac's virtual key code, `NSEvent.keyCode`'s `UInt16`.
#[cfg(target_os = "macos")]
pub type VirtualKeyCode = u16;

/// `UCKeyTranslate`'s key action, a `UInt16`.
#[cfg(target_os = "macos")]
pub type KeyAction = u16;

/// `kUCKeyActionDisplay`: the characters a key's cap shows, the action
/// kitty's layer asks for.
#[cfg(target_os = "macos")]
pub const UC_KEY_ACTION_DISPLAY: KeyAction = 3;

/// The Carbon modifier state `UCKeyTranslate` reads, a `UInt32`: none
/// is 0.
#[cfg(target_os = "macos")]
pub type ModifierKeyState = u32;

/// `LMGetKbdType`'s `UInt8`.
#[cfg(target_os = "macos")]
pub type PhysicalKeyboard = u8;

/// The keyboard type `UCKeyTranslate` takes, a `UInt32`.
#[cfg(target_os = "macos")]
pub type KeyboardType = u32;

/// Carbon's `OptionBits`, a `UInt32`.
#[cfg(target_os = "macos")]
pub type OptionBits = u32;

/// `kUCKeyTranslateNoDeadKeysMask`: a dead key types its own character
/// rather than starting a sequence.
#[cfg(target_os = "macos")]
pub const UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: OptionBits = 1;

/// The dead-key state `UCKeyTranslate` carries between calls, a `UInt32`.
#[cfg(target_os = "macos")]
pub type DeadKeyState = u32;

/// Carbon's `UniCharCount`, an `unsigned long`.
#[cfg(target_os = "macos")]
pub type UniCharCount = c_ulong;

/// Carbon's `OSStatus`, an `SInt32`: `noErr` is 0.
#[cfg(target_os = "macos")]
pub type OsStatus = i32;

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFArrayGetCount(array: *const c_void) -> CfIndex;
    /// The bytes of a `CFData`, which the data owns.
    pub fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    /// The element, which the array owns.
    pub fn CFArrayGetValueAtIndex(array: *const c_void, index: CfIndex) -> *const c_void;
    pub fn CFEqual(a: *const c_void, b: *const c_void) -> CfBoolean;
}

/// CoreFoundation's signed count, a `long`.
#[cfg(target_os = "macos")]
pub type CfIndex = core::ffi::c_long;

/// The encoding `CFStringGetCString` is asked to write in, a `UInt32`.
#[cfg(target_os = "macos")]
pub type CfStringEncoding = u32;

/// `kCFStringEncodingUTF8`.
#[cfg(target_os = "macos")]
pub const CF_STRING_ENCODING_UTF8: CfStringEncoding = 0x0800_0100;

/// CoreFoundation's `Boolean`, an `unsigned char`.
#[cfg(target_os = "macos")]
pub type CfBoolean = u8;

/// A CoreFoundation locale, as `CFLocaleCopyCurrent` hands one out: an
/// opaque reference the caller releases.
#[cfg(target_os = "macos")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CfLocale(*const c_void);

/// A CoreFoundation string: an opaque reference, here only ever one a
/// locale owns or a framework constant, so nothing releases it.
#[cfg(target_os = "macos")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CfString(*const c_void);

#[cfg(target_os = "macos")]
impl CfLocale {
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// The locale as the object `CFRelease` releases.
    pub fn object(self) -> CfObject {
        CfObject(self.0)
    }
}

/// A CoreFoundation object the caller owns and releases once: a font, a
/// font descriptor, a data, a string or a URL CoreText hands out under the
/// Create or Copy rule.
#[cfg(target_os = "macos")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CfObject(*const c_void);

#[cfg(target_os = "macos")]
impl CfObject {
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// The object as the reference the array calls read.
    pub fn as_ptr(self) -> *const c_void {
        self.0
    }

    /// The object as the string it is, for `CFStringGetCString`: only for
    /// one a string-returning call handed out.
    pub fn string(self) -> CfString {
        CfString(self.0)
    }
}

/// A UTF-16 code unit, CoreFoundation's `UniChar`.
#[cfg(target_os = "macos")]
pub type UniChar = u16;

/// A glyph index, CoreGraphics' `CGGlyph`.
#[cfg(target_os = "macos")]
pub type CgGlyph = u16;

/// CoreGraphics' `CGFloat`, a `double` on a 64-bit target.
#[cfg(target_os = "macos")]
pub type CgFloat = f64;

/// CoreFoundation's `CFRange`: a start and a length, in UTF-16 units for
/// a string.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CfRange {
    pub location: CfIndex,
    pub length: CfIndex,
}

// **The system's face for a character no face of baud's chain draws**
// (`fallback.rs`): CoreText's own answer, asked of a font made from the
// primary's bytes, and the file it is in, which baud-font parses.
#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFDataCreate(alloc: *const c_void, bytes: *const u8, length: CfIndex) -> CfObject;
    pub fn CFStringCreateWithCharacters(
        alloc: *const c_void,
        chars: *const UniChar,
        count: CfIndex,
    ) -> CfObject;
    pub fn CFURLGetFileSystemRepresentation(
        url: CfObject,
        resolve: CfBoolean,
        buf: *mut u8,
        max: CfIndex,
    ) -> CfBoolean;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    pub fn CTFontManagerCreateFontDescriptorFromData(data: CfObject) -> CfObject;
    pub fn CTFontCreateWithFontDescriptor(
        descriptor: CfObject,
        size: CgFloat,
        matrix: *const c_void,
    ) -> CfObject;
    pub fn CTFontCreateForString(font: CfObject, text: CfObject, range: CfRange) -> CfObject;
    pub fn CTFontCopyAttribute(font: CfObject, attribute: CfString) -> CfObject;
    pub fn CTFontCopyPostScriptName(font: CfObject) -> CfObject;
    pub fn CTFontGetGlyphsForCharacters(
        font: CfObject,
        chars: *const UniChar,
        glyphs: *mut CgGlyph,
        count: CfIndex,
    ) -> bool;
    #[link_name = "kCTFontURLAttribute"]
    pub static CT_FONT_URL_ATTRIBUTE: CfString;
}

#[cfg(target_os = "macos")]
impl CfString {
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// A string another object owns, such as an element of an array: for
    /// one only, and nothing releases it.
    pub fn borrowed(p: *const c_void) -> CfString {
        CfString(p)
    }

    /// The string as the reference `CFEqual` compares.
    pub fn as_ptr(self) -> *const c_void {
        self.0
    }
}

// The user's locale, from their preferences rather than the environment,
// which a Dock launch leaves without one.
#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFLocaleCopyCurrent() -> CfLocale;
    pub fn CFLocaleGetIdentifier(locale: CfLocale) -> CfString;
    /// Declared returning a string: the two keys below are the only ones
    /// asked, and the value of each is one, or null.
    pub fn CFLocaleGetValue(locale: CfLocale, key: CfString) -> CfString;
    pub fn CFStringGetCString(
        s: CfString,
        buf: *mut c_char,
        size: CfIndex,
        encoding: CfStringEncoding,
    ) -> CfBoolean;
    pub fn CFRelease(object: CfObject);
    #[link_name = "kCFLocaleLanguageCode"]
    pub static CF_LOCALE_LANGUAGE_CODE: CfString;
    #[link_name = "kCFLocaleCountryCode"]
    pub static CF_LOCALE_COUNTRY_CODE: CfString;
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    pub fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    pub fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

// **The log's checks and its generated name** (`hostlog.rs`): xterm
// refuses a log file the real user does not own (`open_userfile`,
// `[[source: xterm-misc]]`), and names one by the host when none is
// given. `uid_t` is a 32-bit unsigned on Darwin and on Linux.
pub type Uid = u32;

unsafe extern "C" {
    pub fn getuid() -> Uid;
    pub fn gethostname(name: *mut c_char, len: NameLen) -> c_int;
}

// **Whether this thread is the process's main one** (`<pthread.h>`, Darwin
// only): the thread AppKit's main-thread marker is made on, which a
// test that must open no window holds itself off (`no_appkit.rs`).
#[cfg(all(test, target_os = "macos"))]
unsafe extern "C" {
    pub fn pthread_main_np() -> c_int;
}

/// **Whether the calling thread is the process's main thread**, where
/// AppKit's main-thread marker can be made: never a test's, which libtest
/// runs on a thread of its own.
#[cfg(all(test, target_os = "macos"))]
pub fn on_main_thread() -> bool {
    // SAFETY: `pthread_main_np` takes nothing and cannot fail.
    let main = unsafe { pthread_main_np() };
    main != 0
}

/// The room `gethostname` is given for the name: a `size_t`.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct NameLen(pub usize);

/// Seconds since the epoch, `time_t`: sixty-four bits signed on both
/// systems' 64-bit targets, which the probe pins.
pub type TimeT = i64;

/// `struct tm`, a time broken down, which `localtime_r` fills: the
/// generated log name's date and time, in local time as xterm's
/// `GenerateLogPath` takes it (`localtime`, `[[source: xterm-misc]]`).
/// Both systems carry `tm_gmtoff` and `tm_zone` after the nine fields
/// POSIX names; the probe holds every offset.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Tm {
    pub tm_sec: c_int,
    pub tm_min: c_int,
    pub tm_hour: c_int,
    pub tm_mday: c_int,
    pub tm_mon: c_int,
    pub tm_year: c_int,
    pub tm_wday: c_int,
    pub tm_yday: c_int,
    pub tm_isdst: c_int,
    pub tm_gmtoff: core::ffi::c_long,
    pub tm_zone: *const c_char,
}

impl Tm {
    /// Every field zero and no zone: what `localtime_r` writes over.
    pub const ZERO: Tm = Tm {
        tm_sec: 0,
        tm_min: 0,
        tm_hour: 0,
        tm_mday: 0,
        tm_mon: 0,
        tm_year: 0,
        tm_wday: 0,
        tm_yday: 0,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: core::ptr::null(),
    };
}

unsafe extern "C" {
    /// "The localtime_r() function shall convert the time in seconds since
    /// the Epoch pointed to by timer into a broken-down time stored in the
    /// structure to which result points"; POSIX does not require it to
    /// read `TZ`, which `tzset` does first.
    pub fn localtime_r(timer: *const TimeT, result: *mut Tm) -> *mut Tm;
    pub fn tzset();
}

// **The system's name** (`hostlog.rs`): the badge's `uname`, the
// `sysname` of `struct utsname`, as iTerm2's `bestGuessAtUName` reads it
// (`[[source: iterm2-964a75d-ptysession]]`). Darwin has five fields of
// 256 bytes, glibc six of 65, `domainname` among them under
// `_GNU_SOURCE`; the probe holds the size, the field and its length.

/// The length of each of `struct utsname`'s fields.
#[cfg(target_os = "macos")]
pub const UTS_FIELD: usize = 256;
#[cfg(not(target_os = "macos"))]
pub const UTS_FIELD: usize = 65;

/// The fields after `sysname`.
#[cfg(target_os = "macos")]
const UTS_REST: usize = 4;
#[cfg(not(target_os = "macos"))]
const UTS_REST: usize = 5;

/// `struct utsname`, which `uname` fills: each field a NUL-ended string.
#[repr(C)]
pub struct Utsname {
    pub sysname: [u8; UTS_FIELD],
    /// `nodename`, `release`, `version`, `machine`, and on glibc
    /// `domainname`, which nothing reads.
    pub rest: [[u8; UTS_FIELD]; UTS_REST],
}

impl Utsname {
    /// Every byte zero: what `uname` writes over.
    pub const ZERO: Utsname = Utsname {
        sysname: [0; UTS_FIELD],
        rest: [[0; UTS_FIELD]; UTS_REST],
    };
}

unsafe extern "C" {
    /// Fills `name` with what names this system; -1 on an error.
    pub fn uname(name: *mut Utsname) -> c_int;
}

// **Freed image pages back to the system** (`pages.rs`). The page size is
// asked, never assumed: 16 KiB on this Mac and 4 KiB on most Linux, and
// `madvise` rounds a length up, so a page assumed too small would zero a
// live neighbour's bytes. Linux gives pages back with `madvise`
// `MADV_DONTNEED`, after which a private anonymous page reads zero. Darwin
// does not: measured on this Mac, `MADV_DONTNEED`, `MADV_FREE`,
// `MADV_FREE_REUSABLE` and `MADV_ZERO` each left `resident_size` where it
// was, and `MADV_FREE_REUSABLE` lowered only the footprint, kept every
// page's bytes and needs `MADV_FREE_REUSE` before each write or the
// footprint under-counts. A fresh anonymous mapping over the pages
// (`MAP_FIXED`) lowered both and reads zero, as Linux's does.
unsafe extern "C" {
    pub fn getpagesize() -> c_int;
    #[cfg(target_os = "linux")]
    pub fn madvise(addr: *mut c_void, len: MapLen, advice: c_int) -> c_int;
}

/// `mmap`'s flag that maps at exactly the address given, replacing what
/// was there; the same bit on both platforms.
#[cfg(any(target_os = "macos", test))]
pub const MAP_FIXED: c_int = 0x10;

/// `mmap`'s flag for memory no file backs, zero-filled: Darwin's bit.
#[cfg(target_os = "macos")]
pub const MAP_ANON: c_int = 0x1000;

/// `madvise`'s advice that the pages are not needed: on Linux a private
/// anonymous page reads zero after it, and stops counting as resident.
#[cfg(target_os = "linux")]
pub const MADV_DONTNEED: c_int = 4;

/// `prctl`'s option that turns transparent huge pages off, inherited
/// across `fork` and kept across `execve`
/// (`[[source: linux-pr-set-thp-disable-2const]]`). The man page says it
/// sets the flag for the calling thread; the kernel sets it on the
/// thread's memory map, the whole address space
/// (`[[source: linux-sys-c-v6-8]]` 2620-2630), so it holds for every
/// thread of the process. It is what the page tests'
/// measuring child sets (`pages.rs`), so a 2 MB page the kernel commits
/// whole is not read as a page baud kept.
#[cfg(all(test, target_os = "linux"))]
pub const PR_SET_THP_DISABLE: c_int = 41;

#[cfg(all(test, target_os = "linux"))]
unsafe extern "C" {
    pub fn prctl(option: c_int, ...) -> c_int;
}

/// A Mach port name, as `mach_task_self` answers it: a `natural_t`.
#[cfg(all(test, target_os = "macos"))]
pub type MachPort = c_uint;

/// `task_info`'s flavor for [`MachTaskBasicInfo`], a `natural_t`.
#[cfg(all(test, target_os = "macos"))]
pub const MACH_TASK_BASIC_INFO: c_uint = 20;

/// Mach's VM size, sixty-four bits: a byte count in a task's address
/// space.
#[cfg(all(test, target_os = "macos"))]
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
pub struct VmSize(pub u64);

/// Seconds and microseconds, Mach's `time_value_t`.
#[cfg(all(test, target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TimeValue {
    pub seconds: c_int,
    pub microseconds: c_int,
}

/// `struct mach_task_basic_info`: what a test reads its own resident
/// size from, the number that falls when a page is given back --
/// `getrusage` and `/usr/bin/time -l` report the peak, which never falls.
#[cfg(all(test, target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MachTaskBasicInfo {
    pub virtual_size: VmSize,
    pub resident_size: VmSize,
    pub resident_size_max: VmSize,
    pub user_time: TimeValue,
    pub system_time: TimeValue,
    pub policy: c_int,
    pub suspend_count: c_int,
}

/// `task_info`'s flavor for [`TaskVmInfo`], a `natural_t`.
#[cfg(all(test, target_os = "macos"))]
pub const TASK_VM_INFO: c_uint = 22;

/// `struct task_vm_info` through `phys_footprint`, the length
/// `TASK_VM_INFO_REV1_COUNT` names: what a test reads its own footprint
/// from -- the memory the system charges the process, which a free the
/// allocator keeps as reusable pages leaves, where the resident size may
/// not fall.
#[cfg(all(test, target_os = "macos"))]
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct TaskVmInfo {
    pub virtual_size: VmSize,
    pub region_count: c_int,
    pub page_size: c_int,
    pub resident_size: VmSize,
    /// `resident_size_peak` through `compressed_lifetime`, unread.
    pub between: [VmSize; 15],
    pub phys_footprint: VmSize,
}

/// `mincore`'s bit for a page in memory, in each byte of the vector it
/// fills (`<sys/mman.h>`): what a test reads a block's own pages from,
/// where the task's footprint counts every thread's.
#[cfg(all(test, target_os = "macos"))]
pub const MINCORE_INCORE: c_char = 0x1;

/// `mincore`'s bit for a page written and since compressed or paged out
/// (`<sys/mman.h>`), which a page in memory does not carry.
#[cfg(all(test, target_os = "macos"))]
pub const MINCORE_PAGED_OUT: c_char = 0x20;

// `mincore` fills one byte a page of `len` bytes from `addr`, a page's
// address; Darwin's header takes the address `const` and the vector as
// `char`.
#[cfg(all(test, target_os = "macos"))]
unsafe extern "C" {
    pub fn mincore(addr: *const c_void, len: MapLen, vec: *mut c_char) -> c_int;
}

#[cfg(all(test, target_os = "macos"))]
unsafe extern "C" {
    pub static mach_task_self_: MachPort;
    pub fn task_info(task: MachPort, flavor: c_uint, info: *mut c_int, count: *mut c_uint)
    -> c_int;
}

// **What the window's title is derived from when a program sets none**
// (`fg.rs`, note AK section 2): the pty's foreground process group, its
// program's argv[0] and its directory. `TIOCGPGRP` is asked on the
// master, which both kernels answer although POSIX's `tcgetpgrp` requires
// the controlling terminal: XNU's pty driver answers it "on the controller"
// before that check (`[[source: xnu-tty-dev]]` 1023-1031), and Linux
// lets a master pty through (`[[source: linux-tty-jobctrl]]` 470-475).

/// A process id, `pid_t`: thirty-two bits signed on both systems, which
/// the probe pins.
pub type Pid = c_int;

/// Read the terminal's foreground process group into an `int`.
#[cfg(target_os = "macos")]
pub const TIOCGPGRP: c_ulong = 0x4004_7477;
#[cfg(not(target_os = "macos"))]
pub const TIOCGPGRP: c_ulong = 0x540f;

/// `proc_pidinfo`'s flavor for `struct proc_vnodepathinfo`, whose first
/// member, `pvi_cdir`, is the current directory's `vnode_info_path`
/// (`[[source: macos-sdk-sys-proc-info-h]]`). The struct is taken as bytes:
/// only the directory's path is read, at the offset the probe pins, and
/// `MAXPATHLEN` bytes long.
#[cfg(target_os = "macos")]
pub const PROC_PIDVNODEPATHINFO: c_int = 9;
#[cfg(target_os = "macos")]
pub const VNODEPATHINFO_SIZE: usize = 2352;
#[cfg(target_os = "macos")]
pub const VNODEPATHINFO_CDIR_PATH: usize = 152;
#[cfg(target_os = "macos")]
pub const MAXPATHLEN: usize = 1024;

/// `proc_pidpath`'s buffer, `PROC_PIDPATHINFO_MAXSIZE` (four `MAXPATHLEN`).
#[cfg(target_os = "macos")]
pub const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;

/// `sysctl`'s names for a process's arguments with its `argc` first:
/// `{CTL_KERN, KERN_PROCARGS2, pid}`.
#[cfg(target_os = "macos")]
pub const CTL_KERN: c_int = 1;
#[cfg(target_os = "macos")]
pub const KERN_PROCARGS2: c_int = 49;

// **What the activity lights' OH and CD read of the job in front**
// (`front.rs`, note AH 3.2 and 3.3), through the same "private interfaces"
// (`[[source: macos-sdk-libproc-h]]` 41-44) and `sys/proc_info.h`'s
// records (`[[source: macos-sdk-sys-proc-info-h]]`), each taken as bytes
// at the offsets the probe pins, as `proc_vnodepathinfo` is.

/// `proc_pidinfo`'s flavor for `struct proc_bsdinfo`: `pbi_pgid`, the
/// process's own group, and `e_tpgid`, "tty process group id", the
/// foreground group of its controlling terminal.
#[cfg(target_os = "macos")]
pub const PROC_PIDTBSDINFO: c_int = 3;
#[cfg(target_os = "macos")]
pub const BSDINFO_SIZE: usize = 136;
#[cfg(target_os = "macos")]
pub const BSDINFO_PGID: usize = 100;
#[cfg(target_os = "macos")]
pub const BSDINFO_TPGID: usize = 112;
/// `pbi_start_tvsec` and `pbi_start_tvusec`: when the process started, the
/// key that tells a reused pid from the process that had it.
#[cfg(target_os = "macos")]
pub const BSDINFO_START_SEC: usize = 120;
#[cfg(target_os = "macos")]
pub const BSDINFO_START_USEC: usize = 128;

/// `proc_pidinfo`'s flavor for `struct proc_bsdshortinfo`, whose
/// `pbsi_comm` is the program's name, `MAXCOMLEN` bytes: answered for a
/// root process too (note AH 3.8, measured against pid 1).
#[cfg(target_os = "macos")]
pub const PROC_PIDT_SHORTBSDINFO: c_int = 13;
#[cfg(target_os = "macos")]
pub const BSDSHORTINFO_SIZE: usize = 64;
#[cfg(target_os = "macos")]
pub const BSDSHORTINFO_COMM: usize = 16;
#[cfg(target_os = "macos")]
pub const MAXCOMLEN: usize = 16;

/// `proc_pidinfo`'s flavor for the process's descriptors, an array of
/// `struct proc_fdinfo`, `proc_fd` then `proc_fdtype`; a socket's type.
#[cfg(target_os = "macos")]
pub const PROC_PIDLISTFDS: c_int = 1;
#[cfg(target_os = "macos")]
pub const FDINFO_SIZE: usize = 8;
#[cfg(target_os = "macos")]
pub const FDINFO_FD: usize = 0;
#[cfg(target_os = "macos")]
pub const FDINFO_TYPE: usize = 4;
#[cfg(target_os = "macos")]
pub const PROX_FDTYPE_SOCKET: c_int = 2;

/// `proc_pidfdinfo`'s flavor for `struct socket_fdinfo`: its `soi_kind`,
/// `SOCKINFO_TCP` for a TCP socket, and that socket's `tcpsi_state`,
/// `TSI_S_ESTABLISHED` once connected.
#[cfg(target_os = "macos")]
pub const PROC_PIDFDSOCKETINFO: c_int = 3;
#[cfg(target_os = "macos")]
pub const SOCKET_FDINFO_SIZE: usize = 792;
#[cfg(target_os = "macos")]
pub const SOCKET_FDINFO_KIND: usize = 256;
#[cfg(target_os = "macos")]
pub const SOCKET_FDINFO_TCP_STATE: usize = 344;
#[cfg(target_os = "macos")]
pub const SOCKINFO_TCP: c_int = 2;
#[cfg(target_os = "macos")]
pub const TSI_S_ESTABLISHED: c_int = 4;

/// A `uint64_t` argument to `proc_pidinfo`: a tid or a descriptor for
/// some flavors, zero for this one.
#[cfg(target_os = "macos")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct ProcArg(pub u64);

/// A buffer's size as `proc_pidpath` takes it, a `uint32_t`.
#[cfg(target_os = "macos")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PathSize(pub u32);

/// One `kevent` record, `struct kevent`: `uintptr_t ident; int16_t
/// filter; uint16_t flags; uint32_t fflags; intptr_t data; void *udata`
/// (`[[source: macos-sdk-sys-event-h]]`), in the C types that name them on
/// this LP64 system; the probe holds every offset.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Kevent {
    pub ident: c_ulong,
    pub filter: i16,
    pub flags: core::ffi::c_ushort,
    pub fflags: c_uint,
    pub data: core::ffi::c_long,
    pub udata: *mut c_void,
}

/// `kevent`'s filter for a process, and the three events watched:
/// "NOTE_EXIT The process has exited", "NOTE_FORK The process created a
/// child process via fork(2) or similar call", "NOTE_EXEC The process
/// executed a new process via execve(2) or similar call" (`[[source:
/// macos-man-kqueue]]` 353-371).
#[cfg(target_os = "macos")]
pub const EVFILT_PROC: i16 = -5;
#[cfg(target_os = "macos")]
pub const EV_ADD: core::ffi::c_ushort = 0x1;
#[cfg(target_os = "macos")]
pub const EV_CLEAR: core::ffi::c_ushort = 0x20;

/// `kevent`'s filter for room to write, and the flag that deletes a
/// registration once it has fired: "EVFILT_WRITE Takes a file descriptor
/// as the identifier, and returns whenever it is possible to write to the
/// descriptor", and "EV_ONESHOT Causes the event to return only the first
/// occurrence of the filter being triggered. After the user retrieves the
/// event from the kqueue, it is deleted" (`[[source: macos-man-kqueue]]`
/// 293-299, 200-202). The reply outbox's watch arms it on the master while
/// the kernel has not taken every byte owed (`pty::outbox_watch`). The
/// page names sockets, pipes and fifos and not a pty; the watch's test is
/// the proof it fires on a master whose subsidiary read.
#[cfg(target_os = "macos")]
pub const EVFILT_WRITE: i16 = -2;
#[cfg(target_os = "macos")]
pub const EV_ONESHOT: core::ffi::c_ushort = 0x10;
#[cfg(target_os = "macos")]
pub const NOTE_EXIT: c_uint = 0x8000_0000;
#[cfg(target_os = "macos")]
pub const NOTE_FORK: c_uint = 0x4000_0000;
#[cfg(target_os = "macos")]
pub const NOTE_EXEC: c_uint = 0x2000_0000;

/// `kevent`'s filter for an event the process raises itself, and the flag
/// that raises it.
///
/// "On input, NOTE_TRIGGER causes the event to be triggered for output"
/// (`[[source: macos-sdk-sys-event-h]]` 77, 203-205); a process watch's
/// close wakes its thread with it (`fg::Watch`).
#[cfg(target_os = "macos")]
pub const EVFILT_USER: i16 = -10;

/// `kevent`'s filter for a signal sent to this process: it "returns when
/// the given signal is generated for the process. This coexists with the
/// signal() and sigaction() facilities, and has a lower precedence", and
/// "will record all attempts to deliver a signal to a process, even if the
/// signal has been marked as SIG_IGN" (`[[source: macos-man-kqueue]]`
/// 383-390). A window's watch takes [`SIGCHLD`] on it, which leaves the
/// signal's default action as it was.
#[cfg(target_os = "macos")]
pub const EVFILT_SIGNAL: i16 = -6;
#[cfg(target_os = "macos")]
pub const NOTE_TRIGGER: c_uint = 0x0100_0000;

/// `kevent`'s filter for a file or a directory, and the events the config
/// watch asks for (`reload.rs`): "EVFILT_VNODE Takes a file descriptor as
/// the identifier and the events to watch for in fflags" -- `NOTE_DELETE`
/// "The unlink() system call was called on the file referenced by the
/// descriptor", `NOTE_WRITE` "A write occurred on the file referenced by
/// the descriptor", `NOTE_EXTEND`, `NOTE_ATTRIB`, `NOTE_LINK` "The link
/// count on the file changed", `NOTE_RENAME` "The file referenced by the
/// descriptor was renamed" and `NOTE_REVOKE` (`[[source:
/// macos-man-kqueue]]` 310-334). A directory's entries are what its
/// `NOTE_WRITE` reports: a file created, removed or renamed over in it.
#[cfg(target_os = "macos")]
pub const EVFILT_VNODE: i16 = -4;
#[cfg(target_os = "macos")]
pub const NOTE_DELETE: c_uint = 0x1;
#[cfg(target_os = "macos")]
pub const NOTE_WRITE: c_uint = 0x2;
#[cfg(target_os = "macos")]
pub const NOTE_EXTEND: c_uint = 0x4;
#[cfg(target_os = "macos")]
pub const NOTE_ATTRIB: c_uint = 0x8;
#[cfg(target_os = "macos")]
pub const NOTE_LINK: c_uint = 0x10;
#[cfg(target_os = "macos")]
pub const NOTE_RENAME: c_uint = 0x20;
#[cfg(target_os = "macos")]
pub const NOTE_REVOKE: c_uint = 0x40;

/// `open`'s flag for a "descriptor requested for event notifications
/// only" (`[[source: macos-sdk-sys-fcntl-h]]` 130): what the config watch
/// opens each file and directory with, so a watched volume can still be
/// unmounted. Darwin's alone.
#[cfg(target_os = "macos")]
pub const O_EVTONLY: c_int = 0x8000;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// "private interfaces to obtain process information. These
    /// interfaces are subject to change in future releases"
    /// (`[[source: macos-sdk-libproc-h]]` 41-44): the only interfaces
    /// macOS has for another process's directory and path.
    pub fn proc_pidinfo(
        pid: Pid,
        flavor: c_int,
        arg: ProcArg,
        buffer: *mut c_void,
        size: c_int,
    ) -> c_int;
    pub fn proc_pidpath(pid: Pid, buffer: *mut c_void, size: PathSize) -> c_int;
    pub fn proc_listchildpids(ppid: Pid, buffer: *mut c_void, size: c_int) -> c_int;
    /// "a count of pids" in the group (`[[source: xnu-libproc-c]]` 76-85).
    pub fn proc_listpgrppids(pgrpid: Pid, buffer: *mut c_void, size: c_int) -> c_int;
    pub fn proc_pidfdinfo(
        pid: Pid,
        fd: c_int,
        flavor: c_int,
        buffer: *mut c_void,
        size: c_int,
    ) -> c_int;
    pub fn sysctl(
        name: *mut c_int,
        namelen: c_uint,
        oldp: *mut c_void,
        oldlenp: *mut MapLen,
        newp: *mut c_void,
        newlen: MapLen,
    ) -> c_int;
    pub fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut MapLen,
        newp: *mut c_void,
        newlen: MapLen,
    ) -> c_int;
    pub fn kqueue() -> Fd;
    pub fn kevent(
        kq: Fd,
        changelist: *const Kevent,
        nchanges: c_int,
        eventlist: *mut Kevent,
        nevents: c_int,
        timeout: *const Timespec,
    ) -> c_int;
}

/// **`struct statfs`, as `getfsstat` fills it** on macOS, the 64-bit-inode
/// layout every arm64 build has and an x86_64 build gets by name
/// (`[[source: macos-sdk-sys-mount-h]]`, `__DARWIN_STRUCT_STATFS64`; the
/// suffix from `[[source: macos-sdk-sys-cdefs-h]]`'s
/// `__DARWIN_SUF_64_BIT_INO_T`). Read for the printed paths' mount check
/// (`probe.rs`): the flags, the type's name and where it is mounted.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Statfs {
    pub f_bsize: u32,
    pub f_iosize: i32,
    pub f_blocks: u64,
    pub f_bfree: u64,
    pub f_bavail: u64,
    pub f_files: u64,
    pub f_ffree: u64,
    pub f_fsid: [i32; 2],
    pub f_owner: Uid,
    pub f_type: u32,
    pub f_flags: u32,
    pub f_fssubtype: u32,
    pub f_fstypename: [c_char; MFSTYPENAMELEN],
    pub f_mntonname: [c_char; MAXPATHLEN],
    pub f_mntfromname: [c_char; MAXPATHLEN],
    pub f_flags_ext: u32,
    pub f_reserved: [u32; 7],
}

/// `MFSTYPENAMELEN`, the type name's field with its NUL.
#[cfg(target_os = "macos")]
pub const MFSTYPENAMELEN: usize = 16;

/// `MNT_LOCAL`: "filesystem is stored locally"
/// (`[[source: macos-sdk-sys-mount-h]]`).
#[cfg(target_os = "macos")]
pub const MNT_LOCAL: u32 = 0x0000_1000;

/// `MNT_NOWAIT`: `getfsstat` "will directly return the information
/// retained in the kernel to avoid delays caused by waiting for updated
/// information from a file system that is perhaps temporarily unable to
/// respond" (`[[source: macos-man-getfsstat-2]]`).
#[cfg(target_os = "macos")]
pub const MNT_NOWAIT: c_int = 2;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// `getfsstat(struct statfs *buf, int bufsize, int flags)`: with a null
    /// `buf` "just the number of mounted file systems", else up to
    /// `bufsize` bytes of them (`[[source: macos-man-getfsstat-2]]`).
    #[cfg_attr(target_arch = "x86_64", link_name = "getfsstat$INODE64")]
    pub fn getfsstat(buf: *mut Statfs, bufsize: c_int, flags: c_int) -> c_int;
}

/// **The mount table, by `getfsstat` with `MNT_NOWAIT`**: asked once for the
/// count and once with room for a few more, as a mount may come between the
/// two; none when either call fails.
#[cfg(target_os = "macos")]
pub fn mounted() -> Option<Vec<Statfs>> {
    // SAFETY: a null buffer: the call writes nothing and answers the count.
    let n = unsafe { getfsstat(std::ptr::null_mut(), 0, MNT_NOWAIT) };
    let room = usize::try_from(n).ok()?.checked_add(8)?;
    let zero = Statfs {
        f_bsize: 0,
        f_iosize: 0,
        f_blocks: 0,
        f_bfree: 0,
        f_bavail: 0,
        f_files: 0,
        f_ffree: 0,
        f_fsid: [0; 2],
        f_owner: 0,
        f_type: 0,
        f_flags: 0,
        f_fssubtype: 0,
        f_fstypename: [0; MFSTYPENAMELEN],
        f_mntonname: [0; MAXPATHLEN],
        f_mntfromname: [0; MAXPATHLEN],
        f_flags_ext: 0,
        f_reserved: [0; 7],
    };
    let mut buf = vec![zero; room];
    let bytes = c_int::try_from(room.checked_mul(size_of::<Statfs>())?).ok()?;
    // SAFETY: a buffer of `bytes` bytes, the size passed beside it.
    let got = unsafe { getfsstat(buf.as_mut_ptr(), bytes, MNT_NOWAIT) };
    buf.truncate(usize::try_from(got).ok()?);
    Some(buf)
}

/// `IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES`: the policy for "dataless
/// files accessed by the current thread or process"
/// (`[[source: macos-man-getiopolicy-np-3]]`; the value from
/// `[[source: macos-sdk-sys-resource-h]]`).
#[cfg(target_os = "macos")]
pub const IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES: c_int = 3;

/// `IOPOL_SCOPE_THREAD`: the calling thread only.
#[cfg(target_os = "macos")]
pub const IOPOL_SCOPE_THREAD: c_int = 1;

/// `IOPOL_MATERIALIZE_DATALESS_FILES_OFF`: "Disables materialization of
/// dataless files by the current thread or process"
/// (`[[source: macos-man-getiopolicy-np-3]]`).
#[cfg(target_os = "macos")]
pub const IOPOL_MATERIALIZE_DATALESS_FILES_OFF: c_int = 1;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// `int setiopolicy_np(int iotype, int scope, int policy)`, 0 or -1
    /// (`[[source: macos-sdk-sys-resource-h]]`).
    fn setiopolicy_np(iotype: c_int, scope: c_int, policy: c_int) -> c_int;
}

/// **No file of a cloud folder fetched by this thread's lookups**: the
/// printed paths' worker sets it on itself before its first `lstat`, so a
/// name inside a File Provider folder that is only a placeholder is not
/// fetched to answer it. Whether the call took, never what it prevented:
/// that is unverified here.
#[cfg(target_os = "macos")]
pub fn no_dataless_fetch() -> bool {
    // SAFETY: three integers in, one out; it changes only this thread.
    let r = unsafe {
        setiopolicy_np(
            IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES,
            IOPOL_SCOPE_THREAD,
            IOPOL_MATERIALIZE_DATALESS_FILES_OFF,
        )
    };
    r == 0
}

// **A window's child, as a descriptor, on Linux** (`pty.rs`): `pidfd_open`
// "creates a file descriptor that refers to the task referenced by pid",
// and "When the task that it refers to terminates and becomes a zombie,
// these interfaces indicate the file descriptor as readable" (`[[source:
// linux-man-pidfd-open]]` 28-29, 168-169), so the loop's one poll hears the
// child's end beside the master's, and a `waitpid` then finds the zombie.
// The page gives it as `syscall(SYS_pidfd_open, pid, flags)`, so it is the
// raw call by its number, 434 in the table every architecture shares from
// 5.3 on; an older kernel answers `ENOSYS`.
#[cfg(target_os = "linux")]
pub const SYS_PIDFD_OPEN: core::ffi::c_long = 434;
/// `close_range(first, last, flags)`, which "closes all open file
/// descriptors from first to last (included)" (`[[source:
/// linux-man-close-range]]`), by its number, 436 in the shared table
/// (`[[source: linux-asm-generic-unistd-h]]` 781), from Linux 5.9; an older
/// kernel answers `ENOSYS`.
#[cfg(target_os = "linux")]
pub const SYS_CLOSE_RANGE: core::ffi::c_long = 436;
#[cfg(target_os = "linux")]
unsafe extern "C" {
    /// Variadic in the header: the call's own arguments follow its number.
    pub fn syscall(number: core::ffi::c_long, ...) -> core::ffi::c_long;
}

// **The config watch on Linux** (`reload.rs`): "When a directory is
// monitored, inotify will return events for the directory itself, and for
// files inside the directory" (`[[source: linux-man-inotify-7]]` 15-16),
// so each directory a config file sits in is one watch, and a file swapped
// in by `rename(2)` is an `IN_MOVED_TO` in it. `inotify_init1` takes
// `IN_NONBLOCK` and `IN_CLOEXEC`, which are `O_NONBLOCK` and `O_CLOEXEC`
// (`[[source: linux-uapi-inotify-h]]` 72-73); the descriptor is read until
// it would wait, and the records are not parsed: any one is a reason to
// read the config's files again.
#[cfg(target_os = "linux")]
unsafe extern "C" {
    pub fn inotify_init1(flags: c_int) -> Fd;
    pub fn inotify_add_watch(fd: Fd, path: *const c_char, mask: InotifyMask) -> c_int;
    pub fn inotify_rm_watch(fd: Fd, wd: c_int) -> c_int;
}

/// The events an inotify watch asks for, `inotify_add_watch`'s `uint32_t`
/// mask: the `IN_` bits below, or'd.
#[cfg(target_os = "linux")]
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct InotifyMask(pub u32);

/// The inotify events the watch asks for (`[[source: linux-uapi-inotify-h]]`
/// 31-41): a file written and closed, moved in or out, created, deleted,
/// its attributes changed, and the watched directory itself deleted or
/// moved.
#[cfg(target_os = "linux")]
pub const IN_MODIFY: u32 = 0x2;
#[cfg(target_os = "linux")]
pub const IN_ATTRIB: u32 = 0x4;
#[cfg(target_os = "linux")]
pub const IN_CLOSE_WRITE: u32 = 0x8;
#[cfg(target_os = "linux")]
pub const IN_MOVED_FROM: u32 = 0x40;
#[cfg(target_os = "linux")]
pub const IN_MOVED_TO: u32 = 0x80;
#[cfg(target_os = "linux")]
pub const IN_CREATE: u32 = 0x100;
#[cfg(target_os = "linux")]
pub const IN_DELETE: u32 = 0x200;
#[cfg(target_os = "linux")]
pub const IN_DELETE_SELF: u32 = 0x400;
#[cfg(target_os = "linux")]
pub const IN_MOVE_SELF: u32 = 0x800;

/// `struct timespec`, `kevent`'s timeout: seconds and nanoseconds.
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timespec {
    pub tv_sec: TimeT,
    pub tv_nsec: core::ffi::c_long,
}

/// **Compiles a test's C probe** from `src` to `bin` with the compiler
/// `CC` names, else `cc`, else `clang`, `args` before `-o`. When none
/// runs and compiles the probe, the test fails naming what it needs and
/// each compiler it tried, with what that compiler said: `holds` is what
/// the probe holds to the system headers.
#[cfg(test)]
pub(crate) fn compile_probe(
    args: &[&str],
    src: &std::path::Path,
    bin: &std::path::Path,
    holds: &str,
) {
    let tried: Vec<String> = match std::env::var("CC") {
        Ok(cc) => vec![cc],
        Err(_) => vec!["cc".to_string(), "clang".to_string()],
    };
    let mut said = Vec::new();
    for cc in &tried {
        match std::process::Command::new(cc)
            .args(args)
            .arg("-o")
            .arg(bin)
            .arg(src)
            .output()
        {
            Ok(out) if out.status.success() => return,
            Ok(out) => said.push(format!(
                "`{cc}` did not compile it:\n{}",
                String::from_utf8_lossy(&out.stderr)
            )),
            Err(e) => said.push(format!("`{cc}` did not run: {e}")),
        }
    }
    panic!(
        "the C header probe needs a C compiler, cc or clang on PATH or one CC names, \
         to hold {holds} to the system headers; {}",
        said.join("; ")
    );
}

/// **The declarations above, checked against the platform's own headers.**
///
/// A fixture generated once on one machine could not do this: `TIOCSWINSZ`
/// is `0x80087467` on Darwin and `0x5414` on glibc, and `nfds_t` is four
/// bytes on one and eight on the other, so the only honest check is the
/// one that asks the headers of whatever it is running on. The probe is
/// written, compiled and run at test time for that reason.
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{align_of, offset_of, size_of};

    const PROBE: &str = r#"
#ifdef __linux__
#define _GNU_SOURCE
#endif
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/types.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <errno.h>
#include <signal.h>
#include <string.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <Carbon/Carbon.h>
#include <CoreFoundation/CoreFoundation.h>
#include <CoreText/CoreText.h>
#include <mach/mach.h>
#include <sys/posix_shm.h>
#include <dlfcn.h>
#include <langinfo.h>
#include <locale.h>
#include <xlocale.h>
#include <libproc.h>
#include <sys/event.h>
#include <sys/proc_info.h>
#include <sys/sysctl.h>
#include <sys/mount.h>
#include <sys/resource.h>
#else
#include <locale.h>
#include <sys/inotify.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#endif
int main(void) {
    printf("winsize.size=%zu\n", sizeof(struct winsize));
    printf("winsize.row=%zu\n", offsetof(struct winsize, ws_row));
    printf("winsize.col=%zu\n", offsetof(struct winsize, ws_col));
    printf("winsize.xpixel=%zu\n", offsetof(struct winsize, ws_xpixel));
    printf("winsize.ypixel=%zu\n", offsetof(struct winsize, ws_ypixel));
    printf("pollfd.size=%zu\n", sizeof(struct pollfd));
    printf("pollfd.fd=%zu\n", offsetof(struct pollfd, fd));
    printf("pollfd.events=%zu\n", offsetof(struct pollfd, events));
    printf("pollfd.revents=%zu\n", offsetof(struct pollfd, revents));
    printf("nfds.size=%zu\n", sizeof(nfds_t));
    printf("request.size=%zu\n", sizeof(unsigned long));
    printf("termios.size=%zu\n", sizeof(struct termios));
    printf("termios.align=%zu\n", _Alignof(struct termios));
    printf("termios.iflag=%zu\n", offsetof(struct termios, c_iflag));
    printf("termios.cc=%zu\n", offsetof(struct termios, c_cc));
    printf("NCCS=%lld\n", (long long)NCCS);
    printf("VLNEXT=%lld\n", (long long)VLNEXT);
    printf("_POSIX_VDISABLE=%lld\n", (long long)_POSIX_VDISABLE);
    printf("tcflag.size=%zu\n", sizeof(tcflag_t));
    printf("TIOCSWINSZ=%llu\n", (unsigned long long)TIOCSWINSZ);
    printf("TIOCSCTTY=%llu\n", (unsigned long long)TIOCSCTTY);
    printf("TIOCGWINSZ=%llu\n", (unsigned long long)TIOCGWINSZ);
    printf("POLLIN=%lld\n", (long long)POLLIN);
    printf("POLLOUT=%lld\n", (long long)POLLOUT);
    printf("POLLNVAL=%lld\n", (long long)POLLNVAL);
    printf("F_GETFL=%lld\n", (long long)F_GETFL);
    printf("F_SETFL=%lld\n", (long long)F_SETFL);
    printf("F_GETFD=%lld\n", (long long)F_GETFD);
    printf("F_SETFD=%lld\n", (long long)F_SETFD);
    printf("FD_CLOEXEC=%lld\n", (long long)FD_CLOEXEC);
    printf("RLIMIT_NOFILE=%lld\n", (long long)RLIMIT_NOFILE);
    printf("rlimit.size=%zu\n", sizeof(struct rlimit));
    printf("rlimit.cur=%zu\n", offsetof(struct rlimit, rlim_cur));
    printf("rlimit.max=%zu\n", offsetof(struct rlimit, rlim_max));
    printf("rlim.size=%zu\n", sizeof(rlim_t));
    printf("O_NONBLOCK=%lld\n", (long long)O_NONBLOCK);
    printf("O_NOCTTY=%lld\n", (long long)O_NOCTTY);
    printf("O_RDWR=%lld\n", (long long)O_RDWR);
    printf("O_RDONLY=%lld\n", (long long)O_RDONLY);
    printf("O_CREAT=%lld\n", (long long)O_CREAT);
    printf("PROT_WRITE=%lld\n", (long long)PROT_WRITE);
    printf("O_NOFOLLOW=%lld\n", (long long)O_NOFOLLOW);
    printf("O_DIRECTORY=%lld\n", (long long)O_DIRECTORY);
    printf("O_CLOEXEC=%lld\n", (long long)O_CLOEXEC);
    printf("AT_FDCWD=%lld\n", (long long)AT_FDCWD);
    printf("MAP_SHARED=%lld\n", (long long)MAP_SHARED);
    printf("size_t.size=%zu\n", sizeof(size_t));
    printf("uid_t.size=%zu\n", sizeof(uid_t));
    printf("time_t.size=%zu\n", sizeof(time_t));
    printf("time_t.signed=%d\n", (time_t)-1 < 0);
    printf("tm.size=%zu\n", sizeof(struct tm));
    printf("tm.sec=%zu\n", offsetof(struct tm, tm_sec));
    printf("tm.min=%zu\n", offsetof(struct tm, tm_min));
    printf("tm.hour=%zu\n", offsetof(struct tm, tm_hour));
    printf("tm.mday=%zu\n", offsetof(struct tm, tm_mday));
    printf("tm.mon=%zu\n", offsetof(struct tm, tm_mon));
    printf("tm.year=%zu\n", offsetof(struct tm, tm_year));
    printf("tm.wday=%zu\n", offsetof(struct tm, tm_wday));
    printf("tm.yday=%zu\n", offsetof(struct tm, tm_yday));
    printf("tm.isdst=%zu\n", offsetof(struct tm, tm_isdst));
    printf("tm.gmtoff=%zu\n", offsetof(struct tm, tm_gmtoff));
    printf("tm.zone=%zu\n", offsetof(struct tm, tm_zone));
    printf("utsname.size=%zu\n", sizeof(struct utsname));
    printf("utsname.sysname=%zu\n", offsetof(struct utsname, sysname));
    printf("utsname.field=%zu\n", sizeof(((struct utsname *)0)->sysname));
    printf("uid_t.signed=%d\n", (uid_t)-1 < 0);
    printf("PROT_READ=%lld\n", (long long)PROT_READ);
    printf("MAP_PRIVATE=%lld\n", (long long)MAP_PRIVATE);
    printf("MAP_FAILED=%lld\n", (long long)(intptr_t)MAP_FAILED);
    printf("IUTF8=%lld\n", (long long)IUTF8);
    printf("TCSANOW=%lld\n", (long long)TCSANOW);
    printf("MAP_FIXED=%lld\n", (long long)MAP_FIXED);
    printf("getpagesize=%lld\n", (long long)getpagesize());
    printf("SIG_DFL=%lld\n", (long long)(intptr_t)SIG_DFL);
    printf("NSIG=%lld\n", (long long)NSIG);
    printf("sigset_t.size=%zu\n", sizeof(sigset_t));
    printf("SIG_SETMASK=%lld\n", (long long)SIG_SETMASK);
    printf("SIG_BLOCK=%lld\n", (long long)SIG_BLOCK);
    printf("SIGUSR2=%lld\n", (long long)SIGUSR2);
    printf("SIG_IGN=%lld\n", (long long)(intptr_t)SIG_IGN);
    printf("SIGHUP=%lld\n", (long long)SIGHUP);
    printf("SIGINT=%lld\n", (long long)SIGINT);
    printf("SIGQUIT=%lld\n", (long long)SIGQUIT);
    {
        // Zeroed first: under glibc, sigemptyset left bytes past the
        // signals it numbers as the stack had them (byte 125 read 0xff).
        sigset_t s;
        memset(&s, 0, sizeof s);
        sigemptyset(&s);
        sigaddset(&s, SIGUSR2);
        for (size_t i = 0; i < sizeof s; i++) {
            unsigned char b = ((unsigned char *)&s)[i];
            if (b) printf("sigset_usr2.byte=%zu\nsigset_usr2.value=%u\n", i, b);
        }
    }
    printf("TIOCGPGRP=%llu\n", (unsigned long long)TIOCGPGRP);
    printf("pid_t.size=%zu\n", sizeof(pid_t));
    printf("WNOHANG=%lld\n", (long long)WNOHANG);
    printf("SIGKILL=%lld\n", (long long)SIGKILL);
    printf("ESRCH=%lld\n", (long long)ESRCH);
    printf("ECHILD=%lld\n", (long long)ECHILD);
    printf("ENXIO=%lld\n", (long long)ENXIO);
    printf("ENOSPC=%lld\n", (long long)ENOSPC);
    printf("EMFILE=%lld\n", (long long)EMFILE);
    printf("EINVAL=%lld\n", (long long)EINVAL);
    printf("EINTR=%lld\n", (long long)EINTR);
    printf("P_PID=%lld\n", (long long)P_PID);
    printf("WEXITED=%lld\n", (long long)WEXITED);
    printf("WNOWAIT=%lld\n", (long long)WNOWAIT);
    printf("SIGCHLD=%lld\n", (long long)SIGCHLD);
    printf("id_t.size=%zu\n", sizeof(id_t));
    printf("pid_t.signed=%d\n", (pid_t)-1 < 0);
    {
        const int stty[] = {VEOF, VEOL, VERASE, VINTR, VKILL, VQUIT, VSTART, VSTOP, VSUSP,
#ifdef VDISCARD
            VDISCARD,
#endif
#ifdef VDSUSP
            VDSUSP,
#endif
#ifdef VEOL2
            VEOL2,
#endif
            VLNEXT,
#ifdef VREPRINT
            VREPRINT,
#endif
#ifdef VSTATUS
            VSTATUS,
#endif
#ifdef VSWTC
            VSWTC,
#endif
#ifdef VWERASE
            VWERASE,
#endif
        };
        printf("stty.len=%zu\n", sizeof stty / sizeof stty[0]);
        for (size_t i = 0; i < sizeof stty / sizeof stty[0]; i++)
            printf("stty.%zu=%d\n", i, stty[i]);
    }
#ifdef CLOCK_UPTIME_RAW
    printf("CLOCK_UPTIME_RAW=%lld\n", (long long)CLOCK_UPTIME_RAW);
#endif
#ifdef __APPLE__
    printf("locale_t.size=%zu\n", sizeof(locale_t));
    printf("nl_item.size=%zu\n", sizeof(nl_item));
    printf("LC_ALL_MASK=%lld\n", (long long)LC_ALL_MASK);
    printf("CODESET=%lld\n", (long long)CODESET);
    printf("CFIndex.size=%zu\n", sizeof(CFIndex));
    printf("CFStringEncoding.size=%zu\n", sizeof(CFStringEncoding));
    printf("Boolean.size=%zu\n", sizeof(Boolean));
    printf("CFLocaleRef.size=%zu\n", sizeof(CFLocaleRef));
    printf("CFStringRef.size=%zu\n", sizeof(CFStringRef));
    printf("CFTypeRef.size=%zu\n", sizeof(CFTypeRef));
    printf("UniChar.size=%zu\n", sizeof(UniChar));
    printf("UniChar.signed=%d\n", (UniChar)-1 < 0);
    printf("UniCharCount.size=%zu\n", sizeof(UniCharCount));
    printf("UniCharCount.signed=%d\n", (UniCharCount)-1 < 0);
    printf("OptionBits.size=%zu\n", sizeof(OptionBits));
    printf("OSStatus.size=%zu\n", sizeof(OSStatus));
    printf("OSStatus.signed=%d\n", (OSStatus)-1 < 0);
    printf("kUCKeyActionDisplay=%lld\n", (long long)kUCKeyActionDisplay);
    printf("kUCKeyTranslateNoDeadKeysMask=%lld\n", (long long)kUCKeyTranslateNoDeadKeysMask);
    printf("CGGlyph.size=%zu\n", sizeof(CGGlyph));
    printf("CGFloat.size=%zu\n", sizeof(CGFloat));
    printf("CFRange.size=%zu\n", sizeof(CFRange));
    printf("CFRange.length=%zu\n", offsetof(CFRange, length));
    printf("kCFStringEncodingUTF8=%lld\n", (long long)kCFStringEncodingUTF8);
    printf("RTLD_LAZY=%lld\n", (long long)RTLD_LAZY);
    printf("RTLD_LOCAL=%lld\n", (long long)RTLD_LOCAL);
    printf("RTLD_NOLOAD=%lld\n", (long long)RTLD_NOLOAD);
    printf("PSHMNAMLEN=%lld\n", (long long)PSHMNAMLEN);
    printf("O_SEARCH=%lld\n", (long long)O_SEARCH);
    printf("MAP_ANON=%lld\n", (long long)MAP_ANON);
    printf("MINCORE_INCORE=%lld\n", (long long)MINCORE_INCORE);
    printf("MINCORE_PAGED_OUT=%lld\n", (long long)MINCORE_PAGED_OUT);
    printf("MACH_TASK_BASIC_INFO=%lld\n", (long long)MACH_TASK_BASIC_INFO);
    printf("MACH_TASK_BASIC_INFO_COUNT=%lld\n", (long long)MACH_TASK_BASIC_INFO_COUNT);
    printf("mach_task_basic_info.size=%zu\n", sizeof(struct mach_task_basic_info));
    printf("TASK_VM_INFO=%lld\n", (long long)TASK_VM_INFO);
    printf("TASK_VM_INFO_REV1_COUNT=%lld\n", (long long)TASK_VM_INFO_REV1_COUNT);
    printf("task_vm_info.resident_size=%zu\n", offsetof(struct task_vm_info, resident_size));
    printf("task_vm_info.phys_footprint=%zu\n", offsetof(struct task_vm_info, phys_footprint));
    printf("mach_task_basic_info.resident_size=%zu\n",
           offsetof(struct mach_task_basic_info, resident_size));
    printf("mach_task_basic_info.policy=%zu\n", offsetof(struct mach_task_basic_info, policy));
    printf("mach_port_t.size=%zu\n", sizeof(mach_port_t));
    printf("PROC_PIDVNODEPATHINFO=%lld\n", (long long)PROC_PIDVNODEPATHINFO);
    printf("proc_vnodepathinfo.size=%zu\n", sizeof(struct proc_vnodepathinfo));
    printf("proc_vnodepathinfo.cdir_path=%zu\n",
           offsetof(struct proc_vnodepathinfo, pvi_cdir.vip_path));
    printf("MAXPATHLEN=%lld\n", (long long)MAXPATHLEN);
    printf("PROC_PIDPATHINFO_MAXSIZE=%lld\n", (long long)PROC_PIDPATHINFO_MAXSIZE);
    printf("PROC_PIDTBSDINFO=%lld\n", (long long)PROC_PIDTBSDINFO);
    printf("proc_bsdinfo.size=%zu\n", sizeof(struct proc_bsdinfo));
    printf("proc_bsdinfo.pgid=%zu\n", offsetof(struct proc_bsdinfo, pbi_pgid));
    printf("proc_bsdinfo.tpgid=%zu\n", offsetof(struct proc_bsdinfo, e_tpgid));
    printf("proc_bsdinfo.start_sec=%zu\n", offsetof(struct proc_bsdinfo, pbi_start_tvsec));
    printf("proc_bsdinfo.start_usec=%zu\n", offsetof(struct proc_bsdinfo, pbi_start_tvusec));
    printf("PROC_PIDT_SHORTBSDINFO=%lld\n", (long long)PROC_PIDT_SHORTBSDINFO);
    printf("proc_bsdshortinfo.size=%zu\n", sizeof(struct proc_bsdshortinfo));
    printf("proc_bsdshortinfo.comm=%zu\n", offsetof(struct proc_bsdshortinfo, pbsi_comm));
    printf("MAXCOMLEN=%lld\n", (long long)MAXCOMLEN);
    printf("PROC_PIDLISTFDS=%lld\n", (long long)PROC_PIDLISTFDS);
    printf("proc_fdinfo.size=%zu\n", sizeof(struct proc_fdinfo));
    printf("proc_fdinfo.fd=%zu\n", offsetof(struct proc_fdinfo, proc_fd));
    printf("proc_fdinfo.type=%zu\n", offsetof(struct proc_fdinfo, proc_fdtype));
    printf("PROX_FDTYPE_SOCKET=%lld\n", (long long)PROX_FDTYPE_SOCKET);
    printf("PROC_PIDFDSOCKETINFO=%lld\n", (long long)PROC_PIDFDSOCKETINFO);
    printf("socket_fdinfo.size=%zu\n", sizeof(struct socket_fdinfo));
    printf("socket_fdinfo.kind=%zu\n", offsetof(struct socket_fdinfo, psi.soi_kind));
    printf("socket_fdinfo.tcp_state=%zu\n",
           offsetof(struct socket_fdinfo, psi.soi_proto.pri_tcp.tcpsi_state));
    printf("SOCKINFO_TCP=%lld\n", (long long)SOCKINFO_TCP);
    printf("TSI_S_ESTABLISHED=%lld\n", (long long)TSI_S_ESTABLISHED);
    printf("CTL_KERN=%lld\n", (long long)CTL_KERN);
    printf("KERN_PROCARGS2=%lld\n", (long long)KERN_PROCARGS2);
    printf("kevent.size=%zu\n", sizeof(struct kevent));
    printf("kevent.ident=%zu\n", offsetof(struct kevent, ident));
    printf("kevent.filter=%zu\n", offsetof(struct kevent, filter));
    printf("kevent.flags=%zu\n", offsetof(struct kevent, flags));
    printf("kevent.fflags=%zu\n", offsetof(struct kevent, fflags));
    printf("kevent.data=%zu\n", offsetof(struct kevent, data));
    printf("kevent.udata=%zu\n", offsetof(struct kevent, udata));
    printf("timespec.size=%zu\n", sizeof(struct timespec));
    printf("timespec.nsec=%zu\n", offsetof(struct timespec, tv_nsec));
    printf("EVFILT_PROC=%lld\n", (long long)EVFILT_PROC);
    printf("EV_ADD=%lld\n", (long long)EV_ADD);
    printf("EV_CLEAR=%lld\n", (long long)EV_CLEAR);
    printf("EVFILT_WRITE=%lld\n", (long long)EVFILT_WRITE);
    printf("EV_ONESHOT=%lld\n", (long long)EV_ONESHOT);
    printf("NOTE_EXIT=%lld\n", (long long)NOTE_EXIT);
    printf("NOTE_FORK=%lld\n", (long long)NOTE_FORK);
    printf("NOTE_EXEC=%lld\n", (long long)NOTE_EXEC);
    printf("EVFILT_USER=%lld\n", (long long)EVFILT_USER);
    printf("EVFILT_SIGNAL=%lld\n", (long long)EVFILT_SIGNAL);
    printf("NOTE_TRIGGER=%lld\n", (long long)NOTE_TRIGGER);
    printf("EVFILT_VNODE=%lld\n", (long long)EVFILT_VNODE);
    printf("NOTE_DELETE=%lld\n", (long long)NOTE_DELETE);
    printf("NOTE_WRITE=%lld\n", (long long)NOTE_WRITE);
    printf("NOTE_EXTEND=%lld\n", (long long)NOTE_EXTEND);
    printf("NOTE_ATTRIB=%lld\n", (long long)NOTE_ATTRIB);
    printf("NOTE_LINK=%lld\n", (long long)NOTE_LINK);
    printf("NOTE_RENAME=%lld\n", (long long)NOTE_RENAME);
    printf("NOTE_REVOKE=%lld\n", (long long)NOTE_REVOKE);
    printf("O_EVTONLY=%lld\n", (long long)O_EVTONLY);
    printf("statfs.size=%zu\n", sizeof(struct statfs));
    printf("statfs.owner=%zu\n", offsetof(struct statfs, f_owner));
    printf("statfs.flags=%zu\n", offsetof(struct statfs, f_flags));
    printf("statfs.fstypename=%zu\n", offsetof(struct statfs, f_fstypename));
    printf("statfs.mntonname=%zu\n", offsetof(struct statfs, f_mntonname));
    printf("statfs.flags_ext=%zu\n", offsetof(struct statfs, f_flags_ext));
    printf("MFSTYPENAMELEN=%lld\n", (long long)MFSTYPENAMELEN);
    printf("MNT_LOCAL=%lld\n", (long long)MNT_LOCAL);
    printf("MNT_NOWAIT=%lld\n", (long long)MNT_NOWAIT);
    printf("IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES=%lld\n", (long long)IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES);
    printf("IOPOL_SCOPE_THREAD=%lld\n", (long long)IOPOL_SCOPE_THREAD);
    printf("IOPOL_MATERIALIZE_DATALESS_FILES_OFF=%lld\n", (long long)IOPOL_MATERIALIZE_DATALESS_FILES_OFF);
#else
    printf("locale_t.size=%zu\n", sizeof(locale_t));
    printf("LC_ALL_MASK=%lld\n", (long long)LC_ALL_MASK);
    printf("NAME_MAX=%lld\n", (long long)NAME_MAX);
    printf("O_PATH=%lld\n", (long long)O_PATH);
    printf("MADV_DONTNEED=%lld\n", (long long)MADV_DONTNEED);
    printf("PR_SET_THP_DISABLE=%lld\n", (long long)PR_SET_THP_DISABLE);
    printf("IN_NONBLOCK=%lld\n", (long long)IN_NONBLOCK);
    printf("IN_CLOEXEC=%lld\n", (long long)IN_CLOEXEC);
    printf("IN_MODIFY=%lld\n", (long long)IN_MODIFY);
    printf("IN_ATTRIB=%lld\n", (long long)IN_ATTRIB);
    printf("IN_CLOSE_WRITE=%lld\n", (long long)IN_CLOSE_WRITE);
    printf("IN_MOVED_FROM=%lld\n", (long long)IN_MOVED_FROM);
    printf("IN_MOVED_TO=%lld\n", (long long)IN_MOVED_TO);
    printf("IN_CREATE=%lld\n", (long long)IN_CREATE);
    printf("IN_DELETE=%lld\n", (long long)IN_DELETE);
    printf("IN_DELETE_SELF=%lld\n", (long long)IN_DELETE_SELF);
    printf("IN_MOVE_SELF=%lld\n", (long long)IN_MOVE_SELF);
    printf("SYS_pidfd_open=%lld\n", (long long)SYS_pidfd_open);
    printf("SYS_close_range=%lld\n", (long long)SYS_close_range);
#endif
    return 0;
}
"#;

    /// The lines the probe prints on every platform, before its
    /// `#ifdef`s: the floor its output is held to.
    const PRINTED_EVERYWHERE: usize = 60;

    /// Compile and run the probe, or say why it could not.
    fn ask_the_headers() -> std::collections::HashMap<String, i128> {
        // **One directory per call, not per process.** Both probe tests call
        // this, and the test harness runs them as threads of one process, so a
        // name keyed on the pid alone gave them one directory: one test could
        // compile over the binary the other was running, or `remove_dir_all`
        // it mid-run. The kernel logged exactly that shape -- AMFI evaluating
        // one `probe` path from two threads at once, "Unrecoverable CT
        // signature issue". A counter makes the paths distinct.
        static CALL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("baud-abi-{}-{call}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let src = dir.join("probe.c");
        let bin = dir.join("probe");
        std::fs::write(&src, PROBE).expect("write the probe");
        compile_probe(&[], &src, &bin, "sys.rs's constants and layouts");
        let run = std::process::Command::new(&bin).output().expect("run it");
        assert!(run.status.success(), "the probe did not run");
        let text = String::from_utf8_lossy(&run.stdout).into_owned();
        let _ = std::fs::remove_dir_all(&dir);
        let map: std::collections::HashMap<String, i128> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| v.trim().parse().ok().map(|n| (k.to_string(), n)))
            .collect();
        // **The control.** A probe that printed nothing, or whose output
        // stopped parsing, would let every assertion below pass over an
        // empty map -- the shape this repo keeps recording as a gate that
        // cannot fire.
        assert!(
            map.len() >= PRINTED_EVERYWHERE,
            "the probe printed {} values, which is fewer than it has lines: {text}",
            map.len()
        );
        map
    }

    #[test]
    fn the_layouts_are_the_platform_headers_own() {
        let h = ask_the_headers();
        assert_eq!(h["winsize.size"], size_of::<Winsize>() as i128);
        assert_eq!(h["winsize.row"], offset_of!(Winsize, ws_row) as i128);
        assert_eq!(h["winsize.col"], offset_of!(Winsize, ws_col) as i128);
        assert_eq!(h["winsize.xpixel"], offset_of!(Winsize, ws_xpixel) as i128);
        assert_eq!(h["winsize.ypixel"], offset_of!(Winsize, ws_ypixel) as i128);
        assert_eq!(h["pollfd.size"], size_of::<PollFd>() as i128);
        assert_eq!(h["pollfd.fd"], offset_of!(PollFd, fd) as i128);
        assert_eq!(h["pollfd.events"], offset_of!(PollFd, events) as i128);
        assert_eq!(h["pollfd.revents"], offset_of!(PollFd, revents) as i128);
        assert_eq!(h["nfds.size"], size_of::<Nfds>() as i128);
        assert_eq!(h["rlimit.size"], size_of::<Rlimit>() as i128);
        assert_eq!(h["rlimit.cur"], offset_of!(Rlimit, rlim_cur) as i128);
        assert_eq!(h["rlimit.max"], offset_of!(Rlimit, rlim_max) as i128);
        assert_eq!(h["rlim.size"], size_of::<RlimT>() as i128);
        assert_eq!(h["request.size"], size_of::<c_ulong>() as i128);
        assert_eq!(h["termios.size"], size_of::<Termios>() as i128);
        assert_eq!(h["termios.align"], align_of::<Termios>() as i128);
        assert_eq!(h["termios.iflag"], offset_of!(Termios, c_iflag) as i128);
        assert_eq!(h["termios.cc"], offset_of!(Termios, c_cc) as i128);
        assert_eq!(h["tcflag.size"], size_of::<TcFlag>() as i128);
        assert_eq!(h["size_t.size"], size_of::<MapLen>() as i128);
        assert_eq!(h["size_t.size"], size_of::<NameLen>() as i128);
        assert_eq!(h["uid_t.size"], size_of::<Uid>() as i128);
        assert_eq!(h["uid_t.signed"], 0, "uid_t is unsigned");
        assert_eq!(h["time_t.size"], size_of::<TimeT>() as i128);
        assert_eq!(h["time_t.signed"], 1, "time_t is signed");
        assert_eq!(h["tm.size"], size_of::<Tm>() as i128);
        assert_eq!(h["tm.sec"], offset_of!(Tm, tm_sec) as i128);
        assert_eq!(h["tm.min"], offset_of!(Tm, tm_min) as i128);
        assert_eq!(h["tm.hour"], offset_of!(Tm, tm_hour) as i128);
        assert_eq!(h["tm.mday"], offset_of!(Tm, tm_mday) as i128);
        assert_eq!(h["tm.mon"], offset_of!(Tm, tm_mon) as i128);
        assert_eq!(h["tm.year"], offset_of!(Tm, tm_year) as i128);
        assert_eq!(h["tm.wday"], offset_of!(Tm, tm_wday) as i128);
        assert_eq!(h["tm.yday"], offset_of!(Tm, tm_yday) as i128);
        assert_eq!(h["tm.isdst"], offset_of!(Tm, tm_isdst) as i128);
        assert_eq!(h["tm.gmtoff"], offset_of!(Tm, tm_gmtoff) as i128);
        assert_eq!(h["tm.zone"], offset_of!(Tm, tm_zone) as i128);
        assert_eq!(h["utsname.size"], size_of::<Utsname>() as i128);
        assert_eq!(h["utsname.sysname"], offset_of!(Utsname, sysname) as i128);
        assert_eq!(h["utsname.field"], UTS_FIELD as i128);
        assert_eq!(h["pid_t.size"], size_of::<Pid>() as i128);
        assert_eq!(h["pid_t.signed"], 1, "pid_t is signed");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(h["locale_t.size"], size_of::<Locale>() as i128);
            assert_eq!(h["nl_item.size"], size_of::<NlItem>() as i128);
            assert_eq!(h["CFIndex.size"], size_of::<CfIndex>() as i128);
            assert_eq!(
                h["CFStringEncoding.size"],
                size_of::<CfStringEncoding>() as i128
            );
            assert_eq!(h["Boolean.size"], size_of::<CfBoolean>() as i128);
            assert_eq!(h["CFLocaleRef.size"], size_of::<CfLocale>() as i128);
            assert_eq!(h["CFStringRef.size"], size_of::<CfString>() as i128);
            assert_eq!(h["CFTypeRef.size"], size_of::<CfObject>() as i128);
            assert_eq!(h["UniChar.size"], size_of::<UniChar>() as i128);
            assert_eq!(h["UniChar.signed"], 0, "UniChar is unsigned");
            assert_eq!(h["UniCharCount.size"], size_of::<UniCharCount>() as i128);
            assert_eq!(h["UniCharCount.signed"], 0, "UniCharCount is unsigned");
            assert_eq!(h["OptionBits.size"], size_of::<OptionBits>() as i128);
            assert_eq!(h["OSStatus.size"], size_of::<OsStatus>() as i128);
            assert_eq!(h["OSStatus.signed"], 1, "OSStatus is signed");
            assert_eq!(h["kUCKeyActionDisplay"], i128::from(UC_KEY_ACTION_DISPLAY));
            assert_eq!(
                h["kUCKeyTranslateNoDeadKeysMask"],
                i128::from(UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK)
            );
            assert_eq!(h["CGGlyph.size"], size_of::<CgGlyph>() as i128);
            assert_eq!(h["CGFloat.size"], size_of::<CgFloat>() as i128);
            assert_eq!(h["CFRange.size"], size_of::<CfRange>() as i128);
            assert_eq!(h["CFRange.length"], offset_of!(CfRange, length) as i128);
            assert_eq!(
                h["mach_task_basic_info.size"],
                size_of::<MachTaskBasicInfo>() as i128
            );
            assert_eq!(h["TASK_VM_INFO"], i128::from(TASK_VM_INFO));
            assert_eq!(
                h["TASK_VM_INFO_REV1_COUNT"],
                (size_of::<TaskVmInfo>() / size_of::<c_int>()) as i128
            );
            assert_eq!(
                h["task_vm_info.resident_size"],
                offset_of!(TaskVmInfo, resident_size) as i128
            );
            assert_eq!(
                h["task_vm_info.phys_footprint"],
                offset_of!(TaskVmInfo, phys_footprint) as i128
            );
            assert_eq!(
                h["mach_task_basic_info.resident_size"],
                offset_of!(MachTaskBasicInfo, resident_size) as i128
            );
            assert_eq!(
                h["mach_task_basic_info.policy"],
                offset_of!(MachTaskBasicInfo, policy) as i128
            );
            assert_eq!(h["mach_port_t.size"], size_of::<MachPort>() as i128);
            assert_eq!(h["proc_bsdinfo.size"], BSDINFO_SIZE as i128);
            assert_eq!(h["proc_bsdinfo.pgid"], BSDINFO_PGID as i128);
            assert_eq!(h["proc_bsdinfo.tpgid"], BSDINFO_TPGID as i128);
            assert_eq!(h["proc_bsdinfo.start_sec"], BSDINFO_START_SEC as i128);
            assert_eq!(h["proc_bsdinfo.start_usec"], BSDINFO_START_USEC as i128);
            assert_eq!(h["proc_bsdshortinfo.size"], BSDSHORTINFO_SIZE as i128);
            assert_eq!(h["proc_bsdshortinfo.comm"], BSDSHORTINFO_COMM as i128);
            assert_eq!(h["proc_fdinfo.size"], FDINFO_SIZE as i128);
            assert_eq!(h["proc_fdinfo.fd"], FDINFO_FD as i128);
            assert_eq!(h["proc_fdinfo.type"], FDINFO_TYPE as i128);
            assert_eq!(h["socket_fdinfo.size"], SOCKET_FDINFO_SIZE as i128);
            assert_eq!(h["socket_fdinfo.kind"], SOCKET_FDINFO_KIND as i128);
            assert_eq!(
                h["socket_fdinfo.tcp_state"],
                SOCKET_FDINFO_TCP_STATE as i128
            );
            assert_eq!(h["proc_vnodepathinfo.size"], VNODEPATHINFO_SIZE as i128);
            assert_eq!(
                h["proc_vnodepathinfo.cdir_path"],
                VNODEPATHINFO_CDIR_PATH as i128
            );
            assert_eq!(h["kevent.size"], size_of::<Kevent>() as i128);
            assert_eq!(h["kevent.ident"], offset_of!(Kevent, ident) as i128);
            assert_eq!(h["kevent.filter"], offset_of!(Kevent, filter) as i128);
            assert_eq!(h["kevent.flags"], offset_of!(Kevent, flags) as i128);
            assert_eq!(h["kevent.fflags"], offset_of!(Kevent, fflags) as i128);
            assert_eq!(h["kevent.data"], offset_of!(Kevent, data) as i128);
            assert_eq!(h["kevent.udata"], offset_of!(Kevent, udata) as i128);
            assert_eq!(h["timespec.size"], size_of::<Timespec>() as i128);
            assert_eq!(h["timespec.nsec"], offset_of!(Timespec, tv_nsec) as i128);
        }
    }

    #[test]
    fn the_constants_are_the_platform_headers_own() {
        let h = ask_the_headers();
        assert_eq!(h["TIOCSWINSZ"], i128::from(TIOCSWINSZ));
        assert_eq!(h["TIOCSCTTY"], i128::from(TIOCSCTTY));
        assert_eq!(h["TIOCGWINSZ"], i128::from(TIOCGWINSZ));
        assert_eq!(h["POLLIN"], i128::from(POLLIN));
        assert_eq!(h["POLLOUT"], i128::from(POLLOUT));
        assert_eq!(h["POLLNVAL"], i128::from(POLLNVAL));
        assert_eq!(h["F_GETFL"], i128::from(F_GETFL));
        assert_eq!(h["F_SETFL"], i128::from(F_SETFL));
        assert_eq!(h["F_GETFD"], i128::from(F_GETFD));
        assert_eq!(h["F_SETFD"], i128::from(F_SETFD));
        assert_eq!(h["FD_CLOEXEC"], i128::from(FD_CLOEXEC));
        assert_eq!(h["RLIMIT_NOFILE"], i128::from(RLIMIT_NOFILE));
        assert_eq!(h["O_NONBLOCK"], i128::from(O_NONBLOCK));
        assert_eq!(h["O_NOCTTY"], i128::from(O_NOCTTY));
        assert_eq!(h["O_RDWR"], i128::from(O_RDWR));
        assert_eq!(h["O_RDONLY"], i128::from(O_RDONLY));
        assert_eq!(h["O_CREAT"], i128::from(O_CREAT));
        assert_eq!(h["PROT_WRITE"], i128::from(PROT_WRITE));
        assert_eq!(h["MAP_SHARED"], i128::from(MAP_SHARED));
        assert_eq!(h["O_NOFOLLOW"], i128::from(O_NOFOLLOW));
        assert_eq!(h["O_DIRECTORY"], i128::from(O_DIRECTORY));
        assert_eq!(h["O_CLOEXEC"], i128::from(O_CLOEXEC));
        assert_eq!(h["AT_FDCWD"], i128::from(AT_FDCWD));
        assert_eq!(h["PROT_READ"], i128::from(PROT_READ));
        assert_eq!(h["MAP_PRIVATE"], i128::from(MAP_PRIVATE));
        assert_eq!(h["MAP_FAILED"], MAP_FAILED as isize as i128);
        assert_eq!(h["IUTF8"], i128::from(IUTF8));
        assert_eq!(h["NCCS"], NCCS as i128);
        assert_eq!(h["VLNEXT"], VLNEXT as i128);
        assert_eq!(h["stty.len"], STTY_CHARS.len() as i128, "STTY's entries");
        for (i, v) in STTY_CHARS.iter().enumerate() {
            assert_eq!(h[&format!("stty.{i}")], *v as i128, "STTY's entry {i}");
        }
        assert_eq!(h["_POSIX_VDISABLE"], i128::from(POSIX_VDISABLE));
        assert_eq!(h["TCSANOW"], i128::from(TCSANOW));
        assert_eq!(h["MAP_FIXED"], i128::from(MAP_FIXED));
        // **The page size is the system's**: what `getpagesize` answers here
        // is what the headers' program answers.
        assert_eq!(h["getpagesize"], i128::from(unsafe { getpagesize() }));
        assert_eq!(h["SIG_DFL"], SIG_DFL.0 as i128);
        assert_eq!(h["NSIG"], i128::from(NSIG));
        assert_eq!(h["sigset_t.size"], size_of::<SigSet>() as i128);
        assert_eq!(h["SIG_SETMASK"], i128::from(SIG_SETMASK));
        assert_eq!(h["SIG_BLOCK"], i128::from(SIG_BLOCK));
        assert_eq!(h["SIGUSR2"], i128::from(SIGUSR2));
        assert_eq!(h["SIG_IGN"], SIG_IGN.0 as i128);
        assert_eq!(h["SIGHUP"], i128::from(SIGHUP));
        assert_eq!(h["SIGINT"], i128::from(SIGINT));
        assert_eq!(h["SIGQUIT"], i128::from(SIGQUIT));
        // **A set holding one signal is `sigaddset`'s, byte for byte**: one
        // byte set, at the same place, to the same bit.
        let ours = SigSet::default().with(SIGUSR2);
        // SAFETY: a `repr(C)` value read as its own bytes.
        let bytes = unsafe {
            core::slice::from_raw_parts((&raw const ours).cast::<u8>(), size_of::<SigSet>())
        };
        let set: Vec<(usize, u8)> = bytes
            .iter()
            .enumerate()
            .filter(|(_, b)| **b != 0)
            .map(|(i, b)| (i, *b))
            .collect();
        assert_eq!(
            set,
            [(
                usize::try_from(h["sigset_usr2.byte"]).expect("a byte"),
                u8::try_from(h["sigset_usr2.value"]).expect("a byte's value"),
            )]
        );
        assert_eq!(h["TIOCGPGRP"], i128::from(TIOCGPGRP));
        assert_eq!(h["WNOHANG"], i128::from(WNOHANG));
        assert_eq!(h["SIGKILL"], i128::from(SIGKILL));
        assert_eq!(h["ESRCH"], i128::from(ESRCH));
        assert_eq!(h["ECHILD"], i128::from(ECHILD));
        assert_eq!(h["ENXIO"], i128::from(ENXIO));
        assert_eq!(h["ENOSPC"], i128::from(ENOSPC));
        assert_eq!(h["EMFILE"], i128::from(EMFILE));
        assert_eq!(h["EINVAL"], i128::from(EINVAL));
        assert_eq!(h["EINTR"], i128::from(EINTR));
        assert_eq!(h["P_PID"], i128::from(P_PID));
        assert_eq!(h["WEXITED"], i128::from(WEXITED));
        assert_eq!(h["WNOWAIT"], i128::from(WNOWAIT));
        assert_eq!(h["id_t.size"], size_of::<core::ffi::c_uint>() as i128);
        #[cfg(target_os = "macos")]
        assert_eq!(h["SIGCHLD"], i128::from(SIGCHLD));
        #[cfg(target_os = "linux")]
        assert_eq!(h["SYS_pidfd_open"], i128::from(SYS_PIDFD_OPEN));
        #[cfg(target_os = "linux")]
        assert_eq!(h["SYS_close_range"], i128::from(SYS_CLOSE_RANGE));
        #[cfg(target_os = "macos")]
        {
            assert_eq!(h["MAP_ANON"], i128::from(MAP_ANON));
            assert_eq!(h["MINCORE_INCORE"], i128::from(MINCORE_INCORE));
            assert_eq!(h["MINCORE_PAGED_OUT"], i128::from(MINCORE_PAGED_OUT));
            assert_eq!(h["MACH_TASK_BASIC_INFO"], i128::from(MACH_TASK_BASIC_INFO));
            assert_eq!(
                h["MACH_TASK_BASIC_INFO_COUNT"],
                (size_of::<MachTaskBasicInfo>() / size_of::<c_int>()) as i128
            );
        }
        #[cfg(not(target_os = "macos"))]
        assert_eq!(h["MADV_DONTNEED"], i128::from(MADV_DONTNEED));
        #[cfg(target_os = "macos")]
        {
            assert_eq!(h["CLOCK_UPTIME_RAW"], i128::from(CLOCK_UPTIME_RAW));
            assert_eq!(h["LC_ALL_MASK"], i128::from(LC_ALL_MASK));
            assert_eq!(h["CODESET"], i128::from(CODESET));
            assert_eq!(
                h["kCFStringEncodingUTF8"],
                i128::from(CF_STRING_ENCODING_UTF8)
            );
            assert_eq!(h["RTLD_LAZY"], i128::from(RTLD_LAZY));
            assert_eq!(h["RTLD_LOCAL"], i128::from(RTLD_LOCAL));
            assert_eq!(h["RTLD_NOLOAD"], i128::from(RTLD_NOLOAD));
            assert_eq!(h["PSHMNAMLEN"], SHM_NAME_MAX as i128);
            assert_eq!(h["O_SEARCH"], i128::from(O_SEARCH));
            assert_eq!(
                h["PROC_PIDVNODEPATHINFO"],
                i128::from(PROC_PIDVNODEPATHINFO)
            );
            assert_eq!(h["MAXPATHLEN"], MAXPATHLEN as i128);
            assert_eq!(
                h["PROC_PIDPATHINFO_MAXSIZE"],
                PROC_PIDPATHINFO_MAXSIZE as i128
            );
            assert_eq!(h["PROC_PIDTBSDINFO"], i128::from(PROC_PIDTBSDINFO));
            assert_eq!(
                h["PROC_PIDT_SHORTBSDINFO"],
                i128::from(PROC_PIDT_SHORTBSDINFO)
            );
            assert_eq!(h["MAXCOMLEN"], MAXCOMLEN as i128);
            assert_eq!(h["PROC_PIDLISTFDS"], i128::from(PROC_PIDLISTFDS));
            assert_eq!(h["PROX_FDTYPE_SOCKET"], i128::from(PROX_FDTYPE_SOCKET));
            assert_eq!(h["PROC_PIDFDSOCKETINFO"], i128::from(PROC_PIDFDSOCKETINFO));
            assert_eq!(h["SOCKINFO_TCP"], i128::from(SOCKINFO_TCP));
            assert_eq!(h["TSI_S_ESTABLISHED"], i128::from(TSI_S_ESTABLISHED));
            assert_eq!(h["CTL_KERN"], i128::from(CTL_KERN));
            assert_eq!(h["KERN_PROCARGS2"], i128::from(KERN_PROCARGS2));
            assert_eq!(h["EVFILT_PROC"], i128::from(EVFILT_PROC));
            assert_eq!(h["EV_ADD"], i128::from(EV_ADD));
            assert_eq!(h["EV_CLEAR"], i128::from(EV_CLEAR));
            assert_eq!(h["EVFILT_WRITE"], i128::from(EVFILT_WRITE));
            assert_eq!(h["EV_ONESHOT"], i128::from(EV_ONESHOT));
            assert_eq!(h["NOTE_EXIT"], i128::from(NOTE_EXIT));
            assert_eq!(h["NOTE_FORK"], i128::from(NOTE_FORK));
            assert_eq!(h["NOTE_EXEC"], i128::from(NOTE_EXEC));
            assert_eq!(h["EVFILT_USER"], i128::from(EVFILT_USER));
            assert_eq!(h["EVFILT_SIGNAL"], i128::from(EVFILT_SIGNAL));
            assert_eq!(h["NOTE_TRIGGER"], i128::from(NOTE_TRIGGER));
            assert_eq!(h["EVFILT_VNODE"], i128::from(EVFILT_VNODE));
            assert_eq!(h["NOTE_DELETE"], i128::from(NOTE_DELETE));
            assert_eq!(h["NOTE_WRITE"], i128::from(NOTE_WRITE));
            assert_eq!(h["NOTE_EXTEND"], i128::from(NOTE_EXTEND));
            assert_eq!(h["NOTE_ATTRIB"], i128::from(NOTE_ATTRIB));
            assert_eq!(h["NOTE_LINK"], i128::from(NOTE_LINK));
            assert_eq!(h["NOTE_RENAME"], i128::from(NOTE_RENAME));
            assert_eq!(h["NOTE_REVOKE"], i128::from(NOTE_REVOKE));
            assert_eq!(h["O_EVTONLY"], i128::from(O_EVTONLY));
            // The printed paths' mount check (`probe.rs`): a wrong offset
            // reads another field as the flags or the mount point.
            assert_eq!(h["statfs.size"], size_of::<Statfs>() as i128);
            assert_eq!(h["statfs.owner"], offset_of!(Statfs, f_owner) as i128);
            assert_eq!(h["statfs.flags"], offset_of!(Statfs, f_flags) as i128);
            assert_eq!(
                h["statfs.fstypename"],
                offset_of!(Statfs, f_fstypename) as i128
            );
            assert_eq!(
                h["statfs.mntonname"],
                offset_of!(Statfs, f_mntonname) as i128
            );
            assert_eq!(
                h["statfs.flags_ext"],
                offset_of!(Statfs, f_flags_ext) as i128
            );
            assert_eq!(h["MFSTYPENAMELEN"], MFSTYPENAMELEN as i128);
            assert_eq!(h["MNT_LOCAL"], i128::from(MNT_LOCAL));
            assert_eq!(h["MNT_NOWAIT"], i128::from(MNT_NOWAIT));
            assert_eq!(
                h["IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES"],
                i128::from(IOPOL_TYPE_VFS_MATERIALIZE_DATALESS_FILES)
            );
            assert_eq!(h["IOPOL_SCOPE_THREAD"], i128::from(IOPOL_SCOPE_THREAD));
            assert_eq!(
                h["IOPOL_MATERIALIZE_DATALESS_FILES_OFF"],
                i128::from(IOPOL_MATERIALIZE_DATALESS_FILES_OFF)
            );
        }
        #[cfg(not(target_os = "macos"))]
        assert_eq!(h["NAME_MAX"], SHM_NAME_MAX as i128);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(h["LC_ALL_MASK"], i128::from(LC_ALL_MASK));
        #[cfg(not(target_os = "macos"))]
        assert_eq!(h["locale_t.size"], size_of::<Locale>() as i128);
        #[cfg(not(target_os = "macos"))]
        assert_eq!(h["O_PATH"], i128::from(O_PATH));
        #[cfg(target_os = "linux")]
        {
            assert_eq!(h["IN_NONBLOCK"], i128::from(O_NONBLOCK));
            assert_eq!(h["IN_CLOEXEC"], i128::from(O_CLOEXEC));
            assert_eq!(h["IN_MODIFY"], i128::from(IN_MODIFY));
            assert_eq!(h["IN_ATTRIB"], i128::from(IN_ATTRIB));
            assert_eq!(h["IN_CLOSE_WRITE"], i128::from(IN_CLOSE_WRITE));
            assert_eq!(h["IN_MOVED_FROM"], i128::from(IN_MOVED_FROM));
            assert_eq!(h["IN_MOVED_TO"], i128::from(IN_MOVED_TO));
            assert_eq!(h["IN_CREATE"], i128::from(IN_CREATE));
            assert_eq!(h["IN_DELETE"], i128::from(IN_DELETE));
            assert_eq!(h["IN_DELETE_SELF"], i128::from(IN_DELETE_SELF));
            assert_eq!(h["IN_MOVE_SELF"], i128::from(IN_MOVE_SELF));
            assert_eq!(h["PR_SET_THP_DISABLE"], i128::from(PR_SET_THP_DISABLE));
        }
    }

    /// **A program baud starts is made only by [`child`]**: no code of the
    /// app outside this file calls `Command::new`, past its tests and the
    /// modules the app as built leaves out, so no program it starts
    /// inherits a blocked mask or an ignored signal. Each module named here
    /// is declared under `cfg(test)`, `cfg(all(test, ..))` or the probe
    /// build's feature, or inside a module that is, and each still calls
    /// `Command::new`, so none outlives its reason. Red with `Command::new`
    /// in `rules::command_of`, `opener::command` or `windows::new_baud`.
    #[test]
    fn a_program_baud_starts_is_made_only_here() {
        use crate::no_appkit::{bare_code, without_tests};
        // A module the app as built leaves out, by its file, and the file
        // and line that declare it.
        const LEFT_OUT: [(&str, &str, &str); 5] = [
            ("app/launch_tests.rs", "app.rs", "mod launch_tests;"),
            ("app/rig.rs", "app.rs", "pub(crate) mod rig;"),
            ("film.rs", "main.rs", "mod film;"),
            ("film/session.rs", "film.rs", "pub(crate) mod session;"),
            ("programs.rs", "main.rs", "mod programs;"),
        ];
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        let mut dirs = vec![src.clone()];
        while let Some(d) = dirs.pop() {
            for e in std::fs::read_dir(&d).expect("the source") {
                let p = e.expect("an entry").path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    files.push(p);
                }
            }
        }
        files.sort();
        let calls = |name: &str| {
            let text = std::fs::read_to_string(src.join(name)).expect("a source file");
            without_tests(&bare_code(&text)).contains("Command::new(")
        };
        let mut found = Vec::new();
        for f in &files {
            let name = f.strip_prefix(&src).expect("under src");
            let name = name.to_string_lossy().into_owned();
            let left_out = LEFT_OUT.iter().any(|(m, _, _)| *m == name);
            if name != "sys.rs" && !left_out && calls(&name) {
                found.push(name);
            }
        }
        assert_eq!(
            found,
            Vec::<String>::new(),
            "Command::new outside sys::child"
        );
        for (m, by, decl) in LEFT_OUT {
            assert!(
                calls(m),
                "{m} no longer calls Command::new: take it off the list"
            );
            let text = std::fs::read_to_string(src.join(by)).expect("the declaring file");
            let lines: Vec<&str> = text.lines().collect();
            let at = lines.iter().position(|l| l.trim() == decl);
            let at = at.unwrap_or_else(|| panic!("{m}: {by} has no {decl:?}"));
            let marked = lines[..at]
                .iter()
                .rev()
                .map(|l| l.trim())
                .take_while(|l| l.starts_with("#[") || l.starts_with("//"))
                .any(|l| {
                    l == "#[cfg(test)]"
                        || l.starts_with("#[cfg(all(test,")
                        || l == "#[cfg(feature = \"effect-probe\")]"
                });
            let inside = LEFT_OUT.iter().any(|(o, _, _)| *o == by);
            assert!(
                marked || inside,
                "{m}: {by}'s {decl:?} is in the app as built"
            );
        }
    }
}
