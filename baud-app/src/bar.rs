//! **The scrollbar's thumb under the Linux driver's pointer**: the macOS
//! app's rules (`app.rs`, `scrollbar`), over the same pure parts --
//! `reveal.rs` for when the thumb is seen and who a press belongs to,
//! `baud_render`'s `thumb`, `place` and `back_for` for the geometry. A press
//! on the knob drags the window through the history, a press in the strip
//! off it jumps there, the pointer over the strip while the window is
//! scrolled back widens and holds the thumb, and a press the scrollbar
//! takes keeps its drags and its release, so no one else sees a release
//! without its press. Hidden by `?30`, nothing is drawn and nothing takes a
//! press. The Linux window has no system scroller setting to follow, so the
//! style is the overlay one, which lies over the grid and gives up no
//! column. Pure, over a real `Term`, so the tests run on every platform.

use crate::reveal::{Button, Drag, Grab, Knob, Motion, Over, Reveal, Takes};
use baud_core::Term;
use baud_domain::{HostTime, ScrollbarShown};
use baud_font::Px;
use baud_render::{Look, Place, Scroller, Widths, back_for, place, thumb};

/// The frame the thumb is drawn in: its size in pixels, the cell's height,
/// and the scroller's widths. The Linux frame draws the grid from its first
/// pixel at a scale of one, so a surface point is a frame pixel.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Frame {
    pub(crate) size: (Px, Px),
    pub(crate) cell_h: Px,
    pub(crate) widths: Widths,
}

/// A pointer event as the scrollbar reads it, at a surface point.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Event {
    /// A move; `held` when a button the scrollbar did not take is down.
    Moved {
        x: f64,
        y: f64,
        held: Held,
    },
    Pressed {
        x: f64,
        y: f64,
        left: Left,
    },
    Released {
        x: f64,
        y: f64,
    },
    Left,
}

/// Whether a button is down for a move.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Held {
    Yes,
    No,
}

/// Whether a press is the left button's, the only one that drags or jumps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Left {
    Yes,
    No,
}

/// Whether the scrollbar took an event, so the seat must not see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Took {
    Yes,
    No,
}

/// A drag of the knob: where the pointer pressed and where the knob's top
/// was then.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Dragging {
    grab_y: Px,
    top: Px,
}

/// The thumb's state between events: its reveal, the look the last frame
/// drew, where the pointer is, a drag in flight and who holds the press.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Bar {
    pub(crate) reveal: Reveal,
    /// What the last frame drew: a press is tested against it, and the
    /// loop owes a frame when the reveal's look leaves it.
    pub(crate) drawn: Look,
    pointer: Place,
    drag: Option<Dragging>,
    grab: Grab,
    scroller: Scroller,
}

impl Bar {
    pub(crate) fn new() -> Bar {
        Bar {
            reveal: Reveal::HIDDEN,
            drawn: Look::HIDDEN,
            pointer: Place::Grid,
            drag: None,
            grab: Grab::Free,
            scroller: Scroller::Overlay,
        }
    }

    /// The style the thumb is drawn in.
    pub(crate) fn scroller(&self) -> Scroller {
        self.scroller
    }

    /// The look at `now`, as the next frame draws it.
    pub(crate) fn look(&self, now: HostTime) -> Look {
        self.reveal.look_under(self.scroller, now)
    }

    /// **A turn of the loop at `now`**: the hold and the fade stepped, the
    /// hover re-read from where the pointer last was and where the window
    /// is now -- a wheel, a key or a paste moves it without the pointer --
    /// and whether the look now differs from the one drawn, or a fade is
    /// running, which owes a frame.
    pub(crate) fn turn(&mut self, now: HostTime, term: &Term<'_>) -> Owes {
        let drag = match self.drag {
            Some(_) => Drag::Dragging,
            None => Drag::Idle,
        };
        self.reveal
            .tick(now, self.pointer, term.window_back(), drag);
        let fading = self.reveal.pending() == crate::reveal::Pending::EveryFrame;
        match fading || self.look(now) != self.drawn {
            true => Owes::Frame,
            false => Owes::Nothing,
        }
    }

    /// When the look next changes without input: a hold's end.
    pub(crate) fn deadline(&self) -> Option<HostTime> {
        match self.reveal.pending() {
            crate::reveal::Pending::At(t) => Some(t),
            crate::reveal::Pending::EveryFrame | crate::reveal::Pending::Nothing => None,
        }
    }

    /// **One pointer event**, before the seat sees it: whether the
    /// scrollbar took it, and whether a frame is owed. A drag or a jump
    /// moves the primary's window (`Term::set_window_back`) and reveals.
    pub(crate) fn input(
        &mut self,
        term: &mut Term<'_>,
        e: Event,
        f: Frame,
        now: HostTime,
    ) -> (Took, Owes) {
        if term.scrollbar_shown() == ScrollbarShown::Hidden {
            self.pointer = Place::Grid;
            self.drag = None;
            return (Took::No, Owes::Nothing);
        }
        let px = |v: f64| Px::of(u32::try_from(v.max(0.0) as i64).unwrap_or(0));
        let (x, y) = match e {
            Event::Moved { x, y, .. } | Event::Pressed { x, y, .. } | Event::Released { x, y } => {
                (px(x), px(y))
            }
            Event::Left => (Px::of(0), Px::of(0)),
        };
        let win = crate::effects::window_of(term);
        let strip = place(x, f.size.0, f.widths);
        self.pointer = match e {
            Event::Left => Place::Grid,
            _ => strip,
        };
        let mut owes = Owes::Nothing;
        // The hover, on every event but a drag's, which holds its own.
        if self.drag.is_none() && self.scroller == Scroller::Overlay {
            let motion = match e {
                Event::Left => Motion::Leaving,
                _ => Motion::Within,
            };
            let before = self.reveal;
            self.reveal
                .hover(crate::reveal::over(strip, win.back, motion), now);
            if self.reveal != before {
                owes = Owes::Frame;
            }
        }
        // A press is tested against what is on the screen: the look the
        // last frame drew.
        let look = self.drawn;
        let knob = thumb(win, f.size, f.cell_h, f.widths, look.hover);
        let top = match e {
            Event::Pressed { left, .. } => {
                let button = match left {
                    Left::Yes => Button::Left,
                    Left::No => Button::Other,
                };
                let on = knob.map(|t| match t.covers_y(y) {
                    true => Knob::On,
                    false => Knob::Off,
                });
                let takes = crate::reveal::press(self.scroller, strip, look.shown, on, button);
                self.grab = takes.grabs();
                match (takes, knob) {
                    (Takes::Cells, _) => return (Took::No, owes),
                    (Takes::Drag, Some(t)) => {
                        self.drag = Some(Dragging {
                            grab_y: y,
                            top: t.top,
                        });
                        self.reveal.hover(Over::Strip, now);
                        return (Took::Yes, Owes::Frame);
                    }
                    (Takes::Jump, Some(t)) => {
                        Px::of(y.value().saturating_sub(t.height.value() / 2))
                    }
                    _ => return (Took::Yes, owes),
                }
            }
            Event::Moved { held, .. } => {
                let Some(d) = self.drag else {
                    // A press the scrollbar took that is not a drag keeps
                    // its drags too; motion with nothing taken is the
                    // cells', or the legacy gutter's.
                    return match (self.grab, held) {
                        (Grab::Bar, _) => (Took::Yes, owes),
                        (Grab::Free, Held::Yes) => (Took::No, owes),
                        (Grab::Free, Held::No) => match crate::reveal::motion(self.scroller, strip)
                        {
                            Takes::Cells => (Took::No, owes),
                            _ => (Took::Yes, owes),
                        },
                    };
                };
                // The window can no longer scroll -- the alternate screen
                // came up mid-drag -- so the drag ends; the press stays
                // taken until its release.
                if win.retained.index() == 0 {
                    self.drag = None;
                    return (Took::Yes, owes);
                }
                let moved = i64::from(y.value()) - i64::from(d.grab_y.value());
                Px::of(u32::try_from((i64::from(d.top.value()) + moved).max(0)).unwrap_or(0))
            }
            Event::Released { .. } => {
                let grabbed = core::mem::replace(&mut self.grab, Grab::Free);
                if self.drag.take().is_some() && self.scroller == Scroller::Overlay {
                    // Still over the strip it stays widened; anywhere else
                    // it is a reveal that holds and fades.
                    let over = crate::reveal::over(strip, win.back, Motion::Within);
                    self.reveal.hover(over, now);
                    self.reveal.reveal(now);
                    owes = Owes::Frame;
                }
                return match grabbed {
                    Grab::Bar => (Took::Yes, owes),
                    Grab::Free => (Took::No, owes),
                };
            }
            Event::Left => return (Took::No, owes),
        };
        let back = back_for(win, f.size, f.cell_h, top);
        self.reveal.reveal(now);
        if back.index() != term.primary_window_back().index() {
            term.set_window_back(back);
        }
        (Took::Yes, Owes::Frame)
    }
}

/// Whether a frame is owed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    Nothing,
    Frame,
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, Rows, WinSize};
    use core::time::Duration;

    const WS: WinSize = WinSize {
        ws_row: 10,
        ws_col: 20,
    };

    fn frame() -> Frame {
        Frame {
            size: (Px::of(200), Px::of(100)),
            cell_h: Px::of(10),
            widths: crate::reveal::widths(crate::reveal::Scale(1.0)),
        }
    }

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    const MS: Duration = Duration::from_millis(1);

    /// A moment `d` into the test's clock.
    fn at(d: Duration) -> HostTime {
        HostTime::of_secs(100.0).unwrap().plus(d)
    }

    /// Fifty lines, so forty are behind the screen.
    fn with_history(t: &mut Term<'_>) {
        for i in 0..50 {
            feed(t, format!("{i}\r\n").as_bytes());
        }
    }

    /// **A press on the knob drags the window through the history, and the
    /// whole gesture is the scrollbar's**: the press, every drag and the
    /// release are taken, the window follows the pointer, and the release
    /// leaves a reveal that holds. The control is a press over the cells,
    /// which the scrollbar never takes. Perturbation: return `Took::No`
    /// for a drag's move -- the drag's assertion fails.
    #[test]
    fn a_drag_on_the_knob_moves_the_window_and_keeps_the_gesture() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        with_history(&mut t);
        t.set_window_back(Rows::of_count(20));
        let mut b = Bar::new();
        let f = frame();
        // Scrolled back: the pointer over the strip widens the thumb.
        let (took, owes) = b.input(
            &mut t,
            Event::Moved {
                x: 195.0,
                y: 50.0,
                held: Held::No,
            },
            f,
            at(Duration::ZERO),
        );
        assert_eq!((took, owes), (Took::No, Owes::Frame), "a hover");
        b.drawn = b.look(at(Duration::ZERO));
        let win = crate::effects::window_of(&t);
        let knob = thumb(win, f.size, f.cell_h, f.widths, b.drawn.hover).expect("a knob");
        let y = f64::from(knob.top.value() + 1);
        let press = Event::Pressed {
            x: 195.0,
            y,
            left: Left::Yes,
        };
        assert_eq!(b.input(&mut t, press, f, at(MS * 1)).0, Took::Yes);
        let drag = Event::Moved {
            x: 120.0,
            y: y + 30.0,
            held: Held::Yes,
        };
        assert_eq!(
            b.input(&mut t, drag, f, at(MS * 2)),
            (Took::Yes, Owes::Frame)
        );
        assert!(t.primary_window_back().index() < 20, "dragged down");
        let up = Event::Released { x: 120.0, y };
        assert_eq!(b.input(&mut t, up, f, at(MS * 3)).0, Took::Yes);
        assert_eq!(
            b.reveal.pending(),
            crate::reveal::Pending::At(at(MS * 3).plus(crate::reveal::HOLD))
        );
        // The control: a press over the cells is not the scrollbar's.
        let cells = Event::Pressed {
            x: 20.0,
            y: 20.0,
            left: Left::Yes,
        };
        assert_eq!(b.input(&mut t, cells, f, at(MS * 4)).0, Took::No);
    }

    /// **A press in the strip off the knob jumps there; with nothing drawn
    /// the strip is the cells'; hidden by `?30` nothing is taken.**
    /// Perturbation: test the reveal's look rather than the drawn one --
    /// the press before any frame drew the thumb is taken and the first
    /// assertion fails.
    #[test]
    fn a_jump_needs_a_drawn_thumb_and_a_shown_scrollbar() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        with_history(&mut t);
        let f = frame();
        let mut b = Bar::new();
        let top = Event::Pressed {
            x: 195.0,
            y: 1.0,
            left: Left::Yes,
        };
        b.reveal.reveal(at(Duration::ZERO));
        assert_eq!(
            b.input(&mut t, top, f, at(Duration::ZERO)).0,
            Took::No,
            "not drawn yet"
        );
        b.input(&mut t, Event::Released { x: 195.0, y: 1.0 }, f, at(MS * 1));
        b.drawn = b.look(at(MS * 1));
        assert_eq!(
            b.input(&mut t, top, f, at(MS * 2)),
            (Took::Yes, Owes::Frame)
        );
        assert!(t.primary_window_back().index() > 0, "jumped into history");
        b.input(&mut t, Event::Released { x: 195.0, y: 1.0 }, f, at(MS * 3));
        // Another button in the strip is the cells', as on macOS.
        let right = Event::Pressed {
            x: 195.0,
            y: 1.0,
            left: Left::No,
        };
        assert_eq!(b.input(&mut t, right, f, at(MS * 3)).0, Took::No, "right");
        b.input(&mut t, Event::Released { x: 195.0, y: 1.0 }, f, at(MS * 3));
        // Leaving the window is not over the strip.
        assert_eq!(b.input(&mut t, Event::Left, f, at(MS * 3)).0, Took::No);
        feed(&mut t, b"\x1b[?30l");
        assert_eq!(
            b.input(&mut t, top, f, at(MS * 4)).0,
            Took::No,
            "?30 hid it"
        );
    }

    /// **The loop's two questions**: a hold names its end and owes no frame
    /// while the look is the drawn one; a fade owes every frame; at rest
    /// nothing. Perturbation: answer `Owes::Nothing` while fading -- the
    /// fade's assertion fails.
    #[test]
    fn a_turn_owes_a_frame_while_the_look_moves() {
        let mut s = Owned::new(&WS);
        let t = Term::new(s.storage(), &WS);
        let mut b = Bar::new();
        assert_eq!(b.turn(at(Duration::ZERO), &t), Owes::Nothing);
        assert_eq!(b.deadline(), None);
        b.reveal.reveal(at(Duration::ZERO));
        assert_eq!(
            b.turn(at(Duration::ZERO), &t),
            Owes::Frame,
            "shown: a frame"
        );
        b.drawn = b.look(at(Duration::ZERO));
        assert_eq!(b.turn(at(MS * 10), &t), Owes::Nothing, "held");
        assert_eq!(
            b.deadline(),
            Some(at(Duration::ZERO).plus(crate::reveal::HOLD))
        );
        let fading = 500 + 100;
        assert_eq!(b.turn(at(MS * fading), &t), Owes::Frame, "fading");
        b.drawn = b.look(at(MS * 2000));
        assert_eq!(b.turn(at(MS * 2000), &t), Owes::Nothing, "gone");
    }
}
