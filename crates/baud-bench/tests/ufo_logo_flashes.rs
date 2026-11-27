//! **The UFO logo's flashes, counted on the files, the repeat included**
//! (`docs/assets/demo/ufo-logo-{dark,light}.gif`, or under `BAUD_UFO_FILES` the
//! ones `tools/baud-pictures logo --ufo --out` wrote there; WCAG 2.3.1,
//! `[[source: wcag22]]`), by [`baud_bench::flash::Census`], as
//! `readme_logo_flashes` counts the plasma logo's: each file decoded by
//! baud's own GIF decoder, every frame held for its delay at 100 samples a
//! second, at 2 pixels a point, as a page shows its 840 pixels at 420, and
//! fed twice over, so every one-second window across the join is counted.
//!
//! **Both pass, general and red, by the area and by the strict reading.**
//! WCAG 2.3.1 passes content with no more than three flashes in any one
//! second *or* flashes below the general and red flash thresholds. A few
//! pixels the saucer crosses flash more than three times in a second -- at
//! most 11 on the dark page, 5 on the light -- over an area of 44.25 and
//! 5.5 of the threshold's 21,824, so both pass by the second half, as the
//! README's plasma logo does. The file's counts are held at what it measured, so a regenerated logo
//! that flashes more is seen. The control: the same frames at a quarter of
//! their delays count more flashes at a pixel, so the census sees the
//! motion.

use baud_bench::flash::{Census, Flashes, Outcome, Rate, Report, Scale};
use baud_domain::gif::Gif;
use baud_font::Px;

/// The files, each with the most flashes at a pixel in a second and the
/// most strict area it measured.
const FILES: [(&str, (Flashes, f64)); 2] = [
    ("ufo-logo-dark.gif", (Flashes(MOST_DARK), STRICT_DARK)),
    ("ufo-logo-light.gif", (Flashes(MOST_LIGHT), STRICT_LIGHT)),
];

/// What the files `baud-pictures logo --ufo` makes measured: the most
/// flashes at a pixel in a second, and the strict reading's area, of the
/// 21,824 the threshold is.
const MOST_DARK: u32 = 11;
const STRICT_DARK: f64 = 336.5;
const MOST_LIGHT: u32 = 5;
const STRICT_LIGHT: f64 = 286.0;

/// Samples a second: a GIF's delays are hundredths.
const RATE: u32 = 100;

/// A file of the UFO logo: the committed one, or under `BAUD_UFO_FILES`
/// the one `baud-pictures logo --ufo --out` wrote there.
fn ufo_file(name: &str) -> String {
    match std::env::var("BAUD_UFO_FILES") {
        Ok(dir) => format!("{dir}/{name}"),
        Err(_) => format!(
            "{}/../../docs/assets/demo/{name}",
            env!("CARGO_MANIFEST_DIR")
        ),
    }
}

/// The census of `path`'s frames, each held `k` of its hundredths in four
/// (4 for the file's own timing), fed twice over, as the repeat shows them.
fn count(path: &str, quarters: u32) -> Report {
    let data = std::fs::read(ufo_file(path)).unwrap();
    let gif = Gif::parse(&data).unwrap();
    assert_eq!(gif.plays(), 0, "{path} repeats for ever");
    let size = gif.size();
    let mut census = Census::new(
        Px::of(size.width),
        Px::of(size.height),
        Scale(2),
        Rate(RATE),
    );
    let mut scratch = vec![0u8; gif.scratch_len()];
    let mut last = Vec::new();
    for _ in 0..2 {
        gif.decode(&mut scratch, |f| {
            last.clear();
            last.extend(
                f.rgba
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| u32::from_be_bytes([0xFF, p[0], p[1], p[2]])),
            );
            assert!(f.gap_ms % 10 == 0, "{path}: a delay of {} ms", f.gap_ms);
            for _ in 0..(f.gap_ms / 10 * quarters / 4).max(1) {
                census.frame(&last);
            }
            true
        })
        .unwrap();
    }
    census.finish()
}

#[test]
fn the_ufo_logos_are_below_the_flash_thresholds() {
    for (path, most) in FILES {
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
        assert!(r.general.flashes <= most.0, "{path}: {:?}", r.general);
        assert_eq!(r.red.flashes, Flashes(0), "{path}: {:?}", r.red);
        assert!(r.strict.0 <= most.1, "{path}: strict {}", r.strict.0);
        // The control: four times as fast.
        let c = count(path, 1);
        assert!(
            c.general.flashes > r.general.flashes,
            "{path}, the control: {:?}",
            c.general
        );
    }
}
