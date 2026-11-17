//! **`scripts/bounded`'s watcher on macOS.** macOS has no external limit to
//! set: `RLIMIT_AS`, `RLIMIT_DATA` and `RLIMIT_RSS` are refused below 512 GiB,
//! so this polls the footprint of a program's tree from outside and kills it.
//! `ri_phys_footprint` is the number and resident size is not: compressed pages
//! leave resident size, a Vec of repeated values compresses, and footprint is
//! what the kernel's own memory killer counts.
//!
//! The script builds this with plain `rustc`: no crate, `extern "C"`
//! declarations as `baud-app/src/sys.rs` has them, and the layouts held to the
//! headers by a C probe in `xtask/tests/bounded.rs`.
//!
//! **No Rust runtime, `#![no_main]`.** std's start-up sets `SIGPIPE` to ignored,
//! which `exec` inherits, and reopens a closed standard descriptor; a program
//! under this must find what it would have found with no runner.
//!
//! Three processes:
//! * **the program**, forked in this process group, so a group's signals still
//!   reach it, and held at a gate until the other two are ready;
//! * **the watcher**, this one, which waits on kqueue so that a program that
//!   ends returns at once, and polls the tree every [`POLL`];
//! * **the sentinel**, in its own session with its standard descriptors closed
//!   so that nothing waits on it, holding the read end of a pipe. Each poll the
//!   watcher tells it the descendants it has newly seen. End of file with no
//!   last word means the watcher was killed, and the sentinel kills all it was
//!   told of and their children. The watcher watches the sentinel the same way.
//!   So `kill -9` of either leaves no test running.
//!
//! A program that ends and leaves descendants running is passed to the sentinel,
//! which goes on bounding them until they end: they keep running, as they would
//! with no runner, and they do not escape.
#![no_main]

use core::ffi::{CStr, c_char, c_int, c_void};
use std::time::{Duration, Instant};

/// Over the bound, and killed; `scripts/bounded` names the same three.
const EXIT_OVER: c_int = 113;
/// The guard could not guard: not run, or killed.
const EXIT_REFUSED: c_int = 114;
/// `--timeout` expired, and killed.
const EXIT_TIMED_OUT: c_int = 115;

/// **How often the tree is read.** 4 Hz: at 0.8 GiB/s, 0.2 GiB past the bound.
/// (`MEASUREMENTS.md`, "One binary cannot take the machine down", has its cost.)
const POLL: Duration = Duration::from_millis(250);

/// **Polls in a row the live program may go unread**: two seconds. One failed
/// read could be the program exiting; this many is a program this cannot see, and
/// what cannot be seen is not bounded. A descendant is not held to it: `sudo` or
/// `top` under a shell under `cargo run` is setuid root, unreadable and unkillable
/// from here, and legitimate, so it counts as nothing and its children are walked.
const BLIND_POLLS: u32 = 8;

/// **How long a signal waits before it is relayed to the program.** One sent to
/// the group has reached the program already, and this cannot tell it from one
/// sent to this process alone, so the program is given time to act on it first.
const RELAY_AFTER: Duration = Duration::from_secs(2);

/// How long a killed program is waited for before this leaves anyway.
const REAP_FOR: Duration = Duration::from_secs(5);

/// **How long a process asked to end with `SIGTERM` is given before
/// `SIGKILL`.** A test binary's default action on `SIGTERM` ends it at once,
/// so the grace is waited out only by what handles the signal: cargo-nextest,
/// which passes it to its tests and waits for them, and a program finishing
/// its work. It is [`RELAY_AFTER`], the time this already gives a program to
/// act on a signal, and it ends as soon as nothing is left. Past the bound, a
/// tree still growing is killed at once: at 0.8 GiB/s two seconds would be
/// 1.6 GiB more. Shorter than `REAP_FOR`, so a program the grace ends is
/// reaped, not abandoned.
const GRACE: Duration = RELAY_AFTER;

/// How often the tree is walked during the [`GRACE`], for what it starts.
const TICK: Duration = Duration::from_millis(10);

type Pid = c_int;

const RUSAGE_INFO_V0: c_int = 0;
const PROC_PIDLISTFDS: c_int = 1;

const EVFILT_PROC: i16 = -5;
const EVFILT_SIGNAL: i16 = -6;
const EV_ADD: u16 = 0x0001;
const NOTE_EXIT: u32 = 0x8000_0000;

const F_SETFL: c_int = 4;
const F_DUPFD_CLOEXEC: c_int = 67;
const O_NONBLOCK: c_int = 0x0004;
const O_WRONLY: c_int = 0x0001;
const O_APPEND: c_int = 0x0008;
const O_CREAT: c_int = 0x0200;
const O_CLOEXEC: c_int = 0x0100_0000;

const ENOENT: c_int = 2;
const ESRCH: c_int = 3;
const EINTR: c_int = 4;

const WNOHANG: c_int = 1;
const SIG_UNBLOCK: c_int = 2;
const RLIMIT_CORE: c_int = 4;

const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;

const SIGHUP: c_int = 1;
const SIGINT: c_int = 2;
const SIGQUIT: c_int = 3;
const SIGKILL: c_int = 9;
const SIGPIPE: c_int = 13;
const SIGALRM: c_int = 14;
const SIGTERM: c_int = 15;
const SIGCHLD: c_int = 20;
const SIGXCPU: c_int = 24;
const SIGXFSZ: c_int = 25;
const SIGVTALRM: c_int = 26;
const SIGPROF: c_int = 27;
const SIGUSR1: c_int = 30;
const SIGUSR2: c_int = 31;

/// **Signals whose default would end this and leave the program to the
/// sentinel**, which would kill a program still acting on the signal. This
/// ignores them and hears of them from kqueue, which reports a signal even if
/// it is ignored.
const HELD: [c_int; 11] = [
    SIGHUP, SIGINT, SIGQUIT, SIGALRM, SIGTERM, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGUSR1,
    SIGUSR2,
];

#[repr(C)]
#[derive(Default)]
struct RusageInfoV0 {
    uuid: [u8; 16],
    user_time: u64,
    system_time: u64,
    pkg_idle_wkups: u64,
    interrupt_wkups: u64,
    pageins: u64,
    wired_size: u64,
    resident_size: u64,
    phys_footprint: u64,
    proc_start_abstime: u64,
    proc_exit_abstime: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Kevent {
    ident: usize,
    filter: i16,
    flags: u16,
    fflags: u32,
    data: isize,
    udata: *mut c_void,
}

const NO_EVENT: Kevent = Kevent {
    ident: 0,
    filter: 0,
    flags: 0,
    fflags: 0,
    data: 0,
    udata: core::ptr::null_mut(),
};

#[repr(C)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// `struct rusage`: two `timeval`s, then `ru_maxrss` and thirteen more longs.
/// On Darwin `ru_maxrss` is in bytes.
#[repr(C)]
#[derive(Default)]
struct Rusage {
    times: [i64; 4],
    ru_maxrss: i64,
    rest: [i64; 13],
}

/// `struct proc_fdinfo`, one entry of `PROC_PIDLISTFDS`.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ProcFdinfo {
    proc_fd: i32,
    proc_fdtype: u32,
}

/// `struct rlimit`, for turning off this process's core file before it raises
/// the signal the program died of.
#[repr(C)]
struct Rlimit {
    cur: u64,
    max: u64,
}

unsafe extern "C" {
    fn __error() -> *mut c_int;
    fn _exit(code: c_int) -> !;
    fn close(fd: c_int) -> c_int;
    fn execvp(file: *const c_char, argv: *const *const c_char) -> c_int;
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn fork() -> Pid;
    fn getenv(name: *const c_char) -> *const c_char;
    fn getpid() -> Pid;
    fn kevent(
        kq: c_int,
        changes: *const Kevent,
        nchanges: c_int,
        events: *mut Kevent,
        nevents: c_int,
        timeout: *const Timespec,
    ) -> c_int;
    fn kill(pid: Pid, sig: c_int) -> c_int;
    fn kqueue() -> c_int;
    fn nanosleep(request: *const Timespec, remain: *mut Timespec) -> c_int;
    fn open(path: *const c_char, flags: c_int, ...) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn proc_listchildpids(pid: Pid, buffer: *mut c_void, size: c_int) -> c_int;
    fn proc_pid_rusage(pid: Pid, flavor: c_int, buffer: *mut RusageInfoV0) -> c_int;
    fn proc_pidinfo(pid: Pid, flavor: c_int, arg: u64, buffer: *mut c_void, size: c_int) -> c_int;
    fn raise(sig: c_int) -> c_int;
    fn read(fd: c_int, buffer: *mut c_void, count: usize) -> isize;
    fn setenv(name: *const c_char, value: *const c_char, overwrite: c_int) -> c_int;
    fn setrlimit(resource: c_int, limit: *const Rlimit) -> c_int;
    fn setsid() -> Pid;
    fn sigprocmask(how: c_int, set: *const u32, old: *mut u32) -> c_int;
    fn signal(sig: c_int, handler: usize) -> usize;
    fn strerror(code: c_int) -> *const c_char;
    fn sysctlbyname(
        name: *const c_char,
        out: *mut c_void,
        outlen: *mut usize,
        new: *const c_void,
        newlen: usize,
    ) -> c_int;
    fn unsetenv(name: *const c_char) -> c_int;
    fn wait4(pid: Pid, status: *mut c_int, options: c_int, usage: *mut Rusage) -> Pid;
    fn write(fd: c_int, buffer: *const c_void, count: usize) -> isize;
}

fn errno() -> c_int {
    // SAFETY: `__error` returns this thread's live errno.
    unsafe { *__error() }
}

fn say(text: &str) {
    let line = format!("bounded: {text}\n");
    // SAFETY: a live buffer of that length. A closed stderr is not a failure.
    unsafe { write(2, line.as_ptr().cast(), line.len()) };
}

/// A process's memory, or a tree's summed.
#[derive(Default, Clone, Copy)]
struct Sizes {
    footprint: u64,
    resident: u64,
}

impl Sizes {
    fn add(&mut self, other: Self) {
        self.footprint += other.footprint;
        self.resident += other.resident;
    }

    /// **The bound is on footprint, and resident size is not consulted:**
    /// clean file pages count in resident size and the compressor takes pages
    /// out of it, so it is over a bound the tree is not over and under one it
    /// is. Over is past the bound.
    fn over(self, bound: u64) -> bool {
        let Self {
            footprint,
            resident: _,
        } = self;
        footprint > bound
    }
}

/// What reading a process found.
enum Seen {
    Alive { sizes: Sizes, start: u64 },
    Gone,
    Unreadable,
}

fn look(pid: Pid) -> Seen {
    let mut info = RusageInfoV0::default();
    // SAFETY: `info` is the flavour's struct, live for the call.
    let rc = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V0, &raw mut info) };
    if rc != 0 {
        return match errno() {
            ESRCH => Seen::Gone,
            _ => Seen::Unreadable,
        };
    }
    // A zombie reads, with a zero footprint and its exit time set.
    if info.proc_exit_abstime != 0 {
        return Seen::Gone;
    }
    Seen::Alive {
        sizes: Sizes {
            footprint: info.phys_footprint,
            resident: info.resident_size,
        },
        start: info.proc_start_abstime,
    }
}

/// A process of the tree as it was first seen. A start time of zero was
/// unreadable then, and is not checked before a kill.
struct Member {
    pid: Pid,
    start: u64,
    blind: u32,
    told: bool,
    /// Sent `SIGTERM` by [`Tree::kill_all`].
    asked: bool,
}

/// A record to the sentinel: a pid and its start time, or a last word.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Record {
    pid: i64,
    start: u64,
}

/// The last word that says the sentinel's work is done.
const DONE: i64 = -1;
/// The last word that says the tree it was told of is now its to bound.
const HANDED_OVER: i64 = -2;

/// What one pass over the tree found.
struct Pass {
    sizes: Sizes,
    blind: bool,
}

#[derive(Default)]
struct Tree {
    /// The program, which [`BLIND_POLLS`] holds; none once it has ended.
    program: Pid,
    members: Vec<Member>,
}

impl Tree {
    fn remember(&mut self, pid: Pid, start: u64, told: bool) {
        self.members.push(Member {
            pid,
            start,
            blind: 0,
            told,
            asked: false,
        });
    }

    /// Every child of `pid` not yet remembered, as it stands now.
    fn adopt_children(&mut self, pid: Pid) {
        let mut buffer = [0 as Pid; 1024];
        // SAFETY: a live buffer of the size given.
        let count = unsafe {
            proc_listchildpids(
                pid,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as c_int,
            )
        };
        if count <= 0 {
            return;
        }
        for &child in &buffer[..(count as usize).min(buffer.len())] {
            if self.members.iter().any(|m| m.pid == child) {
                continue;
            }
            match look(child) {
                Seen::Alive { start, .. } => self.remember(child, start, false),
                Seen::Unreadable => self.remember(child, 0, false),
                Seen::Gone => {}
            }
        }
    }

    /// Sum the tree, drop what has ended, and remember what is new. A pid whose
    /// start time has changed is another process now, and is dropped unkilled.
    fn pass(&mut self) -> Pass {
        let mut pass = Pass {
            sizes: Sizes::default(),
            blind: false,
        };
        let mut at = 0;
        while at < self.members.len() {
            let pid = self.members[at].pid;
            match look(pid) {
                Seen::Gone => {
                    self.members.swap_remove(at);
                    continue;
                }
                Seen::Alive { sizes, start } => {
                    let known = self.members[at].start;
                    if known != 0 && known != start {
                        self.members.swap_remove(at);
                        continue;
                    }
                    self.members[at].start = start;
                    self.members[at].blind = 0;
                    pass.sizes.add(sizes);
                }
                Seen::Unreadable => {
                    self.members[at].blind += 1;
                    pass.blind |= pid == self.program && self.members[at].blind >= BLIND_POLLS;
                }
            }
            self.adopt_children(pid);
            at += 1;
        }
        pass
    }

    fn same_process(member: &Member) -> bool {
        match look(member.pid) {
            Seen::Alive { start, .. } => member.start == 0 || member.start == start,
            // Unreadable is another user's process if the pid was reused, and
            // the kill is refused; if not, it is the one to kill.
            Seen::Unreadable => true,
            Seen::Gone => false,
        }
    }

    /// **Ask, then kill, by pid and not by group:** a child that started its
    /// own session has left the group. Each process is sent `SIGTERM` and the
    /// tree is given [`GRACE`] to end, walked every [`TICK`] meanwhile so that
    /// what it starts is asked too; the grace ends early once nothing is left,
    /// or at once if the tree's footprint passes `ceiling`. What is left is
    /// sent `SIGKILL`, walking each one's children as it goes until a walk
    /// finds nothing new. **Nothing is stopped:** a Metal client stopped
    /// mid-submit holds the GPU work it was handing over until the kill. A pid
    /// whose start time has changed is left alone.
    fn kill_all(&mut self, ceiling: u64) {
        let until = Instant::now() + GRACE;
        loop {
            let (alive, footprint) = self.ask_all();
            if !alive || footprint > ceiling || Instant::now() >= until {
                break;
            }
            let wait = Timespec {
                tv_sec: 0,
                tv_nsec: TICK.subsec_nanos().into(),
            };
            // SAFETY: a live request and no remainder wanted.
            unsafe { nanosleep(&raw const wait, core::ptr::null_mut()) };
        }
        for _ in 0..64 {
            let before = self.members.len();
            let mut at = 0;
            while at < self.members.len() {
                if Self::same_process(&self.members[at]) {
                    let pid = self.members[at].pid;
                    // SAFETY: a signal to a process just checked to be this one.
                    unsafe { kill(pid, SIGKILL) };
                    self.adopt_children(pid);
                }
                at += 1;
            }
            if self.members.len() == before {
                break;
            }
        }
    }

    /// `SIGTERM` to each process not yet sent it, its children walked as it
    /// goes, so one pass reaches the whole tree as it stands. Whether any
    /// readable process is still running, and their footprint.
    fn ask_all(&mut self) -> (bool, u64) {
        let mut alive = false;
        let mut footprint = 0;
        let mut at = 0;
        while at < self.members.len() {
            let member = &self.members[at];
            let pid = member.pid;
            let running = match look(pid) {
                Seen::Alive { sizes, start } if member.start == 0 || member.start == start => {
                    alive = true;
                    footprint += sizes.footprint;
                    true
                }
                // Another user's process, or this one: the kill is refused if
                // the pid was reused, and is not waited on either way.
                Seen::Unreadable => true,
                _ => false,
            };
            if running {
                if !member.asked {
                    // SAFETY: a signal to a process just checked to be this one.
                    unsafe { kill(pid, SIGTERM) };
                    self.members[at].asked = true;
                }
                self.adopt_children(pid);
            }
            at += 1;
        }
        (alive, footprint)
    }

    /// Tell the sentinel of what it has not heard. A full pipe is tried again at
    /// the next poll.
    fn tell(&mut self, fd: c_int) {
        for member in self.members.iter_mut().filter(|m| !m.told) {
            let record = Record {
                pid: i64::from(member.pid),
                start: member.start,
            };
            if !send(fd, record) {
                return;
            }
            member.told = true;
        }
    }
}

fn send(fd: c_int, record: Record) -> bool {
    let size = size_of::<Record>();
    // SAFETY: a live record of that size. A write this size to a pipe is atomic.
    unsafe { write(fd, (&raw const record).cast(), size) == size as isize }
}

/// A pipe with both ends close-on-exec and above the standard descriptors, so
/// that if those were closed at the start, a pipe does not become one.
fn pipe_apart() -> Option<[c_int; 2]> {
    let mut fds = [0; 2];
    // SAFETY: a live array of two.
    if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
        return None;
    }
    for fd in &mut fds {
        // SAFETY: a descriptor just made.
        let moved = unsafe { fcntl(*fd, F_DUPFD_CLOEXEC, 3) };
        // SAFETY: as above, and it is not used again.
        unsafe { close(*fd) };
        if moved < 0 {
            return None;
        }
        *fd = moved;
    }
    Some(fds)
}

fn cstr_lossy(p: *const c_char) -> String {
    // SAFETY: from argv, or from libc, and null-terminated.
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

/// The sentinel's own descriptors: everything inherited is closed, jobserver and
/// output pipes among them, so that it holds nothing a harness waits on.
fn close_all_but(keep: &[c_int]) {
    let mut fds = [ProcFdinfo::default(); 4096];
    // SAFETY: a live buffer of the size given.
    let bytes = unsafe {
        proc_pidinfo(
            getpid(),
            PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            size_of_val(&fds) as c_int,
        )
    };
    for info in &fds[..(bytes.max(0) as usize / size_of::<ProcFdinfo>()).min(fds.len())] {
        if !keep.contains(&info.proc_fd) {
            // SAFETY: it is this process's and nothing here uses it.
            unsafe { close(info.proc_fd) };
        }
    }
}

fn sentinel(from_watcher: c_int, ready: c_int, bound: u64, log: Option<&Log>) -> ! {
    // SAFETY: plain calls in a process of one thread.
    unsafe {
        setsid();
        signal(SIGHUP, SIG_IGN);
        signal(SIGINT, SIG_IGN);
        signal(SIGTERM, SIG_IGN);
    }
    close_all_but(&[from_watcher, ready]);
    // SAFETY: a live byte, and our own descriptor.
    unsafe {
        write(ready, [1u8].as_ptr().cast(), 1);
        close(ready);
    }
    let mut tree = Tree::default();
    loop {
        let mut record = Record::default();
        let size = size_of::<Record>();
        // SAFETY: a live record of that size.
        let got = unsafe { read(from_watcher, (&raw mut record).cast(), size) };
        if got == -1 && errno() == EINTR {
            continue;
        }
        if got != size as isize {
            tree.kill_all(bound);
            // SAFETY: ending this process.
            unsafe { _exit(0) };
        }
        match record.pid {
            DONE => {
                // SAFETY: ending this process.
                unsafe { _exit(0) };
            }
            HANDED_OVER => break,
            pid => tree.remember(pid as Pid, record.start, true),
        }
    }
    let mut peak = 0;
    loop {
        let pass = tree.pass();
        peak = pass.sizes.footprint.max(peak);
        if tree.members.is_empty() {
            // SAFETY: ending this process.
            unsafe { _exit(0) };
        }
        if pass.sizes.over(bound) {
            tree.kill_all(pass.sizes.footprint);
            if let Some(log) = log {
                log.line("(what a finished program left running)", peak, 0);
            }
            // SAFETY: ending this process.
            unsafe { _exit(0) };
        }
        let wait = Timespec {
            tv_sec: 0,
            tv_nsec: POLL.subsec_nanos().into(),
        };
        // SAFETY: a live request and no remainder wanted.
        unsafe { nanosleep(&raw const wait, core::ptr::null_mut()) };
    }
}

/// `BAUD_TEST_MEMORY_LOG`: the file each run's peak is appended to.
struct Log {
    path: *const c_char,
    words: String,
}

impl Log {
    /// One `write` to a file opened to append, so that parallel runs' lines do
    /// not interleave. A failure to write is not the test's failure.
    fn line(&self, program: &str, peak: u64, maxrss: i64) {
        let line = format!(
            "{program}\tpeak_footprint={peak}\tmaxrss={maxrss}\t{}\n",
            self.words
        );
        // SAFETY: a path from the environment, and a mode for the new file.
        let fd = unsafe {
            open(
                self.path,
                O_WRONLY | O_APPEND | O_CREAT | O_CLOEXEC,
                0o644 as c_int,
            )
        };
        if fd >= 0 {
            // SAFETY: a live buffer of that length, and our own descriptor.
            unsafe {
                write(fd, line.as_ptr().cast(), line.len());
                close(fd);
            }
        }
    }
}

/// The exit a status asks for: its code, or the same signal raised here, so that
/// whoever reads this process's status reads what the program's would have said.
fn leave_as(status: c_int) -> ! {
    let signal_number = status & 0x7f;
    if signal_number == 0 {
        // SAFETY: ending this process.
        unsafe { _exit((status >> 8) & 0xff) };
    }
    raise_exactly(signal_number)
}

fn raise_exactly(signal_number: c_int) -> ! {
    let unblock: u32 = 1 << (signal_number - 1);
    let no_core = Rlimit { cur: 0, max: 0 };
    // SAFETY: plain calls with live arguments; this process is ending.
    unsafe {
        setrlimit(RLIMIT_CORE, &raw const no_core);
        signal(signal_number, SIG_DFL);
        sigprocmask(SIG_UNBLOCK, &raw const unblock, core::ptr::null_mut());
        raise(signal_number);
        _exit(128 + signal_number)
    }
}

/// Why the watch ended.
enum End {
    Exited,
    Over(u64),
    Blind,
    TimedOut,
    SentinelGone,
    Signalled(c_int),
}

/// The next events, up to `until`; empty on the timeout.
fn events_until(kq: c_int, until: Instant, out: &mut [Kevent; 8]) -> usize {
    let wait = until.saturating_duration_since(Instant::now());
    let spec = Timespec {
        tv_sec: wait.as_secs() as i64,
        tv_nsec: wait.subsec_nanos().into(),
    };
    // SAFETY: live buffers of the sizes given.
    let got = unsafe {
        kevent(
            kq,
            core::ptr::null(),
            0,
            out.as_mut_ptr(),
            out.len() as c_int,
            &raw const spec,
        )
    };
    got.max(0) as usize
}

fn watch(kq: c_int, tree: &mut Tree, ctx: &Watch, peak: &mut u64) -> End {
    let deadline = ctx.timeout.map(|t| Instant::now() + t);
    let mut relay: Vec<(c_int, Instant)> = Vec::new();
    let mut next_poll = Instant::now() + POLL;
    let mut events = [NO_EVENT; 8];
    loop {
        let mut until = next_poll;
        if let Some(deadline) = deadline {
            until = until.min(deadline);
        }
        if let Some(&(_, at)) = relay.first() {
            until = until.min(at);
        }
        let got = events_until(kq, until, &mut events);
        for event in &events[..got] {
            match event.filter {
                EVFILT_PROC if event.ident == ctx.program as usize => return End::Exited,
                EVFILT_PROC => return End::SentinelGone,
                EVFILT_SIGNAL if ctx.timeout.is_some() => {
                    return End::Signalled(event.ident as c_int);
                }
                EVFILT_SIGNAL => relay.push((event.ident as c_int, Instant::now() + RELAY_AFTER)),
                _ => {}
            }
        }
        let now = Instant::now();
        relay.retain(|&(signal_number, at)| {
            if at > now {
                return true;
            }
            // SAFETY: the program has not been reaped, so its pid is still its.
            unsafe { kill(ctx.program, signal_number) };
            false
        });
        if deadline.is_some_and(|d| d <= now) {
            return End::TimedOut;
        }
        if next_poll <= now {
            next_poll = now + POLL;
            let pass = tree.pass();
            tree.tell(ctx.to_sentinel);
            *peak = pass.sizes.footprint.max(*peak);
            if pass.sizes.over(ctx.bound) {
                return End::Over(pass.sizes.footprint);
            }
            if pass.blind {
                return End::Blind;
            }
        }
    }
}

struct Watch {
    program: Pid,
    bound: u64,
    timeout: Option<Duration>,
    to_sentinel: c_int,
}

/// Wait for a killed program, a bounded time, as an unkillable one would hang
/// the harness. Its status is not what is reported; its peak resident size is.
fn reap_killed(kq: c_int, program: Pid) -> i64 {
    let deadline = Instant::now() + REAP_FOR;
    let mut events = [NO_EVENT; 8];
    loop {
        let mut status = 0;
        let mut usage = Rusage::default();
        // SAFETY: live out-parameters, and our own child.
        let got = unsafe { wait4(program, &raw mut status, WNOHANG, &raw mut usage) };
        if got != 0 || Instant::now() >= deadline {
            return usage.ru_maxrss;
        }
        events_until(kq, deadline.min(Instant::now() + POLL), &mut events);
    }
}

/// The variables bash rewrites before its script's first line, back as they
/// were; the script's `#!` line copied them. An empty copy was an absent one.
fn restore_environment() {
    for (copy, name) in [
        (c"__BOUNDED_UNDERSCORE", c"_"),
        (c"__BOUNDED_PWD", c"PWD"),
        (c"__BOUNDED_OLDPWD", c"OLDPWD"),
        (c"__BOUNDED_SHLVL", c"SHLVL"),
    ] {
        // SAFETY: names as literals; one thread, so nothing races the environment.
        unsafe {
            let saved = getenv(copy.as_ptr());
            if saved.is_null() {
                continue;
            }
            if *saved == 0 {
                unsetenv(name.as_ptr());
            } else {
                setenv(name.as_ptr(), saved, 1);
            }
            unsetenv(copy.as_ptr());
        }
    }
}

fn machine_memory() -> Option<u64> {
    let mut bytes = 0u64;
    let mut size = size_of::<u64>();
    // SAFETY: a live out-parameter of the size given.
    let rc = unsafe {
        sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&raw mut bytes).cast(),
            &raw mut size,
            core::ptr::null(),
            0,
        )
    };
    (rc == 0).then_some(bytes)
}

fn refuse(text: &str) -> c_int {
    say(&format!("{text}; the program was not run"));
    EXIT_REFUSED
}

/// What this was compiled against, printed for the C probe to be compared with.
fn print_layouts() {
    macro_rules! at {
        ($name:literal, $ty:ty, $field:ident) => {
            println!("{}={}", $name, core::mem::offset_of!($ty, $field));
        };
    }
    println!("rusage_info_v0.size={}", size_of::<RusageInfoV0>());
    at!("rusage_info_v0.resident", RusageInfoV0, resident_size);
    at!("rusage_info_v0.footprint", RusageInfoV0, phys_footprint);
    at!("rusage_info_v0.start", RusageInfoV0, proc_start_abstime);
    at!("rusage_info_v0.exit", RusageInfoV0, proc_exit_abstime);
    println!("kevent.size={}", size_of::<Kevent>());
    at!("kevent.ident", Kevent, ident);
    at!("kevent.filter", Kevent, filter);
    at!("kevent.flags", Kevent, flags);
    at!("kevent.fflags", Kevent, fflags);
    at!("kevent.data", Kevent, data);
    at!("kevent.udata", Kevent, udata);
    println!("timespec.size={}", size_of::<Timespec>());
    at!("timespec.nsec", Timespec, tv_nsec);
    println!("rusage.size={}", size_of::<Rusage>());
    at!("rusage.maxrss", Rusage, ru_maxrss);
    println!("proc_fdinfo.size={}", size_of::<ProcFdinfo>());
    at!("proc_fdinfo.fd", ProcFdinfo, proc_fd);
    println!("rlimit.size={}", size_of::<Rlimit>());
    println!("sigset.size={}", size_of::<u32>());
    let constants: &[(&str, i64)] = &[
        ("RUSAGE_INFO_V0", RUSAGE_INFO_V0.into()),
        ("PROC_PIDLISTFDS", PROC_PIDLISTFDS.into()),
        ("EVFILT_PROC", EVFILT_PROC.into()),
        ("EVFILT_SIGNAL", EVFILT_SIGNAL.into()),
        ("EV_ADD", EV_ADD.into()),
        ("NOTE_EXIT", NOTE_EXIT.into()),
        ("F_SETFL", F_SETFL.into()),
        ("F_DUPFD_CLOEXEC", F_DUPFD_CLOEXEC.into()),
        ("O_NONBLOCK", O_NONBLOCK.into()),
        ("O_WRONLY", O_WRONLY.into()),
        ("O_APPEND", O_APPEND.into()),
        ("O_CREAT", O_CREAT.into()),
        ("O_CLOEXEC", O_CLOEXEC.into()),
        ("ENOENT", ENOENT.into()),
        ("ESRCH", ESRCH.into()),
        ("EINTR", EINTR.into()),
        ("WNOHANG", WNOHANG.into()),
        ("SIG_UNBLOCK", SIG_UNBLOCK.into()),
        ("RLIMIT_CORE", RLIMIT_CORE.into()),
        ("SIGHUP", SIGHUP.into()),
        ("SIGINT", SIGINT.into()),
        ("SIGQUIT", SIGQUIT.into()),
        ("SIGKILL", SIGKILL.into()),
        ("SIGPIPE", SIGPIPE.into()),
        ("SIGALRM", SIGALRM.into()),
        ("SIGTERM", SIGTERM.into()),
        ("SIGCHLD", SIGCHLD.into()),
        ("SIGXCPU", SIGXCPU.into()),
        ("SIGXFSZ", SIGXFSZ.into()),
        ("SIGVTALRM", SIGVTALRM.into()),
        ("SIGPROF", SIGPROF.into()),
        ("SIGUSR1", SIGUSR1.into()),
        ("SIGUSR2", SIGUSR2.into()),
    ];
    for (name, value) in constants {
        println!("{name}={value}");
    }
    println!("exit.over={EXIT_OVER}");
    println!("exit.refused={EXIT_REFUSED}");
    println!("exit.timed_out={EXIT_TIMED_OUT}");
}

/// `--over BOUND FOOTPRINT RESIDENT`, in bytes: `over` if a tree of these
/// sizes is past the bound and killed, `under` if it runs on.
fn print_verdict(args: &[*const c_char]) -> c_int {
    let numbers: Vec<Option<u64>> = args.iter().map(|a| cstr_lossy(*a).parse().ok()).collect();
    let [Some(bound), Some(footprint), Some(resident)] = numbers[..] else {
        say("--over wants a bound, a footprint and a resident size, in bytes");
        return EXIT_REFUSED;
    };
    let sizes = Sizes {
        footprint,
        resident,
    };
    println!("{}", if sizes.over(bound) { "over" } else { "under" });
    0
}

/// `bounded MIB TIMEOUT_SECS PROGRAM ARGS...`, from the script, which has
/// checked the numbers' spelling. A timeout of 0 is the runner.
#[unsafe(no_mangle)]
pub extern "C" fn main(argc: c_int, argv: *const *const c_char) -> c_int {
    // SAFETY: `main`'s own arguments.
    let args = unsafe { core::slice::from_raw_parts(argv, argc.max(0) as usize) };
    if args.len() == 2 && cstr_lossy(args[1]) == "--layouts" {
        print_layouts();
        return 0;
    }
    if args.len() == 5 && cstr_lossy(args[1]) == "--over" {
        return print_verdict(&args[2..]);
    }
    if args.len() < 4 {
        return refuse("the watcher wants a bound, a timeout and a program");
    }
    let Ok(mib) = cstr_lossy(args[1]).parse::<u64>() else {
        return refuse("the bound is not a number");
    };
    let Ok(timeout_secs) = cstr_lossy(args[2]).parse::<u64>() else {
        return refuse("the timeout is not a number");
    };
    let mut mib = mib;
    let mut bound = mib.saturating_mul(1 << 20);
    match machine_memory() {
        Some(memory) if bound <= memory => {}
        Some(memory) => {
            // SAFETY: a name as a literal.
            if !unsafe { getenv(c"BAUD_TEST_MEMORY_BOUND_MIB".as_ptr()) }.is_null() {
                return refuse(&format!(
                    "a bound of {mib} MiB is past this machine's memory, and so no bound"
                ));
            }
            // The file's value on a machine smaller than it.
            bound = memory;
            mib = memory >> 20;
        }
        None => return refuse("this machine's memory could not be read"),
    }
    let timeout = (timeout_secs > 0).then(|| Duration::from_secs(timeout_secs));
    let program_name = cstr_lossy(args[3]);

    restore_environment();
    // SAFETY: a name as a literal.
    let log_path = unsafe { getenv(c"BAUD_TEST_MEMORY_LOG".as_ptr()) };
    let log = (!log_path.is_null()).then(|| Log {
        path: log_path,
        words: args[4..]
            .iter()
            .map(|a| cstr_lossy(*a))
            .collect::<Vec<_>>()
            .join(" ")
            .replace(['\n', '\t'], " "),
    });

    let (Some(gate), Some(report), Some(tell), Some(ready)) =
        (pipe_apart(), pipe_apart(), pipe_apart(), pipe_apart())
    else {
        return refuse("no pipes");
    };

    // **The program first, with this process's signals as they arrived,** and
    // held at the gate until the sentinel and kqueue are ready, so it cannot
    // run unwatched and cannot end before it is watched.
    // SAFETY: one thread; the child calls only what `fork` leaves safe to call.
    let program = unsafe { fork() };
    if program < 0 {
        return refuse("fork failed");
    }
    if program == 0 {
        let mut go = 0u8;
        // SAFETY: plain calls with live arguments; every descriptor here is
        // close-on-exec, and the gate's write end is shut so that this reads
        // end of file if the watcher dies.
        unsafe {
            if timeout.is_some() {
                setsid();
            }
            close(gate[1]);
            if read(gate[0], (&raw mut go).cast(), 1) != 1 {
                _exit(EXIT_REFUSED);
            }
            execvp(args[3], argv.add(3));
            let code = errno();
            write(report[1], (&raw const code).cast(), size_of::<c_int>());
            _exit(127);
        }
    }

    // SAFETY: plain calls in this process alone; the program has forked already.
    unsafe {
        signal(SIGPIPE, SIG_IGN);
        signal(SIGCHLD, SIG_DFL);
        for &held in &HELD {
            signal(held, SIG_IGN);
        }
    }

    // SAFETY: one thread, and the child goes straight to `sentinel`.
    let guard = unsafe { fork() };
    if guard < 0 {
        // The gate closes as this ends, which ends the program unrun.
        return refuse("fork failed");
    }
    if guard == 0 {
        sentinel(tell[0], ready[1], bound, log.as_ref());
    }

    // **The program waits until the sentinel has closed what it inherited,** or
    // a program that ends at once would find its output still held open, and a
    // harness that checks for that would call it a leak.
    let mut is_ready = 0u8;
    // SAFETY: our own descriptors and a live byte.
    let sentinel_ready = unsafe {
        close(ready[1]);
        read(ready[0], (&raw mut is_ready).cast(), 1) == 1
    };
    if !sentinel_ready {
        return refuse("the sentinel did not start");
    }

    // SAFETY: our own descriptors.
    let kq = unsafe {
        close(ready[0]);
        close(gate[0]);
        close(report[1]);
        close(tell[0]);
        fcntl(tell[1], F_SETFL, O_NONBLOCK);
        kqueue()
    };
    if kq < 0 {
        // SAFETY: our own child, held at the gate.
        unsafe { kill(program, SIGKILL) };
        return refuse("no kqueue");
    }
    let add = |ident: usize, filter: i16, fflags: u32| -> bool {
        let change = Kevent {
            ident,
            filter,
            flags: EV_ADD,
            fflags,
            data: 0,
            udata: core::ptr::null_mut(),
        };
        // SAFETY: a live change of one.
        unsafe {
            kevent(
                kq,
                &raw const change,
                1,
                core::ptr::null_mut(),
                0,
                core::ptr::null(),
            ) == 0
        }
    };
    // Registering on a process that has already ended fails, so a failure here
    // is that process's end, and the wait below finds it.
    let sentinel_there = add(guard as usize, EVFILT_PROC, NOTE_EXIT);
    let program_there = add(program as usize, EVFILT_PROC, NOTE_EXIT);
    for &held in &HELD {
        add(held as usize, EVFILT_SIGNAL, 0);
    }

    let mut tree = Tree {
        program,
        members: Vec::new(),
    };
    match look(program) {
        Seen::Alive { start, .. } => tree.remember(program, start, false),
        _ => {
            // SAFETY: our own child, held at the gate.
            unsafe { kill(program, SIGKILL) };
            return refuse("the program's start could not be read");
        }
    }
    tree.tell(tell[1]);

    // SAFETY: a live byte; this opens the gate.
    let opened = unsafe { write(gate[1], [1u8].as_ptr().cast(), 1) } == 1;
    // SAFETY: our own descriptor.
    unsafe { close(gate[1]) };
    let mut exec_errno = 0;
    // SAFETY: a live int. End of file, read 0, is an exec that worked.
    let reported = unsafe { read(report[0], (&raw mut exec_errno).cast(), size_of::<c_int>()) };
    // SAFETY: our own descriptor.
    unsafe { close(report[0]) };
    let last_word = |word: i64| {
        // The sentinel is reading, so this blocks only a moment if at all.
        // SAFETY: our own descriptor; flags 0 clears O_NONBLOCK.
        unsafe { fcntl(tell[1], F_SETFL, 0) };
        send(
            tell[1],
            Record {
                pid: word,
                start: 0,
            },
        );
    };
    if !opened || reported > 0 {
        let mut status = 0;
        // SAFETY: live out-parameter, null usage allowed; our own child.
        unsafe { wait4(program, &raw mut status, 0, core::ptr::null_mut()) };
        last_word(DONE);
        // SAFETY: strerror's string is valid until the next call.
        let why = cstr_lossy(unsafe { strerror(exec_errno) });
        say(&format!("cannot run {program_name}: {why}"));
        return if exec_errno == ENOENT { 127 } else { 126 };
    }

    let context = Watch {
        program,
        bound,
        timeout,
        to_sentinel: tell[1],
    };
    let mut peak = 0;
    let end = if !sentinel_there {
        End::SentinelGone
    } else if !program_there {
        End::Exited
    } else {
        watch(kq, &mut tree, &context, &mut peak)
    };

    let write_log = |peak: u64, maxrss: i64| {
        if let Some(log) = &log {
            log.line(&program_name, peak, maxrss);
        }
    };
    // Past the bound, any growth in the grace ends it; otherwise the bound does.
    let ceiling = match &end {
        End::Over(footprint) => *footprint,
        _ => bound,
    };
    let code = match end {
        End::Exited => {
            let mut status = 0;
            let mut usage = Rusage::default();
            // SAFETY: live out-parameters; our own child, already ended.
            unsafe { wait4(program, &raw mut status, 0, &raw mut usage) };
            let pass = tree.pass();
            peak = peak.max(pass.sizes.footprint);
            write_log(peak, usage.ru_maxrss);
            if timeout.is_some() {
                tree.kill_all(bound);
                last_word(DONE);
            } else if tree.members.is_empty() {
                last_word(DONE);
            } else {
                tree.tell(tell[1]);
                last_word(HANDED_OVER);
            }
            leave_as(status);
        }
        End::Over(footprint) => {
            say(&format!(
                "{program_name} and its children reached {:.2} GiB of footprint, past the \
                 bound of {mib} MiB, and were killed; if that much is legitimate, raise \
                 memory-max in budgets.toml, or BAUD_TEST_MEMORY_BOUND_MIB for one run",
                footprint as f64 / f64::from(1u32 << 30),
            ));
            EXIT_OVER
        }
        End::Blind => {
            say(&format!(
                "the footprint of {program_name} or a child could not be read for \
                 {BLIND_POLLS} polls in a row, and what cannot be read is not bounded; \
                 they were killed"
            ));
            EXIT_REFUSED
        }
        End::TimedOut => {
            say(&format!(
                "{program_name} and its children did not end in {timeout_secs} s, and \
                 were killed"
            ));
            EXIT_TIMED_OUT
        }
        End::SentinelGone => {
            say(&format!(
                "the sentinel died, so a kill of this process would leave {program_name} \
                 running; it and its children were killed"
            ));
            EXIT_REFUSED
        }
        End::Signalled(signal_number) => {
            tree.kill_all(bound);
            write_log(peak, reap_killed(kq, program));
            last_word(DONE);
            raise_exactly(signal_number);
        }
    };
    tree.kill_all(ceiling);
    write_log(peak, reap_killed(kq, program));
    last_word(DONE);
    code
}
