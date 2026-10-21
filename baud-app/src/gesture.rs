//! **A pointer event, from either driver, to where it goes**: the child,
//! which tracks the mouse, or the selection -- decided at the press and
//! kept to its release -- the cell and the pixel under it, the selection a
//! press, a drag and a release make, and a click's `?2001` and `OSC 133`
//! keys. Pure, over a real `Term`, so both drivers share one rule and a
//! test reaches it; the routing and the point arithmetic moved here from
//! `app.rs` when the Linux driver took a pointer.

use crate::readline::{self, Arrow};
use baud_core::{ClickHow, ClickKeys, Resource, Term};
use baud_domain::{
    Col, ImagePx, KeyCode, KeyEvent, LocatorAt, LocatorButton, LocatorButtons, LocatorInput,
    LocatorKind, Modifiers, MouseButton, MouseTracking, PointerPx, RegisInput, Row, Rows,
    ShiftEscape,
};
use baud_render::{Grain, Press, Scrolled, Selection, Span};

/// What a pointer event is, as either driver delivers it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    Down,
    Up,
    /// Moved with no button held.
    Moved,
    /// Moved with a button held.
    Dragged,
    /// Into or out of the window.
    Crossed,
}

/// The button an event names: a press's or a release's, else the left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pressed {
    Left,
    Middle,
    Right,
    /// The first button past the three -- macOS's `buttonNumber` 3, the
    /// Wayland seat's `BTN_SIDE` -- which DEC's locator reports as its
    /// fourth (Jev, FIRST_EXTRA, 0.98 and 0.98).
    Fourth,
    Other,
}

impl Pressed {
    /// The button a report carries: none for one the protocol has no code
    /// for.
    pub(crate) fn reported(self) -> MouseButton {
        match self {
            Pressed::Left => MouseButton::Left,
            Pressed::Middle => MouseButton::Middle,
            Pressed::Right => MouseButton::Right,
            Pressed::Fourth | Pressed::Other => MouseButton::None,
        }
    }

    /// The button as DEC's locator names it, or `None` past the fourth.
    fn locator(self) -> Option<LocatorButton> {
        match self {
            Pressed::Left => Some(LocatorButton::Left),
            Pressed::Middle => Some(LocatorButton::Middle),
            Pressed::Right => Some(LocatorButton::Right),
            Pressed::Fourth => Some(LocatorButton::Fourth),
            Pressed::Other => None,
        }
    }
}

/// **The buttons held after an event**, for DEC's locator: "Pb ...
/// indicating which, if any, of up to 4 buttons are down" (`[[source:
/// dec-std-070]]` 070-13, DECLRP). Kept by the driver from every press and
/// release it sees, whoever takes them, so a report carries the state after
/// its own transition, as xterm's does.
pub(crate) fn held_after(held: LocatorButtons, kind: Kind, button: Pressed) -> LocatorButtons {
    match (kind, button.locator()) {
        (Kind::Down, Some(b)) => held.pressed(b),
        (Kind::Up, Some(b)) => held.released(b),
        _ => held,
    }
}

/// **Whether a pointer event owes a frame for DEC's locator**: while the
/// indicator line is shown, whose locator field the event may have moved
/// or, after a one-shot report turned the locator off, taken away
/// (`Term::locator`). 070-14 holds an indicator to 80 ms behind the change
/// it shows (`[[source: dec-std-070]]` pp. 14-5 and 14-6), and a steady
/// cursor draws no frame of its own. Both drivers ask it after every
/// event the locator sees.
pub(crate) fn locator_redraws(term: &Term<'_>) -> bool {
    term.status_type() == baud_core::StatusType::Indicator
}

/// **One pointer event for DEC's locator** (`Term::locator`): a press or
/// a release the route gave the child while the locator is the protocol
/// in force, a move otherwise -- a move, a crossing, or a press the
/// selection or the scrollbar took, whose position and buttons the core
/// keeps and reports nothing for.
pub(crate) fn locator_input(
    (kind, button, to): (Kind, Pressed, To),
    tracking: MouseTracking,
    at: Option<LocatorAt>,
    held: LocatorButtons,
) -> LocatorInput {
    let reported = to == To::Child && tracking == MouseTracking::Locator;
    let kind = match (kind, button.locator(), reported) {
        (Kind::Down, Some(b), true) => LocatorKind::Press(b),
        (Kind::Up, Some(b), true) => LocatorKind::Release(b),
        _ => LocatorKind::Moved,
    };
    LocatorInput { kind, at, held }
}

/// Where a pointer event goes once the scrollbar and links have passed it:
/// to the child, which tracks the mouse, or to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum To {
    Child,
    Select,
}

/// Whether Shift is held at a pointer event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shift {
    Held,
    Up,
}

/// **Whether Shift, if held, overrides the child's tracking for this
/// event**: XTSHIFTESCAPE's choice, `Term::shift_escape`. At 0, the
/// default, Shift takes every event, as xterm's `ShiftOverride` does under
/// `shiftEscape` false; at 1 it takes only a button the selection binds
/// without Shift -- xterm's `shift_buttons` (`[[source: xterm-button-c]]`),
/// which for baud's bindings is the left button, the one a press selects
/// with -- and every other press goes to the child with its Shift bit
/// (Jev's lean, SELECTION_BUTTONS, 0.80/0.88 then 0.78/0.78). A move with
/// no button held is button 1's, as xterm's `MotionButton` reads one.
pub(crate) fn shift_overrides(
    held: Shift,
    escape: ShiftEscape,
    kind: Kind,
    button: Pressed,
) -> Shift {
    match (held, escape, kind, button) {
        (Shift::Up, ..) => Shift::Up,
        (_, ShiftEscape::Override, ..) => Shift::Held,
        (_, ShiftEscape::Conditional, Kind::Moved, _)
        | (_, ShiftEscape::Conditional, _, Pressed::Left) => Shift::Held,
        (_, ShiftEscape::Conditional, ..) => Shift::Up,
    }
}

/// **The tracking the pointer is routed by**: the core's own, unless ReGIS
/// graphics input runs, when the buttons are the core's as they are DEC's
/// locator's -- "pressing a locator button sends a cursor position report
/// to the host" (`[[source: vt3xx-locator]]`) -- so a press reaches
/// `Term::locator` as a press and not the selection. Shift still selects.
/// A tracking protocol in force reports the same press as well, the two
/// kept independent as 070-13 § 5.5 keeps ReGIS's and DEC's locator
/// reports.
pub(crate) fn pointer_tracking(tracking: MouseTracking, regis: RegisInput) -> MouseTracking {
    match regis {
        RegisInput::Off => tracking,
        RegisInput::OneShot | RegisInput::Multiple => MouseTracking::Locator,
    }
}

/// Who owns the press in progress, decided at the press. `Free` between
/// presses.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owner {
    Free,
    Pressed(To),
}

/// **Where one pointer event goes, and who owns the press after it.** The
/// child's when it tracks the mouse, unless Shift is held, which is a
/// selection whatever the child asked for: xterm's default, ctlseqs'
/// XTSHIFTESCAPE at 0. **Decided at the press and kept to its release**, as
/// the scrollbar's grab and a link's arming are: Shift pressed or let go
/// mid-drag moves neither the drag nor the release, so the child never sees
/// a press without its release -- under `?1001` that release is the one
/// that ends the drawn region and sends the report (`[[source: ctlseqs]]`,
/// "until button release, and then sends the program the release
/// coordinates") -- and a selection never loses the release that ends it.
/// A move with no button held has no press to follow and is decided as it
/// comes, as is a drag or release whose press nothing here saw. A second
/// button pressed mid-drag decides again, so a press whose release was lost
/// cannot hold the next gesture.
pub(crate) fn pointer_route(
    owner: Owner,
    kind: Kind,
    tracking: MouseTracking,
    shift: Shift,
) -> (To, Owner) {
    let now = match (tracking, shift) {
        (MouseTracking::Off, _) | (_, Shift::Held) => To::Select,
        _ => To::Child,
    };
    match (kind, owner) {
        (Kind::Down, _) => (now, Owner::Pressed(now)),
        (Kind::Dragged, Owner::Pressed(to)) => (to, owner),
        (Kind::Up, Owner::Pressed(to)) => (to, Owner::Free),
        (Kind::Up, Owner::Free) => (now, Owner::Free),
        _ => (now, owner),
    }
}

/// The cell under a point **and the pixel it is at, from one computation**,
/// so the two cannot name different places: `?1016` reports the pixel where
/// every other encoding reports the cell, and an application divides one by
/// the cell size to get the other.
///
/// The pixel is the point in device pixels -- the unit `Term::set_cell_size`
/// is given, and so the unit `CSI 16 t` and `?2048` answer in -- less the
/// padding, zero-based from the grid's first pixel. It is clamped to the
/// text area the way the cell is clamped to the grid, so a pointer over the
/// padding or past the last cell reports the edge pixel of the edge cell.
/// xterm does not clamp: it reports `event->y - OriginY(screen)` whatever it
/// is (`EditorButton`, `[[source: xterm-button]]`), which over the border is
/// zero or negative and on the wire a `0` or a `-1` -- a position no cell
/// has, and one this cell would contradict.
///
/// The cell is the pixel divided by the cell size. It was the point divided
/// by the cell size before the pixel existed, and for a whole-pixel cell,
/// which the renderer's metrics always are, the two are the same number.
pub(crate) fn pointer_under(
    at: (f64, f64),
    scale: f64,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
) -> (Row, Col, PointerPx) {
    let axis = |p: f64, pad: u32, cell: f64, cells: u16| {
        let extent = (cell * f64::from(cells)).floor() as i64;
        let px = ((p * scale - f64::from(pad)).floor() as i64).clamp(0, (extent - 1).max(0));
        let index =
            ((px as f64 / cell).floor() as i64).clamp(0, i64::from(cells).saturating_sub(1).max(0));
        (
            usize::try_from(index).unwrap_or(0),
            ImagePx::of(u16::try_from(px).unwrap_or(u16::MAX)),
        )
    };
    let (col, x) = axis(at.0, pad.0, cell.0, grid.0);
    let (row, y) = axis(at.1, pad.1, cell.1, grid.1);
    (Row::from_index(row), Col::at_index(col), PointerPx { x, y })
}

/// **The cell and pixel under a point for DEC's locator, or `None`
/// outside the text area**: [`pointer_under`]'s arithmetic, unclamped. A
/// mouse report names the edge cell for a pointer over the padding; the
/// locator says it is outside "the range of defined coordinates", and
/// reports it with the coordinates omitted (`[[source: dec-std-070]]`
/// 070-13 § 3.1).
pub(crate) fn locator_under(
    at: (f64, f64),
    scale: f64,
    cell: (f64, f64),
    grid: (u16, u16),
    pad: (u32, u32),
) -> Option<LocatorAt> {
    let inside = |p: f64, pad: u32, cell: f64, cells: u16| {
        let extent = (cell * f64::from(cells)).floor();
        let px = (p * scale - f64::from(pad)).floor();
        (0.0..extent).contains(&px)
    };
    if !(inside(at.0, pad.0, cell.0, grid.0) && inside(at.1, pad.1, cell.1, grid.1)) {
        return None;
    }
    let (row, col, px) = pointer_under(at, scale, cell, grid, pad);
    Some(LocatorAt {
        col,
        row,
        px: Some(px),
    })
}

/// What a pointer event did to the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Selected {
    /// Nothing the selection binds: a frame is not owed.
    Unchanged,
    Changed,
    /// A release that ended a press with no drag: the selection is gone, and
    /// the click is the driver's to pass to [`clicked`].
    Click,
    /// A release that ended a drag: the selection stands.
    Made,
}

/// **The selection a press, a drag and a release make**, on the cell `at`
/// of the view drawn: a left press starts one, by the cell, the word or
/// the line as its clicks count one, two or more, as `press` says (a new
/// stream, a block, or the one there extended); a drag with the left
/// button held moves its head; a release ends it. `held` is the button the
/// driver holds for its reports, set and cleared here as a press and a
/// release the selection took.
#[allow(clippy::too_many_arguments)]
pub(crate) fn select(
    term: &Term<'_>,
    scrolled: Scrolled,
    selection: &mut Option<Selection>,
    held: &mut Option<MouseButton>,
    (kind, button, clicks): (Kind, Pressed, u8),
    press: Press,
    (row, col): (Row, Col),
) -> Selected {
    let back = term.window_back();
    let view = term.view(back);
    // The line's own column: half the display's on a sized line.
    let col = baud_render::char_col(&view, row, col);
    let top = scrolled.top(back);
    let line = scrolled.line(back, row);
    let span_at = |grain: Grain| match grain {
        Grain::Cell => Span::cell(view.row(row), line, col),
        Grain::Word => Span::word(term, view.row(row), line, col),
        Grain::Line => Span::line(&view, top, row),
    };
    match kind {
        Kind::Down if button == Pressed::Left => {
            let grain = match clicks {
                0 | 1 => Grain::Cell,
                2 => Grain::Word,
                _ => Grain::Line,
            };
            *selection = Some(Selection::pressed(*selection, press, grain, span_at));
            *held = Some(MouseButton::Left);
            Selected::Changed
        }
        Kind::Dragged if *held == Some(MouseButton::Left) => {
            if let Some(sel) = selection.as_mut() {
                sel.extend(span_at(sel.grain()));
            }
            Selected::Changed
        }
        Kind::Up => {
            *held = None;
            match selection.is_some_and(|s| s.is_click()) {
                true => {
                    *selection = None;
                    Selected::Click
                }
                false => Selected::Made,
            }
        }
        _ => Selected::Unchanged,
    }
}

/// **A click the driver has passed as clean** -- the left button, no
/// modifier, the window on the live screen -- at the cell `at` and the
/// pixel `px`, whose cell is `cell_w` pixels wide. **A prompt that asked
/// with `OSC 133` takes it first** (`Term::prompt_click`): its `cl=` or
/// kitty's `click_events=` is the shell saying what it handles, and `?2001`
/// is reached only where no prompt asked (Jev, SPEC_LINE, 0.80/0.89, the
/// lean taken). Then **`?2001`: a click on the cursor's line moves the
/// shell's point there**, with as many cursor keys as `readline::point_move`
/// counts, sent as the wheel's are -- the terminal's, not keystrokes, so KAM
/// does not lock them and SRM does not draw them. xterm's `isClick1_clean`
/// refuses a Shift click and a tracked one, and its rows are the screen's;
/// a click is a press and a release with no drag between, and xterm also
/// bounds its time by `multiClickTime`, which this does not. Every byte to
/// `sink`; the driver flushes.
pub(crate) fn clicked(
    term: &mut Term<'_>,
    (row, col, px): (Row, Col, PointerPx),
    cell_w: ImagePx,
    mut sink: impl FnMut(&[u8]),
) {
    if prompt_click(term, (row, col), &mut sink) {
        return;
    }
    if !term.resource(Resource::ClickMovesPoint) {
        return;
    }
    let col = readline::boundary(px.x, cell_w);
    let view = term.viewport();
    let Some((arrow, n)) = readline::point_move(
        |r| view.wrapped(r),
        term.cursor(),
        (row, col),
        term.geometry().0,
    ) else {
        return;
    };
    let code = match arrow {
        Arrow::Left => KeyCode::Left,
        Arrow::Right => KeyCode::Right,
    };
    for _ in 0..n {
        crate::reads::input(
            term,
            |t| t.wheel_key(KeyEvent::press(code, Modifiers::default())),
            &mut sink,
        );
    }
}

/// **A click at the prompt the shell is at, as its `A` asked**
/// (`Term::prompt_click`): kitty's `click_events=` sends the click as
/// an SGR report, `cl=` the keys that move the line editor's cursor to
/// it (`readline::prompt_move`) -- the cursor keys as the wheel sends
/// them, kitty's `special_key=1` pair, or the proposal's `move-keys=`.
/// Whether a prompt asked, and so took the click.
fn prompt_click(term: &mut Term<'_>, click: (Row, Col), sink: &mut impl FnMut(&[u8])) -> bool {
    let Some(asked) = term.prompt_click() else {
        return false;
    };
    match asked.how {
        ClickHow::Report(events) => {
            if let Some(bytes) = readline::prompt_report(events, asked.top, asked.later, click) {
                sink(&bytes);
            }
        }
        ClickHow::Keys(keys) => {
            let view = term.viewport();
            let rows = Rows::of_count(term.geometry().1.index());
            let Some((arrow, n)) = readline::prompt_move(
                |r| view.row(r),
                |r| view.wrapped(r),
                rows,
                term.cursor(),
                click,
                asked.input,
            ) else {
                return true;
            };
            let left = arrow == Arrow::Left;
            for _ in 0..n {
                match keys {
                    ClickKeys::Arrows => {
                        let code = if left { KeyCode::Left } else { KeyCode::Right };
                        crate::reads::input(
                            term,
                            |t| t.wheel_key(KeyEvent::press(code, Modifiers::default())),
                            &mut *sink,
                        );
                    }
                    // `[[source: kitty-6d5d0c4-screen]]`, its fake cursor
                    // move to a click: "1" right,
                    // "1;1" left, each `CSI ... u`.
                    ClickKeys::Special => sink(match left {
                        true => b"\x1b[1;1u",
                        false => b"\x1b[1u",
                    }),
                    ClickKeys::Sequences(k) => sink(match left {
                        true => k.left(),
                        false => k.right(),
                    }),
                }
            }
        }
    }
    true
}

/// `pointer_route`: the press decides, and Shift mid-drag does not.
#[cfg(test)]
mod route_tests {
    use super::{Kind as K, Owner, Shift, To, pointer_route};
    use baud_domain::MouseTracking;

    /// Feed `events` (kind, Shift held) from `Free`, and say where each went.
    fn routed(tracking: MouseTracking, events: &[(K, Shift)]) -> (Vec<To>, Owner) {
        let mut owner = Owner::Free;
        let mut to = Vec::new();
        for &(kind, shift) in events {
            let (t, next) = pointer_route(owner, kind, tracking, shift);
            to.push(t);
            owner = next;
        }
        (to, owner)
    }

    /// **Shift pressed mid-drag keeps the release the child's.** Under
    /// `?1001` that release ends the drawn region and sends the report; per
    /// event, the selection took it and the region stayed drawn.
    #[test]
    fn shift_pressed_mid_drag_leaves_the_release_with_the_child() {
        let (to, owner) = routed(
            MouseTracking::Highlight,
            &[
                (K::Down, Shift::Up),
                (K::Dragged, Shift::Held),
                (K::Up, Shift::Held),
            ],
        );
        assert_eq!(to, [To::Child, To::Child, To::Child]);
        assert_eq!(owner, Owner::Free, "the release frees it");
    }

    /// **Shift let go mid-drag keeps a selection the selection's**, to the
    /// release that ends it.
    #[test]
    fn shift_let_go_mid_drag_leaves_the_selection_its_release() {
        let (to, owner) = routed(
            MouseTracking::Normal,
            &[
                (K::Down, Shift::Held),
                (K::Dragged, Shift::Up),
                (K::Up, Shift::Up),
            ],
        );
        assert_eq!(to, [To::Select, To::Select, To::Select]);
        assert_eq!(owner, Owner::Free);
    }

    /// **Tracking switched on or off mid-drag moves nothing either**: the
    /// press decided.
    #[test]
    fn a_tracking_change_mid_drag_leaves_the_press_s_owner() {
        let mut owner = Owner::Free;
        let (t, next) = pointer_route(owner, K::Down, MouseTracking::Off, Shift::Up);
        owner = next;
        assert_eq!(t, To::Select);
        let (t, next) = pointer_route(owner, K::Up, MouseTracking::Normal, Shift::Up);
        assert_eq!((t, next), (To::Select, Owner::Free));
    }

    /// **A move with no press is decided as it comes**, so `?1003` hover
    /// reaches the child and Shift hover does not, and it leaves the owner.
    #[test]
    fn a_move_with_no_press_is_decided_as_it_comes() {
        for (shift, want) in [(Shift::Up, To::Child), (Shift::Held, To::Select)] {
            let (t, next) = pointer_route(Owner::Free, K::Moved, MouseTracking::AnyEvent, shift);
            assert_eq!((t, next), (want, Owner::Free));
        }
    }

    /// **XTSHIFTESCAPE 1 hands Shift only the left button.** At 0 Shift
    /// overrides tracking for every button; at 1 the middle and right
    /// buttons, which baud's selection binds nothing to, go to the child
    /// with their Shift bit, and the left button and a bare move stay the
    /// selection's (`[[source: ctlseqs]]`, "conditionally allow shift-key as
    /// modifier in mouse protocol").
    #[test]
    fn xtshiftescape_1_hands_shift_only_the_selection_button() {
        use super::{Pressed as B, ShiftEscape as E, shift_overrides};
        let over = |e, k, b| shift_overrides(Shift::Held, e, k, b);
        for b in [B::Left, B::Middle, B::Right] {
            assert_eq!(over(E::Override, K::Down, b), Shift::Held, "{b:?} at 0");
        }
        assert_eq!(over(E::Conditional, K::Down, B::Left), Shift::Held);
        assert_eq!(over(E::Conditional, K::Down, B::Middle), Shift::Up);
        assert_eq!(over(E::Conditional, K::Down, B::Right), Shift::Up);
        assert_eq!(
            over(E::Conditional, K::Moved, B::Right),
            Shift::Held,
            "a bare move is button 1's"
        );
        assert_eq!(
            shift_overrides(Shift::Up, E::Override, K::Down, B::Left),
            Shift::Up,
            "nothing held, nothing overridden"
        );
        // Through the route: Shift+Middle under 1 is the child's.
        let (t, _) = pointer_route(
            Owner::Free,
            K::Down,
            MouseTracking::Normal,
            over(E::Conditional, K::Down, B::Middle),
        );
        assert_eq!(t, To::Child);
    }

    /// **DEC's locator is told a press only when it is the child's under
    /// DECELR**; a press the selection took, or one under another
    /// protocol, is a move, which keeps the position and reports nothing.
    /// The held buttons count every press and release, whoever took it.
    #[test]
    fn the_locator_gets_a_press_only_through_the_child_route() {
        use super::{Pressed as B, held_after, locator_input};
        use baud_domain::{LocatorButton, LocatorButtons, LocatorKind};
        let held = held_after(LocatorButtons::NONE, K::Down, B::Right);
        assert_eq!(held, LocatorButtons::NONE.pressed(LocatorButton::Right));
        assert_eq!(held_after(held, K::Up, B::Right), LocatorButtons::NONE);
        assert_eq!(held_after(held, K::Down, B::Other), held, "no fifth");
        let kind = |to, tracking| locator_input((K::Down, B::Left, to), tracking, None, held).kind;
        assert_eq!(
            kind(To::Child, MouseTracking::Locator),
            LocatorKind::Press(LocatorButton::Left)
        );
        assert_eq!(kind(To::Select, MouseTracking::Locator), LocatorKind::Moved);
        assert_eq!(kind(To::Child, MouseTracking::Normal), LocatorKind::Moved);
        let other = locator_input(
            (K::Up, B::Other, To::Child),
            MouseTracking::Locator,
            None,
            held,
        );
        assert_eq!(other.kind, LocatorKind::Moved);
    }

    /// **ReGIS graphics input takes the buttons**: with no tracking a
    /// press is the core's and reaches `Term::locator` as a press, not the
    /// selection; Shift still selects; with input off the core's own
    /// tracking routes as before.
    #[test]
    fn regis_input_takes_the_buttons_from_the_selection() {
        use super::{Pressed as B, locator_input, pointer_tracking};
        use baud_domain::{LocatorButton, LocatorButtons, LocatorKind, RegisInput};
        for regis in [RegisInput::OneShot, RegisInput::Multiple] {
            let tracking = pointer_tracking(MouseTracking::Off, regis);
            let (to, _) = pointer_route(Owner::Free, K::Down, tracking, Shift::Up);
            assert_eq!(to, To::Child, "{regis:?}");
            let e = locator_input((K::Down, B::Left, to), tracking, None, LocatorButtons::NONE);
            assert_eq!(e.kind, LocatorKind::Press(LocatorButton::Left));
            let (to, _) = pointer_route(Owner::Free, K::Down, tracking, Shift::Held);
            assert_eq!(to, To::Select, "Shift selects");
        }
        let off = pointer_tracking(MouseTracking::Off, RegisInput::Off);
        assert_eq!(off, MouseTracking::Off);
        let (to, _) = pointer_route(Owner::Free, K::Down, off, Shift::Up);
        assert_eq!(to, To::Select);
        assert_eq!(
            pointer_tracking(MouseTracking::Normal, RegisInput::Off),
            MouseTracking::Normal
        );
    }

    /// **Off the text area the locator's position is undefined**: over the
    /// padding, or past the last cell, where a mouse report names the edge
    /// cell; inside, the same cell and pixel as `pointer_under`.
    #[test]
    fn the_locator_s_position_is_undefined_off_the_text_area() {
        use super::{locator_under, pointer_under};
        let g = (2.0, (10.0, 20.0), (8, 4), (6, 6));
        let at = |x, y| locator_under((x, y), g.0, g.1, g.2, g.3);
        assert_eq!(at(2.0, 2.0), None, "over the padding");
        assert_eq!(at(43.0, 10.0), None, "past the last column");
        assert_eq!(at(10.0, 43.0), None, "past the last row");
        let inside = at(42.9, 42.9).expect("the last cell");
        let (row, col, px) = pointer_under((42.9, 42.9), g.0, g.1, g.2, g.3);
        assert_eq!((inside.row, inside.col, inside.px), (row, col, Some(px)));
        assert_eq!((row.index(), col.index()), (3, 7));
    }

    /// **A press decides again**, even over an owner whose release was lost.
    #[test]
    fn a_press_decides_again() {
        let stale = Owner::Pressed(To::Child);
        let (t, next) = pointer_route(stale, K::Down, MouseTracking::Normal, Shift::Held);
        assert_eq!((t, next), (To::Select, Owner::Pressed(To::Select)));
    }
}

/// The selection a gesture makes and a clean click's keys, over a real
/// `Term`.
#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_col: 20,
        ws_row: 6,
    };

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// **A press, a drag and a release select; a press and a release with
    /// no drag is a click**, which leaves nothing selected for the driver to
    /// pass on; a right press selects nothing.
    #[test]
    fn a_drag_selects_and_a_click_does_not() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let (mut sel, mut held) = (None, None);
        let s = Scrolled::START;
        let at = |k, b, c: usize, sel: &mut Option<Selection>, held: &mut _| {
            select(
                &t,
                s,
                sel,
                held,
                (k, b, 1),
                Press::Stream,
                (Row::ZERO, Col::at_index(c)),
            )
        };
        assert_eq!(
            at(Kind::Down, Pressed::Right, 0, &mut sel, &mut held),
            Selected::Unchanged
        );
        assert!(sel.is_none());
        assert_eq!(
            at(Kind::Down, Pressed::Left, 0, &mut sel, &mut held),
            Selected::Changed
        );
        assert_eq!(held, Some(MouseButton::Left));
        assert_eq!(
            at(Kind::Dragged, Pressed::Left, 4, &mut sel, &mut held),
            Selected::Changed
        );
        assert_eq!(
            at(Kind::Up, Pressed::Left, 4, &mut sel, &mut held),
            Selected::Made
        );
        assert_eq!(held, None);
        assert_eq!(sel.expect("made").text(&t, s), "hello");
        at(Kind::Down, Pressed::Left, 7, &mut sel, &mut held);
        assert_eq!(
            at(Kind::Up, Pressed::Left, 7, &mut sel, &mut held),
            Selected::Click
        );
        assert!(sel.is_none(), "a click selects nothing");
    }

    /// **A double click selects the word** under it.
    #[test]
    fn a_second_click_selects_the_word() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"hello world");
        let (mut sel, mut held) = (None, None);
        let s = Scrolled::START;
        select(
            &t,
            s,
            &mut sel,
            &mut held,
            (Kind::Down, Pressed::Left, 2),
            Press::Stream,
            (Row::ZERO, Col::at_index(8)),
        );
        assert_eq!(sel.expect("pressed").text(&t, s), "world");
    }

    /// **`?2001`: a clean click on the cursor's line sends the cursor keys
    /// that move the point there**, the column rounded at the cell's middle;
    /// without the mode, nothing.
    #[test]
    fn a_clean_click_moves_the_point_under_2001() {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, b"$ abcdef");
        let cell = ImagePx::of(10);
        let px = PointerPx {
            x: ImagePx::of(31),
            y: ImagePx::of(2),
        };
        let mut sent = Vec::new();
        clicked(&mut t, (Row::ZERO, Col::at_index(3), px), cell, |b| {
            sent.extend_from_slice(b)
        });
        assert!(sent.is_empty(), "no ?2001, no keys: {sent:?}");
        feed(&mut t, b"\x1b[?2001h");
        clicked(&mut t, (Row::ZERO, Col::at_index(3), px), cell, |b| {
            sent.extend_from_slice(b)
        });
        assert_eq!(sent, b"\x1b[D\x1b[D\x1b[D\x1b[D\x1b[D", "8 to 3, five left");
    }
}
