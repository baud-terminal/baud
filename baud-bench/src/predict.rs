//! **The prediction `frame_gpu`'s predict arm holds**: one run of typed
//! characters the program has not echoed yet, at the same cells on every
//! frame and every round, drawn as the cell path draws predicted echoes --
//! each a masked cell and a record in the rendition of the cell to its
//! left, the drawn cursor after the last -- so its line prices the mask and
//! the records a held prediction adds to a frame, and a later run's line
//! prices the same frame.

use baud_core::{CursorStyle, Term};
use baud_domain::{Codepoint, Col, Cols, PackedCell, Row, Rows};
use baud_render::{Instance, Overlay, Renderer, Resolved};

/// What the run holds: a command typed at a prompt.
pub const TYPED: &str = "git status";

/// The column the run starts at, after a prompt's two cells.
pub fn start() -> Col {
    Col::at_index(2)
}

/// **The run**: [`TYPED`] on the screen's last row from [`start`], each
/// character a predicted echo at its logical cell, cut at the screen's last
/// column.
pub fn run(cols: Cols, rows: Rows) -> Vec<(Row, Col, Codepoint)> {
    let row = Row::from_index(rows.index().saturating_sub(1));
    TYPED
        .chars()
        .filter_map(|c| Codepoint::new(u32::from(c)))
        .enumerate()
        .map(|(i, cp)| (row, Col::at_index(start().index() + i), cp))
        .take_while(|&(_, col, _)| col.index() < cols.index())
        .collect()
}

/// **The cursor a frame holding `run` draws**: after the run's last cell,
/// in its own style; none where the frame draws none.
pub fn cursor_after(
    cursor: Option<(Row, Col, CursorStyle)>,
    run: &[(Row, Col, Codepoint)],
) -> Option<(Row, Col, CursorStyle)> {
    match (cursor, run.last()) {
        (Some((_, _, style)), Some(&(row, col, _))) => Some((row, col.after(), style)),
        (c, _) => c,
    }
}

/// **Draws `run` over the frame [`Overlay::ensure`] just walked**: each
/// echo's cell masked in `mask` and its records appended to `records`, in
/// the rendition of the logical cell to its left (its own in the first
/// column), with `cursor` the cursor the frame draws.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    exceptions: &mut Overlay,
    r: &mut Renderer<'_, '_>,
    t: &Term<'_>,
    run: &[(Row, Col, Codepoint)],
    styles: &[Resolved],
    cursor: Option<(Row, Col, CursorStyle)>,
    mask: &mut [u8],
    records: &mut Vec<Instance>,
) {
    let view = t.view(Rows::ZERO);
    for &(row, col, cp) in run {
        let left = col.index().saturating_sub(1);
        let style = view
            .row(row)
            .get(left)
            .map_or(PackedCell::EMPTY.style, |c| c.style);
        exceptions.predicted(
            r,
            (row, col),
            PackedCell {
                content: baud_domain::Content::codepoint(cp),
                style,
                flags: baud_domain::CellFlags::NONE,
            },
            styles,
            cursor,
            None,
            view.cols(),
            mask,
            records,
        );
    }
}
