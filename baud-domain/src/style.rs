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

    /// Each attribute with the SGR parameter that sets it, in ascending order.
    ///
    /// A table rather than a `match` because DECRQSS has to walk *all* of them
    /// to report the current rendition, and a `match` per bit is the shape that
    /// silently omits one -- which is how `fold` came to discard every
    /// attribute except reset and foreground.
    pub const WIRE: [(Attrs, u8); 8] = [
        (Attrs::BOLD, 1),
        (Attrs::DIM, 2),
        (Attrs::ITALIC, 3),
        (Attrs::UNDERLINE, 4),
        (Attrs::BLINK, 5),
        (Attrs::REVERSE, 7),
        (Attrs::HIDDEN, 8),
        (Attrs::STRIKE, 9),
    ];

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
    attrs: Attrs,
    /// Explicit, because zerocopy rejects implicit padding.
    ///
    /// DECSCA protection deliberately does **not** live here.
    /// `[[source: vt510-decsca]]`: "DECSCA does not effect visual character
    /// attributes set by the select graphic rendition (SGR) function" -- it is
    /// not a rendition, so it is not in the rendition. It is a [`CellFlags`]
    /// bit on the cell instead, which also keeps it out of the interning key:
    /// protected and unprotected text of the same colour are one style, not
    /// two, and the 64-entry template cache is not halved by turning DECSCA on.
    _pad: u16,
}

impl StyleWord {
    /// All zero, so `Storage::new_box_zeroed` starts every template slot at the
    /// default rendition rather than at an arbitrary grey.
    pub const DEFAULT: StyleWord = StyleWord {
        fg: ColorWord::DEFAULT,
        bg: ColorWord::DEFAULT,
        attrs: Attrs::NONE,
        _pad: 0,
    };
    pub fn fg(self) -> ColorWord {
        self.fg
    }
    pub fn bg(self) -> ColorWord {
        self.bg
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
