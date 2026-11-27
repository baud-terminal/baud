//! iTerm2's expressions, read and evaluated (`expression.rs`): the
//! documentation's grammar and examples (`[[source:
//! iterm2-doc-scripting-fundamentals]]`), and the values' rules where it
//! is silent, as iTerm2 evaluates them.

use baud_domain::{
    Binary, ByteAt, DEPTH, Evaluated, Fault, Nest, Unary, Unreadable, Value, evaluate,
    evaluate_expression, names,
};

/// The variables the tests read: a session's and a few user ones.
fn vars(path: &str) -> Option<Value> {
    let text = |s: &str| Some(Value::Text(s.to_string()));
    match path {
        "rows" => Some(Value::Number(24.0)),
        "columns" => Some(Value::Number(80.0)),
        "hostname" => text("example.com"),
        "session.name" | "name" => text("build"),
        "user.gitBranch" => text("main"),
        "user.env" => text("prod"),
        "user.count" => text("5"),
        "user.a-b" => text("dash"),
        "matches" => Some(Value::Array(vec![
            Value::Text("a".into()),
            Value::Text("b".into()),
        ])),
        _ => None,
    }
}

fn eval(s: &str) -> Result<String, Fault> {
    evaluate(s, &vars)
}

fn ok(s: &str) -> String {
    eval(s).unwrap_or_else(|f| panic!("{s:?}: {f:?}"))
}

/// **The documentation's own examples evaluate as it says**: the host
/// name, `rows * columns` cells, the ternary on a user variable, the
/// badge page's `session.name` and branch, and the literals it lists.
#[test]
fn the_documented_expressions_evaluate_as_documented() {
    assert_eq!(
        ok("The current host name is \\(hostname)"),
        "The current host name is example.com"
    );
    assert_eq!(ok("\\(rows * columns) cells"), "1920 cells");
    assert_eq!(
        ok("\\(user.env == \"prod\" ? \"PRODUCTION\" : \"dev\")"),
        "PRODUCTION"
    );
    let staging = |p: &str| match p {
        "user.env" => Some(Value::Text("staging".into())),
        _ => None,
    };
    assert_eq!(
        evaluate(
            "\\(user.env == \"prod\" ? \"PRODUCTION\" : \"dev\")",
            &staging
        ),
        Ok("dev".into())
    );
    assert_eq!(ok("\\(session.name) \\(user.gitBranch)"), "build main");
    for (lit, shown) in [
        ("-3", "-3"),
        ("0", "0"),
        ("2", "2"),
        ("-1.2", "-1.2"),
        ("2.3", "2.3"),
        ("1.2e5", "120000"),
    ] {
        assert_eq!(ok(&format!("\\({lit})")), shown, "{lit}");
    }
    assert_eq!(ok("\\([1, -1.2, 1.2e5])"), "[1, -1.2, 120000]");
    assert_eq!(ok("\\(matches[1])"), "b");
}

/// **The operators bind as the documentation lists them**, lowest first:
/// the ternary, `||`, `&&`, equality, relational, additive,
/// multiplicative, unary, parentheses; each binary one to the left, the
/// ternary to the right.
#[test]
fn the_operators_bind_in_the_documented_order() {
    for (e, v) in [
        ("1 + 2 * 3", "7"),
        ("(1 + 2) * 3", "9"),
        ("1 - 2 - 3", "-4"),
        ("8 / 4 / 2", "1"),
        ("1 < 2 == 1", "1"),
        ("2 > 1 + 1", "0"),
        ("1 || 0 && 0", "1"),
        ("0 && 1 || 1", "1"),
        ("1 == 1 && 2 != 3", "1"),
        ("0 ? 1 : 1 ? 2 : 3", "2"),
        ("1 ? 0 ? 5 : 6 : 7", "6"),
        ("!0", "1"),
        ("!5", "0"),
        ("-(1 + 2)", "-3"),
        ("- -2", "2"),
        ("-2 * 3", "-6"),
        ("!0 + 1", "2"),
        ("rows - 1", "23"),
        ("1 <= 1", "1"),
        ("2 >= 3", "0"),
    ] {
        assert_eq!(ok(&format!("\\({e})")), v, "{e}");
    }
}

/// **A value is written as iTerm2 writes it**: a number as `%0.16g`, a
/// comparison, `true` and `false` as 1 and 0, null as nothing, an array as
/// `[a, b]` with its strings bare.
#[test]
fn values_are_written_as_iterm2_writes_them() {
    for (e, v) in [
        ("1 == 1", "1"),
        ("true", "1"),
        ("false", "0"),
        ("null", ""),
        ("1 / 0", "inf"),
        ("-1 / 0", "-inf"),
        ("0 / 0", "nan"),
        ("1 / 3", "0.3333333333333333"),
        ("0.1 + 0.2", "0.3"),
        ("1e16", "1e+16"),
        ("123456789012345678", "1.234567890123457e+17"),
        ("0.0001", "0.0001"),
        ("0.00001", "1e-05"),
        ("-0", "-0"),
        ("5.", "5"),
        ("2.50", "2.5"),
        ("[]", "[]"),
        ("[1, [2, \"a\"], null]", "[1, [2, a], ]"),
    ] {
        assert_eq!(ok(&format!("\\({e})")), v, "{e}");
    }
}

/// **Strings, numbers and null under each operator**: a string that reads
/// as a number counts as one for arithmetic and comparison, `+` joins no
/// strings, values of two kinds are unequal, null orders as nothing.
#[test]
fn each_kind_meets_each_operator_as_iterm2_has_it() {
    for (e, v) in [
        ("\"2\" + 3", "5"),
        ("user.count * 2", "10"),
        ("\"a\" == \"a\"", "1"),
        ("\"a\" != \"b\"", "1"),
        ("\"a\" < \"b\"", "1"),
        ("\"b\" <= \"a\"", "0"),
        ("\"5\" == 5", "1"),
        ("\"a\" == 5", "0"),
        ("null == null", "1"),
        ("null == 0", "0"),
        ("null < 1", "0"),
        ("null > 1", "0"),
        ("null <= null", "1"),
        ("null >= 1", "0"),
        ("null ? 1 : 2", "2"),
        ("1 ? 2 : \"a\" * 2", "2"),
    ] {
        assert_eq!(ok(&format!("\\({e})")), v, "{e}");
    }
    for (e, f) in [
        ("\"a\" + \"b\"", Fault::NotNumbers(Binary::Plus)),
        ("null + 1", Fault::NotNumbers(Binary::Plus)),
        ("\"a\" && 1", Fault::NotNumbers(Binary::And)),
        ("\"a\" < 1", Fault::Unordered(Binary::Less)),
        ("-\"5\"", Fault::NotANumber(Unary::Negate)),
        ("!\"x\"", Fault::NotANumber(Unary::Not)),
        ("\"a\" ? 1 : 2", Fault::Condition),
        ("0 ? 2 : \"a\" * 2", Fault::NotNumbers(Binary::Times)),
        ("matches[5]", Fault::OutOfRange),
        ("matches[-1]", Fault::OutOfRange),
        ("user.env[0]", Fault::NotAnArray),
        ("matches[\"a\"]", Fault::NotAnIndex),
    ] {
        assert_eq!(eval(&format!("x \\({e}) y")), Err(f), "{e}");
    }
    assert_eq!(ok("\\(matches[1.9])"), "b", "the index truncated");
}

/// **A reference to an undefined variable is empty, and one an operator
/// is given is a fault**, as iTerm2's lax policy reads: the reference
/// standing alone, parenthesised or indexed, is empty; in an operator, a
/// branch the ternary does not pick, an array or an index, it fails the
/// string, before a fault to its right. `?` reads it as null instead.
#[test]
fn an_undefined_variable_alone_is_empty_and_an_operand_fails() {
    for e in ["bogus", "(bogus)", "bogus[1]", "tab.currentSession.path"] {
        assert_eq!(ok(&format!("<\\({e})>")), "<>", "{e}");
    }
    for e in [
        "bogus == \"x\" ? \"a\" : \"b\"",
        "user.nothing == \"prod\" ? \"PRODUCTION\" : \"dev\"",
        "1 ? 2 : bogus",
        "bogus + \"a\" * 2",
        "-bogus",
        "[1, bogus]",
        "matches[bogus]",
        "bogus[1] + 1",
    ] {
        assert_eq!(eval(&format!("<\\({e})>")), Err(Fault::Undefined), "{e}");
    }
    assert_eq!(
        eval("\\(\"a\" * 2 + bogus)"),
        Err(Fault::NotNumbers(Binary::Times)),
        "the fault to its left first"
    );
    assert_eq!(ok("\\(bogus? == null)"), "1");
    assert_eq!(ok("\\(bogus?)"), "");
    assert_eq!(ok("\\(bogus ?)"), "");
    assert_eq!(ok("\\((bogus ?) == null)"), "1");
    assert_eq!(ok("\\(user.nothing? == \"prod\" ? \"P\" : \"dev\")"), "dev");
    assert_eq!(ok("\\(user.gitBranch?)"), "main", "defined, it reads");
}

/// **A string in an expression is an interpolated string of its own**:
/// its expressions evaluated, an undefined one emptying only its own part,
/// a fault failing the whole.
#[test]
fn a_nested_string_is_interpolated() {
    assert_eq!(ok("\\(\"on \\(user.gitBranch)!\")"), "on main!");
    assert_eq!(ok("\\(\"a\\(bogus)b\")"), "ab");
    assert_eq!(ok("\\(\"a\\(bogus)b\" == \"ab\")"), "1");
    assert_eq!(ok("\\(\"x\\(\"y\\(rows)\")\")"), "xy24", "two strings deep");
    assert_eq!(ok("\\(\"tab\\tq\\\"\")"), "tab\tq\"");
    assert_eq!(
        eval("\\(\"a\\(1 + \"x\")\")"),
        Err(Fault::NotNumbers(Binary::Plus))
    );
}

/// **The tokens are iTerm2's**: a name takes `-`, so `rows-1` is a name
/// and `rows - 1` a subtraction; `?` straight after a path makes it
/// optional, so `x?1:2` is off the grammar; a number begins with a digit.
#[test]
fn names_and_numbers_are_read_as_iterm2_reads_them() {
    assert_eq!(ok("\\(user.a-b)"), "dash");
    assert_eq!(ok("<\\(rows-1)>"), "<>", "one undefined name");
    assert_eq!(ok("\\(rows - 1)"), "23");
    let syntax = |e: &str, at: usize| {
        assert_eq!(
            evaluate_expression(e, Nest::TOP, &vars),
            Evaluated::Fault(Fault::Unreadable(Unreadable::Syntax(ByteAt::of(at)))),
            "{e}"
        );
    };
    syntax("user.x?1:2", 7);
    syntax(".5", 0);
    syntax("1 +", 3);
    syntax("1 2", 2);
    syntax("a.", 1);
    syntax("(1", 2);
    syntax("[1, 2", 5);
    syntax("1 ? 2", 5);
    syntax("x? (1)", 3);
    syntax("[1] == [1]", 4);
    syntax("1 # 2", 2);
    assert_eq!(
        evaluate_expression("f(1)", Nest::TOP, &vars),
        Evaluated::Fault(Fault::Unreadable(Unreadable::Call(ByteAt::of(0))))
    );
    assert_eq!(
        evaluate_expression("1 + iterm2.count(array: [])", Nest::TOP, &vars),
        Evaluated::Fault(Fault::Unreadable(Unreadable::Call(ByteAt::of(4))))
    );
}

/// **A fault fails the whole string**, as iTerm2's evaluator ends a string
/// at its first error; a string with no expression is its escapes
/// expanded, as before.
#[test]
fn a_fault_fails_the_string_and_no_expression_is_text() {
    assert_eq!(
        eval("\\(rows) \\(1 +) \\(\"a\" + 1)"),
        Err(Fault::Unreadable(Unreadable::Syntax(ByteAt::of(3))))
    );
    assert_eq!(ok("a\\tb \\\\(x)"), "a\tb \\(x)");
    assert_eq!(ok("x\\(abc"), "x\\(abc", "an expression never closed");
}

/// **Every badge baud keeps is read to its depth**: the deepest nesting
/// of parentheses, unary operators and ternaries a format of [`DEPTH`]
/// bytes can hold is read, in a debug build's two megabytes of thread
/// stack too, and one level past `DEPTH` is unreadable.
#[test]
fn the_deepest_nesting_a_format_can_hold_is_read() {
    let n = DEPTH / 2 - 2;
    let parens = format!("{}1{}", "(".repeat(n), ")".repeat(n));
    assert_eq!(ok(&format!("\\({parens})")), "1");
    let nots = format!("{}0", "!".repeat(DEPTH - 1));
    assert_eq!(ok(&format!("\\({nots})")), "1");
    let ternaries = format!("{}9{}", "1 ? ".repeat(DEPTH / 6), " : 0".repeat(DEPTH / 6));
    assert_eq!(ok(&format!("\\({ternaries})")), "9");
    let past = format!("{}0", "!".repeat(DEPTH + 1));
    assert_eq!(
        evaluate_expression(&past, Nest::TOP, &vars),
        Evaluated::Fault(Fault::Unreadable(Unreadable::TooDeep))
    );
}

/// **The names an expression uses**, its strings' expressions at any depth
/// too, as far as it can be read.
#[test]
fn the_names_an_expression_uses_are_seen_in_its_strings() {
    assert!(names("a + \"x\\(b)\"", |p| p == "b"));
    assert!(names("\"\\(\"\\(user.deep)\")\"", |p| p == "user.deep"));
    assert!(names("m[1]", |p| p == "m"));
    assert!(names("x ?", |p| p == "x"));
    assert!(!names("\"user.x\"", |p| p == "user.x"), "text in a string");
    assert!(!names("1 + 2", |_| true));
    assert!(names("a + ", |p| p == "a"), "as far as it is read");
}
