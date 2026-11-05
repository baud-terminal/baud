//! **The compiled tables are the committed ones**, element for element:
//! `build/cluster.rs` and `build/merged.rs`'s output in `OUT_DIR` against
//! the files `scripts/gen-cluster-table.py` and `gen-merged-table.py`
//! wrote, and, reassembled in their layout, against `cluster.bin` and
//! `merged.bin` byte for byte, before those files are deleted.

extern crate std;
use crate::committed_cluster_data as c0;
use crate::committed_merged_data as m0;
use crate::{cluster_data as c1, merged_data as m1};
use std::string::String;
use std::vec::Vec;

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

/// `cluster.bin`'s layout: the state count, the transitions, one ready
/// byte a state, the two parts tables.
fn cluster_bin() -> Vec<u8> {
    let n = c1::CLUSTER_STATES;
    let mut out = Vec::new();
    out.extend_from_slice(&u32::try_from(n).expect("94").to_le_bytes());
    out.extend_from_slice(c1::CLUSTER_NEXT);
    out.extend((0..n).map(|s| u8::from(s == usize::from(c1::CLUSTER_READY))));
    out.extend_from_slice(c1::CLUSTER_PARTS_LEGACY);
    out.extend_from_slice(c1::CLUSTER_PARTS_MARKS_JOINED);
    out
}

/// `merged.bin`'s layout: three counts, the index, the blocks, the
/// palette, the flat plane.
fn merged_bin() -> Vec<u8> {
    let mut out = Vec::new();
    for n in [
        m1::MERGED_INDEX.len(),
        m1::MERGED_BLOCKS.len(),
        m1::MERGED_PALETTE.len() / 3,
    ] {
        out.extend_from_slice(&u32::try_from(n).expect("a count").to_le_bytes());
    }
    out.extend_from_slice(m1::MERGED_INDEX);
    out.extend_from_slice(m1::MERGED_BLOCKS);
    out.extend_from_slice(m1::MERGED_PALETTE);
    out.extend_from_slice(m1::BMP_DIRECT);
    out
}

#[test]
fn the_compiled_cluster_and_merged_tables_are_the_committed_ones() {
    assert_eq!(c1::CLUSTER_READY, c0::CLUSTER_READY, "CLUSTER_READY");
    assert_eq!(c1::CLUSTER_STATES, c0::CLUSTER_STATES, "CLUSTER_STATES");
    let n = [
        same("CLUSTER_NEXT", c1::CLUSTER_NEXT, c0::CLUSTER_NEXT),
        same(
            "CLUSTER_PARTS_LEGACY",
            c1::CLUSTER_PARTS_LEGACY,
            c0::CLUSTER_PARTS_LEGACY,
        ),
        same(
            "CLUSTER_PARTS_MARKS_JOINED",
            c1::CLUSTER_PARTS_MARKS_JOINED,
            c0::CLUSTER_PARTS_MARKS_JOINED,
        ),
        same("MERGED_PALETTE", m1::MERGED_PALETTE, m0::MERGED_PALETTE),
        same("MERGED_INDEX", m1::MERGED_INDEX, m0::MERGED_INDEX),
        same("MERGED_BLOCKS", m1::MERGED_BLOCKS, m0::MERGED_BLOCKS),
        same("BMP_DIRECT", m1::BMP_DIRECT, m0::BMP_DIRECT),
        same(
            "cluster.bin",
            &cluster_bin(),
            include_bytes!("../cluster.bin"),
        ),
        same("merged.bin", &merged_bin(), include_bytes!("../merged.bin")),
    ];
    let list: Vec<String> = n
        .iter()
        .map(|(name, len)| std::format!("{name} {len}"))
        .collect();
    std::println!(
        "identical: CLUSTER_READY {}, CLUSTER_STATES {}, {} elements",
        c1::CLUSTER_READY,
        c1::CLUSTER_STATES,
        list.join(", ")
    );
}
