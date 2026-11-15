//! ```text
//! CPython 3.14.7 (floatobject.c, mathmodule.c, bltinmodule.c), parts of it
//! transcribed into Rust in this file.
//! Copyright (c) 2001 Python Software Foundation; All Rights Reserved
//!
//! Licence: Python Software Foundation License Version 2 (Python-2.0).
//! Its text is in LICENSES/cpython-Python-2.0.txt.
//!
//! Ported: float_rem (rem), _float_div_mod's quotient (floordiv),
//! math_fsum (fsum) and builtin_sum_impl's float loop (sum). Changed:
//! each takes f64 values in place of Python objects; fsum takes finite
//! values only and has none of math_fsum's handling of infinities, NaNs
//! or overflow; sum is the float loop alone, from a start of 0, and gives
//! None for no values. The rest of this file is baud's.
//! ```
//!
//! The Python the generator was written in, where its arithmetic or its
//! text is not Rust's: `round()` to even, `round(v, n)` on the exact
//! binary value, and `repr()` of a float or a string.

/// `round(v)`: the nearest integer, a tie to the even one.
#[allow(
    clippy::cast_possible_truncation,
    reason = "every value rounded here is a pixel coordinate or size"
)]
#[must_use]
pub fn round(v: f64) -> i64 {
    v.round_ties_even() as i64
}

/// `round(v, digits)`: the float nearest the decimal that `v`'s exact
/// value rounds to, a tie to even, which is what `format!` gives.
#[must_use]
pub fn round_to(v: f64, digits: usize) -> f64 {
    format!("{v:.digits$}")
        .parse()
        .expect("a formatted float parses")
}

/// `f"{v:.2f}".rstrip("0").rstrip(".")`, with `-0` and the empty string
/// written `0`: the SVGs' numbers.
#[must_use]
pub fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// `repr(v)` for a float: the shortest text that reads back as `v`,
/// which Rust's `{:?}` writes too, but for Python's exponent form.
#[must_use]
pub fn repr_float(v: f64) -> String {
    let a = v.abs();
    if v != 0.0 && v.is_finite() && !(1e-4..1e16).contains(&a) {
        // Python: one digit, a '.' and the rest only if there are more,
        // 'e', a sign and at least two digits.
        let s = format!("{v:e}");
        let (m, e) = s.split_once('e').expect("an exponent");
        let e: i32 = e.parse().expect("an integer exponent");
        let sign = if e < 0 { '-' } else { '+' };
        return format!("{m}e{sign}{:02}", e.abs());
    }
    format!("{v:?}")
}

/// `repr(s)` for a string: single quotes unless it holds one and no
/// double quote, the quote and the backslash escaped, and the controls
/// as `\n`, `\r`, `\t` or `\xNN`.
#[must_use]
pub fn repr_str(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::new();
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// `repr()` of a tuple of floats.
#[must_use]
pub fn repr_tuple(v: &[f64]) -> String {
    format!(
        "({})",
        v.iter()
            .map(|&x| repr_float(x))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// `repr()` of a list of strings.
#[must_use]
pub fn repr_list(v: &[String]) -> String {
    format!(
        "[{}]",
        v.iter().map(|x| repr_str(x)).collect::<Vec<_>>().join(", ")
    )
}

/// `x % y` on floats, CPython 3.14.7's `float_rem` (`floatobject.c`,
/// `LICENSES/cpython-Python-2.0.txt`): C's `fmod`, which is exact on every
/// platform, its sign moved to the divisor's.
#[must_use]
pub fn rem(x: f64, y: f64) -> f64 {
    let m = x % y;
    if m != 0.0 {
        if (y < 0.0) != (m < 0.0) { m + y } else { m }
    } else {
        0.0f64.copysign(y)
    }
}

/// `x // y` on floats, CPython's floor division (`floatobject.c`): the
/// quotient of `x` less its remainder, snapped to the nearest integer.
#[must_use]
pub fn floordiv(x: f64, y: f64) -> f64 {
    let m = x % y;
    let mut div = (x - m) / y;
    if m != 0.0 && (y < 0.0) != (m < 0.0) {
        div -= 1.0;
    }
    if div != 0.0 {
        let f = div.floor();
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        0.0f64.copysign(x / y)
    }
}

/// `math.fsum`: CPython 3.14.7's `math_fsum` (`mathmodule.c`), Shewchuk's
/// partials and the half-even fix-up across them, so the sum is the
/// correctly rounded one. Finite input only, as the generator's is.
#[must_use]
pub fn fsum(values: impl IntoIterator<Item = f64>) -> f64 {
    let mut p: Vec<f64> = Vec::new();
    let mut lo = 0.0;
    for v in values {
        let mut x = v;
        let mut i = 0;
        for j in 0..p.len() {
            let mut y = p[j];
            if x.abs() < y.abs() {
                std::mem::swap(&mut x, &mut y);
            }
            let hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                p[i] = lo;
                i += 1;
            }
            x = hi;
        }
        p.truncate(i);
        assert!(x.is_finite(), "fsum: the generator sums finite values");
        if x != 0.0 {
            p.push(x);
        }
    }
    let mut hi = 0.0;
    let mut n = p.len();
    if n > 0 {
        n -= 1;
        hi = p[n];
        while n > 0 {
            let x = hi;
            n -= 1;
            let y = p[n];
            hi = x + y;
            let yr = hi - x;
            lo = y - yr;
            if lo != 0.0 {
                break;
            }
        }
        if n > 0 && ((lo < 0.0 && p[n - 1] < 0.0) || (lo > 0.0 && p[n - 1] > 0.0)) {
            let y = lo * 2.0;
            let x = hi + y;
            let yr = x - hi;
            if y == yr {
                hi = x;
            }
        }
    }
    hi
}

/// The builtin `sum()` of Python floats from its default start, `0`:
/// CPython 3.14.7's `builtin_sum_impl` (`bltinmodule.c`), where the first
/// float is `0 + x` and the rest are added with Neumaier's compensation.
/// Its float loop takes exact `float`s only; every sum the generator makes
/// this way is of Python floats, never NumPy's. `None` for no values,
/// which sum to the integer `0`.
#[must_use]
pub fn sum(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    let mut it = values.into_iter();
    let first = it.next()?;
    let (mut hi, mut lo) = (0.0 + first, 0.0);
    for x in it {
        let t = hi + x;
        if hi.abs() >= x.abs() {
            lo += (hi - t) + x;
        } else {
            lo += (x - t) + hi;
        }
        hi = t;
    }
    Some(if lo != 0.0 && lo.is_finite() {
        hi + lo
    } else {
        hi
    })
}

/// Python's `min(a, b)`: `a` unless `b` is less.
#[must_use]
pub fn min(a: f64, b: f64) -> f64 {
    if b < a { b } else { a }
}

/// Python's `max(a, b)`: `a` unless `b` is greater.
#[must_use]
pub fn max(a: f64, b: f64) -> f64 {
    if b > a { b } else { a }
}

/// `str.isspace()` of one character, which `\s` matches: Unicode's
/// White_Space and U+001C to U+001F, which Python counts and Rust's
/// `char::is_whitespace` does not.
#[must_use]
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `str.splitlines()`: lines split at every boundary Python names, `\r\n`
/// as one, the boundaries dropped.
#[must_use]
pub fn splitlines(s: &str) -> Vec<&str> {
    let breaks = |c: char| {
        matches!(
            c,
            '\n' | '\r'
                | '\u{0b}'
                | '\u{0c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        )
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if breaks(c) {
            out.push(&s[start..i]);
            let mut end = i + c.len_utf8();
            if c == '\r' && it.peek().is_some_and(|&(_, n)| n == '\n') {
                it.next();
                end += 1;
            }
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each value is what CPython 3.14 gives for the same call.
    #[test]
    fn the_arithmetic_and_the_text_are_pythons() {
        assert_eq!(
            [
                round(2.5),
                round(3.5),
                round(-0.5),
                round(-1.5),
                round(0.49999999999999994)
            ],
            [2, 4, 0, -2, 0]
        );
        assert_eq!(round_to(2.675, 2), 2.67, "2.675 is 2.67499999... in binary");
        assert_eq!(round_to(0.125, 2), 0.12, "an exact tie, to even");
        assert_eq!(round_to(0.375, 2), 0.38);
        assert_eq!(
            [num(10.0), num(-0.001), num(0.125), num(-3.5), num(512.0)],
            ["10", "0", "0.12", "-3.5", "512"]
        );
        assert_eq!(
            [
                repr_float(1e-5),
                repr_float(1e16),
                repr_float(1.234_567_890_123_456_8e17),
                repr_float(0.0001),
                repr_float(-61.0)
            ],
            [
                "1e-05",
                "1e+16",
                "1.2345678901234568e+17",
                "0.0001",
                "-61.0"
            ]
        );
        assert_eq!(repr_str("it's"), "\"it's\"");
        assert_eq!(repr_str("a'\"b\n"), "'a\\'\"b\\n'");
        assert_eq!(repr_tuple(&[223.27, 45.0]), "(223.27, 45.0)");
        assert_eq!(
            splitlines("a\r\nb\rc\u{1c}d\u{2028}e\n"),
            ["a", "b", "c", "d", "e"]
        );
        assert!(is_space('\u{1f}') && is_space('\u{a0}') && !is_space('\u{200b}'));
        assert_eq!(
            (min(0.0, -0.0).to_bits(), max(-0.0, 0.0).to_bits()),
            (0.0f64.to_bits(), (-0.0f64).to_bits())
        );
    }
}
