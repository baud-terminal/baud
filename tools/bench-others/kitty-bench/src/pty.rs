//! The PTY loop, the same shape as `multibench`'s `through_pty`: same fork, same
//! raw-mode slave, same read size, same "stop once `total` bytes have arrived".

use crate::Error;
use std::os::fd::RawFd;
use std::time::Instant;

pub const CHUNK: usize = 4096;

/// What one run moved: the bytes read, and the rate over the read loop alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transfer {
    pub bytes: u64,
    pub mib_per_s: f64,
}

fn last_os(call: &'static str) -> Error {
    Error::Pty {
        call,
        source: std::io::Error::last_os_error(),
    }
}

/// Read `fd` `CHUNK` bytes at a time into `sink` until `total` bytes have arrived.
///
/// `sink` returns the bytes it accounted for. A run whose sink accounted for a
/// different number than was read, or whose source ended short of `total`, is a
/// failure rather than a rate.
pub fn pump(
    fd: RawFd,
    total: u64,
    mut sink: impl FnMut(&[u8]) -> Result<usize, Error>,
) -> Result<Transfer, Error> {
    let mut buf = vec![0u8; CHUNK];
    let mut got = 0u64;
    let mut parsed = 0u64;
    let t0 = Instant::now();
    while got < total {
        // SAFETY: the length is `buf`'s own, and `buf` outlives the call.
        let r = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if r < 0 {
            let source = std::io::Error::last_os_error();
            match source.raw_os_error() {
                Some(libc::EINTR) => continue,
                // How a master reports its last slave gone, on Linux.
                Some(libc::EIO) => break,
                _ => {
                    return Err(Error::Pty {
                        call: "read",
                        source,
                    });
                }
            }
        }
        if r == 0 {
            break;
        }
        let chunk = &buf[..r as usize];
        parsed += sink(chunk)? as u64;
        got += r as u64;
    }
    let mib_per_s = got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0;
    if got < total {
        return Err(Error::ShortRead { got, total });
    }
    if parsed != got {
        return Err(Error::WrongByteCount { read: got, parsed });
    }
    Ok(Transfer {
        bytes: got,
        mib_per_s,
    })
}

/// Fork a writer on a PTY slave and hand the master to `sink`.
///
/// `payload` must not be empty: the writer would send nothing, for ever.
pub fn through_pty(
    payload: &[u8],
    total: u64,
    sink: impl FnMut(&[u8]) -> Result<usize, Error>,
) -> Result<Transfer, Error> {
    assert!(!payload.is_empty(), "an empty payload never reaches total");
    let (mut m, mut s) = (0 as libc::c_int, 0 as libc::c_int);
    // SAFETY: the out-pointers are live and the other arguments may be null.
    let rc = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(last_os("openpty"));
    }
    // SAFETY: `s` is the slave just opened, and `termios` is plain data.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(s, libc::TCSANOW, &t);
    }
    // SAFETY: the child calls only write, close and _exit, all async-signal-safe,
    // so a Python or AppKit loaded in the parent is never touched in it.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(last_os("fork"));
    }
    if pid == 0 {
        // SAFETY: as above, and `payload` is live in the child's copy.
        unsafe { writer(m, s, payload, total) }
    }
    // SAFETY: the parent's copy of the slave, not used again. Closed so that a
    // writer that dies ends the read and shows as a short read.
    unsafe { libc::close(s) };
    let result = pump(m, total, sink);
    // SAFETY: `pid` is the child forked above and `m` the master; each is used
    // once more here and not after.
    unsafe {
        libc::kill(pid, libc::SIGKILL);
        let mut status = 0;
        libc::waitpid(pid, &mut status, 0);
        libc::close(m);
    }
    result
}

/// The child: whole copies of `payload` until `total` is covered, then exit.
unsafe fn writer(m: RawFd, s: RawFd, payload: &[u8], total: u64) -> ! {
    libc::close(m);
    let mut sent = 0u64;
    while sent < total {
        let mut o = 0usize;
        while o < payload.len() {
            let w = libc::write(s, payload.as_ptr().add(o).cast(), payload.len() - o);
            if w <= 0 {
                libc::close(s);
                libc::_exit(0);
            }
            o += w as usize;
        }
        sent += payload.len() as u64;
    }
    libc::close(s);
    libc::_exit(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pipe holding `bytes`, its write end closed: a source that ends.
    fn pipe_with(bytes: &[u8]) -> RawFd {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `fds` is two ints; `bytes` is under a pipe's capacity here.
        unsafe {
            assert_eq!(libc::pipe(fds.as_mut_ptr()), 0);
            assert_eq!(
                libc::write(fds[1], bytes.as_ptr().cast(), bytes.len()),
                bytes.len() as isize
            );
            libc::close(fds[1]);
        }
        fds[0]
    }

    #[test]
    fn a_source_that_ends_short_is_a_failure_and_not_a_rate() {
        let fd = pipe_with(&[7u8; 100]);
        let e = pump(fd, 101, |c| Ok(c.len())).unwrap_err();
        // SAFETY: the read end of the pipe above.
        unsafe { libc::close(fd) };
        assert!(
            matches!(
                e,
                Error::ShortRead {
                    got: 100,
                    total: 101
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_sink_that_accounts_for_too_few_bytes_is_a_wrong_count() {
        let fd = pipe_with(&[7u8; 100]);
        let e = pump(fd, 100, |c| Ok(c.len() - 1)).unwrap_err();
        // SAFETY: the read end of the pipe above.
        unsafe { libc::close(fd) };
        assert!(
            matches!(
                e,
                Error::WrongByteCount {
                    read: 100,
                    parsed: 99
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_read_past_total_stops_and_counts_what_arrived() {
        let fd = pipe_with(&[7u8; 5000]);
        let t = pump(fd, 100, |c| Ok(c.len())).unwrap();
        // SAFETY: the read end of the pipe above.
        unsafe { libc::close(fd) };
        assert_eq!(t.bytes, CHUNK as u64);
    }

    /// A pipe, because a tty on Darwin returns 1024 bytes a read at most and
    /// could not show a read of more than `CHUNK`.
    #[test]
    fn every_read_asks_for_one_chunk() {
        let fd = pipe_with(&[7u8; CHUNK + 904]);
        let mut sizes = Vec::new();
        pump(fd, CHUNK as u64 + 904, |c| {
            sizes.push(c.len());
            Ok(c.len())
        })
        .unwrap();
        // SAFETY: the read end of the pipe above.
        unsafe { libc::close(fd) };
        assert_eq!(sizes, [CHUNK, 904]);
    }

    #[test]
    fn a_bad_descriptor_is_a_failure() {
        let e = pump(-1, 1, |c| Ok(c.len())).unwrap_err();
        assert!(matches!(e, Error::Pty { call: "read", .. }), "{e:?}");
    }

    /// Every byte value, in a payload long enough that the writer is still
    /// blocked with bytes to send when the reader reaches `total`.
    fn all_bytes(len: usize) -> Vec<u8> {
        (0..len).map(|i| i as u8).collect()
    }

    #[test]
    fn the_pty_is_raw_and_delivers_the_payload_byte_for_byte() {
        // Raw mode is what this pins: a cooked tty turns LF into CR LF and
        // swallows ^C, ^D and ^S, and the comparison below would fail.
        let payload = all_bytes(1 << 18);
        let total = 1 << 17;
        let mut seen = Vec::new();
        let t = through_pty(&payload, total, |c| {
            seen.extend_from_slice(c);
            Ok(c.len())
        })
        .unwrap();
        assert_eq!(t.bytes, seen.len() as u64);
        assert!(t.bytes >= total, "{t:?}");
        assert!(t.bytes < total + CHUNK as u64, "{t:?}");
        assert!(t.mib_per_s > 0.0, "{t:?}");
        assert_eq!(seen, payload[..seen.len()], "the bytes were changed");
    }

    #[test]
    fn the_writer_repeats_the_payload_past_its_length() {
        let payload = all_bytes(100_003);
        let total = 250_000;
        let mut seen = Vec::new();
        through_pty(&payload, total, |c| {
            seen.extend_from_slice(c);
            Ok(c.len())
        })
        .unwrap();
        let expected: Vec<u8> = payload.iter().cycle().take(seen.len()).copied().collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn a_sink_that_fails_ends_the_run() {
        let payload = all_bytes(1 << 18);
        let e = through_pty(&payload, 1 << 17, |_c| {
            Err(Error::Python {
                call: "parse".into(),
            })
        })
        .unwrap_err();
        assert!(matches!(e, Error::Python { .. }), "{e:?}");
    }
}
