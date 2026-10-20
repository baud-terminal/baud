//! **A pointer image baud draws itself**, for a compositor without
//! `cursor-shape-v1`. `wl_pointer.enter` leaves the image undefined and
//! asks the client to set one -- "a client should respond to this event by
//! setting an appropriate pointer image with the set_cursor request"
//! (`[[source: wayland]]`) -- and with no shape global the only way is a
//! surface of the client's own. No cursor theme is read and no library
//! draws it: two images, the I-beam over text and the arrow for every
//! other shape, drawn here at the window's buffer scale.
//!
//! **24 logical pixels**, GNOME's `cursor-size` default (`[[source:
//! gnome-interface-schema-2417a27]]`, line 185). Each image is black with
//! a white rim one logical pixel wide, so it reads on any background,
//! sampled four by four in each pixel and premultiplied, as `wl_shm`'s
//! `ARGB8888` is.

use crate::pointer::CursorShape;
use baud_wayland::protocol;

/// The cursor's side in logical pixels.
pub const SIZE: u32 = 24;

/// The two images drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    IBeam,
    Arrow,
}

impl Drawn {
    /// **The image for a shape**: the I-beam for `text` and
    /// `vertical-text`, the arrow for the rest -- the nearest of the two,
    /// as the macOS app falls back to the arrow.
    pub fn of(shape: CursorShape) -> Drawn {
        use protocol::wp_cursor_shape_device_v1::shape as C;
        match shape.0 {
            C::TEXT | C::VERTICAL_TEXT => Drawn::IBeam,
            _ => Drawn::Arrow,
        }
    }

    /// The hotspot, in logical pixels: the I-beam's middle, the arrow's
    /// tip.
    pub fn hotspot(self) -> (i32, i32) {
        match self {
            Drawn::IBeam => (12, 12),
            Drawn::Arrow => (4, 3),
        }
    }

    /// The outline, as polygons in logical pixels.
    fn polygons(self) -> &'static [&'static [(f64, f64)]] {
        match self {
            // A stem with a bar at each end, on whole pixels, so it is as
            // sharp at one as at two.
            Drawn::IBeam => &[
                &[(11.0, 6.0), (13.0, 6.0), (13.0, 18.0), (11.0, 18.0)],
                &[(8.0, 4.0), (16.0, 4.0), (16.0, 6.0), (8.0, 6.0)],
                &[(8.0, 18.0), (16.0, 18.0), (16.0, 20.0), (8.0, 20.0)],
            ],
            // The tip at the hotspot, the left edge straight down, the
            // tail out to the lower right.
            Drawn::Arrow => &[&[
                (4.0, 3.0),
                (4.0, 18.5),
                (7.75, 15.0),
                (10.25, 20.5),
                (12.5, 19.5),
                (10.0, 14.0),
                (15.0, 14.0),
            ]],
        }
    }
}

/// Whether `p` is inside the polygon, by crossings.
fn inside(poly: &[(f64, f64)], p: (f64, f64)) -> bool {
    let mut odd = false;
    for (i, &a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < a.0 + (p.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            odd = !odd;
        }
    }
    odd
}

/// The distance from `p` to the polygon's edge.
fn distance(poly: &[(f64, f64)], p: (f64, f64)) -> f64 {
    poly.iter()
        .enumerate()
        .map(|(i, &a)| {
            let b = poly[(i + 1) % poly.len()];
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

/// The rim's width, in logical pixels.
const RIM: f64 = 1.0;

/// Samples a pixel takes in each direction.
const SAMPLES: u32 = 4;

/// A whole-number buffer scale, the compositor's `preferred_buffer_scale`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale(pub u32);

/// **`d` drawn at `scale`**: `SIZE * scale` pixels a side, row by row,
/// premultiplied `0xAARRGGBB` -- black where the shape is, white on its
/// rim, clear elsewhere, each pixel the mean of its samples.
pub fn draw(d: Drawn, scale: Scale) -> Vec<u32> {
    let s = f64::from(scale.0.max(1));
    let side = SIZE * scale.0.max(1);
    let polys = d.polygons();
    let n = SAMPLES * SAMPLES;
    let mut out = Vec::with_capacity(usize::try_from(side * side).unwrap_or(0));
    for py in 0..side {
        for px in 0..side {
            let (mut shape, mut rim) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let p = (
                        (f64::from(px) + (f64::from(sx) + 0.5) / f64::from(SAMPLES)) / s,
                        (f64::from(py) + (f64::from(sy) + 0.5) / f64::from(SAMPLES)) / s,
                    );
                    if polys.iter().any(|poly| inside(poly, p)) {
                        shape += 1;
                    } else if polys.iter().any(|poly| distance(poly, p) <= RIM) {
                        rim += 1;
                    }
                }
            }
            // The mean of the samples in 0..=255, rounded to nearest.
            let a = ((shape + rim) * 255 + n / 2) / n;
            let w = (rim * 255 + n / 2) / n;
            out.push((a << 24) | (w << 16) | (w << 8) | w);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The I-beam over text, the arrow for the rest**: `text` and
    /// `vertical-text` are the beam, and every other of version 1's 34
    /// shapes is the arrow.
    #[test]
    fn text_is_the_beam_and_the_rest_the_arrow() {
        use protocol::wp_cursor_shape_device_v1::shape as C;
        let beams: Vec<u32> = (1..=34)
            .filter(|&v| Drawn::of(CursorShape(v)) == Drawn::IBeam)
            .collect();
        assert_eq!(beams, [C::TEXT, C::VERTICAL_TEXT]);
    }

    /// **Each image is black within a white rim on a clear ground**, its
    /// hotspot on the black, the I-beam the same reflected left to right
    /// about its stem, and the scale's image the same shape at twice the
    /// pixels a side.
    #[test]
    fn each_image_is_black_in_a_white_rim() {
        for d in [Drawn::IBeam, Drawn::Arrow] {
            for scale in [1, 2] {
                let img = draw(d, Scale(scale));
                let side = SIZE * scale;
                let at = |x: u32, y: u32| img[usize::try_from(y * side + x).unwrap()];
                assert_eq!(img.len(), usize::try_from(side * side).unwrap());
                let opaque_black = img.iter().filter(|&&p| p == 0xff00_0000).count();
                let opaque_white = img.iter().filter(|&&p| p == 0xffff_ffff).count();
                assert!(opaque_black > 0 && opaque_white > 0, "{d:?} at {scale}");
                assert_eq!(at(0, side - 1), 0, "a clear corner");
                // Premultiplied: no channel above the alpha.
                assert!(img.iter().all(|&p| (p & 0xff) <= p >> 24));
                let (hx, hy) = d.hotspot();
                let (hx, hy) = (u32::try_from(hx).unwrap(), u32::try_from(hy).unwrap());
                // The pixel just inside the hotspot, toward the shape.
                let inner = at(hx * scale, hy * scale + scale);
                assert_eq!(inner >> 24, 0xff, "{d:?} at {scale}: the hotspot is on it");
            }
        }
        let beam = draw(Drawn::IBeam, Scale(1));
        let at = |x: u32, y: u32| beam[usize::try_from(y * SIZE + x).unwrap()];
        for y in 0..SIZE {
            for x in 0..SIZE {
                assert_eq!(at(x, y), at(SIZE - 1 - x, y), "mirrored at ({x}, {y})");
            }
        }
        let (one, two) = (draw(Drawn::Arrow, Scale(1)), draw(Drawn::Arrow, Scale(2)));
        let ink = |img: &[u32]| img.iter().map(|p| u64::from(p >> 24)).sum::<u64>();
        let ratio = ink(&two) as f64 / ink(&one) as f64;
        assert!((3.8..4.2).contains(&ratio), "four times the ink: {ratio}");
    }
}
