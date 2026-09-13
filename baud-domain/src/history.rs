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
use crate::quantity::Rows;
use crate::store::RowExtent;

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

/// The lines behind the window, newest first, each to its mark.
///
/// A view over the primary ring: `ring` physical rows at `stride`, the
/// window's first row at `rot`, and `len` rows behind it that are lines
/// rather than blanks. Line `n` back is physical row `rot - 1 - n`, modulo
/// the ring.
pub struct Scrollback<'a> {
    cells: &'a [PackedCell],
    used: &'a [RowExtent],
    stride: usize,
    ring: Rows,
    rot: RowRot,
    len: Rows,
}

impl<'a> Scrollback<'a> {
    pub fn new(
        cells: &'a [PackedCell],
        used: &'a [RowExtent],
        stride: usize,
        ring: Rows,
        rot: RowRot,
        len: Rows,
    ) -> Scrollback<'a> {
        Scrollback {
            cells,
            used,
            stride,
            ring,
            rot,
            len,
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
    /// mark, and how it ended. `None` past what is retained.
    pub fn line(&self, back: Back) -> Option<(&[PackedCell], Wrap)> {
        if back.0 >= self.len.index() {
            return None;
        }
        // One row behind the window, then `back` more, wrapping at the
        // ring's start: `retreat` is the same modulus the window uses.
        let p = self.rot.retreat(back.0 + 1, self.ring).index();
        let n = self.used[p].index();
        let start = p * self.stride;
        let wrap = match self.used[p].wrapped() {
            true => Wrap::Continues,
            false => Wrap::Ended,
        };
        Some((&self.cells[start..start + n], wrap))
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
