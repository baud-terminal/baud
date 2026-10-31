use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};

/// Bytes borrowed from the [`Store`].
///
/// Tied to `&Store`, so it cannot be held across the `&mut` drain that would
/// recycle them. The invariant "no run outlives its tier" is the borrow checker
/// rather than a generation counter.
pub struct Resident<'store> {
    /// Unproven on arrival: what the cold tier holds is whatever the
    /// application wrote, which may be binary. Callers scan it to earn a
    /// stronger [`Kind`] before treating it as text.
    bytes: Bytes<'store, Raw>,
}

impl<'store> Resident<'store> {
    pub fn bytes(&self) -> Bytes<'store, Raw> {
        Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        }
    }
}

/// Two-tier byte store.
///
/// The arena is 1 MiB because a size sweep peaked there against a 2 MiB L2 and
/// went *negative* by 64 MiB. At 280 MB/s it laps every 3.57 ms, so anything
/// retained must reach the cold ring first.
#[allow(dead_code)] // model only: the two-tier store is designed, not implemented
pub struct Store<'buf> {
    arena: &'buf mut [u8],
    cold: &'buf mut [u8],
    head: BytePos,
    drained: BytePos,
    cold_floor: BytePos,
}

impl<'buf> Store<'buf> {
    pub fn head(&self) -> BytePos {
        self.head
    }

    /// `None` means evicted: the bytes are gone and the line cannot be
    /// rendered, searched, or reflowed. Callers must handle it — there is no
    /// panicking accessor.
    pub fn resolve(&self, _pos: BytePos, _len: ByteLen) -> Option<Resident<'_>> {
        None // model only
    }

    /// Ingest may not lap the drain thread.
    ///
    /// Returns [`Lapped`] rather than silently discarding history. This is the
    /// one invariant the type system cannot express, so it is a `Result` the
    /// caller cannot ignore under `#[must_use]`.
    #[must_use = "a Lapped error means ingest overran the drain thread; dropping it silently discards history"]
    pub fn advance_head(&mut self, n: ByteLen) -> Result<BytePos, Lapped> {
        let next = self.head.advance(n);
        if next.gap(self.drained).exceeds(self.arena.len()) {
            return Err(Lapped);
        }
        self.head = next;
        Ok(next)
    }
}

/// Ingest outran the cold-tier writer. History would be lost.
#[derive(Debug)]
pub struct Lapped;

/// A run of bytes sharing one style, as stored.
///
/// Crosses the mmap boundary: the cold tier is a byte-mapped file reinterpreted
/// as `&[Run]` with no copy and no parse. That is what forces `_pad` to be
/// written out — zerocopy rejects implicit padding, because padding is
/// uninitialised memory and writing it to disk or shared memory leaks whatever
/// was there.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Run {
    pub pos: BytePos,
    pub len: ByteLen,
    pub style: StyleId,
    /// [`Encoding`] as a byte. Stored rather than re-derived, so materialisation
    /// branches once per run instead of re-classifying what the scanner knew.
    /// Held as `u8` because not every bit pattern is a valid variant — recover
    /// it with [`Run::encoding`], which is checked.
    pub encoding: u8,
    /// **Dead: nothing writes or reads it.** Carved from the padding this
    /// struct already carried, for [`Charset`] as a byte.
    ///
    /// It was for `promote`, which replayed a run into cells long after the
    /// designation, so a line written under German and materialised later
    /// would have been translated with whatever set was current. `promote`
    /// was `baud-core2`'s, deleted; nothing in this core builds a `Run`, a
    /// cell holds the codepoint its set translated the byte to when it was
    /// written, and `Charset::to_u8`, which encoded the byte, is gone. The
    /// field stays because removing it changes the layout of a struct that
    /// crosses the mmap boundary, which is a change of its own.
    pub charset: u8,
    /// **Dead, like `charset`: nothing writes or reads it.** It held
    /// [`CellFlags`] as a byte for `promote`, which replayed a run into cells:
    /// the run *was* the storage in `baud-core2`'s `OpMode::Runs`, and a
    /// promotion that dropped the flag would have made protection depend on
    /// when the line happened to materialise. A cell carries its own
    /// [`CellFlags`] from the moment it is written.
    ///
    /// **Its accessors are gone** (2026-10-19): a byte cannot hold the
    /// sixteen-bit flags, and the round trip dropped bit 10, the ECMA-48
    /// guard. The field stays for `charset`'s reason, the struct's layout.
    pub flags: u8,
    pub _pad: [u8; 1],
}

impl Run {
    /// Checked: a byte off disk is not necessarily a valid [`Encoding`].
    pub fn encoding(&self) -> Option<Encoding> {
        Encoding::try_read_from_bytes(&[self.encoding]).ok()
    }
}

/// Half-open range into the shared run pool.
#[derive(Clone, Copy)]
pub struct RunRange {
    pub start: RunIdx,
    pub count: RunCount,
}

/// How far glyphs have reached in one **physical** row.
///
/// A high-water mark, monotone between resets: it only grows as text is
/// written and returns to zero only when the row is blanked. That direction is
/// the safety argument -- a mark that is too large costs a few stores, a mark
/// that is too small leaves stale text on screen, and `max` cannot produce the
/// second.
///
/// Physical, not screen, so a `RowRot` scroll leaves every mark where it is:
/// the rotation changes which screen row *reads* a physical row, not what that
/// row contains. The two places that genuinely move content --
/// `Grid::insert_lines_in`/`delete_lines_in` and `Term::viewport`'s
/// un-rotation -- permute the marks in the same loop that permutes the rows.
///
/// The word is a `u16` rather than `Col`'s `usize`: `MAX_COLS` is 2048 so it
/// fits, and with the path and the side margins a row wrapped at beside it
/// ([`WrapMargins`]) an entry is 8 bytes, so at `MAX_ROWS` entries per
/// buffer and two buffers the narrow form is 10 KB of working set against
/// the 20 KB a `usize` word and its padding would take; a line of the
/// history holds one too, 8 bytes beside its cells' 8 a column. That is the
/// "thousands are stored" case `CONTRIBUTING.md` names as the one where
/// narrowing earns its place, and it is read once per row operation rather than
/// once per cell.
///
/// **Beside the word, the row's paragraph properties** ([`LineBidi`]): the
/// character path SCP and SPD give a line (`[[source: ecma48]]` 8.3.111 and
/// 8.3.130) and terminal-wg's autodetection and box mirroring
/// (`[[source: terminal-wg-bidi-basic-modes]]`), which belong to the row the
/// way the wrap and the rendition do and so ride where every move of a row
/// already carries them, one byte. A field of
/// its own rather than bits of the word, because the word has none free --
/// its mark reaches the pitch, past 2048 columns, beside the wrap and the
/// rendition -- and a flag above the mark would win `reached`'s bare `max`
/// and freeze it. A write updates the word and leaves the path; a fill
/// keeps it (an erased line is the same paragraph, terminal-wg's "both new
/// paragraphs receive the old one's BiDi properties"), and a fill over the
/// whole row clears only the mark DECRLM left ([`RowExtent::erased`]); only
/// the core's line rules set the rest ([`RowExtent::with_bidi`]).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(C)]
pub struct RowExtent {
    word: u16,
    bidi: LineBidi,
    links: RowLinks,
    margins: WrapMargins,
}

/// **Whether a row may hold a cell that names an `OSC 8` link**, for the
/// link table's sweep, which reads only the rows that may
/// (`baud-core`'s `links.rs`). In the byte the row's mark left as padding,
/// so every move of a row carries it as it carries the mark, and the entry
/// stays eight bytes.
///
/// **Too many is the safe direction**, as for the mark: a row marked
/// [`RowLinks::May`] that holds none costs the sweep a read of it, and the
/// sweep, finding none, marks it [`RowLinks::None`]. A row marked `None`
/// that holds one would let the sweep free a slot a cell still names, and
/// that cell would open whatever target took the slot next. So a write of
/// a linked cell marks its row, and only a sweep's read, a new line, or an
/// erase of the row from column 0 at the whole pitch says `None`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
pub enum RowLinks {
    /// No cell of the row names a link.
    #[default]
    None = 0,
    /// A cell of the row may name one.
    May = 1,
}

/// **The side margins a row wrapped at** ([`WrappedAt::Margins`]): the
/// DECSLRM left margin the next line started at and the right margin of
/// the row's line, so the display reads the row's paragraph inside them
/// and each run of cells outside them as a paragraph of its own. Read only
/// while the row's mark says it wraps, at margins
/// ([`RowExtent::margins`]); a later wrap at other margins writes the
/// newer pair, and a later wrap at the page's edge drops them
/// ([`RowExtent::at_edge`]). Two columns, each at most [`RowExtent::MAX_MARK`].
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(C)]
pub struct WrapMargins {
    left: u16,
    right: u16,
}

impl WrapMargins {
    /// None recorded: a row that has not wrapped at margins.
    pub const NONE: WrapMargins = WrapMargins { left: 0, right: 0 };

    /// The margins from column `left` to column `right`, both inclusive.
    pub fn of(left: Col, right: Col) -> WrapMargins {
        let at = |c: Col| u16::try_from(c.index()).unwrap_or(RowExtent::MARK);
        WrapMargins {
            left: at(left),
            right: at(right),
        }
    }

    /// The left margin: the first column inside.
    pub fn left(self) -> Col {
        Col::at_index(usize::from(self.left))
    }

    /// The right margin: the last column inside.
    pub fn right(self) -> Col {
        Col::at_index(usize::from(self.right))
    }
}

/// **Which way a line's characters run**: SCP's `Ps1`, ECMA-48 8.3.111's
/// "character path", or none selected. With BDSM set (IMPLICIT) and
/// autodetection off a selected path is the paragraph's level, set
/// outright (`[[source: uax9-bidi]]` HL1), and none is left to right; with
/// autodetection on, as at power-on, the level its first strong letter
/// gives it (P2-P3), the path the fallback ([`LineBidi::paragraph`]).
/// With BDSM reset (EXPLICIT) a right-to-left line is drawn in reverse,
/// terminal-wg's explicit RTL (`[[source: terminal-wg-bidi-basic-modes]]`),
/// and the others in the order the cells hold.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
pub enum CharacterPath {
    /// None selected: the power-on state, and what RIS and DECSTR return to.
    #[default]
    Default,
    /// SCP 1, SPD 0: left to right.
    LeftToRight,
    /// SCP 2, SPD 3: right to left.
    RightToLeft,
}

/// **terminal-wg's direction autodetection**, `?2501`
/// (`[[source: terminal-wg-bidi-basic-modes]]`, "Direction autodetection
/// enabled or disabled"): on, an implicit paragraph's direction is found
/// from its first strong letter (UAX #9 P2-P3), its character path the
/// fallback where none is found; off, the path is its direction. **On at
/// power-on**, so a line nobody set a direction for takes its first strong
/// letter's; the draft leaves the default to the terminal ("terminal
/// emulators might pick their favorite one"), and mintty starts with it on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Autodetect {
    On,
    Off,
}

/// **terminal-wg's box mirroring**, `?2500`
/// (`[[source: terminal-wg-bidi-misc]]`, "to mirror the U+2500 .. U+257F
/// glyphs in RTL"): on, a Box Drawing character drawn right to left is
/// drawn as its left-right counterpart ([`box_counterpart`]). Off at
/// power-on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoxMirror {
    Off,
    On,
}

/// Which of a [`LineBidi`]'s properties a control sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineProperty {
    Path,
    Autodetect,
    BoxMirror,
}

/// **A paragraph's bidirectional properties, one byte**: its character
/// path ([`CharacterPath`]), whether its direction is detected
/// ([`Autodetect`]) and whether its box drawing is mirrored ([`BoxMirror`]),
/// terminal-wg's per-paragraph model (`[[source:
/// terminal-wg-bidi-basic-modes]]`: "the following three new essential
/// BiDi properties are tracked for each paragraph", and box mirroring
/// beside them; implicit or explicit is BDSM's, terminal-wide here).
///
/// **The power-on value is the zero byte**, so a terminal no program has
/// sent one of these to holds zero on every row and the line rules, gated
/// on one mode bit, have nothing to write.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(transparent)]
pub struct LineBidi(u8);

/// **How a row's characters were written**: in the order sent, or under
/// DECRLM (`[[source: vt510-decrlm]]`), right to left from the cursor, so
/// the cells hold them in the order shown. A row written so is drawn as
/// its cells hold, UAX #9 not applied to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowWritten {
    InOrder,
    RightToLeft,
}

/// **Where a row last wrapped since it was last erased whole**: at the
/// page's edge, or at DECSLRM side margins narrower than the page, where
/// the next line starts at the left margin; a wrap at the edge after one
/// at margins leaves the edge. DECSLRM "sets the left
/// and right margins to define the scrolling region"
/// (`[[source: vt510-decslrm]]`) and says nothing of the cells outside
/// them; that they are no part of a paragraph wrapped inside the margins
/// is baud's reading. A paragraph holding a row that wrapped at margins
/// is drawn as the terminal cut it, never re-broken across its margins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WrappedAt {
    Edge,
    Margins,
}

impl LineBidi {
    /// No path, autodetection on, no box mirroring.
    pub const POWER_ON: LineBidi = LineBidi(0);
    const PATH: u8 = 0b0011;
    /// Set while autodetection is off, so that on is the zero bit.
    const FIXED: u8 = 0b0100;
    const BOX: u8 = 0b1000;
    /// A character was written on the row under DECRLM.
    const LEFTWARD: u8 = 0b1_0000;
    /// The row wrapped at side margins narrower than the page.
    const MARGINED: u8 = 0b10_0000;

    /// **Where the row last wrapped since it was last erased whole**
    /// ([`WrappedAt`]).
    pub fn wrapped_at(self) -> WrappedAt {
        match self.0 & Self::MARGINED {
            0 => WrappedAt::Edge,
            _ => WrappedAt::Margins,
        }
    }
    pub fn with_wrapped_at(self, w: WrappedAt) -> LineBidi {
        match w {
            WrappedAt::Edge => LineBidi(self.0 & !Self::MARGINED),
            WrappedAt::Margins => LineBidi(self.0 | Self::MARGINED),
        }
    }

    /// **Whether a character was written on the row under DECRLM**
    /// ([`RowWritten`]): such a row holds its characters in the order a
    /// VT510 shows them, and is drawn as its cells hold.
    pub fn written(self) -> RowWritten {
        match self.0 & Self::LEFTWARD {
            0 => RowWritten::InOrder,
            _ => RowWritten::RightToLeft,
        }
    }
    pub fn with_written(self, w: RowWritten) -> LineBidi {
        match w {
            RowWritten::InOrder => LineBidi(self.0 & !Self::LEFTWARD),
            RowWritten::RightToLeft => LineBidi(self.0 | Self::LEFTWARD),
        }
    }

    pub fn path(self) -> CharacterPath {
        match self.0 & Self::PATH {
            1 => CharacterPath::LeftToRight,
            2 => CharacterPath::RightToLeft,
            _ => CharacterPath::Default,
        }
    }
    pub fn with_path(self, p: CharacterPath) -> LineBidi {
        LineBidi((self.0 & !Self::PATH) | p as u8)
    }
    pub fn autodetect(self) -> Autodetect {
        match self.0 & Self::FIXED {
            0 => Autodetect::On,
            _ => Autodetect::Off,
        }
    }
    pub fn with_autodetect(self, a: Autodetect) -> LineBidi {
        match a {
            Autodetect::On => LineBidi(self.0 & !Self::FIXED),
            Autodetect::Off => LineBidi(self.0 | Self::FIXED),
        }
    }
    pub fn box_mirror(self) -> BoxMirror {
        match self.0 & Self::BOX {
            0 => BoxMirror::Off,
            _ => BoxMirror::On,
        }
    }
    pub fn with_box_mirror(self, b: BoxMirror) -> LineBidi {
        match b {
            BoxMirror::Off => LineBidi(self.0 & !Self::BOX),
            BoxMirror::On => LineBidi(self.0 | Self::BOX),
        }
    }
    /// This with `from`'s `which` property, the others as they are: what
    /// a control applying one property to a paragraph leaves, "The other
    /// BiDi-related properties of the given paragraph remain intact"
    /// (`[[source: terminal-wg-bidi-escape-sequences]]`).
    pub fn with_property(self, from: LineBidi, which: LineProperty) -> LineBidi {
        let mask = match which {
            LineProperty::Path => Self::PATH,
            LineProperty::Autodetect => Self::FIXED,
            LineProperty::BoxMirror => Self::BOX,
        };
        LineBidi((self.0 & !mask) | (from.0 & mask))
    }
    /// **The level an implicit paragraph is resolved at**
    /// (`[[source: terminal-wg-bidi-basic-modes]]`): with autodetection
    /// on, its first strong letter's (P2-P3), falling back to its path,
    /// right to left only where SCP or SPD chose it; with autodetection
    /// off, its path outright (UAX #9 HL1), left to right where none was
    /// chosen.
    pub fn paragraph(self) -> BidiParagraph {
        match (self.autodetect(), self.path()) {
            (Autodetect::On, CharacterPath::RightToLeft) => BidiParagraph::FirstStrongRtl,
            (Autodetect::On, _) => BidiParagraph::FirstStrong,
            (Autodetect::Off, CharacterPath::RightToLeft) => BidiParagraph::Rtl,
            (Autodetect::Off, _) => BidiParagraph::Ltr,
        }
    }
}

impl RowExtent {
    pub const ZERO: RowExtent = RowExtent {
        word: 0,
        bidi: LineBidi::POWER_ON,
        links: RowLinks::None,
        margins: WrapMargins::NONE,
    };
    /// **The row continues onto the next one**: text reached its last column
    /// and wrapped, rather than ending at a newline. The fact a reflow needs
    /// and the cells do not carry, kept in the mark's top bit because the
    /// mark is the one word per row every write already touches, and a
    /// pitch is never past [`RowExtent::MAX_MARK`] cells. A fill to the
    /// row's end clears it -- a blanked row ends where it stops -- and a
    /// newline clears it on the row it leaves.
    const WRAP: u16 = 0x8000;
    /// **The row's line rendition** ([`LineRendition`]), in the two bits
    /// under the wrap: `[[source: dec-std-070]]` § 5.6.1 gives every line
    /// one of four, and they belong to the row the way the wrap does --
    /// every move of a row moves its mark, so a scroll, `IL`, `DL`, the
    /// region repair, the ring and the alternate surface carry them with no
    /// code of their own. A fill to the row's end builds a fresh mark and
    /// so leaves the row single-width, which is what 5-210 and 5-211 give
    /// the line a scroll brings in and ED's note 1 a line it erases whole
    /// (p. 5-156).
    ///
    /// **Set only through [`RowExtent::with_rendition`], which raises the
    /// mark to the live width**, the shape [`RowExtent::wrapping`] gives the
    /// wrap: a flagged word is larger than any column, so `reached`'s bare
    /// `max` keeps it whole and never needs to raise it -- true while the
    /// mark is at least the width, and every write on the row stops before
    /// it. Too large is the safe direction for a mark.
    const SIZE: u16 = 0x6000;
    const SIZE_SHIFT: u32 = 13;
    const MARK: u16 = 0x1fff;
    /// The widest mark a row can carry: a pitch past it would not fit
    /// beside the wrap and the rendition, and `Storage::new` refuses one.
    pub const MAX_MARK: usize = Self::MARK as usize;
    /// The mark after a write that reached column `to` (exclusive).
    ///
    /// **One `max`, and the wrap and the rendition survive it without being
    /// looked at.** A wrapped row's mark is at least the width it wrapped
    /// at -- `wrapping` raises it there -- and a sized row's at least the
    /// width it was sized at, and a write reaches no further than the
    /// width, so the flagged word is the larger operand and comes through
    /// whole. This runs once a character on the per-character path, and
    /// the masked form measured 3% on cat-cjk; the cold paths that must see
    /// past the flags mask for themselves.
    ///
    /// Saturating rather than wrapping: a caller that somehow presents a column
    /// past `u16` must end up with a mark that is too *large*, never one that
    /// wraps to a small number and leaves text behind it.
    pub fn reached(self, to: usize) -> RowExtent {
        RowExtent {
            word: self
                .word
                .max(u16::try_from(to).unwrap_or(Self::MARK).min(Self::MARK)),
            ..self
        }
    }
    /// The mark: where the row stops being uniform.
    pub fn index(self) -> usize {
        (self.word & Self::MARK) as usize
    }
    /// A mark with no wrap, and no paragraph properties: a count of
    /// columns.
    pub fn of(n: usize) -> RowExtent {
        RowExtent {
            word: u16::try_from(n).unwrap_or(Self::MARK).min(Self::MARK),
            bidi: LineBidi::POWER_ON,
            links: RowLinks::None,
            margins: WrapMargins::NONE,
        }
    }
    /// **What a fill leaves**: the mark at `n` with no wrap and no
    /// rendition, and the row's paragraph properties kept -- and its
    /// [`RowLinks`], since the cells before `n` are kept, and past the live
    /// edge a fill may stop short of what the row holds.
    pub fn refilled(self, n: usize) -> RowExtent {
        RowExtent {
            bidi: self.bidi,
            links: self.links,
            margins: self.margins,
            ..RowExtent::of(n)
        }
    }
    /// Whether a cell of the row may name a link ([`RowLinks`]).
    pub fn links(self) -> RowLinks {
        self.links
    }
    /// The mark with [`RowLinks`] `l`: the rest as it is.
    pub fn with_links(self, l: RowLinks) -> RowExtent {
        RowExtent { links: l, ..self }
    }
    /// **What a fill over the whole row leaves of the paragraph
    /// properties**: the row is no longer one DECRLM wrote
    /// ([`RowWritten::InOrder`]; drawn as its cells hold "until it is
    /// erased whole"), and its path, autodetection and box mirroring are
    /// kept, as any fill keeps them.
    pub fn erased(self) -> RowExtent {
        RowExtent {
            bidi: self
                .bidi
                .with_written(RowWritten::InOrder)
                .with_wrapped_at(WrappedAt::Edge),
            margins: WrapMargins::NONE,
            ..self
        }
    }
    /// The row's character path.
    pub fn path(self) -> CharacterPath {
        self.bidi.path()
    }
    /// The row's paragraph properties.
    pub fn bidi(self) -> LineBidi {
        self.bidi
    }
    /// The mark with paragraph properties `b`: the word as it is.
    pub fn with_bidi(self, b: LineBidi) -> RowExtent {
        RowExtent { bidi: b, ..self }
    }
    /// **The side margins the row wrapped at**, where it wraps and wrapped
    /// at margins narrower than the page ([`WrappedAt::Margins`]); none
    /// where it wrapped only at the page's edge, and none once it wraps no
    /// longer -- a newline ended it, or a fill to its end -- since a row
    /// that wraps nowhere has no paragraph past it to confine.
    pub fn margins(self) -> Option<WrapMargins> {
        (self.wrapped() && self.bidi.wrapped_at() == WrappedAt::Margins).then_some(self.margins)
    }
    /// **The mark of a row that wrapped at the page's edge**: no side
    /// margins, its wrap and the rest of its paragraph properties as they
    /// are. What every wrap writes first, so margins a row wrapped at once
    /// stop applying when it wraps again where the page ends.
    pub fn at_edge(self) -> RowExtent {
        RowExtent {
            bidi: self.bidi.with_wrapped_at(WrappedAt::Edge),
            margins: WrapMargins::NONE,
            ..self
        }
    }
    /// The mark of a row that wrapped at side margins `m`: marked so, and
    /// `m` the margins it wrapped at, the newer of two wraps' kept.
    pub fn with_margins(self, m: WrapMargins) -> RowExtent {
        RowExtent {
            bidi: self.bidi.with_wrapped_at(WrappedAt::Margins),
            margins: m,
            ..self
        }
    }
    pub fn wrapped(self) -> bool {
        self.word & Self::WRAP != 0
    }
    /// The mark flagged as continuing, raised to `cols`: the width the row
    /// wrapped at, which is what lets `reached` keep the flag by `max` alone.
    ///
    /// Loose by the cells between a side margin and the edge, once, which is
    /// the safe direction -- a mark too large blanks more, never less.
    pub fn wrapping(self, cols: usize) -> RowExtent {
        let r = self.reached(cols);
        RowExtent {
            word: r.word | Self::WRAP,
            ..r
        }
    }
    /// The mark with the wrap cleared and the rendition kept: a newline
    /// ends the line, it does not change its size. `& !WRAP` is the
    /// immediate the mask before the rendition was, `0x7fff`.
    pub fn ended(self) -> RowExtent {
        RowExtent {
            word: self.word & !Self::WRAP,
            ..self
        }
    }
    /// The mark raised to at least `n`, the wrap and the rendition kept:
    /// the cold path's form of `reached`, for a raise that may exceed a
    /// flagged mark.
    pub fn raised_to(self, n: usize) -> RowExtent {
        let n = u16::try_from(n).unwrap_or(Self::MARK).min(Self::MARK);
        RowExtent {
            word: (self.word & (Self::WRAP | Self::SIZE)) | (self.word & Self::MARK).max(n),
            ..self
        }
    }
    /// Whether the word carries a flag above its mark, the wrap or a
    /// rendition other than single-width: a mark `reached` cannot raise,
    /// which a growth past the width it was flagged at has to.
    pub fn flagged(self) -> bool {
        self.word & (Self::WRAP | Self::SIZE) != 0
    }
    /// The row's line rendition.
    pub fn rendition(self) -> LineRendition {
        LineRendition::from_bits((self.word & Self::SIZE) >> Self::SIZE_SHIFT)
    }
    /// The word with rendition `r`, and with any rendition but single-width
    /// the mark raised to `cols`, the live width: the one way a rendition
    /// gets into a mark (see [`RowExtent::SIZE`]). Single-width clears the
    /// bits and keeps the mark, which is already true of the row.
    pub fn with_rendition(self, r: LineRendition, cols: Cols) -> RowExtent {
        let bare = RowExtent {
            word: self.word & !Self::SIZE,
            ..self
        };
        match r {
            LineRendition::Single => bare,
            _ => {
                let raised = bare.raised_to(cols.index());
                RowExtent {
                    word: raised.word | (r.bits() << Self::SIZE_SHIFT),
                    ..raised
                }
            }
        }
    }
}

/// **A line's rendition**, `[[source: dec-std-070]]` § 5.6.1 (p. 5-95): "A
/// conforming device shall support the following line renditions:
/// single-height, single-width; single-height, double-width; double-height,
/// double-width top; double-height, double-width bottom", one a line and
/// "mutually exclusive". `ESC # 5`, `ESC # 6`, `ESC # 3` and `ESC # 4` set
/// them (DECSWL, DECDWL, DECDHLT, DECDHLB, pp. 5-96 to 5-100).
///
/// Every rendition but single-width "doubles the character cell width
/// within the line, but does not affect the addressing of characters in the
/// line": the line holds half the columns, and column 5 is still column 5.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
pub enum LineRendition {
    #[default]
    Single = 0,
    DoubleWidth = 1,
    DoubleTop = 2,
    DoubleBottom = 3,
}

impl LineRendition {
    fn from_bits(b: u16) -> LineRendition {
        match b & 3 {
            0 => LineRendition::Single,
            1 => LineRendition::DoubleWidth,
            2 => LineRendition::DoubleTop,
            _ => LineRendition::DoubleBottom,
        }
    }
    /// The two bits the row's mark carries it in, below their shift.
    fn bits(self) -> u16 {
        match self {
            LineRendition::Single => 0,
            LineRendition::DoubleWidth => 1,
            LineRendition::DoubleTop => 2,
            LineRendition::DoubleBottom => 3,
        }
    }
    /// Whether characters on the line take two columns of the display:
    /// every rendition but single-width.
    pub fn is_double(self) -> bool {
        self != LineRendition::Single
    }
    /// The line's `end_of_line`, as a count of columns, on a screen `cols`
    /// wide: `[[source: dec-std-070]]` § 5.14.1 (p. 5-209) halves the width
    /// on any line not single-width, `end_line /= 2`, an integer division,
    /// so on an odd width the last column of the display is in no
    /// character's field. At least one, so a one-column screen keeps a
    /// column to write on.
    pub fn end_of_line(self, cols: Cols) -> Cols {
        match self {
            LineRendition::Single => cols,
            _ => Cols::of_index((cols.index() / 2).max(1)),
        }
    }
}

/// **Whether one physical row of the primary may hold anything past the live
/// edge**: a cell other than [`crate::PackedCell::EMPTY`] from the live width
/// to how far the ring's rows hold anything, or a mark past the live width.
///
/// Per row because the region repair copies a row past the edge only when it
/// or the row it lands on holds something there, and a mark cannot say which:
/// it is true to the live edge and no further, so a row uniform in a colour
/// carries that colour past the edge under a mark of zero. Set for every row
/// of the ring when a resize leaves the screen narrower than what its rows
/// hold, and kept by the passes that move or clear cells past the edge. Not
/// in [`RowExtent`]: its spare bits sit above a mark that `reached` raises by
/// a bare `max`, which a flag there would win.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
#[repr(u8)]
pub enum PastEdge {
    /// Nothing past the live edge: every cell there is `EMPTY` and the mark
    /// lies inside it.
    #[default]
    Clear,
    /// A cell or the mark may lie past the live edge.
    Held,
}

/// A number of runs.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    FromBytes,
    IntoBytes,
    Immutable,
    KnownLayout,
)]
#[repr(transparent)]
pub struct RunCount(pub(crate) u16);

impl RunCount {
    pub const ZERO: RunCount = RunCount(0);
    pub fn from_index(n: usize) -> RunCount {
        RunCount(n as u16)
    }
    pub fn inc(self) -> RunCount {
        RunCount(self.0 + 1)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

mod kind {
    pub trait Sealed {}
    impl Sealed for super::Raw {}
    impl Sealed for super::Printable {}
    impl Sealed for super::Utf8 {}
    impl Sealed for super::DecGraphics {}
}

/// How a run of bytes is to be interpreted.
///
/// Not future-proofing: the VT protocol switches this mid-stream. `ESC ( 0`
/// selects DEC Special Graphics for G0, after which `q` is `─` and `x` is `│`
/// — that is how ncurses draws boxes, and it applies to subsequent bytes until
/// switched back. A per-run tag is what the protocol actually requires.
///
/// Stored on [`Run`] so materialisation branches once per run instead of
/// re-deriving what the scanner already knew. One byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, TryFromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(u8)]
pub enum Encoding {
    /// Printable ASCII, `0x20..=0x7e`. One byte, one column, no table lookup.
    /// The ~94% path.
    Ascii,
    /// Well-formed UTF-8. Decode to graphemes for width.
    Utf8,
    /// DEC Special Graphics (`ESC ( 0`). DEC defines 32 positions, `0x5F`
    /// through `0x7E`; `PackedCell::with_dec_graphics` maps the 31 from `0x60`
    /// and leaves `0x5F` as `_`, an open departure. Width is always 1.
    DecGraphics,
    /// Uninterpretable. Render U+FFFD per byte, width 1 each. `cat /bin/ls`
    /// lands here and must render rather than be rejected.
    Raw,
}

impl Encoding {
    /// Whether width can be had without decoding. True for every fixed-width
    /// single-byte encoding, which is the property that matters — not
    /// "is it ASCII".
    pub fn is_byte_per_column(self) -> bool {
        matches!(
            self,
            Encoding::Ascii | Encoding::DecGraphics | Encoding::Raw
        )
    }
}

/// What a [`Bytes`] has been *proven* to hold. A `&[u8]` carries no such proof,
/// which is why one may not appear in a domain signature.
///
/// The static counterpart of [`Encoding`]: same information, no runtime tag,
/// used while the bytes are in flight. [`Kind::ENCODING`] is the bridge.
pub trait Kind: kind::Sealed {
    const ENCODING: Encoding;
}

/// Straight off the wire. No guarantees: a PTY delivers `cat /bin/ls` as
/// readily as text.
pub enum Raw {}

/// Verified printable ASCII, `0x20..=0x7e`. Width is exactly one column per
/// byte — the only case where a byte count legitimately *is* a column count.
pub enum Printable {}

/// UTF-8. A `Bytes<Utf8>` has been verified well formed; a `Vectorized<Utf8>`
/// names only how its sixteen bytes are read, and the scans and decodes on it
/// check the form as far as each one's doc says. Width still unknown until
/// measured.
pub enum Utf8 {}

/// DEC Special Graphics, in flight.
pub enum DecGraphics {}

impl Kind for Raw {
    const ENCODING: Encoding = Encoding::Raw;
}

impl Kind for Printable {
    const ENCODING: Encoding = Encoding::Ascii;
}

impl Kind for Utf8 {
    const ENCODING: Encoding = Encoding::Utf8;
}

impl Kind for DecGraphics {
    const ENCODING: Encoding = Encoding::DecGraphics;
}

/// A byte view carrying a proof of its contents.
#[derive(Clone, Copy)]
pub struct Bytes<'a, K: Kind> {
    raw: &'a [u8],
    _k: core::marker::PhantomData<K>,
}

impl<'a> Bytes<'a, Raw> {
    /// The single entry point for wire data. Everything downstream must earn a
    /// stronger [`Kind`] by passing a scanner.
    pub fn from_source(raw: &'a [u8]) -> Bytes<'a, Raw> {
        Bytes {
            raw,
            _k: core::marker::PhantomData,
        }
    }
    /// Split off the leading run of printable ASCII. This is the SIMD scan, and
    /// the returned `Printable` view is the proof it produced.
    pub fn split_printable(self) -> (Bytes<'a, Printable>, Bytes<'a, Raw>) {
        let n = self
            .raw
            .iter()
            .take_while(|&&b| (0x20..0x7f).contains(&b))
            .count();
        (
            Bytes {
                raw: &self.raw[..n],
                _k: core::marker::PhantomData,
            },
            Bytes {
                raw: &self.raw[n..],
                _k: core::marker::PhantomData,
            },
        )
    }
    /// Promote to UTF-8, or hand the bytes back unproven.
    pub fn validate_utf8(self) -> Result<Bytes<'a, Utf8>, Bytes<'a, Raw>> {
        match core::str::from_utf8(self.raw) {
            Ok(_) => Ok(Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            }),
            Err(_) => Err(self),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

impl<'a> Bytes<'a, Printable> {
    /// Free, and needs no [`WidthTable`] — the proof that these are printable
    /// ASCII is exactly the proof that one byte is one column. This is the
    /// ~94% fast path, and the type is what makes skipping the table sound.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

impl<'a> Bytes<'a, Utf8> {
    // `measure` lived here and returned `from_cells(0)` behind a
    // `// model only` comment, taking a `WidthTable` it discarded. Nothing
    // called it. It is deleted rather than fixed because measuring a stored
    // UTF-8 run is for reflow, selection and search, none of which existed
    // then. An API that answers zero looks implemented; its absence does not.
    pub fn len(&self) -> ByteLen {
        ByteLen::new(self.raw.len() as u16)
    }
}

impl<'a> Bytes<'a, Raw> {
    /// Reinterpret under the active character set. `ESC ( 0` makes subsequent
    /// bytes graphics; this is where that decision lands.
    pub fn as_dec_graphics(self) -> Bytes<'a, DecGraphics> {
        Bytes {
            raw: self.raw,
            _k: core::marker::PhantomData,
        }
    }
}

impl<'a> Bytes<'a, DecGraphics> {
    /// Fixed-width single-byte: one column each, no table.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

/// The dynamic form: a run whose encoding is carried rather than proven.
/// This is what gets stored, and what a `match` over stored runs sees.
pub struct Encoded<'a> {
    bytes: Bytes<'a, Raw>,
    encoding: Encoding,
}

impl<'a> Encoded<'a> {
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }
    /// Width without decoding, where the encoding permits it. `None` means the
    /// caller must decode — which is exactly the branch materialisation wants
    /// to take once per run.
    pub fn width_fast(&self) -> Option<DisplayWidth> {
        self.encoding
            .is_byte_per_column()
            .then(|| DisplayWidth::from_len(ByteLen::of(self.bytes.raw)))
    }
    /// Checked downcast back to a static proof.
    pub fn as_utf8(self) -> Option<Bytes<'a, Utf8>> {
        (self.encoding == Encoding::Utf8).then_some(Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        })
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Static proof to dynamic tag. Free — `K::ENCODING` is a constant.
    pub fn erase(self) -> Encoded<'a> {
        Encoded {
            bytes: Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            },
            encoding: K::ENCODING,
        }
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Named so every crossing back to an untyped slice is visible. Used by the
    /// renderer and by `memcpy` into the cold tier, and nowhere else.
    pub fn as_slice(&self) -> &'a [u8] {
        self.raw
    }
}

impl Run {
    /// Set the encoding from the typed enum rather than casting at the call
    /// site. The field is `u8` because a fieldless enum cannot be `FromBytes`
    /// — a byte off disk is not necessarily a valid variant — so recovery goes
    /// through the checked [`Run::encoding`].
    pub fn with_encoding(mut self, e: Encoding) -> Run {
        self.encoding = e as u8;
        self
    }
}
