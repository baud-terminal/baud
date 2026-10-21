//! ReGIS, `DCS Pn p ... ST`: the body handed to `baud_domain::Regis` a
//! character at a time, and what it draws and answers wired to the
//! terminal.
//!
//! **The graph's `DCS` row routes a bare `p` here** (`Act::DcsQuery`, whose
//! byte tells it from DECRQSS's `$ q` and DECRSPS's `$ t`), after only the
//! numeric parameters a Sixel's `q` takes; `DCS $ p` and `DCS + p` keep their
//! own rows. `[[source: dec-std-070]]` 070-8 § 8.9.1: "ReGIS commands shall
//! be introduced by a DEC-private parameterized form of the ANSI escape
//! sequences Device Control String (DCS) and terminated the ANSI escape
//! sequence String Terminator (ST) ... the <protocol selector> for ReGIS is
//! lower case "p"."
//!
//! **The interpreter outlives the envelope**: an `ST`, a `CAN` or an `ESC`
//! that cancels the string leaves ReGIS where it was, and the next `DCS p`
//! resumes there unless its parameter's low bit resynchronises
//! (`RegisEntry`).
//!
//! **What it draws goes on a page**: an image placed at the window's top
//! line and first column, covering the text area, drawn from a second image
//! of output-map indices (`RegisCanvas`) and shown through the map after
//! each character. The page scrolls with the lines it sits on, as a Sixel
//! does; once the window's top line is no longer the page's, the next
//! drawing begins a new page there, as the VT300's screen-relative
//! coordinates draw on the screen and its scrolled contents move away
//! beneath them. The lines of the old page still on the screen go to the
//! new page, with the indices they are drawn from, so a complement drawing
//! reads what is shown; the old page keeps its lines in the history.

use crate::paint::Paint;
use crate::term::{Screen, Term};
use crate::write::Csi;
use baud_domain::*;

/// The page ReGIS draws on: the image shown, the image of indices it is
/// drawn from, where it is anchored and how large it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Page {
    shown: ImageId,
    plane: ImageId,
    screen: ImageScreen,
    line: LineIdx,
    w: ImagePx,
    h: ImagePx,
}

impl Page {
    /// No page yet.
    pub(crate) const NONE: Page = Page {
        shown: ImageId::NONE,
        plane: ImageId::NONE,
        screen: ImageScreen::Primary,
        line: LineIdx::FIRST,
        w: ImagePx::ZERO,
        h: ImagePx::ZERO,
    };
}

/// Where the page for the next drawing must be, and how large: the live
/// screen's window top, and the text area in pixels, an 800 by 480 page
/// while no cell size is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Want {
    screen: ImageScreen,
    line: LineIdx,
    w: ImagePx,
    h: ImagePx,
    /// The cell height, zero while none is known: how many pixel rows of
    /// the page a line is.
    ch: ImagePx,
}

impl Paint<'_, '_> {
    /// The string a `DcsQuery` byte other than DECRQSS's `q` begins:
    /// DECRSPS's `t` after `$`, or ReGIS's bare `p`. Out of line, so the
    /// per-byte loop's arm is DECRQSS's test and one call, the shape it had
    /// with DECRSPS's alone.
    #[cold]
    #[inline(never)]
    pub(crate) fn dcs_start(&mut self, b: StreamByte, csi: &Csi) -> crate::osc::StrKind {
        match b.raw_byte() {
            b'p' => self.regis_start(csi),
            b'w' => self.locator_keys_start(csi),
            _ => self.restore_start(csi),
        }
    }

    /// A `DCS Pc $ w`, DECLKD: its clear parameter kept for the end, and
    /// its body kept as a DECRQSS body is, every control dropped
    /// (`Paint::str_put`).
    fn locator_keys_start(&mut self, csi: &Csi) -> crate::osc::StrKind {
        let pc = csi.params().raw(ParamIdx::FIRST, 0);
        self.strs.regis.locator_keys_mut().begin(pc);
        crate::osc::StrKind::LocatorKeys
    }

    /// A DECLKD's terminator: its definitions loaded.
    pub(crate) fn locator_keys_end(&mut self) {
        let n = self.cold.str_len.index().min(self.strs.pending.len());
        let strs = &mut *self.strs;
        strs.regis.locator_keys_mut().end(&strs.pending[..n]);
    }

    /// A `DCS Pn p`: enter ReGIS, `Pn` the envelope's first parameter.
    fn regis_start(&mut self, csi: &Csi) -> crate::osc::StrKind {
        // No ReGIS page on the status line, as `sixel_begin` has it.
        if self.on_status() {
            return crate::osc::StrKind::Other;
        }
        let e = RegisEntry::of(Some(csi.params().raw(ParamIdx::FIRST, 0)));
        // An envelope cancelled earlier in this read left its cursors
        // drawn: they go at the read's end, and before this one draws.
        self.regis_cursor_off();
        self.strs.regis.set_encoding(self.cold.str_enc);
        self.regis_run(|regis, out| regis.enter(e, out));
        crate::osc::StrKind::Regis
    }

    /// **One body byte, and the read held at a delay.** Under
    /// [`RegisHold::On`] an `S(T)` stops the read after the byte that ends
    /// it: the rest is held in `WidthAsk::held`, which `Term::take_held`
    /// hands the embedder as it hands a page-width request's, and the ticks
    /// in `regis_wait`, which `Term::take_regis_wait` hands it. The parser
    /// stays in the string's node, so the rest goes on inside the envelope.
    /// A delay inside a macrograph stops the macrograph where it is, and
    /// the next byte plays the rest of it first, the byte itself held if
    /// the macrograph stops again.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_byte(&mut self, from: BytePos, left: ReadLen) -> ReadLen {
        if self.strs.regis.held() {
            self.regis_run(|regis, out| regis.resume(out));
            if self.regis_hold(from, left) == Held::Yes {
                self.regis_input_shown();
                return left;
            }
        }
        self.regis_put(StreamByte::from_stream(self.arena[from.index()]));
        let rest = from.advance_read(ReadLen::ONE);
        match self.regis_hold(rest, left.less(ReadLen::ONE)) {
            Held::Yes => {
                self.regis_input_shown();
                left
            }
            Held::No => {
                // **The read ends inside the envelope**: ReGIS waits on the
                // host, which is when the output cursor shows ("only
                // appears when the terminal is in ReGIS mode and waiting for
                // ReGIS input from the host", `[[source:
                // vt3xx-regis-intro]]`), and multiple mode's input cursor
                // with it. Drawn once a read, here, and taken away by the
                // next byte's run.
                if left == ReadLen::ONE {
                    self.regis_cursor_on(Make::No);
                }
                ReadLen::ONE
            }
        }
    }

    /// The input cursor drawn while a one-shot input holds the read; a
    /// delay's hold shows nothing, ReGIS not waiting on the host then.
    fn regis_input_shown(&mut self) {
        if self.strs.regis.input(RegisEnvelope::Open) == RegisInput::OneShot {
            self.regis_cursor_on(Make::Yes);
        }
    }

    /// Take the cursors drawn away, from the plane they were drawn over.
    pub(crate) fn regis_cursor_off(&mut self) {
        if !self.strs.regis.is_drawn() {
            return;
        }
        erase(&mut *self.images, self.strs.regis);
    }

    /// Draw the cursors ReGIS shows now, inside its envelope: the input
    /// cursor while one-shot input waits, and otherwise the output cursor
    /// and multiple mode's input cursor. `make` says whether a page may be
    /// placed for them: at a read's end it may not, so where the reads fell
    /// never decides whether a page is placed.
    fn regis_cursor_on(&mut self, make: Make) {
        let want = self.regis_want();
        let marks = shown_marks(self.strs.regis, RegisEnvelope::Open);
        let strs = &mut *self.strs;
        draw(
            &mut *self.images,
            &mut strs.regis_page,
            strs.regis,
            want,
            (marks, make),
        );
    }

    /// **The terminator of an envelope whose macrograph a delay stopped**:
    /// the macrograph plays on first, and if it stops again the read is
    /// held from the terminator, the parser back in the node it was read
    /// from -- `StrEsc` for the `\` of `ESC \`, the body's for a C1 `ST`
    /// -- so the rest ends the envelope once the macrograph has played.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_end(
        &mut self,
        from: BytePos,
        left: ReadLen,
        node: &mut crate::graph::Node,
    ) -> ReadLen {
        self.regis_run(|regis, out| regis.resume(out));
        if self.regis_hold(from, left) == Held::Yes {
            self.regis_input_shown();
            *node = match self.arena[from.index()] {
                0x9c => crate::graph::Node::DcsStr,
                _ => crate::graph::Node::StrEsc,
            };
            return left;
        }
        self.str_end(StreamByte::from_stream(self.arena[from.index()]));
        ReadLen::ONE
    }

    /// Hold the read from `at` if the interpreter met a delay, or a
    /// one-shot `R(P(I))` waits: "While the terminal is in graphics input
    /// mode, no further data from the application (screen output) is
    /// processed until the locator input is complete" (070-13 § 5.1). The
    /// embedder reads the second from `Term::regis_input`, where the first
    /// has its ticks.
    fn regis_hold(&mut self, at: BytePos, len: ReadLen) -> Held {
        let wait = self.strs.regis.take_wait();
        let input = self.strs.regis.input(RegisEnvelope::Open) == RegisInput::OneShot;
        if wait.is_none() && !input {
            return Held::No;
        }
        if let Some(t) = wait {
            let w = self.strs.regis_wait.map_or(0, RegisTicks::get);
            self.strs.regis_wait = Some(RegisTicks::of(w.saturating_add(t.get())));
        }
        self.cold.width.held = Some(crate::term::Rest { at, len });
        Held::Yes
    }

    /// One body byte. **Under ISO 8859-1 a GR byte outside a quoted string
    /// is no ReGIS character**, the alphabet being "Columns 2 through 5 of
    /// the ASCII Code Table" (070-8 § 8.4.1), and the interpreter passes it
    /// over; inside one it is the 8-bit code § 8.4.3.2 allows, "10/0 through
    /// 15/15". The byte goes as it came.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_put(&mut self, b: StreamByte) {
        self.regis_run(|regis, out| regis.put(b, out));
    }

    /// Run the interpreter against the page, then show what it drew.
    fn regis_run(&mut self, f: impl FnOnce(&mut Regis, &mut dyn RegisOut)) {
        // The cursors go first: a drawing shows its pixels from the plane,
        // and a new page leaves the old one with whatever it shows.
        self.regis_cursor_off();
        let want = self.regis_want();
        let regis = &mut *self.strs.regis;
        regis.set_page(i32::from(want.w.raw()), i32::from(want.h.raw()));
        // How the page shows its plane, for a carry inside the run and for
        // text and erases after it (`ImageStore::set_page_zoom`).
        self.images.set_page_zoom(regis.zoom());
        let mut out = Out {
            reply: &mut *self.reply,
            reply_len: &mut self.reply_len,
            overflows: &mut self.reply_overflows,
            c1: self.cold.replies.c1(),
            images: &mut *self.images,
            page: &mut self.strs.regis_page,
            want,
            dirty: None,
            recolor: Recolor::No,
            unpaged: Unpaged::None,
        };
        let before = *out.page;
        let line = regis.display_gen();
        f(regis, &mut out);
        // Magnified, the command display's line changed is drawn over the
        // page shown, which it needs placed as a drawing does.
        let line = regis.display_gen() != line;
        if line && regis.zoom() != RegisZoom::ONE {
            let _ = out.page();
        }
        let (dirty, recolor, unpaged) = (out.dirty, out.recolor, out.unpaged);
        if let Unpaged::Some(n) = unpaged {
            regis.unpaged(n.index());
        }
        let zoom = regis.zoom();
        self.images.set_page_zoom(zoom);
        // A new page is placed: text written later in this read may land on
        // it (`Paint::hooks`). Magnified, a carried page shows rows of the
        // plane that were past the window: it is shown whole again.
        let placed = self.strs.regis_page != before;
        if placed {
            self.rehook();
        }
        let page = self.strs.regis_page;
        let whole = recolor == Recolor::Yes || (placed && zoom != RegisZoom::ONE);
        let rect = match (whole, dirty) {
            (true, _) => Some(RegisRect {
                x0: 0,
                y0: 0,
                x1: i32::from(page.w.raw()),
                y1: i32::from(page.h.raw()),
            }),
            // What was drawn on the plane, where the page shows it.
            (false, d) => d.map(|d| clip(zoom.shown(d), page.w, page.h)),
        };
        if let Some(rect) = rect {
            let map = *self.strs.regis.colors();
            self.images
                .repaint_page(page.shown, page.plane, px_of(rect), |shown, plane| {
                    RegisCanvas::show(plane, page.w, &map, rect, shown, zoom);
                });
        }
        // What the repaint showed again on the command display's line, or
        // the line itself changed: its characters drawn over it again.
        let band = self.strs.regis.display_band();
        if zoom != RegisZoom::ONE && (line || rect.is_some_and(|r| meets(r, band))) {
            let p = (page.shown, page.plane, page.w, page.h);
            paint_display(self.images, self.strs.regis, p);
        }
    }

    /// The page the next drawing needs.
    fn regis_want(&self) -> Want {
        want_of(
            self.cold.screen,
            self.book,
            (self.cols, self.rows),
            (self.cell_width, self.cell_height),
        )
    }
}

/// The page the next drawing needs on `screen`: at its window's top line,
/// the text area's size.
pub(crate) fn want_of(
    screen: Screen,
    book: &crate::storage::Book<'_>,
    (cols, rows): (Cols, Rows),
    (cw, ch): (ImagePx, ImagePx),
) -> Want {
    let (screen, line) = match screen {
        Screen::Normal => (ImageScreen::Primary, book.top),
        Screen::Alternate => (ImageScreen::Alternate, book.alt_top),
    };
    let (w, h) = regis_page_size(cols, rows, cw, ch);
    Want {
        screen,
        line,
        w,
        h,
        ch,
    }
}

/// The cursors to show with the envelope `inside`: while a one-shot input
/// waits only the input cursor, the output cursor being for ReGIS waiting
/// on the host; otherwise the output cursor inside the envelope and the
/// input cursor in multiple mode there.
pub(crate) fn shown_marks(regis: &Regis, inside: RegisEnvelope) -> [Option<RegisCursor>; 2] {
    let input = regis.input(inside);
    let output = match (inside, input) {
        (RegisEnvelope::Open, RegisInput::Off | RegisInput::Multiple) => regis.output_cursor(),
        _ => None,
    };
    let input = match input {
        RegisInput::Off => None,
        RegisInput::OneShot | RegisInput::Multiple => Some(regis.input_cursor()),
    };
    [output, input]
}

/// **The indicator's graphics input position while ReGIS input runs**: the
/// input cursor's device pixel, line then column, one-based as the field
/// shows DEC's locator in pixels -- "Graphics Input Position Indicator if
/// some graphic input mode is enabled through ReGIS ... Indicates the pixel
/// row and column of the input position" (070-14 p. 14-6), the real VT340's
/// status line showing "the X and Y pixel coordinates" (`[[source:
/// vt340test-locator]]`).
pub(crate) fn indicated(regis: &Regis, inside: RegisEnvelope) -> Option<(OneBased, OneBased)> {
    if regis.input(inside) == RegisInput::Off {
        return None;
    }
    LocatorPos::pixel(regis.input_pixel()).map(|p| (p.row, p.col))
}

/// **Take the drawn cursors away**: each rectangle they cover shown again
/// from the plane of the page they were drawn on, if it is still live.
pub(crate) fn erase(images: &mut ImageStore<'_>, regis: &mut Regis) {
    let d = regis.take_drawn();
    let map = regis.colors();
    let zoom = regis.zoom();
    let band = regis.display_band();
    let mut on_line = Meets::No;
    for m in d.marks.iter().flatten() {
        m.cover(&mut |r| {
            let r = clip(r, d.w, d.h);
            images.repaint_page(d.shown, d.plane, px_of(r), |shown, plane| {
                RegisCanvas::show(plane, d.w, map, r, shown, zoom);
            });
            if meets(r, band) {
                on_line = Meets::Yes;
            }
        });
    }
    // A cursor taken away from a magnified command display's line: the
    // line's characters drawn over it again.
    if zoom != RegisZoom::ONE && on_line == Meets::Yes {
        paint_display(images, regis, (d.shown, d.plane, d.w, d.h));
    }
}

/// Whether two rectangles meet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Meets {
    No,
    Yes,
}

/// Whether `a` and `b` share a pixel.
fn meets(a: RegisRect, b: RegisRect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

/// **The command display's line over a magnified page**: the line shown
/// again from the plane, then each of its characters drawn over it,
/// unscaled, in the text foreground through the output map, as the
/// cursors are drawn into the page shown and never into the plane
/// (`Regis::display_glyphs`). The page's lit count moves by what changed
/// on the line (`ImageStore::repaint_page`).
fn paint_display(
    images: &mut ImageStore<'_>,
    regis: &mut Regis,
    (shown, plane, w, h): (ImageId, ImageId, ImagePx, ImagePx),
) {
    let band = clip(regis.display_band(), w, h);
    if band.x0 >= band.x1 || band.y0 >= band.y1 {
        return;
    }
    let map = *regis.colors();
    let zoom = regis.zoom();
    let (pw, ph) = (i32::from(w.raw()), i32::from(h.raw()));
    images.repaint_page(shown, plane, px_of(band), |px, pl| {
        RegisCanvas::show(pl, w, &map, band, px, zoom);
        regis.display_glyphs(&mut |g, pen| {
            let Some(cell) = cell_of(g) else {
                return;
            };
            let colour = map[usize::from(pen.fg.raw() & 0x0f)];
            cell_pixels(&cell, pw, ph, &mut |x, y, bit| {
                let on = match pen.negate {
                    Negate::Off => bit,
                    Negate::On => !bit,
                };
                if !on || x < band.x0 || y < band.y0 || x >= band.x1 || y >= band.y1 {
                    return;
                }
                let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
                    return;
                };
                if let Some(o) = px.get_mut(y * w.index() + x) {
                    *o = colour;
                }
            });
        });
    });
}

/// A glyph as the canvas draws it; `None` for a built-in character with
/// no font, counted where it was received.
fn cell_of<'a>(g: &RegisGlyph<'a>) -> Option<CellDraw<'a>> {
    let bits = match g.source {
        GlyphSource::Loaded { rows, w } => CellBits::Rows { rows, w },
        GlyphSource::Builtin { cell: Some(b), .. } => CellBits::Glyph(b),
        GlyphSource::Builtin { cell: None, .. } => return None,
        GlyphSource::Error => CellBits::Solid,
    };
    Some(CellDraw {
        bits,
        at: g.at,
        unit: g.unit,
        display: g.display,
        direction: g.direction,
        slant: g.slant,
    })
}

/// `r`, held to its page, as the `(x0, y0, x1, y1)` a repaint takes.
fn px_of(r: RegisRect) -> (usize, usize, usize, usize) {
    let at = |v: i32| usize::try_from(v.max(0)).unwrap_or(0);
    (at(r.x0), at(r.y0), at(r.x1), at(r.y1))
}

/// **Draw `marks` into the page's shown image**, the page placed where the
/// next drawing needs it if it is not, and keep what was drawn. What was
/// drawn before is the caller's to have taken away. With no room for a
/// page the cursor is not drawn, and nothing is counted: a cursor is no
/// drawing (`RegisCounts::unpaged`).
pub(crate) fn draw(
    images: &mut ImageStore<'_>,
    page: &mut Page,
    regis: &mut Regis,
    want: Want,
    (marks, make): ([Option<RegisCursor>; 2], Make),
) {
    if marks == [None, None] {
        return;
    }
    let p = match make {
        Make::Yes => place(images, page, want),
        Make::No => current(images, *page, want),
    };
    let Some(p) = p else {
        return;
    };
    let map = regis.colors();
    // Each rectangle a cursor covers is repainted on its own, so the
    // page's lit count moves by what the cursor lights there, and what
    // takes it away moves it back (`ImageStore::repaint_page`).
    for m in marks.iter().flatten() {
        m.cover(&mut |r| {
            let r = clip(r, p.w, p.h);
            images.repaint_page(p.shown, p.plane, px_of(r), |shown, _| {
                m.paint(&mut |x, y, tone| {
                    if x < r.x0 || y < r.y0 || x >= r.x1 || y >= r.y1 {
                        return;
                    }
                    let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y)) else {
                        return;
                    };
                    if let Some(o) = shown.get_mut(y * p.w.index() + x) {
                        *o = map[tone.index().slot()];
                    }
                });
            });
        });
    }
    regis.set_drawn(RegisDrawn {
        shown: p.shown,
        plane: p.plane,
        w: p.w,
        h: p.h,
        marks,
    });
}

/// `r` held to a page `w` by `h`.
fn clip(r: RegisRect, w: ImagePx, h: ImagePx) -> RegisRect {
    let (w, h) = (i32::from(w.raw()), i32::from(h.raw()));
    RegisRect {
        x0: r.x0.clamp(0, w),
        y0: r.y0.clamp(0, h),
        x1: r.x1.clamp(0, w),
        y1: r.y1.clamp(0, h),
    }
}

/// Whether a cursor may place a page to be drawn on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Make {
    No,
    Yes,
}

/// The page, if it is where the next drawing needs it.
fn current(images: &ImageStore<'_>, p: Page, w: Want) -> Option<Page> {
    let here = p.screen == w.screen
        && p.line == w.line
        && p.w == w.w
        && p.h == w.h
        && images.is_page(p.shown, p.w, p.h)
        && images.is_placed(p.shown)
        && images.is_page(p.plane, RegisCanvas::plane_width(p.w), p.h);
    here.then_some(p)
}

/// **The page to draw on, placed where the window's top line is** if the
/// one there is not it. `None` without room.
fn place(images: &mut ImageStore<'_>, page: &mut Page, w: Want) -> Option<Page> {
    let p = *page;
    if let Some(p) = current(images, p, w) {
        return Some(p);
    }
    // **The window moved down over the same screen**: the lines of the
    // page still on it move to a page at the window's top, its plane
    // with them, so the next drawing, a complement one included, finds
    // what is shown where it draws (`ImageStore::carry_page`).
    let carried = (p.line < w.line).then(|| w.line.since(p.line)).flatten();
    let dy = carried.map_or(0, |r| r.index().saturating_mul(w.ch.index()));
    let carry = p.screen == w.screen
        && p.w == w.w
        && p.h == w.h
        && dy > 0
        && dy < p.h.index()
        && images.is_page(p.shown, p.w, p.h)
        && images.is_placed(p.shown)
        && images.is_page(p.plane, RegisCanvas::plane_width(p.w), p.h);
    if carry {
        if let Some(shown) = images.store_page(w.w, w.h) {
            let pid = images.place_page(shown, w.screen, w.line, Col::ZERO, KittyPlace::NONE);
            if !pid.is_none() && images.carry_page(p.shown, shown, dy) {
                *page = Page {
                    shown,
                    line: w.line,
                    ..p
                };
                return Some(*page);
            }
            images.free(shown);
        }
        // No room for the page: the old page's indices go, as without a
        // carry, and this drawing has no page. The same store again would
        // be refused again.
        images.free(p.plane);
        return None;
    }
    // The page left behind keeps what it shows; its indices go.
    if images.is_page(p.plane, RegisCanvas::plane_width(p.w), p.h) {
        images.free(p.plane);
    }
    let shown = images.store_page(w.w, w.h)?;
    let Some(plane) = images.store_page(RegisCanvas::plane_width(w.w), w.h) else {
        images.free(shown);
        return None;
    };
    let pid = images.place_page(shown, w.screen, w.line, Col::ZERO, KittyPlace::NONE);
    if pid.is_none() {
        images.free(shown);
        images.free(plane);
        return None;
    }
    images.link_page(shown, plane);
    *page = Page {
        shown,
        plane,
        screen: w.screen,
        line: w.line,
        w: w.w,
        h: w.h,
    };
    Some(*page)
}

/// Whether a delay held the read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    No,
    Yes,
}

/// How many drawings had no page, if any.
#[derive(Clone, Copy)]
enum Unpaged {
    None,
    Some(Count),
}

impl Unpaged {
    fn more(self) -> Unpaged {
        match self {
            Unpaged::None => Unpaged::Some(Count::of(1)),
            Unpaged::Some(n) => Unpaged::Some(Count::of(n.index() + 1)),
        }
    }
}

/// Whether the output map changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Recolor {
    No,
    Yes,
}

/// What ReGIS draws on and answers through.
struct Out<'a, 'b> {
    reply: &'a mut [u8],
    reply_len: &'a mut ByteLen,
    overflows: &'a mut usize,
    /// How the null button's `CSI` is sent (S7C1T, S8C1T).
    c1: C1Transmit,
    images: &'a mut ImageStore<'b>,
    page: &'a mut Page,
    want: Want,
    dirty: Option<RegisRect>,
    recolor: Recolor,
    /// Drawings with no page to draw on: no image memory, or none left.
    unpaged: Unpaged,
}

impl Out<'_, '_> {
    /// The page to draw on, made where the window's top line is if the one
    /// there is not it. `None`, and the drawing counted, without room.
    fn page(&mut self) -> Option<Page> {
        let p = place(self.images, self.page, self.want);
        if p.is_none() {
            self.unpaged = self.unpaged.more();
        }
        p
    }

    /// Draw on the page's plane with `f`, and keep what it touched.
    fn draw(&mut self, f: impl FnOnce(&mut RegisCanvas<'_>)) {
        let Some(p) = self.page() else {
            return;
        };
        let Some((_, plane)) = self.images.page_planes(p.shown, p.plane) else {
            return;
        };
        let mut c = RegisCanvas::new(plane, p.w, p.h);
        f(&mut c);
        if let Some(d) = c.dirty() {
            self.dirty = Some(match self.dirty {
                Some(o) => RegisRect {
                    x0: o.x0.min(d.x0),
                    y0: o.y0.min(d.y0),
                    x1: o.x1.max(d.x1),
                    y1: o.y1.max(d.y1),
                },
                None => d,
            });
        }
    }
}

impl RegisOut for Out<'_, '_> {
    fn line(
        &mut self,
        a: RegisPoint,
        b: RegisPoint,
        pen: &RegisPen,
        phase: &mut u32,
        span: LineSpan,
    ) {
        self.draw(|c| c.line(a, b, pen, phase, span));
    }

    fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen) {
        self.draw(|c| c.fill(points, ends, pen));
    }

    fn glyph(&mut self, g: &RegisGlyph<'_>, pen: &RegisPen) {
        // No font set: counted where it was drawn.
        let Some(cell) = cell_of(g) else {
            return;
        };
        self.draw(|c| c.cell(&cell, pen));
    }

    fn erase(&mut self, bg: RegisIndex) {
        self.draw(|c| c.erase(bg));
    }

    fn scroll(&mut self, by: RegisShift, bg: RegisIndex) {
        self.draw(|c| c.scroll(by, bg));
    }

    fn recolor(&mut self) {
        self.recolor = Recolor::Yes;
    }

    /// **A report is sent whole or not at all**, the rule `want_reply`
    /// follows: a truncated one is a wrong answer, so one that does not fit
    /// is dropped and counted in `reply_overflows`.
    fn reply(&mut self, bytes: &[u8]) {
        let at = self.reply_len.index();
        match self.reply.get_mut(at..at + bytes.len()) {
            Some(dst) => {
                dst.copy_from_slice(bytes);
                *self.reply_len = ByteLen::from_index(at + bytes.len());
            }
            None => *self.overflows += 1,
        }
    }

    fn delay(&mut self, _: RegisTicks) {}

    /// Graphics input shows its cursor from here: the page it is drawn on
    /// is placed now, at the command, and not at whichever read ends next.
    fn input_shown(&mut self) {
        let _ = place(self.images, self.page, self.want);
    }

    /// A wide stroke's marks cleared on the page drawn now; a page the
    /// window has moved off lost them when its rows moved
    /// (`ImageStore::carry_page`), and none is placed for this.
    fn unmark(&mut self, r: RegisRect) {
        let Some(p) = current(self.images, *self.page, self.want) else {
            return;
        };
        if let Some((_, plane)) = self.images.page_planes(p.shown, p.plane) {
            RegisCanvas::new(plane, p.w, p.h).unmark(r);
        }
    }

    /// `R(P(I))` in multiple mode: the null button's string, `CSI 240 ~`
    /// (`[[source: vt3xx-locator]]` Table 15-1, "If the host requests a
    /// report when none of the buttons are in use, the terminal responds
    /// with a null-button code"), the position and the carriage return,
    /// whole or dropped.
    fn interactive(&mut self, pos: &[u8]) {
        let mut b = [0u8; REPORT_MAX];
        let null = LocatorKeys::DEFAULT.string(LocatorTransition::Null, self.c1.csi());
        let r = report_bytes(&mut b, null.as_bytes(), pos);
        self.reply(r);
    }
}

/// The longest ReGIS input report: a key's bytes, at most
/// `KeyBytes::MAX_LEN`, a position and its carriage return.
pub(crate) const REPORT_MAX: usize = KeyBytes::MAX_LEN + 64;

/// `prefix`, the position and the carriage return into `out`: "Locator
/// reports begin with the code(s) of the active non-arrow key or locator
/// button pressed. Following this code is the current position of the
/// input cursor ... The report ends with the carriage return character
/// (CR)" (`[[source: vt3xx-locator]]`). The bytes written.
pub(crate) fn report_bytes<'o>(
    out: &'o mut [u8; REPORT_MAX],
    prefix: &[u8],
    pos: &[u8],
) -> &'o [u8] {
    let mut n = 0;
    for part in [prefix, pos, b"\r"] {
        let take = part.len().min(out.len() - n);
        out[n..n + take].copy_from_slice(&part[..take]);
        n += take;
    }
    &out[..n]
}

impl Term<'_> {
    /// Whether the parser is inside a ReGIS envelope: the string ReGIS
    /// began is open, its body or the `ESC` that may end it. An envelope a
    /// `CAN` or a stray `ESC` cancelled is closed, as `ST` closes one.
    pub(crate) fn regis_inside(&self) -> RegisEnvelope {
        let open = self.cold.str_kind == crate::osc::StrKind::Regis
            && matches!(
                self.node,
                crate::graph::Node::DcsStr | crate::graph::Node::StrEsc
            );
        match open {
            true => RegisEnvelope::Open,
            false => RegisEnvelope::Closed,
        }
    }

    /// **What ReGIS graphics input is doing**, for an embedder routing its
    /// keys and pointer: [`RegisInput::OneShot`] while an `R(P(I))` waits,
    /// when a pointer button is the core's, not the selection's, and the
    /// host's output is held (under [`RegisHold::On`], `Term::take_held`);
    /// [`RegisInput::Multiple`] inside an envelope in multiple mode, when
    /// the buttons are the core's too.
    pub fn regis_input(&self) -> RegisInput {
        self.store.strs.regis.input(self.regis_inside())
    }

    /// **A read's end with a cursor drawn**: if the read left the envelope
    /// by a cancel, the cursors go, as at `ST` -- the output cursor "only
    /// appears when the terminal is in ReGIS mode" (`[[source:
    /// vt3xx-regis-intro]]`). An `ESC` a read ends on is still undecided,
    /// the envelope open, and keeps them.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_read_ends(&mut self) {
        if self.regis_inside() == RegisEnvelope::Closed {
            erase(&mut self.store.images, self.store.strs.regis);
        }
    }

    /// The cursors taken away and drawn again where ReGIS has them now.
    fn regis_cursor_redraw(&mut self) {
        self.regis_cursor_again(Make::Yes);
    }

    /// [`Term::regis_cursor_redraw`], `make` saying whether a page may be
    /// placed for the cursors.
    fn regis_cursor_again(&mut self, make: Make) {
        erase(&mut self.store.images, self.store.strs.regis);
        let marks = shown_marks(self.store.strs.regis, self.regis_inside());
        let want = want_of(
            self.cold.screen,
            &self.store.book,
            (self.cols, self.rows),
            (self.cell_width, self.cell_height),
        );
        let strs = &mut self.store.strs;
        draw(
            &mut self.store.images,
            &mut strs.regis_page,
            strs.regis,
            want,
            (marks, make),
        );
    }

    /// `prefix`, the input position and a carriage return, queued whole or
    /// dropped and counted.
    fn regis_report(&mut self, prefix: &[u8]) -> bool {
        let mut pos = [0u8; 56];
        let k = self.store.strs.regis.input_position(&mut pos);
        let mut b = [0u8; REPORT_MAX];
        let r = report_bytes(&mut b, prefix, &pos[..k]);
        let (at, n) = (self.reply_len.index(), r.len());
        match at + n <= self.store.reply.len() {
            true => {
                self.store.reply[at..at + n].copy_from_slice(r);
                self.reply_len = ByteLen::from_index(at + n);
                true
            }
            false => {
                self.reply_overflows += 1;
                false
            }
        }
    }

    /// **A key while one-shot input waits**: `None` when none does, and the
    /// key is the terminal's as ever, and otherwise whether a report went.
    ///
    /// An arrow key moves the input cursor a device pixel, Shift ten, and
    /// sends nothing ("The cursor moves one pixel in the direction of the
    /// arrow", "Shift-arrow key: The cursor moves 10 pixels",
    /// `[[source: vt3xx-report]]`); a repeat moves it again where DECARM
    /// lets keys repeat. "Pressing any non-arrow key that is not dead"
    /// sends the key's own bytes, as the keyboard's modes encode them, then
    /// the position and a carriage return, and ends the input (070-13
    /// § 5.1); a key that sends nothing -- a modifier, a dead key -- and a
    /// release leave it waiting. In multiple mode the keys are the
    /// terminal's: "If you press an arrow key in multiple mode, the
    /// terminal sends that key's escape sequence to the host"
    /// (`[[source: vt3xx-locator]]`; decided by Jev, SEND, 0.96 and 0.96).
    pub(crate) fn regis_key(&mut self, e: KeyEvent) -> Option<bool> {
        if self.regis_input() != RegisInput::OneShot {
            return None;
        }
        if e.kind == KeyEventKind::Release {
            return Some(false);
        }
        if e.kind == KeyEventKind::Repeat && !self.cold.modes.is_set(ModeBit::AutoRepeat) {
            return Some(false);
        }
        let n = match e.mods.has(Modifiers::SHIFT) {
            true => 10,
            false => 1,
        };
        let step = match e.code {
            KeyCode::Up => Some((0, -n)),
            KeyCode::Down => Some((0, n)),
            KeyCode::Left => Some((-n, 0)),
            KeyCode::Right => Some((n, 0)),
            _ => None,
        };
        if let Some((dx, dy)) = step {
            self.store.strs.regis.input_step(dx, dy);
            self.regis_cursor_redraw();
            self.redraw_indicator();
            return Some(false);
        }
        let Some(k) = self.encode(e) else {
            return Some(false);
        };
        let sent = self.regis_report(k.as_bytes());
        self.store.strs.regis.input_end();
        self.regis_cursor_redraw();
        self.redraw_indicator();
        Some(sent)
    }

    /// **The pointer while graphics input runs**: a move puts the input
    /// cursor under it, and a button's transition whose string is not empty
    /// sends that string, the position and a carriage return (070-13 § 5.1
    /// and § 5.2), ending a one-shot input. The strings are DECLKD's
    /// ([`LocatorKeys`]): by default `CSI 241 ~`, `243`, `245` and `247` for
    /// the four buttons pressed (`[[source: vt3xx-locator]]` Table 15-1) and
    /// nothing for a release, which "will not terminate ReGIS One-Shot
    /// Graphics Input Mode" (DECLKD note 2). Whether a report went.
    pub(crate) fn regis_pointer(&mut self, e: &LocatorInput) -> bool {
        if self.regis_input() == RegisInput::Off {
            return false;
        }
        if let Some(px) = e.at.and_then(|a| a.px) {
            self.store
                .strs
                .regis
                .input_point(i32::from(px.x.raw()), i32::from(px.y.raw()));
        }
        let t = match e.kind {
            LocatorKind::Press(b) => Some(LocatorTransition::Down(b)),
            LocatorKind::Release(b) => Some(LocatorTransition::Up(b)),
            LocatorKind::Moved => None,
        };
        let mut sent = false;
        if let Some(t) = t {
            let csi = self.cold.replies.c1().csi();
            let s = self.store.strs.regis.locator_keys().string(t, csi);
            if !s.as_bytes().is_empty() {
                sent = self.regis_report(s.as_bytes());
                if self.regis_input() == RegisInput::OneShot {
                    self.store.strs.regis.input_end();
                }
            }
        }
        self.regis_cursor_redraw();
        sent
    }

    /// How many DECLKD definitions and strings were refused: a key number
    /// past 4, a broken hex pair, a string past six bytes, a clear
    /// parameter past 1 ([`LocatorKeys::end`]).
    pub fn locator_keys_refused(&self) -> Count {
        Count::of(
            usize::try_from(self.store.strs.regis.locator_keys().refused()).unwrap_or(usize::MAX),
        )
    }

    /// **Give up a one-shot input**: the host wrote more than the embedder
    /// holds while it waited. The input ends with 070-8 § 8.8.5.3's empty
    /// report, "Only the terminating CR is sent ... indicating that NO
    /// INPUT was to be returned", counted in `regis_counts()`, and the held
    /// output is the embedder's to feed. Jev's lean: EMPTY_REPORT at 0.38,
    /// POSITION_REPORT at 0.22 in the other order, EMPTY_REPORT having the
    /// higher probability averaged over the two orders, 0.495 against 0.37.
    /// Whether the report went; nothing when no input waits.
    pub fn regis_input_abandon(&mut self) -> bool {
        if self.regis_input() != RegisInput::OneShot {
            return false;
        }
        self.store.strs.regis.input_abandon();
        self.regis_cursor_redraw();
        let at = self.reply_len.index();
        match at < self.store.reply.len() {
            true => {
                self.store.reply[at] = b'\r';
                self.reply_len = ByteLen::from_index(at + 1);
                true
            }
            false => {
                self.reply_overflows += 1;
                false
            }
        }
    }
}
