//! **`logo`'s parts held to the deleted Python driver**: each input here
//! is built by a seeded generator the driver's own functions were run on,
//! under the pinned Pillow 12.3.0 and NumPy 2.5.2, and each pin is what
//! they returned -- the SHA-256 of the bytes, the report's numbers, or
//! the refusal's words. Each check's perturbation is a test that it
//! fires, as the driver's did.

use baud_pictures::logo::{self, H, W};
use baud_pictures::{gif, json, png, sha256, webp};

/// The generator both sides ran: a 64-bit LCG, its top byte.
struct Lcg(u64);

impl Lcg {
    fn byte(&mut self) -> u8 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 56) as u8
    }
}

fn sha(b: &[u8]) -> String {
    sha256::hex(b)
}

/// An RGBA picture, its alpha a stripe of 0 and 255.
fn rgba(w: usize, h: usize, seed: u64) -> Vec<u8> {
    let mut r = Lcg(seed);
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let a = if (x * 7 + y * 3) % 11 < 6 { 255 } else { 0 };
            let red = r.byte();
            let blue = r.byte() / 2;
            out.extend([red, ((x * 5 + y) % 256) as u8, blue, a]);
        }
    }
    out
}

const SETTINGS: webp::Settings = webp::Settings {
    quality: 90.0,
    alpha_quality: 100.0,
    method: 6,
};

#[test]
fn a_webp_is_the_bytes_pillow_wrote_and_reads_back_as_pillow_read_it() {
    for (w, h, seed, method, file, read) in [
        (
            61,
            37,
            1,
            6,
            "3d94022139bd155a9d220bfdcf02563f200ce28ca1efbc6878107e90a7d0898b",
            "55b2b4851e2d1572828bba8202f8514caae3d15a333433f6df49b07e116e92ac",
        ),
        // The control: `method=5` is another file, so the pin can fail.
        (
            61,
            37,
            1,
            5,
            "9e7e474e8090cee8a91e4fb8983673346436b9d8ba46fef442c5f82111051b6c",
            "3c420abe2c6622227af1df2a913d40c516dea556be545c46f035920bdcf1d763",
        ),
        (
            200,
            3,
            9,
            6,
            "0ece72e826e4a3e554d341ef7a41b5b729495730d90d6a9e0fefacd92b0fc77c",
            "8079c2da24cd69a6cea36debe51b12267a4d78eae64625266c3521f0d42e443e",
        ),
    ] {
        let s = webp::Settings { method, ..SETTINGS };
        let b = webp::encode(&rgba(w, h, seed), w, h, s).expect("encodes");
        assert_eq!(sha(&b), file, "{w}x{h} seed {seed} method {method}");
        let (dw, dh, px) = webp::decode(&b).expect("decodes");
        assert_eq!((dw, dh), (w, h));
        assert_eq!(sha(&px), read, "{w}x{h} read back");
    }
}

/// A GIF of three 4 by 2 graphics, a global table with entries near the
/// dark page, a local one on the second, as the Python built it.
fn gif_fixture(stray: bool, cut: bool) -> Vec<u8> {
    let mut b = b"GIF89a".to_vec();
    b.extend(4u16.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend([0x81, 0, 0]);
    // Entries 5 and 6 levels off the page, then far; the local table one
    // 1 off and one 7 off.
    b.extend([
        0x0D, 0x11, 0x17, 0x10, 0x15, 0x1C, 0x13, 0x11, 0x17, 0xF0, 0x80, 0x10,
    ]);
    for (k, dl) in [2u16, 4, 2].into_iter().enumerate() {
        b.extend([0x21, 0xF9, 4, 0x04]);
        b.extend(dl.to_le_bytes());
        b.extend([0, 0, 0x2C]);
        for v in [0u16, 0, 4, 2] {
            b.extend(v.to_le_bytes());
        }
        b.push(if k == 1 { 0x80 } else { 0 });
        if k == 1 {
            b.extend([0x0E, 0x12, 0x18, 0x0D, 0x18, 0x17]);
        }
        b.extend([2, 2, 0x44, 0x01, 0]);
        if stray && k == 0 {
            b.push(0x99);
        }
    }
    if !cut {
        b.push(b';');
    }
    b
}

const DARK: [u8; 3] = [0x0D, 0x11, 0x17];

#[test]
fn the_gif_edits_are_the_drivers() {
    assert_eq!(
        gif::blocks(&gif_fixture(false, false)).unwrap(),
        [
            gif::Block::Table(13, 4),
            gif::Block::Gce(29),
            gif::Block::Gce(52),
            gif::Block::Table(66, 2),
            gif::Block::Gce(81),
        ]
    );
    let b = gif::rewrite(&gif_fixture(false, false), &[3, 2, 5, 2], DARK, "dark").unwrap();
    assert_eq!(
        sha(&b),
        "efac30e1623f65d0e7402c88413fbb511a0ca8f8bcb4de9971469a4451f2bb4d"
    );
    // The perturbations, each the driver's refusal.
    assert_eq!(
        gif::rewrite(&gif_fixture(false, false), &[3, 2, 5, 2, 2], DARK, "dark").unwrap_err(),
        "dark: gifski's frames span 4 of the film's 5"
    );
    assert_eq!(
        gif::rewrite(&gif_fixture(true, false), &[3, 2, 5, 2], DARK, "dark").unwrap_err(),
        "a GIF block 0x99 at 48"
    );
    assert_eq!(
        gif::rewrite(&gif_fixture(false, true), &[3, 2, 5, 2], DARK, "dark").unwrap_err(),
        "a GIF with no trailer"
    );
}

#[test]
fn a_gif_loops_where_pillow_finds_a_netscape_block() {
    let plain = gif_fixture(false, false);
    let mut looped = plain[..25].to_vec();
    looped.extend([0x21, 0xFF, 11]);
    looped.extend(b"NETSCAPE2.0");
    looped.extend([3, 1, 0, 0, 0]);
    looped.extend(&plain[25..]);
    assert_eq!(
        sha(&looped),
        "18f39e1ca6b3ed4016c9d6135301b5905b3d33b1f0b115f5c8dfe392e81f72df"
    );
    assert!(!gif::loops(&plain));
    assert!(gif::loops(&looped));
    // What Pillow left out of `im.info`: another version's name, a first
    // byte not 1, a sub-block under 3 bytes, a block after the first image.
    let ext = |id: &[u8], sub: &[u8]| {
        let mut b = vec![0x21, 0xFF, 11];
        b.extend(id);
        b.extend(sub);
        b
    };
    for e in [
        ext(b"NETSCAPE1.0", &[3, 1, 0, 0, 0]),
        ext(b"NETSCAPE2.0", &[3, 2, 0, 0, 0]),
        ext(b"NETSCAPE2.0", &[2, 1, 0, 0]),
    ] {
        let mut g = plain[..25].to_vec();
        g.extend(&e);
        g.extend(&plain[25..]);
        assert!(!gif::loops(&g), "{e:?}");
    }
    let mut late = plain[..plain.len() - 1].to_vec();
    late.extend(ext(b"NETSCAPE2.0", &[3, 1, 0, 0, 0]));
    late.push(b';');
    assert!(!gif::loops(&late), "after the first image");
}

/// The bare canvas, a word drawn in its box, and four frames of noise
/// over it then the bare one, as the Python built them.
fn canvases(page: [u8; 3], word: [u8; 3], n: usize) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut bare = page.repeat(W * H);
    let (x0, y0, x1, y1) = (205, 220, 496, 344);
    let mid = [
        ((u16::from(page[0]) + u16::from(word[0])) / 2) as u8,
        ((u16::from(page[1]) + u16::from(word[1])) / 2) as u8,
        ((u16::from(page[2]) + u16::from(word[2])) / 2) as u8,
    ];
    for y in y0 + 20..y1 - 20 {
        for x in x0 + 10..x1 - 10 {
            let i = (y * W + x) * 3;
            match (x / 9 + y / 13) % 3 {
                0 => bare[i..i + 3].copy_from_slice(&word),
                1 if (x + y) % 4 == 0 => bare[i..i + 3].copy_from_slice(&mid),
                _ => {}
            }
        }
    }
    let mut r = Lcg(5);
    let mut frames = Vec::new();
    for k in 0..n {
        let mut f = bare.clone();
        for _ in 0..300 + 40 * k {
            let x = (usize::from(r.byte()) * 4 + usize::from(r.byte())) % W;
            let y = (usize::from(r.byte()) * 3 + usize::from(r.byte())) % H;
            let v = r.byte();
            let i = (y * W + x) * 3;
            f[i..i + 3].copy_from_slice(&[v, 255 - v, v.wrapping_mul(3)]);
        }
        // 1 to 4 levels off the page: the snap's edge and the haze's.
        for j in 0..40u8 {
            let x = 200 + usize::from(r.byte());
            let y = 210 + usize::from(r.byte()) / 2;
            let i = (y * W + x) * 3;
            let d = 1 + j % 4;
            let c0 = if page[0] < 250 {
                page[0] + d
            } else {
                page[0] - d
            };
            f[i..i + 3].copy_from_slice(&[c0, page[1], page[2]]);
        }
        frames.push(f);
    }
    frames.push(bare.clone());
    (bare, frames)
}

const DELAYS: [u32; 5] = [2, 3, 2, 4, 25];

#[test]
fn the_snap_the_haze_the_contrast_and_the_svg_are_the_drivers() {
    for (page, word, raw, contrast, snap, haze, svg, svg_len, seen) in [
        (
            DARK,
            [0xF3, 0xF4, 0xF6],
            (17.010198209515533, 17.104219396980486),
            (17.01, 17.1),
            "82307d83cf416d30bd015e20d8555033e3d4de6f9477e59c54d314950364b1a4",
            (0.014, 242),
            "740b185e9d8f4502ef8d0d59f520301c208e70bdda1412e1fc3ebbaf993eb4fa",
            48097,
            "2e80490eab08786a24efdbc32c9f7c414ec78a1e7cb60319c507f19d945a34ce",
        ),
        (
            [0xFF, 0xFF, 0xFF],
            [0x1E, 0x21, 0x28],
            (15.788064422071582, 15.827408244672716),
            (15.79, 15.83),
            "5e0e8745f1304c178f352e16bf25e2bbe58f48faf7eabf960371ab1ddf2c40dc",
            (0.014, 255),
            "a0cc425e6fecf9757bc7208c345cc75abc2bebfe21f79148b10eb979c4aed30f",
            48657,
            "a8a5a859ad6e907666e8522430c5bb036bd197e4fa23b46e59e54cd07787641a",
        ),
    ] {
        let (bare, frames) = canvases(page, word, 4);
        let all: Vec<&[u8]> = frames.iter().map(Vec::as_slice).collect();
        // Every bit: NumPy's pairwise mean and its `pow`, not only the two
        // decimals the line prints.
        let rs = logo::ratios(&all, &bare, [205, 220, 496, 344], page, word);
        assert_eq!((rs[0], rs[rs.len() / 2]), raw, "{page:?}: the raw ratios");
        assert_eq!(
            logo::contrast(&all, &bare, [205, 220, 496, 344], page, word),
            contrast,
            "{page:?}: the contrast"
        );
        let snapped = logo::snapped(&frames.concat(), frames.len(), page).unwrap();
        assert_eq!(sha(&snapped.concat()), snap, "{page:?}: the snap");
        let got = vec![
            (snapped[0].clone(), 5),
            (frames[2].clone(), 2),
            (snapped[3].clone(), 4),
            (snapped[4].clone(), 25),
        ];
        assert_eq!(
            logo::haze(&snapped, &got, page, &DELAYS),
            haze,
            "{page:?}: the haze"
        );
        let s = logo::svg(&snapped, &snapped[snapped.len() - 1], &DELAYS).unwrap();
        assert_eq!(
            (s.len(), sha(&s).as_str()),
            (svg_len, svg),
            "{page:?}: the SVG"
        );
        let back = logo::svg_frames(&s, &DELAYS).unwrap();
        assert_eq!(sha(&back.concat()), seen, "{page:?}: the SVG as composed");
        // The perturbations: a frame that ends late, and one at no start.
        let text = String::from_utf8(s).unwrap();
        let at = |cs: u32| logo::key_time(cs, 36);
        let late = text.replacen(
            &format!("keyTimes=\"0;{};{}\"", at(2), at(5)),
            &format!("keyTimes=\"0;{};{}\"", at(2), at(6)),
            1,
        );
        assert_ne!(late, text, "the late end is made");
        assert_eq!(
            logo::svg_frames(late.as_bytes(), &DELAYS).unwrap_err(),
            "the SVG's frame at 2 cs ends at 6, not 5"
        );
        let off = text.replacen(
            &format!("keyTimes=\"0;{};{}\"", at(5), at(7)),
            &format!("keyTimes=\"0;{};{}\"", at(6), at(7)),
            1,
        );
        assert_ne!(off, text, "the stray start is made");
        assert_eq!(
            logo::svg_frames(off.as_bytes(), &DELAYS).unwrap_err(),
            "the SVG shows frames at no frame's start: [6]"
        );
        // A third: an animation that repeats over another length.
        let short = text.replacen("dur=\"0.36s\"", "dur=\"0.35s\"", 1);
        assert_ne!(short, text, "the short loop is made");
        let refused = logo::svg_frames(short.as_bytes(), &DELAYS).unwrap_err();
        assert!(
            refused.contains("does not repeat over the film's 36 cs"),
            "{refused}"
        );
    }
}

/// Frames that change a small box and the canvas's two corners: the box
/// and its margin, clipped at the edges.
#[test]
fn an_svg_frame_is_the_box_it_changes_and_two_pixels_round() {
    let bare = DARK.repeat(W * H);
    let put = |f: &mut [u8], x: usize, y: usize, c: [u8; 3]| {
        f[(y * W + x) * 3..(y * W + x) * 3 + 3].copy_from_slice(&c);
    };
    let mut a = bare.clone();
    for y in 100..104 {
        for x in 200..210 {
            put(&mut a, x, y, [250, 120, 30]);
        }
    }
    let mut c = bare.clone();
    for y in 0..2 {
        for x in 0..3 {
            put(&mut c, x, y, [9, 200, 77]);
        }
    }
    put(&mut c, W - 1, H - 1, [1, 2, 3]);
    let s = logo::svg(&[a, c, bare.clone()], &bare, &[2, 2, 25]).unwrap();
    assert_eq!(
        (s.len(), sha(&s).as_str()),
        (
            5409,
            "b7d7f20d1f2ef86fb33f24c4cdea0f72cc622ccd50b8be2d83b34d4c0bb0e2d5"
        )
    );
}

/// A frame with every alpha from 0 to 255 over a noisy ground, composed
/// as the driver composed it.
#[test]
fn a_frames_alpha_composes_as_the_drivers_numpy_did() {
    let (w, h) = (64, 4);
    let mut px = Vec::new();
    for k in 0..w * h {
        px.extend([
            ((k * 7) % 256) as u8,
            ((k * 13 + 40) % 256) as u8,
            (255 - k) as u8,
            k as u8,
        ]);
    }
    let file = webp::encode(&px, w, h, SETTINGS).unwrap();
    assert_eq!(
        sha(&file),
        "2effb3d00f17ab81b3bf8457138fd0b94682563c5824f8a432956b85c67e975d"
    );
    let mut ground = vec![0u8; W * H * 3];
    let mut r = Lcg(11);
    for y in 90..110 {
        for x in 30..110 {
            for c in 0..3 {
                ground[(y * W + x) * 3 + c] = r.byte();
            }
        }
    }
    let base = png::encode(&png::Image {
        w: W,
        h: H,
        mode: png::Mode::Rgb,
        px: ground,
    });
    let t = format!(
        "<image width=\"{W}\" height=\"{H}\" href=\"data:image/png;base64,{}\"/>\n\
         <image x=\"40\" y=\"100\" width=\"{w}\" height=\"{h}\" visibility=\"hidden\" \
         href=\"data:image/webp;base64,{}\"><animate attributeName=\"visibility\" \
         calcMode=\"discrete\" values=\"visible;hidden\" keyTimes=\"0;0.40000\" dur=\"0.05s\" \
         repeatCount=\"indefinite\"/></image>",
        logo::b64encode(&base),
        logo::b64encode(&file)
    );
    assert_eq!(
        sha(t.as_bytes()),
        "a0b2e7bcfbe73cbf04694b7eee78a404cac916b6ab0695886788b4c63f942502",
        "the same SVG"
    );
    let seen = logo::svg_frames(t.as_bytes(), &[2, 3]).unwrap();
    assert_eq!(
        sha(&seen[0]),
        "88e61433f07b807da10bee62ca8c250cf52a545eb40a2d135d8866f74fd29bab"
    );
}

#[test]
fn the_checks_on_the_files_fire_in_the_drivers_words() {
    assert!(logo::gif_holds("dark", 475, 475, true, (W, H), 4_999_999).is_ok());
    for (args, words) in [
        (
            (474, true, (W, H), 100),
            "dark: the GIF lasts 474 cs, not the film's 475",
        ),
        (
            (475, false, (W, H), 100),
            "dark: loops False, 840x580, 100 bytes",
        ),
        (
            (475, true, (840, 581), 100),
            "dark: loops True, 840x581, 100 bytes",
        ),
        (
            (475, true, (W, H), 5_000_000),
            "dark: loops True, 840x580, 5000000 bytes",
        ),
    ] {
        let (lasts, loops, size, bytes) = args;
        assert_eq!(
            logo::gif_holds("dark", lasts, 475, loops, size, bytes).unwrap_err(),
            words
        );
    }
    assert!(logo::contrast_holds("light", 4.5).is_ok());
    assert_eq!(
        logo::contrast_holds("light", 4.49).unwrap_err(),
        "light: the word's contrast falls to 4.49 to 1"
    );
    let report = |o: &str, s: &str| {
        json::parse(&format!(
            "{{\"outcome\": \"{o}\", \"strict_outcome\": \"{s}\", \"area\": 604.0}}"
        ))
        .unwrap()
    };
    assert!(logo::census_holds("dark", &report("passes", "passes")).is_ok());
    assert_eq!(
        logo::census_holds("dark", &report("fails", "passes")).unwrap_err(),
        "dark: the SVG's census fails: {'outcome': 'fails', 'strict_outcome': 'passes', 'area': 604.0}"
    );
    assert!(logo::census_holds("dark", &report("passes", "fails")).is_err());
    let film = |w: u32, half: &str, frames: u32| {
        json::parse(&format!(
            "{{\"half\": \"{half}\", \"width\": {w}, \"height\": 580, \"frames\": {frames}, \"delays_cs\": [2, 3]}}"
        ))
        .unwrap()
    };
    assert!(logo::size_half(&film(840, "dark", 2), "dark", "the film").is_ok());
    assert_eq!(
        logo::size_half(&film(841, "dark", 2), "dark", "the film").unwrap_err(),
        "dark: the film is 841x580 of dark"
    );
    assert_eq!(
        logo::size_half(&film(840, "light", 2), "dark", "the bare canvas").unwrap_err(),
        "dark: the bare canvas is 840x580 of light"
    );
    assert!(logo::frames_match(&film(840, "dark", 2), "dark").is_ok());
    assert_eq!(
        logo::frames_match(&film(840, "dark", 3), "dark").unwrap_err(),
        "dark: 3 frames and 2 delays"
    );
}

/// The walls' four counts, by hand: one pixel 2 levels off, one more 1
/// off, one ink pixel crossing the half-coverage mask by 2.0.
#[test]
fn the_walls_off_counts_are_the_drivers() {
    let walls = [0, 0, 0, 9, 9, 9, 5, 5, 5];
    let today = [0, 0, 0, 9, 7, 9, 5, 5, 6];
    // 0.0 is not inside the ink, on either side.
    let (sa, sb) = ([-1.0f32, 1.0, 0.0], [1.0f32, 1.1251, 0.5]);
    let off = logo::walls_off(&walls, &today, &sa, &sb);
    assert_eq!(
        off,
        logo::Off {
            pixels: 2,
            levels: 2,
            mask: 1,
            sdf: 2.0
        }
    );
    assert!(off.past(&logo::WALLS_OFF), "2.0 is past the 1.0 allowed");
    assert!(!logo::WALLS_OFF.past(&logo::WALLS_OFF));
    for moved in [
        logo::Off {
            levels: 3,
            ..logo::WALLS_OFF
        },
        logo::Off {
            mask: 4,
            ..logo::WALLS_OFF
        },
        logo::Off {
            sdf: 1.001,
            ..logo::WALLS_OFF
        },
    ] {
        assert!(moved.past(&logo::WALLS_OFF), "{moved:?}");
    }
}

/// **The word held to the walls and the mark to where it moved**, on the
/// committed walls: today's the walls with the mark moved as `MARK_MOVED`
/// says passes; the walls themselves, the mark not moved, and the mark moved
/// a row further are refused by name, and a word moved a pixel is past
/// `WALLS_OFF`.
#[test]
fn the_word_stays_on_the_walls_and_the_mark_moves_as_it_says() {
    let (w, h) = (W, H);
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/assets/readme/logo-walls.png"
    ))
    .unwrap();
    let walls = png::decode(&bytes).unwrap();
    let walls: Vec<u8> = match walls.mode {
        png::Mode::Rgb => walls.px,
        png::Mode::Rgba => walls
            .px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
    };
    let moved = |extra: usize| -> Vec<u8> {
        let mut out = walls.clone();
        for m in logo::MARK_MOVED {
            for x in m.columns.0..m.columns.1 {
                for y in 0..h {
                    let from = y + m.rows + extra;
                    let (to, from) = ((y * w + x) * 3, (from.min(h - 1) * w + x) * 3);
                    let px = [walls[from], walls[from + 1], walls[from + 2]];
                    out[to..to + 3].copy_from_slice(&px);
                }
            }
        }
        out
    };
    let page = vec![1.0f32; w * h];
    let ok = moved(0);
    let (off, bad) = logo::lockup_faults(&walls, &ok, &page, &page);
    assert_eq!((off.pixels, off.levels), (0, 0));
    assert!(bad.is_empty(), "{bad:?}");

    let (_, bad) = logo::lockup_faults(&walls, &walls, &page, &page);
    assert_eq!(bad.len(), 2, "not moved: {bad:?}");
    let (_, bad) = logo::lockup_faults(&walls, &moved(1), &page, &page);
    assert_eq!(bad.len(), 2, "a row further: {bad:?}");
    assert!(bad[0].starts_with("columns (510, 562) are "), "{bad:?}");

    let mut word = ok.clone();
    for y in 0..h {
        for x in 1..logo::WORD_COLUMNS {
            let (to, from) = ((y * w + x) * 3, (y * w + x - 1) * 3);
            let px = [ok[from], ok[from + 1], ok[from + 2]];
            word[to..to + 3].copy_from_slice(&px);
        }
    }
    let (off, _) = logo::lockup_faults(&walls, &word, &page, &page);
    assert!(off.past(&logo::WALLS_OFF), "{off:?}");
}

#[test]
fn the_walls_are_the_pinned_file_or_refused() {
    let walls = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/assets/readme/logo-walls.png"
    ))
    .unwrap();
    assert!(logo::walls_pinned(&walls).is_ok());
    let mut flipped = walls;
    flipped[100] ^= 1;
    assert_eq!(
        logo::walls_pinned(&flipped).unwrap_err(),
        "docs/assets/readme/logo-walls.png is not the walls the path was chosen against"
    );
}

#[test]
fn check_says_same_only_of_the_same_bytes() {
    let path = "docs/assets/readme/logo-dark.gif";
    assert_eq!(
        logo::compared(path, Some(b"GIF"), b"GIF"),
        ("same    docs/assets/readme/logo-dark.gif".to_string(), None)
    );
    let differs = (
        "DIFFERS docs/assets/readme/logo-dark.gif".to_string(),
        Some("docs/assets/readme/logo-dark.gif is not what the film and encoders make".to_string()),
    );
    // A byte past the trailer, and a file not there at all.
    assert_eq!(logo::compared(path, Some(b"GIFx"), b"GIF"), differs);
    assert_eq!(logo::compared(path, None, b"GIF"), differs);
}

#[test]
fn logos_arguments_are_check_out_and_keep_and_no_other() {
    use logo::Files;
    let a = |v: &[&str]| logo::args(&v.iter().map(ToString::to_string).collect::<Vec<_>>());
    assert_eq!(a(&[]).unwrap(), (Files::Write, None));
    assert_eq!(a(&["--check"]).unwrap(), (Files::Check, None));
    assert_eq!(
        a(&["--keep", "d", "--check"]).unwrap(),
        (Files::Check, Some("d".into()))
    );
    assert_eq!(
        a(&["--out", "o", "--keep", "d"]).unwrap(),
        (Files::Out("o".into()), Some("d".into()))
    );
    for bad in [
        &["--frob"][..],
        &["--keep"][..],
        &["--out"][..],
        &["--check", "x"][..],
        &["--check", "--out", "o"][..],
        &["--out", "o", "--check"][..],
    ] {
        assert_eq!(
            a(bad).unwrap_err(),
            "usage: baud-pictures logo [--check | --out DIR] [--keep DIR]"
        );
    }
}

/// **The plasma never rests and the wrap is no jump**, on canvases that
/// differ by hand: a frame that is the bare logo is refused, and so is a
/// wrap that moves more than the busiest step; the block's box is left out
/// of the count, as the blink alone moves it, and a change of no more than
/// [`logo::MOVED`] levels counts for nothing.
#[test]
fn the_wrap_and_the_rest_are_held() {
    let still = DARK.repeat(W * H);
    let block = [100, 100, 110, 110];
    let lit = |x0: usize, n: usize| {
        let mut f = still.clone();
        for x in x0..x0 + n {
            f[(10 * W + x) * 3] = 0xFF;
        }
        f
    };
    let frames = vec![lit(0, 4), lit(2, 4), lit(4, 4)];
    assert_eq!(logo::moved(&frames[0], &still, block), 4);
    assert_eq!(
        logo::wrap_holds("dark", &frames, &still, block).unwrap_err(),
        "dark: the wrap moves 8 pixels, past the film's busiest step's 4"
    );
    let closing = vec![lit(0, 4), lit(2, 4), lit(4, 4), lit(2, 4)];
    assert_eq!(
        logo::wrap_holds("dark", &closing, &still, block),
        Ok((4, 4, 4))
    );
    let mut bare = closing.clone();
    bare[2] = still.clone();
    assert_eq!(
        logo::wrap_holds("dark", &bare, &still, block).unwrap_err(),
        "dark: frame 2 is the bare logo"
    );
    // The block's box does not count, and neither does a change of MOVED.
    let mut in_box = still.clone();
    in_box[(105 * W + 105) * 3] = 0xFF;
    assert_eq!(logo::moved(&in_box, &still, block), 0);
    let mut faint = still.clone();
    faint[(10 * W + 10) * 3] = DARK[0] + logo::MOVED;
    assert_eq!(logo::moved(&faint, &still, block), 0);
    faint[(10 * W + 10) * 3] = DARK[0] + logo::MOVED + 1;
    assert_eq!(logo::moved(&faint, &still, block), 1);
}

#[test]
fn a_film_with_a_byte_past_its_frames_is_refused() {
    let raw = vec![0u8; W * H * 3 * 2 + 1];
    assert_eq!(
        logo::snapped(&raw, 2, DARK).unwrap_err(),
        "frames.rgb holds more than the film's frames"
    );
}

#[test]
fn the_walls_line_prints_the_drivers_dict() {
    let off = logo::Off {
        sdf: baud_pictures::py::round_to(f64::from(0.1251f32), 3),
        ..logo::WALLS_OFF
    };
    assert_eq!(
        off.repr(),
        "{'pixels': 3873, 'levels': 2, 'mask': 2, 'sdf': 0.125}"
    );
    assert!(!logo::WALLS_OFF.past(&logo::WALLS_OFF));
    // The perturbation: one pixel more than the walls allow.
    let moved = logo::Off {
        pixels: 3874,
        ..logo::WALLS_OFF
    };
    assert!(moved.past(&logo::WALLS_OFF));
}

#[test]
fn the_gifski_frames_read_back_as_the_film() {
    let (_, frames) = canvases(DARK, [0xF3, 0xF4, 0xF6], 1);
    let p = png::decode(&logo::quick_png(&frames[0])).unwrap();
    assert_eq!((p.w, p.h, p.mode), (W, H, png::Mode::Rgb));
    assert_eq!(p.px, frames[0]);
}

#[test]
fn base64_is_pythons() {
    for n in 0..7 {
        let data: Vec<u8> = (0..n).map(|i| (i * 77 + 3) as u8).collect();
        let t = logo::b64encode(&data);
        assert_eq!(t.len() % 4, 0);
        assert_eq!(logo::b64decode(&t).unwrap(), data);
    }
    assert_eq!(logo::b64encode(b"baud"), "YmF1ZA==");
    assert!(
        logo::b64decode("YmF1ZA=").is_err(),
        "the padding is checked"
    );
}
