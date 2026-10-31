//! **The scrollbar's press rules, once, for both drivers**: who a pointer
//! event over the strip belongs to and what it does, over the pure parts
//! -- `reveal.rs` for when the thumb is seen and who a press belongs to,
//! `baud_render`'s `thumb`, `place` and `back_for` for the geometry. A press
//! on the knob drags the window through the history, a press in the strip
//! off it jumps there, the pointer over the strip while the window is
//! scrolled back widens and holds the thumb under the overlay style, and a
//! press the scrollbar takes keeps its drags and its release, so no one
//! else sees a release without its press. Hidden by `?30`, nothing is drawn
//! and nothing takes a press. The legacy style's gutter is never cells.
//!
//! [`press`] is the rule; the macOS driver calls it from its view callback
//! with AppKit's events in frame pixels, and the Linux driver through
//! [`Bar`], which keeps the thumb's state between events. The Linux window
//! has no system scroller setting to follow, so its style is the overlay
//! one, which lies over the grid and gives up no column. Pure, over a real
//! `Term`, so the tests run on every platform.

use crate::reveal::{Button, Grab, Knob, Motion, Over, Reveal, Takes};
use baud_core::Term;
use baud_domain::{HostTime, ScrollbarShown};
use baud_font::Px;
use baud_render::{Look, Place, Scroller, Widths, back_for, place, thumb};

/// The frame the thumb is drawn in: its size in pixels, the cell's height,
/// and the scroller's widths.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Frame {
    pub(crate) size: (Px, Px),
    pub(crate) cell_h: Px,
    pub(crate) widths: Widths,
}

/// What a pointer event is, as the press rules read it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// A move; `held` when a button is down -- a drag.
    Moved {
        held: Held,
    },
    Entered,
    Pressed {
        left: Left,
    },
    Released,
    /// The pointer left the view.
    Left,
}

/// A pointer event at a frame pixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct At {
    pub(crate) x: Px,
    pub(crate) y: Px,
    pub(crate) kind: Kind,
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

/// Whether the scrollbar took an event, so no one else may see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Took {
    Yes,
    No,
}

/// A drag of the knob: where the pointer pressed and where the knob's top
/// was then, in frame pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Dragging {
    grab_y: Px,
    top: Px,
}

/// **The press the scrollbar holds between events**: a drag in flight and
/// whether the press in progress is the scrollbar's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Hold {
    drag: Option<Dragging>,
    grab: Grab,
}

impl Hold {
    pub(crate) const FREE: Hold = Hold {
        drag: None,
        grab: Grab::Free,
    };

    /// Whether a drag is in flight, for the reveal's tick.
    pub(crate) fn drag(&self) -> crate::reveal::Drag {
        match self.drag {
            Some(_) => crate::reveal::Drag::Dragging,
            None => crate::reveal::Drag::Idle,
        }
    }

    /// A drag in flight ends where it is: `?30` hid the scrollbar.
    #[cfg(target_os = "macos")]
    pub(crate) fn end_drag(&mut self) {
        self.drag = None;
    }
}

/// What the rules read besides the event: the style, whether a program
/// shows the scrollbar (`?30`), the frame, the look the last frame drew --
/// a press is tested against what is on the screen -- and the clock, which
/// the macOS driver may not have yet.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Ctx {
    pub(crate) scroller: Scroller,
    pub(crate) shown: ScrollbarShown,
    pub(crate) frame: Frame,
    pub(crate) drawn: Look,
    pub(crate) now: Option<HostTime>,
}

/// Whether the hover changed the reveal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Hover {
    Changed,
    Same,
}

/// Whether a jump or a drag moved the primary's window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moved {
    Yes,
    No,
}

/// What the event did besides the hover.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Did {
    Nothing,
    /// A press on the knob started a drag.
    Started,
    /// The overlay's drag ended: still over the strip the thumb stays
    /// widened, anywhere else it is a reveal that holds and fades.
    Ended,
    /// A jump or a drag's move set the window and revealed the thumb.
    Scrolled(Moved),
}

/// What [`press`] answers: whether the scrollbar took the event, and what
/// it changed, for the driver to owe a frame by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Answer {
    pub(crate) took: Took,
    pub(crate) hover: Hover,
    pub(crate) did: Did,
}

impl Answer {
    fn of(took: Took, hover: Hover, did: Did) -> Answer {
        Answer { took, hover, did }
    }
}

/// **One pointer event, before anyone else but DEC's locator sees it**:
/// whether the scrollbar takes it, the hover it sets, a drag or a jump it
/// makes (`Term::set_window_back`), and where the pointer is for the
/// reveal's tick (`pointer`).
pub(crate) fn press(
    hold: &mut Hold,
    reveal: &mut Reveal,
    pointer: &mut Place,
    term: &mut Term<'_>,
    e: At,
    c: Ctx,
) -> Answer {
    let none = Answer::of(Took::No, Hover::Same, Did::Nothing);
    // A program hid it (`?30`): nothing there takes a press.
    if c.shown == ScrollbarShown::Hidden {
        *pointer = Place::Grid;
        hold.drag = None;
        return none;
    }
    let (x, y) = (e.x, e.y);
    let f = c.frame;
    let win = crate::effects::window_of(term);
    let strip = place(x, f.size.0, f.widths);
    *pointer = match e.kind {
        Kind::Left => Place::Grid,
        _ => strip,
    };
    let Some(now) = c.now else {
        return none;
    };
    // The hover, on every event but a drag's, which holds its own; the
    // legacy scroller has none.
    let mut hover = Hover::Same;
    if hold.drag.is_none() && c.scroller == Scroller::Overlay {
        let motion = match e.kind {
            Kind::Left => Motion::Leaving,
            _ => Motion::Within,
        };
        let before = *reveal;
        reveal.hover(crate::reveal::over(strip, win.back, motion), now);
        if *reveal != before {
            hover = Hover::Changed;
        }
    }
    let knob = thumb(win, f.size, f.cell_h, f.widths, c.drawn.hover);
    let top = match e.kind {
        Kind::Pressed { left } => {
            let button = match left {
                Left::Yes => Button::Left,
                Left::No => Button::Other,
            };
            let on = knob.map(|t| match t.covers_y(y) {
                true => Knob::On,
                false => Knob::Off,
            });
            let takes = crate::reveal::press(c.scroller, strip, c.drawn.shown, on, button);
            hold.grab = takes.grabs();
            match (takes, knob) {
                (Takes::Cells, _) => return Answer::of(Took::No, hover, Did::Nothing),
                (Takes::Drag, Some(t)) => {
                    hold.drag = Some(Dragging {
                        grab_y: y,
                        top: t.top,
                    });
                    if c.scroller == Scroller::Overlay {
                        reveal.hover(Over::Strip, now);
                    }
                    return Answer::of(Took::Yes, hover, Did::Started);
                }
                (Takes::Jump, Some(t)) => Px::of(y.value().saturating_sub(t.height.value() / 2)),
                _ => return Answer::of(Took::Yes, hover, Did::Nothing),
            }
        }
        Kind::Moved { held: Held::Yes } => {
            let Some(d) = hold.drag else {
                // A press the scrollbar took that is not a drag -- a
                // jump, or the gutter's -- keeps its drags too.
                let took = match hold.grab {
                    Grab::Bar => Took::Yes,
                    Grab::Free => Took::No,
                };
                return Answer::of(took, hover, Did::Nothing);
            };
            // The window can no longer scroll -- the alternate screen came
            // up mid-drag, which keeps no history -- so the drag ends rather
            // than write the primary's window from a geometry with nothing
            // behind it. The press stays taken until its release.
            if win.retained.index() == 0 {
                hold.drag = None;
                return Answer::of(Took::Yes, hover, Did::Nothing);
            }
            let moved = i64::from(y.value()) - i64::from(d.grab_y.value());
            Px::of(u32::try_from((i64::from(d.top.value()) + moved).max(0)).unwrap_or(0))
        }
        Kind::Released => {
            let grabbed = core::mem::replace(&mut hold.grab, Grab::Free);
            let mut did = Did::Nothing;
            if hold.drag.take().is_some() && c.scroller == Scroller::Overlay {
                // The end of a drag: still over the strip it stays widened,
                // anywhere else it is a reveal that holds and fades.
                let over = crate::reveal::over(strip, win.back, Motion::Within);
                reveal.hover(over, now);
                reveal.reveal(now);
                did = Did::Ended;
            }
            let took = match grabbed {
                Grab::Bar => Took::Yes,
                Grab::Free => Took::No,
            };
            return Answer::of(took, hover, did);
        }
        // Motion with nothing pressed: the legacy gutter keeps it from a
        // program tracking every motion.
        Kind::Moved { held: Held::No } => {
            let took = match crate::reveal::motion(c.scroller, strip) {
                Takes::Cells => Took::No,
                _ => Took::Yes,
            };
            return Answer::of(took, hover, Did::Nothing);
        }
        Kind::Entered | Kind::Left => return Answer::of(Took::No, hover, Did::Nothing),
    };
    let back = back_for(win, f.size, f.cell_h, top);
    reveal.reveal(now);
    let moved = match back.index() != term.primary_window_back().index() {
        true => {
            term.set_window_back(back);
            Moved::Yes
        }
        false => Moved::No,
    };
    Answer::of(Took::Yes, hover, Did::Scrolled(moved))
}

/// **Who a left press in `strip` would go to now**, by [`press`]'s rule:
/// the scrollbar's when it would take the press -- under the overlay style
/// while any of the thumb is drawn and there is history, under the legacy
/// one always -- else the cells'. On the knob or off it the press is the
/// scrollbar's either way, so the pointer's height is not asked. Read for
/// the pointer over a link, which a Command or Ctrl click under the strip
/// does not open.
pub(crate) fn left_press(term: &Term<'_>, strip: Place, c: Ctx) -> crate::link::LeftPress {
    use crate::link::LeftPress;
    if c.shown == ScrollbarShown::Hidden || c.now.is_none() {
        return LeftPress::Cells;
    }
    let f = c.frame;
    let win = crate::effects::window_of(term);
    let knob = thumb(win, f.size, f.cell_h, f.widths, c.drawn.hover).map(|_| Knob::On);
    match crate::reveal::press(c.scroller, strip, c.drawn.shown, knob, Button::Left).grabs() {
        Grab::Bar => LeftPress::Scrollbar,
        Grab::Free => LeftPress::Cells,
    }
}

/// A pointer event as the Linux driver hands it to [`Bar::input`], at a
/// surface point: the Linux frame draws the grid from its first pixel at a
/// scale of one, so a surface point is a frame pixel.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Event {
    /// A move; `held` when a button is down.
    Moved {
        x: f64,
        y: f64,
        held: Held,
    },
    Entered {
        x: f64,
        y: f64,
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

/// **A seat's input as the scrollbar reads it**, or `None` for one it
/// never takes (the wheel): at the position the seat last had for a press
/// or a release, a move a drag while a button is down. The thumb's
/// geometry is in frame pixels, the grid's, so a surface point comes in
/// less `strip`, the exit-status strip at the frame's left.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn event_beside(beside: crate::seat::Beside, strip: f64) -> Option<Event> {
    use crate::gesture::Pressed;
    use crate::seat::Input;
    let at = beside.at.map(|(x, y)| (x - strip, y));
    Some(match (beside.input, at) {
        (Input::Entered(x, y), _) => Event::Entered { x: x - strip, y },
        (Input::Moved(x, y), _) => Event::Moved {
            x: x - strip,
            y,
            held: beside.held,
        },
        (Input::Pressed { button, .. }, Some((x, y))) => Event::Pressed {
            x,
            y,
            left: match button {
                Pressed::Left => Left::Yes,
                _ => Left::No,
            },
        },
        (Input::Released { .. }, Some((x, y))) => Event::Released { x, y },
        (Input::Left, _) => Event::Left,
        _ => return None,
    })
}

/// **The thumb's state between the Linux driver's events**: its reveal,
/// the look the last frame drew, where the pointer is and the press held.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Bar {
    pub(crate) reveal: Reveal,
    /// What the last frame drew: a press is tested against it, and the
    /// loop owes a frame when the reveal's look leaves it.
    pub(crate) drawn: Look,
    pointer: Place,
    hold: Hold,
    scroller: Scroller,
}

#[cfg(any(target_os = "linux", test))]
impl Bar {
    pub(crate) fn new() -> Bar {
        Bar {
            reveal: Reveal::HIDDEN,
            drawn: Look::HIDDEN,
            pointer: Place::Grid,
            hold: Hold::FREE,
            scroller: Scroller::Overlay,
        }
    }

    /// The style the thumb is drawn in.
    pub(crate) fn scroller(&self) -> Scroller {
        self.scroller
    }

    /// Who a left press where the pointer last was would go to at `now`,
    /// against the look the last frame drew ([`left_press`]).
    pub(crate) fn left_press(
        &self,
        term: &Term<'_>,
        f: Frame,
        now: HostTime,
    ) -> crate::link::LeftPress {
        let c = Ctx {
            scroller: self.scroller,
            shown: term.scrollbar_shown(),
            frame: f,
            drawn: self.drawn,
            now: Some(now),
        };
        left_press(term, self.pointer, c)
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
        self.reveal
            .tick(now, self.pointer, term.window_back(), self.hold.drag());
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

    /// **One pointer event**, after DEC's locator and before the seat's
    /// routing: whether the scrollbar took it ([`press`]), and whether a
    /// frame is owed -- the hover changed, a drag started or ended, or a
    /// jump or a drag set the window.
    pub(crate) fn input(
        &mut self,
        term: &mut Term<'_>,
        e: Event,
        f: Frame,
        now: HostTime,
    ) -> (Took, Owes) {
        let px = |v: f64| Px::of(u32::try_from(v.max(0.0) as i64).unwrap_or(0));
        let (x, y, kind) = match e {
            Event::Moved { x, y, held } => (px(x), px(y), Kind::Moved { held }),
            Event::Entered { x, y } => (px(x), px(y), Kind::Entered),
            Event::Pressed { x, y, left } => (px(x), px(y), Kind::Pressed { left }),
            Event::Released { x, y } => (px(x), px(y), Kind::Released),
            Event::Left => (Px::of(0), Px::of(0), Kind::Left),
        };
        let c = Ctx {
            scroller: self.scroller,
            shown: term.scrollbar_shown(),
            frame: f,
            drawn: self.drawn,
            now: Some(now),
        };
        let at = At { x, y, kind };
        let a = press(
            &mut self.hold,
            &mut self.reveal,
            &mut self.pointer,
            term,
            at,
            c,
        );
        let owes = match (a.hover, a.did) {
            (Hover::Same, Did::Nothing) => Owes::Nothing,
            _ => Owes::Frame,
        };
        (a.took, owes)
    }
}

/// Whether a frame is owed.
#[cfg(any(target_os = "linux", test))]
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

    /// **Who a left press would go to is what [`press`] does with one**:
    /// over every style, `?30` shown and hidden, the thumb drawn and not,
    /// with history and without, the clock known and not, in the strip and
    /// over the grid, on the knob and off it, [`left_press`] answers the
    /// scrollbar's exactly where a left press is taken. The control: both
    /// answers occur. Perturbation: drop the `?30` test from `left_press`
    /// -- the cases with the scrollbar hidden fail.
    #[test]
    fn a_left_press_is_the_scrollbars_where_press_takes_it() {
        use crate::link::LeftPress;
        let f = frame();
        let t0 = at(Duration::ZERO);
        let mut seen = (0, 0);
        for scroller in [Scroller::Overlay, Scroller::Legacy] {
            for hidden in [false, true] {
                for drawn in [false, true] {
                    for history in [false, true] {
                        for now in [Some(t0), None] {
                            for (x, y) in [(195, 1), (195, 90), (20, 1)] {
                                let mut s = Owned::new(&WS);
                                let mut t = Term::new(s.storage(), &WS);
                                if history {
                                    with_history(&mut t);
                                }
                                if hidden {
                                    feed(&mut t, b"\x1b[?30l");
                                }
                                let mut reveal = Reveal::HIDDEN;
                                reveal.reveal(t0);
                                let c = Ctx {
                                    scroller,
                                    shown: t.scrollbar_shown(),
                                    frame: f,
                                    drawn: match drawn {
                                        true => reveal.look_under(scroller, t0),
                                        false => Look::HIDDEN,
                                    },
                                    now,
                                };
                                let strip = place(Px::of(x), f.size.0, f.widths);
                                let left = left_press(&t, strip, c);
                                let e = At {
                                    x: Px::of(x),
                                    y: Px::of(y),
                                    kind: Kind::Pressed { left: Left::Yes },
                                };
                                let (mut hold, mut pointer) = (Hold::FREE, Place::Grid);
                                let took =
                                    press(&mut hold, &mut reveal, &mut pointer, &mut t, e, c).took;
                                let case = (scroller, hidden, drawn, history, now, x, y);
                                match left {
                                    LeftPress::Scrollbar => seen.0 += 1,
                                    LeftPress::Cells => seen.1 += 1,
                                }
                                assert_eq!(
                                    took == Took::Yes,
                                    left == LeftPress::Scrollbar,
                                    "{case:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert!(seen.0 > 0 && seen.1 > 0, "the control: {seen:?}");
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
        // Entering over the strip is the seat's, and leaving the window
        // is not over the strip.
        let entered = Event::Entered { x: 195.0, y: 1.0 };
        assert_eq!(b.input(&mut t, entered, f, at(MS * 3)).0, Took::No);
        assert_eq!(b.input(&mut t, Event::Left, f, at(MS * 3)).0, Took::No);
        feed(&mut t, b"\x1b[?30l");
        assert_eq!(
            b.input(&mut t, top, f, at(MS * 4)).0,
            Took::No,
            "?30 hid it"
        );
    }

    /// **The legacy scroller's gutter is never cells, and nothing hovers
    /// there**, the rule the macOS driver takes through [`press`]: motion
    /// over it with nothing pressed is taken, a right press is swallowed
    /// with its drags and its release, a left press on the knob drags and
    /// its release ends it with no reveal, and nothing changes a hover.
    /// The controls are the overlay's: the same motion is the cells', and
    /// its drag's release ends with a reveal. Perturbations: hover under
    /// the legacy style -- the first hover assertion fails; give the
    /// swallowed press's drag to the cells -- its assertion fails.
    #[test]
    fn the_legacy_gutter_is_never_cells_and_never_hovers() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        with_history(&mut t);
        t.set_window_back(Rows::of_count(20));
        let f = frame();
        // The hold, the reveal and the pointer between events.
        let mut st = (Hold::FREE, Reveal::HIDDEN, Place::Grid);
        let ctx = |scroller, drawn| Ctx {
            scroller,
            shown: ScrollbarShown::Shown,
            frame: f,
            drawn,
            now: Some(at(Duration::ZERO)),
        };
        let px = Px::of;
        let ev = |y: u32, kind| At {
            x: px(195),
            y: px(y),
            kind,
        };
        let legacy = ctx(Scroller::Legacy, Look::HIDDEN);
        let go = |st: &mut (Hold, Reveal, Place), t: &mut Term<'_>, e: At, c: Ctx| {
            press(&mut st.0, &mut st.1, &mut st.2, t, e, c)
        };
        let still = ev(50, Kind::Moved { held: Held::No });
        let a = go(&mut st, &mut t, still, legacy);
        assert_eq!((a.took, a.hover), (Took::Yes, Hover::Same), "legacy motion");
        let right = ev(50, Kind::Pressed { left: Left::No });
        assert_eq!(
            go(&mut st, &mut t, right, legacy).took,
            Took::Yes,
            "swallowed"
        );
        let drag = ev(60, Kind::Moved { held: Held::Yes });
        assert_eq!(
            go(&mut st, &mut t, drag, legacy).took,
            Took::Yes,
            "its drag"
        );
        let release = ev(60, Kind::Released);
        assert_eq!(
            go(&mut st, &mut t, release, legacy).took,
            Took::Yes,
            "its release"
        );
        let win = crate::effects::window_of(&t);
        let knob = thumb(win, f.size, f.cell_h, f.widths, Look::HIDDEN.hover).expect("a knob");
        let on = knob.top.value() + 1;
        let a = go(
            &mut st,
            &mut t,
            ev(on, Kind::Pressed { left: Left::Yes }),
            legacy,
        );
        assert_eq!((a.took, a.did), (Took::Yes, Did::Started));
        let a = go(
            &mut st,
            &mut t,
            ev(on + 30, Kind::Moved { held: Held::Yes }),
            legacy,
        );
        assert_eq!((a.hover, a.did), (Hover::Same, Did::Scrolled(Moved::Yes)));
        let a = go(&mut st, &mut t, ev(on + 30, Kind::Released), legacy);
        assert_eq!(
            (a.took, a.hover, a.did),
            (Took::Yes, Hover::Same, Did::Nothing),
            "no hover at the end"
        );
        // The controls, under the overlay style.
        let overlay = ctx(Scroller::Overlay, Look::HIDDEN);
        let a = go(&mut st, &mut t, still, overlay);
        assert_eq!((a.took, a.hover), (Took::No, Hover::Changed), "a hover");
        let entered = go(&mut st, &mut t, ev(50, Kind::Entered), overlay);
        assert_eq!(entered.took, Took::No, "entering is the seat's");
        let none = Ctx {
            now: None,
            ..overlay
        };
        st.2 = Place::Grid;
        let a = go(
            &mut st,
            &mut t,
            ev(50, Kind::Pressed { left: Left::Yes }),
            none,
        );
        assert_eq!(a.took, Took::No, "no clock yet");
        assert_eq!(st.2, Place::Strip, "but the pointer is placed");
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

    /// **A surface point reaches the thumb less the exit-status strip**,
    /// as the Linux driver's `thumb` hands it the seat's input: a move and
    /// an entry at their own points, a press and a release at the seat's,
    /// each `strip` to the left, and the wheel none of the thumb's.
    /// Perturbation: drop `- strip` from the move's arm, the entry's or
    /// the seat's point -- that arm's assertion fails.
    #[test]
    fn a_point_beside_reaches_the_thumb_less_the_strip() {
        use crate::gesture::Pressed;
        use crate::seat::{Beside, Input};
        let at = Some((107.0, 30.0));
        let of = |input, held| event_beside(Beside { input, at, held }, 7.0);
        let time = baud_domain::EventMillis::of_millis(5);
        assert_eq!(
            of(Input::Moved(57.0, 40.0), Held::Yes),
            Some(Event::Moved {
                x: 50.0,
                y: 40.0,
                held: Held::Yes
            })
        );
        assert_eq!(
            of(Input::Entered(17.0, 20.0), Held::No),
            Some(Event::Entered { x: 10.0, y: 20.0 })
        );
        let button = Pressed::Left;
        assert_eq!(
            of(Input::Pressed { button, time }, Held::No),
            Some(Event::Pressed {
                x: 100.0,
                y: 30.0,
                left: Left::Yes
            })
        );
        let button = Pressed::Right;
        assert_eq!(
            of(Input::Pressed { button, time }, Held::No),
            Some(Event::Pressed {
                x: 100.0,
                y: 30.0,
                left: Left::No
            })
        );
        assert_eq!(
            of(Input::Released { button, time }, Held::No),
            Some(Event::Released { x: 100.0, y: 30.0 })
        );
        assert_eq!(of(Input::Left, Held::No), Some(Event::Left));
        assert_eq!(of(Input::Steps(1.0), Held::No), None, "the wheel");
    }
}
