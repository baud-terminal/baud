//! **README's logo GIF as `kitten icat` sends it, drawn by the CPU
//! compositor on an opaque page and on the glass**: the first four of
//! kitten's commands (`baud-core/tests/kitten_icat/stream.txt`) -- the
//! first image as RGB, its gap, the second over it as RGBA, and the
//! animation run in loading mode (`s=2`) -- and the clock run to the
//! second frame.
//!
//! The second image's 52,460 transparent pixels are alpha blended over
//! frame `c=1`, "a full alpha blend (the default)" (`[[source:
//! kitty-graphics-rst]]`, "Transferring animation frame data"), so where
//! they are the first image shows, and the first covers the image with no
//! transparent index. Every pixel of the image is then opaque, and the
//! page or the glass beneath shows nowhere through it: each drawn pixel is
//! the GIF's colour at full alpha on both, while a blank cell beside it
//! carries the page's alpha. The expected pixels are composed here from
//! the GIF's own pixels, read by `kitten_icat/gif.rs`, never by the
//! renderer or baud's decoder.

mod support;

#[path = "../../baud-core/tests/kitten_icat/gif.rs"]
mod gif;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Chan, DecodeBudget, DecodeProgress, HostTime, ImageBudget, ImageMotion, ImagePx,
    ReadLen,
};
use baud_font::Face;
use baud_render::{Damage, Theme};
use support::{Canvas, WS, font, renderer};

const STREAM: &str = include_str!("../../baud-core/tests/kitten_icat/stream.txt");

/// The app's default glass, `[glass] dim_dark = 0.70` (`baud-app`'s
/// `config.rs`).
const GLASS: f32 = 0.70;

/// kitten's columns before the image, and the pixels into its first cell
/// (`X=5`).
const COL: usize = 15;
const X: usize = 5;

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325, |h, &x| {
        (h ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3)
    })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3 * 4 + 4);
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// A zlib stream of stored blocks (RFC 1950, RFC 1951 3.2.4): what
/// `o=z` asks the terminal to inflate, with nothing compressed.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut blocks = data.chunks(65_535).peekable();
    while let Some(b) = blocks.next() {
        out.push(u8::from(blocks.peek().is_none()));
        let n = u16::try_from(b.len()).unwrap();
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&(!n).to_le_bytes());
        out.extend_from_slice(b);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    out.extend_from_slice(&(b << 16 | a).to_be_bytes());
    out
}

fn logo() -> gif::Gif {
    gif::read(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../assets/readme/logo-dark.gif"
        ))
        .unwrap(),
    )
}

/// kitten's first four commands, each payload made from the GIF and held
/// to the digest of the one kitten sent.
fn first_four(g: &gif::Gif) -> Vec<u8> {
    let mut out = b"\r\x1b[15C".to_vec();
    let lines = STREAM.lines().filter(|l| !l.starts_with('#')).take(4);
    for (k, line) in lines.enumerate() {
        let w: Vec<&str> = line.split(' ').collect();
        out.extend_from_slice(b"\x1b_G");
        out.extend_from_slice(w[0].as_bytes());
        let data: Vec<u8> = match k {
            0 => g.images[0]
                .rgba
                .chunks(4)
                .flat_map(|p| [p[0], p[1], p[2]])
                .collect(),
            2 => g.images[1].rgba.clone(),
            _ => Vec::new(),
        };
        if w[1] != "-" {
            assert_eq!(
                fnv(&data),
                u64::from_str_radix(w[1], 16).unwrap(),
                "{}",
                w[0]
            );
            out.push(b';');
            out.extend_from_slice(&b64(&zlib_stored(&data)));
        }
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// The first image with the second's opaque pixels over it, as RGBA.
fn second_frame(g: &gif::Gif) -> Vec<u8> {
    let mut c = g.images[0].rgba.clone();
    let im = &g.images[1];
    for (j, p) in im.rgba.chunks(4).enumerate() {
        if p[3] == 255 {
            let at = 4 * ((im.y + j / im.w) * g.width + im.x + j % im.w);
            c[at..at + 4].copy_from_slice(p);
        }
    }
    c
}

/// One full frame under `theme` with the clock at `ms`, and the cell size.
fn drawn(g: &gif::Gif, theme: Theme, ms: u64) -> (Canvas, usize) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    r.set_theme(theme);
    let m = r.metrics();
    let cw = usize::try_from(m.cell_w.value()).unwrap();
    let mut c = Canvas::for_renderer(&r);
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(95 << 18));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(
        ImagePx::of(u16::try_from(m.cell_w.value()).unwrap()),
        ImagePx::of(u16::try_from(m.cell_h.value()).unwrap()),
    );
    let bytes = [b"\x1b[?25l".to_vec(), first_four(g)].concat();
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    }
    let at = |ms: u64| HostTime::of_nanos(1_000_000_000 + ms * 1_000_000);
    t.animate_images(at(0), ImageMotion::Play);
    t.animate_images(at(ms), ImageMotion::Play);
    {
        let mut target = c.target();
        r.render(&mut t, &mut target, Damage::Full);
    }
    (c, cw)
}

/// How many of the image's drawn pixels are not `want`'s colour at full
/// alpha, over what of the image the window shows, and that much.
fn misses(c: &Canvas, cw: usize, g: &gif::Gif, want: &[u8]) -> (usize, usize) {
    let x0 = COL * cw + X;
    let (w, h) = ((c.width - x0).min(g.width), c.height.min(g.height));
    let mut miss = 0;
    for y in 0..h {
        for x in 0..w {
            let p = &want[4 * (y * g.width + x)..][..4];
            let expect = u32::from_be_bytes([255, p[0], p[1], p[2]]);
            if c.pixels[y * c.width + x0 + x] != expect {
                miss += 1;
            }
        }
    }
    (miss, w * h)
}

#[test]
fn the_logo_draws_its_gifs_pixels_opaque_on_the_page_and_on_the_glass() {
    let g = logo();
    let want = second_frame(&g);
    assert!(
        want.chunks(4).all(|p| p[3] == 255),
        "no pixel of the frame is clear"
    );
    let glass = Theme {
        bg_alpha: Chan::from_unit(GLASS),
        ..Theme::dark()
    };
    for (name, theme, alpha) in [
        ("page", Theme::dark(), 255u8),
        ("glass", glass, Chan::from_unit(GLASS).get()),
    ] {
        let (c, cw) = drawn(&g, theme.clone(), 30);
        let (miss, seen) = misses(&c, cw, &g, &want);
        assert!(
            seen > 400_000,
            "{name}: {seen} pixels of the image in the window"
        );
        assert_eq!(miss, 0, "{name}: every pixel is the GIF's, opaque");
        let blank = c.pixels[(c.height - 1) * c.width + c.width - 1];
        assert_eq!(blank.to_be_bytes()[0], alpha, "{name}: the page beside it");
        // The control: a millisecond before its gap the first frame is
        // drawn, and the comparison sees it is not the second.
        let (c, cw) = drawn(&g, theme, 29);
        let (miss, _) = misses(&c, cw, &g, &want);
        assert!(miss > 10_000, "{name}: frame 1 is not frame 2 ({miss})");
    }
}
