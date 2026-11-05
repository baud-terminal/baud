//! **The compiled tables are the committed ones**, element for element and
//! const for const: `build/caseless.rs`, `build/search.rs`,
//! `build/sequence.rs`, `build/joining.rs` and `build/ignorable.rs`'s
//! output in `OUT_DIR` against the files the Python generators wrote,
//! before those files are deleted.

use crate::committed_caseless_data as c0;
use crate::committed_ignorable_data as i0;
use crate::committed_joining_data as j0;
use crate::committed_search_data as s0;
use crate::committed_sequence_data as q0;
use crate::{
    caseless_data as c1, ignorable_data as i1, joining_data as j1, search_data as s1,
    sequence_data as q1,
};

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
    assert_eq!(c1::CASELESS_SHIFT, c0::CASELESS_SHIFT);
    assert_eq!(s1::SEARCH_SHIFT, s0::SEARCH_SHIFT);
    assert_eq!(s1::DECOMP_MAX, s0::DECOMP_MAX);
    assert_eq!(s1::FOLD_MAX, s0::FOLD_MAX);
    assert_eq!(q1::WORD0, q0::WORD0);
    assert_eq!(q1::NODES, q0::NODES);
    assert_eq!(q1::BUCKET_BITS, q0::BUCKET_BITS);
    assert_eq!(q1::EDGE_BITS, q0::EDGE_BITS);
    assert_eq!(q1::M1, q0::M1);
    assert_eq!(q1::M2, q0::M2);
    let n = [
        same("CASELESS_INDEX", &c1::CASELESS_INDEX, &c0::CASELESS_INDEX),
        same(
            "CASELESS_BLOCKS",
            &c1::CASELESS_BLOCKS,
            &c0::CASELESS_BLOCKS,
        ),
        same("CASELESS_POOL", &c1::CASELESS_POOL, &c0::CASELESS_POOL),
        same("SEARCH_INDEX", &s1::SEARCH_INDEX, &s0::SEARCH_INDEX),
        same("SEARCH_BLOCKS", &s1::SEARCH_BLOCKS, &s0::SEARCH_BLOCKS),
        same("SEARCH_PROPS", &s1::SEARCH_PROPS, &s0::SEARCH_PROPS),
        same("SEARCH_POOL", &s1::SEARCH_POOL, &s0::SEARCH_POOL),
        same("NODE_SPAN", &q1::NODE_SPAN, &q0::NODE_SPAN),
        same("SPANS", &q1::SPANS, &q0::SPANS),
        same("SEEDS", &q1::SEEDS, &q0::SEEDS),
        same("EDGES", &q1::EDGES, &q0::EDGES),
        same("CONTINUES", &q1::CONTINUES, &q0::CONTINUES),
        same("JOINING_INDEX", j1::JOINING_INDEX, j0::JOINING_INDEX),
        same("JOINING_BLOCKS", j1::JOINING_BLOCKS, j0::JOINING_BLOCKS),
        same(
            "DEFAULT_IGNORABLE",
            i1::DEFAULT_IGNORABLE,
            i0::DEFAULT_IGNORABLE,
        ),
    ];
    extern crate std;
    let list: std::vec::Vec<std::string::String> = n
        .iter()
        .map(|(name, len)| std::format!("{name} {len}"))
        .collect();
    std::println!("identical: {} elements", list.join(", "));
}
