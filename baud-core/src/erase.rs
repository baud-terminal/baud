use crate::*;

impl<'a> Term<'a> {
    /// A fresh screen is spaces, not NULs. Terminals compare equal on blank
    /// cells and a NUL renders as a hole.
    pub(crate) fn clear_screen(&mut self) {
        for y in 0..self.rows.index() {
            self.view().fill_row(Row::from_index(y), PackedCell::EMPTY);
        }
    }

    pub(crate) fn erase_line(&mut self, e: EraseExtent) {
        let y = self.cy.index();
        // Erase-to-end of an unpromoted line in Runs mode IS the truncation of
        // its span list: there is nothing after nrun to erase, and the viewport
        // is a projection of the spans. Promoting and filling afterwards
        // materialises a row only to blank it.
        //
        // On the TUI payload this is ~134 cell writes per 77 input bytes -- a
        // CSI K per line against a 200-column grid -- which is write
        // amplification of roughly 20:1 against rule #1's <= 1.
        if self.mode == OpMode::Runs && !self.s.promoted[y] && e == EraseExtent::ToEnd {
            let i = self.line();
            self.s.nrun[i.index()] = RunCount::ZERO;
            return;
        }
        self.promote_if_runs();
        let (from, to) = match e {
            EraseExtent::ToEnd => (self.cx.index(), self.cols.index()),
            EraseExtent::ToStart => (0, (self.cx.index() + 1).min(self.cols.index())),
            EraseExtent::All => (0, self.cols.index()),
        };
        let b = PackedCell::empty_with(self.style);
        let (rr, f, t) = (Row::from_index(y), Col::at_index(from), Col::at_index(to));
        self.view().fill(rr, f, t, b);
    }

    pub(crate) fn erase_display(&mut self, e: EraseExtent) {
        let (lo, hi) = match e {
            EraseExtent::ToEnd => (self.cy.index(), self.rows.index()),
            EraseExtent::ToStart => (0, self.cy.index() + 1),
            EraseExtent::All => (0, self.rows.index()),
        };
        let b = PackedCell::empty_with(self.style);
        for y in lo..hi {
            self.view().fill_row(Row::from_index(y), b);
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
            self.s.promoted[y] = true;
        }
    }

    pub(crate) fn erase_chars(&mut self, n: Count) {
        self.promote_if_runs();
        let b = PackedCell::empty_with(self.style);
        let (r, from) = (self.cy, self.cx);
        let to = Col::at_index(self.cx.index() + n.at_least_one());
        self.view().fill(r, from, to, b);
    }

    /// DCH. Bounded by the right margin when DECSLRM has set one.
    ///
    /// Without a margin this is the whole row, which is why the unbounded
    /// `delete_chars` is no longer called: `right_margin` is the last column by
    /// default, so one path covers both and there is no second version to keep
    /// in step.
    pub(crate) fn delete_chars(&mut self, n: Count) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        let to = self.right_margin.after();
        self.view().delete_cells(r, at, n, to, b);
    }

    /// ICH. Bounded by the right margin, for the same reason as DCH: text
    /// beyond it is outside the scrolling region and must not be pushed.
    pub(crate) fn insert_blanks(&mut self, n: Count) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        let to = self.right_margin.after();
        self.view().insert_cells(r, at, n, to, b);
    }

    /// Is the cell at `(r, c)` protected from a selective erase?
    ///
    /// Read from the cell, not from the style table. An earlier version looked
    /// the rendition up in `tmpl_key`, which is a direct-mapped cache with one
    /// slot per bucket: once 64 further styles had been interned, the slot held
    /// somebody else and the answer was whatever that style happened to say.
    /// Cell flags fit in what was `PackedCell::_pad`, so this costs nothing.
    pub(crate) fn is_protected(&mut self, r: Row, c: Col) -> bool {
        self.view().get(r, c).is_protected()
    }

    /// DECSEL, `CSI ? Ps K`.
    ///
    /// `[[source: vt510-decsel]]`: "erases only those characters defined as
    /// erasable by the DECSCA control function", and it "works inside or
    /// outside the scrolling margins".
    pub(crate) fn selective_erase_line(&mut self, e: EraseExtent) {
        self.promote_if_runs();
        let (from, to) = self.erase_span(e);
        let (y, blank) = (self.cy, PackedCell::empty_with(self.style));
        for x in from..to {
            let c = Col::at_index(x);
            if !self.is_protected(y, c) {
                self.view().put(y, c, blank);
            }
        }
    }

    /// DECSED, `CSI ? Ps J`. Same rule, over the display.
    pub(crate) fn selective_erase_display(&mut self, e: EraseExtent) {
        let (lo, hi) = match e {
            EraseExtent::ToEnd => (self.cy.index(), self.rows.index()),
            EraseExtent::ToStart => (0, self.cy.index() + 1),
            EraseExtent::All => (0, self.rows.index()),
        };
        let blank = PackedCell::empty_with(self.style);
        for y in lo..hi {
            let r = Row::from_index(y);
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(r, SB).index()] = RunCount::ZERO;
            for x in 0..self.cols.index() {
                let c = Col::at_index(x);
                if !self.is_protected(r, c) {
                    self.view().put(r, c, blank);
                }
            }
        }
    }

    /// The half-open column span an erase extent names, relative to the cursor.
    pub(crate) fn erase_span(&self, e: EraseExtent) -> (usize, usize) {
        match e {
            EraseExtent::ToEnd => (self.cx.index(), self.cols.index()),
            EraseExtent::ToStart => (0, (self.cx.index() + 1).min(self.cols.index())),
            EraseExtent::All => (0, self.cols.index()),
        }
    }

    /// DECALN, `ESC # 8`.
    ///
    /// `[[source: vt510-decaln]]`: "fills the complete screen area with a test
    /// pattern used for adjusting screen alignment... DECALN sets the margins
    /// to the extremes of the page, and moves the cursor to the home
    /// position." The pattern is uppercase E.
    pub(crate) fn screen_alignment(&mut self) {
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        // The pattern is characters, and characters written while DECSCA is
        // on are protected -- no special case here, so `ESC # 8` followed by a
        // DECSEL behaves like any other text would.
        let cell = PackedCell::from_ascii(StreamByte::from_stream(b'E'), self.style, self.flags);
        for y in 0..self.rows.index() {
            let r = Row::from_index(y);
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(r, SB).index()] = RunCount::ZERO;
            self.view().fill_row(r, cell);
        }
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
    }
}
