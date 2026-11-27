//! **Every store to a cell or a mark stamps its row, and a seal closes a
//! generation**: the cases the generated programs in `properties.rs`
//! (`writes_move_the_rows_they_touch`) cannot reach -- a mark joining a cell
//! an earlier frame wrote, the API calls (`resize`, the host's calls that
//! draw the indicator line again), the erases of the history, a full reset
//! from either screen -- and a seal from another terminal.
//!
//! Each case seals, does one thing, and asks the question a renderer asks:
//! [`Term::row_gens`]' `after`, [`Term::status_gen`]'s, or
//! [`Term::wrote_since`]. A store that skipped its stamp answers "clean" for
//! a row whose cells changed, and the renderer keeping its copy of that row
//! draws it stale.

mod support;

use baud_core::*;
use baud_domain::{PromptAtBottom, RegionTop};

fn ws(rows: u16, cols: u16) -> WinSize {
    WinSize {
        ws_row: rows,
        ws_col: cols,
    }
}

/// Every physical row of surface `w` is written after `kept`.
fn all_written(t: &Term<'_>, w: Which, kept: &Seal) -> bool {
    t.stored_rows(w, kept)
        .all(|(_, _, _, d)| d == RowDirt::Written)
}

/// The physical rows of surface `w` written after `kept`.
fn written(t: &Term<'_>, w: Which, kept: &Seal) -> Vec<usize> {
    t.stored_rows(w, kept)
        .filter(|(_, _, _, d)| *d == RowDirt::Written)
        .map(|(at, _, _, _)| at.p.index())
        .collect()
}

/// **F1's case: a combining mark joining a cell an earlier frame wrote moves
/// its row**, though the row's mark already covers the cell and does not
/// move. A stamp that waited for the mark to rise left the row clean, and a
/// renderer kept drawing the base without its accent.
#[test]
fn a_mark_joining_a_cell_written_last_frame_moves_its_row() {
    support::with_term(&ws(10, 20), |t| {
        support::feed(t, b"a");
        let s = t.seal_writes();
        let mark = t.row_extent(Row::ZERO);
        assert!(mark.index() >= 1, "the row's mark covers the cell");
        support::feed(t, "\u{301}".as_bytes());
        assert_eq!(t.row_extent(Row::ZERO), mark, "the mark did not move");
        assert_eq!(
            t.marks(Row::ZERO, Col::ZERO).count(),
            1,
            "the accent joined"
        );
        assert!(t.row_gens(Rows::ZERO).after(Row::ZERO, &s));
        assert!(t.wrote_since(&s));
        assert!(
            !t.row_gens(Rows::ZERO).after(Row::from_index(1), &s),
            "the control"
        );
    });
}

/// **A resize moves every row of every surface**: it re-cuts, reveals and
/// blanks by more paths than are worth naming, and stamps the lot once, at
/// its end -- a call that keeps the geometry included, which the marks'
/// pass a change of geometry runs over both screens does not reach.
#[test]
fn a_resize_moves_every_row_of_every_surface() {
    for to in [ws(8, 15), ws(10, 20)] {
        support::with_term(&ws(10, 20), |t| {
            support::feed(t, b"some text\r\nmore");
            let s = t.seal_writes();
            assert_eq!(t.resize(&to), Resized::Ok);
            for w in [Which::Primary, Which::Alternate, Which::Status] {
                assert!(
                    all_written(t, w, &s),
                    "{}x{} {w:?}: a row reads clean",
                    to.ws_col,
                    to.ws_row
                );
            }
        });
    }
}

/// **A full-screen scroll's incoming row moves**: with no history, the row
/// a line feed at the bottom brings in is the one that left the top, and it
/// is blanked (`View::fill_new_row`).
#[test]
fn a_line_feed_at_the_bottom_moves_the_row_it_brings_in() {
    let w = ws(50, 20);
    let mut owned = Owned::with_history(&w, &CacheGeometry::UNKNOWN, Depth::NONE);
    let mut t = Term::new(owned.storage(), &w);
    let full: Vec<u8> = (0..50).flat_map(|_| *b"x\r\n").collect();
    support::feed(&mut t, &full[..full.len() - 2]);
    let s = t.seal_writes();
    support::feed(&mut t, b"\r\n");
    let last = Row::from_index(49);
    assert!(t.row(last).expect("a row")[0].content.is_empty(), "blanked");
    assert!(t.row_gens(Rows::ZERO).after(last, &s));
}

/// The terminal with sixty lines written on a fifty-row screen, twenty of
/// history kept.
fn with_history<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let w = ws(50, 20);
    let mut owned = Owned::with_history(&w, &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &w);
    let lines: Vec<u8> = (0..60).flat_map(|_| *b"hist\r\n").collect();
    support::feed(&mut t, &lines);
    f(&mut t)
}

/// The primary's physical rows outside the window: the history and the rows
/// past it.
fn off_window(t: &Term<'_>) -> Vec<usize> {
    let at = t.row_gens(Rows::ZERO).window();
    let on: Vec<usize> = (0..at.rows.index())
        .map(|y| at.at(Row::from_index(y)).p.index())
        .collect();
    (0..at.ring.index()).filter(|p| !on.contains(p)).collect()
}

/// **`CSI 3 J` moves every row it erases**: the history and the rows past
/// it (`View::erase_outside`).
#[test]
fn erasing_the_history_moves_its_rows() {
    with_history(|t| {
        let s = t.seal_writes();
        support::feed(t, b"\x1b[3J");
        let w = written(t, Which::Primary, &s);
        let off = off_window(t);
        assert!(!off.is_empty());
        assert!(
            off.iter().all(|p| w.contains(p)),
            "an erased row reads clean"
        );
    });
}

/// **`CSI ? 3 J` moves the history's rows it rewrites**
/// (`View::selective_wipe_behind`).
#[test]
fn a_selective_erase_of_the_history_moves_its_rows() {
    with_history(|t| {
        let hist = t.scrollback().len();
        assert!(hist > 0);
        let s = t.seal_writes();
        support::feed(t, b"\x1b[?3J");
        let at = t.row_gens(Rows::ZERO).window();
        let w = written(t, Which::Primary, &s);
        for k in 1..=hist {
            let p = at.top.retreat(k, at.ring).index();
            assert!(w.contains(&p), "history row {k} back reads clean");
        }
    });
}

/// **A full reset moves the other screen's rows too**, from either screen
/// (`RIS` erases the parked surface whole).
#[test]
fn a_full_reset_moves_the_other_screen() {
    support::with_term(&ws(10, 20), |t| {
        support::feed(t, b"\x1b[?47halt\x1b[?47lprimary");
        let s = t.seal_writes();
        support::feed(t, b"\x1bc");
        assert!(all_written(t, Which::Alternate, &s), "from the primary");
    });
    support::with_term(&ws(10, 20), |t| {
        support::feed(t, b"primary\x1b[?47halt");
        let s = t.seal_writes();
        support::feed(t, b"\x1bc");
        assert!(
            all_written(t, Which::Primary, &s),
            "from the alternate screen"
        );
    });
}

/// The terminal with the indicator status line shown (DECSSDT 1).
fn with_indicator<R>(f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    support::with_term(&ws(24, 80), |t| {
        support::feed(t, b"\x1b[1$~");
        assert_eq!(t.status_type(), StatusType::Indicator);
        f(t)
    })
}

fn status_after(t: &Term<'_>, s: &Seal) -> bool {
    t.status_gen().expect("a status line is lent").after(s)
}

/// **The indicator line moves at every read's end**, which draws it again.
#[test]
fn the_indicator_line_moves_at_a_read_end() {
    with_indicator(|t| {
        let s = t.seal_writes();
        assert!(!status_after(t, &s), "the control: nothing yet");
        support::feed(t, b"a");
        assert!(status_after(t, &s));
    });
}

/// **Each host call that draws the indicator line again moves it, with no
/// read**: the printer, the lamps, the DEC locator, a key while ReGIS input
/// runs, and a resize.
#[test]
fn each_host_call_that_draws_the_indicator_moves_it() {
    with_indicator(|t| {
        let s = t.seal_writes();
        t.set_printer(PrinterPort::Ready);
        assert!(status_after(t, &s), "set_printer");
    });
    with_indicator(|t| {
        let s = t.seal_writes();
        t.set_indicator_lamps(Some(IndicatorLamps::DARK));
        assert!(status_after(t, &s), "set_indicator_lamps");
    });
    with_indicator(|t| {
        support::feed(t, b"\x1b[1'z");
        let s = t.seal_writes();
        let at = LocatorAt {
            col: Col::at_index(5),
            row: Row::from_index(3),
            px: None,
        };
        let _ = t.locator(LocatorInput {
            kind: LocatorKind::Moved,
            at: Some(at),
            held: LocatorButtons::NONE,
        });
        assert!(status_after(t, &s), "locator");
    });
    with_indicator(|t| {
        t.set_regis_hold(RegisHold::On);
        support::feed(t, b"\x1bPpP[9,4]R(P(I))\x1b\\");
        let s = t.seal_writes();
        t.key(KeyEvent::press(KeyCode::Right, Modifiers::NONE));
        assert!(status_after(t, &s), "a key while ReGIS input runs");
    });
    with_indicator(|t| {
        let s = t.seal_writes();
        assert_eq!(t.resize(&ws(20, 70)), Resized::Ok);
        assert!(status_after(t, &s), "resize");
    });
}

/// **A frame with no read and no host call that draws the line leaves it
/// where it was**, the control for every case above.
#[test]
fn a_frame_with_no_read_leaves_the_indicator_line() {
    with_indicator(|t| {
        support::feed(t, b"a");
        let s = t.seal_writes();
        assert!(!status_after(t, &s));
        assert!(!t.wrote_since(&s));
    });
}

/// **Pinning a prompt at the bottom moves the rows it moves**, outside a
/// read (`Term::set_prompt_at_bottom`): the prompt's row and the last.
#[test]
fn pinning_a_prompt_moves_its_rows() {
    support::with_term(&ws(10, 40), |t| {
        support::feed(t, b"\x1b]133;A\x07$ \x1b]133;B\x07");
        assert_eq!(t.cursor().0.index(), 0);
        let s = t.seal_writes();
        t.set_prompt_at_bottom(PromptAtBottom::On);
        assert_eq!(t.cursor().0.index(), 9, "pinned");
        let g = t.row_gens(Rows::ZERO);
        assert!(g.after(Row::ZERO, &s), "the row the prompt left");
        assert!(g.after(Row::from_index(9), &s), "the row it landed on");
    });
}

/// **A seal from another terminal reads every row as written** (D5): its
/// epoch is not this one's, whatever the two generations say. Terminal A
/// seals ten times, so its seal's generation lies above anything B holds.
#[test]
fn a_seal_from_another_term_reads_every_row_written() {
    support::with_term(&ws(10, 20), |a| {
        support::with_term(&ws(10, 20), |b| {
            let _ = b.seal_writes();
            let mut sa = a.seal_writes();
            for _ in 0..9 {
                sa = a.seal_writes();
            }
            let own = b.sealed();
            assert!(!b.wrote_since(&own), "the control: B wrote nothing");
            assert!(b.wrote_since(&sa));
            let g = b.row_gens(Rows::ZERO);
            assert!((0..10).all(|y| g.after(Row::from_index(y), &sa)));
            assert!(b.status_gen().expect("a status line").after(&sa));
        });
    });
}

/// **A read that writes only a mark has written**: `LF` on a row that
/// wrapped ends it, a store to the row's mark and to no cell, so the count
/// of cells written stays put and a write known by that count would be
/// missed. The surface's last write is the read's borrow's, at its end.
#[test]
fn a_read_that_writes_only_a_mark_has_written() {
    support::with_term(&ws(10, 5), |t| {
        support::feed(t, b"abcdefg");
        assert!(t.row_extent(Row::ZERO).wrapped(), "row 0 wrapped");
        let s = t.seal_writes();
        let cells = t.amplification().1;
        support::feed(t, b"\x1b[A\n");
        assert!(!t.row_extent(Row::ZERO).wrapped(), "the line feed ended it");
        assert_eq!(t.amplification().1, cells, "no cell written");
        assert!(t.row_gens(Rows::ZERO).after(Row::ZERO, &s));
        assert!(t.wrote_since(&s));
    });
}

/// **A switch of screens inside one read moves the row written on each**:
/// the read writes row 0 of the primary, switches (`?47`, which clears
/// nothing) and writes row 0 of the alternate. Each surface's rows carry
/// their own generations, which ride the swap with the surface.
#[test]
fn a_switch_of_screens_inside_one_read_moves_the_row_on_each() {
    support::with_term(&ws(10, 20), |t| {
        let top = t.row_gens(Rows::ZERO).window().top.index();
        let s = t.seal_writes();
        support::feed(t, b"a\x1b[?47hb");
        let g = t.row_gens(Rows::ZERO);
        assert_eq!(g.window().screen, Which::Alternate);
        assert!(g.after(Row::ZERO, &s), "the alternate's row 0");
        assert!(!g.after(Row::from_index(1), &s), "the control");
        assert!(
            written(t, Which::Primary, &s).contains(&top),
            "the primary's row 0"
        );
    });
}

/// **A screen switch and back, inside one read, moves the row written on
/// each**: `a` on the primary, `?1049h`, `b` on the alternate at the same
/// cell, `?1049l`, `c` on the primary. Each surface's rows carry their own
/// generations, which ride both swaps with the surface.
#[test]
fn a_screen_switch_inside_one_read_moves_both_rows() {
    support::with_term(&ws(10, 20), |t| {
        let top = t.row_gens(Rows::ZERO).window().top.index();
        let s = t.seal_writes();
        support::feed(t, b"a\x1b[?1049h\x1b[Hb\x1b[?1049lc");
        assert_eq!(t.row_gens(Rows::ZERO).window().screen, Which::Primary);
        assert!(
            written(t, Which::Alternate, &s).contains(&0),
            "the alternate's row 0"
        );
        assert!(
            written(t, Which::Primary, &s).contains(&top),
            "the primary's row 0"
        );
        let s = t.seal_writes();
        support::feed(t, b"\x1b[?1049h\x1b[Hd\x1b[?1049le");
        assert!(
            written(t, Which::Alternate, &s).contains(&0),
            "the alternate's row 0, a second time"
        );
        assert!(
            written(t, Which::Primary, &s).contains(&top),
            "the primary's row 0, a second time"
        );
    });
}

/// **The same for DECSASD's swap**: `a` on the main display's row 0, the
/// host-writable status line selected (`CSI 1 $ }`), `b` on its row 0,
/// back (`CSI 0 $ }`), `c`. Both physical rows 0 move.
#[test]
fn decsasd_inside_one_read_moves_both_rows() {
    support::with_term(&ws(10, 20), |t| {
        support::feed(t, b"\x1b[2$~");
        let top = t.row_gens(Rows::ZERO).window().top.index();
        let s = t.seal_writes();
        assert!(
            !t.status_gen().expect("a status line is lent").after(&s),
            "the control: the status line, nothing yet"
        );
        support::feed(t, b"a\x1b[1$}b\x1b[0$}c");
        assert!(
            t.status_gen().expect("a status line is lent").after(&s),
            "the status line's row 0"
        );
        assert!(
            written(t, Which::Primary, &s).contains(&top),
            "the main display's row 0"
        );
    });
}

/// **Marking the scroll area's top moves its row** (`View::set_region_top`):
/// with the side margins left of the last column and rows outside the
/// scroll area, a write on those rows runs to the line's end, and the
/// area's top carries a mark of its own (`baud_domain::RegionTop`). DECSLRM
/// in a read of its own writes no cell on the area's top; its row moves for
/// the mark alone, and a row inside the area the margins left unmarked does
/// not.
#[test]
fn marking_the_scroll_areas_top_moves_its_row() {
    support::with_term(&ws(10, 20), |t| {
        support::feed(t, b"\x1b[?69h\x1b[3;8r");
        let top = Row::from_index(2);
        assert_eq!(t.row_extent(top).region_top(), RegionTop::Unmarked);
        let s = t.seal_writes();
        support::feed(t, b"\x1b[4;15s");
        assert_eq!(
            t.row_extent(top).region_top(),
            RegionTop::Marked,
            "the area's top marked"
        );
        let g = t.row_gens(Rows::ZERO);
        assert!(g.after(top, &s), "the area's top");
        assert!(!g.after(Row::from_index(5), &s), "the control");
    });
}
