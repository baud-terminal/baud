//! Sixel: a `DCS Pa;Pb;Ph q ... ST` image, decoded into the image store.
//!
//! The parser routes a bare-`q` DCS to `Act::SixelStart` (a `q` from
//! `Node::Dcs` without the `$` that marks DECRQSS), which renames the string
//! kind so the body accumulates in `sixel_body` -- an OSC body is far smaller
//! and uses `pending`. At the terminator `str_end` calls [`Paint::sixel_end`],
//! which measures the body with the same walk the decode uses, reserves a span,
//! decodes into it **in place**, and places the image at the cursor. Every
//! failure is refused and counted, never a wrong or half image.
//!
//! The cursor advances past the image by `ceil(Pv / cell_h)` rows through the
//! **existing scroll path** ([`Paint::index`]), so an image at the bottom of
//! the screen scrolls and `book.scrolled` moves -- which keeps the anchor of
//! the *next* image right, and which assigning `cy` directly would not. The
//! core lacks only the one number the renderer owns, the cell height in
//! pixels, so the embedder supplies it through [`Term::set_cell_size`]; a
//! caller that never does (a headless test, a bench) leaves it zero and the
//! cursor unmoved, today's behaviour. `[[source: vt3xx-sixel]]`'s mode-8452
//! variant -- the cursor to the *right* of the image rather than below -- is
//! not implemented and falls through to the unsupported-mode counter.

use crate::paint::Paint;
use crate::term::SixelBg;
use baud_domain::*;

impl Paint<'_, '_> {
    /// Decode the accumulated Sixel body and place the image at the cursor.
    #[cold]
    #[inline(never)]
    pub(crate) fn sixel_end(&mut self) {
        let n = self.cold.sixel_len.index();
        // The length kept counting past the scratch buffer, so `n` beyond it
        // means bytes were dropped: refuse the whole image rather than decode a
        // truncated one. An unprovisioned store (an empty `sixel_body`) refuses
        // every non-empty Sixel here, which is the correct nothing -- a
        // terminal with no image memory draws none and says how many it lost.
        if n == 0 || n > self.sixel_body.len() {
            self.images.images_refused += 1;
            return;
        }
        let body = &self.sixel_body[..n];
        // Measure with the same grammar walk `decode` uses, so the reserved
        // span is exactly what the decode fills.
        let report = Sixel::extent(body);
        let (w, h) = (report.width, report.height);
        if w == SixelPx::ZERO || h == SixelPx::ZERO {
            self.images.images_refused += 1;
            return;
        }
        let Some((id, buf)) = self
            .images
            .store(ImagePx::of(w.raw()), ImagePx::of(h.raw()))
        else {
            // `store` already counted the refusal -- no free slot, or no gap in
            // the arena large enough for `w * h`.
            return;
        };
        // Decode into the reserved span in place; the borrow ends with the
        // block so the placement below can reborrow the store. `refused` is
        // **structurally zero** here and the report is dropped for that reason:
        // the canvas is `w` wide over exactly `w * h` words, so its height is
        // `h`, and `decode` walks the same grammar `extent` measured -- every
        // pixel it paints is inside the span reserved from that measurement.
        // (`pixels_past_the_canvas_are_refused_and_counted_never_wrapped` is the
        // case where a caller sizes a canvas *smaller* than the data; here they
        // are equal.)
        {
            let mut canvas = SixelCanvas::new(buf, w);
            let filled = Sixel::decode(body, &mut canvas);
            debug_assert_eq!(filled.refused, 0, "the span was sized from extent");
        }
        // The image stored, so an ignored opaque-background request is now a
        // real one -- counted here, after the store, rather than at `SixelStart`
        // where a Sixel later refused would have inflated the number the fill
        // decision reads. (`buf`'s borrow of the store has ended with the block
        // above, so the counter can be written.)
        if self.cold.sixel_bg == SixelBg::Opaque {
            self.images.sixel_p2_ignored += 1;
        }
        // Anchor to the cursor's absolute line and column. The line is
        // `scrolled + cursor row` -- `book.scrolled` is the monotonic,
        // unbounded count of lines that have left the top (unlike `hist`, which
        // caps at `depth`), so two images placed past a full ring get distinct
        // lines, and it is the same count `Term::scrolled()` exposes for the
        // renderer to position against. `place` counts a refused placement; an
        // image that fails to place keeps a zero refcount and is freed by `RIS`.
        //
        // **`Pa` and `Ph` are dropped; `Pb`/`P2` is read at `SixelStart` and
        // counted.** The graph now routes the `Node::Dcs` parameters into `csi`
        // (`Act::Digit`/`Sep`), so `SixelStart` reads the transparency selector
        // and bumps `sixel_p2_ignored` when it asks for an opaque background
        // baud renders transparent -- `s-sixel.md` records why the fill itself
        // is not built. The aspect ratio `Pa` and grid `Ph` are still dropped;
        // baud draws one image-pixel per cell-pixel and honours no aspect.
        let abs = self.book.scrolled.index().saturating_add(self.cy.index());
        let line = LineIdx::of(u64::try_from(abs).unwrap_or(u64::MAX));
        let _ = self.images.place(id, line, self.cx, ZIndex::ZERO);
        // Advance the cursor below the image, through the scroll path so a
        // bottom-of-screen image scrolls and `book.scrolled` moves with it --
        // the anchor of the next image reads the moved count. `index` does not
        // carriage-return, so the column stays where the image started; the DEC
        // source for the exact column is unread, and the test pins this choice.
        // Only when the embedder has said how tall a cell is; zero leaves the
        // cursor where the image is, the headless default. `h` was refused when
        // zero three branches up and `cell_height` is non-zero here, so the row
        // count is at least one without a guard.
        if self.cell_height != ImagePx::ZERO {
            let rows = usize::from(h.raw()).div_ceil(self.cell_height.index());
            for _ in 0..rows {
                self.index();
            }
        }
    }

    /// Free any Sixel a write at the cursor's row over columns `[col0, col1)`
    /// lands on -- text over an image erases it, which is how a previewer
    /// replaces one preview with the next. Without it a previewer's images
    /// pile up at one cell until the store fills and every later preview is
    /// refused. Gated on `any_placed`, so a stream with no image on screen
    /// pays one comparison; the absolute line is `scrolled + cursor row`, the
    /// same anchor `sixel_end` places against, so a placement and its
    /// overwrite name the same cell.
    #[inline(always)]
    pub(crate) fn overwrite_images(&mut self, col0: Col, col1: Col) {
        if !self.images.any_placed() {
            return;
        }
        let abs = self.book.scrolled.index().saturating_add(self.cy.index());
        let line = LineIdx::of(u64::try_from(abs).unwrap_or(u64::MAX));
        self.images.overwrite(
            line,
            col0.index(),
            col1.index(),
            self.cell_width,
            self.cell_height,
        );
    }
}
