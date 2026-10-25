//! **iTerm2's inline images, wired** (`[[source: iterm2-images]]`): a
//! `File=args:base64` body streamed into the PNG scratch from its `:` on, as
//! a `Copy=:` body streams into the clipboard areas (`clip.rs`), and a
//! `MultipartFile`'s `FilePart`s the same way from each part's `=`; at the
//! end the picture is decoded, sized by iTerm2's arithmetic
//! ([`file_geometry`]), resampled to the device pixels it is drawn at,
//! stored as an iTerm2 picture and placed at the cursor, which moves past
//! it as iTerm2 moves its own.
//!
//! **Drawn as a Sixel is**: one image pixel a device pixel at its anchor
//! cell, so the three drawing paths and the pixel oracle draw it with no
//! code of their own; it scrolls with its lines, an erase clears the cells
//! it covers, and text written over it clears the cells the text lands on,
//! as iTerm2's image characters are replaced one at a time
//! ([`ImageOwner::Iterm`]).
//!
//! **One scratch, two streams.** The kitty stream keeps its PNGs in the
//! same scratch, so whichever begins second takes it: an inline file
//! finishes a kitty decode waiting for the scratch and abandons a kitty PNG
//! still arriving in chunks, counted cancelled; an `APC G` abandons a file
//! still arriving, counted under its key.

use crate::osc::StrKind;
use crate::paint::Paint;
use baud_domain::*;

impl Paint<'_, '_> {
    /// **A file's arguments ended**: a `File=` body reached its `:`, or a
    /// `MultipartFile` arrived. The data that follows goes to the file
    /// stream; a file open before it is abandoned and counted.
    #[cold]
    #[inline(never)]
    pub(crate) fn file_begin(&mut self, args: FileArgs, parts: FileParts) {
        if args.shown == FileShown::Inline && self.strs.kitty.holds_scratch() {
            let s = &mut *self.strs;
            s.kitty.yield_scratch(self.images, s.kitty_scratch);
            self.kitty_replies();
        }
        let key = match self.strs.iterm_file.parts() {
            Some(FileParts::Multipart) => ItermKey::MultipartFile,
            _ => ItermKey::File,
        };
        if let Some(used) = self.strs.iterm_file.begin(args, parts) {
            self.give_back(used);
            self.not_acted(key);
        }
    }

    /// **The body just became `1337;File=args:`**: the header before the
    /// `:` is in `pending` up to `colon`, and the rest streams.
    #[cold]
    #[inline(never)]
    pub(crate) fn file_route(&mut self, colon: ByteLen) {
        let mut header = [0u8; crate::storage::OSC_MAX];
        let n = colon.index() - 10;
        header[..n].copy_from_slice(&self.strs.pending[10..colon.index()]);
        self.file_begin(FileArgs::parse(&header[..n]), FileParts::Whole);
        self.cold.str_kind = StrKind::ItermFile;
    }

    /// **The body just became `1337;FilePart=`**: a part of the file in
    /// parts open streams from here; with none open it stays in `pending`
    /// for the dispatcher, which counts it.
    #[cold]
    #[inline(never)]
    pub(crate) fn file_part_route(&mut self) {
        if self.strs.iterm_file.parts() == Some(FileParts::Multipart) {
            self.cold.str_kind = StrKind::ItermFile;
        }
    }

    /// One data byte.
    #[cold]
    #[inline(never)]
    pub(crate) fn file_put(&mut self, b: StreamByte) {
        let s = &mut *self.strs;
        s.iterm_file.put(b.raw_byte(), s.kitty_scratch);
    }

    /// The `n` data bytes at `from`, at once (`Paint::str_run`).
    pub(crate) fn file_run(&mut self, from: BytePos, n: ByteLen) {
        let run = &self.arena[from.index()..from.index() + n.index()];
        let s = &mut *self.strs;
        s.iterm_file.put_run(run, s.kitty_scratch);
    }

    /// **The terminator of a streamed body**: a whole file ends and is
    /// shown; after a part the file waits for more, its base64 one stream
    /// across the parts.
    #[cold]
    #[inline(never)]
    pub(crate) fn file_end(&mut self) {
        if self.strs.iterm_file.parts() == Some(FileParts::Whole) {
            self.file_finish(ItermKey::File);
        }
    }

    /// `File=`, `MultipartFile`, `FilePart` and `FileEnd` at the
    /// dispatcher: a whole `File=` an embedder parsed without the core, a
    /// file in parts begun or ended, or a part with none open, counted.
    pub(crate) fn file_message(&mut self, o: Osc1337<'_>) {
        match o {
            Osc1337::File {
                args,
                data: Some(data),
            } => {
                self.file_begin(FileArgs::parse(args), FileParts::Whole);
                let s = &mut *self.strs;
                s.iterm_file.put_run(data, s.kitty_scratch);
                self.file_finish(ItermKey::File);
            }
            // No `:`, so no data: not the page's form.
            Osc1337::File { data: None, .. } => self.not_acted(ItermKey::File),
            Osc1337::MultipartFile(args) => {
                self.file_begin(FileArgs::parse(args), FileParts::Multipart)
            }
            Osc1337::FilePart(data) => match self.strs.iterm_file.parts() {
                Some(FileParts::Multipart) => {
                    let s = &mut *self.strs;
                    s.iterm_file.put_run(data, s.kitty_scratch);
                }
                _ => self.not_acted(ItermKey::FilePart),
            },
            Osc1337::FileEnd => match self.strs.iterm_file.parts() {
                Some(FileParts::Multipart) => self.file_finish(ItermKey::MultipartFile),
                _ => self.not_acted(ItermKey::FileEnd),
            },
            _ => {}
        }
    }

    /// **`RIS`**: a file still arriving is abandoned and its scratch given
    /// back, uncounted, as `RIS` drops the kitty transmission in flight.
    pub(crate) fn file_reset(&mut self) {
        if let Some(used) = self.strs.iterm_file.abandon() {
            self.give_back(used);
        }
    }

    /// `APC G` began: a file still arriving gives the scratch up, counted.
    pub(crate) fn file_yield(&mut self) {
        if self.strs.iterm_file.holds_scratch() {
            let key = match self.strs.iterm_file.parts() {
                Some(FileParts::Multipart) => ItermKey::MultipartFile,
                _ => ItermKey::File,
            };
            if let Some(used) = self.strs.iterm_file.abandon() {
                self.give_back(used);
            }
            self.not_acted(key);
        }
    }

    /// **The scratch a file wrote goes back**, its whole pages, as the
    /// kitty stream gives back what its PNGs wrote: on every way a file
    /// ends, shown, refused, cut or abandoned.
    fn give_back(&mut self, used: FileLen) {
        let s = &mut *self.strs;
        let n = used.index().min(s.kitty_scratch.len());
        self.images.release_span(&mut s.kitty_scratch[..n]);
    }

    /// **The file is whole**: a download refused and counted; an inline
    /// picture decoded, placed and the cursor moved, an animated GIF
    /// playing; anything that is no picture baud decodes -- a format other
    /// than PNG, JPEG and GIF, data that is not base64, a file that does
    /// not decode -- drawn as the broken-image
    /// placeholder at the size asked (Jev, BROKEN_PLACEHOLDER, 0.94/0.98)
    /// and counted. More data than the scratch holds is refused whole.
    fn file_finish(&mut self, key: ItermKey) {
        let s = &mut *self.strs;
        let Some(end) = s.iterm_file.end(s.kitty_scratch) else {
            return;
        };
        self.give_back(end.len);
        let args = end.args;
        let len = match end.outcome {
            FileOutcome::Whole => Some(end.len),
            FileOutcome::NotBase64 => None,
            FileOutcome::Overflow => return self.not_acted(key),
        };
        // "Otherwise, it will be downloaded": into the user's folders,
        // which baud does not write for a program.
        if args.shown == FileShown::Download {
            return self.not_acted(key);
        }
        // No picture on the status line, as a Sixel has none there.
        if self.on_status() {
            return self.not_acted(key);
        }
        let s = &mut *self.strs;
        let picture = match len {
            Some(n) => {
                let (picture, used) = decode_picture(self.images, s.kitty_scratch, n);
                self.give_back(used);
                picture
            }
            None => None,
        };
        if picture.is_none() {
            self.not_acted(key);
        }
        if let Some(p) = picture {
            let skipped = Messages::of(usize::try_from(p.parts_not_drawn).unwrap_or(usize::MAX));
            let c = &mut self.strs.iterm.counts;
            c.picture_parts_skipped = c.picture_parts_skipped.plus(skipped);
        }
        self.place_picture(args, picture, key);
    }

    /// **Place a picture, or the placeholder, at the cursor**, as iTerm2's
    /// `writeBaseCharacter` writes its image characters: each row's cells
    /// from the cursor's column, a line feed before each row after the
    /// first, scrolling at the bottom, the cells' text erased as an image
    /// character replaces it (Jev's lean, ERASED); then the cursor right of
    /// the picture on its last row, at the pending wrap past the screen's
    /// edge, as iTerm2 sets its column to the screen's width there.
    fn place_picture(&mut self, args: FileArgs, picture: Option<Picture>, key: ItermKey) {
        let (iw, ih) = picture.map_or((BROKEN_W, BROKEN_H), |p| (p.width, p.height));
        let at = self.display_col(self.cx.col());
        let screen = FileScreen {
            cols: self.cols,
            rows: self.rows,
            cursor: at,
            cell_w: self.cell_width,
            cell_h: self.cell_height,
        };
        // No cell size to size it by, or past what an image side holds.
        let sized = file_geometry(args, iw, ih, screen).and_then(|g| Some((g, g.shown()?)));
        let Some((g, (w, h))) = sized else {
            if let Some(p) = picture {
                self.images.free(p.id);
            }
            return self.not_acted(key);
        };
        let id = match picture {
            Some(p) if g.draws_as_is(p) => Some(p.id),
            Some(p) => {
                let r = self.images.store_resampled(p.id, w, h, g.draw_w);
                self.images.free(p.id);
                r
            }
            None => {
                let ink = match self.cold.shown(DynamicColor::Foreground).as_rgb() {
                    Some((r, gr, b)) => {
                        Pixel::opaque(Chan::from_u8(r), Chan::from_u8(gr), Chan::from_u8(b))
                    }
                    None => Pixel::CLEAR,
                };
                self.images
                    .store(w, h, ImageAlpha::OpaqueOrClear)
                    .map(|(id, px)| {
                        draw_placeholder(px, &g, ink);
                        id
                    })
            }
        };
        let (screen_id, line) = self.image_anchor();
        // The cells: erased row by row, a line feed between rows.
        let end = Col::at_index((at.index() + g.cols.index()).min(self.cols.index()));
        for y in 0..g.rows.index() {
            if y > 0 {
                self.index();
            }
            let r = self.cy;
            let n = self.view.fill(r, at, end, PackedCell::EMPTY);
            self.erase_images(r, at, end);
            self.cells = self.cells.plus(n);
            self.row_changed_to(r);
            self.wrote_rows(r, r);
        }
        if let Some(id) = id {
            self.images.set_iterm(id);
            let alpha = self
                .images
                .image(id)
                .map_or(ImageAlpha::OpaqueOrClear, |v| ImageAlpha::of(v.pixels));
            self.images.complete(id, alpha);
            let _ = self.images.place(id, screen_id, line, at, ZIndex::ZERO);
            self.rehook();
        }
        let right = at.index() + g.cols.index();
        self.cx = match right > self.line_fence().index() {
            true => CursorCol::written(self.line_fence().after()),
            false => CursorCol::moved(Col::at_index(right)),
        };
    }
}
