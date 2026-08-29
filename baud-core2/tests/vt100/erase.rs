//! ED and EL — and the cursor-column split both of them get wrong easily.
//!
//! `[[source: ecma48]]` § 8.3.39 (ED): for `Ps = 0`, "the active presentation
//! position **and the character positions up to the end of the page** are put
//! into the erased state"; for `Ps = 1`, "the character positions from the
//! beginning of the page **up to and including** the active presentation
//! position". § 8.3.41 (EL) says the same for one line.
//!
//! **This was a real bug and it survived a full VT test suite.** `erase_display`
//! erased whole rows, so `CSI 0 J` with the cursor mid-line destroyed the text
//! to its *left* as well. Every existing ED test happened to place the cursor at
//! column 0, where erasing the row and erasing from the cursor are the same
//! thing — so none of them could see it. It was found by reading the spec while
//! implementing VT52's `ESC J`, which is the same operation.
//!
//! Every test here therefore erases from a **non-zero column**.

use baud_core2::*;

fn run(bytes: &[u8]) -> Vec<PackedCell> {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    t.viewport().to_vec()
}

/// Row `n`, first `w` cells, as text.
fn row(cells: &[PackedCell], n: usize, w: usize) -> String {
    (0..w)
        .map(|i| {
            cells[n * 2048 + i]
                .content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or('.')
        })
        .collect()
}

/// `CSI 0 J` keeps everything left of the cursor on the cursor's own row.
#[test]
fn ed_to_end_spares_the_text_left_of_the_cursor() {
    let g = run(b"abcdef\x1b[1;3H\x1b[0J");
    assert_eq!(row(&g, 0, 6), "ab....", "ab is before the cursor and stays");
}

/// `CSI 1 J` erases up to *and including* the cursor, and no further.
#[test]
fn ed_to_start_includes_the_cursor_and_stops() {
    let g = run(b"abcdef\x1b[1;3H\x1b[1J");
    assert_eq!(row(&g, 0, 6), "...def", "a, b and c go; d onward stays");
}

/// `CSI 2 J` erases the whole page regardless of the cursor.
#[test]
fn ed_all_ignores_the_cursor() {
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[2J"), 0, 6), "......");
}

/// Rows away from the cursor are erased whole, in the right direction.
#[test]
fn ed_erases_whole_rows_on_the_correct_side() {
    // Cursor on row 1, mid-line.
    let g = run(b"aaa\r\nbbb\r\nccc\x1b[2;2H\x1b[0J");
    assert_eq!(row(&g, 0, 3), "aaa", "row above survives ToEnd");
    assert_eq!(row(&g, 1, 3), "b..", "cursor row split at the cursor");
    assert_eq!(row(&g, 2, 3), "...", "row below erased");

    let h = run(b"aaa\r\nbbb\r\nccc\x1b[2;2H\x1b[1J");
    assert_eq!(row(&h, 0, 3), "...", "row above erased by ToStart");
    assert_eq!(row(&h, 1, 3), "..b", "cursor row split, cursor included");
    assert_eq!(row(&h, 2, 3), "ccc", "row below survives");
}

/// EL has the same split, and always had it right — asserted so a future
/// change cannot regress ED and EL independently.
#[test]
fn el_splits_at_the_cursor_too() {
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[0K"), 0, 6), "ab....");
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[1K"), 0, 6), "...def");
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[2K"), 0, 6), "......");
}

/// The selective erases split at the cursor for the same reason, and had the
/// same bug.
#[test]
fn selective_ed_splits_at_the_cursor() {
    // Nothing protected, so DECSED must match plain ED exactly.
    assert_eq!(
        run(b"abcdef\x1b[1;3H\x1b[?0J"),
        run(b"abcdef\x1b[1;3H\x1b[0J")
    );
    assert_eq!(
        run(b"abcdef\x1b[1;3H\x1b[?1J"),
        run(b"abcdef\x1b[1;3H\x1b[1J")
    );
}

/// Protected text left of the cursor survives a selective erase-to-end, which
/// is two independent reasons for a cell to be spared at once.
#[test]
fn selective_ed_respects_both_the_cursor_and_protection() {
    let g = run(b"\x1b[1\"qab\x1b[0\"qcdef\x1b[1;3H\x1b[?0J");
    assert_eq!(row(&g, 0, 6), "ab....", "protected and before the cursor");
}

/// Erasing behaves the same however the reads are split.
#[test]
fn erase_is_chunk_invariant() {
    let seq = b"abcdef\r\nghijkl\x1b[1;3H\x1b[0J";
    let whole = run(seq);
    for chunk in 1..seq.len() {
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&seq[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
        assert_eq!(t.viewport().to_vec(), whole, "differed at chunk {chunk}");
    }
}

// ── the split has one definition now ────────────────────────────────────────

/// ED and DECSED must agree on where the cursor's row is split.
///
/// They computed it independently, and the ECMA-48 § 8.3.39 bug above had to be
/// fixed in each of them separately — the second one only because someone
/// remembered it existed. `EraseExtent::full_rows` / `cursor_span` is now the
/// single definition and this asserts the two stay on it.
///
/// With nothing protected, DECSED erases exactly what ED erases: DECSCA decides
/// *which* cells are eligible, not *where* the boundary falls.
#[test]
fn selective_erase_splits_the_row_where_plain_erase_does() {
    for ps in [b'0', b'1', b'2'] {
        let mut plain = b"abcdefghij\x1b[1;5H\x1b[".to_vec();
        plain.push(ps);
        plain.push(b'J');
        let mut selective = b"abcdefghij\x1b[1;5H\x1b[?".to_vec();
        selective.push(ps);
        selective.push(b'J');
        assert_eq!(
            run(&plain),
            run(&selective),
            "CSI {} J and CSI ? {} J must split the cursor's row identically",
            ps as char,
            ps as char
        );
    }
}

/// Erase-to-start with a pending wrap covers the whole row.
///
/// With autowrap on, the cursor legally sits one past the last column — the
/// deferred-wrap sentinel — so "up to and including the active position" is the
/// entire row, and `CSI 1 J` must land exactly where `CSI 2 K` does.
///
/// Be precise about what this does **not** prove. `ToStart` ends at
/// `cx.after()`, which is `cols + 1` here; two of the three call sites clamped
/// that and `erase_display` did not. The difference was invisible, because
/// `View::fill` clamps `to` again underneath — a downstream guard quietly
/// covering an upstream inconsistency, which is why it survived. No test could
/// have caught it from the outside. Consolidating into `line_span` is what
/// fixes it: one definition, clamped once, and `fill`'s clamp stops being
/// load-bearing for anything but its own bounds.
#[test]
fn erase_to_start_with_a_pending_wrap_covers_the_whole_row() {
    // `CSI 9999 G` clamps to the last column, so this needs no grid width:
    // one more character consumes that cell and leaves the wrap pending.
    let head = b"\x1b[?7h\x1b[9999Gx";
    let go = |tail: &[u8]| {
        let mut b = head.to_vec();
        b.extend_from_slice(tail);
        run(&b)
    };
    assert_eq!(
        go(b"\x1b[1J"),
        go(b"\x1b[2K"),
        "with the wrap pending, erase-to-start covers the whole row"
    );
}
