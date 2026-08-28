use crate::*;
use baud_core::*;
use std::time::Instant;

/// Whether the AVX2 scanner exists on this build.
///
/// Off x86-64 the answer is not "the CPU lacks AVX2" but "there is no such
/// entry point to name": `Term::feed_avx2` is `#[cfg(target_arch = "x86_64")]`,
/// and `is_x86_feature_detected!` is itself x86-only. Hence a cfg rather than a
/// runtime probe. There is no NEON scanner yet, so on aarch64 this harness
/// measures the portable path and nothing else.
#[cfg(target_arch = "x86_64")]
pub(crate) fn avx2_available() -> bool {
    is_x86_feature_detected!("avx2")
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn avx2_available() -> bool {
    false
}

/// # Safety
/// When `avx2`, AVX2 must be available — the caller decided that once, at
/// startup, via [`avx2_available`].
#[cfg(target_arch = "x86_64")]
unsafe fn feed_dispatch(t: &mut Term, base: BytePos, n: ByteLen, avx2: bool) {
    if avx2 {
        unsafe { t.feed_avx2(base, n) }
    } else {
        t.feed(base, n)
    }
}

/// # Safety
/// Trivially satisfied: off x86-64 there is only the portable path.
#[cfg(not(target_arch = "x86_64"))]
unsafe fn feed_dispatch(t: &mut Term, base: BytePos, n: ByteLen, _avx2: bool) {
    t.feed(base, n)
}

/// How many bytes a repetition ingests.
///
/// A newtype because every signature here carries `total: usize` next to
/// `reps: usize`, and two adjacent same-typed integers is the shape this
/// project keeps typing its way out of -- swap them and 48 MB becomes 9 bytes
/// nine times, which would look like a very fast terminal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Bytes(pub(crate) usize);

impl Bytes {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

/// What a run cost, beyond how fast it was.
///
/// Named fields rather than a tuple that keeps growing: `(f64, f64, f64, f64,
/// f64, f64)` is six transposable numbers, and two of them are already a
/// ratio and its own subset. This is the shape the hit-rate counters were in
/// when they could be silently swapped.
#[derive(Clone, Copy, Default)]
pub(crate) struct Cost {
    /// Mean bytes per `read(2)`.
    pub(crate) read: f64,
    /// Rule #1: span-table bytes per byte ingested.
    pub(crate) span: f64,
    /// Grid bytes per byte ingested, and the share of it that is blanking.
    pub(crate) cell: f64,
    pub(crate) blank: f64,
    /// Share of input taking each of `feed_with`'s three exits.
    pub(crate) bulk: f64,
    pub(crate) seq: f64,
    pub(crate) slow: f64,
    /// Share of input that reaches the **UTF-8 decoder**, which is the only
    /// part a vectorised validator could speed up. Distinct from `slow`: most
    /// byte-at-a-time input is control sequences, not text.
    pub(crate) utf8: f64,
    /// Share taking `fast_utf8`: a whole multi-byte character decoded without
    /// a dispatch per byte. The fourth exit from `feed_with`.
    pub(crate) text: f64,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tr {
    Pty,
    Pipe,
    /// No transport at all: the payload is copied straight into the slot.
    ///
    /// Not a faster PTY -- a different quantity. There is no fork, no `read(2)`
    /// and no line discipline, so what is left is the parser and the grid.
    /// Subtracting it from the PTY row is what "the tty tax" means, and until
    /// this existed the two could not be separated.
    Memory,
}

pub(crate) fn run_once_t(
    store: &mut Storage,
    mode: OpMode,
    which: &P,
    total: Bytes,
    avx2: bool,
    tr: Tr,
) -> (f64, Cost, Stat, Stat) {
    // No transport: the payload is copied into the slot instead of being read
    // into it, so the slot is still `READ_CHUNK` and the parser sees the
    // chunking a PTY would give it. Inline rather than a second function
    // because a second `total` parameter and a second `as f64` are two more
    // untyped sites, and the budget is a floor.
    if tr == Tr::Memory {
        let mut seq = 0u64;
        let owned;
        let src: &[u8] = match which {
            P::Replay(rec) => rec,
            P::Tui => {
                owned = payload::tui(1 << 19, &mut seq);
                &owned
            }
            P::Streaming => {
                owned = payload::streaming(1 << 20, &mut seq);
                &owned
            }
        };
        let mut t = Term::new(store, mode);
        let (mut got, mut at) = (0usize, 0usize);
        let t0 = Instant::now();
        while got < total.index() {
            let mut slot = t.read_slot();
            let base = slot.base();
            let n = READ_CHUNK.min(src.len() - at);
            let f = slot.write(StreamByte::slice_from(&src[at..at + n]));
            // SAFETY: as in the transport loop -- `feed_dispatch` takes the
            // AVX2 entry point only when `avx2` says the CPU has it, which is
            // established once at startup.
            unsafe { feed_dispatch(&mut t, base, f.len(), avx2) };
            t.commit_read(f.len());
            got += f.len().index();
            at += n;
            if at == src.len() {
                at = 0;
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        return (
            got as f64 / dt / 1_048_576.0,
            cost_of(&t),
            t.tmpl_hits,
            t.tmpl_builds,
        );
    }
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
            // Linux only. The 1 MiB pipe buffer is why the pipe transport reaches
            // 8155 MB/s in README.md; there is no equivalent knob on Darwin, whose
            // default is ~64 KiB. The pipe row measured off Linux is therefore a
            // different quantity, not a slower version of the same one — the banner
            // in main() says so rather than letting the number pass as comparable.
            #[cfg(target_os = "linux")]
            libc::fcntl(m, libc::F_SETPIPE_SZ, 1 << 20);
        }

        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut seq = 0u64;
            let mut sent = 0usize;
            while sent < total.index() {
                // A recording is written from the static slice rather than
                // regenerated: it is a fixed capture, so "fresh per write" has
                // nothing to freshen. Repetition is the cost, and the banner
                // says so.
                let owned;
                let pl: &[u8] = match which {
                    P::Replay(rec) => rec,
                    P::Tui => {
                        owned = payload::tui(1 << 19, &mut seq);
                        &owned
                    }
                    P::Streaming => {
                        owned = payload::streaming(1 << 20, &mut seq);
                        &owned
                    }
                };
                let mut o = 0usize;
                while o < pl.len() {
                    let w = libc::write(s, pl.as_ptr().add(o) as *const _, pl.len() - o);
                    if w <= 0 {
                        libc::close(s);
                        libc::_exit(0);
                    }
                    o += w as usize;
                }
                sent += pl.len();
            }
            libc::close(s);
            libc::_exit(0);
        }
        libc::close(s);

        let mut t = Term::new(store, mode);
        let mut got = 0usize;
        let t0 = Instant::now();
        loop {
            let mut slot = t.read_slot();
            let base = slot.base();
            let (ptr, cap) = slot.as_syscall_buf();
            let r = libc::read(m, ptr as *mut _, cap);
            if r <= 0 {
                break;
            }
            let n = Filled::from_read(r as usize).len();
            feed_dispatch(&mut t, base, n, avx2);
            t.commit_read(n);
            got += n.index();
            if got >= total.index() {
                break;
            }
        }
        let dt = t0.elapsed().as_secs_f64();
        libc::kill(pid, 9);
        let mut st: libc::c_int = 0;
        libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        (
            got as f64 / dt / 1_048_576.0,
            cost_of(&t),
            t.tmpl_hits,
            t.tmpl_builds,
        )
    }
}

/// Everything a run cost, read off the counters in one place.
fn cost_of(t: &Term) -> Cost {
    let (cell, blank) = t.cell_amplification();
    let (bulk, seq, slow, text) = t.path_split();
    Cost {
        read: t.mean_read(),
        span: t.span_amplification(),
        cell,
        blank,
        bulk,
        seq,
        slow,
        text,
        utf8: t.utf8_share(),
    }
}

/// The transport with no parser behind it: read and discard.
///
/// The ceiling every other row is measured against. If a row sits at it, the
/// number is describing the transport and the process feeding it, not baud --
/// which is a thing a benchmark can be wrong about for a long time without
/// anything looking odd.
///
/// Returns throughput, the number of `read(2)` calls, and the bytes they
/// returned, because the read *size* is half the answer: a transport handing
/// back 4 KiB at a time costs a syscall per 4 KiB however fast the copy is.
pub(crate) fn null_ceiling(which: &P, total: Bytes, tr: Tr) -> (f64, u64, u64) {
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
            let mut seq = 0u64;
            let mut sent = 0usize;
            while sent < total.index() {
                let owned;
                let pl: &[u8] = match which {
                    P::Replay(rec) => rec,
                    P::Tui => {
                        owned = payload::tui(1 << 19, &mut seq);
                        &owned
                    }
                    P::Streaming => {
                        owned = payload::streaming(1 << 20, &mut seq);
                        &owned
                    }
                };
                let mut o = 0usize;
                while o < pl.len() {
                    let w = libc::write(s, pl.as_ptr().add(o) as *const _, pl.len() - o);
                    if w <= 0 {
                        libc::close(s);
                        libc::_exit(0);
                    }
                    o += w as usize;
                }
                sent += pl.len();
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

pub(crate) fn run_once(
    store: &mut Storage,
    mode: OpMode,
    which: &P,
    total: Bytes,
    avx2: bool,
) -> (f64, Cost, Stat, Stat) {
    run_once_t(store, mode, which, total, avx2, Tr::Pty)
}
