//! What one step of a window drag costs the presenter.
//!
//! The owner reports that holding a drag freezes the window until the
//! drag stops. `pump` is not the suspect -- the waker and the display
//! link are both on `NSRunLoopCommonModes`, so they run inside the modal
//! tracking loop a drag puts the run loop into. What a drag step *does*
//! is replace the surfaces, and this prices that: `SURFACES` fresh
//! `IOSurface`s and a Metal target over each, at the sizes a drag walks
//! through.
//!
//! A drag delivers a step about every 8 ms. Anything near that is a
//! pipeline that cannot keep up by construction, and the freeze is
//! arithmetic rather than a race.
#![cfg(target_os = "macos")]

use baud_font::Px;
use baud_present_macos::{Presenter, SurfaceSpace};
use std::time::Instant;

#[test]
fn a_drag_step_is_priced() {
    let mut p = Presenter::with_gpu(Px::of(2800), Px::of(1800), SurfaceSpace::Srgb)
        .expect("a presenter with Metal; this test cannot skip itself");
    // The drag's shape: in and back out, two pixels a step of the backing
    // scale, which is what a pointer moving at an ordinary speed produces.
    let mut worst = std::time::Duration::ZERO;
    let mut total = std::time::Duration::ZERO;
    let mut n = 0u32;
    for step in 0..40i64 {
        let d = if step < 20 {
            -step * 24
        } else {
            (step - 40) * 24
        };
        let w = u32::try_from(2800 + d).unwrap();
        let h = u32::try_from(1800 + d * 9 / 14).unwrap();
        let t = Instant::now();
        p.resize(Px::of(w), Px::of(h)).expect("surfaces");
        let dt = t.elapsed();
        worst = worst.max(dt);
        total += dt;
        n += 1;
    }
    let mean = total / n;
    eprintln!(
        "Presenter::resize over {n} drag steps at ~2800x1800: mean {:.2} ms, worst {:.2} ms",
        mean.as_secs_f64() * 1e3,
        worst.as_secs_f64() * 1e3
    );
    // **Not the number -- that would pin the machine -- but the order of
    // magnitude, which is the thing a reader of this path needs.** Three
    // `IOSurface`s and three Metal targets at 2800x1800 measured 0.15 ms
    // mean and 0.23 ms worst here, against a drag step every ~8 ms, which
    // is what rules the surfaces out as the freeze. A budget of 4 ms is
    // twenty-five times the measurement and half a drag step: it cannot
    // fire on this machine being slow, and it does fire if replacing the
    // surfaces ever becomes something a drag cannot afford.
    assert!(
        worst < std::time::Duration::from_millis(4),
        "a drag step's surfaces took {:.2} ms, which a drag cannot absorb",
        worst.as_secs_f64() * 1e3
    );
}
