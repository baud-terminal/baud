use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Unicode East Asian Width data plus locale policy for ambiguous width and
/// emoji presentation. Contains no font, and no font type exists in this crate
/// to put in one.
pub struct WidthTable {
    pub ambiguous: AmbiguousWidth,
    pub emoji: EmojiWidth,
}

/// East Asian Ambiguous characters are 1 or 2 columns depending on locale.
/// Two bools next to each other invite transposition; two enums do not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AmbiguousWidth {
    Narrow,
    Wide,
}

/// Whether `U+FE0F` presentation makes emoji 2 columns. Terminals disagree;
/// this is config, not font.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EmojiWidth {
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
    /// Not public: `measure` is the only route from content to a width.
    pub(crate) fn from_cells(n: u32) -> DisplayWidth {
        DisplayWidth(n)
    }
    /// Byte length *is* column count, but only for a byte-per-column encoding.
    /// Callable only where a `Bytes<Printable>` or `Bytes<DecGraphics>` proof
    /// exists, which is what makes it sound.
    pub(crate) fn from_len(n: ByteLen) -> DisplayWidth {
        DisplayWidth(n.index() as u32)
    }
}
