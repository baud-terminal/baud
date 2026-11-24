//! **Cells copied from one physical row to another, told where they went.**
//!
//! A cell names its rendition by an id into a table, and where the screen
//! is split into bands of rows, each with its own table, an id read in one
//! band means nothing in another. So every [`View`](crate::View) operation
//! that copies cells between two physical rows -- [`View::copy_row`],
//! [`View::copy_past_edge`], [`View::insert_lines_in`],
//! [`View::delete_lines_in`], [`View::scroll_rect`] -- takes a
//! [`CrossBand`] by type, and hands it each destination row's written cells
//! with the rows they came from and went to; and every turn of the ring
//! ([`View::advance_rot`], [`View::retreat_rot`], [`View::reset_rot`])
//! tells it which physical row is the screen's first now, because a row
//! moves under the cursor with no cursor move. The store's row-to-row copies
//! are private to the grid module, so outside it a copy that skips the
//! translation does not compile.
//!
//! With one table for every row there is nothing to translate:
//! [`OneTable`] is zero-sized and its translation empty, and each mover is
//! a separate instance for it, so the copy compiles to what it was.
//!
//! The re-cut ([`View::reflow`]) moves cells inside the module and takes
//! none: it runs over one table for every row.
//!
//! [`View::copy_row`]: crate::View::copy_row
//! [`View::copy_past_edge`]: crate::View::copy_past_edge
//! [`View::insert_lines_in`]: crate::View::insert_lines_in
//! [`View::delete_lines_in`]: crate::View::delete_lines_in
//! [`View::scroll_rect`]: crate::View::scroll_rect
//! [`View::reflow`]: crate::View::reflow
//! [`View::advance_rot`]: crate::View::advance_rot
//! [`View::retreat_rot`]: crate::View::retreat_rot
//! [`View::reset_rot`]: crate::View::reset_rot

use crate::{Col, RowMut};

/// **A physical row of the ring**: a slot of storage, which screen row it
/// shows changing as the ring turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct PhysRow(usize);

impl PhysRow {
    /// The slot at `p`.
    #[inline(always)]
    pub const fn of(p: usize) -> PhysRow {
        PhysRow(p)
    }

    /// Its slot, from 0.
    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// **A band of rows, by its index among a terminal's bands**: which
/// rendition table its rows resolve through. Not a row and not a slot -- a
/// row becomes one through the layout that says which band holds it, and
/// a band's slots start at its index times the slots a band holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Band(usize);

impl Band {
    /// The first band, which a terminal of one band has alone.
    pub const FIRST: Band = Band(0);

    /// Band `b`.
    #[inline(always)]
    pub const fn of(b: usize) -> Band {
        Band(b)
    }

    /// Its index, from 0.
    #[inline(always)]
    pub const fn index(self) -> usize {
        self.0
    }

    /// The bands from this one up to `end`, not including it.
    pub fn up_to(self, end: Band) -> impl Iterator<Item = Band> + Clone {
        (self.0..end.0).map(Band)
    }

    /// The first of its slots, `slots` a band.
    #[inline(always)]
    pub const fn first_slot(self, slots: usize) -> usize {
        self.0 * slots
    }
}

/// **What makes cells copied between two physical rows mean in the row they
/// reached what they meant in the row they left.**
pub trait CrossBand {
    /// `cells`, just copied from physical row `from` into physical row `to`,
    /// where they now are from column `at`: translate each one's rendition
    /// from `from`'s table to `to`'s.
    fn translate(&mut self, from: PhysRow, to: PhysRow, at: Col, cells: RowMut<'_>);

    /// The ring turned: physical row `top` is now the screen's first. A
    /// row moved under the cursor with no cursor move, so whatever is kept
    /// per row (the current rendition's id in the row's table) is looked
    /// at again.
    fn rotated(&mut self, top: PhysRow);

    /// Columns `cols` of physical row `row` were filled with the blank the
    /// operation was handed, whose rendition is the caller's current
    /// table's: what a caller of one table for every row has nothing to do
    /// about.
    #[inline(always)]
    fn blanked(&mut self, row: PhysRow, cols: core::ops::Range<usize>) {
        let _ = (row, cols);
    }
}

/// **One table for every row**: nothing to translate.
#[derive(Clone, Copy, Debug, Default)]
pub struct OneTable;

impl CrossBand for OneTable {
    #[inline(always)]
    fn translate(&mut self, _: PhysRow, _: PhysRow, _: Col, _: RowMut<'_>) {}

    #[inline(always)]
    fn rotated(&mut self, _: PhysRow) {}
}
