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
//! **A wide character a line end would cut is erased**, both halves, and
//! one pushed off the shifted part's end leaves no tail behind. With BDSM
//! reset, on a line holding a string, the cells are shifted, not the
//! positions as drawn.
//!
//! **What it costs while no SEE has chosen another: nothing on the text
//! path.** `ICH` and `DCH` test it out of line, and a write under IRM is a
//! character at a time already, its gap opened out of line.

use baud_domain::{Col, Cols, Count, EditExtent, ModeBit, PackedCell, PathIdx, Row};

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
    from: (Row, Col),
    to: (Row, Col),
    left: Col,
    right: Col,
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
    fn extent_ends(&self) -> Option<Run> {
        let whole = self.rectangle()?;
        let (left, right) = (whole.left, whole.right);
        let (y, x) = (self.cy, self.cx.col());
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
                let guard = |i: usize| {
                    let (r, c) = whole.at(PathIdx::at_index(i));
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
        self.shift_extent(self.cx.col(), n, edit, toward);
        self.row_changed_to(self.cy);
    }

    /// **The gap a write under IRM opens under an extent other than the
    /// line**, `n` positions at the cursor, toward the extent's end.
    #[cold]
    #[inline(never)]
    pub(crate) fn open_gap_extent(&mut self, n: Count) {
        self.shift_extent(self.cx.col(), n, CharEdit::Insert, Toward::End);
        self.row_changed_to(self.cy);
    }

    /// **`n` positions opened or closed at column `x` of the cursor's line**,
    /// along the extent toward `toward`. Ignored outside DEC's margins.
    pub(crate) fn shift_extent(&mut self, x: Col, n: Count, edit: CharEdit, toward: Toward) {
        let Some(ends) = self.extent_ends() else {
            return;
        };
        let c = (self.cy, x.max(ends.left).min(ends.right));
        let run = match toward {
            Toward::End => Run { from: c, ..ends },
            Toward::Start => Run { to: c, ..ends },
        };
        let len = run.len().index();
        let pos = |i: usize| match toward {
            Toward::End => run.at(PathIdx::at_index(i)),
            Toward::Start => run.at(PathIdx::at_index(len - 1 - i)),
        };
        let n = n.at_least_one().min(len);
        let blank = self.blank();
        let get = |p: &Self, i: usize| {
            let (r, c) = pos(i);
            p.view.get(r, c)
        };
        match edit {
            CharEdit::Insert => {
                // A wide character whose tail the shift pushes off the end
                // leaves its lead with none (DEC's ICH loses it whole).
                let cut = n < len && get(self, len - n).content.is_wide_tail();
                for i in (n..len).rev() {
                    let (r, c) = pos(i);
                    let cell = get(self, i - n);
                    self.view.put(r, c, cell);
                }
                for i in 0..n {
                    let (r, c) = pos(i);
                    self.view.put(r, c, blank);
                }
                if cut && toward == Toward::End {
                    let (r, c) = pos(len - 1);
                    self.view.put(r, c, blank);
                }
            }
            CharEdit::Delete => {
                for i in 0..len - n {
                    let (r, c) = pos(i);
                    let cell = get(self, i + n);
                    self.view.put(r, c, cell);
                }
                for i in len - n..len {
                    let (r, c) = pos(i);
                    self.view.put(r, c, blank);
                }
            }
        }
        self.mend_cut_characters(&run, blank);
        for y in run.from.0.index()..=run.to.0.index() {
            let r = Row::from_index(y);
            let (a, b) = run.cols_on(r);
            self.erase_images(r, a, b.after());
            self.row_changed_to(r);
        }
        self.cells = self.cells.plus(len);
        self.wrote_rows(run.from.0, run.to.0);
    }

    /// **A wide character the shift left cut**: one whose halves a line end
    /// parts is erased, both halves, and a tail at the run's start whose
    /// lead is outside it is erased.
    fn mend_cut_characters(&mut self, run: &Run, blank: PackedCell) {
        for y in run.from.0.index()..=run.to.0.index() {
            let r = Row::from_index(y);
            let (a, _) = run.cols_on(r);
            if !self.view.get(r, a).content.is_wide_tail() {
                continue;
            }
            self.view.put(r, a, blank);
            if r > run.from.0 {
                let above = Row::from_index(y - 1);
                let (_, b) = run.cols_on(above);
                self.view.put(above, b, blank);
            }
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
        let run = self.extent_ends()?;
        Some(run.to.0.min(self.cold.bot))
    }
}
