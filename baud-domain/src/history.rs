//! Scrollback: the lines behind the screen's window onto the ring.
//!
//! **The grid is the ring and the screen is a window into it.** A scroll
//! advances the rotation by one row and blanks the row that comes into view
//! at the bottom; the row that left the top is not moved, not copied and not
//! forgotten -- it is still where it was written, one row behind the window,
//! and the rows behind it are the lines that scrolled off before. Nothing is
//! written per scroll that the scroll did not already write, and nothing is
//! added to the per-character loop: the pitch is the pitch, the rotation is
//! the rotation, only the modulus grew from the screen's height to the
//! ring's.
//!
//! The design this replaced copied each retiring row into a dense ring, to
//! its mark, and cost **35% of `cat`, 27% of `git log` and 45% of a flat
//! scroll** whatever the ring's size, because the cost is the frequency of
//! the copy and not its length -- the same finding this repo has for the
//! region copy. Here the copy is gone; what remains is the memory, which is
//! the ring's rows at the pitch.
//!
//! This module is the read side. The write side is `View`'s rotation, and the
//! bookkeeping -- how many rows behind the window are lines rather than
//! blanks -- is the core's.

use crate::cell::PackedCell;
use crate::grid::RowRot;
use crate::quantity::{Cols, Rows};
use crate::store::LineRendition;

/// How a stored line ended: at a newline, or by the writer wrapping onto the
/// next row. The fact a reflow needs and the cells do not carry; it rides in
/// the row mark's top bit, set at the wrap and cleared by a newline or a fill
/// to the row's end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wrap {
    Ended,
    Continues,
}

/// How far back a retained line is: zero is the newest, the row just above
/// the screen.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Back(usize);

impl Back {
    pub const NEWEST: Back = Back(0);
    pub fn of(n: usize) -> Back {
        Back(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

/// The lines behind the window, newest first, each as the window draws it
/// over the live width, and to its mark where that lies past the live
/// edge.
///
/// A view over the primary ring: `ring` physical rows at `stride`, the
/// window's first row at `rot`, `len` rows behind it that are lines
/// rather than blanks, and the live width `cols`. Line `n` back is
/// physical row `rot - 1 - n`, modulo the ring.
pub struct Scrollback<'a> {
    cells: &'a [PackedCell],
    used: crate::RowMarks<'a>,
    stride: usize,
    cols: Cols,
    ring: Rows,
    rot: RowRot,
    len: Rows,
    /// The live tables the retained lines are in, a band's each: what a line
    /// that is not frozen resolves through ([`Scrollback::styles`]).
    live: crate::LiveBands<'a>,
    /// The primary's generations and the window's top line, when any line
    /// may be frozen (`crate::frozen`).
    frozen: Option<(crate::FrozenRows<'a>, crate::LineIdx)>,
}

/// The blank `row` is uniform in from its mark to `width`, the live width
/// it was last filled at: the cell at the mark when the mark lies inside
/// it, and the default blank otherwise -- nothing lies between, and past
/// the edge the cells beyond a mark are the old line's, which the passes
/// past the edge clear to [`PackedCell::EMPTY`]. The one reading of the
/// mark that both the history's read and a reflow take, so they cannot
/// disagree about what a line holds past it.
pub(crate) fn blank_past_mark(row: &[PackedCell], mark: usize, width: usize) -> PackedCell {
    match mark < width {
        true => row[mark],
        false => PackedCell::EMPTY,
    }
}

impl<'a> Scrollback<'a> {
    /// Each argument is a part the caller holds apart; grouping them would
    /// only move the list.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cells: &'a [PackedCell],
        used: crate::RowMarks<'a>,
        stride: usize,
        cols: Cols,
        ring: Rows,
        rot: RowRot,
        len: Rows,
        live: crate::LiveBands<'a>,
    ) -> Scrollback<'a> {
        Scrollback {
            cells,
            used,
            stride,
            cols,
            ring,
            rot,
            len,
            live,
            frozen: None,
        }
    }

    /// The same lines, read through the primary's generations: `top` is the
    /// window's top line, so line `n` back is `top - 1 - n`.
    pub fn with_frozen(self, rows: crate::FrozenRows<'a>, top: crate::LineIdx) -> Scrollback<'a> {
        Scrollback {
            frozen: rows.any().then_some((rows, top)),
            ..self
        }
    }

    /// **Which table a retained line's cells name rendition ids in**, `back`
    /// lines before the newest: its band's live table, or the words of the
    /// generation the line was frozen into. A reader that resolves a line's
    /// cells asks this, never the live table alone.
    pub fn styles(&self, back: Back) -> crate::RowStyles<'a> {
        match self.frozen {
            Some((f, top)) if back.0 < self.len.index() => {
                let p = self.rot.retreat(back.0 + 1, self.ring);
                match top.minus(Rows::of_count(back.0 + 1)) {
                    Some(l) => f.styles(p, l, self.live.table(p)),
                    None => crate::RowStyles::Live(self.live.table(p)),
                }
            }
            _ => crate::RowStyles::Live(self.live.table(self.rot.retreat(back.0 + 1, self.ring))),
        }
    }

    /// Retained lines.
    pub fn len(&self) -> usize {
        self.len.index()
    }

    pub fn is_empty(&self) -> bool {
        self.len.index() == 0
    }

    /// A retained line, `back` lines before the newest: its cells to the
    /// mark, or to the live width when the row is uniform from the mark in
    /// a blank other than the default, and how it ended. `None` past what
    /// is retained.
    ///
    /// **The mark is where the row stops being uniform, not where its text
    /// ends.** A row erased in blue has a mark of zero and forty blue
    /// cells, and `abc` erased on in blue a mark of three; read to the mark
    /// alone, the first was no cells and the second `abc`, where the window
    /// scrolled back to them draws the blue. Past the default blank there
    /// is nothing a caller could not supply, so a plain line still reads to
    /// its mark. One cell read a line, on the read side.
    pub fn line(&self, back: Back) -> Option<(&'a [PackedCell], Wrap)> {
        if back.0 >= self.len.index() {
            return None;
        }
        // One row behind the window, then `back` more, wrapping at the
        // ring's start: `retreat` is the same modulus the window uses.
        let p = self.rot.retreat(back.0 + 1, self.ring).index();
        let start = p * self.stride;
        let mark = self.used[p].index();
        let row = &self.cells[start..start + self.stride];
        let n = match blank_past_mark(row, mark, self.cols.index()) == PackedCell::EMPTY {
            true => mark,
            false => self.cols.index(),
        };
        let wrap = match self.used[p].wrapped() {
            true => Wrap::Continues,
            false => Wrap::Ended,
        };
        Some((&self.cells[start..start + n], wrap))
    }

    /// A retained line's rendition, `back` lines before the newest: a line
    /// keeps the size it had on the screen as it scrolls off, as it keeps
    /// its cells. `None` past what is retained.
    pub fn rendition(&self, back: Back) -> Option<LineRendition> {
        if back.0 >= self.len.index() {
            return None;
        }
        let p = self.rot.retreat(back.0 + 1, self.ring).index();
        Some(self.used[p].rendition())
    }
}

/// How much scrollback a caller provides, in lines: rows of the ring past
/// the screen's capacity, each a row of the pitch. `NONE` is a terminal with
/// no scrollback, whose ring is the screen and whose scroll is what it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Depth {
    lines: usize,
}

impl Depth {
    pub const NONE: Depth = Depth { lines: 0 };
    /// Two thousand lines. At the pitch a 400-column grid chooses on a
    /// 128-byte line -- 400 cells, 3.2 KB a row -- that is 6.4 MB, and at
    /// the corpus's 200 columns 3.3 MB: inside L2, which is where the ring
    /// has to stay, because the scroll into a row past L2 measured 0.56x.
    pub const DEFAULT: Depth = Depth { lines: 2_000 };
    pub fn of(lines: usize) -> Depth {
        Depth { lines }
    }
    pub fn lines(self) -> usize {
        self.lines
    }
}
