//! `gvar`: how each glyph's outline points move across the variation space.
//! This module reads the table and applies one glyph's
//! tuple variations to its points in caller-provided scratch; `glyf` walks the
//! glyph, flattens its components and draws the result.
//!
//! The formats are `[[source: ms-opentype-gvar]]` and
//! `[[source: ms-opentype-otvarcommonformats]]`. Where they leave a choice --
//! the order of the scalar's tests, when deltas are flushed into the points,
//! which positions inference reads, what malformed data does -- this follows
//! HarfBuzz 12.3.0: `accelerator_t::apply_deltas_to_points` in `[[source: harfbuzz-12-3-0-gvar]]`,
//! `calculate_scalar` and the packed decoders in
//! `[[source: harfbuzz-12-3-0-var-common]]`, the 32-bit delta runs in
//! `[[source: harfbuzz-12-3-0-open-type]]`. Deltas stay `f32` as HarfBuzz
//! keeps them on its drawing path; nothing is rounded.
//!
//! **Malformed data varies what came before it.** A tuple whose data is
//! truncated, or whose point numbers or deltas overrun, stops the glyph's
//! variation: the deltas already flushed into the points stay and the pending
//! ones are dropped, which is HarfBuzz's result (its caller ignores the
//! failure). A malformed shared point list or first header applies nothing. A
//! tuple whose scalar is 0 is skipped before its data is looked at, and a
//! shared-tuple index past the table scores 0 -- both HarfBuzz's.

use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::glyf::{LastPoint, VarPoint};
use crate::var::{Coords, F2Dot14};
use baud_domain::GlyphId;

/// The table: its shared tuples and where each glyph's data lies.
#[derive(Clone, Copy)]
pub struct Gvar<'a> {
    data: Bytes<'a>,
    axes: Count,
    shared: Bytes<'a>,
    shared_count: Count,
    glyphs: Count,
    offsets: Bytes<'a>,
    width: OffsetWidth,
    array: Offset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OffsetWidth {
    /// Offset16, the stored value halved.
    Short,
    Long,
}

impl<'a> Gvar<'a> {
    /// Version 1: axisCount 4, sharedTupleCount 6, sharedTuplesOffset 8,
    /// glyphCount 12, flags 14, glyphVariationDataArrayOffset 16, then the
    /// offsets from 20.
    pub fn parse(t: Bytes<'a>) -> Option<Gvar<'a>> {
        let major: u16 = t.read(Offset::ZERO)?;
        if major != 1 {
            return None;
        }
        let axes: u16 = t.read(Offset::of(4u32))?;
        let shared_count: u16 = t.read(Offset::of(6u32))?;
        let shared_at: u32 = t.read(Offset::of(8u32))?;
        let glyphs: u16 = t.read(Offset::of(12u32))?;
        let flags: u16 = t.read(Offset::of(14u32))?;
        let array: u32 = t.read(Offset::of(16u32))?;
        let (axes, shared_count, glyphs) =
            (Count::of(axes), Count::of(shared_count), Count::of(glyphs));
        let tuple = Len::of(2).times(axes)?;
        let shared = t.slice(Offset::of(shared_at), tuple.times(shared_count)?)?;
        let width = if flags & 1 == 0 {
            OffsetWidth::Short
        } else {
            OffsetWidth::Long
        };
        let each = Len::of(if width == OffsetWidth::Short { 2 } else { 4 });
        let entries = Count::of(glyphs.value().checked_add(1)?);
        let offsets = t.slice(Offset::of(20u32), each.times(entries)?)?;
        Some(Gvar {
            data: t,
            axes,
            shared,
            shared_count,
            glyphs,
            offsets,
            width,
            array: Offset::of(array),
        })
    }

    pub fn axes(&self) -> Count {
        self.axes
    }

    fn offset(&self, i: Index) -> Option<Offset> {
        let v = match self.width {
            OffsetWidth::Short => {
                let v: u16 = self.offsets.read(Offset::ZERO.at(i, Len::of(2))?)?;
                u32::from(v).checked_mul(2)?
            }
            OffsetWidth::Long => self.offsets.read(Offset::ZERO.at(i, Len::of(4))?)?,
        };
        self.array.plus(Len::of(v))
    }

    /// Glyph `g`'s variation data; `None` when it has none -- an empty
    /// entry, a glyph past the table, or no tuples -- and its outline never
    /// varies.
    pub fn glyph(&self, g: GlyphId) -> Option<Store<'a>> {
        let i = Index::of(u32::try_from(g.index()).ok()?);
        if !self.glyphs.holds(i) {
            return None;
        }
        let (a, b) = (self.offset(i)?, self.offset(i.next())?);
        let len = b.since(a)?;
        if len.is_zero() {
            return None;
        }
        Store::parse(self.data.slice(a, len)?, *self)
    }

    /// Shared tuple `i`, as the peak of a tuple variation.
    fn shared(&self, i: Index) -> Option<Tuple<'a>> {
        if !self.shared_count.holds(i) {
            return None;
        }
        let each = Len::of(2).times(self.axes)?;
        Some(Tuple(self.shared.slice(Offset::ZERO.at(i, each)?, each)?))
    }
}

/// A tuple record: one 2.14 coordinate per axis.
#[derive(Clone, Copy)]
pub(crate) struct Tuple<'a>(Bytes<'a>);

impl Tuple<'_> {
    pub(crate) fn at(&self, axis: Index) -> F2Dot14 {
        Offset::ZERO
            .at(axis, Len::of(2))
            .and_then(|o| self.0.read(o))
            .unwrap_or(F2Dot14::ZERO)
    }
}

/// One glyph's tuple variation store.
#[derive(Clone, Copy)]
pub struct Store<'a> {
    data: Bytes<'a>,
    gvar: Gvar<'a>,
    count: Count,
    serialized: Offset,
    shared_points: SharedPoints,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SharedPoints {
    Present,
    Absent,
}

const SHARED_POINT_NUMBERS: u16 = 0x8000;
const COUNT_MASK: u16 = 0x0FFF;
const EMBEDDED_PEAK_TUPLE: u16 = 0x8000;
const INTERMEDIATE_REGION: u16 = 0x4000;
const PRIVATE_POINT_NUMBERS: u16 = 0x2000;
const TUPLE_INDEX_MASK: u16 = 0x0FFF;

impl<'a> Store<'a> {
    fn parse(data: Bytes<'a>, gvar: Gvar<'a>) -> Option<Store<'a>> {
        let packed: u16 = data.read(Offset::ZERO)?;
        let serialized: u16 = data.read(Offset::of(2u32))?;
        let count = Count::of(packed & COUNT_MASK);
        if count.is_zero() {
            return None;
        }
        Some(Store {
            data,
            gvar,
            count,
            serialized: Offset::of(serialized),
            shared_points: if packed & SHARED_POINT_NUMBERS != 0 {
                SharedPoints::Present
            } else {
                SharedPoints::Absent
            },
        })
    }

    /// Header `at`'s full size, when it is inside the store: HarfBuzz's
    /// `is_valid`, which ends the walk when it fails.
    fn header(&self, at: Offset) -> Option<Header<'a>> {
        let tuple = Len::of(2).times(self.gvar.axes)?;
        let size: u16 = self.data.read(at)?;
        let index: u16 = self.data.read(at.plus(Len::of(2))?)?;
        let mut end = at.plus(Len::of(4))?;
        let peak = if index & EMBEDDED_PEAK_TUPLE != 0 {
            let t = Tuple(self.data.slice(end, tuple)?);
            end = end.plus(tuple)?;
            Some(t)
        } else {
            // Out of range scores 0, rather than ending the walk.
            self.gvar.shared(Index::of(index & TUPLE_INDEX_MASK))
        };
        let region = if index & INTERMEDIATE_REGION != 0 {
            let start = Tuple(self.data.slice(end, tuple)?);
            let stop = Tuple(self.data.slice(end.plus(tuple)?, tuple)?);
            end = end.plus(tuple)?.plus(tuple)?;
            Some((start, stop))
        } else {
            None
        };
        Some(Header {
            size: Len::of(u32::from(size)),
            peak,
            region,
            private: if index & PRIVATE_POINT_NUMBERS != 0 {
                SharedPoints::Absent
            } else {
                SharedPoints::Present
            },
            end,
        })
    }

    /// Moves `pts` -- the glyph's points, then its four phantom points -- by
    /// its tuple variations at `coords`, as HarfBuzz's
    /// `accelerator_t::apply_deltas_to_points` does. The points' `last` marks contour ends
    /// for inference; the phantoms carry none, so nothing is ever inferred
    /// for them.
    pub(crate) fn apply(&self, coords: &Coords, pts: &mut [VarPoint], track: Track) {
        let total = match track {
            Track::All => Count::of(u32::try_from(pts.len()).unwrap_or(u32::MAX)),
            Track::Phantoms(total) => total,
        };
        let mut data_at = self.serialized;
        let shared = match self.shared_points {
            SharedPoints::Present => match Points::read(self.data, self.serialized) {
                Some(p) => {
                    data_at = p.end;
                    Some(p)
                }
                None => return,
            },
            SharedPoints::Absent => None,
        };
        for p in pts.iter_mut() {
            p.dx = 0.0;
            p.dy = 0.0;
            p.referenced = Referenced::No;
        }
        let mut orig = Orig::NotTaken;
        let mut flush = Flush::No;
        let mut header_at = Offset::of(4u32);
        let mut left = self.count;
        while !left.is_zero() {
            let Some(h) = self.header(header_at) else {
                break;
            };
            left = Count::of(left.value() - 1);
            header_at = h.end;
            let this = data_at;
            // HarfBuzz advances by the data size whatever the tuple scores.
            let Some(next) = data_at.plus(h.size) else {
                return;
            };
            data_at = next;
            let scalar = match h.peak {
                Some(peak) => scalar(coords, self.gvar.axes, peak, h.region),
                None => 0.0,
            };
            if scalar == 0.0 {
                continue;
            }
            let Some(own) = self.data.slice(this, h.size) else {
                return;
            };
            let (indices, deltas_at) = match h.private {
                SharedPoints::Absent => match Points::read(own, Offset::ZERO) {
                    Some(p) => (Some(p), p.end),
                    None => return,
                },
                // No private numbers: the shared list, or -- when there is
                // none -- every point, which is HarfBuzz's empty list.
                SharedPoints::Present => (shared, Offset::ZERO),
            };
            let indices = indices.filter(|p| !p.count.is_zero());
            let n = match &indices {
                Some(p) => p.count,
                None => total,
            };
            let Some(xs) = Deltas::bounded(own, deltas_at, n) else {
                return;
            };
            let Some(ys) = Deltas::bounded(own, xs.end, n) else {
                return;
            };
            if indices.is_some() {
                if orig == Orig::NotTaken && track == Track::All {
                    for p in pts.iter_mut() {
                        p.ox = p.x;
                        p.oy = p.y;
                    }
                    orig = Orig::Taken;
                }
                // HarfBuzz flushes here and sets its flag again below.
                if flush == Flush::Yes {
                    translate(pts);
                }
                for p in pts.iter_mut() {
                    p.dx = 0.0;
                    p.dy = 0.0;
                    p.referenced = Referenced::No;
                }
            }
            let (mut xs, mut ys) = (xs.reader, ys.reader);
            match indices {
                None => {
                    let mut k = Count::ZERO;
                    while k != n {
                        let (x, y) = (xs.next(), ys.next());
                        if let Some(p) = track
                            .place(k.as_index(), total)
                            .and_then(|i| pts.get_mut(i.slot()))
                        {
                            p.dx += x as f32 * scalar;
                            p.dy += y as f32 * scalar;
                        }
                        k = Count::of(k.value() + 1);
                    }
                }
                Some(list) => {
                    let mut ix = list.iter();
                    let mut k = Count::ZERO;
                    while k != n {
                        k = Count::of(k.value() + 1);
                        let (i, x, y) = (Points::next_from(&mut ix), xs.next(), ys.next());
                        let Some(p) = track.place(i, total).and_then(|i| pts.get_mut(i.slot()))
                        else {
                            continue;
                        };
                        p.referenced = Referenced::Yes;
                        p.dx += x as f32 * scalar;
                        p.dy += y as f32 * scalar;
                    }
                    if track == Track::All {
                        infer(pts);
                    }
                }
            }
            flush = Flush::Yes;
        }
        if flush == Flush::Yes {
            translate(pts);
        }
    }
}

/// One tuple variation, whole, as the bake reads it: where it peaks, its
/// intermediate region, the points it names -- `None` for every point -- and
/// its two delta streams, `n` values each.
pub(crate) struct TupleData<'a> {
    pub(crate) peak: Tuple<'a>,
    pub(crate) region: Option<(Tuple<'a>, Tuple<'a>)>,
    pub(crate) points: Option<Points<'a>>,
    pub(crate) n: Count,
    pub(crate) xs: DeltaIter<'a>,
    pub(crate) ys: DeltaIter<'a>,
}

impl<'a> Store<'a> {
    /// Every tuple of the store, in order, for a glyph of `total` points --
    /// its phantoms included. **Strict where [`Store::apply`] forgives**:
    /// malformed data anywhere, or a shared-tuple index past the table,
    /// answers `None` and the caller refuses the glyph, because the bake
    /// writes a font and a guess about a malformed tuple would be written
    /// into it. `f` answering `None` stops the walk the same way.
    pub(crate) fn tuples(
        &self,
        total: Count,
        mut f: impl FnMut(TupleData<'a>) -> Option<()>,
    ) -> Option<()> {
        let mut data_at = self.serialized;
        let shared = match self.shared_points {
            SharedPoints::Present => {
                let p = Points::read(self.data, self.serialized)?;
                data_at = p.end;
                Some(p)
            }
            SharedPoints::Absent => None,
        };
        let mut header_at = Offset::of(4u32);
        let mut left = self.count;
        while !left.is_zero() {
            let h = self.header(header_at)?;
            left = Count::of(left.value() - 1);
            header_at = h.end;
            let own = self.data.slice(data_at, h.size)?;
            data_at = data_at.plus(h.size)?;
            let (points, deltas_at) = match h.private {
                SharedPoints::Absent => {
                    let p = Points::read(own, Offset::ZERO)?;
                    (Some(p), p.end)
                }
                SharedPoints::Present => (shared, Offset::ZERO),
            };
            let points = points.filter(|p| !p.count.is_zero());
            let n = points.map_or(total, |p| p.count);
            let xs = Deltas::bounded(own, deltas_at, n)?;
            let ys = Deltas::bounded(own, xs.end, n)?;
            f(TupleData {
                peak: h.peak?,
                region: h.region,
                points,
                n,
                xs: xs.reader,
                ys: ys.reader,
            })?;
        }
        Some(())
    }
}

/// Which of a glyph's points [`Store::apply`] moves: all of them, or only
/// its four phantom points -- HarfBuzz's `phantom_only`, which an advance
/// reads. The glyph's own point count is carried for the phantoms, so an
/// index is read against the whole glyph and a delta run for every point
/// is still walked in full.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Track {
    All,
    /// The glyph's points, phantoms included; `pts` holds the last four.
    Phantoms(Count),
}

impl Track {
    /// Where point `i` of a glyph of `total` points lives in `pts`, if it
    /// is tracked at all.
    fn place(self, i: Index, total: Count) -> Option<Index> {
        match self {
            Track::All => Some(i),
            Track::Phantoms(_) => {
                let first = Count::of(total.value().checked_sub(4)?).as_index();
                Some(i.since(first)?.as_index())
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Orig {
    NotTaken,
    Taken,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flush {
    No,
    Yes,
}

/// Whether a sparse tuple named a point, which decides whether inference
/// fills it in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Referenced {
    No,
    Yes,
}

struct Header<'a> {
    size: Len,
    /// `None` for a shared-tuple index past the table: it scores 0.
    peak: Option<Tuple<'a>>,
    region: Option<(Tuple<'a>, Tuple<'a>)>,
    /// `Absent` when the tuple carries its own point numbers.
    private: SharedPoints,
    end: Offset,
}

fn translate(pts: &mut [VarPoint]) {
    for p in pts.iter_mut() {
        p.x += p.dx;
        p.y += p.dy;
    }
}

/// Inferred deltas for the points a sparse tuple left out, contour by
/// contour, from the default positions: HarfBuzz's loop, spelled the same way
/// so the gaps it finds and the order it fills them in are its own.
fn infer(pts: &mut [VarPoint]) {
    let count = pts.len();
    let mut start = 0usize;
    let mut end = 0usize;
    loop {
        while end < count && pts[end].last != LastPoint::Yes {
            end += 1;
        }
        if end == count {
            break;
        }
        let referenced = pts[start..=end]
            .iter()
            .filter(|p| p.referenced == Referenced::Yes)
            .count();
        let mut unref = (end - start + 1) - referenced;
        let next = |i: usize| if i >= end { start } else { i + 1 };
        if unref != 0 && unref <= end - start {
            let mut j = start;
            'gaps: loop {
                let mut i;
                loop {
                    i = j;
                    j = next(i);
                    if pts[i].referenced == Referenced::Yes && pts[j].referenced == Referenced::No {
                        break;
                    }
                }
                let prev = i;
                j = i;
                loop {
                    i = j;
                    j = next(i);
                    if pts[i].referenced == Referenced::No && pts[j].referenced == Referenced::Yes {
                        break;
                    }
                }
                let nxt = j;
                i = prev;
                loop {
                    i = next(i);
                    if i == nxt {
                        break;
                    }
                    let (p, n) = (pts[prev], pts[nxt]);
                    let dx = infer_one(pts[i].ox, (p.ox, p.dx), (n.ox, n.dx));
                    let dy = infer_one(pts[i].oy, (p.oy, p.dy), (n.oy, n.dy));
                    pts[i].dx = dx;
                    pts[i].dy = dy;
                    unref -= 1;
                    if unref == 0 {
                        break 'gaps;
                    }
                }
            }
        }
        end += 1;
        start = end;
    }
}

/// HarfBuzz's `infer_delta` on one axis: `t` is the target's default
/// position, `prev` and `next` the referenced neighbours' default position
/// and delta. Between them, interpolated; outside them, the nearer one's
/// delta; between two points at one position, their delta if they agree,
/// else 0.
fn infer_one(t: f32, prev: (f32, f32), next: (f32, f32)) -> f32 {
    let ((pv, pd), (nv, nd)) = (prev, next);
    if pv == nv {
        if pd == nd { pd } else { 0.0 }
    } else if t <= pv.min(nv) {
        if pv < nv { pd } else { nd }
    } else if t >= pv.max(nv) {
        if pv > nv { pd } else { nd }
    } else {
        let r = (t - pv) / (nv - pv);
        pd + r * (nd - pd)
    }
}

/// A tuple variation's scalar at `coords`, as HarfBuzz computes it
/// (`TupleVariationHeader::calculate_scalar`): in `f64`, handed on as `f32`,
/// the order of its tests kept. An axis whose peak is 0 is ignored; a
/// coordinate of 0 on an axis with a peak ends the tuple, before its region
/// is asked whether it is valid; an invalid intermediate region ignores the
/// axis. `region` is the intermediate start and end; without one the region
/// runs from 0 to the peak.
fn scalar(
    coords: &Coords,
    axes: Count,
    peak: Tuple<'_>,
    region: Option<(Tuple<'_>, Tuple<'_>)>,
) -> f32 {
    let mut s = 1.0f64;
    let mut i = Index::ZERO;
    while axes.holds(i) {
        let axis = i;
        i = i.next();
        let p = i32::from(peak.at(axis).0);
        let v = i32::from(coords.get(axis).0);
        if p == 0 || v == p {
            continue;
        }
        if v == 0 {
            return 0.0;
        }
        let Some((a, b)) = region else {
            if v < p.min(0) || v > p.max(0) {
                return 0.0;
            }
            s *= f64::from(v) / f64::from(p);
            continue;
        };
        let (start, end) = (i32::from(a.at(axis).0), i32::from(b.at(axis).0));
        if start > p || p > end || (start < 0 && end > 0) {
            continue;
        }
        if v < start || v > end {
            return 0.0;
        }
        if v < p {
            if p != start {
                s *= f64::from(v - start) / f64::from(p - start);
            }
        } else if p != end {
            s *= f64::from(end - v) / f64::from(end - p);
        }
    }
    // HarfBuzz's `float scalar = ...calculate_scalar(...)`.
    s as f32
}

/// A packed point-number list, validated whole when read: HarfBuzz's
/// `decompile_points`. A count of 0 means every point.
#[derive(Clone, Copy)]
pub(crate) struct Points<'a> {
    data: Bytes<'a>,
    runs: Offset,
    count: Count,
    end: Offset,
}

impl<'a> Points<'a> {
    fn read(data: Bytes<'a>, at: Offset) -> Option<Points<'a>> {
        let first: u8 = data.read(at)?;
        let mut runs = at.plus(Len::of(1))?;
        let count = if first & 0x80 == 0 {
            Count::of(first)
        } else {
            let second: u8 = data.read(runs)?;
            runs = runs.plus(Len::of(1))?;
            Count::of((u16::from(first & 0x7F) << 8) | u16::from(second))
        };
        let mut p = Points {
            data,
            runs,
            count,
            end: runs,
        };
        // Walk it once to validate it and find where it ends; a run that
        // overruns the count is malformed, as HarfBuzz reads it.
        let mut it = p.iter();
        let mut left = count;
        while !left.is_zero() {
            // A run that overruns the count is refused here.
            left = left.minus(it.run()?)?;
            it.skip()?;
        }
        p.end = it.at;
        Some(p)
    }

    pub(crate) fn iter(&self) -> PointIter<'a> {
        PointIter {
            data: self.data,
            at: self.runs,
            in_run: Count::ZERO,
            width: RunWidth::Bytes,
            last: Sum(0),
        }
    }

    /// The numbers, in order, for a list already validated.
    pub(crate) fn next_from(it: &mut PointIter<'a>) -> Index {
        it.next().unwrap_or(Index::of(u32::MAX))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunWidth {
    Bytes,
    Words,
}

/// The running sum of point-number differences. HarfBuzz sums them in an
/// `unsigned`, so the sum does not wrap at 16 bits.
#[derive(Clone, Copy)]
struct Sum(u32);

pub(crate) struct PointIter<'a> {
    data: Bytes<'a>,
    at: Offset,
    in_run: Count,
    width: RunWidth,
    last: Sum,
}

impl PointIter<'_> {
    /// Reads a run's control byte and returns its length.
    fn run(&mut self) -> Option<Count> {
        let control: u8 = self.data.read(self.at)?;
        self.at = self.at.plus(Len::of(1))?;
        self.width = if control & 0x80 != 0 {
            RunWidth::Words
        } else {
            RunWidth::Bytes
        };
        self.in_run = Count::of(u32::from(control & 0x7F) + 1);
        Some(self.in_run)
    }

    /// Steps past the current run's values, checking they are there.
    fn skip(&mut self) -> Option<()> {
        let each = match self.width {
            RunWidth::Words => Len::of(2),
            RunWidth::Bytes => Len::of(1),
        };
        let len = each.times(self.in_run)?;
        self.data.slice(self.at, len)?;
        self.at = self.at.plus(len)?;
        self.in_run = Count::ZERO;
        Some(())
    }

    fn next(&mut self) -> Option<Index> {
        if self.in_run.is_zero() {
            self.run()?;
        }
        let step = match self.width {
            RunWidth::Words => {
                let v: u16 = self.data.read(self.at)?;
                self.at = self.at.plus(Len::of(2))?;
                u32::from(v)
            }
            RunWidth::Bytes => {
                let v: u8 = self.data.read(self.at)?;
                self.at = self.at.plus(Len::of(1))?;
                u32::from(v)
            }
        };
        self.last = Sum(self.last.0.wrapping_add(step));
        self.in_run = Count::of(self.in_run.value() - 1);
        Some(Index::of(self.last.0))
    }
}

/// A packed-delta stream of exactly `n` values, validated when bounded:
/// HarfBuzz's `TupleValues::decompile`, zero, byte, word and 32-bit runs,
/// a run that overruns the count malformed.
struct Deltas<'a> {
    reader: DeltaIter<'a>,
    end: Offset,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeltaRun {
    Zero,
    Bytes,
    Words,
    Longs,
}

impl DeltaRun {
    fn width(self) -> Len {
        Len::of(match self {
            DeltaRun::Zero => 0,
            DeltaRun::Bytes => 1,
            DeltaRun::Words => 2,
            DeltaRun::Longs => 4,
        })
    }
}

impl<'a> Deltas<'a> {
    fn bounded(data: Bytes<'a>, at: Offset, n: Count) -> Option<Deltas<'a>> {
        let reader = DeltaIter {
            data,
            at,
            in_run: Count::ZERO,
            kind: DeltaRun::Zero,
        };
        let mut walk = DeltaIter { ..reader };
        let mut left = n;
        while !left.is_zero() {
            left = left.minus(walk.run()?)?;
            let len = walk.kind.width().times(walk.in_run)?;
            data.slice(walk.at, len)?;
            walk.at = walk.at.plus(len)?;
        }
        Some(Deltas {
            reader,
            end: walk.at,
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DeltaIter<'a> {
    data: Bytes<'a>,
    at: Offset,
    in_run: Count,
    kind: DeltaRun,
}

impl DeltaIter<'_> {
    fn run(&mut self) -> Option<Count> {
        let control: u8 = self.data.read(self.at)?;
        self.at = self.at.plus(Len::of(1))?;
        self.kind = match control & 0xC0 {
            0x80 => DeltaRun::Zero,
            0x40 => DeltaRun::Words,
            0xC0 => DeltaRun::Longs,
            _ => DeltaRun::Bytes,
        };
        self.in_run = Count::of(u32::from(control & 0x3F) + 1);
        Some(self.in_run)
    }

    /// The next value of a stream already validated by `bounded`; 0 past it.
    pub(crate) fn next(&mut self) -> i32 {
        if self.in_run.is_zero() && self.run().is_none() {
            return 0;
        }
        self.in_run = Count::of(self.in_run.value() - 1);
        let v = match self.kind {
            DeltaRun::Zero => Some(0),
            DeltaRun::Bytes => self
                .data
                .read::<u8>(self.at)
                .map(|b| i32::from(i8::from_ne_bytes([b]))),
            DeltaRun::Words => self.data.read::<i16>(self.at).map(i32::from),
            DeltaRun::Longs => self.data.read::<i32>(self.at),
        };
        self.at = self.at.plus(self.kind.width()).unwrap_or(self.at);
        v.unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas(b: &[u8], n: Count) -> Option<([i32; 16], Offset)> {
        let d = Deltas::bounded(Bytes::new(b), Offset::ZERO, n)?;
        let mut r = d.reader;
        let mut out = [0i32; 16];
        for v in out.iter_mut().take(n.slot()) {
            *v = r.next();
        }
        Some((out, d.end))
    }

    /// The common-formats chapter's own example: `03 0A 97 00 C6 87 41 10
    /// 22 FB 34` is 10, -105, 0, -58, eight zeroes, 4130, -1228; and the
    /// 32-bit run HarfBuzz reads for `0xC0`.
    #[test]
    fn packed_deltas_read_the_chapters_example() {
        let b = [
            0x03, 0x0A, 0x97, 0x00, 0xC6, 0x87, 0x41, 0x10, 0x22, 0xFB, 0x34,
        ];
        let (got, end) = deltas(&b, Count::of(14u32)).unwrap();
        assert_eq!(
            got[..14],
            [10, -105, 0, -58, 0, 0, 0, 0, 0, 0, 0, 0, 4130, -1228]
        );
        assert_eq!(
            end,
            Offset::of(11u32),
            "the stream ends where its last run does"
        );
        let (got, end) = deltas(&[0xC0, 0x00, 0x01, 0x00, 0x00, 0xFF], Count::of(1u32)).unwrap();
        assert_eq!((got[0], end), (65536, Offset::of(5u32)), "a 32-bit run");
    }

    /// HarfBuzz's `decompile`: a run that overruns the count is malformed,
    /// and so is a run whose values are not there.
    #[test]
    fn a_delta_run_past_its_count_or_its_data_is_refused() {
        let b = [
            0x03, 0x0A, 0x97, 0x00, 0xC6, 0x87, 0x41, 0x10, 0x22, 0xFB, 0x34,
        ];
        assert!(
            deltas(&b, Count::of(13u32)).is_none(),
            "the last run overruns 13"
        );
        assert!(
            deltas(&b[..10], Count::of(14u32)).is_none(),
            "the last word is cut"
        );
        assert!(
            deltas(&[0xC0, 0, 0, 1], Count::of(1u32)).is_none(),
            "a 32-bit value cut"
        );
    }

    fn points(b: &[u8]) -> Option<([u32; 8], Count, Offset)> {
        let p = Points::read(Bytes::new(b), Offset::ZERO)?;
        let mut out = [0u32; 8];
        let mut it = p.iter();
        for v in out.iter_mut().take(p.count.slot()) {
            *v = Points::next_from(&mut it).value();
        }
        Some((out, p.count, p.end))
    }

    /// Counts in one and two bytes, byte and word runs, the first number
    /// read from zero, later runs continuing from the last number, a
    /// repeated number kept (the chapter: deltas for it apply cumulatively),
    /// and a count of 0, which means every point.
    #[test]
    fn packed_points_decode_every_form() {
        let (v, n, end) = points(&[3, 0x02, 2, 3, 0]).unwrap();
        assert_eq!(
            (&v[..3], n, end),
            (&[2, 5, 5][..], Count::of(3u32), Offset::of(5u32))
        );
        let (v, n, end) = points(&[0x80, 0x04, 0x81, 0x01, 0x2C, 0x00, 0x01, 0x01, 4, 1]).unwrap();
        assert_eq!(
            (&v[..4], n, end),
            (
                &[300, 301, 305, 306][..],
                Count::of(4u32),
                Offset::of(10u32)
            )
        );
        let (_, n, end) = points(&[0, 0xAA]).unwrap();
        assert_eq!((n, end), (Count::ZERO, Offset::of(1u32)));
        // HarfBuzz sums in an `unsigned`: 0xFFFF then +1 is 0x10000, not 0.
        let (v, _, _) = points(&[2, 0x81, 0xFF, 0xFF, 0x00, 0x01]).unwrap();
        assert_eq!(&v[..2], &[0xFFFF, 0x10000]);
    }

    #[test]
    fn a_point_run_past_its_count_or_its_data_is_refused() {
        assert!(
            points(&[2, 0x02, 1, 1, 1]).is_none(),
            "a run of 3 in a count of 2"
        );
        assert!(
            points(&[3, 0x02, 1, 1]).is_none(),
            "the run's third value missing"
        );
        assert!(points(&[0x80]).is_none(), "a two-byte count cut");
    }

    fn tuple(b: &[u8]) -> Tuple<'_> {
        Tuple(Bytes::new(b))
    }

    fn coords(v: &[i16]) -> Coords {
        let mut c = Coords::default();
        for (i, v) in v.iter().enumerate() {
            c.set_for_test(Index::of(u32::try_from(i).unwrap()), F2Dot14(*v));
        }
        c
    }

    /// The overview's scalar: 1 at the peak, falling linearly to 0 at the
    /// region's edge, 0 outside it; an axis whose peak is 0 is ignored; an
    /// invalid region is ignored on that axis. The overview's two-axis
    /// example: region (0.3, 0.15)-(1, 1), peak (0.7, 0.5), instance
    /// (0.5, 0.35) gives 0.5 x 0.571428... (the ratios of distances to the
    /// nearer edge).
    #[test]
    fn the_scalar_is_the_overviews() {
        let one = 16384i16;
        let peak = |v: i16| (v).to_be_bytes();
        let p1 = peak(one);
        let t = tuple(&p1);
        let a1 = Count::of(1u32);
        assert_eq!(scalar(&coords(&[one]), a1, t, None), 1.0);
        assert_eq!(scalar(&coords(&[one / 2]), a1, t, None), 0.5);
        assert_eq!(scalar(&coords(&[0]), a1, t, None), 0.0);
        assert_eq!(
            scalar(&coords(&[-one / 2]), a1, t, None),
            0.0,
            "outside the region"
        );
        let z = peak(0);
        assert_eq!(
            scalar(&coords(&[-one]), a1, tuple(&z), None),
            1.0,
            "peak 0 ignores the axis"
        );
        // Two axes, an intermediate region.
        let f = |v: f32| ((v * 16384.0) as i16).to_be_bytes();
        let cat = |a: [u8; 2], b: [u8; 2]| [a[0], a[1], b[0], b[1]];
        let (pk, st, en) = (
            cat(f(0.7), f(0.5)),
            cat(f(0.3), f(0.15)),
            cat(f(1.0), f(1.0)),
        );
        let s = scalar(
            &coords(&[(0.5 * 16384.0) as i16, (0.35 * 16384.0) as i16]),
            Count::of(2u32),
            tuple(&pk),
            Some((tuple(&st), tuple(&en))),
        );
        assert!((s - 0.5 * (0.2 / 0.35)).abs() < 1e-3, "{s}");
        // A start past the peak is an invalid region: the axis is ignored.
        let (pk, st, en) = (f(0.5), f(0.7), f(1.0));
        assert_eq!(
            scalar(
                &coords(&[(0.9 * 16384.0) as i16]),
                a1,
                tuple(&pk),
                Some((tuple(&st), tuple(&en)))
            ),
            1.0
        );
    }
}
