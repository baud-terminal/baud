//! **Which way a line runs**: SCP and SPD (`[[source: ecma48]]` 8.3.111,
//! 8.3.130), and the rules that carry the direction they select from line
//! to line.
//!
//! SCP selects the character path "for the active line (the line that
//! contains the active presentation position) and subsequent lines", and
//! "This takes effect immediately": the paragraph holding the cursor's line
//! takes it at once, wherever in it the cursor stands (Jev, ACTIVE_LINE,
//! 0.94/0.92), and it becomes the direction in force, which the lines that
//! follow take by terminal-wg's draft rules
//! (`[[source: terminal-wg-bidi-escape-sequences]]`, Jev's lean
//! DRAFT_RULES): a line feed into a new paragraph, a line a scroll brings in
//! that a wrap did not, the lines ED erases whole, the lines SU, SD, IL and
//! DL bring in, each one of its own ([`Paint::lines_bounded`]), and the
//! screen a clear leaves. A wrap joins the row it
//! brings in to the paragraph above, which keeps its own direction, and an
//! erase inside a line keeps the line's. A cursor move takes nothing: "the
//! new mode is not applied" to a line the cursor only reaches.
//!
//! SPD sets the whole presentation component: every line of the screen,
//! the history aside, and the direction in force (Jev's lean,
//! WHOLE_SCREEN, 0.88/0.83).
//!
//! **terminal-wg's `?2500` and `?2501` are paragraph properties too**
//! (`[[source: terminal-wg-bidi-basic-modes]]`), carried by the same rules:
//! each becomes the value in force, and applies at once only to a
//! paragraph whose first position the cursor stands on, "if the cursor
//! stands at the first logical position of a paragraph then the received
//! BiDi property value is immediately applied on that paragraph ... If the
//! cursor is not at the first logical position ... the new mode is not
//! applied" (`[[source: terminal-wg-bidi-escape-sequences]]`; Jev,
//! FIRST_POSITION, 0.90/0.94). SPD spreads only the path: "It would be ugly
//! for SPD to spread the current BDSM, autodetection etc. modes too across
//! all the lines".
//!
//! **What a line holds is its paragraph's properties**, every row of it,
//! so the renderer reads them from any row of the paragraph and a reflow,
//! a scroll into the history and back, IL and DL carry them with the row
//! ([`baud_domain::RowExtent`], [`baud_domain::LineBidi`]).
//!
//! **What this costs a program that never sends either: one test of the
//! mode word where a line feed, a `CR LF` and a wrap already tested LNM and
//! autoprint**, [`ModeBit::PathSelected`] the third bit of it; cells
//! written were equal and the timing gave no result (docs/record.md, "SCP
//! and SPD set which way a line runs"). Until a path or a property is
//! selected every line holds the power-on byte and so does the value in
//! force, so none of the rules has anything to write. The lines SU, SD, IL and DL end are
//! ended whatever the path: a line they bring in is a paragraph of its own
//! either way, which a reflow reads too.

use baud_domain::{
    BandMove, CharacterPath, Col, Count, LineBidi, LineProperty, PathCursor, Row, Rows,
};

use crate::paint::Paint;
use crate::term::Screen;
use baud_domain::ModeBit;

impl Paint<'_, '_> {
    /// SCP: the direction in force, and at once the paragraph holding the
    /// cursor's line; `Ps2` 1 moves the cursor to the line's first
    /// character position, as `CHA 1` addresses it.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_path(&mut self, path: CharacterPath, cursor: PathCursor) {
        self.cold.line = self.cold.line.with_path(path);
        self.cold.modes.set(ModeBit::PathSelected);
        self.paragraph_takes(self.cy, Some(LineProperty::Path));
        if cursor == PathCursor::ToStart {
            self.set_col(self.on_line(self.place(self.confine_col(Col::ZERO))));
        }
    }

    /// SPD: the direction in force, and at once every line of the screen;
    /// `Ps2` 1 moves the cursor to the first character position of the
    /// first line, as `CUP 1 ; 1` addresses it.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_presentation(&mut self, path: CharacterPath, cursor: PathCursor) {
        self.cold.line = self.cold.line.with_path(path);
        self.cold.modes.set(ModeBit::PathSelected);
        for y in 0..self.rows.index() {
            let r = Row::from_index(y);
            self.view.set_bidi(r, self.view.bidi(r).with_path(path));
        }
        if cursor == PathCursor::ToStart {
            let top = self.confine(self.origin_row());
            self.goto_at(top, self.address_col(Col::ZERO));
        }
    }

    /// **`?2500` or `?2501`** (`which`), whose value in force the caller
    /// has set: the paragraph whose first position the cursor stands on
    /// takes it -- column 1 of a row no row above continues -- and any
    /// other paragraph keeps its own.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_line_property(&mut self, which: LineProperty) {
        if self.cold.line != LineBidi::POWER_ON {
            self.cold.modes.set(ModeBit::PathSelected);
        }
        if self.cx.col() != Col::ZERO || self.continues_from_above(self.cy) {
            return;
        }
        self.paragraph_takes(self.cy, Some(which));
    }

    /// Whether row `r` continues the line above it: a soft wrap from the
    /// row above, or on the primary's first row from the history's newest
    /// line.
    fn continues_from_above(&self, r: Row) -> bool {
        match r.index() {
            0 => {
                self.cold.screen == Screen::Normal
                    && !self.on_status()
                    && self.book.hist.index() > 0
                    && self.view.wrapped_behind(Rows::of_count(0)) == baud_domain::Wrap::Continues
            }
            y => self.view.extent(Row::from_index(y - 1)).wrapped(),
        }
    }

    /// Row `r`'s paragraph takes the value in force, `which` property of
    /// it or all of it: the rows above it it continues, into the history
    /// on the primary, and the rows below that continue it.
    fn paragraph_takes(&mut self, r: Row, which: Option<LineProperty>) {
        let mut top = r.index();
        while top > 0 && self.view.extent(Row::from_index(top - 1)).wrapped() {
            top -= 1;
        }
        let from = self.cold.line;
        // A paragraph that begins in the history: its lines there too, so
        // that every row of it says the same.
        if top == 0 && self.cold.screen == Screen::Normal && !self.on_status() {
            let mut k = 0;
            while k < self.book.hist.index()
                && self.view.wrapped_behind(Rows::of_count(k)) == baud_domain::Wrap::Continues
            {
                let back = Rows::of_count(k);
                let now = self.view.bidi_behind(back);
                self.view.set_bidi_behind(back, taken(now, from, which));
                k += 1;
            }
        }
        self.line_from(Row::from_index(top), from, which);
    }

    /// Row `r` and the rows below it that continue its line take `from`,
    /// `which` property of it or all of it.
    fn line_from(&mut self, r: Row, from: LineBidi, which: Option<LineProperty>) {
        let last = self.rows.index() - 1;
        let mut y = r.index();
        loop {
            let row = Row::from_index(y);
            self.view
                .set_bidi(row, taken(self.view.bidi(row), from, which));
            if y >= last || !self.view.extent(row).wrapped() {
                break;
            }
            y += 1;
        }
    }

    /// **A line feed that took the cursor to a new line**: the paragraph it
    /// starts takes the direction in force, the draft's "the emulator's
    /// current state is applied to that paragraph at the cursor's new
    /// location". The line it left is ended, so the cursor's line begins
    /// one.
    #[cold]
    #[inline(never)]
    pub(crate) fn line_fed_into_path(&mut self) {
        let last = self.rows.index() - 1;
        let moves = self.cy == self.cold.bot || self.cy.index() < last;
        self.index();
        if moves {
            self.line_from(self.cy, self.cold.line, None);
        }
    }

    /// **A wrap took the cursor to the next row**: the row joins the
    /// paragraph of the row it left, whose direction it takes -- "the first
    /// old paragraph's BiDi properties live on for the joined paragraph".
    #[cold]
    #[inline(never)]
    pub(crate) fn wrapped_into_path(&mut self) {
        if self.cy.index() > 0 {
            let above = self.view.bidi(Row::from_index(self.cy.index() - 1));
            self.line_from(self.cy, above, None);
        }
    }

    /// `IND` and `NEL`, and every other move down a line that is not a
    /// line feed: a line a scroll brings in takes the direction in force;
    /// a move to a line already there takes nothing.
    pub(crate) fn index_bringing_in(&mut self) {
        let scrolls = self.cy == self.cold.bot;
        self.index();
        if scrolls && self.cold.modes.is_set(ModeBit::PathSelected) {
            self.path_rows(self.cy, self.cy);
        }
    }

    /// **SU and SD end the lines either side of what they move**, as
    /// [`Paint::lines_bounded`] says, over the scrolling region.
    pub(crate) fn scrolled_lines_bounded(&mut self, n: Count, mv: BandMove) {
        let (top, bot) = (self.cold.top, self.cold.bot);
        if top.index() > bot.index() || self.fenced() {
            return;
        }
        let k = n.at_least_one().min(bot.index() + 1 - top.index());
        self.lines_bounded(top, bot, Rows::of_count(k), mv);
    }

    /// **SU, SD, IL and DL end the lines either side of the `k` lines they
    /// insert or remove** in rows `top..=bot`, terminal-wg's rule for SD
    /// and SU ("wherever lines are removed or inserted, we ensure that the
    /// boundaries are all hard newlines"), which baud applies to IL and DL
    /// as well: the row above the band, the row the moved lines end on,
    /// and, for a scroll down or IL at the primary's top, the history's
    /// newest line. Each new line is then a paragraph of its own, which
    /// takes the direction in force and not the paragraph's above, and a
    /// moved row that still runs on into the next keeps its wrap. A line
    /// feed's scroll does not come here: a wrap's scroll joins its new row
    /// to the one above.
    #[cold]
    #[inline(never)]
    pub(crate) fn lines_bounded(&mut self, top: Row, bot: Row, k: Rows, mv: BandMove) {
        if top.index() > 0 {
            self.view.set_ended(Row::from_index(top.index() - 1));
        } else if mv == BandMove::Down
            && self.cold.screen == Screen::Normal
            && !self.on_status()
            && self.book.hist.index() > 0
        {
            self.view.set_ended_behind(Rows::of_count(0));
        }
        // The last row the moved lines reach: above the lines SU and DL
        // bring in at the band's foot, and the band's foot itself once SD
        // and IL push its lines out.
        let edge = match mv {
            BandMove::Up => (bot.index() + 1).checked_sub(k.index() + 1),
            BandMove::Down => Some(bot.index()),
        };
        if let Some(y) = edge.filter(|y| *y >= top.index()) {
            self.view.set_ended(Row::from_index(y));
        }
    }

    /// Rows `from..=to`, new lines -- brought in by a scroll, IL or DL, or
    /// erased whole -- given the direction in force, while one has been
    /// selected.
    pub(crate) fn path_rows(&mut self, from: Row, to: Row) {
        if !self.cold.modes.is_set(ModeBit::PathSelected) {
            return;
        }
        self.path_rows_now(from, to);
    }

    #[cold]
    #[inline(never)]
    fn path_rows_now(&mut self, from: Row, to: Row) {
        let last = self.rows.index().saturating_sub(1);
        for y in from.index()..=to.index().min(last) {
            self.view.set_bidi(Row::from_index(y), self.cold.line);
        }
    }
}

/// `now` with `from`'s `which` property, or all of `from`.
fn taken(now: LineBidi, from: LineBidi, which: Option<LineProperty>) -> LineBidi {
    match which {
        Some(w) => now.with_property(from, w),
        None => from,
    }
}
