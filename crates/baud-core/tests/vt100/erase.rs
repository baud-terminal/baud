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

use crate::support;
use baud_core::*;

/// The viewport **and the stride it is laid out at**.
///
/// `baud-core2`'s grid strode by `MAX_COLS`, so its suites indexed
/// `cells[n * 2048 + i]`. This core strides by `Term::stride`, a pitch fixed
/// from the provisioned width -- leaving 2048 was worth +14% on every payload
/// that scrolls, because at 2048 cells a row consecutive rows landed in the
/// same L1 sets. A ported test that kept the constant reads row 10 where it
/// means row 1, so the stride travels with the cells.
fn run(bytes: &[u8]) -> (Vec<PackedCell>, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let w = t.stride().index();
    (support::screen(&t), w)
}

/// Row `n`, first `w` cells, as text.
fn row(g: &(Vec<PackedCell>, usize), n: usize, w: usize) -> String {
    let (cells, stride) = g;
    (0..w)
        .map(|i| {
            cells[n * stride + i]
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
/// same bug. Nothing protected, so DECSED takes the cells ED takes, each
/// a space (`[[source: dec-std-070]]` p. 5-162) where ED leaves it empty.
#[test]
fn selective_ed_splits_at_the_cursor() {
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[?0J"), 0, 7), "ab    .");
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[0J"), 0, 7), "ab.....");
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[?1J"), 0, 7), "   def.");
    assert_eq!(row(&run(b"abcdef\x1b[1;3H\x1b[1J"), 0, 7), "...def.");
}

/// Protected text left of the cursor survives a selective erase-to-end, which
/// is two independent reasons for a cell to be spared at once.
#[test]
fn selective_ed_respects_both_the_cursor_and_protection() {
    let g = run(b"\x1b[1\"qab\x1b[0\"qcdef\x1b[1;3H\x1b[?0J");
    assert_eq!(row(&g, 0, 6), "ab    ", "protected and before the cursor");
}

/// Erasing behaves the same however the reads are split.
#[test]
fn erase_is_chunk_invariant() {
    let seq = b"abcdef\r\nghijkl\x1b[1;3H\x1b[0J";
    let whole = run(seq);
    for chunk in 1..seq.len() {
        let ws = support::CORPUS;
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        let mut i = 0;
        while i < seq.len() {
            let n = chunk.min(seq.len() - i);
            t.write_at(BytePos::ZERO, &seq[i..i + n]);
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            i += n;
        }
        assert_eq!(support::screen(&t), whole.0, "differed at chunk {chunk}");
    }
}

// ── the split has one definition now ────────────────────────────────────────

/// ED and DECSED must agree on where the cursor's row is split.
///
/// They computed it independently, and the ECMA-48 § 8.3.39 bug above had to be
/// fixed in each of them separately — the second one only because someone
/// remembered it existed. `line_span` and `display_rows` in `ops.rs` are now
/// the single definition, for ED, EL, DECSED and DECSEL alike, and this
/// catches either side being given its own arithmetic again. A fault in the
/// shared definition moves both sides together; the exact-screen tests
/// (`ed_erases_whole_rows_on_the_correct_side`, `el_splits_at_the_cursor_too`,
/// `selective_ed_splits_at_the_cursor`) catch that.
///
/// With nothing protected, DECSED erases exactly the cells ED erases: DECSCA
/// decides *which* cells are eligible, not *where* the boundary falls. DECSED
/// leaves a space where ED leaves an empty cell, so the rows are compared
/// with its spaces read as empty.
#[test]
fn selective_erase_splits_the_row_where_plain_erase_does() {
    for ps in *b"012" {
        let mut plain = b"abcdefghij\x1b[1;5H\x1b[".to_vec();
        plain.push(ps);
        plain.push(b'J');
        let mut selective = b"abcdefghij\x1b[1;5H\x1b[?".to_vec();
        selective.push(ps);
        selective.push(b'J');
        assert_eq!(
            row(&run(&plain), 0, 20),
            row(&run(&selective), 0, 20).replace(' ', "."),
            "CSI {} J and CSI ? {} J must split the cursor's row identically",
            ps as char,
            ps as char
        );
    }
}

/// Erase-to-start with a pending wrap covers the whole row.
///
/// With autowrap on, a pending wrap leaves the active position on the last
/// column (`[[source: dec-std-070]]` D.6.1), so "up to and including the
/// active position" is the entire row, and `CSI 1 J` must land exactly where
/// `CSI 2 K` does.
///
/// Be precise about what this does **not** prove. Each erase ends the wrap
/// on the last column before it takes its span (`Paint::end_wait_erasing`),
/// so `ToStart` ends at `cols`; until 2026-11-01 it took the sentinel one
/// past the row and ended at `cols + 1`, which the grid clamped
/// (`View::snap`). What keeps every erase form on the same span is that ED,
/// EL, DECSED and DECSEL take it from the one `line_span`.
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

/// The screen's rows as text, an empty position read as `.`, and how many
/// parameters were counted as not acted on.
fn listed(bytes: &[u8]) -> (Vec<String>, usize, usize) {
    support::with_corpus(|t| {
        support::feed(t, bytes);
        let rows = (0..3)
            .map(|y| {
                t.row(Row::from_index(y)).unwrap()[..6]
                    .iter()
                    .map(|c| {
                        c.content
                            .as_codepoint()
                            .and_then(|c| char::from_u32(c.value()))
                            .unwrap_or('.')
                    })
                    .collect()
            })
            .collect();
        (rows, t.unsupported_param, t.scrollback().len())
    })
}

/// **Every parameter of ED and EL, in order**: `[[source: dec-std-070]]`'s
/// algorithms loop `for (n=0; n<parc; n++) switch(parv[n])` (EL p. 5-155,
/// ED p. 5-157), so `CSI 1 ; 0 K` erases from the line's start through the
/// cursor and then from the cursor to its end. Reading the first alone left
/// `def`.
#[test]
fn ed_and_el_apply_every_parameter_in_order() {
    let (rows, ..) = listed(b"abcdef\x1b[1;3H\x1b[1;0K");
    assert_eq!(rows[0], "......");
    let (rows, ..) = listed(b"aaa\r\nbbb\r\nccc\x1b[2;2H\x1b[0;1J");
    assert_eq!(rows, ["......", "......", "......"]);
    // The control: each alone leaves its other side.
    let (rows, ..) = listed(b"abcdef\x1b[1;3H\x1b[1K");
    assert_eq!(rows[0], "...def");
}

/// **A value with no case is skipped and the loop goes on**, as the
/// algorithms' `switch` has no default: `CSI 5 ; 1 K` erases through the
/// cursor and counts the `5`. A lone `5` is still refused whole.
#[test]
fn a_value_with_no_case_is_skipped_and_counted() {
    let (rows, skipped, _) = listed(b"abcdef\x1b[1;3H\x1b[5;1K");
    assert_eq!((rows[0].as_str(), skipped), ("...def", 1));
    let (rows, skipped, _) = listed(b"abcdef\x1b[1;3H\x1b[;9;2K");
    assert_eq!((rows[0].as_str(), skipped), ("......", 1));
}

/// **A `3` in a list is skipped**: the algorithms' `switch` has no case
/// 3, and xterm's "Erase Saved Lines" is the value of a lone parameter.
/// `CSI 3 ; 1 J` keeps the history and erases through the cursor; a lone
/// `CSI 3 J` still erases the history.
#[test]
fn a_three_in_a_list_is_skipped() {
    let scroll: Vec<u8> = (0..60)
        .flat_map(|i| format!("{i}\r\n").into_bytes())
        .collect();
    let (_, _, kept) = listed(&scroll);
    assert!(kept > 0, "the control: lines went behind the screen");
    let (_, skipped, after) = listed(&[scroll.as_slice(), b"\x1b[3;1J"].concat());
    assert_eq!((skipped, after), (1, kept));
    let (_, _, after) = listed(&[scroll.as_slice(), b"\x1b[3J"].concat());
    assert_eq!(after, 0);
}
