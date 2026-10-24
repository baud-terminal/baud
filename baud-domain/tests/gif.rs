//! The GIF decoder against `[[source: gif89a]]`.
//!
//! **A real encoder's files, and this file's own.** ImageMagick 7.1.2's
//! encoder wrote three files whose images cover their logical screens --
//! 256 colours of noise, whose table fills and clears; an interlaced
//! scene; a two-colour one at the minimum code size of 2 -- and its own
//! decode of each is the reference (`baud-domain/tests/gif/README.md`):
//! GIF is lossless, so any decoder that follows Appendix F gets the same
//! indices. The rest are written here by an LZW encoder of Appendix F's
//! own, independent of the decoder, so each block's meaning can be set
//! case by case: the background colour, the disposals, transparency,
//! local tables, interlace, zero delays, the loop count, the User Input
//! Flag, a Plain Text Extension, a stream cut short.
//!
//! **Malformed input never panics.** The property harms valid streams by
//! named classes -- a minimum code size outside 2 to 8, a table filled
//! with no Clear, a first code that is no Clear, an image past the
//! logical screen, a sub-block cut short, a size at 65535, the stream cut
//! at every block -- and a positive control requires each to fire.

use baud_domain::gif::{Gif, GifError, GifWait};
use proptest::prelude::*;
use std::collections::HashMap;

/// How an encoder meets a full table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Full {
    /// It sends a Clear.
    Clear,
    /// It keeps sending codes with the table full, "deferred clear".
    Defer,
}

/// **Appendix F's compressor**: the Clear code first, the string table
/// grown a code at a time, each code `width` bits written right to left,
/// the width one more once the table's next code would not fit, at most
/// twelve; the End of Information code last.
fn lzw(indices: &[u8], m: u8, full: Full) -> Vec<u8> {
    let clear: u16 = 1 << m;
    let eoi = clear + 1;
    let mut out = Vec::new();
    let (mut acc, mut n) = (0u32, 0u32);
    let mut width = u32::from(m) + 1;
    let mut put = |c: u16, width: u32| {
        acc |= u32::from(c) << n;
        n += width;
        while n >= 8 {
            out.push(acc.to_le_bytes()[0]);
            acc >>= 8;
            n -= 8;
        }
    };
    put(clear, width);
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = clear + 2;
    let mut w = u16::from(indices[0]);
    for &k in &indices[1..] {
        if let Some(&c) = dict.get(&(w, k)) {
            w = c;
            continue;
        }
        put(w, width);
        if next < 4096 {
            dict.insert((w, k), next);
            next += 1;
            if u32::from(next) > 1 << width && width < 12 {
                width += 1;
            }
        } else if full == Full::Clear {
            put(clear, width);
            dict.clear();
            next = clear + 2;
            width = u32::from(m) + 1;
        }
        w = u16::from(k);
    }
    put(w, width);
    put(eoi, width);
    if n > 0 {
        out.push(acc.to_le_bytes()[0]);
    }
    out
}

/// Data in sub-blocks of at most 255 bytes and the terminator.
fn blocks(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(255) {
        out.push(u8::try_from(c.len()).unwrap());
        out.extend_from_slice(c);
    }
    out.push(0);
    out
}

/// One graphic of a stream this file writes.
#[derive(Clone, Default)]
struct Img {
    rect: [u16; 4],
    px: Vec<u8>,
    /// (disposal, delay, transparency index, user input).
    gce: Option<(u8, u16, Option<u8>, bool)>,
    local: Option<Vec<[u8; 3]>>,
    rows: Rows,
}

/// Whether an image is written in Appendix E's passes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Rows {
    #[default]
    InOrder,
    Interlaced,
}

/// A colour table padded to a power of two, and its size field.
fn palette(t: &[[u8; 3]]) -> (Vec<u8>, u8) {
    let bits = (t.len().max(2) - 1).ilog2() + 1;
    let size = 1usize << bits;
    let mut out: Vec<u8> = t.iter().flatten().copied().collect();
    out.resize(size * 3, 0);
    (out, u8::try_from(bits - 1).unwrap())
}

const K: [u8; 3] = [0, 0, 0];
const W: [u8; 3] = [255, 255, 255];
const R: [u8; 3] = [255, 0, 0];
const G: [u8; 3] = [0, 255, 0];
const B: [u8; 3] = [0, 0, 255];

/// **A GIF89a stream**: the screen `w` by `h`, the global table and
/// background index, a NETSCAPE2.0 block with `loops` if given, the images.
fn gif(w: u16, h: u16, gct: &[[u8; 3]], bg: u8, loops: Option<u16>, images: &[Img]) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    let (table, size) = palette(gct);
    out.extend([0x80 | 0x70 | size, bg, 0]);
    out.extend(table);
    if let Some(n) = loops {
        out.extend(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
        out.extend(n.to_le_bytes());
        out.push(0);
    }
    for im in images {
        if let Some((disposal, delay, t, input)) = im.gce {
            let flags = disposal << 2 | u8::from(input) << 1 | u8::from(t.is_some());
            out.extend([0x21, 0xf9, 4, flags]);
            out.extend(delay.to_le_bytes());
            out.extend([t.unwrap_or(0), 0]);
        }
        out.push(0x2c);
        for v in im.rect {
            out.extend(v.to_le_bytes());
        }
        let local = im.local.as_ref().map(|t| palette(t));
        let flags = local.as_ref().map_or(0, |(_, s)| 0x80 | s)
            | u8::from(im.rows == Rows::Interlaced) << 6;
        out.push(flags);
        if let Some((t, _)) = &local {
            out.extend(t);
        }
        let m = 2.max(local.as_ref().map_or(size, |(_, s)| *s) + 1);
        out.push(m);
        out.extend(blocks(&lzw(&im.px, m, Full::Clear)));
    }
    out.push(0x3b);
    out
}

/// Every frame `g` decodes to: its RGBA bytes, gap and wait.
fn frames(g: &[u8]) -> Result<Vec<(Vec<u8>, u32, GifWait)>, GifError> {
    let gif = Gif::parse(g)?;
    let mut scratch = vec![0x5au8; gif.scratch_len()];
    let mut out = Vec::new();
    gif.decode(&mut scratch, |f| {
        out.push((f.rgba.to_vec(), f.gap_ms, f.wait));
        true
    })?;
    assert_eq!(
        u32::try_from(out.len()).unwrap(),
        gif.frames(),
        "the frames parse counted"
    );
    Ok(out)
}

/// RGBA of opaque colours.
fn rgba(px: &[[u8; 3]]) -> Vec<u8> {
    px.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect()
}

/// **A real encoder's streams decode to its own decode**: a table that
/// fills and clears, interlace, the minimum code size of 2.
#[test]
fn imagemagicks_streams_decode_to_its_reference() {
    let cases: [(&str, &[u8], &[u8]); 3] = [
        (
            "noise256",
            include_bytes!("gif/noise256.gif"),
            include_bytes!("gif/noise256.rgba"),
        ),
        (
            "scene64i",
            include_bytes!("gif/scene64i.gif"),
            include_bytes!("gif/scene64i.rgba"),
        ),
        (
            "scene2",
            include_bytes!("gif/scene2.gif"),
            include_bytes!("gif/scene2.rgba"),
        ),
    ];
    for (name, g, want) in cases {
        let f = frames(g).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(f.len(), 1, "{name}: one frame");
        let wrong = f[0].0.iter().zip(want).filter(|(a, b)| a != b).count();
        assert_eq!(wrong, 0, "{name}: bytes that differ");
    }
}

/// **Codes past twelve bits' table**: the encoder keeps sending with the
/// table full ("deferred clear"), which adds no entry, and both a Clear at
/// full and the deferred form decode to the indices sent.
#[test]
fn a_full_table_decodes_with_and_without_a_clear() {
    // A long run of noise, more than 4096 strings.
    let mut x = 7u32;
    let px: Vec<u8> = (0..120 * 100)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()[0] & 15
        })
        .collect();
    let gct: Vec<[u8; 3]> = (0..16u8).map(|i| [i * 16, 255 - i * 16, i]).collect();
    let want = rgba(&px.iter().map(|&i| gct[usize::from(i)]).collect::<Vec<_>>());
    for full in [Full::Clear, Full::Defer] {
        let mut g = gif(120, 100, &gct, 0, None, &[]);
        g.pop();
        g.push(0x2c);
        for v in [0u16, 0, 120, 100] {
            g.extend(v.to_le_bytes());
        }
        g.extend([0, 4]);
        g.extend(blocks(&lzw(&px, 4, full)));
        g.push(0x3b);
        let f = frames(&g).unwrap();
        assert!(f[0].0 == want, "{full:?}: the indices sent");
    }
}

/// **"The Background Color is the color used for those pixels on the
/// screen that are not covered by an image"**, and disposal 2 restores it
/// (Jev's lean, SPEC_BACKGROUND); with no global table it is clear.
#[test]
fn the_background_colour_shows_where_no_image_covers() {
    let one = |x, y, v, gce| Img {
        rect: [x, y, 1, 1],
        px: vec![v],
        gce: Some(gce),
        ..Img::default()
    };
    // Background index 2, red: a white pixel, disposed to the background,
    // then a green one.
    let g = gif(
        2,
        2,
        &[K, W, R, G],
        2,
        Some(0),
        &[
            one(0, 0, 1, (2, 50, None, false)),
            one(1, 1, 3, (2, 50, None, false)),
        ],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].0, rgba(&[W, R, R, R]), "the screen red round it");
    assert_eq!(f[1].0, rgba(&[R, R, R, G]), "disposal 2 restored red");
    assert_eq!((f[0].1, f[1].1), (500, 500), "50 hundredths");
    // **Transparent pixels over nothing show the background colour**: the
    // index leaves "the corresponding pixel of the display device ... not
    // modified", and the screen there is the Background Color, so a
    // transparent logo is an opaque box of it, where macOS shows clear.
    let logo = Img {
        rect: [0, 0, 2, 1],
        px: vec![0, 1],
        gce: Some((1, 0, Some(0), false)),
        ..Img::default()
    };
    let f = frames(&gif(2, 1, &[K, W, R, G], 2, None, &[logo])).unwrap();
    assert_eq!(f[0].0, rgba(&[R, W]), "red where the logo is transparent");
    // With no global table the background is clear.
    let mut g = b"GIF89a\x02\x00\x02\x00\x00\x00\x00".to_vec();
    g.extend(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x80");
    g.extend([0, 0, 0, 255, 255, 255, 2]);
    g.extend(blocks(&lzw(&[1], 2, Full::Clear)));
    g.push(0x3b);
    let f = frames(&g).unwrap();
    assert_eq!(f[0].0[..4], [255, 255, 255, 255]);
    assert_eq!(f[0].0[4..], [0; 12], "clear where nothing covers");
}

/// **Disposal 3, "Restore to previous"**, and the transparency index,
/// which leaves "the corresponding pixel of the display device ... not
/// modified".
#[test]
fn disposal_restores_and_transparency_leaves_the_pixel() {
    let full = Img {
        rect: [0, 0, 2, 2],
        px: vec![3, 3, 3, 3],
        gce: Some((1, 50, None, false)),
        ..Img::default()
    };
    let over = Img {
        rect: [0, 0, 2, 1],
        // Index 0 is transparent: the left pixel keeps the green.
        px: vec![0, 1],
        gce: Some((3, 50, Some(0), false)),
        ..Img::default()
    };
    let last = Img {
        rect: [1, 1, 1, 1],
        px: vec![2],
        gce: Some((0, 50, None, false)),
        ..Img::default()
    };
    let g = gif(2, 2, &[K, W, R, G], 0, None, &[full, over, last]);
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!(f[0].0, rgba(&[G, G, G, G]));
    assert_eq!(f[1].0, rgba(&[G, W, G, G]), "transparent left, white right");
    assert_eq!(f[2].0, rgba(&[G, G, G, R]), "the white restored to green");
}

/// **A local table, an interlaced image and an image past the screen**:
/// the local table colours its own image alone, Appendix E's rows land in
/// order, and what falls off the logical screen is dropped.
#[test]
fn local_tables_interlace_and_clipping() {
    let px: Vec<u8> = (0..9u8).map(|y| y % 4).collect();
    let tall = Img {
        rect: [0, 0, 1, 9],
        px: px.clone(),
        rows: Rows::Interlaced,
        local: Some(vec![K, W, R, B]),
        gce: Some((1, 10, None, false)),
    };
    let g = gif(2, 9, &[G, G], 0, None, &[tall]);
    let f = frames(&g).unwrap();
    // Rows sent in the order 0, 8, 4, 2, 6, 1, 3, 5, 7.
    let order = [0usize, 8, 4, 2, 6, 1, 3, 5, 7];
    let table = [K, W, R, B];
    for (sent, &row) in order.iter().enumerate() {
        let want = table[usize::from(px[sent])];
        assert_eq!(f[0].0[row * 8..row * 8 + 3], want, "row {row}");
        assert_eq!(f[0].0[row * 8 + 4..row * 8 + 7], G, "the right column");
    }
    let off = Img {
        rect: [1, 1, 3, 3],
        px: vec![1; 9],
        gce: Some((1, 10, None, false)),
        ..Img::default()
    };
    let f = frames(&gif(2, 2, &[K, W], 0, None, &[off])).unwrap();
    assert_eq!(f[0].0, rgba(&[K, K, K, W]), "one pixel on the screen");
}

/// **"If not 0 ... hundredths of a second to wait"**: an image with delay
/// 0 is composited into the next and never shown alone, and a stream
/// that ends on one ends showing it; a stream whose every delay is 0 is
/// one still picture, every image composited.
#[test]
fn a_zero_delay_composites_and_all_zero_is_a_still() {
    let one = |x, v, delay| Img {
        rect: [x, 0, 1, 1],
        px: vec![v],
        gce: Some((1, delay, None, false)),
        ..Img::default()
    };
    let g = gif(
        3,
        1,
        &[K, W, R, G],
        0,
        Some(0),
        &[one(0, 1, 0), one(1, 2, 40), one(2, 3, 0)],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2, "the first composited into the second");
    assert_eq!(f[0].0, rgba(&[W, R, K]));
    assert_eq!(f[0].1, 400);
    assert_eq!(f[1].0, rgba(&[W, R, G]), "the stream ends on its last");
    assert_eq!(f[1].1, 0, "gapless");
    let g = gif(
        2,
        1,
        &[K, W, R, G],
        0,
        Some(0),
        &[one(0, 1, 0), one(1, 2, 0)],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 1, "no wait anywhere");
    assert_eq!(f[0].0, rgba(&[W, R]), "both composited");
    assert_eq!(f[0].1, 0, "gapless");
    // A still picture with no Graphic Control Extension.
    let still = Img {
        rect: [0, 0, 1, 1],
        px: vec![1],
        ..Img::default()
    };
    assert_eq!(
        frames(&gif(1, 1, &[K, W], 0, None, &[still]))
            .unwrap()
            .len(),
        1
    );
}

/// **The loop count**: NETSCAPE2.0's 0 for ever, `n` plays `n + 1`
/// times, and with no such block the stream plays once (Jev,
/// NETSCAPE_COUNT).
#[test]
fn the_loop_count_is_netscapes() {
    let im = Img {
        rect: [0, 0, 1, 1],
        px: vec![1],
        gce: Some((0, 10, None, false)),
        ..Img::default()
    };
    let two = [im.clone(), im];
    for (loops, plays) in [(None, 1), (Some(0), 0), (Some(3), 4), (Some(65535), 65536)] {
        let g = gif(1, 1, &[K, W], 0, loops, &two);
        assert_eq!(Gif::parse(&g).unwrap().plays(), plays, "{loops:?}");
    }
}

/// **The User Input Flag** marks its frame as waiting for input, delay or
/// none: with none it is shown, not composited away.
#[test]
fn a_frame_with_the_user_input_flag_waits() {
    let one = |x, v, gce| Img {
        rect: [x, 0, 1, 1],
        px: vec![v],
        gce: Some(gce),
        ..Img::default()
    };
    let g = gif(
        2,
        1,
        &[K, W, R, G],
        0,
        None,
        &[
            one(0, 1, (1, 0, None, true)),
            one(1, 2, (1, 30, None, false)),
        ],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].1, f[0].2), (0, GifWait::Input));
    assert_eq!((f[1].1, f[1].2), (300, GifWait::Delay));
}

/// **A Plain Text Extension is not drawn, and is counted**; its Graphic
/// Control Extension still delays it, so the image after it is a frame
/// of its own.
#[test]
fn a_plain_text_extension_is_counted_not_drawn() {
    let mut g = gif(2, 1, &[K, W], 0, None, &[]);
    g.pop();
    g.extend([0x21, 0xf9, 4, 4, 20, 0, 0, 0]);
    g.extend([0x21, 0x01, 12, 0, 0, 0, 0, 2, 0, 1, 0, 8, 8, 1, 0]);
    g.extend(blocks(b"hi"));
    g.extend(b"\x2c\x01\x00\x00\x00\x01\x00\x01\x00\x00\x02");
    g.extend(blocks(&lzw(&[1], 2, Full::Clear)));
    g.push(0x3b);
    let gif = Gif::parse(&g).unwrap();
    assert_eq!(gif.texts_not_drawn(), 1);
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2, "the text's frame and the image's");
    assert_eq!(f[0].0, rgba(&[K, K]), "no text drawn");
    assert_eq!(f[1].0, rgba(&[K, W]));
}

/// **A stream cut short shows what arrived**: an image whose data stops
/// keeps the canvas where its pixels never came.
#[test]
fn a_stream_cut_short_shows_what_arrived() {
    let px: Vec<u8> = (0..64u8).map(|i| 1 + i % 2).collect();
    let im = Img {
        rect: [0, 0, 8, 8],
        px,
        ..Img::default()
    };
    let g = gif(8, 8, &[K, W, R], 0, None, &[im]);
    let whole = frames(&g).unwrap();
    let cut = frames(&g[..g.len() - 8]).unwrap();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].0[..32], whole[0].0[..32], "the first row arrived");
    assert_ne!(cut[0].0, whole[0].0);
    assert_eq!(
        cut[0].0[252..],
        [0, 0, 0, 255],
        "the last pixel is the background"
    );
}

/// The ways a stream is harmed, each a class the property names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Harm {
    /// An image's LZW minimum code size set to anything.
    CodeSize,
    /// An image's data replaced by codes that fill the table with no Clear.
    NoClear,
    /// An image's data starting with a code that is no Clear.
    FirstNotClear,
    /// An image moved past the logical screen.
    OffScreen,
    /// A sub-block's length made longer than what follows.
    SubBlock,
    /// The logical screen at 65535 by 65535.
    Size,
    /// The stream cut at a block.
    Cut,
    /// A byte anywhere set to anything.
    Byte,
}

const HARMS: [Harm; 8] = [
    Harm::CodeSize,
    Harm::NoClear,
    Harm::FirstNotClear,
    Harm::OffScreen,
    Harm::SubBlock,
    Harm::Size,
    Harm::Cut,
    Harm::Byte,
];

/// The positions of each image descriptor in `g`, found by walking.
fn images(g: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut pos = 13
        + match g.get(10) {
            Some(f) if f & 0x80 != 0 => 3 * (2usize << (f & 7)),
            _ => 0,
        };
    while let Some(&b) = g.get(pos) {
        match b {
            0x21 => {
                pos += 2;
                while let Some(&n) = g.get(pos) {
                    pos += 1 + usize::from(n);
                    if n == 0 {
                        break;
                    }
                }
            }
            0x2c => {
                out.push(pos);
                let f = g.get(pos + 9).copied().unwrap_or(0);
                pos += 10
                    + match f & 0x80 {
                        0 => 0,
                        _ => 3 * (2usize << (f & 7)),
                    };
                pos += 1;
                while let Some(&n) = g.get(pos) {
                    pos += 1 + usize::from(n);
                    if n == 0 {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    out
}

/// Where image `i`'s minimum code size is.
fn data_of(g: &[u8], i: usize) -> usize {
    let f = g.get(i + 9).copied().unwrap_or(0);
    i + 10
        + match f & 0x80 {
            0 => 0,
            _ => 3 * (2usize << (f & 7)),
        }
}

fn harmed(g: &[u8], harm: Harm, pick: usize, v: u8) -> (Vec<u8>, bool) {
    let mut g = g.to_vec();
    let ims = images(&g);
    let at = (!ims.is_empty()).then(|| ims[pick % ims.len()]);
    let applied = match harm {
        Harm::CodeSize => at.map(|i| {
            let d = data_of(&g, i);
            if d < g.len() {
                g[d] = v;
            }
        }),
        Harm::NoClear | Harm::FirstNotClear => at.map(|i| {
            let d = data_of(&g, i);
            if d + 1 < g.len() {
                // Codes 2, 3, ... at a width that grows to twelve: a table
                // filled past 4096 with no Clear, or one begun without it.
                let m = 2u8;
                let mut codes: Vec<u8> = Vec::new();
                let (mut acc, mut n, mut width, mut next) = (0u32, 0u32, 3u32, 6u32);
                let start = u32::from(harm == Harm::FirstNotClear);
                for k in 0..5000u32 {
                    let c = match (k, start) {
                        (0, 0) => 4,
                        _ => (k + u32::from(v)) % next.min(4096),
                    };
                    acc |= c << n;
                    n += width;
                    while n >= 8 {
                        codes.push(acc.to_le_bytes()[0]);
                        acc >>= 8;
                        n -= 8;
                    }
                    if k > 0 && next < 4096 {
                        next += 1;
                        if next == 1 << width && width < 12 {
                            width += 1;
                        }
                    }
                }
                let tail = g.split_off(d);
                let rest = tail
                    .iter()
                    .position(|&b| b == 0x3b)
                    .map_or(&[][..], |p| &tail[p..]);
                g.push(m);
                g.extend(blocks(&codes));
                g.extend_from_slice(rest);
            }
        }),
        Harm::OffScreen => at.map(|i| {
            if i + 4 < g.len() {
                g[i + 1] = v;
                g[i + 2] = 0xff;
                g[i + 3] = v.rotate_left(1);
                g[i + 4] = 0xfe;
            }
        }),
        Harm::SubBlock => at.map(|i| {
            let d = data_of(&g, i);
            if d + 1 < g.len() {
                g[d + 1] = v | 0x80;
            }
        }),
        Harm::Size => (g.len() > 10).then(|| {
            for b in &mut g[6..10] {
                *b = 0xff;
            }
        }),
        Harm::Cut => at.map(|i| g.truncate(i + usize::from(v % 16))),
        Harm::Byte => (!g.is_empty()).then(|| {
            let i = pick % g.len();
            g[i] = v;
        }),
    };
    (g, applied.is_some())
}

/// What any decode may cost: the scratch it asks for is the screen's, and
/// the decode returns, whatever it returns.
fn survives(g: &[u8]) {
    let Ok(gif) = Gif::parse(g) else {
        return;
    };
    let need = gif.scratch_len();
    if need > 1 << 26 {
        return;
    }
    let mut scratch = vec![0u8; need];
    let mut n = 0u32;
    let _ = gif.decode(&mut scratch, |f| {
        assert_eq!(f.rgba.len(), need / 2, "a frame is the screen");
        n += 1;
        true
    });
    assert!(n <= gif.frames(), "no more frames than counted");
    let mut short = vec![0u8; need - 1];
    assert_eq!(gif.decode(&mut short, |_| true), Err(GifError::Buffer));
}

fn seeds() -> Vec<Vec<u8>> {
    let one = |x, v, gce| Img {
        rect: [x, 0, 2, 2],
        px: vec![v; 4],
        gce: Some(gce),
        ..Img::default()
    };
    vec![
        include_bytes!("gif/scene64i.gif").to_vec(),
        include_bytes!("gif/scene2.gif").to_vec(),
        gif(
            4,
            2,
            &[K, W, R, G],
            1,
            Some(2),
            &[
                one(0, 1, (2, 10, Some(0), false)),
                one(1, 2, (3, 0, None, true)),
                one(2, 3, (1, 20, Some(3), false)),
            ],
        ),
    ]
}

proptest! {
    #[test]
    fn a_malformed_stream_never_panics(
        seed in 0..3usize,
        harms in proptest::collection::vec((0..HARMS.len(), any::<usize>(), any::<u8>()), 1..4),
    ) {
        let mut g = seeds()[seed].clone();
        for (h, pick, v) in harms {
            g = harmed(&g, HARMS[h], pick, v).0;
        }
        survives(&g);
    }
}

/// **The positive control**: every class applies to some seed and makes a
/// stream that differs from it.
#[test]
fn every_harm_class_is_generated() {
    for harm in HARMS {
        let fired = seeds().iter().any(|s| {
            (0..8).any(|pick| {
                let (g, applied) = harmed(s, harm, pick, 0x5a);
                applied && g != *s
            })
        });
        assert!(fired, "{harm:?} never changes a stream");
    }
}
