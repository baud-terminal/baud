//! Hebrew sister letters in baud's embedded font stay apart at text sizes.
//!
//! ז (zayin) and ד (dalet) are each a bar over one leg. What tells them
//! apart is where the leg stands under the bar: under the middle of zayin's
//! head, which passes over on both sides, and at the right end of dalet's
//! roof, which passes it by only a heel. As round 1 left them the two legs
//! sat 60-64 units apart under roofs of the same centre, and "זו" read "דו"
//! (`baud-app/fonts/glyphs/languages/hebrew/build/distinct.py`).
//!
//! ד (dalet) and ר (resh) are each a roof ending in a leg at the right.
//! What tells them apart is the corner: dalet's square, with a heel, "so
//! that it doesn't look like a resh", and resh's round. Moving dalet's leg
//! to its roof's right end brought it beside resh's, so the legs no longer
//! tell the two apart and the corner is what is held.

use baud_domain::Codepoint;
use baud_font::{Canvas, Extent, Face, Px, Rasterizer, Scale, Sink};

const STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];
const ZAYIN: char = '\u{05D6}';
const DALET: char = '\u{05D3}';
const RESH: char = '\u{05E8}';
/// The tangent of each style's lean: the italics are the uprights sheared
/// 9 degrees right about half the letter height, y 315 (note AC, 4c).
const LEAN: [f32; 4] = [0.0, 0.0, 0.158_384_4, 0.158_384_4];
const SHEAR_Y0: f32 = 315.0;
/// The top of a Hebrew letter, in font units.
const TOP: f32 = 630.0;
/// The side of the corner square read, in font units: under either
/// weight's roof (Regular 82 thick, Bold 113) and narrower than its leg.
const CORNER: f32 = 60.0;
/// A square corner fills at least this much of its square at 14 px...
const SQUARE: f32 = 0.5;
/// ...and a round one at most this much.
const ROUND: f32 = 0.25;
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

/// Every on-curve point of an outline, in font units.
#[derive(Default)]
struct Points(Vec<(f32, f32)>);

impl Sink for Points {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn quad_to(&mut self, _: f32, _: f32, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn close(&mut self) {}
}

/// How much of the `CORNER` square at `c`'s outer top-right corner its
/// ink fills at `PX`, 0 to 1. The square's right side is the letter's
/// rightmost extent and its top the letter's top, with the lean taken out
/// and put back row by row, so an italic reads the corner it shears.
fn corner_fill(face: &Face<'_>, c: char, lean: f32) -> f32 {
    let g = face
        .glyph(Codepoint::from_char(c))
        .unwrap_or_else(|| panic!("U+{:04X} is not mapped", u32::from(c)));
    let mut pts = Points::default();
    face.outline(g, &mut pts);
    let right = pts
        .0
        .iter()
        .map(|&(x, y)| x - lean * (y - SHEAR_Y0))
        .fold(f32::NEG_INFINITY, f32::max);
    let cov = coverage(face, c);
    let side = usize::try_from(SIDE).unwrap();
    let units = 1000.0 / PX;
    // Each pixel is split 8 by 8; a sample whose point, unsheared, lies in
    // the square counts its pixel's coverage at a 64th of the pixel.
    let (mut ink, mut area) = (0.0f32, 0.0f32);
    for (row, line) in cov.chunks(side).enumerate() {
        for (col, &value) in line.iter().enumerate() {
            let mut inside = 0u32;
            for sy in 0..8 {
                for sx in 0..8 {
                    let fx = col as f32 + (sx as f32 + 0.5) / 8.0;
                    let fy = row as f32 + (sy as f32 + 0.5) / 8.0;
                    let y = (BASELINE - fy) * units;
                    let x = (fx - LEFT) * units - lean * (y - SHEAR_Y0);
                    if (right - CORNER..=right).contains(&x) && (TOP - CORNER..=TOP).contains(&y) {
                        inside += 1;
                    }
                }
            }
            let part = inside as f32 / 64.0;
            ink += part * value;
            area += part;
        }
    }
    // The square spans 0.84 px a side at 14 px, so about 0.7 px of area: a
    // square that missed the canvas cannot pass for an empty corner.
    assert!(
        area > 0.5,
        "U+{:04X}: the corner square covers {area} px",
        u32::from(c)
    );
    ink / area
}

/// **At 14 px, in each of the four embedded styles, dalet's corner is
/// square and resh's round.** Dalet's roof meets its leg at a square corner
/// with a 45-unit heel; resh's roof curves into its leg. At 14 px dalet's
/// corner square is 0.64-0.74 full and resh's 0.00-0.09, so a dalet whose
/// corner rounded, or a resh whose corner squared, fails. The legs' places
/// cannot tell the two apart: they stand 1.24 px (Regular) and 1.18 px
/// (Bold) apart by `roof_over_leg`, under the 1.26 px the zayin and dalet
/// test asks, because both legs end their roofs.
#[test]
fn dalet_corner_is_square_and_resh_corner_round_at_14_px() {
    let mut wrong = vec![];
    for (style, lean) in STYLES.into_iter().zip(LEAN) {
        let data = built(style);
        let face = Face::parse(&data).unwrap();
        let dalet = corner_fill(&face, DALET, lean);
        let resh = corner_fill(&face, RESH, lean);
        if dalet < SQUARE || resh > ROUND {
            wrong.push(format!("{style}: dalet {dalet:.2}, resh {resh:.2}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "dalet's corner fills under {SQUARE} or resh's over {ROUND}: {wrong:?}"
    );
}
