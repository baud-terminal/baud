//! **The sysfs parser, against a tree written for it.** The walk cannot be
//! run against a real `/sys` on the macOS box this was written on, and a
//! parser that has only ever been compiled is the probe-that-cannot-fire
//! failure the repository keeps recording -- so the test writes the layout
//! Linux would, including the shapes that matter: an instruction cache to
//! skip, `K` and `M` suffixes, and a missing `index` to stop on.

#![cfg(feature = "std")]

use baud_domain::{CacheGeometry, LineSize};
use baud_isa::sysfs;
use std::fs;
use std::path::PathBuf;

fn tree(name: &str, dirs: &[&[(&str, &str)]]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("baud-isa-sysfs-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (i, files) in dirs.iter().enumerate() {
        let dir = root.join(format!("index{i}"));
        fs::create_dir_all(&dir).unwrap();
        for (file, text) in files.iter() {
            fs::write(dir.join(file), format!("{text}\n")).unwrap();
        }
    }
    root
}

/// A typical x86-64 laptop as sysfs reports it: L1d and L1i as index0 and
/// index1, L2 and L3 unified, every size suffixed.
#[test]
fn a_linux_tree_reads_as_its_files_say() {
    let root = tree(
        "laptop",
        &[
            &[
                ("level", "1"),
                ("type", "Data"),
                ("coherency_line_size", "64"),
                ("size", "48K"),
                ("ways_of_associativity", "12"),
            ],
            &[
                ("level", "1"),
                ("type", "Instruction"),
                ("coherency_line_size", "64"),
                ("size", "32K"),
                ("ways_of_associativity", "8"),
            ],
            &[
                ("level", "2"),
                ("type", "Unified"),
                ("coherency_line_size", "64"),
                ("size", "1280K"),
                ("ways_of_associativity", "10"),
            ],
            &[
                ("level", "3"),
                ("type", "Unified"),
                ("coherency_line_size", "64"),
                ("size", "24M"),
                ("ways_of_associativity", "12"),
            ],
        ],
    );
    let g = sysfs::detect_at(&root);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(g.line(), LineSize::of(64));
    let l1d = g.l1d().unwrap();
    assert_eq!((l1d.size.bytes(), l1d.ways.count()), (48 * 1024, 12));
    assert_eq!(
        l1d.way().unwrap().bytes(),
        4096,
        "48K twelve ways is one page a way"
    );
    let l2 = g.l2().unwrap();
    assert_eq!((l2.size.bytes(), l2.ways.count()), (1280 * 1024, 10));
    let l3 = g.l3().unwrap();
    assert_eq!((l3.size.bytes(), l3.ways.count()), (24 * 1024 * 1024, 12));
}

/// A tree that stops reporting is partial data, not an error: no ways file
/// means "not reported", and a missing `index1` ends the walk before the
/// directories after it.
#[test]
fn a_partial_tree_is_partial_data() {
    let root = tree(
        "partial",
        &[&[
            ("level", "1"),
            ("type", "Data"),
            ("coherency_line_size", "128"),
            ("size", "131072"),
        ]],
    );
    let g = sysfs::detect_at(&root);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(g.line(), LineSize::of(128));
    let l1d = g.l1d().unwrap();
    assert_eq!(l1d.size.bytes(), 131_072);
    assert_eq!(l1d.way(), None, "no ways file, so no way size");
    assert_eq!((g.l2(), g.l3()), (None, None));
}

#[test]
fn no_tree_at_all_is_unknown() {
    let root = std::env::temp_dir().join("baud-isa-sysfs-nowhere");
    let _ = fs::remove_dir_all(&root);
    assert_eq!(sysfs::detect_at(&root), CacheGeometry::UNKNOWN);
}
