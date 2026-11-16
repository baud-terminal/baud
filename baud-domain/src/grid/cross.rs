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

use crate::RowMut;

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

/// **What makes cells copied between two physical rows mean in the row they
/// reached what they meant in the row they left.**
pub trait CrossBand {
    /// `cells`, just copied from physical row `from` into physical row `to`,
    /// where they now are: translate each one's rendition from `from`'s
    /// table to `to`'s.
    fn translate(&mut self, from: PhysRow, to: PhysRow, cells: RowMut<'_>);

    /// The ring turned: physical row `top` is now the screen's first. A
    /// row moved under the cursor with no cursor move, so whatever is kept
    /// per row (the current rendition's id in the row's table) is looked
    /// at again.
    fn rotated(&mut self, top: PhysRow);
}

/// **One table for every row**: nothing to translate.
#[derive(Clone, Copy, Debug, Default)]
pub struct OneTable;

impl CrossBand for OneTable {
    #[inline(always)]
    fn translate(&mut self, _: PhysRow, _: PhysRow, _: RowMut<'_>) {}

    #[inline(always)]
    fn rotated(&mut self, _: PhysRow) {}
}
