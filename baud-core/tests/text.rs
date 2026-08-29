//! What the block loop must put on the screen.
//!
//! Written before any measurement: a fast loop that renders the wrong thing is
//! not a result, and this crate exists to be compared against `baud-core2`, so
//! it has to be right first.

use baud_core::*;

/// Feed `bytes` in `chunk`-sized reads and return the first row as text.
fn row0(bytes: &[u8], chunk: usize, cols: usize) -> String {
    let rows = 4usize;
    let mut arena = vec![0u8; READ_CHUNK];
    let mut vp = vec![PackedCell::EMPTY; rows * MAX_COLS];
    let mut used = vec![RowExtent::ZERO; rows];
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: cols as u16,
    };
    let mut t = Term::new(Storage::new(&mut arena, &mut vp, &mut used), &ws);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::from_index(0), &bytes[i..i + n]);
        t.feed(BytePos::from_index(0), ByteLen::from_index(n));
        i += n;
    }
    let vp = t.viewport();
    (0..cols)
        .map(|c| match vp[c].content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => '.',
        })
        .collect()
}

#[test]
fn a_printable_run_is_blitted() {
    assert_eq!(&row0(b"hello", 4096, 8), "hello...");
}

/// A run longer than one block, so the loop takes more than one iteration.
#[test]
fn a_run_crosses_block_boundaries() {
    assert_eq!(
        &row0(b"abcdefghijklmnopqrstuvwxyz", 4096, 30)[..26],
        "abcdefghijklmnopqrstuvwxyz"
    );
}

#[test]
fn a_wide_character_takes_two_cells() {
    // U+4E16 is East Asian Wide; the cell after it is its tail.
    let r = row0("a世b".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('a'));
    assert_eq!(r.chars().nth(1), Some('世'));
    assert_eq!(r.chars().nth(3), Some('b'), "the tail occupies column 2");
}

/// **The property the whole design rests on.** Where the read boundaries fell
/// must not be visible on the screen.
#[test]
fn the_screen_does_not_depend_on_where_the_reads_fell() {
    let src = "ab世cd漢字ef\ngh".as_bytes();
    let whole = row0(src, 4096, 24);
    for chunk in 1..=src.len() {
        assert_eq!(row0(src, chunk, 24), whole, "chunk size {chunk}");
    }
}

/// An overlong form is not a character. `E0 80 AF` used to render `/`, which
/// is the path-traversal overlong.
#[test]
fn an_overlong_form_is_rejected() {
    let r = row0(b"\xe0\x80\xafx", 4096, 8);
    assert_ne!(r.chars().next(), Some('/'));
}
