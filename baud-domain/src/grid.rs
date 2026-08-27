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
        stride: Cols,
        cols: Cols,
        rows: Rows,
        rot: RowRot,
    ) -> View<'a> {
        View {
            cells,
            stride,
            cols,
            rows,
            rot,
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
            for z in 0..n {
                self.cells[base + start + z] =
                    proto.with_charset(StreamByte::from_stream(src[z]), set);
            }
        } else if flags == CellFlags::NONE {
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
        let n = (n.get_raw() as usize).min(end - cx);
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
        let n = (n.get_raw() as usize).min(end - cx);
        let base = self.at(r, Col::ZERO);
        let mut x = end;
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
                self.cells[self.phys(Row::from_index(y)) * s + x] =
                    self.cells[self.phys(Row::from_index(y - n)) * s + x];
            }
        }
        for y in cy..cy + n {
            for x in 0..cols {
                self.cells[self.phys(Row::from_index(y)) * s + x] = blank;
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
        // `copy_within` per row, not a cell at a time. Rows are strided, so the
        // block is not contiguous and one memmove cannot do it -- but each row
        // is, and a scalar loop over 10,000 cells is what made the correct
        // full-screen scroll cost a third of cell ingest.
        for y in cy..bot + 1 - n {
            let (src, dst) = (
                self.phys(Row::from_index(y + n)) * s,
                self.phys(Row::from_index(y)) * s,
            );
            self.cells.copy_within(src..src + cols, dst);
        }
        for y in bot + 1 - n..=bot {
            let a = self.phys(Row::from_index(y)) * s;
            self.cells[a..a + cols].fill(blank);
        }
    }
    pub fn row_slice(&self, r: Row) -> &[PackedCell] {
        let a = self.at(r, Col::ZERO);
        &self.cells[a..a + self.cols.index()]
    }
}
