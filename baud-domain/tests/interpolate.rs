//! iTerm2's interpolated strings, parsed as its code parses them
//! (`[[source: iterm2-964a75d-swifty-string-parser]]`, `[[source:
//! iterm2-964a75d-nsstringiterm]]`, `[[source: iterm2-964a75d-ptysession]]`).

use baud_domain::{Interpolated, calls_a_function, expand_escapes, interpolated_parts};

fn parts(s: &str) -> Vec<Interpolated<'_>> {
    let mut v = Vec::new();
    interpolated_parts(s, |p| v.push(p));
    v
}

fn expanded(s: &str) -> String {
    let mut out = String::new();
    expand_escapes(s, |c| out.push(c));
    out
}

/// **A `\(` opens an expression its matching `)` closes**, the parentheses
/// in it counted and those in its strings skipped, a `\(` in a string
/// nesting; an expression left open at the end is literal text, as the
/// parser's last part always is; an escaped backslash opens nothing.
#[test]
fn a_format_splits_into_text_and_expressions() {
    use Interpolated::{Expression as E, Literal as L};
    assert_eq!(
        parts("host \\(user.host) on \\(user.branch)"),
        [L("host "), E("user.host"), L(" on "), E("user.branch")]
    );
    assert_eq!(parts("\\((a + b) * 2)!"), [E("(a + b) * 2"), L("!")]);
    assert_eq!(parts("\\(f(x: \"a)b\"))"), [E("f(x: \"a)b\")")]);
    assert_eq!(
        parts("\\(cat(x: \"L \\(cat(y: \"i\")) s\"))"),
        [E("cat(x: \"L \\(cat(y: \"i\")) s\")")]
    );
    assert_eq!(parts("x\\(abc"), [L("x"), L("abc")]);
    assert_eq!(parts("a\\\\(b)"), [L("a\\\\(b)")]);
    assert_eq!(parts(""), []);
}

/// **A literal part's escapes, as iTerm2 expands them**: the ten letters,
/// `\u` and four characters `scanHexInt` reads to their end, and any other
/// backslash kept with what follows it.
#[test]
fn escapes_expand_as_iterm2_expands_them() {
    assert_eq!(expanded("a\\nb\\tc"), "a\nb\tc");
    assert_eq!(expanded("\\r\\a\\b\\f\\e"), "\r\u{7}\u{8}\u{c}\u{1b}");
    assert_eq!(expanded("\\\\ \\\" \\/"), "\\ \" /");
    assert_eq!(expanded("\\u00e9t\\u00E9"), "été");
    assert_eq!(expanded("\\u0x41"), "A", "scanHexInt takes a 0x");
    assert_eq!(expanded("\\u  41"), "A", "and skips white space");
    assert_eq!(expanded("\\u12g4"), "\\u12g4", "not hexadecimal to the end");
    assert_eq!(expanded("\\u12"), "\\u12", "fewer than four");
    assert_eq!(expanded("\\ud800"), "\u{FFFD}", "a surrogate");
    assert_eq!(expanded("\\q"), "\\q");
    assert_eq!(expanded("end\\"), "end\\");
}

/// **A function call is seen wherever iTerm2 would run one**: a `(` after
/// a name, in an expression or in one nested in its strings; grouping
/// parentheses and parentheses inside a string are none.
#[test]
fn a_function_call_is_seen_where_iterm2_runs_one() {
    for s in [
        "\\(foo())",
        "\\(a.b (c))",
        "x \\(user.x) \\(f(1))",
        "\\(\"\\(g())\")",
        "\\(1 + len(x))",
    ] {
        assert!(calls_a_function(s), "{s:?}");
    }
    for s in [
        "\\(user.gitBranch)",
        "\\((1 + 2) * 3)",
        "\\(\"a(b)\")",
        "plain text (with parentheses)",
        "\\\\(f(x))",
        "",
    ] {
        assert!(!calls_a_function(s), "{s:?}");
    }
}

/// **A format evaluates as iTerm2 joins it**: literal parts with their
/// escapes, each expression what the evaluator writes; with no expression
/// the whole string expanded. An undefined variable writes nothing, as
/// iTerm2's lax policy does.
#[test]
fn a_format_evaluates_part_by_part() {
    let eval = |s: &str| {
        let mut out = String::new();
        baud_domain::interpolate(
            s,
            |e, w| {
                if baud_domain::user_variable(e) == Some("host") {
                    "db-01".chars().for_each(&mut *w);
                }
            },
            |c| out.push(c),
        );
        out
    };
    assert_eq!(eval("on \\(user.host)\\n\\(user.branch?)!"), "on db-01\n!");
    assert_eq!(
        eval("x\\(abc"),
        "x\\(abc",
        "no expression: the whole expanded"
    );
    assert_eq!(
        baud_domain::user_variable(" user.gitBranch? "),
        Some("gitBranch")
    );
    assert_eq!(baud_domain::user_variable("session.name"), None);
    assert_eq!(baud_domain::user_variable("user."), None);
}
