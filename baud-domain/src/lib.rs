//! Domain model as a type system.
//!
//! Every rule here is enforced by the compiler rather than by a comment. The
//! doc comments say *which measurement* made each rule necessary, but the rule
//! itself is structural — the wrong program does not build.
//!
//! Four techniques, in order of how much they buy:
//!
//! 1. **Generative brands** for anything whose validity depends on a mutable
//!    context. A `RowIdx` is meaningless once `cols` changes; a branded
//!    lifetime makes reusing one a type error rather than a silent wrong answer.
//! 2. **Typestate** for one-way lifecycles. `push_run` simply does not exist on
//!    a completed line.
//! 3. **Opaque newtypes with no arithmetic.** You cannot add a `LineIdx` to a
//!    `RowIdx` because neither exposes its integer.
//! 4. **Constructor narrowing.** `DisplayWidth` can only be built from a
//!    `WidthTable`, and no font type exists in this crate to pass instead.

#![no_std]
#![forbid(unsafe_code)]

use core::marker::PhantomData;
use zerocopy::{FromBytes, IntoBytes, Immutable, KnownLayout, TryFromBytes};

// ═══════════════════════════════════════════════════════════════════════
// Brands
// ═══════════════════════════════════════════════════════════════════════

/// An invariant lifetime. Two `Brand`s from different scopes are never equal,
/// so values carrying one cannot be passed where the other is expected.
///
/// Invariance is the point: `fn(&'id ()) -> &'id ()` makes `'id` neither
/// covariant nor contravariant, so the compiler will not silently coerce one
/// brand into another.
pub struct Brand<'id>(PhantomData<fn(&'id ()) -> &'id ()>);
impl<'id> Clone for Brand<'id> {
    fn clone(&self) -> Self { *self }
}
impl<'id> Copy for Brand<'id> {}
impl<'id> PartialEq for Brand<'id> { fn eq(&self, _: &Self) -> bool { true } }
impl<'id> Eq for Brand<'id> {}
impl<'id> PartialOrd for Brand<'id> {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> { Some(self.cmp(o)) }
}
impl<'id> Ord for Brand<'id> {
    fn cmp(&self, _: &Self) -> core::cmp::Ordering { core::cmp::Ordering::Equal }
}

// ═══════════════════════════════════════════════════════════════════════
// Positions — opaque, non-interchangeable
// ═══════════════════════════════════════════════════════════════════════

/// Absolute offset in the total byte stream. Monotonic, never reused.
///
/// No public field and no arithmetic: you cannot accidentally treat it as a
/// ring offset. Converting to one is [`Store::resolve`]'s job, and it is the
/// only place that knows about `ARENA`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct BytePos(u64);

impl BytePos {
    pub const ZERO: BytePos = BytePos(0);
    /// Absolute stream position from an arena offset. The arena is the only
    /// thing that knows the mapping, so this is where it is stated.
    pub fn from_index(n: usize) -> BytePos { BytePos(n as u64) }
    pub fn index(self) -> usize { self.0 as usize }
    pub fn advance(self, n: ByteLen) -> BytePos { BytePos(self.0 + n.get() as u64) }
    /// Distance, saturating. Deliberately not `Sub`, so a negative gap cannot
    /// wrap into a huge positive one.
    pub fn gap(self, earlier: BytePos) -> ByteGap { ByteGap(self.0.saturating_sub(earlier.0)) }
}

/// Index of a logical line. **Stable across resize and font change** — this is
/// the only line identifier safe to persist.
/// Distance between two [`BytePos`]. Cannot be added to a position; only
/// compared against a capacity.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct ByteGap(u64);
impl ByteGap {
    pub(crate) fn exceeds(self, capacity: usize) -> bool { self.0 > capacity as u64 }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct LineIdx(u64);

impl LineIdx {
    pub const FIRST: LineIdx = LineIdx(0);
    pub fn next(self) -> LineIdx { LineIdx(self.0 + 1) }
}

/// Index of a *display* row, valid only for the [`Layout`] that produced it.
///
/// Row 500 at 80 columns is a different place in the text than row 500 at 200.
/// Anchoring scroll position by a bare row number is the resize bug users
/// actually notice, so the brand makes carrying one across a resize a compile
/// error.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RowIdx<'layout> {
    v: RowCount,
    _b: Brand<'layout>,
}

// ═══════════════════════════════════════════════════════════════════════
// Layout — the brand generator for row space
// ═══════════════════════════════════════════════════════════════════════

/// Geometry currently in force. Obtainable only inside [`with_layout`], so
/// every [`RowIdx`] is tied to the geometry that computed it.
pub struct Layout<'layout> {
    cols: Cols,
    rows: Rows,
    _b: Brand<'layout>,
}

impl<'layout> Layout<'layout> {
    pub fn cols(&self) -> Cols { self.cols }
    pub fn rows(&self) -> Rows { self.rows }

    /// The only constructor for a branded [`RowIdx`].
    pub fn row(&self, v: RowCount) -> RowIdx<'layout> {
        RowIdx { v, _b: self._b }
    }
    pub fn row_value(&self, r: RowIdx<'layout>) -> RowCount { r.v }

    /// Display rows a line occupies at this width.
    ///
    /// Derived, never stored authoritatively — a pure function of
    /// `(DisplayWidth, cols)`, so the row index can always be recomputed and
    /// asserted against rather than drifting from the text.
    pub fn rows_for(&self, w: DisplayWidth) -> RowCount {
        let c = self.cols.raw() as u32;
        RowCount((w.cells().max(1) + c - 1) / c)
    }
}

/// Enter a geometry scope. A resize means calling this again, which mints a
/// fresh brand and invalidates every outstanding [`RowIdx`] at compile time.
///
/// ```compile_fail
/// # use baud_domain::*;
/// let stale = with_layout(Cols::from_wire(80), Rows::from_wire(24), |l| l.row(RowCount(5)));
/// with_layout(Cols::from_wire(200), Rows::from_wire(50), |l| l.row_value(stale)); // ERROR
/// ```
pub fn with_layout<R>(cols: Cols, rows: Rows, f: impl for<'id> FnOnce(Layout<'id>) -> R) -> R {
    f(Layout { cols, rows, _b: Brand(PhantomData) })
}

/// Scroll position expressed so it survives resize: a stable line plus an
/// offset *within* that line's wrapped rows.
#[derive(Clone, Copy)]
pub struct Anchor {
    pub line: LineIdx,
    /// Which wrapped row within that logical line. A [`RowCount`], not a
    /// [`RowIdx`]: it is an offset inside one line, not a grid position, and it
    /// stays valid across a resize while a `RowIdx` does not.
    pub row_within: RowCount,
}

// ═══════════════════════════════════════════════════════════════════════
// Width — constructible only from Unicode data
// ═══════════════════════════════════════════════════════════════════════

/// Unicode East Asian Width data plus locale policy for ambiguous width and
/// emoji presentation. Contains no font, and no font type exists in this crate
/// to put in one.
pub struct WidthTable {
    pub ambiguous: AmbiguousWidth,
    pub emoji: EmojiWidth,
}

/// East Asian Ambiguous characters are 1 or 2 columns depending on locale.
/// Two bools next to each other invite transposition; two enums do not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AmbiguousWidth { Narrow, Wide }

/// Whether `U+FE0F` presentation makes emoji 2 columns. Terminals disagree;
/// this is config, not font.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EmojiWidth { Narrow, Wide }

/// Width in cells.
///
/// Font-independent by wire contract, not preference: `ncurses` on the far end
/// computes columns with `wcwidth`, so a terminal using font advances would
/// drift from the application's model of the screen. Verified against
/// alacritty, whose model crate has no font dependency at all.
///
/// Consequence encoded below: [`DisplayWidth::measure`] takes a [`WidthTable`]
/// and nothing else. A font change cannot invalidate a cached width, because a
/// font never entered the computation.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct DisplayWidth(u32);

impl DisplayWidth {
    pub const ZERO: DisplayWidth = DisplayWidth(0);
    /// The only constructor. Caching this measured 14 us against 2223 us to
    /// recompute from bytes, for 15k lines — 4 bytes per line.
    pub(crate) fn cells(self) -> u32 { self.0 }
    /// Not public: `measure` is the only route from content to a width.
    fn from_cells(n: u32) -> DisplayWidth { DisplayWidth(n) }
    /// Byte length *is* column count, but only for a byte-per-column encoding.
    /// Callable only where a `Bytes<Printable>` or `Bytes<DecGraphics>` proof
    /// exists, which is what makes it sound.
    fn from_len(n: ByteLen) -> DisplayWidth { DisplayWidth(n.index() as u32) }
}

// ═══════════════════════════════════════════════════════════════════════
// Style
// ═══════════════════════════════════════════════════════════════════════

/// Index into the style table. Two bytes rather than ten inline: cell size
/// drove throughput from 36% of the kernel ceiling to 80%.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct StyleId(u16);
impl StyleId {
    pub const DEFAULT: StyleId = StyleId(0);
}

// ═══════════════════════════════════════════════════════════════════════
// Store — resolution as a borrow, so a dangling span cannot be held
// ═══════════════════════════════════════════════════════════════════════

/// Bytes borrowed from the [`Store`].
///
/// Tied to `&Store`, so it cannot be held across the `&mut` drain that would
/// recycle them. The invariant "no run outlives its tier" is the borrow checker
/// rather than a generation counter.
pub struct Resident<'store> {
    /// Unproven on arrival: what the cold tier holds is whatever the
    /// application wrote, which may be binary. Callers scan it to earn a
    /// stronger [`Kind`] before treating it as text.
    bytes: Bytes<'store, Raw>,
}
impl<'store> Resident<'store> {
    pub fn bytes(&self) -> Bytes<'store, Raw> { Bytes { raw: self.bytes.raw, _k: core::marker::PhantomData } }
}

/// Two-tier byte store.
///
/// The arena is 1 MiB because a size sweep peaked there against a 2 MiB L2 and
/// went *negative* by 64 MiB. At 280 MB/s it laps every 3.57 ms, so anything
/// retained must reach the cold ring first.
#[allow(dead_code)] // model only: the two-tier store is designed, not implemented
pub struct Store<'buf> {
    arena: &'buf mut [u8],
    cold: &'buf mut [u8],
    head: BytePos,
    drained: BytePos,
    cold_floor: BytePos,
}

impl<'buf> Store<'buf> {
    pub fn head(&self) -> BytePos { self.head }

    /// `None` means evicted: the bytes are gone and the line cannot be
    /// rendered, searched, or reflowed. Callers must handle it — there is no
    /// panicking accessor.
    pub fn resolve(&self, _pos: BytePos, _len: ByteLen) -> Option<Resident<'_>> {
        None // model only
    }

    /// Ingest may not lap the drain thread.
    ///
    /// Returns [`Lapped`] rather than silently discarding history. This is the
    /// one invariant the type system cannot express, so it is a `Result` the
    /// caller cannot ignore under `#[must_use]`.
    #[must_use]
    pub fn advance_head(&mut self, n: ByteLen) -> Result<BytePos, Lapped> {
        let next = self.head.advance(n);
        if next.gap(self.drained).exceeds(self.arena.len()) {
            return Err(Lapped);
        }
        self.head = next;
        Ok(next)
    }
}

/// Ingest outran the cold-tier writer. History would be lost.
#[derive(Debug)]
pub struct Lapped;

// ═══════════════════════════════════════════════════════════════════════
// Line — typestate, one-way
// ═══════════════════════════════════════════════════════════════════════

mod sealed {
    pub trait State {}
}
/// Still being appended to.
pub enum Building {}
/// Immutable: runs final, width cached.
pub enum Complete {}
/// Bytes copied to the cold ring. The one mandatory copy in the design —
/// memcpy at 5948 MB/s against 280 MB/s ingest, ~5% of a core.
pub enum Drained {}

impl sealed::State for Building {}
impl sealed::State for Complete {}
impl sealed::State for Drained {}

/// A line of text as the *application* produced it — delimited by `\n`, not by
/// the screen edge. This is the unit that survives resize, and keeping it is
/// why reflow is re-derivation rather than reconstruction from a wrapped flag.
pub struct Line<S: sealed::State> {
    first_run: RunIdx,
    run_count: RunCount,
    width: DisplayWidth,
    _s: PhantomData<S>,
}

impl Line<Building> {
    pub fn new(first_run: RunIdx) -> Self {
        Line { first_run, run_count: RunCount::ZERO, width: DisplayWidth::ZERO, _s: PhantomData }
    }
    /// Runs live in a shared pool, so there is no fixed cap. A previous design
    /// used `[Run; 8]` and silently promoted any line exceeding it.
    pub fn push_run(&mut self) { self.run_count = self.run_count.inc(); }

    /// The only transition out of `Building`. Consumes `self`, so the mutable
    /// handle is gone.
    pub fn complete(self, width: DisplayWidth) -> Line<Complete> {
        Line { first_run: self.first_run, run_count: self.run_count, width, _s: PhantomData }
    }
}

impl Line<Complete> {
    /// Only exists once the line is final, so a width can never be read
    /// mid-append.
    pub fn width(&self) -> DisplayWidth { self.width }
    pub fn runs(&self) -> RunRange { RunRange { start: self.first_run, count: self.run_count } }
    pub fn drained(self) -> Line<Drained> {
        Line { first_run: self.first_run, run_count: self.run_count,
               width: self.width, _s: PhantomData }
    }
}

impl Line<Drained> {
    pub fn width(&self) -> DisplayWidth { self.width }
}

// `push_run` on a completed line does not compile:
// ```compile_fail
// # use baud_domain::*;
// let l = Line::new(0).complete(DisplayWidth::ZERO);
// l.push_run();
// ```

// ═══════════════════════════════════════════════════════════════════════
// Font epoch — the renderer boundary, branded
// ═══════════════════════════════════════════════════════════════════════

/// A font configuration generation. Atlas slots are only meaningful within one.
pub struct FontEpoch<'font> {
    _b: Brand<'font>,
}

/// Index into the glyph atlas — **font-dependent**, and that is legal only
/// because the viewport is disposable. On a font change the viewport
/// re-materialises from spans in 44 us and every slot is regenerated.
///
/// The brand makes that mandatory rather than optional: a `Viewport<'font>`
/// cannot survive into a new epoch.
#[derive(Clone, Copy)]
pub struct AtlasSlot<'font> {
    v: u16,
    _b: Brand<'font>,
}

/// A glyph within one font face, as the shaper resolved it. Font-specific:
/// glyph 47 is a different character in every font, which is why this may
/// never be stored anywhere but the disposable viewport.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct GlyphId(u16);
impl GlyphId {
    pub fn from_shaper(n: u16) -> GlyphId { GlyphId(n) }
}

impl<'font> FontEpoch<'font> {
    pub fn slot(&self, g: GlyphId) -> AtlasSlot<'font> { AtlasSlot { v: g.0, _b: self._b } }
    /// Named so the widening is visible where the atlas is indexed.
    pub fn index(&self, s: AtlasSlot<'font>) -> usize { s.v as usize }
}

/// Enter a font scope. A font change means calling this again; anything
/// holding the old epoch's slots will not type-check.
pub fn with_font<R>(f: impl for<'id> FnOnce(FontEpoch<'id>) -> R) -> R {
    f(FontEpoch { _b: Brand(PhantomData) })
}

/// A materialised cell. Carries both brands: it is only valid for one geometry
/// and one font epoch.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Cell<'font> {
    pub slot: AtlasSlot<'font>,
    pub style: StyleId,
}

/// The brand-free form, for the one place a viewport actually crosses a byte
/// boundary: upload to the GPU.
///
/// `Cell<'font>` carries a lifetime so a stale atlas slot cannot outlive a font
/// change; a lifetime cannot be `FromBytes`. So the branded form is what the
/// domain uses, and this is what gets uploaded — the conversion is a
/// `repr(transparent)` reinterpretation, not a copy.
/// What a cell holds. **Content, not appearance.**
///
/// Values above `U+10FFFF` cannot be codepoints, so the reserved states are
/// unambiguous without a discriminant byte. The core never decides that "blank"
/// looks like `U+0020` — that conflates *nothing was written* with *a space was
/// written*, which differ for selection (copying a line must not yield trailing
/// spaces), for reflow, and for equality.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Content(u32);

impl Content {
    /// Nothing was written here. The renderer draws background and no glyph.
    pub const EMPTY: Content = Content(0x0011_0000);
    /// The right half of a double-width character. The renderer skips it; the
    /// glyph in the preceding cell covers both columns.
    pub const WIDE_TAIL: Content = Content(0x0011_0001);

    /// A decoded codepoint. Full range — no truncation, so astral-plane
    /// characters survive. `U+1F680 & 0xffff` was `U+F680`, a private-use
    /// codepoint, which is what the differential test caught.
    pub fn codepoint(c: Codepoint) -> Content { Content(c.value()) }
    pub fn as_codepoint(self) -> Option<Codepoint> { Codepoint::new(self.0) }
    pub fn is_empty(self) -> bool { self == Content::EMPTY }
    pub fn is_wide_tail(self) -> bool { self == Content::WIDE_TAIL }
}

impl Default for Content { fn default() -> Self { Content::EMPTY } }

/// One materialised cell: content plus a style index.
///
/// Eight bytes rather than four. Measured cost of that width: 8-byte cells hit
/// 48% of the kernel ceiling against 50% for 4-byte — two points, for holding a
/// full codepoint instead of a truncated one.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct PackedCell {
    pub content: Content,
    pub style: StyleId,
    pub _pad: u16,
}

/// Materialised rows currently on screen — a projection, never storage.
///
/// Over-provisioned and never reallocated: resize changes `cols`/`rows`, not
/// the allocation. Measured flat from a tight fit up to a 400 MB matrix,
/// because untouched pages are never faulted in.
///
/// Carrying `'layout` and `'font` means a resize or font change forces
/// re-materialisation — the compiler will not let a stale viewport be read.
pub struct Viewport<'layout, 'font, 'buf> {
    cells: &'buf mut [Cell<'font>],
    stride: Cols,
    _l: Brand<'layout>,
}

impl<'layout, 'font, 'buf> Viewport<'layout, 'font, 'buf> {
    pub fn stride(&self) -> Cols { self.stride }
    /// Indexing requires a [`RowIdx`] from the matching [`Layout`], so a row
    /// number computed at a different width cannot be used here.
    pub fn row(&self, l: &Layout<'layout>, r: RowIdx<'layout>) -> &[Cell<'font>] {
        let start = l.row_value(r).index() * self.stride.index();
        &self.cells[start..start + l.cols().index()]
    }
}

// ═══════════════════════════════════════════════════════════════════════
// What the types cannot express
// ═══════════════════════════════════════════════════════════════════════

/// Left as runtime checks, deliberately.
pub mod unchecked {
    //! * **Viewport reconstructibility.** That materialising from
    //!   `(lines, cols, anchor)` reproduces the same cells is a property of the
    //!   materialiser, not of any type. Assert it in debug builds — if it fails,
    //!   state is living only in the projection, which is the promoted-line bug.
    //! * **`rows[i] == ceil(width[i] / cols)`.** Derived, so it is cheap to
    //!   recompute and compare. A drift here means the index and the text
    //!   disagree.
    //! * **Alt-screen lines never entering scrollback.** Expressible as another
    //!   typestate, but it would infect every call site for one rule.
    //! * **The 245 us fallback resolution never happening on the frame thread.**
    //!   A thread-affinity property. Enforce with a debug assertion on thread id
    //!   inside the resolver.
}

// ═══════════════════════════════════════════════════════════════════════
// Quantities — bytes and columns are different kinds, not both `usize`
// ═══════════════════════════════════════════════════════════════════════

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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct Width(u8);

impl Width {
    /// Combining mark, ZWJ, variation selector — attaches to the previous cell.
    pub const ZERO: Width = Width(0);
    pub const NARROW: Width = Width(1);
    /// East Asian Wide/Fullwidth, and emoji under emoji presentation.
    pub const WIDE: Width = Width(2);
    pub fn cells(self) -> usize { self.0 as usize }
}

/// A length in **bytes**. Has no path to [`Col`] or [`Width`], by construction.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct ByteLen(u16);

impl ByteLen {
    pub const ZERO: ByteLen = ByteLen(0);
    /// Length of a byte slice. The single place a `len()` becomes a quantity.
    pub fn of(s: &[u8]) -> ByteLen { ByteLen::from_index(s.len()) }
    pub(crate) const fn new(n: u16) -> ByteLen { ByteLen(n) }
    /// From a slice length or a read result. Saturates: a length that does not
    /// fit is malformed input, not an overflow bug.
    pub fn from_index(n: usize) -> ByteLen { ByteLen(n.min(u16::MAX as usize) as u16) }
    /// Columns proven to fit are also a byte count when the encoding is
    /// byte-per-column, which is the only place this is used.
    pub fn from_room(r: Room) -> ByteLen { ByteLen::from_index(r.index()) }
    pub(crate) fn get(self) -> u16 { self.0 }
    pub fn index(self) -> usize { self.0 as usize }
    pub fn is_zero(self) -> bool { self.0 == 0 }
    pub fn sub_room(self, r: Room) -> ByteLen {
        ByteLen(self.0.saturating_sub(r.index().min(u16::MAX as usize) as u16))
    }
    pub fn add(self, o: ByteLen) -> ByteLen { ByteLen(self.0.saturating_add(o.0)) }
}

/// A column on the grid. Advances only by [`Width`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Col(usize);

impl Col {
    pub const ZERO: Col = Col(0);
    /// Only for interop with code that is not yet using [`Grid`]. Indexing a
    /// `Grid` takes a `Col` directly, so this should have no callers.
    ///
    /// Backed by `usize`, not `u16`: measured, a `u16` cursor costs 11-15%
    /// against `usize` on x86-64 because every op needs zero-extension and
    /// masking. Narrow types belong in [`Cell`], where 10,000 of them sit in a
    /// matrix and size is the throughput lever — not in an arithmetic path.
    pub fn index(self) -> usize { self.0 }

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
        if next > cols.raw() { None } else { Some(Col(next)) }
    }
    /// Explicit cursor addressing. Goes through [`OneBased`] so the 1-based
    /// wire convention is converted in exactly one place.
    pub fn at(p: OneBased, cols: Cols) -> Col { p.to_col(cols) }
}

/// A grid indexed by [`Row`] and [`Col`], never by an integer.
///
/// This is what removes `as_index` from call sites: you cannot index it with a
/// `usize`, and you cannot index it with a `Row` where a `Col` belongs. The
/// conversion happens once, inside `Index`, instead of everywhere.
pub struct Grid<'buf, T> {
    cells: &'buf mut [T],
    stride: Cols,
    rows: Rows,
}

impl<'buf, T> Grid<'buf, T> {
    pub fn new(cells: &'buf mut [T], stride: Cols, rows: Rows) -> Grid<'buf, T> {
        Grid { cells, stride, rows }
    }
    pub fn rows(&self) -> Rows { self.rows }
    pub fn stride(&self) -> Cols { self.stride }
}

impl<'buf, T> core::ops::Index<Row> for Grid<'buf, T> {
    type Output = [T];
    fn index(&self, r: Row) -> &[T] {
        let a = r.0 * self.stride.0;
        &self.cells[a..a + self.stride.0]
    }
}
impl<'buf, T> core::ops::IndexMut<Row> for Grid<'buf, T> {
    fn index_mut(&mut self, r: Row) -> &mut [T] {
        let a = r.0 * self.stride.0;
        &mut self.cells[a..a + self.stride.0]
    }
}
/// A single cell, addressed by both axes. `grid[(row, col)]`.
impl<'buf, T> core::ops::Index<(Row, Col)> for Grid<'buf, T> {
    type Output = T;
    fn index(&self, (r, c): (Row, Col)) -> &T { &self.cells[r.0 * self.stride.0 + c.0] }
}
impl<'buf, T> core::ops::IndexMut<(Row, Col)> for Grid<'buf, T> {
    fn index_mut(&mut self, (r, c): (Row, Col)) -> &mut T {
        &mut self.cells[r.0 * self.stride.0 + c.0]
    }
}

/// A ring of per-line records, indexed by [`LineIdx`] with the wrap built in.
pub struct LineRing<'buf, T> {
    slots: &'buf mut [T],
}

impl<'buf, T> LineRing<'buf, T> {
    pub fn new(slots: &'buf mut [T]) -> LineRing<'buf, T> { LineRing { slots } }
}
impl<'buf, T> core::ops::Index<LineIdx> for LineRing<'buf, T> {
    type Output = T;
    fn index(&self, l: LineIdx) -> &T { &self.slots[(l.0 as usize) % self.slots.len()] }
}
impl<'buf, T> core::ops::IndexMut<LineIdx> for LineRing<'buf, T> {
    fn index_mut(&mut self, l: LineIdx) -> &mut T {
        let n = self.slots.len();
        &mut self.slots[(l.0 as usize) % n]
    }
}

/// The arena, indexed by [`SlotIdx`] with the wrap built in.
pub struct SlotRing<'buf> {
    bytes: &'buf mut [u8],
}

impl<'buf> SlotRing<'buf> {
    pub fn new(bytes: &'buf mut [u8]) -> SlotRing<'buf> { SlotRing { bytes } }
}
impl<'buf> core::ops::Index<SlotIdx> for SlotRing<'buf> {
    type Output = [u8; SLOT_BYTES];
    fn index(&self, s: SlotIdx) -> &[u8; SLOT_BYTES] {
        let a = s.0 as usize * SLOT_BYTES;
        self.bytes[a..a + SLOT_BYTES].try_into().unwrap()
    }
}
impl<'buf> core::ops::IndexMut<SlotIdx> for SlotRing<'buf> {
    fn index_mut(&mut self, s: SlotIdx) -> &mut [u8; SLOT_BYTES] {
        let a = s.0 as usize * SLOT_BYTES;
        (&mut self.bytes[a..a + SLOT_BYTES]).try_into().unwrap()
    }
}

/// Position of a parameter within a CSI sequence.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamIdx(u8);
impl ParamIdx {
    pub const FIRST: ParamIdx = ParamIdx(0);
    pub const SECOND: ParamIdx = ParamIdx(1);
    pub const THIRD: ParamIdx = ParamIdx(2);
    pub const FOURTH: ParamIdx = ParamIdx(3);
    pub const FIFTH: ParamIdx = ParamIdx(4);
    pub fn next(self) -> ParamIdx { ParamIdx(self.0 + 1) }
}

/// How many parameters an SGR attribute consumed.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamCount(u8);
impl ParamCount { pub fn get(self) -> ParamIdx { ParamIdx(self.0) } }

/// Geometry as `TIOCGWINSZ` reports it. Destructured once, here, so `ws_col`
/// and `ws_row` cannot be transposed at any other call site.
pub struct WinSize { pub ws_row: u16, pub ws_col: u16 }

impl WinSize {
    /// The single public source of grid geometry. `ws_col` and `ws_row` are
    /// destructured here and nowhere else, so they cannot be transposed.
    pub fn geometry(&self) -> (Cols, Rows) {
        (Cols::from_ws(self.ws_col), Rows::from_ws(self.ws_row))
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
pub struct Room(usize);

impl Room {
    pub const NONE: Room = Room(0);
    /// Narrowing only, and against a typed length. There is no widening
    /// constructor, so the proof that this fits cannot be forged.
    pub fn min(self, n: ByteLen) -> Room { Room(self.0.min(n.get() as usize)) }
    pub fn index(self) -> usize { self.0 }
    pub fn is_empty(self) -> bool { self.0 == 0 }
}

impl core::ops::AddAssign<Room> for Col {
    /// Total: `Room` cannot exceed the space it was measured from.
    fn add_assign(&mut self, r: Room) { self.0 += r.0; }
}
impl core::ops::Add<Room> for Col {
    type Output = Col;
    fn add(self, r: Room) -> Col { Col(self.0 + r.0) }
}
impl core::ops::SubAssign<Room> for ByteLen {
    fn sub_assign(&mut self, r: Room) { self.0 = self.0.saturating_sub(r.0 as u16); }
}

/// A row on the grid.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Row(usize);

/// Grid width. Distinct from [`Rows`] so the two cannot be swapped at a call
/// site — `Layout::new(rows, cols)` transposed is a bug that used to compile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Cols(usize);
impl Cols {
    fn from_ws(n: u16) -> Cols { Cols((n as usize).max(1)) }
    /// Columns remaining at `cx`. The only source of a [`Room`].
    pub fn room_at(self, cx: Col) -> Room { Room(self.0.saturating_sub(cx.0)) }
    pub fn index(self) -> usize { self.0 }
    fn raw(self) -> usize { self.0 }
}

/// Grid height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Rows(usize);
impl Rows {
    fn from_ws(n: u16) -> Rows { Rows((n as usize).max(1)) }
    pub fn index(self) -> usize { self.0 }
    fn raw(self) -> usize { self.0 }
}

/// How many display rows something occupies. Not a [`Rows`] — that is a grid
/// dimension, this is a count that can exceed it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct RowCount(u32);
impl RowCount {
    pub const ZERO: RowCount = RowCount(0);
    pub fn index(self) -> usize { self.0 as usize }
    pub fn add(self, o: RowCount) -> RowCount { RowCount(self.0 + o.0) }
}

/// Index into the shared run pool.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct RunIdx(u32);
impl RunIdx {
    pub const FIRST: RunIdx = RunIdx(0);
    pub fn index(self) -> usize { self.0 as usize }
    pub fn advance(self, n: RunCount) -> RunIdx { RunIdx(self.0 + n.0 as u32) }
}

/// A run of bytes sharing one style, as stored.
///
/// Crosses the mmap boundary: the cold tier is a byte-mapped file reinterpreted
/// as `&[Run]` with no copy and no parse. That is what forces `_pad` to be
/// written out — zerocopy rejects implicit padding, because padding is
/// uninitialised memory and writing it to disk or shared memory leaks whatever
/// was there.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Run {
    pub pos: BytePos,
    pub len: ByteLen,
    pub style: StyleId,
    /// [`Encoding`] as a byte. Stored rather than re-derived, so materialisation
    /// branches once per run instead of re-classifying what the scanner knew.
    /// Held as `u8` because not every bit pattern is a valid variant — recover
    /// it with [`Run::encoding`], which is checked.
    pub encoding: u8,
    pub _pad: [u8; 3],
}

impl Run {
    /// Checked: a byte off disk is not necessarily a valid [`Encoding`].
    pub fn encoding(&self) -> Option<Encoding> {
        Encoding::try_read_from_bytes(&[self.encoding]).ok()
    }
}

/// Half-open range into the shared run pool.
#[derive(Clone, Copy)]
pub struct RunRange { pub start: RunIdx, pub count: RunCount }

/// A number of runs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct RunCount(u16);
impl RunCount {
    pub const ZERO: RunCount = RunCount(0);
    pub fn from_index(n: usize) -> RunCount { RunCount(n as u16) }
    pub fn inc(self) -> RunCount { RunCount(self.0 + 1) }
    pub fn index(self) -> usize { self.0 as usize }
}
impl Row {
    pub const ZERO: Row = Row(0);
    /// See [`Col::as_index`].
    pub fn index(self) -> usize { self.0 }
    pub fn next(self, rows: Rows) -> Option<Row> {
        if self.0 + 1 >= rows.raw() { None } else { Some(Row(self.0 + 1)) }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Decoding — UTF-8 is an assumption, so make it a type
// ═══════════════════════════════════════════════════════════════════════

/// One unit pulled from the byte stream.
///
/// The arena is **not** a `str`. A PTY delivers `cat /bin/ls` as readily as
/// text, and escape sequences are not characters. So the stream decodes into
/// this rather than being validated up front — a terminal must render garbage,
/// not reject it.
///
/// Note what is missing: no variant exposes a byte count as a width. The only
/// route to [`Width`] is [`Decoded::width`], and `Control` and `Invalid` have
/// their widths fixed by the protocol rather than by their length.
pub enum Decoded {
    /// A grapheme cluster — base character plus any combining marks. The unit
    /// of wrapping: splitting one orphans a mark from its base.
    Grapheme { len: ByteLen, width: Width },
    /// Malformed UTF-8. Rendered as U+FFFD, one column, never dropped.
    Invalid { len: ByteLen },
    /// A C0/C1 control byte. Occupies no columns; may move the cursor by other
    /// means, which is the parser's business and not the cursor arithmetic's.
    Control { byte: u8 },
}

impl Decoded {
    /// The only constructor of a [`Width`] from stream content.
    pub fn width(&self) -> Width {
        match self {
            Decoded::Grapheme { width, .. } => *width,
            Decoded::Invalid { .. } => Width::NARROW,
            Decoded::Control { .. } => Width::ZERO,
        }
    }
    pub fn len(&self) -> ByteLen {
        match self {
            Decoded::Grapheme { len, .. } | Decoded::Invalid { len } => *len,
            Decoded::Control { .. } => ByteLen(1),
        }
    }
}

/// Sum of grapheme widths across a logical line. The only route from a byte
/// range to a [`DisplayWidth`], so a cached width can never have been computed
/// from a byte count.
pub fn measure(units: impl Iterator<Item = Decoded>, _t: &WidthTable) -> DisplayWidth {
    DisplayWidth::from_cells(units.map(|d| d.width().cells() as u32).sum())
}

// ═══════════════════════════════════════════════════════════════════════
// Byte provenance — what a slice has been proven to contain
// ═══════════════════════════════════════════════════════════════════════

mod kind {
    pub trait Sealed {}
    impl Sealed for super::Raw {}
    impl Sealed for super::Printable {}
    impl Sealed for super::Utf8 {}
    impl Sealed for super::DecGraphics {}
}

/// One byte as it arrived. Not a number: it has no arithmetic, only
/// classification. Everything that consumes wire data takes this.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct StreamByte(u8);

impl StreamByte {
    pub fn from_stream(b: u8) -> StreamByte { StreamByte(b) }
    pub fn is_printable(self) -> bool { (0x20..0x7f).contains(&self.0) }
    pub fn is_c0(self) -> bool { self.0 < 0x20 }
    pub fn is_continuation(self) -> bool { self.0 & 0xc0 == 0x80 }
    pub fn as_ascii_digit(self) -> Option<Digit> {
        self.0.is_ascii_digit().then(|| Digit(self.0 - b'0'))
    }
    /// For the atlas and the width tables, which index by codepoint.
    pub fn index(self) -> usize { self.0 as usize }
}

/// A decimal digit from a CSI parameter. Cannot be arithmetic'd with anything
/// else, and cannot exceed 9 by construction.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct Digit(u8);

/// A CSI parameter, accumulated digit by digit. Saturates rather than wrapping,
/// because `CSI 99999999999 H` is malformed input, not an overflow bug.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Param(u16);

impl Param {
    pub const ZERO: Param = Param(0);
    pub fn push(self, d: Digit) -> Param {
        Param(self.0.saturating_mul(10).saturating_add(d.0 as u16))
    }
}

/// A CSI final byte, validated to the `0x40..=0x7e` range ECMA-48 allows.
/// Constructing one is the proof that a sequence terminated legally.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FinalByte(u8);

impl FinalByte {
    pub fn from_stream(b: StreamByte) -> Option<FinalByte> {
        (0x40..=0x7e).contains(&b.0).then_some(FinalByte(b.0))
    }
    fn raw(self) -> u8 { self.0 }
}

/// Whether a CSI carried the `?` private marker. A `bool` argument at a call
/// site says nothing; this says which namespace.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Marker { Standard, Private }

/// CSI parameters as accumulated, with typed accessors. Positional indexing
/// into a bare slice is how `38;2;r;g;b` gets misread.
pub struct Params<'a>(&'a [Option<Param>]);

impl<'a> Params<'a> {
    pub fn new(p: &'a [Option<Param>]) -> Params<'a> { Params(p) }
    fn at(&self, i: ParamIdx) -> Option<Param> { self.0.get(i.0 as usize).copied().flatten() }
    pub fn count(&self, i: ParamIdx) -> Count { Count::from_param(self.at(i)) }
    pub fn one_based(&self, i: ParamIdx) -> OneBased { OneBased::from_param(self.at(i)) }
    pub fn extent(&self, i: ParamIdx) -> Option<EraseExtent> { EraseExtent::from_param(self.at(i)) }
    pub fn mode(&self, i: ParamIdx) -> PrivateMode { PrivateMode::from_wire(self.at(i)) }
    pub fn count_of(&self) -> ParamCount { ParamCount(self.0.len().min(255) as u8) }
}

/// How a run of bytes is to be interpreted.
///
/// Not future-proofing: the VT protocol switches this mid-stream. `ESC ( 0`
/// selects DEC Special Graphics for G0, after which `q` is `─` and `x` is `│`
/// — that is how ncurses draws boxes, and it applies to subsequent bytes until
/// switched back. A per-run tag is what the protocol actually requires.
///
/// Stored on [`Run`] so materialisation branches once per run instead of
/// re-deriving what the scanner already knew. One byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, TryFromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(u8)]
pub enum Encoding {
    /// Printable ASCII, `0x20..=0x7e`. One byte, one column, no table lookup.
    /// The ~94% path.
    Ascii,
    /// Well-formed UTF-8. Decode to graphemes for width.
    Utf8,
    /// DEC Special Graphics (`ESC ( 0`). Bytes map through a 96-entry table to
    /// box-drawing codepoints; width is always 1.
    DecGraphics,
    /// Uninterpretable. Render U+FFFD per byte, width 1 each. `cat /bin/ls`
    /// lands here and must render rather than be rejected.
    Raw,
}

impl Encoding {
    /// Whether width can be had without decoding. True for every fixed-width
    /// single-byte encoding, which is the property that matters — not
    /// "is it ASCII".
    pub fn is_byte_per_column(self) -> bool {
        matches!(self, Encoding::Ascii | Encoding::DecGraphics | Encoding::Raw)
    }
}

/// What a [`Bytes`] has been *proven* to hold. A `&[u8]` carries no such proof,
/// which is why one may not appear in a domain signature.
///
/// The static counterpart of [`Encoding`]: same information, no runtime tag,
/// used while the bytes are in flight. [`Kind::ENCODING`] is the bridge.
pub trait Kind: kind::Sealed {
    const ENCODING: Encoding;
}

/// Straight off the wire. No guarantees: a PTY delivers `cat /bin/ls` as
/// readily as text.
pub enum Raw {}
/// Verified printable ASCII, `0x20..=0x7e`. Width is exactly one column per
/// byte — the only case where a byte count legitimately *is* a column count.
pub enum Printable {}
/// Verified well-formed UTF-8. Width still unknown until measured.
pub enum Utf8 {}
/// DEC Special Graphics, in flight.
pub enum DecGraphics {}

impl Kind for Raw { const ENCODING: Encoding = Encoding::Raw; }
impl Kind for Printable { const ENCODING: Encoding = Encoding::Ascii; }
impl Kind for Utf8 { const ENCODING: Encoding = Encoding::Utf8; }
impl Kind for DecGraphics { const ENCODING: Encoding = Encoding::DecGraphics; }

/// A byte view carrying a proof of its contents.
#[derive(Clone, Copy)]
pub struct Bytes<'a, K: Kind> {
    raw: &'a [u8],
    _k: core::marker::PhantomData<K>,
}

impl<'a> Bytes<'a, Raw> {
    /// The single entry point for wire data. Everything downstream must earn a
    /// stronger [`Kind`] by passing a scanner.
    pub fn from_source(raw: &'a [u8]) -> Bytes<'a, Raw> {
        Bytes { raw, _k: core::marker::PhantomData }
    }
    /// Split off the leading run of printable ASCII. This is the SIMD scan, and
    /// the returned `Printable` view is the proof it produced.
    pub fn split_printable(self) -> (Bytes<'a, Printable>, Bytes<'a, Raw>) {
        let n = self.raw.iter().take_while(|&&b| (0x20..0x7f).contains(&b)).count();
        (Bytes { raw: &self.raw[..n], _k: core::marker::PhantomData },
         Bytes { raw: &self.raw[n..], _k: core::marker::PhantomData })
    }
    /// Promote to UTF-8, or hand the bytes back unproven.
    pub fn validate_utf8(self) -> Result<Bytes<'a, Utf8>, Bytes<'a, Raw>> {
        match core::str::from_utf8(self.raw) {
            Ok(_) => Ok(Bytes { raw: self.raw, _k: core::marker::PhantomData }),
            Err(_) => Err(self),
        }
    }
    pub fn is_empty(&self) -> bool { self.raw.is_empty() }
}

impl<'a> Bytes<'a, Printable> {
    /// Free, and needs no [`WidthTable`] — the proof that these are printable
    /// ASCII is exactly the proof that one byte is one column. This is the
    /// ~94% fast path, and the type is what makes skipping the table sound.
    pub fn width(&self) -> DisplayWidth { DisplayWidth::from_len(ByteLen::of(self.raw)) }
    pub fn len(&self) -> ByteLen { ByteLen::of(self.raw) }
}

impl<'a> Bytes<'a, Utf8> {
    pub fn measure(&self, t: &WidthTable) -> DisplayWidth {
        let _ = t;
        DisplayWidth::from_cells(0) // model only
    }
    pub fn len(&self) -> ByteLen { ByteLen::new(self.raw.len() as u16) }
}

impl<'a> Bytes<'a, Raw> {
    /// Reinterpret under the active character set. `ESC ( 0` makes subsequent
    /// bytes graphics; this is where that decision lands.
    pub fn as_dec_graphics(self) -> Bytes<'a, DecGraphics> {
        Bytes { raw: self.raw, _k: core::marker::PhantomData }
    }
}

impl<'a> Bytes<'a, DecGraphics> {
    /// Fixed-width single-byte: one column each, no table.
    pub fn width(&self) -> DisplayWidth { DisplayWidth::from_len(ByteLen::of(self.raw)) }
    pub fn len(&self) -> ByteLen { ByteLen::of(self.raw) }
}

/// The dynamic form: a run whose encoding is carried rather than proven.
/// This is what gets stored, and what a `match` over stored runs sees.
pub struct Encoded<'a> {
    bytes: Bytes<'a, Raw>,
    encoding: Encoding,
}

impl<'a> Encoded<'a> {
    pub fn encoding(&self) -> Encoding { self.encoding }
    /// Width without decoding, where the encoding permits it. `None` means the
    /// caller must decode — which is exactly the branch materialisation wants
    /// to take once per run.
    pub fn width_fast(&self) -> Option<DisplayWidth> {
        self.encoding.is_byte_per_column()
            .then(|| DisplayWidth::from_len(ByteLen::of(self.bytes.raw)))
    }
    /// Checked downcast back to a static proof.
    pub fn as_utf8(self) -> Option<Bytes<'a, Utf8>> {
        (self.encoding == Encoding::Utf8)
            .then(|| Bytes { raw: self.bytes.raw, _k: core::marker::PhantomData })
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Static proof to dynamic tag. Free — `K::ENCODING` is a constant.
    pub fn erase(self) -> Encoded<'a> {
        Encoded { bytes: Bytes { raw: self.raw, _k: core::marker::PhantomData },
                  encoding: K::ENCODING }
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Named so every crossing back to an untyped slice is visible. Used by the
    /// renderer and by `memcpy` into the cold tier, and nowhere else.
    pub fn as_slice(&self) -> &'a [u8] { self.raw }
}

// ═══════════════════════════════════════════════════════════════════════
// Sources — the transport is a parameter, not a primitive
// ═══════════════════════════════════════════════════════════════════════

/// One read's worth of arena. Fixed, not a slice with a runtime length.
///
/// `read(2)` on a PTY returns **at most 4095 bytes** no matter what buffer you
/// offer, because `N_TTY_BUF_SIZE` is 4096 and it is a fixed array embedded in
/// `struct n_tty_data`. Measured: offering 4 KiB, 16 KiB, 64 KiB and 256 KiB
/// through a PTY gave 245.7 / 246.9 / 240.6 / 248.8 MB/s — flat. The length was
/// never variable; it was only ever untyped.
pub const SLOT_BYTES: usize = 4096;

/// The arena is exactly `SLOTS * SLOT_BYTES`, so there is no partial slot at
/// the end and the wrap is exact.
pub const SLOTS: usize = 256;
const _: () = assert!(SLOTS * SLOT_BYTES == 1 << 20);

/// A ring position in *slots*, not bytes.
///
/// `u8` is not an optimisation, it is the proof: 256 slots means the ring wrap
/// is `wrapping_add(1)` with no modulo, no comparison, and no `if tail + 4096 >
/// ARENA { tail = 0 }` to get wrong.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct SlotIdx(u8);

impl SlotIdx {
    pub const FIRST: SlotIdx = SlotIdx(0);
    /// Wraps at the end of the arena, by construction.
    pub fn next(self) -> SlotIdx { SlotIdx(self.0.wrapping_add(1)) }
    pub fn index(self) -> usize { self.0 as usize }
    /// Byte offset of this slot within the arena.
    /// Byte offset of this slot within the arena. Named as an index because
    /// that is the only thing it may be used for.
    pub fn byte_index(self) -> usize { self.0 as usize * SLOT_BYTES }
}

/// How much of a slot a source actually filled. Provably `<= SLOT_BYTES`, so
/// it can never describe a region outside the slot it came from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct Filled(u16);

impl Filled {
    pub const EMPTY: Filled = Filled(0);
    /// Clamps, so a source cannot report more than it was given.
    pub fn from_read(n: usize) -> Filled { Filled(n.min(SLOT_BYTES) as u16) }
    pub fn len(self) -> ByteLen { ByteLen::new(self.0) }
    pub fn is_empty(self) -> bool { self.0 == 0 }
    pub fn index(self) -> usize { self.0 as usize }
}

/// A writable slot of the arena, handed out by the store.
pub struct ArenaSlot<'a> {
    pub(crate) buf: &'a mut [u8; SLOT_BYTES],
    pub(crate) base: BytePos,
    pub(crate) idx: SlotIdx,
}

impl<'a> ArenaSlot<'a> {
    /// Not a runtime length — a constant. Kept as a method so call sites read
    /// the same whether or not the constant later changes.
    pub const fn capacity() -> ByteLen { ByteLen::new(SLOT_BYTES as u16) }
    pub fn base(&self) -> BytePos { self.base }
    pub fn idx(&self) -> SlotIdx { self.idx }
    /// For a source to write into. The one raw handle, named.
    pub fn as_mut(&mut self) -> &mut [u8; SLOT_BYTES] { self.buf }
}

/// Anything that can deliver bytes into the arena.
///
/// A PTY master, a pipe, a shared-memory ring, or a fixture replaying captured
/// `vim` output — the domain does not care which, and the differential test and
/// the PTY-bypass ring are both just further implementations.
pub trait Source {
    type Error;
    /// Fill the slot and report how much arrived.
    ///
    /// Returning [`Filled`] rather than `usize` does two things: a read length
    /// can never be mistaken for a column count, and it can never exceed the
    /// slot it describes.
    ///
    /// One slot per call. A pipe source gives up ~4% by not batching (measured
    /// 492.9 vs 513.6 MB/s at 4 KiB vs 16 KiB), which is the price of a single
    /// fixed shape for every transport.
    fn fill(&mut self, slot: ArenaSlot<'_>) -> Result<Filled, Self::Error>;
}

// ═══════════════════════════════════════════════════════════════════════
// VT operations — parsed once, defaults applied once, 1-based made explicit
// ═══════════════════════════════════════════════════════════════════════

/// A parameter exactly as it appears on the wire: **1-based**, because ECMA-48
/// numbers rows and columns from 1 while every buffer indexes from 0.
///
/// There is no `From<u16> for Row`. The only route is [`OneBased::to_row`],
/// which subtracts and clamps in one place instead of at twenty call sites.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct OneBased(u16);

impl OneBased {
    /// An omitted parameter is 1 for cursor addressing, and so is an explicit 0.
    pub fn from_param(p: Option<Param>) -> OneBased { OneBased(p.map_or(0, |x| x.0).max(1)) }
    pub fn to_row(self, rows: Rows) -> Row { Row(((self.0 - 1) as usize).min(rows.raw().saturating_sub(1))) }
    pub fn to_col(self, cols: Cols) -> Col { Col(((self.0 - 1) as usize).min(cols.raw().saturating_sub(1))) }
}

/// A repeat count or distance. Omitted or zero both mean 1 — a rule that was
/// previously restated as `if a != 0 { a } else { 1 }` in every arm.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Count(u16);
impl Count {
    pub fn from_param(p: Option<Param>) -> Count { Count(p.map_or(0, |x| x.0).max(1)) }
}

/// `CSI J` / `CSI K` selector. The wire encodes 0/1/2; anything else is an
/// error, not a silent `ToEnd`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EraseExtent { ToEnd, ToStart, All }
impl EraseExtent {
    pub fn from_param(p: Option<Param>) -> Option<EraseExtent> {
        match p.map_or(0, |x| x.0) {
            0 => Some(EraseExtent::ToEnd),
            1 => Some(EraseExtent::ToStart),
            2 => Some(EraseExtent::All),
            _ => None,
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
    CursorPosition { row: OneBased, col: OneBased },
    EraseInDisplay(EraseExtent),
    EraseInLine(EraseExtent),
    InsertLines(Count),
    DeleteLines(Count),
    DeleteChars(Count),
    EraseChars(Count),
    InsertBlanks(Count),
    SetScrollRegion { top: OneBased, bottom: OneBased },
    Sgr,
    /// Private (`?`) sequences are a different namespace. `CSI 4 h` and
    /// `CSI ? 4 h` are unrelated operations; conflating them is a classic bug.
    PrivateMode { set: bool, mode: PrivateMode },
    Unsupported { marker: Marker, final_byte: FinalByte },
}

impl Csi {
    pub fn parse(marker: Marker, p: Params<'_>, f: FinalByte) -> Csi {
        if marker == Marker::Private {
            return match f.raw() {
                b'h' => Csi::PrivateMode { set: true, mode: p.mode(ParamIdx::FIRST) },
                b'l' => Csi::PrivateMode { set: false, mode: p.mode(ParamIdx::FIRST) },
                _ => Csi::Unsupported { marker, final_byte: f },
            };
        }
        match f.raw() {
            b'A' => Csi::CursorUp(p.count(ParamIdx::FIRST)),
            b'B' => Csi::CursorDown(p.count(ParamIdx::FIRST)),
            b'C' => Csi::CursorForward(p.count(ParamIdx::FIRST)),
            b'D' => Csi::CursorBack(p.count(ParamIdx::FIRST)),
            b'H' | b'f' => Csi::CursorPosition { row: p.one_based(ParamIdx::FIRST), col: p.one_based(ParamIdx::SECOND) },
            b'J' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInDisplay(e),
                None => Csi::Unsupported { marker, final_byte: f },
            },
            b'K' => match p.extent(ParamIdx::FIRST) {
                Some(e) => Csi::EraseInLine(e),
                None => Csi::Unsupported { marker, final_byte: f },
            },
            b'L' => Csi::InsertLines(p.count(ParamIdx::FIRST)),
            b'M' => Csi::DeleteLines(p.count(ParamIdx::FIRST)),
            b'P' => Csi::DeleteChars(p.count(ParamIdx::FIRST)),
            b'X' => Csi::EraseChars(p.count(ParamIdx::FIRST)),
            b'@' => Csi::InsertBlanks(p.count(ParamIdx::FIRST)),
            b'r' => Csi::SetScrollRegion { top: p.one_based(ParamIdx::FIRST), bottom: p.one_based(ParamIdx::SECOND) },
            b'm' => Csi::Sgr,
            _ => Csi::Unsupported { marker, final_byte: f },
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Bit sets — closed, and not interchangeable with each other
// ═══════════════════════════════════════════════════════════════════════

/// Character rendition bits.
///
/// No public field, no `From<u16>`, no `BitOr<u16>`. `Attrs` and [`LineFlags`]
/// are both 16-bit sets and would happily OR together as raw integers; here
/// they cannot meet.
#[derive(Clone, Copy, PartialEq, Eq, Default, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct Attrs(u16);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1 << 0);
    pub const DIM: Attrs = Attrs(1 << 1);
    pub const ITALIC: Attrs = Attrs(1 << 2);
    pub const UNDERLINE: Attrs = Attrs(1 << 3);
    pub const BLINK: Attrs = Attrs(1 << 4);
    pub const REVERSE: Attrs = Attrs(1 << 5);
    pub const HIDDEN: Attrs = Attrs(1 << 6);
    pub const STRIKE: Attrs = Attrs(1 << 7);

    pub fn with(self, o: Attrs) -> Attrs { Attrs(self.0 | o.0) }
    pub fn without(self, o: Attrs) -> Attrs { Attrs(self.0 & !o.0) }
    pub fn has(self, o: Attrs) -> bool { self.0 & o.0 == o.0 }
    // No `as_bits`: `Attrs` is `repr(transparent)` over `u16`, so it can be
    // stored in a `repr(C)` cell directly. Unpacking it was the only reason a
    // raw `u16` used to escape.
}

/// SGR colour. `Indexed` and `Rgb` are different kinds — `38;5;n` and
/// `38;2;r;g;b` produce values that must not be confused.
/// A palette index or an sRGB component. Eight bits by protocol, not by
/// convenience — SGR encodes both as a single decimal parameter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Chan(u8);
impl Chan {
    /// Saturating: `CSI 38;2;999;0;0m` is malformed input, not an overflow.
    pub fn from_param(p: Option<Param>) -> Chan { Chan(p.map_or(0, |x| x.0.min(255)) as u8) }
    pub fn from_offset(v: u16, base: u16) -> Chan { Chan(v.saturating_sub(base) as u8) }
    pub fn get(self) -> u8 { self.0 }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Color { Default, Indexed(Chan), Rgb(Chan, Chan, Chan) }

/// One resolved SGR attribute.
pub enum SgrAttr { Reset, Set(Attrs), Unset(Attrs), Fg(Color), Bg(Color) }

impl SgrAttr {
    /// Consumes as many parameters as the attribute needs and reports how many.
    /// Returning the consumed count is what keeps `38;2;r;g;b` from being
    /// misread as five separate attributes.
    pub fn parse(params: Params<'_>) -> Option<(SgrAttr, ParamCount)> {
        let v = params.at(ParamIdx::FIRST).map_or(0, |x| x.0);
        Some(match v {
            0 => (SgrAttr::Reset, ParamCount(1)),
            1 => (SgrAttr::Set(Attrs::BOLD), ParamCount(1)),
            2 => (SgrAttr::Set(Attrs::DIM), ParamCount(1)),
            3 => (SgrAttr::Set(Attrs::ITALIC), ParamCount(1)),
            4 => (SgrAttr::Set(Attrs::UNDERLINE), ParamCount(1)),
            7 => (SgrAttr::Set(Attrs::REVERSE), ParamCount(1)),
            22 => (SgrAttr::Unset(Attrs::BOLD.with(Attrs::DIM)), ParamCount(1)),
            23 => (SgrAttr::Unset(Attrs::ITALIC), ParamCount(1)),
            24 => (SgrAttr::Unset(Attrs::UNDERLINE), ParamCount(1)),
            27 => (SgrAttr::Unset(Attrs::REVERSE), ParamCount(1)),
            30..=37 => (SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 30))), ParamCount(1)),
            40..=47 => (SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 40))), ParamCount(1)),
            39 => (SgrAttr::Fg(Color::Default), ParamCount(1)),
            49 => (SgrAttr::Bg(Color::Default), ParamCount(1)),
            38 | 48 => {
                let mk = |c| if v == 38 { SgrAttr::Fg(c) } else { SgrAttr::Bg(c) };
                match params.at(ParamIdx::SECOND).map(|x| x.0) {
                    Some(5) => (mk(Color::Indexed(Chan::from_param(params.at(ParamIdx::THIRD)))), ParamCount(3)),
                    Some(2) => (mk(Color::Rgb(
                        Chan::from_param(params.at(ParamIdx::THIRD)),
                        Chan::from_param(params.at(ParamIdx::FOURTH)),
                        Chan::from_param(params.at(ParamIdx::FIFTH)))), ParamCount(5)),
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

/// DEC private modes, named. `?1049` and `?25` are not interchangeable
/// integers — one swaps the screen buffer, the other hides the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivateMode {
    CursorVisible,      // ?25
    BracketedPaste,     // ?2004
    AltScreen,          // ?1049
    SynchronizedOutput, // ?2026
    MouseSgr,           // ?1006
    Other(Param),
}
impl PrivateMode {
    pub fn from_wire(p: Option<Param>) -> PrivateMode {
        match p.map_or(0, |x| x.0) {
            25 => PrivateMode::CursorVisible,
            1006 => PrivateMode::MouseSgr,
            1049 => PrivateMode::AltScreen,
            2004 => PrivateMode::BracketedPaste,
            2026 => PrivateMode::SynchronizedOutput,
            n => PrivateMode::Other(Param(n)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// API surface an implementation needs
// ═══════════════════════════════════════════════════════════════════════

impl StreamByte {
    pub const LF: StreamByte = StreamByte(b'\n');
    pub const CR: StreamByte = StreamByte(b'\r');
    pub const ESC: StreamByte = StreamByte(0x1b);
    pub const BS: StreamByte = StreamByte(0x08);
    pub const TAB: StreamByte = StreamByte(b'\t');
    pub const BEL: StreamByte = StreamByte(0x07);
    pub const SEMICOLON: StreamByte = StreamByte(b';');
    pub const QUESTION: StreamByte = StreamByte(b'?');
    pub const CSI_INTRO: StreamByte = StreamByte(b'[');
    pub const OSC_INTRO: StreamByte = StreamByte(b']');
    pub fn is_csi_intermediate(self) -> bool { (0x20..0x30).contains(&self.0) }
    /// Lead byte of a UTF-8 sequence: how many continuation bytes follow, and
    /// the payload bits. `None` for ASCII and for stray continuation bytes.
    pub fn utf8_lead(self) -> Option<(u8, u32)> {
        match self.0 {
            0xc2..=0xdf => Some((1, (self.0 & 0x1f) as u32)),
            0xe0..=0xef => Some((2, (self.0 & 0x0f) as u32)),
            0xf0..=0xf4 => Some((3, (self.0 & 0x07) as u32)),
            _ => None,
        }
    }
    pub fn utf8_continuation(self) -> Option<u32> {
        (self.0 & 0xc0 == 0x80).then(|| (self.0 & 0x3f) as u32)
    }
}

impl Width {
    /// East Asian Width, from Unicode data. No font input, ever — see
    /// [`DisplayWidth`] for why that is a wire contract rather than a choice.
    pub fn of(c: Codepoint) -> Width {
        let cp = c.value();
        if cp == 0 { return Width::ZERO; }
        if (0x300..=0x36f).contains(&cp) || (0x1ab0..=0x1aff).contains(&cp)
            || (0x20d0..=0x20ff).contains(&cp) || (0xfe00..=0xfe0f).contains(&cp)
            || (0x200b..=0x200f).contains(&cp) { return Width::ZERO; }
        if cp >= 0x1100 && (cp <= 0x115f || cp == 0x2329 || cp == 0x232a
            || (0x2e80..=0xa4cf).contains(&cp) || (0xac00..=0xd7a3).contains(&cp)
            || (0xf900..=0xfaff).contains(&cp) || (0xfe30..=0xfe6f).contains(&cp)
            || (0xff00..=0xff60).contains(&cp) || (0xffe0..=0xffe6).contains(&cp)
            || (0x1f300..=0x1f64f).contains(&cp)   // Misc Symbols, Emoticons
            || (0x1f680..=0x1f6ff).contains(&cp)   // Transport and Map  <- was missing
            || (0x1f7e0..=0x1f7eb).contains(&cp)   // geometric shapes ext
            || (0x1f90c..=0x1f9ff).contains(&cp)   // Supplemental Symbols
            || (0x1fa70..=0x1faff).contains(&cp)   // Symbols Extended-A
            || (0x20000..=0x3fffd).contains(&cp)) { return Width::WIDE; }
        Width::NARROW
    }
}

impl Cols {
    pub fn from_stride(n: usize) -> Cols { Cols(n.max(1)) }
    pub fn clamp(self, max: usize) -> Cols { Cols(self.0.min(max).max(1)) } }
impl Rows { pub fn clamp(self, max: usize) -> Rows { Rows(self.0.min(max).max(1)) } }

impl Col {
    pub fn forward(self, c: Count, cols: Cols) -> Col {
        Col((self.0 + c.0 as usize).min(cols.0.saturating_sub(1)))
    }
    pub fn back(self, c: Count) -> Col { Col(self.0.saturating_sub(c.0 as usize)) }
    pub fn next_tab(self, cols: Cols) -> Col {
        Col(((self.0 + 8) & !7).min(cols.0.saturating_sub(1)))
    }
}
impl Row {
    pub fn from_index(n: usize) -> Row { Row(n) }
    pub fn up(self, c: Count) -> Row { Row(self.0.saturating_sub(c.0 as usize)) }
    pub fn down(self, c: Count, rows: Rows) -> Row {
        Row((self.0 + c.0 as usize).min(rows.0.saturating_sub(1)))
    }
}

impl Count {
    pub const ONE: Count = Count(1);
    pub fn get_raw(self) -> u16 { self.0 }
}

impl StyleId {
    /// Direct-mapped bucket for a packed style word.
    ///
    /// Knuth multiplicative hashing: take the **high** bits of the product, not
    /// the low ones. `& (buckets - 1)` on the low bits is what the mixing
    /// constant fails to disturb — SGR colours are multiples of `0x101010`, so
    /// their products shared low bits and collided. Measured before: 8 distinct
    /// styles produced 112,144 interns at a 51.6% hit rate.
    pub fn bucket(raw: StyleWord, buckets: usize) -> usize {
        debug_assert!(buckets.is_power_of_two());
        let shift = 32 - buckets.trailing_zeros();
        (raw.bits().wrapping_mul(2_654_435_761) >> shift) as usize
    }
    pub fn from_index(i: usize) -> StyleId { StyleId(i as u16) }
    pub fn index(self) -> usize { self.0 as usize }
}

impl Param {
    /// Sentinel recorded in the last parameter slot when `?` was seen.
    pub const PRIVATE_FLAG: Param = Param(u16::MAX);
}

impl ParamIdx {
    pub fn raw_index(self) -> usize { self.0 as usize }
    /// At least one parameter always exists, even when omitted: `CSI m` is
    /// `CSI 0 m`. This states that once instead of `.max(1)` at each call site.
    pub fn count_at_least_one(self) -> usize { (self.0 as usize).max(1) }
    pub fn is_first(self) -> bool { self.0 == 0 }
}
impl ParamCount {
    pub fn get_raw(self) -> u8 { self.0 }
    /// Every SGR attribute consumes at least one parameter; a zero here would
    /// loop forever. Stated once rather than as `.max(1)` at the call site.
    pub fn at_least_one(self) -> usize { (self.0 as usize).max(1) }
}

impl SgrAttr {
    /// Parse the attribute starting at `i`.
    pub fn parse_at<'p>(params: &'p [Option<Param>], i: ParamIdx) -> Option<(SgrAttr, ParamCount)> {
        SgrAttr::parse(Params::new(&params[i.0 as usize..]))
    }
    /// Apply to a packed 24-bit colour + attribute word.
    pub fn fold(self, cur: StyleWord) -> StyleWord {
        StyleWord::from_bits(match self {
            SgrAttr::Reset => StyleWord::DEFAULT.bits(),
            SgrAttr::Fg(Color::Rgb(r, g, b)) =>
                ((r.get() as u32) << 16) | ((g.get() as u32) << 8) | b.get() as u32,
            SgrAttr::Fg(Color::Indexed(n)) => 0x0010_1010u32.wrapping_mul(n.get() as u32 + 1),
            _ => cur.bits(),
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════
// What an implementation needs so it never writes `y * STRIDE + x`
// ═══════════════════════════════════════════════════════════════════════

/// Read-only counterpart of [`View`].
pub struct ViewRef<'a> { cells: &'a [PackedCell], stride: Cols, cols: Cols, rows: Rows }
impl<'a> ViewRef<'a> {
    pub fn row_slice(&self, r: Row) -> &'a [PackedCell] {
        let a = r.index() * self.stride.index();
        &self.cells[a..a + self.cols.index()]
    }
    pub fn get(&self, r: Row, c: Col) -> PackedCell {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            self.cells[r.index() * self.stride.index() + c.index()]
        } else { PackedCell::EMPTY }
    }
}

/// A viewport laid out over a caller-owned cell array, addressed by axis.
///
/// The 26 occurrences of `y * MAX_COLS + x` this replaces were 26 chances to
/// transpose the axes or forget the clamp. Here the arithmetic happens once.
pub struct View<'a> {
    cells: &'a mut [PackedCell],
    stride: Cols,
    cols: Cols,
    rows: Rows,
}

impl<'a> View<'a> {
    /// Read-only view. Same addressing, no mutation.
    pub fn new_ref(cells: &'a [PackedCell], stride: Cols, cols: Cols, rows: Rows) -> ViewRef<'a> {
        ViewRef { cells, stride, cols, rows }
    }
    pub fn new(cells: &'a mut [PackedCell], stride: Cols, cols: Cols, rows: Rows) -> View<'a> {
        View { cells, stride, cols, rows }
    }
    pub fn cols(&self) -> Cols { self.cols }
    pub fn rows(&self) -> Rows { self.rows }
    pub fn set_geometry(&mut self, cols: Cols, rows: Rows) { self.cols = cols; self.rows = rows; }

    #[inline(always)]
    fn at(&self, r: Row, c: Col) -> usize { r.index() * self.stride.index() + c.index() }

    #[inline(always)]
    pub fn put(&mut self, r: Row, c: Col, cell: PackedCell) {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            let i = self.at(r, c);
            self.cells[i] = cell;
        }
    }
    #[inline(always)]
    pub fn get(&self, r: Row, c: Col) -> PackedCell {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            self.cells[self.at(r, c)]
        } else { PackedCell::EMPTY }
    }
    /// Write a run of printable ASCII into one row, starting at `at`.
    ///
    /// Bulk rather than per-cell: the bounds check and the row base are
    /// computed once. Doing it per cell cost ~25% of ingest throughput, which
    /// was a refactoring mistake rather than anything to do with the types.
    pub fn put_ascii_run(&mut self, r: Row, at: Col, src: &[u8], style: StyleId) {
        if r.index() >= self.rows.index() { return; }
        let base = self.at(r, Col::ZERO);
        let start = at.index();
        let n = src.len().min(self.cols.index().saturating_sub(start));
        for z in 0..n {
            self.cells[base + start + z] =
                PackedCell::from_ascii(StreamByte::from_stream(src[z]), style);
        }
    }

    /// Fill a half-open column span of one row. Clamped to the visible width.
    pub fn fill(&mut self, r: Row, from: Col, to: Col, cell: PackedCell) {
        let (a, b) = (from.index(), to.index().min(self.cols.index()));
        if r.index() >= self.rows.index() || a >= b { return; }
        let base = self.at(r, Col::ZERO);
        for x in a..b { self.cells[base + x] = cell; }
    }
    pub fn fill_row(&mut self, r: Row, cell: PackedCell) {
        self.fill(r, Col::ZERO, Col::at_index(self.cols.index()), cell);
    }
    /// Shift a row's cells left by `n`, blanking the tail. `CSI P`.
    pub fn delete_chars(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        let base = self.at(r, Col::ZERO);
        for x in cx..cols - n { self.cells[base + x] = self.cells[base + x + n]; }
        for x in cols - n..cols { self.cells[base + x] = blank; }
    }
    /// Shift right, inserting blanks. `CSI @`.
    pub fn insert_blanks(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        let base = self.at(r, Col::ZERO);
        let mut x = cols;
        while x > cx + n { x -= 1; self.cells[base + x] = self.cells[base + x - n]; }
        for x in cx..cx + n { self.cells[base + x] = blank; }
    }
    /// Scroll rows down from `at`, blanking the opened region. `CSI L`.
    pub fn insert_lines(&mut self, at: Row, n: Count, blank: PackedCell) {
        let rows = self.rows.index();
        let (cy, n) = (at.index(), (n.get_raw() as usize).min(rows - at.index()));
        let s = self.stride.index();
        let cols = self.cols.index();
        let mut y = rows;
        while y > cy + n {
            y -= 1;
            for x in 0..cols { self.cells[y * s + x] = self.cells[(y - n) * s + x]; }
        }
        for y in cy..cy + n { for x in 0..cols { self.cells[y * s + x] = blank; } }
    }
    pub fn row_slice(&self, r: Row) -> &[PackedCell] {
        let a = self.at(r, Col::ZERO);
        &self.cells[a..a + self.cols.index()]
    }
}

impl PackedCell {
    /// Unwritten. Note this is *not* a space: the renderer decides what an empty
    /// cell looks like, and selection can tell the difference.
    pub const EMPTY: PackedCell =
        PackedCell { content: Content::EMPTY, style: StyleId::DEFAULT, _pad: 0 };
    pub fn empty_with(style: StyleId) -> PackedCell {
        PackedCell { content: Content::EMPTY, style, _pad: 0 }
    }
    /// A printable ASCII byte. The proof that it is printable is also the proof
    /// that its codepoint equals its byte value.
    pub fn from_ascii(b: StreamByte, style: StyleId) -> PackedCell {
        PackedCell { content: Content::codepoint(Codepoint::from_ascii(b)), style, _pad: 0 }
    }
    pub fn glyph(c: Codepoint, style: StyleId) -> PackedCell {
        PackedCell { content: Content::codepoint(c), style, _pad: 0 }
    }
    pub fn wide_tail(style: StyleId) -> PackedCell {
        PackedCell { content: Content::WIDE_TAIL, style, _pad: 0 }
    }
}

impl Col {
    pub fn at_index(i: usize) -> Col { Col(i) }
    /// One past this column, for a half-open span.
    pub fn after(self) -> Col { Col(self.0 + 1) }
}

/// Position within the scrollback ring. Wraps by construction.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RingPos(usize);

impl RingPos {
    pub const ZERO: RingPos = RingPos(0);
    pub fn advance(self, modulus: usize) -> RingPos { RingPos((self.0 + 1) % modulus) }
    pub fn plus(self, r: Row, modulus: usize) -> RingPos { RingPos((self.0 + r.index()) % modulus) }
    pub fn back(self, n: usize, modulus: usize) -> RingPos {
        RingPos((self.0 + modulus - n % modulus) % modulus)
    }
    pub fn index(self) -> usize { self.0 }
}

/// Write offset into the arena, in bytes, wrapping at capacity.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ArenaTail(usize);

impl ArenaTail {
    pub const ZERO: ArenaTail = ArenaTail(0);
    /// Wraps when a full slot would not fit, so a read never straddles the end.
    pub fn next_slot(self, capacity: usize) -> ArenaTail {
        if self.0 + SLOT_BYTES > capacity { ArenaTail(0) } else { self }
    }
    pub fn advance(self, n: ByteLen) -> ArenaTail { ArenaTail(self.0 + n.index()) }
    pub fn index(self) -> usize { self.0 }
    pub fn pos(self) -> BytePos { BytePos::from_index(self.0) }
}

// ═══════════════════════════════════════════════════════════════════════
// Typed byte surfaces
// ═══════════════════════════════════════════════════════════════════════

/// A writable slot handed to a [`Source`]. Not a `&mut [u8]`: the length is
/// [`SLOT_BYTES`], not whatever the caller feels like, and `commit` is the only
/// way to say how much arrived.
pub struct Slot<'a> {
    buf: &'a mut [u8],
    base: BytePos,
}

impl<'a> Slot<'a> {
    /// Only the store constructs one.
    pub fn new(buf: &'a mut [u8], base: BytePos) -> Slot<'a> {
        debug_assert_eq!(buf.len(), SLOT_BYTES);
        Slot { buf, base }
    }
    pub fn base(&self) -> BytePos { self.base }
    pub const fn capacity() -> ByteLen { ByteLen::new(SLOT_BYTES as u16) }
    /// Copy from a typed byte stream.
    pub fn write(&mut self, src: &[StreamByte]) -> Filled {
        let n = src.len().min(SLOT_BYTES);
        for i in 0..n { self.buf[i] = src[i].raw(); }
        Filled::from_read(n)
    }
    /// The raw handle a syscall needs. Named, and the only one.
    pub fn as_syscall_buf(&mut self) -> (*mut u8, usize) {
        (self.buf.as_mut_ptr(), SLOT_BYTES)
    }
}

impl StreamByte {
    pub(crate) fn raw(self) -> u8 { self.0 }
    /// For matching against ASCII literals in escape dispatch.
    pub fn raw_byte(self) -> u8 { self.0 }
    /// Reinterpret a byte slice as a stream. No copy and no `unsafe`:
    /// `repr(transparent)` plus zerocopy's `FromBytes` make this a checked
    /// no-op. This is the crate's one entry point for wire data.
    pub fn slice_from(b: &[u8]) -> &[StreamByte] {
        <[StreamByte]>::ref_from_bytes(b).expect("transparent over u8, always valid")
    }
    pub fn slice_as_bytes(s: &[StreamByte]) -> &[u8] { s.as_bytes() }
}

/// How many continuation bytes a UTF-8 sequence still needs. 1, 2 or 3 by
/// construction — never an arbitrary `u8` counter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pending(u8);

impl Pending {
    pub fn new(n: u8) -> Pending { Pending(n.min(3)) }
    pub fn is_last(self) -> bool { self.0 == 1 }
    pub fn dec(self) -> Pending { Pending(self.0.saturating_sub(1)) }
}

/// A Unicode scalar value: `0..=0x10FFFF`, surrogates excluded.
///
/// Not a `u32`. `Width::of` and `Content::codepoint` both took one and could be
/// handed a lane mask, a byte count, or a packed colour with equal enthusiasm.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug,
         FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Codepoint(u32);

impl Codepoint {
    pub const REPLACEMENT: Codepoint = Codepoint(0xFFFD);
    pub const SPACE: Codepoint = Codepoint(0x20);

    /// The only fallible constructor. Rejects surrogates and out-of-range —
    /// both of which a UTF-8 decoder can produce from malformed input.
    pub fn new(v: u32) -> Option<Codepoint> {
        (v <= 0x10_FFFF && !(0xD800..0xE000).contains(&v)).then_some(Codepoint(v))
    }
    /// From a decoder that has already validated. Malformed input becomes
    /// U+FFFD rather than an invalid `Codepoint`, so the type's invariant holds
    /// without the caller having to branch.
    pub fn decoded(v: u32) -> Codepoint { Codepoint::new(v).unwrap_or(Codepoint::REPLACEMENT) }
    pub fn from_ascii(b: StreamByte) -> Codepoint { Codepoint(b.raw() as u32) }
    pub fn value(self) -> u32 { self.0 }
}

/// A packed rendition word: 24-bit colour plus attribute bits, as SGR
/// accumulates it. Interned into a [`StyleId`]; never stored per cell.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct StyleWord(u32);

impl StyleWord {
    pub const DEFAULT: StyleWord = StyleWord(0x00d0_d0d0);
    pub fn from_bits(v: u32) -> StyleWord { StyleWord(v) }
    pub fn bits(self) -> u32 { self.0 }
}

/// Index of a line slot in the scrollback ring. Distinct from [`LineIdx`],
/// which is a stable logical line number: this one wraps.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LineSlot(usize);

impl LineSlot {
    pub fn of(pos: RingPos) -> LineSlot { LineSlot(pos.index()) }
    pub fn index(self) -> usize { self.0 }
    pub fn run(self, r: RunCount, maxrun: usize) -> usize { self.0 * maxrun + r.index() }
}

/// A monotonic event counter. Not a `u64`: a statistic cannot be added to a
/// length, a byte position or a column, and nothing can be assigned into it
/// except by counting.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Stat(u64);

impl Stat {
    pub const ZERO: Stat = Stat(0);
    pub fn inc(&mut self) { self.0 += 1; }
    pub fn count(self) -> u64 { self.0 }
    pub fn is_zero(self) -> bool { self.0 == 0 }
    /// Ratio against another counter, for hit rates. Saturating at zero rather
    /// than dividing by it.
    pub fn ratio_to(self, other: Stat) -> f64 {
        let d = self.0 + other.0;
        if d == 0 { 0.0 } else { self.0 as f64 / d as f64 }
    }
}

/// Codepoint bits accumulated mid-sequence, *before* they are known to be a
/// valid scalar value. Deliberately not a [`Codepoint`] — a partial sequence
/// has not earned that type yet, and calling it one would be a lie the width
/// tables would then believe.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Utf8Acc(u32);

impl Utf8Acc {
    pub fn lead(bits: u32) -> Utf8Acc { Utf8Acc(bits) }
    pub fn push(self, cont: u32) -> Utf8Acc { Utf8Acc((self.0 << 6) | cont) }
    /// The only exit. Malformed input becomes U+FFFD rather than an invalid
    /// codepoint, so the invariant holds without the caller branching.
    pub fn finish(self) -> Codepoint { Codepoint::decoded(self.0) }
}

impl Count {
    /// At least one, as a plain count of items. Named because `CSI A` with no
    /// parameter means 1, and that rule was being restated as `.max(1)` at each
    /// call site.
    pub fn at_least_one(self) -> usize { (self.0 as usize).max(1) }
}

impl Run {
    /// Set the encoding from the typed enum rather than casting at the call
    /// site. The field is `u8` because a fieldless enum cannot be `FromBytes`
    /// — a byte off disk is not necessarily a valid variant — so recovery goes
    /// through the checked [`Run::encoding`].
    pub fn with_encoding(mut self, e: Encoding) -> Run { self.encoding = e as u8; self }
}

impl ByteLen {
    /// Offset of a SIMD match within a scanned window.
    pub fn from_lane(n: usize) -> ByteLen { ByteLen::from_index(n) }
}
