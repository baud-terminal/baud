//! **The pointer as the compositor reports it, a frame at a time.**
//! `wl_pointer` (`[[source: wayland]]`) sends enter, leave, motion, button
//! and the axis events, and from version 5 closes each group with `frame`:
//! "A client is expected to accumulate the data in all events within the
//! frame before proceeding." This gathers a frame's events and hands them
//! over when it ends, the axis pieces combined into one scroll; below
//! version 5 there is no `frame`, and each event is its own.
//!
//! Only the vertical axis scrolls: the core's mouse reports have no
//! buttons 6 and 7, and the history has no columns to move across.
//! `axis_source` is not read: whether a frame's scroll is detents or a
//! length is whether it carries `axis_value120` or `axis_discrete`, which
//! only a wheel sends, and a touchpad's lift is its `axis_stop`.
//!
//! What the pointer shows is `cursor-shape-v1`'s (`[[source:
//! wp-cursor-shape-v1]]`): [`shape`] names the shape for a program's
//! `OSC 22`.

use baud_domain::{EventMillis, PointerShape};
use baud_wayland::protocol;

/// A point in the surface's own coordinates, the motion events' unit. With
/// no buffer scale set, one is one pixel of the buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct At {
    pub x: f64,
    pub y: f64,
}

/// `BTN_LEFT`, `BTN_RIGHT` and `BTN_MIDDLE`, the codes the button event
/// carries: "a button code as defined in the Linux kernel's
/// linux/input-event-codes.h header file" (`[[source:
/// linux-input-event-codes]]`, 0x110 to 0x112).
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// A button event's code, as the wire carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCode(pub u32);

impl EventCode {
    /// `BTN_SIDE`, 0x113, the first button past the three (`[[source:
    /// linux-input-event-codes]]` line 359): the one DEC's locator reports
    /// as its fourth.
    pub const SIDE: EventCode = EventCode(0x113);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
    /// A side or extra button, by its code.
    Other(EventCode),
}

impl Button {
    pub(crate) fn of(code: EventCode) -> Button {
        match code.0 {
            BTN_LEFT => Button::Left,
            BTN_MIDDLE => Button::Middle,
            BTN_RIGHT => Button::Right,
            _ => Button::Other(code),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Pressed,
    Released,
}

/// One frame's vertical scroll, positive toward the axis's positive end --
/// down the page, later lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    /// Wheel detents: `axis_value120` over 120, "each multiple of 120
    /// representing one logical scroll step (a wheel detent)", or
    /// `axis_discrete`'s "number of steps" from a pointer below version 8.
    Steps(f64),
    /// A length in surface coordinates from a device with no detents --
    /// a touchpad's fingers, a continuous source: the `axis` value, "the
    /// length of a vector along the specified axis".
    Distance(f64),
}

/// What the pointer did, in the order the compositor sent it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerEvent {
    Entered(At),
    Left,
    Moved(At),
    /// A button and its state; `time` is the event's millisecond stamp,
    /// "with an undefined base", so only differences between stamps mean
    /// anything.
    Button {
        button: Button,
        press: Press,
        time: EventMillis,
    },
    Scroll(Scroll),
    /// `axis_stop` on the vertical axis: "Any wl_pointer.axis events with
    /// the same axis_source after this event should be considered as the
    /// start of a new axis motion."
    ScrollStopped,
}

/// Whether the pointer closes its groups with `frame`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Framing {
    /// Version 5 or later: events wait for the frame.
    #[default]
    Framed,
    /// Below 5: each event stands alone.
    Single,
}

/// An axis event's `axis`, as the wire carries it: `vertical_scroll` or
/// `horizontal_scroll`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisCode(pub u32);

impl AxisCode {
    fn vertical(self) -> bool {
        self.0 == protocol::wl_pointer::axis::VERTICAL_SCROLL
    }
}

/// One axis event's part of a frame's scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// `axis`: a length in surface coordinates.
    Length(f64),
    /// `axis_value120`: detents in 120ths, from version 8.
    Value120(i32),
    /// `axis_discrete`: whole detents, versions 5 to 7.
    Discrete(i32),
    /// `axis_stop`, from version 5.
    Stop,
}

/// Whether an `axis_stop` came in the frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Motion {
    #[default]
    Going,
    Stopped,
}

/// The vertical axis's pieces within one frame. "When multiple
/// wl_pointer.axis events occur within the same frame, the motion vector is
/// the combined motion of all events", so lengths and steps are summed.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Axis {
    length: Option<f64>,
    value120: Option<i32>,
    discrete: Option<i32>,
    motion: Motion,
}

impl Axis {
    /// The scroll the frame carried: its detents where the compositor gave
    /// them, its length otherwise.
    fn scroll(&self) -> Option<Scroll> {
        match (self.value120, self.discrete, self.length) {
            (Some(v), _, _) => Some(Scroll::Steps(f64::from(v) / 120.0)),
            (None, Some(d), _) => Some(Scroll::Steps(f64::from(d))),
            (None, None, Some(l)) => Some(Scroll::Distance(l)),
            (None, None, None) => None,
        }
    }
}

/// One frame of pointer events, gathered until `frame` ends it.
#[derive(Debug)]
pub struct Gather {
    framing: Framing,
    events: Vec<PointerEvent>,
    axis: Axis,
}

impl Gather {
    pub fn new(framing: Framing) -> Gather {
        Gather {
            framing,
            events: Vec::new(),
            axis: Axis::default(),
        }
    }

    /// An enter, leave, motion or button event: kept for the frame, or
    /// handed over now where there are no frames.
    pub fn event(&mut self, e: PointerEvent, out: &mut impl FnMut(PointerEvent)) {
        self.events.push(e);
        if self.framing == Framing::Single {
            self.frame(out);
        }
    }

    /// **One axis event on `axis`**, the vertical one kept: lengths and
    /// `axis_value120`'s 120ths summed; "A wl_pointer.frame must not
    /// contain more than one axis_discrete event per axis type", so a
    /// second replaces the first. Below version 5 a length is its own frame,
    /// and the rest come only from version 5 on.
    pub fn axis(&mut self, axis: AxisCode, piece: Piece, out: &mut impl FnMut(PointerEvent)) {
        if axis.vertical() {
            let a = &mut self.axis;
            match piece {
                Piece::Length(l) => a.length = Some(a.length.unwrap_or(0.0) + l),
                Piece::Value120(v) => a.value120 = Some(a.value120.unwrap_or(0).saturating_add(v)),
                Piece::Discrete(d) => a.discrete = Some(d),
                Piece::Stop => a.motion = Motion::Stopped,
            }
        }
        if self.framing == Framing::Single {
            self.frame(out);
        }
    }

    /// `frame`: everything gathered since the last, in order, the scroll
    /// after the rest and the stop after the scroll -- "When a
    /// wl_pointer.axis and a wl_pointer.axis_stop event occur within the
    /// same frame, this indicates that axis movement in one axis has
    /// stopped but continues in the other axis", so a frame's own scroll is
    /// the motion before its stop.
    pub fn frame(&mut self, out: &mut impl FnMut(PointerEvent)) {
        for e in self.events.drain(..) {
            out(e);
        }
        if let Some(s) = self.axis.scroll() {
            out(PointerEvent::Scroll(s));
        }
        if self.axis.motion == Motion::Stopped {
            out(PointerEvent::ScrollStopped);
        }
        self.axis = Axis::default();
    }
}

/// A `wp_cursor_shape_device_v1.shape` value, as `set_shape` carries it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorShape(pub u32);

/// **The cursor-shape a program's `OSC 22` asks for, or `text` when it has
/// asked for none.** Wayland leaves the image undefined on enter, "a client
/// should respond to this event by setting an appropriate pointer image",
/// so the state with no `OSC 22` must name one: xterm's, the `pointerShape`
/// resource's default "xterm" (`[[source: xterm-man]]`), which ctlseqs says
/// an empty or unknown `Pt` returns to -- the I-beam, `text`. Each of xterm's
/// 77 names takes the shape whose meaning is nearest, where
/// `cursor-shape-v1` has one: the text cursor, the hands, the watch, the
/// question arrow, the crosses and the plus, the fleur, the refusals, and
/// the side, corner and double-arrow resize names; the rest are `default`,
/// the arrow. Jev's picks: `text` at 0.94 both orders; the nearest-meaning
/// table a lean, 0.74 and 0.94.
pub fn shape(asked: Option<PointerShape>) -> CursorShape {
    use PointerShape as S;
    use protocol::wp_cursor_shape_device_v1::shape as C;
    let Some(s) = asked else {
        return CursorShape(C::TEXT);
    };
    CursorShape(match s {
        S::Xterm => C::TEXT,
        S::Hand1 | S::Hand2 => C::POINTER,
        S::Watch => C::WAIT,
        S::QuestionArrow => C::HELP,
        S::Crosshair | S::Cross | S::CrossReverse | S::Tcross | S::DiamondCross => C::CROSSHAIR,
        S::Plus => C::CELL,
        S::Fleur => C::MOVE,
        S::XCursor | S::Pirate => C::NOT_ALLOWED,
        S::TopSide => C::N_RESIZE,
        S::BottomSide => C::S_RESIZE,
        S::LeftSide => C::W_RESIZE,
        S::RightSide => C::E_RESIZE,
        S::TopLeftCorner => C::NW_RESIZE,
        S::TopRightCorner => C::NE_RESIZE,
        S::BottomLeftCorner => C::SW_RESIZE,
        S::BottomRightCorner => C::SE_RESIZE,
        S::SbHDoubleArrow => C::EW_RESIZE,
        S::SbVDoubleArrow | S::DoubleArrow => C::NS_RESIZE,
        _ => C::DEFAULT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gathered(
        g: &mut Gather,
        f: impl FnOnce(&mut Gather, &mut dyn FnMut(PointerEvent)),
    ) -> Vec<PointerEvent> {
        let mut got = Vec::new();
        f(g, &mut |e| got.push(e));
        got
    }

    /// **A frame's axis pieces are one scroll**: two lengths summed, the
    /// detents preferred to the length they come with, and the horizontal
    /// axis dropped.
    #[test]
    fn a_frame_sums_its_axis_and_prefers_detents() {
        let v = AxisCode(protocol::wl_pointer::axis::VERTICAL_SCROLL);
        let h = AxisCode(protocol::wl_pointer::axis::HORIZONTAL_SCROLL);
        let mut g = Gather::new(Framing::Framed);
        let got = gathered(&mut g, |g, out| {
            g.axis(v, Piece::Length(3.0), &mut |e| out(e));
            g.axis(v, Piece::Length(4.5), &mut |e| out(e));
            g.axis(h, Piece::Length(9.0), &mut |e| out(e));
            g.frame(&mut |e| out(e));
        });
        assert_eq!(got, [PointerEvent::Scroll(Scroll::Distance(7.5))]);
        let got = gathered(&mut g, |g, out| {
            g.axis(v, Piece::Value120(60), &mut |e| out(e));
            g.axis(v, Piece::Length(15.0), &mut |e| out(e));
            g.frame(&mut |e| out(e));
        });
        assert_eq!(got, [PointerEvent::Scroll(Scroll::Steps(0.5))]);
        let got = gathered(&mut g, |g, out| {
            g.axis(v, Piece::Discrete(-2), &mut |e| out(e));
            g.axis(v, Piece::Length(-30.0), &mut |e| out(e));
            g.axis(h, Piece::Value120(120), &mut |e| out(e));
            g.frame(&mut |e| out(e));
        });
        assert_eq!(got, [PointerEvent::Scroll(Scroll::Steps(-2.0))]);
    }

    /// **Nothing leaves before the frame**, then all of it in order, and a
    /// stop after the frame's scroll; below version 5 each event at once.
    #[test]
    fn events_wait_for_the_frame_unless_there_are_none() {
        let v = AxisCode(protocol::wl_pointer::axis::VERTICAL_SCROLL);
        let at = At { x: 1.0, y: 2.0 };
        let mut g = Gather::new(Framing::Framed);
        let got = gathered(&mut g, |g, out| {
            g.event(PointerEvent::Moved(at), &mut |e| out(e));
            g.axis(v, Piece::Stop, &mut |e| out(e));
            g.axis(v, Piece::Length(1.0), &mut |e| out(e));
        });
        assert!(got.is_empty(), "held to the frame: {got:?}");
        let got = gathered(&mut g, |g, out| g.frame(&mut |e| out(e)));
        assert_eq!(
            got,
            [
                PointerEvent::Moved(at),
                PointerEvent::Scroll(Scroll::Distance(1.0)),
                PointerEvent::ScrollStopped
            ]
        );
        let mut g = Gather::new(Framing::Single);
        let got = gathered(&mut g, |g, out| {
            g.event(PointerEvent::Moved(at), &mut |e| out(e));
            g.axis(v, Piece::Length(2.0), &mut |e| out(e));
        });
        assert_eq!(
            got,
            [
                PointerEvent::Moved(at),
                PointerEvent::Scroll(Scroll::Distance(2.0))
            ]
        );
    }

    #[test]
    fn the_buttons_are_the_kernels_codes() {
        assert_eq!(Button::of(EventCode(0x110)), Button::Left);
        assert_eq!(Button::of(EventCode(0x111)), Button::Right);
        assert_eq!(Button::of(EventCode(0x112)), Button::Middle);
        assert_eq!(
            Button::of(EventCode(0x113)),
            Button::Other(EventCode(0x113))
        );
    }

    /// **No `OSC 22` is the I-beam, and each name its nearest shape**,
    /// the arrow where cursor-shape has none.
    #[test]
    fn each_name_takes_its_nearest_shape() {
        use protocol::wp_cursor_shape_device_v1::shape as C;
        assert_eq!(shape(None), CursorShape(C::TEXT));
        assert_eq!(shape(Some(PointerShape::Xterm)), CursorShape(C::TEXT));
        assert_eq!(shape(Some(PointerShape::Hand2)), CursorShape(C::POINTER));
        assert_eq!(shape(Some(PointerShape::Watch)), CursorShape(C::WAIT));
        assert_eq!(
            shape(Some(PointerShape::BottomRightCorner)),
            CursorShape(C::SE_RESIZE)
        );
        assert_eq!(
            shape(Some(PointerShape::SbVDoubleArrow)),
            CursorShape(C::NS_RESIZE)
        );
        assert_eq!(shape(Some(PointerShape::LeftPtr)), CursorShape(C::DEFAULT));
        assert_eq!(shape(Some(PointerShape::Gumby)), CursorShape(C::DEFAULT));
        // Every value is one of the version-1 entries, 1 to 34: a shape
        // the bound version lacks is a protocol error. Twenty-five names
        // have a shape of their own; the other 52 are the arrow.
        let mut own = 0;
        for s in PointerShape::ALL {
            let CursorShape(c) = shape(Some(s));
            assert!((C::DEFAULT..=C::ZOOM_OUT).contains(&c), "{s:?}: {c}");
            own += usize::from(c != C::DEFAULT);
        }
        assert_eq!(own, 25);
    }
}
