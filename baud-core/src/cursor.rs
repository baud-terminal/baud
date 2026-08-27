use crate::*;

impl<'a> Term<'a> {
    /// IND: down one line, scrolling the region if already at its bottom.
    ///
    /// **The column is not touched.** This used to reset `cx`, which made `LF`
    /// and `ESC D` behave as `NEL`. `[[source: ctlseqs]]` defines IND as index
    /// only, and LNM (mode 20, reset by default) is what decides whether LF
    /// also returns to column 1 -- so the old behaviour was LNM permanently on.
    #[inline(always)]
    pub(crate) fn index(&mut self) {
        if self.cy == self.scroll_bot {
            self.scroll_region_up(Count::ONE);
        } else if let Some(r) = self.cy.next(self.rows) {
            self.cy = r;
        }
        // The destination line starts empty of spans.
        //
        // Load-bearing for chunk invariance, not tidiness: without it runs
        // accumulate on a line across index operations, so whether MAXRUN
        // overflows -- and therefore whether the line gets promoted -- depends
        // on how the input happened to be split into reads. Dropping this line
        // broke ingest_is_chunk_invariant.
        //
        // KNOWN GAP: this also means IND onto an *existing* unpromoted line
        // discards its spans, where the spec moves the cursor without touching
        // content. Recorded in docs/vt-coverage.md rather than fixed here,
        // because fixing it means bounding run growth some other way and that
        // needs its own measurement.
        let i = self.line();
        self.s.nrun[i.index()] = RunCount::ZERO;
    }

    /// CR + IND: what autowrap does at the right margin, and what NEL is.
    #[inline(always)]
    /// NEL, and the wrap at the right margin.
    ///
    /// Returns to the **left margin**, not to column 0. With DECSLRM set, text
    /// wrapping to column 0 would leave the scrolling region entirely, which is
    /// the whole thing side margins exist to prevent.
    pub(crate) fn wrap(&mut self) {
        self.cx = self.left_margin;
        self.index();
    }

    /// RI. Up one line, scrolling the region down if already at its top.
    ///
    /// Was keyed on row 0 rather than the region top, so `ESC M` inside a
    /// scrolling region scrolled the whole screen.
    #[inline(always)]
    pub(crate) fn reverse_index(&mut self) {
        if self.cy == self.scroll_top {
            self.scroll_region_down(Count::ONE);
        } else if self.cy.index() > 0 {
            self.cy = self.cy.up(Count::ONE);
        }
    }

    /// LF. Adds the carriage return only when LNM is set.
    #[inline(always)]
    pub(crate) fn line_feed(&mut self) {
        self.index();
        if self.modes.is_set(ModeBit::LineFeedNewLine) {
            self.cx = Col::ZERO;
        }
    }

    /// Origin for row addressing, and the row a CUP parameter is relative to.
    ///
    /// `[[source: vt510-decom]]`: "When DECOM is set, the home cursor position
    /// is at the upper-left corner of the screen, within the margins. The
    /// starting point for line numbers depends on the current top margin
    /// setting. The cursor cannot move outside of the margins."
    pub(crate) fn origin_row(&self) -> Row {
        if self.modes.is_set(ModeBit::Origin) {
            self.scroll_top
        } else {
            Row::ZERO
        }
    }

    /// Clamp a row to the margins when DECOM confines the cursor to them.
    pub(crate) fn confine(&self, r: Row) -> Row {
        if !self.modes.is_set(ModeBit::Origin) {
            return r;
        }
        Row::from_index(
            r.index()
                .clamp(self.scroll_top.index(), self.scroll_bot.index()),
        )
    }
}
