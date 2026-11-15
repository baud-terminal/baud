//! ```text
//! Pillow 12.3.0, parts of it transcribed into Rust in this file.
//! Copyright © 1997-2011 by Secret Labs AB
//! Copyright © 1995-2011 by Fredrik Lundh and contributors
//! Copyright © 2010 by Jeffrey 'Alex' Clark and contributors
//!
//! Licence: MIT-CMU. Its text is in LICENSES/pillow-MIT-CMU.txt.
//!
//! Ported: PngImagePlugin._save's and ImageFile._save's PNG writer and
//! src/libImaging/ZipEncode.c's filter choice and deflate loop (Mode,
//! chunk, cost, filter, Encoder, encode_with, encode). Changed: written in
//! Rust for an RGB or RGBA image saved with optimize=True, deflating
//! through this crate's zlib module. The rest of this file is baud's.
//! ```
//!
//! **Pillow 12.3.0's PNG writer**, `im.save(buf, "PNG", optimize=True)`
//! for an `RGB` or `RGBA` image, ported from `PngImagePlugin._save`,
//! `ImageFile._save` and `src/libImaging/ZipEncode.c`, and a reader for
//! the checks.
//!
//! What the writer is, line for line:
//!
//! * the signature, `IHDR` (bit depth 8, colour type 2 or 6, no
//!   interlace), then the image data, then an empty `IEND`, each chunk
//!   with zlib's CRC-32 over its type and data;
//! * the data is one `IDAT` per `encoder.encode(bufsize)` call, with
//!   `bufsize = max(ImageFile.MAXBLOCK, width * 4)` and `MAXBLOCK` 65536;
//!   a call returns whatever deflate wrote into a buffer of that size, so
//!   a picture whose stream is longer than it gets more than one `IDAT`;
//! * deflate is `deflateInit2(Z_BEST_COMPRESSION, Z_DEFLATED, 15, 9,
//!   Z_FILTERED)` (`optimize` sets the level, PNG mode the strategy), and
//!   each row is deflated alone, with `Z_NO_FLUSH`, as soon as it is
//!   filtered; input left over when a buffer fills is deflated first on
//!   the next call; the image's end is `Z_FINISH` until the stream ends;
//! * each row's filter is the one whose bytes, read as signed, sum least
//!   in absolute value, tried in the order None, Up, Sub, Average, Paeth,
//!   a later one taken only when strictly less, and none tried once a sum
//!   is 0; the row above the first is zeros.

use crate::zlib::{self, Deflate};

/// `ImageFile.MAXBLOCK`.
pub const MAXBLOCK: usize = 65536;

/// The two modes the pictures are saved in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Colour type 2, three bytes a pixel.
    Rgb,
    /// Colour type 6, four bytes a pixel.
    Rgba,
}

impl Mode {
    #[must_use]
    pub fn bpp(self) -> usize {
        match self {
            Mode::Rgb => 3,
            Mode::Rgba => 4,
        }
    }

    fn colour_type(self) -> u8 {
        match self {
            Mode::Rgb => 2,
            Mode::Rgba => 6,
        }
    }
}

/// An image: `w` by `h` pixels of `mode`, row-major, no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub mode: Mode,
    pub px: Vec<u8>,
}

impl Image {
    /// The pixel at (x, y), its channels.
    #[must_use]
    pub fn at(&self, x: usize, y: usize) -> &[u8] {
        let b = self.mode.bpp();
        let i = (y * self.w + x) * b;
        &self.px[i..i + b]
    }
}

/// `putchunk`: length, type, data, CRC-32 of type and data.
fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend(
        u32::try_from(data.len())
            .expect("a chunk under 4 GB")
            .to_be_bytes(),
    );
    out.extend(kind);
    out.extend(data);
    out.extend(zlib::crc(zlib::crc(0, kind), data).to_be_bytes());
}

/// Sum of a filtered row's bytes as `ZipEncode.c` counts them.
fn cost(row: &[u8]) -> i64 {
    row.iter()
        .map(|&v| {
            if v < 128 {
                i64::from(v)
            } else {
                256 - i64::from(v)
            }
        })
        .sum()
}

/// One row filtered the way `ZipEncode.c` picks: the filter byte first.
fn filter(row: &[u8], prev: &[u8], bpp: usize) -> Vec<u8> {
    let n = row.len();
    let mut best: Vec<u8> = Vec::with_capacity(n + 1);
    best.push(0);
    best.extend(row);
    let mut sum = cost(row);
    let try_one = |kind: u8, f: &dyn Fn(usize) -> u8, sum: &mut i64, best: &mut Vec<u8>| {
        let mut out = Vec::with_capacity(n + 1);
        out.push(kind);
        out.extend((0..n).map(f));
        let s = cost(&out[1..]);
        if s < *sum {
            *sum = s;
            *best = out;
        }
    };
    let left = |i: usize| if i >= bpp { row[i - bpp] } else { 0 };
    let up_left = |i: usize| if i >= bpp { prev[i - bpp] } else { 0 };
    if sum > 0 {
        try_one(2, &|i| row[i].wrapping_sub(prev[i]), &mut sum, &mut best);
    }
    if sum > 0 {
        try_one(1, &|i| row[i].wrapping_sub(left(i)), &mut sum, &mut best);
    }
    if sum > 0 {
        try_one(
            3,
            &|i| {
                let avg = if i >= bpp {
                    (u16::from(row[i - bpp]) + u16::from(prev[i])) / 2
                } else {
                    u16::from(prev[i]) / 2
                };
                row[i].wrapping_sub(u8::try_from(avg).expect("an average of two bytes"))
            },
            &mut sum,
            &mut best,
        );
    }
    if sum > 0 {
        try_one(
            4,
            &|i| {
                let (a, b, c) = (
                    i32::from(left(i)),
                    i32::from(prev[i]),
                    i32::from(up_left(i)),
                );
                let (pa, pb, pc) = ((b - c).abs(), (a - c).abs(), (a + b - 2 * c).abs());
                let p = if pa <= pb && pa <= pc {
                    a
                } else if pb <= pc {
                    b
                } else {
                    c
                };
                row[i].wrapping_sub(u8::try_from(p).expect("a predictor is a byte"))
            },
            &mut sum,
            &mut best,
        );
    }
    best
}

/// `ImagingZipEncode`'s state between calls.
struct Encoder<'a> {
    im: &'a Image,
    z: Deflate,
    y: usize,
    finishing: bool,
    /// The filtered row deflate may still be reading.
    line: Vec<u8>,
    prev: Vec<u8>,
}

impl Encoder<'_> {
    /// One `encode(bufsize)` call: the bytes it returns and whether the
    /// stream ended.
    fn encode(&mut self, bufsize: usize) -> (Vec<u8>, bool) {
        let mut buf = vec![0u8; bufsize];
        self.z.set_out(&mut buf);
        if self.z.has_in() {
            let err = self.z.deflate(zlib::Z_NO_FLUSH);
            assert!(err >= 0, "deflate: {err}");
        }
        let row = self.im.w * self.im.mode.bpp();
        let mut end = false;
        if !self.finishing {
            while self.z.avail_out() > 0 {
                if self.y >= self.im.h {
                    self.finishing = true;
                    break;
                }
                let at = self.y * row;
                let raw = &self.im.px[at..at + row];
                self.line = filter(raw, &self.prev, self.im.mode.bpp());
                self.prev.copy_from_slice(raw);
                self.y += 1;
                self.z.set_in(&self.line);
                let err = self.z.deflate(zlib::Z_NO_FLUSH);
                assert!(err >= 0, "deflate: {err}");
            }
        }
        if self.finishing && self.z.avail_out() > 0 {
            while self.z.avail_out() > 0 {
                let err = self.z.deflate(zlib::Z_FINISH);
                if err == zlib::Z_STREAM_END {
                    end = true;
                    break;
                }
                assert!(err >= 0, "deflate: {err}");
            }
        }
        let used = bufsize - self.z.avail_out() as usize;
        buf.truncate(used);
        (buf, end)
    }
}

/// The PNG bytes Pillow writes for `im`, its data split as an encode
/// buffer of `bufsize` bytes splits it.
#[must_use]
pub fn encode_with(im: &Image, bufsize: usize) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend(u32::try_from(im.w).expect("a width").to_be_bytes());
    ihdr.extend(u32::try_from(im.h).expect("a height").to_be_bytes());
    ihdr.extend([8, im.mode.colour_type(), 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let mut e = Encoder {
        im,
        z: Deflate::new(zlib::Z_BEST_COMPRESSION, zlib::Z_FILTERED),
        y: 0,
        finishing: false,
        line: Vec::new(),
        prev: vec![0u8; im.w * im.mode.bpp()],
    };
    loop {
        let (data, end) = e.encode(bufsize);
        chunk(&mut out, b"IDAT", &data);
        if end {
            break;
        }
    }
    chunk(&mut out, b"IEND", b"");
    out
}

/// `im.save(buf, "PNG", optimize=True)`.
#[must_use]
pub fn encode(im: &Image) -> Vec<u8> {
    encode_with(im, MAXBLOCK.max(im.w * 4))
}

/// A PNG's chunks: (type, data), in order. `None` when it is not a PNG.
#[must_use]
pub fn chunks(b: &[u8]) -> Option<Vec<(String, &[u8])>> {
    if !b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 8;
    while i + 8 <= b.len() {
        let n = u32::from_be_bytes(b[i..i + 4].try_into().expect("four bytes")) as usize;
        let kind = b[i + 4..i + 8].iter().map(|&c| char::from(c)).collect();
        let data = b.get(i + 8..i + 8 + n).unwrap_or(&b[b.len()..]);
        out.push((kind, data));
        i += 12 + n;
    }
    Some(out)
}

/// `Image.open(png)`: an 8-bit, non-interlaced RGB or RGBA PNG's pixels.
///
/// # Errors
/// When it is not such a PNG.
pub fn decode(b: &[u8]) -> Result<Image, String> {
    let chunks = chunks(b).ok_or("not a PNG")?;
    let ihdr = chunks
        .iter()
        .find(|(k, _)| k == "IHDR")
        .map(|(_, d)| *d)
        .ok_or("no IHDR")?;
    if ihdr.len() != 13 {
        return Err("an IHDR not 13 bytes".into());
    }
    let w = u32::from_be_bytes(ihdr[0..4].try_into().expect("four bytes")) as usize;
    let h = u32::from_be_bytes(ihdr[4..8].try_into().expect("four bytes")) as usize;
    let mode = match (ihdr[8], ihdr[9], ihdr[12]) {
        (8, 2, 0) => Mode::Rgb,
        (8, 6, 0) => Mode::Rgba,
        other => return Err(format!("depth, colour type, interlace {other:?}")),
    };
    let data: Vec<u8> = chunks
        .iter()
        .filter(|(k, _)| k == "IDAT")
        .flat_map(|(_, d)| d.iter().copied())
        .collect();
    let bpp = mode.bpp();
    let row = w * bpp;
    let raw = zlib::inflate_all(&data, 15, Some((row + 1) * h))?;
    let mut px = vec![0u8; row * h];
    for y in 0..h {
        let line = &raw[y * (row + 1)..(y + 1) * (row + 1)];
        let (kind, f) = (line[0], &line[1..]);
        for i in 0..row {
            let a = if i >= bpp { px[y * row + i - bpp] } else { 0 };
            let b = if y > 0 { px[(y - 1) * row + i] } else { 0 };
            let c = if i >= bpp && y > 0 {
                px[(y - 1) * row + i - bpp]
            } else {
                0
            };
            let p = match kind {
                0 => 0,
                1 => a,
                2 => b,
                3 => u8::try_from((u16::from(a) + u16::from(b)) / 2).expect("a byte"),
                4 => {
                    let (ai, bi, ci) = (i32::from(a), i32::from(b), i32::from(c));
                    let (pa, pb, pc) = ((bi - ci).abs(), (ai - ci).abs(), (ai + bi - 2 * ci).abs());
                    if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    }
                }
                k => return Err(format!("filter type {k}")),
            };
            px[y * row + i] = f[i].wrapping_add(p);
        }
    }
    Ok(Image { w, h, mode, px })
}
