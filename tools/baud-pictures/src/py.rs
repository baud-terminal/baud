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
