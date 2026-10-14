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

/// `tcflag_t`, the type of each of a termios's four flag words: `unsigned
/// long` on Darwin and `unsigned int` on glibc.
#[cfg(target_os = "macos")]
pub type TcFlag = c_ulong;
#[cfg(not(target_os = "macos"))]
pub type TcFlag = core::ffi::c_uint;

/// **`struct termios`, opaque but for its first field.** What the kernel
/// hands out is handed back with one bit of `c_iflag` set or cleared, so
/// nothing else is read or built and nothing else is named. The size, the alignment and
/// `c_iflag`'s offset are the header's, which the tests below hold: a block
/// smaller than the header's would let `tcgetattr` write past it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: TcFlag,
    rest: [u8; TERMIOS_SIZE - core::mem::size_of::<TcFlag>()],
}

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
/// [`freelocale`]. Used to check a locale name before a shell is given it.
#[cfg(target_os = "macos")]
pub type Locale = *mut c_void;

/// `newlocale`'s mask for every category. `LANG` feeds them all, and a
/// shell's `setlocale(LC_ALL, "")` fails as a whole when one cannot load,
/// so a name is checked against all of them, not the character type alone
/// (which accepts `UTF-8`, a name the whole mask refuses).
#[cfg(target_os = "macos")]
pub const LC_ALL_MASK: c_int = 63;

/// `nl_langinfo`'s item, an `int`.
#[cfg(target_os = "macos")]
pub type NlItem = c_int;

/// The item that names a locale's character set.
#[cfg(target_os = "macos")]
pub const CODESET: NlItem = 0;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    pub fn newlocale(mask: c_int, name: *const c_char, base: Locale) -> Locale;
    pub fn nl_langinfo_l(item: NlItem, locale: Locale) -> *const c_char;
    pub fn freelocale(locale: Locale);
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
#include <fcntl.h>
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <termios.h>
#include <time.h>
#ifdef __APPLE__
#include <CoreFoundation/CoreFoundation.h>
#include <dlfcn.h>
#include <langinfo.h>
#include <locale.h>
#include <xlocale.h>
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
    printf("tcflag.size=%zu\n", sizeof(tcflag_t));
    printf("TIOCSWINSZ=%llu\n", (unsigned long long)TIOCSWINSZ);
    printf("TIOCGWINSZ=%llu\n", (unsigned long long)TIOCGWINSZ);
    printf("POLLIN=%lld\n", (long long)POLLIN);
    printf("POLLOUT=%lld\n", (long long)POLLOUT);
    printf("POLLNVAL=%lld\n", (long long)POLLNVAL);
    printf("F_GETFL=%lld\n", (long long)F_GETFL);
    printf("F_SETFL=%lld\n", (long long)F_SETFL);
    printf("O_NONBLOCK=%lld\n", (long long)O_NONBLOCK);
    printf("O_NOCTTY=%lld\n", (long long)O_NOCTTY);
    printf("O_RDWR=%lld\n", (long long)O_RDWR);
    printf("PROT_READ=%lld\n", (long long)PROT_READ);
    printf("MAP_PRIVATE=%lld\n", (long long)MAP_PRIVATE);
    printf("MAP_FAILED=%lld\n", (long long)(intptr_t)MAP_FAILED);
    printf("IUTF8=%lld\n", (long long)IUTF8);
    printf("TCSANOW=%lld\n", (long long)TCSANOW);
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
#endif
    return 0;
}
"#;

    /// The lines the probe prints on every platform, before its
    /// `#ifdef`s: the floor its output is held to.
    const PRINTED_EVERYWHERE: usize = 30;

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
        assert_eq!(h["tcflag.size"], size_of::<TcFlag>() as i128);
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
        assert_eq!(h["O_NONBLOCK"], i128::from(O_NONBLOCK));
        assert_eq!(h["O_NOCTTY"], i128::from(O_NOCTTY));
        assert_eq!(h["O_RDWR"], i128::from(O_RDWR));
        assert_eq!(h["PROT_READ"], i128::from(PROT_READ));
        assert_eq!(h["MAP_PRIVATE"], i128::from(MAP_PRIVATE));
        assert_eq!(h["MAP_FAILED"], MAP_FAILED as isize as i128);
        assert_eq!(h["IUTF8"], i128::from(IUTF8));
        assert_eq!(h["TCSANOW"], i128::from(TCSANOW));
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
        }
    }
}
