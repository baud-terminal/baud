//! The text layer the audits read through: Python's universal newlines and
//! `splitlines()`.

use xtask::text::{splitlines, strip, universal};

#[test]
fn universal_newlines_read_crlf_and_cr_as_lf() {
    assert_eq!(universal("a\r\nb\rc\n"), "a\nb\nc\n");
}

#[test]
fn splitlines_breaks_where_python_does() {
    assert_eq!(
        splitlines("a\nb\r\nc\rd\u{0b}e\u{0c}f\u{1c}g\u{1d}h\u{1e}i\u{85}j\u{2028}k\u{2029}l"),
        ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"]
    );
    assert_eq!(splitlines("a\n"), ["a"]);
    assert_eq!(splitlines("a\n\n"), ["a", ""]);
    assert!(splitlines("").is_empty());
    assert_eq!(splitlines("a\u{1f}b"), ["a\u{1f}b"]);
}

#[test]
fn strip_takes_the_separators_python_counts_as_space() {
    assert_eq!(strip("\u{1f} \tx\u{a0}\u{3000}"), "x");
}
