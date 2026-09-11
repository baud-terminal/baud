//! **The transport with no parser behind it: read and discard.**
//!
//! The ceiling every PTY row is measured against, and the reason this is its
//! own binary rather than a mode of a benchmark: it must not have a parser in
//! it. `CLAUDE.md` records what it settled -- *"'the PTY costs 45%' was
//! indistinguishable from 'the process feeding it cannot keep up' until
//! something measured the transport with nothing behind it"* -- and the answer
//! was that a consumer which only reads reaches **198.6 MB/s through a PTY and
//! 6144 through a pipe**, because the tty returns 1024 bytes per `read(2)`
//! where the pipe returns 4096.
//!
//! It lived in `baud-bench`'s span-model harness, which went with `baud-core2`.
//! Nothing here touches a core, so nothing here had to be ported: the whole
//! file is `libc`, and the one constant it borrows is the slot size the real
//! loop uses, so the comparison stays like for like.
//!
//! ```text
//! transport utf8/btop.raw 48
//! ```

use baud_core::READ_CHUNK;
use std::time::Instant;

/// How many bytes a run consumes.
///
/// A newtype because the signature carries it next to a fd and a count, and
/// two adjacent same-typed integers is the shape this project keeps typing its
/// way out of -- swap them and 48 MB becomes 48 bytes, which would look like a
/// very fast transport.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Bytes(usize);

impl Bytes {
    fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tr {
    Pty,
    Pipe,
}

impl Tr {
    fn name(self) -> &'static str {
        match self {
            Tr::Pty => "pty",
            Tr::Pipe => "pipe",
        }
    }
}

/// Fork a writer, read until `total`, and report throughput with the read size.
///
/// The read *size* is half the answer: a transport handing back 4 KiB at a time
/// costs a syscall per 4 KiB however fast the copy is, and the 4x difference
/// between the tty and the pipe on Darwin is the whole of the gap between them.
fn null_ceiling(payload: &[u8], total: Bytes, tr: Tr) -> (f64, u64, u64) {
    unsafe {
        let (mut m, mut s): (libc::c_int, libc::c_int) = (0, 0);
        if tr == Tr::Pty {
            if libc::openpty(
                &mut m,
                &mut s,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) < 0
            {
                panic!("openpty");
            }
            let mut tio: libc::termios = std::mem::zeroed();
            libc::tcgetattr(s, &mut tio);
            libc::cfmakeraw(&mut tio);
            libc::tcsetattr(s, libc::TCSANOW, &tio);
        } else {
            let mut fds = [0 as libc::c_int; 2];
            if libc::pipe(fds.as_mut_ptr()) < 0 {
                panic!("pipe");
            }
            m = fds[0];
            s = fds[1];
            #[cfg(target_os = "linux")]
            libc::fcntl(m, libc::F_SETPIPE_SZ, 1 << 20);
        }

        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut sent = 0usize;
            while sent < total.index() {
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

        // Same slot size the real loop uses, so the comparison is like for like.
        let mut buf = [0u8; READ_CHUNK];
        let (mut got, mut reads) = (0usize, 0u64);
        let t0 = Instant::now();
        loop {
            let r = libc::read(m, buf.as_mut_ptr() as *mut _, READ_CHUNK);
            if r <= 0 {
                break;
            }
            reads += 1;
            got += r as usize;
            if got >= total.index() {
                break;
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        libc::kill(pid, 9);
        let mut st: libc::c_int = 0;
        libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        (got as f64 / dt / 1_048_576.0, reads, got as u64)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "utf8/btop.raw".into());
    let mb: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(48);

    let p = format!("{}/../tests/recordings/{which}", env!("CARGO_MANIFEST_DIR"));
    let payload = std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"));
    let total = Bytes(mb << 20);

    println!(
        "transport ceiling -- read and discard, no parser: {which}, {mb} MB, slot {READ_CHUNK} B"
    );
    for tr in [Tr::Pty, Tr::Pipe] {
        let (mbs, reads, bytes) = null_ceiling(&payload, total, tr);
        println!(
            "  {:<5} {:9.1} MB/s   {:>9} reads   {:6.0} B/read",
            tr.name(),
            mbs,
            reads,
            bytes as f64 / reads.max(1) as f64,
        );
    }
}
