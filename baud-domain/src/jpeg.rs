//! **JPEG, decoded to pixels** (`[[source: itu-t81]]`): the DCT-based
//! processes with Huffman coding at 8-bit precision -- baseline (`SOF0`),
//! extended sequential (`SOF1`) and progressive (`SOF2`), spectral selection
//! and successive approximation both (Annex G) -- in a JFIF file
//! (`[[source: jfif-102]]`), turned as its Exif orientation says
//! (`[[source: exif-232]]`).
//!
//! **Why it is here.** An iTerm2 inline picture is "Any image format that
//! macOS supports" (`[[source: iterm2-images]]`): iTerm2 hands the file to
//! `NSImage`'s `initWithData:` and draws the bitmap that comes back
//! (`[[source: iterm2-964a75d-image-with-data]]`), so a JPEG `imgcat`
//! sends draws there and drew the broken-image placeholder here. "No engine
//! dependencies" covers image libraries, as it covers PNG's
//! (`crate::png`), so the decoder is baud's own.
//!
//! **What the standard leaves to the decoder, and whose choice this is.**
//! T.81 defines the inverse DCT by its formula (A.3.3) and an accuracy it
//! must meet, not one output, and leaves colour and the upsampling of a
//! subsampled component to the file format. Each is taken from the one
//! reference decode the IJG's test set gives, so a decode here can be
//! held to it pixel for pixel: the inverse DCT is the IJG's accurate
//! integer one (`jidctint.c`, Loeffler, Ligtenberg and Moschytz's
//! factorisation in 13-bit constants), a component subsampled two to one
//! is interpolated by its triangle filter (`jdsample.c`), which puts each
//! subsampled sample where JFIF sites it, centred between the samples it
//! covers ("Spatial Relationship of Components"), and YCbCr is turned to
//! RGB by JFIF's equations in its 16-bit fixed point (`jdcolor.c`). The
//! arithmetic is the IJG's, its notice in `LICENSES/ijg-README.txt`
//! (`[[source: ijg-jpeg-6b]]`); `testorig.jpg` (baseline, 4:2:0) and
//! `testprog.jpg` (its progressive equivalent) decode here to the bytes of
//! `testimg.ppm`, as its makefile's `test` target requires of `djpeg -dct
//! int`. macOS's decoder is not the IJG's, so a picture here and in iTerm2
//! may differ by the rounding of either.
//!
//! **What is refused, and counted where the caller counts.** The lossless
//! (`SOF3`) and hierarchical (`SOF5`-`SOF7`) processes, arithmetic coding
//! (`SOF9`-`SOF15`, `DAC`), 12-bit samples, a height left to `DNL`, two
//! components, and sampling factors whose ratio is not whole: each
//! [`JpegError::Unsupported`], drawn by the caller as a picture that does
//! not decode.
//!
//! **Malformed input never panics**, and the work is bounded by the file
//! and the frame. Every length is checked against the file, every
//! arithmetic on a coefficient wraps as the IJG's `int` would, a code no
//! table holds decodes as zero, a scan cut short reads zero bits, and the
//! scans read are at most what a valid file can carry: in a sequential
//! file every component is in exactly one scan (B.2.3), and in a
//! progressive one each coefficient's bits are each coded once (G.1.1.1),
//! so a component takes at most fourteen scans for its DC and fourteen
//! for each of its 63 AC coefficients -- one first and thirteen
//! refinements, `Al` being at most 13.

use crate::Pixel;
use crate::png::Size;

/// Why a JPEG could not be decoded. Every variant is a property of the
/// bytes, but for [`JpegError::Buffer`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JpegError {
    /// Not a JPEG: no `SOI`, no frame before the first scan, a segment
    /// whose length runs past the file.
    Malformed,
    /// A process or a form this decoder does not implement.
    Unsupported,
    /// A frame header T.81 does not allow (B.2.2).
    Frame,
    /// A table T.81 does not allow (B.2.4): a Huffman table whose codes
    /// overflow their lengths, or an identifier past 3.
    Table,
    /// A scan header T.81 does not allow (B.2.3, G.1.1.1), or one naming a
    /// table not defined.
    Scan,
    /// A caller buffer smaller than [`Jpeg::scratch_len`] or the picture.
    Buffer,
}

/// **The Exif orientation, "The image orientation viewed in terms of rows
/// and columns"** (`[[source: exif-232]]`, tag 274): where the coded
/// image's 0th row and 0th column are shown. A value outside 1 to 8 is
/// "reserved" and read as the default, 1.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation {
    /// 1: "The 0th row is at the visual top of the image, and the 0th
    /// column is the visual left-hand side."
    TopLeft,
    /// 2: the 0th column on the right, a mirror.
    TopRight,
    /// 3: the 0th row at the bottom and the 0th column on the right, a
    /// half turn.
    BottomRight,
    /// 4: the 0th row at the bottom, a flip.
    BottomLeft,
    /// 5: the 0th row on the left and the 0th column at the top, a
    /// transpose.
    LeftTop,
    /// 6: the 0th row on the right and the 0th column at the top, a
    /// quarter turn clockwise.
    RightTop,
    /// 7: the 0th row on the right and the 0th column at the bottom.
    RightBottom,
    /// 8: the 0th row on the left and the 0th column at the bottom, a
    /// quarter turn anticlockwise.
    LeftBottom,
}

impl Orientation {
    /// The tag's value read: "Other = reserved", the default for it.
    pub fn of(v: u16) -> Orientation {
        match v {
            2 => Orientation::TopRight,
            3 => Orientation::BottomRight,
            4 => Orientation::BottomLeft,
            5 => Orientation::LeftTop,
            6 => Orientation::RightTop,
            7 => Orientation::RightBottom,
            8 => Orientation::LeftBottom,
            _ => Orientation::TopLeft,
        }
    }

    /// Whether the shown image's rows are the coded image's columns.
    fn turns(self) -> bool {
        matches!(
            self,
            Orientation::LeftTop
                | Orientation::RightTop
                | Orientation::RightBottom
                | Orientation::LeftBottom
        )
    }

    /// **Where the coded pixel `(x, y)` of a `w` by `h` image is shown**:
    /// the column and row in the shown image, from Exif's Figure 12.
    fn place(self, x: usize, y: usize, w: usize, h: usize) -> (usize, usize) {
        match self {
            Orientation::TopLeft => (x, y),
            Orientation::TopRight => (w - 1 - x, y),
            Orientation::BottomRight => (w - 1 - x, h - 1 - y),
            Orientation::BottomLeft => (x, h - 1 - y),
            Orientation::LeftTop => (y, x),
            Orientation::RightTop => (h - 1 - y, x),
            Orientation::RightBottom => (h - 1 - y, w - 1 - x),
            Orientation::LeftBottom => (y, w - 1 - x),
        }
    }
}

/// How the components are turned into RGB: JFIF's YCbCr, grey, or what an
/// Adobe `APP14` or the component identifiers say, as the IJG reads a file
/// that carries no JFIF marker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Colour {
    Grey,
    YCbCr,
    Rgb,
    /// Four components, inverted as Adobe's applications write them.
    Cmyk,
    /// Adobe's YCCK: YCbCr for the inverted CMY, and K.
    Ycck,
}

/// Whether the frame is coded in one pass or many.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Process {
    Sequential,
    Progressive,
}

/// A frame component: its identifier, sampling factors and table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Comp {
    id: u8,
    h: u8,
    v: u8,
    tq: u8,
}

impl Comp {
    const NONE: Comp = Comp {
        id: 0,
        h: 1,
        v: 1,
        tq: 0,
    };
}

/// The frame header, `SOFn` (B.2.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Frame {
    width: u16,
    height: u16,
    n: usize,
    comps: [Comp; 4],
    hmax: u8,
    vmax: u8,
    process: Process,
}

impl Frame {
    /// MCUs across and down: "the number of MCUs ... is the number of
    /// data units ... rounded up" (A.2.4).
    fn mcus(&self) -> (usize, usize) {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        (
            w.div_ceil(8 * usize::from(self.hmax)),
            h.div_ceil(8 * usize::from(self.vmax)),
        )
    }

    /// Component `c`'s samples across and down, `ceil(X * Hi / Hmax)`
    /// (A.1.1).
    fn samples(&self, c: usize) -> (usize, usize) {
        let k = self.comps[c];
        (
            (usize::from(self.width) * usize::from(k.h)).div_ceil(usize::from(self.hmax)),
            (usize::from(self.height) * usize::from(k.v)).div_ceil(usize::from(self.vmax)),
        )
    }

    /// Component `c`'s data units across and down in its plane, the MCUs
    /// whole: the blocks an interleaved scan codes.
    fn blocks(&self, c: usize) -> (usize, usize) {
        let (mx, my) = self.mcus();
        let k = self.comps[c];
        (mx * usize::from(k.h), my * usize::from(k.v))
    }
}

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const DAC: u8 = 0xCC;

/// The zig-zag order (Figure A.6) as natural indices, sixteen more of the
/// last so a run that overshoots the block lands in it, as the IJG pads
/// its jpeg_natural_order.
const ZIGZAG: [u8; 80] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63, 63,
    63, 63, 63, 63, 63, 63,
];

fn be16(d: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*d.get(i)?, *d.get(i + 1)?]))
}

/// **The next marker at or after `pos`**: past any bytes that are not one
/// and the fill bytes before it ("Any marker may optionally be preceded by
/// any number of fill bytes, which are bytes assigned code X'FF'", B.1.1.2),
/// its code and the position after it.
fn next_marker(d: &[u8], mut pos: usize) -> Option<(u8, usize)> {
    loop {
        while *d.get(pos)? != 0xFF {
            pos += 1;
        }
        while *d.get(pos)? == 0xFF {
            pos += 1;
        }
        match *d.get(pos)? {
            0 => pos += 1,
            m => return Some((m, pos + 1)),
        }
    }
}

/// A marker segment's body from `pos`, past its two length bytes, and the
/// position after it.
fn segment(d: &[u8], pos: usize) -> Result<(&[u8], usize), JpegError> {
    let len = usize::from(be16(d, pos).ok_or(JpegError::Malformed)?);
    if len < 2 {
        return Err(JpegError::Malformed);
    }
    let body = d.get(pos + 2..pos + len).ok_or(JpegError::Malformed)?;
    Ok((body, pos + len))
}

/// Whether a marker is one with no segment: `RSTm`, `SOI`, `EOI`, `TEM`.
fn standalone(m: u8) -> bool {
    matches!(m, 0xD0..=0xD9 | 0x01)
}

/// **The Exif orientation in an `APP1` body**, `Exif\0\0` and a TIFF
/// header, either byte order, then the 0th IFD's entries; `None` when the
/// body is not Exif or carries no well-formed tag 274, a `SHORT` of count
/// one.
fn exif_orientation(body: &[u8]) -> Option<Orientation> {
    let tiff = body.strip_prefix(b"Exif\0\0")?;
    let little = match tiff.get(..4)? {
        b"II\x2A\x00" => true,
        b"MM\x00\x2A" => false,
        _ => return None,
    };
    let u16_at = |i: usize| {
        let b = [*tiff.get(i)?, *tiff.get(i + 1)?];
        Some(match little {
            true => u16::from_le_bytes(b),
            false => u16::from_be_bytes(b),
        })
    };
    let u32_at = |i: usize| {
        let b = [
            *tiff.get(i)?,
            *tiff.get(i + 1)?,
            *tiff.get(i + 2)?,
            *tiff.get(i + 3)?,
        ];
        Some(match little {
            true => u32::from_le_bytes(b),
            false => u32::from_be_bytes(b),
        })
    };
    let ifd = usize::try_from(u32_at(4)?).ok()?;
    let n = usize::from(u16_at(ifd)?);
    (0..n).find_map(|k| {
        let e = ifd.checked_add(2 + 12 * k)?;
        (u16_at(e)? == 274 && u16_at(e + 2)? == 3 && u32_at(e + 4)? == 1)
            .then(|| u16_at(e + 8).map(Orientation::of))
            .flatten()
    })
}

/// **A parsed JPEG**: its frame, its colour, its orientation, and the
/// bytes [`Jpeg::decode`] walks again for the tables and scans.
#[derive(Clone, Copy, Debug)]
pub struct Jpeg<'a> {
    data: &'a [u8],
    frame: Frame,
    colour: Colour,
    orientation: Orientation,
}

impl<'a> Jpeg<'a> {
    /// Whether `d` begins as a JPEG does, `SOI` and a marker after it.
    pub fn sniff(d: &[u8]) -> bool {
        d.len() >= 3 && d[0] == 0xFF && d[1] == SOI && d[2] == 0xFF
    }

    /// **The headers up to the first scan**: the frame, the markers that
    /// say its colour -- JFIF's `APP0`, Adobe's `APP14` -- and the first
    /// Exif `APP1`'s orientation.
    pub fn parse(data: &'a [u8]) -> Result<Jpeg<'a>, JpegError> {
        if data.get(..2) != Some(&[0xFF, SOI]) {
            return Err(JpegError::Malformed);
        }
        let mut pos = 2;
        let mut frame = None;
        let (mut jfif, mut adobe, mut orientation) = (None, None, None);
        loop {
            let (m, at) = next_marker(data, pos).ok_or(JpegError::Malformed)?;
            if standalone(m) {
                if m == EOI || m == SOI {
                    return Err(JpegError::Malformed);
                }
                pos = at;
                continue;
            }
            let (body, next) = segment(data, at)?;
            pos = next;
            match m {
                0xC0..=0xC2 => {
                    if frame.is_some() {
                        return Err(JpegError::Malformed);
                    }
                    frame = Some(read_frame(m, body)?);
                }
                0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF | DAC => {
                    return Err(JpegError::Unsupported);
                }
                0xE0 if body.starts_with(b"JFIF\0") => jfif = Some(()),
                0xE1 if orientation.is_none() => orientation = exif_orientation(body),
                0xEE if body.starts_with(b"Adobe") && body.len() >= 12 => adobe = Some(body[11]),
                SOS => break,
                _ => {}
            }
        }
        let frame = frame.ok_or(JpegError::Malformed)?;
        let ids = (frame.comps[0].id, frame.comps[1].id, frame.comps[2].id);
        let colour = match (frame.n, jfif, adobe) {
            (1, ..) => Colour::Grey,
            (3, Some(()), _) => Colour::YCbCr,
            (3, None, Some(0)) => Colour::Rgb,
            (3, None, Some(_)) => Colour::YCbCr,
            (3, None, None) if ids == (b'R', b'G', b'B') => Colour::Rgb,
            (3, None, None) => Colour::YCbCr,
            (4, _, Some(0) | None) => Colour::Cmyk,
            (4, _, Some(_)) => Colour::Ycck,
            _ => return Err(JpegError::Unsupported),
        };
        Ok(Jpeg {
            data,
            frame,
            colour,
            orientation: orientation.unwrap_or(Orientation::TopLeft),
        })
    }

    /// The picture's size as shown: the frame's, its sides swapped by an
    /// orientation that turns it.
    pub fn size(&self) -> Size {
        let (w, h) = (u32::from(self.frame.width), u32::from(self.frame.height));
        match self.orientation.turns() {
            true => Size {
                width: h,
                height: w,
            },
            false => Size {
                width: w,
                height: h,
            },
        }
    }

    /// The orientation the picture is shown in.
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// **The bytes of work [`Jpeg::decode`] needs**: each component's
    /// samples in whole MCUs, a progressive frame's coefficients, two bytes
    /// each, as many again, and a row of each component at the picture's
    /// width. `None` past what a `usize` holds.
    pub fn scratch_len(&self) -> Option<usize> {
        Some(self.layout()?.total)
    }

    fn layout(&self) -> Option<Layout> {
        let f = &self.frame;
        let mut lay = Layout {
            plane: [0; 4],
            coef: [0; 4],
            rows: 0,
            total: 0,
        };
        let mut at = 0usize;
        for c in 0..f.n {
            let (bw, bh) = f.blocks(c);
            lay.plane[c] = at;
            at = at.checked_add(bw.checked_mul(bh)?.checked_mul(64)?)?;
        }
        for c in 0..f.n {
            let (bw, bh) = f.blocks(c);
            lay.coef[c] = at;
            if f.process == Process::Progressive {
                at = at.checked_add(bw.checked_mul(bh)?.checked_mul(128)?)?;
            }
        }
        lay.rows = at;
        at = at.checked_add(usize::from(f.width).checked_mul(f.n)?)?;
        lay.total = at;
        Some(lay)
    }

    /// **The picture decoded into `out`**, [`Jpeg::size`] pixels, row by
    /// row as shown, every one opaque, with `scratch` of at least
    /// [`Jpeg::scratch_len`] bytes for the work: the tables and scans read
    /// in the file's order to its `EOI`, or to its end where it has none,
    /// as a decoder given a file cut short shows what arrived.
    pub fn decode(&self, scratch: &mut [u8], out: &mut [Pixel]) -> Result<Size, JpegError> {
        let lay = self.layout().ok_or(JpegError::Buffer)?;
        let size = self.size();
        let area = (size.width as usize) * (size.height as usize);
        let work = scratch.get_mut(..lay.total).ok_or(JpegError::Buffer)?;
        let out = out.get_mut(..area).ok_or(JpegError::Buffer)?;
        // The planes mid-grey, what a block no scan reaches decodes to,
        // and the coefficients zero.
        work[..lay.coef[0]].fill(128);
        work[lay.coef[0]..lay.rows].fill(0);
        let mut d = Decoder::new(self.frame, lay);
        d.run(self.data, work)?;
        if self.frame.process == Process::Progressive {
            d.idct_all(work);
        }
        self.output(&lay, work, out);
        Ok(size)
    }

    /// **The planes turned into pixels**: each row of each component
    /// brought to the picture's width -- as it is, by the IJG's triangle
    /// filter where it is subsampled two to one across, or across and
    /// down, and wider than two samples, else each sample repeated -- then
    /// the colour, each pixel put where the orientation shows it.
    fn output(&self, lay: &Layout, work: &mut [u8], out: &mut [Pixel]) {
        let f = &self.frame;
        let (w, h) = (usize::from(f.width), usize::from(f.height));
        let shown_w = match self.orientation.turns() {
            true => h,
            false => w,
        };
        let (planes, rows) = work.split_at_mut(lay.rows);
        for y in 0..h {
            for c in 0..f.n {
                let row = &mut rows[c * w..(c + 1) * w];
                upsample(f, c, &planes[lay.plane[c]..], y, row);
            }
            for x in 0..w {
                let s = |c: usize| rows[c * w + x];
                let px = match self.colour {
                    Colour::Grey => {
                        let v = s(0);
                        Pixel::opaque_bytes(v, v, v)
                    }
                    Colour::YCbCr => {
                        let (r, g, b) = ycc_rgb(s(0), s(1), s(2));
                        Pixel::opaque_bytes(r, g, b)
                    }
                    Colour::Rgb => Pixel::opaque_bytes(s(0), s(1), s(2)),
                    Colour::Cmyk => cmyk(s(0), s(1), s(2), s(3)),
                    Colour::Ycck => {
                        let (r, g, b) = ycc_rgb(s(0), s(1), s(2));
                        cmyk(255 - r, 255 - g, 255 - b, s(3))
                    }
                };
                let (ox, oy) = self.orientation.place(x, y, w, h);
                out[oy * shown_w + ox] = px;
            }
        }
    }
}

/// Where each part of the work lies in the scratch.
#[derive(Clone, Copy, Debug)]
struct Layout {
    plane: [usize; 4],
    coef: [usize; 4],
    rows: usize,
    total: usize,
}

/// **The frame header read** (B.2.2): 8-bit samples alone, a height given,
/// one, three or four components, each sampling factor 1 to 4 (B.2.2) and
/// a whole fraction of the largest, each table 0 to 3.
fn read_frame(m: u8, b: &[u8]) -> Result<Frame, JpegError> {
    let process = match m {
        0xC2 => Process::Progressive,
        _ => Process::Sequential,
    };
    if b.len() < 6 {
        return Err(JpegError::Malformed);
    }
    if b[0] != 8 {
        return Err(JpegError::Unsupported);
    }
    let height = be16(b, 1).ok_or(JpegError::Malformed)?;
    let width = be16(b, 3).ok_or(JpegError::Malformed)?;
    let n = usize::from(b[5]);
    if b.len() != 6 + 3 * n {
        return Err(JpegError::Malformed);
    }
    if height == 0 {
        // "If Y = 0 ... the number of lines ... shall be defined by the
        // DNL marker", which this decoder does not read.
        return Err(JpegError::Unsupported);
    }
    if width == 0 || n == 0 {
        return Err(JpegError::Frame);
    }
    if !matches!(n, 1 | 3 | 4) {
        return Err(JpegError::Unsupported);
    }
    let mut comps = [Comp::NONE; 4];
    for (c, k) in comps.iter_mut().enumerate().take(n) {
        let e = &b[6 + 3 * c..9 + 3 * c];
        *k = Comp {
            id: e[0],
            h: e[1] >> 4,
            v: e[1] & 15,
            tq: e[2],
        };
        if !(1..=4).contains(&k.h) || !(1..=4).contains(&k.v) || k.tq > 3 {
            return Err(JpegError::Frame);
        }
    }
    for i in 0..n {
        for j in 0..i {
            if comps[i].id == comps[j].id {
                return Err(JpegError::Frame);
            }
        }
    }
    let hmax = comps[..n].iter().map(|k| k.h).max().unwrap_or(1);
    let vmax = comps[..n].iter().map(|k| k.v).max().unwrap_or(1);
    if comps[..n]
        .iter()
        .any(|k| hmax % k.h != 0 || vmax % k.v != 0)
    {
        return Err(JpegError::Unsupported);
    }
    Ok(Frame {
        width,
        height,
        n,
        comps,
        hmax,
        vmax,
        process,
    })
}

/// **A Huffman decoding table** (F.2.2.3): a nine-bit lookup for the short
/// codes, the IJG's look_nbits and look_sym in one word, and
/// `MAXCODE`, `VALPTR` and `MINCODE` for the rest.
#[derive(Clone, Copy)]
struct Huff {
    /// `len << 8 | symbol` for every nine-bit prefix of a code nine bits
    /// long or less; zero for the rest.
    fast: [u16; 512],
    maxcode: [i32; 17],
    offset: [i32; 17],
    vals: [u8; 256],
}

impl Huff {
    /// **The table a `DHT` defines** from `BITS` and `HUFFVAL`, its codes
    /// generated as Annex C does; refused when the counts come to more
    /// than 256 values or a length's codes overflow it, "the all-1-bits
    /// code word of any length ... reserved as a prefix for longer code
    /// words" (K.2).
    fn build(bits: &[u8], vals: &[u8]) -> Option<Huff> {
        let mut t = Huff {
            fast: [0; 512],
            maxcode: [-1; 17],
            offset: [0; 17],
            vals: [0; 256],
        };
        t.vals.get_mut(..vals.len())?.copy_from_slice(vals);
        let mut code: i32 = 0;
        let mut k: usize = 0;
        for l in 1..=16usize {
            let count = usize::from(bits[l - 1]);
            t.offset[l] = i32::try_from(k).ok()? - code;
            for _ in 0..count {
                if l <= 9 {
                    let sym = *vals.get(k)?;
                    let base = usize::try_from(code).ok()? << (9 - l);
                    let word = (u16::try_from(l).ok()? << 8) | u16::from(sym);
                    t.fast.get_mut(base..base + (1 << (9 - l)))?.fill(word);
                }
                code += 1;
                k += 1;
            }
            if count > 0 {
                t.maxcode[l] = code - 1;
            }
            if code >= 1 << l {
                return None;
            }
            code <<= 1;
        }
        (k == vals.len()).then_some(t)
    }

    /// **One symbol**: the nine-bit lookup, else the lengths 10 to 16 in
    /// turn (F.2.2.3's `DECODE`). A code no length holds is zero, as the
    /// IJG's decoder makes it, and takes sixteen bits.
    fn decode(&self, b: &mut Bits<'_>) -> u8 {
        b.fill();
        let w = self.fast[(b.acc >> 55) as usize];
        if w != 0 {
            b.drop(u32::from(w >> 8));
            return w as u8;
        }
        for l in 10..=16u32 {
            let code = (b.acc >> (64 - l)) as i32;
            if code <= self.maxcode[l as usize] {
                b.drop(l);
                let i = code + self.offset[l as usize];
                return usize::try_from(i)
                    .ok()
                    .and_then(|i| self.vals.get(i))
                    .copied()
                    .unwrap_or(0);
            }
        }
        b.drop(16);
        0
    }
}

/// **The bits of an entropy-coded segment** (F.2.2.5), most significant
/// first, a stuffed `X'FF00'` read as the data byte `X'FF'`; at a marker,
/// or the file's end, zero bits for ever, as the IJG's reader feeds a
/// decoder that has run out of data.
struct Bits<'a> {
    d: &'a [u8],
    pos: usize,
    acc: u64,
    n: u32,
    /// The marker reached, and where its `X'FF'` is.
    hit: Option<(u8, usize)>,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Bits<'a> {
        Bits {
            d,
            pos,
            acc: 0,
            n: 0,
            hit: None,
        }
    }

    fn fill(&mut self) {
        while self.n <= 56 {
            let byte = match self.hit {
                Some(_) => 0,
                None => self.byte(),
            };
            self.acc |= u64::from(byte) << (56 - self.n);
            self.n += 8;
        }
    }

    /// The next data byte, or zero at a marker or the end.
    fn byte(&mut self) -> u8 {
        let Some(&b) = self.d.get(self.pos) else {
            self.hit = Some((EOI, self.pos));
            return 0;
        };
        if b != 0xFF {
            self.pos += 1;
            return b;
        }
        let ff = self.pos;
        let mut i = self.pos + 1;
        while self.d.get(i) == Some(&0xFF) {
            i += 1;
        }
        match self.d.get(i) {
            Some(0) => {
                self.pos = i + 1;
                0xFF
            }
            Some(&m) => {
                self.hit = Some((m, ff.max(i - 1)));
                0
            }
            None => {
                self.hit = Some((EOI, ff));
                0
            }
        }
    }

    fn drop(&mut self, k: u32) {
        let k = k.min(self.n);
        self.acc = match k {
            64 => 0,
            k => self.acc << k,
        };
        self.n -= k;
    }

    /// `RECEIVE(k)`: the next `k` bits, at most 16.
    fn get(&mut self, k: u32) -> i32 {
        if k == 0 || k > 16 {
            return 0;
        }
        self.fill();
        let v = (self.acc >> (64 - k)) as i32;
        self.drop(k);
        v
    }

    /// **At a restart interval's end** (F.2.2.5's restart): the bits
    /// left in the byte dropped, and the `RSTm` that should follow taken,
    /// past any bytes before it; another marker is left for the scan's end
    /// to find.
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        if self.hit.is_none() {
            match next_marker(self.d, self.pos) {
                Some((m, at)) => self.hit = Some((m, at - 2)),
                None => self.hit = Some((EOI, self.d.len())),
            }
        }
        if let Some((0xD0..=0xD7, at)) = self.hit {
            self.pos = at + 2;
            self.hit = None;
        }
    }

    /// Where the segment after the scan begins: at the marker reached, or
    /// the next one past the scan's last byte.
    fn end(&self) -> usize {
        match self.hit {
            Some((_, at)) => at,
            None => self.pos,
        }
    }
}

/// `EXTEND(V, T)` (F.2.2.1): a `T`-bit value as the signed difference it
/// codes.
fn extend(v: i32, t: u32) -> i32 {
    if t == 0 || t > 16 {
        return 0;
    }
    match v < 1 << (t - 1) {
        true => v - (1 << t) + 1,
        false => v,
    }
}

/// A component's place in the scan being decoded.
#[derive(Clone, Copy)]
struct ScanComp {
    c: usize,
    td: usize,
    ta: usize,
}

/// The scan header's fields (B.2.3).
#[derive(Clone, Copy)]
struct Scan {
    comps: [ScanComp; 4],
    n: usize,
    ss: u32,
    se: u32,
    ah: u32,
    al: u32,
}

/// **The decoder's state across the file**: the tables as defined so far,
/// each component's quantization table latched at its first scan, as the
/// IJG latches it, the restart interval, and the scans read.
struct Decoder {
    frame: Frame,
    lay: Layout,
    dc: [Option<Huff>; 4],
    ac: [Option<Huff>; 4],
    quant: [Option<[u16; 64]>; 4],
    latched: [Option<[u16; 64]>; 4],
    restart: usize,
    scans: usize,
}

impl Decoder {
    fn new(frame: Frame, lay: Layout) -> Decoder {
        Decoder {
            frame,
            lay,
            dc: [None; 4],
            ac: [None; 4],
            quant: [None; 4],
            latched: [None; 4],
            restart: 0,
            scans: 0,
        }
    }

    /// The most scans a valid file carries: one per component when
    /// sequential; when progressive, fourteen for each of a component's 64
    /// coefficients.
    fn max_scans(&self) -> usize {
        match self.frame.process {
            Process::Sequential => self.frame.n,
            Process::Progressive => self.frame.n * 64 * 14,
        }
    }

    /// **Every segment from `SOI` on**, the tables defined as they come and
    /// each scan decoded, to `EOI` or the file's end.
    fn run(&mut self, d: &[u8], work: &mut [u8]) -> Result<(), JpegError> {
        let mut pos = 2;
        let mut frames = 0;
        while let Some((m, at)) = next_marker(d, pos) {
            if standalone(m) {
                if m == EOI {
                    break;
                }
                pos = at;
                continue;
            }
            // A file cut short after a scan shows what arrived.
            let (body, next) = match segment(d, at) {
                Ok(s) => s,
                Err(_) if self.scans > 0 => break,
                Err(e) => return Err(e),
            };
            pos = next;
            match m {
                DQT => self.read_dqt(body)?,
                DHT => self.read_dht(body)?,
                DRI => {
                    self.restart = usize::from(be16(body, 0).ok_or(JpegError::Malformed)?);
                }
                0xC0..=0xC2 => {
                    frames += 1;
                    if frames > 1 {
                        return Err(JpegError::Malformed);
                    }
                }
                SOS => {
                    if self.scans == self.max_scans() {
                        break;
                    }
                    self.scans += 1;
                    let scan = self.read_sos(body)?;
                    pos = self.decode_scan(&scan, d, next, work);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `DQT` (B.2.4.1): 8- or 16-bit elements in zig-zag order.
    fn read_dqt(&mut self, mut b: &[u8]) -> Result<(), JpegError> {
        while let Some(&pt) = b.first() {
            let (pq, tq) = (pt >> 4, usize::from(pt & 15));
            if tq > 3 || pq > 1 {
                return Err(JpegError::Table);
            }
            let len = 1 + 64 * (usize::from(pq) + 1);
            let e = b.get(1..len).ok_or(JpegError::Malformed)?;
            let mut q = [0u16; 64];
            for k in 0..64 {
                q[usize::from(ZIGZAG[k])] = match pq {
                    0 => u16::from(e[k]),
                    _ => u16::from_be_bytes([e[2 * k], e[2 * k + 1]]),
                };
            }
            self.quant[tq] = Some(q);
            b = &b[len..];
        }
        Ok(())
    }

    /// `DHT` (B.2.4.2): `Tc`, `Th`, sixteen counts and the values.
    fn read_dht(&mut self, mut b: &[u8]) -> Result<(), JpegError> {
        while let Some(&tc_th) = b.first() {
            let (tc, th) = (tc_th >> 4, usize::from(tc_th & 15));
            if tc > 1 || th > 3 {
                return Err(JpegError::Table);
            }
            let bits = b.get(1..17).ok_or(JpegError::Malformed)?;
            let n: usize = bits.iter().map(|&x| usize::from(x)).sum();
            let vals = b.get(17..17 + n).ok_or(JpegError::Malformed)?;
            let t = Huff::build(bits, vals).ok_or(JpegError::Table)?;
            match tc {
                0 => self.dc[th] = Some(t),
                _ => self.ac[th] = Some(t),
            }
            b = &b[17 + n..];
        }
        Ok(())
    }

    /// **The scan header** (B.2.3), held to G.1.1.1's limits when
    /// progressive: the components the frame's, in its order, at most four,
    /// and only one in an AC scan; `Ss` 0 with `Se` 0 or `Ss` 1 to `Se` 63;
    /// `Ah` and `Al` at most 13. The tables a scan uses are defined, and
    /// each component's quantization table latched at its first scan.
    fn read_sos(&mut self, b: &[u8]) -> Result<Scan, JpegError> {
        let n = usize::from(*b.first().ok_or(JpegError::Malformed)?);
        if !(1..=4).contains(&n) || b.len() != 4 + 2 * n {
            return Err(JpegError::Scan);
        }
        let mut comps = [ScanComp { c: 0, td: 0, ta: 0 }; 4];
        let mut last = None;
        for (k, sc) in comps.iter_mut().enumerate().take(n) {
            let (id, t) = (b[1 + 2 * k], b[2 + 2 * k]);
            let c = (0..self.frame.n)
                .find(|&c| self.frame.comps[c].id == id)
                .ok_or(JpegError::Scan)?;
            if last.is_some_and(|l| c <= l) {
                return Err(JpegError::Scan);
            }
            last = Some(c);
            *sc = ScanComp {
                c,
                td: usize::from(t >> 4),
                ta: usize::from(t & 15),
            };
            if sc.td > 3 || sc.ta > 3 {
                return Err(JpegError::Scan);
            }
        }
        let tail = &b[1 + 2 * n..];
        let (ss, se) = (u32::from(tail[0]), u32::from(tail[1]));
        let (ah, al) = (u32::from(tail[2] >> 4), u32::from(tail[2] & 15));
        let scan = Scan {
            comps,
            n,
            ss,
            se,
            ah,
            al,
        };
        let progressive = self.frame.process == Process::Progressive;
        if progressive {
            let dc = ss == 0;
            if (dc && se != 0) || (!dc && (se < ss || se > 63 || n != 1)) || ah > 13 || al > 13 {
                return Err(JpegError::Scan);
            }
        }
        if n > 1 {
            let units: usize = comps[..n]
                .iter()
                .map(|s| {
                    let k = self.frame.comps[s.c];
                    usize::from(k.h) * usize::from(k.v)
                })
                .sum();
            // "Sum of all Hi x Vi ... shall be <= 10" (B.2.3).
            if units > 10 {
                return Err(JpegError::Scan);
            }
        }
        for s in &comps[..n] {
            let needs_dc = !progressive || (ss == 0 && ah == 0);
            let needs_ac = !progressive || ss > 0;
            if (needs_dc && self.dc[s.td].is_none()) || (needs_ac && self.ac[s.ta].is_none()) {
                return Err(JpegError::Scan);
            }
            if self.latched[s.c].is_none() {
                let tq = usize::from(self.frame.comps[s.c].tq);
                self.latched[s.c] = Some(self.quant[tq].ok_or(JpegError::Scan)?);
            }
        }
        Ok(scan)
    }

    /// **One scan's entropy-coded data from `pos`**, MCU by MCU, restart
    /// markers taken every interval; the position of the marker after it.
    fn decode_scan(&mut self, s: &Scan, d: &[u8], pos: usize, work: &mut [u8]) -> usize {
        let f = self.frame;
        let mut bits = Bits::new(d, pos);
        let mut pred = [0i32; 4];
        let mut eobrun: u32 = 0;
        let (mx, my) = f.mcus();
        let (units_x, units_y) = match s.n {
            1 => {
                let (sw, sh) = f.samples(s.comps[0].c);
                (sw.div_ceil(8), sh.div_ceil(8))
            }
            _ => (mx, my),
        };
        let total = units_x * units_y;
        for m in 0..total {
            if self.restart > 0 && m > 0 && m % self.restart == 0 {
                bits.restart();
                pred = [0; 4];
                eobrun = 0;
            }
            let (ux, uy) = (m % units_x, m / units_x);
            for (k, sc) in s.comps[..s.n].iter().enumerate() {
                let comp = f.comps[sc.c];
                let (h, v) = match s.n {
                    1 => (1, 1),
                    _ => (usize::from(comp.h), usize::from(comp.v)),
                };
                for by in 0..v {
                    for bx in 0..h {
                        let (x, y) = match s.n {
                            1 => (ux, uy),
                            _ => (ux * h + bx, uy * v + by),
                        };
                        self.unit(s, sc, x, y, &mut bits, &mut pred[k], &mut eobrun, work);
                    }
                }
            }
        }
        bits.end()
    }

    /// **One data unit of component `sc` at block `(x, y)`**: sequential,
    /// its coefficients decoded and its samples put in the plane;
    /// progressive, the scan's part of its coefficients decoded into the
    /// ones kept.
    #[allow(clippy::too_many_arguments)]
    fn unit(
        &self,
        s: &Scan,
        sc: &ScanComp,
        x: usize,
        y: usize,
        bits: &mut Bits<'_>,
        pred: &mut i32,
        eobrun: &mut u32,
        work: &mut [u8],
    ) {
        let (bw, _) = self.frame.blocks(sc.c);
        let index = y * bw + x;
        let Some(q) = self.latched[sc.c] else {
            return;
        };
        let mut blk = [0i16; 64];
        match self.frame.process {
            Process::Sequential => {
                let (Some(dc), Some(ac)) = (&self.dc[sc.td], &self.ac[sc.ta]) else {
                    return;
                };
                sequential_unit(dc, ac, bits, pred, &mut blk);
                let at = self.lay.plane[sc.c];
                idct_into(&blk, &q, &mut work[at..], bw * 8, x * 8, y * 8);
            }
            Process::Progressive => {
                let at = self.lay.coef[sc.c] + index * 128;
                let Some(raw) = work.get_mut(at..at + 128) else {
                    return;
                };
                for (k, c) in blk.iter_mut().enumerate() {
                    *c = i16::from_le_bytes([raw[2 * k], raw[2 * k + 1]]);
                }
                match (s.ss, s.ah) {
                    (0, 0) => {
                        if let Some(dc) = &self.dc[sc.td] {
                            let t = u32::from(dc.decode(bits));
                            let diff = extend(bits.get(t), t);
                            *pred = pred.wrapping_add(diff);
                            blk[0] = pred.wrapping_shl(s.al) as i16;
                        }
                    }
                    (0, _) => {
                        if bits.get(1) != 0 {
                            blk[0] |= (1i32 << s.al) as i16;
                        }
                    }
                    (_, 0) => {
                        if let Some(ac) = &self.ac[sc.ta] {
                            ac_first(ac, bits, s, eobrun, &mut blk);
                        }
                    }
                    _ => {
                        if let Some(ac) = &self.ac[sc.ta] {
                            ac_refine(ac, bits, s, eobrun, &mut blk);
                        }
                    }
                }
                for (k, c) in blk.iter().enumerate() {
                    raw[2 * k..2 * k + 2].copy_from_slice(&c.to_le_bytes());
                }
            }
        }
    }

    /// **A progressive frame's coefficients, every block, into its
    /// samples**, once the last scan is read.
    fn idct_all(&self, work: &mut [u8]) {
        for c in 0..self.frame.n {
            let (bw, bh) = self.frame.blocks(c);
            let q = self.latched[c].unwrap_or([0; 64]);
            let (planes, coefs) = work.split_at_mut(self.lay.coef[0]);
            let plane = &mut planes[self.lay.plane[c]..];
            let from = self.lay.coef[c] - self.lay.coef[0];
            for i in 0..bw * bh {
                let raw = &coefs[from + i * 128..from + i * 128 + 128];
                let mut blk = [0i16; 64];
                for (k, v) in blk.iter_mut().enumerate() {
                    *v = i16::from_le_bytes([raw[2 * k], raw[2 * k + 1]]);
                }
                idct_into(&blk, &q, plane, bw * 8, (i % bw) * 8, (i / bw) * 8);
            }
        }
    }
}

/// **A sequential data unit** (F.2.2): the DC difference added to the
/// prediction, then the AC coefficients by run and size to the end of
/// block. A coefficient is kept in sixteen bits, as the IJG's `JCOEF`.
fn sequential_unit(dc: &Huff, ac: &Huff, b: &mut Bits<'_>, pred: &mut i32, blk: &mut [i16; 64]) {
    let t = u32::from(dc.decode(b));
    let diff = extend(b.get(t), t);
    *pred = pred.wrapping_add(diff);
    blk[0] = *pred as i16;
    let mut k = 1usize;
    while k < 64 {
        let rs = ac.decode(b);
        let (r, s) = (usize::from(rs >> 4), u32::from(rs & 15));
        if s != 0 {
            k += r;
            let v = extend(b.get(s), s);
            blk[usize::from(ZIGZAG[k])] = v as i16;
            k += 1;
        } else if r == 15 {
            k += 16;
        } else {
            break;
        }
    }
}

/// **A progressive AC first scan's data unit** (G.1.2.2): the band
/// `Ss..=Se` by run and size, each coefficient shifted up by `Al`, or an
/// end-of-band run that skips this unit and the next ones.
fn ac_first(ac: &Huff, b: &mut Bits<'_>, s: &Scan, eobrun: &mut u32, blk: &mut [i16; 64]) {
    if *eobrun > 0 {
        *eobrun -= 1;
        return;
    }
    let mut k = s.ss as usize;
    while k <= s.se as usize {
        let rs = ac.decode(b);
        let (r, sz) = (u32::from(rs >> 4), u32::from(rs & 15));
        if sz != 0 {
            k += r as usize;
            let v = extend(b.get(sz), sz);
            blk[usize::from(ZIGZAG[k.min(79)])] = v.wrapping_shl(s.al) as i16;
        } else if r == 15 {
            k += 15;
        } else {
            *eobrun = (1 << r) + b.get(r) as u32;
            *eobrun -= 1;
            break;
        }
        k += 1;
    }
}

/// **A progressive AC refinement scan's data unit** (G.1.2.3): a
/// correction bit for every coefficient already non-zero in the band, and
/// each newly non-zero one, one bit of magnitude `Al`, put after the run of
/// zero ones it names; in an end-of-band run, the correction bits alone.
/// The IJG's `decode_mcu_AC_refine()`, in its order; its test that a
/// coefficient's bit is not already set guards a decoder that suspends
/// mid-unit and is not needed here.
fn ac_refine(ac: &Huff, b: &mut Bits<'_>, s: &Scan, eobrun: &mut u32, blk: &mut [i16; 64]) {
    let p1: i16 = (1i32 << s.al) as i16;
    let m1: i16 = (-1i32).wrapping_shl(s.al) as i16;
    let se = s.se as usize;
    let mut k = s.ss as usize;
    let correct = |b: &mut Bits<'_>, c: &mut i16| {
        if b.get(1) != 0 {
            *c = match *c >= 0 {
                true => c.wrapping_add(p1),
                false => c.wrapping_add(m1),
            };
        }
    };
    if *eobrun == 0 {
        while k <= se {
            let rs = ac.decode(b);
            let (mut r, sz) = (i32::from(rs >> 4), rs & 15);
            let mut new = 0i16;
            if sz != 0 {
                new = match b.get(1) {
                    0 => m1,
                    _ => p1,
                };
            } else if r != 15 {
                *eobrun = (1u32 << r) + b.get(r as u32) as u32;
                break;
            }
            while k <= se {
                let at = usize::from(ZIGZAG[k]);
                if blk[at] != 0 {
                    correct(b, &mut blk[at]);
                } else {
                    r -= 1;
                    if r < 0 {
                        break;
                    }
                }
                k += 1;
            }
            if new != 0 {
                blk[usize::from(ZIGZAG[k.min(79)])] = new;
            }
            k += 1;
        }
    }
    if *eobrun > 0 {
        while k <= se {
            let at = usize::from(ZIGZAG[k]);
            if blk[at] != 0 {
                correct(b, &mut blk[at]);
            }
            k += 1;
        }
        *eobrun -= 1;
    }
}

/// The IJG's accurate integer inverse DCT's constants, `FIX(x)` at
/// `CONST_BITS` 13: each the nearest integer to `x * 8192`.
const FIX_0_298631336: i64 = 2446;
const FIX_0_390180644: i64 = 3196;
const FIX_0_541196100: i64 = 4433;
const FIX_0_765366865: i64 = 6270;
const FIX_0_899976223: i64 = 7373;
const FIX_1_175875602: i64 = 9633;
const FIX_1_501321110: i64 = 12299;
const FIX_1_847759065: i64 = 15137;
const FIX_1_961570560: i64 = 16069;
const FIX_2_053119869: i64 = 16819;
const FIX_2_562915447: i64 = 20995;
const FIX_3_072711026: i64 = 25172;
const CONST_BITS: u32 = 13;
const PASS1_BITS: u32 = 2;

/// `DESCALE(x, n)`: `x / 2^n` rounded half up.
fn descale(x: i64, n: u32) -> i64 {
    (x + (1 << (n - 1))) >> n
}

/// **One eight-point inverse DCT**, the IJG's `jpeg_idct_islow()` on one
/// column or row: the even part's rotation by `sqrt(2) * c6`, the odd part
/// by its Figure 8, the eight outputs before their descale. Wide integers,
/// so a coefficient no valid file carries cannot overflow.
fn idct8(i: [i64; 8]) -> [i64; 8] {
    let (z2, z3) = (i[2], i[6]);
    let z1 = (z2 + z3) * FIX_0_541196100;
    let tmp2 = z1 + z3 * -FIX_1_847759065;
    let tmp3 = z1 + z2 * FIX_0_765366865;
    let tmp0 = (i[0] + i[4]) << CONST_BITS;
    let tmp1 = (i[0] - i[4]) << CONST_BITS;
    let (tmp10, tmp13) = (tmp0 + tmp3, tmp0 - tmp3);
    let (tmp11, tmp12) = (tmp1 + tmp2, tmp1 - tmp2);
    let (t0, t1, t2, t3) = (i[7], i[5], i[3], i[1]);
    let (z1, z2, z3, z4) = (t0 + t3, t1 + t2, t0 + t2, t1 + t3);
    let z5 = (z3 + z4) * FIX_1_175875602;
    let (t0, t1, t2, t3) = (
        t0 * FIX_0_298631336,
        t1 * FIX_2_053119869,
        t2 * FIX_3_072711026,
        t3 * FIX_1_501321110,
    );
    let z1 = z1 * -FIX_0_899976223;
    let z2 = z2 * -FIX_2_562915447;
    let z3 = z3 * -FIX_1_961570560 + z5;
    let z4 = z4 * -FIX_0_390180644 + z5;
    let (t0, t1, t2, t3) = (t0 + z1 + z3, t1 + z2 + z4, t2 + z2 + z3, t3 + z1 + z4);
    [
        tmp10 + t3,
        tmp11 + t2,
        tmp12 + t1,
        tmp13 + t0,
        tmp13 - t0,
        tmp12 - t1,
        tmp11 - t2,
        tmp10 - t3,
    ]
}

/// **A block's coefficients, dequantized, into its 64 samples** at `(x0,
/// y0)` of a plane `stride` bytes wide: columns, then rows, each output
/// level-shifted by 128 and limited to 0..=255. The IJG wraps a sample far
/// outside that range through its table's mask, which only a coefficient
/// no encoder makes can reach; here it is limited.
fn idct_into(
    blk: &[i16; 64],
    q: &[u16; 64],
    plane: &mut [u8],
    stride: usize,
    x0: usize,
    y0: usize,
) {
    let mut ws = [0i64; 64];
    for c in 0..8 {
        let col = |r: usize| i64::from(blk[r * 8 + c]) * i64::from(q[r * 8 + c]);
        let out = match (1..8).all(|r| blk[r * 8 + c] == 0) {
            true => [col(0) << PASS1_BITS; 8],
            false => idct8(core::array::from_fn(col)).map(|v| descale(v, CONST_BITS - PASS1_BITS)),
        };
        for (r, v) in out.into_iter().enumerate() {
            ws[r * 8 + c] = v;
        }
    }
    for r in 0..8 {
        let row: [i64; 8] = core::array::from_fn(|c| ws[r * 8 + c]);
        let out = idct8(row).map(|v| descale(v, CONST_BITS + PASS1_BITS + 3));
        let at = (y0 + r) * stride + x0;
        if let Some(dst) = plane.get_mut(at..at + 8) {
            for (d, v) in dst.iter_mut().zip(out) {
                *d = (v + 128).clamp(0, 255) as u8;
            }
        }
    }
}

/// **Row `y` of component `c` at the picture's width**, into `row`: the
/// IJG's `fullsize_upsample()`, `h2v1_fancy_upsample()`, `h2v2_fancy_upsample()`
/// or the plain repetition of `int_upsample()`, as `jdsample.c` picks them.
/// The triangle filter takes three quarters of the nearer sample and a
/// quarter of the further across, and the same down, an edge sample
/// standing for the one past it, as the IJG's edge cases and its context
/// rows do.
fn upsample(f: &Frame, c: usize, plane: &[u8], y: usize, row: &mut [u8]) {
    let k = f.comps[c];
    let (rh, rv) = (usize::from(f.hmax / k.h), usize::from(f.vmax / k.v));
    let (bw, _) = f.blocks(c);
    let stride = bw * 8;
    let (dw, dh) = f.samples(c);
    let at = |sx: usize, sy: usize| usize::from(plane.get(sy * stride + sx).copied().unwrap_or(0));
    let fancy = dw > 2;
    match (rh, rv) {
        (1, 1) => {
            for (x, o) in row.iter_mut().enumerate() {
                *o = at(x, y) as u8;
            }
        }
        (2, 1) if fancy => {
            for (x, o) in row.iter_mut().enumerate() {
                let i = x / 2;
                let near = at(i, y) * 3;
                *o = match x % 2 {
                    0 => (near + at(i.saturating_sub(1), y) + 1) >> 2,
                    _ => (near + at((i + 1).min(dw - 1), y) + 2) >> 2,
                } as u8;
            }
        }
        (2, 2) if fancy => {
            let sy = y / 2;
            let far = match y % 2 {
                0 => sy.saturating_sub(1),
                _ => (sy + 1).min(dh - 1),
            };
            let sum = |i: usize| at(i, sy) * 3 + at(i, far);
            for (x, o) in row.iter_mut().enumerate() {
                let i = x / 2;
                let this = sum(i) * 3;
                *o = match x % 2 {
                    0 => (this + sum(i.saturating_sub(1)) + 8) >> 4,
                    _ => (this + sum((i + 1).min(dw - 1)) + 7) >> 4,
                } as u8;
            }
        }
        _ => {
            for (x, o) in row.iter_mut().enumerate() {
                *o = at(x / rh, y / rv) as u8;
            }
        }
    }
}

/// `x * 2^16`, rounded: the IJG's `FIX` at `SCALEBITS` 16.
const ONE_HALF: i32 = 1 << 15;
const FIX_1_40200: i32 = 91881;
const FIX_1_77200: i32 = 116130;
const FIX_0_71414: i32 = 46802;
const FIX_0_34414: i32 = 22554;

/// **JFIF's YCbCr to RGB**, "R = Y + 1.402 (Cr-128), G = Y - 0.34414
/// (Cb-128) - 0.71414 (Cr-128), B = Y + 1.772 (Cb-128)", in the IJG's
/// fixed point (`build_ycc_rgb_table()`): red's and blue's terms rounded
/// alone, green's two summed and rounded together, each channel limited.
const YCC: [[i32; 256]; 4] = {
    let mut t = [[0i32; 256]; 4];
    let mut i = 0;
    while i < 256 {
        let x = i as i32 - 128;
        t[0][i] = (FIX_1_40200 * x + ONE_HALF) >> 16;
        t[1][i] = (FIX_1_77200 * x + ONE_HALF) >> 16;
        t[2][i] = -FIX_0_71414 * x;
        t[3][i] = -FIX_0_34414 * x + ONE_HALF;
        i += 1;
    }
    t
};

fn ycc_rgb(y: u8, cb: u8, cr: u8) -> (u8, u8, u8) {
    let (y, cb, cr) = (i32::from(y), usize::from(cb), usize::from(cr));
    let lim = |v: i32| v.clamp(0, 255) as u8;
    (
        lim(y + YCC[0][cr]),
        lim(y + ((YCC[3][cb] + YCC[2][cr]) >> 16)),
        lim(y + YCC[1][cb]),
    )
}

/// **Adobe's inverted CMYK as RGB**: each of C, M and Y as stored is the
/// colour's own channel, and K's as stored its brightness, so the product
/// over 255, rounded, is the colour shown. No colour profile is applied.
fn cmyk(c: u8, m: u8, y: u8, k: u8) -> Pixel {
    let k = u32::from(k);
    let mul = |v: u8| ((u32::from(v) * k + 127) / 255) as u8;
    Pixel::opaque_bytes(mul(c), mul(m), mul(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ycc_tables_are_the_ijgs_nearest_integers() {
        // Cr = 255: 1.402 * 127 = 178.054; Cb = 0: 1.772 * -128 = -226.816.
        assert_eq!(YCC[0][255], 178);
        assert_eq!(YCC[1][0], -227);
        assert_eq!(ycc_rgb(128, 128, 128), (128, 128, 128));
    }

    #[test]
    fn a_huffman_table_of_all_ones_is_refused() {
        // Two codes of length 1 take 0 and 1, and 1 is all ones.
        let mut bits = [0u8; 16];
        bits[0] = 2;
        assert!(Huff::build(&bits, &[1, 2]).is_none());
        bits[0] = 1;
        assert!(Huff::build(&bits, &[1]).is_some());
    }

    #[test]
    fn every_orientation_places_a_corner_where_exif_says() {
        // The coded top-left of a 4 by 2 image.
        let want = [
            (Orientation::TopLeft, (0, 0)),
            (Orientation::TopRight, (3, 0)),
            (Orientation::BottomRight, (3, 1)),
            (Orientation::BottomLeft, (0, 1)),
            (Orientation::LeftTop, (0, 0)),
            (Orientation::RightTop, (1, 0)),
            (Orientation::RightBottom, (1, 3)),
            (Orientation::LeftBottom, (0, 3)),
        ];
        for (o, at) in want {
            assert_eq!(o.place(0, 0, 4, 2), at, "{o:?}");
        }
    }
}
