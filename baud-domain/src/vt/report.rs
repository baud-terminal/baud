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
    /// CPR — Cursor Position Report, `CSI r ; c R`. `[[source: ctlseqs]]`:
    /// "Report Cursor Position (CPR) [row;column]. Result is CSI r ; c R".
    ///
    /// 1-based, like CUP, so the report can be fed straight back as one.
    CursorPosition { row: OneBased, col: OneBased },
    /// DSR operating status, `CSI 0 n`. `[[source: ctlseqs]]`: "Status Report.
    /// Result ("OK") is CSI 0 n".
    OperatingStatus,
    /// Primary DA, `CSI ? Ps c`.
    PrimaryDeviceAttributes(DeviceAttribute),
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

impl Report {
    /// Longest possible encoding: an SGR mouse report, which is `ESC [ <` then
    /// three five-digit fields, two separators and a final byte. Five digits
    /// covers `MAX_COLS` and `MAX_ROWS` with room to spare, and nothing larger
    /// can be a coordinate on this grid. `report_encodings_fit` pins it.
    pub const MAX_LEN: usize = 21;

    /// Render the report to bytes.
    ///
    /// No allocator and no `format!`. The shape is chosen here, once per
    /// report, rather than at each call site.
    pub fn encode(self) -> ReportBytes {
        let mut e = ReportBytes {
            buf: [0; Report::MAX_LEN],
            len: ByteLen::ZERO,
        };
        e.push(0x1b);
        e.push(b'[');
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
            Report::PrimaryDeviceAttributes(d) => {
                e.push(b'?');
                e.push_num(d.param());
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
        const DIGITS: &[u8; 10] = b"0123456789";
        let mut tmp = [0u8; 5];
        let mut i = tmp.len();
        let mut n = v;
        loop {
            i -= 1;
            // Indexed rather than `(n % 10) as u8`: the cast would be another
            // raw-integer site against the audit budget, and a table lookup
            // says what it means.
            tmp[i] = DIGITS[usize::from(n % 10)];
            n /= 10;
            if n == 0 || i == 0 {
                break;
            }
        }
        let mut k = i;
        while k < tmp.len() {
            self.push(tmp[k]);
            k += 1;
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
