use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// A glyph within one font face, as the shaper resolved it. Font-specific:
/// glyph 47 is a different character in every font, which is why this is
/// only ever stored beside the face it came from and never in a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct GlyphId(pub(crate) u16);

impl GlyphId {
    /// Glyph 0, `.notdef` in every sfnt: what a `cmap` answers for a
    /// character the font does not carry, and never a glyph a renderer should
    /// draw as if it were the character.
    pub const NOTDEF: GlyphId = GlyphId(0);

    pub fn from_shaper(n: u16) -> GlyphId {
        GlyphId(n)
    }

    /// The glyph's position in the font's own tables -- `hmtx`, `loca`, the
    /// charstrings -- named so the widening is visible where a table is
    /// indexed.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}
