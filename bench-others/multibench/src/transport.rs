use std::time::Instant;

/// Feed `sink` from memory, in the same 4096-byte chunks, with no PTY at all.
///
/// The transport ceiling has never been separated from the parser for baud.
/// MEASUREMENTS.md records the tty tax for *alacritty* (-30% on streaming) and
/// nothing equivalent for baud, so it has never been established whether the
/// numbers in these tables are parser-bound or transport-bound. If every core
/// lands at the same figure through a PTY and they separate from memory, the
/// PTY is the constraint and no amount of parser work moves the table.
pub(crate) fn through_memory(payload: &[u8], total: usize, mut sink: impl FnMut(&[u8])) -> f64 {
    let t0 = Instant::now();
    let mut got = 0usize;
    while got < total {
        let mut o = 0usize;
        while o < payload.len() && got < total {
            let n = CHUNK.min(payload.len() - o).min(total - got);
            sink(&payload[o..o + n]);
            o += n;
            got += n;
        }
    }
    got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0
}

/// Which transport `run_one` drives. `TRANSPORT=memory` selects the in-memory
/// path; anything else keeps the real PTY, which stays the default because it
/// is what a terminal actually faces.
pub(crate) fn from_memory() -> bool {
    std::env::var("TRANSPORT")
        .map(|v| v == "memory")
        .unwrap_or(false)
}

/// Dispatch to whichever transport is selected. Same sink, same chunk size, so
/// the only difference between the two is the kernel.
pub(crate) fn drive(payload: &[u8], total: usize, sink: impl FnMut(&[u8])) -> f64 {
    if from_memory() {
        through_memory(payload, total, sink)
    } else {
        through_pty(payload, total, sink)
    }
}

pub(crate) const CHUNK: usize = 4096;

/// Fork a writer on a PTY slave, hand the master fd to `sink` 4096 bytes at a time.
pub(crate) fn through_pty(payload: &[u8], total: usize, mut sink: impl FnMut(&[u8])) -> f64 {
    unsafe {
        let (mut m, mut s) = (0 as libc::c_int, 0 as libc::c_int);
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        let mut t: libc::termios = std::mem::zeroed();
        libc::tcgetattr(s, &mut t);
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(s, libc::TCSANOW, &t);
        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut sent = 0usize;
            while sent < total {
                let mut o = 0usize;
                while o < payload.len() {
                    let w = libc::write(s, payload.as_ptr().add(o) as *const _, payload.len() - o);
                    if w <= 0 {
                        libc::close(s);
                        libc::_exit(0);
                    }
                    o += w as usize;
                }
                sent += payload.len();
            }
            libc::close(s);
            libc::_exit(0);
        }
        libc::close(s);
        let mut buf = vec![0u8; 4096];
        let mut got = 0usize;
        let t0 = Instant::now();
        loop {
            let r = libc::read(m, buf.as_mut_ptr() as *mut _, 4096);
            if r <= 0 {
                break;
            }
            sink(&buf[..r as usize]);
            got += r as usize;
            if got >= total {
                break;
            }
        }
        let v = got as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0;
        libc::kill(pid, 9);
        let mut st = 0;
        libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        v
    }
}
