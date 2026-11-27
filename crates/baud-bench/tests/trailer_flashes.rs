//! **The trailer GIF's flashes, counted on the file** (WCAG 2.3.1,
//! `[[source: wcag22]]`), by [`baud_bench::flash::Census`]. This is the gate
//! the demo runs on the trailer's take before keeping it
//! (`cargo run --example demo -- --trailer ... --record OUT.gif`).
//!
//! Each frame is decoded by baud's own GIF decoder and held for its delay at
//! 100 samples a second, the file played once, as the take filmed it.
//!
//! **At one pixel a point.** The README shows the trailer's 1164 pixels at 776,
//! which is 1.5 pixels to a CSS pixel. Counting at 1 makes WCAG's 341 x 256
//! window smaller, in the file's pixels, than the one the README shows, so
//! a flashing area reads larger than it is on the page. That errs toward
//! failing.
//!
//! **It passes** under the verdict's area and under the chained, strict one,
//! with every frame opaque. Like the session's census, it prints how many
//! times a pixel flashed in its worst second but does not hold that number:
//! WCAG 2.3.1 passes content with more than three flashes in a second when
//! the area they cover is under the threshold.

use baud_bench::flash::{Census, Outcome, Rate, Report, Scale};
use baud_domain::gif::Gif;
use baud_font::Px;

/// Samples a second: a GIF's delays are hundredths.
const RATE: u32 = 100;

/// **The census of frames**, each `(pixels, delay in ms)`, `w` x `h`
/// pixels at one pixel a point, each held for its delay at [`RATE`].
fn census(w: u32, h: u32, frames: impl IntoIterator<Item = (Vec<u32>, u32)>) -> Report {
    let mut c = Census::new(Px::of(w), Px::of(h), Scale(1), Rate(RATE));
    for (pixels, gap_ms) in frames {
        for _ in 0..(gap_ms / 10).max(1) {
            c.frame(&pixels);
        }
    }
    c.finish()
}

/// **The census of a GIF file**, played once.
fn census_of_gif(path: &str) -> Report {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let gif = Gif::parse(&data).unwrap_or_else(|e| panic!("{path}: {e:?}"));
    let size = gif.size();
    let mut c = Census::new(
        Px::of(size.width),
        Px::of(size.height),
        Scale(1),
        Rate(RATE),
    );
    let mut scratch = vec![0u8; gif.scratch_len()];
    let mut px = Vec::new();
    gif.decode(&mut scratch, |f| {
        px.clear();
        px.extend(
            f.rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| u32::from_be_bytes([0xFF, p[0], p[1], p[2]])),
        );
        for _ in 0..(f.gap_ms / 10).max(1) {
            c.frame(&px);
        }
        true
    })
    .unwrap_or_else(|e| panic!("{path}: {e:?}"));
    c.finish()
}

/// **The verdict on a count**: the area and the strict reading under the
/// threshold, and every frame opaque.
fn passes(r: &Report) -> Result<(), String> {
    if r.translucent.value() != 0 {
        return Err(format!(
            "{} translucent pixel-frames",
            r.translucent.value()
        ));
    }
    if r.outcome() != Outcome::Passes || r.strict_outcome() != Outcome::Passes {
        return Err(format!(
            "flashes more than three times a second over {} (strict {}) of the {} the \
             threshold is, in the second from {:.2} s",
            r.area.0, r.strict.0, r.threshold.0, r.second
        ));
    }
    Ok(())
}

/// **The gate tells a strobe from a still**: a 400x300 field going from
/// black to white and back every 50 ms for two seconds, ten flashes a
/// second over all of WCAG's window, fails. The same field holding white,
/// or going black to white once, passes. Perturbation: make `passes` ignore
/// the area's verdict and the strobe passes.
#[test]
fn the_gate_fails_a_strobe_and_passes_a_still() {
    let (w, h) = (400, 300);
    let field = |argb: u32| vec![argb; usize::try_from(w * h).unwrap()];
    let strobe = (0..40).map(|i| {
        let ground = match i % 2 {
            0 => 0xFF00_0000,
            _ => 0xFFFF_FFFF,
        };
        (field(ground), 50)
    });
    let r = census(w, h, strobe);
    assert!(passes(&r).is_err(), "a strobe passed: {r:?}");
    let still = (0..40).map(|_| (field(0xFFFF_FFFF), 50));
    assert_eq!(passes(&census(w, h, still)), Ok(()));
    let once = [(field(0xFF00_0000), 1000), (field(0xFFFF_FFFF), 1000)];
    assert_eq!(passes(&census(w, h, once)), Ok(()));
}

/// **The trailer passes**, the GIF that `BAUD_TRAILER_GIF` names, as the
/// demo's take of it runs this.
#[test]
#[ignore = "a gate: the demo's `--trailer --record` runs it on its GIF"]
fn the_trailer_is_below_the_flash_thresholds() {
    let path = std::env::var("BAUD_TRAILER_GIF").expect("BAUD_TRAILER_GIF is not set");
    let r = census_of_gif(&path);
    eprintln!(
        "{path}: {} frames; general {} red {} flashes at a pixel in its worst second; area {} \
         strict {} of {}",
        r.frames.0, r.general.flashes.0, r.red.flashes.0, r.area.0, r.strict.0, r.threshold.0
    );
    assert_eq!(passes(&r), Ok(()), "{path}: WCAG 2.3.1");
}
