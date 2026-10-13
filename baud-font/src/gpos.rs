//! `GPOS` (`[[source: ms-opentype-gpos]]`): mark positioning, and only
//! that.
//!
//! **Types 4, 6 and 9.** Mark-to-base and mark-to-mark attachment through
//! anchors, and the extension wrapper. A grid has no use for kerning (type
//! 2) or cursive attachment (3): every base glyph sits at its cell origin
//! by Windows Terminal's rule (D8), and what is left to position is where
//! a combining mark goes relative to that base. Anchors are read in their
//! design-unit form (formats 1, 2 and 3 all start with one); the contour
//! point and the hinting devices are hinting's and D7 has none.
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
//! changes nothing.
//! A device that does not check out is no device: that coordinate does
//! not vary, where HarfBuzz refuses the whole `GPOS` (Jev B, 0.84).
//!
//! **Offsets are from the cell origin.** Before any lookup runs every
//! glyph's `dx` is its pen position within the cluster, so a positioned
//! mark's `dx`/`dy` becomes the target's plus `target anchor - mark
//! anchor` in font units, and a stacked mark rides on the mark below it.
//! HarfBuzz reports the same geometry relative to the mark's own pen
//! position, one base advance further along; `tests/shape.rs` translates.

use crate::buffer::{Buffer, Glyph, Ignorable, Positioned};
use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::ivs::{Store, VarIndex};
use crate::layout::{
    Coverage, Features, Filter, Gdef, Kind, Layout, Lookup, LookupSet, ScriptTag, Skip,
};
use crate::script::Script;
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
    let Some(store) = vary.store else {
        return 0;
    };
    let Some(off) = anchor.read::<u16>(at).filter(|o| *o != 0) else {
        return 0;
    };
    let Some(device) = anchor.tail(Offset::of(u32::from(off))) else {
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

    pub fn parse(data: Bytes<'a>, gdef: Gdef<'a>) -> Option<Gpos<'a>> {
        Some(Gpos {
            layout: Layout::parse(data, gdef)?,
        })
    }

    /// Applies the mark lookups the features name for text in `script`, in
    /// lookup order, with the anchors read at `coords`.
    pub fn apply(
        &self,
        buf: &mut Buffer<'_>,
        script: Script,
        features: Features,
        coords: &Coords,
    ) -> Count {
        let vary = Vary {
            store: self.layout.gdef().store(),
            coords,
        };
        let set = self.layout.lookups_for(script, features);
        for i in set.iter() {
            if let Some(lookup) = self.layout.lookup(i) {
                let mut pos = Index::ZERO;
                while let Some(g) = buf.get(pos) {
                    if lookup.filter.skip_class(g.class, g.id) == Skip::No {
                        self.apply_at(&lookup, buf, pos, vary);
                    }
                    pos = pos.next();
                }
            }
        }
        set.declined()
    }

    fn apply_at(&self, lookup: &Lookup<'a>, buf: &mut Buffer<'_>, pos: Index, vary: Vary<'_, 'a>) {
        let mut i = Index::ZERO;
        while lookup.subtable_count().holds(i) {
            let Some((kind, sub)) = lookup.subtable(i, Kind::GPOS_EXTENSION) else {
                return;
            };
            let done = match kind {
                Kind::MARK_TO_BASE => {
                    self.mark_to(sub, &lookup.filter, buf, pos, Target::Base, vary)
                }
                Kind::MARK_TO_MARK => {
                    self.mark_to(sub, &lookup.filter, buf, pos, Target::Mark, vary)
                }
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

/// What a mark lookup attaches to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Base,
    Mark,
}

#[cfg(test)]
mod tests {
    use super::*;
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
