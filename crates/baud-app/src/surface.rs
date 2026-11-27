//! **A Wayland surface's points and the frame's pixels.** The Linux window
//! is sized in the surface's points, as xdg-shell configures it and as a
//! resize asks for it, and an input method's cursor rectangle is "in
//! surface local coordinates" (`[[source: wp-text-input-unstable-v3]]`);
//! the frame, the cells and what a program asks for in pixels are the
//! buffer's, the points times the scale the compositor prefers
//! (`baud_present_wayland::scale`). The scale is kept as that protocol
//! keeps it, "the numerator of a fraction with a denominator of 120"
//! (`[[source: wp-fractional-scale-v1]]`), so the arithmetic is in
//! integers; and a buffer is the points times it, "rounded halfway away
//! from zero", as the window's is. Pure, so its tests run on every
//! platform.

#![cfg_attr(all(not(test), not(target_os = "linux")), allow(dead_code))]

use baud_font::Px;

/// The frame's pixels to the surface's point, in 120ths: 120 at a scale of
/// one, 180 at 1.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scale120(pub u32);

/// A length in the surface's points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Points(pub u32);

impl Scale120 {
    pub const ONE: Scale120 = Scale120(120);

    /// **The surface's points as the frame's pixels**: the buffer they
    /// make, the product rounded halfway away from zero.
    pub fn pixels(self, p: Points) -> Px {
        let px = (u64::from(p.0) * u64::from(self.0) + 60) / 120;
        Px::of(u32::try_from(px).unwrap_or(u32::MAX))
    }

    /// **The frame's pixels as the surface's points**: the fewest whose
    /// buffer holds them, so a grid asked for in cells is never a pixel
    /// short of its last row -- the points reach the pixels less a half,
    /// `(2 px - 1) / 2` over the scale, rounded up.
    pub fn points(self, px: Px) -> Points {
        let s = u64::from(self.0.max(1));
        let twice = (2 * u64::from(px.value())).saturating_sub(1) * 120;
        Points(u32::try_from(twice.div_ceil(2 * s)).unwrap_or(u32::MAX))
    }

    /// **A rectangle of the frame in the surface's points**: the points
    /// that cover it -- its first point the one its first pixel is in, its
    /// far edge rounded up -- so the "area around the cursor" an input
    /// method keeps its candidate window clear of holds the whole cell.
    pub fn cover(self, rect: [Px; 4]) -> [Points; 4] {
        let s = u64::from(self.0.max(1));
        let at = |p: Px| u64::from(p.value()) * 120;
        let [x, y, w, h] = rect;
        let point = |v: u64| Points(u32::try_from(v).unwrap_or(u32::MAX));
        let (left, top) = (at(x) / s, at(y) / s);
        let far = |a: Px, b: Px| (at(a) + at(b)).div_ceil(s);
        [
            point(left),
            point(top),
            point(far(x, w) - left),
            point(far(y, h) - top),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Points to pixels and back**: at 1.5, 533 points are 799.5 pixels,
    /// rounded away from zero to 800, and 532 are 798, so 800 pixels ask
    /// for 533 points; 598 pixels, 23 rows of 26, ask for 399, as 398 are
    /// 597; at one a point is a pixel, and at 2 a half of one. Perturbation:
    /// round up the plain quotient instead of the half less -- 800 pixels
    /// ask for 534.
    #[test]
    fn a_length_in_pixels_is_the_fewest_points_that_hold_it() {
        let s = Scale120(180);
        let px = |p| s.pixels(Points(p)).value();
        assert_eq!((px(533), px(532), px(399), px(398)), (800, 798, 599, 597));
        assert_eq!(s.points(Px::of(800)), Points(533));
        assert_eq!(s.points(Px::of(598)), Points(399));
        assert_eq!(s.points(Px::of(960)), Points(640));
        assert_eq!(Scale120::ONE.points(Px::of(641)), Points(641));
        assert_eq!(Scale120::ONE.pixels(Points(641)), Px::of(641));
        assert_eq!(Scale120(240).points(Px::of(816)), Points(408));
        assert_eq!(Scale120(240).points(Px::of(817)), Points(409));
        assert_eq!(s.points(Px::of(0)), Points(0));
    }

    /// **The candidate window's rectangle is in the surface's points**:
    /// at one a pixel is a point; at 2 a 16 x 34 cell at (160, 68) is
    /// 8 x 17 points at (80, 34); at 1.5 a 12 x 26 cell at (31, 52) covers
    /// the points from (20, 34) to (29, 52), 9 x 18 of them -- the pixel 31
    /// is in point 20, and the far edge, 43 and 78 pixels, is 28.67 and 52
    /// points, 29 and 52. Perturbation: round the far edge down -- 8 x 18.
    #[test]
    fn a_frame_rectangle_is_the_points_that_cover_it() {
        let r = |x, y, w, h| [Px::of(x), Px::of(y), Px::of(w), Px::of(h)];
        let p = |x, y, w, h| [Points(x), Points(y), Points(w), Points(h)];
        assert_eq!(Scale120::ONE.cover(r(30, 40, 10, 20)), p(30, 40, 10, 20));
        assert_eq!(Scale120(240).cover(r(160, 68, 16, 34)), p(80, 34, 8, 17));
        assert_eq!(Scale120(180).cover(r(31, 52, 12, 26)), p(20, 34, 9, 18));
    }
}
