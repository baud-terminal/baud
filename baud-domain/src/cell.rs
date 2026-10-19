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

    /// Where the cluster range starts: one below the word that names slot
    /// 1, above both reserved states.
    ///
    /// **A cell with continuations holds its cluster's index here, not its
    /// base**: the base is in the slot, with the marks ([`ClusterSlot`]).
    /// That is the id space the owner chose on 2026-10-13 ("1024 slots, 8
    /// probes"), measured flat on plain text because a cell with no
    /// continuations is written and read as it was: the range above
    /// U+10FFFF had two words in use and room for any slot count a `u16`
    /// names, where `CellFlags` had six bits left (docs/record.md, "The
    /// cluster store holds 1024 clusters and sweeps the dead ones").
    const CLUSTERS: u32 = 0x0012_0000;
    /// The word a cell holds when its continuations live in slot `id`.
    pub fn cluster(id: ClusterId) -> Content {
        Content(Content::CLUSTERS + u32::from(id.0))
    }
    /// The slot this cell names, if it names one: a word in the cluster
    /// range, which no codepoint and neither reserved state is.
    pub fn cluster_id(self) -> Option<ClusterId> {
        match self.0.wrapping_sub(Content::CLUSTERS) {
            0 => None,
            i => u16::try_from(i).ok().map(ClusterId),
        }
    }
    /// The word as the grid stores it. A shader reading the grid in place
    /// compares against these words, so a reserved state it tests for is
    /// spelled there as a literal and pinned to this.
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Where the placeholder range starts: above the cluster range, which
    /// a `u16` slot index bounds below `0x0013_0000`.
    ///
    /// **A U+10EEEE cell with its diacritics holds them here, not in a
    /// cluster slot**: three fields of nine bits, the row, the column and
    /// the image id's top byte, each a diacritic's number plus one, zero
    /// when it was not written (`placeholder.rs`). `kitten icat` writes
    /// three on every cell (`[[source: kitty-icat-transmit]]`), each cell a
    /// cluster of its own, and the 1024-slot store dropped 2366 of the 5760
    /// marks of an 80x24 picture; here no slot is used and the cell stays
    /// eight bytes. The cell carries [`CellFlags::MARKED`], as a cluster
    /// cell does: its content is not its base, and
    /// [`ClusterStore::base_of`] and [`ClusterStore::marks_of`] read the
    /// character back, so a copy or a selection sees what was written.
    const PLACEHOLDERS: u32 = 0x0100_0000;
    /// How many bits a field takes: 297 numbers and absent.
    const FIELD: u32 = 9;

    /// The word for U+10EEEE with `marks` after it, none of them absent
    /// before a written one.
    pub fn placeholder(marks: PlaceholderMarks) -> Content {
        let f = |d: Option<Diacritic>| d.map_or(0, |d| u32::try_from(d.index() + 1).unwrap_or(0));
        Content(
            Content::PLACEHOLDERS
                | (f(marks.row) << (2 * Content::FIELD))
                | (f(marks.col) << Content::FIELD)
                | f(marks.msb),
        )
    }

    /// The diacritics a placeholder word holds, if it is one.
    pub fn placeholder_marks(self) -> Option<PlaceholderMarks> {
        let v = self.0.checked_sub(Content::PLACEHOLDERS)?;
        if v >> (3 * Content::FIELD) != 0 {
            return None;
        }
        let field = |k: u32| {
            let n = (v >> (k * Content::FIELD)) & ((1 << Content::FIELD) - 1);
            usize::try_from(n)
                .ok()?
                .checked_sub(1)
                .and_then(Diacritic::at)
        };
        Some(PlaceholderMarks {
            row: field(2),
            col: field(1),
            msb: field(0),
        })
    }

    /// **What a placeholder becomes with one more mark**, when the mark is
    /// a diacritic and there is room for it: U+10EEEE itself, or a
    /// placeholder word with fewer than three. `None` sends the mark to the
    /// cluster store, the path any other base takes.
    pub fn with_diacritic(self, cp: Codepoint) -> Option<Content> {
        let marks = match self.0 {
            crate::placeholder::PLACEHOLDER => PlaceholderMarks::NONE,
            _ => self.placeholder_marks()?,
        };
        Diacritic::of(cp)
            .and_then(|d| marks.then(d))
            .map(Content::placeholder)
    }

    /// Whether this content may take a mark as a placeholder: U+10EEEE, or
    /// a placeholder word. One compare each, the gate the cold attach path
    /// asks before anything else.
    #[inline(always)]
    pub fn is_placeholder_base(self) -> bool {
        self.0 == crate::placeholder::PLACEHOLDER || self.0 >= Content::PLACEHOLDERS
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
        let mut h: u64 = FNV_OFFSET;
        for &b in id.iter().chain(uri) {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        let spread = h.wrapping_mul(FIBONACCI) >> 32;
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
    /// The character in this cell was written while the DEC Special Character
    /// and Line Drawing Set was invoked into GL.
    ///
    /// The cell holds the *translated* codepoint (`with_dec_graphics`) and this
    /// flag besides, so it keeps where the character came from: a copy of `─`
    /// is `─`, and the cell still says it was written under the line-drawing
    /// set. In this core `refresh_flags` sets it and nothing outside the tests
    /// tests the bit; `Charset::from_flags` would, through
    /// `PackedCell::from_wire`, which has no caller.
    ///
    /// This said the cell keeps the byte rather than a translated codepoint,
    /// because `MANIFEST.toml` recorded `dec-special-graphics` as `gated`.
    /// Neither holds. The table is translated, and the DEC pages are pinned:
    /// `[[source: vt100-ug-ch3]]` Table 3-9 names the set's 32 positions,
    /// `0x5F..0x7E`, in text. None of the DEC texts read gives a codepoint, so
    /// the 31 the table has are `[[source: foot-vt100-0]]`'s, and `0x5F`
    /// "Blank" is still `_`: an open departure, U+0020 or U+00A0 being the
    /// owner's call (docs/record.md, "DEC Special Graphics begins at 0x5F, and
    /// what it stores there is the owner's call").
    pub const GRAPHICS: CellFlags = CellFlags(2);
    pub fn is_graphics(self) -> bool {
        self.0 & CellFlags::GRAPHICS.0 != 0
    }
    pub fn with(self, o: CellFlags) -> CellFlags {
        CellFlags(self.0 | o.0)
    }

    /// Bit 2: this cell carries entries in the marks table -- its
    /// [`Content`] names a [`ClusterStore`] slot ([`Content::cluster_id`]),
    /// and the bit is set and cleared with it.
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

    /// Bits 3..10 carry the [`LinkId`] (`LINK_BITS`); bit 10 is
    /// [`CellFlags::GUARDED`] and 11..16 are free. The
    /// [`ClusterId`] took them until it moved into the cell's [`Content`],
    /// whose range above U+10FFFF names 1024 slots where six bits named 63.
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
    /// bits above it free rather than a hazard.
    const LINK_BITS: u16 = 7;
    const LINK_MASK: u16 = (1 << CellFlags::LINK_BITS) - 1;

    pub fn with_link(self, l: LinkId) -> CellFlags {
        let keep = self.0 & !(CellFlags::LINK_MASK << CellFlags::LINK_SHIFT);
        CellFlags(keep | ((l.index() as u16 & CellFlags::LINK_MASK) << CellFlags::LINK_SHIFT))
    }
    pub fn link(self) -> LinkId {
        LinkId::from_index(((self.0 >> CellFlags::LINK_SHIFT) & CellFlags::LINK_MASK) as usize)
    }

    /// Bit 10: the position is in a guarded area, `SPA` to `EPA`.
    ///
    /// `[[source: ecma48]]` 6.5.2.2: "The character position which is the
    /// active presentation position after receipt of SPA is the first
    /// character position of the guarded area. The character position which
    /// is the active presentation position before the receipt of EPA is the
    /// last character position of the guarded area." EPA sets it on every
    /// position of that string; ED, EL and ECH spare it while ERM is reset
    /// (7.2.4) and keep it while ERM is set, as DECSED and DECSEL keep it.
    ///
    /// **A bit apart from [`CellFlags::PROTECTED`]**: DECSCA's attribute is
    /// DEC's and decides DECSED and DECSEL alone, where the guard is
    /// ECMA-48's and decides the erasures ERM names. xterm and ghostty keep
    /// one bit and a mode naming whichever control set it last.
    ///
    /// **A write replaces it**: a host write builds a fresh cell from the
    /// terminal's flags, which never carry it, so the write path reads no
    /// old cell for it.
    pub const GUARDED: CellFlags = CellFlags(1 << 10);
    pub fn is_guarded(self) -> bool {
        self.0 & CellFlags::GUARDED.0 != 0
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
    /// Everything but [`CellFlags::MARKED`], for a cell built from a marked
    /// one that holds no cluster of its own -- a wide character's new tail.
    ///
    /// It is rarely needed, because every ordinary write builds a fresh
    /// `PackedCell` from the terminal's current flags, which never carry the
    /// mark -- so **overwriting a cell un-records its continuations without
    /// the write path doing anything**, which is the same property
    /// `CellFlags::MARKED` was introduced for.
    pub fn without_cluster(self) -> CellFlags {
        CellFlags(self.0 & !CellFlags::MARKED.0)
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
    /// Takes no flags on purpose (but for the guard an erasure under ERM set
    /// puts back, `View::fill_sparing`): an erase leaves a position with no character
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
    /// What DECSED, DECSEL and DECSERA leave at an erasable position:
    /// **U+0020 in the rendition the character had**, and an empty position
    /// as it is. `[[source: dec-std-070]]` DECSEL and DECSED (pp. 5-159,
    /// 5-162): "changed to the Space character (2/0). The character
    /// rendition and attributes associated with each position do not
    /// change. ... empty character positions, are not affected"; DECSERA
    /// (p. 5-172) the same.
    ///
    /// **The flags the position keeps**: DECSCA's attribute, ECMA-48's
    /// guard (Jev, 2026-10-16, 0.94/0.90; for DECSERA 1.00/0.98,
    /// 2026-10-19) and an `OSC 8` link (Jev's lean, KEEP, 2026-10-19).
    /// The line-drawing flag and a cluster's mark belong to the character
    /// and go with it, so the space draws as a space and its stale marks
    /// are unreachable ([`CellFlags::MARKED`]).
    pub fn selectively_erased(old: PackedCell) -> PackedCell {
        if old.content.is_empty() {
            return old;
        }
        let kept = old.flags.0 & !(CellFlags::GRAPHICS.0 | CellFlags::MARKED.0);
        PackedCell::glyph(Codepoint::SPACE, old.style, CellFlags(kept))
    }
    /// An ASCII byte under whatever character set `flags` says is invoked.
    ///
    /// Written (f297b21) for the core since retired as `baud-core2`, whose
    /// `promote` replayed stored runs with their flags. Nothing in this core
    /// calls it. The bulk path translates through `with_charset` in
    /// `View::put_ascii_run` and the per-byte path through `translate`, both
    /// with the set `Term` has invoked. The flags name only the line-drawing
    /// set (`Charset::from_flags`), so this would leave a national set's byte
    /// untranslated.
    pub fn from_wire(b: StreamByte, style: StyleId, flags: CellFlags) -> PackedCell {
        let proto = PackedCell {
            content: Content::EMPTY,
            style,
            flags,
        };
        proto.with_charset(b, Charset::from_flags(flags))
    }

    /// The codepoint an ASCII byte stands for under a designated set.
    ///
    /// **Extracted so the two write paths translate through one rule.** The
    /// bulk path went through `with_charset` and the per-byte path did not
    /// translate at all, so a byte that reached `ground_uncommon` -- printable
    /// ASCII recovered after an ill-formed UTF-8 sequence, the case
    /// `[[source: unicode-ch3]]` § 3.9.5 names -- rendered untranslated. With
    /// DEC Special Graphics designated, `<C2 71>` put a `q` on the screen where
    /// every other route puts `─`
    /// (`ascii_recovered_after_a_bad_sequence_is_still_translated`). This read
    /// `<C2 41>` and `A`, a byte outside the set, which is `A` on every route.
    ///
    /// **Inlined for ASCII, called for a set.** The ASCII answer is a compare
    /// and a return in the caller, as it was; a designated set goes through
    /// `translate_designated`, out of line. Built with `with_charset` forced
    /// inline once it gained the DEC sets' arm, this function went out of line
    /// whole, and every short run and every run's `REP` record called it,
    /// ASCII ones included (docs/record.md, "DEC Supplemental, JIS-Katakana and
    /// JIS-Roman are translated").
    #[inline(always)]
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
        Self::translate_designated(b, set)
    }

    /// [`PackedCell::translate`] under a set other than ASCII.
    #[inline(never)]
    fn translate_designated(b: StreamByte, set: Charset) -> Codepoint {
        let blank = PackedCell::glyph(Codepoint::from_ascii(b), StyleId::DEFAULT, CellFlags::NONE);
        match blank.with_charset(b, set).content.as_codepoint() {
            Some(cp) => cp,
            None => Codepoint::from_ascii(b),
        }
    }

    /// An ASCII byte under a named character set.
    ///
    /// **Every set that substitutes anything goes through here**, from
    /// `View::put_ascii_run` and from `translate`, so a path cannot honour DEC
    /// graphics and forget Finnish -- the shape `baud-core2`'s `promote` had
    /// before `from_wire` existed. This doc sat on `translate` from 01b11dc,
    /// which inserted `translate` between the two.
    ///
    /// **`#[inline(always)]`, into the two out-of-line callers on the
    /// designated path**, `put_designated_run` and `translate_designated`, so a
    /// designated byte is a match and a table read, not a call, as it was
    /// before the DEC sets' arm; the ASCII path reaches neither caller.
    #[inline(always)]
    pub fn with_charset(self, b: StreamByte, set: Charset) -> PackedCell {
        match set {
            Charset::DecGraphics => self.with_dec_graphics(b),
            Charset::Uk => self.with_nrcs(b, crate::uk_table()),
            Charset::National(n) => self.with_nrcs(b, crate::nrcs_table(n)),
            Charset::Dec(d) => match d.table() {
                Some(t) => self.with_dec_set(b, t),
                // DEC Technical, the three VT500 supplementals, Cyrillic and
                // the two undocumented sets: each draws its byte, for the
                // reason on its `DecSet` variant.
                None => self.with_ascii(b),
            },
            // ASCII and anything unrecognised: the byte stands for itself.
            Charset::Ascii | Charset::Other => self.with_ascii(b),
        }
    }

    /// The same, under a DEC 94-character set with a table: DEC Supplemental,
    /// JIS-Katakana, JIS-Roman.
    ///
    /// **One load from a 94-entry table**, indexed by `byte - 0x21`; the
    /// table holds every graphic position, a reserved one as DEC's error
    /// character, so nothing is searched and nothing falls back. SP, `0x20`,
    /// is below the index and stays a space: it is not one of the set's
    /// graphic characters (`[[source: dec-std-070]]` 070-3 §3.4.1).
    pub fn with_dec_set(self, b: StreamByte, table: &[Codepoint; 94]) -> PackedCell {
        let cp = match (b.raw_byte() as usize).checked_sub(0x21) {
            Some(i) if i < table.len() => table[i],
            _ => Codepoint::from_ascii(b),
        };
        PackedCell {
            content: Content::codepoint(cp),
            ..self
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
    /// drives the fourteen `Nrcs` sets' 164 replacements through `Term` and
    /// proves it. This said "all 163", counting UK's, which is a `Charset` of
    /// its own and which that test never drives.
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
    /// Bytes outside `` ` ``..`~` are unchanged, and at `0x5F` that is a
    /// departure. DEC's set replaces 32 positions, `0x5F..0x7E`, and leaves
    /// the rest ASCII (`[[source: vt100-ug-ch3]]`: "the graphics for ASCII
    /// codes 137 through 176 will be replaced", octal). The table holds
    /// `[[source: foot-vt100-0]]`'s 31, so `_` stays `_` where DEC draws
    /// "Blank". Whether Blank stores U+0020 or U+00A0 is the owner's call
    /// (docs/record.md, "DEC Special Graphics begins at 0x5F, and what it
    /// stores there is the owner's call"), and the base here moves to `0x5F`
    /// with the answer.
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

/// FNV-1a's 64-bit offset basis and prime, and the 64-bit Fibonacci
/// multiplier that finalizes a hash before its high bits pick a slot.
///
/// **Named once, because a copy of the prime was mistyped.** The cluster
/// store spelled it `0x1000_0000_01b3`, 2^44 + 0x1b3, where the prime is
/// 2^40 + 0x1b3, and took its slot from the unfinalized hash: below bit 44
/// its state was only ever multiplied by 0x1b3, so at a power-of-two size a
/// change in the last code point never reached the bits that chose the slot.
/// At 1024 slots that store refused 142 of Genesis 1:1-5's 146 points; this
/// construction, [`LinkId::bucket`]'s, refuses none (docs/record.md, "The
/// cluster store holds 1024 clusters and sweeps the dead ones").
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
const FIBONACCI: u64 = 0x9E37_79B9_7F4A_7C15;

/// Which interned cluster a cell's continuations live in, or none.
///
/// Zero is "no continuations", so a zeroed grid has none and there is no
/// parallel validity array to keep in step -- the same trick [`LinkId`] and
/// `ColorWord` already use.
///
/// **It rides in the cell's [`Content`]**, in the range above U+10FFFF
/// ([`Content::cluster`]), so it moves when the cell moves: rotation, a
/// frame's read, the alternate screen and a reflow carry it for free.
/// `baud-core2` keyed its marks on *where the cell is* -- surface, physical
/// row, column, generation -- and an adversarial review reproduced what that
/// costs: its `Term::viewport` un-rotated the grid in place and did not move
/// the table with it, so a renderer reading one frame lost a cell's marks
/// and the next write to that address inherited a stranger's. Six bits of
/// `CellFlags` carried it before, which named 63 slots.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClusterId(u16);

impl ClusterId {
    pub const NONE: ClusterId = ClusterId(0);
    /// The id of slot `i - 1`. An index past what a `u16` names is none.
    pub fn from_index(i: usize) -> ClusterId {
        ClusterId(u16::try_from(i).unwrap_or(0))
    }
    pub fn index(self) -> usize {
        usize::from(self.0)
    }
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// One interned cluster: a base character and the codepoints that follow it.
#[derive(Clone, Copy)]
pub struct ClusterSlot {
    /// The base character these continuations belong to, and the only copy
    /// of it: a cell that names this slot holds the slot's index in its
    /// [`Content`], not its base. [`ClusterStore::base_of`] reads it back.
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
    /// implied by a number, and a departure: the Unicode Standard's grapheme
    /// cluster is a base "together with any number of nonspacing marks
    /// applied to it" (`[[source: unicode18-ch3]]`, D60). The owner left the cap
    /// open on 2026-10-13 when deciding the slot count and the sweep, so it
    /// stays eight and `docs/vt-coverage.md` states it.
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

/// How many times a store's slots may have been handed to other clusters:
/// once a sweep that freed any, and once a reset.
///
/// **What a reader holding cells across reads compares.** A cell's word is
/// the slot's index, so a slot a sweep freed and the next cluster took
/// gives a cell the same word for a different cluster -- and a renderer
/// that repaints only the rows whose words changed draws the old one. The
/// rendition table's eviction is the same hazard for colours and forces a
/// full pass the same way (`baud-render`'s `Evicted`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClusterEpoch(u32);

/// What a store's sweeps have done: how many ran, and how many slots they
/// freed, summed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClusterSweeps {
    pub sweeps: usize,
    pub freed: usize,
}

/// The marks after a cell's base ([`ClusterStore::marks_of`]): a slot's
/// continuations, or a placeholder word's diacritics.
pub enum Marks<'a> {
    Slot(core::slice::Iter<'a, u32>),
    Placeholder(PlaceholderMarks, usize),
}

impl Iterator for Marks<'_> {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        match self {
            Marks::Slot(i) => i.next().copied(),
            Marks::Placeholder(m, k) => {
                let c = m.get(*k)?;
                *k += 1;
                Some(c.value())
            }
        }
    }
}

/// Why an intern found no slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// No continuations, more than [`ClusterSlot::CAPACITY`], or a store
    /// with no slots: nothing a sweep can change.
    Shape,
    /// All [`ClusterStore::PROBES`] of the cluster's slots hold others. A
    /// sweep can free one.
    Full,
}

/// Which slots some cell names, for a sweep: one bit a slot.
pub struct ClusterLive {
    bits: [u64; ClusterStore::SLOTS_MAX / 64],
}

impl ClusterLive {
    pub fn new() -> ClusterLive {
        ClusterLive {
            bits: [0; ClusterStore::SLOTS_MAX / 64],
        }
    }
    /// Every slot a cell of `cells` names.
    ///
    /// **One compare a cell, and branch-free**: the test is whether the word
    /// is in the cluster range, which a cell with no continuations -- nearly
    /// all of them -- answers with the subtract and compare already in
    /// [`Content::cluster_id`].
    pub fn mark(&mut self, cells: &[PackedCell]) {
        for c in cells {
            if let Some(id) = c.content.cluster_id() {
                if let Some(i) = id.index().checked_sub(1) {
                    if let Some(w) = self.bits.get_mut(i / 64) {
                        *w |= 1 << (i % 64);
                    }
                }
            }
        }
    }
    fn has(&self, i: usize) -> bool {
        self.bits
            .get(i / 64)
            .is_some_and(|w| w & (1 << (i % 64)) != 0)
    }
}

impl Default for ClusterLive {
    fn default() -> Self {
        ClusterLive::new()
    }
}

/// A bounded, content-addressed store of clusters, indexed from the cell.
///
/// **It refuses rather than evicts, and that is the whole difference from the
/// rendition table.** `StyleId` into `StyleWord` evicts once full because a
/// wrong colour renders wrong and is rewritten next frame; a wrong *cluster*
/// puts somebody else's accent on your character and is unrecoverable by a
/// renderer, a selection or a copy. So a collision is refused and counted, the
/// way `OSC 52` clipboard reads are refused and counted: the answer is never
/// wrong, and how often it is missing is a number.
///
/// **And it reclaims what no cell names, once every slot is filled.** When
/// a cluster finds its slots taken and every slot of the store holds a
/// cluster, the caller may [`ClusterStore::sweep`] the cells that can name
/// one -- both screens, and the history behind the primary -- and every
/// slot none of them names is freed. The owner's choice, 2026-10-13: "Sweep
/// when full": "when all slots are used, walk the live cells once and free
/// unreferenced slots. No cost until the table fills."
///
/// **That bounds what is refused only once the store has filled.** Before
/// it, a cluster whose eight slots are taken is refused while other slots
/// are free, as it was with no sweep; after a sweep, what is left is what
/// cells name, and a cluster is refused when its eight slots hold those.
/// Neither is a bound on what is on a cell at once: with 300 one-mark
/// clusters on cells, 2,905 of 4,000 more were refused (docs/record.md,
/// "The cluster store holds 1024 clusters and sweeps the dead ones").
pub struct ClusterStore<'b> {
    slots: &'b mut [ClusterSlot],
    /// Clusters that did not fit. Never silent.
    pub refused: usize,
    /// Interns that wanted a slot -- a fill or a refusal for want of one --
    /// since the last sweep, which [`ClusterStore::sweep_due`] reads.
    since: u32,
    /// Slots holding a cluster, kept as they fill and empty, so whether
    /// every slot is used is one compare and not a walk of the slots.
    filled: usize,
    epoch: ClusterEpoch,
    swept: ClusterSweeps,
}

impl<'b> ClusterStore<'b> {
    /// **Eight probes, not a walk**: a bounded table with a bounded probe
    /// cannot degrade into a scan under content chosen to collide, and an
    /// intern reads at most eight 40-byte slots.
    ///
    /// It was two. The owner chose eight on 2026-10-13 ("1024 slots, 8
    /// probes"): at 1024 slots and the hash above, with no sweep, eight
    /// refused 0, 0, 2, 0, 0, 349 and 0 code points on Genesis 1:1-5, Pango's
    /// Genesis 1, OSHB Genesis, Bialik, an Arabic and a Hindi text and
    /// `cat-cjk`, each through a terminal of its own, where four refused 0,
    /// 2, 22, 0, 2, 555 and 0. The sweep, which runs only once every slot
    /// is used, runs on none of them, so they refuse the same with it
    /// (docs/record.md, "The cluster store holds 1024 clusters and sweeps
    /// the dead ones").
    pub const PROBES: usize = 8;
    /// The most slots a store uses, and the size of the sweep's live set:
    /// a larger slice is used to this length.
    pub const SLOTS_MAX: usize = 1024;

    /// A store over `slots`, as they are: a `Term` made again over storage
    /// that held one keeps its clusters, so the slots already filled are
    /// counted here, once.
    pub fn new(slots: &'b mut [ClusterSlot]) -> ClusterStore<'b> {
        let n = slots.len().min(ClusterStore::SLOTS_MAX);
        let filled = slots[..n].iter().filter(|s| !s.is_empty()).count();
        ClusterStore {
            slots,
            refused: 0,
            since: ClusterStore::DUE,
            filled,
            epoch: ClusterEpoch::default(),
            swept: ClusterSweeps::default(),
        }
    }

    /// **A sweep at most once every `SLOTS_MAX / 16` interns that wanted a
    /// slot**, so a store whose every slot is named by a live cell -- where a
    /// sweep frees nothing and the store stays full -- does not walk the grid
    /// on every mark. The colour table's scratch prototype bounds its sweep
    /// at the same rate, a sixteenth of its table, counted in rendition
    /// writes (`scratch-rendition-r2`, `SweepBook::due`). Counted here in
    /// interns, never in reads or time, so where the reads fall cannot move
    /// it.
    const DUE: u32 = (ClusterStore::SLOTS_MAX / 16) as u32;

    fn len(&self) -> usize {
        self.slots.len().min(ClusterStore::SLOTS_MAX)
    }

    /// The slot `content` names, if it names one that holds a cluster.
    pub fn get(&self, content: Content) -> Option<&ClusterSlot> {
        let i = content.cluster_id()?.index().checked_sub(1)?;
        let s = self.slots.get(i)?;
        match s.is_empty() {
            true => None,
            false => Some(s),
        }
    }

    /// The character a cell shows as its base: the slot's for a cell that
    /// names one, else the cell's own content.
    ///
    /// A placeholder word ([`Content::placeholder`]) reads as U+10EEEE.
    /// A codepoint or a reserved state, below the cluster range, is its own
    /// base, answered by one compare before the slot and the placeholder
    /// are asked: a renderer asks this of every cell it draws.
    pub fn base_of(&self, content: Content) -> Content {
        if content.0 < Content::CLUSTERS {
            return content;
        }
        match self.get(content) {
            Some(s) => s.base,
            None => match content.placeholder_marks() {
                Some(_) => Content(crate::placeholder::PLACEHOLDER),
                None => content,
            },
        }
    }

    /// The codepoints attached after `content`'s base: its slot's, or a
    /// placeholder word's diacritics, or none.
    pub fn marks_of(&self, content: Content) -> Marks<'_> {
        match (self.get(content), content.placeholder_marks()) {
            (Some(s), _) => Marks::Slot(s.cps[..s.len as usize].iter()),
            (None, Some(m)) => Marks::Placeholder(m, 0),
            (None, None) => Marks::Slot([].iter()),
        }
    }

    /// Intern `base` plus `cps`, returning the id to write into the cell, or
    /// why there is none. Counts nothing in [`ClusterStore::refused`]: the
    /// caller decides whether a refusal is final, after a sweep or not.
    ///
    /// **Content-addressed: the same cluster twice is one slot.** A page of
    /// repeated emoji costs one entry, which is the property position-keying
    /// cannot have -- there, every occurrence of the same family is its own
    /// slot and its own eviction.
    ///
    /// **Every probe is read before a free slot is taken.** A sweep frees
    /// slots in the middle of a run of filled ones, so a cluster can sit
    /// past a free slot in its own window; taking the first free slot would
    /// intern it twice. The probe length is fixed, so no tombstone is
    /// needed for a lookup to find what lies past a hole.
    ///
    /// **`#[inline]`, into `attach_before`, which is out of line already.**
    /// Called across the crate boundary it was not inlined, and the call
    /// cost `cat-cjk` 2.15% more instructions and the two pointed Hebrew
    /// texts 7.41% and 12.33% against the store it replaced; inlined, as
    /// built, 0.29%, 3.14% and 6.84%, against the costs the owner chose
    /// from ("1024 slots, 8 probes": +0.39%, +3.38% and +6.97% on the
    /// prototype).
    #[inline]
    pub fn try_intern(&mut self, base: Content, cps: &[u32]) -> Result<ClusterId, Refused> {
        let n = self.len();
        if cps.is_empty() || cps.len() > ClusterSlot::CAPACITY || n == 0 {
            return Err(Refused::Shape);
        }
        // FNV-1a over the base and the continuations, then finalized, and the
        // slot taken from the **high** bits: [`LinkId::bucket`]'s construction.
        // The high bits are the correction this codebase already paid for
        // once: `StyleId::bucket` masked the low bits and collided every SGR
        // colour, because they are multiples of 0x101010. The finalizer is the
        // second one: without it the high bits of an FNV state carry little of
        // its last input (`FNV_PRIME`).
        let mut h: u64 = FNV_OFFSET;
        h ^= u64::from(base.0);
        h = h.wrapping_mul(FNV_PRIME);
        for c in cps {
            h ^= u64::from(*c);
            h = h.wrapping_mul(FNV_PRIME);
        }
        let start = ((h.wrapping_mul(FIBONACCI) >> 32) as usize) % n;
        let mut free = None;
        for k in 0..ClusterStore::PROBES.min(n) {
            let i = (start + k) % n;
            let s = &self.slots[i];
            if s.is(base, cps) {
                return Ok(ClusterId::from_index(i + 1));
            }
            if free.is_none() && s.is_empty() {
                free = Some(i);
            }
        }
        self.since = self.since.saturating_add(1);
        let Some(i) = free else {
            return Err(Refused::Full);
        };
        let mut s = ClusterSlot::EMPTY;
        s.base = base;
        s.len = cps.len() as u8;
        s.cps[..cps.len()].copy_from_slice(cps);
        self.slots[i] = s;
        self.filled += 1;
        Ok(ClusterId::from_index(i + 1))
    }

    /// [`ClusterStore::try_intern`] out of line, for a caller off the
    /// path a mark usually takes: the placeholder fallback
    /// (`Paint::attach_placeholder`).
    #[inline(never)]
    pub fn try_intern_cold(&mut self, base: Content, cps: &[u32]) -> Result<ClusterId, Refused> {
        self.try_intern(base, cps)
    }

    /// [`ClusterStore::try_intern`], with a refusal counted.
    pub fn intern(&mut self, base: Content, cps: &[u32]) -> ClusterId {
        match self.try_intern(base, cps) {
            Ok(id) => id,
            Err(_) => {
                self.refused += 1;
                ClusterId::NONE
            }
        }
    }

    /// Whether a refusal for want of a slot should sweep: **only when every
    /// slot holds a cluster**, the owner's "when all slots are used", and
    /// then only once [`ClusterStore::DUE`] interns, fills or refusals, have
    /// wanted a slot since the last sweep. A store never swept, or reset,
    /// sweeps at its first refusal once full; a store a sweep left with
    /// fewer than `DUE` slots free waits `DUE` interns from that sweep,
    /// whatever its cells name by then.
    pub fn sweep_due(&self) -> bool {
        self.filled >= self.len() && self.since >= ClusterStore::DUE
    }

    /// How many slots hold a cluster.
    pub fn filled(&self) -> usize {
        self.filled
    }

    /// **Free every slot no cell in `live` names**, and say how many.
    ///
    /// `live` must have marked every cell that can name a slot, or a cell
    /// loses its base and its marks: a sweep is exact only over what it was
    /// shown. A slot it frees is given to the next cluster that hashes near
    /// it, so the epoch moves whenever one is freed.
    pub fn sweep(&mut self, live: &ClusterLive) -> usize {
        let mut freed = 0;
        let n = self.len();
        for (i, s) in self.slots[..n].iter_mut().enumerate() {
            if !s.is_empty() && !live.has(i) {
                *s = ClusterSlot::EMPTY;
                freed += 1;
            }
        }
        self.since = 0;
        self.filled -= freed;
        self.swept.sweeps += 1;
        self.swept.freed += freed;
        if freed > 0 {
            self.epoch = ClusterEpoch(self.epoch.0.wrapping_add(1));
        }
        freed
    }

    /// See [`ClusterEpoch`].
    pub fn epoch(&self) -> ClusterEpoch {
        self.epoch
    }

    /// See [`ClusterSweeps`].
    pub fn sweeps(&self) -> ClusterSweeps {
        self.swept
    }

    /// Forget everything. Every cell that named a slot is being blanked.
    pub fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            *s = ClusterSlot::EMPTY;
        }
        self.since = ClusterStore::DUE;
        self.filled = 0;
        self.epoch = ClusterEpoch(self.epoch.0.wrapping_add(1));
    }
}
