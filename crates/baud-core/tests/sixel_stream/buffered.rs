//! The buffered Sixel decoder as it was built at e46ee3a7, kept as the
//! oracle the streaming one is held to: the body whole, `Sixel::extent`'s
//! walk to measure it, a span reserved at the lowest gap that holds the
//! measured size and cleared, and the same walk again to paint it. It is
//! that commit's `crates/baud-domain/src/sixel.rs` walk, its numbers, its colour
//! conversions and its aspect table, with only what the test cannot reach
//! from outside the crate (the canvas's painter, the aspect's field)
//! spelled locally. A second spelling of the grammar on purpose: an oracle
//! that shared the decoder's would agree with any mistake in it. **The
//! aspect ratio is 2026-10-23's**, 070-9 §7.1's table preserved where it
//! is not whole, a ratio below one drawn 1:1 and a `P1` past 9 read as
//! omitted, spelled here as the device row a sixel row starts at,
//! `(2 * y * num + den) / (2 * den)`. **And a pixel takes its register's
//! colour when the body ends, 2026-11-01's rule**: the walk notes the
//! register each painted position was last painted with, and the colour
//! each register ends with is written at the end, so a redefinition
//! recolours what the body drew before it, as a VT340's colour map does.

use baud_domain::{Chan, Pixel};

/// What the buffered walk reached: its extent, counted past 65535 as the
/// original counted, so an image too large to state is seen as one.
pub struct Reach {
    pub width: u32,
    pub height: u32,
    /// Painted positions whose colour at the end is not the colour their
    /// register had when they were painted: what the end's mapping
    /// changed, zero where the walk only measures.
    pub recoloured: usize,
    /// The two things that end a decoder's painting in colours, counted
    /// where the walk paints: a register's first paint in a colour another
    /// painted register has, or none (clear); and a painted register
    /// defined again in another colour.
    pub shared_colour: usize,
    pub clear_colour: usize,
    pub redefined: usize,
}

/// The ratio, vertical over horizontal, for the envelope's `P1`.
pub fn rows_of_macro(p1: Option<u16>) -> (u16, u16) {
    let num = match p1 {
        Some(2) => 450,
        Some(3) => 300,
        Some(4) => 250,
        Some(6) => 150,
        Some(7) => 130,
        Some(8) => 112,
        Some(9) => 100,
        _ => 200,
    };
    (num, 100)
}

fn rows_of_raster(pan: u16, pad: u16) -> (u16, u16) {
    let (pan, pad) = (pan.max(1), pad.max(1));
    if pan < pad { (1, 1) } else { (pan, pad) }
}

/// The device row sixel row `y` starts at.
fn start(y: u32, (num, den): (u16, u16)) -> u32 {
    let v = (2 * u64::from(y) * u64::from(num) + u64::from(den)) / (2 * u64::from(den));
    u32::try_from(v).unwrap_or(u32::MAX)
}

enum Raster {
    Open,
    Closed,
}

/// The walk. `canvas` absent measures; present, `(pixels, width)` is painted
/// where a pixel is inside it, as `SixelCanvas::paint` painted, each
/// position in the colour its last register ends the body with.
pub fn walk(data: &[u8], rows0: (u16, u16), mut canvas: Option<(&mut [Pixel], usize)>) -> Reach {
    let mut regs = [Pixel::CLEAR; 256];
    let mut painted: Vec<Option<(usize, Pixel)>> =
        vec![None; canvas.as_ref().map_or(0, |(px, _)| px.len())];
    let mut first: Vec<(usize, Pixel)> = Vec::new();
    let (mut shared_colour, mut clear_colour, mut redefined) = (0, 0, 0);
    let mut rows = rows0;
    let mut raster = Raster::Open;
    let mut reg: usize = 0;
    let mut x: u32 = 0;
    let mut band_n: u32 = 0;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut repeat: u16 = 1;
    let mut i = 0;
    while i < data.len() {
        let b = data[i];
        match b {
            b'#' => {
                let (pc, mut j) = number(data, i + 1);
                let named = usize::from(pc).min(255);
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
                        1 => {
                            if let Some(c) = hls(px, py, pz) {
                                regs[named] = c;
                                reg = named;
                            }
                        }
                        _ => reg = named,
                    }
                } else {
                    reg = named;
                }
                i = j;
                repeat = 1;
            }
            b'!' => {
                let (pn, j) = number(data, i + 1);
                repeat = pn.max(1);
                i = j;
            }
            b'$' => {
                x = 0;
                i += 1;
                repeat = 1;
                raster = Raster::Closed;
            }
            b'-' => {
                x = 0;
                band_n = band_n.saturating_add(1);
                i += 1;
                repeat = 1;
                raster = Raster::Closed;
            }
            b'"' => {
                let (pan, k1) = number(data, i + 1);
                let (pad, k2) = number_after_sep(data, k1);
                let (_ph, k3) = number_after_sep(data, k2);
                let (_pv, k4) = number_after_sep(data, k3);
                if let Raster::Open = raster {
                    rows = rows_of_raster(pan, pad);
                }
                i = k4;
                repeat = 1;
            }
            0x3f..=0x7e => {
                let bits = b - 0x3f;
                if bits != 0 && canvas.is_some() && !first.iter().any(|&(r, _)| r == reg) {
                    let c = regs[reg];
                    clear_colour += usize::from(c == Pixel::CLEAR);
                    shared_colour +=
                        usize::from(c != Pixel::CLEAR && first.iter().any(|&(_, d)| d == c));
                    first.push((reg, c));
                }
                let mut n = repeat;
                while n > 0 {
                    for row in 0u8..6 {
                        if bits & (1 << row) != 0 {
                            if let Some((px, w)) = canvas.as_mut() {
                                let h = px.len().checked_div(*w).unwrap_or(0);
                                let y0 = band_n.saturating_mul(6).saturating_add(u32::from(row));
                                for y in start(y0, rows)..start(y0.saturating_add(1), rows) {
                                    let (xi, yi) = (
                                        usize::try_from(x).unwrap_or(usize::MAX),
                                        usize::try_from(y).unwrap_or(usize::MAX),
                                    );
                                    if x <= 65535 && y <= 65535 && xi < *w && yi < h {
                                        painted[yi * *w + xi] = Some((reg, regs[reg]));
                                    }
                                }
                            }
                        }
                    }
                    x = x.saturating_add(1);
                    n -= 1;
                }
                width = width.max(x);
                height = height.max(start(band_n.saturating_mul(6).saturating_add(6), rows));
                i += 1;
                repeat = 1;
                raster = Raster::Closed;
            }
            _ => i += 1,
        }
    }
    let mut recoloured = 0;
    if let Some((px, _)) = canvas {
        for (p, at) in px.iter_mut().zip(&painted) {
            if let Some((r, then)) = *at {
                *p = regs[r];
                recoloured += usize::from(regs[r] != then);
            }
        }
    }
    // A painted register defined again in another colour: its colour at
    // the end, or at any define between, against its first paint's.
    for &(r, c) in &first {
        redefined += usize::from(regs[r] != c);
    }
    Reach {
        width,
        height,
        recoloured,
        shared_colour,
        clear_colour,
        redefined,
    }
}

fn rgb(r: u16, g: u16, b: u16) -> Pixel {
    Pixel::opaque(pct(r), pct(g), pct(b))
}

fn hls(h: u16, l: u16, s: u16) -> Option<Pixel> {
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
    Some(Pixel::opaque(chan(0), chan(8), chan(4)))
}

fn pct(v: u16) -> Chan {
    let v = v.min(100);
    let scaled = (v * 255 + 50) / 100;
    Chan::from_u8(u8::try_from(scaled).unwrap_or(u8::MAX))
}

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

fn number_after_sep(data: &[u8], at: usize) -> (u16, usize) {
    if data.get(at) == Some(&b';') {
        number(data, at + 1)
    } else {
        (0, at)
    }
}
