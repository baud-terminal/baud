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
/// ```compile_fail,E0521
/// use baud_domain::{with_layout, Cols, DisplayWidth, Rows};
/// with_layout(Cols::of_index(80), Rows::of_count(24), |old| {
///     let stale = old.row(old.rows_for(DisplayWidth::ZERO));
///     with_layout(Cols::of_index(200), Rows::of_count(50), |new| new.row_value(stale))
/// });
/// ```
///
/// An index used under the layout that made it is the legal path:
///
/// ```
/// use baud_domain::{with_layout, Cols, DisplayWidth, Rows};
/// with_layout(Cols::of_index(80), Rows::of_count(24), |l| {
///     let r = l.row(l.rows_for(DisplayWidth::ZERO));
///     let _ = l.row_value(r);
/// });
/// ```
pub fn with_layout<R>(cols: Cols, rows: Rows, f: impl for<'id> FnOnce(Layout<'id>) -> R) -> R {
    f(Layout {
        cols,
        rows,
        _b: Brand(PhantomData),
    })
}

/// Which way `DECIC`/`DECDC` move the cells in a row.
///
/// **Not a `bool`, because the two are opposites and a call site spelling
/// `true` names neither.** `shift_columns(n, true)` inserted and
/// `shift_columns(n, false)` deleted -- two destructive grid operations
/// selected by a literal, where getting it backwards destroys text instead of
/// making room for it. The audit gates `baud-domain` at zero `bool` *fields*
/// for this reason; a `bool` parameter choosing between opposite effects is
/// the same defect one indirection away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColumnShift {
    /// `DECIC`: open a gap at the cursor, pushing cells toward the margin.
    Insert,
    /// `DECDC`: close a gap at the cursor, pulling cells back from it.
    Delete,
}
