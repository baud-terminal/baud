//! **`frame`'s pixel hash on real frames: alike across reps, apart on one
//! pixel.** The bench draws each rep with a fresh renderer into the pixel
//! buffer the rep before it left, after every `read(2)`-sized read, and
//! with `FRAME_HASH=on` folds every frame into [`baud_bench::PixelHash`]
//! and requires every rep to come to the same hash. This draws btop's first
//! reads the same way, over the see-through page in the dark theme and the
//! light, small enough for the debug suite: two reps hash alike, the second
//! drawn over the first's last frame, and one pixel of one frame changed
//! hashes apart.

use baud_bench::PixelHash;
use baud_core::{Owned, Term};
use baud_domain::{BytePos, Chan, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Target, Theme};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// Every frame of one rep, each a copy of the buffer after its render.
fn rep(faces: &[Face<'_>], theme: &Theme, data: &[u8], pixels: &mut Vec<u32>) -> Vec<Vec<u32>> {
    let mut r = Renderer::new(
        Stack::new(faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        theme.clone(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .expect("metrics");
    let (w, h) = r.pixel_size();
    pixels.resize(usize::try_from(w.value() * h.value()).expect("a size"), 0);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut frames = Vec::new();
    for chunk in data.chunks(4096) {
        t.write_at(BytePos::ZERO, chunk);
        t.feed(BytePos::ZERO, ReadLen::from_index(chunk.len()));
        let mut target = Target::new(pixels, w, w, h).expect("a target");
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, Damage::Incremental);
        frames.push(target.pixels().to_vec());
    }
    frames
}

fn hash(frames: &[Vec<u32>]) -> PixelHash {
    frames.iter().fold(PixelHash::START, |h, f| h.frame(f))
}

#[test]
fn two_reps_hash_alike_and_one_pixel_apart_does_not() {
    let root = env!("CARGO_MANIFEST_DIR");
    let face = std::fs::read(format!(
        "{root}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .expect("the committed face");
    let faces = [Face::parse(&face).expect("a face")];
    let btop =
        std::fs::read(format!("{root}/../tests/recordings/utf8/btop.raw")).expect("the recording");
    let data = &btop[..btop.len().min(6 * 4096)];
    for (name, theme) in [
        (
            "dark at 179",
            Theme {
                bg_alpha: Chan::from_u8(179),
                ..Theme::dark()
            },
        ),
        (
            "light at 153",
            Theme {
                bg_alpha: Chan::from_u8(153),
                ..Theme::light()
            },
        ),
    ] {
        let mut pixels = Vec::new();
        let first = rep(&faces, &theme, data, &mut pixels);
        let second = rep(&faces, &theme, data, &mut pixels);
        assert_eq!(first.len(), 6, "{name}: a frame a read");
        let (a, b) = (hash(&first), hash(&second));
        assert_ne!(a, PixelHash::START, "{name}: frames were hashed");
        assert_eq!(
            PixelHash::agreed(&[a, b]),
            Some(a),
            "{name}: a fresh renderer over the last rep's buffer draws the same frames"
        );
        // A frame that drew nothing would hash alike on any change of the
        // blend: each one holds more than the page alone.
        for (k, f) in first.iter().enumerate() {
            assert!(
                f.iter().any(|&p| p != f[0]),
                "{name}: frame {k} is one colour"
            );
        }
        // The positive control: one pixel of one frame, its lowest bit.
        let mut changed = second.clone();
        let mid = changed[3].len() / 2;
        changed[3][mid] ^= 1;
        let c = hash(&changed);
        assert_ne!(c, a, "{name}: one pixel of the fourth frame");
        assert_eq!(
            PixelHash::agreed(&[a, c]),
            None,
            "{name}: the reps disagree"
        );
    }
}
