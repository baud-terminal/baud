//! **JPEG, decoded to pixels** (`[[source: itu-t81]]`): every process
//! T.81 defines but the hierarchical one. The DCT-based processes --
//! baseline (`SOF0`), extended sequential (`SOF1`, `SOF9`) and progressive
//! (`SOF2`, `SOF10`), spectral selection and successive approximation both
//! (Annex G) -- at 8 and 12 bits a sample; the lossless process (`SOF3`,
//! `SOF11`, Annex H) at 2 to 16, by each of its seven predictors and its
//! point transform; each with Huffman coding or the adaptive binary
//! arithmetic coder (Annex D, F.2.4, G.1.3, H.1.2.3). In a JFIF file
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
//! factorisation in 13-bit constants), which at 12 bits keeps one bit
//! fewer after its first pass "to avoid overflow", a component subsampled
//! two to one is interpolated by its triangle filter (`jdsample.c`), which
//! puts each subsampled sample where JFIF sites it, centred between the
//! samples it covers ("Spatial Relationship of Components"), and YCbCr is
//! turned to RGB by JFIF's equations in its 16-bit fixed point
//! (`jdcolor.c`), centred at half the range at every precision. The
//! arithmetic is the IJG's, its notice in `LICENSES/ijg-README.txt`
//! (`[[source: ijg-jpeg-6b]]`); `testorig.jpg` (baseline, 4:2:0) and
//! `testprog.jpg` (its progressive equivalent) decode here to the bytes of
//! `testimg.ppm`, as its makefile's `test` target requires of `djpeg -dct
//! int`. macOS's decoder is not the IJG's, so a picture here and in iTerm2
//! may differ by the rounding of either. The arithmetic decoder is T.81's
//! own procedures, Figures D.16 to D.22, with Table D.3's estimates, which
//! decode K.4.1's test sequence to its bits; libjpeg-turbo's
//! `testimgari.jpg` decodes to the bytes of its Huffman twin
//! (`[[source: libjpeg-turbo-314]]`).
//!
//! **A sample is drawn at 8 bits, as a PNG's is** (`crate::png`): one of
//! more than 8 bits by its high eight, as a 16-bit PNG's, once its colour
//! is turned at its own precision; a lossless one of fewer scaled to the
//! full range, as a sub-byte PNG grey is.
//!
//! **What is refused, and counted where the caller counts.** The
//! hierarchical process -- its differential frames (`SOF5`-`SOF7`,
//! `SOF13`-`SOF15`) and the `DHP` and `EXP` only it carries -- a height left
//! to `DNL`, two components, sampling factors whose ratio is not whole, a
//! precision Table B.2 does not give the process, and a lossless restart
//! interval that is not whole MCU rows, which H.1.1 requires: each
//! [`JpegError::Unsupported`], [`JpegError::Frame`] or [`JpegError::Scan`],
//! drawn by the caller as a picture that does not decode.
//!
//! **Malformed input never panics**, and the work is bounded by the file
//! and the frame. Every length is checked against the file, every
//! arithmetic on a coefficient wraps as the IJG's `int` would, a code no
//! table holds decodes as zero, a scan cut short reads zero bits, and the
//! scans read are at most what a valid file can carry: in a sequential
//! or lossless file every component is in exactly one scan (B.2.3), and in
//! a progressive one each coefficient's bits are each coded once (G.1.1.1),
//! so a component takes at most fourteen scans for its DC and fourteen
//! for each of its 63 AC coefficients -- one first and thirteen
//! refinements, `Al` being at most 13. An arithmetic-coded magnitude
//! reaching 2^15, past the 16 bits F.1.4.3.1.2 gives a difference, or a
//! run of zero coefficients past the band's end, stops that scan's
//! decoding to its next restart interval, as the IJG's `jdarith.c` stops
//! (`[[source: libjpeg-turbo-314]]`).

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
    /// overflow their lengths, an identifier past 3, or an arithmetic
    /// conditioning value outside its range.
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

/// The process a frame is coded by (Table B.1): one pass of DCT blocks,
/// many, or the lossless process's predicted samples.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Process {
    Sequential,
    Progressive,
    Lossless,
}

/// How a frame's scans are entropy coded: by Huffman tables (Annex C,
/// F.2.2), or by the adaptive binary arithmetic coder (Annex D).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Coding {
    Huffman,
    Arithmetic,
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
    /// P, the bits of a sample.
    precision: u8,
    n: usize,
    comps: [Comp; 4],
    hmax: u8,
    vmax: u8,
    process: Process,
    coding: Coding,
}

impl Frame {
    /// The side of a data unit: an 8 by 8 block, or in the lossless
    /// process "one sample" (H.1.1).
    fn side(&self) -> usize {
        match self.process {
            Process::Lossless => 1,
            _ => 8,
        }
    }

    /// The bytes a plane keeps a sample in: one up to 8 bits, two past.
    fn bytes(&self) -> usize {
        match self.precision > 8 {
            true => 2,
            false => 1,
        }
    }

    /// MCUs across and down: "the number of MCUs ... is the number of
    /// data units ... rounded up" (A.2.4).
    fn mcus(&self) -> (usize, usize) {
        let (w, h) = (usize::from(self.width), usize::from(self.height));
        let u = self.side();
        (
            w.div_ceil(u * usize::from(self.hmax)),
            h.div_ceil(u * usize::from(self.vmax)),
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

    /// The MCUs across and down scan `s` codes: one data unit each over
    /// the component's own samples when it holds one component, the
    /// frame's MCUs when it interleaves more (A.2.2, A.2.3).
    fn units(&self, s: &Scan) -> (usize, usize) {
        match s.n {
            1 => {
                let (sw, sh) = self.samples(s.comps[0].c);
                let u = self.side();
                (sw.div_ceil(u), sh.div_ceil(u))
            }
            _ => self.mcus(),
        }
    }
}

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const DAC: u8 = 0xCC;
/// `DHP` and `EXP`, which the hierarchical process alone carries (B.3).
const DHP: u8 = 0xDE;
const EXP: u8 = 0xDF;

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
                0xC0..=0xC3 | 0xC9..=0xCB => {
                    if frame.is_some() {
                        return Err(JpegError::Malformed);
                    }
                    frame = Some(read_frame(m, body)?);
                }
                0xC5..=0xC7 | 0xCD..=0xCF | DHP | EXP => {
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
    /// samples in whole MCUs, a byte each to 8 bits and two past; a
    /// progressive frame's coefficients, two bytes each, as many again; an
    /// arithmetic-coded lossless frame's conditioning, a byte a sample for
    /// one row more than an MCU covers; and a row of each component at the
    /// picture's width. `None` past what a `usize` holds.
    pub fn scratch_len(&self) -> Option<usize> {
        Some(self.layout()?.total)
    }

    fn layout(&self) -> Option<Layout> {
        let f = &self.frame;
        let (u, bytes) = (f.side(), f.bytes());
        let mut lay = Layout {
            plane: [0; 4],
            coef: [0; 4],
            ctx: [0; 4],
            rows: 0,
            total: 0,
        };
        let mut at = 0usize;
        for (c, plane) in lay.plane.iter_mut().enumerate().take(f.n) {
            let (bw, bh) = f.blocks(c);
            *plane = at;
            at = at.checked_add(bw.checked_mul(bh)?.checked_mul(u * u * bytes)?)?;
        }
        for (c, coef) in lay.coef.iter_mut().enumerate().take(f.n) {
            let (bw, bh) = f.blocks(c);
            *coef = at;
            if f.process == Process::Progressive {
                at = at.checked_add(bw.checked_mul(bh)?.checked_mul(128)?)?;
            }
        }
        for c in 0..f.n {
            let (bw, _) = f.blocks(c);
            lay.ctx[c] = at;
            if f.process == Process::Lossless && f.coding == Coding::Arithmetic {
                let rows = usize::from(f.comps[c].v) + 1;
                at = at.checked_add(bw.checked_mul(rows)?)?;
            }
        }
        lay.rows = at;
        at = at.checked_add(usize::from(f.width).checked_mul(f.n)?.checked_mul(bytes)?)?;
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
        // The planes mid-grey, what a data unit no scan reaches decodes
        // to, and the coefficients and the conditioning zero.
        let mid = (1u16 << (self.frame.precision - 1)).to_le_bytes();
        match self.frame.bytes() {
            1 => work[..lay.coef[0]].fill(mid[0]),
            _ => {
                for s in work[..lay.coef[0]].as_chunks_mut::<2>().0 {
                    s.copy_from_slice(&mid);
                }
            }
        }
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
    /// the colour at the frame's precision, each channel at 8 bits, each
    /// pixel put where the orientation shows it.
    fn output(&self, lay: &Layout, work: &mut [u8], out: &mut [Pixel]) {
        match self.frame.bytes() {
            1 => self.output_at::<1>(lay, work, out),
            _ => self.output_at::<2>(lay, work, out),
        }
    }

    fn output_at<const W: usize>(&self, lay: &Layout, work: &mut [u8], out: &mut [Pixel]) {
        let f = &self.frame;
        let (w, h) = (usize::from(f.width), usize::from(f.height));
        let shown_w = match self.orientation.turns() {
            true => h,
            false => w,
        };
        let p = f.precision;
        let max = u16::MAX >> (16 - p);
        let eight = |v: u16| narrow(v, p);
        let (planes, rows) = work.split_at_mut(lay.rows);
        for y in 0..h {
            for c in 0..f.n {
                let row = &mut rows[c * w * W..(c + 1) * w * W];
                upsample::<W>(f, c, &planes[lay.plane[c]..], y, row);
            }
            for x in 0..w {
                let s = |c: usize| get::<W>(rows, c * w + x);
                let px = match self.colour {
                    Colour::Grey => {
                        let v = eight(s(0));
                        Pixel::opaque_bytes(v, v, v)
                    }
                    Colour::YCbCr => {
                        let (r, g, b) = ycc(s(0), s(1), s(2), p);
                        Pixel::opaque_bytes(eight(r), eight(g), eight(b))
                    }
                    Colour::Rgb => Pixel::opaque_bytes(eight(s(0)), eight(s(1)), eight(s(2))),
                    Colour::Cmyk => cmyk(eight(s(0)), eight(s(1)), eight(s(2)), eight(s(3))),
                    Colour::Ycck => {
                        let (r, g, b) = ycc(s(0), s(1), s(2), p);
                        cmyk(eight(max - r), eight(max - g), eight(max - b), eight(s(3)))
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
    ctx: [usize; 4],
    rows: usize,
    total: usize,
}

/// **Sample `i` of a plane or row that keeps `W` bytes a sample**, little
/// endian; zero past its end.
fn get<const W: usize>(p: &[u8], i: usize) -> u16 {
    match W {
        1 => p.get(i).map_or(0, |&v| u16::from(v)),
        _ => p
            .get(2 * i..2 * i + 2)
            .map_or(0, |b| u16::from_le_bytes([b[0], b[1]])),
    }
}

/// Sample `i` of a plane or row of `W` bytes a sample set to `v`; nothing
/// past its end.
fn put<const W: usize>(p: &mut [u8], i: usize, v: u16) {
    match W {
        1 => {
            if let Some(d) = p.get_mut(i) {
                *d = v.to_le_bytes()[0];
            }
        }
        _ => {
            if let Some(d) = p.get_mut(2 * i..2 * i + 2) {
                d.copy_from_slice(&v.to_le_bytes());
            }
        }
    }
}

/// **A `p`-bit sample at 8 bits**, as a PNG's is drawn (`crate::png`):
/// above 8 bits its high eight, as a 16-bit PNG's; below, scaled to the
/// full range, as a sub-byte PNG grey is.
fn narrow(v: u16, p: u8) -> u8 {
    let v = u32::from(v);
    let n = match p {
        8 => v,
        p if p > 8 => v >> (p - 8),
        p => v * 255 / ((1 << p) - 1),
    };
    u8::try_from(n).unwrap_or(u8::MAX)
}

/// **The frame header read** (B.2.2): the precision Table B.2 gives the
/// process -- 8 bits in the baseline process, 8 or 12 in the extended and
/// progressive ones, 2 to 16 in the lossless one -- a height given, one,
/// three or four components, each sampling factor 1 to 4 and a whole
/// fraction of the largest, each table 0 to 3.
fn read_frame(m: u8, b: &[u8]) -> Result<Frame, JpegError> {
    let (process, coding) = match m {
        0xC2 => (Process::Progressive, Coding::Huffman),
        0xC3 => (Process::Lossless, Coding::Huffman),
        0xC9 => (Process::Sequential, Coding::Arithmetic),
        0xCA => (Process::Progressive, Coding::Arithmetic),
        0xCB => (Process::Lossless, Coding::Arithmetic),
        _ => (Process::Sequential, Coding::Huffman),
    };
    if b.len() < 6 {
        return Err(JpegError::Malformed);
    }
    let precision = b[0];
    let allowed = match (m, process) {
        (0xC0, _) => precision == 8,
        (_, Process::Lossless) => (2..=16).contains(&precision),
        _ => matches!(precision, 8 | 12),
    };
    if !allowed {
        return Err(JpegError::Frame);
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
        precision,
        n,
        comps,
        hmax,
        vmax,
        process,
        coding,
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

/// **An entropy-coded segment's bytes** (B.1.1.5): a stuffed `X'FF00'`
/// read as the data byte `X'FF'`; at a marker, or the file's end, zero
/// bytes for ever, as the IJG's reader feeds a decoder that has run out of
/// data and as D.2.6 feeds the arithmetic decoder "0-bytes ... until
/// decoding is complete".
struct Source<'a> {
    d: &'a [u8],
    pos: usize,
    /// The marker reached, and where its `X'FF'` is.
    hit: Option<(u8, usize)>,
}

impl<'a> Source<'a> {
    fn new(d: &'a [u8], pos: usize) -> Source<'a> {
        Source { d, pos, hit: None }
    }

    /// The next data byte, or zero at a marker or the end.
    fn byte(&mut self) -> u8 {
        if self.hit.is_some() {
            return 0;
        }
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

    /// **At a restart interval's end** (F.2.2.5, F.2.4.4): the `RSTm` that
    /// should follow taken, past any bytes before it; another marker is
    /// left for the scan's end to find.
    fn restart(&mut self) {
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

/// **The bits of an entropy-coded segment** (F.2.2.5), most significant
/// first, from its bytes.
struct Bits<'a> {
    src: Source<'a>,
    acc: u64,
    n: u32,
}

impl<'a> Bits<'a> {
    fn new(d: &'a [u8], pos: usize) -> Bits<'a> {
        Bits {
            src: Source::new(d, pos),
            acc: 0,
            n: 0,
        }
    }

    fn fill(&mut self) {
        while self.n <= 56 {
            let byte = self.src.byte();
            self.acc |= u64::from(byte) << (56 - self.n);
            self.n += 8;
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

    /// At a restart interval's end: the bits left in the byte dropped and
    /// the `RSTm` taken.
    fn restart(&mut self) {
        self.acc = 0;
        self.n = 0;
        self.src.restart();
    }

    fn end(&self) -> usize {
        self.src.end()
    }
}

/// **Table D.3, "Qe values and probability estimation state machine"**:
/// each index's `Qe_Value`, `Next_Index_LPS`, `Next_Index_MPS` and
/// `Switch_MPS`, 0 to 112, as T.81 gives them and as the IJG's
/// `jaricom.c` has them, entry for entry (`[[source: libjpeg-turbo-314]]`);
/// and at 113 the fixed estimate.
const QE: [(u16, u8, u8, u8); 114] = [
    (0x5A1D, 1, 1, 1),
    (0x2586, 14, 2, 0),
    (0x1114, 16, 3, 0),
    (0x080B, 18, 4, 0),
    (0x03D8, 20, 5, 0),
    (0x01DA, 23, 6, 0),
    (0x00E5, 25, 7, 0),
    (0x006F, 28, 8, 0),
    (0x0036, 30, 9, 0),
    (0x001A, 33, 10, 0),
    (0x000D, 35, 11, 0),
    (0x0006, 9, 12, 0),
    (0x0003, 10, 13, 0),
    (0x0001, 12, 13, 0),
    (0x5A7F, 15, 15, 1),
    (0x3F25, 36, 16, 0),
    (0x2CF2, 38, 17, 0),
    (0x207C, 39, 18, 0),
    (0x17B9, 40, 19, 0),
    (0x1182, 42, 20, 0),
    (0x0CEF, 43, 21, 0),
    (0x09A1, 45, 22, 0),
    (0x072F, 46, 23, 0),
    (0x055C, 48, 24, 0),
    (0x0406, 49, 25, 0),
    (0x0303, 51, 26, 0),
    (0x0240, 52, 27, 0),
    (0x01B1, 54, 28, 0),
    (0x0144, 56, 29, 0),
    (0x00F5, 57, 30, 0),
    (0x00B7, 59, 31, 0),
    (0x008A, 60, 32, 0),
    (0x0068, 62, 33, 0),
    (0x004E, 63, 34, 0),
    (0x003B, 32, 35, 0),
    (0x002C, 33, 9, 0),
    (0x5AE1, 37, 37, 1),
    (0x484C, 64, 38, 0),
    (0x3A0D, 65, 39, 0),
    (0x2EF1, 67, 40, 0),
    (0x261F, 68, 41, 0),
    (0x1F33, 69, 42, 0),
    (0x19A8, 70, 43, 0),
    (0x1518, 72, 44, 0),
    (0x1177, 73, 45, 0),
    (0x0E74, 74, 46, 0),
    (0x0BFB, 75, 47, 0),
    (0x09F8, 77, 48, 0),
    (0x0861, 78, 49, 0),
    (0x0706, 79, 50, 0),
    (0x05CD, 48, 51, 0),
    (0x04DE, 50, 52, 0),
    (0x040F, 50, 53, 0),
    (0x0363, 51, 54, 0),
    (0x02D4, 52, 55, 0),
    (0x025C, 53, 56, 0),
    (0x01F8, 54, 57, 0),
    (0x01A4, 55, 58, 0),
    (0x0160, 56, 59, 0),
    (0x0125, 57, 60, 0),
    (0x00F6, 58, 61, 0),
    (0x00CB, 59, 62, 0),
    (0x00AB, 61, 63, 0),
    (0x008F, 61, 32, 0),
    (0x5B12, 65, 65, 1),
    (0x4D04, 80, 66, 0),
    (0x412C, 81, 67, 0),
    (0x37D8, 82, 68, 0),
    (0x2FE8, 83, 69, 0),
    (0x293C, 84, 70, 0),
    (0x2379, 86, 71, 0),
    (0x1EDF, 87, 72, 0),
    (0x1AA9, 87, 73, 0),
    (0x174E, 72, 74, 0),
    (0x1424, 72, 75, 0),
    (0x119C, 74, 76, 0),
    (0x0F6B, 74, 77, 0),
    (0x0D51, 75, 78, 0),
    (0x0BB6, 77, 79, 0),
    (0x0A40, 77, 48, 0),
    (0x5832, 80, 81, 1),
    (0x4D1C, 88, 82, 0),
    (0x438E, 89, 83, 0),
    (0x3BDD, 90, 84, 0),
    (0x34EE, 91, 85, 0),
    (0x2EAE, 92, 86, 0),
    (0x299A, 93, 87, 0),
    (0x2516, 86, 71, 0),
    (0x5570, 88, 89, 1),
    (0x4CA9, 95, 90, 0),
    (0x44D9, 96, 91, 0),
    (0x3E22, 97, 92, 0),
    (0x3824, 99, 93, 0),
    (0x32B4, 99, 94, 0),
    (0x2E17, 93, 86, 0),
    (0x56A8, 95, 96, 1),
    (0x4F46, 101, 97, 0),
    (0x47E5, 102, 98, 0),
    (0x41CF, 103, 99, 0),
    (0x3C3D, 104, 100, 0),
    (0x375E, 99, 93, 0),
    (0x5231, 105, 102, 0),
    (0x4C0F, 106, 103, 0),
    (0x4639, 107, 104, 0),
    (0x415E, 103, 99, 0),
    (0x5627, 105, 106, 1),
    (0x50E7, 108, 107, 0),
    (0x4B85, 109, 103, 0),
    (0x5597, 110, 109, 0),
    (0x504F, 111, 107, 0),
    (0x5A10, 110, 111, 1),
    (0x5522, 112, 109, 0),
    (0x59EB, 112, 111, 1),
    (0x5A1D, 113, 113, 0),
];

/// **The fixed estimate**: "a fixed probability value of approximately 0.5
/// (Qe = X'5A1D', MPS = 0)" (F.1.4.4.2, G.1.3.1), which no decision moves;
/// at 113, past Table D.3's last index, where the IJG's `jaricom.c` keeps
/// it.
const FIXED: u8 = 113;

/// Whether an arithmetic-coded scan is decoding, or has met a magnitude or
/// a run its model cannot hold and stopped to the next restart interval.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Flow {
    Live,
    Stopped,
}

/// **The arithmetic decoder** (D.2): the interval register A, the code
/// register C -- `Cx` its high sixteen bits, `C-low` its low sixteen --
/// and `CT`, the bits of `C-low` not yet shifted into `Cx`. Wider than
/// sixteen bits, as D.2.3's note allows, so A starts at `X'10000'`.
struct Arith<'a> {
    src: Source<'a>,
    a: u32,
    c: u32,
    ct: u32,
    flow: Flow,
}

impl<'a> Arith<'a> {
    fn new(d: &'a [u8], pos: usize) -> Arith<'a> {
        let mut ar = Arith {
            src: Source::new(d, pos),
            a: 0,
            c: 0,
            ct: 0,
            flow: Flow::Live,
        };
        ar.init();
        ar
    }

    /// **Initdec** (Figure D.22): A at `X'10000'`, the first two bytes into
    /// `Cx`, and `CT` zero, so the next renormalization fetches a byte.
    fn init(&mut self) {
        self.a = 0x10000;
        self.c = 0;
        self.byte_in();
        self.c <<= 8;
        self.byte_in();
        self.c <<= 8;
        self.ct = 0;
        self.flow = Flow::Live;
    }

    /// **Byte_in** (Figures D.20 and D.21): the next byte into `C-low`'s
    /// high eight bits.
    fn byte_in(&mut self) {
        self.c += u32::from(self.src.byte()) << 8;
    }

    /// **Renorm_d** (Figure D.19): A and C doubled until A is `X'8000'` or
    /// more, a byte fetched each time `CT` runs out.
    fn renorm(&mut self) {
        loop {
            if self.ct == 0 {
                self.byte_in();
                self.ct = 8;
            }
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.a >= 0x8000 {
                break;
            }
        }
    }

    /// **Decode(S)** (Figure D.16): one binary decision in the context
    /// whose state `s` holds, its Table D.3 index in the low seven bits and
    /// its MPS in the top one; the conditional exchanges of Figures D.17
    /// and D.18, and the estimate moved on renormalization as Figures D.5
    /// and D.6 move it, the MPS inverted after an LPS where `Switch_MPS` is
    /// 1.
    fn decode(&mut self, s: &mut u8) -> usize {
        let (qe, next_lps, next_mps, switch) = QE
            .get(usize::from(*s & 0x7F))
            .copied()
            .unwrap_or(QE[usize::from(FIXED)]);
        let qe = u32::from(qe);
        let mps = *s >> 7;
        let after_mps = next_mps | mps << 7;
        let after_lps = next_lps | (mps ^ switch) << 7;
        self.a -= qe;
        let d = match (self.c >> 16) < self.a {
            true if self.a >= 0x8000 => return usize::from(mps),
            true => {
                let d = match self.a < qe {
                    true => {
                        *s = after_lps;
                        1 - mps
                    }
                    false => {
                        *s = after_mps;
                        mps
                    }
                };
                self.renorm();
                d
            }
            false => {
                self.c -= self.a << 16;
                let d = match self.a < qe {
                    true => {
                        *s = after_mps;
                        mps
                    }
                    false => {
                        *s = after_lps;
                        1 - mps
                    }
                };
                self.a = qe;
                self.renorm();
                d
            }
        };
        usize::from(d)
    }

    /// At a restart interval's end (F.2.4.4): the `RSTm` taken and the
    /// decoder initialized again.
    fn restart(&mut self) {
        self.src.restart();
        self.init();
    }
}

/// **The statistics areas** (F.1.4.4, G.1.3.3.1, H.1.2.3.2), each bin a
/// Table D.3 index and its MPS, all of them index 0 and MPS 0 at the start
/// of a scan and of each restart interval (D.2.7): for each table a DC
/// area's 49 bins or a lossless one's 158, an AC area's 245, and the fixed
/// estimate.
struct Stats {
    dc: [[u8; 158]; 4],
    ac: [[u8; 245]; 4],
    fixed: u8,
}

impl Stats {
    fn new() -> Stats {
        Stats {
            dc: [[0; 158]; 4],
            ac: [[0; 245]; 4],
            fixed: FIXED,
        }
    }
}

/// **A non-zero value's magnitude category and low bits** (Figures F.23
/// and F.24): `m` the bound its decisions have reached and `s` the bin of
/// the next, each 1 doubling `m` and moving a bin on to the first 0; then
/// the bits below the leading one, most significant first, each in the bin
/// fourteen past the one the category ended in. `Sz`, one less than the
/// magnitude; a bound at 2^15, past the 16 bits a difference has
/// (F.1.4.3.1.2), stops the scan.
fn magnitude(ar: &mut Arith<'_>, st: &mut [u8], mut s: usize, mut m: i32) -> Option<i32> {
    while ar.decode(&mut st[s]) != 0 {
        m <<= 1;
        if m == 0x8000 {
            ar.flow = Flow::Stopped;
            return None;
        }
        s += 1;
    }
    let mut v = m;
    let mut bit = m >> 1;
    while bit != 0 {
        if ar.decode(&mut st[s + 14]) != 0 {
            v |= bit;
        }
        bit >>= 1;
    }
    Some(v)
}

/// **A difference's conditioning class** (F.1.4.4.1.2, H.1.2.3.1): zero,
/// small positive, small negative, large positive or large negative, 0 to
/// 4, by the table's bounds: "the lower bound (exclusive) for difference
/// magnitudes classified as 'small' is zero for L = 0, and is 2^(L-1) for
/// L > 0. The upper bound (inclusive) ... is 2^U."
fn class(v: i32, (l, u): (u8, u8)) -> usize {
    let mag = v.unsigned_abs();
    let sign = usize::from(v < 0);
    match mag {
        0 => 0,
        m if l > 0 && m <= 1 << (l - 1) => 0,
        m if m <= 1 << u => 1 + sign,
        _ => 3 + sign,
    }
}

/// **A difference, DC's or a lossless sample's** (F.2.4.1, Figures F.19
/// to F.22; H.1.2.3, Table H.3): `V = 0` decided at `s0`, the sign at
/// `s0 + 1`, `Sz < 1` at `s0 + 2` when positive and `s0 + 3` when
/// negative, the magnitude category on from `x1`; and its class.
fn arith_diff(
    ar: &mut Arith<'_>,
    st: &mut [u8; 158],
    s0: usize,
    x1: usize,
    bounds: (u8, u8),
) -> (i32, usize) {
    if ar.decode(&mut st[s0]) == 0 {
        return (0, 0);
    }
    let sign = ar.decode(&mut st[s0 + 1]);
    let sz = match ar.decode(&mut st[s0 + 2 + sign]) {
        0 => Some(0),
        _ => magnitude(ar, st, x1, 1),
    };
    match sz {
        Some(sz) => {
            let v = match sign {
                0 => sz + 1,
                _ => -(sz + 1),
            };
            (v, class(v, bounds))
        }
        None => (0, 0),
    }
}

/// **A band of AC coefficients** (F.2.4.2, Figure F.20; G.1.3.2): from
/// `ss` to `se`, an end-of-band decision in `SE = 3 × (K - 1)` before the
/// first and after each non-zero one but the last, runs of zero ones in
/// `S0`, each value shifted up by `al`; the magnitude category from
/// `X2 = AC_Context(K)`, 189 at or below `kx` and 217 past it. A run past
/// the band stops the scan.
#[allow(clippy::too_many_arguments)]
fn arith_ac(
    ar: &mut Arith<'_>,
    st: &mut [u8; 245],
    fixed: &mut u8,
    kx: usize,
    ss: usize,
    se: usize,
    al: u32,
    blk: &mut [i16; 64],
) {
    let mut k = ss;
    while k <= se {
        let mut s = 3 * (k - 1);
        if ar.decode(&mut st[s]) != 0 {
            return;
        }
        while ar.decode(&mut st[s + 1]) == 0 {
            s += 3;
            k += 1;
            if k > se {
                ar.flow = Flow::Stopped;
                return;
            }
        }
        // The sign at the fixed estimate; `Sz < 1` and `Sz < 2` both in
        // `S0 + 1`, "a X1 statistics bin is not used" (F.1.4.4.2).
        let sign = ar.decode(fixed);
        let sp = s + 2;
        let sz = match ar.decode(&mut st[sp]) {
            0 => 0,
            _ => match ar.decode(&mut st[sp]) {
                0 => 1,
                _ => {
                    let x2 = match k <= kx {
                        true => 189,
                        false => 217,
                    };
                    let Some(sz) = magnitude(ar, st, x2, 2) else {
                        return;
                    };
                    sz
                }
            },
        };
        let v = match sign {
            0 => sz + 1,
            _ => -(sz + 1),
        };
        blk[usize::from(ZIGZAG[k])] = v.wrapping_shl(al) as i16;
        k += 1;
    }
}

/// **A subsequent successive approximation scan of an AC band** (G.1.3.3,
/// Figures G.10 and G.11, Table G.2): the end-of-band decision only past
/// the band's end in the scans before (`EOBx`), each coefficient already
/// non-zero corrected by a bit in `SC`, each zero one tested in `S0` and,
/// if it becomes non-zero, given magnitude one in bit `al` and a sign at
/// the fixed estimate. The IJG's `decode_mcu_AC_refine()` in `jdarith.c`,
/// in its order.
fn arith_refine(
    ar: &mut Arith<'_>,
    st: &mut [u8; 245],
    fixed: &mut u8,
    s: &Scan,
    blk: &mut [i16; 64],
) {
    let (ss, se) = (s.ss as usize, s.se as usize);
    let p1: i16 = (1i32 << s.al) as i16;
    let m1: i16 = (-1i32).wrapping_shl(s.al) as i16;
    let mut eobx = se;
    while eobx > 0 && blk[usize::from(ZIGZAG[eobx])] == 0 {
        eobx -= 1;
    }
    let mut k = ss - 1;
    while k < se {
        let mut at = 3 * k;
        if k >= eobx && ar.decode(&mut st[at]) != 0 {
            return;
        }
        loop {
            k += 1;
            let c = &mut blk[usize::from(ZIGZAG[k])];
            if *c != 0 {
                if ar.decode(&mut st[at + 2]) != 0 {
                    *c = c.wrapping_add(match *c < 0 {
                        true => m1,
                        false => p1,
                    });
                }
                break;
            }
            if ar.decode(&mut st[at + 1]) != 0 {
                *c = match ar.decode(fixed) {
                    0 => p1,
                    _ => m1,
                };
                break;
            }
            at += 3;
            if k >= se {
                ar.flow = Flow::Stopped;
                return;
            }
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
/// IJG latches it, the arithmetic conditioning, the restart interval, and
/// the scans read.
struct Decoder {
    frame: Frame,
    lay: Layout,
    dc: [Option<Huff>; 4],
    ac: [Option<Huff>; 4],
    quant: [Option<[u16; 64]>; 4],
    latched: [Option<[u16; 64]>; 4],
    /// Each DC or lossless conditioning table's `(L, U)`, "L = 0 and U =
    /// 1" until a `DAC` sets them (F.1.4.4.1.4, H.1.2.3.3).
    bounds: [(u8, u8); 4],
    /// Each AC conditioning table's `Kx`, "The default value of Kx is 5"
    /// (F.1.4.4.2.1).
    kx: [u8; 4],
    restart: usize,
    scans: usize,
}

/// What a scan's walk reaches next: a restart interval's start, at MCU
/// `m`, or the data unit at `(x, y)` of the scan's `k`th component.
#[derive(Clone, Copy)]
enum Step {
    Restart(usize),
    Unit {
        k: usize,
        sc: ScanComp,
        x: usize,
        y: usize,
    },
}

/// **A scan's data units in order** (A.2): MCU by MCU, an interleaved
/// one's components each `Hi` by `Vi` units, a restart before each interval
/// but the first.
fn walk(f: &Frame, restart: usize, s: &Scan, mut each: impl FnMut(Step)) {
    let (units_x, units_y) = f.units(s);
    for m in 0..units_x * units_y {
        if restart > 0 && m > 0 && m % restart == 0 {
            each(Step::Restart(m));
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
                    each(Step::Unit { k, sc: *sc, x, y });
                }
            }
        }
    }
}

/// An arithmetic-coded DCT scan's state: the decoder, the statistics, and
/// each scan component's DC prediction and `DC_Context`, a multiple of 4
/// to 16 (Table F.4).
struct ArithScan<'a> {
    ar: Arith<'a>,
    st: Stats,
    pred: [i32; 4],
    ctx: [usize; 4],
}

/// A lossless scan's entropy decoder, Huffman's bits or the arithmetic
/// decoder.
enum Entropy<'a> {
    Huffman(Bits<'a>),
    Arithmetic(Arith<'a>),
}

/// A block's 64 coefficients from the 128 bytes a progressive frame keeps
/// them in, and back.
fn load(raw: &[u8]) -> [i16; 64] {
    core::array::from_fn(|k| i16::from_le_bytes([raw[2 * k], raw[2 * k + 1]]))
}

fn store(blk: &[i16; 64], raw: &mut [u8]) {
    for (k, c) in blk.iter().enumerate() {
        raw[2 * k..2 * k + 2].copy_from_slice(&c.to_le_bytes());
    }
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
            bounds: [(0, 1); 4],
            kx: [5; 4],
            restart: 0,
            scans: 0,
        }
    }

    /// The most scans a valid file carries: one per component when
    /// sequential or lossless; when progressive, fourteen for each of a
    /// component's 64 coefficients.
    fn max_scans(&self) -> usize {
        match self.frame.process {
            Process::Sequential | Process::Lossless => self.frame.n,
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
                DAC => self.read_dac(body)?,
                DRI => {
                    self.restart = usize::from(be16(body, 0).ok_or(JpegError::Malformed)?);
                }
                0xC0..=0xC3 | 0xC9..=0xCB => {
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

    /// `DAC` (B.2.4.3): `Tc` and `Tb`, then `Cs` -- for a DC or lossless
    /// table "L + 16 × U", with "0 ≤ L ≤ U ≤ 15"; for an AC one Kx, "1 ≤ Kx
    /// ≤ 63".
    fn read_dac(&mut self, mut b: &[u8]) -> Result<(), JpegError> {
        while let [t, cs, rest @ ..] = b {
            let (tc, tb, cs) = (t >> 4, usize::from(t & 15), *cs);
            if tb > 3 {
                return Err(JpegError::Table);
            }
            match tc {
                0 if cs & 15 <= cs >> 4 => self.bounds[tb] = (cs & 15, cs >> 4),
                1 if (1..=63).contains(&cs) => self.kx[tb] = cs,
                _ => return Err(JpegError::Table),
            }
            b = rest;
        }
        match b.is_empty() {
            true => Ok(()),
            false => Err(JpegError::Malformed),
        }
    }

    /// **The scan header** (B.2.3), held to G.1.1.1's limits when
    /// progressive: the components the frame's, in its order, at most four,
    /// and only one in an AC scan; `Ss` 0 with `Se` 0 or `Ss` 1 to `Se` 63;
    /// `Ah` and `Al` at most 13. Held to Table B.3's when lossless: `Ss` a
    /// predictor, 1 to 7, `Se` and `Ah` zero, the point transform `Al`
    /// below the precision; and the restart interval whole MCU rows, as
    /// H.1.1 requires. The Huffman tables a scan uses are defined, and each
    /// component's quantization table latched at its first scan.
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
        let f = self.frame;
        match f.process {
            Process::Progressive => {
                let dc = ss == 0;
                if (dc && se != 0) || (!dc && (se < ss || se > 63 || n != 1)) || ah > 13 || al > 13
                {
                    return Err(JpegError::Scan);
                }
            }
            Process::Lossless => {
                if !(1..=7).contains(&ss) || se != 0 || ah != 0 || al >= u32::from(f.precision) {
                    return Err(JpegError::Scan);
                }
                // "For the lossless processes the restart interval shall
                // be an integer multiple of the number of MCU in an
                // MCU-row" (H.1.1).
                if !self.restart.is_multiple_of(f.units(&scan).0) {
                    return Err(JpegError::Scan);
                }
            }
            Process::Sequential => {}
        }
        if n > 1 {
            let units: usize = comps[..n]
                .iter()
                .map(|s| {
                    let k = f.comps[s.c];
                    usize::from(k.h) * usize::from(k.v)
                })
                .sum();
            // "Sum of all Hi x Vi ... shall be <= 10" (B.2.3).
            if units > 10 {
                return Err(JpegError::Scan);
            }
        }
        for s in &comps[..n] {
            if f.coding == Coding::Huffman {
                let (needs_dc, needs_ac) = match f.process {
                    Process::Sequential => (true, true),
                    Process::Progressive => (ss == 0 && ah == 0, ss > 0),
                    Process::Lossless => (true, false),
                };
                if (needs_dc && self.dc[s.td].is_none()) || (needs_ac && self.ac[s.ta].is_none()) {
                    return Err(JpegError::Scan);
                }
            }
            if f.process != Process::Lossless && self.latched[s.c].is_none() {
                let tq = usize::from(f.comps[s.c].tq);
                self.latched[s.c] = Some(self.quant[tq].ok_or(JpegError::Scan)?);
            }
        }
        Ok(scan)
    }

    /// **One scan's entropy-coded data from `pos`**, by the frame's process
    /// and coding; the position of the marker after it.
    fn decode_scan(&self, s: &Scan, d: &[u8], pos: usize, work: &mut [u8]) -> usize {
        match (self.frame.process, self.frame.coding) {
            (Process::Lossless, _) => match self.frame.bytes() {
                1 => self.lossless_scan::<1>(s, d, pos, work),
                _ => self.lossless_scan::<2>(s, d, pos, work),
            },
            (_, Coding::Huffman) => self.huffman_scan(s, d, pos, work),
            (_, Coding::Arithmetic) => self.arith_scan(s, d, pos, work),
        }
    }

    /// **A Huffman-coded DCT scan**, MCU by MCU, the predictions and the
    /// end-of-band run reset at each restart.
    fn huffman_scan(&self, s: &Scan, d: &[u8], pos: usize, work: &mut [u8]) -> usize {
        let mut bits = Bits::new(d, pos);
        let mut pred = [0i32; 4];
        let mut eobrun: u32 = 0;
        walk(&self.frame, self.restart, s, |step| match step {
            Step::Restart(_) => {
                bits.restart();
                pred = [0; 4];
                eobrun = 0;
            }
            Step::Unit { k, sc, x, y } => {
                self.unit(s, &sc, x, y, &mut bits, &mut pred[k], &mut eobrun, work);
            }
        });
        bits.end()
    }

    /// **An arithmetic-coded DCT scan** (F.2.4, G.1.3), the decoder, its
    /// statistics, the predictions and the DC contexts set afresh at the
    /// scan's start and each restart (F.1.4.4.1.5, F.1.4.4.2.2).
    fn arith_scan(&self, s: &Scan, d: &[u8], pos: usize, work: &mut [u8]) -> usize {
        let mut a = ArithScan {
            ar: Arith::new(d, pos),
            st: Stats::new(),
            pred: [0; 4],
            ctx: [0; 4],
        };
        walk(&self.frame, self.restart, s, |step| match step {
            Step::Restart(_) => {
                a.ar.restart();
                a.st = Stats::new();
                a.pred = [0; 4];
                a.ctx = [0; 4];
            }
            Step::Unit { k, sc, x, y } => self.arith_unit(s, &sc, k, x, y, &mut a, work),
        });
        a.ar.src.end()
    }

    /// **One arithmetic-coded data unit**: sequential, its DC difference and
    /// AC coefficients decoded and its samples put in the plane;
    /// progressive, the scan's part of its coefficients decoded into the
    /// ones kept -- a DC first scan's difference shifted up by `Al`, a DC
    /// refinement's bit at the fixed estimate, an AC band or its
    /// refinement.
    #[allow(clippy::too_many_arguments)]
    fn arith_unit(
        &self,
        s: &Scan,
        sc: &ScanComp,
        k: usize,
        x: usize,
        y: usize,
        a: &mut ArithScan<'_>,
        work: &mut [u8],
    ) {
        if a.ar.flow == Flow::Stopped {
            return;
        }
        let Some(q) = self.latched[sc.c] else {
            return;
        };
        let bounds = self.bounds[sc.td];
        let kx = usize::from(self.kx[sc.ta]);
        let dc = |a: &mut ArithScan<'_>| {
            let (diff, class) = arith_diff(&mut a.ar, &mut a.st.dc[sc.td], a.ctx[k], 20, bounds);
            a.ctx[k] = 4 * class;
            a.pred[k] = a.pred[k].wrapping_add(diff);
            a.pred[k]
        };
        match self.frame.process {
            Process::Progressive => {
                let (bw, _) = self.frame.blocks(sc.c);
                let at = self.lay.coef[sc.c] + (y * bw + x) * 128;
                let Some(raw) = work.get_mut(at..at + 128) else {
                    return;
                };
                let mut blk = load(raw);
                match (s.ss, s.ah) {
                    (0, 0) => blk[0] = dc(a).wrapping_shl(s.al) as i16,
                    (0, _) => {
                        if a.ar.decode(&mut a.st.fixed) != 0 {
                            blk[0] |= (1i32 << s.al) as i16;
                        }
                    }
                    (_, 0) => arith_ac(
                        &mut a.ar,
                        &mut a.st.ac[sc.ta],
                        &mut a.st.fixed,
                        kx,
                        s.ss as usize,
                        s.se as usize,
                        s.al,
                        &mut blk,
                    ),
                    _ => arith_refine(&mut a.ar, &mut a.st.ac[sc.ta], &mut a.st.fixed, s, &mut blk),
                }
                store(&blk, raw);
            }
            _ => {
                let mut blk = [0i16; 64];
                blk[0] = dc(a) as i16;
                let ac = &mut a.st.ac[sc.ta];
                arith_ac(&mut a.ar, ac, &mut a.st.fixed, kx, 1, 63, 0, &mut blk);
                self.put_block(sc.c, &blk, &q, x, y, work);
            }
        }
    }

    /// **A lossless scan** (H.2), sample by sample in MCU order, each
    /// component's first row of the scan and of each restart interval
    /// predicted along the row from 2^(P - Pt - 1) (H.1.2.1).
    fn lossless_scan<const W: usize>(
        &self,
        s: &Scan,
        d: &[u8],
        pos: usize,
        work: &mut [u8],
    ) -> usize {
        let f = &self.frame;
        let mut coder = match f.coding {
            Coding::Huffman => Entropy::Huffman(Bits::new(d, pos)),
            Coding::Arithmetic => Entropy::Arithmetic(Arith::new(d, pos)),
        };
        let mut st = Stats::new();
        let mut top = [0usize; 4];
        let across = f.units(s).0;
        walk(f, self.restart, s, |step| match step {
            Step::Restart(m) => {
                match &mut coder {
                    Entropy::Huffman(bits) => bits.restart(),
                    Entropy::Arithmetic(ar) => ar.restart(),
                }
                st = Stats::new();
                for (k, sc) in s.comps[..s.n].iter().enumerate() {
                    let v = match s.n {
                        1 => 1,
                        _ => usize::from(f.comps[sc.c].v),
                    };
                    top[k] = m / across * v;
                }
            }
            Step::Unit { k, sc, x, y } => {
                self.sample::<W>(s, &sc, x, y, top[k], &mut coder, &mut st, work);
            }
        });
        match &coder {
            Entropy::Huffman(bits) => bits.end(),
            Entropy::Arithmetic(ar) => ar.src.end(),
        }
    }

    /// **One lossless sample** at `(x, y)` of its component (H.1.2.1,
    /// Table H.1): predicted from the reconstructed samples left (`Ra`),
    /// above (`Rb`) and above left (`Rc`) -- by `Ra` on the interval's first
    /// row `top`, from 2^(P - Pt - 1) at its start, by `Rb` at each later
    /// row's start, and by the scan's predictor elsewhere, "with full
    /// integer arithmetic precision, and without clamping", 5 and 6 halving
    /// by an arithmetic shift -- the difference decoded and "added, modulo
    /// 2^16, to the prediction", and the sample kept multiplied by 2^Pt
    /// (H.2.2). An arithmetic-coded difference is conditioned on the
    /// classes of the differences left and above (H.1.2.3.1), zero at a
    /// row's start and on the interval's first row.
    #[allow(clippy::too_many_arguments)]
    fn sample<const W: usize>(
        &self,
        s: &Scan,
        sc: &ScanComp,
        x: usize,
        y: usize,
        top: usize,
        coder: &mut Entropy<'_>,
        st: &mut Stats,
        work: &mut [u8],
    ) {
        let f = &self.frame;
        let (stride, _) = f.blocks(sc.c);
        let (planes, rest) = work.split_at_mut(self.lay.coef[0]);
        let (pt, p) = (s.al, u32::from(f.precision));
        let plane = &planes[self.lay.plane[sc.c]..];
        let r = |x: usize, y: usize| i32::from(get::<W>(plane, y * stride + x) >> pt);
        let px = match (y == top, x) {
            (true, 0) => 1 << (p - pt - 1),
            (true, _) => r(x - 1, y),
            (false, 0) => r(x, y - 1),
            (false, _) => {
                let (ra, rb, rc) = (r(x - 1, y), r(x, y - 1), r(x - 1, y - 1));
                match s.ss {
                    1 => ra,
                    2 => rb,
                    3 => rc,
                    4 => ra + rb - rc,
                    5 => ra + ((rb - rc) >> 1),
                    6 => rb + ((ra - rc) >> 1),
                    _ => (ra + rb) >> 1,
                }
            }
        };
        let diff = match coder {
            Entropy::Huffman(bits) => {
                let Some(dc) = &self.dc[sc.td] else {
                    return;
                };
                // "No extra bits are appended after SSSS = 16" (H.1.2.2).
                match u32::from(dc.decode(bits)) {
                    16 => 32768,
                    t => extend(bits.get(t), t),
                }
            }
            Entropy::Arithmetic(ar) => {
                if ar.flow == Flow::Stopped {
                    return;
                }
                let rows = usize::from(f.comps[sc.c].v) + 1;
                let ctx = &mut rest[self.lay.ctx[sc.c] - self.lay.coef[0]..];
                let at = |x: usize, y: usize| (y % rows) * stride + x;
                let ca = match x {
                    0 => 0,
                    _ => usize::from(ctx.get(at(x - 1, y)).copied().unwrap_or(0)),
                };
                let cb = match y == top {
                    true => 0,
                    false => usize::from(ctx.get(at(x, y - 1)).copied().unwrap_or(0)),
                };
                // L_Context(Da, Db), Figure H.2's row the left difference's
                // class and its column the one above; X1_Context(Db), 100
                // for a zero or small one above and 129 for a large one.
                let x1 = match cb {
                    0..=2 => 100,
                    _ => 129,
                };
                let (diff, class) = arith_diff(
                    ar,
                    &mut st.dc[sc.td],
                    20 * ca + 4 * cb,
                    x1,
                    self.bounds[sc.td],
                );
                if let Some(c) = ctx.get_mut(at(x, y)) {
                    *c = class as u8;
                }
                diff
            }
        };
        let v = (px.wrapping_add(diff) as u32 & 0xFFFF) << pt;
        let max = (1u32 << p) - 1;
        let plane = &mut planes[self.lay.plane[sc.c]..];
        put::<W>(plane, y * stride + x, v.min(max) as u16);
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
        match self.frame.process {
            Process::Progressive => {
                let at = self.lay.coef[sc.c] + index * 128;
                let Some(raw) = work.get_mut(at..at + 128) else {
                    return;
                };
                let mut blk = load(raw);
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
                store(&blk, raw);
            }
            _ => {
                let (Some(dc), Some(ac)) = (&self.dc[sc.td], &self.ac[sc.ta]) else {
                    return;
                };
                let mut blk = [0i16; 64];
                sequential_unit(dc, ac, bits, pred, &mut blk);
                self.put_block(sc.c, &blk, &q, x, y, work);
            }
        }
    }

    /// **A block's coefficients through the inverse DCT** into component
    /// `c`'s plane at block `(x, y)`, at the frame's precision.
    fn put_block(
        &self,
        c: usize,
        blk: &[i16; 64],
        q: &[u16; 64],
        x: usize,
        y: usize,
        planes: &mut [u8],
    ) {
        let f = &self.frame;
        let (bw, _) = f.blocks(c);
        let plane = &mut planes[self.lay.plane[c]..];
        match f.bytes() {
            1 => idct_into::<1>(blk, q, plane, bw * 8, x * 8, y * 8, f.precision),
            _ => idct_into::<2>(blk, q, plane, bw * 8, x * 8, y * 8, f.precision),
        }
    }

    /// **A progressive frame's coefficients, every block, into its
    /// samples**, once the last scan is read.
    fn idct_all(&self, work: &mut [u8]) {
        for c in 0..self.frame.n {
            let (bw, bh) = self.frame.blocks(c);
            let q = self.latched[c].unwrap_or([0; 64]);
            let (planes, coefs) = work.split_at_mut(self.lay.coef[0]);
            let from = self.lay.coef[c] - self.lay.coef[0];
            for i in 0..bw * bh {
                let blk = load(&coefs[from + i * 128..from + i * 128 + 128]);
                self.put_block(c, &blk, &q, i % bw, i / bw, planes);
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

/// The bits the IJG's first pass keeps past the result: two at 8 bits, and
/// at 12 "lose a little precision to avoid overflow", one (`jidctint.c`,
/// `[[source: libjpeg-turbo-314]]`).
fn pass1_bits(p: u8) -> u32 {
    match p {
        8 => 2,
        _ => 1,
    }
}

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
/// y0)` of a plane `stride` samples wide and `W` bytes a sample: columns,
/// then rows, each output level-shifted by 2^(p - 1) and limited to 0 to
/// 2^p - 1. The IJG wraps a sample far outside that range through its
/// table's mask, which only a coefficient no encoder makes can reach; here
/// it is limited.
fn idct_into<const W: usize>(
    blk: &[i16; 64],
    q: &[u16; 64],
    plane: &mut [u8],
    stride: usize,
    x0: usize,
    y0: usize,
    p: u8,
) {
    let pass1 = pass1_bits(p);
    let (centre, max) = (1i64 << (p - 1), (1i64 << p) - 1);
    let mut ws = [0i64; 64];
    for c in 0..8 {
        let col = |r: usize| i64::from(blk[r * 8 + c]) * i64::from(q[r * 8 + c]);
        let out = match (1..8).all(|r| blk[r * 8 + c] == 0) {
            true => [col(0) << pass1; 8],
            false => idct8(core::array::from_fn(col)).map(|v| descale(v, CONST_BITS - pass1)),
        };
        for (r, v) in out.into_iter().enumerate() {
            ws[r * 8 + c] = v;
        }
    }
    for r in 0..8 {
        let row: [i64; 8] = core::array::from_fn(|c| ws[r * 8 + c]);
        let out = idct8(row).map(|v| descale(v, CONST_BITS + pass1 + 3));
        let at = (y0 + r) * stride + x0;
        if let Some(dst) = plane.get_mut(at * W..(at + 8) * W) {
            for (d, v) in dst.as_chunks_mut::<W>().0.iter_mut().zip(out) {
                let sample = (v + centre).clamp(0, max) as u16;
                d.copy_from_slice(&sample.to_le_bytes()[..W]);
            }
        }
    }
}

/// **Row `y` of component `c` at the picture's width**, into `row`, `W`
/// bytes a sample: the IJG's `fullsize_upsample()`,
/// `h2v1_fancy_upsample()`, `h2v2_fancy_upsample()` or the plain
/// repetition of `int_upsample()`, as `jdsample.c` picks them. The
/// triangle filter takes three quarters of the nearer sample and a quarter
/// of the further across, and the same down, an edge sample standing for
/// the one past it, as the IJG's edge cases and its context rows do.
fn upsample<const W: usize>(f: &Frame, c: usize, plane: &[u8], y: usize, row: &mut [u8]) {
    let k = f.comps[c];
    let (rh, rv) = (usize::from(f.hmax / k.h), usize::from(f.vmax / k.v));
    let (bw, _) = f.blocks(c);
    let stride = bw * f.side();
    let (dw, dh) = f.samples(c);
    let at = |sx: usize, sy: usize| usize::from(get::<W>(plane, sy * stride + sx));
    let fancy = dw > 2;
    let width = row.len() / W;
    let mut set = |x: usize, v: usize| put::<W>(row, x, v as u16);
    match (rh, rv) {
        (1, 1) => {
            for x in 0..width {
                set(x, at(x, y));
            }
        }
        (2, 1) if fancy => {
            for x in 0..width {
                let i = x / 2;
                let near = at(i, y) * 3;
                set(
                    x,
                    match x % 2 {
                        0 => (near + at(i.saturating_sub(1), y) + 1) >> 2,
                        _ => (near + at((i + 1).min(dw - 1), y) + 2) >> 2,
                    },
                );
            }
        }
        (2, 2) if fancy => {
            let sy = y / 2;
            let far = match y % 2 {
                0 => sy.saturating_sub(1),
                _ => (sy + 1).min(dh - 1),
            };
            let sum = |i: usize| at(i, sy) * 3 + at(i, far);
            for x in 0..width {
                let i = x / 2;
                let this = sum(i) * 3;
                set(
                    x,
                    match x % 2 {
                        0 => (this + sum(i.saturating_sub(1)) + 8) >> 4,
                        _ => (this + sum((i + 1).min(dw - 1)) + 7) >> 4,
                    },
                );
            }
        }
        _ => {
            for x in 0..width {
                set(x, at(x / rh, y / rv));
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

/// **YCbCr to RGB at `p` bits**: at 8 the tables above; else the IJG's
/// arithmetic as its 12-bit build computes it (`jdcolor.c`,
/// `[[source: libjpeg-turbo-314]]`), centred at 2^(p - 1) and limited to
/// 2^p - 1.
fn ycc(y: u16, cb: u16, cr: u16, p: u8) -> (u16, u16, u16) {
    if p == 8 {
        let b = |v: u16| v.to_le_bytes()[0];
        let (r, g, b) = ycc_rgb(b(y), b(cb), b(cr));
        return (u16::from(r), u16::from(g), u16::from(b));
    }
    ycc_wide(y, cb, cr, p)
}

fn ycc_wide(y: u16, cb: u16, cr: u16, p: u8) -> (u16, u16, u16) {
    let (centre, max) = (1i64 << (p - 1), (1i64 << p) - 1);
    let (y, cb, cr) = (i64::from(y), i64::from(cb) - centre, i64::from(cr) - centre);
    let fix = |f: i32| i64::from(f);
    let lim = |v: i64| v.clamp(0, max) as u16;
    (
        lim(y + ((fix(FIX_1_40200) * cr + fix(ONE_HALF)) >> 16)),
        lim(y + ((fix(-FIX_0_34414) * cb + fix(ONE_HALF) - fix(FIX_0_71414) * cr) >> 16)),
        lim(y + ((fix(FIX_1_77200) * cb + fix(ONE_HALF)) >> 16)),
    )
}

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

    /// At 8 bits the arithmetic the wider precisions use is the tables'.
    #[test]
    fn the_wide_colour_arithmetic_is_the_tables_at_8_bits() {
        for y in (0..=255u16).step_by(15) {
            for cb in 0..=255u16 {
                for cr in (0..=255u16).step_by(3) {
                    let (r, g, b) = ycc_rgb(y as u8, cb as u8, cr as u8);
                    let want = (u16::from(r), u16::from(g), u16::from(b));
                    assert_eq!(ycc_wide(y, cb, cr, 8), want, "({y}, {cb}, {cr})");
                }
            }
        }
    }

    /// **K.4.1's test sequence**: "the actual compressed data sequence for
    /// the encoder", `X'655B5144 ... F6FFD900'`, decodes in one context to
    /// the 256 decisions it codes, `X'00020051 ... 697EE74C'`, which pass
    /// through renormalizations on both paths, both conditional exchanges
    /// and a stuffed `X'FF00'`.
    #[test]
    fn the_k41_test_sequence_decodes_to_its_decisions() {
        let coded = [
            0x65, 0x5B, 0x51, 0x44, 0xF7, 0x96, 0x9D, 0x51, 0x78, 0x55, 0xBF, 0xFF, 0x00, 0xFC,
            0x51, 0x84, 0xC7, 0xCE, 0xF9, 0x39, 0x00, 0x28, 0x7D, 0x46, 0x70, 0x8E, 0xCB, 0xC0,
            0xF6, 0xFF, 0xD9, 0x00,
        ];
        let want = [
            0x00, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA,
            0xAA, 0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0x74, 0xEA, 0xAB, 0xF7,
            0x69, 0x7E, 0xE7, 0x4C,
        ];
        let mut ar = Arith::new(&coded, 0);
        let mut s = 0u8;
        let mut got = [0u8; 32];
        for i in 0..256 {
            if ar.decode(&mut s) != 0 {
                got[i / 8] |= 0x80 >> (i % 8);
            }
        }
        assert_eq!(got, want);
    }

    /// **The fixed estimate never moves**: a thousand decisions at it,
    /// LPS and MPS both, leave it Qe = X'5A1D' and MPS 0.
    #[test]
    fn the_fixed_estimate_never_moves() {
        let data: [u8; 256] = core::array::from_fn(|i| (i as u8).wrapping_mul(97) ^ 0x5A);
        let mut ar = Arith::new(&data, 0);
        let mut s = FIXED;
        let (mut zeros, mut ones) = (0, 0);
        for _ in 0..1000 {
            match ar.decode(&mut s) {
                0 => zeros += 1,
                _ => ones += 1,
            }
            assert_eq!(s, FIXED);
        }
        assert!(zeros > 0 && ones > 0, "both decisions reached");
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
