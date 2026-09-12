//! The hostile-input contract: no byte string panics.
//!
//! A font file is the same class of input as bytes from a pty -- whatever the
//! other end sends -- and the crate is `forbid(unsafe_code)`, so the only
//! failure a malformed file can produce is a panic, and the property here is
//! that it does not. Two generators: arbitrary bytes, which exercise the
//! directory and the version checks, and a real face with a handful of bytes
//! flipped, which reaches the table parsers with offsets and counts that lie.
//! The PNG decoder gets the same pair: arbitrary bytes, and a real strike
//! PNG mutated and truncated, because an inflater is the deepest state
//! machine in the crate and the one a hostile file can steer furthest.
//!
//! `PROPTEST_CASES=20000` before trusting a change here; the default is what
//! CI can afford.

use baud_domain::Codepoint;
use baud_domain::GlyphId;
use baud_font::bytes::Index;
use baud_font::{
    Cluster, Face, FaceIndex, Features, Glyph, Ignorable, Palette, Png, Shaper, faces,
};
use proptest::prelude::*;

const JETBRAINS: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");
const NOTO_SUBSET: &[u8] = include_bytes!("fonts/NotoColorEmoji-subset.ttf");

/// A sink that discards everything: the property is that emitting does not
/// panic, not what is emitted.
struct Null;

impl baud_font::Sink for Null {
    fn move_to(&mut self, _: f32, _: f32) {}
    fn line_to(&mut self, _: f32, _: f32) {}
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {}
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {}
    fn close(&mut self) {}
}

/// Everything a caller can ask a face, on a fixed set of inputs.
fn exercise(data: &[u8]) {
    let n = faces(data).map(|n| n.value()).unwrap_or(1).min(8);
    for i in 0..n {
        let Ok(face) = Face::parse_index(data, FaceIndex::of(i)) else {
            continue;
        };
        let _ = face.units_per_em();
        let _ = face.line_metrics();
        let _ = face.typo_metrics();
        let _ = face.win_metrics();
        let _ = face.underline();
        let _ = face.pitch();
        let count = u32::from(face.glyph_count().value()).min(4096);
        for g in 0..count {
            let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
            let _ = face.advance(g);
            let _ = face.side_bearing(g);
            let _ = face.outline(g, &mut Null);
        }
        let strikes = face.strikes().value().min(16);
        for st in 0..strikes {
            let st = Index::of(st);
            let _ = face.strike_ppem(st);
            for g in 0..count.min(256) {
                let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
                if let Some(b) = face.bitmap(st, g) {
                    decode(b.png.as_slice());
                }
            }
        }
        for g in 0..count {
            let g = baud_domain::GlyphId::from_shaper(u16::try_from(g).unwrap());
            if let Some(layers) = face.layers(g) {
                for l in layers.take(64) {
                    let _ = face.color(Palette::FIRST, l.color);
                }
            }
        }
        let _ = face.palettes();
        shape_some(&face);
        for c in [
            0x20u32, 0x41, 0x7E, 0xE9, 0x4E2D, 0x1F600, 0x1F468, 0xFFFF, 0x10FFFF,
        ] {
            let Some(c) = Codepoint::new(c) else { continue };
            let _ = face.glyph(c);
            for vs in [0xFE0E_u32, 0xFE0F, 0xE0100] {
                let Some(vs) = Codepoint::new(vs) else {
                    continue;
                };
                let _ = face.variation(c, vs);
            }
        }
    }
}

/// Shape a handful of clusters that reach every substitution path: a
/// ligature trigger, a composed pair, a ZWJ sequence, a variation
/// selector, and an unmapped character. The property is that no table
/// contents can make the shaper panic, not what it emits.
fn shape_some(face: &Face<'_>) {
    const CLUSTERS: &[&[u32]] = &[
        &[0x61],
        &[0x2D],
        &[0x3E],
        &[0x65, 0x0301],
        &[0x1F468, 0x200D, 0x1F469],
        &[0x263A, 0xFE0F],
        &[0x10FFFF],
        &[0xAC00],
    ];
    let mut storage = [Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 64];
    let mut shaper = Shaper::new(face, &mut storage);
    for (i, cps) in CLUSTERS.iter().enumerate() {
        let cps: Vec<Codepoint> = cps.iter().filter_map(|&v| Codepoint::new(v)).collect();
        let _ = shaper.push_cluster(&cps, Cluster(u16::try_from(i).unwrap()));
    }
    let _ = shaper.finish(Features::GRID);
}

/// Decode a PNG the way a renderer would, with the size-bomb check the
/// register's hazard 7 asks for: an `IHDR` may claim any size, so the
/// buffers are refused above a bound rather than allocated on trust.
fn decode(bytes: &[u8]) {
    const BOUND: usize = 1 << 22;
    let Ok(png) = Png::parse(bytes) else { return };
    let _ = png.size();
    let (Some(raw), Some(rgba)) = (png.raw_len(), png.rgba_len()) else {
        return;
    };
    if raw > BOUND || rgba > BOUND {
        return;
    }
    let mut raw = vec![0u8; raw];
    let mut rgba = vec![0u8; rgba];
    let _ = png.decode(&mut raw, &mut rgba);
}

/// One real PNG to mutate: the first bitmap of the committed CBDT subset.
fn a_real_png() -> Vec<u8> {
    let face = Face::parse(NOTO_SUBSET).unwrap();
    face.bitmap(Index::ZERO, baud_domain::GlyphId::from_shaper(4))
        .expect("glyph 4 of the subset carries a bitmap")
        .png
        .as_slice()
        .to_vec()
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_decode_into_a_panic(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        decode(&bytes);
    }

    #[test]
    fn a_mutated_png_never_panics(
        at in 0usize..4096,
        byte in any::<u8>(),
        cut in 0usize..4096,
    ) {
        let mut png = a_real_png();
        let n = png.len();
        png[at % n] = byte;
        decode(&png);
        decode(&png[..cut % n]);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        exercise(&bytes);
    }

    #[test]
    fn a_mutated_face_never_panics(
        flips in proptest::collection::vec((0usize..JETBRAINS.len(), any::<u8>()), 1..16)
    ) {
        let mut data = JETBRAINS.to_vec();
        for (at, byte) in flips {
            data[at] = byte;
        }
        exercise(&data);
    }

    #[test]
    fn a_truncated_face_never_panics(len in 0usize..JETBRAINS.len()) {
        exercise(&JETBRAINS[..len]);
    }
}
