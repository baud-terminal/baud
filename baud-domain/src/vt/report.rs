use crate::*;

/// Which sequence a special colour was asked by: `OSC 5`, by its own
/// number, or `OSC 4`, by 256 more.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecialVia {
    Special,
    Palette,
}

/// How a control string ended.
///
/// `[[source: ctlseqs]]`: "XTerm accepts either BEL or ST for terminating OSC
/// sequences, and when returning information, uses the same terminator used in
/// a query." A reply therefore has to remember which arrived -- a `bool` at
/// that call site would read as `if self.osc_st`, which says nothing about what
/// the other branch is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum StringTerm {
    /// `ESC \`, the terminator ECMA-48 documents and xterm prefers.
    #[default]
    St,
    /// `BEL`, accepted "for legacy applications".
    Bel,
}

impl StringTerm {
    /// **The terminator a reply ends with**, the query's: `BEL` as `BEL`,
    /// and `ST` as `ESC \` or, after S8C1T, 9C -- xterm's `unparseputc1`
    /// sends the final it was given, in C1 when it is one
    /// (`[[source: xterm-sgr]]` charproc.c 9356-9365).
    pub fn bytes(self, c1: C1Transmit) -> &'static [u8] {
        match self {
            StringTerm::St => c1.st(),
            StringTerm::Bel => b"\x07",
        }
    }
}

/// Which control string a reply is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StringIntro {
    /// `OSC`: a colour, a font, an allow list, iTerm2's and kitty's answers.
    Osc,
    /// `DCS`: XTVERSION, DECRQSS, the presentation state reports.
    Dcs,
    /// `APC`: kitty's graphics replies.
    Apc,
}

/// **A string reply's frame**: its introducer, and the terminator of the
/// query it answers, each spelled in the C1 form the terminal sends in,
/// which the writer passes as it writes: the frame carries no mode of its
/// own, so no caller can pick one. The one way a reply's `OSC`, `DCS`,
/// `APC` and `ST` are spelled, so a reply after S8C1T opens with 9D, 90 or
/// 9F and closes with 9C, as xterm's do (`ReportAnsiColorRequest`,
/// `[[source: xterm-misc]]` 2636-2655), and a `BEL` query is answered with
/// `BEL`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StringReply {
    pub intro: StringIntro,
    pub term: StringTerm,
}

impl StringReply {
    /// An `OSC` reply to a query that ended with `term`.
    pub fn osc(term: StringTerm) -> StringReply {
        StringReply {
            intro: StringIntro::Osc,
            term,
        }
    }

    /// A `DCS` reply, ended by `ST`.
    pub fn dcs() -> StringReply {
        StringReply {
            intro: StringIntro::Dcs,
            term: StringTerm::St,
        }
    }

    /// An `APC` reply, ended by `ST`.
    pub fn apc() -> StringReply {
        StringReply {
            intro: StringIntro::Apc,
            term: StringTerm::St,
        }
    }

    /// The introducer, in the C1 form `c1` names.
    pub fn open(self, c1: C1Transmit) -> &'static [u8] {
        match self.intro {
            StringIntro::Osc => c1.osc(),
            StringIntro::Dcs => c1.dcs(),
            StringIntro::Apc => c1.apc(),
        }
    }

    /// The terminator, in the C1 form `c1` names.
    pub fn close(self, c1: C1Transmit) -> &'static [u8] {
        self.term.bytes(c1)
    }
}

/// **Whether a read stops so its answers can go out**
/// (`Term::set_reply_hold`): once the replies a read has queued leave less
/// room than the longest answer one control can owe, the read stops after
/// that control, and the embedder sends what is owed and feeds the rest.
/// **Off by default**: an embedder that never feeds the rest of a read is
/// fed every byte as before, and a reply that does not fit is dropped and
/// counted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ReplyHold {
    #[default]
    Off,
    On,
}

/// Whether the terminal's window gained or lost keyboard focus. The two
/// states have names so a call site cannot pass a `bool` that reads as
/// nothing in particular; the platform layers (`macos-liquid-glass`'s
/// `Focus`, Wayland's `wl_keyboard.enter`/`leave`) each map onto it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    Gained,
    Lost,
}

/// What a terminal sends *back*, named by the report rather than by its bytes.
///
/// The mirror of [`Csi`]. Parsing has had a type since the beginning; replying
/// was three byte literals and a hand-rolled decimal formatter at the call
/// site, which is the same "two quantities confused" shape the newtypes exist
/// to prevent -- one level up, at the protocol.
///
/// `docs/vt-coverage.md` makes one rule non-negotiable: **never answer a query
/// with the wrong shape**, because an application parses whatever arrives as
/// the report it asked for. A byte literal cannot enforce that; a variant can.
/// The final byte, the introducer and the parameter layout are picked by the
/// variant, so a DSR cannot be answered in a DA's shape, and there is no
/// spelling of "reply" that does not name the report being sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Report {
    /// `CSI I` or `CSI O`: the terminal gained or lost focus, sent only while
    /// `?1004` is set. `[[source: ctlseqs]]`, "FocusIn/FocusOut": "it causes
    /// xterm to send CSI I when the terminal gains focus, and CSI O when it
    /// loses it".
    Focus(Focus),
    /// CPR — Cursor Position Report, `CSI r ; c R`. `[[source: ctlseqs]]`:
    /// "Report Cursor Position (CPR) [row;column]. Result is CSI r ; c R".
    ///
    /// 1-based, like CUP, so the report can be fed straight back as one.
    CursorPosition { row: OneBased, col: OneBased },
    /// DSR operating status, `CSI 0 n`. `[[source: ctlseqs]]`: "Status Report.
    /// Result ("OK") is CSI 0 n".
    OperatingStatus,
    /// `CSI ? 997 ; 1 n` (dark) or `CSI ? 997 ; 2 n` (light): the answer to
    /// `CSI ? 996 n`, and the unsolicited `?2031` notification.
    /// `[[source: contour-palette-notifications]]` gives exactly these two and
    /// nothing else; tmux rejected a `997 ; 0 n`, and neovim needs exactly two
    /// parameters.
    ColorScheme(ColorScheme),
    /// Primary DA, `CSI ? Ps c`: one of the two answers baud gives, chosen
    /// by the [`SixelClaim`]. Under the claim, made when the embedder has
    /// provisioned image memory, the answer carries attribute `4`, so an
    /// application learns baud will draw a Sixel and uses it rather than a
    /// text fallback.
    PrimaryDeviceAttributes(DeviceAttribute),
    /// The answer to VT52's `ESC Z`: `ESC / Z`.
    ///
    /// `[[source: vt510-decanm]]` Table 5-1: "ESC Z Identify (host to
    /// terminal)" and "ESC /Z Report (terminal to host)". Not a CSI, and not a
    /// DCS either -- a third shape, which is why replies are a typed `Report`
    /// rather than byte literals at each call site.
    Vt52Identity,
    /// `[[source: kitty-keyboard]]`'s answer to the flags query `CSI ? u`:
    /// `CSI ? flags u`, the currently enabled progressive-enhancement bits.
    ///
    /// **Answering is the support claim.** The protocol detects support by
    /// whether this reply arrives at all, so it is emitted only now that every
    /// enhancement is honoured -- before that the query was consumed and
    /// counted, unanswered.
    KittyFlags(KittyFlags),
    /// The in-band resize notification of `?2048`:
    /// `CSI 48 ; rows ; cols ; height_px ; width_px t`.
    ///
    /// **Character height and width come first, then pixels**, which is
    /// the order the specification gives and the reverse of the order a
    /// `WinSize` is usually read in -- so the two are destructured here
    /// and nowhere else, the same rule `WinSize::geometry` carries.
    ///
    /// **The pixel fields are the text area**, and zero until the embedder
    /// has told the core its cell size. `[[source: inband-resize]]`: "If a
    /// terminal is not capable of reporting pixel sizes, it must report them
    /// as 0. A terminal MUST report pixel sizes if it is capable of reporting
    /// them", and "The reported size MUST be the text area size. Text area
    /// does not include any padding" -- so it is `rows` cells by `cols`
    /// cells of the cell size, the same numbers `CSI 14 t` answers with, and
    /// not whatever the window measures.
    InbandResize {
        rows: Rows,
        cols: Cols,
        height: ImagePx,
        width: ImagePx,
    },
    /// `CSI 4 ; height ; width t`, the answer to `CSI 14 t`: the text area in
    /// pixels. `[[source: ctlseqs]]`, XTWINOPS `Ps = 1 4`.
    TextAreaPixels { height: ImagePx, width: ImagePx },
    /// `CSI 6 ; height ; width t`, the answer to `CSI 16 t`: one cell in
    /// pixels. `[[source: ctlseqs]]`, XTWINOPS `Ps = 1 6`. This is the reply
    /// an image client sizes its pictures from -- `[[source: yazi-emulator]]`
    /// reads it out of its DA1 burst.
    CellPixels { height: ImagePx, width: ImagePx },
    /// `CSI 8 ; rows ; cols t`, the answer to `CSI 18 t`: the text area in
    /// cells. `[[source: ctlseqs]]`, XTWINOPS `Ps = 1 8`.
    TextAreaChars { rows: Rows, cols: Cols },
    /// `CSI 1 t` or `CSI 2 t`, the answer to `CSI 11 t`: open or iconified.
    WindowState(WindowState),
    /// `CSI 3 ; x ; y t`, the answer to `CSI 13 t` and `CSI 13 ; 2 t`: a
    /// position on the screen, **x first**, unlike every size XTWINOPS
    /// reports, and each coordinate in [`ScreenCoord::wire`]'s range.
    Position(ScreenPoint),
    /// `CSI 4 ; height ; width t`, the answer to `CSI 14 ; 2 t`: the window,
    /// frame included, in pixels -- the shape `CSI 14 t` answers in.
    WindowPixels(PixelSize),
    /// `CSI 5 ; height ; width t`, the answer to `CSI 15 t`: the screen in
    /// pixels.
    ScreenPixels(PixelSize),
    /// `CSI 9 ; height ; width t`, the answer to `CSI 19 t`: the screen in
    /// characters, its pixels divided by the cell's.
    ScreenChars { rows: u16, cols: u16 },
    /// XTVERSION, the answer to `CSI > 0 q`.
    ///
    /// `[[source: ctlseqs]]`: "Report xterm name and version (XTVERSION). The
    /// response is a DSR sequence identifying the version: `DCS > | text ST`".
    ///
    /// The only report here that is not a CSI, which is why the encoder has a
    /// DCS branch at all. Answering it lets an application feature-detect
    /// instead of guessing from `$TERM` -- and guessing from `$TERM` is how a
    /// terminal ends up being sent sequences it does not implement.
    Version,
    /// DECCKSR, `DCS Pid ! ~ D...D ST`: the answer to DECRQCRA.
    ///
    /// `[[source: dec-std-070]]` p. 5-181: "The numeric parameter of the
    /// DECCKSR control string is a numeric label for the checksum report
    /// specified in the DECRQCRA ... The data of the control string consists
    /// of four hexadecimal digits (3/0 through 3/9 and 4/1 through 4/6)", so
    /// the letters are capitals. The introducer and terminator follow
    /// `S7C1T`/`S8C1T`, as DECRQSS's do.
    AreaChecksum { id: Param, sum: Checksum },
    /// DECXCPR, `CSI ? Pl ; Pc ; Pp R`: the cursor and its page, which is
    /// always the first. `[[source: dec-std-070]]` p. 5-55's report format,
    /// with the `?` that the standard and `[[source: ctlseqs]]` give and
    /// `[[source: vt420-rm]]` and `[[source: vt510-dsr-xcpr]]` print
    /// without, STD 070's form taken.
    ExtendedCursorPosition { row: OneBased, col: OneBased },
    /// A DEC private status, `CSI ? Ps n` ([`DecReply`]).
    DecStatus(DecReply),
    /// DECLRP, DEC's locator report ([`LocatorReport`]): `CSI Pe ; Pb ; Pr
    /// ; Pc & w` (`[[source: dec-std-070]]` 070-13 p. 6, PDF 1021).
    Locator(LocatorReport),
    /// DECMSR, `CSI 0 * {`: "the number of bytes available for macro
    /// definitions divided by 16" (`[[source: dec-std-070]]` p. 5-185), and
    /// baud has no macro memory.
    MacroSpace,
    /// DECREPTPARM, `CSI sol ; 1 ; 1 ; 120 ; 120 ; 1 ; 0 x`
    /// (`[[source: vt100-ug-ch3]]`): no parity, eight bits, 19200 both ways
    /// -- the highest speed the table defines, for a pty that has no line
    /// speed (xterm sends 128, a code the table does not have) -- the clock
    /// multiplier 1 and no STP flags.
    TerminalParameters { sol: u16 },
    /// DECAUPSS, `DCS 0 ! u % 5 ST`, the answer to DECRQUPSS: the
    /// user-preference supplemental set is DEC Supplemental
    /// (`[[source: dec-std-070]]` p. 5-125).
    PreferredSupplemental,
    /// DECRPDE, `CSI Ph ; Pw ; 1 ; 1 ; 1 " w`: every line and column of the
    /// one page, shown from its first cell (`[[source: dec-std-070]]`
    /// p. 5-88).
    DisplayedExtent { rows: Rows, cols: Cols },
    /// The answer to XTQMODKEYS or XTQFMTKEYS: `CSI > Pp ; Pv m` or `f`, the
    /// control that sets the resource, so "the response can be used to
    /// restore this state" (`[[source: ctlseqs]]`; `report_mod_fkeys` and
    /// `report_fmt_fkeys`, `[[source: xterm-reset]]`'s file). `value` is as
    /// xterm writes it, an unsigned short, so a disabled resource and a `Pp`
    /// naming none are 65535, xterm's `DEFAULT`.
    KeyOption {
        which: KeyOption,
        pp: u16,
        value: u16,
    },
    /// `OSC 4 ; c ; rgb:RRRR/GGGG/BBBB ST` -- the answer to `OSC 4 ; c ; ?`,
    /// four digits a channel for the reason [`ReportBytes::push_hex`] gives.
    ///
    /// `[[source: ctlseqs]]`: xterm "replies with a control sequence of the
    /// same form which can be used to set the corresponding color", and it
    /// "uses the same terminator used in a query" -- which is why the
    /// terminator is a field. A query sent with `BEL` answered with `ST` is a
    /// differently-shaped answer to the one that was asked for, and some
    /// readers stop at the terminator they expect.
    Color {
        index: Chan,
        r: u8,
        g: u8,
        b: u8,
        term: StringTerm,
    },
    /// `OSC 5 ; c ; rgb:RRRR/GGGG/BBBB ST`, or `OSC 4 ; 256+c ; ...` when
    /// the query came by `OSC 4`: a special colour, in the shape it was
    /// asked in, as xterm's `ReportAnsiColorRequest` writes "(opcode == 5) ?
    /// (colornum - NUM_ANSI_COLORS) : colornum" (`[[source: xterm-misc]]`
    /// 2647-2652).
    SpecialColor {
        which: SpecialColor,
        via: SpecialVia,
        r: u8,
        g: u8,
        b: u8,
        term: StringTerm,
    },
    /// `OSC 10 ; rgb:RRRR/GGGG/BBBB ST` or `OSC 11 ; ...`: the answer to a
    /// dynamic-colour query, in the setter's own shape so it can be fed back.
    DynamicColor {
        which: DynamicColor,
        r: u8,
        g: u8,
        b: u8,
        term: StringTerm,
    },
    /// DECRPM, the answer to DECRQM: `CSI Ps ; Pm $ y`, or
    /// `CSI ? Ps ; Pm $ y` for the private namespace.
    ///
    /// The marker is carried because the two namespaces are unrelated: `CSI 4`
    /// is IRM and `CSI ? 4` is DECSCLM. Answering a private request without the
    /// `?` would have the application read it as an answer about a different
    /// mode entirely.
    ModeStatus {
        marker: Marker,
        mode: Param,
        state: ModeState,
    },
    /// An SGR-encoded mouse report, `CSI < Cb ; Px ; Py M` (press or motion) or
    /// `... m` (release).
    ///
    /// `[[source: ctlseqs]]`, "SGR (1006)": "CSI < followed by
    /// semicolon-separated encoded button value, Px and Py ordinates and a
    /// final character which is M for button press and m for button release."
    MouseSgr(MouseEvent),
    /// `ESC [ M Cb Cx Cy`, each byte offset by 32, a release written as code
    /// 3. See [`MouseEncoding::X10`] for why the caller must check the range
    /// first: past 223 the byte wraps and names a different cell.
    MouseX10(MouseEvent),
    /// `?1005`: X10's three values, each written as UTF-8 once it passes 127.
    /// The caller refuses an ordinate past 2015, whose `+ 32` has no two-byte
    /// form.
    MouseUtf8(MouseEvent),
    /// `?1015`: `CSI Cb ; Cx ; Cy M`, X10's button value -- its `+ 32` and
    /// its release code -- and the position in decimal, `M` for a release
    /// too.
    MouseUrxvt(MouseEvent),
    /// `?1016`: [`Report::MouseSgr`]'s shape with the position in pixels,
    /// one-based. The pixel is resolved by the caller -- an event that
    /// carries none is refused before it gets here, so this variant cannot
    /// be built without one.
    MouseSgrPixels(MouseEvent, PointerPx),
    /// `?1001`'s release report, in the encoding in force.
    /// `[[source: ctlseqs]]`: `CSI t CxCy` when the start and the end are
    /// the same location, else `CSI T CxCyCxCyCxCy` -- each position "as
    /// value+32" in one byte, which `?1005` writes as UTF-8 "under all
    /// tracking modes". Under `?1006` "The highlight tracking responses are
    /// also modified to an SGR-like format, using the same SGR-style
    /// scheme": `CSI <`, decimal positions separated by `;`, and the same
    /// final byte -- the text names the scheme and not the bytes, and these
    /// are xterm's (`EndExtend`, `[[source: xterm-button]]`). `?1015` is
    /// decimal with no `<`, the form ctlseqs warns "can be mistaken for SD".
    /// `?1016` writes the region's four fields in cells and the mouse's two
    /// in pixels: the highlight paragraph calls startx, starty, endx and
    /// endy "character positions" and mousex, mousey "the location of the
    /// mouse at button up, which may not be over a character", and the 1016
    /// paragraph reports "position in pixels rather than character cells" --
    /// the one reading that satisfies both ([`MouseAt`]; xterm writes all
    /// six in cells). The caller refuses an ordinate the encoding cannot
    /// name.
    Highlight(HighlightEnd, MouseEncoding),
    /// XTSMGRAPHICS's answer, `CSI ? Pi ; Ps ; Pv S` (`[[source:
    /// ctlseqs]]`): the item asked about, then a status and, on success, the
    /// value -- one for the colour registers, `width ; height` for a
    /// geometry. Built by [`Report::graphics`].
    Graphics { item: Param, reply: GraphicsReply },
    /// XTTITLEPOS's answer, `CSI Pn ; Pm # S` (`[[source: ctlseqs]]` 664):
    /// the pushes not yet popped and the stack's depth, as xterm's
    /// `xtermReportTitleStack` writes `used` and `MAX_SAVED_TITLES`
    /// (`[[source: xterm-misc]]`), so `index` passes `max` once a push
    /// past the tenth has overwritten the oldest.
    TitlePosition { index: u16, max: u16 },
    /// XTREPORTCOLORS's answer, `CSI ? Pu ; Ps # Q`: the palette stack's
    /// current entry and the palettes stored on it, "using the same form as
    /// XTPOPCOLORS" (`[[source: ctlseqs]]` 658-660) with the `?` xterm's
    /// `xtermReportColors` writes (`[[source: xterm-misc]]`), which keeps
    /// the answer from popping two palettes if it is echoed back.
    ColorStack { current: u16, stored: u16 },
}

/// What an XTSMGRAPHICS answer carries after its item: the value read,
/// with status 0, or the failing status alone, as xterm writes no value
/// unless the status is 0 (`CASE_GRAPHICS_ATTRIBUTES`, `[[source:
/// xterm-reset]]`'s file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraphicsReply {
    /// Status 0 and one value: a count of colour registers.
    Count(u16),
    /// Status 0 and `width ; height`, in pixels.
    Geometry { width: ImagePx, height: ImagePx },
    /// "Ps = 1 <- error in Pi."
    ErrorInItem,
    /// "Ps = 2 <- error in Pa."
    ErrorInAction,
    /// "Ps = 3 <- failure."
    Failure,
}

impl GraphicsReply {
    fn status(self) -> u16 {
        match self {
            GraphicsReply::Count(_) | GraphicsReply::Geometry { .. } => 0,
            GraphicsReply::ErrorInItem => 1,
            GraphicsReply::ErrorInAction => 2,
            GraphicsReply::Failure => 3,
        }
    }
}

// **`Report` is 40 bytes, and a wider variant is a cost on every control
// sequence**: the enum is built on the stack wherever a reply is, `csi_final`
// among them, and `?1001`'s first report, at 64 bytes with `usize` cells,
// put two more register saves into `csi_final`'s prologue. `WireCell` and
// `MouseAt` keep the highlight report to `u16`s, and this pins the size, so
// a variant that widens it fails here rather than in a disassembly.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<Report>() == 40);

// `ESC P > |`, the text and `ESC \` fit, or a longer version would lose the
// `ST` to `ReportBytes::push`'s silent drop and send a reply of the wrong
// shape. Checked at compile time because the text is the build's.
const _: () = assert!(VersionText::PACKAGE.reply_len() <= Report::MAX_LEN);

/// **The version XTVERSION names, as the embedder tells it**: the program's
/// own, build metadata included, between the parentheses of
/// `DCS > | baud(<version>) ST`. Until an embedder tells one it is
/// [`VersionText::PACKAGE`], the workspace version: the core has no git and
/// no build to ask, so what a build is called is the embedder's to say, as
/// the `TERM` XTGETTCAP names is (`TermName`).
///
/// The bound is xterm's: `sprintf("%.80s(%d)", ...)`
/// (`[[source: xterm-version]]`, line 72) keeps 80 bytes of its text. Each
/// byte is `21-7E`, so nothing in a text can end the control
/// string early (`[[source: williams]]`: ESC cancels one).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VersionText {
    version: &'static str,
}

impl VersionText {
    /// The longest text told, in bytes.
    pub const MAX: usize = 80;

    /// The workspace version, every crate's: what an embedder that tells
    /// nothing answers with.
    pub const PACKAGE: VersionText = VersionText {
        version: env!("CARGO_PKG_VERSION"),
    };

    /// `version`, if it is 1 to [`VersionText::MAX`] bytes, each `21-7E`.
    /// A `const fn` so a build's own text can be refused at compile time.
    pub const fn new(version: &'static str) -> Option<VersionText> {
        let b = version.as_bytes();
        if b.is_empty() || b.len() > VersionText::MAX {
            return None;
        }
        let mut i = 0;
        while i < b.len() {
            if b[i] < 0x21 || b[i] > 0x7e {
                return None;
            }
            i += 1;
        }
        Some(VersionText { version })
    }

    /// The text told.
    pub const fn as_str(self) -> &'static str {
        self.version
    }

    /// The reply's bytes in order: `DCS > | baud(`, the text and `) ST`,
    /// `DCS` and `ST` in the C1 form `c1` sends, as xterm's
    /// `CASE_REPORT_VERSION` writes them with `unparseputc1`
    /// (`[[source: xterm-sgr]]` charproc.c 5461-5470).
    pub const fn reply_parts(self, c1: C1Transmit) -> [&'static [u8]; 3] {
        match c1 {
            C1Transmit::SevenBit => [b"\x1bP>|baud(", self.version.as_bytes(), b")\x1b\\"],
            C1Transmit::EightBit => [b"\x90>|baud(", self.version.as_bytes(), b")\x9c"],
        }
    }

    /// How long the reply is, in bytes, in 7-bit: the longer of its two
    /// forms, so the room it is checked against holds either.
    pub const fn reply_len(self) -> usize {
        let [a, b, c] = self.reply_parts(C1Transmit::SevenBit);
        a.len() + b.len() + c.len()
    }
}

/// Whether the Primary DA advertises Sixel graphics (attribute `4`). A typed
/// flag rather than a `bool` so the call site reads as what it is, and so the
/// audit sees a name; `Yes` only when the image store is provisioned, since an
/// unprovisioned baud refuses every Sixel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelClaim {
    No,
    Yes,
}

/// Whether DECRQCRA, `CSI Pi ; Pp ; Pt ; Pl ; Pb ; Pr * y`, is answered
/// with a checksum report. The embedder's choice: `On` by default, and
/// `Off` makes every request go unanswered, as xterm does with
/// `GetChecksum` among its disallowed window operations (`CASE_DECRQCRA`,
/// `[[source: xterm-reset]]`'s file; its default list,
/// `[[source: xterm-man]]`). A checksum of one cell at a time reads the
/// screen back, other programs' output included, which is why a setting
/// turns it off.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum ChecksumReport {
    #[default]
    On,
    Off,
}

/// Which terminal baud claims to be, in a Primary DA response: the whole
/// answer, so only the two baud sends can be built.
///
/// `[[source: ctlseqs]]` lists the identities under `CSI Ps c`, and picking
/// one is a promise about *everything* at that level -- over-claiming makes
/// applications send sequences baud drops, which is the `parsed` failure by
/// another route. Widening the claim should cost a deliberate edit here, next
/// to the reason, rather than a changed digit in a byte string.
///
/// **Both answers are below what baud implements, a departure.**
/// `[[source: dec-std-070]]` 070-4 DA (PDF 221-222) has the first parameter
/// name "the highest level that the device can support", and baud has VT102
/// editing, selective erase and more. The default answers a VT100 because
/// `kitten icat`'s detection ends only on a DA whose CSI, final byte
/// included, is longer than three bytes (`[[source: kitty-icat-detect]]`,
/// `detect.go:93`), and VT102's `?6c` is three: kitten waited out its ten
/// seconds and drew nothing. `docs/vt-coverage.md` lists it under its open
/// departures.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceAttribute {
    /// `CSI ? 1 ; 2 c`, "VT100 with Advanced Video Option"
    /// (`[[source: ctlseqs]]`), the answer without the Sixel claim and what
    /// xterm sends with `decTerminalID` 100. Under-claiming only makes an
    /// application use less, and ctlseqs: "The VT100-style response
    /// parameters do not mean anything by themselves".
    Vt100Avo,
    /// `CSI ? 6 ; 4 c`, VT102 with attribute `4`, Sixel graphics
    /// (`[[source: ctlseqs]]`), the answer under the Sixel claim. An
    /// unprovisioned baud refuses every Sixel and never makes the claim, so
    /// an application that reads `4` and sends images sees them drawn.
    Vt102Sixel,
}

impl DeviceAttribute {
    /// The two `Ps` in `CSI ? Ps ; Ps c`.
    fn params(self) -> [u16; 2] {
        match self {
            DeviceAttribute::Vt100Avo => [1, 2],
            DeviceAttribute::Vt102Sixel => [6, 4],
        }
    }
}

/// A rendered [`Report`], owned and fixed-size.
///
/// Returned by value rather than written through a `&mut` into the reply
/// buffer: the core is `no_std` with no allocator, and this keeps the encoding
/// testable without a `Term` to hang it on.
pub struct ReportBytes {
    buf: [u8; Report::MAX_LEN],
    len: ByteLen,
}

impl ReportBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len.index()]
    }
}

/// Whether the terminal **sends** C1 controls in their 7-bit or 8-bit form.
///
/// `[[source: ctlseqs]]`, on `ESC SP G`: "8-bit controls (S8C1T), VT220. This
/// tells the terminal to send C1 control characters as 8-bit sequences, e.g.,
/// its responses to queries."
///
/// **This is a different axis from what baud accepts, and conflating the two is
/// a mistake worth naming.** baud decodes UTF-8, so it cannot *parse* an 8-bit
/// C1 introducer — `9B` is a continuation byte there and nothing else. But a
/// reply is not UTF-8 text; it is a control stream the application explicitly
/// asked for by sending S8C1T. Refusing to send what was asked for on the
/// grounds that baud could not read it back is a category error: the two
/// directions are independent, and `[[source: ctlseqs]]` describes S7C1T/S8C1T
/// purely in terms of what the terminal sends.
///
/// An enum rather than a `bool`, because a `bool` field is always a missing
/// two-state enum — and `c1: true` does not say which way round it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum C1Transmit {
    /// `ESC [`, `ESC P`, `ESC \`. The default, and xterm's.
    #[default]
    SevenBit,
    /// `9B`, `90`, `9C`.
    EightBit,
}

impl C1Transmit {
    /// CSI: the introducer for a control sequence reply.
    pub fn csi(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1b[",
            C1Transmit::EightBit => b"\x9b",
        }
    }
    /// OSC: the introducer for an operating system command reply, 9D in C1
    /// as xterm's `unparseputc1(xw, ANSI_OSC)` sends it.
    pub fn osc(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1b]",
            C1Transmit::EightBit => b"\x9d",
        }
    }
    /// DCS: the introducer for a device control string reply.
    pub fn dcs(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1bP",
            C1Transmit::EightBit => b"\x90",
        }
    }
    /// APC: the introducer for an application program command reply, 9F
    /// in C1, as S8C1T governs every C1 control the terminal sends.
    pub fn apc(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1b_",
            C1Transmit::EightBit => b"\x9f",
        }
    }
    /// SS3: the single-shift the cursor and keypad keys use in application
    /// mode.
    ///
    /// `[[source: ctlseqs]]` line 2473 gives `CSI A` for a cursor key in normal
    /// mode and `SS3 A` in application mode -- so an encoder needs both
    /// introducers, and which one it picks is DECCKM's business rather than
    /// this type's.
    pub fn ss3(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1bO",
            C1Transmit::EightBit => b"\x8f",
        }
    }
    /// ST: the string terminator.
    pub fn st(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1b\\",
            C1Transmit::EightBit => b"\x9c",
        }
    }
}

impl Report {
    /// The `?2048` report for a text area of `rows` by `cols` cells, each
    /// `cell_w` by `cell_h` device pixels.
    ///
    /// The pixel fields are derived here and nowhere else, so the report
    /// sent on enablement, after a resize and after a cell-size change
    /// cannot disagree about what "the text area" is. **Both zero unless
    /// both dimensions are known**: `[[source: inband-resize]]` gives zero as
    /// the answer of a terminal that cannot report pixels, and one known
    /// dimension beside one unknown is not a size.
    pub fn inband(rows: Rows, cols: Cols, cell_w: ImagePx, cell_h: ImagePx) -> Report {
        let known = cell_w != ImagePx::ZERO && cell_h != ImagePx::ZERO;
        let (height, width) = match known {
            true => (cell_h.span(rows.index()), cell_w.span(cols.index())),
            false => (ImagePx::ZERO, ImagePx::ZERO),
        };
        Report::InbandResize {
            rows,
            cols,
            height,
            width,
        }
    }

    /// The answer to an XTWINOPS report, or `None` when there is none to
    /// give: an operation that is not a report, or a report whose numbers
    /// nobody has handed over.
    ///
    /// **A pixel answer needs its pixels, and without them there is no
    /// answer at all** rather than `CSI 6 ; 0 ; 0 t`. Mode 2048's
    /// specification defines zero as "not capable"; XTWINOPS defines no such
    /// value, and a client that divides by the cell size is better served by
    /// the silence it already has to handle than by a zero it may not. The
    /// text area in cells is always known; the window and the screen are
    /// known when the embedder says (`facts`), and the screen in characters
    /// when both the screen and the cell are -- xterm divides the screen's
    /// pixels by the font's (`ewGetScreenSizeChars`, `[[source:
    /// xterm-reset]]`'s file).
    pub fn window(
        op: WindowOp,
        rows: Rows,
        cols: Cols,
        cell_w: ImagePx,
        cell_h: ImagePx,
        facts: &WindowFacts,
    ) -> Option<Report> {
        let cell = match cell_w != ImagePx::ZERO && cell_h != ImagePx::ZERO {
            true => Some((cell_w, cell_h)),
            false => None,
        };
        match op {
            WindowOp::TextAreaChars => Some(Report::TextAreaChars { rows, cols }),
            WindowOp::TextAreaPixels => cell.map(|(w, h)| Report::TextAreaPixels {
                height: h.span(rows.index()),
                width: w.span(cols.index()),
            }),
            WindowOp::CellPixels => cell.map(|(w, h)| Report::CellPixels {
                height: h,
                width: w,
            }),
            WindowOp::WindowState => facts.state.map(Report::WindowState),
            WindowOp::WindowPosition => facts.position.map(Report::Position),
            WindowOp::TextAreaPosition => facts.text_area.map(Report::Position),
            WindowOp::WindowPixels => facts.window.map(Report::WindowPixels),
            WindowOp::ScreenPixels => facts.screen.map(Report::ScreenPixels),
            WindowOp::ScreenChars => match (facts.screen, cell) {
                (Some(s), Some((w, h))) => Some(Report::ScreenChars {
                    rows: s.height.raw() / h.raw(),
                    cols: s.width.raw() / w.raw(),
                }),
                _ => None,
            },
            WindowOp::TitleReport
            | WindowOp::PushTitle
            | WindowOp::PopTitle
            | WindowOp::Deiconify
            | WindowOp::Iconify
            | WindowOp::Move
            | WindowOp::ResizePixels
            | WindowOp::Raise
            | WindowOp::Lower
            | WindowOp::Refresh
            | WindowOp::ResizeCells
            | WindowOp::Maximize
            | WindowOp::FullScreen
            | WindowOp::Lines
            | WindowOp::Refused => None,
        }
    }

    /// XTSMGRAPHICS's answer to `req`, from what the core has: whether DA
    /// claims Sixel, the grid and the cell size. The shape is xterm's
    /// (`CASE_GRAPHICS_ATTRIBUTES`, `[[source: xterm-reset]]`'s file): an
    /// unknown `Pa` is an error in `Pa` whatever the configuration, and a
    /// success becomes a failure while graphics are not configured --
    /// `[[source: ctlseqs]]`, "If xterm is able to support graphics
    /// (compile-time), but is not configured (runtime) for graphics, these
    /// responses will indicate a failure". baud is configured for graphics
    /// when DA lists Sixel, the configuration ctlseqs' Sixel section names
    /// ("If xterm is configured as VT240, VT241, VT330, VT340 or VT382 ...
    /// it supports Sixel Graphics controls").
    ///
    /// * `Pi = 1`, the colour registers: **256**, the number 070-9 §8.1.3
    ///   requires ("Devices must implement color numbers 0 to 255"), with a
    ///   number past them "mapped back into the supported range". Read,
    ///   reset and read-maximum answer it; a set succeeds only to 256, the one
    ///   count there is.
    /// * `Pi = 2`, the Sixel geometry: read answers the text area in pixels,
    ///   a failure while the cell size is unknown; read-maximum answers
    ///   `0 ; 0`, ctlseqs' shape for "implementations which do not use the
    ///   maximum graphics dimensions" -- baud limits an image by its arena's
    ///   area and a coordinate's range, not by a width and height; set and
    ///   reset fail, as xterm's do.
    /// * `Pi = 3`, the ReGIS geometry: read answers the page ReGIS draws on
    ///   (`regis_page_size`), the text area in pixels or the VT300's 800 by
    ///   480; read-maximum answers `0 ; 0`, as the Sixel maximum does, the
    ///   page following the window; set and reset fail, the page being the
    ///   window's. **Not gated on DA**: ReGIS is always on, so baud is
    ///   configured for it, though DA lists no attribute 3.
    /// * Any other item: an error in `Pi`.
    pub fn graphics(
        req: GraphicsRequest,
        claim: SixelClaim,
        rows: Rows,
        cols: Cols,
        cell_w: ImagePx,
        cell_h: ImagePx,
    ) -> Report {
        const REGISTERS: u16 = 256;
        let reply = match (req.item.0, req.action.0) {
            (1, 1 | 2 | 4) => GraphicsReply::Count(REGISTERS),
            (1, 3) if req.value.0 == REGISTERS => GraphicsReply::Count(REGISTERS),
            (1, 3) => GraphicsReply::Failure,
            (2, 1) if cell_w != ImagePx::ZERO && cell_h != ImagePx::ZERO => {
                GraphicsReply::Geometry {
                    width: cell_w.span(cols.index()),
                    height: cell_h.span(rows.index()),
                }
            }
            (2, 1..=3) => GraphicsReply::Failure,
            (2 | 3, 4) => GraphicsReply::Geometry {
                width: ImagePx::ZERO,
                height: ImagePx::ZERO,
            },
            (3, 1) => {
                let (width, height) = crate::regis_page_size(cols, rows, cell_w, cell_h);
                GraphicsReply::Geometry { width, height }
            }
            (3, 2 | 3) => GraphicsReply::Failure,
            (1..=3, _) => GraphicsReply::ErrorInAction,
            _ => GraphicsReply::ErrorInItem,
        };
        let reply = match (req.item.0, reply, claim) {
            (3, r, _) => r,
            (_, GraphicsReply::Count(_) | GraphicsReply::Geometry { .. }, SixelClaim::No) => {
                GraphicsReply::Failure
            }
            (_, r, _) => r,
        };
        Report::Graphics {
            item: req.item,
            reply,
        }
    }

    /// Longest possible encoding: `?1001`'s long release report under SGR,
    /// `ESC [ <` then six five-digit fields, five separators and `T` -- 39
    /// bytes. It took the title from the `?2048` report at 29, which took
    /// it from the palette reply at 22 when the pixel fields stopped being
    /// zero. `report_encodings_fit` pins it, and `ReportBytes::push` drops
    /// past it in silence, which is why that test names every variant.
    pub const MAX_LEN: usize = 39;

    /// Render the report to bytes.
    ///
    /// No allocator and no `format!`. The shape is chosen here, once per
    /// report, rather than at each call site.
    pub fn encode(self, c1: C1Transmit) -> ReportBytes {
        let mut e = ReportBytes {
            buf: [0; Report::MAX_LEN],
            len: ByteLen::ZERO,
        };
        for b in c1.csi() {
            e.push(*b);
        }
        match self {
            Report::CursorPosition { row, col } => {
                e.push_num(row.get());
                e.push(b';');
                e.push_num(col.get());
                e.push(b'R');
            }
            Report::OperatingStatus => {
                e.push_num(0);
                e.push(b'n');
            }
            Report::ColorScheme(s) => {
                e.push(b'?');
                e.push_num(997);
                e.push(b';');
                e.push_num(s.code());
                e.push(b'n');
            }
            Report::Focus(f) => e.push(match f {
                Focus::Gained => b'I',
                Focus::Lost => b'O',
            }),
            Report::InbandResize {
                rows,
                cols,
                height,
                width,
            } => {
                e.push_num(48);
                e.push(b';');
                e.push_num(u16::try_from(rows.index()).unwrap_or(u16::MAX));
                e.push(b';');
                e.push_num(u16::try_from(cols.index()).unwrap_or(u16::MAX));
                e.push(b';');
                e.push_num(height.raw());
                e.push(b';');
                e.push_num(width.raw());
                e.push(b't');
            }
            Report::TextAreaPixels { height, width } => {
                e.push_num(4);
                e.push(b';');
                e.push_num(height.raw());
                e.push(b';');
                e.push_num(width.raw());
                e.push(b't');
            }
            Report::CellPixels { height, width } => {
                e.push_num(6);
                e.push(b';');
                e.push_num(height.raw());
                e.push(b';');
                e.push_num(width.raw());
                e.push(b't');
            }
            Report::TextAreaChars { rows, cols } => {
                e.push_num(8);
                e.push(b';');
                e.push_num(u16::try_from(rows.index()).unwrap_or(u16::MAX));
                e.push(b';');
                e.push_num(u16::try_from(cols.index()).unwrap_or(u16::MAX));
                e.push(b't');
            }
            Report::WindowState(w) => {
                e.push_num(w.code());
                e.push(b't');
            }
            Report::Position(at) => {
                e.push_num(3);
                e.push(b';');
                e.push_num(at.x.wire());
                e.push(b';');
                e.push_num(at.y.wire());
                e.push(b't');
            }
            Report::WindowPixels(s) | Report::ScreenPixels(s) => {
                e.push_num(match self {
                    Report::WindowPixels(_) => 4,
                    _ => 5,
                });
                e.push(b';');
                e.push_num(s.height.raw());
                e.push(b';');
                e.push_num(s.width.raw());
                e.push(b't');
            }
            Report::ScreenChars { rows, cols } => {
                e.push_num(9);
                e.push(b';');
                e.push_num(rows);
                e.push(b';');
                e.push_num(cols);
                e.push(b't');
            }
            Report::KittyFlags(f) => {
                e.push(b'?');
                e.push_num(u16::from(f.bits()));
                e.push(b'u');
            }
            Report::PrimaryDeviceAttributes(d) => {
                let [identity, attribute] = d.params();
                e.push(b'?');
                e.push_num(identity);
                e.push(b';');
                e.push_num(attribute);
                e.push(b'c');
            }
            Report::MouseSgr(ev) => {
                // `cb` is computed here rather than passed in: it is a wire
                // encoding, and the only place that should know how to build
                // one is the place that writes it to the wire.
                e.push(b'<');
                e.push_num(ev.cb());
                e.push(b';');
                e.push_num(ev.col.one_based().get());
                e.push(b';');
                e.push_num(ev.row.one_based().get());
                e.push(match ev.kind {
                    MouseKind::Release => b'm',
                    _ => b'M',
                });
            }
            Report::MouseX10(ev) => {
                // `ESC [ M` then three bytes, each biased by 32. The caller
                // has already refused anything past 223: encoding it here
                // would wrap and name a different cell.
                e.push(b'M');
                e.push((ev.cb_x10() as u8).wrapping_add(32));
                e.push((ev.col.one_based().get() as u8).wrapping_add(32));
                e.push((ev.row.one_based().get() as u8).wrapping_add(32));
            }
            Report::MouseUtf8(ev) => {
                // X10's shape, each value `+ 32` and then UTF-8: one byte
                // below 128, which is "identical under both modes", and the
                // two-byte form from 128 to 2047. xterm writes the two bytes
                // as `0xC0 + (v >> 6)` and `0x80 + (v & 0x3F)`
                // (`EmitMousePosition`, `[[source: xterm-button]]`), which for
                // those values is exactly what `char::encode_utf8` writes.
                e.push(b'M');
                e.push_utf8(ev.cb_x10() + 32);
                e.push_utf8(ev.col.one_based().get() + 32);
                e.push_utf8(ev.row.one_based().get() + 32);
            }
            Report::MouseUrxvt(ev) => {
                // No `<`, X10's button value with its `+ 32` kept -- "the
                // same button encoding as X10, but printing it as a decimal
                // integer" -- and `M` whatever happened.
                e.push_num(ev.cb_x10() + 32);
                e.push(b';');
                e.push_num(ev.col.one_based().get());
                e.push(b';');
                e.push_num(ev.row.one_based().get());
                e.push(b'M');
            }
            Report::MouseSgrPixels(ev, px) => {
                // SGR's shape; the pixel is zero-based on the event and
                // one-based on the wire, xterm's `value + 1`
                // (`EmitMousePosition`, `[[source: xterm-button]]`). The
                // caller refuses `u16::MAX`, whose `+ 1` does not fit, so the
                // saturation never decides a byte.
                e.push(b'<');
                e.push_num(ev.cb());
                e.push(b';');
                e.push_num(px.x.raw().saturating_add(1));
                e.push(b';');
                e.push_num(px.y.raw().saturating_add(1));
                e.push(match ev.kind {
                    MouseKind::Release => b'm',
                    _ => b'M',
                });
            }
            Report::Highlight(end, encoding) => {
                let (fin, wire, n) = match end {
                    HighlightEnd::Same(at) => (b't', [[at.col.get(), at.row.get()]; 3], 1),
                    HighlightEnd::Moved { start, end, mouse } => (
                        b'T',
                        [
                            [start.col.get(), start.row.get()],
                            [end.col.get(), end.row.get()],
                            mouse.wire(),
                        ],
                        3,
                    ),
                };
                let values = wire[..n].iter().flatten().copied();
                match encoding {
                    // `CSI t` or `CSI T`, then each value `+ 32` in a byte;
                    // the caller has refused anything past 223.
                    MouseEncoding::X10 => {
                        e.push(fin);
                        for v in values {
                            e.push((v as u8).wrapping_add(32));
                        }
                    }
                    // The same, each `+ 32` written as UTF-8 once it passes
                    // 127; the caller has refused anything past 2015.
                    MouseEncoding::Utf8 => {
                        e.push(fin);
                        for v in values {
                            e.push_utf8(v + 32);
                        }
                    }
                    MouseEncoding::Sgr | MouseEncoding::SgrPixels | MouseEncoding::Urxvt => {
                        if encoding != MouseEncoding::Urxvt {
                            e.push(b'<');
                        }
                        for (i, v) in values.enumerate() {
                            if i > 0 {
                                e.push(b';');
                            }
                            e.push_num(v);
                        }
                        e.push(fin);
                    }
                }
            }
            Report::Vt52Identity => {
                e.len = ByteLen::ZERO;
                e.push(0x1b);
                e.push(b'/');
                e.push(b'Z');
            }
            Report::AreaChecksum { id, sum } => {
                e.len = ByteLen::ZERO;
                for b in c1.dcs() {
                    e.push(*b);
                }
                e.push_num(id.0);
                e.push(b'!');
                e.push(b'~');
                const HEX: &[u8; 16] = b"0123456789ABCDEF";
                let v = sum.value();
                for shift in [12u16, 8, 4, 0] {
                    e.push(HEX[usize::from((v >> shift) & 0xf)]);
                }
                for b in c1.st() {
                    e.push(*b);
                }
            }
            Report::ExtendedCursorPosition { row, col } => {
                e.push(b'?');
                e.push_num(row.get());
                e.push(b';');
                e.push_num(col.get());
                e.push(b';');
                e.push_num(1);
                e.push(b'R');
            }
            Report::DecStatus(r) => {
                e.push(b'?');
                for (k, v) in r.params().iter().enumerate() {
                    if k > 0 {
                        e.push(b';');
                    }
                    e.push_num(*v);
                }
                e.push(b'n');
            }
            Report::Locator(r) => {
                let (v, n) = r.params();
                for (k, v) in v.iter().take(n).enumerate() {
                    if k > 0 {
                        e.push(b';');
                    }
                    e.push_num(*v);
                }
                e.push(b'&');
                e.push(b'w');
            }
            Report::MacroSpace => {
                e.push_num(0);
                e.push(b'*');
                e.push(b'{');
            }
            Report::TerminalParameters { sol } => {
                for v in [sol, 1, 1, 120, 120, 1] {
                    e.push_num(v);
                    e.push(b';');
                }
                e.push_num(0);
                e.push(b'x');
            }
            Report::PreferredSupplemental => {
                e.len = ByteLen::ZERO;
                for b in c1.dcs() {
                    e.push(*b);
                }
                for b in b"0!u%5" {
                    e.push(*b);
                }
                for b in c1.st() {
                    e.push(*b);
                }
            }
            Report::DisplayedExtent { rows, cols } => {
                e.push_num(u16::try_from(rows.index()).unwrap_or(u16::MAX));
                e.push(b';');
                e.push_num(u16::try_from(cols.index()).unwrap_or(u16::MAX));
                for b in b";1;1;1\"w" {
                    e.push(*b);
                }
            }
            Report::KeyOption { which, pp, value } => {
                e.push(b'>');
                e.push_num(pp);
                e.push(b';');
                e.push_num(value);
                e.push(which.final_byte());
            }
            Report::Version => {
                // Not a CSI: `DCS > | <text> ST`. The introducer written here
                // replaces the `ESC [` the encoder starts with, so the branch
                // rewinds rather than appending. The text is the package's
                // here; a `Term` told another writes that one
                // (`Paint::want_reply`), which may be longer than this
                // buffer.
                e.len = ByteLen::ZERO;
                for part in VersionText::PACKAGE.reply_parts(c1) {
                    for &b in part {
                        e.push(b);
                    }
                }
            }
            Report::Color {
                index,
                r,
                g,
                b,
                term,
            } => {
                // Not a CSI: `OSC 4 ; c ; spec`. Rewinds over the `ESC [` the
                // encoder starts with, the way `Version` and `Vt52Identity`
                // do.
                e.len = ByteLen::ZERO;
                let frame = StringReply::osc(term);
                for b in frame.open(c1) {
                    e.push(*b);
                }
                e.push(b'4');
                e.push(b';');
                e.push_num(index.get() as u16);
                e.push(b';');
                for &c in b"rgb:" {
                    e.push(c);
                }
                e.push_hex(r);
                e.push(b'/');
                e.push_hex(g);
                e.push(b'/');
                e.push_hex(b);
                for b in frame.close(c1) {
                    e.push(*b);
                }
            }
            Report::SpecialColor {
                which,
                via,
                r,
                g,
                b,
                term,
            } => {
                // `OSC 5 ; c ; spec` or `OSC 4 ; 256+c ; spec`, the rewind
                // and spec form of `Color`.
                e.len = ByteLen::ZERO;
                let frame = StringReply::osc(term);
                for b in frame.open(c1) {
                    e.push(*b);
                }
                match via {
                    SpecialVia::Special => {
                        e.push(b'5');
                        e.push(b';');
                        e.push_num(which.code());
                    }
                    SpecialVia::Palette => {
                        e.push(b'4');
                        e.push(b';');
                        e.push_num(256 + which.code());
                    }
                }
                e.push(b';');
                for &c in b"rgb:" {
                    e.push(c);
                }
                e.push_hex(r);
                e.push(b'/');
                e.push_hex(g);
                e.push(b'/');
                e.push_hex(b);
                for b in frame.close(c1) {
                    e.push(*b);
                }
            }
            Report::DynamicColor {
                which,
                r,
                g,
                b,
                term,
            } => {
                // `OSC 10 ; rgb:RRRR/GGGG/BBBB` -- the same rewind and the
                // same spec form as `Color`, with the colour's own code in
                // place of `4 ; index`.
                e.len = ByteLen::ZERO;
                let frame = StringReply::osc(term);
                for b in frame.open(c1) {
                    e.push(*b);
                }
                e.push_num(which.code());
                e.push(b';');
                for &c in b"rgb:" {
                    e.push(c);
                }
                e.push_hex(r);
                e.push(b'/');
                e.push_hex(g);
                e.push(b'/');
                e.push_hex(b);
                for b in frame.close(c1) {
                    e.push(*b);
                }
            }
            Report::ModeStatus {
                marker,
                mode,
                state,
            } => {
                if marker == Marker::Private {
                    e.push(b'?');
                }
                e.push_num(mode.0);
                e.push(b';');
                e.push_num(state.wire());
                e.push(b'$');
                e.push(b'y');
            }
            Report::Graphics { item, reply } => {
                e.push(b'?');
                e.push_num(item.0);
                e.push(b';');
                e.push_num(reply.status());
                match reply {
                    GraphicsReply::Count(n) => {
                        e.push(b';');
                        e.push_num(n);
                    }
                    GraphicsReply::Geometry { width, height } => {
                        e.push(b';');
                        e.push_num(width.raw());
                        e.push(b';');
                        e.push_num(height.raw());
                    }
                    _ => {}
                }
                e.push(b'S');
            }
            Report::ColorStack { current, stored } => {
                e.push(b'?');
                e.push_num(current);
                e.push(b';');
                e.push_num(stored);
                e.push(b'#');
                e.push(b'Q');
            }
            Report::TitlePosition { index, max } => {
                e.push_num(index);
                e.push(b';');
                e.push_num(max);
                e.push(b'#');
                e.push(b'S');
            }
        }
        e
    }
}

impl ReportBytes {
    /// **Four** lowercase hex digits for one 8-bit channel: the byte twice,
    /// which is `v * 257`, the 16-bit value X colours carry.
    ///
    /// This said "two hex digits, which is the width `rgb:` specifies", and
    /// `rgb:` specifies no width: XParseColor reads one to four digits a
    /// channel, so either is a colour an application may *send*. What
    /// matters in a *reply* is what readers parse, and they parse xterm's,
    /// which is four -- `[[source: sfte]]` answers `rgb:%02x%02x/...` for the
    /// same reason. `[[source: yazi-emulator]]`'s `light_bg` reads the
    /// channels at fixed offsets 0..2, 5..7 and 10..12, so a two-digit reply
    /// fails there, and yazi takes the failure as a dark background whatever
    /// the theme.
    fn push_hex(&mut self, b: u8) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for _ in 0..2 {
            self.push(HEX[(b >> 4) as usize]);
            self.push(HEX[(b & 0x0f) as usize]);
        }
    }

    fn push(&mut self, b: u8) {
        // Cannot overflow: MAX_LEN is the longest variant, checked by
        // `report_encodings_fit` in baud-domain's tests.
        let n = self.len.index();
        if n < Report::MAX_LEN {
            self.buf[n] = b;
            self.len = ByteLen::from_index(n + 1);
        }
    }
    /// Decimal, most significant digit first.
    fn push_num(&mut self, v: u16) {
        let d = Decimal::of(v);
        for b in d.as_bytes() {
            self.push(*b);
        }
    }
    /// `v` as UTF-8: one byte below 128, two from 128 to 2047, which is
    /// every value `?1005` writes once the caller has refused an ordinate
    /// past 2015. A value with no scalar -- none below 2048 is a surrogate
    /// -- writes nothing rather than a guess.
    fn push_utf8(&mut self, v: u16) {
        if let Some(c) = char::from_u32(u32::from(v)) {
            let mut buf = [0; 4];
            for b in c.encode_utf8(&mut buf).as_bytes() {
                self.push(*b);
            }
        }
    }
}

/// The two DSR reports baud answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    /// `CSI 5 n` -> `CSI 0 n`, "OK".
    Operating,
    /// `CSI 6 n` -> `CSI r ; c R`, the cursor position, 1-based.
    CursorPosition,
}

impl StatusKind {
    pub fn from_param(p: Option<Param>) -> Option<StatusKind> {
        match p.map_or(0, |x| x.0) {
            5 => Some(StatusKind::Operating),
            6 => Some(StatusKind::CursorPosition),
            _ => None,
        }
    }
}

/// A report answered out of line, its parameters read where it is answered
/// ([`QueryOp::with`]), as [`AreaOp`]'s are: the variant carries nothing, so
/// `csi_final`'s dispatch keeps nothing live for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryOp {
    /// `CSI ? Ps n`, one of the DEC private device status reports
    /// [`DecStatus`] names.
    DecStatus,
    /// DECREQTPARM, `CSI Ps x` with `Ps` 0 or 1 (`[[source: vt100-ug-ch3]]`).
    TerminalParameters,
    /// DECRQUPSS, `CSI & u` (`[[source: dec-std-070]]` p. 5-125).
    PreferredSupplemental,
    /// DECRQDE, `CSI " v` (`[[source: dec-std-070]]` p. 5-88).
    DisplayedExtent,
    /// XTQMODKEYS, `CSI ? Pp m`, one answer a parameter.
    ModifyKeys,
    /// XTQFMTKEYS, `CSI > Pp g`, one answer a parameter.
    FormatKeys,
}

/// A [`QueryOp`] with its parameters read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueryCall {
    DecStatus(DecStatus),
    /// DECREPTPARM's first parameter: the request's `<sol>` plus two.
    TerminalParameters {
        sol: u16,
    },
    PreferredSupplemental,
    DisplayedExtent,
    /// The keyboard queries take a list, which the caller walks.
    ModifyKeys,
    FormatKeys,
}

impl QueryOp {
    /// The query `p` asks, or `None` when the parameters name none, which
    /// the caller counts as it counts an unsupported sequence: a private
    /// `n` no text gives, a DECREPTPARM, or a parameter on DECRQUPSS or
    /// DECRQDE, which "take no parameters" (`[[source: dec-std-070]]`
    /// pp. 5-125 and 5-88), so only the bare form is the request, as SCOSC's
    /// is.
    pub fn with(self, p: &Params<'_>) -> Option<QueryCall> {
        Some(match self {
            QueryOp::DecStatus => QueryCall::DecStatus(DecStatus::of(p)?),
            QueryOp::TerminalParameters => QueryCall::TerminalParameters {
                sol: QueryOp::solicit(p)? + 2,
            },
            QueryOp::PreferredSupplemental if p.all_default() => QueryCall::PreferredSupplemental,
            QueryOp::DisplayedExtent if p.all_default() => QueryCall::DisplayedExtent,
            QueryOp::PreferredSupplemental | QueryOp::DisplayedExtent => return None,
            QueryOp::ModifyKeys => QueryCall::ModifyKeys,
            QueryOp::FormatKeys => QueryCall::FormatKeys,
        })
    }

    /// The final byte the query arrived with, for the count of a sequence
    /// it does not answer.
    pub fn final_byte(self) -> FinalByte {
        let b = match self {
            QueryOp::DecStatus => b'n',
            QueryOp::TerminalParameters => b'x',
            QueryOp::PreferredSupplemental => b'u',
            QueryOp::DisplayedExtent => b'v',
            QueryOp::ModifyKeys => b'm',
            QueryOp::FormatKeys => b'g',
        };
        FinalByte::of_final(StreamByte::from_stream(b))
    }

    /// DECREQTPARM's `<sol>`: "0 or none" and "1" are requests
    /// (`[[source: vt100-ug-ch3]]`); 2 and 3 are the reports a terminal
    /// sends, and a terminal does not answer them.
    pub fn solicit(p: &Params<'_>) -> Option<u16> {
        match p.raw(ParamIdx::FIRST, 0) {
            v @ (0 | 1) => Some(v),
            _ => None,
        }
    }

    /// The level each report is the text's at, for [`Csi::level`].
    pub fn level(self) -> Level {
        match self {
            // "Request Terminal Parameters" is the VT100's, and no later
            // DEC text has it.
            QueryOp::TerminalParameters => Level::Vt100,
            // "Levels: 2x (8-bit Interface Architecture), 3-4".
            QueryOp::PreferredSupplemental => Level::Vt220,
            // "Levels: 3x, 4x (Windowing)"; this table has no third level,
            // and the private DSRs run from Level 1x to Level 4 by report.
            QueryOp::DisplayedExtent | QueryOp::DecStatus => Level::Vt420,
            QueryOp::ModifyKeys | QueryOp::FormatKeys => Level::Xterm,
        }
    }
}

/// The DEC private device status reports, `CSI ? Ps n`, each by the
/// report its text gives (`[[source: dec-std-070]]`, `[[source: vt420-rm]]`
/// chapter 12, `[[source: vt510-dsr]]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecStatus {
    /// `?6`, DECXCPR: the cursor with its page.
    ExtendedCursor,
    /// `?15`, the printer port.
    Printer,
    /// `?25`, the user-defined keys.
    UserKeys,
    /// `?26`, the keyboard.
    Keyboard,
    /// `?55`, the locator's status.
    LocatorStatus,
    /// `?56`, the locator's type.
    LocatorType,
    /// `?62`, DECMSR, the macro space.
    MacroSpace,
    /// `?63 ; Pid`, DECCKSR of the macro definitions, labelled `id`.
    MemoryChecksum { id: Param },
    /// `?75`, the data integrity flag.
    DataIntegrity,
    /// `?85`, the multiple-session configuration.
    Sessions,
}

impl DecStatus {
    /// The report `CSI ? Ps n` asks for, or `None` for a `Ps` no text gives.
    /// `?996` is the colour preference query and not a DEC report.
    pub fn of(p: &Params<'_>) -> Option<DecStatus> {
        Some(match p.raw(ParamIdx::FIRST, 0) {
            6 => DecStatus::ExtendedCursor,
            15 => DecStatus::Printer,
            25 => DecStatus::UserKeys,
            26 => DecStatus::Keyboard,
            55 => DecStatus::LocatorStatus,
            56 => DecStatus::LocatorType,
            62 => DecStatus::MacroSpace,
            // "pid is an optional numeric parameter" (p. 5-180); omitted, the
            // label is 0, as DECRQCRA's is.
            63 => DecStatus::MemoryChecksum {
                id: p.at(ParamIdx::SECOND).unwrap_or(Param::ZERO),
            },
            75 => DecStatus::DataIntegrity,
            85 => DecStatus::Sessions,
            _ => return None,
        })
    }
}

/// The data integrity flag of `[[source: dec-std-070]]` 070-4 § 3.3.2:
/// "Power-up, No request since last power-up or reset" until the first
/// report, and then "OK, no comm error detected since last report". The
/// third state, "Error, a comm error has been detected", needs a line that
/// can fail parity or framing, and a pty has neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Integrity {
    /// Set at power-up and by RIS; `?73`.
    #[default]
    PowerUp,
    /// Reported since; `?70`.
    Ok,
}

/// What a [`DecStatus`] other than the cursor, the macro space and the
/// checksum answers: `CSI ? Ps n` with one parameter, or the keyboard's and
/// the locator type's longer forms.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecReply {
    /// `CSI ? 13 n`: "The printer is not connected to the terminal"
    /// (070-7 p. 7-33).
    NoPrinter,
    /// `CSI ? 10 n`: "The printer is ready to print" (070-7 p. 7-32).
    PrinterReady,
    /// `CSI ? 11 n`: "The printer is not ready to print. The Data Set Ready
    /// signal at the printer port was previously asserted (since power on),
    /// but is not asserted now" (070-7 p. 7-32).
    PrinterNotReady,
    /// `CSI ? 23 n`: "No User Defined Keys are present. The device does not
    /// support this extension" (070-11 p. 11-7).
    NoUserKeys,
    /// `CSI ? 27 ; 0 ; 0 n`: the keyboard language "cannot be determined"
    /// and the keyboard ready (`[[source: vt510-dsr-kbd]]`). No keyboard
    /// type: each `Ptyp` names a DEC keyboard, none an unidentified one, and
    /// the three-parameter form is ctlseqs' VT300 one.
    KeyboardUnknown,
    /// `CSI ? 50 n`: "Locator ready" (070-13 p. 15, PDF 1030), "a locator
    /// device is plugged in" -- the embedder's pointer, once it has said it
    /// has one ([`LocatorDevice::Present`]); `[[source: vt3xx-tp]]` p. 203,
    /// "Locator device ready. The terminal detects a locator device."
    LocatorReady,
    /// `CSI ? 53 n`: "No locator", "no locator is currently plugged in"
    /// (070-13 p. 15): an embedder that has not said it has a pointer.
    NoLocator,
    /// `CSI ? 57 ; 1 n`: "Device is Digital Corporate Mouse" (070-13), "the
    /// device as Digital's optional mouse" (`[[source: vt3xx-tp]]` p. 204);
    /// ctlseqs' "Mouse". The other answer, `57;2`, is a tablet.
    LocatorMouse,
    /// `CSI ? 57 ; 0 n`: "Unknown Locator" (070-13 p. 15), ctlseqs' "Cannot
    /// identify", the answer it pairs with `?53`: no pointer told.
    LocatorUnknown,
    /// `CSI ? 73 n` or `CSI ? 70 n`, from the flag.
    DataIntegrity(Integrity),
    /// `CSI ? 83 n`: "not configured for virtual terminals" (070-4).
    SingleSession,
}

impl DecReply {
    fn params(self) -> &'static [u16] {
        match self {
            DecReply::NoPrinter => &[13],
            DecReply::PrinterReady => &[10],
            DecReply::PrinterNotReady => &[11],
            DecReply::NoUserKeys => &[23],
            DecReply::KeyboardUnknown => &[27, 0, 0],
            DecReply::LocatorReady => &[50],
            DecReply::NoLocator => &[53],
            DecReply::LocatorMouse => &[57, 1],
            DecReply::LocatorUnknown => &[57, 0],
            DecReply::DataIntegrity(Integrity::PowerUp) => &[73],
            DecReply::DataIntegrity(Integrity::Ok) => &[70],
            DecReply::SingleSession => &[83],
        }
    }
}

/// Which of xterm's two key-option queries a [`Report::KeyOption`] answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyOption {
    /// XTQMODKEYS, answered as XTMODKEYS, `CSI > Pp ; Pv m`.
    Modify,
    /// XTQFMTKEYS, answered as XTFMTKEYS, `CSI > Pp ; Pv f`.
    Format,
}

impl KeyOption {
    fn final_byte(self) -> u8 {
        match self {
            KeyOption::Modify => b'm',
            KeyOption::Format => b'f',
        }
    }
}

/// A `u16` in decimal, without an allocator and without a cast.
///
/// **Written once because it had been written twice.** `ReportBytes::push_num`
/// and the key encoder both need it, and `baud-core2`'s reply path spelled it
/// twice more as `emit_param` and `emit_dec` -- four copies of a loop whose
/// only interesting property is that it indexes a digit table rather than
/// casting `n % 10`, which would be another raw-integer site against the
/// audit budget.
///
/// Five digits, because `u16` saturates at 65535 and `Param` saturates there
/// deliberately so a reply can carry the value back.
pub(crate) struct Decimal {
    buf: [u8; 5],
    at: usize,
}

impl Decimal {
    pub(crate) fn of(v: u16) -> Decimal {
        const DIGITS: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 5];
        let mut i = buf.len();
        let mut n = v;
        loop {
            i -= 1;
            buf[i] = DIGITS[usize::from(n % 10)];
            n /= 10;
            if n == 0 || i == 0 {
                break;
            }
        }
        Decimal { buf, at: i }
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buf[self.at..]
    }
}

/// A `u32` rendered to decimal, for `[[source: kitty-keyboard]]`'s key numbers:
/// a `Char`'s number is its codepoint, up to `U+10FFFF` -- seven digits, which
/// [`Decimal`]'s five cannot hold. The functional-key numbers fit `u16`, but
/// the codepoint does not, so the CSI-u encoder renders through this.
pub(crate) struct Decimal32 {
    buf: [u8; 10],
    at: usize,
}

impl Decimal32 {
    pub(crate) fn of(v: u32) -> Decimal32 {
        const DIGITS: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 10];
        let mut i = buf.len();
        let mut n = v;
        loop {
            i -= 1;
            buf[i] = DIGITS[(n % 10) as usize];
            n /= 10;
            if n == 0 || i == 0 {
                break;
            }
        }
        Decimal32 { buf, at: i }
    }
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.buf[self.at..]
    }
}
