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
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};

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
    fn clone(&self) -> Self {
        *self
    }
}
impl<'id> Copy for Brand<'id> {}
impl<'id> PartialEq for Brand<'id> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<'id> Eq for Brand<'id> {}
impl<'id> PartialOrd for Brand<'id> {
    fn partial_cmp(&self, o: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(o))
    }
}
impl<'id> Ord for Brand<'id> {
    fn cmp(&self, _: &Self) -> core::cmp::Ordering {
        core::cmp::Ordering::Equal
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Positions — opaque, non-interchangeable
// ═══════════════════════════════════════════════════════════════════════

/// Absolute offset in the total byte stream. Monotonic, never reused.
///
/// No public field and no arithmetic: you cannot accidentally treat it as a
/// ring offset. Converting to one is [`Store::resolve`]'s job, and it is the
/// only place that knows about `ARENA`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct BytePos(u64);

impl BytePos {
    pub const ZERO: BytePos = BytePos(0);
    /// Absolute stream position from an arena offset. The arena is the only
    /// thing that knows the mapping, so this is where it is stated.
    pub fn from_index(n: usize) -> BytePos {
        BytePos(n as u64)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn advance(self, n: ByteLen) -> BytePos {
        BytePos(self.0 + n.get() as u64)
    }
    /// Distance, saturating. Deliberately not `Sub`, so a negative gap cannot
    /// wrap into a huge positive one.
    pub fn gap(self, earlier: BytePos) -> ByteGap {
        ByteGap(self.0.saturating_sub(earlier.0))
    }
}

/// Index of a logical line. **Stable across resize and font change** — this is
/// the only line identifier safe to persist.
/// Distance between two [`BytePos`]. Cannot be added to a position; only
/// compared against a capacity.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct ByteGap(u64);
impl ByteGap {
    pub(crate) fn exceeds(self, capacity: usize) -> bool {
        self.0 > capacity as u64
    }
}

#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct LineIdx(u64);

impl LineIdx {
    pub const FIRST: LineIdx = LineIdx(0);
    pub fn next(self) -> LineIdx {
        LineIdx(self.0 + 1)
    }
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
    pub fn cols(&self) -> Cols {
        self.cols
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }

    /// The only constructor for a branded [`RowIdx`].
    pub fn row(&self, v: RowCount) -> RowIdx<'layout> {
        RowIdx { v, _b: self._b }
    }
    pub fn row_value(&self, r: RowIdx<'layout>) -> RowCount {
        r.v
    }

    /// Display rows a line occupies at this width.
    ///
    /// Derived, never stored authoritatively — a pure function of
    /// `(DisplayWidth, cols)`, so the row index can always be recomputed and
    /// asserted against rather than drifting from the text.
    pub fn rows_for(&self, w: DisplayWidth) -> RowCount {
        let c = self.cols.raw() as u32;
        RowCount(w.cells().max(1).div_ceil(c))
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
    f(Layout {
        cols,
        rows,
        _b: Brand(PhantomData),
    })
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
pub enum AmbiguousWidth {
    Narrow,
    Wide,
}

/// Whether `U+FE0F` presentation makes emoji 2 columns. Terminals disagree;
/// this is config, not font.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EmojiWidth {
    Narrow,
    Wide,
}

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
    pub(crate) fn cells(self) -> u32 {
        self.0
    }
    /// Not public: `measure` is the only route from content to a width.
    fn from_cells(n: u32) -> DisplayWidth {
        DisplayWidth(n)
    }
    /// Byte length *is* column count, but only for a byte-per-column encoding.
    /// Callable only where a `Bytes<Printable>` or `Bytes<DecGraphics>` proof
    /// exists, which is what makes it sound.
    fn from_len(n: ByteLen) -> DisplayWidth {
        DisplayWidth(n.index() as u32)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Style
// ═══════════════════════════════════════════════════════════════════════

/// Index into the style table. Two bytes rather than ten inline: cell size
/// drove throughput from 36% of the kernel ceiling to 80%.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
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
    pub fn bytes(&self) -> Bytes<'store, Raw> {
        Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        }
    }
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
    pub fn head(&self) -> BytePos {
        self.head
    }

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
    #[must_use = "a Lapped error means ingest overran the drain thread; dropping it silently discards history"]
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
        Line {
            first_run,
            run_count: RunCount::ZERO,
            width: DisplayWidth::ZERO,
            _s: PhantomData,
        }
    }
    /// Runs live in a shared pool, so there is no fixed cap. A previous design
    /// used `[Run; 8]` and silently promoted any line exceeding it.
    pub fn push_run(&mut self) {
        self.run_count = self.run_count.inc();
    }

    /// The only transition out of `Building`. Consumes `self`, so the mutable
    /// handle is gone.
    pub fn complete(self, width: DisplayWidth) -> Line<Complete> {
        Line {
            first_run: self.first_run,
            run_count: self.run_count,
            width,
            _s: PhantomData,
        }
    }
}

impl Line<Complete> {
    /// Only exists once the line is final, so a width can never be read
    /// mid-append.
    pub fn width(&self) -> DisplayWidth {
        self.width
    }
    pub fn runs(&self) -> RunRange {
        RunRange {
            start: self.first_run,
            count: self.run_count,
        }
    }
    pub fn drained(self) -> Line<Drained> {
        Line {
            first_run: self.first_run,
            run_count: self.run_count,
            width: self.width,
            _s: PhantomData,
        }
    }
}

impl Line<Drained> {
    pub fn width(&self) -> DisplayWidth {
        self.width
    }
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
    pub fn from_shaper(n: u16) -> GlyphId {
        GlyphId(n)
    }
}

impl<'font> FontEpoch<'font> {
    pub fn slot(&self, g: GlyphId) -> AtlasSlot<'font> {
        AtlasSlot {
            v: g.0,
            _b: self._b,
        }
    }
    /// Named so the widening is visible where the atlas is indexed.
    pub fn index(&self, s: AtlasSlot<'font>) -> usize {
        s.v as usize
    }
}

/// Enter a font scope. A font change means calling this again; anything
/// holding the old epoch's slots will not type-check.
pub fn with_font<R>(f: impl for<'id> FnOnce(FontEpoch<'id>) -> R) -> R {
    f(FontEpoch {
        _b: Brand(PhantomData),
    })
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
    pub fn codepoint(c: Codepoint) -> Content {
        Content(c.value())
    }
    pub fn as_codepoint(self) -> Option<Codepoint> {
        Codepoint::new(self.0)
    }
    pub fn is_empty(self) -> bool {
        self == Content::EMPTY
    }
    pub fn is_wide_tail(self) -> bool {
        self == Content::WIDE_TAIL
    }
}

impl Default for Content {
    fn default() -> Self {
        Content::EMPTY
    }
}

/// One materialised cell: content plus a style index.
///
/// Eight bytes rather than four. Measured cost of that width: 8-byte cells hit
/// 48% of the kernel ceiling against 50% for 4-byte — two points, for holding a
/// full codepoint instead of a truncated one.
#[repr(C)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct PackedCell {
    pub content: Content,
    pub style: StyleId,
    pub flags: CellFlags,
}

/// Per-cell state that is **not** a rendition.
///
/// Lives in what used to be `PackedCell::_pad`, so the cell is still eight
/// bytes and the measurement behind that width still holds.
///
/// The distinction is the point. A rendition is interned: many cells share one
/// [`StyleId`], and the table behind it is a direct-mapped cache that evicts.
/// That is tolerable for colour -- an evicted bucket renders wrong until it is
/// rewritten -- and not tolerable for DECSCA protection, where eviction would
/// silently expose text an application asked to keep. Anything whose loss is a
/// correctness bug rather than a cosmetic one belongs here, per cell, where
/// nothing can evict it.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct CellFlags(u16);

impl CellFlags {
    pub const NONE: CellFlags = CellFlags(0);
    /// DECSCA: DECSED and DECSEL may not erase this cell.
    ///
    /// `[[source: vt510-decsca]]`: "1 DECSED and DECSEL cannot erase
    /// characters."
    pub const PROTECTED: CellFlags = CellFlags(1);
    /// The byte in this cell was written while the DEC Special Character and
    /// Line Drawing Set was invoked into GL.
    ///
    /// The cell keeps the byte the application sent, plus this flag, rather
    /// than a translated codepoint. That is deliberate and it is a *sourcing*
    /// decision, not a design preference: `[[source: ctlseqs]]` names the set
    /// (`C = 0 -> DEC Special Character and Line Drawing Set, VT100`) but does
    /// not give the byte-to-glyph mapping, and `MANIFEST.toml` records
    /// `dec-special-graphics` as `gated` -- vt100.net publishes the table as a
    /// figure, so no claim may rest on it. Translating here would mean writing
    /// the table from memory, which this project has been wrong about before.
    ///
    /// Carrying the flag loses nothing: a renderer that has the table has
    /// everything it needs, and one that does not can say so. Translating to a
    /// guessed codepoint would be unrecoverable.
    pub const GRAPHICS: CellFlags = CellFlags(2);
    pub fn is_graphics(self) -> bool {
        self.0 & CellFlags::GRAPHICS.0 != 0
    }
    pub fn with(self, o: CellFlags) -> CellFlags {
        CellFlags(self.0 | o.0)
    }
    pub fn protection(on: bool) -> CellFlags {
        CellFlags(u16::from(on))
    }
    pub fn is_protected(self) -> bool {
        self.0 & CellFlags::PROTECTED.0 != 0
    }
    /// The byte, for the one place a flag set has to survive a round trip
    /// through [`Run`]'s fixed layout.
    pub fn bits(self) -> u8 {
        self.0 as u8
    }
    pub fn from_bits(b: u8) -> CellFlags {
        CellFlags(b as u16)
    }
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
    pub fn stride(&self) -> Cols {
        self.stride
    }
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

pub mod unchecked {
    //! Left as runtime checks, deliberately.
    //!
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
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Width(u8);

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
pub struct ByteLen(u16);

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
        Grid {
            cells,
            stride,
            rows,
        }
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }
    pub fn stride(&self) -> Cols {
        self.stride
    }
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
    fn index(&self, (r, c): (Row, Col)) -> &T {
        &self.cells[r.0 * self.stride.0 + c.0]
    }
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
    pub fn new(slots: &'buf mut [T]) -> LineRing<'buf, T> {
        LineRing { slots }
    }
}
impl<'buf, T> core::ops::Index<LineIdx> for LineRing<'buf, T> {
    type Output = T;
    fn index(&self, l: LineIdx) -> &T {
        &self.slots[(l.0 as usize) % self.slots.len()]
    }
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
    pub fn new(bytes: &'buf mut [u8]) -> SlotRing<'buf> {
        SlotRing { bytes }
    }
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
    pub fn next(self) -> ParamIdx {
        ParamIdx(self.0 + 1)
    }
    pub fn plus(self, n: u8) -> ParamIdx {
        ParamIdx(self.0 + n)
    }
}

/// How many parameters an SGR attribute consumed.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ParamCount(u8);
impl ParamCount {
    pub fn get(self) -> ParamIdx {
        ParamIdx(self.0)
    }
}

/// Geometry as `TIOCGWINSZ` reports it. Destructured once, here, so `ws_col`
/// and `ws_row` cannot be transposed at any other call site.
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
}

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
pub struct Row(usize);

/// Grid width. Distinct from [`Rows`] so the two cannot be swapped at a call
/// site — `Layout::new(rows, cols)` transposed is a bug that used to compile.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Cols(usize);
impl Cols {
    fn from_ws(n: u16) -> Cols {
        Cols((n as usize).max(1))
    }
    /// Columns remaining at `cx`. The only source of a [`Room`].
    pub fn room_at(self, cx: Col) -> Room {
        Room(self.0.saturating_sub(cx.0))
    }
    pub fn index(self) -> usize {
        self.0
    }
    fn raw(self) -> usize {
        self.0
    }
}

/// Grid height.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Rows(usize);
impl Rows {
    fn from_ws(n: u16) -> Rows {
        Rows((n as usize).max(1))
    }
    pub fn index(self) -> usize {
        self.0
    }
    fn raw(self) -> usize {
        self.0
    }
}

/// How many display rows something occupies. Not a [`Rows`] — that is a grid
/// dimension, this is a count that can exceed it.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct RowCount(u32);
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

/// Index into the shared run pool.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct RunIdx(u32);
impl RunIdx {
    pub const FIRST: RunIdx = RunIdx(0);
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn advance(self, n: RunCount) -> RunIdx {
        RunIdx(self.0 + n.0 as u32)
    }
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
    /// [`CellFlags`] as a byte, for the same reason `encoding` is one: the
    /// struct crosses the mmap boundary and zerocopy will not take a type whose
    /// bit patterns are not all valid. Recover it with [`Run::flags`].
    ///
    /// A run carries the protection so that promoting it to cells does not lose
    /// it -- the run *is* the storage in `OpMode::Runs`, and a promotion that
    /// dropped the flag would make protection depend on when the line happened
    /// to materialise.
    pub flags: u8,
    pub _pad: [u8; 2],
}

impl Run {
    /// Checked: a byte off disk is not necessarily a valid [`Encoding`].
    pub fn encoding(&self) -> Option<Encoding> {
        Encoding::try_read_from_bytes(&[self.encoding]).ok()
    }
    /// Unchecked, unlike [`Run::encoding`]: every bit pattern is a valid flag
    /// set, so there is nothing to reject.
    pub fn flags(&self) -> CellFlags {
        CellFlags::from_bits(self.flags)
    }
}

/// Half-open range into the shared run pool.
#[derive(Clone, Copy)]
pub struct RunRange {
    pub start: RunIdx,
    pub count: RunCount,
}

/// A number of runs.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    FromBytes,
    IntoBytes,
    Immutable,
    KnownLayout,
)]
#[repr(transparent)]
pub struct RunCount(u16);
impl RunCount {
    pub const ZERO: RunCount = RunCount(0);
    pub fn from_index(n: usize) -> RunCount {
        RunCount(n as u16)
    }
    pub fn inc(self) -> RunCount {
        RunCount(self.0 + 1)
    }
    pub fn index(self) -> usize {
        self.0 as usize
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
    pub fn from_stream(b: u8) -> StreamByte {
        StreamByte(b)
    }
    pub fn is_printable(self) -> bool {
        (0x20..0x7f).contains(&self.0)
    }
    pub fn is_c0(self) -> bool {
        self.0 < 0x20
    }
    pub fn is_continuation(self) -> bool {
        self.0 & 0xc0 == 0x80
    }
    pub fn as_ascii_digit(self) -> Option<Digit> {
        self.0.is_ascii_digit().then(|| Digit(self.0 - b'0'))
    }
    /// For the atlas and the width tables, which index by codepoint.
    pub fn index(self) -> usize {
        self.0 as usize
    }
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
    fn raw(self) -> u8 {
        self.0
    }
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
    sub: u16,
}

impl<'a> Params<'a> {
    pub fn new(p: &'a [Option<Param>]) -> Params<'a> {
        Params { v: p, sub: 0 }
    }
    /// With the sub-parameter mask, as the CSI parser accumulated it.
    pub fn with_subs(p: &'a [Option<Param>], sub: u16) -> Params<'a> {
        Params { v: p, sub }
    }
    /// Is parameter `i` joined to the one before it by a colon?
    pub fn is_sub(&self, i: ParamIdx) -> bool {
        i.0 < 16 && self.sub & (1 << i.0) != 0
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
    pub fn skip(&self, k: u8) -> Params<'a> {
        let k = k.min(16);
        Params {
            v: &self.v[(k as usize).min(self.v.len())..],
            sub: self.sub >> k,
        }
    }
    fn at(&self, i: ParamIdx) -> Option<Param> {
        self.v.get(i.0 as usize).copied().flatten()
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
        matches!(
            self,
            Encoding::Ascii | Encoding::DecGraphics | Encoding::Raw
        )
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

impl Kind for Raw {
    const ENCODING: Encoding = Encoding::Raw;
}
impl Kind for Printable {
    const ENCODING: Encoding = Encoding::Ascii;
}
impl Kind for Utf8 {
    const ENCODING: Encoding = Encoding::Utf8;
}
impl Kind for DecGraphics {
    const ENCODING: Encoding = Encoding::DecGraphics;
}

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
        Bytes {
            raw,
            _k: core::marker::PhantomData,
        }
    }
    /// Split off the leading run of printable ASCII. This is the SIMD scan, and
    /// the returned `Printable` view is the proof it produced.
    pub fn split_printable(self) -> (Bytes<'a, Printable>, Bytes<'a, Raw>) {
        let n = self
            .raw
            .iter()
            .take_while(|&&b| (0x20..0x7f).contains(&b))
            .count();
        (
            Bytes {
                raw: &self.raw[..n],
                _k: core::marker::PhantomData,
            },
            Bytes {
                raw: &self.raw[n..],
                _k: core::marker::PhantomData,
            },
        )
    }
    /// Promote to UTF-8, or hand the bytes back unproven.
    pub fn validate_utf8(self) -> Result<Bytes<'a, Utf8>, Bytes<'a, Raw>> {
        match core::str::from_utf8(self.raw) {
            Ok(_) => Ok(Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            }),
            Err(_) => Err(self),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

impl<'a> Bytes<'a, Printable> {
    /// Free, and needs no [`WidthTable`] — the proof that these are printable
    /// ASCII is exactly the proof that one byte is one column. This is the
    /// ~94% fast path, and the type is what makes skipping the table sound.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

impl<'a> Bytes<'a, Utf8> {
    pub fn measure(&self, t: &WidthTable) -> DisplayWidth {
        let _ = t;
        DisplayWidth::from_cells(0) // model only
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::new(self.raw.len() as u16)
    }
}

impl<'a> Bytes<'a, Raw> {
    /// Reinterpret under the active character set. `ESC ( 0` makes subsequent
    /// bytes graphics; this is where that decision lands.
    pub fn as_dec_graphics(self) -> Bytes<'a, DecGraphics> {
        Bytes {
            raw: self.raw,
            _k: core::marker::PhantomData,
        }
    }
}

impl<'a> Bytes<'a, DecGraphics> {
    /// Fixed-width single-byte: one column each, no table.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

/// The dynamic form: a run whose encoding is carried rather than proven.
/// This is what gets stored, and what a `match` over stored runs sees.
pub struct Encoded<'a> {
    bytes: Bytes<'a, Raw>,
    encoding: Encoding,
}

impl<'a> Encoded<'a> {
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }
    /// Width without decoding, where the encoding permits it. `None` means the
    /// caller must decode — which is exactly the branch materialisation wants
    /// to take once per run.
    pub fn width_fast(&self) -> Option<DisplayWidth> {
        self.encoding
            .is_byte_per_column()
            .then(|| DisplayWidth::from_len(ByteLen::of(self.bytes.raw)))
    }
    /// Checked downcast back to a static proof.
    pub fn as_utf8(self) -> Option<Bytes<'a, Utf8>> {
        (self.encoding == Encoding::Utf8).then_some(Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        })
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Static proof to dynamic tag. Free — `K::ENCODING` is a constant.
    pub fn erase(self) -> Encoded<'a> {
        Encoded {
            bytes: Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            },
            encoding: K::ENCODING,
        }
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Named so every crossing back to an untyped slice is visible. Used by the
    /// renderer and by `memcpy` into the cold tier, and nowhere else.
    pub fn as_slice(&self) -> &'a [u8] {
        self.raw
    }
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
    pub fn next(self) -> SlotIdx {
        SlotIdx(self.0.wrapping_add(1))
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    /// Byte offset of this slot within the arena.
    /// Byte offset of this slot within the arena. Named as an index because
    /// that is the only thing it may be used for.
    pub fn byte_index(self) -> usize {
        self.0 as usize * SLOT_BYTES
    }
}

/// How much of a slot a source actually filled. Provably `<= SLOT_BYTES`, so
/// it can never describe a region outside the slot it came from.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Filled(u16);

impl Filled {
    pub const EMPTY: Filled = Filled(0);
    /// Clamps, so a source cannot report more than it was given.
    pub fn from_read(n: usize) -> Filled {
        Filled(n.min(SLOT_BYTES) as u16)
    }
    pub fn len(self) -> ByteLen {
        ByteLen::new(self.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
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
    pub const fn capacity() -> ByteLen {
        ByteLen::new(SLOT_BYTES as u16)
    }
    pub fn base(&self) -> BytePos {
        self.base
    }
    pub fn idx(&self) -> SlotIdx {
        self.idx
    }
    /// For a source to write into. The one raw handle, named.
    ///
    /// Not an `AsMut` impl: that would make the raw handle reachable generically
    /// by anything with a `T: AsMut<_>` bound, which is the opposite of "named".
    #[allow(clippy::should_implement_trait)]
    pub fn as_mut(&mut self) -> &mut [u8; SLOT_BYTES] {
        self.buf
    }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct OneBased(u16);

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
pub struct Count(u16);
impl Count {
    pub fn from_param(p: Option<Param>) -> Count {
        Count(p.map_or(0, |x| x.0).max(1))
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

/// An `ESC Fe` sequence, resolved -- the two-byte escapes that carry no
/// parameters.
///
/// The counterpart to [`Csi`], and it exists for the same reason. This dispatch
/// used to be a `match` on `b'7'`, `b'8'`, `b'M'`, `b'D'`, `b'E'`, `b'c'` and
/// `b'H'`, which are not the names of anything -- they are the bytes those
/// operations happen to be spelled with. Every one has a name in the standard,
/// and a reader should not have to remember that `ESC M` scrolls *down*
/// (RI, reverse index) while `ESC D` scrolls up.
///
/// `Unsupported` is a variant rather than a `_ => {}` arm, on the same
/// principle as [`Csi`]: what is not implemented stays enumerable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Esc {
    /// DECSC — Save Cursor, `ESC 7`.
    ///
    /// `[[source: vt510-decsc]]`: saves the cursor position, the SGR
    /// attributes, the character sets, the wrap flag, the state of DECOM, and
    /// the selective erase attribute.
    SaveCursor,
    /// DECRC — Restore Cursor, `ESC 8`. The inverse of [`Esc::SaveCursor`].
    RestoreCursor,
    /// RI — Reverse Index, `ESC M`. Up one line, scrolling the region *down* at
    /// the top margin.
    ReverseIndex,
    /// IND — Index, `ESC D`. Down one line, keeping the column.
    Index,
    /// NEL — Next Line, `ESC E`. Down one line *and* to column 1, which is what
    /// distinguishes it from [`Esc::Index`].
    NextLine,
    /// RIS — Reset to Initial State, `ESC c`. A full reset, not a soft one.
    Reset,
    /// DECKPAM — Application Keypad, `ESC =`. `[[source: ctlseqs]]`:
    /// "ESC = Application Keypad (DECKPAM)."
    KeypadApplication,
    /// DECKPNM — Normal Keypad, `ESC >`, VT100.
    KeypadNumeric,
    /// HTS — Horizontal Tab Set, `ESC H`. Sets a tab stop at the cursor column.
    TabSet,
    /// Recognised as an `ESC Fe` and deliberately not acted on.
    Unsupported,
}

impl Esc {
    pub fn parse(b: StreamByte) -> Esc {
        match b.raw_byte() {
            b'7' => Esc::SaveCursor,
            b'8' => Esc::RestoreCursor,
            b'M' => Esc::ReverseIndex,
            b'D' => Esc::Index,
            b'E' => Esc::NextLine,
            b'c' => Esc::Reset,
            b'=' => Esc::KeypadApplication,
            b'>' => Esc::KeypadNumeric,
            b'H' => Esc::TabSet,
            _ => Esc::Unsupported,
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
        protect: bool,
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
        set: bool,
        mode: AnsiMode,
    },
    Sgr,
    /// DECSCUSR, `CSI Ps SP q`. Cursor shape and blink.
    CursorStyle(CursorStyle),
    /// XTVERSION, `CSI > 0 q`. Answered with [`Report::Version`].
    RequestVersion,
    /// DECRQM, `CSI Ps $ p` and `CSI ? Ps $ p`. A *question*, answered with
    /// [`Report::ModeStatus`].
    RequestMode {
        marker: Marker,
        mode: Param,
    },
    /// Private (`?`) sequences are a different namespace. `CSI 4 h` and
    /// `CSI ? 4 h` are unrelated operations; conflating them is a classic bug.
    PrivateMode {
        set: bool,
        mode: PrivateMode,
    },
    Unsupported {
        marker: Marker,
        final_byte: FinalByte,
    },
}

impl Csi {
    /// `inter` is the intermediate byte, which is part of the operation's
    /// identity: `CSI ! p` is DECSTR and `CSI p` is not the same thing.
    pub fn parse(marker: Marker, inter: Option<StreamByte>, p: Params<'_>, f: FinalByte) -> Csi {
        if let Some(i) = inter {
            return match (i.raw_byte(), f.raw()) {
                (b'!', b'p') => Csi::SoftReset,
                (b'"', b'q') => Csi::SelectProtection {
                    protect: p.at(ParamIdx::FIRST).map_or(0, |x| x.0) == 1,
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
        if marker == Marker::Private {
            return match f.raw() {
                b'h' => Csi::PrivateMode {
                    set: true,
                    mode: p.mode(ParamIdx::FIRST),
                },
                b'l' => Csi::PrivateMode {
                    set: false,
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
            b'T' => Csi::ScrollDown(p.count(ParamIdx::FIRST)),
            b'h' => Csi::Mode {
                set: true,
                mode: AnsiMode::from_wire(p.at(ParamIdx::FIRST)),
            },
            b'l' => Csi::Mode {
                set: false,
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

// ═══════════════════════════════════════════════════════════════════════
// Bit sets — closed, and not interchangeable with each other
// ═══════════════════════════════════════════════════════════════════════

/// Character rendition bits.
///
/// No public field, no `From<u16>`, no `BitOr<u16>`. `Attrs` and [`LineFlags`]
/// are both 16-bit sets and would happily OR together as raw integers; here
/// they cannot meet.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
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

    /// The raw set, for hashing a rendition.
    pub fn bits(self) -> u16 {
        self.0
    }
    pub fn with(self, o: Attrs) -> Attrs {
        Attrs(self.0 | o.0)
    }
    pub fn without(self, o: Attrs) -> Attrs {
        Attrs(self.0 & !o.0)
    }
    pub fn has(self, o: Attrs) -> bool {
        self.0 & o.0 == o.0
    }
    // No `as_bits`: `Attrs` is `repr(transparent)` over `u16`, so it can be
    // stored in a `repr(C)` cell directly. Unpacking it was the only reason a
    // raw `u16` used to escape.
}

/// SGR colour. `Indexed` and `Rgb` are different kinds — `38;5;n` and
/// `38;2;r;g;b` produce values that must not be confused.
/// A palette index or an sRGB component. Eight bits by protocol, not by
/// convenience — SGR encodes both as a single decimal parameter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chan(u8);
impl Chan {
    /// Saturating: `CSI 38;2;999;0;0m` is malformed input, not an overflow.
    pub fn from_param(p: Option<Param>) -> Chan {
        Chan(p.map_or(0, |x| x.0.min(255)) as u8)
    }
    pub fn from_offset(v: u16, base: u16) -> Chan {
        Chan(v.saturating_sub(base) as u8)
    }
    pub fn get(self) -> u8 {
        self.0
    }
    /// A channel that is already eight bits wide -- from an X11 colour spec
    /// rather than from a CSI parameter.
    pub fn from_u8(v: u8) -> Chan {
        Chan(v)
    }
    /// As a palette subscript. The palette has exactly 256 entries because a
    /// `Chan` has exactly 256 values, so this cannot be out of range.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Default,
    Indexed(Chan),
    Rgb(Chan, Chan, Chan),
}

/// One resolved SGR attribute.
pub enum SgrAttr {
    Reset,
    Set(Attrs),
    Unset(Attrs),
    Fg(Color),
    Bg(Color),
}

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
            // 5 slow blink, 6 rapid blink. `[[source: ecma48]]` § 8.3.117
            // distinguishes them by rate; a terminal core has no timer and
            // nothing downstream can use the distinction, so both set BLINK.
            5 | 6 => (SgrAttr::Set(Attrs::BLINK), ParamCount(1)),
            7 => (SgrAttr::Set(Attrs::REVERSE), ParamCount(1)),
            8 => (SgrAttr::Set(Attrs::HIDDEN), ParamCount(1)),
            9 => (SgrAttr::Set(Attrs::STRIKE), ParamCount(1)),
            // 21 is doubly-underlined in ECMA-48 3rd and in xterm. baud has one
            // underline bit, so a double underline is an underline -- losing the
            // doubling is visibly wrong but losing the underline entirely is
            // worse, and inventing a second bit no renderer reads would be the
            // "set and never used" failure.
            21 => (SgrAttr::Set(Attrs::UNDERLINE), ParamCount(1)),
            22 => (SgrAttr::Unset(Attrs::BOLD.with(Attrs::DIM)), ParamCount(1)),
            23 => (SgrAttr::Unset(Attrs::ITALIC), ParamCount(1)),
            24 => (SgrAttr::Unset(Attrs::UNDERLINE), ParamCount(1)),
            25 => (SgrAttr::Unset(Attrs::BLINK), ParamCount(1)),
            27 => (SgrAttr::Unset(Attrs::REVERSE), ParamCount(1)),
            28 => (SgrAttr::Unset(Attrs::HIDDEN), ParamCount(1)),
            29 => (SgrAttr::Unset(Attrs::STRIKE), ParamCount(1)),
            30..=37 => (
                SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 30))),
                ParamCount(1),
            ),
            40..=47 => (
                SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 40))),
                ParamCount(1),
            ),
            39 => (SgrAttr::Fg(Color::Default), ParamCount(1)),
            49 => (SgrAttr::Bg(Color::Default), ParamCount(1)),
            // aixterm bright colours, `[[source: ctlseqs]]` § SGR: "aixterm
            // colors are the bright versions of the ISO colors". They are
            // indices 8..15 of the 256-colour palette, so they need no separate
            // representation -- 90 is index 8, not a new kind of colour.
            90..=97 => (
                SgrAttr::Fg(Color::Indexed(Chan::from_offset(v, 90 - 8))),
                ParamCount(1),
            ),
            100..=107 => (
                SgrAttr::Bg(Color::Indexed(Chan::from_offset(v, 100 - 8))),
                ParamCount(1),
            ),
            38 | 48 => {
                let mk = |c| {
                    if v == 38 {
                        SgrAttr::Fg(c)
                    } else {
                        SgrAttr::Bg(c)
                    }
                };
                // Two spellings, and the difference is not cosmetic.
                //
                // `[[source: ctlseqs]]` gives the ISO 8613-6 form as
                // `38 : 2 : Pi : Pr : Pg : Pb` -- six sub-parameters, with a
                // colour-space identifier `Pi` that "is ignored" -- and the
                // konsole-compatible legacy form as `38 ; 2 ; Pr ; Pg ; Pb`,
                // five parameters with no `Pi`. Reading the colon form as the
                // semicolon form takes `Pi` for red and shifts every channel
                // by one, which is the bug this whole distinction exists to
                // avoid. Some emitters also write `38:2:r:g:b` with no `Pi`,
                // so the group length decides rather than the separator alone.
                let group = params.group_len(ParamIdx::FIRST);
                let colon_form = group > 1;
                match params.at(ParamIdx::SECOND).map(|x| x.0) {
                    Some(5) => {
                        let n = mk(Color::Indexed(Chan::from_param(params.at(ParamIdx::THIRD))));
                        // The whole colon group is one attribute, however long
                        // the emitter made it.
                        (n, ParamCount(if colon_form { group } else { 3 }))
                    }
                    Some(2) => {
                        // Where the channels start: after `Pi` in the six-part
                        // colon form, immediately in every other spelling.
                        let base = if colon_form && group >= 6 {
                            ParamIdx::FOURTH
                        } else {
                            ParamIdx::THIRD
                        };
                        let c = mk(Color::Rgb(
                            Chan::from_param(params.at(base)),
                            Chan::from_param(params.at(base.plus(1))),
                            Chan::from_param(params.at(base.plus(2))),
                        ));
                        (c, ParamCount(if colon_form { group } else { 5 }))
                    }
                    _ => return None,
                }
            }
            _ => return None,
        })
    }
}

/// An OSC (Operating System Command) payload, resolved.
///
/// `[[source: ctlseqs]]` § "Set Text Parameters" gives the form as
/// `OSC Ps ; Pt BEL` or `OSC Ps ; Pt ST`, and both terminators are accepted:
/// "XTerm accepts either BEL or ST for terminating OSC sequences, and when
/// returning information, uses the same terminator used in a query."
///
/// Everything here is attacker-controlled: a window title is whatever the
/// remote end of an ssh session cared to send. Nothing in this type owns a
/// growable buffer, and the payload the core keeps is bounded and truncated
/// rather than rejected -- a title that is too long should be shortened, not
/// dropped, and must never be a reason to allocate.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Osc<'a> {
    /// `OSC 0` — "Change Icon Name and Window Title to Pt."
    SetIconAndTitle(&'a [u8]),
    /// `OSC 1` — "Change Icon Name to Pt."
    SetIcon(&'a [u8]),
    /// `OSC 2` — "Change Window Title to Pt."
    SetTitle(&'a [u8]),
    /// `OSC 4 ; c ; spec` — "Change Color Number c to the color specified by
    /// spec." Only the first pair is taken; see [`Osc::parse`].
    SetColor { index: Chan, spec: XColor<'a> },
    /// `OSC 104` — reset the palette. `None` resets every entry, which is what
    /// an omitted parameter means.
    ResetColor(Option<Chan>),
    /// `OSC 52` — clipboard. Carried so the core can *refuse* the read
    /// deliberately rather than by omission; see the note on
    /// [`ClipboardOp::Read`].
    Clipboard(ClipboardOp<'a>),
    /// Recognised as an OSC and not acted on.
    Unsupported,
}

/// The two halves of `OSC 52`, kept apart because they have very different
/// security properties.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClipboardOp<'a> {
    /// `OSC 52 ; c ; <base64>` — set the clipboard.
    Write(&'a [u8]),
    /// `OSC 52 ; c ; ?` — *read* the clipboard back to the application.
    ///
    /// A hostile program on the far end of an ssh session can use this to
    /// exfiltrate whatever the user last copied, which may be a password. xterm
    /// disables it by default and so does baud: the variant exists so the
    /// refusal is a decision recorded in the type rather than a sequence that
    /// happens to fall through.
    Read,
}

/// What the numeric keypad sends. DECKPAM / DECKPNM.
///
/// `[[source: ctlseqs]]`: "ESC = Application Keypad (DECKPAM)" and
/// "ESC > Normal Keypad (DECKPNM), VT100."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeypadMode {
    /// Digits and operators, as printed on the keys.
    #[default]
    Numeric,
    /// `SS3`-prefixed sequences, so an application can tell keypad `5` from the
    /// `5` on the top row.
    Application,
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
        }
    }
}

/// Modifier keys held during a mouse event.
///
/// `[[source: ctlseqs]]`: "The next three bits encode the modifiers which were
/// down when the button was pressed and are added together: 4=Shift, 8=Meta,
/// and 16=Control."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct MouseMods(u16);

impl MouseMods {
    pub const NONE: MouseMods = MouseMods(0);
    pub const SHIFT: MouseMods = MouseMods(4);
    pub const META: MouseMods = MouseMods(8);
    pub const CONTROL: MouseMods = MouseMods(16);
    pub fn with(self, o: MouseMods) -> MouseMods {
        MouseMods(self.0 | o.0)
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

/// One mouse event, in cells.
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
}

/// Which mouse events an application asked to hear about.
///
/// `[[source: ctlseqs]]`: 1000 is normal tracking (press and release), 1002
/// adds button-motion, 1003 reports "all motion events, even if no mouse button
/// is down".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseTracking {
    #[default]
    Off,
    /// `?1000`
    Normal,
    /// `?1002`
    ButtonEvent,
    /// `?1003`
    AnyEvent,
}

/// A character set designated into one of the G0-G3 slots.
///
/// `[[source: ctlseqs]]` lists the final characters for `ESC ( C` and friends.
/// baud distinguishes only the two that change behaviour here -- ASCII and the
/// line-drawing set -- and keeps the rest as `Other` so that designating one is
/// *recorded* rather than mistaken for ASCII.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Charset {
    /// `B` (USASCII), and `A` (British Latin-1), whose difference is one glyph
    /// baud does not special-case.
    #[default]
    Ascii,
    /// `0` — "DEC Special Character and Line Drawing Set, VT100."
    DecGraphics,
    /// Designated, recognised, and not translated. See [`CellFlags::GRAPHICS`]
    /// for why translation needs a source baud does not have.
    Other,
}

impl Charset {
    pub fn from_final(b: StreamByte) -> Charset {
        match b.raw_byte() {
            b'0' => Charset::DecGraphics,
            b'B' | b'A' => Charset::Ascii,
            _ => Charset::Other,
        }
    }
}

/// Which G-slot an `ESC ( ) * +` designates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GSlot {
    G0,
    G1,
    G2,
    G3,
}

impl GSlot {
    /// The intermediate byte that selects the slot: `(` is G0, `)` G1, `*` G2,
    /// `+` G3.
    pub fn from_intermediate(b: StreamByte) -> Option<GSlot> {
        Some(match b.raw_byte() {
            b'(' => GSlot::G0,
            b')' => GSlot::G1,
            b'*' => GSlot::G2,
            b'+' => GSlot::G3,
            _ => return None,
        })
    }
    pub fn index(self) -> usize {
        self as usize
    }
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

/// An X11 colour specification, as `XParseColor` accepts it.
///
/// `[[source: ctlseqs]]`: "The spec can be a name or RGB specification as per
/// XParseColor." Names are deliberately *not* resolved -- that needs the X11
/// `rgb.txt` table, which baud has no citable copy of, and inventing the
/// mapping would be guessing at a protocol. A name parses to
/// [`XColor::Unresolved`] and is ignored rather than approximated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XColor<'a> {
    Rgb(Chan, Chan, Chan),
    /// `?` — a query. The reply must have the same shape as the setter.
    Query,
    Unresolved(&'a [u8]),
}

impl<'a> XColor<'a> {
    /// `rgb:R/G/B` with 1-4 hex digits per channel, or `#RGB` / `#RRGGBB` /
    /// `#RRRGGGBBB` / `#RRRRGGGGBBBB`.
    ///
    /// Channels are scaled to 8 bits by taking the *high* bits, which is what
    /// XParseColor does: `rgb:f/f/f` is white, not rgb(15,15,15).
    pub fn parse(s: &'a [u8]) -> XColor<'a> {
        if s == b"?" {
            return XColor::Query;
        }
        if let Some(rest) = strip(s, b"rgb:") {
            let mut it = Split::new(rest, b'/');
            if let (Some(r), Some(g), Some(b), None) = (it.next(), it.next(), it.next(), it.next())
            {
                if let (Some(r), Some(g), Some(b)) = (hex_chan(r), hex_chan(g), hex_chan(b)) {
                    return XColor::Rgb(r, g, b);
                }
            }
            return XColor::Unresolved(s);
        }
        if let Some(rest) = strip(s, b"#") {
            let n = rest.len();
            if n % 3 == 0 && (3..=12).contains(&n) {
                let w = n / 3;
                if let (Some(r), Some(g), Some(b)) = (
                    hex_chan(&rest[..w]),
                    hex_chan(&rest[w..2 * w]),
                    hex_chan(&rest[2 * w..]),
                ) {
                    return XColor::Rgb(r, g, b);
                }
            }
        }
        XColor::Unresolved(s)
    }
}

fn strip<'a>(s: &'a [u8], p: &[u8]) -> Option<&'a [u8]> {
    (s.len() >= p.len() && &s[..p.len()] == p).then(|| &s[p.len()..])
}

/// One to four hex digits, scaled to eight bits from the top.
fn hex_chan(s: &[u8]) -> Option<Chan> {
    if s.is_empty() || s.len() > 4 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u32;
    }
    // Scale so the widest digit run maps its maximum to 255: `f` -> 255,
    // `ffff` -> 255. Left-align then take the top byte.
    let bits = s.len() * 4;
    let max = (1u32 << bits) - 1;
    Some(Chan::from_u8(((v * 255 + max / 2) / max) as u8))
}

/// Split a byte slice on a separator without allocating.
struct Split<'a> {
    rest: Option<&'a [u8]>,
    sep: u8,
}

impl<'a> Split<'a> {
    fn new(s: &'a [u8], sep: u8) -> Split<'a> {
        Split { rest: Some(s), sep }
    }
}

impl<'a> Iterator for Split<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let cur = self.rest?;
        match cur.iter().position(|&b| b == self.sep) {
            Some(i) => {
                self.rest = Some(&cur[i + 1..]);
                Some(&cur[..i])
            }
            None => {
                self.rest = None;
                Some(cur)
            }
        }
    }
}

impl<'a> Osc<'a> {
    /// Resolve an OSC payload -- the bytes between the introducer and the
    /// string terminator, with neither included.
    ///
    /// `OSC 4` may carry "any number of c/spec pairs"; baud takes the first and
    /// ignores the rest, which is a deliberate under-implementation rather than
    /// a silent one: applications emit one pair per sequence in practice, and
    /// handling n pairs means n replies from a fixed reply buffer.
    pub fn parse(payload: &'a [u8]) -> Osc<'a> {
        let mut it = Split::new(payload, b';');
        let Some(ps) = it.next() else {
            return Osc::Unsupported;
        };
        let rest = match payload.len() > ps.len() {
            true => &payload[ps.len() + 1..],
            false => &payload[payload.len()..],
        };
        match ps {
            b"0" => Osc::SetIconAndTitle(rest),
            b"1" => Osc::SetIcon(rest),
            b"2" => Osc::SetTitle(rest),
            b"4" => {
                let mut p = Split::new(rest, b';');
                match (p.next(), p.next()) {
                    (Some(c), Some(spec)) => match dec_chan(c) {
                        Some(index) => Osc::SetColor {
                            index,
                            spec: XColor::parse(spec),
                        },
                        None => Osc::Unsupported,
                    },
                    _ => Osc::Unsupported,
                }
            }
            b"104" => {
                if rest.is_empty() {
                    Osc::ResetColor(None)
                } else {
                    match dec_chan(rest) {
                        Some(c) => Osc::ResetColor(Some(c)),
                        None => Osc::Unsupported,
                    }
                }
            }
            b"52" => {
                let mut p = Split::new(rest, b';');
                let (_sel, data) = (p.next(), p.next());
                match data {
                    Some(b"?") => Osc::Clipboard(ClipboardOp::Read),
                    Some(d) => Osc::Clipboard(ClipboardOp::Write(d)),
                    None => Osc::Unsupported,
                }
            }
            _ => Osc::Unsupported,
        }
    }
}

/// A decimal palette index, 0-255.
fn dec_chan(s: &[u8]) -> Option<Chan> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let mut v: u32 = 0;
    for &b in s {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as u32;
    }
    (v <= 255).then(|| Chan::from_u8(v as u8))
}

/// DECSCUSR cursor shapes, `CSI Ps SP q`.
///
/// `[[source: ctlseqs]]`: "0 -> blinking block. 1 -> blinking block (default).
/// 2 -> steady block. 3 -> blinking underline. 4 -> steady underline.
/// 5 -> blinking bar, xterm. 6 -> steady bar, xterm."
///
/// Shape and blink are kept as one value rather than split into a shape plus a
/// flag, because the wire has seven states and only six are a clean product --
/// 0 and 1 are the same shape, so a split would have to invent a rule for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorStyle {
    #[default]
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl CursorStyle {
    /// `Ps = 7` ("initial resources") means the power-on style, which for baud
    /// is the default -- there are no resources to restore from.
    pub fn from_param(p: Option<Param>) -> Option<CursorStyle> {
        Some(match p.map_or(0, |x| x.0) {
            0 | 1 | 7 => CursorStyle::BlinkingBlock,
            2 => CursorStyle::SteadyBlock,
            3 => CursorStyle::BlinkingUnderline,
            4 => CursorStyle::SteadyUnderline,
            5 => CursorStyle::BlinkingBar,
            6 => CursorStyle::SteadyBar,
            _ => return None,
        })
    }
    /// Does this style blink? A renderer needs the two axes even though the
    /// wire encodes them together.
    pub fn blinks(self) -> bool {
        matches!(
            self,
            CursorStyle::BlinkingBlock | CursorStyle::BlinkingUnderline | CursorStyle::BlinkingBar
        )
    }
}

/// What DECRQM answers about one mode.
///
/// `[[source: ctlseqs]]`: "0 - not recognized, 1 - set, 2 - reset,
/// 3 - permanently set, 4 - permanently reset".
///
/// `NotRecognized` is the important one and it is not a failure case: an
/// application uses DECRQM precisely to find out whether a mode exists, so
/// answering 0 for a mode baud does not implement is the *correct* answer.
/// Answering 2 ("reset") instead would tell it the mode exists and is off,
/// and it would then set the mode and believe it took effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeState {
    NotRecognized,
    Set,
    Reset,
    PermanentlySet,
    PermanentlyReset,
}

impl ModeState {
    fn wire(self) -> u16 {
        match self {
            ModeState::NotRecognized => 0,
            ModeState::Set => 1,
            ModeState::Reset => 2,
            ModeState::PermanentlySet => 3,
            ModeState::PermanentlyReset => 4,
        }
    }
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
    MouseSgr {
        cb: u16,
        col: OneBased,
        row: OneBased,
        release: bool,
    },
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
    len: usize,
}

impl ReportBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
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
            len: 0,
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
            Report::MouseSgr {
                cb,
                col,
                row,
                release,
            } => {
                e.push(b'<');
                e.push_num(cb);
                e.push(b';');
                e.push_num(col.get());
                e.push(b';');
                e.push_num(row.get());
                e.push(if release { b'm' } else { b'M' });
            }
            Report::Version => {
                // Not a CSI: `DCS > | <text> ST`. The introducer written here
                // replaces the `ESC [` the encoder starts with, so the branch
                // rewinds rather than appending.
                e.len = 0;
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
        if self.len < Report::MAX_LEN {
            self.buf[self.len] = b;
            self.len += 1;
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

/// ANSI modes, the unprefixed `CSI Pm h` / `CSI Pm l` namespace.
///
/// A different namespace from [`PrivateMode`]: `CSI 4 h` sets insert mode and
/// `CSI ? 4 h` sets DECSCLM (smooth scroll). Conflating them is a classic bug,
/// which is why they are separate types rather than a shared integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnsiMode {
    /// IRM, mode 4. Printing inserts rather than overwrites.
    InsertReplace,
    /// LNM, mode 20. When set, LF also performs a carriage return.
    LineFeedNewLine,
    Other(Param),
}
impl AnsiMode {
    pub fn from_wire(p: Option<Param>) -> AnsiMode {
        match p.map_or(0, |x| x.0) {
            4 => AnsiMode::InsertReplace,
            20 => AnsiMode::LineFeedNewLine,
            n => AnsiMode::Other(Param(n)),
        }
    }
}

/// One bit of terminal mode state.
///
/// Only the modes baud actually acts on. A mode it parses but does not
/// implement must NOT appear here: a bit that is set and never read is exactly
/// the "accepted, did nothing, no way to find out" failure that
/// `docs/vt-coverage.md` calls `parsed`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeBit {
    /// LNM. Off by default: `[[source: ctlseqs]]` lists mode 20 as reset, so
    /// LF moves down without returning to column 1.
    LineFeedNewLine,
    /// IRM.
    InsertReplace,
    /// DECOM, origin mode: cursor addressing is relative to the scroll region.
    Origin,
    /// DECAWM, autowrap. Set by default.
    AutoWrap,
    /// DECTCEM. Set by default.
    CursorVisible,
    /// DECSET ?2004. Read by the embedder to decide whether to wrap a paste in
    /// `ESC [ 200 ~` / `ESC [ 201 ~`.
    ///
    /// The core has no input path, so "acting on it" means exposing it --
    /// `Term::bracketed_paste()`. That is not the "set and never read" failure:
    /// an embedder cannot implement the mode without being told, and without
    /// the bit a pasted newline executes as typed input, which is the
    /// paste-jacking class rather than a cosmetic gap.
    BracketedPaste,
    /// DECSET ?2026. Read by the renderer to hold the frame.
    ///
    /// A *hint*: ignoring it is correct-but-worse, so an embedder that never
    /// reads it still renders correctly, just with tearing.
    SynchronizedOutput,
}
impl ModeBit {
    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Terminal mode state as a bitset.
///
/// A struct of `bool` fields would be the obvious representation and is what
/// the audit gate exists to prevent: each would be an untyped flag, and
/// `self.lnm = self.decom` would compile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modes(u16);
impl Default for Modes {
    /// DECAWM and DECTCEM are set on a fresh terminal; everything else reset.
    fn default() -> Modes {
        Modes(ModeBit::AutoWrap.bit() | ModeBit::CursorVisible.bit())
    }
}
impl Modes {
    pub fn is_set(self, m: ModeBit) -> bool {
        self.0 & m.bit() != 0
    }
    pub fn set(&mut self, m: ModeBit) {
        self.0 |= m.bit();
    }
    pub fn reset(&mut self, m: ModeBit) {
        self.0 &= !m.bit();
    }
    /// `CSI Pm h` and `CSI Pm l` differ only in this bit, so they share a path.
    pub fn apply(&mut self, m: ModeBit, on: bool) {
        if on { self.set(m) } else { self.reset(m) }
    }
}

/// DEC private modes, named. `?1049` and `?25` are not interchangeable
/// integers — one swaps the screen buffer, the other hides the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivateMode {
    /// DECOM, ?6. Cursor addressing relative to the margins, and confined
    /// to them.
    Origin,
    /// DECAWM, ?7.
    AutoWrap,
    CursorVisible,      // ?25
    BracketedPaste,     // ?2004
    AltScreen,          // ?1049
    SynchronizedOutput, // ?2026
    /// `?1000` normal tracking.
    MouseNormal,
    /// `?1002` button-event tracking (adds motion while a button is down).
    MouseButtonEvent,
    /// `?1003` any-event tracking (adds motion with no button down).
    MouseAnyEvent,
    /// `?1006` SGR encoding.
    MouseSgr,
    Other(Param),
}
impl PrivateMode {
    pub fn from_wire(p: Option<Param>) -> PrivateMode {
        match p.map_or(0, |x| x.0) {
            6 => PrivateMode::Origin,
            7 => PrivateMode::AutoWrap,
            25 => PrivateMode::CursorVisible,
            1000 => PrivateMode::MouseNormal,
            1002 => PrivateMode::MouseButtonEvent,
            1003 => PrivateMode::MouseAnyEvent,
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
    /// NUL. Discarded, never printed: `[[source: ecma48]]` § 8.3.88 makes it a
    /// fill character with no effect on the presentation.
    pub const NUL: StreamByte = StreamByte(0x00);
    /// DEL. `[[source: williams]]` gives it the `ignore` action inside escape
    /// and CSI sequences: it neither ends them nor contributes to them.
    pub const DEL: StreamByte = StreamByte(0x7f);
    /// The intermediate of `ESC # 8` (DECALN).
    pub const HASH: StreamByte = StreamByte(b'#');
    /// VT. `[[source: ctlseqs]]` treats it as an index, like LF.
    pub const VT: StreamByte = StreamByte(0x0b);
    /// FF. Also an index -- a terminal does not eject a page.
    pub const FF: StreamByte = StreamByte(0x0c);
    /// SO / SI, the G1 and G0 shifts. Consumed so they are not printed; the
    /// charset they select is not implemented -- see docs/vt-coverage.md.
    pub const SO: StreamByte = StreamByte(0x0e);
    pub const SI: StreamByte = StreamByte(0x0f);
    pub const SEMICOLON: StreamByte = StreamByte(b';');

    /// The ISO 8613-6 sub-parameter separator. A *parameter* byte (0x3a), not
    /// an intermediate -- which is why it must be handled in the parameter
    /// loop and not by `is_csi_intermediate`.
    pub const COLON: StreamByte = StreamByte(b':');
    pub const QUESTION: StreamByte = StreamByte(b'?');
    /// `>`, the secondary CSI marker. A parameter byte like `?`, and a
    /// different namespace from it.
    pub const GT: StreamByte = StreamByte(b'>');
    pub const CSI_INTRO: StreamByte = StreamByte(b'[');
    pub const OSC_INTRO: StreamByte = StreamByte(b']');
    /// `ESC P`. Device Control String: a payload that must be consumed, never
    /// executed.
    pub const DCS_INTRO: StreamByte = StreamByte(b'P');
    /// `ESC X`, `ESC ^`, `ESC _`: SOS, PM, APC. Same rule as DCS.
    pub const SOS_INTRO: StreamByte = StreamByte(b'X');
    pub const PM_INTRO: StreamByte = StreamByte(b'^');
    pub const APC_INTRO: StreamByte = StreamByte(b'_');
    /// `ESC \`, the string terminator's final byte.
    pub const ST_FINAL: StreamByte = StreamByte(b'\\');
    pub fn is_csi_intermediate(self) -> bool {
        (0x20..0x30).contains(&self.0)
    }
    /// Intermediate bytes after `ESC`, per `[[source: williams]]`'s
    /// `escape_intermediate`. Same range as the CSI intermediates: `ESC # 8` is
    /// DECALN, and without this the `#` is dropped and the `8` printed.
    pub fn is_esc_intermediate(self) -> bool {
        (0x20..0x30).contains(&self.0)
    }
    /// A byte that ends an escape sequence: `0x30..=0x7e`.
    pub fn is_esc_final(self) -> bool {
        (0x30..0x7f).contains(&self.0)
    }
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
        (self.0 & 0xc0 == 0x80).then_some((self.0 & 0x3f) as u32)
    }
}

impl Width {
    /// East Asian Width, from Unicode data. No font input, ever — see
    /// [`DisplayWidth`] for why that is a wire contract rather than a choice.
    pub fn of(c: Codepoint) -> Width {
        let cp = c.value();
        if cp == 0 {
            return Width::ZERO;
        }
        if (0x300..=0x36f).contains(&cp)
            || (0x1ab0..=0x1aff).contains(&cp)
            || (0x20d0..=0x20ff).contains(&cp)
            || (0xfe00..=0xfe0f).contains(&cp)
            || (0x200b..=0x200f).contains(&cp)
        {
            return Width::ZERO;
        }
        if cp >= 0x1100
            && (cp <= 0x115f || cp == 0x2329 || cp == 0x232a
            || (0x2e80..=0xa4cf).contains(&cp) || (0xac00..=0xd7a3).contains(&cp)
            || (0xf900..=0xfaff).contains(&cp) || (0xfe30..=0xfe6f).contains(&cp)
            || (0xff00..=0xff60).contains(&cp) || (0xffe0..=0xffe6).contains(&cp)
            || (0x1f300..=0x1f64f).contains(&cp)   // Misc Symbols, Emoticons
            || (0x1f680..=0x1f6ff).contains(&cp)   // Transport and Map  <- was missing
            || (0x1f7e0..=0x1f7eb).contains(&cp)   // geometric shapes ext
            || (0x1f90c..=0x1f9ff).contains(&cp)   // Supplemental Symbols
            || (0x1fa70..=0x1faff).contains(&cp)   // Symbols Extended-A
            || (0x20000..=0x3fffd).contains(&cp))
        {
            return Width::WIDE;
        }
        Width::NARROW
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
}

impl Count {
    pub const ONE: Count = Count(1);
    pub fn get_raw(self) -> u16 {
        self.0
    }
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
        // 64-bit now that a rendition is more than a colour, but the same
        // lesson: take the HIGH bits. The 32-bit constant is replaced by the
        // 64-bit Fibonacci one for the wider key.
        let shift = 64 - buckets.trailing_zeros();
        (raw.key().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
    }
    pub fn from_index(i: usize) -> StyleId {
        StyleId(i as u16)
    }
    pub fn index(self) -> usize {
        self.0 as usize
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

impl SgrAttr {
    /// Parse the attribute starting at `i`.
    pub fn parse_at(params: &[Option<Param>], i: ParamIdx) -> Option<(SgrAttr, ParamCount)> {
        SgrAttr::parse(Params::new(&params[i.0 as usize..]))
    }
    /// As [`SgrAttr::parse_at`], but keeping the sub-parameter mask aligned
    /// with the slice. Dropping the mask here would make every attribute after
    /// the first look like the legacy form.
    pub fn parse_sub(params: Params<'_>, i: ParamIdx) -> Option<(SgrAttr, ParamCount)> {
        SgrAttr::parse(params.skip(i.0))
    }
    /// Apply to a packed 24-bit colour + attribute word.
    /// Apply one attribute to the accumulating rendition.
    ///
    /// Every variant is handled. It used to fall through to `cur.bits()` for
    /// `Set`, `Unset` and every `Bg`, so bold, italic, underline, reverse,
    /// blink, conceal, strike and all background colours were parsed and then
    /// silently discarded.
    pub fn fold(self, cur: StyleWord) -> StyleWord {
        match self {
            // `[[source: ecma48]]` § 8.3.117: SGR 0 "cancels the effect of any
            // preceding occurrence of SGR in the data stream".
            SgrAttr::Reset => StyleWord::DEFAULT,
            SgrAttr::Set(a) => cur.with_attrs(cur.attrs().with(a)),
            SgrAttr::Unset(a) => cur.with_attrs(cur.attrs().without(a)),
            SgrAttr::Fg(c) => cur.with_fg(ColorWord::from_color(c)),
            SgrAttr::Bg(c) => cur.with_bg(ColorWord::from_color(c)),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// What an implementation needs so it never writes `y * STRIDE + x`
// ═══════════════════════════════════════════════════════════════════════

/// Read-only counterpart of [`View`].
pub struct ViewRef<'a> {
    cells: &'a [PackedCell],
    stride: Cols,
    cols: Cols,
    rows: Rows,
}
impl<'a> ViewRef<'a> {
    pub fn row_slice(&self, r: Row) -> &'a [PackedCell] {
        let a = r.index() * self.stride.index();
        &self.cells[a..a + self.cols.index()]
    }
    pub fn get(&self, r: Row, c: Col) -> PackedCell {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            self.cells[r.index() * self.stride.index() + c.index()]
        } else {
            PackedCell::EMPTY
        }
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
        ViewRef {
            cells,
            stride,
            cols,
            rows,
        }
    }
    pub fn new(cells: &'a mut [PackedCell], stride: Cols, cols: Cols, rows: Rows) -> View<'a> {
        View {
            cells,
            stride,
            cols,
            rows,
        }
    }
    pub fn cols(&self) -> Cols {
        self.cols
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }
    pub fn set_geometry(&mut self, cols: Cols, rows: Rows) {
        self.cols = cols;
        self.rows = rows;
    }

    #[inline(always)]
    fn at(&self, r: Row, c: Col) -> usize {
        r.index() * self.stride.index() + c.index()
    }

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
        } else {
            PackedCell::EMPTY
        }
    }
    /// Write a run of printable ASCII into one row, starting at `at`.
    ///
    /// Bulk rather than per-cell: the bounds check and the row base are
    /// computed once. Doing it per cell cost ~25% of ingest throughput, which
    /// was a refactoring mistake rather than anything to do with the types.
    ///
    /// The indexed loop stays. Clippy asks for `src.iter().enumerate()`, which is
    /// a codegen change on the measured ingest path — and the note above is the
    /// measurement that says this loop's shape is worth 25%. Changing it needs a
    /// benchmark, not a lint fix.
    #[allow(clippy::needless_range_loop)]
    pub fn put_ascii_run(&mut self, r: Row, at: Col, src: &[u8], style: StyleId, flags: CellFlags) {
        if r.index() >= self.rows.index() {
            return;
        }
        let base = self.at(r, Col::ZERO);
        let start = at.index();
        let n = src.len().min(self.cols.index().saturating_sub(start));
        // Two loops, and the duplication is the point.
        //
        // Threading `flags` as a *runtime* value into the per-cell store cost
        // 5% of the cells path -- 1049.5 MB/s against 1104.2 for the same build
        // with a constant, ranges not overlapping. With `CellFlags::NONE`
        // spelled as a literal the compiler folds the whole non-content half
        // into an immediate and the loop goes back to what it was: the same
        // build measured 1127.0 [1106.1-1135.9].
        //
        // DECSCA is rare -- essentially no application sets it -- so the branch
        // is hoisted out of the loop and the cost falls on the programs that
        // actually ask for protection. Same shape as the AutoWrap hoist in
        // `baud-core`, and for the same measured reason.
        if flags == CellFlags::NONE {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags: CellFlags::NONE,
            };
            for z in 0..n {
                self.cells[base + start + z] = proto.with_ascii(StreamByte::from_stream(src[z]));
            }
        } else {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags,
            };
            for z in 0..n {
                self.cells[base + start + z] = proto.with_ascii(StreamByte::from_stream(src[z]));
            }
        }
    }

    /// Fill a half-open column span of one row. Clamped to the visible width.
    pub fn fill(&mut self, r: Row, from: Col, to: Col, cell: PackedCell) {
        let (a, b) = (from.index(), to.index().min(self.cols.index()));
        if r.index() >= self.rows.index() || a >= b {
            return;
        }
        let base = self.at(r, Col::ZERO);
        for x in a..b {
            self.cells[base + x] = cell;
        }
    }
    pub fn fill_row(&mut self, r: Row, cell: PackedCell) {
        self.fill(r, Col::ZERO, Col::at_index(self.cols.index()), cell);
    }
    /// Shift a row's cells left by `n`, blanking the tail. `CSI P`.
    pub fn delete_chars(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        let base = self.at(r, Col::ZERO);
        for x in cx..cols - n {
            self.cells[base + x] = self.cells[base + x + n];
        }
        for x in cols - n..cols {
            self.cells[base + x] = blank;
        }
    }
    /// Shift right, inserting blanks. `CSI @`.
    pub fn insert_blanks(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        let base = self.at(r, Col::ZERO);
        let mut x = cols;
        while x > cx + n {
            x -= 1;
            self.cells[base + x] = self.cells[base + x - n];
        }
        for x in cx..cx + n {
            self.cells[base + x] = blank;
        }
    }
    /// Scroll rows down from `at`, blanking the opened region. `CSI L`.
    ///
    /// Bounded by the last row of the screen. `CSI L` inside a scrolling region
    /// must stop at the region's bottom instead -- see [`Grid::insert_lines_in`].
    pub fn insert_lines(&mut self, at: Row, n: Count, blank: PackedCell) {
        let last = Row::from_index(self.rows.index() - 1);
        self.insert_lines_in(at, n, last, blank);
    }

    /// Scroll rows down from `at`, stopping at `bottom` inclusive.
    ///
    /// Rows below `bottom` are outside the scrolling region and must not move:
    /// ECMA-48 defines IL as operating within the region when the cursor is
    /// inside it, and DECSTBM is what makes the region smaller than the screen
    /// `[[source: ecma48]]` § 8.3.67. Content scrolled past `bottom` is
    /// discarded rather than pushed off the screen.
    pub fn insert_lines_in(&mut self, at: Row, n: Count, bottom: Row, blank: PackedCell) {
        let (cy, bot) = (at.index(), bottom.index().min(self.rows.index() - 1));
        if cy > bot {
            return;
        }
        let n = (n.get_raw() as usize).min(bot + 1 - cy);
        let (s, cols) = (self.stride.index(), self.cols.index());
        let mut y = bot + 1;
        while y > cy + n {
            y -= 1;
            for x in 0..cols {
                self.cells[y * s + x] = self.cells[(y - n) * s + x];
            }
        }
        for y in cy..cy + n {
            for x in 0..cols {
                self.cells[y * s + x] = blank;
            }
        }
    }

    /// Scroll rows up from `at`, stopping at `bottom` inclusive, blanking the
    /// rows exposed at the bottom. `CSI M` (DL), and the engine of SU and of a
    /// line feed on the last row of a scrolling region.
    pub fn delete_lines_in(&mut self, at: Row, n: Count, bottom: Row, blank: PackedCell) {
        let (cy, bot) = (at.index(), bottom.index().min(self.rows.index() - 1));
        if cy > bot {
            return;
        }
        let n = (n.get_raw() as usize).min(bot + 1 - cy);
        let (s, cols) = (self.stride.index(), self.cols.index());
        // `cy..(bot + 1 - n)`, not `cy..=(bot - n)`: with n saturated to the
        // region height, `bot - n` underflows when the whole region is deleted
        // (n == bot + 1, cy == 0). The half-open form is empty in that case
        // instead of panicking, which is what DL with a huge count must do.
        for y in cy..bot + 1 - n {
            for x in 0..cols {
                self.cells[y * s + x] = self.cells[(y + n) * s + x];
            }
        }
        for y in bot + 1 - n..=bot {
            for x in 0..cols {
                self.cells[y * s + x] = blank;
            }
        }
    }
    pub fn row_slice(&self, r: Row) -> &[PackedCell] {
        let a = self.at(r, Col::ZERO);
        &self.cells[a..a + self.cols.index()]
    }
}

impl PackedCell {
    /// Unwritten. Note this is *not* a space: the renderer decides what an empty
    /// cell looks like, and selection can tell the difference.
    pub const EMPTY: PackedCell = PackedCell {
        content: Content::EMPTY,
        style: StyleId::DEFAULT,
        flags: CellFlags::NONE,
    };
    /// An erased cell.
    ///
    /// Takes no flags on purpose: an erase leaves a position with no character
    /// in it, and `[[source: vt510-decsca]]` scopes protection to "characters
    /// that follow the DECSCA function". A blank that inherited the protection
    /// would make the first selective erase under `DECSCA 1` protect everything
    /// it had just cleared.
    pub fn empty_with(style: StyleId) -> PackedCell {
        PackedCell {
            content: Content::EMPTY,
            style,
            flags: CellFlags::NONE,
        }
    }
    /// A printable ASCII byte. The proof that it is printable is also the proof
    /// that its codepoint equals its byte value.
    pub fn from_ascii(b: StreamByte, style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::codepoint(Codepoint::from_ascii(b)),
            style,
            flags,
        }
    }
    pub fn glyph(c: Codepoint, style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::codepoint(c),
            style,
            flags,
        }
    }
    pub fn wide_tail(style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::WIDE_TAIL,
            style,
            flags,
        }
    }
    pub fn is_protected(self) -> bool {
        self.flags.is_protected()
    }
    /// This cell's style and flags with a different character in it.
    ///
    /// The whole point is that the tail is *already assembled*. `PackedCell` is
    /// `{u32, u16, u8, u8}`, and building one field-by-field inside a per-cell
    /// loop let the compiler emit four narrow stores instead of one 8-byte one
    /// -- measured at ~8% of the cells path when the flags byte was carved out
    /// of the old `_pad`. Hoisting the invariant tail out of the loop and only
    /// replacing `content` puts it back.
    #[inline(always)]
    pub fn with_ascii(self, b: StreamByte) -> PackedCell {
        PackedCell {
            content: Content::codepoint(Codepoint::from_ascii(b)),
            ..self
        }
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
}

/// Position within the scrollback ring. Wraps by construction.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RingPos(usize);

impl RingPos {
    pub const ZERO: RingPos = RingPos(0);
    pub fn advance(self, modulus: usize) -> RingPos {
        RingPos((self.0 + 1) % modulus)
    }
    pub fn plus(self, r: Row, modulus: usize) -> RingPos {
        RingPos((self.0 + r.index()) % modulus)
    }
    pub fn back(self, n: usize, modulus: usize) -> RingPos {
        RingPos((self.0 + modulus - n % modulus) % modulus)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

/// Write offset into the arena, in bytes, wrapping at capacity.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ArenaTail(usize);

impl ArenaTail {
    pub const ZERO: ArenaTail = ArenaTail(0);
    /// Wraps when a full slot would not fit, so a read never straddles the end.
    pub fn next_slot(self, capacity: usize) -> ArenaTail {
        if self.0 + SLOT_BYTES > capacity {
            ArenaTail(0)
        } else {
            self
        }
    }
    pub fn advance(self, n: ByteLen) -> ArenaTail {
        ArenaTail(self.0 + n.index())
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn pos(self) -> BytePos {
        BytePos::from_index(self.0)
    }
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
    pub fn base(&self) -> BytePos {
        self.base
    }
    pub const fn capacity() -> ByteLen {
        ByteLen::new(SLOT_BYTES as u16)
    }
    /// Copy from a typed byte stream.
    ///
    /// Indexed rather than iterated for the same reason as `put_ascii_run`: this
    /// runs once per `read(2)` on the ingest path, so the loop's codegen is a
    /// measured quantity and not clippy's to choose.
    #[allow(clippy::needless_range_loop)]
    pub fn write(&mut self, src: &[StreamByte]) -> Filled {
        let n = src.len().min(SLOT_BYTES);
        for i in 0..n {
            self.buf[i] = src[i].raw();
        }
        Filled::from_read(n)
    }
    /// The raw handle a syscall needs. Named, and the only one.
    pub fn as_syscall_buf(&mut self) -> (*mut u8, usize) {
        (self.buf.as_mut_ptr(), SLOT_BYTES)
    }
}

impl StreamByte {
    pub(crate) fn raw(self) -> u8 {
        self.0
    }
    /// For matching against ASCII literals in escape dispatch.
    pub fn raw_byte(self) -> u8 {
        self.0
    }
    /// Reinterpret a byte slice as a stream. No copy and no `unsafe`:
    /// `repr(transparent)` plus zerocopy's `FromBytes` make this a checked
    /// no-op. This is the crate's one entry point for wire data.
    pub fn slice_from(b: &[u8]) -> &[StreamByte] {
        <[StreamByte]>::ref_from_bytes(b).expect("transparent over u8, always valid")
    }
    pub fn slice_as_bytes(s: &[StreamByte]) -> &[u8] {
        s.as_bytes()
    }
}

/// How many continuation bytes a UTF-8 sequence still needs. 1, 2 or 3 by
/// construction — never an arbitrary `u8` counter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pending(u8);

impl Pending {
    pub fn new(n: u8) -> Pending {
        Pending(n.min(3))
    }
    pub fn is_last(self) -> bool {
        self.0 == 1
    }
    pub fn dec(self) -> Pending {
        Pending(self.0.saturating_sub(1))
    }
}

/// A Unicode scalar value: `0..=0x10FFFF`, surrogates excluded.
///
/// Not a `u32`. `Width::of` and `Content::codepoint` both took one and could be
/// handed a lane mask, a byte count, or a packed colour with equal enthusiasm.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
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
    pub fn decoded(v: u32) -> Codepoint {
        Codepoint::new(v).unwrap_or(Codepoint::REPLACEMENT)
    }
    pub fn from_ascii(b: StreamByte) -> Codepoint {
        Codepoint(b.raw() as u32)
    }
    pub fn value(self) -> u32 {
        self.0
    }
}

/// One colour slot: default, a palette index, or 24-bit RGB.
///
/// A tagged `u32` rather than an enum so the whole rendition stays a plain
/// zeroable block -- `Storage` is `new_box_zeroed`, and all-zero must be a valid
/// default rather than a colour that happens to look like one.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct ColorWord(u32);

impl ColorWord {
    const TAG_INDEXED: u32 = 0x0100_0000;
    const TAG_RGB: u32 = 0x0200_0000;
    /// Zero, so a zeroed `Storage` starts with default colours.
    pub const DEFAULT: ColorWord = ColorWord(0);
    pub fn indexed(n: Chan) -> ColorWord {
        ColorWord(Self::TAG_INDEXED | n.get() as u32)
    }
    pub fn rgb(r: Chan, g: Chan, b: Chan) -> ColorWord {
        ColorWord(
            Self::TAG_RGB | ((r.get() as u32) << 16) | ((g.get() as u32) << 8) | b.get() as u32,
        )
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn is_default(self) -> bool {
        self.0 == 0
    }
    /// The RGB triple, when this word holds one. `None` for the default colour
    /// and for palette indices, neither of which names a pixel value on their
    /// own.
    pub fn as_rgb(self) -> Option<(u8, u8, u8)> {
        (self.0 & Self::TAG_RGB != 0).then_some((
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
            self.0 as u8,
        ))
    }
    /// The parsed SGR colour, packed.
    pub fn from_color(c: Color) -> ColorWord {
        match c {
            Color::Default => ColorWord::DEFAULT,
            Color::Indexed(n) => ColorWord::indexed(n),
            Color::Rgb(r, g, b) => ColorWord::rgb(r, g, b),
        }
    }
}

/// A packed rendition: foreground, background, and attribute bits, as SGR
/// accumulates it. Interned into a [`StyleId`]; never stored per cell.
///
/// This used to be a bare `u32` holding only the foreground, with the result
/// that `SgrAttr::fold` had nowhere to put bold, underline, reverse or any
/// background colour -- so it dropped them. They were parsed and discarded,
/// which is the failure `docs/vt-coverage.md` calls `parsed`, at the scale of
/// every attribute a terminal has.
#[repr(C)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct StyleWord {
    fg: ColorWord,
    bg: ColorWord,
    attrs: Attrs,
    /// Explicit, because zerocopy rejects implicit padding.
    ///
    /// DECSCA protection deliberately does **not** live here.
    /// `[[source: vt510-decsca]]`: "DECSCA does not effect visual character
    /// attributes set by the select graphic rendition (SGR) function" -- it is
    /// not a rendition, so it is not in the rendition. It is a [`CellFlags`]
    /// bit on the cell instead, which also keeps it out of the interning key:
    /// protected and unprotected text of the same colour are one style, not
    /// two, and the 64-entry template cache is not halved by turning DECSCA on.
    _pad: u16,
}

impl StyleWord {
    /// All zero, so `Storage::new_box_zeroed` starts every template slot at the
    /// default rendition rather than at an arbitrary grey.
    pub const DEFAULT: StyleWord = StyleWord {
        fg: ColorWord::DEFAULT,
        bg: ColorWord::DEFAULT,
        attrs: Attrs::NONE,
        _pad: 0,
    };
    pub fn fg(self) -> ColorWord {
        self.fg
    }
    pub fn bg(self) -> ColorWord {
        self.bg
    }
    pub fn attrs(self) -> Attrs {
        self.attrs
    }
    pub fn with_fg(self, c: ColorWord) -> StyleWord {
        StyleWord { fg: c, ..self }
    }
    pub fn with_bg(self, c: ColorWord) -> StyleWord {
        StyleWord { bg: c, ..self }
    }
    pub fn with_attrs(self, a: Attrs) -> StyleWord {
        StyleWord { attrs: a, ..self }
    }
    /// Everything that distinguishes one rendition from another, folded into
    /// one integer for hashing.
    pub fn key(self) -> u64 {
        (self.fg.0 as u64) ^ ((self.bg.0 as u64) << 21) ^ ((self.attrs.bits() as u64) << 45)
    }
}

/// Index of a line slot in the scrollback ring. Distinct from [`LineIdx`],
/// which is a stable logical line number: this one wraps.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LineSlot(usize);

impl LineSlot {
    pub fn of(pos: RingPos) -> LineSlot {
        LineSlot(pos.index())
    }
    pub fn index(self) -> usize {
        self.0
    }
    pub fn run(self, r: RunCount, maxrun: usize) -> usize {
        self.0 * maxrun + r.index()
    }
}

/// A monotonic event counter. Not a `u64`: a statistic cannot be added to a
/// length, a byte position or a column, and nothing can be assigned into it
/// except by counting.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct Stat(u64);

impl Stat {
    pub const ZERO: Stat = Stat(0);
    pub fn inc(&mut self) {
        self.0 += 1;
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

/// Codepoint bits accumulated mid-sequence, *before* they are known to be a
/// valid scalar value. Deliberately not a [`Codepoint`] — a partial sequence
/// has not earned that type yet, and calling it one would be a lie the width
/// tables would then believe.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Utf8Acc(u32);

impl Utf8Acc {
    pub fn lead(bits: u32) -> Utf8Acc {
        Utf8Acc(bits)
    }
    pub fn push(self, cont: u32) -> Utf8Acc {
        Utf8Acc((self.0 << 6) | cont)
    }
    /// The only exit. Malformed input becomes U+FFFD rather than an invalid
    /// codepoint, so the invariant holds without the caller branching.
    pub fn finish(self) -> Codepoint {
        Codepoint::decoded(self.0)
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

impl Run {
    /// Set the encoding from the typed enum rather than casting at the call
    /// site. The field is `u8` because a fieldless enum cannot be `FromBytes`
    /// — a byte off disk is not necessarily a valid variant — so recovery goes
    /// through the checked [`Run::encoding`].
    pub fn with_encoding(mut self, e: Encoding) -> Run {
        self.encoding = e as u8;
        self
    }
    pub fn with_flags(mut self, f: CellFlags) -> Run {
        self.flags = f.bits();
        self
    }
}

impl ByteLen {
    /// Offset of a SIMD match within a scanned window.
    pub fn from_lane(n: usize) -> ByteLen {
        ByteLen::from_index(n)
    }
}
