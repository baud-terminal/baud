use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Display width of **one grapheme cluster**: 0 (combining), 1, or 2 (wide).
///
/// The only thing that may advance a [`Col`]. Obtainable from the width
/// tables -- [`Width::of`], [`Width::of_unicode8`] and the
/// [`crate::Classified`] entries generated from them -- from its three
/// constants, and back out of a written cell's flags
/// ([`crate::CellFlags::width_of`]), never from a byte count, so a byte
/// count can never reach the cursor by accident.
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

/// How many bytes one `read(2)` delivered.
///
/// **Separate from [`ByteLen`] because that one saturates.** `ByteLen` is a
/// `u16` and its `from_index` clamps at 65,535 -- correct for a length *inside*
/// a control sequence, where anything longer is malformed input rather than an
/// overflow. A read length is not that: it is a count the caller measured, and
/// clamping it silently drops bytes the terminal was given.
///
/// It did. Handing `feed` a 256 KB chunk through `ByteLen` parsed 65,535 bytes
/// and discarded the rest, so `cells_written` came out at 3,322,229 against
/// 8,325,429 for the same recording and read as a chunk-invariance bug in the
/// parser. The parser was right and the length was lost before it arrived. A
/// 64 KB chunk lost exactly one byte per read, which is the same failure
/// small enough to look like noise.
///
/// Bounded by the arena rather than by a word size, so the ceiling is the one
/// the caller can actually observe.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct ReadLen(usize);

impl ReadLen {
    pub const ZERO: ReadLen = ReadLen(0);
    /// One byte of a read, which is what most actions consume.
    pub const ONE: ReadLen = ReadLen(1);
    /// What a read actually placed. The single place a `len()` becomes a read
    /// length.
    pub fn of(s: &[u8]) -> ReadLen {
        ReadLen(s.len())
    }
    /// From a `read(2)` return value.
    pub fn from_index(n: usize) -> ReadLen {
        ReadLen(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
    /// What is left of the read after `n` bytes of it have been consumed.
    ///
    /// Saturating, so a step that overshot cannot wrap the loop bound into a
    /// huge positive number.
    pub fn less(self, n: ReadLen) -> ReadLen {
        ReadLen(self.0.saturating_sub(n.0))
    }
    pub fn plus(self, n: ReadLen) -> ReadLen {
        ReadLen(self.0 + n.0)
    }
    /// A character's encoded length, as an amount of the read.
    ///
    /// **One direction only.** A `ByteLen` is a `u16` and a `ReadLen` is not,
    /// so the conversion back would have to clamp -- and a clamp on the ingest
    /// loop is a silent truncation waiting to happen, which is the bug this
    /// type exists because of.
    pub fn of_bytes(n: ByteLen) -> ReadLen {
        ReadLen(n.index())
    }
    /// Whether `n` bytes are still to come. The one question the split test
    /// asks, so it is spelled once here rather than as a comparison at the
    /// call site with the operands free to be swapped.
    pub fn holds(self, n: ByteLen) -> bool {
        n.index() <= self.0
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
    /// **The four lengths UTF-8 allows**, as `const`s a compile-time table can
    /// hold. An action in the parse graph carries how many bytes it eats, and
    /// that value is also the length its decoder reads -- naming them once is
    /// what stops those being two literals free to disagree.
    pub const ONE: ByteLen = ByteLen(1);
    pub const TWO: ByteLen = ByteLen(2);
    pub const THREE: ByteLen = ByteLen(3);
    pub const FOUR: ByteLen = ByteLen(4);
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, zerocopy::FromZeros)]
#[repr(transparent)]
pub struct Col(pub(crate) usize);

impl Col {
    pub const ZERO: Col = Col(0);
    /// Only for interop with code that is not yet using [`Grid`]. Indexing a
    /// `Grid` takes a `Col` directly, so this should have no callers.
    ///
    /// Backed by `usize`, not `u16`: a `u16` cursor adds zero-extension and
    /// masking to every op on x86-64, at a cost unmeasured here. Narrow types
    /// belong in [`Cell`], where 10,000 of them sit in a matrix and size is the
    /// throughput lever — not in an arithmetic path.
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
    /// The same, against a **margin** rather than the screen edge.
    ///
    /// DECSLRM's right margin is where autowrap happens when one is set, and
    /// `baud-core2`'s two spellings of that rule disagreed: its `put_run`
    /// asked `room_until(cx, right_margin.after())` and its `put_glyph` asked
    /// `try_advance(w, cols)`, so ASCII wrapped at the margin and every
    /// non-ASCII character wrapped at the screen edge instead.
    ///
    /// The limit is `min(edge, cols)` unconditionally, which is exactly what
    /// `Cols::room_until` computed for that bulk path -- including for a cursor
    /// *past* the right margin, where the room is zero and the next character
    /// wraps. Adding a "outside the margins uses the screen edge" rule here
    /// was tried and is a third behaviour neither path had.
    pub fn try_advance_within(self, w: Width, cols: Cols, edge: Col) -> Option<Col> {
        let limit = edge.0.min(cols.raw());
        let next = self.0 + w.cells();
        if next > limit { None } else { Some(Col(next)) }
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, zerocopy::FromZeros)]
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, zerocopy::FromZeros)]
#[repr(transparent)]
pub struct Rows(pub(crate) usize);

impl Rows {
    pub(crate) fn from_ws(n: u16) -> Rows {
        Rows((n as usize).max(1))
    }
    /// No rows: the start of a range over physical rows, or a park that
    /// holds nothing yet. Not a geometry -- `of_index` clamps to one for
    /// that, because a screen with no rows has no cell to address.
    pub const ZERO: Rows = Rows(0);
    /// A count of rows that may be none -- rows hidden below a screen, rows
    /// a park holds -- as distinct from a geometry, which `of_index` keeps
    /// at one.
    pub fn of_count(n: usize) -> Rows {
        Rows(n)
    }
    /// A row count that is already known, such as the length of a slice the
    /// caller placed. `WinSize::geometry` is the path for geometry that came
    /// off the wire; this is for one that did not.
    pub fn of_index(n: usize) -> Rows {
        Rows(n.max(1))
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
    pub fn leading(rows: Rows, stride: Stride) -> RowBlock {
        RowBlock {
            start: 0,
            len: rows.index() * stride.index(),
        }
    }
    /// One row-sized block at row index `n`. Used for the style template table,
    /// where each style owns one row of pre-filled cells.
    pub fn nth(n: usize, stride: Stride) -> RowBlock {
        RowBlock {
            start: n * stride.index(),
            len: stride.index(),
        }
    }
    /// `rows` rows starting at row index `start`: a window into a ring that
    /// does not wrap around its end.
    pub fn from_row(start: Rows, rows: Rows, stride: Stride) -> RowBlock {
        RowBlock {
            start: start.index() * stride.index(),
            len: rows.index() * stride.index(),
        }
    }
    pub fn of<T>(self, buf: &[T]) -> &[T] {
        &buf[self.start..self.start + self.len]
    }
    pub fn of_mut<T>(self, buf: &mut [T]) -> &mut [T] {
        &mut buf[self.start..self.start + self.len]
    }
    /// How many entries the block spans, for a caller *allocating* it. Every
    /// other caller should hand the buffer to `of` or `fits` and never see
    /// the number.
    pub fn len(self) -> usize {
        self.len
    }
    /// Is the block empty? `clippy::len_without_is_empty` asks, and a block of
    /// no rows is a real state during a resize.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    /// Does a buffer hold this block? The one question a caller can ask
    /// without being handed the arithmetic back.
    pub fn fits<T>(self, buf: &[T]) -> bool {
        buf.len() >= self.start + self.len
    }
    /// Rotate a buffer left by this block's worth of entries.
    ///
    /// Takes the shift as a block rather than a count so the row count and the
    /// stride cannot be transposed at the call site -- which is a plausible
    /// number and a silently wrong grid.
    pub fn rotate_left_by<T>(self, buf: &mut [T], by: RowBlock) {
        self.of_mut(buf).rotate_left(by.len);
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

/// A value written to the wire as decimal, bounded to a byte.
///
/// **It exists to be un-confusable with [`OneBased`].** Both end up as decimal
/// digits in a reply and they are not interchangeable: this is at most three
/// digits and saturates the moment a value exceeds 255, where a coordinate can
/// be five digits and a right margin can sit past column 255. Passing a margin
/// to a byte-wide emitter does not fail -- it silently writes the wrong
/// number, which is the class this repo's audit exists to catch and the reason
/// `baud-core2`'s `emit_num` carried a comment saying it was "wider than
/// `emit_dec`".
///
/// Everything it wraps is a wire code an SGR or an `OSC 4` already defines as
/// a byte: an attribute's parameter, a font's `SGR 1x`, a colour introducer,
/// an indexed colour, an RGB channel.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
#[repr(transparent)]
pub struct WireByte(pub(crate) u8);

impl WireByte {
    pub const fn of(v: u8) -> WireByte {
        WireByte(v)
    }

    pub fn get(self) -> u8 {
        self.0
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
    /// The row, or `None` past the last one: for a margin, where
    /// `[[source: dec-std-070]]` ignores a control whose value is "greater
    /// than the number of lines in the Logical Display Page" (DECSTBM note
    /// 3, p. 5-25) rather than clamping it as [`Self::to_row`] does.
    pub fn row_within(self, rows: Rows) -> Option<Row> {
        ((self.0 as usize) <= rows.raw()).then(|| Row((self.0 - 1) as usize))
    }
    /// The column, or `None` past the last one: DECSLRM note 3 (p. 5-27)
    /// ignores a right margin "greater than the number of columns in the
    /// Logical Display Page".
    pub fn col_within(self, cols: Cols) -> Option<Col> {
        ((self.0 as usize) <= cols.raw()).then(|| Col((self.0 - 1) as usize))
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

/// **A position's index along a run of character positions**, in the
/// order of the character path and across lines: `baud-core`'s SEE extents
/// walk one, a page of them at most.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct PathIdx(pub(crate) usize);

impl PathIdx {
    pub const ZERO: PathIdx = PathIdx(0);
    pub fn at_index(i: usize) -> PathIdx {
        PathIdx(i)
    }
    pub fn index(self) -> usize {
        self.0
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
    /// The trie is compiled by `build/width.rs` and *exhaustively* checked,
    /// and `every_code_point_has_the_width_the_pinned_files_give` holds all
    /// 1,114,112 codepoints to the pinned files read apart. A generated table
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
            //
            // **Private use stays narrow under it**: xterm's default
            // `privateWidth`, the tailoring `[[source: uax11]]` § 2 invites.
            // The trie cannot say so -- its two bits are all used, and the
            // data gives private use `A` -- so the three ranges are tested
            // here, on the opt-in arm only. The write path reads
            // `Classified`, generated from this.
            3 if ambiguous == AmbiguousWidth::Wide && !c.is_private_use() => Width::WIDE,
            _ => Width::NARROW,
        }
    }

    /// **The width under Unicode 8's tables**, which iTerm2's
    /// `UnicodeVersion=8` selects (`[[source: iterm2-escape-codes]]`): East
    /// Asian Width 8.0's wide and ambiguous sets (`[[source: eaw-data-8]]`),
    /// zero width as [`Width::of`]. Not on the write path, which reads
    /// [`crate::Classified`], generated from this and [`Width::of`].
    pub fn of_unicode8(c: Codepoint, ambiguous: AmbiguousWidth) -> Width {
        let cp = c.value();
        if cp < 0x80 {
            return if cp == 0 { Width::ZERO } else { Width::NARROW };
        }
        let block = width_data::WIDTH8_INDEX[(cp >> 8) as usize] as usize;
        let byte = width_data::WIDTH8_BLOCKS[block * 64 + ((cp as usize & 0xff) >> 2)];
        match (byte >> ((cp & 3) * 2)) & 3 {
            1 => Width::ZERO,
            2 => Width::WIDE,
            3 if ambiguous == AmbiguousWidth::Wide && !c.is_private_use() => Width::WIDE,
            _ => Width::NARROW,
        }
    }
}

impl Cols {
    /// A width from a count. At least one: a grid with no columns has no cell
    /// to address, and every caller clamps to a geometry anyway.
    ///
    /// **Not a stride.** The row pitch is [`Stride`], a different type. The
    /// two were the same number for long enough that swapping them at a call
    /// site was invisible; now it does not compile.
    pub fn of_index(n: usize) -> Cols {
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
    /// `c` columns on, stopping at column `last`: [`Col::forward`] to a
    /// column rather than a width, for a line that ends before the screen.
    pub fn forward_to(self, c: Count, last: Col) -> Col {
        Col((self.0 + c.0 as usize).min(last.0))
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

/// **The cursor's column, and whether a move put it one past the right
/// margin.**
///
/// One past the right margin is two states under side margins narrower
/// than the screen. A write that filled the line to the margin leaves the
/// cursor there with a wrap pending, the sentinel the write path keeps; a
/// move that lands there leaves a cursor on that column, which writes on
/// from it (`[[source: dec-std-070]]` p. 5-140, where the margin bounds a
/// write only from at or inside it, and D.6.1, where every move resets the
/// Last Column Flag). **The sentinel stays the common case, and the move
/// gets a tag**, a high bit only [`CursorCol::placed`] sets: tagged, the
/// value is past every column, so the write path's own fit test sends it
/// out of line and the per-character path is unchanged. Note V's V10, the
/// option C2-min.
///
/// **A wrap kept waiting where the line no longer ends is a second tag**,
/// [`CursorCol::waiting`]: the margins `?69l` and `DECSTR` put back, a
/// resize and a `DECRC` keep a pending wrap on a column the sentinel cannot
/// name once the right margin moved (D.6.1 resets the flag only by a
/// control that moves the active position; `[[source: dec-std-070]]` p.
/// 5-30's `reset_left_right_margins_mode()` assigns no position). The value
/// is the column one past the one it waits on, the sentinel's, tagged so
/// the write path's fit test sends it out of line, where the wrap is taken
/// first. Unlike the placed tag it lives at full width too.
///
/// A plain [`Col`] is not one, so a reader that forgets to decode does not
/// compile: [`CursorCol::raw`] is for the write path past its fit test and
/// for code that tests it against the fence first, [`CursorCol::col`]
/// strips both tags, and [`CursorCol::at`] says which state it is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CursorCol(usize);

/// Where the cursor stands, read against the right margin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorAt {
    /// On a column: inside the fence, left of it, or right of it where a
    /// move or a write right of the fence left it.
    Column(Col),
    /// A write filled the line to where it ends -- the right margin, or the
    /// screen's last column for a write right of the fence -- and the
    /// cursor is one past it, the sentinel: with `DECAWM` set the next
    /// character wraps first. Or a wrap kept waiting on a column since the
    /// line's end moved ([`CursorCol::waiting`]), one past it likewise.
    Pending(Col),
    /// A move put the cursor exactly one past a right margin narrower than
    /// the screen: a column, the one the sentinel would otherwise name.
    Placed(Col),
}

impl CursorCol {
    /// The low bit of the high half, bit 32: a tagged value plus a
    /// character's two columns cannot overflow, one less than a tagged
    /// value is still tagged, and every column fits the low half.
    ///
    /// **In the high half so that decoding is free.** [`CursorCol::col`]
    /// keeps the low half, which a load from memory does by its width; at
    /// bit 62 it was an `and` on each zero-width character's path (the
    /// cold `attach` and `widen`), 2.9 M instructions over 200 reps of
    /// `cat-cjk`.
    const PLACED: usize = 1 << (usize::BITS / 2);
    /// The next bit, bit 33: [`CursorCol::waiting`]'s, which
    /// [`CursorCol::col`]'s mask strips with the placed one.
    const WAITING: usize = Self::PLACED << 1;
    pub const ZERO: CursorCol = CursorCol(0);

    /// Where a write left the cursor, the sentinel one past where the line
    /// ends included.
    #[inline(always)]
    pub fn written(c: Col) -> CursorCol {
        CursorCol(c.0)
    }
    /// A move that cannot land one past a right margin narrower than the
    /// screen: at or left of the margin, or with the margins at the
    /// screen's edges, where no move reaches one past the last column.
    #[inline(always)]
    pub fn moved(c: Col) -> CursorCol {
        CursorCol(c.0)
    }
    /// A move that may land one past the right margin: tagged there when
    /// that column is on the screen, which it is only while the margin is
    /// left of the last column.
    #[inline(always)]
    pub fn placed(c: Col, right: Col, cols: Cols) -> CursorCol {
        match c.0 == right.0 + 1 && c.0 < cols.0 {
            true => CursorCol(c.0 | Self::PLACED),
            false => CursorCol(c.0),
        }
    }
    /// **A wrap waiting on column `c`**, which the line no longer ends at:
    /// one past it, tagged. Only where the sentinel cannot say it -- a wrap
    /// on the line's last column is the sentinel ([`CursorCol::written`]).
    #[inline(always)]
    pub fn waiting(c: Col) -> CursorCol {
        CursorCol((c.0 + 1) | Self::WAITING)
    }
    /// The value undecoded, a tagged one past every column. For the write
    /// path after its fit test, which no tagged value passes, and for code
    /// that tests it against the fence before reading it as a column.
    #[inline(always)]
    pub fn raw(self) -> Col {
        Col(self.0)
    }
    /// The column, placed or not: one past the right margin for a pending
    /// wrap and a placed cursor alike, as every reader that names a column
    /// read the sentinel before the tag, and one past the column a kept
    /// wrap waits on.
    #[inline(always)]
    pub fn col(self) -> Col {
        Col(self.0 & (Self::PLACED - 1))
    }
    /// Whether a move put the cursor one past the right margin.
    pub fn is_placed(self) -> bool {
        self.0 & Self::PLACED != 0
    }
    /// Whether a wrap is kept waiting on a column the line no longer ends
    /// at ([`CursorCol::waiting`]).
    pub fn is_waiting(self) -> bool {
        self.0 & Self::WAITING != 0
    }
    /// One column left of an untagged value, for `BS` inside the fence:
    /// the raw arithmetic, as before the tags.
    #[inline(always)]
    pub fn before(self) -> CursorCol {
        CursorCol(self.0 - 1)
    }
    /// Which state the value is, under right margin `right` on a screen
    /// `cols` wide.
    pub fn at(self, right: Col, cols: Cols) -> CursorAt {
        let c = self.col();
        match (
            self.is_placed(),
            self.is_waiting() || c.0 == right.0 + 1 || c.0 >= cols.0,
        ) {
            (true, _) => CursorAt::Placed(c),
            (false, true) => CursorAt::Pending(c),
            (false, false) => CursorAt::Column(c),
        }
    }
    /// The column a report names: a pending wrap the column it waits on,
    /// anything else its own.
    pub fn reported(self, right: Col, cols: Cols) -> Col {
        match self.at(right, cols) {
            CursorAt::Pending(c) => Col(c.0 - 1),
            CursorAt::Column(c) | CursorAt::Placed(c) => c,
        }
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

/// How many cells a write touched.
///
/// **Its own type because it is the numerator of rule #1**, and because it is
/// not a [`Count`]: that is a VT parameter's repeat count and holds sixteen
/// bits, where a single read can blank thousands of cells a row. Conflating
/// them saturates the measurement silently, which is the failure mode this
/// project cares about most -- a counter that stops counting reads exactly
/// like work that stopped happening.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(transparent)]
pub struct Cells(usize);

impl Cells {
    pub const ZERO: Cells = Cells(0);
    pub fn of(n: usize) -> Cells {
        Cells(n)
    }
    pub fn plus(self, n: usize) -> Cells {
        Cells(self.0 + n)
    }
    pub fn index(self) -> usize {
        self.0
    }
}
