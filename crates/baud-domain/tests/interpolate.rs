//! iTerm2's interpolated strings, as its documentation gives them
//! (`[[source: iterm2-doc-scripting-fundamentals]]`, `[[source:
//! iterm2-badges]]`), and the function calls iTerm2 refuses in a format a
//! program sends (`[[source: iterm2-964a75d-ptysession]]`).

use baud_domain::{
    CALL_CHECK_MAX, Interpolated, calls_a_function, expand_escapes, interpolated_parts,
};

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
    assert_eq!(
        parts("\\(a(\"\\(b(\")\"))\"))!"),
        [E("a(\"\\(b(\")\"))\")"), L("!")],
        "a `)` in a nested expression's string closes nothing"
    );
    assert_eq!(
        parts("\\(a == \"x\\\")y\")"),
        [E("a == \"x\\\")y\"")],
        "an escaped quote ends no string"
    );
    assert_eq!(parts("x\\(abc"), [L("x"), L("abc")]);
    assert_eq!(parts("a\\\\(b)"), [L("a\\\\(b)")]);
    assert_eq!(parts(""), []);
}

/// **A literal part's escapes**: the ten letters, `\u` and four
/// characters that are hexadecimal to their end, white space before them
/// or a `0x` in them allowed, and any other backslash kept with what
/// follows it.
#[test]
fn escapes_expand_as_iterm2_expands_them() {
    assert_eq!(expanded("a\\nb\\tc"), "a\nb\tc");
    assert_eq!(expanded("\\r\\a\\b\\f\\e"), "\r\u{7}\u{8}\u{c}\u{1b}");
    assert_eq!(expanded("\\\\ \\\" \\/"), "\\ \" /");
    assert_eq!(expanded("\\u00e9t\\u00E9"), "été");
    assert_eq!(expanded("\\u0x41"), "A", "a 0x");
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
        "\\(\"a\\\" f(1)\")",
        "\\(\"a\\\" f(1)\")\")",
        "x \\(f(1)",
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

fn calls(s: &str) -> bool {
    calls_a_function(s)
}

fn evaluated(s: &str, eval: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    baud_domain::interpolate(
        s,
        |e, w| {
            if let Some(v) = eval(e) {
                v.chars().for_each(&mut *w);
            }
        },
        |c| out.push(c),
    );
    out
}

/// **Every interpolated string the documentation shows**, split as it
/// describes: "To embed an expression in a string, place it in `\(…)`",
/// a variable, a path, a function call, arithmetic, the ternary, and the
/// badge page's example.
#[test]
fn the_documented_strings_split_as_documented() {
    use Interpolated::{Expression as E, Literal as L};
    assert_eq!(
        parts("The current host name is \\(hostname)"),
        [L("The current host name is "), E("hostname")]
    );
    assert_eq!(
        parts("Two plus three is \\(add(x: 2, y: 3))"),
        [L("Two plus three is "), E("add(x: 2, y: 3)")]
    );
    assert_eq!(
        parts("\\(rows * columns) cells"),
        [E("rows * columns"), L(" cells")]
    );
    assert_eq!(
        parts("\\(user.env == \"prod\" ? \"PRODUCTION\" : \"dev\")"),
        [E("user.env == \"prod\" ? \"PRODUCTION\" : \"dev\"")]
    );
    assert_eq!(
        parts("\\(currentTab.currentSession.hostname)"),
        [E("currentTab.currentSession.hostname")]
    );
    assert_eq!(parts("\\(user.gitBranch)"), [E("user.gitBranch")]);
    assert_eq!(
        parts("\\(session.name) \\(user.gitBranch)"),
        [E("session.name"), L(" "), E("user.gitBranch")]
    );
    assert_eq!(
        parts("\\(create_window(initialDirectory: path, profileName: \"Default\"))"),
        [E(
            "create_window(initialDirectory: path, profileName: \"Default\")"
        )]
    );
    assert_eq!(parts("\\(matches[1])"), [E("matches[1]")]);
    assert_eq!(parts("\\([1, -1.2, 1.2e5])"), [E("[1, -1.2, 1.2e5]")]);
}

/// **"Interpolated strings may be nested"**: the documentation's `cat`
/// example is one expression, its strings each an interpolated string
/// with an expression of its own, and its value is "Lorem ipsum dolor sit
/// amet" when `cat` joins `x` and `y`.
#[test]
fn the_documented_nesting_is_one_expression() {
    use Interpolated::{Expression as E, Literal as L};
    let s = "\\(cat(x: \"Lorem \\(cat(x: \"ipsum \", y: \"dolor\")) sit \", y: \"amet\"))";
    let outer = "cat(x: \"Lorem \\(cat(x: \"ipsum \", y: \"dolor\")) sit \", y: \"amet\")";
    assert_eq!(parts(s), [E(outer)]);
    assert_eq!(
        parts("Lorem \\(cat(x: \"ipsum \", y: \"dolor\")) sit "),
        [
            L("Lorem "),
            E("cat(x: \"ipsum \", y: \"dolor\")"),
            L(" sit ")
        ]
    );
    // A `cat` for the test: its two arguments are strings, each an
    // interpolated string evaluated with this same `cat`.
    fn cat(e: &str) -> Option<String> {
        let args = e.strip_prefix("cat(x: \"")?.strip_suffix("\")")?;
        let at = args.rfind("\", y: \"")?;
        let x = evaluated(&args[..at], cat);
        let y = evaluated(&args[at + 7..], cat);
        Some(x + &y)
    }
    assert_eq!(evaluated(s, cat), "Lorem ipsum dolor sit amet");
}

/// **What the documentation's strings evaluate to**, the expression
/// replaced by "its value" and the text around it kept.
#[test]
fn the_documented_strings_evaluate_as_documented() {
    let value = |e: &str| match e {
        "hostname" => Some("example.com".to_string()),
        "add(x: 2, y: 3)" => Some("5".to_string()),
        "rows * columns" => Some("1920".to_string()),
        _ => None,
    };
    assert_eq!(
        evaluated("The current host name is \\(hostname)", value),
        "The current host name is example.com"
    );
    assert_eq!(
        evaluated("Two plus three is \\(add(x: 2, y: 3))", value),
        "Two plus three is 5"
    );
    assert_eq!(evaluated("\\(rows * columns) cells", value), "1920 cells");
}

/// **The documentation's escaping rules, each of the eleven**, in a
/// literal part and in the text of a format with an expression.
#[test]
fn each_documented_escape_expands() {
    for (escape, to) in [
        ("\\n", "\n"),
        ("\\a", "\u{7}"),
        ("\\t", "\t"),
        ("\\r", "\r"),
        ("\\b", "\u{8}"),
        ("\\f", "\u{c}"),
        ("\\e", "\u{1b}"),
        ("\\\\", "\\"),
        ("\\\"", "\""),
        ("\\/", "/"),
        ("\\u263a", "\u{263a}"),
    ] {
        assert_eq!(expanded(escape), to, "{escape:?}");
        assert_eq!(
            evaluated(&format!("<{escape}\\(v)>"), |_| Some("v".to_string())),
            format!("<{to}v>"),
            "{escape:?}"
        );
    }
}

/// **The documented strings that call a function are refused, and the
/// rest are not**: `add`, `cat` nested, and the dotted built-ins call;
/// a variable, a path, arithmetic, the ternary and an array do not.
#[test]
fn the_documented_calls_are_seen_and_the_rest_are_not() {
    for s in [
        "Two plus three is \\(add(x: 2, y: 3))",
        "\\(cat(x: \"Lorem \\(cat(x: \"ipsum \", y: \"dolor\")) sit \", y: \"amet\"))",
        "\\(\"Lorem \\(cat(x: \"ipsum \", y: \"dolor\")) sit \")",
        "\\(iterm2.alert(title: \"t\", subtitle: \"s\", buttons: []))",
        "\\(iterm2.count(array: [1, 2]))",
        "\\(create_window(initialDirectory: path, profileName: \"Default\"))",
        "\\(create_window(initialDirectory: \"/etc\", profileName: \"Default\"))",
        "\\(foo(bar: baz))",
        "\\(iterm2.get_string(title: \"t\", subtitle: \"s\", placeholder: \"p\", defaultValue: \"d\"))",
        "\\(iterm2.move_tab_to_window(tab_id: \"1\"))",
    ] {
        assert!(calls(s), "{s:?}");
    }
    for s in [
        "The current host name is \\(hostname)",
        "\\(rows * columns) cells",
        "\\(user.env == \"prod\" ? \"PRODUCTION\" : \"dev\")",
        "\\(currentTab.currentSession.hostname)",
        "\\(session.name) \\(user.gitBranch)",
        "\\(tab.currentSession.path)",
        "\\(matches[1])",
        "\\([1, -1.2, 1.2e5])",
        "\\(-(1 + 2))",
    ] {
        assert!(!calls(s), "{s:?}");
    }
}

/// **A nested expression is read whole, however long and however deep**:
/// a call is refused at any depth, and a format that calls nothing in
/// either reading is accepted however deeply it nests, past the sixteen
/// levels the split follows too.
#[test]
fn a_long_or_deep_nesting_is_read_to_its_end() {
    let long = format!("\\(\"\\({})\")", "a + ".repeat(200) + "b");
    assert!(!calls(&long));
    let long_call = format!("\\(\"\\({}f(1))\")", "a + ".repeat(200));
    assert!(calls(&long_call));
    let nest = |n: usize, mid: &str| {
        let mut s = "\\(\"".repeat(n);
        s.push_str(mid);
        s.push_str(&"\")".repeat(n));
        s
    };
    // Sixteen strings deep the split follows: a `(` in the last is text.
    assert!(!calls(&nest(17, "(x)")));
    assert_eq!(parts(&nest(17, "(x)")).len(), 1);
    for n in [17, 18, 40] {
        assert!(!calls(&nest(n, "x")), "{n}");
        assert!(!calls(&nest(n, "\\(x)")), "{n}");
        assert!(calls(&nest(n, "\\(f(1))")), "{n}");
        assert!(calls(&nest(n, "\\(a(\"x\") + f(1))")), "{n}");
        // The parentheses are in a string's text, at any depth.
        assert!(!calls(&nest(n, "\\(\"a(b)\")")), "{n}");
        assert!(!calls(&nest(n, "\\(\"f(1)\")")), "{n}");
    }
    // Past sixteen the split reads this `\(` as text and closes the
    // format; read to every depth, nothing closes it, so it is text.
    assert_eq!(parts(&nest(17, "\\(f(")).len(), 1);
    assert!(!calls(&nest(17, "\\(f(")));
    // There the split closes the nest and reads a second expression that
    // calls; read to every depth, nothing closes the format. The split's
    // call is refused.
    let second = nest(17, "\\(") + " \\(f(1))";
    assert_eq!(
        parts(&second).last(),
        Some(&Interpolated::Expression("f(1)"))
    );
    assert!(calls(&second));
    // A call the split reads before or after a string that nests past
    // sixteen, in the expression it gives, is refused, though read to
    // every depth nothing closes that string.
    let deep = nest(16, "\\(");
    for s in [
        format!("\\(f(1) + \"{deep}\")"),
        format!("\\(\"{deep}\" + f(1))"),
        format!("\\(\"\\(f(1))\" + \"{deep}\")"),
    ] {
        assert!(
            matches!(parts(&s)[..], [Interpolated::Expression(_)]),
            "{s:?}"
        );
        assert!(calls(&s), "{s:?}");
    }
}

/// **Every format of [`CALL_CHECK_MAX`] bytes is read at every depth**,
/// which is longer than any badge format a program can send, and a
/// string that opens one expression more than that length allows is read
/// as a call.
#[test]
fn the_call_check_reads_every_depth_a_format_can_reach() {
    assert_eq!(CALL_CHECK_MAX, 385);
    let deepest = format!("\\({}", "\"\\(".repeat((CALL_CHECK_MAX - 2) / 3));
    assert_eq!(deepest.len(), CALL_CHECK_MAX - 2);
    assert!(!calls(&deepest), "open to the end: text");
    let deeper = deepest + "\"\\(";
    assert_eq!(deeper.len(), CALL_CHECK_MAX + 1);
    assert!(calls(&deeper));
}
