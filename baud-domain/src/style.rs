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
    pub const WIRE: [(Attrs, u8); 18] = [
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
        (Attrs::IDEOGRAM_UNDERLINE, 60),
        (Attrs::IDEOGRAM_DOUBLE_UNDERLINE, 61),
        (Attrs::IDEOGRAM_OVERLINE, 62),
        (Attrs::IDEOGRAM_DOUBLE_OVERLINE, 63),
        (Attrs::IDEOGRAM_STRESS, 64),
    ];

    /// Fourteen bits of the thirty-two are still free.
    ///
    /// The ideogram aspects needed five and there were three, so `Attrs` went
    /// to `u32` and `StyleWord` to twenty bytes -- `NSTYLE * 8` = 512 bytes in
    /// a 24 MB `Storage`, and nothing at all on the cell, which holds a
    /// `StyleId` index rather than a rendition.
    ///
    /// This bit was previously spent on an argument that the aspects were
    /// out "because nothing sends them", while `docs/vt-coverage.md` gave a
    /// *different* reason -- that no readable primary source defined them.
    /// Both were wrong: CJK terminals do send them, and the source read
    /// perfectly once searched with its own letter-spacing removed. Two
    /// reasons for one refusal is the signal that neither had been checked.
    const _FREE_BITS: u32 = 14;

    /// The raw set, for hashing a rendition.
    pub fn bits(self) -> u32 {
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
