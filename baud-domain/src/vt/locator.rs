//! DEC's text locator: the pointer as `[[source: dec-std-070]]` 070-13,
//! the Text Locator Extension, gives it -- DECELR, DECSLE, DECEFR, DECRQLP
//! and the report they produce, DECLRP, `CSI Pe ; Pb ; Pr ; Pc ; Pp & w`.
//!
//! **One of ctlseqs' mutually exclusive mouse protocols**: "The mouse
//! protocols include DEC Locator mode, enabled by the DECELR CSI Ps ; Ps '
//! z control sequence" (`[[source: ctlseqs]]`), so the switch is
//! [`MouseTracking::Locator`] and what is here is the rest of its state:
//! the units, the events, the filter rectangle and the pointer the
//! embedder last gave.
//!
//! Each control's parameters are read here, out of the core's dispatch, the
//! way [`crate::QueryOp`]'s are.

use crate::*;

/// Which of the four locator controls a `CSI ... '` sequence is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorOp {
    /// DECELR, `CSI Ps ; Pu ' z`.
    Enable,
    /// DECSLE, `CSI Pm ' {`.
    SelectEvents,
    /// DECEFR, `CSI Pt ; Pl ; Pb ; Pr ' w`.
    FilterRectangle,
    /// DECRQLP, `CSI Ps ' |`.
    RequestPosition,
}

/// Whether the embedder has a pointer to feed the locator (the core's
/// `Term::locator`): 070-13's "No
/// locator" against "Locator ready", "a locator device is plugged in"
/// (p. 15, PDF 1030). **`Absent` until the embedder says**, so a core
/// embedded with no pointer answers `DSR ?55` with `?53`, "No locator",
/// and `?56` with `?57;0`, "Unknown Locator", as ctlseqs pairs them, and
/// DECRQLP and DECEFR with DECLRP's "the locator is unavailable (not
/// enabled, or physically not present)".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LocatorDevice {
    Present,
    #[default]
    Absent,
}

/// DECELR's `Ps` when it enables: "1-Locator reports enabled 2-One shot
/// (allow one report, then disable)" (070-13 p. 5, PDF 1020).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LocatorReporting {
    #[default]
    Continuous,
    /// "the next trigger event that occurs generates a single locator
    /// report. No further locator reports occur (the locator is
    /// disabled), until another DECELR sequence is received."
    OneShot,
}

/// DECELR's `Pu`: "0-(or omitted) Default to character cells 1-Device
/// physical pixels 2-Character cells".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LocatorUnits {
    #[default]
    Cells,
    /// The embedder's device pixels, the unit [`PointerPx`] is in.
    Pixels,
}

/// What a DECELR asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorEnable {
    /// `Ps` 0 or omitted: "Locator disabled (default)".
    Off,
    On(LocatorReporting, LocatorUnits),
}

impl LocatorEnable {
    /// DECELR's two parameters, or `None` for a value the text does not
    /// define: "Undefined values for Ps or Pu causes the entire sequence to
    /// be ignored" (070-13 p. 5). xterm reads any `Ps` but 1 and 2 as off and
    /// any `Pu` but 1 as cells (`CASE_DECELR`, `[[source: xterm-reset]]`'s
    /// file), a practice not followed.
    pub fn of(p: &Params<'_>) -> Option<LocatorEnable> {
        let units = match p.raw(ParamIdx::SECOND, 0) {
            0 | 2 => LocatorUnits::Cells,
            1 => LocatorUnits::Pixels,
            _ => return None,
        };
        Some(match p.raw(ParamIdx::FIRST, 0) {
            0 => LocatorEnable::Off,
            1 => LocatorEnable::On(LocatorReporting::Continuous, units),
            2 => LocatorEnable::On(LocatorReporting::OneShot, units),
            _ => return None,
        })
    }
}

/// Which button transitions DECSLE asked to have reported: none at
/// power-up, "0-Respond only to explicit host requests (default)".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LocatorEvents(u8);

impl LocatorEvents {
    pub const NONE: LocatorEvents = LocatorEvents(0);
    /// "1-Report button down transitions".
    pub const DOWN: LocatorEvents = LocatorEvents(1);
    /// "3-Report button up transitions".
    pub const UP: LocatorEvents = LocatorEvents(2);

    pub fn has(self, e: LocatorEvents) -> bool {
        self.0 & e.0 == e.0 && e.0 != 0
    }
}

/// One DECSLE parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorSelect {
    /// 0 or omitted: "Respond only to explicit host requests (default,
    /// also cancels any pending filter rectangle)".
    RequestsOnly,
    /// 1 to 4: the events after this parameter.
    Events(LocatorEvents),
}

impl LocatorEvents {
    /// The selection after one DECSLE parameter `p`, or `None` for a value
    /// 070-13 does not give (p. 9, PDF 1024), which changes nothing.
    pub fn select(self, p: Option<Param>) -> Option<LocatorSelect> {
        let v = p.map_or(0, |v| v.0);
        let d = LocatorEvents::DOWN.0;
        let u = LocatorEvents::UP.0;
        Some(match v {
            0 => LocatorSelect::RequestsOnly,
            1 => LocatorSelect::Events(LocatorEvents(self.0 | d)),
            2 => LocatorSelect::Events(LocatorEvents(self.0 & !d)),
            3 => LocatorSelect::Events(LocatorEvents(self.0 | u)),
            4 => LocatorSelect::Events(LocatorEvents(self.0 & !u)),
            _ => return None,
        })
    }
}

/// DECRQLP's `Ps`: "0-(or omitted) Default to 1. 1-Transmit a single
/// locator report (DECLRP)" (070-13 p. 8, PDF 1023); `None` for any other,
/// which the text does not give.
pub fn locator_request(p: &Params<'_>) -> Option<()> {
    matches!(p.raw(ParamIdx::FIRST, 0), 0 | 1).then_some(())
}

/// One of the locator's buttons: "up to 4 buttons", "1-Right 2-Middle
/// 4-Left 8-Fourth" (DECLRP, 070-13 p. 6). The fourth is a tablet
/// cursor's in the text ("Ky4 = B4 (tablet cursor only)", § 5.4); on a
/// mouse it is the first button past the three (Jev, FIRST_EXTRA, 0.98 and
/// 0.98).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorButton {
    Left,
    Middle,
    Right,
    Fourth,
}

impl LocatorButton {
    fn bit(self) -> u8 {
        match self {
            LocatorButton::Right => 1,
            LocatorButton::Middle => 2,
            LocatorButton::Left => 4,
            LocatorButton::Fourth => 8,
        }
    }

    /// The event code of this button going down: "2-Left button down
    /// ... 4-Middle button down ... 6-Right button down ... 8-Fourth button
    /// down"; up is one more.
    fn down(self) -> u16 {
        match self {
            LocatorButton::Left => 2,
            LocatorButton::Middle => 4,
            LocatorButton::Right => 6,
            LocatorButton::Fourth => 8,
        }
    }
}

/// The buttons held: "Pb is the button code, ASCII decimal 0 - 15,
/// indicating which, if any, of up to 4 buttons are down". The embedder
/// keeps it, from every press and release it sees, and hands it in with
/// each event, so a report carries the state after its own transition.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LocatorButtons(u8);

impl LocatorButtons {
    pub const NONE: LocatorButtons = LocatorButtons(0);

    pub fn pressed(self, b: LocatorButton) -> LocatorButtons {
        LocatorButtons(self.0 | b.bit())
    }
    pub fn released(self, b: LocatorButton) -> LocatorButtons {
        LocatorButtons(self.0 & !b.bit())
    }
    pub fn has(self, b: LocatorButton) -> bool {
        self.0 & b.bit() != 0
    }
    fn code(self) -> u16 {
        u16::from(self.0)
    }
}

/// Where the pointer is, as the embedder gives it: the cell under it and
/// its device pixel, both zero-based from the text area's top-left, as a
/// mouse report's are ([`MouseEvent`]). The pixel is `None` when the
/// embedder does not know it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocatorAt {
    pub col: Col,
    pub row: Row,
    pub px: Option<PointerPx>,
}

/// What happened to the pointer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorKind {
    /// It moved, entered or left, or a button changed that is not the
    /// locator's to report (one the selection took).
    Moved,
    Press(LocatorButton),
    Release(LocatorButton),
}

/// One pointer event for the locator: what happened, where -- `None`
/// outside the text area, "the locator position is undefined (outside the
/// terminal window for example)" (DECLRP) -- and the buttons held after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocatorInput {
    pub kind: LocatorKind,
    pub at: Option<LocatorAt>,
    pub held: LocatorButtons,
}

/// A position as DECLRP writes it, "in the units selected by the last
/// received Enable Locator Reports (DECELR)", one-based: "The origin is
/// coordinate pair 1:1 in the upper-left corner" (DECEFR).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocatorPos {
    pub row: OneBased,
    pub col: OneBased,
}

impl LocatorPos {
    /// A cell, zero-based from the origin.
    pub fn cell(row: Row, col: Col) -> LocatorPos {
        LocatorPos {
            row: row.one_based(),
            col: col.one_based(),
        }
    }
    /// A pixel, zero-based from the text area's first; `None` at the last
    /// value `ImagePx` holds, whose one-based form the field cannot carry.
    pub fn pixel(px: PointerPx) -> Option<LocatorPos> {
        Some(LocatorPos {
            row: OneBased(px.y.raw().checked_add(1)?),
            col: OneBased(px.x.raw().checked_add(1)?),
        })
    }
}

/// DECEFR's corners as they arrived: "If any parameters are omitted, they
/// are defaulted to the current locator position" (070-13 p. 10, PDF
/// 1025). A 0 is the default, as for every DEC parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FilterCorners {
    top: Option<OneBased>,
    left: Option<OneBased>,
    bottom: Option<OneBased>,
    right: Option<OneBased>,
}

impl FilterCorners {
    pub fn of(p: &Params<'_>) -> FilterCorners {
        FilterCorners {
            top: p.far_margin(ParamIdx::FIRST),
            left: p.far_margin(ParamIdx::SECOND),
            bottom: p.far_margin(ParamIdx::THIRD),
            right: p.far_margin(ParamIdx::FOURTH),
        }
    }

    /// Whether any corner was given: note 2's "a filter rectangle with
    /// explicit coordinates", which triggers at once while the position is
    /// undefined, where one with every corner omitted waits for the
    /// locator to come back.
    pub fn any_given(self) -> bool {
        self.top.is_some() || self.left.is_some() || self.bottom.is_some() || self.right.is_some()
    }

    /// The rectangle, its omitted corners at `at`.
    pub fn at(self, at: LocatorPos) -> FilterRect {
        FilterRect {
            top: self.top.unwrap_or(at.row),
            left: self.left.unwrap_or(at.col),
            bottom: self.bottom.unwrap_or(at.row),
            right: self.right.unwrap_or(at.col),
        }
    }
}

/// An armed filter rectangle. "The filter rectangle includes the
/// boundaries". **Taken as given** when its top is below its bottom or its
/// left right of its right: it contains no position, so it triggers at
/// once, as the text says "a rectangle that does not contain the locator"
/// does. DEC's area operations ignore such a rectangle, and xterm swaps
/// its corners (`InitLocatorFilter`, `[[source: xterm-button-c]]`);
/// ignoring it leaves the program waiting for a report that never comes,
/// which 070-13 avoids wherever it gives a reason (Jev's lean, AS_GIVEN,
/// 0.89 and 0.85 on a second ask, after IGNORE at 0.82 and 0.69).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FilterRect {
    top: OneBased,
    left: OneBased,
    bottom: OneBased,
    right: OneBased,
}

impl FilterRect {
    pub fn contains(self, p: LocatorPos) -> bool {
        self.top.get() <= p.row.get()
            && p.row.get() <= self.bottom.get()
            && self.left.get() <= p.col.get()
            && p.col.get() <= self.right.get()
    }
}

/// The filter rectangle's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LocatorFilter {
    #[default]
    Off,
    /// Set and containing the locator: the next position outside it, or
    /// an undefined one, reports and ends it.
    Armed(FilterRect),
    /// Every corner omitted while the position was undefined: "the
    /// rectangle shall be triggered the next time the locator is within
    /// the defined coordinate space" (DECEFR note 2).
    Pending,
}

/// What a DECLRP reports: "the specific event that initiated the report"
/// (070-13 § 3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorTrigger {
    /// "1-Request; the terminal received an explicit request".
    Request,
    Down(LocatorButton),
    Up(LocatorButton),
    /// "10-Locator outside filter rectangle".
    Outside,
}

impl LocatorTrigger {
    fn code(self) -> u16 {
        match self {
            LocatorTrigger::Request => 1,
            LocatorTrigger::Down(b) => b.down(),
            LocatorTrigger::Up(b) => b.down() + 1,
            LocatorTrigger::Outside => 10,
        }
    }
}

/// One DECLRP.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorReport {
    /// `CSI 0 & w`: "the locator is unavailable (not enabled, or physically
    /// not present)"; "If Pe is 0 or omitted, all other parameters shall
    /// not be sent".
    Unavailable,
    /// `CSI Pe ; Pb ; Pr ; Pc & w`, or `CSI Pe ; Pb & w` with the position
    /// undefined: "The parameter values Pr and Pc shall be omitted,
    /// indicating locator position undefined" (DECLRP note 3). **No `Pp`**:
    /// "The page coordinate may be omitted if the locator is on page one
    /// (the default)", and baud has one page; xterm sends none either.
    At {
        trigger: LocatorTrigger,
        buttons: LocatorButtons,
        pos: Option<LocatorPos>,
    },
}

impl LocatorReport {
    /// The parameters before `& w`, in order; `len` of them are live.
    pub(crate) fn params(self) -> ([u16; 4], usize) {
        match self {
            LocatorReport::Unavailable => ([0, 0, 0, 0], 1),
            LocatorReport::At {
                trigger,
                buttons,
                pos: None,
            } => ([trigger.code(), buttons.code(), 0, 0], 2),
            LocatorReport::At {
                trigger,
                buttons,
                pos: Some(p),
            } => (
                [trigger.code(), buttons.code(), p.row.get(), p.col.get()],
                4,
            ),
        }
    }
}
