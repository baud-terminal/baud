//! **Whether an fd is a DRM syncobj, asked of the kernel before the
//! compositor is told it is one.** `import_timeline` has no refusal: "If
//! the FD cannot be imported, the invalid_timeline error is raised"
//! (`[[source: wp-linux-drm-syncobj-v1]]`), a protocol error, which ends
//! the connection. A
//! timeline semaphore exported as an opaque fd is a syncobj on the drivers
//! that run here, but Vulkan does not promise it, so the fd is turned into
//! a handle on the device's render node with `DRM_IOCTL_SYNCOBJ_FD_TO_HANDLE`
//! and the handle destroyed at once: only a syncobj converts. This module
//! declares `ioctl` as the extern it is, the way the Wayland crates declare
//! theirs, and is the crate's other place `unsafe` is allowed.

#![allow(unsafe_code)]

use crate::choose::Node;
use std::fs::File;
use std::os::fd::{AsRawFd, BorrowedFd};

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
