//! `reshape`: what one step of a window drag costs the presenter.
//!
//! A user reported that holding a drag froze the window until the drag
//! stopped. `pump` is not the suspect -- the waker and the display link are
//! both on `NSRunLoopCommonModes`, so they run inside the modal tracking loop
//! a drag puts the run loop into. What a drag step *does* is replace the
//! surfaces, and this prices that: fresh `IOSurface`s and a Metal target over
//! each, at the sizes a drag walks through -- in from ~2800x1800 and back out,
//! 24 pixels a step, which is what a pointer at an ordinary speed produces at
//! a backing scale of 2.
//!
//! A drag delivers a step about every 8 ms, so the number to read this
//! against is that period: a step near it is a pipeline that cannot keep up
//! by construction, and the freeze would be arithmetic rather than a race.
//! As this bench, one launch of five rounds on this box, the settled four
//! read **0.091-0.098 ms mean and 0.112-0.208 ms worst** -- forty times
//! under a drag step, which is what rules the surfaces out. (As the test it
//! replaced, under `cargo test --release` alone, five launches read 0.14-0.18
//! mean and 0.21-0.56 worst. Why the bench reads lower is **unattributed**:
//! both build under `[profile.release]`, LTO included, so it is not the
//! profile; a test runs on a libtest thread and a bench on the main thread,
//! they are different links, and the readings are one launch against five
//! taken in different sessions. Two instruments, and neither figure is
//! quoted as the other.)
//!
//! **This was a test with a 4 ms wall-clock bound, and it is a bench because
//! a wall-clock bound inside `cargo test` measures the machine's load.**
//! Inside the full parallel `devenv test` the same 40 steps read a 1.85 ms
//! mean and a 9.41 ms worst and failed the pre-push run -- the preemption of
//! a test binary sharing the cores with a hundred others, not the surfaces.
//! So a measurement prints, and the order-of-magnitude claim is reproduced
//! by running it rather than gated on how busy the machine happened to be.
//!
//! Every round is printed so the reader discards the first, the protocol
//! every bench here follows.
//!
//! Usage: `cargo bench -p baud-bench --bench reshape -- [rounds]`. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    baud_bench::warm::refuse_while_warm();
    // Imported here rather than at the top of the file: the bench is macOS
    // only, so a top-level `use` is unused elsewhere and `-D warnings` makes
    // that an error on the Linux lint.
    use baud_font::Px;
    use baud_present_macos::{Presenter, SurfaceSpace};
    use std::time::{Duration, Instant};

    /// Steps a drag: twenty in, twenty back out.
    const STEPS: u32 = 40;
    /// How far one step moves the width, in device pixels.
    const STRIDE: u32 = 24;
    /// The size the drag starts and ends at.
    const START: (u32, u32) = (2800, 1800);

    let rounds: u32 = baud_bench::args()
        .next()
        .and_then(|a| a.parse().ok())
        .unwrap_or(5);

    let mut p = Presenter::with_gpu(Px::of(START.0), Px::of(START.1), SurfaceSpace::Srgb)
        .expect("a presenter with Metal");

    println!(
        "reshape: {STEPS} drag steps a round from {}x{}, {rounds} rounds",
        START.0, START.1
    );
    println!("  round   mean ms   worst ms");
    for round in 0..rounds {
        let mut worst = Duration::ZERO;
        let mut total = Duration::ZERO;
        for step in 0..STEPS {
            // How far in the drag is at this step: 0, 1, ... 19, 20, 19 ... 1.
            let depth = if step < STEPS / 2 { step } else { STEPS - step };
            let w = START.0 - depth * STRIDE;
            // The height follows the width at the window's own aspect.
            let h = START.1 - depth * STRIDE * START.1 / START.0;
            let t = Instant::now();
            p.resize(Px::of(w), Px::of(h)).expect("surfaces");
            let dt = t.elapsed();
            worst = worst.max(dt);
            total += dt;
        }
        let mean = total / STEPS;
        println!(
            "  {round:>5}   {:>7.3}   {:>8.3}",
            mean.as_secs_f64() * 1e3,
            worst.as_secs_f64() * 1e3
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    baud_bench::warm::refuse_while_warm();
    eprintln!("reshape replaces IOSurfaces and Metal targets and runs on macOS only");
}
