//! `surface_copy`: what every frame costs a window that gave the GPU up.
//! From the give-up on, each frame the window draws is copied whole into
//! a surface on the CPU, premultiplied on the way, as Core Animation reads
//! an `IOSurface` (`Presenter::copy_changed` with `Changed::Whole`, the
//! cost when the whole grid churns; a frame that changes fewer rows copies
//! fewer). Timed here at the app's default window, 960x600 points, and a
//! 14-inch MacBook Pro's whole screen, 1512x982 points, both at a backing
//! scale of 2, the frame filling the surface.
//!
//! Each round copies [`FRAMES`] frames into three surfaces in turn, as the
//! presenter rotates them, the frames alternating between two patterns so
//! no copy repeats the one before; every round is printed, the first to be
//! discarded, with the median, least and most microseconds a copy, and the
//! sizes alternate within a round. Asserts that every copy was taken.
//!
//! Usage: `surface_copy [rounds]`, five by default. macOS only.

#[cfg(target_os = "macos")]
fn main() {
    baud_bench::warm::refuse_while_warm();
    use baud_font::Px;
    use baud_present_macos::{Changed, Presented, Presenter};
    use baud_render::Argb;
    use macos_liquid_glass::drawable::ColorSpace;
    use std::time::{Duration, Instant};

    /// Frames copied a size a round.
    const FRAMES: usize = 120;
    /// The two windows, in pixels: the default and a whole screen.
    const SIZES: [(u32, u32); 2] = [(1920, 1200), (3024, 1964)];

    let rounds = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5_usize);
    let clear = Argb(0x8020_2020);
    let frames: Vec<[Vec<u32>; 2]> = SIZES
        .iter()
        .map(|&(w, h)| {
            let n = usize::try_from(w * h).expect("a frame");
            // Straight-alpha pixels, glass and opaque text, every channel
            // varied, so the premultiply has work on every pixel.
            let pattern = |seed: u32| -> Vec<u32> {
                (0..n)
                    .map(|i| {
                        let i = u32::try_from(i).unwrap_or(0).wrapping_add(seed);
                        let alpha = if i % 7 == 0 { 0xff } else { 0xb0 };
                        (alpha << 24) | (i.wrapping_mul(2_654_435_761) & 0x00ff_ffff)
                    })
                    .collect()
            };
            [pattern(0), pattern(1)]
        })
        .collect();
    let mut presenters: Vec<Presenter> = SIZES
        .iter()
        .map(|&(w, h)| Presenter::new(Px::of(w), Px::of(h), ColorSpace::Srgb).expect("surfaces"))
        .collect();
    println!("round size frames median_us min_us max_us");
    for round in 0..rounds {
        for (k, &(w, h)) in SIZES.iter().enumerate() {
            let p = &mut presenters[k];
            let mut took: Vec<Duration> = Vec::with_capacity(FRAMES);
            for f in 0..FRAMES {
                let frame = &frames[k][f % 2];
                let t = Instant::now();
                let shown = p.copy_changed(frame, Px::of(w), clear, Changed::Whole);
                took.push(t.elapsed());
                assert_eq!(shown, Presented::Yes, "the copy was refused");
            }
            took.sort();
            let us = |d: Duration| d.as_secs_f64() * 1e6;
            println!(
                "{round} {w}x{h} {FRAMES} {:.1} {:.1} {:.1}",
                us(took[FRAMES / 2]),
                us(took[0]),
                us(took[FRAMES - 1])
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    baud_bench::warm::refuse_while_warm();
    eprintln!("surface_copy copies into IOSurfaces and runs on macOS only");
}
