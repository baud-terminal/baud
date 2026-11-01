//! What a control sequence does to the grid.
//!
//! **Every one of these delegates to `View`.** `baud-domain` already owns
//! `insert_lines_in`, `delete_lines_in`, `delete_cells`, `insert_cells`,
//! `fill` and `fill_row`, and `baud-core2` reached the same primitives; a
//! second spelling of a grid operation here is the "one write path knowing a
//! rule the other did not" failure this repo records four times. What lives in
//! this file is the *state* -- the region, the modes, the saved cursor -- and
//! the arithmetic that turns a parameter into a span.
//!
//! Out of the per-byte loop on purpose: the write path calls none of it, and
//! this repo has measured that a large `match` inlined into a per-byte body
//! costs every payload whether or not it executes.

use crate::paint::{Paint, Pen};
use crate::term::{AltStyle, Saved, Screen, Strip, SurfaceLine};
use baud_domain::*;

/// What a rendition with SAPV's variants found on its own key's path
/// ([`Paint::variants_probe`]).
enum Probed {
    Hit(StyleId),
    Empty(StyleId),
    Full,
}

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
    /// `?1047`: the buffer, and on the way out the alternate screen cleared
    /// first, xterm's `srm_OPT_ALTBUF` (`[[source: xterm-reset]]`'s file).
    ClearOnExit,
}

/// Whether a synchronised update (`?2026`) is open, for where a `?5` change is
/// counted: a frame could show one made while it is released, and the
/// application asked for one made while it is held not to be seen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Update {
    Released,
    Held,
}

/// What `REP` wrote: its copies, or nothing, when there was no graphic
/// character to repeat. `csi_final` moves the parser to `Node::Ground` only
/// after copies, since only they told the cluster machine anything.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wrote {
    Nothing,
    Copies,
}

impl Paint<'_, '_> {
    /// Where row 1 is. DECOM makes the scrolling region's top the origin.
    pub(crate) fn origin_row(&self) -> Row {
        match self.cold.modes.is_set(ModeBit::Origin) {
            true => self.cold.top,
            false => Row::ZERO,
        }
    }

    /// Where column 1 is: **the left margin under DECOM**.
    /// `[[source: dec-std-070]]` DECOM (p. 5-31) makes addressing "relative
    /// to ... the origin of the current scrolling region (Top Margin, Left
    /// Margin)". The first implementation displaced only the row, so under
    /// side margins a `CUP` meant for inside the fence landed left of it.
    pub(crate) fn origin_col(&self) -> Col {
        match self.cold.modes.is_set(ModeBit::Origin) {
            true => self.cold.left,
            false => Col::ZERO,
        }
    }

    /// `CUP`'s column `c`, 0-based, from the origin: under DECOM
    /// `(left_margin - 1) + x`, and the right margin where that passes it
    /// (`[[source: dec-std-070]]` CUP, p. 5-50); absolute otherwise, and
    /// placed one past a narrower right margin ([`Paint::place`]). Under
    /// DECOM it stops at the right margin, so it is never placed; the
    /// encode beside DECOM's test reads the mode byte that test loaded.
    /// Inline, as it was before it returned a [`CursorCol`]: out of line it
    /// was a call a `CUP`.
    #[inline(always)]
    pub(crate) fn address_col(&self, c: Col) -> CursorCol {
        match self.cold.modes.is_set(ModeBit::Origin) {
            true => CursorCol::moved(Col::at_index(
                (self.cold.left.index() + c.index()).min(self.cold.right.index()),
            )),
            false => self.place(c),
        }
    }

    /// **The page origin obeying DECOM**, where `?6`, `DECSTBM` and
    /// `DECSLRM` put the cursor. DECSTBM note 1 and DECSLRM note 1
    /// (`[[source: dec-std-070]]` pp. 5-25 and 5-27): "First column of first
    /// line if Origin Mode is in the reset (Absolute) state; Top and Left
    /// Margins if Origin Mode is in the set (Displaced) state." `DECSTBM`
    /// put the cursor on the region's top row with DECOM reset until the V10
    /// oracle found it. **A cursor movement, so it ends an aligned string**
    /// (`crate::aligned`): DECSTBM's, DECSLRM's, DECOM's, DECCOLM's, NP's,
    /// PP's and VT52's home.
    pub(crate) fn home(&mut self) {
        self.effector_ends_string();
        self.goto(self.origin_row(), self.origin_col());
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

    /// **And the column**: under DECOM a column lands between the left and
    /// right margins. `[[source: dec-std-070]]` DECOM (p. 5-31): "the Active
    /// position cannot be moved above the Top Margin or left of the Left
    /// Margin"; `[[source: vt510-decom]]`: "The cursor cannot move outside of
    /// the margins." The first left-margin origin confined only `CUP`, so
    /// `CR`, `CUB` or `CHA` left the cursor left of the origin and `CPR`
    /// answered the origin's column for it. Every horizontal move but a write
    /// and `BS` (which stops at the left margin by itself) comes through here,
    /// through the line start of [`Paint::return_fenced`] or through
    /// [`Paint::origin_col`]; not inside `set_col`, which every write's move
    /// takes too.
    pub(crate) fn confine_col(&self, c: Col) -> Col {
        match self.cold.modes.is_set(ModeBit::Origin) {
            false => c,
            true => Col::at_index(
                c.index()
                    .clamp(self.cold.left.index(), self.cold.right.index()),
            ),
        }
    }

    /// **`CR`, and every line start built on it**: `NEL`, `CNL`, `CPL` and
    /// `LF` under `LNM`. One bit of the mode word inline: **a left margin
    /// past column 0 is set only under `DECLRMM`** -- `DECSLRM` parses only
    /// then, and `?69 l`, `DECCOLM`, `DECSTR`, `RIS` and a resize put the
    /// margins back wherever the bit goes -- so with it reset every line
    /// starts at column 0, DECOM or not, and nothing else is read. A mask of
    /// both bits, `0x24`, is no logical immediate on arm64: it cost a `mov`
    /// and a `tst` where one `tbnz` stands, 2.4 M instructions over 100 reps
    /// of `cat`.
    ///
    /// **Under DCSM's PRESENTATION** the column chosen is the line's home
    /// in the presentation, and the cursor goes to the cell drawn there
    /// (`crate::component`).
    /// Tested first, out of line, so nothing of it is live across the
    /// column's store: kept live, it took a register the loop around `CR`
    /// had, and `less` retired 0.5% more instructions.
    #[inline(always)]
    pub(crate) fn carriage_return(&mut self) {
        let modes = self.cold.modes;
        if modes.is_set(ModeBit::ReturnMapped) {
            return self.return_presented();
        }
        match modes.is_set(ModeBit::SideMargins) {
            false => self.set_col(CursorCol::moved(Col::ZERO)),
            true => self.return_fenced(),
        }
    }

    /// `CR` under `DECLRMM`. `[[source: dec-std-070]]` CR, p. 5-58:
    /// "The CR control moves the Active Position to the Left Margin. If the
    /// Active Position is before the left margin, or outside the top and
    /// bottom margins, the cursor moves to the first column of the Active
    /// Line"; its algorithm tests `column >= left_margin` and the line
    /// between the top and bottom margins. xterm's `CarriageReturn`
    /// (`[[source: xterm-cursor-c]]` 345-371) and ghostty's
    /// (`[[source: ghostty-d9840f3-terminal]]` 1822-1833) test the column
    /// only: their departure. Under DECOM the cursor cannot be outside the
    /// margins (p. 5-31), so it goes to the left margin with no test.
    ///
    /// Where the line starts past column 0 the cluster machine is told a
    /// control went past, as `Act::EndCluster` tells it after every other
    /// control: `CR` leads to the ground because at column 0 `glyph` restarts
    /// the machine itself, and at a margin past it a continuation would find
    /// a cell before it and join it across the control.
    #[cold]
    #[inline(never)]
    pub(crate) fn return_fenced(&mut self) {
        let (x, y) = (self.cx.col().index(), self.cy.index());
        let left = self.cold.left;
        let inside = self.cold.modes.is_set(ModeBit::Origin)
            || (x >= left.index() && y >= self.cold.top.index() && y <= self.cold.bot.index());
        let home = match inside {
            true => left,
            false => Col::ZERO,
        };
        if home != Col::ZERO {
            self.cluster = ClusterBreak::after_control();
        }
        self.set_col(CursorCol::moved(home));
    }

    /// **`CUF`: the right margin stops a cursor at or inside it.**
    /// `[[source: dec-std-070]]` CUF, p. 5-45, note 1: "If the Active
    /// Position is at or inside the Right Margin ... The Active Position will
    /// not move beyond the Right Margin"; note 2: "If the Active Position is
    /// beyond the Right Margin ... the control will be executed until the
    /// Active Position reaches the last column of the Active Line." No row is
    /// tested (the algorithm, p. 5-46), and xterm's `CursorForward`
    /// (`[[source: xterm-cursor-c]]` 211-241) and ghostty's `cursorRight`
    /// (`[[source: ghostty-d9840f3-terminal]]` 1884-1895) agree.
    /// `[[source: vt510-cuf]]` says "the right border of the page", which
    /// STD 070's notes refine.
    ///
    /// **A pending wrap is on the right margin**, as in `Paint::tab_end`:
    /// with `DECAWM` set the sentinel one past it reads as on it, so a `CUF`
    /// there stays on the margin; with it reset one past is beyond.
    ///
    /// **One bit inline, the margins out of line**, as for `CR`: with
    /// `DECLRMM` reset the margins are the page's edges
    /// ([`Paint::carriage_return`]), both notes stop at the last column, and
    /// DECOM's clamp ([`Paint::confine_col`]) to them is no clamp, so the bit
    /// replaces DECOM's test rather than adding one. Inline, the margin test
    /// was 2.4 M instructions over 100 reps of `btop`, whose 6,150 `CUF`s and
    /// 1,863 `CUB`s a rep are all at full width.
    ///
    /// **A cursor a move placed one past the margin is beyond it** (note
    /// 2), and only the sentinel a write leaves there reads as on it
    /// ([`Paint::at_or_inside_right`]).
    ///
    /// **And a double line's half**: with `DECLRMM` reset the fence is the
    /// last column of the cursor's line, so `CUF` stops at it at no cost
    /// over stopping at the screen's (`[[source: dec-std-070]]` CUF, p. 5-46,
    /// `column < end_of_line`). Only a move that reaches it is checked, out
    /// of line, against a fence a sized line left ([`Paint::line_fence`]).
    #[inline(always)]
    pub(crate) fn forward_in_margins(&self, n: Count) -> CursorCol {
        match self.cold.modes.is_set(ModeBit::SideMargins) {
            false => {
                let to = self.cx.raw().forward_to(n, self.cold.fence);
                match to == self.cold.fence {
                    false => CursorCol::moved(to),
                    true => self.forward_to_line_end(n),
                }
            }
            true => self.forward_fenced(n),
        }
    }

    /// `CUF` that reached the fence, to the end of the cursor's line.
    #[cold]
    #[inline(never)]
    fn forward_to_line_end(&self, n: Count) -> CursorCol {
        CursorCol::moved(self.moved_from().forward_to(n, self.line_fence()))
    }

    #[cold]
    #[inline(never)]
    fn forward_fenced(&self, n: Count) -> CursorCol {
        let x = self.moved_from().index();
        let end = match self.at_or_inside_right() {
            true => self.cold.right.index(),
            false => self.cols.index() - 1,
        };
        let c = self.confine_col(Col::at_index((x + n.at_least_one()).min(end)));
        CursorCol::placed(c, self.cold.right, self.cols)
    }

    /// **Whether the cursor is at or inside the right margin**, for the
    /// moves whose notes split there (`CUF`, `HT`): a column at or left of
    /// it, or the sentinel one past it with `DECAWM` set, a pending wrap,
    /// which reads as on it. A cursor a move placed one past it, the
    /// sentinel past the screen's last column, and any column right of it
    /// are beyond; with `DECAWM` reset no wrap is pending, so one past is
    /// beyond too.
    pub(crate) fn at_or_inside_right(&self) -> bool {
        match self.cursor_at() {
            CursorAt::Column(c) => c <= self.cold.right,
            CursorAt::Pending(c) => {
                c.index() == self.cold.right.index() + 1
                    && self.cold.modes.is_set(ModeBit::AutoWrap)
            }
            CursorAt::Placed(_) => false,
        }
    }

    /// **`CUB`: the left margin stops a cursor at or inside it.**
    /// `[[source: dec-std-070]]` CUB, p. 5-47, note 1: "If the Active
    /// Position is at or inside the Left Margin ... The Active position will
    /// not move beyond the Left Margin"; note 2: from before it, "until the
    /// Active Position reaches the first column of the Active Line". No row
    /// is tested (p. 5-48), as xterm's `CursorBack` does not
    /// (`[[source: xterm-cursor-c]]` 148-149: "if the cursor is already
    /// before the left-margin, we have to let it go"). Gated on `DECLRMM`
    /// as `CUF` is.
    ///
    /// From right of the fence it can land one past the right margin, and
    /// is placed there.
    #[inline(always)]
    pub(crate) fn back_in_margins(&self, n: Count) -> CursorCol {
        match self.cold.modes.is_set(ModeBit::SideMargins) {
            false => CursorCol::moved(self.cx.raw().back(n)),
            true => self.back_fenced(n),
        }
    }

    #[cold]
    #[inline(never)]
    fn back_fenced(&self, n: Count) -> CursorCol {
        let x = self.cx.col().index();
        let left = self.cold.left.index();
        let floor = match x >= left {
            true => left,
            false => 0,
        };
        let c = self.confine_col(Col::at_index(x.saturating_sub(n.at_least_one()).max(floor)));
        CursorCol::placed(c, self.cold.right, self.cols)
    }

    /// **`CUU`: the top margin stops a cursor at or below it, DECOM or
    /// not.** `[[source: dec-std-070]]` CUU, p. 5-41, note 1: "If the Active
    /// Position is at or below the Top Margin ... The Active Position will
    /// not move beyond the Top Margin"; note 2: from above it, "until the
    /// Active Position reaches the first line of the Page". The algorithm
    /// (p. 5-42) tests `line >= top_margin` and no mode, so a cursor below
    /// the region crosses it to the top margin. xterm's `CursorUp`
    /// (`[[source: xterm-cursor-c]]` 271-285) and ghostty's `cursorUp`
    /// (`[[source: ghostty-d9840f3-terminal]]` 1849-1862) agree. This
    /// confined only under DECOM, so a region alone let `CUU` out of it.
    /// DECOM's clamp follows, as before, for a cursor it finds outside.
    pub(crate) fn up_in_margins(&self, n: Count) -> Row {
        let to = self.cy.up(n);
        self.confine(match self.cy >= self.cold.top {
            true => to.max(self.cold.top),
            false => to,
        })
    }

    /// **`CUD`: the bottom margin stops a cursor at or above it**, the
    /// mirror: `[[source: dec-std-070]]` CUD, p. 5-43, notes 1 and 2, "the
    /// bottom line of the Page" from below it; the algorithm (p. 5-44) tests
    /// `line <= bottom_margin`. xterm's `CursorDown` (251-264) and ghostty's
    /// `cursorDown` (1867-1878) agree.
    pub(crate) fn down_in_margins(&self, n: Count) -> Row {
        let to = self.cy.down(n, self.rows);
        self.confine(match self.cy <= self.cold.bot {
            true => to.min(self.cold.bot),
            false => to,
        })
    }

    pub(crate) fn save(&self) -> Saved {
        Saved {
            cy: self.cy,
            cx: self.cx.col(),
            style_raw: self.kept_rendition(),
            flags: self.flags,
            g: self.cold.g,
            gl: self.cold.gl,
            gr: self.cold.gr,
            desig: self.cold.desig,
            protect: self.cold.protect,
            autowrap: ModeChange::of(self.cold.modes.is_set(ModeBit::AutoWrap)),
            origin: ModeChange::of(self.cold.modes.is_set(ModeBit::Origin)),
            // The sentinel, and not a cursor a move placed on its column.
            pending: ModeChange::of(
                !self.cx.is_placed() && self.cx.col().index() == self.line_fence().index() + 1,
            ),
            widths: self.cold.widths.widths(),
        }
    }

    /// `?1049 l`'s restore, made on the primary: "restore cursor as in
    /// DECRC" (`[[source: ctlseqs]]`), so the primary's slot, which it does
    /// not empty, or with nothing saved what `DECRC` then does, xterm's
    /// `CursorRestore` after `FromAlternate`. Then the style and `?12`
    /// `?1049 h` saved beside it, which `DECRC` leaves alone.
    fn restore_alt(&mut self) {
        self.save_or_restore_cursor(ModeChange::Reset);
        if let Some(a) = self.cold.alt_style.take() {
            self.cold.cursor_style = a.style;
            self.cold.cursor_blink = a.blink;
        }
    }

    /// **Clamped, because the save may predate a resize.** An unclamped
    /// restore puts the cursor outside the grid, and writes and erases index
    /// with it.
    ///
    /// **Under DECOM the restored position is confined**, after the saved
    /// mode is back: `[[source: dec-std-070]]` `restore_cursor` (p. 5-190)
    /// clamps the line to the top and bottom margins and the column to the
    /// left and right when the restored Origin Mode is displaced. Its note 3 (p. 5-189),
    /// written before side margins, keeps "the stored column value"; the
    /// algorithm and DECOM's "cannot be moved ... left of the Left Margin"
    /// clamp it.
    pub(crate) fn restore(&mut self, s: Option<Saved>) {
        let Some(s) = s else { return };
        // A cursor movement, which ends an aligned string
        // (`Paint::effector_ends_string`).
        self.effector_ends_string();
        self.cold.widths = WidthPolicy::of(self.cold.widths.ambiguous(), s.widths);
        self.reselect();
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
        let r = Row::from_index(s.cy.index().min(self.rows.index() - 1));
        let c = Col::at_index(s.cx.index().min(self.cols.index() - 1));
        // **A pending wrap saved under side margins narrower than the screen
        // is restored pending** while the right margin is where it was: the
        // sentinel one past it is not a column the clamp should pull back, as
        // this restore kept it before the clamp and as xterm and ghostty keep
        // theirs. Saved at full width and restored with the margins at full
        // width, it is dropped by the clamp to the screen above, as before
        // the clamp; restored under a right margin one before the screen's
        // last column (`DECSLRM 1;19` on 20 columns), that clamp puts it on
        // the new right margin + 1 and it comes back pending, as xterm's
        // does, as can a `?1049` save a reflow moved (`Saved::pending`);
        // under any other moved right margin it comes back a column. Which
        // wrap state `DECRC` restores is note AE's Q12, open. **Only a saved
        // sentinel**: a column on the right margin that a narrower `DECSLRM`
        // made level with the new right margin + 1 is a column, and
        // `restore_cursor` (p. 5-190) clamps it.
        let keep = c.index() == self.cold.right.index() + 1 && s.pending == ModeChange::Set;
        let c = match keep {
            true => c,
            false => self.confine_col(c),
        };
        self.goto(self.confine(r), c);
        // **The kept sentinel comes back the sentinel**, not through
        // [`Paint::place`], which would make a saved pending wrap a cursor
        // placed on its column and write on from it; a saved placed cursor,
        // not pending, comes back placed through it.
        self.cx = match keep {
            true => CursorCol::written(c),
            false => self.place(c),
        };
        // Onto a double line, inside its half, as every move lands.
        self.cx = self.on_line(self.cx);
        // **Interned again, not the saved id.** An eviction between the save
        // and the restore can give the saved id's slot to another rendition,
        // and writing under the old id would draw in its colours. The
        // rendition is what was saved; the id was where it happened to be.
        // A hit, in the ordinary case.
        let raw = self.keeping_string(s.style_raw);
        self.style = self.intern_style(raw);
        self.style_raw = raw;
        self.rendition_changed();
        self.flags = s.flags;
        self.cold.g = s.g;
        self.cold.gl = s.gl;
        self.cold.gr = s.gr;
        self.cold.desig = s.desig;
        self.cold.protect = s.protect;
        self.invoke();
    }

    /// `DECRC` with nothing saved. `[[source: vt510-decrc]]`: "If nothing was
    /// saved by DECSC, then DECRC performs the following actions: Moves the
    /// cursor to the home position (upper left of screen). Resets origin mode
    /// (DECOM). Turns all character attributes off (normal setting). Maps the
    /// ASCII character set into GL, and the DEC Supplemental Graphic set into
    /// GR." `[[source: vt220-rm-ch4]]` § 4.7 ends the same list "the default
    /// character set mapping is established", so the four slots, GL and GR
    /// go back to this core's power-on sets ([`Paint::reset_sets`]), and not
    /// only G0.
    ///
    /// **The selective erase attribute goes off too**, which neither vt510's
    /// list nor `[[source: vt220-rm-ch4]]` § 4.7's names and the reset save
    /// buffer holds: `[[source: dec-std-070]]` clears the save buffer's
    /// `attribute.selective erase` to `FALSE` on reset (DECSC note 3,
    /// p. 5-187), and `[[source: vt220-rm-ch4]]` Table 4-10
    /// gives the state `DECSTR` saves as "Selective erase attribute bit write
    /// state Off". `DECSTR` and `RIS` leave `saved` empty here, so this is the
    /// state a `DECRC` after either must restore.
    ///
    /// **`DECAWM` stays**: 070's buffer has no wrap flag and vt510's list does
    /// not reset it. `SCORC` does not come here (`scorc_without_a_save_is_inert`,
    /// note AE's Q13).
    pub(crate) fn restore_unsaved(&mut self) {
        self.effector_ends_string();
        self.cold.modes.apply(ModeBit::Origin, false);
        self.goto(Row::ZERO, Col::ZERO);
        self.style_raw = self.keeping_string(StyleWord::DEFAULT);
        self.style = self.intern_style(self.style_raw);
        self.rendition_changed();
        self.cold.protect = CellFlags::NONE;
        self.reset_sets();
        self.invoke();
    }

    /// The current level's designations and invocations: what DECSTR,
    /// DECSCL, RIS and a DECRC with nothing saved establish.
    /// `[[source: dec-std-070]]` 070-4 p. 31, DECSTR: "In Use Table (gl,
    /// gr) | set to current level defaults / Designated Sets | set to
    /// current level defaults", and 070-3 §3.6.4: "The following
    /// assignments will apply to the power-on and soft reset states of the
    /// terminal". **Level 1's are ASCII in all four**
    /// ([`crate::term::LEVEL_ONE_SETS`]), Levels 2 to 4's the power-up
    /// sets ([`crate::term::POWER_UP_SETS`]). The texts differ on DECSCL
    /// 61: its algorithm calls `soft_terminal_reset()`, whose body opens
    /// `if (conformance level != LEVEL_1)`, the guard of DECSTR's note b
    /// ("ignored when operating at Level 1"), and so would leave the sets
    /// as they were, while DECSCL's note 3, "automatically places the
    /// terminal into a known state", and §3.6.4.1 give Level 1's; baud
    /// follows the two.
    pub(crate) fn reset_sets(&mut self) {
        // DECHEM with them: multinational, as DECSTR's table returns
        // DECNRCM, its opposite, to "Multinational set"
        // (`[[source: vt510-decstr]]`).
        self.cold.hebrew = HebrewEncoding::Multinational;
        (self.cold.g, self.cold.desig) = match self.cold.level {
            ConformanceLevel::One => (
                crate::term::LEVEL_ONE_SETS,
                crate::term::LEVEL_ONE_DESIGNATIONS,
            ),
            _ => (
                crate::term::POWER_UP_SETS,
                crate::term::POWER_UP_DESIGNATIONS,
            ),
        };
        self.cold.gl = GSlot::G0;
        self.cold.gr = GSlot::G2;
    }

    /// **What an erase leaves, and what a line scrolled or opened onto the
    /// screen holds**: an empty cell in the current text background with
    /// every other attribute cleared, or under DECECM set the screen
    /// background, the default rendition (`[[source: vt520-rm]]` p. 5-48,
    /// "Controls the background color used when text is erased or new text is
    /// scrolled on to the screen"; ED, EL and ECH "clear all character
    /// attributes", pp. 5-179 and 5-180). Reverse is an attribute and goes
    /// too, so the background is the colour word SGR 40-49 set, as xterm,
    /// ghostty, foot and alacritty erase. The mode is one bit of a word the
    /// erase's caller reads anyway; a blank is built once an erase or a line,
    /// never per byte, and in the default rendition it interns nothing.
    #[inline(always)]
    pub(crate) fn blank(&mut self) -> PackedCell {
        if self.style == StyleId::DEFAULT || self.cold.modes.is_set(ModeBit::EraseScreenBackground)
        {
            return PackedCell::empty_with(StyleId::DEFAULT);
        }
        PackedCell::empty_with(self.text_background())
    }

    /// The current background alone, interned: the rendition an erase
    /// leaves under DECECM reset. Out of line, behind the default test.
    #[cold]
    #[inline(never)]
    fn text_background(&mut self) -> StyleId {
        self.intern_style(StyleWord::DEFAULT.with_bg(self.style_raw.bg()))
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
    ///
    /// **Inlined into its callers**, as it was before the watch's count on
    /// the alternate screen: with the count's compare in that branch it went
    /// out of line, and `Paint::index`, the line feed at the bottom that
    /// `cat` spends its time in, paid a call a line for it.
    #[inline(always)]
    pub(crate) fn retire(&mut self) {
        if self.cold.screen != crate::term::Screen::Normal {
            if self.cold.watched == crate::term::Watched::Rows {
                self.cold.watch_uncounted(Scroll::Up, self.book.top);
            }
            if self.images.any_placed() || self.anchored() {
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

    /// Drop the primary's placements whose rows the ring no longer retains
    /// -- one whose last row, a kitty placement's last shown row, is before
    /// the oldest history line, or whose anchor is past the rows a shorter
    /// window cut -- and one on `blanked`.
    pub(crate) fn expire_primary_images(&mut self, blanked: Option<LineIdx>) {
        let b = &self.book;
        let lo = b.top.minus(b.hist).unwrap_or(LineIdx::FIRST);
        let hi = b.top.plus(self.rows).plus(b.hidden);
        self.images.expire(ImageScreen::Primary, lo, hi, blanked);
    }

    /// **The alternate screen's images after its window moved one row.** It
    /// keeps no history, so a row that leaves the window leaves the screen,
    /// and an image goes with the last of its rows; one on the row the scroll
    /// blanked goes too. Its top line is kept only while an image is placed
    /// or an SPA is open, which is when this runs: an open SPA's start is a
    /// line in its numbering.
    #[cold]
    #[inline(never)]
    pub(crate) fn alt_images_moved(&mut self, way: Scroll) {
        // **The status line keeps no history and moves none of the main
        // display's lines**: it reads as the alternate screen while it is
        // written (`status.rs`), so its scroll reaches here, and leaves the
        // images and their numbering as they are. Here, out of line, and not
        // in `retire`, whose shape the per-line scroll pays for: the build
        // with the test there retired 1.07% more instructions on cat-cjk.
        if self.on_status() {
            return;
        }
        let rows = self.rows;
        let blanked = match way {
            Scroll::Up => {
                self.book.alt_top = self.book.alt_top.plus(Rows::of_count(1));
                self.book.alt_top.plus(Rows::of_count(rows.index() - 1))
            }
            Scroll::Down => {
                match self.book.alt_top.minus(Rows::of_count(1)) {
                    Some(t) => self.book.alt_top = t,
                    None => {
                        self.images.shift(ImageScreen::Alternate, Rows::of_count(1));
                        self.guard_shift(ImageScreen::Alternate);
                        self.cold
                            .placement
                            .shift(ImageScreen::Alternate, Rows::of_count(1));
                    }
                }
                self.book.alt_top
            }
        };
        let top = self.book.alt_top;
        self.images
            .expire(ImageScreen::Alternate, top, top.plus(rows), Some(blanked));
        self.cold
            .placement
            .scrolled(ImageScreen::Alternate, (top, top.plus(rows)), Some(blanked));
    }

    /// **What the scrolls test before they move a line anchor**: an open
    /// SPA's start or a placed cursor (`placed.rs`), each a line of its
    /// screen's numbering. On the alternate screen the numbering is kept
    /// only while one of them or an image is there.
    #[inline(always)]
    pub(crate) fn anchored(&self) -> bool {
        self.cold.guard_open.is_some() || self.cold.placement.is_placed()
    }

    /// **A placed cursor after rows `[from, to]` of the live screen moved
    /// `k` `way`**, the full width: a region's scroll by copy, `IL`, `DL`, a
    /// top region's rows put back (`Placement::band_moved`). Out of line;
    /// the callers test that one is placed.
    #[cold]
    #[inline(never)]
    pub(crate) fn placement_band_moved(&mut self, from: Row, to: Row, k: Rows, way: BandMove) {
        let within = (Col::ZERO, Col::at_index(self.cols.index()));
        self.placement_moved_in(from, to, within, k, way);
    }

    /// The same, for a placement between the columns `within`.
    fn placement_moved_in(
        &mut self,
        from: Row,
        to: Row,
        within: (Col, Col),
        k: Rows,
        way: BandMove,
    ) {
        let (screen, top) = self.image_top();
        let lo = top.plus(Rows::of_count(from.index()));
        let hi = top.plus(Rows::of_count(to.index() + 1));
        self.cold
            .placement
            .band_moved(screen, (lo, hi), within, k, way);
    }

    /// **A placed cursor after the primary's window moved back**, a row by
    /// a scroll down or over a shell's block (`prompt.rs`): a placement on
    /// a row it let go of at the bottom left the screen, and one past what
    /// the history keeps is gone with its line. `blanked` is the row a
    /// scroll down takes back at the top, the newest line of the history
    /// brought back without its text. A line the history keeps is still a
    /// line: it can come back with its text over a shell's block. Out of
    /// line, on the scroll down, which `cat` never takes.
    #[cold]
    #[inline(never)]
    pub(crate) fn primary_placement_moved(&mut self, blanked: Option<LineIdx>) {
        let b = &self.book;
        let lo = b.top.minus(b.hist).unwrap_or(LineIdx::FIRST);
        let hi = b.top.plus(self.rows);
        self.cold
            .placement
            .scrolled(ImageScreen::Primary, (lo, hi), blanked);
    }

    /// An open SPA's start, on `screen`, after the window moved back over a
    /// row its numbering cannot count below line zero: it moves down a row
    /// with its text, as the images do (`ImageStore::shift`).
    #[cold]
    #[inline(never)]
    fn guard_shift(&mut self, screen: ImageScreen) {
        if let Some(g) = self.cold.guard_open.as_mut().filter(|g| g.screen == screen) {
            g.line = g.line.plus(Rows::of_count(1));
        }
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

    /// **The rows `[from, to]` under a top-anchored region, put back a row
    /// after its window move**, and their images with them by their
    /// anchors (`ImageStore::put_back_band`): unlike a band that scrolled,
    /// they never left their screen rows. Outlined, gated on `any_placed`
    /// as the band move is.
    #[cold]
    #[inline(never)]
    pub(crate) fn band_images_put_back(&mut self, from: Row, to: Row) {
        let (screen, top) = self.image_top();
        let lo = top.plus(Rows::of_count(from.index()));
        let hi = top.plus(Rows::of_count(to.index() + 1));
        self.images.put_back_band(screen, lo, hi, Rows::of_count(1));
    }

    /// **Side margins narrower than the screen**: then the scrolling region
    /// is the rectangle the four margins bound, and every vertical scroll is
    /// [`Paint::scroll_rect`]'s. `DECSLRM` admits only a left margin below
    /// the right, so full width is exactly both at the screen's edges.
    #[inline(always)]
    pub(crate) fn fenced(&self) -> bool {
        self.cold.left != Col::ZERO || self.cold.right.index() + 1 != self.cols.index()
    }

    /// **A vertical scroll of the rectangle**: rows `at` to the bottom margin,
    /// between the side margins, `n` rows `way` -- `SU`, `SD`, `IND`, `RI`,
    /// `LF` and a wrap at a margin, `IL` and `DL`, whenever [`Paint::fenced`].
    ///
    /// `[[source: dec-std-070]]` § 5.4.3, p. 5-23: "A scrolling region is
    /// defined as the area bounded by the top, bottom, left, and right
    /// margins ... All scrolling, both vertical and horizontal, is limited to
    /// this area", and its `scroll_up` and `scroll_down` move only the
    /// columns from the left margin to the right (§ 5.14.2-3, revision note
    /// 28). **Nothing goes to the history**: "Lines scrolled off the top of
    /// the scrolling region are lost", and part of a row is not a line --
    /// xterm keeps nothing from `scrollInMargins` either
    /// (`[[source: xterm-scroll]]`, `xtermScroll`). So no ring moves, on
    /// either screen, and the rows' content changed in place, which is what
    /// [`Term::row_moves`] tells a reader holding a selection.
    ///
    /// Out of line: no recording the corpus holds sets side margins, and the
    /// callers pay one test of [`Paint::fenced`] for it.
    #[cold]
    #[inline(never)]
    pub(crate) fn scroll_rect(&mut self, at: Row, n: Count, way: BandMove) {
        let (bot, left, right) = (self.cold.bot, self.cold.left, self.cold.right);
        if at.index() > bot.index() {
            return;
        }
        if self.on_status() {
            self.status_scrolled();
            return;
        }
        let b = self.blank();
        let w = self
            .view
            .scroll_rect(at, bot, left, right.after(), n, way, b);
        self.cells = self.cells.plus(w);
        self.cold.moved_in_place(at, bot, self.book.top);
        if self.images.any_placed() {
            let k = n.at_least_one().min(bot.index() + 1 - at.index());
            let (screen, top) = self.image_top();
            let lo = top.plus(Rows::of_count(at.index()));
            let hi = top.plus(Rows::of_count(bot.index() + 1));
            self.images.scroll_rect(
                screen,
                lo,
                hi,
                (left, right),
                self.cell_width,
                Rows::of_count(k),
                way,
            );
        }
        // An open SPA's start moves with the rectangle only when its column
        // is in it: the columns outside the side margins do not move.
        if self
            .cold
            .guard_open
            .is_some_and(|g| g.col >= left && g.col <= right)
        {
            let k = n.at_least_one().min(bot.index() + 1 - at.index());
            self.guard_band_moved(at, bot, Rows::of_count(k), way);
        }
        // And so does a placed cursor.
        if self.cold.placement.is_placed() {
            let k = n.at_least_one().min(bot.index() + 1 - at.index());
            self.placement_moved_in(at, bot, (left, right), Rows::of_count(k), way);
        }
        self.row_changed_to(self.cy);
    }

    /// Whether the cursor is between the side margins for `IL` and `DL`,
    /// which do "not work outside these margins" (`[[source: dec-std-070]]`
    /// p. 5-24). **A pending wrap is on the right margin**: the sentinel is
    /// one past it, and reading it as outside made `IL` at the end of a full
    /// row inert. A cursor a move *placed* at exactly right + 1 is outside
    /// ([`CursorCol::placed`]).
    fn within_side_margins(&self) -> bool {
        let x = self.cx.col().index();
        !self.cx.is_placed() && x >= self.cold.left.index() && x <= self.cold.right.index() + 1
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
    ///
    /// Inlined into both of its callers, the full-screen scroll down and
    /// its twin past the edge: called from two places it went out of line,
    /// and the scroll down paid a call a line for a body it had inlined.
    #[inline(always)]
    pub(crate) fn unretire(&mut self) {
        if self.cold.screen != crate::term::Screen::Normal {
            if self.cold.watched == crate::term::Watched::Rows {
                self.cold.watch_uncounted(Scroll::Down, self.book.top);
            }
            if self.images.any_placed() || self.anchored() {
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
            None => {
                if self.cold.watched == crate::term::Watched::Rows {
                    self.cold.watch_uncounted(Scroll::Down, self.book.top);
                }
                if self.images.any_placed() {
                    self.images.shift(ImageScreen::Primary, Rows::of_count(1));
                }
                if self.cold.guard_open.is_some() {
                    self.guard_shift(ImageScreen::Primary);
                }
                self.cold
                    .placement
                    .shift(ImageScreen::Primary, Rows::of_count(1));
            }
        }
        if self.cold.placement.is_placed() {
            self.primary_placement_moved(Some(self.book.top));
        }
        // A window over the history this took back to is on the live screen
        // from this row on (`Book::window_taken_back`).
        self.book.window_taken_back();
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
        // **Before every rotation**: under side margins the region is a
        // rectangle, the whole screen and a top region included, and a
        // rotation would move the rows outside it and retire them.
        if self.fenced() {
            self.scroll_rect(top, n, BandMove::Up);
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
            self.path_rows(Row::from_index(last + 1 - k), Row::from_index(last));
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
        // and never reaches here: it is a rectangle, scrolled above. The
        // alternate screen keeps no history, so there the cheaper path still
        // decides.
        //
        // **What it costs is the repair.** A short region leaves most rows
        // under it, and each scroll moves every one of them: `below` rows of
        // cells where the copy moved the region's. xterm, kitty and foot move
        // row *pointers* for the same scroll, so the lever for this shape is a
        // row indirection, which this repo measured once as not worth its
        // carrying cost on the corpus -- none of whose recordings has it.
        let below = last - bot.index();
        let height = bot.index() + 1 - top.index();
        let keeps = self.cold.screen == Screen::Normal;
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
            // **The rows under it stay, and the numbering moves.** A
            // selection numbered by the lines retired would be renumbered a
            // row up for each, onto text that is not its own, so the rows
            // under the region are tested against the watch on it as the
            // scroll finds them -- before it advances, which is how the
            // selection was numbered -- whatever region the read ends in.
            self.cold
                .under_top_region(bot, Rows::of_count(k), self.book.top);
            // The margin bell's line, if it lies under the region, is put
            // back too (`margin.rs`).
            if self.cold.bell.armed.is_some() {
                self.margin_bell_under(bot, Rows::of_count(k));
            }
            // **To the furthest the rows hold, on the primary.** A growth
            // without a re-cut shows the cells past a narrower edge as they
            // are, so the rows put back carry theirs (`View::copy_row`). The
            // alternate screen blanks what a growth reveals (`View::widened`),
            // so its cells past the edge are never shown and are not carried.
            let reach = match self.cold.screen {
                Screen::Normal => self.book.reach,
                Screen::Alternate => self.cols,
            };
            // A screen with nothing past its edge -- every screen the corpus
            // records, and one narrowed before anything was written past the
            // new edge -- runs the loop with nothing past the edge compiled
            // into it.
            match reach.index() > self.cols.index() {
                false => self.rotate_top_region::<false>(Rows::of_count(k), bot, reach),
                true => self.rotate_top_region_past_edge(Rows::of_count(k), bot, reach),
            }
            self.path_rows(Row::from_index(bot.index() + 1 - k), bot);
            return;
        }
        #[cfg(feature = "diag")]
        {
            self.scroll_copy += k;
        }
        // **The rows moved and no ring did**: what a reader holding a
        // selection on them has to learn (`Term::row_moves`), and the
        // watched lines move with them.
        self.cold
            .band_moved((top, bot), Rows::of_count(k), Scroll::Up, self.book.top);
        // **Ceiling arm: the region move, priced.** Incorrect by construction --
        // a region that never scrolls leaves stale rows on the screen -- but
        // the cell accounting below still runs, so both arms attribute the
        // same work and only the stores differ. Same shape as
        // `ceiling-no-blank`, and it exists because `utf8/cava.raw` is the
        // first recording in the corpus that reaches this path at all: 51
        // copied region scrolls a rep against zero everywhere else.
        #[cfg(not(feature = "ceiling-no-region-copy"))]
        {
            if self.past_edge() {
                self.clear_swept();
            }
            let b = self.blank();
            self.view.delete_lines_in(top, n, bot, b);
            if self.past_edge() {
                self.lines_moved_past_edge(top, n, bot, BandMove::Up);
            }
        }
        if self.images.any_placed() {
            self.band_images_moved(top, bot, Rows::of_count(k), BandMove::Up);
        }
        if self.cold.guard_open.is_some() {
            self.guard_band_moved(top, bot, Rows::of_count(k), BandMove::Up);
        }
        if self.cold.placement.is_placed() {
            self.placement_band_moved(top, bot, Rows::of_count(k), BandMove::Up);
        }
        self.path_rows(Row::from_index(bot.index() + 1 - k), bot);
        // **The whole region, not `k` rows.** `delete_lines_in` moves every
        // row inside the region and blanks `k`; counting only the blanks
        // undercounted the copying path by the height of the region, which is
        // exactly the traffic the rotation above avoids.
        self.cells = self
            .cells
            .plus((bot.index() + 1 - top.index()) * self.cols.index());
    }

    /// **The margin bell armed on a row under a top region about to scroll
    /// `k` rows** keeps its line: the rotation moves the ring under the
    /// rows, and the repair puts their text back where it was, so the
    /// armed line is read at the rotation the scroll leaves. A line armed
    /// at another rotation is left to disarm at the write.
    #[cold]
    #[inline(never)]
    fn margin_bell_under(&mut self, bot: Row, k: Rows) {
        let rot = self.view.rot();
        let ring = self.view.ring();
        if let Some(a) = self.cold.bell.armed.as_mut() {
            if a.row > bot && a.rot == rot {
                a.rot = rot.advance(k.index(), ring);
            }
        }
    }

    /// **A top region's `n` scrolls, each a rotation and a repair**: the
    /// rows under the region put back, the line the scroll opens blanked,
    /// and their images moved with them. `PAST` also carries the cells past
    /// the live edge, to `reach`, for a screen narrower than its rows reach
    /// ([`View::copy_past_edge`]); a monomorph of its own, so the loop a
    /// screen with nothing past its edge runs has none of it compiled in.
    /// Counts the cells the repair wrote.
    #[inline(always)]
    fn rotate_top_region<const PAST: bool>(&mut self, n: Rows, bot: Row, reach: Cols) {
        let last = self.rows.index() - 1;
        let mut moved = 0;
        for _ in 0..n.index() {
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
            // Past the live edge, before the blank below can write past
            // it on the row the first copy read.
            if PAST {
                let mut past = PastRows {
                    flags: &mut *self.book.past,
                    held: &mut self.book.held,
                };
                moved += self
                    .view
                    .copy_past_edge(bot, Row::from_index(last), reach, &mut past);
            }
            self.fill_row_counted(bot);
            // The line the scroll opens is on the row the first row under
            // the region held, just copied down: past the edge it held
            // that row's cells.
            if PAST {
                let mut past = PastRows {
                    flags: &mut *self.book.past,
                    held: &mut self.book.held,
                };
                moved += self.view.clear_past_edge(bot, reach, &mut past);
            }
            // **The rows under the region went back, and their images
            // go back with them.** `retire` moved every image up with the
            // window, which is right for the region's and one row wrong
            // for these: without this an image on a status line walks up
            // into the region a row a scroll while its text stays put.
            if self.images.any_placed() {
                self.band_images_put_back(bot, Row::from_index(last));
            }
            if self.cold.guard_open.is_some() {
                self.guard_put_back(bot, Row::from_index(last));
            }
            if self.cold.placement.is_placed() {
                self.placement_band_moved(
                    bot,
                    Row::from_index(last),
                    Rows::of_count(1),
                    BandMove::Down,
                );
            }
        }
        self.cells = self.cells.plus(moved);
    }

    /// [`Paint::rotate_top_region`] past the live edge: only after a
    /// narrowing, so out of line.
    #[cold]
    #[inline(never)]
    fn rotate_top_region_past_edge(&mut self, n: Rows, bot: Row, reach: Cols) {
        self.clear_swept();
        self.rotate_top_region::<true>(n, bot, reach);
        // Its own scrolls brought in rows the repair has just put right.
        self.book.swept = self.book.top;
        self.settle_reach();
    }

    /// **The rows the full-screen scrolls swept since the last pass past the
    /// edge, cleared there** ([`View::clear_swept_past_edge`]), before a
    /// pass that moves or clears rows past the edge reads them -- the swept
    /// rows are found by where they lie, so before anything moves one -- on
    /// the primary narrower than its rows hold. The scrolls themselves pay
    /// nothing for it.
    #[cold]
    #[inline(never)]
    pub(crate) fn clear_swept(&mut self) {
        if self.cold.screen != Screen::Normal {
            return;
        }
        let n = self.book.top.since(self.book.swept).unwrap_or(Rows::ZERO);
        self.book.swept = self.book.top;
        if n.index() == 0 {
            return;
        }
        let reach = self.book.reach;
        let mut past = PastRows {
            flags: &mut *self.book.past,
            held: &mut self.book.held,
        };
        let w = self.view.clear_swept_past_edge(n, reach, &mut past);
        self.cells = self.cells.plus(w);
    }

    /// **Back to the live width once no row holds anything past it.** The
    /// passes past the edge carry a held row's flag with its cells and clear
    /// it with them, so a row of history wider than the narrowed screen is
    /// carried off by the repair when the ring brings it round, and from
    /// then the scroll paths take the loop with nothing past the edge
    /// compiled in. Without it the measure at the resize was the last word,
    /// and the cost of a narrowing over one long line never fell.
    fn settle_reach(&mut self) {
        if self.book.held.index() == 0 {
            self.book.reach = self.cols;
        }
    }

    /// **The line a scroll down takes back, cleared past the live edge**,
    /// on the primary narrower than its rows hold: the newest line of
    /// history comes back as the blank top row, blanked to the live edge, or
    /// to its mark when that lies past the edge, and past both it kept the
    /// line's cells, which a growth showed on a line that was blank when it
    /// came in. At once, not with the swept rows: the window moves back over
    /// it, so where it lies says nothing of how it came. Out of line, behind
    /// one compare in the caller.
    #[cold]
    #[inline(never)]
    pub(crate) fn clear_incoming_past_edge(&mut self, r: Row) {
        if self.cold.screen != Screen::Normal {
            return;
        }
        let reach = self.book.reach;
        let mut past = PastRows {
            flags: &mut *self.book.past,
            held: &mut self.book.held,
        };
        let n = self.view.clear_past_edge(r, reach, &mut past);
        self.cells = self.cells.plus(n);
        self.settle_reach();
    }

    /// **A row an erase covers to its end, cleared past the live edge
    /// first** ([`View::clear_past_edge_of`]), on the primary narrower than
    /// its rows hold: `EL 0` and `EL 2`, the cursor's row of `ED 0`, and
    /// each row of the prompt block a redraw erases, behind the window too;
    /// the rows `ED` and `DECCOLM`'s clear cover whole go through
    /// [`Paint::clear_erased_rows_past_edge`]. Not `DECSEL` or `DECSED`,
    /// which spare protected characters and stop at the live edge. The
    /// erase stands in for the line being cleared, so nothing of the old
    /// line survives past the edge for a growth to show; the erase's own
    /// blank then runs on to the mark, which is TO_THE_MARK. `at` is the
    /// ring row and `blank` the erase's. Out of line, behind one compare of
    /// the reach in each caller.
    #[cold]
    #[inline(never)]
    pub(crate) fn clear_erased_past_edge(&mut self, at: RowRot, blank: PackedCell) {
        if self.cold.screen != Screen::Normal {
            return;
        }
        let reach = self.book.reach;
        let mut past = PastRows {
            flags: &mut *self.book.past,
            held: &mut self.book.held,
        };
        let n = self.view.clear_past_edge_of(at, reach, &mut past, blank);
        self.cells = self.cells.plus(n);
        self.settle_reach();
    }

    /// [`Paint::clear_erased_past_edge`] on screen rows `from` to `to`, the
    /// rows an `ED` or `DECCOLM`'s clear covers whole, before their fills.
    /// One compare of the reach for the erase and not one a row: with the
    /// compare in the row loop, `vim`'s 46 `EL` and 15 `ED 2` a rep took
    /// 6,448 more instructions than the base, ranges apart; out of it, the
    /// difference is inside the base's range.
    #[cold]
    #[inline(never)]
    pub(crate) fn clear_erased_rows_past_edge(&mut self, from: Row, to: Row) {
        if self.cold.screen != Screen::Normal {
            return;
        }
        let blank = self.blank();
        let (reach, rot, ring) = (self.book.reach, self.view.rotation(), self.view.ring());
        let mut past = PastRows {
            flags: &mut *self.book.past,
            held: &mut self.book.held,
        };
        let mut n = 0;
        for y in from.index()..to.index() {
            n += self
                .view
                .clear_past_edge_of(rot.advance(y, ring), reach, &mut past, blank);
        }
        self.cells = self.cells.plus(n);
        self.settle_reach();
    }

    /// **The rows `IL`, `DL` and a copying region scroll moved, past the
    /// live edge**, on the primary narrower than its rows hold: the same
    /// moves on the columns past the edge, and the rows they opened cleared
    /// there ([`View::insert_lines_past_edge`],
    /// [`View::delete_lines_past_edge`]). Called after the move, which went
    /// to the live edge and carried the marks. Out of line, behind one
    /// compare in each caller.
    #[cold]
    #[inline(never)]
    pub(crate) fn lines_moved_past_edge(&mut self, at: Row, n: Count, bottom: Row, way: BandMove) {
        if self.cold.screen != Screen::Normal {
            return;
        }
        let reach = self.book.reach;
        let mut past = PastRows {
            flags: &mut *self.book.past,
            held: &mut self.book.held,
        };
        let w = match way {
            BandMove::Down => self
                .view
                .insert_lines_past_edge(at, n, bottom, reach, &mut past),
            BandMove::Up => self
                .view
                .delete_lines_past_edge(at, n, bottom, reach, &mut past),
        };
        self.cells = self.cells.plus(w);
        self.settle_reach();
    }

    /// Whether the primary's rows may hold anything past the live edge: the
    /// one compare the line paths pay for the passes past it.
    #[inline(always)]
    pub(crate) fn past_edge(&self) -> bool {
        self.book.reach.index() > self.cols.index()
    }

    /// [`Paint::scroll_region_down`]'s full-screen scroll on a screen
    /// whose rows may hold something past the live edge: the swept rows
    /// cleared before the window moves back over them, each line taken
    /// back cleared past the edge, and the swept count restarted. Only
    /// after a narrowing, so out of line.
    ///
    /// **The count restarts here and not on the scroll the caller keeps.**
    /// `Book::swept` is read only by [`Paint::clear_swept`] and the
    /// resize's twin, each behind the reach lying past the live edge, and
    /// the reach rises there only in a resize, which restarts the count as
    /// it measures; so on a screen with nothing past the edge the count is
    /// written again before it is next read.
    #[cold]
    #[inline(never)]
    fn scroll_down_past_edge(&mut self, k: Rows) {
        self.clear_swept();
        for _ in 0..k.index() {
            self.view.retreat_rot(1);
            self.unretire();
            self.watch_taken_back();
            self.fill_row_counted(Row::ZERO);
            if self.past_edge() {
                self.clear_incoming_past_edge(Row::ZERO);
            }
        }
        // The window moved back, so the swept rows are counted from here:
        // on the primary, whose rows they are and whose window moved.
        if self.cold.screen == Screen::Normal {
            self.book.swept = self.book.top;
        }
    }

    /// A line the whole screen's scroll down takes back, tested against the
    /// watch on a selection's lines once the line is counted (`unretire`):
    /// the top row, which shows the line taken back and is blanked, and the
    /// row the window gave up below, whose text a later line feed blanks as
    /// it brings it back. Numbered as the selection is, either is a line the
    /// selection may name -- the top row one of the history's when the
    /// read has taken back more lines than the arm's row -- neither moved in
    /// place, and a read that takes the line back and retires it again nets
    /// no movement, so nothing else tells the embedder the text is gone. One
    /// compare when no watch is armed, which is every read `ingest2` makes.
    #[inline(always)]
    fn watch_taken_back(&mut self) {
        if self.cold.watched != crate::term::Watched::Rows {
            return;
        }
        let under = Row::from_index(self.rows.index());
        self.cold.test_watch(Row::ZERO, Row::ZERO, self.book.top);
        self.cold.pushed_under(under, self.book.top);
    }

    pub(crate) fn scroll_region_down(&mut self, n: Count) {
        let (top, bot) = (self.cold.top, self.cold.bot);
        if top.index() > bot.index() {
            return;
        }
        if self.fenced() {
            self.scroll_rect(top, n, BandMove::Down);
            return;
        }
        let k = n.at_least_one().min(bot.index() + 1 - top.index());
        let last = self.rows.index() - 1;
        if top.index() == 0 && bot.index() == last {
            // **One compare for the passes past the edge, then the scroll
            // the core had before them.** Tested again after each line and
            // followed by the swept count's store, they were sixteen
            // instructions a scroll down on a screen with nothing past the
            // edge, for a scroll of one line, and are five: 1,209 fewer a
            // rep on `less`, 593,934 to 592,725.
            if self.past_edge() {
                self.scroll_down_past_edge(Rows::of_count(k));
                self.path_rows(Row::ZERO, Row::from_index(k - 1));
                return;
            }
            for _ in 0..k {
                self.view.retreat_rot(1);
                self.unretire();
                self.watch_taken_back();
                self.fill_row_counted(Row::ZERO);
            }
            self.path_rows(Row::ZERO, Row::from_index(k - 1));
            return;
        }
        #[cfg(feature = "diag")]
        {
            self.scroll_copy += k;
        }
        self.cold
            .band_moved((top, bot), Rows::of_count(k), Scroll::Down, self.book.top);
        if self.past_edge() {
            self.clear_swept();
        }
        let b = self.blank();
        self.view.insert_lines_in(top, n, bot, b);
        if self.past_edge() {
            self.lines_moved_past_edge(top, n, bot, BandMove::Down);
        }
        if self.images.any_placed() {
            self.band_images_moved(top, bot, Rows::of_count(k), BandMove::Down);
        }
        if self.cold.guard_open.is_some() {
            self.guard_band_moved(top, bot, Rows::of_count(k), BandMove::Down);
        }
        if self.cold.placement.is_placed() {
            self.placement_band_moved(top, bot, Rows::of_count(k), BandMove::Down);
        }
        self.path_rows(top, Row::from_index(top.index() + k - 1));
        self.cells = self
            .cells
            .plus((bot.index() + 1 - top.index()) * self.cols.index());
    }

    /// DECSTBM. **A region needs at least two rows**, and setting one homes
    /// the cursor, to the page origin ([`Paint::home`]). **An omitted or 0
    /// bottom is the last line** ([`Params::far_margin`]): "default Pb: last
    /// line", and the algorithm reads a 0 as omitted (`[[source:
    /// dec-std-070]]` DECSTBM, pp. 5-25 and 5-26). A 0 read as line 1 until
    /// 2026-10-13, so `CSI 2;0 r` was dropped. **A bottom past the screen
    /// leaves the region alone**: note 3, "will be ignored (not executed)",
    /// and the algorithm's `parv[1] <= lines_per_page` (`OneBased::row_within`);
    /// it was clamped to the last line until the same day, as xterm and
    /// ghostty do.
    pub(crate) fn set_scroll_region(&mut self, top: OneBased, bottom: Option<OneBased>) {
        let last = Row::from_index(self.rows.index() - 1);
        let t = top.to_row(self.rows);
        let Some(b) = bottom.map_or(Some(last), |v| v.row_within(self.rows)) else {
            return;
        };
        if t.index() >= b.index() {
            return;
        }
        self.cold.top = t;
        self.cold.bot = b;
        self.home();
    }

    /// IL. **Outside the region it does nothing**, which is the rule that
    /// keeps a pager's `IL` from disturbing its status line -- outside the
    /// side margins too: `[[source: dec-std-070]]` p. 5-24 marks `IL` and `DL`
    /// for both margin pairs, and its IL (p. 5-146) reads "Only that portion
    /// of the display between the top, bottom, left, and right margins is
    /// affected. IL is ignored if the Active Position is outside the Scroll
    /// Area" (`[[source: vt510-il]]` says "page margins"). Under them it moves
    /// the rectangle.
    ///
    /// **And it puts the cursor on the left margin**, on its own row, before
    /// anything moves: IL and DL note 2 (pp. 5-146 and 5-148), "The Active
    /// Position is set to the Left Margin in the active line after execution
    /// of this control. The active line does not change", and both algorithms
    /// set `active_position.column = left_margin` inside the test that
    /// ignores the control. xterm's `InsertLine` and `DeleteLine` do
    /// (`[[source: xterm-scroll]]` 1650 and 1760) and so does ghostty
    /// (`[[source: ghostty-d9840f3-terminal]]` 2969 and 3143).
    /// At full width that is column 0, where the cursor stayed on its column
    /// until 2026-10-15, as alacritty's still does.
    pub(crate) fn insert_lines(&mut self, n: Count) {
        if self.cy.index() < self.cold.top.index() || self.cy.index() > self.cold.bot.index() {
            return;
        }
        if !self.within_side_margins() {
            return;
        }
        // Past the guards, so one they ignore leaves a string open
        // (`Paint::edit_reaches`).
        self.edit_reaches(crate::aligned::EditRows::Lines);
        // The line home once SLH has set one (`crate::bounds`).
        self.set_col(CursorCol::moved(self.homed(self.cold.left)));
        // SEE 1 and 2 erase the active line; SEE 3 shifts to the area's
        // last line (`crate::extent`).
        if self.lines_in_line() {
            return;
        }
        if let Some(last) = self.area_last_line() {
            return self.lines_to(last, |p| p.insert_lines_in_region(n));
        }
        self.insert_lines_in_region(n);
    }

    /// `IL` from the cursor's line to the bottom margin.
    fn insert_lines_in_region(&mut self, n: Count) {
        if self.fenced() {
            self.scroll_rect(self.cy, n, BandMove::Down);
            return;
        }
        if self.on_status() {
            self.status_scrolled();
            return;
        }
        let (at, bot) = (self.cy, self.cold.bot);
        let k = n.at_least_one().min(bot.index() + 1 - at.index());
        self.cold
            .lines_inserted((at, bot), Rows::of_count(k), self.book.top);
        if self.past_edge() {
            self.clear_swept();
        }
        let b = self.blank();
        self.view.insert_lines_in(at, n, bot, b);
        self.lines_bounded(at, bot, Rows::of_count(k), BandMove::Down);
        self.path_rows(at, Row::from_index(at.index() + k - 1));
        if self.past_edge() {
            self.lines_moved_past_edge(at, n, bot, BandMove::Down);
        }
        if self.images.any_placed() {
            self.band_images_moved(at, bot, Rows::of_count(k), BandMove::Down);
        }
        if self.cold.guard_open.is_some() {
            self.guard_band_moved(at, bot, Rows::of_count(k), BandMove::Down);
        }
        if self.cold.placement.is_placed() {
            self.placement_band_moved(at, bot, Rows::of_count(k), BandMove::Down);
        }
        self.cells = self.cells.plus(k * self.cols.index());
        self.row_changed_to(self.cy);
    }

    /// DL, `IL`'s inverse, under the same two bounds (`[[source: vt510-dl]]`:
    /// "DL has no effect outside the scrolling margins").
    pub(crate) fn delete_lines(&mut self, n: Count) {
        if self.cy.index() < self.cold.top.index() || self.cy.index() > self.cold.bot.index() {
            return;
        }
        if !self.within_side_margins() {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Lines);
        self.set_col(CursorCol::moved(self.homed(self.cold.left)));
        if self.lines_in_line() {
            return;
        }
        if let Some(last) = self.area_last_line() {
            return self.lines_to(last, |p| p.delete_lines_in_region(n));
        }
        self.delete_lines_in_region(n);
    }

    /// **`IL` or `DL` with the bottom margin at `last`** for the shift, SEE
    /// 3's area's last line, and put back after.
    #[cold]
    #[inline(never)]
    fn lines_to(&mut self, last: Row, f: impl FnOnce(&mut Self)) {
        let bot = self.cold.bot;
        self.cold.bot = last;
        f(self);
        self.cold.bot = bot;
    }

    /// `DL` from the cursor's line to the bottom margin.
    fn delete_lines_in_region(&mut self, n: Count) {
        if self.fenced() {
            self.scroll_rect(self.cy, n, BandMove::Up);
            return;
        }
        if self.on_status() {
            self.status_scrolled();
            return;
        }
        let (at, bot) = (self.cy, self.cold.bot);
        let k = n.at_least_one().min(bot.index() + 1 - at.index());
        self.cold
            .lines_deleted((at, bot), Rows::of_count(k), self.book.top);
        if self.past_edge() {
            self.clear_swept();
        }
        let b = self.blank();
        self.view.delete_lines_in(at, n, bot, b);
        self.lines_bounded(at, bot, Rows::of_count(k), BandMove::Up);
        self.path_rows(Row::from_index(bot.index() + 1 - k), bot);
        if self.past_edge() {
            self.lines_moved_past_edge(at, n, bot, BandMove::Up);
        }
        if self.images.any_placed() {
            self.band_images_moved(at, bot, Rows::of_count(k), BandMove::Up);
        }
        if self.cold.guard_open.is_some() {
            self.guard_band_moved(at, bot, Rows::of_count(k), BandMove::Up);
        }
        if self.cold.placement.is_placed() {
            self.placement_band_moved(at, bot, Rows::of_count(k), BandMove::Up);
        }
        self.cells = self.cells.plus(k * self.cols.index());
        self.row_changed_to(self.cy);
    }

    // ---- erasing --------------------------------------------------------

    /// ECH: blank `n` cells from the cursor, without moving anything.
    pub(crate) fn erase_chars(&mut self, n: Count) {
        if self.cold.modes.is_set(ModeBit::PresentationEdits) && self.erase_chars_presented(n) {
            return;
        }
        let (r, from) = (self.cy, self.cx.col());
        // To the line's end, the half on a double line.
        let to = Col::at_index((from.index() + n.at_least_one()).min(self.line_cols().index()));
        let b = self.blank();
        self.erase_span(r, from, to, b);
        self.wrote_rows(r, r);
    }

    /// DCH: pull the rest of the row left over `n` cells.
    pub(crate) fn delete_chars(&mut self, n: Count) {
        // Under an extent SEE chose other than the line, out of line
        // (`crate::extent`).
        if self.extent_chosen() {
            return self.edit_extent(n, crate::editing::CharEdit::Delete);
        }
        // Under HEM's PRECEDING or DCSM's PRESENTATION, out of line
        // (`crate::editing`).
        if self
            .cold
            .modes
            .either_set(ModeBit::PresentationEdits, ModeBit::EditPreceding)
            && self.edit_moded(n, crate::editing::CharEdit::Delete)
        {
            return;
        }
        let (r, at, b) = (self.cy, self.cx.col(), self.blank());
        // **The right margin, not the row.** `to` was `cols` before the
        // margins existed, so a `DCH` inside a region pulled text in from
        // *outside* it -- correct only while the margin is the edge, which is
        // every stream that never sends `DECSLRM`. **And the line's end**: a
        // double line's half, `margin = end_of_line(line)` where the right
        // margin is past it (`[[source: dec-std-070]]` p. 5-145), which the
        // fence is.
        let to = self.line_fence().after();
        // Right of the fence it is ignored, `[[source: dec-std-070]]` DCH
        // note 3 (p. 5-144); `to - at` below underflowed there, a panic in a
        // debug build.
        if at > to {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Chars);
        self.view.delete_cells(r, at, n, to, b);
        self.erase_images(r, at, to);
        self.cells = self.cells.plus(to.index() - at.index());
        self.row_changed_to(r);
        self.wrote_rows(r, r);
    }

    /// ICH: push the rest of the row right over `n` blanks.
    pub(crate) fn insert_blanks(&mut self, n: Count) {
        if self.extent_chosen() {
            return self.edit_extent(n, crate::editing::CharEdit::Insert);
        }
        if self
            .cold
            .modes
            .either_set(ModeBit::PresentationEdits, ModeBit::EditPreceding)
            && self.edit_moded(n, crate::editing::CharEdit::Insert)
        {
            return;
        }
        let (r, at, b) = (self.cy, self.cx.col(), self.blank());
        // The line's end on a double line too (p. 5-143), as `DCH`.
        let to = self.line_fence().after();
        // Right of the fence it is ignored, `[[source: dec-std-070]]` ICH
        // note 2 (p. 5-142), as `DCH` is.
        if at > to {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Chars);
        self.view.insert_cells(r, at, n, to, b);
        // The blanks join the string of the character they pushed on, the
        // active position's (TR/53 note A.1 i).
        if self.strs.strings.begun == crate::strings::Begun::Yes {
            let k = n.at_least_one().min(to.index().saturating_sub(at.index()));
            let pushed = self.view.get(r, Col::at_index(at.index() + k));
            let s = match at.index() + k < to.index() {
                true => self.string_of(pushed.style),
                false => StringId::NONE,
            };
            let end = Col::at_index(at.index() + k);
            self.strings_kept(r, (at, end), b, |_| s);
        }
        self.erase_images(r, at, to);
        self.cells = self.cells.plus(to.index() - at.index());
        self.row_changed_to(r);
        self.wrote_rows(r, r);
    }

    /// **ED, EL and ECH over `[a, b)` of row `r`**: the fill every erase
    /// took, while no EPA has guarded a position since the last `RIS` -- one
    /// compare of a byte the erase's dispatch already has in cache -- and
    /// otherwise the fill that spares a guarded position under ERM reset and
    /// keeps its guard under ERM set (`View::fill_sparing`). Counts the
    /// cells written.
    ///
    /// **Once a string has been begun** (`crate::strings`), each erased
    /// position stays in the string it was in, out of line: ECMA-48's
    /// erasures put positions "into the erased state" and move nothing, so
    /// a string's positions keep their places in its presentation.
    #[inline(always)]
    pub(crate) fn erase_span(&mut self, r: Row, a: Col, b: Col, blank: PackedCell) {
        if self.strs.strings.begun == crate::strings::Begun::Yes {
            return self.erase_keeping_strings(r, a, b, blank);
        }
        self.erase_cells(r, a, b, blank);
    }

    #[inline(always)]
    fn erase_cells(&mut self, r: Row, a: Col, b: Col, blank: PackedCell) {
        let n = match self.cold.guards {
            crate::term::Guards::None => {
                let n = self.view.fill(r, a, b, blank);
                self.erase_images(r, a, b);
                n
            }
            crate::term::Guards::Marked => self.erase_sparing(r, a, b, blank).index(),
        };
        self.cells = self.cells.plus(n);
    }

    #[cold]
    #[inline(never)]
    fn erase_keeping_strings(&mut self, r: Row, a: Col, b: Col, blank: PackedCell) {
        let mut was = [StringId::NONE; crate::MAX_COLS];
        let cols = self.cols.index().min(crate::MAX_COLS);
        for (c, w) in was.iter_mut().enumerate().take(cols) {
            *w = self.string_of(self.view.get(r, Col::at_index(c)).style);
        }
        self.erase_cells(r, a, b, blank);
        // The span widened to whole characters, as the fill widens it.
        let from = Col::at_index(a.index().saturating_sub(1));
        let to = Col::at_index((b.index() + 1).min(cols));
        self.strings_kept(r, (from, to), blank, |c| was[c.index()]);
    }

    #[cold]
    #[inline(never)]
    fn erase_sparing(&mut self, r: Row, a: Col, b: Col, blank: PackedCell) -> Cols {
        // `[[source: ecma48]]` 7.2.4: PROTECT, "Only the contents of
        // unprotected areas are affected by an erasure control function";
        // ALL, "The contents of protected as well as of unprotected areas".
        let how = match self.cold.modes.is_set(ModeBit::EraseGuarded) {
            true => GuardedErasure::All,
            false => GuardedErasure::Protect,
        };
        let n = self.view.fill_sparing(r, a, b, blank, how);
        // A guarded position spared keeps what is drawn over it.
        match how {
            GuardedErasure::All => self.erase_images(r, a, b),
            GuardedErasure::Protect => self.erase_images_where(r, a, b, |c| c.flags.is_guarded()),
        }
        Cols::of_index(n)
    }

    /// **SPA**: the active position starts a guarded area
    /// (`[[source: ecma48]]` 6.5.2.2). A second SPA before EPA starts it
    /// again. The position is the cursor's as `CPR` reports it, so a pending
    /// wrap is the last column, where the cursor is shown, and with BDSM
    /// reset its presentation position: SPA and EPA name "the active
    /// presentation position" (8.3.129, 8.3.46), in both DCSM states as the
    /// cursor functions do (`crate::component`).
    pub(crate) fn start_guarded_area(&mut self) {
        let (screen, line) = self.image_anchor();
        let col = self.guard_position();
        self.cold.guard_open = Some(crate::term::GuardStart { screen, line, col });
    }

    /// The cursor's position for SPA and EPA: its column as `CPR` reports
    /// it, the position it is drawn at on a line holding a string with
    /// BDSM reset.
    fn guard_position(&self) -> Col {
        let c = self.cx.reported(self.report_fence(), self.cols);
        match self.guards_drawn() {
            true => self.line_map(self.cy).map_or(c, |m| m.presentation(c)),
            false => c,
        }
    }

    /// **Whether SPA and EPA take the positions as drawn**: BDSM reset,
    /// and no SDS or SRS string open. Inside one, where 8.3.129's and
    /// 8.3.46's NOTE says "the control functions for area definition ...
    /// should not be used", the drawn positions move under each character
    /// written, so the cells between the two are guarded, as before.
    fn guards_drawn(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorPresented) && self.style_raw.string().is_none()
    }

    /// **EPA**: guard every position from SPA's through the cursor, in the
    /// order of the character path -- the rest of the first row, the rows
    /// between whole, the last row to the cursor -- whichever of the two
    /// comes first. Nothing is guarded before it. A start whose line
    /// scrolled above the screen is its first position, and one below it
    /// its last. With no SPA open it does nothing.
    pub(crate) fn end_guarded_area(&mut self) {
        let Some(g) = self.cold.guard_open.take() else {
            return;
        };
        let (screen, top) = self.image_top();
        if g.screen != screen {
            return;
        }
        let (rows, cols) = (self.rows.index(), self.cols.index());
        let start = match g.line.since(top) {
            None => (0, 0),
            Some(r) if r.index() >= rows => (rows - 1, cols - 1),
            Some(r) => (r.index(), g.col.index().min(cols - 1)),
        };
        let end = (self.cy.index(), self.guard_position().index());
        let (a, b) = match start <= end {
            true => (start, end),
            false => (end, start),
        };
        let presented = self.guards_drawn();
        for y in a.0..=b.0 {
            let from = if y == a.0 { a.1 } else { 0 };
            let to = if y == b.0 { b.1 + 1 } else { cols };
            let r = Row::from_index(y);
            // On a line holding a string the positions are the ones drawn,
            // each guarding the cell drawn there.
            match presented.then(|| self.line_map(r)).flatten() {
                Some(m) => {
                    for p in from..to {
                        let c = m.data(Col::at_index(p));
                        self.view.guard_span(r, c, c.after());
                    }
                }
                None => {
                    self.view
                        .guard_span(r, Col::at_index(from), Col::at_index(to));
                }
            }
        }
        self.cold.guards = crate::term::Guards::Marked;
        // The cursor's row took cells past its written extent, perhaps.
        self.row_changed_to(self.cy);
    }

    /// **An open SPA's start after rows `[from, to]` of the live screen
    /// moved `k` `way`**: IL, DL and a region's scroll. A start inside the
    /// band moves with its line, 8.3.67's and 8.3.32's "the start ... of a
    /// qualified area ... in the shifted part, are also shifted", and one
    /// whose line left the band was deleted with it and is forgotten, as an
    /// image is. Out of line; the callers test that an SPA is open.
    #[cold]
    #[inline(never)]
    pub(crate) fn guard_band_moved(&mut self, from: Row, to: Row, k: Rows, way: BandMove) {
        let (screen, top) = self.image_top();
        let Some(g) = self.cold.guard_open.as_mut() else {
            return;
        };
        let lo = top.plus(Rows::of_count(from.index()));
        let hi = top.plus(Rows::of_count(to.index() + 1));
        if g.screen != screen || g.line < lo || g.line >= hi {
            return;
        }
        let moved = match way {
            BandMove::Up => g.line.minus(k).filter(|l| *l >= lo),
            BandMove::Down => Some(g.line.plus(k)).filter(|l| *l < hi),
        };
        match moved {
            Some(l) => g.line = l,
            None => self.cold.guard_open = None,
        }
    }

    /// An open SPA's start after a top-anchored region's scroll put the rows
    /// `[from, to]` under it back down a row: it goes back with its row, as
    /// an image does (`ImageStore::put_back_band`).
    #[cold]
    #[inline(never)]
    pub(crate) fn guard_put_back(&mut self, from: Row, to: Row) {
        self.guard_band_moved(from, to, Rows::of_count(1), BandMove::Down);
    }

    /// **ED, EL, DECSED and DECSEL with a list: every parameter, in order**,
    /// as `[[source: dec-std-070]]`'s algorithms loop
    /// `for (n=0; n<parc; n++) switch(parv[n])` (EL p. 5-155, ED p. 5-157,
    /// DECSEL p. 5-160, DECSED p. 5-163). A value the `switch` has no case
    /// for is skipped, and counted. **So is a `3`**: xterm's "Erase Saved
    /// Lines" is the value of a lone parameter, and STD 070's `switch` has
    /// no case 3. Out of line: a lone parameter never comes here. **Each
    /// extent ends an aligned string open on a line it reaches**, before it
    /// erases, as the lone parameter's does ([`Paint::edit_reaches`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_each(&mut self, csi: &crate::write::Csi, which: EraseList) {
        use crate::aligned::EditRows;
        csi.for_each_param(|v| match EraseExtent::from_param(v) {
            Some(e) => match which {
                EraseList::Display => {
                    self.edit_reaches(EditRows::Display(e));
                    self.erase_display(e)
                }
                EraseList::Line => {
                    self.edit_reaches(EditRows::Line);
                    self.erase_line(e)
                }
                EraseList::SelectiveDisplay => {
                    self.edit_reaches(EditRows::Display(e));
                    self.selective_erase_display(e)
                }
                EraseList::SelectiveLine => {
                    self.edit_reaches(EditRows::Line);
                    self.selective_erase_line(e)
                }
            },
            None => self.unsupported_param += 1,
        });
    }

    /// **EA**: the active qualified area, `[[source: ecma48]]` 8.3.37 --
    /// from the active position to the area's end, from its start through
    /// the active position, or all of it. baud's qualified areas are SPA and
    /// EPA's guarded areas, kept as a flag on each position, so **the active
    /// area is the run, in the character path, of positions that share the
    /// active position's guard**: a guarded area, or the unguarded positions
    /// between two, or between one and the page's edge, each line to its own
    /// end. Two guarded areas that touch read as one. Guarded positions go
    /// as ERM says, through `erase_span`, and the blank is DECECM's. Out of
    /// line, and a walk over the area it erases. **Under DCSM's
    /// PRESENTATION** the walk is over presentation positions, each line's
    /// as drawn, and the cells drawn at the positions it covers are erased
    /// where they stand (8.3.37, "the qualified area in the presentation
    /// component"; `crate::component`).
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_in_area(&mut self, e: EraseExtent, csi: &crate::write::Csi) {
        self.count_past_first(csi);
        let end = |p: &Self, r: usize| {
            p.view
                .rendition(Row::from_index(r))
                .end_of_line(p.cols)
                .index()
        };
        let mut drawn = crate::component::Drawn::of(self.cold.modes);
        let cursor = self.drawn_from(&mut drawn, self.cy, self.cx.col());
        let mut guarded = |p: &Self, r: usize, c: usize| {
            let r = Row::from_index(r);
            let c = p.drawn_at(&mut drawn, r, Col::at_index(c));
            p.view.get(r, c).flags.is_guarded()
        };
        let at = (
            self.cy.index(),
            cursor.index().min(end(self, self.cy.index()) - 1),
        );
        let g = guarded(self, at.0, at.1);
        let mut start = at;
        loop {
            let prev = match start {
                (r, c) if c > 0 => (r, c - 1),
                (r, _) if r > 0 => (r - 1, end(self, r - 1) - 1),
                _ => break,
            };
            if guarded(self, prev.0, prev.1) != g {
                break;
            }
            start = prev;
        }
        let last_row = self.rows.index() - 1;
        let mut stop = at;
        loop {
            let next = match stop {
                (r, c) if c + 1 < end(self, r) => (r, c + 1),
                (r, _) if r < last_row => (r + 1, 0),
                _ => break,
            };
            if guarded(self, next.0, next.1) != g {
                break;
            }
            stop = next;
        }
        let (from, to) = match e {
            EraseExtent::ToEnd => (at, stop),
            EraseExtent::ToStart => (start, at),
            EraseExtent::All => (start, stop),
        };
        let span = (Row::from_index(from.0), Row::from_index(to.0));
        self.edit_reaches(crate::aligned::EditRows::Span(span.0, span.1));
        let blank = self.blank();
        for r in from.0..=to.0 {
            let a = if r == from.0 { from.1 } else { 0 };
            let b = if r == to.0 { to.1 + 1 } else { end(self, r) };
            // A row erased to its end is cleared past a narrower edge
            // first, as `EL 0` and `EL 2` clear it
            // (`Paint::clear_erased_past_edge`).
            if b == end(self, r) && self.past_edge() {
                let at = self.view.rotation().advance(r, self.view.ring());
                self.clear_erased_past_edge(at, blank);
            }
            let (row, a, b) = (Row::from_index(r), Col::at_index(a), Col::at_index(b));
            match drawn.map_of(self, row) {
                Some(m) => self.erase_drawn_span(row, &m, (a, b), blank),
                None => self.erase_span(row, a, b, blank),
            }
        }
        self.wrote_rows(Row::from_index(from.0), Row::from_index(to.0));
    }

    /// **EF**: the active field, `[[source: ecma48]]` 8.3.40 -- "the
    /// character position at a character tabulation stop (beginning of the
    /// field) and the character positions up to, but not including, the
    /// character position at the following character tabulation stop"
    /// (4.2.40). **A field stays in its line**: the line's start bounds the
    /// positions before its first stop, its end the positions after its last,
    /// and with no stops the line is one field. Guarded positions go as ERM
    /// says, as for EA.
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_in_field(&mut self, e: EraseExtent, csi: &crate::write::Csi) {
        self.count_past_first(csi);
        self.edit_reaches(crate::aligned::EditRows::Line);
        let r = self.cy;
        let end = self.line_cols().index();
        // **Under DCSM's PRESENTATION** the field holding the cursor's
        // presentation position, its stops at presentation positions as
        // `HT` counts them (8.3.40, "the active field (the field which
        // contains the active presentation position in the presentation
        // component)"), and the cells drawn there erased where they stand.
        let mut drawn = crate::component::Drawn::of(self.cold.modes);
        let x = self
            .drawn_from(&mut drawn, r, self.cx.col())
            .index()
            .min(end - 1);
        let start = (0..=x).rev().find(|&c| self.tabs[c]).unwrap_or(0);
        let stop = (x + 1..end).find(|&c| self.tabs[c]).unwrap_or(end);
        let (a, b) = match e {
            EraseExtent::ToEnd => (x, stop),
            EraseExtent::ToStart => (start, x + 1),
            EraseExtent::All => (start, stop),
        };
        let blank = self.blank();
        // A field that reaches the line's end clears the line past a
        // narrower edge first, as `EA` does a row it erases to its end.
        if b == end && self.past_edge() {
            let at = self.view.rotation().advance(r.index(), self.view.ring());
            self.clear_erased_past_edge(at, blank);
        }
        let (a, b) = (Col::at_index(a), Col::at_index(b));
        match drawn.map_of(self, r) {
            Some(m) => self.erase_drawn_span(r, &m, (a, b), blank),
            None => self.erase_span(r, a, b, blank),
        }
        self.wrote_rows(r, r);
    }

    /// The parameters past the first, which EA and EF do not act on.
    fn count_past_first(&mut self, csi: &crate::write::Csi) {
        let mut n = 0;
        csi.for_each_param(|_| n += 1);
        self.unsupported_param += core::cmp::max(n, 1) - 1;
    }

    /// DECSED / DECSEL: each character DECSCA left erasable becomes a space
    /// in its own rendition, and a protected one and an empty position stay
    /// ([`PackedCell::selectively_erased`]).
    pub(crate) fn selective_erase_line(&mut self, e: EraseExtent) {
        let (a, b) = self.line_span(e);
        self.selective_wipe(self.cy, a, b);
    }

    pub(crate) fn selective_erase_display(&mut self, e: EraseExtent) {
        let (from, to) = self.display_rows(e);
        for y in from.index()..to.index() {
            // Each line to its own end, `x <= end_of_line(y)` (p. 5-163).
            let r = Row::from_index(y);
            let end = self.view.rendition(r).end_of_line(self.cols);
            self.selective_wipe(r, Col::ZERO, Col::at_index(end.index()));
        }
        if e != EraseExtent::All {
            let (a, b) = self.line_span(e);
            self.selective_wipe(self.cy, a, b);
        }
    }

    /// **To whole characters** (`View::snap_span`): a wide character an
    /// edge cuts becomes two spaces, as ED and EL snap; both halves carry
    /// its protection, so a protected one stays whole.
    fn selective_wipe(&mut self, r: Row, from: Col, to: Col) {
        let (from, to) = self.view.snap_span(r, from, to);
        for x in from.index()..to.index() {
            let c = Col::at_index(x);
            let old = self.view.get(r, c);
            if !old.is_protected() && !old.content.is_empty() {
                self.view.put(r, c, PackedCell::selectively_erased(old));
                self.cells = self.cells.plus(1);
            }
        }
        // The positions erased lose what is drawn over them; a protected
        // one keeps it, and so does an unoccupied one, which the erase does
        // not touch ("Unoccupied character positions are not affected",
        // DECSERA). Judged after the wipe, an erased position holding a
        // space.
        self.erase_images_where(r, from, to, |c| c.is_protected() || c.content.is_empty());
        self.selectively_erased_whole(r, from, to);
        self.row_changed_to(r);
        self.wrote_rows(r, r);
    }

    /// `CSI 3 J`, "Erase Saved Lines, xterm" (`[[source: ctlseqs]]`): the
    /// history goes and the screen, the cursor and the rendition stay.
    ///
    /// **The primary's, from either screen.** The text names the saved lines
    /// and not a screen, and only the primary keeps any; xterm's `case 3`
    /// zeroes its one `savedlines` count whichever buffer is shown
    /// (`do_erase_display`, `[[source: xterm-scroll]]`). While the alternate
    /// screen is up the primary is the parked surface, so it is swapped in
    /// for the erase and back out -- four slices exchanged, nothing copied.
    ///
    /// **The whole ring past the screen and the cut rows, not the count**:
    /// `RIS`'s reason, for the same readers -- `Term::cells` hands the whole
    /// ring to a shader, and a re-cut that spills past the screen writes into
    /// those rows without clearing past its width. **The rows a shorter
    /// window cut below the cursor stay**: they are the bottom of the screen
    /// a growth brings back, not lines that scrolled off its top.
    ///
    /// An image whose rows were all on the erased lines goes, as one whose
    /// last row leaves a full ring does; one with a row still on screen
    /// stays, a kitty picture
    /// (`erasing_the_history_keeps_a_picture_with_rows_on_screen`) and a
    /// Sixel (`erasing_the_history_keeps_a_sixel_with_rows_on_screen`),
    /// which xterm's `case 3` keeps too: it zeroes the count and touches no
    /// graphic (`[[source: xterm-scroll]]`). The erasure is counted for an
    /// embedder's window over the history
    /// ([`crate::Term::history_erasures`]).
    pub(crate) fn erase_saved_lines(&mut self) {
        // The saved lines are the main display's, from the status line too.
        if self.on_status() {
            self.on_main(|p| p.erase_saved_lines());
            return;
        }
        let keep = Rows::of_count(self.rows.index() + self.book.hidden.index());
        let (n, links) = self.on_primary(|v| v.erase_outside(keep));
        self.cells = self.cells.plus(n);
        // Only an erase of rows that held links pays for the sweep after it:
        // one of a history holding none frees nothing a sweep could find.
        if links == RowLinks::May {
            self.strs.link_book.history_erased();
        }
        self.book.hist = Rows::ZERO;
        // The lines a window over the history showed are gone, so it is on
        // the live screen, from either screen.
        self.book.window = crate::storage::HistoryWindow::Live;
        self.cold.erased(crate::term::Erasure::SavedLines);
        if self.images.any_placed() {
            self.expire_primary_images(None);
        }
        // And the prompts on them: a jump to one would land on nothing.
        self.strs.prompts.forget_before(self.book.top);
    }

    /// `CSI ? 3 J`, "Selective Erase Saved Lines, xterm": DECSED over the
    /// history, so the characters DECSCA protected stay and the rest become
    /// spaces in their own rendition, as `selective_wipe` does on the
    /// screen (`[[source: vt510-decsca]]`: "The selective erase
    /// control functions (DECSED and DECSEL) can only erase characters
    /// defined as erasable"). The lines stay lines of the history, as rows
    /// stay rows under DECSED, so nothing is counted as erased.
    ///
    /// **xterm erases them whole**: `do_erase_display`'s `case 3` is the same
    /// for both forms and sets `savedlines` to zero whatever the protection
    /// (`[[source: xterm-scroll]]`). That departs from the text; this follows
    /// the text (since 2026-10-10).
    ///
    /// **A selection on a saved line goes**, as one on a row a selective
    /// erase wipes does: the lines are tested against the watch on a
    /// selection's lines while the primary is up, whose history they are.
    /// The wipe is counted for an embedder that keeps what it read of the
    /// history ([`crate::Term::history_wipes`]).
    pub(crate) fn selective_erase_saved_lines(&mut self) {
        if self.on_status() {
            self.on_main(|p| p.selective_erase_saved_lines());
            return;
        }
        let hist = self.book.hist;
        let n = self.on_primary(|v| v.selective_wipe_behind(hist));
        self.cells = self.cells.plus(n);
        self.cold.history_wipes = self.cold.history_wipes.next();
        if self.cold.screen == Screen::Normal && hist.index() > 0 {
            let (from, to) = (
                SurfaceLine::Above(hist),
                SurfaceLine::Above(Rows::of_count(1)),
            );
            self.cold.test_watch_lines(from, to, self.book.top);
        }
    }

    /// Run `f` on the primary's surface whichever screen is up: the view
    /// itself on the primary, and on the alternate screen the parked primary
    /// swapped into the view for the call and back out after it.
    fn on_primary<R>(&mut self, f: impl FnOnce(&mut View<'_>) -> R) -> R {
        let parked = self.cold.screen == Screen::Alternate;
        if parked {
            self.view.swap_surface(&mut self.other);
        }
        let r = f(&mut self.view);
        if parked {
            self.view.swap_surface(&mut self.other);
        }
        r
    }

    /// The span `EraseExtent` names on the cursor's row: EL's and DECSEL's,
    /// and the cursor's row of ED and DECSED. The one definition of where
    /// an erase splits the row, so the four cannot disagree.
    /// `[[source: ecma48]]` § 8.3.40 (EL): `Ps = 1` erases from the
    /// beginning of the line up to and including the active position,
    /// which is why `ToStart` ends one past the cursor's column and not
    /// at it.
    pub(crate) fn line_span(&self, e: EraseExtent) -> (Col, Col) {
        let cols = Col::at_index(self.line_cols().index());
        match e {
            EraseExtent::ToStart => (Col::ZERO, Col::at_index(self.cx.col().index() + 1)),
            EraseExtent::All => (Col::ZERO, cols),
            EraseExtent::ToEnd => (self.cx.col(), cols),
        }
    }

    /// The whole rows `EraseExtent` names, excluding the cursor's own: the
    /// rows ED and DECSED erase whole. ED fills each to the row's end and
    /// DECSED each to its line's end (`Paint::selective_erase_display`).
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
    /// rendition evicts another only when every slot is occupied and neither a
    /// tombstone nor a due sweep frees one (`Paint::full_table`).**
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
    /// only once every slot is filled and no tombstone or due sweep frees one.
    /// Below that the mapping is **exact**:
    /// distinct renditions never share a slot and never corrupt each other's
    /// colour.
    ///
    /// The cost is a probe on the write path, and the probe length is
    /// `1/(1 - load)` on average -- a fraction of a slot while the table is
    /// far from full, which a screen holding a few hundred renditions in a
    /// screen-sized table is.
    ///
    /// **A miss reads to its bucket's [`Reach`], not to an empty slot.** A
    /// linear probe stops at its rendition or at an empty slot, and a full
    /// table has none, so each new colour there read all 65,536 slots: a
    /// picture viewer printing pictures into the shell took 8.4-9.8 s to
    /// ingest what it wrote in about 1.2 s, 95% of it in this loop. Each
    /// bucket keeps how far past it its farthest rendition lies, so the probe
    /// stops there, and the slot it picks is the one the walk to the end
    /// picked: 0.25-0.40 s, the table and every cell as before (note BH). The
    /// inline part reads at most [`SweepBook::INLINE`] slots, and the rest is
    /// out of line ([`Paint::intern_far`]).
    ///
    /// **The table is sized to the screen** (the screen-sizing in `Owned::with_history`), not fixed at
    /// 1024, because a text-drawn image preview mints one distinct truecolour
    /// rendition per cell and every one is live at once -- the whole image is on
    /// screen. A 1024-slot table under a photo evicts thousands of on-screen
    /// cells' renditions, and each resolves to a later cell's colour: the top
    /// band of the preview scrambles while the bottom stays coherent, because
    /// eviction hits the cells interned first. A table holding a screenful of
    /// distinct renditions cannot: below capacity the mapping is exact.
    /// Capacity is the fix while the image is shown, since nothing on it is
    /// dead.
    ///
    /// **A table sweeps at 7/8 load, and a full one before it evicts**
    /// ([`Paint::sweep_styles`]): the renditions no cell names are freed,
    /// so a session that mints more over its life than the table has slots
    /// -- picture after picture -- reuses the slots of those it no longer
    /// shows. `[[source: dec-std-070]]`'s
    /// SGR, page 5-104: "In no way does the selection of current rendition
    /// affect the rendition of characters already entered into the display",
    /// which an eviction does. It remains for cells naming more distinct
    /// renditions than the table has slots, where a cell naming the evicted
    /// slot is recoloured: an open `SGR` bug.
    ///
    /// **Inlined, because `restore` is its second caller.** With one caller
    /// LLVM inlined it into `apply_sgr`; when `DECRC` began interning the
    /// saved rendition again it outlined it, and every `SGR` paid a call.
    /// Forced, `apply_sgr` compiles as it did with one caller.
    ///
    /// **The probe inline keys a rendition without SAPV's variants or a
    /// string** ([`StyleId::plain_bucket`], [`StyleWord::is_plain`]), the
    /// key of every rendition that has neither, so it costs what it did
    /// before they were hashed. One with either lives on its own key's
    /// path, which the probe does not walk: it finds it or its place out
    /// of line, at the miss ([`Paint::intern_new`]).
    #[inline(always)]
    pub(crate) fn intern_style(&mut self, raw: StyleWord) -> StyleId {
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
        let base = StyleId::plain_bucket(raw, len);
        // Probe for this style's own slot or the first free one. `len` is a
        // power of two, so `& (len - 1)` wraps. A hit returns the slot the
        // style already has, which is why equal cells share a `StyleId`.
        // `INLINE` is below `NSTYLE`, so the loop never wraps the table.
        let mut k = base;
        for _ in 0..SweepBook::INLINE {
            match self.tmpl[k] {
                Some(w) if w == raw => return StyleId::from_index(k),
                None => return self.intern_new(StyleId::from_index(k), raw),
                Some(_) => k = (k + 1) & (len - 1),
            }
        }
        self.intern_far(StyleId::from_index(base), raw)
    }

    /// **The probe past [`SweepBook::INLINE`], to its bucket's [`Reach`].**
    /// No rendition homed at `base` lies farther past it than
    /// `reach[base]` ([`Paint::write_slot`]), so a probe that reads that
    /// far without finding `raw` or an empty slot has proved a miss without
    /// reading the rest of the table. The walk to the table's end it
    /// replaces did with that miss what this does: a rendition with SAPV's
    /// variants or a string goes to its own key's path
    /// ([`Paint::intern_new`]), and a plain one takes the first empty slot
    /// past, or the full table's tombstone, sweep or eviction when the table
    /// has none. Every slot it picks is the one the whole walk picked.
    #[cold]
    #[inline(never)]
    fn intern_far(&mut self, base: StyleId, raw: StyleWord) -> StyleId {
        let len = self.tmpl.len();
        let far = self.style_log.reach[base.index()].slots();
        let mut k = (base.index() + SweepBook::INLINE) & (len - 1);
        for n in SweepBook::INLINE..far {
            match self.tmpl[k] {
                Some(w) if w == raw => {
                    self.style_log.book.sweep.probed(n + 1);
                    return StyleId::from_index(k);
                }
                None => {
                    self.style_log.book.sweep.probed(n + 1);
                    return self.intern_new(StyleId::from_index(k), raw);
                }
                Some(_) => k = (k + 1) & (len - 1),
            }
        }
        self.style_log.book.sweep.probed(far.max(SweepBook::INLINE));
        if !raw.is_plain() {
            return self.intern_new(base, raw);
        }
        match self.first_empty(StyleId::from_index(k)) {
            Some(e) => self.intern_new(e, raw),
            None => self.full_table(base, raw),
        }
    }

    /// **The first empty slot from `from`**, or `None` when the table holds
    /// none, which [`SweepBook::filled`] says without a walk: it counts
    /// every filled slot but slot 0, which is never empty.
    fn first_empty(&mut self, from: StyleId) -> Option<StyleId> {
        let len = self.tmpl.len();
        if self.style_log.book.sweep.filled + 1 >= len {
            return None;
        }
        let mut j = from.index();
        for n in 0..len {
            if self.tmpl[j].is_none() {
                self.style_log.book.sweep.probed(n + 1);
                return Some(StyleId::from_index(j));
            }
            j = (j + 1) & (len - 1);
        }
        self.style_log.book.sweep.probed(len);
        None
    }

    /// **A miss: `raw` takes slot `k`, which the probe found empty.** The
    /// first tombstone on its path takes it instead, when the table holds
    /// one; otherwise a fill past [`SweepBook::LOAD`] sweeps first, when one
    /// is due. Out of line, as the write it replaced was, so the hit stays
    /// inline in `apply_sgr` and costs what it did.
    #[cold]
    #[inline(never)]
    fn intern_new(&mut self, k: StyleId, raw: StyleWord) -> StyleId {
        let len = self.tmpl.len();
        // A rendition with SAPV's variants or a string on its own key's
        // path, which the inline probe left unwalked: a hit there is its
        // slot, and the first empty slot there its place.
        let k = match raw.is_plain() {
            true => k,
            false => match self.variants_probe(raw) {
                Probed::Hit(id) => return id,
                Probed::Empty(k) => k,
                Probed::Full => {
                    return self.full_table(StyleId::from_index(StyleId::bucket(raw, len)), raw);
                }
            },
        };
        if self.style_log.book.sweep.tombs > 0 {
            let mut j = StyleId::bucket(raw, len);
            let mut n = 0;
            while j != k.index() {
                n += 1;
                if self.tmpl[j] == Some(StyleWord::TOMBSTONE) {
                    self.style_log.book.sweep.probed(n);
                    self.style_log.book.sweep.tomb_taken();
                    return self.write_slot(StyleId::from_index(j), Some(raw));
                }
                j = (j + 1) & (len - 1);
            }
            self.style_log.book.sweep.probed(n);
        }
        let book = &self.style_log.book;
        if book.sweep.over_load(len) && book.sweep.due(book.writes, len) {
            if let Some(id) = self.sweep_styles(raw) {
                return id;
            }
        }
        // `probe-double-intern-full` writes here over a filled slot.
        if self.tmpl[k.index()].is_none() {
            self.style_log.book.sweep.filled_one();
        }
        self.write_slot(k, Some(raw))
    }

    /// **`raw`'s own key's path, for a rendition with SAPV's variants**:
    /// its slot, the first empty one, or neither.
    /// Bounded as the inline probe is, by its bucket's [`Reach`].
    fn variants_probe(&mut self, raw: StyleWord) -> Probed {
        let len = self.tmpl.len();
        let home = StyleId::bucket(raw, len);
        let far = self.style_log.reach[home].slots();
        let mut k = home;
        for n in 0..far {
            match self.tmpl[k] {
                Some(w) if w == raw => {
                    self.style_log.book.sweep.probed(n + 1);
                    return Probed::Hit(StyleId::from_index(k));
                }
                None => {
                    self.style_log.book.sweep.probed(n + 1);
                    return Probed::Empty(StyleId::from_index(k));
                }
                Some(_) => k = (k + 1) & (len - 1),
            }
        }
        self.style_log.book.sweep.probed(far);
        match self.first_empty(StyleId::from_index(k)) {
            Some(e) => Probed::Empty(e),
            None => Probed::Full,
        }
    }

    /// **Every slot is filled.** The first tombstone on `raw`'s path from
    /// `base` takes it, which evicts nothing; failing that a sweep, if one is
    /// due; failing that an eviction at the bucket -- except slot 0, the
    /// default every blank cell names without interning it (`Term::new`),
    /// whose eviction would recolour them all. Its bucket's victim is the
    /// slot after it. A rendition with SAPV's variants or a string takes its
    /// own key's path, not the inline probe's.
    #[cold]
    #[inline(never)]
    fn full_table(&mut self, base: StyleId, raw: StyleWord) -> StyleId {
        let len = self.tmpl.len();
        let base = match raw.is_plain() {
            true => base,
            false => StyleId::from_index(StyleId::bucket(raw, len)),
        };
        if self.style_log.book.sweep.tombs > 0 {
            let mut k = base.index();
            for n in 0..len {
                if self.tmpl[k] == Some(StyleWord::TOMBSTONE) {
                    self.style_log.book.sweep.probed(n + 1);
                    self.style_log.book.sweep.tomb_taken();
                    return self.write_slot(StyleId::from_index(k), Some(raw));
                }
                k = (k + 1) & (len - 1);
            }
            self.style_log.book.sweep.probed(len);
        }
        let book = &self.style_log.book;
        if book.sweep.due(book.writes, len) {
            if let Some(id) = self.sweep_styles(raw) {
                return id;
            }
        }
        self.style_log.book.sweep.evicted();
        self.write_slot(StyleId::from_index(base.index().max(1)), Some(raw))
    }

    /// **The sweep.** Marks every slot a cell names -- every cell of the live
    /// surface's buffer (its screen, the primary's history and the rows held
    /// past a narrower edge) and of the other surface's, blanks past a row's
    /// extent included, since an erase leaves its rendition there -- and
    /// slot 0 and the current rendition. Nothing else holds a slot: `Saved`
    /// holds a word and `restore` interns it again, and the three-byte cell
    /// table's cells name slot 0 (`Term::fill_cells3`). Every other filled
    /// slot is freed: emptied, or made a [`StyleWord::TOMBSTONE`] where it
    /// lies on a live rendition's probe path, so that rendition stays
    /// findable. Ids never move and no cell is rewritten, and every slot
    /// that changes is logged, so a reader replaying the log holds the table
    /// (`Term::styles_written`). Each bucket's [`Reach`] is set again from
    /// the renditions kept. Then `raw` takes the first tombstone or empty
    /// slot on its path; `None` when there is neither.
    ///
    /// **The tombstones in one pass.** A probe path never crosses an empty
    /// slot, so a slot is on some live rendition's path exactly when a live
    /// rendition above it (cyclically) has its bucket at or below it. Walking
    /// the table downwards with `need`, how many slots below the current one
    /// a live rendition above still probes through, decides each slot in
    /// constant time; the walk goes round twice, the first lap only to learn
    /// the paths that wrap past the top. Two laps of the table, where asking
    /// each live rendition's path in turn is quadratic at full load.
    ///
    /// `#[cold]` and out of line: on the app's provisioning it walks the two
    /// surfaces' cells, about 940,000 of them, and two laps of 65,536 slots.
    #[cold]
    #[inline(never)]
    fn sweep_styles(&mut self, raw: StyleWord) -> Option<StyleId> {
        let len = self.tmpl.len();
        let mut live = SlotSet::new();
        live.insert(StyleId::DEFAULT);
        live.insert(self.style);
        live.mark(self.view.ring_cells());
        live.mark(self.other.cells);
        // The status line's row and the parked display's rendition: the
        // line, or the main display while the line is written.
        live.mark(self.status.cells);
        live.insert(self.strs.status.parked_style());
        let (mut freed, mut filled, mut tombs) = (0, 0, 0);
        let mut need = 0usize;
        // Each bucket's reach again, from the renditions the sweep keeps.
        self.style_log.reach.fill(Reach::ZERO);
        for step in 0..2 * len {
            let k = (len - 1) - (step & (len - 1));
            let was = self.tmpl[k];
            let named = live.has(k) && was.is_some();
            if step >= len && k != 0 {
                let now = match was {
                    _ if named => was,
                    Some(_) if need > 0 => Some(StyleWord::TOMBSTONE),
                    _ => None,
                };
                if now.is_some() {
                    filled += 1;
                }
                if now == Some(StyleWord::TOMBSTONE) {
                    tombs += 1;
                }
                if was.is_some_and(|w| w != StyleWord::TOMBSTONE) && now != was {
                    freed += 1;
                }
                if now != was {
                    self.write_slot(StyleId::from_index(k), now);
                }
            }
            let reach = match was {
                Some(w) if named => k.wrapping_sub(StyleId::bucket(w, len)) & (len - 1),
                _ => 0,
            };
            if step >= len {
                if let Some(w) = was.filter(|w| named && *w != StyleWord::TOMBSTONE) {
                    let at = &mut self.style_log.reach[StyleId::bucket(w, len)];
                    *at = (*at).max(Reach::of(reach));
                }
            }
            need = need.saturating_sub(1).max(reach);
        }
        let now = self.style_log.book.writes;
        self.style_log.book.sweep.swept(now, freed, filled, tombs);
        let mut k = StyleId::bucket(raw, len);
        for _ in 0..len {
            match self.tmpl[k] {
                Some(w) if w == StyleWord::TOMBSTONE => {
                    self.style_log.book.sweep.tomb_taken();
                    return Some(self.write_slot(StyleId::from_index(k), Some(raw)));
                }
                None => {
                    self.style_log.book.sweep.filled_one();
                    return Some(self.write_slot(StyleId::from_index(k), Some(raw)));
                }
                Some(_) => k = (k + 1) & (len - 1),
            }
        }
        None
    }

    /// **Slot `id` written, and the write logged**: a rendition interned, a
    /// rendition evicting another, or a sweep's free. Every write to the
    /// table goes through here, so the log holds every slot a renderer has
    /// to resolve again (`Term::styles_written`), and every rendition's
    /// bucket reaches it ([`Reach`]).
    #[inline(always)]
    fn write_slot(&mut self, id: StyleId, w: Option<StyleWord>) -> StyleId {
        self.tmpl[id.index()] = w;
        // A rendition written past its bucket raises the bucket's reach, so
        // a probe from there reads as far as it.
        if let Some(r) = w.filter(|r| *r != StyleWord::TOMBSTONE) {
            let len = self.tmpl.len();
            let home = StyleId::bucket(r, len);
            let at = &mut self.style_log.reach[home];
            *at = (*at).max(Reach::of(id.index().wrapping_sub(home) & (len - 1)));
        }
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
        // **Level 1 has no colour and no "off" forms**, out of line: one
        // compare a sequence, never a byte (`Paint::apply_sgr_level_one`).
        if self.cold.level == ConformanceLevel::One {
            return self.apply_sgr_level_one(csi);
        }
        csi.fold_sgr(&mut self.style_raw);
        self.style = self.intern_style(self.style_raw);
    }

    /// SGR at Level 1: each attribute the level defines folded, the rest
    /// skipped and counted (`baud_domain::sgr_introduced`), so `CSI 1 ; 31 m`
    /// sets bold and leaves the colour. `[[source: dec-std-070]]` SGR note 1:
    /// "All other parameter values shall be ignored unless they are part of a
    /// well defined extension to the architecture"; colour is §5.6.3's, "at
    /// Level 2 or higher".
    #[cold]
    #[inline(never)]
    fn apply_sgr_level_one(&mut self, csi: &crate::write::Csi) {
        let at = self.cold.level;
        let mut skipped = 0usize;
        csi.fold_sgr_where(&mut self.style_raw, |v| {
            let ok = sgr_introduced(v).admits(at);
            if !ok {
                skipped += 1;
            }
            ok
        });
        self.unsupported_param += skipped;
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
        let next = self.gl_set();
        if next != self.charset {
            self.keep_repeated_set();
        }
        self.charset = next;
        self.refresh_flags();
    }

    /// **The set GL draws by**: the one it holds, but ASCII drawn as DEC
    /// 7-bit Hebrew while DECHEBM's Group 2 is active under 7-bit
    /// characters (`crate::hebrew_keys`).
    pub(crate) fn gl_set(&self) -> Charset {
        let held = self.cold.g[self.cold.gl.index()];
        match held == Charset::Ascii && self.hebrew_group_two() {
            true => Charset::National(Nrcs::Hebrew),
            false => held,
        }
    }

    /// **What REP repeats keeps the set it was received under** when GL's
    /// set changes. ECMA-48 8.3.103 repeats "the preceding character in the
    /// data stream, if it is a graphic character", one "that has a coded
    /// representation consisting of one or more bit combinations" (4.2.43),
    /// and does not settle a copy made after a control: "If the character
    /// preceding REP is a control function or part of a control function,
    /// the effect of REP is not defined by this Standard." The copy is the
    /// character as drawn, counted as the code it was received as, the text
    /// leaving it undefined. A copy under GL's pen took the new set's tag,
    /// so a German `[` drawn `Ä`, copied after `ESC ( B`, counted 0xC4
    /// where the original counts 0x5B. Resolved here, once a set change, so
    /// the write path stores the same constant pen it did.
    #[cold]
    #[inline(never)]
    fn keep_repeated_set(&mut self) {
        if let Some((cp, w, Pen::Gl)) = self.last_cp {
            self.last_cp = Some((cp, w, Pen::Kept(Received::of(self.charset))));
        }
    }

    /// [`Paint::invoke`] after a designation or a locking shift, which move
    /// only a slot or GL: **where the set GL resolves to is the one in
    /// force, nothing changes**, since the pen is built from that set, the
    /// protection and the link, and the other two refresh it themselves.
    /// ncurses' `sgr0` for xterm is `ESC ( B ESC [ m`, so `asciiquarium.raw`
    /// sends 10,487 designations of the set already in G0.
    #[inline(always)]
    pub(crate) fn reinvoke(&mut self) {
        if self.cold.g[self.cold.gl.index()] != self.charset {
            self.invoke();
        }
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
        let base = base.with_received(Received::of(self.charset));
        self.flags = base.with_link(self.link);
        // An open link sends every text write through the hook that marks
        // its row ([`Paint::overwrote`]).
        self.rehook();
    }

    /// REP: repeat the last graphic character. Bounded by the row, so
    /// `CSI 65535 b` cannot spin.
    ///
    /// **Each copy is a cell of its own and the machine is told the last one
    /// went past**, from the state a control leaves, since `CSI b` is one:
    /// `glyph` writes without asking it, so without this a continuation after
    /// `ab CSI b` broke from the copy it follows, where GB9 and GB9a join it,
    /// and `csi_final` leaves the parser in `Node::Ground` after it so
    /// `Act::EndCluster` does not undo it (`after_rep_the_cluster_goes_on`).
    /// The copy never joins what came before it, an open choice (note AB, Q24).
    ///
    /// **Says whether it wrote**: with no last graphic character, after `RIS`
    /// or before any text, it writes nothing and tells the machine nothing, so
    /// the control before it must stay in force
    /// (`a_rep_with_nothing_to_repeat_leaves_the_control_before_it_in_force`).
    pub(crate) fn repeat(&mut self, c: Count) -> Wrote {
        let Some((cp, w, pen)) = self.last_cp else {
            return Wrote::Nothing;
        };
        // The copy takes the pen of the character it copies: a UTF-8 `Ä`
        // under German is copied as itself, counting 0xC4, and a GL `[`
        // German drew as `Ä` counts 0x5B, under GL's pen while the set is
        // unchanged and under the set `invoke` kept for it once GL's set
        // changes.
        // Each copy is a graphic character, so one a SAPV form waits for
        // takes it or spends it, by SAPV's writer.
        for _ in 0..c.at_least_one().min(self.cols.index()) {
            match crate::variants::form_read(self.cold.modes) {
                crate::graph::FormRead::OneByOne => {
                    self.glyph_by::<crate::paint::Formed>(cp, w, pen)
                }
                crate::graph::FormRead::Runs => self.glyph_as(cp, w, pen),
            }
        }
        // Through `Classified::of`, the table `glyph` asks: `is_break` takes
        // the class from a trie of its own, which nothing else on the write
        // path reads, and calling it here linked 32 KB more `__const` into
        // the binary for an operation that runs once per `REP`.
        let cl = Classified::of(cp);
        self.cluster = ClusterBreak::after_control();
        let _ = self.cluster.is_break_classified(cl.class, cl.lookback);
        Wrote::Copies
    }

    // ---- tabs -----------------------------------------------------------

    pub(crate) fn reset_tabs(&mut self) {
        for (i, stop) in self.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
        self.strs.aligned.all_plain();
    }

    /// `HT` and `CHT`: the next stop, or the edge [`Paint::tab_end`] names
    /// when none comes first. At full width that is the last column; under
    /// side margins narrower than the screen it depends on where the cursor
    /// starts, and **that question is out of line**: carried in, a clamp took
    /// this function past what `act` inlines, and moving `HT` to a call
    /// reshaped the per-byte loop. A cursor at a pending wrap, the sentinel
    /// one past the edge, comes back to it, as at full width.
    pub(crate) fn tab_forward(&mut self, n: Count) {
        let last = self.cols.index() - 1;
        // The fence: on a double line its half's last column, which goes
        // out of line with the margins' case.
        let end = match self.line_fence().index() == last {
            true => last,
            false => return self.tab_forward_fenced(n),
        };
        // With the right margin on the last column no value is tagged.
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.raw().index();
            loop {
                if x >= end {
                    // Past the end is the sentinel: a wrap pending.
                    if x > end && self.tab_wraps_first() {
                        x = 0;
                        continue;
                    }
                    x = end;
                    break;
                }
                x += 1;
                if self.tabs[x] {
                    break;
                }
            }
            self.set_col(CursorCol::moved(Col::at_index(x)));
        }
    }

    /// **`?41`: a tab at a pending wrap takes the wrap first**, xterm's
    /// `TabNext` (`[[source: xterm-tabs-c]]` 113-118): with the curses
    /// resource set, a wrap pending and `DECAWM` set, `xtermIndex`, the
    /// screen's first column and the wrap reset, and the tab from there.
    /// So it is `IND` and column 1 even under `DECLRMM`, and the row left
    /// is not marked as continuing, where xterm's own `WrapLine` marks it:
    /// more(1)'s next line is a line of its own (a second ask split, so
    /// the first stands). Out of line, so an `HT` that finds no wrap
    /// pending reads nothing new.
    #[cold]
    #[inline(never)]
    pub(crate) fn tab_wraps_first(&mut self) -> bool {
        let wraps = self.cold.more_fix == baud_domain::MoreFix::On
            && self.cold.modes.is_set(ModeBit::AutoWrap);
        if wraps {
            self.index_bringing_in();
            self.set_col(CursorCol::moved(Col::ZERO));
        }
        wraps
    }

    /// `HT` under a right margin left of the last column, from the column
    /// placed or not, out of line with [`Paint::tab_end`]. A cursor placed
    /// one past the margin that no stop moves keeps its tag.
    #[cold]
    #[inline(never)]
    fn tab_forward_fenced(&mut self, n: Count) {
        if matches!(self.cursor_at(), CursorAt::Pending(_)) {
            self.tab_wraps_first();
        }
        let end = self.tab_end().index();
        let mut x = self.cx.col().index();
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            loop {
                if x >= end {
                    x = end;
                    break;
                }
                x += 1;
                if self.tabs[x] {
                    break;
                }
            }
        }
        self.set_col(self.place(Col::at_index(x)));
    }

    /// Where `HT` stops when no tab stop comes first, under a right margin
    /// left of the last column. `[[source: dec-std-070]]` HT note 1 (p.
    /// 5-67): "If no tabulation stop is reached before the Right Margin of
    /// the Active Line (within the Scroll Area), the Active Position will be
    /// set to the Right Margin of the Active Line. If the Active Column is
    /// beyond the Right Margin or the Active Line is outside the Scroll Area
    /// (due to absolute cursor positioning), the Active Position will move to
    /// the next tab stop, or last column of the Active Line, whichever comes
    /// first."
    ///
    /// **The scroll area includes its margin rows**, as note 1 and BS's
    /// algorithm (p. 5-63) have it; HT's own algorithm (p. 5-68) compares
    /// strictly, which would put them outside. **A pending wrap is on the
    /// right margin**: with `DECAWM` set the sentinel one past it reads as
    /// on it, as [`crate::Term::wrap_pending`] reads it; with it reset no
    /// wrap is pending, so one past the margin is beyond it. Under DECOM
    /// every move keeps the cursor inside the margins (the `?6` row of
    /// `docs/vt-coverage.md`), so the right margin stops it with no test of
    /// the mode here.
    #[cold]
    #[inline(never)]
    fn tab_end(&self) -> Col {
        // A double line ends at its half, which is where `HT` stops when no
        // stop comes first: `active_position.column < end_of_line(...)`
        // (p. 5-68). Side margins and a sized line never meet (p. 5-29).
        let fence = self.line_fence();
        if fence != self.cold.right {
            return fence;
        }
        let right = self.cold.right;
        let y = self.cy.index();
        let inside =
            y >= self.cold.top.index() && y <= self.cold.bot.index() && self.at_or_inside_right();
        match inside {
            true => right,
            false => Col::at_index(self.cols.index() - 1),
        }
    }

    /// `BS` where the column is at or left of the left margin. `[[source:
    /// dec-std-070]]` BS, p. 5-63, note 1: "If the Active Position is at or
    /// inside the Left Margin and within the Scroll Area, it will not move
    /// beyond the Left Margin"; note 2: "If the Active Position is already
    /// before the Left Margin, the Active Position will move left one column
    /// unless it is at the first column of the Active Line." Its algorithm
    /// moves a cursor on the margin too when its line is above the top margin
    /// or below the bottom. **Where it does not move, `?45` reverse-wraps**
    /// into a wrapped line above ([`Paint::back_reverse_wrapping`]).
    ///
    /// At full width the left margin is column 0, so only a cursor there
    /// arrives, and it does not move.
    #[cold]
    #[inline(never)]
    pub(crate) fn back_space_at_left(&mut self) {
        let x = self.cx.col().index();
        let y = self.cy.index();
        let free =
            x < self.cold.left.index() || y < self.cold.top.index() || y > self.cold.bot.index();
        if free && x > 0 {
            self.set_col(CursorCol::moved(Col::at_index(x - 1)));
        } else if self.wraps_back_extended() {
            // **`?1045` wraps past the top margin too**, to the bottom
            // margin's line, and needs `?7`, as xterm's `REVERSEWRAP2` acts
            // only with `WRAPAROUND` (`[[source: xterm-cursor-c]]`,
            // `CursorBack`). Above the top margin it goes up a line, and
            // not off the screen.
            let to = match y {
                y if y == self.cold.top.index() => Some(self.cold.bot),
                0 => None,
                y => Some(Row::from_index(y - 1)),
            };
            if let Some(row) = to {
                self.set_row(row);
                self.set_col(CursorCol::moved(self.line_fence()));
            }
        } else if self.wraps_back() {
            let (to, x) = self.reverse_walk(Count::ONE);
            if to != y {
                self.set_row(Row::from_index(to));
            }
            self.set_col(CursorCol::moved(Col::at_index(x)));
        }
    }

    /// Whether `?45` reverse-wraps: set, with `?7` set, xterm's `rev`,
    /// `REVERSEWRAP` and `WRAPAROUND` both (`[[source: xterm-cursor-c]]`,
    /// `CursorBack`). It acted without `?7` until 2026-10-20.
    #[inline(always)]
    pub(crate) fn wraps_back(&self) -> bool {
        self.cold.modes.is_set(ModeBit::ReverseWrap) && self.cold.modes.is_set(ModeBit::AutoWrap)
    }

    /// **`CUB` under `?45`**: the walk `BS` takes, a count of columns back
    /// ([`Paint::reverse_walk`]), as xterm's `CursorBack` serves both
    /// (`[[source: xterm-reset]]`, `CASE_CUB`). It stopped at the left
    /// margin until 2026-10-20.
    #[cold]
    #[inline(never)]
    pub(crate) fn back_reverse_wrapping(&mut self, n: Count) {
        let (y, x) = self.reverse_walk(n);
        if y != self.cy.index() {
            self.set_row(Row::from_index(y));
        }
        self.set_col(CursorCol::placed(
            Col::at_index(x),
            self.cold.right,
            self.cols,
        ));
    }

    /// **Where `?45`'s walk back of `n` columns ends**, as `(line, column)`,
    /// xterm's `CursorBack` with `rev` and not `rev2`
    /// (`[[source: xterm-cursor-c]]` 121-191): a step past the left margin
    /// goes to the end of the line above only when that line is marked
    /// wrapped, its text having run on into this one (`LineTstWrapped`), and
    /// otherwise the walk stops at the left margin. **No top margin bounds it
    /// while `DECOM` is reset**: only an unwrapped line does, and the
    /// screen's first line, where xterm would read a line of its history.
    /// **Under `DECOM` the walk stops at the top margin**, which confines all
    /// cursor movement to the region (`[[source: vt510-decom]]`); xterm's
    /// `rev` reads no `ORIGIN` and would leave it, not followed. A failed
    /// step from below the region leaves the cursor on the line above it,
    /// xterm's `if (row < bottom) ++row; /* reverse-wrap failed */` as
    /// written. From left of the left margin the walk turns at the screen's
    /// first column, as xterm lets it go there; within a line it moves as
    /// `CUB` does, a pending wrap included, as `?1045`'s walk does
    /// ([`Paint::back_wrapping`]).
    fn reverse_walk(&self, n: Count) -> (usize, usize) {
        let start = self.cx.col().index();
        let left = match start < self.cold.left.index() {
            true => 0,
            false => self.cold.left.index(),
        };
        let bot = self.cold.bot.index();
        // The highest line the walk may reach: under `DECOM` the top
        // margin's, else the screen's first.
        let first = match self.cold.modes.is_set(ModeBit::Origin) {
            true => self.cold.top.index(),
            false => 0,
        };
        let (mut x, mut y) = (start, self.cy.index());
        let mut steps = n.at_least_one();
        loop {
            let room = x - left;
            if steps <= room {
                x -= steps;
                break;
            }
            steps -= room + 1;
            if y <= first {
                x = left;
                break;
            }
            let above = y - 1;
            if !self.view.extent(Row::from_index(above)).wrapped() {
                if above >= bot {
                    y = above;
                }
                x = left;
                break;
            }
            y = above;
            x = crate::paint::fence_of(
                self.view.rendition(Row::from_index(y)),
                self.cold.right,
                self.cols,
            )
            .index();
        }
        (y, x)
    }

    /// Whether `?1045` reverse-wraps: set, with `?7` set, as xterm's
    /// `REVERSEWRAP2` acts only with `WRAPAROUND`.
    #[inline(always)]
    pub(crate) fn wraps_back_extended(&self) -> bool {
        self.cold.resources.is_set(Resource::ReverseWrapExtended)
            && self.cold.modes.is_set(ModeBit::AutoWrap)
    }

    /// **`CUB` under `?1045`: a count of columns back, each step past the
    /// left margin to the end of the line above, and from the top margin
    /// to the bottom margin's line**, xterm's `CursorBack` with
    /// `REVERSEWRAP2` (`[[source: xterm-cursor-c]]` 121-190), which serves
    /// `CUB` as it serves `BS` (`[[source: xterm-reset]]`, `CASE_CUB`).
    /// From left of the left margin the walk turns at the screen's first
    /// column, as xterm lets it go there. Within a line it moves as `CUB`
    /// without the mode does, a pending wrap included, as the original
    /// `CursorBack` counts one (`[[source: xterm-x11r5-cursor-c]]` 81-84),
    /// where today's takes it off twice. Above the top margin it goes up a
    /// line and not off the screen, as `BS` does
    /// ([`Paint::back_space_at_left`]).
    ///
    /// **A whole lap of the region is taken off the count** the first time
    /// the walk turns from the top margin to the bottom, so `CSI 65535 D`
    /// walks at most two laps rather than a line at a time.
    #[cold]
    #[inline(never)]
    pub(crate) fn back_wrapping(&mut self, n: Count) {
        let start = self.cx.col().index();
        let left = match start < self.cold.left.index() {
            true => 0,
            false => self.cold.left.index(),
        };
        let (top, bot) = (self.cold.top.index(), self.cold.bot.index());
        let end = |p: &Self, y: usize| {
            crate::paint::fence_of(p.view.rendition(Row::from_index(y)), p.cold.right, p.cols)
                .index()
        };
        let (mut x, mut y) = (start, self.cy.index());
        let mut steps = n.at_least_one();
        let mut lapped = false;
        loop {
            let room = x - left;
            if steps <= room {
                x -= steps;
                break;
            }
            steps -= room + 1;
            y = match y {
                y if y == top => {
                    if !lapped {
                        lapped = true;
                        let lap: usize = (top..=bot).map(|r| end(self, r) - left + 1).sum();
                        steps %= lap;
                    }
                    bot
                }
                0 => {
                    x = left;
                    break;
                }
                y => y - 1,
            };
            x = end(self, y);
        }
        if y != self.cy.index() {
            self.set_row(Row::from_index(y));
        }
        self.set_col(CursorCol::placed(
            Col::at_index(x),
            self.cold.right,
            self.cols,
        ));
    }

    /// `BS` from outside the margins: at or left of the left one, or right
    /// of the right one, the sentinel and a placed cursor included. With
    /// `?69` reset no value is tagged and the left margin is column 0, so
    /// one column left unless at 0, as before the tag.
    #[cold]
    #[inline(never)]
    pub(crate) fn back_space_outside(&mut self) {
        match self.cold.modes.is_set(ModeBit::SideMargins) {
            true => self.back_space_fenced(),
            false => match self.cx.raw() > Col::ZERO {
                true => self.set_col(self.cx.before()),
                false => self.back_space_at_left(),
            },
        }
    }

    /// `BS` under `DECLRMM`: one column left from right of the left
    /// margin, where it can land one past the right margin and is placed
    /// there; at or left of the left margin, [`Paint::back_space_at_left`].
    /// Out of line, as `CUB` is, because a cursor right of the fence is a
    /// column the arm's test cannot tell from the sentinel.
    #[cold]
    #[inline(never)]
    fn back_space_fenced(&mut self) {
        let x = self.cx.col().index();
        match x > self.cold.left.index() {
            true => {
                let c = Col::at_index(x - 1);
                self.set_col(CursorCol::placed(c, self.cold.right, self.cols));
            }
            false => self.back_space_at_left(),
        }
    }

    /// `CBT`, clamped to the left margin under DECOM, the clamp out of line
    /// for the reason `HT`'s edge is.
    pub(crate) fn tab_back(&mut self, n: Count) {
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.col().index();
            loop {
                if x == 0 {
                    break;
                }
                x -= 1;
                if self.tabs[x] {
                    break;
                }
            }
            // A stop one past the right margin places the cursor there.
            self.set_col(self.place(Col::at_index(x)));
        }
        if self.cold.modes.is_set(ModeBit::Origin) {
            self.confine_cursor_col();
        }
    }

    /// The cursor's column back inside the margins under DECOM, after a tab
    /// walk that went past one.
    #[cold]
    #[inline(never)]
    fn confine_cursor_col(&mut self) {
        let c = self.confine_col(self.cx.col());
        self.set_col(self.place(c));
    }

    /// TBC, and CTC 2 to 6. **2 is 3 here**: a character stop is in every
    /// line under TSM MULTIPLE, the one state baud has, so the stops "in the
    /// active line" are all of them (`[[source: ecma48]]` 8.3.154, "the
    /// number of lines affected depends on the setting of" TSM).
    pub(crate) fn tab_clear(&mut self, w: Option<TabClear>) {
        match w {
            Some(TabClear::All | TabClear::InLine) => {
                self.tabs.fill(false);
                self.strs.aligned.all_plain();
            }
            Some(TabClear::AtCursor) => {
                let x = self.stop_col().index();
                if x < self.tabs.len() {
                    self.tabs[x] = false;
                    self.strs.aligned.plain_at(Col::at_index(x));
                }
            }
            Some(TabClear::LineAtCursor) => self.cold.line_stops.clear(self.cy),
            Some(TabClear::AllLines) => self.cold.line_stops.clear_all(),
            Some(TabClear::Everything) => {
                self.tabs.fill(false);
                self.strs.aligned.all_plain();
                self.cold.line_stops.clear_all();
            }
            // A value 8.3.154 does not define: counted, and no stop moves.
            // `None` once cleared the stop at the cursor, so `CSI 1 g` -- a
            // request about a line stop -- removed a character stop.
            None => self.unsupported_param += 1,
        }
    }

    /// **TSR**, `CSI Pn SP d` (`[[source: ecma48]]` 8.3.156): "any character
    /// tabulation stop at character position n in the active line ... and
    /// lines of subsequent text in the presentation component to be cleared,
    /// but does not affect other tabulation stops". A character stop is in
    /// every line under TSM MULTIPLE, so the stop at column `n` goes from
    /// them all; a position past the line holds none. Position `n` is a
    /// presentation position, the column `HTS` sets a stop at. `Pn` has no
    /// default, so an omitted or zero one clears nothing and is counted.
    pub(crate) fn remove_stop(&mut self, n: Option<baud_domain::OneBased>) {
        let Some(n) = n else {
            self.unsupported_param += 1;
            return;
        };
        if let Some(c) = n.col_within(self.cols) {
            self.tabs[c.index()] = false;
            self.strs.aligned.plain_at(c);
        }
    }

    /// **HTJ**, `ESC I` (`[[source: ecma48]]` 8.3.61): "the contents of the
    /// active field ... to be shifted forward so that it ends at the
    /// character position preceding the following character tabulation
    /// stop", the positions before it "put into the erased state", and the
    /// cursor to that stop, where `HT` takes it. The field is `EF`'s, from
    /// the stop at or before the cursor to the next, bounded by the line
    /// (4.2.40); its contents run from its start through its last position
    /// not in the erased state, wherever the cursor is in it. So the shift
    /// is an `ICH` at the field's start of the erased positions after its
    /// contents, within the field: nothing leaves it. The stops are found
    /// at the cursor's presentation position, as `HT` finds them, and with
    /// BDSM reset on a line holding a string the content drawn at the
    /// field's positions moves (`crate::extent`).
    #[cold]
    #[inline(never)]
    pub(crate) fn tab_justify(&mut self) {
        let r = self.cy;
        let end = self.line_cols().index();
        let x = self.stop_col().index().min(end - 1);
        let start = (0..=x).rev().find(|&c| self.tabs[c]).unwrap_or(0);
        let stop = (x + 1..end).find(|&c| self.tabs[c]).unwrap_or(end);
        // With BDSM reset on a line holding a string the positions are the
        // ones drawn, and their content moves.
        let mut d = crate::component::Drawn::presented(self.cold.modes);
        let written = (start..stop).rev().find(|&p| {
            let c = self.drawn_at(&mut d, r, Col::at_index(p));
            !self.view.get(r, c).content.is_empty()
        });
        if let Some(last) = written {
            let n = stop - (last + 1);
            let (at, to) = (Col::at_index(start), Col::at_index(stop));
            match (n > 0, d.is_drawn() && self.line_map(r).is_some()) {
                (false, _) => {}
                (true, true) => {
                    let run = crate::extent::Run {
                        from: (r, at),
                        to: (r, to.before()),
                        left: at,
                        right: to.before(),
                    };
                    let end = crate::extent::Toward::End;
                    let ins = crate::editing::CharEdit::Insert;
                    self.shift_run(&run, Count::of(n), ins, end, &mut d);
                }
                (true, false) => {
                    let b = self.blank();
                    self.view.insert_cells(r, at, Count::of(n), to, b);
                    self.erase_images(r, at, to);
                    self.cells = self.cells.plus(stop - start);
                    self.row_changed_to(r);
                    self.wrote_rows(r, r);
                }
            }
        }
        match self.cold.modes.is_set(ModeBit::ReturnMapped) {
            true => self.tab_mapped(),
            false => self.tab_forward(Count::ONE),
        }
    }

    /// HTS, `ESC H`, and CTC 0: a character stop at the cursor's column.
    pub(crate) fn tab_set(&mut self) {
        let x = self.stop_col().index();
        if x < self.tabs.len() {
            self.tabs[x] = true;
            self.strs.aligned.plain_at(Col::at_index(x));
        }
    }

    /// **The column a stop is set or cleared at**: "the active
    /// presentation position" (`[[source: ecma48]]` 8.3.62, 8.3.154,
    /// 8.3.17), with BDSM reset the cursor's position as drawn, in both
    /// DCSM states (`crate::component`), its cell's column otherwise.
    ///
    /// **A wrap waiting at the line limit, the last column or a right
    /// margin names the column it waits on**, which a report names: the
    /// active position is not advanced from "the last character position
    /// of the active line" (6.1.6), DEC's Last Column Flag leaves it there
    /// and its HTS sets the stop at "the Active Column"
    /// (`[[source: dec-std-070]]` D.6.1, p. 5-69), and xterm's `TabSet()`
    /// and `TabClear()` take the cursor column, which stays there while
    /// the wrap waits. The wrap still waits: HTS is not among the controls
    /// D.6.1 lists.
    fn stop_col(&self) -> Col {
        let c = self.cx.reported(self.report_fence(), self.cols);
        match self.cold.modes.is_set(ModeBit::CursorPresented) {
            true => self.line_map(self.cy).map_or(c, |m| m.presentation(c)),
            false => c,
        }
    }

    /// CTC, `CSI Ps... W`: every parameter, in order ("Notation: (Ps...)",
    /// `[[source: ecma48]]` 8.3.17); a value it does not define is counted.
    pub(crate) fn tab_control(&mut self, csi: &crate::write::Csi) {
        csi.for_each_param(|p| match TabControl::from_param(p) {
            Some(TabControl::SetHere) => self.tab_set(),
            Some(TabControl::SetLine) => self.cold.line_stops.set(self.cy),
            Some(TabControl::Clear(w)) => self.tab_clear(Some(w)),
            None => self.unsupported_param += 1,
        });
    }

    /// The last line a line tabulation stop is read to from the cursor's:
    /// the bottom margin inside the scrolling region, the last line outside
    /// it -- HT's rule from `[[source: dec-std-070]]`'s note 1, turned
    /// vertical, which CVT's text leaves open. A region is its rows here:
    /// the side margins bound no line.
    fn line_tab_end(&self) -> Row {
        let y = self.cy.index();
        match y >= self.cold.top.index() && y <= self.cold.bot.index() {
            true => self.cold.bot,
            false => Row::from_index(self.rows.index() - 1),
        }
    }

    /// CVT, `CSI Pn Y` (`[[source: ecma48]]` 8.3.23): to "the line
    /// corresponding to the n-th following line tabulation stop", the column
    /// kept, and to [`Paint::line_tab_end`] when the stops run out first.
    /// A cursor movement: it never scrolls.
    pub(crate) fn line_tab(&mut self, n: Count) {
        let end = self.line_tab_end();
        let mut y = self.cy;
        for _ in 0..n.at_least_one().min(self.rows.index()) {
            match self.cold.line_stops.next_after(y, end) {
                Some(next) => y = next,
                None => {
                    y = end.max(y);
                    break;
                }
            }
        }
        self.set_row(y);
    }

    /// VT, `0x0B` (`[[source: ecma48]]` 8.3.161): to "the corresponding
    /// character position on the line at which the following line
    /// tabulation stop is set", read to [`Paint::line_tab_end`]; with none
    /// following, `LF`, as DEC's table treats VT. Out of line: the
    /// per-byte loop's arm is one call, and its own table entry rather
    /// than a test in `LF`'s.
    #[cold]
    #[inline(never)]
    pub(crate) fn vertical_tab(&mut self) {
        match self
            .cold
            .line_stops
            .next_after(self.cy, self.line_tab_end())
        {
            Some(y) => {
                // A line ends where the cursor leaves it, as at `LF`, and
                // autoprint prints it, ended by `VT` (070-7 § 7.3.7).
                if self.cold.modes.is_set(ModeBit::AutoPrint) {
                    self.autoprint(crate::print::LineEnd::LineTab);
                }
                self.view.set_ended(self.cy);
                // With BDSM reset "the corresponding character position"
                // is the presentation position, in both DCSM states.
                let kept = self.cursor_presented_col();
                self.set_row(y);
                self.keep_presented(kept);
            }
            None => self.line_feed(crate::paint::Feed::LineTab),
        }
    }

    // ---- margins and columns -------------------------------------------

    /// Ask the embedder for a page width, and under [`WidthHold::On`] stop
    /// the read after this control ([`Paint::hold_read`]). The first
    /// request since power-on or `RIS` also notes the grid's width, which
    /// `RIS` asks back for.
    pub(crate) fn ask_width(&mut self, w: PageWidth, node: &mut crate::graph::Node) {
        let a = &mut self.cold.width;
        a.requests = a.requests.next();
        a.last = Some(WidthRequest::Page(w));
        if a.home.is_none() {
            a.home = Some(self.cols);
        }
        if a.hold == WidthHold::On {
            self.hold_read(node);
        }
    }

    /// Stop the read after this control: the parser goes to `Node::Held`,
    /// whose every byte is the rest of the read, for the embedder to feed
    /// (`Term::take_held`), and the node it was in is the one the rest goes
    /// on from. **Only the first stop in a sequence saves it**: a second,
    /// in `CSI ? 3 ; 3 h`, finds the parser already `Held`, and saving that
    /// would park the rest for good. A page-width request stops here, and
    /// a change of `?46` (`crate::logging`).
    pub(crate) fn hold_read(&mut self, node: &mut crate::graph::Node) {
        if *node != crate::graph::Node::Held {
            self.cold.width.resume = *node;
        }
        *node = crate::graph::Node::Held;
    }

    /// **Stop the read for its answers to go out**, under [`ReplyHold::On`],
    /// once those queued leave less room than one control can owe
    /// (`REPLY_MOST`): the embedder sends them and feeds the rest, so the
    /// next control's answer always fits. Called after every control that
    /// can answer -- the arms of `csi_final` that may, DECID in
    /// `esc_final`, VT52's identify in `Paint::vt52`, every string's
    /// terminator in `Paint::str_end` -- and between two ReGIS commands
    /// (`Paint::regis_hold`), never per byte. One load and a compare when
    /// the room is there.
    #[inline(always)]
    pub(crate) fn hold_for_replies(&mut self, node: &mut crate::graph::Node) {
        if self.replies_low() {
            self.hold_read(node);
        }
    }

    /// Whether the answers queued leave less room than one control can owe,
    /// with the hold on. **Against `REPLY`, a constant**, which the buffer
    /// is at least (`Storage::new` asserts it): one load and a compare when
    /// the room is there, where the buffer's own length was a second load.
    #[inline(always)]
    pub(crate) fn replies_low(&self) -> bool {
        self.reply_len.index() > crate::storage::REPLY - crate::storage::REPLY_MOST
            && self.cold.width.reply_hold == ReplyHold::On
    }

    /// DECSCPP, `CSI Pn $ |`: the width asked for and nothing else here.
    /// `[[source: dec-std-070]]` p. 5-73: it "does not cause the Active
    /// position to move" and "does not cause the Right Margin to move",
    /// except past the new page's edge, and "When changing from a 132 column
    /// to a 80 column page, the information in columns 1-80 is preserved" --
    /// all of which is the resize's, when the embedder makes it. **`?3` is
    /// not moved**: `set_columns_per_page` (p. 5-74) changes
    /// `columns_per_page` and not `column_mode`, where xterm's `CASE_DECSCPP`
    /// sets `IN132COLUMNS`. **Not gated by `?40`**, as neither STD 070 nor
    /// xterm gates it.
    pub(crate) fn set_columns_per_page(&mut self, w: PageWidth, node: &mut crate::graph::Node) {
        self.ask_width(w, node);
    }

    /// The margins a `RIS`, a `DECSTR` or a resize puts back.
    ///
    /// **A cursor placed one past the old right margin keeps its column**,
    /// an ordinary one now: the tag goes with the margin it was set
    /// against, so no tagged value is left at full width, where
    /// [`Paint::place`] trusts there is none.
    pub(crate) fn reset_side_margins(&mut self) {
        self.cold.left = Col::ZERO;
        self.cold.right = Col::at_index(self.cols.index() - 1);
        self.cold.fence = crate::bounds::limited(self.cold.right, self.cold, self.cols);
        self.cx = CursorCol::moved(self.cx.col());
        self.row_changed_to(self.cy);
    }

    /// **`DECLRMM` set: every line single-width**, on both screens.
    /// `[[source: dec-std-070]]` p. 5-29: "When DECLRMM is set, all line
    /// attributes currently in Page Memory for the Session will be reset to
    /// Single Wide, Single High", its algorithm setting `line_rendition[i] =
    /// SINGLE_WIDTH` for every line of the page memory, which is the screen
    /// of each surface and not the history behind it. A line keeps its
    /// cells: the reset is DECSWL's, which loses nothing (p. 5-96).
    #[cold]
    #[inline(never)]
    pub(crate) fn single_width_pages(&mut self) {
        // The page memory is the main display's, both its screens, from
        // the status line too.
        if self.on_status() {
            self.on_main(|p| p.single_width_pages());
            return;
        }
        for live in [true, false] {
            for y in 0..self.rows.index() {
                let r = Row::from_index(y);
                if self.view.rendition(r).is_double() {
                    self.view.set_rendition(r, LineRendition::Single);
                    // A watch on a selection's rows is the live screen's.
                    if live {
                        self.wrote_rows(r, r);
                    }
                }
            }
            self.view.swap_surface(&mut self.other);
        }
        self.cold.fence = crate::bounds::limited(self.cold.right, self.cold, self.cols);
    }

    /// `DECSLRM`, `CSI Pl ; Pr s`.
    ///
    /// `[[source: vt510-decslrm]]`: "The value of the left margin (Pl) must be
    /// less than the right margin (Pr) ... The minimum size of the scrolling
    /// region is two columns ... DECSLRM moves the cursor to column 1, line 1
    /// of the page." Under DECOM that page origin is the new margins' corner,
    /// `[[source: dec-std-070]]` DECSLRM note 1 ([`Paint::home`]).
    ///
    /// **An omitted or 0 `Pr` is the page's last column**: "default Pr: Last
    /// Column" (`[[source: dec-std-070]]` DECSLRM, p. 5-27, whose algorithm
    /// reads a 0 as omitted; [`Params::far_margin`]). It was decoded as
    /// column 1, so `CSI s` and `CSI 5 s` were dropped.
    ///
    /// **A `Pr` past the page leaves the margins alone**: STD 070's note 3
    /// says such a control "will be ignored (not executed)", and its
    /// algorithm (p. 5-28) sets the margins only when `parv[1] <=
    /// columns_per_page` (`OneBased::col_within`). `OneBased::to_col`
    /// clamped it to the last column until 2026-10-13, as xterm and ghostty
    /// do, so `CSI 5;99 s` set 5..20 on 20 columns.
    ///
    /// **An inverted or one-column pair is rejected rather than clamped**,
    /// leaving the margins alone, because clamping would confine the screen
    /// to a region the application did not ask for and then behave correctly
    /// with respect to it -- which is worse than doing nothing, since nothing
    /// is visible as a bug and a plausible wrong region is not.
    pub(crate) fn set_side_margins(&mut self, left: OneBased, right: Option<OneBased>) {
        let last = self.cols.index() - 1;
        let l = left.to_col(self.cols).index();
        let Some(r) = right.map_or(Some(last), |v| v.col_within(self.cols).map(Col::index)) else {
            return;
        };
        if l >= r {
            return;
        }
        self.cold.left = Col::at_index(l);
        self.cold.right = Col::at_index(r);
        // Only under `DECLRMM`, which no sized line survives (p. 5-29).
        self.cold.fence = crate::bounds::limited(self.cold.right, self.cold, self.cols);
        self.home();
    }

    /// Is the cursor inside the scrolling region, in **both** axes?
    ///
    /// `DECIC` and `DECDC` are defined only there, so the rectangle both
    /// margin pairs describe is what bounds them -- not the full height, which
    /// is what a vertical-only test would give.
    pub(crate) fn within_margins(&self) -> bool {
        let (x, y) = (self.cx.col().index(), self.cy.index());
        x >= self.cold.left.index()
            && x <= self.cold.right.index()
            && y >= self.cold.top.index()
            && y <= self.cold.bot.index()
    }

    /// SL and SR, `CSI Pn SP @` and `CSI Pn SP A` (`[[source: ecma48]]`
    /// 8.3.121 and 8.3.135): the rectangle the four margins bound moves left
    /// or right "and the active presentation position is not affected",
    /// whatever line or column the cursor is on. The columns brought in take
    /// the fill SU and SD give a line, [`Paint::blank`], DECECM's
    /// "background color used when text is erased or new text is scrolled on
    /// to the screen" (`[[source: vt520-rm]]` p. 5-48), where DECIC and
    /// DECDC take normal rendition by their own text.
    ///
    /// **A double-width line shifts to the right margin**, as DEC's own
    /// horizontal shifts move it: DECFI and DECBI shift "one column
    /// regardless of their line attributes", through scroll_left and
    /// scroll_right, which run every row of the region to the right margin
    /// (`[[source: dec-std-070]]` pp. 5-37, 5-39, 5-212 and 5-213), so what
    /// goes past its half is kept and DECSWL shows it again. It stopped at
    /// the half until then. Side margins and a sized line never meet (p.
    /// 5-29), so the left margin is column 1 on any line that is not
    /// single-width.
    pub(crate) fn scroll_columns(&mut self, n: Count, dir: ColumnShift) {
        self.edit_reaches(crate::aligned::EditRows::Region);
        let (left, b) = (self.cold.left, self.blank());
        self.shift_columns_at(left, n, dir, b);
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
        self.edit_reaches(crate::aligned::EditRows::Region);
        self.shift_columns(n, ColumnShift::Insert);
    }

    /// `DECDC`, `CSI Pn ' ~`. The inverse, filling at the right margin.
    pub(crate) fn delete_columns(&mut self, n: Count) {
        if !self.within_margins() {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Region);
        self.shift_columns(n, ColumnShift::Delete);
    }

    /// Move every row's columns between the cursor and the **right margin**.
    ///
    /// The `to` bound was `cols` before the margins existed, so an insert
    /// pushed text off the screen edge rather than off the margin -- correct
    /// only while the margin *is* the edge, which is every stream that never
    /// sends `DECSLRM`.
    ///
    /// **The columns brought in have normal rendition**, not the current
    /// one: "The inserted columns are set to blank with normal rendition"
    /// and "Columns containing blank characters with normal rendition are
    /// shifted into the display" (`[[source: dec-std-070]]` DECIC and
    /// DECDC, pp. 5-150 and 5-151); "blank columns with no visual
    /// character attributes" (`[[source: vt510-decic]]`,
    /// `[[source: vt510-decdc]]`). ICH and DCH take the current rendition.
    pub(crate) fn shift_columns(&mut self, n: Count, dir: ColumnShift) {
        let at = self.cx.col();
        self.shift_columns_at(at, n, dir, PackedCell::EMPTY);
    }

    /// The same from column `at`, filling with `b`: `DECBI` and `DECFI` move
    /// the rectangle from the left margin, filling with "all attributes
    /// turned off" (`crate::rect`), as DECIC and DECDC fill.
    pub(crate) fn shift_columns_at(&mut self, at: Col, n: Count, dir: ColumnShift, b: PackedCell) {
        let to = self.cold.right.after();
        for y in self.cold.top.index()..=self.cold.bot.index() {
            let r = Row::from_index(y);
            match dir {
                ColumnShift::Insert => self.view.insert_cells(r, at, n, to, b),
                ColumnShift::Delete => self.view.delete_cells(r, at, n, to, b),
            }
            self.cells = self.cells.plus(to.index() - at.index());
        }
        self.wrote_rows(self.cold.top, self.cold.bot);
        // DECIC moved the cursor's row right under it, past the mark.
        self.row_changed_to(self.cy);
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

    /// XTSAVE, `CSI ? Pm s` (`[[source: ctlseqs]]`): "Ps values are the same
    /// as for DECSET ... Only those modes listed as parameters are saved."
    ///
    /// **Each listed mode DECSET acts on keeps the value `DECRQM` would
    /// answer**, so what is saved is what an application asking would have
    /// been told, and one listed twice keeps its last value -- one level, as
    /// DECSC's is. A mode DECSET does not act on is counted, a parameter at a
    /// time, as DECSET's wildcard counts it: unnamed, the read-only `?1020`
    /// to `?1023`, the permanent `?2027` ([`PrivateMode::slot`]).
    ///
    /// **`?1048` saves the cursor**, as xterm's `savemodes` runs
    /// `CursorSave` for it (`[[source: xterm-reset]]`'s file): the mode
    /// is an action and has no value to keep.
    pub(crate) fn save_private_modes(&mut self, csi: &crate::write::Csi) {
        csi.for_each_param(|p| {
            let m = PrivateMode::from_wire(p);
            if m == PrivateMode::SaveCursor {
                if self.cold.resources.is_set(Resource::AltScreenAllowed) {
                    self.save_or_restore_cursor(ModeChange::Set);
                }
                return;
            }
            // **The keyboard family is one value**, kept whichever member
            // is listed (`SavedModes::save_keyboard`).
            if let PrivateMode::Keyboard(_) = m {
                self.cold.xtsave.save_keyboard(self.cold.keyboard);
                return;
            }
            let value = match self.private_state(m) {
                ModeState::Set => Some(ModeChange::Set),
                ModeState::Reset => Some(ModeChange::Reset),
                _ => None,
            };
            match (m.slot(), value) {
                (Some(slot), Some(v)) => self.cold.xtsave.save(slot, v),
                _ => self.unsupported_param += 1,
            }
        });
    }

    /// XTRESTORE, `CSI ? Pm r`: "The value of Ps previously saved is
    /// restored ... Only those modes listed as parameters are restored."
    ///
    /// **Through DECSET's own path**, so a restore has every effect setting
    /// or resetting that mode has -- `?1049` saves and clears or restores the
    /// cursor, `?6` homes, `?3` clears, `?2048` reports -- and `DECRQM`
    /// answers from the state it left, never from the cache. **Only a mode
    /// whose value differs from the one saved is set or reset**: a value
    /// already in force is restored, and replaying it would undo what a
    /// family's other members restored -- DECRST of any tracking level turns
    /// tracking off, so restoring `?1000` reset under `?1002` would drop the
    /// `?1002` restored before it -- and would clear an alternate screen that
    /// is already up. **A mode never saved restores nothing**: there is no
    /// value "previously saved". Where xterm's bespoke code and zeroed array
    /// differ (`restoremodes`, `[[source: xterm-reset]]`), baud keeps `?1049`
    /// and an equal value as here; the rest is open.
    pub(crate) fn restore_private_modes(
        &mut self,
        csi: &crate::write::Csi,
        node: &mut crate::graph::Node,
        at: BytePos,
    ) {
        csi.for_each_param(|p| {
            let m = PrivateMode::from_wire(p);
            // `?1048` restores the cursor, xterm's `restoremodes`.
            if m == PrivateMode::SaveCursor {
                if self.cold.resources.is_set(Resource::AltScreenAllowed) {
                    self.save_or_restore_cursor(ModeChange::Reset);
                }
                return;
            }
            // **The keyboard family comes back whole**, whichever member is
            // listed: through DECSET of the member saved, or a DECRST of the
            // member in force for the default keyboard, and only where it
            // differs. Never saved, nothing is restored.
            if let PrivateMode::Keyboard(_) = m {
                let now = self.cold.keyboard;
                match self.cold.xtsave.saved_keyboard() {
                    Some(k) if k != now => match k {
                        Keyboard::Default => self.set_private_mode(
                            PrivateMode::Keyboard(now),
                            ModeChange::Reset,
                            node,
                            at,
                        ),
                        k => self.set_private_mode(
                            PrivateMode::Keyboard(k),
                            ModeChange::Set,
                            node,
                            at,
                        ),
                    },
                    _ => {}
                }
                return;
            }
            let Some(slot) = m.slot() else {
                self.unsupported_param += 1;
                return;
            };
            // A mode with a slot that is not recognized now -- a Sixel mode
            // while DA does not list Sixel -- or is permanent now -- `?46`
            // while the embedder has logging off -- is counted, as its save
            // was.
            if !matches!(self.private_state(m), ModeState::Set | ModeState::Reset) {
                self.unsupported_param += 1;
                return;
            }
            let Some(want) = self.cold.xtsave.saved(slot) else {
                return;
            };
            let now = match self.private_state(m) {
                ModeState::Set => ModeChange::Set,
                _ => ModeChange::Reset,
            };
            if now != want {
                self.set_private_mode(m, want, node, at);
            }
        });
    }

    /// DECSC for a set, DECRC for a reset: `?1048`'s two halves, through
    /// the slot `ESC 7` and `ESC 8` use, a restore with nothing saved
    /// doing what `ESC 8`'s does.
    fn save_or_restore_cursor(&mut self, change: ModeChange) {
        match (change, self.cold.saved) {
            (ModeChange::Set, _) => self.cold.saved = Some(self.save()),
            (ModeChange::Reset, Some(s)) => self.restore(Some(s)),
            (ModeChange::Reset, None) => self.restore_unsaved(),
        }
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
    ///
    /// **And the cursor colour, before the bit moves**: one equal to the text
    /// shown becomes the page shown, which the flip then shows as the text
    /// ([`Video::cursor_across`], xterm's `ReverseVideo` rule, kept under the
    /// per-cell exchange because its reason holds there). Here because every
    /// change of `?5` comes through here, as every one of xterm's calls
    /// `ReverseVideo` -- `DECSCNM`, XTRESTORE and `RIS` alike.
    fn flip_reverse_video(&mut self, change: ModeChange, during: Update) {
        let was = self.cold.modes.is_set(ModeBit::ReverseVideo);
        let on = change == ModeChange::Set;
        if was != on {
            // The colour shown, a configured one included: xterm's rule acts
            // on its cursor colour whatever set it. Written only when it
            // moves, so a configured colour that does not stays configured.
            let shown = self.cold.shown(DynamicColor::Cursor);
            let across = Video::cursor_across(
                shown,
                self.cold.shown(DynamicColor::Foreground),
                self.cold.shown(DynamicColor::Background),
            );
            if across != shown {
                self.cold.dynamic[DynamicColor::Cursor.index()] = across;
            }
        }
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
        // **A mode the level in force does not define is not set**, and
        // counted, as any other parameter it does not act on.
        if !introduced_by(m.function()).admits(self.cold.level) {
            self.unsupported_param += 1;
            return;
        }
        let on = change == ModeChange::Set;
        match m {
            PrivateMode::Origin => {
                self.cold.modes.apply(ModeBit::Origin, on);
                // Both directions home the cursor, and where home *is* depends
                // on the mode just applied (`set_origin_mode` and
                // `reset_origin_mode`, `[[source: dec-std-070]]` p. 5-31).
                self.home();
            }
            // **The pin goes with the mode it was written under.** With
            // `DECAWM` set a write does not touch `Cold::pinned`, so a pin left
            // from before a `?7h` would name a character long since passed by
            // the time a `?7l` makes it readable again.
            //
            // **A wrap waiting at the line limit stays on the limit**: the
            // active position never left it (`crate::bounds`), and a report
            // names it with the wrap ended.
            PrivateMode::AutoWrap => {
                self.cold.modes.apply(ModeBit::AutoWrap, on);
                if self.cold.pinned != crate::term::Pinned::AtLimit {
                    self.cold.pinned = crate::term::Pinned::Before;
                }
            }
            // **`?3` DECCOLM, behind xterm's `?40`**. While `?40` is reset
            // the control is ignored, as xterm's `srm_DECCOLM` does nothing
            // without `c132`, and counted. Allowed, it is
            // `[[source: dec-std-070]]`'s `set_column_mode` and
            // `reset_column_mode` (p. 5-72): "Selecting Eighty or One Thirty
            // Two Column Mode causes the display data to be cleared, the
            // scrolling region to be eliminated, and the Active Position to
            // be set to the display origin (1,1), even if the terminal was
            // already in the selected state" (p. 5-71) -- the top, bottom,
            // left and right margins all reset. **The clear is `?95`'s to
            // withhold**: "When enabled, a column mode change ... does not
            // clear the screen" (`[[source: vt510-decncsm]]`). **The width is
            // asked for**, not taken: the window is the embedder's.
            //
            // **And DECLRMM is reset**: "DECCOLM resets vertical split screen
            // mode (DECLRMM) to unavailable" (`[[source: vt510-deccolm]]`).
            // STD 070's algorithm sets `left_right_margins_mode = SETABLE`
            // instead, and xterm leaves the mode as it was, calling that line
            // an error -- both readings stand, an open question. **On the
            // status line, DECCOLM is the main display's**, and clears the
            // line too: "DECCOLM affects both the Main Display and the Status
            // Display simultaneously ... Receipt of this sequence will clear
            // both" (`[[source: dec-std-070]]` 070-14 p. 14-9).
            PrivateMode::Column132 if self.on_status() => {
                self.on_main(|p| p.set_private_mode(m, change, node, at));
            }
            PrivateMode::Column132 => {
                if !self.cold.modes.is_set(ModeBit::AllowColumnMode) {
                    self.cold.width.refused = self.cold.width.refused.next();
                    return;
                }
                self.cold.modes.apply(ModeBit::Column132, on);
                self.cold.top = Row::ZERO;
                self.cold.bot = Row::from_index(self.rows.index() - 1);
                self.reset_side_margins();
                self.cold.modes.reset(ModeBit::SideMargins);
                if !self.cold.modes.is_set(ModeBit::NoClearOnColumn) {
                    self.clear_screen();
                    self.status_column_mode();
                }
                self.home();
                self.ask_width(PageWidth::of_column_mode(change), node);
            }
            // **`?40` and `?95` act through `?3`**, which reads both.
            PrivateMode::AllowColumnMode => self.cold.modes.apply(ModeBit::AllowColumnMode, on),
            PrivateMode::NoClearOnColumn => self.cold.modes.apply(ModeBit::NoClearOnColumn, on),
            // DECECM: read by `Paint::blank`, which every erase and every line
            // brought onto the screen takes its blank from.
            PrivateMode::EraseColor => self.cold.modes.apply(ModeBit::EraseScreenBackground, on),
            // **`?4` and `?8` are recorded and exposed, which is what acting
            // on them means here.** Smooth scroll is renderer policy and
            // auto-repeat is keyboard policy; this core has neither, and an
            // embedder cannot implement either without being told. Dropping
            // them is the "set and never read" failure in reverse -- a mode
            // parsed and discarded, which `docs/vt-coverage.md` rates worse
            // than not implementing it. **`?12` is a second input to the
            // blink, not a write to the style** -- foot's rule. It used to
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
                match on {
                    false => self.reset_side_margins(),
                    true => self.single_width_pages(),
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
            // DECNKM is DECKPAM and DECKPNM as a mode, one state with them.
            PrivateMode::NumericKeypad => {
                self.cold.keypad = match change {
                    ModeChange::Set => KeypadMode::Application,
                    ModeChange::Reset => KeypadMode::Numeric,
                }
            }
            PrivateMode::BackarrowKey => {
                self.cold.backarrow = match change {
                    ModeChange::Set => BackarrowKey::Backspace,
                    ModeChange::Reset => BackarrowKey::Delete,
                }
            }
            PrivateMode::NumLockModifiers => {
                self.cold.num_lock = match change {
                    ModeChange::Set => NumLockKeypad::Overrides,
                    ModeChange::Reset => NumLockKeypad::Ignored,
                }
            }
            PrivateMode::DeleteIsDel => {
                self.cold.delete = match change {
                    ModeChange::Set => DeleteKey::Del,
                    ModeChange::Reset => DeleteKey::Remove,
                }
            }
            // **One keyboard at a time**: DECSET selects the member, and a
            // DECRST of any member returns to the default keyboard, the
            // one in force or not, as xterm's `set_keyboard_type` stores
            // `keyboardIsDefault` for any reset (`[[source: xterm-scroll]]`'s
            // file, util.c 6077-6093).
            PrivateMode::Keyboard(k) => {
                self.cold.keyboard = match change {
                    ModeChange::Set => k,
                    ModeChange::Reset => Keyboard::Default,
                }
            }
            // **Every hide decides where a program placed the cursor**, a hide
            // of a hidden cursor too, and a show forgets it (`placed.rs`).
            PrivateMode::CursorVisible => {
                self.cold.modes.apply(ModeBit::CursorVisible, on);
                match on {
                    true => self.cold.placement.forget(),
                    false => {
                        let line = self.image_anchor();
                        self.cold
                            .placement
                            .hidden(self.arena, at, line, self.cx.col(), self.cols)
                    }
                }
            }
            PrivateMode::ReverseVideo => self.set_reverse_video(change),
            // **Mouse tracking is a level, not five flags.** `[[source:
            // ctlseqs]]` has xterm treat each DECSET as *selecting* one, and
            // `?1003` is a superset of `?1002` which is a superset of `?1000`
            // -- so resetting any of them turns tracking off, which is what an
            // application expects from `?1000 l`. `?9` and `?1001` are levels and
            // follow the same rule: xterm's `really_set_mousemode` stores
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
            // **`?1042` and `?1043` are the bell's**, and the bell is the
            // embedder's: exposing them is acting on them
            // (`Term::bell_is_urgent`, `Term::pop_on_bell`), as `?1007` is
            // the wheel's. They were counted and dropped while `BEL` itself
            // rang nothing.
            PrivateMode::BellIsUrgent => self.cold.modes.apply(ModeBit::BellIsUrgent, on),
            // **`?30` is the embedder's scrollbar** (`Term::scrollbar_shown`),
            // as `?1007` is its wheel's.
            PrivateMode::Scrollbar => {
                self.cold.scrollbar = match change {
                    ModeChange::Set => baud_domain::ScrollbarShown::Shown,
                    ModeChange::Reset => baud_domain::ScrollbarShown::Hidden,
                }
            }
            // **`?41` is read by `HT` at a pending wrap** (`Paint::tab_wraps_first`).
            PrivateMode::MoreFix => {
                self.cold.more_fix = match change {
                    ModeChange::Set => baud_domain::MoreFix::On,
                    ModeChange::Reset => baud_domain::MoreFix::Off,
                }
            }
            // **`?44` is the margin bell's switch** (`margin.rs`); reset, it
            // disarms the bell, as xterm's `bellArmed = -1`.
            PrivateMode::MarginBell => {
                self.cold.bell.set_switch(match change {
                    ModeChange::Set => baud_domain::MarginBell::On,
                    ModeChange::Reset => baud_domain::MarginBell::Off,
                });
                self.rehook();
            }
            PrivateMode::PopOnBell => self.cold.modes.apply(ModeBit::PopOnBell, on),
            // **`?46` starts and stops the embedder's log** (`logging.rs`).
            PrivateMode::Logging => self.set_logging_mode(change, node),
            // DECPFF and DECPEX (`print.rs`): set and reset whether or not
            // there is a printer, as a mode needs no printer port.
            PrivateMode::PrintFormFeed => {
                self.strs.print.form_feed = match change {
                    ModeChange::Set => baud_domain::PrintFormFeed::On,
                    ModeChange::Reset => baud_domain::PrintFormFeed::Off,
                }
            }
            PrivateMode::PrintExtent => {
                self.strs.print.extent = match change {
                    ModeChange::Set => baud_domain::PrintExtent::Page,
                    ModeChange::Reset => baud_domain::PrintExtent::ScrollingRegion,
                }
            }
            // DECGEPM (`hardcopy.rs`): a hard copy's grid, set and reset
            // with or without a printer, as DECPEX is.
            PrivateMode::GraphicsExpandedPrint => {
                self.strs.print.size = match change {
                    ModeChange::Set => baud_domain::PrintSize::Expanded,
                    ModeChange::Reset => baud_domain::PrintSize::Compressed,
                }
            }
            // `?1001` is the fifth level: "The remaining five modes of the
            // mouse protocols are each enabled (or disabled) by a different
            // parameter", exclusive with the other four (`[[source:
            // ctlseqs]]`). It was a flag beside the level until 2026-10-11.
            PrivateMode::HighlightTracking => self.set_tracking(MouseTracking::Highlight, change),
            // `[[source: ctlseqs]]` for `?1049 h`: save the cursor as DECSC,
            // switch to the alternate buffer, clear it. For `?1049 l`: use the
            // normal buffer and restore as DECRC.
            //
            // **A second `?1049 h` does not re-save over the cursor it saved
            // on the way in**: it saves into the alternate screen's slot.
            // **DECANM: `CSI ? 2 l` leaves ANSI, and leaving is a parser
            // transition.** The mode is recorded *and* the node moves, because
            // the very next byte has to be read by the VT52 graph -- `ESC [`
            // there is a command VT52 does not define rather than an
            // introducer. Recording the mode without moving the node is the
            // "a bit that is set and never read" failure; moving the node
            // without recording it would leave `terminal_mode()` lying.
            //
            // **Beside a held request the switch is the rest's**: in
            // `CSI ? 3 ; 2 l` the parser is already `Held` when DECANM comes,
            // and in `CSI ? 2 ; 3 l` it parks after it, so the protocol goes
            // to the node the held rest resumes from rather than over the
            // stop (`WidthAsk::resume`).
            // "DECANM - ANSI/VT52 Mode: This sequence is ignored if
            // received in the Status Display" (070-14 p. 14-8).
            PrivateMode::AnsiMode if self.on_status() => {}
            PrivateMode::AnsiMode => {
                self.cold.terminal = match change {
                    ModeChange::Set => TerminalMode::Ansi,
                    ModeChange::Reset => TerminalMode::Vt52,
                };
                let to = match self.cold.terminal {
                    TerminalMode::Ansi => crate::graph::Node::AfterControl,
                    TerminalMode::Vt52 => crate::graph::Node::Vt52AfterControl,
                };
                match *node {
                    crate::graph::Node::Held => self.cold.width.resume = to,
                    _ => *node = to,
                }
            }
            // **`?1046` reset holds the screen where it is**: "Enable
            // switching to/from Alternate Screen Buffer" (`[[source:
            // ctlseqs]]`), and xterm's `?47`, `?1047` and `?1049` do nothing
            // while its titeInhibit is set (`[[source: xterm-reset]]`'s
            // file). Its `tiXtraScroll`, a resource no mode sets and off by
            // default, has no counterpart here.
            PrivateMode::AltScreen
            | PrivateMode::AltScreenLegacy
            | PrivateMode::AltScreenClearing
            | PrivateMode::SaveCursor
                if !self.cold.resources.is_set(Resource::AltScreenAllowed) => {}
            PrivateMode::AltScreen => self.set_alt_screen(change, AltSwitch::SaveAndClear),
            PrivateMode::AltScreenClearing => self.set_alt_screen(change, AltSwitch::ClearOnExit),
            // **`?1048` is DECSC and DECRC**, "Save cursor as in DECSC" and
            // "Restore cursor as in DECRC" (`[[source: ctlseqs]]`), the shown
            // screen's slot `ESC 7` and `ESC 8` use, as xterm's `CursorSave`
            // and `CursorRestore` are (`[[source: xterm-reset]]`'s file).
            PrivateMode::SaveCursor => self.save_or_restore_cursor(change),
            // **`?47` switches buffers and does nothing else.** xterm's
            // `srm_ALTBUF` is `ToAlternate(xw, False)` in and
            // `FromAlternate(xw, False)` out (`xterm-charproc.c`, the
            // `srm_ALTBUF` case): no cursor save or restore, and no clear, so
            // the alternate screen shows what it held when it was last left.
            // It was routed nowhere while `docs/vt-coverage.md` called it
            // done, and its one test passed on a no-op.
            PrivateMode::AltScreenLegacy => self.set_alt_screen(change, AltSwitch::Bare),
            // The Sixel modes, recorded only while DA lists Sixel
            // ([`Paint::sixel_claim`]): `sixel_end` reads `?80` and `?8452`,
            // and `Act::SixelStart` `?80` and `?1070`.
            PrivateMode::SixelDisplay => self.set_sixel_mode(ModeBit::SixelDisplay, change),
            PrivateMode::SixelPrivateRegisters => {
                self.set_sixel_mode(ModeBit::SixelPrivateRegisters, change)
            }
            PrivateMode::SixelScrollsRight => {
                self.set_sixel_mode(ModeBit::SixelScrollsRight, change)
            }
            // **Resetting `?1046` on the alternate screen leaves it**, as
            // xterm's `srm_ALLOW_ALTBUF` calls `FromAlternate` when titeInhibit
            // turns on: the bare switch back, no cursor restored, the
            // primary as it was.
            PrivateMode::Resource(Resource::AltScreenAllowed) => {
                if !on && self.cold.screen == crate::term::Screen::Alternate {
                    self.set_alt_screen(ModeChange::Reset, AltSwitch::Bare);
                }
                self.cold
                    .resources
                    .apply(Resource::AltScreenAllowed, change);
            }
            // The rest are read where they act: `?1010` by `Term::feed`,
            // `?1045` by `BS` at the left margin and by the `CUB` arm,
            // `?1040` by the watch on a selection's lines and by the
            // embedder, and `?1011`, `?1041`, `?2001`, `?2005` and `?2006`
            // by the embedder through `Term::resource`.
            PrivateMode::Resource(r) => self.cold.resources.apply(r, change),
            // **terminal-wg's paragraph properties** (`crate::path`): the
            // value in force, and the paragraph the cursor stands at the
            // start of.
            PrivateMode::BoxMirroring => {
                let b = match on {
                    true => BoxMirror::On,
                    false => BoxMirror::Off,
                };
                self.cold.line = self.cold.line.with_box_mirror(b);
                self.select_line_property(LineProperty::BoxMirror);
            }
            PrivateMode::DirectionAutodetect => {
                let a = match on {
                    true => Autodetect::On,
                    false => Autodetect::Off,
                };
                self.cold.line = self.cold.line.with_autodetect(a);
                self.select_line_property(LineProperty::Autodetect);
            }
            // Global, read by `Term::key`: "Unlike the output modes, the
            // keyboard mode is global".
            PrivateMode::ArrowKeySwap => {
                self.cold.arrows = match on {
                    true => ArrowSwap::On,
                    false => ArrowSwap::Off,
                };
            }
            // **`?42` DECNRCM is recognised and ignored**, not counted:
            // "This sequence is ignored when the North American or Dutch
            // Keyboard Dialect is selected" (`[[source: dec-std-070]]`
            // DECNRCM note 3, p. 5-126), and baud has no dialect setting,
            // its keyboard a North American one, for which 7-bit NRCS "is
            // not available" (070-6 p. 6-58). The national sets act
            // whenever designated, as SCS's algorithm (p. 5-121) and its
            // note 5's workstation emulator have them (note AF's
            // Q-DECNRCM).
            PrivateMode::NationalReplacement => {}
            // **`?34` DECRLM** (`crate::leftward`): the one-by-one graph
            // while set, and a wrap left pending at the left margin taken
            // back to a column when it is reset.
            PrivateMode::CursorRightToLeft => self.cursor_direction(change),
            // **`?35` DECHEBM** (`crate::hebrew_keys`): the keyboard's map,
            // and GL's set again, which it may draw as DEC 7-bit Hebrew.
            PrivateMode::HebrewKeyboard => {
                self.cold.hebrew_keyboard = match on {
                    true => HebrewKeyboard::Hebrew,
                    false => HebrewKeyboard::NorthAmerican,
                };
                self.invoke();
            }
            // Global, read by the embedder's copy (`Term::copy_direction`).
            PrivateMode::RightToLeftCopy => {
                self.cold.copy = match on {
                    true => CopyDirection::RightToLeft,
                    false => CopyDirection::LeftToRight,
                };
            }
            // **`?36` DECHEM**, "opposite in action to DECNRCM": set is
            // STD 070's `reset_character_set_mode()`, the sets back to their
            // power-up designations and invocations, and reset its
            // `set_character_set_mode()`, the keyboard's 7-bit NRC set in G0
            // to G3, a Hebrew VT510's DEC 7-bit Hebrew; "Changing this mode
            // has the side effect of re-initializing the designated and
            // invoked character sets to their default state" (`[[source:
            // dec-std-070]]` DECNRCM, pp. 5-126 to 5-128, note 1).
            PrivateMode::HebrewEncoding if on => {
                self.reset_sets();
                self.invoke();
            }
            PrivateMode::HebrewEncoding => {
                let set = Charset::National(Nrcs::Hebrew);
                if !self.designable(GSlot::G0, set) {
                    return;
                }
                self.cold.hebrew = HebrewEncoding::National;
                self.cold.g = [set; 4];
                self.cold.desig = [Designation::HEBREW_NRCS; 4];
                self.cold.gl = GSlot::G0;
                self.cold.gr = GSlot::G2;
                self.invoke();
            }
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
    pub(crate) fn set_tracking(&mut self, level: MouseTracking, change: ModeChange) {
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
        // And `?1001`'s handshake ends: a press owed an answer, or a region
        // tracking the mouse, belongs to the level that took the press, and
        // its release is reported by whatever is in force now.
        self.cold.handshake = Handshake::Idle;
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
        // A switch takes the active position to another page buffer, and
        // an aligned string ends.
        self.effector_ends_string();
        // **The alternate screen is the main display's**: from the status
        // line it switches the main display's screen, and the line stays
        // selected.
        if self.on_status() {
            self.on_main(|p| p.set_alt_screen(change, how));
            return;
        }
        // A placement names a position on the screen it was made on, and a
        // switch leaves that screen, clears it or restores a saved cursor
        // (`placed.rs`). The calls that do none of these -- `?47 l` on the
        // primary, `?47 h` while it is up -- forget it too, at no cost that
        // matters: a hide after any of them is not right after a move.
        self.cold.placement.forget();
        // **An open SPA's start is kept as its screen position** across the
        // switch: its row on the screen left, anchored again on the screen
        // shown below.
        let open = self.cold.guard_open.take().map(|g| {
            let (_, top) = self.image_top();
            (g.line.since(top), g.col)
        });
        self.switch_screen(change, how);
        if let Some((row, col)) = open {
            let (screen, top) = self.image_top();
            let line = match row {
                Some(r) => top.plus(r),
                None => top,
            };
            self.cold.guard_open = Some(crate::term::GuardStart { screen, line, col });
        }
    }

    /// [`Paint::set_alt_screen`]'s switch itself.
    fn switch_screen(&mut self, change: ModeChange, how: AltSwitch) {
        use crate::term::Screen;
        match (change == ModeChange::Set, self.cold.screen) {
            (true, Screen::Normal) => {
                // **Into the primary's `DECSC` slot**, "Save cursor as in
                // DECSC" (`[[source: ctlseqs]]`), xterm's `CursorSave` before
                // `ToAlternate`; the style and `?12` ride beside it.
                if how == AltSwitch::SaveAndClear {
                    self.cold.saved = Some(self.save());
                    self.cold.alt_style = Some(AltStyle {
                        style: self.cold.cursor_style,
                        blink: self.cold.cursor_blink,
                    });
                }
                // **Each screen has its own slot**, xterm's
                // `sc[screen->whichBuf]` (`[[source: xterm-cursor-c]]`): the
                // alternate screen's comes up as it was last left.
                core::mem::swap(&mut self.cold.saved, &mut self.cold.saved_other);
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
                core::mem::swap(&mut self.strs.shell, &mut self.strs.shell_other);
                self.strs.shell = crate::prompt::Shell::FRESH;
                // **Two surfaces, swapped, nothing copied.** The alternate
                // screen is its own buffer at the pitch; the view takes it
                // and the primary rides as `other` until the exit swaps them
                // back, its cells, marks, ring and rotation all where they
                // were. This replaced a park that copied the primary out and
                // back, which was the one thing standing between the grid
                // and being the GPU's in place on both screens.
                self.view.swap_surface(&mut self.other);
                // The cursor's row is the other screen's now, and under `?47`
                // it keeps whatever that screen held.
                self.row_changed_to(self.cy);
                // `?1049` clears the alternate screen on entry, so a rotation
                // left from the last time an application was in it describes
                // nothing. Blanks rotate to blanks, so this is tidiness rather
                // than a fix -- and an invariant that holds only by accident is
                // one the next change breaks. **`?47` does neither**: it
                // shows the rows as they were left, so resetting the rotation
                // would put them out of order.
                //
                // **Neither moves the cursor.** ctlseqs' `1049` saves it "as
                // in DECSC", switches and clears, and names no move; xterm's
                // set arm is `CursorSave; ToAlternate; ClearScreen`, and
                // `ClearScreen` leaves the cursor (`xterm-util.c`), as
                // `SwitchBufs` does for `?47`. This homed until 2026-10-13.
                // `ClearScreen` does reset the wrap (`ResetWrap`), so a
                // cursor in pending wrap at the screen's edge stays in the
                // last column with nothing pending: the `goto` to where it
                // already is, whose clamp to the last column drops the
                // sentinel and which re-reads the cleared row's extent.
                // Under side margins narrower than the screen the sentinel
                // one past the right margin is a column the clamp keeps, and
                // stays the sentinel, as `goto` places nothing; a placed
                // cursor keeps its tag.
                if how == AltSwitch::SaveAndClear {
                    self.view.reset_rot();
                    self.clear_screen();
                    // The same cursor, placed or not.
                    let c = self.cx;
                    self.goto(self.cy, c.col());
                    if c.is_placed() {
                        self.cx = c;
                    }
                }
                // Its image numbering starts again with the screen. No image
                // can be on it: leaving is the only way off it, and leaving
                // deletes them.
                self.book.alt_top = LineIdx::FIRST;
            }
            (false, Screen::Alternate) => {
                // **`?1047 l` clears the alternate screen before it leaves**,
                // "Clear the screen first if in the Alternate Screen Buffer"
                // (`[[source: ctlseqs]]`), xterm's `ClearScreen` before
                // `FromAlternate` (`[[source: xterm-reset]]`'s file); the
                // cursor stays, as it does under `?47`.
                if how == AltSwitch::ClearOnExit {
                    self.clear_screen();
                }
                // **The editor's images go with the editor's screen.** foot
                // and ghostty both delete them; the alternate screen keeps no
                // history, so nothing could scroll back to them, and a shell
                // that got them would show a picture over its own text.
                if self.images.any_placed() {
                    self.images.expire_screen(ImageScreen::Alternate);
                }
                self.cold.screen = Screen::Normal;
                self.cold.epoch = self.cold.epoch.next();
                core::mem::swap(&mut self.cold.saved, &mut self.cold.saved_other);
                // Restore the primary screen's kitty stack and prompt state;
                // see the enter arm.
                core::mem::swap(&mut self.cold.kitty, &mut self.cold.kitty_alt);
                core::mem::swap(&mut self.strs.shell, &mut self.strs.shell_other);
                // Surface first, then the cursor: `restore` clamps against the
                // geometry and moves the cursor, and it should be moving over
                // the screen the application is about to see. A resize taken
                // while the alternate screen was up was applied to the
                // primary as it happened, around the cursor it will have --
                // the one `?1049` saved, or under `?47` the one both screens
                // share (`Term::primary_cursor`) -- so there is nothing to
                // catch up on here.
                self.view.swap_surface(&mut self.other);
                self.row_changed_to(self.cy);
                // **`?47` restores nothing, and leaves the save alone.** A
                // cursor `?1049 h` saved outlives a `?47 l` exit, as the
                // cursor xterm saves outlives `FromAlternate`, and `ESC 8` on
                // the primary finds it.
                if how == AltSwitch::SaveAndClear {
                    self.restore_alt();
                }
            }
            // **`?1049 h` while the alternate screen is up saves and clears
            // it**, as xterm's does: its set arm is `CursorSave; ToAlternate;
            // ClearScreen`, and `ToAlternate` is a no-op on the alternate
            // buffer while the save and the clear are not (`xterm-charproc.c`,
            // the `srm_OPT_ALTBUF_CURSOR` case). The save is the alternate
            // screen's cursor into the alternate screen's slot, and the
            // primary's save and style stay; it does not home, because
            // xterm's `ClearScreen` does not (`xterm-util.c`). `?47 h` then
            // `?1049 h` used to leave the `?47` screen's text under the
            // program that asked for a cleared one.
            (true, Screen::Alternate) => {
                if how == AltSwitch::SaveAndClear {
                    self.cold.saved = Some(self.save());
                    self.clear_screen();
                }
            }
            // **`?1049 l` on the primary restores as `DECRC`**, as xterm's
            // `CursorRestore` runs whatever `FromAlternate` did: what
            // `?1049 h` saved, if a `?47 l` left it up with a save, or an
            // `ESC 7`, or with nothing saved home. `RIS`, which leaves the
            // alternate screen first, comes here too.
            (false, Screen::Normal) => {
                if how == AltSwitch::SaveAndClear {
                    self.restore_alt();
                }
            }
        }
    }

    /// SM and RM, `CSI Pm h` and `CSI Pm l`: **every parameter, in
    /// order**, as DECSET's are. `[[source: ecma48]]` § 8.3.125 and
    /// § 8.3.106 give both the notation `(Ps...)`, the modes "set as
    /// specified by the parameter values"; xterm's `ansi_modes` walks them
    /// all. Reading the first set LNM alone from `CSI 20 ; 4 h`. A mode
    /// baud does not act on, a hole and a bare `CSI h` included, is
    /// counted a parameter at a time.
    #[inline(never)]
    pub(crate) fn apply_ansi_modes(&mut self, csi: &crate::write::Csi, change: ModeChange) {
        csi.for_each_param(|p| self.apply_ansi_mode(change, AnsiMode::from_wire(p)));
    }

    fn apply_ansi_mode(&mut self, change: ModeChange, mode: AnsiMode) {
        if !introduced_by(mode.function()).admits(self.cold.level) {
            self.unsupported_param += 1;
            return;
        }
        let on = change == ModeChange::Set;
        match mode {
            AnsiMode::InsertReplace => {
                self.cold.modes.apply(ModeBit::InsertReplace, on);
                self.refresh_insert();
            }
            AnsiMode::LineFeedNewLine => self.cold.modes.apply(ModeBit::LineFeedNewLine, on),
            AnsiMode::Erasure => self.cold.modes.apply(ModeBit::EraseGuarded, on),
            // GATM: set ALL, reset GUARD (`crate::print`).
            AnsiMode::GuardedAreaTransfer => self.cold.modes.apply(ModeBit::TransferGuarded, on),
            AnsiMode::KeyboardAction => self.cold.modes.apply(ModeBit::KeyboardAction, on),
            AnsiMode::SendReceive => self.cold.modes.apply(ModeBit::SendReceive, on),
            AnsiMode::BiDirectionalSupport => {
                self.cold.bidi = match on {
                    true => BidiSupport::Implicit,
                    false => BidiSupport::Explicit,
                };
                self.refresh_presentation_edits();
            }
            AnsiMode::DeviceComponentSelect => {
                self.cold.dcsm = match on {
                    true => DeviceComponent::Data,
                    false => DeviceComponent::Presentation,
                };
                self.refresh_presentation_edits();
            }
            // HEM: set PRECEDING, reset FOLLOWING (`crate::editing`).
            AnsiMode::CharacterEditing => {
                self.cold.modes.apply(ModeBit::EditPreceding, on);
                self.refresh_insert();
            }
            _ => self.unsupported_param += 1,
        }
    }

    /// **The whole screen cleared, pictures with it**: DECCOLM's "display
    /// data to be cleared" (`[[source: dec-std-070]]` p. 5-71), `?1049`'s
    /// and `?1047`'s clear. The VT330 and VT340 keep text and graphics in
    /// one bitmap, and xterm's `ClearScreen` clears the graphics over every
    /// row (`[[source: xterm-scroll]]`'s file, lines 2370-2374), so a Sixel
    /// and the ReGIS page lose their pixels as ED 2's rows do
    /// (`Paint::erase_display`), each row read before its rendition is
    /// reset, and the kitty placements on the screen go as ED 2 takes them.
    pub(crate) fn clear_screen(&mut self) {
        if self.past_edge() {
            self.clear_erased_rows_past_edge(Row::ZERO, Row::from_index(self.rows.index()));
        }
        if self.images.any_placed() {
            self.clear_screen_images();
        }
        for y in 0..self.rows.index() {
            let b = self.blank();
            let n = self.view.fill_row(Row::from_index(y), b);
            self.cells = self.cells.plus(n.index());
        }
        self.path_rows(Row::ZERO, Row::from_index(self.rows.index() - 1));
        self.wrote_rows(Row::ZERO, Row::from_index(self.rows.index() - 1));
    }

    #[cold]
    #[inline(never)]
    fn clear_screen_images(&mut self) {
        let cols = Col::at_index(self.cols.index());
        for y in 0..self.rows.index() {
            self.erase_images(Row::from_index(y), Col::ZERO, cols);
        }
        if self.images.any_placed() && !self.on_status() {
            self.kitty_clear();
        }
    }

    /// DECALN: origin mode absolute and the rendition normal, then the
    /// screen filled with `E`, every margin reset and the cursor home.
    ///
    /// `[[source: dec-std-070]]`'s DECALN (pp. D-19 and D-20) sets
    /// `ORIGIN_MODE := ABSOLUTE` and the four renditions it knows to `FALSE`
    /// before it fills, then the Active Position to 1;1 and the top and
    /// bottom margins to the page's. The colours belong to the same Current
    /// Rendition (§ 5.13.1, p. 5-186), so the rendition goes as `SGR 0`
    /// takes it, SAPV's variants kept: they are no rendition of DEC's, and
    /// a form waiting for the following character still waits. The pattern
    /// is in the default rendition. The 1985 algorithm predates side
    /// margins; "DECALN sets the margins to the extremes of the page"
    /// (`[[source: vt510-decaln]]`) takes all four, and no text resets
    /// `DECLRMM`. It kept DECOM, the rendition and the side margins until
    /// 2026-10-14.
    pub(crate) fn screen_alignment(&mut self) {
        // It homes the cursor: an aligned string ends (`crate::aligned`).
        self.effector_ends_string();
        self.cold.modes.apply(ModeBit::Origin, false);
        self.form_ends();
        self.style_raw = self.style_raw.sgr_reset();
        self.style = self.intern_style(self.style_raw);
        self.rendition_changed();
        let cell = PackedCell::glyph(
            Codepoint::from_ascii(StreamByte::from_stream(b'E')),
            StyleId::DEFAULT,
            self.flags,
        );
        for y in 0..self.rows.index() {
            let n = self.view.fill_row(Row::from_index(y), cell);
            self.cells = self.cells.plus(n.index());
        }
        self.wrote_rows(Row::ZERO, Row::from_index(self.rows.index() - 1));
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        self.goto(Row::ZERO, Col::ZERO);
    }

    /// RIS. **Everything a soft reset does, and the screen as well.**
    pub(crate) fn hard_reset(&mut self) {
        // **RIS erases and leaves the status line**, "The Status Display will
        // be erased and exited" (070-14 p. 14-10), to the power-up type,
        // none: first, so the rest resets the main display.
        self.reset_status();
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
        // The screen the reset was made from, for the erasure it counts
        // below: leaving makes it the primary's whichever it was. And where
        // the primary's window over the history stood at this byte, which
        // the reset takes to the live screen.
        let from = self.cold.screen;
        let looking = match self.book.window_back().index() {
            0 => crate::term::Looking::Live,
            _ => crate::term::Looking::Back,
        };
        self.set_alt_screen(ModeChange::Reset, AltSwitch::SaveAndClear);
        // DECHEBM's power-on mapping, North American, before `soft_reset`
        // invokes GL's set (`crate::hebrew_keys`). **And Level 4 before it
        // too**, which `soft_reset` loads the sets for: a reset from Level 1
        // otherwise loaded Level 1's and then left Level 1.
        self.cold.hebrew_keyboard = HebrewKeyboard::NorthAmerican;
        self.cold.level = ConformanceLevel::Four;
        self.soft_reset();
        // SDS's and SRS's table empties, the screens it named erased.
        self.reset_strings();
        // **The progress a program set goes with it**: `RIS` returns to the
        // initial state (`[[source: ecma48]]` § 8.3.105), which shows no
        // bar, and a kitty notification half-assembled is dropped. The ones
        // already shown stay shown: they are the desktop's now.
        if self.strs.progress != Progress::HIDDEN {
            self.strs.progress = Progress::HIDDEN;
            self.strs.progress_reports = self.strs.progress_reports.next();
        }
        self.strs.notices.reset();
        // **Both screens' saved cursors go**: `soft_reset` empties the shown
        // screen's slot, as `DECSTR` does, and `RIS` returns to the initial
        // state (`[[source: ecma48]]` § 8.3.105), in which nothing is saved
        // on either. xterm's full reset saves the primary's cursor at home
        // and leaves the alternate screen's slot (`[[source: xterm-reset]]`'s
        // file): that departs from the initial state, and baud does not
        // follow it.
        self.cold.saved_other = None;
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
        // **And the colours a program set with `OSC 10`, `11`, `12`, `17`
        // and `19`**, which `OSC 21` and iTerm2's `SetColors` write too,
        // the text under the cursor and the visual bell's colour: none is
        // set in "the state it has after it is made operational"
        // (`[[source: ecma48]]` 8.3.105), so the embedder's colours stand
        // again. After the flip, which can
        // move the cursor's colour into the program's slot. xterm's
        // `ReallyReset` keeps every dynamic colour (`[[source:
        // xterm-reset]]`'s file), a practice not followed. `DECSTR` keeps
        // them, as its list names none.
        crate::osc::clear_dynamic(self.cold, self.style_log);
        self.strs.palette[crate::storage::CURSOR_TEXT] = ColorWord::DEFAULT;
        self.strs.palette[crate::storage::VISUAL_BELL] = ColorWord::DEFAULT;
        // `?1042` and `?1043` go back to reset with the rest: ECMA-48
        // § 8.3.105 has `RIS` "set the modes into the reset state"
        // (`[[source: ecma48]]`). xterm keeps both -- no `ReallyReset` path
        // names `bellIsUrgent` or `popOnBell` (`[[source: xterm-reset]]`'s
        // file) -- a departure from the text not followed. `DECSTR` above
        // keeps them, as its list does not name them.
        // **And the width the window had before a program asked**:
        // `[[source: dec-std-070]]`'s `RIS` is its Device Initialization
        // (PDF 243), whose `host_state_init` reads `column mode = EIGHTY;
        // /* NVM */` and `if (column_mode == EIGHTY) columns-per-page = 80;
        // /* NVM */` (PDF 219) -- the width the user stored in Set-Up -- and
        // whose values "apply only to those implementations in which the
        // state cannot be set and stored locally by the terminal user" (PDF
        // 216). Here the user stores a width by giving it to the window, so
        // `RIS` asks back for the grid's width at the first request since
        // power-on or the last `RIS`, unless that is the width last asked;
        // with no request made it asks nothing. xterm's `ReallyReset` asks
        // for 80 from 132 (`[[source: xterm-reset]]`'s file), a practice
        // departing from the text and not followed. `?40` and `?95` go back
        // to reset with the rest; `DECSTR` keeps them, as `[[source:
        // vt510-decstr]]`'s table names neither. **Not held**: `RIS` is an
        // `ESC` final, and the read goes on at the old width -- a gap in the
        // record.
        let a = &mut self.cold.width;
        if let Some(home) = a.home.take() {
            let back = WidthRequest::of_window(home);
            if a.last != Some(back) {
                a.requests = a.requests.next();
                a.last = Some(back);
            }
        }
        self.cold.modes = Modes::default();
        // **DECARM, DECCKM and LNM to their saved Set-up values**: "RIS
        // replaces all set-up features with their saved settings"
        // (`[[source: vt510-ris]]`), the embedder's `[keyboard]`
        // (`Term::set_up_at_launch`); the keypad stays as `soft_reset` left
        // it, Numeric, which is not a saved value (`[[source: vt510-deckpam]]`).
        self.cold.restore_set_up();
        // **The input encoding back to UTF-8, and `?1007` back on**, the
        // state baud is made operational in (`Term::new`): `RIS` returns
        // the terminal "to its initial state, i.e. the state it has after
        // it is made operational" (`[[source: ecma48]]` 8.3.105). After
        // `ESC % @` the encoding stayed ISO 8859-1, and after `?1007 l` the
        // wheel stayed off. The graph follows the encoding here, and it
        // decodes the byte after the reset in the same read; `esc_final`'s
        // `RIS` arm sets the write loop's live copy, the encoding a
        // control string begins under. `DECSTR` keeps both, as its list
        // names neither. xterm's `ReallyReset` touches neither its UTF-8
        // mode nor `alternateScroll` (`[[source: xterm-reset]]`'s file), a
        // practice not followed.
        self.cold.encoding = InputEncoding::Utf8;
        self.reselect();
        self.cold.alternate_scroll = AlternateScroll::On;
        // BDSM to its initial state, set: ECMA-48 8.3.105 resets a device
        // "to its initial state, i.e. the state it has after it is made
        // operational", setting the modes into the reset state being what
        // that "may imply, if applicable". `DECSTR` keeps it, as its DEC
        // table names no BDSM.
        self.cold.bidi = BidiSupport::POWER_ON;
        self.cold.dcsm = DeviceComponent::POWER_ON;
        // SLH's and SLL's positions go (`crate::bounds`).
        self.cold.bounds = crate::bounds::LineBounds::NONE;
        self.cold.extent = None;
        self.refresh_presentation_edits();
        // No character path, autodetection on and no box mirroring, in
        // force and on every line: the mode word above took
        // `ModeBit::PathSelected` back, and the rows below are erased with
        // their marks, which carry the lines' properties. Arrow-key
        // swapping on, its power-on state.
        self.cold.line = LineBidi::POWER_ON;
        self.cold.arrows = ArrowSwap::On;
        // `?96`'s default, "Disable right-to-left-copy"
        // (`[[source: vt510-decrlcm]]`).
        self.cold.copy = CopyDirection::LeftToRight;
        // iTerm2's cursor guide goes back to the embedder's setting: its
        // reset sets the profile's again (`screenDidReset`, `[[source:
        // iterm2-964a75d-ptysession]]`), and `DECSTR`'s list does not name
        // it, as iTerm2's `softReset` does not.
        self.cold.cursor_guide = None;
        // **And what else a program set that the initial state has none
        // of** (`[[source: ecma48]]` 8.3.105), the program's colours above
        // among it; `DECSTR`'s table (`[[source: vt510-decstr]]`) names
        // none of it, so it keeps all of it. Both kitty keyboard stacks,
        // the parked one too, so the shell a crashed program leaves in the
        // protocol gets its keys back at `reset`, the case
        // `[[source: kitty-keyboard]]` keeps Enter, Tab and Backspace
        // legacy for. XTSHIFTESCAPE and `?30`, which xterm keeps
        // (`[[source: xterm-reset]]`'s file), a practice not followed,
        // since `[[source: ecma48]]` 8.3.105 resets the device to its
        // initial state. The title and icon name, the badge and its colour,
        // the user variables, and the reported directory with the host
        // `RemoteHost` named: none of it is set at power-on. xterm keeps the
        // title and iTerm2 the rest.
        self.cold.kitty = baud_domain::KittyStack::new();
        self.cold.kitty_alt = baud_domain::KittyStack::new();
        self.cold.shift_escape = ShiftEscape::Override;
        self.cold.scrollbar = baud_domain::ScrollbarShown::Shown;
        self.title_len = ByteLen::ZERO;
        self.icon_len = ByteLen::ZERO;
        self.strs.title_enc = InputEncoding::Utf8;
        self.strs.icon_enc = InputEncoding::Utf8;
        self.strs.iterm.forget_session();
        self.strs.user_vars.fill(baud_domain::UserVar::FREE);
        self.forget_directory();
        // **And the font, when a program changed it**: the configured one,
        // asked of the embedder in place of whatever is still queued, as
        // "RIS replaces all set-up features with their saved settings"
        // (`[[source: vt510-rm-b]]` p. 5-184). xterm keeps a font a program
        // set, a practice not followed.
        self.strs.fonts.reset();
        // The xterm resources a program set go back with the modes, as
        // `?1042` and `?1043` do: ECMA-48's "set the modes into the reset
        // state". `DECSTR` keeps them, as its list names none.
        self.cold.resources = baud_domain::Resources::POWER_ON;
        self.cold.more_fix = baud_domain::MoreFix::Off;
        // XTSMPOINTER back to the embedder's start value: "Reset title and
        // pointer modes per resources titleModes and pointerMode", in the
        // man page's full-reset list (`[[source: xterm-man]]`), and xterm's
        // hard branch `pointer_mode = pointer_mode0` (`[[source:
        // xterm-reset]]`'s file). `DECSTR` keeps it.
        self.cold.pointer_mode = self.cold.initial_pointer_mode;
        // **And the bells' volumes and `?44`**: the volumes are set-up
        // features, and "RIS replaces all set-up features with their saved
        // settings" (`[[source: vt510-rm-b]]` p. 5-184), the power-on ones
        // here; `?44` is a mode, and RIS sets "the modes into the reset
        // state" (ECMA-48 8.3.105). xterm's reset keeps both volumes, a
        // departure not followed. The `BEL`s before it are credited to the
        // volume they rang at first.
        self.fold_bells();
        self.cold.bell.reset();
        self.rehook();
        // **And the conformance level and the C1 transmission**: DEC STD
        // 070's Device Initialization takes both from NVM (`host_state_init`,
        // "conformance level = LEVEL 4"), which here is the power-up state,
        // Level 4 and seven-bit, the VT510's factory default. `RIS` kept an
        // `S8C1T` before. The level is set above, before the soft reset.
        self.cold.replies = self.cold.replies.with_c1(C1Transmit::SevenBit);
        // `RIS` "remove[s] qualified areas" (`[[source: ecma48]]` 8.3.105):
        // the screens were cleared above, and ED, EL and ECH take their
        // fill again.
        self.cold.guards = crate::term::Guards::None;
        // The shared Sixel registers go with `?1070`, which `Modes::default`
        // has just set: xterm's full reset restores the mode
        // (`[[source: xterm-reset]]`), and a terminal made operational has
        // defined none.
        self.strs.sixel.forget_shared();
        // **ReGIS to its power-up state**: the device state, the
        // macrographs and the loaded alphabets, as the VT300 keeps its
        // defaults "after you power up or reset the terminal"
        // (`[[source: vt3xx-regis-intro]]`).
        self.strs.regis.reset();
        // The second drawing surface goes with it, its memory with the
        // images below; a plane made after the reset is its own.
        self.strs.regis_second.free(self.images);
        // **The printer to its power-up state** (`print.rs`): an autoprint
        // run's job ends, kept for the embedder, and DECPFF and DECPEX go
        // back to their defaults; the port is the embedder's, and a printer
        // that is gone is forgotten, "not available" again.
        self.print_reset();
        // **XTSAVE's cache goes back to empty**: ECMA-48 § 8.3.105 has `RIS`
        // reset "to its initial state, i.e. the state it has after it is made
        // operational" (`[[source: ecma48]]`), and nothing is saved then.
        // `DECSTR` keeps it, as its list does not name it. xterm keeps it
        // across both -- `ReallyReset` never touches `save_modes`
        // (`[[source: xterm-reset]]`) -- which departs from the text, and the
        // text is followed.
        self.cold.xtsave = SavedModes::NONE;
        // **And XTPUSHSGR's stack**, for the same text: nothing is saved in
        // the initial state. xterm keeps it across both resets, a departure
        // from the text not followed; `DECSTR` keeps it.
        self.strs.renditions = crate::stack::RenditionStack::EMPTY;
        // XTPUSHCOLORS's stack too, for the same reason.
        self.strs.colors = crate::stack::ColorStack::EMPTY;
        // **The title modes go back to the default**, xterm's full reset
        // `title_modes = title_modes0` (`[[source: xterm-reset]]`'s file);
        // `DECSTR` keeps them, as xterm's soft reset does.
        self.strs.title_modes = TitleModes::DEFAULT;
        // **And the embedder's own pointer**, the state after power-up, for
        // the same text; xterm's `ReallyReset` keeps `OSC 22`'s shape, a
        // departure not followed. `DECSTR` keeps it.
        self.cold.pointer = None;
        // The special-colour modes go back to off, "set the modes into the
        // reset state" (`[[source: ecma48]]` 8.3.105); xterm keeps them as
        // resources, not followed. The colours go with the palette, in
        // `soft_reset`, as xterm's reset of all its `MAXCOLORS` does.
        if self.strs.color_modes != ColorModes::NONE {
            self.strs.color_modes = ColorModes::NONE;
            self.style_log.colors_moved();
        }
        self.cold.mouse = MouseTracking::Off;
        self.cold.mouse_encoding = MouseEncoding::X10;
        // The locator's units, events and rectangle to power-up, the
        // pointer kept (`Locator::reset`); `DECSTR` keeps them all.
        self.cold.locator.reset();
        self.cold.terminal = TerminalMode::Ansi;
        self.cold.vt52_row = Vt52Coord::ZERO;
        // DECSACE: STD 070's host_state_init sets "attribute change extent =
        // STREAM" (`[[source: dec-std-070]]` 070-4 p. 15, PDF 219) and
        // DECSTR's table (p. 31, PDF 235) does not name it, so only `RIS` puts it back, as xterm's
        // `if (full)` branch does (`[[source: xterm-reset]]`).
        self.cold.attr_extent = AttrExtent::Stream;
        // "power-up. data integrity flag has not been reported since last
        // power-up or RIS" (`[[source: dec-std-070]]` 070-4, DSR); `DECSTR`
        // is neither.
        self.cold.replies = self.cold.replies.with_integrity(Integrity::PowerUp);

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
        // baud-app dropped the selection only by that accident, reading a
        // rotation under half the ring as rows restored, and lost it when
        // the rotation stayed; it reads the reset itself now, from the count
        // below (note V, V1).
        self.book.hist = Rows::ZERO;
        self.book.hidden = Rows::ZERO;
        // **A window over the history goes to the live screen**, set here
        // rather than left to the empty history to imply (note V, V2): the
        // history refills from the next line, and a window that names a line
        // the reset erased would read as one over the refill.
        self.book.window = crate::storage::HistoryWindow::Live;
        // **The reset is counted, for an embedder's state over the screen**
        // (`Term::resets`, note V, V1): its selection and the echoes it drew
        // ahead of the shell name cells the power-on state does not have,
        // and nothing else across a read says a reset came -- the rotation
        // stayed, a terminal with no history reads the same at both ends,
        // and the erasure count below moves on `CSI 3 J` too, which keeps
        // the screen. foot's hard reset cancels the selection
        // (`selection_cancel` in `term_reset`'s hard path,
        // `[[source: foot-term-reset]]`), and xterm's `RIS` reaches
        // `ClearScreen`, which disowns it (`[[source: xterm-reset]]`,
        // `[[source: xterm-scroll]]`); neither does so on `DECSTR`, and
        // `soft_reset` does not count.
        self.book.resets = self.book.resets.next();
        // The history was erased, which an embedder's window over it reads
        // here as it reads `CSI 3 J` (`Term::history_erasures`): nothing
        // above moved the rotation or the top line, so no other reading says
        // the lines it showed are gone. Which screen it came from is kept
        // when it is the first since an embedder armed the watch
        // (`Term::first_erasure`): a window over the history is drawn only
        // on the primary, so a reset made from the alternate screen took no
        // window it was looking through to the live screen.
        self.cold.erased(crate::term::Erasure::Reset(from, looking));
        // And what the marks said: the block start names rows the reset
        // erased, and the shell is at no prompt after one. This is the
        // primary's -- `set_alt_screen` above came back to it -- and the
        // alternate screen's starts fresh on its next entry, as its cells do.
        self.strs.shell = crate::prompt::Shell::FRESH;
        self.strs.prompts.clear();
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
        self.strs.link_book.reset(self.strs.link_len);
        // Images go with the clusters and for the same reason: `RIS` is the one
        // point where every cell is gone, so a placement outliving a mere clear
        // is correct -- an image is content a program may redraw over -- and
        // only a reset drops them all. Leaving the alternate screen, above,
        // drops that screen's alone. A Sixel body the reset abandons lets
        // its claim go first, while its rows are still its own.
        self.strs.sixel.abandon(self.images);
        self.images.reset();
        // An iTerm2 file in parts is abandoned: a terminal made operational
        // has none open (`[[source: ecma48]]` 8.3.105), and after it the
        // file's parts and `FileEnd` are counted, none open.
        self.file_reset();
        // The kitty transmission in flight and any decode pending name
        // images the reset just dropped; the scratch a PNG wrote goes back.
        let s = &mut *self.strs;
        s.kitty.reset(self.images, s.kitty_scratch);
        self.reset_tabs();
        self.cold.line_stops.clear_all();
        self.cold.backarrow = BackarrowKey::Delete;
        self.cold.delete = DeleteKey::Unset;
        self.cold.num_lock = NumLockKeypad::Overrides;
        // The default keyboard, the one a terminal made operational has
        // (`[[source: ecma48]]` 8.3.105), as xterm's `RIS` decodes its
        // configured keyboard again (`[[source: xterm-reset]]`, charproc.c
        // 14451-14455).
        self.cold.keyboard = Keyboard::Default;
        self.last_cp = None;
        self.goto(Row::ZERO, Col::ZERO);
    }

    /// DECSCL, `CSI Pl ; Pc " p` (`[[source: dec-std-070]]` 070-4 DECSCL):
    /// `Pl` 61 to 64 selects Levels 1 to 4, any other is ignored and counted
    /// ("Attempts to select conformance levels that are unsupported by the
    /// device will be ignored", note 4). Then a soft reset, note 3's "soft
    /// reset ... without affecting the data contained in the display", which
    /// the VT220 manual and xterm do and the VT320 to VT520 manuals call a
    /// hard one; DECSTR unlocks the keyboard, as 070-6 says DECSCL does.
    /// Then the C1 transmission: `Pc` 0 or 2 eight-bit, 1 seven-bit, at
    /// Levels 2 to 4; at Level 1 seven-bit whatever `Pc` says, "it will be
    /// ignored in Level 1 operation" and "Level 1 operation is 7-bits only"
    /// (070-3 §3.6.4.1). The primary DA answer does not change: it is the
    /// terminal's identity.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_conformance_level(&mut self, csi: &crate::write::Csi) {
        let p = csi.params();
        let Some(level) = ConformanceLevel::from_value(p.raw(ParamIdx::FIRST, 0)) else {
            self.unsupported_param += 1;
            return;
        };
        self.cold.level = level;
        self.soft_reset();
        let c1 = match (level, p.raw(ParamIdx::FIRST.plus(1), 0)) {
            (ConformanceLevel::One, _) => Some(C1Transmit::SevenBit),
            (_, 0 | 2) => Some(C1Transmit::EightBit),
            (_, 1) => Some(C1Transmit::SevenBit),
            _ => None,
        };
        if let Some(c1) = c1 {
            self.cold.replies = self.cold.replies.with_c1(c1);
        }
    }

    /// **A change of the eighth-bit strip holds the rest of the read**, for
    /// `Term::feed` to strip as the state now says and parse at once: a
    /// strip `Term::feed` makes once a read would otherwise reach the bytes
    /// after DECSCL, `ESC <`, `RIS` or `ESC % @` in the same read with the
    /// state before them. `was` is [`Cold::strips`] before the control.
    pub(crate) fn restrip_from(&mut self, was: Strip, node: &mut crate::graph::Node) {
        if self.cold.strips() != was {
            self.hold_read(node);
            self.cold.restrip = crate::term::Restrip::Held;
        }
    }

    /// `ESC SP L`, `M` or `N` at Level 3 or 4 (`[[source: dec-std-070]]`
    /// 070-3 §3.8.1): ASCII into G0 and G0 into GL, and for L and M ISO
    /// Latin-1 Supplemental into G1 and G1 into GR.
    #[cold]
    #[inline(never)]
    pub(crate) fn announce(&mut self, f: StreamByte) {
        self.cold.g[GSlot::G0.index()] = Charset::Ascii;
        self.cold.desig[GSlot::G0.index()] = Designation::ASCII;
        self.cold.gl = GSlot::G0;
        if f.raw_byte() != b'N' {
            self.cold.g[GSlot::G1.index()] = Charset::Iso96(Iso96::Latin1);
            self.cold.desig[GSlot::G1.index()] = Designation::LATIN1_SUPPLEMENTAL;
            self.cold.gr = GSlot::G1;
        }
        self.reinvoke();
    }

    /// DECSTR. **A soft reset stays on the alternate screen and leaves the
    /// cursor where it is.**
    pub(crate) fn soft_reset(&mut self) {
        // **DECSTR leaves the status line, and does not erase it**: "The
        // Status Display will be exited, but not erased" (`[[source:
        // dec-std-070]]` 070-14 p. 14-9), so the rest resets the main
        // display. DECSCL reaches here too: it "performs a Soft Terminal
        // Reset". Its own state waiting parked is reset with the rest.
        self.soft_reset_status();
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
        // reset is a recovery. **The character path in force returns to none**
        // (`crate::path`): DEC's DECSTR table returns its own right-to-left
        // setting, "Cursor direction DECRLM Reset (Left-to-right)"
        // (`[[source: vt510-decstr]]` Table 5-9), and VTE's soft reset its
        // current direction; the lines keep theirs, drawn as they are. **And
        // `?2500`, `?2501` and `?1243` to their power-on states**, the first
        // two in force only, as mintty's soft reset returns its paragraph
        // modes and VTE's every mode, and as DEC's table returns the cursor
        // keys.
        self.cold.line = LineBidi::POWER_ON;
        self.cold.arrows = ArrowSwap::On;
        // "Cursor direction DECRLM Reset (Left-to-right), regardless of NVR
        // setting" (`[[source: vt510-decstr]]`).
        self.cursor_direction(ModeChange::Reset);
        // Every held arrow key's decision forgotten (`crate::arrows`),
        // which `RIS` reaches through here.
        self.cold.held_arrows = HeldArrows::NONE;
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
            // "Keyboard action KAM: Unlocked" (`[[source: vt510-decstr]]`
            // Table 5-9), and xterm's common reset path clears `MODE_KAM`.
            ModeBit::KeyboardAction,
        ] {
            self.cold.modes.apply(m, power_on.is_set(m));
        }
        // IRM reset, so a write under it shifts nothing; HEM is kept.
        self.refresh_insert();
        // The keypad and the key-modifier resources, in `ReallyReset`'s
        // common path -- `MODE_DECKPAM` cleared and `modify_now = modify_1st`
        // -- so both resets restore them. `[[source: vt510-decstr]]` gives the
        // keypad the same: DECNKM "Numeric characters".
        self.cold.keypad = KeypadMode::Numeric;
        // The palette's overrides, in the same common path: `ReallyReset`
        // calls `ResetAnsiColorRequest` for both resets and leaves `OSC 10`,
        // `11` and `12` alone (`[[source: xterm-reset]]`).
        self.reset_palette();
        self.cold.keys = KeyResources::default();
        // The last reported mouse cell, on both resets: `DECSTR` keeps the
        // tracking level and the encoding, and the program recovering with
        // `CSI ! p` is told where the pointer is at its next move rather than
        // relying on a report it may have discarded. `RIS` reaches this
        // through `hard_reset`, and turns tracking off besides. `?1001`'s
        // handshake ends with it, for the same reason: the program recovering
        // has not answered, nor asked for, the press it was owed for.
        self.cold.mouse_at = None;
        self.cold.handshake = Handshake::Idle;
        self.reset_side_margins();
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(self.rows.index() - 1);
        self.cold.protect = CellFlags::NONE;
        // **An open SPA is forgotten**, as xterm's soft reset clears the
        // flag its SPA set (`bitclr(&xw->flags, PROTECTED)`,
        // `[[source: xterm-reset]]`'s file); what EPA guarded stays, and ERM
        // with it, neither in DEC's table (`[[source: vt510-decstr]]`).
        // xterm's reset also sets `protected_mode = OFF_PROTECT`, so its
        // guarded characters become erasable; baud's stay spared, as DEC's
        // table names no guarded area.
        self.cold.guard_open = None;
        self.end_strings();
        self.style_raw = StyleWord::DEFAULT;
        self.cold.saved = None;
        self.style = StyleId::DEFAULT;
        self.reset_variants();
        self.reset_sets();
        // **Both resets restore the cursor shape.** A first version put this in
        // `hard_reset` only, on the argument that `DECSCUSR` is an xterm
        // extension and xterm's soft path leaves it alone -- which is the
        // reason mouse tracking *is* excluded here. `baud-core2`, the oracle
        // then, set it in both; the argument was mine and the code was not
        // checked against it. xterm's `ReallyReset` agrees since it was
        // pinned: its common path, both resets', restores the shape and the
        // blink from the resources (`[[source: xterm-reset]]`'s file) -- the
        // configured style, which is what this restores.
        self.cold.cursor_style = self.cold.initial_cursor;
        // And `?12` with it, on both resets, as xterm's `ReallyReset` clears
        // its escape bit in the common path.
        self.cold.cursor_blink = ModeChange::Reset;
        // XTCHECKSUM back to `csDEC`, on both resets: "Reset checksum
        // extension to the checksumExtension resource" (`[[source:
        // xterm-man]]`'s soft-reset list; `checksum_ext = checksum_ext0`).
        self.cold.checksum_ext = ChecksumExtension::DEC;
        // DECTCEM is shown above, and a shown cursor has no placement
        // (`placed.rs`).
        self.cold.placement.forget();
        self.invoke();
        self.refresh_flags();
        // **The cursor stays**: "DECSTR has no affect on the Active
        // Position" (`[[source: dec-std-070]]` 070-4, DECSTR note c, p. 32),
        // and `soft_terminal_reset` (p. 34) assigns a position only to the
        // save buffer, which `saved = None` above stands for. xterm's soft
        // branch moves nothing either (`[[source: xterm-reset]]`). This homed
        // until 2026-10-14; `RIS` still does, at the end of `hard_reset`.
        // `cx` is kept as it is, so a pending wrap at the screen's edge
        // survives, and one at a narrower right margin is left one column
        // past it, which the widened margins read as a column: the sentinel
        // cannot say which it was (note V's "PendingAt", open).
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
    /// Whether DA lists Sixel: the embedder's claim and a provisioned store.
    /// **The one condition for every Sixel query and mode**, so DA, DECRQM
    /// and XTSMGRAPHICS cannot disagree. `[[source: ctlseqs]]` puts all of
    /// them under one sentence, "If xterm is configured as VT240, VT241,
    /// VT330, VT340 or VT382 using the decTerminalID or decGraphicsID
    /// resource, it supports Sixel Graphics controls": DA's `4`, `?80`,
    /// `?1070`, `?8452` and the `DCS q`. baud's configuration is the claim.
    /// xterm answers DECRQM from its flags whatever its configuration, a
    /// practice departing from that sentence; this follows the text's
    /// reading, an open question. A Sixel sent while unclaimed still draws,
    /// also outside that sentence, and is recorded as open.
    pub(crate) fn sixel_claim(&self) -> SixelClaim {
        match (self.cold.sixel_claim, self.images.provisioned()) {
            (SixelClaim::Yes, true) => SixelClaim::Yes,
            _ => SixelClaim::No,
        }
    }

    /// `?80`, `?1070` or `?8452`: recorded while DA lists Sixel, counted as
    /// unsupported otherwise, so a request baud does not act on is not kept.
    fn set_sixel_mode(&mut self, bit: ModeBit, change: ModeChange) {
        match self.sixel_claim() {
            SixelClaim::Yes => self.cold.modes.apply(bit, change == ModeChange::Set),
            SixelClaim::No => self.unsupported_param += 1,
        }
    }

    /// What DECRQM answers for a Sixel mode: its bit while DA lists Sixel,
    /// "not recognized" otherwise, as [`Paint::set_sixel_mode`] then counts a
    /// request rather than keep it.
    fn sixel_state(&self, bit: ModeBit) -> ModeState {
        match self.sixel_claim() {
            SixelClaim::Yes => bit_state(self.cold.modes, bit),
            SixelClaim::No => ModeState::NotRecognized,
        }
    }

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
            let m = AnsiMode::from_wire(Some(mode));
            // A mode the level in force does not define is not recognised.
            if !introduced_by(m.function()).admits(self.cold.level) {
                return ModeState::NotRecognized;
            }
            return match m {
                AnsiMode::LineFeedNewLine => bit_state(self.cold.modes, ModeBit::LineFeedNewLine),
                AnsiMode::InsertReplace => bit_state(self.cold.modes, ModeBit::InsertReplace),
                AnsiMode::Erasure => bit_state(self.cold.modes, ModeBit::EraseGuarded),
                AnsiMode::TabulationStop => ModeState::PermanentlyReset,
                AnsiMode::KeyboardAction => bit_state(self.cold.modes, ModeBit::KeyboardAction),
                AnsiMode::SendReceive => bit_state(self.cold.modes, ModeBit::SendReceive),
                AnsiMode::BiDirectionalSupport => match self.cold.bidi {
                    BidiSupport::Implicit => ModeState::Set,
                    BidiSupport::Explicit => ModeState::Reset,
                },
                // With BDSM set, "it is considered to be set to DATA"
                // (`[[source: ecma48]]` 7.3.4 c).
                AnsiMode::DeviceComponentSelect => match (self.cold.bidi, self.cold.dcsm) {
                    (BidiSupport::Implicit, _) | (_, DeviceComponent::Data) => ModeState::Set,
                    (BidiSupport::Explicit, DeviceComponent::Presentation) => ModeState::Reset,
                },
                AnsiMode::CharacterEditing => bit_state(self.cold.modes, ModeBit::EditPreceding),
                AnsiMode::GuardedAreaTransfer => {
                    bit_state(self.cold.modes, ModeBit::TransferGuarded)
                }
                _ => ModeState::NotRecognized,
            };
        }
        self.private_state(PrivateMode::from_wire(Some(mode)))
    }

    /// What `DECRQM` answers for one private mode: the one reading XTSAVE
    /// saves and XTRESTORE compares, so a value saved is the value an
    /// application asking would have been told.
    pub(crate) fn private_state(&self, m: PrivateMode) -> ModeState {
        // **A mode the level in force does not define is not recognised**, so
        // DECRQM, XTSAVE and DECTSR read it as `set_private_mode` treats it.
        if !introduced_by(m.function()).admits(self.cold.level) {
            return ModeState::NotRecognized;
        }
        match m {
            PrivateMode::Origin => bit_state(self.cold.modes, ModeBit::Origin),
            PrivateMode::AutoWrap => bit_state(self.cold.modes, ModeBit::AutoWrap),
            PrivateMode::CursorKeys => bit_state(self.cold.modes, ModeBit::CursorKeys),
            PrivateMode::SmoothScroll => bit_state(self.cold.modes, ModeBit::SmoothScroll),
            PrivateMode::ReverseVideo => bit_state(self.cold.modes, ModeBit::ReverseVideo),
            PrivateMode::AutoRepeat => bit_state(self.cold.modes, ModeBit::AutoRepeat),
            PrivateMode::MetaSendsEscape => bit_state(self.cold.modes, ModeBit::MetaSendsEscape),
            PrivateMode::AltSendsEscape => bit_state(self.cold.modes, ModeBit::AltSendsEscape),
            PrivateMode::EightBitInput => bit_state(self.cold.modes, ModeBit::EightBitInput),
            PrivateMode::NumericKeypad => match self.cold.keypad {
                KeypadMode::Application => ModeState::Set,
                KeypadMode::Numeric => ModeState::Reset,
            },
            PrivateMode::BackarrowKey => match self.cold.backarrow {
                BackarrowKey::Backspace => ModeState::Set,
                BackarrowKey::Delete => ModeState::Reset,
            },
            PrivateMode::NumLockModifiers => match self.cold.num_lock {
                NumLockKeypad::Overrides => ModeState::Set,
                NumLockKeypad::Ignored => ModeState::Reset,
            },
            // **Set exactly when the editing Delete sends DEL** under the
            // keyboard in force ([`Keyboard::delete_sends_del`]), the
            // text's "Send DEL from the editing-keypad Delete key".
            PrivateMode::DeleteIsDel => match self.cold.keyboard.delete_sends_del(self.cold.delete)
            {
                true => ModeState::Set,
                false => ModeState::Reset,
            },
            // Set for the member in force alone, as xterm answers each
            // (`[[source: xterm-misc]]` 5723-5747).
            PrivateMode::Keyboard(k) => match self.cold.keyboard == k {
                true => ModeState::Set,
                false => ModeState::Reset,
            },
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
            // **`?1021` mirrors the width policy, as `?1020` the encoding**:
            // `1` or `2`, as xterm answers its read-only `cjkWidth` with
            // `MdBool` (`[[source: xterm-misc]]` 5845-5853), not `3` or `4`.
            // `?1023` is `privateWidth`, which baud always holds: private
            // use is one column under either policy. `?1022`, `emojiWidth`,
            // has no switch here and falls to `0` below.
            PrivateMode::CjkWidthResource => match self.cold.widths.ambiguous() {
                AmbiguousWidth::Wide => ModeState::Set,
                AmbiguousWidth::Narrow => ModeState::Reset,
            },
            PrivateMode::PrivateWidthResource => ModeState::Set,
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
            PrivateMode::HighlightTracking => {
                tracking_state(self.cold.mouse, MouseTracking::Highlight)
            }
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
            // xterm answers both flags (`MdBool(screen->bellIsUrgent)`,
            // `MdBool(screen->poponbell)`, `[[source: xterm-misc]]`).
            PrivateMode::BellIsUrgent => bit_state(self.cold.modes, ModeBit::BellIsUrgent),
            PrivateMode::Scrollbar => match self.cold.scrollbar {
                baud_domain::ScrollbarShown::Shown => ModeState::Set,
                baud_domain::ScrollbarShown::Hidden => ModeState::Reset,
            },
            // xterm answers the flag, `MdBool(screen->curses)` (`[[source:
            // xterm-misc]]` 5569-5570).
            PrivateMode::MoreFix => match self.cold.more_fix {
                baud_domain::MoreFix::On => ModeState::Set,
                baud_domain::MoreFix::Off => ModeState::Reset,
            },
            // xterm answers the flag, `MdBool(screen->marginbell)`
            // (`[[source: xterm-misc]]` 5579-5580).
            PrivateMode::Logging => self.strs.log.state(),
            // xterm answers both, `MdBool(PrinterOf(screen).printer_formfeed)`
            // and `printer_extent` (`[[source: xterm-misc]]` 5544-5548).
            PrivateMode::PrintFormFeed => match self.strs.print.form_feed {
                baud_domain::PrintFormFeed::On => ModeState::Set,
                baud_domain::PrintFormFeed::Off => ModeState::Reset,
            },
            PrivateMode::GraphicsExpandedPrint => match self.strs.print.size {
                baud_domain::PrintSize::Expanded => ModeState::Set,
                baud_domain::PrintSize::Compressed => ModeState::Reset,
            },
            PrivateMode::PrintExtent => match self.strs.print.extent {
                baud_domain::PrintExtent::Page => ModeState::Set,
                baud_domain::PrintExtent::ScrollingRegion => ModeState::Reset,
            },
            PrivateMode::MarginBell => match self.cold.bell.switch {
                baud_domain::MarginBell::On => ModeState::Set,
                baud_domain::MarginBell::Off => ModeState::Reset,
            },
            PrivateMode::PopOnBell => bit_state(self.cold.modes, ModeBit::PopOnBell),
            // xterm answers both (`MdBool(screen->c132)` and, at its VT510
            // level, `MdFlag(xw->flags, NOCLEAR_COLM)`, `[[source:
            // xterm-misc]]`); `?95`'s level gate is note AF's Q-Level.
            PrivateMode::AllowColumnMode => bit_state(self.cold.modes, ModeBit::AllowColumnMode),
            PrivateMode::NoClearOnColumn => bit_state(self.cold.modes, ModeBit::NoClearOnColumn),
            // "DECECM can be reported using DECRQM and DECRPM"
            // (`[[source: vt520-rm]]` p. 5-48). xterm answers `4`,
            // permanently reset, at a VT500 level and above (`vtXX_level >=
            // 5`, `[[source: xterm-misc]]`), and acts on neither state.
            PrivateMode::EraseColor => bit_state(self.cold.modes, ModeBit::EraseScreenBackground),
            // The alternate screen is a whole buffer rather than a flag, and it
            // is implemented, so `DECRQM` must not deny it.
            //
            // `?47` answers the same, from which buffer is shown, whichever
            // mode switched it, as xterm's `do_dec_rqm` answers `?1047` and
            // `?1049` from `whichBuf`, and `?47` too unless print graphics
            // are configured (`[[source: xterm-misc]]` 5610-5617).
            PrivateMode::AltScreen
            | PrivateMode::AltScreenLegacy
            | PrivateMode::AltScreenClearing => match self.cold.screen {
                Screen::Alternate => ModeState::Set,
                Screen::Normal => ModeState::Reset,
            },
            // `?1048` answers whether a cursor is saved on the screen shown,
            // xterm's `MdBool(screen->sc[screen->whichBuf].saved)`
            // (`[[source: xterm-misc]]` 5716-5717).
            PrivateMode::SaveCursor => match self.cold.saved {
                Some(_) => ModeState::Set,
                None => ModeState::Reset,
            },
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
            PrivateMode::SixelDisplay => self.sixel_state(ModeBit::SixelDisplay),
            PrivateMode::SixelPrivateRegisters => self.sixel_state(ModeBit::SixelPrivateRegisters),
            PrivateMode::SixelScrollsRight => self.sixel_state(ModeBit::SixelScrollsRight),
            // `?13` answers the blink the embedder configured, `1` or `2`, as
            // xterm answers its `cursorBlink` resource while it can still
            // change (`[[source: xterm-misc]]`, `srm_CURSOR_BLINK_OPS`).
            // `?14`, `cursorBlinkXOR`, is never what baud does: `?12` is
            // or-ed with the style's blink (rule F).
            PrivateMode::CursorBlinkResource => {
                match self.cold.initial_cursor == self.cold.initial_cursor.with_blink(true) {
                    true => ModeState::Set,
                    false => ModeState::Reset,
                }
            }
            PrivateMode::CursorBlinkXor => ModeState::PermanentlyReset,
            // `?1014` and `?1044` are behaviour baud always has.
            PrivateMode::FastScroll | PrivateMode::KeepClipboard => ModeState::PermanentlySet,
            // **`?1046` answers whether switching is allowed**, the mode's own
            // sense; xterm answers its titeInhibit bit, the inverse, a
            // departure not followed.
            PrivateMode::Resource(r) => match self.cold.resources.is_set(r) {
                true => ModeState::Set,
                false => ModeState::Reset,
            },
            // terminal-wg's three answer the value in force, which a line
            // feed's new line takes.
            PrivateMode::BoxMirroring => match self.cold.line.box_mirror() {
                BoxMirror::On => ModeState::Set,
                BoxMirror::Off => ModeState::Reset,
            },
            PrivateMode::DirectionAutodetect => match self.cold.line.autodetect() {
                Autodetect::On => ModeState::Set,
                Autodetect::Off => ModeState::Reset,
            },
            PrivateMode::ArrowKeySwap => match self.cold.arrows {
                ArrowSwap::On => ModeState::Set,
                ArrowSwap::Off => ModeState::Reset,
            },
            // **`?42` answers `0`**, as STD 070's DECRQM has it: "DEC
            // Private modes that cannot be set or reset from the host will
            // report as unknown" (p. 5-193), and no program can set it
            // here. The arm equals the wildcard below on purpose, so the
            // answer stays `0` if the wildcard changes.
            PrivateMode::NationalReplacement => ModeState::NotRecognized,
            PrivateMode::CursorRightToLeft => {
                bit_state(self.cold.modes, ModeBit::CursorRightToLeft)
            }
            PrivateMode::RightToLeftCopy => match self.cold.copy {
                CopyDirection::RightToLeft => ModeState::Set,
                CopyDirection::LeftToRight => ModeState::Reset,
            },
            PrivateMode::HebrewEncoding => match self.cold.hebrew {
                HebrewEncoding::Multinational => ModeState::Set,
                HebrewEncoding::National => ModeState::Reset,
            },
            PrivateMode::HebrewKeyboard => match self.cold.hebrew_keyboard {
                HebrewKeyboard::Hebrew => ModeState::Set,
                HebrewKeyboard::NorthAmerican => ModeState::Reset,
            },
            _ => ModeState::NotRecognized,
        }
    }
}
