//! The JPEG decoder against `[[source: itu-t81]]`, held pixel for pixel to
//! the IJG's reference decodes (`[[source: ijg-jpeg-6b]]`).
//!
//! **The conformance images are the IJG's own.** Its makefile's `test`
//! target requires `djpeg -dct int` to turn `testorig.jpg`, baseline 4:2:0,
//! and `testprog.jpg`, its progressive equivalent with successive
//! approximation, into the bytes of `testimg.ppm`; the decoder's arithmetic
//! is the IJG's, so the same holds here. The forms the IJG's images do not
//! reach -- grey, 4:4:4, 4:2:2 with restart markers, 4:1:1, 4:4:0, RGB,
//! sequential one component a scan, progressive 4:2:0 at an odd size,
//! baseline 4:2:0 at 29 by 21 -- are baud's own scene (`scene` below,
//! `baud-domain/tests/jpeg/scene.ppm`) encoded by the IJG's 6b `cjpeg`,
//! `-rgb` by libjpeg-turbo 3.2.0's, and each decoded by 6b's `djpeg -dct
//! int`, as `baud-domain/tests/jpeg/README.md` records.
//!
//! **The arithmetic, 12-bit and lossless forms** are libjpeg-turbo 3.1.4's
//! (`[[source: libjpeg-turbo-314]]`): its arithmetic test image beside its
//! Huffman twin, and baud's scene encoded by its `cjpeg` and decoded by its
//! `djpeg -dct int`, which decodes the 6b references here to 6b's bytes.
//! It writes no arithmetic-coded lossless file and no subsampled lossless
//! one; those are Thomas Richter's T.81 implementation's
//! (`[[source: richter-libjpeg]]`), its own decoder returning the source.
//!
//! **Malformed input never panics.** The property mutates the valid files
//! by named classes -- a length that lies, a Huffman table whose counts
//! overflow, a table identifier past 3, a scan naming a component the frame
//! lacks, a spectral band out of range, a restart marker missing, a size at
//! 65535, the file cut at every marker -- and a positive control requires
//! that each class is generated. A second property harms the arithmetic,
//! 12-bit and lossless seeds by those classes and four more: a `DAC`'s
//! values, the precision, the restart interval, and a lossless scan's
//! predictor and point transform.

use baud_domain::Pixel;
use baud_domain::jpeg::{Jpeg, JpegError, Orientation};
use proptest::prelude::*;

/// A PPM's or PGM's width, height and RGB bytes, each sample at 8 bits
/// as the decoder draws it: a sample of more than 8 bits by its high eight,
/// as a 16-bit PNG's is drawn, and one of fewer scaled to the full range,
/// as a sub-byte PNG grey is.
fn pnm(b: &[u8]) -> (usize, usize, Vec<u8>) {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&b[s..i]).unwrap().to_owned());
    }
    let body = &b[i + 1..];
    let (w, h): (usize, usize) = (fields[1].parse().unwrap(), fields[2].parse().unwrap());
    let max: u32 = fields[3].parse().unwrap();
    let bits = 32 - max.leading_zeros();
    assert_eq!(max, (1 << bits) - 1, "a maxval of whole bits");
    let channels = match fields[0].as_str() {
        "P6" => 3,
        "P5" => 1,
        other => panic!("not a PPM or PGM: {other}"),
    };
    let n = w * h * channels;
    let samples: Vec<u32> = match max > 255 {
        true => body[..2 * n]
            .chunks(2)
            .map(|p| u32::from(u16::from_be_bytes([p[0], p[1]])))
            .collect(),
        false => body[..n].iter().map(|&v| u32::from(v)).collect(),
    };
    let eight = |v: u32| {
        u8::try_from(match bits {
            8 => v,
            b if b > 8 => v >> (b - 8),
            _ => v * 255 / max,
        })
        .unwrap()
    };
    let rgb = match channels {
        3 => samples.iter().map(|&v| eight(v)).collect(),
        _ => samples
            .iter()
            .flat_map(|&v| {
                let g = eight(v);
                [g, g, g]
            })
            .collect(),
    };
    (w, h, rgb)
}

/// The picture `jpg` decodes to: its size and its pixels as RGB bytes.
fn decode(jpg: &[u8]) -> Result<(usize, usize, Vec<u8>), JpegError> {
    let j = Jpeg::parse(jpg)?;
    let size = j.size();
    let (w, h) = (
        usize::try_from(size.width).unwrap(),
        usize::try_from(size.height).unwrap(),
    );
    let mut scratch = vec![0xA5u8; j.scratch_len().unwrap()];
    let mut out = vec![Pixel::CLEAR; w * h];
    let got = j.decode(&mut scratch, &mut out)?;
    assert_eq!(got, size);
    let rgb = out
        .iter()
        .flat_map(|p| {
            let [a, r, g, b] = p.bits().to_be_bytes();
            assert_eq!(a, 255, "every pixel opaque");
            [r, g, b]
        })
        .collect();
    Ok((w, h, rgb))
}

/// How many bytes of `got` differ from `want`, with the first pixel that does.
fn differ(got: &[u8], want: &[u8], w: usize) -> (usize, Option<(usize, usize)>) {
    let n = got.iter().zip(want).filter(|(a, b)| a != b).count();
    let first = got
        .chunks(3)
        .zip(want.chunks(3))
        .position(|(a, b)| a != b)
        .map(|i| (i % w, i / w));
    (n, first)
}

fn holds(jpg: &[u8], reference: &[u8], name: &str) {
    let (w, h, want) = pnm(reference);
    let (gw, gh, got) = decode(jpg).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    assert_eq!((gw, gh), (w, h), "{name}: the size");
    assert_eq!(
        differ(&got, &want, w),
        (0, None),
        "{name}: bytes that differ"
    );
}

const TESTIMG: &[u8] = include_bytes!("ijg/testimg.ppm");

#[test]
fn the_ijgs_baseline_test_image_decodes_to_its_reference() {
    holds(include_bytes!("ijg/testorig.jpg"), TESTIMG, "testorig.jpg");
}

#[test]
fn the_ijgs_progressive_test_image_decodes_to_its_reference() {
    holds(include_bytes!("ijg/testprog.jpg"), TESTIMG, "testprog.jpg");
}

/// **The arithmetic-coded test image decodes to the same bytes as its
/// Huffman twin**: libjpeg-turbo's `testimgari.jpg` is `cjpeg -dct int
/// -arithmetic` of its `testorig.ppm` and `testimgint.jpg` the same without
/// `-arithmetic`, and its suite requires `jpegtran` to turn the first into
/// the second, so the coefficients are the same; the reference is its
/// `djpeg -dct int`, which decodes this decoder's 4:2:0 references to 6b's
/// bytes.
#[test]
fn the_arithmetic_test_image_decodes_to_its_reference() {
    let reference = include_bytes!("ijg/testimgari.ppm");
    holds(
        include_bytes!("ijg/testimgari.jpg"),
        reference,
        "testimgari.jpg",
    );
    holds(
        include_bytes!("ijg/testimgint.jpg"),
        reference,
        "testimgint.jpg",
    );
}

/// **Every arithmetic form decodes to its reference** (Annex D, F.2.4,
/// G.1.3): sequential with restart markers, progressive with spectral
/// selection and successive approximation, in colour and grey, as
/// libjpeg-turbo 3.1.4's `cjpeg -arithmetic` writes them; and both again
/// with conditioning a `DAC` sets past the defaults (B.2.4.3), L = 2 and U
/// = 5, L = 1 and U = 3, Kx 20 and 40, as libjpeg-turbo writes them for
/// `dacenc.c`.
#[test]
fn every_arithmetic_form_decodes_to_its_reference() {
    let cases: [(&str, &[u8], &[u8]); 6] = [
        (
            "4:2:2 sequential, a restart every two MCUs",
            include_bytes!("jpeg/a422r.jpg"),
            include_bytes!("jpeg/a422r.ppm"),
        ),
        (
            "4:2:0 progressive",
            include_bytes!("jpeg/a420p.jpg"),
            include_bytes!("jpeg/a420p.ppm"),
        ),
        (
            "4:4:4 progressive, a restart every three MCUs",
            include_bytes!("jpeg/ap444.jpg"),
            include_bytes!("jpeg/ap444.ppm"),
        ),
        (
            "grey progressive",
            include_bytes!("jpeg/agreyp.jpg"),
            include_bytes!("jpeg/agreyp.pgm"),
        ),
        (
            "4:2:0 sequential, conditioning set",
            include_bytes!("jpeg/adac.jpg"),
            include_bytes!("jpeg/adac.ppm"),
        ),
        (
            "4:2:0 progressive, conditioning set",
            include_bytes!("jpeg/adacp.jpg"),
            include_bytes!("jpeg/adacp.ppm"),
        ),
    ];
    for (name, jpg, reference) in cases {
        holds(jpg, reference, name);
    }
}

/// **12-bit samples draw at their high eight bits** (F.2.5, F.2.6), as a
/// 16-bit PNG's do: the reference is libjpeg-turbo's 12-bit decode, its
/// samples shifted down by four, Huffman and arithmetic, sequential and
/// progressive.
#[test]
fn twelve_bit_samples_draw_at_their_high_eight_bits() {
    let cases: [(&str, &[u8], &[u8]); 4] = [
        (
            "extended sequential, Huffman",
            include_bytes!("jpeg/p12.jpg"),
            include_bytes!("jpeg/p12.ppm"),
        ),
        (
            "progressive, Huffman",
            include_bytes!("jpeg/p12p.jpg"),
            include_bytes!("jpeg/p12p.ppm"),
        ),
        (
            "extended sequential, arithmetic, a restart every MCU row",
            include_bytes!("jpeg/p12a.jpg"),
            include_bytes!("jpeg/p12a.ppm"),
        ),
        (
            "progressive, arithmetic, 4:2:2",
            include_bytes!("jpeg/p12ap.jpg"),
            include_bytes!("jpeg/p12ap.ppm"),
        ),
    ];
    for (name, jpg, reference) in cases {
        holds(jpg, reference, name);
    }
}

const SCENE: &[u8] = include_bytes!("jpeg/scene.ppm");
const SCENE16: &[u8] = include_bytes!("jpeg/scene16.ppm");

/// The C library's `rand` step, `seed * 1103515245 + 12345` kept to 31
/// bits, that seeds every picture written below.
fn lcg(seed: &mut u32) -> u32 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345) & 0x7fff_ffff;
    *seed
}

/// **baud's scene**, the source of `scene.ppm` and `scene16.ppm`: 61 by
/// 45, gradients, a hard edge and a checker, so every sampling ratio and
/// the IDCT see detail at block edges. At 16 bits each sample is its 8-bit
/// value times 257 with a deterministic eight-bit dither in the low byte,
/// so the low byte carries detail.
fn scene(sixteen: bool) -> Vec<u8> {
    let (w, h) = (61u32, 45u32);
    let max = if sixteen { 65535 } else { 255 };
    let mut out = format!("P6\n{w} {h}\n{max}\n").into_bytes();
    let mut seed = 12345;
    for y in 0..h {
        for x in 0..w {
            let n = (lcg(&mut seed) >> 16) & 15;
            let r = (x * 4 + n) & 255;
            let g = (y * 5 + if (x / 6 + y / 6) % 2 == 1 { 40 } else { 0 }) & 255;
            let b = if x > 30 && y < 20 { 230 } else { (x * y) & 255 };
            for v in [r, g, b] {
                if sixteen {
                    let dither = (lcg(&mut seed) >> 16) & 255;
                    let sample = u16::try_from((v * 257) ^ dither).unwrap();
                    out.extend_from_slice(&sample.to_be_bytes());
                } else {
                    out.push(u8::try_from(v).unwrap());
                }
            }
        }
    }
    out
}

/// **`edge16.pgm`**: a 16 by 8 16-bit grey picture whose neighbours differ
/// by half the range and more, so a lossless difference reaches 32768 (SSSS
/// 16) and the arithmetic coder's magnitude its top category.
fn edge16() -> Vec<u8> {
    let (w, h) = (16u32, 8u32);
    let mut rows: Vec<Vec<u32>> = [[0, 32768], [32768, 0], [65535, 32767], [1, 32769]]
        .iter()
        .map(|pair| pair.repeat(8))
        .collect();
    let mut seed = 4242;
    for y in 4..h {
        rows.push(
            (0..w)
                .map(|x| {
                    ((lcg(&mut seed) >> 8) & 0xFFFF) ^ if (x + y) % 2 == 1 { 0x8000 } else { 0 }
                })
                .collect(),
        );
    }
    let mut out = format!("P5\n{w} {h}\n65535\n").into_bytes();
    for v in rows.iter().flatten() {
        out.extend_from_slice(&u16::try_from(*v).unwrap().to_be_bytes());
    }
    out
}

/// **The committed sources are what their writers write**, each held byte
/// for byte; a copy one byte off compares unequal.
#[test]
fn the_scene_and_the_edge_are_what_their_writers_write() {
    let edge: &[u8] = include_bytes!("jpeg/edge16.pgm");
    for (name, fresh, committed) in [
        ("scene.ppm", scene(false), SCENE),
        ("scene16.ppm", scene(true), SCENE16),
        ("edge16.pgm", edge16(), edge),
    ] {
        assert!(fresh == committed, "{name} is not what its writer writes");
        let mut off = fresh.clone();
        let last = off.len() - 1;
        off[last] ^= 1;
        assert!(off != committed, "{name}: the check cannot fire");
    }
}

/// **Each of the seven predictors decodes to the source** (H.2, Table
/// H.1): the lossless process returns the picture it was given, so the
/// reference is the scene itself, which libjpeg-turbo's `djpeg` also
/// returns.
#[test]
fn every_lossless_predictor_decodes_to_the_source() {
    let cases: [&[u8]; 7] = [
        include_bytes!("jpeg/l1.jpg"),
        include_bytes!("jpeg/l2.jpg"),
        include_bytes!("jpeg/l3.jpg"),
        include_bytes!("jpeg/l4.jpg"),
        include_bytes!("jpeg/l5.jpg"),
        include_bytes!("jpeg/l6.jpg"),
        include_bytes!("jpeg/l7.jpg"),
    ];
    for (k, jpg) in cases.iter().enumerate() {
        holds(jpg, SCENE, &format!("predictor {}", k + 1));
    }
}

/// **Lossless precisions from 2 to 16 bits and the point transform**
/// (H.1.1, H.2.2): each draws at 8 bits as the reference's samples do --
/// above 8 bits their high eight, below them scaled to the full range --
/// with restart intervals of whole MCU rows.
#[test]
fn every_lossless_precision_decodes_to_its_reference() {
    let cases: [(&str, &[u8], &[u8]); 5] = [
        (
            "12 bits, predictor 4, Pt 2, a restart every row",
            include_bytes!("jpeg/l4t2r.jpg"),
            include_bytes!("jpeg/l4t2r.ppm"),
        ),
        (
            "16 bits, predictor 6",
            include_bytes!("jpeg/l6p16.jpg"),
            SCENE16,
        ),
        (
            "16 bits, predictor 5, Pt 3, a restart every two rows",
            include_bytes!("jpeg/l5p16t3.jpg"),
            include_bytes!("jpeg/l5p16t3.ppm"),
        ),
        (
            "5-bit grey, predictor 7, Pt 1",
            include_bytes!("jpeg/l7g5.jpg"),
            include_bytes!("jpeg/l7g5.pgm"),
        ),
        (
            "2-bit grey, predictor 2, a restart every three rows",
            include_bytes!("jpeg/l2g2.jpg"),
            include_bytes!("jpeg/l2g2.pgm"),
        ),
    ];
    for (name, jpg, reference) in cases {
        holds(jpg, reference, name);
    }
}

/// **Arithmetic-coded lossless decodes to the source** (H.1.2.3, H.2): the
/// two-dimensional model's 25 conditioning states, written by Thomas
/// Richter's T.81 implementation, whose own decoder returns the source;
/// grey, RGB interleaved, with a restart every MCU row, and at 16 bits.
#[test]
fn arithmetic_lossless_decodes_to_the_source() {
    let cases: [(&str, &[u8], &[u8]); 4] = [
        (
            "grey",
            include_bytes!("jpeg/ar_grey.jpg"),
            include_bytes!("jpeg/grey.pgm"),
        ),
        ("RGB", include_bytes!("jpeg/ar_rgb.jpg"), SCENE),
        (
            "RGB, a restart every MCU row",
            include_bytes!("jpeg/ar_rgbr.jpg"),
            SCENE,
        ),
        ("RGB at 16 bits", include_bytes!("jpeg/ar16.jpg"), SCENE16),
    ];
    for (name, jpg, reference) in cases {
        holds(jpg, reference, name);
    }
}

/// **An interleaved lossless scan of subsampled components decodes**:
/// RGB with G and B at half the rows, each MCU one R sample above another
/// and one each of G and B, a restart every MCU row, Huffman- and
/// arithmetic-coded by Thomas Richter's implementation; the reference is
/// libjpeg-turbo's decode of the Huffman one, whose upsampling repeats a
/// row as this decoder's does where the ratio is two to one down.
#[test]
fn a_subsampled_lossless_scan_decodes_to_its_reference() {
    let reference = include_bytes!("jpeg/lsub.ppm");
    holds(include_bytes!("jpeg/lsub.jpg"), reference, "Huffman");
    holds(include_bytes!("jpeg/arsub.jpg"), reference, "arithmetic");
}

/// **A difference of half the range decodes**: a 16-bit grey picture whose
/// neighbours differ by 32768 and more, so predictor 1's Huffman-coded
/// difference takes the category Table H.2 adds, 16, which "No extra bits
/// are appended after", and the arithmetic-coded one its top magnitude
/// category.
#[test]
fn a_difference_of_half_the_range_decodes() {
    let reference = include_bytes!("jpeg/edge16.pgm");
    holds(
        include_bytes!("jpeg/l1e16.jpg"),
        reference,
        "Huffman, predictor 1",
    );
    holds(
        include_bytes!("jpeg/are16.jpg"),
        reference,
        "arithmetic, predictor 4",
    );
}

#[test]
fn every_sampling_and_colour_form_decodes_to_its_reference() {
    let cases: [(&str, &[u8], &[u8]); 9] = [
        (
            "grey",
            include_bytes!("jpeg/grey.jpg"),
            include_bytes!("jpeg/grey.pgm"),
        ),
        (
            "4:4:4",
            include_bytes!("jpeg/s444.jpg"),
            include_bytes!("jpeg/s444.ppm"),
        ),
        (
            "4:2:2, a restart every two MCUs",
            include_bytes!("jpeg/s422r.jpg"),
            include_bytes!("jpeg/s422r.ppm"),
        ),
        (
            "4:2:0 progressive",
            include_bytes!("jpeg/s420p.jpg"),
            include_bytes!("jpeg/s420p.ppm"),
        ),
        (
            "4:1:1",
            include_bytes!("jpeg/s411.jpg"),
            include_bytes!("jpeg/s411.ppm"),
        ),
        (
            "4:4:0",
            include_bytes!("jpeg/s440.jpg"),
            include_bytes!("jpeg/s440.ppm"),
        ),
        (
            "RGB, Adobe's transform 0",
            include_bytes!("jpeg/rgb.jpg"),
            include_bytes!("jpeg/rgb.ppm"),
        ),
        (
            "4:2:0 sequential, a scan a component",
            include_bytes!("jpeg/s420seq.jpg"),
            include_bytes!("jpeg/s420seq.ppm"),
        ),
        (
            "4:2:0 at 29 by 21",
            include_bytes!("jpeg/small.jpg"),
            include_bytes!("jpeg/small.ppm"),
        ),
    ];
    for (name, jpg, reference) in cases {
        holds(jpg, reference, name);
    }
}

/// **Four components draw as the inverted product**: Adobe's applications
/// store C, M, Y and K inverted, and each of R, G, B is its stored
/// channel times stored K over 255, rounded, as Firefox draws them. The
/// samples are ImageMagick's decode with libjpeg's integer DCT
/// (`baud-domain/tests/jpeg/README.md`), which reports each channel as
/// the colour's own, the inverse of what is stored; a YCCK file reaches
/// them through JFIF's YCbCr equations first.
#[test]
fn cmyk_and_ycck_draw_as_the_inverted_product() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        (
            "CMYK, Adobe's transform 0",
            include_bytes!("jpeg/cmyk.jpg"),
            include_bytes!("jpeg/cmyk.cmyk"),
        ),
        (
            "YCCK, Adobe's transform 2",
            include_bytes!("jpeg/ycck.jpg"),
            include_bytes!("jpeg/ycck.cmyk"),
        ),
    ];
    for (name, jpg, samples) in cases {
        let (w, h, got) = decode(jpg).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!((w, h), (16, 8), "{name}");
        for (i, s) in samples.chunks(4).enumerate() {
            let k = 255 - u32::from(s[3]);
            let ch = |c: u8| u8::try_from(((255 - u32::from(c)) * k + 127) / 255).unwrap();
            let want = [ch(s[0]), ch(s[1]), ch(s[2])];
            assert_eq!(got[i * 3..i * 3 + 3], want, "{name}: pixel {i}");
        }
    }
}

/// `jpg` with an Exif `APP1` after its `APP0`, the 0th IFD's one entry tag
/// 274, `SHORT`, count 1, value `o`, in the byte order `big` names.
fn with_exif(jpg: &[u8], o: u16, big: bool) -> Vec<u8> {
    let mut tiff: Vec<u8> = Vec::new();
    let w16 = |v: u16| match big {
        true => v.to_be_bytes(),
        false => v.to_le_bytes(),
    };
    let w32 = |v: u32| match big {
        true => v.to_be_bytes(),
        false => v.to_le_bytes(),
    };
    tiff.extend_from_slice(match big {
        true => b"MM\x00\x2A",
        false => b"II\x2A\x00",
    });
    tiff.extend(w32(8));
    tiff.extend(w16(1));
    tiff.extend(w16(274));
    tiff.extend(w16(3));
    tiff.extend(w32(1));
    tiff.extend(w16(o));
    tiff.extend([0, 0]);
    tiff.extend(w32(0));
    let mut body = b"Exif\0\0".to_vec();
    body.extend(tiff);
    let at = match jpg[2..4] == [0xFF, 0xE0] {
        true => 4 + usize::from(u16::from_be_bytes([jpg[4], jpg[5]])),
        false => 2,
    };
    let mut out = jpg[..at].to_vec();
    out.extend([0xFF, 0xE1]);
    out.extend(u16::try_from(body.len() + 2).unwrap().to_be_bytes());
    out.extend(body);
    out.extend_from_slice(&jpg[at..]);
    out
}

/// **Each orientation shows the reference turned as Exif's Figure 12 says**,
/// in both byte orders, and a reserved value as 1; the expected picture is
/// built here from the definitions, not from the decoder's own mapping.
#[test]
fn the_exif_orientation_turns_the_picture() {
    let jpg = include_bytes!("ijg/testorig.jpg");
    let (w, h, want) = pnm(TESTIMG);
    let px = |x: usize, y: usize| &want[(y * w + x) * 3..(y * w + x) * 3 + 3];
    // For a shown pixel (x, y), the coded pixel it is, from each value's
    // 0th row and 0th column.
    type Coded = fn(usize, usize, usize, usize) -> (usize, usize);
    let cases: [(u16, Coded); 10] = [
        (1, |x, y, _, _| (x, y)),
        (2, |x, y, w, _| (w - 1 - x, y)),
        (3, |x, y, w, h| (w - 1 - x, h - 1 - y)),
        (4, |x, y, _, h| (x, h - 1 - y)),
        (5, |x, y, _, _| (y, x)),
        (6, |x, y, _, h| (y, h - 1 - x)),
        (7, |x, y, w, h| (w - 1 - y, h - 1 - x)),
        (8, |x, y, w, _| (w - 1 - y, x)),
        (0, |x, y, _, _| (x, y)),
        (9, |x, y, _, _| (x, y)),
    ];
    for big in [true, false] {
        for (o, coded) in cases {
            let file = with_exif(jpg, o, big);
            let turned = matches!(o, 5..=8);
            let (sw, sh) = match turned {
                true => (h, w),
                false => (w, h),
            };
            let mut expect = Vec::new();
            for y in 0..sh {
                for x in 0..sw {
                    let (cx, cy) = coded(x, y, w, h);
                    expect.extend_from_slice(px(cx, cy));
                }
            }
            let (gw, gh, got) = decode(&file).unwrap();
            assert_eq!((gw, gh), (sw, sh), "orientation {o}, big-endian {big}");
            assert_eq!(
                differ(&got, &expect, sw),
                (0, None),
                "orientation {o}, big-endian {big}"
            );
            let want_o = Orientation::of(o);
            assert_eq!(Jpeg::parse(&file).unwrap().orientation(), want_o);
        }
    }
}

/// **The process T.81 defines that this decoder does not implement is
/// refused, not misdrawn**: the hierarchical process's differential frames
/// and its `DHP`, and a height left to `DNL`; and a precision Table B.2
/// does not allow the process is refused as a frame T.81 does not allow.
#[test]
fn the_processes_not_built_are_refused() {
    let jpg = include_bytes!("ijg/testorig.jpg");
    let sof = jpg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    for (what, byte) in [
        ("differential sequential SOF5", 0xC5),
        ("differential progressive SOF6", 0xC6),
        ("differential lossless SOF7", 0xC7),
        ("differential sequential, arithmetic, SOF13", 0xCD),
        ("differential progressive, arithmetic, SOF14", 0xCE),
        ("differential lossless, arithmetic, SOF15", 0xCF),
        ("DHP before the frame", 0xDE),
    ] {
        let mut f = jpg.to_vec();
        f[sof + 1] = byte;
        assert_eq!(
            Jpeg::parse(&f).err(),
            Some(JpegError::Unsupported),
            "{what}"
        );
    }
    let mut f = jpg.to_vec();
    f[sof + 5] = 0;
    f[sof + 6] = 0;
    assert_eq!(
        Jpeg::parse(&f).err(),
        Some(JpegError::Unsupported),
        "a height left to DNL"
    );
    for (what, marker, p) in [
        ("baseline at 12 bits", 0xC0, 12),
        ("extended sequential at 10 bits", 0xC1, 10),
        ("progressive at 16 bits", 0xC2, 16),
        ("arithmetic sequential at 9 bits", 0xC9, 9),
        ("lossless at 1 bit", 0xC3, 1),
        ("lossless at 17 bits", 0xC3, 17),
        ("arithmetic lossless at 0 bits", 0xCB, 0),
    ] {
        let mut f = jpg.to_vec();
        f[sof + 1] = marker;
        f[sof + 4] = p;
        assert_eq!(Jpeg::parse(&f).err(), Some(JpegError::Frame), "{what}");
    }
}

/// **A file cut inside its entropy-coded data shows what arrived**: the
/// blocks before the cut as the whole file draws them.
#[test]
fn a_file_cut_short_draws_what_arrived() {
    let jpg = include_bytes!("ijg/testorig.jpg");
    let (w, _, want) = pnm(TESTIMG);
    let (_, _, got) = decode(&jpg[..jpg.len() * 3 / 4]).unwrap();
    // The first MCU row, sixteen lines, came whole.
    let row = w * 3 * 8;
    assert_eq!(got[..row], want[..row]);
    assert_ne!(got, want);
}

/// The ways a file is made malformed, each a class the property names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Harm {
    /// A segment's length set to `v`.
    Length,
    /// A `DHT`'s sixteen counts set high.
    HuffmanCounts,
    /// A table identifier past 3.
    TableId,
    /// A scan component selector no frame component has.
    ScanComponent,
    /// `Ss`, `Se`, `Ah`/`Al` out of range.
    Band,
    /// A restart marker removed, or a stray one inserted.
    Restart,
    /// The frame's width and height at 65535.
    Size,
    /// The file cut at a marker.
    Cut,
    /// A byte anywhere set to anything.
    Byte,
    /// A `DAC`'s class and destination, or its conditioning value, set to
    /// `v`.
    Conditioning,
    /// The frame's sample precision set to `v`.
    Precision,
    /// The `DRI`'s interval set to `v`, a lossless one then not whole MCU
    /// rows.
    Interval,
    /// A scan's `Ss` set to a predictor and its `Al` to a point transform,
    /// each in or near its range.
    Predictor,
}

const HARMS: [Harm; 9] = [
    Harm::Length,
    Harm::HuffmanCounts,
    Harm::TableId,
    Harm::ScanComponent,
    Harm::Band,
    Harm::Restart,
    Harm::Size,
    Harm::Cut,
    Harm::Byte,
];

/// The classes the arithmetic, wide and lossless forms are harmed by: every
/// one above and the four their headers add.
const HARMS_WIDE: [Harm; 13] = [
    Harm::Length,
    Harm::HuffmanCounts,
    Harm::TableId,
    Harm::ScanComponent,
    Harm::Band,
    Harm::Restart,
    Harm::Size,
    Harm::Cut,
    Harm::Byte,
    Harm::Conditioning,
    Harm::Precision,
    Harm::Interval,
    Harm::Predictor,
];

/// Every frame marker this decoder reads.
const SOFS: [u8; 7] = [0xC0, 0xC1, 0xC2, 0xC3, 0xC9, 0xCA, 0xCB];

/// The positions of every marker `m` in `f`.
fn markers(f: &[u8], m: u8) -> Vec<usize> {
    f.windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] == 0xFF && w[1] == m)
        .map(|(i, _)| i)
        .collect()
}

/// `f` harmed by `harm`, at the `pick`th place it applies, with `v`; and
/// whether it applied.
fn harmed(f: &[u8], harm: Harm, pick: usize, v: u8) -> (Vec<u8>, bool) {
    let mut f = f.to_vec();
    let at = |ps: Vec<usize>| (!ps.is_empty()).then(|| ps[pick % ps.len()]);
    let segs: Vec<usize> = (0xC0..=0xFE)
        .filter(|&m| !(0xD0..=0xD9).contains(&m))
        .flat_map(|m| markers(&f, m))
        .collect();
    let applied = match harm {
        Harm::Length => at(segs).map(|i| {
            if i + 3 < f.len() {
                f[i + 2] = v;
                f[i + 3] = v.rotate_left(3);
            }
        }),
        Harm::HuffmanCounts => at(markers(&f, 0xC4)).map(|i| {
            for k in 0..16 {
                if i + 5 + k < f.len() {
                    f[i + 5 + k] = v.wrapping_add(u8::try_from(k).unwrap());
                }
            }
        }),
        Harm::TableId => at([markers(&f, 0xC4), markers(&f, 0xDB)].concat()).map(|i| {
            if i + 4 < f.len() {
                f[i + 4] = (f[i + 4] & 0xF0) | (4 + (v & 11));
            }
        }),
        Harm::ScanComponent => at(markers(&f, 0xDA)).map(|i| {
            if i + 5 < f.len() {
                f[i + 5] = v | 0x80;
            }
        }),
        Harm::Band => at(markers(&f, 0xDA)).map(|i| {
            let n = usize::from(f.get(i + 4).copied().unwrap_or(0));
            let t = i + 5 + 2 * n;
            if t + 2 < f.len() {
                f[t + usize::from(v % 3)] = v | 0x40;
            }
        }),
        Harm::Restart => {
            let rst: Vec<usize> = (0xD0..=0xD7).flat_map(|m| markers(&f, m)).collect();
            match at(rst) {
                Some(i) => {
                    f.drain(i..i + 2);
                    Some(())
                }
                None => at(markers(&f, 0xDA)).map(|i| {
                    let j = (i + 12 + usize::from(v)).min(f.len());
                    f.splice(j..j, [0xFF, 0xD0 | (v & 7)]);
                }),
            }
        }
        Harm::Size => at(SOFS.iter().flat_map(|&m| markers(&f, m)).collect()).map(|i| {
            for k in 5..9 {
                if i + k < f.len() {
                    f[i + k] = 0xFF;
                }
            }
        }),
        Harm::Cut => at(segs).map(|i| f.truncate(i + usize::from(v % 4))),
        Harm::Byte => (!f.is_empty()).then(|| {
            let i = pick % f.len();
            f[i] = v;
        }),
        Harm::Conditioning => at(markers(&f, 0xCC)).map(|i| {
            let n = usize::from(f.get(i + 3).copied().unwrap_or(0)).saturating_sub(2);
            let j = i + 4 + usize::from(v) % n.max(1);
            if j < f.len() {
                f[j] = v.rotate_left(u32::from(v & 7));
            }
        }),
        Harm::Precision => at(SOFS.iter().flat_map(|&m| markers(&f, m)).collect()).map(|i| {
            if i + 4 < f.len() {
                f[i + 4] = v % 20;
            }
        }),
        Harm::Interval => at(markers(&f, 0xDD)).map(|i| {
            if i + 5 < f.len() {
                f[i + 4] = v >> 6;
                f[i + 5] = v;
            }
        }),
        Harm::Predictor => at(markers(&f, 0xDA)).map(|i| {
            let n = usize::from(f.get(i + 4).copied().unwrap_or(0));
            let t = i + 5 + 2 * n;
            if t + 2 < f.len() {
                match v & 1 {
                    0 => f[t] = (v >> 1) % 9,
                    _ => f[t + 2] = (f[t + 2] & 0xF0) | ((v >> 1) & 15),
                }
            }
        }),
    };
    (f, applied.is_some())
}

/// What any decode may cost: the scratch it asks for is what its frame
/// needs, and the decode returns, whatever it returns.
fn survives(f: &[u8]) {
    let Ok(j) = Jpeg::parse(f) else {
        return;
    };
    let size = j.size();
    let area = usize::try_from(size.width).unwrap() * usize::try_from(size.height).unwrap();
    // A frame past a few megapixels is the store's to refuse, not this
    // property's to allocate.
    let need = j.scratch_len().unwrap();
    if area > 1 << 22 || need > 1 << 26 {
        return;
    }
    let mut scratch = vec![0u8; need];
    let mut out = vec![Pixel::CLEAR; area];
    let _ = j.decode(&mut scratch, &mut out);
    // A buffer one short is refused, never written past.
    if need > 0 {
        let mut short = vec![0u8; need - 1];
        assert_eq!(j.decode(&mut short, &mut out), Err(JpegError::Buffer));
    }
}

const SEEDS: [&[u8]; 4] = [
    include_bytes!("ijg/testorig.jpg"),
    include_bytes!("ijg/testprog.jpg"),
    include_bytes!("jpeg/s422r.jpg"),
    include_bytes!("jpeg/s420p.jpg"),
];

/// The arithmetic, wide and lossless seeds: the arithmetic test image,
/// progressive arithmetic with and without restarts, 12-bit progressive
/// arithmetic, lossless at 16 and 2 bits with restarts, and arithmetic
/// lossless with restarts.
const SEEDS_WIDE: [&[u8]; 8] = [
    include_bytes!("ijg/testimgari.jpg"),
    include_bytes!("jpeg/a420p.jpg"),
    include_bytes!("jpeg/ap444.jpg"),
    include_bytes!("jpeg/p12ap.jpg"),
    include_bytes!("jpeg/l5p16t3.jpg"),
    include_bytes!("jpeg/l2g2.jpg"),
    include_bytes!("jpeg/ar_rgbr.jpg"),
    include_bytes!("jpeg/a422r.jpg"),
];

proptest! {
    #[test]
    fn a_malformed_file_never_panics(
        seed in 0..SEEDS.len(),
        harms in proptest::collection::vec((0..HARMS.len(), any::<usize>(), any::<u8>()), 1..4),
    ) {
        let mut f = SEEDS[seed].to_vec();
        for (h, pick, v) in harms {
            f = harmed(&f, HARMS[h], pick, v).0;
        }
        survives(&f);
    }

    #[test]
    fn a_malformed_arithmetic_wide_or_lossless_file_never_panics(
        seed in 0..SEEDS_WIDE.len(),
        harms in proptest::collection::vec((0..HARMS_WIDE.len(), any::<usize>(), any::<u8>()), 1..4),
    ) {
        let mut f = SEEDS_WIDE[seed].to_vec();
        for (h, pick, v) in harms {
            f = harmed(&f, HARMS_WIDE[h], pick, v).0;
        }
        survives(&f);
    }
}

/// **The positive control**: every class applies to some seed, and the
/// files it makes differ from the seed, so no class is a probe that cannot
/// fire.
#[test]
fn every_harm_class_is_generated() {
    for harm in HARMS {
        let fired = SEEDS.iter().any(|s| {
            (0..8).any(|pick| {
                let (f, applied) = harmed(s, harm, pick, 0x5A);
                applied && f != *s
            })
        });
        assert!(fired, "{harm:?} never changes a file");
    }
    // Restart markers are in the seed that has them, and taken out.
    let s = SEEDS[2];
    let rst = (0xD0..=0xD7).map(|m| markers(s, m).len()).sum::<usize>();
    assert!(rst > 0, "the 4:2:2 seed carries restart markers");
    for harm in HARMS_WIDE {
        let fired = SEEDS_WIDE.iter().any(|s| {
            (0..8).any(|pick| {
                (0..=255u8).step_by(37).any(|v| {
                    let (f, applied) = harmed(s, harm, pick, v);
                    applied && f != *s
                })
            })
        });
        assert!(fired, "{harm:?} never changes a wide seed");
    }
    // The wide seeds reach every process and coding they stand for.
    let frames: Vec<u8> = SEEDS_WIDE
        .iter()
        .flat_map(|s| SOFS.iter().filter(|&&m| !markers(s, m).is_empty()).copied())
        .collect();
    for m in [0xC3, 0xC9, 0xCA, 0xCB] {
        assert!(frames.contains(&m), "a wide seed has SOF {m:02X}");
    }
    let p12 = SEEDS_WIDE[3];
    let sof = markers(p12, 0xCA)[0];
    assert_eq!(p12[sof + 4], 12, "the 12-bit seed");
}
