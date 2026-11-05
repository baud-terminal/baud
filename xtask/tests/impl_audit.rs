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
