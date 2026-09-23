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
