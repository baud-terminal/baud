use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

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
    /// The word as the grid stores it. A shader reading the grid in place
    /// compares against these words, so a reserved state it tests for is
    /// spelled there as a literal and pinned to this.
    pub const fn bits(self) -> u32 {
        self.0
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
    /// **What the cursor advances by, parked in the flag field.** The cursor
    /// *position* is unbounded state and no table can enumerate it -- but the
    /// *delta* is one column or two, which is a bounded function of the bytes
    /// and so belongs in the entry beside the cell. A write replaces the flags
    /// with the terminal's own, so the slot is free until then.
    ///
    /// **Both, because they are different questions.** The advance is one
    /// column or two -- never zero, since without a cluster machine a
    /// zero-width character still takes a cell. The width is 0, 1 or 2 and is
    /// what a cluster machine will read. Dropping the width and keeping only
    /// the advance made `the_cell_table_is_what_the_write_path_writes` fail on `E0 A0 96`
    /// (U+0816 SAMARITAN MARK IN), which is the test saying the entry no longer
    /// carried enough.
    ///
    /// Low two bits the advance, next two the width. A write replaces the flags
    /// with the terminal's own, so the field is free until then.
    pub fn of_entry(w: Width) -> CellFlags {
        CellFlags((1 + (w.cells() == 2) as u16) | ((w.cells() as u16) << 2))
    }
    pub fn advance_of(self) -> usize {
        (self.0 & 0b11) as usize
    }
    pub fn width_of(self) -> Width {
        Width((self.0 >> 2) as u8)
    }
    pub fn is_wide(self) -> bool {
        self.0 & 0b11 == 2
    }

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
    /// The link takes exactly the bits `NLINK = 128` needs, and no more.
    ///
    /// **It used to take all thirteen**, which was harmless while nothing else
    /// wanted them and became a bug the moment something did: `link()` read
    /// `self.0 >> 3` unmasked, so any bit above the link's own range came back
    /// as part of the link index. Bounding it here is what makes the top six
    /// bits available to [`ClusterId`] rather than a hazard.
    const LINK_BITS: u16 = 7;
    const LINK_MASK: u16 = (1 << CellFlags::LINK_BITS) - 1;
    /// Where the cluster index lives: the six bits above the link.
    const CLUSTER_SHIFT: u16 = CellFlags::LINK_SHIFT + CellFlags::LINK_BITS;
    const CLUSTER_MASK: u16 = 0x3f;

    pub fn with_link(self, l: LinkId) -> CellFlags {
        let keep = self.0 & !(CellFlags::LINK_MASK << CellFlags::LINK_SHIFT);
        CellFlags(keep | ((l.index() as u16 & CellFlags::LINK_MASK) << CellFlags::LINK_SHIFT))
    }
    pub fn link(self) -> LinkId {
        LinkId::from_index(((self.0 >> CellFlags::LINK_SHIFT) & CellFlags::LINK_MASK) as usize)
    }

    /// **The cluster this cell's continuations live under, carried *in* the
    /// cell.**
    ///
    /// That placement is the whole design. `baud-core2` keys its marks on
    /// *where the cell is* -- surface, physical row, column, generation -- and
    /// an adversarial review reproduced what that costs: `Term::viewport`
    /// un-rotates the grid in place and does not move the table with it, so a
    /// renderer reading **one frame** loses a cell's marks and the next write
    /// to that address inherits a stranger's. An index that rides in the cell
    /// survives rotation, `viewport` and the alternate screen for free,
    /// because it moves when the cell moves.
    ///
    /// Same shape as [`StyleId`] into the rendition table, which this codebase
    /// already trusts: a bounded, content-addressed side table whose index is
    /// in the cell.
    pub fn with_cluster(self, c: ClusterId) -> CellFlags {
        let keep = self.0 & !(CellFlags::CLUSTER_MASK << CellFlags::CLUSTER_SHIFT);
        CellFlags(
            keep | ((c.index() as u16 & CellFlags::CLUSTER_MASK) << CellFlags::CLUSTER_SHIFT)
                | CellFlags::MARKED.0,
        )
    }
    pub fn cluster(self) -> ClusterId {
        ClusterId::from_index(
            ((self.0 >> CellFlags::CLUSTER_SHIFT) & CellFlags::CLUSTER_MASK) as usize,
        )
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
    /// Everything but the cluster index, for a write that replaces the cell's
    /// content: a fresh character has no continuations, and leaving the old
    /// index would hand it the previous occupant's.
    ///
    /// It is rarely needed, because every ordinary write builds a fresh
    /// `PackedCell` from the terminal's current flags, which never carry a
    /// cluster -- so **overwriting a cell un-records its continuations without
    /// the write path doing anything**, which is the same property
    /// `CellFlags::MARKED` was introduced for.
    pub fn without_cluster(self) -> CellFlags {
        CellFlags(
            self.0 & !((CellFlags::CLUSTER_MASK << CellFlags::CLUSTER_SHIFT) | CellFlags::MARKED.0),
        )
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
    /// The codepoint an ASCII byte stands for under a designated set.
    ///
    /// **Extracted so the two write paths translate through one rule.** The
    /// bulk path went through `with_charset` and the per-byte path did not
    /// translate at all, so a byte that reached `ground_uncommon` -- printable
    /// ASCII recovered after an ill-formed UTF-8 sequence, the case
    /// `[[source: unicode-ch3]]` § 3.9.5 names -- rendered untranslated. With
    /// DEC Special Graphics designated, `<C2 41>` put an `A` on the screen
    /// where every other route puts a line-drawing glyph.
    pub fn translate(b: StreamByte, set: Charset) -> Codepoint {
        // **ASCII is the answer, not a case of the general one.** Without this
        // the common path builds a throwaway `PackedCell`, runs the charset
        // map that has nothing to do, and then round-trips the content back
        // through `Codepoint::new`'s range-and-surrogate check -- on a byte the
        // caller has already proven printable ASCII -- after which the caller
        // builds a *second* cell from the result. Two cell constructions and a
        // validity check per byte, on the path `cat-cjk.raw` takes for 39% of
        // its characters.
        if matches!(set, Charset::Ascii) {
            return Codepoint::from_ascii(b);
        }
        let blank = PackedCell::glyph(Codepoint::from_ascii(b), StyleId::DEFAULT, CellFlags::NONE);
        match blank.with_charset(b, set).content.as_codepoint() {
            Some(cp) => cp,
            None => Codepoint::from_ascii(b),
        }
    }

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
    /// **A linear search, and it should be an index.** `nrcs_data.rs` is
    /// generated and its pair arrays are private, so spreading them into
    /// direct-indexed rows needs a generator change rather than a hand edit --
    /// `nrcs_tables_match_the_pinned_source` correctly rejected the hand edit.
    ///
    /// **The reason it was left alone has expired, and the new one is a
    /// prediction rather than an excuse.** That reason read "a charset is not on
    /// the ingest path: the new core never leaves `Charset::Ascii`", and it
    /// does now -- the G0-G3 slots and the locking shifts landed, so
    /// `ESC ( % 6` puts this search on the per-character path for as long as the
    /// set is designated. `every_national_replacement_renders_as_its_table_says`
    /// drives all 163 replacements through `Term` and proves it.
    ///
    /// What replaces it: the tables are **8 to 10 pairs** over a `const` array,
    /// and this project has measured a table against a short predictable chain
    /// twice and lost both times -- `SgrAttr::one` at flat to -3%, the
    /// single-parameter fold table at -7.5%. So the index is predicted to be
    /// flat at best, and **no recording designates a national set**, so there is
    /// nothing in the corpus to measure it on. Building it means writing a
    /// payload first and quoting the number against that payload only.
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

/// Which interned cluster a cell's continuations live in, or none.
///
/// Zero is "no continuations", so a zeroed grid has none and there is no
/// parallel validity array to keep in step -- the same trick [`LinkId`] and
/// `ColorWord` already use.
///
/// **Six bits, and the bound is a consequence rather than a choice.**
/// `PackedCell` is eight bytes because this file measured the ninth costing a
/// round to twelve; `CellFlags` is the `u16` that used to be padding; three
/// bits are flags and `NLINK = 128` needs seven. Six are left, so
/// [`ClusterStore`] holds **63** distinct clusters at once.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClusterId(u8);

impl ClusterId {
    pub const NONE: ClusterId = ClusterId(0);
    pub fn from_index(i: usize) -> ClusterId {
        ClusterId((i & 0x3f) as u8)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// One interned cluster: the codepoints that follow a cell's base character.
///
/// **The base is not here.** It is in the cell's `Content`, so storing it again
/// would be storing what the index already implies -- and comparing it on
/// lookup is what lets an evicted slot be *detected* rather than believed.
#[derive(Clone, Copy)]
pub struct ClusterSlot {
    /// The base character these continuations belong to.
    ///
    /// Checked on read against the cell's own content. A slot is only reused
    /// by a cluster with the same continuations, so this cannot go stale under
    /// the shipped policy -- it is here because "cannot" is a claim, and a
    /// cheap check that turns a wrong answer into an empty one is worth more
    /// than the claim.
    pub base: Content,
    len: u8,
    cps: [u32; ClusterSlot::CAPACITY],
}

impl ClusterSlot {
    /// A base plus this many continuations.
    ///
    /// **Eight, because a ZWJ family is seven codepoints.** `U+1F468 ZWJ
    /// U+1F469 ZWJ U+1F467 ZWJ U+1F466` is a base and six attachments, and a
    /// capacity of three -- justified once as "a base plus two accents, past
    /// which no normal orthography stacks further" -- dropped three of them.
    /// That reasoning was true of Latin, Devanagari and kana and false of
    /// emoji, which are not orthography.
    ///
    /// It still does not cover every RGI sequence: a four-person family with a
    /// skin tone on each member is eleven codepoints, and those lose their
    /// tail. That is a bounded table behaving as one, stated rather than
    /// implied by a number.
    pub const CAPACITY: usize = 8;

    pub const EMPTY: ClusterSlot = ClusterSlot {
        base: Content::EMPTY,
        len: 0,
        cps: [0; ClusterSlot::CAPACITY],
    };

    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    pub fn marks(&self) -> impl Iterator<Item = u32> + '_ {
        self.cps[..self.len as usize].iter().copied()
    }
    /// Whether this slot holds exactly these continuations under this base.
    fn is(&self, base: Content, cps: &[u32]) -> bool {
        self.len as usize == cps.len() && self.base == base && self.cps[..cps.len()] == *cps
    }
}

/// A bounded, content-addressed store of clusters, indexed from the cell.
///
/// **It refuses rather than evicts, and that is the whole difference from the
/// rendition table.** `StyleId` into `StyleWord` evicts on collision because a
/// wrong colour renders wrong and is rewritten next frame; a wrong *cluster*
/// puts somebody else's accent on your character and is unrecoverable by a
/// renderer, a selection or a copy. So a collision is refused and counted, the
/// way `OSC 52` clipboard reads are refused and counted: the answer is never
/// wrong, and how often it is missing is a number.
pub struct ClusterStore<'b> {
    slots: &'b mut [ClusterSlot],
    /// Clusters that did not fit. Never silent.
    pub refused: usize,
}

impl<'b> ClusterStore<'b> {
    pub fn new(slots: &'b mut [ClusterSlot]) -> ClusterStore<'b> {
        ClusterStore { slots, refused: 0 }
    }

    /// The continuations `id` names, if it still names them.
    pub fn get(&self, id: ClusterId, base: Content) -> Option<&ClusterSlot> {
        let s = self.slots.get(id.index().checked_sub(1)?)?;
        match !s.is_empty() && s.base == base {
            true => Some(s),
            false => None,
        }
    }

    /// Intern `base` plus `cps`, returning the id to write into the cell.
    ///
    /// **Content-addressed: the same cluster twice is one slot.** A page of
    /// repeated emoji costs one entry, which is the property position-keying
    /// cannot have -- there, every occurrence of the same family is its own
    /// slot and its own eviction.
    pub fn intern(&mut self, base: Content, cps: &[u32]) -> ClusterId {
        if cps.is_empty() || cps.len() > ClusterSlot::CAPACITY {
            self.refused += 1;
            return ClusterId::NONE;
        }
        let n = self.slots.len().min(63);
        if n == 0 {
            self.refused += 1;
            return ClusterId::NONE;
        }
        // Multiplicative hash on the **high** bits, which is the correction
        // this codebase already paid for once: `StyleId::bucket` masked the low
        // bits and collided every SGR colour, because they are multiples of
        // 0x101010.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= u64::from(base.0);
        h = h.wrapping_mul(0x1000_0000_01b3);
        for c in cps {
            h ^= u64::from(*c);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        let start = ((h >> 32) as usize) % n;
        // Two probes, not a walk: a bounded table with a bounded probe cannot
        // degrade into a scan under adversarial content.
        for k in 0..2 {
            let i = (start + k) % n;
            if self.slots[i].is(base, cps) {
                return ClusterId::from_index(i + 1);
            }
            if self.slots[i].is_empty() {
                let mut s = ClusterSlot::EMPTY;
                s.base = base;
                s.len = cps.len() as u8;
                s.cps[..cps.len()].copy_from_slice(cps);
                self.slots[i] = s;
                return ClusterId::from_index(i + 1);
            }
        }
        self.refused += 1;
        ClusterId::NONE
    }

    /// Forget everything. Every cell that named a slot is being blanked.
    pub fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            *s = ClusterSlot::EMPTY;
        }
    }
}
