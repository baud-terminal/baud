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

/// The first eight cells of screen row `y`, as text.
fn text(t: &Term<'_>, y: usize) -> String {
    t.row(baud_domain::Row::from_index(y))[..8]
        .iter()
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect()
}

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
    support::with_room(60, |t| {
        support::feed(t, b"\x1b[?1049h");
        let (_, rows) = t.geometry();
        // A full-screen application's frame: every row addressed
        // absolutely, which is how btop and asciiquarium draw.
        for y in 0..rows.index() {
            support::feed(t, format!("\x1b[{};1Hrow {y:03}", y + 1).as_bytes());
        }
        let before = rows_of(t, 8);
        let (cols, _) = t.geometry();
        assert_eq!(
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok,
            "the growth is inside the provisioned grid"
        );
        let after = rows_of(t, 8);
        for (y, (b, a)) in before.iter().zip(&after).enumerate() {
            assert_eq!(
                b, a,
                "row {y} moved when the window grew: the screen scrolled under the application"
            );
        }
    });
}

/// **The primary screen is the other way round, and this test used to say
/// otherwise.** It was written on 2026-09-14 from the report quoted above,
/// as "a taller primary screen does not pull history back", and it could not
/// fail: `with_corpus` provisions exactly the fifty rows the screen has, so
/// the growth came back `Truncated` into a `let _`, nothing grew, and the
/// comparison held on a screen that had not moved. On 2026-09-18 the owner's
/// rule for the primary became "pull down to the bottom, and blank-fill only
/// when nothing is left to pull" (`Term::resize`, and note M), because
/// refusing to pull lost the lines a shrink had pushed into the history. The
/// alternate screen keeps no history and has nothing to pull, which is why
/// the report above still governs it.
///
/// So this pins the rule in force, on the path the other growth tests do
/// not take: the lines pulled back are ones that **scrolled** off, not ones
/// a shrink retired. The bottom row stays the bottom row, the rows above it
/// are the history in order, and the history is shorter by what came back.
#[test]
fn a_taller_primary_screen_pulls_the_scrolled_lines_back_above_it() {
    support::with_room(60, |t| {
        // Sixty numbered lines and the cursor's blank row below them: 61
        // rows on 50, so eleven have scrolled off.
        for i in 0..60 {
            support::feed(t, format!("line {i:03}\r\n").as_bytes());
        }
        assert_eq!(t.scrollback().len(), 11, "lines 000 to 010 scrolled off");
        assert_eq!(text(t, 0), "line 011");
        let (cols, rows) = t.geometry();
        assert_eq!(
            t.resize(&WinSize {
                ws_col: u16::try_from(cols.index()).unwrap(),
                ws_row: u16::try_from(rows.index() + 6).unwrap(),
            }),
            baud_core::Resized::Ok,
            "the growth is inside the provisioned grid"
        );
        for y in 0..6 {
            assert_eq!(
                text(t, y),
                format!("line {:03}", y + 5),
                "pulled back, in order"
            );
        }
        assert_eq!(
            text(t, 6),
            "line 011",
            "what was the first row is below them"
        );
        assert_eq!(text(t, 54), "line 059");
        assert_eq!(
            t.cursor().0.index(),
            55,
            "the cursor's row stays the bottom row"
        );
        assert_eq!(t.scrollback().len(), 5, "and the history is shorter by six");
    });
}
