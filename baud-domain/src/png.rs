//! PNG, decoded to RGBA8 (`[[source: w3c-png-2003]]`; the compression is
//! zlib over deflate, `[[source: rfc1950-zlib]]` and `[[source: rfc1951-deflate]]`).
//!
//! **Why it lives here.** Two crates decode PNG: `baud-font` its colour emoji
//! strikes -- Apple Color Emoji's `sbix` carries 31,707 of them on this Mac,
//! Noto Color Emoji's `CBDT` thousands more -- and the core a kitty `f=100`
//! picture (note V, V19 step 2: "In baud-domain"). Both depend on this crate,
//! and "no engine dependencies" covers image libraries too. The decoder is
//! the smallest complete one: every colour type and bit depth the
//! specification allows, the five filters, both interlace methods, CRC-32 on
//! every chunk, and the zlib stream through [`crate::zlib`], which checks its
//! Adler-32.
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
//! **The container is read whole; the stream inside it is not.** `parse`
//! and `decode` take the file's bytes in one slice. The `IDAT` chunks are
//! handed to [`Inflate::run`] one by one as a source of slices, the same
//! resumable inflate a caller with a stream split across reads feeds itself.

use crate::zlib::{Inflate, Inflated, ZlibError};
use core::ops::Range;

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
    data: &'a [u8],
    size: Size,
    depth: u8,
    color: Color,
    interlace: Interlace,
    palette: Option<&'a [u8]>,
    trns: Option<&'a [u8]>,
    /// The first `IDAT` chunk, read as the header walk reached it: `None`
    /// when `IEND` came first. [`Idat::new`] and [`Png::steps`] start
    /// here, so the chunk's CRC is taken once, by the walk: a single
    /// `IDAT` of 960,772 bytes, mosaic-blue's, was taken again by each.
    first_idat: Option<FirstIdat<'a>>,
}

/// Where the image data begins: the first `IDAT` chunk's data and the
/// chunk after it.
#[derive(Clone, Copy)]
struct FirstIdat<'a> {
    data: &'a [u8],
    next: ChunkAt,
}

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// The CRC-32 tables, built at compile time from the polynomial the
/// specification gives (5.5); a chunk whose CRC disagrees is refused, which
/// is what every reference decoder does by default.
///
/// **Eight tables, so eight bytes go in a step** (slicing). Table 0 is
/// Annex D's `crc_table`: byte `n` run through the eight bit steps of its
/// `make_crc_table`. Table `k` is the same byte followed by `k` zero bytes,
/// `n` through `8 (k + 1)` bit steps, so the CRC of eight bytes is the XOR
/// of each byte's entry in the table for how many bytes follow it.
const CRC_TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    let mut n: u32 = 0;
    while n < 256 {
        let mut c = n;
        let mut k = 0;
        while k < 64 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
            if k % 8 == 0 {
                // The one cast in this file: a `const fn` cannot call
                // `try_from`, and `n` is below 256 by the loop bound.
                t[k / 8 - 1][n as usize] = c;
            }
        }
        n += 1;
    }
    t
};

/// The CRC of `bytes` (5.5, Annex D's `update_crc` from all ones, the
/// result inverted), eight bytes a step and the rest one at a time.
fn crc32(bytes: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut c = 0xFFFF_FFFFu32;
    let (steps, tail) = bytes.as_chunks::<8>();
    for step in steps {
        let lo = u32::from_le_bytes([step[0], step[1], step[2], step[3]]) ^ c;
        let [b0, b1, b2, b3] = lo.to_le_bytes();
        c = t[7][usize::from(b0)]
            ^ t[6][usize::from(b1)]
            ^ t[5][usize::from(b2)]
            ^ t[4][usize::from(b3)]
            ^ t[3][usize::from(step[4])]
            ^ t[2][usize::from(step[5])]
            ^ t[1][usize::from(step[6])]
            ^ t[0][usize::from(step[7])];
    }
    for &b in tail {
        c = t[0][usize::from((c ^ u32::from(b)).to_le_bytes()[0])] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// One chunk: its type and its data, already CRC-checked.
struct Chunk<'a> {
    kind: [u8; 4],
    data: &'a [u8],
    next: ChunkAt,
}

/// Where a chunk starts in the file: its length field's first byte.
#[derive(Clone, Copy)]
struct ChunkAt(usize);

/// The big-endian `u32` at the start of `bytes`, which holds at least four.
fn be32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// Whether a chunk's CRC is checked as it is read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Crc {
    Check,
    /// Already checked: [`Png::reparse`] reading a header [`Png::parse`]
    /// accepted, over bytes that have not changed.
    Trusted,
}

fn chunk(data: &[u8], at: ChunkAt) -> Result<Chunk<'_>, PngError> {
    chunk_with(data, at, Crc::Check)
}

fn chunk_with(data: &[u8], at: ChunkAt, crc: Crc) -> Result<Chunk<'_>, PngError> {
    let at = at.0;
    let len = be32(data.get(at..).ok_or(PngError::Malformed)?).ok_or(PngError::Malformed)?;
    let len = usize::try_from(len).map_err(|_| PngError::Malformed)?;
    let kind_at = at.checked_add(4).ok_or(PngError::Malformed)?;
    let body_at = kind_at.checked_add(4).ok_or(PngError::Malformed)?;
    let crc_at = body_at.checked_add(len).ok_or(PngError::Malformed)?;
    let next = crc_at.checked_add(4).ok_or(PngError::Malformed)?;
    // Type and body together are what the CRC covers.
    let covered = data.get(kind_at..crc_at).ok_or(PngError::Malformed)?;
    let stored =
        be32(data.get(crc_at..next).ok_or(PngError::Malformed)?).ok_or(PngError::Malformed)?;
    if crc == Crc::Check && crc32(covered) != stored {
        return Err(PngError::Crc);
    }
    let kind: [u8; 4] = covered
        .get(..4)
        .and_then(|k| k.try_into().ok())
        .ok_or(PngError::Malformed)?;
    Ok(Chunk {
        kind,
        data: &covered[4..],
        next: ChunkAt(next),
    })
}

impl<'a> Png<'a> {
    /// Parses the signature, `IHDR`, and the `PLTE`/`tRNS` chunks that
    /// precede the image data; checks every chunk's CRC on the way.
    pub fn parse(bytes: &'a [u8]) -> Result<Png<'a>, PngError> {
        Png::parse_with(bytes, Crc::Check)
    }

    /// **The header of bytes [`Png::parse`] already accepted, read again
    /// without its CRCs**: what a stepped decode reads at each step over the
    /// same bytes. A first `IDAT` is checked as the header walk reaches it,
    /// and an encoder that writes the whole stream as one -- mosaic-blue's
    /// is 960,772 bytes -- had every step check it again, which took the
    /// steps of its decode to 2.4 times the one-shot decode.
    pub fn reparse(bytes: &'a [u8]) -> Result<Png<'a>, PngError> {
        Png::parse_with(bytes, Crc::Trusted)
    }

    #[inline(always)]
    fn parse_with(bytes: &'a [u8], crc: Crc) -> Result<Png<'a>, PngError> {
        let data = bytes;
        if bytes.get(..8) != Some(&SIGNATURE) {
            return Err(PngError::Malformed);
        }
        let ihdr = chunk_with(data, ChunkAt(8), crc)?;
        let &[
            w0,
            w1,
            w2,
            w3,
            h0,
            h1,
            h2,
            h3,
            depth,
            color,
            compression,
            filter,
            interlace,
        ] = ihdr.data
        else {
            return Err(PngError::Malformed);
        };
        if ihdr.kind != *b"IHDR" {
            return Err(PngError::Malformed);
        }
        let width = u32::from_be_bytes([w0, w1, w2, w3]);
        let height = u32::from_be_bytes([h0, h1, h2, h3]);
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
            first_idat: None,
        };
        let mut at = ihdr.next;
        loop {
            let c = chunk_with(data, at, crc)?;
            match &c.kind {
                b"PLTE" => png.palette = Some(c.data),
                b"tRNS" => png.trns = Some(c.data),
                b"IDAT" => {
                    png.first_idat = Some(FirstIdat {
                        data: c.data,
                        next: c.next,
                    });
                    break;
                }
                b"IEND" => break,
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
            // before Adam7 (`baud-bench`'s `png_decode`). `codes`,
            // inflate's symbol loop, was the same machine code in all
            // three.
            let h = usize::try_from(self.size.height).map_err(|_| PngError::Buffer)?;
            self.unfilter(raw, self.size, 0..h)?;
            self.expand(raw, rgba, self.size, Pass::WHOLE, 0..h)?;
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
            self.unfilter(part, size, 0..h)?;
            self.expand(part, rgba, size, pass, 0..h)?;
            at += stride * h;
        }
        Ok(self.size)
    }

    /// The `IDAT` chunks in order, as one zlib stream, inflated into `raw`:
    /// each chunk's data is one slice of [`Inflate::run`]'s source, and a
    /// stream the chunks end inside is malformed.
    fn inflate(&self, raw: &mut [u8]) -> Result<(), PngError> {
        let mut z = Inflate::NEW;
        match z.run(Idat::new(self)?, raw) {
            Ok(Inflated::Done) => Ok(()),
            Ok(Inflated::More) => Err(PngError::Deflate),
            Err(ZlibError::Adler) => Err(PngError::Adler),
            Err(_) => Err(PngError::Deflate),
        }
    }

    /// Reconstructs scanlines `rows` of one reduced image of `size` pixels in
    /// place (9.2): the filter byte stays where it is and the row after it
    /// becomes the original bytes. `raw` starts at the image's first
    /// scanline, so its prior scanline is zero, as 9.2 requires of every
    /// reduced image; the rows before `rows` are already reconstructed.
    fn unfilter(&self, raw: &mut [u8], size: Size, rows: Range<usize>) -> Result<(), PngError> {
        let rb = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
            .map_err(|_| PngError::Buffer)?;
        // The "corresponding byte of the previous pixel": whole bytes for
        // depths of 8 and more, one byte for sub-byte pixels (9.2).
        let bpp = usize::try_from(self.bits_per_pixel().div_ceil(8))
            .map_err(|_| PngError::Buffer)?
            .max(1);
        unfilter_rows(raw, Scan { bytes: rb, rows }, bpp)
    }

    /// The pixels of scanlines `rows` of one reduced image of `size` pixels
    /// to RGBA8, at the place `pass` puts them in the whole image: each
    /// scanline's pixels go to every `step`-th four-byte slot of their row
    /// from the first.
    fn expand(
        &self,
        raw: &[u8],
        rgba: &mut [u8],
        size: Size,
        pass: Pass,
        rows: Range<usize>,
    ) -> Result<(), PngError> {
        let rb = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
            .map_err(|_| PngError::Buffer)?;
        let w = usize::try_from(size.width).map_err(|_| PngError::Buffer)?;
        let full = usize::try_from(self.size.width).map_err(|_| PngError::Buffer)?;
        let at = |n: u32| usize::try_from(n).map_err(|_| PngError::Buffer);
        let (x0, dx) = (at(pass.col.first.0)?, at(pass.col.step.0)?);
        let (y0, dy) = (at(pass.row.first.0)?, at(pass.row.step.0)?);
        for r in rows {
            let line = &raw[r * (rb + 1) + 1..r * (rb + 1) + 1 + rb];
            let start = ((y0 + r * dy) * full + x0) * 4;
            // The row's last slot is in `rgba`, so every one before it is.
            let end = start + ((w - 1) * dx + 1) * 4;
            let slots = rgba
                .get_mut(start..end)
                .ok_or(PngError::Buffer)?
                .as_chunks_mut::<4>()
                .0
                .iter_mut();
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

    /// One scanline's pixels to RGBA8, into `slots` in order, each an array
    /// store rather than a `copy_from_slice` (`zlib.rs`'s stored-block copy
    /// says why). Sixteen-bit samples keep their high byte; sub-byte grey
    /// scales to the full range; palette indices look up `PLTE` with `tRNS`
    /// alpha; a `tRNS` colour key on grey and RGB images makes that one
    /// colour transparent (11.3.2.1).
    fn pixels<'p>(
        &self,
        line: &[u8],
        slots: impl Iterator<Item = &'p mut [u8; 4]>,
    ) -> Result<(), PngError> {
        let depth = usize::from(self.depth);
        let palette = self.palette.unwrap_or(&[]);
        let trns = self.trns.unwrap_or(&[]);
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
                    *px = [g, g, g, a];
                }
                Color::Palette => {
                    let i = usize::from(
                        u8::try_from(sample(line, x, depth)).map_err(|_| PngError::Palette)?,
                    );
                    let rgb = palette.get(i * 3..i * 3 + 3).ok_or(PngError::Palette)?;
                    let a = trns.get(i).copied().unwrap_or(255);
                    *px = [rgb[0], rgb[1], rgb[2], a];
                }
                Color::GrayAlpha => {
                    let s = depth / 8;
                    let g = line[x * 2 * s];
                    let a = line[x * 2 * s + s];
                    *px = [g, g, g, a];
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
                    *px = [r8, g8, b8, a];
                }
                Color::Rgba => {
                    let s = depth / 8;
                    let at = x * 4 * s;
                    *px = [line[at], line[at + s], line[at + 2 * s], line[at + 3 * s]];
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

/// Which of a reduced image's filtered scanlines to reconstruct, `rows`,
/// each a filter byte and `bytes` more; the rows before them are done.
#[derive(Clone, Debug)]
struct Scan {
    bytes: usize,
    rows: Range<usize>,
}

/// [`Png::unfilter`] at a byte count per pixel: each count a PNG can have
/// (one for every depth below 8, then 1, 2, 3, 4, 6 or 8 bytes) gets its
/// own copy of the loops, with the pixel to the left held in registers.
fn unfilter_rows(raw: &mut [u8], scan: Scan, bpp: usize) -> Result<(), PngError> {
    match bpp {
        1 => rows_of::<1>(raw, scan),
        2 => rows_of::<2>(raw, scan),
        3 => rows_of::<3>(raw, scan),
        4 => rows_of::<4>(raw, scan),
        6 => rows_of::<6>(raw, scan),
        8 => rows_of::<8>(raw, scan),
        _ => Err(PngError::Buffer),
    }
}

/// Every scanline, `N` bytes a pixel (9.2-9.4). Each filter's
/// reconstruction depends on the byte `N` to its left, so a scanline is
/// reconstructed a pixel at a time, left to right, with that pixel -- `a`,
/// and for Paeth the one above it, `c` -- carried as `N` bytes rather than
/// read back from the bytes just written. "The bytes to the left of the
/// first pixel" are zero, and on the first scanline so is every byte of the
/// prior one.
fn rows_of<const N: usize>(raw: &mut [u8], scan: Scan) -> Result<(), PngError> {
    let stride = scan.bytes + 1;
    for r in scan.rows {
        let (before, rest) = raw.split_at_mut(r * stride);
        let (filter, line) = rest[..stride].split_first_mut().ok_or(PngError::Buffer)?;
        let prev: &[u8] = if r == 0 {
            &[]
        } else {
            &before[(r - 1) * stride + 1..]
        };
        match (*filter, prev.is_empty()) {
            (0, _) | (2, true) => {}
            // On the first scanline Paeth's `b` and `c` are zero and it
            // predicts `a` (9.4: `pa` is 0), which is Sub.
            (1, _) | (4, true) => sub::<N>(line),
            (2, false) => {
                for (x, up) in line.iter_mut().zip(prev) {
                    *x = x.wrapping_add(*up);
                }
            }
            (3, true) => average_first::<N>(line),
            (3, false) => average::<N>(line, prev),
            (4, false) => paeth_row::<N>(line, prev),
            _ => return Err(PngError::Deflate),
        }
    }
    Ok(())
}

/// Sub: `Recon(a)` added to each byte.
fn sub<const N: usize>(line: &mut [u8]) {
    let mut a = [0u8; N];
    let (pixels, tail) = line.as_chunks_mut::<N>();
    for px in pixels {
        for i in 0..N {
            px[i] = px[i].wrapping_add(a[i]);
        }
        a = *px;
    }
    for (x, a) in tail.iter_mut().zip(a) {
        *x = x.wrapping_add(a);
    }
}

/// `floor((x + y) / 2)` in eight bits, as 9.3's nine-bit sum gives it: the
/// bits both have, plus half of those only one has.
#[inline(always)]
fn floor_mean(x: u8, y: u8) -> u8 {
    (x & y) + ((x ^ y) >> 1)
}

/// Average on the first scanline, where `Recon(b)` is zero.
fn average_first<const N: usize>(line: &mut [u8]) {
    let mut a = [0u8; N];
    let (pixels, tail) = line.as_chunks_mut::<N>();
    for px in pixels {
        for i in 0..N {
            px[i] = px[i].wrapping_add(a[i] >> 1);
        }
        a = *px;
    }
    for (x, a) in tail.iter_mut().zip(a) {
        *x = x.wrapping_add(a >> 1);
    }
}

/// Average: `floor((Recon(a) + Recon(b)) / 2)` added to each byte (9.3).
fn average<const N: usize>(line: &mut [u8], prev: &[u8]) {
    let mut a = [0u8; N];
    let (pixels, tail) = line.as_chunks_mut::<N>();
    let (ups, up_tail) = prev.as_chunks::<N>();
    for (px, up) in pixels.iter_mut().zip(ups) {
        for i in 0..N {
            px[i] = px[i].wrapping_add(floor_mean(a[i], up[i]));
        }
        a = *px;
    }
    for ((x, a), b) in tail.iter_mut().zip(a).zip(up_tail) {
        *x = x.wrapping_add(floor_mean(a, *b));
    }
}

/// Paeth: `PaethPredictor(Recon(a), Recon(b), Recon(c))` added to each
/// byte (9.4).
fn paeth_row<const N: usize>(line: &mut [u8], prev: &[u8]) {
    let mut a = [0u8; N];
    let mut c = [0u8; N];
    let (pixels, tail) = line.as_chunks_mut::<N>();
    let (ups, up_tail) = prev.as_chunks::<N>();
    for (px, up) in pixels.iter_mut().zip(ups) {
        for i in 0..N {
            px[i] = px[i].wrapping_add(paeth(a[i], up[i], c[i]));
        }
        a = *px;
        c = *up;
    }
    for (((x, a), b), c) in tail.iter_mut().zip(a).zip(up_tail).zip(c) {
        *x = x.wrapping_add(paeth(a, *b, c));
    }
}

/// The `IDAT` chunks' data, in order: the first one and every one that
/// follows it with no other chunk between (table 5.3, "Multiple IDAT
/// chunks shall be consecutive"). A chunk that fails its CRC ends the run, as the
/// end of the image data does.
struct Idat<'a> {
    data: &'a [u8],
    /// The first chunk's data, read and checked by the header walk.
    first: Option<&'a [u8]>,
    next: Option<ChunkAt>,
}

impl<'a> Idat<'a> {
    /// From the first `IDAT` the header walk read, CRC and all; an image
    /// with none is malformed.
    fn new(png: &Png<'a>) -> Result<Idat<'a>, PngError> {
        let f = png.first_idat.ok_or(PngError::Malformed)?;
        Ok(Idat {
            data: png.data,
            first: Some(f.data),
            next: Some(f.next),
        })
    }
}

impl<'a> Iterator for Idat<'a> {
    type Item = &'a [u8];

    /// The next chunk is read only when the inflate asks for more, so a
    /// chunk after the stream's end is never checked, as before the move.
    fn next(&mut self) -> Option<&'a [u8]> {
        if let Some(first) = self.first.take() {
            return Some(first);
        }
        let c = chunk(self.data, self.next?)
            .ok()
            .filter(|c| c.kind == *b"IDAT");
        self.next = c.as_ref().map(|c| c.next);
        c.map(|c| c.data)
    }
}

/// **A decode advanced a bounded amount at a time**: [`Png::step`] resumes
/// it. What the core's deferred `f=100` decode keeps between steps (note V,
/// V19 step 3: deferred inside the core, in bounded steps). The same work as
/// [`Png::decode`] in the same order -- the `IDAT` stream inflated into the
/// scratch, then each reduced image unfiltered and expanded a scanline at a
/// time -- so the output is the same whatever the steps' sizes. `Copy`, for
/// a caller that keeps it in caller memory beside the stream it came from.
#[derive(Clone, Copy)]
pub struct PngSteps {
    z: Inflate,
    stage: Stage,
}

/// Where a stepped decode stands.
#[derive(Clone, Copy)]
enum Stage {
    /// Feeding the `IDAT` data from `at` to `end`; the next chunk is at
    /// `next`.
    Inflate {
        at: usize,
        end: usize,
        next: ChunkAt,
    },
    /// Reconstructing reduced image `pass` from scanline `row`; its
    /// scanlines start at `base` in the scratch.
    Rows {
        pass: usize,
        row: usize,
        base: usize,
    },
    /// Every pixel written.
    Done,
}

/// What a step left the decode as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PngProgress {
    /// Work remains.
    More,
    /// Every pixel is written.
    Done,
}

impl<'a> Png<'a> {
    /// A stepped decode of this image, at its first `IDAT` chunk, which
    /// the header walk read.
    pub fn steps(&self) -> Result<PngSteps, PngError> {
        let f = self.first_idat.ok_or(PngError::Malformed)?;
        let start = f.next.0 - 4 - f.data.len();
        Ok(PngSteps {
            z: Inflate::NEW,
            stage: Stage::Inflate {
                at: start,
                end: start + f.data.len(),
                next: f.next,
            },
        })
    }

    /// **Advance `st` by about `budget` bytes of work**: input fed to the
    /// inflate, or scanline bytes reconstructed, at least one of either.
    /// `raw` and `rgba` are the buffers [`Png::decode`] takes, the same
    /// ones on every step. A refusal is [`Png::decode`]'s for the same bytes.
    pub fn step(
        &self,
        st: &mut PngSteps,
        raw: &mut [u8],
        rgba: &mut [u8],
        budget: usize,
    ) -> Result<PngProgress, PngError> {
        let raw_len = self.raw_len().ok_or(PngError::Buffer)?;
        let rgba_len = self.rgba_len().ok_or(PngError::Buffer)?;
        let raw = raw.get_mut(..raw_len).ok_or(PngError::Buffer)?;
        let rgba = rgba.get_mut(..rgba_len).ok_or(PngError::Buffer)?;
        let mut left = budget.max(1);
        while left > 0 {
            match st.stage {
                Stage::Done => return Ok(PngProgress::Done),
                Stage::Inflate { at, end, next } if at == end => {
                    // The chunk is spent: the next must be `IDAT`, the
                    // stream ending inside the run being malformed, as
                    // `Idat` makes it in the one-shot decode.
                    let c = chunk(self.data, next)
                        .ok()
                        .filter(|c| c.kind == *b"IDAT")
                        .ok_or(PngError::Deflate)?;
                    let start = c.next.0 - 4 - c.data.len();
                    st.stage = Stage::Inflate {
                        at: start,
                        end: start + c.data.len(),
                        next: c.next,
                    };
                    left -= 1;
                }
                Stage::Inflate { at, end, next } => {
                    let to = end.min(at.saturating_add(left));
                    let fed = st.z.feed(&self.data[at..to], raw).map_err(|e| match e {
                        ZlibError::Adler => PngError::Adler,
                        _ => PngError::Deflate,
                    })?;
                    left -= to - at;
                    st.stage = match fed {
                        Inflated::Done => Stage::Rows {
                            pass: 0,
                            row: 0,
                            base: 0,
                        },
                        Inflated::More => Stage::Inflate { at: to, end, next },
                    };
                }
                Stage::Rows { pass, row, base } => {
                    let Some(&p) = self.passes().get(pass) else {
                        st.stage = Stage::Done;
                        continue;
                    };
                    let Some(size) = p.reduced(self.size) else {
                        st.stage = Stage::Rows {
                            pass: pass + 1,
                            row: 0,
                            base,
                        };
                        continue;
                    };
                    let stride = usize::try_from(self.row_bytes(size).ok_or(PngError::Buffer)?)
                        .map_err(|_| PngError::Buffer)?
                        + 1;
                    let h = usize::try_from(size.height).map_err(|_| PngError::Buffer)?;
                    let n = (left / stride).clamp(1, h - row);
                    let part = raw
                        .get_mut(base..base + stride * h)
                        .ok_or(PngError::Buffer)?;
                    self.unfilter(part, size, row..row + n)?;
                    self.expand(part, rgba, size, p, row..row + n)?;
                    left = left.saturating_sub(n * stride);
                    st.stage = match row + n == h {
                        true => Stage::Rows {
                            pass: pass + 1,
                            row: 0,
                            base: base + stride * h,
                        },
                        false => Stage::Rows {
                            pass,
                            row: row + n,
                            base,
                        },
                    };
                }
            }
        }
        Ok(match st.stage {
            Stage::Done => PngProgress::Done,
            _ => PngProgress::More,
        })
    }
}

/// The widened unfilter and the sliced CRC against the byte-at-a-time
/// forms they replaced, kept here as the oracles.
#[cfg(test)]
mod tests {
    // The no_std crate's tests run hosted: the checks below build buffers.
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// The unfilter as it was, a byte at a time with `bpp` read at run
    /// time and every neighbour looked up.
    fn unfilter_per_byte(
        raw: &mut [u8],
        rb: usize,
        bpp: usize,
        rows: usize,
    ) -> Result<(), PngError> {
        let stride = rb + 1;
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

    /// The CRC as Annex D's `update_crc` spells it, a byte at a time.
    fn crc_per_byte(bytes: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for &b in bytes {
            let mut x = (c ^ u32::from(b)) & 0xff;
            for _ in 0..8 {
                x = if x & 1 != 0 {
                    0xEDB8_8320 ^ (x >> 1)
                } else {
                    x >> 1
                };
            }
            c = x ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }

    /// A xorshift, so every buffer is the same on every run.
    fn bytes_from(seed: u64, len: usize) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x.to_le_bytes()[2]
            })
            .collect()
    }

    /// Scanlines for the unfilter: random bytes with each row's filter
    /// byte drawn from 0-4, or, with `bad`, a 5 somewhere.
    fn scanlines(seed: u64, rb: usize, rows: usize, bad: Option<usize>) -> Vec<u8> {
        let mut raw = bytes_from(seed, (rb + 1) * rows);
        for r in 0..rows {
            raw[r * (rb + 1)] %= 5;
        }
        if let Some(r) = bad.filter(|&r| r < rows) {
            raw[r * (rb + 1)] = 5;
        }
        raw
    }

    fn agree(seed: u64, rb: usize, bpp: usize, rows: usize, bad: Option<usize>) {
        let raw = scanlines(seed, rb, rows, bad);
        let (mut fast, mut slow) = (raw.clone(), raw.clone());
        let got = unfilter_rows(
            &mut fast,
            Scan {
                bytes: rb,
                rows: 0..rows,
            },
            bpp,
        );
        let want = unfilter_per_byte(&mut slow, rb, bpp, rows);
        assert_eq!(got, want, "verdict, {rb} bytes at {bpp}, {rows} rows");
        assert_eq!(fast, slow, "{rb} bytes at {bpp}, {rows} rows, seed {seed}");
    }

    #[test]
    fn each_byte_count_a_png_can_have_is_one_the_unfilter_has() {
        let colors = [
            Color::Gray,
            Color::Rgb,
            Color::Palette,
            Color::GrayAlpha,
            Color::Rgba,
        ];
        let mut counts = Vec::new();
        for color in colors {
            for depth in [1u8, 2, 4, 8, 16] {
                if color.allows(depth) {
                    let bits = u32::from(depth) * color.channels();
                    let bpp = usize::try_from(bits.div_ceil(8)).expect("small").max(1);
                    let mut raw = [0u8; 9];
                    assert_eq!(
                        unfilter_rows(
                            &mut raw,
                            Scan {
                                bytes: 8,
                                rows: 0..1
                            },
                            bpp
                        ),
                        Ok(())
                    );
                    counts.push(bpp);
                }
            }
        }
        counts.sort_unstable();
        counts.dedup();
        assert_eq!(counts, [1, 2, 3, 4, 6, 8]);
    }

    #[test]
    fn the_unfilter_is_the_per_byte_one_at_every_width_and_filter() {
        for bpp in [1, 2, 3, 4, 6, 8] {
            // Widths below a pixel, at it, and with a partial pixel after.
            for rb in 0..40 {
                for (k, rows) in [1usize, 2, 7].into_iter().enumerate() {
                    let seed = u64::try_from(1000 * bpp + 10 * rb + k).expect("small");
                    agree(seed, rb, bpp, rows, None);
                    agree(seed, rb, bpp, rows, Some(rows - 1));
                }
            }
        }
        // The control: every filter on a row after another, and on the first.
        let raw = scanlines(7, 12, 64, None);
        let filters: Vec<u8> = (0..64).map(|r| raw[r * 13]).collect();
        assert!((0..5).all(|f| filters[1..].contains(&f)), "{filters:?}");
    }

    #[test]
    fn the_crc_is_annex_d_at_every_length() {
        for len in 0..80 {
            let b = bytes_from(u64::try_from(len).expect("small") + 3, len);
            assert_eq!(crc32(&b), crc_per_byte(&b), "{len} bytes");
        }
        // This CRC's customary check value, over the ASCII digits 1-9, and
        // the CRC every PNG's empty `IEND` chunk carries.
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }

    /// A chunk of `kind` holding `data`, its CRC over both (5.3).
    fn chunk_bytes(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = Vec::new();
        c.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&crc32(&c[4..]).to_be_bytes());
        c
    }

    /// A 3x2 RGB PNG whose stream is one `IDAT` behind a `tEXt`, and the
    /// offset of that `IDAT`'s CRC.
    fn one_idat() -> (Vec<u8>, usize) {
        let raw = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 9, 8, 7, 6, 5, 4, 3, 2, 1];
        let z = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&3u32.to_be_bytes());
        ihdr.extend_from_slice(&2u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut png = SIGNATURE.to_vec();
        png.extend(chunk_bytes(b"IHDR", &ihdr));
        png.extend(chunk_bytes(b"tEXt", b"k\0v"));
        png.extend(chunk_bytes(b"IDAT", &z));
        let crc_at = png.len() - 4;
        png.extend(chunk_bytes(b"IEND", &[]));
        (png, crc_at)
    }

    /// **A first `IDAT` whose CRC disagrees is refused**, by the header
    /// walk, whichever decode follows: "A CRC should be checked before
    /// processing the chunk data" (13.2).
    #[test]
    fn a_bad_first_idat_crc_is_refused() {
        let (mut png, crc_at) = one_idat();
        let (mut raw, mut rgba) = (std::vec![0u8; 64], std::vec![0u8; 64]);
        let good = Png::parse(&png).expect("good");
        assert_eq!(good.decode(&mut raw, &mut rgba).ok(), Some(good.size()));
        png[crc_at] ^= 1;
        assert_eq!(Png::parse(&png).err(), Some(PngError::Crc));
    }

    /// **The first `IDAT`'s CRC is taken once, by the header walk**: a
    /// decode and a stepped decode start from the chunk it read, so over
    /// bytes [`Png::reparse`] trusts, a bad CRC there is not taken again.
    /// They walked from the signature and took it a second time.
    #[test]
    fn the_first_idat_crc_is_taken_by_the_walk_alone() {
        let (mut png, crc_at) = one_idat();
        png[crc_at] ^= 1;
        let trusted = Png::reparse(&png).expect("trusted");
        let (mut raw, mut rgba) = (std::vec![0u8; 64], std::vec![0u8; 64]);
        assert_eq!(
            trusted.decode(&mut raw, &mut rgba).ok(),
            Some(trusted.size())
        );
        assert!(trusted.steps().is_ok(), "steps took the CRC again");
    }

    proptest::proptest! {
        /// Any scanlines, at any byte count a PNG can have, unfilter as
        /// the per-byte form does, a bad filter byte included.
        #[test]
        fn any_scanlines_unfilter_as_the_per_byte_form(
            seed in proptest::prelude::any::<u64>(),
            k in 0usize..6,
            rb in 0usize..200,
            rows in 1usize..12,
            bad in proptest::option::weighted(0.1, 0usize..12),
        ) {
            agree(seed, rb, [1, 2, 3, 4, 6, 8][k], rows, bad);
        }

        /// The CRC of any bytes is the byte-at-a-time one.
        #[test]
        fn any_bytes_crc_as_annex_d(seed in proptest::prelude::any::<u64>(), len in 0usize..600) {
            let b = bytes_from(seed, len);
            proptest::prop_assert_eq!(crc32(&b), crc_per_byte(&b));
        }
    }
}
