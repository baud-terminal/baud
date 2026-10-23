use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Locale policy for East Asian Ambiguous width. Contains no font, and no font
/// type exists in this crate to put in one.
///
/// One field, not two. It carried an `emoji: EmojiWidth` as well — whether
/// `U+FE0F` presentation makes an emoji two columns — and that cannot be
/// answered per codepoint: `U+FE0F` is `Mn` and therefore zero-width, so the
/// question is whether the *preceding* character is upgraded, which is a
/// grapheme-cluster property. baud did not cluster when the field was
/// removed, so it could never have been read. It clusters now (UAX #29, GB11
/// included), and the question is answered where the cluster is: the core
/// widens a listed base when its `U+FE0F` attaches ([`crate::vs16`]), and the
/// embedder's choice is [`crate::Vs16Width`], not a field here.
#[derive(Clone, Copy, Default)]
pub struct WidthTable {
    pub ambiguous: AmbiguousWidth,
}

/// East Asian Ambiguous characters are 1 or 2 columns depending on locale.
///
/// `[[source: uax11]]` § 5: narrow when the context is unavailable. For a
/// `no_std` core with no locale it is always unavailable, so `Narrow` is the
/// default — and it is what xterm does too, whose `cjkWidth` defaults false.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum AmbiguousWidth {
    #[default]
    Narrow,
    Wide,
}

/// Width in cells.
///
/// Font-independent by wire contract, not preference: `ncurses` on the far end
/// computes columns with `wcwidth`, so a terminal using font advances would
/// drift from the application's model of the screen. Verified against
/// alacritty, whose model crate has no font dependency at all.
///
/// Consequence encoded below: [`DisplayWidth::measure`] takes a [`WidthTable`]
/// and nothing else. A font change cannot invalidate a cached width, because a
/// font never entered the computation.
#[derive(Clone, Copy, PartialEq, Eq, FromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(transparent)]
pub struct DisplayWidth(pub(crate) u32);

impl DisplayWidth {
    pub const ZERO: DisplayWidth = DisplayWidth(0);
    /// The only constructor. Caching this measured 14 us against 2223 us to
    /// recompute from bytes, for 15k lines — 4 bytes per line.
    pub(crate) fn cells(self) -> u32 {
        self.0
    }
    /// Byte length *is* column count, but only for a byte-per-column encoding.
    /// Callable only where a `Bytes<Printable>` or `Bytes<DecGraphics>` proof
    /// exists, which is what makes it sound.
    pub(crate) fn from_len(n: ByteLen) -> DisplayWidth {
        DisplayWidth(n.index() as u32)
    }
}

impl WidthTable {
    pub const DIAG: WidthTable = WidthTable {
        ambiguous: AmbiguousWidth::Narrow,
    };
}

/// **Whether `c` is `Default_Ignorable_Code_Point`**
/// (`[[source: dcp-data]]`): ZWJ, ZWNJ, the variation selectors, the tags
/// and the like, which TUS 5.21 says are ignored for display when not
/// supported -- drawn as nothing. The core asks it of a zero-width
/// character that starts a cluster after a control: one that draws nothing
/// has no cell to take (`Paint::glyph_as`). A binary search over the
/// generated ranges, on that cold path alone. The ranges,
/// `baud-domain/src/ignorable_data.rs`, are the same generated table byte
/// for byte as `baud-font/src/ignorable.rs`: `scripts/gen-default-ignorable.py`
/// writes each from the pinned `DerivedCoreProperties.txt`, and both
/// regenerate to what is committed.
pub fn is_default_ignorable(c: Codepoint) -> bool {
    let v = c.value();
    let ranges = crate::ignorable_data::DEFAULT_IGNORABLE;
    let i = ranges.partition_point(|&(_, end)| end < v);
    ranges.get(i).is_some_and(|&(start, _)| start <= v)
}
