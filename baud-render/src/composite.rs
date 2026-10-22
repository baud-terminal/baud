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
use crate::instance::Instance;
use crate::trail::{Pt, Smear};
use crate::units::{Band, Coverage, slots};
use baud_domain::{Chan, Cols, ImageAlpha, ImageGround, ImageView, Pixel};
use baud_font::Px;

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
pub struct Target<'p> {
    pixels: &'p mut [u32],
    stride: Px,
    width: Px,
    height: Px,
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
        })
    }

    pub fn pixels(&self) -> &[u32] {
        self.pixels
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
    }

    /// Fills the cells `set` names with their words from `bg`, over the
    /// rows of `band`, as [`Target::fill_band`] fills them -- replacing
    /// what is there -- and leaves every other cell and the margin as they
    /// are.
    pub fn fill_cells(&mut self, band: Band, bg: &[Argb], cell_w: Px, set: impl Fn(usize) -> bool) {
        let cell_w = slots(cell_w);
        let (stride, width) = (slots(self.stride), slots(self.width));
        for y in self.rows_of(band) {
            let row = &mut self.pixels[y * stride..y * stride + width];
            for (c, w) in bg.iter().enumerate() {
                let x = c * cell_w;
                let end = (x + cell_w).min(width);
                if x >= end {
                    break;
                }
                if set(c) {
                    row[x..end].fill(w.0);
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
            let row = &mut self.pixels[y * stride..y * stride + width];
            for c in 0..cols.index() {
                let x = c * cell_w;
                let end = (x + cell_w).min(width);
                if x >= end {
                    break;
                }
                if set(c) {
                    row[x..end].fill(0);
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
            let row = &mut self.pixels[y * stride..y * stride + width];
            for (c, &w) in bg.iter().enumerate() {
                let x = c * cell_w;
                let end = (x + cell_w).min(width);
                if x >= end {
                    break;
                }
                if set(c) {
                    for d in &mut row[x..end] {
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
            for mx in 0..w {
                let Ok(x) = usize::try_from(i32::from(inst.x.0) + i32::try_from(mx).unwrap_or(0))
                else {
                    continue;
                };
                if x >= width {
                    continue;
                }
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
                    // effect's records are the GPU's alone: on the CPU an
                    // effect is its emitter, drawn as a quad.
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
        let xs = slots(x0)..slots(x1).min(width);
        for y in ys {
            let row = &mut self.pixels[y * stride..y * stride + width];
            let cy = f32::from(u16::try_from(y).unwrap_or(u16::MAX)) + 0.5;
            for x in xs.clone() {
                let cx = f32::from(u16::try_from(x).unwrap_or(u16::MAX)) + 0.5;
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
    pub fn blit_image(
        &mut self,
        blit: &Blit<'_>,
        view: &ImageView<'_>,
        ground: Pixel,
        blend: Blend,
        page: Page,
    ) {
        let Some(area) = self.clip(blit) else {
            return;
        };
        match view.ground {
            ImageGround::Clear => self.blit_texels(&area, blit, view, |p| p, blend, page),
            ImageGround::Filled { .. } => self.blit_texels(
                &area,
                blit,
                view,
                move |p| crate::renderer::filled(p, ground),
                blend,
                page,
            ),
        }
    }

    /// [`Target::blit_image`] over `area`, each texel through `drawn`.
    #[inline(always)]
    fn blit_texels(
        &mut self,
        area: &Area,
        blit: &Blit<'_>,
        view: &ImageView<'_>,
        drawn: impl Fn(Pixel) -> Pixel + Copy,
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
                self.blit_by(area, row, view.alpha, blend, page);
            }
            Texels::Mapped { xs, ys } => {
                let index = |v: u32| usize::try_from(v).unwrap_or(0);
                let row = |j: usize| {
                    let from = index(ys[j]) * iw;
                    xs[cols.clone()]
                        .iter()
                        .map(move |&x| drawn(pixels[from + index(x)]))
                };
                self.blit_by(area, row, view.alpha, blend, page);
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
        let (i0, i1) = ((-x).max(0), (i64::from(self.width.value()) - x).min(w));
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
