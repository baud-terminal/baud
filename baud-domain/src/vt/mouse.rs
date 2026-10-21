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

/// The program's answer to a highlight-tracking press: `CSI Ps ; Ps ; Ps ;
/// Ps ; Ps T`, XTHIMOUSE.
///
/// `[[source: ctlseqs]]`: "The parameters are func, startx, starty, firstrow,
/// and lastrow" -- `func` "is non-zero to initiate highlight tracking and zero
/// to abort", the start is "the starting x and y location for the
/// highlighted region", and "The ending location tracks the mouse, but will
/// never be above row firstrow and will always be above row lastrow. (The top
/// of the screen is row 1.)" The values as they arrived, one-based; what they
/// mean is [`Handshake::answered`]'s.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HighlightRegion {
    pub func: u16,
    pub start_x: u16,
    pub start_y: u16,
    pub first_row: u16,
    pub last_row: u16,
}

/// A cell of the live screen, as highlight tracking names one: the start the
/// program gave, the end that tracks the mouse, and the mouse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellAt {
    pub col: Col,
    pub row: Row,
}

impl CellAt {
    pub fn new(col: Col, row: Row) -> CellAt {
        CellAt { col, row }
    }
}

/// What `?1001` highlights while button 1 is held: "highlights the region
/// covered by the mouse within that range until button release"
/// (`[[source: ctlseqs]]`). From the start the program gave to the end that
/// tracks the mouse, **in that order and not sorted**: the text names a start
/// and an end, not a top and a bottom, and the release report writes them as
/// they are. A drawer sorts them for itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Highlighted {
    pub start: CellAt,
    pub end: CellAt,
}

/// A cell as a highlight report writes it: one-based, column then row.
///
/// **Two `u16`s and not a [`CellAt`]**, whose `usize` coordinates made the
/// report the widest [`Report`] variant -- 64 bytes against 40 -- and the
/// enum is built on the stack wherever a reply is, `csi_final` among them,
/// which saved two more registers on every control sequence to carry it.
/// [`Col::one_based`] and [`Row::one_based`] saturate at `u16::MAX`, past
/// any grid baud provisions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WireCell {
    pub col: OneBased,
    pub row: OneBased,
}

impl WireCell {
    pub fn of(c: CellAt) -> WireCell {
        WireCell {
            col: c.col.one_based(),
            row: c.row.one_based(),
        }
    }

    /// The larger of the two ordinates.
    fn max_ordinate(self) -> u16 {
        self.col.get().max(self.row.get())
    }
}

/// Where the mouse was at button up, as the long release report writes it:
/// its cell, or under `?1016` its pixel.
///
/// `[[source: ctlseqs]]`: "mousex and mousey give the location of the mouse
/// at button up, which may not be over a character" -- a location, where
/// the other four fields are "character positions" of the region -- and
/// SGR-Pixels (1016) has the 1006 format "report position in pixels rather
/// than character cells". So under 1016 these two fields alone are pixels.
/// **Built in baud by [`HighlightEnd::of`] alone**, which picks the variant
/// from the encoding; the variants are public, so a caller building one by
/// hand can pair a pixel with a cell encoding.
/// Narrow, two `u16`s either way: a `usize` here is what took [`Report`] to
/// 64 bytes (see [`WireCell`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseAt {
    /// One-based, column then row.
    Cell(WireCell),
    /// Zero-based as the embedder gave it, and never at `u16::MAX`, whose
    /// one-based form the field cannot carry: the report writes each `+ 1`,
    /// as a `?1016` press writes its pixel.
    Px(PointerPx),
}

impl MouseAt {
    /// The larger of the two values the report writes.
    fn max_ordinate(self) -> u16 {
        match self {
            MouseAt::Cell(c) => c.max_ordinate(),
            MouseAt::Px(p) => p.x.raw().max(p.y.raw()).saturating_add(1),
        }
    }

    /// The two values the report writes, x then y, one-based.
    pub fn wire(self) -> [u16; 2] {
        match self {
            MouseAt::Cell(c) => [c.col.get(), c.row.get()],
            MouseAt::Px(p) => [p.x.raw().saturating_add(1), p.y.raw().saturating_add(1)],
        }
    }
}

/// The report a tracked release sends: `[[source: ctlseqs]]`, "When the
/// button is released, xterm reports the ending position one of two ways".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighlightEnd {
    /// "if the start and end coordinates are the same locations: `CSI t
    /// CxCy`" -- the one location.
    ///
    /// **The plain reading**: the region's start and its end are one cell.
    /// xterm's `EndExtend` (`[[source: xterm-button]]`) tests something else
    /// -- the region's start is the program's start *and* its end is the
    /// mouse's cell, after sorting the two -- and sends nothing at all for a
    /// region of one cell. Which reading the text means is an open question
    /// for Jev (the record, "`?1001` highlight tracking is built to the
    /// text").
    Same(WireCell),
    /// "otherwise: `CSI T CxCyCxCyCxCy`. The parameters are startx, starty,
    /// endx, endy, mousex, and mousey" -- "mousex and mousey give the
    /// location of the mouse at button up, which may not be over a
    /// character" of the region, where the rows the program allowed held the
    /// end back. The mouse is a pixel under `?1016` ([`MouseAt`]).
    Moved {
        start: WireCell,
        end: WireCell,
        mouse: MouseAt,
    },
}

impl HighlightEnd {
    /// The report for a region from `start` to `end`, released with the
    /// mouse over the cell `mouse` and at the pixel `px` when the embedder
    /// knows it, in `encoding`.
    ///
    /// **`None` when the report would write a pixel it does not have**: the
    /// long form under `?1016` with no pixel, or one at `u16::MAX`, refused
    /// as a `?1016` press with none is. The short form writes only the
    /// region's one location, a character position, and needs none.
    pub fn of(
        start: CellAt,
        end: CellAt,
        mouse: CellAt,
        px: Option<PointerPx>,
        encoding: MouseEncoding,
    ) -> Option<HighlightEnd> {
        if start == end {
            return Some(HighlightEnd::Same(WireCell::of(start)));
        }
        let mouse = match (encoding, px) {
            (MouseEncoding::SgrPixels, Some(p)) if p.x.raw() < u16::MAX && p.y.raw() < u16::MAX => {
                MouseAt::Px(p)
            }
            (MouseEncoding::SgrPixels, _) => return None,
            _ => MouseAt::Cell(WireCell::of(mouse)),
        };
        Some(HighlightEnd::Moved {
            start: WireCell::of(start),
            end: WireCell::of(end),
            mouse,
        })
    }

    /// The largest one-based ordinate the report writes, which an encoding
    /// with a ceiling has to be able to name -- the whole report or none of
    /// it, as a mouse report past X10's 223 is refused whole.
    pub fn max_ordinate(self) -> u16 {
        match self {
            HighlightEnd::Same(at) => at.max_ordinate(),
            HighlightEnd::Moved { start, end, mouse } => start
                .max_ordinate()
                .max(end.max_ordinate())
                .max(mouse.max_ordinate()),
        }
    }
}

/// Where `?1001`'s handshake stands, between a press and its release.
///
/// `[[source: ctlseqs]]`: "On button press, the same information as for
/// normal tracking is generated; xterm then waits for the program to send
/// mouse tracking information. All X events are ignored until the proper
/// escape sequence is received from the pty". **Nothing waits here**: the
/// ignoring is xterm's X event loop blocking, and baud has no event loop to
/// block. The state records that a press is owed an answer, and every event
/// that arrives meanwhile is handled as normal tracking handles it -- a
/// release before the answer is a normal release ([`Handshake::released`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Handshake {
    /// No press is waiting and nothing is highlighted.
    #[default]
    Idle,
    /// A button-1 press was reported and its answer has not arrived; the
    /// pointer is last seen at `pointer`, where the end starts if the answer
    /// initiates.
    Awaiting { pointer: CellAt },
    /// The program answered and the end tracks the mouse within the rows it
    /// gave, `first` to `last` inclusive, zero-based.
    Tracking {
        start: CellAt,
        end: CellAt,
        first: Row,
        last: Row,
    },
}

impl Handshake {
    /// A button-1 press was reported at `at`: its answer is owed.
    pub fn pressed(at: CellAt) -> Handshake {
        Handshake::Awaiting { pointer: at }
    }

    /// The pointer moved to `at` with the button held: "The ending location
    /// tracks the mouse", held to the program's rows.
    pub fn moved(self, at: CellAt) -> Handshake {
        match self {
            Handshake::Idle => Handshake::Idle,
            Handshake::Awaiting { .. } => Handshake::Awaiting { pointer: at },
            Handshake::Tracking {
                start, first, last, ..
            } => Handshake::Tracking {
                start,
                end: hold(at, first, last),
                first,
                last,
            },
        }
    }

    /// The program's answer, on a screen of `cols` by `rows`.
    ///
    /// **Only an owed answer is one.** With no press waiting there is nothing
    /// to answer -- the text has the program answer a press -- and the
    /// sequence changes nothing; xterm drops it the same way, behind
    /// `waitingForTrackInfo` (`TrackMouse`, `[[source: xterm-button]]`).
    /// `func` 0 aborts. Otherwise the start is the program's, clamped to the
    /// screen because a cell off it can be neither drawn nor reported, and
    /// the rows are "never above row firstrow" and "always above row
    /// lastrow": one-based `firstrow` to `lastrow - 1`, so zero-based
    /// `firstrow - 1` to `lastrow - 2`, which is xterm's `GetParam(3) - 1,
    /// GetParam(4) - 2` (`CASE_TRACK_MOUSE`, `[[source: xterm-reset]]`'s
    /// file). The end starts where the pointer was last seen, since it
    /// "tracks the mouse".
    ///
    /// **When no row satisfies both** -- `lastrow` at or above `firstrow` --
    /// the text gives the end nowhere to be, and `firstrow` holds it: a
    /// provisional choice, among the questions the record leaves for Jev.
    pub fn answered(self, r: HighlightRegion, cols: Cols, rows: Rows) -> Handshake {
        let Handshake::Awaiting { pointer } = self else {
            return self;
        };
        if r.func == 0 {
            return Handshake::Idle;
        }
        let start = CellAt::new(
            OneBased(r.start_x.max(1)).to_col(cols),
            OneBased(r.start_y.max(1)).to_row(rows),
        );
        let first = OneBased(r.first_row.max(1)).to_row(rows);
        let last = match r.last_row.checked_sub(1) {
            Some(above) if above >= 1 => OneBased(above).to_row(rows),
            _ => first,
        };
        Handshake::Tracking {
            start,
            end: hold(pointer, first, last),
            first,
            last,
        }
    }

    /// Button 1 was released with the mouse over `mouse`: the region it
    /// ends, when the program's answer started tracking, and `Idle` after
    /// either way. A release before the answer, or after an abort, is `None`
    /// -- normal tracking's release, the caller's to send -- and an answer
    /// arriving after it finds nothing owed. [`HighlightEnd::of`] writes the
    /// region's report.
    pub fn released(self, mouse: CellAt) -> (Handshake, Option<Highlighted>) {
        match self {
            Handshake::Tracking {
                start, first, last, ..
            } => (
                Handshake::Idle,
                Some(Highlighted {
                    start,
                    end: hold(mouse, first, last),
                }),
            ),
            _ => (Handshake::Idle, None),
        }
    }

    /// Every cell held on a screen of `cols` by `rows`: a resize under a drag
    /// leaves no cell off the screen to draw or report.
    pub fn within(self, cols: Cols, rows: Rows) -> Handshake {
        let row = |r: Row| Row(r.0.min(rows.raw().saturating_sub(1)));
        let cell =
            |c: CellAt| CellAt::new(Col(c.col.0.min(cols.raw().saturating_sub(1))), row(c.row));
        match self {
            Handshake::Idle => Handshake::Idle,
            Handshake::Awaiting { pointer } => Handshake::Awaiting {
                pointer: cell(pointer),
            },
            Handshake::Tracking {
                start,
                end,
                first,
                last,
            } => Handshake::Tracking {
                start: cell(start),
                end: cell(end),
                first: row(first),
                last: row(last),
            },
        }
    }

    /// What is highlighted now: the region while tracking, else nothing.
    pub fn highlighted(self) -> Option<Highlighted> {
        match self {
            Handshake::Tracking { start, end, .. } => Some(Highlighted { start, end }),
            _ => None,
        }
    }
}

/// `at` with its row held to `first ..= last`; `first` wins when the range is
/// empty, [`Handshake::answered`]'s provisional choice.
fn hold(at: CellAt, first: Row, last: Row) -> CellAt {
    CellAt::new(at.col, Row(at.row.0.min(last.0).max(first.0)))
}

impl MouseEncoding {
    /// The largest cell ordinate this encoding can name, one-based.
    ///
    /// X10 writes `ordinate + 32` in one byte, so 223; `?1005` writes it as
    /// UTF-8 in at most two, whose largest is U+07FF, so 2015 -- ctlseqs'
    /// "from 223 to 2015". `None` for the decimal encodings, whose limit is
    /// the grid's, and for `?1016`, which writes pixels -- and cells only for
    /// `?1001`'s region, in decimal.
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

/// XTSHIFTESCAPE, `CSI > Ps s`: whether Shift takes a pointer event from a
/// program that tracks the mouse (`[[source: ctlseqs]]`, "This corresponds
/// to the shiftEscape resource").
///
/// **Kept by both resets**, as xterm writes `shift_escape` only at start-up
/// and in `CASE_XTERM_SHIFT_ESCAPE` (`[[source: xterm-reset]]`'s file).
/// Read by the embedder's pointer, never here.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ShiftEscape {
    /// `Ps = 0`, and an omitted `Ps`: "allow shift-key to override mouse
    /// protocol". Shift held, every event is the terminal's.
    #[default]
    Override,
    /// `Ps = 1`: "conditionally allow shift-key as modifier in mouse
    /// protocol". xterm's `ShiftOverride` takes the event only for a button
    /// its selection binds without Shift (`[[source: xterm-button-c]]`);
    /// every other one is the program's, its Shift bit sent.
    Conditional,
}

impl ShiftEscape {
    /// `Ps`, or `None` for one the control does not allow: 2 and 3 are
    /// "disallowed in the control sequence". An omitted `Ps` is 0, "If no
    /// parameter is given, xterm uses the default, which is 0", and so is an
    /// explicit one, as the text gives it -- xterm's `one_if_default` reads
    /// an explicit 0 as 1.
    pub fn from_param(p: Option<Param>) -> Option<ShiftEscape> {
        match p.map_or(0, |v| v.value()) {
            0 => Some(ShiftEscape::Override),
            1 => Some(ShiftEscape::Conditional),
            _ => None,
        }
    }
}

/// XTSMPOINTER, `CSI > Ps p`: when the pointer hides as the user types
/// (`[[source: ctlseqs]]`, "Set resource value pointerMode"). The core
/// keeps it and answers whether a key hides the pointer
/// (`Term::hides_pointer_on_key`); the embedder hides it.
///
/// **`RIS` returns it to the embedder's start value** and `DECSTR` keeps
/// it, as xterm's man page lists "Reset title and pointer modes per
/// resources titleModes and pointerMode" under the full reset only
/// (`[[source: xterm-man]]`) and its `ReallyReset` restores
/// `pointer_mode0` (`[[source: xterm-reset]]`'s file).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PointerMode {
    /// `Ps = 0`: "never hide the pointer".
    Never,
    /// `Ps = 1`, and an omitted `Ps`: "hide if the mouse tracking mode is
    /// not enabled". xterm's default, "If no parameter is given, xterm
    /// uses the default, which is 1".
    #[default]
    UnlessTracking,
    /// `Ps = 2`: "always hide the pointer, except when leaving the
    /// window". Shown again when the pointer leaves the window or the
    /// window loses the keyboard, as xterm's `unselectwindow` shows it
    /// for every mode below 3 (`[[source: xterm-misc]]`).
    Always,
    /// `Ps = 3`: "always hide the pointer, even if leaving/entering the
    /// window". Shown again only when the pointer moves, as xterm's
    /// `xevents` shows it only on `MotionNotify` at this mode.
    AlwaysEvenLeaving,
}

impl PointerMode {
    /// `Ps`, or `None` past 3, which ctlseqs does not list among the
    /// "Valid values for the parameter": refused and counted, as
    /// XTSHIFTESCAPE's unlisted values are, where xterm's
    /// `CASE_HIDE_POINTER` stores any value and acts on one past 3 as on
    /// 3 (Jev, IGNORE, 0.96/0.92 after research: its neighbours
    /// `CASE_XTERM_SHIFT_ESCAPE` and `CASE_DECSCUSR` range-check).
    pub fn from_param(p: Option<Param>) -> Option<PointerMode> {
        match p.map_or(1, |v| v.value()) {
            0 => Some(PointerMode::Never),
            1 => Some(PointerMode::UnlessTracking),
            2 => Some(PointerMode::Always),
            3 => Some(PointerMode::AlwaysEvenLeaving),
            _ => None,
        }
    }

    /// Whether a key that sends bytes hides the pointer, with `tracking`
    /// the mouse tracking in force: never at 0, at 1 only while no
    /// tracking is on -- xterm's `send_mouse_pos != MOUSE_OFF`, the DEC
    /// locator among them -- and always at 2 and 3 (`xtermShowPointer`,
    /// `[[source: xterm-misc]]`).
    pub fn hides(self, tracking: MouseTracking) -> PointerHides {
        match (self, tracking) {
            (PointerMode::Never, _) => PointerHides::No,
            (PointerMode::UnlessTracking, MouseTracking::Off) => PointerHides::Yes,
            (PointerMode::UnlessTracking, _) => PointerHides::No,
            (PointerMode::Always | PointerMode::AlwaysEvenLeaving, _) => PointerHides::Yes,
        }
    }

    /// Whether a pointer this mode hid stays hidden when it leaves the
    /// window or the window loses the keyboard: only at 3, as xterm's
    /// `unselectwindow` shows it below 3 (`[[source: xterm-misc]]`).
    pub fn hidden_on_leaving(self) -> PointerHides {
        match self {
            PointerMode::AlwaysEvenLeaving => PointerHides::Yes,
            _ => PointerHides::No,
        }
    }
}

/// Whether the pointer is hidden: what [`PointerMode::hides`] answers for
/// a key, and [`PointerMode::hidden_on_leaving`] for a leave.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerHides {
    Yes,
    No,
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
    /// Button 6, the wheel toward the left: a tilt, a sideways swipe.
    WheelLeft,
    /// Button 7, the wheel toward the right.
    WheelRight,
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
            // "Additional buttons are encoded like the wheel mice, by adding
            // 64 (for buttons 6 and 7)": 6 is 64 + 2 and 7 is 64 + 3, as
            // xterm's `BtnCode` adds 64 to the low two bits.
            MouseButton::WheelLeft => 66,
            MouseButton::WheelRight => 67,
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
    /// Whether every bit of `o` is held.
    pub fn has(self, o: MouseMods) -> bool {
        self.0 & o.0 == o.0
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
    /// counted in `mouse_unencodable`, the way X10 refuses column 224; only
    /// `?1001`'s short release report, which writes no mouse position, goes
    /// out without one ([`HighlightEnd::of`]).
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
/// tracking (press and release), 1001 is normal tracking with highlight
/// tracking's handshake, 1002 adds button-motion, 1003 reports "all
/// motion events, even if no mouse button is down". A level, not five flags:
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
    /// `?1001`, highlight tracking: normal tracking's presses and releases,
    /// with button 1's handshake ([`Handshake`]). `[[source: ctlseqs]]`
    /// lists it among "the remaining five modes of the mouse protocols",
    /// exclusive with the other four, and xterm's `set_mousemode` stores
    /// `VT200_HIGHLIGHT_MOUSE` in the one field that holds them all
    /// (`[[source: xterm-reset]]`'s file). It was a flag beside the level
    /// until 2026-10-11, so `?1001` alone reported nothing.
    Highlight,
    /// `?1002`
    ButtonEvent,
    /// `?1003`
    AnyEvent,
    /// DEC's text locator, enabled by DECELR (`crate::LocatorOp`): "The
    /// mouse protocols include DEC Locator mode, enabled by the DECELR CSI
    /// Ps ; Ps ' z control sequence" (`[[source: ctlseqs]]`), exclusive
    /// with the five above. A DECSET of any of them replaces it and a
    /// DECRST of any turns it off, as for the others; DECELR 0 turns off
    /// whichever is in force, as xterm's `CASE_DECELR` stores `MOUSE_OFF`
    /// (Jev, ALL_OFF, 0.92 and 0.92). Its reports are `Term::locator`'s, so
    /// `Term::mouse_report` sends nothing under it.
    Locator,
}
