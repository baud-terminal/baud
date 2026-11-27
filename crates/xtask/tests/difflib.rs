//! `difflib.unified_diff`: each expected list is what Python 3.13's `difflib`
//! printed for the same two lists, and each case holds one of its rules.

use xtask::difflib::unified_diff;

fn lines(s: &str) -> Vec<String> {
    s.chars().map(|c| c.to_string()).collect()
}

fn diff(a: &[String], b: &[String]) -> Vec<String> {
    unified_diff(a, b, "A", "B")
}

#[test]
fn equal_sides_print_nothing() {
    assert!(diff(&lines("ab"), &lines("ab")).is_empty());
    assert!(diff(&[], &[]).is_empty());
}

#[test]
fn a_range_of_one_has_no_length_and_a_range_of_none_has_the_line_before() {
    assert_eq!(
        diff(&lines("a"), &lines("b")),
        &["--- A", "+++ B", "@@ -1 +1 @@", "-a", "+b"]
    );
    assert_eq!(
        diff(&lines("ab"), &[]),
        &["--- A", "+++ B", "@@ -1,2 +0,0 @@", "-a", "-b"]
    );
    assert_eq!(
        diff(&[], &lines("ab")),
        &["--- A", "+++ B", "@@ -0,0 +1,2 @@", "+a", "+b"]
    );
}

#[test]
fn an_insertion_and_a_deletion_carry_two_lines_of_context() {
    assert_eq!(
        diff(&lines("abcdefg"), &lines("abcXdefg")),
        &[
            "--- A",
            "+++ B",
            "@@ -2,4 +2,5 @@",
            " b",
            " c",
            "+X",
            " d",
            " e"
        ]
    );
    assert_eq!(
        diff(&lines("abcdefg"), &lines("abcefg")),
        &[
            "--- A",
            "+++ B",
            "@@ -2,5 +2,4 @@",
            " b",
            " c",
            "-d",
            " e",
            " f"
        ]
    );
}

#[test]
fn more_than_four_equal_lines_split_a_hunk_and_four_do_not() {
    assert_eq!(
        diff(&lines("abcdefghijklmnop"), &lines("abXdefghijklmYop")),
        &[
            "--- A",
            "+++ B",
            "@@ -1,5 +1,5 @@",
            " a",
            " b",
            "-c",
            "+X",
            " d",
            " e",
            "@@ -12,5 +12,5 @@",
            " l",
            " m",
            "-n",
            "+Y",
            " o",
            " p"
        ]
    );
    assert_eq!(
        diff(&lines("abcdefgh"), &lines("aXcdeYgh")),
        &[
            "--- A",
            "+++ B",
            "@@ -1,8 +1,8 @@",
            " a",
            "-b",
            "+X",
            " c",
            " d",
            " e",
            "-f",
            "+Y",
            " g",
            " h"
        ]
    );
}

/// The longest match is taken first, not the first match: `abcd` on the
/// right of both, where a left-to-right walk would pair the left ones.
#[test]
fn the_longest_match_is_found_first() {
    assert_eq!(
        diff(&lines("abcd1abcd"), &lines("abcd2xabcd")),
        &[
            "--- A",
            "+++ B",
            "@@ -3,5 +3,6 @@",
            " c",
            " d",
            "-1",
            "+2",
            "+x",
            " a",
            " b"
        ]
    );
}

/// `b` past 200 lines, half of them one line: that line is popular, so it
/// cannot start a match and only extends one. `u0` starts it and `nop`
/// extends it, so the second `nop` is the one inserted; without the rule the
/// run after it is the longest match, and the first is.
#[test]
fn a_line_in_more_than_one_percent_of_two_hundred_cannot_start_a_match() {
    let mut a = Vec::new();
    for i in 0..110 {
        a.push(format!("u{i}"));
        a.push("nop".to_string());
    }
    let mut b = a.clone();
    b.insert(1, "nop".to_string());
    assert_eq!(
        diff(&a, &b),
        [
            "--- A",
            "+++ B",
            "@@ -1,4 +1,5 @@",
            " u0",
            " nop",
            "+nop",
            " u1",
            " nop"
        ]
    );
}

/// Of two as long, the first: the `ab` kept is the one before the `X`.
#[test]
fn of_equally_long_matches_the_first_is_taken() {
    assert_eq!(
        diff(&lines("abXab"), &lines("ab")),
        [
            "--- A",
            "+++ B",
            "@@ -1,5 +1,2 @@",
            " a",
            " b",
            "-X",
            "-a",
            "-b"
        ]
    );
}
