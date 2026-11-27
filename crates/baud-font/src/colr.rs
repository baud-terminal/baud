//! `COLR` version 0 and `CPAL` (`[[source: ms-opentype-colr]]`,
//! `[[source: ms-opentype-cpal]]`).
//!
//! **Layered glyphs.** A colour glyph in `COLR` v0 is a list of layers, each
//! an ordinary glyph drawn in one palette colour, painted in order from the
//! first layer at the bottom. `CPAL` holds the palettes as BGRA bytes. That
//! is the whole of version 0, and it is why the register (D10) keeps it in
//! v1 and defers `COLR` version 1 -- a paint graph with gradients and
//! transforms -- to its own phase.
//!
//! A version-1 table still carries the version-0 arrays, and this reader
//! reads them; what it does not read is the `BaseGlyphPaintRecord`s that
//! version 1 adds, so a glyph that only version 1 describes answers `None`.

use crate::bytes::{Array, Bytes, Count, FromBe, Index, Len, Offset};
use baud_domain::GlyphId;

/// An index into a palette.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaletteEntry(u16);

impl PaletteEntry {
    /// `0xFFFF`: the text foreground colour, whatever the renderer's is.
    pub const FOREGROUND: PaletteEntry = PaletteEntry(0xFFFF);

    pub const fn of(i: u16) -> PaletteEntry {
        PaletteEntry(i)
    }

    pub const fn value(self) -> u16 {
        self.0
    }
}

/// Which palette; most fonts carry one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette(u16);

impl Palette {
    pub const FIRST: Palette = Palette(0);

    pub const fn of(i: u16) -> Palette {
        Palette(i)
    }
}

/// A colour from `CPAL`, unpremultiplied, in the byte order a renderer
/// wants rather than the table's BGRA.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

/// One layer: the glyph to draw and the palette entry to draw it in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Layer {
    pub glyph: GlyphId,
    pub color: PaletteEntry,
}

impl FromBe for Layer {
    const SIZE: Len = Len::of(4);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Layer {
            glyph: GlyphId::from_shaper(u16::parse_be(b)?),
            color: PaletteEntry(u16::parse_be(b.get(2..)?)?),
        })
    }
}

/// A `BaseGlyphRecord`: the glyph, its first layer, its layer count.
#[derive(Clone, Copy)]
struct Base(u16, u16, u16);

impl FromBe for Base {
    const SIZE: Len = Len::of(6);
    fn parse_be(b: &[u8]) -> Option<Self> {
        Some(Base(
            u16::parse_be(b)?,
            u16::parse_be(b.get(2..)?)?,
            u16::parse_be(b.get(4..)?)?,
        ))
    }
}

#[derive(Clone, Copy)]
pub struct Colr<'a> {
    bases: Array<'a, Base>,
    layers: Array<'a, Layer>,
}

impl<'a> Colr<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Colr<'a>> {
        let n_bases: u16 = data.read(Offset::of(2u32))?;
        let bases_at: u32 = data.read(Offset::of(4u32))?;
        let layers_at: u32 = data.read(Offset::of(8u32))?;
        let n_layers: u16 = data.read(Offset::of(12u32))?;
        Some(Colr {
            bases: data.array(Offset::of(bases_at), Count::of(n_bases))?,
            layers: data.array(Offset::of(layers_at), Count::of(n_layers))?,
        })
    }

    /// The layers of `g`, bottom first, or `None` for a glyph with none.
    /// The base records are sorted by glyph id, so this is a binary search.
    pub fn layers(&self, g: GlyphId) -> Option<Layers<'a>> {
        let gid = u16::try_from(g.index()).ok()?;
        let i = self.bases.partition_point(|b| b.0.cmp(&gid))?;
        let base = self.bases.get(i)?;
        if base.0 != gid {
            return None;
        }
        let first = Index::of(base.1);
        let count = Count::of(base.2);
        // Every layer must exist, or the record lies.
        let last = first.plus(count)?.prev()?;
        if !self.layers.count().holds(last) {
            return None;
        }
        Some(Layers {
            layers: self.layers,
            next: first,
            end: first.plus(count)?,
        })
    }
}

/// An iterator over a glyph's layers.
#[derive(Clone, Copy)]
pub struct Layers<'a> {
    layers: Array<'a, Layer>,
    next: Index,
    end: Index,
}

impl Iterator for Layers<'_> {
    type Item = Layer;
    fn next(&mut self) -> Option<Layer> {
        if self.next >= self.end {
            return None;
        }
        let l = self.layers.get(self.next)?;
        self.next = self.next.next();
        Some(l)
    }
}

#[derive(Clone, Copy)]
pub struct Cpal<'a> {
    data: Bytes<'a>,
    entries: Count,
    palettes: Count,
    records_at: Offset,
}

impl<'a> Cpal<'a> {
    pub fn parse(data: Bytes<'a>) -> Option<Cpal<'a>> {
        let entries: u16 = data.read(Offset::of(2u32))?;
        let palettes: u16 = data.read(Offset::of(4u32))?;
        let records_at: u32 = data.read(Offset::of(8u32))?;
        Some(Cpal {
            data,
            entries: Count::of(entries),
            palettes: Count::of(palettes),
            records_at: Offset::of(records_at),
        })
    }

    pub fn palettes(&self) -> Count {
        self.palettes
    }

    /// Entry `e` of palette `p`: the palette's first-record index, from
    /// `colorRecordIndices`, plus the entry, into the BGRA records.
    pub fn color(&self, p: Palette, e: PaletteEntry) -> Option<Rgba> {
        let pi = Index::of(p.0);
        if !self.palettes.holds(pi) {
            return None;
        }
        let ei = Index::of(e.0);
        if !self.entries.holds(ei) {
            return None;
        }
        let first: u16 = self.data.read(Offset::of(12u32).at(pi, Len::of(2))?)?;
        let rec = self
            .records_at
            .at(Index::of(first).plus(Count::of(e.0))?, Len::of(4))?;
        let b: u8 = self.data.read(rec)?;
        let g: u8 = self.data.read(rec.plus(Len::of(1))?)?;
        let r: u8 = self.data.read(rec.plus(Len::of(2))?)?;
        let a: u8 = self.data.read(rec.plus(Len::of(3))?)?;
        Some(Rgba { r, g, b, a })
    }
}
