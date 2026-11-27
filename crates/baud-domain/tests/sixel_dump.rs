//! **A sixel dump decodes back to the picture it printed**: the encoder of
//! `[[source: dec-std-070]]` 070-7 § 7.8.2's Level 2 dump checked against
//! the decoder baud already holds to 070-9, pixel for pixel, in each of the
//! graphics print choices, and its bytes pinned where the text fixes them.

use baud_domain::{
    Chan, DumpCanvas, DumpSpec, ImagePx, Pixel, PrintBackground, PrintColour, PrintRotation,
    PrintSize, PrintSyntax, Sixel, SixelAspect, SixelCanvas, SixelInk, SixelPx, SixelSelect,
    dec_hls, dump_room, encode_band, write_dump,
};
use proptest::prelude::*;

/// A picture as rows of colours, `None` the background.
struct Grid {
    size: (ImagePx, ImagePx),
    px: Vec<Option<Pixel>>,
}

impl DumpCanvas for Grid {
    fn at(&self, x: ImagePx, y: ImagePx) -> Option<Pixel> {
        self.px[y.index() * self.size.0.index() + x.index()]
    }
}

fn rgb(c: [u8; 3]) -> Pixel {
    Pixel::opaque(
        Chan::from_u8(c[0]),
        Chan::from_u8(c[1]),
        Chan::from_u8(c[2]),
    )
}

fn spec(g: &Grid) -> DumpSpec {
    DumpSpec {
        width: g.size.0,
        height: g.size.1,
        offset: (ImagePx::of(0), ImagePx::of(0)),
        size: PrintSize::Compressed,
        rotation: PrintRotation::Upright,
        colour: PrintColour::Colour,
        syntax: PrintSyntax::Rgb,
        background: PrintBackground::Off,
        index0: rgb([255, 255, 255]),
    }
}

fn dump(s: &DumpSpec, g: &Grid) -> Vec<u8> {
    let mut room = vec![0u8; dump_room(s)];
    let mut out = Vec::new();
    assert!(write_dump(s, g, &mut room, &mut |b| out.extend_from_slice(b)));
    out
}

/// The body between `q` and `ST`, as a printer reads it.
fn body(dump: &[u8]) -> &[u8] {
    let q = dump.iter().position(|&b| b == b'q').expect("an introducer");
    &dump[q + 1..dump.len() - 2]
}

/// Decode a dump as baud's printer does, an undefined register `ink`, on
/// a canvas the data's extent or `at_least`, whichever is larger.
fn decode(dump: &[u8], ink: Pixel, at_least: (u16, u16)) -> (u16, u16, Vec<Pixel>) {
    let data = body(dump);
    let e = Sixel::extent(data, SixelAspect::ONE_TO_ONE);
    let (w, h) = (
        e.width.raw().max(at_least.0),
        e.height.raw().max(at_least.1),
    );
    let mut px = vec![Pixel::CLEAR; usize::from(w) * usize::from(h)];
    let mut c = SixelCanvas::new(&mut px, SixelPx::of(w));
    Sixel::decode_inked(data, SixelAspect::ONE_TO_ONE, ink, &mut c);
    (w, h, px)
}

fn grid(size: (u16, u16), f: impl Fn(u16, u16) -> Option<Pixel>) -> Grid {
    let mut px = Vec::new();
    for y in 0..size.1 {
        for x in 0..size.0 {
            px.push(f(x, y));
        }
    }
    Grid {
        size: (ImagePx::of(size.0), ImagePx::of(size.1)),
        px,
    }
}

/// **A two-pixel monochrome dump, byte for byte**: SSU, `DCS 0;1;6 q`,
/// raster attributes at 1:1 with the extent, the sixels, `ST`; no `CR`
/// first, no register named.
#[test]
fn a_monochrome_dump_is_level_two_byte_for_byte() {
    let g = grid((3, 2), |x, y| (x != 1 && y == 0).then(|| rgb([9, 9, 9])));
    let mut s = spec(&g);
    s.colour = PrintColour::Mono;
    assert_eq!(dump(&s, &g), b"\x1b[2 I\x1bP0;1;6q\"1;1;3;2@?@\x1b\\");
}

/// **Expanded and rotated print at grid 9, background printing 2**, and the
/// colour specifiers for each register in use, register 0 only with the
/// background.
#[test]
fn the_grid_the_background_and_the_specifiers() {
    let g = grid((2, 1), |x, _| (x == 0).then(|| rgb([255, 0, 0])));
    let mut s = spec(&g);
    s.size = PrintSize::Expanded;
    let d = dump(&s, &g);
    assert!(
        d.starts_with(b"\x1b[2 I\x1bP0;1;9q\"1;1;2;1#1;2;100;0;0#1@"),
        "{:?}",
        String::from_utf8_lossy(&d)
    );
    s.size = PrintSize::Compressed;
    s.rotation = PrintRotation::Rotated;
    s.background = PrintBackground::On;
    s.syntax = PrintSyntax::Hls;
    let d = dump(&s, &g);
    let text = String::from_utf8_lossy(&d);
    assert!(
        text.starts_with("\x1b[2 I\x1bP0;2;9q\"1;1;1;2#0;1;0;100;0#1;1;120;50;100"),
        "{text:?}"
    );
}

/// **The colour round trip**: every pixel of a colour dump decodes to its
/// colour, the background to nothing; with the background printed, to
/// index 0's colour; in monochrome, every coloured pixel to the ink.
#[test]
fn a_dump_decodes_to_its_picture_in_each_choice() {
    let red = rgb([255, 0, 0]);
    let blue = rgb([0, 0, 255]);
    let g = grid((13, 9), |x, y| match (x + y) % 3 {
        0 => Some(red),
        1 => Some(blue),
        _ => None,
    });
    let mut s = spec(&g);
    let (w, h, px) = decode(&dump(&s, &g), Pixel::CLEAR, (0, 0));
    assert_eq!((w, h), (13, 12), "two bands");
    for y in 0..9u16 {
        for x in 0..13u16 {
            let want = g.at(ImagePx::of(x), ImagePx::of(y)).unwrap_or(Pixel::CLEAR);
            assert_eq!(px[usize::from(y) * 13 + usize::from(x)], want, "({x},{y})");
        }
    }
    s.background = PrintBackground::On;
    let (_, _, px) = decode(&dump(&s, &g), Pixel::CLEAR, (13, 9));
    assert_eq!(px[2], rgb([255, 255, 255]), "index 0 printed");
    s.colour = PrintColour::Mono;
    let ink = rgb([0, 0, 0]);
    let (_, _, px) = decode(&dump(&s, &g), ink, (13, 9));
    assert_eq!(
        (px[0], px[1], px[2]),
        (ink, ink, Pixel::CLEAR),
        "no background in monochrome"
    );
}

/// **Rotated print turns the picture counter-clockwise**: the screen's top
/// row runs up the paper's left edge (070-7 § 7.8.2.3); the offset leaves
/// blank pixels before it.
#[test]
fn rotation_turns_counter_clockwise_and_the_offset_is_blank() {
    let ink = rgb([0, 0, 0]);
    let g = grid((4, 2), |x, y| (y == 0 && x == 0).then_some(ink));
    let mut s = spec(&g);
    s.colour = PrintColour::Mono;
    s.rotation = PrintRotation::Rotated;
    let (w, h, px) = decode(&dump(&s, &g), ink, (2, 4));
    assert_eq!((w, h), (2, 6), "the raster's width, the band's height");
    // Screen (0,0) lands at paper (0, 3): the left edge, the bottom of
    // the four rows the screen's columns become.
    let lit: Vec<(u16, u16)> = (0..4u16)
        .flat_map(|y| (0..2u16).map(move |x| (x, y)))
        .filter(|&(x, y)| px[usize::from(y) * 2 + usize::from(x)] == ink)
        .collect();
    assert_eq!(lit, [(0, 3)]);
    s.rotation = PrintRotation::Upright;
    s.offset = (ImagePx::of(3), ImagePx::of(7));
    let d = dump(&s, &g);
    assert!(String::from_utf8_lossy(&d).contains("\"1;1;7;9"));
    let (_, _, px) = decode(&d, ink, (7, 9));
    let at = |x: usize, y: usize| px[y * 7 + x];
    assert_eq!(
        (at(3, 7), at(0, 0), at(4, 7)),
        (ink, Pixel::CLEAR, Pixel::CLEAR)
    );
}

/// **The offset is held to 17 inches of the dump's pixels each way**,
/// 2040 at grid 6 and 1360 at grid 9: past the largest common sheet it
/// would put the picture off the paper, and it is written as blank
/// pixels the raster attributes count.
#[test]
fn the_offset_is_held_to_a_sheet() {
    let ink = rgb([0, 0, 0]);
    let g = grid((4, 2), |x, y| (y == 0 && x == 0).then_some(ink));
    let mut s = spec(&g);
    s.colour = PrintColour::Mono;
    s.offset = (ImagePx::of(60_000), ImagePx::of(2040));
    let d = dump(&s, &g);
    assert!(String::from_utf8_lossy(&d).contains("6q\"1;1;2044;2042"));
    assert_eq!(dump_room(&s), 2044 * 6 * 2);
    s.size = PrintSize::Expanded;
    let d = dump(&s, &g);
    assert!(String::from_utf8_lossy(&d).contains("9q\"1;1;1364;1362"));
    let (w, _, px) = decode(&d, ink, (1364, 1362));
    assert_eq!(
        px[1360 * usize::from(w) + 1360],
        ink,
        "the picture after it"
    );
}

/// **Past 255 colours every colour takes the cube**: 252 registers named,
/// each pixel within half a cube step of its colour.
#[test]
fn past_255_colours_the_cube_is_named() {
    let g = grid((40, 10), |x, y| {
        Some(rgb([
            u8::try_from(x * 6).unwrap(),
            u8::try_from(y * 25).unwrap(),
            77,
        ]))
    });
    let s = spec(&g);
    let d = dump(&s, &g);
    let n = d.windows(3).filter(|w| w == b";2;").count();
    assert_eq!(n, 252);
    let (_, _, px) = decode(&d, Pixel::CLEAR, (40, 10));
    for y in 0..10u16 {
        for x in 0..40u16 {
            let want = g.at(ImagePx::of(x), ImagePx::of(y)).unwrap();
            let got = px[usize::from(y) * 40 + usize::from(x)];
            let (a, b) = (want.rgb(), got.rgb());
            let near = |p: Chan, q: Chan, step: i32| {
                (i32::from(p.get()) - i32::from(q.get())).abs() <= step
            };
            assert!(
                near(a.0, b.0, 26) && near(a.1, b.1, 22) && near(a.2, b.2, 26),
                "({x},{y}) {want:?} {got:?}"
            );
        }
    }
}

/// **DEC HLS puts blue at 0, red at 120, green at 240**
/// (`[[source: vt3xx-screen-control]]`), and the decoder takes it back.
#[test]
fn dec_hls_has_blue_at_zero() {
    assert_eq!(dec_hls(rgb([0, 0, 255])), (0, 50, 100));
    assert_eq!(dec_hls(rgb([255, 0, 0])), (120, 50, 100));
    assert_eq!(dec_hls(rgb([0, 255, 0])), (240, 50, 100));
    assert_eq!(dec_hls(rgb([128, 128, 128])), (0, 50, 0));
}

/// **A band sends each register's sixels with runs** and leaves its blank
/// tail off.
#[test]
fn a_band_runs_and_trims() {
    let mut band = vec![SixelInk::NONE; 6 * 9];
    band[..6].fill(SixelInk::register(3));
    band[9 + 7] = SixelInk::register(1);
    let mut out = Vec::new();
    encode_band(&band, 9, SixelSelect::Registers, &mut |b| {
        out.extend_from_slice(b)
    });
    assert_eq!(out, b"#1!7?A$#3!6@");
}

proptest! {
    /// **Any picture decodes back from its colour dump**, at any size,
    /// with any offset.
    #[test]
    fn any_picture_round_trips(
        w in 1u16..20, h in 1u16..20, ox in 0u16..8, oy in 0u16..8,
        cells in proptest::collection::vec(0u8..4, 400),
    ) {
        let palette = [None, Some(rgb([255, 0, 0])), Some(rgb([0, 255, 255])), Some(rgb([0, 0, 0]))];
        let g = grid((w, h), |x, y| palette[usize::from(cells[usize::from(y) * 20 + usize::from(x)])]);
        let mut s = spec(&g);
        s.offset = (ImagePx::of(ox), ImagePx::of(oy));
        let (dw, _, px) = decode(&dump(&s, &g), Pixel::CLEAR, (w + ox, h + oy));
        for y in 0..h {
            for x in 0..w {
                let want = g.at(ImagePx::of(x), ImagePx::of(y)).unwrap_or(Pixel::CLEAR);
                let i = usize::from(y + oy) * usize::from(dw) + usize::from(x + ox);
                prop_assert_eq!(px.get(i).copied().unwrap_or(Pixel::CLEAR), want);
            }
        }
    }
}
