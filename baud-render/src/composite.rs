//! Pixels: background rows, then masks over them.
//!
//! pixman's shape (`docs/research/notes/j-rendering.md` D2, E4): a solid
//! fill per row, then for each glyph its `A8` mask `OVER` the row with the
//! glyph's colour. Coverage arrives linear from the rasteriser and the
//! blend is a policy (D7): `Native` blends in the sRGB numbers the way
//! every macOS terminal does, `Linear` converts through a lookup table
//! and back, which two of three references found too thin for dark text
//! on light and is here for the side-by-side.

use crate::atlas::{Atlas, Kind};
use crate::color::Argb;
use crate::effect::Vfx;
use crate::instance::Instance;
use crate::renderer::ImageComposite;
use crate::trail::{Pt, Smear};
use crate::units::{Band, Coverage, slots};
use baud_domain::{Chan, Cols, ImageAlpha, ImageGround, ImageView, Pixel};
use baud_font::Px;
use core::simd::prelude::{SimdFloat, f32x4, u32x4};
use std::simd::StdFloat;

/// How coverage mixes a glyph colour into the background.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blend {
    /// In sRGB-encoded numbers, as CoreText and the GPU terminals on macOS.
    #[default]
    Native,
    /// In linear light, through an 8-bit to 12-bit table and back.
    Linear,
}

/// What the page a frame is composited over is: opaque, or the window's
/// glass, a page whose alpha is below one. Chosen once a frame, from the
/// theme's dim, so the per-pixel loops of an opaque page are the ones they
/// were: over an opaque backdrop source-over reduces to the mix
/// [`Blend`] has always done (`glass_reduces_to_over_on_an_opaque_page`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// Every pixel under the frame has alpha 255.
    Opaque,
    /// A pixel under the frame may have alpha below 255.
    Glass,
}

/// The caller's pixel buffer: `width` by `height` pixels of `Argb`, rows
/// `stride` pixels apart. What an `IOSurface`, a `wl_shm` map or an
/// upload heap hands over.
///
/// **Every draw writes only the columns `cols`**, the whole width unless
/// a frame repaints part of a row ([`Target::within`]): a row's fill and
/// every record drawn over it are clipped alike, so the pixels inside are
/// what the whole row's repaint gives and the pixels outside are the
/// frame before's.
pub struct Target<'p> {
    pixels: &'p mut [u32],
    stride: Px,
    width: Px,
    height: Px,
    cols: core::ops::Range<usize>,
}

impl<'p> Target<'p> {
    /// `None` when the buffer cannot hold `height` rows of `stride`.
    pub fn new(pixels: &'p mut [u32], stride: Px, width: Px, height: Px) -> Option<Target<'p>> {
        let (s, w, h) = (slots(stride), slots(width), slots(height));
        (w <= s && h.checked_mul(s)? <= pixels.len()).then_some(Target {
            pixels,
            stride,
            width,
            height,
            cols: 0..w,
        })
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels
    }

    /// **`width` columns from column `x` on, as a target of their own** over
    /// the same rows and stride: the grid beside the strip a frame keeps for
    /// the exit-status marks ([`crate::status`]). `None` past the width.
    pub fn columns(&mut self, x: Px, width: Px) -> Option<Target<'_>> {
        let (s, x0, w, h) = (
            slots(self.stride),
            slots(x),
            slots(width),
            slots(self.height),
        );
        if x0.checked_add(w)? > slots(self.width) {
            return None;
        }
        // The last row's end is the farthest pixel any row of it reaches.
        let need = match h {
            0 => 0,
            _ => (h - 1).checked_mul(s)?.checked_add(x0 + w)?,
        };
        (need <= self.pixels.len()).then(|| Target {
            pixels: &mut self.pixels[x0..],
            stride: self.stride,
            width,
            height: self.height,
            cols: 0..w,
        })
    }

    /// **The columns from `left` to `right` only**, as a target over the
    /// same pixels: what a row repaints when only the records over part of
    /// it changed. Each draw through it writes those columns and no other,
    /// in the same coordinates.
    pub fn within(&mut self, left: Px, right: Px) -> Target<'_> {
        let start = self.cols.start.max(slots(left));
        let end = self.cols.end.min(slots(right)).max(start);
        Target {
            pixels: &mut *self.pixels,
            stride: self.stride,
            width: self.width,
            height: self.height,
            cols: start..end,
        }
    }

    /// The pixels of `xs` inside the columns drawn.
    fn drawn(&self, xs: core::ops::Range<usize>) -> core::ops::Range<usize> {
        let start = xs.start.max(self.cols.start);
        start..xs.end.min(self.cols.end).max(start)
    }

    fn rows_of(&self, band: Band) -> core::ops::Range<usize> {
        let h = slots(self.height);
        slots(band.top).min(h)..slots(band.bottom).min(h)
    }

    /// Fills the pixel rows of `band` from a per-cell background: one
    /// word of `bg` painted `cell_w` wide each, and `margin` past the last
    /// cell -- the scroller's gutter, when the frame has one.
    pub fn fill_band(&mut self, band: Band, bg: &[Argb], cell_w: Px, margin: Argb) {
        let cell_w = slots(cell_w);
        let (stride, width) = (slots(self.stride), slots(self.width));
        let xs = self.cols.clone();
        // Every column: the loop with no clip to take.
        if xs.start == 0 && xs.end == width {
            for y in self.rows_of(band) {
                let row = &mut self.pixels[y * stride..y * stride + width];
                let mut x = 0;
                for &c in bg {
                    let end = (x + cell_w).min(width);
                    if x >= end {
                        break;
                    }
                    row[x..end].fill(c.0);
                    x = end;
                }
                if x < width {
                    row[x..].fill(margin.0);
                }
            }
            return;
        }
        // The first cell the columns reach, and where it starts.
        let first = match cell_w {
            0 => 0,
            w => (xs.start / w).min(bg.len()),
        };
        for y in self.rows_of(band) {
            let row = &mut self.pixels[y * stride..y * stride + width];
            let mut x = first * cell_w;
            for &c in &bg[first..] {
                let end = (x + cell_w).min(xs.end);
                if x >= end {
                    break;
                }
                row[x.max(xs.start)..end].fill(c.0);
                x = end;
            }
            if x < xs.end {
                row[x.max(xs.start)..xs.end].fill(margin.0);
            }
        }
    }

    /// Fills the cells `set` names with their words from `bg`, over the
    /// rows of `band`, as [`Target::fill_band`] fills them -- replacing
    /// what is there -- and leaves every other cell and the margin as they
    /// are.
    pub fn fill_cells(&mut self, band: Band, bg: &[Argb], cell_w: Px, set: impl Fn(usize) -> bool) {
        let cell_w = slots(cell_w);
        let (stride, width) = (slots(self.stride), slots(self.width));
        for y in self.rows_of(band) {
            for (c, w) in bg.iter().enumerate() {
                let xs = self.drawn(c * cell_w..(c + 1) * cell_w);
                if c * cell_w >= self.cols.end {
                    break;
                }
                if set(c) {
                    self.pixels[y * stride..y * stride + width][xs].fill(w.0);
                }
            }
        }
    }

    /// Clears the cells `set` names, of the first `cols`, over the rows
    /// of `band`, to the transparent zero word, and leaves every other cell
    /// and the margin as they are.
    pub fn clear_cells(&mut self, band: Band, cols: Cols, cell_w: Px, set: impl Fn(usize) -> bool) {
        let cell_w = slots(cell_w);
        let (stride, width) = (slots(self.stride), slots(self.width));
        for y in self.rows_of(band) {
            for c in 0..cols.index() {
                let xs = self.drawn(c * cell_w..(c + 1) * cell_w);
                if c * cell_w >= self.cols.end {
                    break;
                }
                if set(c) {
                    self.pixels[y * stride..y * stride + width][xs].fill(0);
                }
            }
        }
    }

    /// Composites the cells `set` names with their words from `bg`, over the
    /// rows of `band`, source-over what is there at each word's alpha
    /// ([`Blend::over_glass`]), and leaves every other cell and the margin
    /// as they are. Over a cleared pixel a word is itself
    /// (`Target::clear_cells`), as the GPU's `One, OneMinusSourceAlpha`
    /// blend over a zero is.
    pub fn over_cells(
        &mut self,
        band: Band,
        bg: &[Argb],
        cell_w: Px,
        blend: Blend,
        set: impl Fn(usize) -> bool,
    ) {
        let cell_w = slots(cell_w);
        let (stride, width) = (slots(self.stride), slots(self.width));
        for y in self.rows_of(band) {
            for (c, &w) in bg.iter().enumerate() {
                let xs = self.drawn(c * cell_w..(c + 1) * cell_w);
                if c * cell_w >= self.cols.end {
                    break;
                }
                if set(c) {
                    for d in &mut self.pixels[y * stride..y * stride + width][xs] {
                        *d = blend.over_glass(w, Argb(*d), Coverage(255)).0;
                    }
                }
            }
        }
    }

    /// Paints `inst` over the pixels, clipped to the buffer and to the
    /// rows of `band`: a mask in the instance's colour, its coverage bent
    /// by the instance's contrast curve, a colour image as it is, or a solid
    /// rectangle. Over a [`Page::Glass`] page by [`Blend::over_glass`].
    ///
    /// Out of line, the dispatch alone: its callers call it as they did
    /// before there was one, and it branches to the loop.
    #[inline(never)]
    pub fn over(&mut self, inst: &Instance, atlas: &Atlas, blend: Blend, page: Page, band: Band) {
        if inst.kind == Kind::Mask && inst.color.a().get() < 0xFF {
            self.over_translucent(inst, atlas, blend, page, band);
        } else {
            self.over_as_is(inst, atlas, blend, page, band);
        }
    }

    /// [`Target::over`] for every instance but a mask in a colour below
    /// opaque. Out of line and apart from the dispatch, so its loops are
    /// the ones `over` ran before there was one.
    #[inline(never)]
    fn over_as_is(&mut self, inst: &Instance, atlas: &Atlas, blend: Blend, page: Page, band: Band) {
        match page {
            Page::Opaque => self.over_by(inst, atlas, band, |s, d, c| blend.over(s, d, c)),
            Page::Glass => self.over_by(inst, atlas, band, |s, d, c| blend.over_glass(s, d, c)),
        }
    }

    /// [`Target::over`] for a mask in a colour below opaque: the source
    /// alpha the coverage scaled by the colour's alpha and rounded,
    /// `baud_domain::premultiply_chan`, as the GPU's `alpha_byte` forms
    /// it, and the colour then taken opaque. [`Blend::over`] truncates
    /// that product, which is exact only when one factor is 255, so the
    /// opaque colours every shipped mask is in keep their loop. Out of
    /// line, so `over`'s callers keep theirs.
    #[inline(never)]
    #[cold]
    fn over_translucent(
        &mut self,
        inst: &Instance,
        atlas: &Atlas,
        blend: Blend,
        page: Page,
        band: Band,
    ) {
        let ca = inst.color.a();
        let opaque = Instance {
            color: inst.color.with_alpha(Chan::from_u8(0xFF)),
            ..*inst
        };
        let cov = move |c: Coverage| {
            Coverage(baud_domain::premultiply_chan(Chan::from_u8(c.0), ca).get())
        };
        match page {
            Page::Opaque => self.over_by(&opaque, atlas, band, |s, d, c| blend.over(s, d, cov(c))),
            Page::Glass => {
                self.over_by(&opaque, atlas, band, |s, d, c| {
                    blend.over_glass(s, d, cov(c))
                });
            }
        }
    }

    /// [`Target::over`]'s loop, one copy for each page's blend.
    #[inline(always)]
    fn over_by(
        &mut self,
        inst: &Instance,
        atlas: &Atlas,
        band: Band,
        mix: impl Fn(Argb, Argb, Coverage) -> Argb,
    ) {
        let (w, h) = (usize::from(inst.w.0), usize::from(inst.h.0));
        let (u, v) = (usize::from(inst.u.0), usize::from(inst.v.0));
        let rows = self.rows_of(band);
        let (stride, width) = (slots(self.stride), slots(self.width));
        // The record's columns inside the columns drawn, once a record, and
        // the screen column the first of them lands on.
        let x0 = i64::from(inst.x.0);
        let at = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        let lo = (at(self.cols.start) - x0).max(0);
        let hi = (at(self.cols.end) - x0).min(at(w));
        let to = |v: i64| usize::try_from(v).unwrap_or(0);
        let mxs = to(lo)..to(hi).max(to(lo));
        let left = to(x0 + lo);
        let color = inst.color;
        let mask_side = slots(Px::of(atlas.side().value()));
        let color_side = slots(Px::of(atlas.color_side().value()));
        for my in 0..h {
            let Ok(y) = usize::try_from(i32::from(inst.y.0) + i32::try_from(my).unwrap_or(0))
            else {
                continue;
            };
            if !rows.contains(&y) {
                continue;
            }
            let row = &mut self.pixels[y * stride..y * stride + width];
            for (mx, x) in mxs.clone().zip(left..) {
                let dst = Argb(row[x]);
                let out = match inst.kind {
                    Kind::Mask => {
                        let cov = atlas.pixels()[(v + my) * mask_side + u + mx];
                        if cov == 0 {
                            continue;
                        }
                        // The text-contrast curve, a lookup: the exact
                        // coverage bent for the colour the glyph is in.
                        mix(color, dst, Coverage(inst.curve.apply(cov)))
                    }
                    Kind::Solid => mix(color, dst, Coverage(255)),
                    // A quad's record is only its bounds; the pixels come
                    // from its corners, which `Target::quad` is handed. An
                    // effect's come from its light, which `Target::effect`
                    // is handed.
                    Kind::Quad | Kind::Band | Kind::Particle => continue,
                    Kind::Color => {
                        let src = Argb(atlas.argb()[(v + my) * color_side + u + mx]);
                        if src.a().get() == 0 {
                            continue;
                        }
                        mix(src, dst, Coverage(255))
                    }
                };
                row[x] = out.0;
            }
        }
    }

    /// Paints the trail's quad over the pixels in `fill`, clipped to its
    /// bounds, the buffer and the rows of `band`: each pixel as much as
    /// [`crate::trail::coverage`] says the quad covers its centre, as a
    /// byte, scaled by the fill's alpha and rounded -- the GPU's
    /// `quad_fragment` forms the same byte.
    pub fn quad(&mut self, s: &Smear, fill: Argb, blend: Blend, page: Page, band: Band) {
        match page {
            Page::Opaque => self.quad_by(s, fill, band, |f, d, c| blend.over(f, d, c)),
            Page::Glass => self.quad_by(s, fill, band, |f, d, c| blend.over_glass(f, d, c)),
        }
    }

    /// [`Target::quad`]'s loop, one copy for each page's blend.
    #[inline(always)]
    fn quad_by(
        &mut self,
        s: &Smear,
        fill: Argb,
        band: Band,
        mix: impl Fn(Argb, Argb, Coverage) -> Argb,
    ) {
        let (x0, y0, x1, y1) = s.bounds();
        let fa = fill.a();
        let fill = fill.with_alpha(Chan::from_u8(0xFF));
        let rows = self.rows_of(band);
        let (stride, width) = (slots(self.stride), slots(self.width));
        let ys = slots(y0).max(rows.start)..slots(y1).min(rows.end);
        let xs = slots(x0).max(self.cols.start)..slots(x1).min(self.cols.end);
        let px = |v: usize| Px::of(u32::try_from(v).unwrap_or(u32::MAX));
        for y in ys {
            // Only the columns the smear can cover on this row
            // (`Smear::columns`): every pixel left out covers nothing.
            let Some((left, right)) = s.columns(px(y), px(y + 1)) else {
                continue;
            };
            let start = slots(left).max(xs.start);
            let xs = start..slots(right).min(xs.end).max(start);
            let row = &mut self.pixels[y * stride..y * stride + width];
            let cy = f32::from(u16::try_from(y).unwrap_or(u16::MAX)) + 0.5;
            for x in xs {
                let cx = f32::from(u16::try_from(x).unwrap_or(u16::MAX)) + 0.5;
                #[cfg(test)]
                crate::det::tally::count(&crate::det::tally::COVERAGES);
                let cover = crate::trail::coverage(s, Pt { x: cx, y: cy });
                if cover <= 0.0 {
                    continue;
                }
                let c = u8::try_from((cover * 255.0).round() as i64).unwrap_or(u8::MAX);
                let a = baud_domain::premultiply_chan(Chan::from_u8(c), fa).get();
                row[x] = mix(fill, Argb(row[x]), Coverage(a)).0;
            }
        }
    }

    /// **Paints an effect's records over the pixels**, each clipped to the
    /// buffer, the columns drawn and the rows of `band`, as the GPU's
    /// viewport and its effect pipeline draw them: a band's pixel at the
    /// effect's light at the pixel's centre ([`Vfx::light`]'s bits, through
    /// [`Vfx::shade`], which leaves out only pixels whose light is zero), a
    /// particle's at its shape where the pixel sits in its record, -1 to 1
    /// on each axis ([`Vfx::particle`]), each times the record's alpha --
    /// which carries a particle's glow and a repeat pass's flash cap -- in
    /// the record's colour, in order, so a particle lands over the band
    /// under it.
    pub fn effect(
        &mut self,
        vfx: &Vfx,
        records: &[Instance],
        blend: Blend,
        page: Page,
        band: Band,
    ) {
        let (left, right) = (self.cols.start, self.cols.end);
        let piece = Piece {
            rows: self.rows_of(band),
            cols: left..right,
        };
        let mut rows = Rows {
            pixels: &mut *self.pixels,
            first: Px::of(0),
            stride: self.stride,
            width: self.width,
        };
        rows.effect(vfx, records, (blend, page), piece);
    }

    /// **[`Target::effect`] over many rows at once, on up to `threads`
    /// threads**: each of `pieces`, a band's rows and the columns drawn on
    /// them, clipped to the buffer and the columns drawn. The pixel rows are
    /// cut into runs of about equal work -- the record pixels over them --
    /// one a thread, none under [`SHARE`]: every pixel is under the same
    /// records in the same order whichever run it falls in, and its light is
    /// its own ([`Vfx::shade`]), so the pixels are the one thread's bit for
    /// bit.
    pub(crate) fn effect_over(
        &mut self,
        vfx: &Vfx,
        records: &[Instance],
        (blend, page): (Blend, Page),
        pieces: &[(Band, Px, Px)],
        threads: core::num::NonZeroUsize,
    ) {
        let pieces: Vec<Piece> = pieces
            .iter()
            .map(|&(band, left, right)| {
                let start = slots(left).max(self.cols.start);
                Piece {
                    rows: self.rows_of(band),
                    cols: start..slots(right).min(self.cols.end).max(start),
                }
            })
            .filter(|p| !p.rows.is_empty() && !p.cols.is_empty())
            .collect();
        let (stride, width) = (self.stride, self.width);
        let row = |y: usize| Px::of(u32::try_from(y).unwrap_or(u32::MAX));
        // One thread draws on this one, with no rows to cut and no scope.
        let cuts = match threads.get() {
            1 => Vec::new(),
            _ => cuts(records, &pieces, self.height, threads),
        };
        if cuts.is_empty() {
            let mut all = Rows {
                pixels: &mut *self.pixels,
                first: Px::of(0),
                stride,
                width,
            };
            for p in pieces {
                all.effect(vfx, records, (blend, page), p);
            }
            return;
        }
        let mut rest = &mut *self.pixels;
        let mut first = 0;
        let mut shares = Vec::with_capacity(cuts.len() + 1);
        for &cut in &cuts {
            let (head, tail) = rest.split_at_mut((cut - first) * slots(stride));
            shares.push(Rows {
                pixels: head,
                first: row(first),
                stride,
                width,
            });
            (rest, first) = (tail, cut);
        }
        shares.push(Rows {
            pixels: rest,
            first: row(first),
            stride,
            width,
        });
        let draw = |mut share: Rows<'_>| {
            let start = slots(share.first);
            let end = start + share.pixels.len().div_ceil(slots(stride).max(1));
            for p in &pieces {
                let rows = p.rows.start.max(start)..p.rows.end.min(end);
                if rows.start < rows.end {
                    let cols = p.cols.clone();
                    share.effect(vfx, records, (blend, page), Piece { rows, cols });
                }
            }
        };
        let last = shares.pop();
        std::thread::scope(|scope| {
            for share in shares {
                scope.spawn(move || draw(share));
            }
            if let Some(share) = last {
                draw(share);
            }
        });
    }

    /// Blits a placement's drawn pixels over the buffer: `blit.at` is the
    /// screen top-left of its visible rectangle, which may be above the top
    /// when an image has scrolled partway off it, and `blit.texels` says
    /// which image pixel each drawn pixel shows. Clipped to the buffer on
    /// every side and to the rows of `blit.band`: the whole frame for an
    /// image over the text, the row being repainted for one under it. A
    /// pixel is composited `blend.over`, exactly as a `Kind::Color` glyph is
    /// (an image is ARGB the same way an `sbix` glyph is), so an image drawn
    /// over glyphs reads over them; a clear pixel draws nothing and lets the
    /// cell underneath show through. A partial pixel is blended by its
    /// straight alpha (`[[source: w3c-png-2003]]` 6.2 stores colour
    /// straight): over an opaque page by `blend.over`, over the glass by
    /// [`Blend::over_glass`], whose straight result the presenter
    /// premultiplies to what the GPU's premultiplied blend stores.
    ///
    /// An opaque-or-clear image needs no page: source-over of an opaque
    /// pixel is that pixel, at alpha one, whatever is under it
    /// (`[[source: w3c-compositing-1]]` 5.1 at `as = 1`). Nearest-neighbour
    /// scaling keeps it so: every drawn pixel is one of the image's.
    ///
    /// **An opaque-or-clear image under `Blend::Native` is stored, not
    /// mixed**: the OVER of an opaque pixel is that pixel (`(255 s + 127) /
    /// 255` is `s`; `an_opaque_pixel_over_anything_is_itself` holds every
    /// channel value to it), which is the loop LLVM made of the blend before
    /// the store held alpha. Sharing one loop with the partial case, it
    /// stopped unswitching on the blend and blended every pixel, 53% more
    /// instructions a frame for a still Sixel.
    ///
    /// **Each loop runs over the clipped rectangle only**, never over the
    /// rest with a skip: a placement's size is the client's (`c=65535`), and
    /// what it costs is the pixels on screen.
    ///
    /// **A Sixel's background fill draws as `ground`**: in an image whose
    /// ground is [`ImageGround::Filled`] each [`Pixel::FILL`] word is taken
    /// as `ground` ([`crate::renderer::filled`]), and in every other image
    /// no word is read as one, through a copy of the loops of its own.
    ///
    /// **A replacing placement** ([`ImageComposite::Replace`]) stores its
    /// words, the loop an opaque-or-clear image takes under `Native`: its
    /// data is opaque, and its fill lands with its own alpha.
    pub fn blit_image(
        &mut self,
        blit: &Blit<'_>,
        view: &ImageView<'_>,
        (ground, composite): (Pixel, ImageComposite),
        blend: Blend,
        page: Page,
    ) {
        let Some(area) = self.clip(blit) else {
            return;
        };
        match (view.ground, composite) {
            (ImageGround::Clear, _) => {
                self.blit_texels(&area, blit, view, |p| p, view.alpha, blend, page);
            }
            // A fill at the window's dim makes the image partial, blended.
            (ImageGround::Filled { .. }, ImageComposite::Over) => self.blit_texels(
                &area,
                blit,
                view,
                move |p| crate::renderer::filled(p, ground),
                match ground.alpha().get() {
                    0 | 0xFF => view.alpha,
                    _ => ImageAlpha::Partial,
                },
                blend,
                page,
            ),
            (ImageGround::Filled { .. }, ImageComposite::Replace) => {
                self.blit_replacing(&area, blit, view, ground);
            }
        }
    }

    /// [`Target::blit_image`] for a replacing placement: each texel, a
    /// fill word taken as `ground`, stored. Out of line, so it leaves the
    /// other loops' caller as it was.
    #[inline(never)]
    fn blit_replacing(
        &mut self,
        area: &Area,
        blit: &Blit<'_>,
        view: &ImageView<'_>,
        ground: Pixel,
    ) {
        let (iw, pixels) = (view.width.index(), view.pixels);
        let cols = area.cols.clone();
        let drawn = move |p: Pixel| crate::renderer::filled(p, ground);
        match blit.texels {
            Texels::Unit { x, y } => {
                let (x, y) = (slots(x), slots(y));
                self.blit_opaque(area, |j: usize| {
                    let from = (y + j) * iw + x;
                    pixels[from + cols.start..from + cols.end]
                        .iter()
                        .map(move |&p| drawn(p))
                });
            }
            Texels::Mapped { xs, ys } => {
                let index = |v: u32| usize::try_from(v).unwrap_or(0);
                self.blit_opaque(area, |j: usize| {
                    let from = index(ys[j]) * iw;
                    xs[cols.clone()]
                        .iter()
                        .map(move |&x| drawn(pixels[from + index(x)]))
                });
            }
        }
    }

    /// [`Target::blit_image`] over `area`, each texel through `drawn`, the
    /// drawn pixels `alpha`.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn blit_texels(
        &mut self,
        area: &Area,
        blit: &Blit<'_>,
        view: &ImageView<'_>,
        drawn: impl Fn(Pixel) -> Pixel + Copy,
        alpha: ImageAlpha,
        blend: Blend,
        page: Page,
    ) {
        let (iw, pixels) = (view.width.index(), view.pixels);
        let cols = area.cols.clone();
        match blit.texels {
            Texels::Unit { x, y } => {
                let (x, y) = (slots(x), slots(y));
                let row = |j: usize| {
                    let from = (y + j) * iw + x;
                    pixels[from + cols.start..from + cols.end]
                        .iter()
                        .map(move |&p| drawn(p))
                };
                self.blit_by(area, row, alpha, blend, page);
            }
            Texels::Mapped { xs, ys } => {
                let index = |v: u32| usize::try_from(v).unwrap_or(0);
                let row = |j: usize| {
                    let from = index(ys[j]) * iw;
                    xs[cols.clone()]
                        .iter()
                        .map(move |&x| drawn(pixels[from + index(x)]))
                };
                self.blit_by(area, row, alpha, blend, page);
            }
        }
    }

    /// The drawn pixels of `blit` that land in the buffer and its band:
    /// `None` when none do.
    fn clip(&self, blit: &Blit<'_>) -> Option<Area> {
        let (x, y) = blit.at;
        let (w, h) = (i64::from(blit.w.value()), i64::from(blit.h.value()));
        let (x, y) = (i64::from(x), i64::from(y));
        let top = i64::from(blit.band.top.value()).min(i64::from(self.height.value()));
        let bottom = i64::from(blit.band.bottom.value()).min(i64::from(self.height.value()));
        let (j0, j1) = ((top - y).max(0), (bottom - y).min(h));
        let (left, right) = (
            i64::try_from(self.cols.start).unwrap_or(i64::MAX),
            i64::try_from(self.cols.end).unwrap_or(i64::MAX),
        );
        let (i0, i1) = ((left - x).max(0), (right - x).min(w));
        let at = |v: i64| usize::try_from(v).ok();
        let px = |v: i64| u32::try_from(v).ok().map(Px::of);
        if j0 >= j1 || i0 >= i1 {
            return None;
        }
        Some(Area {
            cols: at(i0)?..at(i1)?,
            rows: at(j0)?..at(j1)?,
            x: px(x + i0)?,
            y: px(y + j0)?,
        })
    }

    /// [`Target::blit_image`]'s dispatch, one copy for each way texels are
    /// fetched: `row(j)` is drawn row `j`'s texels, across the area's
    /// columns.
    #[inline(always)]
    fn blit_by<I: Iterator<Item = Pixel>>(
        &mut self,
        area: &Area,
        row: impl Fn(usize) -> I,
        alpha: ImageAlpha,
        blend: Blend,
        page: Page,
    ) {
        match (alpha, blend, page) {
            (ImageAlpha::OpaqueOrClear, Blend::Native, _) => self.blit_opaque(area, row),
            (ImageAlpha::OpaqueOrClear, Blend::Linear, _) => self.blit_opaque_linear(area, row),
            (ImageAlpha::Partial, _, Page::Opaque) => {
                self.blit_blended(area, row, |s, d| blend.over(s, d, Coverage(255)));
            }
            (ImageAlpha::Partial, _, Page::Glass) => {
                self.blit_blended(area, row, |s, d| blend.over_glass(s, d, Coverage(255)));
            }
        }
    }

    /// The buffer's pixels each drawn row of `area` lands on, with the row.
    #[inline(always)]
    fn area_rows<'s>(
        &'s mut self,
        area: &Area,
    ) -> impl Iterator<Item = (usize, &'s mut [u32])> + use<'s> {
        let stride = slots(self.stride);
        let (x, y, n) = (slots(area.x), slots(area.y), area.cols.len());
        self.pixels[y * stride..]
            .chunks_mut(stride)
            .zip(area.rows.clone())
            .map(move |(line, j)| (j, &mut line[x..x + n]))
    }

    /// [`Target::blit_image`]'s opaque-or-clear `Linear` loop: each drawn
    /// pixel taken opaque and mixed at a constant blend, the shape the loop
    /// had before the store held alpha. Through `blit_blended`, reading each
    /// pixel's alpha, it cost 68% more instructions a frame.
    fn blit_opaque_linear<I: Iterator<Item = Pixel>>(
        &mut self,
        area: &Area,
        row: impl Fn(usize) -> I,
    ) {
        for (j, line) in self.area_rows(area) {
            for (d, p) in line.iter_mut().zip(row(j)) {
                if p.alpha().get() == 0 {
                    continue;
                }
                let (r, g, b) = p.rgb();
                *d = Blend::Linear
                    .over(Argb::rgb(r, g, b), Argb(*d), Coverage(255))
                    .0;
            }
        }
    }

    /// [`Target::blit_image`]'s opaque-or-clear `Native` loop: each drawn
    /// pixel stored as it is.
    fn blit_opaque<I: Iterator<Item = Pixel>>(&mut self, area: &Area, row: impl Fn(usize) -> I) {
        for (j, line) in self.area_rows(area) {
            for (d, p) in line.iter_mut().zip(row(j)) {
                if p.alpha().get() == 0 {
                    continue;
                }
                *d = p.bits();
            }
        }
    }

    /// [`Target::blit_image`]'s loop for a partial image: each drawn pixel
    /// mixed by its alpha, `mix` the page's blend. Out of line, so it leaves
    /// the opaque loops' caller as it was.
    #[inline(never)]
    fn blit_blended<I: Iterator<Item = Pixel>>(
        &mut self,
        area: &Area,
        row: impl Fn(usize) -> I,
        mix: impl Fn(Argb, Argb) -> Argb,
    ) {
        for (j, line) in self.area_rows(area) {
            for (d, p) in line.iter_mut().zip(row(j)) {
                if p.alpha().get() == 0 {
                    continue;
                }
                *d = mix(Argb(p.bits()), Argb(*d)).0;
            }
        }
    }
}

/// **The record pixels a thread is worth starting for**: the plasma's
/// band pixels cost about 7 ns each on the CPU path (note AJ section 19's
/// 6K worst frame, 18.85-19.15 ms, less the 4.0 ms with the bands shading
/// nothing, over 2,158,300 band pixels), a thread's start and join 9 to 64
/// us on that machine (best and mean of 2,000 scopes of one; note AJ
/// section 22), so a share under this, about 0.45 ms of light, is drawn on
/// a thread already running.
pub(crate) const SHARE: u64 = 1 << 16;

/// The rows of a band an effect draws and the columns drawn on them.
struct Piece {
    rows: core::ops::Range<usize>,
    cols: core::ops::Range<usize>,
}

/// **Some whole pixel rows of the buffer**, from row `first` on: one
/// thread's share of [`Target::effect_over`].
struct Rows<'p> {
    pixels: &'p mut [u32],
    first: Px,
    stride: Px,
    width: Px,
}

/// **Where [`Target::effect_over`] cuts the pixel rows**, ascending, at
/// most `threads - 1` cuts: the rows where the record pixels over the
/// pieces, taken top to bottom, pass each equal share of their total --
/// none when the total is under two [`SHARE`]s.
fn cuts(
    records: &[Instance],
    pieces: &[Piece],
    height: Px,
    threads: core::num::NonZeroUsize,
) -> Vec<usize> {
    let mut work = vec![0u64; slots(height)];
    for p in pieces {
        for r in records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Band | Kind::Particle))
        {
            let (x0, y0) = (i64::from(r.x.0), i64::from(r.y.0));
            let span = |a: i64, b: i64, lo: usize, hi: usize| {
                let lo = i64::try_from(lo).unwrap_or(i64::MAX);
                let hi = i64::try_from(hi).unwrap_or(i64::MAX);
                (a.max(lo), b.min(hi))
            };
            let (c0, c1) = span(x0, x0 + i64::from(r.w.0), p.cols.start, p.cols.end);
            let (r0, r1) = span(y0, y0 + i64::from(r.h.0), p.rows.start, p.rows.end);
            if c0 >= c1 || r0 >= r1 {
                continue;
            }
            let w = u64::try_from(c1 - c0).unwrap_or(0);
            let ys = usize::try_from(r0).unwrap_or(0)..usize::try_from(r1).unwrap_or(0);
            for row in &mut work[ys] {
                *row += w;
            }
        }
    }
    let total: u64 = work.iter().sum();
    let n = u64::try_from(threads.get())
        .unwrap_or(1)
        .min(total / SHARE)
        .max(1);
    let mut out = Vec::new();
    let mut seen = 0u64;
    for (y, &w) in work.iter().enumerate() {
        let next = u64::try_from(out.len() + 1).unwrap_or(u64::MAX);
        if next >= n {
            break;
        }
        if seen >= total * next / n && y > out.last().copied().unwrap_or(0) {
            out.push(y);
        }
        seen += w;
    }
    out
}

impl Rows<'_> {
    /// [`Target::effect`] over `piece`, its rows within this share's.
    fn effect(
        &mut self,
        vfx: &Vfx,
        records: &[Instance],
        (blend, page): (Blend, Page),
        piece: Piece,
    ) {
        let (stride, width, first) = (slots(self.stride), slots(self.width), slots(self.first));
        let (rows, cols) = (piece.rows, piece.cols);
        let at = |v: i16| usize::try_from(v.max(0)).unwrap_or(0);
        for r in records
            .iter()
            .filter(|r| matches!(r.kind, Kind::Band | Kind::Particle))
        {
            let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
            let (w, h) = (f32::from(r.w.0), f32::from(r.h.0));
            let ys = at(r.y.0).max(rows.start)
                ..usize::try_from(y0 + i32::from(r.h.0))
                    .unwrap_or(0)
                    .min(rows.end);
            let xs = at(r.x.0).max(cols.start)
                ..usize::try_from(x0 + i32::from(r.w.0))
                    .unwrap_or(0)
                    .min(cols.end)
                    .min(width);
            let alpha = f32::from(r.color.a().get()) / 255.0;
            let color = r.color.with_alpha(Chan::from_u8(0xFF));
            if r.kind == Kind::Band {
                // The last pixel's blend, by its opacity's bits and the word
                // under it: beside a level edge one light runs along a row
                // over one ground.
                let pixels = &mut *self.pixels;
                let mut kept: Option<(u32, u32, u32)> = None;
                vfx.shade(xs, ys, |x, y, v| {
                    let a = alpha * v;
                    if a > 0.0 {
                        let i = (y - first) * stride + x;
                        let (key, under) = (a.to_bits(), pixels[i]);
                        pixels[i] = match kept {
                            Some((k, u, out)) if k == key && u == under => out,
                            _ => {
                                let out = blend.over_unit(color, Argb(under), a, page).0;
                                kept = Some((key, under, out));
                                out
                            }
                        };
                    }
                });
                continue;
            }
            for y in ys {
                let at = (y - first) * stride;
                let row = &mut self.pixels[at..at + width];
                let cy = f32::from(u16::try_from(y).unwrap_or(u16::MAX)) + 0.5;
                for x in xs.clone() {
                    let cx = f32::from(u16::try_from(x).unwrap_or(u16::MAX)) + 0.5;
                    let v = vfx.particle(Pt {
                        x: (cx - x0 as f32) / w * 2.0 - 1.0,
                        y: (cy - y0 as f32) / h * 2.0 - 1.0,
                    });
                    let a = alpha * v;
                    if a > 0.0 {
                        row[x] = blend.over_unit(color, Argb(row[x]), a, page).0;
                    }
                }
            }
        }
    }
}

/// **One placement's blit**: where its visible rectangle's top-left lands
/// on screen (signed: above the top while it scrolls off), its size in
/// device pixels, which image pixel each drawn pixel shows, and the rows
/// it may write.
pub struct Blit<'m> {
    pub at: (i32, i32),
    pub w: Px,
    pub h: Px,
    pub texels: Texels<'m>,
    pub band: Band,
}

/// **Which image pixel a drawn pixel shows.** Drawn pixel `(i, j)` of a
/// placement's visible rectangle shows texel `(x + i, y + j)` when it is
/// drawn one image pixel a device pixel, and `(xs[i], ys[j])` otherwise --
/// the tables `baud_domain::ImageDraw::nearest` filled, which the GPU
/// paths read too, so the CPU and the GPU sample one way.
#[derive(Clone, Copy)]
pub enum Texels<'m> {
    Unit { x: Px, y: Px },
    Mapped { xs: &'m [u32], ys: &'m [u32] },
}

/// The part of a blit inside the buffer and its band: the drawn columns
/// and rows, by their index in the visible rectangle, and the buffer pixel
/// the first of each lands on.
struct Area {
    cols: core::ops::Range<usize>,
    rows: core::ops::Range<usize>,
    x: Px,
    y: Px,
}

impl Blend {
    /// `src` over `dst` with `cov` scaling the source alpha. The result's
    /// alpha is the union.
    fn over(self, src: Argb, dst: Argb, cov: Coverage) -> Argb {
        let a = u32::from(cov.0) * u32::from(src.a().get()) / 255;
        let mix = |s: Chan, d: Chan| -> Chan {
            let v = match self {
                Blend::Native => {
                    (u32::from(s.get()) * a + u32::from(d.get()) * (255 - a) + 127) / 255
                }
                Blend::Linear => {
                    let t = luts();
                    let (ls, ld) = (
                        u32::from(t.to_linear[s.index()]),
                        u32::from(t.to_linear[d.index()]),
                    );
                    let l = (ls * a + ld * (255 - a) + 127) / 255;
                    u32::from(t.to_srgb[usize::try_from(l).unwrap_or(0).min(4095)])
                }
            };
            Chan::from_u8(u8::try_from(v).unwrap_or(255))
        };
        let alpha = u8::try_from(a + u32::from(dst.a().get()) * (255 - a) / 255).unwrap_or(255);
        Argb::rgb(
            mix(src.r(), dst.r()),
            mix(src.g(), dst.g()),
            mix(src.b(), dst.b()),
        )
        .with_alpha(Chan::from_u8(alpha))
    }

    /// **`src`'s colour over `dst` at the source alpha `a`, a fraction**:
    /// an effect's pixel, whose alpha is its light times its record's, a
    /// float as the GPU's fragment function forms it. `Native` blends as
    /// the GPU's `One, OneMinusSourceAlpha` does over its 8-bit
    /// premultiplied target, `co = Cs a + cb (1 - a)` and `ao = a + ab (1 -
    /// a)` rounded to bytes -- `over` in `baud-present-macos/tests/cells.rs`
    /// -- over the backdrop as the presenter premultiplies it, and keeps the
    /// straight byte that premultiplies back to `co`, as [`Blend::over_glass`]
    /// does; over an opaque page that is `co` itself. `Linear`, which has no
    /// GPU counterpart, takes `a` as a coverage byte through `over`.
    fn over_unit(self, src: Argb, dst: Argb, a: f32, page: Page) -> Argb {
        let byte =
            |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap_or(0);
        match self {
            Blend::Native => {
                // Each channel's `co` and the alpha's `ao` in a lane of one
                // vector, by the same operations a lane: `Cs a + pb (1 -
                // a)` for a channel, and `1 a + ab (1 - a)` for the alpha,
                // `1 a` being `a` exactly; then each rounded to a byte as
                // `byte` rounds, ties away from zero, a NaN to 0.
                let ab = dst.a();
                let unit = |c: Chan| UNIT[c.index()];
                let pb = |d: Chan| unit(baud_domain::premultiply_chan(d, ab));
                let s = f32x4::from_array([unit(src.r()), unit(src.g()), unit(src.b()), 1.0]);
                let b = f32x4::from_array([pb(dst.r()), pb(dst.g()), pb(dst.b()), unit(ab)]);
                let v = s * f32x4::splat(a) + b * f32x4::splat(1.0 - a);
                let bytes: u32x4 = (v * f32x4::splat(255.0))
                    .round()
                    .simd_clamp(f32x4::splat(0.0), f32x4::splat(255.0))
                    .cast();
                let [r, g, bl, ao] = bytes.to_array();
                if ao == 0 {
                    return Argb(0);
                }
                let m = u64::from(RECIP[usize::try_from(ao & 0xFF).unwrap_or(0)]);
                let mix = |co: u32| -> Chan {
                    let v = (u64::from(co * 255 + ao / 2) * m) >> 24;
                    Chan::from_u8(u8::try_from(v).unwrap_or(255))
                };
                Argb::rgb(mix(r), mix(g), mix(bl))
                    .with_alpha(Chan::from_u8(u8::try_from(ao).unwrap_or(255)))
            }
            Blend::Linear => {
                let cov = Coverage(u8::try_from(byte(a)).unwrap_or(255));
                match page {
                    Page::Opaque => self.over(src, dst, cov),
                    Page::Glass => self.over_glass(src, dst, cov),
                }
            }
        }
    }

    /// `src` over `dst` with `cov` scaling the source alpha, where `dst`
    /// may be translucent: source-over as W3C Compositing and Blending
    /// Level 1 defines it, `co = Cs as + Cb ab (1 - as)` premultiplied and
    /// `ao = as + ab (1 - as)`, its colour `Co = co / ao` straight (5.1,
    /// 9.1.4, `[[source: w3c-compositing-1]]`).
    ///
    /// **`Native` rounds where the GPU rounds.** The target holds straight
    /// pixels and the presenter premultiplies them, `(C a + 127) / 255`
    /// (`baud_domain::premultiply`, the function both presenters call); the
    /// GPU stores 8-bit premultiplied pixels and blends `One,
    /// OneMinusSourceAlpha` over them. So `co` is formed from the backdrop as
    /// the presenter would premultiply it, rounded to a byte as the GPU's
    /// store is, and the straight byte kept is the nearest to `co / ao`,
    /// which premultiplies back to `co` exactly
    /// (`a_straight_byte_presents_as_its_premultiplied_byte`). The alpha
    /// rounds too: [`Blend::over`]'s truncation is exact only at `ab = 255`.
    ///
    /// **`Linear` is the same formula in linear light**, `Co` taken before
    /// the conversion back, divided by `ao` unrounded. It has no GPU
    /// counterpart and no app path selects it, so it is held to the formula
    /// and to `over` on an opaque page.
    fn over_glass(self, src: Argb, dst: Argb, cov: Coverage) -> Argb {
        let a = u32::from(cov.0) * u32::from(src.a().get()) / 255;
        if a == 0 {
            return dst;
        }
        let ab = u32::from(dst.a().get());
        // At least `a`, so never zero here.
        let ao = (a * 255 + ab * (255 - a) + 127) / 255;
        let alpha = Chan::from_u8(u8::try_from(ao).unwrap_or(255));
        match self {
            Blend::Native => {
                // A whole source pixel is itself: `ao` is 255 and `co` is
                // `Cs` -- a glyph's interior and a solid's every pixel.
                if a == 255 {
                    return src;
                }
                let m = u64::from(RECIP[usize::try_from(ao & 0xFF).unwrap_or(0)]);
                let mix = |s: Chan, d: Chan| -> Chan {
                    // The backdrop as the presenter premultiplies it.
                    let pb = u32::from(baud_domain::premultiply_chan(d, dst.a()).get());
                    let co = (u32::from(s.get()) * a + pb * (255 - a) + 127) / 255;
                    let v = (u64::from(co * 255 + ao / 2) * m) >> 24;
                    Chan::from_u8(u8::try_from(v).unwrap_or(255))
                };
                Argb::rgb(
                    mix(src.r(), dst.r()),
                    mix(src.g(), dst.g()),
                    mix(src.b(), dst.b()),
                )
                .with_alpha(alpha)
            }
            Blend::Linear => {
                let t = luts();
                // `255 ao` before its rounding: the divisor `Native` takes
                // rounded is the one its presenter multiplies by.
                let den = a * 255 + ab * (255 - a);
                let mix = |s: Chan, d: Chan| -> Chan {
                    let (ls, ld) = (
                        u32::from(t.to_linear[s.index()]),
                        u32::from(t.to_linear[d.index()]),
                    );
                    let l = (ls * a * 255 + ld * ab * (255 - a) + den / 2) / den;
                    Chan::from_u8(t.to_srgb[usize::try_from(l).unwrap_or(0).min(4095)])
                };
                Argb::rgb(
                    mix(src.r(), dst.r()),
                    mix(src.g(), dst.g()),
                    mix(src.b(), dst.b()),
                )
                .with_alpha(alpha)
            }
        }
    }
}

/// `UNIT[c]` is `c / 255` as an `f32`, a byte's fraction, built at compile
/// time by the division the blend took a channel: three channels, the
/// backdrop's three and its alpha were seven divisions an effect pixel.
/// `the_unit_table_is_the_division` holds its bits to the division's.
const UNIT: [f32; 256] = {
    let mut t = [0.0f32; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = i as f32 / 255.0;
        i += 1;
    }
    t
};

/// `RECIP[d]` is `ceil(2^24 / d)`: `(n * RECIP[d]) >> 24` is `n / d` for
/// every `n` below `2^16` and `d` from 1 to 255, since `n` times the
/// rounding error, at most `d - 1`, stays under `2^24`. What
/// [`Blend::over_glass`] divides by the alpha with, three channels a pixel;
/// `a_straight_byte_presents_as_its_premultiplied_byte` runs every
/// numerator it is handed.
const RECIP: [u32; 256] = {
    let mut t = [0u32; 256];
    let (mut i, mut d) = (1usize, 1u32);
    while i < 256 {
        t[i] = (1u32 << 24).div_ceil(d);
        i += 1;
        d += 1;
    }
    t
};

/// The two transfer tables, built once: sRGB byte to 12-bit linear, and
/// 12-bit linear back to a byte.
struct Luts {
    to_linear: [u16; 256],
    to_srgb: [u8; 4096],
}

fn luts() -> &'static Luts {
    static LUTS: std::sync::OnceLock<Luts> = std::sync::OnceLock::new();
    LUTS.get_or_init(|| {
        let mut to_linear = [0u16; 256];
        for (i, slot) in to_linear.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            let l = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            *slot = u16::try_from((l * 4095.0).round() as i64).unwrap_or(4095);
        }
        let mut to_srgb = [0u8; 4096];
        for (i, slot) in to_srgb.iter_mut().enumerate() {
            let l = i as f32 / 4095.0;
            let c = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            *slot = u8::try_from((c * 255.0).round().clamp(0.0, 255.0) as i64).unwrap_or(255);
        }
        Luts { to_linear, to_srgb }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A jump's smear is measured only near its shape**: a quad and a
    /// sweep from the top-left cell of a 612 by 340 frame to the
    /// bottom-right, a frame and two frames in at 60 Hz, when their bounds
    /// are most of the frame, measure the coverage at no more pixels than it
    /// is above zero at and eight a row -- where measuring the bounds took
    /// 6.4 to 7.6 times the pixels covered (note AJ section 24). The
    /// positive control: each smear covers pixels.
    #[test]
    fn a_jump_measures_the_smear_only_near_its_shape() {
        use crate::trail::{Rect, Segment, Shape};
        let block = |col: f32, row: f32| Rect {
            left: col * 17.0,
            top: row * 34.0,
            right: (col + 1.0) * 17.0,
            bottom: (row + 1.0) * 34.0,
        };
        let (from, to) = (block(0.0, 0.0), block(35.0, 9.0));
        let (w, h) = (612u32, 340u32);
        let band = Band {
            top: Px::of(0),
            bottom: Px::of(h),
        };
        for shape in [Shape::Quad, Shape::Sweep] {
            for t in [1.0f32 / 60.0, 2.0 / 60.0] {
                let seg = match shape {
                    Shape::Quad => Segment::new(from.corners(), to),
                    Shape::Sweep => Segment::sweep(from.corners(), to),
                };
                let s = Smear {
                    corners: seg.at(t),
                    hole: to,
                    shape,
                    strength: crate::flash::Strength::Full,
                };
                let (_, y0, _, y1) = s.bounds();
                let mut covered = 0u64;
                for y in 0..h {
                    for x in 0..w {
                        let p = Pt {
                            x: f32::from(u16::try_from(x).unwrap()) + 0.5,
                            y: f32::from(u16::try_from(y).unwrap()) + 0.5,
                        };
                        covered += u64::from(crate::trail::coverage(&s, p) > 0.0);
                    }
                }
                let mut px = vec![0xFF10_1010u32; slots(Px::of(w * h))];
                let mut target = Target::new(&mut px, Px::of(w), Px::of(w), Px::of(h)).unwrap();
                let measured = crate::det::tally::counted(&crate::det::tally::COVERAGES, || {
                    target.quad(&s, Argb(0xFFE0_E0E0), Blend::Native, Page::Opaque, band);
                });
                let rows = u64::from(y1.value().min(h) - y0.value());
                assert!(covered > 1_000, "the positive control: {shape:?} at {t}");
                assert!(
                    measured <= covered + 8 * rows,
                    "{shape:?} at {t}: measured {measured} pixels, {covered} covered over {rows} rows"
                );
            }
        }
    }

    /// **A record clipped to the columns drawn is each pixel's own test**:
    /// a quadrant block's mask, whose columns differ, and a solid, at every
    /// left edge from 12 pixels off the buffer's left to past its right,
    /// over the whole width and over windows of columns inside it, at its
    /// edges and empty, through [`Target::over`] on both pages -- each
    /// pixel the one a record pixel at the buffer, the band's rows and the
    /// columns drawn gives, and every other pixel the ground. The control:
    /// the record drew.
    #[test]
    fn a_record_clipped_to_the_columns_drawn_is_each_pixels_own_test() {
        use crate::instance::Screen;
        use baud_domain::Codepoint;
        use baud_domain::contrast::CurveRow;
        let mut atlas = Atlas::new();
        let cell = crate::boxes::Cell {
            w: Px::of(9),
            h: Px::of(18),
        };
        let slot = atlas
            .synthetic(Codepoint::new(0x259A).unwrap(), cell, Px::of(14))
            .expect("a block element's mask");
        let (w, h) = (40u32, 24u32);
        let ground: Vec<u32> = (0..w * h)
            .map(|i| 0xFF00_0000 | ((i * 0x0001_0203) & 0x00FF_FFFF))
            .collect();
        let band = Band {
            top: Px::of(2),
            bottom: Px::of(20),
        };
        let side = slots(Px::of(atlas.side().value()));
        let mut drew = false;
        for kind in [Kind::Mask, Kind::Solid] {
            for page in [Page::Opaque, Page::Glass] {
                for x in -12i16..44 {
                    for (left, right) in [(0, u32::MAX), (5, 17), (0, 3), (31, 40), (11, 11)] {
                        let inst = Instance {
                            x: Screen(x),
                            y: Screen(1),
                            w: slot.w,
                            h: slot.h,
                            u: slot.x,
                            v: slot.y,
                            color: Argb(0xFF20_80F0),
                            kind,
                            curve: CurveRow::default(),
                        };
                        let mut got = ground.clone();
                        {
                            let mut t =
                                Target::new(&mut got, Px::of(w), Px::of(w), Px::of(h)).unwrap();
                            let mut t = t.within(Px::of(left), Px::of(right));
                            t.over(&inst, &atlas, Blend::Native, page, band);
                        }
                        let mut want = ground.clone();
                        let cols = slots(Px::of(left))..slots(Px::of(right.min(w)));
                        for my in 0..usize::from(slot.h.0) {
                            for mx in 0..usize::from(slot.w.0) {
                                let px = i64::from(x) + i64::try_from(mx).unwrap();
                                let py = 1 + my;
                                let Ok(px) = usize::try_from(px) else {
                                    continue;
                                };
                                if !cols.contains(&px) || !(2..20).contains(&py) {
                                    continue;
                                }
                                let cov = match kind {
                                    Kind::Mask => atlas.pixels()[(usize::from(slot.y.0) + my)
                                        * side
                                        + usize::from(slot.x.0)
                                        + mx],
                                    _ => 255,
                                };
                                if cov == 0 {
                                    continue;
                                }
                                let i = py * slots(Px::of(w)) + px;
                                let (src, dst) = (inst.color, Argb(want[i]));
                                let c = Coverage(match kind {
                                    Kind::Mask => inst.curve.apply(cov),
                                    _ => cov,
                                });
                                want[i] = match page {
                                    Page::Opaque => Blend::Native.over(src, dst, c),
                                    Page::Glass => Blend::Native.over_glass(src, dst, c),
                                }
                                .0;
                            }
                        }
                        drew |= want != ground;
                        let off = got.iter().zip(&want).filter(|(a, b)| a != b).count();
                        assert_eq!(off, 0, "{kind:?} {page:?} at {x}, columns {left}..{right}");
                    }
                }
            }
        }
        assert!(drew, "the control: the record drew");
    }

    /// **The effect on any number of threads is the one thread's, bit for
    /// bit**: the splash's plasma at a 2x frame (200 by 50 cells at 28 px,
    /// 3400 by 1850), at its first frame, through the emerge and the sweep,
    /// over a ground of every cell row's own colour, drawn by
    /// [`Target::effect_over`] on 1, 2, 3, 7 and 18 threads, each the
    /// pixels a cell row at a time by [`Target::effect`] give -- over the
    /// whole width and over a few columns of it, as a row repainted in part
    /// draws it. The control: at 7 threads the rows are cut once a
    /// [`SHARE`] of record pixels, 6 times on the first frame, each cut
    /// inside the records' rows; at 1 not at all.
    #[test]
    fn the_effect_on_any_number_of_threads_is_one_threads() {
        use crate::splash;
        let (w, h) = (Px::of(3400), Px::of(1850));
        let cell = 56;
        let frame = (3400.0, 1850.0);
        let scale = splash::scale(frame);
        let at = splash::origin(frame, scale);
        let ground: Vec<u32> = (0..1850 * 3400u32)
            .map(|i: u32| 0xFF00_0000 | (i / 3400 / cell * 0x0003_0507))
            .collect();
        let bands: Vec<Band> = (0..1850u32.div_ceil(56))
            .map(|r| Band {
                top: Px::of(r * 56),
                bottom: Px::of((r * 56 + 56).min(1850)),
            })
            .collect();
        for age in [0.0, splash::OUT, splash::OUT + splash::HOLD, splash::LEFT] {
            let vfx = splash::vfx(at, scale, age);
            let mut records = Vec::new();
            vfx.instances((w, h), Argb(0xFFF4_A32C), &mut records);
            splash::paint(&mut records, at, scale, splash::DARK);
            for (left, right) in [(Px::of(0), Px::of(u32::MAX)), (Px::of(1500), Px::of(1900))] {
                let mut want = ground.clone();
                {
                    let mut t = Target::new(&mut want, w, w, h).unwrap();
                    let mut t = t.within(left, right);
                    for &band in &bands {
                        t.effect(&vfx, &records, Blend::Native, Page::Opaque, band);
                    }
                }
                let pieces: Vec<(Band, Px, Px)> = bands.iter().map(|&b| (b, left, right)).collect();
                for n in [1usize, 2, 3, 7, 18] {
                    let mut got = ground.clone();
                    let mut t = Target::new(&mut got, w, w, h).unwrap();
                    t.effect_over(
                        &vfx,
                        &records,
                        (Blend::Native, Page::Opaque),
                        &pieces,
                        core::num::NonZeroUsize::new(n).unwrap(),
                    );
                    let off = got.iter().zip(&want).filter(|(a, b)| a != b).count();
                    assert_eq!(off, 0, "{age} s, {n} threads, columns {left:?}..{right:?}");
                }
                assert!(want != ground, "{age} s: the control, the effect drew");
            }
            let pieces: Vec<Piece> = bands
                .iter()
                .map(|b| Piece {
                    rows: slots(b.top)..slots(b.bottom),
                    cols: 0..3400,
                })
                .collect();
            let top = records
                .iter()
                .map(|r| usize::try_from(r.y.0).unwrap())
                .min()
                .unwrap();
            let bottom = records
                .iter()
                .map(|r| usize::try_from(i32::from(r.y.0) + i32::from(r.h.0)).unwrap())
                .max()
                .unwrap();
            // A thread a [`SHARE`] of record pixels, at most seven.
            let total: u64 = records
                .iter()
                .filter(|r| matches!(r.kind, Kind::Band | Kind::Particle))
                .map(|r| u64::from(r.w.0) * u64::from(r.h.0))
                .sum();
            let n = |k: usize| core::num::NonZeroUsize::new(k).unwrap();
            let seven = cuts(&records, &pieces, h, n(7));
            let want = (total / SHARE).clamp(1, 7) - 1;
            assert_eq!(
                u64::try_from(seven.len()).unwrap(),
                want,
                "{age} s: {seven:?}"
            );
            assert!(age > 0.0 || want == 6, "the first frame is worth seven");
            assert!(
                seven.windows(2).all(|c| c[0] < c[1])
                    && seven.iter().all(|&c| top < c && c < bottom),
                "{age} s: {seven:?} not inside {top}..{bottom}"
            );
            assert!(cuts(&records, &pieces, h, n(1)).is_empty());
        }
    }

    /// **Over an opaque backdrop the glass blend is the opaque one**, bit
    /// for bit, under both blends: what lets `paint` choose the page once a
    /// frame and leave an opaque page's loops as they were. Every source and
    /// backdrop channel value at every source alpha.
    #[test]
    fn glass_reduces_to_over_on_an_opaque_page() {
        for blend in [Blend::Native, Blend::Linear] {
            for a in 0..=255u8 {
                for s in 0..=255u8 {
                    let src = Argb::rgb(Chan::from_u8(s), Chan::from_u8(!s), Chan::from_u8(s));
                    for d in 0..=255u8 {
                        let dst = Argb::rgb(Chan::from_u8(d), Chan::from_u8(d), Chan::from_u8(!d));
                        assert_eq!(
                            blend.over_glass(src, dst, Coverage(a)),
                            blend.over(src, dst, Coverage(a)),
                            "{blend:?}: {s} over {d} at {a}"
                        );
                    }
                }
            }
        }
    }

    /// The scalar blend [`Blend::over_unit`] formed a channel at a time
    /// before its lanes, kept as the reference its vector is held to.
    fn over_unit_scalar(src: Argb, dst: Argb, a: f32) -> Argb {
        let byte =
            |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap_or(0);
        let ab = dst.a();
        let unit = |c: Chan| f32::from(c.get()) / 255.0;
        let ao = byte(a + unit(ab) * (1.0 - a));
        if ao == 0 {
            return Argb(0);
        }
        let m = u64::from(RECIP[usize::try_from(ao & 0xFF).unwrap_or(0)]);
        let mix = |s: Chan, d: Chan| -> Chan {
            let pb = baud_domain::premultiply_chan(d, ab);
            let co = byte(unit(s) * a + unit(pb) * (1.0 - a));
            let v = (u64::from(co * 255 + ao / 2) * m) >> 24;
            Chan::from_u8(u8::try_from(v).unwrap_or(255))
        };
        Argb::rgb(
            mix(src.r(), dst.r()),
            mix(src.g(), dst.g()),
            mix(src.b(), dst.b()),
        )
        .with_alpha(Chan::from_u8(u8::try_from(ao).unwrap_or(255)))
    }

    /// **An effect pixel's vector blend is the scalar one**, word for word,
    /// on both pages: every source channel over every backdrop channel at
    /// eleven backdrop alphas, at 1,021 source alphas spread over 0 to 1
    /// and the 256 byte fractions, the ties of the rounding among them, and
    /// past both ends.
    /// Perturbation: the alpha's lane formed as `ab a + (1 - a)` fails it.
    #[test]
    fn an_effect_pixels_vector_blend_is_the_scalar() {
        let mut alphas: Vec<f32> = (0..=1020u16).map(|i| f32::from(i) / 1020.0).collect();
        alphas.extend((0..=255u8).map(|c| f32::from(c) / 255.0));
        // A byte and a half: `v * 255` lands on a tie.
        alphas.extend((0..255u8).map(|c| (f32::from(c) + 0.5) / 255.0));
        alphas.extend([-0.5, 0.0, 1.0, 1.5, f32::MIN_POSITIVE, 1.0 - f32::EPSILON]);
        let mut checked = 0u64;
        for ab in [0u8, 1, 2, 64, 127, 128, 192, 200, 253, 254, 255] {
            for s in (0..=255u8).step_by(3) {
                let src = Argb::rgb(Chan::from_u8(s), Chan::from_u8(!s), Chan::from_u8(s / 2));
                for d in (0..=255u8).step_by(5) {
                    let dst = Argb::rgb(Chan::from_u8(d), Chan::from_u8(!d), Chan::from_u8(d / 3))
                        .with_alpha(Chan::from_u8(ab));
                    for &a in &alphas {
                        for page in [Page::Opaque, Page::Glass] {
                            assert_eq!(
                                Blend::Native.over_unit(src, dst, a, page),
                                over_unit_scalar(src, dst, a),
                                "{s} over {d} at {ab}, {a}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert!(checked > 10_000_000, "{checked}");
    }

    /// **`UNIT` is the division it replaced**, bit for bit, at every byte,
    /// against the division at run time: built at compile time, the table
    /// must not be a nearer or a farther rounding of the same quotient.
    /// Perturbation: the table built as `c * (1 / 255)` fails it.
    #[test]
    fn the_unit_table_is_the_division() {
        for c in 0..=255u8 {
            let run = core::hint::black_box(f32::from(c)) / core::hint::black_box(255.0f32);
            assert_eq!(UNIT[usize::from(c)].to_bits(), run.to_bits(), "{c}");
        }
    }

    /// **`RECIP` divides exactly** over the whole range its comment
    /// claims: every numerator below `2^16` by every alpha.
    #[test]
    fn the_reciprocal_divides_exactly() {
        for d in 1..=255u32 {
            let m = u64::from(RECIP[usize::try_from(d).unwrap()]);
            for n in 0..(1u32 << 16) {
                assert_eq!((u64::from(n) * m) >> 24, u64::from(n / d), "{n} / {d}");
            }
        }
    }

    /// **The straight byte `over_glass` keeps premultiplies back to the
    /// premultiplied byte it was formed from**: for every alpha and every
    /// premultiplied value it admits, the nearest straight byte to `co /
    /// ao`, formed by `RECIP` as the blend forms it and put through the
    /// presenter's rounding, is `co`.
    #[test]
    fn a_straight_byte_presents_as_its_premultiplied_byte() {
        // One channel as both presenters premultiply it, the shared
        // `baud_domain::premultiply_chan`.
        let byte = |v: u32| Chan::from_u8(u8::try_from(v).unwrap());
        let present =
            |c: u32, a: u32| u32::from(baud_domain::premultiply_chan(byte(c), byte(a)).get());
        for ao in 1..=255u32 {
            for co in 0..=ao {
                let n = co * 255 + ao / 2;
                let m = u64::from(RECIP[usize::try_from(ao).unwrap()]);
                let straight = u32::try_from((u64::from(n) * m) >> 24).unwrap();
                assert_eq!(straight, n / ao, "the reciprocal at {co}, {ao}");
                assert!(straight <= 255, "{co} at {ao}");
                assert_eq!(present(straight, ao), co, "{co} at {ao}");
            }
        }
    }

    /// **Over the glass the presented pixel is the GPU's**: the `Native`
    /// result, premultiplied by the presenter, equals source-over formed as
    /// the GPU forms it over its 8-bit premultiplied store -- `co = Cs as +
    /// cb (1 - as)` and `ao = as + ab (1 - as)`, each rounded to a byte, `cb`
    /// the backdrop presented -- at every source and backdrop alpha and a
    /// spread of channel values. `as` is the blend's own, a whole byte: the
    /// GPU's for an opaque colour and for a solid, while a mask in a colour
    /// below opaque takes `ca cov / 255` unrounded on the GPU.
    #[test]
    fn glass_presents_as_the_gpu_blends() {
        // One channel as both presenters premultiply it, the shared
        // `baud_domain::premultiply_chan`.
        let byte = |v: u32| Chan::from_u8(u8::try_from(v).unwrap());
        let present =
            |c: u32, a: u32| u32::from(baud_domain::premultiply_chan(byte(c), byte(a)).get());
        let chans = |p: Argb| p.0.to_be_bytes().map(u32::from);
        let vals = [0u8, 1, 37, 51, 102, 128, 153, 204, 254, 255];
        for sa in [255u8, 200, 90, 1] {
            for cov in 0..=255u8 {
                let a = u32::from(cov) * u32::from(sa) / 255;
                for ab in 0..=255u8 {
                    for &s in &vals {
                        for &d in &vals {
                            let src =
                                Argb::rgb(Chan::from_u8(s), Chan::from_u8(d), Chan::from_u8(s))
                                    .with_alpha(Chan::from_u8(sa));
                            let dst =
                                Argb::rgb(Chan::from_u8(d), Chan::from_u8(s), Chan::from_u8(d))
                                    .with_alpha(Chan::from_u8(ab));
                            let got = chans(Blend::Native.over_glass(src, dst, Coverage(cov)));
                            let (sc, dc) = (chans(src), chans(dst));
                            let ab = u32::from(ab);
                            let want_a = (a * 255 + ab * (255 - a) + 127) / 255;
                            assert_eq!(got[0], want_a, "alpha: {s} over {d}, {a} over {ab}");
                            for i in 1..4 {
                                let cb = present(dc[i], ab);
                                let want = (sc[i] * a + cb * (255 - a) + 127) / 255;
                                assert_eq!(
                                    present(got[i], got[0]),
                                    want,
                                    "channel {i}: {s} over {d}, {a} over {ab}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// **`Linear` over the glass is source-over in linear light**: against
    /// the formula in `f64`, through the exact transfer functions, within
    /// the one byte the 12-bit tables cost.
    #[test]
    fn linear_glass_is_source_over_in_linear_light() {
        let chans = |p: Argb| p.0.to_be_bytes().map(u32::from);
        let lin = |c: u32| {
            let c = f64::from(c) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let enc = |l: f64| {
            let c = if l <= 0.003_130_8 {
                l * 12.92
            } else {
                1.055 * l.powf(1.0 / 2.4) - 0.055
            };
            c * 255.0
        };
        let mut most = 0.0f64;
        for a in 1..=255u8 {
            for ab in (0..=255u8).step_by(5) {
                for s in (0..=255u8).step_by(15) {
                    for d in (0..=255u8).step_by(15) {
                        let src = Argb::rgb(Chan::from_u8(s), Chan::from_u8(s), Chan::from_u8(s));
                        let dst = Argb::rgb(Chan::from_u8(d), Chan::from_u8(d), Chan::from_u8(d))
                            .with_alpha(Chan::from_u8(ab));
                        let got = chans(Blend::Linear.over_glass(src, dst, Coverage(a)));
                        let (fa, fb) = (f64::from(a) / 255.0, f64::from(ab) / 255.0);
                        let ao = fa + fb * (1.0 - fa);
                        let co = (lin(s.into()) * fa + lin(d.into()) * fb * (1.0 - fa)) / ao;
                        most = most.max((f64::from(got[1]) - enc(co)).abs());
                    }
                }
            }
        }
        assert!(most <= 1.0, "{most} from the formula");
    }

    /// **The OVER of an opaque pixel is that pixel**, under `Blend::Native`,
    /// whatever it is drawn over: what lets `blit_opaque` store a drawn pixel
    /// rather than mix it. Every channel value, over a spread of
    /// destinations, translucent ones included.
    #[test]
    fn an_opaque_pixel_over_anything_is_itself() {
        for v in 0..=255u8 {
            let c = Chan::from_u8(v);
            let w = Chan::from_u8(v.wrapping_mul(37));
            let src = Argb::rgb(c, w, Chan::from_u8(255 - v));
            for d in [
                0u32,
                0x00FF_FFFF,
                0x3812_3456,
                0x80FF_0000,
                0xFFFF_FFFF,
                0xFF00_0000,
            ] {
                let got = Blend::Native.over(src, Argb(d), Coverage(255));
                assert_eq!(got.0, src.0, "{:08x} over {d:08x}", src.0);
            }
        }
    }
}
