//! **Text contrast: DirectWrite's grayscale curve, compiled.**
//!
//! A glyph is rasterised once, as exact coverage, and the coverage `A` is
//! bent by a curve that depends on the text colour alone before it is
//! blended in encoded sRGB. This is the grayscale rule Windows Terminal ports
//! from DirectWrite (`dwrite_helpers.hlsl`, read for note on text quality):
//!
//! * `k = saturate(4 (0.75 - dot(fg, (0.30, 0.59, 0.11))))` -- darker text
//!   gets an "enhanced contrast" boost, light text none;
//! * `A1 = A (k + 1) / (A k + 1)`;
//! * `f = dot(fg, (0.25, 0.5, 0.25))`;
//! * `A2 = A1 + A1 (1 - A1) ((g0 f + g1) A1 + (g2 f + g3))`, with the
//!   gamma-1.8 ratios `g`.
//!
//! `fg` is the encoded sRGB colour in [0, 1]. The curve needs no
//! background, which glass hides, and it is the same on every system: baud
//! draws it.
//!
//! **It is a lookup, never a calculation per pixel.** The colour enters only
//! through `k` and `f`, so the table is one row per quantised `(k, f)` and
//! 256 coverages a row, built by the compiler: 128 x 64 rows, 2 MB, of which
//! a screen touches the rows of the colours on it. A colour's row is chosen
//! once per rendition, where the renderer resolves the rendition; a pixel
//! reads `CURVES[row][A]`. 128 steps of `k` by 64 of `f` is what keeps every
//! colour and coverage within one code of the exact formula -- 64 by 64 left
//! two -- and `tests/contrast.rs` holds it there.

/// Steps in `k`, the contrast boost, and in `f`, the gamma weight: 128 by
/// 64 keeps every colour within one code, where 64 by 64 left two.
pub const K_STEPS: usize = 128;
pub const F_STEPS: usize = 64;

/// Rows in [`CURVES`].
pub const ROWS: usize = K_STEPS * F_STEPS;

/// The gamma-1.8 ratios DirectWrite's alpha correction is fitted with.
const G: [f64; 4] = [0.148054421, -0.894594550, 1.47590804, -0.324668258];

/// Which curve a colour takes: `k`'s step times [`STEPS`] plus `f`'s.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct CurveRow(u16);

impl CurveRow {
    /// The row a pixel of coverage alone takes: `k = 0` and `f = 1`, white
    /// text -- the row a caller with no colour to hand should not need.
    pub const WHITE: CurveRow = CurveRow((F_STEPS - 1) as u16);

    /// The row text in the encoded sRGB colour `(r, g, b)` takes.
    pub fn of(r: u8, g: u8, b: u8) -> CurveRow {
        let (r, g, b) = (
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        );
        let k = k_of(r, g, b);
        let f = f_of(r, g, b);
        CurveRow(step(k, K_STEPS) * F_STEPS as u16 + step(f, F_STEPS))
    }

    /// The row as the index a shader multiplies by 256.
    pub fn get(self) -> u16 {
        self.0
    }

    /// The row a raw index names, for a test walking every row.
    pub fn at(index: u16) -> Option<CurveRow> {
        (usize::from(index) < ROWS).then_some(CurveRow(index))
    }

    /// `A` bent by this row's curve.
    pub fn apply(self, a: u8) -> u8 {
        CURVES[usize::from(self.0) * 256 + usize::from(a)]
    }
}

fn k_of(r: f64, g: f64, b: f64) -> f64 {
    (4.0 * (0.75 - (0.30 * r + 0.59 * g + 0.11 * b))).clamp(0.0, 1.0)
}

fn f_of(r: f64, g: f64, b: f64) -> f64 {
    0.25 * r + 0.5 * g + 0.25 * b
}

/// A value in [0, 1] to its nearest of `steps` steps.
fn step(v: f64, steps: usize) -> u16 {
    ((v * (steps - 1) as f64) + 0.5) as u16
}

/// The exact curve: coverage `a` in [0, 1] for contrast `k` and gamma
/// weight `f`, clamped to [0, 1]. What the table is built from and what the
/// test holds the table to.
pub fn exact(a: f64, k: f64, f: f64) -> f64 {
    let a1 = a * (k + 1.0) / (a * k + 1.0);
    let a2 = a1 + a1 * (1.0 - a1) * ((G[0] * f + G[1]) * a1 + (G[2] * f + G[3]));
    a2.clamp(0.0, 1.0)
}

/// The exact curve for an encoded sRGB text colour, for a test comparing a
/// colour's row against the formula it approximates.
pub fn exact_for(a: f64, r: u8, g: u8, b: u8) -> f64 {
    let (r, g, b) = (
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    );
    exact(a, k_of(r, g, b), f_of(r, g, b))
}

/// Every row's curve, flat: `CURVES[row * 256 + A]`. Built at compile time
/// by `build.rs`, which spells the formula `exact` spells; the test holds
/// the two to each other.
pub static CURVES: [u8; ROWS * 256] = *include_bytes!(concat!(env!("OUT_DIR"), "/contrast.bin"));
