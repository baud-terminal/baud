use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

// `ParamIdx::PAST_LAST` casts the capacity into the index's `u8`. `Attrs` has
// free bits left, so `WIRE` can still grow and take `CAPACITY` with it -- and an
// unguarded cast would wrap rather than fail. `SubParams` carries the parallel
// assert against `u64::BITS`; neither constant may drift alone.
const _: () = assert!(ParamIdx::CAPACITY < u8::MAX as usize);

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

impl Digit {
    /// The value of an ASCII digit. **Infallible on purpose**: the parse graph
    /// only names a digit action for `0x30..=0x39`, so a caller that has one
    /// has already been told. Re-testing it there re-asks a settled question
    /// and produces a `None` arm that cannot be taken.
    pub const fn of_ascii(b: StreamByte) -> Digit {
        Digit(b.raw_byte().wrapping_sub(b'0') & 0x0f)
    }

    /// A digit read back out of a table entry that stored one.
    ///
    /// Masked rather than checked: the caller is a compile-time table whose
    /// entries were built from `of_ascii`, so there is no failing arm to take
    /// and an `Option` here would be one the graph has made unreachable.
    pub const fn of_value(v: u8) -> Digit {
        Digit(v & 0x0f)
    }

    pub const fn value(self) -> u8 {
        self.0
    }
}

/// Two decimal digits, `0..=99`, as one value.
///
/// **The thing a two-byte key can hand back that a byte-class key cannot.**
/// A class says "a digit"; the parameter needs "a 3", and the pair `38`
/// arrives as one lookup rather than two.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DigitPair(pub(crate) u8);

impl DigitPair {
    /// Only ever built from two bytes the caller has already established are
    /// ASCII digits, so there is no failing arm to take.
    pub const fn of(hi: Digit, lo: Digit) -> DigitPair {
        DigitPair(hi.0 * 10 + lo.0)
    }
    pub const fn value(self) -> u8 {
        self.0
    }

    /// A pair read back out of a table entry that stored one.
    pub const fn of_value(v: u8) -> DigitPair {
        DigitPair(v)
    }
}

impl Param {
    pub const ZERO: Param = Param(0);
    /// As an index or a count. The cast lives here because a newtype's job is
    /// to hold one; a caller doing `as usize` is a raw-integer site.
    pub fn index(self) -> usize {
        self.0 as usize
    }
    /// The value, or one, which is the default nearly every CSI parameter has.
    pub fn or_one(self) -> usize {
        match self.0 {
            0 => 1,
            n => n as usize,
        }
    }
    /// The value, for an encoder that has to write it out.
    ///
    /// `u16` because that is what a parameter *is* -- `push` saturates at
    /// 65535 -- and narrowing it here would silently truncate the very
    /// saturation the parser is careful to produce.
    pub fn value(self) -> u16 {
        self.0
    }
    pub fn push(self, d: Digit) -> Param {
        Param(self.0.saturating_mul(10).saturating_add(d.0 as u16))
    }

    /// A `Param` from a stored value. **For a test that has to start from an
    /// arbitrary accumulator**, and for nothing on the hot path.
    pub const fn from_raw(v: u16) -> Param {
        Param(v)
    }

    /// Fold in one or two digits at once: `p * m + v`, saturating.
    ///
    /// **The multiplier is data, not a branch.** One digit folds with 10 and
    /// two with 100, and a table entry carries which -- so the per-digit path
    /// is straight-line arithmetic rather than a data-dependent dispatch on
    /// how many digits arrived.
    pub fn saturating_scale(self, m: u16, v: u16) -> Param {
        Param(self.0.saturating_mul(m).saturating_add(v))
    }

    /// Two digits at once.
    ///
    /// **Identical to two `push`es, saturation included**: both forms reach
    /// 65535 and stay there, so a parameter accumulated a pair at a time
    /// cannot differ from one accumulated a digit at a time. `the_pair_form_
    /// agrees_with_two_pushes` checks every value against every pair.
    pub fn push_pair(self, d: DigitPair) -> Param {
        Param(self.0.saturating_mul(100).saturating_add(d.0 as u16))
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
    ///
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
    /// The private-namespace bytes, `3C-3F`, and their GR spellings `BC-BF`.
    ///
    /// `[[source: williams]]` collects these in `csi_entry` and **only** there:
    /// `csi_param` sends the same bytes to `csi_ignore`, because a marker is
    /// legal only immediately after `CSI`. Both of baud's feed paths call this,
    /// so the rule has one definition and the two paths cannot drift.
    ///
    /// **`BC-BF` are the same markers in GR**, which under ISO 8859-1 a
    /// parser reads as their 7-bit bytes, williams' "In all states, these
    /// codes are treated identically to GL codes 20-7F" (462). Named here
    /// rather than masked by the caller, whose arm is inlined into a per-byte
    /// loop; `Act::Inter`, which stores its byte and has no match to widen,
    /// masks instead. Under UTF-8 no parser names a marker for either.
    pub const fn from_private(b: StreamByte) -> Option<Marker> {
        Some(match b.raw_byte() {
            b'<' | 0xbc => Marker::Lt,
            b'=' | 0xbd => Marker::Equals,
            b'>' | 0xbe => Marker::Gt,
            b'?' | 0xbf => Marker::Private,
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
    ///
    /// **An explicit early-out for the empty mask is measured flat.** A colour
    /// introducer runs this on every `SGR 38`, `48` and `58`, and the
    /// semicolon form -- which is what every recording sends -- has no
    /// sub-parameters, so guarding on `is_plain` looked like skipping a loop.
    /// It is not: the loop's first test is one mask AND against a zero word,
    /// which is the same work as the guard, so the guard only adds a branch.
    /// btop 936.5 to 928.6, git-log 1862 to 1831, the rest flat.
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
    /// The same parameters again, by value.
    ///
    /// `Params` holds a borrow and a mask and is deliberately not `Copy` -- the
    /// type exists so a positional read cannot be spelled twice by accident.
    /// A probe that needs to run a parse twice needs the same object without
    /// cloning the data, and that is exactly this: two words, no allocation,
    /// nothing the real path does not already do when it constructs one.
    #[cfg(any(
        feature = "probe-double-sgr-color",
        feature = "probe-sgr-color-reborrow"
    ))]
    pub(crate) fn reborrow(&self) -> Params<'a> {
        Params {
            v: self.v,
            sub: self.sub,
        }
    }
    pub(crate) fn at(&self, i: ParamIdx) -> Option<Param> {
        self.v.get(i.0 as usize).copied().flatten()
    }
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
    /// How many parameters arrived.
    ///
    /// Needed because two different operations share the final byte `T` and
    /// are told apart only by *arity*: one parameter is SD, five are XTHIMOUSE.
    /// XTWINOPS leans on it too: `CSI 14 t` is the text area and
    /// `CSI 14 ; 2 t` the window.
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

/// An XTWINOPS operation, `CSI Ps ; Ps ; Ps t`: one of the three reports
/// baud answers, or the rest of the family.
///
/// `[[source: ctlseqs]]`, "Window manipulation (XTWINOPS)": `Ps = 1 4` "Report
/// xterm text area size in pixels. Result is CSI 4 ; height ; width t",
/// `Ps = 1 6` "Report xterm character cell size in pixels. Result is CSI 6 ;
/// height ; width t", and `Ps = 1 8` "Report the size of the text area in
/// characters. Result is CSI 8 ; height ; width t".
///
/// **Every other form is recognised and refused**, which is why it is a
/// variant and not `Csi::Unsupported`: the operation is known, and what it
/// asks -- move, resize, iconify or report the *window*, a thing this core is
/// never handed, or push the title -- is not done. The core counts it as an
/// unsupported parameter, the way it counts a `DSR` it cannot answer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowOp {
    /// `CSI 14 t`, the text area in pixels. Not `CSI 14 ; 2 t`, which is the
    /// window including its frame, and nothing here knows the frame.
    TextAreaPixels,
    /// `CSI 16 t`, one cell in pixels.
    CellPixels,
    /// `CSI 18 t`, the text area in cells.
    TextAreaChars,
    /// Any other `Ps`, any extra parameter, or none at all.
    Refused,
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
    /// `CSI 3 J`, "Erase Saved Lines, xterm" (`[[source: ctlseqs]]`): the
    /// lines that scrolled off the top of the primary screen, and not the
    /// screen. A variant of its own rather than a fourth [`EraseExtent`],
    /// because EL and DECSEL share that selector and have no `3` -- widening
    /// it would read `CSI 3 K` as an erase.
    EraseSavedLines,
    /// `CSI ? 3 J`, "Selective Erase Saved Lines, xterm": the same lines,
    /// erasable characters only, as every DECSED is.
    SelectiveEraseSavedLines,
    /// REP, `CSI b`. Repeat the preceding graphic character.
    Repeat(Count),
    /// DA, `CSI c`. Primary Device Attributes: "what are you?".
    DeviceAttributes,
    /// DSR, `CSI n`. `[[source: ctlseqs]]` defines 5 (status) and 6 (CPR); any
    /// other value is a report baud does not produce, and answering it with the
    /// wrong shape is worse than not answering at all.
    DeviceStatus(Option<StatusKind>),
    /// `CSI ? 996 n`: the colour preference query of
    /// `[[source: contour-palette-notifications]]`, answered `CSI ? 997 ; 1 n`
    /// for dark and `; 2 n` for light. The only private `n` baud answers --
    /// `CSI ? 6 n`, DECXCPR, and the rest stay [`Csi::Unsupported`].
    ColorSchemeQuery,
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
    /// DECSCUSR, `CSI Ps SP q`. Cursor shape and blink, or the initial style.
    CursorStyle(CursorPick),
    /// XTVERSION, `CSI > 0 q`. Answered with [`Report::Version`].
    RequestVersion,
    /// XTMODKEYS, `CSI > 4 ; Pv m` -- `modifyOtherKeys`.
    ///
    /// **Only `Pp = 4` is represented, and only levels 0 to 2.**
    /// `[[source: ctlseqs]]` line 1281 lists seven resources and line 2241 a
    /// third level, and the ones this does not carry are `Unsupported` and
    /// counted rather than approximated -- a level rounded down to one the type
    /// can express is a mode recorded and not acted on, which this project
    /// rates worse than not implementing it.
    SetOtherKeys(OtherKeys),
    /// XTFMTKEYS, `CSI > 4 ; Pv f` -- `formatOtherKeys`.
    ///
    /// Which of the two sequence formats [`Csi::SetOtherKeys`] sends. Same
    /// restriction to `Pp = 4`, for the same reason.
    SetOtherKeysFormat(OtherKeysFormat),
    /// `[[source: kitty-keyboard]]` set, `CSI = flags ; mode u`.
    KittySet {
        flags: KittyFlags,
        mode: KittySetMode,
    },
    /// `[[source: kitty-keyboard]]` query, `CSI ? u` -- "The terminal will
    /// reply with: CSI ? flags u", which [`Report::KittyFlags`] encodes.
    /// Answering is the protocol's support claim, and the core makes it, since
    /// every enhancement is honoured.
    KittyQuery,
    /// `[[source: kitty-keyboard]]` push, `CSI > flags u` -- save the current
    /// flags on the stack and set them to `flags` (default zero).
    KittyPush {
        flags: KittyFlags,
    },
    /// `[[source: kitty-keyboard]]` pop, `CSI < number u` -- pop `number`
    /// entries (default one); emptying the stack resets all flags.
    KittyPop {
        count: Count,
    },
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
    /// XTSAVE, `CSI ? Pm s` (`[[source: ctlseqs]]`): "Save DEC Private Mode
    /// Values ... Ps values are the same as for DECSET." The `?` is what keeps
    /// it from SCOSC, `CSI s`, and from DECSLRM, which needs DECLRMM *and* the
    /// standard namespace; every parameter is a mode, read by the caller.
    SaveModes,
    /// XTRESTORE, `CSI ? Pm r`: "The value of Ps previously saved is
    /// restored." The `?` keeps it from DECSTBM, `CSI Ps ; Ps r`.
    RestoreModes,
    /// `CSI Ps ; Ps ; Ps ; Ps ; Ps T` — XTHIMOUSE.
    ///
    /// Recognised so it cannot be mistaken for SD, and not acted on:
    /// highlighting a region needs a selection model baud does not have. The
    /// hazard `?1001` carries is xterm's rather than baud's — "all X events are
    /// ignored until the proper escape sequence is received" describes an event
    /// loop, and baud has none to block. What baud must not do is *scroll*.
    HighlightMouseTracking(HighlightRegion),
    /// XTWINOPS, `CSI Ps ; Ps ; Ps t`. Three forms are *questions* about the
    /// text area's size, answered with [`Report::TextAreaPixels`],
    /// [`Report::CellPixels`] and [`Report::TextAreaChars`]; the rest are
    /// [`WindowOp::Refused`].
    Window(WindowOp),
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
    /// **`#[inline]` so the construct-and-match pair can collapse.** This
    /// builds a 56-variant enum that `baud-core`'s `csi_final` destructures
    /// immediately -- two dispatches on the same final byte where one would
    /// do. Across a call boundary LLVM cannot see that; inlined it can thread
    /// the construction straight into the caller's arms. The caller is
    /// `#[cold] #[inline(never)]`, so nothing here reaches the per-byte loop.
    #[inline]
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

    #[inline]
    fn parse_inner(marker: Marker, inter: Option<StreamByte>, p: Params<'_>, f: FinalByte) -> Csi {
        // `<` and `=` open namespaces baud implements nothing in. Falling
        // through would read them against the *standard* table, so `CSI < c`
        // would answer a primary DA -- a reply in the wrong shape, which is
        // worse than silence and is the one thing this parser must never do.
        if marker == Marker::Lt || marker == Marker::Equals {
            // `[[source: kitty-keyboard]]`: `CSI = flags ; mode u` sets the
            // enhancement flags and `CSI < number u` pops the stack. Every
            // other `<`/`=` final is a namespace baud implements nothing in.
            if f.raw() == b'u' {
                return match marker {
                    Marker::Equals => Csi::KittySet {
                        flags: KittyFlags::from_param(p.at(ParamIdx::FIRST)),
                        mode: KittySetMode::from_param(p.at(ParamIdx::SECOND)),
                    },
                    _ => Csi::KittyPop {
                        count: Count::from_param(p.at(ParamIdx::FIRST)),
                    },
                };
            }
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
        // XTMODKEYS and XTFMTKEYS. `[[source: ctlseqs]]` lines 1271 and 795:
        // both are `CSI > Pp ; Pv <final>`, `Pp` naming one of seven resources
        // and `Pv` its value, and an omitted `Pv` resets the resource.
        //
        // **`Pp = 4` only.** The other six name resources that decide how the
        // cursor, function and keypad keys carry modifiers, and baud sends
        // those from `[[source: ctlseqs]]`'s own tables rather than from a
        // resource -- so accepting them would be recording a mode with nothing
        // reading it. They fall to `Unsupported` and are counted.
        if marker == Marker::Gt && matches!(f.raw(), b'm' | b'f') {
            let pp = p.at(ParamIdx::FIRST).map_or(0, |x| x.0);
            let pv = p.at(ParamIdx::SECOND).map_or(0, |x| x.0);
            if pp == 4 {
                let got = match f.raw() {
                    b'm' => match pv {
                        // An omitted `Pv` resets the resource, and `0` is the
                        // initial value, so both land here.
                        0 => Some(Csi::SetOtherKeys(OtherKeys::Off)),
                        1 => Some(Csi::SetOtherKeys(OtherKeys::Level1)),
                        2 => Some(Csi::SetOtherKeys(OtherKeys::Level2)),
                        _ => None,
                    },
                    _ => match pv {
                        0 => Some(Csi::SetOtherKeysFormat(OtherKeysFormat::Xterm)),
                        1 => Some(Csi::SetOtherKeysFormat(OtherKeysFormat::CsiU)),
                        _ => None,
                    },
                };
                if let Some(c) = got {
                    return c;
                }
            }
            return Csi::Unsupported {
                marker,
                final_byte: f,
            };
        }
        // `[[source: kitty-keyboard]]` push, `CSI > flags u`.
        if marker == Marker::Gt && f.raw() == b'u' {
            return Csi::KittyPush {
                flags: KittyFlags::from_param(p.at(ParamIdx::FIRST)),
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
                // `[[source: kitty-keyboard]]` query, `CSI ? u`.
                b'u' => Csi::KittyQuery,
                // XTSAVE and XTRESTORE. Not SCOSC or DECSLRM, not DECSTBM:
                // those are the standard namespace's `s` and `r`.
                b's' => Csi::SaveModes,
                b'r' => Csi::RestoreModes,
                b'n' if p.at(ParamIdx::FIRST).map_or(0, |x| x.0) == 996 => Csi::ColorSchemeQuery,
                b'J' => match p.extent(ParamIdx::FIRST) {
                    Some(e) => Csi::SelectiveEraseDisplay(e),
                    None if p.raw(ParamIdx::FIRST, 0) == 3 => Csi::SelectiveEraseSavedLines,
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
                None if p.raw(ParamIdx::FIRST, 0) == 3 => Csi::EraseSavedLines,
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
            // XTWINOPS. **Only the exact forms `[[source: ctlseqs]]` gives**,
            // one parameter each: `CSI 14 ; 2 t` is the *window* in pixels
            // and a second parameter on the other two is undefined, so both
            // fall to `Unsupported` rather than being read as the bare form --
            // an answer to a question that was not asked is the wrong shape.
            b't' => Csi::Window(match (p.len(), p.at(ParamIdx::FIRST).map_or(0, |x| x.0)) {
                (1, 14) => WindowOp::TextAreaPixels,
                (1, 16) => WindowOp::CellPixels,
                (1, 18) => WindowOp::TextAreaChars,
                _ => WindowOp::Refused,
            }),
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

            // XTWINOPS is "dtterm, extended by xterm" -- the three reports are
            // xterm's extensions of it.
            RequestVersion | HighlightMouseTracking(_) | Window(_) => Level::Xterm,
            // "Erase Saved Lines, xterm" in both forms: the saved lines are
            // xterm's, and no DEC level has them.
            EraseSavedLines | SelectiveEraseSavedLines => Level::Xterm,
            // XTSAVE and XTRESTORE are xterm's by name.
            SaveModes | RestoreModes => Level::Xterm,
            // XTMODKEYS and XTFMTKEYS are xterm's, and input-side: they change
            // what the keyboard sends rather than what the screen shows. The
            // kitty negotiation is the same shape -- input-side, and an
            // extension rather than a VT level.
            SetOtherKeys(_)
            | SetOtherKeysFormat(_)
            | KittySet { .. }
            | KittyQuery
            | KittyPush { .. }
            | KittyPop { .. } => Level::Xterm,
            // contour's extension, not xterm's; this table has no level of its
            // own for it and it is not a VT level either.
            ColorSchemeQuery => Level::Xterm,

            Unsupported { .. } => return None,
        })
    }
}

#[cfg(test)]
mod digit_pair {
    use super::*;

    /// The pair form and two single pushes must agree, **including where they
    /// saturate**. `p * 100 + 10a + b` and `((p * 10) + a) * 10 + b` are the
    /// same number until `Param`'s ceiling is reached, and this is the check
    /// that they are still the same afterwards.
    #[test]
    fn the_pair_form_agrees_with_two_pushes() {
        for p in [0u16, 1, 9, 65, 655, 6553, 6554, 65535] {
            for a in 0..10u8 {
                for b in 0..10u8 {
                    let (da, db) = (Digit(a), Digit(b));
                    assert_eq!(
                        Param(p).push_pair(DigitPair::of(da, db)),
                        Param(p).push(da).push(db),
                        "p={p} a={a} b={b}"
                    );
                }
            }
        }
    }
}
