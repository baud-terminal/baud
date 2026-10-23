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
//! `newlocale` -- for a shell whose environment names none.
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

/// Read the terminal's window size back. Only a test asks: it forks a pty and
/// reads what the kernel holds, which is the one reading of "the child was
/// told" that does not trust baud's own bookkeeping.
#[cfg(all(test, target_os = "macos"))]
pub const TIOCGWINSZ: c_ulong = 0x4008_7468;
#[cfg(all(test, not(target_os = "macos")))]
pub const TIOCGWINSZ: c_ulong = 0x5413;

pub const POLLIN: i16 = 0x1;
/// Room to write. The Linux loop asks for it while the pty owes bytes it
/// has not taken; the macOS driver retries those on the display link's tick
/// instead, and only a test asks there.
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

/// `fcntl`'s commands and flag for a descriptor's own flags: the pty's
/// master is set to close on exec, and a test reads that what `openat`
/// opened for a kitty file does. The same values on both platforms.
#[cfg(test)]
pub const F_GETFD: c_int = 1;
pub const F_SETFD: c_int = 2;
pub const FD_CLOEXEC: c_int = 1;
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

// `forkpty` is in libutil on glibc and in libSystem on Darwin.
#[cfg_attr(not(target_os = "macos"), link(name = "util"))]
unsafe extern "C" {
    /// Fork with a fresh pty; the parent gets the master in `amaster`.
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

    /// A new session with no controlling terminal, for the test that holds
    /// [`O_NOCTTY`] to its job: only such a process could take one. A
    /// `pid_t` is an `int` on both platforms.
    #[cfg(test)]
    pub fn setsid() -> c_int;

    /// A pseudo-terminal no session has held, for the same test:
    /// `forkpty`'s child takes its subsidiary as its terminal at once, and
    /// on Darwin the hold outlives the child.
    #[cfg(test)]
    pub fn posix_openpt(flags: c_int) -> Fd;
    #[cfg(test)]
    pub fn grantpt(fd: Fd) -> c_int;
    #[cfg(test)]
    pub fn unlockpt(fd: Fd) -> c_int;
}

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
#[cfg(test)]
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

/// A mapping's length, as `munmap` and `madvise` take it: a `size_t`.
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
}

#[cfg(target_os = "macos")]
impl CfString {
    pub fn is_null(self) -> bool {
        self.0.is_null()
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
    pub fn CFRelease(locale: CfLocale);
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
#[cfg(target_os = "macos")]
pub const NOTE_EXIT: c_uint = 0x8000_0000;
#[cfg(target_os = "macos")]
pub const NOTE_FORK: c_uint = 0x4000_0000;
#[cfg(target_os = "macos")]
pub const NOTE_EXEC: c_uint = 0x2000_0000;

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
    pub fn sysctl(
        name: *mut c_int,
        namelen: c_uint,
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
#include <sys/types.h>
#include <termios.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h>
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
#else
#include <locale.h>
#include <sys/inotify.h>
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
    printf("TIOCGWINSZ=%llu\n", (unsigned long long)TIOCGWINSZ);
    printf("POLLIN=%lld\n", (long long)POLLIN);
    printf("POLLOUT=%lld\n", (long long)POLLOUT);
    printf("POLLNVAL=%lld\n", (long long)POLLNVAL);
    printf("F_GETFL=%lld\n", (long long)F_GETFL);
    printf("F_SETFL=%lld\n", (long long)F_SETFL);
    printf("F_GETFD=%lld\n", (long long)F_GETFD);
    printf("F_SETFD=%lld\n", (long long)F_SETFD);
    printf("FD_CLOEXEC=%lld\n", (long long)FD_CLOEXEC);
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
    printf("uid_t.signed=%d\n", (uid_t)-1 < 0);
    printf("PROT_READ=%lld\n", (long long)PROT_READ);
    printf("MAP_PRIVATE=%lld\n", (long long)MAP_PRIVATE);
    printf("MAP_FAILED=%lld\n", (long long)(intptr_t)MAP_FAILED);
    printf("IUTF8=%lld\n", (long long)IUTF8);
    printf("TCSANOW=%lld\n", (long long)TCSANOW);
    printf("MAP_FIXED=%lld\n", (long long)MAP_FIXED);
    printf("getpagesize=%lld\n", (long long)getpagesize());
    printf("TIOCGPGRP=%llu\n", (unsigned long long)TIOCGPGRP);
    printf("pid_t.size=%zu\n", sizeof(pid_t));
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
    printf("kCFStringEncodingUTF8=%lld\n", (long long)kCFStringEncodingUTF8);
    printf("RTLD_LAZY=%lld\n", (long long)RTLD_LAZY);
    printf("RTLD_LOCAL=%lld\n", (long long)RTLD_LOCAL);
    printf("RTLD_NOLOAD=%lld\n", (long long)RTLD_NOLOAD);
    printf("PSHMNAMLEN=%lld\n", (long long)PSHMNAMLEN);
    printf("O_SEARCH=%lld\n", (long long)O_SEARCH);
    printf("MAP_ANON=%lld\n", (long long)MAP_ANON);
    printf("MACH_TASK_BASIC_INFO=%lld\n", (long long)MACH_TASK_BASIC_INFO);
    printf("MACH_TASK_BASIC_INFO_COUNT=%lld\n", (long long)MACH_TASK_BASIC_INFO_COUNT);
    printf("mach_task_basic_info.size=%zu\n", sizeof(struct mach_task_basic_info));
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
    printf("NOTE_EXIT=%lld\n", (long long)NOTE_EXIT);
    printf("NOTE_FORK=%lld\n", (long long)NOTE_FORK);
    printf("NOTE_EXEC=%lld\n", (long long)NOTE_EXEC);
    printf("EVFILT_VNODE=%lld\n", (long long)EVFILT_VNODE);
    printf("NOTE_DELETE=%lld\n", (long long)NOTE_DELETE);
    printf("NOTE_WRITE=%lld\n", (long long)NOTE_WRITE);
    printf("NOTE_EXTEND=%lld\n", (long long)NOTE_EXTEND);
    printf("NOTE_ATTRIB=%lld\n", (long long)NOTE_ATTRIB);
    printf("NOTE_LINK=%lld\n", (long long)NOTE_LINK);
    printf("NOTE_RENAME=%lld\n", (long long)NOTE_RENAME);
    printf("NOTE_REVOKE=%lld\n", (long long)NOTE_REVOKE);
    printf("O_EVTONLY=%lld\n", (long long)O_EVTONLY);
#else
    printf("locale_t.size=%zu\n", sizeof(locale_t));
    printf("LC_ALL_MASK=%lld\n", (long long)LC_ALL_MASK);
    printf("NAME_MAX=%lld\n", (long long)NAME_MAX);
    printf("O_PATH=%lld\n", (long long)O_PATH);
    printf("MADV_DONTNEED=%lld\n", (long long)MADV_DONTNEED);
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
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = std::process::Command::new(&cc)
            .arg("-o")
            .arg(&bin)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
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
            assert_eq!(
                h["mach_task_basic_info.size"],
                size_of::<MachTaskBasicInfo>() as i128
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
        assert_eq!(h["TIOCGWINSZ"], i128::from(TIOCGWINSZ));
        assert_eq!(h["POLLIN"], i128::from(POLLIN));
        assert_eq!(h["POLLOUT"], i128::from(POLLOUT));
        assert_eq!(h["POLLNVAL"], i128::from(POLLNVAL));
        assert_eq!(h["F_GETFL"], i128::from(F_GETFL));
        assert_eq!(h["F_SETFL"], i128::from(F_SETFL));
        assert_eq!(h["F_GETFD"], i128::from(F_GETFD));
        assert_eq!(h["F_SETFD"], i128::from(F_SETFD));
        assert_eq!(h["FD_CLOEXEC"], i128::from(FD_CLOEXEC));
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
        assert_eq!(h["TIOCGPGRP"], i128::from(TIOCGPGRP));
        #[cfg(target_os = "macos")]
        {
            assert_eq!(h["MAP_ANON"], i128::from(MAP_ANON));
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
            assert_eq!(h["CTL_KERN"], i128::from(CTL_KERN));
            assert_eq!(h["KERN_PROCARGS2"], i128::from(KERN_PROCARGS2));
            assert_eq!(h["EVFILT_PROC"], i128::from(EVFILT_PROC));
            assert_eq!(h["EV_ADD"], i128::from(EV_ADD));
            assert_eq!(h["EV_CLEAR"], i128::from(EV_CLEAR));
            assert_eq!(h["NOTE_EXIT"], i128::from(NOTE_EXIT));
            assert_eq!(h["NOTE_FORK"], i128::from(NOTE_FORK));
            assert_eq!(h["NOTE_EXEC"], i128::from(NOTE_EXEC));
            assert_eq!(h["EVFILT_VNODE"], i128::from(EVFILT_VNODE));
            assert_eq!(h["NOTE_DELETE"], i128::from(NOTE_DELETE));
            assert_eq!(h["NOTE_WRITE"], i128::from(NOTE_WRITE));
            assert_eq!(h["NOTE_EXTEND"], i128::from(NOTE_EXTEND));
            assert_eq!(h["NOTE_ATTRIB"], i128::from(NOTE_ATTRIB));
            assert_eq!(h["NOTE_LINK"], i128::from(NOTE_LINK));
            assert_eq!(h["NOTE_RENAME"], i128::from(NOTE_RENAME));
            assert_eq!(h["NOTE_REVOKE"], i128::from(NOTE_REVOKE));
            assert_eq!(h["O_EVTONLY"], i128::from(O_EVTONLY));
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
        }
    }
}
