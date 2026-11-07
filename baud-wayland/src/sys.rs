//! **The two system calls the protocol needs and `std` does not expose:**
//! `sendmsg` and `recvmsg` with `SCM_RIGHTS`, so a file descriptor -- a
//! `wl_shm` pool, a keymap, a dmabuf plane -- rides beside a message. This
//! is the one module in the crate that allows what the crate root denies
//! (`deny(unsafe_code)`), and it is the whole of the dependency floor on
//! Linux: the kernel's socket, declared as the four `extern` lines it is
//! rather than a `libc` crate, the way the ISA crates declare
//! `sysctlbyname`.
//!
//! The struct layouts are the platform's: Linux pads `msg_namelen` to the
//! pointer that follows and sizes the lengths as `size_t`; macOS sizes them
//! as `int` and `socklen_t` and aligns a control message to four rather
//! than eight. Both are here because the tests run on this Mac against a
//! socket pair, and the app runs on Linux.

#![allow(unsafe_code)]

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

#[repr(C)]
struct Iovec {
    base: *mut u8,
    len: usize,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct MsgHdr {
    name: *mut u8,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: usize,
    control: *mut u8,
    controllen: Len,
    flags: i32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct MsgHdr {
    name: *mut u8,
    namelen: u32,
    iov: *mut Iovec,
    iovlen: i32,
    control: *mut u8,
    controllen: Len,
    flags: i32,
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct CmsgHdr {
    len: Len,
    level: i32,
    ty: i32,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct CmsgHdr {
    len: Len,
    level: i32,
    ty: i32,
}

/// The kernel's width for a control buffer's length and a control
/// message's: `size_t` on Linux, `socklen_t` on Darwin.
#[cfg(target_os = "linux")]
type Len = usize;
#[cfg(target_os = "macos")]
type Len = u32;

/// A length of control-message bytes: a header and the descriptors after
/// it, or the aligned space that holds them. Bounded by `MAX_FDS`, so it
/// always fits `Len`, and it is the one quantity that crosses into the
/// kernel's width and back -- `raw` and `of_raw` are where, on either
/// system, without a conversion of a type to itself.
#[derive(Clone, Copy)]
struct ControlBytes(usize);

impl ControlBytes {
    /// A header with `fds` descriptors after it, unaligned: what a control
    /// message's own `len` says.
    fn message(fds: &[BorrowedFd<'_>]) -> ControlBytes {
        ControlBytes(core::mem::size_of::<CmsgHdr>() + fds.len() * core::mem::size_of::<RawFd>())
    }

    /// The largest message this module receives: `MAX_FDS` descriptors.
    fn largest() -> ControlBytes {
        ControlBytes(core::mem::size_of::<CmsgHdr>() + MAX_FDS * core::mem::size_of::<RawFd>())
    }

    /// The buffer space a message of this length takes, aligned.
    fn space(self) -> ControlBytes {
        ControlBytes(self.0.div_ceil(CMSG_ALIGN) * CMSG_ALIGN)
    }

    fn index(self) -> usize {
        self.0
    }

    #[cfg(target_os = "linux")]
    fn raw(self) -> Len {
        self.0
    }
    #[cfg(target_os = "macos")]
    fn raw(self) -> Len {
        Len::try_from(self.0).expect("a control buffer of a few fds fits its length field")
    }

    #[cfg(target_os = "linux")]
    fn of_raw(l: Len) -> ControlBytes {
        ControlBytes(l)
    }
    #[cfg(target_os = "macos")]
    fn of_raw(l: Len) -> ControlBytes {
        ControlBytes(usize::try_from(l).expect("a length"))
    }
}

#[cfg(target_os = "linux")]
const SOL_SOCKET: i32 = 1;
#[cfg(target_os = "macos")]
const SOL_SOCKET: i32 = 0xffff;
const SCM_RIGHTS: i32 = 1;
#[cfg(target_os = "linux")]
const SEND_FLAGS: i32 = 0x4000; // MSG_NOSIGNAL
#[cfg(target_os = "macos")]
const SEND_FLAGS: i32 = 0;
#[cfg(target_os = "linux")]
const RECV_FLAGS: i32 = 0x4000_0000; // MSG_CMSG_CLOEXEC
#[cfg(target_os = "macos")]
const RECV_FLAGS: i32 = 0;

/// The alignment of a control message: Linux rounds to its `size_t`,
/// macOS to four -- the `CMSG_ALIGN` macro of each platform's headers.
#[cfg(target_os = "linux")]
const CMSG_ALIGN: usize = 8;
#[cfg(target_os = "macos")]
const CMSG_ALIGN: usize = 4;

/// The most fds one message may carry: the protocol's own bound.
pub const MAX_FDS: usize = 28;

unsafe extern "C" {
    fn sendmsg(fd: i32, msg: *const MsgHdr, flags: i32) -> isize;
    fn recvmsg(fd: i32, msg: *mut MsgHdr, flags: i32) -> isize;
}

/// Send `bytes` with `fds` beside them. Returns the bytes accepted; a short
/// send carries every fd with the first byte, which is what the protocol
/// expects, so the caller retries without the fds.
pub fn send(sock: &UnixStream, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<usize> {
    assert!(fds.len() <= MAX_FDS);
    let mut iov = Iovec {
        base: bytes.as_ptr().cast_mut(),
        len: bytes.len(),
    };
    let mut control = [0u8; 256];
    let space = ControlBytes::message(fds).space();
    assert!(space.index() <= control.len());
    if !fds.is_empty() {
        let hdr = CmsgHdr {
            len: ControlBytes::message(fds).raw(),
            level: SOL_SOCKET,
            ty: SCM_RIGHTS,
        };
        // SAFETY: `control` is a byte array larger than the header, and the
        // header is plain old data written at offset zero.
        unsafe { core::ptr::write_unaligned(control.as_mut_ptr().cast::<CmsgHdr>(), hdr) };
        let data = core::mem::size_of::<CmsgHdr>();
        for (i, fd) in fds.iter().enumerate() {
            let raw = fd.as_raw_fd().to_ne_bytes();
            control[data + i * 4..data + i * 4 + 4].copy_from_slice(&raw);
        }
    }
    let msg = MsgHdr {
        name: core::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: match fds.is_empty() {
            true => core::ptr::null_mut(),
            false => control.as_mut_ptr(),
        },
        controllen: match fds.is_empty() {
            true => 0,
            false => space.raw(),
        },
        flags: 0,
    };
    // SAFETY: every pointer in `msg` is to a live local or to `bytes`, the
    // lengths are those buffers' lengths, and the fd is an open socket.
    let n = unsafe { sendmsg(sock.as_raw_fd(), &msg, SEND_FLAGS) };
    usize::try_from(n).map_err(|_| io::Error::last_os_error())
}

/// Receive into `buf`, and any fds that arrived into `fds`, owned. Returns
/// the bytes received; zero is the peer hanging up.
pub fn recv(sock: &UnixStream, buf: &mut [u8], fds: &mut Vec<OwnedFd>) -> io::Result<usize> {
    let mut iov = Iovec {
        base: buf.as_mut_ptr(),
        len: buf.len(),
    };
    let mut control = [0u8; 256];
    let space = ControlBytes::largest().space();
    assert!(space.index() <= control.len());
    let mut msg = MsgHdr {
        name: core::ptr::null_mut(),
        namelen: 0,
        iov: &mut iov,
        iovlen: 1,
        control: control.as_mut_ptr(),
        controllen: space.raw(),
        flags: 0,
    };
    // SAFETY: as in `send`; the kernel writes within the lengths given.
    let n = unsafe { recvmsg(sock.as_raw_fd(), &mut msg, RECV_FLAGS) };
    let n = usize::try_from(n).map_err(|_| io::Error::last_os_error())?;
    // Walk the control messages the kernel filled in.
    let filled = ControlBytes::of_raw(msg.controllen).index();
    let hdr = core::mem::size_of::<CmsgHdr>();
    let mut at = 0;
    while at + hdr <= filled {
        // SAFETY: `at + hdr <= filled <= control.len()`, so the read is in
        // bounds; the kernel wrote a header there.
        let c: CmsgHdr = unsafe { core::ptr::read_unaligned(control.as_ptr().add(at).cast()) };
        let len = ControlBytes::of_raw(c.len).index();
        if len < hdr || at + len > filled {
            break;
        }
        if c.level == SOL_SOCKET && c.ty == SCM_RIGHTS {
            let data = &control[at + hdr..at + len];
            for raw in data.as_chunks::<4>().0 {
                let fd = RawFd::from_ne_bytes(*raw);
                // SAFETY: the kernel just installed this descriptor for us and
                // nothing else owns it.
                fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
            }
        }
        at += len.div_ceil(CMSG_ALIGN) * CMSG_ALIGN;
    }
    Ok(n)
}

/// An inherited descriptor number as an owned one: `WAYLAND_SOCKET` is the
/// contract by which a parent hands a client a socket it owns.
///
/// **Close-on-exec, as libwayland makes it**: `wl_display_connect` sets
/// `FD_CLOEXEC` on the descriptor it is handed (`[[source:
/// libwayland-93c04ad-client]]` 1374-1378), because a parent hands it
/// over open across its `exec` and so without the flag. Without it a
/// program the client starts -- a new window's baud -- would hold the
/// compositor's connection open after the client exits, and the client's
/// window would stay on screen. A copy made close-on-exec stands in for
/// the number handed over, which is closed: the same flag with no `fcntl`.
pub fn inherited(raw: RawFd) -> io::Result<OwnedFd> {
    if raw < 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "negative fd"));
    }
    // SAFETY: borrowed only for the copy, which `F_DUPFD_CLOEXEC` makes;
    // a number that is not open fails it with `EBADF` and is not owned.
    let copy = unsafe { BorrowedFd::borrow_raw(raw) }.try_clone_to_owned()?;
    // SAFETY: the copy succeeded, so `raw` is open, and the parent gave it
    // to this process to own: nothing else here holds it.
    drop(unsafe { OwnedFd::from_raw_fd(raw) });
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::inherited;
    use std::os::fd::{AsRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream;

    /// What a child sees of a descriptor after its exec.
    #[derive(Debug, PartialEq, Eq)]
    enum Seen {
        Open,
        Closed,
    }

    /// Whether `/dev/fd/<fd>` names an open descriptor in a child.
    fn in_a_child(fd: &impl AsRawFd) -> Seen {
        let open = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("test -e /dev/fd/{}", fd.as_raw_fd()))
            .status()
            .expect("sh runs")
            .success();
        match open {
            true => Seen::Open,
            false => Seen::Closed,
        }
    }

    /// **The socket a parent hands over is kept close-on-exec, and the
    /// number it came as is closed**, so a program the client starts holds
    /// no copy of the compositor's connection. The control is that the
    /// child's probe sees its own standard input. `UnixStream::pair`
    /// already hands out close-on-exec descriptors, so a version that kept
    /// the number it was given fails here on the number alone (3 and 3):
    /// the exec assertion checks the copy's flag, and does not reproduce a
    /// parent-handed socket that lacks `FD_CLOEXEC`.
    #[test]
    fn an_inherited_socket_is_kept_close_on_exec_under_a_new_number() {
        assert_eq!(
            in_a_child(&std::io::stdin()),
            Seen::Open,
            "the control: the probe sees an open descriptor"
        );
        let (ours, theirs) = UnixStream::pair().expect("a socket pair");
        // Set while both ends are open: Darwin refuses it once the peer
        // has gone.
        theirs
            .set_read_timeout(Some(std::time::Duration::from_secs(60)))
            .expect("a read timeout");
        let raw = ours.into_raw_fd();
        let kept = inherited(raw).expect("an open descriptor");
        assert_ne!(kept.as_raw_fd(), raw, "a copy stands in for it");
        assert_eq!(
            in_a_child(&kept),
            Seen::Closed,
            "the copy is not open across an exec"
        );
        // The copy is the socket: what is written reaches the other end.
        use std::io::{Read, Write};
        let mut kept = UnixStream::from(kept);
        kept.write_all(b"hi").expect("write");
        let mut got = [0u8; 2];
        let mut theirs = theirs;
        theirs.read_exact(&mut got).expect("read");
        assert_eq!(&got, b"hi");
        // **The number handed over was closed**: with the copy dropped, the
        // other end reads the end of the stream, which it does only once
        // every descriptor for this end is closed. The bound turns a leaked
        // number's hang into a failure; a pass reads at once.
        drop(kept);
        let mut rest = [0u8; 1];
        assert_eq!(
            theirs
                .read(&mut rest)
                .expect("the end of the stream, not a timeout"),
            0,
            "no copy of the handed-over number is left open"
        );
        assert!(inherited(-1).is_err(), "a negative number is refused");
    }
}
