//! **Scrollback is the grid as a ring, and the screen a window into it.** A
//! row that scrolls off the top of the primary screen stays where it was
//! written; the window advances over it and it becomes the newest line
//! behind the window, so a scroll copies nothing and the per-character loop
//! never learns the lines exist. These pin what retires and what does not,
//! and the resize that follows from it: a shorter primary screen retires its
//! top rows so the cursor's stay, and a taller one pulls them back.

mod support;

use baud_core::*;
use baud_domain::*;

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

fn row_text(t: &Term<'_>, y: usize, n: usize) -> String {
    text_of(&t.row(Row::from_index(y))[..n])
}

fn back(t: &Term<'_>, n: usize) -> (String, Wrap) {
    let sb = t.scrollback();
    let (c, w) = sb.line(Back::of(n)).expect("a retained line");
    (text_of(c), w)
}

/// Sixty numbered lines on a fifty-row screen retire eleven, newest first,
/// each to its mark and not to the width.
#[test]
fn a_scroll_retires_the_top_row_to_its_mark() {
    support::with_corpus(|t| {
        assert!(t.scrollback().is_empty());
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        assert_eq!(t.scrollback().len(), 11, "49 fit, eleven scrolled off");
        assert_eq!(back(t, 0), ("line 10".to_string(), Wrap::Ended));
        assert_eq!(back(t, 10), ("line 0".to_string(), Wrap::Ended));
        assert!(t.scrollback().line(Back::of(11)).is_none());
        assert_eq!(
            row_text(t, 0, 7),
            "line 11",
            "the screen starts where the history ends"
        );
    });
}

/// The alternate screen retires nothing, and a region that does not start
/// at the top retires nothing; a region anchored at the top does.
#[test]
fn only_the_primary_screen_and_a_top_anchored_region_retire() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h");
        let lines: Vec<u8> = (0..60).flat_map(|_| b"alt\r\n".to_vec()).collect();
        support::feed(t, &lines);
        assert!(
            t.scrollback().is_empty(),
            "the alternate screen has no history"
        );
        support::feed(t, b"\x1b[?1049l");
        // A region in the middle: rows 10..20, scrolled by an index at its
        // bottom. The rows above it are not the screen moving.
        support::feed(t, b"\x1b[10;20r\x1b[20;1Hmid\n\n\n\x1b[r");
        assert!(
            t.scrollback().is_empty(),
            "a region inside the screen retires nothing"
        );
        // A region anchored at the top, the status-line shape: it does.
        support::feed(t, b"\x1b[1;49r\x1b[1;1Htop line\x1b[49;1H\n");
        assert_eq!(t.scrollback().len(), 1);
        assert_eq!(back(t, 0).0, "top line");
    });
}

/// **A top-anchored region retires its scrolled line whatever its height.**
/// The line used to go to the history when the region was tall enough to
/// rotate (`1 ; 40`) and to be deleted when it copied (`1 ; 3`): one
/// operation, two fates, chosen by a cost heuristic. xterm saves when the top
/// margin is the first row and never tests the bottom (`[[source:
/// xterm-scroll]]`, `xtermScroll`), kitty adds to the history when
/// `margin_top == 0` (`[[source: kitty-screen-0-49]]`, `screen_index` and
/// `screen_scroll`), wezterm keeps it when `scroll_region.start == 0`
/// (`[[source: wezterm-screen-b09b56c]]`, `Screen::scroll_up`), and foot
/// keeps every region's (`[[source: foot-linefeed]]`, `term_scroll_partial`).
///
/// Heights 3, 10 and 25 took the copy on fifty rows, and 25 is the boundary:
/// the rows under it are as many as the rows in it. 40 rotated all along and
/// is the control. `LF` at the region's bottom and `SU` are the two ways in,
/// and the rows under the region must not move for either.
#[test]
fn a_top_anchored_region_retires_its_line_whatever_its_height() {
    for h in [3usize, 10, 25, 40] {
        support::with_corpus(|t| {
            let mut s = format!("\x1b[{};1HUNDER\x1b[50;1HLAST\x1b[1;{h}r\x1b[1;1H", h + 1);
            for i in 0..h {
                s.push_str(&format!("r{i:02}"));
                if i + 1 < h {
                    s.push_str("\r\n");
                }
            }
            // Two lines past the region's bottom: `LF` there scrolls it.
            s.push_str("\r\nx0\r\nx1");
            support::feed(t, s.as_bytes());
            assert_eq!(t.scrollback().len(), 2, "height {h}: two lines retired");
            assert_eq!(back(t, 0).0, "r01", "height {h}: the newest");
            assert_eq!(back(t, 1).0, "r00", "height {h}");
            assert_eq!(row_text(t, h - 1, 2), "x1", "height {h}: the bottom");
            assert_eq!(row_text(t, h, 5), "UNDER", "height {h}: under it");
            assert_eq!(row_text(t, 49, 4), "LAST", "height {h}");
            // `SU` scrolls the region wherever the cursor is.
            support::feed(t, b"\x1b[S");
            assert_eq!(t.scrollback().len(), 3, "height {h}: SU retires");
            assert_eq!(back(t, 0).0, "r02", "height {h}");
            assert_eq!(row_text(t, h, 5), "UNDER", "height {h}: SU");
            assert_eq!(row_text(t, 49, 4), "LAST", "height {h}: SU");
        });
    }
}

/// **The controls: a region not at the top, and the alternate screen,
/// retire nothing at any height.** xterm, kitty and wezterm discard a middle
/// region's line; foot keeps it, and baud follows the three. The alternate
/// screen keeps no history in any of them.
#[test]
fn a_short_region_off_the_top_or_on_the_alternate_screen_retires_nothing() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[5;7r\x1b[5;1Ha\r\nb\r\nc\r\nd\r\ne\x1b[S");
        assert!(t.scrollback().is_empty(), "a middle region");
        support::feed(
            t,
            b"\x1b[r\x1b[?1049h\x1b[1;3r\x1b[1;1Ha\r\nb\r\nc\r\nd\x1b[S",
        );
        assert!(t.scrollback().is_empty(), "the alternate screen");
    });
}

/// **A region narrower than the screen keeps nothing**, in xterm
/// (`[[source: xterm-scroll]]`, `scroll_full_line`) and in wezterm
/// (`[[source: wezterm-screen-b09b56c]]`, `scroll_up_within_margins`), because
/// what leaves it is part of a row. So the widening asks for full side
/// margins: a short top region under `DECSLRM` copies as it did and retires
/// nothing.
#[test]
fn a_short_top_region_under_side_margins_retires_nothing() {
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b[?69h\x1b[5;20s\x1b[1;3r\x1b[1;5Ha\r\nb\r\nc\r\nd\r\ne\x1b[S",
        );
        assert!(t.scrollback().is_empty());
    });
}

/// **Only a scroll retires.** `IL` and `DL` inside a region anchored at the
/// top move its rows and keep nothing, and `SD` in it takes nothing back
/// from the history -- xterm keeps what `DL` deletes at row 0
/// (`[[source: xterm-scroll]]`, `DeleteLine`), kitty and wezterm do not, and
/// baud follows the two.
#[test]
fn il_dl_and_sd_in_a_top_region_leave_the_history_alone() {
    support::with_corpus(|t| {
        // One line in the history first, so `SD` has something to take.
        support::feed(t, b"\x1b[1;3r\x1b[1;1Hkept\r\nb\r\nc\r\nd");
        assert_eq!(t.scrollback().len(), 1);
        assert_eq!(back(t, 0).0, "kept");
        support::feed(t, b"\x1b[1;1H\x1b[2M\x1b[1;1H\x1b[2L\x1b[T");
        assert_eq!(t.scrollback().len(), 1, "IL, DL and SD kept nothing");
        assert_eq!(back(t, 0).0, "kept", "and took nothing back");
    });
}

/// **`Term::region_scrolls` moves on a region scroll that advances the ring
/// and on nothing else**: not on the whole screen scrolling, not on a region
/// in the middle, and on a tall top region on the alternate screen too, which
/// rotates there. It did not move on the alternate screen, which keeps no
/// history, and a selection under vim's region there was renumbered as if
/// the rows under it had scrolled.
#[test]
fn the_region_count_moves_only_when_a_region_advances_the_ring() {
    support::with_corpus(|t| {
        let start = t.region_scrolls();
        let lines: Vec<u8> = (0..60).flat_map(|_| b"x\r\n".to_vec()).collect();
        support::feed(t, &lines);
        assert!(!t.scrollback().is_empty(), "the whole screen retired");
        assert_eq!(t.region_scrolls(), start, "and that is not a region");
        support::feed(t, b"\x1b[5;7r\x1b[7;1H\n\n\x1b[r");
        assert_eq!(t.region_scrolls(), start, "a middle region keeps nothing");
        support::feed(t, b"\x1b[1;3r\x1b[3;1H\n");
        let once = t.region_scrolls();
        assert_ne!(once, start, "a top region retired");
        support::feed(t, b"\x1b[S");
        assert_ne!(t.region_scrolls(), once, "and again for SU");
        let twice = t.region_scrolls();
        // A short region and a tall one on the alternate screen: the short
        // one copies there and does not move the count; the tall one
        // rotates, and does.
        support::feed(t, b"\x1b[r\x1b[?1049h\x1b[1;3r\x1b[3;1H\n\n\x1b[S");
        assert_eq!(
            t.region_scrolls(),
            twice,
            "a short region on the alternate screen copies"
        );
        support::feed(t, b"\x1b[1;40r\x1b[40;1H\n");
        assert_ne!(
            t.region_scrolls(),
            twice,
            "a tall one rotates there, and the rows under it stayed"
        );
    });
}

/// **The row count moves on every move of rows that advances no ring, and
/// on nothing that does.** A region in the middle, or a short top region
/// under side margins or on the alternate screen, scrolls by copying its
/// rows; `SD` and `RI` copy in any region smaller than the screen; `IL` and
/// `DL` copy wherever they act. Each leaves the rings where they were, so
/// nothing else across a read says the text on those rows moved. The whole
/// screen scrolling, and a top region rotating, advance a ring and say so
/// through the history and `region_scrolls`; they leave this alone.
#[test]
fn the_row_count_moves_when_rows_move_and_no_ring_does() {
    support::with_corpus(|t| {
        let moved = |t: &mut Term<'_>, bytes: &[u8]| {
            let before = t.row_moves();
            support::feed(t, bytes);
            t.row_moves() != before
        };
        // The controls.
        let lines: Vec<u8> = (0..60).flat_map(|_| b"x\r\n".to_vec()).collect();
        assert!(!moved(t, &lines), "the whole screen scrolling");
        assert!(
            !moved(t, b"\x1b[1;5r\x1b[5;1H\n\n\x1b[r"),
            "a top region rotating on the primary"
        );
        assert!(
            !moved(t, b"\x1b[1;1H\x1bM"),
            "RI at the top of the whole screen"
        );
        assert!(!moved(t, b"\x1b[5;7r\x1b[r"), "setting a region alone");
        // The copies.
        assert!(
            moved(t, b"\x1b[5;7r\x1b[7;1H\n\x1b[r"),
            "LF at the bottom of a middle region"
        );
        assert!(moved(t, b"\x1b[5;7r\x1b[S\x1b[r"), "SU in a middle region");
        assert!(moved(t, b"\x1b[5;7r\x1b[T\x1b[r"), "SD in a middle region");
        assert!(moved(t, b"\x1b[1;5r\x1b[T\x1b[r"), "SD in a top region");
        assert!(
            moved(t, b"\x1b[5;7r\x1b[5;1H\x1bM\x1b[r"),
            "RI at the top of a middle region"
        );
        assert!(moved(t, b"\x1b[10;1H\x1b[L"), "IL");
        assert!(moved(t, b"\x1b[10;1H\x1b[M"), "DL");
        assert!(
            moved(
                t,
                b"\x1b[?69h\x1b[1;100s\x1b[1;5r\x1b[5;1H\n\x1b[r\x1b[s\x1b[?69l"
            ),
            "LF at the bottom of a short top region under side margins"
        );
        assert!(
            moved(t, b"\x1b[?1049h\x1b[1;3r\x1b[3;1H\n\x1b[r"),
            "LF at the bottom of a short top region on the alternate screen"
        );
    });
}

/// **On the alternate screen a short top region still copies**, because
/// nothing there is kept and the copy moves the region's rows where the
/// rotation would move every row under it. Both render the same screen, so
/// the cell count is the only thing that can tell them apart: one `SU` of a
/// three-row region writes the region, three rows.
#[test]
fn the_alternate_screen_copies_a_short_top_region() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h\x1b[1;3r");
        let cols = t.geometry().0.index();
        let (_, before) = t.amplification();
        support::feed(t, b"\x1b[S");
        let (_, after) = t.amplification();
        assert_eq!(after - before, 3 * cols);
    });
}

/// **The region is reported as DECSTBM set it**, and as the whole screen when
/// none is: an embedder reads it to know which rows a region scroll left in
/// place.
#[test]
fn the_scroll_region_is_reported() {
    support::with_corpus(|t| {
        let last = Row::from_index(49);
        assert_eq!(t.scroll_region(), (Row::ZERO, last), "none set");
        support::feed(t, b"\x1b[1;5r");
        assert_eq!(t.scroll_region(), (Row::ZERO, Row::from_index(4)));
        support::feed(t, b"\x1b[3;40r");
        assert_eq!(t.scroll_region(), (Row::from_index(2), Row::from_index(39)));
        support::feed(t, b"\x1b[r");
        assert_eq!(t.scroll_region(), (Row::ZERO, last), "reset");
    });
}

/// **A top log pane over a fixed dashboard**, the shape a program draws when
/// it keeps its output in a few rows and a status display below them on the
/// primary screen. Thirty log lines through a five-row pane: twenty-five
/// scroll off, and all twenty-five are in the history, in order, with the
/// dashboard where it was drawn. No recording carries this shape, which is
/// why it is written here.
#[test]
fn a_top_log_pane_keeps_every_line_it_scrolls_off() {
    support::with_corpus(|t| {
        let mut s = String::from("\x1b[1;5r");
        for y in 6..=50 {
            s.push_str(&format!("\x1b[{y};1Hdash {y:02}"));
        }
        s.push_str("\x1b[1;1H");
        for i in 0..30 {
            if i > 0 {
                s.push_str("\r\n");
            }
            s.push_str(&format!("log {i:02}"));
        }
        support::feed(t, s.as_bytes());
        assert_eq!(t.scrollback().len(), 25);
        for n in 0..25 {
            assert_eq!(back(t, n).0, format!("log {:02}", 24 - n));
        }
        assert_eq!(row_text(t, 0, 6), "log 25");
        assert_eq!(row_text(t, 4, 6), "log 29");
        for y in 6..=50 {
            assert_eq!(row_text(t, y - 1, 7), format!("dash {y:02}"));
        }
    });
}

/// **The rows under a top region come back cell for cell, whatever row takes
/// them.** The repair copies each row under the region onto the row below it
/// only as far as the larger of the two rows' marks, and past that fills with
/// the source's last cell when the two rows' last cells differ. The rows here
/// need both halves: a status line erased in blue, whose mark is zero and
/// whose tail is not the default blank, lands on a line of red text and then
/// a short line lands on it; and a short last line lands, once the ring has
/// wrapped, on a recycled history line longer than it. The ring is three
/// lines deep so it wraps inside the test.
#[test]
fn the_rows_under_a_top_region_come_back_cell_for_cell() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::of(3));
    let mut t = Term::new(owned.storage(), &ws);
    support::feed(
        &mut t,
        b"\x1b[1;6r\x1b[7;1Hdash 07\x1b[8;1H\x1b[44m\x1b[2K\x1b[0m\
          \x1b[9;1H\x1b[31mred text\x1b[0m\x1b[10;1Hend",
    );
    let under = |t: &Term<'_>| {
        (6..10)
            .map(|y| t.row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>()
    };
    let want = under(&t);
    assert_eq!(want[0].len(), 40, "rows are read to the live edge");
    let mut s = String::from("\x1b[1;1H");
    for i in 0..40 {
        s.push_str(&format!("log {i:02} {}\r\n", "x".repeat(30)));
    }
    support::feed(&mut t, s.as_bytes());
    assert_eq!(t.scrollback().len(), 3, "the ring wrapped");
    assert_eq!(under(&t), want);
}

/// **A mark past the live edge is not a width.** At a prompt a narrower
/// screen is not re-cut, so a row written wide keeps its tail past the edge
/// and a mark there. The repair copies that row to the live edge and the
/// cells past it to the furthest the rows hold, and does not read past the
/// row; what a growth then shows is
/// `a_hidden_tail_under_a_top_region_comes_back_when_the_screen_grows`.
#[test]
fn a_row_with_a_hidden_tail_under_a_top_region_survives_a_scroll() {
    let ws = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let narrow = WinSize {
        ws_row: 10,
        ws_col: 20,
    };
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &ws);
    let mut s = String::from("\x1b[9;1H");
    s.push_str(&"S".repeat(30));
    s.push_str("\x1b[10;1H\x1b]133;A\x07");
    support::feed(&mut t, s.as_bytes());
    assert_eq!(t.resize(&narrow), Resized::Ok);
    support::feed(&mut t, b"\x1b[1;6r");
    assert_eq!(
        t.row_extent(Row::from_index(8)).index(),
        30,
        "at a prompt the narrower screen did not re-cut, so the tail is hidden"
    );
    let want = (6..10)
        .map(|y| t.row(Row::from_index(y)).to_vec())
        .collect::<Vec<_>>();
    let before = t.region_scrolls();
    support::feed(&mut t, b"\x1b[6;1H\n\n\n\n\n");
    assert_ne!(t.region_scrolls(), before, "the region rotated");
    let got = (6..10)
        .map(|y| t.row(Row::from_index(y)).to_vec())
        .collect::<Vec<_>>();
    assert_eq!(got, want);
    assert_eq!(t.row_extent(Row::from_index(8)).index(), 30);
}

/// The width a terminal is made at, before it is grown to 40.
#[derive(Clone, Copy)]
enum Made {
    Wide,
    Narrow,
}

/// The rows under a top region at 40 columns, then again after the screen
/// narrowed to 20 at a prompt, the region scrolled a line, and the screen
/// grew back to 40 at the same prompt: neither resize re-cuts, so the rows
/// under the region did not move and every cell a growth reveals is the one
/// that was there. `rows` draws rows 7 to 9, and row 10 is the prompt. One
/// line, because the rotation lands each row under the region on the
/// physical row that held the one below it, and a second scroll lands it on
/// another: one scroll is what puts row 8 over row 9's cells. The terminal
/// is made as `made` says and grown to 40 before anything is written.
fn under_a_region_through_a_prompt_resize(
    made: Made,
    rows: &[u8],
) -> (Vec<Vec<PackedCell>>, Vec<Vec<PackedCell>>) {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let narrow = WinSize {
        ws_row: 10,
        ws_col: 20,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let first = WinSize {
        ws_row: 10,
        ws_col: match made {
            Made::Wide => wide.ws_col,
            Made::Narrow => narrow.ws_col,
        },
    };
    let mut t = Term::new(owned.storage(), &first);
    assert_eq!(t.resize(&wide), Resized::Ok);
    support::feed(&mut t, rows);
    support::feed(&mut t, b"\x1b[10;1H\x1b]133;A\x07$ ");
    let under = |t: &Term<'_>| {
        (6..10)
            .map(|y| t.row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>()
    };
    let want = under(&t);
    assert_eq!(t.resize(&narrow), Resized::Ok);
    let before = t.region_scrolls();
    support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H\n\x1b[r\x1b[10;3H");
    assert_ne!(t.region_scrolls(), before, "the region rotated");
    assert_eq!(t.resize(&wide), Resized::Ok);
    (want, under(&t))
}

/// **A hidden tail goes with its row through the repair.** At a prompt a
/// narrower screen is not re-cut, so a row written wide keeps its tail past
/// the edge, under a mark there; the repair copied the row to the live edge
/// only, and the tail past it was the destination row's -- a growth back
/// showed twenty `S` where thirty were written.
#[test]
fn a_hidden_tail_under_a_top_region_comes_back_when_the_screen_grows() {
    let (want, got) = under_a_region_through_a_prompt_resize(
        Made::Wide,
        b"\x1b[7;1Hdash 07\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
    );
    assert_eq!(got[2], want[2], "row 9, thirty S");
}

/// **And a row the repair copies over one with a hidden tail does not take
/// that tail.** The rotation lands row 8 on the physical row that held row
/// 9's thirty `S`; copied to the live edge, row 8 showed its own text and
/// then ten of row 9's `S` past column 20.
#[test]
fn a_row_the_repair_copies_over_a_hidden_tail_does_not_show_it() {
    let (want, got) = under_a_region_through_a_prompt_resize(
        Made::Wide,
        b"\x1b[8;1Hdash 08\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
    );
    assert_eq!(got[1], want[1], "row 8, short");
}

/// **A row uniform in a colour to the old edge keeps it past the new one.**
/// A blue bar is uniform from column 0, so its mark is zero at either width
/// and says nothing about the twenty blue cells past the narrow edge; the
/// repair must carry them all the same.
#[test]
fn a_coloured_bar_under_a_top_region_comes_back_whole_when_the_screen_grows() {
    let (want, got) =
        under_a_region_through_a_prompt_resize(Made::Wide, b"\x1b[9;1H\x1b[44m\x1b[2K\x1b[0m");
    assert_eq!(got[2], want[2], "row 9, blue to column 40");
}

/// **A screen made narrow and grown carries its widest cells too.** How far
/// the rows hold is raised by every growth, not set once when the terminal
/// is made: made at 20 and grown to 40 before a line is written, the hidden
/// tail is the same thirty `S`.
#[test]
fn a_screen_made_narrow_carries_the_cells_it_grew_to() {
    let (want, got) = under_a_region_through_a_prompt_resize(
        Made::Narrow,
        b"\x1b[7;1Hdash 07\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
    );
    assert_eq!(got[2], want[2], "row 9, thirty S");
}

/// **The line a region scroll opens is blank past the narrow edge too.** The
/// rotation makes the region's bottom row of the physical row the first row
/// under it held, and the repair copies that row down before the bottom row
/// is blanked to the live edge -- so the cells past it were that row's: a
/// blue bar under the region left twenty blue cells on the new line, which
/// a growth showed. Past the edge the new line is the default blank, as an
/// erase taken while narrow leaves it
/// (`at_a_prompt_a_wider_screen_still_erases_a_band_to_the_old_edge`).
#[test]
fn the_line_a_region_scroll_opens_is_blank_past_the_narrow_edge() {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(
        &mut t,
        b"\x1b[7;1H\x1b[44m\x1b[2K\x1b[0m\x1b[10;1H\x1b]133;A\x07$ ",
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H\n\x1b[r\x1b[10;3H");
    assert_eq!(t.resize(&wide), Resized::Ok);
    assert!(
        t.row(Row::from_index(5))
            .iter()
            .all(|c| *c == PackedCell::EMPTY),
        "the region's new bottom line, blank to column 40"
    );
    assert!(
        t.row(Row::from_index(6))
            .iter()
            .all(|c| *c != PackedCell::EMPTY),
        "the control: the bar stayed on row 7, blue to column 40"
    );
}

/// **But a coloured blank runs on to the mark it reused**, as the blank of
/// any row whose mark lies past the edge does (`View::fill_row`). The line
/// opens on the row that held thirty `S` under a mark of thirty, with a red
/// background current: red to column 30, the default blank past it.
#[test]
fn the_line_a_region_scroll_opens_keeps_its_blank_to_the_mark_it_reused() {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(
        &mut t,
        b"\x1b[7;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\x1b[10;1H\x1b]133;A\x07$ ",
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(
        &mut t,
        b"\x1b[1;6r\x1b[6;1H\x1b[41m\n\x1b[0m\x1b[r\x1b[10;3H",
    );
    assert_eq!(t.resize(&wide), Resized::Ok);
    let line = t.row(Row::from_index(5));
    assert!(
        line[0] != PackedCell::EMPTY && line[..30].iter().all(|c| *c == line[0]),
        "red to column 30"
    );
    assert!(
        line[30..].iter().all(|c| *c == PackedCell::EMPTY),
        "the default blank past it"
    );
    // And the row it held went down whole: its cells past the edge were
    // carried before the blank ran on through them.
    let s: Vec<_> = t.row(Row::from_index(6))[..31]
        .iter()
        .map(|c| c.content.as_codepoint().map(|cp| cp.value()))
        .collect();
    assert_eq!(
        s,
        [vec![Some(u32::from(b'S')); 30], vec![None]].concat(),
        "row 7, thirty S"
    );
}

/// The cells one scroll of a top region writes on a 20-column screen made
/// at `first` columns: `before` is fed at that width, the screen narrows to
/// 20 when it was made wider, and rows 7 to 9 are drawn under the region
/// before the scroll. The storage is provisioned for 40 either way.
fn cells_a_region_scroll_writes(first: u16, before: &[u8]) -> usize {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let narrow = WinSize {
        ws_row: 10,
        ws_col: 20,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(
        owned.storage(),
        &WinSize {
            ws_row: 10,
            ws_col: first,
        },
    );
    support::feed(&mut t, before);
    if first != narrow.ws_col {
        assert_eq!(t.resize(&narrow), Resized::Ok);
    }
    support::feed(&mut t, b"\x1b[7;1Hdash 07\x1b[8;1Hdash 08\x1b[9;1Hdash 09");
    let (_, a) = t.amplification();
    let scrolls = t.region_scrolls();
    support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H\n\x1b[r");
    assert_ne!(t.region_scrolls(), scrolls, "the region rotated");
    let (_, b) = t.amplification();
    b - a
}

/// **A narrowing that leaves nothing past the edge carries nothing past
/// it.** The repair carries each row's cells past the live edge to the
/// furthest any row of the ring holds one, and that is measured at the
/// resize, not remembered as the widest the screen has been: a screen made
/// at 40 and narrowed to 20 at a prompt before a line past column 20 was
/// written scrolls a top region for the cells one made at 20 does. Taken
/// as the widest width instead, the 40-column screen copied twenty cells
/// past the edge for every row under the region, every scroll -- the log
/// pane over a dashboard read 54% down after a narrowing.
#[test]
fn a_narrowing_with_nothing_past_the_edge_carries_nothing_past_it() {
    let prompt = b"\x1b[10;1H\x1b]133;A\x07$ ";
    assert_eq!(
        cells_a_region_scroll_writes(40, prompt),
        cells_a_region_scroll_writes(20, prompt)
    );
}

/// **Nor does one a re-cut leaves bare.** Thirty `S` at 40 columns, in
/// output, re-cut to 20 when the screen narrows: the re-cut blanks every
/// row it writes past the new width, and every other row of the ring is as
/// provisioned, so nothing lies past the edge and a scroll writes what it
/// writes on a screen made at 20 that took the same line.
#[test]
fn a_recut_narrowing_carries_nothing_past_the_edge() {
    let line = b"SSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\r\n";
    assert_eq!(
        cells_a_region_scroll_writes(40, line),
        cells_a_region_scroll_writes(20, line)
    );
}

/// **The control: a hidden tail keeps the reach.** Thirty `S` on row 9 at a
/// prompt, narrowed: the tail past column 20 is a cell a growth shows, so
/// the scroll carries the rows under the region past the edge and writes
/// more than a screen made at 20 -- the comparison above can fail.
#[test]
fn a_narrowing_over_a_hidden_tail_still_carries_past_the_edge() {
    let tail = b"\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\x1b[10;1H\x1b]133;A\x07$ ";
    assert!(cells_a_region_scroll_writes(40, tail) > cells_a_region_scroll_writes(20, tail));
}

/// **A mark past the edge holds the reach where the cells do not.** A
/// `DCH` raises a row's mark to the width, so at 40 columns a row reading
/// `ab` has a mark of 40 over blank cells. Narrowed at a prompt, an erase
/// of that row in red runs on to its mark (`View::fill_row`), forty red
/// cells, twenty past the edge. A reach measured from the cells alone sat
/// at 20 when the erase wrote past it, and the scroll after it left the
/// twenty on the physical row the repair moved the line off.
#[test]
fn a_mark_past_the_edge_keeps_the_reach_a_blank_runs_on_to() {
    let row9 = |scroll: bool| {
        let wide = WinSize {
            ws_row: 10,
            ws_col: 40,
        };
        let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
        let mut t = Term::new(owned.storage(), &wide);
        support::feed(
            &mut t,
            b"\x1b[9;1Hab\x1b[9;3H\x1b[P\x1b[10;1H\x1b]133;A\x07$ ",
        );
        assert_eq!(
            t.row_extent(Row::from_index(8)).index(),
            40,
            "the DCH's mark"
        );
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 10,
                ws_col: 20
            }),
            Resized::Ok
        );
        support::feed(&mut t, b"\x1b[9;1H\x1b[41m\x1b[2K\x1b[0m\x1b[10;3H");
        if scroll {
            support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H\n\x1b[r\x1b[10;3H");
        }
        assert_eq!(t.resize(&wide), Resized::Ok);
        t.row(Row::from_index(8)).to_vec()
    };
    let (want, got) = (row9(false), row9(true));
    assert!(
        want.iter()
            .all(|c| *c == want[0] && *c != PackedCell::EMPTY),
        "the control: red to column 40 with no scroll"
    );
    assert_eq!(got, want, "row 9 after the scroll");
}

/// **The reach is the whole ring's, not the window's.** A tail written at
/// 40 columns on row 3, retired into the history by a shorter screen as
/// it narrows at a prompt, then pulled back onto it by a taller one: the
/// rows on screen at the narrowing hold nothing past column 20, the row in
/// the history does, and the repair it comes back under carries it. A reach
/// measured over the window lost ten of its thirty `S`.
#[test]
fn a_tail_the_history_holds_at_the_narrowing_keeps_the_reach() {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(
        &mut t,
        b"\x1b[3;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\x1b[10;1H\x1b]133;A\x07$ ",
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 5,
            ws_col: 20
        }),
        Resized::Ok
    );
    assert!(
        (0..5).all(|y| !row_text(&t, y, 20).contains('S')),
        "the control: the tail's row left the screen"
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    assert_eq!(
        row_text(&t, 2, 20),
        "S".repeat(20),
        "and came back to row 3"
    );
    let scrolls = t.region_scrolls();
    support::feed(&mut t, b"\x1b[1;2r\x1b[2;1H\n\x1b[r\x1b[10;3H");
    assert_ne!(t.region_scrolls(), scrolls, "the region rotated");
    assert_eq!(t.resize(&wide), Resized::Ok);
    assert_eq!(row_text(&t, 2, 31), format!("{} ", "S".repeat(30)), "row 3");
}

/// **A long line above the rows under a region costs them nothing past the
/// edge.** Thirty `S` on row 3 at 40 columns, kept past the edge by a
/// narrowing at a prompt: the ring holds a cell past column 20, so the reach
/// stays at 30, but no row under the region holds one, and the repair carries
/// a row past the edge only where it or the row it lands on does. Copying
/// every row under the region past the edge whenever any row of the ring
/// held anything, one line of history wider than the narrowed screen cost
/// the narrowed log pane 54%: 95.7 MB/s against 208.4.
#[test]
fn a_long_line_above_the_rows_under_a_region_costs_them_nothing_past_the_edge() {
    let line = b"\x1b[3;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS\x1b[10;1H\x1b]133;A\x07$ ";
    assert_eq!(
        cells_a_region_scroll_writes(40, line),
        cells_a_region_scroll_writes(20, line)
    );
}

/// Rows of cells, a screen's or a band's.
type Band = Vec<Vec<PackedCell>>;

/// A 40-column screen with two lines of history: `top` on row 1, three
/// rows under a region drawn at 7 to 9 and a prompt on row 10, narrowed to
/// 20 there, then `scrolls` lines through the region `1 ; 6`. Returns the
/// rows under the region at 40 columns before the narrowing, the same rows
/// after the screen grew back to 40 at the prompt, the screen's text after
/// it, and the cells the last of the scrolls wrote. With the ring twelve
/// rows, row 1's physical row comes round as the bottom row under the
/// region on the third scroll.
fn a_ring_brought_round(top: &[u8], scrolls: usize) -> (Band, Band, Vec<String>, usize) {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(2));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(&mut t, b"\x1b[1;1H");
    support::feed(&mut t, top);
    support::feed(
        &mut t,
        b"\x1b[7;1Hdash 07\x1b[8;1Hdash 08\x1b[9;1Hdash 09\x1b[10;1H\x1b]133;A\x07$ ",
    );
    let under = |t: &Term<'_>| {
        (6..10)
            .map(|y| t.row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>()
    };
    let want = under(&t);
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H");
    let before = t.region_scrolls();
    let mut last = 0;
    for _ in 0..scrolls {
        let (_, a) = t.amplification();
        support::feed(&mut t, b"\n");
        let (_, b) = t.amplification();
        last = b - a;
    }
    assert_ne!(t.region_scrolls(), before, "the region rotated");
    support::feed(&mut t, b"\x1b[r\x1b[10;3H");
    assert_eq!(t.resize(&wide), Resized::Ok);
    let text = (0..10).map(|y| row_text(&t, y, 40)).collect();
    (want, under(&t), text, last)
}

/// **A long line the ring brings round under the region is carried off.**
/// Its physical row comes round as the bottom row under the region, which
/// the repair fills with the prompt's row: past the edge it is the
/// destination that holds something, and the copy has to run for it, or the
/// ten `S` past column 20 stay under the prompt and a growth shows them.
#[test]
fn a_long_line_the_ring_brings_round_under_a_region_is_carried_off() {
    let (want, got, text, _) = a_ring_brought_round(&[b'S'; 30], 12);
    assert_eq!(got, want, "the rows under the region, to column 40");
    assert!(
        text.iter().all(|row| !row.contains('S')),
        "the line left the ring: {text:?}"
    );
}

/// **And then the cost falls.** Once the repair has carried the long line
/// off, nothing past the edge is held anywhere, and a scroll writes what it
/// writes where row 1 held ten `S` -- nothing past the edge at the narrowing
/// at all. Measured only at the resize, the reach stayed at the long line's
/// width, and every scroll after copied every row under the region past the
/// edge, however long the line had been gone.
#[test]
fn a_narrowing_over_one_long_line_costs_nothing_once_the_ring_carries_it_off() {
    let (_, _, _, long) = a_ring_brought_round(&[b'S'; 30], 13);
    let (_, _, _, short) = a_ring_brought_round(&[b'S'; 10], 13);
    assert_eq!(long, short);
}

/// A 40-column screen with twenty lines of history: `rows` drawn and a
/// prompt on row 10, narrowed to 20 there, `ops` fed, the region reset and
/// the cursor back at the prompt, and grown back to 40. Returns every screen
/// row at 40 columns before the narrowing and after the growth. Neither
/// resize re-cuts, so every cell a growth reveals is the one that was there.
fn through_a_prompt_narrowing(
    rows: &[u8],
    ops: &[u8],
) -> (Vec<Vec<PackedCell>>, Vec<Vec<PackedCell>>) {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(&mut t, rows);
    support::feed(&mut t, b"\x1b[10;1H\x1b]133;A\x07$ ");
    let screen = |t: &Term<'_>| {
        (0..10)
            .map(|y| t.row(Row::from_index(y)).to_vec())
            .collect::<Vec<_>>()
    };
    let want = screen(&t);
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(&mut t, ops);
    support::feed(&mut t, b"\x1b[r\x1b[10;3H");
    assert_eq!(t.resize(&wide), Resized::Ok);
    (want, screen(&t))
}

fn is_blank(row: &[PackedCell]) -> bool {
    row.iter().all(|c| *c == PackedCell::EMPTY)
}

/// **`IL` moves a hidden tail with its row, and the line it opens is blank
/// past the edge.** Thirty `S` on row 8 under the region `1 ; 9`, narrowed
/// at a prompt, then `IL` there: the row goes down to row 9. `IL` moved it
/// to the live edge, so it came back as twenty `S`, and the line it opened
/// on row 8 kept the ten past the edge.
#[test]
fn il_carries_a_hidden_tail_and_blanks_the_line_it_opens_past_the_edge() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[8;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        b"\x1b[1;9r\x1b[8;1H\x1b[L",
    );
    assert_eq!(got[8], want[7], "row 9, thirty S");
    assert!(is_blank(&got[7]), "row 8, opened, blank to column 40");
}

/// **And a bar uniform in a colour, whose mark is zero.** A blue bar on row
/// 8 goes down whole to row 9, and the line `IL` opens on row 8 is the
/// default blank past the edge, as it is inside it.
#[test]
fn il_carries_a_coloured_bar_whole() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[8;1H\x1b[44m\x1b[2K\x1b[0m",
        b"\x1b[1;9r\x1b[8;1H\x1b[L",
    );
    assert_eq!(got[8], want[7], "row 9, blue to column 40");
    assert!(is_blank(&got[7]), "row 8, opened, blank to column 40");
}

/// **`DL`, the same both ways.** Thirty `S` on row 9 under the region
/// `1 ; 9`, `DL` on row 8: the row comes up to row 8 whole, and the line
/// opened at the region's bottom, on the row that held it, is blank past the
/// edge.
#[test]
fn dl_carries_a_hidden_tail_and_blanks_the_line_it_opens_past_the_edge() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        b"\x1b[1;9r\x1b[8;1H\x1b[M",
    );
    assert_eq!(got[7], want[8], "row 8, thirty S");
    assert!(is_blank(&got[8]), "row 9, opened, blank to column 40");
}

/// A blue bar on row 9, `DL` on row 8: row 8 blue to column 40, row 9 the
/// default blank to it.
#[test]
fn dl_carries_a_coloured_bar_whole() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[9;1H\x1b[44m\x1b[2K\x1b[0m",
        b"\x1b[1;9r\x1b[8;1H\x1b[M",
    );
    assert_eq!(got[7], want[8], "row 8, blue to column 40");
    assert!(is_blank(&got[8]), "row 9, opened, blank to column 40");
}

/// **A region that does not start at the top scrolls by copying**, through
/// `DL`'s engine, and carries a hidden tail the same way: thirty `S` on row
/// 5 under the region `2 ; 9`, a line feed at its bottom, and the row is on
/// row 4 whole.
#[test]
fn a_copying_region_scroll_carries_a_hidden_tail() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[5;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        b"\x1b[2;9r\x1b[9;1H\n",
    );
    assert_eq!(got[3], want[4], "row 4, thirty S");
    assert!(is_blank(&got[8]), "row 9, opened, blank to column 40");
}

/// And down, through `IL`'s: a reverse index at the top of the region
/// `2 ; 9` moves the row on row 5 to row 6 whole.
#[test]
fn a_copying_region_scroll_down_carries_a_hidden_tail() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[5;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        b"\x1b[2;9r\x1b[2;1H\x1bM",
    );
    assert_eq!(got[5], want[4], "row 6, thirty S");
}

/// The ring full of blue bars -- forty lines of the blue blank through a
/// ring of thirty rows -- narrowed to 20 at a prompt, `scroll` fed, then a
/// prompt and a growth back to 40: the screen's rows after it.
fn rows_a_full_screen_scroll_brings_in(scroll: &[u8]) -> Vec<Vec<PackedCell>> {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(&mut t, b"\x1b[44m\x1b[2J\x1b[10;1H");
    support::feed(&mut t, &[b'\n'; 40]);
    support::feed(&mut t, b"\x1b[0m\x1b]133;A\x07$ ");
    assert!(
        t.row(Row::from_index(0))
            .iter()
            .all(|c| *c != PackedCell::EMPTY),
        "the control: a bar to column 40"
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(&mut t, scroll);
    support::feed(&mut t, b"\x1b[10;1H$ ");
    assert_eq!(t.resize(&wide), Resized::Ok);
    (0..10)
        .map(|y| t.row(Row::from_index(y)).to_vec())
        .collect()
}

/// **The line a full-screen scroll brings in is blank past the edge.**
/// Twelve line feeds on the bar-filled ring: each line comes in on a ring
/// row that held a bar, blanked to the live edge, and a growth showed it
/// blue past column 20.
#[test]
fn the_line_a_full_screen_scroll_brings_in_is_blank_past_the_edge() {
    let rows =
        rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n\r\n");
    for (y, row) in rows[..9].iter().enumerate() {
        assert!(
            is_blank(row),
            "row {}, a line fed in, blank to column 40",
            y + 1
        );
    }
}

/// **And through `SU`**, which scrolls the whole screen by the same
/// rotation and blank, on its own path.
#[test]
fn the_lines_su_brings_in_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\x1b[12S");
    for (y, row) in rows[..9].iter().enumerate() {
        assert!(
            is_blank(row),
            "row {}, a line SU brought in, blank to column 40",
            y + 1
        );
    }
}

/// **Every line a scroll brings in, once the scrolls have gone round the
/// ring.** Forty line feeds through a ring of thirty rows: each row the
/// window's bottom swept is cleared past the edge however many times it was
/// swept, and the clear covers the whole ring once.
#[test]
fn the_lines_a_scroll_brings_in_past_a_whole_ring_are_blank_past_the_edge() {
    let mut scroll = Vec::new();
    for _ in 0..40 {
        scroll.extend_from_slice(b"\r\n");
    }
    let rows = rows_a_full_screen_scroll_brings_in(&scroll);
    for (y, row) in rows[..9].iter().enumerate() {
        assert!(
            is_blank(row),
            "row {}, a line fed in, blank to column 40",
            y + 1
        );
    }
}

/// **A line a scroll brought in is cleared before `DL` moves it.** The rows
/// a full-screen scroll swept are cleared past the edge when something next
/// reads or moves rows there, and they are found by where they lie: three
/// line feeds bring lines in on rows 8 to 10, then `DL` on row 2 moves them
/// up a row. Cleared after the move, by position, the line moved onto row 7
/// kept its retired bar past column 20; the bars above, moved too, keep
/// theirs.
#[test]
fn lines_a_scroll_brought_in_and_dl_moved_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[2;1H\x1b[M");
    assert!(
        rows[1].iter().all(|c| *c != PackedCell::EMPTY),
        "the control: row 2, a bar moved up, blue to column 40"
    );
    for (y, row) in rows.iter().enumerate().take(9).skip(6) {
        assert!(
            is_blank(row),
            "row {}, a line fed in and moved up, blank to column 40",
            y + 1
        );
    }
}

/// **And before a top region's repair carries it.** Three line feeds, then
/// a scroll of the region `1 ; 5`: the lines brought in are rows under the
/// region, and its repair put them back on other rows with their retired
/// bars past the edge, counted then as rows the repair had put right.
#[test]
fn lines_a_scroll_brought_in_under_a_region_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[1;5r\x1b[5;1H\n");
    for (y, row) in rows.iter().enumerate().take(9).skip(7) {
        assert!(
            is_blank(row),
            "row {}, a line fed in under the region, blank to column 40",
            y + 1
        );
    }
}

/// **And before a scroll down moves the window back over them.** Three line
/// feeds, then a reverse index at the top: the lines brought in go down a
/// row, and counted from the window's new place they were rows nothing had
/// swept.
#[test]
fn lines_a_scroll_brought_in_then_scrolled_down_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[1;1H\x1bM");
    assert!(is_blank(&rows[0]), "row 1, taken back, blank to column 40");
    assert!(
        is_blank(&rows[8]),
        "row 9, a line fed in, blank to column 40"
    );
}

/// **A scroll down on the alternate screen forgets none of them.** The
/// primary's swept rows are counted by its top line, which the alternate
/// screen does not move: three line feeds, then a reverse index at the top
/// of the alternate screen and back. Restarting the count there, on the
/// alternate screen's scroll, lost the three lines' rows, and a growth
/// showed their retired bars.
#[test]
fn lines_a_scroll_brought_in_survive_a_scroll_down_on_the_alternate_screen() {
    let rows =
        rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[?1049h\x1b[1;1H\x1bM\x1b[?1049l");
    for (y, row) in rows.iter().enumerate().take(9).skip(7) {
        assert!(
            is_blank(row),
            "row {}, a line fed in, blank to column 40",
            y + 1
        );
    }
}

/// **And before a prompt's redraw moves the window back over them.** A
/// prompt block of thirteen lines on a ten-row screen, every line after its
/// first brought in by a line feed on a row that held a blue bar, narrowed
/// before them; the shell erases its last rows and redraws, and the
/// window moves back three rows to replace the block where it began.
/// Counted from the window's new place, those lines' rows were rows nothing
/// had swept, and a growth showed their retired bars.
#[test]
fn lines_a_scroll_brought_in_and_a_prompt_redraw_took_back_are_blank_past_the_edge() {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(&mut t, b"\x1b[44m\x1b[2J\x1b[10;1H");
    support::feed(&mut t, &[b'\n'; 40]);
    support::feed(&mut t, b"\x1b[0m\x1b]133;A\x07");
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    for _ in 0..12 {
        support::feed(&mut t, b"p\r\n");
    }
    let erased = t.prompt_erases;
    support::feed(&mut t, b"\x1b[8;1H\x1b[J\x1b]133;A\x07$ ");
    assert_eq!(
        t.prompt_erases,
        erased + 1,
        "the control: the block was replaced"
    );
    assert_eq!(t.resize(&wide), Resized::Ok);
    for y in 1..10 {
        assert!(
            is_blank(t.row(Row::from_index(y))),
            "row {}, a line of the block, blank to column 40",
            y + 1
        );
    }
}

/// **A hidden tail goes with its row through many scrolls.** Each scroll of
/// the region `1 ; 6` lands the rows under it on other physical rows, so the
/// flag that says a row holds something past the edge has to go with its
/// cells, or the second scroll finds both rows clear and leaves the tail
/// behind: thirty `S` on row 9, five lines, and a growth.
#[test]
fn a_hidden_tail_under_a_region_survives_many_scrolls_and_a_growth() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[9;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS",
        b"\x1b[1;6r\x1b[6;1H\n\n\n\n\n",
    );
    assert_eq!(got[8], want[8], "row 9, thirty S");
}

/// **And the line a scroll down takes back.** The newest line of history,
/// a blue bar, comes back as the top row a reverse index at the top opens:
/// blanked to the live edge, it kept the bar past it.
#[test]
fn the_line_a_full_screen_scroll_down_brings_in_is_blank_past_the_edge() {
    let (want, got) = through_a_prompt_narrowing(
        b"\x1b[44m\x1b[2J\x1b[10;1H\n\x1b[0m\x1b[1;1H\x1b[2K",
        b"\x1b[1;1H\x1bM",
    );
    assert!(is_blank(&want[0]), "the control: row 1 blank at 40");
    assert!(is_blank(&got[0]), "row 1, opened, blank to column 40");
    assert_eq!(got[1], want[0], "row 2, the old row 1");
}

/// **And before `IL` moves it.** Three line feeds, then `IL` on row 2: the
/// lines brought in go down a row, and the bar on row 7 goes down onto row
/// 8. Cleared after the move, by position, row 8's bar lost its cells past
/// column 20 and the line moved onto row 9 kept its retired bar.
#[test]
fn lines_a_scroll_brought_in_and_il_moved_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[2;1H\x1b[L");
    assert!(
        rows[7].iter().all(|c| *c != PackedCell::EMPTY),
        "row 8, a bar moved down, blue to column 40"
    );
    assert!(
        is_blank(&rows[8]),
        "row 9, a line fed in and moved down, blank to column 40"
    );
}

/// **And before a copying region scroll moves it**, up through `DL`'s
/// engine: three line feeds, then a line feed at the bottom of the region
/// `2 ; 10`, and the lines brought in go up a row.
#[test]
fn lines_a_scroll_brought_in_and_a_copying_scroll_moved_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[2;10r\x1b[10;1H\n");
    for (y, row) in rows.iter().enumerate().take(9).skip(6) {
        assert!(
            is_blank(row),
            "row {}, a line fed in and moved up, blank to column 40",
            y + 1
        );
    }
}

/// And down, through `IL`'s: a reverse index at the top of the region
/// `2 ; 10` moves the bar on row 7 onto row 8 and a line brought in onto
/// row 9.
#[test]
fn lines_a_scroll_brought_in_and_a_copying_scroll_down_moved_are_blank_past_the_edge() {
    let rows = rows_a_full_screen_scroll_brings_in(b"\r\n\r\n\r\n\x1b[2;10r\x1b[2;1H\x1bM");
    assert!(
        rows[7].iter().all(|c| *c != PackedCell::EMPTY),
        "row 8, a bar moved down, blue to column 40"
    );
    assert!(
        is_blank(&rows[8]),
        "row 9, a line fed in and moved down, blank to column 40"
    );
}

/// **The rows a top region's own scrolls bring in are not counted as swept.**
/// The repair puts them right as it goes, so the count restarts after it:
/// `SU 3` in the region `1 ; 6` is three scrolls in one call, and counted,
/// the clear at the growth took the bottom three rows under the region for
/// lines a scroll brought in and cut row 9's blue bar, whose mark is zero,
/// at column 20.
#[test]
fn a_coloured_bar_under_a_region_survives_three_scrolls_in_one_call() {
    let (want, got) =
        through_a_prompt_narrowing(b"\x1b[9;1H\x1b[44m\x1b[2K\x1b[0m", b"\x1b[1;6r\x1b[3S");
    assert!(
        want[8].iter().all(|c| *c != PackedCell::EMPTY),
        "the control: row 9 blue to column 40"
    );
    assert_eq!(got[8], want[8], "row 9, blue to column 40");
}

/// **A row the repair clears past the edge is clear after**, so the ring
/// does not bring it round as a row to copy. Thirty `S` on row 7, the first
/// row under the region `1 ; 6`, narrowed at a prompt: every scroll copies
/// the tail down onto a new physical row and opens the line on the one it
/// left, cleared past the edge. Left held there, each scroll added a row the
/// ring brought round under the region and copied past the edge, and from
/// the ring's first lap the scrolls cost more each time.
#[test]
fn a_row_the_repair_clears_past_the_edge_is_not_copied_again() {
    let wide = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let mut owned = Owned::with_history(&wide, &CacheGeometry::UNKNOWN, Depth::of(2));
    let mut t = Term::new(owned.storage(), &wide);
    support::feed(&mut t, b"\x1b[7;1HSSSSSSSSSSSSSSSSSSSSSSSSSSSSSS");
    support::feed(
        &mut t,
        b"\x1b[8;1Hdash 08\x1b[9;1Hdash 09\x1b[10;1H\x1b]133;A\x07$ ",
    );
    assert_eq!(
        t.resize(&WinSize {
            ws_row: 10,
            ws_col: 20
        }),
        Resized::Ok
    );
    support::feed(&mut t, b"\x1b[1;6r\x1b[6;1H");
    let costs: Vec<usize> = (0..30)
        .map(|_| {
            let (_, a) = t.amplification();
            support::feed(&mut t, b"\n");
            let (_, b) = t.amplification();
            b - a
        })
        .collect();
    assert!(
        costs.iter().all(|c| *c == costs[0]),
        "every scroll the same cells: {costs:?}"
    );
}

/// **The repair counts the cells it writes**, which is the rows under the
/// region to their marks, not to the width. Three rows of four glyphs under a
/// top region, one scroll: each row is copied to a mark of four, and the
/// region's new bottom row is blanked to its mark of four.
#[test]
fn the_repair_counts_the_cells_it_writes() {
    support::with_corpus(|t| {
        support::feed(
            t,
            b"\x1b[48;1Hdash\x1b[49;1Hdash\x1b[50;1Hdash\x1b[1;47r\x1b[47;1H",
        );
        let (_, before) = t.amplification();
        support::feed(t, b"\n");
        let (_, after) = t.amplification();
        assert_eq!(after - before, 3 * 4 + 4);
    });
}

/// **A shorter primary screen retires its top rows so the cursor's stay.**
/// Fifty numbered rows, cursor on the last; shrink to ten: the screen shows
/// rows 40..49, the cursor is on row 9, and the forty rows are in history.
#[test]
fn a_shorter_screen_retires_its_top_rows_and_keeps_the_cursors() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..50)
            .flat_map(|i| format!("row {i}{}", if i < 49 { "\r\n" } else { "" }).into_bytes())
            .collect();
        support::feed(t, &lines);
        assert_eq!(t.cursor().0.index(), 49);
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 200,
            },
        );
        assert_eq!(t.scrollback().len(), 40);
        assert_eq!(back(t, 0).0, "row 39");
        assert_eq!(row_text(t, 0, 6), "row 40");
        assert_eq!(row_text(t, 9, 6), "row 49");
        assert_eq!(t.cursor().0.index(), 9, "the cursor came with its row");
    });
}

/// **A taller screen pulls them back**, in order, and a shrink then a growth
/// is the identity on the screen.
#[test]
fn a_taller_screen_pulls_the_retired_rows_back() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i} {}\r\n", "x".repeat(100)).into_bytes())
            .collect();
        support::feed(t, &lines);
        let before: Vec<String> = (0..50).map(|y| row_text(t, y, 200)).collect();
        let depth = t.scrollback().len();
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 200,
            },
        );
        assert_eq!(t.scrollback().len(), depth + 40);
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        let after: Vec<String> = (0..50).map(|y| row_text(t, y, 200)).collect();
        assert_eq!(after, before, "every row is back where it was");
        assert_eq!(
            t.scrollback().len(),
            depth,
            "and the history is what it was"
        );
        assert_eq!(t.cursor().0.index(), 49);
    });
}

/// Growing past what the history holds shows blank rows, not the stale
/// copies of lines already retired.
#[test]
fn growing_past_the_history_shows_blank_rows() {
    support::with_room(120, |t| {
        let lines: Vec<u8> = (0..49)
            .flat_map(|i| format!("row {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        assert!(t.scrollback().is_empty(), "nothing scrolled");
        support::shrink(
            t,
            &WinSize {
                ws_row: 10,
                ws_col: 200,
            },
        );
        assert_eq!(t.scrollback().len(), 40);
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 100,
                ws_col: 200,
            }),
            Resized::Ok
        );
        assert!(t.scrollback().is_empty(), "all forty came back");
        assert_eq!(row_text(t, 0, 5), "row 0");
        assert_eq!(row_text(t, 48, 6), "row 48");
        for y in 50..100 {
            assert!(
                row_text(t, y, 200).trim().is_empty(),
                "row {y} is blank, not a stale copy"
            );
        }
    });
}

/// **Which cut rows a growth reveals is decided by the row's mark, and a
/// mark is not a picture of the row.** The height rule reveals the cut rows
/// "as far as the last one that holds anything" and pulls history after --
/// the owner's "pull down to the bottom, and blank-fill only when nothing is
/// left to pull" -- and it asks a cut row whether it holds anything by
/// reading its mark. A mark says where a row stops being uniform, which is
/// neither "shows something" nor its converse, so the test misfires both
/// ways. The three below are pinned at what the core does, not at what the
/// rule's words ask: what a cut row has to hold to count is the owner's to
/// define, it is on the list, and each of these goes red the day it moves.
///
/// Sixty numbered lines on the corpus screen (eleven in the history), the
/// rows below row 25 reshaped by `cut`, the cursor on row 20, a shrink to 25
/// rows that cuts them, and a growth by one.
fn grow_over_cut_rows(t: &mut Term<'_>, cut: impl FnOnce(&mut Term<'_>)) {
    let lines: Vec<u8> = (0..60)
        .flat_map(|i| format!("line {i:03}\r\n").into_bytes())
        .collect();
    support::feed(t, &lines);
    cut(t);
    support::feed(t, b"\x1b[21;1H");
    let (cols, _) = t.geometry();
    let at = |rows: u16| WinSize {
        ws_row: rows,
        ws_col: u16::try_from(cols.index()).unwrap(),
    };
    support::shrink(t, &at(25));
    assert_eq!(
        t.scrollback().len(),
        11,
        "the shrink cut, it retired nothing"
    );
    assert_eq!(t.resize(&at(26)), Resized::Ok);
}

/// Rows 26 to 50 of the corpus screen, one-based, each reshaped by `row`.
fn each_cut_row(t: &mut Term<'_>, row: impl Fn(usize) -> String) {
    let bytes: Vec<u8> = (26..=50).flat_map(|y| row(y).into_bytes()).collect();
    support::feed(t, &bytes);
}

/// The control: cut rows that hold text come back before any history does.
#[test]
fn a_growth_reveals_a_cut_row_of_text_before_pulling_history() {
    support::with_corpus(|t| {
        grow_over_cut_rows(t, |_| {});
        assert_eq!(t.scrollback().len(), 11, "nothing pulled");
        assert_eq!(row_text(t, 0, 8), "line 011");
        assert_eq!(
            row_text(t, 25, 8),
            "line 036",
            "the first cut row came back"
        );
    });
}

/// **Pinned: a cut row of invisible blanks is revealed as content.** Each cut
/// row is default blanks and then ten blanks erased in a foreground-only
/// rendition -- red text colour, default background, no attribute -- which
/// draw nothing. The mark sits past them, so the row counts, and the growth
/// shows a row that looks blank while a history line was left to pull.
#[test]
fn pinned_a_cut_row_of_invisible_blanks_is_revealed_as_content() {
    support::with_corpus(|t| {
        grow_over_cut_rows(t, |t| {
            each_cut_row(t, |y| {
                format!("\x1b[{y};1H\x1b[2K\x1b[{y};5H\x1b[31m\x1b[10X\x1b[0m")
            })
        });
        assert_eq!(t.scrollback().len(), 11, "no history pulled");
        assert_eq!(row_text(t, 0, 8), "line 011");
        assert!(
            row_text(t, 25, 200).trim().is_empty(),
            "the revealed row holds no character"
        );
    });
}

/// **Pinned: a cut row in a visible colour is left cut.** Each cut row is
/// erased whole in a red background, which is uniform, so its mark is zero,
/// the row does not count, and the growth pulls a history line over a red
/// band that stays below the window.
#[test]
fn pinned_a_cut_row_uniform_in_a_visible_colour_is_left_cut() {
    support::with_corpus(|t| {
        grow_over_cut_rows(t, |t| {
            each_cut_row(t, |y| format!("\x1b[{y};1H\x1b[41m\x1b[2K\x1b[0m"))
        });
        assert_eq!(t.scrollback().len(), 10, "a history line was pulled");
        assert_eq!(row_text(t, 0, 8), "line 010");
        assert_eq!(
            row_text(t, 25, 8),
            "line 035",
            "and the red row is still cut"
        );
    });
}

/// **Pinned: a band past the old edge counts once a width growth has raised
/// its mark**, which is the owner's live drag at steps 641 to 654, in
/// miniature. At a prompt the shell erases to the line's end in a
/// foreground-only rendition while the screen is 36 wide; later a default
/// erase from column 0 at a narrower width leaves the invisible band past the
/// edge with a mark of zero -- a mark true up to the live edge and not past
/// it. (The drag reaches that in two steps, a mark of 14 at 22 columns and 0
/// at 21; this test takes one, at 22.) A growth to 27 columns raises the mark
/// to 22, which is what lets a later erase clear the band, and it also makes
/// the row count: the height growth after it reveals the blank-looking row.
/// Without the width growth the mark stays zero and the same height growth
/// pulls a history line, which is the contrast this test carries in one
/// binary.
#[test]
fn pinned_a_band_past_the_old_edge_counts_once_a_growth_raised_its_mark() {
    fn band(widen: bool) -> (usize, String, String) {
        support::with_corpus(|t| {
            let at = |rows: u16, cols: u16| WinSize {
                ws_row: rows,
                ws_col: cols,
            };
            let lines: Vec<u8> = (0..60)
                .flat_map(|i| format!("line {i:03}\r\n").into_bytes())
                .collect();
            support::feed(t, &lines);
            support::feed(t, b"\x1b]133;A\x07");
            assert_eq!(t.resize(&at(50, 36)), Resized::Ok);
            each_cut_row(t, |y| {
                format!("\x1b[{y};1H\x1b[2K\x1b[{y};14H\x1b[31m\x1b[K\x1b[0m")
            });
            assert_eq!(t.resize(&at(50, 22)), Resized::Ok);
            each_cut_row(t, |y| format!("\x1b[{y};1H\x1b[K"));
            support::feed(t, b"\x1b[21;1H");
            assert_eq!(t.resize(&at(25, 22)), Resized::Ok);
            assert_eq!(
                t.scrollback().len(),
                11,
                "the shrink cut, it retired nothing"
            );
            let cols = if widen { 27 } else { 22 };
            assert_eq!(t.resize(&at(25, cols)), Resized::Ok);
            assert_eq!(t.resize(&at(26, cols)), Resized::Ok);
            assert_eq!(t.primary_zone(), Zone::Prompt, "no re-cut ran");
            (
                t.scrollback().len(),
                row_text(t, 0, 8),
                row_text(t, 25, usize::from(cols)),
            )
        })
    }
    let (hist, first, revealed) = band(true);
    assert_eq!(hist, 11, "after the width growth nothing was pulled");
    assert_eq!(first, "line 011");
    assert!(
        revealed.trim().is_empty(),
        "the revealed row holds no character"
    );
    let (hist, first, _) = band(false);
    assert_eq!(hist, 10, "without it a history line was pulled");
    assert_eq!(first, "line 010");
}

/// A storage built with no depth retains nothing from a scroll: its ring is
/// the screen, so the row that leaves the top is the row that comes in at
/// the bottom. A shorter window still keeps the rows it cut -- they are in
/// the ring the screen already has -- and a taller one shows them again.
#[test]
fn no_depth_is_no_history() {
    let mut owned = Owned::with_history(&support::CORPUS, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    let lines: Vec<u8> = (0..60).flat_map(|_| b"x\r\n".to_vec()).collect();
    support::feed(&mut t, &lines);
    assert!(t.scrollback().is_empty());
    support::shrink(
        &mut t,
        &WinSize {
            ws_row: 10,
            ws_col: 200,
        },
    );
    assert_eq!(
        t.scrollback().len(),
        40,
        "the cut rows are behind the window, in the ring the screen already has"
    );
    assert_eq!(row_text(&t, 8, 1), "x");
    assert!(
        row_text(&t, 9, 1).trim().is_empty(),
        "the cursor's row, blank"
    );
}

/// The ring is the screen's capacity plus the depth, on the primary; the
/// alternate screen's ring is the capacity alone, so a scroll there rotates
/// blanks into blanks and retains nothing.
#[test]
fn the_ring_is_the_capacity_plus_the_depth() {
    let mut owned = Owned::with_history(&support::CORPUS, &CacheGeometry::UNKNOWN, Depth::of(7));
    assert_eq!(owned.storage().ring_rows().index(), 57);
    assert_eq!(owned.ring().index(), 57);
    assert_eq!(owned.rows().index(), 50);
    let mut t = Term::new(owned.storage(), &support::CORPUS);
    assert_eq!(t.ring().index(), 57, "the primary's ring");
    let lines: Vec<u8> = (0..60).flat_map(|_| b"x\r\n".to_vec()).collect();
    support::feed(&mut t, &lines);
    assert_eq!(t.scrollback().len(), 7, "ten scrolled off, seven kept");
    support::feed(&mut t, b"\x1b[?1049h");
    assert_eq!(t.ring().index(), 50, "the alternate's ring is the capacity");
    support::feed(&mut t, &lines);
    assert_eq!(
        t.scrollback().len(),
        7,
        "the alternate screen retains nothing"
    );
    support::feed(&mut t, b"\x1b[?1049l");
    assert_eq!(t.ring().index(), 57);
    assert_eq!(row_text(&t, 48, 1), "x", "the primary is back, unmoved");
}

/// `Term::view(back)` is the screen scrolled `back` lines into the history:
/// its top row is the line that far above the live screen, it is bounded by
/// what is retained, and the alternate screen -- which keeps none -- shows
/// itself whatever is asked.
#[test]
fn a_view_back_starts_that_many_lines_above_the_screen() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        assert_eq!(t.scrollback().len(), 11);
        let text = |v: Viewport<'_>, y: usize| text_of(&v.row(Row::from_index(y))[..7]);
        assert_eq!(
            text(t.view(Rows::ZERO), 0),
            "line 11",
            "back zero is the screen"
        );
        assert_eq!(text(t.view(Rows::of_count(1)), 0), "line 10");
        assert_eq!(text(t.view(Rows::of_count(11)), 0), "line 0 ");
        assert_eq!(text(t.view(Rows::of_count(11)), 11), "line 11");
        assert_eq!(
            text(t.view(Rows::of_count(500)), 0),
            "line 0 ",
            "bounded by what is retained"
        );
        support::feed(t, b"\x1b[?1049h\x1b[1;1Halt");
        assert_eq!(
            text(t.view(Rows::of_count(5)), 0),
            "alt    ",
            "the alternate screen has no history to look back into"
        );
    });
}

/// **A row that wrapped says so, and a row a newline ended does not.** The
/// flag rides in the row mark, so it costs no store the write did not make,
/// and the scrollback reads it back as `Wrap::Continues`.
#[test]
fn a_wrapped_row_continues_and_a_newline_ends_it() {
    support::with_corpus(|t| {
        let long = "x".repeat(450);
        support::feed(t, long.as_bytes());
        support::feed(t, b"\r\nshort\r\n");
        let vp = t.viewport();
        assert_eq!(
            vp.wrapped(Row::from_index(0)),
            Wrap::Continues,
            "200 of 450"
        );
        assert_eq!(
            vp.wrapped(Row::from_index(1)),
            Wrap::Continues,
            "400 of 450"
        );
        assert_eq!(
            vp.wrapped(Row::from_index(2)),
            Wrap::Ended,
            "the tail, ended by the newline"
        );
        assert_eq!(vp.wrapped(Row::from_index(3)), Wrap::Ended);
        assert_eq!(
            t.row_extent(Row::from_index(0)).index(),
            200,
            "the mark is the mark"
        );
        // Erasing a wrapped row to its end ends it: what the row held is gone.
        support::feed(t, b"\x1b[1;1H\x1b[2K");
        assert_eq!(t.viewport().wrapped(Row::from_index(0)), Wrap::Ended);
        // And a line that scrolled off reads back with its wrap.
        let lines: Vec<u8> = (0..60).flat_map(|_| b"y\r\n".to_vec()).collect();
        support::feed(t, long.as_bytes());
        support::feed(t, b"\r\n");
        support::feed(t, &lines);
        let sb = t.scrollback();
        let mut seen = 0;
        for n in 0..sb.len() {
            let (cells, w) = sb.line(Back::of(n)).expect("retained");
            if cells.len() == 200 {
                assert_eq!(w, Wrap::Continues, "a full row that wrapped, {n} back");
                seen += 1;
            } else {
                assert_eq!(w, Wrap::Ended);
            }
        }
        assert_eq!(seen, 2, "the two full rows of the long line");
    });
}

#[test]
fn moved_since_reads_a_rotation_the_short_way_round() {
    // Twelve lines onto an eight-row screen retire four; a growth of two
    // pulls two back. Both are the short way round a ring of thousands of
    // rows, which is what an embedder anchoring a scrolled-back window
    // must not compute for itself.
    // Provisioned at ten rows and started at eight, so the growth is real:
    // a storage sized to the short screen answers it with `Truncated`.
    let mut owned = Owned::new(&WinSize {
        ws_col: 40,
        ws_row: 10,
    });
    let mut t = Term::new(
        owned.storage(),
        &WinSize {
            ws_col: 40,
            ws_row: 8,
        },
    );
    {
        let t = &mut t;
        {
            let before = t.rotation();
            for i in 0..12 {
                support::feed(t, format!("line {i}\r\n").as_bytes());
            }
            assert_eq!(t.moved_since(before), Moved::Retired(Rows::of_count(5)));
            let before = t.rotation();
            let _ = t.resize(&WinSize {
                ws_col: 40,
                ws_row: 10,
            });
            assert_eq!(t.moved_since(before), Moved::Restored(Rows::of_count(2)));
            assert_eq!(
                t.moved_since(t.rotation()),
                Moved::Retired(Rows::of_count(0))
            );
        }
    }
}

/// **The clamp every reader of a scrolled-back window has to ask.**
/// `looking_back` is what `Term::view` applies: the history on the primary,
/// and nothing on the alternate screen -- beside a history count that still
/// reports the primary's while the alternate screen is up, which is what a
/// reader that skips the clamp reads instead, and what drew a thumb over a
/// screen with nothing behind it.
#[test]
fn looking_back_is_the_history_on_the_primary_and_nothing_on_the_alternate_screen() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        assert_eq!(t.scrollback().len(), 11);
        assert_eq!(t.looking_back(Rows::ZERO).index(), 0);
        assert_eq!(t.looking_back(Rows::of_count(5)).index(), 5);
        assert_eq!(
            t.looking_back(Rows::of_count(500)).index(),
            11,
            "bounded by what is retained"
        );
        support::feed(t, b"\x1b[?1049h");
        assert_eq!(
            t.scrollback().len(),
            11,
            "the history is still the primary's, and still reported"
        );
        assert_eq!(
            t.looking_back(Rows::of_count(5)).index(),
            0,
            "the alternate screen has no window over it"
        );
        assert_eq!(t.looking_back(Rows::of_count(11)).index(), 0);
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(t.looking_back(Rows::of_count(5)).index(), 5);
    });
}

/// **The history mark measures the primary, whatever screen is up.** The
/// alternate screen scrolling rotates its own ring and not the primary's,
/// so a window over the history must not follow it; the primary's rows
/// retired before a switch are still counted while the alternate screen is
/// live; and a read that ends back on the primary counts what the primary
/// scrolled on both sides of the excursion. Only the first of the three
/// tells this mark from the live rotation: the other two are held to it so
/// a mark that answered nothing on the alternate screen, or only read the
/// primary's ring when it is live, cannot pass.
#[test]
fn the_history_mark_moves_with_the_primary_and_not_the_alternate_screen() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..60)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        let alt = |from: usize, to: usize| -> Vec<u8> {
            (from..to)
                .flat_map(|i| format!("alt {i}\r\n").into_bytes())
                .collect()
        };
        // The alternate screen scrolls; the primary does not.
        // The control reads the alternate screen's own ring over ten rows:
        // it is the screen's capacity, fifty, and the short way round a
        // ring of fifty cannot tell thirty forward from twenty back.
        let mark = t.history_mark();
        support::feed(t, b"\x1b[?1049h\x1b[50;1H");
        let live = t.rotation();
        support::feed(t, &alt(0, 10));
        assert_eq!(
            t.moved_since(live),
            Moved::Retired(Rows::of_count(10)),
            "the control: the alternate screen's own ring did turn"
        );
        support::feed(t, &alt(10, 30));
        assert_eq!(
            t.history_moved_since(mark),
            Moved::Retired(Rows::ZERO),
            "and the primary's did not"
        );
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(t.history_moved_since(mark), Moved::Retired(Rows::ZERO));
        // Measured while the alternate screen is up, after the primary
        // moved: three rows retired before the switch are three rows, not
        // nothing because the alternate screen is live.
        let mark = t.history_mark();
        support::feed(t, b"a\r\nb\r\nc\r\n\x1b[?1049h\x1b[50;1H");
        support::feed(t, &alt(0, 30));
        assert_eq!(
            t.history_moved_since(mark),
            Moved::Retired(Rows::of_count(3))
        );
        // And a read that ends back on the primary: two more after it.
        support::feed(t, b"\x1b[?1049ld\r\ne\r\n");
        assert_eq!(
            t.history_moved_since(mark),
            Moved::Retired(Rows::of_count(5))
        );
    });
}

/// **A flood is counted, not read round a ring.** Forty lines on a
/// ten-row screen with twenty lines of history retire thirty-one rows in
/// one read: more than half of a thirty-row ring, which the short way round
/// reads as one, and more than the whole ring, which it cannot read at all.
/// A window scrolled back through that had its lines scroll away above it,
/// and the count that says so is the line at the window's top.
#[test]
fn the_history_mark_counts_a_flood_past_the_ring() {
    let ws = WinSize {
        ws_col: 20,
        ws_row: 10,
    };
    let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &ws);
    assert_eq!(t.ring().index(), 30);
    let (mark, live) = (t.history_mark(), t.rotation());
    let flood: Vec<u8> = (0..40)
        .flat_map(|i| format!("line {i}\r\n").into_bytes())
        .collect();
    support::feed(&mut t, &flood);
    assert_eq!(
        t.moved_since(live),
        Moved::Retired(Rows::of_count(1)),
        "the control: the ring's rotation came round to one"
    );
    assert_eq!(
        t.history_moved_since(mark),
        Moved::Retired(Rows::of_count(31))
    );
}

/// **The screen epoch moves on every switch and on nothing else**, so a
/// read that left the alternate screen and entered it again -- the same
/// screen at both ends, a cleared ring between -- is seen to have switched.
#[test]
fn the_screen_epoch_moves_on_every_switch_and_nothing_else() {
    support::with_corpus(|t| {
        let first = t.screen_epoch();
        support::feed(t, b"text\r\n\x1b[2J\x1b[?1049l\x1b[1;5r\x1b[r");
        assert_eq!(
            t.screen_epoch(),
            first,
            "output, and a leave while not in it"
        );
        support::feed(t, b"\x1b[?1049h");
        let entered = t.screen_epoch();
        assert_ne!(entered, first);
        support::feed(t, b"\x1b[?1049h\x1b[Hvi");
        assert_eq!(t.screen_epoch(), entered, "an entry while in it");
        support::feed(t, b"\x1b[?1049l");
        let left = t.screen_epoch();
        assert_ne!(left, entered, "leaving is a switch too");
        support::feed(t, b"\x1b[?1049h");
        let again = t.screen_epoch();
        assert!(
            again != left && again != entered,
            "and so is entering again"
        );
        support::feed(t, b"\x1b[?1049l\x1b[?1049h");
        assert_eq!(t.screen(), Screen::Alternate);
        assert_ne!(
            t.screen_epoch(),
            again,
            "left and entered again in one read: the same screen, not the same surface"
        );
    });
}

/// **The reset count moves on a full reset and on nothing else** (note V,
/// V1): `ESC c` from the primary, from the alternate screen, and on a fresh
/// terminal with no history -- where nothing an embedder reads across a read
/// changes, the case an inference from the history cannot see -- and once
/// for each of two resets in one read. Not on `DECSTR`, `ED 2`, `CSI ? 3 J`,
/// a `?1049` round trip, a whole-screen or a region scroll, a resize, or
/// `CSI 3 J`, which moves the erasure count and leaves the screen as it was:
/// the one operation that tells this count from that one.
#[test]
fn the_reset_count_moves_on_a_full_reset_and_nothing_else() {
    support::with_corpus(|t| {
        let fresh = t.resets();
        support::feed(t, b"\x1bc");
        assert!(t.scrollback().is_empty(), "the control: no history at all");
        let first = t.resets();
        assert_ne!(first, fresh, "a reset with no history moves it");
        let mut lines = Vec::new();
        for i in 0..80 {
            lines.extend_from_slice(format!("line {i}\r\n").as_bytes());
        }
        support::feed(t, &lines);
        assert!(!t.scrollback().is_empty(), "the control: a history");
        let erasures = t.history_erasures();
        for (bytes, what) in [
            (&b"\x1b[!p"[..], "DECSTR"),
            (b"\x1b[2J", "ED 2"),
            (b"\x1b[?3J", "DECSED 3"),
            (
                b"\x1b[?1049h\x1b[?1049l",
                "a round trip to the alternate screen",
            ),
            (b"\x1b[50;1Hx\r\ny\r\n", "a whole-screen scroll"),
            (b"\x1b[1;5r\x1b[5;1Hx\r\ny\r\n\x1b[r", "a region scroll"),
        ] {
            support::feed(t, bytes);
            assert_eq!(t.resets(), first, "{what} moved it");
        }
        assert_eq!(
            t.history_erasures(),
            erasures,
            "the control: nothing erased yet"
        );
        support::feed(t, b"\x1b[3J");
        assert_ne!(
            t.history_erasures(),
            erasures,
            "the control: ED 3 erased the history"
        );
        assert_eq!(t.resets(), first, "ED 3 moved it");
        support::feed(t, &lines);
        support::feed(t, b"\x1bc");
        let second = t.resets();
        assert_ne!(second, first, "a reset with a history moves it");
        support::feed(t, b"\x1b[?1049h\x1b[Hvi");
        assert_eq!(t.resets(), second, "entering the alternate screen");
        support::feed(t, b"\x1bc");
        assert_eq!(t.screen(), Screen::Normal, "the control: RIS left it");
        assert_ne!(
            t.resets(),
            second,
            "a reset from the alternate screen moves it"
        );
    });
    // Two resets in one read move it twice: as far as two reads of one each,
    // and further than one.
    let after = |reads: &[&[u8]]| {
        support::with_corpus(|t| {
            for r in reads {
                support::feed(t, r);
            }
            t.resets()
        })
    };
    let one = after(&[b"\x1bc"]);
    let two = after(&[b"\x1bc\x1bc"]);
    assert_eq!(two, after(&[b"\x1bc", b"\x1bc"]), "two in one read are two");
    assert_ne!(two, one, "and not one");
    // A read that splits `ESC` from `c` is still one reset.
    assert_eq!(after(&[b"\x1b", b"c"]), one, "split across two reads");
    // A resize, both ways, moves it neither way.
    support::with_room(60, |t| {
        let before = t.resets();
        for rows in [60, 20, 50] {
            let ws = WinSize {
                ws_row: rows,
                ws_col: support::CORPUS.ws_col,
            };
            assert_eq!(t.resize(&ws), Resized::Ok, "the control: {rows} rows fit");
            assert_eq!(t.resets(), before, "a resize to {rows} rows moved it");
        }
    });
}

// **A full reset clears the history, and leaves nothing to reveal below the
// window.** `RIS` reset the primary's rotation and left its history counts,
// so the scrollback read rows the reset had not written at the old length --
// never-written rows on a ring that had not wrapped, stale lines on one that
// had -- a taller window pulled the window back over them, and rows a shrink
// had hidden below it came back after the reset that should have erased
// them. xterm's `ESC c` clears its saved lines where `DECSTR` keeps them
// (`[[source: xterm-reset]]`); alacritty clears the history on both grids and
// returns the view to the live screen (`[[source: alacritty-reset-state]]`,
// `[[source: alacritty-grid-reset]]`); and foot's `ESC c` is
// `term_reset(term, true)` (`[[source: foot-vt-ris]]`), whose hard path sets
// the ring's offset and view to zero and frees every row past the screen
// (`[[source: foot-term-reset]]`). The Linux console
// (`[[source: linux-vt-ris]]`) and sfte (`[[source: sfte-ris]]`) keep the
// history on `ESC c`, and no pinned specification says.
// The reference implementation and the two terminals this repo compares
// against agree, and that is what is followed.

/// Every line in the primary's history and on its screen, as text.
fn everything(t: &Term<'_>) -> Vec<String> {
    let sb = t.scrollback();
    let rows = t.geometry().1.index();
    (0..sb.len())
        .rev()
        .map(|n| {
            let (c, _) = sb.line(Back::of(n)).expect("a retained line");
            text_of(c).trim_end().to_string()
        })
        .chain((0..rows).map(|y| text_of(t.row(Row::from_index(y))).trim_end().to_string()))
        .collect()
}

/// The real `reset(1)`, captured: `seq 1 45; reset; seq 101 105` on a
/// twenty-row pty. Tab stops, `ESC c`, `DECSTR`, the modes, then output.
/// `tests/reset/README.md` says how it was made.
#[test]
fn a_captured_reset_leaves_no_history() {
    let raw = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tests/reset/seq-reset.raw"
    ))
    .expect("the capture");
    assert!(
        raw.windows(2).any(|w| w == b"\x1bc"),
        "the control: the capture carries an ESC c"
    );
    let ws = WinSize {
        ws_col: 60,
        ws_row: 20,
    };
    // The control: the capture had a history to clear. Everything before its
    // `ESC c` retires twenty-six lines on twenty rows.
    let at = raw
        .windows(2)
        .position(|w| w == b"\x1bc")
        .expect("the capture's ESC c");
    support::with_term(&ws, |t| {
        support::feed(t, &raw[..at]);
        assert_eq!(
            t.scrollback().len(),
            26,
            "the control: history before the reset"
        );
    });
    for n in [1, 2, 3, 4, 5, 8, 17, 4096] {
        support::with_term(&ws, |t| {
            for chunk in raw.chunks(n) {
                support::feed(t, chunk);
            }
            assert_eq!(
                t.scrollback().len(),
                0,
                "read size {n}: the reset cleared the history"
            );
            let all = everything(t);
            let want: Vec<String> = (101..106)
                .map(|i| i.to_string())
                .chain((0..15).map(|_| String::new()))
                .collect();
            assert_eq!(all, want, "read size {n}: only what came after the reset");
        });
    }
}

#[test]
fn a_full_reset_clears_the_history_and_the_next_lines_refill_it() {
    for n in [1, 4096] {
        support::with_corpus(|t| {
            let lines = |tag: &str| -> Vec<u8> {
                (0..60)
                    .flat_map(|i| format!("{tag} {i}\r\n").into_bytes())
                    .collect()
            };
            for c in lines("old").chunks(n) {
                support::feed(t, c);
            }
            assert_eq!(t.scrollback().len(), 11, "read size {n}: the control");
            for c in b"\x1bc".chunks(n) {
                support::feed(t, c);
            }
            assert_eq!(t.scrollback().len(), 0, "read size {n}: cleared");
            assert!(t.scrollback().line(Back::NEWEST).is_none(), "read size {n}");
            assert_eq!(
                t.looking_back(Rows::of_count(5)),
                Rows::ZERO,
                "read size {n}: a window scrolled back has nowhere to be"
            );
            for c in lines("new").chunks(n) {
                support::feed(t, c);
            }
            assert_eq!(t.scrollback().len(), 11, "read size {n}");
            let all = everything(t);
            assert!(
                all.iter().all(|l| !l.starts_with("old")),
                "read size {n}: a line from before the reset came back:\n{all:#?}"
            );
            assert_eq!(all[0], "new 0", "read size {n}");
            assert_eq!(all[59], "new 59", "read size {n}");
        });
    }
}

/// Rows a shrink cut below the cursor are content a taller window brings
/// back -- until a reset erases the screen they belonged to.
#[test]
fn a_full_reset_leaves_nothing_hidden_below_the_window() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..30)
            .flat_map(|i| format!("old {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        support::feed(t, b"\x1b[6;1H");
        support::shrink(
            t,
            &WinSize {
                ws_row: 20,
                ws_col: 200,
            },
        );
        assert_eq!(t.cursor().0.index(), 5, "the cursor kept its row");
        support::feed(t, b"\x1bc");
        assert_eq!(
            t.resize(&WinSize {
                ws_row: 50,
                ws_col: 200,
            }),
            Resized::Ok
        );
        let all = everything(t);
        assert!(
            all.iter().all(|l| l.is_empty()),
            "a row hidden before the reset came back after it:\n{all:#?}"
        );
        // The control: the same shrink and growth with no reset between
        // brings the cut rows back, so the rows were really there.
        support::feed(t, &lines);
        support::feed(t, b"\x1b[6;1H");
        support::shrink(
            t,
            &WinSize {
                ws_row: 20,
                ws_col: 200,
            },
        );
        let _ = t.resize(&WinSize {
            ws_row: 50,
            ws_col: 200,
        });
        assert_eq!(
            text_of(t.row(Row::from_index(29))).trim_end(),
            "old 29",
            "the control: without a reset the cut rows come back"
        );
    });
}

/// A reset erases; it does not scroll. The window's movement across one is
/// none, so an embedder anchoring a selection or a scrolled-back window by
/// the rotation reads nothing -- where resetting the rotation read a row
/// count the short way round a ring.
#[test]
fn a_full_reset_scrolls_nothing() {
    support::with_corpus(|t| {
        let lines: Vec<u8> = (0..73)
            .flat_map(|i| format!("line {i}\r\n").into_bytes())
            .collect();
        support::feed(t, &lines);
        let (live, mark) = (t.rotation(), t.history_mark());
        assert_ne!(live, RowRot::NONE, "the control: the ring has turned");
        support::feed(t, b"\x1bc");
        assert_eq!(t.moved_since(live), Moved::Retired(Rows::ZERO));
        assert_eq!(t.history_moved_since(mark), Moved::Retired(Rows::ZERO));
    });
}

/// Every row of the text in the ring, as the core holds it.
fn ring_text(t: &Term<'_>) -> Vec<String> {
    t.cells()
        .chunks(t.stride().index())
        .map(|r| text_of(r).trim_end().to_string())
        .collect()
}

/// **Nothing from before a full reset is left in the ring.** The history
/// count going to zero makes the lines unreadable through the scrollback,
/// and `Term::cells` hands the whole ring to a reader that applies the
/// rotation itself -- so a ring whose rows kept their bytes still held what
/// the user ran `reset` to erase, and a narrowing re-cut that spills into
/// those rows can bring their tails back on screen. foot frees every row
/// past the screen and xterm empties its saved lines; the ring here is
/// erased. The first arm is a ring that has not wrapped, the second one that
/// has, where the rows behind rotation zero hold real text rather than rows
/// nothing wrote -- the face of the defect the first arm cannot show.
#[test]
fn a_full_reset_leaves_nothing_of_the_old_text_in_the_ring() {
    let ws = WinSize {
        ws_col: 60,
        ws_row: 20,
    };
    for (depth, lines, arm) in [
        (Depth::DEFAULT, 45usize, "a ring that has not wrapped"),
        (Depth::of(20), 100, "a ring that has wrapped"),
    ] {
        let mut owned = Owned::with_history(&ws, &CacheGeometry::UNKNOWN, depth);
        let mut t = Term::new(owned.storage(), &ws);
        let old: Vec<u8> = (0..lines)
            .flat_map(|i| format!("old {i}\r\n").into_bytes())
            .collect();
        support::feed(&mut t, &old);
        assert!(
            ring_text(&t).iter().any(|l| l.starts_with("old")),
            "{arm}: the control, the ring holds the old text"
        );
        support::feed(&mut t, b"\x1bc");
        let left: Vec<String> = ring_text(&t)
            .into_iter()
            .filter(|l| !l.is_empty())
            .collect();
        assert!(left.is_empty(), "{arm}: the ring kept {left:?}");
        let new: Vec<u8> = (0..lines)
            .flat_map(|i| format!("new {i}\r\n").into_bytes())
            .collect();
        support::feed(&mut t, &new);
        let sb = t.scrollback();
        let back: Vec<String> = (0..sb.len())
            .map(|n| {
                text_of(sb.line(Back::of(n)).expect("a retained line").0)
                    .trim_end()
                    .to_string()
            })
            .collect();
        assert!(
            back.iter().all(|l| l.starts_with("new")),
            "{arm}: a line from before the reset is in the history: {back:?}"
        );
    }
}
