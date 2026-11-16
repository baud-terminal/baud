//! **The rendition table, written in one place.** The table is read by id
//! everywhere a cell is resolved, and written only here: [`write_slot`]
//! logs every write, so a reader replaying the log holds the table
//! (`Term::styles_written`), and raises the written rendition's bucket's
//! [`Reach`], so a probe from there reads as far as it. [`Table`] gives
//! reads and no writes, so a write that skipped the log does not compile.
//!
//! One table today; design-grid-8k's banded mode makes it a table a band
//! of rows, behind the same calls.

use crate::storage::StyleLog;
#[cfg(doc)]
use baud_domain::RowStyles;
use baud_domain::{LiveTable, Reach, StyleId, StyleWord};

/// **The rendition table, readable by id and written only by
/// [`write_slot`]**: a slot is `None` where nothing was minted, which reads
/// as [`StyleWord::DEFAULT`].
pub(crate) struct Table<'a>(&'a mut [Option<StyleWord>]);

impl<'a> Table<'a> {
    pub(crate) fn new(slots: &'a mut [Option<StyleWord>]) -> Table<'a> {
        Table(slots)
    }

    /// The same table, borrowed for a shorter life: what a `Paint` takes
    /// from its storage, one pointer as the slice was.
    #[inline(always)]
    pub(crate) fn reborrow(&mut self) -> Table<'_> {
        Table(&mut *self.0)
    }

    /// Every slot, to read.
    #[inline(always)]
    pub(crate) fn as_slice(&self) -> &[Option<StyleWord>] {
        self.0
    }
}

/// **Slot `id` of `table` written, and the write logged**: a rendition
/// interned, or a sweep's free. Every write to the table goes through here,
/// so the log holds every slot a renderer has to resolve again, and every
/// rendition's bucket reaches it ([`Reach`]).
#[inline(always)]
pub(crate) fn write_slot(
    table: &mut Table<'_>,
    log: &mut StyleLog<'_>,
    id: StyleId,
    w: Option<StyleWord>,
) -> StyleId {
    table.0[id.index()] = w;
    // A rendition written past its bucket raises the bucket's reach, so
    // a probe from there reads as far as it.
    if let Some(r) = w.filter(|r| *r != StyleWord::TOMBSTONE) {
        let len = table.0.len();
        let home = StyleId::bucket(r, len);
        let at = &mut log.reach[home];
        *at = (*at).max(Reach::of(id.index().wrapping_sub(home) & (len - 1)));
    }
    log.note(id);
    id
}

/// **Slot 0 seeded with the default**, as a terminal is made: reserved for
/// it, never evicted, and logged as a write is.
pub(crate) fn seed_default(table: &mut Table<'_>, log: &mut StyleLog<'_>) {
    table.0[StyleId::DEFAULT.index()] = Some(StyleWord::DEFAULT);
    log.note(StyleId::DEFAULT);
}

impl Table<'_> {
    /// This table as a live row's reader holds it ([`RowStyles::Live`]).
    #[inline(always)]
    pub(crate) fn live(&self) -> LiveTable<'_> {
        LiveTable::of(self.0)
    }
}

/// **Every band's live rendition table, read-only**: what [`Term::styles`]
/// gives a renderer that resolves each band's slots once a frame. A reader
/// resolving one row's cells takes that row's table from its
/// [`RowStyles`] instead ([`Term::row_styles`], [`Viewport::styles`]).
///
/// [`Term::styles`]: crate::Term::styles
/// [`Term::row_styles`]: crate::Term::row_styles
/// [`Viewport::styles`]: baud_domain::Viewport::styles
#[derive(Clone, Copy, Debug)]
pub struct BandsView<'a> {
    one: LiveTable<'a>,
}

impl<'a> BandsView<'a> {
    pub(crate) fn of(one: LiveTable<'a>) -> BandsView<'a> {
        BandsView { one }
    }

    /// Every band's table, in band order; every band has as many slots.
    pub fn bands(&self) -> impl ExactSizeIterator<Item = LiveTable<'a>> {
        core::iter::once(self.one)
    }

    /// **The table of a terminal that keeps one band**, which every live
    /// row is in: what a reader that resolves live ids without their row's
    /// [`RowStyles`] reads. Panics on a terminal of more bands than one.
    pub fn one(&self) -> LiveTable<'a> {
        assert_eq!(self.bands().len(), 1, "a terminal of one band");
        self.one
    }
}
