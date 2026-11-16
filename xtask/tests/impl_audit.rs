//! Each of the nine categories fires on the shape it names, and is counted
//! in its own sum. The snippets are spelled in pieces so that this file's
//! own text matches none of the patterns.

use xtask::impl_audit::{CATEGORIES, Counts, Sites, Sum};

/// The shape each category is for, in `CATEGORIES` order.
const SHAPES: [&str; 9] = [
    concat!("let i = y *", " MAX_COLS + x;"),
    concat!("fn put(&mut self, n:", " usize) {}"),
    concat!("fn f(x:", " u8) {}"),
    concat!("fn f() ->", " Option<u32> {}"),
    concat!("fn f(on:", " bool) {}"),
    concat!("struct A {\n    n:", " u16,\n}"),
    concat!("let b = x a", "s u8;"),
    concat!("(0x20..0", "x7e).contains(&b)"),
    concat!("struct A {\n    on:", " bool,\n}"),
];

#[test]
fn every_category_fires_on_its_shape_and_lands_in_its_sum() {
    for (i, (c, shape)) in CATEGORIES.iter().zip(SHAPES).enumerate() {
        let n = Counts::of(shape);
        assert_eq!(n.0[i], Sites(1), "{} on {shape:?}", c.name);
        assert!(n.sum(c.sum) >= Sites(1), "{} is not in its sum", c.name);
    }
}

#[test]
fn a_clean_file_counts_nothing() {
    let n = Counts::of("fn f(x: Col) -> Row {\n    Row(x.0)\n}\n");
    assert_eq!(n.sum(Sum::Total), Sites(0));
    assert_eq!(n.sum(Sum::Signature), Sites(0));
}

/// The shell budgets read `tail -1` as the total and `grep '^  total'` and
/// `grep '^  signature'` by name, so the table's last two lines are those,
/// in that order, with the number as the second field.
#[test]
fn the_table_ends_in_the_two_sums_the_budgets_parse() {
    let src = [SHAPES[2], SHAPES[6], SHAPES[6]].join("\n");
    let table = Counts::of(&src).table();
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 13);
    assert_eq!(
        lines[0],
        "  category                                     occurrences"
    );
    let field = |l: &str| l.split_whitespace().nth(1).map(str::to_string);
    assert!(lines[11].starts_with("  signature "));
    assert_eq!(field(lines[11]).as_deref(), Some("1"));
    assert!(lines[12].starts_with("  total "));
    assert_eq!(field(lines[12]).as_deref(), Some("2"));
}

/// Matches are counted without overlap, over the whole file, with `^` at
/// every line: three fields on three lines are three.
#[test]
fn fields_are_counted_line_by_line_across_the_file() {
    let src = concat!(
        "struct A {\n    a:",
        " u16,\n    b:",
        " u16,\n}\n\nstruct B {\n    c:",
        " u16,\n}\n"
    );
    assert_eq!(Counts::of(src).0[5], Sites(3), "{src:?}");
}

/// A generic fn's parameters are read as a non-generic fn's are: type
/// parameters with bounds, lifetimes, a const generic, and a list rustfmt
/// has broken over lines.
#[test]
fn a_generic_fn_s_raw_parameter_is_counted() {
    let bounded = concat!("fn g<B: AsRef<[u8]>>(b: B, at:", " u16) {}");
    assert_eq!(Counts::of(bounded).0[2], Sites(1), "{bounded:?}");
    let lifetimes = concat!("fn g<'a, 'b: 'a>(s: &'a str, n:", " u32) {}");
    assert_eq!(Counts::of(lifetimes).0[2], Sites(1), "{lifetimes:?}");
    let konst = concat!("fn g<const N: usize>(a: [Col; N], i:", " u8) {}");
    assert_eq!(Counts::of(konst).0[2], Sites(1), "{konst:?}");
    let broken = concat!(
        "    fn g<\n        F: Fn(&str) -> Row,\n    >(\n        f: F,\n        n:",
        " u64,\n    ) {\n    }\n"
    );
    assert_eq!(Counts::of(broken).0[2], Sites(1), "{broken:?}");
    let flag = concat!("fn g<T: Copy>(t: T, on:", " bool) {}");
    assert_eq!(Counts::of(flag).0[4], Sites(1), "{flag:?}");
    let operation = concat!("fn g<T>(&mut self, t: T, n:", " usize) {}");
    let n = Counts::of(operation);
    assert_eq!((n.0[1], n.0[2]), (Sites(1), Sites(1)), "{operation:?}");
    let opaque = concat!("fn g<T>(t: T, b: impl AsRef<[u8]>, n:", " u16) {}");
    assert_eq!(Counts::of(opaque).0[2], Sites(1), "{opaque:?}");
}

/// A generic fn's return is read as a non-generic fn's is, with or without
/// a where clause after it.
#[test]
fn a_generic_fn_s_raw_return_is_counted() {
    let bounded = concat!("fn g<B: AsRef<[u8]>>(b: B, at: Col) ->", " u32 {}");
    assert_eq!(Counts::of(bounded).0[3], Sites(1), "{bounded:?}");
    let bound_after = concat!(
        "fn g<T>(t: T) ->",
        " Option<u16>\nwhere\n    T: Copy,\n{\n}\n"
    );
    assert_eq!(Counts::of(bound_after).0[3], Sites(1), "{bound_after:?}");
}

/// A generic fn with typed parameters and a typed return counts nothing,
/// and the const generic's own type is not a parameter.
#[test]
fn a_typed_generic_fn_counts_nothing() {
    for src in [
        "fn g<B: AsRef<[u8]>>(b: B, at: Col) -> Row {\n    Row(at.0)\n}\n",
        "fn g<const N: usize>(a: [Col; N]) -> Row {\n    Row(a[0].0)\n}\n",
        "fn g<'a, T>(t: &'a T, x: Col) -> Option<Row>\nwhere\n    T: Copy,\n{\n    None\n}\n",
    ] {
        let n = Counts::of(src);
        assert_eq!(n.sum(Sum::Signature), Sites(0), "{src:?}");
        assert_eq!(n.sum(Sum::Total), Sites(0), "{src:?}");
    }
}
