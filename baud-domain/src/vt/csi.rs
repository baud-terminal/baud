use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Position of a parameter within a CSI sequence.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamIdx(pub(crate) u8);

impl ParamIdx {
    pub const FIRST: ParamIdx = ParamIdx(0);
    pub const SECOND: ParamIdx = ParamIdx(1);
    pub const THIRD: ParamIdx = ParamIdx(2);
    pub const FOURTH: ParamIdx = ParamIdx(3);
    pub const FIFTH: ParamIdx = ParamIdx(4);
    /// How many parameters a CSI or DCS sequence can hold.
    ///
    /// `Term::params` is sized from this, so the array and the index cannot
    /// disagree about the limit -- they did when the limit was the literal
    /// `16` written in four places.
    ///
    /// **Derived, not chosen.** The longest parameter list baud has to accept
    /// is the one baud itself emits: the DECRQSS SGR reply is a leading `0`,
    /// every attribute in [`Attrs::WIRE`], a font, and three colours in their
    /// five-parameter direct form. DECRQSS answers with something the
    /// application can send back verbatim, so a reply the parser cannot re-read
    /// is a reply that does not round-trip -- and at sixteen it could not.
    /// Bold plus a foreground, a background and an underline colour is already
    /// a seventeen-parameter answer, which is an editor drawing an undercurl,
    /// not a corner case.
    ///
    /// Writing a number here instead would put the limit back out of step the
    /// next time `WIRE` grows, which is exactly how `SGR 60-65` widened it.
    pub const CAPACITY: usize = 1 + Attrs::WIRE.len() + 1 + 3 * 5;
    /// One position past the last storable parameter.
    ///
    /// [`ParamIdx::next_capped`] saturates here rather than at `CAPACITY`, so
    /// "sixteen parameters arrived" and "more than sixteen arrived" stay
    /// distinguishable. Pinning at `CAPACITY` is what let a seventeenth
    /// parameter's digits append to the sixteenth.
    pub const PAST_LAST: ParamIdx = ParamIdx((ParamIdx::CAPACITY + 1) as u8);
    pub fn next(self) -> ParamIdx {
        ParamIdx(self.0 + 1)
    }
    /// Advance, saturating one past capacity so it cannot run away.
    ///
    /// A sequence of nothing but separators would otherwise overflow the `u8`.
    pub fn next_capped(self) -> ParamIdx {
        ParamIdx(self.0.saturating_add(1).min(ParamIdx::PAST_LAST.0))
    }
    /// A parameter position.
    ///
    /// Saturating rather than casting: there are at most 16 parameters, so a
    /// larger index is malformed input and clamping it is what every other
    /// parameter path here already does.
    pub fn at(n: usize) -> ParamIdx {
        ParamIdx(n.min(u8::MAX as usize) as u8)
    }
    /// Step forward by however many parameters an attribute consumed.
    pub fn advance(self, by: ParamCount) -> ParamIdx {
        ParamIdx(self.0.saturating_add(by.raw()))
    }
    pub fn plus(self, n: u8) -> ParamIdx {
        ParamIdx(self.0 + n)
    }
}

/// How many parameters an SGR attribute consumed.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamCount(pub(crate) u8);

impl ParamCount {
    pub fn get(self) -> ParamIdx {
        ParamIdx(self.0)
    }
    pub(crate) fn raw(self) -> u8 {
        self.0
    }
    /// At least one, so a malformed attribute cannot stall the SGR loop.
    pub fn at_least_one_count(self) -> ParamCount {
        ParamCount(self.0.max(1))
    }
}

/// A CSI parameter, accumulated digit by digit. Saturates rather than wrapping,
/// because `CSI 99999999999 H` is malformed input, not an overflow bug.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Param(pub(crate) u16);

impl Param {
    pub const ZERO: Param = Param(0);
    pub fn push(self, d: Digit) -> Param {
        Param(self.0.saturating_mul(10).saturating_add(d.0 as u16))
    }
}

/// Whether DECLRMM is in force, which decides how `CSI s` is read.
///
/// `CSI s` is SCOSC normally and DECSLRM under DECLRMM. Passing that as a
/// `bool` to a parser is precisely the "two quantities confused" shape these
/// types exist to prevent -- there is another `bool` in the same call
/// (`is_none()` on the intermediate), and nothing would catch them swapped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MarginMode {
    #[default]
    Off,
    On,
}

/// Whether a CSI carried the `?` private marker. A `bool` argument at a call
/// site says nothing; this says which namespace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    /// `>` — the secondary namespace, used by XTVERSION and secondary DA.
    /// Distinct from `?`: `CSI > 0 q` and `CSI ? 0 q` are unrelated.
    Gt,
    Standard,
    Private,
    /// `<` — `[[source: williams]]` collects `3C-3F` in `csi_entry`, and this
    /// is one of the two baud does not act on.
    ///
    /// Collected rather than discarded so the sequence reaches dispatch and is
    /// counted in `Term::unsupported`. Sending it to `csi_ignore` instead would
    /// consume it silently, which is the *parsed and dropped* failure
    /// `docs/vt-coverage.md` exists to catch: the application is told nothing
    /// and cannot find out.
    Lt,
    /// `=` — the other one. Same reasoning as [`Marker::Lt`].
    Equals,
}

impl Marker {
    /// The private-namespace bytes, `3C-3F`.
    ///
    /// `[[source: williams]]` collects these in `csi_entry` and **only** there:
    /// `csi_param` sends the same bytes to `csi_ignore`, because a marker is
    /// legal only immediately after `CSI`. Both of baud's feed paths call this,
    /// so the rule has one definition and the two paths cannot drift.
    pub fn from_private(b: StreamByte) -> Option<Marker> {
        Some(match b.raw_byte() {
            b'<' => Marker::Lt,
            b'=' => Marker::Equals,
            b'>' => Marker::Gt,
            b'?' => Marker::Private,
            _ => return None,
        })
    }
}

/// CSI parameters as accumulated, with typed accessors. Positional indexing
/// into a bare slice is how `38;2;r;g;b` gets misread.
pub struct Params<'a> {
    v: &'a [Option<Param>],
    /// Bit `i` is set when parameter `i` was separated from `i-1` by a **colon**
    /// rather than a semicolon -- that is, when it is a sub-parameter of the
    /// same attribute.
    ///
    /// `[[source: ctlseqs]]`: "xterm allows either colons (standard) or
    /// semicolons (legacy) to separate the subparameters". The distinction is
    /// not cosmetic: the colon form of direct colour carries a colour-space
    /// identifier that the semicolon form does not, so `38:2:Pi:r:g:b` has six
    /// sub-parameters where `38;2;r;g;b` has five. A parser that flattens
    /// colons into semicolons reads the `Pi` as red and shifts every channel.
    sub: SubParams,
}

impl<'a> Params<'a> {
    pub fn new(p: &'a [Option<Param>]) -> Params<'a> {
        Params {
            v: p,
            sub: SubParams::NONE,
        }
    }
    /// With the sub-parameter mask, as the CSI parser accumulated it.
    pub fn with_subs(p: &'a [Option<Param>], sub: SubParams) -> Params<'a> {
        Params { v: p, sub }
    }
    /// Is parameter `i` joined to the one before it by a colon?
    pub fn is_sub(&self, i: ParamIdx) -> bool {
        self.sub.is_sub(i)
    }
    /// Length of the colon-joined group that starts at `i`, counting `i`.
    ///
    /// `38:2:0:1:2:3` from index 0 is 6; a bare `38` is 1.
    pub fn group_len(&self, i: ParamIdx) -> u8 {
        let mut n: u8 = 1;
        while (i.0 + n) < 16
            && ((i.0 + n) as usize) < self.v.len()
            && self.is_sub(ParamIdx(i.0 + n))
        {
            n += 1;
        }
        n
    }
    /// Drop the first `k` parameters, keeping the mask aligned.
    pub fn skip(&self, k: ParamIdx) -> Params<'a> {
        Params {
            v: &self.v[k.raw_index().min(self.v.len())..],
            sub: self.sub.drop_first(k),
        }
    }
    pub(crate) fn at(&self, i: ParamIdx) -> Option<Param> {
        self.v.get(i.0 as usize).copied().flatten()
    }
    /// How many parameters arrived.
    ///
    /// Needed because two different operations share the final byte `T` and
    /// are told apart only by *arity*: one parameter is SD, five are XTHIMOUSE.
    /// One parameter as a raw number, with a default when it is absent.
    ///
    /// For operations whose parameters are coordinates rather than counts or
    /// enumerations -- XTHIMOUSE's `func;startx;starty;firstrow;lastrow` --
    /// where the typed accessors would be the wrong shape.
    pub fn raw(&self, i: ParamIdx, default: u16) -> u16 {
        self.v
            .get(i.raw_index())
            .copied()
            .flatten()
            .map_or(default, |p| p.0)
    }
    pub fn len(&self) -> usize {
        self.v.len()
    }
    pub fn is_empty(&self) -> bool {
        self.v.is_empty()
    }
    pub fn count(&self, i: ParamIdx) -> Count {
        Count::from_param(self.at(i))
    }
    pub fn one_based(&self, i: ParamIdx) -> OneBased {
        OneBased::from_param(self.at(i))
    }
    pub fn extent(&self, i: ParamIdx) -> Option<EraseExtent> {
        EraseExtent::from_param(self.at(i))
    }
    pub fn mode(&self, i: ParamIdx) -> PrivateMode {
        PrivateMode::from_wire(self.at(i))
    }
    pub fn count_of(&self) -> ParamCount {
        ParamCount(self.v.len().min(255) as u8)
    }
}

/// `CSI J` / `CSI K` selector. The wire encodes 0/1/2; anything else is an
/// error, not a silent `ToEnd`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EraseExtent {
    ToEnd,
    ToStart,
    All,
}

/// A half-open span of columns within one row, `[from, to)`.
///
/// A pair of `Col`s rather than two loose arguments: `fill(r, from, to)` and
/// `fill(r, to, from)` both compile and one of them is silently empty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColSpan {
    pub from: Col,
    pub to: Col,
}

/// A half-open span of rows, `[from, to)`. Same reasoning as [`ColSpan`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowSpan {
    pub from: Row,
    pub to: Row,
}

impl EraseExtent {
    pub fn from_param(p: Option<Param>) -> Option<EraseExtent> {
        match p.map_or(0, |x| x.0) {
            0 => Some(EraseExtent::ToEnd),
            1 => Some(EraseExtent::ToStart),
            2 => Some(EraseExtent::All),
            _ => None,
        }
    }

    /// The columns an erase covers **within a single row**, for EL and DECSEL.
    ///
    /// `[[source: ecma48]]` § 8.3.40 (EL): `Ps = 0` erases from the active
    /// position to the end of the line, `Ps = 1` from the beginning up to *and
    /// including* it, `Ps = 2` the whole line. The "and including" is why
    /// `ToStart` ends at `cx.after()` and not at `cx`.
    ///
    /// The clamp is not decoration. With autowrap pending, the cursor legally
    /// sits one past the last column — the deferred-wrap sentinel — so
    /// `cx.after()` can be `cols + 1`.
    pub fn line_span(self, cx: Col, cols: Cols) -> ColSpan {
        let end = Col::at_index(cols.index());
        match self {
            EraseExtent::ToEnd => ColSpan { from: cx, to: end },
            EraseExtent::ToStart => ColSpan {
                from: Col::ZERO,
                to: Col::at_index(cx.after().index().min(cols.index())),
            },
            EraseExtent::All => ColSpan {
                from: Col::ZERO,
                to: end,
            },
        }
    }

    /// The rows an erase covers **in full**, for ED and DECSED.
    ///
    /// `[[source: ecma48]]` § 8.3.39: the split is at the *cursor*, not at a
    /// row boundary, so the cursor's own row is not in here for `ToEnd` or
    /// `ToStart` — [`EraseExtent::cursor_span`] has it.
    pub fn full_rows(self, cy: Row, rows: Rows) -> RowSpan {
        let end = Row::from_index(rows.index());
        match self {
            EraseExtent::ToEnd => RowSpan {
                from: cy.after(),
                to: end,
            },
            EraseExtent::ToStart => RowSpan {
                from: Row::ZERO,
                to: cy,
            },
            EraseExtent::All => RowSpan {
                from: Row::ZERO,
                to: end,
            },
        }
    }

    /// The part of the **cursor's own row** an erase covers, for ED and DECSED.
    ///
    /// `None` for `All`, where the cursor's row is already covered in full by
    /// [`EraseExtent::full_rows`] and erasing it twice would be the same cells
    /// written a second time.
    ///
    /// ED erasing whole rows — ignoring this split — destroyed the text to the
    /// left of the cursor on `CSI 0 J`, text the application had every reason
    /// to believe was safe. It survived every existing ED test because they all
    /// erased from column 0.
    pub fn cursor_span(self, cx: Col, cols: Cols) -> Option<ColSpan> {
        match self {
            EraseExtent::All => None,
            _ => Some(self.line_span(cx, cols)),
        }
    }
}

/// A CSI sequence, resolved.
///
/// `Unsupported` is a variant rather than a `_ => {}` arm on purpose: the set of
/// operations not yet implemented becomes *enumerable*, so the differential test
/// can report which finals are being dropped instead of silently diverging from
/// a real terminal.
pub enum Csi {
    CursorUp(Count),
    CursorDown(Count),
    CursorForward(Count),
    CursorBack(Count),
    CursorPosition {
        row: OneBased,
        col: OneBased,
    },
    EraseInDisplay(EraseExtent),
    EraseInLine(EraseExtent),
    InsertLines(Count),
    DeleteLines(Count),
    DeleteChars(Count),
    EraseChars(Count),
    InsertBlanks(Count),
    SetScrollRegion {
        top: OneBased,
        /// `None` when the parameter was omitted, which means the last row --
        /// bare `CSI r` resets the region to the full screen. `OneBased`
        /// cannot carry that: `from_param` clamps an absent parameter to 1,
        /// making it indistinguishable from an explicit `CSI 1;1 r`.
        bottom: Option<OneBased>,
    },
    /// DECSCA, `CSI Ps " q`. Whether following characters may be erased by
    /// DECSED/DECSEL. `[[source: vt510-decsca]]`: 1 means they cannot; 0 and 2
    /// mean they can.
    SelectProtection {
        protect: Protection,
    },
    /// DECSED, `CSI ? Ps J`. Erase in display, erasable characters only.
    SelectiveEraseDisplay(EraseExtent),
    /// DECSEL, `CSI ? Ps K`. Erase in line, erasable characters only.
    SelectiveEraseLine(EraseExtent),
    /// REP, `CSI b`. Repeat the preceding graphic character.
    Repeat(Count),
    /// DA, `CSI c`. Primary Device Attributes: "what are you?".
    DeviceAttributes,
    /// DSR, `CSI n`. `[[source: ctlseqs]]` defines 5 (status) and 6 (CPR); any
    /// other value is a report baud does not produce, and answering it with the
    /// wrong shape is worse than not answering at all.
    DeviceStatus(Option<StatusKind>),
    /// DECSTR, `CSI ! p`. Soft reset: modes, region, cursor and attributes to
    /// their defaults, but **not** the screen contents -- that is what makes it
    /// soft, and what distinguishes it from RIS.
    SoftReset,
    /// CHT, `CSI I`. Forward `n` tab stops.
    CursorForwardTab(Count),
    /// CBT, `CSI Z`. Backward `n` tab stops.
    CursorBackTab(Count),
    /// TBC, `CSI g`. `None` for the values xterm does not implement.
    TabClear(Option<TabClear>),
    /// CNL, `CSI E`. Down `n` lines, to column 1.
    CursorNextLine(Count),
    /// CPL, `CSI F`. Up `n` lines, to column 1.
    CursorPrevLine(Count),
    /// CHA `CSI G` and HPA `CSI \``: absolute column, row unchanged. ECMA-48
    /// gives them separate names; they are the same operation.
    ColumnAbsolute(OneBased),
    /// VPA, `CSI d`. Absolute row, column unchanged.
    LineAbsolute(OneBased),
    /// HPR, `CSI a`. Right `n` columns -- CUF by another name.
    ColumnRelative(Count),
    /// VPR, `CSI e`. Down `n` rows, column unchanged. Unlike CNL.
    LineRelative(Count),
    /// SCOSC, `CSI s`. Ambiguous with DECSLRM when DECLRMM is enabled; baud
    /// does not implement left/right margins, so this is unambiguous here.
    SaveCursor,
    /// SCORC, `CSI u`.
    RestoreCursor,
    /// SU, `CSI S`. Scroll the scrolling region up, cursor unmoved.
    ScrollUp(Count),
    /// SD, `CSI T`. Scroll the scrolling region down, cursor unmoved.
    ScrollDown(Count),
    /// SM / RM, the unprefixed mode namespace.
    Mode {
        change: ModeChange,
        mode: AnsiMode,
    },
    Sgr,
    /// DECSCUSR, `CSI Ps SP q`. Cursor shape and blink.
    CursorStyle(CursorStyle),
    /// XTVERSION, `CSI > 0 q`. Answered with [`Report::Version`].
    RequestVersion,
    /// DECSLRM, `CSI Pl ; Pr s`.
    ///
    /// `[[source: vt510-decslrm]]`: "sets the left and right margins to define
    /// the scrolling region ... DECSLRM only works when vertical split screen
    /// mode (DECLRMM) is set." Without DECLRMM the same bytes are SCOSC, which
    /// is why this cannot be resolved from the sequence alone -- the parser is
    /// told which mode is in force.
    SetSideMargins {
        left: OneBased,
        right: OneBased,
    },
    /// DECIC, `CSI Pn ' }`. Insert columns within the margins.
    InsertColumns(Count),
    /// DECDC, `CSI Pn ' ~`. Delete columns within the margins.
    DeleteColumns(Count),
    /// DECRQM, `CSI Ps $ p` and `CSI ? Ps $ p`. A *question*, answered with
    /// [`Report::ModeStatus`].
    RequestMode {
        marker: Marker,
        mode: Param,
    },
    /// Private (`?`) sequences are a different namespace. `CSI 4 h` and
    /// `CSI ? 4 h` are unrelated operations; conflating them is a classic bug.
    PrivateMode {
        change: ModeChange,
        mode: PrivateMode,
    },
    /// `CSI Ps ; Ps ; Ps ; Ps ; Ps T` — XTHIMOUSE.
    ///
    /// Recognised so it cannot be mistaken for SD, and not acted on:
    /// highlighting a region needs a selection model baud does not have. The
    /// hazard `?1001` carries is xterm's rather than baud's — "all X events are
    /// ignored until the proper escape sequence is received" describes an event
    /// loop, and baud has none to block. What baud must not do is *scroll*.
    HighlightMouseTracking(HighlightRegion),
    Unsupported {
        marker: Marker,
        final_byte: FinalByte,
    },
}

impl Csi {
    /// `inter` is the intermediate byte, which is part of the operation's
    /// identity: `CSI ! p` is DECSTR and `CSI p` is not the same thing.
    pub fn parse(marker: Marker, inter: Option<StreamByte>, p: Params<'_>, f: FinalByte) -> Csi {
        Csi::parse_with(marker, inter, p, f, MarginMode::Off)
    }

    /// As [`Csi::parse`], but told whether DECLRMM is set.
    ///
    /// `CSI s` is SCOSC normally and DECSLRM when left/right margin mode is on.
    /// The ambiguity is in the protocol, not in this parser, and it cannot be
    /// resolved without the mode -- so the mode is a parameter rather than
    /// something the caller patches up afterwards.
    pub fn parse_with(
        marker: Marker,
        inter: Option<StreamByte>,
        p: Params<'_>,
        f: FinalByte,
        margins: MarginMode,
    ) -> Csi {
        if margins == MarginMode::On
            && marker == Marker::Standard
            && inter.is_none()
            && f.raw() == b's'
        {
            return Csi::SetSideMargins {
                left: p.one_based(ParamIdx::FIRST),
                right: p.one_based(ParamIdx::SECOND),
            };
        }
        Csi::parse_inner(marker, inter, p, f)
    }

    fn parse_inner(marker: Marker, inter: Option<StreamByte>, p: Params<'_>, f: FinalByte) -> Csi {
        // `<` and `=` open namespaces baud implements nothing in. Falling
        // through would read them against the *standard* table, so `CSI < c`
        // would answer a primary DA -- a reply in the wrong shape, which is
        // worse than silence and is the one thing this parser must never do.
        if marker == Marker::Lt || marker == Marker::Equals {
            return Csi::Unsupported {
                marker,
                final_byte: f,
            };
        }
        if let Some(i) = inter {
            return match (i.raw_byte(), f.raw()) {
                (b'!', b'p') => Csi::SoftReset,
                (b'"', b'q') => Csi::SelectProtection {
                    protect: Protection::from_param(p.at(ParamIdx::FIRST)),
                },
                // `CSI Ps SP q` -- the intermediate is a space, which is why
                // DECSCUSR and DECSCA can share a final byte without colliding.
                (b' ', b'q') => match CursorStyle::from_param(p.at(ParamIdx::FIRST)) {
                    Some(c) => Csi::CursorStyle(c),
                    None => Csi::Unsupported {
                        marker,
                        final_byte: f,
                    },
                },
                // `[[source: vt510-decic]]` / `[[source: vt510-decdc]]`.
                (b'\'', b'}') => Csi::InsertColumns(p.count(ParamIdx::FIRST)),
                (b'\'', b'~') => Csi::DeleteColumns(p.count(ParamIdx::FIRST)),
                (b'$', b'p') => Csi::RequestMode {
                    marker,
                    mode: p.at(ParamIdx::FIRST).unwrap_or(Param::ZERO),
                },
                _ => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            };
        }
        // `CSI > Ps q` -- the `>` is a secondary marker, distinct from `?`.
        if marker == Marker::Gt && f.raw() == b'q' {
            return match p.at(ParamIdx::FIRST).map_or(0, |x| x.0) {
                0 => Csi::RequestVersion,
                _ => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            };
        }
        if marker == Marker::Gt {
            // Every other `>` final. **Not a fallthrough to the standard
            // table**, which is what it was: `CSI > 1 n` was read as `CSI 1 n`
            // and answered as a DSR, and `CSI > 1 s` as SCOSC. That is the
            // wrong-namespace failure the `Lt`/`Equals` guard above already
            // prevents -- `>` had the same hole and kept it because one final
            // in the namespace *is* implemented, so the guard looked present.
            return Csi::Unsupported {
                marker,
                final_byte: f,
            };
        }
        if marker == Marker::Private {
            return match f.raw() {
                b'h' => Csi::PrivateMode {
                    change: ModeChange::Set,
                    mode: p.mode(ParamIdx::FIRST),
                },
                b'l' => Csi::PrivateMode {
                    change: ModeChange::Reset,
                    mode: p.mode(ParamIdx::FIRST),
                },
                b'J' => match p.extent(ParamIdx::FIRST) {
                    Some(e) => Csi::SelectiveEraseDisplay(e),
                    None => Csi::Unsupported {
                        marker,
                        final_byte: f,
                    },
                },
                b'K' => match p.extent(ParamIdx::FIRST) {
                    Some(e) => Csi::SelectiveEraseLine(e),
                    None => Csi::Unsupported {
                        marker,
                        final_byte: f,
                    },
                },
                _ => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            };
        }
        match f.raw() {
            b'A' => Csi::CursorUp(p.count(ParamIdx::FIRST)),
            b'B' => Csi::CursorDown(p.count(ParamIdx::FIRST)),
            b'C' => Csi::CursorForward(p.count(ParamIdx::FIRST)),
            b'D' => Csi::CursorBack(p.count(ParamIdx::FIRST)),
            b'H' | b'f' => Csi::CursorPosition {
                row: p.one_based(ParamIdx::FIRST),
                col: p.one_based(ParamIdx::SECOND),
            },
            b'J' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInDisplay(e),
                None => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            },
            b'K' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInLine(e),
                None => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            },
            b'L' => Csi::InsertLines(p.count(ParamIdx::FIRST)),
            b'b' => Csi::Repeat(p.count(ParamIdx::FIRST)),
            b'c' => Csi::DeviceAttributes,
            b'n' => Csi::DeviceStatus(StatusKind::from_param(p.at(ParamIdx::FIRST))),
            b'I' => Csi::CursorForwardTab(p.count(ParamIdx::FIRST)),
            b'Z' => Csi::CursorBackTab(p.count(ParamIdx::FIRST)),
            b'g' => Csi::TabClear(TabClear::from_param(p.at(ParamIdx::FIRST))),
            b'E' => Csi::CursorNextLine(p.count(ParamIdx::FIRST)),
            b'F' => Csi::CursorPrevLine(p.count(ParamIdx::FIRST)),
            b'G' | b'`' => Csi::ColumnAbsolute(p.one_based(ParamIdx::FIRST)),
            b'd' => Csi::LineAbsolute(p.one_based(ParamIdx::FIRST)),
            b'a' => Csi::ColumnRelative(p.count(ParamIdx::FIRST)),
            b'e' => Csi::LineRelative(p.count(ParamIdx::FIRST)),
            b's' => Csi::SaveCursor,
            b'u' => Csi::RestoreCursor,
            b'S' => Csi::ScrollUp(p.count(ParamIdx::FIRST)),
            // `[[source: ctlseqs]]` gives `T` to two operations and separates
            // them by arity: `CSI Ps T` is SD, and
            // `CSI Ps ; Ps ; Ps ; Ps ; Ps T` is "Initiate highlight mouse
            // tracking (XTHIMOUSE)". Reading the five-parameter form as SD
            // *scrolled the screen* when an application asked to highlight --
            // a visible corruption, and a silent one.
            b'T' if p.len() >= 5 => Csi::HighlightMouseTracking(HighlightRegion {
                func: p.raw(ParamIdx::FIRST, 0),
                start_x: p.raw(ParamIdx::FIRST.plus(1), 1),
                start_y: p.raw(ParamIdx::FIRST.plus(2), 1),
                first_row: p.raw(ParamIdx::FIRST.plus(3), 1),
                last_row: p.raw(ParamIdx::FIRST.plus(4), 1),
            }),
            b'T' => Csi::ScrollDown(p.count(ParamIdx::FIRST)),
            b'h' => Csi::Mode {
                change: ModeChange::Set,
                mode: AnsiMode::from_wire(p.at(ParamIdx::FIRST)),
            },
            b'l' => Csi::Mode {
                change: ModeChange::Reset,
                mode: AnsiMode::from_wire(p.at(ParamIdx::FIRST)),
            },
            b'M' => Csi::DeleteLines(p.count(ParamIdx::FIRST)),
            b'P' => Csi::DeleteChars(p.count(ParamIdx::FIRST)),
            b'X' => Csi::EraseChars(p.count(ParamIdx::FIRST)),
            b'@' => Csi::InsertBlanks(p.count(ParamIdx::FIRST)),
            b'r' => Csi::SetScrollRegion {
                top: p.one_based(ParamIdx::FIRST),
                bottom: p
                    .at(ParamIdx::SECOND)
                    .map(|v| OneBased::from_param(Some(v))),
            },
            b'm' => Csi::Sgr,
            _ => Csi::Unsupported {
                marker,
                final_byte: f,
            },
        }
    }
}

/// TBC, `CSI g`. ECMA-48 defines six values; `[[source: ctlseqs]]` notes xterm
/// implements 0 and 3, which are the two any application sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabClear {
    /// 0: the stop at the cursor.
    AtCursor,
    /// 3: every stop.
    All,
}

impl TabClear {
    pub fn from_param(p: Option<Param>) -> Option<TabClear> {
        match p.map_or(0, |x| x.0) {
            0 => Some(TabClear::AtCursor),
            3 => Some(TabClear::All),
            _ => None,
        }
    }
}

impl Param {
    /// Sentinel recorded in the last parameter slot when `?` was seen.
    pub const PRIVATE_FLAG: Param = Param(u16::MAX);
}

impl ParamIdx {
    pub fn raw_index(self) -> usize {
        self.0 as usize
    }
    /// At least one parameter always exists, even when omitted: `CSI m` is
    /// `CSI 0 m`. This states that once instead of `.max(1)` at each call site.
    pub fn count_at_least_one(self) -> usize {
        (self.0 as usize).max(1)
    }
    pub fn is_first(self) -> bool {
        self.0 == 0
    }
}

impl ParamCount {
    pub fn get_raw(self) -> u8 {
        self.0
    }
    /// Every SGR attribute consumes at least one parameter; a zero here would
    /// loop forever. Stated once rather than as `.max(1)` at the call site.
    pub fn at_least_one(self) -> usize {
        (self.0 as usize).max(1)
    }
}

/// Which protocol level introduced an operation.
///
/// **Data, not structure.** `Csi` stays one flat enum rather than nesting per
/// level, and the reason is the parser: dispatch keys on
/// `(marker, intermediate, final byte)`, which does not partition by level.
/// `CSI J` is VT100 and `CSI ? J` is VT220 — same final byte. `CSI s` is worse:
/// it is SCOSC *or* DECSLRM, two operations at two levels, told apart at
/// runtime by whether DECLRMM is set. Nesting would force it into one arbitrary
/// home and split the dispatch in two for a distinction the terminal never
/// branches on.
///
/// The one place a protocol *does* change behaviour is VT52, whose bytes
/// conflict rather than extend — and VT52 is split out, into its own type, its
/// own module and its own ground state.
///
/// So the level lives here, where it can be queried and tested against
/// `docs/vt-coverage.md`'s tiers, without reshaping the code around a
/// historical fact.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Level {
    /// VT100 and ECMA-48: the set every terminal must have.
    Vt100,
    /// VT220/VT320: selective erase, status reports, DCS.
    Vt220,
    /// VT420: left and right margins, column operations.
    Vt420,
    /// xterm extensions in universal use.
    Xterm,
}

impl Csi {
    /// The protocol level this operation belongs to.
    ///
    /// `Unsupported` has none — it is the absence of an operation.
    pub fn level(self) -> Option<Level> {
        use Csi::*;
        Some(match self {
            CursorUp(_)
            | CursorDown(_)
            | CursorForward(_)
            | CursorBack(_)
            | CursorNextLine(_)
            | CursorPrevLine(_)
            | ColumnAbsolute(_)
            | LineAbsolute(_)
            | ColumnRelative(_)
            | LineRelative(_)
            | CursorPosition { .. }
            | SaveCursor
            | RestoreCursor
            | EraseInDisplay(_)
            | EraseInLine(_)
            | EraseChars(_)
            | InsertBlanks(_)
            | DeleteChars(_)
            | InsertLines(_)
            | DeleteLines(_)
            | ScrollUp(_)
            | ScrollDown(_)
            | SetScrollRegion { .. }
            | CursorForwardTab(_)
            | CursorBackTab(_)
            | TabClear(_)
            | Mode { .. }
            | PrivateMode { .. }
            | Sgr
            | Repeat(_)
            | DeviceAttributes
            | DeviceStatus(_) => Level::Vt100,

            SoftReset
            | SelectProtection { .. }
            | SelectiveEraseDisplay(_)
            | SelectiveEraseLine(_)
            | RequestMode { .. }
            | CursorStyle(_) => Level::Vt220,

            SetSideMargins { .. } | InsertColumns(_) | DeleteColumns(_) => Level::Vt420,

            RequestVersion | HighlightMouseTracking(_) => Level::Xterm,

            Unsupported { .. } => return None,
        })
    }
}
