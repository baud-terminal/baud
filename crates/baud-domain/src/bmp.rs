//! **BMP, decoded to pixels**: the bitmap file Microsoft documents, "a
//! BITMAPFILEHEADER structure followed by a BITMAPINFOHEADER,
//! BITMAPV4HEADER, or BITMAPV5HEADER structure" (`[[source: ms-bitmap-storage]]`),
//! or the BITMAPCOREHEADER the four header types close with
//! (`[[source: ms-bitmap-header-types]]`); 1, 4, 8, 16, 24 and 32 bits a
//! pixel; uncompressed (`BI_RGB`), with colour masks (`BI_BITFIELDS`), and
//! run-length encoded (`BI_RLE8`, `BI_RLE4`); bottom-up and top-down rows.
//!
//! **Why it is here.** An `OSC 1337 ; File` picture is "Any image format
//! that macOS supports" (`[[source: iterm2-images]]`), and a bitmap is one
//! of them; with no engine dependency the decoder is baud's own.
//!
//! **The file.** `bfType` "must be 0x4d42 (the ASCII string "BM")", and
//! `bfOffBits` is "The offset, in bytes, from the beginning of the
//! BITMAPFILEHEADER structure to the bitmap bits"
//! (`[[source: ms-bitmapfileheader]]`): the pixels are read from there,
//! never from where the colour table ends. "The four types of bitmap
//! headers are differentiated by the Size member", so the header is told by
//! its size alone: 12 for BITMAPCOREHEADER, 40 for BITMAPINFOHEADER, 108
//! for BITMAPV4HEADER and 124 for BITMAPV5HEADER, the sums of the fields
//! each structure lists, the V4 header being "BitmapInfoHeader (40 bytes)"
//! and four masks, a colour space, "Endpoints (36 bytes)" and three gamma
//! values (`[[source: ms-wmf-bitmapv4header]]`), and the V5 one that and
//! four more (`[[source: ms-wmf-bitmapv5header]]`). A header of any other
//! size is refused, [`BmpError::Unsupported`].
//!
//! **The header.** For `biHeight`, "if biHeight is positive, the bitmap is a
//! bottom-up DIB with the origin at the lower left corner. If biHeight is
//! negative, the bitmap is a top-down DIB with the origin at the upper left
//! corner", and "For compressed formats, biHeight must be positive"
//! (`[[source: ms-bitmapinfoheader]]`); the width "MUST be positive" and
//! the height "MUST NOT be zero" (`[[source: ms-wmf-bitmapinfoheader]]`).
//! `biPlanes` "must be set to 1". A core header's sides are unsigned words,
//! "The origin of the bitmap is the lower-left corner", and its bit count
//! "must be 1, 4, 8, or 24" (`[[source: ms-bitmapcoreheader]]`,
//! `[[source: ms-bitmapcoreinfo]]`). The compression's values are
//! `BI_RGB = 0x0000`, `BI_RLE8 = 0x0001`, `BI_RLE4 = 0x0002` and
//! `BI_BITFIELDS = 0x0003` (`[[source: ms-wmf-compression]]`).
//!
//! **The colour table.** "If biClrUsed is zero, the array contains the
//! maximum number of colors for the given bitdepth; that is, 2^biBitCount
//! colors" (`[[source: ms-bitmapinfoheader]]`), and past that maximum "the
//! maximum color table size SHOULD be assumed"
//! (`[[source: ms-wmf-bitmapinfoheader]]`); an entry is an RGBQUAD, its
//! bytes `rgbBlue`, `rgbGreen`, `rgbRed` and `rgbReserved`
//! (`[[source: ms-rgbquad]]`). A core header's table is RGBTRIPLEs,
//! `rgbtBlue`, `rgbtGreen`, `rgbtRed` (`[[source: ms-rgbtriple]]`), with
//! "two entries" at 1 bit and "up to 16" and "up to 256" at 4 and 8
//! (`[[source: ms-bitmapcoreinfo]]`): as many as lie between the header and
//! `bfOffBits`, to that maximum. At 16 bits and more a table is "used for
//! optimizing colors used on palette-based devices" and not read.
//!
//! **The pixels.** "The scan lines are DWORD aligned, except for
//! RLE-compressed bitmaps" (`[[source: ms-bitmap-header-types]]`). At 1, 4
//! and 8 bits a pixel is an index, its first pixel in a byte's high-order
//! bits: "if the first byte in the bitmap is 0x1F, the byte represents two
//! pixels. The first pixel contains the color in the second table entry, and
//! the second pixel contains the color in the sixteenth table entry"
//! (`[[source: ms-bitmapv5header]]`). At 24, "Each 3-byte triplet in the
//! bitmap array represents the relative intensities of blue, green, and
//! red". At 16 under `BI_RGB`, "The value for blue is in the least
//! significant five bits, followed by five bits each for green and red. The
//! most significant bit is not used"; at 32, "The value for blue is in the
//! least significant 8 bits, followed by 8 bits each for green and red. The
//! high byte in each DWORD is not used", so every such pixel is opaque.
//!
//! **The masks.** `BI_BITFIELDS` is "Valid for 16-bpp and 32-bpp bitmaps"
//! and "bits set in each DWORD mask must be contiguous". After a
//! BITMAPINFOHEADER, "the bitmap uses three DWORD color masks (red, green,
//! and blue, respectively)" (`[[source: ms-bitmapinfoheader]]`), and the
//! colour table follows them; a V4 or V5 header carries its own, red,
//! green, blue and alpha, in its fields (`[[source: ms-bitmapv4header]]`,
//! `[[source: ms-bitmapv5header]]`). The two texts conflict there: the
//! header-types page has the masks "immediately follow the BITMAPINFOHEADER,
//! BITMAPV4HEADER, and BITMAPV5HEADER structures", where the V4 and V5
//! structures hold them; the structures are followed. The alpha mask is
//! read only with `BI_BITFIELDS`, beside the three it is listed with; under
//! `BI_RGB` the high byte "is not used".
//!
//! **Run-length encoding** (`[[source: ms-bitmap-compression]]`): "the
//! first byte specifies the number of consecutive pixels to be drawn using
//! the color index contained in the second byte", a first byte of zero an
//! escape, its second "End of line", "End of bitmap" or "Delta. The 2 bytes
//! following the escape contain unsigned values indicating the offset to the
//! right and up of the next pixel from the current position", and from 3 an
//! absolute run, "each run must be zero-padded to end on a 16-bit word
//! boundary". In `BI_RLE4` "The first of the pixels is drawn using the
//! color specified by the high-order 4 bits, the second is drawn using the
//! color in the low-order 4 bits", alternating. The data runs from
//! `bfOffBits` for `biSizeImage` bytes, or to the file's end where that is
//! zero or past it.
//!
//! **What the text leaves to the decoder, and whose choice this is.** A
//! pixel no run reaches -- one a delta or an end of line skips, or one past
//! an end of bitmap -- is clear, the terminal showing through, as a
//! destination the bitmap does not write keeps what it held. A run past its
//! row's end draws to the end and no further. A colour index past the
//! table, which no table entry gives a colour, refuses the picture,
//! [`BmpError::Palette`], as a PNG's does. A mask of fewer than 8 bits is
//! scaled to the full range, `v * 255 / max` rounded to nearest, as a
//! sub-byte PNG grey is, and one of more by its high eight, as a 16-bit
//! PNG's sample is; a mask of no bits gives 0 for a colour and opaque for
//! the alpha. The alpha is straight, the colour not multiplied by it.
//!
//! **What is refused, and counted where the caller counts.** `BI_JPEG` and
//! `BI_PNG`, whose bits are another format's file, and the CMYK codes, each
//! [`BmpError::Unsupported`]; a header that breaks a rule above
//! [`BmpError::Malformed`]; a side past 65535 pixels
//! [`BmpError::TooLarge`]. A colour profile, linked or embedded, is not
//! read, and a linked one's path never opened.
//!
//! **Malformed input never panics**, and the work is bounded by the
//! picture and the file: every read is checked against the file, every
//! product is checked, the uncompressed rows are read once each, and a run
//! draws at most to its row's end. A file cut short in its pixels shows
//! what arrived, the rest clear; one cut short in its headers or its
//! colour table is refused.

use crate::png::Size;
use crate::{Chan, Pixel};

/// Why a bitmap could not be decoded. Every variant is a property of the
/// bytes, but for [`BmpError::Buffer`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BmpError {
    /// Not a bitmap file, or headers that break the format's rules or run
    /// past the file.
    Malformed,
    /// A header size, or a compression, this decoder does not draw.
    Unsupported,
    /// A side past 65535 pixels.
    TooLarge,
    /// A colour index past the colour table.
    Palette,
    /// A caller buffer smaller than the picture.
    Buffer,
}

/// The largest side drawn, in pixels: an image side's range.
const MAX_SIDE: usize = 65535;

/// `BI_RGB`, `BI_RLE8`, `BI_RLE4`, `BI_BITFIELDS`, and the codes refused.
const BI_RGB: u32 = 0;
const BI_RLE8: u32 = 1;
const BI_RLE4: u32 = 2;
const BI_BITFIELDS: u32 = 3;
const BI_JPEG: u32 = 4;
const BI_PNG: u32 = 5;
const BI_CMYK: u32 = 0x0B;
const BI_CMYKRLE8: u32 = 0x0C;
const BI_CMYKRLE4: u32 = 0x0D;

/// The file header's size, and so where the bitmap header starts.
const FILE_HEADER: usize = 14;

/// The four header sizes.
const CORE: usize = 12;
const INFO: usize = 40;
const V4: usize = 108;
const V5: usize = 124;

/// Which way the rows are stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rows {
    /// The first row stored is the bottom one shown.
    BottomUp,
    /// The first row stored is the top one shown.
    TopDown,
}

/// How the pixels are coded.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coding {
    /// Uncompressed: indexes at 1, 4 and 8 bits, colours through the masks
    /// at 16 and 32, triplets at 24.
    Raw,
    /// `BI_RLE8`.
    Rle8,
    /// `BI_RLE4`.
    Rle4,
}

/// The colour table: where it starts, its entries, and each entry's bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Table {
    at: usize,
    n: usize,
    entry: usize,
}

/// **One colour mask**: its bits, where they start, and how many there
/// are.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Mask {
    mask: u32,
    shift: u32,
    bits: u32,
}

impl Mask {
    const NONE: Mask = Mask {
        mask: 0,
        shift: 0,
        bits: 0,
    };

    /// The mask `m`, refused where its set bits are not contiguous.
    fn of(m: u32) -> Result<Mask, BmpError> {
        if m == 0 {
            return Ok(Mask::NONE);
        }
        let shift = m.trailing_zeros();
        let run = m >> shift;
        let bits = run.trailing_ones();
        match run.checked_shr(bits).unwrap_or(0) {
            0 => Ok(Mask {
                mask: m,
                shift,
                bits,
            }),
            _ => Err(BmpError::Malformed),
        }
    }

    /// Its component of the pixel `px` at 8 bits, `None` for a mask of no
    /// bits.
    fn take(self, px: u32) -> Option<Chan> {
        if self.bits == 0 {
            return None;
        }
        let v = (px & self.mask) >> self.shift;
        let byte = match self.bits {
            8.. => v >> (self.bits - 8),
            n => {
                let max = (1u32 << n) - 1;
                (v * 255 + max / 2) / max
            }
        };
        Some(Chan::from_u8(u8::try_from(byte).unwrap_or(u8::MAX)))
    }
}

/// The red, green, blue and alpha masks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Masks {
    red: Mask,
    green: Mask,
    blue: Mask,
    alpha: Mask,
}

impl Masks {
    /// The colour of the pixel `px`.
    fn colour(&self, px: u32) -> Pixel {
        let c = |m: Mask| m.take(px).unwrap_or(Chan::from_u8(0));
        Pixel::straight(
            self.alpha.take(px).unwrap_or(Chan::from_u8(0xFF)),
            c(self.red),
            c(self.green),
            c(self.blue),
        )
    }
}

/// **A parsed bitmap**: its size, its rows' order, its coding, its colour
/// table and masks, and where its pixels are.
#[derive(Clone, Copy, Debug)]
pub struct Bmp<'a> {
    data: &'a [u8],
    width: usize,
    height: usize,
    rows: Rows,
    depth: usize,
    coding: Coding,
    table: Table,
    masks: Masks,
    /// The pixels: from `bfOffBits` to the end of the data.
    pixels: &'a [u8],
}

/// The colour of each index a table can hold, `None` past the table.
type Colours = [Option<Pixel>; 256];

fn le16(d: &[u8], at: usize) -> Option<u16> {
    let b = d.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn le32(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn offset(d: &[u8], at: usize) -> Result<usize, BmpError> {
    le32(d, at)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or(BmpError::Malformed)
}

impl<'a> Bmp<'a> {
    /// Whether `d` begins as a bitmap file does, "BM".
    pub fn sniff(d: &[u8]) -> bool {
        d.starts_with(b"BM")
    }

    /// **The file header, the bitmap header, its masks and its colour
    /// table**, each checked against the file and the rules above.
    pub fn parse(data: &'a [u8]) -> Result<Bmp<'a>, BmpError> {
        if !Bmp::sniff(data) {
            return Err(BmpError::Malformed);
        }
        let off = offset(data, 10)?;
        let size = offset(data, FILE_HEADER)?;
        let end = FILE_HEADER.checked_add(size).ok_or(BmpError::Malformed)?;
        if data.len() < end {
            return Err(BmpError::Malformed);
        }
        let word = |at| le16(data, at).map(usize::from).ok_or(BmpError::Malformed);
        let mut bmp = match size {
            CORE => {
                if word(22)? != 1 {
                    return Err(BmpError::Malformed);
                }
                let depth = word(24)?;
                if !matches!(depth, 1 | 4 | 8 | 24) {
                    return Err(BmpError::Malformed);
                }
                let room = off.saturating_sub(end) / 3;
                Bmp {
                    data,
                    width: word(18)?,
                    height: word(20)?,
                    rows: Rows::BottomUp,
                    depth,
                    coding: Coding::Raw,
                    table: Table {
                        at: end,
                        n: match depth {
                            24 => 0,
                            d => room.min(1 << d),
                        },
                        entry: 3,
                    },
                    masks: Masks {
                        red: Mask::NONE,
                        green: Mask::NONE,
                        blue: Mask::NONE,
                        alpha: Mask::NONE,
                    },
                    pixels: &[],
                }
            }
            INFO | V4 | V5 => Bmp::info(data, size)?,
            _ => return Err(BmpError::Unsupported),
        };
        if bmp.width == 0 || bmp.height == 0 {
            return Err(BmpError::Malformed);
        }
        if bmp.width > MAX_SIDE || bmp.height > MAX_SIDE {
            return Err(BmpError::TooLarge);
        }
        let table_end = bmp
            .table
            .n
            .checked_mul(bmp.table.entry)
            .and_then(|n| n.checked_add(bmp.table.at))
            .ok_or(BmpError::Malformed)?;
        if table_end > data.len() || off > data.len() {
            return Err(BmpError::Malformed);
        }
        let mut pixels = &data[off..];
        if matches!(bmp.coding, Coding::Rle8 | Coding::Rle4) {
            let n = offset(data, 34)?;
            if n != 0 && n < pixels.len() {
                pixels = &pixels[..n];
            }
        }
        bmp.pixels = pixels;
        Ok(bmp)
    }

    /// A BITMAPINFOHEADER, or the V4 or V5 header that extends it, of
    /// `size` bytes.
    fn info(data: &'a [u8], size: usize) -> Result<Bmp<'a>, BmpError> {
        let long = |at| {
            le32(data, at)
                .map(|v| i32::from_le_bytes(v.to_le_bytes()))
                .ok_or(BmpError::Malformed)
        };
        let dword = |at| le32(data, at).ok_or(BmpError::Malformed);
        let (width, height) = (long(18)?, long(22)?);
        let rows = match height < 0 {
            true => Rows::TopDown,
            false => Rows::BottomUp,
        };
        let planes = le16(data, 26).ok_or(BmpError::Malformed)?;
        let depth = usize::from(le16(data, 28).ok_or(BmpError::Malformed)?);
        let compression = dword(30)?;
        let used = usize::try_from(dword(46)?).unwrap_or(usize::MAX);
        if planes != 1 || width <= 0 {
            return Err(BmpError::Malformed);
        }
        let coding = match (compression, depth) {
            (BI_RGB, 1 | 4 | 8 | 16 | 24 | 32) | (BI_BITFIELDS, 16 | 32) => Coding::Raw,
            (BI_RLE8, 8) => Coding::Rle8,
            (BI_RLE4, 4) => Coding::Rle4,
            (BI_JPEG | BI_PNG | BI_CMYK | BI_CMYKRLE8 | BI_CMYKRLE4, _) => {
                return Err(BmpError::Unsupported);
            }
            _ => return Err(BmpError::Malformed),
        };
        if coding != Coding::Raw && rows == Rows::TopDown {
            return Err(BmpError::Malformed);
        }
        let header_end = FILE_HEADER + size;
        let (masks, table_at) = match (compression, depth) {
            (BI_BITFIELDS, _) => {
                let alpha = match size {
                    INFO => Mask::NONE,
                    _ => Mask::of(dword(FILE_HEADER + 52)?)?,
                };
                let masks = Masks {
                    red: Mask::of(dword(FILE_HEADER + 40)?)?,
                    green: Mask::of(dword(FILE_HEADER + 44)?)?,
                    blue: Mask::of(dword(FILE_HEADER + 48)?)?,
                    alpha,
                };
                let after = match size {
                    INFO => header_end + 12,
                    _ => header_end,
                };
                (masks, after)
            }
            (_, 16) => (Masks::fixed(0x7C00, 0x03E0, 0x001F)?, header_end),
            (_, 32) => (
                Masks::fixed(0x00FF_0000, 0x0000_FF00, 0x0000_00FF)?,
                header_end,
            ),
            _ => (Masks::fixed(0, 0, 0)?, header_end),
        };
        let n = match depth {
            1 | 4 | 8 => {
                let max = 1usize << depth;
                match used {
                    0 => max,
                    u => u.min(max),
                }
            }
            _ => 0,
        };
        Ok(Bmp {
            data,
            width: usize::try_from(width).map_err(|_| BmpError::Malformed)?,
            height: usize::try_from(height.unsigned_abs()).map_err(|_| BmpError::TooLarge)?,
            rows,
            depth,
            coding,
            table: Table {
                at: table_at,
                n,
                entry: 4,
            },
            masks,
            pixels: &[],
        })
    }

    /// The picture's size.
    pub fn size(&self) -> Size {
        Size {
            width: u32::try_from(self.width).unwrap_or(u32::MAX),
            height: u32::try_from(self.height).unwrap_or(u32::MAX),
        }
    }

    /// The colour of every index the table holds; the rest stay `None`.
    fn colours(&self) -> Colours {
        let mut out = [None; 256];
        let t = self.table;
        for (i, c) in out.iter_mut().enumerate().take(t.n) {
            let at = t.at + i * t.entry;
            if let Some(&[b, g, r]) = self.data.get(at..at + 3) {
                *c = Some(Pixel::opaque(
                    Chan::from_u8(r),
                    Chan::from_u8(g),
                    Chan::from_u8(b),
                ));
            }
        }
        out
    }

    /// **The picture decoded into `out`**, [`Bmp::size`] pixels, row by
    /// row from the top, straight ARGB; a pixel the data does not reach is
    /// clear.
    pub fn decode(&self, out: &mut [Pixel]) -> Result<Size, BmpError> {
        let area = self
            .width
            .checked_mul(self.height)
            .ok_or(BmpError::Buffer)?;
        let out = out.get_mut(..area).ok_or(BmpError::Buffer)?;
        out.fill(Pixel::CLEAR);
        let colours = self.colours();
        match self.coding {
            Coding::Raw => self.raw(out, &colours)?,
            Coding::Rle8 | Coding::Rle4 => self.rle(out, &colours)?,
        }
        Ok(self.size())
    }

    /// The row stored `stored`th, counted from the first stored, where it
    /// is shown in `out`.
    fn row<'o>(&self, out: &'o mut [Pixel], stored: usize) -> &'o mut [Pixel] {
        let y = match self.rows {
            Rows::BottomUp => self.height - 1 - stored,
            Rows::TopDown => stored,
        };
        &mut out[y * self.width..(y + 1) * self.width]
    }

    /// The uncompressed rows, each `ceil(width * depth / 32)` DWORDs.
    fn raw(&self, out: &mut [Pixel], colours: &Colours) -> Result<(), BmpError> {
        let stride = self
            .width
            .checked_mul(self.depth)
            .map(|b| b.div_ceil(32) * 4)
            .ok_or(BmpError::Buffer)?;
        for (stored, line) in self.pixels.chunks(stride).take(self.height).enumerate() {
            let row = self.row(out, stored);
            match self.depth {
                1 | 4 | 8 => {
                    let d = self.depth;
                    let low = (1usize << d) - 1;
                    for (x, px) in row.iter_mut().enumerate() {
                        let bit = x * d;
                        let Some(&byte) = line.get(bit / 8) else {
                            break;
                        };
                        let i = (usize::from(byte) >> (8 - d - bit % 8)) & low;
                        *px = colours[i].ok_or(BmpError::Palette)?;
                    }
                }
                16 => {
                    for (px, w) in row.iter_mut().zip(line.as_chunks::<2>().0) {
                        *px = self.masks.colour(u32::from(u16::from_le_bytes(*w)));
                    }
                }
                24 => {
                    for (px, &[b, g, r]) in row.iter_mut().zip(line.as_chunks::<3>().0) {
                        *px = Pixel::opaque(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
                    }
                }
                _ => {
                    for (px, w) in row.iter_mut().zip(line.as_chunks::<4>().0) {
                        *px = self.masks.colour(u32::from_le_bytes(*w));
                    }
                }
            }
        }
        Ok(())
    }

    /// **The runs of `BI_RLE8` or `BI_RLE4`**, from the bottom row up, to
    /// the end of bitmap or the data's end.
    fn rle(&self, out: &mut [Pixel], colours: &Colours) -> Result<(), BmpError> {
        let d = self.pixels;
        let four = self.coding == Coding::Rle4;
        let (mut i, mut x, mut y) = (0usize, 0usize, 0usize);
        // The index the `k`th pixel of a run takes from `byte`: the byte
        // itself, or its high and low four bits in turn.
        let nibble = |byte: u8, k: usize| match (four, k % 2) {
            (false, _) => byte,
            (true, 0) => byte >> 4,
            (true, _) => byte & 0x0F,
        };
        while y < self.height {
            let (Some(&n), Some(&b)) = (d.get(i), d.get(i + 1)) else {
                break;
            };
            i += 2;
            let row = self.row(out, y);
            match (n, b) {
                (0, 0) => {
                    x = 0;
                    y += 1;
                }
                (0, 1) => break,
                (0, 2) => {
                    let (Some(&dx), Some(&dy)) = (d.get(i), d.get(i + 1)) else {
                        break;
                    };
                    i += 2;
                    x = x.saturating_add(usize::from(dx));
                    y += usize::from(dy);
                }
                (0, count) => {
                    let count = usize::from(count);
                    let len = match four {
                        true => count.div_ceil(2),
                        false => count,
                    };
                    let run = d.get(i..).unwrap_or(&[]);
                    let run = &run[..len.min(run.len())];
                    for (k, px) in row.iter_mut().skip(x).take(count).enumerate() {
                        let byte = match four {
                            true => run.get(k / 2),
                            false => run.get(k),
                        };
                        let Some(&byte) = byte else {
                            break;
                        };
                        *px = colours[usize::from(nibble(byte, k))].ok_or(BmpError::Palette)?;
                    }
                    x = x.saturating_add(count);
                    i += len + len % 2;
                }
                (count, _) => {
                    let count = usize::from(count);
                    for (k, px) in row.iter_mut().skip(x).take(count).enumerate() {
                        *px = colours[usize::from(nibble(b, k))].ok_or(BmpError::Palette)?;
                    }
                    x = x.saturating_add(count);
                }
            }
        }
        Ok(())
    }
}

impl Masks {
    /// The masks a `BI_RGB` pixel's layout fixes, with no alpha.
    fn fixed(red: u32, green: u32, blue: u32) -> Result<Masks, BmpError> {
        Ok(Masks {
            red: Mask::of(red)?,
            green: Mask::of(green)?,
            blue: Mask::of(blue)?,
            alpha: Mask::NONE,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_reads_its_bits_and_refuses_a_gap() {
        assert_eq!(
            Mask::of(0x0000_F800).map(|m| (m.shift, m.bits)),
            Ok((11, 5))
        );
        assert_eq!(
            Mask::of(0xFFFF_FFFF).map(|m| (m.shift, m.bits)),
            Ok((0, 32))
        );
        assert_eq!(Mask::of(0x0000_0101), Err(BmpError::Malformed));
        let five = Mask::of(0x1F).unwrap();
        assert_eq!(five.take(31).map(Chan::get), Some(255));
        assert_eq!(five.take(1).map(Chan::get), Some(8));
        let ten = Mask::of(0x3FF).unwrap();
        assert_eq!(ten.take(0x3FF).map(Chan::get), Some(255));
        assert_eq!(ten.take(0x200).map(Chan::get), Some(128));
        assert_eq!(Mask::NONE.take(0xFFFF_FFFF), None);
    }
}
