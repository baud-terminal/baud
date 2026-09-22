use crate::*;

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
    /// Primary DA, `CSI ? Ps c`. The [`SixelClaim`] flag adds attribute `4` to the
    /// reply when the embedder has provisioned image memory, so an application
    /// learns baud will draw a Sixel and uses it rather than a text fallback.
    PrimaryDeviceAttributes(DeviceAttribute, SixelClaim),
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
    /// DECRPM, the answer to DECRQM: `CSI Ps ; Pm $ y`, or
    /// `CSI ? Ps ; Pm $ y` for the private namespace.
    ///
    /// The marker is carried because the two namespaces are unrelated: `CSI 4`
    /// is IRM and `CSI ? 4` is DECSCLM. Answering a private request without the
    /// `?` would have the application read it as an answer about a different
    /// mode entirely.
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
    /// `OSC 10 ; rgb:RRRR/GGGG/BBBB ST` or `OSC 11 ; ...`: the answer to a
    /// dynamic-colour query, in the setter's own shape so it can be fed back.
    DynamicColor {
        which: DynamicColor,
        r: u8,
        g: u8,
        b: u8,
        term: StringTerm,
    },
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
    ///
    /// Only the SGR encoding exists here. The X10 scheme it replaced encodes
    /// each ordinate as one byte at `value + 32`, which caps it at 223 -- and
    /// baud's grid goes to 2048 columns. A report from column 400 in that
    /// scheme is not a truncated answer, it is a *wrong* one, pointing the
    /// application at some other cell. Under the rule that a wrong answer beats
    /// no answer nowhere, baud reports nothing unless `?1006` is on.
    MouseSgr(MouseEvent),
    /// `ESC [ M Cb Cx Cy`, each ordinate offset by 32. See
    /// [`MouseEncoding::X10`] for why the caller must check the range first.
    MouseX10(MouseEvent),
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

/// Which terminal baud claims to be, in a Primary DA response.
///
/// An enum with one variant on purpose. `[[source: ctlseqs]]` lists the
/// identities under `CSI Ps c`, and picking one is a promise about *everything*
/// at that level -- over-claiming makes applications send sequences baud drops,
/// which is the `parsed` failure by another route. Widening the claim should
/// cost a deliberate edit here, next to the reason, rather than a changed digit
/// in a byte string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceAttribute {
    /// `CSI ? 6 c`. What baud actually is: DECSTBM (VT100) and IL/DL (VT102).
    /// It has selective erase, which is VT220, and still answers VT102 --
    /// one VT220 feature does not buy the level, and it has neither UDKs nor
    /// NRCS nor 132-column mode.
    Vt102,
}

impl DeviceAttribute {
    /// The `Ps` in `CSI ? Ps c`.
    fn param(self) -> u16 {
        match self {
            DeviceAttribute::Vt102 => 6,
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
    /// DCS: the introducer for a device control string reply.
    pub fn dcs(self) -> &'static [u8] {
        match self {
            C1Transmit::SevenBit => b"\x1bP",
            C1Transmit::EightBit => b"\x90",
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

    /// The answer to an XTWINOPS operation, or `None` when there is none to
    /// give: a [`WindowOp::Refused`] form, or a pixel question before the
    /// cell size is known.
    ///
    /// **The two pixel answers need the cell size, and without it there is
    /// no answer at all** rather than `CSI 6 ; 0 ; 0 t`. Mode 2048's
    /// specification defines zero as "not capable"; XTWINOPS defines no such
    /// value, and a client that divides by the cell size is better served by
    /// the silence it already has to handle than by a zero it may not. The
    /// text area in cells is always known.
    pub fn window(
        op: WindowOp,
        rows: Rows,
        cols: Cols,
        cell_w: ImagePx,
        cell_h: ImagePx,
    ) -> Option<Report> {
        let known = cell_w != ImagePx::ZERO && cell_h != ImagePx::ZERO;
        match (op, known) {
            (WindowOp::Refused, _) => None,
            (WindowOp::TextAreaChars, _) => Some(Report::TextAreaChars { rows, cols }),
            (_, false) => None,
            (WindowOp::TextAreaPixels, true) => Some(Report::TextAreaPixels {
                height: cell_h.span(rows.index()),
                width: cell_w.span(cols.index()),
            }),
            (WindowOp::CellPixels, true) => Some(Report::CellPixels {
                height: cell_h,
                width: cell_w,
            }),
        }
    }

    /// Longest possible encoding: the `?2048` report with its pixel fields,
    /// `ESC [ 48` then four five-digit fields, four separators and a final
    /// byte -- 29 bytes. It took the title from the palette reply at 22 when
    /// the pixel fields stopped being zero. `report_encodings_fit` pins it,
    /// and `ReportBytes::push` drops past it in silence, which is why that
    /// test names every variant.
    pub const MAX_LEN: usize = 29;

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
            Report::KittyFlags(f) => {
                e.push(b'?');
                e.push_num(u16::from(f.bits()));
                e.push(b'u');
            }
            Report::PrimaryDeviceAttributes(d, sixel) => {
                e.push(b'?');
                e.push_num(d.param());
                // Attribute `4` is Sixel graphics (`[[source: ctlseqs]]`), added
                // only when the store is provisioned so the claim is honest: an
                // unprovisioned baud refuses every Sixel, and advertising it
                // would make an application send images it draws as nothing.
                if let SixelClaim::Yes = sixel {
                    e.push(b';');
                    e.push_num(4);
                }
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
                // `ESC [ M` then three bytes, each ordinate biased by 32. The
                // caller has already refused anything past 223: encoding it
                // here would wrap and name a different cell.
                e.push(b'M');
                e.push((ev.cb() as u8).wrapping_add(32));
                e.push((ev.col.one_based().get() as u8).wrapping_add(32));
                e.push((ev.row.one_based().get() as u8).wrapping_add(32));
            }
            Report::Vt52Identity => {
                e.len = ByteLen::ZERO;
                e.push(0x1b);
                e.push(b'/');
                e.push(b'Z');
            }
            Report::Version => {
                // Not a CSI: `DCS > | <text> ST`. The introducer written here
                // replaces the `ESC [` the encoder starts with, so the branch
                // rewinds rather than appending.
                e.len = ByteLen::ZERO;
                e.push(0x1b);
                e.push(b'P');
                e.push(b'>');
                e.push(b'|');
                for &b in b"baud" {
                    e.push(b);
                }
                e.push(0x1b);
                e.push(b'\\');
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
                e.push(0x1b);
                e.push(b']');
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
                match term {
                    StringTerm::St => {
                        e.push(0x1b);
                        e.push(b'\\');
                    }
                    StringTerm::Bel => e.push(0x07),
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
                e.push(0x1b);
                e.push(b']');
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
                match term {
                    StringTerm::St => {
                        e.push(0x1b);
                        e.push(b'\\');
                    }
                    StringTerm::Bel => e.push(0x07),
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

/// A `u16` in decimal, without an allocator and without a cast.
///
/// **Written once because it had been written twice.** `ReportBytes::push_num`
/// and the key encoder both need it, and `baud-core2`'s reply path spells it
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
