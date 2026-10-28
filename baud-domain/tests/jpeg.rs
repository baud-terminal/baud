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
//! baseline 4:2:0 at 29 by 21 -- are baud's own scene
//! (`baud-domain/tests/jpeg/scene.py`) encoded by the IJG's 6b `cjpeg`,
//! `-rgb` by libjpeg-turbo 3.2.0's, and each decoded by 6b's `djpeg -dct
//! int`, as `baud-domain/tests/jpeg/README.md` records.
//!
//! **Malformed input never panics.** The property mutates the valid files
//! by named classes -- a length that lies, a Huffman table whose counts
//! overflow, a table identifier past 3, a scan naming a component the frame
//! lacks, a spectral band out of range, a restart marker missing, a size at
//! 65535, the file cut at every marker -- and a positive control requires
//! that each class is generated.

use baud_domain::Pixel;
use baud_domain::jpeg::{Jpeg, JpegError, Orientation};
use proptest::prelude::*;

/// A PPM's or PGM's width, height and RGB bytes.
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
    let rgb = match fields[0].as_str() {
        "P6" => body[..w * h * 3].to_vec(),
        "P5" => body[..w * h].iter().flat_map(|&v| [v, v, v]).collect(),
        other => panic!("not a PPM or PGM: {other}"),
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

/// **The processes and forms T.81 defines that this decoder does not
/// implement are refused, not misdrawn**: the lossless, hierarchical and
/// arithmetic-coded frames, 12-bit samples, a height left to `DNL`.
#[test]
fn the_processes_not_built_are_refused() {
    let jpg = include_bytes!("ijg/testorig.jpg");
    let sof = jpg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
    for (what, at, byte) in [
        ("lossless SOF3", sof + 1, 0xC3),
        ("hierarchical SOF5", sof + 1, 0xC5),
        ("arithmetic SOF9", sof + 1, 0xC9),
        ("arithmetic progressive SOF10", sof + 1, 0xCA),
        ("12-bit samples", sof + 4, 12),
    ] {
        let mut f = jpg.to_vec();
        f[at] = byte;
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
        Harm::Size => at([0xC0, 0xC1, 0xC2]
            .iter()
            .flat_map(|&m| markers(&f, m))
            .collect())
        .map(|i| {
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
}
