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
/// included), and a `U+FE0F` attaches to its base without changing the
/// base's width: the question is answerable and not answered, which is
/// recorded rather than reintroduced as a setting nothing acts on.
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
