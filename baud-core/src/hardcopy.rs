//! **ReGIS hard copy**: `S(H)` prints the visible image, or a rectangle of
//! it, as a sixel dump (`[[source: dec-std-070]]` 070-8 § 8.8.1.4.4: "The
//! hard-copy option retransmits the visible image on the display surface
//! ... Sixels are the implementation choice in video terminals"; 070-7
//! § 7.8.1, "Graphics Print Screen" and "Graphics Print Region", invoked
//! "by the host through the ReGIS hardcopy command").
//!
//! **The visible image is text and pictures**: the VT340's one bit map
//! holds both, and its own dump shows the shell's prompt beside a Sixel
//! (`[[source: vt340test-background-select-2-png]]`). Each cell's
//! background, its character drawn by the embedder's face at the cell's
//! size -- the face ReGIS's built-in alphabet uses
//! (`Term::set_regis_font`), a pixel half covered or more set -- in its
//! foreground, an underline along its last pixel row, and the pictures
//! in their drawing order. Reverse video swaps a cell's colours, an
//! invisible character is not drawn, and a placeholder cell, U+10EEEE,
//! draws no character.
//!
//! **The pictures take the layers the screen draws them in**: those of the
//! terminal graphics protocol (APC G) with a negative `z` under the text,
//! "Negative z-index values below INT32_MIN/2 (-1,073,741,824) will be
//! drawn under cells with non-default background colors"
//! (`[[source: kitty-graphics]]`, "Controlling displayed image layout"),
//! and the rest over it, the Sixel images and the ReGIS page among them.
//! Each pixel is laid over what is under it by its alpha, in encoded sRGB,
//! as the screen mixes it. A placement shows at its anchor, a relative one
//! where its chain of parents puts it, and a virtual one where its
//! placeholder cells are, a strip a run of them.
//!
//! **A print screen stays text**: 070-7 § 7.6.2 says its contents "may be
//! transmitted as sixels", a guideline, and a picture is printed by `S(H)`.
//!
//! **The dump is built in the print area**: the job writes from its front,
//! and the characters' pixels, a bit each, and one band of the dump are
//! kept at its end while it is written. A hard copy the area has no room
//! for prints nothing and is counted.

use crate::paint::Paint;
use crate::print::{JobKind, Status};
use baud_domain::*;

/// The most pictures a dump draws, a strip of placeholders each; past it
/// the ones found later are left out.
const PICTURES: usize = 256;

/// The most virtual placements whose placeholders a dump finds, to place
/// the relative placements under them; past it those under the later
/// ones are left out.
const ROOTS: usize = 32;

/// **Which layer a picture is drawn in against the text**: below, over
/// the default backgrounds and under every other; under, over the
/// backgrounds and under the characters; over, after the text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layer {
    Below,
    Under,
    Over,
}

impl Layer {
    /// `INT32_MIN/2`: a `z` strictly below it is drawn under the cells'
    /// non-default backgrounds.
    const BELOW_BACKGROUNDS: i32 = i32::MIN / 2;

    /// The layer a draw order puts a picture in.
    fn of(z: ZIndex) -> Layer {
        match z.raw() {
            z if z < Layer::BELOW_BACKGROUNDS => Layer::Below,
            z if z < 0 => Layer::Under,
            _ => Layer::Over,
        }
    }
}

/// **Where a picture comes in the drawing order**: by `z`, then by the
/// client's id, then by when the image was stored, then by when it was
/// placed, the order the screen draws them in.
type Order = (ZIndex, KittyId, ImageAge, PlaceSerial);

/// A picture on the page: which image, its layer and order, where its
/// drawn rectangle starts in page pixels, how it maps to the image, and
/// which of its drawn columns and rows show.
#[derive(Clone, Copy)]
struct Picture {
    image: ImageId,
    owner: ImageOwner,
    layer: Layer,
    order: Order,
    x0: i64,
    y0: i64,
    draw: ImageDraw,
    cols: (i64, i64),
    rows: (i64, i64),
}

/// **`top` laid over `under` by `top`'s alpha**, each channel
/// `(t a + u (255 - a) + 127) / 255` in encoded sRGB, the screen's mix;
/// opaque, as `under` is.
fn over(top: Pixel, under: Pixel) -> Pixel {
    let a = u32::from(top.alpha().get());
    let ((tr, tg, tb), (ur, ug, ub)) = (top.rgb(), under.rgb());
    let mix = |t: Chan, u: Chan| {
        let v = (u32::from(t.get()) * a + u32::from(u.get()) * (255 - a) + 127) / 255;
        Chan::from_u8(u8::try_from(v).unwrap_or(u8::MAX))
    };
    Pixel::opaque(mix(tr, ur), mix(tg, ug), mix(tb, ub))
}

/// A pixel of the page, which may lie off it.
#[derive(Clone, Copy)]
struct PagePx {
    x: i64,
    y: i64,
}

/// A region of the page in pixels: from its upper left `(x0, y0)` up to,
/// not including, `(x1, y1)`.
#[derive(Clone, Copy)]
struct PageRegion {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

/// A cell's size in pixels, zero while none is known.
#[derive(Clone, Copy)]
struct CellPx {
    w: i64,
    h: i64,
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
    cell: CellPx,
    pictures: [Option<Picture>; PICTURES],
    fg: Pixel,
    bg: Pixel,
    /// The ReGIS output map: index 0's colour is what a page left behind
    /// is read by, and a blinking pixel prints its index's colour.
    map: [Pixel; REGIS_COLORS],
    /// How the page being drawn shows its plane, `S(S)` and the window
    /// `S(D0)` moves.
    zoom: RegisZoom,
    /// The page showing the second surface and its plane, under `S(P1)`.
    second: Option<(ImageId, ImageId)>,
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
        let CellPx { w: cw, h: ch } = self.cell;
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

    /// **The pictures of `layer` at `at`, laid over `under` in drawing
    /// order**: a pixel with ink over what is under it by its alpha, the
    /// default background, `None`, read as its colour; a clear one, index
    /// 0 or a Sixel's fill, making it the default background again.
    fn layer(&self, at: PagePx, layer: Layer, mut under: Option<Pixel>) -> Option<Pixel> {
        for p in self.pictures.iter().flatten() {
            if p.layer != layer {
                continue;
            }
            match self.picture_at(p, at) {
                Hit::Miss => {}
                Hit::Clear => under = None,
                Hit::Ink(px) => under = Some(over(px, under.unwrap_or(self.bg))),
            }
        }
        under
    }

    /// **Picture `p`'s pixel at `at`**. A Sixel's is ink where it is not
    /// clear, and a picture of the terminal graphics protocol's where its
    /// alpha is above zero. A ReGIS page's is read from its plane: a pixel
    /// never written misses, and one written with index 0 is clear, "no
    /// bits set in any plane" (070-7 § 7.8.2.1), and covers the text, as
    /// the page shows its colour over it. A page left behind keeps no
    /// plane, so a pixel of it showing index 0's colour is taken for index
    /// 0. Magnified, a page pixel reads the plane pixel `S(S)` shows there.
    fn picture_at(&self, p: &Picture, at: PagePx) -> Hit {
        let PagePx { x: gx, y: gy } = at;
        let imgs = &*self.paint.images;
        let (dx, dy) = (gx - p.x0, gy - p.y0);
        let d = p.draw.dest;
        if dx < p.cols.0 || dx >= p.cols.1 || dy < p.rows.0 || dy >= p.rows.1 {
            return Hit::Miss;
        }
        let Some(view) = imgs.image(p.image) else {
            return Hit::Miss;
        };
        let at = |v: i64| u32::try_from(v).unwrap_or(0);
        let sx = ImageDraw::nearest(at(dx), d.w, p.draw.src.x, p.draw.src.w);
        let sy = ImageDraw::nearest(at(dy), d.h, p.draw.src.y, p.draw.src.h);
        let i = usize::try_from(sy)
            .unwrap_or(usize::MAX)
            .saturating_mul(view.width.index())
            .saturating_add(usize::try_from(sx).unwrap_or(usize::MAX));
        let Some(&px) = view.pixels.get(i) else {
            return Hit::Miss;
        };
        // A Sixel's fill is "set to the current background color"
        // (`[[source: vt3xx-sixel]]`): the background, over the text;
        // and so is a shared Sixel's pixel drawn in a clear ink there,
        // which draws as the fill (`Pixel::UNSAID`).
        if (px == Pixel::FILL || px == Pixel::UNSAID) && view.ground != ImageGround::Clear {
            return Hit::Clear;
        }
        if p.owner == ImageOwner::Regis {
            let at = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
            let (w, h) = (view.width.index(), view.height.index());
            // Where the window shows no pixel of the plane, none is held.
            let xy = self.zoom.plane_of(at(sx), at(sy), w, h);
            let plane = match self.second {
                Some((page, plane)) if page == p.image => imgs.regis_bytes(plane),
                _ => imgs.page_indices(p.image),
            };
            let index =
                |plane| xy.and_then(|(x, y)| RegisCanvas::index_at(plane, view.width, x, y));
            // A blinking pixel prints its own colour in either phase.
            let blinks = plane
                .zip(xy)
                .is_some_and(|(pl, (x, y))| RegisCanvas::blinks_at(pl, view.width, x, y));
            match plane.map(index) {
                Some(None) => return Hit::Miss,
                Some(Some(RegisIndex::ZERO)) => return Hit::Clear,
                Some(Some(i)) if blinks => {
                    let (r, g, b) = self.map[i.slot()].rgb();
                    return Hit::Ink(Pixel::opaque(r, g, b));
                }
                Some(Some(_)) => {}
                None if px.alpha().get() != 0 && px == self.map[RegisIndex::ZERO.slot()] => {
                    return Hit::Clear;
                }
                None => {}
            }
        }
        match px.alpha().get() {
            0 => Hit::Miss,
            _ => Hit::Ink(px),
        }
    }
}

impl DumpCanvas for Visible<'_, '_, '_> {
    fn at(&self, x: ImagePx, y: ImagePx) -> Option<Pixel> {
        let at = PagePx {
            x: self.x0 + i64::from(x.raw()),
            y: self.y0 + i64::from(y.raw()),
        };
        let cell = self.cell_at(at);
        let (fg, bg) = cell.map_or((None, None), |(r, c)| self.colours(r, c));
        let bit = usize::from(y.raw())
            .saturating_mul(usize::try_from(self.w).unwrap_or(0))
            .saturating_add(usize::from(x.raw()));
        let glyph = cell.is_some()
            && self
                .bits
                .get(bit / 8)
                .is_some_and(|b| b & (1 << (bit % 8)) != 0);
        // The default background, the pictures below the backgrounds, a
        // cell's own background over them, the pictures under the text,
        // the character, and the pictures over it.
        let below = self.layer(at, Layer::Below, None);
        let under = self.layer(at, Layer::Under, bg.or(below));
        let text = match glyph {
            true => fg,
            false => under,
        };
        self.layer(at, Layer::Over, text)
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
        // A Sixel prints in the colour a shared body earlier in this read
        // gave its register: what the read's end owes it is paid first
        // (`SixelStream::recolour`).
        self.strs.sixel.recolour(self.images);
        let (pw, ph) = self.regis_page_px();
        let (pw, ph) = (i64::from(pw.raw()), i64::from(ph.raw()));
        let (x0, y0, x1, y1) = match area {
            RegisHardCopy::Screen => (0, 0, pw, ph),
            // The positions name plane pixels; magnified or windowed, the
            // visible image shows each as a block of page pixels where the
            // window puts it.
            RegisHardCopy::Rect(a, b) => {
                let r = self.strs.regis.zoom().shown(RegisRect {
                    x0: a.x.min(b.x),
                    y0: a.y.min(b.y),
                    x1: a.x.max(b.x).saturating_add(1),
                    y1: a.y.max(b.y).saturating_add(1),
                });
                (
                    i64::from(r.x0).clamp(0, pw),
                    i64::from(r.y0).clamp(0, ph),
                    i64::from(r.x1).clamp(0, pw),
                    i64::from(r.y1).clamp(0, ph),
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
        self.begin_job(JobKind::Other);
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
            let cell = CellPx {
                w: i64::from(self.cell_width.raw()),
                h: i64::from(self.cell_height.raw()),
            };
            self.raster_text(PageRegion { x0, y0, x1, y1 }, cell, bits);
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
                map: *self.strs.regis.colors(),
                zoom: self.strs.regis.zoom(),
                second: self.regis_second_shown(),
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
    /// along each underlined cell's last row; no character for a
    /// placeholder cell.
    fn raster_text(&self, region: PageRegion, cell: CellPx, bits: &mut [u8]) {
        let PageRegion { x0, y0, x1, y1 } = region;
        let CellPx { w: cw, h: ch } = cell;
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
                // A placeholder, U+10EEEE, draws no character: its cell
                // shows a strip of a picture or its background.
                let cp = base
                    .as_codepoint()
                    .filter(|c| !is_placeholder(*c))
                    .map(|c| c.value());
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

    /// **The pictures on the live screen**, in drawing order ([`Order`]):
    /// each placement where it is drawn from its anchor cell
    /// (`KittyPlace::draw`), a relative one hanging off the edge by what
    /// its anchor was clamped by, with the drawn rows a margin scroll left
    /// shown; and, while a virtual placement is live, a strip for each run
    /// of placeholder cells, drawn as a placement anchored where the
    /// virtual placement's top-left cell would be and shown over the run's
    /// cells alone, and the relative placements under each virtual one, at
    /// the least row and column of its runs plus the offsets down the chain.
    fn pictures(&self, cell: CellPx) -> [Option<Picture>; PICTURES] {
        let CellPx { w: cw, h: ch } = cell;
        let imgs = &*self.images;
        let (screen, top) = self.image_top();
        let size = |v: i64| ImagePx::of(u16::try_from(v).unwrap_or(0));
        let n64 = |v: usize| i64::try_from(v).unwrap_or(0);
        let order = |image: ImageId, z: ZIndex, serial: PlaceSerial| -> Order {
            (
                z,
                imgs.kitty_names(image).map_or(KittyId::NONE, |(k, _)| k),
                imgs.born(image).unwrap_or(ImageAge::ZERO),
                serial,
            )
        };
        let mut out = [None; PICTURES];
        let mut n = 0;
        let mut push = |p: Picture| {
            if let Some(slot) = out.get_mut(n) {
                *slot = Some(p);
                n += 1;
            }
        };
        for (pid, p) in imgs.placements() {
            if p.screen != screen {
                continue;
            }
            // Under a virtual parent: placed below, from its placeholders.
            if p.kitty.parent.is_relative()
                && !matches!(imgs.kitty_root(pid), KittyRoot::Screen { .. })
            {
                continue;
            }
            let (Some(owner), Some(view)) = (imgs.owner(p.image), imgs.image(p.image)) else {
                continue;
            };
            let draw = p.kitty.draw(view.width, view.height, size(cw), size(ch));
            let d = draw.dest;
            if d.is_empty() {
                continue;
            }
            let rows_px = |r: Rows| n64(r.index()) * ch;
            let (cut_top, cut_bottom) = (rows_px(p.kitty.cut_top), rows_px(p.kitty.cut_bottom));
            let line = i64::try_from(p.line.value()).unwrap_or(0)
                - i64::try_from(top.value()).unwrap_or(0)
                - n64(p.kitty.over_rows.index());
            let col = n64(p.col.index()) - n64(p.kitty.over_cols.index());
            let x0 = col * cw + i64::from(d.x);
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
            push(Picture {
                image: p.image,
                owner,
                layer: Layer::of(p.z),
                order: order(p.image, p.z, p.serial),
                x0,
                y0,
                draw,
                cols: (0, i64::from(d.w)),
                rows,
            });
        }
        if self.strs.kitty.has_virtuals() {
            let at = KittyAt {
                screen,
                top,
                row: Rows::ZERO,
                col: Col::ZERO,
                span: Cols::of_index(1),
                rows: self.rows,
                cols: self.cols,
                cell_w: self.cell_width,
                cell_h: self.cell_height,
            };
            // Each virtual placement shown: the least row and column of
            // its runs.
            let mut roots: [Option<(PlaceSerial, i64, i64)>; ROOTS] = [None; ROOTS];
            let (vw, clusters, live) = (&self.view, &*self.clusters, self.tmpl.live());
            for r in 0..self.rows.index() {
                let row = Row::from_index(r);
                let rendition = vw.rendition(row);
                // A line not single-width draws its cells twice as wide, and
                // a double-height one twice as tall.
                let (ux, uy): (u32, u32) = match rendition {
                    LineRendition::Single => (1, 1),
                    LineRendition::DoubleWidth => (2, 1),
                    LineRendition::DoubleTop | LineRendition::DoubleBottom => (2, 2),
                };
                let (sx, sy) = (i64::from(ux), i64::from(uy));
                let ri = n64(r);
                let cells = (0..self.cols.index()).map(|c| {
                    crate::kitty::placeholder_cell(
                        clusters,
                        RowStyles::Live(live),
                        vw.get(row, Col::at_index(c)),
                    )
                });
                for run in PlaceholderRuns::new(cells) {
                    let Some(v) = self
                        .strs
                        .kitty
                        .virtual_of(run.image, run.placement, &at, imgs)
                    else {
                        continue;
                    };
                    let col = n64(run.at.index()) * sx;
                    match roots.iter_mut().flatten().find(|e| e.0 == v.serial) {
                        Some(e) => (e.1, e.2) = (e.1.min(ri), e.2.min(col)),
                        None => {
                            if let Some(slot) = roots.iter_mut().find(|e| e.is_none()) {
                                *slot = Some((v.serial, ri, col));
                            }
                        }
                    }
                    let (Some(owner), Some(view)) = (imgs.owner(v.image), imgs.image(v.image))
                    else {
                        continue;
                    };
                    let mut draw = v.place.draw(view.width, view.height, size(cw), size(ch));
                    draw.dest = PxRect {
                        x: draw.dest.x.saturating_mul(ux),
                        y: draw.dest.y.saturating_mul(uy),
                        w: draw.dest.w.saturating_mul(ux),
                        h: draw.dest.h.saturating_mul(uy),
                    };
                    let d = draw.dest;
                    if d.is_empty() {
                        continue;
                    }
                    // The top of the boxes on this row: a row above it for
                    // the bottom half of a double-height line.
                    let box_top = match rendition {
                        LineRendition::DoubleBottom => (ri - 1) * ch,
                        _ => ri * ch,
                    };
                    let x0 =
                        (n64(run.at.index()) - n64(run.col.index())) * cw * sx + i64::from(d.x);
                    let y0 = box_top - n64(run.row.index()) * ch * sy + i64::from(d.y);
                    let left = n64(run.at.index()) * cw * sx;
                    let right = left + n64(run.cells.index()) * cw * sx;
                    let cols = ((left - x0).max(0), (right - x0).min(i64::from(d.w)));
                    let rows = (
                        (ri * ch - y0).max(0),
                        (ri * ch + ch - y0).min(i64::from(d.h)),
                    );
                    if cols.0 >= cols.1 || rows.0 >= rows.1 {
                        continue;
                    }
                    // Never below its own cells' backgrounds: "The
                    // background color is interpreted as the background
                    // color, visible if the image is transparent"
                    // (`[[source: kitty-graphics]]`, "Unicode placeholders").
                    let z = v.z.max(ZIndex::of(Layer::BELOW_BACKGROUNDS));
                    push(Picture {
                        image: v.image,
                        owner,
                        layer: Layer::of(z),
                        order: order(v.image, z, v.serial),
                        x0,
                        y0,
                        draw,
                        cols,
                        rows,
                    });
                }
            }
            for (pid, p) in imgs.placements() {
                if p.screen != screen || !p.kitty.parent.is_relative() {
                    continue;
                }
                let KittyRoot::Virtual { serial, h, v } = imgs.kitty_root(pid) else {
                    continue;
                };
                let Some((r, c)) = roots
                    .iter()
                    .flatten()
                    .find(|e| e.0 == serial)
                    .map(|e| (e.1, e.2))
                else {
                    continue;
                };
                let (Some(owner), Some(view)) = (imgs.owner(p.image), imgs.image(p.image)) else {
                    continue;
                };
                let draw = p.kitty.draw(view.width, view.height, size(cw), size(ch));
                let d = draw.dest;
                if d.is_empty() {
                    continue;
                }
                push(Picture {
                    image: p.image,
                    owner,
                    layer: Layer::of(p.z),
                    order: order(p.image, p.z, p.serial),
                    x0: (c + h) * cw + i64::from(d.x),
                    y0: (r + v) * ch + i64::from(d.y),
                    draw,
                    cols: (0, i64::from(d.w)),
                    rows: (0, i64::from(d.h)),
                });
            }
        }
        out[..n].sort_unstable_by_key(|p| p.map(|p| p.order));
        out
    }
}
