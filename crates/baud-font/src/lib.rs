//! The font stack baud owns: parse, shape, rasterise.
//!
//! **Why this crate exists and what it refuses.** The constraint is *no
//! engine dependencies*: HarfBuzz, FreeType, rustybuzz and swash are
//! reference literature, not dependencies, and this crate reads font files
//! itself. The `[[source: ...]]` keys at each table it reads name the
//! pinned specification behind it.
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
//! **Phase 1b: outlines.** `loca`/`glyf` with composites, CFF Type 2
//! charstrings, and CFF2 charstrings with their `blend` and `vsindex`
//! (`[[source: ms-opentype-cff2]]`), through one [`Sink`] in font units. A
//! face whose only outlines are a `CFF2` table that does not read is refused
//! at load rather than drawn blank.
//!
//! **Phase 1c: bitmap strikes and layered colour.** `sbix`, `CBLC`/`CBDT`
//! and `COLR` v0 with `CPAL`. The PNG decoder those strikes need is
//! `baud_domain::png`, written in the workspace rather than pulled in
//! because a decoder is a dependency in every sense that constraint names,
//! and lives in `baud-domain` so the core decodes the terminal graphics
//! protocol's (APC G) pictures through the same code; it is re-exported here.
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
// `core::f32::math`: `floor`, `ceil`, `round` and `sqrt`, which the
// rasteriser, a flex's depth in pixels (`outline.rs`) and the fixed-point
// roundings use, and which `core` keeps behind a feature and `std` does not.
#![feature(core_float_math)]

mod bake;
mod bitmap;
mod buffer;
pub mod bytes;
mod cff;
mod cff2;
mod cmap;
mod colr;
mod compose {
    //! Canonical composition and decomposition and the canonical combining
    //! classes, compiled by `build/compose.rs` from `[[source: unicode-data]]`
    //! and `[[source: unicode-dnp]]`.
    include!(concat!(env!("OUT_DIR"), "/compose.rs"));
}
mod context;
mod describe;
mod face;
mod forms;
mod glyf;
mod gpos;
mod gsub;
pub mod gvar;
mod hvar;
mod ignorable {
    //! `Default_Ignorable_Code_Point`, compiled by `build/ignorable.rs` from
    //! `[[source: dcp-data]]`.
    include!(concat!(env!("OUT_DIR"), "/ignorable.rs"));
}
mod indic;
mod indic_data {
    //! Each character's class in an Indic syllable and its position, which
    //! the Indic shaper reads (`crate::indic`), compiled by `build/indic.rs`
    //! from `[[source: unicode-indicsyllabiccategory]]`,
    //! `[[source: unicode-indicpositionalcategory]]` and
    //! `[[source: unicode-blocks]]` through HarfBuzz 14.5.1's mapping
    //! (`[[source: harfbuzz-14-5-1-gen-indic-table]]`).
    include!(concat!(env!("OUT_DIR"), "/indic_data.rs"));
}
pub mod ivs;
mod layout;
mod morx;
mod name;
pub mod outline;
mod presentation {
    //! The Arabic presentation forms a face with no `GSUB` forms is drawn
    //! through, compiled by `build/presentation.rs` from the sources pinned
    //! in `docs/research/sources/MANIFEST.toml` (`[[source: unicode-data]]`,
    //! `[[source: unicode-arabicshaping]]`).
    include!(concat!(env!("OUT_DIR"), "/presentation.rs"));
}
mod raster;
mod script;
mod script_data {
    //! The Script property and Script_Extensions, the OpenType script tags
    //! each script selects, each script's direction and the paired
    //! brackets, compiled by `build/script.rs` from the sources pinned in
    //! `docs/research/sources/MANIFEST.toml`.
    include!(concat!(env!("OUT_DIR"), "/script_data.rs"));
}
mod shape;
mod standard_encoding {
    //! The CFF Standard Encoding, code to SID, compiled by
    //! `build/standard_encoding.rs` from Appendix B of
    //! `[[source: adobe-5176-cff]]`, whose columns
    //! `data/adobe-5176-cff-standard-encoding.txt` holds with the
    //! attribution Adobe's licence asks for. Indexed by the code `seac`
    //! supplies; the value is the standard string id the face's charset
    //! then maps to a glyph. Every SID in Standard Encoding is below 150,
    //! so a byte holds it.
    include!(concat!(env!("OUT_DIR"), "/standard_encoding.rs"));
}
mod stroke;
mod syllabic;
pub mod var;

pub use bake::{BakePoint, IupStep, Modified, Refused, bake, bake_partial};
pub use baud_domain::png::{Color, Png, PngError, Size};
pub use bitmap::{
    Bitmap, Mirror, Placement, Ppem, ReadStrike, Records, Strike, StrikeRefusal, StrikeState,
};
pub use buffer::{
    Attached, Buffer, Cluster, ClusterLevel, Glyph, Ignorable, Mark, Pens, Positioned,
};
pub use cmap::Variation;
pub use colr::{Layer, Layers, Palette, PaletteEntry, Rgba};
pub use describe::{Angle, Described, Flavour, Slope, Weight, Width};
pub use face::{
    Advance, Face, FaceCount, FaceIndex, FontUnits, GlyphCount, LineMetrics, Pitch, Underline,
    UnitsPerEm, faces,
};
pub use forms::{lam_alef_form, presentation_form};
pub use glyf::VarPoint;
pub use gpos::Gpos;
pub use gsub::{Gsub, Walked};
pub use layout::{
    Digest, Enabled, FeatureTag, Features, Gdef, GlyphClass, LookupSet, MAX_LOOKUPS, ScriptTag,
};
pub use morx::{Morx, Whole};
pub use name::{Family, NameId};
pub use outline::{Drawn, Flex, FlexDrawn, Sink, Transform};
pub use raster::{Bounds, Canvas, Extent, Fit, InkBox, Phase, Point, Px, Rasterizer, Scale};
pub use script::{Bracket, Direction, Pair, PerScript, RunText, Script, Unresolved, resolve};
pub use shape::{
    CombiningClass, Engine, Kept, MarkPlacement, Normalised, Shaped, Shaper, Substitution,
    combining_class, compose, decompose, each_decomposable, joined_reach, keeps_its_cells,
};
pub use stroke::{Edges, MITER_LIMIT, Reach, Stroker};
pub use syllabic::{Category, Formed, Masks, Position, Syllabic, Syllable};

/// A four-byte table tag, as the directory spells it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tag([u8; 4]);

impl Tag {
    pub const HEAD: Tag = Tag(*b"head");
    pub const MAXP: Tag = Tag(*b"maxp");
    pub const HHEA: Tag = Tag(*b"hhea");
    pub const HMTX: Tag = Tag(*b"hmtx");
    pub const VHEA: Tag = Tag(*b"vhea");
    pub const VMTX: Tag = Tag(*b"vmtx");
    pub const CMAP: Tag = Tag(*b"cmap");
    pub const OS2: Tag = Tag(*b"OS/2");
    pub const POST: Tag = Tag(*b"post");
    pub const LOCA: Tag = Tag(*b"loca");
    pub const GLYF: Tag = Tag(*b"glyf");
    pub const CFF: Tag = Tag(*b"CFF ");
    pub const CFF2: Tag = Tag(*b"CFF2");
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

/// What a caller says when it refuses a face: "no 'head' table", "its
/// 'CFF2' table does not read". A tag's bytes are printed as they are,
/// which every registered tag spells in ASCII.
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let tag = |t: &Tag| t.0.map(char::from);
        match self {
            Error::NotAFont => f.write_str("not a font"),
            Error::FaceOutOfRange => f.write_str("the collection has no face at that index"),
            Error::MissingTable(t) => {
                let [a, b, c, d] = tag(t);
                write!(f, "no '{a}{b}{c}{d}' table")
            }
            Error::Malformed(t) => {
                let [a, b, c, d] = tag(t);
                write!(f, "its '{a}{b}{c}{d}' table does not read")
            }
        }
    }
}
