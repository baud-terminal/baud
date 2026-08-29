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

    /// Bit 2: this cell carries entries in the marks table.
    ///
    /// **The flag is in the cell so that overwriting the cell clears it.**
    /// Marks live in a side table keyed on cell identity, and a key alone
    /// cannot see a cell being rewritten *in place*: no scroll, no blank, no
    /// generation bump, so a space written over `e`-plus-acute inherited the
    /// accent and a renderer drew it on a blank. Every write builds a fresh
    /// `PackedCell` from the terminal's current flags, which never include
    /// this bit, so the stale marks become unreachable without the write path
    /// doing anything at all.
    ///
    /// It also takes the hash off the read path: a cell without this bit has
    /// no marks, and that is the overwhelming majority of cells, answered by
    /// a bit that is already in cache rather than by a probe into 22 KB.
    /// `[[source: ghostty-page]]` reaches the same shape from the other
    /// direction: `appendGrapheme` tests `Cell.content_tag` before touching
    /// `grapheme_map`, so the map is consulted only for cells that have one.
    pub const MARKED: CellFlags = CellFlags(4);

    /// Bits 3..16 carry the [`LinkId`].
    ///
    /// **The link rides in the spare bits of this word rather than in a field
    /// of its own, and that is a measured constraint, not a packing game.**
    /// `CLAUDE.md`: "`PackedCell` is 8 bytes" — going from 4 to 8 already cost
    /// two points of the kernel ceiling, and a ninth and tenth byte would round
    /// the cell to twelve. Three flag bits are in use and thirteen are not, so
    /// a link index fits with 13 bits of room, against `NLINK` of 128.
    const LINK_SHIFT: u16 = 3;

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
    pub fn is_marked(self) -> bool {
        self.0 & CellFlags::MARKED.0 != 0
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
    /// An ASCII byte under whatever character set `flags` says is invoked.
    ///
    /// The one constructor for "a byte from the wire became a cell", so the
    /// DEC Special Graphics translation cannot be applied on one path and
    /// forgotten on another. `promote` replays stored runs and had exactly that
    /// shape: the run carries the flags, and translating only in
    /// `put_ascii_run` would have made a line's content depend on whether it
    /// had been promoted yet.
    pub fn from_wire(b: StreamByte, style: StyleId, flags: CellFlags) -> PackedCell {
        let proto = PackedCell {
            content: Content::EMPTY,
            style,
            flags,
        };
        proto.with_charset(b, Charset::from_flags(flags))
    }

    /// An ASCII byte under a named character set.
    ///
    /// **The one place a wire byte becomes a character.** Every set that
    /// substitutes anything goes through here, so a path cannot honour DEC
    /// graphics and forget Finnish -- which is exactly the shape `promote`
    /// had before `from_wire` existed.
    pub fn with_charset(self, b: StreamByte, set: Charset) -> PackedCell {
        match set {
            Charset::DecGraphics => self.with_dec_graphics(b),
            Charset::Uk => self.with_nrcs(b, crate::uk_table()),
            Charset::National(n) => self.with_nrcs(b, crate::nrcs_table(n)),
            // ASCII, the DEC sets baud does not translate, and anything
            // unrecognised: the byte stands for itself.
            _ => self.with_ascii(b),
        }
    }

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

    /// The same, under a national replacement set.
    ///
    /// Each set replaces a handful of ASCII positions and leaves the rest
    /// alone, so an unlisted byte is itself.
    pub fn with_nrcs(self, b: StreamByte, table: &[(u8, u32)]) -> PackedCell {
        let v = b.raw_byte();
        let cp = match table.iter().find(|(k, _)| *k == v) {
            Some((_, cp)) => Codepoint::decoded(*cp),
            None => Codepoint::from_ascii(b),
        };
        PackedCell {
            content: Content::codepoint(cp),
            ..self
        }
    }

    /// The same, under DEC Special Graphics.
    ///
    /// **The flag stays set.** Translating and dropping `CellFlags::GRAPHICS`
    /// would lose where the character came from, and translating without
    /// storing the result would lose the character -- selection and copy would
    /// hand back `q` where the screen shows `─`. Both, and neither is lost.
    ///
    /// Bytes outside `` ` ``..`~` are unchanged: the set is ASCII everywhere
    /// else, which is why the table is 31 entries rather than 128.
    pub fn with_dec_graphics(self, b: StreamByte) -> PackedCell {
        let v = b.raw_byte();
        let cp = match v.checked_sub(0x60) {
            Some(i) if (i as usize) < crate::DEC_GRAPHICS.len() => {
                Codepoint::decoded(crate::DEC_GRAPHICS[i as usize])
            }
            _ => Codepoint::from_ascii(b),
        };
        PackedCell {
            content: Content::codepoint(cp),
            ..self
        }
    }
}

/// Combining marks attached to one cell, keyed on where that cell is.
///
/// **Not part of [`PackedCell`], deliberately.** The cell is eight measured
/// bytes with no room, so a grapheme that needs more than one codepoint keeps
/// its base in the cell and its marks here, found by position.
///
/// `len == 0` means the slot is free — a zeroed grid therefore has no marks and
/// there is nothing to initialise, which is what keeps `Storage` a plain
/// zeroable block for `new_box_zeroed`.
/// Which cell buffer a row lives in. Part of a cell's identity: the two
/// screens are separate buffers, so one row and column names a cell in each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    Primary,
    Alternate,
}

/// A row's position in the backing store, **after rotation**.
///
/// Distinct from [`Row`], which is a position on the screen. A scroll advances
/// the rotation, so a screen row names a different physical row afterwards --
/// and anything remembering something about a *cell* must remember the
/// physical one, or it describes whatever text later occupies that position.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PhysRow(pub(crate) usize);

impl PhysRow {
    pub fn of(n: usize) -> PhysRow {
        PhysRow(n)
    }
    pub fn index(self) -> usize {
        self.0
    }
}

/// How many times the row a mark slot belongs to has been blanked.
///
/// A physical row is not stable identity either: the ring recycles it, so the
/// same address later holds unrelated text. A generation makes invalidation
/// **O(1)** -- bumping it strands every slot that named the old contents, where
/// scanning the table for them would cost more than the blanking.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct MarkGen(pub(crate) u16);

impl MarkGen {
    pub const ZERO: MarkGen = MarkGen(0);
    /// Wrapping is intended: a slot 65,536 blankings stale is indistinguishable
    /// from a fresh one, and that row has been overwritten thousands of times.
    pub fn bump(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }
}

/// What identifies the cell a mark attaches to.
///
/// **Not `(Row, Col)`, and that was three bugs.** A screen row does not
/// identify a cell: the grid rotates under it, the ring recycles the row
/// beneath that, and the alternate screen is a different buffer. Marks keyed on
/// `(Row, Col)` outlived their cell across a scroll, and the primary screen's
/// marks were readable from the alternate one.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct MarkKey {
    /// Physical row, with the surface in the high bit.
    row: u16,
    col: u16,
    generation: u16,
}

impl MarkKey {
    const ALT: u16 = 0x8000;

    pub fn new(surface: Surface, row: PhysRow, col: Col, generation: MarkGen) -> MarkKey {
        debug_assert!(
            row.index() < MarkKey::ALT as usize,
            "physical row past the surface bit"
        );
        let alt = match surface {
            Surface::Alternate => MarkKey::ALT,
            Surface::Primary => 0,
        };
        MarkKey {
            row: row.index() as u16 | alt,
            col: col.index() as u16,
            generation: generation.0,
        }
    }

    /// Which slot of `n` this key hashes to.
    ///
    /// Takes the **high** bits of the product for the measured reason
    /// `StyleId::bucket` does: the low bits of a multiplicative hash are the
    /// ones that collide, and `& (n-1)` on the product is the bug that cost
    /// 112,144 interns.
    pub fn slot(self, n: usize) -> usize {
        let key = ((self.row as u64) << 32) ^ ((self.col as u64) << 16) ^ self.generation as u64;
        (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 55) as usize % n
    }
}

#[repr(C)]
/// **Not `Copy`, deliberately.** This is 44 bytes -- an eight-byte key, a
/// length and eight codepoints -- and it was `Copy` with by-value predicates,
/// so `marks[i].is_for(key)` copied the whole codepoint array to compare the
/// key at the front of it. A large aggregate that hands out silent copies is
/// a type lying about its cost, in the same way a `bool` field is a missing
/// two-state enum.
#[derive(Clone, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct MarkSlot {
    /// The cell these attach to. Only meaningful when `len > 0`.
    pub key: MarkKey,
    pub len: u16,
    pub cps: [u32; MarkSlot::CAPACITY],
}

impl MarkSlot {
    /// The most marks one cell may carry.
    ///
    /// **Eight, and three was too few for emoji.** The old value was justified
    /// as "a base plus two accents, past which no normal orthography stacks
    /// further", which holds for Latin, Devanagari and kana -- and does not
    /// hold for a ZWJ sequence, which is not orthography. A family emoji is
    /// seven codepoints attaching six marks to one cell, so three of them were
    /// dropped and the cell kept a glyph nobody sent: `a_zwj_family_keeps_all_
    /// of_its_codepoints` fails at the old value.
    ///
    /// Eight covers the sequences the recordings carry -- three, four and
    /// seven codepoints -- with headroom. It does **not** cover every RGI
    /// sequence: a four-person family with a skin tone on each member is
    /// eleven codepoints, so ten marks, and those still lose their tail. That
    /// is a bounded table doing what a bounded table does, and the bound is
    /// now stated rather than implied by a number.
    pub const CAPACITY: usize = 8;

    pub const EMPTY: MarkSlot = MarkSlot {
        key: MarkKey {
            row: 0,
            col: 0,
            generation: 0,
        },
        len: 0,
        cps: [0; MarkSlot::CAPACITY],
    };

    /// An empty slot keyed to a cell.
    pub fn for_cell(key: MarkKey) -> MarkSlot {
        MarkSlot {
            key,
            ..MarkSlot::EMPTY
        }
    }

    /// Does this slot belong to that cell?
    ///
    /// One comparison of a whole [`MarkKey`] rather than a field at a time --
    /// surface, physical row and generation all have to match. Comparing a
    /// subset is what the previous version did, and it reported a mark for a
    /// cell that had been scrolled away.
    pub fn is_for(&self, key: MarkKey) -> bool {
        !self.is_empty() && self.key == key
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len as usize >= MarkSlot::CAPACITY
    }

    /// Append a mark. `false` when the slot is full, which the caller counts.
    pub fn push(&mut self, cp: Codepoint) -> bool {
        if self.is_full() {
            return false;
        }
        self.cps[self.len as usize] = cp.value();
        self.len += 1;
        true
    }

    /// The marks, as codepoints.
    pub fn marks(&self) -> impl Iterator<Item = Codepoint> + '_ {
        self.cps[..(self.len as usize).min(MarkSlot::CAPACITY)]
            .iter()
            .filter_map(|c| Codepoint::new(*c))
    }
}
