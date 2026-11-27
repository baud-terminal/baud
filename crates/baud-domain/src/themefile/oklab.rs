//! `oklch(` and `lab(` colours in a kitty theme, drawn by these rules:
//!
//! - **Edge**: a number that is not finite, or L at or below 0, is black;
//!   L at or above 1 is white (CSS Color 4 14.2).
//! - **Grey**, chroma under 1e-5: every channel is L's sRGB encoding, L
//!   taken as linear light, as kitty draws a grey.
//! - **Inside sRGB**, the linear channels in [0, 1] before any clip: for
//!   OKLCH their encoding; for Lab its OKLCH drawn the same way, as kitty
//!   draws it.
//! - **Outside**: the chroma search, a bisection on chroma at constant L and
//!   hue that keeps a chroma whose colour is inside sRGB or within the JND
//!   of its clip (`[[source: css-color-4-20261009]]` 14.2.1, 20.3), without
//!   14.2.2's pre-test, flag and early return, as kitty's preview draws it.
//!
//! OKLab is Ottosson's (`[[source: ottosson-oklab]]`, his matrices and their
//! sums, public domain); Lab is CIE's inverse
//! (`[[source: wikipedia-cielab-1370652355]]`) with ASTM E308's D65 and the
//! sRGB matrix as Lindbloom publishes them
//! (`[[source: lindbloom-chromadapt]]`, `[[source: lindbloom-rgb-xyz-matrix]]`);
//! the sRGB curve is IEC 61966-2-1's (CSS Color 4 10.2).
//!
//! **The bytes are kitty's, on every value of
//! `crates/baud-domain/tests/kitty-colours.txt`** (`tools/scripts/kitty-colour-oracle.sh`
//! runs kitty's own C and Go on them). baud-domain has no maths library, so
//! the functions the conversions need are written here: a square root by
//! the hardware's correctly rounded one, `fmod` exactly, and `pow`, `cbrt`,
//! `sin`, `cos` and `atan2` by series. Against macOS's libm on 2,000,000
//! arguments each, in the ranges the conversions pass them, those are
//! within 2 units in the last place for `sin` and `cos`, 5 for `atan2`, 1
//! for `cbrt`, and 4 and 23 for `pow` by 1/2.4 and by 2.4 -- far inside
//! the half step of 1/255 a drawn byte rounds at, and the oracle holds the
//! bytes.

use core::f64::consts::{FRAC_PI_2, PI};

// ── The functions a libm would give. ──

fn sqrt(x: f64) -> f64 {
    core::f64::math::sqrt(x)
}

/// `x` rounded to an integer towards zero, for `|x| < 2^52`.
fn trunc(x: f64) -> f64 {
    if x.abs() < 4_503_599_627_370_496.0 {
        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let t = x as i64 as f64;
        t
    } else {
        x
    }
}

/// C's `round` and Go's `math.Round`: half away from zero.
fn round(x: f64) -> f64 {
    let t = trunc(x);
    let f = x - t;
    if f >= 0.5 {
        t + 1.0
    } else if f <= -0.5 {
        t - 1.0
    } else {
        t
    }
}

/// C's `fmod` and Go's `math.Mod`: `x - n y` for the integer `n` that
/// truncates `x / y`, exactly, by long division on the significands.
fn fmod(x: f64, y: f64) -> f64 {
    if !x.is_finite() || y.is_nan() || y == 0.0 {
        return f64::NAN;
    }
    if x.abs() < y.abs() {
        return x;
    }
    let split = |v: f64| {
        let bits = v.to_bits();
        let e = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
        let m = bits & 0x000f_ffff_ffff_ffff;
        if e == 0 {
            // Subnormal: normalise.
            let shift = i64::from(m.leading_zeros()) - 11;
            (m << shift, 1 - shift)
        } else {
            (m | 1 << 52, e)
        }
    };
    let (mut mx, mut ex) = split(x);
    let (my, ey) = split(y);
    while ex > ey {
        if mx >= my {
            mx -= my;
        }
        mx <<= 1;
        ex -= 1;
    }
    if mx >= my {
        mx -= my;
    }
    if mx == 0 {
        return 0.0 * x;
    }
    while mx < 1 << 52 {
        mx <<= 1;
        ex -= 1;
    }
    let bits = if ex > 0 {
        (mx & 0x000f_ffff_ffff_ffff) | u64::try_from(ex).unwrap_or(0) << 52
    } else {
        mx >> (1 - ex)
    };
    f64::from_bits(bits | (x.to_bits() & 1 << 63))
}

/// ln 2 in two parts, the first's low 21 bits zero so an integer below
/// 2^21 times it is exact, the second what is left.
const LN2_HI: f64 = f64::from_bits(0x3fe6_2e42_fee0_0000);
const LN2_LO: f64 = f64::from_bits(0x3dea_39ef_3579_3c76);
/// pi/2 likewise, the first's low 20 bits zero.
const PIO2_HI: f64 = f64::from_bits(0x3ff9_21fb_5440_0000);
const PIO2_LO: f64 = f64::from_bits(0x3dd0_b461_1a62_6331);

/// The natural logarithm of a positive, finite `x`: `x = m 2^e`, `m` in
/// [sqrt(1/2), sqrt(2)), `ln m` by the series of `atanh`.
fn ln(x: f64) -> f64 {
    let mut x = x;
    let mut e = 0i64;
    if x < f64::MIN_POSITIVE {
        x *= 18_014_398_509_481_984.0; // 2^54
        e -= 54;
    }
    let bits = x.to_bits();
    e += i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let mut m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    if m > core::f64::consts::SQRT_2 {
        m /= 2.0;
        e += 1;
    }
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut sum = 0.0;
    let mut k = 41.0;
    while k >= 1.0 {
        sum = sum * s2 + 1.0 / k;
        k -= 2.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let e = e as f64;
    2.0 * s * sum + e * core::f64::consts::LN_2
}

/// `e^x`: `x = k ln 2 + r`, `e^r` by its series, `2^k` exactly.
fn exp(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 709.8 {
        return f64::INFINITY;
    }
    if x < -745.2 {
        return 0.0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let k = round(x / core::f64::consts::LN_2) as i64;
    #[allow(clippy::cast_precision_loss)]
    let kf = k as f64;
    let r = (x - kf * LN2_HI) - kf * LN2_LO;
    let mut sum = 1.0;
    let mut n = 27.0;
    while n >= 1.0 {
        sum = 1.0 + sum * r / n;
        n -= 1.0;
    }
    // 2^k in two steps, so a k past the exponent's range still scales.
    let half = k / 2;
    let two = |p: i64| f64::from_bits(u64::try_from(p + 1023).unwrap_or(0) << 52);
    sum * two(half) * two(k - half)
}

/// `x^y` for `x >= 0` and `y > 0`: the sRGB curves' and the cube root's.
fn pow(x: f64, y: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY || x == 0.0 || x == 1.0 {
        return if x == 1.0 { 1.0 } else { x };
    }
    exp(y * ln(x))
}

/// The real cube root, by `pow` and a Newton step.
fn cbrt(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let a = x.abs();
    let mut r = pow(a, 1.0 / 3.0);
    r -= (r * r * r - a) / (3.0 * r * r);
    if x < 0.0 { -r } else { r }
}

/// `sin` and `cos` of `x`, `|x| < 2^20`: `x = k pi/2 + r`, `|r| <= pi/4`,
/// each by its series.
fn sin_cos(x: f64) -> (f64, f64) {
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    #[allow(clippy::cast_possible_truncation)]
    let k = round(x / FRAC_PI_2) as i64;
    #[allow(clippy::cast_precision_loss)]
    let kf = k as f64;
    let r = (x - kf * PIO2_HI) - kf * PIO2_LO;
    let r2 = r * r;
    // sin r = r (1 - r^2/(2*3) (1 - r^2/(4*5) (...))), and cos alike.
    let (mut s, mut c) = (1.0, 1.0);
    let mut n = 26.0;
    while n >= 2.0 {
        s = 1.0 - s * r2 / (n * (n + 1.0));
        c = 1.0 - c * r2 / ((n - 1.0) * n);
        n -= 2.0;
    }
    let s = r * s;
    match k.rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// `atan t` for `0 <= t <= 1`: halved twice, `atan t = 2 atan(t / (1 +
/// sqrt(1 + t^2)))`, to `|u| < 0.2`, then its series.
fn atan01(t: f64) -> f64 {
    let half = |t: f64| t / (1.0 + sqrt(1.0 + t * t));
    let u = half(half(t));
    let u2 = u * u;
    let mut sum = 0.0;
    let mut k = 35.0;
    while k >= 1.0 {
        sum = 1.0 / k - sum * u2;
        k -= 2.0;
    }
    4.0 * u * sum
}

/// C's `atan2` and Go's `math.Atan2`, signed zeros included.
fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    let negative_x = x < 0.0 || (x == 0.0 && x.is_sign_negative());
    if y == 0.0 {
        return if negative_x { PI.copysign(y) } else { y };
    }
    if x == 0.0 {
        return FRAC_PI_2.copysign(y);
    }
    let (ay, ax) = (y.abs(), x.abs());
    let a = if ay.is_infinite() || ax.is_infinite() {
        match (ay.is_infinite(), ax.is_infinite()) {
            (true, true) => PI / 4.0,
            (true, false) => FRAC_PI_2,
            _ => 0.0,
        }
    } else if ay <= ax {
        atan01(ay / ax)
    } else {
        FRAC_PI_2 - atan01(ax / ay)
    };
    let a = if negative_x { PI - a } else { a };
    a.copysign(y)
}

// ── The colour spaces. ──

/// sRGB's encoding of linear light (IEC 61966-2-1, as CSS Color 4 10.2
/// gives it): the straight segment at and below 0.0031308.
fn encode(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * pow(c, 1.0 / 2.4) - 0.055
    }
}

/// sRGB's decoding to linear light, the inverse of [`encode`]: the
/// straight segment at and below 0.04045.
fn decode(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        pow((c + 0.055) / 1.055, 2.4)
    }
}

/// OKLab to linear sRGB: Ottosson's second matrix, the cube, his first
/// matrix's inverse, each sum in his order.
fn oklab_to_linear([l, a, b]: [f64; 3]) -> [f64; 3] {
    let l_ = l + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
    let m_ = l - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
    let s_ = l - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
    let (l3, m3, s3) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.076_741_662_1 * l3 - 3.307_711_591_3 * m3 + 0.230_969_929_2 * s3,
        -1.268_438_004_6 * l3 + 2.609_757_401_1 * m3 - 0.341_319_396_5 * s3,
        -0.004_196_086_3 * l3 - 0.703_418_614_7 * m3 + 1.707_614_701_0 * s3,
    ]
}

/// Linear sRGB to OKLab: Ottosson's first matrix, the real cube root, his
/// second.
fn linear_to_oklab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let [l, m, s] = [
        0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b,
        0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b,
        0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b,
    ]
    .map(cbrt);
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

/// A hue in degrees and a chroma as OKLab's a and b.
fn polar(l: f64, c: f64, h: f64) -> [f64; 3] {
    let (s, co) = sin_cos(h * (PI / 180.0));
    [l, c * co, c * s]
}

/// OKLab as lightness, chroma and a hue in [0, 360).
fn lch([l, a, b]: [f64; 3]) -> (f64, f64, f64) {
    let h = 180.0 * atan2(b, a) / PI;
    (l, sqrt(a * a + b * b), if h < 0.0 { h + 360.0 } else { h })
}

/// CIE Lab to linear sRGB: CIE's inverse of f, with delta = 6/29, scaled by
/// D65's white, then the sRGB matrix.
fn lab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let f_inv = |t: f64| {
        let delta = 6.0 / 29.0;
        if t > delta {
            t * t * t
        } else {
            3.0 * delta * delta * (t - 4.0 / 29.0)
        }
    };
    let fy = (l + 16.0) / 116.0;
    let (x, y, z) = (
        0.950_47 * f_inv(a / 500.0 + fy),
        f_inv(fy),
        1.088_83 * f_inv(fy - b / 200.0),
    );
    [
        3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z,
        -0.969_266 * x + 1.876_010_8 * y + 0.041_556 * z,
        0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z,
    ]
}

fn inside(c: &[f64; 3]) -> bool {
    c.iter().all(|v| (0.0..=1.0).contains(v))
}

/// The edge and grey cases; `None` past them.
fn edge(l: f64, c: f64, h: f64) -> Option<[f64; 3]> {
    if !(l.is_finite() && c.is_finite() && h.is_finite()) || l <= 0.0 {
        Some([0.0; 3])
    } else if l >= 1.0 {
        Some([1.0; 3])
    } else if c < 0.000_01 {
        Some([encode(l); 3])
    } else {
        None
    }
}

/// One just noticeable difference in deltaEOK (CSS Color 4 14.2.1).
const JND: f64 = 0.02;
/// Where the bisection stops (CSS Color 4 14.2.2's epsilon).
const CONVERGED: f64 = 0.0001;

/// The chroma search: the highest chroma found, at constant L and hue,
/// whose colour is inside sRGB or within the JND of its own clip, drawn
/// clipped. The distance is taken between the colour encoded to sRGB and
/// the same with each channel clipped to [0, 1], both decoded back to
/// OKLab. The edge and grey tests apply to these numbers too.
fn search(l: f64, c: f64, h: f64) -> [f64; 3] {
    if let Some(drawn) = edge(l, c, h) {
        return drawn;
    }
    let srgb = |chroma: f64| oklab_to_linear(polar(l, chroma, h)).map(encode);
    let oklab = |c: [f64; 3]| linear_to_oklab(c.map(decode));
    let clip = |c: [f64; 3]| c.map(|v| v.clamp(0.0, 1.0));
    let full = srgb(c);
    if inside(&full) {
        return full;
    }
    let (mut low, mut high) = (0.0, c);
    while high - low > CONVERGED {
        let mid = (low + high) * 0.5;
        let at = srgb(mid);
        let (a, b) = (oklab(at), oklab(clip(at)));
        let (dl, da, db) = (a[0] - b[0], a[1] - b[1], a[2] - b[2]);
        if inside(&at) || sqrt(dl * dl + da * da + db * db) < JND {
            low = mid;
        } else {
            high = mid;
        }
    }
    clip(srgb(low))
}

/// A channel in [0, 1] as a byte: `round(f * 255)`, half away from zero.
fn byte(f: f64) -> u8 {
    let v = round(f * 255.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = v.clamp(0.0, 255.0) as u8;
    b
}

/// An `oklch(L C h)` colour, a `%` already divided out: L in [0, 1] with a
/// NaN as 1, a chroma below 0 as 0, the hue `h mod 360` (its sign kept);
/// then the case.
pub(super) fn oklch(l: f64, c: f64, h: f64) -> [u8; 3] {
    let l = if l.is_nan() { 1.0 } else { l.clamp(0.0, 1.0) };
    let c = if c < 0.0 { 0.0 } else { c };
    let h = fmod(h, 360.0);
    let drawn = edge(l, c, h).unwrap_or_else(|| {
        let linear = oklab_to_linear(polar(l, c, h));
        if inside(&linear) {
            linear.map(encode)
        } else {
            search(l, c, if h < 0.0 { h + 360.0 } else { h })
        }
    });
    drawn.map(byte)
}

/// A `lab(L a b)` colour: L in [0, 100] with a NaN as 100, then the case.
/// The edge and grey tests read OKLab from the linear channels with those
/// below 0 (or not a number) raised to 0, and a colour inside sRGB is
/// drawn from that OKLCH, not from its linear channels, which would draw
/// about 6.5 in a million values a step off kitty. The search reads OKLab
/// from the channels as they are.
pub(super) fn lab(l: f64, a: f64, b: f64) -> [u8; 3] {
    let l = if l.is_nan() {
        100.0
    } else {
        l.clamp(0.0, 100.0)
    };
    let linear = lab_to_linear(l, a, b);
    let (ok_l, c, h) = lch(linear_to_oklab(linear.map(|v| v.max(0.0))));
    let drawn = edge(ok_l, c, h).unwrap_or_else(|| {
        if inside(&linear) {
            oklab_to_linear(polar(ok_l, c, h)).map(|v| encode(v.clamp(0.0, 1.0)))
        } else {
            let (l, c, h) = lch(linear_to_oklab(linear));
            search(l, c, h)
        }
    });
    drawn.map(byte)
}
