use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// A materialised cell. Carries both brands: it is only valid for one geometry
/// and one font epoch.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Cell<'font> {
    pub slot: AtlasSlot<'font>,
    pub style: StyleId,
}

/// The brand-free form, for the one place a viewport actually crosses a byte
/// boundary: upload to the GPU.
///
/// `Cell<'font>` carries a lifetime so a stale atlas slot cannot outlive a font
/// change; a lifetime cannot be `FromBytes`. So the branded form is what the
/// domain uses, and this is what gets uploaded — the conversion is a
/// `repr(transparent)` reinterpretation, not a copy.
/// What a cell holds. **Content, not appearance.**
///
/// Values above `U+10FFFF` cannot be codepoints, so the reserved states are
/// unambiguous without a discriminant byte. The core never decides that "blank"
/// looks like `U+0020` — that conflates *nothing was written* with *a space was
/// written*, which differ for selection (copying a line must not yield trailing
/// spaces), for reflow, and for equality.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Content(pub(crate) u32);

impl Content {
    /// Nothing was written here. The renderer draws background and no glyph.
    pub const EMPTY: Content = Content(0x0011_0000);
    /// The right half of a double-width character. The renderer skips it; the
    /// glyph in the preceding cell covers both columns.
    pub const WIDE_TAIL: Content = Content(0x0011_0001);

    /// A decoded codepoint. Full range — no truncation, so astral-plane
    /// characters survive. `U+1F680 & 0xffff` was `U+F680`, a private-use
    /// codepoint, which is what the differential test caught.
    pub fn codepoint(c: Codepoint) -> Content {
        Content(c.value())
    }
    pub fn as_codepoint(self) -> Option<Codepoint> {
        Codepoint::new(self.0)
    }
    pub fn is_empty(self) -> bool {
        self == Content::EMPTY
    }
    pub fn is_wide_tail(self) -> bool {
        self == Content::WIDE_TAIL
    }
}

impl Default for Content {
    fn default() -> Self {
        Content::EMPTY
    }
}

/// One materialised cell: content plus a style index.
///
/// Eight bytes rather than four. Measured cost of that width: 8-byte cells hit
/// 48% of the kernel ceiling against 50% for 4-byte — two points, for holding a
/// full codepoint instead of a truncated one.
#[repr(C)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct PackedCell {
    pub content: Content,
    pub style: StyleId,
    pub flags: CellFlags,
}

/// Per-cell state that is **not** a rendition.
///
/// Lives in what used to be `PackedCell::_pad`, so the cell is still eight
/// bytes and the measurement behind that width still holds.
///
/// The distinction is the point. A rendition is interned: many cells share one
/// [`StyleId`], and the table behind it is a direct-mapped cache that evicts.
/// That is tolerable for colour -- an evicted bucket renders wrong until it is
/// rewritten -- and not tolerable for DECSCA protection, where eviction would
/// silently expose text an application asked to keep. Anything whose loss is a
/// correctness bug rather than a cosmetic one belongs here, per cell, where
/// nothing can evict it.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct CellFlags(pub(crate) u16);

/// Which hyperlink a cell belongs to, or none.
///
/// Zero is "no link", so a zeroed grid has no hyperlinks and there is no
/// parallel validity array to keep in step -- the same trick `ColorWord`
/// uses for the default colour.
#[repr(transparent)]
#[derive(
    Clone, Copy, Default, PartialEq, Eq, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct LinkId(pub(crate) u16);

impl LinkId {
    pub const NONE: LinkId = LinkId(0);
    pub fn from_index(i: usize) -> LinkId {
        LinkId(i as u16)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }

    /// Which slot an `OSC 8` link hashes to.
    ///
    /// The twin of [`StyleId::bucket`], and it takes the **high** bits for the
    /// same measured reason: the low bits of a multiplicative hash are the ones
    /// that collide, and this codebase has already lost a whole style table to
    /// masking the wrong end.
    ///
    /// Slot 0 is reserved for "no link", so the range is `1..buckets`.
    ///
    /// The `id` is hashed alongside the URI because the spec lets two runs with
    /// the same `id` be one logical link across lines; distinct ids must not
    /// collapse into a single slot merely because their URIs match.
    pub fn bucket(id: &[u8], uri: &[u8], buckets: usize) -> LinkId {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in id.iter().chain(uri) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let spread = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32;
        LinkId::from_index(1 + (spread as usize) % (buckets - 1))
    }
}

impl CellFlags {
    pub const NONE: CellFlags = CellFlags(0);
    /// DECSCA: DECSED and DECSEL may not erase this cell.
    ///
    /// `[[source: vt510-decsca]]`: "1 DECSED and DECSEL cannot erase
    /// characters."
    pub const PROTECTED: CellFlags = CellFlags(1);
    /// The byte in this cell was written while the DEC Special Character and
    /// Line Drawing Set was invoked into GL.
    ///
    /// The cell keeps the byte the application sent, plus this flag, rather
    /// than a translated codepoint. That is deliberate and it is a *sourcing*
    /// decision, not a design preference: `[[source: ctlseqs]]` names the set
    /// (`C = 0 -> DEC Special Character and Line Drawing Set, VT100`) but does
    /// not give the byte-to-glyph mapping, and `MANIFEST.toml` records
    /// `dec-special-graphics` as `gated` -- vt100.net publishes the table as a
    /// figure, so no claim may rest on it. Translating here would mean writing
    /// the table from memory, which this project has been wrong about before.
    ///
    /// Carrying the flag loses nothing: a renderer that has the table has
    /// everything it needs, and one that does not can say so. Translating to a
    /// guessed codepoint would be unrecoverable.
    pub const GRAPHICS: CellFlags = CellFlags(2);
    pub fn is_graphics(self) -> bool {
        self.0 & CellFlags::GRAPHICS.0 != 0
    }
    pub fn with(self, o: CellFlags) -> CellFlags {
        CellFlags(self.0 | o.0)
    }

    /// Bits 2..16 carry the [`LinkId`].
    ///
    /// **The link rides in the spare bits of this word rather than in a field
    /// of its own, and that is a measured constraint, not a packing game.**
    /// `CLAUDE.md`: "`PackedCell` is 8 bytes" — going from 4 to 8 already cost
    /// two points of the kernel ceiling, and a ninth and tenth byte would round
    /// the cell to twelve. Two flag bits were in use and fourteen were not, so
    /// a link index fits with 14 bits of room, against `NLINK` of 128.
    const LINK_SHIFT: u16 = 2;

    pub fn with_link(self, l: LinkId) -> CellFlags {
        let flags = self.0 & ((1 << CellFlags::LINK_SHIFT) - 1);
        CellFlags(flags | ((l.index() as u16) << CellFlags::LINK_SHIFT))
    }
    pub fn link(self) -> LinkId {
        LinkId::from_index((self.0 >> CellFlags::LINK_SHIFT) as usize)
    }
    pub fn protection(on: bool) -> CellFlags {
        CellFlags(u16::from(on))
    }
    pub fn is_protected(self) -> bool {
        self.0 & CellFlags::PROTECTED.0 != 0
    }
    /// Just the flag bits, with any link index masked off.
    ///
    /// Interning and equality use the whole word, so two cells in different
    /// links are different cells — which is correct, and is why this exists
    /// separately for the places that mean *only* the flags.
    pub fn bare(self) -> CellFlags {
        CellFlags(self.0 & ((1 << CellFlags::LINK_SHIFT) - 1))
    }
    /// The byte, for the one place a flag set has to survive a round trip
    /// through [`Run`]'s fixed layout.
    pub fn bits(self) -> u8 {
        self.0 as u8
    }
    pub fn from_bits(b: u8) -> CellFlags {
        CellFlags(b as u16)
    }
}

/// Materialised rows currently on screen — a projection, never storage.
///
/// Over-provisioned and never reallocated: resize changes `cols`/`rows`, not
/// the allocation. Measured flat from a tight fit up to a 400 MB matrix,
/// because untouched pages are never faulted in.
///
/// Carrying `'layout` and `'font` means a resize or font change forces
/// re-materialisation — the compiler will not let a stale viewport be read.
pub struct Viewport<'layout, 'font, 'buf> {
    cells: &'buf mut [Cell<'font>],
    stride: Cols,
    _l: Brand<'layout>,
}

impl<'layout, 'font, 'buf> Viewport<'layout, 'font, 'buf> {
    pub fn stride(&self) -> Cols {
        self.stride
    }
    /// Indexing requires a [`RowIdx`] from the matching [`Layout`], so a row
    /// number computed at a different width cannot be used here.
    pub fn row(&self, l: &Layout<'layout>, r: RowIdx<'layout>) -> &[Cell<'font>] {
        let start = l.row_value(r).index() * self.stride.index();
        &self.cells[start..start + l.cols().index()]
    }
}

impl PackedCell {
    /// Unwritten. Note this is *not* a space: the renderer decides what an empty
    /// cell looks like, and selection can tell the difference.
    pub const EMPTY: PackedCell = PackedCell {
        content: Content::EMPTY,
        style: StyleId::DEFAULT,
        flags: CellFlags::NONE,
    };
    /// An erased cell.
    ///
    /// Takes no flags on purpose: an erase leaves a position with no character
    /// in it, and `[[source: vt510-decsca]]` scopes protection to "characters
    /// that follow the DECSCA function". A blank that inherited the protection
    /// would make the first selective erase under `DECSCA 1` protect everything
    /// it had just cleared.
    pub fn empty_with(style: StyleId) -> PackedCell {
        PackedCell {
            content: Content::EMPTY,
            style,
            flags: CellFlags::NONE,
        }
    }
    /// A printable ASCII byte. The proof that it is printable is also the proof
    /// that its codepoint equals its byte value.
    pub fn from_ascii(b: StreamByte, style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::codepoint(Codepoint::from_ascii(b)),
            style,
            flags,
        }
    }
    pub fn glyph(c: Codepoint, style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::codepoint(c),
            style,
            flags,
        }
    }
    pub fn wide_tail(style: StyleId, flags: CellFlags) -> PackedCell {
        PackedCell {
            content: Content::WIDE_TAIL,
            style,
            flags,
        }
    }
    pub fn is_protected(self) -> bool {
        self.flags.is_protected()
    }
    /// This cell's style and flags with a different character in it.
    ///
    /// The whole point is that the tail is *already assembled*. `PackedCell` is
    /// `{u32, u16, u8, u8}`, and building one field-by-field inside a per-cell
    /// loop let the compiler emit four narrow stores instead of one 8-byte one
    /// -- measured at ~8% of the cells path when the flags byte was carved out
    /// of the old `_pad`. Hoisting the invariant tail out of the loop and only
    /// replacing `content` puts it back.
    #[inline(always)]
    pub fn with_ascii(self, b: StreamByte) -> PackedCell {
        PackedCell {
            content: Content::codepoint(Codepoint::from_ascii(b)),
            ..self
        }
    }
}
