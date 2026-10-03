//! The font stack baud owns: parse, shape, rasterise.
//!
//! **Why this crate exists and what it refuses.** The owner's constraint is
//! *no engine dependencies*: HarfBuzz, FreeType, rustybuzz and swash are
//! reference literature, not dependencies, and this crate reads font files
//! itself. `docs/research/notes/j-rendering.md` is the decisions register
//! (D1 names the crate, D6-D11 decide what it parses and how it rasterises,
//! D8-D9 how it shapes) and `j-rendering-evidence.md` carries the pinned
//! specifications behind every table it reads.
//!
//! **The contract is the core's.** `no_std`, `forbid(unsafe_code)`, and one
//! dependency -- `baud-domain`, for the vocabulary a renderer and the core
//! already share (`Codepoint`, `GlyphId`). Every table is a bounds-checked
//! view over borrowed bytes ([`bytes::Bytes`]), and nothing in a file is
//! trusted: a font from disk is the same class of input as bytes from a pty,
//! and the hostile-input property in `tests/hostile.rs` is the gate on that.
//!
//! **What is here so far: phase 1a of the plan.** The sfnt and collection
//! directory (`[[source: ms-opentype-otff]]`), `head`, `maxp`, `hhea`, `hmtx`,
//! `OS/2` and `post` (`[[source: ms-opentype-head]]`, `[[source: ms-opentype-maxp]]`,
//! `[[source: ms-opentype-hhea]]`, `[[source: ms-opentype-hmtx]]`,
//! `[[source: ms-opentype-os2]]`, `[[source: ms-opentype-post]]`), and `cmap`
//! formats 0, 4, 6, 12, 13 and 14 (`[[source: ms-opentype-cmap]]`).
//!
//! **Phase 1b: outlines.** `loca`/`glyf` with composites, and CFF Type 2
//! charstrings, through one [`Sink`] in font units.
//!
//! **Phase 1c: bitmap strikes and layered colour.** `sbix`, `CBLC`/`CBDT`
//! and `COLR` v0 with `CPAL`, plus the PNG decoder those strikes need --
//! the one piece of general-purpose code in the crate, written here rather
//! than pulled in because a decoder is a dependency in every sense the
//! owner's constraint names.
//!
//! **Phase 2: the rasteriser.** Signed-area accumulation into a caller's
//! `f32` scratch, then a prefix sum into an 8-bit coverage mask -- the
//! font-rs form the register (D6) chose, held bit-exact to
//! `ab_glyph_rasterizer` in `tests/raster.rs` and within a tolerance of
//! FreeType's unhinted smooth rasteriser. Shaping follows in its own phase.
//!
//! **The oracle is `ttf-parser` as a dev-dependency**, and `tests/oracle.rs`
//! compares every face on this machine field by field. Where the two
//! disagree the test says which, and the specification decides.
#![no_std]
#![forbid(unsafe_code)]
// `core::f32::math::{floor, ceil, sqrt}`: the rasteriser's three float
// operations that `core` keeps behind a feature and `std` does not.
#![feature(core_float_math)]

mod bake;
mod bitmap;
mod buffer;
pub mod bytes;
mod cff;
mod cmap;
mod colr;
mod compose;
mod face;
mod glyf;
mod gpos;
mod gsub;
pub mod gvar;
mod hvar;
mod ignorable;
pub mod ivs;
mod layout;
mod morx;
pub mod outline;
mod png;
mod raster;
mod shape;
mod standard_encoding;
pub mod var;

pub use bake::{BakePoint, Refused, bake};
pub use bitmap::{
    Bitmap, Mirror, Placement, Ppem, ReadStrike, Records, Strike, StrikeRefusal, StrikeState,
};
pub use buffer::{Attached, Buffer, Cluster, Glyph, Ignorable, Mark, Positioned};
pub use cmap::Variation;
pub use colr::{Layer, Layers, Palette, PaletteEntry, Rgba};
pub use face::{
    Advance, Face, FaceCount, FaceIndex, FontUnits, GlyphCount, LineMetrics, Pitch, Underline,
    UnitsPerEm, faces,
};
pub use glyf::VarPoint;
pub use gpos::Gpos;
pub use gsub::{Gsub, Walked};
pub use layout::{Digest, Enabled, FeatureTag, Features, Gdef, GlyphClass, MAX_LOOKUPS};
pub use morx::Morx;
pub use outline::{Drawn, Sink, Transform};
pub use png::{Color, Png, PngError, Size};
pub use raster::{Bounds, Canvas, Extent, Fit, InkBox, Phase, Point, Px, Rasterizer, Scale};
pub use shape::{CombiningClass, Shaped, Shaper};

/// A four-byte table tag, as the directory spells it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tag([u8; 4]);

impl Tag {
    pub const HEAD: Tag = Tag(*b"head");
    pub const MAXP: Tag = Tag(*b"maxp");
    pub const HHEA: Tag = Tag(*b"hhea");
    pub const HMTX: Tag = Tag(*b"hmtx");
    pub const CMAP: Tag = Tag(*b"cmap");
    pub const OS2: Tag = Tag(*b"OS/2");
    pub const POST: Tag = Tag(*b"post");
    pub const LOCA: Tag = Tag(*b"loca");
    pub const GLYF: Tag = Tag(*b"glyf");
    pub const CFF: Tag = Tag(*b"CFF ");
    pub const TTCF: Tag = Tag(*b"ttcf");
    pub const SBIX: Tag = Tag(*b"sbix");
    pub const CBLC: Tag = Tag(*b"CBLC");
    pub const CBDT: Tag = Tag(*b"CBDT");
    pub const COLR: Tag = Tag(*b"COLR");
    pub const CPAL: Tag = Tag(*b"CPAL");
    pub const GDEF: Tag = Tag(*b"GDEF");
    pub const GSUB: Tag = Tag(*b"GSUB");
    pub const GPOS: Tag = Tag(*b"GPOS");
    pub const MORX: Tag = Tag(*b"morx");
    pub const FVAR: Tag = Tag(*b"fvar");
    pub const AVAR: Tag = Tag(*b"avar");
    pub const GVAR: Tag = Tag(*b"gvar");
    pub const HVAR: Tag = Tag(*b"HVAR");

    pub const fn from_bytes(b: [u8; 4]) -> Tag {
        Tag(b)
    }

    pub const fn as_bytes(self) -> [u8; 4] {
        self.0
    }
}

/// Why a face could not be read. **Every variant is a property of the
/// bytes**, never of the caller: a font that parses once parses always.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Error {
    /// Neither an sfnt version this crate knows nor a collection.
    NotAFont,
    /// A collection with fewer faces than the index asked for.
    FaceOutOfRange,
    /// A table every face must carry is absent or lies outside the file.
    MissingTable(Tag),
    /// A table is present and does not say what its specification requires.
    Malformed(Tag),
}
