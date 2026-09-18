use crate::*;

mod reflow;
pub use reflow::Reflowed;

/// A grid indexed by [`Row`] and [`Col`], never by an integer.
///
/// This is what removes `as_index` from call sites: you cannot index it with a
/// `usize`, and you cannot index it with a `Row` where a `Col` belongs. The
/// conversion happens once, inside `Index`, instead of everywhere.
pub struct Grid<'buf, T> {
    cells: &'buf mut [T],
    stride: Stride,
    rows: Rows,
}

impl<'buf, T> Grid<'buf, T> {
    pub fn new(cells: &'buf mut [T], stride: Stride, rows: Rows) -> Grid<'buf, T> {
        Grid {
            cells,
            stride,
            rows,
        }
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }
    pub fn stride(&self) -> Stride {
        self.stride
    }
}

impl<'buf, T> core::ops::Index<Row> for Grid<'buf, T> {
    type Output = [T];
    fn index(&self, r: Row) -> &[T] {
        let a = r.0 * self.stride.index();
        &self.cells[a..a + self.stride.index()]
    }
}

impl<'buf, T> core::ops::IndexMut<Row> for Grid<'buf, T> {
    fn index_mut(&mut self, r: Row) -> &mut [T] {
        let a = r.0 * self.stride.index();
        &mut self.cells[a..a + self.stride.index()]
    }
}

/// A single cell, addressed by both axes. `grid[(row, col)]`.
impl<'buf, T> core::ops::Index<(Row, Col)> for Grid<'buf, T> {
    type Output = T;
    fn index(&self, (r, c): (Row, Col)) -> &T {
        &self.cells[r.0 * self.stride.index() + c.0]
    }
}

impl<'buf, T> core::ops::IndexMut<(Row, Col)> for Grid<'buf, T> {
    fn index_mut(&mut self, (r, c): (Row, Col)) -> &mut T {
        &mut self.cells[r.0 * self.stride.index() + c.0]
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
    stride: Stride,
    cols: Cols,
    rows: Rows,
    ring: Rows,
    rot: RowRot,
}

impl<'a> ViewRef<'a> {
    fn phys(&self, r: Row) -> usize {
        let ring = self.ring.index();
        let y = r.index() + self.rot.index();
        if y >= ring { y - ring } else { y }
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

    /// The cell a [`LeadCol`] names -- the read-side counterpart of
    /// [`View::cell_at`].
    ///
    /// It exists so a reader can go through `lead` without the field being
    /// public. `LeadCol` has one constructor on purpose: making its inner
    /// column reachable would put "one column back" and "the cell that owns
    /// this position" back into the same type, which is the confusion it was
    /// introduced to end.
    pub fn cell_at(&self, r: Row, c: LeadCol) -> PackedCell {
        self.get(r, c.0)
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
    stride: Stride,
    cols: Cols,
    rows: Rows,
    /// **Physical rows in the buffer -- the modulus of the rotation.** The
    /// screen is `rows` of them, a window that advances one row a scroll;
    /// the rows behind the window are the lines that scrolled off, still
    /// where they were written, and the window moving back over them is
    /// scrollback. A buffer with no more rows than the screen is the ring
    /// this used to be, where the row that leaves the top is the row that
    /// comes in at the bottom.
    ring: Rows,
    rot: RowRot,
}

/// The screen, read where it is.
///
/// **The same pointers as the ring, in the shape its readers use.** A row is
/// a slice of the ring at physical row `(y + rot) % ring`, and an index
/// `y * stride + x` -- the way a screen laid out at the pitch is addressed
/// -- resolves to the same cell, so a reader that walks rows and a reader
/// that indexes both read the grid in place and nothing is turned or
/// copied. The one thing the ring cannot offer is a single slice across a
/// row boundary where the window wraps its end; [`Viewport::contiguous`]
/// says when it can, and [`Viewport::parts`] is the two slices otherwise.
#[derive(Clone, Copy)]
pub struct Viewport<'a> {
    cells: &'a [PackedCell],
    used: &'a [RowExtent],
    stride: Stride,
    cols: Cols,
    rows: Rows,
    ring: Rows,
    rot: RowRot,
}

impl<'a> Viewport<'a> {
    pub fn new(
        cells: &'a [PackedCell],
        used: &'a [RowExtent],
        stride: Stride,
        cols: Cols,
        rows: Rows,
        ring: Rows,
        rot: RowRot,
    ) -> Viewport<'a> {
        Viewport {
            cells,
            used,
            stride,
            cols,
            rows,
            ring,
            rot,
        }
    }

    fn phys(&self, y: usize) -> usize {
        let ring = self.ring.index();
        let p = y + self.rot.index();
        if p >= ring { p - ring } else { p }
    }

    /// The same view `n` rows further back into the ring: the window over
    /// the lines that scrolled off, its top `n` lines above the screen's.
    /// The rotation retreats and nothing else changes, so a scrolled-back
    /// screen is read the way the live one is -- in place, at the pitch,
    /// through the rotation -- and costs a frame what the live one costs. A
    /// caller bounds `n` by what is retained; the ring holds whatever is
    /// behind the window past that, which is stale.
    pub fn back(&self, n: usize) -> Viewport<'a> {
        Viewport {
            rot: self.rot.retreat(n, self.ring),
            ..*self
        }
    }

    /// Screen row `y`'s `cols` live cells.
    pub fn row(&self, y: Row) -> &'a [PackedCell] {
        let a = self.phys(y.index()) * self.stride.index();
        &self.cells[a..a + self.cols.index()]
    }

    /// Whether screen row `y` continues onto the next: how its text ended,
    /// which the cells do not say and a reflow or a selection needs.
    pub fn wrapped(&self, y: Row) -> Wrap {
        match self.used[self.phys(y.index())].wrapped() {
            true => Wrap::Continues,
            false => Wrap::Ended,
        }
    }

    /// Screen row `y` at the full pitch, which is what an index at the pitch
    /// addresses.
    fn pitch_row(&self, y: usize) -> &'a [PackedCell] {
        assert!(y < self.rows.index(), "row {y} of {}", self.rows.index());
        let a = self.phys(y) * self.stride.index();
        &self.cells[a..a + self.stride.index()]
    }

    pub fn rows(&self) -> Rows {
        self.rows
    }
    pub fn cols(&self) -> Cols {
        self.cols
    }
    pub fn stride(&self) -> Stride {
        self.stride
    }
    /// The rotation this view reads under -- what a shader that reads the
    /// ring in place takes, `(row + rotation) % ring`.
    pub fn rotation(&self) -> RowRot {
        self.rot
    }
    pub fn ring(&self) -> Rows {
        self.ring
    }
    /// Cells at the pitch, `rows * stride`: the extent an index can address.
    pub fn len(&self) -> usize {
        self.rows.index() * self.stride.index()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The screen as one slice at the pitch, when the window does not wrap
    /// around the ring's end -- which it does not for all but `rows` of
    /// every `ring` positions.
    pub fn contiguous(&self) -> Option<&'a [PackedCell]> {
        let (a, n, s) = (self.rot.index(), self.rows.index(), self.stride.index());
        match a + n <= self.ring.index() {
            true => Some(&self.cells[a * s..(a + n) * s]),
            false => None,
        }
    }

    /// The screen as two slices at the pitch, the rows before the ring's end
    /// and the rows after it, the second empty when the window does not
    /// wrap. Together they are the screen in order, and neither is a copy.
    pub fn parts(&self) -> (&'a [PackedCell], &'a [PackedCell]) {
        let (a, n, s, ring) = (
            self.rot.index(),
            self.rows.index(),
            self.stride.index(),
            self.ring.index(),
        );
        let head = n.min(ring - a);
        (
            &self.cells[a * s..(a + head) * s],
            &self.cells[..(n - head) * s],
        )
    }

    /// Every cell in screen order at the pitch, the rows chained.
    pub fn iter(&self) -> impl Iterator<Item = PackedCell> + 'a {
        let v = *self;
        (0..self.rows.index()).flat_map(move |y| v.pitch_row(y).iter().copied())
    }
}

/// `vp[y * stride + x]`: the cell at screen row `y`, column `x`, read in
/// place. Any column of the pitch, not only the live ones, so a reader can
/// see what lies past the edge.
impl core::ops::Index<usize> for Viewport<'_> {
    type Output = PackedCell;
    fn index(&self, i: usize) -> &PackedCell {
        let s = self.stride.index();
        &self.pitch_row(i / s)[i % s]
    }
}

/// A range at the pitch, which must lie within one row: a row is one slice
/// of the ring and the next row is elsewhere in it. A range that crosses a
/// row asks for the copy this type exists not to make, and panics.
impl core::ops::Index<core::ops::Range<usize>> for Viewport<'_> {
    type Output = [PackedCell];
    fn index(&self, r: core::ops::Range<usize>) -> &[PackedCell] {
        let s = self.stride.index();
        let (y, a) = (r.start / s, r.start % s);
        assert!(
            r.end <= (y + 1) * s,
            "{}..{} crosses row {y} of the pitch {s}",
            r.start,
            r.end
        );
        &self.pitch_row(y)[a..r.end - y * s]
    }
}

impl core::ops::Index<core::ops::RangeTo<usize>> for Viewport<'_> {
    type Output = [PackedCell];
    fn index(&self, r: core::ops::RangeTo<usize>) -> &[PackedCell] {
        &self[0..r.end]
    }
}

/// From a cell to the end of its row at the pitch.
impl core::ops::Index<core::ops::RangeFrom<usize>> for Viewport<'_> {
    type Output = [PackedCell];
    fn index(&self, r: core::ops::RangeFrom<usize>) -> &[PackedCell] {
        let s = self.stride.index();
        &self[r.start..(r.start / s + 1) * s]
    }
}

/// The surface a [`View`] is not addressing: the other screen's cells, marks,
/// depth and rotation, held so the two can be swapped without a copy when
/// `?1049` switches screens.
pub struct Surface<'a> {
    pub cells: &'a mut [PackedCell],
    pub used: &'a mut [RowExtent],
    pub ring: Rows,
    pub rot: RowRot,
}

impl<'a> View<'a> {
    /// Read-only view. Same addressing, no mutation.
    pub fn new_ref(
        cells: &'a [PackedCell],
        stride: Stride,
        cols: Cols,
        rows: Rows,
        ring: Rows,
        rot: RowRot,
    ) -> ViewRef<'a> {
        ViewRef {
            cells,
            stride,
            cols,
            rows,
            ring,
            rot,
        }
    }
    pub fn new(
        cells: &'a mut [PackedCell],
        used: &'a mut [RowExtent],
        stride: Stride,
        cols: Cols,
        rows: Rows,
        ring: Rows,
        rot: RowRot,
    ) -> View<'a> {
        View {
            cells,
            used,
            stride,
            cols,
            rows,
            ring,
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

    /// The rotation this view holds.
    pub fn rot(&self) -> RowRot {
        self.rot
    }

    /// A fill from `a` to the live edge on a row whose mark lies past it:
    /// the fill runs to the mark instead. `fill_raw`'s cold half.
    #[cold]
    #[inline(never)]
    fn fill_past_edge(&mut self, p: usize, a: usize, cell: PackedCell) -> usize {
        let base = p * self.stride.index();
        let to = self.used[p].index();
        self.used[p] = self.mark_after_fill(base, to, cell, RowExtent::of(a));
        if a >= to {
            return 0;
        }
        self.cells[base + a..base + to].fill(cell);
        to - a
    }

    /// The mark after a fill of `cell` that ran to `to`, where `low` is the
    /// mark the fill would leave on a row it covered to the live edge and no
    /// further. Past the live edge the fill may have reached into content the
    /// screen is too narrow to show, and past *that* lies whatever blank the
    /// row was last cleared to; if that blank is not this one, the row is not
    /// uniform from `low` on and the mark has to say so.
    fn mark_after_fill(
        &self,
        base: usize,
        to: usize,
        cell: PackedCell,
        low: RowExtent,
    ) -> RowExtent {
        let (cols, s) = (self.cols.index(), self.stride.index());
        match to <= cols || to >= s || self.cells[base + to] == cell {
            true => low,
            false => RowExtent::of(to),
        }
    }

    /// Row `r` continues onto the next: its text reached the last column and
    /// wrapped. Set by the writer at the wrap, on the row being left.
    ///
    /// Out of line, and so is `set_ended`: both are once-a-line events, and
    /// inlined into the writers they grew the per-byte body by enough to
    /// cost btop 7% on a recording that wraps nothing -- the body's size is
    /// paid by every payload whether the call runs or not.
    #[cold]
    #[inline(never)]
    pub fn set_wrapped(&mut self, r: Row) {
        let p = self.phys(r);
        self.used[p] = self.used[p].wrapping(self.cols.index());
    }

    /// Row `r` ends where it stops: a newline left it. Clears the wrap.
    #[cold]
    #[inline(never)]
    pub fn set_ended(&mut self, r: Row) {
        let p = self.phys(r);
        self.used[p] = self.used[p].ended();
    }

    /// Exchange this view's surface with `other`: the cells, marks, depth
    /// and rotation, leaving the geometry. What `?1049` does now -- the
    /// alternate screen is a second surface rather than a place to park the
    /// primary, so entering and leaving it copies nothing.
    pub fn swap_surface(&mut self, other: &mut Surface<'a>) {
        core::mem::swap(&mut self.cells, &mut other.cells);
        core::mem::swap(&mut self.used, &mut other.used);
        core::mem::swap(&mut self.ring, &mut other.ring);
        core::mem::swap(&mut self.rot, &mut other.rot);
    }

    /// The rotation this view is under: screen row `y` is physical row
    /// `(y + rotation) % ring`. Live across a read -- a scroll advances it.
    pub fn rotation(&self) -> RowRot {
        self.rot
    }
    /// The ring's depth: physical rows in the buffer.
    pub fn ring(&self) -> Rows {
        self.ring
    }

    /// The screen's rows as one slice in screen order, when the window does
    /// not wrap around the ring's end; a caller copies through
    /// [`View::window_into`] otherwise. Most of the time it does not: the
    /// window wraps for `rows` of every `ring` positions.
    pub fn window(&self) -> Option<&[PackedCell]> {
        let (a, n, s) = (self.rot.index(), self.rows.index(), self.stride.index());
        match a + n <= self.ring.index() {
            true => Some(&self.cells[a * s..(a + n) * s]),
            false => None,
        }
    }

    /// The screen's rows into `dst` in screen order, a row at a time, for
    /// the window that wraps.
    pub fn window_into(&self, dst: &mut [PackedCell]) {
        let s = self.stride.index();
        for y in 0..self.rows.index() {
            let a = self.phys(Row::from_index(y)) * s;
            dst[y * s..(y + 1) * s].copy_from_slice(&self.cells[a..a + s]);
        }
    }

    /// After the screen widened, blank the columns the alternate surface just
    /// revealed -- `[old_cols, cols)` in every visible row. The alternate
    /// screen has no scrollback and the application owns it: a cell past the
    /// old edge holds whatever was there when the screen was last this wide,
    /// and a full-screen program redraws a frame or two *after* the resize, so
    /// showing that stale content under the incoming draw is the "two images
    /// at once" a grow used to flash. Blanking the revealed columns leaves the
    /// overlap -- which the program redraws -- and clears the new region,
    /// which is what a resize should show. `fill` sets each row's mark to the
    /// old edge as it goes, which is what the earlier mark-only version did
    /// for the fill shortcut; the difference is that the cells are now blank
    /// rather than merely marked stale. This is the width half of a grow; the
    /// primary reflows instead, so it is never revealed stale.
    pub fn widened(&mut self, old_cols: Cols) {
        let (from, to) = (
            Col::at_index(old_cols.index()),
            Col::at_index(self.cols.index()),
        );
        for y in 0..self.rows.index() {
            self.fill(Row::from_index(y), from, to, PackedCell::EMPTY);
        }
    }

    /// The row `ahead` rows past the window's end -- `ahead == 0` is the row
    /// the next scroll brings in, `1` the one after -- as its `cols` live
    /// cells, or nothing when the ring has no such row past the screen. What
    /// a caller warms before the blank that will land on it: with the grid as
    /// the ring, a scroll writes into a row nothing touched for `ring`
    /// scrolls, so its lines are exactly the ones nothing has in cache.
    pub fn incoming(&self, ahead: usize) -> &[PackedCell] {
        if self.ring.index() <= self.rows.index() + ahead {
            return &[];
        }
        let p = self
            .rot
            .advance(self.rows.index() + ahead, self.ring)
            .index();
        let base = p * self.stride.index();
        &self.cells[base..base + self.cols.index()]
    }

    /// Advance the rotation by `n` rows: a scroll, from the view's side.
    ///
    /// The view holds the rotation by value, so a caller that keeps one across
    /// a whole read -- which is the point, since building it per run costs
    /// more than the writes do -- has to be able to scroll without dropping
    /// it. Returns the new rotation so the caller can keep its own copy in
    /// step.
    pub fn advance_rot(&mut self, n: usize) -> RowRot {
        self.rot = self.rot.advance(n, self.ring);
        self.rot
    }

    /// Retreat the rotation by `n` rows: a reverse scroll, from the view's
    /// side. The counterpart of [`View::advance_rot`], and it exists for the
    /// same reason: a scroll that moves an index costs one row of blanking
    /// where one that moves the rows costs the whole screen.
    pub fn retreat_rot(&mut self, n: usize) -> RowRot {
        self.rot = self.rot.retreat(n, self.ring);
        self.rot
    }

    /// Put the rotation back to identity.
    ///
    /// **For entering the alternate screen**, whose buffer is about to be
    /// cleared: a rotation left over from the last visit describes rows that
    /// are about to stop existing.
    pub fn reset_rot(&mut self) {
        self.rot = RowRot::NONE;
    }

    /// Copy screen row `from` over screen row `to`, extent included.
    ///
    /// **For repairing the rows a rotation moved that should not have moved.**
    /// A scrolling region that does not span the screen cannot be a whole-grid
    /// rotation -- but it can be one *plus* putting the few rows outside the
    /// region back, and for the usual shape (a full-height region with a
    /// status line under it) that is one row instead of forty-nine.
    pub fn copy_row(&mut self, from: Row, to: Row) {
        let (a, b) = (self.phys(from), self.phys(to));
        if a == b {
            return;
        }
        let w = self.cols.index();
        let stride = self.stride.index();
        let (lo, hi) = match a < b {
            true => (a, b),
            false => (b, a),
        };
        let (head, tail) = self.cells.split_at_mut(hi * stride);
        let (src, dst) = match a < b {
            true => (&head[lo * stride..lo * stride + w], &mut tail[..w]),
            false => (&tail[..w], &mut head[lo * stride..lo * stride + w]),
        };
        dst.copy_from_slice(src);
        self.used[b] = self.used[a];
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
        let ring = self.ring.index();
        let y = r.index() + self.rot.index();
        if y >= ring { y - ring } else { y }
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
        // `ceiling-no-store` keeps every setup -- row resolution, orphan
        // repair, extent, clip -- and skips only the stores, so a pair with the
        // default build separates writing the cells from deciding where they
        // go. On `cat-scroll.raw` that split is **28% stores, 21% setup**.
        #[cfg(feature = "ceiling-no-store")]
        {
            let _ = (dst, src, proto);
            return;
        }
        #[cfg(all(target_endian = "little", not(feature = "ceiling-no-store")))]
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
        #[cfg(all(not(target_endian = "little"), not(feature = "ceiling-no-store")))]
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

    /// The same, for a caller that already knows the row is uniform from `c`
    /// onward -- so there is no wide half to orphan and no probe to make.
    ///
    /// **The probe is two loads and two compares a character**, and the caller
    /// that can answer without them is the one holding the cursor: a write
    /// moving left to right past a row's used extent can only be landing on
    /// blanks, and a blank is not a `WIDE_TAIL`. That is the `RowExtent`
    /// invariant, and the caller keeps the current row's mark in a register
    /// rather than re-loading it from the parallel array -- which is what made
    /// the same idea lose when it was tried one layer down.
    #[inline(always)]
    pub fn put_glyph_fresh(&mut self, r: Row, c: Col, lead: PackedCell, tail: Option<PackedCell>) {
        let cols = self.cols.index();
        let x = c.index();
        if r.index() >= self.rows.index() || x >= cols {
            return;
        }
        let p = self.phys(r);
        let base = p * self.stride.index();
        #[cfg(feature = "ceiling-no-store")]
        let _ = (lead, base);
        // **Two stores, and combining them lost.** A lead and its `WIDE_TAIL`
        // are sixteen contiguous bytes, so one wide store should serve --
        // `copy_from_slice(&[lead, t])` measured **662-690 against 690-709**,
        // because the pair is built on the stack first and that costs more
        // than the store it saves. This is the dominant write on CJK text
        // (69% of runs are one character, so the vectored run writer never
        // sees them), which is why it was worth trying and worth recording.
        #[cfg(not(feature = "ceiling-no-store"))]
        {
            self.cells[base + x] = lead;
        }
        let end = match tail {
            Some(_t) if x + 1 < cols => {
                #[cfg(not(feature = "ceiling-no-store"))]
                {
                    self.cells[base + x + 1] = _t;
                }
                x + 2
            }
            _ => x + 1,
        };
        self.used[p] = self.used[p].reached(end);
        #[cfg(feature = "probe-double-extent")]
        {
            self.used[p] = self.used[p].reached(end);
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
    /// otherwise widen. core2's no-DECSCA property found that at
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

    /// Which cell owns the character at `c`. Mirrors [`ViewRef::lead`], which
    /// is the definition; a `View` is a `ViewRef` that can also write, and a
    /// caller holding one should not have to give up the write to ask.
    pub fn lead(&self, r: Row, c: Col) -> LeadCol {
        match c.index() > 0 && self.get(r, c).content.is_wide_tail() {
            true => LeadCol(c.before()),
            false => LeadCol(c),
        }
    }

    /// The cell at `c`, for a caller that has to read its cluster before
    /// extending it.
    pub fn cell_at(&self, r: Row, c: LeadCol) -> PackedCell {
        match r.index() < self.rows.index() && c.col().index() < self.cols.index() {
            true => self.cells[self.phys(r) * self.stride.index() + c.col().index()],
            false => PackedCell::EMPTY,
        }
    }

    /// Point a cell at an interned cluster.
    ///
    /// Raises `RowExtent` for the same reason [`View::set_marked`] does, and
    /// that reason is written out there: a cell carrying continuations is not
    /// equal to the blank in front of it, so a row left non-uniform beyond its
    /// own mark has a later erase skip it.
    pub fn set_cluster(&mut self, r: Row, c: LeadCol, id: ClusterId) {
        if r.index() >= self.rows.index() || c.col().index() >= self.cols.index() {
            return;
        }
        let p = self.phys(r);
        let i = p * self.stride.index() + c.col().index();
        self.cells[i].flags = self.cells[i].flags.with_cluster(id);
        self.used[p] = self.used[p].reached(c.col().index() + 1);
    }

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
        let p = self.phys(r);
        let i = p * self.stride.index() + c.col().index();
        self.cells[i].flags = self.cells[i].flags.with(CellFlags::MARKED);
        // **And the extent has to move with it.** `RowExtent` records where the
        // row stops being *uniform*, not where the glyphs are, and a cell
        // carrying `MARKED` is not equal to the blank in front of it. Setting
        // the bit without raising the mark left the row non-uniform beyond its
        // own extent, so a later erase skipped the cell: `CSI ? 1 J` cleared a
        // mark that `CSI 1 J` left alone, which
        // `selective_erase_matches_plain_erase_when_nothing_is_protected`
        // reports.
        //
        // This is the same defect `ECH` at the last column already had, and
        // that this file records: an operation that makes a cell differ from
        // the blank must say so.
        // **Only when it would actually raise.** The base character was just
        // written to this same cell and already moved the extent past it, so
        // the mark is inside it in almost every case -- and an unconditional
        // store dirties a line in a hot array to write back the value that was
        // already there.
        let to = c.col().index() + 1;
        if self.used[p].index() < to {
            self.used[p] = self.used[p].reached(to);
        }
    }

    /// The boundary rule itself, against a row base the caller already has.
    ///
    /// Split out so `clear_partners` -- which runs once per non-ASCII
    /// codepoint -- computes `phys` **once**. Going through `snap` cost three
    /// per call there (`snap`'s, `at`'s and `reached`'s), and it profiled at
    /// 7% of the CJK path for two loads and two compares of actual work.
    ///
    /// No `#[inline(always)]`: it carried one, stranded for a long time onto
    /// `lead` by an insertion between this comment and the signature, and
    /// re-attaching it measured cat-cjk 0.98x, disjoint from an A/A copy.
    /// LLVM inlines this private function on its own judgement, and the
    /// forced form is the one that costs.
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
        // is a real bug that core2's cleared-screen property caught: `ECH`
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
        // **Through the mark, when the mark lies past the live edge.** A fill
        // to the end of the row is the row being cleared, and a row written
        // wider than the screen is now keeps that content past the edge until
        // the screen widens again -- so the clear has to reach it, or a line
        // the application erased comes back with a tail. Rare -- it needs a
        // resize -- so it is a cold call, and the body below is the one that
        // was measured.
        if to_end && self.used[p].index() > cols {
            return self.fill_past_edge(p, a, cell);
        }
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
        // Through the mark when it lies past the live edge: see `fill_raw`.
        if self.used[p].index() > cols {
            let low = match blanking {
                true => RowExtent::ZERO,
                false => RowExtent::of(cols),
            };
            return RowExtent::of(self.fill_past_edge_row(p, cell, low));
        }
        let to = match blanking && self.cells[base + cols - 1] == cell {
            true => self.used[p].index().min(cols),
            false => cols,
        };
        // **Ceiling arm: the blank itself, and only the blank.** The ring
        // still advances, the extent bookkeeping still happens, and the
        // returned extent is the one the real fill would report -- so both
        // arms attribute identical cells and only the stores differ. The older
        // `ceiling-no-blank` reported a *full row* here, so its counters
        // disagreed with the real arm and it could not show the effect it
        // existed to measure.
        #[cfg(not(feature = "ceiling-no-rowfill"))]
        self.cells[base..base + to].fill(cell);
        self.used[p] = match blanking {
            true => RowExtent::ZERO,
            false => RowExtent::of(cols),
        };
        RowExtent::of(to)
    }

    /// Whether a screen row holds nothing within the live width: every
    /// cell's content is [`Content::EMPTY`]. Content, not the mark: a row
    /// written wide and then narrowed keeps a mark past the live edge,
    /// and the mark a blanking fill leaves there is about what lies past
    /// the edge, not about whether the row shows anything. The question
    /// a retreat asks -- may this row leave the screen -- is about what
    /// it shows.
    pub fn row_is_blank(&self, r: Row) -> bool {
        r.index() < self.rows.index()
            && self
                .row_slice(r)
                .iter()
                .all(|c| c.content == Content::EMPTY)
    }

    /// Blank the row `back` lines behind the window -- a retained line --
    /// to its mark, and mark it empty. The caller says the row is retained
    /// (`back` is below the history's length); this addresses it the way
    /// `Scrollback::line` does, `rot - 1 - back` modulo the ring.
    ///
    /// For the prompt block a redraw replaces when it was taller than the
    /// screen: its rows above the window are the newest retained lines,
    /// and they are the shell's stale prompt, not content.
    pub fn fill_behind(&mut self, back: crate::Back, cell: PackedCell) -> RowExtent {
        self.fill_at(self.rot.retreat(back.index() + 1, self.ring), cell)
    }

    /// Blank one physical row of the ring -- wherever the window is -- to
    /// its mark, and mark it empty. `at` is the ring row as [`RowRot`]
    /// spells it: `phys(y)` for a screen row is `rot.advance(y, ring)`,
    /// and a row behind the window is `rot.retreat(1 + back, ring)`.
    ///
    /// For a block of rows the terminal knows the ring extent of and must
    /// erase whether they are on the screen, behind it or, after the
    /// window moved, hidden past it.
    pub fn fill_at(&mut self, at: RowRot, cell: PackedCell) -> RowExtent {
        let p = at.index();
        let base = p * self.stride.index();
        let to = self.used[p].index();
        #[cfg(not(feature = "ceiling-no-rowfill"))]
        self.cells[base..base + to].fill(cell);
        self.used[p] = RowExtent::ZERO;
        RowExtent::of(to)
    }

    /// A whole-row fill on a row whose mark lies past the live edge: the
    /// fill runs to the mark, and the mark after it is `low` unless the
    /// blank beyond is a different one. `fill_row`'s cold half.
    #[cold]
    #[inline(never)]
    fn fill_past_edge_row(&mut self, p: usize, cell: PackedCell, low: RowExtent) -> usize {
        let base = p * self.stride.index();
        let to = self.used[p].index();
        #[cfg(not(feature = "ceiling-no-rowfill"))]
        self.cells[base..base + to].fill(cell);
        self.used[p] = self.mark_after_fill(base, to, cell, low);
        to
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
