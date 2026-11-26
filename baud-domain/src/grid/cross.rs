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

use crate::{Col, RowMut, RowRot, Rows};

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

/// **Which physical row a place in the window's ring order is stored in**:
/// an entry of a [`SlotIndex`]. Made only by the index, which keeps its
/// entries a permutation, so no caller can name a slot the index does not
/// hold.
///
/// ```compile_fail,E0603
/// let _ = baud_domain::WindowSlot(3);
/// ```
///
/// The legal path, reading one the index holds:
///
/// ```
/// let mut store = [baud_domain::WindowSlot::UNSET; 8];
/// let ix = baud_domain::SlotIndex::identity(&mut store, baud_domain::Rows::of_index(4));
/// let _ = ix.order().slot(2);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct WindowSlot(pub(crate) u16);

impl WindowSlot {
    /// An entry of storage no index is laid over yet: what a store is
    /// filled with before [`SlotIndex::identity`] lays the index on it.
    pub const UNSET: WindowSlot = WindowSlot(0);

    /// The slot at `p`, below the index's length, which `SlotIndex::identity`
    /// bounds by `u16`.
    fn at(p: usize) -> WindowSlot {
        WindowSlot(u16::try_from(p).expect("a window slot past u16"))
    }

    /// Its physical row, from 0.
    #[inline(always)]
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// **The order a surface's ring is read in, a [`WindowSlot`] a place**, lent
/// mutably: `slot[p]` is the physical row that place `p` of the ring order
/// shows, and `row_of[s]` the place that shows physical row `s`. A
/// permutation of `0..H` by type: nothing but [`SlotIndex::identity`],
/// [`SlotIndex::rotate`] and [`SlotIndex::normalise`] changes an entry, and
/// each keeps the two arrays each other's inverse. No `IndexMut` and no
/// setter:
///
/// ```compile_fail,E0608
/// fn f(ix: &mut baud_domain::SlotIndex<'_>) { ix[3] = baud_domain::WindowSlot::UNSET; }
/// ```
///
/// The legal path:
///
/// ```
/// let mut store = [baud_domain::WindowSlot::UNSET; 8];
/// let mut ix = baud_domain::SlotIndex::identity(&mut store, baud_domain::Rows::of_index(4));
/// ix.rotate(
///     baud_domain::RowRot::NONE,
///     baud_domain::Row::ZERO..=baud_domain::Row::from_index(3),
///     baud_domain::Rows::of_count(1),
/// );
/// ```
///
/// An index of no places is a surface that keeps none: every place reads
/// as its own slot ([`SlotIndex::none`]).
#[derive(Debug)]
pub struct SlotIndex<'a> {
    slot: &'a mut [WindowSlot],
    row_of: &'a mut [WindowSlot],
}

impl<'a> SlotIndex<'a> {
    /// An index of no places: a surface that keeps none, read through the
    /// identity.
    pub fn none() -> SlotIndex<'a> {
        SlotIndex {
            slot: &mut [],
            row_of: &mut [],
        }
    }

    /// The identity over `rows` places, laid on `store`, which holds the
    /// index and its inverse: `2 * rows` entries.
    ///
    /// # Panics
    /// If `store` is not `2 * rows` entries, or `rows` is past `u16`.
    pub fn identity(store: &'a mut [WindowSlot], rows: Rows) -> SlotIndex<'a> {
        let h = rows.index();
        assert!(
            store.len() == 2 * h,
            "an index and its inverse, {h} entries each"
        );
        let (slot, row_of) = store.split_at_mut(h);
        for (p, (s, q)) in slot.iter_mut().zip(row_of.iter_mut()).enumerate() {
            *s = WindowSlot::at(p);
            *q = WindowSlot::at(p);
        }
        SlotIndex { slot, row_of }
    }

    /// The index an earlier [`SlotIndex::identity`] laid on `store`, lent
    /// again as it was left.
    ///
    /// # Panics
    /// If `store` has an odd length.
    pub fn lend(store: &'a mut [WindowSlot]) -> SlotIndex<'a> {
        assert!(store.len() % 2 == 0, "an index and its inverse");
        let h = store.len() / 2;
        let (slot, row_of) = store.split_at_mut(h);
        let ix = SlotIndex { slot, row_of };
        debug_assert!(ix.order().is_permutation(), "a lent index is a permutation");
        ix
    }

    /// Its places: the ring's rows it orders.
    pub fn len(&self) -> Rows {
        Rows::of_index(self.slot.len())
    }

    /// Whether it orders no places: a surface that keeps no index.
    pub fn is_empty(&self) -> bool {
        self.slot.is_empty()
    }

    /// The index, to read.
    #[inline(always)]
    pub fn order(&self) -> SlotOrder<'_> {
        SlotOrder {
            slot: self.slot,
            row_of: self.row_of,
        }
    }

    /// **Window rows `region` rotated up by `by`**, under ring rotation `at`:
    /// the row `by` below the region's top shows at its top, and the `by`
    /// rows that left the top come back at its bottom. A window row's place
    /// is `(row + at) % H`, so a region can straddle the ring's end; `by`
    /// counts modulo the region's height, and a down rotation by `k` is an
    /// up rotation by the height less `k`.
    ///
    /// # Panics
    /// If the region is empty or its bottom is past the index.
    pub fn rotate(&mut self, at: RowRot, region: core::ops::RangeInclusive<crate::Row>, by: Rows) {
        let h = self.slot.len();
        let (top, bot) = (region.start().index(), region.end().index());
        assert!(
            top <= bot && bot < h,
            "rows {top}..={bot} of an index of {h}"
        );
        let n = bot + 1 - top;
        let k = by.index() % n;
        if k == 0 {
            return;
        }
        // Place of the region's `i`th row, wrapping at the ring's end.
        let first = (top + at.index()) % h;
        let place = |i: usize| {
            if first + i >= h {
                first + i - h
            } else {
                first + i
            }
        };
        // A rotation left by `k` as three reversals over the places.
        let reverse = |slot: &mut [WindowSlot], mut a: usize, mut b: usize| {
            while a + 1 < b {
                b -= 1;
                slot.swap(place(a), place(b));
                a += 1;
            }
        };
        reverse(self.slot, 0, k);
        reverse(self.slot, k, n);
        reverse(self.slot, 0, n);
        for i in 0..n {
            let p = place(i);
            self.row_of[self.slot[p].index()] = WindowSlot::at(p);
        }
    }

    /// **The identity again, the rows following their places**: each pair
    /// of physical rows whose contents the caller swaps is handed to `swap`,
    /// in order, so that afterwards place `p`'s row is stored in physical
    /// row `p`. At most `H - 1` swaps.
    pub fn normalise(&mut self, mut swap: impl FnMut(WindowSlot, WindowSlot)) {
        for p in 0..self.slot.len() {
            let s = self.slot[p];
            if s.index() == p {
                continue;
            }
            // Place `p`'s row comes home from `s`; the row that was at `p`,
            // place `q`'s, goes to `s`. Every place before `p` is home, so
            // `q` lies past it.
            let here = WindowSlot::at(p);
            swap(here, s);
            let q = self.row_of[p];
            self.slot[q.index()] = s;
            self.row_of[s.index()] = q;
            self.slot[p] = here;
            self.row_of[p] = here;
        }
    }
}

/// **A [`SlotIndex`], to read**: the slot each place of the ring order shows
/// and the place each slot is shown at. Empty is the identity -- a surface
/// that keeps no index -- and a place past its length reads as its own slot.
#[derive(Clone, Copy, Debug)]
pub struct SlotOrder<'a> {
    slot: &'a [WindowSlot],
    row_of: &'a [WindowSlot],
}

impl<'a> SlotOrder<'a> {
    /// No index: every place shows its own slot.
    pub const NONE: SlotOrder<'static> = SlotOrder {
        slot: &[],
        row_of: &[],
    };

    /// The index an earlier [`SlotIndex::identity`] laid on `store`, to read.
    ///
    /// # Panics
    /// If `store` has an odd length.
    pub fn of(store: &'a [WindowSlot]) -> SlotOrder<'a> {
        assert!(store.len() % 2 == 0, "an index and its inverse");
        let (slot, row_of) = store.split_at(store.len() / 2);
        SlotOrder { slot, row_of }
    }

    /// The physical row place `p` shows.
    #[inline(always)]
    pub fn slot(&self, p: usize) -> usize {
        self.slot.get(p).map_or(p, |s| s.index())
    }

    /// The place physical row `s` is shown at.
    #[inline(always)]
    pub fn position(&self, s: usize) -> usize {
        self.row_of.get(s).map_or(s, |p| p.index())
    }

    /// Its places.
    pub fn len(&self) -> Rows {
        Rows::of_index(self.slot.len())
    }

    /// Whether it orders no places.
    pub fn is_empty(&self) -> bool {
        self.slot.is_empty()
    }

    /// **Whether every place shows its own slot**: every place compared,
    /// with no early exit, so the loop runs as wide compares. Empty is the
    /// identity.
    #[inline]
    pub fn is_identity(&self) -> bool {
        let off = self
            .slot
            .iter()
            .zip(0u32..)
            .fold(0u32, |off, (s, p)| off | (u32::from(s.0) ^ p));
        off == 0
    }

    /// Whether the index is a permutation and `row_of` its inverse:
    /// `row_of[slot[p]] == p` at every place.
    pub fn is_permutation(&self) -> bool {
        self.slot.len() == self.row_of.len()
            && self
                .slot
                .iter()
                .enumerate()
                .all(|(p, s)| self.row_of.get(s.index()).is_some_and(|q| q.index() == p))
    }
}

/// **How a surface reads its rows' places**: through the identity
/// ([`Identity`], a place is its own slot) or through an index
/// ([`Indexed`], a load). A [`crate::View`] and a [`crate::Surface`] carry
/// one by type, as a read carries its band mode, so a surface read through
/// the identity compiles to the arithmetic of the ring alone. Its reads are
/// the grid's own; outside it a mode is a type to name and nothing more.
pub trait SlotMode: sealed::Lends {}

impl SlotMode for Identity {}
impl SlotMode for Indexed {}

/// What a mode holds and how it reads, private to the grid so that only
/// the grid mints an [`Identity`].
pub(crate) mod sealed {
    use super::{Identity, SlotIndex, SlotOrder};

    pub trait Lends: Sized + 'static {
        /// What a surface read this way holds of its index.
        type Lent<'a>;

        /// The index lent to a surface read this way: dropped through the
        /// identity, held through an index.
        fn lend(index: SlotIndex<'_>) -> Self::Lent<'_>;

        /// The physical row place `p` shows.
        fn slot(lent: &Self::Lent<'_>, p: usize) -> usize;

        /// The place physical row `s` is shown at.
        fn position(lent: &Self::Lent<'_>, s: usize) -> usize;

        /// The index, to read.
        fn order<'s>(lent: &'s Self::Lent<'_>) -> SlotOrder<'s>;

        /// Proof the surface keeps no index, when it keeps none.
        fn unindexed(lent: &Self::Lent<'_>) -> Option<Identity>;
    }
}

/// **Read through the identity**: a place is its own slot. Made only by the
/// grid, so holding one says the surface it came from keeps no index --
/// what a reader that assumes ring order (a freeze) is handed:
///
/// ```compile_fail,E0603
/// let _ = baud_domain::Identity(());
/// ```
///
/// A surface read through an index answers no such proof:
///
/// ```compile_fail,E0599
/// fn f(s: &baud_domain::Surface<'_, baud_domain::Indexed>) -> baud_domain::Identity { s.identity() }
/// ```
///
/// The legal path:
///
/// ```
/// fn f(s: &baud_domain::Surface<'_>) -> baud_domain::Identity { s.identity() }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Identity(pub(crate) ());

impl Identity {
    /// **The physical row place `p` of the ring order shows**: `p` itself,
    /// which is what holding one proves.
    #[inline(always)]
    pub fn slot(self, p: RowRot) -> RowRot {
        p
    }

    /// **The place physical row `s` shows at**: `s` itself.
    #[inline(always)]
    pub fn place(self, s: RowRot) -> RowRot {
        s
    }
}

/// An [`Identity`], for the store that keeps no index for a surface.
pub(crate) fn identity_of_store() -> Identity {
    Identity(())
}

/// **Read through an index**: a place shows the slot its [`SlotIndex`]
/// holds. A mode, never a value.
#[derive(Clone, Copy, Debug)]
pub enum Indexed {}

impl sealed::Lends for Identity {
    type Lent<'a> = Identity;

    /// **Drops the index**, which must be the identity: a surface read
    /// through the identity holds no index, so nothing it does can move
    /// one away from it.
    #[inline(always)]
    fn lend(index: SlotIndex<'_>) -> Identity {
        debug_assert!(
            index.order().is_identity(),
            "an index read through the identity is the identity"
        );
        Identity(())
    }

    #[inline(always)]
    fn slot(_: &Identity, p: usize) -> usize {
        p
    }

    #[inline(always)]
    fn position(_: &Identity, s: usize) -> usize {
        s
    }

    #[inline(always)]
    fn order<'s>(_: &'s Identity) -> SlotOrder<'s> {
        SlotOrder::NONE
    }

    #[inline(always)]
    fn unindexed(i: &Identity) -> Option<Identity> {
        Some(*i)
    }
}

impl sealed::Lends for Indexed {
    type Lent<'a> = SlotIndex<'a>;

    #[inline(always)]
    fn lend(index: SlotIndex<'_>) -> SlotIndex<'_> {
        index
    }

    #[inline(always)]
    fn slot(lent: &SlotIndex<'_>, p: usize) -> usize {
        lent.order().slot(p)
    }

    #[inline(always)]
    fn position(lent: &SlotIndex<'_>, s: usize) -> usize {
        lent.order().position(s)
    }

    #[inline(always)]
    fn order<'s>(lent: &'s SlotIndex<'_>) -> SlotOrder<'s> {
        lent.order()
    }

    #[inline(always)]
    fn unindexed(lent: &SlotIndex<'_>) -> Option<Identity> {
        lent.is_empty().then_some(Identity(()))
    }
}
