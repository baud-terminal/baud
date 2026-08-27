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
/// No public field, no `From<u16>`, no `BitOr<u16>`. `Attrs` and [`LineFlags`]
/// are both 16-bit sets and would happily OR together as raw integers; here
/// they cannot meet.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Attrs(pub(crate) u16);

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
    pub const PROPORTIONAL: Attrs = Attrs(1 << 9);
    /// `SGR 51`, off by `SGR 54`.
    pub const FRAMED: Attrs = Attrs(1 << 10);
    /// `SGR 52`, off by `SGR 54` — the same parameter clears both.
    pub const ENCIRCLED: Attrs = Attrs(1 << 11);
    /// `SGR 53`, off by `SGR 55`.
    pub const OVERLINED: Attrs = Attrs(1 << 12);

    /// Each attribute with the SGR parameter that sets it, in ascending order.
    ///
    /// A table rather than a `match` because DECRQSS has to walk *all* of them
    /// to report the current rendition, and a `match` per bit is the shape that
    /// silently omits one -- which is how `fold` came to discard every
    /// attribute except reset and foreground.
    pub const WIRE: [(Attrs, u8); 13] = [
        (Attrs::BOLD, 1),
        (Attrs::DIM, 2),
        (Attrs::ITALIC, 3),
        (Attrs::UNDERLINE, 4),
        (Attrs::BLINK, 5),
        (Attrs::REVERSE, 7),
        (Attrs::HIDDEN, 8),
        (Attrs::STRIKE, 9),
        (Attrs::FRAKTUR, 20),
        (Attrs::PROPORTIONAL, 26),
        (Attrs::FRAMED, 51),
        (Attrs::ENCIRCLED, 52),
        (Attrs::OVERLINED, 53),
    ];

    /// Three bits of the sixteen are still free.
    ///
    /// `SGR 60-65` -- the ideogram attributes -- would need five, so widening
    /// `Attrs` to `u32` is what they cost, and that is `NSTYLE * 4` bytes: 256
    /// in a 24 MB `Storage`. They are out because nothing sends them, which is
    /// a scope decision, and this records that the *structure* is not the
    /// reason. Saying "there is no room" when there is would be the mistake
    /// that kept fonts and the underline colour out.
    const _FREE_BITS: u16 = 3;

    /// The raw set, for hashing a rendition.
    pub fn bits(self) -> u16 {
        self.0
    }
    pub fn with(self, o: Attrs) -> Attrs {
        Attrs(self.0 | o.0)
    }
    pub fn without(self, o: Attrs) -> Attrs {
        Attrs(self.0 & !o.0)
    }
    pub fn has(self, o: Attrs) -> bool {
        self.0 & o.0 == o.0
    }
    // No `as_bits`: `Attrs` is `repr(transparent)` over `u16`, so it can be
    // stored in a `repr(C)` cell directly. Unpacking it was the only reason a
    // raw `u16` used to escape.
}

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
    pub fn from_offset(v: u16, base: u16) -> Chan {
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
    /// As a palette subscript. The palette has exactly 256 entries because a
    /// `Chan` has exactly 256 values, so this cannot be out of range.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
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
    /// **This word is twelve bytes, not eight.** `PackedCell` is the eight-byte
    /// one, and it holds a `StyleId` *index* into the template table rather
    /// than a rendition — so growing this to sixteen costs `NSTYLE * 4` bytes,
    /// which is 256 in a 24 MB `Storage`, and costs the cell nothing. The row
    /// that listed 58/59 as out said the rendition word had no room; it was
    /// the cell that had no room, and the cell was never being asked.
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
}

/// Which of the ten fonts `[[source: ctlseqs]]` gives SGR the text is set in.
///
/// `SGR 10` is the primary font and `SGR 11`-`19` the nine alternates. baud
/// records the selection and does not resolve it: choosing a face is a
/// renderer's job, and there is no font machinery in a `no_std` core with no
/// allocator. Recording it is the channel -- the same one charset designations
/// and `?1001` use -- and it is read wherever a cell's rendition is read.
///
/// A newtype rather than a bare `u16`, and `repr(transparent)` so the word is
/// still eight bytes.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct FontId(u16);

impl FontId {
    /// `SGR 10`. Zero, so a zeroed style table is the primary font.
    pub const PRIMARY: FontId = FontId(0);

    /// From an SGR parameter in `10..=19`, or `None` for anything else.
    pub fn from_sgr(n: u16) -> Option<FontId> {
        (10..=19).contains(&n).then(|| FontId(n - 10))
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
    /// Everything that distinguishes one rendition from another, folded into
    /// one integer for hashing.
    pub fn key(self) -> u64 {
        (self.fg.0 as u64) ^ ((self.bg.0 as u64) << 21) ^ ((self.attrs.bits() as u64) << 45)
    }
}
