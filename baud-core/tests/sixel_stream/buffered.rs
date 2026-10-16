//! The buffered Sixel decoder as it was built at 2992a8e9, kept as the
//! oracle the streaming one is held to: the body whole, `Sixel::extent`'s
//! walk to measure it, a span reserved at the lowest gap that holds the
//! measured size and cleared, and the same walk again to paint it. It is
//! that commit's `baud-domain/src/sixel.rs` walk, its numbers, its colour
//! conversions and its aspect table, with only what the test cannot reach
//! from outside the crate (the canvas's painter, the aspect's field)
//! spelled locally. A second spelling of the grammar on purpose: an oracle
//! that shared the decoder's would agree with any mistake in it.

use baud_domain::{Chan, Pixel};

/// What the buffered walk reached: its extent, counted past 65535 as the
/// original counted, so an image too large to state is seen as one.
pub struct Reach {
    pub width: u32,
    pub height: u32,
}

/// The device rows a sixel row takes for the envelope's `P1`, as
/// `SixelAspect::from_macro` was.
pub fn rows_of_macro(p1: Option<u16>) -> u16 {
    match p1 {
        None | Some(0) | Some(1) | Some(5) => 2,
        Some(3) => 3,
        _ => 1,
    }
}

fn rows_of_raster(pan: u16, pad: u16) -> u16 {
    let (pan, pad) = (pan.max(1), pad.max(1));
    match pan % pad {
        0 => pan / pad,
        _ => 1,
    }
}

enum Raster {
    Open,
    Closed,
}

/// The walk. `canvas` absent measures; present, `(pixels, width)` is painted
/// where a pixel is inside it, as `SixelCanvas::paint` painted.
pub fn walk(data: &[u8], rows0: u16, mut canvas: Option<(&mut [Pixel], usize)>) -> Reach {
    let mut regs = [Pixel::CLEAR; 256];
    let mut rows = rows0;
    let mut raster = Raster::Open;
    let mut reg: usize = 0;
    let mut x: u32 = 0;
    let mut band_top: u32 = 0;
    let mut width: u32 = 0;
    let mut height: u32 = 0;
    let mut band = 6 * u32::from(rows);
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
                band_top = band_top.saturating_add(band);
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
                    band = 6 * u32::from(rows);
                }
                i = k4;
                repeat = 1;
            }
            0x3f..=0x7e => {
                let bits = b - 0x3f;
                let colour = regs[reg];
                let mut n = repeat;
                while n > 0 {
                    for row in 0u8..6 {
                        if bits & (1 << row) != 0 {
                            if let Some((px, w)) = canvas.as_mut() {
                                let h = px.len().checked_div(*w).unwrap_or(0);
                                let top = band_top.saturating_add(u32::from(row) * u32::from(rows));
                                for k in 0..u32::from(rows) {
                                    let y = top.saturating_add(k);
                                    let (xi, yi) = (
                                        usize::try_from(x).unwrap_or(usize::MAX),
                                        usize::try_from(y).unwrap_or(usize::MAX),
                                    );
                                    if x <= 65535 && y <= 65535 && xi < *w && yi < h {
                                        px[yi * *w + xi] = colour;
                                    }
                                }
                            }
                        }
                    }
                    x = x.saturating_add(1);
                    n -= 1;
                }
                width = width.max(x);
                height = height.max(band_top.saturating_add(band));
                i += 1;
                repeat = 1;
                raster = Raster::Closed;
            }
            _ => i += 1,
        }
    }
    Reach { width, height }
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
