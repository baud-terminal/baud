use crate::*;

/// How mouse events are encoded on the wire: the second of `[[source:
/// ctlseqs]]`' "two sets of mutually exclusive modes", "protocol encoding",
/// the first being the tracking level ([`MouseTracking`]).
///
/// **There is no "off" here.** Tracking is the switch; this says only how a
/// report that tracking let through is written, and X10 is what it is
/// written in when no encoding mode is set.
///
/// **Setting one replaces the one in force, and resetting one undoes only
/// that one** ([`MouseEncoding::after`]). ctlseqs makes them exclusive and
/// says nothing of a reset of one while another is set; xterm decides --
/// "Rather than choose an arbitrary precedence among the coordinate modes,
/// they are mutually exclusive. For consistency, a reset is only effective
/// against the matching mode" (`srm_EXT_MODE_MOUSE` and its three neighbours,
/// `[[source: xterm-reset]]`'s file). Practice, and the only practice pinned.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseEncoding {
    /// The original protocol, and the default: no mode turns it on, it is what
    /// `?1000` gives you without `?1005`, `?1006`, `?1015` or `?1016`.
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
    /// `?1005`. X10's three bytes, each written as UTF-8 once it passes 127:
    /// "This enables UTF-8 encoding for Cx and Cy under all tracking modes,
    /// expanding the maximum encodable position from 223 to 2015 ...
    /// Likewise, Cb will be UTF-8 encoded" (`[[source: ctlseqs]]`). Past
    /// 2015 is refused and counted, as X10 past 223 is.
    Utf8,
    /// `?1006`. No limit worth the name: the position is decimal.
    Sgr,
    /// `?1015`. "This uses the same button encoding as X10, but printing it
    /// as a decimal integer rather than as a single byte" -- X10's `+ 32`
    /// and its release code kept, `CSI Cb ; Cx ; Cy M` with no `<`, and `M`
    /// for a release too. ctlseqs calls it "not recommended; it is not an
    /// improvement over 1006", and herdr and zellij still send it.
    Urxvt,
    /// `?1016`. "Use the same mouse response format as the 1006 control, but
    /// report position in pixels rather than character cells" -- the pixel
    /// position the embedder put on the event ([`PointerPx`]).
    SgrPixels,
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
    /// The largest cell ordinate this encoding can name, one-based.
    ///
    /// X10 writes `ordinate + 32` in one byte, so 223; `?1005` writes it as
    /// UTF-8 in at most two, whose largest is U+07FF, so 2015 -- ctlseqs'
    /// "from 223 to 2015". `None` for the decimal encodings, whose limit is
    /// the grid's, and for `?1016`, which writes no cell at all.
    pub fn max_ordinate(self) -> Option<u16> {
        match self {
            MouseEncoding::X10 => Some(223),
            MouseEncoding::Utf8 => Some(2015),
            MouseEncoding::Sgr | MouseEncoding::Urxvt | MouseEncoding::SgrPixels => None,
        }
    }

    /// The encoding after `change` of the mode that selects `mode`.
    ///
    /// A set selects `mode` whatever was in force; a reset returns to X10
    /// only when `mode` is the one in force, and otherwise changes nothing --
    /// xterm's rule, quoted on the type. So `?1015 h ?1006 l` stays urxvt,
    /// where `?1006 l` used to return to X10 whatever was set, and herdr's
    /// start, which resets all four before setting `?1015` and `?1006`,
    /// resets nothing it had not set.
    pub fn after(self, mode: MouseEncoding, change: ModeChange) -> MouseEncoding {
        match change {
            ModeChange::Set => mode,
            ModeChange::Reset if self == mode => MouseEncoding::X10,
            ModeChange::Reset => self,
        }
    }
}

/// `?1007` -- whether the wheel on the alternate screen sends the cursor
/// keys when no tracking takes it.
///
/// `[[source: ctlseqs]]`, "Wheel mice": "if Alternate Scroll mode is set,
/// then cursor up/down controls are sent when the terminal is displaying the
/// Alternate Screen Buffer. The initial state of Alternate Scroll mode is set
/// using the alternateScroll resource." **On at power-on**, which is what
/// baud did before the mode was read, so nothing changes for a user until an
/// application resets it. xterm's resource defaults to `False`
/// (`Bres(XtNalternateScroll, ..., False)`, `[[source: xterm-reset]]`'s
/// file): the specification leaves the initial state to a resource, and this
/// one is baud's, and differs from xterm's.
///
/// A two-state enum for the reason `baud-domain` is gated at zero `bool`
/// fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AlternateScroll {
    Off,
    #[default]
    On,
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

/// Where the pointer was, in pixels: what `?1016` reports in place of the
/// cell.
///
/// `[[source: ctlseqs]]`, "SGR-Pixels (1016)": "Use the same mouse response
/// format as the 1006 control, but report position in pixels rather than
/// character cells." It says no more -- not where pixel zero is, nor in what
/// unit -- so both are chosen here and said:
///
/// * **Zero-based from the text area's top-left pixel**, the first pixel of
///   the cell at `(1, 1)`, after any padding the embedder draws. xterm takes
///   `event->y - OriginY(screen)`, the pointer less the inner border, and
///   writes it `value + 1` (`EditorButton` and `EmitMousePosition`,
///   `[[source: xterm-button]]`), so the wire is one-based and this is not:
///   the `+ 1` is the encoder's, as it is for a cell.
/// * **In the unit of the cell size the embedder gives the core**
///   (`Term::set_cell_size`), the one `CSI 16 t`, `CSI 14 t` and `?2048`
///   answer in. A position divided by the cell size is then the cell
///   `col` and `row` name, and an application that sized itself from either
///   report reads the pointer in the same pixels.
///
/// [`ImagePx`] is a `u16`, as the cell size and the text area are, and
/// saturates where they do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerPx {
    pub x: ImagePx,
    pub y: ImagePx,
}

/// One mouse event, in cells, and in pixels when the embedder knows them.
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
    /// The same position in pixels, for `?1016`, or `None` when the embedder
    /// does not know it. **`None` is not made up from the cell** -- the
    /// cell's corner times the cell size is a pixel the pointer was probably
    /// not at, and an application that asked for pixels asked because cells
    /// were too coarse -- so under `?1016` an event without it is refused and
    /// counted in `mouse_unencodable`, the way X10 refuses column 224.
    pub px: Option<PointerPx>,
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

    /// The `Cb` field as the X10 family writes it -- X10, `?1005` and
    /// `?1015` -- before the printability 32: **a release names no button.**
    ///
    /// `[[source: ctlseqs]]`: "The low two bits of Cb encode button
    /// information: 0=MB1 pressed, 1=MB2 pressed, 2=MB3 pressed, and
    /// 3=release." [`MouseEvent::cb`] keeps the released button, which is
    /// SGR's rule -- "A different final character is used for button release
    /// to resolve the X10 ambiguity regarding which button was released" --
    /// and the X10 arm used it, so a left release went out byte for byte as
    /// a left press until 2026-10-09. xterm maps the release to `-1`, code 3,
    /// unless the encoding is SGR (`EditorButton`, `[[source: xterm-button]]`).
    /// The modifiers stay.
    pub fn cb_x10(self) -> u16 {
        match self.kind {
            MouseKind::Release => MouseButton::None.code() + self.mods.bits(),
            _ => self.cb(),
        }
    }
}

/// Which mouse events an application asked to hear about: the first of
/// ctlseqs' two exclusive sets, "mouse protocol".
///
/// `[[source: ctlseqs]]`: 9 is X10 compatibility (press only), 1000 is normal
/// tracking (press and release), 1002 adds button-motion, 1003 reports "all
/// motion events, even if no mouse button is down". A level, not four flags:
/// each DECSET selects one and **any** DECRST turns tracking off, as xterm's
/// `really_set_mousemode` stores `enabled ? mode : MOUSE_OFF` for every one
/// (`[[source: xterm-reset]]`'s file).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseTracking {
    #[default]
    Off,
    /// `?9`, X10 compatibility: "sends an escape sequence only on button
    /// press, encoding the location and the mouse button pressed ... Cb is
    /// button-1, where button is 1, 2 or 3" (`[[source: ctlseqs]]`). No
    /// release, no motion, no wheel and no modifiers.
    X10Compat,
    /// `?1000`
    Normal,
    /// `?1002`
    ButtonEvent,
    /// `?1003`
    AnyEvent,
}
