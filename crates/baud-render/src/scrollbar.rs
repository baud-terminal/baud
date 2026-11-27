//! The scrollbar: a thumb on the right edge saying where the window sits
//! in the ring, seen as much as the driver's look says -- revealed by a
//! scroll, held, faded, and widened with a track under the pointer, as
//! AppKit's overlay scroller is; or, under the legacy style, a track and
//! knob always drawn in a gutter of their own beside the grid.
//! `tools/scripts/scroller-probe.swift` measures AppKit's; this is the geometry,
//! pure, and the solids every path draws.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use baud_domain::{Chan, Rows};
use baud_font::Px;

/// Where the window is: the lines retained behind the live screen, how
/// many of them it is up, and the screen's rows.
pub type Window = baud_domain::ScrollWindow;

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

    /// Whether the thumb touches any pixel row of `band`, top inclusive,
    /// bottom exclusive.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        self.top.value() < bottom.value() && self.top.value() + self.height.value() > top.value()
    }
}

/// The minimum thumb, in cells: a deep history leaves something to grab.
const MIN_CELLS: u32 = 2;

/// Whether the pointer has widened the thumb: AppKit's overlay knob widens
/// under the pointer and a track appears behind it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hover {
    Rest,
    Hovered,
}

/// How the thumb is seen this frame: how much of it, and whether it is
/// widened. The driver computes both off the display link; the renderer
/// draws what it is handed and interpolates nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Look {
    /// The thumb's opacity as a fraction of its own: 255 is the knob at its
    /// full half alpha, 0 is nothing drawn.
    pub shown: Chan,
    pub hover: Hover,
}

impl Look {
    /// Nothing drawn.
    pub const HIDDEN: Look = Look {
        shown: Chan::from_offset(0, 0),
        hover: Hover::Rest,
    };
}

/// The scrollbar's widths, in pixels of the frame: the driver's points --
/// AppKit's, measured -- times the backing scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Widths {
    /// The knob at rest.
    pub rest: Px,
    /// The knob, and the track behind it, under the pointer.
    pub hovered: Px,
    /// From the frame's right edge to the knob's.
    pub inset: Px,
    /// The strip the pointer hovers and a press is taken in, from the right
    /// edge: the overlay scroller's frame.
    pub strip: Px,
}

/// AppKit's two scroller styles, `NSScrollerStyle`: which one the user's
/// "Show scroll bars" setting asks for is the driver's to read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scroller {
    /// Over the grid's right edge, revealed by a scroll and faded.
    Overlay,
    /// In a gutter of its own beside the grid, always drawn: a track, and
    /// the knob in it whenever there is history. No fade, no hover.
    Legacy,
}

/// The gutter the grid gives up to the scroller, in pixels of the frame:
/// the strip under the legacy style, nothing under the overlay one, which
/// lies over the grid. One function, so the layout that takes the columns
/// and the bar that is drawn in them agree.
pub fn gutter(scroller: Scroller, widths: Widths) -> Px {
    match scroller {
        Scroller::Overlay => Px::of(0),
        Scroller::Legacy => widths.strip,
    }
}

/// What the scrollbar draws, with the window, its look, its widths and its
/// style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scroll {
    pub window: Window,
    pub look: Look,
    pub widths: Widths,
    pub scroller: Scroller,
}

/// The scrollbar as drawn this frame: the knob, the track when the pointer
/// widened it or the style keeps one, and how much of both is seen. Its
/// equality is the damage rule's: a fade changes `shown` and a hover
/// changes the knob and the track, so a row either covers is dirty whenever
/// any of them moves. The knob is none only under the legacy style with
/// nothing to scroll, where the track is drawn alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Bar {
    pub knob: Option<Thumb>,
    pub track: Option<Thumb>,
    pub shown: Chan,
}

impl Bar {
    /// What the bar draws in the band of pixel rows from `top` to `bottom`:
    /// the knob and the track clipped to it, and how much of both is seen.
    /// None where neither touches the band. Two bars with the same slice of
    /// a band draw the same pixels there, which is the damage rule: a fade
    /// changes every slice the bar has, a hover every row the track spans,
    /// and a knob that moves under a hover only the rows it entered or left.
    pub fn slice(&self, top: Px, bottom: Px) -> Option<Slice> {
        let knob = self.knob.and_then(|k| clip(k, top, bottom));
        let track = self.track.and_then(|t| clip(t, top, bottom));
        (knob.is_some() || track.is_some()).then_some(Slice {
            knob,
            track,
            shown: self.shown,
        })
    }
}

/// A bar's drawing inside one band of rows: [`Bar::slice`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slice {
    knob: Option<Thumb>,
    track: Option<Thumb>,
    shown: Chan,
}

/// `t` clipped to the rows from `top` to `bottom`, or none outside them.
fn clip(t: Thumb, top: Px, bottom: Px) -> Option<Thumb> {
    let lo = t.top.value().max(top.value());
    let hi = (t.top.value() + t.height.value()).min(bottom.value());
    // Lazily: `hi - lo` is only a height when the band and `t` meet.
    (lo < hi).then(|| Thumb {
        top: Px::of(lo),
        height: Px::of(hi - lo),
        ..t
    })
}

/// The knob's alpha at full, half the foreground, as AppKit's
/// (`tools/scripts/scroller-probe.swift` measures it).
const KNOB_ALPHA: Chan = Chan::from_offset(0x80, 0);
/// The track's, 12 of 255: AppKit's legacy track, measured at 0.047 in both
/// appearances.
const TRACK_ALPHA: Chan = Chan::from_offset(12, 0);

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

/// The knob for `win` on a frame of `size` with cells `cell_h` high, at
/// rest or widened, or none with nothing retained. On the live screen it is
/// at the bottom of its travel: whether it is seen there is the look's.
pub fn thumb(
    win: Window,
    size: (Px, Px),
    cell_h: Px,
    widths: Widths,
    hover: Hover,
) -> Option<Thumb> {
    if win.retained.index() == 0 || win.rows.index() == 0 {
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
    let width = u64::from(
        match hover {
            Hover::Rest => widths.rest,
            Hover::Hovered => widths.hovered,
        }
        .value(),
    )
    .min(w);
    let left = w.saturating_sub(width + u64::from(widths.inset.value()));
    Some(Thumb {
        left: px(left),
        top: px(top),
        width: px(width),
        height: len,
    })
}

/// The track behind a widened knob: the knob's band down the whole height.
/// None at rest, where AppKit's overlay scroller draws no track.
pub fn track(size: (Px, Px), widths: Widths, hover: Hover) -> Option<Thumb> {
    if hover == Hover::Rest {
        return None;
    }
    let (w, h) = size;
    let width = Px::of(widths.hovered.value().min(w.value()));
    Some(Thumb {
        left: Px::of(
            w.value()
                .saturating_sub(width.value() + widths.inset.value()),
        ),
        top: Px::of(0),
        width,
        height: h,
    })
}

/// The bar to draw for `s` on a frame of `size`, or none when nothing of
/// it is seen. Under the overlay style that is the look's; under the legacy
/// style the look is ignored, since nothing fades or hovers, and the track
/// is drawn whether or not there is a knob to put in it.
pub fn bar(s: Scroll, size: (Px, Px), cell_h: Px) -> Option<Bar> {
    match s.scroller {
        Scroller::Overlay => {
            if s.look.shown.get() == 0 {
                return None;
            }
            let knob = thumb(s.window, size, cell_h, s.widths, s.look.hover)?;
            Some(Bar {
                knob: Some(knob),
                track: track(size, s.widths, s.look.hover),
                shown: s.look.shown,
            })
        }
        Scroller::Legacy => Some(Bar {
            knob: thumb(s.window, size, cell_h, s.widths, Hover::Hovered),
            track: track(size, s.widths, Hover::Hovered),
            shown: Chan::from_u8(255),
        }),
    }
}

/// Where a pixel column is: in the scrollbar's strip, or over the grid.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Strip,
    Grid,
}

/// Where the pixel column `x` of a frame `w` wide is.
pub fn place(x: Px, w: Px, widths: Widths) -> Place {
    match x.value() >= w.value().saturating_sub(widths.strip.value()) && x.value() < w.value() {
        true => Place::Strip,
        false => Place::Grid,
    }
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

/// `full` of 255 scaled by `shown`: a fade's opacity applied to an alpha.
fn scaled(full: Chan, shown: Chan) -> Chan {
    let v = (u16::from(full.get()) * u16::from(shown.get()) + 127) / 255;
    Chan::from_u8(u8::try_from(v).unwrap_or(u8::MAX))
}

fn solid(t: Thumb, color: Argb) -> Instance {
    Instance {
        x: Screen(i16::try_from(t.left.value()).unwrap_or(i16::MAX)),
        y: Screen(i16::try_from(t.top.value()).unwrap_or(i16::MAX)),
        w: Tex(u16::try_from(t.width.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(t.height.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color,
        kind: Kind::Solid,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

/// The bar as the solids every path draws, in `fg`: the track first, then
/// the knob over it, each at its alpha times how much of the bar is seen.
pub fn instances(b: Bar, fg: Argb) -> impl Iterator<Item = Instance> {
    let track = b
        .track
        .map(|t| solid(t, fg.with_alpha(scaled(TRACK_ALPHA, b.shown))));
    let knob = b
        .knob
        .map(|k| solid(k, fg.with_alpha(scaled(KNOB_ALPHA, b.shown))));
    track.into_iter().chain(knob)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (Px, Px) = (Px::of(800), Px::of(500));
    const CELL_H: Px = Px::of(10);
    /// AppKit's at 2x: a 6 pt knob, 11 pt widened, 3 pt in, a 17 pt strip.
    const WIDTHS: Widths = Widths {
        rest: Px::of(12),
        hovered: Px::of(22),
        inset: Px::of(6),
        strip: Px::of(34),
    };

    fn win(retained: Rows, back: Rows, rows: Rows) -> Window {
        Window {
            retained,
            back,
            rows,
        }
    }

    #[test]
    fn no_thumb_with_no_history_and_the_live_screen_at_the_bottom_of_its_travel() {
        let bare = win(Rows::ZERO, Rows::ZERO, Rows::of_count(50));
        assert!(thumb(bare, SIZE, CELL_H, WIDTHS, Hover::Rest).is_none());
        // At the live screen the thumb is at the bottom: whether it is seen
        // is the look's, which a scroll to the live screen reveals.
        let live = win(Rows::of_count(100), Rows::ZERO, Rows::of_count(50));
        let t = thumb(live, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
        assert_eq!(t.top.value() + t.height.value(), 500);
    }

    #[test]
    fn the_thumb_spans_its_travel_and_never_shrinks_below_two_cells() {
        // 50 screen rows over 150 lines: a third of the height, 166 px.
        let top = win(Rows::of_count(100), Rows::of_count(100), Rows::of_count(50));
        let t = thumb(top, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
        assert_eq!((t.top.value(), t.height.value()), (0, 166));
        // 6 px in from the edge, 12 px wide: AppKit's knob at rest, at 2x.
        assert_eq!((t.left.value(), t.width.value()), (782, 12));
        // One line up: the thumb is at the bottom of its travel, near enough.
        let one = win(Rows::of_count(100), Rows::of_count(1), Rows::of_count(50));
        let t = thumb(one, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
        assert_eq!(t.top.value(), 330);
        // A deep history: two cells, not two pixels.
        let deep = win(
            Rows::of_count(100_000),
            Rows::of_count(5),
            Rows::of_count(50),
        );
        let t = thumb(deep, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
        assert_eq!(t.height.value(), 20);
    }

    #[test]
    fn the_pointer_widens_the_knob_and_draws_a_track_the_whole_height() {
        let w = win(Rows::of_count(100), Rows::of_count(40), Rows::of_count(50));
        let rest = thumb(w, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
        let over = thumb(w, SIZE, CELL_H, WIDTHS, Hover::Hovered).unwrap();
        // Wider, the same inset, the same rows: only the width moves.
        assert_eq!((over.left.value(), over.width.value()), (772, 22));
        assert_eq!((over.top, over.height), (rest.top, rest.height));
        assert!(
            track(SIZE, WIDTHS, Hover::Rest).is_none(),
            "no track at rest"
        );
        let t = track(SIZE, WIDTHS, Hover::Hovered).unwrap();
        assert_eq!((t.left, t.width), (over.left, over.width));
        assert_eq!((t.top.value(), t.height.value()), (0, 500));
    }

    #[test]
    fn a_bar_seen_not_at_all_draws_nothing_and_a_fade_scales_both_alphas() {
        let s = |shown: u8, hover: Hover| Scroll {
            window: win(Rows::of_count(100), Rows::of_count(40), Rows::of_count(50)),
            look: Look {
                shown: Chan::from_u8(shown),
                hover,
            },
            widths: WIDTHS,
            scroller: Scroller::Overlay,
        };
        assert!(bar(s(0, Hover::Hovered), SIZE, CELL_H).is_none());
        let alphas = |b: Bar| -> Vec<u8> {
            instances(b, Argb(0xFFFF_FFFF))
                .map(|i| u8::try_from(i.color.0 >> 24).unwrap())
                .collect()
        };
        // Full: the knob at half the foreground, no track at rest.
        assert_eq!(
            alphas(bar(s(255, Hover::Rest), SIZE, CELL_H).unwrap()),
            [0x80]
        );
        // Widened: the track first, at 12, then the knob over it.
        assert_eq!(
            alphas(bar(s(255, Hover::Hovered), SIZE, CELL_H).unwrap()),
            [12, 0x80]
        );
        // Half faded: both halved, rounded.
        assert_eq!(
            alphas(bar(s(128, Hover::Hovered), SIZE, CELL_H).unwrap()),
            [6, 64]
        );
    }

    /// **Under the legacy style the track is always drawn and the knob is
    /// always wide**: no look hides or narrows it, the track stands alone
    /// with nothing to scroll, and both lie in the gutter the layout gave
    /// up -- the strip -- and nowhere left of it.
    #[test]
    fn the_legacy_bar_is_a_track_always_and_a_wide_knob_with_history() {
        let s = |retained: usize, look: Look| Scroll {
            window: win(
                Rows::of_count(retained),
                Rows::of_count(retained.min(40)),
                Rows::of_count(50),
            ),
            look,
            widths: WIDTHS,
            scroller: Scroller::Legacy,
        };
        let alphas = |b: Bar| -> Vec<u8> {
            instances(b, Argb(0xFFFF_FFFF))
                .map(|i| u8::try_from(i.color.0 >> 24).unwrap())
                .collect()
        };
        // Hidden and at rest by the overlay's look, and still drawn whole.
        let b = bar(s(100, Look::HIDDEN), SIZE, CELL_H).unwrap();
        assert_eq!(alphas(b), [12, 0x80]);
        let knob = b.knob.unwrap();
        assert_eq!((knob.left.value(), knob.width.value()), (772, 22));
        let t = b.track.unwrap();
        assert_eq!((t.left.value(), t.width.value()), (772, 22));
        assert_eq!((t.top.value(), t.height.value()), (0, 500));
        // Nothing to scroll: the track alone.
        let bare = bar(s(0, Look::HIDDEN), SIZE, CELL_H).unwrap();
        assert!(bare.knob.is_none());
        assert_eq!(alphas(bare), [12]);
        // The gutter is the strip, and the bar lies inside it.
        let g = gutter(Scroller::Legacy, WIDTHS);
        assert_eq!(g, WIDTHS.strip);
        assert!(t.left.value() >= SIZE.0.value() - g.value());
        assert_eq!(gutter(Scroller::Overlay, WIDTHS), Px::of(0));
    }

    #[test]
    fn the_strip_is_the_scrollers_frame_from_the_right_edge() {
        let w = Px::of(800);
        assert_eq!(place(Px::of(766), w, WIDTHS), Place::Strip);
        assert_eq!(place(Px::of(799), w, WIDTHS), Place::Strip);
        assert_eq!(place(Px::of(765), w, WIDTHS), Place::Grid);
        assert_eq!(place(Px::of(800), w, WIDTHS), Place::Grid);
    }

    #[test]
    fn the_inverse_lands_where_the_thumb_was() {
        for back in [1, 7, 40, 99, 100] {
            let w = win(
                Rows::of_count(100),
                Rows::of_count(back),
                Rows::of_count(50),
            );
            let t = thumb(w, SIZE, CELL_H, WIDTHS, Hover::Rest).unwrap();
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
