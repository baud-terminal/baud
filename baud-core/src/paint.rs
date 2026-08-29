//! One borrow of the grid, held for a whole read.
//!
//! **Why this type exists.** `ceiling-no-write` prices the write path at 49%
//! of this core, and most of that is not the stores -- it is building a
//! `View`. A `View` is six fields and a bounds re-derivation, and the block
//! walk was building one per *run*: on `cat-cjk.raw` a run is a handful of
//! bytes, so the setup was paid thousands of times where the stores it guards
//! are eight cells each.
//!
//! This repo already records the same failure one layer down -- "a view/borrow
//! helper was constructed inside a per-cell loop ... bulk operations belong on
//! the view, not per cell". This is that rule applied per *read* rather than
//! per run.
//!
//! The cursor lives here too, because it moves on every write and reading it
//! back through `Term` each time would put the borrow back.

use crate::storage::{MAX_COLS, Storage};
use baud_domain::*;

/// The grid, the bytes, and the cursor, borrowed together for one read.
pub(crate) struct Paint<'a> {
    pub(crate) arena: &'a [u8],
    pub(crate) view: View<'a>,
    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    pub(crate) style: StyleId,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,
    /// Cells written this read, the numerator of the amplification ratio.
    /// Two arms of an experiment that disagree here are not doing the same
    /// work, whatever their timings say.
    pub(crate) cells: Cells,
    /// How many runs of each length the block walk found, and of each encoded
    /// width. **Do not read a bulk path as covering a character class -- count
    /// how often it is entered**, which is the mistake this repo records
    /// against a collector that took 0.0 characters an entry.
    #[cfg(feature = "diag")]
    pub(crate) runs: [usize; 8],
    #[cfg(feature = "diag")]
    pub(crate) run_len: [usize; 5],
}

impl<'a> Paint<'a> {
    /// **One destructure, one view.** `arena` and `vp` are different fields of
    /// the same `Storage`, so splitting the single `&mut` is what lets a run be
    /// blitted straight out of the arena with no copy in between.
    pub(crate) fn new(
        store: &'a mut Storage<'_>,
        cy: Row,
        cx: Col,
        rows: Rows,
        cols: Cols,
        rot: RowRot,
        style: StyleId,
        flags: CellFlags,
        charset: Charset,
    ) -> Paint<'a> {
        let Storage { arena, vp, used } = store;
        Paint {
            arena,
            view: View::new(vp, used, Cols::from_stride(MAX_COLS), cols, rows, rot),
            cy,
            cx,
            rows,
            cols,
            style,
            flags,
            charset,
            cells: Cells::ZERO,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
        }
    }

    /// A run of printable ASCII, blitted eight cells to a store.
    #[inline(always)]
    pub(crate) fn text(&mut self, from: BytePos, n: ByteLen) {
        let k = n.index().min(self.cols.index() - self.cx.index());
        if k == 0 {
            return;
        }
        let src = &self.arena[from.index()..from.index() + k];
        self.view
            .put_ascii_run(self.cy, self.cx, src, self.style, self.flags, self.charset);
        self.cells = self.cells.plus(k);
        self.cx = Col::at_index(self.cx.index() + k);
    }

    /// One glyph under the cursor, wrapping first if it will not fit.
    #[inline(always)]
    pub(crate) fn glyph(&mut self, cp: Codepoint, w: Width) {
        let need = 1 + usize::from(w == Width::WIDE);
        if self.cx.index() + need > self.cols.index() {
            self.wrap();
        }
        let (style, flags) = (self.style, self.flags);
        let tail = match w == Width::WIDE {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        self.view.put_glyph(
            self.cy,
            self.cx,
            PackedCell::glyph(cp, style, flags),
            tail,
            PackedCell::empty_with(style),
        );
        self.cells = self.cells.plus(need);
        self.cx = Col::at_index(self.cx.index() + need);
    }

    /// A run of wide characters, two cells each, eight cells to a store.
    ///
    /// **Held across blocks was tried and lost.** A sixteen-byte block holds at
    /// most five three-byte characters, so a vectored store is amortised over
    /// five at best; accumulating the run into a 32-character buffer and
    /// flushing on a cursor-observing operation should have made the run's
    /// length a property of the text instead. It measured **465-480 against
    /// 508-521**, and `cells_written` rose 8.33M to 8.85M -- so it also
    /// dropped characters at the row edge, which the counters showed and the
    /// five short-string tests did not. Two lessons: the flush points are
    /// every path that reads the cursor, which is more of them than it looks,
    /// and the saving was smaller than the bookkeeping.
    #[inline(always)]
    pub(crate) fn wide(&mut self, cps: &[Codepoint]) {
        self.view
            .put_wide_run(self.cy, self.cx, cps, self.style, self.flags);
        self.cells = self.cells.plus(2 * cps.len());
        self.cx = Col::at_index(self.cx.index() + 2 * cps.len());
    }

    /// Does a run of `n` wide characters fit the rest of the row?
    ///
    /// A question rather than a number, so no count crosses the boundary and
    /// gets converted. Returning `Count` -- which is a `u16` VT parameter
    /// count, not a column count -- put a narrowing cast on each side and
    /// measured **983 against 1016 on `cat`**, which is a payload that never
    /// reaches this function at all: the cost was the conversions in the
    /// inlined body, not the work.
    #[inline(always)]
    pub(crate) fn fits_wide(&self, n: Count) -> bool {
        2 * n.index() <= self.cols.index() - self.cx.index()
    }

    pub(crate) fn wrap(&mut self) {
        self.cx = Col::ZERO;
        self.index();
    }

    /// Down one row, scrolling when that would leave the screen.
    ///
    /// The scroll is a rotation: the view advances its own and the only cells
    /// written are the incoming row's, which stop at its used extent.
    pub(crate) fn index(&mut self) -> RowRot {
        let last = self.rows.index() - 1;
        if self.cy.index() < last {
            self.cy = Row::from_index(self.cy.index() + 1);
            return self.view.rot();
        }
        let rot = self.view.advance_rot(1);
        let n = self
            .view
            .fill_row(Row::from_index(last), PackedCell::empty_with(self.style));
        self.cells = self.cells.plus(n.index());
        rot
    }
}
