//! A [`Phase`] moves a glyph's ink by exactly that fraction of a pixel.
//!
//! The renderer centres every glyph in its cell with one phase; the
//! arithmetic that makes it is here, apart from any cell.

use baud_domain::Codepoint;
use baud_font::{Bounds, Canvas, Face, Fit, Phase, Px, Rasterizer, Scale};

fn face_data() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .unwrap()
}

/// Where the left edge of `c`'s stem is on its middle row, in pixels right
/// of the glyph origin, drawn at `phase`. A straight vertical edge's pixel
/// holds exactly the area right of the edge, so the edge is that pixel's
/// right side less its coverage: exact to the mask's 1/255, where an ink
/// centroid is not (a box filter sampled at pixel centres moves by other
/// than the shift).
fn left_edge(face: &Face<'_>, c: char, scale: Scale, phase: Phase) -> f64 {
    let g = face.glyph(Codepoint::from_char(c)).unwrap();
    let mut b = Bounds::new();
    face.outline(g, &mut b);
    let fit = Fit::at(&b, scale, phase).unwrap();
    let mut scratch = vec![0f32; fit.extent.scratch().unwrap()];
    let canvas = Canvas::new(fit.extent, &mut scratch).unwrap();
    let mut r = Rasterizer::new(canvas, fit.transform);
    face.outline(g, &mut r);
    let mut cov = vec![0u8; fit.extent.cells().unwrap()];
    r.finish().coverage(&mut cov).unwrap();
    let w = usize::try_from(fit.extent.w.value()).unwrap();
    let h = usize::try_from(fit.extent.h.value()).unwrap();
    let row = &cov[(h / 2) * w..(h / 2 + 1) * w];
    let i = row.iter().position(|&a| a > 0).unwrap();
    f64::from(fit.left) + (i + 1) as f64 - f64::from(row[i]) / 255.0
}

/// JetBrains Mono's `M` is 600 units, 8.4 px at 14 px: in an 8 px cell the
/// phase is half of -0.4, and in a 9 px cell half of +0.6.
#[test]
fn centring_is_half_the_slack() {
    let data = face_data();
    let face = Face::parse(&data).unwrap();
    let scale = Scale::of(14.0, face.units_per_em());
    let m = face
        .default_advance(face.glyph(Codepoint::from_char('M')).unwrap())
        .unwrap();
    let narrow = Phase::centring(scale, m, Px::of(8)).unwrap();
    let wide = Phase::centring(scale, m, Px::of(9)).unwrap();
    let shift =
        |p: Phase| left_edge(&face, '|', scale, p) - left_edge(&face, '|', scale, Phase::ZERO);
    assert!((shift(narrow) + 0.2).abs() < 0.005, "{}", shift(narrow));
    assert!((shift(wide) - 0.3).abs() < 0.005, "{}", shift(wide));
    // A two-cell span takes more than a pixel; the whole pixels move the
    // mask and the stem lands where the fraction says.
    let two = Phase::centring(scale, m, Px::of(16)).unwrap();
    assert!((shift(two) - 3.8).abs() < 0.005, "{}", shift(two));
    assert_eq!(
        Phase::centring(scale, m, Px::of(20)),
        None,
        "11.6 px of slack in 20"
    );
}

/// A stem's edge moves by the phase, across a pixel boundary as well as
/// within one, and whichever way the rounding went.
#[test]
fn a_phase_moves_a_stem_by_exactly_that_much() {
    let data = face_data();
    let face = Face::parse(&data).unwrap();
    for px in [13.0, 14.0, 26.0] {
        let scale = Scale::of(px, face.units_per_em());
        let m = face
            .default_advance(face.glyph(Codepoint::from_char('M')).unwrap())
            .unwrap();
        let w = scale.px(600.0).round() as i64;
        for cell in [w - 1, w, w + 1] {
            let cell = u32::try_from(cell).unwrap();
            let Some(p) = Phase::centring(scale, m, Px::of(cell)) else {
                continue;
            };
            let want = (f64::from(cell) - f64::from(scale.px(600.0))) / 2.0;
            for c in ['|', 'I', '!'] {
                let moved = left_edge(&face, c, scale, p) - left_edge(&face, c, scale, Phase::ZERO);
                assert!(
                    (moved - want).abs() < 0.006,
                    "{c:?} at {px} px in a {cell} px cell moved {moved:.4}, not {want:.4}"
                );
            }
        }
    }
}
