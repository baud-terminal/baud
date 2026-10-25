//! `GPOS` (`[[source: ms-opentype-gpos]]`): what a mark feature runs.
//!
//! **Types 1, 2, 4, 5, 6, 8 and 9.** Single and pair adjustment,
//! mark-to-base, mark-to-ligature and mark-to-mark attachment through
//! anchors, chained contexts in all three formats, and the extension
//! wrapper. A grid has no use for kerning
//! between cells or for cursive attachment (3): every base glyph sits at
//! its cell origin by Windows Terminal's rule (D8), and what is left to
//! position is where a combining mark goes relative to that base. But a
//! `mark` feature is not only anchors: Courier New's `hebr` one moves a
//! holam 25 units by a single adjustment and spreads two points below one
//! letter by chained contexts that run single and pair adjustments, and a
//! feature names whatever lookups it names. So every lookup the grid's
//! features name runs, whatever its type, except contexts without a chain
//! (7), which no measured face reaches. Mark-to-ligature places a mark on
//! the lam-alef ligature: Courier New's `arab` `mark` names it.
//! Anchors are read in their design-unit form (formats 1, 2 and 3 all start
//! with one); the contour point and the hinting devices are hinting's and
//! D7 has none.
//!
//! **An adjustment moves its glyph; an advance moves the glyphs after it
//! in its cluster.** A placement adds to the glyph's offset. An advance
//! changes where the pen leaves the glyphs of the cluster that follow it
//! and are not attached -- rightward, or leftward in a right-to-left run,
//! where "if the first glyph has an advance width of 100, then the second
//! glyph begins at -100,0" (`[[source: ms-opentype-gpos-2026-10-23]]`) --
//! unless the glyph advances nothing, as a mark and a default-ignorable do:
//! HarfBuzz zeroes a mark's advance after `GPOS` has run
//! (`[[source: harfbuzz-hb-ot-shape]]`, `zero_mark_widths_by_gdef()`), so an
//! advance a lookup gives one is spent before anything reads it.
//!
//! **At a variation, a format-3 anchor moves** (note Z step 4): its x and
//! y device tables, when of the VariationIndex format `0x8000`, index
//! `GDEF`'s item variation store, and the coordinate takes the store's
//! delta rounded -- HarfBuzz 12.3.0's `AnchorFormat3::get_anchor` and
//! `VariationDevice::get_x_delta`, which rounds through `em_scalef`
//! (`[[source: harfbuzz-12-3-0-anchor-format3]]`,
//! `[[source: harfbuzz-12-3-0-layout-common]]`, `[[source: harfbuzz-12-3-0-font]]`,
//! `[[source: harfbuzz-12-3-0-hb]]`). The anchors' difference is whole units
//! then, so HarfBuzz's final `roundf` (`[[source: harfbuzz-12-3-0-mark-array]]`)
//! changes nothing. A value record's devices move its values the same way.
//! A device that does not check out is no device: that coordinate does
//! not vary, where HarfBuzz refuses the whole `GPOS` (Jev B, 0.84).
//!
//! **Offsets are from the cell origin.** Before any lookup runs every
//! glyph's `dx` is its pen position within the cluster, so a positioned
//! mark's `dx`/`dy` becomes the target's plus `target anchor - mark
//! anchor` in font units, and a stacked mark rides on the mark below it.
//! HarfBuzz reports the same geometry relative to the mark's own pen
//! position, one base advance further along; `tests/shape.rs` translates.

use crate::buffer::{Buffer, Glyph, Ignorable, Mark, Positioned};
use crate::bytes::{Array, Bytes, Count, Index, Len, Offset};
use crate::context::{Applied, Depth, Nested, chained, coverage_at, same, seen};
use crate::ivs::{Store, VarIndex};
use crate::layout::{
    Class, ClassDef, Coverage, FeatureTag, Features, Filter, Gdef, Kind, Layout, Lookup, LookupSet,
    ScriptTag, Skip,
};
use crate::script::{Direction, Script};
use crate::var::Coords;

/// An anchor's design-unit coordinates, the head of every anchor format,
/// moved by its variation devices at `coords`.
#[derive(Clone, Copy)]
struct Anchor {
    x: i16,
    y: i16,
}

/// Where an anchor's coordinates are read: the store its devices index,
/// and the coordinates.
#[derive(Clone, Copy)]
struct Vary<'v, 'a> {
    store: Option<Store<'a>>,
    coords: &'v Coords,
}

fn anchor(table: Bytes<'_>, at: Offset, vary: Vary<'_, '_>) -> Option<Anchor> {
    let off: u16 = table.read(at)?;
    if off == 0 {
        return None;
    }
    let a = table.tail(Offset::of(u32::from(off)))?;
    let format: u16 = a.read(Offset::ZERO)?;
    if !(1..=3).contains(&format) {
        return None;
    }
    let (x, y): (i16, i16) = (a.read(Offset::of(2u32))?, a.read(Offset::of(4u32))?);
    if format != 3 || vary.coords.is_default() {
        return Some(Anchor { x, y });
    }
    let moved = |c: i16, device: u32| -> i16 {
        let d = delta(a, Offset::of(device), vary);
        let v = i32::from(c).saturating_add(d);
        i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(c)
    };
    Some(Anchor {
        x: moved(x, 6),
        y: moved(y, 8),
    })
}

/// The rounded delta the device at `at` in `anchor` names, or 0: no
/// offset, a device past the data, a hinting format, or no store.
fn delta(anchor: Bytes<'_>, at: Offset, vary: Vary<'_, '_>) -> i32 {
    match anchor.read::<u16>(at) {
        Some(off) => device_delta(anchor, Offset::of(u32::from(off)), vary),
        None => 0,
    }
}

/// The rounded delta of the device `off` into `parent` names, or 0: no
/// offset (zero), a device past the data, a hinting format, or no store.
fn device_delta(parent: Bytes<'_>, off: Offset, vary: Vary<'_, '_>) -> i32 {
    let Some(store) = vary.store else {
        return 0;
    };
    if off == Offset::ZERO {
        return 0;
    }
    let Some(device) = parent.tail(off) else {
        return 0;
    };
    let (Some(index), Some(format)) = (
        VarIndex::of_device(device),
        device.read::<u16>(Offset::of(4u32)),
    ) else {
        return 0;
    };
    if format != VARIATION_INDEX {
        return 0;
    }
    let d = crate::face::round_half_up(store.delta(index, vary.coords));
    // Past i16 the anchor clamps anyway; this keeps the cast in range.
    d.clamp(-65536.0, 65536.0) as i32
}

/// A device table's `deltaFormat` for an index into the item variation
/// store.
const VARIATION_INDEX: u16 = 0x8000;

#[derive(Clone, Copy)]
pub struct Gpos<'a> {
    layout: Layout<'a>,
}

impl<'a> Gpos<'a> {
    /// The lookups the features name for text in `script`.
    pub fn lookups(&self, script: Script, features: Features) -> LookupSet {
        self.layout.lookups_for(script, features)
    }

    /// The tag of the Script table text in `script` is positioned with.
    pub fn chosen(&self, script: Script) -> Option<ScriptTag> {
        self.layout.chosen(script)
    }

    /// Whether text in `script` is positioned with a `mark` feature here.
    pub fn names_mark(&self, script: Script) -> bool {
        self.layout.names(script, FeatureTag::MARK)
    }

    pub fn parse(data: Bytes<'a>, gdef: Gdef<'a>) -> Option<Gpos<'a>> {
        Some(Gpos {
            layout: Layout::parse(data, gdef)?,
        })
    }

    /// Applies the lookups the features name for text in `script`, in
    /// lookup order, with the anchors and values read at `coords`.
    /// `direction` is the run's: which way an advance moves the pen.
    pub fn apply(
        &self,
        buf: &mut Buffer<'_>,
        script: Script,
        direction: Direction,
        features: Features,
        coords: &Coords,
    ) -> Count {
        let run = Run {
            gpos: self,
            vary: Vary {
                store: self.layout.gdef().store(),
                coords,
            },
            direction,
        };
        let set = self.layout.lookups_for(script, features);
        for i in set.iter() {
            let Some(lookup) = self.layout.lookup(i) else {
                continue;
            };
            let mut pos = Index::ZERO;
            match Reach::of(&lookup) {
                // A lookup of attachments alone moves on one glyph each
                // time, applied or not, and needs no nested lookups: the
                // walk JetBrains Mono's `mark` and `mkmk` take, kept out of
                // the general one that can recurse.
                Reach::Attachments => {
                    while let Some(g) = buf.get(pos) {
                        if seen(&lookup.filter, g) == Skip::No {
                            self.attach_at(&lookup, buf, pos, run.vary);
                        }
                        pos = pos.next();
                    }
                }
                Reach::Any => {
                    while let Some(g) = buf.get(pos) {
                        if seen(&lookup.filter, g) == Skip::Yes {
                            pos = pos.next();
                            continue;
                        }
                        pos = match run.apply_at(&lookup, buf, pos, Depth::TOP) {
                            Some(a) => a.next.max(pos.next()),
                            None => pos.next(),
                        };
                    }
                }
            }
        }
        set.declined()
    }

    /// [`Run::apply_at`] for a lookup of [`Reach::Attachments`]: each
    /// subtable in turn until one attaches the mark at `pos`, or one does
    /// not read, which ends the lookup there as it ends the general walk.
    fn attach_at(&self, lookup: &Lookup<'a>, buf: &mut Buffer<'_>, pos: Index, vary: Vary<'_, 'a>) {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            let Some((kind, sub)) = lookup.subtable(i, Kind::GPOS_EXTENSION) else {
                return;
            };
            let attached = match kind {
                Kind::MARK_TO_BASE => {
                    self.mark_to(sub, &lookup.filter, buf, pos, Target::Base, vary)
                }
                Kind::MARK_TO_LIGATURE => {
                    self.mark_to_ligature(sub, &lookup.filter, buf, pos, vary)
                }
                _ => self.mark_to(sub, &lookup.filter, buf, pos, Target::Mark, vary),
            };
            if attached.is_some() {
                return;
            }
            i = i.next();
        }
    }

    /// The glyph a mark attaches to: the nearest earlier glyph the filter
    /// does not skip and that is covered, with marks stepped over on the
    /// way to a base and kept on the way to a mark. A default-ignorable is
    /// stepped over either way, as HarfBuzz's matcher skips one no lookup
    /// names outright.
    fn attach_target(
        &self,
        filter: &Filter<'_>,
        buf: &Buffer<'_>,
        pos: Index,
        coverage: &Coverage<'_>,
        target: Target,
    ) -> Option<(Index, Index)> {
        let gdef = self.layout.gdef();
        let mut j = pos;
        while let Some(p) = j.prev() {
            j = p;
            let g = buf.get(j)?;
            if filter.skip_class(g.class, g.id) == Skip::Yes || g.ignorable == Ignorable::Yes {
                continue;
            }
            if target == Target::Base && gdef.is_mark(g.id) {
                continue;
            }
            return coverage.index(g.id).map(|ci| (j, ci));
        }
        None
    }

    /// `MarkBasePosFormat1` and `MarkMarkPosFormat1` share one layout:
    /// mark coverage, target coverage, class count, mark array, target
    /// array.
    fn mark_to(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
        target: Target,
        vary: Vary<'_, 'a>,
    ) -> Option<()> {
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let cov = |at: u32| -> Option<Coverage<'a>> {
            let off: u16 = sub.read(Offset::of(at))?;
            Coverage::parse(sub.tail(Offset::of(u32::from(off)))?)
        };
        let mark_cov = cov(2)?;
        let target_cov = cov(4)?;
        let classes: u16 = sub.read(Offset::of(6u32))?;
        let mark_array_off: u16 = sub.read(Offset::of(8u32))?;
        let target_array_off: u16 = sub.read(Offset::of(10u32))?;
        let mark = buf.get(pos)?;
        let mi = mark_cov.index(mark.id)?;
        let (tpos, ti) = self.attach_target(filter, buf, pos, &target_cov, target)?;
        // MarkArray: count, then (class, anchor offset) records.
        let marks = sub.tail(Offset::of(u32::from(mark_array_off)))?;
        let mark_count: u16 = marks.read(Offset::ZERO)?;
        if !Count::of(mark_count).holds(mi) {
            return None;
        }
        let rec = Offset::of(2u32).at(mi, Len::of(4))?;
        let class: u16 = marks.read(rec)?;
        if class >= classes {
            return None;
        }
        let mark_anchor = anchor(marks, rec.plus(Len::of(2))?, vary)?;
        // BaseArray / Mark2Array: count, then per target an anchor offset
        // per class.
        let targets = sub.tail(Offset::of(u32::from(target_array_off)))?;
        let target_count: u16 = targets.read(Offset::ZERO)?;
        if !Count::of(target_count).holds(ti) {
            return None;
        }
        let row = Len::of(2).times(Count::of(classes))?;
        let rec = Offset::of(2u32)
            .at(ti, row)?
            .at(Index::of(class), Len::of(2))?;
        let target_anchor = anchor(targets, rec, vary)?;
        let carried = buf.get(tpos)?;
        let g: &mut Glyph = buf.get_mut(pos)?;
        g.dx = carried
            .dx
            .saturating_add(target_anchor.x.saturating_sub(mark_anchor.x));
        g.dy = carried
            .dy
            .saturating_add(target_anchor.y.saturating_sub(mark_anchor.y));
        g.positioned = Positioned::Yes;
        Some(())
    }
}

impl<'a> Gpos<'a> {
    /// `MarkLigPosFormat1` (`[[source: ms-opentype-gpos]]`, lookup type 5):
    /// mark coverage, ligature coverage, class count, mark array, ligature
    /// array, whose `LigatureAttach` holds an anchor a class for each of a
    /// ligature's components. The mark attaches to the nearest earlier
    /// glyph that is not a mark, at the component its own cell gave the
    /// ligature: a ligature keeps the cell of its first component and spans
    /// to its last, one component a cell, so a mark of the second cell
    /// takes the second component's anchor, and one past the last takes the
    /// last's. An anchor offset of zero attaches nothing there.
    fn mark_to_ligature(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
        vary: Vary<'_, 'a>,
    ) -> Option<()> {
        let format: u16 = sub.read(Offset::ZERO)?;
        if format != 1 {
            return None;
        }
        let cov = |at: u32| -> Option<Coverage<'a>> {
            let off: u16 = sub.read(Offset::of(at))?;
            Coverage::parse(sub.tail(Offset::of(u32::from(off)))?)
        };
        let mark_cov = cov(2)?;
        let lig_cov = cov(4)?;
        let classes: u16 = sub.read(Offset::of(6u32))?;
        let mark_array_off: u16 = sub.read(Offset::of(8u32))?;
        let lig_array_off: u16 = sub.read(Offset::of(10u32))?;
        let mark = buf.get(pos)?;
        let mi = mark_cov.index(mark.id)?;
        let (lpos, li) = self.attach_target(filter, buf, pos, &lig_cov, Target::Base)?;
        let marks = sub.tail(Offset::of(u32::from(mark_array_off)))?;
        let mark_count: u16 = marks.read(Offset::ZERO)?;
        if !Count::of(mark_count).holds(mi) {
            return None;
        }
        let rec = Offset::of(2u32).at(mi, Len::of(4))?;
        let class: u16 = marks.read(rec)?;
        if class >= classes {
            return None;
        }
        let mark_anchor = anchor(marks, rec.plus(Len::of(2))?, vary)?;
        // LigatureArray: count, then an offset to each LigatureAttach.
        let ligs = sub.tail(Offset::of(u32::from(lig_array_off)))?;
        let lig_count: u16 = ligs.read(Offset::ZERO)?;
        if !Count::of(lig_count).holds(li) {
            return None;
        }
        let attach_off: u16 = ligs.read(Offset::of(2u32).at(li, Len::of(2))?)?;
        let attach = ligs.tail(Offset::of(u32::from(attach_off)))?;
        let components: u16 = attach.read(Offset::ZERO)?;
        let lig = buf.get(lpos)?;
        let last = components.checked_sub(1)?;
        let comp = match mark.cluster.0.checked_sub(lig.cluster.0) {
            Some(k) if mark.cluster.0 <= lig.last.0 => k.min(last),
            _ => last,
        };
        let row = Len::of(2).times(Count::of(classes))?;
        let rec = Offset::of(2u32)
            .at(Index::of(comp), row)?
            .at(Index::of(class), Len::of(2))?;
        let lig_anchor = anchor(attach, rec, vary)?;
        let g: &mut Glyph = buf.get_mut(pos)?;
        g.dx = lig
            .dx
            .saturating_add(lig_anchor.x.saturating_sub(mark_anchor.x));
        g.dy = lig
            .dy
            .saturating_add(lig_anchor.y.saturating_sub(mark_anchor.y));
        g.positioned = Positioned::Yes;
        Some(())
    }
}

/// What one lookup's subtables can do, read once a lookup before the walk
/// over the run rather than at every glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reach {
    /// Every subtable that reads, up to the first that does not, attaches
    /// a mark to a base or to a mark (types 4 and 6, through the extension
    /// wrapper or not): no adjustment, no context, nothing nested.
    Attachments,
    /// Some subtable is of another type.
    Any,
}

impl Reach {
    fn of(lookup: &Lookup<'_>) -> Reach {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            match lookup.subtable(i, Kind::GPOS_EXTENSION) {
                Some((Kind::MARK_TO_BASE | Kind::MARK_TO_LIGATURE | Kind::MARK_TO_MARK, _)) => {}
                Some(_) => return Reach::Any,
                None => return Reach::Attachments,
            }
            i = i.next();
        }
        Reach::Attachments
    }
}

/// One application of `GPOS` to a run: the table, where its anchors and
/// values are read, and the run's direction.
struct Run<'g, 'v, 'a> {
    gpos: &'g Gpos<'a>,
    vary: Vary<'v, 'a>,
    direction: Direction,
}

impl<'a> Nested<'a> for Run<'_, '_, 'a> {
    fn nested(
        &self,
        lookup: Index,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let lookup = self.gpos.layout.lookup(lookup)?;
        self.apply_at(&lookup, buf, pos, depth)
    }
}

impl<'a> Run<'_, '_, 'a> {
    /// Tries each subtable at `pos`; the first that applies wins.
    fn apply_at(
        &self,
        lookup: &Lookup<'a>,
        buf: &mut Buffer<'_>,
        pos: Index,
        depth: Depth,
    ) -> Option<Applied> {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            let (kind, sub) = lookup.subtable(i, Kind::GPOS_EXTENSION)?;
            let done = match kind {
                Kind::SINGLE => self.single(sub, buf, pos),
                Kind::PAIR => self.pair(sub, &lookup.filter, buf, pos),
                Kind::MARK_TO_BASE => self
                    .gpos
                    .mark_to(sub, &lookup.filter, buf, pos, Target::Base, self.vary)
                    .map(|()| Applied::after(pos)),
                Kind::MARK_TO_MARK => self
                    .gpos
                    .mark_to(sub, &lookup.filter, buf, pos, Target::Mark, self.vary)
                    .map(|()| Applied::after(pos)),
                Kind::MARK_TO_LIGATURE => self
                    .gpos
                    .mark_to_ligature(sub, &lookup.filter, buf, pos, self.vary)
                    .map(|()| Applied::after(pos)),
                Kind::CHAINED_POS => chained(self, sub, &lookup.filter, buf, pos, depth),
                _ => None,
            };
            if done.is_some() {
                return done;
            }
            i = i.next();
        }
        None
    }

    /// Type 1: one value record for every covered glyph (format 1), or one
    /// per coverage index (format 2).
    fn single(&self, sub: Bytes<'a>, buf: &mut Buffer<'_>, pos: Index) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        let coverage = coverage_at(sub, Offset::of(2u32))?;
        let ci = coverage.index(buf.get(pos)?.id)?;
        let vf = ValueFormat(sub.read(Offset::of(4u32))?);
        let at = match format {
            1 => Offset::of(6u32),
            2 => {
                let count: u16 = sub.read(Offset::of(6u32))?;
                if !Count::of(count).holds(ci) {
                    return None;
                }
                Offset::of(8u32).at(ci, vf.size())?
            }
            _ => return None,
        };
        let v = vf.read(sub, sub, at, self.vary)?;
        self.adjust(buf, pos, v);
        Some(Applied::after(pos))
    }

    /// Type 2: a value record for each of two glyphs, the second the next
    /// one the filter does not skip, by glyph pair (format 1) or by class
    /// pair (format 2). The walk goes on after the second glyph when it was
    /// given a value, and from it otherwise, as HarfBuzz's does.
    fn pair(
        &self,
        sub: Bytes<'a>,
        filter: &Filter<'_>,
        buf: &mut Buffer<'_>,
        pos: Index,
    ) -> Option<Applied> {
        let format: u16 = sub.read(Offset::ZERO)?;
        let coverage = coverage_at(sub, Offset::of(2u32))?;
        let first = buf.get(pos)?;
        let ci = coverage.index(first.id)?;
        let vf1 = ValueFormat(sub.read(Offset::of(4u32))?);
        let vf2 = ValueFormat(sub.read(Offset::of(6u32))?);
        let second = next_unskipped(filter, buf, pos)?;
        let second_id = buf.get(second)?.id;
        let record = vf1.size().value().saturating_add(vf2.size().value());
        let (table, at) = match format {
            1 => {
                let sets_n: u16 = sub.read(Offset::of(8u32))?;
                let sets: Array<'a, u16> = sub.array(Offset::of(10u32), Count::of(sets_n))?;
                let set = sub.tail(Offset::of(u32::from(sets.get(ci)?)))?;
                let n: u16 = set.read(Offset::ZERO)?;
                let stride = Len::of(record.saturating_add(2));
                let mut k = Index::ZERO;
                let mut found = None;
                while Count::of(n).holds(k) {
                    let rec = Offset::of(2u32).at(k, stride)?;
                    let glyph: u16 = set.read(rec)?;
                    if same(second_id, glyph) {
                        found = Some(rec.plus(Len::of(2))?);
                        break;
                    }
                    k = k.next();
                }
                (set, found?)
            }
            2 => {
                let def = |at: u32| -> Option<ClassDef<'a>> {
                    let off: u16 = sub.read(Offset::of(at))?;
                    ClassDef::parse(sub.tail(Offset::of(u32::from(off)))?)
                };
                let (def1, def2) = (def(8)?, def(10)?);
                let n1: u16 = sub.read(Offset::of(12u32))?;
                let n2: u16 = sub.read(Offset::of(14u32))?;
                let (Class(c1), Class(c2)) = (def1.class(first.id), def2.class(second_id));
                if c1 >= n1 || c2 >= n2 {
                    return None;
                }
                let row = Len::of(record).times(Count::of(n2))?;
                let at = Offset::of(16u32)
                    .at(Index::of(c1), row)?
                    .at(Index::of(c2), Len::of(record))?;
                (sub, at)
            }
            _ => return None,
        };
        // A record's devices count from its immediate parent: the PairSet
        // in format 1, the subtable in format 2.
        let v1 = vf1.read(table, table, at, self.vary)?;
        let v2 = vf2.read(table, table, at.plus(vf1.size())?, self.vary)?;
        self.adjust(buf, pos, v1);
        self.adjust(buf, second, v2);
        Some(Applied {
            next: if vf2.size().is_zero() {
                second
            } else {
                second.next()
            },
            removed: Count::ZERO,
        })
    }

    /// Applies one value record to the glyph at `pos`.
    fn adjust(&self, buf: &mut Buffer<'_>, pos: Index, v: Value) {
        let Some(g) = buf.get_mut(pos) else {
            return;
        };
        g.dx = g.dx.saturating_add(v.dx);
        g.dy = g.dy.saturating_add(v.dy);
        // What advances nothing has no advance to change.
        if v.advance == 0 || g.mark == Mark::Yes || g.ignorable == Ignorable::Yes {
            return;
        }
        let (cluster, first) = (g.cluster, g.attached == crate::buffer::Attached::No);
        // Left to right, the glyphs after this one move by the change;
        // right to left, this one moves too unless it is the base, whose
        // advance runs away from the glyphs after it.
        let (from, by) = match self.direction {
            Direction::LeftToRight => (pos.next(), v.advance),
            Direction::RightToLeft if first => return,
            Direction::RightToLeft => (pos, v.advance.saturating_neg()),
        };
        let mut at = from;
        while let Some(h) = buf.get_mut(at) {
            if h.cluster != cluster {
                break;
            }
            if h.positioned == Positioned::No {
                h.dx = h.dx.saturating_add(by);
            }
            at = at.next();
        }
    }
}

/// The next glyph after `pos` that `filter` does not skip and that is not
/// a default-ignorable, which a pair's second glyph never is.
fn next_unskipped(filter: &Filter<'_>, buf: &Buffer<'_>, pos: Index) -> Option<Index> {
    let mut j = pos.next();
    while let Some(g) = buf.get(j) {
        if seen(filter, g) == Skip::No && g.ignorable == Ignorable::No {
            return Some(j);
        }
        j = j.next();
    }
    None
}

/// Which fields a value record carries (`ValueFormat`), each two bytes, in
/// this order.
#[derive(Clone, Copy)]
struct ValueFormat(u16);

impl ValueFormat {
    const X_PLACEMENT: ValueFormat = ValueFormat(0x0001);
    const Y_PLACEMENT: ValueFormat = ValueFormat(0x0002);
    const X_ADVANCE: ValueFormat = ValueFormat(0x0004);
    const Y_ADVANCE: ValueFormat = ValueFormat(0x0008);
    const X_PLACEMENT_DEVICE: ValueFormat = ValueFormat(0x0010);
    const Y_PLACEMENT_DEVICE: ValueFormat = ValueFormat(0x0020);
    const X_ADVANCE_DEVICE: ValueFormat = ValueFormat(0x0040);
    const Y_ADVANCE_DEVICE: ValueFormat = ValueFormat(0x0080);

    fn carries(self, field: ValueFormat) -> bool {
        self.0 & field.0 != 0
    }

    /// The record's size: two bytes a field. The high byte is reserved.
    fn size(self) -> Len {
        Len::of((self.0 & 0x00FF).count_ones().saturating_mul(2))
    }

    /// The record at `at` in `table`, its devices' offsets counted from
    /// `parent`, "the immediate parent table (SinglePos or PairPosFormat2
    /// lookup subtable, PairSet table within a PairPosFormat1 lookup
    /// subtable)" (`[[source: ms-opentype-gpos-2026-10-23]]`), moved at
    /// `vary`. Only the horizontal advance is kept: a run here is
    /// horizontal.
    fn read(
        self,
        table: Bytes<'_>,
        parent: Bytes<'_>,
        at: Offset,
        vary: Vary<'_, '_>,
    ) -> Option<Value> {
        let mut v = Value::ZERO;
        let mut at = at;
        let mut field = |f: ValueFormat| -> Option<Option<i16>> {
            if !self.carries(f) {
                return Some(None);
            }
            let x: i16 = table.read(at)?;
            at = at.plus(Len::of(2))?;
            Some(Some(x))
        };
        let x_place = field(ValueFormat::X_PLACEMENT)?;
        let y_place = field(ValueFormat::Y_PLACEMENT)?;
        let x_adv = field(ValueFormat::X_ADVANCE)?;
        field(ValueFormat::Y_ADVANCE)?;
        let x_place_dev = field(ValueFormat::X_PLACEMENT_DEVICE)?;
        let y_place_dev = field(ValueFormat::Y_PLACEMENT_DEVICE)?;
        let x_adv_dev = field(ValueFormat::X_ADVANCE_DEVICE)?;
        field(ValueFormat::Y_ADVANCE_DEVICE)?;
        let moved = |c: Option<i16>, dev: Option<i16>| -> i16 {
            let base = i32::from(c.unwrap_or(0));
            let d = dev.map_or(0, |o| {
                device_delta(parent, Offset::of(u32::from(o.cast_unsigned())), vary)
            });
            let total = base.saturating_add(d);
            i16::try_from(total.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0)
        };
        v.dx = moved(x_place, x_place_dev);
        v.dy = moved(y_place, y_place_dev);
        v.advance = moved(x_adv, x_adv_dev);
        Some(v)
    }
}

/// What a value record does to a glyph, in font units: its placement and
/// its horizontal advance.
#[derive(Clone, Copy)]
struct Value {
    dx: i16,
    dy: i16,
    advance: i16,
}

impl Value {
    const ZERO: Value = Value {
        dx: 0,
        dy: 0,
        advance: 0,
    };
}

/// What a mark lookup attaches to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Base,
    Mark,
}

#[cfg(test)]
mod tests {
    extern crate alloc;
    use super::*;
    use crate::buffer::{Attached, Cluster};
    use alloc::vec;
    use alloc::vec::Vec;
    use baud_domain::GlyphId;

    /// A `GPOS` table of one `DFLT` Script table whose `mark` feature names
    /// the first lookup only; every lookup is one subtable, given as its
    /// type and bytes, the subtable laid right after its lookup header.
    fn table(lookups: &[(u16, Vec<u8>)]) -> Vec<u8> {
        let be = |v: usize| u16::try_from(v).unwrap().to_be_bytes();
        let mut t = vec![0, 1, 0, 0, 0, 10, 0, 30, 0, 44];
        // ScriptList at 10: DFLT -> Script at 8; LangSys at 4 from it.
        t.extend([0, 1, b'D', b'F', b'L', b'T', 0, 8]);
        t.extend([0, 4, 0, 0, 0, 0, 0xFF, 0xFF, 0, 1, 0, 0]);
        // FeatureList at 30: mark -> Feature at 8, naming lookup 0.
        t.extend([0, 1, b'm', b'a', b'r', b'k', 0, 8, 0, 0, 0, 1, 0, 0]);
        // LookupList at 44.
        let mut list = be(lookups.len()).to_vec();
        let mut bodies = Vec::new();
        let head = 2 + 2 * lookups.len();
        for (kind, sub) in lookups {
            list.extend(be(head + bodies.len()));
            bodies.extend(kind.to_be_bytes());
            bodies.extend([0, 0, 0, 1, 0, 8]);
            bodies.extend(sub);
        }
        t.extend(list);
        t.extend(bodies);
        t
    }

    /// A buffer of `(glyph, cluster, attached, mark, pen)`.
    fn buffer<'b>(
        storage: &'b mut [Glyph],
        glyphs: &[(u16, u16, Attached, Mark, i16)],
    ) -> Buffer<'b> {
        let mut buf = Buffer::new(storage);
        for &(id, cluster, attached, mark, pen) in glyphs {
            let mut g = Glyph::new(GlyphId::from_shaper(id), Cluster(cluster), Ignorable::No);
            g.attached = attached;
            g.mark = mark;
            g.dx = pen;
            buf.push(g).unwrap();
        }
        buf
    }

    fn run(t: &[u8], buf: &mut Buffer<'_>, direction: Direction) {
        let gpos = Gpos::parse(Bytes::new(t), Gdef::default()).unwrap();
        let declined = gpos.apply(
            buf,
            Script::COMMON,
            direction,
            Features::GRID,
            &Coords::default(),
        );
        assert!(declined.is_zero());
    }

    fn placed(buf: &Buffer<'_>) -> Vec<(i16, i16)> {
        buf.as_slice().iter().map(|g| (g.dx, g.dy)).collect()
    }

    /// Format 1 single adjustment: XPlacement 30, YPlacement -20 and
    /// XAdvance 100 for glyph 10 -- coverage at 12, after the record.
    fn single_10() -> Vec<u8> {
        vec![
            0, 1, 0, 12, 0, 7, 0, 30, 0xFF, 0xEC, 0, 100, 0, 1, 0, 1, 0, 10,
        ]
    }

    use Attached::{No as Base, Yes as After};
    use Mark::{No as Letter, Yes as Point};

    /// **A placement moves its glyph; an advance moves the glyphs after
    /// it in its cluster, the way the pen runs.** Left to right, the glyph
    /// after the adjusted base moves right by the advance and the next
    /// cluster's glyphs do not; right to left, a base's advance runs away
    /// from the glyphs after it and moves none.
    #[test]
    fn a_single_adjustment_places_its_glyph_and_its_advance_moves_the_pen() {
        let t = table(&[(1, single_10())]);
        let glyphs = [
            (10, 0, Base, Letter, 0),
            (20, 0, After, Letter, 600),
            (10, 1, Base, Letter, 0),
        ];
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(&mut storage, &glyphs);
        run(&t, &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(30, -20), (700, 0), (30, -20)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(&mut storage, &glyphs);
        run(&t, &mut buf, Direction::RightToLeft);
        assert_eq!(placed(&buf), [(30, -20), (600, 0), (30, -20)]);
    }

    /// **A mark advances nothing, so an advance given to one moves
    /// nothing**: HarfBuzz zeroes a mark's advance after `GPOS` has run.
    /// Right to left, an advance given to a glyph after the base moves that
    /// glyph and the ones after it left.
    #[test]
    fn a_marks_advance_moves_nothing_and_right_to_left_moves_the_glyph_itself() {
        let t = table(&[(1, single_10())]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[
                (20, 0, Base, Letter, 0),
                (10, 0, After, Point, 0),
                (21, 0, After, Point, 0),
            ],
        );
        run(&t, &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(0, 0), (30, -20), (0, 0)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[
                (20, 0, Base, Letter, 0),
                (10, 0, After, Letter, -5),
                (21, 0, After, Point, -5),
            ],
        );
        run(&t, &mut buf, Direction::RightToLeft);
        assert_eq!(placed(&buf), [(0, 0), (-75, -20), (-105, 0)]);
    }

    /// Format 2: one record per coverage index -- glyphs 10 and 11, the
    /// second moved 7 left.
    #[test]
    fn a_format_two_single_adjustment_reads_the_record_of_its_coverage_index() {
        let sub = vec![
            0, 2, 0, 12, 0, 1, 0, 2, 0, 3, 0xFF, 0xF9, 0, 1, 0, 2, 0, 10, 0, 11,
        ];
        let t = table(&[(1, sub)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[(10, 0, Base, Letter, 0), (11, 1, Base, Letter, 0)],
        );
        run(&t, &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(3, 0), (-7, 0)]);
    }

    /// **A pair adjusts both glyphs**, by glyph pair (format 1) and by class
    /// pair (format 2), and the walk goes on after the second glyph only
    /// when the second was given a value: three glyphs 10 in a row with a
    /// first-only value adjust the first two, and with a value for both
    /// adjust the first pair alone.
    #[test]
    fn a_pair_adjusts_both_glyphs_and_steps_past_the_second_only_when_it_took_a_value() {
        // Format 1: first 10, second 10 -- set at 12, coverage at 22.
        let both = vec![
            0, 1, 0, 22, 0, 1, 0, 1, 0, 1, 0, 12, 0, 1, 0, 10, 0, 5, 0xFF, 0xF9, 0, 0, 0, 1, 0, 1,
            0, 10,
        ];
        let first = vec![
            0, 1, 0, 20, 0, 1, 0, 0, 0, 1, 0, 12, 0, 1, 0, 10, 0, 5, 0, 0, 0, 1, 0, 1, 0, 10,
        ];
        let three = [
            (10, 0, Base, Letter, 0),
            (10, 1, Base, Letter, 0),
            (10, 2, Base, Letter, 0),
        ];
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(&mut storage, &three);
        run(&table(&[(2, both)]), &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(5, 0), (-7, 0), (0, 0)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(&mut storage, &three);
        run(&table(&[(2, first)]), &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(5, 0), (5, 0), (0, 0)]);
        // Format 2: glyph 10 is first class 1, glyph 20 second class 1;
        // classes 2 by 2, the record for (1, 1) moving 9 and 4; the two
        // class definitions at 32 and 40, the coverage at 48.
        let classes = vec![
            0, 2, 0, 48, 0, 1, 0, 1, 0, 32, 0, 40, 0, 2, 0, 2, //
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 9, 0, 4, // (0,0) (0,1) (1,0) (1,1)
            0, 1, 0, 10, 0, 1, 0, 1, // first classes: glyph 10 is 1
            0, 1, 0, 20, 0, 1, 0, 1, // second classes: glyph 20 is 1
            0, 1, 0, 1, 0, 10, // coverage
        ];
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[(10, 0, Base, Letter, 0), (20, 1, Base, Letter, 0)],
        );
        run(&table(&[(2, classes)]), &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(9, 0), (4, 0)]);
    }

    /// **A chained context runs a nested lookup at its input**: glyph 20
    /// after glyph 30 moves 250 right by lookup 1, which no feature names;
    /// after glyph 31 it does not move.
    #[test]
    fn a_chained_context_runs_its_nested_adjustment_where_the_context_matches() {
        // Format 3: one backtrack coverage (30), one input coverage (20),
        // no lookahead, one record (0 -> lookup 1); coverages at 18 and 24.
        let chain = vec![
            0, 3, 0, 1, 0, 18, 0, 1, 0, 24, 0, 0, 0, 1, 0, 0, 0, 1, //
            0, 1, 0, 1, 0, 30, // backtrack coverage
            0, 1, 0, 1, 0, 20, // input coverage
        ];
        let nested = vec![0, 1, 0, 8, 0, 1, 0, 250, 0, 1, 0, 1, 0, 20];
        let t = table(&[(8, chain), (1, nested)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[(30, 0, Base, Letter, 0), (20, 1, Base, Letter, 0)],
        );
        run(&t, &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(0, 0), (250, 0)]);
        let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 8];
        let mut buf = buffer(
            &mut storage,
            &[(31, 0, Base, Letter, 0), (20, 1, Base, Letter, 0)],
        );
        run(&t, &mut buf, Direction::LeftToRight);
        assert_eq!(placed(&buf), [(0, 0), (0, 0)]);
    }
    use crate::var::F2Dot14;

    /// An item variation store of one axis, one region peaking at 1, and
    /// one data set whose items move by 100 and -37.
    const STORE: [u8; 34] = [
        0, 1, 0, 0, 0, 12, 0, 1, 0, 0, 0, 22, // format, regions at 12, one set at 22
        0, 1, 0, 1, 0, 0, 0x40, 0, 0x40, 0, // one axis, one region: (0, 1, 1)
        0, 2, 0, 1, 0, 1, 0, 0, // two items, one word column, region 0
        0, 100, 0xFF, 0xDB, // 100 and -37
    ];

    /// A table holding an anchor offset at 0 and, from 2, a format-3 anchor
    /// at (10, 20) whose x and y devices, when given, sit at 12 and 18.
    fn anchor_table(x: Option<[u8; 6]>, y: Option<[u8; 6]>) -> [u8; 24] {
        let mut t = [0u8; 24];
        t[..10].copy_from_slice(&[0, 2, 0, 3, 0, 10, 0, 20, 0, 0]);
        if let Some(d) = x {
            t[8..10].copy_from_slice(&10u16.to_be_bytes());
            t[12..18].copy_from_slice(&d);
        }
        if let Some(d) = y {
            t[10..12].copy_from_slice(&16u16.to_be_bytes());
            t[18..24].copy_from_slice(&d);
        }
        t
    }

    fn at(t: &[u8], coord: F2Dot14, store: Option<Store<'_>>) -> (i16, i16) {
        let mut coords = Coords::default();
        coords.set_for_test(Index::ZERO, coord);
        let vary = Vary {
            store,
            coords: &coords,
        };
        let a = anchor(Bytes::new(t), Offset::ZERO, vary).unwrap();
        (a.x, a.y)
    }

    fn store() -> Option<Store<'static>> {
        Store::parse(Bytes::new(&STORE))
    }

    const ITEM_0: [u8; 6] = [0, 0, 0, 0, 0x80, 0];
    const ITEM_1: [u8; 6] = [0, 0, 0, 1, 0x80, 0];

    #[test]
    fn a_variation_device_moves_its_coordinate_by_the_rounded_delta() {
        assert!(store().is_some(), "the fixture store parses");
        let t = anchor_table(Some(ITEM_0), Some(ITEM_1));
        assert_eq!(
            at(&t, F2Dot14(16384), store()),
            (110, -17),
            "the whole delta at the peak"
        );
        // A quarter of the way: 25 and -9.25, which rounds to -9.
        assert_eq!(at(&t, F2Dot14(4096), store()), (35, 11));
        // A half: 50 and -18.5, which HarfBuzz's roundf takes to -18.
        assert_eq!(at(&t, F2Dot14(8192), store()), (60, 2));
        assert_eq!(
            at(&t, F2Dot14(0), store()),
            (10, 20),
            "the default instance"
        );
    }

    /// Without a ppem a hinting device moves nothing, as in HarfBuzz; an
    /// offset of 0 is no device; a device cut short, one past the table,
    /// or an index past the store is no delta -- baud's per-subtable rule,
    /// where HarfBuzz refuses the whole GPOS for the malformed ones (Jev B,
    /// 0.84).
    #[test]
    fn a_device_that_is_not_a_variation_index_moves_nothing() {
        // Sizes 0 to 0, which read as outer 0 and inner 0 would name the
        // store's first item: misread, the anchor would move by 100.
        let hinting = [0, 0, 0, 0, 0, 1];
        let t = anchor_table(Some(hinting), None);
        assert_eq!(
            at(&t, F2Dot14(16384), store()),
            (10, 20),
            "a hinting device, and no y device"
        );
        let past = [0, 5, 0, 0, 0x80, 0];
        let t = anchor_table(Some(past), Some(ITEM_1));
        assert_eq!(
            at(&t, F2Dot14(16384), store()),
            (10, -17),
            "an outer index past the store"
        );
        let mut t = anchor_table(Some(ITEM_0), Some(ITEM_1));
        t[8..10].copy_from_slice(&21u16.to_be_bytes());
        assert_eq!(
            at(&t, F2Dot14(16384), store()),
            (10, -17),
            "a device cut short by the table's end"
        );
        t[8..10].copy_from_slice(&0xFFF0u16.to_be_bytes());
        assert_eq!(
            at(&t, F2Dot14(16384), store()),
            (10, -17),
            "a device offset past the table"
        );
    }

    /// **A lookup's reach is read through the extension wrapper**: a mark
    /// attachment wrapped in type 9 walks as one unwrapped does, and a
    /// single adjustment wrapped or not takes the general walk.
    #[test]
    fn a_lookups_reach_is_read_through_the_extension_wrapper() {
        let wrap = |kind: u16, sub: Vec<u8>| -> Vec<u8> {
            let mut e = vec![0, 1];
            e.extend(kind.to_be_bytes());
            e.extend(8u32.to_be_bytes());
            e.extend(sub);
            e
        };
        let mark = vec![0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let t = table(&[
            (4, mark.clone()),
            (6, mark.clone()),
            (1, single_10()),
            (9, wrap(4, mark.clone())),
            (9, wrap(6, mark)),
            (9, wrap(1, single_10())),
        ]);
        let gpos = Gpos::parse(Bytes::new(&t), Gdef::default()).unwrap();
        let reach = |i: u32| Reach::of(&gpos.layout.lookup(Index::of(i)).unwrap());
        let got: Vec<Reach> = (0..6).map(reach).collect();
        assert_eq!(
            got,
            [
                Reach::Attachments,
                Reach::Attachments,
                Reach::Any,
                Reach::Attachments,
                Reach::Attachments,
                Reach::Any,
            ]
        );
    }

    /// Formats 1 and 2 carry no devices, and a format-3 anchor with no
    /// store to index moves nothing.
    #[test]
    fn only_format_three_varies_and_only_with_a_store() {
        let mut t = anchor_table(Some(ITEM_0), Some(ITEM_1));
        t[3] = 1;
        assert_eq!(at(&t, F2Dot14(16384), store()), (10, 20), "format 1");
        t[3] = 2;
        assert_eq!(at(&t, F2Dot14(16384), store()), (10, 20), "format 2");
        let t = anchor_table(Some(ITEM_0), Some(ITEM_1));
        assert_eq!(at(&t, F2Dot14(16384), None), (10, 20), "no store");
    }
}
