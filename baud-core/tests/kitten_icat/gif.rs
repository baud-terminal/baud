//! **The GIF's own facts, read here and not by baud's decoder**
//! (`[[source: gif89a]]`): the logical screen, the NETSCAPE loop count,
//! and each image's rectangle, Graphic Control Extension and pixels,
//! decoded by Appendix F's LZW. A pixel is its colour from the image's
//! table, opaque, or `0, 0, 0, 0` where its index is the transparent one,
//! which is what `kitten icat` sends for it (the inflated payloads,
//! `stream.txt`).

#![allow(dead_code)]

/// One image of the stream, as its descriptor and control extension say.
pub struct Image {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
    /// The Delay Time, in hundredths of a second.
    pub delay_cs: u16,
    /// The Disposal Method, 0 to 7.
    pub disposal: u8,
    /// How many of its pixels are the transparent index.
    pub transparent: usize,
    pub interlaced: bool,
    /// `w * h` RGBA pixels, row by row.
    pub rgba: Vec<u8>,
}

pub struct Gif {
    pub width: usize,
    pub height: usize,
    /// The NETSCAPE2.0 loop count, zero for ever, when there is one.
    pub loops: Option<u16>,
    pub images: Vec<Image>,
}

fn le(d: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([d[at], d[at + 1]]))
}

/// The data sub-blocks from `at`, joined, and where they end.
fn blocks(d: &[u8], mut at: usize) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    loop {
        let n = usize::from(d[at]);
        at += 1;
        if n == 0 {
            return (out, at);
        }
        out.extend_from_slice(&d[at..at + n]);
        at += n;
    }
}

/// **Appendix F's decompressor**: codes of `m + 1` bits read right to
/// left, the width one more once the table's next code needs it, at most
/// twelve; a Clear resets the table, End of Information ends it. Each
/// string in the table is the string of its prefix code and one byte.
fn lzw(data: &[u8], m: u8, n: usize) -> Vec<u8> {
    let clear = 1usize << m;
    let eoi = clear + 1;
    // A code's prefix code, last byte, first byte and length.
    let mut prefix = [0usize; 4096];
    let mut last = [0u8; 4096];
    let mut first = [0u8; 4096];
    let mut len = [0usize; 4096];
    for i in 0..clear {
        last[i] = u8::try_from(i).unwrap();
        first[i] = last[i];
        len[i] = 1;
    }
    let mut next = clear + 2;
    let mut width = usize::from(m) + 1;
    let mut prev: Option<usize> = None;
    let mut out = Vec::with_capacity(n);
    let (mut acc, mut bits, mut at) = (0u32, 0usize, 0usize);
    while out.len() < n {
        while bits < width {
            if at == data.len() {
                return out;
            }
            acc |= u32::from(data[at]) << bits;
            at += 1;
            bits += 8;
        }
        let code = usize::try_from(acc & ((1 << width) - 1)).unwrap();
        acc >>= width;
        bits -= width;
        if code == clear {
            next = clear + 2;
            width = usize::from(m) + 1;
            prev = None;
            continue;
        }
        if code == eoi {
            break;
        }
        if let Some(p) = prev {
            if next < 4096 {
                // The new string is the previous one and the first byte of
                // this code's -- of the previous one's when this code is
                // the one being made.
                let head = match code < next {
                    true => first[code],
                    false => first[p],
                };
                prefix[next] = p;
                last[next] = head;
                first[next] = first[p];
                len[next] = len[p] + 1;
                next += 1;
            }
        }
        let start = out.len();
        out.resize(start + len[code], 0);
        let mut c = code;
        for k in (start..out.len()).rev() {
            out[k] = last[c];
            c = prefix[c];
        }
        prev = Some(code);
        if next == 1 << width && width < 12 {
            width += 1;
        }
    }
    out.truncate(n);
    out
}

pub fn read(d: &[u8]) -> Gif {
    assert_eq!(&d[..6], b"GIF89a");
    let (width, height) = (le(d, 6), le(d, 8));
    let packed = d[10];
    let table = |p: u8| match p & 0x80 {
        0 => 0,
        _ => 3 << ((p & 7) + 1),
    };
    let global = d[13..13 + table(packed)].to_vec();
    let mut at = 13 + table(packed);
    let mut loops = None;
    let mut control: Option<(u16, u8, Option<u8>)> = None;
    let mut images = Vec::new();
    loop {
        match d[at] {
            0x21 => {
                let label = d[at + 1];
                let (b, next) = blocks(d, at + 2);
                match label {
                    0xF9 => {
                        let transparent = (b[0] & 1 == 1).then_some(b[3]);
                        control = Some((
                            u16::from_le_bytes([b[1], b[2]]),
                            (b[0] >> 2) & 7,
                            transparent,
                        ));
                    }
                    0xFF if b.starts_with(b"NETSCAPE2.0") => {
                        loops = Some(u16::from_le_bytes([b[12], b[13]]));
                    }
                    _ => {}
                }
                at = next;
            }
            0x2C => {
                let (x, y, w, h) = (le(d, at + 1), le(d, at + 3), le(d, at + 5), le(d, at + 7));
                let p = d[at + 9];
                at += 10;
                let local = d[at..at + table(p)].to_vec();
                at += table(p);
                let colours = match local.is_empty() {
                    true => &global,
                    false => &local,
                };
                let m = d[at];
                let (data, next) = blocks(d, at + 1);
                at = next;
                let (delay_cs, disposal, key) = control.take().unwrap_or((0, 0, None));
                let indices = lzw(&data, m, w * h);
                assert_eq!(
                    indices.len(),
                    w * h,
                    "image {} decodes whole",
                    images.len() + 1
                );
                let mut rgba = Vec::with_capacity(w * h * 4);
                let mut transparent = 0;
                for &i in &indices {
                    match Some(i) == key {
                        true => {
                            transparent += 1;
                            rgba.extend_from_slice(&[0, 0, 0, 0]);
                        }
                        false => {
                            let c = 3 * usize::from(i);
                            rgba.extend_from_slice(&[
                                colours[c],
                                colours[c + 1],
                                colours[c + 2],
                                255,
                            ]);
                        }
                    }
                }
                images.push(Image {
                    x,
                    y,
                    w,
                    h,
                    delay_cs,
                    disposal,
                    transparent,
                    interlaced: p & 0x40 != 0,
                    rgba,
                });
            }
            0x3B => break,
            b => panic!("no block starts {b:#x} at {at}"),
        }
    }
    Gif {
        width,
        height,
        loops,
        images,
    }
}
