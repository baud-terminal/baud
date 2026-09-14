//! The scrollbar: a thumb on the right edge saying where the window sits
//! in the ring, visible while the window is off the live screen.
//! `docs/research/notes/l-scrollbar.md` is the spec; this is the geometry,
//! pure, and the one solid instance every path draws.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::{Chan, Rows};
use baud_font::Px;

/// Where the window is: the lines retained behind the live screen, how
/// many of them it is up, and the screen's rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Window {
    pub retained: Rows,
    pub back: Rows,
    pub rows: Rows,
}

/// The thumb, in pixels of the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Thumb {
    pub left: Px,
    pub top: Px,
    pub width: Px,
    pub height: Px,
}

impl Thumb {
    /// Whether the pixel row `y` is inside the thumb's rows.
    pub fn covers_y(&self, y: Px) -> bool {
        y.value() >= self.top.value() && y.value() < self.top.value() + self.height.value()
    }

    /// Whether the pixel column `x` is inside the thumb's strip.
    pub fn in_strip(&self, x: Px) -> bool {
        x.value() >= self.left.value() && x.value() < self.left.value() + self.width.value()
    }

    /// Whether the thumb touches any pixel row of `band`, top inclusive,
    /// bottom exclusive.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        self.top.value() < bottom.value() && self.top.value() + self.height.value() > top.value()
    }
}

/// The minimum thumb, in cells: a deep history leaves something to grab.
const MIN_CELLS: u32 = 2;
/// The inset from the right edge, in pixels.
const INSET: u32 = 2;
/// The narrowest thumb, in pixels.
const MIN_WIDTH: u32 = 4;

/// The thumb's length, in pixels, for a frame `h` high.
fn length(win: Window, h: Px, cell_h: Px) -> Px {
    let wide = |n: Rows| u64::try_from(n.index()).unwrap_or(u64::MAX);
    let px = |n: u64| Px::of(u32::try_from(n).unwrap_or(u32::MAX));
    let h = u64::from(h.value());
    let total = wide(win.retained) + wide(win.rows);
    let share = h.saturating_mul(wide(win.rows)) / total.max(1);
    let floor = u64::from(cell_h.value()) * u64::from(MIN_CELLS);
    px(share.max(floor).min(h))
}

/// The thumb for `win` on a frame of `size` with cells `cell_w` by
/// `cell_h`, or none on the live screen or with nothing retained.
pub fn thumb(win: Window, size: (Px, Px), cell_w: Px, cell_h: Px) -> Option<Thumb> {
    if win.back.index() == 0 || win.retained.index() == 0 || win.rows.index() == 0 {
        return None;
    }
    let wide = |n: Rows| u64::try_from(n.index()).unwrap_or(u64::MAX);
    let px = |n: u64| Px::of(u32::try_from(n).unwrap_or(u32::MAX));
    let (w, h) = (u64::from(size.0.value()), u64::from(size.1.value()));
    let len = length(win, size.1, cell_h);
    let travel = h - u64::from(len.value());
    let retained = wide(win.retained);
    let back = wide(win.back).min(retained);
    let top = travel * (retained - back) / retained;
    let width = (u64::from(cell_w.value()) / 2)
        .max(u64::from(MIN_WIDTH))
        .min(w);
    let left = w.saturating_sub(width + u64::from(INSET));
    Some(Thumb {
        left: px(left),
        top: px(top),
        width: px(width),
        height: len,
    })
}

/// The window whose thumb's top is at pixel row `top`: the inverse of
/// [`thumb`], clamped to the history. What a drag and a jump ask.
pub fn back_for(win: Window, size: (Px, Px), cell_h: Px, top: Px) -> Rows {
    let h = u64::from(size.1.value());
    let len = u64::from(length(win, size.1, cell_h).value());
    let travel = h - len;
    if travel == 0 {
        return win.back;
    }
    let retained = u64::try_from(win.retained.index()).unwrap_or(u64::MAX);
    let down = u64::from(top.value()).min(travel);
    let back = retained - (down * retained / travel);
    Rows::of_count(usize::try_from(back).unwrap_or(usize::MAX))
}

/// The thumb as the one solid instance every path draws, in `fg` at half
/// alpha.
pub fn instance(t: Thumb, fg: Argb) -> Instance {
    Instance {
        x: Screen(i16::try_from(t.left.value()).unwrap_or(i16::MAX)),
        y: Screen(i16::try_from(t.top.value()).unwrap_or(i16::MAX)),
        w: Tex(u16::try_from(t.width.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(t.height.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color: fg.with_alpha(Chan::from_u8(0x80)),
        kind: Kind::Solid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (Px, Px) = (Px::of(800), Px::of(500));
    const CELL_W: Px = Px::of(8);
    const CELL_H: Px = Px::of(10);

    fn win(retained: Rows, back: Rows, rows: Rows) -> Window {
        Window {
            retained,
            back,
            rows,
        }
    }

    #[test]
    fn no_thumb_on_the_live_screen_or_with_no_history() {
        let live = win(Rows::of_count(100), Rows::ZERO, Rows::of_count(50));
        assert!(thumb(live, SIZE, CELL_W, CELL_H).is_none());
        let bare = win(Rows::ZERO, Rows::ZERO, Rows::of_count(50));
        assert!(thumb(bare, SIZE, CELL_W, CELL_H).is_none());
    }

    #[test]
    fn the_thumb_spans_its_travel_and_never_shrinks_below_two_cells() {
        // 50 screen rows over 150 lines: a third of the height, 166 px.
        let top = win(Rows::of_count(100), Rows::of_count(100), Rows::of_count(50));
        let t = thumb(top, SIZE, CELL_W, CELL_H).unwrap();
        assert_eq!((t.top.value(), t.height.value()), (0, 166));
        assert_eq!((t.left.value(), t.width.value()), (794, 4));
        // One line up: the thumb is at the bottom of its travel, near enough.
        let one = win(Rows::of_count(100), Rows::of_count(1), Rows::of_count(50));
        let t = thumb(one, SIZE, CELL_W, CELL_H).unwrap();
        assert_eq!(t.top.value(), 330);
        // A deep history: two cells, not two pixels.
        let deep = win(
            Rows::of_count(100_000),
            Rows::of_count(5),
            Rows::of_count(50),
        );
        let t = thumb(deep, SIZE, CELL_W, CELL_H).unwrap();
        assert_eq!(t.height.value(), 20);
    }

    #[test]
    fn the_inverse_lands_where_the_thumb_was() {
        for back in [1, 7, 40, 99, 100] {
            let w = win(
                Rows::of_count(100),
                Rows::of_count(back),
                Rows::of_count(50),
            );
            let t = thumb(w, SIZE, CELL_W, CELL_H).unwrap();
            let got = back_for(w, SIZE, CELL_H, t.top).index();
            assert!(
                got.abs_diff(back) <= 1,
                "back {back} -> top {} -> {got}",
                t.top.value()
            );
        }
        // Past the travel clamps to the live screen, not below.
        let half = win(Rows::of_count(100), Rows::of_count(50), Rows::of_count(50));
        assert_eq!(back_for(half, SIZE, CELL_H, Px::of(10_000)).index(), 0);
    }
}
