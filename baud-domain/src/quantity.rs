use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Display width of **one grapheme cluster**: 0 (combining), 1, or 2 (wide).
///
/// The only thing that may advance a [`Col`]. Obtainable only from
/// [`Decoded::width`], which means only by decoding — so a byte count can never
/// reach the cursor by accident.
///
/// This exists because of a real bug: `cx += 1` per UTF-8 continuation byte
/// advanced a 3-byte CJK character three columns instead of two, corrupting
/// every position after the first non-ASCII character on a line. Both operands
/// were `usize`, so it compiled.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Width(pub(crate) u8);

impl Width {
    /// Combining mark, ZWJ, variation selector — attaches to the previous cell.
    pub const ZERO: Width = Width(0);
    pub const NARROW: Width = Width(1);
    /// East Asian Wide/Fullwidth, and emoji under emoji presentation.
    pub const WIDE: Width = Width(2);
    pub fn cells(self) -> usize {
        self.0 as usize
    }
}

/// A length in **bytes**. Has no path to [`Col`] or [`Width`], by construction.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct ByteLen(pub(crate) u16);

impl ByteLen {
    pub const ZERO: ByteLen = ByteLen(0);
    /// Length of a byte slice. The single place a `len()` becomes a quantity.
    pub fn of(s: &[u8]) -> ByteLen {
        ByteLen::from_index(s.len())
    }
    pub(crate) const fn new(n: u16) -> ByteLen {
        ByteLen(n)
    }
    /// From a slice length or a read result. Saturates: a length that does not
    /// fit is malformed input, not an overflow bug.
    pub fn from_index(n: usize) -> ByteLen {
        ByteLen(n.min(u16::MAX as usize) as u16)
    }
    /// Columns proven to fit are also a byte count when the encoding is
    /// byte-per-column, which is the only place this is used.
    pub fn from_room(r: Room) -> ByteLen {
        ByteLen::from_index(r.index())
    }
    pub(crate) fn get(self) -> u16 {
        self.0
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    pub fn sub_room(self, r: Room) -> ByteLen {
        ByteLen(
            self.0
                .saturating_sub(r.index().min(u16::MAX as usize) as u16),
        )
    }
    /// Named for what it does. `add` hid the saturation, which is the kind of
    /// invisible behaviour this crate exists to prevent.
    pub fn saturating_add(self, o: ByteLen) -> ByteLen {
        ByteLen(self.0.saturating_add(o.0))
    }
}

/// A column on the grid. Advances only by [`Width`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Col(pub(crate) usize);

impl Col {
    pub const ZERO: Col = Col(0);
    /// Only for interop with code that is not yet using [`Grid`]. Indexing a
    /// `Grid` takes a `Col` directly, so this should have no callers.
    ///
    /// Backed by `usize`, not `u16`: measured, a `u16` cursor costs 11-15%
    /// against `usize` on x86-64 because every op needs zero-extension and
    /// masking. Narrow types belong in [`Cell`], where 10,000 of them sit in a
    /// matrix and size is the throughput lever — not in an arithmetic path.
    pub fn index(self) -> usize {
        self.0
    }

    /// The only way to move the cursor horizontally through text.
    ///
    /// Returns `None` when the grapheme will not fit — a 2-wide character at
    /// the last column cannot be split, so the caller must leave the cell blank
    /// and wrap. Returning `Option` makes that decision unavoidable rather than
    /// something to forget.
    /// For placing a *single* grapheme, where the fit is genuinely in question:
    /// a 2-wide character at the last column cannot be split, so the caller
    /// must blank the cell and wrap. Use `+= Room` for bulk runs, where the fit
    /// was already established.
    #[must_use]
    pub fn try_advance(self, w: Width, cols: Cols) -> Option<Col> {
        let next = self.0 + w.cells();
        if next > cols.raw() {
            None
        } else {
            Some(Col(next))
        }
    }
    /// Explicit cursor addressing. Goes through [`OneBased`] so the 1-based
    /// wire convention is converted in exactly one place.
    pub fn at(p: OneBased, cols: Cols) -> Col {
        p.to_col(cols)
    }
}

/// A number of columns **proven to fit** at a given cursor position.
///
/// Obtained only from [`Cols::room_at`], narrowed only downward by
/// [`Room::min`]. Because it can never exceed the space remaining, adding it to
/// the [`Col`] it came from is total — which is what lets `cx += k` be both
/// typed and infallible.
///
/// This is the ordinary case: `put` computes `k = min(cols - cx, n)` and then
/// advances by it. Returning `Option` there was checking a condition the caller
/// had already established.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Room(pub(crate) usize);

impl Room {
    pub const NONE: Room = Room(0);
    /// Narrowing only, and against a typed length. There is no widening
    /// constructor, so the proof that this fits cannot be forged.
    pub fn min(self, n: ByteLen) -> Room {
        Room(self.0.min(n.get() as usize))
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::AddAssign<Room> for Col {
    /// Total: `Room` cannot exceed the space it was measured from.
    fn add_assign(&mut self, r: Room) {
        self.0 += r.0;
    }
}

impl core::ops::Add<Room> for Col {
    type Output = Col;
    fn add(self, r: Room) -> Col {
        Col(self.0 + r.0)
    }
}

impl core::ops::SubAssign<Room> for ByteLen {
    fn sub_assign(&mut self, r: Room) {
        self.0 = self.0.saturating_sub(r.0 as u16);
    }
}

/// A row on the grid.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Row(pub(crate) usize);

/// Grid width. Distinct from [`Rows`] so the two cannot be swapped at a call
/// site — `Layout::new(rows, cols)` transposed is a bug that used to compile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Cols(pub(crate) usize);

impl Cols {
    pub(crate) fn from_ws(n: u16) -> Cols {
        Cols((n as usize).max(1))
    }
    /// Columns remaining at `cx`. The only source of a [`Room`].
    pub fn room_at(self, cx: Col) -> Room {
        Room(self.0.saturating_sub(cx.0))
    }
    /// Cells from `cx` up to, but not including, `edge`.
    ///
    /// The right-margin form of [`Cols::room_at`]. With DECSLRM unset the edge
    /// is one past the last column and the two agree, so the write path can
    /// use this one unconditionally rather than branching per run.
    pub fn room_until(self, cx: Col, edge: Col) -> Room {
        Room(edge.index().min(self.0).saturating_sub(cx.index()))
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub(crate) fn raw(self) -> usize {
        self.0
    }
}

/// Grid height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Rows(pub(crate) usize);

impl Rows {
    pub(crate) fn from_ws(n: u16) -> Rows {
        Rows((n as usize).max(1))
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub(crate) fn raw(self) -> usize {
        self.0
    }
}

/// How many display rows something occupies. Not a [`Rows`] — that is a grid
/// dimension, this is a count that can exceed it.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct RowCount(pub(crate) u32);

impl RowCount {
    pub const ZERO: RowCount = RowCount(0);
    pub fn index(self) -> usize {
        self.0 as usize
    }
    /// Not an `Add` impl, deliberately. An operator makes the quantity invisible
    /// at the call site, and a visible quantity is the whole point of this crate.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, o: RowCount) -> RowCount {
        RowCount(self.0 + o.0)
    }
}

/// A block of `rows` consecutive rows in a fixed-stride cell buffer.
///
/// The `rows * MAX_COLS` arithmetic this replaces appeared twice outside the
/// [`Grid`] helpers -- once for the visible viewport and once for a template
/// row -- and both multiply a *count of rows* by a *stride*. Getting the
/// operands the wrong way round produces a plausible number and a silently
/// wrong slice, which is the same class of mistake `Grid::at` already exists to
/// prevent for single cells.
#[derive(Clone, Copy)]
pub struct RowBlock {
    start: usize,
    len: usize,
}

impl RowBlock {
    /// The first `rows` rows.
    pub fn leading(rows: Rows, stride: Cols) -> RowBlock {
        RowBlock {
            start: 0,
            len: rows.index() * stride.index(),
        }
    }
    /// One row-sized block at row index `n`. Used for the style template table,
    /// where each style owns one row of pre-filled cells.
    pub fn nth(n: usize, stride: Cols) -> RowBlock {
        RowBlock {
            start: n * stride.index(),
            len: stride.index(),
        }
    }
    pub fn of<T>(self, buf: &[T]) -> &[T] {
        &buf[self.start..self.start + self.len]
    }
    pub fn of_mut<T>(self, buf: &mut [T]) -> &mut [T] {
        &mut buf[self.start..self.start + self.len]
    }
}

impl Row {
    pub const ZERO: Row = Row(0);
    /// See [`Col::as_index`].
    pub fn index(self) -> usize {
        self.0
    }
    pub fn next(self, rows: Rows) -> Option<Row> {
        if self.0 + 1 >= rows.raw() {
            None
        } else {
            Some(Row(self.0 + 1))
        }
    }
}

/// A parameter exactly as it appears on the wire: **1-based**, because ECMA-48
/// numbers rows and columns from 1 while every buffer indexes from 0.
///
/// There is no `From<u16> for Row`. The only route is [`OneBased::to_row`],
/// which subtracts and clamps in one place instead of at twenty call sites.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OneBased(pub(crate) u16);

impl OneBased {
    /// An omitted parameter is 1 for cursor addressing, and so is an explicit 0.
    pub fn from_param(p: Option<Param>) -> OneBased {
        OneBased(p.map_or(0, |x| x.0).max(1))
    }
    pub fn to_row(self, rows: Rows) -> Row {
        Row(((self.0 - 1) as usize).min(rows.raw().saturating_sub(1)))
    }
    pub fn to_col(self, cols: Cols) -> Col {
        Col(((self.0 - 1) as usize).min(cols.raw().saturating_sub(1)))
    }
    /// The wire value. For emitting a report, which is the only direction that
    /// needs the number back out.
    pub fn get(self) -> u16 {
        self.0
    }
}

impl Row {
    /// This row as it appears on the wire. CPR and CUP are both 1-based, so a
    /// report built from this round-trips through `OneBased::to_row`.
    pub fn one_based(self) -> OneBased {
        OneBased(self.0.saturating_add(1).min(u16::MAX as usize) as u16)
    }
}

impl Col {
    /// This column as it appears on the wire.
    pub fn one_based(self) -> OneBased {
        OneBased(self.0.saturating_add(1).min(u16::MAX as usize) as u16)
    }
}

/// A repeat count or distance. Omitted or zero both mean 1 — a rule that was
/// previously restated as `if a != 0 { a } else { 1 }` in every arm.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Count(pub(crate) u16);

impl Count {
    pub const ZERO: Count = Count(0);
    /// A count of things, where the caller has one.
    pub fn of(n: usize) -> Count {
        Count(n as u16)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub fn from_param(p: Option<Param>) -> Count {
        Count(p.map_or(0, |x| x.0).max(1))
    }
}

impl Width {
    /// East Asian Width, from the Unicode data pinned in `MANIFEST.toml`. No
    /// font input, ever — see [`DisplayWidth`] for why that is a wire contract
    /// rather than a choice.
    ///
    /// The ranges used to be written out by hand: five zero-width ranges and a
    /// dozen wide ones, each added when something looked wrong. That set was
    /// not the Unicode property it claimed to be — it covered
    /// `U+0300..U+036F` but not Arabic, Hebrew or Devanagari combining marks,
    /// so `بّ` cost two columns and a virama cost one. Against an `ncurses`
    /// peer computing with `wcwidth`, each of those is a cursor that has
    /// silently desynchronised. They are generated now, from
    /// `[[source: uax11]]`'s data file and the general category.
    ///
    /// ASCII returns before touching a table, which is the ~94% path.
    ///
    /// **~~`#[inline(never)]`, and measured.~~ Reversed, and the reason it was
    /// right is the reason it no longer is.**
    ///
    /// It read: "this is only ever reached from the `Utf8` arm of the state
    /// machine, so inlining it only grows `control`, which is reached from the
    /// `#[inline(always)]` ingest loop. Letting it inline cost ~5% of the
    /// streaming cells path (1046.8 [1042.9-1052.4] down to 994.0
    /// [985.1-1003.4], ranges not overlapping)."
    ///
    /// Both halves of that premise have since gone. The body was **three binary
    /// searches over 677 ranges** and is now two indexed loads, so there is
    /// almost nothing left to grow the caller with; and `fast_utf8` calls this
    /// directly rather than through `control`. Re-measured at HEAD, inlining is
    /// **+4.8%** on less-cjk, +4% on `payload::cjk`, +3.6% on vim and +3.4% on
    /// cat-cjk, with streaming, cat-scroll, git-log, less-pager and
    /// asciiquarium all flat -- the payloads the old number was taken on.
    ///
    /// Carried rather than deleted because the old measurement was correct when
    /// it was made. What dated it was two other changes, neither of which
    /// looked like it touched this decision.
    ///
    /// **Two indexed loads, not three binary searches.**
    ///
    /// This ran `in_ranges` over `ZERO`, then `WIDE`, then `AMBIGUOUS` -- up to
    /// three searches across 677 ranges, about eleven data-dependent branches,
    /// for every non-ASCII codepoint. It profiled at 9.4% of the CJK path, and
    /// a ceiling arm returning a constant measured **+32 to +34%** on cat-cjk,
    /// less-cjk and vim.
    ///
    /// The trie is generated and *exhaustively* checked against those same
    /// ranges for all 1,114,112 codepoints by `scripts/gen-width-tables.py`,
    /// which `width_tables_match_the_pinned_data` runs. A generated table
    /// nobody verifies is the "guessed table would look like it worked" failure
    /// this repo already records.
    /// It profiled at **6.7%** of `cat-cjk.raw` with its own leaf frame, which
    /// is the tell: an inlined two-load helper does not get one. The same shape
    /// was worth 11% on `View::clear_partners`, 7% on `ClusterBreak::is_break`
    /// and 6.5% on `View::fill_row`.
    #[inline(always)]
    pub fn of(c: Codepoint, ambiguous: AmbiguousWidth) -> Width {
        let cp = c.value();
        if cp < 0x80 {
            return if cp == 0 { Width::ZERO } else { Width::NARROW };
        }
        // `cp` is a `Codepoint`, so it is below 0x110000 by construction and
        // both indices are in range without a bounds check surviving.
        let block = width_data::WIDTH_INDEX[(cp >> 8) as usize] as usize;
        let byte = width_data::WIDTH_BLOCKS[block * 64 + ((cp as usize & 0xff) >> 2)];
        match (byte >> ((cp & 3) * 2)) & 3 {
            1 => Width::ZERO,
            2 => Width::WIDE,
            // East Asian Ambiguous. `[[source: uax11]]` § 5 says narrow when the
            // context is unavailable; for a core with no locale it always is, so
            // narrow is the default and this is the opt-in.
            3 if ambiguous == AmbiguousWidth::Wide => Width::WIDE,
            _ => Width::NARROW,
        }
    }
}

impl Cols {
    pub fn from_stride(n: usize) -> Cols {
        Cols(n.max(1))
    }
    pub fn clamp(self, max: usize) -> Cols {
        Cols(self.0.min(max).max(1))
    }
}

impl Rows {
    pub fn clamp(self, max: usize) -> Rows {
        Rows(self.0.min(max).max(1))
    }
}

impl Col {
    pub fn forward(self, c: Count, cols: Cols) -> Col {
        Col((self.0 + c.0 as usize).min(cols.0.saturating_sub(1)))
    }
    pub fn back(self, c: Count) -> Col {
        Col(self.0.saturating_sub(c.0 as usize))
    }
    pub fn next_tab(self, cols: Cols) -> Col {
        Col(((self.0 + 8) & !7).min(cols.0.saturating_sub(1)))
    }
}

impl Row {
    pub fn from_index(n: usize) -> Row {
        Row(n)
    }
    pub fn up(self, c: Count) -> Row {
        Row(self.0.saturating_sub(c.0 as usize))
    }
    pub fn down(self, c: Count, rows: Rows) -> Row {
        Row((self.0 + c.0 as usize).min(rows.0.saturating_sub(1)))
    }
    /// One past this row, for a half-open span. Mirrors [`Col::after`].
    pub fn after(self) -> Row {
        Row(self.0 + 1)
    }
}

impl Count {
    pub const ONE: Count = Count(1);
    /// Two cells: what a double-width character occupies, and therefore the
    /// size of the gap IRM must open for one.
    pub const TWO: Count = Count(2);
    pub fn get_raw(self) -> u16 {
        self.0
    }
    /// As many cells as a run of bytes will fill.
    ///
    /// Narrowing only, like [`Room::min`]: a `Room` is already bounded by the
    /// row, so this cannot manufacture a count larger than the line.
    pub fn from_room(r: Room) -> Count {
        Count(r.index().min(u16::MAX as usize) as u16)
    }
}

impl Col {
    pub fn at_index(i: usize) -> Col {
        Col(i)
    }
    /// One past this column, for a half-open span.
    pub fn after(self) -> Col {
        Col(self.0 + 1)
    }
    /// The column before this one, saturating at zero.
    pub fn before(self) -> Col {
        Col(self.0.saturating_sub(1))
    }
}

/// The column that **owns** the character occupying a position.
///
/// Not a `Col`, and the difference was a bug that stored data nothing could
/// read. A double-width character occupies two cells: its glyph is in the
/// lead and `Content::WIDE_TAIL` is in the column after it. A combining mark
/// or a ZWJ continuation belongs to the *character*, and a renderer draws the
/// character at its lead -- so a mark keyed on the tail is written, counted as
/// attached, and then unreachable, which is what every ZWJ emoji sequence did.
///
/// `Col::before` answers "one column back", which is the tail. This answers
/// "which cell does that position belong to", and it can only be obtained
/// from [`ViewRef::lead`], because the grid is the only thing that knows
/// whether a column holds a tail.
///
/// [`ViewRef::lead`]: crate::ViewRef::lead
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LeadCol(pub(crate) Col);

impl LeadCol {
    /// The underlying column, for addressing.
    pub fn col(self) -> Col {
        self.0
    }
}

/// A monotonic event counter. Not a `u64`: a statistic cannot be added to a
/// length, a byte position or a column, and nothing can be assigned into it
/// except by counting.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Stat(pub(crate) u64);

impl Stat {
    pub const ZERO: Stat = Stat(0);
    pub fn inc(&mut self) {
        self.0 += 1;
    }
    /// Add a bulk count.
    ///
    /// Write amplification is measured in *bytes*, and a run writes many cells
    /// at once -- counting one at a time would put an increment inside
    /// `put_ascii_run`'s loop, which is the loop this project already measured
    /// a 5% regression in for threading one runtime flag through it.
    pub fn add(&mut self, n: usize) {
        self.0 += n as u64;
    }
    pub fn count(self) -> u64 {
        self.0
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    /// Ratio against another counter, for hit rates. Saturating at zero rather
    /// than dividing by it.
    pub fn ratio_to(self, other: Stat) -> f64 {
        let d = self.0 + other.0;
        if d == 0 {
            0.0
        } else {
            self.0 as f64 / d as f64
        }
    }
}

impl Count {
    /// At least one, as a plain count of items. Named because `CSI A` with no
    /// parameter means 1, and that rule was being restated as `.max(1)` at each
    /// call site.
    pub fn at_least_one(self) -> usize {
        (self.0 as usize).max(1)
    }
}

impl ByteLen {
    /// Offset of a SIMD match within a scanned window.
    pub fn from_lane(n: usize) -> ByteLen {
        ByteLen::from_index(n)
    }
}
