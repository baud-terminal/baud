//! Colour spaces a theme file may name, converted to the sRGB numbers a
//! [`super::Scheme`] holds: the matrices are computed at compile time from
//! each space's own definition, and the transfer curves are evaluated here,
//! without a maths library (`no_std`).
//!
//! - **Display P3**: "the DCI P3 primaries, a D65 white point, and the
//!   sRGB transfer function" (`[[source: apple-cgcolorspace-displayp3]]`),
//!   the chromaticities (0.680, 0.320), (0.265, 0.690), (0.150, 0.060)
//!   (`[[source: css-color-4-20261009]]` 10.4); to sRGB through both
//!   spaces' D65 matrices, sRGB's from 10.2, so a P3 grey is the same grey.
//! - **Calibrated RGB**, macOS's generic RGB, "a device-independent RGB
//!   color space" (`[[source: apple-nscolorspace-genericrgb]]`): its only
//!   numbers are the system profile's D50 colorants, copied below as stored
//!   (`[[source: macos-generic-rgb-icc]]`), with gamma 1.8 -- what macOS's
//!   own conversion uses, where the profile stores the nearest u8Fixed8,
//!   1.80078125; to sRGB through the sRGB profile's D50
//!   colorants (`[[source: macos-srgb-icc]]`). An input outside [0, 1] "is
//!   interpreted as" the bound (`[[source: apple-nscolor-calibrated]]`).
//! - **sRGB**: the curve of `[[source: css-color-4-20261009]]` 10.2.
//!
//! A channel outside sRGB is clipped, as macOS clips it, and said. Held to
//! macOS's own `NSColor` conversion on a 7x7x7 grid
//! (`baud-domain/tests/nscolor-to-srgb.txt`): every P3 channel equal, and
//! every calibrated one but one that lands within 0.005 of a half step.

/// A 3x3 matrix, rows.
type M3 = [[f64; 3]; 3];

const fn mul(a: M3, b: M3) -> M3 {
    let mut out = [[0.0; 3]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
            j += 1;
        }
        i += 1;
    }
    out
}

const fn inverse(m: M3) -> M3 {
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    [
        [
            (e * i - f * h) / det,
            (c * h - b * i) / det,
            (b * f - c * e) / det,
        ],
        [
            (f * g - d * i) / det,
            (a * i - c * g) / det,
            (c * d - a * f) / det,
        ],
        [
            (d * h - e * g) / det,
            (b * g - a * h) / det,
            (a * e - b * d) / det,
        ],
    ]
}

/// A chromaticity `(x, y)` as `XYZ` with `Y = 1`.
const fn col((x, y): (f64, f64)) -> [f64; 3] {
    [x / y, 1.0, (1.0 - x - y) / y]
}

/// The RGB-to-XYZ matrix of three primaries and a white, each `(x, y)`.
const fn primaries(p: [(f64, f64); 3], w: (f64, f64)) -> M3 {
    let (r, g, b) = (col(p[0]), col(p[1]), col(p[2]));
    let m = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let white = col(w);
    let inv = inverse(m);
    let mut s = [0.0; 3];
    let mut i = 0;
    while i < 3 {
        s[i] = inv[i][0] * white[0] + inv[i][1] * white[1] + inv[i][2] * white[2];
        i += 1;
    }
    let mut out = m;
    let mut row = 0;
    while row < 3 {
        let mut c = 0;
        while c < 3 {
            out[row][c] *= s[c];
            c += 1;
        }
        row += 1;
    }
    out
}

const D65: (f64, f64) = (0.3127, 0.3290);
const SRGB_D65: M3 = primaries([(0.64, 0.33), (0.30, 0.60), (0.15, 0.06)], D65);
const P3_D65: M3 = primaries([(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)], D65);

/// `s15Fixed16` from an ICC tag, as the profile stores it.
const fn s15(raw: i32) -> f64 {
    raw as f64 / 65536.0
}

/// A profile's `rXYZ`, `gXYZ` and `bXYZ` tags as matrix columns.
const fn columns(r: [i32; 3], g: [i32; 3], b: [i32; 3]) -> M3 {
    [
        [s15(r[0]), s15(g[0]), s15(b[0])],
        [s15(r[1]), s15(g[1]), s15(b[1])],
        [s15(r[2]), s15(g[2]), s15(b[2])],
    ]
}

/// Generic RGB Profile.icc's colorants, as stored.
const GENERIC_D50: M3 = columns(
    [0x0000_744d, 0x0000_3dee, 0x0000_03d0],
    [0x0000_5a75, 0x0000_ac73, 0x0000_1734],
    [0x0000_281a, 0x0000_159f, 0x0000_b836],
);

/// sRGB Profile.icc's colorants, as stored.
const SRGB_D50: M3 = columns(
    [0x0000_6fa2, 0x0000_38f5, 0x0000_0390],
    [0x0000_6299, 0x0000_b785, 0x0000_18da],
    [0x0000_24a0, 0x0000_0f84, 0x0000_b6cf],
);

const P3_TO_SRGB: M3 = mul(inverse(SRGB_D65), P3_D65);
const CALIBRATED_TO_SRGB: M3 = mul(inverse(SRGB_D50), GENERIC_D50);

/// The natural logarithm, for `x > 0`: `x = m * 2^e` with `m` in
/// [sqrt(1/2), sqrt(2)), and `ln m` by the series of `atanh`.
fn ln(x: f64) -> f64 {
    let bits = x.to_bits();
    let mut e = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > core::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let (mut term, mut sum) = (s, 0.0);
    let mut k = 1.0;
    while k < 60.0 {
        sum += term / k;
        term *= s2;
        k += 2.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let e = e as f64;
    2.0 * sum + e * core::f64::consts::LN_2
}

/// `e^x`, for the range a transfer curve needs: `x = k ln 2 + r`, `e^r` by
/// its series, and `2^k` set in the exponent.
fn exp(x: f64) -> f64 {
    if x < -700.0 {
        return 0.0;
    }
    let k = (x / core::f64::consts::LN_2 + if x < 0.0 { -0.5 } else { 0.5 }) as i64;
    #[allow(clippy::cast_precision_loss)]
    let r = x - k as f64 * core::f64::consts::LN_2;
    let (mut term, mut sum) = (1.0, 1.0);
    let mut n = 1.0;
    while n < 30.0 {
        term *= r / n;
        sum += term;
        n += 1.0;
    }
    let scale = f64::from_bits(u64::try_from(k + 1023).unwrap_or(0) << 52);
    sum * scale
}

fn powf(x: f64, y: f64) -> f64 {
    if x <= 0.0 { 0.0 } else { exp(y * ln(x)) }
}

/// IEC 61966-2-1's decoding, to linear light.
fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        powf((v + 0.055) / 1.055, 2.4)
    }
}

/// IEC 61966-2-1's encoding, from linear light.
fn srgb_encode(l: f64) -> f64 {
    if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * powf(l, 1.0 / 2.4) - 0.055
    }
}

/// The colour spaces a theme file may name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Space {
    Srgb,
    DisplayP3,
    Calibrated,
}

/// Whether a converted colour fit sRGB.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    Inside,
    /// A channel fell outside [0, 1] and was clipped.
    Clipped,
}

/// The nearest 8-bit value of a channel in [0, 1] (rounded, not
/// truncated).
fn byte(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0) * 255.0 + 0.5;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = v as u8;
    b
}

/// Components in `space`, each nominally in [0, 1], to 8-bit sRGB.
pub fn to_srgb(space: Space, c: [f64; 3]) -> ([u8; 3], Fit) {
    let (m, lin) = match space {
        Space::Srgb => {
            let fit = if c.iter().all(|v| (0.0..=1.0).contains(v)) {
                Fit::Inside
            } else {
                Fit::Clipped
            };
            return (c.map(byte), fit);
        }
        Space::DisplayP3 => (P3_TO_SRGB, c.map(|v| srgb_decode(v.clamp(0.0, 1.0)))),
        Space::Calibrated => (CALIBRATED_TO_SRGB, c.map(|v| powf(v.clamp(0.0, 1.0), 1.8))),
    };
    let out = [0, 1, 2].map(|i| m[i][0] * lin[0] + m[i][1] * lin[1] + m[i][2] * lin[2]);
    // A hair outside from the arithmetic is not out of gamut.
    let fit = if out.iter().all(|v| (-1e-9..=1.0 + 1e-9).contains(v)) {
        Fit::Inside
    } else {
        Fit::Clipped
    };
    (out.map(|l| byte(srgb_encode(l.clamp(0.0, 1.0)))), fit)
}
