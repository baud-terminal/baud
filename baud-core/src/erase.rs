use crate::*;

impl<'a> Term<'a> {
    /// A fresh screen is spaces, not NULs. Terminals compare equal on blank
    /// cells and a NUL renders as a hole.
    pub(crate) fn clear_screen(&mut self) {
        for y in 0..self.rows.index() {
            self.fill_row_counted(Row::from_index(y), PackedCell::EMPTY);
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
        if self.mode == OpMode::Runs
            && !self.row_is_promoted(Row::from_index(y))
            && e == EraseExtent::ToEnd
        {
            let i = self.line();
            self.s.nrun[i.index()] = RunCount::ZERO;
            return;
        }
        self.promote_if_runs();
        let s = e.line_span(self.cx, self.cols);
        let b = PackedCell::empty_with(self.style);
        self.fill_counted(Row::from_index(y), s.from, s.to, b);
    }

    /// ED. **The cursor's own row is erased only partially.**
    ///
    /// `[[source: ecma48]]` 8.3.39: for `Ps = 0`, "the active presentation
    /// position and the character positions up to the end of the page are put
    /// into the erased state"; for `Ps = 1`, "the character positions from the
    /// beginning of the page up to and including the active presentation
    /// position". The split is at the *cursor*, not at the row boundary.
    ///
    /// This erased whole rows, so `CSI 0 J` with the cursor mid-line destroyed
    /// the text to its left — text the application had every reason to believe
    /// was safe. It was found by a VT52 test (`ESC J` is the same operation)
    /// after reading the spec, not by any of the existing ED tests, which only
    /// ever erased from column 0.
    pub(crate) fn erase_display(&mut self, e: EraseExtent) {
        self.promote_if_runs();
        let b = PackedCell::empty_with(self.style);
        let full = e.full_rows(self.cy, self.rows);
        for y in full.from.index()..full.to.index() {
            let r = Row::from_index(y);
            self.discard_row(r);
            self.fill_row_counted(r, b);
        }
        if let Some(s) = e.cursor_span(self.cx, self.cols) {
            let r = self.cy;
            self.discard_row(r);
            self.fill_counted(r, s.from, s.to, b);
        }
    }

    pub(crate) fn erase_chars(&mut self, n: Count) {
        self.promote_if_runs();
        let b = PackedCell::empty_with(self.style);
        let (r, from) = (self.cy, self.cx);
        let to = Col::at_index(self.cx.index() + n.at_least_one());
        self.fill_counted(r, from, to, b);
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
        self.delete_cells_counted(r, at, n, to, b);
    }

    /// ICH. Bounded by the right margin, for the same reason as DCH: text
    /// beyond it is outside the scrolling region and must not be pushed.
    pub(crate) fn insert_blanks(&mut self, n: Count) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        let to = self.right_margin.after();
        self.insert_cells_counted(r, at, n, to, b);
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
        self.selective_wipe(self.cy, e.line_span(self.cx, self.cols));
    }

    /// DECSED, `CSI ? Ps J`. Same rule, over the display.
    ///
    /// Splits at the cursor for the same reason [`Term::erase_display`] does,
    /// and had the same bug: it erased the cursor's whole row, including the
    /// text to the left of it.
    pub(crate) fn selective_erase_display(&mut self, e: EraseExtent) {
        self.promote_if_runs();
        let full = e.full_rows(self.cy, self.rows);
        let whole = EraseExtent::All.line_span(self.cx, self.cols);
        for y in full.from.index()..full.to.index() {
            self.selective_wipe(Row::from_index(y), whole);
        }
        if let Some(s) = e.cursor_span(self.cx, self.cols) {
            self.selective_wipe(self.cy, s);
        }
    }

    /// Erase the unprotected cells of one row across a half-open column span.
    ///
    /// Typed rather than three `usize`s: a row index and two column indices are
    /// three different quantities, and passing them in the wrong order compiles
    /// perfectly. The audit refused the `usize` version, correctly.
    fn selective_wipe(&mut self, r: Row, s: ColSpan) {
        let blank = PackedCell::empty_with(self.style);
        self.discard_row(r);
        // Same widening as a plain erase, or the two disagree on a span that
        // starts or ends inside a double-width character -- which is what
        // `without_decsca_selective_equals_plain` reported as soon as `fill`
        // began snapping and this did not.
        let (from, to) = self.view().snap_span(r, s.from, s.to);
        for x in from.index()..to.index() {
            let c = Col::at_index(x);
            if !self.is_protected(r, c) {
                self.put_counted(r, c, blank);
            }
        }
    }

    /// Mark a row materialised and drop the spans behind it.
    ///
    /// Every bulk erase does these two together, and they must stay together:
    /// `promoted` without clearing `nrun` leaves a stale run list that a later
    /// promote would replay over the blanks it just wrote.
    /// **A no-op on the alternate screen, and that is the correctness half.**
    /// `head` indexes the *primary's* run ring and does not move during an
    /// alternate-screen session, and `promoted` is the primary's too, so every
    /// one of these writes lands on whatever the primary is holding
    /// underneath. Nothing is lost by skipping it: `blit` materialises every
    /// write on the alternate screen, so no text there ever enters the run
    /// table -- which is what [`Term::row_is_promoted`] says the other way
    /// round.
    ///
    /// Six call sites used to spell this pair inline -- the four line-moves,
    /// DECALN and DECIC/DECDC -- and each was an independent chance to get the
    /// screen test wrong. They go through here now, for the same reason
    /// `Term::active_rot` exists.
    pub(crate) fn discard_row(&mut self, r: Row) {
        if self.screen == Screen::Alternate {
            return;
        }
        self.s.promoted[r.index()] = true;
        self.s.nrun[self.head.plus(r, SB).index()] = RunCount::ZERO;
    }

    /// Is this screen row materialised in the cells?
    ///
    /// **Always true on the alternate screen**, which has no run table: `blit`
    /// materialises every write there. Reading `s.promoted[y]` directly
    /// instead is a live bug rather than a style point -- `erase_line`'s
    /// "truncate the span list" fast path did exactly that, so in
    /// `OpMode::Runs` a `CSI K` on the alternate screen took the span path,
    /// zeroed a *primary* run count, and returned without erasing anything:
    /// `HELLO WORLD` survived an erase-to-end from column 6.
    pub(crate) fn row_is_promoted(&self, r: Row) -> bool {
        self.screen == Screen::Alternate || self.s.promoted[r.index()]
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
            self.discard_row(r);
            self.fill_row_counted(r, cell);
        }
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
    }
}
