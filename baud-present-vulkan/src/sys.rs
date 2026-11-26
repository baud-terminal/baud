//! **Whether an fd is a DRM syncobj, asked of the kernel before the
//! compositor is told it is one.** `import_timeline` has no refusal: "If
//! the FD cannot be imported, the invalid_timeline error is raised"
//! (`[[source: wp-linux-drm-syncobj-v1]]`), a protocol error, which ends
//! the connection. A
//! timeline semaphore exported as an opaque fd is a syncobj on the drivers
//! that run here, but Vulkan does not promise it, so the fd is turned into
//! a handle on the device's render node with `DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE`
//! and the handle destroyed at once: only a syncobj converts.
//!
//! **A frame's end on its target's dmabuf**, where frames carry no points:
//! the sync file is put on the dmabuf as its write fence, which the
//! compositor's implicitly synchronised reads wait on, and whether a
//! dmabuf takes sync files at all is asked once per target set
//! (`[[source: linux-uapi-dma-buf]]`).
//!
//! This module declares `ioctl` as the extern it is, the way the Wayland crates declare
//! theirs, and is the crate's other place `unsafe` is allowed.

use crate::choose::Node;
use std::fs::File;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

#[allow(unsafe_code)]
unsafe extern "C" {
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

/// `struct drm_syncobj_handle`: the handle, flags, the fd and padding.
#[repr(C)]
#[derive(Default)]
struct SyncobjHandle {
    handle: Handle,
    flags: Handle,
    fd: i32,
    pad: Handle,
}

/// `struct drm_syncobj_destroy`.
#[repr(C)]
struct SyncobjDestroy {
    handle: Handle,
    pad: Handle,
}

/// A DRM object handle, or a field of its width beside one.
#[repr(transparent)]
#[derive(Clone, Copy, Default)]
struct Handle(u32);

/// `DRM_IOWR(0xC2, struct drm_syncobj_handle)`: read-write, sixteen bytes,
/// type `'d'`.
const SYNCOBJ_FD_TO_HANDLE: u64 = 0xC010_64C2;
/// `DRM_IOWR(0xC0, struct drm_syncobj_destroy)`: eight bytes.
const SYNCOBJ_DESTROY: u64 = 0xC008_64C0;

/// **Whether `fd` is a syncobj**, asked on `render`'s node. A node that
/// cannot be opened answers no, and the path is implicit.
#[allow(unsafe_code)]
pub fn is_syncobj(render: Node, fd: BorrowedFd<'_>) -> bool {
    let path = format!("/dev/dri/renderD{}", render.minor.0);
    let Ok(node) = File::options().read(true).write(true).open(path) else {
        return false;
    };
    let mut h = SyncobjHandle {
        fd: fd.as_raw_fd(),
        ..SyncobjHandle::default()
    };
    // SAFETY: an open DRM node, the request's own struct, which the kernel
    // reads and writes and does not keep.
    let ok = unsafe { ioctl(node.as_raw_fd(), SYNCOBJ_FD_TO_HANDLE, &mut h) } == 0;
    if ok {
        let mut d = SyncobjDestroy {
            handle: h.handle,
            pad: Handle(0),
        };
        // SAFETY: as above; the handle was just made on this node.
        unsafe { ioctl(node.as_raw_fd(), SYNCOBJ_DESTROY, &mut d) };
    }
    ok
}

/// `struct pollfd`: the descriptor, the events asked for and those found.
#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

/// `POLLIN`: there is data to read, or a sync file has signalled.
const POLLIN: i16 = 1;

/// `nfds_t`: how many `pollfd`s.
#[repr(transparent)]
struct Nfds(u64);

/// A wait in milliseconds, `0` for none; or what `poll` answered: the
/// descriptors ready, or `-1`.
#[repr(transparent)]
struct Int(i32);

#[allow(unsafe_code)]
unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: Nfds, timeout: Int) -> Int;
}

/// Whether a descriptor is readable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readable {
    Yes,
    No,
}

/// **Whether `fd` is readable now**, asked of `poll` with no wait: a sync
/// file is once the work it stands for has finished.
#[allow(unsafe_code)]
pub fn readable(fd: BorrowedFd<'_>) -> Readable {
    let mut p = PollFd {
        fd: fd.as_raw_fd(),
        events: POLLIN,
        revents: 0,
    };
    // SAFETY: one `pollfd` that lives through the call, and no wait.
    let n = unsafe { poll(&mut p, Nfds(1), Int(0)) };
    match n.0 > 0 && p.revents & POLLIN != 0 {
        true => Readable::Yes,
        false => Readable::No,
    }
}

/// `struct dma_buf_export_sync_file` and `struct dma_buf_import_sync_file`
/// alike: the access flags and a sync file's fd
/// (`[[source: linux-uapi-dma-buf]]`).
#[repr(C)]
struct DmaSync {
    flags: Handle,
    fd: i32,
}

/// `DMA_BUF_SYNC_READ`: an export that waits on the dmabuf's writers.
const SYNC_READ: Handle = Handle(1);
/// `DMA_BUF_SYNC_WRITE`: an import that "inserts the sync_file as a write
/// fence. All subsequent implicitly synchronized access to this dma-buf
/// will wait on this fence" (`[[source: linux-uapi-dma-buf]]`).
const SYNC_WRITE: Handle = Handle(2);
/// `_IOWR('b', 2, struct dma_buf_export_sync_file)`: read-write, eight
/// bytes, type `'b'`.
const EXPORT_SYNC_FILE: u64 = 0xC008_6202;
/// `_IOW('b', 3, struct dma_buf_import_sync_file)`: write, eight bytes.
const IMPORT_SYNC_FILE: u64 = 0x4008_6203;

/// Whether a dmabuf takes sync files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncFiles {
    Taken,
    /// The kernel refused: `ENOTTY` where the dmabuf has no such ioctl,
    /// `EINVAL` where its exporter does not keep fences.
    Refused,
}

/// **Whether `dmabuf` takes sync files**, asked once of a target with
/// `DMA_BUF_IOCTL_EXPORT_SYNC_FILE` (`[[source: linux-uapi-dma-buf]]`):
/// the sync file it answers with is closed at once. A refusal of any kind
/// answers no.
#[allow(unsafe_code)]
pub fn takes_sync_files(dmabuf: BorrowedFd<'_>) -> SyncFiles {
    let mut s = DmaSync {
        flags: SYNC_READ,
        fd: -1,
    };
    // SAFETY: an open descriptor and the request's own struct, which the
    // kernel reads and writes and does not keep.
    let ok = unsafe { ioctl(dmabuf.as_raw_fd(), EXPORT_SYNC_FILE, &mut s) } == 0;
    match ok && s.fd >= 0 {
        true => {
            // SAFETY: the kernel hands over a new descriptor the caller owns.
            drop(unsafe { OwnedFd::from_raw_fd(s.fd) });
            SyncFiles::Taken
        }
        false => SyncFiles::Refused,
    }
}

/// **`end` put on `dmabuf` as its write fence**, with
/// `DMA_BUF_IOCTL_IMPORT_SYNC_FILE` and `DMA_BUF_SYNC_WRITE`
/// (`[[source: linux-uapi-dma-buf]]`): every implicitly synchronised
/// access after waits for the work `end` stands for. The kernel takes its
/// own reference to the fence; `end` stays the caller's.
#[allow(unsafe_code)]
pub fn put_write_fence(dmabuf: BorrowedFd<'_>, end: BorrowedFd<'_>) -> SyncFiles {
    let mut s = DmaSync {
        flags: SYNC_WRITE,
        fd: end.as_raw_fd(),
    };
    // SAFETY: as above; the kernel reads the struct and keeps no pointer.
    match unsafe { ioctl(dmabuf.as_raw_fd(), IMPORT_SYNC_FILE, &mut s) } == 0 {
        true => SyncFiles::Taken,
        false => SyncFiles::Refused,
    }
}
