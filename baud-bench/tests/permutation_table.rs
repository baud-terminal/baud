//! **Build the write table by feeding every permutation through the original
//! core, and read off what it writes.**
//!
//! The indexing tables are the permutations; the flat write table is what
//! `baud-core2` produces for them. Deriving it this way rather than
//! re-implementing the rules means the table cannot disagree with the
//! reference — there is only one description of the protocol, and it is the
//! one that already passes 149 coverage rows and the Unicode suites.
//!
//! Feeding is one character per row, so row *i* of a batch is permutation *i*
//! and no cursor arithmetic is needed to find it. **A batch is `ROWS - 1`**:
//! fifty sequences each ending in `LF` on a fifty-row screen scrolls, and then
//! row 0 is not the first permutation. The first version did that and the test
//! reported `E0 80 B1` -- an overlong -- as a mismatch when the real fault was
//! the harness sliding under it.

use baud_core2::*;

const COLS: usize = 200;
const ROWS: usize = 50;

/// Every valid three-byte sequence, in index order: `(b0-0xE0)*64 + (b1-0x80)`
/// selects the table, `b2` the entry.
fn permutations() -> Vec<[u8; 3]> {
    let mut v = Vec::with_capacity(16 * 64 * 64);
    for b0 in 0xE0u8..=0xEF {
        for b1 in 0x80u8..=0xBF {
            for b2 in 0x80u8..=0xBF {
                v.push([b0, b1, b2]);
            }
        }
    }
    v
}

/// Drive core2 with one character per row and return the first cell of each.
fn cells_for(batch: &[[u8; 3]]) -> Vec<PackedCell> {
    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, OpMode::Computed);
    let mut bytes = Vec::new();
    for seq in batch {
        bytes.extend_from_slice(seq);
        bytes.extend_from_slice(b"\r\n");
    }
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    (0..batch.len())
        .map(|r| t.row(Row::from_index(r))[0])
        .collect()
}

/// **The table the new core builds must equal what core2 writes**, on all
/// 65,536 three-byte sequences. This is the whole correctness argument for
/// indexing the cell instead of computing it.
#[test]
fn the_cell_table_is_what_core2_writes() {
    let perms = permutations();
    assert_eq!(perms.len(), 65_536);

    let mut arena = vec![0u8; baud_core::ARENA];
    let ws = baud_core::WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span =
        baud_core::RowBlock::leading(rows, baud_core::Cols::from_stride(baud_core::MAX_COLS));
    let mut vp = vec![baud_core::PackedCell::EMPTY; span.len()];
    let mut used = vec![baud_core::RowExtent::ZERO; rows.index()];
    let mut cells3 = vec![baud_core::PackedCell::EMPTY; baud_core::CELLS3];
    let mut tmpl = vec![None; baud_core::NSTYLE];
    let mut tabs = vec![false; baud_core::MAX_COLS];
    let mut reply = vec![0u8; baud_core::REPLY];
    let store = baud_core::Storage::new(
        &mut arena,
        &mut vp,
        &mut used,
        &mut cells3,
        &mut tmpl,
        &mut tabs,
        &mut reply,
    );
    let t = baud_core::Term::new(store, &ws);
    drop(t);

    let mut checked = 0usize;
    for batch in perms.chunks(ROWS - 1) {
        let want = cells_for(batch);
        for (k, seq) in batch.iter().enumerate() {
            let row = ((seq[0] - 0xE0) as usize * 64 + (seq[1] - 0x80) as usize) * 256;
            let got = cells3[row + seq[2] as usize];
            // **A zero-width character produces no cell.** It attaches to the
            // one before it, and at column 0 there is none -- so core2 leaves
            // the cell empty. The table records the width, so the entry says
            // so; a builder that assumed every sequence yields a cell is what
            // this comparison caught, on `E0 A0 96` (U+0816 SAMARITAN MARK IN).
            match got.flags.width_of().cells() {
                0 => assert_eq!(
                    want[k].content,
                    baud_core::Content::EMPTY,
                    "{:02x} {:02x} {:02x} is zero-width; core2 should write no cell",
                    seq[0],
                    seq[1],
                    seq[2]
                ),
                _ => assert_eq!(
                    got.content, want[k].content,
                    "{:02x} {:02x} {:02x}",
                    seq[0], seq[1], seq[2]
                ),
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 65_536);
}

/// **How much of the write table would a pointer array save?**
///
/// The proposal is to intern the writes -- one array of the distinct cells,
/// and the table holds an index into it -- on the expectation that the same
/// cell is written by many different byte strings. This counts the duplicates
/// rather than assuming them, and it counts *rows* too, because a two-level
/// table whose rows dedupe is the other half of the same idea.
///
/// It is a measurement kept as a test so the numbers cannot go stale: both are
/// asserted, so a change in the entry's shape has to come past this.
#[test]
fn the_write_table_has_almost_nothing_to_intern() {
    use std::collections::HashSet;
    let perms = permutations();
    let mut words: Vec<u64> = Vec::with_capacity(perms.len());
    for batch in perms.chunks(ROWS - 1) {
        for c in cells_for(batch) {
            let b: [u8; 8] = zerocopy::IntoBytes::as_bytes(&c)
                .try_into()
                .expect("PackedCell is eight bytes");
            words.push(u64::from_le_bytes(b));
        }
    }
    let distinct: HashSet<u64> = words.iter().copied().collect();
    let rows: HashSet<Vec<u64>> = words.chunks(256).map(|r| r.to_vec()).collect();
    std::println!(
        "entries {} distinct of {} ({:.1}% unique); rows {} distinct of {}",
        distinct.len(),
        words.len(),
        100.0 * distinct.len() as f64 / words.len() as f64,
        rows.len(),
        words.len() / 256,
    );
    // **92.4% of the writes are unique, so there is almost nothing to
    // intern.** A cell carries the codepoint and the codepoint *is* the
    // concatenation of the index bytes, so an entry can only collide with
    // another when both decode to something that is not their own value --
    // overlongs and surrogates, which all become U+FFFD. That is the whole of
    // the 7.6%, and buying it costs an indirection: index then array is two
    // dependent loads where the value inline is one.
    assert_eq!(distinct.len(), 60573, "distinct write values");
    // Rows dedupe barely better, for the same reason.
    assert_eq!(rows.len(), 241, "distinct 256-entry rows");
    // **And three quarters of the table is unreachable.** `b2` indexes 256
    // slots where only `0x80..=0xBF` can occur, so 65,536 valid sequences sit
    // in 262,144 entries. Subtracting `0x80` would make it 512 KB instead of
    // 2 MB, at the cost of the "no masking, bytes index directly" property.
    assert_eq!(words.len(), 65_536, "valid three-byte sequences");
}
