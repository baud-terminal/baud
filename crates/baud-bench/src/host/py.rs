//! Values read and written as the Python these programs were read and
//! wrote them, so a report's bytes stay the same: what the keystroke driver's
//! module has, and `float` and the `f` format beside it.

pub use crate::key_latency::py::{abspath, int, is_space, repr_list, repr_str, splitlines, strip};

/// Whether single underscores separate the digits of `s` as Python's
/// numbers allow, and `s` holds nothing else but `also`.
fn well_spaced(s: &str, also: impl Fn(char) -> bool) -> bool {
    let bytes = s.as_bytes();
    s.chars().all(|c| c.is_ascii_digit() || c == '_' || also(c))
        && bytes.iter().enumerate().all(|(i, b)| {
            *b != b'_'
                || (i > 0
                    && i + 1 < bytes.len()
                    && bytes[i - 1].is_ascii_digit()
                    && bytes[i + 1].is_ascii_digit())
        })
}

/// `float(s)` in ASCII digits.
pub fn float(s: &str) -> Option<f64> {
    let s = strip(s);
    if s.contains('_') && !well_spaced(s, |c| !c.is_ascii_alphabetic() || c == 'e' || c == 'E') {
        return None;
    }
    s.replace('_', "").parse().ok()
}

/// `f"{x:{width}.{places}f}"`, whose not-a-number is lower case.
pub fn fixed(x: f64, width: usize, places: usize) -> String {
    if x.is_nan() {
        format!("{:>width$}", "nan")
    } else {
        format!("{x:width$.places$}")
    }
}
