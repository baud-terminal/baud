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
use baud_domain::{Chan, ImageAlpha, ImageView};
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
    /// rows of `band`: a mask in the instance's colour, its coverage bent
    /// by the instance's contrast curve, a colour image as it is, or a solid
    /// rectangle.
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
                        // The text-contrast curve, a lookup: the exact
                        // coverage bent for the colour the glyph is in.
                        blend.over(color, dst, Coverage(inst.curve.apply(cov)))
                    }
                    Kind::Solid => blend.over(color, dst, Coverage(255)),
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

    /// Blits an image's pixels over the buffer at slot `(x0, y0)`, which
    /// may be negative when an image has scrolled partway off the top. Clipped
    /// to the buffer on every side. A pixel is composited `blend.over`,
    /// exactly as a `Kind::Color` glyph is (an image is ARGB the same way an
    /// `sbix` glyph is), so an image drawn over glyphs reads over them; a
    /// clear pixel draws nothing and lets the cell underneath show through.
    /// A partial pixel is blended by its straight alpha, which `blend.over`
    /// weights by as it mixes, taking the pixel under it as opaque; the
    /// presenter premultiplies the result (`[[source: w3c-png-2003]]` 6.2
    /// stores colour straight; the record, "Images keep straight alpha, and
    /// a partial one repaints its rows", Found, not fixed (1)).
    ///
    /// **An opaque-or-clear image under `Blend::Native` is stored, not
    /// mixed**: the OVER of an opaque pixel is that pixel (`(255 s + 127) /
    /// 255` is `s`; `an_opaque_pixel_over_anything_is_itself` holds every
    /// channel value to it), which is the loop LLVM made of the blend before
    /// the store held alpha. Sharing one loop with the partial case, it
    /// stopped unswitching on the blend and blended every pixel, 53% more
    /// instructions a frame for a still Sixel.
    pub fn blit_image(&mut self, origin: (i32, i32), view: &ImageView<'_>, blend: Blend) {
        match (view.alpha, blend) {
            (ImageAlpha::OpaqueOrClear, Blend::Native) => self.blit_opaque(origin, view),
            (ImageAlpha::OpaqueOrClear, Blend::Linear) => self.blit_opaque_linear(origin, view),
            (ImageAlpha::Partial, _) => self.blit_blended(origin, view, blend),
        }
    }

    /// [`Target::blit_image`]'s opaque-or-clear `Linear` loop: each drawn
    /// pixel taken opaque and mixed at a constant blend, the shape the loop
    /// had before the store held alpha. Through `blit_blended`, reading each
    /// pixel's alpha, it cost 68% more instructions a frame.
    fn blit_opaque_linear(&mut self, origin: (i32, i32), view: &ImageView<'_>) {
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
                let p = pixels[my * iw + mx];
                if p.alpha().get() == 0 {
                    continue;
                }
                let (r, g, b) = p.rgb();
                let dst = Argb(self.pixels[base + x]);
                self.pixels[base + x] =
                    Blend::Linear.over(Argb::rgb(r, g, b), dst, Coverage(255)).0;
            }
        }
    }

    /// [`Target::blit_image`]'s opaque-or-clear `Native` loop: each drawn
    /// pixel stored as it is.
    fn blit_opaque(&mut self, origin: (i32, i32), view: &ImageView<'_>) {
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
                let p = pixels[my * iw + mx];
                if p.alpha().get() == 0 {
                    continue;
                }
                self.pixels[base + x] = p.bits();
            }
        }
    }

    /// [`Target::blit_image`]'s loop for a partial image: each drawn pixel
    /// `blend.over` by its alpha. Out of line, so it leaves the opaque
    /// loops' caller as it was.
    #[inline(never)]
    fn blit_blended(&mut self, origin: (i32, i32), view: &ImageView<'_>, blend: Blend) {
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
                let p = pixels[my * iw + mx];
                if p.alpha().get() == 0 {
                    continue;
                }
                let dst = Argb(self.pixels[base + x]);
                self.pixels[base + x] = blend.over(Argb(p.bits()), dst, Coverage(255)).0;
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

#[cfg(test)]
mod tests {
    use super::*;

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
