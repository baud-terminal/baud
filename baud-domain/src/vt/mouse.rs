use crate::*;

/// How mouse events are encoded on the wire, if at all.
///
/// `None` is not "mouse is off" -- tracking is a separate mode. It is "no
/// encoding has been negotiated", and it is the state in which baud reports
/// nothing: the only other encoding, X10, caps each ordinate at 223 against a
/// 2048-column grid, so a report from column 400 would name a different cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseEncoding {
    /// The original protocol, and the default: no mode turns it on, it is what
    /// `?1000` gives you without `?1005`, `?1006` or `?1015`.
    ///
    /// `[[source: ctlseqs]]`: "The original X10 mouse protocol limits the Cx
    /// and Cy ordinates to 223 (=255 - 32)." baud's grid goes to 2048, so a
    /// report from column 400 has no encoding here -- and the byte it would
    /// produce names a *different cell* rather than a truncated one, which is
    /// the wrong-shaped answer `docs/vt-coverage.md` rules out. Those events
    /// are dropped and counted in `mouse_unencodable`; the ones that fit are
    /// sent, which is every position on any terminal anyone actually runs.
    #[default]
    X10,
    /// `?1006`. No limit worth the name, and the only encoding baud emits
    /// beyond 223.
    Sgr,
}

/// `?1001` — whether the application asked for highlight tracking.
///
/// A two-state enum for the reason `baud-domain` is gated at zero `bool`
/// fields, and a `Term` field rather than a `ModeBit` because the bitset has
/// one bit left and this does not need to spend it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HighlightTracking {
    #[default]
    Off,
    On,
}

/// The region an application gave in `CSI Ps ; Ps ; Ps ; Ps ; Ps T`.
///
/// `[[source: ctlseqs]]`: "The parameters are func, startx, starty, firstrow,
/// and lastrow" — `func` non-zero initiates and zero aborts.
///
/// Recorded rather than acted on. Highlighting the region needs a selection
/// model and a renderer, neither of which baud has; recording it is what lets
/// an embedder that *does* have them implement the mode. The hazard ctlseqs
/// warns about — "all X events are ignored until the proper escape sequence is
/// received" — is an event loop blocking, and baud has none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HighlightRegion {
    pub func: u16,
    pub start_x: u16,
    pub start_y: u16,
    pub first_row: u16,
    pub last_row: u16,
}

impl MouseEncoding {
    /// The largest ordinate this encoding can name, one-based.
    ///
    /// `None` for SGR: it writes decimal, so the limit is the grid's.
    pub fn max_ordinate(self) -> Option<u16> {
        match self {
            MouseEncoding::X10 => Some(223),
            MouseEncoding::Sgr => None,
        }
    }
}

/// A mouse button, as the wire encodes it.
///
/// `[[source: ctlseqs]]`: "The low two bits of Cb encode button information:
/// 0=MB1 pressed, 1=MB2 pressed, 2=MB3 pressed, and 3=release." Wheel buttons
/// are "represented by the same event codes as buttons 1 and 2 respectively,
/// except that 64 is added to the event code", and buttons 6-7 add 64 while
/// 8-11 add 128.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
    /// No button — the "3=release" code, and the button field of a bare motion
    /// event in any-event tracking.
    None,
}

impl MouseButton {
    /// The button bits, before motion and modifier bits are added.
    fn code(self) -> u16 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::None => 3,
            // "Wheel mice may return buttons 4 and 5 ... except that 64 is
            // added to the event code."
            MouseButton::WheelUp => 64,
            MouseButton::WheelDown => 65,
        }
    }
}

/// Modifier keys held during a mouse event.
///
/// `[[source: ctlseqs]]`: "The next three bits encode the modifiers which were
/// down when the button was pressed and are added together: 4=Shift, 8=Meta,
/// and 16=Control."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MouseMods(pub(crate) u16);

impl MouseMods {
    pub const NONE: MouseMods = MouseMods(0);
    pub const SHIFT: MouseMods = MouseMods(4);
    pub const META: MouseMods = MouseMods(8);
    pub const CONTROL: MouseMods = MouseMods(16);
    pub fn with(self, o: MouseMods) -> MouseMods {
        MouseMods(self.0 | o.0)
    }
    fn bits(self) -> u16 {
        self.0
    }
}

/// What happened, as distinct from which button it happened to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    Press,
    Release,
    /// A drag or a bare move. `[[source: ctlseqs]]`: "On button-motion events,
    /// xterm adds 32 to the event code."
    Motion,
}

/// One mouse event, in cells.
///
/// Coordinates are [`Col`]/[`Row`], not raw integers, so an event cannot be
/// built from a pixel position by accident -- the pixel-to-cell division is the
/// embedder's job and is exactly where an off-by-one hides.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MouseEvent {
    pub button: MouseButton,
    pub mods: MouseMods,
    pub kind: MouseKind,
    pub col: Col,
    pub row: Row,
}

impl MouseEvent {
    /// The `Cb` field: button, plus 32 for motion, plus the modifiers.
    ///
    /// `[[source: ctlseqs]]` for SGR: "The encoded button value in this case
    /// does not add 32 since that was useful only in the X10 scheme" -- that
    /// 32 is the *printability* offset, which is a different 32 from the
    /// motion indicator. The motion 32 stays in both schemes.
    pub fn cb(self) -> u16 {
        let motion = match self.kind {
            MouseKind::Motion => 32,
            _ => 0,
        };
        self.button.code() + motion + self.mods.bits()
    }
}

/// Which mouse events an application asked to hear about.
///
/// `[[source: ctlseqs]]`: 1000 is normal tracking (press and release), 1002
/// adds button-motion, 1003 reports "all motion events, even if no mouse button
/// is down".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseTracking {
    #[default]
    Off,
    /// `?1000`
    Normal,
    /// `?1002`
    ButtonEvent,
    /// `?1003`
    AnyEvent,
}
