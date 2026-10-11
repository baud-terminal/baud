//! The Sixel raster decoder: the bytes inside a `DCS ... q ... ST` turned into
//! a paletted bitmap. Pure, `no_std`, and it allocates nothing -- the caller
//! owns the pixels through a [`SixelCanvas`], exactly as the caller owns the
//! grid and the arena.
//!
//! The grammar is `[[source: vt3xx-sixel]]` -- the VT330/VT340 manual chapter
//! `[[source: ctlseqs]]` names and defers to. The DCS *envelope*
//! (`DCS Pa ; Pb ; Ph q`) is the parser's, not this module's; this decodes the
//! data that follows the `q`.
//!
//! Only the parts a decoder implements are here: the data bytes, the colour
//! registers, the repeat introducer, graphics carriage return and new line,
//! and the raster attributes. `docs/research/notes/s-sixel.md` is the design;
//! this is the decode half of its first acceptance gate.

use crate::style::{Chan, ColorWord};

/// A pixel coordinate or extent. A Sixel is bounded by its raster attributes
/// and by the caller's canvas; `u16` covers every image a terminal draws and
/// keeps the type small.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub struct SixelPx(u16);

impl SixelPx {
    /// The origin.
    pub const ZERO: SixelPx = SixelPx(0);

    /// A coordinate or extent of `n` pixels.
    pub fn of(n: u16) -> SixelPx {
        SixelPx(n)
    }

    /// As an index into a caller slice.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The raw pixel count, for a caller sizing an image arena from a report.
    pub fn raw(self) -> u16 {
        self.0
    }

    fn next(self) -> SixelPx {
        SixelPx(self.0.saturating_add(1))
    }

    fn plus_six(self) -> SixelPx {
        SixelPx(self.0.saturating_add(6))
    }

    fn plus_row(self, row: u8) -> SixelPx {
        SixelPx(self.0.saturating_add(u16::from(row)))
    }

    fn largest(self, o: SixelPx) -> SixelPx {
        SixelPx(self.0.max(o.0))
    }
}

/// A caller-provided image the decoder paints into. `DEFAULT` (zero) means a
/// pixel was never painted -- transparent, so a Sixel composited over the
/// screen shows the cells through its gaps -- which the reserved-value
/// discipline for blank cells is the precedent for.
pub struct SixelCanvas<'a> {
    pixels: &'a mut [ColorWord],
    width: SixelPx,
}

impl<'a> SixelCanvas<'a> {
    /// A canvas `width` pixels wide over `pixels`; the height is what fits.
    /// A row of the image is `width` consecutive words.
    pub fn new(pixels: &'a mut [ColorWord], width: SixelPx) -> SixelCanvas<'a> {
        SixelCanvas { pixels, width }
    }

    fn height(&self) -> SixelPx {
        match self.pixels.len().checked_div(self.width.index()) {
            Some(h) => SixelPx::of(u16::try_from(h).unwrap_or(u16::MAX)),
            None => SixelPx::ZERO,
        }
    }

    /// Paint one pixel. Returns whether it landed inside the canvas; a write
    /// past the edge is refused, never wrapped, and the caller counts it.
    fn paint(&mut self, x: SixelPx, y: SixelPx, colour: ColorWord) -> Painted {
        if x >= self.width || y >= self.height() {
            return Painted::Refused;
        }
        let i = y.index() * self.width.index() + x.index();
        match self.pixels.get_mut(i) {
            Some(slot) => {
                *slot = colour;
                Painted::Inside
            }
            None => Painted::Refused,
        }
    }
}

/// Whether a paint landed. A two-state answer rather than a `bool` so the call
/// site reads as what it is.
enum Painted {
    Inside,
    Refused,
}

/// What a decode produced. The used extent bounds what a renderer draws, and
/// the two counters are the "refuse and count" contract the design settles:
/// a Sixel is never drawn wrong, and how much it could not represent is a
/// number rather than a silent truncation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelReport {
    /// How wide the data reached, in pixels: the last column the cursor
    /// advanced through, **not** the rightmost painted pixel. A trailing `?`
    /// (all bits clear) still advances a column, and `[[source: ctlseqs]]`
    /// mode 8452 places the next text past the image's extent, so the extent
    /// is the data's, not the ink's. Independent of the canvas: it is what the
    /// sender described, and `refused` is what did not fit.
    pub width: SixelPx,
    /// How tall the data reached, in pixels: six times the number of bands a
    /// data byte occupied. The extent, not the ink, for the reason `width` is.
    pub height: SixelPx,
    /// Pixels that would have fallen outside the canvas.
    pub refused: u32,
    /// Colour-register defines this decoder did not apply, so the caller knows
    /// the image is missing a colour. Two cases fall here: HLS (`Pu = 1`),
    /// because `[[source: vt3xx-sixel]]` gives the hue as "0 to 360 degrees"
    /// and defers the origin to a chapter that is not pinned and DEC's HLS
    /// does not put red at 0; and an unrecognised coordinate system. A wrongly
    /// converted register is worse than a counted one -- the "never record a
    /// mode you do not act on" rule -- so both are counted rather than guessed,
    /// and pinning chapter 2 for the HLS conversion is the follow-on.
    pub unapplied_registers: u32,
}

/// The 256 colour registers a Sixel image shares
/// (`[[source: vt3xx-sixel]]`, `Pc` 0..=255).
const REGISTERS: usize = 256;

/// The Sixel decoder. Stateless between calls; a decode is one pass over the
/// data with the caller's canvas.
pub struct Sixel;

impl Sixel {
    /// Decode `data` -- the bytes after the `q` and before the `ST` -- into
    /// `canvas`. The canvas starts however the caller cleared it (`DEFAULT` for
    /// transparent), and set data bits paint the current register; unset bits
    /// leave the pixel, so a later Sixel over the same canvas overlays it.
    pub fn decode(data: &[u8], canvas: &mut SixelCanvas) -> SixelReport {
        Sixel::walk(data, Some(canvas))
    }

    /// The extent the data reaches -- its `width` and `height` -- without
    /// painting. **The same walk as [`Sixel::decode`], not a second grammar**,
    /// so a size reserved from `extent` is exactly what `decode` fills; two
    /// spellings of one grammar is the `csi_whole` mistake this repo already
    /// paid for. A caller sizes an image arena from this, then decodes into it.
    pub fn extent(data: &[u8]) -> SixelReport {
        Sixel::walk(data, None)
    }

    /// The one grammar walk. `canvas` absent measures the extent and paints
    /// nothing; present, it paints and counts what fell outside. `width` and
    /// `height` are the data's reach either way, because the extent is tracked
    /// independently of whether a pixel landed.
    fn walk(data: &[u8], mut canvas: Option<&mut SixelCanvas>) -> SixelReport {
        let mut regs = [ColorWord::DEFAULT; REGISTERS];
        let mut reg: usize = 0;
        let mut x = SixelPx::ZERO;
        let mut band_top = SixelPx::ZERO;
        let mut width = SixelPx::ZERO;
        let mut height = SixelPx::ZERO;
        let mut refused: u32 = 0;
        let mut unapplied: u32 = 0;
        // The repeat introducer applies to the one data byte that follows it;
        // one column otherwise.
        let mut repeat: u16 = 1;

        let mut i = 0;
        while i < data.len() {
            let b = data[i];
            match b {
                // Colour register: `#Pc` selects, `#Pc;Pu;Px;Py;Pz` defines.
                b'#' => {
                    let (pc, mut j) = number(data, i + 1);
                    reg = usize::from(pc).min(REGISTERS - 1);
                    if data.get(j) == Some(&b';') {
                        let (pu, k1) = number(data, j + 1);
                        let (px, k2) = number_after_sep(data, k1);
                        let (py, k3) = number_after_sep(data, k2);
                        let (pz, k4) = number_after_sep(data, k3);
                        j = k4;
                        match pu {
                            2 => regs[reg] = rgb(px, py, pz),
                            // HLS (`Pu = 1`) and any unrecognised system are
                            // left unapplied rather than guessed at, and
                            // counted so the caller can see it.
                            _ => unapplied = unapplied.saturating_add(1),
                        }
                    }
                    i = j;
                    repeat = 1;
                }
                // Repeat introducer: the next data byte is drawn `Pn` times.
                // `[[source: vt3xx-sixel]]` is silent on `Pn = 0`;
                // `[[source: dec-std-070]]` 070-9 §8.1.1 settles it: "A repeat
                // count of zero implies a repeat count of 1" (PDF p. 920).
                b'!' => {
                    let (pn, j) = number(data, i + 1);
                    repeat = pn.max(1);
                    i = j;
                }
                // Graphics carriage return: back to the left of this band.
                b'$' => {
                    x = SixelPx::ZERO;
                    i += 1;
                    repeat = 1;
                }
                // Graphics new line: left, and down one six-pixel band.
                b'-' => {
                    x = SixelPx::ZERO;
                    band_top = band_top.plus_six();
                    i += 1;
                    repeat = 1;
                }
                // Raster attributes: aspect ratio and a declared size. The
                // aspect is informational to a pixel decoder and the size is
                // the caller's canvas here, so the parameters are consumed and
                // not otherwise used.
                b'"' => {
                    let (_pan, k1) = number(data, i + 1);
                    let (_pad, k2) = number_after_sep(data, k1);
                    let (_ph, k3) = number_after_sep(data, k2);
                    let (_pv, k4) = number_after_sep(data, k3);
                    i = k4;
                    repeat = 1;
                }
                // A sixel data byte: six vertical pixels, the low six bits of
                // `b - 0x3F`, least significant at the top
                // (`[[source: vt3xx-sixel]]`).
                0x3f..=0x7e => {
                    let bits = b - 0x3f;
                    let colour = regs[reg];
                    let mut n = repeat;
                    while n > 0 {
                        for row in 0u8..6 {
                            if bits & (1 << row) != 0 {
                                let y = band_top.plus_row(row);
                                // Absent in measure mode: the extent below is
                                // tracked whether or not a pixel is painted.
                                if let Some(c) = canvas.as_deref_mut() {
                                    match c.paint(x, y, colour) {
                                        Painted::Inside => {}
                                        Painted::Refused => refused = refused.saturating_add(1),
                                    }
                                }
                            }
                        }
                        x = x.next();
                        n -= 1;
                    }
                    // The extent is where the data reached, not where pixels
                    // were set: `x` is now the first column past this data byte,
                    // and the band is six rows tall whether or not any bit set.
                    width = width.largest(x);
                    height = height.largest(band_top.plus_six());
                    i += 1;
                    repeat = 1;
                }
                // Everything else is skipped. No C0 reaches here -- the Sixel
                // arm of `str_put` drops each one before the body is stored --
                // so what does is `SPACE`, a byte in `80-FF`, and a command this
                // decoder does not recognise (note AN, section 11).
                _ => {
                    i += 1;
                }
            }
        }

        SixelReport {
            width,
            height,
            refused,
            unapplied_registers: unapplied,
        }
    }
}

/// A colour register from RGB percentages (`Pu = 2`): each of `Px`, `Py`, `Pz`
/// is 0 to 100, scaled to a 0..=255 channel with rounding.
fn rgb(r: u16, g: u16, b: u16) -> ColorWord {
    ColorWord::rgb(pct(r), pct(g), pct(b))
}

/// A 0-100 percentage as a channel, clamped and rounded.
fn pct(v: u16) -> Chan {
    let v = v.min(100);
    // 0..=100 -> 0..=255, `(v * 255 + 50) / 100`, all within `u16`.
    let scaled = (v * 255 + 50) / 100;
    Chan::from_u8(u8::try_from(scaled).unwrap_or(u8::MAX))
}

/// Read a run of ASCII digits starting at `at`, returning the value (saturating
/// at `u16::MAX`) and the index of the first non-digit.
fn number(data: &[u8], at: usize) -> (u16, usize) {
    let mut v: u16 = 0;
    let mut i = at;
    while let Some(&b) = data.get(i) {
        if b.is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add(u16::from(b - b'0'));
            i += 1;
        } else {
            break;
        }
    }
    (v, i)
}

/// Read a `;`-separated parameter: skip the separator if present, then a number.
/// A missing parameter reads as zero, which is the default a Sixel omits.
fn number_after_sep(data: &[u8], at: usize) -> (u16, usize) {
    if data.get(at) == Some(&b';') {
        number(data, at + 1)
    } else {
        (0, at)
    }
}
