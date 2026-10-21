//! **A sixel dump**: a picture written for a printer as `[[source:
//! dec-std-070]]` 070-7 § 7.8.2 has a terminal print its bit map, "setting
//! context and attributes for the pixels and then printing each Sixel in
//! left-to-right, top-to-bottom order. Wherever possible, run-length
//! encoding is used".
//!
//! **Level 2 always** (070-7 § 7.8.2.4; Jev's lean, LEVEL_2, 0.78/0.94):
//! baud's printer reads raster attributes, the grid size and SSU, and the
//! level exists "to match the capabilities of the printer being used", so
//! each pixel is sent once at 1:1, where Level 1 would OR vertical pairs.
//! The dump is SSU `ESC [ 2 SP I` ("SSU should be sent once before each
//! sixel dump"), `ESC P 0 ; Ps2 ; Pn3 q`, raster attributes `" 1 ; 1 ;
//! Pn6 ; Pn7`, the colour specifiers in colour, the bands and `ESC \`; no
//! `CR` first, the host having started it. Pn6 and Pn7, "number of pixels
//! in image horizontally" and "vertically", count the offset's blank
//! pixels as well as the picture's, so a printer reading the extent takes
//! the offset as part of the image; the offset is held to 17 inches of the
//! dump's pixels each way.
//!
//! **Which register a pixel takes**: in monochrome every pixel with a bit
//! set in any plane is ink and no register is named, "a logical OR of all
//! pixels which are selected for any color"; in colour register 0 is the
//! background, sent only when it is printed, and every other colour a
//! register of its own while 255 suffice, past which every colour takes the
//! nearest of a fixed 6 by 7 by 6 cube (Jev's lean, EXACT_ELSE_CUBE,
//! 0.24/0.66), 070-9 § 8.1.3's registers being 0 to 255.
//!
//! The caller supplies the picture ([`DumpCanvas`]) and the room the bands
//! are built in; nothing is allocated.

use crate::{
    Chan, ImagePx, Pixel, PrintBackground, PrintColour, PrintRotation, PrintSize, PrintSyntax,
};

/// The picture a dump prints: the colour at each of its pixels, or `None`
/// where "no bits [are] set in any plane", the background, index 0.
pub trait DumpCanvas {
    fn at(&self, x: ImagePx, y: ImagePx) -> Option<Pixel>;
}

/// What a dump prints and how: the picture's size, `S(H(P[x,y]))`'s offset
/// in printer pixels, the five graphics print choices, and the colour of
/// index 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DumpSpec {
    pub width: ImagePx,
    pub height: ImagePx,
    pub offset: (ImagePx, ImagePx),
    pub size: PrintSize,
    pub rotation: PrintRotation,
    pub colour: PrintColour,
    pub syntax: PrintSyntax,
    pub background: PrintBackground,
    pub index0: Pixel,
}

/// One pixel of a band: no ink, or a register.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixelInk(u16);

impl SixelInk {
    pub const NONE: SixelInk = SixelInk(u16::MAX);

    pub const fn register(n: u8) -> SixelInk {
        SixelInk(n as u16)
    }

    fn reg(self) -> Option<u8> {
        u8::try_from(self.0).ok()
    }

    fn to_le(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }

    fn from_le(b: [u8; 2]) -> SixelInk {
        SixelInk(u16::from_le_bytes(b))
    }
}

/// The registers named past 255 distinct colours: six reds, seven greens
/// and six blues, 252 in all.
const CUBE: (u16, u16, u16) = (6, 7, 6);

/// The open-addressed table of exact colours: twice the 255 registers.
const SLOTS: usize = 512;

/// The registers a colour dump names: the exact colours, or the cube.
#[derive(Clone, Copy)]
pub struct DumpPalette {
    colours: [Pixel; 256],
    /// Registers in use, register 0 among them only for a background.
    used: u16,
    table: [u16; SLOTS],
    cube: Cube,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cube {
    Exact,
    Cube,
}

fn rgb24(p: Pixel) -> u32 {
    p.bits() & 0x00FF_FFFF
}

fn slot_of(rgb: u32) -> usize {
    // The high bits of a multiplicative hash, as `StyleId::bucket` takes.
    (rgb.wrapping_mul(0x9E37_79B1) >> 23) as usize
}

impl DumpPalette {
    /// **The registers a dump of `canvas` names**, every pixel looked at
    /// once; register 0 the background when it is printed.
    pub fn of(spec: &DumpSpec, canvas: &dyn DumpCanvas) -> DumpPalette {
        let mut p = DumpPalette {
            colours: [Pixel::CLEAR; 256],
            used: 1,
            table: [0; SLOTS],
            cube: Cube::Exact,
        };
        p.colours[0] = spec.index0;
        if spec.colour == PrintColour::Mono {
            return p;
        }
        'all: for y in 0..spec.height.raw() {
            for x in 0..spec.width.raw() {
                if let Some(c) = canvas.at(ImagePx::of(x), ImagePx::of(y)) {
                    if p.exact(c).is_none() && !p.add(c) {
                        p.cube = Cube::Cube;
                        p.used = 1 + CUBE.0 * CUBE.1 * CUBE.2;
                        break 'all;
                    }
                }
            }
        }
        p
    }

    fn exact(&self, c: Pixel) -> Option<u8> {
        let key = rgb24(c);
        let mut i = slot_of(key);
        loop {
            match self.table[i] {
                0 => return None,
                r => {
                    let r = u8::try_from(r).ok()?;
                    if rgb24(self.colours[usize::from(r)]) == key {
                        return Some(r);
                    }
                }
            }
            i = (i + 1) % SLOTS;
        }
    }

    /// Give `c` the next register; false when all 255 are taken.
    fn add(&mut self, c: Pixel) -> bool {
        if self.used > 255 {
            return false;
        }
        let r = self.used;
        self.colours[usize::from(r)] = c;
        self.used += 1;
        let mut i = slot_of(rgb24(c));
        while self.table[i] != 0 {
            i = (i + 1) % SLOTS;
        }
        self.table[i] = r;
        true
    }

    /// The register a colour pixel takes.
    fn register(&self, c: Pixel) -> SixelInk {
        match self.cube {
            Cube::Exact => self.exact(c).map_or(SixelInk::NONE, SixelInk::register),
            Cube::Cube => {
                let (r, g, b) = c.rgb();
                let step = |v: Chan, n: u16| (u16::from(v.get()) * (n - 1) + 127) / 255;
                let i = (step(r, CUBE.0) * CUBE.1 + step(g, CUBE.1)) * CUBE.2 + step(b, CUBE.2);
                SixelInk::register(u8::try_from(1 + i).unwrap_or(u8::MAX))
            }
        }
    }

    /// Register `r`'s colour.
    fn colour(&self, r: u16) -> Pixel {
        match (self.cube, r) {
            (_, 0) | (Cube::Exact, _) => self.colours[usize::from(r.min(255))],
            (Cube::Cube, r) => {
                let i = r - 1;
                let level = |v: u16, n: u16| {
                    Chan::from_u8(u8::try_from(v * 255 / (n - 1)).unwrap_or(u8::MAX))
                };
                let b = i % CUBE.2;
                let g = (i / CUBE.2) % CUBE.1;
                let rr = i / (CUBE.2 * CUBE.1);
                Pixel::opaque(level(rr, CUBE.0), level(g, CUBE.1), level(b, CUBE.2))
            }
        }
    }
}

/// The grid a dump is sent at, in decipoints: 6 compressed, 9 expanded or
/// rotated (070-7 § 7.8.2.4's Pn3).
fn grid(spec: &DumpSpec) -> usize {
    match (spec.size, spec.rotation) {
        (PrintSize::Compressed, PrintRotation::Upright) => 6,
        _ => 9,
    }
}

/// The longest side of the largest common sheet, tabloid's, in decipoints:
/// 17 inches of 720.
const SHEET_DECIPOINTS: usize = 17 * 720;

/// **The offset, held to a sheet**: each part at most 17 inches of the
/// dump's pixels, 2040 at grid 6 and 1360 at grid 9. 070-8 § 8.4.5.2 item 6
/// gives the offset no range, its relationship to the coordinate space
/// "implementation- and retransmission protocol- dependent", and past the
/// largest sheet it would put the picture off the paper while its blank
/// pixels cost a page's work each (Jev's lean, CLAMP_SHEET, 0.25/0.35
/// after a re-ask).
fn offset(spec: &DumpSpec) -> (usize, usize) {
    let most = SHEET_DECIPOINTS / grid(spec);
    (
        spec.offset.0.index().min(most),
        spec.offset.1.index().min(most),
    )
}

/// The dump's own size on paper: the picture turned when rotated, with the
/// offset before it.
fn paper(spec: &DumpSpec) -> (usize, usize) {
    let (w, h) = (spec.width.index(), spec.height.index());
    let (ox, oy) = offset(spec);
    match spec.rotation {
        PrintRotation::Upright => (ox + w, oy + h),
        PrintRotation::Rotated => (ox + h, oy + w),
    }
}

/// **The bytes of room a dump's bands are built in**: six rows of its
/// width, two bytes a pixel.
pub fn dump_room(spec: &DumpSpec) -> usize {
    paper(spec).0 * 6 * 2
}

/// The picture's pixel at paper pixel `(px, py)`: past the offset, turned
/// counter-clockwise when rotated, "the left side of the paper ...
/// corresponds to the top of the image on the terminal screen" (070-7
/// § 7.8.2.3). `None` outside it.
fn source(spec: &DumpSpec, px: usize, py: usize) -> Option<(ImagePx, ImagePx)> {
    let (ox, oy) = offset(spec);
    let u = px.checked_sub(ox)?;
    let v = py.checked_sub(oy)?;
    let (w, h) = (spec.width.index(), spec.height.index());
    let (x, y) = match spec.rotation {
        PrintRotation::Upright => (u, v),
        PrintRotation::Rotated => (w.checked_sub(v + 1)?, u),
    };
    if x >= w || y >= h {
        return None;
    }
    Some((
        ImagePx::of(u16::try_from(x).ok()?),
        ImagePx::of(u16::try_from(y).ok()?),
    ))
}

/// A number in decimal.
fn decimal(n: usize, out: &mut dyn FnMut(&[u8])) {
    let mut b = [0u8; 20];
    let mut i = b.len();
    let mut v = n;
    loop {
        i -= 1;
        b[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    out(&b[i..]);
}

/// A channel as a percentage, rounded: 070-9's RGB specifier.
fn percent(c: Chan) -> usize {
    (usize::from(c.get()) * 100 + 127) / 255
}

/// **A colour as DEC HLS**: hue in degrees with blue at 0 ("Blue: 0 degrees
/// Red: 120 degrees Green: 240 degrees", `[[source: vt3xx-screen-control]]`),
/// lightness and saturation in percent; the inverse of the conversion the
/// decoder applies (`crate::sixel`'s `hls`), the double hexcone turned 120
/// degrees.
pub fn dec_hls(p: Pixel) -> (u16, u16, u16) {
    let (r, g, b) = p.rgb();
    let (r, g, b) = (i32::from(r.get()), i32::from(g.get()), i32::from(b.get()));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let round = |a: i32, q: i32| (2 * a + q).div_euclid(2 * q);
    let l = round((max + min) * 100, 510);
    let s = match d {
        0 => 0,
        _ => round(100 * d, 255 - (max + min - 255).abs()),
    };
    let h_css = match d {
        0 => 0,
        _ if max == r => round(60 * (g - b), d).rem_euclid(360),
        _ if max == g => round(60 * (b - r), d) + 120,
        _ => round(60 * (r - g), d) + 240,
    };
    // A grey has no hue; 0, as HLS's own convention gives it.
    let h = match d {
        0 => 0,
        _ => (h_css + 120).rem_euclid(360),
    };
    let c = |v: i32| u16::try_from(v).unwrap_or(0);
    (c(h), c(l), c(s.min(100)))
}

/// One register's specifier: `# Pc ; 1 ; H ; L ; S` or `# Pc ; 2 ; R ; G ; B`.
fn specifier(r: u16, c: Pixel, syntax: PrintSyntax, out: &mut dyn FnMut(&[u8])) {
    out(b"#");
    decimal(usize::from(r), out);
    let (u, a, b, cc) = match syntax {
        PrintSyntax::Hls => {
            let (h, l, s) = dec_hls(c);
            (b"1", usize::from(h), usize::from(l), usize::from(s))
        }
        PrintSyntax::Rgb => {
            let (r, g, b) = c.rgb();
            (b"2", percent(r), percent(g), percent(b))
        }
    };
    out(b";");
    out(u);
    for v in [a, b, cc] {
        out(b";");
        decimal(v, out);
    }
}

/// A run of `n` of the sixel `c`: `!` and the count from four up, where it
/// is shorter.
fn run(c: u8, n: usize, out: &mut dyn FnMut(&[u8])) {
    match n {
        0 => {}
        1..=3 => {
            for _ in 0..n {
                out(&[c]);
            }
        }
        _ => {
            out(b"!");
            decimal(n, out);
            out(&[c]);
        }
    }
}

/// Whether a dump names its registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixelSelect {
    /// Colour: `#Pc` before each register's sixels.
    Registers,
    /// Monochrome: every ink in the printer's own colour.
    None,
}

/// **One band**, six rows of `width` inks, as sixels: for each register in
/// it, lowest first, `#Pc` when selecting, its sixels with runs, `$` before
/// the next; a register's blank sixels after its last are left off.
pub fn encode_band(
    band: &[SixelInk],
    width: usize,
    select: SixelSelect,
    out: &mut dyn FnMut(&[u8]),
) {
    encode_with(
        &|i| band.get(i).copied().unwrap_or(SixelInk::NONE),
        width,
        select,
        out,
    );
}

/// [`encode_band`] over inks read by index, row-major.
fn encode_with(
    ink_at: &dyn Fn(usize) -> SixelInk,
    width: usize,
    select: SixelSelect,
    out: &mut dyn FnMut(&[u8]),
) {
    let mut present = [0u64; 4];
    for i in 0..width * 6 {
        if let Some(r) = ink_at(i).reg() {
            present[usize::from(r / 64)] |= 1 << (r % 64);
        }
    }
    let regs = (0u16..256).filter(|&r| present[usize::from(r / 64)] & (1 << (r % 64)) != 0);
    let mut first = true;
    for r in regs {
        let wanted = |ink: SixelInk| match select {
            SixelSelect::Registers => ink.reg() == u8::try_from(r).ok(),
            SixelSelect::None => ink != SixelInk::NONE,
        };
        if !first {
            out(b"$");
        }
        first = false;
        if select == SixelSelect::Registers {
            out(b"#");
            decimal(usize::from(r), out);
        }
        let sixel = |x: usize| {
            let mut bits = 0u8;
            for row in 0..6 {
                if wanted(ink_at(row * width + x)) {
                    bits |= 1 << row;
                }
            }
            b'?' + bits
        };
        let end = (0..width)
            .rev()
            .find(|&x| sixel(x) != b'?')
            .map_or(0, |x| x + 1);
        let mut x = 0;
        while x < end {
            let c = sixel(x);
            let mut n = 1;
            while x + n < end && sixel(x + n) == c {
                n += 1;
            }
            run(c, n, out);
            x += n;
        }
        if select == SixelSelect::None {
            break;
        }
    }
}

/// **The dump of `canvas`**: SSU, the introducer, raster attributes, the
/// specifiers in colour, the bands and `ST`, written to `out`; `room` is
/// [`dump_room`] bytes the bands are built in. False, and nothing written,
/// when `room` is short.
pub fn write_dump(
    spec: &DumpSpec,
    canvas: &dyn DumpCanvas,
    room: &mut [u8],
    out: &mut dyn FnMut(&[u8]),
) -> bool {
    let (pw, ph) = paper(spec);
    if room.len() < dump_room(spec) {
        return false;
    }
    let palette = DumpPalette::of(spec, canvas);
    let colour = spec.colour == PrintColour::Colour;
    let background = colour && spec.background == PrintBackground::On;
    out(b"\x1b[2 I\x1bP0;");
    out(match background {
        true => b"2",
        false => b"1",
    });
    out(match grid(spec) {
        6 => b";6q\"1;1;",
        _ => b";9q\"1;1;",
    });
    decimal(pw, out);
    out(b";");
    decimal(ph, out);
    if colour {
        let first = if background { 0 } else { 1 };
        for r in first..palette.used {
            specifier(r, palette.colour(r), spec.syntax, out);
        }
    }
    let select = match colour {
        true => SixelSelect::Registers,
        false => SixelSelect::None,
    };
    let bands = ph.div_ceil(6);
    for band in 0..bands {
        for row in 0..6 {
            for px in 0..pw {
                let py = band * 6 + row;
                let ink = match (py < ph).then(|| source(spec, px, py)).flatten() {
                    None => SixelInk::NONE,
                    Some((x, y)) => match (canvas.at(x, y), colour) {
                        (Some(_), false) => SixelInk::register(0),
                        (Some(c), true) => palette.register(c),
                        (None, _) if background => SixelInk::register(0),
                        (None, _) => SixelInk::NONE,
                    },
                };
                let i = (row * pw + px) * 2;
                room[i..i + 2].copy_from_slice(&ink.to_le());
            }
        }
        let band_room = &*room;
        let ink_at = |i: usize| match band_room.get(2 * i..2 * i + 2) {
            Some(b) => SixelInk::from_le([b[0], b[1]]),
            None => SixelInk::NONE,
        };
        encode_with(&ink_at, pw, select, out);
        if band + 1 < bands {
            out(b"-");
        }
    }
    out(b"\x1b\\");
    true
}
