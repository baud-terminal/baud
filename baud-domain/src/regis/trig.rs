//! The arithmetic a circle needs, in integers: `no_std` has no `sin`, and a
//! page drawn the same on every machine wants none of a float's rounding.
//!
//! **A table built at compile time**: the sine of every quarter degree from
//! 0 to 90, in units of 2^-30, by its Taylor series in `i128`; between two
//! entries the value is interpolated, an error near 2.4e-6 of the radius --
//! a thousandth of a pixel on the largest circle the page holds.

/// Quarter degrees in a right angle.
const STEPS: usize = 360;

/// `sin(k * pi / 720)`, `k` quarter degrees, in units of 2^-30.
const SIN: [i64; STEPS + 1] = {
    // pi in units of 2^-60, to more digits than an `i128` product keeps.
    const PI: i128 = 3_622_009_729_038_561_421;
    let mut t = [0i64; STEPS + 1];
    let mut k = 0;
    while k <= STEPS {
        // x = k * pi / 720, in units of 2^-60.
        let x = PI * k as i128 / 720;
        // sin x = x - x^3/3! + x^5/5! - ..., each term from the last.
        let mut term = x;
        let mut sum = x;
        let mut n = 1;
        while n < 14 {
            term = -((((term * x) >> 60) * x) >> 60) / ((2 * n) as i128 * (2 * n + 1) as i128);
            sum += term;
            n += 1;
        }
        t[k] = (sum >> 30) as i64;
        k += 1;
    }
    t
};

/// The sine of `deg` degrees, `deg` in units of 1/65536, in units of 2^-30.
pub(crate) fn sin(deg: i64) -> i64 {
    // To quarter degrees in units of 1/65536, within one turn.
    let full = 360i64 * 4 * 65536;
    let q = (deg * 4).rem_euclid(full);
    let quad = q / (90 * 4 * 65536);
    let r = q % (90 * 4 * 65536);
    let (r, sign) = match quad {
        0 => (r, 1),
        1 => (90 * 4 * 65536 - r, 1),
        2 => (r, -1),
        _ => (90 * 4 * 65536 - r, -1),
    };
    let i = (r >> 16) as usize;
    let f = r & 0xffff;
    let a = SIN[i.min(STEPS)];
    let b = SIN[(i + 1).min(STEPS)];
    sign * (a + (((b - a) * f) >> 16))
}

/// The cosine, the same way.
pub(crate) fn cos(deg: i64) -> i64 {
    sin(deg + 90 * 65536)
}

/// The integer square root, rounded down.
pub(crate) fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    // Newton's iteration from above, which falls to the floor and stops.
    let mut x = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const Q: i128 = 1 << 30;

    #[test]
    fn the_table_is_the_sine() {
        assert_eq!(sin(0), 0);
        assert_eq!(sin(90 * 65536), Q as i64);
        assert_eq!(sin(180 * 65536), 0);
        assert_eq!(sin(270 * 65536), -(Q as i64));
        // sin 30 = 1/2, sin 45 = sqrt(2)/2, to the table's precision.
        assert!((sin(30 * 65536) - (Q as i64) / 2).abs() < 4);
        assert!((sin(45 * 65536) - 759_250_125).abs() < 4);
        assert!((cos(60 * 65536) - (Q as i64) / 2).abs() < 4);
    }

    #[test]
    fn the_square_root_is_the_floor() {
        for n in [
            0u64,
            1,
            2,
            3,
            4,
            15,
            16,
            17,
            1 << 40,
            (1 << 40) + 1,
            u32::MAX as u64,
        ] {
            let r = isqrt(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n}");
        }
    }
}
