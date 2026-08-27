use crate::*;
use core::marker::PhantomData;

/// Index of a *display* row, valid only for the [`Layout`] that produced it.
///
/// Row 500 at 80 columns is a different place in the text than row 500 at 200.
/// Anchoring scroll position by a bare row number is the resize bug users
/// actually notice, so the brand makes carrying one across a resize a compile
/// error.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowIdx<'layout> {
    v: RowCount,
    _b: Brand<'layout>,
}

/// Geometry currently in force. Obtainable only inside [`with_layout`], so
/// every [`RowIdx`] is tied to the geometry that computed it.
pub struct Layout<'layout> {
    cols: Cols,
    rows: Rows,
    _b: Brand<'layout>,
}

impl<'layout> Layout<'layout> {
    pub fn cols(&self) -> Cols {
        self.cols
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }

    /// The only constructor for a branded [`RowIdx`].
    pub fn row(&self, v: RowCount) -> RowIdx<'layout> {
        RowIdx { v, _b: self._b }
    }
    pub fn row_value(&self, r: RowIdx<'layout>) -> RowCount {
        r.v
    }

    /// Display rows a line occupies at this width.
    ///
    /// Derived, never stored authoritatively — a pure function of
    /// `(DisplayWidth, cols)`, so the row index can always be recomputed and
    /// asserted against rather than drifting from the text.
    pub fn rows_for(&self, w: DisplayWidth) -> RowCount {
        let c = self.cols.raw() as u32;
        RowCount(w.cells().max(1).div_ceil(c))
    }
}

/// Enter a geometry scope. A resize means calling this again, which mints a
/// fresh brand and invalidates every outstanding [`RowIdx`] at compile time.
///
/// ```compile_fail
/// # use baud_domain::*;
/// let stale = with_layout(Cols::from_wire(80), Rows::from_wire(24), |l| l.row(RowCount(5)));
/// with_layout(Cols::from_wire(200), Rows::from_wire(50), |l| l.row_value(stale)); // ERROR
/// ```
pub fn with_layout<R>(cols: Cols, rows: Rows, f: impl for<'id> FnOnce(Layout<'id>) -> R) -> R {
    f(Layout {
        cols,
        rows,
        _b: Brand(PhantomData),
    })
}

/// Scroll position expressed so it survives resize: a stable line plus an
/// offset *within* that line's wrapped rows.
#[derive(Clone, Copy)]
pub struct Anchor {
    pub line: LineIdx,
    /// Which wrapped row within that logical line. A [`RowCount`], not a
    /// [`RowIdx`]: it is an offset inside one line, not a grid position, and it
    /// stays valid across a resize while a `RowIdx` does not.
    pub row_within: RowCount,
}
