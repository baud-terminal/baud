//! The cursor trail: a fading ghost of the cursor sweeping from the cell
//! it left to the cell it reached, over a few frames. Off by default; the
//! feature sfte ships as `SFTE_CURSOR_TRAIL`, built to baud's blink
//! standard instead of sfte's full-wipe -- the geometry is pure and the
//! one translucent solid every path draws, the scrollbar's shape one
//! module over.
//!
//! The driver hands the renderer a [`Trail`] whose `progress` it computed
//! from the display link's timestamp -- the way [`crate::Blink`] is driven
//! -- and the renderer only interpolates, so a frame is a pure function of
//! the value it was handed and the pixel oracle can compare two GPU arms
//! on it. Sampling a clock inside the render path would make a golden frame
//! non-deterministic; the phase is a parameter, never a clock.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::{Chan, Col, Row};
use baud_font::Px;

/// A cursor move to animate: the cell the cursor left, the cell it
/// reached, and how far along the sweep the driver has advanced it -- 0.0
/// at the start, 1.0 when the ghost lands on the new cell.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Trail {
    pub from: (Row, Col),
    pub to: (Row, Col),
    pub progress: f32,
}

/// The ghost's rectangle in pixels of the frame and the alpha it draws at
/// -- a cursor-cell-sized box at the interpolated position, fading to
/// nothing as it lands.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ghost {
    pub left: Px,
    pub top: Px,
    pub width: Px,
    pub height: Px,
    pub alpha: Chan,
}

impl Ghost {
    /// Whether the ghost touches any pixel row of a band, top inclusive,
    /// bottom exclusive -- the scrollbar's `overlaps`, so a row is dirtied
    /// only while the ghost is over it and the damage stays bounded.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        self.top.value() < bottom.value() && self.top.value() + self.height.value() > top.value()
    }
}

/// The alpha the ghost starts at, before the fade.
const BASE_ALPHA: u8 = 0xB0;

/// Round a float pixel to a `Px`, the `boxes` idiom -- `as i64` is not a
/// counted cast, so this is the one float-to-pixel conversion the audit
/// permits.
fn round_px(v: f32) -> Px {
    Px::of(u32::try_from(v.round().max(0.0) as i64).unwrap_or(0))
}

/// The ghost for `trail` on cells `cell_w` by `cell_h`: a cursor-sized box
/// linearly interpolated from the cell it left to the cell it reached, at
/// an alpha that fades to zero as it lands.
pub fn ghost(trail: Trail, cell_w: Px, cell_h: Px) -> Ghost {
    let p = trail.progress.clamp(0.0, 1.0);
    let col_px = |c: Col| {
        u32::try_from(c.index())
            .unwrap_or(0)
            .saturating_mul(cell_w.value())
    };
    let row_px = |r: Row| {
        u32::try_from(r.index())
            .unwrap_or(0)
            .saturating_mul(cell_h.value())
    };
    let lerp = |a: u32, b: u32| a as f32 + (b as f32 - a as f32) * p;
    let left = round_px(lerp(col_px(trail.from.1), col_px(trail.to.1)));
    let top = round_px(lerp(row_px(trail.from.0), row_px(trail.to.0)));
    let fade = (f32::from(BASE_ALPHA) * (1.0 - p)).round().max(0.0) as i64;
    Ghost {
        left,
        top,
        width: cell_w,
        height: cell_h,
        alpha: Chan::from_u8(u8::try_from(fade).unwrap_or(0)),
    }
}

/// The ghost as the one translucent solid every path draws, in `fg` at its
/// faded alpha.
pub fn instance(g: Ghost, fg: Argb) -> Instance {
    Instance {
        x: Screen(i16::try_from(g.left.value()).unwrap_or(i16::MAX)),
        y: Screen(i16::try_from(g.top.value()).unwrap_or(i16::MAX)),
        w: Tex(u16::try_from(g.width.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(g.height.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color: fg.with_alpha(g.alpha),
        kind: Kind::Solid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: Px = Px::of(8);
    const CELL_H: Px = Px::of(18);

    fn trail(from: (usize, usize), to: (usize, usize), progress: f32) -> Trail {
        Trail {
            from: (Row::from_index(from.0), Col::at_index(from.1)),
            to: (Row::from_index(to.0), Col::at_index(to.1)),
            progress,
        }
    }

    /// At the start the ghost is over the cell the cursor left, opaque;
    /// halfway it is halfway there and half-faded; at the end it is on the
    /// destination cell and gone.
    #[test]
    fn the_ghost_lerps_from_the_old_cell_to_the_new_and_fades() {
        // Row 2 col 3 -> row 2 col 13, a ten-cell move along one row.
        let start = ghost(trail((2, 3), (2, 13), 0.0), CELL_W, CELL_H);
        assert_eq!((start.left.value(), start.top.value()), (3 * 8, 2 * 18));
        assert_eq!(start.alpha.get(), 0xB0);

        let mid = ghost(trail((2, 3), (2, 13), 0.5), CELL_W, CELL_H);
        assert_eq!(mid.left.value(), (3 * 8 + 13 * 8) / 2);
        assert_eq!(mid.top.value(), 2 * 18);
        assert_eq!(mid.alpha.get(), 0x58);

        let end = ghost(trail((2, 3), (2, 13), 1.0), CELL_W, CELL_H);
        assert_eq!(end.left.value(), 13 * 8);
        assert_eq!(end.alpha.get(), 0);
        // Always one cell in size, wherever it is on the sweep.
        assert_eq!((end.width.value(), end.height.value()), (8, 18));
    }

    /// A vertical move sweeps the row band, so a mid-sweep ghost overlaps a
    /// row between the two endpoints and not the rows outside them.
    #[test]
    fn a_vertical_ghost_overlaps_only_the_rows_it_is_over() {
        // Row 1 -> row 5, half way: the ghost's top is at row 3.
        let g = ghost(trail((1, 0), (5, 0), 0.5), CELL_W, CELL_H);
        let band = |r: u32| (Px::of(r * 18), Px::of(r * 18 + 18));
        let (t3, b3) = band(3);
        assert!(g.overlaps(t3, b3), "over row 3");
        let (t0, b0) = band(0);
        assert!(!g.overlaps(t0, b0), "not over row 0");
        let (t5, b5) = band(5);
        assert!(!g.overlaps(t5, b5), "not yet at row 5");
    }

    /// Progress is clamped, so a driver that overshoots draws the landed,
    /// invisible ghost rather than reading past the endpoints.
    #[test]
    fn progress_is_clamped() {
        let over = ghost(trail((0, 0), (0, 4), 1.5), CELL_W, CELL_H);
        let end = ghost(trail((0, 0), (0, 4), 1.0), CELL_W, CELL_H);
        assert_eq!(over, end);
    }
}
