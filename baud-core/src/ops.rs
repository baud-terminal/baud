//! What a control sequence does to the grid.
//!
//! **Every one of these delegates to `View`.** `baud-domain` already owns
//! `insert_lines_in`, `delete_lines_in`, `delete_cells`, `insert_cells`,
//! `fill` and `fill_row`, and `baud-core2` reaches the same primitives; a
//! second spelling of a grid operation here is the "one write path knowing a
//! rule the other did not" failure this repo records four times. What lives in
//! this file is the *state* -- the region, the modes, the saved cursor -- and
//! the arithmetic that turns a parameter into a span.
//!
//! Out of the per-byte loop on purpose: the write path calls none of it, and
//! this repo has measured that a large `match` inlined into a per-byte body
//! costs every payload whether or not it executes.

use crate::paint::Paint;
use crate::term::{AltSaved, Saved, Screen};
use baud_domain::*;

/// Which way a full-screen scroll moved the window over the ring: up one,
/// retiring a row, or down one, taking one back.
#[derive(Clone, Copy)]
pub(crate) enum Scroll {
    Up,
    Down,
}

/// Which alternate-screen mode is switching buffers. `?1049` saves the
/// cursor and clears the alternate screen on the way in and restores the
/// cursor on the way out; `?47` switches and does nothing else.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AltSwitch {
    /// `?47`: the buffer, and nothing else.
    Bare,
    /// `?1049`: the buffer, with DECSC and a clear on entry and DECRC on exit.
    SaveAndClear,
}

/// Whether a synchronised update (`?2026`) is open, for where a `?5` change is
/// counted: a frame could show one made while it is released, and the
/// application asked for one made while it is held not to be seen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Update {
    Released,
    Held,
}

impl Paint<'_, '_> {
    /// Where row 1 is. DECOM makes the scrolling region's top the origin.
    pub(crate) fn origin_row(&self) -> Row {
        match self.cold.modes.is_set(ModeBit::Origin) {
            true => self.cold.top,
            false => Row::ZERO,
        }
    }

    /// **DECOM confines all cursor movement, not just absolute addressing.**
    /// `[[source: vt510-decom]]`, and the first implementation in this repo
    /// confined only CUP.
    pub(crate) fn confine(&self, r: Row) -> Row {
        match self.cold.modes.is_set(ModeBit::Origin) {
            false => r,
            true => Row::from_index(
                r.index()
                    .clamp(self.cold.top.index(), self.cold.bot.index()),
            ),
        }
    }

    pub(crate) fn save(&self) -> Saved {
        Saved {
            cy: self.cy,
            cx: self.cx,
            style_raw: self.style_raw,
            flags: self.flags,
            g: self.cold.g,
            gl: self.cold.gl,
            protect: self.cold.protect,
            autowrap: ModeChange::of(self.cold.modes.is_set(ModeBit::AutoWrap)),
            origin: ModeChange::of(self.cold.modes.is_set(ModeBit::Origin)),
        }
    }

    /// `?1049 l`'s restore: the cursor `DECSC` would restore, and the style
    /// and `?12` saved with it, which `DECRC` leaves alone.
    fn restore_alt(&mut self) {
        let Some(a) = self.cold.alt_saved.take() else {
            return;
        };
        self.cold.cursor_style = a.style;
        self.cold.cursor_blink = a.blink;
        self.restore(Some(a.cursor));
    }

    /// **Clamped, because the save may predate a resize.** An unclamped
    /// restore puts the cursor outside the grid, and writes and erases index
    /// with it.
    pub(crate) fn restore(&mut self, s: Option<Saved>) {
        let Some(s) = s else { return };
        let r = Row::from_index(s.cy.index().min(self.rows.index() - 1));
        self.goto(r, Col::at_index(s.cx.index().min(self.cols.index() - 1)));
        // **Interned again, not the saved id.** An eviction between the save
        // and the restore can give the saved id's slot to another rendition,
        // and writing under the old id would draw in its colours. The
        // rendition is what was saved; the id was where it happened to be.
        // A hit, in the ordinary case.
        self.style = self.intern_style(s.style_raw);
        self.style_raw = s.style_raw;
        self.flags = s.flags;
        self.cold.g = s.g;
        self.cold.gl = s.gl;
        self.cold.protect = s.protect;
        self.invoke();
        // **The two flags DECSC saves that neither core restored.**
        // `[[source: vt510-decsc]]` lists seven items and the wrap flag and
        // origin mode are two of them. Origin mode especially is not
        // cosmetic: it decides where home is, so an application that sets it,
        // saves, resets it and restores must get its coordinate system back.
        self.cold
            .modes
            .apply(ModeBit::AutoWrap, s.autowrap == ModeChange::Set);
        self.cold
            .modes
            .apply(ModeBit::Origin, s.origin == ModeChange::Set);
    }

    pub(crate) fn blank(&self) -> PackedCell {
        PackedCell::empty_with(self.style)
    }

    fn fill_counted(&mut self, r: Row, from: Col, to: Col) {
        let b = self.blank();
        let n = self.view.fill(r, from, to, b);
        self.cells = self.cells.plus(n);
    }

    fn fill_row_counted(&mut self, r: Row) {
        let b = self.blank();
        let n = self.view.fill_row(r, b);
        self.cells = self.cells.plus(n.index());
    }

    /// A row is about to scroll off the top of the primary screen: **nothing
    /// moves.** The window advances over it and it becomes the newest line
    /// behind the window, so the bookkeeping is one count up, bounded by the
    /// rows the ring has past the screen -- and one hidden row fewer, because
    /// the row the window takes in at the bottom is the first of the rows a
    /// shorter window cut. The alternate screen keeps no lines: an editor's
    /// redraws are not lines anyone scrolls back to. A region that does not
    /// start at the top is a window inside the screen, not the screen moving,
    /// and never reaches here -- which was false for one shape until
    /// `Paint::index` tested both ends: a region ending on the last row but
    /// starting below the top took the full-screen path and retired its
    /// header rows. A `debug_assert!` there holds it now.
    pub(crate) fn retire(&mut self) {
        if self.cold.screen != crate::term::Screen::Normal {
            if self.images.any_placed() {
                self.alt_images_moved(Scroll::Up);
            }
            return;
        }
        let room = self.view.ring().index() - self.rows.index();
        let hist = (self.book.hist.index() + 1).min(room);
        self.book.hist = Rows::of_count(hist);
        self.book.hidden =
            Rows::of_count(self.book.hidden.index().saturating_sub(1).min(room - hist));
        // Uncapped where `hist` is capped at `room`: the window's top line
        // moves on with the window, so a placement past a full ring gets a
        // distinct line, and `unretire` moves it back.
        self.book.top = self.book.top.plus(Rows::of_count(1));
        // **The one compare a stream with no image pays.** The work behind it
        // is outlined, the wide-row fill's shape: `retire` runs once a scroll,
        // on the path `cat` spends its time in.
        if self.images.any_placed() {
            self.primary_images_moved(Scroll::Up);
        }
    }

    /// **The primary's images after its window moved one row**: those whose
    /// row left what the ring retains go, and so does one on the row the
    /// scroll blanked as it came in -- the new bottom row going up, the new
    /// top row going down, which is the newest history line taken back.
    #[cold]
    #[inline(never)]
    pub(crate) fn primary_images_moved(&mut self, way: Scroll) {
        let top = self.book.top;
        let blanked = match way {
            Scroll::Up => top.plus(Rows::of_count(self.rows.index() - 1)),
            Scroll::Down => top,
        };
        self.expire_primary_images(Some(blanked));
    }

    /// Drop the primary's placements whose anchor row the ring no longer
    /// retains -- before the oldest history line, or past the rows a shorter
    /// window cut -- and one on `blanked`.
    pub(crate) fn expire_primary_images(&mut self, blanked: Option<LineIdx>) {
        let b = &self.book;
        let lo = b.top.minus(b.hist).unwrap_or(LineIdx::FIRST);
        let hi = b.top.plus(self.rows).plus(b.hidden);
        self.images.expire(ImageScreen::Primary, lo, hi, blanked);
    }

    /// **The alternate screen's images after its window moved one row.** It
    /// keeps no history, so a row that leaves the window leaves the screen,
    /// and its image with it; the row the scroll blanked goes too. Its top
    /// line is kept only while an image is placed, which is when this runs.
    #[cold]
    #[inline(never)]
    pub(crate) fn alt_images_moved(&mut self, way: Scroll) {
        let rows = self.rows;
        let blanked = match way {
            Scroll::Up => {
                self.book.alt_top = self.book.alt_top.plus(Rows::of_count(1));
                self.book.alt_top.plus(Rows::of_count(rows.index() - 1))
            }
            Scroll::Down => {
                match self.book.alt_top.minus(Rows::of_count(1)) {
                    Some(t) => self.book.alt_top = t,
                    None => self.images.shift(ImageScreen::Alternate, Rows::of_count(1)),
                }
                self.book.alt_top
            }
        };
        let top = self.book.alt_top;
        self.images
            .expire(ImageScreen::Alternate, top, top.plus(rows), Some(blanked));
    }

    /// **The live screen's images after the rows `[from, to]` moved `k`
    /// `way` inside the window**: a region scrolling by copy, `IL`, `DL`, or
    /// a top-anchored region's rotation putting the rows under it back. The
    /// window's top line does not move for any of them, so an anchor has to
    /// be moved with its row by name; one whose row left the band was
    /// deleted with it and goes. Outlined like the moves above: the callers
    /// gate it on `any_placed`, so a stream with no image pays one compare.
    #[cold]
    #[inline(never)]
    pub(crate) fn band_images_moved(&mut self, from: Row, to: Row, k: Rows, way: BandMove) {
        let (screen, top) = self.image_top();
        let lo = top.plus(Rows::of_count(from.index()));
        let hi = top.plus(Rows::of_count(to.index() + 1));
        self.images.scroll_band(screen, lo, hi, k, way);
    }

    /// **The row the scroll after the next will blank, asked for now.** With
    /// the grid as the ring, the row a scroll brings in was last touched
    /// `ring` scrolls ago, so the blank's stores land on lines nothing has in
    /// cache: the counters put the same thirteen misses on every scroll at
    /// any depth, served from L2 at fifty lines of history and from further
    /// out at two thousand, where they cost ten times the stall -- 0.56x on
    /// `flat`. One row ahead of the one being blanked is a scroll's worth of
    /// lead, ~50 ns on `flat`; the hint is thirteen instructions on the
    /// ISA arms and nothing on the portable build.
    #[cfg(feature = "probe-warm-scroll")]
    #[inline(always)]
    pub(crate) fn warm_incoming(&self) {
        let next = self.view.incoming(1);
        #[cfg(all(feature = "neon-scan", target_arch = "aarch64"))]
        <baud_isa_aarch64::Neon as baud_isa::Warm>::warm_stores(next);
        #[cfg(all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2"))]
        <baud_isa_x86_64::Avx2 as baud_isa::Warm>::warm_stores(next);
        #[cfg(not(any(
            all(feature = "neon-scan", target_arch = "aarch64"),
            all(feature = "avx2-scan", target_arch = "x86_64", target_feature = "avx2")
        )))]
        let _ = next;
    }

    /// The inverse, for a full-screen scroll *down* on the primary: the row
    /// the window takes in at the top is the newest line behind it, blanked
    /// as it arrives, and the row that leaves the bottom is kept past the
    /// live height the way a shorter window keeps one.
    pub(crate) fn unretire(&mut self) {
        if self.cold.screen != crate::term::Screen::Normal {
            if self.images.any_placed() {
                self.alt_images_moved(Scroll::Down);
            }
            return;
        }
        let room = self.view.ring().index() - self.rows.index();
        let hist = self.book.hist.index().saturating_sub(1);
        self.book.hist = Rows::of_count(hist);
        self.book.hidden = Rows::of_count((self.book.hidden.index() + 1).min(room - hist));
        // **The window's top line moves back with it.** With no history it
        // moves back over a row no line was numbered for, and at line zero the
        // numbering cannot follow -- so the images move down a row instead,
        // which is the same move seen from the other side: their text went
        // down one.
        match self.book.top.minus(Rows::of_count(1)) {
            Some(t) => self.book.top = t,
            None if self.images.any_placed() => {
                self.images.shift(ImageScreen::Primary, Rows::of_count(1));
            }
            None => {}
        }
        if self.images.any_placed() {
            self.primary_images_moved(Scroll::Down);
        }
    }

    // ---- scrolling ------------------------------------------------------

    /// **The full-screen case rotates, a region anchored at the top rotates
    /// and repairs the rows under it, and any other region copies.** A scroll
    /// that moves an index costs one row of blanking where one that moves the
    /// rows costs the whole region -- the split this repo measured at 45% --
    /// and on the primary a top region rotates whatever that costs, because
    /// the rotation is what sends its line to the history.
    pub(crate) fn scroll_region_up(&mut self, n: Count) {
        let (top, bot) = (self.cold.top, self.cold.bot);
        if top.index() > bot.index() {
            return;
        }
        let k = n.at_least_one().min(bot.index() + 1 - top.index());
        let last = self.rows.index() - 1;
        if top.index() == 0 && bot.index() == last {
            #[cfg(feature = "diag")]
            {
                self.scroll_rot += k;
            }
            for _ in 0..k {
                self.retire();
                self.view.advance_rot(1);
                #[cfg(feature = "probe-warm-scroll")]
                self.warm_incoming();
                self.fill_row_counted(Row::from_index(last));
            }
            return;
        }
        // **A region anchored at the top is a rotation plus a repair**, and
        // the repair is the rows *outside* it rather than the rows inside.
        //
        // The shape every status-line application has -- `less`, `vim`,
        // `htop`, `tmux` -- is `DECSTBM 1 ; rows-1`, so the region is the whole
        // screen but one line. Copying it moves forty-nine rows a scroll;
        // rotating and putting the one row below the region back moves
        // **one**. Found by concatenating the corpus into one stream: a program
        // that sets a region and does not reset it leaves the next one
        // scrolling by copy, and the mixed stream took **89,209 scrolls, 100%
        // of them copying**, at 201 MB/s against the 835 the recordings predict
        // individually.
        //
        // **On the primary a full-width one rotates whatever its height,
        // because the rotation is what keeps its line.** A short region used to
        // copy, which deleted the line a tall one sent to the history: one
        // operation, two fates, chosen by which path moved fewer rows. xterm
        // keeps the line whenever the top margin is the first row and never
        // tests the bottom (`[[source: xterm-scroll]]`, `xtermScroll`), kitty
        // when `margin_top == 0` (`[[source: kitty-screen-0-49]]`,
        // `screen_index` and `screen_scroll`), wezterm when the region starts
        // at row 0 (`[[source: wezterm-screen-b09b56c]]`, `scroll_up`). A
        // region narrower than the screen keeps nothing in xterm or wezterm,
        // so the widening asks for full side margins; a tall one under narrow
        // margins rotates as it always did, which is the rectangle gap the
        // `DECSLRM` row records. The alternate screen keeps no history, so
        // there the cheaper path still decides.
        //
        // **What it costs is the repair.** A short region leaves most rows
        // under it, and each scroll moves every one of them: `below` rows of
        // cells where the copy moved the region's. xterm, kitty and foot move
        // row *pointers* for the same scroll, so the lever for this shape is a
        // row indirection, which this repo measured once as not worth its
        // carrying cost on the corpus -- none of whose recordings has it.
        let below = last - bot.index();
        let height = bot.index() + 1 - top.index();
        let keeps = self.cold.screen == Screen::Normal
            && self.cold.left == Col::ZERO
            && self.cold.right.index() == self.cols.index() - 1;
        if top.index() == 0 && (keeps || below < height) {
            // **On either screen.** The count says the ring advanced through a
            // region rather than with the whole screen, which is what a
            // reader numbering rows by the ring's advance has to know: the
            // rows under the region did not move. It counted the primary
            // only, because the alternate screen keeps no history, and so on
            // the alternate screen, where vim's `1 ; 45` rotates, a
            // selection on the status line under it was renumbered as if it
            // had scrolled.
            self.cold.region_scrolls = self.cold.region_scrolls.next();
            #[cfg(feature = "diag")]
            {
                self.scroll_rot += k;
            }
            let mut moved = 0;
            for _ in 0..k {
                self.retire();
                self.view.advance_rot(1);
                #[cfg(feature = "probe-warm-scroll")]
                self.warm_incoming();
                // **Descending, so a row is read before it is overwritten.**
                for y in (bot.index() + 1..=last).rev() {
                    moved += self
                        .view
                        .copy_row(Row::from_index(y - 1), Row::from_index(y));
                }
                self.fill_row_counted(bot);
                // **The rows under the region went back, and their images
                // go back with them.** `retire` moved every image up with the
                // window, which is right for the region's and one row wrong
                // for these: without this an image on a status line walks up
                // into the region a row a scroll while its text stays put.
                if self.images.any_placed() {
                    self.band_images_moved(
                        bot,
                        Row::from_index(last),
                        Rows::of_count(1),
                        BandMove::Down,
                    );
                }
            }
            self.cells = self.cells.plus(moved);
            return;
        }
        #[cfg(feature = "diag")]
        {
            self.scroll_copy += k;
        }
        // **Ceiling arm: the region move, priced.** Incorrect by construction --
        // a region that never scrolls leaves stale rows on the screen -- but
        // the cell accounting below still runs, so both arms attribute the
        // same work and only the stores differ. Same shape as
        // `ceiling-no-blank`, and it exists because `utf8/cava.raw` is the
        // first recording in the corpus that reaches this path at all: 51
        // copied region scrolls a rep against zero everywhere else.
        #[cfg(not(feature = "ceiling-no-region-copy"))]
        {
            let b = self.blank();
            self.view.delete_lines_in(top, n, bot, b);
        }
        if self.images.any_placed() {
            self.band_images_moved(top, bot, Rows::of_count(k), BandMove::Up);
        }
        // **The whole region, not `k` rows.** `delete_lines_in` moves every
        // row inside the region and blanks `k`; counting only the blanks
        // undercounted the copying path by the height of the region, which is
        // exactly the traffic the rotation above avoids.
        self.cells = self
            .cells
            .plus((bot.index() + 1 - top.index()) * self.cols.index());
    }

    pub(crate) fn scroll_region_down(&mut self, n: Count) {
        let (top, bot) = (self.cold.top, self.cold.bot);
        if top.index() > bot.index() {
            return;
        }
        let k = n.at_least_one().min(bot.index() + 1 - top.index());
        let last = self.rows.index() - 1;
        if top.index() == 0 && bot.index() == last {
            for _ in 0..k {
                self.view.retreat_rot(1);
                self.unretire();
                self.fill_row_counted(Row::ZERO);
            }
            return;
        }
        #[cfg(feature = "diag")]
        {
            self.scroll_copy += k;
        }
        let b = self.blank();
        self.view.insert_lines_in(top, n, bot, b);
        if self.images.any_placed() {
            self.band_images_moved(top, bot, Rows::of_count(k), BandMove::Down);
        }
        self.cells = self
            .cells
            .plus((bot.index() + 1 - top.index()) * self.cols.index());
    }

    /// DECSTBM. **A region needs at least two rows**, and setting one homes
    /// the cursor.
    pub(crate) fn set_scroll_region(&mut self, top: OneBased, bottom: Option<OneBased>) {
        let last = Row::from_index(self.rows.index() - 1);
        let t = top.to_row(self.rows);
        let b = bottom.map_or(last, |v| v.to_row(self.rows));
        if t.index() >= b.index() {
            return;
        }
        self.cold.top = t;
        self.cold.bot = b;
        self.goto(t, Col::ZERO);
    }

    /// IL. **Outside the region it does nothing**, which is the rule that
    /// keeps a pager's `IL` from disturbing its status line.
    pub(crate) fn insert_lines(&mut self, n: Count) {
        if self.cy.index() < self.cold.top.index() || self.cy.index() > self.cold.bot.index() {
            return;
        }
        let (at, bot) = (self.cy, self.cold.bot);
        let k = n.at_least_one().min(bot.index() + 1 - at.index());
        let b = self.blank();
        self.view.insert_lines_in(at, n, bot, b);
        if self.images.any_placed() {
            self.band_images_moved(at, bot, Rows::of_count(k), BandMove::Down);
        }
        self.cells = self.cells.plus(k * self.cols.index());
        self.row_changed_to(self.cy);
    }

    /// DL.
    pub(crate) fn delete_lines(&mut self, n: Count) {
        if self.cy.index() < self.cold.top.index() || self.cy.index() > self.cold.bot.index() {
            return;
        }
        let (at, bot) = (self.cy, self.cold.bot);
        let k = n.at_least_one().min(bot.index() + 1 - at.index());
        let b = self.blank();
        self.view.delete_lines_in(at, n, bot, b);
        if self.images.any_placed() {
            self.band_images_moved(at, bot, Rows::of_count(k), BandMove::Up);
        }
        self.cells = self.cells.plus(k * self.cols.index());
        self.row_changed_to(self.cy);
    }

    // ---- erasing --------------------------------------------------------

    /// ECH: blank `n` cells from the cursor, without moving anything.
    pub(crate) fn erase_chars(&mut self, n: Count) {
        let (r, from) = (self.cy, self.cx);
        let to = Col::at_index((self.cx.index() + n.at_least_one()).min(self.cols.index()));
        self.fill_counted(r, from, to);
    }

    /// DCH: pull the rest of the row left over `n` cells.
    pub(crate) fn delete_chars(&mut self, n: Count) {
        let (r, at, b) = (self.cy, self.cx, self.blank());
        // **The right margin, not the row.** `to` was `cols` before the
        // margins existed, so a `DCH` inside a region pulled text in from
        // *outside* it -- correct only while the margin is the edge, which is
        // every stream that never sends `DECSLRM`.
        let to = self.cold.right.after();
        self.view.delete_cells(r, at, n, to, b);
        self.cells = self.cells.plus(to.index() - at.index());
        self.row_changed_to(r);
    }

    /// ICH: push the rest of the row right over `n` blanks.
    pub(crate) fn insert_blanks(&mut self, n: Count) {
        let (r, at, b) = (self.cy, self.cx, self.blank());
        let to = self.cold.right.after();
        self.view.insert_cells(r, at, n, to, b);
        self.cells = self.cells.plus(to.index() - at.index());
        self.row_changed_to(r);
    }

    /// DECSED / DECSEL: erase only what DECSCA left unprotected.
    pub(crate) fn selective_erase_line(&mut self, e: EraseExtent) {
        let (a, b) = self.line_span(e);
        self.selective_wipe(self.cy, a, b);
    }

    pub(crate) fn selective_erase_display(&mut self, e: EraseExtent) {
        let cols = Col::at_index(self.cols.index());
        let (from, to) = self.display_rows(e);
        for y in from.index()..to.index() {
            self.selective_wipe(Row::from_index(y), Col::ZERO, cols);
        }
        if e != EraseExtent::All {
            let (a, b) = self.line_span(e);
            self.selective_wipe(self.cy, a, b);
        }
    }

    fn selective_wipe(&mut self, r: Row, from: Col, to: Col) {
        let b = self.blank();
        for x in from.index()..to.index() {
            let c = Col::at_index(x);
            if !self.view.get(r, c).is_protected() {
                self.view.put(r, c, b);
                self.cells = self.cells.plus(1);
            }
        }
        self.row_changed_to(r);
    }

    /// The span `EraseExtent` names on the cursor's row.
    pub(crate) fn line_span(&self, e: EraseExtent) -> (Col, Col) {
        let cols = Col::at_index(self.cols.index());
        match e {
            EraseExtent::ToStart => (Col::ZERO, Col::at_index(self.cx.index() + 1)),
            EraseExtent::All => (Col::ZERO, cols),
            EraseExtent::ToEnd => (self.cx, cols),
        }
    }

    /// The whole rows `EraseExtent` names, excluding the cursor's own.
    ///
    /// A pair of `Row`s and not of integers: the two are a half-open span of
    /// *screen rows*, and the one place this repo let a row be a bare index it
    /// went through `View::phys` unrotated and marks read the wrong cell.
    pub(crate) fn display_rows(&self, e: EraseExtent) -> (Row, Row) {
        let rows = Row::from_index(self.rows.index());
        match e {
            EraseExtent::ToStart => (Row::ZERO, self.cy),
            EraseExtent::All => (Row::ZERO, rows),
            EraseExtent::ToEnd => (Row::from_index(self.cy.index() + 1), rows),
        }
    }

    // ---- rendition ------------------------------------------------------

    /// **A bounded table, open-addressed over its whole length so a distinct
    /// rendition evicts another only when every slot is occupied.**
    /// The read stays one load -- a cell holds the final slot as its `StyleId`
    /// and `resolve` is `tmpl[id]` -- and only the intern, run once per `SGR`
    /// rather than per cell, probes.
    ///
    /// The evict-on-collision version this replaced was a visible bug, not a
    /// rare one: a foreground cell and a colliding background style shared a
    /// bucket, the later store evicted the earlier, and the foreground cell
    /// then resolved to the other style's background -- a coloured box behind
    /// text on any screen with enough distinct renditions to collide, which a
    /// 256-colour chart or a busy TUI reaches well under the 1024 slots. The
    /// birthday bound is the tell: ~38 distinct styles already give an even
    /// chance of one collision in 1024 buckets, so "rare" was wrong.
    ///
    /// Linear probing fixes it without a search on the read. A distinct style
    /// takes the first free or matching slot from its bucket, so it evicts
    /// another style only when the probe wraps the whole table -- that is,
    /// only once every slot is filled, live or not, since no slot is ever
    /// freed. Below that the mapping is **exact**: distinct renditions never
    /// share a slot and never corrupt each other's colour.
    ///
    /// The cost is a probe on the write path, and the probe length is
    /// `1/(1 - load)` on average -- a fraction of a slot while the table is
    /// far from full, which a screen holding a few hundred renditions in a
    /// screen-sized table is.
    ///
    /// **The table is sized to the screen** (the screen-sizing in `Owned::with_history`), not fixed at
    /// 1024, because a text-drawn image preview mints one distinct truecolour
    /// rendition per cell and every one is live at once -- the whole image is on
    /// screen. A 1024-slot table under a photo evicts thousands of on-screen
    /// cells' renditions, and each resolves to a later cell's colour: the top
    /// band of the preview scrambles while the bottom stays coherent, because
    /// eviction hits the cells interned first. A table holding a screenful of
    /// distinct renditions cannot: below capacity the mapping is exact. A
    /// garbage collector could not help, because nothing is dead to reclaim
    /// while the image is displayed; the fix is capacity, sized to what a screen
    /// can show. Eviction remains for a session that mints more *distinct*
    /// renditions over its life than the table has slots -- an animation
    /// cycling colours, not a still image -- and a cell still naming an evicted
    /// slot is then recoloured: an open `SGR` bug, not a settled trade.
    ///
    /// **Inlined, because `restore` is its second caller.** With one caller
    /// LLVM inlined it into `apply_sgr`; when `DECRC` began interning the
    /// saved rendition again it outlined it, and every `SGR` paid a call.
    /// Forced, `apply_sgr` compiles as it did with one caller.
    #[inline(always)]
    fn intern_style(&mut self, raw: StyleWord) -> StyleId {
        // The table is sized to the screen at construction (the screen-sizing in `Owned::with_history`), so
        // `len` is a screen's worth of slots -- a power of two, always at least
        // `NSTYLE`. Below that many distinct renditions interned since
        // construction the mapping is exact, which is why a text-drawn image
        // preview (one distinct rendition per cell) no longer scrambles: a
        // screenful of them all fit.
        let len = self.tmpl.len();
        #[cfg(feature = "probe-double-intern")]
        core::hint::black_box(StyleId::bucket(raw, len));
        #[cfg(feature = "probe-double-intern-full")]
        {
            // Slot 0 is the reserved default (`Term::new`), which this arm
            // must not overwrite any more than the eviction below may.
            let k2 = StyleId::bucket(raw, len).max(1);
            self.intern_new(StyleId::from_index(k2), raw);
        }
        let base = StyleId::bucket(raw, len);
        // Probe for this style's own slot or the first free one. `len` is a
        // power of two, so `& (len - 1)` wraps and the loop visits every slot
        // exactly once. A hit returns the slot the style already has, which is
        // why equal cells share a `StyleId`.
        let mut k = base;
        for _ in 0..len {
            match self.tmpl[k] {
                Some(w) if w == raw => return StyleId::from_index(k),
                None => return self.intern_new(StyleId::from_index(k), raw),
                Some(_) => k = (k + 1) & (len - 1),
            }
        }
        // Every slot is filled: evict at the bucket, the only case a fixed
        // table cannot serve exactly, and the read stays one load --
        // except slot 0, the default every blank cell names without
        // interning it (`Term::new`), whose eviction would recolour them all.
        // Its bucket's victim is the slot after it.
        self.intern_new(StyleId::from_index(base.max(1)), raw)
    }

    /// A rendition written into slot `id`, and the write logged: a miss, which
    /// an `SGR` usually is not, so out of line where the hit stays inline.
    /// Every write to the table goes through here, so the log holds every
    /// slot a renderer has to resolve again (`Term::styles_written`).
    #[cold]
    #[inline(never)]
    fn intern_new(&mut self, id: StyleId, raw: StyleWord) -> StyleId {
        self.tmpl[id.index()] = Some(raw);
        self.style_log.note(id);
        id
    }

    /// SGR. **The single-parameter form first**, because that is what a
    /// sixteen-colour program emits and it reaches the attribute without
    /// building a `Params` at all.
    pub(crate) fn apply_sgr(&mut self, csi: &crate::write::Csi) {
        // **Folded in place, not through a copy.** `StyleWord` is twenty
        // bytes, and reading it into a local, folding, and writing it back was
        // forty bytes of copying per `SGR` -- on `asciiquarium.raw`, where the
        // fold is 24% of the clock and there are 876,840 of them a rep.
        #[cfg(feature = "probe-double-sgr")]
        {
            let mut again = self.style_raw;
            csi.fold_sgr(&mut again);
            core::hint::black_box(again);
        }
        // **The copy alone, so it can be subtracted.** `probe-double-sgr`
        // reads the twenty-byte `StyleWord` into a local and `black_box`es it
        // back to memory, and the real path does neither -- so its figure is
        // the fold *plus* a copy and a forced spill per `SGR`. Same
        // contamination as `probe-csi-scratch`, and the same fix.
        #[cfg(feature = "probe-sgr-copy")]
        {
            let again = self.style_raw;
            core::hint::black_box(again);
        }
        // **One parameter is a table lookup, not a grammar walk.** It is 43%
        // of `btop.raw`'s `SGR`s and 80% of `asciiquarium.raw`'s, and the
        // answer is a pure function of the value. `get` declines the colour
        // introducers and anything the grammar does not recognise, which fall
        // through to the general path rather than silently applying nothing.
        csi.fold_sgr(&mut self.style_raw);
        self.style = self.intern_style(self.style_raw);
    }

    /// The same rendition change, from the table instead of from the bytes.
    ///
    /// This is the whole hit path: no parameter walk, no `SgrAttr::parse`, no
    /// fold over the parameter array -- one masked replace and the intern that
    /// `apply_sgr` also does. `probe-double-intern` prices the intern at
    /// nothing, so what is left of the `SGR` route on a hit is the compare.
    #[cfg(feature = "sgr-table")]
    #[inline(always)]
    pub(crate) fn apply_fold(&mut self, f: StyleFold) {
        f.apply(&mut self.style_raw);
        self.style = self.intern_style(self.style_raw);
    }

    /// **Resolve `GL` into the one byte the write path reads.**
    ///
    /// Designation loads a slot and a locking shift picks the slot; neither is
    /// what `put_ascii_run` wants, which is *which set is in force*. So the
    /// slots live in `Cold` and this is the single place that turns them into
    /// `Paint::charset` -- called by every path that moves a slot or `gl`, so
    /// the resolved byte cannot go stale behind them.
    pub(crate) fn invoke(&mut self) {
        self.charset = self.cold.g[self.cold.gl.index()];
        self.refresh_flags();
    }

    /// Recombine DECSCA protection, the active character set and the open
    /// `OSC 8` link, so the write path reads one value instead of three.
    ///
    /// **The link rides in the cell rather than in a side table**, which is
    /// what makes a link survive everything a cell survives: a scroll rotates
    /// the grid and a link goes with it, an overwrite builds a fresh
    /// `PackedCell` from the flags in force and un-links the cell without the
    /// write path doing anything. That is the same argument `CellFlags::MARKED`
    /// settled for combining marks, and the same reason a position-keyed side
    /// table needed a generation counter and three bug fixes.
    pub(crate) fn refresh_flags(&mut self) {
        let base = match self.charset {
            Charset::DecGraphics => self.cold.protect.with(CellFlags::GRAPHICS),
            _ => self.cold.protect,
        };
        self.flags = base.with_link(self.link);
    }

    /// REP: repeat the last graphic character. Bounded by the row, so
    /// `CSI 65535 b` cannot spin.
    pub(crate) fn repeat(&mut self, c: Count) {
        let Some((cp, w)) = self.last_cp else {
            return;
        };
        for _ in 0..c.at_least_one().min(self.cols.index()) {
            self.glyph(cp, w);
        }
    }

    // ---- tabs -----------------------------------------------------------

    pub(crate) fn reset_tabs(&mut self) {
        for (i, stop) in self.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
    }

    pub(crate) fn tab_forward(&mut self, n: Count) {
        let last = self.cols.index() - 1;
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x >= last {
                    x = last;
                    break;
                }
                x += 1;
                if self.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }

    pub(crate) fn tab_back(&mut self, n: Count) {
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x == 0 {
                    break;
                }
                x -= 1;
                if self.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }

    pub(crate) fn tab_clear(&mut self, w: Option<TabClear>) {
        match w {
            Some(TabClear::All) => self.tabs.fill(false),
            Some(TabClear::AtCursor) => {
                let x = self.cx.index();
                if x < self.tabs.len() {
                    self.tabs[x] = false;
                }
            }
            // **`None` was clearing the stop at the cursor, and that is a
            // wrong action rather than a missing one.** `TabClear::from_param`
            // answers `None` for everything but 0 and 3, and ECMA-48 § 8.3.154
            // gives 1 and 4 to *line* tab stops and 2 to the character stops in
            // the active line. baud has no vertical tabs, so `CSI 1 g` -- a
            // request about a line stop -- silently removed a **character**
            // stop and the next `HT` went somewhere else.
            //
            // Counted instead, which is also what the conformance inventory
            // asks: the wildcard made an unimplemented parameter observationally
            // identical to an implemented one.
            None => self.unsupported_param += 1,
        }
    }

    // ---- margins and columns -------------------------------------------

    /// The margins a `RIS`, a `DECSTR` or a resize puts back.
    pub(crate) fn reset_side_margins(&mut self) {
        self.cold.left = Col::ZERO;
        self.cold.right = Col::at_index(self.cols.index() - 1);
    }

    /// `DECSLRM`, `CSI Pl ; Pr s`.
    ///
    /// `[[source: vt510-decslrm]]`: "The value of the left margin (Pl) must be
    /// less than the right margin (Pr) ... The minimum size of the scrolling
    /// region is two columns ... DECSLRM moves the cursor to column 1, line 1
    /// of the page."
    ///
    /// **Rejected rather than clamped.** An invalid pair leaves the margins
    /// alone, because clamping would confine the screen to a region the
    /// application did not ask for and then behave correctly with respect to
    /// it -- which is worse than doing nothing, since nothing is visible as a
    /// bug and a plausible wrong region is not.
    pub(crate) fn set_side_margins(&mut self, left: OneBased, right: OneBased) {
        let last = self.cols.index() - 1;
        let l = left.to_col(self.cols).index();
        let r = right.to_col(self.cols).index();
        if l >= r || r > last {
            return;
        }
        self.cold.left = Col::at_index(l);
        self.cold.right = Col::at_index(r);
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
    }

    /// Is the cursor inside the scrolling region, in **both** axes?
    ///
    /// `DECIC` and `DECDC` are defined only there, so the rectangle both
    /// margin pairs describe is what bounds them -- not the full height, which
    /// is what a vertical-only test would give.
    pub(crate) fn within_margins(&self) -> bool {
        let (x, y) = (self.cx.index(), self.cy.index());
        x >= self.cold.left.index()
            && x <= self.cold.right.index()
            && y >= self.cold.top.index()
            && y <= self.cold.bot.index()
    }

    /// `DECIC`, `CSI Pn ' }`.
    ///
    /// `[[source: vt510-decic]]`: "As columns are inserted, the columns
    /// between the cursor and the right margin move to the right ... DECIC has
    /// no effect outside the scrolling margins."
    pub(crate) fn insert_columns(&mut self, n: Count) {
        if !self.within_margins() {
            return;
        }
        self.shift_columns(n, ColumnShift::Insert);
    }

    /// `DECDC`, `CSI Pn ' ~`. The inverse, filling at the right margin.
    pub(crate) fn delete_columns(&mut self, n: Count) {
        if !self.within_margins() {
            return;
        }
        self.shift_columns(n, ColumnShift::Delete);
    }

    /// Move every row's columns between the cursor and the **right margin**.
    ///
    /// The `to` bound was `cols` before the margins existed, so an insert
    /// pushed text off the screen edge rather than off the margin -- correct
    /// only while the margin *is* the edge, which is every stream that never
    /// sends `DECSLRM`.
    pub(crate) fn shift_columns(&mut self, n: Count, dir: ColumnShift) {
        let (b, to) = (self.blank(), self.cold.right.after());
        for y in self.cold.top.index()..=self.cold.bot.index() {
            let r = Row::from_index(y);
            match dir {
                ColumnShift::Insert => self.view.insert_cells(r, self.cx, n, to, b),
                ColumnShift::Delete => self.view.delete_cells(r, self.cx, n, to, b),
            }
            self.cells = self.cells.plus(to.index() - self.cx.index());
        }
    }

    // ---- modes ----------------------------------------------------------

    /// `at` is where the sequence's final byte is in the arena, which a `?25l`
    /// looks back from (`placed.rs`).
    pub(crate) fn apply_private_modes(
        &mut self,
        csi: &crate::write::Csi,
        change: ModeChange,
        node: &mut crate::graph::Node,
        at: BytePos,
    ) {
        // **Every parameter, not the first.** `CSI ? 1 ; 3 ; 4 ; 6 l` sets
        // four modes and rxvt's own reset string is exactly that; reading one
        // left the rest silently unset.
        csi.for_each_param(|p| self.set_private_mode(PrivateMode::from_wire(p), change, node, at));
    }

    /// `?5`, counted where it happens.
    fn set_reverse_video(&mut self, change: ModeChange) {
        let during = self.update();
        self.flip_reverse_video(change, during);
    }

    /// Whether a synchronised update is open now.
    fn update(&self) -> Update {
        if self.cold.modes.is_set(ModeBit::SynchronizedOutput) {
            Update::Held
        } else {
            Update::Released
        }
    }

    /// `?5`, and the count of its changes a driver reads to see a flip no
    /// frame drew ([`crate::ReverseFlips`]): a change made `during` an open
    /// update goes in the held count, any other in the shown one. Every
    /// change of the bit goes through here -- `hard_reset`'s `Modes::default`
    /// after it writes the value this has already set -- and a change made
    /// around this would be a flash nothing could see.
    fn flip_reverse_video(&mut self, change: ModeChange, during: Update) {
        let was = self.cold.modes.is_set(ModeBit::ReverseVideo);
        let on = change == ModeChange::Set;
        self.cold.modes.apply(ModeBit::ReverseVideo, on);
        if was != on {
            match during {
                Update::Released => self.cold.flips = self.cold.flips.next(),
                Update::Held => self.cold.flips_held = self.cold.flips_held.next(),
            }
        }
    }

    fn set_private_mode(
        &mut self,
        m: PrivateMode,
        change: ModeChange,
        node: &mut crate::graph::Node,
        at: BytePos,
    ) {
        let on = change == ModeChange::Set;
        match m {
            PrivateMode::Origin => {
                self.cold.modes.apply(ModeBit::Origin, on);
                // Both directions home the cursor, and where home *is* depends
                // on the mode just applied.
                let r = match on {
                    true => self.cold.top,
                    false => Row::ZERO,
                };
                self.goto(r, Col::ZERO);
            }
            PrivateMode::AutoWrap => self.cold.modes.apply(ModeBit::AutoWrap, on),
            // **`?3` DECCOLM is not just a bit.** `[[source: ctlseqs]]` has it
            // reset the scrolling region, home the cursor and **clear the
            // screen**, either way -- an application uses it to get a known
            // state, so a core that recorded the width and left the grid alone
            // would leave the previous screen under the new geometry. The
            // *width* itself is the embedder's: this core does not resize
            // itself, because the window belongs to whoever owns it.
            PrivateMode::Column132 => {
                self.cold.modes.apply(ModeBit::Column132, on);
                self.cold.top = Row::ZERO;
                self.cold.bot = Row::from_index(self.rows.index() - 1);
                self.cy = Row::ZERO;
                self.cx = Col::ZERO;
                self.clear_screen();
            }
            // **`?4` and `?8` are recorded and exposed, which is what acting
            // on them means here.** Smooth scroll is renderer policy and
            // auto-repeat is keyboard policy; this core has neither, and an
            // embedder cannot implement either without being told. Dropping
            // them is the "set and never read" failure in reverse -- a mode
            // parsed and discarded, which `docs/vt-coverage.md` rates worse
            // than not implementing it.
            // **`?12` is a second input to the blink, not a write to the
            // style** -- foot's rule, the owner's F of 2026-09-27. It used to
            // fold into the style, so the `?12l` vim sends at start and every
            // `cnorm` sends stopped a configured blink for good. Now the drawn
            // blink is `?12` *or* the style's own (`Term::cursor_style`), and
            // `DECSCUSR` and `?12` each keep what they were told.
            PrivateMode::CursorBlink => {
                self.cold.cursor_blink = ModeChange::of(on);
            }
            // **`?2004` and `?2026` are the embedder's, and exposing them is
            // acting on them.** Bracketing a paste is the input path's job and
            // holding a frame is the renderer's; this core has neither, so
            // recording the request and answering `DECRQM` truthfully is the
            // whole of the terminal's part.
            // **`DECLRMM` is what makes `DECSLRM` mean anything**, and
            // clearing it puts the margins back rather than leaving a region
            // no mode admits to. `[[source: ctlseqs]]` for `?69 l`: "disable
            // left and right margin mode".
            PrivateMode::ReverseWrap => self.cold.modes.apply(ModeBit::ReverseWrap, on),
            PrivateMode::LeftRightMargin => {
                self.cold.modes.apply(ModeBit::SideMargins, on);
                if !on {
                    self.reset_side_margins();
                }
            }
            PrivateMode::BracketedPaste => self.cold.modes.apply(ModeBit::BracketedPaste, on),
            PrivateMode::FocusReporting => self.cold.modes.apply(ModeBit::FocusReporting, on),
            PrivateMode::SynchronizedOutput => {
                if on && !self.cold.modes.is_set(ModeBit::SynchronizedOutput) {
                    self.cold.updates_opened = self.cold.updates_opened.next();
                }
                self.cold.modes.apply(ModeBit::SynchronizedOutput, on)
            }
            // **Enabling it reports at once.** The specification requires
            // the current size on enablement and after every resize, so an
            // application that has just asked never has to guess what it
            // has, and one that re-enables an already-enabled mode gets a
            // fresh report rather than silence.
            PrivateMode::InbandResize => {
                self.cold.modes.apply(ModeBit::InbandResize, on);
                if on {
                    let r = Report::inband(self.rows, self.cols, self.cell_width, self.cell_height);
                    self.want_reply(r);
                }
            }
            // **Nothing is sent on setting it.** tmux removed a report on
            // enable after it raced an exiting program and typed `997;2n`
            // into the next one, and no verified implementation sends one;
            // an application that wants the class asks with `CSI ? 996 n`.
            PrivateMode::ColorPaletteUpdates => {
                self.cold.modes.apply(ModeBit::ColorPaletteUpdates, on)
            }
            PrivateMode::SmoothScroll => self.cold.modes.apply(ModeBit::SmoothScroll, on),
            PrivateMode::AutoRepeat => self.cold.modes.apply(ModeBit::AutoRepeat, on),
            // **The input-side modes.** Each decides what a *key* sends, so
            // "acting on it" is exposing it -- the same argument
            // `ModeBit::BracketedPaste` already makes: an embedder cannot
            // implement the mode without being told, and being told is the
            // whole of the terminal's part. `Term::cursor_keys_application`
            // and friends are the channel, and the key encoder reads them.
            PrivateMode::CursorKeys => self.cold.modes.apply(ModeBit::CursorKeys, on),
            PrivateMode::MetaSendsEscape => self.cold.modes.apply(ModeBit::MetaSendsEscape, on),
            PrivateMode::AltSendsEscape => self.cold.modes.apply(ModeBit::AltSendsEscape, on),
            PrivateMode::EightBitInput => self.cold.modes.apply(ModeBit::EightBitInput, on),
            // **Every hide decides where a program placed the cursor**, a hide
            // of a hidden cursor too, and a show forgets it (`placed.rs`).
            PrivateMode::CursorVisible => {
                self.cold.modes.apply(ModeBit::CursorVisible, on);
                match on {
                    true => self.cold.placement.forget(),
                    false => {
                        let cursor = (self.cy, self.cx);
                        self.cold
                            .placement
                            .hidden(self.arena, at, cursor, self.cols)
                    }
                }
            }
            PrivateMode::ReverseVideo => self.set_reverse_video(change),
            // **Mouse tracking is a level, not four flags.** `[[source:
            // ctlseqs]]` has xterm treat each DECSET as *selecting* one, and
            // `?1003` is a superset of `?1002` which is a superset of `?1000`
            // -- so resetting any of them turns tracking off, which is what an
            // application expects from `?1000 l`. `?9` is the fourth level and
            // follows the same rule: xterm's `really_set_mousemode` stores
            // `enabled ? mode : MOUSE_OFF` for all of them.
            PrivateMode::MouseX10Compat => self.set_tracking(MouseTracking::X10Compat, change),
            PrivateMode::MouseNormal => self.set_tracking(MouseTracking::Normal, change),
            PrivateMode::MouseButtonEvent => self.set_tracking(MouseTracking::ButtonEvent, change),
            PrivateMode::MouseAnyEvent => self.set_tracking(MouseTracking::AnyEvent, change),
            // **The encodings are exclusive, and a reset undoes only its
            // own.** Resetting the one in force returns to X10, the
            // protocol's default rather than "off" -- there is no off -- and
            // resetting another changes nothing: xterm's "a reset is only
            // effective against the matching mode" (`MouseEncoding::after`).
            // `?1006 l` returned to X10 whatever was set until 2026-10-09,
            // and `?1005`, `?1015` and `?1016` were counted and dropped.
            PrivateMode::MouseUtf8 => self.set_encoding(MouseEncoding::Utf8, change),
            PrivateMode::MouseSgr => self.set_encoding(MouseEncoding::Sgr, change),
            PrivateMode::MouseUrxvt => self.set_encoding(MouseEncoding::Urxvt, change),
            PrivateMode::MouseSgrPixels => self.set_encoding(MouseEncoding::SgrPixels, change),
            // **`?1007` is the wheel's, and exposing it is acting on it**
            // (`Term::alternate_scroll`), as `?1` is the keys'. It was
            // counted and dropped while the app sent the arrows anyway.
            PrivateMode::AlternateScroll => {
                self.cold.alternate_scroll = match change {
                    ModeChange::Set => AlternateScroll::On,
                    ModeChange::Reset => AlternateScroll::Off,
                }
            }
            PrivateMode::HighlightTracking => {
                self.cold.highlight = match on {
                    true => HighlightTracking::On,
                    false => HighlightTracking::Off,
                }
            }
            // `[[source: ctlseqs]]` for `?1049 h`: save the cursor as DECSC,
            // switch to the alternate buffer, clear it. For `?1049 l`: use the
            // normal buffer and restore as DECRC.
            //
            // **Idempotent in both directions.** A second `?1049 h` must not
            // re-save over the cursor it saved on the way in.
            // **DECANM: `CSI ? 2 l` leaves ANSI, and leaving is a parser
            // transition.** The mode is recorded *and* the node moves, because
            // the very next byte has to be read by the VT52 graph -- `ESC [`
            // there is a command VT52 does not define rather than an
            // introducer. Recording the mode without moving the node is the
            // "a bit that is set and never read" failure; moving the node
            // without recording it would leave `terminal_mode()` lying.
            PrivateMode::AnsiMode => {
                self.cold.terminal = match change {
                    ModeChange::Set => TerminalMode::Ansi,
                    ModeChange::Reset => TerminalMode::Vt52,
                };
                *node = match self.cold.terminal {
                    TerminalMode::Ansi => crate::graph::Node::Ground,
                    TerminalMode::Vt52 => crate::graph::Node::Vt52Ground,
                };
            }
            PrivateMode::AltScreen => self.set_alt_screen(change, AltSwitch::SaveAndClear),
            // **`?47` switches buffers and does nothing else.** xterm's
            // `srm_ALTBUF` is `ToAlternate(xw, False)` in and
            // `FromAlternate(xw, False)` out (`xterm-charproc.c`, the
            // `srm_ALTBUF` case): no cursor save or restore, and no clear, so
            // the alternate screen shows what it held when it was last left.
            // It was routed nowhere while `docs/vt-coverage.md` called it
            // done, and its one test passed on a no-op.
            PrivateMode::AltScreenLegacy => self.set_alt_screen(change, AltSwitch::Bare),
            // **Counted, as the ANSI table's wildcard is.** This was a bare
            // `{}` while `?1004` was named nowhere in the code, so the request
            // `vim-unicode.raw` sends on its first line arrived and vanished
            // with nothing to say it had; a mode dropped uncounted is the
            // failure `docs/vt-coverage.md` rates worst.
            _ => self.unsupported_param += 1,
        }
    }

    /// Takes the [`ModeChange`] the caller already holds rather than a `bool`:
    /// degrading a domain type to a primitive and reconstructing it here is
    /// what the audit exists to stop.
    fn set_tracking(&mut self, level: MouseTracking, change: ModeChange) {
        self.cold.mouse = match change {
            ModeChange::Set => level,
            ModeChange::Reset => MouseTracking::Off,
        };
        // A mode change forgets the last reported cell, so the first move
        // after it reports wherever it is: the report that set the cell went
        // out under the old level, perhaps to another program. xterm forgets
        // only when the widget is made (`VTInitialize`, `[[source:
        // xterm-reset]]`'s file), which is practice the text's "moved to a
        // different character cell" does not ask for.
        self.cold.mouse_at = None;
    }

    /// The encoding after `change` of the mode selecting `mode`; the rule is
    /// [`MouseEncoding::after`]'s, kept beside the type it describes.
    fn set_encoding(&mut self, mode: MouseEncoding, change: ModeChange) {
        self.cold.mouse_encoding = self.cold.mouse_encoding.after(mode, change);
        // Forgotten for `set_tracking`'s reason: a report written in the
        // old encoding is not one an application reading the new one has.
        self.cold.mouse_at = None;
    }

    /// **`ModeChange`, not a `bool`.** The two states already have a name in
    /// the protocol, and this repo's audit exists to stop that name being
    /// thrown away at a call site -- `set_alt_screen(true)` reads as nothing
    /// in particular where `ModeChange::Set` reads as DECSET.
    ///
    /// **What differs between `?1049` and `?47` is the [`AltSwitch`], and
    /// nothing else.** Both swap the surfaces, the kitty stacks and the prompt
    /// state, move the epoch and delete the alternate screen's images on the
    /// way out; only `?1049` saves the cursor and clears on the way in and
    /// restores it on the way out.
    fn set_alt_screen(&mut self, change: ModeChange, how: AltSwitch) {
        use crate::term::Screen;
        // A placement names a position on the screen it was made on, and a
        // switch leaves that screen, clears it or restores a saved cursor
        // (`placed.rs`). The calls that do none of these -- `?47 l` on the
        // primary, `?47 h` while it is up -- forget it too, at no cost that
        // matters: a hide after any of them is not right after a move.
        self.cold.placement.forget();
        match (change == ModeChange::Set, self.cold.screen) {
            (true, Screen::Normal) => {
                if how == AltSwitch::SaveAndClear {
                    self.cold.alt_saved = Some(AltSaved {
                        cursor: self.save(),
                        style: self.cold.cursor_style,
                        blink: self.cold.cursor_blink,
                    });
                }
                self.cold.alt_entry = how;
                self.cold.screen = Screen::Alternate;
                self.cold.epoch = self.cold.epoch.next();
                // **The kitty keyboard stacks are per-screen.** The active one
                // parks and the alternate's takes over, so an editor's flags do
                // not leak back to the shell -- the specification's separate
                // main/alternate stacks. A swap is balanced because this arm and
                // the exit arm each fire once per transition.
                core::mem::swap(&mut self.cold.kitty, &mut self.cold.kitty_alt);
                // **And so is what the prompt marks said.** The primary's
                // zone and block park; the alternate screen's start fresh on
                // every entry. `?1049` clears the screen below; `?47` keeps
                // its rows, and still does not carry a block start across
                // visits, because what a program left there is not a prompt
                // the shell will redraw by counting rows.
                core::mem::swap(&mut self.cold.shell, &mut self.cold.shell_other);
                self.cold.shell = crate::prompt::Shell::FRESH;
                // **Two surfaces, swapped, nothing copied.** The alternate
                // screen is its own buffer at the pitch; the view takes it
                // and the primary rides as `other` until the exit swaps them
                // back, its cells, marks, ring and rotation all where they
                // were. This replaced a park that copied the primary out and
                // back, which was the one thing standing between the grid
                // and being the GPU's in place on both screens.
                self.view.swap_surface(&mut self.other);
                // `?1049` clears the alternate screen on entry, so a rotation
                // left from the last time an application was in it describes
                // nothing. Blanks rotate to blanks, so this is tidiness rather
                // than a fix -- and an invariant that holds only by accident is
                // one the next change breaks. **`?47` does none of the three**:
                // it shows the rows as they were left, so resetting the
                // rotation would put them out of order, and the cursor stays
                // where it is, as xterm's `SwitchBufs` leaves it.
                if how == AltSwitch::SaveAndClear {
                    self.view.reset_rot();
                    self.clear_screen();
                    self.goto(Row::ZERO, Col::ZERO);
                }
                // Its image numbering starts again with the screen. No image
                // can be on it: leaving is the only way off it, and leaving
                // deletes them.
                self.book.alt_top = LineIdx::FIRST;
            }
            (false, Screen::Alternate) => {
                // **The editor's images go with the editor's screen.** foot
                // and ghostty both delete them; the alternate screen keeps no
                // history, so nothing could scroll back to them, and a shell
                // that got them would show a picture over its own text.
                if self.images.any_placed() {
                    self.images.expire_screen(ImageScreen::Alternate);
                }
                self.cold.screen = Screen::Normal;
                self.cold.epoch = self.cold.epoch.next();
                // Restore the primary screen's kitty stack and prompt state;
                // see the enter arm.
                core::mem::swap(&mut self.cold.kitty, &mut self.cold.kitty_alt);
                core::mem::swap(&mut self.cold.shell, &mut self.cold.shell_other);
                // Surface first, then the cursor: `restore` clamps against the
                // geometry and moves the cursor, and it should be moving over
                // the screen the application is about to see. A resize taken
                // while the alternate screen was up was applied to the
                // primary as it happened, around the cursor it will have --
                // the one `?1049` saved, or under `?47` the one both screens
                // share (`Term::primary_cursor`) -- so there is nothing to
                // catch up on here.
                self.view.swap_surface(&mut self.other);
                // **`?47` restores nothing, and leaves the save alone.** A
                // cursor `?1049 h` saved outlives a `?47 l` exit, as the
                // cursor xterm saves outlives `FromAlternate`.
                if how == AltSwitch::SaveAndClear {
                    self.restore_alt();
                }
            }
            // **`?1049 h` while the alternate screen is up clears it**, as
            // xterm's does: its set arm is `CursorSave; ToAlternate;
            // ClearScreen`, and `ToAlternate` is a no-op on the alternate
            // buffer while the clear is not (`xterm-charproc.c`, the
            // `srm_OPT_ALTBUF_CURSOR` case). It saves nothing here, because
            // the save is the primary's and this cursor is the alternate
            // screen's; and it does not home, because xterm's `ClearScreen`
            // does not (`xterm-util.c`). `?47 h` then `?1049 h` used to
            // leave the `?47` screen's text under the program that asked for
            // a cleared one.
            (true, Screen::Alternate) => {
                if how == AltSwitch::SaveAndClear {
                    self.clear_screen();
                }
            }
            // **`?1049 l` on the primary restores what `?1049 h` saved**, as
            // xterm's `CursorRestore` runs whatever `FromAlternate` did. The
            // primary can be up with a save pending only since `?47`: a
            // `?1049 h ... ?47 l` leaves it, and a later `?1049 l`, or `RIS`,
            // which leaves the alternate screen first, takes it here.
            (false, Screen::Normal) => {
                if how == AltSwitch::SaveAndClear {
                    self.restore_alt();
                }
            }
        }
    }

    pub(crate) fn apply_ansi_mode(&mut self, change: ModeChange, mode: AnsiMode) {
        let on = change == ModeChange::Set;
        match mode {
            AnsiMode::InsertReplace => self.cold.modes.apply(ModeBit::InsertReplace, on),
            AnsiMode::LineFeedNewLine => self.cold.modes.apply(ModeBit::LineFeedNewLine, on),
            _ => self.unsupported_param += 1,
        }
    }

    pub(crate) fn clear_screen(&mut self) {
        for y in 0..self.rows.index() {
            let b = self.blank();
            let n = self.view.fill_row(Row::from_index(y), b);
            self.cells = self.cells.plus(n.index());
        }
    }

    /// DECALN: fill the screen with `E`, home the cursor, drop the region.
    pub(crate) fn screen_alignment(&mut self) {
        let cell = PackedCell::glyph(
            Codepoint::from_ascii(StreamByte::from_stream(b'E')),
            self.style,
            self.flags,
        );
        for y in 0..self.rows.index() {
            let n = self.view.fill_row(Row::from_index(y), cell);
            self.cells = self.cells.plus(n.index());
        }
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(self.rows.index() - 1);
        self.goto(Row::ZERO, Col::ZERO);
    }

    /// RIS. **Everything a soft reset does, and the screen as well.**
    pub(crate) fn hard_reset(&mut self) {
        // **`RIS` returns to the normal screen and `DECSTR` does not**, which
        // is why this is here rather than in `soft_reset` -- and **first**,
        // because leaving restores what `?1049 h` saved: the rendition, the
        // charsets, DECAWM, DECOM and DECSCA's protection. After the soft
        // reset it put all of that back over it, and a reset issued from the
        // alternate screen left the shell's state rather than the power-on
        // one. xterm's hard branch leaves and then restores the initial flags
        // (`[[source: xterm-reset]]`); leaving first and resetting after is
        // the same order.
        // This takes a save `?47 l` left pending as well, on the primary: see
        // `set_alt_screen`'s `(false, Normal)` arm.
        // Whether an update was open when the reset arrived, for `?5`'s count
        // below; the soft reset releases it.
        let during = self.update();
        self.set_alt_screen(ModeChange::Reset, AltSwitch::SaveAndClear);
        self.soft_reset();
        // **What `RIS` resets and `DECSTR` does not.** `soft_reset` leaves
        // mouse tracking alone deliberately -- a full-screen application
        // recovering with `CSI ! p` must not have it pulled out from under it,
        // and xterm clears it only in `ReallyReset`'s `if (full)` branch -- and
        // the modes xterm's soft reset keeps. A power-on reset has no such
        // caller to protect. The keypad was here as well, and is in
        // `soft_reset` now: xterm resets it on both paths.
        // `?5` goes back to normal through the one writer that counts a
        // change, as of *before* the reset: `soft_reset` above has released
        // `?2026` by now, and a reset abandons an open update rather than
        // committing it, so a reverse video that only ever existed inside one
        // is not a flip anything could have shown.
        self.flip_reverse_video(ModeChange::Reset, during);
        self.cold.modes = Modes::default();
        self.cold.mouse = MouseTracking::Off;
        self.cold.mouse_encoding = MouseEncoding::X10;
        self.cold.highlight = HighlightTracking::Off;
        self.cold.terminal = TerminalMode::Ansi;
        self.cold.vt52_row = Vt52Coord::ZERO;
        // The open hyperlink is content rather than mode: a `RIS` clears the
        // screen, so a link that outlived it would apply to cells that no
        // longer exist.
        self.link = LinkId::NONE;
        self.refresh_flags();
        // **The history goes, and the rotation stays.** A reset erases; it
        // does not scroll. It reset the rotation and left the history counts,
        // so the scrollback read rows the reset had not written at the old
        // length, a taller window pulled the window back over them, and rows
        // a shrink had hidden below the screen came back after the reset
        // that erased the screen they were part of. xterm's `ESC c` clears
        // its saved lines where `DECSTR` keeps them (`[[source: xterm-reset]]`),
        // alacritty clears the history on both grids
        // (`[[source: alacritty-reset-state]]`, `[[source: alacritty-grid-reset]]`),
        // and foot's hard reset frees every row past the screen
        // (`[[source: foot-term-reset]]`). The Linux console
        // (`[[source: linux-vt-ris]]`) and sfte (`[[source: sfte-ris]]`)
        // keep it. The rotation stays because a reset scrolls no row: reset,
        // it read as a row count the short way round the ring, and what an
        // embedder should do across a reset -- drop a selection, bring a
        // scrolled-back window to the live screen -- is not a count of rows.
        // (baud-app dropped the selection only by that accident, reading a
        // rotation under half the ring as rows restored; see the TODO.)
        self.book.hist = Rows::ZERO;
        self.book.hidden = Rows::ZERO;
        // And what the marks said: the block start names rows the reset
        // erased, and the shell is at no prompt after one. This is the
        // primary's -- `set_alt_screen` above came back to it -- and the
        // alternate screen's starts fresh on its next entry, as its cells do.
        self.cold.shell = crate::prompt::Shell::FRESH;
        // **The whole ring, not the window.** Zeroing the counts made the old
        // lines unreadable through the scrollback and left their bytes: a
        // reader of the whole ring (`Term::cells`) still found what the user
        // ran `reset` to erase, and a narrowing re-cut that spills past the
        // screen writes only its new width into those rows, so their tails
        // came back when the screen grew again at a prompt. foot frees every
        // row past the screen and xterm empties its saved lines.
        //
        // **And the parked alternate surface, which `?47` shows as it was
        // left.** This relied on every entry clearing it, which `?1049` does
        // and `?47` does not, so a `?47 h` after a reset showed what the
        // reset was sent to erase -- and, worse, cells naming clusters and
        // links the reset frees below, which the next new cluster would take
        // and those cells would then read. xterm clears the alternate buffer
        // only when the reset is issued from it (`FromAlternate(xw, True)`);
        // baud clears it from either screen, because its slots are freed on
        // the promise that no cell a reader can reach still names one.
        let n = self.view.erase_ring();
        self.other.cells.fill(PackedCell::EMPTY);
        self.other.used.fill(RowExtent::ZERO);
        self.cells = self.cells.plus(n + self.other.cells.len());
        // **Here and not in `clear_screen`, and the difference is the
        // alternate screen.** Entering it with `?1049` clears the grid while
        // the primary's cells wait parked, still naming their clusters, so a
        // reset there
        // would lose exactly the marks the parking exists to keep. Nothing
        // overwrites them meanwhile: the store *refuses* a colliding cluster
        // rather than evicting one, so the alternate screen can be denied a
        // slot but cannot take an occupied one. `RIS` is the one point where
        // every cell a reader can reach is gone: the ring and the parked
        // alternate surface are both erased above.
        self.clusters.reset();
        // The link table for the same reason. It refuses rather than evicts,
        // because a slot a cell still names must not take another target; no
        // cell names one now, so a table filled before the reset takes links
        // after it.
        for len in self.strs.link_len.iter_mut() {
            *len = ByteLen::ZERO;
        }
        // Images go with the clusters and for the same reason: `RIS` is the one
        // point where every cell is gone, so a placement outliving a mere clear
        // is correct -- an image is content a program may redraw over -- and
        // only a reset drops them all. Leaving the alternate screen, above,
        // drops that screen's alone.
        self.images.reset();
        self.reset_tabs();
        self.last_cp = None;
        self.goto(Row::ZERO, Col::ZERO);
    }

    /// DECSTR. **A soft reset leaves the alternate screen**, which the first
    /// implementation of it in this repo did not.
    pub(crate) fn soft_reset(&mut self) {
        // **`DECSTR` does not leave the alternate screen**, and this called
        // `set_alt_screen(Reset)` as its first act. `[[source: ctlseqs]]`'s
        // soft-reset list does not include the screen, and a full-screen
        // application recovering with `CSI ! p` would be dropped onto the
        // primary mid-draw -- which is the same bug `baud-core2` fixed and
        // this file records under "DECSTR was leaving the alternate screen".
        // Found by `soft_reset_stays_on_the_alternate_screen`, ported.
        //
        // **Only the modes xterm's soft reset restores**, not the whole mode
        // word: `ReallyReset` with `full` false sets DECTCEM, clears DECLRMM
        // (`resetMarginMode`) and DECOM, copies DECCKM, DECAWM and reverse
        // wraparound from their power-on values and clears IRM. LNM, `?4`,
        // `?5`, `?8`, focus reporting, the paste brackets and the meta and
        // eight-bit input modes are RIS-only there, and this reset them all --
        // so a program recovering with `CSI ! p` lost its bracketed paste.
        // `?2026` has no xterm precedent, and a held frame is released: a soft
        // reset is a recovery.
        let power_on = Modes::default();
        for m in [
            ModeBit::CursorVisible,
            ModeBit::SideMargins,
            ModeBit::Origin,
            ModeBit::CursorKeys,
            ModeBit::AutoWrap,
            ModeBit::ReverseWrap,
            ModeBit::InsertReplace,
            ModeBit::SynchronizedOutput,
        ] {
            self.cold.modes.apply(m, power_on.is_set(m));
        }
        // The keypad and the key-modifier resources, in `ReallyReset`'s
        // common path -- `MODE_DECKPAM` cleared and `modify_now = modify_1st`
        // -- so both resets restore them. `[[source: vt510-decstr]]` gives the
        // keypad the same: DECNKM "Numeric characters".
        self.cold.keypad = KeypadMode::Numeric;
        // The palette's overrides, in the same common path: `ReallyReset`
        // calls `ResetAnsiColorRequest` for both resets and leaves `OSC 10`,
        // `11` and `12` alone (`[[source: xterm-reset]]`).
        self.reset_palette();
        self.cold.other_keys = OtherKeys::default();
        self.cold.other_keys_format = OtherKeysFormat::default();
        // The last reported mouse cell, on both resets: `DECSTR` keeps the
        // tracking level and the encoding, and the program recovering with
        // `CSI ! p` is told where the pointer is at its next move rather than
        // relying on a report it may have discarded. `RIS` reaches this
        // through `hard_reset`, and turns tracking off besides.
        self.cold.mouse_at = None;
        self.reset_side_margins();
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(self.rows.index() - 1);
        self.cold.protect = CellFlags::NONE;
        self.style_raw = StyleWord::DEFAULT;
        self.cold.saved = None;
        self.style = StyleId::DEFAULT;
        self.cold.g = [Charset::Ascii; 4];
        self.cold.gl = GSlot::G0;
        // **Both resets restore the cursor shape.** A first version put this in
        // `hard_reset` only, on the argument that `DECSCUSR` is an xterm
        // extension and xterm's soft path leaves it alone -- which is the
        // reason mouse tracking *is* excluded here. `baud-core2` sets it in
        // both, and it is the oracle; the argument was mine and the code was
        // not checked against it. xterm's `ReallyReset` agrees since it was
        // pinned: its common path, both resets', restores the shape and the
        // blink from the resources (`[[source: xterm-reset]]`'s file) -- the
        // configured style, which is what this restores.
        self.cold.cursor_style = self.cold.initial_cursor;
        // And `?12` with it, on both resets, as xterm's `ReallyReset` clears
        // its escape bit in the common path.
        self.cold.cursor_blink = ModeChange::Reset;
        // DECTCEM is shown above, and a shown cursor has no placement
        // (`placed.rs`).
        self.cold.placement.forget();
        self.invoke();
        self.refresh_flags();
        self.goto(Row::ZERO, Col::ZERO);
    }
}

/// [`ModeState`] for one bit, so `mode_state` reads as a table rather than as
/// a chain of ifs.
fn bit_state(m: Modes, b: ModeBit) -> ModeState {
    match m.is_set(b) {
        true => ModeState::Set,
        false => ModeState::Reset,
    }
}

fn tracking_state(cur: MouseTracking, level: MouseTracking) -> ModeState {
    match cur == level {
        true => ModeState::Set,
        false => ModeState::Reset,
    }
}

/// Set only for the encoding in force, as xterm answers each of the four
/// (`extend_coords == params[0]`, `do_dec_rqm`, `[[source: xterm-misc]]`).
fn encoding_state(cur: MouseEncoding, mode: MouseEncoding) -> ModeState {
    match cur == mode {
        true => ModeState::Set,
        false => ModeState::Reset,
    }
}

impl Paint<'_, '_> {
    /// `DECRQM`: what state is this mode in?
    ///
    /// **`NotRecognized` is the answer for anything this core does not
    /// record**, and that is the whole discipline. Answering `Reset` for a
    /// mode that does not exist tells an application the mode is available; it
    /// then sets the mode and believes it took effect. `docs/vt-coverage.md`
    /// refuses that outright, and the arms below are exactly the modes
    /// `set_private_mode` and `apply_ansi_mode` actually store -- so the
    /// answer cannot drift from the behaviour without one of them failing.
    #[cold]
    #[inline(never)]
    pub(crate) fn mode_state(&self, marker: Marker, mode: Param) -> ModeState {
        if marker != Marker::Private {
            return match AnsiMode::from_wire(Some(mode)) {
                AnsiMode::LineFeedNewLine => bit_state(self.cold.modes, ModeBit::LineFeedNewLine),
                AnsiMode::InsertReplace => bit_state(self.cold.modes, ModeBit::InsertReplace),
                _ => ModeState::NotRecognized,
            };
        }
        match PrivateMode::from_wire(Some(mode)) {
            PrivateMode::Origin => bit_state(self.cold.modes, ModeBit::Origin),
            PrivateMode::AutoWrap => bit_state(self.cold.modes, ModeBit::AutoWrap),
            PrivateMode::CursorKeys => bit_state(self.cold.modes, ModeBit::CursorKeys),
            PrivateMode::SmoothScroll => bit_state(self.cold.modes, ModeBit::SmoothScroll),
            PrivateMode::ReverseVideo => bit_state(self.cold.modes, ModeBit::ReverseVideo),
            PrivateMode::AutoRepeat => bit_state(self.cold.modes, ModeBit::AutoRepeat),
            PrivateMode::MetaSendsEscape => bit_state(self.cold.modes, ModeBit::MetaSendsEscape),
            PrivateMode::AltSendsEscape => bit_state(self.cold.modes, ModeBit::AltSendsEscape),
            PrivateMode::EightBitInput => bit_state(self.cold.modes, ModeBit::EightBitInput),
            PrivateMode::Column132 => bit_state(self.cold.modes, ModeBit::Column132),
            PrivateMode::CursorVisible => bit_state(self.cold.modes, ModeBit::CursorVisible),
            // **`?1020` mirrors the encoding rather than carrying a bit.**
            // There is nothing to set: the mode *is* the answer to "which
            // table is the parser using", so a bit beside it could disagree
            // with the parser and a recorded-and-never-read bit is the failure
            // `docs/vt-coverage.md` rates worst.
            PrivateMode::Utf8Resource => match self.cold.encoding {
                InputEncoding::Utf8 => ModeState::Set,
                InputEncoding::Latin1 => ModeState::Reset,
            },
            PrivateMode::ReverseWrap => bit_state(self.cold.modes, ModeBit::ReverseWrap),
            PrivateMode::LeftRightMargin => bit_state(self.cold.modes, ModeBit::SideMargins),
            PrivateMode::BracketedPaste => bit_state(self.cold.modes, ModeBit::BracketedPaste),
            PrivateMode::FocusReporting => bit_state(self.cold.modes, ModeBit::FocusReporting),
            PrivateMode::SynchronizedOutput => {
                bit_state(self.cold.modes, ModeBit::SynchronizedOutput)
            }
            PrivateMode::InbandResize => bit_state(self.cold.modes, ModeBit::InbandResize),
            PrivateMode::ColorPaletteUpdates => {
                bit_state(self.cold.modes, ModeBit::ColorPaletteUpdates)
            }
            // Not a `Modes` bit, and not the drawn blink: the bit `?12` set,
            // as foot and xterm answer it. An application that saves the mode
            // with DECRQM and restores it with `?12h`/`l` gets its own state
            // back; answering the drawn blink would have it set `?12` from a
            // configured blink, which would then override its own later
            // steady DECSCUSR.
            PrivateMode::CursorBlink => match self.cold.cursor_blink {
                ModeChange::Set => ModeState::Set,
                ModeChange::Reset => ModeState::Reset,
            },
            PrivateMode::AnsiMode => match self.cold.terminal {
                TerminalMode::Ansi => ModeState::Set,
                TerminalMode::Vt52 => ModeState::Reset,
            },
            PrivateMode::HighlightTracking => match self.cold.highlight {
                HighlightTracking::On => ModeState::Set,
                HighlightTracking::Off => ModeState::Reset,
            },
            PrivateMode::MouseX10Compat => {
                tracking_state(self.cold.mouse, MouseTracking::X10Compat)
            }
            PrivateMode::MouseNormal => tracking_state(self.cold.mouse, MouseTracking::Normal),
            PrivateMode::MouseButtonEvent => {
                tracking_state(self.cold.mouse, MouseTracking::ButtonEvent)
            }
            PrivateMode::MouseAnyEvent => tracking_state(self.cold.mouse, MouseTracking::AnyEvent),
            PrivateMode::MouseUtf8 => encoding_state(self.cold.mouse_encoding, MouseEncoding::Utf8),
            PrivateMode::MouseSgr => encoding_state(self.cold.mouse_encoding, MouseEncoding::Sgr),
            PrivateMode::MouseUrxvt => {
                encoding_state(self.cold.mouse_encoding, MouseEncoding::Urxvt)
            }
            PrivateMode::MouseSgrPixels => {
                encoding_state(self.cold.mouse_encoding, MouseEncoding::SgrPixels)
            }
            // xterm answers the flag (`MdBool(screen->alternateScroll)`,
            // `[[source: xterm-misc]]`), which here is set at power-on.
            PrivateMode::AlternateScroll => match self.cold.alternate_scroll {
                AlternateScroll::On => ModeState::Set,
                AlternateScroll::Off => ModeState::Reset,
            },
            // The alternate screen is a whole buffer rather than a flag, and it
            // is implemented, so `DECRQM` must not deny it.
            //
            // `?47` answers the same, from which buffer is shown, whichever
            // mode switched it. That is baud's choice: the pinned
            // `xterm-charproc.c` records both modes from `whichBuf` in
            // `savemodes`, which is XTSAVE, and xterm's `DECRQM` handler is
            // not pinned.
            PrivateMode::AltScreen | PrivateMode::AltScreenLegacy => match self.cold.screen {
                Screen::Alternate => ModeState::Set,
                Screen::Normal => ModeState::Reset,
            },
            // **`?1020` is not answered here and that is deliberate.** It
            // mirrors the live input encoding, which lives on `Term` rather
            // than on `Paint` -- the per-byte loop carries the encoding as a
            // local and never reads it back. Threading a field through `Paint`
            // to answer one query is the trade this file prices at 5-10%, so
            // the honest answer is `NotRecognized` rather than a value read
            // from the wrong place. `baud-core2` answers it; the difference is
            // recorded rather than papered over.
            //
            // `?2027`: this core clusters per `[[source: uax29]]` and has no
            // mode that stops it, so `1` would invite an application to try
            // turning it off. `3` is the value DECRPM has for exactly this,
            // and it is true: a `U+FE0F` widens its base (`Paint::attach`).
            // Configured to one cell, the answer is `4`: neither is an answer
            // an application can change, because bubbletea v2 resets `?2027`
            // on exit, behind the shell.
            PrivateMode::GraphemeClustering => match self.cold.vs16 {
                Vs16Width::Two => ModeState::PermanentlySet,
                Vs16Width::One => ModeState::PermanentlyReset,
            },
            _ => ModeState::NotRecognized,
        }
    }
}
