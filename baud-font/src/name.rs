//! `name`, the naming table (`[[source: ms-opentype-name]]`): a face's family,
//! which is what a program names when it asks for a font (`OSC 50`).
//!
//! "16 Typographic Family name ... If name ID 16 is absent, then name ID 1
//! is considered to be the typographic family name": nameID 16, else 1.
//! "Strings for the Unicode platform must be encoded in UTF-16BE", and a
//! Windows record with encoding 1 (Unicode BMP) or 10 (full repertoire) is
//! UTF-16BE too; a Macintosh Roman record (platform 1, encoding 0) is read
//! where its bytes are ASCII, the one range Mac Roman shares with Unicode.
//! Of several records, Windows English (United States, language 0x0409)
//! first, then any Windows one, then the Unicode platform's, then Mac Roman.

use crate::bytes::{Bytes, Count, Index, Len, Offset};
use crate::{Face, Tag};

impl Tag {
    pub const NAME: Tag = Tag(*b"name");
}

/// How a name record's bytes are encoded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Encoding {
    Utf16Be,
    Ascii,
}

/// A face's family name, as its `name` table stores it.
#[derive(Clone, Copy)]
pub struct Family<'a> {
    bytes: Bytes<'a>,
    encoding: Encoding,
}

impl<'a> Family<'a> {
    /// The name's characters, or `None` at the first that does not decode:
    /// an unpaired surrogate, or a Mac Roman byte past ASCII.
    pub fn chars(&self) -> impl Iterator<Item = Option<char>> + 'a {
        let b = self.bytes.as_slice();
        let encoding = self.encoding;
        let mut at = 0;
        core::iter::from_fn(move || match encoding {
            Encoding::Ascii => {
                let c = *b.get(at)?;
                at += 1;
                Some(c.is_ascii().then_some(char::from(c)))
            }
            Encoding::Utf16Be => {
                let unit = |i: usize| -> Option<u16> {
                    Some(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]))
                };
                let hi = unit(at)?;
                at += 2;
                let c = match hi {
                    0xd800..=0xdbff => {
                        let lo = unit(at).filter(|lo| (0xdc00..=0xdfff).contains(lo));
                        at += 2;
                        lo.and_then(|lo| {
                            let v = 0x10000
                                + ((u32::from(hi) - 0xd800) << 10)
                                + (u32::from(lo) - 0xdc00);
                            char::from_u32(v)
                        })
                    }
                    _ => char::from_u32(u32::from(hi)),
                };
                Some(c)
            }
        })
    }

    /// The name in UTF-8, written into `out`: `None` when it does not
    /// decode or does not fit.
    pub fn utf8<'o>(&self, out: &'o mut [u8]) -> Option<&'o [u8]> {
        let mut n = 0;
        for c in self.chars() {
            let c = c?;
            let len = c.len_utf8();
            c.encode_utf8(out.get_mut(n..n + len)?);
            n += len;
        }
        Some(&out[..n])
    }

    /// Whether `name`, UTF-8, is this family, ASCII letters compared
    /// without their case, as a family typed by hand is compared.
    pub fn matches(&self, name: &[u8]) -> bool {
        let Ok(name) = core::str::from_utf8(name) else {
            return false;
        };
        let mut want = name.chars();
        for c in self.chars() {
            match (c, want.next()) {
                (Some(c), Some(w)) if c.eq_ignore_ascii_case(&w) => {}
                _ => return false,
            }
        }
        want.next().is_none()
    }
}

impl<'a> Face<'a> {
    /// **The face's family**: `name`'s typographic family (nameID 16), else
    /// its family (nameID 1), from the record the module's order prefers;
    /// `None` with no `name` table, or none of the two in an encoding read
    /// here.
    pub fn family(&self) -> Option<Family<'a>> {
        let t = self.table(Tag::NAME)?;
        let count = Count::of(t.read::<u16>(Offset::of(2u32))?);
        let storage = Offset::of(t.read::<u16>(Offset::of(4u32))?);
        let mut best: Option<(Rank, Family<'a>)> = None;
        let mut i = Index::ZERO;
        while count.holds(i) {
            let at = Offset::of(6u32).at(i, Len::of(12))?;
            let field = |k: u32| t.read::<u16>(at.plus(Len::of(k))?);
            let (platform, encoding, language, id) = (field(0)?, field(2)?, field(4)?, field(6)?);
            let (length, offset) = (field(8)?, field(10)?);
            i = i.next();
            let id_rank = match id {
                16 => 0,
                1 => 1,
                _ => continue,
            };
            let (place, how) = match (platform, encoding, language) {
                (3, 1 | 10, 0x0409) => (0, Encoding::Utf16Be),
                (3, 1 | 10, _) => (1, Encoding::Utf16Be),
                (0, _, _) => (2, Encoding::Utf16Be),
                (1, 0, _) => (3, Encoding::Ascii),
                _ => continue,
            };
            let start = storage.plus(Len::of(u32::from(offset)))?;
            let Some(bytes) = t.slice(start, Len::of(u32::from(length))) else {
                continue;
            };
            let rank = Rank(id_rank * 4 + place);
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((
                    rank,
                    Family {
                        bytes,
                        encoding: how,
                    },
                ));
            }
        }
        best.map(|(_, f)| f)
    }
}

/// A record's place in the module's order, lower first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Rank(u8);
