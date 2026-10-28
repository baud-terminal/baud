//! **The compiled contrast curves are DirectWrite's formula, within one
//! code, for every colour and coverage** -- the bound the table's size was
//! chosen for. And the curve is not the identity for light text: `k` is zero
//! there, but the gamma term still bends it, so a renderer that skipped the
//! curve for light text would draw something other than what was chosen.

use baud_domain::contrast::{CURVES, CurveRow, ROWS, exact_for};

#[test]
fn every_colour_and_coverage_is_within_one_code() {
    let mut worst = 0i32;
    for r in (0..=255u8).step_by(5) {
        for g in (0..=255u8).step_by(5) {
            for b in (0..=255u8).step_by(5) {
                let row = CurveRow::of(r, g, b);
                for a in 0..=255u8 {
                    let want = (exact_for(f64::from(a) / 255.0, r, g, b) * 255.0).round() as i32;
                    let got = i32::from(row.apply(a));
                    worst = worst.max((got - want).abs());
                }
            }
        }
    }
    assert!(worst <= 1, "worst {worst} codes");
}

#[test]
fn every_row_runs_from_zero_to_full() {
    assert_eq!(CURVES.len(), ROWS * 256);
    for (i, row) in CURVES.chunks_exact(256).enumerate() {
        assert_eq!(row[0], 0, "row {i}: no coverage stays none");
        assert_eq!(row[255], 255, "row {i}: full coverage stays full");
        assert!(row.windows(2).all(|w| w[0] <= w[1]), "row {i}: monotonic");
    }
}

#[test]
fn the_curve_bends_light_text_and_depends_on_the_colour() {
    let light = CurveRow::of(0xe8, 0xe8, 0xe8);
    let dark = CurveRow::of(0x1c, 0x1c, 0x1c);
    let differ = (0..=255u8)
        .filter(|&a| light.apply(a) != dark.apply(a))
        .count();
    assert!(differ > 100, "the curve depends on the colour ({differ})");
    // The gamma term thickens light text and thins dark text at half
    // coverage, by more than dark text's contrast boost gives back.
    assert!(light.apply(128) > dark.apply(128));
}
