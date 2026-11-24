//! **The bands a frame draws, each in a slot of its resolved table.**
//!
//! A terminal of more bands than one names each live cell's rendition by
//! an id in its row's band's table, and ids repeat from band to band. A
//! frame's resolved table therefore holds one slot of `band_slots` entries
//! for each band its rows are in, laid end to end, and each row a base:
//! its band's slot times `band_slots`. A cell resolves as
//! `table[base + id]`, which the cell shader reads as
//! `styles[seg_base[row] + (cell.y & 0xffff)]`. A band keeps its slot
//! while the frame draws it, so a scroll that stays among the same bands
//! moves no base and resyncs no slot, and a band coming in takes a slot a
//! band going out left. The single mode's one band is one slot, every base
//! 0.
//!
//! **A display line joining two bands** -- a paragraph drawn at its words
//! across a band's last row -- names ids of both, which no one base serves.
//! Its cells are given ids of its own in the frame's copy, one for each
//! (band, id) pair it names, in its logical order, and its base points into
//! the loose region past the slots, where each pair's resolved value is
//! copied from its band's slot. A line joins two bands only across a
//! boundary between bands the frame draws, so the region holds `cols` entries
//! for each of them: `cols` times one fewer than the bands placed. It is
//! filled again every frame, from slots synced already, `cols` copies a
//! joined line.

use core::ops::Range;

use crate::{StyleStamp, Tally};
use baud_core::{BandSlots, Term};
use baud_domain::{Band, Col, Cols, PackedCell, Row, Rows, StyleId, StyleSlot};

/// **Moves when any row's base moves**: what a reader that uploads the
/// bases (the cell shader's `seg_base`) compares, so it uploads them only
/// then.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BaseStamp(u64);

/// **What a window of bands is at**: moves whenever one of its slots
/// resolves or a base moves ([`BandWindow::moved`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct WindowGen(u64);

/// A slot of a window of bands, in table order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct SlotIx(usize);

/// **How a draw row's cells index the resolved table**: taken once a row
/// ([`BandWindow::row_read`]), so each of its cells resolves as one load.
/// A row of a terminal of one band reads the whole table ([`WholeTable`]),
/// and a row of a window that placed bands reads from its base
/// ([`RowBase`]); a reader generic over the two is compiled once for
/// each, and the one-band path adds nothing to an id.
pub(crate) trait RowRead: Copy {
    /// Id `id`'s entry in the resolved table `all`, if it holds one.
    fn entry<T>(self, all: &[T], id: StyleId) -> Option<&T>;
}

/// **The whole resolved table, every id at its own index**: how every row
/// of a terminal of one band reads it, where no band is placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct WholeTable;

impl RowRead for WholeTable {
    #[inline(always)]
    fn entry<T>(self, all: &[T], id: StyleId) -> Option<&T> {
        all.get(id.index())
    }
}

/// **Where a draw row's entries start in the resolved table**
/// ([`BandWindow::base`]): its base, 0 past the rows placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct RowBase(usize);

impl RowRead for RowBase {
    #[inline(always)]
    fn entry<T>(self, all: &[T], id: StyleId) -> Option<&T> {
        all.get(self.0 + id.index())
    }
}

/// **The entries a buffer set's resolved table is allocated with**
/// ([`cell_styles_len`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableLen(pub usize);

/// **A frame's resolved table as each of its draw rows reads it**: from
/// the row's base on ([`BandWindow`], [`crate::Renderer::cell_bases`]), so a
/// cell's id indexes its own band's renditions. One table every row reads
/// whole on a terminal of one band, where there are no bases.
#[derive(Clone, Copy, Debug)]
pub struct RowTables<'s> {
    all: &'s [crate::Resolved],
    bases: &'s [u32],
}

impl<'s> RowTables<'s> {
    /// One table every row reads whole.
    pub fn one(all: &'s [crate::Resolved]) -> RowTables<'s> {
        RowTables { all, bases: &[] }
    }

    /// `all` read by each draw row from its base in `bases`; a row past
    /// them from 0.
    pub fn banded(all: &'s [crate::Resolved], bases: &'s [u32]) -> RowTables<'s> {
        RowTables { all, bases }
    }

    /// **The table draw row `r` reads**: from its base on.
    pub fn at(&self, r: Row) -> &'s [crate::Resolved] {
        let b = self.bases.get(r.index()).copied().unwrap_or(0);
        self.all
            .get(usize::try_from(b).unwrap_or(0)..)
            .unwrap_or(&[])
    }

    /// The whole table, every band's slots and the loose region: what a
    /// reader that copies it as it is reads.
    pub fn all(&self) -> &'s [crate::Resolved] {
        self.all
    }
}

/// **A buffer set's rows in the overlay's order** ([`crate::Renderer::overlay_rows`]):
/// the window's rows, then the row after them -- the leaving row while
/// sliding, which the shader draws first, or the status line's -- each
/// with its base and its band. What a reader of the set's copy reads it
/// through.
#[derive(Clone, Debug, Default)]
pub struct OverlayRows {
    bases: Vec<u32>,
    bands: Vec<Band>,
    /// The rows of a line of its own ([`Joined`]), in the overlay's order,
    /// and each one's table of its pairs' words, `cols` a row: what the
    /// copy's ids name on that row ([`OverlayRows::view`]).
    own_rows: Vec<Row>,
    own: Vec<StyleSlot>,
}

impl OverlayRows {
    /// `window`'s rows put in the overlay's order, `leaving` whether the
    /// first draw row is the leaving row's; each line of its own a table
    /// of its pairs' words, `cols` a row, read in `term`.
    pub(crate) fn of(
        window: &BandWindow,
        leaving: crate::Leaving,
        (term, cols): (&Term<'_>, Cols),
    ) -> OverlayRows {
        let (bases, bands) = (window.bases(), window.bands());
        let order = |n: usize| -> Vec<usize> {
            match (leaving, n) {
                (crate::Leaving::Drawn, 1..) => (1..n).chain(core::iter::once(0)).collect(),
                _ => (0..n).collect(),
            }
        };
        let n = bases.len();
        // A draw row's place in the overlay's order.
        let place = |d: usize| match (leaving, d) {
            (crate::Leaving::Drawn, 0) => n.saturating_sub(1),
            (crate::Leaving::Drawn, d) => d - 1,
            (_, d) => d,
        };
        let view = term.styles();
        let mut own_rows = Vec::new();
        let mut own = Vec::new();
        for (j, pairs) in window.all_joined().iter().zip(&window.pairs) {
            own_rows.push(Row::from_index(place(j.row.index())));
            own.extend((0..cols.index()).map(|k| match pairs.get(k) {
                Some(&(b, id, _)) => StyleSlot::Word(view.table(b).word(id)),
                None => StyleSlot::Empty,
            }));
        }
        OverlayRows {
            bases: order(bases.len()).into_iter().map(|k| bases[k]).collect(),
            bands: order(bands.len()).into_iter().map(|k| bands[k]).collect(),
            own_rows,
            own,
        }
    }

    /// Whether any row is a line of its own: a view of the set's copy reads
    /// its ids as numbered there.
    pub fn any_own(&self) -> bool {
        !self.own_rows.is_empty()
    }

    /// The set's table as each overlay row reads it.
    pub fn tables<'s>(&'s self, all: &'s [crate::Resolved]) -> RowTables<'s> {
        RowTables::banded(all, &self.bases)
    }

    /// **The set's copy as a view**, `cells` at `stride`, `cols` by `rows`
    /// with `used` their extents, each row's ids read through its band's
    /// table in `term`, a line of its own's through its pairs' words;
    /// through one table on a terminal of one band.
    pub fn view<'a>(
        &'a self,
        term: &'a Term<'_>,
        (cells, used): (&'a [PackedCell], &'a [baud_domain::RowExtent]),
        stride: baud_domain::Stride,
        (cols, rows): (Cols, Rows),
    ) -> baud_domain::Viewport<'a> {
        let view = term.styles();
        baud_domain::Viewport::new_in(
            cells,
            used,
            stride,
            cols,
            rows,
            rows,
            baud_domain::RowRot::NONE,
            baud_domain::LiveBands::lines_own(
                view.slots(),
                view.band_slots().0,
                &self.bands,
                (&self.own_rows, &self.own),
            ),
        )
    }
}

/// **Whether a pair of a line's own is a struck link's**: drawn in the
/// rendition the strike's rule leaves ([`crate::MissingPaint`]), or in its
/// band's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Strike {
    No,
    Yes,
}

/// A cell's pair on a line of its own: its band, its id there, and whether
/// a struck link covers it.
pub(crate) type Pair = (Band, StyleId, Strike);

/// **A draw row given ids of its own**: a display line whose cells name ids
/// of two bands, or a row holding a struck link's cells, on a terminal of
/// more bands than one. Its draw row; its columns in their logical order as
/// runs, each with the band its ids are in; and the columns struck links
/// cover, as [`crate::Renderer::take_struck`] names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Joined {
    pub(crate) row: Row,
    pub(crate) runs: Vec<(Range<usize>, Band)>,
    pub(crate) struck: Vec<Range<usize>>,
}

impl Joined {
    /// **Line `logical`'s cells with ids of its own**: each cell's pair
    /// numbered in the order the line first names it, into `pairs`, and the
    /// cell's id replaced by that number; logical column `l` struck where
    /// `struck_at(l)` is a struck column.
    pub(crate) fn rewrite(
        &self,
        logical: &mut [PackedCell],
        struck_at: impl Fn(Col) -> Col,
        pairs: &mut Vec<Pair>,
    ) {
        self.rewrite_drawn(logical, (|l| l, struck_at), pairs, |_, _| {});
    }

    /// **[`Joined::rewrite`] on the line as a copy holds it**: logical
    /// column `l` is `cells[at(l)]` and struck where `struck_at(l)` is a
    /// struck column, numbered in the logical order all the same, and
    /// `rewrote` told each column of `cells` and the id it held.
    pub(crate) fn rewrite_drawn(
        &self,
        cells: &mut [PackedCell],
        (at, struck_at): (impl Fn(Col) -> Col, impl Fn(Col) -> Col),
        pairs: &mut Vec<Pair>,
        mut rewrote: impl FnMut(Col, StyleId),
    ) {
        pairs.clear();
        for (run, b) in &self.runs {
            for l in run.clone() {
                let l = Col::at_index(l);
                let x = at(l);
                let Some(cell) = cells.get_mut(x.index()) else {
                    continue;
                };
                let s = struck_at(l).index();
                let strike = match self.struck.iter().any(|r| r.contains(&s)) {
                    true => Strike::Yes,
                    false => Strike::No,
                };
                let pair = (*b, cell.style, strike);
                let k = match pairs.iter().position(|&p| p == pair) {
                    Some(k) => k,
                    None => {
                        pairs.push(pair);
                        pairs.len() - 1
                    }
                };
                rewrote(x, cell.style);
                cell.style = StyleId::from_index(k);
            }
        }
    }

    /// **The line's entries of the loose region filled** in `styles`, from
    /// its row's base in `window`: each pair's resolved value, its own in
    /// `greys` where it has one -- a struck pair's -- else copied from its
    /// band's slot, `bs` entries a slot. An entry past the region is left.
    pub(crate) fn fill<T: Copy>(
        &self,
        (pairs, greys): (&[Pair], &[Option<T>]),
        (window, bs): (&BandWindow, BandSlots),
        styles: &mut [T],
    ) {
        let base = window.rows_from(self.row).start;
        let end = window.loose().end.min(styles.len());
        for (k, &(b, id, _)) in pairs.iter().enumerate() {
            let from = window.slot_of(b).map(|s| s.0 * bs.0 + id.index());
            let own = greys.get(k).copied().flatten();
            let value = own.or_else(|| from.and_then(|f| styles.get(f)).copied());
            if let (Some(v), true) = (value, base + k < end) {
                styles[base + k] = v;
            }
        }
    }
}

/// The window of bands: which band each slot holds, each slot's resolved
/// copy's stamp, each drawn row's base, and the lines joining two bands.
#[derive(Default)]
pub(crate) struct BandWindow {
    slots: Vec<Option<Band>>,
    stamps: Vec<StyleStamp>,
    bases: Vec<u32>,
    stamp: BaseStamp,
    /// Each drawn row's band, in row order.
    rows: Vec<Band>,
    joined: Vec<Joined>,
    /// Each line's pairs, by the same index as `joined`, as the cell path's
    /// copy last numbered them ([`BandWindow::keep_pairs`]).
    pairs: Vec<Vec<Pair>>,
    loose: Range<usize>,
    generation: WindowGen,
    /// Frozen sources the last placing met.
    frozen: Tally,
}

impl BandWindow {
    /// **Each band of `rows` and of `joined`'s runs gets a slot**: a band a
    /// slot holds keeps it, a slot whose band neither names is freed, and a
    /// band no slot holds takes the first free one, the slots growing only
    /// when none is. Each row's base is then its band's slot times
    /// `band_slots`, and a line of its own's ([`Joined`]) its place in the
    /// loose region past the slots, `cols` entries a line.
    pub(crate) fn place(
        &mut self,
        rows: &[Band],
        joined: Vec<Joined>,
        band_slots: BandSlots,
        cols: Cols,
    ) {
        let (bs, cols) = (band_slots.0, cols.index());
        let named = |b: Band| {
            rows.contains(&b) || joined.iter().any(|j| j.runs.iter().any(|(_, r)| *r == b))
        };
        for s in self.slots.iter_mut() {
            if s.is_some_and(|b| !named(b)) {
                *s = None;
            }
        }
        let all = rows
            .iter()
            .copied()
            .chain(joined.iter().flat_map(|j| j.runs.iter().map(|(_, b)| *b)));
        for b in all {
            if self.slots.contains(&Some(b)) {
                continue;
            }
            match self.slots.iter().position(Option::is_none) {
                Some(k) => {
                    self.slots[k] = Some(b);
                    self.stamps[k] = StyleStamp::default();
                }
                None => {
                    self.slots.push(Some(b));
                    self.stamps.push(StyleStamp::default());
                }
            }
        }
        let start = self.slots.len() * bs;
        self.loose = start..start + cols * joined.len();
        let bases: Vec<u32> = rows
            .iter()
            .enumerate()
            .map(|(r, &b)| {
                let at = match joined.iter().position(|j| j.row.index() == r) {
                    Some(j) => start + j * cols,
                    None => self.slot_of(b).map_or(0, |s| s.0 * bs),
                };
                u32::try_from(at).unwrap_or(u32::MAX)
            })
            .collect();
        self.rows = rows.to_vec();
        self.joined = joined;
        self.pairs.clear();
        if bases != self.bases {
            self.bases = bases;
            self.stamp = BaseStamp(self.stamp.0 + 1);
        }
    }

    /// The slot band `b` is placed in, if it is.
    pub(crate) fn slot_of(&self, b: Band) -> Option<SlotIx> {
        self.slots.iter().position(|s| *s == Some(b)).map(SlotIx)
    }

    /// The slots that hold a band, in table order.
    pub(crate) fn placed(&self) -> impl Iterator<Item = (SlotIx, Band)> + '_ {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(k, s)| s.map(|b| (SlotIx(k), b)))
    }

    /// Slot `k`'s entries in the resolved table, `bs` a slot.
    pub(crate) fn entries(&self, k: SlotIx, bs: BandSlots) -> Range<usize> {
        k.0 * bs.0..(k.0 + 1) * bs.0
    }

    /// Slot `k`'s resolved copy's stamp.
    pub(crate) fn stamp_mut(&mut self, k: SlotIx) -> &mut StyleStamp {
        &mut self.stamps[k.0]
    }

    /// Each drawn row's base, in row order.
    pub(crate) fn bases(&self) -> &[u32] {
        &self.bases
    }

    /// Each drawn row's band, in row order.
    pub(crate) fn bands(&self) -> &[Band] {
        &self.rows
    }

    /// What the bases are at.
    pub(crate) fn base_stamp(&self) -> BaseStamp {
        self.stamp
    }

    /// **The resolved table as draw row `r` reads it**, `all` from its base
    /// on: the whole of it past the rows placed.
    pub(crate) fn row_table<'t, T>(&self, r: Row, all: &'t [T]) -> &'t [T] {
        all.get(self.rows_from(r)).unwrap_or(&[])
    }

    /// Draw row `r`'s entries on from its base: from 0 past the rows placed.
    fn rows_from(&self, r: Row) -> core::ops::RangeFrom<usize> {
        self.base(r).0..
    }

    /// **Draw row `r`'s base**, 0 past the rows placed and on a terminal
    /// of one band, which places none.
    pub(crate) fn base(&self, r: Row) -> RowBase {
        let b = self.bases.get(r.index()).copied().unwrap_or(0);
        RowBase(usize::try_from(b).unwrap_or(0))
    }

    /// **How draw row `r` reads the resolved table**: from its base where
    /// the window placed bands, and `None`, the whole table
    /// ([`WholeTable`]), where it placed none, which is every frame of a
    /// terminal of one band.
    pub(crate) fn row_read(&self, r: Row) -> Option<RowBase> {
        match self.bases.is_empty() {
            true => None,
            false => Some(self.base(r)),
        }
    }

    /// **The loose region**: where in the resolved table the joined lines'
    /// entries are, past the slots.
    pub(crate) fn loose(&self) -> Range<usize> {
        self.loose.clone()
    }

    /// The window moved: a slot resolved, or a base moved.
    pub(crate) fn moved(&mut self) {
        self.generation = WindowGen(self.generation.0 + 1);
    }

    /// What the window is at: moves with [`BandWindow::moved`].
    pub(crate) fn generation(&self) -> WindowGen {
        self.generation
    }

    /// **The frozen sources the frame's rows and lines met**: a frame of
    /// more bands than one lends nothing, which is right while this is 0.
    pub(crate) fn met_frozen(&mut self, n: Tally) {
        self.frozen = n;
    }

    /// What [`BandWindow::met_frozen`] last counted.
    pub(crate) fn frozen(&self) -> Tally {
        self.frozen
    }

    /// How many lines join two bands.
    pub(crate) fn joined_lines(&self) -> Tally {
        Tally::of(self.joined.len())
    }

    /// **Each line's pairs as its copy numbered them**, by the same index as
    /// [`BandWindow::all_joined`]: what a reader of the copy names a line's
    /// ids by.
    pub(crate) fn keep_pairs(&mut self, pairs: Vec<Vec<Pair>>) {
        self.pairs = pairs;
    }

    /// Every line that joins two bands.
    pub(crate) fn all_joined(&self) -> &[Joined] {
        &self.joined
    }

    /// Draw row `r`'s joined line, where it is one.
    pub(crate) fn joined(&self, r: Row) -> Option<&Joined> {
        self.joined.iter().find(|j| j.row == r)
    }
}

/// **The entries a buffer set's resolved table holds**, for a window of
/// `cols` by `rows` on `term`: its one table's slots on a terminal of one
/// band. On one of more, a slot for each band a frame can place and the
/// loose region past them ([`BandWindow::place`]): the window's rows, the
/// leaving row, and a line past the window at each end that a display
/// line's runs may come from, consecutive, so in at most one band more
/// than they fill, and the status line's band; and `cols` entries for each
/// line of its own a frame can draw ([`Joined`]): one a draw row, the
/// window's rows and the row after them, at most, and one a boundary
/// between those bands, at most, so `rows` and one fewer than the bands
/// is a bound. Allocate it zeroed and the slots no frame resolves are
/// never touched.
pub fn cell_styles_len(term: &Term<'_>, cols: Cols, rows: Rows) -> TableLen {
    let view = term.styles();
    let Some(per) = term.band_rows().filter(|_| view.bands().len() > 1) else {
        return TableLen(view.slots().len());
    };
    let lines = rows.index() + 3;
    let bands = lines.div_ceil(per.index().max(1)) + 2;
    TableLen(bands * view.band_slots().0 + cols.index() * (rows.index() + bands - 1))
}

#[cfg(test)]
mod tests {
    use super::{BandWindow, Joined, Strike};
    use baud_core::BandSlots;
    use baud_domain::{Band, Cols, PackedCell, Row, StyleId};

    /// **The loose region holds a line for each boundary between the bands
    /// placed, filled at that bound**: three bands, rows in each, and a
    /// joined line across both boundaries, each naming as many pairs as it
    /// has columns. The region starts where the slots end, is `cols` times
    /// two long, and the last line's last pair is its last entry; every
    /// other row's base is its band's slot.
    /// Breaks: size the region `cols` a band placed rather than a boundary.
    #[test]
    fn the_loose_region_holds_a_line_a_boundary_filled_at_its_bound() {
        let base = |w: &BandWindow, r: usize| usize::try_from(w.bases()[r]).unwrap();
        let (cols, bs) = (6, 16);
        let bands = [Band::of(3), Band::of(4), Band::of(5)];
        let rows = [bands[0], bands[0], bands[1], bands[1], bands[2]];
        let joined = vec![
            Joined {
                row: Row::from_index(1),
                runs: vec![(0..2, bands[0]), (2..cols, bands[1])],
                struck: vec![],
            },
            Joined {
                row: Row::from_index(3),
                runs: vec![(0..5, bands[1]), (5..cols, bands[2])],
                struck: vec![],
            },
        ];
        let mut w = BandWindow::default();
        w.place(&rows, joined, BandSlots(bs), Cols::of_index(cols));
        let start = 3 * bs;
        assert_eq!(w.loose(), start..start + 2 * cols, "a line a boundary");
        assert_eq!(base(&w, 1), start);
        assert_eq!(base(&w, 3), start + cols);
        for r in [0, 2, 4] {
            let slot = w.slot_of(rows[r]).unwrap();
            let at = w.entries(slot, BandSlots(bs)).start;
            assert_eq!(base(&w, r), at, "row {r}: its band's slot");
        }
        let mut pairs = Vec::new();
        for r in [1, 3] {
            let mut line: Vec<PackedCell> = (0..cols)
                .map(|c| PackedCell {
                    style: StyleId::from_index(c + 1),
                    ..PackedCell::EMPTY
                })
                .collect();
            w.joined(Row::from_index(r))
                .unwrap()
                .rewrite(&mut line, |l| l, &mut pairs);
            assert_eq!(pairs.len(), cols, "row {r}: a pair a column");
            let ids: Vec<usize> = line.iter().map(|c| c.style.index()).collect();
            assert_eq!(ids, (0..cols).collect::<Vec<_>>(), "row {r}: its own ids");
            assert!(base(&w, r) + pairs.len() <= w.loose().end, "row {r} fits");
        }
        assert_eq!(
            base(&w, 3) + pairs.len(),
            w.loose().end,
            "filled at the bound"
        );
    }

    /// **A pair a line names twice is one entry**: the same band's id in
    /// two runs, and the same id in two bands, two.
    #[test]
    fn a_joined_line_numbers_each_pair_once() {
        let (a, b) = (Band::of(1), Band::of(2));
        let j = Joined {
            row: Row::ZERO,
            runs: vec![(0..2, a), (2..4, b)],
            struck: vec![],
        };
        let mut line: Vec<PackedCell> = [7, 7, 7, 9]
            .iter()
            .map(|&s| PackedCell {
                style: StyleId::from_index(s),
                ..PackedCell::EMPTY
            })
            .collect();
        let mut pairs = Vec::new();
        j.rewrite(&mut line, |l| l, &mut pairs);
        let s7 = StyleId::from_index(7);
        let s9 = StyleId::from_index(9);
        let no = Strike::No;
        assert_eq!(pairs, vec![(a, s7, no), (b, s7, no), (b, s9, no)]);
        let ids: Vec<usize> = line.iter().map(|c| c.style.index()).collect();
        assert_eq!(ids, vec![0, 0, 1, 2]);
    }
}
