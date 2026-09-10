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
use crate::storage::NSTYLE;
use crate::term::{Saved, Screen};
use baud_domain::*;

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
            style: self.style,
            style_raw: self.style_raw,
            flags: self.flags,
            g: self.cold.g,
            gl: self.cold.gl,
            autowrap: ModeChange::of(self.cold.modes.is_set(ModeBit::AutoWrap)),
            origin: ModeChange::of(self.cold.modes.is_set(ModeBit::Origin)),
        }
    }

    /// **Clamped, because the save may predate a resize.** An unclamped
    /// restore puts the cursor outside the grid, and writes and erases index
    /// with it.
    pub(crate) fn restore(&mut self, s: Option<Saved>) {
        let Some(s) = s else { return };
        let r = Row::from_index(s.cy.index().min(self.rows.index() - 1));
        self.goto(r, Col::at_index(s.cx.index().min(self.cols.index() - 1)));
        self.style = s.style;
        self.style_raw = s.style_raw;
        self.flags = s.flags;
        self.cold.g = s.g;
        self.cold.gl = s.gl;
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

    // ---- scrolling ------------------------------------------------------

    /// **The full-screen case rotates and the region case copies**, which is
    /// the split this repo already measured at 45%: a scroll that moves an
    /// index costs one row of blanking where one that moves the rows costs the
    /// whole region.
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
                self.view.advance_rot(1);
                self.fill_row_counted(Row::from_index(last));
            }
            return;
        }
        // **A region anchored at the top is a rotation plus a repair**, and the
        // repair is the rows *outside* it rather than the rows inside.
        //
        // The shape every status-line application has -- `less`, `vim`, `htop`,
        // `tmux` -- is `DECSTBM 1 ; rows-1`, so the region is the whole screen
        // but one line. Copying it moves forty-nine rows a scroll; rotating and
        // putting the one row below the region back moves **one**.
        //
        // Found by concatenating the corpus into one stream: a program that
        // sets a region and does not reset it leaves the next one scrolling by
        // copy, and the mixed stream took **89,209 scrolls, 100% of them
        // copying**, at 201 MB/s against the 835 the recordings predict
        // individually. Every recording measured alone shows zero, because
        // each starts from a fresh terminal -- which is this repo's "payloads
        // that don't exercise the case" rule, found at last.
        // **A region anchored at the top is a rotation plus a repair**, and
        // the repair is the rows *outside* it rather than the rows inside.
        //
        // The shape every status-line application has -- `less`, `vim`,
        // `htop`, `tmux` -- is `DECSTBM 1 ; rows-1`, so the region is the whole
        // screen but one line. Copying it moves forty-nine rows a scroll;
        // rotating and putting the one row below the region back moves **one**.
        let below = last - bot.index();
        if top.index() == 0 && below < bot.index() + 1 - top.index() {
            #[cfg(feature = "diag")]
            {
                self.scroll_rot += k;
            }
            for _ in 0..k {
                self.view.advance_rot(1);
                // **Descending, so a row is read before it is overwritten.**
                for y in (bot.index() + 1..=last).rev() {
                    self.view
                        .copy_row(Row::from_index(y - 1), Row::from_index(y));
                }
                self.fill_row_counted(bot);
            }
            self.cells = self.cells.plus(k * below * self.cols.index());
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
        let to = Col::at_index(self.cols.index());
        self.view.delete_cells(r, at, n, to, b);
        self.cells = self.cells.plus(self.cols.index() - at.index());
        self.row_changed_to(r);
    }

    /// ICH: push the rest of the row right over `n` blanks.
    pub(crate) fn insert_blanks(&mut self, n: Count) {
        let (r, at, b) = (self.cy, self.cx, self.blank());
        let to = Col::at_index(self.cols.index());
        self.view.insert_cells(r, at, n, to, b);
        self.cells = self.cells.plus(self.cols.index() - at.index());
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

    /// **A bounded, evict-on-collision table**, the same contract `StyleWord`
    /// carries in `baud-core2`. A growable one is the content-addressing
    /// failure mode this repo already measured at -18%.
    fn intern_style(&mut self, raw: StyleWord) -> StyleId {
        #[cfg(feature = "probe-double-intern")]
        core::hint::black_box(StyleId::bucket(raw, NSTYLE));
        // **The whole intern, not just the hash.** `probe-double-intern`
        // doubles `bucket` only and reads flat, which prices the multiply and
        // says nothing about the 24-byte store -- and the store is what a
        // lazy intern would skip. Storing twice to the same slot is
        // idempotent, so every counter is unchanged.
        #[cfg(feature = "probe-double-intern-full")]
        {
            let k2 = StyleId::bucket(raw, NSTYLE);
            self.tmpl[k2] = Some(raw);
        }
        let k = StyleId::bucket(raw, NSTYLE);
        // **Stored unconditionally, and comparing first was measured flat.**
        // `Option<StyleWord>` is 24 bytes and `btop.raw` sends ~20,000 SGRs a
        // rep, so skipping the store on a hit looked like obvious duplicate
        // work -- `baud-core2` does exactly that. It is worth nothing here:
        // btop 814-818 against 806-811, asciiquarium and vim overlapping. The
        // slot is already in L1 and already dirty, so the store is free and
        // the compare is not.
        self.tmpl[k] = Some(raw);
        StyleId::from_index(k)
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
            _ => {
                let x = self.cx.index();
                if x < self.tabs.len() {
                    self.tabs[x] = false;
                }
            }
        }
    }

    // ---- margins and columns -------------------------------------------

    /// DECSLRM. **Recorded and not acted on** would be the failure this repo
    /// names `parsed`; the margins bound `insert_cells`/`delete_cells` below.
    pub(crate) fn set_side_margins(&mut self, _left: OneBased, _right: OneBased) {
        // Left/right margins need DECLRMM set to mean anything, and this core
        // does not record them yet -- so the sequence is counted rather than
        // half-applied. A margin stored and ignored confines nothing and
        // explains nothing.
    }

    pub(crate) fn insert_columns(&mut self, n: Count) {
        let (b, to) = (self.blank(), Col::at_index(self.cols.index()));
        for y in self.cold.top.index()..=self.cold.bot.index() {
            self.view
                .insert_cells(Row::from_index(y), self.cx, n, to, b);
            self.cells = self.cells.plus(self.cols.index() - self.cx.index());
        }
    }

    pub(crate) fn delete_columns(&mut self, n: Count) {
        let (b, to) = (self.blank(), Col::at_index(self.cols.index()));
        for y in self.cold.top.index()..=self.cold.bot.index() {
            self.view
                .delete_cells(Row::from_index(y), self.cx, n, to, b);
            self.cells = self.cells.plus(self.cols.index() - self.cx.index());
        }
    }

    // ---- modes ----------------------------------------------------------

    pub(crate) fn apply_private_modes(
        &mut self,
        csi: &crate::write::Csi,
        change: ModeChange,
        node: &mut crate::graph::Node,
    ) {
        // **Every parameter, not the first.** `CSI ? 1 ; 3 ; 4 ; 6 l` sets
        // four modes and rxvt's own reset string is exactly that; reading one
        // left the rest silently unset.
        csi.for_each_param(|p| self.set_private_mode(PrivateMode::from_wire(p), change, node));
    }

    fn set_private_mode(
        &mut self,
        m: PrivateMode,
        change: ModeChange,
        node: &mut crate::graph::Node,
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
            // **`?12` changes whether the cursor blinks, not what shape it
            // is.** Blink is a property of the shape here -- `BlinkingBar` and
            // `SteadyBar` are one cursor in two states -- so it folds into the
            // style rather than setting a bit beside it, and `DECSCUSR`'s
            // shape survives the toggle. A separate bit would let the two
            // disagree about the same cursor.
            PrivateMode::CursorBlink => {
                self.cold.cursor_style = self.cold.cursor_style.with_blink(on);
            }
            // **`?2004` and `?2026` are the embedder's, and exposing them is
            // acting on them.** Bracketing a paste is the input path's job and
            // holding a frame is the renderer's; this core has neither, so
            // recording the request and answering `DECRQM` truthfully is the
            // whole of the terminal's part.
            PrivateMode::BracketedPaste => self.cold.modes.apply(ModeBit::BracketedPaste, on),
            PrivateMode::SynchronizedOutput => {
                self.cold.modes.apply(ModeBit::SynchronizedOutput, on)
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
            PrivateMode::CursorVisible => self.cold.modes.apply(ModeBit::CursorVisible, on),
            PrivateMode::ReverseVideo => self.cold.modes.apply(ModeBit::ReverseVideo, on),
            // **Mouse tracking is a level, not three flags.** `[[source:
            // ctlseqs]]` has xterm treat each DECSET as *selecting* one, and
            // `?1003` is a superset of `?1002` which is a superset of `?1000`
            // -- so resetting any of them turns tracking off, which is what an
            // application expects from `?1000 l`.
            PrivateMode::MouseNormal => self.set_tracking(MouseTracking::Normal, change),
            PrivateMode::MouseButtonEvent => self.set_tracking(MouseTracking::ButtonEvent, change),
            PrivateMode::MouseAnyEvent => self.set_tracking(MouseTracking::AnyEvent, change),
            PrivateMode::MouseSgr => {
                // Resetting `?1006` returns to X10, the protocol's default
                // rather than "off" -- there is no off.
                self.cold.mouse_encoding = match on {
                    true => MouseEncoding::Sgr,
                    false => MouseEncoding::X10,
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
            PrivateMode::AltScreen => self.set_alt_screen(change),
            _ => {}
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
    }

    /// **`ModeChange`, not a `bool`.** The two states already have a name in
    /// the protocol, and this repo's audit exists to stop that name being
    /// thrown away at a call site -- `set_alt_screen(true)` reads as nothing
    /// in particular where `ModeChange::Set` reads as DECSET.
    fn set_alt_screen(&mut self, change: ModeChange) {
        use crate::term::Screen;
        match (change == ModeChange::Set, self.cold.screen) {
            (true, Screen::Normal) => {
                self.cold.alt_saved = Some(self.save());
                self.cold.screen = Screen::Alternate;
                // **Park the primary before anything clears it.** This is the
                // whole of the second buffer: the grid is handed to the
                // alternate screen and the primary waits in `AltBuf` until the
                // exit brings it back. Cells, extents and rotation go together
                // in one call so two of the three cannot travel.
                // **Counted, because it is write traffic to the grid.** This
                // file records a region copy that counted only the blanks it
                // left and so undercounted by the height of the region;
                // parking is the same shape, one screen at a time. Both arms
                // of any A/B must attribute it or the counter stops being the
                // check that two arms did equal work.
                let (rot, n) = self.view.save_to(self.alt.cells, self.alt.used);
                self.alt.rot = rot;
                self.cells = self.cells.plus(n);
                // The alternate screen is cleared on entry, so a rotation left
                // from the last time an application was in it describes
                // nothing. Blanks rotate to blanks, so this is tidiness rather
                // than a fix -- and an invariant that holds only by accident is
                // one the next change breaks.
                self.view.reset_rot();
                self.clear_screen();
                self.goto(Row::ZERO, Col::ZERO);
            }
            (false, Screen::Alternate) => {
                self.cold.screen = Screen::Normal;
                // Cells first, then the cursor: `restore` clamps against the
                // geometry and moves the cursor, and it should be moving over
                // the screen the application is about to see.
                let n = self
                    .view
                    .restore_from(self.alt.cells, self.alt.used, self.alt.rot);
                self.cells = self.cells.plus(n);
                let s = self.cold.alt_saved.take();
                self.restore(s);
            }
            _ => {}
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
        self.soft_reset();
        // **`RIS` returns to the normal screen and `DECSTR` does not**, which
        // is why this is here rather than in `soft_reset`.
        self.set_alt_screen(ModeChange::Reset);
        // **What `RIS` resets and `DECSTR` does not.** `soft_reset` leaves
        // mouse tracking and the keypad alone deliberately -- a full-screen
        // application recovering with `CSI ! p` must not have them pulled out
        // from under it -- and a power-on reset has no such caller to protect.
        self.cold.mouse = MouseTracking::Off;
        self.cold.mouse_encoding = MouseEncoding::X10;
        self.cold.highlight = HighlightTracking::Off;
        self.cold.keypad = KeypadMode::Numeric;
        self.cold.terminal = TerminalMode::Ansi;
        self.cold.vt52_row = Vt52Coord::ZERO;
        // The open hyperlink is content rather than mode: a `RIS` clears the
        // screen, so a link that outlived it would apply to cells that no
        // longer exist.
        self.link = LinkId::NONE;
        self.refresh_flags();
        self.view.reset_rot();
        self.clear_screen();
        // **Here and not in `clear_screen`, and the difference is the
        // alternate screen.** Entering it clears the grid while the primary's
        // cells wait in `AltBuf` still naming their clusters, so a reset there
        // would lose exactly the marks the parking exists to keep. Nothing
        // overwrites them meanwhile: the store *refuses* a colliding cluster
        // rather than evicting one, so the alternate screen can be denied a
        // slot but cannot take an occupied one. `RIS` is the one point where
        // every cell really is gone.
        self.clusters.reset();
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
        self.cold.modes = Modes::default();
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
        // not checked against it.
        self.cold.cursor_style = CursorStyle::BlinkingBlock;
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
            PrivateMode::BracketedPaste => bit_state(self.cold.modes, ModeBit::BracketedPaste),
            PrivateMode::SynchronizedOutput => {
                bit_state(self.cold.modes, ModeBit::SynchronizedOutput)
            }
            // Not a `Modes` bit -- the cursor shape is a value, and whether it
            // blinks is a property of the shape rather than a mode of its own.
            PrivateMode::CursorBlink => match self.cold.cursor_style.blinks() {
                true => ModeState::Set,
                false => ModeState::Reset,
            },
            PrivateMode::AnsiMode => match self.cold.terminal {
                TerminalMode::Ansi => ModeState::Set,
                TerminalMode::Vt52 => ModeState::Reset,
            },
            PrivateMode::HighlightTracking => match self.cold.highlight {
                HighlightTracking::On => ModeState::Set,
                HighlightTracking::Off => ModeState::Reset,
            },
            PrivateMode::MouseNormal => tracking_state(self.cold.mouse, MouseTracking::Normal),
            PrivateMode::MouseButtonEvent => {
                tracking_state(self.cold.mouse, MouseTracking::ButtonEvent)
            }
            PrivateMode::MouseAnyEvent => tracking_state(self.cold.mouse, MouseTracking::AnyEvent),
            PrivateMode::MouseSgr => match self.cold.mouse_encoding {
                MouseEncoding::Sgr => ModeState::Set,
                MouseEncoding::X10 => ModeState::Reset,
            },
            // The alternate screen is a whole buffer rather than a flag, and it
            // is implemented, so `DECRQM` must not deny it.
            PrivateMode::AltScreen => match self.cold.screen {
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
            // turning it off. `3` is the value DECRPM has for exactly this.
            PrivateMode::GraphemeClustering => ModeState::PermanentlySet,
            _ => ModeState::NotRecognized,
        }
    }
}
