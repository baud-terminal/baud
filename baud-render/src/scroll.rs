//! Smooth scrolling: output slides the screen up rather than jumping it,
//! the feature sfte ships as `SFTE_TERM_SCROLL_SMOOTH`. On by default, as
//! sfte's is (`[scroll] mode`, where `normal` opts out), and built to answer
//! the cost the first attempt was withdrawn for -- ~100 ms of
//! scrolled-output latency.
//!
//! The performance answer is the cap. sfte's form, and baud's first, let
//! the pending offset accumulate: a line takes the whole slide to arrive,
//! and continuous output at a rate faster than the slide drains falls
//! arbitrarily far behind. Here the offset is **capped at one row**, so
//! the screen is never more than a row behind whatever the output rate,
//! and a fast burst shows a continuous sub-row catch-up rather than a
//! growing backlog. The most a keystroke of scrolled output can be delayed
//! is one row's slide, [`SLIDE_SECS_PER_ROW`], not an unbounded queue.
//!
//! The geometry is a single pixel offset the renderer shifts every row
//! down by, drawing one extra "leaving" row above the window -- the row
//! that just scrolled off, which the scrollback ring still holds one row
//! up (`view(back + 1).row(0)`), addressed as ring row -1 by the same
//! rotation modulo every path already uses. The offset is a parameter the
//! driver advances off the display link's timestamp, the way the trail and
//! the transition are, so a frame is a pure function of it and the pixel
//! oracle can compare the GPU arms on it.

use baud_domain::Rows;
use baud_font::Px;

/// One row's slide, in seconds: the most output can be delayed, and the
/// time a single discrete scroll takes to settle. 60 ms -- shorter than
/// the first attempt's 100 ms, and bounded rather than accumulating.
pub const SLIDE_SECS_PER_ROW: f32 = 0.06;

/// The live smooth-scroll offset: how many pixels the screen is currently
/// shifted **down** from its settled position, sliding toward zero. The
/// state is the driver's; the renderer reads only [`Slide::pan`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Slide {
    /// Pixels still to slide, in `[0, cell_h]` after every mutation.
    remaining: f32,
    /// One row in pixels: the cap on `remaining` and the unit a row crosses
    /// in [`SLIDE_SECS_PER_ROW`]. Kept here so the driver hands it once and
    /// a resize is one `set_cell_h`.
    cell_h: f32,
}

impl Slide {
    /// A settled slide for a screen whose rows are `cell_h` pixels tall.
    pub fn new(cell_h: Px) -> Slide {
        Slide {
            remaining: 0.0,
            cell_h: cell_h.value() as f32,
        }
    }

    /// The row height changed (a resize or a font zoom): keep the offset a
    /// valid fraction of a row so a slide in flight does not overshoot.
    pub fn set_cell_h(&mut self, cell_h: Px) {
        self.cell_h = cell_h.value() as f32;
        self.remaining = self.remaining.min(self.cell_h);
    }

    /// Output retired `rows` lines at the live screen: the window advanced,
    /// so the screen should appear to lag by that much and catch up. The
    /// offset is **capped at one row** -- the whole performance argument --
    /// so a burst never builds a backlog and the added latency is bounded
    /// to one row's slide regardless of the output rate.
    pub fn retire(&mut self, rows: Rows) {
        if self.cell_h <= 0.0 {
            return;
        }
        self.remaining = (self.remaining + rows.index() as f32 * self.cell_h).min(self.cell_h);
    }

    /// Advance the slide toward settled by `dt` seconds, at one row per
    /// [`SLIDE_SECS_PER_ROW`]. Returns whether a slide is still in flight,
    /// which the driver uses to decide whether to force another frame --
    /// so an idle screen, whose `remaining` is zero, forces nothing.
    pub fn advance(&mut self, dt: f32) -> bool {
        if self.cell_h > 0.0 && dt > 0.0 {
            let speed = self.cell_h / SLIDE_SECS_PER_ROW;
            self.remaining = (self.remaining - speed * dt).max(0.0);
        }
        self.active()
    }

    /// Whether a slide is in flight.
    pub fn active(&self) -> bool {
        self.remaining > 0.0
    }

    /// Snap to settled -- used when the offset would name a row the view no
    /// longer has: the view leaves the live screen (a scroll back into
    /// history), the alternate screen is switched, or a resize reflows the
    /// ring the leaving row was read from.
    pub fn snap(&mut self) {
        self.remaining = 0.0;
    }

    /// The offset the renderer shifts every row down by, rounded to whole
    /// pixels: zero when settled, so the non-slide path is unchanged. The
    /// `as i64` then `try_from` is the one float-to-pixel idiom the audit
    /// permits, the `boxes`/`trail` precedent.
    pub fn pan(&self) -> Px {
        Px::of(u32::try_from(self.remaining.round().max(0.0) as i64).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell() -> Px {
        Px::of(34)
    }

    #[test]
    fn a_settled_slide_pans_by_nothing() {
        let s = Slide::new(cell());
        assert!(!s.active());
        assert_eq!(s.pan(), Px::of(0));
    }

    #[test]
    fn retiring_a_row_pans_by_a_row_and_drains_to_nothing() {
        let mut s = Slide::new(cell());
        s.retire(Rows::of_count(1));
        assert_eq!(s.pan(), Px::of(34));
        assert!(s.active());
        // One full slide's worth of time settles it exactly.
        assert!(!s.advance(SLIDE_SECS_PER_ROW));
        assert_eq!(s.pan(), Px::of(0));
    }

    #[test]
    fn a_burst_is_capped_at_one_row_so_latency_cannot_accumulate() {
        let mut s = Slide::new(cell());
        s.retire(Rows::of_count(50));
        assert_eq!(s.pan(), Px::of(34), "fifty rows still pan by at most one");
        // And retiring again while already at the cap stays at the cap.
        s.retire(Rows::of_count(1));
        assert_eq!(s.pan(), Px::of(34));
    }

    #[test]
    fn half_a_slide_pans_by_about_half_a_row() {
        let mut s = Slide::new(cell());
        s.retire(Rows::of_count(1));
        s.advance(SLIDE_SECS_PER_ROW / 2.0);
        // 34 - 34*(0.5) = 17.
        assert_eq!(s.pan(), Px::of(17));
        assert!(s.active());
    }

    #[test]
    fn advancing_a_settled_slide_does_nothing_and_forces_no_frame() {
        let mut s = Slide::new(cell());
        assert!(!s.advance(1.0));
        assert_eq!(s.pan(), Px::of(0));
    }

    #[test]
    fn a_snap_settles_a_slide_in_flight() {
        let mut s = Slide::new(cell());
        s.retire(Rows::of_count(1));
        s.snap();
        assert!(!s.active());
        assert_eq!(s.pan(), Px::of(0));
    }

    #[test]
    fn a_shorter_row_clamps_an_offset_in_flight() {
        let mut s = Slide::new(cell());
        s.retire(Rows::of_count(1));
        assert_eq!(s.pan(), Px::of(34));
        s.set_cell_h(Px::of(20));
        assert_eq!(s.pan(), Px::of(20), "the offset never exceeds a row");
    }
}
