//! **Python's float formats, ported, because Rust's `Display` is not one
//! of them.** The committed oracle files print floats as Python printed
//! them, and the two languages disagree on the cases that matter: `1.0` is
//! `1.0` in Python and `1` under `{}`, and `1e16` is `1e+16` in Python and
//! `10000000000000000` under `{}`. So no oracle formats a float with `{}`.
//!
//! * [`fixed`] is `f"{x:.Nf}"`. Rust's `{:.N}` is the same function: both
//!   round the exact binary value to N places, ties to even -- 0.125 to
//!   0.12, 0.375 to 0.38, 0.03125 to 0.0312 -- and both print negative zero
//!   with its sign. It is a function of its own so that the claim has one
//!   place to be tested.
//! * [`repr`] is `repr(x)`: as many digits as the shortest round trip
//!   takes, the value rounded to that many ties to even (where two such
//!   strings tie, Rust's `{:e}` rounds up and CPython rounds to even), laid
//!   out by CPython's rule in `format_float_short` -- fixed notation while
//!   the decimal point's position `decpt` satisfies `-4 < decpt <= 16`, an
//!   exponent of at least two digits and a sign otherwise, and `.0` added
//!   to a fixed number with no fraction (`Py_DTSF_ADD_DOT_0`).
//! * [`general`] is `"%g" % x`: six significant digits, fixed notation
//!   while the rounded value's exponent is from -4 to 5, trailing zeros
//!   and a bare point dropped, and an exponent of at least two digits and
//!   a sign otherwise.

/// `f"{x:.{places}f}"`.
pub fn fixed(x: f64, places: usize) -> String {
    format!("{x:.places$}")
}

/// `repr(x)` for a Python `float`.
pub fn repr(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    // `{:e}` gives how many digits the shortest round trip takes, `n`. It
    // is not taken for the digits themselves: where two n-digit strings sit
    // exactly as far from the value, it rounds the tie up, and CPython's
    // dtoa rounds it to even -- f32 235.358428955078125, widened, is
    // `235.35842895507812` in Python and `2.3535842895507813e2` under
    // `{:e}`. `{:.*e}` rounds the exact value to n digits, ties to even,
    // and the nearest n-digit string round-trips whenever any does.
    let shortest = format!("{x:e}");
    let n = shortest
        .split('e')
        .next()
        .expect("a mantissa")
        .chars()
        .filter(char::is_ascii_digit)
        .count();
    let e = format!("{x:.*e}", n - 1);
    debug_assert_eq!(e.parse::<f64>().ok(), Some(x), "{e} round-trips");
    let (sign, body) = match e.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", e.as_str()),
    };
    let (mantissa, exp) = body.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    // CPython's decpt: the value is 0.<digits> x 10^decpt.
    let decpt = exp + 1;
    let n = i32::try_from(digits.len()).expect("a digit count");
    let body = if decpt <= -4 || decpt > 16 {
        let (head, tail) = digits.split_at(1);
        let frac = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        let e = decpt - 1;
        let esign = if e < 0 { '-' } else { '+' };
        format!("{head}{frac}e{esign}{:02}", e.unsigned_abs())
    } else if decpt <= 0 {
        format!("0.{}{digits}", "0".repeat(decpt.unsigned_abs() as usize))
    } else if decpt >= n {
        format!("{digits}{}.0", "0".repeat((decpt - n) as usize))
    } else {
        let (int, frac) = digits.split_at(decpt as usize);
        format!("{int}.{frac}")
    };
    format!("{sign}{body}")
}

/// `"%g" % x` for a Python `float`.
pub fn general(x: f64) -> String {
    const P: i32 = 6;
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf" } else { "-inf" }.to_string();
    }
    // The exponent of the value rounded to P digits, which decides the
    // notation, as C's `%g` takes it.
    let e = format!("{x:.*e}", (P - 1) as usize);
    let (mantissa, exp) = e.split_once('e').expect("{:e} has an exponent");
    let exp: i32 = exp.parse().expect("an exponent");
    let strip = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if (-4..P).contains(&exp) {
        strip(&format!("{x:.*}", (P - 1 - exp) as usize))
    } else {
        let esign = if exp < 0 { '-' } else { '+' };
        format!("{}e{esign}{:02}", strip(mantissa), exp.unsigned_abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each row is CPython 3.13's `repr`, run on the value.
    #[test]
    fn repr_is_python_s() {
        let rows: &[(f64, &str)] = &[
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (1.0, "1.0"),
            (84.0, "84.0"),
            (-30.0, "-30.0"),
            (445.5, "445.5"),
            (0.1, "0.1"),
            (f64::from(0.1f32), "0.10000000149011612"),
            (f64::from(123.456f32), "123.45600128173828"),
            // Two 17-digit strings tie; CPython takes the even one.
            (f64::from(235.358_43f32), "235.35842895507812"),
            (f64::from(604.837_95f32), "604.8379516601562"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (1e15, "1000000000000000.0"),
            (9999999999999998.0, "9999999999999998.0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (2.5e-5, "2.5e-05"),
            (-1.25e-7, "-1.25e-07"),
            (1e100, "1e+100"),
            (123456789.0, "123456789.0"),
            (f64::NAN, "nan"),
            (f64::INFINITY, "inf"),
            (f64::NEG_INFINITY, "-inf"),
        ];
        for &(x, want) in rows {
            assert_eq!(repr(x), want, "repr({x:e})");
        }
    }

    /// Each row is CPython's `"%g" % x`.
    #[test]
    fn general_is_python_s() {
        let rows: &[(f64, &str)] = &[
            (12.0, "12"),
            (24.0, "24"),
            (13.5, "13.5"),
            (0.5, "0.5"),
            (-0.0, "-0"),
            (0.0, "0"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (123456.0, "123456"),
            (999999.5, "1e+06"),
            (1234567.0, "1.23457e+06"),
            (1e6, "1e+06"),
            (0.1, "0.1"),
            (2.0 / 3.0, "0.666667"),
            (f64::INFINITY, "inf"),
        ];
        for &(x, want) in rows {
            assert_eq!(general(x), want, "%g of {x:e}");
        }
    }

    /// **The trap, stated as a test.** `{}` on a float is not Python's
    /// `repr`: it drops the `.0` and never writes an exponent. If this
    /// stopped failing for `{}`, the port would still be needed for the
    /// exponent cases, so both are pinned.
    #[test]
    fn display_is_not_repr() {
        assert_ne!(format!("{}", 1.0f64), repr(1.0));
        assert_ne!(format!("{}", 1e16f64), repr(1e16));
        assert_ne!(format!("{:?}", 1e16f64), repr(1e16));
    }

    /// `f"{x:.4f}"` and `f"{x:.2f}"`, CPython 3.13's output: ties go to
    /// even on the exact binary value, and negative zero keeps its sign.
    #[test]
    fn fixed_is_python_s() {
        let rows: &[(f64, usize, &str)] = &[
            (0.125, 2, "0.12"),
            (0.375, 2, "0.38"),
            (0.625, 2, "0.62"),
            (0.03125, 4, "0.0312"),
            (0.09375, 4, "0.0938"),
            (0.15625, 4, "0.1562"),
            (-0.0, 4, "-0.0000"),
            (20.0, 4, "20.0000"),
            (445.5, 4, "445.5000"),
            (2.5e-5, 4, "0.0000"),
        ];
        for &(x, n, want) in rows {
            assert_eq!(fixed(x, n), want, "{x:e} to {n}");
        }
    }
}
