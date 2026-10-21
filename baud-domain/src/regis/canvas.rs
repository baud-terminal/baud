//! The page ReGIS draws on: a plane of output-map indices, one byte a pixel,
//! turned into colour only when it is shown.
//!
//! **Indices, not colours**, because three things the text asks for read a
//! pixel's index back: an output map change recolours what is already drawn
//! (`[[source: vt3xx-screen-control]]`, "You do not have to redraw an image to
//! change its color"), complement writing inverts "the binary code for that
//! pixel" (`[[source: vt3xx-write-control]]`; 070-8 § 8.7.2.4.1.1), and the
//! foreground mask limits a write to some of its planes (§ 8.7.2.4.2.1).
//!
//! **A pixel never written is clear**, so the text beneath shows through:
//! the low four bits are the index, and [`WRITTEN`] says there is one.

use super::*;

/// The bit a written pixel carries beside its index.
pub const WRITTEN: u8 = 0x80;
/// **The bit a pixel the wide stroke in progress wrote carries**, so a
/// round pen swept along a polyline or a curve writes each pixel once,
/// complement writing included, however many of its positions cover it
/// ([`RegisCanvas::unmark`] clears it when the stroke ends; a move of
/// the page's rows drops it).
pub const MARK: u8 = 0x40;

/// A rectangle of the page, `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl RegisRect {
    fn union(self, o: RegisRect) -> RegisRect {
        RegisRect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// Which ends of a line are drawn. A polyline's joints are drawn once, so
/// complement writing does not invert a joint back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineSpan {
    /// Both ends.
    Whole,
    /// All but the first pixel, which the line before it drew.
    Rest,
    /// Neither end: the closing segment of a loop.
    Inner,
    /// [`LineSpan::Rest`], the line before a wide one of the same stroke
    /// having drawn its last step with the same pen: a wide line's writing
    /// starts from that pen ([`RegisCanvas::wide_line`]).
    Chained,
    /// [`LineSpan::Inner`], after a line as [`LineSpan::Chained`] says.
    ChainedInner,
}

/// A page's plane, lent: `w` by `h` pixels, a row `stride` bytes.
pub struct RegisCanvas<'a> {
    plane: &'a mut [u8],
    stride: usize,
    w: i32,
    h: i32,
    dirty: Option<RegisRect>,
}

impl<'a> RegisCanvas<'a> {
    /// A canvas over `plane`, a page `w` by `h` pixels laid out a row of
    /// [`RegisCanvas::stride`] bytes after another.
    pub fn new(
        plane: &'a mut [u8],
        w: crate::image::ImagePx,
        h: crate::image::ImagePx,
    ) -> RegisCanvas<'a> {
        let stride = RegisCanvas::stride(w);
        let h = i32::from(h.raw()).min((plane.len() / stride.max(1)) as i32);
        RegisCanvas {
            plane,
            stride,
            w: i32::from(w.raw()),
            h,
            dirty: None,
        }
    }

    /// The bytes a row of a `w`-pixel page's plane takes: its width,
    /// rounded to whole arena words, four indices a word.
    pub fn stride(w: crate::image::ImagePx) -> usize {
        w.index().div_ceil(4) * 4
    }

    /// The plane of a `w`-pixel page as an image's width in arena words.
    pub fn plane_width(w: crate::image::ImagePx) -> crate::image::ImagePx {
        crate::image::ImagePx::of(u16::try_from(w.index().div_ceil(4)).unwrap_or(u16::MAX))
    }

    /// What has been drawn since the canvas was made.
    pub fn dirty(&self) -> Option<RegisRect> {
        self.dirty
    }

    fn touch(&mut self, x: i32, y: i32) {
        let r = RegisRect {
            x0: x,
            y0: y,
            x1: x + 1,
            y1: y + 1,
        };
        self.dirty = Some(match self.dirty {
            Some(d) => d.union(r),
            None => r,
        });
    }

    fn mark(&mut self, r: RegisRect) {
        self.dirty = Some(match self.dirty {
            Some(d) => d.union(r),
            None => r,
        });
    }

    /// One pixel, `on` where the pattern (after `W(N)`) has a one: the
    /// writing modes of § 8.7.2.4.1 through the foreground mask. Whether
    /// it was written.
    fn put(&mut self, x: i32, y: i32, on: bool, pen: &RegisPen) -> bool {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || !pen.clip.allows(x, y) {
            return false;
        }
        let i = y as usize * self.stride + x as usize;
        let old = self.plane[i];
        let had = old & 0x0f;
        let mask = pen.planes.bits();
        let value = match (pen.mode, on) {
            (WriteMode::Overlay, true) => pen.fg.raw(),
            (WriteMode::Overlay, false) => return false,
            (WriteMode::Replace, true) => pen.fg.raw(),
            (WriteMode::Replace, false) => pen.bg.raw(),
            // "When you use complement writing to write over a pixel, the
            // terminal changes the binary code for that pixel to its opposite
            // value" -- in the planes the mask allows.
            (WriteMode::Complement, true) => !had,
            (WriteMode::Complement, false) => return false,
            // "ignore the contents of the pattern register, and Erase each
            // pixel to the background specifier if W(N0) is in force, or write
            // each pixel with the foreground specifier if W(N1)".
            (WriteMode::Erase, _) => match pen.negate {
                Negate::Off => pen.bg.raw(),
                Negate::On => pen.fg.raw(),
            },
        };
        let v = (had & !mask) | (value & mask);
        self.plane[i] = WRITTEN | (v & 0x0f);
        self.touch(x, y);
        true
    }

    /// One pixel of a wide stroke: written as [`RegisCanvas::put`] writes
    /// it unless the stroke has written it already, and marked if written.
    fn stamp(&mut self, x: i32, y: i32, on: bool, pen: &RegisPen) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = y as usize * self.stride + x as usize;
        if self.plane[i] & MARK != 0 {
            return;
        }
        if self.put(x, y, on, pen) {
            self.plane[i] |= MARK;
        }
    }

    /// **The stroke's marks in `r` cleared**: the next stroke writes its
    /// pixels again. Nothing shown changes.
    pub fn unmark(&mut self, r: RegisRect) {
        let (x0, x1) = (r.x0.max(0), r.x1.min(self.w));
        if x0 >= x1 {
            return;
        }
        for y in r.y0.max(0)..r.y1.min(self.h) {
            let at = y as usize * self.stride;
            for b in &mut self.plane[at + x0 as usize..at + x1 as usize] {
                *b &= !MARK;
            }
        }
    }

    /// Whether the line pattern has a one at step `i`, after `W(N)`.
    fn pattern_on(pen: &RegisPen, i: u32) -> bool {
        let on = pen.pattern.bit(i, u32::from(pen.pattern_mul));
        match pen.negate {
            Negate::Off => on,
            Negate::On => !on,
        }
    }

    /// Whether a texture has a one at page pixel `(x, y)`: registered to the
    /// page, "so that patterns of adjacent shaded areas match where the
    /// areas abut" (§ 8.7.2.4.2.3).
    fn texture_on(tex: &Texture, x: i32, y: i32, vertical: bool, pen: &RegisPen) -> bool {
        let on = match *tex {
            Texture::Solid => true,
            Texture::Line(p, m) => {
                let i = if vertical { y } else { x };
                p.bit(i.rem_euclid(1 << 20) as u32, u32::from(m))
            }
            Texture::Cell { rows, side, scale } => {
                let s = i32::from(scale.max(1));
                let side = i32::from(side.max(1));
                let (cx, cy) = ((x / s).rem_euclid(side), (y / s).rem_euclid(side));
                let row = rows[cy as usize];
                row & (1 << (side - 1 - cx)) != 0
            }
        };
        match pen.negate {
            Negate::Off => on,
            Negate::On => !on,
        }
    }

    /// A line from `a` to `b`, the pattern stepping on from `phase`, a
    /// pixel a step whether on the page or not, so a line clipped by the
    /// page keeps its pattern.
    pub fn line(
        &mut self,
        a: RegisPoint,
        b: RegisPoint,
        pen: &RegisPen,
        phase: &mut u32,
        span: LineSpan,
    ) {
        if pen.width > 1 {
            return self.wide_line(a, b, pen, phase, span);
        }
        let (dx, dy) = (
            i64::from(b.x) - i64::from(a.x),
            i64::from(b.y) - i64::from(a.y),
        );
        let steps = dx.abs().max(dy.abs());
        let (k0, k1) = match span {
            LineSpan::Whole => (0, steps),
            LineSpan::Rest | LineSpan::Chained => (1, steps),
            LineSpan::Inner | LineSpan::ChainedInner => (1, steps - 1),
        };
        if k1 < k0 {
            return;
        }
        // The steps whose pixels can be on the page: the rest only advance
        // the pattern.
        let (lo, hi) = clip_steps(a, dx, dy, steps, self.w, self.h);
        let (lo, hi) = (lo.max(k0), hi.min(k1));
        let start = *phase;
        if lo <= hi {
            for k in lo..=hi {
                let (x, y) = at_step(a, dx, dy, steps, k);
                let i = start.wrapping_add((k - k0) as u32);
                if let Some((to, tex)) = pen.shade {
                    self.shade(x, y, to, &tex, pen);
                }
                self.put(x, y, Self::pattern_on(pen, i), pen);
            }
        }
        *phase = start.wrapping_add((k1 - k0 + 1) as u32);
    }

    /// **A wide line** (§ 8.8.3.2): the round pen `pen.width` pixels
    /// across at each step of the one-pixel line, as the pattern has that
    /// step, each pixel written once in the stroke ([`MARK`]); shading from
    /// the steps themselves. Jev's lean, ROUND, 0.25 and 0.19 on a second
    /// ask: "the width or weight of lines and curves" the same in every
    /// direction.
    ///
    /// **A step writes only what the pen last written did not cover.** A
    /// pixel is written at the first step that covers it and writes --
    /// every step for replace and erase writing, a step the pattern has a
    /// one for overlay and complement -- and the mark keeps every later
    /// one off it; so a writing step visits its pen less the pen of the
    /// writing step before, the line before's last for a line that
    /// [`LineSpan::Chained`] says ended where this one starts, with the
    /// same pen. The pixels written are those of the whole pen at every
    /// step, at a cost of the pen's rows on the page a step, and a line's
    /// first writing step with no pen before it -- at most one a line --
    /// visits its whole pen, held to the page, whatever its width.
    fn wide_line(
        &mut self,
        a: RegisPoint,
        b: RegisPoint,
        pen: &RegisPen,
        phase: &mut u32,
        span: LineSpan,
    ) {
        let (dx, dy) = (
            i64::from(b.x) - i64::from(a.x),
            i64::from(b.y) - i64::from(a.y),
        );
        let steps = dx.abs().max(dy.abs());
        let (k0, k1) = match span {
            LineSpan::Whole => (0, steps),
            LineSpan::Rest | LineSpan::Chained => (1, steps),
            LineSpan::Inner | LineSpan::ChainedInner => (1, steps - 1),
        };
        if k1 < k0 {
            return;
        }
        // The steps whose pen can touch the page.
        let r = i32::from(pen.width) / 2 + 1;
        let shifted = RegisPoint {
            x: a.x.saturating_add(r),
            y: a.y.saturating_add(r),
        };
        let (lo, hi) = clip_steps(shifted, dx, dy, steps, self.w + 2 * r, self.h + 2 * r);
        let (lo, hi) = (lo.max(k0), hi.min(k1));
        let start = *phase;
        let writes = |i: u32| match pen.mode {
            WriteMode::Replace | WriteMode::Erase => true,
            WriteMode::Overlay | WriteMode::Complement => Self::pattern_on(pen, i),
        };
        // The pen last written, its centre.
        let mut last = match span {
            LineSpan::Chained | LineSpan::ChainedInner if writes(start.wrapping_sub(1)) => {
                Some((i64::from(a.x), i64::from(a.y)))
            }
            _ => None,
        };
        if lo <= hi {
            for k in lo..=hi {
                let (x, y) = at_step(a, dx, dy, steps, k);
                let i = start.wrapping_add((k - k0) as u32);
                if let Some((to, tex)) = pen.shade {
                    self.shade(x, y, to, &tex, pen);
                }
                if !writes(i) {
                    continue;
                }
                let at = (i64::from(x), i64::from(y));
                self.pen_less(at, last, i, pen);
                last = Some(at);
            }
        }
        *phase = start.wrapping_add((k1 - k0 + 1) as u32);
    }

    /// The round pen at `at`, less the same pen at `last`, each pixel on
    /// the page stamped as the pattern has step `i`.
    fn pen_less(&mut self, at: (i64, i64), last: Option<(i64, i64)>, i: u32, pen: &RegisPen) {
        let (d, on) = (i64::from(pen.width), Self::pattern_on(pen, i));
        let (y0, y1) = (
            (at.1 - d / 2).max(0),
            (at.1 + (d - 1) / 2).min(i64::from(self.h) - 1),
        );
        for y in y0..=y1 {
            let Some((u0, u1)) = pen_row(d, y - at.1) else {
                continue;
            };
            let (n0, n1) = (at.0 + u0, at.0 + u1);
            let old =
                last.and_then(|(lx, ly)| pen_row(d, y - ly).map(|(o0, o1)| (lx + o0, lx + o1)));
            match old {
                Some((o0, o1)) if o0 <= n1 && n0 <= o1 => {
                    self.stamp_span(y, n0, o0 - 1, on, pen);
                    self.stamp_span(y, o1 + 1, n1, on, pen);
                }
                _ => self.stamp_span(y, n0, n1, on, pen),
            }
        }
    }

    /// Row `y`'s pixels `x0..=x1` that are on the page, stamped `on`.
    fn stamp_span(&mut self, y: i64, x0: i64, x1: i64, on: bool, pen: &RegisPen) {
        let (x0, x1) = (x0.max(0), x1.min(i64::from(self.w) - 1));
        for x in x0..=x1 {
            self.stamp(x as i32, y as i32, on, pen);
        }
    }

    /// Shading from one pixel of a path to its reference: "drawing lines
    /// from each distinct point on the drawing path ... to the shading
    /// reference. It is these lines, not the path line, which must take on
    /// the shading pattern" (§ 8.7.2.4.2.3).
    fn shade(&mut self, x: i32, y: i32, to: ShadeTo, tex: &Texture, pen: &RegisPen) {
        match to {
            ShadeTo::Row(r) => {
                let (y0, y1) = (y.min(r).max(0), y.max(r).min(self.h - 1));
                if x < 0 || x >= self.w {
                    return;
                }
                for yy in y0..=y1 {
                    let on = Self::texture_on(tex, x, yy, true, pen);
                    self.put(x, yy, on, pen);
                }
            }
            ShadeTo::Column(c) => {
                let (x0, x1) = (x.min(c).max(0), x.max(c).min(self.w - 1));
                if y < 0 || y >= self.h {
                    return;
                }
                for xx in x0..=x1 {
                    let on = Self::texture_on(tex, xx, y, false, pen);
                    self.put(xx, y, on, pen);
                }
            }
            ShadeTo::Point(p) => {
                let a = RegisPoint { x, y };
                let (dx, dy) = (i64::from(p.x) - i64::from(x), i64::from(p.y) - i64::from(y));
                let steps = dx.abs().max(dy.abs());
                let (lo, hi) = clip_steps(a, dx, dy, steps, self.w, self.h);
                for k in lo..=hi {
                    let (xx, yy) = at_step(a, dx, dy, steps, k);
                    let on = Self::texture_on(tex, xx, yy, false, pen);
                    self.put(xx, yy, on, pen);
                }
            }
        }
    }

    /// A filled area by the even-odd rule: "From any given point not on an
    /// edge of the defined polygon, consider a ray ... If that constructed
    /// ray crosses an odd number of edges, then the specified point is
    /// inside the polygon" (§ 8.5.8.4.1). A pixel is in where its centre is,
    /// the left edge of a span counted in and the right out.
    pub fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen) {
        let (mut ymin, mut ymax) = (i32::MAX, i32::MIN);
        for p in points {
            ymin = ymin.min(p.y);
            ymax = ymax.max(p.y);
        }
        let (ymin, ymax) = (ymin.max(0), ymax.min(self.h - 1));
        let tex = pen.shade.map_or(Texture::Solid, |(_, t)| t);
        let mut xs = [0i64; super::sem::FILL_VERTICES];
        for y in ymin..=ymax {
            let mut n = 0;
            let mut from = 0usize;
            for &end in ends {
                let end = usize::from(end).min(points.len());
                let c = &points[from..end];
                for i in 0..c.len() {
                    let (p, q) = (c[i], c[(i + 1) % c.len()]);
                    if (p.y <= y && y < q.y) || (q.y <= y && y < p.y) {
                        // x where the edge crosses the row, in 1/65536.
                        let num =
                            (i64::from(y) - i64::from(p.y)) * (i64::from(q.x) - i64::from(p.x));
                        let x = (i64::from(p.x) << 16)
                            + (num << 16) / (i64::from(q.y) - i64::from(p.y));
                        if n < xs.len() {
                            xs[n] = x;
                            n += 1;
                        }
                    }
                }
                from = end;
            }
            let xs = &mut xs[..n];
            xs.sort_unstable();
            for pair in xs.chunks_exact(2) {
                let x0 = ((pair[0] + 0xffff) >> 16).max(0);
                let x1 = (((pair[1] + 0xffff) >> 16) - 1).min(i64::from(self.w) - 1);
                for x in x0..=x1 {
                    let x = x as i32;
                    let on = Self::texture_on(&tex, x, y, true, pen);
                    self.put(x, y, on, pen);
                }
            }
        }
    }

    /// `S(E)`: every pixel the background, whatever the mask -- "The screen
    /// erase command erases all planes, regardless of the plane select
    /// control setting" (`[[source: vt3xx-write-control]]`).
    pub fn erase(&mut self, bg: RegisIndex) {
        let v = WRITTEN | bg.raw();
        for y in 0..self.h as usize {
            let row = &mut self.plane[y * self.stride..y * self.stride + self.w as usize];
            row.fill(v);
        }
        self.mark(RegisRect {
            x0: 0,
            y0: 0,
            x1: self.w,
            y1: self.h,
        });
    }

    /// **The page's pixels moved `by`**, every plane -- the
    /// foreground mask "does not affect ... those planes which can be moved
    /// by screen scrolling (S[])" (§ 8.7.2.4.2.1) -- and what the move
    /// reveals written in `bg`: data "moved off and erased from the display
    /// medium" (§ 8.8.1.1), erasing being writing the background (Jev's
    /// lean, BACKGROUND, 0.48 and 0.64 on a second ask). Neither the clip
    /// nor a mask holds it back; the whole page is marked drawn, so the
    /// caller repaints it and recounts what is lit.
    pub fn scroll(&mut self, by: RegisShift, bg: RegisIndex) {
        let (dx, dy) = (by.dx, by.dy);
        let (w, h, stride) = (self.w, self.h, self.stride);
        let fill = WRITTEN | (bg.raw() & 0x0f);
        let one = |plane: &mut [u8], y: i32| {
            let at = y as usize * stride;
            let sy = i64::from(y) - i64::from(dy);
            let n = i64::from(w) - i64::from(dx).abs();
            if sy < 0 || sy >= i64::from(h) || n <= 0 {
                plane[at..at + w as usize].fill(fill);
                return;
            }
            let (n, from) = (n as usize, sy as usize * stride);
            if dx >= 0 {
                let dx = dx as usize;
                plane.copy_within(from..from + n, at + dx);
                plane[at..at + dx].fill(fill);
            } else {
                let dx = dx.unsigned_abs() as usize;
                plane.copy_within(from + dx..from + dx + n, at);
                plane[at + n..at + w as usize].fill(fill);
            }
        };
        // Rows taken from above are written from the bottom, and the other
        // way, so each is read before it is written.
        match dy > 0 {
            true => (0..h).rev().for_each(|y| one(self.plane, y)),
            false => (0..h).for_each(|y| one(self.plane, y)),
        }
        self.mark(RegisRect {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        });
    }

    /// A rectangle back to clear: what text written over the page leaves.
    pub fn clear(&mut self, r: RegisRect) {
        let r = RegisRect {
            x0: r.x0.max(0),
            y0: r.y0.max(0),
            x1: r.x1.min(self.w),
            y1: r.y1.min(self.h),
        };
        if r.x0 >= r.x1 || r.y0 >= r.y1 {
            return;
        }
        for y in r.y0..r.y1 {
            let at = y as usize * self.stride;
            self.plane[at + r.x0 as usize..at + r.x1 as usize].fill(0);
        }
        self.mark(r);
    }

    /// A stored character cell, scaled to `unit` and written over `display`
    /// at `at`, turned by `direction` and slanted by `slant`: the cell's ones
    /// write as the pattern's ones, its zeros and the display cell past the
    /// unit cell as its zeros (§ 8.7.3.5: "any display cell area not filled
    /// by the character image explicitly filled with the background color").
    ///
    /// **Every page pixel the turned cell covers is mapped back into the
    /// cell**, so a turned or slanted character has no holes. The slant
    /// keeps the cell's upper-left corner and leans the rows below it:
    /// "Slant subsequent characters by <ang> degrees, referenced from the
    /// perpendicular to the baseline and measured from the upper left corner
    /// of the character cell. Positive angles cause the characters to appear
    /// to slant backwards" (§ 8.7.3.4.6) -- a row `v` down moves `v tan a`
    /// along the baseline.
    pub fn cell(&mut self, g: &CellDraw<'_>, pen: &RegisPen) {
        let (w, h) = (self.w, self.h);
        cell_pixels(g, w, h, &mut |x, y, bit| {
            let on = match pen.negate {
                Negate::Off => bit,
                Negate::On => !bit,
            };
            self.put(x, y, on, pen);
        });
    }

    /// **The index pixel `(x, y)` of a `w`-pixel page's `plane` holds**,
    /// read through the rows' stride; `None` for a pixel never written,
    /// which is clear, or one off the page.
    pub fn index_at(
        plane: &[u8],
        w: crate::image::ImagePx,
        x: usize,
        y: usize,
    ) -> Option<RegisIndex> {
        if x >= w.index() {
            return None;
        }
        let b = *plane.get(y * RegisCanvas::stride(w) + x)?;
        (b & WRITTEN != 0).then(|| RegisIndex::wrap(i64::from(b & 0x0f)))
    }

    /// The page pixels of `r` of a `w`-pixel page as colours through `map`
    /// into `out`, each the plane's pixel `zoom` shows there: a pixel never
    /// written clear.
    pub fn show(
        plane: &[u8],
        w: crate::image::ImagePx,
        map: &[crate::image::Pixel; REGIS_COLORS],
        r: RegisRect,
        out: &mut [crate::image::Pixel],
        zoom: RegisZoom,
    ) {
        let (stride, w) = (RegisCanvas::stride(w), w.index());
        // Held to the page's width: a column past it is the next row's.
        let x1 = (r.x1.max(0) as usize).min(w);
        for y in r.y0.max(0) as usize..r.y1.max(0) as usize {
            for x in r.x0.max(0) as usize..x1 {
                let (px, py) = zoom.plane_of(x, y);
                let (Some(&b), Some(o)) = (plane.get(py * stride + px), out.get_mut(y * w + x))
                else {
                    continue;
                };
                *o = match b & WRITTEN {
                    0 => crate::image::Pixel::CLEAR,
                    _ => map[usize::from(b & 0x0f)],
                };
            }
        }
    }
}

/// A character's image, sampled into its unit cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellBits<'a> {
    /// A loaded cell's rows, most significant bit leftmost, `w` wide.
    Rows { rows: &'a [u16], w: u8 },
    /// A built-in character the font drew.
    Glyph(GlyphBits<'a>),
    /// The error character, a solid block.
    Solid,
}

impl CellBits<'_> {
    /// The image's width and height in its own bits.
    fn size(&self) -> (i32, i32) {
        match *self {
            CellBits::Rows { rows, w } => (i32::from(w.max(1)), rows.len().max(1) as i32),
            CellBits::Glyph(g) => (i32::from(g.w.max(1)), i32::from(g.h.max(1))),
            CellBits::Solid => (1, 1),
        }
    }

    /// Whether the image has a one at `(x, y)` of its own bits.
    fn bit(&self, x: i32, y: i32) -> bool {
        match *self {
            CellBits::Rows { rows, w } => {
                let row = rows.get(y as usize).copied().unwrap_or(0);
                row & (1 << (i32::from(w.max(1)) - 1 - x)) != 0
            }
            CellBits::Glyph(g) => g.bit(x as usize, y as usize),
            CellBits::Solid => true,
        }
    }
}

/// A character cell to draw, resolved to page pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellDraw<'a> {
    /// The character's image.
    pub bits: CellBits<'a>,
    pub at: RegisPoint,
    pub unit: (i32, i32),
    pub display: (i32, i32),
    pub direction: i32,
    pub slant: i32,
}

/// The pixel of step `k` of `steps` along `(dx, dy)` from `a`, rounded.
/// **Each page pixel of a cell drawn**, `(x, y, bit)`, on a page `w` by
/// `h`: the cell's bit at the pixel's centre, turned and leant as `g`
/// says, before any negate.
pub fn cell_pixels(g: &CellDraw<'_>, w: i32, h: i32, f: &mut dyn FnMut(i32, i32, bool)) {
    let (uw, uh) = (g.unit.0.max(1), g.unit.1.max(1));
    let (dw, dh) = (g.display.0.max(1), g.display.1.max(1));
    let a = i64::from(g.direction) * 65536;
    let (co, si) = (super::trig::cos(a), super::trig::sin(a));
    // tan of the slant, in 2^-30.
    let tan = {
        let s = i64::from(g.slant.clamp(-45, 45)) * 65536;
        (super::trig::sin(s) << 30) / super::trig::cos(s).max(1)
    };
    // Cell to page: lean, then turn counterclockwise as the viewer
    // sees it on a page whose `y` grows downward.
    let to_page = |u: i64, v: i64| -> (i64, i64) {
        let fu = u + ((v * tan) >> 30);
        ((fu * co + v * si) >> 30, (-fu * si + v * co) >> 30)
    };
    let corners = [
        to_page(0, 0),
        to_page(i64::from(dw), 0),
        to_page(0, i64::from(dh)),
        to_page(i64::from(dw), i64::from(dh)),
    ];
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    for (x, y) in corners {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (sw, sh) = g.bits.size();
    for py in y0..=y1 {
        for px in x0..=x1 {
            let (x, y) = (g.at.x + px as i32, g.at.y + py as i32);
            if x < 0 || y < 0 || x >= w || y >= h {
                continue;
            }
            // Page to cell at the pixel's centre: turn back, unlean.
            let (cx, cy) = (2 * px + 1, 2 * py + 1);
            let fu = (cx * co - cy * si) >> 30;
            let v2 = (cx * si + cy * co) >> 30;
            let u2 = fu - ((v2 * tan) >> 30);
            // `u2` and `v2` are twice the cell coordinates.
            if u2 < 0 || v2 < 0 || u2 >= 2 * i64::from(dw) || v2 >= 2 * i64::from(dh) {
                continue;
            }
            let (u, v) = ((u2 / 2) as i32, (v2 / 2) as i32);
            let bit = u < uw && v < uh && g.bits.bit(u * sw / uw, v * sh / uh);
            f(x, y, bit);
        }
    }
}

/// **The round pen's row `dv`**, `d` pixels across: the columns about
/// its centre, first and last, of the pixels whose centres are within
/// `d / 2` of the pen's -- centred on a pixel for an odd `d` and on a
/// pixel's upper-left corner for an even one -- or `None` past the pen.
fn pen_row(d: i64, dv: i64) -> Option<(i64, i64)> {
    let c = (d + 1) % 2;
    if dv < -(d / 2) || dv > (d - 1) / 2 {
        return None;
    }
    // `(2 du + c)^2 + (2 dv + c)^2 <= d^2`, solved for `du`.
    let rem = d * d - (2 * dv + c).pow(2);
    if rem < 0 {
        return None;
    }
    let du1 = (rem.isqrt() - c).div_euclid(2);
    let du0 = -du1 - c;
    (du0 <= du1).then_some((du0, du1))
}

fn at_step(a: RegisPoint, dx: i64, dy: i64, steps: i64, k: i64) -> (i32, i32) {
    if steps == 0 {
        return (a.x, a.y);
    }
    let r = |d: i64| {
        let n = d * k;
        // To the nearest, a half away from zero.
        let q = (2 * n.abs() + steps) / (2 * steps);
        if n < 0 { -q } else { q }
    };
    (
        (i64::from(a.x) + r(dx)) as i32,
        (i64::from(a.y) + r(dy)) as i32,
    )
}

/// The steps of a line whose pixels may be on a `w` by `h` page, a pixel
/// of margin either side; empty as `(1, 0)`.
fn clip_steps(a: RegisPoint, dx: i64, dy: i64, steps: i64, w: i32, h: i32) -> (i64, i64) {
    if steps == 0 {
        return (0, 0);
    }
    let (mut lo, mut hi) = (0i64, steps);
    let mut axis = |p: i64, d: i64, size: i64| {
        // p + d * k / steps within [-1, size]: k * d within
        // [(-1 - p) * steps, (size - p) * steps].
        let (min, max) = ((-1 - p) * steps, (size - p) * steps);
        if d == 0 {
            if p < -1 || p > size {
                lo = 1;
                hi = 0;
            }
            return;
        }
        let (a, b) = if d > 0 { (min, max) } else { (max, min) };
        let k0 = div_ceil(a, d);
        let k1 = div_floor(b, d);
        lo = lo.max(k0);
        hi = hi.min(k1);
    };
    axis(i64::from(a.x), dx, i64::from(w));
    axis(i64::from(a.y), dy, i64::from(h));
    (lo, hi)
}

fn div_floor(a: i64, b: i64) -> i64 {
    let q = a / b;
    if (a % b != 0) && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

fn div_ceil(a: i64, b: i64) -> i64 {
    -div_floor(-a, b)
}
