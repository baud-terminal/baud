//! std-side harness. Feeds payloads through a real PTY so the numbers include
//! the kernel tty layer a terminal cannot avoid.
mod payload;

use std::time::Instant;
use baud_core::*;

enum P { Streaming, Tui }
#[derive(Clone,Copy,PartialEq)] enum Tr { Pty, Pipe }

fn run_once_t(store: &mut Storage, mode: OpMode, which: &P, total: usize, avx2: bool, tr: Tr)
    -> (f64, u64, u64, u64)
{
    unsafe {
        let (mut m, mut s): (libc::c_int, libc::c_int) = (0, 0);
        if tr == Tr::Pty {
            if libc::openpty(&mut m, &mut s, std::ptr::null_mut(),
                             std::ptr::null_mut(), std::ptr::null_mut()) < 0 {
                panic!("openpty");
            }
            let mut tio: libc::termios = std::mem::zeroed();
            libc::tcgetattr(s, &mut tio);
            libc::cfmakeraw(&mut tio);
            libc::tcsetattr(s, libc::TCSANOW, &tio);
        } else {
            let mut fds = [0 as libc::c_int; 2];
            if libc::pipe(fds.as_mut_ptr()) < 0 { panic!("pipe"); }
            m = fds[0]; s = fds[1];
            libc::fcntl(m, libc::F_SETPIPE_SZ, 1 << 20);
        }

        let is_tui = matches!(which, P::Tui);
        let pid = libc::fork();
        if pid == 0 {
            libc::close(m);
            let mut seq = 0u64; let mut sent = 0usize;
            while sent < total {
                let pl = if is_tui { payload::tui(1<<19, &mut seq) }
                         else      { payload::streaming(1<<20, &mut seq) };
                let mut o = 0usize;
                while o < pl.len() {
                    let w = libc::write(s, pl.as_ptr().add(o) as *const _, pl.len()-o);
                    if w <= 0 { libc::close(s); libc::_exit(0); }
                    o += w as usize;
                }
                sent += pl.len();
            }
            libc::close(s); libc::_exit(0);
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
            if r <= 0 { break; }
            let n = Filled::from_read(r as usize).len();
            if avx2 { t.feed_avx2(base, n) } else { t.feed(base, n) }
            t.commit_read(n);
            got += n.index();
            if got >= total { break; }
        }
        let dt = t0.elapsed().as_secs_f64();
        libc::kill(pid, 9);
        let mut st: libc::c_int = 0;
        libc::waitpid(pid, &mut st, 0);
        libc::close(m);
        (got as f64/dt/1_048_576.0, t.promotions.count(), t.tmpl_hits.count(), t.tmpl_builds.count())
    }
}

fn run_once(store: &mut Storage, mode: OpMode, which: &P, total: usize, avx2: bool)
    -> (f64,u64,u64,u64) { run_once_t(store, mode, which, total, avx2, Tr::Pty) }

fn bench_t(store: &mut Storage, label: &str, mode: OpMode, which: &P,
           total: usize, reps: usize, avx2: bool, tr: Tr) {
    let mut v = Vec::with_capacity(reps);
    for _ in 0..reps { v.push(run_once_t(store, mode, which, total, avx2, tr).0); }
    v.sort_by(|a,b| a.partial_cmp(b).unwrap());
    println!("  {:<34} {:7.1}  [{:5.1}-{:5.1}]", label, v[reps/2], v[0], v[reps-1]);
}

fn bench(store: &mut Storage, label: &str, mode: OpMode, which: &P,
         total: usize, reps: usize, avx2: bool) {
    let mut v = Vec::with_capacity(reps);
    let (mut promo, mut hits, mut builds) = (0u64, 0u64, 0u64);
    for _ in 0..reps {
        let (mbs, p, h, b) = run_once(store, mode, which, total, avx2);
        v.push(mbs); promo = p; hits = h; builds = b;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    print!("  {:<32} {:7.1}  [{:5.1}-{:5.1}]", label, v[reps / 2], v[0], v[reps - 1]);
    print!("   styles {} interned / {} hits ({:.1}%)", builds, hits,
           100.0 * hits as f64 / (hits + builds).max(1) as f64);
    if promo > 0 { print!("   promotions {}", promo); }
    println!();
}

fn main() {
    let total = 48usize << 20;
    let reps = 9;
    let avx2 = is_x86_feature_detected!("avx2");
    let mut store: Box<Storage> = Storage::boxed();

    println!("baud-core (no_std core, std harness) — real PTY, {} MB/rep, median of {}",
             total>>20, reps);
    println!("scan path: {}\n", if avx2 {"AVX2"} else {"portable"});

    println!("  {:<32} {:>7}  {:>13}", "streaming text", "median", "[min - max]");
    bench(&mut store,"spans, zero-copy",       OpMode::Runs,      &P::Streaming,total,reps,avx2);
    bench(&mut store,"viewport cells, computed",OpMode::Computed, &P::Streaming,total,reps,avx2);
    bench(&mut store,"viewport cells, templated",OpMode::Templated,&P::Streaming,total,reps,avx2);

    println!("\n  {:<32} {:>7}  {:>13}", "TUI redraw", "median", "[min - max]");
    bench(&mut store,"viewport cells, computed",OpMode::Computed, &P::Tui,total,reps,avx2);
    bench(&mut store,"viewport cells, templated",OpMode::Templated,&P::Tui,total,reps,avx2);
    bench(&mut store,"spans + promote hybrid",  OpMode::Runs,     &P::Tui,total,reps,avx2);

    println!("\n  transport: what does the PTY itself cost?");
    bench_t(&mut store,"spans, through a PTY",  OpMode::Runs,&P::Streaming,total,reps,avx2,Tr::Pty);
    bench_t(&mut store,"spans, through a pipe", OpMode::Runs,&P::Streaming,total,reps,avx2,Tr::Pipe);
    bench_t(&mut store,"TUI, through a PTY",    OpMode::Runs,&P::Tui,total,reps,avx2,Tr::Pty);
    bench_t(&mut store,"TUI, through a pipe",   OpMode::Runs,&P::Tui,total,reps,avx2,Tr::Pipe);
}
