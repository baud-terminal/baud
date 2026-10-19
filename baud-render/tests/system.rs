//! The macOS stack -- Menlo with Apple Color Emoji behind it -- painting
//! the emoji-dense recording, pinned as a digest of the frame and of the
//! two system files it was taken with, so a font the OS replaces fails
//! loudly rather than being compared. Runs on macOS and says so elsewhere.

mod support;

use baud_font::{Face, FaceIndex};
use baud_render::Damage;
use support::*;

const MENLO: &str = "/System/Library/Fonts/Menlo.ttc";
const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

/// FNV-1a over a file, the same digest `baud-font`'s morx fixtures use.
fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// `(menlo digest, emoji digest, frame digest)`; regenerate with
/// `GOLDEN_PRINT=1`. The frame moved when a cluster's marks that no face
/// draws stopped drawing as `.notdef` boxes (`Renderer::marks_unmapped`):
/// here the vowel signs of the recording's 18 Devanagari letters, which
/// neither Menlo nor Apple Color Emoji has: 18 cells changed, every one in
/// or just right of a letter whose vowel sign was left out. Every emoji
/// cluster is Apple Color Emoji's whole and drew as it did; drawn from the
/// base's face with nothing left out, the frame is its old pin again. It
/// moved again when the cluster store grew to 1024 slots: two wide kana,
/// U+30C5 and U+30C7, keep the U+3099 the 63-slot store refused them, and
/// all 128 pixels that changed are in those two cells. It moved again when
/// the CPU compositor began drawing a glyph's ink past its row into the
/// row it reaches, as the GPU paths draw it: 438 pixels in 138 cells, every
/// one on the first or last pixel row of a band. It moved again when a
/// precomposed character no face here has, and none can draw in parts,
/// stayed whole (2026-10-18): each Hangul syllable and each kana with a
/// voiced mark is one `.notdef` in its own cells where its parts' boxes had
/// spilled into the cells after it, 340 cells.
const PINNED: (u64, u64, u64) = (0x04a5a3bd3567bfeb, 0xf937979684a91195, 0xff8c8a54df759923);

#[test]
fn the_macos_stack_paints_cat_cjk_to_its_pinned_frame() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the system stack is macOS's");
        return;
    }
    let menlo = std::fs::read(MENLO).unwrap();
    let emoji = std::fs::read(EMOJI).unwrap();
    let faces = [
        Face::parse_index(&menlo, FaceIndex::of(0)).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
    ];
    let mut r = renderer(&faces);
    let mut canvas = Canvas::for_renderer(&r);
    let bytes = recording("utf8/cat-cjk.raw");
    let mut frame = None;
    replay_final(&bytes, |t| {
        frame = Some(paint(&mut r, t, &mut canvas, Damage::Full))
    });
    let frame = frame.unwrap();
    // Positive controls: emoji reached the colour texture, and Menlo drew
    // the rest.
    assert!(frame.colored.value() >= 10, "{:?} coloured", frame.colored);
    assert!(frame.instances.value() > 500, "{:?}", frame.instances);
    if let Ok(dir) = std::env::var("GOLDEN_DUMP") {
        let mut out = format!("P6\n{} {}\n255\n", canvas.width, canvas.height).into_bytes();
        for &p in &canvas.pixels {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(format!("{dir}/system-cat-cjk.ppm"), out).unwrap();
    }
    let got = (fnv64(&menlo), fnv64(&emoji), canvas.digest());
    if std::env::var("GOLDEN_PRINT").is_ok() {
        println!(
            "    const PINNED: (u64, u64, u64) = (0x{:016x}, 0x{:016x}, 0x{:016x});",
            got.0, got.1, got.2
        );
        return;
    }
    assert_eq!(
        (got.0, got.1),
        (PINNED.0, PINNED.1),
        "the system faces are not the files the frame was pinned with; rerun with GOLDEN_PRINT=1"
    );
    assert_eq!(got.2, PINNED.2, "frame digest 0x{:016x}", got.2);
}
