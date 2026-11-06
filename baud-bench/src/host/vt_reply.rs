//! Reads the terminal's reply to a query from the tty: everything that
//! arrives until the expected reply -- `ESC [ row ; col R` for the row and
//! column given, which the child put the cursor at first, so a reply to a
//! query the *recording* made (vim's asks for the cursor position and the
//! device attributes on every replay) cannot be mistaken for it -- or until
//! `timeout` seconds pass, and prints it as hex, or `none`. A shell `read`
//! cannot do this portably: bash 3.2's `read -t` discards what it had read
//! when it times out, and a killed `od` never flushes.
//!
//!     vt-reply [timeout=10] [row col]

use super::{Host, py};
use std::io::{self, Write};
use std::os::fd::RawFd;

/// How long one wait for input lasts before the clock is read again, in
/// microseconds.
const WAIT_US: libc::suseconds_t = 50_000;

/// Whether `fd` has input within [`WAIT_US`].
fn ready(fd: RawFd) -> io::Result<bool> {
    loop {
        // SAFETY: `set` is initialised by `FD_ZERO` before it is read and
        // outlives the call, as `wait` does; `fd` is the caller's.
        let n = unsafe {
            let mut set: libc::fd_set = core::mem::zeroed();
            libc::FD_ZERO(&raw mut set);
            libc::FD_SET(fd, &raw mut set);
            let mut wait = libc::timeval {
                tv_sec: 0,
                tv_usec: WAIT_US,
            };
            libc::select(
                fd + 1,
                &raw mut set,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut wait,
            )
        };
        if n >= 0 {
            return Ok(n > 0);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// Up to 256 bytes from `fd`, on the end of `buf`.
fn read_some(fd: RawFd, buf: &mut Vec<u8>) -> io::Result<()> {
    let mut chunk = [0u8; 256];
    loop {
        // SAFETY: `chunk` is a live buffer of the length passed.
        let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
        if let Ok(n) = usize::try_from(n) {
            buf.extend_from_slice(&chunk[..n]);
            return Ok(());
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// `buf` as lower-case hex, or `none`.
fn hex(buf: &[u8]) -> String {
    if buf.is_empty() {
        return "none".to_string();
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// Runs the program over `fd`; the return value is its exit code.
pub fn run(args: &[String], fd: RawFd, host: &mut dyn Host, out: &mut dyn Write) -> i32 {
    let timeout = match args.first() {
        None => Some(10.0),
        Some(t) => py::float(t),
    };
    let Some(timeout) = timeout else {
        eprintln!("usage: vt-reply [timeout=10] [row col]");
        return 1;
    };
    let want = match args {
        [_, row, col, ..] => format!("\x1b[{row};{col}R"),
        _ => "R".to_string(),
    };
    let want = want.as_bytes();
    let mut buf = Vec::new();
    let end = host.wall() + timeout;
    while host.wall() < end {
        let got = ready(fd).and_then(|ready| {
            if ready {
                read_some(fd, &mut buf)?;
            }
            Ok(ready)
        });
        match got {
            Ok(true) if buf.windows(want.len()).any(|w| w == want) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("vt-reply: {e}");
                return 1;
            }
        }
    }
    let _ = writeln!(out, "{}", hex(&buf));
    0
}
