//! **Math that gives the same bits on every OS.** Rust's `f32::exp`, `sin`
//! and the rest call the platform's libm -- Apple's on macOS, glibc's on
//! Linux -- and the two disagree: with std's math in place of these, 12 of
//! the 21 light digests pinned on macOS
//! (`every_effect_lights_the_same_bits_on_every_os`) differ on Linux
//! aarch64. A trail's light and its corners are built from them, so a
//! frame drawn from std's would differ between the two systems where one
//! ulp moves a pixel's rounding.
//!
//! These are evaluated in `f64` with IEEE 754 basic operations alone --
//! add, subtract, multiply, divide, square root, `floor`, `round` and bit
//! moves, each correctly rounded by the standard, so each gives the same
//! bits on every conforming machine -- by range reduction and a series,
//! and an `f32` result is rounded once at the end. They are as accurate as
//! std's to within an ulp of `f32` (`as_accurate_as_std_to_an_ulp`), and
//! `baud-domain`'s Xcms routines are the precedent: baud's own sine where
//! the answer must not depend on the host. Every effect, the quad chase
//! and the flash bound call these, and a test bars std's from their
//! modules (`the_trails_call_no_platform_math`).

/// The functions, as methods so a call keeps its receiver: `x.det_exp()`
/// where std's is `x.exp()`.
pub trait Det: Sized {
    fn det_exp(self) -> Self;
    fn det_exp2(self) -> Self;
    fn det_ln(self) -> Self;
    fn det_log2(self) -> Self;
    fn det_sin(self) -> Self;
    fn det_cos(self) -> Self;
    fn det_sin_cos(self) -> (Self, Self);
    fn det_asin(self) -> Self;
    fn det_acos(self) -> Self;
    fn det_atan2(self, x: Self) -> Self;
    fn det_hypot(self, y: Self) -> Self;
    /// `self` to the power `y`, for a base at or above zero -- every
    /// caller's -- through `exp(y ln x)`; a negative base is NaN.
    fn det_powf(self, y: Self) -> Self;
}

impl Det for f64 {
    fn det_exp(self) -> f64 {
        exp(self)
    }
    fn det_exp2(self) -> f64 {
        exp2(self)
    }
    fn det_ln(self) -> f64 {
        ln(self)
    }
    fn det_log2(self) -> f64 {
        log2(self)
    }
    fn det_sin(self) -> f64 {
        sin_cos(self).0
    }
    fn det_cos(self) -> f64 {
        sin_cos(self).1
    }
    fn det_sin_cos(self) -> (f64, f64) {
        sin_cos(self)
    }
    fn det_asin(self) -> f64 {
        asin(self)
    }
    fn det_acos(self) -> f64 {
        acos(self)
    }
    fn det_atan2(self, x: f64) -> f64 {
        atan2(self, x)
    }
    fn det_hypot(self, y: f64) -> f64 {
        hypot(self, y)
    }
    fn det_powf(self, y: f64) -> f64 {
        powf(self, y)
    }
}

impl Det for f32 {
    fn det_exp(self) -> f32 {
        exp(f64::from(self)) as f32
    }
    fn det_exp2(self) -> f32 {
        exp2(f64::from(self)) as f32
    }
    fn det_ln(self) -> f32 {
        ln(f64::from(self)) as f32
    }
    fn det_log2(self) -> f32 {
        log2(f64::from(self)) as f32
    }
    fn det_sin(self) -> f32 {
        sin_cos(f64::from(self)).0 as f32
    }
    fn det_cos(self) -> f32 {
        sin_cos(f64::from(self)).1 as f32
    }
    fn det_sin_cos(self) -> (f32, f32) {
        let (s, c) = sin_cos(f64::from(self));
        (s as f32, c as f32)
    }
    fn det_asin(self) -> f32 {
        asin(f64::from(self)) as f32
    }
    fn det_acos(self) -> f32 {
        acos(f64::from(self)) as f32
    }
    fn det_atan2(self, x: f32) -> f32 {
        atan2(f64::from(self), f64::from(x)) as f32
    }
    fn det_hypot(self, y: f32) -> f32 {
        hypot(f64::from(self), f64::from(y)) as f32
    }
    fn det_powf(self, y: f32) -> f32 {
        powf(f64::from(self), f64::from(y)) as f32
    }
}

/// **How much of the trails' math a test's thread has run**: every
/// exponential and every sine and cosine ([`SERIES`]), and every distance to
/// a quad the plasma measured ([`DISTANCES`]), every pixel a smear's
/// coverage was measured at ([`COVERAGES`]), every vertex of a bolt
/// ([`VERTICES`]), every pose of a saucer ([`POSES`]) and every state of a
/// gel ([`STATES`]) -- the work a frame costs, counted where a wall clock
/// would read the machine's load.
/// Tests only.
#[cfg(test)]
pub(crate) mod tally {
    use std::cell::Cell;
    use std::thread::LocalKey;

    std::thread_local! {
        pub(crate) static SERIES: Cell<u64> = const { Cell::new(0) };
        pub(crate) static DISTANCES: Cell<u64> = const { Cell::new(0) };
        pub(crate) static COVERAGES: Cell<u64> = const { Cell::new(0) };
        pub(crate) static VERTICES: Cell<u64> = const { Cell::new(0) };
        pub(crate) static POSES: Cell<u64> = const { Cell::new(0) };
        pub(crate) static STATES: Cell<u64> = const { Cell::new(0) };
    }

    pub(crate) fn count(c: &'static LocalKey<Cell<u64>>) {
        c.with(|v| v.set(v.get() + 1));
    }

    /// How many times `f` counted `c`.
    pub(crate) fn counted(c: &'static LocalKey<Cell<u64>>, f: impl FnOnce()) -> u64 {
        let before = c.with(Cell::get);
        f();
        c.with(Cell::get) - before
    }

    /// What `f` ran: its series and its distances.
    pub(crate) fn of(f: impl FnOnce()) -> (u64, u64) {
        let read = || (SERIES.with(Cell::get), DISTANCES.with(Cell::get));
        let before = read();
        f();
        let after = read();
        (after.0 - before.0, after.1 - before.1)
    }
}

/// ln 2 split as fdlibm splits it: a high part whose product with any
/// exponent below 2^20 is exact, and the rest.
const LN2_HI: f64 = 0.6931471803691238;
const LN2_LO: f64 = 1.9082149292705877e-10;
const LN2: f64 = core::f64::consts::LN_2;
const LOG2_E: f64 = core::f64::consts::LOG2_E;

/// pi / 2 in four parts, fdlibm's `pio2_1`, `pio2_2`, `pio2_3` and
/// `pio2_3t`: the first three carry 33 bits each, so a multiple of each by
/// a quadrant count below 2^20 is exact.
const PIO2_1: f64 = 1.5707963267341256;
const PIO2_2: f64 = 6.077100506303966e-11;
const PIO2_3: f64 = 2.0222662487111665e-21;
const PIO2_3T: f64 = 8.4784276603689e-32;
const FRAC_2_PI: f64 = core::f64::consts::FRAC_2_PI;
const PI: f64 = core::f64::consts::PI;
const FRAC_PI_2: f64 = core::f64::consts::FRAC_PI_2;
const FRAC_PI_6: f64 = core::f64::consts::FRAC_PI_6;
const SQRT_3: f64 = 1.7320508075688772;
/// tan(pi / 12), below which the arc tangent's series is taken directly.
const TAN_PI_12: f64 = 0.2679491924311227;

/// `1 / n!` for n from 0 to 16: the exponential's, the sine's and the
/// cosine's coefficients, built at compile time.
const INV_FACT: [f64; 17] = {
    let mut t = [1.0f64; 17];
    let mut i = 1;
    while i < 17 {
        t[i] = t[i - 1] / (i as f64);
        i += 1;
    }
    t
};

/// `1 / (2k + 1)` for k from 0 to 11: the logarithm's and the arc
/// tangent's coefficients.
const INV_ODD: [f64; 12] = {
    let mut t = [1.0f64; 12];
    let mut i = 0;
    while i < 12 {
        t[i] = 1.0 / ((2 * i + 1) as f64);
        i += 1;
    }
    t
};

/// `2^k` for a whole `k` within an `f64`'s normal exponents.
fn pow2(k: f64) -> f64 {
    let e = (k as i64 + 1023).clamp(1, 2046);
    f64::from_bits(e.unsigned_abs() << 52)
}

/// `y * 2^k` for a whole `k` past the normal range, in two steps.
fn scale(y: f64, k: f64) -> f64 {
    let half = (k * 0.5).floor();
    y * pow2(half) * pow2(k - half)
}

/// e^x: `x = k ln 2 + r` with `|r| <= ln 2 / 2`, then e^r by its Taylor
/// series to the eleventh power, whose remainder is below 1e-14 of it.
fn exp(x: f64) -> f64 {
    #[cfg(test)]
    tally::count(&tally::SERIES);
    if x.is_nan() {
        return x;
    }
    if x > 709.7 {
        return f64::INFINITY;
    }
    if x < -745.2 {
        return 0.0;
    }
    let k = (x * LOG2_E).round();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let mut p = INV_FACT[11];
    let mut i = 11;
    while i > 0 {
        i -= 1;
        p = p * r + INV_FACT[i];
    }
    scale(p, k)
}

/// **e^x for many `f32`s, rounded to `f32`**, four at a time: each lane
/// runs [`exp`]'s operations in [`exp`]'s order -- the reduction by
/// `round`, ties away from zero as `f64::round`, the series by Horner's
/// rule, the two powers of two from the exponent's bits -- and takes
/// [`exp`]'s edges, so `out[i]` is `args[i].det_exp()` bit for bit
/// (`the_vector_exp_is_the_scalar`). Four independent series in flight
/// where the scalar's one waited on each step. `out` is at least as long
/// as `args`.
pub(crate) fn exp_f32s(args: &[f32], out: &mut [f32]) {
    use core::simd::Select;
    use core::simd::prelude::{SimdFloat, SimdInt, SimdOrd, SimdPartialOrd, f64x4, i64x4};
    use std::simd::StdFloat;
    #[cfg(test)]
    for _ in args {
        tally::count(&tally::SERIES);
    }
    let pow2 = |k: f64x4| -> f64x4 {
        let e =
            (k.cast::<i64>() + i64x4::splat(1023)).simd_clamp(i64x4::splat(1), i64x4::splat(2046));
        f64x4::from_bits(e.cast::<u64>() << 52)
    };
    for (args, out) in args.chunks(4).zip(out.chunks_mut(4)) {
        let mut lanes = [0.0f64; 4];
        for (lane, &a) in lanes.iter_mut().zip(args) {
            *lane = f64::from(a);
        }
        let x = f64x4::from_array(lanes);
        let k = (x * f64x4::splat(LOG2_E)).round();
        let r = (x - k * f64x4::splat(LN2_HI)) - k * f64x4::splat(LN2_LO);
        let mut p = f64x4::splat(INV_FACT[11]);
        let mut i = 11;
        while i > 0 {
            i -= 1;
            p = p * r + f64x4::splat(INV_FACT[i]);
        }
        let half = (k * f64x4::splat(0.5)).floor();
        let y = p * pow2(half) * pow2(k - half);
        let y = x.simd_lt(f64x4::splat(-745.2)).select(f64x4::splat(0.0), y);
        let y = x
            .simd_gt(f64x4::splat(709.7))
            .select(f64x4::splat(f64::INFINITY), y);
        let y = x.is_nan().select(x, y);
        for (o, v) in out.iter_mut().zip(y.cast::<f32>().to_array()) {
            *o = v;
        }
    }
}

/// 2^x: the whole part exactly, the rest through [`exp`].
fn exp2(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x > 1024.0 {
        return f64::INFINITY;
    }
    if x < -1075.0 {
        return 0.0;
    }
    let k = x.round();
    scale(exp((x - k) * LN2), k)
}

/// The binary exponent `e` and the mantissa `m` of a positive finite `x`,
/// `x = m 2^e` with `m` in [sqrt(1/2), sqrt(2)).
fn split(x: f64) -> (f64, f64) {
    let (x, bias) = if x < f64::MIN_POSITIVE {
        (x * pow2(54.0), -54.0)
    } else {
        (x, 0.0)
    };
    let bits = x.to_bits();
    let e = ((bits >> 52) & 0x7ff) as i64 - 1023;
    let m = f64::from_bits((bits & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
    let (m, e) = if m > core::f64::consts::SQRT_2 {
        (m * 0.5, e + 1)
    } else {
        (m, e)
    };
    (e as f64 + bias, m)
}

/// ln of the mantissa, `2 atanh s` with `s = (m - 1) / (m + 1)`, `|s|`
/// below 0.172, by its series to the 17th power, whose remainder is below
/// 1e-14 of it.
fn ln_mantissa(m: f64) -> f64 {
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut p = INV_ODD[8];
    let mut i = 8;
    while i > 0 {
        i -= 1;
        p = p * s2 + INV_ODD[i];
    }
    2.0 * s * p
}

/// The edges of a logarithm: NaN below zero, minus infinity at zero, and
/// a NaN or infinity as it is.
fn log_edge(x: f64) -> Option<f64> {
    if x.is_nan() || x < 0.0 {
        Some(f64::NAN)
    } else if x == 0.0 {
        Some(f64::NEG_INFINITY)
    } else if x.is_infinite() {
        Some(x)
    } else {
        None
    }
}

fn ln(x: f64) -> f64 {
    if let Some(v) = log_edge(x) {
        return v;
    }
    let (e, m) = split(x);
    e * LN2_HI + (e * LN2_LO + ln_mantissa(m))
}

/// log2: the exponent exactly, so a power of two is a whole number.
fn log2(x: f64) -> f64 {
    if let Some(v) = log_edge(x) {
        return v;
    }
    let (e, m) = split(x);
    e + ln_mantissa(m) * LOG2_E
}

/// sin and cos: `x = k pi/2 + r` with `|r| <= pi/4`, by fdlibm's four-part
/// pi / 2, then the sine's series to the 15th power and the cosine's to
/// the 16th, each remainder below 1e-14, and the quadrant's swap and signs.
fn sin_cos(x: f64) -> (f64, f64) {
    #[cfg(test)]
    tally::count(&tally::SERIES);
    if !x.is_finite() {
        return (f64::NAN, f64::NAN);
    }
    let k = (x * FRAC_2_PI).round();
    let r = ((x - k * PIO2_1) - k * PIO2_2) - k * PIO2_3 - k * PIO2_3T;
    let r2 = r * r;
    let (mut s, mut c) = (INV_FACT[15], INV_FACT[16]);
    let mut i = 16;
    while i > 1 {
        i -= 2;
        c = c * -r2 + INV_FACT[i];
        if i > 0 {
            s = s * -r2 + INV_FACT[i - 1];
        }
    }
    let s = s * r;
    match (k as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

/// The arc tangent of `a` at or above zero: past one by `pi/2 - atan(1/a)`,
/// past tan(pi/12) by `pi/6 + atan((a sqrt 3 - 1) / (a + sqrt 3))`, then
/// the series to the 23rd power on `|t| <= tan(pi/12)`, whose remainder
/// is below 1e-14.
fn atan_positive(a: f64) -> f64 {
    let (a, flip) = if a > 1.0 { (1.0 / a, true) } else { (a, false) };
    let (t, base) = if a > TAN_PI_12 {
        ((a * SQRT_3 - 1.0) / (a + SQRT_3), FRAC_PI_6)
    } else {
        (a, 0.0)
    };
    let t2 = t * t;
    let mut p = INV_ODD[11];
    let mut i = 11;
    while i > 0 {
        i -= 1;
        p = p * -t2 + INV_ODD[i];
    }
    let v = base + t * p;
    if flip { FRAC_PI_2 - v } else { v }
}

/// atan2 with IEEE 754's signed zeros and infinities.
fn atan2(y: f64, x: f64) -> f64 {
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        return if x.is_sign_positive() {
            y
        } else {
            PI.copysign(y)
        };
    }
    if x == 0.0 {
        return FRAC_PI_2.copysign(y);
    }
    if x.is_infinite() || y.is_infinite() {
        let v = match (x.is_infinite(), y.is_infinite(), x > 0.0) {
            (true, true, true) => 0.25 * PI,
            (true, true, false) => 0.75 * PI,
            (true, false, true) => 0.0,
            (true, false, false) => PI,
            _ => FRAC_PI_2,
        };
        return v.copysign(y);
    }
    let a = atan_positive((y / x).abs());
    if x > 0.0 {
        a.copysign(y)
    } else {
        (PI - a).copysign(y)
    }
}

fn asin(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    atan2(x, ((1.0 - x) * (1.0 + x)).sqrt())
}

fn acos(x: f64) -> f64 {
    if x.is_nan() || x.abs() > 1.0 {
        return f64::NAN;
    }
    atan2(((1.0 - x) * (1.0 + x)).sqrt(), x)
}

fn hypot(x: f64, y: f64) -> f64 {
    if x.is_infinite() || y.is_infinite() {
        return f64::INFINITY;
    }
    (x * x + y * y).sqrt()
}

fn powf(x: f64, y: f64) -> f64 {
    if y == 0.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return if y > 0.0 { 0.0 } else { f64::INFINITY };
    }
    exp(y * ln(x))
}

/// **The platform's functions, as a call spells them**: what a source
/// whose numbers reach a trail's pixels must not spell, which
/// `the_trails_call_no_platform_math` and the app's chase hold theirs to.
pub const PLATFORM: [&str; 20] = [
    ".exp(",
    ".exp2(",
    ".exp_m1(",
    ".ln(",
    ".ln_1p(",
    ".log2(",
    ".log10(",
    ".log(",
    ".sin(",
    ".cos(",
    ".tan(",
    ".sin_cos(",
    ".asin(",
    ".acos(",
    ".atan(",
    ".atan2(",
    ".hypot(",
    ".powf(",
    ".cbrt(",
    ".tanh(",
];

#[cfg(test)]
mod tests {
    use super::{Det, PLATFORM};

    /// **The trails call no platform math**: every effect's model, the quad
    /// chase and the flash bound -- what a trail's pixels are built from --
    /// spell none of std's libm calls, each of which is the host's and
    /// differs between macOS and glibc, so each is `det`'s. Perturbation: a
    /// `.exp(` put back in the fire's embers fails it.
    #[test]
    fn the_trails_call_no_platform_math() {
        for (name, src) in [
            ("effect.rs", include_str!("effect.rs")),
            ("fire.rs", include_str!("fire.rs")),
            ("plasma.rs", include_str!("plasma.rs")),
            ("lightning.rs", include_str!("lightning.rs")),
            ("ice.rs", include_str!("ice.rs")),
            ("ufo.rs", include_str!("ufo.rs")),
            ("orb.rs", include_str!("orb.rs")),
            ("liquid.rs", include_str!("liquid.rs")),
            ("trail.rs", include_str!("trail.rs")),
            ("flash.rs", include_str!("flash.rs")),
            ("fireworks.rs", include_str!("fireworks.rs")),
        ] {
            for call in PLATFORM {
                assert!(!src.contains(call), "{name} calls the platform's {call}");
            }
            // The positive control: the scan reads the source it names.
            assert!(src.contains(".det_"), "{name} calls none of det's");
        }
    }

    /// **The four-lane exponential is the scalar one**, bit for bit: over
    /// every `f32` from -105 to 0 a 997th apart -- the filaments' range --
    /// over the `f32`s nearest where `x log2 e` is a half, either side of
    /// the reduction's rounding, over both ends of the range and past
    /// them, the subnormal results, a NaN and the infinities, and at
    /// several lengths, so a part-filled last vector is read too. No `f32`
    /// puts `x log2 e` on a half exactly, so the lanes' rounding is held to
    /// `f64::round` at the halves themselves, ties away from zero.
    /// Perturbation: the reduction's `round` spelled `floor` fails it.
    #[test]
    fn the_vector_exp_is_the_scalar() {
        let mut args: Vec<f32> = (0..=104_685u32).map(|i| -(i as f32) / 997.0).collect();
        // The `f32`s nearest each half of `x log2 e`, and the ones beside.
        for k in -160i32..160 {
            let x = ((f64::from(k) + 0.5) / super::LOG2_E) as f32;
            args.extend([
                f32::from_bits(x.to_bits() - 1),
                x,
                f32::from_bits(x.to_bits() + 1),
            ]);
        }
        args.extend([
            0.0,
            -0.0,
            1.0,
            -1.0,
            88.7,
            88.8,
            -87.3,
            -103.9,
            -104.0,
            -149.0,
            -745.1,
            -745.3,
            709.6,
            709.8,
            f32::MAX,
            f32::MIN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::NAN,
            f32::MIN_POSITIVE,
            -f32::MIN_POSITIVE,
        ]);
        let mut out = vec![0.0f32; args.len()];
        for len in [
            args.len(),
            args.len() - 1,
            args.len() - 2,
            args.len() - 3,
            1,
            2,
            3,
            5,
        ] {
            super::exp_f32s(&args[..len], &mut out[..len]);
            for (&a, &v) in args[..len].iter().zip(&out[..len]) {
                assert_eq!(v.to_bits(), a.det_exp().to_bits(), "e^{a}");
            }
        }
        // The lanes' rounding at the halves, against the scalar's.
        {
            use core::simd::f64x4;
            use std::simd::StdFloat;
            for k in -1100i32..1100 {
                let h = f64::from(k) + 0.5;
                let lanes = f64x4::from_array([h, -h, h + 1.0, f64::from(k)]).round();
                for (v, x) in lanes
                    .to_array()
                    .into_iter()
                    .zip([h, -h, h + 1.0, f64::from(k)])
                {
                    assert_eq!(v.to_bits(), x.round().to_bits(), "round({x})");
                }
            }
        }
        // The positive control: the series reached its subnormal results.
        assert!(args.iter().any(|&a| {
            let v = a.det_exp();
            v > 0.0 && !v.is_normal()
        }));
    }

    /// **As accurate as std's, to an ulp**: every function against std's
    /// `f32` one over a sweep of the ranges the trails use and past them,
    /// edges included, and none more than one ulp off -- std's is itself
    /// within an ulp of the true value, so this bounds these at two. A
    /// series cut short, or a reduction constant mistyped, misses by many.
    #[test]
    fn as_accurate_as_std_to_an_ulp() {
        // The ulps between two finite `f32`s, zero for two equal values
        // (signed zeros included).
        let ulps = |a: f32, b: f32| {
            let key = |v: f32| {
                let i = v.to_bits().cast_signed();
                if i < 0 { i32::MIN - i } else { i }
            };
            if a == b { 0 } else { key(a).abs_diff(key(b)) }
        };
        let mut s = 0x1234_5678u32;
        let mut next = || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f32::from_bits((s >> 9) | 0x3f80_0000) - 1.0
        };
        // A check: this function and std's at a pair of sweep values.
        type Pair = fn(f32, f32) -> (f32, f32);
        let checks: [(&str, Pair); 12] = [
            ("exp", |x, _| ((x * 2.0).det_exp(), (x * 2.0).exp())),
            ("exp2", |x, _| ((x * 3.0).det_exp2(), (x * 3.0).exp2())),
            ("ln", |x, _| {
                let v = (x.abs() * 50.0).max(1e-30);
                (v.det_ln(), v.ln())
            }),
            ("log2", |x, _| {
                let v = (x.abs() * 50.0).max(1e-30);
                (v.det_log2(), v.log2())
            }),
            ("sin", |x, _| ((x * 4.0).det_sin(), (x * 4.0).sin())),
            ("cos", |x, _| ((x * 4.0).det_cos(), (x * 4.0).cos())),
            ("sin_cos.1", |x, _| {
                ((x * 9.0).det_sin_cos().1, (x * 9.0).sin_cos().1)
            }),
            ("asin", |x, _| ((x / 40.0).det_asin(), (x / 40.0).asin())),
            ("acos", |x, _| ((x / 40.0).det_acos(), (x / 40.0).acos())),
            ("atan2", |x, y| (x.det_atan2(y), x.atan2(y))),
            ("hypot", |x, y| (x.det_hypot(y), x.hypot(y))),
            ("powf", |x, y| {
                let (b, e) = (x.abs() / 40.0, y.abs() / 10.0);
                (b.det_powf(e), b.powf(e))
            }),
        ];
        for (name, f) in checks {
            let mut worst = 0u32;
            for _ in 0..20_000 {
                let (x, y) = (next() * 80.0 - 40.0, next() * 80.0 - 40.0);
                let (a, b) = f(x, y);
                assert_eq!(a.is_nan(), b.is_nan(), "{name}({x}, {y}): {a} against {b}");
                if !a.is_nan() {
                    worst = worst.max(ulps(a, b));
                }
            }
            assert!(worst <= 1, "{name}: {worst} ulps from std");
        }
        // The edges, each as std has it.
        for v in [
            0.0f32, -0.0, 1.0, -1.0, 0.5, 2.0, 1e-40, 88.0, -103.0, 100.0, -200.0,
        ] {
            for (name, a, b) in [
                ("exp", v.det_exp(), v.exp()),
                ("exp2", v.det_exp2(), v.exp2()),
                ("ln", v.det_ln(), v.ln()),
                ("log2", v.det_log2(), v.log2()),
                ("sin", v.det_sin(), v.sin()),
                ("cos", v.det_cos(), v.cos()),
                ("acos", v.det_acos(), v.acos()),
                ("asin", v.det_asin(), v.asin()),
            ] {
                assert!(
                    (a.is_nan() && b.is_nan()) || ulps(a, b) <= 1,
                    "{name}({v}): {a} against {b}"
                );
            }
        }
        for (y, x) in [
            (0.0f32, 0.0f32),
            (-0.0, 0.0),
            (0.0, -0.0),
            (-0.0, -0.0),
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, -1.0),
            (f32::INFINITY, 1.0),
            (1.0, f32::NEG_INFINITY),
            (f32::INFINITY, f32::INFINITY),
            (-f32::INFINITY, f32::NEG_INFINITY),
        ] {
            let (a, b) = (y.det_atan2(x), y.atan2(x));
            assert!(
                a.to_bits() == b.to_bits() || ulps(a, b) <= 1,
                "atan2({y}, {x}): {a} against {b}"
            );
        }
        assert_eq!(8.0f32.det_log2(), 3.0, "a power of two's log2 is whole");
        assert_eq!(3.0f32.det_exp2(), 8.0, "a whole power of two is exact");
    }
}
