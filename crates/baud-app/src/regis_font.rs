//! ReGIS's built-in alphabet, drawn with baud's embedded face by baud's own
//! rasteriser, at the size each character is shown.
//!
//! **Every ReGIS device "must have a built in character definition and
//! writing capability"** (`[[source: dec-std-070]]` 070-8 § 8.5.6), and the
//! VT330's and VT340's is a ROM of 8 by 10 cells scaled to the unit cell
//! (`[[source: vt3xx-text]]`). baud's is JetBrains Mono with baud's Hebrew,
//! the face its text is drawn in (`embedded::REGULAR`), rasterised straight
//! into each character's unit cell -- "In the computational limit ... the
//! display cell and the unit cell should map to identical sizes" (§ 8.7.3.5)
//! -- rather than a small bitmap magnified. The face's cell, its advance
//! across and its ascender to its descender down, is stretched to the unit
//! cell each way, as ReGIS stretches a character to the width and height it
//! names (§ 8.7.3.4.7: "Multiply each pixel from the stored character image
//! width by hint, and each pixel from the stored character height by vint";
//! rather than keeping the face's proportions); a pixel half covered or more
//! is set, ReGIS writing a character's ones and zeros.

use baud_domain::{Codepoint, ImagePx};
use baud_font::{Bounds, Canvas, Face, Fit, Phase, Rasterizer, Scale};
use std::sync::OnceLock;

/// The embedded face, parsed once.
fn face() -> Option<&'static Face<'static>> {
    static FACE: OnceLock<Option<Face<'static>>> = OnceLock::new();
    FACE.get_or_init(|| Face::parse(crate::embedded::REGULAR.bytes).ok())
        .as_ref()
}

/// Draw `c` in a `w` by `h` cell: [`baud_domain::CellFont`].
pub fn glyph(c: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    let Some(face) = face() else {
        return;
    };
    let Some(cp) = Codepoint::new(u32::from(c)) else {
        return;
    };
    let Some(g) = face.glyph(cp) else {
        return;
    };
    let lm = face.line_metrics();
    let (asc, desc) = (
        f32::from(lm.ascender.value()),
        f32::from(lm.descender.value()),
    );
    let height = (asc - desc).max(1.0);
    let advance = face
        .advance(g)
        .map_or(height / 2.0, |a| f32::from(a.value()))
        .max(1.0);
    let (cw, ch) = (f32::from(w.raw()), f32::from(h.raw()));
    // Pixels per font unit down, and how much wider the cell is than the
    // face's cell at that scale.
    let t = ch / height;
    let scale = Scale::of(
        t * f32::from(face.units_per_em().value()),
        face.units_per_em(),
    );
    let sx = (cw / advance) / t;
    let mut b = Bounds::new();
    let _ = face.outline(g, &mut b);
    let Some(fit) = Fit::stretched(&b, scale, Phase::ZERO, sx, 1.0) else {
        return;
    };
    let (Some(cells), Some(need)) = (fit.extent.cells(), fit.extent.scratch()) else {
        return;
    };
    let mut scratch = vec![0f32; need];
    let Some(canvas) = Canvas::new(fit.extent, &mut scratch) else {
        return;
    };
    let mut r = Rasterizer::new(canvas, fit.transform);
    let _ = face.outline(g, &mut r);
    let mut cover = vec![0u8; cells];
    if r.finish().coverage(&mut cover).is_none() {
        return;
    }
    // The mask's origin: `left` pixels from the glyph's origin across, and
    // `top` pixels from the baseline down; the baseline is the ascender's
    // height below the cell's top.
    let base = (asc * t).round();
    let mw = usize::try_from(fit.extent.w.value()).unwrap_or(0);
    for (i, &v) in cover.iter().enumerate() {
        if v < 128 || mw == 0 {
            continue;
        }
        let (mx, my) = (i % mw, i / mw);
        let x = i64::from(fit.left) + i64::try_from(mx).unwrap_or(i64::MAX);
        let y = base as i64 + i64::from(fit.top) + i64::try_from(my).unwrap_or(i64::MAX);
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            continue;
        };
        if x < w.raw() && y < h.raw() {
            set(ImagePx::of(x), ImagePx::of(y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(c: char, w: ImagePx, h: ImagePx) -> Vec<(u16, u16)> {
        let mut v = Vec::new();
        glyph(c, w, h, &mut |x, y| v.push((x.raw(), y.raw())));
        v
    }

    /// A character the face has draws inside its cell, and a space draws
    /// nothing.
    #[test]
    fn a_character_draws_inside_its_cell() {
        let a = mask('A', ImagePx::of(8), ImagePx::of(20));
        assert!(
            a.len() > 20,
            "an A of 8 by 20 sets some pixels, {}",
            a.len()
        );
        assert!(a.iter().all(|&(x, y)| x < 8 && y < 20));
        assert!(mask(' ', ImagePx::of(8), ImagePx::of(20)).is_empty());
    }

    /// A larger cell draws a larger character: the ink of an `H` at 32 by
    /// 80 spans more of its cell's width than a pixel.
    #[test]
    fn the_character_fills_the_cell_it_is_given() {
        let h = mask('H', ImagePx::of(32), ImagePx::of(80));
        let (x0, x1) = (
            h.iter().map(|p| p.0).min().unwrap(),
            h.iter().map(|p| p.0).max().unwrap(),
        );
        let (y0, y1) = (
            h.iter().map(|p| p.1).min().unwrap(),
            h.iter().map(|p| p.1).max().unwrap(),
        );
        assert!(x1 - x0 >= 16, "{x0}..{x1}");
        assert!(y1 - y0 >= 30, "{y0}..{y1}");
    }
}
