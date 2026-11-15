//! Sixel: a `DCS Pa;Pb;Ph q ... ST` image, decoded into the image store.
//!
//! The parser routes a bare-`q` DCS to `Act::SixelStart` (a `q` from
//! `Node::Dcs` without the `$` that marks DECRQSS), which renames the string
//! kind and begins the image: the decoder (`SixelStream`, in `StrBuf`)
//! claims the image arena's largest free gap. Each body byte the handler
//! keeps is decoded **as it arrives**, through the same `StrPut` path an OSC
//! body takes, and paints into the claim -- no copy of the body is kept, so a
//! body of any length draws, and what limits an image is its pixels, as
//! `[[source: dec-std-070]]` 070-9 has it. At the terminator `str_end` calls
//! [`Paint::sixel_end`], which records the image where the store would have
//! put it and places it at the cursor. Every failure is refused and counted,
//! never a wrong or half image.
//!
//! The cursor advances past the image by `ceil(h / cell_h)` rows, `h` the
//! height the data reached (not the raster's `Pv`), through the
//! **existing scroll path** ([`Paint::index`]), so an
//! image at the bottom of the screen scrolls and the window's top line moves --
//! which keeps the anchor of the *next* image right, and which assigning `cy`
//! directly would not. The core lacks only the one number the renderer owns,
//! the cell height in pixels, so the embedder supplies it through
//! [`Term::set_cell_size`]; a caller that never does (a headless test, a
//! bench) leaves it zero and the cursor unmoved.
//!
//! Three modes change this, each recorded only while DA lists Sixel
//! ([`Paint::sixel_claim`]): `?80` (DECSDM) set puts the image at the
//! window's upper-left cell, stops it at the screen's bottom edge and leaves
//! the cursor; `?8452` puts the cursor to the right of the image
//! (`[[source: ctlseqs]]`, "Sixel scrolling leaves cursor to right of
//! graphic, RLogin, xterm"); and `?1070` reset carries the colour registers
//! from one Sixel to the next.

use crate::paint::Paint;
use crate::term::{Screen, SixelBg};
use baud_domain::*;

impl Paint<'_, '_> {
    /// Begin the decoder for a Sixel drawn at `aspect`. **Out of line**: it
    /// reads the modes, the foreground and the screen's height, which the
    /// byte loop does not, and inlined into `Act::SixelStart` it grew the
    /// per-byte loop by 128 instructions and 54 loads from the stack.
    ///
    /// An undefined register paints the foreground shown -- 070-9 §10's
    /// "default Sixel color", "Contrasting to background"; a foreground the
    /// embedder has not said is `DEFAULT`, drawn as nothing. `?1070` picks
    /// whose registers it starts from, and `?80`'s display mode stops it at
    /// the screen's bottom and right edges.
    ///
    /// The right edge is `[[source: dec-std-070]]` 070-9 §11.2.2, "Right
    /// Margin: Sixels defined to be printed past the right margin are not
    /// printed": the image is drawn at the window's first column, so no
    /// pixel at or past the screen's width is painted or counted in its
    /// width. A tall image whose ignored new lines lay its last bands side
    /// by side is then at most the screen, not refused for the arena.
    ///
    /// **A horizontal grid size, `P3`, falls back to baud's one grid**: a
    /// sixel column is one device pixel whatever the envelope asks, as "The
    /// VT300 ignores this parameter because the horizontal grid size is
    /// fixed" (`[[source: vt3xx-sixel]]`), and 070-9 §12 asks a device to
    /// "clearly state in their product documentation how grid sizes fall
    /// back" (PDF p. 928), which `docs/vt-coverage.md` does. One above zero
    /// is counted in `ImageStore::sixel_grids_fixed`. No text maps a
    /// decipoint to a screen pixel, and this is chosen over scaling by the
    /// display's points.
    #[cold]
    #[inline(never)]
    pub(crate) fn sixel_begin(&mut self, aspect: SixelAspect, grid: Option<u16>) {
        // **No image on the status line**: an image is placed on a line of
        // the main display, which the line is not. The string is kept and
        // discarded, and counted at its end (`status.rs`).
        if self.on_status() {
            self.cold.str_kind = crate::osc::StrKind::Other;
            return;
        }
        if grid.is_some_and(|g| g > 0) {
            self.images.sixel_grids_fixed += 1;
        }
        let shown = self.cold.shown(DynamicColor::Foreground);
        let ink = match shown.as_rgb() {
            Some((r, g, b)) => Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)),
            None => Pixel::CLEAR,
        };
        let registers = match self.cold.modes.is_set(ModeBit::SixelPrivateRegisters) {
            true => SixelRegisters::Private,
            false => SixelRegisters::Shared,
        };
        let (floor, right) = match self.cold.modes.is_set(ModeBit::SixelDisplay) {
            true => (
                SixelEdge::screen(self.rows.index(), self.cell_height),
                SixelEdge::screen(self.cols.index(), self.cell_width),
            ),
            false => (SixelEdge::Open, SixelEdge::Open),
        };
        let fill = match self.cold.sixel_bg {
            SixelBg::Transparent => SixelFill::Keep,
            SixelBg::Opaque => self.sixel_fill_to_margins(),
        };
        let begin = SixelBegin {
            aspect,
            ink,
            registers,
            floor,
            right,
            fill,
        };
        self.strs.sixel.begin_with(begin, self.images);
    }

    /// **The region a `P2` of 0 or 2 fills when no raster extent is sent**:
    /// from the image's corner to the right margin and the bottom margin in
    /// force, `[[source: dec-std-070]]` 070-9 §10's "Distance from current
    /// ANSI text position to current ANSI right margin" and "to current ANSI
    /// bottom margin" (PDF p. 926), as a VT340 clears "from the cursor to
    /// the bottom right corner" (`[[source: vt340test-p2effect-sh]]`
    /// 12-13). The corner is where the image is placed: the cursor, or in
    /// display mode the screen's upper-left cell. A cursor past a margin
    /// fills to the screen's edge on that side. In device pixels, so zero
    /// while the cell size is unknown; with the cell's height, the step the
    /// fill goes on by as the image scrolls past the rows left, zero in
    /// display mode, where the image never scrolls the screen.
    fn sixel_fill_to_margins(&self) -> SixelFill {
        let display = self.cold.modes.is_set(ModeBit::SixelDisplay);
        let (col, row) = match display {
            true => (0, 0),
            false => (self.cx.col().index(), self.cy.index()),
        };
        let (cols, rows) = (self.cols.index(), self.rows.index());
        let right = match col <= self.cold.right.index() {
            true => self.cold.right.index(),
            false => cols - 1,
        };
        let bottom = match row <= self.cold.bot.index() {
            true => self.cold.bot.index(),
            false => rows - 1,
        };
        // As display columns: twice the character positions on a double
        // line ([`Paint::display_col`]).
        let k = match display {
            true => 1,
            false => 1 + usize::from(self.view.rendition(self.cy).is_double()),
        };
        let w_cells = ((right + 1) * k).min(cols).saturating_sub(col * k);
        let h_cells = bottom + 1 - row;
        let px = |n: usize, cell: ImagePx| {
            u32::try_from(n.saturating_mul(cell.index())).unwrap_or(u32::MAX)
        };
        SixelFill::Background {
            w: px(w_cells, self.cell_width),
            h: px(h_cells, self.cell_height),
            cell: match display {
                true => 0,
                false => px(1, self.cell_height),
            },
        }
    }

    /// Record the Sixel decoded as its body arrived, and place it at the
    /// cursor.
    #[cold]
    #[inline(never)]
    pub(crate) fn sixel_end(&mut self) {
        // **Refused in the order the measured decode refused**, each naming
        // the limit it hit through `refuse`: wider or taller than a
        // coordinate holds, no pixels at all -- an empty body among them --
        // no free slot, then no gap in the arena for `w * h`, with the size.
        // **No refusal is for the body's length**: a Sixel decodes as it
        // arrives and none is kept. Until 2026-10-14 a body was held whole in
        // a scratch of `[images] body_kb` bytes and one longer was refused --
        // yazi's screenshot, photo and text-PDF previews ran 334 KiB to
        // 820 KiB against the app's 256 KiB, and the pane went blank while
        // the arena had room. An unprovisioned store's empty arena refuses
        // every Sixel with pixels for the arena, which is the correct nothing
        // -- a terminal with no image memory draws none and says how many it
        // lost, and how large each was.
        //
        // **Each row was cleared as the image reached it**, because the claim
        // holds whatever the arena held there: the decode paints set bits
        // only, so every position the data leaves unset -- an unset bit, a
        // band's rows past the last set bit -- showed a freed image's pixels
        // until 2026-10-12, when the span was cleared before a buffered
        // decode. `[[source: vt3xx-sixel]]` gives such a position two
        // meanings, `P2` 1 "remain at their current color" and 0 or 2 "set
        // to the current background color" (192-203); `[[source: dec-std-070]]`
        // 070-9 §7.2 (PDF p. 918) gives 1 "no action", 2 "set to background
        // color" and 0 "device default action". A transparent pixel is what
        // "current color" composites to (a reading); under `P2` 0 or 2 the
        // decode sets each such position to the background
        // ([`SixelFill`], `Pixel::FILL`), counted in `sixels_filled`; and an
        // erased image is none of these. `w` and `h` are how far the data
        // reached, which the fill may pass.
        //
        // **The pictures already stored take what a shared body redefined
        // at the read's end** (`SixelStream::recolour`, in `Term::feed`),
        // once for every body of the read.
        let Some((id, w, h)) = self.strs.sixel.finish_deferring(self.images) else {
            return;
        };
        // Anchor to the cursor's absolute line and column on the live screen:
        // `top + cursor row`, where `top` is the line at the window's top in
        // that screen's numbering (`Book::top`, `Book::alt_top`). It moves with
        // the window, so the anchor follows its row -- and it is what
        // `Term::image_origin` positions the renderer against. `place` counts a
        // refused placement; an image that fails to place keeps a zero refcount
        // and is freed by `RIS`.
        //
        // **`Pa` sets the aspect ratio, `Pb`/`P2` the fill, and the
        // envelope's grid size falls back to one grid and is counted.** The graph routes the `Node::Dcs`
        // parameters into `csi` (`Act::Digit`/`Sep`), so `SixelStart` begins
        // the decode at the macro parameter's ratio and with the fill the
        // background selector asks for ([`Paint::sixel_begin`]). The third parameter, the
        // horizontal grid size that ctlseqs calls `Ph` and the VT3xx chapter
        // `P3`, falls back: a sixel pixel is one image pixel wide, as "The
        // VT300 ignores this parameter because the horizontal grid size is
        // fixed" (`[[source: vt3xx-sixel]]`), the fallback 070-9 §12 asks a
        // device to state ([`Paint::sixel_begin`]). It is not the raster command's `Ph`,
        // the horizontal extent, which the decode lays its rows out by.
        //
        // **Display mode (`?80` set) puts the image at the window's
        // upper-left cell and leaves the cursor**: `[[source: vt3xx-sixel]]`,
        // "When sixel scrolling is disabled, the sixel active position begins
        // at the upper-left corner of the active graphics page", and "When
        // sixel mode is exited, the text cursor does not change from the
        // position it was in when sixel mode was entered". The corner is the
        // screen's, whatever DECOM and the margins say, as xterm places it
        // (`get_new_graphic(xw, 0, 0)`, `[[source: xterm-graphics-sixel]]`).
        // The window's top line is spelled here rather than through
        // `image_top`, for the reason `image_top`'s doc gives.
        if self.cold.modes.is_set(ModeBit::SixelDisplay) {
            let (screen, top) = match self.cold.screen {
                Screen::Normal => (ImageScreen::Primary, self.book.top),
                Screen::Alternate => (ImageScreen::Alternate, self.book.alt_top),
            };
            let _ = self.images.place(id, screen, top, Col::ZERO, ZIndex::ZERO);
            self.rehook();
            return;
        }
        let (screen, line) = self.image_anchor();
        let at = self.display_col(self.cx.col());
        let _ = self.images.place(id, screen, line, at, ZIndex::ZERO);
        // Text written later in this read may land on it (`Paint::hooks`).
        self.rehook();
        // Advance the cursor below the image, through the scroll path so a
        // bottom-of-screen image scrolls and the window's top line moves with
        // it -- the anchor of the next image reads the moved line. `index` does
        // not carriage-return, so the column stays where the image started, as
        // `[[source: dec-std-070]]` 070-9 §11.3 requires: "The Horizontal
        // Active position of the ANSI text is not affected by Sixel mode and
        // set exactly to what is was when Sixel mode was entered" (PDF p.
        // 928). Only when the embedder has said how tall a
        // cell is; zero leaves the cursor where the image is, the headless
        // default. `finish` refused a zero `h` and `cell_height` is non-zero
        // here, so the row count is at least one without a guard.
        //
        // **Which row is open.** The row below the image departs from both
        // texts: `[[source: vt3xx-sixel]]` sets the text cursor "to the
        // current sixel cursor position", which a VT340 rounds up to the row
        // it falls in (`[[source: vt340test-textcursor-sh]]`), and 070-9
        // §11.3 moves it by "the vertical motion commands executed", allowing
        // "an adjustment less than one character height ... in a downward
        // direction". Whether to take the VT340's row is open, and this is
        // unchanged until it is decided.
        if self.cell_height == ImagePx::ZERO {
            return;
        }
        let rows = usize::from(h.raw()).div_ceil(self.cell_height.index());
        // **`?8452`: the cursor to the right of the image**
        // (`[[source: ctlseqs]]`, "Sixel scrolling leaves cursor to right
        // of graphic"), on the image's last row and `ceil(w / cell_w)`
        // columns past where it started, as xterm's `finished_parsing` puts
        // it (`[[source: xterm-graphics-sixel]]`:
        // `new_row = charrow - 1 + ceil(h / FontHeight)`, and under
        // `sixel_scrolls_right` only the column changes). Past the right
        // margin it goes to the left margin of the next row, scrolling at
        // the bottom. No column moves while the cell width
        // is unknown, and the cursor then goes below the image as without
        // the mode.
        let right_of =
            self.cold.modes.is_set(ModeBit::SixelScrollsRight) && self.cell_width != ImagePx::ZERO;
        // The image's right edge on the display, from where it began.
        let edge = Col::at_index(
            at.index() + usize::from(w.raw()).div_ceil(self.cell_width.index().max(1)),
        );
        // A line an image scrolls in takes the direction in force, as a
        // line feed's does (`crate::path`).
        for _ in 0..rows - usize::from(right_of) {
            self.index_bringing_in();
        }
        if right_of {
            // As a column of the row the cursor ends on: a character
            // position on a double line (`Paint::char_at_display`).
            let right = self.char_at_display(edge);
            if right > self.line_fence() {
                self.index_bringing_in();
                self.cx = CursorCol::moved(self.cold.left);
            } else {
                self.cx = CursorCol::moved(right);
            }
        }
    }

    /// **The display column character position `c` of the cursor's line
    /// starts at**: `c` on a single-width line, and on any other the left
    /// edge of its doubled field, "column 9 of the physical display" for
    /// position 5 (`[[source: dec-std-070]]` § 5.6.1, p. 5-95). Where an
    /// image begun at the cursor is placed: "the sixel active position
    /// begins at the upper-left corner of the ANSI text active position"
    /// (`[[source: vt3xx-sixel]]`), unscaled, as "No other information is
    /// carried from the ANSI text state" (070-9 § 11.1); xterm places it
    /// at the character position's single-width column
    /// (`[[source: xterm-graphics-c]]`, `charcol * FontWidth` in
    /// `refresh_graphic`). An image's columns are the display's from then
    /// on: it stays where it was drawn.
    pub(crate) fn display_col(&self, c: Col) -> Col {
        match self.view.rendition(self.cy).is_double() {
            true => Col::at_index((c.index() * 2).min(self.cols.index() - 1)),
            false => c,
        }
    }

    /// The character position of the cursor's line that display column
    /// `edge` begins, rounded up: `edge` on a single-width line, half of it
    /// on any other. Where the cursor goes right of an image.
    pub(crate) fn char_at_display(&self, edge: Col) -> Col {
        match self.view.rendition(self.cy).is_double() {
            true => Col::at_index(edge.index().div_ceil(2)),
            false => edge,
        }
    }

    /// Where an image placed at the cursor anchors: the live screen, and the
    /// absolute line of the cursor's row in that screen's numbering.
    pub(crate) fn image_anchor(&self) -> (ImageScreen, LineIdx) {
        let cy = Rows::of_count(self.cy.index());
        match self.cold.screen {
            Screen::Normal => (ImageScreen::Primary, self.book.top.plus(cy)),
            Screen::Alternate => (ImageScreen::Alternate, self.book.alt_top.plus(cy)),
        }
    }

    /// The live screen, and the line its window's top row is in that
    /// screen's numbering: what a screen row is turned into a line against
    /// for the rows a region moves under an image. [`Paint::image_anchor`]
    /// spells the same match rather than calling this, and on purpose: it is
    /// inlined into every write's gated tail, and routing it through here
    /// grew the per-byte loop by 20 bytes. Kept apart, the loop is
    /// instruction for instruction what it was before regions moved images.
    pub(crate) fn image_top(&self) -> (ImageScreen, LineIdx) {
        match self.cold.screen {
            Screen::Normal => (ImageScreen::Primary, self.book.top),
            Screen::Alternate => (ImageScreen::Alternate, self.book.alt_top),
        }
    }

    /// **Clear the graphic pixels under cells `[a, b)` of row `r`**, which
    /// an erase or a shift of characters left showing no character there
    /// (`ImageStore::erase`): ED, EL, ECH, DECERA, the selective erases in
    /// the positions they erase, and ICH, DCH and IRM's gap over the span
    /// they shift. DECIC and DECDC leave an image where it is. Gated on
    /// `any_placed`, so a stream with no image on screen pays one
    /// comparison.
    #[inline(always)]
    pub(crate) fn erase_images(&mut self, r: Row, a: Col, b: Col) {
        if self.images.any_placed() {
            self.erase_images_on(r, a, b);
        }
    }

    #[cold]
    #[inline(never)]
    fn erase_images_on(&mut self, r: Row, a: Col, b: Col) {
        // **The status line holds no picture**, and its row is not the
        // main display's: an erase on it leaves the screen's pictures.
        if self.on_status() {
            return;
        }
        let (screen, top) = self.image_top();
        let k = 1 + usize::from(self.view.rendition(r).is_double());
        self.images.erase(
            screen,
            top.plus(Rows::of_count(r.index())),
            a.index() * k,
            b.index() * k,
            self.cell_width,
            self.cell_height,
        );
    }

    /// [`Paint::erase_images`] for the cells of `[a, b)` that `kept` leaves
    /// out, a run at a time: a protected position a selective erase spares,
    /// a guarded one ERM spares.
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_images_where(
        &mut self,
        r: Row,
        a: Col,
        b: Col,
        kept: impl Fn(PackedCell) -> bool,
    ) {
        if !self.images.any_placed() {
            return;
        }
        let mut x = a.index();
        while x < b.index() {
            if kept(self.view.get(r, Col::at_index(x))) {
                x += 1;
                continue;
            }
            let start = x;
            while x < b.index() && !kept(self.view.get(r, Col::at_index(x))) {
                x += 1;
            }
            self.erase_images_on(r, Col::at_index(start), Col::at_index(x));
        }
    }

    /// Free any Sixel a write at the cursor's row over columns `[col0, col1)`
    /// lands on -- text over an image erases it, which is how a previewer
    /// replaces one preview with the next. Without it a previewer's images
    /// pile up at one cell until the store fills and every later preview is
    /// refused. Gated on `any_placed`, so a stream with no image on screen
    /// pays one comparison; the anchor is [`Paint::image_anchor`], the one
    /// `sixel_end` places against, so a placement and its overwrite name the
    /// same cell on the same screen.
    ///
    /// **By the display columns the write covers**, which is what an
    /// image's columns are ([`Paint::display_col`]): twice the character
    /// positions on a double line.
    #[inline(always)]
    pub(crate) fn overwrite_images(&mut self, col0: Col, col1: Col) {
        // The status line's text frees none of the main display's images.
        if !self.images.any_placed() || self.on_status() {
            return;
        }
        let (screen, line) = self.image_anchor();
        let k = 1 + usize::from(self.view.rendition(self.cy).is_double());
        self.images.overwrite(
            screen,
            line,
            col0.index() * k,
            col1.index() * k,
            self.cell_width,
            self.cell_height,
        );
    }
}
