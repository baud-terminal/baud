use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Index into the style table. Two bytes rather than ten inline: cell size
/// drove throughput from 36% of the kernel ceiling to 80%.
#[derive(
    Clone, Copy, PartialEq, Eq, Default, Debug, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct StyleId(pub(crate) u16);

impl StyleId {
    pub const DEFAULT: StyleId = StyleId(0);
}

/// Character rendition bits.
///
/// No public field, no `From<u32>`, no `BitOr<u32>`. `Attrs` and [`LineFlags`]
/// are set types that would happily OR together as raw integers; here they
/// cannot meet. `Attrs` outgrew `LineFlags`' sixteen bits when the ideogram
/// aspects landed, which is a second reason they are not interchangeable.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Attrs(pub(crate) u32);

impl Attrs {
    pub const NONE: Attrs = Attrs(0);
    pub const BOLD: Attrs = Attrs(1 << 0);
    pub const DIM: Attrs = Attrs(1 << 1);
    pub const ITALIC: Attrs = Attrs(1 << 2);
    pub const UNDERLINE: Attrs = Attrs(1 << 3);
    pub const BLINK: Attrs = Attrs(1 << 4);
    pub const REVERSE: Attrs = Attrs(1 << 5);
    pub const HIDDEN: Attrs = Attrs(1 << 6);
    pub const STRIKE: Attrs = Attrs(1 << 7);
    /// `SGR 20`. Turned off by `SGR 23` along with italic, which is why
    /// `[[source: ecma48]]` § 8.3.117 names 23 "not italicized, not fraktur".
    pub const FRAKTUR: Attrs = Attrs(1 << 8);
    /// `SGR 26`, off by `SGR 50`.
    ///
    /// `[[source: ecma48]]` § 8.3.117 does not *define* 26 -- it reserves it,
    /// "for proportional spacing as specified in CCITT Recommendation T.61",
    /// and reserves 50 for cancelling it. baud records the bit and resolves
    /// nothing, which is what a reservation permits: spacing is a renderer's
    /// decision and T.61 is not a pinned source here.
    pub const PROPORTIONAL: Attrs = Attrs(1 << 9);
    /// `SGR 51`, off by `SGR 54`.
    pub const FRAMED: Attrs = Attrs(1 << 10);
    /// `SGR 52`, off by `SGR 54` — the same parameter clears both.
    pub const ENCIRCLED: Attrs = Attrs(1 << 11);
    /// `SGR 53`, off by `SGR 55`.
    pub const OVERLINED: Attrs = Attrs(1 << 12);
    /// `SGR 60`, off by `SGR 65`. `[[source: ecma48]]` § 8.3.117 calls it
    /// "ideogram underline or right side line" -- one aspect with two names,
    /// because which edge it marks depends on the writing direction.
    pub const IDEOGRAM_UNDERLINE: Attrs = Attrs(1 << 13);
    /// `SGR 61` -- "ideogram double underline or double line on the right
    /// side", off by `SGR 65`.
    pub const IDEOGRAM_DOUBLE_UNDERLINE: Attrs = Attrs(1 << 14);
    /// `SGR 62` -- "ideogram overline or left side line", off by `SGR 65`.
    pub const IDEOGRAM_OVERLINE: Attrs = Attrs(1 << 15);
    /// `SGR 63` -- "ideogram double overline or double line on the left side",
    /// off by `SGR 65`.
    pub const IDEOGRAM_DOUBLE_OVERLINE: Attrs = Attrs(1 << 16);
    /// `SGR 64` -- "ideogram stress marking", off by `SGR 65`.
    pub const IDEOGRAM_STRESS: Attrs = Attrs(1 << 17);
    /// `SGR 21` -- doubly underlined (ECMA-48 3rd, xterm) -- and `SGR 4:2`,
    /// which names the same state. A distinct bit from [`Attrs::UNDERLINE`]
    /// now that a renderer draws it as two lines; the note that a second bit
    /// "no renderer reads" would be the set-and-never-used failure held only
    /// until one did. Distinct, but in [`Attrs::UNDERLINES`]: `21` replaces
    /// the family like every other style.
    pub const DOUBLE_UNDERLINE: Attrs = Attrs(1 << 18);
    /// `SGR 4:3` -- curly underline, `[[source: kitty-underlines]]`; what
    /// every language server draws a diagnostic with. Kitty's extension,
    /// adopted by xterm, wezterm and foot.
    pub const CURLY_UNDERLINE: Attrs = Attrs(1 << 19);
    /// `SGR 4:4` -- dotted underline.
    pub const DOTTED_UNDERLINE: Attrs = Attrs(1 << 20);
    /// `SGR 4:5` -- dashed underline.
    pub const DASHED_UNDERLINE: Attrs = Attrs(1 << 21);

    /// **The five underline styles are one family and exactly one is in
    /// force**, so a `4:n` -- or a bare `4`, or `21` -- replaces the family
    /// rather than adding a bit, and `24` (`4:0`) clears all five. Named so the clear cannot drift from the
    /// set, the shape `IDEOGRAM` has for `SGR 65`.
    pub const UNDERLINES: Attrs = Attrs(
        Attrs::UNDERLINE.0
            | Attrs::DOUBLE_UNDERLINE.0
            | Attrs::CURLY_UNDERLINE.0
            | Attrs::DOTTED_UNDERLINE.0
            | Attrs::DASHED_UNDERLINE.0,
    );

    /// The five aspects `SGR 65` cancels together.
    ///
    /// `[[source: ecma48]]` § 8.3.117 defines 65 as cancelling "the effect of
    /// the rendition aspects established by parameter values 60 to 64" -- one
    /// parameter clearing five bits, the same shape as 54 clearing framed and
    /// encircled. A named constant rather than five `without` calls so the
    /// cancel cannot drift out of step with the set.
    pub const IDEOGRAM: Attrs = Attrs(
        Attrs::IDEOGRAM_UNDERLINE.0
            | Attrs::IDEOGRAM_DOUBLE_UNDERLINE.0
            | Attrs::IDEOGRAM_OVERLINE.0
            | Attrs::IDEOGRAM_DOUBLE_OVERLINE.0
            | Attrs::IDEOGRAM_STRESS.0,
    );

    /// Each attribute with the SGR parameter that sets it, in ascending order.
    ///
    /// A table rather than a `match` because DECRQSS has to walk *all* of them
    /// to report the current rendition, and a `match` per bit is the shape that
    /// silently omits one -- which is how `fold` came to discard every
    /// attribute except reset and foreground.
    pub const WIRE: [(Attrs, u8); 22] = [
        (Attrs::BOLD, 1),
        (Attrs::DIM, 2),
        (Attrs::ITALIC, 3),
        (Attrs::UNDERLINE, 4),
        (Attrs::BLINK, 5),
        (Attrs::REVERSE, 7),
        (Attrs::HIDDEN, 8),
        (Attrs::STRIKE, 9),
        (Attrs::FRAKTUR, 20),
        (Attrs::DOUBLE_UNDERLINE, 21),
        // The three colon styles have no single-parameter spelling; `emit_sgr`
        // writes them as `4:n`, keyed on these placeholder codes. They are in
        // `WIRE` so that DECRQSS walks them like every other bit and so
        // `CAPACITY` grows with them.
        (Attrs::CURLY_UNDERLINE, 3),
        (Attrs::DOTTED_UNDERLINE, 4),
        (Attrs::DASHED_UNDERLINE, 5),
        (Attrs::PROPORTIONAL, 26),
        (Attrs::FRAMED, 51),
        (Attrs::ENCIRCLED, 52),
        (Attrs::OVERLINED, 53),
        (Attrs::IDEOGRAM_UNDERLINE, 60),
        (Attrs::IDEOGRAM_DOUBLE_UNDERLINE, 61),
        (Attrs::IDEOGRAM_OVERLINE, 62),
        (Attrs::IDEOGRAM_DOUBLE_OVERLINE, 63),
        (Attrs::IDEOGRAM_STRESS, 64),
    ];

    /// Ten bits of the thirty-two are still free.
    ///
    /// **Derived from [`Attrs::WIRE`], not typed.** The constant read fourteen
    /// and this line thirteen while twenty-two bits were taken, wrong both
    /// ways; `the_free_bit_count_is_what_the_defined_bits_leave` counts the
    /// bits this file defines independently of `WIRE`, so a bit left out of
    /// the table fails there, and holds this line to the number.
    ///
    /// The ideogram aspects needed five and there were three, so `Attrs` went
    /// to `u32` and `StyleWord` to twenty bytes -- a cost the rendition table
    /// pays per slot (it is sized to the screen, between `NSTYLE` and
    /// `NSTYLE_MAX` slots), and nothing at all on the cell, which holds a
    /// `StyleId` index rather than a rendition.
    ///
    /// This bit was previously spent on an argument that the aspects were
    /// out "because nothing sends them", while `docs/vt-coverage.md` gave a
    /// *different* reason -- that no readable primary source defined them.
    /// Both were wrong: CJK terminals do send them, and the source read
    /// perfectly once searched with its own letter-spacing removed. Two
    /// reasons for one refusal is the signal that neither had been checked.
    const _FREE_BITS: u32 = u32::BITS - Attrs::wire_union().count_ones();

    /// Every bit `WIRE` names, OR-ed. A `const fn` so the count above is
    /// computed by the compiler, and so the assertion below can refuse two
    /// entries that share a bit.
    const fn wire_union() -> u32 {
        let mut u = 0;
        let mut i = 0;
        while i < Attrs::WIRE.len() {
            u |= Attrs::WIRE[i].0.0;
            i += 1;
        }
        u
    }

    /// The raw set, for hashing a rendition.
    pub fn bits(self) -> u32 {
        self.0
    }
    pub const fn with(self, o: Attrs) -> Attrs {
        Attrs(self.0 | o.0)
    }
    pub const fn without(self, o: Attrs) -> Attrs {
        Attrs(self.0 & !o.0)
    }
    pub fn has(self, o: Attrs) -> bool {
        self.0 & o.0 == o.0
    }
    // No `as_bits`: `Attrs` is `repr(transparent)` over `u32`, so it can be
    // stored in a `repr(C)` rendition directly. Unpacking it was the only
    // reason a raw integer used to escape.
}

// Two `WIRE` entries on one bit would make the derived free count too high
// and DECRQSS name one attribute twice.
const _: () = assert!(
    Attrs::wire_union().count_ones() as usize == Attrs::WIRE.len(),
    "two `WIRE` entries share a bit"
);

/// SGR colour. `Indexed` and `Rgb` are different kinds — `38;5;n` and
/// `38;2;r;g;b` produce values that must not be confused.
/// A palette index or an sRGB component. Eight bits by protocol, not by
/// convenience — SGR encodes both as a single decimal parameter.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Chan(pub(crate) u8);

impl Chan {
    /// Saturating: `CSI 38;2;999;0;0m` is malformed input, not an overflow.
    pub fn from_param(p: Option<Param>) -> Chan {
        Chan(p.map_or(0, |x| x.0.min(255)) as u8)
    }
    pub const fn from_offset(v: u16, base: u16) -> Chan {
        Chan(v.saturating_sub(base) as u8)
    }
    pub fn get(self) -> u8 {
        self.0
    }
    /// A channel that is already eight bits wide -- from an X11 colour spec
    /// rather than from a CSI parameter.
    pub fn from_u8(v: u8) -> Chan {
        Chan(v)
    }
    /// A channel from a unit fraction -- an alpha written as `0.22` rather
    /// than as `56`, which is what a config and an AppKit colour both
    /// carry.
    ///
    /// **Rounded, not truncated**, and clamped before it is scaled: 0.22
    /// of 255 is 56.1, and the floor of a fraction is a different colour
    /// from the nearest one.
    ///
    /// Half-up by `+ 0.5` rather than by `round`, which is libm's and this
    /// crate is `no_std`. They agree here and only here: the operand is
    /// clamped to 0..=1 first, so it is never negative, which is the one
    /// case where truncating a half-up differs from rounding to nearest.
    ///
    /// A NaN gives zero -- `clamp` returns the NaN, and `as` saturates it
    /// -- which is the safe end for an opacity. Pinned, rather than left
    /// to be rediscovered.
    pub fn from_unit(v: f32) -> Chan {
        Chan((v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
    }
    /// As a palette subscript. The palette has exactly 256 entries because a
    /// `Chan` has exactly 256 values, so this cannot be out of range.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Default,
    Indexed(Chan),
    Rgb(Chan, Chan, Chan),
}

impl StyleId {
    /// Direct-mapped bucket for a packed style word.
    ///
    /// Knuth multiplicative hashing: take the **high** bits of the product, not
    /// the low ones. `& (buckets - 1)` on the low bits is what the mixing
    /// constant fails to disturb — SGR colours are multiples of `0x101010`, so
    /// their products shared low bits and collided. Measured before: 8 distinct
    /// styles produced 112,144 interns at a 51.6% hit rate.
    pub fn bucket(raw: StyleWord, buckets: usize) -> usize {
        debug_assert!(buckets.is_power_of_two());
        // 64-bit now that a rendition is more than a colour, but the same
        // lesson: take the HIGH bits. The 32-bit constant is replaced by the
        // 64-bit Fibonacci one for the wider key.
        let shift = 64 - buckets.trailing_zeros();
        (raw.key().wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
    }
    pub fn from_index(i: usize) -> StyleId {
        StyleId(i as u16)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// **How many writes the rendition table has taken** since it was allocated:
/// every slot filled and every slot evicted, in order. A reader that resolved
/// the table up to one count asks for the slots written since, and the table
/// never frees a slot, so while the count is no more than the table's length
/// the writes are exactly the live slots.
///
/// Zero is a table nothing has written; slot 0's reservation is write 1.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct StyleWrites(u64);

/// What a reader behind the log replays.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Replay {
    /// The log's entries at these index ranges, oldest first; the second
    /// range is the wrap past the log's end, empty when there is none.
    Entries([core::ops::Range<usize>; 2]),
    /// More writes than the log holds: every slot may have changed.
    Every,
}

impl StyleWrites {
    pub const NEVER: StyleWrites = StyleWrites(0);

    /// The count after one more write.
    pub fn next(self) -> StyleWrites {
        StyleWrites(self.0 + 1)
    }

    /// Where write number `self` sits in a log of `len` entries, a power of
    /// two: write 1 at index 0.
    pub fn index_in(self, len: usize) -> usize {
        debug_assert!(len.is_power_of_two() && self.0 > 0);
        (self.0 - 1) as usize & (len - 1)
    }

    /// Whether the table had taken more writes than it has slots, `len`: a
    /// table fills each slot once and never frees one, so any write past its
    /// length evicted a slot a cell may still name.
    pub fn past(self, len: usize) -> bool {
        self.0 > len as u64
    }

    /// The writes after `since` up to `self` in a log of `len` entries: the
    /// index ranges to replay, or [`Replay::Every`] when the reader is more
    /// than a log's length behind -- the ring has overwritten what it missed.
    /// A reader ahead of the log, which only a reader of another table can
    /// be, is told `Every` too.
    pub fn replay(self, since: StyleWrites, len: usize) -> Replay {
        if since > self || self.0 - since.0 > len as u64 {
            return Replay::Every;
        }
        let n = (self.0 - since.0) as usize;
        if n == 0 {
            return Replay::Entries([0..0, 0..0]);
        }
        let start = since.next().index_in(len);
        if start + n <= len {
            Replay::Entries([start..start + n, 0..0])
        } else {
            Replay::Entries([start..len, 0..start + n - len])
        }
    }
}

/// **Which rendition table this is.** A reader that caches what it resolved
/// keeps the serial it resolved against, and a table allocated where another
/// was freed is still a different table. Minted once per table, from one
/// counter for the process.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TableSerial(u64);

impl TableSerial {
    /// A serial no table in this process has had.
    pub fn mint() -> TableSerial {
        static NEXT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(1);
        TableSerial(NEXT.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

/// **Moves when a colour a rendition resolves through changes**: a palette
/// entry (`OSC 4`, `OSC 104`) or the default foreground or background (`OSC
/// 10`/`11`, `110`/`111`), and never for a write that leaves the value as it
/// was. A reader that resolved the table under one epoch resolves it again
/// under another; nothing else it depends on lives in the core.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ColorEpoch(u32);

impl ColorEpoch {
    pub fn next(self) -> ColorEpoch {
        ColorEpoch(self.0.wrapping_add(1))
    }
}

/// One colour slot: default, a palette index, or 24-bit RGB.
///
/// A tagged `u32` rather than an enum so the whole rendition stays a plain
/// zeroable block -- `Storage` is `new_box_zeroed`, and all-zero must be a valid
/// default rather than a colour that happens to look like one.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct ColorWord(pub(crate) u32);

impl ColorWord {
    const TAG_INDEXED: u32 = 0x0100_0000;
    const TAG_RGB: u32 = 0x0200_0000;
    /// Zero, so a zeroed `Storage` starts with default colours.
    pub const DEFAULT: ColorWord = ColorWord(0);
    pub fn indexed(n: Chan) -> ColorWord {
        ColorWord(Self::TAG_INDEXED | n.get() as u32)
    }
    pub fn rgb(r: Chan, g: Chan, b: Chan) -> ColorWord {
        ColorWord(
            Self::TAG_RGB | ((r.get() as u32) << 16) | ((g.get() as u32) << 8) | b.get() as u32,
        )
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn is_default(self) -> bool {
        self.0 == 0
    }
    /// The RGB triple, when this word holds one. `None` for the default colour
    /// and for palette indices, neither of which names a pixel value on their
    /// own.
    /// The palette index, when this word holds one.
    pub fn as_index(self) -> Option<u8> {
        (self.0 & Self::TAG_INDEXED != 0).then_some(self.0 as u8)
    }
    pub fn as_rgb(self) -> Option<(u8, u8, u8)> {
        (self.0 & Self::TAG_RGB != 0).then_some((
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
            self.0 as u8,
        ))
    }
    /// The parsed SGR colour, packed.
    pub fn from_color(c: Color) -> ColorWord {
        match c {
            Color::Default => ColorWord::DEFAULT,
            Color::Indexed(n) => ColorWord::indexed(n),
            Color::Rgb(r, g, b) => ColorWord::rgb(r, g, b),
        }
    }
}

/// A packed rendition: foreground, background, and attribute bits, as SGR
/// accumulates it. Interned into a [`StyleId`]; never stored per cell.
///
/// This used to be a bare `u32` holding only the foreground, with the result
/// that `SgrAttr::fold` had nowhere to put bold, underline, reverse or any
/// background colour -- so it dropped them. They were parsed and discarded,
/// which is the failure `docs/vt-coverage.md` calls `parsed`, at the scale of
/// every attribute a terminal has.
#[repr(C)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct StyleWord {
    fg: ColorWord,
    bg: ColorWord,
    /// `SGR 58` — the underline's own colour, defaulting to "follow the
    /// foreground".
    ///
    /// **This word is twenty bytes, not eight.** `PackedCell` is the
    /// eight-byte one, and it holds a `StyleId` *index* into the template
    /// table rather than a rendition — so each time this grows it costs
    /// `NSTYLE` times the growth (512 bytes in a 24 MB `Storage` across both
    /// widenings) and costs the cell nothing. The row that listed 58/59 as out
    /// said the rendition word had no room; it was the cell that had no room,
    /// and the cell was never being asked.
    underline: ColorWord,
    attrs: Attrs,
    /// `SGR 10-19` — which font the text is set in.
    ///
    /// **This word had sixteen bytes of explicit padding and the font row said
    /// fonts needed cell bits that did not exist.** They do not need cell bits:
    /// a font is a *rendition*, and rendition lives here, interned, where two
    /// cells in the same font are one style. `PackedCell` never had to grow.
    ///
    /// DECSCA protection deliberately does **not** live here.
    /// `[[source: vt510-decsca]]`: "DECSCA does not effect visual character
    /// attributes set by the select graphic rendition (SGR) function" -- it is
    /// not a rendition, so it is not in the rendition. It is a [`CellFlags`]
    /// bit on the cell instead, which also keeps it out of the interning key:
    /// protected and unprotected text of the same colour are one style, not
    /// two, and the 64-entry template cache is not halved by turning DECSCA on.
    font: FontId,
    /// Explicit, because `zerocopy`'s `IntoBytes` rejects implicit padding and
    /// widening `attrs` to `u32` left two bytes of it. The same reason
    /// `Run::_pad` exists: uninitialised tail bytes leak stack across any
    /// shm or disk boundary.
    _pad: u16,
}

/// Which of the ten fonts `[[source: ctlseqs]]` gives SGR the text is set in.
///
/// `SGR 10` is the primary font and `SGR 11`-`19` the nine alternates. baud
/// records the selection and does not resolve it: choosing a face is a
/// renderer's job, and there is no font machinery in a `no_std` core with no
/// allocator. Recording it is the channel -- the same one charset designations
/// and `?1001` use -- and it is read wherever a cell's rendition is read.
///
/// A newtype rather than a bare `u16`, and `repr(transparent)` so it adds its
/// two bytes and no more.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct FontId(u16);

impl FontId {
    /// `SGR 10`. Zero, so a zeroed style table is the primary font.
    pub const PRIMARY: FontId = FontId(0);

    /// From an SGR parameter in `10..=19`, or `None` for anything else.
    /// `const` so the single-parameter `SGR` table can be built at compile
    /// time; it is a pure range test either way.
    pub const fn from_sgr(n: u16) -> Option<FontId> {
        match n >= 10 && n <= 19 {
            true => Some(FontId(n - 10)),
            false => None,
        }
    }

    /// `0` for the primary font, `1..=9` for the alternates.
    pub fn index(self) -> u16 {
        self.0
    }

    /// The SGR parameter that selects it, for DECRQSS.
    ///
    /// A `u8` because the range is `10..=19` by construction, so the caller
    /// does not need a cast -- and `baud-core`'s budget is a floor rather than
    /// slack, so one there would have to displace something forced.
    pub fn sgr(self) -> u8 {
        (self.0 + 10) as u8
    }

    pub fn is_primary(self) -> bool {
        self.0 == 0
    }
}

impl StyleWord {
    /// All zero, so `Storage::new_box_zeroed` starts every template slot at the
    /// default rendition rather than at an arbitrary grey.
    pub const DEFAULT: StyleWord = StyleWord {
        fg: ColorWord::DEFAULT,
        bg: ColorWord::DEFAULT,
        underline: ColorWord::DEFAULT,
        attrs: Attrs::NONE,
        font: FontId::PRIMARY,
        _pad: 0,
    };
    /// The underline's own colour. `ColorWord::DEFAULT` means "follow the
    /// foreground", which is what every terminal does without `SGR 58`.
    pub fn underline(self) -> ColorWord {
        self.underline
    }
    pub fn with_underline(self, c: ColorWord) -> StyleWord {
        StyleWord {
            underline: c,
            ..self
        }
    }
    pub fn fg(self) -> ColorWord {
        self.fg
    }
    pub fn bg(self) -> ColorWord {
        self.bg
    }
    /// Which font `SGR 10-19` selected.
    pub fn font(self) -> FontId {
        self.font
    }
    pub fn with_font(self, f: FontId) -> StyleWord {
        StyleWord { font: f, ..self }
    }
    pub fn attrs(self) -> Attrs {
        self.attrs
    }
    pub fn with_fg(self, c: ColorWord) -> StyleWord {
        StyleWord { fg: c, ..self }
    }
    pub fn with_bg(self, c: ColorWord) -> StyleWord {
        StyleWord { bg: c, ..self }
    }
    pub fn with_attrs(self, a: Attrs) -> StyleWord {
        StyleWord { attrs: a, ..self }
    }

    /// In-place setters, for [`SgrAttr::fold_into`].
    ///
    /// The `with_*` forms above copy the whole twenty-byte word to change four
    /// bytes of it, and SGR handling is roughly **half** of `asciiquarium.raw`
    /// by `opcost`. A rendition is folded once per attribute and then assigned
    /// back, so the by-value form moved sixty bytes to set one field.
    pub fn set_fg(&mut self, c: ColorWord) {
        self.fg = c;
    }
    pub fn set_bg(&mut self, c: ColorWord) {
        self.bg = c;
    }
    pub fn set_attrs(&mut self, a: Attrs) {
        self.attrs = a;
    }
    pub fn set_font(&mut self, f: FontId) {
        self.font = f;
    }
    pub fn set_underline(&mut self, c: ColorWord) {
        self.underline = c;
    }
    /// The fields that distinguish one rendition from another, folded into one
    /// integer for hashing.
    ///
    /// **`underline` and `font` are deliberately not in it**, and this used to
    /// claim they were ("everything that distinguishes one rendition from
    /// another") after they were added to the word and not to the fold. Two
    /// renditions differing only in underline colour or font therefore hash to
    /// the same bucket.
    ///
    /// That is a hit-rate cost, not a correctness bug: `intern_style` compares
    /// the whole `StyleWord` on a hit, so a collision evicts and rebuilds
    /// rather than returning the wrong style. Folding them in is a plausible
    /// improvement and is **unmeasured** -- both bench payloads report `styles
    /// 0 interned / 100% hits`, so nothing here can currently tell the two
    /// versions apart. It needs a payload that cycles renditions faster than
    /// `NSTYLE` before it is worth changing.
    /// The bucketing key. **Every field, mixed rather than overlapped.**
    ///
    /// This was `fg ^ (bg << 21) ^ (attrs << 45)`, which had two faults. The
    /// three ranges *overlap* -- `fg` is 32 bits and `bg << 21` starts inside
    /// it -- so unrelated renditions folded onto each other. And `underline`
    /// and `font` were not in it at all, so two styles differing only in
    /// underline colour or font were guaranteed the same bucket and evicted
    /// each other on every write.
    ///
    /// A collision is not a correctness bug -- `intern_style` compares the
    /// whole `StyleWord`, not the key -- it is a *thrash*: the table is
    /// direct-mapped and evicts, so two hot styles sharing a bucket miss every
    /// time they alternate. `asciiquarium.raw` emits **13** distinct SGR
    /// parameter strings and was interning 184,463 times against 911,587 hits.
    ///
    /// Multiply-then-xor per field, so every input bit reaches the high bits
    /// that [`StyleId::bucket`] takes.
    pub fn key(self) -> u64 {
        // `fg` and `bg` are laid side by side, **not overlapped**, and the
        // low-entropy tail is mixed once and xored in.
        //
        // The shape matters as much as the coverage. `StyleId::bucket`
        // multiplies this by the Fibonacci constant and takes the high bits,
        // which spreads a *smoothly varying* input almost perfectly -- that is
        // why a 200-colour truecolour ramp lands in 200 distinct buckets. A
        // hash that multiplies `fg` itself destroys that structure and gives
        // ordinary birthday collisions instead: an earlier attempt here put 13
        // of those 200 cells in the wrong colour, which
        // `a_full_style_table_makes_cells_report_the_wrong_colour` caught.
        //
        // So `fg | bg << 32` is left alone, and `underline`, `attrs` and
        // `font` -- which are the default on essentially all text -- are
        // multiplied together into a constant that shifts the whole space.
        const M: u64 = 0xD6E8_FEB8_6659_FD93;
        let base = u64::from(self.fg.0) | (u64::from(self.bg.0) << 32);
        let tail = u64::from(self.underline.0)
            | (u64::from(self.attrs.bits()) << 32)
            | (u64::from(self.font.0) << 48);
        base ^ tail.wrapping_mul(M)
    }
}

/// How many bytes a [`StyleWord`] is, for the fold below.
const STYLE_BYTES: usize = core::mem::size_of::<StyleWord>();

/// The bit pattern half of a [`StyleFold`].
///
/// A newtype rather than a bare array because the two halves mean different
/// things -- which bits the fold *forces* and what it forces them *to* -- and
/// passing them the wrong way round would compile and produce a plausible
/// rendition.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct StyleBits([u8; STYLE_BYTES]);

/// A rendition change as a **masked replace**, which is what every
/// [`SgrAttr`] arm already is: `Reset` replaces the whole word, `Set` and
/// `Unset` force attribute bits, and the colour and font arms replace a
/// field. None of them reads the bits it writes.
///
/// Masked replaces **compose** -- applying `(m1,v1)` then `(m2,v2)` is
/// `(m1|m2, (v1 & !m2) | v2)` -- so an entire `SGR` sequence, however many
/// parameters it carries, collapses to one pair that is a pure function of
/// the sequence's bytes. That is what makes a repeated `SGR` answerable from
/// a table keyed on those bytes, without the parameter walk or the parse.
///
/// **The pair is derived, not hand-written.** `derive` runs the fold on two
/// witnesses whose bytes differ in *every* bit, so a bit the fold forces comes
/// out equal on both and a bit it passes through comes out different:
/// `mask = !(a ^ b)`. Nothing here needs to know a field's offset, which is
/// the part a hand-written mask would get wrong the next time `StyleWord`
/// grows -- and it has grown twice.
///
/// Both witnesses are valid: `StyleWord` is `FromBytes`, so every bit pattern
/// is one.
#[repr(C)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct StyleFold {
    mask: StyleBits,
    value: StyleBits,
}

impl StyleFold {
    // `as_mut_bytes` comes from `IntoBytes`.

    /// Changes nothing.
    pub const IDENTITY: StyleFold = StyleFold {
        mask: StyleBits([0; STYLE_BYTES]),
        value: StyleBits([0; STYLE_BYTES]),
    };

    /// The masked replace equivalent to running `f`.
    ///
    /// Only sound for an `f` that is one -- every `SgrAttr` is, and
    /// `a_fold_is_a_masked_replace_of_the_style_word` checks that claim
    /// against `fold_into` over every attribute and a spread of start words.
    pub fn derive<F: Fn(&mut StyleWord)>(f: F) -> StyleFold {
        let mut a = StyleWord::DEFAULT;
        let mut b = StyleWord::DEFAULT;
        b.as_mut_bytes().fill(0xff);
        f(&mut a);
        f(&mut b);
        let mut mask = [0u8; STYLE_BYTES];
        let mut value = [0u8; STYLE_BYTES];
        for (i, (x, y)) in a.as_bytes().iter().zip(b.as_bytes().iter()).enumerate() {
            mask[i] = !(x ^ y);
            value[i] = x & mask[i];
        }
        StyleFold {
            mask: StyleBits(mask),
            value: StyleBits(value),
        }
    }

    /// Apply it in place.
    ///
    /// **Word-sized, not byte-sized.** The first version looped over the
    /// twenty bytes one at a time and that was most of what made the table
    /// slower than the parameter walk it replaces: sixty operations to do
    /// what three `u64`s do in nine. `STYLE_BYTES` is checked against the
    /// chunking below so the next widening of `StyleWord` cannot silently
    /// leave a field unfolded.
    #[inline(always)]
    pub fn apply(self, cur: &mut StyleWord) {
        let dst = cur.as_mut_bytes();
        let (m, v) = (&self.mask.0, &self.value.0);
        for i in (0..STYLE_BYTES - STYLE_BYTES % 8).step_by(8) {
            let mut w = [0u8; 8];
            w.copy_from_slice(&dst[i..i + 8]);
            let mut mw = [0u8; 8];
            mw.copy_from_slice(&m[i..i + 8]);
            let mut vw = [0u8; 8];
            vw.copy_from_slice(&v[i..i + 8]);
            let out = (u64::from_le_bytes(w) & !u64::from_le_bytes(mw)) | u64::from_le_bytes(vw);
            dst[i..i + 8].copy_from_slice(&out.to_le_bytes());
        }
        for i in STYLE_BYTES - STYLE_BYTES % 8..STYLE_BYTES {
            dst[i] = (dst[i] & !m[i]) | v[i];
        }
    }
}

#[cfg(test)]
mod attrs_free_bits {
    use crate::*;

    /// The bits `Attrs` defines, read off this file's own `Attrs(1 << n)`
    /// lines rather than off `WIRE`, so a bit added to the type and left out
    /// of `WIRE` is still counted -- and then caught below. The union, and
    /// how many lines named a bit: the two disagree when two lines share one.
    fn defined() -> (u32, u32) {
        const SRC: &str = include_str!("style.rs");
        // Spelled apart, so this function's own source does not match.
        let marker = concat!(": Attrs = Attrs(", "1 << ");
        let (mut union, mut lines) = (0u32, 0u32);
        for (_, rest) in SRC.lines().filter_map(|l| l.split_once(marker)) {
            let n = rest.split(')').next().expect("a closing paren");
            let n: u32 = n.trim().parse().expect("a literal shift");
            assert!(n < u32::BITS, "bit {n} is past the word");
            union |= 1 << n;
            lines += 1;
        }
        (union, lines)
    }

    /// **The free-bit count is derived, and the doc above it says it.** It
    /// read fourteen in the constant and thirteen in the doc while 22 of the
    /// 32 bits were taken, wrong both ways, because both were typed.
    #[test]
    fn the_free_bit_count_is_what_the_defined_bits_leave() {
        let (union, lines) = defined();
        assert_eq!(
            union & 0xff,
            0xff,
            "the control: `BOLD` to `STRIKE` are bits 0-7, and the reader must see them"
        );
        assert_eq!(union.count_ones(), lines, "two attributes share a bit");
        assert_eq!(Attrs::_FREE_BITS, u32::BITS - union.count_ones());

        // Every defined bit is in `WIRE`, which is what DECRQSS walks.
        for b in (0..u32::BITS).filter(|b| union & (1 << b) != 0) {
            assert!(
                Attrs::WIRE.iter().any(|(a, _)| a.bits() == 1 << b),
                "bit {b} is defined and not in `WIRE`, so DECRQSS cannot report it"
            );
        }

        // The doc's first word names the count.
        const SRC: &str = include_str!("style.rs");
        let before = SRC
            .split_once(concat!("const _FREE_", "BITS"))
            .map(|(head, _)| head)
            .expect("the constant");
        let doc = before
            .lines()
            .rev()
            // The indentation before `const`, on the constant's own line.
            .skip(1)
            .take_while(|l| l.trim_start().starts_with("///"))
            .last()
            .expect("a doc on the constant");
        const WORDS: &str = "No One Two Three Four Five Six Seven Eight Nine Ten Eleven Twelve \
            Thirteen Fourteen Fifteen Sixteen Seventeen Eighteen Nineteen Twenty Twenty-one \
            Twenty-two Twenty-three Twenty-four Twenty-five Twenty-six Twenty-seven \
            Twenty-eight Twenty-nine Thirty Thirty-one Thirty-two";
        let want = WORDS
            .split_whitespace()
            .nth(Attrs::_FREE_BITS as usize)
            .expect("a word for every count");
        assert!(
            doc.trim_start()
                .trim_start_matches('/')
                .trim_start()
                .starts_with(want),
            "the doc should open with {want:?}: {doc}"
        );
    }
}

#[cfg(test)]
mod style_fold {
    use crate::*;
    use zerocopy::IntoBytes;

    /// **Every `SgrAttr` is a masked replace, and this is the claim the SGR
    /// table rests on.** If one arm ever reads a bit it writes, the derived
    /// pair silently produces a different rendition -- so it is checked
    /// against `fold_into` itself, over every attribute the grammar can
    /// produce and a spread of starting words including both witnesses.
    #[test]
    fn a_fold_is_a_masked_replace_of_the_style_word() {
        let mut starts = [StyleWord::DEFAULT; 66];
        starts[1].as_mut_bytes().fill(0xff);
        // A spread of ordinary renditions, so a pass-through bit that happens
        // to agree on the two witnesses would still be caught.
        for (seed, w) in starts.iter_mut().enumerate().skip(2) {
            for (i, b) in w.as_mut_bytes().iter_mut().enumerate() {
                let h = (seed as u32)
                    .wrapping_mul(2_654_435_761)
                    .wrapping_add(i as u32);
                *b = (h >> 13) as u8;
            }
        }

        let mut checked = 0usize;
        for v in 0u16..=200 {
            let Some(attr) = SgrAttr::parse_one(v) else {
                continue;
            };
            let fold = StyleFold::derive(|w| attr.fold_into(w));
            for s in starts.iter() {
                let (mut want, mut got) = (*s, *s);
                attr.fold_into(&mut want);
                fold.apply(&mut got);
                assert_eq!(want, got, "SGR {v} from {s:?}");
            }
            checked += 1;
        }
        // The count is asserted so the test cannot quietly stop reaching the
        // grammar -- a probe that cannot fire looks exactly like a pass.
        // 37: the thirty-six the fast path always answered, plus `4` since the
        // colon underline styles landed -- a bare `4` became a family replace
        // (`UnderlineStyle`) so the fast path has to fold it the same way the
        // grammar does, and a masked replace over `UNDERLINES` derives cleanly.
        assert_eq!(checked, 37, "single-parameter SGR values in 0..=200");
    }

    /// Composition: two folds applied in turn must equal the pair that
    /// composes them, which is what lets one entry stand for a whole sequence.
    #[test]
    fn folds_compose() {
        for a in [0u16, 1, 31, 44, 22, 39] {
            for b in [0u16, 7, 34, 49, 24, 1] {
                let (Some(x), Some(y)) = (SgrAttr::parse_one(a), SgrAttr::parse_one(b)) else {
                    continue;
                };
                let both = StyleFold::derive(|w| {
                    x.fold_into(w);
                    y.fold_into(w);
                });
                let mut want = StyleWord::DEFAULT;
                x.fold_into(&mut want);
                y.fold_into(&mut want);
                let mut got = StyleWord::DEFAULT;
                both.apply(&mut got);
                assert_eq!(want, got, "SGR {a} then {b}");
            }
        }
    }
}

/// How many bytes of an `SGR` sequence a [`SgrKey`] holds.
///
/// **Measured, not guessed.** The longest plain `SGR` in the corpus is
/// `ESC [ 38 ; 2 ; 204 ; 204 ; 204 m` at 17 bytes after the `ESC [`, and
/// `btop.raw` sends it 4,943 times a rep. Twenty-four covers every recording
/// with room, and pads to three `u64` reads so the hash touches every byte
/// without a loop.
pub const SGR_KEY_BYTES: usize = 24;

/// The bytes of a plain `SGR` sequence, after the `ESC [` and including the
/// final `m`, zero-padded.
///
/// **The key is the sequence itself**, which is what makes the answer to it a
/// pure function: parsing `ESC [ 38 ; 2 ; 204 ; 204 ; 204 m` cannot depend on
/// anything but those bytes. A shorter sequence pads with zeros, and zero is
/// not a byte any `SGR` contains, so two different sequences cannot share a
/// key.
#[repr(C)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct SgrKey {
    bytes: [u8; SGR_KEY_BYTES],
    /// Zero for the empty slot. A real sequence is at least `m`, so a length
    /// of zero can never match one and no separate occupancy flag is needed.
    len: u8,
    _pad: [u8; 7],
}

impl SgrKey {
    /// The slot a fresh table is full of, and a key nothing matches.
    pub const EMPTY: SgrKey = SgrKey {
        bytes: [0; SGR_KEY_BYTES],
        len: 0,
        _pad: [0; 7],
    };

    /// The key for a sequence, or `None` if it is longer than the table can
    /// hold. Declining is how a long sequence keeps the normal path rather
    /// than being answered wrongly.
    ///
    /// **Only the miss path builds one.** A lookup hashes and compares the
    /// bytes where they already are; materialising 32 bytes to ask a question
    /// is what made the first version of this table slower than the walk it
    /// replaces.
    pub fn of(seq: &[StreamByte]) -> Option<SgrKey> {
        match seq.len() <= SGR_KEY_BYTES && !seq.is_empty() {
            false => None,
            true => {
                let mut k = SgrKey::EMPTY;
                for (dst, src) in k.bytes.iter_mut().zip(seq.iter()) {
                    *dst = src.raw_byte();
                }
                k.len = seq.len() as u8;
                Some(k)
            }
        }
    }

    /// The slot a sequence hashes to, read straight from the stream.
    ///
    /// **Eight bytes and the length, not all twenty-four.** The distinct
    /// sequences number 168 at worst, and a truecolor `SGR`'s first eight
    /// bytes -- `38;2;204` against `48;2;0;0` -- already separate the families;
    /// what they do not separate, the full compare does. Reading three
    /// `u64`s to spread 168 keys was pure cost on the hit path.
    #[inline(always)]
    pub fn bucket_of(seq: &[StreamByte], buckets: usize) -> usize {
        debug_assert!(buckets.is_power_of_two());
        let mut w = [0u8; 8];
        for (dst, src) in w.iter_mut().zip(seq.iter()) {
            *dst = src.raw_byte();
        }
        let h =
            (u64::from_le_bytes(w) ^ (seq.len() as u64) << 56).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let shift = 64 - buckets.trailing_zeros();
        (h >> shift) as usize
    }

    /// Whether this key is that sequence, comparing only the bytes it has.
    #[inline(always)]
    pub fn matches(&self, seq: &[StreamByte]) -> bool {
        if self.len as usize != seq.len() {
            return false;
        }
        for (a, b) in self.bytes.iter().zip(seq.iter()) {
            if *a != b.raw_byte() {
                return false;
            }
        }
        true
    }
}

/// One entry: the sequence, and the rendition change it means.
#[repr(C)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct SgrSlot {
    pub key: SgrKey,
    pub fold: StyleFold,
}

impl SgrSlot {
    pub const EMPTY: SgrSlot = SgrSlot {
        key: SgrKey::EMPTY,
        fold: StyleFold::IDENTITY,
    };
}

/// The shortest `SGR` sequence worth answering from the table.
///
/// **A short sequence is already cheaper than any lookup**, and this constant
/// is the whole difference between the table being a win and a regression.
/// `ESC [ 4 0 m` is a three-byte parameter walk -- three iterations of a
/// predictable match -- which no hash, compare and masked replace can beat.
/// The first version consulted the table for every `SGR` and measured
/// **-34% on asciiquarium**, whose sequences are all this shape.
///
/// The corpus splits cleanly, which is why a threshold works at all. Plain
/// `SGR` length after the `ESC [`:
///
/// | recording | 1-6 bytes | 11-17 bytes |
/// |---|---|---|
/// | `btop.raw` | 25% | **75%** |
/// | `asciiquarium.raw` | **100%** | 0% |
/// | `git-log.raw` | **100%** | 0% |
/// | `less-pager.raw` | **100%** | 0% |
///
/// So eight excludes every recording whose sequences the walk already handles
/// well, and admits the truecolor `ESC [ 38 ; 2 ; r ; g ; b m` that is 45% of
/// `btop.raw`'s alone.
pub const SGR_MIN_BYTES: usize = 8;

/// The hash of a redraw segment, for `baud-core`'s `ceiling-segment-cache`
/// arm.
///
/// **Eight bytes a multiply, not one.** A byte-at-a-time FNV is one dependent
/// multiply per byte at three to four cycles of latency, which is *more* than
/// the parser costs per byte -- the first version of that arm measured btop
/// 935 -> 566 while skipping 81% of the cells, and it was pricing the hash
/// rather than the idea. Two accumulators, so the chain is two deep.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SegHash(u64);

impl SegHash {
    const SEED_A: u64 = 0xcbf2_9ce4_8422_2325;
    const SEED_B: u64 = 0x9E37_79B9_7F4A_7C15;
    const PRIME: u64 = 0x0100_0000_01b3;

    /// Nothing hashed. A real segment is at least one byte, so this cannot
    /// collide with one by accident often enough to matter in a ceiling arm.
    pub const ZERO: SegHash = SegHash(0);

    #[inline(always)]
    pub fn of(seg: &[StreamByte]) -> SegHash {
        let (mut a, mut b) = (Self::SEED_A, Self::SEED_B);
        let mut c = seg.chunks_exact(16);
        for q in &mut c {
            a = (a ^ Self::word(&q[..8])).wrapping_mul(Self::PRIME);
            b = (b ^ Self::word(&q[8..])).wrapping_mul(Self::PRIME);
        }
        let mut tail = [StreamByte::NUL; 16];
        for (d, x) in tail.iter_mut().zip(c.remainder().iter()) {
            *d = *x;
        }
        a = (a ^ Self::word(&tail[..8])).wrapping_mul(Self::PRIME);
        b = (b ^ Self::word(&tail[8..]) ^ seg.len() as u64).wrapping_mul(Self::PRIME);
        SegHash(a ^ b.rotate_left(32))
    }

    /// **One load, not eight byte copies.** `StreamByte` is
    /// `repr(transparent)` and `IntoBytes`, so eight of them *are* a `u64` in
    /// memory. Spelling that as a byte loop -- which the first typed version
    /// did -- took the arm from 664 to 363 on btop: a newtype is free when the
    /// shape matches, and a manual copy is not the shape of a load.
    #[inline(always)]
    fn word(b: &[StreamByte]) -> u64 {
        let mut w = [0u8; 8];
        w.copy_from_slice(&b.as_bytes()[..8]);
        u64::from_le_bytes(w)
    }

    /// High bits, the same rule as every other table here.
    pub fn slot(self, buckets: usize) -> usize {
        debug_assert!(buckets.is_power_of_two());
        (self.0 >> (64 - buckets.trailing_zeros())) as usize
    }
}
