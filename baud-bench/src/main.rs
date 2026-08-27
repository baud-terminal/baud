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
    /// A recording: real bytes from a real program, replayed verbatim.
    ///
    /// Every other payload here is generated fresh per write, because a child
    /// re-writing one buffer makes "all unique" content 97% duplicate. A
    /// recording cannot be freshened -- it is a fixed capture -- so filling a
    /// repetition means replaying it end to end. That is stated in the banner
    /// rather than hidden: caches and branch predictors see the same bytes
    /// again, which a live terminal never does.
    Replay(&'static [u8]),
}

/// The recordings, embedded so the benchmark has no runtime file dependency.
/// `tests/recordings/README.md` is where they came from.
const REC_BTOP: &[u8] = include_bytes!("../../baud-core/tests/recordings/btop.raw");
const REC_AQUA: &[u8] = include_bytes!("../../baud-core/tests/recordings/asciiquarium.raw");
const REC_VIM: &[u8] = include_bytes!("../../baud-core/tests/recordings/vim-unicode.raw");

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
    println!("\n  real capture, through a PTY — replayed, so warmer than life");
    for (name, rec) in [
        ("btop (truecolour, box drawing)", REC_BTOP),
        ("asciiquarium (curses, 16-colour)", REC_AQUA),
        ("vim (scroll regions, CJK/emoji)", REC_VIM),
    ] {
        bench_t(
            &mut store,
            name,
            OpMode::Runs,
            &P::Replay(rec),
            total,
            reps,
            avx2,
            Tr::Pty,
        );
    }

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
