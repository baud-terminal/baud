//! **The compiled tables are the committed ones**, element for element:
//! `build/ignorable.rs` and `build/presentation.rs`'s output in `OUT_DIR`
//! against the files the Python generators wrote, before those files are
//! deleted.

use crate::committed_ignorable as i0;
use crate::committed_presentation as p0;
use crate::{ignorable as i1, presentation as p1};

fn same<T: PartialEq + core::fmt::Debug>(
    name: &'static str,
    a: &[T],
    b: &[T],
) -> (&'static str, usize) {
    assert_eq!(a.len(), b.len(), "{name}: lengths differ");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x, y, "{name}[{i}] differs");
    }
    (name, a.len())
}

#[test]
fn the_compiled_tables_are_the_committed_ones() {
    let n = [
        same(
            "DEFAULT_IGNORABLE",
            i1::DEFAULT_IGNORABLE,
            i0::DEFAULT_IGNORABLE,
        ),
        same("FORMS", &p1::FORMS, &p0::FORMS),
        same("LAM_ALEF", &p1::LAM_ALEF, &p0::LAM_ALEF),
    ];
    extern crate std;
    let list: std::vec::Vec<std::string::String> = n
        .iter()
        .map(|(name, len)| std::format!("{name} {len}"))
        .collect();
    std::println!("identical: {} elements", list.join(", "));
}
