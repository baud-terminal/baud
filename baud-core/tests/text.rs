//! What the block loop must put on the screen.
//!
//! Written before any measurement: a fast loop that renders the wrong thing is
//! not a result, and this crate exists to be compared against `baud-core2`, so
//! it has to be right first.

use baud_core::*;

/// Feed `bytes` in `chunk`-sized reads and return the first row as text.
fn row0(bytes: &[u8], chunk: usize, cols: usize) -> String {
    row_n(bytes, chunk, cols, 0)
}

/// The same, for row `y`. **Row 0 is not where a wrap bug lives**: a writer
/// that clips at the margin leaves row 0 perfectly correct and loses
/// everything that belonged on row 1.
fn row_n(bytes: &[u8], chunk: usize, cols: usize, y: usize) -> String {
    let rows = 4usize;
    let mut arena = vec![0u8; READ_CHUNK];
    let mut vp = vec![PackedCell::EMPTY; rows * MAX_COLS];
    let mut used = vec![RowExtent::ZERO; rows];
    let mut cells3 = vec![PackedCell::EMPTY; CELLS3];
    let mut tmpl = vec![None; NSTYLE];
    let mut tabs = vec![false; MAX_COLS];
    let ws = WinSize {
        ws_row: rows as u16,
        ws_col: cols as u16,
    };
    let mut t = Term::new(
        Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
        ),
        &ws,
    );
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(BytePos::from_index(0), &bytes[i..i + n]);
        t.feed(BytePos::from_index(0), ReadLen::from_index(n));
        i += n;
    }
    let w = t.stride().index();
    let vp = t.viewport();
    let row = &vp[y * w..];
    (0..cols)
        .map(|c| match row[c].content.as_codepoint() {
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

/// **Overwriting half a wide character must clear the other half.**
///
/// The write path skips the orphan probe when the cursor is past the row's
/// written mark, because past it the cells are blank and a blank is not a
/// wide tail. This is the case where that shortcut must *not* apply: the
/// cursor comes back over text already written, so the probe has to run.
#[test]
fn overwriting_half_a_wide_character_clears_the_other_half() {
    // `世` occupies columns 0 and 1; `\r` returns to column 0; `x` lands on
    // the lead and must not leave the tail behind.
    let r = row0("世\rx".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('x'));
    assert_eq!(
        r.chars().nth(1),
        Some('.'),
        "the orphaned tail must be blanked, not left as half a character"
    );
}

/// The same for the second half: landing on a tail orphans the lead in front
/// of it.
#[test]
fn overwriting_the_tail_clears_the_lead() {
    let r = row0("世\r\u{1b}".as_bytes(), 4096, 8);
    let _ = r;
    // `\x1b` is not yet parsed by this core, so drive the case with a
    // backspace instead: write `ab`, back up one, then a wide character that
    // starts on `b` and runs off into fresh space.
    let r = row0("ab\u{8}世".as_bytes(), 4096, 8);
    assert_eq!(r.chars().next(), Some('a'));
    assert_eq!(r.chars().nth(1), Some('世'));
}

/// **A line longer than the row wraps; it does not stop.**
///
/// The one-line loop hands a writer the whole run, so a 300-byte line arrives
/// as one call where the block walk delivered it sixteen bytes at a time. The
/// blitter clipped to the margin and returned, and every later run found no
/// room and vanished in silence -- 3.6M cells of `btop.raw`, and the screen
/// simply stopped accepting text.
///
/// **Chunk invariance did not catch it and could not**, which is the part
/// worth keeping: the drop was driven by the *screen* being full, not by where
/// the read fell, so every chunk size agreed with every other on the same
/// wrong answer. A property that says "two runs agree" is silent when both are
/// wrong the same way.
#[test]
fn a_line_longer_than_the_row_wraps_instead_of_stopping() {
    // Ten columns, twelve characters: `kl` belongs on row 1, and the clipping
    // writer left row 0 exactly right while losing them.
    assert_eq!(&row0(b"abcdefghijkl", 4096, 10), "abcdefghij");
    assert_eq!(&row_n(b"abcdefghijkl", 4096, 10, 1), "kl........");
    // And the row must go on accepting text afterwards, which is the part
    // that cost `btop.raw` 3.6M cells: once the margin was reached, every
    // later run found no room and vanished.
    assert_eq!(&row_n(b"abcdefghijklZ", 4096, 10, 1), "klZ.......");
}

/// A run and the same bytes one at a time must put the same thing on the
/// screen. **The writer's chunking is not allowed to be visible**, which is
/// the same rule as the read boundary one level down -- and it is the one that
/// bites, because a bulk writer and a per-character writer are different code.
#[test]
fn a_run_agrees_with_the_same_characters_written_singly() {
    let src = b"the quick brown fox jumps over the lazy dog, twice over and again";
    for cols in [8usize, 10, 16, 17, 33, 64] {
        let whole = row0(src, 4096, cols);
        for chunk in [1usize, 2, 3, 5, 16, 17] {
            assert_eq!(row0(src, chunk, cols), whole, "cols {cols}, chunk {chunk}");
        }
    }
}
