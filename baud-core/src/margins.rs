use crate::*;

impl<'a> Term<'a> {
    pub(crate) fn reset_side_margins(&mut self) {
        self.left_margin = Col::ZERO;
        self.right_margin = Col::at_index(self.cols.index() - 1);
    }

    /// DECSLRM, `CSI Pl ; Pr s`.
    ///
    /// `[[source: vt510-decslrm]]`: "The value of the left margin (Pl) must be
    /// less than the right margin (Pr) ... The minimum size of the scrolling
    /// region is two columns ... DECSLRM moves the cursor to column 1, line 1
    /// of the page."
    pub(crate) fn set_side_margins(&mut self, left: OneBased, right: OneBased) {
        let last = self.cols.index() - 1;
        let l = left.to_col(self.cols).index();
        let r = right.to_col(self.cols).index();
        // Reject rather than clamp into something the application did not ask
        // for: an invalid DECSLRM leaves the margins alone, which is what the
        // "must be less than" wording requires.
        if l >= r || r > last {
            return;
        }
        self.left_margin = Col::at_index(l);
        self.right_margin = Col::at_index(r);
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
    }

    /// DECIC, `CSI Pn ' }`.
    ///
    /// `[[source: vt510-decic]]`: "As columns are inserted, the columns between
    /// the cursor and the right margin move to the right ... DECIC has no
    /// effect outside the scrolling margins."
    pub(crate) fn insert_columns(&mut self, n: Count) {
        if !self.within_margins() {
            return;
        }
        self.shift_columns(n, true);
    }

    /// DECDC, `CSI Pn ' ~`. The inverse, filling at the right margin.
    pub(crate) fn delete_columns(&mut self, n: Count) {
        if !self.within_margins() {
            return;
        }
        self.shift_columns(n, false);
    }

    /// Is the cursor inside the scrolling region, in both axes?
    ///
    /// DECIC and DECDC are defined only there, and so is the column shifting
    /// that scrolling does when side margins are set.
    pub(crate) fn within_margins(&self) -> bool {
        let (x, y) = (self.cx.index(), self.cy.index());
        x >= self.left_margin.index()
            && x <= self.right_margin.index()
            && y >= self.scroll_top.index()
            && y <= self.scroll_bot.index()
    }

    /// Move every row's columns between the cursor and the right margin.
    ///
    /// Rows outside the *vertical* region are untouched: DECIC's "scrolling
    /// region" is the rectangle both margin pairs define, not the full height.
    pub(crate) fn shift_columns(&mut self, n: Count, insert: bool) {
        let blank = PackedCell::empty_with(self.style);
        let (from, to) = (self.cx, self.right_margin.after());
        let (top, bot) = (self.scroll_top.index(), self.scroll_bot.index());
        for y in top..=bot {
            let r = Row::from_index(y);
            self.discard_row(r);
            if insert {
                self.insert_cells_counted(r, from, n, to, blank);
            } else {
                self.delete_cells_counted(r, from, n, to, blank);
            }
        }
    }
}
