//! **The watch on a selection's rows: each move of rows judged by the rows
//! it moved, and the selection carried with its text.**
//!
//! An embedder holding a selection on the live screen arms
//! [`Term::watch_rows`] on the rows it names before a read, and asks
//! [`Term::watched_lines`] after it where they stand -- carried by `IL`,
//! `DL` and a region scroll that copies, kept under a top region's scroll,
//! trimmed where a scroll takes them out of its region, pushes them under
//! the screen or off the alternate screen's top, as xterm does with
//! `keepSelection` at its default (since 2026-10-20) -- and
//! [`Term::watched_rows_changed`] whether a move left none of them, a move
//! under side margins, `IL` or `DL` reached one, a write or an erase
//! landed on one, or a line taken back erased one. It used to ask whether
//! the count of moves had moved and judge by the region the read ended in,
//! which a region set and set back inside one read hid; then whether any
//! move reached the rows at all, which dropped a selection whose text had
//! only moved.
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
//!   `IL` and `DL` with no region and in one, `RI` and `SD` in a partial
//!   region, scroll a region anchored at the top or under side margins, and
//!   retire lines before and after a move, with the selection moved between
//!   reads as an embedder moves it, on the primary and on the alternate
//!   screen; and a selection kept holds a run of the labels it was made on.

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

/// Where the watched lines stand after a read: the first and last as lines
/// of the live surface now, and how far their text moved against the count.
type Stand = (SurfaceLine, SurfaceLine, TextMoved);

/// Arms the watch on `sel`, feeds `bytes` as one read, and says where the
/// watched lines stand after it: `None` when the read reached them.
fn carried(t: &mut Term<'_>, sel: Option<(Row, Row)>, bytes: &[u8]) -> Option<Stand> {
    t.watch_rows(sel);
    feed(t, bytes);
    t.watched_lines().map(|w| (w.first, w.last, w.moved))
}

/// Screen rows `first` to `last`, their text moved `moved` against the
/// count.
fn on(first: usize, last: usize, moved: TextMoved) -> Option<Stand> {
    Some((
        SurfaceLine::At(Row::from_index(first)),
        SurfaceLine::At(Row::from_index(last)),
        moved,
    ))
}

/// Text that moved with the count alone.
const STILL: TextMoved = TextMoved::Down(Rows::ZERO);

fn up(k: usize) -> TextMoved {
    TextMoved::Up(Rows::of_count(k))
}

fn down(k: usize) -> TextMoved {
    TextMoved::Down(Rows::of_count(k))
}

/// **A region scroll that copies carries the region's rows and no others**
/// (xterm's `adjustHiliteOnFwdScroll`, since 2026-10-20). A line feed at
/// the bottom of a middle region, 4;6 (rows 3 to 5): a selection on its
/// bottom row goes up a row with its text, one on its top row leaves the
/// region and is reached, one across the region keeps the part inside it,
/// carried and trimmed at the region's top, and one above it or under it
/// stays.
#[test]
fn a_copying_scroll_up_carries_the_regions_rows_and_only_those() {
    let prog = b"\x1b[4;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (rows(3, 3), None, "the region's top row, scrolled out"),
        (rows(5, 5), on(4, 4, up(1)), "its bottom row"),
        (rows(4, 5), on(3, 4, up(1)), "its two lower rows"),
        (rows(2, 2), on(2, 2, STILL), "the row above it"),
        (rows(6, 6), on(6, 6, STILL), "the row under it"),
        (rows(0, 9), on(3, 4, up(1)), "a selection across it"),
    ] {
        with_term(|t| {
            let count = t.row_moves();
            assert_eq!(carried(t, sel, prog), want, "{what}");
            assert_ne!(t.row_moves(), count, "the control: a row moved");
        });
    }
}

/// **`RI` at the top of a middle region carries the region's rows down**,
/// and trims what it pushes out of the region's bottom (xterm's
/// `adjustHiliteOnBakScroll`).
#[test]
fn a_copying_scroll_down_carries_the_regions_rows_and_only_those() {
    let prog = b"\x1b[4;6r\x1b[4;1H\x1bM";
    for (sel, want, what) in [
        (rows(3, 3), on(4, 4, down(1)), "the region's top row"),
        (rows(5, 5), None, "its bottom row, pushed out"),
        (
            rows(3, 5),
            on(4, 5, down(1)),
            "the region, its last row trimmed",
        ),
        (rows(2, 2), on(2, 2, STILL), "the row above it"),
        (rows(6, 6), on(6, 6, STILL), "the row under it"),
    ] {
        with_term(|t| assert_eq!(carried(t, sel, prog), want, "{what}"));
    }
}

/// **`IL` carries the rows from the cursor to the region's bottom down**:
/// with no region, from the cursor to the screen's. xterm's `InsertLine`
/// disowns a selection holding the cursor's row, where blank rows open
/// inside it, so that one is reached, and so is one on the last row, pushed
/// out; the rows above the cursor keep their text.
#[test]
fn il_carries_the_rows_from_the_cursor_down() {
    let prog = b"\x1b[6;1H\x1b[L";
    for (sel, want, what) in [
        (rows(5, 5), None, "the cursor's row"),
        (rows(4, 6), None, "a selection holding it"),
        (rows(9, 9), None, "the screen's last row, pushed out"),
        (rows(6, 7), on(7, 8, down(1)), "two rows under the cursor"),
        (
            rows(7, 9),
            on(8, 9, down(1)),
            "the bottom, its last row trimmed",
        ),
        (rows(4, 4), on(4, 4, STILL), "the row above the cursor"),
    ] {
        with_term(|t| assert_eq!(carried(t, sel, prog), want, "{what}"));
    }
}

/// **`DL` the same**, inside a region: the rows under the deleted one go up
/// to the region's bottom, a selection holding the deleted row is reached
/// as xterm's `DeleteLine` disowns it, and the row under the region stays.
#[test]
fn dl_carries_the_rows_from_the_cursor_to_the_regions_bottom() {
    let prog = b"\x1b[3;8r\x1b[5;1H\x1b[M";
    for (sel, want, what) in [
        (rows(4, 4), None, "the cursor's row, deleted"),
        (rows(3, 5), None, "a selection holding it"),
        (rows(7, 7), on(6, 6, up(1)), "the region's last row"),
        (
            rows(5, 9),
            on(4, 6, up(1)),
            "across its bottom, trimmed there",
        ),
        (rows(3, 3), on(3, 3, STILL), "the row above the cursor"),
        (rows(8, 8), on(8, 8, STILL), "the row under the region"),
    ] {
        with_term(|t| assert_eq!(carried(t, sel, prog), want, "{what}"));
    }
}

/// **A scroll of a region anchored at the top leaves the rows under it
/// where they are.** The scroll advances the ring and the rows under the
/// region stay, so a selection there is carried down against the count and
/// stays on its own text; one across the region's bottom keeps the
/// region's part, which the ring moves; one on the region's rows moves
/// with the ring. A line feed at the bottom of 1;5 (rows 0 to 4) on the
/// primary. It is not a move in place: the count of those holds, and the
/// region-scroll count moves.
#[test]
fn a_top_regions_scroll_keeps_the_rows_under_it() {
    let prog = b"\x1b[1;5r\x1b[5;1H\n";
    for (sel, want, what) in [
        (
            rows(5, 5),
            on(5, 5, down(1)),
            "the first row under the region",
        ),
        (rows(9, 9), on(9, 9, down(1)), "the screen's last row"),
        (
            rows(3, 6),
            on(2, 3, STILL),
            "a selection across the region's bottom",
        ),
        (rows(4, 4), on(3, 3, STILL), "the region's bottom row"),
        (rows(1, 4), on(0, 3, STILL), "the region's rows"),
    ] {
        with_term(|t| {
            let (moves, regions, rot) = (t.row_moves(), t.region_scrolls(), t.rotation());
            assert_eq!(carried(t, sel, prog), want, "{what}");
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
/// is taller than the rows under it: 1;6 (rows 0 to 5) over four rows. The
/// region's top row leaves the screen there and is gone.
#[test]
fn a_top_regions_scroll_keeps_the_rows_under_it_on_the_alternate_screen() {
    let prog = b"\x1b[1;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (
            rows(6, 6),
            on(6, 6, down(1)),
            "the first row under the region",
        ),
        (rows(9, 9), on(9, 9, down(1)), "the screen's last row"),
        (rows(5, 5), on(4, 4, STILL), "the region's bottom row"),
        (rows(0, 2), on(0, 1, STILL), "its top, trimmed as it leaves"),
        (rows(0, 0), None, "its top row alone, gone"),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[?1049h");
            let (regions, rot) = (t.region_scrolls(), t.rotation());
            assert_eq!(carried(t, sel, prog), want, "{what}");
            assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(1)));
            assert_ne!(t.region_scrolls(), regions, "the control: it rotated");
        });
    }
}

/// **Whatever region the read ends in.** Begun in 1;9 and scrolled in 1;5,
/// then a copy in 8;9 and back to 1;9, or straight back to 1;9: the rows
/// that were under 1;5 stay, carried against the count, and the copy then
/// takes the arm's row 7 out of 8;9; the region's rows move with the ring.
#[test]
fn a_top_regions_scroll_is_judged_by_its_own_rows_whatever_the_read_ends_in() {
    let copied = &b"\x1b[1;5r\x1b[5;1H\n\x1b[8;9r\x1b[9;1H\n\x1b[1;9r"[..];
    let back = &b"\x1b[1;5r\x1b[5;1H\n\x1b[1;9r"[..];
    for (prog, sel, want, what) in [
        (
            copied,
            rows(5, 5),
            on(5, 5, down(1)),
            "the arm's row 5, under 1;5",
        ),
        (copied, rows(7, 7), None, "its row 7, then 8;9's top row"),
        (
            copied,
            rows(8, 8),
            on(7, 7, STILL),
            "its row 8, up a row in 8;9",
        ),
        (copied, rows(2, 2), on(1, 1, STILL), "its row 2, in 1;5"),
        (back, rows(5, 5), on(5, 5, down(1)), "the arm's row 5"),
        (back, rows(7, 7), on(7, 7, down(1)), "its row 7"),
        (back, rows(2, 2), on(1, 1, STILL), "its row 2"),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[1;9r");
            assert_eq!(carried(t, sel, prog), want, "{what}, {prog:?}");
            assert_eq!(t.scroll_region(), (Row::ZERO, Row::from_index(8)));
        });
    }
}

/// **A region set and set back within one read**: begun in 8;9, copied in
/// 4;6, ended in 8;9. The copy's rows are carried and the rows of the
/// region the read began and ended in stay -- as when the three sequences
/// arrive in three reads.
#[test]
fn a_region_set_and_set_back_within_one_read_is_judged_by_the_copy() {
    let prog = b"\x1b[4;6r\x1b[6;1H\n\x1b[8;9r";
    for (sel, want, what) in [
        (rows(4, 4), on(3, 3, up(1)), "a row the copy moved"),
        (
            rows(7, 7),
            on(7, 7, STILL),
            "the end region's row, which nothing moved",
        ),
        (rows(2, 2), on(2, 2, STILL), "a row above both"),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[8;9r");
            assert_eq!(carried(t, sel, prog), want, "{what}");
            assert_eq!(t.scroll_region(), (Row::from_index(7), Row::from_index(8)));
        });
    }
}

/// **Lines retired before the move, in the same read, number its rows as
/// the selection is numbered.** Two line feeds at the bottom of the whole
/// screen retire two lines, so the arm's row 7 is row 5 when 4;6 copies,
/// and goes up a row with its text. The arm's row 3 is row 1 then, and
/// stays, though row 3 is the region's top.
#[test]
fn lines_retired_before_the_move_number_its_rows() {
    let prog = b"\x1b[10;1H\n\n\x1b[4;6r\x1b[6;1H\n";
    for (sel, want, what) in [
        (
            rows(7, 7),
            on(4, 4, up(1)),
            "the arm's row 7, carried into the region",
        ),
        (
            rows(3, 3),
            on(1, 1, STILL),
            "the arm's row 3, carried above it",
        ),
        (
            rows(2, 2),
            on(0, 0, STILL),
            "the arm's row 2, carried above it too",
        ),
    ] {
        with_term(|t| {
            let rot = t.rotation();
            assert_eq!(carried(t, sel, prog), want, "{what}");
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

/// **A scroll down trims what it pushes under the screen**, on either
/// screen, as xterm's `adjustHiliteOnBakScroll` truncates a selection at
/// the bottom margin: `SD 2` carries rows 5 to 9 down to 7 to 11 and the
/// two pushed under go; rows 8 and 9 go whole and the selection with them;
/// `RI` at the top keeps row 8 on row 9.
#[test]
fn a_scroll_down_trims_what_it_pushes_under_the_screen() {
    for alt in [false, true] {
        for (sel, prog, want, what) in [
            (
                rows(5, 9),
                &b"\x1b[2T"[..],
                on(7, 9, STILL),
                "SD 2 over rows 5 to 9",
            ),
            (rows(8, 9), b"\x1b[2T", None, "SD 2 over rows 8 and 9"),
            (
                rows(8, 9),
                b"\x1b[1;1H\x1bM",
                on(9, 9, STILL),
                "RI at the top",
            ),
            (
                rows(2, 4),
                b"\x1b[2T",
                on(4, 6, STILL),
                "the control: none pushed",
            ),
        ] {
            with_term(|t| {
                feed(t, &history());
                if alt {
                    feed(t, b"\x1b[?1049h");
                }
                assert_eq!(carried(t, sel, prog), want, "{what}, alternate {alt}");
            });
        }
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
        // The prompt's mark first: every row from it on is the shell's
        // block, which the narrowing keeps whole, the tail past the edge.
        feed(&mut t, b"\x1b]133;A\x07");
        feed(&mut t, &history());
        feed(&mut t, format!("\x1b[5;1H{}", "S".repeat(30)).as_bytes());
        feed(&mut t, b"\x1b[10;1H");
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

/// **A redraw mark erases a block wholly on the screen, so it reaches the
/// rows it erases** as any erase does. The block opens on row 5 with `$ `
/// and the shell redraws it from row 6, so the mark erases rows 5 and 6:
/// a selection on row 6, which the `$ ` did not write, is reached, and one
/// on row 7, under the block, is not. Until 2026-10-17 only the rows a mark
/// took back were tested, and row 6 was kept on the blank the erase left.
#[test]
fn a_redraw_mark_reaches_the_rows_of_a_block_on_the_screen() {
    let open = b"\x1b[6;1H\x1b]133;A\x07$ ";
    let redraw = b"\x1b[7;1H\x1b]133;A\x07";
    for (sel, want, what) in [
        (rows(6, 6), true, "the block's second row"),
        (rows(7, 7), false, "the row under the block"),
    ] {
        with_term(|t| {
            feed(t, &history());
            feed(t, open);
            assert_eq!(text(t, 6), "line 11", "the control: row 6 holds its line");
            assert_eq!(moved(t, sel, redraw), want, "{what}");
            assert_eq!(text(t, 6), "", "the mark erased it");
        });
    }
}

/// **A watch on lines of the history**: a line the whole screen's scroll
/// down takes back shows on the top row, blanked, and a line feed at the
/// bottom sends it back, so the read nets no movement and the saved line
/// that held "line 4" holds nothing. A selection on it is reached; one on
/// the line above it, which no take-back reached, is not, and neither is
/// one on the screen's rows 1 to 8. And a selective erase of the saved
/// lines, `CSI ? 3 J`, reaches a selection on them.
#[test]
fn a_watch_on_saved_lines_is_reached_where_their_text_goes() {
    let above = |n: usize| SurfaceLine::Above(Rows::of_count(n));
    let on = |y: usize| SurfaceLine::At(Row::from_index(y));
    for (prog, sel, want, what) in [
        (
            &b"\x1b[T\x1b[10;1H\n"[..],
            (above(1), above(1)),
            true,
            "the newest saved line",
        ),
        (
            &b"\x1b[T\x1b[10;1H\n"[..],
            (above(2), above(2)),
            false,
            "the one above it",
        ),
        (
            &b"\x1b[T\x1b[10;1H\n"[..],
            (above(1), on(1)),
            true,
            "from it onto the screen",
        ),
        (
            &b"\x1b[T\x1b[10;1H\n"[..],
            (on(1), on(8)),
            false,
            "the screen's rows",
        ),
        (
            &b"\x1b[?3J"[..],
            (above(3), above(2)),
            true,
            "two saved lines, erased",
        ),
        (
            &b"\x1b[?3J"[..],
            (on(3), on(4)),
            false,
            "rows a saved-line erase leaves",
        ),
    ] {
        with_term(|t| {
            feed(t, &history());
            let rot = t.rotation();
            t.watch_lines(Some(sel));
            feed(t, prog);
            assert_eq!(t.watched_rows_changed(), want, "{what}");
            if prog.len() > 5 {
                assert_eq!(t.moved_since(rot), Moved::Retired(Rows::ZERO));
                let newest: String = t
                    .view(Rows::of_count(1))
                    .row(Row::ZERO)
                    .iter()
                    .filter_map(|c| c.content.as_codepoint())
                    .filter_map(|c| char::from_u32(c.value()))
                    .collect();
                assert_eq!(newest.trim_end(), "", "the saved line is blank");
            }
        });
    }
}

/// `n` line feeds at the bottom of the screen.
fn feeds(n: usize) -> Vec<u8> {
    let mut v = b"\x1b[10;1H".to_vec();
    v.resize(v.len() + n, b'\n');
    v
}

/// **The watch counts how far the live surface moved, however far.** The
/// rotation is read round the ring: up to half of it retired, beyond that
/// taken back, so more lines than that in one arm are misread by
/// [`Term::moved_since`] -- one ring and a line reads as one line. The watch
/// counts each line instead, and [`Term::watched_moved`] says how far the
/// surface went, so the selection is numbered by that and not reached by the
/// distance alone: half the ring retired, a line more, and the ring and a
/// line, each on the alternate screen, whose lines `top` does not count, and
/// taken back by `SD` a line under half the ring and half of it. The
/// selection is on rows no line taken back blanks or pushes under the
/// window. Until 2026-10-17 a read past half the ring reached the watch
/// whatever it moved, which dropped a selection on text that was still its
/// own: `ESC [ 6 S ESC [ 5 T` kept row 8's text on row 7. The ring and a
/// line take the selection's row off the alternate screen's top, which trims
/// it at that scroll and leaves nothing: reached, and the count stops, as it
/// does once a watch is reached.
#[test]
fn a_surface_moved_past_half_its_ring_is_counted_on_the_alternate_screen() {
    let n = with_term(|t| {
        feed(t, b"\x1b[?1049h");
        t.ring().index()
    });
    let half = n / 2;
    let back_half = n - half;
    let up = |k: usize| feeds(k);
    let down = |k: usize| format!("\x1b[{k}T").into_bytes();
    for (prog, sel, want, what) in [
        (
            up(half),
            rows(9, 9),
            Moved::Retired(Rows::of_count(half)),
            "half the ring retired",
        ),
        (
            up(half + 1),
            rows(9, 9),
            Moved::Retired(Rows::of_count(half + 1)),
            "a line more",
        ),
        (
            up(n + 1),
            rows(9, 9),
            Moved::Retired(Rows::of_count(n + 1)),
            "the ring and a line, off the top",
        ),
        (
            down(back_half - 1),
            rows(0, 0),
            Moved::Restored(Rows::of_count(back_half - 1)),
            "a line under half taken back",
        ),
        (
            down(back_half),
            rows(0, 0),
            Moved::Restored(Rows::of_count(back_half)),
            "half the ring taken back",
        ),
    ] {
        with_term(|t| {
            feed(t, b"\x1b[?1049h\x1b[10;1H");
            let rot = t.rotation();
            let off = what.ends_with("off the top");
            assert_eq!(moved(t, sel, &prog), off, "{what}, a ring of {n}");
            let count = (!off).then_some(want);
            assert_eq!(t.watched_moved(), count, "{what}, a ring of {n}");
            if off {
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

/// **A read past half the ring and back is numbered by where it went.**
/// On the alternate screen's ten-row ring, `ESC [ 6 S ESC [ 5 T` nets one
/// line retired: row 8's text is on row 7, and no line the read took back
/// blanked it or pushed it under. Six line feeds at the bottom and six `RI`
/// at the top net none, the arm's rows 8 and 9 back where they were, a row
/// of the ring that held them never blanked. And `ESC [ 6 T ESC [ 6 S`
/// nets none, the arm's row 1 back where it was. Each read whole and at
/// every read size and split. The control: row 9 of the last, which the
/// scroll down pushed under the window, is reached.
#[test]
fn a_read_past_half_the_ring_and_back_reaches_no_row_it_left_alone() {
    let ri: Vec<u8> = [&b"\x1b[1;1H"[..], &b"\x1bM".repeat(6)].concat();
    for (prog, sel, what) in [
        (b"\x1b[6S\x1b[5T".to_vec(), (8, 8), "six up, five down"),
        ([feeds(6), ri].concat(), (8, 9), "six line feeds, six RI"),
        (b"\x1b[6T\x1b[6S".to_vec(), (1, 1), "six down, six up"),
    ] {
        assert!(
            !changes(b"\x1b[?1049h", &prog, sel),
            "{what}: {:?}",
            String::from_utf8_lossy(&prog)
        );
    }
    assert!(changes(b"\x1b[?1049h", b"\x1b[6T\x1b[6S", (9, 9)));
}

/// **On the primary the top line counts them**: half its ring of lines
/// retired, a line more, and the ring and a line, each counted, and none
/// reaching a selection on the last row.
#[test]
fn a_surface_moved_past_half_its_ring_is_counted_on_the_primary() {
    let n = with_term(|t| t.ring().index());
    for k in [n / 2, n / 2 + 1, n + 1] {
        with_term(|t| {
            let rot = t.rotation();
            assert!(!moved(t, rows(9, 9), &feeds(k)), "{k}, a ring of {n}");
            assert_eq!(t.watched_moved(), Some(Moved::Retired(Rows::of_count(k))));
            if k == n + 1 {
                assert_eq!(t.moved_since(rot), Moved::Retired(Rows::of_count(1)));
            }
        });
    }
}

/// **And at line zero, where a line taken back moves no top line**, the
/// watch counts it. A history of two lines makes a ring of twelve: five
/// taken back on a fresh terminal are counted as five, and six as six,
/// which [`Term::moved_since`] reads as six retired. Neither reaches row 1,
/// whose text went down with the rest.
#[test]
fn lines_taken_back_at_line_zero_are_counted() {
    for k in [5usize, 6] {
        let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(2));
        let mut t = Term::new(owned.storage(), &WS);
        assert_eq!(t.ring().index(), 12, "the control: a ring of twelve");
        let rot = t.rotation();
        let prog = format!("\x1b[{k}T").into_bytes();
        assert!(!moved(&mut t, rows(1, 1), &prog), "{k} taken back");
        assert_eq!(t.watched_moved(), Some(Moved::Restored(Rows::of_count(k))));
        if k == 6 {
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
        assert!(moved(t, rows(3, 3), b"\x1b[4;6r\x1b[6;1H\n"), "the control");
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
    let whole = reached_rows(setup, read, sel, &[]);
    for size in 1..read.len() {
        let cuts: Vec<usize> = (size..read.len()).step_by(size).collect();
        assert_eq!(
            reached_rows(setup, read, sel, &cuts),
            whole,
            "{size} bytes to a read: {read:?}"
        );
    }
    for k in 1..read.len() {
        assert_eq!(
            reached_rows(setup, read, sel, &[k]),
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

/// **A mark past a cluster's capacity reaches the row, as a mark that
/// joins does.** Eight `U+0301` fill the cluster on `a`, and a ninth is
/// dropped and counted with the cell unchanged; xterm disowns there too,
/// for the row is written whether or not `addXtermCombining` found a slot
/// (`[[source: xterm-411-util-c]]`; `[[source: xterm-reset]]`, charproc.c's
/// combining path). The control: seven marks and an eighth, which joins.
#[test]
fn a_mark_past_a_clusters_capacity_reaches_the_row() {
    let mark = "\u{301}";
    let full = format!("\x1b[1;1Ha{}", mark.repeat(8));
    with_term(|t| {
        feed(t, full.as_bytes());
        let (before, dropped) = (t.row(Row::ZERO).map(<[_]>::to_vec), t.marks_dropped);
        feed(t, mark.as_bytes());
        assert_eq!(t.row(Row::ZERO).map(<[_]>::to_vec), before, "unchanged");
        assert_eq!(t.marks_dropped, dropped + 1, "the ninth is dropped");
    });
    assert!(changes(full.as_bytes(), mark.as_bytes(), (0, 0)), "a ninth");
    assert!(!changes(full.as_bytes(), mark.as_bytes(), (1, 1)), "row 1");
    let seven = format!("\x1b[1;1Ha{}", mark.repeat(7));
    assert!(
        changes(seven.as_bytes(), mark.as_bytes(), (0, 0)),
        "an eighth"
    );
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

/// **`?1040`, xterm's keepSelection, is set at power-on** -- "The default
/// is \"true\"" (`[[source: xterm-man]]`) -- and `DECRQM` answers it as
/// it stands: `1` set, `2` reset. `DECSET` and `DECRST` of it are not
/// counted as unsupported. A write on the selection's rows reaches the
/// watch either way: what the mode keeps is the embedder's saved text,
/// not the selection's place.
#[test]
fn keep_selection_is_set_at_power_on_and_answers_decrqm() {
    with_term(|t| {
        t.clear_reply();
        feed(t, b"\x1b[?1040$p");
        assert_eq!(t.reply(), b"\x1b[?1040;1$y", "set at power-on");
        let before = t.unsupported_param;
        feed(t, b"\x1b[?1040l");
        t.clear_reply();
        feed(t, b"\x1b[?1040$p");
        assert_eq!(t.reply(), b"\x1b[?1040;2$y", "reset");
        feed(t, b"\x1b[?1040h");
        assert_eq!(t.unsupported_param, before, "recognised");
        assert!(t.resource(Resource::KeepSelection));
    });
    for mode in [b"\x1b[?1040h".as_slice(), b"\x1b[?1040l"] {
        let prog = [mode, DRAWN].concat();
        assert!(changes(&prog, b"\x1b[1;1Hhello", (0, 0)));
    }
}

/// What a case's move leaves of the selection while `?1040` is reset.
#[derive(Clone, Copy)]
enum Kept {
    Whole,
    Dropped,
}

/// **While `?1040` is reset, a scroll that would trim the selection drops
/// it**, as xterm's reset disowns it where its set truncates it
/// (`[[source: xterm-scroll]]`, `adjustHiliteOnFwdScroll` and
/// `adjustHiliteOnBakScroll`, 451-456 and 553-559): across a region's
/// edge, pushed out of a region or under the screen, across a top region's
/// bottom, and off the alternate screen's top. A selection the move carries
/// whole is kept either way, and so is one it does not reach. `IL` and `DL`
/// trim at the region's bottom in either mode: xterm's `InsertLine` and
/// `DeleteLine` read no keepSelection and disown only where the cursor's
/// row or a deleted row lies in the selection. Set, each is carried as the
/// tests above pin.
#[test]
fn while_keep_selection_is_reset_a_trimming_scroll_drops_the_selection() {
    type Case = (
        &'static [u8],
        Option<(Row, Row)>,
        &'static [u8],
        Option<Stand>,
        Kept,
    );
    let cases: [Case; 10] = [
        (
            b"",
            rows(0, 9),
            b"\x1b[4;6r\x1b[6;1H\n",
            on(3, 4, up(1)),
            Kept::Dropped,
        ),
        (
            b"",
            rows(4, 5),
            b"\x1b[4;6r\x1b[6;1H\n",
            on(3, 4, up(1)),
            Kept::Whole,
        ),
        (
            b"",
            rows(6, 6),
            b"\x1b[4;6r\x1b[6;1H\n",
            on(6, 6, STILL),
            Kept::Whole,
        ),
        (
            b"",
            rows(3, 5),
            b"\x1b[4;6r\x1b[4;1H\x1bM",
            on(4, 5, down(1)),
            Kept::Dropped,
        ),
        (
            b"",
            rows(5, 9),
            b"\x1b[3;8r\x1b[5;1H\x1b[M",
            on(4, 6, up(1)),
            Kept::Whole,
        ),
        (
            b"",
            rows(7, 9),
            b"\x1b[6;1H\x1b[L",
            on(8, 9, down(1)),
            Kept::Whole,
        ),
        (
            b"",
            rows(6, 7),
            b"\x1b[6;1H\x1b[L",
            on(7, 8, down(1)),
            Kept::Whole,
        ),
        (
            b"",
            rows(3, 6),
            b"\x1b[1;5r\x1b[5;1H\n",
            on(2, 3, STILL),
            Kept::Dropped,
        ),
        (
            b"\x1b[?1049h",
            rows(0, 2),
            b"\x1b[1;6r\x1b[6;1H\n",
            on(0, 1, STILL),
            Kept::Dropped,
        ),
        (
            b"history",
            rows(5, 9),
            b"\x1b[2T",
            on(7, 9, STILL),
            Kept::Dropped,
        ),
    ];
    for (setup, sel, prog, set, reset) in cases {
        for keep in [true, false] {
            with_term(|t| {
                if setup == b"history" {
                    feed(t, &history());
                } else {
                    feed(t, setup);
                }
                if !keep {
                    feed(t, b"\x1b[?1040l");
                }
                let want = match (keep, reset) {
                    (false, Kept::Dropped) => None,
                    _ => set,
                };
                let what = format!("{sel:?} under {prog:?}, keep {keep}");
                assert_eq!(carried(t, sel, prog), want, "{what}");
            });
        }
    }
}

// ---- the same answer whatever the reads ----------------------------------

/// What a program is made of: regions set and set back, moves in place,
/// the whole screen's scroll down -- `SD`, `RI` at the top, a redraw mark
/// over a block on the screen -- reads that move the surface past half its
/// ring and back, and text, `EL`, `ED`, `ICH`, `DCH`, `ECH` and the
/// selective erases, the saved lines' too. Until 2026-10-17 no program
/// scrolled the whole screen down: the app dropped a selection on a read
/// whose net movement took lines back, judged at the read's two ends, so
/// `ESC [ 2 S ESC [ 2 T` left a selection on row 5 kept read whole and
/// dropped split after `ESC [ 2 S`. It renumbers one now, by the watch's
/// count, and the watch tests every line a take-back erases.
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
    b"\x1b[r\x1b[T",
    b"\x1b[r\x1b[2T",
    b"\x1b[r\x1b[1;1H\x1bM",
    b"\x1b[r\x1b[6T",
    b"\x1b[r\x1b[11S",
    b"\x1b[r\x1b[12T",
    b"\x1b[?2J",
    b"\x1b[?3J",
    b"\x1b[6;1H\x1b]133;A\x07$ \x1b[7;1H\x1b]133;A\x07",
    b"\x1b[?69h\x1b[2;8s\x1b[S\x1b[?69l",
    b"\x1b[3;8r\x1b[5;1H\x1b[2L",
    b"\x1b[3;8r\x1b[4;1H\x1b[M",
    b"\x1b[2;7r\x1b[2;1H\x1b[2T",
];

/// The history the property starts on: forty-five numbered lines, so
/// thirty are saved and a selection may lie in them.
fn saved() -> Vec<u8> {
    (0..45)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect()
}

/// A program, the selection's lines when the first read is armed --
/// negative is a line of the history, `-1` the newest -- and where to cut
/// the bytes into reads.
fn case() -> impl Strategy<Value = (Vec<usize>, Option<(i32, i32)>, Vec<usize>)> {
    (
        prop::collection::vec(0..ATOMS.len(), 1..24),
        prop_oneof![
            1 => Just(None),
            6 => (-8i32..10, -8i32..10).prop_map(|(a, b)| Some((a.min(b), a.max(b)))),
        ],
        prop::collection::vec(0usize..400, 0..6),
    )
}

/// `y` as a line of the live surface: a screen row, or a line of the
/// history `-y` lines above it.
fn line(y: i32) -> SurfaceLine {
    match usize::try_from(y) {
        Ok(y) => SurfaceLine::At(Row::from_index(y)),
        Err(_) => SurfaceLine::Above(Rows::of_count(
            usize::try_from(y.unsigned_abs()).expect("a line the history holds"),
        )),
    }
}

/// Feeds `bytes` in the reads `cuts` makes of it, as an embedder does:
/// armed on its selection's lines before each read -- those the history
/// still holds -- and the selection moved after it to where the watch says
/// its lines stand (`Term::watched_lines`). Where it ends -- `Some(None)`
/// for no selection -- or `None` when a read reached it.
fn reached(bytes: &[u8], sel: Option<(i32, i32)>, cuts: &[usize]) -> Option<Option<(i32, i32)>> {
    reached_after(&saved(), bytes, sel, cuts)
}

/// The text of line `y` of the live surface: a screen row, or a line of the
/// history `-y` lines above it.
fn label(t: &Term<'_>, y: i32) -> String {
    let sb = t.scrollback();
    let cells = match usize::try_from(y) {
        Ok(y) => t.row(Row::from_index(y)).expect("a row on the grid"),
        Err(_) => {
            let back = usize::try_from(y.unsigned_abs()).expect("a line") - 1;
            sb.line(Back::of(back)).expect("a line the history holds").0
        }
    };
    let s: String = cells
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

/// [`reached`], after `setup` fed before the first arm. **A kept selection
/// holds its own text**: the lines it ends on show a run of the labels it
/// was made on, in order -- carried with their text and trimmed, never
/// moved onto another line's.
fn reached_after(
    setup: &[u8],
    bytes: &[u8],
    sel: Option<(i32, i32)>,
    cuts: &[usize],
) -> Option<Option<(i32, i32)>> {
    with_term(|t| {
        feed(t, setup);
        let mut sel = sel;
        let labels: Vec<String> = sel
            .map(|(a, b)| (a..=b).map(|y| label(t, y)).collect())
            .unwrap_or_default();
        let mut at = 0;
        let mut ends: Vec<usize> = cuts.iter().map(|c| c % (bytes.len() + 1)).collect();
        ends.push(bytes.len());
        ends.sort_unstable();
        for end in ends {
            let saved = i32::try_from(t.scrollback().len()).expect("a short history");
            sel = sel.and_then(|(a, b)| (b >= -saved).then(|| (a.max(-saved), b)));
            t.watch_lines(sel.map(|(a, b)| (line(a), line(b))));
            feed(t, &bytes[at..end]);
            at = end;
            if t.watched_rows_changed() {
                return None;
            }
            let y = |l: SurfaceLine| match l {
                SurfaceLine::At(r) => i32::try_from(r.index()).expect("a screen row"),
                SurfaceLine::Above(n) => -i32::try_from(n.index()).expect("a short history"),
            };
            sel = t.watched_lines().map(|w| (y(w.first), y(w.last)));
        }
        let saved = i32::try_from(t.scrollback().len()).expect("a short history");
        sel = sel.and_then(|(a, b)| (b >= -saved).then(|| (a.max(-saved), b)));
        if let Some((a, b)) = sel {
            let now: Vec<String> = (a..=b).map(|y| label(t, y)).collect();
            assert!(
                labels.windows(now.len()).any(|w| w == now.as_slice()),
                "kept on {now:?}, made on {labels:?}"
            );
        }
        Some(sel)
    })
}

/// [`reached_after`] on screen rows, whole: the tests of one mechanism.
fn reached_rows(setup: &[u8], bytes: &[u8], sel: (usize, usize), cuts: &[usize]) -> bool {
    let row = |y: usize| i32::try_from(y).expect("a screen row");
    reached_after(setup, bytes, Some((row(sel.0), row(sel.1))), cuts).is_none()
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

    /// **And on the alternate screen**, whose ring holds no history and
    /// whose top loses each line it scrolls over: ten labelled rows, a
    /// selection on any run of them.
    #[test]
    fn the_answer_is_the_same_however_the_reads_fall_on_the_alternate_screen(
        (prog, sel, cuts) in alt_case()
    ) {
        let bytes = bytes_of(&prog);
        let setup = alternate();
        let whole = reached_after(&setup, &bytes, sel, &[]);
        let ones: Vec<usize> = (1..bytes.len()).collect();
        prop_assert_eq!(whole, reached_after(&setup, &bytes, sel, &ones), "one byte to a read");
        prop_assert_eq!(whole, reached_after(&setup, &bytes, sel, &cuts), "cut at {:?}", cuts);
    }
}

/// The alternate screen over the saved lines, its rows labelled `A0` to
/// `A9`, so a row's text says which row it was.
fn alternate() -> Vec<u8> {
    let mut v = saved();
    v.extend_from_slice(b"\x1b[?1049h");
    v.extend((0..10).flat_map(|y| format!("\x1b[{};1HA{y}", y + 1).into_bytes()));
    v
}

/// [`case`] on the alternate screen: a selection on its rows alone.
fn alt_case() -> impl Strategy<Value = (Vec<usize>, Option<(i32, i32)>, Vec<usize>)> {
    (
        prop::collection::vec(0..ATOMS.len(), 1..24),
        (0i32..10, 0i32..10).prop_map(|(a, b)| Some((a.min(b), a.max(b)))),
        prop::collection::vec(0usize..400, 0..6),
    )
}

/// **The alternate screen's property case, pinned**: its rows labelled,
/// `A0` to `A2` selected, the two lines feed at the bottom takes off the
/// top, then scrolls down of five lines, `DL 2` at the top and a scroll down
/// of six in one read, which leave `A2` on row 9. Cut after the line
/// feeds, the second read took back eleven lines with the selection
/// armed on row 0, more than the alternate ring holds, and the watch put
/// `A2` on row 8, a blank row. Named break: number the watch's row 0 by
/// `now`'s saturating count alone, with no lift -- cut after the line
/// feeds, the selection ends on the blank row 8.
#[test]
fn a_read_taking_back_more_than_the_alternate_ring_keeps_its_selection() {
    let prog = bytes_of(&[17, 0, 14, 10, 4, 0, 27, 27, 26, 13, 0, 29]);
    let sel = Some((0, 2));
    let setup = alternate();
    let ones: Vec<usize> = (1..prog.len()).collect();
    for cuts in [vec![], vec![12], ones] {
        assert_eq!(
            reached_after(&setup, &prog, sel, &cuts),
            Some(Some((9, 9))),
            "cut at {cuts:?}"
        );
    }
}

/// **A second alternate-screen property case, pinned**: `A0` selected,
/// a reverse index at the top and a scroll down of six, margins set,
/// a scroll down of two that leaves `A0` on row 9, `DL 2` at the top
/// carrying it up to row 7, and a scroll down of two that puts it back on
/// row 9. Nine lines taken back with the selection armed on row 0, more
/// than the alternate ring holds, before the carry up. Whole, a byte a
/// read and cut after each step, the watch keeps `A0` on row 9.
#[test]
fn a_selection_carried_up_between_reads_taking_back_past_the_alternate_ring_keeps_its_line() {
    let atoms = [28, 29, 0, 27, 13, 27, 0];
    let prog = bytes_of(&atoms);
    let sel = Some((0, 0));
    let setup = alternate();
    let ones: Vec<usize> = (1..prog.len()).collect();
    let steps: Vec<usize> = atoms
        .iter()
        .scan(0, |end, &k| {
            *end += ATOMS[k].len();
            Some(*end)
        })
        .collect();
    for cuts in [vec![], ones, steps] {
        assert_eq!(
            reached_after(&setup, &prog, sel, &cuts),
            Some(Some((9, 9))),
            "cut at {cuts:?}"
        );
    }
}

/// **A line kept on the alternate screen through more lines taken back
/// than its ring holds**: armed on `A0`, a line scrolled down and deleted
/// back at the top, again and again in one read -- each round a line taken
/// back and `A0` back on row 0 -- two rounds more than the ring is long,
/// then three lines down. `A0` is on row 3, and the watch says so. Named
/// break: as above, no lift -- the watch puts `A0` on another row.
#[test]
fn a_line_kept_through_more_take_backs_than_the_ring_holds_is_where_its_text_is() {
    with_term(|t| {
        feed(t, &alternate());
        let n = t.ring().index();
        t.watch_rows(rows(0, 0));
        let round = b"\x1b[r\x1b[T\x1b[M";
        let prog: Vec<u8> = round
            .iter()
            .copied()
            .cycle()
            .take(round.len() * (n + 2))
            .chain(b"\x1b[3T".iter().copied())
            .collect();
        feed(t, &prog);
        assert_eq!(label(t, 3), "A0", "the control: A0's text is on row 3");
        assert!(!t.watched_rows_changed(), "no move left A0 nor wrote on it");
        let w = t.watched_lines().expect("the watch");
        assert_eq!(
            (w.first, w.last),
            (
                SurfaceLine::At(Row::from_index(3)),
                SurfaceLine::At(Row::from_index(3))
            ),
            "where A0's text is"
        );
    });
}

/// **And how far its text moved against the count is exact**: armed on
/// `A5`, five lines deleted at the top bring it to row 0, then a line
/// scrolled down and deleted back, round after round, two rounds more than
/// the ring is long, then three lines down. `A5` is on row 3, and its text
/// has moved up five lines and a line a round against the count, which
/// [`Term::watched_lines`]'s `moved` says. Named break: no lift before a
/// carry up -- the distance moved is cut short where it would go below
/// zero.
#[test]
fn a_line_moved_up_past_the_count_says_how_far_its_text_went() {
    with_term(|t| {
        feed(t, &alternate());
        let n = t.ring().index();
        t.watch_rows(rows(5, 5));
        let round = b"\x1b[r\x1b[T\x1b[M";
        let prog: Vec<u8> = b"\x1b[r\x1b[M\x1b[M\x1b[M\x1b[M\x1b[M"
            .iter()
            .copied()
            .chain(round.iter().copied().cycle().take(round.len() * (n + 2)))
            .chain(b"\x1b[3T".iter().copied())
            .collect();
        feed(t, &prog);
        assert_eq!(label(t, 3), "A5", "the control: A5's text is on row 3");
        let w = t.watched_lines().expect("the watch");
        assert_eq!(
            (w.first, w.moved),
            (
                SurfaceLine::At(Row::from_index(3)),
                TextMoved::Up(Rows::of_count(5 + n + 2))
            ),
            "where A5's text is, and how far it moved against the count"
        );
    });
}

/// **The generator reaches the shapes the property is for**, counted over a
/// fixed sample: two moves in one read in bands apart, a region set and set
/// back around a move, `IL` or `DL` with no region, lines retired before a
/// move in one read, a scroll of a region anchored at the top, a line taken
/// back by the whole screen's scroll down, a program that moves the
/// surface more than half its ring, and a selection in the history; and
/// both answers. A property whose programs never built them would pass
/// whatever the watch did.
#[test]
fn the_generator_reaches_the_shapes() {
    use proptest::strategy::ValueTree;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let (mut two, mut set_back, mut bare, mut retired_first) = (0, 0, 0, 0);
    let (mut top_scrolled, mut taken_back, mut past_half, mut in_history) = (0, 0, 0, 0);
    let (mut yes, mut no) = (0, 0);
    for _ in 0..400 {
        let (prog, sel, _) = case().new_tree(&mut runner).expect("tree").current();
        match reached(&bytes_of(&prog), sel, &[]) {
            None => yes += 1,
            Some(_) => no += 1,
        }
        in_history += usize::from(sel.is_some_and(|(a, _)| a < 0));
        with_term(|t| {
            feed(t, &saved());
            let (mark, ring) = (t.history_mark(), t.ring().index());
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
                match t.moved_since(rot) {
                    Moved::Retired(n) if n.index() > 0 => retired = true,
                    Moved::Restored(n) if n.index() > 0 => taken_back += 1,
                    _ => {}
                }
            }
            let (Moved::Retired(n) | Moved::Restored(n)) = t.history_moved_since(mark);
            past_half += usize::from(n.index() * 2 > ring);
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
         top region scrolled {top_scrolled}, taken back {taken_back}, past half {past_half}, \
         in the history {in_history}, reached {yes}, not {no}"
    );
    for (n, what) in [
        (two, "two bands apart"),
        (set_back, "a region set back after a move"),
        (bare, "IL or DL with no region"),
        (retired_first, "lines retired before a move"),
        (top_scrolled, "a scroll of a region anchored at the top"),
        (
            taken_back,
            "a line taken back by the whole screen's scroll down",
        ),
        (past_half, "a program past half the ring"),
        (in_history, "a selection in the history"),
        (yes, "a selection reached"),
        (no, "a selection not reached"),
    ] {
        assert!(n > 0, "the generator never built {what}");
    }
}
