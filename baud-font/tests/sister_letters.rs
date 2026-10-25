//! Hebrew sister letters in baud's embedded font stay apart at text sizes.
//!
//! ז (zayin) and ד (dalet) are each a bar over one leg. What tells them
//! apart is where the leg stands under the bar: under the middle of zayin's
//! head, which passes over on both sides, and at the right end of dalet's
//! roof, which passes it by only a heel. As round 1 left them the two legs
//! sat 60-64 units apart under roofs of the same centre, and "זו" read "דו"
//! (`baud-app/fonts/glyphs/languages/hebrew/build/distinct.py`).

use baud_domain::Codepoint;
use baud_font::{Canvas, Extent, Face, Px, Rasterizer, Scale};

const STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];
const ZAYIN: char = '\u{05D6}';
const DALET: char = '\u{05D3}';
/// The text size the check is taken at, in pixels per em.
const PX: f32 = 14.0;
/// The canvas, square, in pixels: a 600-unit cell and a 630-unit letter at
/// 14 px are under 9 px, so the glyph sits well inside with the origin at
/// `LEFT`, `BASELINE`.
const SIDE: u32 = 24;
const LEFT: f32 = 6.0;
const BASELINE: f32 = 18.0;
/// The bands read, in font units: the roof's top (both letters' bars span
/// 548 or 517 to 630) and the leg below any neck (zayin's leg bends right
/// from about y 380 in Regular).
const ROOF: (f32, f32) = (570.0, 640.0);
const LEG: (f32, f32) = (40.0, 300.0);
/// How far apart the two legs must stand, in pixels at 14 px: one Regular
/// stem, 90 of 1000 units.
const APART: f32 = 90.0 * PX / 1000.0;

fn built(style: &str) -> Vec<u8> {
    let path = format!(
        "{}/../baud-app/fonts/built/{style}.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `c`'s coverage at `PX`, row-major on a `SIDE` square, each 0..=1.
fn coverage(face: &Face<'_>, c: char) -> Vec<f32> {
    let g = face
        .glyph(Codepoint::from_char(c))
        .unwrap_or_else(|| panic!("U+{:04X} is not mapped", u32::from(c)));
    let extent = Extent::new(Px::of(SIDE), Px::of(SIDE));
    let mut scratch = vec![0f32; extent.scratch().unwrap()];
    let canvas = Canvas::new(extent, &mut scratch).unwrap();
    let mut r = Rasterizer::new(
        canvas,
        Scale::of(PX, face.units_per_em()).placing(LEFT, BASELINE),
    );
    face.outline(g, &mut r);
    let mut cov = vec![0u8; extent.cells().unwrap()];
    r.finish().coverage(&mut cov).unwrap();
    cov.into_iter().map(|v| f32::from(v) / 255.0).collect()
}

/// The ink in the rows whose centres lie in `band` (font units above the
/// baseline): its mass and its centroid's x, in pixels.
fn band(cov: &[f32], band: (f32, f32)) -> (f32, f32) {
    let side = usize::try_from(SIDE).unwrap();
    let (mut mass, mut moment) = (0.0, 0.0);
    for (y, row) in cov.chunks(side).enumerate() {
        let units = (BASELINE - (y as f32 + 0.5)) / PX * 1000.0;
        if units < band.0 || units > band.1 {
            continue;
        }
        for (x, c) in row.iter().enumerate() {
            mass += c;
            moment += c * (x as f32 + 0.5);
        }
    }
    (mass, moment / mass)
}

/// Where the roof's ink centre sits right of the leg's, in pixels: about 0
/// for a leg under the middle of its bar, negative for one toward the right
/// end. A shear moves the roof against the leg by the same amount in both
/// letters, so the italics compare as the uprights do.
fn roof_over_leg(face: &Face<'_>, c: char) -> f32 {
    let cov = coverage(face, c);
    let total: f32 = cov.iter().sum();
    let (roof_mass, roof) = band(&cov, ROOF);
    let (leg_mass, leg) = band(&cov, LEG);
    // Ink-present controls: an unmapped or empty glyph, or a band that missed
    // the letter, cannot pass for a centroid.
    assert!(total > 10.0, "U+{:04X}: {total} px of ink", u32::from(c));
    assert!(
        roof_mass > 2.0,
        "U+{:04X}: roof band holds {roof_mass}",
        u32::from(c)
    );
    assert!(
        leg_mass > 2.0,
        "U+{:04X}: leg band holds {leg_mass}",
        u32::from(c)
    );
    roof - leg
}

/// **At 14 px, in each of the four embedded styles, zayin's leg and
/// dalet's stand at least one Regular stem (1.26 px) apart under their
/// roofs.** As round 1 left them they stood 0.80-0.92 px apart; round 5
/// moved dalet's leg to the roof's right end, keeping a 45-unit heel, and
/// they stand 1.41-1.51 px apart. The offset is read from the rasterised
/// coverage, so moving either glyph sideways changes nothing.
#[test]
fn zayin_and_dalet_legs_stand_apart_at_14_px() {
    let mut short = vec![];
    for style in STYLES {
        let data = built(style);
        let face = Face::parse(&data).unwrap();
        let apart = roof_over_leg(&face, ZAYIN) - roof_over_leg(&face, DALET);
        if apart < APART {
            short.push(format!("{style}: {apart:.2} px"));
        }
    }
    assert!(
        short.is_empty(),
        "zayin's and dalet's legs stand under {APART:.2} px apart: {short:?}"
    );
}
