//! PNG, decoded to RGBA8 (`[[source: w3c-png-2003]]`; the compression is
//! zlib over deflate, `[[source: rfc1950-zlib]]` and `[[source: rfc1951-deflate]]`).
//!
//! **Why a PNG decoder lives in a font crate.** Colour emoji arrive as PNG
//! strikes -- Apple Color Emoji's `sbix` carries 31,707 of them on this Mac,
//! Noto Color Emoji's `CBDT` carries thousands more -- and "no engine
//! dependencies" covers image libraries too. The decoder is the smallest
//! complete one: every colour type and bit depth the specification allows,
//! the five filters, both interlace methods, stored, fixed and dynamic
//! deflate blocks, CRC-32 on every chunk and Adler-32 on the stream.
//!
//! **Adam7 is read, because PNG requires it**: "Decoders are required to be
//! able to read interlaced images" (13.10; 15.2.3 asks for "all the
//! standardized ... interlace methods"). An interlaced image is seven
//! reduced images in one zlib stream, each filtered on its own -- "the
//! entire prior scanline ... shall be treated as being zeroes for the first
//! scanline of a reduced image" (9.2) -- and an empty one carries nothing:
//! "no filter type bytes are present in an empty reduced image" (13.10).
//! Each is unfiltered in place in the scratch buffer and its pixels written
//! straight to where the pass puts them in the output, so interlacing needs
//! no third buffer.
//!
//! **No allocation.** The caller provides two buffers sized by
//! [`Png::raw_len`] and [`Png::rgba_len`]: the inflated, still-filtered
//! scanlines land in the first, and the reconstructed, expanded pixels in
//! the second. A renderer keeps one scratch buffer and decodes every strike
//! through it, which is the same shape as the core's caller-provided
//! `Storage`.
//!
//! **Huffman decoding is the canonical bit-at-a-time walk** the
//! specification describes in 3.2.2: codes of each length are consecutive,
//! so a symbol is found by counting how many codes of each length precede
//! it. It is a few dozen lines, needs a 16-entry count array and a 288-entry
//! symbol array, and costs a handful of instructions per bit -- an emoji
//! strike is a few kilobytes, decoded once per glyph and then cached in the
//! atlas, so the table-driven fast forms are not worth their size here.

use crate::bytes::{Bytes, Len, Offset};

/// A decoded image's dimensions, in pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

/// Why a PNG could not be decoded. Every variant is a property of the bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PngError {
    /// Not a PNG: bad signature, no `IHDR`, or a chunk that lies about
    /// its length.
    Malformed,
    /// A chunk's CRC-32 does not match its bytes.
    Crc,
    /// A colour type, bit depth or interlace method the specification
    /// does not allow.
    Header,
    /// A caller buffer is smaller than [`Png::raw_len`] or [`Png::rgba_len`].
    Buffer,
    /// The zlib stream is malformed: a bad header, an invalid block, a
    /// distance before the start, a length that overruns the image.
    Deflate,
    /// The Adler-32 in the zlib trailer does not match the inflated bytes.
    Adler,
    /// A palette index with no palette entry, or a palette image with no
    /// `PLTE`.
    Palette,
}

/// The colour type from `IHDR`, with the channel count it implies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Gray,
    Rgb,
    Palette,
    GrayAlpha,
    Rgba,
}

impl Color {
    fn parse(v: u8) -> Option<Color> {
        match v {
            0 => Some(Color::Gray),
            2 => Some(Color::Rgb),
            3 => Some(Color::Palette),
            4 => Some(Color::GrayAlpha),
            6 => Some(Color::Rgba),
            _ => None,
        }
    }

    fn channels(self) -> u32 {
        match self {
            Color::Gray | Color::Palette => 1,
            Color::GrayAlpha => 2,
            Color::Rgb => 3,
            Color::Rgba => 4,
        }
    }

    /// The bit depths the specification allows for this colour type
    /// (11.2.2, table 11.1).
    fn allows(self, depth: u8) -> bool {
        match self {
            Color::Gray => matches!(depth, 1 | 2 | 4 | 8 | 16),
            Color::Palette => matches!(depth, 1 | 2 | 4 | 8),
            Color::Rgb | Color::GrayAlpha | Color::Rgba => matches!(depth, 8 | 16),
        }
    }
}

/// The interlace method from `IHDR` (8.2): the null method, or Adam7.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Interlace {
    Null,
    Adam7,
}

/// Where one reduced image's pixels sit in the whole image (8.2): its
/// first column and row, and the distance to the next of each.
#[derive(Clone, Copy)]
struct Pass {
    col: Lattice,
    row: Lattice,
}

/// One axis of a [`Pass`]. The first pixel and the step are the same width
/// and a swapped pair decodes to a plausible wrong picture, as `Run` and
/// `Back` would, so they do not share a type.
#[derive(Clone, Copy)]
struct Lattice {
    first: First,
    step: Step,
}

#[derive(Clone, Copy)]
struct First(u32);

#[derive(Clone, Copy)]
struct Step(u32);

impl Pass {
    /// The null method's one reduced image: the whole image.
    const WHOLE: Pass = Pass {
        col: Lattice {
            first: First(0),
            step: Step(1),
        },
        row: Lattice {
            first: First(0),
            step: Step(1),
        },
    };

    /// This pass's reduced image of an image of `full` pixels, or `None`
    /// when it has no columns or no rows: "If the reference image contains
    /// fewer than five columns or fewer than five rows, some passes will be
    /// empty" (8.2).
    fn reduced(self, full: Size) -> Option<Size> {
        let along = |n: u32, axis: Lattice| n.saturating_sub(axis.first.0).div_ceil(axis.step.0);
        let size = Size {
            width: along(full.width, self.col),
            height: along(full.height, self.row),
        };
        (size.width != 0 && size.height != 0).then_some(size)
    }
}

/// 8.2's pattern, as the specification prints it: the pass that carries
/// each pixel, "replicating the following 8-by-8 pattern over the entire
/// image, starting at the upper left corner".
const ADAM7_PATTERN: [[u8; 8]; 8] = [
    [1, 6, 4, 6, 2, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [3, 6, 4, 6, 3, 6, 4, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
    [5, 6, 5, 6, 5, 6, 5, 6],
    [7, 7, 7, 7, 7, 7, 7, 7],
];

/// The seven passes, read off [`ADAM7_PATTERN`] at compile time: a pass
/// starts at its first cell in reading order and steps to the next cell of
/// its number along that row and down that column of the replicated
/// pattern. The build then rebuilds a 16-by-16 area from the lattices and
/// fails unless each cell lands in exactly the pass the pattern names, so
/// the table cannot drift from the text it was derived from.
const ADAM7: [Pass; 7] = {
    let mut passes = [Pass::WHOLE; 7];
    let mut k = 0;
    let mut p: u8 = 1;
    while k < 7 {
        // The first cell of pass `p`, in reading order. Each index is
        // counted twice, as the `usize` that indexes the pattern and the
        // `u32` the lattice holds: a `const` block cannot call `try_from`.
        let (mut r, mut c, mut r32, mut c32) = (0, 0, 0u32, 0u32);
        while ADAM7_PATTERN[r][c] != p {
            c += 1;
            c32 += 1;
            if c == 8 {
                (c, c32) = (0, 0);
                r += 1;
                r32 += 1;
            }
        }
        // The next cell of `p` along the row and down the column, in the
        // replicated pattern: at most a tile on, where the cell repeats.
        let (mut dc, mut dr, mut dc32, mut dr32) = (1, 1, 1u32, 1u32);
        while ADAM7_PATTERN[r][(c + dc) % 8] != p {
            dc += 1;
            dc32 += 1;
        }
        while ADAM7_PATTERN[(r + dr) % 8][c] != p {
            dr += 1;
            dr32 += 1;
        }
        passes[k] = Pass {
            col: Lattice {
                first: First(c32),
                step: Step(dc32),
            },
            row: Lattice {
                first: First(r32),
                step: Step(dr32),
            },
        };
        // Every cell of four tiles, so a step that is right inside one tile
        // and wrong across two cannot pass: in pass `p`'s lattice exactly
        // when the replicated pattern says `p`.
        let mut y = 0;
        while y < 16 {
            let mut x = 0;
            while x < 16 {
                let on = y >= r && (y - r) % dr == 0 && x >= c && (x - c) % dc == 0;
                assert!(on == (ADAM7_PATTERN[y % 8][x % 8] == p));
                x += 1;
            }
            y += 1;
        }
        k += 1;
        p += 1;
    }
    passes
};

/// A parsed header: enough to size the buffers and to walk the chunks.
#[derive(Clone, Copy)]
pub struct Png<'a> {
    data: Bytes<'a>,
    size: Size,
    depth: u8,
    color: Color,
    interlace: Interlace,
    palette: Option<Bytes<'a>>,
    trns: Option<Bytes<'a>>,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The CRC-32 table, built at compile time from the polynomial the
/// specification gives (5.5); a chunk whose CRC disagrees is refused, which
/// is what every reference decoder does by default.
const CRC_TABLE: [u32; 256] = {
    let mut t = [0u32; 256];
    let mut n: u32 = 0;
    while n < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        // The one cast in this file: a `const fn` cannot call `try_from`,
        // and `n` is below 256 by the loop bound.
        t[n as usize] = c;
        n += 1;
    }
    t
};

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c = CRC_TABLE[usize::from((c ^ u32::from(b)).to_le_bytes()[0])] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// One chunk: its type and its data, already CRC-checked.
struct Chunk<'a> {
    kind: [u8; 4],
    data: Bytes<'a>,
    next: Offset,
}

fn chunk(data: Bytes<'_>, at: Offset) -> Result<Chunk<'_>, PngError> {
    let len: u32 = data.read(at).ok_or(PngError::Malformed)?;
    let kind_at = at.plus(Len::of(4)).ok_or(PngError::Malformed)?;
    let body_at = kind_at.plus(Len::of(4)).ok_or(PngError::Malformed)?;
    let crc_at = body_at.plus(Len::of(len)).ok_or(PngError::Malformed)?;
    let next = crc_at.plus(Len::of(4)).ok_or(PngError::Malformed)?;
    // Type and body together are what the CRC covers.
    let covered = data
        .slice(
            kind_at,
            Len::of(len.checked_add(4).ok_or(PngError::Malformed)?),
        )
        .ok_or(PngError::Malformed)?;
    let stored: u32 = data.read(crc_at).ok_or(PngError::Malformed)?;
    if crc32(covered.as_slice()) != stored {
        return Err(PngError::Crc);
    }
    let kind_bytes = data.slice(kind_at, Len::of(4)).ok_or(PngError::Malformed)?;
    let mut kind = [0u8; 4];
    kind.copy_from_slice(kind_bytes.as_slice());
    Ok(Chunk {
        kind,
        data: data
            .slice(body_at, Len::of(len))
            .ok_or(PngError::Malformed)?,
        next,
    })
}

impl<'a> Png<'a> {
    /// Parses the signature, `IHDR`, and the `PLTE`/`tRNS` chunks that
    /// precede the image data; checks every chunk's CRC on the way.
    pub fn parse(bytes: &'a [u8]) -> Result<Png<'a>, PngError> {
        let data = Bytes::new(bytes);
        if bytes.get(..8) != Some(&SIGNATURE) {
            return Err(PngError::Malformed);
        }
        let ihdr = chunk(data, Offset::of(8u32))?;
        if ihdr.kind != *b"IHDR" || ihdr.data.len().value() != 13 {
            return Err(PngError::Malformed);
        }
        let width: u32 = ihdr.data.read(Offset::ZERO).ok_or(PngError::Malformed)?;
        let height: u32 = ihdr
            .data
            .read(Offset::of(4u32))
            .ok_or(PngError::Malformed)?;
        let depth: u8 = ihdr
            .data
            .read(Offset::of(8u32))
            .ok_or(PngError::Malformed)?;
        let color: u8 = ihdr
            .data
            .read(Offset::of(9u32))
            .ok_or(PngError::Malformed)?;
        let compression: u8 = ihdr
            .data
            .read(Offset::of(10u32))
            .ok_or(PngError::Malformed)?;
        let filter: u8 = ihdr
            .data
            .read(Offset::of(11u32))
            .ok_or(PngError::Malformed)?;
        let interlace: u8 = ihdr
            .data
            .read(Offset::of(12u32))
            .ok_or(PngError::Malformed)?;
        let color = Color::parse(color).ok_or(PngError::Header)?;
        if !color.allows(depth) || compression != 0 || filter != 0 {
            return Err(PngError::Header);
        }
        // 11.2.2: width and height are non-zero and below 2^31.
        if width == 0 || height == 0 || width > 0x7FFF_FFFF || height > 0x7FFF_FFFF {
            return Err(PngError::Header);
        }
        // 8.2 defines methods 0 and 1; 15.2.3, "Unexpected values in
        // fields of known chunks ... are treated as errors."
        let interlace = match interlace {
            0 => Interlace::Null,
            1 => Interlace::Adam7,
            _ => return Err(PngError::Header),
        };
        let mut png = Png {
            data,
            size: Size { width, height },
            depth,
            color,
            interlace,
            palette: None,
            trns: None,
        };
        let mut at = ihdr.next;
        loop {
            let c = chunk(data, at)?;
            match &c.kind {
                b"PLTE" => png.palette = Some(c.data),
                b"tRNS" => png.trns = Some(c.data),
                b"IDAT" | b"IEND" => break,
                _ => {}
            }
            at = c.next;
        }
        if color == Color::Palette && png.palette.is_none() {
            return Err(PngError::Palette);
        }
        Ok(png)
    }

    pub fn size(&self) -> Size {
        self.size
    }

    /// Bits per pixel in the filtered scanlines.
    fn bits_per_pixel(&self) -> u32 {
        u32::from(self.depth) * self.color.channels()
    }

    /// Bytes per scanline of an image `size` wide, without the filter
    /// byte; a scanline of a reduced image is padded to a whole byte on its
    /// own (7.2).
    fn row_bytes(&self, size: Size) -> Option<u32> {
        let bits = self.bits_per_pixel().checked_mul(size.width)?;
        bits.checked_add(7).map(|b| b / 8)
    }

    /// The reduced images the scanlines are stored as, in order: the whole
    /// image under the null method, Adam7's seven passes under method 1.
    fn passes(&self) -> &'static [Pass] {
        match self.interlace {
            Interlace::Null => &[Pass::WHOLE],
            Interlace::Adam7 => &ADAM7,
        }
    }

    /// The bytes the inflated stream occupies: every non-empty reduced
    /// image's scanlines, each a filter byte plus [`Self::row_bytes`] at the
    /// reduced width. For a non-interlaced image that is `height` scanlines
    /// of the full width. An interlaced one has about 15/8 as many
    /// scanlines, so this is larger by a filter byte for each of the extra
    /// 7/8 `height` and, below 8 bits, up to one padding byte for every
    /// reduced scanline -- 2x3 grey is 11 bytes against 9. Size the scratch
    /// from this, never from `(row + 1) * height`.
    pub fn raw_len(&self) -> Option<usize> {
        let mut total = 0u32;
        for pass in self.passes() {
            let Some(size) = pass.reduced(self.size) else {
                continue;
            };
            let row = self.row_bytes(size)?.checked_add(1)?;
            total = total.checked_add(row.checked_mul(size.height)?)?;
        }
        usize::try_from(total).ok()
    }

    /// The bytes the decoded image occupies: four per pixel.
    pub fn rgba_len(&self) -> Option<usize> {
        let px = self.size.width.checked_mul(self.size.height)?;
        usize::try_from(px.checked_mul(4)?).ok()
    }

    /// Decodes into `rgba`, using `raw` as scratch for the inflated
    /// scanlines. Both must be at least the lengths above; extra is ignored.
    pub fn decode(&self, raw: &mut [u8], rgba: &mut [u8]) -> Result<Size, PngError> {
        let raw_len = self.raw_len().ok_or(PngError::Buffer)?;
        let rgba_len = self.rgba_len().ok_or(PngError::Buffer)?;
        let raw = raw.get_mut(..raw_len).ok_or(PngError::Buffer)?;
        let rgba = rgba.get_mut(..rgba_len).ok_or(PngError::Buffer)?;
        self.inflate(raw)?;
        if self.interlace == Interlace::Null {
            // The whole image as its one reduced image, called on its own:
            // sent through the loop below instead, a 1920x1080 RGB decode
            // retired 491 M instructions against 470 M this way and 477 M
            // before Adam7 (`baud-bench`'s `png_decode`; the record,
            // "png.rs reads Adam7"). `codes`, inflate's symbol loop, was
            // the same machine code in all three.
            self.unfilter(raw, self.size)?;
            self.expand(raw, rgba, self.size, Pass::WHOLE)?;
            return Ok(self.size);
        }
        // The reduced images follow one another in the stream, an empty
        // pass taking no bytes at all (13.10).
        let mut at = 0;
        for &pass in self.passes() {
            let Some(size) = pass.reduced(self.size) else {
                continue;
            };
            let stride = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
                .map_err(|_| PngError::Buffer)?
                + 1;
            let h = usize::try_from(size.height).map_err(|_| PngError::Buffer)?;
            let part = raw.get_mut(at..at + stride * h).ok_or(PngError::Buffer)?;
            self.unfilter(part, size)?;
            self.expand(part, rgba, size, pass)?;
            at += stride * h;
        }
        Ok(self.size)
    }

    /// The `IDAT` chunks in order, as one zlib stream, inflated into `raw`.
    fn inflate(&self, raw: &mut [u8]) -> Result<(), PngError> {
        let mut input = Idat::new(self.data)?;
        // RFC 1950: CMF and FLG; CM must be 8 (deflate), the pair must be a
        // multiple of 31, and a preset dictionary is not allowed in PNG.
        let cmf = input.byte().ok_or(PngError::Deflate)?;
        let flg = input.byte().ok_or(PngError::Deflate)?;
        if cmf & 0x0f != 8 || (u16::from(cmf) * 256 + u16::from(flg)) % 31 != 0 || flg & 0x20 != 0 {
            return Err(PngError::Deflate);
        }
        let mut out = Out { buf: raw, len: 0 };
        let mut bits = BitReader {
            input,
            bits: 0,
            count: 0,
        };
        loop {
            let last = bits.take(1)?;
            let kind = bits.take(2)?;
            match kind {
                0 => stored(&mut bits, &mut out)?,
                1 => {
                    let (lit, dist) = fixed_tables();
                    codes(&mut bits, &mut out, &lit, &dist)?;
                }
                2 => {
                    let (lit, dist) = dynamic_tables(&mut bits)?;
                    codes(&mut bits, &mut out, &lit, &dist)?;
                }
                _ => return Err(PngError::Deflate),
            }
            if last == 1 {
                break;
            }
        }
        if out.len != out.buf.len() {
            return Err(PngError::Deflate);
        }
        // Adler-32 over the inflated bytes, big-endian in the trailer.
        bits.align();
        let mut adler = [0u8; 4];
        for b in &mut adler {
            *b = bits.input.byte().ok_or(PngError::Deflate)?;
        }
        if adler32(out.buf) != u32::from_be_bytes(adler) {
            return Err(PngError::Adler);
        }
        Ok(())
    }

    /// Reconstructs every scanline of one reduced image of `size` pixels in
    /// place (9.2): the filter byte stays where it is and the row after it
    /// becomes the original bytes. `raw` starts at the image's first
    /// scanline, so its prior scanline is zero, as 9.2 requires of every
    /// reduced image.
    fn unfilter(&self, raw: &mut [u8], size: Size) -> Result<(), PngError> {
        let rb = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
            .map_err(|_| PngError::Buffer)?;
        let stride = rb + 1;
        // The "corresponding byte of the previous pixel": whole bytes for
        // depths of 8 and more, one byte for sub-byte pixels (9.2).
        let bpp = usize::try_from(self.bits_per_pixel().div_ceil(8))
            .map_err(|_| PngError::Buffer)?
            .max(1);
        let rows = usize::try_from(size.height).map_err(|_| PngError::Buffer)?;
        for r in 0..rows {
            let (before, rest) = raw.split_at_mut(r * stride);
            let prev: &[u8] = if r == 0 {
                &[]
            } else {
                &before[(r - 1) * stride + 1..r * stride]
            };
            let row = &mut rest[..stride];
            let filter = row[0];
            let line = &mut row[1..];
            match filter {
                0 => {}
                1 => {
                    for x in bpp..rb {
                        line[x] = line[x].wrapping_add(line[x - bpp]);
                    }
                }
                2 => {
                    // `prev` is empty on the first row, where "up" is zero.
                    for (x, up) in line
                        .iter_mut()
                        .zip(prev.iter().chain(core::iter::repeat(&0)))
                    {
                        *x = x.wrapping_add(*up);
                    }
                }
                3 => {
                    for x in 0..rb {
                        let a = if x >= bpp {
                            u16::from(line[x - bpp])
                        } else {
                            0
                        };
                        let b = u16::from(prev.get(x).copied().unwrap_or(0));
                        line[x] =
                            line[x].wrapping_add(u8::try_from((a + b) / 2).unwrap_or(u8::MAX));
                    }
                }
                4 => {
                    for x in 0..rb {
                        let a = if x >= bpp { line[x - bpp] } else { 0 };
                        let b = prev.get(x).copied().unwrap_or(0);
                        let c = if x >= bpp {
                            prev.get(x - bpp).copied().unwrap_or(0)
                        } else {
                            0
                        };
                        line[x] = line[x].wrapping_add(paeth(a, b, c));
                    }
                }
                _ => return Err(PngError::Deflate),
            }
        }
        Ok(())
    }

    /// Every pixel of one reduced image of `size` pixels to RGBA8, at the
    /// place `pass` puts it in the whole image: each scanline's pixels go
    /// to every `step`-th four-byte slot of their row from the first.
    fn expand(&self, raw: &[u8], rgba: &mut [u8], size: Size, pass: Pass) -> Result<(), PngError> {
        let rb = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
            .map_err(|_| PngError::Buffer)?;
        let w = usize::try_from(size.width).map_err(|_| PngError::Buffer)?;
        let h = usize::try_from(size.height).map_err(|_| PngError::Buffer)?;
        let full = usize::try_from(self.size.width).map_err(|_| PngError::Buffer)?;
        let at = |n: u32| usize::try_from(n).map_err(|_| PngError::Buffer);
        let (x0, dx) = (at(pass.col.first.0)?, at(pass.col.step.0)?);
        let (y0, dy) = (at(pass.row.first.0)?, at(pass.row.step.0)?);
        for r in 0..h {
            let line = &raw[r * (rb + 1) + 1..r * (rb + 1) + 1 + rb];
            let start = ((y0 + r * dy) * full + x0) * 4;
            // The row's last slot is in `rgba`, so every one before it is.
            let end = start + ((w - 1) * dx + 1) * 4;
            let slots = rgba
                .get_mut(start..end)
                .ok_or(PngError::Buffer)?
                .chunks_exact_mut(4);
            // A step of 1 is the null method's and pass 7's; it is its own
            // loop so that it has no step to take.
            if dx == 1 {
                self.pixels(line, slots)?;
            } else {
                self.pixels(line, slots.step_by(dx))?;
            }
        }
        Ok(())
    }

    /// One scanline's pixels to RGBA8, into `slots` in order. Sixteen-bit
    /// samples keep their high byte; sub-byte grey scales to the full range;
    /// palette indices look up `PLTE` with `tRNS` alpha; a `tRNS` colour key
    /// on grey and RGB images makes that one colour transparent (11.3.2.1).
    fn pixels<'p>(
        &self,
        line: &[u8],
        slots: impl Iterator<Item = &'p mut [u8]>,
    ) -> Result<(), PngError> {
        let depth = usize::from(self.depth);
        let palette = self.palette.map(Bytes::as_slice).unwrap_or(&[]);
        let trns = self.trns.map(Bytes::as_slice).unwrap_or(&[]);
        for (x, px) in slots.enumerate() {
            match self.color {
                Color::Gray => {
                    let v = sample(line, x, depth);
                    let g = scale(v, depth);
                    let a = if trns.len() >= 2 && u16::from_be_bytes([trns[0], trns[1]]) == v {
                        0
                    } else {
                        255
                    };
                    px.copy_from_slice(&[g, g, g, a]);
                }
                Color::Palette => {
                    let i = usize::from(
                        u8::try_from(sample(line, x, depth)).map_err(|_| PngError::Palette)?,
                    );
                    let rgb = palette.get(i * 3..i * 3 + 3).ok_or(PngError::Palette)?;
                    let a = trns.get(i).copied().unwrap_or(255);
                    px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], a]);
                }
                Color::GrayAlpha => {
                    let s = depth / 8;
                    let g = line[x * 2 * s];
                    let a = line[x * 2 * s + s];
                    px.copy_from_slice(&[g, g, g, a]);
                }
                Color::Rgb => {
                    let s = depth / 8;
                    let at = x * 3 * s;
                    let (r8, g8, b8) = (line[at], line[at + s], line[at + 2 * s]);
                    let a = if trns.len() >= 6 && self.color_key_matches(line, at, s, trns) {
                        0
                    } else {
                        255
                    };
                    px.copy_from_slice(&[r8, g8, b8, a]);
                }
                Color::Rgba => {
                    let s = depth / 8;
                    let at = x * 4 * s;
                    px.copy_from_slice(&[
                        line[at],
                        line[at + s],
                        line[at + 2 * s],
                        line[at + 3 * s],
                    ]);
                }
            }
        }
        Ok(())
    }

    /// Whether the RGB sample at `at` equals the `tRNS` colour key, compared
    /// at the image's own depth as the specification requires.
    fn color_key_matches(&self, line: &[u8], at: usize, s: usize, trns: &[u8]) -> bool {
        let key = |i: usize| u16::from_be_bytes([trns[i * 2], trns[i * 2 + 1]]);
        let sample_at = |i: usize| -> u16 {
            if s == 2 {
                u16::from_be_bytes([line[at + i * 2], line[at + i * 2 + 1]])
            } else {
                u16::from(line[at + i])
            }
        };
        sample_at(0) == key(0) && sample_at(1) == key(1) && sample_at(2) == key(2)
    }
}

/// Sample `x` of a scanline at `depth` bits, big-endian within the byte and
/// the word, as the specification packs them (7.2).
fn sample(line: &[u8], x: usize, depth: usize) -> u16 {
    match depth {
        16 => u16::from_be_bytes([line[x * 2], line[x * 2 + 1]]),
        8 => u16::from(line[x]),
        _ => {
            let per_byte = 8 / depth;
            let byte = line[x / per_byte];
            let shift = 8 - depth * (x % per_byte + 1);
            u16::from((byte >> shift) & ((1u8 << depth) - 1))
        }
    }
}

/// A grey sample to 8 bits: the high byte of a 16-bit sample, replication
/// of a sub-byte one (the `png` reference's `EXPAND`).
fn scale(v: u16, depth: usize) -> u8 {
    match depth {
        16 => u8::try_from(v >> 8).unwrap_or(u8::MAX),
        8 => u8::try_from(v).unwrap_or(u8::MAX),
        _ => {
            let max = (1u16 << depth) - 1;
            u8::try_from((v * 255) / max).unwrap_or(u8::MAX)
        }
    }
}

/// The Paeth predictor, exactly as 9.4 spells it.
fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

fn adler32(bytes: &[u8]) -> u32 {
    let mut a = 1u32;
    let mut b = 0u32;
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// The `IDAT` chunks as one byte stream.
struct Idat<'a> {
    data: Bytes<'a>,
    chunk: Bytes<'a>,
    pos: usize,
    next: Offset,
}

impl<'a> Idat<'a> {
    fn new(data: Bytes<'a>) -> Result<Idat<'a>, PngError> {
        let mut at = Offset::of(8u32);
        loop {
            let c = chunk(data, at)?;
            match &c.kind {
                b"IDAT" => {
                    return Ok(Idat {
                        data,
                        chunk: c.data,
                        pos: 0,
                        next: c.next,
                    });
                }
                b"IEND" => return Err(PngError::Malformed),
                _ => at = c.next,
            }
        }
    }

    fn byte(&mut self) -> Option<u8> {
        loop {
            if let Some(&b) = self.chunk.as_slice().get(self.pos) {
                self.pos += 1;
                return Some(b);
            }
            // The next IDAT, or the end of the image data.
            let c = chunk(self.data, self.next).ok()?;
            if c.kind != *b"IDAT" {
                return None;
            }
            self.chunk = c.data;
            self.pos = 0;
            self.next = c.next;
        }
    }
}

/// Deflate's bit order: least-significant bit first from each byte (3.1.1).
struct BitReader<'a> {
    input: Idat<'a>,
    bits: u32,
    count: u32,
}

impl BitReader<'_> {
    fn take(&mut self, n: u32) -> Result<u32, PngError> {
        while self.count < n {
            let b = self.input.byte().ok_or(PngError::Deflate)?;
            self.bits |= u32::from(b) << self.count;
            self.count += 8;
        }
        let v = self.bits & ((1u32 << n) - 1);
        self.bits >>= n;
        self.count -= n;
        Ok(v)
    }

    /// Drops the bits left in the current byte (a stored block, the trailer).
    fn align(&mut self) {
        self.bits = 0;
        self.count = 0;
    }
}

struct Out<'b> {
    buf: &'b mut [u8],
    len: usize,
}

impl Out<'_> {
    fn push(&mut self, b: u8) -> Result<(), PngError> {
        *self.buf.get_mut(self.len).ok_or(PngError::Deflate)? = b;
        self.len += 1;
        Ok(())
    }

    /// A back-reference: `run` bytes from `back` behind the write position,
    /// byte by byte so an overlapping copy repeats as the format intends.
    fn copy(&mut self, run: Run, back: Back) -> Result<(), PngError> {
        if back.0 == 0 || back.0 > self.len {
            return Err(PngError::Deflate);
        }
        for _ in 0..run.0 {
            let b = self.buf[self.len - back.0];
            self.push(b)?;
        }
        Ok(())
    }
}

/// How many bytes a back-reference copies (3.2.5's length).
#[derive(Clone, Copy)]
struct Run(usize);

/// How far behind the write position it copies from (3.2.5's distance);
/// the two are the same width and a swapped pair decodes to plausible
/// garbage, so they do not share a type.
#[derive(Clone, Copy)]
struct Back(usize);

/// A stored block: LEN, its one's complement, then the bytes (3.2.4).
fn stored(bits: &mut BitReader<'_>, out: &mut Out<'_>) -> Result<(), PngError> {
    bits.align();
    let mut hdr = [0u8; 4];
    for b in &mut hdr {
        *b = bits.input.byte().ok_or(PngError::Deflate)?;
    }
    let len = u16::from_le_bytes([hdr[0], hdr[1]]);
    let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
    if len != !nlen {
        return Err(PngError::Deflate);
    }
    for _ in 0..len {
        let b = bits.input.byte().ok_or(PngError::Deflate)?;
        out.push(b)?;
    }
    Ok(())
}

const MAX_BITS: usize = 15;
const MAX_LIT: usize = 288;
const MAX_DIST: usize = 32;

/// A canonical Huffman code as the specification defines it (3.2.2): how
/// many codes there are of each length, and the symbols in code order.
struct Code<const N: usize> {
    count: [u16; MAX_BITS + 1],
    symbol: [u16; N],
}

impl<const N: usize> Code<N> {
    /// Builds the code from a length per symbol. An over-subscribed set of
    /// lengths is refused; an incomplete one is allowed, as the reference
    /// implementations allow it, because a single-code distance tree is
    /// legal and incomplete.
    fn build(lengths: &[u8]) -> Result<Code<N>, PngError> {
        let mut code = Code {
            count: [0; MAX_BITS + 1],
            symbol: [0; N],
        };
        for &l in lengths {
            code.count[usize::from(l)] += 1;
        }
        code.count[0] = 0;
        let mut left = 1i32;
        for len in 1..=MAX_BITS {
            left <<= 1;
            left -= i32::from(code.count[len]);
            if left < 0 {
                return Err(PngError::Deflate);
            }
        }
        let mut offs = [0u16; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offs[len + 1] = offs[len] + code.count[len];
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let slot = usize::from(offs[usize::from(l)]);
                *code.symbol.get_mut(slot).ok_or(PngError::Deflate)? =
                    u16::try_from(sym).map_err(|_| PngError::Deflate)?;
                offs[usize::from(l)] += 1;
            }
        }
        Ok(code)
    }

    /// One symbol, a bit at a time: at each length, `first` is the first code
    /// of that length and `index` where its symbols start.
    fn decode(&self, bits: &mut BitReader<'_>) -> Result<u16, PngError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..=MAX_BITS {
            code |= i32::from(bits.take(1)? != 0);
            let count = i32::from(self.count[len]);
            if code - count < first {
                return self
                    .symbol
                    .get(usize::try_from(index + (code - first)).map_err(|_| PngError::Deflate)?)
                    .copied()
                    .ok_or(PngError::Deflate);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(PngError::Deflate)
    }
}

/// The fixed literal/length and distance codes (3.2.6).
fn fixed_tables() -> (Code<MAX_LIT>, Code<MAX_DIST>) {
    let mut lengths = [0u8; MAX_LIT];
    for (i, l) in lengths.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let lit = Code::build(&lengths).unwrap_or(Code {
        count: [0; MAX_BITS + 1],
        symbol: [0; MAX_LIT],
    });
    let dist = Code::build(&[5u8; 30]).unwrap_or(Code {
        count: [0; MAX_BITS + 1],
        symbol: [0; MAX_DIST],
    });
    (lit, dist)
}

/// A field of at most a few bits as a count.
fn small(v: u32) -> Result<usize, PngError> {
    usize::try_from(v).map_err(|_| PngError::Deflate)
}

/// The dynamic tables (3.2.7): code lengths for the code-length code, then
/// the literal/length and distance lengths run-length coded with it.
fn dynamic_tables(bits: &mut BitReader<'_>) -> Result<(Code<MAX_LIT>, Code<MAX_DIST>), PngError> {
    let nlen = small(bits.take(5)?)? + 257;
    let ndist = small(bits.take(5)?)? + 1;
    let ncode = small(bits.take(4)?)? + 4;
    if nlen > 286 || ndist > 30 {
        return Err(PngError::Deflate);
    }
    const ORDER: [usize; 19] = [
        16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
    ];
    let mut lengths = [0u8; MAX_LIT + MAX_DIST];
    for &o in ORDER.iter().take(ncode) {
        lengths[o] = u8::try_from(bits.take(3)?).map_err(|_| PngError::Deflate)?;
    }
    let lencode: Code<19> = Code::build(&lengths[..19])?;
    let mut i = 0;
    while i < nlen + ndist {
        let sym = lencode.decode(bits)?;
        match sym {
            0..=15 => {
                lengths[i] = u8::try_from(sym).map_err(|_| PngError::Deflate)?;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(PngError::Deflate);
                }
                let prev = lengths[i - 1];
                let n = 3 + small(bits.take(2)?)?;
                if i + n > nlen + ndist {
                    return Err(PngError::Deflate);
                }
                for _ in 0..n {
                    lengths[i] = prev;
                    i += 1;
                }
            }
            17 | 18 => {
                let n = if sym == 17 {
                    3 + small(bits.take(3)?)?
                } else {
                    11 + small(bits.take(7)?)?
                };
                if i + n > nlen + ndist {
                    return Err(PngError::Deflate);
                }
                for _ in 0..n {
                    lengths[i] = 0;
                    i += 1;
                }
            }
            _ => return Err(PngError::Deflate),
        }
    }
    // The end-of-block code must exist.
    if lengths[256] == 0 {
        return Err(PngError::Deflate);
    }
    let lit = Code::build(&lengths[..nlen])?;
    let dist = Code::build(&lengths[nlen..nlen + ndist])?;
    Ok((lit, dist))
}

/// Length and distance bases and extra bits (3.2.5).
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Decodes one compressed block's symbols until end-of-block (3.2.3).
fn codes(
    bits: &mut BitReader<'_>,
    out: &mut Out<'_>,
    lit: &Code<MAX_LIT>,
    dist: &Code<MAX_DIST>,
) -> Result<(), PngError> {
    loop {
        let sym = lit.decode(bits)?;
        match sym {
            0..=255 => out.push(u8::try_from(sym).map_err(|_| PngError::Deflate)?)?,
            256 => return Ok(()),
            257..=285 => {
                let i = usize::from(sym - 257);
                let len = usize::from(LEN_BASE[i]) + small(bits.take(u32::from(LEN_EXTRA[i]))?)?;
                let d = usize::from(dist.decode(bits)?);
                if d >= 30 {
                    return Err(PngError::Deflate);
                }
                let distance =
                    usize::from(DIST_BASE[d]) + small(bits.take(u32::from(DIST_EXTRA[d]))?)?;
                out.copy(Run(len), Back(distance))?;
            }
            _ => return Err(PngError::Deflate),
        }
    }
}
