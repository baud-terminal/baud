//! **SEE, `CSI Ps Q`** (`[[source: ecma48]]` 8.3.115): "the editing extent
//! for subsequent character or line insertion or deletion": 0 the active
//! page, 1 the active line, 2 the active field, 3 the active qualified
//! area, 4 "the relevant part of the entire presentation component".
//!
//! **Before any SEE the extent is DEC's**: `ICH`, `DCH` and a write under
//! IRM shift within the line between the margins, SEE 1's, and `IL` and
//! `DL` within the scrolling region, SEE 0's, as DEC STD 070's ICH note 1 and xterm do
//! and as terminfo's xterm entries, which carry no `in`, tell curses (Jev's
//! lean, LINE, 0.54/0.48 then 0.72/0.74). SEE's "default Ps = 0" is the
//! value of an omitted parameter, so `CSI Q` selects the page. `RIS`
//! returns to the line; `DECSTR` and a resize keep the extent (Jev, KEEP,
//! 0.96/0.96); a value past 4 is counted and changes nothing (Jev,
//! COUNTED_KEEP, 0.98/0.94).
//!
//! **A shift across lines keeps within DEC's margins** (Jev's lean,
//! WITHIN_MARGINS, 0.52/0.64 then 0.92/0.86): the shifted part runs through
//! the rectangle DECSTBM and DECSLRM bound, from one line's right margin to
//! the next line's left margin, and ends at the bottom margin; outside the
//! rectangle the edit is ignored, as DEC's are outside the margins. SEE 4
//! is the page here, the history not being on the display (Jev, AS_PAGE,
//! 1.00/1.00). The field is `EF`'s, bounded by the line and the margins;
//! the qualified area `EA`'s run of positions sharing the cursor's guard,
//! within the rectangle. HEM's PRECEDING shifts from the extent's start to
//! the cursor, as FOLLOWING shifts from the cursor to its end.
//!
//! **`IL` and `DL`**: with the extent a line or a field, the shifted part is
//! the active line alone, so they erase it (Jev, ERASE_ACTIVE, 0.98/0.98);
//! with a qualified area they shift the lines from the cursor's to the
//! area's last, within the scrolling region (Jev's lean, AREA_LINES,
//! 0.24/0.22 then 0.66/0.86); with the page as before.
//!
//! **A wide character stays whole, as under DEC's `ICH` and `DCH`**: an
//! edit at either half takes the whole character, the shift starting at
//! whichever half it reaches first (the lead under FOLLOWING, the tail
//! under PRECEDING), a delete taking the character and the n-1 positions
//! after it in the shift's direction, and a character the shift
//! would part is erased, both halves: one a line end would cut, one pushed
//! off the shifted part's end or out of it, one whose other half lies
//! outside it. Under DCSM's PRESENTATION, on a line holding a string, the
//! positions are the ones drawn and their content moves between the cells
//! drawn there, each keeping the string it is stored in, so the line is
//! drawn as before with the content shifted (Jev's lean, BUILD_DRAWN,
//! 0.66/0.66 then 0.69/0.76); a wide character there moves whole where
//! its two halves land side by side in their order, and is erased where
//! they would not.
//!
//! **What it costs while no SEE has chosen another: nothing on the text
//! path.** `ICH` and `DCH` test it out of line, and a write under IRM is a
//! character at a time already, its gap opened out of line.

use baud_domain::{Col, Cols, Count, EditExtent, ModeBit, PackedCell, PathIdx, Row};

use crate::component::Drawn;
use crate::editing::CharEdit;
use crate::paint::Paint;

/// Which way a shift runs along its positions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Toward {
    /// From the cursor to the extent's end, FOLLOWING's.
    End,
    /// From the cursor back to the extent's start, PRECEDING's.
    Start,
}

/// **A run of positions in the order of the character path**: from `from`
/// through `to`, every row between them spanning columns `left` through
/// `right`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Run {
    pub(crate) from: (Row, Col),
    pub(crate) to: (Row, Col),
    pub(crate) left: Col,
    pub(crate) right: Col,
}

impl Run {
    /// The columns a row between `from` and `to` spans.
    fn width(&self) -> Cols {
        Cols::of_index(self.right.index() + 1 - self.left.index())
    }

    /// How many positions the first row holds.
    fn first_len(&self) -> PathIdx {
        PathIdx::at_index(match self.from.0 == self.to.0 {
            true => self.to.1.index() + 1 - self.from.1.index(),
            false => self.right.index() + 1 - self.from.1.index(),
        })
    }

    fn len(&self) -> PathIdx {
        let (a, b) = (self.from.0.index(), self.to.0.index());
        PathIdx::at_index(match a == b {
            true => self.first_len().index(),
            false => {
                self.first_len().index()
                    + (b - a - 1) * self.width().index()
                    + (self.to.1.index() + 1 - self.left.index())
            }
        })
    }

    /// The `i`-th position from `from`.
    fn at(&self, i: PathIdx) -> (Row, Col) {
        let (i, f, w) = (i.index(), self.first_len().index(), self.width().index());
        if i < f {
            return (self.from.0, Col::at_index(self.from.1.index() + i));
        }
        let j = i - f;
        (
            Row::from_index(self.from.0.index() + 1 + j / w),
            Col::at_index(self.left.index() + j % w),
        )
    }

    /// The index from `from` of position `x` of row `y`, if the run holds
    /// it.
    fn index_of(&self, y: Row, x: Col) -> Option<PathIdx> {
        if y < self.from.0 || y > self.to.0 {
            return None;
        }
        let (a, b) = self.cols_on(y);
        if x < a || x > b {
            return None;
        }
        Some(PathIdx::at_index(match y == self.from.0 {
            true => x.index() - self.from.1.index(),
            false => {
                self.first_len().index()
                    + (y.index() - self.from.0.index() - 1) * self.width().index()
                    + (x.index() - self.left.index())
            }
        }))
    }

    /// The columns the run covers on row `y`.
    fn cols_on(&self, y: Row) -> (Col, Col) {
        let a = if y == self.from.0 {
            self.from.1
        } else {
            self.left
        };
        let b = if y == self.to.0 {
            self.to.1
        } else {
            self.right
        };
        (a, b)
    }
}

impl Paint<'_, '_> {
    /// **SEE**: the extent, or a value 8.3.115 does not define counted.
    /// `None` in [`crate::term::Cold::extent`] is DEC's, before any SEE.
    #[cold]
    #[inline(never)]
    pub(crate) fn select_editing_extent(&mut self, e: Option<EditExtent>) {
        match e {
            Some(e) => self.cold.extent = Some(e),
            None => self.unsupported_param += 1,
        }
    }

    /// **Whether SEE chose an extent other than the line** for the
    /// character edits: `ICH`, `DCH` and a write under IRM then go along it.
    pub(crate) fn extent_chosen(&self) -> bool {
        !matches!(self.cold.extent, None | Some(EditExtent::Line))
    }

    /// The cursor's line's map for a write under IRM, `None` under an
    /// extent SEE chose, whose shift takes the cells.
    pub(crate) fn edit_map(&self) -> Option<crate::component::LineMap> {
        match self.extent_chosen() {
            true => None,
            false => self.cursor_map(),
        }
    }

    /// The rectangle DEC's margins bound, as its first and last rows and
    /// columns, while the cursor is inside it.
    fn rectangle(&self) -> Option<Run> {
        let (top, bot) = (self.cold.top, self.cold.bot);
        let right = self
            .cold
            .right
            .min(Col::at_index(self.line_cols().index() - 1));
        let (left, y, x) = (self.cold.left, self.cy, self.cx.col());
        let inside = y >= top && y <= bot && x >= left && x <= right;
        inside.then_some(Run {
            from: (top, left),
            to: (bot, right),
            left,
            right,
        })
    }

    /// **The extent holding the cursor**: its first and last positions and
    /// the columns its rows between span; `None` outside DEC's margins.
    fn extent_ends(&self, d: &mut Drawn) -> Option<Run> {
        let whole = self.rectangle()?;
        let (left, right) = (whole.left, whole.right);
        let (y, x) = (self.cy, self.drawn_from(d, self.cy, self.cx.col()));
        let run = |from, to| Run { from, to, ..whole };
        Some(match self.cold.extent.unwrap_or(EditExtent::Line) {
            EditExtent::Line => run((y, left), (y, right)),
            EditExtent::Page | EditExtent::Entire => whole,
            EditExtent::Field => {
                let start = (0..=x.index()).rev().find(|&c| self.tabs[c]).unwrap_or(0);
                let stop = (x.index() + 1..self.cols.index()).find(|&c| self.tabs[c]);
                let end = stop.map_or(right, |s| Col::at_index(s - 1).min(right));
                run((y, Col::at_index(start).max(left)), (y, end))
            }
            EditExtent::QualifiedArea => {
                let mut guard = |i: usize| {
                    let (r, p) = whole.at(PathIdx::at_index(i));
                    let c = self.drawn_at(d, r, p);
                    self.view.get(r, c).flags.is_guarded()
                };
                let at = (y.index() - whole.from.0.index()) * whole.width().index()
                    + (x.index() - left.index());
                let g = guard(at);
                let mut a = at;
                while a > 0 && guard(a - 1) == g {
                    a -= 1;
                }
                let mut b = at;
                while b + 1 < whole.len().index() && guard(b + 1) == g {
                    b += 1;
                }
                run(
                    whole.at(PathIdx::at_index(a)),
                    whole.at(PathIdx::at_index(b)),
                )
            }
        })
    }

    /// **`ICH` or `DCH` under an extent other than the line** (`crate::editing`
    /// for the line's): `n` positions opened or closed at the cursor along
    /// the extent, toward its end under FOLLOWING and its start under
    /// PRECEDING. The cursor stays.
    #[cold]
    #[inline(never)]
    pub(crate) fn edit_extent(&mut self, n: Count, edit: CharEdit) {
        let toward = match self.cold.modes.is_set(ModeBit::EditPreceding) {
            true => Toward::Start,
            false => Toward::End,
        };
        // Outside DEC's margins the shift is ignored and a string stays
        // open (`Paint::edit_reaches`).
        if self.rectangle().is_some() {
            self.edit_reaches(crate::aligned::EditRows::Chars);
        }
        let x = self.presented_at(self.cx.col());
        self.shift_extent(x, n, edit, toward);
        self.row_changed_to(self.cy);
    }

    /// Column `c` of the cursor's line as the position it is drawn at,
    /// under DCSM's PRESENTATION on a line holding a string.
    pub(crate) fn presented_at(&self, c: Col) -> Col {
        let mut d = Drawn::of(self.cold.modes);
        self.drawn_from(&mut d, self.cy, c)
    }

    /// **The gap a write under IRM opens under an extent other than the
    /// line**, `n` positions at the cursor, toward the extent's end.
    #[cold]
    #[inline(never)]
    pub(crate) fn open_gap_extent(&mut self, n: Count) {
        let x = self.presented_at(self.cx.col());
        self.shift_extent(x, n, CharEdit::Insert, Toward::End);
        self.row_changed_to(self.cy);
    }

    /// **`n` positions opened or closed at position `x` of the cursor's
    /// line**, along the extent toward `toward`. Ignored outside DEC's
    /// margins. Under DCSM's PRESENTATION the positions are the ones drawn.
    /// The cell the shift starts at, a wide character's other half where
    /// it took the whole character; `None` where it was ignored.
    pub(crate) fn shift_extent(
        &mut self,
        x: Col,
        n: Count,
        edit: CharEdit,
        toward: Toward,
    ) -> Option<Col> {
        let mut d = Drawn::of(self.cold.modes);
        let ends = self.extent_ends(&mut d)?;
        let mut x = x.max(ends.left).min(ends.right);
        // **On a wide character's second half the edit takes the whole
        // character**, as DEC's `ICH` and `DCH` do (`Grid::insert_cells`):
        // the shift starts at whichever half it reaches first where the
        // extent holds it.
        let (first, last) = ends.cols_on(self.cy);
        let at = x;
        if let Some((_, p)) = self.other_half(&mut d, self.cy, x) {
            match toward {
                Toward::End if p.index() + 1 == x.index() && p >= first => x = p,
                Toward::Start if p.index() == x.index() + 1 && p <= last => x = p,
                _ => {}
            }
        }
        let c = (self.cy, x);
        let run = match toward {
            Toward::End => Run { from: c, ..ends },
            Toward::Start => Run { to: c, ..ends },
        };
        // A delete snapped to the character's other half still removes the
        // `n` positions from the cursor's own (ECMA-48 8.3.26: the active
        // position and the n-1 after it), so the span grows by the half the
        // snap took in; a delete whose last position is a wide character's
        // first half takes its second (`Grid::delete_cells`).
        let mut k = n.at_least_one();
        if edit == CharEdit::Delete && x != at {
            k += 1;
        }
        if edit == CharEdit::Delete && k < run.len().index() {
            let last = PathIdx::at_index(k - 1);
            if let Some((_, _, Some(j))) = self.half_of(&run, toward, &mut d, last) {
                if j.index() == k {
                    k += 1;
                }
            }
        }
        let start = self.drawn_at(&mut d, self.cy, x);
        self.shift_run(&run, Count::of(k), edit, toward, &mut d);
        Some(start)
    }

    /// **The other half of the wide character drawn at position `x` of row
    /// `r`**: its cell, and the position it is drawn at; `None` where the
    /// cell holds no wide character. A first half is told by the tail
    /// after it.
    fn other_half(&self, d: &mut Drawn, r: Row, x: Col) -> Option<(Col, Col)> {
        let c = self.drawn_at(d, r, x);
        let tail = |c: Col| self.view.get(r, c).content.is_wide_tail();
        let other = match c.index() > 0 && tail(c) {
            true => c.before(),
            false if c.index() + 1 < self.cols.index() && tail(c.after()) => c.after(),
            false => return None,
        };
        Some((other, self.drawn_from(d, r, other)))
    }

    /// The other half of the wide character at the run's `i`-th position in
    /// the shift's order: its row and cell, and its index there if the run
    /// holds it.
    fn half_of(
        &self,
        run: &Run,
        toward: Toward,
        d: &mut Drawn,
        i: PathIdx,
    ) -> Option<(Row, Col, Option<PathIdx>)> {
        let last = run.len().index() - 1;
        let (r, x) = match toward {
            Toward::End => run.at(i),
            Toward::Start => run.at(PathIdx::at_index(last - i.index())),
        };
        let (c, p) = self.other_half(d, r, x)?;
        let j = run.index_of(r, p).map(|j| match toward {
            Toward::End => j,
            Toward::Start => PathIdx::at_index(last - j.index()),
        });
        Some((r, c, j))
    }

    /// **`n` positions opened or closed along `run`**, from its start
    /// toward its end or from its end toward its start, through `d`: where
    /// `d` reads a line's map the content moves between the cells drawn at
    /// the positions, each cell keeping the string it is stored in, so the
    /// line is drawn as it was with the content moved.
    pub(crate) fn shift_run(
        &mut self,
        run: &Run,
        n: Count,
        edit: CharEdit,
        toward: Toward,
        d: &mut Drawn,
    ) {
        let len = run.len().index();
        let pos = |i: usize| match toward {
            Toward::End => run.at(PathIdx::at_index(i)),
            Toward::Start => run.at(PathIdx::at_index(len - 1 - i)),
        };
        let n = n.at_least_one().min(len);
        let blank = self.blank();
        // The cell drawn at the run's `i`-th position.
        let cell_at = |p: &Self, d: &mut Drawn, i: usize| {
            let (r, c) = pos(i);
            (r, p.drawn_at(d, r, c))
        };
        let get = |p: &Self, d: &mut Drawn, i: usize| {
            let (r, c) = cell_at(p, d, i);
            p.view.get(r, c)
        };
        let put = |p: &mut Self, d: &mut Drawn, i: usize, cell: PackedCell| {
            let (r, c) = cell_at(p, d, i);
            let cell = p.kept_in_place(r, c, cell);
            p.view.put(r, c, cell);
        };
        // **A wide character the shift would part is erased first**, both
        // halves: one half outside the run, one pushed off its end or
        // deleted while the other stays, or the two landing anywhere but
        // side by side in their order, across a line end or out of a
        // string drawn reversed. Told before anything moves, as a first
        // half is told by the tail after it.
        let dest = |k: usize| match edit {
            CharEdit::Insert => (k + n < len).then(|| k + n),
            CharEdit::Delete => (k >= n).then(|| k - n),
        };
        let mut erased = 0;
        for i in 0..len {
            let Some((r, other, j)) = self.half_of(run, toward, d, PathIdx::at_index(i)) else {
                continue;
            };
            let j = j.map(PathIdx::index);
            let whole = match j {
                Some(j) if j < i => continue,
                Some(j) if j == i + 1 => match (dest(i), dest(j)) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        let (ra, ca) = cell_at(self, d, a);
                        let (rb, cb) = cell_at(self, d, b);
                        let (_, ci) = cell_at(self, d, i);
                        ra == rb && (cb.index() + ci.index() == ca.index() + other.index())
                    }
                    _ => false,
                },
                _ => false,
            };
            if !whole {
                put(self, d, i, blank);
                let cell = self.kept_in_place(r, other, blank);
                self.view.put(r, other, cell);
                self.erase_images(r, other, other.after());
                erased += 1;
            }
        }
        match edit {
            CharEdit::Insert => {
                for i in (n..len).rev() {
                    let cell = get(self, d, i - n);
                    put(self, d, i, cell);
                }
                for i in 0..n {
                    put(self, d, i, blank);
                }
            }
            CharEdit::Delete => {
                for i in 0..len - n {
                    let cell = get(self, d, i + n);
                    put(self, d, i, cell);
                }
                for i in len - n..len {
                    put(self, d, i, blank);
                }
            }
        }
        for y in run.from.0.index()..=run.to.0.index() {
            let r = Row::from_index(y);
            let (a, b) = run.cols_on(r);
            self.erase_images(r, a, b.after());
            self.row_changed_to(r);
        }
        self.cells = self.cells.plus(len + 2 * erased);
        self.wrote_rows(run.from.0, run.to.0);
    }

    /// **`cell` put in the string the cell at column `c` of row `r` is in
    /// now**: a move by drawn position keeps each cell's string where it is
    /// stored, so the line's map stays as it was.
    fn kept_in_place(&mut self, r: Row, c: Col, cell: PackedCell) -> PackedCell {
        if self.strs.strings.begun != crate::strings::Begun::Yes {
            return cell;
        }
        let s = self.string_of(self.view.get(r, c).style);
        if self.string_of(cell.style) == s {
            return cell;
        }
        let word = self
            .tmpl
            .get(cell.style.index())
            .copied()
            .flatten()
            .unwrap_or(baud_domain::StyleWord::DEFAULT);
        PackedCell {
            style: self.intern_style(word.with_string(s)),
            ..cell
        }
    }

    /// **`IL` or `DL` under SEE 1 or 2**: the active line alone is the
    /// shifted part, so it is put into the erased state within the side
    /// margins. `true` where the extent makes it so.
    pub(crate) fn lines_in_line(&mut self) -> bool {
        if !matches!(self.cold.extent, Some(EditExtent::Line | EditExtent::Field)) {
            return false;
        }
        let (r, b) = (self.cy, self.blank());
        let (a, z) = (self.cold.left, Col::at_index(self.cold.right.index() + 1));
        let k = self.view.fill(r, a, z, b);
        self.erase_images(r, a, z);
        self.cells = self.cells.plus(k);
        self.row_changed_to(r);
        self.wrote_rows(r, r);
        true
    }

    /// **The last line `IL` and `DL` shift to under SEE 3**: the active
    /// qualified area's, within the scrolling region; `None` under any
    /// other extent.
    pub(crate) fn area_last_line(&self) -> Option<Row> {
        if self.cold.extent != Some(EditExtent::QualifiedArea) {
            return None;
        }
        let run = self.extent_ends(&mut Drawn::of(self.cold.modes))?;
        Some(run.to.0.min(self.cold.bot))
    }
}
