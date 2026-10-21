//! **ReGIS hard copy**: `S(H)` prints the visible image, or a rectangle of
//! it, as a sixel dump (`[[source: dec-std-070]]` 070-8 § 8.8.1.4.4: "The
//! hard-copy option retransmits the visible image on the display surface
//! ... Sixels are the implementation choice in video terminals"; 070-7
//! § 7.8.1, "Graphics Print Screen" and "Graphics Print Region", invoked
//! "by the host through the ReGIS hardcopy command").
//!
//! **The visible image is text and pictures** (Jev, VISIBLE_IMAGE,
//! 0.98/0.98): the VT340's one bit map holds both, and its own dump shows
//! the shell's prompt beside a Sixel (`[[source:
//! vt340test-background-select-2-png]]`). Each cell's background, its
//! character drawn by the embedder's face at the cell's size -- the face
//! ReGIS's built-in alphabet uses (`Term::set_regis_font`), a pixel half
//! covered or more set -- in its foreground, an underline along its last
//! pixel row, and over them the Sixel images and the ReGIS page, in their
//! drawing order. Reverse video swaps a cell's colours, and an invisible
//! character is not drawn. kitty's images are not printed.
//!
//! **A print screen stays text** (Jev's lean, TEXT_ONLY, 0.35/0.28 after a
//! re-ask): 070-7 § 7.6.2 says its contents "may be transmitted as sixels",
//! a guideline, and a picture is printed by `S(H)`.
//!
//! **The dump is built in the print area**: the job writes from its front,
//! and the characters' pixels, a bit each, and one band of the dump are
//! kept at its end while it is written. A hard copy the area has no room
//! for prints nothing and is counted.

use crate::paint::Paint;
use crate::print::Status;
use baud_domain::*;

/// The most pictures a dump draws; past it the later ones in drawing order
/// are left out.
const PICTURES: usize = 64;

/// A picture on the page: which image, where its drawn rectangle starts in
/// page pixels, how it maps to the image, and which of its drawn rows show.
#[derive(Clone, Copy)]
struct Picture {
    image: ImageId,
    owner: ImageOwner,
    z: ZIndex,
    x0: i64,
    y0: i64,
    draw: ImageDraw,
    rows: (i64, i64),
}

/// A pixel of the page, which may lie off it.
#[derive(Clone, Copy)]
struct PagePx {
    x: i64,
    y: i64,
}

/// The picture a dump draws: the region's pixels, read from the screen.
struct Visible<'p, 'a, 'b> {
    paint: &'p Paint<'a, 'b>,
    /// The region's upper left in page pixels, and its width.
    x0: i64,
    y0: i64,
    w: i64,
    /// The characters' pixels in the region, a bit each, row by row.
    bits: &'p [u8],
    /// The cell in pixels, zero while none is known.
    cell: (i64, i64),
    pictures: [Option<Picture>; PICTURES],
    fg: Pixel,
    bg: Pixel,
    /// What the ReGIS output map shows index 0 as.
    regis0: Pixel,
    /// How the page being drawn shows its plane, `S(S)`.
    zoom: RegisZoom,
}

/// What a picture holds at a pixel of the page.
enum Hit {
    /// Nothing: what lies under it shows.
    Miss,
    /// Index 0, no bit set in any plane, covering what lies under it.
    Clear,
    Ink(Pixel),
}

/// A colour word's pixel: its own, the palette's, or `default`.
fn resolve(paint: &Paint<'_, '_>, w: ColorWord, default: Pixel) -> Pixel {
    let px = |(r, g, b): (u8, u8, u8)| {
        Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
    };
    if let Some(c) = w.as_rgb() {
        return px(c);
    }
    match w.as_index() {
        Some(i) => {
            let pal = &paint.strs.palette;
            let at = |n: usize| pal.get(n).and_then(|c| c.as_rgb());
            at(usize::from(i))
                .or_else(|| at(crate::storage::EMBEDDER_PALETTE + usize::from(i)))
                .map_or(default, px)
        }
        None => default,
    }
}

impl Visible<'_, '_, '_> {
    /// The cell at page pixel `(gx, gy)`: its row, its column on that row,
    /// and the column's width in pixels.
    fn cell_at(&self, at: PagePx) -> Option<(Row, Col)> {
        let PagePx { x: gx, y: gy } = at;
        let (cw, ch) = self.cell;
        if cw == 0 || ch == 0 || gx < 0 || gy < 0 {
            return None;
        }
        let r = usize::try_from(gy / ch).ok()?;
        if r >= self.paint.rows.index() {
            return None;
        }
        let row = Row::from_index(r);
        let size = self.paint.view.rendition(row);
        let wide = match size {
            LineRendition::Single => 1,
            _ => 2,
        };
        let c = usize::try_from(gx / (cw * wide)).ok()?;
        if c >= size.end_of_line(self.paint.cols).index() {
            return None;
        }
        Some((row, Col::at_index(c)))
    }

    /// A cell's foreground and background, `None` for the default
    /// background, index 0, which no plane has a bit of: reverse video
    /// swaps the two, so a reversed character on the default background is
    /// index 0 in a cell of its foreground.
    fn colours(&self, r: Row, c: Col) -> (Option<Pixel>, Option<Pixel>) {
        let cell = self.paint.view.get(r, c);
        let word = self.paint.word_of(cell.style);
        let fg = Some(resolve(self.paint, word.fg(), self.fg));
        let bg = match word.bg().is_default() {
            true => None,
            false => Some(resolve(self.paint, word.bg(), self.bg)),
        };
        match word.attrs().has(Attrs::REVERSE) {
            true => (bg, fg),
            false => (fg, bg),
        }
    }

    /// **The topmost picture's pixel at `at`**. A Sixel's is ink where it
    /// is not clear. A ReGIS page's is read from its plane: a pixel never
    /// written misses, and one written with index 0 is clear, "no bits set
    /// in any plane" (070-7 § 7.8.2.1), and covers the text, as the page
    /// shows its colour over it. A page left behind keeps no plane, so a
    /// pixel of it showing index 0's colour is taken for index 0 (Jev's
    /// lean, COLOUR_MATCH, 0.23/0.53 after a re-ask). Magnified, a page
    /// pixel reads the plane pixel `S(S)` shows there.
    fn picture_at(&self, at: PagePx) -> Hit {
        let PagePx { x: gx, y: gy } = at;
        let imgs = &*self.paint.images;
        for p in self.pictures.iter().rev().flatten() {
            let (dx, dy) = (gx - p.x0, gy - p.y0);
            let d = p.draw.dest;
            if dx < 0 || dx >= i64::from(d.w) || dy < p.rows.0 || dy >= p.rows.1 {
                continue;
            }
            let Some(view) = imgs.image(p.image) else {
                continue;
            };
            let at = |v: i64| u32::try_from(v).unwrap_or(0);
            let sx = ImageDraw::nearest(at(dx), d.w, p.draw.src.x, p.draw.src.w);
            let sy = ImageDraw::nearest(at(dy), d.h, p.draw.src.y, p.draw.src.h);
            let i = usize::try_from(sy)
                .unwrap_or(usize::MAX)
                .saturating_mul(view.width.index())
                .saturating_add(usize::try_from(sx).unwrap_or(usize::MAX));
            let Some(&px) = view.pixels.get(i) else {
                continue;
            };
            if p.owner == ImageOwner::Regis {
                let index = |plane| {
                    let at = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
                    let (x, y) = self.zoom.plane_of(at(sx), at(sy));
                    RegisCanvas::index_at(plane, view.width, x, y)
                };
                match imgs.page_indices(p.image).map(index) {
                    Some(None) => continue,
                    Some(Some(RegisIndex::ZERO)) => return Hit::Clear,
                    Some(Some(_)) => {}
                    None if px.alpha().get() != 0 && px == self.regis0 => return Hit::Clear,
                    None => {}
                }
            }
            if px.alpha().get() != 0 {
                let (r, g, b) = px.rgb();
                return Hit::Ink(Pixel::opaque(r, g, b));
            }
        }
        Hit::Miss
    }
}

impl DumpCanvas for Visible<'_, '_, '_> {
    fn at(&self, x: ImagePx, y: ImagePx) -> Option<Pixel> {
        let at = PagePx {
            x: self.x0 + i64::from(x.raw()),
            y: self.y0 + i64::from(y.raw()),
        };
        match self.picture_at(at) {
            Hit::Ink(p) => return Some(p),
            Hit::Clear => return None,
            Hit::Miss => {}
        }
        let (r, c) = self.cell_at(at)?;
        let (fg, bg) = self.colours(r, c);
        let bit = usize::from(y.raw())
            .saturating_mul(usize::try_from(self.w).unwrap_or(0))
            .saturating_add(usize::from(x.raw()));
        match self
            .bits
            .get(bit / 8)
            .is_some_and(|b| b & (1 << (bit % 8)) != 0)
        {
            true => fg,
            false => bg,
        }
    }
}

impl Paint<'_, '_> {
    /// **`S(H)`**: the dump of `area` as a job of its own, its upper left at
    /// `offset` on the printer, a form feed after it under DECPFF, which
    /// "affect[s] both 'Print page' and 'Print Screen'" (070-7 § 7.9.3) and
    /// which the VT330/VT340 sends "following text or graphics operations".
    /// With no printer since power-up or RIS it is ignored and counted, as
    /// every print function is (070-7 § 7.3.2).
    #[cold]
    #[inline(never)]
    pub(crate) fn hard_copy(&mut self, area: RegisHardCopy, offset: RegisOffset) {
        if self.strs.print.status() == Status::NotAvailable {
            self.strs.print.refusals.no_printer += 1;
            return;
        }
        let (pw, ph) = self.regis_page_px();
        let (pw, ph) = (i64::from(pw.raw()), i64::from(ph.raw()));
        let (x0, y0, x1, y1) = match area {
            RegisHardCopy::Screen => (0, 0, pw, ph),
            // The positions name plane pixels; magnified, the visible
            // image shows each as a block of page pixels.
            RegisHardCopy::Rect(a, b) => {
                let z = self.strs.regis.zoom();
                let (zx, zy) = (i64::from(z.x.max(1)), i64::from(z.y.max(1)));
                let (ax, bx) = (i64::from(a.x.min(b.x)), i64::from(a.x.max(b.x)) + 1);
                let (ay, by) = (i64::from(a.y.min(b.y)), i64::from(a.y.max(b.y)) + 1);
                let (ax, bx, ay, by) = (ax * zx, bx * zx, ay * zy, by * zy);
                (
                    ax.clamp(0, pw),
                    ay.clamp(0, ph),
                    bx.clamp(0, pw),
                    by.clamp(0, ph),
                )
            }
        };
        let px = |v: i64| ImagePx::of(u16::try_from(v.max(0)).unwrap_or(u16::MAX));
        let print = self.strs.print;
        let shown = |which| self.cold.shown(which).as_rgb();
        let rgb = |(r, g, b): (u8, u8, u8)| {
            Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        };
        let bg = shown(DynamicColor::Background).map_or(rgb((255, 255, 255)), rgb);
        let fg = shown(DynamicColor::Foreground).map_or(rgb((0, 0, 0)), rgb);
        let spec = DumpSpec {
            width: px(x1 - x0),
            height: px(y1 - y0),
            offset: (ImagePx::of(offset.x), ImagePx::of(offset.y)),
            size: print.size,
            rotation: print.graphics.rotation,
            colour: print.graphics.colour,
            syntax: print.graphics.syntax,
            background: print.graphics.background,
            index0: bg,
        };
        self.end_run_job();
        self.begin_job();
        let w = spec.width.index();
        let bits_len = (w * spec.height.index()).div_ceil(8);
        let scratch = bits_len + dump_room(&spec);
        let area = core::mem::take(&mut self.strs.print_area);
        let at = self.strs.print.open_end().map_or(0, ReadLen::index);
        let mut wrote = (0, 0);
        let fits = area
            .len()
            .checked_sub(scratch)
            .is_some_and(|front| front >= at);
        if fits {
            let split = area.len() - scratch;
            let (front, tail) = area.split_at_mut(split);
            let (bits, room) = tail.split_at_mut(bits_len);
            bits.fill(0);
            let cell = (
                i64::from(self.cell_width.raw()),
                i64::from(self.cell_height.raw()),
            );
            self.raster_text((x0, y0, x1, y1), cell, bits);
            let visible = Visible {
                paint: self,
                x0,
                y0,
                w: x1 - x0,
                bits,
                cell,
                pictures: self.pictures(cell),
                fg,
                bg,
                regis0: self.strs.regis.colors()[RegisIndex::ZERO.slot()],
                zoom: self.strs.regis.zoom(),
            };
            let mut n = at;
            let mut lost = 0usize;
            write_dump(&spec, &visible, room, &mut |b| {
                let k = b.len().min(front.len().saturating_sub(n));
                front[n..n + k].copy_from_slice(&b[..k]);
                n += k;
                lost += b.len() - k;
            });
            wrote = (n - at, lost);
        }
        self.strs.print_area = area;
        match fits {
            true => self
                .strs
                .print
                .wrote(ReadLen::from_index(wrote.0), ReadLen::from_index(wrote.1)),
            false => self.strs.print.refusals.hard_copies_cut += 1,
        }
        if self.strs.print.form_feed == PrintFormFeed::On {
            self.put(b"\x0c");
        }
        self.finish_job();
        self.resume_run_job();
    }

    /// **The characters of the region `(x0, y0, x1, y1)`, a bit a pixel**:
    /// each cell's character drawn by the embedder's face at the cell's
    /// size, twice as wide for a wide character or a double-width line and
    /// twice as tall, its half shown, on a double-height one; an underline
    /// along each underlined cell's last row.
    fn raster_text(&self, region: (i64, i64, i64, i64), cell: (i64, i64), bits: &mut [u8]) {
        let (x0, y0, x1, y1) = region;
        let (cw, ch) = cell;
        if cw == 0 || ch == 0 {
            return;
        }
        let font = self.strs.regis.font();
        let w = x1 - x0;
        let mut set = |gx: i64, gy: i64| {
            if gx < x0 || gx >= x1 || gy < y0 || gy >= y1 {
                return;
            }
            let i = usize::try_from((gy - y0) * w + (gx - x0)).unwrap_or(usize::MAX);
            if let Some(b) = bits.get_mut(i / 8) {
                *b |= 1 << (i % 8);
            }
        };
        let first = usize::try_from(y0 / ch).unwrap_or(0);
        let last = usize::try_from((y1 + ch - 1) / ch)
            .unwrap_or(0)
            .min(self.rows.index());
        for r in first..last {
            let row = Row::from_index(r);
            let size = self.view.rendition(row);
            let (wide, tall, half) = match size {
                LineRendition::Single => (1, 1, 0),
                LineRendition::DoubleWidth => (2, 1, 0),
                LineRendition::DoubleTop => (2, 2, 0),
                LineRendition::DoubleBottom => (2, 2, 1),
            };
            let top = i64::try_from(r).unwrap_or(0) * ch;
            for c in 0..size.end_of_line(self.cols).index() {
                let left = i64::try_from(c).unwrap_or(0) * cw * wide;
                if left >= x1 || left + 2 * cw * wide <= x0 {
                    continue;
                }
                let cellv = self.view.get(row, Col::at_index(c));
                let attrs = self.word_of(cellv.style).attrs();
                let base = self.clusters.base_of(cellv.content);
                if base.is_wide_tail() {
                    continue;
                }
                let cp = base.as_codepoint().map(|c| c.value());
                let span = match cp.and_then(char::from_u32) {
                    Some(c)
                        if Width::of(Codepoint::from_char(c), AmbiguousWidth::Narrow).cells()
                            == 2 =>
                    {
                        2
                    }
                    _ => 1,
                };
                let gw = cw * wide * span;
                if attrs.only(Attrs::UNDERLINES) != Attrs::NONE {
                    for x in 0..gw {
                        set(left + x, top + ch - 1);
                    }
                }
                let (Some(f), Some(c)) = (font, cp.and_then(char::from_u32)) else {
                    continue;
                };
                if attrs.has(Attrs::HIDDEN) || c == ' ' {
                    continue;
                }
                let dim = |v: i64| ImagePx::of(u16::try_from(v).unwrap_or(u16::MAX));
                f(c, dim(gw), dim(ch * tall), &mut |px, py| {
                    let y = i64::from(py.raw()) - half * ch;
                    if (0..ch).contains(&y) {
                        set(left + i64::from(px.raw()), top + y);
                    }
                });
            }
        }
    }

    /// **The Sixel images and the ReGIS page on the live screen**, in
    /// drawing order: by `z`, then by image, as the renderer orders them;
    /// where each is drawn from its anchor cell (`KittyPlace::draw`) and
    /// which of its drawn rows a margin scroll left shown.
    fn pictures(&self, cell: (i64, i64)) -> [Option<Picture>; PICTURES] {
        let (cw, ch) = cell;
        let imgs = &*self.images;
        let (screen, top) = match self.cold.screen {
            crate::term::Screen::Normal => (ImageScreen::Primary, self.book.top),
            crate::term::Screen::Alternate => (ImageScreen::Alternate, self.book.alt_top),
        };
        let size = |v: i64| ImagePx::of(u16::try_from(v).unwrap_or(0));
        let mut out = [None; PICTURES];
        let mut n = 0;
        for (_, p) in imgs.placements() {
            if p.screen != screen || n == PICTURES {
                continue;
            }
            if !matches!(
                imgs.owner(p.image),
                Some(ImageOwner::Local | ImageOwner::Regis)
            ) {
                continue;
            }
            let Some(view) = imgs.image(p.image) else {
                continue;
            };
            let draw = p.kitty.draw(view.width, view.height, size(cw), size(ch));
            let d = draw.dest;
            if d.is_empty() {
                continue;
            }
            let rows_px = |r: Rows| i64::try_from(r.index()).unwrap_or(0) * ch;
            let (cut_top, cut_bottom) = (rows_px(p.kitty.cut_top), rows_px(p.kitty.cut_bottom));
            let line = i64::try_from(p.line.value()).unwrap_or(0)
                - i64::try_from(top.value()).unwrap_or(0);
            let x0 = i64::try_from(p.col.index()).unwrap_or(0) * cw + i64::from(d.x);
            let y0 = line * ch - cut_top + i64::from(d.y);
            let mut rows = (0, i64::from(d.h));
            if cut_top > 0 {
                rows.0 = rows.0.max(cut_top - i64::from(d.y));
            }
            if cut_bottom > 0 {
                rows.1 = rows
                    .1
                    .min(rows_px(p.kitty.rows) - cut_bottom - i64::from(d.y));
            }
            let Some(owner) = imgs.owner(p.image) else {
                continue;
            };
            out[n] = Some(Picture {
                image: p.image,
                owner,
                z: p.z,
                x0,
                y0,
                draw,
                rows,
            });
            n += 1;
        }
        out[..n].sort_unstable_by_key(|p| p.map(|p| (p.z, p.image)));
        out
    }
}
