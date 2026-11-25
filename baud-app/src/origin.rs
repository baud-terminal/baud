//! **Where the frame sits in the Linux surface**: `padding` points of
//! gutter on every side, and what the surface leaves beyond the frame
//! split evenly between opposite sides, the odd pixel right and bottom, as
//! the macOS driver centres its frame.
//!
//! The frame is the renderer's whole frame (`Renderer::whole_size`): the
//! exit-status strip, the grid and the activity lights' line. Its corner
//! in the surface, [`Origin`], is the one place the gutter and the
//! leftover are added: the target the frame is drawn into, the damage sent
//! to the compositor, the slide's rows, the pointer, the scrollbar, the
//! input method's rectangle and the lights' hover all go through it. The
//! surface outside the frame is the margin colour the driver fills the
//! buffer with.

use baud_font::Px;
use baud_render::Target;

/// **The frame's top-left corner in the surface**, in the frame's pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Origin {
    pub(crate) x: Px,
    pub(crate) y: Px,
}

impl Origin {
    /// The surface's own corner: before the first configure.
    pub(crate) const CORNER: Origin = Origin {
        x: Px::of(0),
        y: Px::of(0),
    };
}

/// **The gutter `padding` points make** at `px_per_point` pixels a point
/// (`typesize.rs`), whole pixels rounded down, as the macOS driver rounds
/// its padding.
pub(crate) fn gutter(padding: f32, px_per_point: f64) -> Px {
    // The padding in points, at most a few dozen, times a few pixels a
    // point: `as i64` is the widening the audit permits.
    let px = (f64::from(padding) * px_per_point).max(0.0).floor() as i64;
    Px::of(u32::try_from(px).unwrap_or(0))
}

/// **The room inside the gutter**: the surface less `gutter` on each side,
/// per axis, which the grid's cells are counted in.
pub(crate) fn room(surface: (Px, Px), gutter: Px) -> (Px, Px) {
    let less = |s: Px| Px::of(s.value().saturating_sub(gutter.value().saturating_mul(2)));
    (less(surface.0), less(surface.1))
}

/// **Where a frame `whole` sits** in a surface `surface` with `gutter` on
/// every side: the gutter plus half of what the room leaves beyond the
/// frame, per axis. A frame wider than the room keeps the frame inside
/// the surface, the gutter giving way first.
pub(crate) fn origin(surface: (Px, Px), whole: (Px, Px), gutter: Px) -> Origin {
    let (rw, rh) = room(surface, gutter);
    let axis = |s: Px, r: Px, w: Px| {
        let left = r.value().saturating_sub(w.value());
        let at = gutter.value().saturating_add(left / 2);
        Px::of(at.min(s.value().saturating_sub(w.value())))
    };
    Origin {
        x: axis(surface.0, rw, whole.0),
        y: axis(surface.1, rh, whole.1),
    }
}

/// **`f` over the frame's own target**: `whole` at `at` in the surface's
/// `frame`, its rows `stride` pixels apart. `None` when the frame does not
/// fit the buffer.
pub(crate) fn in_frame<R>(
    frame: &mut [u32],
    stride: Px,
    at: Origin,
    whole: (Px, Px),
    f: impl FnOnce(&mut Target<'_>) -> R,
) -> Option<R> {
    let pitch = usize::try_from(stride.value()).ok().filter(|&p| p > 0)?;
    let rows = Px::of(u32::try_from(frame.len() / pitch).ok()?);
    let mut surface = Target::new(frame, stride, stride, rows)?;
    let mut band = surface.rows(at.y, whole.1)?;
    let mut target = band.columns(at.x, whole.0)?;
    Some(f(&mut target))
}

/// **`f` over the whole surface `frame`**, `width` pixels wide and its
/// rows `stride` pixels apart, given the surface's size: the target a lent
/// draw paints, the renderer placing the whole frame at its origin in it
/// (`Renderer::set_surface`). `None` for a buffer of no whole row, or rows
/// narrower than `width`.
pub(crate) fn surface<R>(
    frame: &mut [u32],
    stride: Px,
    width: Px,
    f: impl FnOnce(&mut Target<'_>, (Px, Px)) -> R,
) -> Option<R> {
    let pitch = usize::try_from(stride.value()).ok().filter(|&p| p > 0)?;
    let rows = Px::of(u32::try_from(frame.len() / pitch).ok()?);
    let mut surface = Target::new(frame, stride, width, rows)?;
    Some(f(&mut surface, (width, rows)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The gutter, then the leftover split**: 8 points at 96 dpi
    /// (4/3 px a point) is a 10 px gutter; a 1000 × 500 surface leaves a
    /// 980 × 480 room, and a 970 × 471 frame in it sits at 10 + 10/2 and
    /// 10 + 9/2, the odd pixel below. Padding 0 still centres. A frame
    /// wider than the room stays inside the surface. Perturbation: drop the
    /// half -- the frame sits at the gutter, the leftover all right and
    /// below -- and the first row goes red.
    #[test]
    fn the_frame_sits_inside_the_gutter_with_the_leftover_split() {
        let g = gutter(8.0, 96.0 / 72.0);
        assert_eq!(g, Px::of(10));
        assert_eq!(
            room((Px::of(1000), Px::of(500)), g),
            (Px::of(980), Px::of(480))
        );
        let rows = [
            (
                (Px::of(1000), Px::of(500)),
                (Px::of(970), Px::of(471)),
                g,
                (15, 14),
            ),
            (
                (Px::of(1000), Px::of(500)),
                (Px::of(970), Px::of(471)),
                Px::of(0),
                (15, 14),
            ),
            (
                (Px::of(1000), Px::of(500)),
                (Px::of(1000), Px::of(500)),
                Px::of(0),
                (0, 0),
            ),
            (
                (Px::of(1000), Px::of(500)),
                (Px::of(995), Px::of(500)),
                g,
                (5, 0),
            ),
        ];
        for (surface, whole, g, (x, y)) in rows {
            assert_eq!(
                origin(surface, whole, g),
                Origin {
                    x: Px::of(x),
                    y: Px::of(y)
                },
                "{surface:?} {whole:?} {g:?}"
            );
        }
        let whole = (Px::of(10), Px::of(10));
        assert_eq!(origin(whole, whole, Px::of(0)), Origin::CORNER);
        assert_eq!(gutter(0.0, 2.0), Px::of(0));
        assert_eq!(gutter(-3.0, 2.0), Px::of(0));
        assert_eq!(gutter(8.0, 2.0 * 96.0 / 72.0), Px::of(21));
    }

    /// **The frame's target starts at its origin**: in an 8 × 6 surface
    /// whose pixels hold their own index, a 4 × 3 frame's first pixel is
    /// the origin's and its next row's is one stride on, the last row
    /// reaching the surface's bottom with nothing below it. A frame past
    /// the surface's side gets no target. Perturbation: build the target
    /// over the buffer's start -- its first pixel is 0.
    #[test]
    fn the_frames_target_is_the_frames_pixels_in_the_surface() {
        let index: Vec<u32> = (0..48).collect();
        for (x, y) in [(3, 2), (4, 3)] {
            let mut buf = index.clone();
            let at = Origin {
                x: Px::of(x),
                y: Px::of(y),
            };
            let first = in_frame(&mut buf, Px::of(8), at, (Px::of(4), Px::of(3)), |t| {
                (t.pixels()[0], t.pixels()[8])
            });
            assert_eq!(first, Some((y * 8 + x, (y + 1) * 8 + x)), "{at:?}");
        }
        let mut buf = index.clone();
        let over = Origin {
            x: Px::of(5),
            y: Px::of(0),
        };
        assert_eq!(
            in_frame(&mut buf, Px::of(8), over, (Px::of(4), Px::of(3)), |_| ()),
            None
        );
    }

    /// **The Linux driver maps between the surface and the frame through
    /// the origin alone**: every line of linux.rs that reads the grid's top
    /// or the strip as a position adds the origin on that axis, the frame
    /// is drawn only through [`in_frame`] or into the [`surface`] placed at
    /// the origin, and `origin` is written in one place. Configure's cell count, the one `strip()` that is a width,
    /// is named. The three positions are counted, so a check that matched
    /// nothing would fail. Perturbation: map the pointer by `grid_top()`
    /// alone, or build a `Target` over the buffer's start, and this goes
    /// red.
    #[test]
    fn the_driver_maps_through_the_origin_alone() {
        let src = include_str!("linux.rs");
        let width = "room_w.value().saturating_sub(self.renderer.strip().value())";
        for line in src.lines() {
            if line.contains("grid_top()") {
                let at = "self.origin.y.value() + self.renderer.grid_top()";
                assert!(line.contains(at), "{line}");
            }
            if line.contains(".strip()") && !line.contains(width) {
                let at = "self.origin.x.value() + self.renderer.strip()";
                assert!(line.contains(at), "{line}");
            }
            for no in [
                "lights_top()",
                "grid_left()",
                "Target::new(",
                ".at((0.0, 0.0))",
            ] {
                assert!(!line.contains(no), "{line}");
            }
        }
        assert_eq!(src.matches("self.renderer.grid_top()").count(), 3);
        assert_eq!(src.matches(width).count(), 1);
        assert_eq!(src.matches("crate::origin::in_frame(").count(), 1);
        assert_eq!(src.matches("crate::origin::surface(").count(), 1);
        assert_eq!(
            src.matches("renderer.set_surface((sw, sh), (at.x, at.y))")
                .count(),
            1
        );
        assert_eq!(src.matches("self.origin = ").count(), 1);
    }
}
