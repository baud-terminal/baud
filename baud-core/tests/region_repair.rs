//! **A top region's scroll on the primary is a rotation and a repair**, and
//! `diag` counts the repair apart: the rows under the region each scroll
//! puts back (`Term::repair_rows`) and the cells the copies wrote
//! (`Term::repair_cells`). `ceiling-no-region-repair` skips the copies and
//! nothing else, and these pin both halves of that on one scroll: every
//! cell the scroll writes past the repair's is the same whatever lies under
//! the region, the retire, the rotation and the blank still run, and only
//! the rows under the region are left where the rotation put them.

#![cfg(feature = "diag")]

mod support;

use baud_core::*;
use baud_domain::*;

/// Ten rows, a region of the top three: seven rows under it.
const WS: WinSize = WinSize {
    ws_row: 10,
    ws_col: 20,
};
const UNDER: usize = 7;

/// What lies under the region before the scroll.
#[derive(Clone, Copy)]
enum Under {
    /// `D4` to `D9` on rows 4 to 9; row 3, the first under the region, blank.
    Dashboard,
    /// Nothing written under the region.
    Blank,
}

/// One region scroll, read back.
struct Scroll {
    /// `cells_written` by the scroll, and the repair's share of it.
    cells: Cells,
    repair: Cells,
    rows: Rows,
    /// The diag scroll counters' moves: rows rotated, rows copied.
    rotated: Rows,
    copied: Rows,
    /// Rows 0 to 9, two columns each.
    screen: Vec<String>,
    history: Vec<String>,
}

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

/// `a`, `b`, `c` in the region `1 ; 3`, then the `LF` at its bottom.
fn scroll(under: Under) -> Scroll {
    support::with_term(&WS, |t| {
        if let Under::Dashboard = under {
            for y in 4..10 {
                support::feed(t, format!("\x1b[{};1HD{y}", y + 1).as_bytes());
            }
        }
        support::feed(t, b"\x1b[1;3ra\r\nb\r\nc");
        assert!(t.scrollback().is_empty(), "nothing scrolled yet");
        let (_, c0) = t.amplification();
        let (r0, rows0) = (t.repair_cells, t.repair_rows);
        let (rot0, copy0) = (t.scroll_rot, t.scroll_copy);
        support::feed(t, b"\n");
        let (_, c1) = t.amplification();
        let sb = t.scrollback();
        Scroll {
            cells: Cells::of(c1 - c0),
            repair: Cells::of(t.repair_cells.index() - r0.index()),
            rows: Rows::of_count(t.repair_rows.index() - rows0.index()),
            rotated: Rows::of_count(t.scroll_rot - rot0),
            copied: Rows::of_count(t.scroll_copy - copy0),
            screen: (0..10)
                .map(|y| text_of(&t.row(Row::from_index(y)).expect("a row on the grid")[..2]))
                .collect(),
            history: (0..sb.len())
                .map(|n| text_of(sb.line(Back::of(n)).expect("a retained line").0))
                .collect(),
        }
    })
}

/// **Every cell the scroll writes past the repair's is the same whatever
/// lies under the region**, on both arms: the repair is the only work the
/// rows under the region decide, and the counter takes all of it. The
/// retire, the rotation and the blank of the line the scroll opens run
/// either way, and the scroll rotates rather than copies.
#[test]
fn a_region_scroll_writes_past_its_repair_the_same_whatever_lies_under_it() {
    let (d, b) = (scroll(Under::Dashboard), scroll(Under::Blank));
    for (name, s) in [("dashboard", &d), ("blank", &b)] {
        assert_eq!(
            s.rows,
            Rows::of_count(UNDER),
            "{name}: the rows under the region"
        );
        assert_eq!(s.rotated, Rows::of_count(1), "{name}: rotated");
        assert_eq!(s.copied, Rows::ZERO, "{name}: not copied");
        assert_eq!(s.history, ["a"], "{name}: the region's top line retired");
        assert_eq!(
            s.screen[..3],
            ["b ", "c ", "  "],
            "{name}: the region moved"
        );
    }
    assert_eq!(
        d.cells.index() - d.repair.index(),
        b.cells.index() - b.repair.index(),
        "the cells past the repair depend on nothing under the region"
    );
}

/// **The base: the copies put the rows under the region back**, and the
/// counter sees cells on the rows that hold something.
#[cfg(not(feature = "ceiling-no-region-repair"))]
#[test]
fn the_repair_puts_the_rows_under_a_top_region_back_and_counts_its_cells() {
    let (d, b) = (scroll(Under::Dashboard), scroll(Under::Blank));
    assert_eq!(
        d.screen[3..],
        ["  ", "D4", "D5", "D6", "D7", "D8", "D9"],
        "the rows under the region did not move"
    );
    assert!(
        d.repair > b.repair,
        "the dashboard's rows were copied: {:?} against {:?}",
        d.repair,
        b.repair
    );
}

/// **The arm: the copies skipped, and nothing else.** The rows under the
/// region scroll up with it, the counter of copied cells stays at zero, and
/// the rows it skipped are counted as the base counts the rows it repairs.
#[cfg(feature = "ceiling-no-region-repair")]
#[test]
fn the_ceiling_arm_skips_the_repair_and_counts_the_rows_it_skipped() {
    for under in [Under::Dashboard, Under::Blank] {
        let s = scroll(under);
        assert_eq!(s.repair, Cells::ZERO, "no copy wrote a cell");
        assert_eq!(s.rows, Rows::of_count(UNDER), "the rows skipped");
    }
    let d = scroll(Under::Dashboard);
    assert_eq!(
        d.screen[3..8],
        ["D4", "D5", "D6", "D7", "D8"],
        "the rows under the region moved up with the rotation"
    );
    assert_eq!(
        d.cells,
        scroll(Under::Blank).cells,
        "only the repair differed"
    );
}
