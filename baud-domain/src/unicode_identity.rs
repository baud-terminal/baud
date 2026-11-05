//! **The compiled tables are the committed ones**, element for element:
//! `build/grapheme.rs`, `build/width.rs` and `build/linebreak.rs`'s output
//! in `OUT_DIR` against the files the Python generators wrote, before
//! those files are deleted.

use crate::committed_grapheme_data as g0;
use crate::committed_linebreak_data as l0;
use crate::committed_width_data as w0;
use crate::{grapheme_data as g1, linebreak_data as l1, width_data as w1};

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
fn the_compiled_unicode_tables_are_the_committed_ones() {
    let n = [
        same("BREAK_INDEX", g1::BREAK_INDEX, g0::BREAK_INDEX),
        same("BREAK_BLOCKS", g1::BREAK_BLOCKS, g0::BREAK_BLOCKS),
        same("SPACING_EXTEND", &g1::SPACING_EXTEND, &g0::SPACING_EXTEND),
        same("WIDTH_INDEX", w1::WIDTH_INDEX, w0::WIDTH_INDEX),
        same("WIDTH_BLOCKS", w1::WIDTH_BLOCKS, w0::WIDTH_BLOCKS),
        same("WIDTH8_INDEX", w1::WIDTH8_INDEX, w0::WIDTH8_INDEX),
        same("WIDTH8_BLOCKS", w1::WIDTH8_BLOCKS, w0::WIDTH8_BLOCKS),
        same("LINE_INDEX", l1::LINE_INDEX, l0::LINE_INDEX),
        same("LINE_BLOCKS", l1::LINE_BLOCKS, l0::LINE_BLOCKS),
    ];
    extern crate std;
    let list: std::vec::Vec<std::string::String> = n
        .iter()
        .map(|(name, len)| std::format!("{name} {len}"))
        .collect();
    std::println!("identical: {} elements", list.join(", "));
}
