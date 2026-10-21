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
    Before, Ring, followed, kept_across_resize, numbered, oldest, reset_in, snap_live,
    stale_in_moved_rows, switched, tracked_highlight,
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
/// driver reads no desktop setting. Jev, XTERM_250, 1.00 and 0.98.
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

/// What turns a point into a cell: the cell in pixels and the grid in
/// cells. The Linux frame draws the grid from its first pixel, so there is
/// no padding, and no buffer scale, so a point is a pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
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
    scrolled: Scrolled,
    /// A Ctrl press the link gesture took, and the link its release opens
    /// (`link::gesture`); whether the window has the keyboard, which a press
    /// must for it to arm.
    armed: Option<crate::link::Armed>,
    opening: Option<crate::link::Under>,
    focus: Focus,
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
            scrolled: Scrolled::START,
            armed: None,
            opening: None,
            focus: Focus::Lost,
        }
    }

    /// The keyboard came or went: a link arms only in a focused window.
    pub(crate) fn focused(&mut self, f: Focus) {
        self.focus = f;
    }

    /// The link a [`Owed::Link`] opens, once.
    pub(crate) fn take_link(&mut self) -> Option<crate::link::Under> {
        self.opening.take()
    }

    /// Whether the pointer is over the surface, for its shape.
    pub(crate) fn inside(&self) -> bool {
        self.at.is_some()
    }

    /// Where the pointer is over the surface, for the scrollbar's press.
    pub(crate) fn at(&self) -> Option<(f64, f64)> {
        self.at
    }

    /// Whether a button is physically down, for a move's kind.
    pub(crate) fn button_down(&self) -> bool {
        self.down.is_some()
    }

    /// **A move the scrollbar took** (`bar.rs`): the position is kept, so
    /// the next event's cell and the pointer's shape are right, and no one
    /// else is told.
    pub(crate) fn moved_to(&mut self, x: f64, y: f64) {
        self.at = Some((x, y));
    }

    fn under(&self, g: Geometry) -> (Row, Col, PointerPx) {
        let at = self.at.unwrap_or((0.0, 0.0));
        gesture::pointer_under(at, 1.0, g.cell, g.grid, (0, 0))
    }

    /// Where DEC's locator is: `None` off the surface or its text area.
    fn locator_at(&self, g: Geometry) -> Option<LocatorAt> {
        self.at
            .and_then(|at| gesture::locator_under(at, 1.0, g.cell, g.grid, (0, 0)))
    }

    /// **One pointer input**: the wheel to the history, the alternate
    /// screen's keys or a tracking child's reports (`wheel::route`); a
    /// press, a drag, a move and a release to the child that tracks the
    /// mouse or to the selection, decided at the press with Shift's
    /// override (`gesture::pointer_route`); a click to the prompt or
    /// `?2001`. Every byte for the child to `sink`.
    pub(crate) fn input(
        &mut self,
        term: &mut Term<'_>,
        e: Input,
        mods: Modifiers,
        g: Geometry,
        sink: impl FnMut(&[u8]),
    ) -> Owed {
        let pointer = !matches!(e, Input::Steps(_) | Input::Distance(_) | Input::Stopped);
        // A ReGIS cursor the pointer moved is a frame too.
        let drawn = term.images().generation();
        match self.route(term, e, mods, g, sink) {
            Owed::Nothing if pointer && gesture::locator_redraws(term) => Owed::Frame,
            Owed::Nothing if term.images().generation() != drawn => Owed::Frame,
            owed => owed,
        }
    }

    /// [`Seat::input`]'s routing, before the indicator's frame.
    fn route(
        &mut self,
        term: &mut Term<'_>,
        e: Input,
        mods: Modifiers,
        g: Geometry,
        mut sink: impl FnMut(&[u8]),
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
            Input::Pressed { button, time } => {
                self.down = Some(button);
                let _ = self.clicks.press(button, time);
                (Kind::Down, button)
            }
            Input::Released { button, time } => {
                if self.down == Some(button) {
                    self.down = None;
                }
                self.clicks.release(button, time);
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
        // or a crossing here, a press or a release once routed, below.
        self.buttons = gesture::held_after(self.buttons, kind, button);
        if !matches!(kind, Kind::Down | Kind::Up) {
            let e = LocatorInput {
                kind: LocatorKind::Moved,
                at: self.locator_at(g),
                held: self.buttons,
            };
            crate::reads::input(term, |t| t.locator(e), &mut sink);
        }
        // **A Ctrl press over a link is the terminal's, the whole
        // gesture**, before the child's tracking, as Cmd is on macOS, so a
        // child never sees a release without its press (`link::gesture`;
        // gnome-terminal's chord, `[[source: gnome-terminal-screen]]`
        // 2010-2022; Jev's lean, CTRL, 0.52/0.78, over kitty's Ctrl+Shift).
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
            let selected = gesture::select(
                term,
                self.scrolled,
                &mut self.selection,
                &mut self.held,
                (kind, button, self.clicks.count.0),
                press_of(mods),
                (row, col),
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

    /// The wheel: whole lines of `delta`, one line a detent -- Jev's lean,
    /// ONE, 0.14 and 0.27, the protocol's logical step taken as a line, as
    /// AppKit's line is on macOS -- routed as the macOS app routes them.
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
        self.selection = selection;
    }

    /// The selection's text, as Ctrl+Shift+C and a finished drag copy it;
    /// `None` without one or for one with no text.
    pub(crate) fn selected_text(&self, term: &Term<'_>) -> Option<String> {
        let text = self.selection?.text(term, self.scrolled);
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
        (self.selection, self.scrolled) =
            kept_across_resize(term, was, self.selection.take(), self.scrolled);
    }

    /// The readings a read is judged by, with the core's watches armed on
    /// the selection's lines (`keep::Before::watched`).
    pub(crate) fn before_read(&self, term: &mut Term<'_>) -> Before {
        Before::watched(term, self.selection.as_ref(), self.scrolled)
    }

    /// **What a read did to the selection**: dropped where it wrote, erased
    /// or moved in place on its rows, renumbered by the lines the surface
    /// retired or took back, carried with its text by a scroll
    /// (`keep::followed`), dropped by a switch or a reset with the
    /// wheel's part-line, and clipped where the history it named is gone --
    /// the macOS pump's order (`app.rs`, `pump`).
    pub(crate) fn after_read(&mut self, term: &Term<'_>, before: Before) -> AfterRead {
        // The macOS pump's `restart`: a switch is a surface the trail and
        // the fade answer, and a reset made from the primary while the
        // window looked back brings it live, which reveals the thumb.
        let after = AfterRead {
            surface: match switched(term, before) {
                true => Surface::Switched,
                false => Surface::Same,
            },
            thumb: match term.first_erasure(before.erased) {
                Some(Erasure::Reset(Screen::Normal, Looking::Back)) => Thumb::Reveal,
                _ => Thumb::Leave,
            },
        };
        if self.selection.is_some() && stale_in_moved_rows(term) {
            self.selection = None;
        }
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
            self.selection = None;
        }
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
            None => self
                .selection
                .and_then(|s| s.highlight(top, view.rows(), view.cols())),
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

    /// **A move the scrollbar took keeps the position and tells no one**,
    /// and a button down is seen for the next move's kind.
    #[test]
    fn a_taken_move_keeps_the_position() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        let mut seat = Seat::new();
        assert_eq!(seat.at(), None);
        seat.moved_to(12.0, 3.0);
        assert_eq!(seat.at(), Some((12.0, 3.0)));
        assert!(!seat.button_down());
        put(
            &mut seat,
            &mut t,
            down(Pressed::Left, Ms::of_millis(1)),
            NONE,
        );
        assert!(seat.button_down());
        put(&mut seat, &mut t, up(Pressed::Left, Ms::of_millis(2)), NONE);
        assert!(!seat.button_down());
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
        let (_, sent) = put(&mut seat, &mut t, down(Pressed::Fourth, ms(15)), NONE);
        assert_eq!(sent, b"\x1b[8;12;2;3&w", "the side button is the fourth");
        put(&mut seat, &mut t, up(Pressed::Fourth, ms(16)), NONE);
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
}
