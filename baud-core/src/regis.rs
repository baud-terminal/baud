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
    /// **Where its plane's blinking pixels may be**: every drawing made
    /// with blink on widens it, a scroll of the page widens it to the
    /// page, and a flip of the blink phase repaints it and drops it when
    /// none is left there. `None` when none can be.
    blink: Option<RegisRect>,
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
        blink: None,
    };

    /// Where the page is and what it is drawn from, without what blinks.
    fn spot(self) -> Page {
        Page {
            blink: None,
            ..self
        }
    }

    /// The whole page.
    fn whole(self) -> RegisRect {
        RegisRect {
            x0: 0,
            y0: 0,
            x1: i32::from(self.w.raw()),
            y1: i32::from(self.h.raw()),
        }
    }
}

/// **The second drawing surface** (070-8 § 8.8.2.1, the VT340's second
/// "page of graphics page memory"): a plane of indices the page's size,
/// placed nowhere, shown through the page while `S(P1)` displays it. It is
/// the screen's: the shell's text, its erases and its scrolling act on the
/// first surface, the text page it is coupled to, and never reach it. Made
/// when first drawn on or displayed, and made again, empty, at another
/// size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Second {
    plane: ImageId,
    w: ImagePx,
    h: ImagePx,
    /// Where its blinking pixels may be, as [`Page`] keeps its own.
    blink: Option<RegisRect>,
}

impl Second {
    /// None made.
    pub(crate) const NONE: Second = Second {
        plane: ImageId::NONE,
        w: ImagePx::ZERO,
        h: ImagePx::ZERO,
        blink: None,
    };

    /// Whether it is a live plane for a page `w` by `h`.
    fn fits(self, images: &ImageStore<'_>, w: ImagePx, h: ImagePx) -> bool {
        self.w == w && self.h == h && images.is_page(self.plane, RegisCanvas::plane_width(w), h)
    }

    /// The memory it holds back to the arena: a reset's.
    pub(crate) fn free(&mut self, images: &mut ImageStore<'_>) {
        if images.is_page(self.plane, RegisCanvas::plane_width(self.w), self.h) {
            images.free(self.plane);
        }
        *self = Second::NONE;
    }
}

/// **The plane the page displays and where its blinking pixels may be**:
/// the first surface's own, or under `S(P1)` the second's, if it fits the
/// page.
fn shown_plane(page: Page, second: Second, regis: &Regis, images: &ImageStore<'_>) -> ImageId {
    match regis.surfaces().1 {
        RegisSurface::First => page.plane,
        RegisSurface::Second if second.fits(images, page.w, page.h) => second.plane,
        RegisSurface::Second => ImageId::NONE,
    }
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
    /// DECRSPS's `t`, DECLKD's `w` and DECRSTS's `p` after `$`, ReGIS's bare
    /// `p`, or the
    /// `+` of XTGETTCAP and XTSETTCAP. Out of line, so the
    /// per-byte loop's arm is DECRQSS's test and one call, the shape it had
    /// with DECRSPS's alone.
    #[cold]
    #[inline(never)]
    pub(crate) fn dcs_start(&mut self, b: StreamByte, csi: &Csi) -> crate::osc::StrKind {
        let key = (b.raw_byte(), csi.intermediate().map(StreamByte::raw_byte));
        // **DECRSTS and DECRSPS are Level 2's** (Terminal State
        // Interrogation, "Levels: 2x (TSI), 3-4", `[[source: dec-std-070]]`
        // pp. 5-205 and 5-208): below it the body is consumed and dropped,
        // and the string counted.
        let f = match key {
            (b'p', Some(b'$')) => Some(Function::Decrsts),
            (b't', Some(b'$')) => Some(Function::Decrsps),
            _ => None,
        };
        if !introduced_by(f).admits(self.cold.level) {
            self.unsupported_total += 1;
            return crate::osc::StrKind::Other;
        }
        match key {
            (b'p', Some(b'$')) => self.restore_state_start(csi),
            (b'p', _) => self.regis_start(csi),
            (b'w', _) => self.locator_keys_start(csi),
            (b'+', _) => crate::dcs::tcap_start(csi),
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
        // drawn, and maybe the second surface displayed: they go at the
        // read's end, and before this one draws.
        self.regis_cursor_off();
        show_first(self.images, self.strs.regis_page, self.strs.regis);
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
    /// A delay inside a macrograph stops the macrograph where it is, and so
    /// do its reports once they leave the replies low (`Out::replies_low`);
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
            (&mut strs.regis_page, strs.regis_second),
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
        self.str_end(
            StreamByte::from_stream(self.arena[from.index()]),
            from,
            node,
        );
        ReadLen::ONE
    }

    /// Hold the read from `at` if the interpreter met a delay, or a
    /// one-shot `R(P(I))` waits: "While the terminal is in graphics input
    /// mode, no further data from the application (screen output) is
    /// processed until the locator input is complete" (070-13 § 5.1). The
    /// embedder reads the second from `Term::regis_input`, where the first
    /// has its ticks. **And under `ReplyHold::On` once its reports fill the
    /// reply buffer past one control's worth** (`Paint::hold_for_replies`):
    /// a report goes at each `R` command, inside the string, so this is
    /// where the read can stop for them to go out, a macrograph playing
    /// stopped with it.
    fn regis_hold(&mut self, at: BytePos, len: ReadLen) -> Held {
        let wait = self.strs.regis.take_wait();
        let input = self.strs.regis.input(RegisEnvelope::Open) == RegisInput::OneShot;
        if wait.is_none() && !input && !self.replies_low() {
            return Held::No;
        }
        if let Some(t) = wait {
            let w = self.strs.regis_wait.map_or(0, RegisTicks::get);
            self.strs.regis_wait = Some(RegisTicks::of(w.saturating_add(t.get())));
        }
        self.cold.width.held = Some(crate::term::Rest { at, len });
        Held::Yes
    }

    /// **A macrograph its reports stopped plays on before the rest of the
    /// read**, once they have gone out (`Term::take_held`): the rest's first
    /// byte may leave the envelope -- `CAN`, `SUB`, an `ESC` and a final --
    /// and the reports the macrograph still holds were owed before it, as
    /// they are when the whole macrograph fits. If it stops again the read
    /// it starts, `len` bytes from `at`, is held whole. Returns how much of
    /// that read the playing on took.
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_play_on(&mut self, at: BytePos, len: ReadLen) -> ReadLen {
        if self.cold.str_kind != crate::osc::StrKind::Regis {
            return ReadLen::ZERO;
        }
        self.regis_run(|regis, out| regis.resume(out));
        match self.regis_hold(at, len) {
            Held::Yes => {
                self.regis_input_shown();
                len
            }
            Held::No => ReadLen::ZERO,
        }
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
        // A page the next drawing leaves behind shows its blinking pixels
        // in their own colours from now on.
        // A page left behind while the second surface is displayed, as a
        // resize inside an envelope leaves one, shows the first, the
        // surface the shell's text is on.
        if current(self.images, self.strs.regis_page, want).is_none() {
            settle_blink(self.images, &mut self.strs.regis_page, self.strs.regis);
            if self.strs.regis.surfaces().1 == RegisSurface::Second {
                repaint_first(self.images, self.strs.regis_page, self.strs.regis);
            }
        }
        let regis = &mut *self.strs.regis;
        regis.set_page(i32::from(want.w.raw()), i32::from(want.h.raw()));
        // How the page shows its plane, for a carry inside the run and for
        // text and erases after it (`ImageStore::set_page_zoom`).
        self.images.set_page_zoom(regis.zoom());
        let mut out = Out {
            reply: &mut *self.reply,
            cold: &*self.cold,
            hold: self.cold.width.reply_hold,
            images: &mut *self.images,
            page: &mut self.strs.regis_page,
            second: &mut self.strs.regis_second,
            surfaces: regis.surfaces(),
            want,
            dirty: None,
            recolor: Recolor::No,
            unpaged: Unpaged::None,
            hard_copy: None,
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
        let hard_copy = out.hard_copy;
        if let Unpaged::Some(n) = unpaged {
            regis.unpaged(n.index());
        }
        let zoom = regis.zoom();
        self.images.set_page_zoom(zoom);
        // A new page is placed: text written later in this read may land on
        // it (`Paint::hooks`). Magnified, a carried page shows rows of the
        // plane that were past the window: it is shown whole again.
        let placed = self.strs.regis_page.spot() != before.spot();
        if placed {
            self.rehook();
        }
        let page = self.strs.regis_page;
        // A page placed under `S(P1)` shows the second surface, whole.
        let second = self.strs.regis.surfaces().1 == RegisSurface::Second;
        let whole = recolor == Recolor::Yes || (placed && (zoom != RegisZoom::ONE || second));
        let plane = shown_plane(page, self.strs.regis_second, self.strs.regis, self.images);
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
            let blink = self.strs.regis.blink_show();
            self.images
                .repaint_page(page.shown, plane, px_of(rect), |shown, plane| {
                    RegisCanvas::show(plane, page.w, &map, rect, shown, zoom, blink);
                });
        }
        // What the repaint showed again on the command display's line, or
        // the line itself changed: its characters drawn over it again.
        let band = self.strs.regis.display_band();
        if zoom != RegisZoom::ONE && (line || rect.is_some_and(|r| meets(r, band))) {
            let p = (page.shown, plane, page.w, page.h);
            paint_display(self.images, self.strs.regis, p);
        }
        if let Some((area, offset)) = hard_copy {
            self.hard_copy(area, offset);
        }
    }

    /// The page's size in pixels: the text area, or the VT300's 800 by 480
    /// while no cell size is known -- what a hard copy's whole screen is.
    pub(crate) fn regis_page_px(&self) -> (ImagePx, ImagePx) {
        let w = self.regis_want();
        (w.w, w.h)
    }

    /// **The page displaying the second surface and that surface's
    /// plane**, under `S(P1)`: what a hard copy reads the page's indices
    /// from in place of its own plane.
    pub(crate) fn regis_second_shown(&self) -> Option<(ImageId, ImageId)> {
        let page = self.strs.regis_page;
        let plane = shown_plane(page, self.strs.regis_second, self.strs.regis, self.images);
        (plane != page.plane && !plane.is_none()).then_some((page.shown, plane))
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
    // "Both the input cursor and output cursor move to the corresponding
    // position on the selected page" (`[[source: vt3xx-position]]`): on
    // a surface not displayed neither is seen.
    let (drawn, shown) = regis.surfaces();
    if drawn != shown {
        return [None, None];
    }
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
    let blink = regis.blink_show();
    let map = regis.colors();
    let zoom = regis.zoom();
    let band = regis.display_band();
    let mut on_line = Meets::No;
    for m in d.marks.iter().flatten() {
        m.cover(&mut |r| {
            let r = clip(r, d.w, d.h);
            images.repaint_page(d.shown, d.plane, px_of(r), |shown, plane| {
                RegisCanvas::show(plane, d.w, map, r, shown, zoom, blink);
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
    let blink = regis.blink_show();
    let (pw, ph) = (i32::from(w.raw()), i32::from(h.raw()));
    images.repaint_page(shown, plane, px_of(band), |px, pl| {
        RegisCanvas::show(pl, w, &map, band, px, zoom, blink);
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
    (page, second): (&mut Page, Second),
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
    let plane = shown_plane(p, second, regis, images);
    let map = regis.colors();
    // Each rectangle a cursor covers is repainted on its own, so the
    // page's lit count moves by what the cursor lights there, and what
    // takes it away moves it back (`ImageStore::repaint_page`).
    for m in marks.iter().flatten() {
        m.cover(&mut |r| {
            let r = clip(r, p.w, p.h);
            images.repaint_page(p.shown, plane, px_of(r), |shown, _| {
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
        plane,
        w: p.w,
        h: p.h,
        marks,
    });
}

/// The smallest rectangle holding `a` and `b`.
fn union(a: RegisRect, b: RegisRect) -> RegisRect {
    RegisRect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// Whether `p` is a live page drawn from its plane, wherever the window is.
fn live(images: &ImageStore<'_>, p: Page) -> bool {
    images.is_page(p.shown, p.w, p.h)
        && images.is_placed(p.shown)
        && images.is_page(p.plane, RegisCanvas::plane_width(p.w), p.h)
}

/// **A page's blinking pixels repainted as the blink phase shows them**,
/// as `show` says, where the page keeps them; the rectangle is dropped
/// when none is left in it. Whether anything was repainted.
fn repaint_blink(
    images: &mut ImageStore<'_>,
    (p, plane, rect): (Page, ImageId, &mut Option<RegisRect>),
    regis: &mut Regis,
    show: RegisBlinkShow,
) -> RegisRepainted {
    let Some(r) = *rect else {
        return RegisRepainted::No;
    };
    let shows = images.is_page(plane, RegisCanvas::plane_width(p.w), p.h);
    if !live(images, p) || !shows {
        *rect = None;
        return RegisRepainted::No;
    }
    let map = *regis.colors();
    let zoom = regis.zoom();
    let shown = clip(zoom.shown(r), p.w, p.h);
    let mut left = Blinks::No;
    images.repaint_page(p.shown, plane, px_of(shown), |px, plane| {
        if RegisCanvas::blinks(plane, p.w, r) {
            left = Blinks::Yes;
        }
        RegisCanvas::show(plane, p.w, &map, shown, px, zoom, show);
    });
    if left == Blinks::No {
        *rect = None;
    }
    // The command display's line drawn over a magnified page again.
    if zoom != RegisZoom::ONE && meets(shown, regis.display_band()) {
        paint_display(images, regis, (p.shown, plane, p.w, p.h));
    }
    RegisRepainted::Yes
}

/// **A page left behind stops blinking**, its blinking pixels shown in
/// their own colours: once the next drawing's page is elsewhere nothing
/// repaints it at a flip. What blinks on the lines a carry takes to the
/// new page goes on blinking there ([`place`]).
fn settle_blink(images: &mut ImageStore<'_>, page: &mut Page, regis: &mut Regis) {
    if regis.blink_phase() != RegisPhase::Shown {
        let mut keep = page.blink;
        let p = *page;
        repaint_blink(
            images,
            (p, p.plane, &mut keep),
            regis,
            RegisBlinkShow::Shown,
        );
    }
}

/// **The first surface displayed again as an envelope ends**
/// ([`Regis::show_first`]): the page shown whole from its own plane.
pub(crate) fn show_first(images: &mut ImageStore<'_>, page: Page, regis: &mut Regis) {
    if regis.show_first() == RegisRepainted::No {
        return;
    }
    repaint_first(images, page, regis);
}

/// **The page shown whole from its own plane**, the first surface's.
fn repaint_first(images: &mut ImageStore<'_>, page: Page, regis: &mut Regis) {
    if !live(images, page) {
        return;
    }
    erase(images, regis);
    let (map, zoom, blink) = (*regis.colors(), regis.zoom(), regis.blink_show());
    let r = page.whole();
    images.repaint_page(page.shown, page.plane, px_of(r), |px, plane| {
        RegisCanvas::show(plane, page.w, &map, r, px, zoom, blink);
    });
    if zoom != RegisZoom::ONE {
        paint_display(images, regis, (page.shown, page.plane, page.w, page.h));
    }
}

/// Whether a page's plane still holds a blinking pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Blinks {
    No,
    Yes,
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
                // The plane's rows moved up: what blinks is above where it
                // was, and the rectangle reaching the top still holds it.
                *page = Page {
                    shown,
                    line: w.line,
                    blink: p.blink.map(|r| RegisRect { y0: 0, ..r }),
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
        blink: None,
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
    reply: &'a mut crate::outgoing::Outgoing<'b>,
    /// The terminal's state, read for what S7C1T and S8C1T last set: how
    /// the null button's `CSI` and every report are sent.
    cold: &'a crate::term::Cold,
    /// Whether a read stops for its answers (`Paint::replies_low`).
    hold: ReplyHold,
    images: &'a mut ImageStore<'b>,
    page: &'a mut Page,
    second: &'a mut Second,
    /// The surface drawn on and the one displayed, `P(P)` and `S(P)`.
    surfaces: (RegisSurface, RegisSurface),
    want: Want,
    dirty: Option<RegisRect>,
    recolor: Recolor,
    /// Drawings with no page to draw on: no image memory, or none left.
    unpaged: Unpaged,
    /// An `S(H)` met, printed once the page shows what was drawn before it.
    hard_copy: Option<(RegisHardCopy, RegisOffset)>,
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

    /// **The second surface's plane**, made, empty, if there is none of
    /// the page's size. `None`, and the drawing counted, without room.
    fn second_plane(&mut self) -> Option<ImageId> {
        let (w, h) = (self.want.w, self.want.h);
        if self.second.fits(self.images, w, h) {
            return Some(self.second.plane);
        }
        self.second.free(self.images);
        let Some(plane) = self.images.store_page(RegisCanvas::plane_width(w), h) else {
            self.unpaged = self.unpaged.more();
            return None;
        };
        *self.second = Second {
            plane,
            w,
            h,
            blink: None,
        };
        Some(plane)
    }

    /// Draw on the plane of the surface `P(P)` selects with `f`, and keep
    /// what it touched where the page displays it, and where it may have
    /// drawn blinking pixels when `blink` is on. Drawing on the second
    /// surface while the first is displayed places no page.
    fn draw(&mut self, blink: RegisBlink, f: impl FnOnce(&mut RegisCanvas<'_>)) {
        let (on, shown) = self.surfaces;
        let d = match on {
            RegisSurface::First => {
                let Some(p) = self.page() else {
                    return;
                };
                let Some((_, plane)) = self.images.page_planes(p.shown, p.plane) else {
                    return;
                };
                let mut c = RegisCanvas::new(plane, p.w, p.h);
                f(&mut c);
                let d = c.dirty();
                if let (Some(d), RegisBlink::On) = (d, blink) {
                    self.page.blink = Some(self.page.blink.map_or(d, |b| union(b, d)));
                }
                d
            }
            RegisSurface::Second => {
                let (w, h) = (self.want.w, self.want.h);
                if shown == RegisSurface::Second && self.page().is_none() {
                    return;
                }
                let Some(id) = self.second_plane() else {
                    return;
                };
                let Some(plane) = self.images.regis_bytes_mut(id) else {
                    return;
                };
                let mut c = RegisCanvas::new(plane, w, h);
                f(&mut c);
                let d = c.dirty();
                if let (Some(d), RegisBlink::On) = (d, blink) {
                    self.second.blink = Some(self.second.blink.map_or(d, |b| union(b, d)));
                }
                d
            }
        };
        if on != shown {
            return;
        }
        if let Some(d) = d {
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
        self.draw(pen.blink, |c| c.line(a, b, pen, phase, span));
    }

    fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen) {
        self.draw(pen.blink, |c| c.fill(points, ends, pen));
    }

    fn flood(&mut self, seed: RegisPoint, bound: RegisBound, pen: &RegisPen) {
        let mut stack = [RegisSpan::default(); FLOOD_STACK];
        self.draw(pen.blink, |c| c.flood(seed, bound, pen, &mut stack));
    }

    fn glyph(&mut self, g: &RegisGlyph<'_>, pen: &RegisPen) {
        // No font set: counted where it was drawn.
        let Some(cell) = cell_of(g) else {
            return;
        };
        self.draw(pen.blink, |c| c.cell(&cell, pen));
    }

    fn erase(&mut self, bg: RegisIndex) {
        self.draw(RegisBlink::Off, |c| c.erase(bg));
        match self.surfaces.0 {
            RegisSurface::First => self.page.blink = None,
            RegisSurface::Second => self.second.blink = None,
        }
    }

    fn scroll(&mut self, by: RegisShift, bg: RegisIndex) {
        self.draw(RegisBlink::Off, |c| c.scroll(by, bg));
        let whole = RegisRect {
            x0: 0,
            y0: 0,
            x1: i32::from(self.want.w.raw()),
            y1: i32::from(self.want.h.raw()),
        };
        match self.surfaces.0 {
            RegisSurface::First if self.page.blink.is_some() => self.page.blink = Some(whole),
            RegisSurface::Second if self.second.blink.is_some() => {
                self.second.blink = Some(whole);
            }
            _ => {}
        }
    }

    /// **The surface displayed changed**: the page shows the other plane,
    /// whole, placed for it if need be, the second made if it is the one.
    fn surfaces(&mut self, draw: RegisSurface, show: RegisSurface) {
        let was = self.surfaces.1;
        self.surfaces = (draw, show);
        if show == was || self.page().is_none() {
            return;
        }
        if show == RegisSurface::First || self.second_plane().is_some() {
            self.recolor = Recolor::Yes;
        }
    }

    fn hard_copy(&mut self, area: RegisHardCopy, offset: RegisOffset) {
        self.hard_copy = Some((area, offset));
    }

    fn recolor(&mut self) {
        self.recolor = Recolor::Yes;
    }

    /// **A report is sent whole or not at all**, the rule `want_reply`
    /// follows: a truncated one is a wrong answer, so one that does not fit
    /// is dropped and counted in `ReplyCounts::overflows`.
    fn reply(&mut self, bytes: &[u8]) {
        self.reply.lend(self.cold).regis(&RegisReport(bytes));
    }

    /// The test `Paint::replies_low` makes between two controls, made
    /// between two characters of a macrograph playing: with the hold on,
    /// the playing stops once its reports leave less room than one control
    /// can owe, and `Paint::regis_hold` stops the read there.
    fn replies_low(&self) -> bool {
        self.reply.low() && self.hold == ReplyHold::On
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
        let null =
            LocatorKeys::DEFAULT.string(LocatorTransition::Null, self.cold.replies.c1().csi());
        let r = report_bytes(&mut b, null.as_bytes(), pos);
        self.reply.lend(self.cold).regis(&r);
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
fn report_bytes<'o>(out: &'o mut [u8; REPORT_MAX], prefix: &[u8], pos: &[u8]) -> RegisReport<'o> {
    let mut n = 0;
    for part in [prefix, pos, b"\r"] {
        let take = part.len().min(out.len() - n);
        out[n..n + take].copy_from_slice(&part[..take]);
        n += take;
    }
    RegisReport(&out[..n])
}

/// **A ReGIS input report**, as [`report_bytes`] builds it or the
/// carriage return alone: the one shape of reply [`crate::outgoing::Outgoing`]
/// takes from ReGIS, built only in this file.
pub(crate) struct RegisReport<'a>(&'a [u8]);

impl RegisReport<'_> {
    pub(crate) fn as_bytes(&self) -> &[u8] {
        self.0
    }
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

    /// **The blink clock's phase**, from the embedder at each flip: a
    /// ReGIS page's blinking pixels show their own colours in
    /// [`RegisPhase::Shown`] and the background's in
    /// [`RegisPhase::Hidden`] (070-8 § 8.8.3.1), at SGR 5's rate, as text
    /// blinks; in [`RegisPhase::Faint`], dimmed flashing lights' off
    /// phase, half way to the background's, as text's off phase is drawn
    /// faint. The page the next drawing goes on is repainted where it
    /// keeps its blinking pixels, the cursors drawn over it again;
    /// whether anything was, which owes a frame.
    pub fn set_regis_blink(&mut self, phase: RegisPhase) -> RegisRepainted {
        let regis = &mut *self.store.strs.regis;
        if regis.blink_phase() == phase {
            return RegisRepainted::No;
        }
        regis.set_blink_phase(phase);
        let strs = &mut self.store.strs;
        let page = strs.regis_page;
        let plane = shown_plane(page, strs.regis_second, strs.regis, &self.store.images);
        let rect = match strs.regis.surfaces().1 {
            RegisSurface::First => &mut strs.regis_page.blink,
            RegisSurface::Second => &mut strs.regis_second.blink,
        };
        if rect.is_none() {
            return RegisRepainted::No;
        }
        erase(&mut self.store.images, strs.regis);
        let show = strs.regis.blink_show();
        let done = repaint_blink(
            &mut self.store.images,
            (page, plane, rect),
            strs.regis,
            show,
        );
        self.regis_cursor_again(Make::No);
        done
    }

    /// **Whether the page shown keeps blinking pixels**, for an embedder
    /// that wakes only when something is due: [`RegisBlink::On`] owes a
    /// [`Term::set_regis_blink`] at each of SGR 5's flips, and
    /// [`RegisBlink::Off`] none, a page whose blinking pixels were all
    /// written over steadily reading `Off` once a flip has found none.
    pub fn regis_blink(&self) -> RegisBlink {
        let strs = &self.store.strs;
        let rect = match strs.regis.surfaces().1 {
            RegisSurface::First => &strs.regis_page.blink,
            RegisSurface::Second => &strs.regis_second.blink,
        };
        match rect {
            Some(_) => RegisBlink::On,
            None => RegisBlink::Off,
        }
    }

    /// **A read's end with a cursor drawn or the second surface
    /// displayed**: if the read left the envelope, by a cancel, the
    /// cursors go -- the output cursor "only appears when the terminal is
    /// in ReGIS mode" (`[[source: vt3xx-regis-intro]]`) -- and the first
    /// surface is displayed again ([`Regis::show_first`]); `ST` did both
    /// as it ended the envelope. An `ESC` a read ends on is still
    /// undecided, the envelope open, and keeps them. Text written after a
    /// cancel clears its cells of the first too, and a page it leaves with
    /// nothing lit is freed only once the first shows
    /// (`ImageStore::repaint_page`).
    #[cold]
    #[inline(never)]
    pub(crate) fn regis_read_ends(&mut self) {
        if self.regis_inside() == RegisEnvelope::Closed {
            erase(&mut self.store.images, self.store.strs.regis);
            let strs = &mut self.store.strs;
            show_first(&mut self.store.images, strs.regis_page, strs.regis);
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
            (&mut strs.regis_page, strs.regis_second),
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
        self.store.reply.lend(&self.cold).regis(&r)
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
    /// the position and a carriage return, and ends the input (070-13 §
    /// 5.1); a key that sends nothing -- a modifier, a dead key -- and a
    /// release leave it waiting. In multiple mode the keys are the
    /// terminal's: "If you press an arrow key in multiple mode, the
    /// terminal sends that key's escape sequence to the host"
    /// (`[[source: vt3xx-locator]]`).
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
    /// output is the embedder's to feed. Whether the report went; nothing
    /// when no input waits.
    pub fn regis_input_abandon(&mut self) -> bool {
        if self.regis_input() != RegisInput::OneShot {
            return false;
        }
        self.store.strs.regis.input_abandon();
        self.regis_cursor_redraw();
        self.store.reply.lend(&self.cold).regis(&RegisReport(b"\r"))
    }
}
