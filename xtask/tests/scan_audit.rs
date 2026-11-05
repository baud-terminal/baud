//! The scan audit counts each `while` in a scanner's body, and reads the
//! body's extent as it always has: braces counted on every line, inside
//! strings and comments too.

use xtask::scan_audit::{Limit, Line, report, scans};

fn body(lines: &[&str]) -> String {
    let mut s = String::from("pub(crate) fn run_of_text(s: &[u8]) -> ByteLen {\n");
    for l in lines {
        s += l;
        s += "\n";
    }
    s += "}\n";
    s
}

#[test]
fn each_loop_in_a_scanner_counts_and_the_limit_fails_past_it() {
    let src = body(&["    while a {", "    }", "    while b {", "    }"]);
    let found = scans(&src);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].line, Line(2));
    assert_eq!(found[1].text, "while b {");
    let (out, ok) = report(&found, Limit(1));
    assert!(!ok);
    assert!(
        out.ends_with("  FAIL ingest scans: 2 (limit 1, target 0)\n"),
        "{out}"
    );
    assert!(out.starts_with("  ..   baud-core/src/write.rs:2  run_of_text: while a {\n"));
    assert!(report(&found, Limit(2)).1);
}

#[test]
fn a_loop_outside_the_scanners_does_not_count() {
    let src = "fn feed() {\n    while done < n {\n    }\n}\n";
    assert!(scans(src).is_empty());
    let after = format!("{}fn other() {{\n    while x {{\n    }}\n}}\n", body(&[]));
    assert!(scans(&after).is_empty());
}

/// **The quirk is kept**: a closing brace in a string ends the body early,
/// so the loop after it is not counted. A change to that is a change to the
/// gate, measured on its own.
#[test]
fn a_brace_in_a_string_ends_the_body() {
    let src = body(&["    let s = \"}\";", "    while a {", "    }"]);
    assert!(scans(&src).is_empty());
}

/// Lines are split as `str.splitlines()` splits them, so a form feed starts
/// a line, and its line numbers count it.
#[test]
fn a_form_feed_starts_a_line() {
    let src = body(&["    let x = 1;\u{0c}    while a {", "    }"]);
    let found = scans(&src);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line, Line(3));
}

#[test]
fn the_text_is_cut_at_sixty_characters() {
    let long = format!("    while {} {{", "é".repeat(70));
    let found = scans(&body(&[&long, "    }"]));
    assert_eq!(found[0].text.chars().count(), 60);
}
