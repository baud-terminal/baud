//! `HVAR`: how each glyph's advance moves across the variation space. Step
//! 3 of note Z. The format is `[[source: ms-opentype-hvar]]`; it accepts what
//! HarfBuzz 12.3.0 accepts (`[[source: harfbuzz-12-3-0-hvar]]`): a table whose
//! major version is not 1, or any of whose subtables -- the store, the
//! advance map, the side-bearing maps -- does not check out, is absent, and
//! the face then takes its advances from `gvar`'s phantom points. A
//! subtable whose offset is 0 is null: no store varies nothing, no advance
//! map maps a glyph to itself.
//!
//! The side-bearing maps are checked and not read: nothing in baud reads a
//! side bearing at a variation.

use crate::bytes::{Bytes, Index, Offset};
use crate::ivs::{Map, Store, Subtable};
use crate::var::Coords;
use baud_domain::GlyphId;

#[derive(Clone, Copy)]
pub struct Hvar<'a> {
    store: Option<Store<'a>>,
    advances: Option<Map<'a>>,
}

impl<'a> Hvar<'a> {
    /// majorVersion 0, minorVersion 2, itemVariationStoreOffset 4,
    /// advanceWidthMappingOffset 8, then the side-bearing maps' offsets:
    /// twenty bytes.
    pub fn parse(t: Bytes<'a>) -> Option<Hvar<'a>> {
        let major: u16 = t.read(Offset::ZERO)?;
        let _: u32 = t.read(Offset::of(16u32))?;
        if major != 1 {
            return None;
        }
        let at = |o: u32| -> Option<Subtable<'a>> {
            let offset: u32 = t.read(Offset::of(o))?;
            Subtable::at(t, Offset::of(offset))
        };
        let store = match at(4)? {
            Subtable::Null => None,
            Subtable::At(b) => Some(Store::parse(b)?),
        };
        let map = |o: u32| -> Option<Option<Map<'a>>> {
            match at(o)? {
                Subtable::Null => Some(None),
                Subtable::At(b) => Map::parse(b).map(Some),
            }
        };
        let advances = map(8)?;
        map(12)?;
        map(16)?;
        Some(Hvar { store, advances })
    }

    /// Glyph `g`'s advance delta at `coords`, unrounded; 0 with no store.
    pub fn delta(&self, g: GlyphId, coords: &Coords) -> f32 {
        let Some(store) = self.store else {
            return 0.0;
        };
        let Ok(i) = u32::try_from(g.index()) else {
            return 0.0;
        };
        store.delta(Map::map(self.advances, Index::of(i)), coords)
    }
}
