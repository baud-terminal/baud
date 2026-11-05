//! **The shader table's build refuses what is not a trie, and splices
//! once**: `build/stress.rs`, run here on bytes and texts of its own, writes
//! the arrays of a well-formed trie and refuses a stage 1 of the wrong
//! length, a stage 2 that is not whole blocks and a block number past the
//! last; and it splices the table over the shaders' one `#include` line,
//! refusing none or two at the line each stands on. The module only
//! computes, so running it here touches no `OUT_DIR`.

#[path = "../build/stress.rs"]
mod stress;

/// A trie of two blocks: the first all clear, the second with bit 0 set.
fn trie() -> (Vec<u8>, Vec<u8>) {
    let mut index = vec![0u8; 0x1100];
    index[1] = 1;
    let mut blocks = vec![0u8; 64];
    blocks[32] = 1;
    (index, blocks)
}

#[test]
fn a_trie_is_written_as_two_uchar_arrays() {
    let (index, blocks) = trie();
    let text = stress::metal(&index, &blocks).expect("a trie");
    assert!(text.contains("// bytes: 2 blocks."));
    assert!(text.contains("constant uchar STRESS_SKIP_INDEX[4352] = {\n    0x00, 0x01, 0x00,"));
    assert!(text.contains("constant uchar STRESS_SKIP_BLOCKS[64] = {\n"));
    assert!(text.ends_with("};\n"));
    assert_eq!(text.matches("};\n").count(), 2);
}

#[test]
fn what_is_not_a_trie_is_refused() {
    let (index, blocks) = trie();
    let short = stress::metal(&index[..0x10FF], &blocks).unwrap_err();
    assert!(short.contains("4351 entries, 4352 expected"), "{short}");
    let ragged = stress::metal(&index, &blocks[..63]).unwrap_err();
    assert!(
        ragged.contains("63 bytes, not whole blocks of 32"),
        "{ragged}"
    );
    let mut past = index.clone();
    past[7] = 2;
    let past = stress::metal(&past, &blocks).unwrap_err();
    assert!(past.contains("names block 2, of 2 blocks"), "{past}");
}

#[test]
fn the_table_is_spliced_over_the_one_include() {
    assert_eq!(stress::LINE, format!("#include \"{}\"", stress::INCLUDE));
    let src = "a\n#include \"stress_skips.metal\"\nb\n";
    let out = stress::splice("s.metal", src, "T1\nT2\n").expect("one include");
    assert_eq!(out, "a\nT1\nT2\nb\n");
    let none = stress::splice("s.metal", "a\nb\n", "T\n").unwrap_err();
    assert!(none.contains("s.metal: no `#include"), "{none}");
    let twice = format!("{src}{src}");
    let twice = stress::splice("s.metal", &twice, "T\n").unwrap_err();
    assert!(
        twice.contains("s.metal:2:1: `#include \"stress_skips.metal\"` is given 2 times"),
        "{twice}"
    );
    assert!(twice.contains("again at s.metal:5:1"), "{twice}");
}
