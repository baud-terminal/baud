//! ```text
//! NumPy 2.5.2, parts of it transcribed into Rust in this file.
//! Copyright (c) 2005-2025, NumPy Developers.
//! All rights reserved.
//!
//! Licence: BSD-3-Clause. Its text is in LICENSES/numpy-BSD-3-Clause.txt.
//!
//! Ported: loops_utils.h.src's DOUBLE_pairwise_sum (pairwise),
//! arraytypes.c.src's DOUBLE_fill (arange), calculation.c's power-of-ten
//! rounding (round_to), clip's and maximum's comparisons (clip, maximum)
//! and _function_base_impl.py's gradient (gradient). Changed: each takes
//! f64 values or slices in place of arrays, and gradient takes a rows by
//! cols array with unit spacing and edge_order=1 only.
//! ```
//!
//! The NumPy 2.5.2 the generator was written in, where its arithmetic is
//! not the plain loop's (`LICENSES/numpy-BSD-3-Clause.txt`):
//!
//! - `sum` and `mean` over `float64` add the values pairwise
//!   (`loops_utils.h.src`, `DOUBLE_pairwise_sum`): under 8 a plain loop,
//!   up to 128 eight accumulators combined `((r0+r1)+(r2+r3))+((r4+r5)+
//!   (r6+r7))` and then the rest, above that the two halves, the first a
//!   multiple of 8; the reduction adds that to its initial `0.0`;
//! - `arange` fills `start + i * delta`, `delta` being the second value
//!   less the first (`arraytypes.c.src`, `DOUBLE_fill`);
//! - `round(x, n)` on a `float64` multiplies by 10^n, rounds to even and
//!   divides back, which is not Python's `round` on the exact value:
//!   `round(np.float64(146.95), 1)` is 147.0, Python's 146.9;
//! - `gradient` is central differences over two, one-sided at the edges
//!   (`_function_base_impl.py`, `edge_order=1`).

/// `DOUBLE_pairwise_sum` over `a`.
#[must_use]
pub fn pairwise(a: &[f64]) -> f64 {
    let n = a.len();
    if n < 8 {
        let mut res = -0.0;
        for &x in a {
            res += x;
        }
        res
    } else if n <= 128 {
        let mut r = [0.0; 8];
        r.copy_from_slice(&a[..8]);
        let mut i = 8;
        while i < n - n % 8 {
            for (k, acc) in r.iter_mut().enumerate() {
                *acc += a[i + k];
            }
            i += 8;
        }
        let mut res = ((r[0] + r[1]) + (r[2] + r[3])) + ((r[4] + r[5]) + (r[6] + r[7]));
        for &x in &a[i..] {
            res += x;
        }
        res
    } else {
        let mut n2 = n / 2;
        n2 -= n2 % 8;
        pairwise(&a[..n2]) + pairwise(&a[n2..])
    }
}

/// `a.sum()` for a `float64` array of fewer than 8192 values (one buffer).
#[must_use]
pub fn sum(a: &[f64]) -> f64 {
    0.0 + pairwise(a)
}

/// `a.mean()`: the pairwise sum over the count.
#[allow(clippy::cast_precision_loss, reason = "a count of a few thousand")]
#[must_use]
pub fn mean(a: &[f64]) -> f64 {
    sum(a) / a.len() as f64
}

/// `np.arange(start, stop, step)` of floats.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a positive length of a few thousand"
)]
#[must_use]
pub fn arange(start: f64, stop: f64, step: f64) -> Vec<f64> {
    let len = ((stop - start) / step).ceil();
    let len = if len > 0.0 { len as usize } else { 0 };
    let next = start + step;
    let delta = next - start;
    (0..len)
        .map(|i| match i {
            0 => start,
            1 => next,
            _ => start + i as f64 * delta,
        })
        .collect()
}

/// `round(np.float64(x), n)`: NumPy's, by a power of ten
/// (`calculation.c`'s `power_of_ten`, a table to 10^8).
#[must_use]
pub fn round_to(x: f64, n: usize) -> f64 {
    const P10: [f64; 9] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8];
    let f = P10[n];
    (x * f).round_ties_even() / f
}

/// `np.clip(x, lo, hi)` on floats: `_NPY_MIN(_NPY_MAX(x, lo), hi)`.
#[must_use]
pub fn clip(x: f64, lo: f64, hi: f64) -> f64 {
    let m = if x.is_nan() || x > lo { x } else { lo };
    if m.is_nan() || m < hi { m } else { hi }
}

/// `np.maximum(a, b)`: `a` when it is at least `b`. The vector loop's
/// answer for -0 against +0 can differ from this one; the generator never
/// compares the two zeros (its arguments are distances less positive
/// bounds, or positive).
#[must_use]
pub fn maximum(a: f64, b: f64) -> f64 {
    if a >= b || a.is_nan() { a } else { b }
}

/// `np.gradient(f)` of a `rows` by `cols` array, row by row: the gradient
/// down the rows and the gradient along them.
#[must_use]
pub fn gradient(f: &[f64], rows: usize, cols: usize) -> (Vec<f64>, Vec<f64>) {
    let at = |y: usize, x: usize| f[y * cols + x];
    let mut gy = vec![0.0; rows * cols];
    let mut gx = vec![0.0; rows * cols];
    for y in 0..rows {
        for x in 0..cols {
            gy[y * cols + x] = if y == 0 {
                (at(1, x) - at(0, x)) / 1.0
            } else if y == rows - 1 {
                (at(y, x) - at(y - 1, x)) / 1.0
            } else {
                (at(y + 1, x) - at(y - 1, x)) / (2.0 * 1.0)
            };
            gx[y * cols + x] = if x == 0 {
                (at(y, 1) - at(y, 0)) / 1.0
            } else if x == cols - 1 {
                (at(y, x) - at(y, x - 1)) / 1.0
            } else {
                (at(y, x + 1) - at(y, x - 1)) / (2.0 * 1.0)
            };
        }
    }
    (gy, gx)
}

/// `np.lexsort((k1, k0))` for distinct pairs: the indices sorted by `k0`,
/// then `k1`, stably.
#[must_use]
pub fn lexsort(k1: &[f64], k0: &[f64]) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..k0.len()).collect();
    idx.sort_by(|&a, &b| {
        k0[a]
            .partial_cmp(&k0[b])
            .expect("no NaN")
            .then(k1[a].partial_cmp(&k1[b]).expect("no NaN"))
    });
    idx
}
