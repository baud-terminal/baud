//! ```text
//! fdlibm (k_sin.c, k_cos.c, e_rem_pio2.c), parts of it transcribed into
//! Rust in this file, under the notice those files carry:
//!
//! ====================================================
//! Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//!
//! Developed at SunSoft, a Sun Microsystems, Inc. business.
//! Permission to use, copy, modify, and distribute this
//! software is freely granted, provided that this notice
//! is preserved.
//! ====================================================
//!
//! Licence: SunPro. Its text is in LICENSES/fdlibm-SunPro.txt.
//!
//! Ported: k_sin.c's and k_cos.c's coefficients and polynomials (S, C,
//! ksin, kcos) and e_rem_pio2.c's pio2_1, pio2_1t and pio2_2t (PIO2_1,
//! PIO2_2, PIO2_3). Changed: the kernels take no tail argument, and kcos
//! sums 1 - z/2 + z*p, without k_cos.c's grouping or its branch for
//! |x| >= 0.3. The rest of this file is baud's.
//! ```
//!
//! **Deterministic arithmetic for the README logo's generator**: `sin`,
//! `cos`, `exp` and `hypot` built from `+`, `-`, `*`, `/`, `floor` and
//! `sqrt`, which IEEE 754 rounds correctly on every platform, so the
//! same input gives the same bits everywhere. The platform's libm may
//! round its own functions differently from one machine to the next, and
//! the generator prints a strand's place to four decimals and branches on
//! it; `the_logo_generator_calls_no_platform_math` (`tests/plasma.rs`)
//! holds this file, [`crate::geometry`] and [`crate::plasma`] to that.
//!
//! `sin` and `cos` take fdlibm's kernels (`k_sin.c`, `k_cos.c`, the
//! coefficients and the polynomial's grouping, `LICENSES/fdlibm-SunPro.txt`)
//! on the argument reduced to [-pi/4, pi/4] by a three-part pi/2; `exp`
//! reduces by ln 2 in two parts and sums the Taylor series to the
//! thirteenth power, then scales by a power of two exactly. Each is within
//! a few units in the last place of the true value; what the generator
//! needs is the same value on every machine. `baud_render::det` cannot
//! stand in: it is another reduction with an `f32` result.

#![allow(
    clippy::excessive_precision,
    clippy::approx_constant,
    reason = "fdlibm's constants, spelled as fdlibm spells them"
)]

// pi/2 in three parts, three of fdlibm's (`e_rem_pio2.c`): `pio2_1`, its
// first 33 bits, so k times it is exact for |k| < 2^20; `pio2_1t`, the
// rest to 53 bits; and `pio2_2t`, what lies past `pio2_2`. These are the
// three the generator took, which is not fdlibm's own medium reduction.
const PIO2_1: f64 = 1.570_796_326_734_125_614_17e0;
const PIO2_2: f64 = 6.077_100_506_506_192_249_32e-11;
const PIO2_3: f64 = 2.022_266_248_795_950_631_54e-21;
const TWO_OVER_PI: f64 = 6.366_197_723_675_813_824_33e-1;

const S: [f64; 6] = [
    -1.666_666_666_666_663_243_48e-1,
    8.333_333_333_322_489_461_24e-3,
    -1.984_126_982_985_794_931_34e-4,
    2.755_731_370_707_006_767_89e-6,
    -2.505_076_025_340_686_341_95e-8,
    1.589_690_995_211_550_102_21e-10,
];
const C: [f64; 6] = [
    4.166_666_666_666_660_190_37e-2,
    -1.388_888_888_887_410_957_49e-3,
    2.480_158_728_947_672_941_78e-5,
    -2.755_731_435_139_066_330_35e-7,
    2.087_572_321_298_174_827_90e-9,
    -1.135_964_755_778_819_482_65e-11,
];

const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-1;
const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
const INV_LN2: f64 = 1.442_695_040_888_963_387_00e0;

fn ksin(r: f64) -> f64 {
    let z = r * r;
    let p = S[1] + z * (S[2] + z * (S[3] + z * (S[4] + z * S[5])));
    r + r * z * (S[0] + z * p)
}

fn kcos(r: f64) -> f64 {
    let z = r * r;
    let p = z * (C[0] + z * (C[1] + z * (C[2] + z * (C[3] + z * (C[4] + z * C[5])))));
    1.0 - 0.5 * z + z * p
}

/// `k` and `r` with `x = k pi/2 + r`, `|r|` about pi/4 at most.
fn reduce(x: f64) -> (f64, f64) {
    let k = (x * TWO_OVER_PI + 0.5).floor();
    let r = ((x - k * PIO2_1) - k * PIO2_2) - k * PIO2_3;
    (k, r)
}

/// Which quarter `k` names, Python's `k % 4.0`.
fn quarter(k: f64) -> f64 {
    crate::py::rem(k, 4.0)
}

/// sin x, the same bits on every platform.
#[must_use]
pub fn sin(x: f64) -> f64 {
    let (k, r) = reduce(x);
    let (s, c) = (ksin(r), kcos(r));
    let q = quarter(k);
    if q == 0.0 {
        s
    } else if q == 1.0 {
        c
    } else if q == 2.0 {
        -s
    } else {
        -c
    }
}

/// cos x, as [`sin`].
#[must_use]
pub fn cos(x: f64) -> f64 {
    let (k, r) = reduce(x);
    let (s, c) = (ksin(r), kcos(r));
    let q = quarter(k);
    if q == 0.0 {
        c
    } else if q == 1.0 {
        -s
    } else if q == 2.0 {
        -c
    } else {
        s
    }
}

/// `n!` for `n` up to 13, each exact in an `f64`.
const FACTORIAL: [f64; 14] = [
    1.0,
    1.0,
    2.0,
    6.0,
    24.0,
    120.0,
    720.0,
    5040.0,
    40320.0,
    362_880.0,
    3_628_800.0,
    39_916_800.0,
    479_001_600.0,
    6_227_020_800.0,
];

/// e^x, `x` above -700 so the power of two stays normal.
#[must_use]
pub fn exp(x: f64) -> f64 {
    let k = (x * INV_LN2 + 0.5).floor();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    // The Taylor series to r^13 / 13!, Horner's way from the top.
    let mut s = 1.0 / FACTORIAL[13];
    for n in (1..=12).rev() {
        s = 1.0 / FACTORIAL[n] + r * s;
    }
    s = 1.0 + r * s;
    ldexp(s, k)
}

/// `math.ldexp(s, k)` for a normal result: `s` times 2^k, exact, the power
/// built from its bits in two halves so neither leaves the normal range.
#[allow(
    clippy::cast_possible_truncation,
    reason = "k is an integer of magnitude under 1100"
)]
fn ldexp(s: f64, k: f64) -> f64 {
    let k = k as i64;
    let half = k / 2;
    let pow = |e: i64| f64::from_bits(u64::try_from(e + 1023).expect("a normal power") << 52);
    s * pow(half) * pow(k - half)
}

/// `sqrt(x^2 + y^2)`, the square root correctly rounded.
#[must_use]
pub fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

/// `det.fsum_mean(values)`: the mean, its sum correctly rounded.
#[allow(clippy::cast_precision_loss, reason = "a count of a few thousand")]
#[must_use]
pub fn fsum_mean(values: &[f64]) -> f64 {
    crate::py::fsum(values.iter().copied()) / values.len() as f64
}
