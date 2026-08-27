//! std-side harness. Feeds payloads through a real PTY so the numbers include
//! the kernel tty layer a terminal cannot avoid.

mod drive;
mod graphemes;
mod harness;

use drive::*;
use harness::*;

mod payload;

use baud_core::*;

enum P {
    Streaming,
    Tui,
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

    graphemes::run(reps);
}
