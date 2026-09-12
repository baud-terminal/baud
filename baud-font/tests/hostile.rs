//! The hostile-input contract: no byte string panics.
//!
//! A font file is the same class of input as bytes from a pty -- whatever the
//! other end sends -- and the crate is `forbid(unsafe_code)`, so the only
//! failure a malformed file can produce is a panic, and the property here is
//! that it does not. Two generators: arbitrary bytes, which exercise the
//! directory and the version checks, and a real face with a handful of bytes
//! flipped, which reaches the table parsers with offsets and counts that lie.
//!
//! `PROPTEST_CASES=20000` before trusting a change here; the default is what
//! CI can afford.

use baud_domain::Codepoint;
use baud_font::{Face, FaceIndex, faces};
use proptest::prelude::*;

const JETBRAINS: &[u8] = include_bytes!("fonts/JetBrainsMono-Regular.ttf");

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
        }
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

proptest! {
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
