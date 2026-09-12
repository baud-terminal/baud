//! `GPOS` (`[[source: ms-opentype-gpos]]`): mark positioning, and only
//! that.
//!
//! **Types 4, 6 and 9.** Mark-to-base and mark-to-mark attachment through
//! anchors, and the extension wrapper. A grid has no use for kerning (type
//! 2) or cursive attachment (3): every base glyph sits at its cell origin
//! by Windows Terminal's rule (D8), and what is left to position is where
//! a combining mark goes relative to that base. Anchors are read in their
//! design-unit form (formats 1, 2 and 3 all start with one); the contour
//! point and device refinements are hinting's and D7 has none.
//!
//! **Offsets are from the cell origin.** Before any lookup runs every
//! glyph's `dx` is its pen position within the cluster, so a positioned
//! mark's `dx`/`dy` becomes the target's plus `target anchor - mark
//! anchor` in font units, and a stacked mark rides on the mark below it.
//! HarfBuzz reports the same geometry relative to the mark's own pen
//! position, one base advance further along; `tests/shape.rs` translates.

use crate::buffer::{Buffer, Glyph, Ignorable, Positioned};
use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::layout::{Coverage, Features, Filter, Gdef, Kind, Layout, Lookup, Skip};

/// An anchor's design-unit coordinates, the head of every anchor format.
#[derive(Clone, Copy)]
struct Anchor {
    x: i16,
    y: i16,
}

fn anchor(table: Bytes<'_>, at: Offset) -> Option<Anchor> {
    let off: u16 = table.read(at)?;
    if off == 0 {
        return None;
    }
    let a = table.tail(Offset::of(u32::from(off)))?;
    let format: u16 = a.read(Offset::ZERO)?;
    if !(1..=3).contains(&format) {
        return None;
    }
    Some(Anchor {
        x: a.read(Offset::of(2u32))?,
        y: a.read(Offset::of(4u32))?,
    })
}

#[derive(Clone, Copy)]
pub struct Gpos<'a> {
    layout: Layout<'a>,
}

impl<'a> Gpos<'a> {
    pub fn parse(data: Bytes<'a>, gdef: Gdef<'a>) -> Option<Gpos<'a>> {
        Some(Gpos {
            layout: Layout::parse(data, gdef)?,
        })
    }

    /// Applies the mark lookups the features name, in lookup order.
    pub fn apply(&self, buf: &mut Buffer<'_>, features: Features) -> Count {
        let set = self.layout.lookups_for(features);
        for i in set.iter() {
            if let Some(lookup) = self.layout.lookup(i) {
                let mut pos = Index::ZERO;
                while let Some(g) = buf.get(pos) {
                    if lookup.filter.skip(g.id) == Skip::No {
                        self.apply_at(&lookup, buf, pos);
                    }
                    pos = pos.next();
                }
            }
        }
        set.declined()
    }

    fn apply_at(&self, lookup: &Lookup<'a>, buf: &mut Buffer<'_>, pos: Index) {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            let Some((kind, sub)) = lookup.subtable(i, Kind::GPOS_EXTENSION) else {
                return;
            };
            let done = match kind {
                Kind::MARK_TO_BASE => self.mark_to(sub, &lookup.filter, buf, pos, Target::Base),
                Kind::MARK_TO_MARK => self.mark_to(sub, &lookup.filter, buf, pos, Target::Mark),
                _ => None,
            };
            if done.is_some() {
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
            if filter.skip(g.id) == Skip::Yes || g.ignorable == Ignorable::Yes {
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
        let mark_anchor = anchor(marks, rec.plus(Len::of(2))?)?;
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
        let target_anchor = anchor(targets, rec)?;
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

/// What a mark lookup attaches to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Base,
    Mark,
}
