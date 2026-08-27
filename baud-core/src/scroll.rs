use crate::*;

impl<'a> Term<'a> {
    /// Scroll the scrolling region up by `n`, blanking what comes in at the
    /// bottom. The engine of LF-at-region-bottom, SU, and DL.
    ///
    /// The full-screen case advances the ring instead of moving cells: O(1)
    /// against O(rows x cols), and it is what makes streaming ingest fast. A
    /// region smaller than the screen cannot use it -- rows outside the region
    /// must not move -- so it pays for the copy.
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
        if top == 0 && bot == last && self.screen == Screen::Normal {
            for _ in 0..k {
                self.head = self.head.advance(SB);
                let y = Row::from_index(last);
                self.s.nrun[self.head.plus(y, SB).index()] = RunCount::ZERO;
                self.s.promoted[last] = false;
                self.view().fill_row(y, blank);
            }
            return;
        }
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
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
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
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
        self.view().insert_lines_in(at, n, rb, b);
        for y in cy..cy + k {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }
}
