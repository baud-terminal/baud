//! **What a frame must draw again of the one drawn before it**: everything,
//! for a reason [`WholeWhy`] names, or the grid rows in a [`DirtyRows`], what
//! the cursor did and which overlays changed. Built by comparing what a frame
//! was drawn from with what the next one reads; none of it allocates, and
//! each is `Copy`.

use crate::{MAX_ROWS, Row};

/// How many rows a [`DirtyRows`] can name: the screen's [`MAX_ROWS`], its
/// status row, and the row a smooth scroll is moving off.
pub const DIRTY_ROWS: usize = MAX_ROWS + 2;

const WORDS: usize = 11;
const _: () = assert!(WORDS * 64 >= DIRTY_ROWS && (WORDS - 1) * 64 < DIRTY_ROWS);

/// A set of grid rows, one bit a row, under [`DIRTY_ROWS`].
///
/// Its bits are private: a set is built only from [`DirtyRows::NONE`] through
/// `with`, `span` and `union`, which refuse a row past [`DIRTY_ROWS`], so no
/// set names a row the frame cannot hold, and it is read only through
/// `contains` and `iter`.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::DirtyRows([0; 11]);
/// ```
///
/// ```compile_fail,E0616
/// fn f(rows: baud_domain::DirtyRows) -> [u64; 11] { rows.0 }
/// ```
///
/// ```compile_fail,E0608
/// fn f(rows: baud_domain::DirtyRows) -> bool { rows[3] }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{DirtyRows, Row};
/// let rows = DirtyRows::NONE.with(Row::from_index(3)).expect("under DIRTY_ROWS");
/// assert!(rows.contains(Row::from_index(3)));
/// assert_eq!(rows.iter().map(Row::index).collect::<Vec<_>>(), [3]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DirtyRows([u64; WORDS]);

const _: () = assert!(core::mem::size_of::<DirtyRows>() == 88);

impl DirtyRows {
    /// No row.
    pub const NONE: DirtyRows = DirtyRows([0; WORDS]);

    /// The set and `r`, or `None` when `r` is at or past [`DIRTY_ROWS`]; the
    /// caller then draws the frame whole ([`WholeWhy::Overflow`]).
    pub fn with(self, r: Row) -> Option<DirtyRows> {
        let i = r.index();
        if i >= DIRTY_ROWS {
            return None;
        }
        let mut words = self.0;
        *words.get_mut(i / 64)? |= 1 << (i % 64);
        Some(DirtyRows(words))
    }

    /// The set and the rows `from..to`, or `None` when one of them is at or
    /// past [`DIRTY_ROWS`]. An empty range adds nothing and is never `None`.
    pub fn span(self, from: Row, to: Row) -> Option<DirtyRows> {
        let (from, to) = (from.index(), to.index());
        if from >= to {
            return Some(self);
        }
        if to > DIRTY_ROWS {
            return None;
        }
        let mut words = self.0;
        for (k, word) in words.iter_mut().enumerate() {
            let lo = k * 64;
            let (a, b) = (from.max(lo), to.min(lo + 64));
            if a < b {
                let ones = u64::MAX >> (64 - (b - a));
                *word |= ones << (a - lo);
            }
        }
        Some(DirtyRows(words))
    }

    /// Every row in either set.
    pub fn union(self, other: DirtyRows) -> DirtyRows {
        let mut words = self.0;
        for (w, o) in words.iter_mut().zip(other.0) {
            *w |= o;
        }
        DirtyRows(words)
    }

    /// Whether `r` is in the set; a row at or past [`DIRTY_ROWS`] never is.
    pub fn contains(self, r: Row) -> bool {
        let i = r.index();
        i < DIRTY_ROWS && self.0.get(i / 64).is_some_and(|w| w & (1 << (i % 64)) != 0)
    }

    /// The rows in the set, top to bottom.
    pub fn iter(self) -> impl Iterator<Item = Row> {
        self.0.into_iter().enumerate().flat_map(|(k, mut w)| {
            core::iter::from_fn(move || {
                let bit = w.trailing_zeros();
                (w != 0).then(|| {
                    w &= w - 1;
                    Row(k * 64 + bit as usize)
                })
            })
        })
    }
}

/// Why a frame is drawn whole: what moved between the frame drawn and the
/// one to draw that no set of rows can say.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WholeWhy {
    /// The grid's geometry changed.
    Resized,
    /// The surface that is up changed: [`crate::ScreenEpoch`] moved.
    Screen,
    /// A full reset: [`crate::Resets`] moved.
    Reset,
    /// The history was erased: [`crate::HistoryErasures`] moved.
    Erased,
    /// The saved lines were selectively erased: [`crate::HistoryWipes`]
    /// moved.
    Wiped,
    /// A colour the cells are drawn in changed: [`crate::ColorEpoch`] moved.
    Colours,
    /// A cluster the cells name may hold other characters:
    /// [`crate::ClusterEpoch`] moved.
    Clusters,
    /// The renditions written between the frame drawn and the one to draw
    /// cannot be told slot by slot.
    Renditions,
    /// A changed row is at or past [`DIRTY_ROWS`].
    Overflow,
}

/// What the cursor did between the frame drawn and the one to draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorChange {
    /// It is where it was and drawn as it was.
    Same,
    /// It is on another cell.
    Moved,
    /// It is on the same cell and drawn otherwise: its shape, its colour,
    /// or whether it shows.
    Restyled,
}

/// Which overlays drawn over the grid changed: the selection, the find's
/// matches, the hovered link, the scrollbar, the input method's text and the
/// splash, one bit each.
///
/// Its bits are private: a set is built only from the named overlays and
/// their `union`, so no bit outside the six is ever set.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::OverlayChanges(0x80);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::OverlayChanges;
/// let changed = OverlayChanges::FIND.union(OverlayChanges::HOVER);
/// assert!(changed.contains(OverlayChanges::HOVER));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverlayChanges(u8);

impl OverlayChanges {
    /// No overlay changed.
    pub const NONE: OverlayChanges = OverlayChanges(0);
    /// The selection.
    pub const SELECTION: OverlayChanges = OverlayChanges(1);
    /// The find's matches and the current one.
    pub const FIND: OverlayChanges = OverlayChanges(1 << 1);
    /// The link under the pointer.
    pub const HOVER: OverlayChanges = OverlayChanges(1 << 2);
    /// The scrollbar.
    pub const SCROLLBAR: OverlayChanges = OverlayChanges(1 << 3);
    /// The input method's text and its caret.
    pub const IME: OverlayChanges = OverlayChanges(1 << 4);
    /// The splash.
    pub const SPLASH: OverlayChanges = OverlayChanges(1 << 5);

    /// Every overlay in either set.
    pub fn union(self, other: OverlayChanges) -> OverlayChanges {
        OverlayChanges(self.0 | other.0)
    }

    /// Whether every overlay in `other` is in the set.
    pub fn contains(self, other: OverlayChanges) -> bool {
        self.0 & other.0 == other.0
    }
}

/// What a frame must draw again of the one drawn before it.
///
/// Every value is a valid request: `Whole` draws every pixel, and `Rows`
/// draws the rows it names, the cursor and the overlays it names, and no
/// pixel outside them. `Rows` with no row, [`CursorChange::Same`] and
/// [`OverlayChanges::NONE`] is a frame the same as the last.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameDamage {
    /// Every pixel, for this reason.
    Whole(WholeWhy),
    /// These rows, the cursor as it changed, and these overlays.
    Rows {
        rows: DirtyRows,
        cursor: CursorChange,
        overlays: OverlayChanges,
    },
}

const _: () = assert!(core::mem::size_of::<FrameDamage>() == 96);
