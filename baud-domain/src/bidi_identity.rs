//! **The compiled tables are the committed ones**, element for element:
//! `build/bidi.rs`'s output in `OUT_DIR` against the `bidi_data.rs`
//! `scripts/gen-bidi-tables.py` wrote, before that file is deleted.

use crate::bidi_data as compiled;
use crate::committed_bidi_data as committed;

fn same<T: PartialEq + core::fmt::Debug>(name: &str, a: &[T], b: &[T]) -> usize {
    assert_eq!(a.len(), b.len(), "{name}: lengths differ");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert_eq!(x, y, "{name}[{i}] differs");
    }
    a.len()
}

#[test]
fn the_compiled_bidi_tables_are_the_committed_ones() {
    let n = [
        same("BIDI_INDEX", compiled::BIDI_INDEX, committed::BIDI_INDEX),
        same("BIDI_BLOCKS", compiled::BIDI_BLOCKS, committed::BIDI_BLOCKS),
        same(
            "MIRROR_INDEX",
            &compiled::MIRROR_INDEX,
            &committed::MIRROR_INDEX,
        ),
        same(
            "MIRROR_BLOCKS",
            &compiled::MIRROR_BLOCKS,
            &committed::MIRROR_BLOCKS,
        ),
        same(
            "MIRROR_GLYPHS",
            &compiled::MIRROR_GLYPHS,
            &committed::MIRROR_GLYPHS,
        ),
        same(
            "BOX_COUNTERPART",
            &compiled::BOX_COUNTERPART,
            &committed::BOX_COUNTERPART,
        ),
    ];
    extern crate std;
    std::println!(
        "identical: BIDI_INDEX {}, BIDI_BLOCKS {}, MIRROR_INDEX {}, MIRROR_BLOCKS {}, \
         MIRROR_GLYPHS {}, BOX_COUNTERPART {} elements",
        n[0],
        n[1],
        n[2],
        n[3],
        n[4],
        n[5]
    );
}
