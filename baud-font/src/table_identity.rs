//! **The compiled tables are the committed ones**, element for element and
//! const for const: `build/compose.rs`, `build/indic.rs`, `build/script.rs`
//! and `build/standard_encoding.rs`'s output in `OUT_DIR` against the
//! files the Python generators wrote, before those files are deleted.

use crate::committed_compose as c0;
use crate::committed_indic_data as i0;
use crate::committed_script_data as s0;
use crate::committed_standard_encoding as e0;
use crate::{compose as c1, indic_data as i1, script_data as s1, standard_encoding as e1};

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
    assert_eq!(i1::DENSE_FIRST, i0::DENSE_FIRST);
    assert_eq!(s1::SCRIPT_COUNT, s0::SCRIPT_COUNT);
    assert_eq!(s1::EXTENSION_SLOT, s0::EXTENSION_SLOT);
    let n = [
        same("COMPOSE", c1::COMPOSE, c0::COMPOSE),
        same("DECOMPOSE", c1::DECOMPOSE, c0::DECOMPOSE),
        same("CCC", c1::CCC, c0::CCC),
        same("DENSE", i1::DENSE, i0::DENSE),
        same("LATIN1", i1::LATIN1, i0::LATIN1),
        same("SPARSE_BLOCKS", &i1::SPARSE_BLOCKS, &i0::SPARSE_BLOCKS),
        same("SPARSE", i1::SPARSE, i0::SPARSE),
        same("CODES", s1::CODES, s0::CODES),
        same("TAGS", s1::TAGS, s0::TAGS),
        same("EXTENSION_SETS", s1::EXTENSION_SETS, s0::EXTENSION_SETS),
        same("SCRIPT_INDEX", s1::SCRIPT_INDEX, s0::SCRIPT_INDEX),
        same("SCRIPT_BLOCKS", s1::SCRIPT_BLOCKS, s0::SCRIPT_BLOCKS),
        same("EXTENSION_INDEX", s1::EXTENSION_INDEX, s0::EXTENSION_INDEX),
        same(
            "EXTENSION_BLOCKS",
            s1::EXTENSION_BLOCKS,
            s0::EXTENSION_BLOCKS,
        ),
        same("DIRECTIONS", s1::DIRECTIONS, s0::DIRECTIONS),
        same("RUN_INDEX", s1::RUN_INDEX, s0::RUN_INDEX),
        same("RUN_BLOCKS", s1::RUN_BLOCKS, s0::RUN_BLOCKS),
        same("BRACKET_INDEX", s1::BRACKET_INDEX, s0::BRACKET_INDEX),
        same("BRACKET_BLOCKS", s1::BRACKET_BLOCKS, s0::BRACKET_BLOCKS),
        same(
            "STANDARD_ENCODING",
            e1::STANDARD_ENCODING,
            e0::STANDARD_ENCODING,
        ),
    ];
    extern crate std;
    let list: std::vec::Vec<std::string::String> = n
        .iter()
        .map(|(name, len)| std::format!("{name} {len}"))
        .collect();
    std::println!("identical: {} elements", list.join(", "));
}
