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

/// A text run at or below this length goes a character at a time.
const SHORT_TEXT: usize = 2;

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
    /// How far the *current* row is known to be written, kept in a register.
    ///
    /// **This is what makes the orphan probe skippable.** `put_glyph` reads
    /// the cells either side of a write to repair a split wide character;
    /// past the row's used extent there is nothing to repair, because a blank
    /// is not a `WIDE_TAIL`. Asking `View` for the extent instead is a load
    /// from a second array -- a different cache line -- and that is exactly
    /// how the same idea measured *slower* in the previous core. Held here it
    /// is a compare against a value already in flight.
    ///
    /// Conservative in the safe direction: it starts at zero on every row
    /// change, so the first write to a row takes the probing path.
    row_written: Col,
    /// How many runs of each length the block walk found, and of each encoded
    /// width. **Do not read a bulk path as covering a character class -- count
    /// how often it is entered**, which is the mistake this repo records
    /// against a collector that took 0.0 characters an entry.
    #[cfg(feature = "diag")]
    pub(crate) runs: [usize; 8],
    #[cfg(feature = "diag")]
    pub(crate) run_len: [usize; 5],
    /// How many times the walk goes round per block. **If this is close to the
    /// characters in a block, the block bought nothing**: the loop is per
    /// character wearing a block's clothes.
    #[cfg(feature = "diag")]
    pub(crate) blocks: usize,
    #[cfg(feature = "diag")]
    pub(crate) block_runs: [usize; 10],
    /// What a block that missed every fast path begins with: text, control,
    /// a two/three/four-byte lead, or something else.
    #[cfg(feature = "diag")]
    pub(crate) miss_first: [usize; 6],
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
            row_written: Col::ZERO,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
            #[cfg(feature = "diag")]
            blocks: 0,
            #[cfg(feature = "diag")]
            block_runs: [0; 10],
            #[cfg(feature = "diag")]
            miss_first: [0; 6],
        }
    }

    /// A run of printable ASCII, blitted eight cells to a store.
    ///
    /// **A short run does not want the blitter.** `put_ascii_run` resolves the
    /// row, repairs whatever the run orphans and then runs a vectored loop
    /// eight cells at a time; for one or two characters that is all setup and
    /// no vector. `cat-cjk.raw` is 39% ASCII by character and interleaved, so
    /// its text runs are short -- the same shape that made a dedicated
    /// single-character path worth 12% on the multi-byte side.
    #[inline(always)]
    pub(crate) fn text(&mut self, from: BytePos, n: ByteLen) {
        if n.index() <= SHORT_TEXT {
            for i in 0..n.index() {
                let b = StreamByte::from_stream(self.arena[from.index() + i]);
                self.glyph(PackedCell::translate(b, self.charset), Width::NARROW);
            }
            return;
        }
        self.blit(from, n);
    }

    /// The vectored form, for a run long enough to fill a store.
    #[inline(always)]
    fn blit(&mut self, from: BytePos, n: ByteLen) {
        let k = n.index().min(self.cols.index() - self.cx.index());
        if k == 0 {
            return;
        }
        let src = &self.arena[from.index()..from.index() + k];
        self.view
            .put_ascii_run(self.cy, self.cx, src, self.style, self.flags, self.charset);
        self.cells = self.cells.plus(k);
        self.cx = Col::at_index(self.cx.index() + k);
        self.row_written = self.row_written.max(self.cx);
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
        let lead = PackedCell::glyph(cp, style, flags);
        match self.cx >= self.row_written {
            true => self.view.put_glyph_fresh(self.cy, self.cx, lead, tail),
            false => {
                self.view
                    .put_glyph(self.cy, self.cx, lead, tail, PackedCell::empty_with(style))
            }
        }
        self.cells = self.cells.plus(need);
        self.cx = Col::at_index(self.cx.index() + need);
        self.row_written = self.row_written.max(self.cx);
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
        self.row_written = self.row_written.max(self.cx);
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

    /// **Caching the row's resolved address was tried and lost.** `phys` is a
    /// conditional subtract and the stride is a multiply, both paid per glyph
    /// for an answer that changes only when the cursor changes rows -- so a
    /// `RowBase` held here should have moved them to once a *row*. Lazily,
    /// behind an `Option`, it measured flat: the test per glyph cost the
    /// saving. Eagerly, resolved whenever the row changes, it measured
    /// **608-618 against 621-632** -- because `cat-cjk.raw` has a newline
    /// every 78 bytes, so "once a row" is not rare, and a rotation invalidates
    /// every row's address at once. LLVM keeps the inline form cheap and there
    /// was nothing to hoist.
    ///
    /// A new row: nothing is known about it yet, so the probe comes back.
    #[inline(always)]
    fn row_changed(&mut self) {
        self.row_written = Col::ZERO;
    }

    /// Down one row, scrolling when that would leave the screen.
    ///
    /// The scroll is a rotation: the view advances its own and the only cells
    /// written are the incoming row's, which stop at its used extent.
    pub(crate) fn index(&mut self) -> RowRot {
        let last = self.rows.index() - 1;
        self.row_changed();
        if self.cy.index() < last {
            self.cy = Row::from_index(self.cy.index() + 1);
            return self.view.rot();
        }
        let rot = self.view.advance_rot(1);
        // **Ceiling arm: the blank behind a scroll.** The ring still advances
        // and the row is still counted, so both arms attribute the same cells
        // and only the stores differ -- which is what the previous core's
        // blanking arm got wrong, reporting a full row on one side and the
        // used extent on the other.
        #[cfg(not(feature = "ceiling-no-blank"))]
        let n = self
            .view
            .fill_row(Row::from_index(last), PackedCell::empty_with(self.style));
        #[cfg(feature = "ceiling-no-blank")]
        let n = self.view.extent(Row::from_index(last));
        self.cells = self.cells.plus(n.index());
        rot
    }
}
