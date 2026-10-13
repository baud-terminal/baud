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

use crate::decode::{InputEncoding, StreamByte};
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

    /// A walk position as a coordinate, or `None` past what a `u16` holds.
    fn at(v: u32) -> Option<SixelPx> {
        u16::try_from(v).ok().map(SixelPx)
    }
}

/// Whether an image's extent fits a [`SixelPx`] each way. A Sixel can
/// describe more: `!65535~!1~` is 65536 columns, and a raster ratio of 20000
/// makes one band 120000 rows. The walk keeps counting past 65535, so an
/// extent too large to state is reported as such rather than stopped at the
/// limit while the painting runs on, and the caller refuses the image whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelFit {
    /// Width and height are each at most 65535: the report's extent is exact.
    Fits,
    /// Wider or taller than 65535: the extent is stopped there and is not the
    /// image's.
    TooLarge,
}

/// The pixel aspect ratio a Sixel is drawn at, as the device rows one sixel
/// row takes: a sixel pixel is one device pixel wide and this many tall.
///
/// `[[source: vt3xx-sixel]]` and `[[source: dec-std-070]]` 070-9 §7.1 agree
/// on the macro parameter `P1` where both make the ratio whole: omitted, 0, 1
/// and 5 are 2:1, 3 is 3:1, and 9 is 1:1. A raster command `"Pan;Pad` before
/// any data overrides it ([`Sixel::decode`]). The other values are not drawn
/// at a ratio: `P1` 2, 4, 6, 7 and 8, which 070-9 gives as 4.5, 2.5, 1.5, 1.3
/// and 1.12 to 1 and the VT330/VT340 manual rounds, are note AN's item 12, for
/// Jev, and a `P1` past 9, which neither text lists; each is drawn 1:1, as
/// every Sixel was before the ratio was honoured.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelAspect(u16);

impl SixelAspect {
    /// One device row a sixel row: what `P1 = 9` selects, and a raster
    /// `"1;1`.
    pub const ONE_TO_ONE: SixelAspect = SixelAspect(1);

    /// The ratio the envelope's macro parameter `P1` sets; `None` when it is
    /// omitted, which both texts make 2:1.
    pub fn from_macro(p1: Option<u16>) -> SixelAspect {
        match p1 {
            None | Some(0) | Some(1) | Some(5) => SixelAspect(2),
            Some(3) => SixelAspect(3),
            _ => SixelAspect::ONE_TO_ONE,
        }
    }

    /// The ratio a raster command's `Pan;Pad` sets. An omitted or zero
    /// parameter reads as one, 070-9 §8.1.2's guideline ("omitted parameters
    /// for Pn1 and Pn2 may be interpreted as '0' (zero) and default to '1'
    /// (one)", PDF p. 921). A ratio that is not a whole number, `"3;2` or
    /// `"1;2`, is item 12 and draws 1:1.
    fn from_raster(pan: u16, pad: u16) -> SixelAspect {
        let (pan, pad) = (pan.max(1), pad.max(1));
        match pan % pad {
            0 => SixelAspect(pan / pad),
            _ => SixelAspect::ONE_TO_ONE,
        }
    }
}

/// Whether a raster command may still set the ratio: only before the first
/// data byte and the first `$` or `-` (070-9 §8.1.2).
enum Raster {
    Open,
    Closed,
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
    ///
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
    /// How tall the data reached, in pixels: the bands a data byte occupied,
    /// each six sixel rows of as many pixels as the aspect ratio
    /// ([`SixelAspect`]). The extent, not the ink, for the reason `width` is.
    pub height: SixelPx,
    /// Pixels that would have fallen outside the canvas.
    pub refused: u32,
    /// Colour-register defines this decoder did not apply, so the caller knows
    /// the image is missing a colour: an unrecognised coordinate system, and
    /// an HLS (`Pu = 1`) define with a value past its range, which
    /// `[[source: dec-std-070]]` 070-9 §8.1.3 ignores whole. HLS itself is
    /// applied since 2026-10-11, its hue origin pinned (see `hls`); it was
    /// counted here until then.
    pub unapplied_registers: u32,
    /// Whether `width` and `height` are the image's or stopped at 65535.
    pub fit: SixelFit,
}

/// The 256 colour registers a Sixel image shares
/// (`[[source: vt3xx-sixel]]`, `Pc` 0..=255).
const REGISTERS: usize = 256;

/// The Sixel decoder. Stateless between calls; a decode is one pass over the
/// data with the caller's canvas.
pub struct Sixel;

impl Sixel {
    /// The byte a Sixel's handler stores for body byte `b` under `enc`, or
    /// `None` when the byte is to be dropped before the decoder.
    ///
    /// `[[source: dec-std-070]]` 070-9 §9 (PDF pp. 925-926), read for a body:
    /// the format effectors "shall have no effect on the interpretation of
    /// Sixel Control strings" and every other C0 that reaches a handler is
    /// ignored (§9.1); "2/0 (SPACE) Ignored ... 7/15 (DELETE) Ignored" (§9.2);
    /// and GR codes "are errors. The 8th bit is set to zero and the code is
    /// then processed as a GL code" (§9.4), so under ISO 8859-1 `A0` is a
    /// `SPACE` and `FF` a `DEL`, both dropped. §8.1 (PDF p. 919) says what
    /// the drop is for: a command "is terminated by any non parameter code
    /// ... Note Format Effectors (0/8 to 0/13) and SPACE (2/0) are ignored
    /// within sixel control strings", so neither ends a number. Under UTF-8
    /// `80-FF` belong to a character and are kept as they came, and under ISO
    /// 8859-1 `80-9F` are C1 controls the graph takes before a body sees them
    /// (§9.3), so both are kept. `[[source: williams]]` treats GR as GL "in
    /// all states" (462), and `[[source: ecma48]]` §9 gives "the contents of
    /// the command string" the GL meanings, so the texts agree.
    ///
    /// **The common byte is decided by one compare**
    /// ([`Sixel::body_byte_is_gl_past_space`]), and the encoding is read only
    /// for `A0-FF`. A fold and two compares on every byte, the first shape,
    /// retired 16.5% more instructions on `sixel/yazi-walk.raw` (the record,
    /// "A SPACE in a Sixel body is ignored, and a GR byte is its GL byte under
    /// ISO 8859-1").
    #[inline]
    pub fn body_byte(enc: InputEncoding, b: StreamByte) -> Option<StreamByte> {
        match b.raw_byte() {
            0x21..=0x7e => Some(b),
            _ => Sixel::body_byte_rare(enc, b),
        }
    }

    /// Whether `b` is in `21-7F`, GL past `SPACE`. [`Sixel::body_byte`] keeps
    /// every byte here but `DEL` as it is, under both encodings: every Sixel
    /// command and data byte. A handler behind baud's graph never sees a
    /// `DEL`, which the `DcsStr` row ignores as `[[source: williams]]`'s
    /// `dcs passthrough` does (388), so on its per-byte path it tests this
    /// inline, stores the byte as it is, and sends the rest to `body_byte`
    /// out of line.
    ///
    /// **One signed compare, the cost of the `is_c0` it replaced**: as an
    /// `i8`, `21-7F` are the bytes above `0x20`. Written as a `match` on `u8`
    /// ranges with `80-9F` beside them, LLVM tested the high bit first and the
    /// common byte took seven instructions where `is_c0` took three; as an
    /// unsigned range `21-7E` it took four, a subtract, a mask, a compare and
    /// a branch.
    #[inline]
    pub fn body_byte_is_gl_past_space(b: StreamByte) -> bool {
        i8::from_ne_bytes([b.raw_byte()]) > 0x20
    }

    /// [`Sixel::body_byte`] for a byte outside `21-7E`: a C0, `SPACE`, `DEL`,
    /// or one in `80-FF`, none of which a Sixel encoder sends but a line
    /// break.
    #[cold]
    #[inline(never)]
    fn body_byte_rare(enc: InputEncoding, b: StreamByte) -> Option<StreamByte> {
        match b.raw_byte() {
            0x80..=0x9f => Some(b),
            0xa0..=0xff => {
                let gl = enc.gr_as_gl(b);
                match gl.raw_byte() {
                    0x20 | 0x7f => None,
                    _ => Some(gl),
                }
            }
            _ => None,
        }
    }

    /// Decode `data` -- the bytes after the `q` and before the `ST` -- into
    /// `canvas`. The canvas starts however the caller cleared it (`DEFAULT` for
    /// transparent), and set data bits paint the current register; unset bits
    /// leave the pixel, so a later Sixel over the same canvas overlays it.
    ///
    /// `aspect` is what the envelope's `P1` set ([`SixelAspect::from_macro`]);
    /// a raster command at the head of `data` overrides it. A sixel row is
    /// painted that many device rows tall, and a band is six of them.
    pub fn decode(data: &[u8], aspect: SixelAspect, canvas: &mut SixelCanvas) -> SixelReport {
        Sixel::walk(data, aspect, Some(canvas))
    }

    /// The extent the data reaches -- its `width` and `height` -- without
    /// painting. **The same walk as [`Sixel::decode`], not a second grammar**,
    /// so a size reserved from `extent` is exactly what `decode` fills; two
    /// spellings of one grammar is the `csi_whole` mistake this repo already
    /// paid for. A caller sizes an image arena from this, then decodes into it.
    pub fn extent(data: &[u8], aspect: SixelAspect) -> SixelReport {
        Sixel::walk(data, aspect, None)
    }

    /// The one grammar walk. `canvas` absent measures the extent and paints
    /// nothing; present, it paints and counts what fell outside. `width` and
    /// `height` are the data's reach either way, because the extent is tracked
    /// independently of whether a pixel landed.
    fn walk(data: &[u8], aspect: SixelAspect, mut canvas: Option<&mut SixelCanvas>) -> SixelReport {
        let mut regs = [ColorWord::DEFAULT; REGISTERS];
        // The device rows one sixel row takes, and whether a raster command
        // may still change it.
        let mut rows = aspect.0;
        let mut raster = Raster::Open;
        let mut reg: usize = 0;
        // Positions and the extent are counted in `u32`, past what a
        // coordinate holds, so an image too large to state is seen as one.
        let mut x: u32 = 0;
        let mut band_top: u32 = 0;
        let mut width: u32 = 0;
        let mut height: u32 = 0;
        let mut band = 6 * u32::from(rows);
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
                    let named = usize::from(pc).min(REGISTERS - 1);
                    if data.get(j) == Some(&b';') {
                        let (pu, k1) = number(data, j + 1);
                        let (px, k2) = number_after_sep(data, k1);
                        let (py, k3) = number_after_sep(data, k2);
                        let (pz, k4) = number_after_sep(data, k3);
                        j = k4;
                        match pu {
                            2 => {
                                regs[named] = rgb(px, py, pz);
                                reg = named;
                            }
                            // HLS, hue 0-360 and lightness and saturation
                            // 0-100 (`[[source: vt3xx-sixel]]` Table 14-1).
                            // A value past its range ignores the whole
                            // command, `Pc` included: `[[source: dec-std-070]]`
                            // 070-9 §8.1.3 (PDF p. 921), so `reg` is kept.
                            1 => match hls(px, py, pz) {
                                Some(c) => {
                                    regs[named] = c;
                                    reg = named;
                                }
                                None => unapplied = unapplied.saturating_add(1),
                            },
                            // An unrecognised system selects `Pc` and is
                            // counted; 070-9 §8.1.3 ignores this whole command
                            // too, and RGB past 100 is clamped where it says
                            // the same: note AN's item 6, not built here.
                            _ => {
                                reg = named;
                                unapplied = unapplied.saturating_add(1);
                            }
                        }
                    } else {
                        reg = named;
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
                    x = 0;
                    i += 1;
                    repeat = 1;
                    raster = Raster::Closed;
                }
                // Graphics new line: left, and down one six-pixel band.
                b'-' => {
                    x = 0;
                    band_top = band_top.saturating_add(band);
                    i += 1;
                    repeat = 1;
                    raster = Raster::Closed;
                }
                // Raster attributes: the aspect ratio `Pan:Pad`, which sets the
                // rows a sixel row takes, and a declared size `Ph`x`Pv`, which
                // "does not limit the number of pixels a device may image"
                // (070-9 §8.1.2) and is not used here. After data or a `$` or
                // `-` the whole command is ignored, as §8.1.2 says.
                b'"' => {
                    let (pan, k1) = number(data, i + 1);
                    let (pad, k2) = number_after_sep(data, k1);
                    let (_ph, k3) = number_after_sep(data, k2);
                    let (_pv, k4) = number_after_sep(data, k3);
                    if let Raster::Open = raster {
                        rows = SixelAspect::from_raster(pan, pad).0;
                        band = 6 * u32::from(rows);
                    }
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
                                // Absent in measure mode: the extent below is
                                // tracked whether or not a pixel is painted.
                                if let Some(c) = canvas.as_deref_mut() {
                                    let top =
                                        band_top.saturating_add(u32::from(row) * u32::from(rows));
                                    for k in 0..u32::from(rows) {
                                        let y = top.saturating_add(k);
                                        let painted = match (SixelPx::at(x), SixelPx::at(y)) {
                                            (Some(px), Some(py)) => c.paint(px, py, colour),
                                            _ => Painted::Refused,
                                        };
                                        match painted {
                                            Painted::Inside => {}
                                            Painted::Refused => refused = refused.saturating_add(1),
                                        }
                                    }
                                }
                            }
                        }
                        x = x.saturating_add(1);
                        n -= 1;
                    }
                    // The extent is where the data reached, not where pixels
                    // were set: `x` is now the first column past this data byte,
                    // and the band is six rows tall whether or not any bit set.
                    width = width.max(x);
                    height = height.max(band_top.saturating_add(band));
                    i += 1;
                    repeat = 1;
                    raster = Raster::Closed;
                }
                // Everything else is skipped. No C0, `SPACE` or `DEL` reaches
                // here from the core -- the Sixel arm of `str_put` keeps only
                // what [`Sixel::body_byte`] keeps, a GR byte under ISO 8859-1
                // as its GL byte -- so what does is a byte in `80-FF` under
                // UTF-8, and a command this decoder does not recognise (note
                // AN, section 11). A caller that hands this a body with a
                // `SPACE` in a number gets the number ended there: the drop is
                // `body_byte`'s, before the body is stored.
                _ => {
                    i += 1;
                }
            }
        }

        let fit = match (SixelPx::at(width), SixelPx::at(height)) {
            (Some(_), Some(_)) => SixelFit::Fits,
            _ => SixelFit::TooLarge,
        };
        SixelReport {
            width: SixelPx::at(width).unwrap_or(SixelPx(u16::MAX)),
            height: SixelPx::at(height).unwrap_or(SixelPx(u16::MAX)),
            refused,
            unapplied_registers: unapplied,
            fit,
        }
    }
}

/// A colour register from RGB percentages (`Pu = 2`): each of `Px`, `Py`, `Pz`
/// is 0 to 100, scaled to a 0..=255 channel with rounding.
fn rgb(r: u16, g: u16, b: u16) -> ColorWord {
    ColorWord::rgb(pct(r), pct(g), pct(b))
}

/// A colour register from DEC HLS (`Pu = 1`): hue `h` in degrees, lightness
/// `l` and saturation `s` in percent; `None` when any is past its range
/// (`[[source: vt3xx-sixel]]` Table 14-1: 0-360, 0-100, 0-100).
///
/// **DEC's hue origin is blue.** `[[source: vt3xx-screen-control]]`, the
/// chapter the Sixel chapter defers its colours to: "Blue: 0 degrees Red: 120
/// degrees Green: 240 degrees". DEC gives no formula, calling HLS
/// "universally recognized" (`[[source: vt3xx-sixel]]`), so this is the
/// double-hexcone conversion `[[source: css-color-4-hsl]]` prints, whose red
/// is at 0, with the hue turned 120 degrees back. It reproduces that
/// chapter's Table 2-3, the VT340's sixteen default colours given as HLS and
/// RGB both, within one percent on every row.
///
/// In integers, since `no_std` has no `f32::round`: with `K = (30 n + h) mod
/// 360` and `m = clamp(min(K - 90, 270 - K), -30, 30)`, CSS's
/// `l - a * max(-1, min(k - 3, 9 - k, 1))` is `(3000 l - s * min(l, 100 - l)
/// * m) / 3000` percent, which stays within 0..=100, so the channel is that
/// times 255/100, rounded half up as [`pct`] rounds.
fn hls(h: u16, l: u16, s: u16) -> Option<ColorWord> {
    if h > 360 || l > 100 || s > 100 {
        return None;
    }
    let h = (i32::from(h) + 240) % 360;
    let (l, s) = (i32::from(l), i32::from(s));
    let a = s * l.min(100 - l);
    let chan = |n: i32| {
        let k = (30 * n + h) % 360;
        let m = (k - 90).min(270 - k).clamp(-30, 30);
        let v = 3000 * l - a * m;
        let c = (255 * v + 150_000) / 300_000;
        Chan::from_u8(u8::try_from(c).unwrap_or(u8::MAX))
    };
    Some(ColorWord::rgb(chan(0), chan(8), chan(4)))
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
