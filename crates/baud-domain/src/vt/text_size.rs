//! **Text sizing, `OSC 66 ; metadata ; text`**
//! (`[[source: kitty-text-sizing]]` 52-82): the metadata read into a
//! [`TextSize`], whose fields hold only the values the protocol's key
//! table defines, and the text read as safe UTF-8 ([`SizedText`]).
//!
//! A value outside the table is not represented: [`TextSize::parse`]
//! answers it with a [`TextSizeRefused`] that names the cause, and every
//! field type is built by the parser alone.

use crate::*;

/// The most bytes of text one sequence carries: "the text must be no
/// longer than ``4096`` bytes".
pub const TEXT_SIZE_TEXT_MAX: usize = 4096;

/// `s`: the block is `s * w` cells wide and `s` cells tall. 1 to 7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeScale(u8);

impl SizeScale {
    /// `s=1`, the default, and the only scale a width-only reader draws.
    pub const ONE: SizeScale = SizeScale(1);
    /// The largest scale.
    pub const MAX: usize = 7;
    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

/// `w` of 1 to 7: how many scaled cells wide one block is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeCells(u8);

impl SizeCells {
    /// The widest block, in scaled cells.
    pub const MAX: usize = 7;
    pub fn get(self) -> usize {
        usize::from(self.0)
    }
}

/// `w`: how the text is laid into cells.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeWidth {
    /// `w=0`: the text is split into cells as other text is, each cell an
    /// `s` by `s` block.
    Split,
    /// `w` of 1 to 7: all of the text in one block this many scaled cells
    /// wide. The width is explicit, and no variation selector or spacing
    /// mark after the text changes it.
    Cells(SizeCells),
}

impl SizeWidth {
    /// Whether the sequence set the width: `w` of 1 or more.
    pub fn is_explicit(self) -> bool {
        matches!(self, SizeWidth::Cells(_))
    }
    /// The block's width in scaled cells, when the sequence set one.
    pub fn cells(self) -> Option<usize> {
        match self {
            SizeWidth::Split => None,
            SizeWidth::Cells(c) => Some(c.get()),
        }
    }
}

/// `n / d` with `n < d <= 15`: the text drawn at this fraction of the
/// block's size, which never changes how many cells the block takes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SizeRatio {
    n: u8,
    d: u8,
}

impl SizeRatio {
    /// The largest numerator or denominator.
    pub const MAX: usize = 15;
    pub fn numerator(self) -> usize {
        usize::from(self.n)
    }
    pub fn denominator(self) -> usize {
        usize::from(self.d)
    }
}

/// `n` and `d`: the fractional scale.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeFrac {
    /// `d=0`: no fractional scale, whatever `n` is.
    Whole,
    Ratio(SizeRatio),
}

/// `v`: where a fractionally scaled render area sits in the block's
/// height.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeVAlign {
    Top,
    Bottom,
    Centre,
}

/// `h`: where a fractionally scaled render area sits in the block's
/// width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeHAlign {
    Left,
    Right,
    Centre,
}

/// One sequence's metadata, every key in its table's range.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TextSize {
    pub scale: SizeScale,
    pub width: SizeWidth,
    pub frac: SizeFrac,
    pub valign: SizeVAlign,
    pub halign: SizeHAlign,
}

/// Why a sequence's metadata is not a [`TextSize`]: one variant a cause.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextSizeRefused {
    /// A part that is not `key=value`: no `=`, or nothing before it.
    Malformed,
    /// A key the table does not name.
    UnknownKey,
    /// A value that is not a decimal integer, an optional `-` and digits.
    NotInteger,
    /// `s` outside 1 to 7.
    Scale,
    /// `w` outside 0 to 7.
    Width,
    /// `n` outside 0 to 15.
    Numerator,
    /// `d` outside 0 to 15.
    Denominator,
    /// `d` non-zero and not above `n`.
    Order,
    /// `v` outside 0 to 2.
    VAlign,
    /// `h` outside 0 to 2.
    HAlign,
}

/// A decimal integer: an optional `-` and one or more ASCII digits.
/// Saturates, so a value past what an `i64` holds stays out of every
/// key's range rather than wrapping into it.
fn integer(v: &[u8]) -> Option<i64> {
    let (neg, digits) = match v.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, v),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n = digits.iter().fold(0i64, |n, &b| {
        n.saturating_mul(10).saturating_add(i64::from(b - b'0'))
    });
    Some(match neg {
        true => n.saturating_neg(),
        false => n,
    })
}

/// `x` as a `u8` when it is in `lo..=hi`.
fn within(x: i64, lo: u8, hi: u8) -> Option<u8> {
    u8::try_from(x).ok().filter(|v| (lo..=hi).contains(v))
}

impl TextSize {
    /// Every key at its default: `s=1`, `w=0`, `n=d=0`, `v=h=0`.
    pub const DEFAULT: TextSize = TextSize {
        scale: SizeScale::ONE,
        width: SizeWidth::Split,
        frac: SizeFrac::Whole,
        valign: SizeVAlign::Top,
        halign: SizeHAlign::Left,
    };

    /// The metadata and the text of the bytes after `66;`: split at the
    /// first `;`, since the metadata holds none and the text may. `None`
    /// when there is no `;`.
    pub fn split(rest: &[u8]) -> Option<(&[u8], &[u8])> {
        let i = rest.iter().position(|&b| b == b';')?;
        Some((&rest[..i], &rest[i + 1..]))
    }

    /// Read the metadata: `key=value` parts separated by `:`, an empty part
    /// skipped, and a repeated key's last value kept. Each value is read
    /// as an integer as its part arrives, and checked against its range
    /// once every part has been read.
    pub fn parse(meta: &[u8]) -> Result<TextSize, TextSizeRefused> {
        let (mut s, mut w, mut n, mut d, mut v, mut h) = (1i64, 0i64, 0i64, 0i64, 0i64, 0i64);
        for part in meta.split(|&b| b == b':') {
            if part.is_empty() {
                continue;
            }
            let eq = match part.iter().position(|&b| b == b'=') {
                Some(i) if i > 0 => i,
                _ => return Err(TextSizeRefused::Malformed),
            };
            let slot = match &part[..eq] {
                b"s" => &mut s,
                b"w" => &mut w,
                b"n" => &mut n,
                b"d" => &mut d,
                b"v" => &mut v,
                b"h" => &mut h,
                _ => return Err(TextSizeRefused::UnknownKey),
            };
            *slot = integer(&part[eq + 1..]).ok_or(TextSizeRefused::NotInteger)?;
        }
        let scale = SizeScale(within(s, 1, 7).ok_or(TextSizeRefused::Scale)?);
        let width = match within(w, 0, 7).ok_or(TextSizeRefused::Width)? {
            0 => SizeWidth::Split,
            c => SizeWidth::Cells(SizeCells(c)),
        };
        let n = within(n, 0, 15).ok_or(TextSizeRefused::Numerator)?;
        let d = within(d, 0, 15).ok_or(TextSizeRefused::Denominator)?;
        let frac = match d {
            0 => SizeFrac::Whole,
            d if d > n => SizeFrac::Ratio(SizeRatio { n, d }),
            _ => return Err(TextSizeRefused::Order),
        };
        let valign = match within(v, 0, 2).ok_or(TextSizeRefused::VAlign)? {
            0 => SizeVAlign::Top,
            1 => SizeVAlign::Bottom,
            _ => SizeVAlign::Centre,
        };
        let halign = match within(h, 0, 2).ok_or(TextSizeRefused::HAlign)? {
            0 => SizeHAlign::Left,
            1 => SizeHAlign::Right,
            _ => SizeHAlign::Centre,
        };
        Ok(TextSize {
            scale,
            width,
            frac,
            valign,
            halign,
        })
    }

    /// The size as one word, every key in its own bits: what a store hashes
    /// and compares.
    pub(crate) fn packed(self) -> u32 {
        let w = self.width.cells().unwrap_or(0);
        let (n, d) = match self.frac {
            SizeFrac::Whole => (0, 0),
            SizeFrac::Ratio(r) => (r.numerator(), r.denominator()),
        };
        let v = match self.valign {
            SizeVAlign::Top => 0,
            SizeVAlign::Bottom => 1,
            SizeVAlign::Centre => 2,
        };
        let h = match self.halign {
            SizeHAlign::Left => 0,
            SizeHAlign::Right => 1,
            SizeHAlign::Centre => 2,
        };
        let word = self.scale.get() | w << 3 | n << 6 | d << 10 | v << 14 | h << 16;
        u32::try_from(word).unwrap_or(0)
    }
}

/// **The code points of a sequence's text**, read as safe UTF-8: each
/// maximal ill-formed subpart is one U+FFFD, and a C0 control, `DEL` or a
/// C1 control, which the text may not hold, is left out and counted
/// ([`SizedText::dropped`]).
pub struct SizedText<'a> {
    chunks: core::str::Utf8Chunks<'a>,
    valid: core::str::Chars<'a>,
    /// The U+FFFD owed for the ill-formed bytes after `valid`.
    owed: Option<Codepoint>,
    dropped: usize,
}

impl<'a> SizedText<'a> {
    pub fn new(text: &'a [u8]) -> SizedText<'a> {
        SizedText {
            chunks: text.utf8_chunks(),
            valid: "".chars(),
            owed: None,
            dropped: 0,
        }
    }
    /// How many controls were left out so far.
    pub fn dropped(&self) -> usize {
        self.dropped
    }
}

impl Iterator for SizedText<'_> {
    type Item = Codepoint;
    fn next(&mut self) -> Option<Codepoint> {
        loop {
            if let Some(c) = self.valid.next() {
                let v = u32::from(c);
                if v < 0x20 || (0x7F..=0x9F).contains(&v) {
                    self.dropped += 1;
                    continue;
                }
                match Codepoint::new(v) {
                    Some(cp) => return Some(cp),
                    None => continue,
                }
            }
            if let Some(r) = self.owed.take() {
                return Some(r);
            }
            let chunk = self.chunks.next()?;
            self.valid = chunk.valid().chars();
            if !chunk.invalid().is_empty() {
                self.owed = Some(Codepoint::REPLACEMENT);
            }
        }
    }
}
