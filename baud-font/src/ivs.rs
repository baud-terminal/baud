//! The item variation store and delta-set index maps: how `HVAR` (step 3
//! of note Z) and `GDEF`'s device tables (step 4) vary a value.
//!
//! The formats are `[[source: ms-opentype-otvarcommonformats]]`. Where it
//! leaves a choice -- the arithmetic, what a region that is not a region
//! scores, what malformed data does -- this follows HarfBuzz 12.3.0's
//! `ItemVariationStore`, `VarData`, `VarRegionList` and `DeltaSetIndexMap`
//! (`[[source: harfbuzz-12-3-0-layout-common]]`): scalars and deltas in
//! `f32`, a region past the list scoring 0, a region whose start, peak and
//! end are out of order or straddle 0 scoring 1, and `regionCount` read as
//! all sixteen bits -- the format calls it 15 bits wide and HarfBuzz's
//! `HBUINT15` does not mask the top one (a TODO in
//! `[[source: harfbuzz-12-3-0-open-type]]`).
//!
//! **Malformed data refuses the whole table.** HarfBuzz 12.3.0's
//! `sanitize_blob` replaces a table that fails any check with the empty
//! blob -- it no longer rewrites a bad offset to null
//! (`[[source: harfbuzz-12-3-0-sanitize]]`) -- so a store, a data set, a
//! region list or a map that does not check out takes the table that owns
//! it with it. `tools/baud-oracle`'s `advance` shows it on four broken `HVAR`s.
//! An offset of 0 is not malformed: it is a null subtable, which varies
//! nothing. Every subtable is checked when the table is parsed, and read
//! lazily afterwards.

use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::var::{Coords, F2Dot14};

/// An index into a store: the outer (data set) index in the high 16 bits
/// and the inner (item) index in the low 16, as a delta-set index map
/// packs it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VarIndex(u32);

impl VarIndex {
    /// A VariationIndex device's index: its first two fields, the outer
    /// and the inner half.
    pub(crate) fn of_device(device: Bytes<'_>) -> Option<VarIndex> {
        let outer: u16 = device.read(Offset::ZERO)?;
        let inner: u16 = device.read(Offset::of(2u32))?;
        Some(VarIndex((u32::from(outer) << 16) | u32::from(inner)))
    }

    /// `NO_VARIATION_INDEX`, which varies nothing.
    pub(crate) fn is_none(self) -> bool {
        self.0 == 0xFFFF_FFFF
    }

    fn outer(self) -> Index {
        Index::of(self.0 >> 16)
    }

    fn inner(self) -> Index {
        Index::of(self.0 & 0xFFFF)
    }
}

/// The store: its region list and its data sets.
#[derive(Clone, Copy)]
pub struct Store<'a> {
    data: Bytes<'a>,
    regions: Option<Regions<'a>>,
    sets: Array<'a, u32>,
}

impl<'a> Store<'a> {
    /// Format 1: variationRegionListOffset at 2, itemVariationDataCount at
    /// 6, the data offsets from 8. `None` when the header or the offset
    /// array is out of range or the format is not 1.
    pub fn parse(t: Bytes<'a>) -> Option<Store<'a>> {
        let format: u16 = t.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let regions: u32 = t.read(Offset::of(2u32))?;
        let count: u16 = t.read(Offset::of(6u32))?;
        let sets: Array<'a, u32> = t.array(Offset::of(8u32), Count::of(count))?;
        let regions = match Subtable::at(t, Offset::of(regions))? {
            Subtable::Null => None,
            Subtable::At(b) => Some(Regions::parse(b)?),
        };
        let mut i = Index::ZERO;
        while sets.count().holds(i) {
            if let Subtable::At(b) = Subtable::at(t, Offset::of(sets.get(i)?))? {
                Data::parse(b)?;
            }
            i = i.next();
        }
        Some(Store {
            data: t,
            regions,
            sets,
        })
    }

    /// The delta `index` names at `coords`: 0 for an index past the store,
    /// a null data set, or a store with no regions.
    pub fn delta(&self, index: VarIndex, coords: &Coords) -> f32 {
        let Some(offset) = self.sets.get(index.outer()) else {
            return 0.0;
        };
        let Some(Subtable::At(b)) = Subtable::at(self.data, Offset::of(offset)) else {
            return 0.0;
        };
        let Some(set) = Data::parse(b) else {
            return 0.0;
        };
        set.delta(index.inner(), coords, self.regions)
    }
}

impl Store<'_> {
    /// How many data sets the store holds: the `vsindex` values a CFF2
    /// table may name.
    pub(crate) fn sets(&self) -> Count {
        self.sets.count()
    }

    /// Data set `outer`'s regions at `coords`, for a CFF2 `blend`, which
    /// uses a data set's region list and none of its rows
    /// (`[[source: ms-opentype-cff2]]`, "VariationStore"): writes each
    /// region's scalar into `out` in the set's order and answers how many
    /// there are, `k`. A null data set names no regions, as HarfBuzz reads
    /// it. `None` for a set past the store, or more regions than `out`
    /// holds -- which no `blend` of one value could use on a 513-deep
    /// stack.
    pub(crate) fn scalars(&self, outer: Index, coords: &Coords, out: &mut [f32]) -> Option<Count> {
        let offset = self.sets.get(outer)?;
        let Subtable::At(b) = Subtable::at(self.data, Offset::of(offset))? else {
            return Some(Count::ZERO);
        };
        let set = Data::parse(b)?;
        let k = set.regions.count();
        let out = out.get_mut(..k.slot())?;
        let mut i = Index::ZERO;
        for slot in out {
            let r = Index::of(set.regions.get(i)?);
            *slot = self.regions.map_or(0.0, |list| list.evaluate(r, coords));
            i = i.next();
        }
        Some(k)
    }

    /// Item `index`'s row, column by column: each column's region -- its
    /// start, peak and end on the one axis -- and its delta, for the bake,
    /// which sums them as fontTools does. **Strict**: an index past the
    /// store, a null or malformed data set, a region past the list or a
    /// store over more than one axis answers `None`.
    pub(crate) fn columns(
        &self,
        index: VarIndex,
        mut f: impl FnMut([F2Dot14; 3], i32) -> Option<()>,
    ) -> Option<()> {
        let offset = self.sets.get(index.outer())?;
        let Subtable::At(b) = Subtable::at(self.data, Offset::of(offset))? else {
            return None;
        };
        let set = Data::parse(b)?;
        let regions = self.regions?;
        if regions.axes != Count::of(1u32) || !set.items.holds(index.inner()) {
            return None;
        }
        let row = set
            .rows
            .slice(Offset::ZERO.at(index.inner(), set.row)?, set.row)?;
        let (wide, narrow) = match set.long {
            RowWidth::Long => (Len::of(4), Len::of(2)),
            RowWidth::Short => (Len::of(2), Len::of(1)),
        };
        let mut at = Offset::ZERO;
        let mut i = Index::ZERO;
        while set.regions.count().holds(i) {
            let is_wide = set.words.holds(i);
            let value: i32 = match (set.long, is_wide) {
                (RowWidth::Long, true) => row.read::<i32>(at)?,
                (RowWidth::Long, false) | (RowWidth::Short, true) => {
                    i32::from(row.read::<i16>(at)?)
                }
                (RowWidth::Short, false) => i32::from(i8::from_ne_bytes([row.read::<u8>(at)?])),
            };
            at = at.plus(if is_wide { wide } else { narrow })?;
            let r = Index::of(set.regions.get(i)?);
            if !regions.count.holds(r) {
                return None;
            }
            let rec = Offset::ZERO.at(r, Len::of(6))?;
            let c = |k: u32| regions.records.read::<F2Dot14>(rec.plus(Len::of(k))?);
            f([c(0)?, c(2)?, c(4)?], value)?;
            i = i.next();
        }
        Some(())
    }
}

/// What an offset names: a null subtable for 0, the bytes from it
/// otherwise; `None` -- malformed -- past the data.
pub(crate) enum Subtable<'a> {
    Null,
    At(Bytes<'a>),
}

impl<'a> Subtable<'a> {
    pub(crate) fn at(t: Bytes<'a>, offset: Offset) -> Option<Subtable<'a>> {
        if offset == Offset::ZERO {
            return Some(Subtable::Null);
        }
        t.tail(offset).map(Subtable::At)
    }
}

/// `VarRegionList`: `axisCount` regions of three 2.14 coordinates per axis.
#[derive(Clone, Copy)]
struct Regions<'a> {
    axes: Count,
    count: Count,
    records: Bytes<'a>,
}

impl<'a> Regions<'a> {
    fn parse(t: Bytes<'a>) -> Option<Regions<'a>> {
        let axes: u16 = t.read(Offset::ZERO)?;
        let count: u16 = t.read(Offset::of(2u32))?;
        let (axes, count) = (Count::of(axes), Count::of(count));
        let each = Len::of(6).times(axes)?;
        let records = t.slice(Offset::of(4u32), each.times(count)?)?;
        Some(Regions {
            axes,
            count,
            records,
        })
    }

    /// Region `r`'s scalar at `coords`, as HarfBuzz evaluates it: 0 for a
    /// region past the list, the product of the axes' factors otherwise,
    /// stopping at the first 0.
    fn evaluate(&self, r: Index, coords: &Coords) -> f32 {
        if !self.count.holds(r) {
            return 0.0;
        }
        let each = Len::of(6).times(self.axes);
        let Some(region) = each
            .and_then(|each| Some((Offset::ZERO.at(r, each)?, each)))
            .and_then(|(at, each)| self.records.slice(at, each))
        else {
            return 0.0;
        };
        let mut v = 1.0f32;
        let mut a = Index::ZERO;
        while self.axes.holds(a) {
            let axis = |k: u32| -> F2Dot14 {
                Offset::ZERO
                    .at(a, Len::of(6))
                    .and_then(|o| o.plus(Len::of(k)))
                    .and_then(|o| region.read::<F2Dot14>(o))
                    .unwrap_or(F2Dot14::ZERO)
            };
            let factor = factor([axis(0), axis(2), axis(4)], coords.get(a));
            if factor == 0.0 {
                return 0.0;
            }
            v *= factor;
            a = a.next();
        }
        v
    }
}

/// One axis of a region at `coord`: HarfBuzz's `VarRegionAxis::evaluate`,
/// its tests in its order.
fn factor(region: [F2Dot14; 3], coord: F2Dot14) -> f32 {
    let [start, peak, end] = region.map(|c| i32::from(c.0));
    let coord = i32::from(coord.0);
    if peak == 0 || coord == peak {
        return 1.0;
    }
    if coord == 0 {
        return 0.0;
    }
    if start > peak || peak > end {
        return 1.0;
    }
    if start < 0 && end > 0 && peak != 0 {
        return 1.0;
    }
    if coord <= start || end <= coord {
        return 0.0;
    }
    if coord < peak {
        (coord - start) as f32 / (peak - start) as f32
    } else {
        (end - coord) as f32 / (end - peak) as f32
    }
}

/// `ItemVariationData`: rows of deltas, one a region the set names, the
/// first `wordCount` wide (32 bits under LONG_WORDS, 16 otherwise) and the
/// rest narrow (16 or 8).
#[derive(Clone, Copy)]
struct Data<'a> {
    items: Count,
    words: Count,
    long: RowWidth,
    regions: Array<'a, u16>,
    rows: Bytes<'a>,
    row: Len,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowWidth {
    Short,
    Long,
}

const LONG_WORDS: u16 = 0x8000;
const WORD_COUNT: u16 = 0x7FFF;

impl<'a> Data<'a> {
    /// itemCount 0, wordDeltaCount 2, regionIndexCount 4, the indices from
    /// 6, then the rows. `None` -- a null set -- when a count does not fit:
    /// more words than regions, or rows past the data.
    fn parse(t: Bytes<'a>) -> Option<Data<'a>> {
        let items: u16 = t.read(Offset::ZERO)?;
        let packed: u16 = t.read(Offset::of(2u32))?;
        let n: u16 = t.read(Offset::of(4u32))?;
        let (items, n) = (Count::of(items), Count::of(n));
        let words = Count::of(packed & WORD_COUNT);
        if words > n {
            return None;
        }
        let long = if packed & LONG_WORDS != 0 {
            RowWidth::Long
        } else {
            RowWidth::Short
        };
        let regions = t.array(Offset::of(6u32), n)?;
        let unit = Len::of(if long == RowWidth::Long { 2 } else { 1 });
        let row = unit.times(Count::of(words.value().checked_add(n.value())?))?;
        let at = Offset::of(6u32).plus(Len::of(2).times(n)?)?;
        let rows = t.slice(at, row.times(items)?)?;
        Some(Data {
            items,
            words,
            long,
            regions,
            rows,
            row,
        })
    }

    /// Item `inner`'s delta: 0 past the set or with no regions, otherwise
    /// each region's scalar times its column, summed in column order, a
    /// column whose scalar is 0 skipped -- HarfBuzz's `VarData::get_delta`.
    fn delta(&self, inner: Index, coords: &Coords, regions: Option<Regions<'_>>) -> f32 {
        if self.regions.count().is_zero() || !self.items.holds(inner) {
            return 0.0;
        }
        let Some(row) = Offset::ZERO
            .at(inner, self.row)
            .and_then(|at| self.rows.slice(at, self.row))
        else {
            return 0.0;
        };
        let (wide, narrow) = match self.long {
            RowWidth::Long => (Len::of(4), Len::of(2)),
            RowWidth::Short => (Len::of(2), Len::of(1)),
        };
        let mut delta = 0.0f32;
        let mut at = Offset::ZERO;
        let mut i = Index::ZERO;
        while self.regions.count().holds(i) {
            let is_wide = self.words.holds(i);
            let value: Option<i32> = match (self.long, is_wide) {
                (RowWidth::Long, true) => row.read::<i32>(at),
                (RowWidth::Long, false) | (RowWidth::Short, true) => {
                    row.read::<i16>(at).map(i32::from)
                }
                (RowWidth::Short, false) => row
                    .read::<u8>(at)
                    .map(|b| i32::from(i8::from_ne_bytes([b]))),
            };
            at = match at.plus(if is_wide { wide } else { narrow }) {
                Some(next) => next,
                None => return delta,
            };
            let r = self.regions.get(i).map_or(Index::of(u32::MAX), Index::of);
            let scalar = regions.map_or(0.0, |list| list.evaluate(r, coords));
            if scalar != 0.0 {
                delta += scalar * value.unwrap_or(0) as f32;
            }
            i = i.next();
        }
        delta
    }
}

/// `DeltaSetIndexMap`: glyph (or other) index to [`VarIndex`]. Format 0
/// counts its entries in 16 bits, format 1 in 32; any other format is the
/// identity, as a null map is -- HarfBuzz checks nothing more of it.
#[derive(Clone, Copy)]
pub struct Map<'a> {
    count: Count,
    width: Len,
    inner_bits: Count,
    entries: Bytes<'a>,
}

impl<'a> Map<'a> {
    /// `None` when the entries run past the data. A format other than 0
    /// and 1 parses as the identity.
    pub fn parse(t: Bytes<'a>) -> Option<Map<'a>> {
        let format: u8 = t.read(Offset::ZERO)?;
        let entry: u8 = t.read(Offset::of(1u32))?;
        let (count, at) = match format {
            0 => (
                Count::of(t.read::<u16>(Offset::of(2u32))?),
                Offset::of(4u32),
            ),
            1 => (
                Count::of(t.read::<u32>(Offset::of(2u32))?),
                Offset::of(6u32),
            ),
            _ => (Count::ZERO, Offset::of(2u32)),
        };
        let width = Len::of(u32::from((entry >> 4) & 3) + 1);
        let entries = t.slice(at, width.times(count)?)?;
        Some(Map {
            count,
            width,
            inner_bits: Count::of(u32::from(entry & 0xF) + 1),
            entries,
        })
    }

    /// What `v` maps to: `v` itself for an empty map, otherwise entry `v`,
    /// or the last entry past the map, repacked from `inner_bits` into
    /// outer.inner.
    pub fn map(map: Option<Map<'_>>, v: Index) -> VarIndex {
        let direct = VarIndex(v.value());
        let Some(m) = map.filter(|m| !m.count.is_zero()) else {
            return direct;
        };
        let Some(last) = m.count.last() else {
            return direct;
        };
        let i = v.min(last);
        let Some(bytes) = Offset::ZERO
            .at(i, m.width)
            .and_then(|at| m.entries.slice(at, m.width))
        else {
            return direct;
        };
        let mut u = 0u32;
        let mut k = Offset::ZERO;
        while let Some(b) = bytes.read::<u8>(k) {
            u = (u << 8) | u32::from(b);
            k = match k.plus(Len::of(1)) {
                Some(next) => next,
                None => break,
            };
        }
        let n = m.inner_bits.value();
        VarIndex(((u >> n) << 16) | (u & ((1 << n) - 1)))
    }
}
