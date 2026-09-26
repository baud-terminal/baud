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
use baud_domain::{Chan, ImageView};
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

    /// Paints `inst` over the pixels, clipped to the buffer and to the
    /// rows of `band`: a mask in the instance's colour, a colour image as
    /// it is, or a solid rectangle.
    pub fn over(&mut self, inst: &Instance, atlas: &Atlas, blend: Blend, band: Band) {
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
                        blend.over(color, dst, Coverage(cov))
                    }
                    Kind::Solid => blend.over(color, dst, Coverage(255)),
                    // A quad's record is only its bounds; the pixels come
                    // from its corners, which `Target::quad` is handed. A
                    // fire's records are the GPU's alone: on the CPU a fire
                    // is its emitter, drawn as a quad.
                    Kind::Quad | Kind::Fire | Kind::Ember => continue,
                    Kind::Color => {
                        let src = Argb(atlas.argb()[(v + my) * color_side + u + mx]);
                        if src.a().get() == 0 {
                            continue;
                        }
                        blend.over(src, dst, Coverage(255))
                    }
                };
                row[x] = out.0;
            }
        }
    }

    /// Paints the trail's quad over the pixels in `fill`, clipped to its
    /// bounds, the buffer and the rows of `band`: each pixel as much as
    /// [`crate::trail::coverage`] says the quad covers its centre.
    pub fn quad(&mut self, s: &Smear, fill: Argb, blend: Blend, band: Band) {
        let (x0, y0, x1, y1) = s.bounds();
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
                row[x] = blend.over(fill, Argb(row[x]), Coverage(c)).0;
            }
        }
    }

    /// Blits an image's ARGB pixels over the buffer at slot `(x0, y0)`, which
    /// may be negative when an image has scrolled partway off the top. Clipped
    /// to the buffer on every side. A pixel is composited `blend.over` opaque,
    /// exactly as a `Kind::Color` glyph is (an image is ARGB the same way an
    /// `sbix` glyph is), so an image drawn over glyphs reads over them.
    ///
    /// A `ColorWord` that resolves to RGB is drawn; anything else -- a
    /// `DEFAULT` word, which is an unpainted, transparent Sixel pixel, or an
    /// indexed word, which a Sixel arena never holds -- draws nothing and lets
    /// the cell underneath show through. Transparency is by omission, not an
    /// alpha channel; the Sixel `Pb`/`P2` whole-image transparency mode is a
    /// separate thing the core does not yet see.
    pub fn blit_image(&mut self, origin: (i32, i32), view: &ImageView<'_>, blend: Blend) {
        let (x0, y0) = origin;
        let (iw, ih) = (view.width.index(), view.height.index());
        let pixels = view.pixels;
        let (stride, width, height) = (slots(self.stride), slots(self.width), slots(self.height));
        for my in 0..ih {
            let Ok(y) = usize::try_from(y0 + i32::try_from(my).unwrap_or(0)) else {
                continue;
            };
            if y >= height {
                continue;
            }
            let base = y * stride;
            for mx in 0..iw {
                let Ok(x) = usize::try_from(x0 + i32::try_from(mx).unwrap_or(0)) else {
                    continue;
                };
                if x >= width {
                    continue;
                }
                let Some((r, g, b)) = pixels[my * iw + mx].as_rgb() else {
                    continue;
                };
                let src = Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
                let dst = Argb(self.pixels[base + x]);
                self.pixels[base + x] = blend.over(src, dst, Coverage(255)).0;
            }
        }
    }
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
}

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
