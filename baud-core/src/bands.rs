//! **The rendition table, written in one place.** The table is read by id
//! everywhere a cell is resolved, and written only here: [`write_slot`]
//! logs every write, so a reader replaying the log holds the table
//! (`Term::styles_written`), and raises the written rendition's bucket's
//! [`Reach`], so a probe from there reads as far as it. [`Table`] gives
//! reads and no writes, so a write that skipped the log does not compile.
//!
//! One table today; design-grid-8k's banded mode makes it a table a band
//! of rows, behind the same calls.

use crate::storage::BandSlots;
use crate::storage::StyleLog;
#[cfg(doc)]
use baud_domain::RowStyles;
use baud_domain::{Band, Col, LiveTable, PhysRow, Reach, Rows, StyleId, StyleSlot, StyleWord};

/// **The rendition table, readable by id and written only by
/// [`write_slot`]**: a slot is [`StyleSlot::Empty`] where nothing was
/// minted, which reads as [`StyleWord::DEFAULT`].
pub(crate) struct Table<'a>(&'a mut [StyleSlot]);

impl<'a> Table<'a> {
    pub(crate) fn new(slots: &'a mut [StyleSlot]) -> Table<'a> {
        Table(slots)
    }

    /// The same table, borrowed for a shorter life: what a `Paint` takes
    /// from its storage, one pointer as the slice was.
    #[inline(always)]
    pub(crate) fn reborrow(&mut self) -> Table<'_> {
        Table(&mut *self.0)
    }
}

/// **Slot `id` of `table` written, and the write logged**: a rendition
/// interned, or a sweep's free. Every write to the table goes through here,
/// so the log holds every slot a renderer has to resolve again, and every
/// rendition's bucket reaches it ([`Reach`]).
#[inline(always)]
pub(crate) fn write_slot<T: Slots>(
    table: &mut T,
    log: &mut StyleLog<'_>,
    id: StyleId,
    w: StyleSlot,
) -> StyleId {
    table.as_mut()[id.index()] = w;
    // A rendition written past its bucket raises the bucket's reach, so
    // a probe from there reads as far as it.
    if let Some(r) = w.word().filter(|r| *r != StyleWord::TOMBSTONE) {
        let len = table.as_slice().len();
        let home = StyleId::bucket(r, len);
        let at = &mut log.reach_mut()[home];
        *at = (*at).max(Reach::of(id.index().wrapping_sub(home) & (len - 1)));
    }
    log.note(id);
    id
}

/// **Slot 0 of every band's table seeded with the default**, as a terminal
/// is made: reserved for it, never evicted, and logged as a write is, in
/// the band's log. A band whose table has been written has it already.
pub(crate) fn seed_defaults(table: &mut Table<'_>, log: &mut StyleLog<'_>) {
    let bs = log.band_slots();
    let back = log.band();
    for b in Band::FIRST.up_to(log.bands()) {
        log.set_band(b);
        if log.book().writes == baud_domain::StyleWrites::NEVER {
            table.0[b.first_slot(bs.0) + StyleId::DEFAULT.index()] =
                StyleSlot::Word(StyleWord::DEFAULT);
            log.note(StyleId::DEFAULT);
        }
    }
    log.set_band(back);
}

/// **`to` stored in every element of `s` that does not hold it already**:
/// a band emptied by a change of width. A table allocated zeroed is
/// resident only where it was written, and a store of zero over an
/// untouched page commits that page, so a band whose slots were never
/// written is only read, and reading it maps no page of its own.
#[cold]
pub(crate) fn clear_written<T: Copy + PartialEq>(s: &mut [T], to: T) {
    for x in s.iter_mut().filter(|x| **x != to) {
        *x = to;
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
    /// Every band's slots, `band_slots` a band, in band order.
    all: &'a [StyleSlot],
    band_slots: BandSlots,
}

impl<'a> BandsView<'a> {
    pub(crate) fn of(all: &'a [StyleSlot], band_slots: BandSlots) -> BandsView<'a> {
        BandsView { all, band_slots }
    }

    /// Every band's table, in band order; every band has as many slots.
    pub fn bands(&self) -> impl ExactSizeIterator<Item = LiveTable<'a>> {
        self.all.chunks(self.band_slots.0.max(1)).map(LiveTable::of)
    }

    /// **The table of a terminal that keeps one band**, which every live
    /// row is in: what a reader that resolves live ids without their row's
    /// [`RowStyles`] reads. Panics on a terminal of more bands than one.
    pub fn one(&self) -> LiveTable<'a> {
        assert_eq!(self.bands().len(), 1, "a terminal of one band");
        LiveTable::of(self.all)
    }
}

/// **Whether a terminal's rendition tables are one or a band of rows
/// each**, by type: a `Paint` is generic over it, so the single mode's
/// write path is compiled with no band to ask about.
pub(crate) trait BandMode: Copy + 'static {
    /// Whether each band of rows has its own table.
    const BANDED: bool;

    /// The table the write path interns into: the current band's.
    type Table<'a>: Slots;

    /// That table, over every band's slots `all`, `band_slots` a band.
    fn lend(all: Table<'_>, band_slots: BandSlots) -> Self::Table<'_>;

    /// The table band `band`'s.
    fn at(table: &mut Self::Table<'_>, band: Band);

    /// Band `band`'s slots, to read.
    fn band<'s>(table: &'s Self::Table<'_>, band: Band) -> &'s [StyleSlot];

    /// What a View operation that copies rows is handed: nothing in the
    /// single mode; the copies, to translate after it, banded.
    type Cross: baud_domain::CrossBand + Default + Recorded;
}

/// One table every row resolves through.
#[derive(Clone, Copy)]
pub(crate) struct OneBand;

impl BandMode for OneBand {
    const BANDED: bool = false;

    type Table<'a> = Table<'a>;

    #[inline(always)]
    fn lend(all: Table<'_>, _: BandSlots) -> Table<'_> {
        all
    }

    #[inline(always)]
    fn at(_: &mut Table<'_>, _: Band) {}

    #[inline(always)]
    fn band<'s>(table: &'s Table<'_>, _: Band) -> &'s [StyleSlot] {
        table.as_slice()
    }

    type Cross = baud_domain::OneTable;
}

/// **The slots the write path interns into**: read through
/// [`Slots::as_slice`], written only by [`write_slot`] and
/// [`seed_default`].
pub(crate) trait Slots {
    /// Every slot, to read.
    fn as_slice(&self) -> &[StyleSlot];

    /// Every slot, to write: [`write_slot`]'s and [`seed_default`]'s alone.
    fn as_mut(&mut self) -> &mut [StyleSlot];

    /// These slots as a live row's reader holds them ([`RowStyles::Live`]).
    #[inline(always)]
    fn live(&self) -> LiveTable<'_> {
        LiveTable::of(self.as_slice())
    }
}

impl Slots for Table<'_> {
    #[inline(always)]
    fn as_slice(&self) -> &[StyleSlot] {
        self.0
    }

    #[inline(always)]
    fn as_mut(&mut self) -> &mut [StyleSlot] {
        self.0
    }
}

/// **Which rows each rendition table holds**: in the single mode one table
/// of every row; banded, a table a `rows` of the primary's ring, then one a
/// `rows` of the alternate screen, then the status row's, in that order in
/// the slots, each fixed to its band for the storage's life.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    /// The rows a band holds; `None` for the single mode's one band.
    rows: Option<baud_domain::Rows>,
    /// The alternate screen's first band, past the primary's, and the
    /// status row's, past the alternate screen's.
    alt_at: Band,
    status_at: Band,
}

impl Layout {
    /// The single mode's one band.
    pub(crate) const ONE: Layout = Layout {
        rows: None,
        alt_at: Band::of(1),
        status_at: Band::of(1),
    };

    /// Bands of `rows` over a primary ring of `ring` rows and an alternate
    /// screen of `screen`.
    #[cfg(feature = "alloc")]
    pub(crate) fn banded(ring: Rows, screen: Rows, rows: Rows) -> Layout {
        let r = rows.index().max(1);
        let primary = ring.index().div_ceil(r);
        Layout {
            rows: Some(rows),
            alt_at: Band::of(primary),
            status_at: Band::of(primary + screen.index().div_ceil(r)),
        }
    }

    /// Whether each band of rows has its own table.
    pub(crate) fn is_banded(&self) -> bool {
        self.rows.is_some()
    }

    /// **Past the last table this layout holds**: every band before it is
    /// one of its tables, the status row's last.
    pub(crate) fn end(&self) -> Band {
        match self.rows {
            None => Band::of(1),
            Some(_) => Band::of(self.status_at.index() + 1),
        }
    }
}

/// **What names a band's slots besides its rows' cells**: every holder of
/// an id in a band's table that the band's sweep must keep, gathered in one
/// type the sweep takes, so a holder the sweep does not see cannot be added
/// without a field here. The pen is the current rendition interned into the
/// band the write path is in, re-interned on entering another. The display
/// parked while the other is written keeps its rendition as a word and no
/// id: its id would be another band's, and it is interned from the word
/// into its own band when it comes back (`Paint::swap_display`). The cursor
/// DECSC saves keeps its rendition as a word, not an id, so it holds no
/// slot; a row a translation rewrites has its ids reset before its words
/// are interned into its band, so a translation in flight holds none.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BandRoots {
    pub(crate) pen: StyleId,
}

impl BandRoots {
    /// The ids the roots hold.
    pub(crate) fn ids(&self) -> [StyleId; BandRoots::COUNT] {
        [self.pen]
    }

    /// How many ids the roots hold.
    pub(crate) const COUNT: usize = 1;
}

/// **The slots of a band's table no row's cell names**: slot 0, the
/// default; [`BandRoots`]; and the one rendition interned while the cell it
/// is for still names the one it replaces. A band of `rows` rows of a
/// `stride` holds every rendition its cells can name when
/// `rows * stride + BAND_RESERVED <= band_slots` ([`crate::Provision`]).
pub const BAND_RESERVED: usize = 1 + BandRoots::COUNT + 1;

/// A table a band of rows ([`Layout`]).
#[derive(Clone, Copy)]
pub(crate) struct ManyBands;

impl BandMode for ManyBands {
    const BANDED: bool = true;

    type Table<'a> = BandTable<'a>;

    fn lend(all: Table<'_>, band_slots: BandSlots) -> BandTable<'_> {
        BandTable {
            all,
            band: Band::FIRST,
            slots: band_slots,
        }
    }

    fn at(table: &mut BandTable<'_>, band: Band) {
        table.band = band;
    }

    fn band<'s>(table: &'s BandTable<'_>, band: Band) -> &'s [StyleSlot] {
        let at = band.first_slot(table.slots.0);
        &table.all.0[at..at + table.slots.0]
    }

    type Cross = Moves;
}

/// **Every band's slots, and which band the write path is in**: the
/// banded mode's table, read and written as the current band's alone.
pub(crate) struct BandTable<'a> {
    all: Table<'a>,
    band: Band,
    slots: BandSlots,
}

impl Slots for BandTable<'_> {
    fn as_slice(&self) -> &[StyleSlot] {
        let at = self.band.first_slot(self.slots.0);
        &self.all.0[at..at + self.slots.0]
    }

    fn as_mut(&mut self) -> &mut [StyleSlot] {
        let at = self.band.first_slot(self.slots.0);
        &mut self.all.0[at..at + self.slots.0]
    }
}

/// Which surface a band's rows are on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    Primary,
    Alt,
    Status,
}

impl Layout {
    /// **The band physical row `p` of `surface` is in**: the status row's
    /// own, or its surface's first band and `p`'s band within the surface.
    pub(crate) fn band_of(&self, surface: Surface, p: PhysRow) -> Band {
        let Some(rows) = self.rows else {
            return Band::FIRST;
        };
        let within = p.index() / rows.index().max(1);
        match surface {
            Surface::Primary => Band::of(within),
            Surface::Alt => Band::of(self.alt_at.index() + within),
            Surface::Status => self.status_at,
        }
    }

    /// The rows a band holds; every row, in the single mode.
    pub(crate) fn rows(&self) -> Option<baud_domain::Rows> {
        self.rows
    }
}

impl Layout {
    /// **Band `b`'s rows**: its surface, and the physical rows of the
    /// surface it holds, `rows` of them from its first, past which a
    /// surface's last band holds fewer.
    pub(crate) fn span_of(&self, b: Band) -> (Surface, core::ops::Range<usize>) {
        let r = self.rows.map_or(usize::MAX, |r| r.index().max(1));
        let (surface, k) = match b {
            _ if b < self.alt_at => (Surface::Primary, b.index()),
            _ if b < self.status_at => (Surface::Alt, b.index() - self.alt_at.index()),
            _ => (Surface::Status, 0),
        };
        match surface {
            Surface::Status => (surface, 0..1),
            _ => (surface, k.saturating_mul(r)..(k + 1).saturating_mul(r)),
        }
    }
}

impl Layout {
    /// **`surface`'s bands**: its first, and the one past its last. The
    /// single mode's one band is every surface's.
    pub(crate) fn surface_bands(&self, surface: Surface) -> (Band, Band) {
        match (self.rows, surface) {
            (None, _) => (Band::FIRST, Band::of(1)),
            (Some(_), Surface::Primary) => (Band::FIRST, self.alt_at),
            (Some(_), Surface::Alt) => (self.alt_at, self.status_at),
            (Some(_), Surface::Status) => (self.status_at, self.end()),
        }
    }
}

/// **What a View operation's row copies were**, read back after it: each
/// row that was written, with the row its cells were first read from.
pub(crate) trait Recorded {
    /// Each row written, where its ids name their renditions, and the
    /// columns written.
    fn moves(&self) -> impl Iterator<Item = (PhysRow, Origin, core::ops::Range<usize>)>;
}

impl Recorded for baud_domain::OneTable {
    #[inline(always)]
    fn moves(&self) -> impl Iterator<Item = (PhysRow, Origin, core::ops::Range<usize>)> {
        core::iter::empty()
    }
}

/// **What a View operation wrote across rows**, each entry a row's columns:
/// copied from another row, with the row its cells were first read from (a
/// row copied on again in the same operation keeps the first row's, since
/// its cells still name that row's band), or filled with the blank, whose
/// rendition is the band the operation ran in. What a banded `Paint`
/// translates after the operation, where a row's cells came from another
/// band.
pub(crate) struct Moves {
    to: [PhysRow; Moves::CAP],
    origin: [Origin; Moves::CAP],
    cols: [core::ops::Range<usize>; Moves::CAP],
    /// How many rows are recorded.
    n: Rows,
}

/// Where a row's moved cells' ids name their renditions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// In this physical row's band.
    Row(PhysRow),
    /// In the band the operation ran in: the blank it was handed.
    Blank,
}

impl Moves {
    /// A screen's rows copied and a screen's blanked, the most one
    /// operation writes.
    const CAP: usize = 2 * crate::storage::MAX_ROWS;

    fn push(&mut self, to: PhysRow, origin: Origin, cols: core::ops::Range<usize>) {
        let n = self.n.index();
        assert!(
            n < Moves::CAP,
            "a View operation writes at most two screens of rows"
        );
        self.to[n] = to;
        self.origin[n] = origin;
        self.cols[n] = cols;
        self.n = Rows::of_index(n + 1);
    }
}

impl Default for Moves {
    fn default() -> Moves {
        Moves {
            to: [PhysRow::of(0); Moves::CAP],
            origin: [Origin::Blank; Moves::CAP],
            cols: core::array::from_fn(|_| 0..0),
            n: Rows::of_index(0),
        }
    }
}

impl baud_domain::CrossBand for Moves {
    fn translate(&mut self, from: PhysRow, to: PhysRow, at: Col, cells: baud_domain::RowMut<'_>) {
        let at = at.index();
        let origin = self.to[..self.n.index()]
            .iter()
            .rposition(|&t| t == from)
            .map_or(Origin::Row(from), |i| self.origin[i]);
        self.push(to, origin, at..at + cells.len());
    }

    fn rotated(&mut self, _: PhysRow) {}

    fn blanked(&mut self, row: PhysRow, cols: core::ops::Range<usize>) {
        self.push(row, Origin::Blank, cols);
    }
}

impl Recorded for Moves {
    fn moves(&self) -> impl Iterator<Item = (PhysRow, Origin, core::ops::Range<usize>)> {
        (0..self.n.index()).map(|i| (self.to[i], self.origin[i], self.cols[i].clone()))
    }
}

/// **Slots of one band a test holds** (`probe-band-pin`): renditions no
/// cell names, interned into band `band`, which that band's sweep keeps
/// beside its roots, so a test fills the band to its reserve exactly.
#[cfg(feature = "probe-band-pin")]
#[derive(Clone, Copy)]
pub(crate) struct Pins {
    pub(crate) band: Band,
    ids: [StyleId; Pins::CAP],
    held: BandSlots,
}

#[cfg(feature = "probe-band-pin")]
impl Pins {
    /// The most a test holds.
    pub(crate) const CAP: usize = 256;

    /// None held.
    pub(crate) const NONE: Pins = Pins {
        band: Band::FIRST,
        ids: [StyleId::DEFAULT; Pins::CAP],
        held: BandSlots(0),
    };

    /// The ids held.
    pub(crate) fn held(&self) -> &[StyleId] {
        &self.ids[..self.held.0]
    }

    /// One more held, while there is room.
    pub(crate) fn hold(&mut self, id: StyleId) {
        if self.held.0 < Pins::CAP {
            self.ids[self.held.0] = id;
            self.held = BandSlots(self.held.0 + 1);
        }
    }
}
