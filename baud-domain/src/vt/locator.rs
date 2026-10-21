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

/// What a locator key's string is sent for in ReGIS graphics input: no
/// transition -- a report the host asked for -- or a button pressed or
/// released.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorTransition {
    Null,
    Down(LocatorButton),
    Up(LocatorButton),
}

/// "There are six bytes available to each of the locator keys" (070-13
/// § 5.4); "You can only use 6 characters per button transition"
/// (`[[source: vt3xx-locator]]`).
pub const LOCATOR_KEY_MAX: usize = 6;

/// One transition's string: the default, or what DECLKD loaded, up to
/// [`LOCATOR_KEY_MAX`] bytes, empty for a transition that reports nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KeyDef {
    Default,
    Loaded {
        bytes: [u8; LOCATOR_KEY_MAX],
        len: u8,
    },
}

impl KeyDef {
    const EMPTY: KeyDef = KeyDef::Loaded {
        bytes: [0; LOCATOR_KEY_MAX],
        len: 0,
    };
}

/// **DECLKD's locator key definitions** (`[[source: dec-std-070]]` 070-13
/// § 5.4; the VT330/VT340's DECLBD, `[[source: vt3xx-locator]]`): the
/// string each of the four keys sends going down and coming up ahead of a
/// ReGIS position report.
///
/// **The defaults** are "No button transition ESC [240 ~", "Ky1 down
/// transition ESC [241 ~" to "Ky4 down transition ESC [247 ~", and "Up
/// Transitions are ignored by default" -- the `CSI` sent as replies send
/// it, seven or eight bits. A power-up and `RIS` restore them; "LKDs are
/// not affected by Soft Terminal Reset (DECSTR)". The null button's string
/// is not one of the four keys a definition names, and stays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocatorKeys {
    /// Down then up, for keys 1 to 4.
    defs: [KeyDef; 8],
    /// Definitions and strings refused (DECLKD's own count).
    refused: u32,
    /// The clear parameter of the DECLKD being read.
    pc: u16,
}

/// The bytes a transition sends: its introducer and up to six more.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LocatorString {
    bytes: [u8; 8],
    len: u8,
}

impl LocatorString {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    fn of(parts: &[&[u8]]) -> LocatorString {
        let mut s = LocatorString {
            bytes: [0; 8],
            len: 0,
        };
        for &b in parts.iter().flat_map(|p| p.iter()) {
            if usize::from(s.len) < s.bytes.len() {
                s.bytes[usize::from(s.len)] = b;
                s.len += 1;
            }
        }
        s
    }
}

impl LocatorKeys {
    /// Power-up: every key its default.
    pub const DEFAULT: LocatorKeys = LocatorKeys {
        defs: [KeyDef::Default; 8],
        refused: 0,
        pc: 0,
    };

    /// `RIS`: the defaults again; the count of refusals is a reading and
    /// survives.
    pub fn reset(&mut self) {
        self.defs = [KeyDef::Default; 8];
    }

    /// How many definitions and strings DECLKD refused.
    pub fn refused(&self) -> u32 {
        self.refused
    }

    fn slot(b: LocatorButton) -> usize {
        match b {
            LocatorButton::Left => 0,
            LocatorButton::Middle => 1,
            LocatorButton::Right => 2,
            LocatorButton::Fourth => 3,
        }
    }

    /// The string `t` sends, with `csi` the introducer replies use.
    pub fn string(&self, t: LocatorTransition, csi: &[u8]) -> LocatorString {
        let (def, code): (KeyDef, &[u8]) = match t {
            LocatorTransition::Null => return LocatorString::of(&[csi, b"240~"]),
            LocatorTransition::Down(b) => {
                let code: &[u8] = match b {
                    LocatorButton::Left => b"241~",
                    LocatorButton::Middle => b"243~",
                    LocatorButton::Right => b"245~",
                    LocatorButton::Fourth => b"247~",
                };
                (self.defs[2 * Self::slot(b)], code)
            }
            LocatorTransition::Up(b) => (self.defs[2 * Self::slot(b) + 1], b""),
        };
        match def {
            KeyDef::Default if code.is_empty() => LocatorString::of(&[]),
            KeyDef::Default => LocatorString::of(&[csi, code]),
            KeyDef::Loaded { bytes, len } => LocatorString::of(&[&bytes[..usize::from(len)]]),
        }
    }

    /// A DECLKD begins: `pc` its clear parameter, its body to follow
    /// ([`LocatorKeys::end`]).
    pub fn begin(&mut self, pc: u16) {
        self.pc = pc;
    }

    /// **A DECLKD**, `DCS Pc $ w Ky1/Std1/Stu1;...;Kyn/Stdn/Stun ST`,
    /// ends: `body` its data with every control already dropped.
    ///
    /// "0-(or omitted) Clear all LKDs before loading new values. All button
    /// definition strings are empty (not the default). 1-Clear old
    /// definition only for keys that are redefined"; any other value is no
    /// DECLKD and the string is refused whole, as DECUDK's `OTHERWISE
    /// VALID_UDK_SYNTAX:= FALSE` (070-11). Each definition is "Key
    /// selection code n, slash delimiter (2/15), and string parameter n for
    /// down and up key transitions. Kyn is a single ASCII digit, and
    /// Std/Stu is a series of ASCII hex pairs. If Std or Stu is omitted,
    /// the corresponding button string will be empty", separated by `;`.
    /// Read as DECUDK's loading reads its definitions (070-11 FILL_KEYS),
    /// all three decided by Jev: a key other than 1 to 4 skips its
    /// definition (SKIP_DEFINITION, 0.96 and 0.94); a hex pair broken or a
    /// character that is no hex digit stops the rest of the string, the
    /// definitions before it kept (STOP, 0.96 and 0.98); a string past six
    /// bytes is not stored -- the key keeps what the clear left it -- and
    /// the rest of the string is ignored (UDK_OVERFLOW, 0.92 and 0.94).
    /// Each refusal is counted.
    pub fn end(&mut self, body: &[u8]) {
        match self.pc {
            0 => self.defs = [KeyDef::EMPTY; 8],
            1 => {}
            _ => {
                self.refused += 1;
                return;
            }
        }
        for def in body.split(|&b| b == b';') {
            if def.is_empty() {
                continue;
            }
            match Self::definition(def) {
                Def::Load(k, down, up) => {
                    self.defs[2 * k] = down;
                    self.defs[2 * k + 1] = up;
                }
                Def::Skip => self.refused += 1,
                Def::Stop => {
                    self.refused += 1;
                    return;
                }
            }
        }
    }

    /// One `Ky/Std/Stu`.
    fn definition(def: &[u8]) -> Def {
        let mut parts = def.splitn(3, |&b| b == b'/');
        let key = parts.next().unwrap_or(&[]);
        let k = match key {
            [d @ b'1'..=b'4'] => usize::from(d - b'1'),
            [b'0'..=b'9'] => return Def::Skip,
            _ => return Def::Stop,
        };
        let (Some(down), Some(up)) = (
            hex_string(parts.next().unwrap_or(&[])),
            hex_string(parts.next().unwrap_or(&[])),
        ) else {
            return Def::Stop;
        };
        Def::Load(k, down, up)
    }
}

/// What one definition does.
enum Def {
    Load(usize, KeyDef, KeyDef),
    Skip,
    Stop,
}

/// A string of hex pairs as its bytes: `None` for a broken pair, a
/// character no hex digit, or more than [`LOCATOR_KEY_MAX`] bytes.
fn hex_string(s: &[u8]) -> Option<KeyDef> {
    if !s.len().is_multiple_of(2) || s.len() > 2 * LOCATOR_KEY_MAX {
        return None;
    }
    let mut bytes = [0u8; LOCATOR_KEY_MAX];
    for (i, pair) in s.chunks(2).enumerate() {
        let hi = hex_digit(pair[0])?;
        let lo = hex_digit(pair[1])?;
        bytes[i] = hi << 4 | lo;
    }
    Some(KeyDef::Loaded {
        bytes,
        len: (s.len() / 2) as u8,
    })
}

/// "3/0 through 3/9 (0 through 9) 4/1 through 4/6 (A through F) 6/1
/// through 6/6 (a through f)".
fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'A'..=b'F' => Some(b - b'A' + 10),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    }
}
