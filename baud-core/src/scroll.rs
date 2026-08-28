use crate::*;

impl<'a> Term<'a> {
    /// Scroll the scrolling region up by `n`, blanking what comes in at the
    /// bottom. The engine of LF-at-region-bottom, SU, and DL.
    ///
    /// The full-screen case advances the ring instead of moving cells: O(1)
    /// against O(rows x cols), and it is what makes streaming ingest fast. A
    /// region smaller than the screen cannot use it -- rows outside the region
    /// must not move -- so it pays for the copy.
    /// Count what a line-block move costs, for the four sites that do one.
    ///
    /// **`insert_lines_in` and `delete_lines_in` live in `baud-domain`, where
    /// the counters do not reach.** All four call sites -- region scroll up and
    /// down, `IL` and `DL` -- moved and blanked cells that nothing counted, so
    /// amplification was understated for every application that scrolls a
    /// region or inserts a line. That is most TUIs: vim reported **zero**
    /// scrolls and 74% of its cells as `fill`, because it scrolls with `IL`
    /// and `DL` rather than by running off the bottom of a region.
    ///
    /// `k` rows are blanked and the rest of the block is moved, and a move is a
    /// write: `copy_within` per row writes every cell it touches.
    fn count_line_block(&mut self, top: Row, bot: Row, n: Count) {
        if top.index() > bot.index() {
            return;
        }
        let height = bot.index() + 1 - top.index();
        // `at_least_one().min(..)` rather than a cast, which is the idiom the
        // scrolling paths already use and what keeps this off the audit.
        let k = n.at_least_one().min(height);
        let cols = self.cols.index();
        let (moved, blanked) = ((height - k) * cols, k * cols);
        self.cells_written.add(moved + blanked);
        self.cells_fill.add(moved + blanked);
        self.cells_blanked.add(blanked);
        self.cells_scroll.add(blanked);
        self.scrolls.add(k);
    }

    pub(crate) fn scroll_region_up(&mut self, n: Count) {
        let (top, bot) = (self.scroll_top.index(), self.scroll_bot.index());
        if top > bot {
            return;
        }
        let k = n.at_least_one().min(bot + 1 - top);
        let blank = PackedCell::empty_with(self.style);
        let last = self.rows.index() - 1;
        // The O(1) ring advance *is* the scrollback: it retires the top line by
        // moving the window, which is only meaningful when there is somewhere
        // for the line to go. The alternate screen has no scrollback, so it
        // takes the shifting path -- taking the ring path there would scroll
        // the primary screen's history from under it.
        //
        // **And only in `Runs` mode, which is the correction.** `head` indexes
        // the *run* table; `vp` is indexed by screen row. Advancing the ring
        // moves which line each row draws from, and in `Runs` that is the whole
        // answer because the content is the runs. In `Computed` and
        // `Templated` the content is `vp`, nothing re-derives it, and the
        // advance moved no cells: the screen kept its old text and only the
        // bottom row went blank. That is the most common scroll there is --
        // every line of output past the last row -- and no test looked at what
        // it left on the screen, because every other test in `scrolling.rs`
        // sets a region and takes the copying path.
        // **The alternate screen gets the rotation without the ring.** The two
        // halves of the primary's fast path are separable: `head` is the
        // scrollback and `rot` is the addressing, and the alternate screen
        // wants only the second. Excluding it from the whole branch was
        // collateral -- the ring advance is meaningless there, and the rotation
        // went with it.
        //
        // It is *simpler* than the primary's, not a copy. `blit` treats the
        // alternate screen as always promoted, so text there never reaches the
        // run table: there is no `nrun` to zero and no `promoted` to re-derive,
        // only cells and the index that addresses them.
        //
        // What it cost to leave out: `less-pager.raw` is 638 newlines in the
        // alternate screen and measured **213x** cell amplification against
        // rule #1's <= 1, because each of those copied 49 rows of 200 cells.
        // `cat-scroll.raw` does the same thing on the primary at 6.1x.
        if top == 0 && bot == last && self.screen == Screen::Alternate {
            for _ in 0..k {
                self.alt_rot = self.alt_rot.advance(1, self.rows);
                // Counted exactly as the primary's path counts it. A fast path
                // that stops feeding the counters reports an improvement it did
                // not make -- "instrumentation outlived what fed it" is already
                // in this repo's history, and it hid the style-hash bug for as
                // long as it was printing garbage.
                self.fill_row_counted(Row::from_index(last), blank);
                self.cells_scroll.add(self.cols.index());
                self.scrolls.inc();
            }
            return;
        }
        if top == 0 && bot == last && self.screen == Screen::Normal {
            for _ in 0..k {
                self.head = self.head.advance(SB);
                // Rotate first, then blank what is now the bottom row: the
                // rotation is what moves the cells, and it costs one addition.
                self.rot = self.rot.advance(1, self.rows);
                let y = Row::from_index(last);
                self.s.nrun[self.head.plus(y, SB).index()] = RunCount::ZERO;
                self.fill_row_counted(y, blank);
                self.cells_scroll.add(self.cols.index());
                self.scrolls.inc();
            }
            // `promoted` is indexed by screen row and the screen just moved, so
            // every flag now describes the wrong line. Clearing re-derives from
            // the runs, which `head` has already pointed at the right ones.
            self.s.promoted[..self.rows.index()].fill(false);
            return;
        }
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
        self.count_line_block(rt, rb, n);
        self.view().delete_lines_in(rt, n, rb, blank);
        for y in top..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    /// Scroll the scrolling region down by `n`. The engine of RI-at-region-top,
    /// SD, and IL.
    pub(crate) fn scroll_region_down(&mut self, n: Count) {
        let (top, bot) = (self.scroll_top.index(), self.scroll_bot.index());
        if top > bot {
            return;
        }
        let blank = PackedCell::empty_with(self.style);
        // The mirror of `scroll_region_up`'s alternate-screen branch, and it
        // exists for the same reason: `less-pager.raw` holds 147 `ESC M`, and
        // with the upward path rotating they were the *whole* of the 53.8x
        // amplification left over -- 147 x 49 rows x 200 cells per pass.
        //
        // Alternate screen only, again. Retreating the primary's `rot` without
        // retreating `head` desynchronises the run table from the addressing,
        // which is exactly the bug that made full-screen scrolling not scroll;
        // and retreating `head` would resurrect a scrollback line where the
        // protocol says a *blank* enters. The alternate screen has no ring, so
        // neither question arises.
        //
        // Retreating by `k` maps screen row 0 onto the physical row that was
        // the bottom one, which is the row the protocol discards -- so blanking
        // row 0 after the retreat blanks exactly the recycled row.
        let last = self.rows.index() - 1;
        if top == 0 && bot == last && self.screen == Screen::Alternate {
            let k = n.at_least_one().min(bot + 1 - top);
            for _ in 0..k {
                self.alt_rot = self.alt_rot.retreat(1, self.rows);
                self.fill_row_counted(Row::ZERO, blank);
                self.cells_scroll.add(self.cols.index());
                self.scrolls.inc();
            }
            return;
        }
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
        self.count_line_block(rt, rb, n);
        self.view().insert_lines_in(rt, n, rb, blank);
        for y in top..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    /// DECSTBM. Sets the region and homes the cursor, per `[[source: ctlseqs]]`.
    ///
    /// A region of fewer than two lines is rejected and leaves the previous one
    /// in place: a one-line region cannot scroll, and applications send
    /// `CSI r` with no parameters to mean "the whole screen".
    pub(crate) fn set_scroll_region(&mut self, top: OneBased, bottom: Option<OneBased>) {
        let last = Row::from_index(self.rows.index() - 1);
        let t = top.to_row(self.rows);
        let b = bottom.map_or(last, |v| v.to_row(self.rows));
        if t.index() >= b.index() {
            return;
        }
        self.scroll_top = t;
        self.scroll_bot = b;
        self.cx = Col::ZERO;
        self.cy = t;
    }

    pub(crate) fn delete_lines(&mut self, n: Count) {
        if self.cy.index() < self.scroll_top.index() || self.cy.index() > self.scroll_bot.index() {
            return;
        }
        let blank = PackedCell::empty_with(self.style);
        let (cy, bot) = (self.cy.index(), self.scroll_bot.index());
        let (at, rb) = (self.cy, self.scroll_bot);
        self.count_line_block(at, rb, n);
        self.view().delete_lines_in(at, n, rb, blank);
        for y in cy..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    /// IL. Bounded by the scrolling region, and a no-op with the cursor
    /// outside it -- `[[source: ctlseqs]]` defines IL as operating within the
    /// region. It used to shift to the bottom of the *screen*, which moved rows
    /// that DECSTBM had placed outside the region.
    pub(crate) fn insert_lines(&mut self, n: Count) {
        if self.cy.index() < self.scroll_top.index() || self.cy.index() > self.scroll_bot.index() {
            return;
        }
        let (cy, bot) = (self.cy.index(), self.scroll_bot.index());
        let k = n.at_least_one().min(bot + 1 - cy);
        let (at, rb, b) = (self.cy, self.scroll_bot, PackedCell::empty_with(self.style));
        self.count_line_block(at, rb, n);
        self.view().insert_lines_in(at, n, rb, b);
        for y in cy..cy + k {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }
}
