use crate::*;

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

/// Read-only counterpart of [`View`].
/// Rows the grid is rotated by: screen row `y` lives at physical row
/// `(y + rot) % rows`.
///
/// **This is what makes a full-screen scroll O(1).** Scrolling used to advance
/// `head`, which indexes the run table, and leave the cells alone -- so
/// `Computed` and `Templated` never scrolled. Copying the grid instead was
/// correct and cost 45% of cell ingest, because a streaming payload scrolls
/// every screenful: write amplification around 20:1 against the rule that it
/// stays at or below 1. Rotating an index costs neither.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct RowRot(pub(crate) usize);

impl RowRot {
    pub const NONE: RowRot = RowRot(0);
    pub fn advance(self, n: usize, rows: Rows) -> RowRot {
        RowRot((self.0 + n) % rows.index().max(1))
    }
    /// The other direction, for a screen that scrolls **down**: RI at the top
    /// of the region, SD, IL.
    ///
    /// Spelled as a forward advance by `rows - n` rather than a subtraction,
    /// because `self.0` is unsigned and `rot - n` underflows to a vast number
    /// that `%` then turns into a plausible row. `n` is reduced first so
    /// `rows - n` cannot itself be `rows`.
    ///
    /// `%` here and a conditional subtract in `View::phys`: this runs once per
    /// scroll, `phys` runs once per addressed row, and the modulo was worth
    /// ~4% there.
    pub fn retreat(self, n: usize, rows: Rows) -> RowRot {
        let r = rows.index().max(1);
        RowRot((self.0 + (r - n % r)) % r)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

pub struct ViewRef<'a> {
    cells: &'a [PackedCell],
    stride: Cols,
    cols: Cols,
    rows: Rows,
    rot: RowRot,
}

impl<'a> ViewRef<'a> {
    fn phys(&self, r: Row) -> usize {
        let rows = self.rows.index();
        let y = r.index() + self.rot.index();
        if y >= rows { y - rows } else { y }
    }
    pub fn row_slice(&self, r: Row) -> &'a [PackedCell] {
        let a = self.phys(r) * self.stride.index();
        &self.cells[a..a + self.cols.index()]
    }
    pub fn get(&self, r: Row, c: Col) -> PackedCell {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            self.cells[self.phys(r) * self.stride.index() + c.index()]
        } else {
            PackedCell::EMPTY
        }
    }

    /// Which cell owns the character at `c`.
    ///
    /// `c` itself, unless `c` names a double-width character's tail, in which
    /// case the character lives one column back. **This is the only way to
    /// build a [`LeadCol`]**, because the grid is the only thing that knows
    /// which columns hold tails -- the same reason `snap_at` widens a fill to
    /// whole characters rather than trusting the caller's bounds.
    pub fn lead(&self, r: Row, c: Col) -> LeadCol {
        match c.index() > 0 && self.get(r, c).content.is_wide_tail() {
            true => LeadCol(c.before()),
            false => LeadCol(c),
        }
    }
}

/// A viewport laid out over a caller-owned cell array, addressed by axis.
///
/// The 26 occurrences of `y * MAX_COLS + x` this replaces were 26 chances to
/// transpose the axes or forget the clamp. Here the arithmetic happens once.
pub struct View<'a> {
    cells: &'a mut [PackedCell],
    /// How far glyphs have reached in each **physical** row. See [`RowExtent`].
    ///
    /// Carried here rather than maintained by the caller so that the two
    /// cannot drift: every mutator on this type bumps it, and the line moves
    /// permute it in the same loop that permutes the rows. A caller holding the
    /// marks separately would have to remember both, which is the shape of
    /// invariant this crate keeps moving *into* `View` rather than out of it.
    used: &'a mut [RowExtent],
    stride: Cols,
    cols: Cols,
    rows: Rows,
    rot: RowRot,
}

impl<'a> View<'a> {
    /// Read-only view. Same addressing, no mutation.
    pub fn new_ref(
        cells: &'a [PackedCell],
        stride: Cols,
        cols: Cols,
        rows: Rows,
        rot: RowRot,
    ) -> ViewRef<'a> {
        ViewRef {
            cells,
            stride,
            cols,
            rows,
            rot,
        }
    }
    pub fn new(
        cells: &'a mut [PackedCell],
        used: &'a mut [RowExtent],
        stride: Cols,
        cols: Cols,
        rows: Rows,
        rot: RowRot,
    ) -> View<'a> {
        View {
            cells,
            used,
            stride,
            cols,
            rows,
            rot,
        }
    }

    /// Record that a write reached column `to` (exclusive) on screen row `r`.
    ///
    /// Monotone: `RowExtent::reached` takes a max, so a caller that
    /// over-reports costs a few stores and one that under-reports would leave
    /// stale text. Every mutator below calls this, and
    /// `tests/row_extent.rs` fails the build if one stops.
    #[inline(always)]
    fn reached(&mut self, r: Row, to: usize) {
        let p = self.phys(r);
        self.used[p] = self.used[p].reached(to);
    }

    /// The mark for screen row `r`.
    pub fn extent(&self, r: Row) -> RowExtent {
        self.used[self.phys(r)]
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

    /// Screen row to physical offset, through the rotation. Every addressing
    /// path goes here, which is what makes the rotation one line rather than
    /// twenty-six chances to forget it.
    #[inline(always)]
    pub(crate) fn phys(&self, r: Row) -> usize {
        // A conditional subtract, not `%`. Both operands are below `rows`, so
        // the sum is below `2 * rows` and one subtraction is enough. The
        // modulo cost ~16% of the cells path -- it sits on the addressing path
        // that `put_ascii_run` reaches once per run and `put` once per cell.
        let rows = self.rows.index();
        let y = r.index() + self.rot.index();
        if y >= rows { y - rows } else { y }
    }

    #[inline(always)]
    pub(crate) fn at(&self, r: Row, c: Col) -> usize {
        self.phys(r) * self.stride.index() + c.index()
    }

    #[inline(always)]
    pub fn put(&mut self, r: Row, c: Col, cell: PackedCell) {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            let p = self.phys(r);
            self.cells[p * self.stride.index() + c.index()] = cell;
            self.used[p] = self.used[p].reached(c.index() + 1);
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
    pub fn put_ascii_run(
        &mut self,
        r: Row,
        at: Col,
        src: &[u8],
        style: StyleId,
        flags: CellFlags,
        set: Charset,
    ) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let start = at.index();
        let n = src.len().min(self.cols.index().saturating_sub(start));
        // **Clear whatever this run is about to orphan**, inline against the
        // `base` already computed rather than through `Grid::clear_partners`.
        // Calling that from `blit` needed a second `self.view()` per run and
        // measured **-6 to -9%** on every payload; here it is two loads and two
        // compares against a pointer that already exists, per ~32-byte run.
        //
        // A run starting on a tail orphans the lead in front of it; a run
        // ending on a lead orphans the tail after it. ASCII never reaches
        // `put_cp`, so without this the common path keeps the bug.
        //
        // The blank is built **inside** the guard. Constructing it
        // unconditionally cost measurably more than the two loads did: a run
        // that cuts nothing -- which is every run in ASCII text -- should pay
        // for the test and not for the repair.
        if n > 0 {
            let end = start + n;
            let cut_lead = start > 0 && self.cells[base + start].content.is_wide_tail();
            let cut_tail = end < self.cols.index() && self.cells[base + end].content.is_wide_tail();
            if cut_lead || cut_tail {
                let blank = PackedCell::empty_with(style);
                if cut_lead {
                    self.cells[base + start - 1] = blank;
                }
                if cut_tail {
                    self.cells[base + end] = blank;
                }
            }
        }
        self.used[p] = self.used[p].reached(start + n);
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
        // The charset is tested *first*. `flags == NONE` was, and a national
        // set sets no flag -- only the line-drawing set has one -- so UK and
        // German text took the ASCII fast path and was never translated. The
        // graphics set worked only because `refresh_flags` gives it a flag.
        if set != Charset::Ascii {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags,
            };
            for (d, &b) in self.cells[base + start..base + start + n]
                .iter_mut()
                .zip(&src[..n])
            {
                *d = proto.with_charset(StreamByte::from_stream(b), set);
            }
        } else if flags == CellFlags::NONE {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags: CellFlags::NONE,
            };
            Self::put_ascii_vectored(
                &mut self.cells[base + start..base + start + n],
                &src[..n],
                proto,
            );
        } else {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags,
            };
            for (d, &b) in self.cells[base + start..base + start + n]
                .iter_mut()
                .zip(&src[..n])
            {
                *d = proto.with_ascii(StreamByte::from_stream(b));
            }
        }
    }

    /// Write one cell per source byte, eight at a time.
    ///
    /// **Why this is not `slice::fill` and not a plain loop.** `fill` writes one
    /// repeated value and lowers to a vector store; this writes a *different*
    /// content per cell, so it was a scalar loop -- `otool -tv` on the release
    /// binary showed `ldr`/`str`/`orr` and **no vector register at all**. On the
    /// streaming payload that loop writes 27% of the cells and takes 33% of the
    /// cells-path profile, where `fill` writes the other 73% in 17%: about 5.3x
    /// the cost per cell for the same eight bytes.
    ///
    /// Only the content differs between cells in a run -- style and flags are
    /// fixed -- so under `repr(C)` on a little-endian target a cell is
    /// `(style, flags) << 32 | content`, and eight of them are one `u64x8` OR and
    /// one 64-byte store.
    ///
    /// No `unsafe`: `baud-domain` forbids it, so the reinterpretation goes through
    /// `zerocopy`, which `PackedCell` already derives for the shm and disk paths.
    /// Write a run of double-width characters: a glyph and its tail per
    /// codepoint, **eight cells a store**.
    ///
    /// The wide counterpart of `put_ascii_run`, and the asymmetry it closes is
    /// the one `[[source: simdutf]]` measures inside its own library: 18
    /// gigacharacters a second on Latin against 1.7 on Chinese, because every
    /// fast implementation has an ASCII fast path and a per-character path for
    /// everything else. baud had exactly that shape -- eight cells a store for
    /// ASCII, two individual stores per codepoint for CJK.
    ///
    /// Both cells of a pair carry the same style and flags, so the high half of
    /// every `PackedCell` is one constant and only the content differs: the
    /// codepoint, then `Content::WIDE_TAIL`. Interleaving those is a swizzle.
    ///
    /// Returns cells written, which is twice the codepoints taken.
    pub fn put_wide_run(
        &mut self,
        r: Row,
        at: Col,
        cps: &[Codepoint],
        style: StyleId,
        flags: CellFlags,
    ) -> RowExtent {
        let cols = self.cols.index();
        let x = at.index();
        if r.index() >= self.rows.index() || x >= cols {
            return RowExtent::ZERO;
        }
        // Two cells per codepoint, and never past the last column.
        let n = cps.len().min((cols - x) / 2);
        if n == 0 {
            return RowExtent::ZERO;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let end = x + 2 * n;
        // The orphan rule, once for the whole run rather than once per
        // character: only its two edges can cut a pair.
        let blank = PackedCell::empty_with(style);
        let (sa, sb) = self.snap_at(base, x, end);
        if sa < x {
            self.cells[base + sa] = blank;
        }
        if sb > end {
            self.cells[base + sb - 1] = blank;
        }
        let proto = PackedCell::wide_tail(style, flags);
        Self::put_wide_vectored(&mut self.cells[base + x..base + end], &cps[..n], proto);
        self.used[p] = self.used[p].reached(sb.max(end));
        RowExtent::of(2 * n)
    }

    /// A run of ordinary **single**-column non-ASCII characters, one cell each.
    ///
    /// The counterpart of `put_ascii_run` for text that is multi-byte but one
    /// column: Latin-1, Greek, Cyrillic, box drawing, and the CJK punctuation
    /// East Asian Width calls Ambiguous. `opcost` priced that shape at
    /// **266 MB/s against a wide character's 1155** -- half the cells for five
    /// times the cost, an inversion whose only available mechanism was that it
    /// had no bulk path and paid `put_cp` per character.
    ///
    /// Simpler than `put_wide_run`: one cell per codepoint means no interleave
    /// and no pair to keep together, so only the two edges of the run can cut
    /// an existing wide pair.
    pub fn put_narrow_run(
        &mut self,
        r: Row,
        at: Col,
        cps: &[Codepoint],
        style: StyleId,
        flags: CellFlags,
    ) -> RowExtent {
        let cols = self.cols.index();
        let x = at.index();
        if r.index() >= self.rows.index() || x >= cols {
            return RowExtent::ZERO;
        }
        let n = cps.len().min(cols - x);
        if n == 0 {
            return RowExtent::ZERO;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let end = x + n;
        let blank = PackedCell::empty_with(style);
        let (sa, sb) = self.snap_at(base, x, end);
        if sa < x {
            self.cells[base + sa] = blank;
        }
        if sb > end {
            self.cells[base + sb - 1] = blank;
        }
        let proto = PackedCell::glyph(Codepoint::REPLACEMENT, style, flags);
        Self::put_narrow_vectored(&mut self.cells[base + x..base + end], &cps[..n], proto);
        self.used[p] = self.used[p].reached(sb.max(end));
        RowExtent::of(n)
    }

    /// Eight cells a store. `dst` is exactly `cps.len()` cells.
    fn put_narrow_vectored(dst: &mut [PackedCell], cps: &[Codepoint], proto: PackedCell) {
        #[cfg(target_endian = "little")]
        {
            use core::simd::Simd;
            use core::simd::num::SimdUint;
            use zerocopy::{FromBytes, IntoBytes};

            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(proto.as_bytes());
            // The high half is style and flags, shared by every cell; the low
            // half is `proto`'s own codepoint and is replaced per cell.
            let hi = u64::from_ne_bytes(bytes) & 0xffff_ffff_0000_0000;

            let mut d = dst.chunks_exact_mut(8);
            let mut s = cps.chunks_exact(8);
            for (dc, sc) in d.by_ref().zip(s.by_ref()) {
                let c = Simd::<u32, 8>::from_array([
                    sc[0].value(),
                    sc[1].value(),
                    sc[2].value(),
                    sc[3].value(),
                    sc[4].value(),
                    sc[5].value(),
                    sc[6].value(),
                    sc[7].value(),
                ])
                .cast::<u64>();
                let cells = (c | Simd::splat(hi)).to_array();
                let packed = <[PackedCell; 8]>::read_from_bytes(cells.as_bytes())
                    .expect("eight u64 is sixty-four bytes, which is eight cells");
                dc.copy_from_slice(&packed);
            }
            for (dc, &cp) in d.into_remainder().iter_mut().zip(s.remainder()) {
                *dc = PackedCell::glyph(cp, proto.style, proto.flags);
            }
        }
        #[cfg(not(target_endian = "little"))]
        for (dc, &cp) in dst.iter_mut().zip(cps) {
            *dc = PackedCell::glyph(cp, proto.style, proto.flags);
        }
    }

    /// The interleave. `dst` is exactly `2 * cps.len()` cells.
    fn put_wide_vectored(dst: &mut [PackedCell], cps: &[Codepoint], proto: PackedCell) {
        #[cfg(target_endian = "little")]
        {
            use core::simd::num::SimdUint;
            use core::simd::{Simd, simd_swizzle};
            use zerocopy::{FromBytes, IntoBytes};

            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(proto.as_bytes());
            let word = u64::from_ne_bytes(bytes);
            // Style and flags, shared by both cells of every pair.
            let hi = word & 0xffff_ffff_0000_0000;
            // `proto` is already the tail, so its low half is `WIDE_TAIL`.
            let tail = Simd::<u64, 4>::splat(word & 0x0000_0000_ffff_ffff);

            let mut d = dst.chunks_exact_mut(8);
            let mut s = cps.chunks_exact(4);
            for (dc, sc) in d.by_ref().zip(s.by_ref()) {
                let c = Simd::<u32, 4>::from_array([
                    sc[0].value(),
                    sc[1].value(),
                    sc[2].value(),
                    sc[3].value(),
                ])
                .cast::<u64>();
                // [c0, tail, c1, tail, c2, tail, c3, tail]
                let content: Simd<u64, 8> = simd_swizzle!(c, tail, [0, 4, 1, 5, 2, 6, 3, 7]);
                let cells = (content | Simd::splat(hi)).to_array();
                let packed = <[PackedCell; 8]>::read_from_bytes(cells.as_bytes())
                    .expect("eight u64 is sixty-four bytes, which is eight cells");
                dc.copy_from_slice(&packed);
            }
            for (dc, &cp) in d.into_remainder().chunks_exact_mut(2).zip(s.remainder()) {
                dc[0] = PackedCell::glyph(cp, proto.style, proto.flags);
                dc[1] = proto;
            }
        }
        #[cfg(not(target_endian = "little"))]
        for (dc, &cp) in dst.chunks_exact_mut(2).zip(cps) {
            dc[0] = PackedCell::glyph(cp, proto.style, proto.flags);
            dc[1] = proto;
        }
    }

    fn put_ascii_vectored(dst: &mut [PackedCell], src: &[u8], proto: PackedCell) {
        #[cfg(target_endian = "little")]
        {
            use core::simd::Simd;
            use core::simd::num::SimdUint;
            // `PackedCell` already derives these for the shm and disk paths;
            // here they are what makes the reinterpretation possible without
            // `unsafe`, which this crate forbids.
            use zerocopy::{FromBytes, IntoBytes};

            // The high half is style and flags; masking the low half off discards
            // `Content::EMPTY`, which is a reserved value rather than zero.
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(proto.as_bytes());
            let hi = u64::from_ne_bytes(bytes) & 0xffff_ffff_0000_0000;

            let mut d = dst.chunks_exact_mut(8);
            let mut s = src.chunks_exact(8);
            for (dc, sc) in d.by_ref().zip(s.by_ref()) {
                let content = Simd::<u8, 8>::from_slice(sc).cast::<u64>();
                let cells = (content | Simd::splat(hi)).to_array();
                let packed = <[PackedCell; 8]>::read_from_bytes(cells.as_bytes())
                    .expect("eight u64 is sixty-four bytes, which is eight cells");
                dc.copy_from_slice(&packed);
            }
            // A run averages ~32 bytes, so the tail is not rare and is not free.
            for (dc, &b) in d.into_remainder().iter_mut().zip(s.remainder()) {
                *dc = proto.with_ascii(StreamByte::from_stream(b));
            }
        }
        #[cfg(not(target_endian = "little"))]
        for (dc, &b) in dst.iter_mut().zip(src) {
            *dc = proto.with_ascii(StreamByte::from_stream(b));
        }
    }

    /// Widen a half-open column span so it covers **whole characters**.
    ///
    /// A double-width character is stored as a lead cell and a
    /// `Content::WIDE_TAIL`. An operation whose boundary falls between the two
    /// leaves an orphan -- a lead claiming two columns that owns one, or a tail
    /// with nothing in front of it -- and a renderer has no coherent way to
    /// draw either. `[[source: ghostty-screen]]`'s `eraseChars` states the rule
    /// baud was missing: "if our last cell is a wide char then we need to also
    /// clear the cell beyond it since **we can't just split a wide char**", and
    /// its `deleteChars` does the same.
    ///
    /// Both ends, because both were wrong. A start sitting **on** a tail
    /// belongs to the lead before it; an end whose first excluded cell **is** a
    /// tail is excluding the partner of a lead it included.
    ///
    /// One place rather than at each operation, for the reason this crate
    /// already applies to `phys`: the range operations are `fill`,
    /// `delete_chars`, `insert_blanks`, `delete_cells` and `insert_cells`, and
    /// five call sites each remembering a two-line invariant is five chances to
    /// forget it.
    /// Blank the orphaned half of any character this write is about to cut.
    ///
    /// The write itself cannot be widened the way an erase can -- the incoming
    /// text has its own length -- so the partner is cleared instead. A write
    /// starting on a tail orphans the lead in front of it; a write ending on a
    /// lead orphans the tail after it. Writing a *wide* character onto a tail
    /// does both at once.
    ///
    /// Expressed through [`Grid::snap`] so there is one definition of where a
    /// character boundary is: whatever `snap` would have added is exactly what
    /// must be blanked.
    /// `#[inline(always)]`: this runs once per non-ASCII codepoint, and left
    /// as a real call it profiled at **26%** of pure-Han throughput -- a
    /// ceiling arm removing it entirely measured 270 -> 339 MB/s. Two loads and
    /// two compares do not cost that; a function call on a four-nanosecond path
    /// does.
    #[inline(always)]
    pub fn clear_partners(&mut self, r: Row, from: Col, to: Col, blank: PackedCell) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let cols = self.cols.index();
        let (a, b) = (from.index().min(cols), to.index().min(cols));
        let (sa, sb) = self.snap_at(base, from.index(), to.index());
        if sa < a {
            self.cells[base + sa] = blank;
        }
        if sb > b {
            self.cells[base + sb - 1] = blank;
            // **Past the mark, so the mark must move.** Everything beyond the
            // extent has to stay equal to the blank last written across the
            // row, or the O(1) "has the template changed" test that reads the
            // *last* cell stops implying anything about the cells before it.
            self.used[p] = self.used[p].reached(sb);
        }
    }

    /// Write one glyph, and a wide character's tail, in **one** pass.
    ///
    /// This was `clear_partners`, then `put`, then `put` again: three `phys`
    /// computations, two bounds checks and three read-modify-writes of the
    /// used extent, for a character that occupies two cells. It runs once per
    /// **non-ASCII codepoint** -- ASCII goes through `put_ascii_run` -- so it
    /// is most of what the CJK path is made of, and `cat-cjk.raw` is where baud
    /// is furthest behind another implementation.
    ///
    /// The orphan repair is the same rule as everywhere else, against the base
    /// already computed: a write starting on a tail orphans the lead in front
    /// of it, and one ending on a lead orphans the tail after it.
    #[inline(always)]
    pub fn put_glyph(
        &mut self,
        r: Row,
        c: Col,
        lead: PackedCell,
        tail: Option<PackedCell>,
        blank: PackedCell,
    ) {
        let cols = self.cols.index();
        let x = c.index();
        if r.index() >= self.rows.index() || x >= cols {
            return;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let end = match tail {
            Some(_) => (x + 2).min(cols),
            None => x + 1,
        };
        let (sa, sb) = self.snap_at(base, x, end);
        if sa < x {
            self.cells[base + sa] = blank;
        }
        if sb > end {
            self.cells[base + sb - 1] = blank;
        }
        self.cells[base + x] = lead;
        if let Some(t) = tail {
            if x + 1 < cols {
                self.cells[base + x + 1] = t;
            }
        }
        // One extent update covering the glyph and whatever the repair touched.
        self.used[p] = self.used[p].reached(sb.max(end));
    }

    /// [`Grid::snap`] for callers outside this type, in `Col`s.
    ///
    /// `Term::selective_wipe` needs it: DECSEL walks the span cell by cell
    /// testing each for protection, so it never reaches `fill` and would not
    /// otherwise widen. `without_decsca_selective_equals_plain` found that at
    /// 20,000 cases the moment plain erase started snapping and selective did
    /// not -- the two must agree when nothing is protected, and a property that
    /// compares two implementations catches exactly the divergence a
    /// hand-written test would have had to predict.
    pub fn snap_span(&self, r: Row, from: Col, to: Col) -> (Col, Col) {
        let (a, b) = self.snap(r, from, to);
        (Col::at_index(a), Col::at_index(b))
    }

    fn snap(&self, r: Row, from: Col, to: Col) -> (usize, usize) {
        let base = self.at(r, Col::ZERO);
        self.snap_at(base, from.index(), to.index())
    }

    /// The boundary rule itself, against a row base the caller already has.
    ///
    /// Split out so `clear_partners` -- which runs once per non-ASCII
    /// codepoint -- computes `phys` **once**. Going through `snap` cost three
    /// per call there (`snap`'s, `at`'s and `reached`'s), and it profiled at
    /// 7% of the CJK path for two loads and two compares of actual work.
    #[inline(always)]
    /// Record that a cell carries entries in the marks table.
    ///
    /// The inverse needs no code: every write builds a fresh [`PackedCell`]
    /// from the terminal's current flags, which never carry
    /// [`CellFlags::MARKED`], so overwriting a cell un-records its marks. That
    /// is the whole point of keeping the bit here rather than in the table --
    /// a table keyed on cell identity cannot see a cell rewritten in place.
    pub fn set_marked(&mut self, r: Row, c: LeadCol) {
        if r.index() >= self.rows.index() || c.col().index() >= self.cols.index() {
            return;
        }
        let i = self.phys(r) * self.stride.index() + c.col().index();
        self.cells[i].flags = self.cells[i].flags.with(CellFlags::MARKED);
    }

    fn snap_at(&self, base: usize, a: usize, b: usize) -> (usize, usize) {
        let cols = self.cols.index();
        let (mut a, mut b) = (a.min(cols), b.min(cols));
        if a >= b {
            return (a, b);
        }
        if a > 0 && self.cells[base + a].content.is_wide_tail() {
            a -= 1;
        }
        if b < cols && self.cells[base + b].content.is_wide_tail() {
            b += 1;
        }
        (a, b)
    }

    /// Fill a half-open column span of one row. Clamped to the visible width,
    /// and widened to whole characters by [`Grid::snap`].
    /// Returns how many cells it wrote, which is not what it was asked for: a
    /// fill running to the end of the row stops at the used extent. Counting
    /// the request instead is the "instrumentation outlived what fed it"
    /// failure, and it hid this optimisation's effect entirely on the first
    /// measurement -- the counters came out identical across a change that
    /// halved the work.
    pub fn fill(&mut self, r: Row, from: Col, to: Col, cell: PackedCell) -> usize {
        if r.index() >= self.rows.index() {
            return 0;
        }
        let (a, b) = self.snap(r, from, to);
        self.fill_raw(r, a, b, cell)
    }

    /// The unsnapped fill, for spans already known to be whole rows.
    fn fill_raw(&mut self, r: Row, a: usize, b: usize, cell: PackedCell) -> usize {
        if r.index() >= self.rows.index() || a >= b {
            return 0;
        }
        // **The one place the mark can go down.** Everything else only ever
        // raises it, which is what makes `max` a safe maintenance rule; a fill
        // is the operation that makes a row *emptier*, so it needs the other
        // direction or `CSI 2 K` would leave the mark at the full width and
        // blanking would never get cheaper.
        //
        // **The mark is not "where glyphs are". It is "where the row stops
        // being uniform".** Those coincide most of the time and the difference
        // is a real bug that `a_cleared_screen_is_fully_erasable` caught: `ECH`
        // at the last column writes a *styled* blank there, which holds no
        // glyph but does not match the default blanks in front of it. Lowering
        // the mark to zero on that basis left the row non-uniform behind its
        // own mark, and the next full clear skipped it.
        //
        // So a fill that runs to the end of the row sets the mark to exactly
        // `a`: everything from `a` on is now this one cell, and nothing weaker
        // can be assumed about what is in front of it. That is a *lowering*
        // when the fill starts early (`CSI 2 K` gives zero) and a *raising*
        // when it starts late, and both are needed.
        //
        // **`phys` once, for the base and the mark together.** The first
        // version called it three times here -- `extent`, `set_extent` and
        // `at` -- and this is the path carrying 73-85% of all cell writes. It
        // measured -7 to -8% for bookkeeping that changes no output.
        let p = self.phys(r);
        let cols = self.cols.index();
        let base = p * self.stride.index();
        let to_end = cell.content == Content::EMPTY && b >= cols;
        // **A fill that runs to the end of the row can stop at the mark**, on
        // exactly the condition `fill_row` uses: everything from the extent to
        // the end is already the blank last written across the row, so if that
        // is the blank going in now, writing it again changes nothing.
        //
        // `EL` and `ED` to end of line land here, and they are what an editor
        // redraws with -- `opcost` prices `EL to end` at **26.6 ns** against a
        // `CUP` at 1.2, almost all of it clearing 140 columns that a 60-column
        // line never used.
        let b = match to_end && self.cells[base + cols - 1] == cell {
            true => self.used[p].index().max(a).min(b),
            false => b,
        };
        match to_end {
            true => self.used[p] = RowExtent::of(a),
            false => self.used[p] = self.used[p].reached(b),
        }
        if a >= b {
            return 0;
        }
        // `slice::fill`, not an indexed loop: one bounds check instead of one
        // per cell, and it lowers to a vector store rather than a scalar one.
        self.cells[base + a..base + b].fill(cell);
        b - a
    }

    /// **Deliberately not through `fill`.** A whole row cannot split a
    /// character -- column 0 is never a tail and there is no cell past the last
    /// -- so the snap could only ever be two compares that never fire, on the
    /// path that carries 73-85% of all cell writes. Blanking pays nothing for
    /// an invariant it cannot violate.
    /// **Stops at the used extent when it soundly can.** This is what the mark
    /// is for.
    ///
    /// Two conditions, both O(1), and the second is the one that makes this
    /// work where scan-before-blank did not:
    ///
    /// * the incoming cell is a **blank**, so the row is being emptied rather
    ///   than filled (DECALN comes through here with an `E`);
    /// * the **last** cell of the row already equals that blank, so the
    ///   rendition in force has not changed since the row was last cleared
    ///   across its full width -- and therefore every cell from the mark to the
    ///   end is already exactly this cell.
    ///
    /// The second is `[[source: alacritty-row]]`'s test, comparing one cell
    /// against the template rather than scanning for a difference. baud tried
    /// the scanning form twice and it lost both times, at -24% and -66%,
    /// because the blank carries the current style: on any payload that emits
    /// SGR the scan runs to the end of every row and saves nothing. One load
    /// cannot have that failure.
    ///
    /// Measured saving, counted rather than argued: **72-82%** of all blanking
    /// on every real scrolling recording, and 16% on `payload::streaming`,
    /// whose SGR every 232 bytes keeps the template changing.
    /// Returns how many cells it actually wrote, so the counters can report
    /// the work done rather than the work asked for. An instrument that keeps
    /// reporting the full width after the write got shorter is the
    /// "instrumentation outlived what fed it" failure, and it would hide
    /// exactly the effect this change exists to produce.
    /// The return is a **column count**, which is what `RowExtent` is -- not a
    /// bare `usize`, because the audit is right that "how many cells" and "how
    /// far along a row" are the same quantity here and a raw integer invites
    /// adding it to something it is not.
    #[inline(always)]
    pub fn fill_row(&mut self, r: Row, cell: PackedCell) -> RowExtent {
        let cols = self.cols.index();
        if cols == 0 || r.index() >= self.rows.index() {
            return RowExtent::ZERO;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        let blanking = cell.content == Content::EMPTY;
        let to = match blanking && self.cells[base + cols - 1] == cell {
            true => self.used[p].index().min(cols),
            false => cols,
        };
        self.cells[base..base + to].fill(cell);
        self.used[p] = match blanking {
            true => RowExtent::ZERO,
            false => RowExtent::of(cols),
        };
        RowExtent::of(to)
    }
    /// Shift a row's cells left by `n`, blanking the tail. `CSI P`.
    pub fn delete_chars(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        // A shift rewrites to the end of the row, so the mark goes there too.
        // Tighter bookkeeping is possible -- a left shift can only shrink the
        // used extent -- but shrinking a high-water mark is the direction that
        // loses text, and this operation is rare next to blanking.
        self.reached(r, cols);
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        // The deleted span is what must cover whole characters: shifting from
        // the middle of a pair drags a lone tail leftwards into a cell with no
        // lead in front of it.
        let (cx, n) = {
            let (a, b) = self.snap(r, Col::at_index(cx), Col::at_index(cx + n));
            (a, b - a)
        };
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
        // A shift rewrites to the end of the row, so the mark goes there too.
        // Tighter bookkeeping is possible -- a left shift can only shrink the
        // used extent -- but shrinking a high-water mark is the direction that
        // loses text, and this operation is rare next to blanking.
        self.reached(r, cols);
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        // Inserting *at* a tail would put blanks between a lead and its
        // partner. `[[source: ghostty-screen]]`'s `insertBlanks` erases the
        // character in that case; snapping the start back to the lead moves the
        // whole pair instead, which keeps the text rather than deleting it and
        // leaves no orphan either way.
        let cx = self.snap(r, Col::at_index(cx), Col::at_index(cx + 1)).0;
        let n = n.min(cols - cx);
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
    /// Shift left within `[at, right)`, filling at the right edge. `CSI P` with
    /// a right margin, and the engine of DECDC.
    ///
    /// The bounded twin of [`Grid::delete_chars`], for the same reason
    /// [`Grid::delete_lines_in`] is the bounded twin of the row version: with
    /// DECSLRM set, a delete must not pull text in from beyond the right
    /// margin, because that text is outside the scrolling region and is not
    /// supposed to move.
    pub fn delete_cells(&mut self, r: Row, at: Col, n: Count, right: Col, blank: PackedCell) {
        let end = right.index().min(self.cols.index());
        let cx = at.index();
        if cx >= end {
            return;
        }
        // Rewrites everything from the cursor to the margin. Same reasoning as
        // `delete_chars`: raise to the far edge rather than compute a tighter
        // bound, because the tighter bound is the one that can be too small.
        self.reached(r, end);
        let n = (n.get_raw() as usize).min(end - cx);
        // **The deleted span, not the surviving one.** Both boundaries can
        // split a character: a start on a tail orphans the lead in front of it,
        // and an end on a lead drags that lead's tail leftwards to sit against
        // something else. `snap` widens the span so neither happens, which is
        // `[[source: ghostty-screen]]`'s `deleteChars` rule -- "we can't just
        // split a wide char".
        let (cx, n) = {
            let (a, b) = self.snap(r, Col::at_index(cx), Col::at_index(cx + n));
            (a, (b - a).min(end - a))
        };
        let base = self.at(r, Col::ZERO);
        for x in cx..end - n {
            self.cells[base + x] = self.cells[base + x + n];
        }
        for x in end - n..end {
            self.cells[base + x] = blank;
        }
    }

    /// Shift right within `[at, right)`, inserting blanks. `CSI @` with a right
    /// margin, and the engine of DECIC.
    pub fn insert_cells(&mut self, r: Row, at: Col, n: Count, right: Col, blank: PackedCell) {
        let end = right.index().min(self.cols.index());
        let cx = at.index();
        if cx >= end {
            return;
        }
        // Rewrites everything from the cursor to the margin. Same reasoning as
        // `delete_chars`: raise to the far edge rather than compute a tighter
        // bound, because the tighter bound is the one that can be too small.
        self.reached(r, end);
        // Inserting *at* a tail would put blanks between a lead and its
        // partner. Snapping the start back to the lead moves the whole pair
        // instead, which keeps the character rather than deleting it and
        // leaves no orphan either way.
        let cx = self.snap(r, Col::at_index(cx), Col::at_index(cx + 1)).0;
        let n = (n.get_raw() as usize).min(end - cx);
        let base = self.at(r, Col::ZERO);
        // **The character shifted off the right edge takes its partner with
        // it.** The cell at `end - n` is pushed past `end` and lost; if it is a
        // tail, its lead lands at `end - 1` with nothing after it.
        // `[[source: ghostty-screen]]`'s `insertBlanks` clears that cell for
        // the same reason. Read before the shift, because the shift overwrites
        // the evidence.
        let orphans_at_edge = end > n && n > 0 && self.cells[base + end - n].content.is_wide_tail();
        let mut x = end;
        while x > cx + n {
            x -= 1;
            self.cells[base + x] = self.cells[base + x - n];
        }
        for x in cx..cx + n {
            self.cells[base + x] = blank;
        }
        if orphans_at_edge {
            self.cells[base + end - 1] = blank;
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
    /// Move a block of lines down, then blank what it vacated.
    ///
    /// **Row at a time, and explicitly.** This was a scalar `for x in 0..cols`
    /// loop with `phys()` recomputed inside it, twice per cell, which LLVM
    /// happened to lower to `memmove` -- the 13.3% `_platform_memmove` in
    /// `less-pager.raw`'s profile. Fast by luck of optimisation, and the
    /// sibling `delete_lines_in` exists in its explicit form precisely because
    /// that luck ran out once: "a scalar loop over 10,000 cells is what made
    /// the correct full-screen scroll cost a third of cell ingest".
    ///
    /// `copy_within` is the obvious spelling and measured **2% slower** on
    /// `less-pager.raw`, 27.0/27.3/27.2 against 27.8/27.7/27.9. It has to
    /// promise overlap-safety and gets a `memmove` for it. Two distinct rows
    /// never overlap, so `split_at_mut` plus `copy_from_slice` says that and
    /// gets a `memcpy`: 27.0/27.0/26.5/26.9 against the loop's
    /// 26.9/27.6/27.0/27.0, which is even.
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
            let src = self.phys(Row::from_index(y - n)) * s;
            let dst = self.phys(Row::from_index(y)) * s;
            // `split_at_mut` and `copy_from_slice`, not `copy_within`. Two
            // distinct rows never overlap, and saying so gets a `memcpy`;
            // `copy_within` must promise overlap-safety and pays for a
            // `memmove` -- measured 27.0/27.3/27.2 MB/s against 27.8/27.7/27.9
            // on `less-pager.raw`, slower in all three pairs.
            //
            // **The rotation is why the order is tested.** `phys` maps screen
            // rows onto physical ones, so `y - n` can sit *after* `y` in the
            // buffer, and a split taken on the wrong side panics.
            match src < dst {
                true => {
                    let (lo, hi) = self.cells.split_at_mut(dst);
                    hi[..cols].copy_from_slice(&lo[src..src + cols]);
                }
                false => {
                    let (lo, hi) = self.cells.split_at_mut(src);
                    lo[dst..dst + cols].copy_from_slice(&hi[..cols]);
                }
            }
            // **The mark travels with the row**, in the same loop and under
            // the same `phys`. Kept here rather than in a second pass because a
            // second pass over the same indices is a second chance to compute
            // them differently -- and a mark left behind on a moved row is
            // stale text, which is the failure this whole mechanism must not
            // have.
            let (ps, pd) = (
                self.phys(Row::from_index(y - n)),
                self.phys(Row::from_index(y)),
            );
            self.used[pd] = self.used[ps];
        }
        for y in cy..cy + n {
            let p = self.phys(Row::from_index(y));
            let a = p * s;
            self.cells[a..a + cols].fill(blank);
            self.used[p] = match blank.content == Content::EMPTY {
                true => RowExtent::ZERO,
                false => RowExtent::of(cols),
            };
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
        // `copy_within` per row, not a cell at a time. Rows are strided, so the
        // block is not contiguous and one memmove cannot do it -- but each row
        // is, and a scalar loop over 10,000 cells is what made the correct
        // full-screen scroll cost a third of cell ingest.
        for y in cy..bot + 1 - n {
            let (ps, pd) = (
                self.phys(Row::from_index(y + n)),
                self.phys(Row::from_index(y)),
            );
            self.cells.copy_within(ps * s..ps * s + cols, pd * s);
            self.used[pd] = self.used[ps];
        }
        for y in bot + 1 - n..=bot {
            let p = self.phys(Row::from_index(y));
            let a = p * s;
            self.cells[a..a + cols].fill(blank);
            self.used[p] = match blank.content == Content::EMPTY {
                true => RowExtent::ZERO,
                false => RowExtent::of(cols),
            };
        }
    }
    pub fn row_slice(&self, r: Row) -> &[PackedCell] {
        let a = self.at(r, Col::ZERO);
        &self.cells[a..a + self.cols.index()]
    }
}
