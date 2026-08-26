//! std-side harness. Feeds payloads through a real PTY so the numbers include
//! the kernel tty layer a terminal cannot avoid.
mod payload;

use baud_core::*;
use std::time::Instant;

enum P {
    Streaming,
    Tui,
}
/// Whether the AVX2 scanner exists on this build.
///
/// Off x86-64 the answer is not "the CPU lacks AVX2" but "there is no such
/// entry point to name": `Term::feed_avx2` is `#[cfg(target_arch = "x86_64")]`,
/// and `is_x86_feature_detected!` is itself x86-only. Hence a cfg rather than a
/// runtime probe. There is no NEON scanner yet, so on aarch64 this harness
/// measures the portable path and nothing else.
#[cfg(target_arch = "x86_64")]
fn avx2_available() -> bool {
    is_x86_feature_detected!("avx2")
}
#[cfg(not(target_arch = "x86_64"))]
fn avx2_available() -> bool {
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
enum Tr {
    Pty,
    Pipe,
}

fn run_once_t(
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

        let is_tui = matches!(which, P::Tui);
        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut seq = 0u64;
            let mut sent = 0usize;
            while sent < total {
                let pl = if is_tui {
                    payload::tui(1 << 19, &mut seq)
                } else {
                    payload::streaming(1 << 20, &mut seq)
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

fn run_once(
    store: &mut Storage,
    mode: OpMode,
    which: &P,
    total: usize,
    avx2: bool,
) -> (f64, u64, u64, u64) {
    run_once_t(store, mode, which, total, avx2, Tr::Pty)
}

/// Eight parameters because eight things vary between rows of the table this
/// prints, and a config struct would put them behind a name instead of at the
/// call site. The audit gate already caps how many may be raw integers.
#[allow(clippy::too_many_arguments)]
fn bench_t(
    store: &mut Storage,
    label: &str,
    mode: OpMode,
    which: &P,
    total: usize,
    reps: usize,
    avx2: bool,
    tr: Tr,
) {
    let mut v = Vec::with_capacity(reps);
    for _ in 0..reps {
        v.push(run_once_t(store, mode, which, total, avx2, tr).0);
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "  {:<34} {:7.1}  [{:5.1}-{:5.1}]",
        label,
        v[reps / 2],
        v[0],
        v[reps - 1]
    );
}

fn bench(
    store: &mut Storage,
    label: &str,
    mode: OpMode,
    which: &P,
    total: usize,
    reps: usize,
    avx2: bool,
) {
    let mut v = Vec::with_capacity(reps);
    let (mut promo, mut hits, mut builds) = (0u64, 0u64, 0u64);
    for _ in 0..reps {
        let (mbs, p, h, b) = run_once(store, mode, which, total, avx2);
        v.push(mbs);
        promo = p;
        hits = h;
        builds = b;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    print!(
        "  {:<32} {:7.1}  [{:5.1}-{:5.1}]",
        label,
        v[reps / 2],
        v[0],
        v[reps - 1]
    );
    print!(
        "   styles {} interned / {} hits ({:.1}%)",
        builds,
        hits,
        100.0 * hits as f64 / (hits + builds).max(1) as f64
    );
    if promo > 0 {
        print!("   promotions {}", promo);
    }
    println!();
}

fn main() {
    let total = 48usize << 20;
    let reps = 9;
    let avx2 = avx2_available();
    let mut store: Box<Storage> = Storage::boxed();

    println!(
        "baud-core (no_std core, std harness) — real PTY, {} MB/rep, median of {}",
        total >> 20,
        reps
    );
    println!(
        "scan path: {}",
        if avx2 {
            "AVX2"
        } else if cfg!(target_arch = "x86_64") {
            "portable (AVX2 not detected)"
        } else {
            "portable (no SIMD scanner on this target)"
        }
    );
    // CLAUDE.md's first measurement failure is comparing across machines. The
    // tty layer is the thing being measured and it is not the same layer here:
    // Darwin has no n_tty, so no N_TTY_BUF_SIZE cap on read(2) and no 1 MiB
    // pipe buffer. Nothing below is comparable to MEASUREMENTS.md off Linux.
    if !cfg!(target_os = "linux") {
        println!(
            "WARNING: {} is not Linux. Different tty layer, no N_TTY_BUF_SIZE cap,\n\
             no F_SETPIPE_SZ. These numbers are NOT comparable to MEASUREMENTS.md.",
            std::env::consts::OS
        );
    }
    println!();

    println!(
        "  {:<32} {:>7}  {:>13}",
        "streaming text", "median", "[min - max]"
    );
    bench(
        &mut store,
        "spans, zero-copy",
        OpMode::Runs,
        &P::Streaming,
        total,
        reps,
        avx2,
    );
    bench(
        &mut store,
        "viewport cells, computed",
        OpMode::Computed,
        &P::Streaming,
        total,
        reps,
        avx2,
    );
    bench(
        &mut store,
        "viewport cells, templated",
        OpMode::Templated,
        &P::Streaming,
        total,
        reps,
        avx2,
    );

    println!(
        "\n  {:<32} {:>7}  {:>13}",
        "TUI redraw", "median", "[min - max]"
    );
    bench(
        &mut store,
        "viewport cells, computed",
        OpMode::Computed,
        &P::Tui,
        total,
        reps,
        avx2,
    );
    bench(
        &mut store,
        "viewport cells, templated",
        OpMode::Templated,
        &P::Tui,
        total,
        reps,
        avx2,
    );
    bench(
        &mut store,
        "spans + promote hybrid",
        OpMode::Runs,
        &P::Tui,
        total,
        reps,
        avx2,
    );

    println!("\n  transport: what does the PTY itself cost?");
    bench_t(
        &mut store,
        "spans, through a PTY",
        OpMode::Runs,
        &P::Streaming,
        total,
        reps,
        avx2,
        Tr::Pty,
    );
    bench_t(
        &mut store,
        "spans, through a pipe",
        OpMode::Runs,
        &P::Streaming,
        total,
        reps,
        avx2,
        Tr::Pipe,
    );
    bench_t(
        &mut store,
        "TUI, through a PTY",
        OpMode::Runs,
        &P::Tui,
        total,
        reps,
        avx2,
        Tr::Pty,
    );
    bench_t(
        &mut store,
        "TUI, through a pipe",
        OpMode::Runs,
        &P::Tui,
        total,
        reps,
        avx2,
        Tr::Pipe,
    );
}
