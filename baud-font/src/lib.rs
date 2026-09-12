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
//! formats 0, 4, 6, 12, 13 and 14 (`[[source: ms-opentype-cmap]]`). Outlines,
//! bitmap strikes, shaping and rasterisation follow in their own phases.
//!
//! **The oracle is `ttf-parser` as a dev-dependency**, and `tests/oracle.rs`
//! compares every face on this machine field by field. Where the two
//! disagree the test says which, and the specification decides.
#![no_std]
#![forbid(unsafe_code)]

pub mod bytes;
mod cmap;
mod face;

pub use cmap::Variation;
pub use face::{
    Advance, Face, FaceCount, FaceIndex, FontUnits, GlyphCount, LineMetrics, Pitch, Underline,
    UnitsPerEm, faces,
};

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
    pub const TTCF: Tag = Tag(*b"ttcf");

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
