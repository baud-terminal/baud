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
//! drawing begins a new page there and the old one stays where it scrolled,
//! as the VT300's screen-relative coordinates draw on the screen and its
//! scrolled contents move away beneath them.

use crate::paint::Paint;
use crate::term::Screen;
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
struct Want {
    screen: ImageScreen,
    line: LineIdx,
    w: ImagePx,
    h: ImagePx,
}

/// The largest page side: a text area past this is drawn on a page of
/// this size, the 800 by 480 device fitted into it.
const PAGE_MAX: u16 = 8192;

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
            _ => self.restore_start(csi),
        }
    }

    /// A `DCS Pn p`: enter ReGIS, `Pn` the envelope's first parameter.
    fn regis_start(&mut self, csi: &Csi) -> crate::osc::StrKind {
        let e = RegisEntry::of(Some(csi.params().raw(ParamIdx::FIRST, 0)));
        self.regis_run(|regis, out| regis.enter(e, out));
        crate::osc::StrKind::Regis
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
        let want = self.regis_want();
        let regis = &mut *self.strs.regis;
        regis.set_page(i32::from(want.w.raw()), i32::from(want.h.raw()));
        let mut out = Out {
            reply: &mut *self.reply,
            reply_len: &mut self.reply_len,
            overflows: &mut self.reply_overflows,
            images: &mut *self.images,
            page: &mut self.strs.regis_page,
            want,
            dirty: None,
            recolor: Recolor::No,
            unpaged: Unpaged::None,
        };
        let before = *out.page;
        f(regis, &mut out);
        let (dirty, recolor, unpaged) = (out.dirty, out.recolor, out.unpaged);
        if let Unpaged::Some(n) = unpaged {
            regis.unpaged(n.index());
        }
        // A new page is placed: text written later in this read may land on
        // it (`Paint::hooks`).
        if self.strs.regis_page != before {
            self.rehook();
        }
        let page = self.strs.regis_page;
        let rect = match (recolor, dirty) {
            (Recolor::Yes, _) => Some(RegisRect {
                x0: 0,
                y0: 0,
                x1: i32::from(page.w.raw()),
                y1: i32::from(page.h.raw()),
            }),
            (Recolor::No, d) => d,
        };
        let Some(rect) = rect else {
            return;
        };
        let map = *self.strs.regis.colors();
        if let Some((shown, plane)) = self.images.page_planes(page.shown, page.plane) {
            RegisCanvas::show(plane, page.w, &map, rect, shown);
            self.images.repainted();
        }
    }

    /// The page the next drawing needs.
    fn regis_want(&self) -> Want {
        let (screen, line) = match self.cold.screen {
            Screen::Normal => (ImageScreen::Primary, self.book.top),
            Screen::Alternate => (ImageScreen::Alternate, self.book.alt_top),
        };
        let side = |cells: usize, px: ImagePx| {
            let n = cells
                .saturating_mul(px.index())
                .clamp(2, usize::from(PAGE_MAX));
            ImagePx::of(u16::try_from(n).unwrap_or(PAGE_MAX))
        };
        let (w, h) = match (self.cell_width.raw(), self.cell_height.raw()) {
            (0, _) | (_, 0) => (ImagePx::of(800), ImagePx::of(480)),
            _ => (
                side(self.cols.index(), self.cell_width),
                side(self.rows.index(), self.cell_height),
            ),
        };
        Want { screen, line, w, h }
    }
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
        let p = *self.page;
        let w = self.want;
        let current = p.screen == w.screen
            && p.line == w.line
            && p.w == w.w
            && p.h == w.h
            && self.images.is_page(p.shown, p.w, p.h)
            && self.images.is_placed(p.shown)
            && self
                .images
                .is_page(p.plane, RegisCanvas::plane_width(p.w), p.h);
        if current {
            return Some(p);
        }
        // The page left behind keeps what it shows; its indices go.
        if self
            .images
            .is_page(p.plane, RegisCanvas::plane_width(p.w), p.h)
        {
            self.images.free(p.plane);
        }
        let Some(shown) = self.images.store_page(w.w, w.h) else {
            self.unpaged = self.unpaged.more();
            return None;
        };
        let Some(plane) = self.images.store_page(RegisCanvas::plane_width(w.w), w.h) else {
            self.images.free(shown);
            self.unpaged = self.unpaged.more();
            return None;
        };
        let pid = self
            .images
            .place_page(shown, w.screen, w.line, Col::ZERO, KittyPlace::NONE);
        if pid.is_none() {
            self.images.free(shown);
            self.images.free(plane);
            self.unpaged = self.unpaged.more();
            return None;
        }
        self.images.link_page(shown, plane);
        *self.page = Page {
            shown,
            plane,
            screen: w.screen,
            line: w.line,
            w: w.w,
            h: w.h,
        };
        Some(*self.page)
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
        let GlyphSource::Loaded { rows, w } = g.source else {
            return;
        };
        let cell = CellDraw {
            rows,
            w,
            at: g.at,
            unit: g.unit,
            display: g.display,
            direction: g.direction,
            slant: g.slant,
        };
        self.draw(|c| c.cell(&cell, pen));
    }

    fn erase(&mut self, bg: RegisIndex) {
        self.draw(|c| c.erase(bg));
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
}
