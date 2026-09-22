//! **The calls the pty and the font mapping need and `std` does not
//! expose**, declared as the extern lines they are rather than through a
//! `libc` crate -- the way `baud-wayland`'s `sys` declares `sendmsg` and
//! `baud-isa-aarch64` declares `sysctlbyname`.
//!
//! What is *not* here is as deliberate as what is. `open`, `fstat`,
//! `close`, `O_RDONLY` and `O_CLOEXEC` went to [`std::fs::File`], which
//! opens close-on-exec, answers the length from `metadata` and closes on
//! drop; that took the surface from twenty-five items to ten. Everything
//! left is something `std` has no API for: a pty, an ioctl, a poll over
//! two descriptors, a mapping, and Darwin's raw uptime clock.
//!
//! **Every layout and constant below is checked against the platform's own
//! headers** by `tests/abi.rs`, which compiles a C probe with `cc` and
//! compares what it prints. A wrong *signature* usually fails to link; a
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
/// Only the Linux loop polls -- the macOS run loop cannot wait on a
/// descriptor, so the pty is read on a thread there -- which is why this
/// and the three items below carry the same gate.
#[cfg(target_os = "linux")]
pub type Nfds = c_ulong;

/// The terminal's size, as `TIOCSWINSZ` takes it. Four `u16`s, in this
/// order, on both platforms.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// One descriptor `poll` watches.
#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
pub const POLLIN: i16 = 0x1;
pub const PROT_READ: c_int = 0x1;
pub const MAP_PRIVATE: c_int = 0x2;

/// What `mmap` answers with on failure: `(void *) -1`, not null.
pub const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;

/// Darwin's monotonic clock that does not stop while the machine sleeps,
/// which is the one the keystroke instrument stamps on.
#[cfg(target_os = "macos")]
pub const CLOCK_UPTIME_RAW: c_uint = 8;

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
    #[cfg(target_os = "linux")]
    pub fn poll(fds: *mut PollFd, nfds: Nfds, timeout: c_int) -> c_int;
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
    use core::mem::{offset_of, size_of};

    const PROBE: &str = r#"
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <time.h>
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
    printf("TIOCSWINSZ=%llu\n", (unsigned long long)TIOCSWINSZ);
    printf("POLLIN=%lld\n", (long long)POLLIN);
    printf("PROT_READ=%lld\n", (long long)PROT_READ);
    printf("MAP_PRIVATE=%lld\n", (long long)MAP_PRIVATE);
    printf("MAP_FAILED=%lld\n", (long long)(intptr_t)MAP_FAILED);
#ifdef CLOCK_UPTIME_RAW
    printf("CLOCK_UPTIME_RAW=%lld\n", (long long)CLOCK_UPTIME_RAW);
#endif
    return 0;
}
"#;

    /// Compile and run the probe, or say why it could not.
    fn ask_the_headers() -> std::collections::HashMap<String, i128> {
        let dir = std::env::temp_dir().join(format!("baud-abi-{}", std::process::id()));
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
            map.len() >= 16,
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
        #[cfg(target_os = "linux")]
        {
            assert_eq!(h["pollfd.size"], size_of::<PollFd>() as i128);
            assert_eq!(h["pollfd.fd"], offset_of!(PollFd, fd) as i128);
            assert_eq!(h["pollfd.events"], offset_of!(PollFd, events) as i128);
            assert_eq!(h["pollfd.revents"], offset_of!(PollFd, revents) as i128);
        }
        #[cfg(target_os = "linux")]
        assert_eq!(h["nfds.size"], size_of::<Nfds>() as i128);
        assert_eq!(h["request.size"], size_of::<c_ulong>() as i128);
    }

    #[test]
    fn the_constants_are_the_platform_headers_own() {
        let h = ask_the_headers();
        assert_eq!(h["TIOCSWINSZ"], i128::from(TIOCSWINSZ));
        #[cfg(target_os = "linux")]
        assert_eq!(h["POLLIN"], i128::from(POLLIN));
        assert_eq!(h["PROT_READ"], i128::from(PROT_READ));
        assert_eq!(h["MAP_PRIVATE"], i128::from(MAP_PRIVATE));
        assert_eq!(h["MAP_FAILED"], MAP_FAILED as isize as i128);
        #[cfg(target_os = "macos")]
        assert_eq!(h["CLOCK_UPTIME_RAW"], i128::from(CLOCK_UPTIME_RAW));
    }
}
