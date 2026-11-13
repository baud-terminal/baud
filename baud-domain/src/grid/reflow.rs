//! **A change of width re-cuts the primary's lines in place, once.**
//!
//! A line is the rows the wrap flag joins -- every row but the last carries
//! [`RowExtent::wrapped`] -- and its cells are the full rows followed by
//! the last row to its mark. At the new width the same cells fall into a
//! different number of rows, and a wide character that would straddle the
//! new edge leaves a blank and moves whole to the next row, which is what a
//! writer does at the edge. Every design in the field copies here, and
//! ghostty's `ReflowCursor` says why it copies into *fresh* pages: an
//! in-place reflow can overwrite a source row it has not read yet. This one
//! is in place anyway, because baud has one ring and no allocator, and the
//! order is what makes it sound:
//!
//! * **Widening walks oldest to newest, cells ascending.** A destination
//!   row holds at least as many real cells as a source row (the new width
//!   less at most one spacer is still at least the old width), so cell `k`
//!   lands at or before where it was read from, and a write can only cover
//!   a cell already consumed.
//! * **Narrowing walks newest to oldest, cells descending, with the
//!   destination end advanced by the total growth.** A destination row
//!   holds fewer cells than a source row, so cell `k` lands at or after
//!   where it was read from; walking down, a write can only cover a cell
//!   already consumed. The rows past the source's newest end that the
//!   destination spills into are the ring's free rows and then, wrapping,
//!   the oldest rows -- which are exactly the lines dropped when the
//!   re-cut history does not fit, chosen whole from the oldest end so the
//!   oldest retained row is always a line's head.
//!
//! One counting pass over the marks first (and one cell read per
//! destination row, for the spacers), then one copy of every cell. The
//! window keeps its top row where the row landed, slides down only as far
//! as keeping the cursor on the screen needs -- so the blank rows under a
//! prompt absorb a re-cut before any line leaves the top -- and when lines
//! join it pulls lines back from the history so the screen stays full.
//!
//! **Rows a caller names are kept whole** ([`Recut::Above`]): from that
//! row on, every row is a line of its own, one row, copied whole at the
//! pitch with its mark -- cells past the new width held there, unseen until
//! a growth shows them -- as a line not single-width is kept. What a resize
//! at a shell's prompt keeps: the rows the shell redraws by counting them.
//! A kept row is the copy of a source row onto one destination row, so the
//! order above holds for it as it holds for a line of one row: widening
//! writes at or before the row it read, narrowing at or after it.

use super::{RowRot, View};
use crate::PackedCell;
use crate::{Col, Cols, Row, Rows};
use crate::{LineBidi, LineRendition, RowExtent};

/// **Which of the lines a reflow re-cuts**: every one, or only those
/// before a row, counted from the oldest retained row, from which every
/// row is kept whole -- the rows a shell at its prompt redraws by counting
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recut {
    All,
    Above(Rows),
}

/// Where the window and the cursor landed after a reflow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reflowed {
    pub rot: RowRot,
    pub hist: Rows,
    pub hidden: Rows,
    pub cursor: (Row, Col),
    /// Rows dropped from the oldest end because the re-cut history did not
    /// fit: the new oldest retained row is this many rows after the old one.
    pub dropped: Rows,
    /// **The window's last rows that no re-cut line reached**, blanked to
    /// the new width and marked empty, and left as they were past it. Every
    /// other row the re-cut writes is blank past the new width to the pitch
    /// (`finish_line`); these are not, so a row here may hold an old line's
    /// cells from the new width on, under a mark of zero that says nothing
    /// of them. What the resize's measure has to read whole
    /// (`View::held_past_edge`).
    pub blanked: Rows,
    /// **The rows kept whole** ([`Recut::Above`]): the first one's ring row
    /// and how many, the last rows the re-cut wrote. Each may hold cells
    /// past the new width, as the rows in `blanked` may, and lands on a ring
    /// row whose flag (`PastEdge`) was another row's.
    pub kept: (RowRot, Rows),
}

/// **A hook a reflow calls once for every line it keeps, before copying
/// it**: the line's first row and how many rows it has, both counted from
/// the oldest retained row, and where a cell of it lands -- asked by its
/// row, counted the same way, and its column in that row; answered as the
/// row and the column it lands at. Before the copy, because narrowing copies
/// downwards over rows it has read, and where a cell lands is computed from
/// the cells before it.
///
/// What lets a thing anchored to a row -- an image placement -- follow the
/// row through the re-cut rather than keep a number that now names another.
/// A row lands where **its first cell** lands -- foot's rule
/// (`[[source: foot-grid-c-1-21]]`, `grid_resize_and_reflow`) -- and the
/// anchor keeps its own column, so a column past the new edge is cropped
/// where it is drawn rather than moved. The first cell is the one the row
/// began with where the anchor was placed, not whichever cell a narrower
/// re-cut put first on the row it landed on: an anchor on a continuation row
/// lands on a narrow row that begins earlier in the line, and asked by that
/// row's own first cell a widen brought it back a row higher. So the hook
/// answers the column too, and an anchor that keeps it asks by it next time.
pub type Follow<'f> = &'f mut dyn FnMut(Rows, Rows, &dyn Fn(Rows, Col) -> (Rows, Col));

/// The re-cut: where the oldest retained row is, and the two widths.
#[derive(Clone, Copy)]
struct Cut {
    /// The physical row of linear index 0, the oldest retained row.
    base: usize,
    old: usize,
    new: usize,
    /// The first row kept whole, counted from the oldest retained row;
    /// past every row when every line is re-cut.
    keep: usize,
}

/// How a line is copied: re-cut at the new width, or kept as it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shape {
    /// Re-cut: its cells fall into rows of the new width.
    Cut,
    /// Not single-width: one row, its cells where they were.
    Sized,
    /// At or past the first row kept whole: one row, copied whole.
    Kept,
}

/// One line of the source and its shape at the new width.
#[derive(Clone, Copy)]
struct Line {
    /// Its first row's linear index, from the oldest retained row.
    start: usize,
    /// Source rows.
    rows: usize,
    /// Cells: the full rows, then the last to its mark.
    cells: usize,
    /// Blanks left where a wide character could not take the last column.
    spacers: usize,
    /// Destination rows: the cells and the spacers at the new width.
    dest: usize,
    /// The blank the last row is uniform in past its mark.
    tail: PackedCell,
    /// Its rendition: any but single-width is a line of one row, kept whole.
    rendition: LineRendition,
    /// Its first row's paragraph properties, which every row it is cut
    /// into takes: a paragraph's direction is its own, wherever it wraps.
    bidi: LineBidi,
    shape: Shape,
}

impl<'a> View<'a> {
    fn lin(&self, c: Cut, i: usize) -> usize {
        (c.base + i) % self.ring.index()
    }

    fn cell(&self, p: usize, x: usize) -> PackedCell {
        self.store.cells()[p * self.stride().index() + x]
    }

    /// The cell at ordinal `k` of the line starting at `start`, the source
    /// rows `old` wide.
    fn src(&self, c: Cut, start: usize, k: usize) -> PackedCell {
        self.cell(self.lin(c, start + k / c.old), k % c.old)
    }

    /// Whether cell `k` is a wide lead: not itself a tail, and the cell
    /// after it in the same source row is one. A writer never lets a wide
    /// character straddle a row, so a lead in the last column is not one.
    fn is_lead(&self, c: Cut, start: usize, cells: usize, k: usize) -> bool {
        k + 1 < cells
            && !(k + 1).is_multiple_of(c.old)
            && !self.src(c, start, k).content.is_wide_tail()
            && self.src(c, start, k + 1).content.is_wide_tail()
    }

    /// The spacers a line of `cells` needs at width `new`: walk the
    /// ordinals that land on a destination row's last column, and each
    /// that holds a lead pushes everything after it by one.
    fn spacers(&self, c: Cut, start: usize, cells: usize) -> usize {
        let new = c.new;
        if new < 2 {
            return 0;
        }
        let (mut sp, mut d) = (0, new - 1);
        loop {
            let k = d - sp;
            if k + 1 >= cells {
                return sp;
            }
            if self.is_lead(c, start, cells, k) {
                sp += 1;
            }
            d += new;
        }
    }

    /// Whether the row at linear index `i` is not single-width.
    fn sized(&self, c: Cut, i: usize) -> bool {
        self.store.used()[self.lin(c, i)].rendition().is_double()
    }

    fn line_at(&self, c: Cut, start: usize, n_src: usize) -> Line {
        let (old, new) = (c.old, c.new);
        if start >= c.keep {
            return self.kept_line(c, start);
        }
        if self.sized(c, start) {
            return self.sized_line(c, start);
        }
        // A line before the rows kept whole ends where they begin.
        let n_src = n_src.min(c.keep);
        let mut rows = 1;
        while start + rows < n_src
            && self.store.used()[self.lin(c, start + rows - 1)].wrapped()
            && !self.sized(c, start + rows)
        {
            rows += 1;
        }
        let last = self.lin(c, start + rows - 1);
        let mark = self.store.used()[last].index().min(old);
        let cells = (rows - 1) * old + mark;
        let s = self.stride().index();
        let tail =
            crate::history::blank_past_mark(&self.store.cells()[last * s..last * s + s], mark, old);
        let spacers = self.spacers(c, start, cells);
        let dest = (cells + spacers).div_ceil(new).max(1);
        Line {
            start,
            rows,
            cells,
            spacers,
            dest,
            tail,
            rendition: LineRendition::Single,
            bidi: self.store.used()[self.lin(c, start)].bidi(),
            shape: Shape::Cut,
        }
    }

    /// **A row at or past the first kept whole is a line of its own**, one
    /// row, its cells and its mark as they were.
    fn kept_line(&self, c: Cut, start: usize) -> Line {
        let p = self.lin(c, start);
        Line {
            start,
            rows: 1,
            cells: self.stride().index(),
            spacers: 0,
            dest: 1,
            tail: PackedCell::EMPTY,
            rendition: self.store.used()[p].rendition(),
            bidi: self.store.used()[p].bidi(),
            shape: Shape::Kept,
        }
    }

    /// A kept row copied to row `dst`: every cell to the pitch and its
    /// mark, as they were.
    fn copy_kept(&mut self, c: Cut, l: &Line, dst: usize) {
        let s = self.stride().index();
        let (ps, pd) = (self.lin(c, l.start), self.lin(c, dst));
        if ps != pd {
            self.store.copy_within_rows(ps, 0..s, pd, 0);
            self.store.copy_mark(ps, pd);
        }
    }

    /// **A line not single-width is a line of its own, one row, kept whole.** No
    /// DEC text resizes a page but DECCOLM, which clears it
    /// (`[[source: dec-std-070]]` p. 5-72); with the texts and xterm's, mintty's
    /// and konsole's practice read, every character the row holds is kept. So it is
    /// never joined to the rows round it by a wrap -- a double-height pair would
    /// interleave -- and its cells go where they were, those past the new half
    /// unseen until a growth shows them: xterm keeps a double line's cells, and
    /// konsole never splits a double-height line.
    fn sized_line(&self, c: Cut, start: usize) -> Line {
        let p = self.lin(c, start);
        Line {
            start,
            rows: 1,
            cells: self.store.used()[p].index().min(self.stride().index()),
            spacers: 0,
            dest: 1,
            tail: PackedCell::EMPTY,
            rendition: self.store.used()[p].rendition(),
            bidi: self.store.used()[p].bidi(),
            shape: Shape::Sized,
        }
    }

    /// A sized line copied to row `dst`: its cells where they were, blank
    /// past them to the pitch, and its rendition on a mark raised to the new
    /// width. Rows are distinct or the same, and the order the module's head
    /// states puts the read first either way.
    fn copy_sized(&mut self, c: Cut, l: &Line, dst: usize) {
        let s = self.stride().index();
        let (ps, pd) = (self.lin(c, l.start), self.lin(c, dst));
        if ps != pd {
            self.store.copy_within_rows(ps, 0..l.cells, pd, 0);
        }
        let (mut row, mut mark) = self.store.row_and_mark(pd);
        row[l.cells..s].fill(PackedCell::EMPTY);
        *mark = RowExtent::of(l.cells)
            .with_rendition(l.rendition, Cols::of_index(c.new))
            .with_bidi(l.bidi);
    }

    /// The line ending at linear index `end` (exclusive), found from its
    /// marks: the row before its first is the nearest that does not wrap.
    fn line_ending_at(&self, c: Cut, end: usize, first: usize) -> Line {
        if end > c.keep {
            return self.kept_line(c, end - 1);
        }
        let mut start = end - 1;
        while start > first
            && self.store.used()[self.lin(c, start - 1)].wrapped()
            && !self.sized(c, start - 1)
            && !self.sized(c, start)
        {
            start -= 1;
        }
        self.line_at(c, start, end)
    }

    fn write_ord(&mut self, c: Cut, dst: usize, d: usize, cell: PackedCell) {
        let p = self.lin(c, dst + d / c.new);
        *self.store.cell_mut(p, d % c.new) = cell;
    }

    /// The marks and the tail of a line copied to `dst`, and **nothing past
    /// the new width**. A destination row held other cells before the copy
    /// -- a line the re-cut moved, or a tail hidden past the old width -- and
    /// the copy writes only `[0, new)`; left there, what the row held showed
    /// again when the screen grew back without a re-cut, the second half of
    /// one line with another's tail after it. Every row the copy wrote is
    /// blank past the new width to the pitch: its reads are all behind it,
    /// by the order the module's head states.
    fn finish_line(&mut self, c: Cut, l: &Line, dst: usize) {
        let (new, s) = (c.new, self.stride().index());
        for j in 0..l.dest - 1 {
            let p = self.lin(c, dst + j);
            let (mut row, mut m) = self.store.row_and_mark(p);
            *m = RowExtent::ZERO.wrapping(new).with_bidi(l.bidi);
            row[new..s].fill(PackedCell::EMPTY);
        }
        let mark = l.cells + l.spacers - (l.dest - 1) * new;
        let p = self.lin(c, dst + l.dest - 1);
        let (mut row, mut m) = self.store.row_and_mark(p);
        row[mark..new].fill(l.tail);
        row[new..s].fill(PackedCell::EMPTY);
        *m = RowExtent::of(mark).with_bidi(l.bidi);
    }

    /// Widening: cells ascending, every write at or before its read.
    fn copy_up(&mut self, c: Cut, l: &Line, dst: usize) {
        match l.shape {
            Shape::Kept => return self.copy_kept(c, l, dst),
            Shape::Sized => return self.copy_sized(c, l, dst),
            Shape::Cut => {}
        }
        let new = c.new;
        let mut sp = 0;
        for k in 0..l.cells {
            let cell = self.src(c, l.start, k);
            let mut d = k + sp;
            if new >= 2 && d % new == new - 1 && self.is_lead(c, l.start, l.cells, k) {
                self.write_ord(c, dst, d, PackedCell::EMPTY);
                sp += 1;
                d += 1;
            }
            self.write_ord(c, dst, d, cell);
        }
        self.finish_line(c, l, dst);
    }

    /// Narrowing: cells descending, every write at or after its read. The
    /// spacer count runs down from the line's total: cell `k` sits at
    /// `k + sp`, and it owns a spacer at `k + sp - 1` when that is a row's
    /// last column and `k` is a lead -- the forward layout read backwards.
    fn copy_down(&mut self, c: Cut, l: &Line, dst: usize) {
        match l.shape {
            Shape::Kept => return self.copy_kept(c, l, dst),
            Shape::Sized => return self.copy_sized(c, l, dst),
            Shape::Cut => {}
        }
        let new = c.new;
        let mut sp = l.spacers;
        for k in (0..l.cells).rev() {
            let cell = self.src(c, l.start, k);
            let d = k + sp;
            let own = new >= 2
                && sp >= 1
                && (d - 1) % new == new - 1
                && self.is_lead(c, l.start, l.cells, k);
            self.write_ord(c, dst, d, cell);
            if own {
                self.write_ord(c, dst, d - 1, PackedCell::EMPTY);
                sp -= 1;
            }
        }
        self.finish_line(c, l, dst);
    }

    /// Where the cell at (`r`, `x`) of line `l` lands: its destination row
    /// and column, for the cursor and for the window's bottom row.
    fn landing(&self, c: Cut, l: &Line, dst: usize, r: usize, x: usize) -> (usize, usize) {
        // A sized line's cells stay where they were, and a kept row's.
        if l.shape != Shape::Cut {
            return (dst, x);
        }
        let (old, new) = (c.old, c.new);
        let k = (r - l.start) * old + x.min(old);
        if k < l.cells {
            let (mut sp, mut d) = (0, new.max(2) - 1);
            if new >= 2 {
                loop {
                    let kk = d - sp;
                    if kk >= k || kk + 1 >= l.cells {
                        break;
                    }
                    if self.is_lead(c, l.start, l.cells, kk) {
                        sp += 1;
                    }
                    d += new;
                }
                // The cursor's own cell may owe a spacer.
                if (k + sp) % new == new - 1 && self.is_lead(c, l.start, l.cells, k) {
                    sp += 1;
                }
            }
            let d = k + sp;
            return (dst + d / new, d % new);
        }
        let d = k + l.spacers;
        match d < l.dest * new {
            true => (dst + d / new, d % new),
            false => (dst + l.dest - 1, new - 1),
        }
    }

    /// Re-cut the primary's lines from `old` columns to `new`, over the
    /// `hist` rows behind the window, the window, and the `hidden` rows
    /// past it. `cursor` is the window's cursor; the answer says where the
    /// window and the cursor are afterwards. `recut` says which lines are
    /// re-cut and from which row on the rows are kept whole.
    pub fn reflow(
        &mut self,
        (old, new): (Cols, Cols),
        hist: Rows,
        hidden: Rows,
        cursor: (Row, Col),
        recut: Recut,
        mut follow: Option<Follow<'_>>,
    ) -> Reflowed {
        let (old, new) = (old.index(), new.index());
        let rows = self.rows.index();
        let ring = self.ring.index();
        let unchanged = Reflowed {
            rot: self.rot,
            hist,
            hidden,
            cursor,
            dropped: Rows::ZERO,
            blanked: Rows::ZERO,
            kept: (self.rot, Rows::ZERO),
        };
        if old == new || old == 0 || new == 0 || rows == 0 {
            return unchanged;
        }
        let n_src = hist.index() + rows + hidden.index();
        let c = Cut {
            base: self.rot.retreat(hist.index(), self.ring).index(),
            old,
            new,
            keep: match recut {
                Recut::All => usize::MAX,
                Recut::Above(r) => r.index(),
            },
        };
        let cur_row = hist.index() + cursor.0.index().min(rows - 1);
        let top_row = hist.index();

        // Pass one: the lines and what they become.
        let (mut i, mut dest_total) = (0, 0);
        while i < n_src {
            let l = self.line_at(c, i, n_src);
            dest_total += l.dest;
            i += l.rows;
        }
        // Drop whole lines from the oldest end until the re-cut fits.
        let mut first = 0;
        while dest_total > ring {
            let l = self.line_at(c, first, n_src);
            dest_total -= l.dest;
            first += l.rows;
        }

        // Pass two: the copy, and where the two rows that matter land.
        let (mut top, mut cur) = (None, None);
        if new > old {
            let (mut i, mut dst) = (first, first);
            while i < n_src {
                let l = self.line_at(c, i, n_src);
                if (l.start..l.start + l.rows).contains(&top_row) {
                    top = Some(self.landing(c, &l, dst, top_row, 0).0);
                }
                if (l.start..l.start + l.rows).contains(&cur_row) {
                    cur = Some(self.landing(c, &l, dst, cur_row, cursor.1.index()));
                }
                self.follow(&mut follow, c, &l, dst);
                self.copy_up(c, &l, dst);
                dst += l.dest;
                i += l.rows;
            }
        } else {
            let (mut end, mut dend) = (n_src, first + dest_total);
            while end > first {
                let l = self.line_ending_at(c, end, first);
                let dst = dend - l.dest;
                if (l.start..end).contains(&top_row) {
                    top = Some(self.landing(c, &l, dst, top_row, 0).0);
                }
                if (l.start..end).contains(&cur_row) {
                    cur = Some(self.landing(c, &l, dst, cur_row, cursor.1.index()));
                }
                self.follow(&mut follow, c, &l, dst);
                self.copy_down(c, &l, dst);
                dend = dst;
                end = l.start;
            }
        }
        let last = first + dest_total;
        let kept = n_src.saturating_sub(c.keep.max(first));
        let (cur_row, cur_col) = cur.unwrap_or((last.saturating_sub(1).max(first), 0));
        // The window: its top row where it landed, down to keep the cursor
        // on the screen, up to keep the screen full, never above the first
        // retained row.
        let mut top = top.unwrap_or(first).max(first);
        if cur_row >= top + rows {
            top = cur_row + 1 - rows;
        }
        if last < top + rows {
            top = last.saturating_sub(rows).max(first);
        }
        if cur_row < top {
            top = cur_row;
        }
        // Rows the window shows past the last re-cut row hold stale lines.
        for j in last..top + rows {
            let p = self.lin(c, j);
            let (mut row, mut m) = self.store.row_and_mark(p);
            row[..new].fill(PackedCell::EMPTY);
            *m = RowExtent::ZERO;
        }
        Reflowed {
            rot: RowRot(self.lin(c, top)),
            hist: Rows::of_count(top - first),
            hidden: Rows::of_count(last.saturating_sub(top + rows)),
            cursor: (
                Row::from_index(cur_row.saturating_sub(top).min(rows - 1)),
                Col::at_index(cur_col.min(new - 1)),
            ),
            dropped: Rows::of_count(first),
            blanked: Rows::of_count((top + rows).saturating_sub(last)),
            kept: (RowRot(self.lin(c, last - kept)), Rows::of_count(kept)),
        }
    }

    /// Hand one kept line to the follow hook, with where its cells land. A
    /// column at or past the old width is no cell of the line -- a row a
    /// resize at a prompt narrowed without a re-cut keeps a column it had --
    /// and is asked as the row's first cell.
    fn follow(&self, follow: &mut Option<Follow<'_>>, c: Cut, l: &Line, dst: usize) {
        if let Some(f) = follow.as_mut() {
            let land = |r: Rows, x: Col| {
                let x = match x.index() < c.old {
                    true => x.index(),
                    false => 0,
                };
                let (row, col) = self.landing(c, l, dst, r.index(), x);
                (Rows::of_count(row), Col::at_index(col))
            };
            f(Rows::of_count(l.start), Rows::of_count(l.rows), &land);
        }
    }
}
