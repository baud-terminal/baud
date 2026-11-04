//! **The README logo's flashes, counted on the committed files**
//! (`assets/readme/logo-{dark,light}.gif`, `scripts/make-readme-logo.py`;
//! WCAG 2.3.1, `[[source: wcag22]]`), by [`baud_bench::flash::Census`]:
//! each file decoded by baud's own GIF decoder, every frame held for its
//! delay at 100 samples a second, the file's own grid, at 2 pixels a
//! point, as the README shows its 840 pixels at 420, and the last frame
//! held 3 s more, as a file played once rests on it.
//!
//! **Both pass, and at no pixel does the blink flash more than three times
//! in a second**, the rule's first half alone, which is stricter than the
//! text: the block is far under the area limit, so the verdict alone
//! cannot fail on it, and the count is what holds it. The control: the
//! same frames at a quarter of their delays do count more than three
//! flashes at a pixel, so the census sees the block.

use baud_bench::flash::{Census, Flashes, Outcome, Rate, Report, Scale};
use baud_domain::gif::Gif;
use baud_font::Px;

const FILES: [&str; 2] = [
    "../assets/readme/logo-dark.gif",
    "../assets/readme/logo-light.gif",
];

/// Samples a second: a GIF's delays are hundredths.
const RATE: u32 = 100;

/// The census of `path`'s frames, each held `k` of its hundredths in four
/// (4 for the file's own timing), then the last 3 s more.
fn count(path: &str, quarters: u32) -> Report {
    let data = std::fs::read(format!("{}/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let gif = Gif::parse(&data).unwrap();
    assert_eq!(gif.plays(), 1, "{path} plays once");
    let size = gif.size();
    let mut census = Census::new(
        Px::of(size.width),
        Px::of(size.height),
        Scale(2),
        Rate(RATE),
    );
    let mut scratch = vec![0u8; gif.scratch_len()];
    let mut last = Vec::new();
    gif.decode(&mut scratch, |f| {
        last.clear();
        last.extend(
            f.rgba
                .chunks_exact(4)
                .map(|p| u32::from_be_bytes([0xFF, p[0], p[1], p[2]])),
        );
        assert!(f.gap_ms % 10 == 0, "{path}: a delay of {} ms", f.gap_ms);
        for _ in 0..(f.gap_ms / 10 * quarters / 4).max(1) {
            census.frame(&last);
        }
        true
    })
    .unwrap();
    for _ in 0..3 * RATE {
        census.frame(&last);
    }
    census.finish()
}

#[test]
fn the_readme_logos_are_below_the_flash_thresholds() {
    for path in FILES {
        let r = count(path, 4);
        eprintln!(
            "{path}: general {} red {} red-encoded {} flashes at a pixel in a second, area {} strict {} of {}",
            r.general.flashes.0,
            r.red.flashes.0,
            r.red_encoded.flashes.0,
            r.area.0,
            r.strict.0,
            r.threshold.0
        );
        assert_eq!(r.translucent.value(), 0, "{path}: an opaque ground");
        assert_eq!(r.outcome(), Outcome::Passes, "{path}: {r:?}");
        assert_eq!(r.strict_outcome(), Outcome::Passes, "{path}: {r:?}");
        assert!(r.general.flashes <= Flashes(3), "{path}: {:?}", r.general);
        assert!(r.red.flashes <= Flashes(3), "{path}: {:?}", r.red);
        // The control: four times as fast.
        let c = count(path, 1);
        assert!(
            c.general.flashes > Flashes(3),
            "{path}, the control: {:?}",
            c.general
        );
    }
}
