//! Growing the window must not move what is already on the screen.
//!
//! The owner's report, twice: *"when resizing the current first row
//! changes and it should not -- we should not change the scroll ever, we
//! should only re-arrange the grid of what is visible, not go to
//! history."* A screen recording of btop mid-drag shows what it costs --
//! three copies of its lower panels at three different sizes, stacked
//! down the screen, because each redraw landed where the last one had
//! been moved to rather than over it.
mod support;
use baud_core::{PackedCell, Term};
use baud_domain::WinSize;

/// The first `rows` screen rows, raw, so a comparison is cell for cell.
fn rows_of(t: &Term<'_>, rows: usize) -> Vec<Vec<PackedCell>> {
    let cells = support::screen(t);
    let stride = t.stride().index();
    let cols = t.geometry().0.index();
    (0..rows)
        .map(|y| cells[y * stride..y * stride + cols].to_vec())
        .collect()
}

#[test]
fn a_taller_alternate_screen_does_not_move_what_is_on_it() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h");
        let (_, rows) = t.geometry();
        // A full-screen application's frame: every row addressed
        // absolutely, which is how btop and asciiquarium draw.
        for y in 0..rows.index() {
            support::feed(t, format!("\x1b[{};1Hrow {y:03}", y + 1).as_bytes());
        }
        let before = rows_of(t, 8);
        let (cols, _) = t.geometry();
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: u16::try_from(rows.index() + 6).unwrap(),
        });
        let after = rows_of(t, 8);
        for (y, (b, a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(
                b, a,
                "row {y} moved when the window grew: the screen scrolled under the application"
            );
        }
    });
}

#[test]
fn a_taller_primary_screen_does_not_pull_history_back() {
    support::with_corpus(|t| {
        // Sixty numbered lines, so most are behind the window.
        for i in 0..60 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        let before = rows_of(t, 8);
        // The probe has to be able to fire: there must be history to pull
        // and rows to grow into, or this passes for the wrong reason.
        assert!(
            t.scrollback().len() >= 6,
            "nothing retired, so a grow has nothing to pull back: {}",
            t.scrollback().len()
        );
        eprintln!("retained {} lines before the grow", t.scrollback().len());
        let (cols, rows) = t.geometry();
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: u16::try_from(rows.index() + 6).unwrap(),
        });
        let after = rows_of(t, 8);
        for (y, (b, a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(
                b, a,
                "row {y} moved when the window grew: history was pulled back over the screen"
            );
        }
    });
}
