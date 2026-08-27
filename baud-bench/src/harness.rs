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

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Tr {
    Pty,
    Pipe,
}

pub(crate) fn run_once_t(
    store: &mut Storage,
    mode: OpMode,
    which: &P,
    total: usize,
    avx2: bool,
    tr: Tr,
) -> (f64, u64, u64, u64) {
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
            while sent < total {
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
            if got >= total {
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
            t.promotions.count(),
            t.tmpl_hits.count(),
            t.tmpl_builds.count(),
        )
    }
}

pub(crate) fn run_once(
    store: &mut Storage,
    mode: OpMode,
    which: &P,
    total: usize,
    avx2: bool,
) -> (f64, u64, u64, u64) {
    run_once_t(store, mode, which, total, avx2, Tr::Pty)
}
