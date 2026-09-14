//! **Shared memory for a `wl_shm` pool: a sealed anonymous file and a
//! mapping of it.** On Linux the file is a `memfd`; on this Mac, where the
//! tests run against a socket pair, it is an unlinked temporary file --
//! the compositor side of the test never maps it, and the mapping calls
//! are the same. This is the one module in the crate that allows
//! `unsafe`, for the four externs it declares.

#![allow(unsafe_code)]

use std::io;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
use std::os::fd::{AsRawFd, OwnedFd};

unsafe extern "C" {
    #[cfg(target_os = "linux")]
    fn memfd_create(name: *const u8, flags: u32) -> i32;
    fn mmap(addr: *mut u8, len: usize, prot: i32, flags: i32, fd: i32, off: i64) -> *mut u8;
    fn munmap(addr: *mut u8, len: usize) -> i32;
    fn ftruncate(fd: i32, len: i64) -> i32;
}

const PROT_READ: i32 = 1;
const PROT_WRITE: i32 = 2;
const MAP_SHARED: i32 = 1;

/// An anonymous file of `len` bytes, ready to be sent as a pool.
pub fn anonymous(len: usize) -> io::Result<OwnedFd> {
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
        i64::try_from(len).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "size"))?;
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
    len: usize,
}

impl Map {
    pub fn shared(fd: &OwnedFd, len: usize) -> io::Result<Map> {
        // SAFETY: a shared mapping of an open file at offset zero; the
        // kernel validates the length against the file.
        let ptr = unsafe {
            mmap(
                core::ptr::null_mut(),
                len,
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
        unsafe { core::slice::from_raw_parts_mut(self.ptr.cast::<u32>(), self.len / 4) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for Map {
    fn drop(&mut self) {
        // SAFETY: the mapping this struct made and nothing else refers to.
        unsafe { munmap(self.ptr, self.len) };
    }
}

// SAFETY: the mapping is memory this process owns; moving the handle
// between threads moves nothing but the pointer.
unsafe impl Send for Map {}
