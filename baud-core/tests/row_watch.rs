//! **The watch on a selection's rows: each move in place judged by the rows
//! it moved.**
//!
//! An embedder holding a selection on the live screen arms
//! [`Term::watch_rows`] on the rows it names before a read, and asks
//! [`Term::watched_rows_changed`] after it whether `IL`, `DL` or a region
//! scroll that copies moved one of them, a scroll of a region anchored at
//! the top advanced the ring over one under it, which stayed, or a line
//! taken back erased one -- or whether the surface moved further than its
//! rotation can say. It used to ask
//! whether the count of such moves had moved and judge by the region the
//! read ended in, which a region set and set back inside one read hid: begun
//! in 8;9, copied in 4;6 and ended in 8;9, the read was judged as one in 8;9
//! and the selection on the copied rows stayed while their text went up a
//! row -- and fed in two reads, the same bytes dropped a selection on rows
//! nothing moved.
//!
//! What is held here:
//!
//! * one test a mechanism: each move's rows and both sides of its band, and
//!   the rows under a top region's scroll on either screen, numbered through
//!   the lines retired, or taken back, before the move in the same read; the
//!   rows a line taken back erases -- the top row and the row given up below
//!   by `SD`, also past the live edge, and a redraw mark's block; a read that
//!   moves the live surface past half its ring, on either screen and at line
//!   zero; the arm's reset; and a watch on no rows;
//! * the answer is the same however the bytes are cut into reads -- whole,
//!   one byte to a read, and at random cuts -- on generated programs that
//!   set regions and set them back, move rows in two bands in one read,
//!   `IL` and `DL` with no region, `RI` and `SD` in a partial region, scroll
//!   a region anchored at the top, and retire lines before and after a move,
//!   with the selection renumbered between reads as an embedder renumbers
//!   it.

use baud_core::*;
use proptest::prelude::*;

const WS: WinSize = WinSize {
    ws_col: 20,
    ws_row: 10,
};

fn with_term<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(30));
    let mut t = Term::new(owned.storage(), &WS);
    f(&mut t)
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// Screen rows `first` to `last`, zero-based.
fn rows(first: usize, last: usize) -> Option<(Row, Row)> {
    Some((Row::from_index(first), Row::from_index(last)))
}

/// Arms the watch on `sel`, feeds `bytes` as one read, and says whether a
/// move in place reached it.
fn moved(t: &mut Term<'_>, sel: Option<(Row, Row)>, bytes: &[u8]) -> bool {
    t.watch_rows(sel);
    feed(t, bytes);
    t.watched_rows_changed()
}

/// **A region scroll that copies moves the region's rows and no others.**
/// A line feed at the bottom of a middle region, 4;6 (rows 3 to 5): a
/// selection on its top row or its bottom row is reached, one on the row
/// above it or under it is not.
#[test]
fn a_copying_scroll_up_reaches_the_regions_rows_and_only_those() {
    let prog = b"\x1b[4;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (rows(3, 3), true, "the region's top row"),
        (rows(5, 5), true, "its bottom row"),
        (rows(2, 2), false, "the row above it"),
        (rows(6, 6), false, "the row under it"),
        (rows(0, 9), true, "a selection across it"),
    ] {
        with_term(|t| {
            let count = t.row_moves();
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_ne!(t.row_moves(), count, "the control: a row moved");
        });
    }
}

/// **`RI` at the top of a middle region copies the region's rows down.**
#[test]
fn a_copying_scroll_down_reaches_the_regions_rows_and_only_those() {
    let prog = b"\x1b[4;6r\x1b[4;1H\x1bM";
    for (sel, want, what) in [
        (rows(3, 3), true, "the region's top row"),
        (rows(5, 5), true, "its bottom row"),
        (rows(2, 2), false, "the row above it"),
        (rows(6, 6), false, "the row under it"),
    ] {
        with_term(|t| assert_eq!(moved(t, sel, prog), want, "{what}"));
    }
}

/// **`IL` moves the rows from the cursor to the region's bottom**: with no
/// region, from the cursor to the screen's. The row above the cursor keeps
/// its text, so a selection there is not reached.
#[test]
fn il_reaches_the_rows_from_the_cursor_down() {
    let prog = b"\x1b[6;1H\x1b[L";
    for (sel, want, what) in [
        (rows(5, 5), true, "the cursor's row"),
        (rows(9, 9), true, "the screen's last row"),
        (rows(4, 4), false, "the row above the cursor"),
    ] {
        with_term(|t| assert_eq!(moved(t, sel, prog), want, "{what}"));
    }
}

/// **`DL` the same**, inside a region: from the cursor to the region's
/// bottom, and not the row under it.
#[test]
fn dl_reaches_the_rows_from_the_cursor_to_the_regions_bottom() {
    let prog = b"\x1b[3;8r\x1b[5;1H\x1b[M";
    for (sel, want, what) in [
        (rows(4, 4), true, "the cursor's row"),
        (rows(7, 7), true, "the region's last row"),
        (rows(3, 3), false, "the row above the cursor"),
        (rows(8, 8), false, "the row under the region"),
    ] {
        with_term(|t| assert_eq!(moved(t, sel, prog), want, "{what}"));
    }
}

/// **A scroll of a region anchored at the top reaches the rows under it.**
/// The scroll advances the ring and the rows under the region stay, so a
/// selection numbered by the lines retired would be a row up, on text that
/// is not its own. A line feed at the bottom of 1;5 (rows 0 to 4) on the
/// primary: a selection on the first row under it or the last is reached,
/// one on the region's rows, which moved with the ring, is not. It is not a
/// move in place: the count of those holds, and the region-scroll count
/// moves.
#[test]
fn a_top_regions_scroll_reaches_the_rows_under_it() {
    let prog = b"\x1b[1;5r\x1b[5;1H\n";
    for (sel, want, what) in [
        (rows(5, 5), true, "the first row under the region"),
        (rows(9, 9), true, "the screen's last row"),
        (rows(3, 6), true, "a selection across the region's bottom"),
        (rows(4, 4), false, "the region's bottom row"),
        (rows(1, 4), false, "the region's rows"),
    ] {
        with_term(|t| {
            let (moves, regions, rot) = (t.row_moves(), t.region_scrolls(), t.rotation());
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(
                t.moved_since(rot),
                Moved::Retired(Rows::of_count(1)),
                "the control: the ring advanced"
            );
            assert_ne!(t.region_scrolls(), regions, "through a region");
            assert_eq!(t.row_moves(), moves, "and moved no row in place");
        });
    }
}

/// **On the alternate screen the same**, where a top region rotates when it
/// is taller than the rows under it: 1;6 (rows 0 to 5) over four rows.
#[test]
fn a_top_regions_scroll_reaches_the_rows_under_it_on_the_alternate_screen() {
    let prog = b"\x1b[1;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (rows(6, 6), true, "the first row under the region"),
        (rows(9, 9), true, "the screen's last row"),
        (rows(5, 5), false, "the region's bottom row"),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[?1049h");
            let (regions, rot) = (t.region_scrolls(), t.rotation());
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(1)));
            assert_ne!(t.region_scrolls(), regions, "the control: it rotated");
        });
    }
}

/// **Whatever region the read ends in.** Begun in 1;9 and scrolled in 1;5,
/// then a copy in 8;9 and back to 1;9, or straight back to 1;9: the rows
/// that were under 1;5 are reached, the region's rows are not -- the copy's
/// rows, the arm's 8 and 9 numbered a row on, are under 1;5 as well.
#[test]
fn a_top_regions_scroll_is_judged_by_its_own_rows_whatever_the_read_ends_in() {
    for prog in [
        &b"\x1b[1;5r\x1b[5;1H\n\x1b[8;9r\x1b[9;1H\n\x1b[1;9r"[..],
        &b"\x1b[1;5r\x1b[5;1H\n\x1b[1;9r"[..],
    ] {
        for (sel, want, what) in [
            (rows(5, 5), true, "the arm's row 5, under 1;5"),
            (rows(7, 7), true, "its row 7"),
            (rows(2, 2), false, "its row 2, in 1;5"),
        ] {
            with_term(|t| {
                feed(t, b"\x1b[1;9r");
                assert_eq!(moved(t, sel, prog), want, "{what}, {prog:?}");
                assert_eq!(t.scroll_region(), (Row::ZERO, Row::from_index(8)));
            });
        }
    }
}

/// **A region set and set back within one read**: begun in 8;9, copied in
/// 4;6, ended in 8;9. The copy's rows are reached and the rows of the
/// region the read began and ended in are not -- as when the three
/// sequences arrive in three reads.
#[test]
fn a_region_set_and_set_back_within_one_read_is_judged_by_the_copy() {
    let prog = b"\x1b[4;6r\x1b[6;1H\n\x1b[8;9r";
    for (sel, want, what) in [
        (rows(4, 4), true, "a row the copy moved"),
        (
            rows(7, 7),
            false,
            "the end region's row, which nothing moved",
        ),
        (rows(2, 2), false, "a row above both"),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[8;9r");
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.scroll_region(), (Row::from_index(7), Row::from_index(8)));
        });
    }
}

/// **Lines retired before the move, in the same read, number its rows as
/// the selection is numbered.** Two line feeds at the bottom of the whole
/// screen retire two lines, so the arm's row 7 is row 5 when 4;6 copies:
/// reached. The arm's row 3 is row 1 then: not reached, though row 3 is the
/// region's top.
#[test]
fn lines_retired_before_the_move_number_its_rows() {
    let prog = b"\x1b[10;1H\n\n\x1b[4;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (rows(7, 7), true, "the arm's row 7, carried into the region"),
        (rows(3, 3), false, "the arm's row 3, carried above it"),
        (rows(2, 2), false, "the arm's row 2, carried above it too"),
    ] {
        with_term(|t| {
            let rot = t.rotation();
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(2)));
        });
    }
}

/// **And lines taken back before the move the same way, the other
/// direction.** `RI` at the top of the whole screen takes a line back from
/// the history, so the arm's row 3 is row 4 when 5;6 copies (rows 4 and
/// 5): reached. The arm's row 5 is row 6 then: not reached, though row 5 is
/// the region's bottom.
#[test]
fn lines_taken_back_before_the_move_number_its_rows() {
    let history: Vec<u8> = (0..15)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    let prog = b"\x1b[1;1H\x1bM\x1b[5;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (rows(3, 3), true, "the arm's row 3, carried into the region"),
        (rows(5, 5), false, "the arm's row 5, carried under it"),
    ] {
        with_term(|t| {
            feed(t, &history);
            let rot = t.rotation();
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Restored(Rows::of_count(1)));
        });
    }
}

/// **A line the whole screen's scroll down takes back is blanked on the top
/// row.** Retired by a line feed at the bottom and taken back by `SD` in the
/// same read, the arm's row 0 is the screen's top row again, blank where its
/// text was, and the read nets no movement: reached. The arm's row 1, back
/// where it was with its text, is not.
#[test]
fn a_line_taken_back_and_blanked_reaches_the_top_row() {
    let prog = b"\x1b[10;1H\n\x1b[T";
    for (sel, want, what) in [
        (rows(0, 0), true, "the arm's row 0, taken back blank"),
        (rows(1, 1), false, "the arm's row 1, back with its text"),
        (rows(5, 9), false, "the rows under it"),
    ] {
        with_term(|t| {
            feed(t, &history());
            assert_eq!(text(t, 0), "line 5", "the arm's row 0");
            let (moves, rot) = (t.row_moves(), t.rotation());
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::ZERO));
            assert_eq!(t.row_moves(), moves, "no row moved in place");
            assert_eq!((text(t, 0), text(t, 1)), (String::new(), "line 6".into()));
        });
    }
}

/// **And the row the window gives up below, which a line feed brings back
/// blank.** `SD` pushes the arm's row 9 under the window and a line feed at
/// the bottom brings it back, blanked, netting no movement: reached. The
/// arm's row 8, back where it was with its text, is not.
#[test]
fn a_line_given_up_below_and_brought_back_blank_reaches_the_last_row() {
    let prog = b"\x1b[T\x1b[10;1H\n";
    for (sel, want, what) in [
        (rows(9, 9), true, "the arm's row 9, brought back blank"),
        (rows(8, 8), false, "the arm's row 8, back with its text"),
        (rows(0, 4), false, "the rows above it"),
    ] {
        with_term(|t| {
            feed(t, &history());
            assert_eq!(text(t, 9), "line 14", "the arm's row 9");
            let rot = t.rotation();
            assert_eq!(moved(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::ZERO));
            assert_eq!((text(t, 9), text(t, 8)), (String::new(), "line 13".into()));
        });
    }
}

/// **The same where the rows may hold something past the live edge**, whose
/// scroll down takes its lines back out of line: written 40 wide, narrowed
/// to 20 at a prompt, so a row keeps its tail past the edge.
#[test]
fn a_line_taken_back_past_the_edge_reaches_the_same_rows() {
    let wide = WinSize {
        ws_col: 40,
        ws_row: 10,
    };
    for (prog, sel, want, what) in [
        (&b"\x1b[10;1H\n\x1b[T"[..], rows(0, 0), true, "the top row"),
        (
            &b"\x1b[10;1H\n\x1b[T"[..],
            rows(1, 1),
            false,
            "the row under it",
        ),
        (&b"\x1b[T\x1b[10;1H\n"[..], rows(9, 9), true, "the last row"),
        (
            &b"\x1b[T\x1b[10;1H\n"[..],
            rows(8, 8),
            false,
            "the row above it",
        ),
    ] {
        let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(30));
        let mut t = Term::new(owned.storage(), &wide);
        feed(&mut t, &history());
        feed(&mut t, format!("\x1b[5;1H{}", "S".repeat(30)).as_bytes());
        feed(&mut t, b"\x1b[10;1H\x1b]133;A\x07");
        assert_eq!(t.resize(&WS), Resized::Ok);
        assert_eq!(
            t.row_extent(Row::from_index(4)).index(),
            30,
            "the control: a tail lies past the edge"
        );
        let rot = t.rotation();
        assert_eq!(moved(&mut t, sel, prog), want, "{what}, {prog:?}");
        assert_eq!(t.moved_since(rot), Moved::Retired(Rows::ZERO));
    }
}

/// The text on screen row `y`, trailing blanks dropped.
fn text(t: &Term<'_>, y: usize) -> String {
    let s: String = t
        .row(Row::from_index(y))
        .expect("a row on the grid")
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or(' ')
        })
        .collect();
    s.trim_end().to_string()
}

/// **A redraw mark takes back the lines of its block that scrolled off and
/// erases them**, so they reach the watch as the line a scroll down takes
/// back does. The block opens on the last row with `$ ` on it and runs
/// eleven rows further, so its first two rows scrolled off; the shell homes,
/// erases the screen and marks the redraw, which moves the window back over
/// them and erases the block. The arm's row 9, the block's first, is
/// reached; its row 8, which stays in the history, is not.
#[test]
fn a_redraw_mark_reaches_the_lines_of_its_block_it_takes_back() {
    let mut prog = b"\r\n".repeat(11);
    prog.extend_from_slice(b"\x1b[H\x1b[J\x1b]133;A\x07");
    for (sel, want, what) in [
        (rows(9, 9), true, "the block's first row, taken back"),
        (rows(8, 8), false, "the row above it, left in the history"),
    ] {
        with_term(|t| {
            feed(t, &history());
            feed(t, b"\r\n\x1b]133;A\x07$ ");
            assert_eq!(text(t, 9), "$", "the block's first row");
            let rot = t.rotation();
            assert_eq!(moved(t, sel, &prog), want, "{what}");
            assert_eq!(
                t.moved_since(rot),
                Moved::Retired(Rows::of_count(9)),
                "the control: eleven retired, two taken back"
            );
        });
    }
}

/// `n` line feeds at the bottom of the screen.
fn feeds(n: usize) -> Vec<u8> {
    let mut v = b"\x1b[10;1H".to_vec();
    v.resize(v.len() + n, b'\n');
    v
}

/// **A read that moves the live surface further than its rotation can say
/// reaches the watch.** The rotation is read round the ring: up to half of
/// it retired, beyond that taken back, so more lines than that in one arm
/// are misread, and a selection numbered by them would be renumbered onto
/// rows that are not its own -- one ring and a line reads as one line. On
/// the alternate screen, whose lines `top` does not count, the watch counts
/// them: half the ring retired is not reached, a line more is, and so is the
/// ring and a line, which [`Term::moved_since`] reads as one. Taken back by
/// `SD`, a line under half the ring is not reached and half the ring is,
/// which it reads as retired. The selection is on rows no line taken back
/// blanks or pushes under the window.
#[test]
fn a_surface_moved_past_half_its_ring_reaches_the_watch_on_the_alternate_screen() {
    let n = with_term(|t| {
        feed(t, b"\x1b[?1049h");
        t.ring().index()
    });
    let half = n / 2;
    let back_half = n - half;
    let up = |k: usize| feeds(k);
    let down = |k: usize| format!("\x1b[{k}T").into_bytes();
    for (prog, sel, want, what) in [
        (up(half), rows(9, 9), false, "half the ring retired"),
        (up(half + 1), rows(9, 9), true, "a line more"),
        (up(n + 1), rows(9, 9), true, "the ring and a line"),
        (
            down(back_half - 1),
            rows(0, 0),
            false,
            "a line under half taken back",
        ),
        (
            down(back_half),
            rows(0, 0),
            true,
            "half the ring taken back",
        ),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[?1049h\x1b[10;1H");
            let rot = t.rotation();
            assert_eq!(moved(t, sel, &prog), want, "{what}, a ring of {n}");
            if what == "the ring and a line" {
                assert_eq!(
                    t.moved_since(rot),
                    Moved::Retired(Rows::of_count(1)),
                    "the control: read round the ring, one line"
                );
            }
            if what == "half the ring taken back" {
                assert!(
                    matches!(t.moved_since(rot), Moved::Retired(_)),
                    "the control: read round the ring, retired"
                );
            }
        });
    }
}

/// **On the primary the top line counts them**, read when the embedder
/// asks: half its ring of lines retired is not reached, a line more is, and
/// so is the ring and a line.
#[test]
fn a_surface_moved_past_half_its_ring_reaches_the_watch_on_the_primary() {
    let n = with_term(|t| t.ring().index());
    for (k, want, what) in [
        (n / 2, false, "half the ring retired"),
        (n / 2 + 1, true, "a line more"),
        (n + 1, true, "the ring and a line"),
    ] {
        with_term(|t| {
            let rot = t.rotation();
            assert_eq!(
                moved(t, rows(9, 9), &feeds(k)),
                want,
                "{what}, a ring of {n}"
            );
            if k == n + 1 {
                assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(1)));
            }
        });
    }
}

/// **And at line zero, where a line taken back moves no top line**, the
/// watch counts it. A history of two lines makes a ring of twelve, so half
/// of it is more lines than the screen's ten taken back, and the rows a
/// take-back blanks or pushes under the window do not reach the selection
/// on row 1 first: five taken back on a fresh terminal are not reached, six
/// are, which [`Term::moved_since`] reads as six retired.
#[test]
fn lines_taken_back_at_line_zero_are_counted() {
    for (k, want) in [(5usize, false), (6, true)] {
        let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(2));
        let mut t = Term::new(owned.storage(), &WS);
        assert_eq!(t.ring().index(), 12, "the control: a ring of twelve");
        let rot = t.rotation();
        let prog = format!("\x1b[{k}T").into_bytes();
        assert_eq!(moved(&mut t, rows(1, 1), &prog), want, "{k} taken back");
        if want {
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(6)));
        }
    }
}

/// **Erasing the history moves no line the watch counts.** `CSI 3 J` and
/// `RIS` move neither the rotation nor the primary's top line, so neither
/// reaches a watch on a live row; across a reset the embedder drops a
/// selection anyway. The control: the history was there to erase.
#[test]
fn erasing_the_history_moves_no_line_the_watch_counts() {
    for prog in [&b"\x1b[3J"[..], &b"\x1bc"[..]] {
        with_term(|t| {
            feed(t, &history());
            assert_eq!(t.scrollback().len(), 5, "the control: five saved lines");
            assert!(!moved(t, rows(5, 5), prog), "{prog:?}");
            assert_eq!(t.scrollback().len(), 0, "{prog:?} erased them");
        });
    }
}

/// Fifteen numbered lines: five in the history and ten on the screen, the
/// cursor on the bottom row.
fn history() -> Vec<u8> {
    let mut h: Vec<u8> = (0..15)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    h.truncate(h.len() - 2);
    h
}

/// **The arm starts the answer again**, and a watch on no rows -- a
/// selection wholly in the history -- is never reached.
#[test]
fn the_arm_starts_again_and_a_watch_on_no_rows_is_never_reached() {
    with_term(|t| {
        assert!(moved(t, rows(3, 5), b"\x1b[4;6r\x1b[6;1H\n"), "the control");
        assert!(
            !moved(t, rows(3, 5), b"\x1b[1;1Hx"),
            "re-armed, a write on another row"
        );
        assert!(!moved(t, None, b"\x1b[6;1H\n"), "armed on no rows");
    });
}

// ---- a write or an erase on a watched row ---------------------------------

/// Text on rows 0, 4 and 9, and the cursor parked on row 2, which no
/// selection below names.
const DRAWN: &[u8] = b"\x1b[1;1Hhello\x1b[5;1Hworld\x1b[10;1Hlast\x1b[3;1H";

/// Whether `read`, after `setup`, reaches a watch on screen rows `sel`:
/// the one answer whole, at every read size from one byte up, and cut into
/// two reads at every byte, each read armed as an embedder arms it.
fn changes(setup: &[u8], read: &[u8], sel: (usize, usize)) -> bool {
    let sel = Some(sel);
    let whole = reached_after(setup, read, sel, &[]);
    for size in 1..read.len() {
        let cuts: Vec<usize> = (size..read.len()).step_by(size).collect();
        assert_eq!(
            reached_after(setup, read, sel, &cuts),
            whole,
            "{size} bytes to a read: {read:?}"
        );
    }
    for k in 1..read.len() {
        assert_eq!(
            reached_after(setup, read, sel, &[k]),
            whole,
            "cut at {k}: {read:?}"
        );
    }
    whole
}

/// **An erase in place and a move elsewhere in one read**: `EL` on row 0
/// and `IL` on row 9. The move reaches only row 9; the erase reaches row
/// 0, and a selection on row 4, which neither touched, is not reached.
#[test]
fn an_erase_and_a_move_elsewhere_in_one_read_reach_the_erased_row() {
    let read = b"\x1b[1;1H\x1b[2K\x1b[10;1H\x1b[L";
    assert!(changes(DRAWN, read, (0, 0)), "the erased row");
    assert!(changes(DRAWN, read, (9, 9)), "the moved row");
    assert!(!changes(DRAWN, read, (4, 4)), "a row neither touched");
}

/// **A lone erase, and text written over the row**, each reach it.
#[test]
fn a_lone_erase_and_text_over_the_row_reach_it() {
    assert!(changes(DRAWN, b"\x1b[1;1H\x1b[2K", (0, 0)), "EL 2");
    assert!(changes(DRAWN, b"\x1b[1;3H\x1b[1K", (0, 0)), "EL 1");
    assert!(changes(DRAWN, b"\x1b[1;3H\x1b[K", (0, 0)), "EL 0");
    assert!(changes(DRAWN, b"\x1b[1;1HXY", (0, 0)), "XY over it");
}

/// **The same text written again reaches the row**: nothing compares what
/// a write leaves with what was there.
#[test]
fn the_same_text_written_again_reaches_the_row() {
    assert!(changes(DRAWN, b"\x1b[1;1Hhello", (0, 0)));
}

/// **`ICH`, `DCH` and `ECH` in a selected row reach it.**
#[test]
fn ich_dch_and_ech_reach_the_row() {
    assert!(changes(DRAWN, b"\x1b[1;1H\x1b[@", (0, 0)), "ICH");
    assert!(changes(DRAWN, b"\x1b[1;1H\x1b[P", (0, 0)), "DCH");
    assert!(changes(DRAWN, b"\x1b[1;1H\x1b[X", (0, 0)), "ECH");
}

/// **The control: a write, an erase or an edit on another row, and a
/// visit to the selected row that writes nothing, reach nothing.**
#[test]
fn writes_on_another_row_and_a_visit_reach_nothing() {
    let elsewhere = b"\x1b[5;1HXY\x1b[5;1H\x1b[2K\x1b[@\x1b[P\x1b[X\x1b[?2K";
    assert!(!changes(DRAWN, elsewhere, (0, 0)), "row 4's edits");
    assert!(!changes(DRAWN, b"\x1b[1;3H\x1b[3;1H", (0, 0)), "a visit");
    assert!(
        !changes(DRAWN, b"\x1b[1;30H\x1b[1;1H\r", (0, 0)),
        "moves on it"
    );
}

/// **`ED` reaches the rows it erases**: below the cursor and the cursor's
/// row from it, above the cursor and the cursor's row to it, or all.
#[test]
fn ed_reaches_the_rows_it_erases() {
    let below = b"\x1b[5;1H\x1b[J";
    assert!(!changes(DRAWN, below, (3, 3)), "ED 0, the row above");
    assert!(changes(DRAWN, below, (4, 4)), "ED 0, the cursor's row");
    assert!(changes(DRAWN, below, (9, 9)), "ED 0, the last row");
    let above = b"\x1b[5;1H\x1b[1J";
    assert!(changes(DRAWN, above, (0, 0)), "ED 1, the first row");
    assert!(changes(DRAWN, above, (4, 4)), "ED 1, the cursor's row");
    assert!(!changes(DRAWN, above, (5, 5)), "ED 1, the row under it");
    assert!(changes(DRAWN, b"\x1b[2J", (5, 5)), "ED 2");
}

/// **`DECSEL` and `DECSED` the same**, protected cells or not.
#[test]
fn selective_erases_reach_the_rows_they_erase() {
    assert!(changes(DRAWN, b"\x1b[1;1H\x1b[?2K", (0, 0)), "DECSEL");
    let below = b"\x1b[5;1H\x1b[?J";
    assert!(!changes(DRAWN, below, (3, 3)), "DECSED 0, the row above");
    assert!(changes(DRAWN, below, (9, 9)), "DECSED 0, the last row");
}

/// **`DECIC` and `DECDC` reach the region's rows, and `DECALN` the
/// screen's.**
#[test]
fn column_edits_and_decaln_reach_their_rows() {
    let ic = b"\x1b[4;6r\x1b[5;1H\x1b['}";
    assert!(changes(DRAWN, ic, (4, 4)), "DECIC, in the region");
    assert!(!changes(DRAWN, ic, (9, 9)), "DECIC, under it");
    assert!(changes(DRAWN, b"\x1b[5;1H\x1b['~", (9, 9)), "DECDC");
    assert!(changes(DRAWN, b"\x1b#8", (4, 4)), "DECALN");
}

/// **Every writer reaches the row**: a run long enough for the vectored
/// store, a run of two-byte and of wide characters, `REP`, a write under
/// `IRM`, a mark joined to the last character on the row, and a `U+FE0F`
/// whose base, in the last column, moves whole to the next row -- which
/// reaches that row.
#[test]
fn every_writer_reaches_the_row() {
    for (read, what) in [
        ("\x1b[1;1H0123456789abcdefgh", "a long ASCII run"),
        ("\x1b[1;1H\u{e9}\u{e9}\u{e9}\u{e9}", "two-byte characters"),
        ("\x1b[1;1H\u{4e2d}\u{6587}\u{5b57}", "wide characters"),
        ("\x1b[1;1H\x1b[3b", "REP"),
        ("\x1b[4h\x1b[1;1Hz\x1b[4l", "IRM"),
    ] {
        assert!(changes(DRAWN, read.as_bytes(), (0, 0)), "{what}");
        assert!(!changes(DRAWN, read.as_bytes(), (1, 1)), "{what}: row 1");
    }
    let base = b"\x1b[1;1He";
    assert!(changes(base, "\u{301}".as_bytes(), (0, 0)), "a mark");
    assert!(
        !changes(base, "\u{301}".as_bytes(), (1, 1)),
        "a mark: row 1"
    );
    let margin = "\x1b[1;20H\u{2764}".as_bytes();
    let vs16 = "\u{fe0f}".as_bytes();
    assert!(changes(margin, vs16, (1, 1)), "the base moved to row 1");
    assert!(!changes(margin, vs16, (2, 2)), "and not row 2");
}

/// **A write after a pending wrap lands on the next row**: row 0 filled to
/// its last column holds the wrap, and the `X` after it is row 1's, so a
/// selection on row 0 is not reached and one on row 1 is. With `DECAWM`
/// reset there is no wrap and the `X` overwrites row 0's last column.
#[test]
fn a_write_after_a_pending_wrap_reaches_the_next_row_only() {
    let full = b"\x1b[1;1H0123456789abcdefghij";
    assert!(!changes(full, b"X", (0, 0)), "the row holding the wrap");
    assert!(changes(full, b"X", (1, 1)), "the row the X lands on");
    let pinned = b"\x1b[?7l\x1b[1;1H0123456789abcdefghij";
    assert!(
        changes(pinned, b"X", (0, 0)),
        "DECAWM reset: the last column"
    );
    assert!(!changes(pinned, b"X", (1, 1)), "DECAWM reset: not row 1");
}

/// **A vertical scroll under side margins moves the rectangle's rows in
/// place**: `SU` between `DECSLRM`'s margins reaches the rows between the
/// top and bottom margins and not a row under them.
#[test]
fn a_scroll_under_side_margins_reaches_the_rectangles_rows() {
    let read = b"\x1b[?69h\x1b[1;10s\x1b[4;6r\x1b[S";
    assert!(changes(DRAWN, read, (4, 4)), "a row of the rectangle");
    assert!(
        !changes(DRAWN, read, (8, 8)),
        "a row under the bottom margin"
    );
}

/// **A change of columns clears the screen**, `?3` behind `?40`, so it
/// reaches every row.
#[test]
fn a_column_change_reaches_every_row() {
    assert!(changes(DRAWN, b"\x1b[?40h\x1b[?3h", (4, 4)));
}

/// **`?1040`, xterm's keepSelection, is not recognised**: `DECSET` counts
/// it as unsupported and `DECRQM` answers 0. What a selection does when a
/// program writes on its rows is the one thing above, whatever was sent.
#[test]
fn keep_selection_is_not_recognised() {
    with_term(|t| {
        let before = t.unsupported_param;
        feed(t, b"\x1b[?1040h\x1b[?1040l");
        assert_eq!(t.unsupported_param, before + 2, "DECSET and DECRST counted");
        t.clear_reply();
        feed(t, b"\x1b[?1040$p");
        assert_eq!(t.reply(), b"\x1b[?1040;0$y");
    });
    let kept = [b"\x1b[?1040h".as_slice(), DRAWN].concat();
    assert!(changes(&kept, b"\x1b[1;1Hhello", (0, 0)));
}

// ---- the same answer whatever the reads ----------------------------------

/// What a program is made of. `RI` and `SD` come with a partial region set
/// first, so no program scrolls the whole screen down, which takes lines
/// back: the app drops a selection on a read whose net movement takes lines
/// back, judged at the read's two ends, so with one in a program the answer
/// would depend on the reads -- `ESC [ 2 S ESC [ 2 T` leaves a selection on
/// row 5 unreached read whole, and split after `ESC [ 2 S` the second read
/// takes two lines back. The rows a line taken back erases are held by
/// their own tests above. Text, `EL`, `ED`, `ICH`, `DCH` and `ECH` reach
/// the rows they land on.
const ATOMS: &[&[u8]] = &[
    b"\x1b[4;6r",
    b"\x1b[8;9r",
    b"\x1b[2;9r",
    b"\x1b[1;3r",
    b"\x1b[r",
    b"\x1b[6;1H",
    b"\x1b[9;1H",
    b"\x1b[3;1H",
    b"\x1b[10;1H",
    b"\n",
    b"\r\n",
    b"abc",
    b"\x1b[L",
    b"\x1b[2M",
    b"\x1b[S",
    b"\x1b[4;6r\x1b[4;1H\x1bM",
    b"\x1b[2;5r\x1b[T",
    b"\x1b[r\x1b[10;1H\n\n",
    b"\x1b[2K",
    b"\x1b[J",
    b"\x1b[1J",
    b"\x1b[@",
    b"\x1b[P",
    b"\x1b[X",
    b"0123456789abcdef",
    "\u{4e2d}\u{6587}e\u{301}".as_bytes(),
];

/// A program, the selection's rows when the first read is armed, and where
/// to cut the bytes into reads.
fn case() -> impl Strategy<Value = (Vec<usize>, Option<(usize, usize)>, Vec<usize>)> {
    (
        prop::collection::vec(0..ATOMS.len(), 1..24),
        prop_oneof![
            1 => Just(None),
            6 => (0usize..10, 0usize..10).prop_map(|(a, b)| Some((a.min(b), a.max(b)))),
        ],
        prop::collection::vec(0usize..400, 0..6),
    )
}

/// Feeds `bytes` in the reads `cuts` makes of it, as an embedder does:
/// armed on its selection's live rows before each read, and the selection
/// renumbered after it by the lines the live surface retired -- rows that
/// left the top are history, which no move in place reaches. Whether any
/// read reached it.
fn reached(bytes: &[u8], sel: Option<(usize, usize)>, cuts: &[usize]) -> bool {
    reached_after(b"", bytes, sel, cuts)
}

/// [`reached`], after `setup` fed before the first arm.
fn reached_after(setup: &[u8], bytes: &[u8], sel: Option<(usize, usize)>, cuts: &[usize]) -> bool {
    with_term(|t| {
        feed(t, setup);
        let mut sel = sel;
        let mut at = 0;
        let mut ends: Vec<usize> = cuts.iter().map(|c| c % (bytes.len() + 1)).collect();
        ends.push(bytes.len());
        ends.sort_unstable();
        for end in ends {
            let rot = t.rotation();
            t.watch_rows(sel.map(|(a, b)| (Row::from_index(a), Row::from_index(b))));
            feed(t, &bytes[at..end]);
            at = end;
            if t.watched_rows_changed() {
                return true;
            }
            let Moved::Retired(n) = t.moved_since(rot) else {
                panic!("no program takes lines back");
            };
            sel = sel.and_then(|(a, b)| {
                let n = n.index();
                (b >= n).then(|| (a.saturating_sub(n), b - n))
            });
        }
        false
    })
}

fn bytes_of(prog: &[usize]) -> Vec<u8> {
    prog.iter()
        .flat_map(|&k| ATOMS[k].iter().copied())
        .collect()
}

proptest! {
    /// **The answer is the bytes', not the reads'**: whole, one byte to a
    /// read, and cut anywhere, the same.
    #[test]
    fn the_answer_is_the_same_however_the_reads_fall((prog, sel, cuts) in case()) {
        let bytes = bytes_of(&prog);
        let whole = reached(&bytes, sel, &[]);
        let ones: Vec<usize> = (1..bytes.len()).collect();
        prop_assert_eq!(whole, reached(&bytes, sel, &ones), "one byte to a read");
        prop_assert_eq!(whole, reached(&bytes, sel, &cuts), "cut at {:?}", cuts);
    }
}

/// **The generator reaches the shapes the property is for**, counted over a
/// fixed sample: two moves in one read in bands apart, a region set and set
/// back around a move, `IL` or `DL` with no region, lines retired before a
/// move in one read, and a scroll of a region anchored at the top; and both
/// answers. A property whose programs
/// never built them would pass whatever the watch did.
#[test]
fn the_generator_reaches_the_shapes() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let (mut two, mut set_back, mut bare, mut retired_first) = (0, 0, 0, 0);
    let mut top_scrolled = 0;
    let (mut yes, mut no) = (0, 0);
    for _ in 0..400 {
        let (prog, sel, _) = case().new_tree(&mut runner).expect("tree").current();
        match reached(&bytes_of(&prog), sel, &[]) {
            true => yes += 1,
            false => no += 1,
        }
        with_term(|t| {
            let start = t.scroll_region();
            let (mut bands, mut retired) = (Vec::new(), false);
            let regions = t.region_scrolls();
            for &k in &prog {
                let (count, rot) = (t.row_moves(), t.rotation());
                feed(t, ATOMS[k]);
                if t.row_moves() != count {
                    // The region the move was made in: no atom that moves
                    // rows changes the region after the move.
                    let (top, bot) = t.scroll_region();
                    if top.index() == 0
                        && bot.index() == 9
                        && matches!(ATOMS[k], b"\x1b[L" | b"\x1b[2M")
                    {
                        bare += 1;
                    }
                    if retired {
                        retired_first += 1;
                    }
                    bands.push((top, bot));
                }
                if t.moved_since(rot) != Moved::Retired(Rows::ZERO) {
                    retired = true;
                }
            }
            if t.region_scrolls() != regions {
                top_scrolled += 1;
            }
            if t.scroll_region() == start && bands.iter().any(|&b| b != start) {
                set_back += 1;
            }
            if bands.iter().any(|a| {
                bands
                    .iter()
                    .any(|b| a.1.index() < b.0.index() || b.1.index() < a.0.index())
            }) {
                two += 1;
            }
        });
    }
    eprintln!(
        "two bands {two}, set back {set_back}, bare IL/DL {bare}, retired first {retired_first}, \
         top region scrolled {top_scrolled}, reached {yes}, not {no}"
    );
    for (n, what) in [
        (two, "two bands apart"),
        (set_back, "a region set back after a move"),
        (bare, "IL or DL with no region"),
        (retired_first, "lines retired before a move"),
        (top_scrolled, "a scroll of a region anchored at the top"),
        (yes, "a selection reached"),
        (no, "a selection not reached"),
    ] {
        assert!(n > 0, "the generator never built {what}");
    }
}
