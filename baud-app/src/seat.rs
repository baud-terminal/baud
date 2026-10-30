//! **The Linux driver's pointer, between the compositor's events and the
//! core**: where the pointer is, which press owns the gesture, the click
//! count, the wheel's travel, and the selection with the numbering that
//! keeps it on its lines across a read. The rules are the macOS app's,
//! shared -- the route and the selection in `gesture.rs`, the wheel in
//! `wheel.rs`, the read's bookkeeping in `keep.rs` -- and what is here is the
//! state a Wayland seat needs that AppKit keeps for the macOS one: the
//! position, which no button event carries, and the click count, which no
//! Wayland event carries. Pure, over a real `Term`, so the driver's path is
//! tested on any platform; `linux.rs` only turns `wl_pointer` events into
//! [`Input`].

use crate::gesture::{self, Kind, Owner, Pressed, Selected, Shift, To};
use crate::keep::{
    Before, KeptText, Ring, followed, kept_across_resize, numbered, oldest, reset_in,
    scrolled_off_the_top, snap_live, stale_in_moved_rows, switched, tracked_highlight,
};
use crate::wheel::{Delta, Route, Travel};
use baud_core::{Erasure, Focus, HistoryMark, Looking, Screen, Term};
use baud_domain::{
    Col, Cols, EventMillis, ImagePx, LocatorAt, LocatorButtons, LocatorInput, LocatorKind,
    Modifiers, MouseButton, MouseEvent, MouseKind, MouseMods, PointerPx, Row, Rows,
};
use baud_render::{Clip, Highlight, Press, Scrolled, Selection};
use std::time::Duration;

/// **The longest a second press may follow the last release and count as
/// the next click**: xterm's `multiClickTime` default, "the maximum time in
/// milliseconds between multi-click select events. The default is 250
/// milliseconds" (`[[source: xterm-man]]`). For a driver whose system says
/// no interval of its own -- AppKit counts its clicks itself, the Linux
/// driver reads no desktop setting.
pub(crate) const MULTI_CLICK: Duration = Duration::from_millis(250);

/// How many clicks a press makes: one, two for a word, three for a line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ClickCount(pub(crate) u8);

/// **The click count a press makes, as xterm's `EvalSelectUnit` counts it**
/// (`[[source: xterm-button-c]]`): the same button as the last release, and
/// the press no more than [`MULTI_CLICK`] after that release, is the next
/// click; anything else is the first. The stamps are the events' own
/// milliseconds, "with an undefined base" on Wayland, so only their
/// difference is read (`EventMillis::since`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Clicks {
    last_up: Option<(Pressed, EventMillis)>,
    count: ClickCount,
}

impl Clicks {
    pub(crate) fn press(&mut self, button: Pressed, time: EventMillis) -> ClickCount {
        let again = self
            .last_up
            .is_some_and(|(b, t)| b == button && time.since(t) <= MULTI_CLICK);
        self.count = match again {
            true => ClickCount(self.count.0.saturating_add(1)),
            false => ClickCount(1),
        };
        self.count
    }

    pub(crate) fn release(&mut self, button: Pressed, time: EventMillis) {
        self.last_up = Some((button, time));
    }
}

/// What the compositor said the pointer did, in the driver's terms. Points
/// are the surface's own, which with no buffer scale are its pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Input {
    Entered(f64, f64),
    Left,
    Moved(f64, f64),
    /// A button went down, at the event's millisecond stamp.
    Pressed {
        button: Pressed,
        time: EventMillis,
    },
    /// A button came up.
    Released {
        button: Pressed,
        time: EventMillis,
    },
    /// Wheel detents, positive down the page.
    Steps(f64),
    /// A length in surface pixels, positive down the page, from a device
    /// with no detents.
    Distance(f64),
    /// The touchpad's fingers lifted: what is not yet a line is dropped.
    Stopped,
    /// Wheel detents across, positive to the right.
    StepsAcross(f64),
    /// A length across in surface pixels, positive to the right.
    DistanceAcross(f64),
    /// The fingers lifted off a sideways swipe.
    StoppedAcross,
}

/// What turns a point into a cell: where the grid begins, the cell in
/// pixels and the grid in cells. The Linux frame draws from its first
/// pixel, the grid after the exit-status strip when it is kept, and has no
/// buffer scale, so a point is a pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) pad: (u32, u32),
    pub(crate) cell: (f64, f64),
    pub(crate) grid: (u16, u16),
}

/// What an input owes the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owed {
    Nothing,
    /// A frame: the selection, `?1001`'s region, the window or the
    /// indicator line's locator field moved.
    Frame,
    /// The wheel's cursor keys went out, which the caret counts as input.
    Keys,
    /// A selection was made -- a drag released, or a double or triple
    /// click's word or line -- and is drawn: the driver copies it to
    /// SELECT (`clip::select_board`), xterm's `select-end(SELECT,
    /// CUT_BUFFER0)` (`[[source: xterm-man]]`).
    Selected,
    /// The middle button's release: the driver pastes SELECT.
    PasteSelection,
    /// A Ctrl-click's release over the link it pressed: the driver opens
    /// what [`Seat::take_link`] hands it.
    Link,
}

/// The modifier bits a mouse report carries: Alt is Meta, the way xterm
/// reads the key.
pub(crate) fn mouse_mods(m: Modifiers) -> MouseMods {
    let mut out = MouseMods::NONE;
    if m.has(Modifiers::SHIFT) {
        out = out.with(MouseMods::SHIFT);
    }
    if m.has(Modifiers::CTRL) {
        out = out.with(MouseMods::CONTROL);
    }
    if m.has(Modifiers::ALT) {
        out = out.with(MouseMods::META);
    }
    out
}

/// What a left press does to the selection, the macOS rule with Alt for
/// Option: Alt starts a block and wins over Shift, which extends.
fn press_of(m: Modifiers) -> Press {
    if m.has(Modifiers::ALT) {
        Press::Block
    } else if m.has(Modifiers::SHIFT) {
        Press::Extend
    } else {
        Press::Stream
    }
}

/// What a read did that the driver's effects answer: another surface up,
/// and a thumb a reset revealed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AfterRead {
    pub(crate) surface: Surface,
    pub(crate) thumb: Thumb,
    pub(crate) select: Select,
}

/// **Whether the selection SELECT offered is given up**: output took it
/// while `?1040` is reset, where xterm's `ScrnDisownSelection` calls
/// `DisownSelection` and that `XtDisownSelection` on each selection it
/// owns (`[[source: xterm-screen]]`, `[[source: xterm-button]]`); set, it
/// only takes the highlight and keeps offering the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Select {
    Kept,
    GivenUp,
}

/// Whether a read put another surface up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Switched,
    Same,
}

/// Whether a read revealed the scrollbar's thumb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Thumb {
    Reveal,
    Leave,
}

/// What the scrollbar is asked of a pointer input, after DEC's locator:
/// the input, where the pointer is over the surface -- a press and a
/// release carry no position of their own -- and whether a button is down,
/// which makes a move a drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Beside {
    pub(crate) input: Input,
    pub(crate) at: Option<(f64, f64)>,
    pub(crate) held: crate::bar::Held,
}

pub(crate) struct Seat {
    /// Where the pointer is over the surface; `None` outside it.
    at: Option<(f64, f64)>,
    owner: Owner,
    /// The button a report or the selection holds, from its press.
    held: Option<MouseButton>,
    /// The button physically down, for a motion's kind: a drag or a move.
    down: Option<Pressed>,
    /// Every button physically down, for DEC's locator's `Pb`
    /// (`gesture::held_after`).
    buttons: LocatorButtons,
    clicks: Clicks,
    travel: Travel,
    /// Sideways travel not yet a whole column.
    travel_across: Travel,
    selection: Option<Selection>,
    /// The selection's text, kept past its highlight while `?1040` is set
    /// (`keep::KeptText`).
    kept: KeptText,
    scrolled: Scrolled,
    /// A Ctrl press the link gesture took, and the link its release opens
    /// (`link::gesture`); whether the window has the keyboard, which a press
    /// must for it to arm.
    armed: Option<crate::link::Armed>,
    opening: Option<crate::link::Under>,
    focus: Focus,
    /// The link preview: the link under the pointer while Ctrl is held,
    /// underlined, and its address in a bar (`link::Hover`).
    hover: crate::link::Hover,
    /// The order the last frame drew each row in, which a pointer position
    /// is mapped back through (`gesture::logical`).
    orders: baud_render::Orders,
}

impl Seat {
    pub(crate) fn new() -> Seat {
        Seat {
            at: None,
            owner: Owner::Free,
            held: None,
            down: None,
            buttons: LocatorButtons::NONE,
            clicks: Clicks::default(),
            travel: Travel::default(),
            travel_across: Travel::default(),
            selection: None,
            kept: KeptText::default(),
            scrolled: Scrolled::START,
            armed: None,
            opening: None,
            focus: Focus::Lost,
            hover: crate::link::Hover::default(),
            orders: baud_render::Orders::default(),
        }
    }

    /// **The display's wrap the window is drawn with** (`wrap_words`),
    /// which a copy under `?96` reads its lines by.
    pub(crate) fn set_word_wrap(&mut self, wrap: baud_render::WordWrap) {
        self.kept.set_word_wrap(wrap);
    }

    /// The order the frame just drawn put each row in: a pointer on it is
    /// over the logical cell its visual one shows.
    pub(crate) fn drawn(&mut self, orders: &baud_render::Orders) {
        if self.orders != *orders {
            self.orders.clone_from(orders);
        }
    }

    /// The keyboard came or went: a link arms only in a focused window,
    /// and is previewed only in one.
    pub(crate) fn focused(&mut self, f: Focus) {
        self.focus = f;
        if f == Focus::Lost {
            self.hover.clear();
        }
    }

    /// **The link preview after a pointer event or a change of the
    /// modifiers**: with Ctrl held and not Shift -- the link gesture's
    /// chord -- the link under the pointer is underlined and its address
    /// shown; otherwise, or off the surface, nothing. Whether the frame
    /// changed.
    pub(crate) fn hover(
        &mut self,
        term: &Term<'_>,
        mods: Modifiers,
        g: Geometry,
        host: crate::link::Host<'_>,
    ) -> bool {
        use crate::link::Chord;
        let chord = match mods.has(Modifiers::CTRL) && !mods.has(Modifiers::SHIFT) {
            true => Chord::Cmd,
            false => Chord::Other,
        };
        let at = self.at.map(|_| {
            let (row, col, _) = self.under(g);
            (row, col)
        });
        self.hover
            .update(term, at, self.scrolled, chord, self.focus, host)
    }

    /// The preview read again under the pointer where it rests, after a
    /// read, on the view shown now and with the modifiers held now:
    /// whether the frame changed.
    pub(crate) fn hover_refresh(
        &mut self,
        term: &Term<'_>,
        mods: Modifiers,
        host: crate::link::Host<'_>,
    ) -> bool {
        use crate::link::Chord;
        let chord = match mods.has(Modifiers::CTRL) && !mods.has(Modifiers::SHIFT) {
            true => Chord::Cmd,
            false => Chord::Other,
        };
        self.hover.over_text()
            && self
                .hover
                .refresh(term, self.scrolled, chord, self.focus, host)
    }

    /// What the link preview draws, or none.
    pub(crate) fn hover_look(&self) -> Option<&baud_render::LinkLook> {
        self.hover.look()
    }

    /// Who a left press where the pointer rests goes to now, the
    /// scrollbar's strip read by the driver (`bar::left_press`): whether
    /// that changed, which may change the pointer.
    pub(crate) fn hover_press(&mut self, left: crate::link::LeftPress) -> bool {
        self.hover.set_left_press(left)
    }

    /// The pointer's shape over the text for a program's `asked` one: the
    /// hand while a click on the link previewed opens it
    /// (`link::Hover::pointer`).
    pub(crate) fn pointer_shape(
        &self,
        asked: Option<baud_core::PointerShape>,
    ) -> Option<baud_core::PointerShape> {
        self.hover.pointer(asked)
    }

    /// The link a [`Owed::Link`] opens, once.
    pub(crate) fn take_link(&mut self) -> Option<crate::link::Under> {
        self.opening.take()
    }

    /// Whether the pointer is over the surface, for its shape.
    pub(crate) fn inside(&self) -> bool {
        self.at.is_some()
    }

    fn under(&self, g: Geometry) -> (Row, Col, PointerPx) {
        let at = self.at.unwrap_or((0.0, 0.0));
        gesture::logical(
            &self.orders,
            gesture::pointer_under(at, 1.0, g.cell, g.grid, g.pad),
            g.cell,
        )
    }

    /// **The stored cell under the pointer, for a selection**: a cell the
    /// display's word wrap draws from a line past the window named by that
    /// line (`baud_render::Orders::pointer_line`); a report keeps
    /// [`Seat::under`]'s window cell.
    fn under_line(&self, g: Geometry) -> (baud_render::WindowLine, Col) {
        let at = self.at.unwrap_or((0.0, 0.0));
        let (row, col, _) = gesture::pointer_under(at, 1.0, g.cell, g.grid, g.pad);
        self.orders.pointer_line(row, col)
    }

    /// Where DEC's locator is: `None` off the surface or its text area.
    fn locator_at(&self, g: Geometry) -> Option<LocatorAt> {
        self.at
            .and_then(|at| gesture::locator_under(at, 1.0, g.cell, g.grid, g.pad))
            .map(|at| match at.px {
                Some(px) => {
                    let (row, col, px) =
                        gesture::logical(&self.orders, (at.row, at.col, px), g.cell);
                    LocatorAt {
                        row,
                        col,
                        px: Some(px),
                    }
                }
                None => at,
            })
    }

    /// **One pointer input** with no scrollbar beside it
    /// ([`Seat::input_beside`]).
    #[cfg(test)]
    pub(crate) fn input(
        &mut self,
        term: &mut Term<'_>,
        e: Input,
        mods: Modifiers,
        g: Geometry,
        sink: impl FnMut(&[u8]),
    ) -> Owed {
        self.input_beside(term, e, mods, g, sink, |_, _| crate::bar::Took::No)
    }

    /// **One pointer input**: DEC's locator told of it first, then the
    /// scrollbar asked (`bar`, given where the pointer is and whether a
    /// button is down), and what the scrollbar did not take routed -- the
    /// macOS driver's order. The wheel to the history, the alternate
    /// screen's keys or a tracking child's reports (`wheel::route`); a
    /// press, a drag, a move and a release to the child that tracks the
    /// mouse or to the selection, decided at the press with Shift's
    /// override (`gesture::pointer_route`); a click to the prompt or
    /// `?2001`. Every byte for the child to `sink`.
    pub(crate) fn input_beside(
        &mut self,
        term: &mut Term<'_>,
        e: Input,
        mods: Modifiers,
        g: Geometry,
        sink: impl FnMut(&[u8]),
        bar: impl FnOnce(&mut Term<'_>, Beside) -> crate::bar::Took,
    ) -> Owed {
        let pointer = !matches!(e, Input::Steps(_) | Input::Distance(_) | Input::Stopped);
        // A ReGIS cursor the pointer moved is a frame too.
        let drawn = term.images().generation();
        let was = self.selection;
        let owed = self.route(term, e, mods, g, sink, bar);
        self.gestured(was, term);
        match owed {
            Owed::Nothing if pointer && gesture::locator_redraws(term) => Owed::Frame,
            Owed::Nothing if term.images().generation() != drawn => Owed::Frame,
            owed => owed,
        }
    }

    /// **The kept text hears of a selection a gesture changed**
    /// (`keep::KeptText::gestured`): a newer selection replaces what output
    /// left to copy, and a press or a click that made none keeps the text
    /// of the one it took the highlight from.
    fn gestured(&mut self, was: Option<Selection>, term: &Term<'_>) {
        if self.selection != was {
            self.kept
                .gestured(self.selection.as_ref(), term, self.scrolled);
        }
    }

    /// **The kept text hears of a selection the find or a resize changed**
    /// (`keep::KeptText::changed`): the newer one's text is taken at the
    /// next read.
    fn told(&mut self, was: Option<Selection>) {
        if self.selection != was {
            self.kept.changed();
        }
    }

    /// [`Seat::input_beside`]'s routing, before the indicator's frame.
    fn route(
        &mut self,
        term: &mut Term<'_>,
        e: Input,
        mods: Modifiers,
        g: Geometry,
        mut sink: impl FnMut(&[u8]),
        bar: impl FnOnce(&mut Term<'_>, Beside) -> crate::bar::Took,
    ) -> Owed {
        let (kind, button) = match e {
            Input::Entered(x, y) => {
                self.at = Some((x, y));
                (Kind::Crossed, Pressed::Left)
            }
            Input::Left => {
                self.at = None;
                (Kind::Crossed, Pressed::Left)
            }
            Input::Moved(x, y) => {
                self.at = Some((x, y));
                match self.down {
                    Some(b) => (Kind::Dragged, b),
                    None => (Kind::Moved, Pressed::Left),
                }
            }
            Input::Pressed { button, .. } => {
                // Any press, whatever takes it below, is input to a GIF
                // frame waiting for the user (`Term::pointer_pressed`).
                term.pointer_pressed();
                self.down = Some(button);
                (Kind::Down, button)
            }
            Input::Released { button, .. } => {
                if self.down == Some(button) {
                    self.down = None;
                }
                (Kind::Up, button)
            }
            Input::Steps(s) => return self.wheel(term, Delta::Lines(-s), mods, g, sink),
            Input::Distance(d) => {
                let delta = Delta::Points {
                    dy: -d,
                    scale: 1.0,
                    cell: g.cell.1,
                };
                return self.wheel(term, delta, mods, g, sink);
            }
            Input::Stopped => {
                self.travel.settle();
                return Owed::Nothing;
            }
            // Across, positive to the right as the axis gives it, and the
            // wheel's lines count toward the start, so it is negated as the
            // vertical is.
            Input::StepsAcross(st) => {
                return self.wheel_across(term, Delta::Lines(-st), mods, g, sink);
            }
            Input::DistanceAcross(d) => {
                let delta = Delta::Points {
                    dy: -d,
                    scale: 1.0,
                    cell: g.cell.0,
                };
                return self.wheel_across(term, delta, mods, g, sink);
            }
            Input::StoppedAcross => {
                self.travel_across.settle();
                return Owed::Nothing;
            }
        };
        // DEC's locator keeps the pointer whatever takes the event: a move
        // or a crossing here, before the scrollbar, so a filter rectangle
        // sees every position, and the buttons held after a press the
        // scrollbar takes; a press or a release once routed, below.
        self.buttons = gesture::held_after(self.buttons, kind, button);
        if !matches!(kind, Kind::Down | Kind::Up) {
            let e = LocatorInput {
                kind: LocatorKind::Moved,
                at: self.locator_at(g),
                held: self.buttons,
            };
            crate::reads::input(term, |t| t.locator(e), &mut sink);
        }
        // **The scrollbar next** (`bar.rs`): a press it takes keeps its
        // drags and its release, and a move it takes is no one else's.
        let beside = Beside {
            input: e,
            at: self.at,
            held: match self.down {
                Some(_) => crate::bar::Held::Yes,
                None => crate::bar::Held::No,
            },
        };
        if bar(term, beside) == crate::bar::Took::Yes {
            return Owed::Nothing;
        }
        match e {
            Input::Pressed { button, time } => {
                let _ = self.clicks.press(button, time);
            }
            Input::Released { button, time } => self.clicks.release(button, time),
            _ => {}
        }
        // **A Ctrl press over a link is the terminal's, the whole
        // gesture**, before the child's tracking, as Cmd is on macOS, so a
        // child never sees a release without its press (`link::gesture`;
        // gnome-terminal's chord, `[[source: gnome-terminal-screen]]`
        // 2010-2022; rather than kitty's Ctrl+Shift).
        if let Some(owed) = self.link(term, e, kind, button, mods, g) {
            return owed;
        }
        let held = match mods.has(Modifiers::SHIFT) {
            true => Shift::Held,
            false => Shift::Up,
        };
        let tracking = gesture::pointer_tracking(term.mouse_tracking(), term.regis_input());
        let (to, owner) = gesture::pointer_route(
            self.owner,
            kind,
            tracking,
            gesture::shift_overrides(held, term.shift_escape(), kind, button),
        );
        self.owner = owner;
        if matches!(kind, Kind::Down | Kind::Up) {
            let e = gesture::locator_input(
                (kind, button, to),
                tracking,
                self.locator_at(g),
                self.buttons,
            );
            crate::reads::input(term, |t| t.locator(e), &mut sink);
        }
        let (row, col, px) = self.under(g);
        // **The middle button pastes on its release**, as xterm binds it:
        // `~Ctrl ~Meta <Btn2Down>: ignore()` and `~Ctrl ~Meta <Btn2Up>:
        // insert-selection(SELECT, CUT_BUFFER0)` (`[[source: xterm-man]]`),
        // kitty's `paste_selection middle release ungrabbed` (`[[source:
        // kitty-options-definition]]`). Taken here, before the selection's
        // rules, so it never ends or drops a selection the left button made.
        if to == To::Select && button == Pressed::Middle {
            let plain = !mods.has(Modifiers::CTRL) && !mods.has(Modifiers::ALT);
            return match (kind, plain) {
                (Kind::Up, true) => Owed::PasteSelection,
                _ => Owed::Nothing,
            };
        }
        if to == To::Select {
            let at = self.under_line(g);
            let selected = gesture::select(
                term,
                self.scrolled,
                &mut self.selection,
                &mut self.held,
                (kind, button, self.clicks.count.0),
                press_of(mods),
                at,
            );
            return match selected {
                Selected::Unchanged => Owed::Nothing,
                Selected::Changed => Owed::Frame,
                Selected::Made => Owed::Selected,
                Selected::Click => {
                    let clean = button == Pressed::Left
                        && !mods.has(Modifiers::SHIFT)
                        && !mods.has(Modifiers::ALT)
                        && !mods.has(Modifiers::CTRL)
                        && !mods.has(Modifiers::SUPER)
                        && term.window_back().index() == 0;
                    if clean {
                        let cell_w =
                            ImagePx::of(u16::try_from(g.cell.0 as i64).unwrap_or(u16::MAX));
                        gesture::clicked(term, (row, col, px), cell_w, &mut sink);
                    }
                    Owed::Frame
                }
            };
        }
        let pressed = button.reported();
        let (kind, button) = match kind {
            Kind::Down => {
                self.held = Some(pressed);
                (MouseKind::Press, pressed)
            }
            Kind::Up => {
                self.held = None;
                (MouseKind::Release, pressed)
            }
            Kind::Dragged => (MouseKind::Motion, self.held.unwrap_or(pressed)),
            Kind::Moved => (MouseKind::Motion, MouseButton::None),
            Kind::Crossed => return Owed::Nothing,
        };
        // A drag under `?1001` moves the end of the region the program's
        // answer started, and its release ends it, with no report of the
        // move: the frame follows the core's region, not the reply.
        let tracked = term.highlight();
        crate::reads::input(
            term,
            |t| {
                t.mouse_report(MouseEvent {
                    button,
                    mods: mouse_mods(mods),
                    kind,
                    col,
                    row,
                    px: Some(px),
                })
            },
            &mut sink,
        );
        match term.highlight() != tracked {
            true => Owed::Frame,
            false => Owed::Nothing,
        }
    }

    /// **The link gesture's share of a pointer event**, or `None` when it
    /// is not the gesture's. Ctrl without Shift is the chord; Shift keeps
    /// its meaning, the selection's override.
    fn link(
        &mut self,
        term: &Term<'_>,
        e: Input,
        kind: Kind,
        button: Pressed,
        mods: Modifiers,
        g: Geometry,
    ) -> Option<Owed> {
        use crate::link::{Chord, Clicks as Count, Gesture, Press, Spot};
        let press = match (e, kind, button) {
            (Input::Left, ..) => Press::Exited,
            (_, Kind::Down, Pressed::Left) => Press::Down,
            (_, Kind::Up, Pressed::Left) => Press::Up,
            (_, Kind::Dragged, _) => Press::Dragged,
            _ => Press::Other,
        };
        let chord = match mods.has(Modifiers::CTRL) && !mods.has(Modifiers::SHIFT) {
            true => Chord::Cmd,
            false => Chord::Other,
        };
        let clicks = match self.clicks.count.0 <= 1 {
            true => Count::Single,
            false => Count::Repeat,
        };
        let (row, col, _) = self.under(g);
        let back = term.window_back();
        let col = baud_render::char_col(&term.view(back), row, col);
        let spot = Spot {
            scrolled: self.scrolled,
            back,
            row,
            col,
        };
        let armed = self.armed.as_ref();
        let under = crate::link::under(term, spot, press, chord, self.focus, armed);
        match crate::link::gesture(press, chord, clicks, self.focus, armed, under.as_ref()) {
            Gesture::Pass => None,
            Gesture::Arm(a) => {
                self.armed = Some(a);
                Some(Owed::Nothing)
            }
            Gesture::Hold => Some(Owed::Nothing),
            Gesture::Drop => {
                self.armed = None;
                Some(Owed::Nothing)
            }
            Gesture::Open(link) => {
                self.armed = None;
                self.opening = Some(link);
                Some(Owed::Link)
            }
        }
    }

    /// The wheel: whole lines of `delta`, one line a detent -- the
    /// protocol's logical step taken as a line, as AppKit's line is on
    /// macOS -- routed as the macOS app routes them.
    fn wheel(
        &mut self,
        term: &mut Term<'_>,
        delta: Delta,
        mods: Modifiers,
        g: Geometry,
        mut sink: impl FnMut(&[u8]),
    ) -> Owed {
        let lines = self.travel.lines(delta);
        let step = lines.get();
        if step == 0 {
            return Owed::Nothing;
        }
        match crate::wheel::route(term, mouse_mods(mods)) {
            Route::Report => {
                let at = self.under(g);
                crate::wheel::report(term, lines, at, mouse_mods(mods), sink);
                Owed::Nothing
            }
            // The wheel's keys, not keystrokes: KAM does not lock them and
            // SRM does not draw them (`Term::wheel_key`).
            Route::Keys => {
                for key in crate::wheel::keys(lines) {
                    crate::reads::input(term, |t| t.wheel_key(key), &mut sink);
                }
                Owed::Keys
            }
            Route::Nothing => Owed::Nothing,
            Route::History => {
                let retained = i64::try_from(term.scrollback().len()).unwrap_or(i64::MAX);
                let was = term.primary_window_back();
                let now = i64::try_from(was.index()).unwrap_or(0);
                let back =
                    Rows::of_count(usize::try_from((now + step).clamp(0, retained)).unwrap_or(0));
                if back.index() == was.index() {
                    return Owed::Nothing;
                }
                term.set_window_back(back);
                Owed::Frame
            }
        }
    }

    /// **The wheel across**: whole columns of the cell width, to a program
    /// tracking the mouse as buttons 6 and 7, and nothing otherwise
    /// (`wheel::route_on`), as on macOS.
    fn wheel_across(
        &mut self,
        term: &mut Term<'_>,
        delta: Delta,
        mods: Modifiers,
        g: Geometry,
        sink: impl FnMut(&[u8]),
    ) -> Owed {
        let cols = self.travel_across.lines(delta);
        if cols.get() == 0 {
            return Owed::Nothing;
        }
        let axis = crate::wheel::Axis::Horizontal;
        if crate::wheel::route_on(term, mouse_mods(mods), axis) == Route::Report {
            let at = self.under(g);
            crate::wheel::report_on(term, axis, cols, at, mouse_mods(mods), sink);
        }
        Owed::Nothing
    }

    /// The count that numbers the selection's lines.
    #[cfg(target_os = "linux")]
    pub(crate) fn scrolled(&self) -> Scrolled {
        self.scrolled
    }

    /// **Select `selection`**, numbered by [`Seat::scrolled`]: the find's
    /// current match.
    #[cfg(target_os = "linux")]
    pub(crate) fn select(&mut self, selection: Option<Selection>) {
        let was = core::mem::replace(&mut self.selection, selection);
        self.told(was);
    }

    /// The selection's text, as Ctrl+Shift+C and a finished drag copy it;
    /// without one, the text output took a selection's highlight away from
    /// while `?1040` kept it (`keep::KeptText`); `None` for none or no text.
    pub(crate) fn selected_text(&self, term: &Term<'_>) -> Option<String> {
        let text = match self.selection {
            Some(s) => match self.kept.text(Some(&s)) {
                Some(t) => t.to_owned(),
                None => s.text_as(term, self.scrolled, self.kept.word_wrap()),
            },
            None => self.kept.text(None)?.to_owned(),
        };
        (!text.is_empty()).then_some(text)
    }

    /// **A key snaps the window back to the live screen**, where its effect
    /// is (`keep::snap_live`), and drops the wheel's part-line; whether that
    /// moved it.
    pub(crate) fn key(&mut self, term: &mut Term<'_>) -> bool {
        let moved = snap_live(term);
        if moved {
            self.travel.settle();
        }
        moved
    }

    /// **A new width re-cuts every line**, so the columns a selection
    /// named no longer name cells and it goes; a change of height alone on
    /// the primary keeps it on its lines, renumbered by what the resize
    /// retired or took back (`keep::kept_across_resize`, note V, V3), as
    /// the macOS driver's resize does. `was` is read before the resize.
    pub(crate) fn resized(&mut self, term: &Term<'_>, was: (HistoryMark, Cols, Screen)) {
        let sel = self.selection;
        (self.selection, self.scrolled) =
            kept_across_resize(term, was, self.selection.take(), self.scrolled);
        self.told(sel);
    }

    /// The readings a read is judged by, with the core's watches armed on
    /// the selection's lines (`keep::Before::watched`), and the selection's
    /// text taken while `?1040` would keep it (`keep::KeptText::take`).
    pub(crate) fn before_read(&mut self, term: &mut Term<'_>) -> Before {
        self.kept.take(self.selection.as_ref(), term, self.scrolled);
        Before::watched(term, self.selection.as_ref(), self.scrolled)
    }

    /// **What a read did to the selection**: dropped where it wrote, erased
    /// or moved in place on its rows, renumbered by the lines the surface
    /// retired or took back, carried with its text by a scroll
    /// (`keep::followed`), dropped by a switch or a reset with the
    /// wheel's part-line, and clipped where the history it named is gone --
    /// the macOS pump's order (`app.rs`, `pump`). The text kept for it is
    /// the one taken when it was made, however the read carried it, and
    /// output that took it while `?1040` is reset gives up what SELECT
    /// offered ([`Select`]).
    pub(crate) fn after_read(&mut self, term: &Term<'_>, before: Before) -> AfterRead {
        // The macOS pump's `restart`: a switch is a surface the trail and
        // the fade answer, and a reset made from the primary while the
        // window looked back brings it live, which reveals the thumb.
        let mut after = AfterRead {
            surface: match switched(term, before) {
                true => Surface::Switched,
                false => Surface::Same,
            },
            thumb: match term.first_erasure(before.erased) {
                Some(Erasure::Reset(Screen::Normal, Looking::Back)) => Thumb::Reveal,
                _ => Thumb::Leave,
            },
            select: Select::Kept,
        };
        let keeps = term.resource(baud_core::Resource::KeepSelection);
        if self.selection.is_some() && stale_in_moved_rows(term) {
            self.selection = None;
            self.kept.dropped(term);
            if !keeps {
                after.select = Select::GivenUp;
            }
        }
        let top_was = self.scrolled.top(Rows::ZERO);
        self.scrolled = match numbered(term, before, None) {
            Ring::Retired(n) => self.scrolled.advanced(n),
            Ring::Restored(n) => match self.scrolled.retreated(n) {
                Some(s) => s,
                None => {
                    self.selection = None;
                    Scrolled::START
                }
            },
        };
        let tops = (top_was, self.scrolled.top(Rows::ZERO));
        if self
            .selection
            .is_some_and(|s| scrolled_off_the_top(term, &s, tops))
        {
            self.selection = None;
            after.select = Select::GivenUp;
        }
        followed(term, &mut self.selection, self.scrolled);
        if switched(term, before) || reset_in(term, before) {
            self.selection = None;
            self.travel.settle();
        }
        let oldest = oldest(term, self.scrolled);
        if self
            .selection
            .as_mut()
            .is_some_and(|s| s.clip(oldest) == Clip::Cleared)
        {
            // Clipped away: `?1040` keeps its text, as xterm's disowning
            // under keepSelection takes only the highlight.
            self.selection = None;
            self.kept.dropped(term);
        }
        // Carried or clipped, the text kept is the one it was made with.
        self.kept.carried(self.selection.as_ref());
        after
    }

    /// **What the frame highlights**: the selection's share of the view
    /// drawn, or, while a program's `?1001` region tracks the mouse, that
    /// region in its place (`keep::tracked_highlight`).
    pub(crate) fn highlight(&self, term: &Term<'_>) -> Option<Highlight> {
        let back = term.window_back();
        let view = term.view(back);
        let top = self.scrolled.top(back);
        match term.highlight() {
            Some(_) => tracked_highlight(term, self.scrolled, back),
            None => {
                let beyond = crate::keep::beyond(term, back, self.kept.word_wrap());
                self.selection
                    .and_then(|s| s.highlight_reaching(top, view.rows(), view.cols(), beyond))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::EventMillis as Ms;
    use baud_domain::{BytePos, ReadLen, WinSize};
    use baud_render::Region;

    /// **The next click is the same button within 250 ms of the last
    /// release**, as xterm's `EvalSelectUnit` counts: 250 counts and 251
    /// does not, another button starts again, and a stamp that rolled over
    /// is read by its difference.
    #[test]
    fn a_click_counts_as_xterm_counts_it() {
        let mut c = Clicks::default();
        let (left, right, at) = (Pressed::Left, Pressed::Right, Ms::of_millis);
        assert_eq!(c.press(left, at(1000)), ClickCount(1));
        c.release(left, at(1050));
        let n = c.press(left, at(1300));
        assert_eq!(n, ClickCount(2), "250 ms after the release");
        c.release(left, at(1310));
        assert_eq!(c.press(left, at(1400)), ClickCount(3));
        c.release(left, at(1400));
        let n = c.press(left, at(1651));
        assert_eq!(n, ClickCount(1), "251 ms is a new click");
        c.release(left, at(1660));
        let n = c.press(right, at(1670));
        assert_eq!(n, ClickCount(1), "another button");
        c.release(right, at(u32::MAX - 9));
        let n = c.press(right, at(40));
        assert_eq!(n, ClickCount(2), "50 ms across the wrap");
    }

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    /// Ten-pixel cells on the 20x6 grid.
    const G: Geometry = Geometry {
        pad: (0, 0),
        cell: (10.0, 10.0),
        grid: (20, 6),
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// One input with no modifiers, and the bytes it sent.
    fn put(seat: &mut Seat, t: &mut Term<'_>, e: Input, mods: Modifiers) -> (Owed, Vec<u8>) {
        let mut sent = Vec::new();
        let owed = seat.input(t, e, mods, G, |b| sent.extend_from_slice(b));
        sent.extend_from_slice(t.reply());
        t.clear_reply();
        (owed, sent)
    }

    fn down(button: Pressed, time: Ms) -> Input {
        Input::Pressed { button, time }
    }

    fn up(button: Pressed, time: Ms) -> Input {
        Input::Released { button, time }
    }

    const NONE: Modifiers = Modifiers::NONE;
    const SIDE: Pressed = Pressed::Button8;
    const PAST: Pressed = Pressed::Other;

    /// **A press that selects moves a GIF frame waiting for the user on**,
    /// "processing will continue when user input is entered", "Mouse
    /// Button Click" among GIF89a's examples: with tracking off the press
    /// is the selection's and sends nothing, and the core hears of it all
    /// the same (`Term::pointer_pressed`). The picture is a 1x1 GIF whose
    /// first frame waits with no delay. Perturbation: leave the press
    /// untold -- the frame stays.
    #[test]
    fn a_press_that_selects_moves_a_waiting_gif_frame_on() {
        use baud_domain::{Animated, HostTime, ImageBudget, ImageMotion, ImagePx};
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(1 << 16));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(10));
        let mut seat = Seat::new();
        feed(
            &mut t,
            b"\x1b]1337;File=inline=1:R0lGODlhAQABAIAAAAAAAP///yH/C05FVFNDQVBFMi4wAwEAAAAh\
              +QQGAAAAACwAAAAAAQABAAACAkwBACH5BAQKAAAALAAAAAABAAEAAAICRAEAOw==\x07",
        );
        let at = |ms: u64| HostTime::of_nanos(1_000_000_000 + ms * 1_000_000);
        t.animate_images(at(0), ImageMotion::Play);
        let tick = |t: &mut Term<'_>, ms| t.animate_images(at(ms), ImageMotion::Play);
        assert_eq!(tick(&mut t, 9_000), Animated::Same, "waiting");
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            NONE,
        );
        assert!(sent.is_empty(), "the selection's press: {sent:?}");
        assert_eq!(tick(&mut t, 9_001), Animated::Changed, "moved on");
    }

    /// **A read says what the effects answer**: a switch of surface, and
    /// a reset from a primary looked back, which reveals the thumb; text
    /// says neither. Perturbation: answer `Same` for a switch -- the first
    /// assertion fails.
    #[test]
    fn a_read_names_a_switch_and_a_reset_that_reveals() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut seat = Seat::new();
        let read = |seat: &mut Seat, t: &mut Term<'_>, b: &[u8]| {
            let before = seat.before_read(t);
            feed(t, b);
            seat.after_read(t, before)
        };
        let alt = read(&mut seat, &mut t, b"\x1b[?1049h");
        assert_eq!(alt.surface, Surface::Switched);
        let back = read(&mut seat, &mut t, b"\x1b[?1049l");
        assert_eq!(back.surface, Surface::Switched);
        let text = read(&mut seat, &mut t, b"x");
        assert_eq!((text.surface, text.thumb), (Surface::Same, Thumb::Leave));
        for i in 0..40 {
            feed(&mut t, format!("{i}\r\n").as_bytes());
        }
        t.set_window_back(Rows::of_count(5));
        let reset = read(&mut seat, &mut t, b"\x1bc");
        assert_eq!(reset.thumb, Thumb::Reveal);
    }

    /// **DEC's locator sees an event before the scrollbar takes it**, the
    /// macOS driver's order: a move the scrollbar takes that leaves a
    /// filter rectangle still reports it, and the buttons a press the
    /// scrollbar took holds are in that report's `Pb`; the scrollbar is
    /// asked with the pointer's position and a button down after that
    /// press, and a press it takes is no click. The control is the same
    /// move with nothing pressed, whose `Pb` is 0. Perturbations: ask the
    /// scrollbar before the locator -- no report; leave the buttons
    /// unheld for a taken press -- `Pb` is 0.
    #[test]
    fn the_locator_sees_a_move_before_the_scrollbar_takes_it() {
        use crate::bar::{Held, Took};
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_locator_device(baud_core::LocatorDevice::Present);
        // DECELR on, and with the pointer there a filter rectangle over
        // the top-left corner.
        let rect = b"\x1b[1'z\x1b[1;1;2;5'w";
        let mut seat = Seat::new();
        let mut asked: Vec<(Option<(f64, f64)>, Held)> = Vec::new();
        let mut take = |seat: &mut Seat, t: &mut Term<'_>, e: Input| {
            let mut sent = Vec::new();
            seat.input_beside(
                t,
                e,
                NONE,
                G,
                |b| sent.extend_from_slice(b),
                |_, b| {
                    asked.push((b.at, b.held));
                    Took::Yes
                },
            );
            sent
        };
        take(&mut seat, &mut t, Input::Entered(5.0, 5.0));
        feed(&mut t, rect);
        t.clear_reply();
        // The control: a move out of the rectangle, nothing pressed.
        let sent = take(&mut seat, &mut t, Input::Moved(155.0, 45.0));
        assert_eq!(sent, b"\x1b[10;0;5;16&w", "reported, nothing held");
        take(&mut seat, &mut t, Input::Moved(5.0, 5.0));
        feed(&mut t, rect);
        t.clear_reply();
        take(&mut seat, &mut t, down(Pressed::Left, Ms::of_millis(1)));
        let sent = take(&mut seat, &mut t, Input::Moved(155.0, 45.0));
        assert_eq!(sent, b"\x1b[10;4;5;16&w", "the left button held");
        take(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(2)));
        assert_eq!(
            asked[2..],
            [
                (Some((5.0, 5.0)), Held::No),
                (Some((5.0, 5.0)), Held::Yes),
                (Some((155.0, 45.0)), Held::Yes),
                (Some((155.0, 45.0)), Held::No),
            ],
            "the press at the pointer, the move a drag"
        );
        // The press the scrollbar took was no click: the next is a first.
        let n = seat.clicks.press(Pressed::Left, Ms::of_millis(3));
        assert_eq!(n, ClickCount(1));
    }

    /// **The wheel across on Linux**: a detent to the left is button 6 to
    /// a program tracking the mouse, a touchpad's pixels by the cell's
    /// width, a stop dropping what is not yet a column; untracked it moves
    /// nothing.
    #[test]
    fn the_wheel_across_reports_buttons_6_and_7_under_tracking() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &b"line\r\n".repeat(30));
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Entered(25.0, 15.0), NONE);
        let (owed, sent) = put(&mut seat, &mut t, Input::StepsAcross(-1.0), NONE);
        assert_eq!((owed, sent.as_slice()), (Owed::Nothing, &b""[..]));
        assert_eq!(t.window_back().index(), 0, "untracked, nothing moves");
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        let (_, sent) = put(&mut seat, &mut t, Input::StepsAcross(-1.0), NONE);
        assert_eq!(sent, b"\x1b[<66;3;2M", "a detent left");
        let (_, sent) = put(&mut seat, &mut t, Input::DistanceAcross(25.0), NONE);
        assert_eq!(sent, b"\x1b[<67;3;2M\x1b[<67;3;2M", "25 px of 10-px cells");
        put(&mut seat, &mut t, Input::StoppedAcross, NONE);
        let (_, sent) = put(&mut seat, &mut t, Input::DistanceAcross(5.0), NONE);
        assert_eq!(sent, b"", "the half column before the stop was dropped");
    }

    /// **A click on a reordered row reports the cell the program wrote**:
    /// the pointer is over a visual cell, and a mouse report undergoes the
    /// inverse of the drawn order, terminal-wg's "whenever a mouse event is
    /// reported back to an app, it undergoes the inverse transformation"
    /// (`[[source: terminal-wg-bidi-display-only]]`). `ab אבג` at paragraph
    /// level 0 draws the alef at column 5; a press there reports column 4
    /// (1-based), where the program wrote it. The control: before the seat
    /// is told the order, the press reports the column under the pointer.
    #[test]
    fn a_press_on_a_reordered_row_reports_the_logical_cell() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            "\x1b[8hab \u{5D0}\u{5D1}\u{5D2}\x1b[?1000h\x1b[?1006h".as_bytes(),
        );
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Entered(55.0, 5.0), NONE);
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(10)),
            NONE,
        );
        assert_eq!(
            sent, b"\x1b[<0;6;1M",
            "the control: the column under the pointer"
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(20)),
            NONE,
        );
        let mut order = baud_render::Reorder::new();
        order.lay_out(&t, baud_domain::Rows::ZERO);
        seat.drawn(order.orders());
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(500)),
            NONE,
        );
        assert_eq!(sent, b"\x1b[<0;4;1M", "the alef, where it was written");
    }

    /// **The wheel moves the window over the history**, a line a detent,
    /// a half detent kept until the other half comes, and a touchpad's
    /// pixels by the cell's height; down the page is toward the live screen.
    #[test]
    fn the_wheel_moves_the_window_over_the_history() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &b"line\r\n".repeat(30));
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Entered(5.0, 5.0), NONE);
        assert_eq!(
            put(&mut seat, &mut t, Input::Steps(-3.0), NONE).0,
            Owed::Frame
        );
        assert_eq!(t.window_back().index(), 3, "three detents up");
        assert_eq!(
            put(&mut seat, &mut t, Input::Steps(-0.5), NONE).0,
            Owed::Nothing
        );
        put(&mut seat, &mut t, Input::Steps(-0.5), NONE);
        assert_eq!(t.window_back().index(), 4, "two half detents");
        put(&mut seat, &mut t, Input::Distance(25.0), NONE);
        assert_eq!(t.window_back().index(), 2, "25 px of a 10 px cell");
        put(&mut seat, &mut t, Input::Steps(9.0), NONE);
        assert_eq!(t.window_back().index(), 0, "held at the live screen");
        assert!(!seat.key(&mut t), "a key at the live screen moves nothing");
        put(&mut seat, &mut t, Input::Steps(-2.0), NONE);
        assert!(seat.key(&mut t), "a key snaps it back");
        assert_eq!(t.window_back().index(), 0);
    }

    /// **The side button is button 8 to a tracking child**, "adding 128
    /// (for buttons 8 through 11)" (`[[source: ctlseqs]]`): its press and
    /// release in SGR, its drag under `?1002` with the motion 32 added,
    /// and X10's bytes, whose release names no button; a button past the
    /// eleventh, which no encoding names, sends nothing rather than a
    /// press of "3=release", and the core counts it.
    #[test]
    fn the_side_button_is_button_8_to_a_tracking_child() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?1002h\x1b[?1006h");
        let mut seat = Seat::new();
        let ms = Ms::of_millis;
        put(&mut seat, &mut t, Input::Moved(25.0, 15.0), NONE);
        let (_, sent) = put(&mut seat, &mut t, down(SIDE, ms(10)), NONE);
        assert_eq!(sent, b"\x1b[<128;3;2M");
        let (_, sent) = put(&mut seat, &mut t, Input::Moved(35.0, 15.0), NONE);
        assert_eq!(sent, b"\x1b[<160;4;2M", "a drag with it held");
        let (_, sent) = put(&mut seat, &mut t, up(SIDE, ms(20)), NONE);
        assert_eq!(sent, b"\x1b[<128;4;2m");
        let (_, sent) = put(&mut seat, &mut t, down(PAST, ms(30)), NONE);
        assert_eq!(sent, b"", "past the eleventh, nothing");
        let (_, sent) = put(&mut seat, &mut t, up(PAST, ms(40)), NONE);
        assert_eq!(sent, b"");
        assert_eq!(t.mouse_unencodable, 2, "and counted");
        feed(&mut t, b"\x1b[?1006l\x1b[?1000h");
        let (_, sent) = put(&mut seat, &mut t, down(SIDE, ms(50)), NONE);
        assert_eq!(sent, b"\x1b[M\xa0$\"", "X10: 32 + 128");
        let (_, sent) = put(&mut seat, &mut t, up(SIDE, ms(60)), NONE);
        assert_eq!(sent, b"\x1b[M#$\"", "a release names no button");
    }

    /// **A tracking child gets the buttons and the wheel as its reports**,
    /// at the cell under the pointer; Shift takes the press for the
    /// selection instead, as xterm's override does.
    #[test]
    fn a_tracking_child_gets_the_buttons_and_the_wheel() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(25.0, 15.0), NONE);
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(10)),
            NONE,
        );
        assert_eq!(sent, b"\x1b[<0;3;2M");
        let (_, sent) = put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(20)),
            NONE,
        );
        assert_eq!(sent, b"\x1b[<0;3;2m");
        let (_, sent) = put(&mut seat, &mut t, Input::Steps(-1.0), NONE);
        assert_eq!(sent, b"\x1b[<64;3;2M", "a detent up is button 4");
        let shift = Modifiers::NONE.with(Modifiers::SHIFT);
        let (owed, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(30)),
            shift,
        );
        assert_eq!((owed, sent), (Owed::Frame, Vec::new()), "Shift selects");
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(40)),
            shift,
        );
    }

    /// **DEC's locator takes the pointer under DECELR** (`gesture.rs`):
    /// a press and a release report DECLRP at the cell, the held buttons
    /// after each, the side button as the fourth; Shift still selects and
    /// reports nothing; a point past the grid is undefined, its coordinates
    /// omitted; the pointer leaving the surface ends a filter rectangle with
    /// event 10; and the wheel scrolls the history, as with no protocol.
    #[test]
    fn the_locator_takes_the_pointer_under_decelr() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        // The seat has a pointer, as the driver tells the core.
        t.set_locator_device(baud_core::LocatorDevice::Present);
        feed(&mut t, &b"line\r\n".repeat(30));
        feed(&mut t, b"\x1b[1'z\x1b[1;3'{");
        let mut seat = Seat::new();
        let ms = Ms::of_millis;
        put(&mut seat, &mut t, Input::Entered(25.0, 15.0), NONE);
        let (_, sent) = put(&mut seat, &mut t, down(Pressed::Left, ms(10)), NONE);
        assert_eq!(sent, b"\x1b[2;4;2;3&w");
        let (_, sent) = put(&mut seat, &mut t, down(Pressed::Button8, ms(15)), NONE);
        assert_eq!(sent, b"\x1b[8;12;2;3&w", "the side button is the fourth");
        put(&mut seat, &mut t, up(Pressed::Button8, ms(16)), NONE);
        let (_, sent) = put(&mut seat, &mut t, up(Pressed::Left, ms(20)), NONE);
        assert_eq!(sent, b"\x1b[3;0;2;3&w");
        let shift = Modifiers::NONE.with(Modifiers::SHIFT);
        let (_, sent) = put(&mut seat, &mut t, down(Pressed::Left, ms(30)), shift);
        assert_eq!(sent, b"", "Shift selects");
        put(&mut seat, &mut t, up(Pressed::Left, ms(40)), shift);
        put(&mut seat, &mut t, Input::Moved(250.0, 15.0), NONE);
        let (_, sent) = put(&mut seat, &mut t, down(Pressed::Right, ms(50)), NONE);
        assert_eq!(sent, b"\x1b[6;1&w", "past the grid, no coordinates");
        put(&mut seat, &mut t, up(Pressed::Right, ms(60)), NONE);
        put(&mut seat, &mut t, Input::Moved(25.0, 15.0), NONE);
        feed(&mut t, b"\x1b[1;1;6;20'w");
        let (_, sent) = put(&mut seat, &mut t, Input::Left, NONE);
        assert_eq!(sent, b"\x1b[10;0&w", "leaving ends the rectangle");
        put(&mut seat, &mut t, Input::Entered(25.0, 15.0), NONE);
        let (owed, sent) = put(&mut seat, &mut t, Input::Steps(-2.0), NONE);
        assert_eq!((owed, sent.as_slice()), (Owed::Frame, &b""[..]));
        assert_eq!(t.window_back().index(), 2, "the wheel scrolls");
    }

    /// **The indicator line follows the pointer**: with DECSSDT 1 shown, a
    /// move under DEC's locator rewrites the line's locator field and owes
    /// a frame, and so does the move after a one-shot report turned the
    /// locator off, which takes the field away; with no line a move owes
    /// none.
    #[test]
    fn a_move_redraws_the_indicator_line() {
        const WIDE: WinSize = WinSize {
            ws_col: 40,
            ws_row: 6,
        };
        let g = Geometry {
            pad: (0, 0),
            cell: (10.0, 10.0),
            grid: (40, 6),
        };
        let line = |t: &Term<'_>| {
            let cells = t.status_line().map_or(&[][..], |l| l.cells);
            cells
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(' ')
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        let mut owned = Owned::new(&WIDE);
        let mut t = Term::new(owned.storage(), &WIDE);
        // A pointer, as the drivers tell the core.
        t.set_locator_device(baud_core::LocatorDevice::Present);
        let mut seat = Seat::new();
        let move_to = |seat: &mut Seat, t: &mut Term<'_>, x: f64| {
            let mut sent = Vec::new();
            let owed = seat.input(t, Input::Moved(x, 15.0), NONE, g, |b| {
                sent.extend_from_slice(b)
            });
            t.clear_reply();
            owed
        };
        feed(&mut t, b"\x1b[1'z");
        assert_eq!(move_to(&mut seat, &mut t, 25.0), Owed::Nothing, "no line");
        feed(&mut t, b"\x1b[1$~");
        assert_eq!(move_to(&mut seat, &mut t, 25.0), Owed::Frame);
        assert!(line(&t).ends_with("[2,3]"), "{:?}", line(&t));
        assert_eq!(move_to(&mut seat, &mut t, 85.0), Owed::Frame);
        assert!(line(&t).ends_with("[2,9]"), "{:?}", line(&t));
        feed(&mut t, b"\x1b[2'z\x1b['|");
        t.clear_reply();
        assert_eq!(move_to(&mut seat, &mut t, 95.0), Owed::Frame, "one shot");
        assert!(!line(&t).contains('['), "{:?}", line(&t));
    }

    /// **On the alternate screen the wheel is the cursor keys** while
    /// `?1007` is set, which it is from power-on.
    #[test]
    fn the_alternate_screens_wheel_is_the_cursor_keys() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"\x1b[?1049h");
        let mut seat = Seat::new();
        let (owed, sent) = put(&mut seat, &mut t, Input::Steps(2.0), NONE);
        assert_eq!((owed, sent.as_slice()), (Owed::Keys, &b"\x1b[B\x1b[B"[..]));
    }

    /// **A drag selects and is drawn, a double click selects the word, and
    /// a reset drops what was selected.**
    #[test]
    fn a_selection_is_made_drawn_and_dropped() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(1.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(100)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(41.0, 1.0), NONE);
        assert_eq!(
            put(
                &mut seat,
                &mut t,
                up(Pressed::Left, Ms::of_millis(200)),
                NONE
            )
            .0,
            Owed::Selected,
            "made: the driver copies it to SELECT"
        );
        assert_eq!(seat.selected_text(&t).as_deref(), Some("hello"));
        let h = seat.highlight(&t).expect("drawn");
        assert_eq!(
            (h.from, h.to),
            ((Row::ZERO, Col::ZERO), (Row::ZERO, Col::at_index(4)))
        );
        // Two clicks 100 ms apart on "world".
        put(&mut seat, &mut t, Input::Moved(81.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1000)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1050)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1150)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1200)),
            NONE,
        );
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("world"),
            "a double click's word is made too"
        );
        let h = seat.highlight(&t).expect("the word");
        assert_eq!(
            (h.from, h.to),
            (
                (Row::ZERO, Col::at_index(6)),
                (Row::ZERO, Col::at_index(10))
            )
        );
        let before = seat.before_read(&mut t);
        feed(&mut t, b"\x1bc");
        seat.after_read(&t, before);
        assert_eq!(seat.highlight(&t), None, "a reset drops it");
    }

    /// A left drag over the first five cells of row 0, which hold `want`,
    /// pressed and released at the two stamps.
    fn drag(seat: &mut Seat, t: &mut Term<'_>, (press, release): (Ms, Ms), want: &str) {
        put(seat, t, Input::Moved(1.0, 1.0), NONE);
        put(seat, t, down(Pressed::Left, press), NONE);
        put(seat, t, Input::Moved(41.0, 1.0), NONE);
        put(seat, t, up(Pressed::Left, release), NONE);
        assert_eq!(seat.selected_text(t).as_deref(), Some(want), "made");
    }

    /// One read through the seat, as the driver takes it.
    fn read(seat: &mut Seat, t: &mut Term<'_>, bytes: &[u8]) {
        let before = seat.before_read(t);
        feed(t, bytes);
        seat.after_read(t, before);
    }

    /// **While `?1040` is set, output over a selection takes its highlight
    /// away and leaves its text to copy**, xterm's keepSelection, set at
    /// power-on: "keep the selection even after the selected area was
    /// touched by some output" (`[[source: xterm-man]]`). Ctrl+Shift+C then
    /// copies "hello", which the screen no longer shows. Reset, the text goes
    /// with the highlight, as it did before `?1040` was built.
    #[test]
    fn output_over_a_selection_leaves_its_text_while_1040_is_set() {
        for (mode, kept) in [(&b""[..], Some("hello")), (b"\x1b[?1040l", None)] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"hello world");
            feed(&mut t, mode);
            let mut seat = Seat::new();
            drag(
                &mut seat,
                &mut t,
                (Ms::of_millis(100), Ms::of_millis(150)),
                "hello",
            );
            read(&mut seat, &mut t, b"\x1b[1;1HXXXXX");
            assert_eq!(seat.highlight(&t), None, "{mode:?}: the highlight goes");
            assert_eq!(seat.selected_text(&t).as_deref(), kept, "{mode:?}");
        }
    }

    /// **The kept text is the last selection's**: a selection made after
    /// output took one away is what a copy takes, and once output takes
    /// that one away too its text is kept in place of the first; so is a
    /// selection made over another while reads with no output on it went
    /// by. A selection the user clicks away, with no output over it, leaves
    /// its text to copy too: xterm's press takes the highlight and its
    /// release disowns nothing (`StartSelect`, `SelectSet`,
    /// `[[source: xterm-button]]`).
    #[test]
    fn the_kept_text_is_the_last_selections() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let mut seat = Seat::new();
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(100), Ms::of_millis(150)),
            "hello",
        );
        read(&mut seat, &mut t, b"\x1b[1;1HHELLO");
        assert_eq!(seat.selected_text(&t).as_deref(), Some("hello"), "kept");
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(1000), Ms::of_millis(1050)),
            "HELLO",
        );
        read(&mut seat, &mut t, b"\x1b[1;1Hjelly");
        assert_eq!(seat.highlight(&t), None);
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("HELLO"),
            "the second"
        );
        // Made, a read that leaves it, then made again over "world" and
        // written over: the second is kept.
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(1500), Ms::of_millis(1550)),
            "jelly",
        );
        read(&mut seat, &mut t, b"\x1b[3;1Hz");
        put(&mut seat, &mut t, Input::Moved(61.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1700)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(101.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1750)),
            NONE,
        );
        assert_eq!(seat.selected_text(&t).as_deref(), Some("world"), "made");
        read(&mut seat, &mut t, b"\x1b[1;7HWORLD");
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("world"),
            "the third"
        );
        // Made again and clicked away: a press and release in place.
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(5000), Ms::of_millis(5050)),
            "jelly",
        );
        read(&mut seat, &mut t, b"\x1b[3;1Hz");
        put(&mut seat, &mut t, Input::Moved(101.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(6000)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(6050)),
            NONE,
        );
        read(&mut seat, &mut t, b"\x1b[3;1Hz");
        assert_eq!(seat.highlight(&t), None, "clicked away");
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("jelly"),
            "clicked away: its text kept"
        );
    }

    /// **A selection made and clicked away between two reads replaces the
    /// kept text**, as xterm's `SaltTextAway` saves the newer selection
    /// when it is made (`[[source: xterm-button]]`): output takes "hello"
    /// away, "world" is made and clicked away with no read between, and a
    /// copy takes "world" rather than "hello". On an idle screen no read
    /// comes, so the gesture tells the kept text itself; a move that
    /// changes no selection leaves it. Reset, a click keeps nothing.
    #[test]
    fn a_selection_dismissed_between_reads_replaces_the_kept_text() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let mut seat = Seat::new();
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(100), Ms::of_millis(150)),
            "hello",
        );
        read(&mut seat, &mut t, b"\x1b[1;1HHELLO");
        put(&mut seat, &mut t, Input::Moved(61.0, 1.0), NONE);
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("hello"),
            "kept: a move that selects nothing leaves it"
        );
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1000)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(101.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1050)),
            NONE,
        );
        assert_eq!(seat.selected_text(&t).as_deref(), Some("world"), "made");
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(3000)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(3050)),
            NONE,
        );
        assert_eq!(seat.highlight(&t), None, "clicked away");
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("world"),
            "the newer text, not the earlier"
        );
        feed(&mut t, b"\x1b[?1040l");
        drag(
            &mut seat,
            &mut t,
            (Ms::of_millis(5000), Ms::of_millis(5050)),
            "HELLO",
        );
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(7000)),
            NONE,
        );
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(7050)),
            NONE,
        );
        assert_eq!(seat.selected_text(&t), None, "reset: nothing kept");
    }

    /// **A trimmed selection copies the text it was made with** while
    /// `?1040` is set, as xterm's truncating scroll moves only the ends of
    /// the selection and its saved text stays (`[[source: xterm-scroll]]`,
    /// `adjustHiliteOnBakScroll`): a region's scroll down pushes the second
    /// of its two lines out of the region, and the copy is both lines, before
    /// another read and after one. Reset, the scroll drops it and the copy
    /// is nothing. So too where the history its first line was on is gone,
    /// as xterm's text saved when it was made stays.
    #[test]
    fn a_trimmed_selection_copies_the_text_it_was_made_with() {
        for (mode, want) in [(&b""[..], Some("tail1\ntail2")), (b"\x1b[?1040l", None)] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"1\r\n2\r\n3\r\n4\r\ntail1\r\ntail2");
            feed(&mut t, mode);
            let mut seat = Seat::new();
            put(&mut seat, &mut t, Input::Moved(1.0, 41.0), NONE);
            put(
                &mut seat,
                &mut t,
                down(Pressed::Left, Ms::of_millis(100)),
                NONE,
            );
            put(&mut seat, &mut t, Input::Moved(41.0, 51.0), NONE);
            put(
                &mut seat,
                &mut t,
                up(Pressed::Left, Ms::of_millis(150)),
                NONE,
            );
            assert_eq!(
                seat.selected_text(&t).as_deref(),
                Some("tail1\ntail2"),
                "made"
            );
            read(&mut seat, &mut t, b"\x1b[3;6r\x1b[T");
            assert_eq!(seat.selected_text(&t).as_deref(), want, "{mode:?}");
            read(&mut seat, &mut t, b"\x1b[1;1H");
            assert_eq!(seat.selected_text(&t).as_deref(), want, "{mode:?}, later");
        }
        // Clipped where the history it named is gone: four lines kept, and
        // the selection's first line pushed past them.
        let mut owned = Owned::with_history(
            &WS,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(4),
        );
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"L0\r\nL1\r\nL2\r\nL3\r\nL4\r\nL5");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(1.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(100)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(17.0, 41.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(150)),
            NONE,
        );
        let made = "L0\nL1\nL2\nL3\nL4";
        assert_eq!(seat.selected_text(&t).as_deref(), Some(made), "made");
        read(&mut seat, &mut t, b"\r\n\r\n\r\n\r\n\r\n");
        assert_eq!(t.scrollback().len(), 4, "the premise: four lines kept");
        assert!(seat.highlight(&t).is_none(), "the premise: in the history");
        assert_eq!(seat.selected_text(&t).as_deref(), Some(made), "clipped");
        read(&mut seat, &mut t, b"x");
        assert_eq!(seat.selected_text(&t).as_deref(), Some(made), "later");
    }

    /// **A selection the end of the history clips away copies the text it
    /// was made with** while `?1040` is set: xterm's scroll past the saved
    /// lines finds the selection off its margins and disowns it
    /// (`[[source: xterm-scroll]]`, `adjustHiliteOnFwdScroll`, "truncate
    /// because selection is scrolled off margins"), and under
    /// keepSelection that disowning takes only the highlight
    /// (`[[source: xterm-screen]]`, `ScrnDisownSelection`), so the text
    /// saved when it was made stays to paste, after another read too.
    /// Reset, nothing is kept.
    #[test]
    fn a_selection_the_history_clips_away_copies_the_text_it_was_made_with() {
        for (mode, want) in [(&b""[..], Some("L0\nL1")), (b"\x1b[?1040l", None)] {
            let mut owned = Owned::with_history(
                &WS,
                &baud_core::CacheGeometry::UNKNOWN,
                baud_core::Depth::of(4),
            );
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"L0\r\nL1\r\nL2\r\nL3\r\nL4\r\nL5");
            feed(&mut t, mode);
            let mut seat = Seat::new();
            put(&mut seat, &mut t, Input::Moved(1.0, 1.0), NONE);
            put(
                &mut seat,
                &mut t,
                down(Pressed::Left, Ms::of_millis(100)),
                NONE,
            );
            put(&mut seat, &mut t, Input::Moved(17.0, 11.0), NONE);
            put(
                &mut seat,
                &mut t,
                up(Pressed::Left, Ms::of_millis(150)),
                NONE,
            );
            assert_eq!(seat.selected_text(&t).as_deref(), Some("L0\nL1"), "made");
            read(&mut seat, &mut t, &b"\r\n".repeat(12));
            assert_eq!(t.scrollback().len(), 4, "the premise: four lines kept");
            assert!(seat.highlight(&t).is_none(), "the premise: clipped away");
            assert_eq!(seat.selected_text(&t).as_deref(), want, "{mode:?}");
            read(&mut seat, &mut t, b"x");
            assert_eq!(seat.selected_text(&t).as_deref(), want, "{mode:?}, later");
        }
    }

    /// **The Linux driver gives up what SELECT offered where the seat
    /// says** (`Select::GivenUp`): it notes the source a finished
    /// selection's copy set, and gives that one up after the read, so a
    /// copy since keeps its own (`baud_present_wayland`'s `give_up`). The
    /// driver is Wayland-bound, so the source is held.
    #[test]
    fn the_linux_driver_gives_up_select_where_the_seat_says() {
        const SRC: &str = include_str!("linux.rs");
        let read = SRC
            .split("let after = self.seat.after_read(&self.term, before);")
            .nth(1)
            .expect("the read");
        let next = read.find("let ").expect("a statement after");
        assert!(
            read[..next].contains("if after.select == crate::seat::Select::GivenUp {")
                && read[..next].contains("self.give_up_select();"),
            "given up after the read: {}",
            &read[..next]
        );
        let selected = SRC.split("Owed::Selected => {").nth(1).expect("the arm");
        let arm = &selected[..selected.find("\n            }").expect("ends")];
        let copied = arm.find("self.copy_selection(board);").expect("copied");
        let noted = arm
            .find("self.select_offer = self.window.offering(board)")
            .expect("noted");
        assert!(copied < noted, "the source the copy set: {arm}");
        let give = SRC
            .split("fn give_up_select(&mut self) {")
            .nth(1)
            .expect("fn");
        assert!(
            give[..give.find("\n    }").expect("ends")]
                .contains("self.window.give_up(&mut self.conn.sender(), board, source)"),
            "only the source noted"
        );
    }

    /// **Output that takes the selection while `?1040` is reset gives up
    /// what SELECT offered** (`Select::GivenUp`), as xterm's
    /// `DisownSelection` does: a write on its row, and a scroll that takes
    /// its first line off the top. Set, the highlight goes and SELECT keeps
    /// offering the text; a read that leaves the selection gives up
    /// nothing.
    #[test]
    fn output_that_takes_the_selection_while_1040_is_reset_gives_up_select() {
        for (mode, out, want) in [
            (&b"\x1b[?1040l"[..], &b"\x1b[1;1HXX"[..], Select::GivenUp),
            (b"\x1b[?1040l", b"\x1b[6;1H\r\n7", Select::GivenUp),
            (b"", b"\x1b[1;1HXX", Select::Kept),
            (b"", b"\x1b[6;1H\r\n7", Select::Kept),
            (b"\x1b[?1040l", b"\x1b[3;1HXX", Select::Kept),
        ] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"hello world");
            feed(&mut t, mode);
            let mut seat = Seat::new();
            drag(
                &mut seat,
                &mut t,
                (Ms::of_millis(100), Ms::of_millis(150)),
                "hello",
            );
            let before = seat.before_read(&mut t);
            feed(&mut t, out);
            let after = seat.after_read(&t, before);
            assert_eq!(after.select, want, "{mode:?} {out:?}");
        }
    }

    /// **While `?1040` is reset, a scroll that takes the selection's first
    /// line off the top drops it**, as xterm's reset disowns it
    /// (`[[source: xterm-scroll]]`, `adjustHiliteOnFwdScroll`); set, it goes
    /// into the history with its text. A selection on a lower row the
    /// scroll leaves on the screen is kept either way.
    #[test]
    fn while_1040_is_reset_a_selection_scrolled_off_the_top_goes() {
        for (mode, off) in [(&b""[..], Some("hello")), (b"\x1b[?1040l", None)] {
            let mut owned = Owned::new(&WS);
            let mut t = Term::new(owned.storage(), &WS);
            feed(&mut t, b"hello world\r\n2\r\n3\r\n4\r\n5\r\n6");
            feed(&mut t, mode);
            let mut seat = Seat::new();
            drag(
                &mut seat,
                &mut t,
                (Ms::of_millis(100), Ms::of_millis(150)),
                "hello",
            );
            read(&mut seat, &mut t, b"\r\n7");
            assert_eq!(seat.selected_text(&t).as_deref(), off, "{mode:?}");
        }
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"1\r\nhello world\r\n3\r\n4\r\n5\r\n6\x1b[?1040l");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(1.0, 11.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(100)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(41.0, 11.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(150)),
            NONE,
        );
        read(&mut seat, &mut t, b"\r\n7");
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("hello"),
            "the control: row 1, still on the screen"
        );
    }

    /// **The middle button pastes SELECT on its release**, xterm's
    /// `~Ctrl ~Meta <Btn2Up>: insert-selection(SELECT, CUT_BUFFER0)`, its
    /// press doing nothing (`ignore()`): not with Control or Alt held, not
    /// where a tracking child takes the press -- unless Shift overrides the
    /// tracking -- and never touching the selection a left drag made.
    #[test]
    fn the_middle_button_pastes_on_its_release() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(1.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(41.0, 1.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(50)),
            NONE,
        );
        let middle = |seat: &mut Seat, t: &mut Term<'_>, at: u32, mods| {
            let press = put(seat, t, down(Pressed::Middle, Ms::of_millis(at)), mods);
            let release = put(seat, t, up(Pressed::Middle, Ms::of_millis(at + 10)), mods);
            (press, release)
        };
        let (press, release) = middle(&mut seat, &mut t, 1000, NONE);
        assert_eq!(press, (Owed::Nothing, Vec::new()), "the press does nothing");
        assert_eq!(release, (Owed::PasteSelection, Vec::new()));
        assert_eq!(
            seat.selected_text(&t).as_deref(),
            Some("hello"),
            "the selection stands"
        );
        for held in [Modifiers::CTRL, Modifiers::ALT] {
            let (_, release) = middle(&mut seat, &mut t, 2000, Modifiers::NONE.with(held));
            assert_eq!(release.0, Owed::Nothing, "{held:?}");
        }
        feed(&mut t, b"\x1b[?1000h\x1b[?1006h");
        let (press, release) = middle(&mut seat, &mut t, 3000, NONE);
        assert_eq!(press.1, b"\x1b[<1;5;1M", "the child's");
        assert_eq!(release, (Owed::Nothing, b"\x1b[<1;5;1m".to_vec()));
        let shift = Modifiers::NONE.with(Modifiers::SHIFT);
        let (_, release) = middle(&mut seat, &mut t, 4000, shift);
        assert_eq!(
            release,
            (Owed::PasteSelection, Vec::new()),
            "Shift overrides"
        );
    }

    /// **A Ctrl-click on a link is the terminal's, the whole gesture**: the
    /// press arms, a tracking child hears neither half, and the release
    /// over the same link hands it to the driver once; a URI printed as
    /// text opens the same way. The controls: without the keyboard the
    /// press is the child's report, Shift with Ctrl is the selection's,
    /// and a plain click reports. Perturbation: take Shift+Ctrl as the
    /// chord -- the Shift assertion fails.
    #[test]
    fn a_ctrl_click_on_a_link_opens_it() {
        use crate::link::Under;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\ https://a.example/b\x1b[?1000h",
        );
        let mut seat = Seat::new();
        let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
        let shift_ctrl = ctrl.with(Modifiers::SHIFT);
        put(&mut seat, &mut t, Input::Moved(15.0, 5.0), NONE);
        let (owed, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            ctrl,
        );
        assert!(!sent.is_empty(), "unfocused, the child's report: {owed:?}");
        put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(5)), ctrl);
        assert_eq!(seat.take_link(), None, "no keyboard, no link");
        seat.focused(Focus::Gained);
        let (owed, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1000)),
            ctrl,
        );
        assert_eq!((owed, sent.as_slice()), (Owed::Nothing, &b""[..]), "armed");
        let (owed, sent) = put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1005)),
            ctrl,
        );
        assert_eq!((owed, sent.as_slice()), (Owed::Link, &b""[..]));
        assert!(matches!(seat.take_link(), Some(Under::Osc8(_))));
        assert_eq!(seat.take_link(), None, "once");
        put(&mut seat, &mut t, Input::Moved(85.0, 5.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(2000)),
            ctrl,
        );
        let (owed, _) = put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(2005)),
            ctrl,
        );
        assert_eq!(owed, Owed::Link, "a URI in the text");
        assert!(matches!(seat.take_link(), Some(Under::Bare(_))));
        put(&mut seat, &mut t, Input::Moved(15.0, 5.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(3000)),
            shift_ctrl,
        );
        let (owed, _) = put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(3005)),
            shift_ctrl,
        );
        assert_ne!(owed, Owed::Link, "Shift is the selection's");
        assert_eq!(seat.take_link(), None);
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(4000)),
            NONE,
        );
        assert!(!sent.is_empty(), "a plain click is the child's report");
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(4005)),
            NONE,
        );
        assert_eq!(seat.take_link(), None);
    }

    /// **A clean click moves the shell's point under `?2001`**, and a
    /// modified one does not.
    #[test]
    fn a_clean_click_moves_the_point() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"$ abcdef\x1b[?2001h");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Moved(31.0, 2.0), NONE);
        let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            ctrl,
        );
        let (_, sent) = put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(5)), ctrl);
        assert!(sent.is_empty(), "Control is not a clean click: {sent:?}");
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1000)),
            NONE,
        );
        let (_, sent) = put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(1005)),
            NONE,
        );
        assert_eq!(sent, b"\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D");
    }

    /// **A touchpad's lift drops the part-line, the pointer's leaving is
    /// known, and a resize that changes the width drops the selection.**
    #[test]
    fn a_lift_a_leave_and_a_resize() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &b"line\r\n".repeat(30));
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Entered(1.0, 1.0), NONE);
        assert!(seat.inside());
        put(&mut seat, &mut t, Input::Distance(-6.0), NONE);
        put(&mut seat, &mut t, Input::Stopped, NONE);
        put(&mut seat, &mut t, Input::Distance(-6.0), NONE);
        assert_eq!(
            t.window_back().index(),
            0,
            "the first 6 px went with the lift"
        );
        put(&mut seat, &mut t, Input::Distance(-6.0), NONE);
        assert_eq!(t.window_back().index(), 1);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(30.0, 1.0), NONE);
        put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(9)), NONE);
        assert!(seat.highlight(&t).is_some());
        // A size that leaves the grid as it was keeps the selection, and
        // so does a change of height alone, as the macOS driver's does; a
        // change of width drops it.
        let mark = |t: &Term<'_>| (t.history_mark(), t.geometry().0, t.screen());
        let was = mark(&t);
        seat.resized(&t, was);
        assert!(seat.highlight(&t).is_some(), "the same grid keeps it");
        let (cols, rows) = t.geometry();
        let was = mark(&t);
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index()).unwrap(),
            ws_row: u16::try_from(rows.index() + 2).unwrap(),
        });
        seat.resized(&t, was);
        assert!(seat.highlight(&t).is_some(), "a taller window keeps it");
        let was = mark(&t);
        let _ = t.resize(&WinSize {
            ws_col: u16::try_from(cols.index() - 1).unwrap(),
            ws_row: u16::try_from(rows.index()).unwrap(),
        });
        seat.resized(&t, was);
        assert_eq!(seat.highlight(&t), None, "a new width drops it");
        put(&mut seat, &mut t, Input::Left, NONE);
        assert!(!seat.inside());
    }

    /// **A region's scroll carries the selection with its text**, as the
    /// macOS pump does (`keep::followed`): `d`, selected on the region's
    /// fourth row, is still what the selection copies after a line feed at
    /// the region's bottom moved it up a row.
    #[test]
    fn a_region_scroll_carries_the_selection() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"a\r\nb\r\nc\r\nd\r\ne\r\nf\x1b[2;5r");
        let mut seat = Seat::new();
        put(&mut seat, &mut t, Input::Entered(1.0, 35.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(15.0, 35.0), NONE);
        put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(9)), NONE);
        let copied = |seat: &Seat, t: &Term<'_>| {
            seat.selection
                .map(|s| s.text(t, seat.scrolled))
                .unwrap_or_default()
        };
        assert_eq!(copied(&seat, &t), "d", "selected before the scroll");
        let before = seat.before_read(&mut t);
        feed(&mut t, b"\x1b[5;1H\n");
        seat.after_read(&t, before);
        assert_eq!(copied(&seat, &t), "d", "carried up with its row");
    }

    /// **Alt is Meta in a report and starts a block in a selection**, the
    /// macOS rule with Alt for Option: the press and the wheel carry the
    /// meta bit, 8, under tracking; without it Alt drags a block, and Alt
    /// wins over Shift, so Shift+Alt is a block under tracking too.
    #[test]
    fn alt_is_meta_in_a_report_and_drags_a_block() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"abcdef\r\nghijkl\r\nmnopqr\x1b[?1000h\x1b[?1006h");
        let mut seat = Seat::new();
        let alt = Modifiers::NONE.with(Modifiers::ALT);
        put(&mut seat, &mut t, Input::Moved(25.0, 15.0), alt);
        let (_, sent) = put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(0)),
            alt,
        );
        assert_eq!(sent, b"\x1b[<8;3;2M", "Alt is the meta bit");
        let (_, sent) = put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(5)), alt);
        assert_eq!(sent, b"\x1b[<8;3;2m");
        let (_, sent) = put(&mut seat, &mut t, Input::Steps(-1.0), alt);
        assert_eq!(sent, b"\x1b[<72;3;2M", "and on the wheel");

        let block = |seat: &mut Seat, t: &mut Term<'_>, mods, at: Ms| {
            put(seat, t, Input::Moved(11.0, 1.0), mods);
            put(seat, t, down(Pressed::Left, at), mods);
            put(seat, t, Input::Moved(31.0, 21.0), mods);
            put(seat, t, up(Pressed::Left, at), mods);
            seat.highlight(t).expect("drawn")
        };
        let shift_alt = alt.with(Modifiers::SHIFT);
        let h = block(&mut seat, &mut t, shift_alt, Ms::of_millis(1000));
        assert_eq!(h.region, Region::Block, "Alt wins over Shift's override");

        feed(&mut t, b"\x1b[?1000l");
        let h = block(&mut seat, &mut t, alt, Ms::of_millis(2000));
        assert_eq!(
            (h.region, h.from, h.to),
            (
                Region::Block,
                (Row::ZERO, Col::at_index(1)),
                (Row::from_index(2), Col::at_index(3))
            ),
            "Alt drags a block"
        );
        let h = block(&mut seat, &mut t, NONE, Ms::of_millis(3000));
        assert_eq!(h.region, Region::Stream, "the control: no Alt, a stream");
    }

    /// **Ctrl held over a link previews it**: its cells underlined and its
    /// address in a bar on the bottom row; for a URI in the text, across
    /// the soft wrap it runs over. Taken away when Ctrl is let go, with
    /// Shift (the selection's), when the pointer leaves and when the window
    /// loses the keyboard; never shown without the keyboard. Perturbation:
    /// keep the preview without the keyboard -- the first assertion fails.
    #[test]
    fn ctrl_held_over_a_link_previews_it() {
        use crate::link::Host;
        use baud_render::LinkSpan;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\ https://a.example/b",
        );
        let mut seat = Seat::new();
        let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
        let host = Host("");
        put(&mut seat, &mut t, Input::Moved(15.0, 5.0), NONE);
        assert!(!seat.hover(&t, ctrl, G, host), "no keyboard, no preview");
        seat.focused(Focus::Gained);
        assert!(seat.hover(&t, ctrl, G, host), "Ctrl held over the link");
        let look = seat.hover_look().cloned().expect("a preview");
        let span = |r: usize, a: usize, b: usize| LinkSpan {
            row: Row::from_index(r),
            from: Col::at_index(a),
            to: Col::at_index(b),
        };
        assert_eq!(look.spans, vec![span(0, 0, 3)]);
        assert_eq!(look.bar.row, Row::from_index(5), "the bottom row");
        let text: String = look
            .bar
            .text
            .iter()
            .filter_map(|c| char::from_u32(c.value()))
            .collect();
        assert_eq!(
            text, " https://\u{2026}ple.com\u{2026}/ ",
            "shortened to the row"
        );
        assert!(seat.hover(&t, NONE, G, host), "Ctrl let go");
        assert!(seat.hover_look().is_none());
        let shift_ctrl = ctrl.with(Modifiers::SHIFT);
        assert!(
            !seat.hover(&t, shift_ctrl, G, host),
            "Shift is the selection's"
        );
        // The URI in the text, from column 5 of the first row over the wrap
        // to column 3 of the second.
        put(&mut seat, &mut t, Input::Moved(75.0, 5.0), NONE);
        assert!(seat.hover(&t, ctrl, G, host));
        let look = seat.hover_look().cloned().expect("a preview");
        assert_eq!(look.spans, vec![span(0, 5, 19), span(1, 0, 3)]);
        // Leaving takes it away, and so does losing the keyboard.
        put(&mut seat, &mut t, Input::Left, NONE);
        assert!(seat.hover(&t, ctrl, G, host), "the pointer left");
        put(&mut seat, &mut t, Input::Moved(15.0, 5.0), NONE);
        assert!(seat.hover(&t, ctrl, G, host));
        seat.focused(Focus::Lost);
        assert!(seat.hover_look().is_none(), "the keyboard gone");
        assert!(!seat.hover_refresh(&t, ctrl, host));
        // **A read under a pointer whose Ctrl went up unreported draws no
        // preview**: Ctrl held over plain text, then a link written under
        // the resting pointer, read again with Ctrl up. The control: with
        // Ctrl still held, the link is previewed.
        seat.focused(Focus::Gained);
        put(&mut seat, &mut t, Input::Moved(15.0, 45.0), NONE);
        assert!(!seat.hover(&t, ctrl, G, host), "plain text under it");
        feed(
            &mut t,
            b"\x1b[5;1H\x1b]8;;https://c.example/\x1b\\later\x1b]8;;\x1b\\",
        );
        assert!(
            !seat.hover_refresh(&t, NONE, host),
            "Ctrl up: nothing drawn"
        );
        assert!(seat.hover_look().is_none());
        assert!(
            seat.hover_refresh(&t, ctrl, host),
            "the control: Ctrl held, the pointer resting"
        );
    }

    /// **A preview held over a window on the history follows the view**:
    /// Ctrl held over the top row of a window three lines up, and two lines
    /// of output that move the window up with its line, so the view shows
    /// the same lines; the frame's refresh previews the link drawn under
    /// the pointer, the one a move there would. The control: before the
    /// output the refresh changes nothing. Perturbation:
    /// refresh with the `back` of the last pointer event -- this fails.
    #[test]
    fn a_held_preview_follows_the_window_over_the_history() {
        use crate::link::Host;
        let mut owned = Owned::with_history(
            &WS,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(50),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut bytes = Vec::new();
        for n in 0..14 {
            bytes.extend_from_slice(
                format!("\x1b]8;;http://l{n}/\x1b\\L{n}\x1b]8;;\x1b\\\r\n").as_bytes(),
            );
        }
        feed(&mut t, &bytes);
        t.set_window_back(baud_domain::Rows::of_count(3));
        let mut seat = Seat::new();
        seat.focused(Focus::Gained);
        let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
        let host = Host("");
        put(&mut seat, &mut t, Input::Moved(5.0, 5.0), NONE);
        assert!(seat.hover(&t, ctrl, G, host));
        let bar = |seat: &Seat| -> String {
            let look = seat.hover_look().expect("a preview");
            look.bar
                .text
                .iter()
                .filter_map(|c| char::from_u32(c.value()))
                .collect()
        };
        let shown = bar(&seat);
        assert!(!seat.hover_refresh(&t, ctrl, host), "the control");
        feed(&mut t, b"more\r\nmore\r\n");
        let _ = seat.hover_refresh(&t, ctrl, host);
        assert_eq!(bar(&seat), shown, "the link drawn under the pointer");
    }

    /// **A drag over the cells the display draws from a row above the
    /// window selects them**: "Y" ten times, cut after its sixth letter at
    /// the window's top, is drawn whole on the window's first line, five
    /// of the letters under columns 1 to 5 stored on the row above the
    /// window. A press at column 1 and a release at column 8 select the
    /// eight letters between, five of them from above the window; naming
    /// the window cell nearest each instead, the selection began at the
    /// window row's first letter and held three.
    #[test]
    fn a_drag_over_cells_drawn_from_above_the_window_selects_them() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            format!(
                "h0\r\nh1\r\n{} {} zz\r\nf1\r\nf2\r\nf3\r\nf4\r\n\x1b]133;A\x07$ ",
                "X".repeat(13),
                "Y".repeat(10)
            )
            .as_bytes(),
        );
        let mut seat = Seat::new();
        seat.scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
        let mut order = baud_render::Reorder::new();
        order.lay_out(&t, Rows::ZERO);
        assert!(
            order.orders().display().is_some(),
            "the premise: the paragraph is drawn at its words"
        );
        seat.drawn(order.orders());
        put(&mut seat, &mut t, Input::Entered(15.0, 5.0), NONE);
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(10)),
            NONE,
        );
        put(&mut seat, &mut t, Input::Moved(85.0, 5.0), NONE);
        put(
            &mut seat,
            &mut t,
            up(Pressed::Left, Ms::of_millis(20)),
            NONE,
        );
        assert_eq!(seat.selected_text(&t).as_deref(), Some("YYYYYYYY"));
        let h = seat.highlight(&t).expect("drawn");
        let shown = order.view(&t, Rows::ZERO).row(Row::ZERO).to_vec();
        let lit: Vec<bool> = (0..13)
            .map(|x| {
                order
                    .orders()
                    .selects(&h, Row::ZERO, Col::at_index(x), shown[x].content)
            })
            .collect();
        let want: Vec<bool> = (0..13).map(|x| (1..9).contains(&x)).collect();
        assert_eq!(lit, want, "drawn selected where the pointer went");
    }

    /// **The seat's copy reads `?96` by the display's wrap it was given**
    /// (`wrap_words`): on, a finished paragraph the display draws at its
    /// words copies by its display lines, the English line from its right
    /// edge whole; off, by the stored rows, where the cut "mid|dle" falls
    /// inside the English.
    #[test]
    fn the_seats_copy_reads_decrlcm_by_its_word_wrap() {
        let ws = WinSize {
            ws_row: 6,
            ws_col: 44,
        };
        let he1 = "\u{5D6}\u{5D5} \u{5E4}\u{5E1}\u{5E7}\u{5D4} \u{5D1}\u{5E2}\u{5D1}\u{5E8}\u{5D9}\u{5EA} ";
        let he2 = "\u{5E9}\u{5DE}\u{5DE}\u{5E9}\u{5D9}\u{5DB}\u{5D4} \u{5DC}\u{5E9}\u{5D5}\u{5E8}\u{5D4} \u{5D4}\u{5D1}\u{5D0}\u{5D4}.";
        let history: String = (0..8).map(|i| format!("h{i}\r\n")).collect();
        let bytes = format!(
            "{history}{he1}with English words in the middle, {he2}\r\n\x1b]133;A\x07$ \x1b[?96h"
        );
        let mut owned = Owned::new(&ws);
        let mut t = Term::new(owned.storage(), &ws);
        feed(&mut t, bytes.as_bytes());
        let scrolled = Scrolled::START.advanced(t.looking_back(Rows::of_index(usize::MAX)));
        let line = |r: usize| scrolled.line(Rows::ZERO, Row::from_index(r));
        let row = |r: usize| t.view(Rows::ZERO).row(Row::from_index(r));
        let mut sel = Selection::at(
            baud_render::Span::cell(row(3), line(3), Col::ZERO),
            baud_render::Grain::Cell,
        );
        sel.extend(baud_render::Span::cell(row(4), line(4), Col::at_index(43)));
        let mut seat = Seat::new();
        seat.selection = Some(sel);
        seat.scrolled = scrolled;
        let on = seat.selected_text(&t).expect("text");
        assert!(on.contains(" ,elddim eht ni"), "{on}");
        seat.set_word_wrap(baud_render::WordWrap::Off);
        let off = seat.selected_text(&t).expect("text");
        assert!(off.contains("dim eht"), "{off}");
        assert!(!off.contains("elddim"), "{off}");
    }

    /// **The functions of `src`, by name, each with its code collapsed**:
    /// comment lines and whitespace left out. A function starts at a line
    /// that opens one at the top level or in an `impl`.
    fn functions(src: &str) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in src.lines() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            let head = ["fn ", "pub fn ", "pub(crate) fn "]
                .iter()
                .find_map(|h| t.strip_prefix(h))
                .filter(|_| indent <= 4);
            if let Some(rest) = head {
                out.push((
                    rest.chars().take_while(|c| *c != '(').collect(),
                    String::new(),
                ));
            } else if !t.starts_with("//") {
                if let Some((_, code)) = out.last_mut() {
                    code.extend(t.chars().filter(|c| !c.is_whitespace()));
                }
            }
        }
        out
    }

    /// **Ctrl held over a link a click opens names the hand**, over a
    /// program's `OSC 22` crosshair and under its tracking; the crosshair
    /// again when Ctrl is let go and when the keyboard goes. Perturbation:
    /// the program's shape alone -- the first assertion fails.
    #[test]
    fn ctrl_over_a_link_names_the_hand() {
        use crate::link::Host;
        use baud_core::PointerShape;
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(
            &mut t,
            b"\x1b]22;crosshair\x1b\\\x1b[?1000h\x1b]8;;https://example.com/\x1b\\link\x1b]8;;\x1b\\",
        );
        let asked = t.pointer_shape();
        let mut seat = Seat::new();
        seat.focused(Focus::Gained);
        let ctrl = Modifiers::NONE.with(Modifiers::CTRL);
        put(&mut seat, &mut t, Input::Moved(15.0, 5.0), NONE);
        assert!(seat.hover(&t, ctrl, G, Host("")));
        assert_eq!(seat.pointer_shape(asked), Some(PointerShape::Hand2));
        assert!(seat.hover(&t, NONE, G, Host("")));
        assert_eq!(seat.pointer_shape(asked), asked, "Ctrl let go");
        assert!(seat.hover(&t, ctrl, G, Host("")));
        seat.focused(Focus::Lost);
        assert_eq!(seat.pointer_shape(asked), asked, "the keyboard gone");
    }

    /// **Over the scrollbar's strip, while it takes a left press, the
    /// pointer keeps its shape over a link**, in the Linux driver's order
    /// -- the scrollbar told of a move, then who a left press goes to,
    /// then the preview, and again at each frame with the thumb it draws.
    /// Ctrl held over a link on a window three lines up the history: one
    /// column left of the strip the hand (the control); in the strip
    /// before a frame has drawn the thumb, the hand, since a press there
    /// is still the cells'; the frame that draws it, the program's
    /// crosshair, since a click jumps the thumb (`bar::press`); out and
    /// back over the same link, the hand and the crosshair, though the
    /// preview does not change; on the live screen, with the pointer
    /// resting in the strip, the crosshair while the thumb fades and the
    /// hand on the frame that draws it gone. Perturbations: the hand
    /// whoever takes the press -- the crosshair assertions fail; the
    /// change of who takes it not told -- the frame's assertion fails.
    #[test]
    fn over_the_scrollbar_a_link_keeps_the_programs_pointer() {
        use crate::bar::{Bar, Event, Frame, Held, Took};
        use crate::link::Host;
        use baud_core::PointerShape;
        use core::time::Duration;
        let mut owned = Owned::with_history(
            &WS,
            &baud_core::CacheGeometry::UNKNOWN,
            baud_core::Depth::of(50),
        );
        let mut t = Term::new(owned.storage(), &WS);
        let mut bytes = b"\x1b]22;crosshair\x1b\\".to_vec();
        for n in 0..14 {
            bytes.extend_from_slice(
                format!(
                    "\x1b]8;;http://l{n}/\x1b\\{}\x1b]8;;\x1b\\\r\n",
                    "x".repeat(19)
                )
                .as_bytes(),
            );
        }
        feed(&mut t, &bytes);
        t.set_window_back(baud_domain::Rows::of_count(3));
        let asked = t.pointer_shape();
        assert_eq!(asked, Some(PointerShape::Crosshair));
        let f = Frame {
            size: (baud_font::Px::of(200), baud_font::Px::of(60)),
            cell_h: baud_font::Px::of(10),
            widths: crate::reveal::widths(crate::reveal::Scale(1.0)),
        };
        let at = |ms: u64| {
            baud_domain::HostTime::of_secs(100.0)
                .expect("a time")
                .plus(Duration::from_millis(ms))
        };
        let (ctrl, host) = (Modifiers::NONE.with(Modifiers::CTRL), Host(""));
        let mut seat = Seat::new();
        seat.focused(Focus::Gained);
        let mut b = Bar::new();
        // A move: the scrollbar, who a left press goes to, the preview.
        let step = |seat: &mut Seat, t: &mut Term<'_>, b: &mut Bar, x: f64, ms: u64| {
            seat.input_beside(
                t,
                Input::Moved(x, 5.0),
                ctrl,
                G,
                |_| {},
                |term, beside| match beside.input {
                    Input::Moved(x, y) => {
                        let e = Event::Moved {
                            x,
                            y,
                            held: Held::No,
                        };
                        b.input(term, e, f, at(ms)).0
                    }
                    _ => Took::No,
                },
            );
            let pressed = seat.hover_press(b.left_press(t, f, at(ms)));
            let hovered = seat.hover(t, ctrl, G, host);
            (pressed, hovered)
        };
        // A frame: the thumb drawn, who a left press goes to read again.
        let frame = |seat: &mut Seat, t: &Term<'_>, b: &mut Bar, ms: u64| {
            let _ = b.turn(at(ms), t);
            b.drawn = b.look(at(ms));
            seat.hover_press(b.left_press(t, f, at(ms)))
        };
        let _ = step(&mut seat, &mut t, &mut b, 175.0, 0);
        assert!(seat.hover_look().is_some(), "a link under the pointer");
        assert_eq!(
            seat.pointer_shape(asked),
            Some(PointerShape::Hand2),
            "the control: beside the strip"
        );
        assert_eq!(step(&mut seat, &mut t, &mut b, 185.0, 1), (false, false));
        assert_eq!(
            seat.pointer_shape(asked),
            Some(PointerShape::Hand2),
            "no thumb drawn yet"
        );
        assert!(frame(&mut seat, &t, &mut b, 2), "the thumb drawn");
        assert_eq!(seat.pointer_shape(asked), asked, "a click scrolls");
        assert_eq!(step(&mut seat, &mut t, &mut b, 175.0, 3), (true, false));
        assert_eq!(seat.pointer_shape(asked), Some(PointerShape::Hand2));
        assert_eq!(
            step(&mut seat, &mut t, &mut b, 185.0, 4),
            (true, false),
            "back in over the same link"
        );
        assert_eq!(seat.pointer_shape(asked), asked);
        // The wheel to the live screen reveals the thumb, which then fades
        // under the resting pointer.
        t.set_window_back(baud_domain::Rows::of_count(0));
        b.reveal.reveal(at(5));
        let _ = frame(&mut seat, &t, &mut b, 5);
        let _ = seat.hover_refresh(&t, ctrl, host);
        assert!(seat.hover_look().is_some(), "a link on the live screen");
        assert_eq!(seat.pointer_shape(asked), asked, "the thumb held");
        let gone = (5..=2000)
            .step_by(16)
            .find(|ms| frame(&mut seat, &t, &mut b, *ms))
            .expect("the thumb fades");
        assert_eq!(b.drawn.shown.get(), 0, "gone at {gone} ms");
        assert_eq!(
            seat.pointer_shape(asked),
            Some(PointerShape::Hand2),
            "a click opens"
        );
    }

    /// **The Linux driver shows the pointer again at every change of the
    /// link preview, and keeps a hidden one hidden**: every function of
    /// `linux.rs` that moves the preview -- a pointer event, a change of
    /// the modifiers, a frame's read, the keyboard's coming and going --
    /// shows the pointer again; showing it asks the seat with the
    /// program's `OSC 22` shape and sends nothing while a key has it
    /// hidden, since a shape set shows it (`[[source: wayland]]`). The
    /// driver runs only on Linux, so this holds its source.
    /// Perturbations: drop the show after a change of the modifiers, or
    /// the hidden guard; each fails here.
    #[test]
    fn the_linux_driver_shows_the_pointer_at_every_change_of_the_preview() {
        const SRC: &str = include_str!("linux.rs");
        // Spelled apart, so this test's own text is not what it finds.
        let calls = [
            concat!("seat.", "hover("),
            concat!("seat.", "hover_refresh("),
            concat!("seat.", "focused("),
            concat!("seat.", "hover_press("),
        ];
        let mut names = Vec::new();
        for (name, code) in functions(SRC) {
            if calls.iter().any(|c| code.contains(c)) {
                let after = calls
                    .iter()
                    .filter_map(|c| code.rfind(c))
                    .max()
                    .expect("a call");
                assert!(
                    code[after..].contains(concat!(".show_", "pointer()")),
                    "{name} moves the preview and does not show the pointer after"
                );
                names.push(name);
            }
        }
        assert_eq!(names.len(), 4, "the four sites: {names:?}");
        let head = concat!("    fn show_", "pointer(&mut self) {");
        assert_eq!(SRC.matches(head).count(), 1);
        let show = SRC.split(head).nth(1).expect(head);
        let show = &show[..show.find("\n    }\n").expect("the function ends")];
        let code: String = show.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            code.contains(concat!("self.hiding.hidden()==", "PointerHides::Yes")),
            "a hidden pointer is shown by a shape"
        );
        assert!(
            code.contains(concat!(
                "self.seat.pointer_shape(self.term.",
                "pointer_shape())"
            )),
            "the shape is not the preview's"
        );
    }
}
