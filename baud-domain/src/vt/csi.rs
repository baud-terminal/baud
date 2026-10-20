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
    ///
    /// **One of the two blink rates is not counted**: `SGR 5` and `6` replace
    /// each other ([`Attrs::BLINKS`]), so a reply holds one of them, and
    /// counting both would grow `Term::params` by a parameter no reply sends.
    /// Measured, the change with the array grown read `less-pager` 1.2% below
    /// the parent, ranges apart; with it held, the change overlaps the parent
    /// on five recordings (the record, "SGR 6 blinks at its own rate"). The
    /// underline family is still counted whole, as it was before it became
    /// exclusive, which bounds the reply from above by four parameters.
    pub const CAPACITY: usize =
        1 + Attrs::WIRE.len() - (Attrs::wire_in(Attrs::BLINKS) - 1) + 1 + 3 * 5;
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
    /// Whether parameter `i` arrived, given or omitted: `CSI ? 1 ; 1 ; S`
    /// has a third.
    pub fn has(&self, i: ParamIdx) -> bool {
        i.raw_index() < self.v.len()
    }
    pub fn count(&self, i: ParamIdx) -> Count {
        Count::from_param(self.at(i))
    }
    pub fn one_based(&self, i: ParamIdx) -> OneBased {
        OneBased::from_param(self.at(i))
    }
    /// A margin's far end: `None` when omitted or 0, which means the page's
    /// last column or line, and the line or column otherwise.
    ///
    /// **A 0 is the default, as an omitted parameter is.** `[[source:
    /// dec-std-070]]` DECSLRM: "default Pr: Last Column" (p. 5-27), and its
    /// algorithm (p. 5-28) reads the two alike, "if (parv[1] == 0) parv[1] =
    /// columns_per_page;"; DECSTBM: "default Pb: last line" (p. 5-25), and
    /// "if (parv[1] == 0) parv[1] = lines_per_page;" (p. 5-26).
    /// `[[source: vt100-ug-ch3]]` defines a default as "A function-dependent
    /// value that is assumed when no explicit value, or a value of 0, is
    /// specified". `[[source: vt510-decslrm]]` and `[[source: vt510-decstbm]]`
    /// default `Pr` and `Pb` to the page's width and height, and xterm
    /// (`CASE_ANSI_SC` and `CASE_DECSTBM`, charproc.c 4891-4894 and
    /// 4779-4782, `[[source: xterm-reset]]`) and ghostty
    /// (`setLeftAndRightMargin` and `setTopAndBottomMargin`, `[[source:
    /// ghostty-d9840f3-terminal]]` 2610 and 2596) read an omitted or 0 one as
    /// the last column or line. [`Self::one_based`] cannot carry it: it reads
    /// both as 1.
    pub fn far_margin(&self, i: ParamIdx) -> Option<OneBased> {
        self.at(i)
            .filter(|v| v.0 != 0)
            .map(|v| OneBased::from_param(Some(v)))
    }
    /// Is every parameter a default -- omitted, empty or 0 -- with no colon?
    ///
    /// For an operation that takes no parameter, which is what a defaulted
    /// one is. `[[source: msdos2-ansi-doc]]` note 1 (6-7): "The default value
    /// is used when no explicit value is given or a value of zero is given";
    /// `[[source: dec-std-070]]` § 3.5.3.1 (p. 3-24): "A zero length
    /// parameter, or one consisting only of 3/0 bit combinations, represents
    /// a default". A colon is not: there "the entire sequence up to and
    /// including the final character shall be ignored" (the same section).
    /// A bare `CSI s` arrives here as one omitted parameter, not none.
    pub fn all_default(&self) -> bool {
        self.sub == SubParams::NONE && self.v.iter().all(|v| v.is_none_or(|v| v.0 == 0))
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

/// Which erase a parameter list names, [`Csi::EraseEach`]'s: `CSI J`,
/// `CSI K`, `CSI ? J` or `CSI ? K` with more than one parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EraseList {
    Display,
    Line,
    SelectiveDisplay,
    SelectiveLine,
}

/// An XTWINOPS operation, `CSI Ps ; Ps ; Ps t` (`[[source: ctlseqs]]`,
/// "Window manipulation (XTWINOPS)"): a window operation the embedder
/// performs, a report baud answers, the title stack, a title report
/// refused, or a form ctlseqs does not give.
///
/// **Fieldless**, as [`Csi::Area`] is: the title stack's `Ps` and slot, and
/// a window operation's parameters ([`WindowRequest::of`]), are read where
/// the operation is performed, so the variant adds no payload to every
/// `CSI`.
///
/// **Only the forms ctlseqs gives are anything but [`WindowOp::Refused`]**:
/// `CSI 13 ; 2 t` and `CSI 14 ; 2 t` are the second forms of 13 and 14, any
/// other second parameter on a report is undefined, and an answer to a
/// question that was not asked is the wrong shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowOp {
    /// `CSI 1 t`, de-iconify.
    Deiconify,
    /// `CSI 2 t`, iconify.
    Iconify,
    /// `CSI 3 ; x ; y t`, move.
    Move,
    /// `CSI 4 ; height ; width t`, resize in pixels.
    ResizePixels,
    /// `CSI 5 t`, raise.
    Raise,
    /// `CSI 6 t`, lower.
    Lower,
    /// `CSI 7 t`, refresh.
    Refresh,
    /// `CSI 8 ; height ; width t`, resize in cells.
    ResizeCells,
    /// `CSI 9 ; Ps t`, `Ps` 0 to 3: maximise or restore.
    Maximize,
    /// `CSI 10 ; Ps t`, `Ps` 0 to 2: full screen.
    FullScreen,
    /// DECSLPP, `CSI Ps t` with `Ps` 24 and up: "Resize to Ps lines".
    Lines,
    /// `CSI 11 t`, "Report xterm window state": `CSI 1 t` open, `CSI 2 t`
    /// iconified.
    WindowState,
    /// `CSI 13 t`, "Report xterm window position": `CSI 3 ; x ; y t`.
    WindowPosition,
    /// `CSI 13 ; 2 t`, "Report xterm text-area position": `CSI 3 ; x ; y t`.
    TextAreaPosition,
    /// `CSI 14 t`, the text area in pixels.
    TextAreaPixels,
    /// `CSI 14 ; 2 t`, "Report xterm window size in pixels", the frame
    /// included: `CSI 4 ; height ; width t`.
    WindowPixels,
    /// `CSI 15 t`, "Report size of the screen in pixels": `CSI 5 ; height ;
    /// width t`.
    ScreenPixels,
    /// `CSI 16 t`, one cell in pixels.
    CellPixels,
    /// `CSI 18 t`, the text area in cells.
    TextAreaChars,
    /// `CSI 19 t`, "Report the size of the screen in characters": `CSI 9 ;
    /// height ; width t`.
    ScreenChars,
    /// `CSI 20 t` and `CSI 21 t`, the icon label and the window title
    /// reports. **Refused, and counted on their own**: the reply would put
    /// text a program chose back into the input stream, which a remote
    /// program can use to make the terminal type for it.
    TitleReport,
    /// `CSI 22 ; Ps t`, the title push; `Ps` 0, 1 or 2 and an optional slot,
    /// 1 to 10, read where it is performed.
    PushTitle,
    /// `CSI 23 ; Ps t`, the title pop, read as [`WindowOp::PushTitle`] is.
    PopTitle,
    /// Any other form: 12 and 17, an undefined parameter, a parameter more
    /// than ctlseqs gives, or none at all.
    Refused,
}

impl WindowOp {
    /// The form `p` spells, from its first two parameters' values and the
    /// list's length.
    fn of(p: &Params<'_>) -> WindowOp {
        let first = p.at(ParamIdx::FIRST).map_or(0, |x| x.0);
        let second = p.at(ParamIdx::FIRST.plus(1)).map_or(0, |x| x.0);
        match (first, p.len(), second) {
            // The window operations, each with the parameters ctlseqs gives
            // it and no more; an omitted second parameter is 0, xterm's
            // `zero_if_default`. **A `Ps` past ctlseqs' list is refused**,
            // 9 ; 4 and 10 ; 3 among them, where xterm's `RequestMaximize`
            // falls through to a restore: an answer to an operation that was
            // not asked is the wrong shape.
            (1, 1, _) => WindowOp::Deiconify,
            (2, 1, _) => WindowOp::Iconify,
            (3, 1..=3, _) => WindowOp::Move,
            (4, 1..=3, _) => WindowOp::ResizePixels,
            (5, 1, _) => WindowOp::Raise,
            (6, 1, _) => WindowOp::Lower,
            (7, 1, _) => WindowOp::Refresh,
            (8, 1..=3, _) => WindowOp::ResizeCells,
            (9, 1..=2, 0..=3) => WindowOp::Maximize,
            (10, 1..=2, 0..=2) => WindowOp::FullScreen,
            (24.., 1, _) => WindowOp::Lines,
            (11, 1, _) => WindowOp::WindowState,
            (13, 1, _) => WindowOp::WindowPosition,
            (13, 2, 2) => WindowOp::TextAreaPosition,
            (14, 1, _) => WindowOp::TextAreaPixels,
            (14, 2, 2) => WindowOp::WindowPixels,
            (15, 1, _) => WindowOp::ScreenPixels,
            (16, 1, _) => WindowOp::CellPixels,
            (18, 1, _) => WindowOp::TextAreaChars,
            (19, 1, _) => WindowOp::ScreenChars,
            (20 | 21, _, _) => WindowOp::TitleReport,
            // An omitted `Ps` is 0, both titles, as xterm reads it
            // (`zero_if_default`, `[[source: xterm-reset]]`'s file); the slot
            // is checked where it is read.
            (22, 1..=3, 0..=2) => WindowOp::PushTitle,
            (23, 1..=3, 0..=2) => WindowOp::PopTitle,
            _ => WindowOp::Refused,
        }
    }
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
        /// `None` when the parameter was omitted or 0, which means the last
        /// row -- bare `CSI r` resets the region to the full screen.
        /// `OneBased` cannot carry that: `from_param` clamps an absent
        /// parameter and a 0 to 1, making them indistinguishable from an
        /// explicit `CSI 1;1 r`. A 0 read as 1 until 2026-10-13, so
        /// `CSI 2;0 r` was dropped (see [`Params::far_margin`]).
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
    /// ED, EL, DECSED or DECSEL with more than one parameter: **every one,
    /// in order**, which the caller walks. `[[source: dec-std-070]]`'s four
    /// algorithms loop `for (n=0; n<parc; n++) switch(parv[n])` (pp. 5-155,
    /// 5-157, 5-160 and 5-163), and a value with no case is skipped. A lone
    /// parameter keeps the variants above.
    EraseEach(EraseList),
    /// EA, `CSI Ps O`, erase in the active qualified area
    /// (`[[source: ecma48]]` 8.3.37). The first parameter only: ECMA-48
    /// gives it `(Ps)` and no DEC text has it (decided by Jev, FIRST_ONLY,
    /// 0.90/0.90); the caller counts the rest.
    EraseInArea(EraseExtent),
    /// EF, `CSI Ps N`, erase in the active field (8.3.40), as EA reads its
    /// parameter.
    EraseInField(EraseExtent),
    /// REP, `CSI b`. Repeat the preceding graphic character.
    Repeat(Count),
    /// DA, `CSI c` or `CSI 0 c`. Primary Device Attributes: "what are
    /// you?". Any other first parameter is [`Csi::Unsupported`].
    DeviceAttributes,
    /// DSR, `CSI n`. `[[source: ctlseqs]]` defines 5 (status) and 6 (CPR); any
    /// other value is a report baud does not produce, and answering it with the
    /// wrong shape is worse than not answering at all.
    DeviceStatus(Option<StatusKind>),
    /// `CSI ? 996 n`: the colour preference query of
    /// `[[source: contour-palette-notifications]]`, answered `CSI ? 997 ; 1 n`
    /// for dark and `; 2 n` for light. The DEC private `n`s are
    /// [`Csi::Query`]'s, and a `Ps` neither names is [`Csi::Unsupported`].
    ColorSchemeQuery,
    /// A report answered out of line ([`QueryOp`]): the DEC private device
    /// status reports, DECREQTPARM, DECRQUPSS, DECRQDE, XTQMODKEYS and
    /// XTQFMTKEYS. **Its parameters are read where it is answered**, as
    /// [`Csi::Area`]'s are.
    Query(QueryOp),
    /// DECSTR, `CSI ! p`. Soft reset: modes, region, cursor and attributes to
    /// their defaults, but **not** the screen contents -- that is what makes it
    /// soft, and what distinguishes it from RIS.
    SoftReset,
    /// DECSCL, `CSI Pl ; Pc " p`: select the conformance level and, above
    /// Level 1, the C1 transmission. **Its parameters are read where it is
    /// performed**, as [`Csi::Area`]'s are.
    SetConformanceLevel,
    /// CHT, `CSI I`. Forward `n` tab stops.
    CursorForwardTab(Count),
    /// CBT, `CSI Z`. Backward `n` tab stops.
    CursorBackTab(Count),
    /// TBC, `CSI g`: `[[source: ecma48]]` 8.3.154's six values, `None` for
    /// any other.
    TabClear(Option<TabClear>),
    /// CTC, `CSI Ps... W` (`[[source: ecma48]]` 8.3.17, "Notation:
    /// (Ps...)"): sets and clears stops of both kinds,
    /// every parameter in order, which the caller walks through
    /// [`TabControl::from_param`].
    CursorTabControl,
    /// CVT, `CSI Pn Y` (8.3.23): to the line of the n-th following line
    /// tabulation stop, the column kept.
    CursorLineTab(Count),
    /// DECST8C, `CSI ? 5 W` (`[[source: vt510-decst8c]]`): "Set a tab stop at
    /// every eight columns starting with column 9 ... Any tab stop setting
    /// before this command is executed is cleared automatically." Only the
    /// one form the texts give; `CSI ? W` and any other value are
    /// [`Csi::Unsupported`] (Jev's lean, FIVE_ONLY, 0.74/0.90).
    TabsEveryEight,
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
    /// HPB, `CSI j` (`[[source: ecma48]]` 8.3.58): left `n` columns, HPR's
    /// mirror.
    ColumnBackward(Count),
    /// VPB, `CSI k` (8.3.159): up `n` rows, column unchanged, VPR's mirror.
    LineBackward(Count),
    /// SCOSC, `CSI s`, with DECLRMM reset: under DECLRMM the same bytes are
    /// [`Csi::SetSideMargins`]. Every parameter defaulted or none; one that
    /// is not is DECSLRM's shape and [`Csi::Unsupported`].
    SaveCursor,
    /// SCORC, `CSI u`. Every parameter defaulted or none, as SCOSC.
    RestoreCursor,
    /// SU, `CSI S`. Scroll the scrolling region up, cursor unmoved.
    ScrollUp(Count),
    /// SD, `CSI T`. Scroll the scrolling region down, cursor unmoved.
    ScrollDown(Count),
    /// SL, `CSI Pn SP @` (`[[source: ecma48]]` 8.3.121): the rectangle the
    /// margins bound moves left, cursor unmoved.
    ScrollLeft(Count),
    /// SR, `CSI Pn SP A` (8.3.135): the mirror.
    ScrollRight(Count),
    /// SM / RM, the unprefixed mode namespace, `CSI Pm h` and `CSI Pm l`.
    /// `mode` is the first parameter's; the list is every parameter, which
    /// the caller walks, as it walks [`Csi::PrivateMode`]'s.
    Mode {
        change: ModeChange,
        mode: AnsiMode,
    },
    Sgr,
    /// DECSCUSR, `CSI Ps SP q`. Cursor shape and blink, or the initial style.
    CursorStyle(CursorPick),
    /// XTVERSION, `CSI > 0 q`. Answered with [`Report::Version`].
    RequestVersion,
    /// XTMODKEYS `CSI > Pp ; Pv m`, XTFMTKEYS `CSI > Pp ; Pv f` and the
    /// disable form `CSI > Ps n`: xterm's seven key-modifier resources
    /// (`[[source: ctlseqs]]`). **Its parameters are read where it is
    /// acted on** ([`key_change`]), as [`Csi::Query`]'s are: read here, they
    /// put a call and its live values in `csi_final`, six registers saved
    /// on every CSI. A value no text documents is refused and counted there
    /// rather than approximated.
    KeyControl(KeyControl),
    /// XTSHIFTESCAPE, `CSI > Ps s`: whether Shift takes a pointer event from
    /// a program tracking the mouse.
    ShiftEscape(ShiftEscape),
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
        /// `None` when omitted or 0, which is the page's last column:
        /// "default Pr: Last Column" (`[[source: dec-std-070]]`). Decoded
        /// as `OneBased` until 2026-10-13, both read as column 1, so `Pl` >=
        /// `Pr` and the sequence was dropped (see [`Params::far_margin`]).
        right: Option<OneBased>,
    },
    /// DECIC, `CSI Pn ' }`. Insert columns within the margins.
    InsertColumns(Count),
    /// DECDC, `CSI Pn ' ~`. Delete columns within the margins.
    DeleteColumns(Count),
    /// DECSCPP, `CSI Pn $ |` (`[[source: dec-std-070]]` p. 5-73,
    /// `[[source: vt510-decscpp]]`): "sets the number of columns in the
    /// display page", which [`PageWidth::per_page`] reads. Unlike DECCOLM it
    /// "does not clear page memory or reset the scrolling regions".
    SetColumnsPerPage(PageWidth),
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
    /// `CSI Ps ; Ps ; Ps ; Ps ; Ps T` — XTHIMOUSE, the program's answer to a
    /// `?1001` press ([`Handshake::answered`]).
    ///
    /// Told from SD by its arity, so it can never *scroll*. The hazard `?1001`
    /// carries is xterm's rather than baud's — "all X events are ignored until
    /// the proper escape sequence is received" describes an event loop, and
    /// baud has none to block: an answer that never comes leaves the next
    /// release a normal one.
    HighlightMouseTracking(HighlightRegion),
    /// XTWINOPS, `CSI Ps ; Ps ; Ps t`: the window operations
    /// ([`WindowRequest::of`]), the reports about the text area, the window
    /// and the screen ([`Report::window`]), the title stack, and the refused
    /// rest ([`WindowOp`]).
    Window(WindowOp),
    /// XTSMGRAPHICS, `CSI ? Pi ; Pa ; Pv S`: "Set or request graphics
    /// attribute" (`[[source: ctlseqs]]`), answered with
    /// [`Report::Graphics`]. Only the three-parameter form ctlseqs gives is
    /// this, "xterm accepts a three-parameter control sequence"; any other
    /// count is counted and unanswered, decided by Jev 2026-10-15
    /// (THREE_ONLY, 0.96/0.96).
    Graphics(GraphicsRequest),
    /// One of the VT420 rectangle operations, DECCRA to DECRQCRA
    /// ([`AreaOp`]). **Its parameters are read where it is performed**
    /// ([`AreaOp::with`]), not here: a variant carrying a rectangle or two
    /// made the caller's dispatch keep them all live, and its prologue save
    /// six register pairs where it saved three, on every `CSI`.
    Area(AreaOp),
    /// DECRQPSR, `CSI Ps $ w`: a presentation state report, DECCIR for 1
    /// and DECTABSR for 2 ([`PresentationState`]). `Ps` is read where the
    /// report is made, as [`Csi::Area`]'s rectangle is.
    RequestPresentationState,
    /// One of xterm's stack controls ([`StackOp`]), its parameters read
    /// where it is performed, as [`Csi::Area`]'s are.
    Stack(StackOp),
    /// XTSMTITLE `CSI > Pm t` and XTRMTITLE `CSI > Pm T`: set or reset the
    /// title modes each parameter names ([`crate::TitleMode`]), or with none
    /// return them to the default. **Its parameters are read where it is
    /// acted on.**
    TitleModes(ModeChange),
    Unsupported {
        marker: Marker,
        final_byte: FinalByte,
    },
}

/// One XTSMGRAPHICS request: the item `Pi`, the action `Pa` and the value
/// `Pv`, as they arrived, an omitted one as 0. The reply repeats `Pi`, so
/// the raw parameter is kept rather than only its meaning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraphicsRequest {
    pub item: Param,
    pub action: Param,
    pub value: Param,
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
    /// builds a 58-variant enum that `baud-core`'s `csi_final` destructures
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
                right: p.far_margin(ParamIdx::SECOND),
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
                (b'"', b'p') if marker == Marker::Standard => Csi::SetConformanceLevel,
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
                // SL and SR, the standard namespace's alone: `[[source:
                // ecma48]]` 8.3.121 and 8.3.135 give `CSI Pn 02/00 04/00`
                // and `04/01`, and no prefix.
                (b' ', b'@') if marker == Marker::Standard => {
                    Csi::ScrollLeft(p.count(ParamIdx::FIRST))
                }
                (b' ', b'A') if marker == Marker::Standard => {
                    Csi::ScrollRight(p.count(ParamIdx::FIRST))
                }
                // `[[source: vt510-decic]]` / `[[source: vt510-decdc]]`.
                (b'\'', b'}') => Csi::InsertColumns(p.count(ParamIdx::FIRST)),
                (b'\'', b'~') => Csi::DeleteColumns(p.count(ParamIdx::FIRST)),
                (b'$', b'p') => Csi::RequestMode {
                    marker,
                    mode: p.at(ParamIdx::FIRST).unwrap_or(Param::ZERO),
                },
                // DECSCPP is the standard namespace's: `CSI ? Pn $ |` is no
                // control anywhere in `[[source: ctlseqs]]`.
                (b'$', b'|') if marker == Marker::Standard => {
                    Csi::SetColumnsPerPage(PageWidth::per_page(p.at(ParamIdx::FIRST)))
                }
                // The rectangle operations, the standard namespace's alone
                // (`[[source: ctlseqs]]` and `[[source: dec-std-070]]`
                // § 5.12 give none of them a prefix).
                (b'$', b'v') if marker == Marker::Standard => Csi::Area(AreaOp::Copy),
                (b'$', b'x') if marker == Marker::Standard => Csi::Area(AreaOp::Fill),
                (b'$', b'z') if marker == Marker::Standard => Csi::Area(AreaOp::Erase),
                (b'$', b'{') if marker == Marker::Standard => Csi::Area(AreaOp::SelectiveErase),
                (b'$', b'r') if marker == Marker::Standard => Csi::Area(AreaOp::ChangeAttrs),
                (b'$', b't') if marker == Marker::Standard => Csi::Area(AreaOp::ReverseAttrs),
                (b'*', b'x') if marker == Marker::Standard => Csi::Area(AreaOp::SelectExtent),
                (b'*', b'y') if marker == Marker::Standard => Csi::Area(AreaOp::RequestChecksum),
                // XTCHECKSUM, `CSI Ps # y` (`[[source: ctlseqs]]`).
                (b'#', b'y') if marker == Marker::Standard => Csi::Area(AreaOp::SelectChecksum),
                // DECRQPSR, `CSI Ps $ w`: `[[source: dec-std-070]]` p. 5-198,
                // "causes the terminal to respond with a Presentation State
                // Report"; `[[source: vt510-decrqpsr]]`.
                (b'$', b'w') if marker == Marker::Standard => Csi::RequestPresentationState,
                // DECRQUPSS and DECRQDE. Their parameters, which neither
                // takes, are read where they are answered ([`QueryOp::with`]).
                (b'&', b'u') if marker == Marker::Standard => {
                    Csi::Query(QueryOp::PreferredSupplemental)
                }
                (b'"', b'v') if marker == Marker::Standard => Csi::Query(QueryOp::DisplayedExtent),
                // XTTITLEPOS, `CSI # S` (`[[source: ctlseqs]]` 664).
                (b'#', b'S') if marker == Marker::Standard => Csi::Stack(StackOp::TitlePosition),
                // XTPUSHSGR and XTPOPSGR, `# {` and `# }`, and their aliases
                // `# p` and `# q`, "used to work around language limitations
                // of C#" (`[[source: ctlseqs]]` 1525-1568 and 1871-1953);
                // XTREPORTSGR, `# |`, a rectangle's.
                (b'#', b'{' | b'p') if marker == Marker::Standard => {
                    Csi::Stack(StackOp::PushRendition)
                }
                (b'#', b'}' | b'q') if marker == Marker::Standard => {
                    Csi::Stack(StackOp::PopRendition)
                }
                (b'#', b'|') if marker == Marker::Standard => Csi::Area(AreaOp::ReportRendition),
                // XTPUSHCOLORS, XTPOPCOLORS and XTREPORTCOLORS
                // (`[[source: ctlseqs]]` 644-660).
                (b'#', b'P') if marker == Marker::Standard => Csi::Stack(StackOp::PushColors),
                (b'#', b'Q') if marker == Marker::Standard => Csi::Stack(StackOp::PopColors),
                (b'#', b'R') if marker == Marker::Standard => Csi::Stack(StackOp::ReportColors),
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
        // XTMODKEYS, XTFMTKEYS and the disable form. `[[source: ctlseqs]]`:
        // `CSI > Pp ; Pv <final>`, `Pp` naming one of seven resources and
        // `Pv` its value, an omitted `Pv` resetting the resource and no
        // parameter every one; `CSI > Ps n` disables `Ps`, an omitted one
        // modifyFunctionKeys.
        if marker == Marker::Gt {
            match f.raw() {
                b'm' => return Csi::KeyControl(KeyControl::Modify),
                b'f' => return Csi::KeyControl(KeyControl::Format),
                b'n' => return Csi::KeyControl(KeyControl::Disable),
                // XTSMTITLE and XTRMTITLE (`[[source: ctlseqs]]` 1707 and
                // 733): the `>` keeps them from XTWINOPS and from SD.
                b't' => return Csi::TitleModes(ModeChange::Set),
                b'T' => return Csi::TitleModes(ModeChange::Reset),
                _ => {}
            }
        }
        // XTQFMTKEYS, `CSI > Pp g`: xterm 411's parse table
        // (`[[source: xterm-vtprstbl-c]]`, `dec2_table`) and not the
        // `CSI ? Pp g` `[[source: ctlseqs]]` line 884 documents, which xterm
        // ignores (Jev's lean, 0.48 and 0.34, taken).
        if marker == Marker::Gt && f.raw() == b'g' {
            return Csi::Query(QueryOp::FormatKeys);
        }
        // XTSHIFTESCAPE, `CSI > Ps s`: the `>` keeps it from SCOSC and
        // DECSLRM, the standard namespace's `s`.
        if marker == Marker::Gt && f.raw() == b's' && !p.has(ParamIdx::SECOND) {
            return match ShiftEscape::from_param(p.at(ParamIdx::FIRST)) {
                Some(e) => Csi::ShiftEscape(e),
                None => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
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
                // DECST8C, one parameter and that one 5.
                b'W' if p.raw(ParamIdx::FIRST, 0) == 5 && !p.has(ParamIdx::SECOND) => {
                    Csi::TabsEveryEight
                }
                // XTSAVE and XTRESTORE. Not SCOSC or DECSLRM, not DECSTBM:
                // those are the standard namespace's `s` and `r`.
                b's' => Csi::SaveModes,
                b'r' => Csi::RestoreModes,
                // Three parameters, asked as "a third and no fourth" rather
                // than `len() == 3`: that count was hoisted into `csi_final`'s
                // entry, three instructions on every CSI but SGR, and these
                // two fold to tests of the parser's own index.
                b'S' if p.has(ParamIdx::at(2)) && !p.has(ParamIdx::at(3)) => {
                    Csi::Graphics(GraphicsRequest {
                        item: Param(p.raw(ParamIdx::FIRST, 0)),
                        action: Param(p.raw(ParamIdx::SECOND, 0)),
                        value: Param(p.raw(ParamIdx::at(2), 0)),
                    })
                }
                b'n' if p.at(ParamIdx::FIRST).map_or(0, |x| x.0) == 996 => Csi::ColorSchemeQuery,
                // The DEC private DSRs, `Ps` read where they are answered:
                // a test of it here put a clamp of the parameter count on
                // `csi_final`'s entry, three instructions on every `CSI` but
                // SGR, as `S` once did.
                b'n' => Csi::Query(QueryOp::DecStatus),
                // XTQMODKEYS (`[[source: ctlseqs]]` line 1356).
                b'm' => Csi::Query(QueryOp::ModifyKeys),
                b'J' if p.has(ParamIdx::SECOND) => Csi::EraseEach(EraseList::SelectiveDisplay),
                b'K' if p.has(ParamIdx::SECOND) => Csi::EraseEach(EraseList::SelectiveLine),
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
            b'J' if p.has(ParamIdx::SECOND) => Csi::EraseEach(EraseList::Display),
            b'K' if p.has(ParamIdx::SECOND) => Csi::EraseEach(EraseList::Line),
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
            b'O' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInArea(e),
                None => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            },
            b'N' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInField(e),
                None => Csi::Unsupported {
                    marker,
                    final_byte: f,
                },
            },
            b'b' => Csi::Repeat(p.count(ParamIdx::FIRST)),
            // **Only a request is answered**: `[[source: ecma48]]` 8.3.24,
            // "With a parameter value not equal to 0, DA is used to identify
            // the device which sends the DA ... If the parameter value is 0,
            // DA is used to request an identifying DA from a device", and
            // xterm answers only `GetParam(0) <= 0` (`CASE_DA1`,
            // `[[source: xterm-reset]]`'s file). A device identifying itself
            // to baud is not a function baud has, so it is counted.
            b'c' if p.raw(ParamIdx::FIRST, 0) == 0 => Csi::DeviceAttributes,
            b'c' => Csi::Unsupported {
                marker,
                final_byte: f,
            },
            b'n' => Csi::DeviceStatus(StatusKind::from_param(p.at(ParamIdx::FIRST))),
            b'I' => Csi::CursorForwardTab(p.count(ParamIdx::FIRST)),
            b'Z' => Csi::CursorBackTab(p.count(ParamIdx::FIRST)),
            b'g' => Csi::TabClear(TabClear::from_param(p.at(ParamIdx::FIRST))),
            b'W' => Csi::CursorTabControl,
            b'Y' => Csi::CursorLineTab(p.count(ParamIdx::FIRST)),
            // **`CSI Ps ^` is SIMD, not SD**, and falls to `Unsupported`: the
            // pinned `[[source: ecma48]]` gives `CSI Ps 05/14` to SELECT
            // IMPLICIT MOVEMENT DIRECTION and SD `05/04`. ctlseqs keeps it
            // as SD: "This was a publication error in the original ECMA-48
            // 5th edition (1991) corrected in 2003". baud has no
            // implicit-movement directions to select (Jev's lean,
            // SIMD_COUNTED, 0.80/0.84).
            b'E' => Csi::CursorNextLine(p.count(ParamIdx::FIRST)),
            b'F' => Csi::CursorPrevLine(p.count(ParamIdx::FIRST)),
            b'G' | b'`' => Csi::ColumnAbsolute(p.one_based(ParamIdx::FIRST)),
            b'd' => Csi::LineAbsolute(p.one_based(ParamIdx::FIRST)),
            b'a' => Csi::ColumnRelative(p.count(ParamIdx::FIRST)),
            b'e' => Csi::LineRelative(p.count(ParamIdx::FIRST)),
            b'j' => Csi::ColumnBackward(p.count(ParamIdx::FIRST)),
            b'k' => Csi::LineBackward(p.count(ParamIdx::FIRST)),
            // SCOSC and SCORC take no parameter (`[[source: ctlseqs]]` 1592
            // and 1734), so a defaulted one is the bare form. Anything else
            // is DECSLRM's shape for `s`, "available only when DECLRMM is
            // enabled" (1595-1597), and no form for `u`: counted, and the
            // cursor neither saved nor restored. Both saved or restored it
            // until 2026-10-14 (`tests/vt100/save_params.rs`).
            b's' | b'u' if !p.all_default() => Csi::Unsupported {
                marker,
                final_byte: f,
            },
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
            // XTWINOPS. **Only the forms `[[source: ctlseqs]]` gives**
            // (`WindowOp::of`); any other is `WindowOp::Refused`, never read
            // as a neighbouring form.
            b't' => Csi::Window(WindowOp::of(&p)),
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
                bottom: p.far_margin(ParamIdx::SECOND),
            },
            b'm' => Csi::Sgr,
            // DECREQTPARM (`[[source: vt100-ug-ch3]]`); a `<sol>` of 2 or 3
            // is a report, sent to baud rather than asked of it, and is told
            // apart where the parameter is read.
            b'x' => Csi::Query(QueryOp::TerminalParameters),
            _ => Csi::Unsupported {
                marker,
                final_byte: f,
            },
        }
    }
}

/// TBC, `CSI g`: `[[source: ecma48]]` 8.3.154's six values, the line
/// tabulation stops' included. `[[source: vt510-tbc]]` and xterm keep 0 and
/// 3 ("There are only two values for Ps"); ECMA-48's whole set was decided
/// by Jev 2026-10-19 (BUILD_ECMA, 0.98/0.94).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabClear {
    /// 0: "the character tabulation stop at the active presentation
    /// position".
    AtCursor,
    /// 1: "the line tabulation stop at the active line".
    LineAtCursor,
    /// 2: "all character tabulation stops in the active line". A stop is
    /// in every line under TSM MULTIPLE, the one state baud has, so every
    /// character stop.
    InLine,
    /// 3: "all character tabulation stops".
    All,
    /// 4: "all line tabulation stops".
    AllLines,
    /// 5: "all tabulation stops".
    Everything,
}

impl TabClear {
    pub fn from_param(p: Option<Param>) -> Option<TabClear> {
        match p.map_or(0, |x| x.0) {
            0 => Some(TabClear::AtCursor),
            1 => Some(TabClear::LineAtCursor),
            2 => Some(TabClear::InLine),
            3 => Some(TabClear::All),
            4 => Some(TabClear::AllLines),
            5 => Some(TabClear::Everything),
            _ => None,
        }
    }
}

/// One CTC parameter (`[[source: ecma48]]` 8.3.17): 0
/// and 1 set a stop, 2 to 6 clear what TBC 0, 1, 2, 3 and 4 clear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabControl {
    /// 0: "a character tabulation stop is set at the active presentation
    /// position", as HTS.
    SetHere,
    /// 1: "a line tabulation stop is set at the active line", as VTS.
    SetLine,
    Clear(TabClear),
}

impl TabControl {
    pub fn from_param(p: Option<Param>) -> Option<TabControl> {
        Some(match p.map_or(0, |x| x.0) {
            0 => TabControl::SetHere,
            1 => TabControl::SetLine,
            2 => TabControl::Clear(TabClear::AtCursor),
            3 => TabControl::Clear(TabClear::LineAtCursor),
            4 => TabControl::Clear(TabClear::InLine),
            5 => TabControl::Clear(TabClear::All),
            6 => TabControl::Clear(TabClear::AllLines),
            _ => return None,
        })
    }
}

/// The line tabulation stops, one bit a line (`[[source: ecma48]]` 8.3.162,
/// VTS: "a line tabulation stop ... at the active line"). Fixed to the
/// screen's lines, as the character stops are fixed to its columns: a
/// scroll moves the text and leaves them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LineStops([u64; LineStops::WORDS]);

impl LineStops {
    const WORDS: usize = 10;
    /// How many lines can carry a stop.
    pub const LINES: usize = LineStops::WORDS * 64;
    pub const NONE: LineStops = LineStops([0; LineStops::WORDS]);

    pub fn set(&mut self, y: Row) {
        if y.index() < Self::LINES {
            self.0[y.index() / 64] |= 1 << (y.index() % 64);
        }
    }
    pub fn clear(&mut self, y: Row) {
        if y.index() < Self::LINES {
            self.0[y.index() / 64] &= !(1 << (y.index() % 64));
        }
    }
    pub fn clear_all(&mut self) {
        *self = Self::NONE;
    }
    /// The first stop after line `y` and not past line `last`: a word at a
    /// time, so at most ten words whatever the screen.
    pub fn next_after(self, y: Row, last: Row) -> Option<Row> {
        let from = y.index() + 1;
        let to = last.index().min(Self::LINES - 1);
        if from > to {
            return None;
        }
        let mut w = from / 64;
        let mut bits = self.0[w] & (u64::MAX << (from % 64));
        loop {
            if bits != 0 {
                let at = w * 64 + bits.trailing_zeros() as usize;
                return (at <= to).then(|| Row::from_index(at));
            }
            w += 1;
            if w > to / 64 {
                return None;
            }
            bits = self.0[w];
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
            | ColumnBackward(_)
            | LineBackward(_)
            | CursorPosition { .. }
            | SaveCursor
            | RestoreCursor
            | EraseInDisplay(_)
            | EraseInLine(_)
            | EraseEach(EraseList::Display | EraseList::Line)
            | EraseInArea(_)
            | EraseInField(_)
            | EraseChars(_)
            | InsertBlanks(_)
            | DeleteChars(_)
            | InsertLines(_)
            | DeleteLines(_)
            | ScrollUp(_)
            | ScrollDown(_)
            | ScrollLeft(_)
            | ScrollRight(_)
            | SetScrollRegion { .. }
            | CursorForwardTab(_)
            | CursorBackTab(_)
            | TabClear(_)
            | CursorTabControl
            | CursorLineTab(_)
            | Mode { .. }
            | PrivateMode { .. }
            | Sgr
            | Repeat(_)
            | DeviceAttributes
            | DeviceStatus(_) => Level::Vt100,

            // "Levels: 1x, 2, 3, 4" (070-4 DECSCL), required from Level 2.
            SoftReset
            | SetConformanceLevel
            | SelectProtection { .. }
            | SelectiveEraseDisplay(_)
            | SelectiveEraseLine(_)
            | EraseEach(EraseList::SelectiveDisplay | EraseList::SelectiveLine)
            | RequestMode { .. }
            | CursorStyle(_) => Level::Vt220,

            SetSideMargins { .. } | InsertColumns(_) | DeleteColumns(_) => Level::Vt420,
            // "VT510" (`[[source: ctlseqs]]`), a level this table does not
            // carry; VT420 is the nearest it has.
            TabsEveryEight => Level::Vt420,
            // "Levels: 3X (Windowing), 4" (`[[source: dec-std-070]]` p. 5-73);
            // ctlseqs says VT340, a level this table does not carry.
            SetColumnsPerPage(_) => Level::Vt420,
            // "Level: 4" each (`[[source: dec-std-070]]` pp. 5-169 to 5-179).
            // XTREPORTSGR is xterm's, on DEC's rectangle.
            Area(AreaOp::ReportRendition) => Level::Xterm,
            // "Select checksum extension (XTCHECKSUM), xterm."
            Area(AreaOp::SelectChecksum) => Level::Xterm,
            Area(_) | RequestPresentationState => Level::Vt420,

            // XTWINOPS is "dtterm, extended by xterm" -- the three reports are
            // xterm's extensions of it.
            RequestVersion | HighlightMouseTracking(_) | Window(_) => Level::Xterm,
            // "Set or request graphics attribute (XTSMGRAPHICS), xterm."
            Graphics(_) => Level::Xterm,
            // "Erase Saved Lines, xterm" in both forms: the saved lines are
            // xterm's, and no DEC level has them.
            EraseSavedLines | SelectiveEraseSavedLines => Level::Xterm,
            // XTSAVE and XTRESTORE are xterm's by name.
            SaveModes | RestoreModes => Level::Xterm,
            // XTMODKEYS and XTFMTKEYS are xterm's, and input-side: they change
            // what the keyboard sends rather than what the screen shows. The
            // kitty negotiation is the same shape -- input-side, and an
            // extension rather than a VT level.
            KeyControl(_)
            | ShiftEscape(_)
            | KittySet { .. }
            | KittyQuery
            | KittyPush { .. }
            | KittyPop { .. } => Level::Xterm,
            // contour's extension, not xterm's; this table has no level of its
            // own for it and it is not a VT level either.
            ColorSchemeQuery => Level::Xterm,
            // XTTITLEPOS, XTSMTITLE and XTRMTITLE are xterm's by name.
            Stack(_) | TitleModes(_) => Level::Xterm,
            Query(q) => q.level(),

            Unsupported { .. } => return None,
        })
    }
}

/// Which of the key-modifier controls a [`Csi::KeyControl`] is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyControl {
    /// XTMODKEYS, `CSI > Pp ; Pv m`.
    Modify,
    /// XTFMTKEYS, `CSI > Pp ; Pv f`.
    Format,
    /// `CSI > Ps n`.
    Disable,
}

impl KeyControl {
    pub fn final_byte(self) -> FinalByte {
        FinalByte::of_final(StreamByte::from_stream(match self {
            KeyControl::Modify => b'm',
            KeyControl::Format => b'f',
            KeyControl::Disable => b'n',
        }))
    }
}

/// What a key-modifier control changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyChange {
    Modify(KeySet),
    Format(FormatSet),
}

/// A [`KeyControl`]'s parameters read, or `None` for a shape or a value the
/// texts do not give.
///
/// The ignore mask is `CSI > 4 ; Pv : mask m`, a sub-parameter of `Pv`, as
/// xterm's `CASE_SET_MOD_FKEYS` parses it (Jev's lean, CODE_FORM,
/// 0.69/0.88 then 0.80/0.89). **Open departure:** ctlseqs' own example,
/// `CSI > 4 : 1 m`, puts it on `Pp`, which xterm's parser does not read
/// there, and is refused and counted here. The mask is the parameter's
/// bits, Shift 1, Alt 2, Control 4, Meta 8, as ctlseqs' "a mask of
/// modifier bits to factor out of the parameter encoding" reads; xterm
/// applies ignore_now to the X event state (Shift 1, Lock 2, Control 4,
/// Mod1 8), so the two agree on Shift and Control only.
pub fn key_change(p: &Params<'_>, op: KeyControl) -> Option<KeyChange> {
    let final_byte = op.final_byte().raw();
    let (first, second, third) = (ParamIdx::FIRST, ParamIdx::SECOND, ParamIdx::at(2));
    let subs = p.is_sub(second) || p.is_sub(third) || p.has(ParamIdx::at(3));
    let resource = || KeyResource::from_param(p.at(first)?.0);
    if final_byte == b'n' {
        return match (p.at(first), p.has(second) || subs) {
            (_, true) => None,
            (None, false) => Some(KeyChange::Modify(KeySet::Disable(KeyResource::Function))),
            (Some(_), false) => Some(KeyChange::Modify(KeySet::Disable(resource()?))),
        };
    }
    // The mask, `Pv : mask`, on modifyOtherKeys' level alone.
    let mask = match (p.is_sub(third), p.is_sub(second), p.has(ParamIdx::at(3))) {
        (false, false, false) if !p.has(third) => Modifiers::NONE,
        (true, false, false) if final_byte == b'm' && resource()? == KeyResource::Other => {
            Modifiers::from_wire(u8::try_from(p.at(third)?.0).ok()?.checked_add(1)?)?
        }
        _ => return None,
    };
    let none_at_all = p.at(first).is_none() && !p.has(second);
    let set = match (none_at_all, p.at(second)) {
        (true, _) => None,
        (false, None) => Some(None),
        (false, Some(v)) => Some(Some(v.0)),
    };
    Some(match (final_byte, set) {
        (b'm', None) => KeyChange::Modify(KeySet::ResetAll),
        (_, None) => KeyChange::Format(FormatSet::ResetAll),
        (b'm', Some(None)) => KeyChange::Modify(KeySet::Reset(resource()?)),
        (_, Some(None)) => KeyChange::Format(FormatSet::Reset(resource()?)),
        (b'm', Some(Some(v))) => {
            let r = resource()?;
            KeyChange::Modify(KeySet::Set(r, r.value(v)?, mask))
        }
        (_, Some(Some(v))) => KeyChange::Format(FormatSet::Set(
            resource()?,
            match v {
                0 => OtherKeysFormat::Xterm,
                1 => OtherKeysFormat::CsiU,
                _ => return None,
            },
        )),
    })
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
