//! **The cell table must equal what the write path writes**, on all 65,536
//! three-byte sequences.
//!
//! This is the whole correctness argument for indexing the cell instead of
//! computing it, and it used to be made against `baud-core2`: feed every
//! permutation through the reference core and read off what it rendered. That
//! oracle is gone, and the question it answered was the weaker one. The table
//! lives in *this* crate and is read by *this* crate's `put_one`, so what has
//! to hold is that **turning `cell-table` on cannot change the screen** --
//! which is a statement about two paths in one program, not about agreement
//! with another one.
//!
//! Two tests make it, and only the second exercises the read in `put_one`.
//! `the_cell_table_is_what_the_write_path_writes` runs with the feature
//! **off**, which is what makes it a comparison at all: `Term::new` fills the
//! table whenever the slice is non-empty, while the read in `put_one` is
//! gated. So the rendering comes from the computing path and the table is
//! inert data beside it. Gating `fill_cells3` on the feature instead would
//! make it compare the table against itself, and `storage.rs` says so where
//! the switch is. It compares an entry's content with the cell at column 0
//! and skips a zero-width entry, so it cannot see what the read does to the
//! cursor or to the cell before.
//! `the_table_read_writes_what_the_computing_path_writes` feeds the same
//! bytes to a terminal with the table and one without and compares the
//! screens and the cursors, which tests the read when `cell-table` is on.
//! **It is red under the feature**: the read writes a cell of its own for
//! every zero-width character the computing path attaches to the cell before
//! (combining marks, ZWJ, format characters such as U+200B, U+FEFF and the
//! bidi controls, three Prepend), and for 235 spacing marks and Extend
//! characters that take a column.
//!
//! In `the_cell_table_is_what_the_write_path_writes`, feeding is one
//! character per row, so row *i* of a batch is permutation *i* and no cursor
//! arithmetic is needed to find it. **A batch is `ROWS - 1`**: fifty
//! sequences each ending in `LF` on a fifty-row screen scrolls, and then
//! row 0 is not the first permutation.
//! `the_table_read_writes_what_the_computing_path_writes` feeds one context
//! line a row, in batches of `ROWS - 2` after `RIS`.

mod support;

use baud_core::*;
use std::collections::HashSet;

const ROWS: usize = 50;

/// Every valid three-byte sequence, in index order: `(b0-0xE0)*64 + (b1-0x80)`
/// selects the row, `b2` the entry within it.
fn permutations() -> Vec<[u8; 3]> {
    let mut v = Vec::with_capacity(CELLS3_PREFIXES * 64);
    for b0 in 0xE0u8..=0xEF {
        for b1 in 0x80u8..=0xBF {
            for b2 in 0x80u8..=0xBF {
                v.push([b0, b1, b2]);
            }
        }
    }
    v
}

/// Where sequence `seq` lives in the table.
///
/// `usize::from` rather than a cast: the conversion is infallible for a `u8`,
/// and the audit counts casts, which is right -- a cast is where a width can be
/// lost, and this one cannot, so it should not look like one.
///
/// (`impl_audit.py` matches the cast token in comments as well as in code, so
/// naming the spelling it objects to inside this paragraph spent a site. Worth
/// knowing before writing a comment about one.)
fn index_of(seq: [u8; 3]) -> Slot {
    Slot((usize::from(seq[0] - 0xE0) * 64 + usize::from(seq[1] - 0x80)) * 256 + usize::from(seq[2]))
}

/// A position in the flat table.
///
/// A newtype because it sits next to a permutation count and a row index in
/// this file, and three adjacent `usize`s is the shape this project keeps
/// typing its way out of -- `index_of` returning a bare one could be passed
/// where a prefix was wanted and would read a plausible wrong cell.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Slot(usize);

/// Drive `baud-core` with one character per row and return the first cell of
/// each, alongside the table the same `Term::new` built.
fn rendered(batch: &[[u8; 3]], out: &mut Vec<PackedCell>) {
    let mut bytes = Vec::new();
    for seq in batch {
        bytes.extend_from_slice(seq);
        bytes.extend_from_slice(b"\r\n");
    }
    support::with_term(&support::CORPUS, |t| {
        support::feed(t, &bytes);
        out.clear();
        out.extend(
            (0..batch.len()).map(|r| t.row(Row::from_index(r)).expect("a row on the grid")[0]),
        );
    });
}

/// **Each entry's content is the cell the write path writes at column 0**,
/// for every entry of nonzero width; a zero-width entry is checked only for
/// writing no cell there. The read in `put_one` is the next test's.
#[test]
fn the_cell_table_is_what_the_write_path_writes() {
    let perms = permutations();
    assert_eq!(perms.len(), 65_536);

    // One construction for the table, and a fresh terminal per batch for the
    // rendering -- so a scroll or a stale cursor in one batch cannot be read
    // as a table mismatch in the next.
    // **Constructing the terminal is the point**: `Term::new` is what fills the
    // table, and `let _` ends the borrow on `owned` immediately so the table can
    // be read back. Nothing is fed through this one.
    let mut owned = Owned::with_cell_table(&support::CORPUS);
    let _ = Term::new(owned.storage(), &support::CORPUS);
    let table: Vec<PackedCell> = owned.cell_table().to_vec();
    assert_eq!(table.len(), CELLS3);

    let mut want = Vec::new();
    let mut checked = 0usize;
    for batch in perms.chunks(ROWS - 1) {
        rendered(batch, &mut want);
        for (k, &seq) in batch.iter().enumerate() {
            let got = table[index_of(seq).0];
            // **A zero-width character produces no cell.** It attaches to the
            // one before it, and at column 0 there is none -- so the write
            // path leaves the cell empty while the table still records the
            // character and its width, which is what the reader needs to know
            // not to advance. A builder that assumed every sequence yields a
            // cell is what the core2-era version of this caught, on `E0 A0 96`
            // (U+0816 SAMARITAN MARK IN).
            match got.flags.width_of().cells() {
                0 => assert_eq!(
                    want[k].content,
                    Content::EMPTY,
                    "{:02x} {:02x} {:02x} is zero-width; the write path should write no cell",
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

/// A geometry no context line can wrap in, at a cost per batch of 800 cells
/// to compare rather than the corpus's 10,000.
const NARROW: WinSize = WinSize {
    ws_row: 50,
    ws_col: 16,
};

/// **The table read writes the screen the computing path writes**, for every
/// three-byte sequence in each context below, comparing the whole screen and
/// the cursor.
///
/// Two terminals take the same bytes: one on `Owned::new`, whose empty table
/// sends `put_one` down the computing path, and one on
/// `Owned::with_cell_table`, which under `--features cell-table` reads the
/// entry instead. **Under default features the read is compiled out and both
/// terminals compute**, so this passes there by construction; it tests the arm
/// only when the arm is on. The test above cannot stand in for it: it compares
/// an entry's content with the cell at column 0 and skips width 0, so it says
/// nothing about where the read leaves the cursor or what it does to the cell
/// before.
///
/// The read fires on a three-byte character that arrives on its own, so each
/// sequence `X` sits in a context, one per row, and a row that differs names
/// its sequence: `X` then a space at column 0, ` X `, `aXb`, and `X` between
/// two four-byte emoji, the woman and the man, where `X` = ZWJ makes one
/// cluster of three. Each batch starts with `RIS`, which empties the cluster
/// store, so 48 rows never fill its 63 slots, and ends on row 48, short of the
/// scroll.
#[test]
fn the_table_read_writes_what_the_computing_path_writes() {
    let perms = permutations();
    let contexts: [(&str, &[u8], &[u8]); 4] = [
        ("X, space, at column 0", b"", b" "),
        ("space, X, space", b" ", b" "),
        ("a, X, b", b"a", b"b"),
        (
            "woman, X, man",
            "\u{1F469}".as_bytes(),
            "\u{1F468}".as_bytes(),
        ),
    ];
    assert_eq!(usize::from(NARROW.ws_row), ROWS);
    let batch_rows = ROWS - 2;
    let mut computed = Owned::new(&NARROW);
    let mut indexed = Owned::with_cell_table(&NARROW);
    let mut a = Term::new(computed.storage(), &NARROW);
    let mut b = Term::new(indexed.storage(), &NARROW);
    let mut report = Vec::new();
    for (name, before, after) in contexts {
        let mut red: Vec<[u8; 3]> = Vec::new();
        let mut cursor_red = 0usize;
        let mut line = Vec::new();
        for batch in perms.chunks(batch_rows) {
            support::feed(&mut a, b"\x1bc");
            support::feed(&mut b, b"\x1bc");
            for &seq in batch {
                line.clear();
                line.extend_from_slice(before);
                line.extend_from_slice(&seq);
                line.extend_from_slice(after);
                support::feed(&mut a, &line);
                support::feed(&mut b, &line);
                // The cursor where the context leaves it, before the CR LF
                // that would put both at column 0 whatever was written.
                if a.cursor() != b.cursor() {
                    cursor_red += 1;
                }
                support::feed(&mut a, b"\r\n");
                support::feed(&mut b, b"\r\n");
            }
            // The layout check: every row ended in CR LF and none wrapped.
            assert_eq!(a.cursor(), (Row::from_index(batch.len()), Col::ZERO));
            for (r, &seq) in batch.iter().enumerate() {
                if a.row(Row::from_index(r)) != b.row(Row::from_index(r)) {
                    red.push(seq);
                }
            }
            if support::screen(&a) != support::screen(&b) && red.is_empty() {
                red.push(batch[0]);
            }
        }
        if !red.is_empty() || cursor_red != 0 {
            report.push(format!(
                "{name}: {} sequences on a different row, {cursor_red} with the cursor apart, first {:02x?}",
                red.len(),
                &red[..red.len().min(4)]
            ));
        }
    }
    assert!(report.is_empty(), "{}", report.join("\n"));
}

/// **How much of the write table would a pointer array save?**
///
/// The proposal is to intern the entries -- one array of the distinct cells,
/// and the table holds an index into it -- on the expectation that the same
/// cell is named by many different byte strings. This counts the duplicates
/// rather than assuming them, and it counts *rows* too, because a two-level
/// table whose rows dedupe is the other half of the same idea.
///
/// It reads the table directly and needs no oracle at all: the entries are a
/// function of the property tables this crate already carries. Both numbers
/// are asserted, so a change in the entry's shape has to come past this.
#[test]
fn the_write_table_has_almost_nothing_to_intern() {
    let mut owned = Owned::with_cell_table(&support::CORPUS);
    let _ = Term::new(owned.storage(), &support::CORPUS);
    let table = owned.cell_table();

    let word = |c: PackedCell| -> u64 {
        let b: [u8; 8] = zerocopy::IntoBytes::as_bytes(&c)
            .try_into()
            .expect("PackedCell is eight bytes");
        u64::from_le_bytes(b)
    };

    // Only the 64 reachable entries of each row: `b2` indexes 256 slots where
    // only `0x80..=0xBF` can occur, so the other 192 are `EMPTY` in every row
    // and discriminate nothing.
    let mut words: Vec<u64> = Vec::with_capacity(CELLS3_PREFIXES * 64);
    let mut rows: HashSet<Vec<u64>> = HashSet::new();
    for prefix in 0..CELLS3_PREFIXES {
        let row: Vec<u64> = (0x80..=0xBF)
            .map(|b2| word(table[prefix * 256 + b2]))
            .collect();
        words.extend_from_slice(&row);
        rows.insert(row);
    }
    let distinct: HashSet<u64> = words.iter().copied().collect();

    // **93.8% of the entries are unique, so there is almost nothing to
    // intern.** A cell carries the codepoint and the codepoint *is* the
    // concatenation of the index bytes, so two entries can only agree when at
    // least one of them does not decode to its own value -- the overlongs
    // `E0 80..9F xx` and the surrogates `ED A0..BF xx`, 2,048 sequences each,
    // which all become U+FFFD. Buying the 6.2% costs an indirection: index then
    // array is two dependent loads where the value inline is one.
    //
    // **It is 4,097 collapsing to one and not 4,096, and the extra one is
    // `EF BF BD`** -- the well-formed encoding of U+FFFD itself, which lands on
    // the same entry as every malformed sequence. So the table cannot tell
    // "the stream sent a replacement character" from "the stream was
    // malformed", and neither can a renderer reading the cell. That is a
    // property of writing the decoded codepoint rather than the bytes, it is
    // what `Term::replacements` counts separately for, and it is the shape of
    // collision an interning scheme would be sharing a slot over.
    assert_eq!(words.len(), 65_536, "valid three-byte sequences");
    assert_eq!(distinct.len(), 61_440, "distinct entries");
    // Rows dedupe for exactly the same reason and no other: the 64 prefixes
    // that are entirely replacement characters are one row, and the remaining
    // 960 each name 64 codepoints nothing else names.
    assert_eq!(rows.len(), 961, "distinct 64-entry rows of 1024");
}
