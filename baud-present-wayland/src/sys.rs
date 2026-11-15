//! **Shared memory for a `wl_shm` pool: a sealed anonymous file and a
//! mapping of it.** On Linux the file is a `memfd`; on this Mac, where the
//! tests run against a socket pair, it is an unlinked temporary file --
//! the compositor side of the test never maps it, and the mapping calls
//! are the same. And a clipboard transfer's pipe, made non-blocking so
//! the driver's one wait reads and writes it a piece at a time. And the
//! compositor's presentation clock, read as `clock_gettime` reads it
//! (`timing.rs`). This is the one module in the crate that allows
//! `unsafe`, for the six externs it declares.

#![allow(unsafe_code)]

use crate::timing::{ClockId, Stamp};
use std::io;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::{AsRawFd, OwnedFd};
use std::time::Duration;

unsafe extern "C" {
    #[cfg(target_os = "linux")]
    fn memfd_create(name: *const u8, flags: u32) -> i32;
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn ftruncate(fd: i32, len: i64) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn clock_gettime(clock: ClockId, ts: *mut Timespec) -> i32;
}

/// `struct timespec` where `time_t` and `long` are 64 bits: the 64-bit
/// Linux targets and Darwin.
#[repr(C)]
#[derive(Default)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

/// The clock `clock` reads now, or `None` where this system has no such
/// clock -- a Linux clock id asked on another system among them.
pub fn clock_now(clock: ClockId) -> Option<Stamp> {
    let mut ts = Timespec::default();
    // SAFETY: a clock id and a pointer to a timespec this frame owns,
    // which the call fills and nothing else holds.
    if unsafe { clock_gettime(clock, &mut ts) } != 0 {
        return None;
    }
    let secs = u64::try_from(ts.tv_sec).ok()?;
    let nanos = u32::try_from(ts.tv_nsec).ok()?;
    Some(Stamp(Duration::new(secs, nanos)))
}

const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
#[cfg(target_os = "linux")]
const O_NONBLOCK: i32 = 0o4000;
#[cfg(not(target_os = "linux"))]
const O_NONBLOCK: i32 = 0x4;

/// `fd` made non-blocking: a read with nothing to read, or a write the
/// pipe has no room for, says `WouldBlock` instead of waiting.
pub fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    // SAFETY: an open descriptor and the two commands that read and set
    // its status flags; neither touches memory.
    let flags = unsafe { fcntl(fd.as_raw_fd(), F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: as above, with the flags just read and `O_NONBLOCK` added.
    if unsafe { fcntl(fd.as_raw_fd(), F_SETFL, flags | O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_SHARED: i32 = 1;
const MAP_PRIVATE: i32 = 2;

/// A shared file's length in bytes: what `ftruncate` sets and `mmap` maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileLen(pub usize);

/// An anonymous file of `len` bytes, ready to be sent as a pool.
pub fn anonymous(len: FileLen) -> io::Result<OwnedFd> {
    #[cfg(target_os = "linux")]
    let fd = {
        const MFD_CLOEXEC: u32 = 1;
        // SAFETY: a NUL-terminated name and a flag; the call allocates
        // nothing the caller owns but the descriptor it returns.
        let raw = unsafe { memfd_create(c"baud-shm".as_ptr().cast(), MFD_CLOEXEC) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the kernel just gave us this descriptor.
        unsafe { OwnedFd::from_raw_fd(raw) }
    };
    #[cfg(not(target_os = "linux"))]
    let fd = {
        let path = std::env::temp_dir().join(format!(
            "baud-shm-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        std::fs::remove_file(&path)?;
        OwnedFd::from(f)
    };
    let len_i =
        i64::try_from(len.0).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "size"))?;
    // SAFETY: an open descriptor and a length.
    if unsafe { ftruncate(fd.as_raw_fd(), len_i) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

#[cfg(not(target_os = "linux"))]
static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A read-write shared mapping of `fd`'s first `len` bytes.
pub struct Map {
    ptr: *mut u8,
    len: FileLen,
}

impl Map {
    pub fn shared(fd: &OwnedFd, len: FileLen) -> io::Result<Map> {
        // SAFETY: a shared mapping of an open file at offset zero; the
        // kernel validates the length against the file.
        let ptr = unsafe {
            mmap(
                core::ptr::null_mut(),
                len.0,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr as isize == -1 || ptr.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Map { ptr, len })
    }

    /// The mapping as 32-bit words: a pool holds pixels.
    pub fn words(&mut self) -> &mut [u32] {
        // SAFETY: the mapping is `len` bytes, writable, shared, and lives as
        // long as `self`; the pointer is page-aligned so the words are.
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast::<u32>(), self.len.0 / 4) }
    }

    pub fn len(&self) -> FileLen {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len.0 == 0
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        // SAFETY: the mapping this struct made and nothing else refers to.
        unsafe { munmap(self.ptr, self.len.0) };
    }
}

/// **A read-only private mapping of `fd`'s first `len` bytes**: what a
/// file the compositor sends to be read -- the dmabuf format table, which
/// "The client must map ... in read-only private mode" -- is mapped as.
pub struct ReadMap {
    ptr: *mut u8,
    len: FileLen,
}

impl ReadMap {
    pub fn private(fd: &OwnedFd, len: FileLen) -> io::Result<ReadMap> {
        // SAFETY: a private read-only mapping of an open file at offset
        // zero; the kernel validates the length against the file.
        let ptr = unsafe {
            mmap(
                core::ptr::null_mut(),
                len.0,
                PROT_READ,
                MAP_PRIVATE,
                fd.as_raw_fd(),
                0,
            )
        };
        if ptr as isize == -1 || ptr.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(ReadMap { ptr, len })
    }

    pub fn bytes(&self) -> &[u8] {
        // SAFETY: the mapping is `len` readable bytes and lives as long as
        // `self`; nothing writes it.
        unsafe { core::slice::from_raw_parts(self.ptr, self.len.0) }
    }
}

impl Drop for ReadMap {
    fn drop(&mut self) {
        // SAFETY: the mapping this struct made and nothing else refers to.
        unsafe { munmap(self.ptr, self.len.0) };
    }
}

// SAFETY: the mapping is memory this process owns; moving the handle
// between threads moves nothing but the pointer.
unsafe impl Send for Map {}
