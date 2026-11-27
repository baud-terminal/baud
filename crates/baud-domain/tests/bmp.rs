//! The bitmap decoder against Microsoft's bitmap documentation
//! (`[[source: ms-bitmap-storage]]`, `[[source: ms-bitmapinfoheader]]`,
//! `[[source: ms-bitmapv5header]]`, `[[source: ms-bitmap-compression]]`).
//!
//! **The text's own examples first**: the `BI_RLE8` and `BI_RLE4` streams
//! the compression page expands, pixel for pixel. The rest are files this
//! file writes field by field, independent of the decoder, one for each
//! depth, compression, header and row order, each decoded to the pixels its
//! bytes name.
//!
//! **Malformed input never panics, and is refused.** Every file here is
//! cut at every byte and harmed byte by byte; a cut in the headers or the
//! colour table is refused, a cut in the pixels shows what arrived.

use baud_domain::bmp::{Bmp, BmpError};
use baud_domain::{Chan, Pixel};

/// A field of a header: a WORD or a DWORD, little-endian.
#[derive(Clone, Copy)]
enum F {
    W(i64),
    D(i64),
}

use F::{D, W};

fn le(fields: &[F]) -> Vec<u8> {
    let mut v = Vec::new();
    for f in fields {
        match *f {
            W(x) => v.extend(u16::try_from(x & 0xFFFF).unwrap().to_le_bytes()),
            D(x) => v.extend(u32::try_from(x & 0xFFFF_FFFF).unwrap().to_le_bytes()),
        }
    }
    v
}

fn len(v: &[u8]) -> i64 {
    i64::try_from(v.len()).unwrap()
}

/// **A bitmap header's fields**: its size (40, 108 or 124), the sides, the
/// bit count, the compression, `biClrUsed`, and the red, green, blue and
/// alpha masks.
#[derive(Clone, Copy)]
struct Info {
    size: i64,
    width: i64,
    height: i64,
    bits: i64,
    compression: i64,
    used: i64,
    masks: [i64; 4],
}

const INFO: Info = Info {
    size: 40,
    width: 3,
    height: 2,
    bits: 24,
    compression: 0,
    used: 0,
    masks: [0; 4],
};

/// The file header before `header`, `table` and `data`, `bfOffBits` at
/// `data`.
fn wrap(header: &[u8], table: &[u8], data: &[u8]) -> Vec<u8> {
    let off = 14 + len(header) + len(table);
    let mut f = b"BM".to_vec();
    f.extend(le(&[D(off + len(data)), W(0), W(0), D(off)]));
    f.extend(header);
    f.extend(table);
    f.extend(data);
    f
}

/// **A file of `h`'s header**, the masks after a 40-byte header when it is
/// `BI_BITFIELDS` and inside a V4 or V5 one, then `table` and `data`.
fn file(h: Info, table: &[u8], data: &[u8]) -> Vec<u8> {
    let image = match h.compression {
        1 | 2 => len(data),
        _ => 0,
    };
    let mut head = le(&[
        D(h.size),
        D(h.width),
        D(h.height),
        W(1),
        W(h.bits),
        D(h.compression),
        D(image),
        D(2835),
        D(2835),
        D(h.used),
        D(0),
    ]);
    let [r, g, b, a] = h.masks;
    match h.size {
        40 if h.compression == 3 => head.extend(le(&[D(r), D(g), D(b)])),
        40 => {}
        _ => {
            // The masks, then LCS_sRGB, 's' 'R' 'G' 'B'; the endpoints,
            // gammas and the V5 fields zero.
            head.extend(le(&[D(r), D(g), D(b), D(a), D(0x7352_4742)]));
            head.resize(usize::try_from(h.size).unwrap(), 0);
        }
    }
    wrap(&head, table, data)
}

/// A colour table of RGBQUADs, each `[r, g, b]` as blue, green, red and a
/// zero.
fn quads(colours: &[[u8; 3]]) -> Vec<u8> {
    colours.iter().flat_map(|&[r, g, b]| [b, g, r, 0]).collect()
}

/// The words of `[r, g, b]`, opaque.
fn opaque(colours: &[[u8; 3]]) -> Vec<u32> {
    colours
        .iter()
        .map(|&[r, g, b]| u32::from_be_bytes([0xFF, r, g, b]))
        .collect()
}

/// The picture's words, row by row from the top.
fn decode(f: &[u8]) -> Result<Vec<u32>, BmpError> {
    let bmp = Bmp::parse(f)?;
    let s = bmp.size();
    let area = usize::try_from(s.width * s.height).unwrap();
    // Words the decoder must overwrite, clear where nothing reaches.
    let c = Chan::from_u8;
    let mut out = vec![Pixel::opaque(c(0xDE), c(0xAD), c(0xBE)); area];
    bmp.decode(&mut out)?;
    Ok(out.iter().map(|p| p.bits()).collect())
}

const K: [u8; 3] = [0, 0, 0];
const WH: [u8; 3] = [255, 255, 255];
const RD: [u8; 3] = [255, 0, 0];
const GN: [u8; 3] = [0, 255, 0];
const BL: [u8; 3] = [0, 0, 255];
const YL: [u8; 3] = [255, 255, 0];

/// A 256-entry table whose entry `i` is red `i`, green `255 - i`, blue
/// `i` with its bits turned.
fn ramp() -> Vec<[u8; 3]> {
    (0..=255u8).map(|i| [i, 255 - i, !i]).collect()
}

const CLEAR: u32 = 0;

/// **The `BI_RLE8` example, as the compression page expands it**: "04 04
/// 04", "06 06 06 06 06", "45 56 67", "78 78", "move current position 5
/// right and 1 up", "78 78", "end of line", nine "1E", "end of RLE
/// bitmap". The first row is the bottom one; every pixel no run reaches is
/// clear.
#[test]
fn the_rle8_example_expands_as_the_text_shows() {
    let data = [
        0x03, 0x04, 0x05, 0x06, 0x00, 0x03, 0x45, 0x56, 0x67, 0x00, 0x02, 0x78, 0x00, 0x02, 0x05,
        0x01, 0x02, 0x78, 0x00, 0x00, 0x09, 0x1E, 0x00, 0x01,
    ];
    let table = ramp();
    let h = Info {
        width: 20,
        height: 3,
        bits: 8,
        compression: 1,
        ..INFO
    };
    let got = decode(&file(h, &quads(&table), &data)).unwrap();
    let c = |i: usize| opaque(&table[i..=i])[0];
    let mut want = vec![CLEAR; 60];
    // The top row shown is the third coded.
    for px in &mut want[..9] {
        *px = c(0x1E);
    }
    want[20 + 18] = c(0x78);
    want[20 + 19] = c(0x78);
    let bottom = [
        0x04, 0x04, 0x04, 0x06, 0x06, 0x06, 0x06, 0x06, 0x45, 0x56, 0x67, 0x78, 0x78,
    ];
    for (x, i) in bottom.into_iter().enumerate() {
        want[40 + x] = c(i);
    }
    assert_eq!(got, want);
}

/// **The `BI_RLE4` example, as the compression page expands it**: "0 4 0",
/// "0 6 0 6 0", "4 5 5 6 6 7", "7 8 7 8", the same delta, "7 8 7 8", "end
/// of line", "1 E 1 E 1 E 1 E 1": the high four bits first, then the low,
/// in turn.
#[test]
fn the_rle4_example_expands_as_the_text_shows() {
    let data = [
        0x03, 0x04, 0x05, 0x06, 0x00, 0x06, 0x45, 0x56, 0x67, 0x00, 0x04, 0x78, 0x00, 0x02, 0x05,
        0x01, 0x04, 0x78, 0x00, 0x00, 0x09, 0x1E, 0x00, 0x01,
    ];
    let table = &ramp()[..16];
    let h = Info {
        width: 27,
        height: 3,
        bits: 4,
        compression: 2,
        ..INFO
    };
    let got = decode(&file(h, &quads(table), &data)).unwrap();
    let c = |i: usize| opaque(&table[i..=i])[0];
    let mut want = vec![CLEAR; 81];
    for (x, i) in [1, 14, 1, 14, 1, 14, 1, 14, 1].into_iter().enumerate() {
        want[x] = c(i);
    }
    for (x, i) in [7, 8, 7, 8].into_iter().enumerate() {
        want[27 + 23 + x] = c(i);
    }
    let bottom = [0, 4, 0, 0, 6, 0, 6, 0, 4, 5, 5, 6, 6, 7, 7, 8, 7, 8];
    for (x, i) in bottom.into_iter().enumerate() {
        want[54 + x] = c(i);
    }
    assert_eq!(got, want);
}

/// **A run past its row's end draws to the end and no further**, an
/// absolute run likewise, and the next row starts at its own left.
#[test]
fn a_run_past_the_row_is_cut_at_its_end() {
    let table = &ramp()[..4];
    let data = [
        0x05, 0x01, 0x00, 0x00, 0x00, 0x04, 0x02, 0x03, 0x02, 0x03, 0x00, 0x01,
    ];
    let h = Info {
        width: 3,
        height: 2,
        bits: 8,
        compression: 1,
        used: 4,
        ..INFO
    };
    let got = decode(&file(h, &quads(table), &data)).unwrap();
    assert_eq!(
        got,
        opaque(&[table[2], table[3], table[2], table[1], table[1], table[1]])
    );
}

/// **Each uncompressed depth**, bottom-up, its rows padded to DWORDs: 1 bit
/// (the first pixel in the byte's high bit), 4 (the high four bits first),
/// 8 with a table of `biClrUsed` entries, 16 as RGB 555, 24 as blue, green,
/// red, and 32 with its high byte unused.
#[test]
fn every_uncompressed_depth_decodes() {
    // 1 bit, 10 wide: 0b10110000 0b01000000 and two bytes of padding.
    let one = Info {
        width: 10,
        height: 1,
        bits: 1,
        ..INFO
    };
    let got = decode(&file(
        one,
        &quads(&[K, WH]),
        &[0b1011_0000, 0b0100_0000, 0, 0],
    ))
    .unwrap();
    assert_eq!(got, opaque(&[WH, K, WH, WH, K, K, K, K, K, WH]));
    // 4 bits: "0x1F" is the second entry, then the sixteenth.
    let mut table = vec![K; 16];
    table[1] = RD;
    table[15] = BL;
    let four = Info {
        width: 3,
        height: 2,
        bits: 4,
        ..INFO
    };
    let data = [0x1F, 0x10, 0, 0, 0xF1, 0xF0, 0, 0];
    let got = decode(&file(four, &quads(&table), &data)).unwrap();
    assert_eq!(got, opaque(&[BL, RD, BL, RD, BL, RD]));
    // 8 bits, a table of three.
    let eight = Info {
        width: 3,
        height: 2,
        bits: 8,
        used: 3,
        ..INFO
    };
    let data = [0, 1, 2, 0, 2, 1, 0, 0];
    let got = decode(&file(eight, &quads(&[RD, GN, BL]), &data)).unwrap();
    assert_eq!(got, opaque(&[BL, GN, RD, RD, GN, BL]));
    // 16 bits, RGB 555: 0x7C00 red, 0x03E0 green, 0x001F blue, the top bit
    // ignored; 0x0421 is one step of each, 8 at eight bits.
    let sixteen = Info {
        width: 3,
        height: 1,
        bits: 16,
        ..INFO
    };
    let data = le(&[W(0xFC00), W(0x03E0), W(0x0421), W(0)]);
    let got = decode(&file(sixteen, &[], &data)).unwrap();
    assert_eq!(got, opaque(&[RD, GN, [8, 8, 8]]));
    // 24 bits, three pixels of nine bytes padded to twelve, two rows.
    let data = [
        255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 255, 255, 0, 0, 0,
    ];
    let got = decode(&file(INFO, &[], &data)).unwrap();
    assert_eq!(got, opaque(&[K, WH, YL, BL, GN, RD]));
    // 32 bits: the high byte, whatever it holds, is not alpha.
    let thirty_two = Info {
        width: 2,
        height: 1,
        bits: 32,
        ..INFO
    };
    let got = decode(&file(thirty_two, &[], &[0, 0, 255, 0, 255, 0, 0, 0x80])).unwrap();
    assert_eq!(got, opaque(&[RD, BL]));
}

/// **A negative height is a top-down bitmap**: the first row stored is the
/// top one shown.
#[test]
fn a_negative_height_is_top_down() {
    let data = [
        255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0, 255, 255, 0, 0, 0,
    ];
    let down = Info { height: -2, ..INFO };
    let got = decode(&file(down, &[], &data)).unwrap();
    assert_eq!(got, opaque(&[BL, GN, RD, K, WH, YL]));
}

/// **`BI_BITFIELDS`**: three masks after a 40-byte header, here RGB 565,
/// a five-bit and a six-bit field scaled to the full range; and a V5
/// header's four, its alpha mask read, straight.
#[test]
fn bitfields_read_their_masks() {
    let h = Info {
        width: 3,
        height: 1,
        bits: 16,
        compression: 3,
        masks: [0xF800, 0x07E0, 0x001F, 0],
        ..INFO
    };
    let data = le(&[W(0xF800), W(0x07E0), W(0x0821), W(0)]);
    let got = decode(&file(h, &[], &data)).unwrap();
    assert_eq!(got, opaque(&[RD, GN, [8, 4, 8]]));
    // A V5 header with alpha in the low byte and blue in the high one.
    let v5 = Info {
        size: 124,
        width: 2,
        height: 1,
        bits: 32,
        compression: 3,
        masks: [0x0000_FF00, 0x00FF_0000, 0xFF00_0000, 0x0000_00FF],
        ..INFO
    };
    let data = le(&[D(0x30_20_10_80), D(0x00_00_FF_FF)]);
    let got = decode(&file(v5, &[], &data)).unwrap();
    assert_eq!(got, vec![0x80_10_20_30, 0xFF_FF_00_00]);
    // The same masks in a V4 header.
    let v4 = Info { size: 108, ..v5 };
    let got = decode(&file(v4, &[], &data)).unwrap();
    assert_eq!(got, vec![0x80_10_20_30, 0xFF_FF_00_00]);
    // A 40-byte header's three masks give no alpha: the low byte is unused.
    let info = Info {
        size: 40,
        masks: [0x0000_FF00, 0x00FF_0000, 0xFF00_0000, 0],
        ..v5
    };
    let got = decode(&file(info, &[], &data)).unwrap();
    assert_eq!(got, vec![0xFF_10_20_30, 0xFF_FF_00_00]);
}

/// **A BITMAPCOREHEADER**: unsigned word sides, bottom-up, and a table of
/// RGBTRIPLEs, as many as lie before `bfOffBits`.
#[test]
fn a_core_header_reads_its_triples() {
    let head = le(&[D(12), W(2), W(2), W(1), W(8)]);
    let table: Vec<u8> = [RD, GN, BL]
        .iter()
        .flat_map(|&[r, g, b]| [b, g, r])
        .collect();
    let got = decode(&wrap(&head, &table, &[0, 1, 0, 0, 2, 2, 0, 0])).unwrap();
    assert_eq!(got, opaque(&[BL, BL, RD, GN]));
    let head = le(&[D(12), W(1), W(1), W(1), W(24)]);
    let got = decode(&wrap(&head, &[], &[0x10, 0x20, 0x30, 0])).unwrap();
    assert_eq!(got, opaque(&[[0x30, 0x20, 0x10]]));
}

/// **What is refused, and why**, each against a file that decodes with the
/// one field changed: a positive control for every refusal.
#[test]
fn what_breaks_the_rules_is_refused() {
    let ok = |f: &[u8]| decode(f).is_ok();
    let good = file(INFO, &[], &[0; 24]);
    assert!(ok(&good), "the control");
    let with = |h: Info| decode(&file(h, &[], &[0; 24]));
    assert_eq!(decode(b"BX"), Err(BmpError::Malformed), "not BM");
    assert_eq!(with(Info { size: 64, ..INFO }), Err(BmpError::Unsupported));
    assert_eq!(with(Info { size: 56, ..INFO }), Err(BmpError::Unsupported));
    assert_eq!(with(Info { width: 0, ..INFO }), Err(BmpError::Malformed));
    assert_eq!(with(Info { width: -3, ..INFO }), Err(BmpError::Malformed));
    assert_eq!(with(Info { height: 0, ..INFO }), Err(BmpError::Malformed));
    assert_eq!(
        with(Info {
            width: 65536,
            ..INFO
        }),
        Err(BmpError::TooLarge)
    );
    assert_eq!(
        with(Info {
            height: -0x8000_0000,
            ..INFO
        }),
        Err(BmpError::TooLarge)
    );
    for c in [4, 5, 11, 12, 13] {
        let h = Info {
            compression: c,
            ..INFO
        };
        assert_eq!(with(h), Err(BmpError::Unsupported), "compression {c}");
    }
    for (bits, c) in [
        (24, 3),
        (8, 3),
        (8, 2),
        (4, 1),
        (24, 1),
        (0, 0),
        (2, 0),
        (7, 6),
    ] {
        let h = Info {
            bits,
            compression: c,
            ..INFO
        };
        assert_eq!(with(h), Err(BmpError::Malformed), "{bits} bits, {c}");
    }
    // Compressed bitmaps are bottom-up.
    let rle = Info {
        bits: 8,
        compression: 1,
        height: -2,
        used: 1,
        ..INFO
    };
    assert_eq!(
        decode(&file(rle, &quads(&[K]), &[0, 1])),
        Err(BmpError::Malformed)
    );
    let rle = Info { height: 2, ..rle };
    assert!(ok(&file(rle, &quads(&[K]), &[0, 1])), "the control");
    // A mask whose bits are not contiguous.
    let gap = Info {
        bits: 32,
        compression: 3,
        masks: [0x00FF_00FF, 0x0000_FF00, 0, 0],
        ..INFO
    };
    assert_eq!(with(gap), Err(BmpError::Malformed));
    // Two planes.
    let mut planes = good.clone();
    planes[26] = 2;
    assert_eq!(decode(&planes), Err(BmpError::Malformed));
    // A bfOffBits past the file.
    let mut off = good.clone();
    off[10] = 0xFF;
    assert_eq!(decode(&off), Err(BmpError::Malformed));
    // An index past a table of two.
    let short = Info {
        bits: 8,
        used: 2,
        ..INFO
    };
    assert!(ok(&file(short, &quads(&[K, WH]), &[1; 8])), "the control");
    let past = decode(&file(short, &quads(&[K, WH]), &[2; 8]));
    assert_eq!(past, Err(BmpError::Palette));
    let rle = Info {
        bits: 8,
        compression: 1,
        used: 2,
        ..INFO
    };
    assert_eq!(
        decode(&file(rle, &quads(&[K, WH]), &[1, 2, 0, 1])),
        Err(BmpError::Palette)
    );
    // A buffer smaller than the picture.
    let bmp = Bmp::parse(&good).unwrap();
    let mut out = vec![Pixel::CLEAR; 5];
    assert_eq!(bmp.decode(&mut out), Err(BmpError::Buffer));
}

/// Every file the tests above decode, for the cuts and the harms below.
fn corpus() -> Vec<Vec<u8>> {
    let rle8 = Info {
        width: 20,
        height: 3,
        bits: 8,
        compression: 1,
        ..INFO
    };
    let rle4 = Info {
        width: 27,
        height: 3,
        bits: 4,
        compression: 2,
        ..INFO
    };
    let v5 = Info {
        size: 124,
        width: 2,
        height: 1,
        bits: 32,
        compression: 3,
        masks: [0x0000_FF00, 0x00FF_0000, 0xFF00_0000, 0x0000_00FF],
        ..INFO
    };
    let one = Info {
        width: 10,
        height: 2,
        bits: 1,
        ..INFO
    };
    let data = [
        0x03, 0x04, 0x05, 0x06, 0x00, 0x03, 0x45, 0x56, 0x67, 0x00, 0x02, 0x78, 0x00, 0x02, 0x05,
        0x01, 0x02, 0x78, 0x00, 0x00, 0x09, 0x1E, 0x00, 0x01,
    ];
    let head = le(&[D(12), W(2), W(2), W(1), W(8)]);
    vec![
        file(rle8, &quads(&ramp()), &data),
        file(rle4, &quads(&ramp()[..16]), &data),
        file(v5, &[], &[1, 2, 3, 4, 5, 6, 7, 8]),
        file(one, &quads(&[K, WH]), &[0xAA, 0x55, 0, 0, 0x55, 0xAA, 0, 0]),
        file(INFO, &[], &[7; 24]),
        wrap(&head, &[1, 2, 3, 4, 5, 6], &[0, 1, 0, 0, 1, 0, 0, 0]),
    ]
}

/// **A file cut anywhere never panics**: cut in its headers or colour
/// table it is refused, and cut in its pixels it shows what arrived, what
/// did not clear. The whole file decodes: the control.
#[test]
fn a_file_cut_short_is_refused_or_shows_what_arrived() {
    for f in corpus() {
        let whole = decode(&f).expect("the control");
        let off = usize::try_from(u32::from_le_bytes([f[10], f[11], f[12], f[13]])).unwrap();
        for cut in 0..f.len() {
            let got = decode(&f[..cut]);
            if cut < off {
                assert_eq!(got, Err(BmpError::Malformed), "cut at {cut}");
                continue;
            }
            let got = got.unwrap_or_else(|e| panic!("cut at {cut}: {e:?}"));
            for (g, w) in got.iter().zip(&whole) {
                assert!(*g == *w || *g == CLEAR, "cut at {cut}");
            }
        }
    }
}

/// **A file harmed byte by byte never panics**: each byte set to 0, 0xFF,
/// 0x80 and its own value with the low bit turned, decoded into a buffer
/// of 65536 pixels, so a side harmed large is refused as too large or the
/// buffer as too small rather than allocated.
#[test]
fn a_harmed_file_never_panics() {
    let mut out = vec![Pixel::CLEAR; 1 << 16];
    let mut refused = 0;
    for f in corpus() {
        for at in 0..f.len() {
            for v in [0, 0xFF, 0x80, f[at] ^ 1] {
                let mut g = f.clone();
                g[at] = v;
                let r = Bmp::parse(&g).and_then(|b| b.decode(&mut out));
                refused += usize::from(r.is_err());
            }
        }
    }
    // The control: the harms reach the refusals.
    assert!(refused > 100, "{refused} refused");
}
