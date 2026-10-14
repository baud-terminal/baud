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
//! **existing scroll path** ([`Paint::index`]), so an image at the bottom of
//! the screen scrolls and the window's top line moves -- which keeps the anchor of
//! the *next* image right, and which assigning `cy` directly would not. The
//! core lacks only the one number the renderer owns, the cell height in
//! pixels, so the embedder supplies it through [`Term::set_cell_size`]; a
//! caller that never does (a headless test, a bench) leaves it zero and the
//! cursor unmoved, today's behaviour. `[[source: vt3xx-sixel]]`'s mode-8452
//! variant -- the cursor to the *right* of the image rather than below -- is
//! not implemented and falls through to the unsupported-mode counter.

use crate::paint::Paint;
use crate::term::{Screen, SixelBg};
use baud_domain::*;

impl Paint<'_, '_> {
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
        // "current color" composites to (a reading) and baud's counted
        // stand-in for the background, and an erased image is none of these.
        let Some((id, _, h)) = self.strs.sixel.finish(self.images) else {
            return;
        };
        // The image stored, so an ignored opaque-background request is now a
        // real one -- counted here, after the store, rather than at `SixelStart`
        // where a Sixel later refused would have inflated the number the fill
        // decision reads.
        if self.cold.sixel_bg == SixelBg::Opaque {
            self.images.sixel_p2_ignored += 1;
        }
        // Anchor to the cursor's absolute line and column on the live screen:
        // `top + cursor row`, where `top` is the line at the window's top in
        // that screen's numbering (`Book::top`, `Book::alt_top`). It moves with
        // the window, so the anchor follows its row -- and it is what
        // `Term::image_origin` positions the renderer against. `place` counts a
        // refused placement; an image that fails to place keeps a zero refcount
        // and is freed by `RIS`.
        //
        // **`Pa` sets the aspect ratio, `Pb`/`P2` is counted, and the
        // envelope's grid size is dropped.** The graph routes the `Node::Dcs`
        // parameters into `csi` (`Act::Digit`/`Sep`), so `SixelStart` begins
        // the decode at the macro parameter's ratio and reads the
        // transparency selector, bumping `sixel_p2_ignored` when it asks for
        // an opaque background baud renders transparent -- `s-sixel.md`
        // records why the fill itself is not built. The third parameter, the
        // horizontal grid size that ctlseqs calls `Ph` and the VT3xx chapter
        // `P3`, is dropped: a sixel pixel is one image pixel wide, as "The
        // VT300 ignores this parameter because the horizontal grid size is
        // fixed" (`[[source: vt3xx-sixel]]`), and what 070-9 §7.3 falls back
        // to is for Jev (note AN, `Ph`). It is not the raster command's `Ph`,
        // the horizontal extent, which the decode lays its rows out by.
        let (screen, line) = self.image_anchor();
        let _ = self.images.place(id, screen, line, self.cx, ZIndex::ZERO);
        // Advance the cursor below the image, through the scroll path so a
        // bottom-of-screen image scrolls and the window's top line moves with
        // it -- the anchor of the next image reads the moved line. `index` does
        // not carriage-return, so the column stays where the image started, as
        // `[[source: dec-std-070]]` 070-9 §11.3 requires: "The Horizontal
        // Active position of the ANSI text is not affected by Sixel mode and
        // set exactly to what is was when Sixel mode was entered" (PDF p. 928,
        // as note AN quotes it). Only when the embedder has said how tall a
        // cell is; zero leaves the cursor where the image is, the headless
        // default. `finish` refused a zero `h` and `cell_height` is non-zero
        // here, so the row count is at least one without a guard.
        if self.cell_height != ImagePx::ZERO {
            let rows = usize::from(h.raw()).div_ceil(self.cell_height.index());
            for _ in 0..rows {
                self.index();
            }
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

    /// Free any Sixel a write at the cursor's row over columns `[col0, col1)`
    /// lands on -- text over an image erases it, which is how a previewer
    /// replaces one preview with the next. Without it a previewer's images
    /// pile up at one cell until the store fills and every later preview is
    /// refused. Gated on `any_placed`, so a stream with no image on screen
    /// pays one comparison; the anchor is [`Paint::image_anchor`], the one
    /// `sixel_end` places against, so a placement and its overwrite name the
    /// same cell on the same screen.
    #[inline(always)]
    pub(crate) fn overwrite_images(&mut self, col0: Col, col1: Col) {
        if !self.images.any_placed() {
            return;
        }
        let (screen, line) = self.image_anchor();
        self.images.overwrite(
            screen,
            line,
            col0.index(),
            col1.index(),
            self.cell_width,
            self.cell_height,
        );
    }
}
