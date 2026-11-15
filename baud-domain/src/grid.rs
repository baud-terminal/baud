use crate::*;

mod reflow;
mod store;
pub use reflow::{Follow, Recut, Reflowed};
pub use store::*;

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

/// Rows the grid is rotated by: screen row `y` lives at physical row
/// `(y + rot) % rows`.
///
/// **This is what makes a full-screen scroll O(1).** In `baud-core2` scrolling
/// advanced `head`, which indexed its run table, and left the cells alone --
/// so its `Computed` and `Templated` modes never scrolled. Copying the grid
/// instead was
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

/// Read-only counterpart of [`View`].
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
    /// which columns hold tails -- the same reason `snap_row` widens a fill to
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
///
/// Its memory is a [`Stamped`]: no method hands out the surface's cells
/// unstamped.
///
/// ```compile_fail,E0599
/// fn f(v: &mut baud_domain::View<'_>) { let _ = v.ring_cells_mut(); }
/// ```
/// ```
/// fn f(v: &mut baud_domain::View<'_>) { let _ = v.ring_cells(); }
/// ```
pub struct View<'a> {
    /// The surface's cells, its marks -- how far glyphs have reached in
    /// each **physical** row, see [`RowExtent`] -- and its rows'
    /// generations, as one [`Stamped`]: every store this view makes goes
    /// through it and stamps the row it writes, so a renderer can keep the
    /// rows nothing wrote.
    ///
    /// The marks are carried here rather than maintained by the caller so
    /// that the two cannot drift: every mutator on this type bumps them, and
    /// the line moves permute them in the same loop that permutes the rows.
    store: Stamped<'a>,
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

// A `View` is its [`Stamped`] (48 bytes) and four words: 80 bytes, at most
// 16 more than the two slices and the stride it held before the store.
const _: () = assert!(
    core::mem::size_of::<View<'static>>() <= 88,
    "a View past 88 bytes"
);

/// **Which physical rows of the primary hold anything past the live edge**,
/// one [`PastEdge`] a row of the ring, and how many are [`PastEdge::Held`].
///
/// What the passes that carry cells past a narrower edge read to skip a row
/// with nothing there ([`View::copy_past_edge`], [`View::clear_past_edge`],
/// [`View::insert_lines_past_edge`], [`View::delete_lines_past_edge`]), set
/// whole by [`View::held_past_edge`] at a resize, and what that measure reads
/// to skip all but one cell of a row clear past the old edge. Between resizes
/// nothing else writes past the live edge but a fill to a mark that lies past
/// it, on a row already held, and the clear an erase to the end of a row
/// makes first ([`View::clear_past_edge_of`]), so the flags stay true while only those passes
/// keep them. The count is what lets a caller drop back to the loop with
/// nothing past the edge compiled in once the last held row is carried off or
/// cleared. Physical, as the marks are, so a rotation leaves every flag on
/// its row.
pub struct PastRows<'a> {
    pub flags: &'a mut [PastEdge],
    pub held: &'a mut Rows,
}

impl PastRows<'_> {
    #[inline(always)]
    fn get(&self, p: usize) -> PastEdge {
        self.flags[p]
    }

    /// Set physical row `p`'s flag, keeping the count.
    #[inline(always)]
    fn set(&mut self, p: usize, to: PastEdge) {
        let was = core::mem::replace(&mut self.flags[p], to);
        *self.held = match (was, to) {
            (PastEdge::Clear, PastEdge::Held) => Rows::of_count(self.held.index() + 1),
            (PastEdge::Held, PastEdge::Clear) => {
                Rows::of_count(self.held.index().saturating_sub(1))
            }
            _ => *self.held,
        };
    }
}

/// **What a whole-row fill leaves of the row's paragraph properties**
/// ([`View::fill_row`], [`View::fill_new_row`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowFill {
    /// Erased in place: its path, autodetection and box mirroring kept.
    Erased,
    /// A new line: none.
    New,
}

/// What a ring row holds, by [`View::ring_row_holds`]: a written
/// character, a space included, or nothing but erased cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Holds {
    Text,
    Nothing,
}

/// **Whether the flags a resize starts from were kept**, for
/// [`View::held_past_edge`]: they are kept while the reach lies past the live
/// width, and read only then. Not kept, they are cleared before the measure
/// reads them -- for its cost, not its answer: a stale flag held costs a row
/// a full read, and one clear says what the reach's invariant says of every
/// row, so trusting them reads more and finds the same.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kept {
    /// The reach lay past the old width: each flag is true of its row there.
    Flags,
    /// The reach lay at the old width: every row of the ring is clear past it
    /// -- nothing at or past the reach, no mark past it -- whatever the flags
    /// still say from an earlier narrowing.
    Nothing,
}

/// **The past-edge bookkeeping against every cell of the ring**
/// ([`Viewport::past_edge_check`]): the first row whose flag is wrong in each
/// direction, the reach kept and read, and the count kept, flagged and read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PastEdgeCheck {
    /// The first physical row flagged clear that holds something past the
    /// live edge: what neither the measure nor any pass may leave, since a
    /// pass skips the row and a growth shows what it kept.
    pub clear_but_holding: Option<RowRot>,
    /// The first physical row flagged held that holds nothing past the edge:
    /// the safe direction, which an erase that empties a held row leaves
    /// between resizes, and which a resize's measure does not.
    pub held_but_clear: Option<RowRot>,
    /// The reach kept, and the furthest any row holds anything, read to the
    /// pitch.
    pub reach: (Cols, Cols),
    /// The count kept, the rows flagged held, and the rows holding anything.
    pub held: (Rows, Rows, Rows),
}

impl PastEdgeCheck {
    /// What a resize's measure leaves: every flag the row's, the reach the
    /// furthest any row holds anything, the count the rows that do.
    pub fn exact(&self) -> bool {
        self.clear_but_holding.is_none()
            && self.held_but_clear.is_none()
            && self.reach.0 == self.reach.1
            && self.held.0 == self.held.2
            && self.held.1 == self.held.2
    }

    /// What holds between resizes: no row flagged clear holds anything,
    /// nothing lies past the reach, and the count is the flags'.
    pub fn sound(&self) -> bool {
        self.clear_but_holding.is_none()
            && self.reach.0 >= self.reach.1
            && self.held.0 == self.held.1
    }
}

/// How far past `w` physical row `p` holds anything, to `end`: past its last
/// cell other than `EMPTY` there, or its mark, whichever is further, and `w`
/// when neither lies past it. The full read of one row, which
/// [`View::held_past_edge`] takes for a row held and
/// [`Viewport::past_edge_check`] for every row.
#[inline(always)]
fn reach_of_row(
    cells: &[PackedCell],
    mark: RowExtent,
    s: usize,
    p: usize,
    w: usize,
    end: usize,
) -> usize {
    let m = mark.index().min(end);
    let row = &cells[p * s + w.min(end)..p * s + end];
    let far = match row.iter().rposition(|c| *c != PackedCell::EMPTY) {
        Some(k) => w + k + 1,
        None => w,
    };
    far.max(m)
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
    used: Extents<'a>,
    stride: Stride,
    cols: Cols,
    rows: Rows,
    ring: Rows,
    rot: RowRot,
    /// The primary's generations and the line row 0 holds, on a view of the
    /// primary with any row frozen (`crate::frozen`); `None` reads every row
    /// through the live table.
    frozen: Option<(crate::FrozenRows<'a>, crate::LineIdx)>,
    /// The rotation and the ring the frozen rows are looked up by, when
    /// they are not this view's own: a copy of another view's rows, each in
    /// its place ([`Viewport::styled_from`]).
    looked_up: Option<(RowRot, Rows)>,
}

/// **Where a [`Viewport`]'s marks are read from**: a reader's own copy, one
/// [`RowExtent`] a row, or a surface's entries in place, each a row's mark
/// beside its generation ([`RowMarks`]). One match a mark read, and a mark is
/// read once a row, never per cell.
#[derive(Clone, Copy)]
enum Extents<'a> {
    Lent(&'a [RowExtent]),
    Stored(RowMarks<'a>),
}

impl core::ops::Index<usize> for Extents<'_> {
    type Output = RowExtent;
    #[inline(always)]
    fn index(&self, p: usize) -> &RowExtent {
        match self {
            Extents::Lent(u) => &u[p],
            Extents::Stored(m) => &m[p],
        }
    }
}

impl<'a> Viewport<'a> {
    /// A view over a reader's own cells and marks: a copy of the screen.
    pub fn new(
        cells: &'a [PackedCell],
        used: &'a [RowExtent],
        stride: Stride,
        cols: Cols,
        rows: Rows,
        ring: Rows,
        rot: RowRot,
    ) -> Viewport<'a> {
        Viewport::over(cells, Extents::Lent(used), stride, cols, rows, ring, rot)
    }

    /// **A view over a surface where it is stored**: its cells, and its
    /// marks read out of the entries that hold each row's generation too.
    /// What `Term` builds its views on.
    pub fn stored(
        cells: &'a [PackedCell],
        used: RowMarks<'a>,
        stride: Stride,
        cols: Cols,
        rows: Rows,
        ring: Rows,
        rot: RowRot,
    ) -> Viewport<'a> {
        Viewport::over(cells, Extents::Stored(used), stride, cols, rows, ring, rot)
    }

    /// The view both constructors build, on either source of marks: no row
    /// frozen, and every row looked up by this view's own rotation.
    fn over(
        cells: &'a [PackedCell],
        used: Extents<'a>,
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
            frozen: None,
            looked_up: None,
        }
    }

    /// **A copy of `src`'s rows reading each row through `src`'s table for
    /// it**: row `y` here holds row `y` of `src`'s cells, in whatever order
    /// within the row, so its ids name what `src`'s row `y` names.
    pub fn styled_from(self, src: &Viewport<'a>) -> Viewport<'a> {
        Viewport {
            frozen: src.frozen,
            looked_up: src.frozen.map(|_| (src.rot, src.ring)),
            ..self
        }
    }

    /// **The same view, reading its rows through the primary's
    /// generations**: row 0 holds line `line0`. What `Term::view` builds on
    /// the primary, so a row behind the window that was frozen resolves its
    /// cells through its generation's words ([`Viewport::styles`]).
    pub fn with_frozen(self, rows: crate::FrozenRows<'a>, line0: crate::LineIdx) -> Viewport<'a> {
        Viewport {
            frozen: rows.any().then_some((rows, line0)),
            ..self
        }
    }

    /// **Which table screen row `y`'s cells name rendition ids in**: the
    /// live table, or the words of the generation the row was frozen into.
    /// A reader resolving a cell of a view that may hold history asks this
    /// once a row, never the live table alone.
    pub fn styles(&self, y: Row) -> crate::RowStyles<'a> {
        let p = match self.looked_up {
            Some((rot, ring)) => rot.advance(y.index(), ring),
            None => RowRot(self.phys(y.index())),
        };
        match self.frozen {
            Some((f, line0)) => f.styles(p, line0.plus(Rows::of_count(y.index()))),
            None => crate::RowStyles::Live,
        }
    }

    /// Whether any row of this view may resolve through a generation:
    /// `false` is every row live, without asking each.
    pub fn any_frozen(&self) -> bool {
        self.frozen.is_some()
    }

    fn phys(&self, y: usize) -> usize {
        let ring = self.ring.index();
        let p = y + self.rot.index();
        if p >= ring { p - ring } else { p }
    }

    /// **The flags, the count and the reach against every cell of the
    /// ring**, read to the pitch: what [`View::held_past_edge`] must leave,
    /// and what the passes past the edge must keep. While `reach` lies past
    /// the live width the flags are read as kept; at it they are not kept,
    /// every row must be clear past the edge, and the count is taken as
    /// zero. Reads every cell past the live edge of every row, so a check
    /// for tests and debug builds, never a path.
    pub fn past_edge_check(&self, reach: Cols, flags: &[PastEdge], held: Rows) -> PastEdgeCheck {
        let (s, w) = (self.stride.index(), self.cols.index());
        let kept = reach.index() > w;
        let mut check = PastEdgeCheck {
            clear_but_holding: None,
            held_but_clear: None,
            reach: (reach, Cols::of_index(w)),
            held: (
                match kept {
                    true => held,
                    false => Rows::ZERO,
                },
                Rows::ZERO,
                Rows::ZERO,
            ),
        };
        assert!(flags.len() >= self.ring.index(), "a flag a row of the ring");
        let (mut flagged, mut holding, mut far) = (0, 0, w);
        for (p, &kept_flag) in flags.iter().enumerate().take(self.ring.index()) {
            let reach_p = reach_of_row(self.cells, self.used[p], s, p, w, s);
            far = far.max(reach_p);
            let flag = match kept {
                true => kept_flag,
                false => PastEdge::Clear,
            };
            match (flag, reach_p > w) {
                (PastEdge::Clear, true) => {
                    check.clear_but_holding = check.clear_but_holding.or(Some(RowRot(p)));
                }
                (PastEdge::Held, false) => {
                    check.held_but_clear = check.held_but_clear.or(Some(RowRot(p)));
                }
                _ => {}
            }
            flagged += usize::from(flag == PastEdge::Held);
            holding += usize::from(reach_p > w);
        }
        check.reach.1 = Cols::of_index(far);
        check.held.1 = Rows::of_count(flagged);
        check.held.2 = Rows::of_count(holding);
        check
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
            looked_up: self.looked_up.map(|(r, ring)| (r.retreat(n, ring), ring)),
            frozen: self.frozen.map(|(f, l)| {
                (
                    f,
                    l.minus(Rows::of_count(n)).unwrap_or(crate::LineIdx::FIRST),
                )
            }),
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

    /// Screen row `y`'s character path: which way SCP or SPD set its line
    /// to run ([`CharacterPath`]), or none.
    pub fn path(&self, y: Row) -> CharacterPath {
        self.used[self.phys(y.index())].path()
    }

    /// Screen row `y`'s paragraph properties ([`LineBidi`]).
    pub fn bidi(&self, y: Row) -> LineBidi {
        self.used[self.phys(y.index())].bidi()
    }

    /// **The side margins screen row `y` wrapped at**, where it wrapped at
    /// margins narrower than the page ([`crate::WrapMargins`]).
    pub fn margins(&self, y: Row) -> Option<crate::WrapMargins> {
        self.used[self.phys(y.index())].margins()
    }

    /// Screen row `y`'s line rendition: how wide, and which half tall, its
    /// characters are drawn ([`LineRendition`]). A line not single-width
    /// holds [`LineRendition::end_of_line`] characters, each drawn two
    /// columns wide, and its cells past them are empty.
    pub fn rendition(&self, y: Row) -> LineRendition {
        self.used[self.phys(y.index())].rendition()
    }

    /// Screen row `y`'s mark whole -- its extent, wrap, rendition, path and
    /// paragraph -- for a reader that keeps a copy of the rows it read and
    /// reads that copy again as a view of its own.
    pub fn extent(&self, y: Row) -> RowExtent {
        self.used[self.phys(y.index())]
    }

    /// **Whether screen row `y` holds text past the view's width**: a
    /// written cell between the width and the row's mark, which a narrowing
    /// that did not re-cut the row keeps there, to show again if the width
    /// comes back ([`View::held_past_edge`]). [`Viewport::row`] stops at the
    /// width, so a reader that lays a paragraph's text out from the cells it
    /// returns asks this first: such a row's visible cells are not all of
    /// its text. A row whose mark lies within the width reads nothing.
    pub fn text_past_edge(&self, y: Row) -> PastEdge {
        let row = self.pitch_row(y.index());
        let end = self.extent(y).index().min(row.len());
        let held = row
            .get(self.cols.index()..end)
            .is_some_and(|past| past.iter().any(|c| !c.content.is_empty()));
        match held {
            true => PastEdge::Held,
            false => PastEdge::Clear,
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
    store: Stamped<'a>,
    ring: Rows,
    rot: RowRot,
}

/// ```compile_fail,E0616
/// fn f(s: &baud_domain::Surface<'_>) -> baud_domain::RowRot { s.rot }
/// ```
impl<'a> Surface<'a> {
    /// A surface over `store`: a ring of `ring` rows under rotation `rot`.
    pub fn new(store: Stamped<'a>, ring: Rows, rot: RowRot) -> Surface<'a> {
        Surface { store, ring, rot }
    }
    /// Its rotation.
    pub fn rot(&self) -> RowRot {
        self.rot
    }
    /// Its ring's depth.
    pub fn ring(&self) -> Rows {
        self.ring
    }
    /// Its memory, to read.
    pub fn store(&self) -> &Stamped<'a> {
        &self.store
    }
    /// Its memory, to write: every store through it stamps.
    pub fn store_mut(&mut self) -> &mut Stamped<'a> {
        &mut self.store
    }
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
    /// A view of `rows` x `cols` over `store`, a ring of `ring` rows under
    /// rotation `rot`, each row at the store's pitch.
    ///
    /// # Panics
    /// If `cols` exceeds the pitch: a column past it would land in the next
    /// row, which a one-character writer's [`RowWrite`] does not check.
    pub fn new(store: Stamped<'a>, cols: Cols, rows: Rows, ring: Rows, rot: RowRot) -> View<'a> {
        assert!(
            cols.index() <= store.stride().index(),
            "a view wider than its pitch"
        );
        View {
            store,
            cols,
            rows,
            ring,
            rot,
        }
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
        let base = p * self.stride().index();
        let (to, bidi) = (self.store.used()[p].index(), self.store.used()[p].bidi());
        let mark = self
            .mark_after_fill(base, to, cell, RowExtent::of(a))
            .with_bidi(bidi)
            .with_links(links_after(self.store.used()[p], cell));
        // From column 0 the row is erased whole, and DECRLM's mark goes.
        let (mut row, mut m) = self.store.row_and_mark(p);
        *m = match a == 0 {
            true => mark.erased(),
            false => mark,
        };
        if a >= to {
            return 0;
        }
        row[a..to].fill(cell);
        to - a
    }

    /// The mark after a fill of `cell` that ran to `to`, where `low` is the
    /// mark the fill would leave on a row it covered to the live edge and no
    /// further. Past the live edge the fill ran on to the mark, and that
    /// blank is the line's to the mark and no further: past it lie the cells
    /// of whatever line the row held before, which the passes past the edge
    /// clear to [`PackedCell::EMPTY`] from the mark on
    /// ([`View::clear_past_edge`]). So the mark stays at `to`, and comes
    /// down to `low` only when `cell` is `EMPTY` and either the cell at `to`
    /// is `EMPTY` or `to` is the pitch: then the row is uniform from `low`
    /// and that clear changes nothing. That lowering is also how a held row
    /// comes back inside the edge and the reach comes down. A coloured blank
    /// equal to the retired cell at `to` is not uniform with it -- the
    /// retired cell is cleared and the blank is not -- and reading it so
    /// lost the blank past the edge on a growth.
    fn mark_after_fill(
        &self,
        base: usize,
        to: usize,
        cell: PackedCell,
        low: RowExtent,
    ) -> RowExtent {
        let (cols, s) = (self.cols.index(), self.stride().index());
        match to <= cols
            || (cell == PackedCell::EMPTY && (to >= s || self.store.cells()[base + to] == cell))
        {
            true => low,
            false => RowExtent::of(to),
        }
    }

    /// Row `r` continues onto the next: its text reached the last column and
    /// wrapped. Set by the writer at the wrap, on the row being left, at the
    /// page's edge ([`RowExtent::at_edge`]): a wrap at side margins writes
    /// them after ([`View::set_margins`]). The writer calls it only for a
    /// wrap at the screen's edge ([`WrappedAt`](crate::WrappedAt)).
    ///
    /// Out of line, and so is `set_ended`: both are once-a-line events, and
    /// inlined into the writers they grew the per-byte body by enough to
    /// cost btop 7% on a recording that wraps nothing -- the body's size is
    /// paid by every payload whether the call runs or not.
    #[cold]
    #[inline(never)]
    pub fn set_wrapped(&mut self, r: Row) {
        let p = self.phys(r);
        self.store.mark(
            p,
            self.store.used()[p].wrapping(self.cols.index()).at_edge(),
        );
    }

    /// Row `r` ends where it stops: a newline left it. Clears the wrap.
    #[cold]
    #[inline(never)]
    pub fn set_ended(&mut self, r: Row) {
        let p = self.phys(r);
        self.store.mark(p, self.store.used()[p].ended());
    }

    /// Exchange this view's surface with `other`: the cells, marks, depth
    /// and rotation, leaving the geometry. What `?1049` does now -- the
    /// alternate screen is a second surface rather than a place to park the
    /// primary, so entering and leaving it copies nothing.
    pub fn swap_surface(&mut self, other: &mut Surface<'a>) {
        core::mem::swap(&mut self.store, &mut other.store);
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
        let (a, n, s) = (self.rot.index(), self.rows.index(), self.stride().index());
        match a + n <= self.ring.index() {
            true => Some(&self.store.cells()[a * s..(a + n) * s]),
            false => None,
        }
    }

    /// The screen's rows into `dst` in screen order, a row at a time, for
    /// the window that wraps.
    pub fn window_into(&self, dst: &mut [PackedCell]) {
        let s = self.stride().index();
        for y in 0..self.rows.index() {
            let a = self.phys(Row::from_index(y)) * s;
            dst[y * s..(y + 1) * s].copy_from_slice(&self.store.cells()[a..a + s]);
        }
    }

    /// After the screen widened, blank the columns the alternate surface just
    /// revealed -- `[old_cols, cols)` in every row of the ring. The alternate
    /// screen has no scrollback and the application owns it: a cell past the
    /// old edge holds whatever was there when the screen was last this wide,
    /// and a full-screen program redraws a frame or two *after* the resize, so
    /// showing that stale content under the incoming draw is the "two images
    /// at once" a grow used to flash. Blanking the revealed columns leaves the
    /// overlap -- which the program redraws -- and clears the new region,
    /// which is what a resize should show. Each row's mark is set to the old
    /// edge as it goes, which is what the earlier mark-only version did for
    /// the fill shortcut; the difference is that the cells are now blank
    /// rather than merely marked stale. This is the width half of a grow; the
    /// primary reflows instead, so it is never revealed stale.
    ///
    /// **Every row of the ring, not only the screen's.** A row a shorter
    /// alternate screen cut, or one that scrolled off its top, stays in the
    /// ring past the window with a mark true only to the width it was left
    /// at; a later growth reveals it, and the blank that clears it trusts the
    /// mark. Walking only the screen's rows left such a row's band past its
    /// old edge standing, under a mark of zero that the next erase then
    /// trusted too (`a_band_on_a_cut_row_is_blanked_when_a_growth_reveals_it`).
    /// So for the walk the view's rows are the ring, and screen row `y` of
    /// `0..ring` is every physical row once under the rotation.
    ///
    /// **Written, not taken through `fill`'s shortcut.** A fill to the end
    /// of a row stops at the mark when the row's last cell is already the
    /// blank, trusting the row uniform from its mark to the live edge -- and
    /// here the live edge is the new one, which the mark is not yet true of.
    /// A row erased in blue at 20 columns is blue to 20 under a mark of
    /// zero; narrowed to 19 and grown to 21, its last cell is the default
    /// blank the erase never reached, so the shortcut wrote nothing and left
    /// the blue cell at column 19 standing under a mark of 19 that said the
    /// row was uniform from there. A later `CSI 2 K` trusted that mark too,
    /// and the region repair's `copy_row` asserted on it
    /// (`a_band_past_the_old_edge_is_blanked_when_a_growth_reveals_it`). The
    /// span is snapped to whole characters, as `fill` snaps it, and the mark
    /// set to its start, as a fill to the end of a row sets it; only the
    /// early stop goes. A row whose mark lies past the new edge is cleared to
    /// its mark, as a fill to the end of a row clears it (`fill_past_edge`):
    /// the next growth's snap reads the cell at its old edge, and a wide tail
    /// left standing there pulled the blank back over the live cell in front
    /// of it (`a_growth_leaves_no_tail_past_the_edge_for_the_next_one_to_snap_to`).
    pub fn widened(&mut self, old_cols: Cols) {
        let (from, to) = (
            Col::at_index(old_cols.index()),
            Col::at_index(self.cols.index()),
        );
        let rows = self.rows;
        self.rows = self.ring;
        for y in 0..self.ring.index() {
            let r = Row::from_index(y);
            let (a, b) = self.snap(r, from, to);
            let p = self.phys(r);
            let kept = self.store.used()[p].rendition();
            if self.store.used()[p].index() > self.cols.index() {
                self.fill_past_edge(p, a, PackedCell::EMPTY);
                self.store
                    .mark(p, self.store.used()[p].with_rendition(kept, self.cols));
                continue;
            }
            let (mut row, mut m) = self.store.row_and_mark(p);
            row[a..b].fill(PackedCell::EMPTY);
            // The line keeps its rendition: a growth reveals columns, it does
            // not erase the line (`[[source: dec-std-070]]` resets a rendition
            // only where a line is erased whole or scrolled in, pp. 5-156 and
            // 5-210).
            *m = m.refilled(a).with_rendition(kept, self.cols);
        }
        self.rows = rows;
    }

    /// After the live width grew from `old`: make every row's mark true of
    /// the wider screen. A mark says the row is uniform from it to the live
    /// edge -- the fill shortcut trusts `cells[cols - 1]` to be that uniform
    /// cell and stops at the mark -- and at the old width that held to the
    /// old edge only. Where the columns the growth reveals are not that
    /// uniform cell, the mark rises to where the row's uniform suffix
    /// begins, the wrap kept; a row that already is uniform keeps its mark,
    /// so a short line is not read as a line of blanks by a later re-cut, the
    /// way raising every mark to the old width would have it. Over the whole
    /// ring, because the history, the rows below the window and the free rows
    /// are all cells a scroll or a growth brings into view. Returns the rows
    /// raised.
    ///
    /// **A wrapped row's mark rises to the new edge, whatever its cells.**
    /// Its mark is the width it wrapped at, and [`RowExtent::reached`] keeps
    /// a flagged mark by `max` alone, on the promise that no write reaches
    /// past it -- true while the screen is no wider than where the row
    /// wrapped, and false once it grows without a re-cut, which a prompt's
    /// does. Left there, a write past it was below a mark no write could
    /// raise: grown from 13 columns to 17 at a prompt, `XY` written at
    /// columns 15 and 16 of a wrapped row survived `CSI 2 K`, which stops at
    /// the mark when the row ends in the blank, and a prompt's redraw left
    /// them under a mark of zero (`row_extent.rs`). Too large is the safe
    /// direction for a mark; a re-cut reads a wrapped row's cells by the
    /// width, not by its mark.
    pub fn settle_growth(&mut self, old: Cols) -> usize {
        let (old, new, s) = (old.index(), self.cols.index(), self.stride().index());
        if new <= old || old == 0 {
            return 0;
        }
        let mut raised = 0;
        for p in 0..self.ring.index() {
            let m = self.store.used()[p].index();
            if m >= new {
                continue;
            }
            // A sized row's mark is the width it was sized at, and `reached`
            // keeps it by `max` alone on the same promise the wrap's makes.
            if self.store.used()[p].flagged() {
                self.store.mark(p, self.store.used()[p].raised_to(new));
                raised += 1;
                continue;
            }
            let base = p * s;
            let from = m.max(old);
            let u = self.store.cells()[base + if m < old { old - 1 } else { m }];
            if self.store.cells()[base + from..base + new]
                .iter()
                .all(|c| *c == u)
            {
                continue;
            }
            let last = self.store.cells()[base + new - 1];
            let mut k = new;
            while k > m && self.store.cells()[base + k - 1] == last {
                k -= 1;
            }
            self.store.mark(p, self.store.used()[p].raised_to(k));
            raised += 1;
        }
        raised
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
        let base = p * self.stride().index();
        &self.store.cells()[base..base + self.cols.index()]
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

    /// Erase every row of the ring at the whole pitch, and every mark: the
    /// surface as it was built, with the rotation where it is. What a full
    /// reset leaves, because the rows behind and below the window are cells
    /// too -- a reader of the whole ring reaches them, and a re-cut that spills
    /// past the screen writes into them without clearing past its width.
    /// Returns the cells written.
    pub fn erase_ring(&mut self) -> usize {
        let (mut cells, mut marks) = self.store.all_mut();
        cells.fill(PackedCell::EMPTY);
        marks.fill(RowExtent::ZERO);
        cells.len()
    }

    /// Erase every row of the ring but the `keep` rows from the window's top,
    /// at the whole pitch, marks included: the screen and the rows a shorter
    /// window cut below it stay, and the history goes -- with the rows past
    /// both, which hold no line a count can reach and are cells all the same.
    /// What `CSI 3 J` leaves, for [`View::erase_ring`]'s reason: a reader of
    /// the whole ring reaches every row, and a re-cut that spills past the
    /// screen writes into them without clearing past its width. The rotation
    /// stays, because nothing scrolled. Returns the cells written, and
    /// whether a row it erased may have held a linked cell ([`RowLinks`]):
    /// what lets the link table count the erase as paying for the sweep
    /// after it.
    pub fn erase_outside(&mut self, keep: Rows) -> (usize, RowLinks) {
        let mut links = RowLinks::None;
        let (ring, s) = (self.ring.index(), self.stride().index());
        let mut n = 0;
        for k in keep.index().min(ring)..ring {
            let p = self.rot.advance(k, self.ring).index();
            let (mut row, mut m) = self.store.row_and_mark(p);
            row.fill(PackedCell::EMPTY);
            if m.links() == RowLinks::May {
                links = RowLinks::May;
            }
            *m = RowExtent::ZERO;
            n += s;
        }
        (n, links)
    }

    /// Turn every character DECSCA left erasable in the `hist` rows behind
    /// the window into a space in its own rendition, to the live edge, and
    /// keep the rest, an empty position included
    /// ([`PackedCell::selectively_erased`]): `CSI ? 3 J`, which is DECSED
    /// and so does what DECSED does on the screen. The rows stay rows of the history. A mark only rises, as
    /// [`View::put`]'s does. Returns the cells written.
    pub fn selective_wipe_behind(&mut self, hist: Rows) -> usize {
        let cols = self.cols.index();
        let mut n = 0;
        for k in 1..=hist.index().min(self.ring.index()) {
            let p = self.rot.retreat(k, self.ring).index();
            let (mut row, mut m) = self.store.row_and_mark(p);
            for x in 0..cols {
                let old = row[x];
                if !old.is_protected() && !old.content.is_empty() {
                    row[x] = PackedCell::selectively_erased(old);
                    *m = m.reached(x + 1);
                    n += 1;
                }
            }
        }
        n
    }

    /// Copy screen row `from` over screen row `to` to the live edge, extent
    /// included, and return the cells written.
    ///
    /// **For repairing the rows a rotation moved that should not have moved.**
    /// A scrolling region that does not span the screen cannot be a whole-grid
    /// rotation -- but it can be one *plus* putting the few rows outside the
    /// region back, and for the usual shape (a full-height region with a
    /// status line under it) that is one row instead of forty-nine.
    ///
    /// **Only as far as the larger of the two rows' marks, inside the live
    /// edge.** Past its mark a row is uniform to the live edge -- every cell
    /// its last -- so past the larger mark the destination already holds the
    /// source's cells when the two rows end in the same cell, and one fill
    /// from the source's last cell makes it hold them when they do not. A log
    /// pane over a dashboard copies every dashboard row on every scroll, and
    /// a dashboard row uses a quarter of a 200-column screen.
    ///
    /// **The cells past the live edge are not this one's.** A mark is true
    /// to the live edge and no further; [`View::copy_past_edge`] carries what
    /// lies past it, for a screen narrower than it has been.
    ///
    /// **Inlined into its caller, always.** The repair's loop has two
    /// monomorphs, one for a screen at its widest and one past the edge
    /// (`Paint::rotate_top_region`), and with two call sites LLVM put this
    /// out of line: the log pane over a dashboard read 6.5% down, on the
    /// widest screen's loop, which is the one every recording runs.
    #[inline(always)]
    pub fn copy_row(&mut self, from: Row, to: Row) -> usize {
        let (a, b) = (self.phys(from), self.phys(to));
        if a == b {
            return 0;
        }
        let w = self.cols.index();
        let n = self.store.used()[a]
            .index()
            .max(self.store.used()[b].index())
            .min(w);
        let (src, from, mut row, mut m) = self.store.from_into(a, b);
        let (src, dst) = (&src[..w], &mut row[..w]);
        dst[..n].copy_from_slice(&src[..n]);
        *m = from;
        if n == w {
            return n;
        }
        let last = src[w - 1];
        debug_assert!(
            src[n..].iter().all(|&c| c == last) && dst[n..].iter().all(|&c| c == dst[w - 1]),
            "a row is not uniform past its mark"
        );
        if dst[w - 1] == last {
            return n;
        }
        dst[n..].fill(last);
        w
    }

    /// Each of screen rows `top..bottom` over the row below it, descending,
    /// **past the live edge, to `reach`** -- the furthest any row of the ring
    /// holds anything ([`View::held_past_edge`]) -- verbatim, for a pair
    /// either of whose rows holds something there, and the cells written.
    /// The other half of the region repair's [`View::copy_row`], for a screen
    /// narrower than what its rows hold.
    ///
    /// A mark is true to the live edge and no further, so it says nothing of
    /// the cells past it: a tail a resize at a prompt hid, or a colour a row
    /// was uniform in to the old edge under a mark of zero. A growth without
    /// a re-cut shows those cells as they are, so a row the repair puts back
    /// has to carry them, and the row it lands on must not keep its own. Past
    /// `reach` every row holds what the storage was provisioned with, so
    /// nothing there differs.
    ///
    /// **Only the pairs that hold something.** Two rows both
    /// [`PastEdge::Clear`] are `EMPTY` past the edge to `reach` alike, so the
    /// copy would write what is there; a row's flag goes with its cells. One
    /// line of history wider than the narrowed screen held `reach` at the old
    /// width and the repair copied every row under the region past the edge
    /// every scroll: the narrowed log pane read 95.7 MB/s against 208.4.
    ///
    /// A pass of its own rather than a step of `copy_row`, because the two
    /// halves touch disjoint columns: a screen with nothing past its edge --
    /// every screen the corpus records -- keeps `copy_row`'s loop as it was.
    /// Only after a narrowing, so out of line.
    #[cold]
    #[inline(never)]
    pub fn copy_past_edge(
        &mut self,
        top: Row,
        bottom: Row,
        reach: Cols,
        past: &mut PastRows<'_>,
    ) -> usize {
        let s = self.stride().index();
        let (w, end) = (self.cols.index(), reach.index().min(s));
        if end <= w {
            return 0;
        }
        let mut n = 0;
        for y in (top.index() + 1..=bottom.index()).rev() {
            let (a, b) = (
                self.phys(Row::from_index(y - 1)),
                self.phys(Row::from_index(y)),
            );
            n += self.move_past_edge(a, b, w, end, past);
        }
        n
    }

    /// Physical row `a`'s cells from `w` to `end` over row `b`'s, and its
    /// flag with them, when either row is held; the cells written.
    #[inline(always)]
    fn move_past_edge(
        &mut self,
        a: usize,
        b: usize,
        w: usize,
        end: usize,
        past: &mut PastRows<'_>,
    ) -> usize {
        let from = past.get(a);
        if a == b || (from == PastEdge::Clear && past.get(b) == PastEdge::Clear) {
            return 0;
        }
        self.store.copy_within_rows(a, w..end, b, w);
        past.set(b, from);
        end - w
    }

    /// The rows `IL` moves, past the live edge: [`View::insert_lines_in`]'s
    /// moves on the columns from the live edge to `reach`, row for row and
    /// in its order, then the rows it opened cleared there. That pass moves
    /// a row to the live edge and carries its mark, so without this a hidden
    /// tail moved by `IL` came back cut at the narrow edge, and a line it
    /// opened over a row uniform in a colour came back in that colour past
    /// it. Each pair as [`View::copy_past_edge`] takes it, only where either
    /// row holds something. Run after it, on the primary narrower than its
    /// rows hold; the cells written.
    #[cold]
    #[inline(never)]
    pub fn insert_lines_past_edge(
        &mut self,
        at: Row,
        n: Count,
        bottom: Row,
        reach: Cols,
        past: &mut PastRows<'_>,
    ) -> usize {
        let (cy, bot) = (at.index(), bottom.index().min(self.rows.index() - 1));
        let (w, end) = (self.cols.index(), reach.index().min(self.stride().index()));
        if cy > bot || end <= w {
            return 0;
        }
        let n = (n.get_raw() as usize).min(bot + 1 - cy);
        let mut written = 0;
        for y in (cy + n..=bot).rev() {
            let (a, b) = (
                self.phys(Row::from_index(y - n)),
                self.phys(Row::from_index(y)),
            );
            written += self.move_past_edge(a, b, w, end, past);
        }
        for y in cy..cy + n {
            written += self.clear_past_edge(Row::from_index(y), reach, past);
        }
        written
    }

    /// The rows `DL` moves, past the live edge: [`View::delete_lines_in`]'s
    /// moves on the columns from the live edge to `reach`, in its order, then
    /// the rows it opened at the bottom cleared there. The twin of
    /// [`View::insert_lines_past_edge`], for the same defect; the cells
    /// written.
    #[cold]
    #[inline(never)]
    pub fn delete_lines_past_edge(
        &mut self,
        at: Row,
        n: Count,
        bottom: Row,
        reach: Cols,
        past: &mut PastRows<'_>,
    ) -> usize {
        let (cy, bot) = (at.index(), bottom.index().min(self.rows.index() - 1));
        let (w, end) = (self.cols.index(), reach.index().min(self.stride().index()));
        if cy > bot || end <= w {
            return 0;
        }
        let n = (n.get_raw() as usize).min(bot + 1 - cy);
        let mut written = 0;
        for y in cy..bot + 1 - n {
            let (a, b) = (
                self.phys(Row::from_index(y + n)),
                self.phys(Row::from_index(y)),
            );
            written += self.move_past_edge(a, b, w, end, past);
        }
        for y in bot + 1 - n..=bot {
            written += self.clear_past_edge(Row::from_index(y), reach, past);
        }
        written
    }

    /// **How far past the live edge the ring holds anything**: the least
    /// width, from the live width up to `reach`, past which every row of the
    /// ring is [`PackedCell::EMPTY`] to `reach` and no row's mark lies -- and
    /// each row's [`PastEdge`], with the count of rows held. What the passes
    /// that carry cells past a narrower edge read ([`View::copy_past_edge`])
    /// is measured here when the screen narrows, not remembered as the widest
    /// it has been: a screen narrowed before anything was written past the
    /// new width -- or re-cut, which blanks every row it writes past it --
    /// holds nothing there, and a repair that copied to the widest width wrote
    /// every row under the region past the edge again on every scroll, empty
    /// over empty.
    ///
    /// **Every row of the ring**, because a history row comes back onto the
    /// screen under a taller one, and a ring row comes round as the line a
    /// scroll brings in. **The marks too**, because a fill on a row whose
    /// mark lies past the edge runs on to the mark ([`View::fill_row`]), so a
    /// mark over blank cells is where a later blank can write: past the width
    /// this returns nothing writes but such a fill, and only on a row this
    /// holds. Compared with `EMPTY`, what `baud-core`'s `Owned` provisions,
    /// and what a surface lent through `Owned::storage_over` must hold past
    /// the width too, as every caller's does (`Gpu::grid` in
    /// `baud-present-macos`, `marks.rs`): a row clear past the old edge is
    /// read one cell deep and the rest trusted, so anything else there goes
    /// unseen, and the debug check after every resize
    /// ([`Viewport::past_edge_check`]), which reads to the pitch, panics on
    /// it. `Term::new` clears neither surface nor its marks, so an `Owned`
    /// lent to a second terminal carries the first one's cells and marks and
    /// must meet it too. Only on a resize, so out of line.
    ///
    /// **Read only as far as the flags leave open.** `was` is the width the
    /// flags were kept at, and `kept` says whether they were
    /// ([`Kept::Flags`]: the reach lay past `was`) or every row is clear past
    /// it by the reach's own invariant ([`Kept::Nothing`]), when the flags
    /// are cleared first. A row clear past `was` holds nothing from there to
    /// `reach`, its mark lies inside `was`, and from its mark to `was` it is
    /// uniform -- the mark's meaning -- so on a narrowing one cell, the last
    /// inside `was`, says what it holds past the new edge: that cell is the
    /// uniform run's, and is `EMPTY` exactly when everything from the mark or
    /// the new edge to `was` is. On a growth or at the same width such a row
    /// holds nothing past the edge at all, and is not read. The uniform run
    /// is every writer's to keep, a wrapped row's after a growth included
    /// ([`View::settle_growth`]): a generated program that broke it found the
    /// one-cell read three columns short. Only a row held is read whole, as
    /// every row was: before this, a narrowing of `k` columns read up to `k`
    /// cells of every row of the ring, and a same-size or height-only resize
    /// read them all whenever anything lay past the edge; the measure was
    /// 67-90% of a drag's resize time. The answer is the full read's, row for
    /// row: [`Viewport::past_edge_check`] reads every cell and is the oracle.
    ///
    /// **The re-cut's blanked rows are read whole** (`blanked`, the window's
    /// last rows no re-cut line reached, [`Reflowed::blanked`]): blanked to
    /// the new width under a mark of zero, they keep an old line's cells past
    /// it, which neither their flag nor their last cell describes. Every
    /// other row the re-cut writes is blank past the new width to the pitch,
    /// so its flag and the cell inside `was` both read clear, which it is.
    /// **So are the rows it kept whole** (`whole`, [`Reflowed::kept`]): each
    /// is copied with its cells past the new width, onto a ring row whose
    /// flag was another row's.
    #[cold]
    #[inline(never)]
    pub fn held_past_edge(
        &self,
        was: Cols,
        reach: Cols,
        kept: Kept,
        blanked: Rows,
        whole: (RowRot, Rows),
        past: &mut PastRows<'_>,
    ) -> Cols {
        let s = self.stride().index();
        let (w, end) = (self.cols.index(), reach.index().min(s));
        let was = was.index().min(end);
        if kept == Kept::Nothing {
            past.flags.fill(PastEdge::Clear);
            *past.held = Rows::ZERO;
        }
        let rows = self.rows.index();
        for y in rows - blanked.index().min(rows)..rows {
            past.set(self.phys(Row::from_index(y)), PastEdge::Held);
        }
        // The rows a re-cut kept whole, read whole: each landed where
        // another row's flag was.
        for k in 0..whole.1.index() {
            past.set(whole.0.advance(k, self.ring).index(), PastEdge::Held);
        }
        // Nothing held and nothing narrower: every row is clear past `was`,
        // so past this edge too.
        if w >= was && past.held.index() == 0 {
            return Cols::of_index(w);
        }
        let mut held = w;
        let mut count = 0;
        for (p, f) in past.flags.iter_mut().enumerate().take(self.ring.index()) {
            let reach_p = match *f {
                PastEdge::Held => {
                    reach_of_row(self.store.cells(), self.store.mark_at(p), s, p, w, end)
                }
                PastEdge::Clear if w < was => match self.store.cells()[p * s + was - 1] {
                    c if c != PackedCell::EMPTY => was,
                    _ => self.store.used()[p].index().min(end).max(w),
                },
                PastEdge::Clear => w,
            };
            *f = match reach_p > w {
                true => {
                    count += 1;
                    PastEdge::Held
                }
                false => PastEdge::Clear,
            };
            held = held.max(reach_p);
        }
        *past.held = Rows::of_count(count);
        Cols::of_index(held)
    }

    /// Clear the cells past the live edge and past the mark of screen row
    /// `r`, to `reach`, to the provisioned blank, when the row holds anything
    /// past the edge: the line a scroll opens or brings in, and a row `IL` or
    /// `DL` opens, whose cells past the edge were the copied row's or a
    /// retired line's. Past the mark, because a blank on a row whose mark lay
    /// past the edge ran on to the mark ([`View::fill_row`]) and that is the
    /// blank's; past both, the default blank, as an erase taken while narrow
    /// leaves it. A row left with its mark inside the edge is clear. Only
    /// after a narrowing, so out of line. Returns the cells written.
    #[cold]
    #[inline(never)]
    pub fn clear_past_edge(&mut self, r: Row, reach: Cols, past: &mut PastRows<'_>) -> usize {
        let p = self.phys(r);
        self.clear_past_edge_at(p, reach, past)
    }

    /// [`View::clear_past_edge`] on the ring row `at`, wherever the window
    /// is -- `rot.advance(y, ring)` for screen row `y`,
    /// `rot.retreat(1 + back, ring)` for a row behind it: **a row an erase
    /// covers to its end**, before the erase's fill. The erase is the line
    /// being cleared, so what the row held past the edge is the erased
    /// line's: past the mark a coloured blank run on from the width it was
    /// written at, which the mark does not bound because the mark is where
    /// the row stops being uniform, and past the edge a tail under a mark
    /// inside it. Left there, a growth showed the erased line's colour
    /// again. The erase then fills to the mark, which reads the cleared
    /// cell there, so a default erase lowers the mark and a coloured one
    /// keeps it. So when `blank`, the erase's, is the default, the row
    /// holds nothing past the edge once filled, and its flag goes clear
    /// here: left held, the reach stayed up for nothing and the next pass
    /// cleared the row again. Only after a narrowing, so out of line.
    #[cold]
    #[inline(never)]
    pub fn clear_past_edge_of(
        &mut self,
        at: RowRot,
        reach: Cols,
        past: &mut PastRows<'_>,
        blank: PackedCell,
    ) -> usize {
        let n = self.clear_past_edge_at(at.index(), reach, past);
        if blank == PackedCell::EMPTY {
            past.set(at.index(), PastEdge::Clear);
        }
        n
    }

    /// **The rows a full-screen scroll brought in since the passes past the
    /// edge last ran, cleared there**: the `n` rows of the ring that end at
    /// the screen's bottom row, going back, each as [`View::clear_past_edge`]
    /// clears it; every row of the ring once `n` reaches its length.
    ///
    /// A full-screen scroll hands the line it brings in a row of the ring --
    /// the oldest line of history once the ring is full, or a row a shorter
    /// window cut -- and blanks it to the live edge, or to its mark when that
    /// lies past it; past both it keeps the retired line's cells, which a
    /// growth shows on a line that was blank when it came in. Nothing reads
    /// those cells before a growth or a pass that moves or clears rows past
    /// the edge, so each of those clears the rows the window's bottom swept
    /// first, and the scroll -- the line feed's, the hottest path in the core
    /// -- keeps its source. Clearing there at once is a test of the reach and
    /// a cold call on every scroll; `multibench`'s cat row did not separate
    /// that arm from the base or from this form, so the work goes to the rare
    /// passes, not the common one. Rows only: the caller knows how many lines
    /// came in. Only after a narrowing, so out of line. Returns the cells
    /// written.
    #[cold]
    #[inline(never)]
    pub fn clear_swept_past_edge(
        &mut self,
        n: Rows,
        reach: Cols,
        past: &mut PastRows<'_>,
    ) -> usize {
        let (ring, rows) = (self.ring.index(), self.rows.index());
        let mut written = 0;
        for k in 0..n.index().min(ring) {
            let p = self.rot.advance(rows - 1 + ring - k, self.ring).index();
            written += self.clear_past_edge_at(p, reach, past);
        }
        written
    }

    /// [`View::clear_past_edge`] on physical row `p`.
    fn clear_past_edge_at(&mut self, p: usize, reach: Cols, past: &mut PastRows<'_>) -> usize {
        let s = self.stride().index();
        if past.get(p) == PastEdge::Clear {
            return 0;
        }
        let (w, m) = (self.cols.index(), self.store.used()[p].index());
        let from = w.max(m);
        let end = reach.index().min(s);
        if m <= w {
            past.set(p, PastEdge::Clear);
        }
        if end <= from {
            return 0;
        }
        self.store.span_mut(p, from..end).fill(PackedCell::EMPTY);
        end - from
    }

    /// The mark for screen row `r`.
    pub fn extent(&self, r: Row) -> RowExtent {
        self.store.used()[self.phys(r)]
    }

    /// Screen row `r`'s line rendition.
    pub fn rendition(&self, r: Row) -> LineRendition {
        self.store.used()[self.phys(r)].rendition()
    }

    /// Screen row `r`'s character path.
    pub fn path(&self, r: Row) -> CharacterPath {
        self.store.used()[self.phys(r)].path()
    }

    /// Screen row `r`'s paragraph properties ([`LineBidi`]).
    pub fn bidi(&self, r: Row) -> LineBidi {
        self.store.used()[self.phys(r)].bidi()
    }

    /// Give screen row `r` the paragraph properties `to`: the cells and
    /// the mark as they are.
    #[cold]
    #[inline(never)]
    pub fn set_bidi(&mut self, r: Row, to: LineBidi) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        self.store.mark(p, self.store.used()[p].with_bidi(to));
    }

    /// **Screen row `r` wrapped at side margins `m`**, narrower than the
    /// page: marked so, with the margins ([`RowExtent::with_margins`]).
    #[cold]
    #[inline(never)]
    pub fn set_margins(&mut self, r: Row, m: crate::WrapMargins) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        self.store.mark(p, self.store.used()[p].with_margins(m));
    }

    /// The paragraph properties of the row `back` rows behind the window's
    /// top, the newest at zero: a line of the history, read as
    /// [`View::set_bidi_behind`] writes it.
    pub fn bidi_behind(&self, back: Rows) -> LineBidi {
        self.store.used()[self.rot.retreat(back.index() + 1, self.ring).index()].bidi()
    }

    /// Give the row `back` rows behind the window's top the paragraph
    /// properties `to`: a line of the history that begins the paragraph a
    /// screen row continues. The caller bounds `back` by the lines the
    /// history holds.
    #[cold]
    #[inline(never)]
    pub fn set_bidi_behind(&mut self, back: Rows, to: LineBidi) {
        let p = self.rot.retreat(back.index() + 1, self.ring).index();
        self.store.mark(p, self.store.used()[p].with_bidi(to));
    }

    /// The row `back` rows behind the window's top ends where it stops: a
    /// line of the history no longer continues onto the screen.
    #[cold]
    #[inline(never)]
    pub fn set_ended_behind(&mut self, back: Rows) {
        let p = self.rot.retreat(back.index() + 1, self.ring).index();
        self.store.mark(p, self.store.used()[p].ended());
    }

    /// Whether the row `back` rows behind the window's top continues onto
    /// the next.
    pub fn wrapped_behind(&self, back: Rows) -> Wrap {
        match self.store.used()[self.rot.retreat(back.index() + 1, self.ring).index()].wrapped() {
            true => Wrap::Continues,
            false => Wrap::Ended,
        }
    }

    /// Give screen row `r` rendition `to`, through
    /// [`RowExtent::with_rendition`], which raises the mark to the live
    /// width for any rendition but single-width. The cells are the caller's:
    /// DECDWL and DECDHL empty the ones past the line's end first.
    #[cold]
    #[inline(never)]
    pub fn set_rendition(&mut self, r: Row, to: LineRendition) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        self.store
            .mark(p, self.store.used()[p].with_rendition(to, self.cols));
    }

    /// **Mark screen row `r` as the scroll area's top, or unmark it**,
    /// through [`RowExtent::with_region_top`], which raises the mark to the
    /// live width to mark it ([`RegionTop`]). The mark is stored through
    /// the store, which stamps the row; a row already marked as asked
    /// stores nothing and is not stamped, so the move that marks the top
    /// again on every row change (`Fences::ByRow`, in `baud-core`) moves
    /// a generation only where a fill rebuilt the mark.
    #[cold]
    #[inline(never)]
    pub fn set_region_top(&mut self, r: Row, t: RegionTop) {
        if r.index() >= self.rows.index() {
            return;
        }
        let p = self.phys(r);
        let m = self.store.mark_at(p);
        if m.region_top() != t {
            self.store.mark(p, m.with_region_top(t, self.cols));
        }
    }

    pub fn cols(&self) -> Cols {
        self.cols
    }
    pub fn rows(&self) -> Rows {
        self.rows
    }
    /// # Panics
    /// If `cols` exceeds the pitch, as [`View::new`].
    pub fn set_geometry(&mut self, cols: Cols, rows: Rows) {
        assert!(
            cols.index() <= self.stride().index(),
            "a view wider than its pitch"
        );
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
        self.phys(r) * self.stride().index() + c.index()
    }

    /// One cell, at `(r, c)`: a rectangle's copy or fill, an attribute
    /// changed in place. **A linked cell marks its row** ([`RowLinks`]):
    /// `DECCRA` carries cells, links and all, to rows that held none.
    #[inline(always)]
    pub fn put(&mut self, r: Row, c: Col, cell: PackedCell) {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            let p = self.phys(r);
            let (mut row, mut m) = self.store.row_and_mark(p);
            row[c.index()] = cell;
            *m = m.reached(c.index() + 1).with_links(links_after(*m, cell));
        }
    }

    /// **Row `r` may hold a linked cell** ([`RowLinks`]): what a text write
    /// under an open `OSC 8` link tells the row it lands on.
    pub fn link_row(&mut self, r: Row) {
        if r.index() < self.rows.index() {
            let p = self.phys(r);
            self.store
                .mark(p, self.store.used()[p].with_links(RowLinks::May));
        }
    }

    /// **Every link a cell of this surface may name, marked in `live`**: the
    /// rows [`RowLinks::May`] says may hold one, each to its mark and its
    /// last live cell ([`mark_linked_rows`]). Returns what it read.
    pub fn mark_links(&mut self, live: &mut LinkLive) -> Cells {
        mark_linked_rows(&mut self.store, self.cols.index(), live)
    }
    #[inline(always)]
    pub fn get(&self, r: Row, c: Col) -> PackedCell {
        if r.index() < self.rows.index() && c.index() < self.cols.index() {
            self.store.cells()[self.at(r, c)]
        } else {
            PackedCell::EMPTY
        }
    }
    /// The run under a designated set, each byte through `with_charset`.
    ///
    /// **Out of line, so the ASCII loops' function keeps its shape.** Inline,
    /// the DEC sets' arm took `with_charset` out of line and the call in this
    /// loop made `put_ascii_run`'s prologue save five register pairs on every
    /// run, ASCII ones included: `cat` retired 1.6% more instructions a rep.
    /// Forced inline, `put_ascii_run` still saved four.
    #[inline(never)]
    fn put_designated_run(dst: &mut [PackedCell], src: &[u8], proto: PackedCell, set: Charset) {
        for (d, &b) in dst.iter_mut().zip(src) {
            *d = proto.with_charset(StreamByte::from_stream(b), set);
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
        let cols = self.cols.index();
        let start = at.index();
        let n = src.len().min(cols.saturating_sub(start));
        // **One stamp a run**, before the cut tests and every arm below: the
        // row's cells and its mark, minted together. `ceiling-no-store` keeps
        // it whichever arm runs.
        let (mut row, mut mark) = self.store.row_and_mark(p);
        // **Clear whatever this run is about to orphan**, inline against the
        // row already in hand rather than through `View::clear_partners`.
        // Calling that from `baud-core2`'s `blit` needed a second `self.view()`
        // per run and measured **-6 to -9%** on every payload; here it is two
        // loads and two compares against a pointer that already exists, per
        // ~32-byte run.
        //
        // A run starting on a tail orphans the lead in front of it; a run
        // ending on a lead orphans the tail after it. A run that `Paint::blit`
        // sends here never passes through `put_glyph`, which repairs one
        // character at a time, so without this the common path keeps the bug.
        //
        // The blank is built **inside** the guard. Constructing it
        // unconditionally cost measurably more than the two loads did: a run
        // that cuts nothing -- which is every run in ASCII text -- should pay
        // for the test and not for the repair.
        if n > 0 {
            let end = start + n;
            let cut_lead = start > 0 && row[start].content.is_wide_tail();
            let cut_tail = end < cols && row[end].content.is_wide_tail();
            if cut_lead || cut_tail {
                let blank = PackedCell::empty_with(style);
                if cut_lead {
                    row[start - 1] = blank;
                }
                if cut_tail {
                    row[end] = blank;
                }
            }
        }
        *mark = mark.reached(start + n);
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
            Self::put_designated_run(&mut row[start..start + n], &src[..n], proto, set);
        } else if flags == CellFlags::NONE {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags: CellFlags::NONE,
            };
            Self::put_ascii_vectored(&mut row[start..start + n], &src[..n], proto);
        } else {
            let proto = PackedCell {
                content: Content::EMPTY,
                style,
                flags,
            };
            for (d, &b) in row[start..start + n].iter_mut().zip(&src[..n]) {
                *d = proto.with_ascii(StreamByte::from_stream(b));
            }
        }
    }

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
        let end = x + 2 * n;
        // The orphan rule, once for the whole run rather than once per
        // character: only its two edges can cut a pair. Read through the
        // row minted for the run, so its base is computed once.
        let blank = PackedCell::empty_with(style);
        let (mut row, mut mark) = self.store.row_and_mark(p);
        let (sa, sb) = snap_row(|i| row[i].content.is_wide_tail(), cols, x, end);
        if sa < x {
            row[sa] = blank;
        }
        if sb > end {
            row[sb - 1] = blank;
        }
        let proto = PackedCell::wide_tail(style, flags);
        Self::put_wide_vectored(&mut row[x..end], &cps[..n], proto);
        *mark = mark.reached(sb.max(end));
        RowExtent::of(2 * n)
    }

    /// A run of ordinary **single**-column non-ASCII characters, one cell each.
    ///
    /// The counterpart of `put_ascii_run` for text that is multi-byte but one
    /// column: Latin-1, Greek, Cyrillic, box drawing, and the CJK punctuation
    /// East Asian Width calls Ambiguous. `opcost` priced that shape on
    /// `baud-core2` at **266 MB/s against a wide character's 1155** -- half
    /// the cells for five times the cost, an inversion whose only available
    /// mechanism was that it had no bulk path there and paid `put_cp` per
    /// character.
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
        let end = x + n;
        let blank = PackedCell::empty_with(style);
        let (mut row, mut mark) = self.store.row_and_mark(p);
        let (sa, sb) = snap_row(|i| row[i].content.is_wide_tail(), cols, x, end);
        if sa < x {
            row[sa] = blank;
        }
        if sb > end {
            row[sb - 1] = blank;
        }
        let proto = PackedCell::glyph(Codepoint::REPLACEMENT, style, flags);
        Self::put_narrow_vectored(&mut row[x..end], &cps[..n], proto);
        *mark = mark.reached(sb.max(end));
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

            let (d, d_rest) = dst.as_chunks_mut::<8>();
            let (s, s_rest) = cps.as_chunks::<8>();
            for (dc, sc) in d.iter_mut().zip(s) {
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
                *dc = packed;
            }
            for (dc, &cp) in d_rest.iter_mut().zip(s_rest) {
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

            let (d, d_rest) = dst.as_chunks_mut::<8>();
            let (s, s_rest) = cps.as_chunks::<4>();
            for (dc, sc) in d.iter_mut().zip(s) {
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
                *dc = packed;
            }
            for (dc, &cp) in d_rest.as_chunks_mut::<2>().0.iter_mut().zip(s_rest) {
                dc[0] = PackedCell::glyph(cp, proto.style, proto.flags);
                dc[1] = proto;
            }
        }
        #[cfg(not(target_endian = "little"))]
        for (dc, &cp) in dst.as_chunks_mut::<2>().0.iter_mut().zip(cps) {
            dc[0] = PackedCell::glyph(cp, proto.style, proto.flags);
            dc[1] = proto;
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

            let (d, d_rest) = dst.as_chunks_mut::<8>();
            let (s, s_rest) = src.as_chunks::<8>();
            for (dc, sc) in d.iter_mut().zip(s) {
                let content = Simd::<u8, 8>::from_array(*sc).cast::<u64>();
                let cells = (content | Simd::splat(hi)).to_array();
                let packed = <[PackedCell; 8]>::read_from_bytes(cells.as_bytes())
                    .expect("eight u64 is sixty-four bytes, which is eight cells");
                *dc = packed;
            }
            // A run averages ~32 bytes, so the tail is not rare and is not free.
            for (dc, &b) in d_rest.iter_mut().zip(s_rest) {
                *dc = proto.with_ascii(StreamByte::from_stream(b));
            }
        }
        #[cfg(all(not(target_endian = "little"), not(feature = "ceiling-no-store")))]
        for (dc, &b) in dst.iter_mut().zip(src) {
            *dc = proto.with_ascii(StreamByte::from_stream(b));
        }
    }

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
        let cols = self.cols.index();
        let (a, b) = (from.index().min(cols), to.index().min(cols));
        let mut row = self.store.row_at(p);
        let (sa, sb) = snap_row(
            |i| row.get(i).content.is_wide_tail(),
            cols,
            from.index(),
            to.index(),
        );
        if sa < a {
            row.set(sa, blank);
        }
        if sb > b {
            row.set(sb - 1, blank);
            // **Past the mark, so the mark must move.** Everything beyond the
            // extent has to stay equal to the blank last written across the
            // row, or the O(1) "has the template changed" test that reads the
            // *last* cell stops implying anything about the cells before it.
            let mut mark = row.mark();
            *mark = mark.reached(sb);
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
        // One stamp, before the stores and kept by `ceiling-no-store`; the
        // row's base once, and each store one compare against the cells.
        let mut row = self.store.row_at(p);
        #[cfg(feature = "ceiling-no-store")]
        let _ = (lead, &mut row);
        // **Two stores, and combining them lost.** A lead and its `WIDE_TAIL`
        // are sixteen contiguous bytes, so one wide store should serve --
        // `copy_from_slice(&[lead, t])` measured **662-690 against 690-709**,
        // because the pair is built on the stack first and that costs more
        // than the store it saves. This is the dominant write on CJK text
        // (69% of runs are one character, so the vectored run writer never
        // sees them), which is why it was worth trying and worth recording.
        #[cfg(not(feature = "ceiling-no-store"))]
        {
            row.set(x, lead);
        }
        let end = match tail {
            Some(_t) if x + 1 < cols => {
                #[cfg(not(feature = "ceiling-no-store"))]
                {
                    row.set(x + 1, _t);
                }
                x + 2
            }
            _ => x + 1,
        };
        let mut mark = row.mark();
        *mark = mark.reached(end);
        #[cfg(feature = "probe-double-extent")]
        {
            *mark = mark.reached(end);
        }
    }

    /// Write one glyph, and a wide character's tail, in **one** pass.
    ///
    /// This was `clear_partners`, then `put`, then `put` again: three `phys`
    /// computations, two bounds checks and three read-modify-writes of the
    /// used extent, for a character that occupies two cells. On `baud-core2`
    /// it ran once per **non-ASCII codepoint** -- ASCII went through
    /// `put_ascii_run` -- so it was most of what the CJK path was made of, and
    /// `cat-cjk.raw` was where baud was furthest behind another
    /// implementation. In this core `Paint::glyph`, and `Paint::indexed`
    /// under the `cell-table` feature, call it for a character landing before
    /// the row's written mark, and `put_glyph_fresh` past it; that character
    /// may be ASCII in a run of one or two bytes, and a multi-byte run that
    /// `put_run` admits, of one width and fitting the row, goes to
    /// `put_wide_run` or `put_narrow_run` instead. `Paint::widen` calls it too.
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
        let end = match tail {
            Some(_) => (x + 2).min(cols),
            None => x + 1,
        };
        let mut row = self.store.row_at(p);
        let (sa, sb) = snap_row(|i| row.get(i).content.is_wide_tail(), cols, x, end);
        if sa < x {
            row.set(sa, blank);
        }
        if sb > end {
            row.set(sb - 1, blank);
        }
        row.set(x, lead);
        if let Some(t) = tail {
            if x + 1 < cols {
                row.set(x + 1, t);
            }
        }
        // One extent update covering the glyph and whatever the repair touched.
        let mut mark = row.mark();
        *mark = mark.reached(sb.max(end));
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
    fn snap(&self, r: Row, from: Col, to: Col) -> (usize, usize) {
        let row = self.store.row(self.phys(r));
        snap_row(
            |i| row[i].content.is_wide_tail(),
            self.cols.index(),
            from.index(),
            to.index(),
        )
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
            true => self.store.cells()[self.phys(r) * self.stride().index() + c.col().index()],
            false => PackedCell::EMPTY,
        }
    }

    /// Point a cell at an interned cluster: its content becomes the slot's
    /// word ([`Content::cluster`]), whose slot holds the base the content
    /// held, and its flags take [`CellFlags::MARKED`].
    ///
    /// **And the extent moves with it.** `RowExtent` records where the row
    /// stops being *uniform*, not where the glyphs are, and a cell carrying
    /// `MARKED` is not equal to the blank in front of it. Setting the bit
    /// without raising the mark left the row non-uniform beyond its own
    /// extent, so a later erase skipped the cell: `CSI ? 1 J` cleared a mark
    /// that `CSI 1 J` left alone, which core2's DECSED-subset property
    /// reported -- the defect `ECH` at the last column had too: an operation
    /// that makes a cell differ from the blank must say so.
    ///
    /// **And the row is stamped whether or not the mark moves**: the base was
    /// written to this cell, usually in an earlier frame, so the mark is
    /// almost always past it already, and a renderer that kept the row would
    /// draw the base without its mark.
    pub fn set_cluster(&mut self, r: Row, c: LeadCol, id: ClusterId) {
        if r.index() >= self.rows.index() || c.col().index() >= self.cols.index() {
            return;
        }
        let (p, x) = (self.phys(r), c.col().index());
        let mut row = self.store.row_at(p);
        let mut cell = row.get(x);
        cell.content = Content::cluster(id);
        cell.flags = cell.flags.with(CellFlags::MARKED);
        row.set(x, cell);
        let mut mark = row.mark();
        *mark = mark.reached(x + 1);
    }

    /// Give a U+10EEEE cell its diacritics: its content becomes the
    /// placeholder word `word` ([`Content::placeholder`]) and its flags take
    /// [`CellFlags::MARKED`], as [`View::set_cluster`] does for a slot, and
    /// for the same reason it raises `RowExtent`.
    pub fn set_placeholder(&mut self, r: Row, c: LeadCol, word: Content) {
        if r.index() >= self.rows.index() || c.col().index() >= self.cols.index() {
            return;
        }
        let (p, x) = (self.phys(r), c.col().index());
        let mut row = self.store.row_at(p);
        let mut cell = row.get(x);
        cell.content = word;
        cell.flags = cell.flags.with(CellFlags::MARKED);
        row.set(x, cell);
        let mut mark = row.mark();
        *mark = mark.reached(x + 1);
    }

    /// The pitch: cells from one row of the buffer to the next.
    ///
    /// **The store's, and only the store's.** The view carried a copy beside
    /// the one its [`Stamped`] cuts rows by, and the per-byte writers loaded
    /// both and computed a row's base twice: once to snap the run's edges,
    /// once to mint the row ([`Stamped::row_and_mark`]).
    #[inline(always)]
    pub fn stride(&self) -> Stride {
        self.store.stride()
    }

    /// Every cell of this surface's buffer -- the screen, the history behind
    /// it and the rows held past a narrower edge -- for the cluster store's
    /// sweep, which must see every cell that can name a slot.
    pub fn ring_cells(&self) -> &[PackedCell] {
        self.store.cells()
    }

    /// The surface's memory, to read.
    pub fn store(&self) -> &Stamped<'a> {
        &self.store
    }

    /// **The surface's memory, to rewrite**: a freeze moving rows off the
    /// active screen into a generation, and a thaw bringing them back
    /// (`baud-core`'s `freeze.rs`). Every way in through it stamps.
    pub fn store_mut(&mut self) -> &mut Stamped<'a> {
        &mut self.store
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

    /// **Guard the positions `[from, to)` of a row**, widened to whole
    /// characters: EPA's mark (`[[source: ecma48]]` 6.5.2.2). Each cell keeps
    /// what it holds and takes [`CellFlags::GUARDED`], and **the row's mark
    /// rises over every position marked**, so a row a scroll later recycles
    /// is blanked over them rather than to an extent that stops short and
    /// leaves the guard on the line coming in. Returns the cells marked.
    pub fn guard_span(&mut self, r: Row, from: Col, to: Col) -> usize {
        if r.index() >= self.rows.index() {
            return 0;
        }
        let (a, b) = self.snap(r, from, to);
        if a >= b {
            return 0;
        }
        let p = self.phys(r);
        let (mut row, mut mark) = self.store.row_and_mark(p);
        for c in &mut row[a..b] {
            c.flags = c.flags.with(CellFlags::GUARDED);
        }
        *mark = mark.reached(b);
        b - a
    }

    /// **ED, EL and ECH over `[from, to)` of a row while a guarded area may
    /// exist**: the runs of unguarded cells are filled with `blank`, and the
    /// runs of guarded ones are left (`GuardedErasure::Protect`) or filled
    /// with `blank` keeping the guard (`GuardedErasure::All`), each run
    /// through the fill every erase takes, so the row's mark follows the same
    /// rule. The span
    /// is widened to whole characters once; EPA guards whole characters, so
    /// no run inside it splits one. Returns the cells written.
    pub fn fill_sparing(
        &mut self,
        r: Row,
        from: Col,
        to: Col,
        blank: PackedCell,
        how: GuardedErasure,
    ) -> usize {
        if r.index() >= self.rows.index() {
            return 0;
        }
        let (a, b) = self.snap(r, from, to);
        let base = self.at(r, Col::ZERO);
        let guarded = PackedCell {
            flags: blank.flags.with(CellFlags::GUARDED),
            ..blank
        };
        let (mut n, mut x) = (0, a);
        while x < b {
            let g = self.store.cells()[base + x].flags.is_guarded();
            let mut y = x + 1;
            while y < b && self.store.cells()[base + y].flags.is_guarded() == g {
                y += 1;
            }
            n += match (g, how) {
                (false, _) => self.fill_raw(r, x, y, blank),
                (true, GuardedErasure::Protect) => 0,
                (true, GuardedErasure::All) => self.fill_raw(r, x, y, guarded),
            };
            x = y;
        }
        n
    }

    /// The unsnapped fill, for spans already known to be whole rows.
    ///
    /// **Inlined into each caller**: with [`View::fill_sparing`] calling it
    /// too, LLVM took it out of line, and every ED and EL paid a call that
    /// `fill` had not made.
    #[inline(always)]
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
        // A span is erased or filled with a character no link names
        // (`DECFRA` drops it), so its row's `RowLinks` stands.
        debug_assert!(cell.flags.link().is_none());
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
        if to_end && self.store.mark_at(p).index() > cols {
            return self.fill_past_edge(p, a, cell);
        }
        let (mut row, mut mark) = self.store.row_and_mark(p);
        let b = match to_end && row[cols - 1] == cell {
            true => mark.index().max(a).min(b),
            false => b,
        };
        // From column 0 to the end the row is erased whole, and DECRLM's
        // mark goes with what it wrote ([`RowExtent::erased`]).
        match to_end {
            true if a == 0 => *mark = mark.refilled(0).erased(),
            true => *mark = mark.refilled(a),
            false => *mark = mark.reached(b),
        }
        if a >= b {
            return 0;
        }
        // `slice::fill`, not an indexed loop: one bounds check instead of one
        // per cell, and it lowers to a vector store rather than a scalar one.
        row[a..b].fill(cell);
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
    ///
    /// **The row erased in place** (ED, DECALN): the same paragraph, its
    /// path kept, only DECRLM's mark gone ([`RowExtent::erased`]).
    #[inline(always)]
    pub fn fill_row(&mut self, r: Row, cell: PackedCell) -> RowExtent {
        self.fill_row_as(r, cell, RowFill::Erased)
    }

    /// [`View::fill_row`] for **a line a full-screen scroll brings in**: a
    /// new paragraph, which keeps nothing of the ring row it takes and has
    /// no properties of its own until the core's line rules give it the
    /// direction in force. It stores the mark and reads nothing of the row,
    /// about 0.15% more instructions on cat than a fill that keeps the row's
    /// properties; clearing DECRLM's bit in the row kept read 0.4% more on
    /// cat and 0.3% on less.
    #[inline(always)]
    pub fn fill_new_row(&mut self, r: Row, cell: PackedCell) -> RowExtent {
        self.fill_row_as(r, cell, RowFill::New)
    }

    #[inline(always)]
    fn fill_row_as(&mut self, r: Row, cell: PackedCell, fill: RowFill) -> RowExtent {
        let cols = self.cols.index();
        if cols == 0 || r.index() >= self.rows.index() {
            return RowExtent::ZERO;
        }
        let p = self.phys(r);
        let blanking = cell.content == Content::EMPTY;
        // Through the mark when it lies past the live edge: see `fill_raw`.
        if self.store.mark_at(p).index() > cols {
            let low = match blanking {
                true => RowExtent::ZERO,
                false => RowExtent::of(cols),
            };
            return RowExtent::of(self.fill_past_edge_row(p, cell, low));
        }
        // One stamp, before the store and kept by `ceiling-no-rowfill`.
        let (mut row, mut m) = self.store.row_and_mark(p);
        let to = match blanking && row[cols - 1] == cell {
            true => m.index().min(cols),
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
        row[..to].fill(cell);
        #[cfg(feature = "ceiling-no-rowfill")]
        let _ = &mut row;
        let mark = match blanking {
            true => 0,
            false => cols,
        };
        // A blank names no link, so a row erased whole holds none; `DECALN`
        // under an open link writes one into every cell.
        debug_assert!(fill == RowFill::Erased || cell.flags.link().is_none());
        *m = match fill {
            RowFill::Erased => {
                let e = m.refilled(mark).erased();
                e.with_links(match blanking {
                    true => RowLinks::None,
                    false => links_after(e, cell),
                })
            }
            RowFill::New => RowExtent::of(mark),
        };
        RowExtent::of(to)
    }

    /// Whether a screen row holds nothing within the live width: every
    /// cell's content is [`Content::EMPTY`]. Content, not the mark: a row
    /// written wide and then narrowed keeps a mark past the live edge,
    /// and the mark a blanking fill leaves there is about what lies past
    /// the edge, not about whether the row shows anything. The question
    /// a retreat asks -- may this row leave the screen -- is about what
    /// it shows. [`View::ring_row_holds`] answers it, the one test the
    /// retreat and a growth's reveal share.
    pub fn row_is_blank(&self, r: Row) -> bool {
        r.index() < self.rows.index()
            && self.ring_row_holds(RowRot(self.phys(r)), self.cols) == Holds::Nothing
    }

    /// **Whether the ring row `at` holds a written character** in its first
    /// `width` columns: any cell whose content is not [`Content::EMPTY`],
    /// a written space included, since U+0020 is a character written and
    /// an erase leaves `EMPTY` in whatever colour it erased in. Colour alone
    /// is not text: the renderer decides how an empty cell looks, so a row
    /// erased in a background holds nothing here. `width` is clamped to the
    /// pitch.
    pub fn ring_row_holds(&self, at: RowRot, width: Cols) -> Holds {
        let w = width.index().min(self.stride().index());
        let a = (at.index() % self.ring.index()) * self.stride().index();
        match self.store.cells()[a..a + w]
            .iter()
            .any(|c| c.content != Content::EMPTY)
        {
            true => Holds::Text,
            false => Holds::Nothing,
        }
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

    /// Blank one physical row of the ring -- wherever the window is -- as
    /// a whole-row erase in `cell` leaves it. `at` is the ring row as
    /// [`RowRot`] spells it: `phys(y)` for a screen row is
    /// `rot.advance(y, ring)`, and a row behind the window is
    /// `rot.retreat(1 + back, ring)`. `cell` is a blank.
    ///
    /// For a block of rows the terminal knows the ring extent of and must
    /// erase whether they are on the screen, behind it or, after the
    /// window moved, hidden past it: the prompt redraw, which stands in
    /// for the shell's `ED` over its block. So the row ends as `ED` leaves
    /// a screen row ([`View::fill`] from column 0 to the live edge): every
    /// cell to the edge is `cell`, stopping at the mark only when the last
    /// cell already is, and on a row whose mark lies past the edge the
    /// erase runs on to the mark and keeps it there unless `cell` is the
    /// default ([`View::mark_after_fill`]). Filling to the mark alone and
    /// marking the row empty left the old blank between the mark and the
    /// edge, and took a coloured erase's cells past the edge on a growth.
    pub fn fill_at(&mut self, at: RowRot, cell: PackedCell) -> RowExtent {
        let cols = self.cols.index();
        if cols == 0 {
            return RowExtent::ZERO;
        }
        let p = at.index();
        if self.store.mark_at(p).index() > cols {
            return RowExtent::of(self.fill_past_edge_row(p, cell, RowExtent::ZERO));
        }
        let (mut row, mut m) = self.store.row_and_mark(p);
        let to = match row[cols - 1] == cell {
            true => m.index().min(cols),
            false => cols,
        };
        #[cfg(not(feature = "ceiling-no-rowfill"))]
        row[..to].fill(cell);
        #[cfg(feature = "ceiling-no-rowfill")]
        let _ = &mut row;
        *m = m.refilled(0).erased();
        RowExtent::of(to)
    }

    /// A whole-row fill on a row whose mark lies past the live edge: the
    /// fill runs to the mark, and the mark stays there unless the row is
    /// the default blank from `low` on ([`View::mark_after_fill`]).
    /// `fill_row`'s cold half, and `fill_at`'s.
    #[cold]
    #[inline(never)]
    fn fill_past_edge_row(&mut self, p: usize, cell: PackedCell, low: RowExtent) -> usize {
        let base = p * self.stride().index();
        let was = self.store.mark_at(p);
        let to = was.index();
        // The mark reads the cell at `to`, which the fill does not reach.
        let mark = self
            .mark_after_fill(base, to, cell, low)
            .with_bidi(was.bidi())
            .with_links(links_after(was, cell))
            .erased();
        let (mut row, mut m) = self.store.row_and_mark(p);
        #[cfg(not(feature = "ceiling-no-rowfill"))]
        row[..to].fill(cell);
        #[cfg(feature = "ceiling-no-rowfill")]
        let _ = &mut row;
        *m = mark;
        to
    }
    /// Shift a row's cells left by `n`, blanking the tail. `CSI P`.
    pub fn delete_chars(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        // A shift rewrites to the end of the row, so the mark goes there too.
        // Tighter bookkeeping is possible -- a left shift can only shrink the
        // used extent -- but shrinking a high-water mark is the direction that
        // loses text, and this operation is rare next to blanking.
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        // The deleted span is what must cover whole characters: shifting from
        // the middle of a pair drags a lone tail leftwards into a cell with no
        // lead in front of it.
        let (cx, n) = {
            let (a, b) = self.snap(r, Col::at_index(cx), Col::at_index(cx + n));
            (a, b - a)
        };
        let (mut row, mut mark) = self.store.row_and_mark(self.phys(r));
        *mark = mark.reached(cols);
        for x in cx..cols - n {
            row[x] = row[x + n];
        }
        for x in cols - n..cols {
            row[x] = blank;
        }
    }
    /// Shift right, inserting blanks. `CSI @`.
    pub fn insert_blanks(&mut self, r: Row, at: Col, n: Count, blank: PackedCell) {
        let cols = self.cols.index();
        // A shift rewrites to the end of the row, so the mark goes there too.
        // Tighter bookkeeping is possible -- a left shift can only shrink the
        // used extent -- but shrinking a high-water mark is the direction that
        // loses text, and this operation is rare next to blanking.
        let (cx, n) = (at.index(), (n.get_raw() as usize).min(cols - at.index()));
        // Inserting *at* a tail would put blanks between a lead and its
        // partner. `[[source: ghostty-screen]]`'s `insertBlanks` erases the
        // character in that case; snapping the start back to the lead moves the
        // whole pair instead, which keeps the text rather than deleting it and
        // leaves no orphan either way.
        let cx = self.snap(r, Col::at_index(cx), Col::at_index(cx + 1)).0;
        let n = n.min(cols - cx);
        let (mut row, mut mark) = self.store.row_and_mark(self.phys(r));
        *mark = mark.reached(cols);
        let mut x = cols;
        while x > cx + n {
            x -= 1;
            row[x] = row[x - n];
        }
        for x in cx..cx + n {
            row[x] = blank;
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
        let (mut row, mut mark) = self.store.row_and_mark(self.phys(r));
        *mark = mark.reached(end);
        for x in cx..end - n {
            row[x] = row[x + n];
        }
        for x in end - n..end {
            row[x] = blank;
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
        // Inserting *at* a tail would put blanks between a lead and its
        // partner. Snapping the start back to the lead moves the whole pair
        // instead, which keeps the character rather than deleting it and
        // leaves no orphan either way.
        let cx = self.snap(r, Col::at_index(cx), Col::at_index(cx + 1)).0;
        let n = (n.get_raw() as usize).min(end - cx);
        let (mut row, mut mark) = self.store.row_and_mark(self.phys(r));
        *mark = mark.reached(end);
        // **The character shifted off the right edge takes its partner with
        // it.** The cell at `end - n` is pushed past `end` and lost; if it is a
        // tail, its lead lands at `end - 1` with nothing after it.
        // `[[source: ghostty-screen]]`'s `insertBlanks` clears that cell for
        // the same reason. Read before the shift, because the shift overwrites
        // the evidence.
        let orphans_at_edge = end > n && n > 0 && row[end - n].content.is_wide_tail();
        let mut x = end;
        while x > cx + n {
            x -= 1;
            row[x] = row[x - n];
        }
        for x in cx..cx + n {
            row[x] = blank;
        }
        if orphans_at_edge {
            row[end - 1] = blank;
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
        let cols = self.cols.index();
        let mut y = bot + 1;
        while y > cy + n {
            y -= 1;
            let (ps, pd) = (
                self.phys(Row::from_index(y - n)),
                self.phys(Row::from_index(y)),
            );
            // `split_at_mut` and `copy_from_slice`, not `copy_within`. Two
            // distinct rows never overlap, and saying so gets a `memcpy`;
            // `copy_within` must promise overlap-safety and pays for a
            // `memmove` -- measured 27.0/27.3/27.2 MB/s against 27.8/27.7/27.9
            // on `less-pager.raw`, slower in all three pairs.
            //
            // **The rotation is why the order is tested.** `phys` maps screen
            // rows onto physical ones, so `y - n` can sit *after* `y` in the
            // buffer, and a split taken on the wrong side panics.
            // [`Stamped::from_into`] takes the split on the right side and
            // stamps the destination once, for its cells and its mark.
            let (src, from, mut row, mut m) = self.store.from_into(ps, pd);
            row[..cols].copy_from_slice(&src[..cols]);
            // **The mark travels with the row**, in the same loop and under
            // the same `phys`. Kept here rather than in a second pass because a
            // second pass over the same indices is a second chance to compute
            // them differently -- and a mark left behind on a moved row is
            // stale text, which is the failure this whole mechanism must not
            // have.
            *m = from;
        }
        for y in cy..cy + n {
            let p = self.phys(Row::from_index(y));
            let (mut row, mut m) = self.store.row_and_mark(p);
            row[..cols].fill(blank);
            *m = m
                .refilled(match blank.content == Content::EMPTY {
                    true => 0,
                    false => cols,
                })
                .erased();
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
        let cols = self.cols.index();
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
            let (src, from, mut row, mut m) = self.store.from_into(ps, pd);
            row[..cols].copy_from_slice(&src[..cols]);
            *m = from;
        }
        for y in bot + 1 - n..=bot {
            let p = self.phys(Row::from_index(y));
            let (mut row, mut m) = self.store.row_and_mark(p);
            row[..cols].fill(blank);
            *m = m
                .refilled(match blank.content == Content::EMPTY {
                    true => 0,
                    false => cols,
                })
                .erased();
        }
    }
    pub fn row_slice(&self, r: Row) -> &[PackedCell] {
        let a = self.at(r, Col::ZERO);
        &self.store.cells()[a..a + self.cols.index()]
    }
    /// **Scroll the rectangle** rows `top..=bottom`, columns `left..right`,
    /// `n` rows `way`, blanking the `n` rows it opens, and leave every cell
    /// outside it where it is. Side margins make the scrolling region this
    /// rectangle: `[[source: dec-std-070]]` § 5.4.3, "All scrolling, both
    /// vertical and horizontal, is limited to this area", and its
    /// `scroll_up` and `scroll_down` copy from the left margin to the right
    /// only (§ 5.14.2-3). Returns the cells written.
    ///
    /// **A wide character a side of the rectangle cuts is erased first, both
    /// halves, on every row in it** -- the rows that move and the rows that
    /// are written into alike -- since moving one half would leave the other
    /// an orphan. No text settles it; it is [`View::snap`]'s rule, and what
    /// xterm's `scrollInMargins` (`[[source: xterm-scroll]]`) and ghostty do.
    ///
    /// **Each row in it is marked to `right` (to `right + 1` where an erase
    /// reached past it)**: cells copied or blanked inside the rectangle can
    /// differ from what the row was uniform with past its mark, and the mark
    /// only rises, the safe direction. **A row that wrapped at the
    /// rectangle's own side margins moves its wrap and its margins with its
    /// cells** ([`RowExtent::margins`]): its paragraph is the cells inside
    /// them, which are what move. A row the rectangle writes over that held
    /// such a paragraph, and one it blanks, ends and keeps none. Every
    /// other row keeps its wrap state: a row whose middle moved is still
    /// the line it was.
    ///
    /// Rows are distinct, so each copy is of one row's span into another's,
    /// in the order that reads a row before it is overwritten. Cold: only a
    /// scroll under side margins comes here.
    #[allow(clippy::too_many_arguments)]
    pub fn scroll_rect(
        &mut self,
        top: Row,
        bottom: Row,
        left: Col,
        right: Col,
        n: Count,
        way: BandMove,
        blank: PackedCell,
    ) -> usize {
        let cols = self.cols.index();
        let (cy, bot) = (top.index(), bottom.index().min(self.rows.index() - 1));
        let (l, r) = (left.index(), right.index().min(cols));
        if cy > bot || l >= r {
            return 0;
        }
        let n = (n.get_raw() as usize).min(bot + 1 - cy);
        let mut written = 0;
        for y in cy..=bot {
            let p = self.phys(Row::from_index(y));
            let (mut cells, mut mark) = self.store.row_and_mark(p);
            let mut far = r;
            if l > 0 && cells[l].content.is_wide_tail() {
                cells[l - 1] = blank;
                cells[l] = blank;
                written += 2;
            }
            if r < cols && cells[r].content.is_wide_tail() {
                cells[r - 1] = blank;
                cells[r] = blank;
                written += 2;
                far = r + 1;
            }
            *mark = mark.reached(far);
        }
        let w = r - l;
        let inside = crate::WrapMargins::of(left, Col::at_index(r - 1));
        let ours = move |e: RowExtent| e.margins() == Some(inside);
        let copy = |v: &mut Self, from: usize, to: usize| {
            let (ps, pd) = (v.phys(Row::from_index(from)), v.phys(Row::from_index(to)));
            let (src, dst) = (v.store.mark_at(ps), v.store.mark_at(pd));
            v.store.copy_within_rows(ps, l..l + w, pd, l);
            let moved = match (ours(src), ours(dst)) {
                (true, _) => dst.wrapping(cols).with_margins(inside),
                (false, true) => dst.ended().at_edge(),
                (false, false) => dst,
            };
            // The band's cells carry their links to the row they land on.
            v.store.mark(
                pd,
                match src.links() {
                    RowLinks::May => moved.with_links(RowLinks::May),
                    RowLinks::None => moved,
                },
            );
        };
        let blank_row = |v: &mut Self, y: usize| {
            let p = v.phys(Row::from_index(y));
            let (mut cells, mut m) = v.store.row_and_mark(p);
            cells[l..l + w].fill(blank);
            if ours(*m) {
                *m = m.ended().at_edge();
            }
        };
        match way {
            BandMove::Up => {
                for y in cy..bot + 1 - n {
                    copy(self, y + n, y);
                }
                for y in bot + 1 - n..=bot {
                    blank_row(self, y);
                }
            }
            BandMove::Down => {
                for y in (cy + n..=bot).rev() {
                    copy(self, y - n, y);
                }
                for y in cy..cy + n {
                    blank_row(self, y);
                }
            }
        }
        written + (bot + 1 - cy) * w
    }
}

/// **The boundary rule itself** ([`View::snap`]), against a row the caller
/// already holds: `[a, b)` clamped to `cols` and widened to whole
/// characters.
///
/// Split out so `clear_partners` -- which runs once per non-ASCII
/// codepoint -- computes `phys` **once**. Going through `snap` cost three
/// per call there (`snap`'s, `at`'s and `reached`'s), and it profiled at
/// 7% of the CJK path for two loads and two compares of actual work.
///
/// **Over the row the writer has already minted**, not over the store: the
/// per-byte writers take their [`RowMut`] or [`RowWrite`] first and snap
/// through it, so a row's base is computed once and the store's cells are
/// not loaded a second time to read two edges of a row already in hand.
/// `tail(i)` says whether column `i` of that row is a wide tail: one rule
/// for the cut row and the token alike.
///
/// No `#[inline(always)]`: it carried one, stranded for a long time onto
/// `lead` by an insertion between this comment and the signature, and
/// re-attaching it measured cat-cjk 0.98x, disjoint from an A/A copy.
/// LLVM inlines this private function on its own judgement, and the
/// forced form is the one that costs.
fn snap_row(tail: impl Fn(usize) -> bool, cols: usize, a: usize, b: usize) -> (usize, usize) {
    let (mut a, mut b) = (a.min(cols), b.min(cols));
    if a >= b {
        return (a, b);
    }
    if a > 0 && tail(a) {
        a -= 1;
    }
    if b < cols && tail(b) {
        b += 1;
    }
    (a, b)
}

/// The [`RowLinks`] a row has once `cell` is written into it: `May` for a
/// linked cell, else what `was` said.
#[inline(always)]
fn links_after(was: RowExtent, cell: PackedCell) -> RowLinks {
    match cell.flags.link().is_none() {
        true => was.links(),
        false => RowLinks::May,
    }
}

/// **Every link the cells of `store` may name, marked in `live`**: one row a
/// [`RowExtent`], at the store's pitch, `cols` of them live.
///
/// **Only the rows [`RowLinks::May`] says may hold one, and each only to
/// its mark** ([`RowExtent::index`]) **and its last live cell.** A linked
/// cell is written, and a write raises the mark over it; past the mark the
/// row is uniform to the live edge -- every cell its last -- so the last
/// live cell stands for the rest. Past the live edge a row of the primary
/// holds what a narrower screen cut off, which the caller reads
/// (`baud-core`'s `links.rs`), and the alternate screen's is blanked
/// before a growth shows it ([`View::widened`]). A row read and found to
/// name none is marked [`RowLinks::None`], so a row that held a link once
/// costs one read and no more, and only such a row is stamped: the mark
/// changes and no cell does. Returns the cells read, every row's mark
/// counted as one.
///
/// No program the tests run leaves a linked cell past a row's mark, so the
/// read of the last live cell is kept as the safe direction, and no test
/// yet fails without it.
pub fn mark_linked_rows(store: &mut Stamped<'_>, cols: usize, live: &mut LinkLive) -> Cells {
    let stride = store.stride().index();
    let rows = store.rows().min(store.cells().len() / stride.max(1));
    let mut read = rows;
    for p in 0..rows {
        let e = store.mark_at(p);
        if e.links() == RowLinks::None {
            continue;
        }
        let base = p * stride;
        let to = e.index().min(stride);
        let cells = store.cells();
        let mut named = live.mark(&cells[base..base + to]);
        read += to;
        let last = cols.min(stride);
        if to < last {
            named |= live.mark(&cells[base + last - 1..base + last]);
            read += 1;
        }
        if !named {
            store.mark(p, e.with_links(RowLinks::None));
        }
    }
    Cells::of(read)
}
