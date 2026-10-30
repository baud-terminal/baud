//! **A pointer image baud draws itself**, for a compositor without
//! `cursor-shape-v1`. `wl_pointer.enter` leaves the image undefined and
//! asks the client to set one -- "a client should respond to this event by
//! setting an appropriate pointer image with the set_cursor request"
//! (`[[source: wayland]]`) -- and with no shape global the only way is a
//! surface of the client's own. No cursor theme is read and no library
//! draws it: three images, the I-beam over text, the pointing hand for
//! `pointer` -- "pointer that indicates a link or another interactive
//! element" (`[[source: wp-cursor-shape-v1]]`) -- and the arrow for every
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

/// The three images drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    IBeam,
    Arrow,
    Hand,
}

impl Drawn {
    /// Every image, in the order a window keeps their buffers.
    pub const ALL: [Drawn; 3] = [Drawn::IBeam, Drawn::Arrow, Drawn::Hand];

    /// **The image for a shape**: the I-beam for `text` and
    /// `vertical-text`, the pointing hand for `pointer` -- a link, as the
    /// macOS app shows AppKit's pointing hand -- and the arrow for the
    /// rest, the nearest of the three, as the macOS app falls back to the
    /// arrow.
    pub fn of(shape: CursorShape) -> Drawn {
        use protocol::wp_cursor_shape_device_v1::shape as C;
        match shape.0 {
            C::TEXT | C::VERTICAL_TEXT => Drawn::IBeam,
            C::POINTER => Drawn::Hand,
            _ => Drawn::Arrow,
        }
    }

    /// The hotspot, in logical pixels: the I-beam's middle, the arrow's
    /// tip, the tip of the hand's pointing finger.
    pub fn hotspot(self) -> (i32, i32) {
        match self {
            Drawn::IBeam => (12, 12),
            Drawn::Arrow => (4, 3),
            Drawn::Hand => (9, 2),
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
            // The index finger up from the palm to the hotspot, the other
            // three folded below it with a gap between each, so the rim
            // draws them apart, and the thumb out to the left.
            Drawn::Hand => &[
                &[
                    (7.5, 14.0),
                    (7.5, 3.0),
                    (8.25, 2.0),
                    (9.35, 2.0),
                    (10.1, 3.0),
                    (10.1, 14.0),
                ],
                &[
                    (10.9, 14.0),
                    (10.9, 9.0),
                    (11.65, 8.0),
                    (12.35, 8.0),
                    (13.1, 9.0),
                    (13.1, 14.0),
                ],
                &[
                    (13.9, 14.0),
                    (13.9, 10.0),
                    (14.65, 9.0),
                    (15.35, 9.0),
                    (16.1, 10.0),
                    (16.1, 14.0),
                ],
                &[
                    (16.9, 14.0),
                    (16.9, 11.5),
                    (17.5, 10.5),
                    (18.4, 10.5),
                    (19.0, 11.5),
                    (19.0, 14.0),
                ],
                &[
                    (7.5, 13.0),
                    (19.0, 13.0),
                    (19.0, 17.0),
                    (17.5, 20.5),
                    (17.5, 22.0),
                    (9.5, 22.0),
                    (9.5, 20.5),
                    (7.5, 18.0),
                ],
                &[
                    (7.6, 13.6),
                    (5.5, 11.8),
                    (4.3, 11.8),
                    (3.6, 12.8),
                    (4.0, 14.0),
                    (7.6, 19.0),
                ],
            ],
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

/// A whole-number buffer scale: the compositor's `preferred_buffer_scale`,
/// or below `wl_compositor` version 6 the largest `wl_output.scale` of the
/// outputs the window is on.
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

    /// **The I-beam over text, the hand over a link, the arrow for the
    /// rest**: `text` and `vertical-text` are the beam, `pointer` -- the
    /// link's shape -- the hand, and every other of version 1's 34 shapes
    /// the arrow. Perturbation: `pointer` drawn as the arrow -- the hands
    /// fail.
    #[test]
    fn text_is_the_beam_pointer_the_hand_and_the_rest_the_arrow() {
        use protocol::wp_cursor_shape_device_v1::shape as C;
        let of = |d: Drawn| -> Vec<u32> {
            (1..=34)
                .filter(|&v| Drawn::of(CursorShape(v)) == d)
                .collect()
        };
        assert_eq!(of(Drawn::IBeam), [C::TEXT, C::VERTICAL_TEXT]);
        assert_eq!(of(Drawn::Hand), [C::POINTER]);
        assert_eq!(of(Drawn::Arrow).len(), 31);
    }

    /// **Each image is black within a white rim on a clear ground**, its
    /// hotspot on the black, the I-beam the same reflected left to right
    /// about its stem, and the scale's image the same shape at twice the
    /// pixels a side.
    #[test]
    fn each_image_is_black_in_a_white_rim() {
        for d in Drawn::ALL {
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
        let ink = |img: &[u32]| img.iter().map(|p| u64::from(p >> 24)).sum::<u64>();
        for d in [Drawn::Arrow, Drawn::Hand] {
            let (one, two) = (draw(d, Scale(1)), draw(d, Scale(2)));
            let ratio = ink(&two) as f64 / ink(&one) as f64;
            assert!(
                (3.8..4.2).contains(&ratio),
                "{d:?}: four times the ink: {ratio}"
            );
        }
        // The hotspots tell the images apart, as a compositor's log shows
        // them (`scripts/linux-pointer-trace.py`).
        let spots: std::collections::HashSet<_> = Drawn::ALL.map(Drawn::hotspot).into();
        assert_eq!(spots.len(), 3);
    }
}
