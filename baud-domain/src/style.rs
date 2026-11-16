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

/// **How far past one bucket of the rendition table the farthest rendition
/// homed there lies**, one a slot, beside the table. A probe for a
/// rendition homed at `h` that has read `reach[h] + 1` slots from `h`
/// without finding it has proved it is in no slot, so a miss on a table
/// with no empty slot reads that many and not the whole table. A
/// [`StyleId`] is two bytes, so a rendition lies at most 65,535 slots past
/// its bucket, which two bytes hold.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
#[repr(transparent)]
pub struct Reach(u16);

impl Reach {
    /// No rendition past its bucket, which is a new table's every slot.
    pub const ZERO: Reach = Reach(0);
    /// `n` slots past the bucket; `n` is below the table's length.
    pub fn of(n: usize) -> Reach {
        debug_assert!(
            n <= usize::from(u16::MAX),
            "a rendition lies within the table"
        );
        Reach(n as u16)
    }
    /// The slots a probe from the bucket reads to prove a miss.
    pub fn slots(self) -> usize {
        usize::from(self.0) + 1
    }
}

/// **Slots the rendition table's long probes have read**, a count that only
/// grows ([`SweepBook::probes`]).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Debug)]
pub struct SlotsRead(u64);

impl SlotsRead {
    /// The slots read since the count was `then`.
    pub fn since(self, then: SlotsRead) -> u64 {
        self.0 - then.0
    }
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
    /// `SGR 5`, "slowly blinking (less then 150 per minute)" in
    /// `[[source: ecma48]]` § 8.3.117. One of [`Attrs::BLINKS`].
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
    /// and reserves 50 for cancelling it. `[[source: itu-t61]]` says the text
    /// that follows "may be presented with proportional spacing at the
    /// recipient's option", so baud records the bit and every path draws the
    /// text at the cell pitch, as it draws it without 26.
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
    /// `SGR 6`, "rapidly blinking (150 per minute or more)" in
    /// `[[source: ecma48]]` § 8.3.117: its own state, drawn at its own rate,
    /// where it shared [`Attrs::BLINK`] and 5's slow rate until 2026-10-14.
    /// One of [`Attrs::BLINKS`].
    pub const RAPID_BLINK: Attrs = Attrs(1 << 22);

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

    /// **Slow and rapid blink are one aspect and exactly one is in force**,
    /// so `5` and `6` each replace the family and the later one wins, and
    /// `25`, "steady (not blinking)", clears both. `[[source: ecma48]]` §
    /// 8.3.117 gives the three as values of one rendition aspect, by rate,
    /// and leaves combinations to the implementation ("The usable
    /// combinations of parameter values are determined by the
    /// implementation"); the exclusive reading is taken.
    /// The shape [`Attrs::UNDERLINES`] has.
    pub const BLINKS: Attrs = Attrs(Attrs::BLINK.0 | Attrs::RAPID_BLINK.0);

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
    pub const WIRE: [(Attrs, u8); 23] = [
        (Attrs::BOLD, 1),
        (Attrs::DIM, 2),
        (Attrs::ITALIC, 3),
        (Attrs::UNDERLINE, 4),
        (Attrs::BLINK, 5),
        (Attrs::RAPID_BLINK, 6),
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

    /// Nine bits of the thirty-two are still free.
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

    /// How many `WIRE` entries name a bit of `family`: two for
    /// [`Attrs::BLINKS`]. A `const fn` so `ParamIdx::CAPACITY` can leave one
    /// rate out of its count without a literal that would drift.
    pub const fn wire_in(family: Attrs) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < Attrs::WIRE.len() {
            if Attrs::WIRE[i].0.0 & family.0 != 0 {
                n += 1;
            }
            i += 1;
        }
        n
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
    /// The attributes of `self` that are also in `o`.
    pub const fn only(self, o: Attrs) -> Attrs {
        Attrs(self.0 & o.0)
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

/// SGR colour. `Indexed` and `Rgb` are different kinds — `38;5;n` and
/// `38;2;r;g;b` produce values that must not be confused.
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
        StyleId::bucket_of_key(raw.key(), buckets)
    }
    /// [`StyleId::bucket`] by [`StyleWord::plain_key`]: the bucket the
    /// rendition table's inline probe starts from, the rendition's own when
    /// it has no SAPV variant.
    #[inline(always)]
    pub fn plain_bucket(raw: StyleWord, buckets: usize) -> usize {
        StyleId::bucket_of_key(raw.plain_key(), buckets)
    }
    #[inline(always)]
    fn bucket_of_key(key: u64, buckets: usize) -> usize {
        debug_assert!(buckets.is_power_of_two());
        // 64-bit now that a rendition is more than a colour, but the same
        // lesson: take the HIGH bits. The 32-bit constant is replaced by the
        // 64-bit Fibonacci one for the wider key.
        let shift = 64 - buckets.trailing_zeros();
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> shift) as usize
    }
    pub fn from_index(i: usize) -> StyleId {
        StyleId(i as u16)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// **How many writes the rendition table has taken** since it was allocated:
/// every slot filled, evicted or freed by a sweep, in order. A reader that
/// resolved the table up to one count asks for the slots written since.
/// Until the first sweep or eviction the table frees no slot, so while the
/// count is no more than the table's length the writes are exactly the
/// filled slots.
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

    /// Whether the table had taken more writes than it has slots, `len`. A
    /// table that sweeps passes its length without evicting, so this says
    /// nothing about whether a slot was reclaimed: that is
    /// [`Reclaims`]'s.
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
    const TAG_AUTOMATIC: u32 = 0x0400_0000;
    /// Zero, so a zeroed `Storage` starts with default colours.
    pub const DEFAULT: ColorWord = ColorWord(0);
    /// **The terminal's own automatic colour, which a program asked for**:
    /// kitty's empty value, "the terminal should use a dynamic color for
    /// example reverse video for selections" (`[[source:
    /// kitty-6d5d0c4-color-stack]]`). Held only where a cursor's or a
    /// selection's colour is kept, and set only by `OSC 21`; it is neither a
    /// palette index nor RGB, so [`ColorWord::as_rgb`] reads `None`, and
    /// unlike [`ColorWord::DEFAULT`] it wins over a configured colour.
    pub const AUTOMATIC: ColorWord = ColorWord(Self::TAG_AUTOMATIC);
    pub fn indexed(n: Chan) -> ColorWord {
        ColorWord(Self::TAG_INDEXED | n.get() as u32)
    }
    pub fn rgb(r: Chan, g: Chan, b: Chan) -> ColorWord {
        ColorWord(
            Self::TAG_RGB | ((r.get() as u32) << 16) | ((g.get() as u32) << 8) | b.get() as u32,
        )
    }
    /// The RGB triple an embedder gives, packed: the inverse of
    /// [`ColorWord::as_rgb`].
    pub fn from_rgb((r, g, b): (u8, u8, u8)) -> ColorWord {
        ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
    }
    pub fn bits(self) -> u32 {
        self.0
    }
    pub fn is_default(self) -> bool {
        self.0 == 0
    }
    /// Whether this is [`ColorWord::AUTOMATIC`].
    pub fn is_automatic(self) -> bool {
        self.0 == Self::TAG_AUTOMATIC
    }
    /// The palette index, when this word holds one.
    pub fn as_index(self) -> Option<u8> {
        (self.0 & Self::TAG_INDEXED != 0).then_some(self.0 as u8)
    }
    /// The RGB triple, when this word holds one. `None` for the default colour
    /// and for palette indices, neither of which names a pixel value on their
    /// own.
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
    ///
    /// **The byte above the font's is the character's string**: SDS's or
    /// SRS's (`[[source: ecma48]]` 8.3.114, 8.3.137; [`StringId`]), in the
    /// one word with the font, which needs one byte, so the word stays
    /// twenty bytes and its compare and its key what they were
    /// ([`FontString`]).
    font: FontString,
    /// SAPV's presentation variants (`[[source: ecma48]]` 8.3.108), in the
    /// two bytes that were explicit padding: `zerocopy`'s `IntoBytes`
    /// rejects implicit padding, and widening `attrs` to `u32` left two
    /// bytes of it. A variant is a rendition of the character it was
    /// written with, so it costs the cell nothing ([`crate::Variants`]).
    variants: Variants,
}

impl StyleWord {
    /// **A slot a sweep freed that a live rendition's probe path crosses**,
    /// kept filled so the probe walks on to that rendition: emptied, it would
    /// end the walk there, and naming the rendition again would intern it a
    /// second time. A new rendition takes it as it takes an empty slot. No
    /// `SGR` or SAPV folds to it, because no value sets the variants' bit 15
    /// ([`Variants::TOMBSTONE`]), and no cell names it, because only a slot
    /// no cell names becomes one.
    pub const TOMBSTONE: StyleWord = StyleWord {
        fg: ColorWord::DEFAULT,
        bg: ColorWord::DEFAULT,
        underline: ColorWord::DEFAULT,
        attrs: Attrs::NONE,
        font: FontString::PRIMARY,
        variants: Variants::TOMBSTONE,
    };
}

/// **Which slots of a rendition table some cell names**, for a sweep: one
/// bit a slot of the largest table, since a [`StyleId`] is a `u16`. 8 KiB,
/// so it lives on the sweep's stack and nowhere else.
pub struct SlotSet([u64; SlotSet::WORDS]);

impl SlotSet {
    const WORDS: usize = (1 << 16) / 64;
    pub const fn new() -> SlotSet {
        SlotSet([0; SlotSet::WORDS])
    }
    pub fn insert(&mut self, id: StyleId) {
        let k = id.index();
        self.0[k >> 6] |= 1 << (k & 63);
    }
    /// Every slot a cell of `cells` names.
    pub fn mark(&mut self, cells: &[PackedCell]) {
        for c in cells {
            self.insert(c.style);
        }
    }
    pub fn has(&self, k: usize) -> bool {
        self.0[(k >> 6) & (SlotSet::WORDS - 1)] & (1 << (k & 63)) != 0
    }
}

impl Default for SlotSet {
    fn default() -> SlotSet {
        SlotSet::new()
    }
}

/// **What the rendition table has reclaimed**, counted for a reader that
/// holds a frame. A sweep frees slots no cell names, which a later write can
/// give another rendition at a position the reader's last frame showed in
/// the old one -- the cell's word unchanged and its colour not; a freeze or
/// a thaw rewrites the ids of rows moved into a generation or back. Either
/// way row damage cannot see it, so a reader whose count moved repaints
/// everything ([`Reclaims::recolours_since`]).
///
/// **No slot a cell names is ever given another rendition.** That was the
/// full table's last resort, an eviction, and it recoloured the cells that
/// named the slot: `[[source: dec-std-070]]`'s SGR, page 5-104, note 3,
/// "In no way does the selection of current rendition affect the rendition
/// of characters already entered into the display". What a table that has
/// no slot to give does now is refuse the new rendition, which
/// `[[source: ecma48]]` § 8.3.117 leaves to it: "The usable combinations of
/// parameter values are determined by the implementation."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Reclaims {
    /// Renditions a full table had no slot for, after its sweep and its
    /// freeze: the text that followed took the default rendition, and no
    /// cell already drawn changed.
    pub refusals: u64,
    pub sweeps: u64,
    /// Slots a sweep emptied or made a [`StyleWord::TOMBSTONE`], over every
    /// sweep: a tombstone a sweep finds again is not counted twice.
    pub freed: u64,
    /// Freezes and thaws that rewrote rows' rendition ids
    /// (`crate::frozen`): the same cells name other ids in another table.
    pub rewrites: u64,
}

impl Reclaims {
    /// Whether a cell's id may name another rendition than it did at
    /// `since`: a sweep that freed a slot, or a freeze or a thaw that
    /// rewrote ids. A sweep that freed nothing changed no slot, and a
    /// refusal changes none.
    pub fn recolours_since(self, since: Reclaims) -> bool {
        self.freed != since.freed || self.rewrites != since.rewrites
    }
}

/// **The sweep's book beside the rendition table**: how full it is, how many
/// of its filled slots are tombstones, the write count at the last sweep, and
/// what it has reclaimed. In the caller's memory with the table
/// (`StyleBook`), so a terminal lent the table again finds them as it left
/// them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SweepBook {
    /// Filled slots, tombstones included; slot 0 is not counted.
    pub filled: usize,
    /// Tombstones in the table. A miss looks for one on its path only when
    /// there is one, so a table that never swept never looks.
    pub tombs: usize,
    pub swept_at: StyleWrites,
    /// The refusals at the last sweep: a table refusing renditions writes
    /// nothing, so [`SweepBook::due`] counts its refusals as writes.
    pub refused_at: u64,
    pub reclaims: Reclaims,
    /// Slots read by every probe that went past the inline
    /// [`SweepBook::INLINE`], each counted whole, and by the walks behind a
    /// miss to an empty slot or a tombstone.
    pub probes: SlotsRead,
}

impl SweepBook {
    /// The load a fill may not pass without a sweep: 7/8 of the table, where
    /// a linear probe's miss averages 32.5 slots (Knuth's `(1 + 1/(1-a)^2)/2`)
    /// and is still climbing towards the whole table's walk at full load.
    pub const LOAD: (usize, usize) = (7, 8);
    /// The slots the inline probe reads before the cold half takes over:
    /// twice the 32.5 slots a miss reads on average at 7/8 load
    /// ([`SweepBook::LOAD`]).
    pub const INLINE: usize = 64;
    /// Whether `len / 16` writes have passed since the last sweep, so a
    /// table whose cells name nearly every slot is not swept a rendition at
    /// a time.
    pub fn due(&self, now: StyleWrites, len: usize) -> bool {
        let refused = self.reclaims.refusals.saturating_sub(self.refused_at);
        now.0.saturating_sub(self.swept_at.0) + refused >= (len / 16) as u64
    }
    /// Whether one more filled slot passes [`SweepBook::LOAD`].
    pub fn over_load(&self, len: usize) -> bool {
        (self.filled + 1) * SweepBook::LOAD.1 > len * SweepBook::LOAD.0
    }
    /// The cold half of a probe read `n` slots.
    pub fn probed(&mut self, n: usize) {
        self.probes.0 += n as u64;
    }
    /// A miss filled an empty slot.
    pub fn filled_one(&mut self) {
        self.filled += 1;
    }
    /// A miss took a tombstone.
    pub fn tomb_taken(&mut self) {
        self.tombs = self.tombs.saturating_sub(1);
    }
    /// A full table had no slot for a rendition.
    pub fn refused(&mut self) {
        self.reclaims.refusals += 1;
    }
    /// A freeze or a thaw rewrote rows' ids.
    pub fn rewrote(&mut self) {
        self.reclaims.rewrites += 1;
    }
    /// A sweep ran, after its frees were logged at `now`: `freed` slots
    /// emptied or made tombstones, and `filled` left filled, `tombs` of them
    /// tombstones.
    pub fn swept(&mut self, now: StyleWrites, freed: usize, filled: usize, tombs: usize) {
        self.filled = filled;
        self.tombs = tombs;
        self.swept_at = now;
        self.refused_at = self.reclaims.refusals;
        self.reclaims.sweeps += 1;
        self.reclaims.freed += freed as u64;
    }
}

/// Which of the ten fonts `[[source: ctlseqs]]` gives SGR the text is set in.
///
/// `SGR 10` is the primary font and `SGR 11`-`19` the nine alternates. baud
/// records the selection and does not resolve it: choosing a face is a
/// renderer's job, and there is no font machinery in a `no_std` core with no
/// allocator. Recording it is the channel -- the same one charset designations
/// use -- and it is read wherever a cell's rendition is read.
///
/// A newtype rather than a bare `u8`, and `repr(transparent)` so it adds its
/// one byte and no more: ten fonts need one, and the byte beside it holds
/// the character's string ([`StringId`]).
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct FontId(u8);

impl FontId {
    /// `SGR 10`. Zero, so a zeroed style table is the primary font.
    pub const PRIMARY: FontId = FontId(0);

    /// From an SGR parameter in `10..=19`, or `None` for anything else.
    /// `const` so the single-parameter `SGR` table can be built at compile
    /// time; it is a pure range test either way.
    pub const fn from_sgr(n: u16) -> Option<FontId> {
        match n >= 10 && n <= 19 {
            true => Some(FontId((n - 10) as u8)),
            false => None,
        }
    }

    /// `0` for the primary font, `1..=9` for the alternates.
    pub fn index(self) -> u16 {
        u16::from(self.0)
    }

    /// The SGR parameter that selects it, for DECRQSS.
    ///
    /// A `u8` because the range is `10..=19` by construction, so the caller
    /// does not need a cast -- and `baud-core`'s budget is a floor rather than
    /// slack, so one there would have to displace something forced.
    pub fn sgr(self) -> u8 {
        self.0 + 10
    }

    pub fn is_primary(self) -> bool {
        self.0 == 0
    }
}

/// The odd multiplier [`StyleWord::key`] mixes SAPV's variants by, apart
/// from the one the rest is mixed by.
const VARIANTS_MIX: u64 = 0x9FB2_1C65_1E98_DF25;

/// The odd multiplier [`StyleWord::key`] mixes a character's string by,
/// apart from the other two.
const STRING_MIX: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// **A rendition's font and string in one word**: [`FontId`] in the low
/// byte and [`StringId`] in the high one. One field, where two compared
/// each on its own, so a probe's compare is the instructions it was with
/// the font alone, and so is the inline probe's key, which reads the word
/// whole. A rendition with a string is not keyed there: it takes its own
/// key's path, as one with SAPV's variants does ([`StyleWord::is_plain`]),
/// where [`StyleWord::key`] mixes the string apart; in the inline key it
/// lies over `attrs`' bits 3 to 10, xored.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
pub struct FontString(u16);

impl FontString {
    const PRIMARY: FontString = FontString(0);

    fn font(self) -> FontId {
        FontId((self.0 & 0xff) as u8)
    }
    fn string(self) -> StringId {
        StringId::from_index(usize::from(self.0 >> 8)).unwrap_or(StringId::NONE)
    }
    fn with_font(self, f: FontId) -> FontString {
        FontString((self.0 & 0xff00) | u16::from(f.0))
    }
    fn with_string(self, s: StringId) -> FontString {
        FontString((self.0 & 0xff) | ((s.index() as u16) << 8))
    }
    /// The string alone: what `SGR 0` leaves.
    fn string_only(self) -> FontString {
        FontString(self.0 & 0xff00)
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
        font: FontString::PRIMARY,
        variants: Variants::DEFAULT,
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
        self.font.font()
    }
    pub fn with_font(self, f: FontId) -> StyleWord {
        StyleWord {
            font: self.font.with_font(f),
            ..self
        }
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
    /// SAPV's variants ([`Variants`]).
    pub fn variants(self) -> Variants {
        self.variants
    }
    pub fn with_variants(self, v: Variants) -> StyleWord {
        StyleWord {
            variants: v,
            ..self
        }
    }
    /// The SDS or SRS string the character was written in ([`StringId`]).
    pub fn string(self) -> StringId {
        self.font.string()
    }
    pub fn with_string(self, s: StringId) -> StyleWord {
        StyleWord {
            font: self.font.with_string(s),
            ..self
        }
    }
    /// **Whether the rendition table's inline probe keys it**: no SAPV
    /// variant and no string, the key of almost every rendition
    /// ([`StyleWord::plain_key`]).
    #[inline(always)]
    pub fn is_plain(self) -> bool {
        self.variants.is_default() && self.font.string().is_none()
    }
    /// **What `SGR 0` leaves**: the default rendition, these variants and
    /// this string kept, since neither SAPV nor SDS and SRS is an `SGR`
    /// ("cancels the effect of any preceding occurrence of SGR",
    /// `[[source: ecma48]]` 8.3.117).
    pub fn sgr_reset(self) -> StyleWord {
        StyleWord {
            variants: self.variants,
            font: self.font.string_only(),
            ..StyleWord::DEFAULT
        }
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
        self.font = self.font.with_font(f);
    }
    pub fn set_underline(&mut self, c: ColorWord) {
        self.underline = c;
    }
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
    ///
    /// **SAPV's variants are mixed apart, and only when there are any**:
    /// with none the key is [`StyleWord::plain_key`], which the rendition
    /// table's inline probe walks by, so a rendition without variants keys
    /// and probes as it did before they were hashed. Their product has a
    /// multiplier of its own; `no_font_and_variant_share_a_key` holds every
    /// font and set of variants apart from each other and from every
    /// underline colour.
    pub fn key(self) -> u64 {
        match (self.variants.0, self.font.0 >> 8) {
            (0, 0) => self.plain_key(),
            (v, s) => {
                self.plain_key()
                    ^ u64::from(v).wrapping_mul(VARIANTS_MIX)
                    ^ u64::from(s).wrapping_mul(STRING_MIX)
            }
        }
    }

    /// [`StyleWord::key`] with SAPV's variants left out: the key of every
    /// rendition that has none.
    #[inline(always)]
    pub fn plain_key(self) -> u64 {
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
        //
        // **The font is at bit 27 of the underline's word**, above every
        // colour's tag, where no colour sets a bit: laid at bit 48, over
        // `attrs`' top bits, `SGR 11` keyed as `SGR 63`'s ideogram double
        // overline, and two renditions that shared a key shared a bucket,
        // and which took it went by which was interned first -- an order a
        // read boundary can move, the status line's indicator being drawn
        // at a read's end, so one screen came out under two numberings.
        //
        // **The font's word is xored in, not ored**: its high byte, the
        // character's string, lies over `attrs`' bits 3 to 10, and ored a
        // string bit hid the attribute under it -- a string with and
        // without `SGR 4` keyed alike, and the indicator's rendition took
        // its twin's bucket. Xored, each string permutes the attributes, and
        // [`StyleWord::key`] mixes the string apart. With no string the two
        // are the same bits.
        const M: u64 = 0xD6E8_FEB8_6659_FD93;
        let base = u64::from(self.fg.0) | (u64::from(self.bg.0) << 32);
        let tail = (u64::from(self.underline.0) | (u64::from(self.attrs.bits()) << 32))
            ^ (u64::from(self.font.0) << 27);
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

    /// **The blink rates are masked replaces too**, and the test above cannot
    /// say so: it walks `parse_one`, which `5`, `6` and `25` never reach, so
    /// the SGR table's derived pair for them was otherwise unchecked. Each,
    /// and `5;6` and `6;5` composed, from every start word and from a word
    /// holding the other rate: the later rate replaces the earlier.
    #[test]
    fn the_blink_rates_are_masked_replaces() {
        let one = |v: u16| SgrAttr::one(Param::from_raw(v)).expect("a one-parameter SGR");
        let mut starts = [StyleWord::DEFAULT; 4];
        starts[1].as_mut_bytes().fill(0xff);
        starts[2] = StyleWord::DEFAULT.with_attrs(Attrs::BLINK.with(Attrs::BOLD));
        starts[3] = StyleWord::DEFAULT.with_attrs(Attrs::RAPID_BLINK);
        for seq in [&[5u16][..], &[6], &[25], &[5, 6], &[6, 5], &[6, 25]] {
            let fold = StyleFold::derive(|w| seq.iter().for_each(|&v| one(v).fold_into(w)));
            for s in starts.iter() {
                let (mut want, mut got) = (*s, *s);
                seq.iter().for_each(|&v| one(v).fold_into(&mut want));
                fold.apply(&mut got);
                assert_eq!(want, got, "SGR {seq:?} from {s:?}");
                let blinks = want.attrs().bits() & Attrs::BLINKS.bits();
                let last = *seq.last().expect("a parameter");
                let expect = match last {
                    5 => Attrs::BLINK.bits(),
                    6 => Attrs::RAPID_BLINK.bits(),
                    _ => 0,
                };
                assert_eq!(blinks, expect, "SGR {seq:?} from {s:?}: the last rate wins");
            }
        }
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
        let (quads, rest) = seg.as_chunks::<16>();
        for q in quads {
            a = (a ^ Self::word(&q[..8])).wrapping_mul(Self::PRIME);
            b = (b ^ Self::word(&q[8..])).wrapping_mul(Self::PRIME);
        }
        let mut tail = [StreamByte::NUL; 16];
        for (d, x) in tail.iter_mut().zip(rest) {
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

#[cfg(test)]
mod style_key {
    use crate::*;

    /// **No font and no set of SAPV's variants share a key**: every font,
    /// 0 to 9, against every word of the variants' eleven live bits, and
    /// against every underline colour of the palette and of each RGB
    /// channel's bits, which a font and variants laid where a colour is
    /// would trade places with; and each font against every `Attrs` bit
    /// alone. Exclusive or gave `SGR 11` and SAPV 2 one key, and `SGR 11`
    /// and the ideogram double overline shared one by overlapping.
    #[test]
    fn no_font_and_variant_share_a_key() {
        const FACES: usize = 10 << 11;
        const N: usize = FACES + 256 + 24;
        let mut keys = [0u64; N];
        for (i, k) in keys.iter_mut().enumerate() {
            let w = match i {
                i if i < FACES => {
                    let (f, v) = (i >> 11, i & 0x7ff);
                    let font = FontId::from_sgr(10 + f as u16).expect("a font");
                    StyleWord::DEFAULT
                        .with_font(font)
                        .with_variants(Variants(v as u16))
                }
                i if i < FACES + 256 => StyleWord::DEFAULT
                    .with_underline(ColorWord(ColorWord::TAG_INDEXED | (i - FACES) as u32)),
                i => StyleWord::DEFAULT
                    .with_underline(ColorWord(ColorWord::TAG_RGB | 1 << (i - FACES - 256))),
            };
            *k = w.key();
        }
        keys.sort_unstable();
        assert!(keys.windows(2).all(|p| p[0] != p[1]));
        let fonts: [u64; 10] = core::array::from_fn(|f| {
            StyleWord::DEFAULT
                .with_font(FontId::from_sgr(10 + f as u16).expect("a font"))
                .key()
        });
        for b in 0..u32::BITS {
            let a = StyleWord::DEFAULT.with_attrs(Attrs(1 << b)).key();
            assert!(!fonts.contains(&a), "attribute bit {b}");
        }
        assert_eq!(
            StyleWord::DEFAULT.key(),
            StyleWord::DEFAULT.with_variants(Variants::DEFAULT).key(),
            "the control"
        );
    }

    /// **No string shares a key with a font, an attribute or a set of
    /// variants**: each of the 255 strings, alone and with each font,
    /// against every `Attrs` bit alone, every font alone and every word of
    /// the variants' live bits alone. Laid at bit 35 of the inline key a
    /// string was an attribute: string 1 keyed as `SGR 4`.
    #[test]
    fn no_string_shares_a_key() {
        const STRINGS: usize = StringId::CAPACITY * 10;
        const N: usize = STRINGS + 32 + 10 + 0x7ff;
        let mut keys = [0u64; N];
        let mut n = 0;
        for s in 1..=StringId::CAPACITY {
            let id = StringId::from_index(s).expect("a string");
            for f in 0..10u16 {
                let font = FontId::from_sgr(10 + f).expect("a font");
                keys[n] = StyleWord::DEFAULT.with_font(font).with_string(id).key();
                n += 1;
            }
        }
        for b in 0..u32::BITS {
            keys[n] = StyleWord::DEFAULT.with_attrs(Attrs(1 << b)).key();
            n += 1;
        }
        for f in 0..10u16 {
            let font = FontId::from_sgr(10 + f).expect("a font");
            keys[n] = StyleWord::DEFAULT.with_font(font).key();
            n += 1;
        }
        for v in 1..0x800u16 {
            keys[n] = StyleWord::DEFAULT.with_variants(Variants(v)).key();
            n += 1;
        }
        assert_eq!(n, N);
        let mut strings = keys;
        strings[..STRINGS].sort_unstable();
        assert!(
            strings[..STRINGS].windows(2).all(|p| p[0] != p[1]),
            "two strings"
        );
        let others = &keys[STRINGS..];
        for k in &keys[..STRINGS] {
            assert!(!others.contains(k), "a string shares a key");
        }
        let s1 = StringId::from_index(1).expect("a string");
        assert!(!StyleWord::DEFAULT.with_string(s1).is_plain());
        assert!(StyleWord::DEFAULT.is_plain(), "the control");
        // A string with and without an attribute under its byte: ored, the
        // two keyed alike (the property's shrunk case, `sds-rtl` and
        // `sgr-underline-sds` beside the status line's indicator).
        for b in 0..u32::BITS {
            let w = StyleWord::DEFAULT.with_string(s1);
            let a = w.with_attrs(Attrs(1 << b));
            assert_ne!(w.plain_key(), a.plain_key(), "attribute bit {b}, inline");
            assert_ne!(w.key(), a.key(), "attribute bit {b}");
        }
    }
}
