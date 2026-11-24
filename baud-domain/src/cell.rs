use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// What a cell holds. **Content, not appearance.**
///
/// Values above `U+10FFFF` cannot be codepoints, so the reserved states are
/// unambiguous without a discriminant byte. The core never decides that "blank"
/// looks like `U+0020` — that conflates *nothing was written* with *a space was
/// written*, which differ for selection (copying a line must not yield trailing
/// spaces), for reflow, and for equality.
///
/// A cell word is a scalar value, EMPTY, WIDE_TAIL, DEFECTIVE, a cluster id or
/// a placeholder, the reserved states above U+10FFFF so blank is never U+0020:
/// the field is pub(crate) and the word is built only through
/// codepoint(Codepoint), cluster, placeholder and the constants.
///
/// ```compile_fail,E0603
/// let _ = baud_domain::Content(0x20);
/// ```
///
/// The legal path:
///
/// ```
/// let _ = baud_domain::Content::codepoint(baud_domain::Codepoint::SPACE);
/// ```
///
/// Bytes are not a cell word: the type does not implement `FromBytes`, so
/// a word past the reserved states cannot be read in.
///
/// ```compile_fail,E0599
/// use zerocopy::FromBytes;
/// let _ = baud_domain::Content::read_from_bytes(&[0xff; 4]);
/// ```
///
/// The legal path:
///
/// ```
/// let c = baud_domain::Codepoint::new(u32::from_le_bytes([0x41, 0, 0, 0]))
///     .map(baud_domain::Content::codepoint);
/// assert!(c.is_some());
/// ```
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, IntoBytes, Immutable, KnownLayout)]
pub struct Content(pub(crate) u32);

impl Content {
    /// Nothing was written here. The renderer draws background and no glyph.
    pub const EMPTY: Content = Content(0x0011_0000);
    /// The right half of a double-width character. The renderer skips it; the
    /// glyph in the preceding cell covers both columns.
    pub const WIDE_TAIL: Content = Content(0x0011_0001);
    /// **The base of a defective combining character sequence**: a
    /// combining mark that follows a control, which `[[source: uax29]]`
    /// GB4 and GB5 make a cluster of its own with no base. Never a cell's
    /// word: only a [`ClusterSlot`]'s base, the mark its first mark, so
    /// the mark is stored alone, as written (TUS D52 lets it be presented
    /// "as if it were a base character"), and a copy returns the mark and
    /// nothing else. What draws it is a no-break space, TUS 5.13's
    /// "rendered as if they had a no-break space as a base character"
    /// ([`Content::drawn`]).
    pub const DEFECTIVE: Content = Content(0x0011_0002);

    /// A decoded codepoint, over the full range, so an astral-plane
    /// character keeps every bit: `U+1F680` cut to 16 bits would be
    /// `U+F680`, a private-use codepoint.
    pub fn codepoint(c: Codepoint) -> Content {
        Content(c.value())
    }
    pub fn as_codepoint(self) -> Option<Codepoint> {
        Codepoint::new(self.0)
    }
    /// The base a renderer draws for this one: U+00A0 for
    /// [`Content::DEFECTIVE`] (TUS 5.13), and every other content as it
    /// is.
    pub fn drawn(self) -> Content {
        match self == Content::DEFECTIVE {
            true => Content(0x00A0),
            false => self,
        }
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
    /// The range above U+10FFFF has room for any slot index a `u16` names,
    /// and a cell with no continuations is written and read as its
    /// codepoint, with nothing added.
    const CLUSTERS: u32 = 0x0012_0000;
    /// The word a cell holds when its continuations live in slot `id`.
    pub fn cluster(id: ClusterId) -> Content {
        Content(Content::cluster_word(id))
    }
    /// [`Content::cluster`]'s word, for a constant: the sequence table's
    /// first word is checked against it at compile time (`sequence.rs`).
    pub const fn cluster_word(id: ClusterId) -> u32 {
        Content::CLUSTERS + id.0 as u32
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
    /// cluster slot**: three fields of nine bits, the row at bits 18-26,
    /// the column at 9-17 and the image id's top byte at 0-8, each a
    /// diacritic's number plus one, zero when it was not written
    /// (`placeholder.rs`). The fields are **added** to this base, and
    /// [`Content::placeholder_marks`] subtracts it: the row field's bit 6
    /// is the base's own bit, so the word is a sum, never an OR. The range
    /// is `0x0100_0000..=0x08FF_FFFF`, the base plus every 27-bit sum:
    /// above every codepoint, the reserved states and the cluster range,
    /// and below `0x1000_0000`, so a word's top four bits are always zero.
    /// A cell holding one uses no slot and stays eight bytes. It carries
    /// [`CellFlags::MARKED`], as a cluster cell does: its content is not
    /// its base, and [`ClusterStore::base_of`] and
    /// [`ClusterStore::marks_of`] read the character back, so a copy or a
    /// selection sees what was written.
    const PLACEHOLDERS: u32 = 0x0100_0000;
    /// How many bits a field takes: 297 numbers and absent.
    const FIELD: u32 = 9;

    /// The word for U+10EEEE with `marks` after it, none of them absent
    /// before a written one.
    pub fn placeholder(marks: PlaceholderMarks) -> Content {
        let f = |d: Option<Diacritic>| d.map_or(0, |d| u32::try_from(d.index() + 1).unwrap_or(0));
        Content(
            Content::PLACEHOLDERS
                + (f(marks.row) << (2 * Content::FIELD))
                + (f(marks.col) << Content::FIELD)
                + f(marks.msb),
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
    /// a word in the placeholder range, which ends where a multicell
    /// block's words begin. One compare each, the gate the cold attach path
    /// asks before anything else.
    #[inline(always)]
    pub fn is_placeholder_base(self) -> bool {
        self.0 == crate::placeholder::PLACEHOLDER
            || self.0.wrapping_sub(Content::PLACEHOLDERS) < 1 << (3 * Content::FIELD)
    }
}

impl Default for Content {
    fn default() -> Self {
        Content::EMPTY
    }
}

/// One materialised cell: content plus a style index.
///
/// Eight bytes: four for the content, so a full codepoint fits, two for the
/// style index and two for the flags.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, IntoBytes, Immutable, KnownLayout)]
pub struct PackedCell {
    pub content: Content,
    pub style: StyleId,
    pub flags: CellFlags,
}

/// Per-cell state that is **not** a rendition.
///
/// Lives in the cell's last two bytes, so the cell stays eight bytes.
///
/// The distinction is the point. A rendition is interned: many cells share one
/// [`StyleId`], and a table with no slot to give refuses a new rendition, so
/// the text after it takes the default. That is tolerable for colour and not
/// for DECSCA protection, where a refusal would silently expose text an
/// application asked to keep. Anything whose loss is a correctness bug rather
/// than a cosmetic one belongs here, per cell, where no table can refuse it.
///
/// Per-cell state (DECSCA protection, graphics, link index, entry advance) is
/// built only through named constants and constructors outside the crate, so no
/// raw u16 can set a stray link index or protection bit.
///
/// ```compile_fail,E0603
/// let _ = baud_domain::CellFlags(1);
/// ```
///
/// The legal path:
///
/// ```
/// let f = baud_domain::CellFlags::protection(true);
/// assert!(f.is_protected());
/// ```
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
    /// same reason: the low bits of a multiplicative hash are the ones that
    /// collide.
    ///
    /// Slot 0 is reserved for "no link", so the range is `1..buckets`.
    ///
    /// The `id` is hashed alongside the URI because the spec lets two runs with
    /// the same `id` be one logical link across lines; distinct ids must not
    /// collapse into a single slot merely because their URIs match.
    pub fn bucket(id: &[u8], uri: &[u8], buckets: usize) -> LinkId {
        LinkKey::of(id, uri).home(buckets)
    }
}

/// **What a link table slot is looked up by**: the FNV-1a state over the
/// `id` and the URI, and the slot a probe starts from.
///
/// The URI alone is the destination; the key is what keeps two `id`s apart
/// that share one. `[[source: osc8-spec]]`: "Character cells that have the
/// same target URI and the same nonempty `id` are always underlined
/// together on mouseover", and two runs with different ids are two links.
/// A slot matches only on both its key and its bytes, so a key collision
/// can at worst join two runs of the *same* URI, never give a cell another
/// destination.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkKey(u64);

impl LinkKey {
    pub fn of(id: &[u8], uri: &[u8]) -> LinkKey {
        let mut h: u64 = FNV_OFFSET;
        for &b in id.iter().chain(uri) {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        LinkKey(h)
    }
    /// The slot a probe for this key starts at, in `1..buckets`: the
    /// **high** bits of the finalized hash, as [`LinkId::bucket`] says.
    pub fn home(self, buckets: usize) -> LinkId {
        let spread = self.0.wrapping_mul(FIBONACCI) >> 32;
        LinkId::from_index(1 + (spread as usize) % (buckets - 1))
    }
    /// The word a slot keeps to compare against.
    pub fn word(self) -> u32 {
        (self.0 ^ (self.0 >> 32)) as u32
    }
}

/// Moves whenever a slot of the `OSC 8` link table may have been given to
/// another target: a sweep that freed one, or a reset.
///
/// **What a reader holding a [`LinkId`] across reads compares.** A sweep
/// frees only slots no cell names, so every cell keeps its target; but an
/// index copied out of a cell -- a press waiting for its release, a held
/// preview -- names whatever took the slot after it was freed. A reader
/// that resolves a held index only while the epoch is the one it was read
/// under can never open another link's target.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkEpoch(u32);

impl LinkEpoch {
    /// A table's first epoch.
    pub const ZERO: LinkEpoch = LinkEpoch(0);
    /// The epoch after this one.
    pub fn next(self) -> LinkEpoch {
        LinkEpoch(self.0.wrapping_add(1))
    }
}

/// What the link table's sweeps have done: how many ran, how many slots
/// they freed, and how many cells they read, row marks counted as cells,
/// each summed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkSweeps {
    pub sweeps: usize,
    pub freed: usize,
    pub read: usize,
}

/// Which link slots something names, for a sweep: one bit a slot.
pub struct LinkLive {
    bits: [u64; LinkLive::SLOTS / 64],
}

impl LinkLive {
    /// Every index a cell's link bits can spell, `NONE`'s included.
    pub const SLOTS: usize = CellFlags::LINK_MASK as usize + 1;
    pub fn new() -> LinkLive {
        LinkLive {
            bits: [0; LinkLive::SLOTS / 64],
        }
    }
    /// Every slot a cell of `cells` names; whether any does.
    ///
    /// **Sixteen cells at a time, and a block naming none is skipped**:
    /// nearly every cell names no link, and the one test a block -- the
    /// link bits of sixteen flag words OR-ed -- is what keeps a sweep from
    /// paying a dependent read-modify-write of `bits` a cell.
    pub fn mark(&mut self, cells: &[PackedCell]) -> bool {
        const LINKS: u16 = CellFlags::LINK_MASK << CellFlags::LINK_SHIFT;
        let mut any = false;
        for block in cells.chunks(16) {
            if block.iter().fold(0, |a, c| a | c.flags.0) & LINKS == 0 {
                continue;
            }
            for c in block {
                let l = c.flags.link();
                if !l.is_none() {
                    self.name(l);
                    any = true;
                }
            }
        }
        any
    }
    /// The slot `l` names, as live; `LinkId::NONE` names none.
    pub fn name(&mut self, l: LinkId) {
        let i = l.index() & CellFlags::LINK_MASK as usize;
        self.bits[i / 64] |= 1 << (i % 64);
    }
    /// Whether anything marked names slot `l`.
    pub fn has(&self, l: LinkId) -> bool {
        let i = l.index() & CellFlags::LINK_MASK as usize;
        self.bits[i / 64] & (1 << (i % 64)) != 0
    }
}

impl Default for LinkLive {
    fn default() -> Self {
        LinkLive::new()
    }
}

/// **A slot a cell names**: `1..LinkLive::SLOTS`, never `LinkId::NONE`'s.
/// Made only by [`LinkId::slot`]; the field is private.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::LinkSlot(core::num::NonZeroU8::MIN);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::LinkId;
/// let s = LinkId::from_index(5).slot().expect("a slot");
/// assert_eq!(s.id(), LinkId::from_index(5));
/// assert!(LinkId::NONE.slot().is_none());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct LinkSlot(core::num::NonZeroU8);

impl LinkId {
    /// **The slot this names**: none for `NONE`, and none for an index no
    /// cell's link bits can spell.
    pub fn slot(self) -> Option<LinkSlot> {
        match self.index() < LinkLive::SLOTS {
            true => core::num::NonZeroU8::new(self.0 as u8).map(LinkSlot),
            false => None,
        }
    }
}

impl LinkSlot {
    /// The link the slot is, as a cell names it.
    pub fn id(self) -> LinkId {
        LinkId(u16::from(self.0.get()))
    }

    fn bit(self) -> u128 {
        1 << self.0.get()
    }

    fn index(self) -> usize {
        usize::from(self.0.get())
    }
}

/// **Which link slots a run of cells names**: one bit a slot, the width of
/// every index a cell's link bits can spell, and bit 0 -- `NONE`'s -- never
/// set. A value a row of a view holds, never a field of a cell. Made from
/// cells, or from slots, never from bits; the field is private.
///
/// ```compile_fail,E0308
/// let _ = baud_domain::LinkSet::EMPTY.with(baud_domain::LinkId::NONE);
/// ```
///
/// ```compile_fail,E0423
/// let _ = baud_domain::LinkSet(1);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{LinkId, LinkSet};
/// let s = LinkId::from_index(3).slot().expect("a slot");
/// let set = LinkSet::EMPTY.with(s);
/// assert!(set.has(s) && set.meets(LinkSet::EMPTY.with(s)));
/// assert_eq!(set.slots().collect::<Vec<_>>(), [s]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LinkSet(u128);

const _: () = assert!(u128::BITS as usize == LinkLive::SLOTS);

impl LinkSet {
    /// No slot.
    pub const EMPTY: LinkSet = LinkSet(0);

    /// **Every slot a cell of `cells` names.** Sixteen cells at a time, and
    /// a block naming none is skipped: the link bits of sixteen flag words
    /// OR-ed are one test a block, and nearly every cell names no link.
    pub fn of_cells(cells: &[PackedCell]) -> LinkSet {
        const LINKS: u16 = CellFlags::LINK_MASK << CellFlags::LINK_SHIFT;
        let mut bits = 0u128;
        for block in cells.chunks(16) {
            if block.iter().fold(0, |a, c| a | c.flags.0) & LINKS == 0 {
                continue;
            }
            for c in block {
                bits |= 1 << c.flags.link().index();
            }
        }
        LinkSet(bits & !1)
    }

    /// This set and slot `s`.
    pub fn with(self, s: LinkSlot) -> LinkSet {
        LinkSet(self.0 | s.bit())
    }

    /// Whether the set holds slot `s`.
    pub fn has(self, s: LinkSlot) -> bool {
        self.0 & s.bit() != 0
    }

    /// The slots either set holds.
    pub fn union(self, o: LinkSet) -> LinkSet {
        LinkSet(self.0 | o.0)
    }

    /// Whether the two sets hold a slot in common.
    pub fn meets(self, o: LinkSet) -> bool {
        self.0 & o.0 != 0
    }

    /// Whether the set holds no slot.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The slots held, lowest first.
    pub fn slots(self) -> impl Iterator<Item = LinkSlot> {
        let mut rest = self.0;
        // Bit 0 is never set, so a set bit's place is never zero.
        core::iter::from_fn(move || {
            let low = (rest != 0).then(|| rest.trailing_zeros() as u8)?;
            rest &= rest - 1;
            core::num::NonZeroU8::new(low).map(LinkSlot)
        })
    }
}

/// **A mark per link slot, each judged once between clears**: the first
/// [`SlotMarks::get_or_judge`] of a slot runs its judge and keeps the
/// answer, and every later one until [`SlotMarks::clear`] returns it. There
/// is no setter, and the fields are private.
///
/// ```compile_fail,E0616
/// fn f(m: &mut baud_domain::SlotMarks<u8>) { let _ = &mut m.marks; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{LinkId, SlotMarks};
/// let s = LinkId::from_index(9).slot().expect("a slot");
/// let mut m = SlotMarks::<u8>::new();
/// assert_eq!(m.get_or_judge(s, || 4), 4);
/// assert_eq!(m.get_or_judge(s, || 7), 4);
/// m.clear();
/// assert_eq!(m.get_or_judge(s, || 7), 7);
/// ```
#[derive(Clone, Debug)]
pub struct SlotMarks<M: Copy + Default> {
    judged: LinkSet,
    marks: [M; LinkLive::SLOTS],
}

impl<M: Copy + Default> SlotMarks<M> {
    /// Nothing judged.
    pub fn new() -> SlotMarks<M> {
        SlotMarks {
            judged: LinkSet::EMPTY,
            marks: [M::default(); LinkLive::SLOTS],
        }
    }

    /// Every slot to be judged again.
    pub fn clear(&mut self) {
        self.judged = LinkSet::EMPTY;
    }

    /// **Slot `s`'s mark**: the one kept after the last clear, else
    /// `judge`'s, kept.
    pub fn get_or_judge(&mut self, s: LinkSlot, judge: impl FnOnce() -> M) -> M {
        if !self.judged.has(s) {
            self.marks[s.index()] = judge();
            self.judged = self.judged.with(s);
        }
        self.marks[s.index()]
    }
}

impl<M: Copy + Default> Default for SlotMarks<M> {
    fn default() -> Self {
        SlotMarks::new()
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
    /// what a cluster machine will read. They differ on `E0 A0 96` (U+0816
    /// SAMARITAN MARK IN), an advance of one and a width of 0, which
    /// `the_cell_table_is_what_the_write_path_writes` holds the entry to.
    ///
    /// Low two bits the advance, next two the width, and bit 4 whether the
    /// character is one the table may write without asking the cluster
    /// machine anything but whether it breaks before it
    /// ([`CellFlags::is_ordinary_entry`]). A write replaces the flags with the
    /// terminal's own, so the field is free until then.
    pub fn of_entry(w: Width, c: &Classified) -> CellFlags {
        let ordinary = c.class == GraphemeClass::Other && c.lookback.is_none();
        CellFlags(
            (1 + (w.cells() == 2) as u16)
                | ((w.cells() as u16) << 2)
                | ((ordinary as u16) << CellFlags::ORDINARY_SHIFT),
        )
    }
    const ORDINARY_SHIFT: u16 = 4;
    pub fn advance_of(self) -> usize {
        (self.0 & 0b11) as usize
    }
    pub fn width_of(self) -> Width {
        Width(((self.0 >> 2) & 0b11) as u8)
    }
    /// **The entry's class is `Other` with no lookback**, the run gate's
    /// test (`all_ordinary` in `put_run`): such a character starts a
    /// cluster wherever the machine breaks before an `Other`, and the read
    /// asks it that. Anything else -- a mark, a ZWJ, a Prepend, a spacing
    /// mark, an emoji -- goes to the computing path, where the machine
    /// decides whether it joins the cell before.
    pub fn is_ordinary_entry(self) -> bool {
        self.0 & (1 << CellFlags::ORDINARY_SHIFT) != 0
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
    /// set. `refresh_flags` sets it and nothing outside the tests tests the
    /// bit; `Charset::from_flags` would, through `PackedCell::from_wire`,
    /// which has no caller.
    ///
    /// `[[source: vt100-ug-ch3]]` Table 3-9 names the set's 32 positions,
    /// `0x5F..0x7E`, in text. None of the DEC texts read gives a codepoint, so
    /// the 31 the table has are `[[source: foot-vt100-0]]`'s, and `0x5F`
    /// "Blank" stays `_`: an open departure, U+0020 or U+00A0 still to be
    /// chosen.
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
    /// A cell rewritten *in place* has no scroll, no blank and no generation
    /// bump to say so, and a space written over `e`-plus-acute must not keep
    /// the accent. Every write builds a fresh `PackedCell` from the terminal's
    /// current flags, which never include this bit, so the stale marks become
    /// unreachable without the write path doing anything at all.
    ///
    /// It also takes the store off the read path: a cell without this bit
    /// has no marks, and that is the overwhelming majority of cells, answered
    /// by a bit that is already in cache rather than by a probe into the
    /// store.
    pub const MARKED: CellFlags = CellFlags(4);

    /// Bits 3..10 carry the [`LinkId`] (`LINK_BITS`); bit 10 is
    /// [`CellFlags::GUARDED`] and 11..15 the set a code was received under
    /// ([`CellFlags::received`]); none is free. The [`ClusterId`] rides in
    /// the cell's [`Content`] instead.
    ///
    /// **The link rides in this word rather than in a field of its own.**
    /// `PackedCell` is 8 bytes, the width a codepoint needs, and a ninth and
    /// tenth byte would round the cell to twelve.
    const LINK_SHIFT: u16 = 3;
    /// The link takes exactly the bits `NLINK = 128` needs, and no more.
    ///
    /// **Masked on every read**: `link()` takes `self.0 >> 3` through
    /// `LINK_MASK`, so the bits above the link's range -- the guard and the
    /// received set -- never come back as part of the link index.
    const LINK_BITS: u16 = 7;
    const LINK_MASK: u16 = (1 << CellFlags::LINK_BITS) - 1;

    pub fn with_link(self, l: LinkId) -> CellFlags {
        let keep = self.0 & !(CellFlags::LINK_MASK << CellFlags::LINK_SHIFT);
        CellFlags(keep | ((l.index() as u16 & CellFlags::LINK_MASK) << CellFlags::LINK_SHIFT))
    }
    pub fn link(self) -> LinkId {
        LinkId::from_index(((self.0 >> CellFlags::LINK_SHIFT) & CellFlags::LINK_MASK) as usize)
    }
    /// The flags without the link: a cell built from the pen that names no
    /// text that was written, DECFRA's fill.
    pub fn without_link(self) -> CellFlags {
        CellFlags(self.0 & !(CellFlags::LINK_MASK << CellFlags::LINK_SHIFT))
    }

    /// Bits 11..15: the set GL held when a GL code wrote the cell, where
    /// that set replaced the code with another character ([`Received`]),
    /// 0 for none, else [`Received::tag`].
    ///
    /// **What lets DECRQCRA count the code a character was received as**:
    /// `[[source: dec-std-070]]`'s display stores "code = parv[0] & 0x7F"
    /// and the set beside it (p. 5-140), and the cell holds the translated
    /// codepoint alone, from which the code cannot be recovered -- German's
    /// `[` and Norwegian/Danish's `@` are both `Ä`. Carried in the pen a GL
    /// code is written with, beside [`CellFlags::GRAPHICS`], so the write
    /// path ORs what it already ORs; the pen a decoded character is written
    /// with, a UTF-8 `Ä`'s among them, has it cleared, since no set
    /// replaced that character. **Apart from `GRAPHICS`**, which names the
    /// set GL held whatever arrived: a UTF-8 `─` written
    /// while GL holds line drawing carries that flag and no tag.
    const SET_SHIFT: u16 = 11;
    const SET_MASK: u16 = 0x1f;

    /// These flags with `r` as the set the code was received under, or none.
    pub fn with_received(self, r: Option<Received>) -> CellFlags {
        let keep = self.0 & !(CellFlags::SET_MASK << CellFlags::SET_SHIFT);
        let tag = r.map_or(0, |r| u16::from(r.tag()));
        CellFlags(keep | (tag << CellFlags::SET_SHIFT))
    }

    /// The set a GL code that wrote the cell was received under, if it
    /// replaced the code.
    pub fn received(self) -> Option<Received> {
        let tag = (self.0 >> CellFlags::SET_SHIFT) & CellFlags::SET_MASK;
        Received::from_tag(tag as u8)
    }

    /// The national set GL held when a GL code wrote the cell.
    pub fn national(self) -> Option<Nrcs> {
        match self.received() {
            Some(Received::National(n)) => Some(n),
            _ => None,
        }
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
    /// ECMA-48's and decides the erasures ERM names.
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
    /// the write path doing anything**, the property `CellFlags::MARKED`
    /// states.
    pub fn without_cluster(self) -> CellFlags {
        CellFlags(self.0 & !CellFlags::MARKED.0)
    }
}

impl PackedCell {
    /// **Whether two rows of cells are the same, a byte compare**: the
    /// cells' words compared as one block of bytes, which the library runs
    /// a vector at a time, where comparing them cell by cell field by field
    /// does not. A cell has no padding (`IntoBytes`), so equal bytes are
    /// equal cells.
    pub fn same_cells(a: &[PackedCell], b: &[PackedCell]) -> bool {
        a.as_bytes() == b.as_bytes()
    }

    /// **A cell from its eight bytes as one word**: the content in the low
    /// thirty-two bits, the style above it and the flags in the top sixteen,
    /// which under `repr(C)` on a little-endian target is the cell's own
    /// layout, so a vector of words stores as cells.
    ///
    /// Inside the crate only, and only for words built from parts already
    /// valid: a [`Codepoint`]'s value or a printable ASCII byte in the low
    /// half, and a valid cell's style and flags in the high half.
    #[inline(always)]
    pub(crate) const fn from_word(w: u64) -> PackedCell {
        PackedCell {
            content: Content(w as u32),
            style: StyleId((w >> 32) as u16),
            flags: CellFlags((w >> 48) as u16),
        }
    }

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
    /// **The flags the position keeps**: DECSCA's attribute, ECMA-48's guard
    /// (under DECSERA as well) and an
    /// `OSC 8` link. The line-drawing
    /// flag, the set the character was received under and a cluster's mark
    /// belong to the character and go with it, so the space draws as a
    /// space, counts 0x20, and its stale marks are unreachable
    /// ([`CellFlags::MARKED`]).
    pub fn selectively_erased(old: PackedCell) -> PackedCell {
        if old.content.is_empty() {
            return old;
        }
        let kept = old.flags.0 & !(CellFlags::GRAPHICS.0 | CellFlags::MARKED.0);
        PackedCell::glyph(
            Codepoint::SPACE,
            old.style,
            CellFlags(kept).with_received(None),
        )
    }
    /// An ASCII byte under whatever character set `flags` says is invoked.
    ///
    /// Nothing calls it. The bulk path translates through `with_charset` in
    /// `View::put_ascii_run` and the per-byte path through `translate`, both
    /// with the set `Term` has invoked. The flags name the sets that replace
    /// a code (`Charset::from_flags`), not the UK set or the DEC sets that
    /// draw their byte, so this would leave the UK set's `#` untranslated.
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
    /// **The per-byte path translates through the bulk path's rule**,
    /// `with_charset`, so a byte that reaches `ground_uncommon` -- printable
    /// ASCII recovered after an ill-formed UTF-8 sequence, the case
    /// `[[source: unicode-ch3]]` § 3.9.5 names -- is translated as on every
    /// other route: with DEC Special Graphics designated, `<C2 71>` puts `─`
    /// on the screen (`ascii_recovered_after_a_bad_sequence_is_still_translated`).
    ///
    /// **Inlined for ASCII, called for a set.** The ASCII answer is a compare
    /// and a return in the caller; a designated set goes through
    /// `translate_designated`, out of line, so `with_charset`, forced inline
    /// there, never makes this function too large to inline into every short
    /// run and every run's `REP` record.
    #[inline(always)]
    pub fn translate(b: StreamByte, set: Charset) -> Codepoint {
        // **ASCII is the answer, not a case of the general one.** The general
        // path builds a throwaway `PackedCell`, runs a charset map with
        // nothing to do and checks the content back through
        // `Codepoint::new`'s range-and-surrogate test, on a byte the caller
        // has already proven printable ASCII; this is one compare.
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
    /// graphics and forget Finnish.
    ///
    /// **`#[inline(always)]`, into the two out-of-line callers on the
    /// designated path**, `put_designated_run` and `translate_designated`, so a
    /// designated byte is a match and a table read, not a call; the ASCII path
    /// reaches neither caller.
    #[inline(always)]
    pub fn with_charset(self, b: StreamByte, set: Charset) -> PackedCell {
        match set {
            Charset::DecGraphics => self.with_dec_graphics(b),
            Charset::Uk => self.with_nrcs(b, crate::uk_table()),
            Charset::National(n) => self.with_nrcs(b, crate::nrcs_table(n)),
            Charset::Dec(d) => match d.table() {
                Some(t) => self.with_dec_set(b, t),
                // DEC Technical, DEC Turkish Supplemental, Cyrillic and the
                // two undocumented sets: each draws its byte, for the reason
                // on its `DecSet` variant.
                None => self.with_ascii(b),
            },
            Charset::Iso96(i) => self.with_iso96(b, i.table()),
            // ASCII and anything unrecognised: the byte stands for itself.
            Charset::Ascii | Charset::Other => self.with_ascii(b),
        }
    }

    /// The same, under a 96-character set: **one load from a 96-entry
    /// table** indexed by `code - 0x20`, SP's and DEL's positions included
    /// (`[[source: ecma35]]` 8.3.1). `b` is the 7-bit code, 2/0 through 7/15.
    pub fn with_iso96(self, b: StreamByte, table: &[Codepoint; 96]) -> PackedCell {
        let cp = match (b.raw_byte() as usize).checked_sub(0x20) {
            Some(i) if i < table.len() => table[i],
            _ => Codepoint::from_ascii(b),
        };
        PackedCell {
            content: Content::codepoint(cp),
            ..self
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
    /// The whole point is that the tail is *already assembled*. A cell built
    /// field by field inside a per-cell loop can compile to narrow stores
    /// instead of one 8-byte one; hoisting the invariant tail out of the loop
    /// and replacing only `content` keeps the one store.
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
    ///
    /// **A linear search over 8 to 10 pairs**, a `const` array compiled by
    /// `build/nrcs.rs`, on the per-character path for as long as a national
    /// set is designated (`ESC ( % 6`, for one). The pair arrays are
    /// private, so direct-indexed rows would be a change to the build, not
    /// to a table. `every_national_replacement_renders_as_its_table_says`
    /// drives the fourteen `Nrcs` sets' 164 replacements through `Term`; UK
    /// is a `Charset` of its own, which that test does not drive.
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
    /// "Blank". Whether Blank stores U+0020 or U+00A0 is open, and the base
    /// here moves to `0x5F` with the answer.
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
/// **Named once, so the link table and the cluster store hash alike**: the
/// prime is 2^40 + 0x1b3, and both take their slot from the high bits of the
/// state finalized by `FIBONACCI`, [`LinkId::bucket`]'s construction, since
/// the high bits of an unfinalized FNV state carry little of its last input.
pub(crate) const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub(crate) const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
pub(crate) const FIBONACCI: u64 = 0x9E37_79B9_7F4A_7C15;

/// Which interned cluster a cell's continuations live in, or none.
///
/// Zero is "no continuations", so a zeroed grid has none and there is no
/// parallel validity array to keep in step -- the same trick [`LinkId`] and
/// `ColorWord` use.
///
/// **It rides in the cell's [`Content`]**, in the range above U+10FFFF
/// ([`Content::cluster`]), so it moves when the cell moves: rotation, a
/// frame's read, the alternate screen and a reflow carry it for free, and
/// nothing keyed on *where the cell is* has to move with them, or can leave
/// one cell's marks at an address the next write inherits.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClusterId(u16);

impl ClusterId {
    pub const NONE: ClusterId = ClusterId(0);
    /// The first id of a named sequence's node (`sequence.rs`): above every
    /// slot a store has, [`ClusterStore::SLOTS_MAX`], so a node's id never
    /// names a slot of the store and a sweep never frees one.
    pub const SEQUENCES: ClusterId = ClusterId(0x8000);
    /// The id of the named sequence's node `node`.
    pub(crate) const fn sequence(node: usize) -> ClusterId {
        ClusterId(ClusterId::SEQUENCES.0 + node as u16)
    }
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
///
/// A slot's continuations and their count are private: a cluster is written
/// only by the store's intern, so len never exceeds CAPACITY and readers see
/// only cps()[..len].
///
/// ```compile_fail,E0451
/// let _ = baud_domain::ClusterSlot { base: baud_domain::Content::EMPTY, len: 0, cps: [0; 8] };
/// ```
///
/// The legal path:
///
/// ```
/// let s = baud_domain::ClusterSlot::EMPTY;
/// assert!(s.is_empty() && s.cps().is_empty());
/// ```
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
    /// U+1F469 ZWJ U+1F467 ZWJ U+1F466` is a base and six attachments, more
    /// than the accents Latin, Devanagari or kana stack on one base.
    ///
    /// It still does not cover every RGI sequence: a four-person family with a
    /// skin tone on each member is eleven codepoints, and those lose their tail.
    /// That is a bounded table behaving as one, stated rather than implied by a
    /// number, and a departure: the Unicode Standard's grapheme cluster is a
    /// base "together with any number of nonspacing marks applied to it"
    /// (`[[source: unicode18-ch3]]`, D60). The cap is an open departure, and
    /// `docs/vt-coverage.md` states it.
    pub const CAPACITY: usize = 8;

    pub const EMPTY: ClusterSlot = ClusterSlot {
        base: Content::EMPTY,
        len: 0,
        cps: [0; ClusterSlot::CAPACITY],
    };

    /// A slot of `seq`, a base and up to [`ClusterSlot::CAPACITY`]
    /// continuations: a named sequence's node, built at compile time.
    pub(crate) const fn named(seq: &[u32]) -> ClusterSlot {
        let mut s = ClusterSlot::EMPTY;
        s.base = Content(seq[0]);
        let mut i = 1;
        while i < seq.len() {
            s.cps[i - 1] = seq[i];
            i += 1;
        }
        s.len = (seq.len() - 1) as u8;
        s
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }
    pub fn marks(&self) -> impl Iterator<Item = u32> + '_ {
        self.cps[..self.len as usize].iter().copied()
    }
    /// The continuations, as the slot holds them.
    pub fn cps(&self) -> &[u32] {
        &self.cps[..self.len as usize]
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
/// rendition table's sweep is the same hazard for colours and forces a full
/// pass the same way (`Reclaims::recolours_since`).
///
/// Only the cluster store moves its epoch (a sweep that freed slots, a reset);
/// a reader holding cells across reads can compare epochs for equality but
/// cannot construct or forge one.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::ClusterEpoch(1);
/// ```
///
/// The legal path:
///
/// ```
/// let a = baud_domain::ClusterEpoch::default();
/// assert_eq!(a, baud_domain::ClusterEpoch::default());
/// ```
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
/// **It refuses rather than evicts.** A wrong *cluster* puts another cell's
/// accent on a character, which no renderer, selection or copy can
/// recover. So a collision is refused and counted, the
/// way `OSC 52` clipboard reads are refused and counted: the answer is never
/// wrong, and how often it is missing is a number.
///
/// **And it reclaims what no cell names, once every slot is filled.** When
/// a cluster finds its slots taken and every slot of the store holds a
/// cluster, the caller may [`ClusterStore::sweep`] the cells that can name
/// one -- both screens, and the history behind the primary -- and every
/// slot none of them names is freed. Sweep when full: when all slots are
/// used, walk the live cells once and free unreferenced slots. No cost
/// until the table fills.
///
/// **That bounds what is refused only once the store has filled.** Before
/// it, a cluster whose eight slots are taken is refused while other slots
/// are free; after a sweep, what is left is what cells name, and a cluster
/// is refused when its eight slots hold those. Neither is a bound on what
/// is on a cell at once: a cluster can be refused while far fewer clusters
/// are on cells than the store has slots.
///
/// The store refuses rather than evicts: its slots, fill count and epoch are
/// private, so a slot a cell names is changed only by intern, a sweep of
/// unnamed slots, or reset, and a refusal is counted in refused.
///
/// ```compile_fail,E0616
/// fn f(s: &baud_domain::ClusterStore<'_>) -> usize { s.slots.len() }
/// ```
///
/// The legal path:
///
/// ```
/// let mut slots = [baud_domain::ClusterSlot::EMPTY; 16];
/// let s = baud_domain::ClusterStore::new(&mut slots);
/// assert_eq!(s.filled() + s.refused, 0);
/// let _ = s.sweep_due();
/// ```
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
    /// Eight, because at 1024 slots and the hash above fewer probes refuse
    /// clusters of pointed Hebrew, Arabic and Hindi text that eight keep.
    /// The sweep runs only once every slot is used, so before that the
    /// probe count alone decides what is refused.
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
    /// on every mark. The rendition table bounds its sweep at the same rate,
    /// a sixteenth of its table, counted in rendition writes
    /// (`SweepBook::due`). Counted here in
    /// interns, never in reads or time, so where the reads fall cannot move
    /// it.
    const DUE: u32 = (ClusterStore::SLOTS_MAX / 16) as u32;

    fn len(&self) -> usize {
        self.slots.len().min(ClusterStore::SLOTS_MAX)
    }

    /// The slot `content` names, if it names one that holds a cluster: one
    /// of the store's, or a named sequence's node's (`sequence.rs`), which
    /// the binary holds and no store does.
    pub fn get(&self, content: Content) -> Option<&ClusterSlot> {
        let i = content.cluster_id()?.index().checked_sub(1)?;
        if i >= self.len() {
            return crate::sequence::slot(content);
        }
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

    /// **The cluster `cp` makes of a cell with content `content`, when it is
    /// a prefix of a sequence the Unicode Standard names** (`sequence.rs`):
    /// its node's id, which no slot of the store holds, and its slot. One
    /// step of a trie built from the pinned data, so nothing is hashed over
    /// the cluster, probed or written; `None` is every other cluster, which
    /// the caller interns whole ([`ClusterStore::try_intern`]).
    #[inline(always)]
    pub fn named(
        &self,
        content: Content,
        cp: Codepoint,
    ) -> Option<(ClusterId, &'static ClusterSlot)> {
        crate::sequence::step(content, cp)
    }

    /// Whether some named sequence continues with a codepoint of `cp`'s
    /// block: one bit, so a caller can keep every other mark off the trie.
    #[inline(always)]
    pub fn may_name(&self, cp: Codepoint) -> bool {
        crate::sequence::continues(cp)
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
    /// **`#[inline]`, into `attach_before`, which is out of line already**:
    /// without the attribute a call across the crate boundary is not
    /// inlined, and an intern is on the path a cell's mark takes.
    #[inline]
    pub fn try_intern(&mut self, base: Content, cps: &[u32]) -> Result<ClusterId, Refused> {
        let n = self.len();
        if cps.is_empty() || cps.len() > ClusterSlot::CAPACITY || n == 0 {
            return Err(Refused::Shape);
        }
        // FNV-1a over the base and the continuations, then finalized, and the
        // slot taken from the **high** bits: [`LinkId::bucket`]'s construction.
        // The low bits of a multiplicative hash collide on structured input,
        // SGR colours being multiples of 0x101010, and without the finalizer
        // the high bits of an FNV state carry little of its last input
        // (`FNV_PRIME`).
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
    /// slot holds a cluster**, and then only once [`ClusterStore::DUE`]
    /// interns, fills or refusals, have wanted a slot since the last sweep.
    /// A store never swept, or reset, sweeps at its first refusal once full;
    /// a store a sweep left with fewer than `DUE` slots free waits `DUE`
    /// interns from that sweep, whatever its cells name by then.
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
