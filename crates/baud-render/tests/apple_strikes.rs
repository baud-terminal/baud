//! Apple Color Emoji, the face the per-glyph strike rule was found on: every
//! glyph with an image in any of its nine strikes draws at every size, a
//! `'flip'` draws its target mirrored, and colour is asked of every strike.
//! Pinned to the file's digest, as `system.rs` is, so a font the OS replaces
//! fails loudly rather than being compared. Runs on macOS and says so
//! elsewhere.
//!
//! **What the face holds, measured.** 3,844 glyphs in strikes at 20, 26,
//! 32, 40, 48, 52, 64, 96 and 160 ppem. 177 are empty in some strikes and
//! present in others -- the melting face U+1FAE0 is empty at 40, 48 and 52,
//! the pink heart U+1FA77 at 20, 26, 40, 48 and 52, and the family
//! `cat-cjk.raw` carries 1,041 times at 40, 48 and 52 -- and 108 are `'flip'` in
//! every strike, the people facing right. With one strike chosen per size,
//! 99 glyphs drew nothing at the app's default 26 px, and every flip drew
//! nothing at every size.

mod support;

use baud_domain::{Codepoint, GlyphId};
use baud_domain::{Cols, Rows};
use baud_font::bytes::Index;
use baud_font::{Face, FaceIndex, Mirror, Phase, Scale};
use baud_render::{Atlas, Blend, Damage, FaceIx, Kind, PxSize, Renderer, Slot, Stack, Theme};
use support::*;

const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

/// FNV-1a over a file, the digest `system.rs` pins this file with.
fn fnv64(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

const PINNED: u64 = 0xf937979684a91195;

fn emoji() -> Option<Vec<u8>> {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: Apple Color Emoji is macOS's");
        return None;
    }
    let data = std::fs::read(EMOJI).unwrap();
    assert_eq!(
        fnv64(&data),
        PINNED,
        "Apple Color Emoji is not the file these counts were taken on"
    );
    Some(data)
}

/// Every pixel of a colour slot, as `0xAARRGGBB`.
fn pixels(atlas: &Atlas, s: Slot) -> Vec<u32> {
    let side = usize::try_from(atlas.color_side().value()).unwrap();
    let (x, y, w, h) = (
        usize::from(s.x.0),
        usize::from(s.y.0),
        usize::from(s.w.0),
        usize::from(s.h.0),
    );
    (0..h)
        .flat_map(|r| atlas.argb()[(y + r) * side + x..(y + r) * side + x + w].to_vec())
        .collect()
}

fn slot(atlas: &mut Atlas, face: &Face<'_>, g: u16, px: f32) -> Option<Slot> {
    atlas
        .slot(
            face,
            FaceIx::PRIMARY,
            GlyphId::from_shaper(g),
            Scale::of(px, face.units_per_em()),
            Phase::ZERO,
        )
        .filter(|s| s.kind == Kind::Color)
}

/// Each glyph's strikes: which hold an image for it, and whether a flip.
fn census(face: &Face<'_>) -> (Vec<u16>, Vec<u16>, Vec<u16>) {
    let strikes = face.strikes().value();
    let (mut partial, mut flips, mut whole) = (Vec::new(), Vec::new(), Vec::new());
    for g in 0..face.glyph_count().value() {
        let id = GlyphId::from_shaper(g);
        let held: Vec<_> = (0..strikes)
            .filter_map(|s| face.bitmap(Index::of(s), id))
            .collect();
        if held.is_empty() {
            continue;
        }
        if held.iter().any(|b| b.mirror == Mirror::LeftRight) {
            flips.push(g);
        } else if held.len() < usize::try_from(strikes).unwrap() {
            partial.push(g);
        } else {
            whole.push(g);
        }
    }
    (partial, flips, whole)
}

/// One size either side of every strike, 20 to 160: where the choice
/// changes. Above 160 every glyph draws from its largest strike.
const SIZES: [f32; 27] = [
    10.0, 14.0, 16.0, 20.0, 21.0, 24.0, 26.0, 27.0, 28.0, 32.0, 33.0, 36.0, 40.0, 41.0, 44.0, 48.0,
    49.0, 52.0, 53.0, 56.0, 64.0, 65.0, 80.0, 96.0, 97.0, 160.0, 161.0,
];

#[test]
fn every_glyph_with_an_image_in_any_strike_draws_at_every_size() {
    let Some(data) = emoji() else { return };
    let face = Face::parse_index(&data, FaceIndex::of(0)).unwrap();
    let (partial, flips, whole) = census(&face);
    // The counts this file was measured with; the digest above holds them.
    assert_eq!((partial.len(), flips.len()), (177, 108));
    // Every partial glyph and every flip, and one whole glyph in twenty as
    // the control that the sweep can draw anything at all.
    let glyphs: Vec<u16> = partial
        .iter()
        .chain(&flips)
        .chain(whole.iter().step_by(20))
        .copied()
        .collect();
    for px in SIZES {
        let mut atlas = Atlas::new();
        for &g in &glyphs {
            let s = slot(&mut atlas, &face, g, px)
                .unwrap_or_else(|| panic!("glyph {g} draws nothing at {px} px"));
            assert!(
                pixels(&atlas, s).iter().any(|&p| p >> 24 != 0),
                "glyph {g} at {px} px is a transparent slot"
            );
        }
    }
}

#[test]
fn a_flip_draws_its_target_mirrored_at_a_native_size() {
    let Some(data) = emoji() else { return };
    let face = Face::parse_index(&data, FaceIndex::of(0)).unwrap();
    let (_, flips, _) = census(&face);
    assert_eq!(flips.len(), 108);
    // At 64 px the 64 strike is drawn as stored, so the flip's image must be
    // its target's mirrored, texel for texel. The target is the glyph whose
    // own record is the PNG the flip resolved to.
    let strike = (0..face.strikes().value())
        .map(Index::of)
        .find(|&i| face.strike_ppem(i).unwrap().value() == 64)
        .unwrap();
    let mut atlas = Atlas::new();
    for &f in &flips {
        let png = face
            .bitmap(strike, GlyphId::from_shaper(f))
            .unwrap()
            .png
            .as_slice()
            .as_ptr();
        let target = (0..face.glyph_count().value())
            .find(|&g| {
                face.bitmap(strike, GlyphId::from_shaper(g))
                    .is_some_and(|b| {
                        b.mirror == Mirror::AsStored && b.png.as_slice().as_ptr() == png
                    })
            })
            .unwrap_or_else(|| panic!("flip {f} has no target"));
        let (a, b) = (
            slot(&mut atlas, &face, f, 64.0).unwrap(),
            slot(&mut atlas, &face, target, 64.0).unwrap(),
        );
        assert_eq!(
            (a.w, a.h, a.left, a.top),
            (b.w, b.h, b.left, b.top),
            "flip {f}"
        );
        let w = usize::from(a.w.0);
        let (mine, theirs) = (pixels(&atlas, a), pixels(&atlas, b));
        for (row_a, row_b) in mine.chunks_exact(w).zip(theirs.chunks_exact(w)) {
            assert!(
                row_a.iter().eq(row_b.iter().rev()),
                "flip {f} is not glyph {target} mirrored"
            );
        }
        // And not merely a symmetric image: the flip differs from its target.
        assert!(mine != theirs, "flip {f} equals its target unmirrored");
    }
}

#[test]
fn colour_is_asked_of_every_strike() {
    let Some(data) = emoji() else { return };
    let face = Face::parse_index(&data, FaceIndex::of(0)).unwrap();
    // The pink heart is empty in the first strike, 20 ppem.
    let heart = face.glyph(Codepoint::new(0x1FA77).unwrap()).unwrap();
    assert!(face.bitmap(Index::ZERO, heart).is_none());
    assert!(face.is_colour(heart));
    // Every glyph that draws from some strike is colour.
    let (partial, flips, whole) = census(&face);
    assert_eq!((partial.len(), flips.len()), (177, 108));
    for g in partial.iter().chain(&flips).chain(&whole) {
        assert!(face.is_colour(GlyphId::from_shaper(*g)), "glyph {g}");
    }
    // And a glyph empty in every strike is not colour: there are 136, and
    // the face has no `COLR` to answer for them instead.
    let empty: Vec<u16> = (0..face.glyph_count().value())
        .filter(|&g| !partial.contains(&g) && !flips.contains(&g) && !whole.contains(&g))
        .collect();
    assert_eq!(empty.len(), 136);
    for g in empty {
        assert!(!face.is_colour(GlyphId::from_shaper(g)), "glyph {g}");
    }
}

/// The recorded level: `cat-cjk`'s last screen, with Menlo in front, at
/// sizes either side of every strike. The screen is the same at every
/// size, so the colour glyphs it draws must be the same in number; at 40,
/// 48 and 52 px the family, 1,041 times in the recording, drew nothing.
#[test]
fn cat_cjk_draws_the_same_colour_glyphs_at_every_size() {
    let Some(emoji) = emoji() else { return };
    let menlo = std::fs::read("/System/Library/Fonts/Menlo.ttc").unwrap();
    let faces = [
        Face::parse_index(&menlo, FaceIndex::of(0)).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
    ];
    let bytes = recording("utf8/cat-cjk.raw");
    let mut counts = Vec::new();
    replay_final(&bytes, |t| {
        for px in [14.0, 20.0, 26.0, 28.0, 32.0, 40.0, 48.0, 52.0, 64.0] {
            let mut r = Renderer::new(
                Stack::new(&faces).unwrap(),
                PxSize::of(px).unwrap(),
                Theme::dark(),
                Blend::Native,
                Cols::of_index(usize::from(WS.ws_col)),
                Rows::of_index(usize::from(WS.ws_row)),
            )
            .unwrap();
            let mut canvas = Canvas::for_renderer(&r);
            let frame = paint(&mut r, t, &mut canvas, Damage::Full);
            counts.push((px, frame.colored.value()));
        }
    });
    let first = counts[0].1;
    // The positive control: the screen has colour glyphs to draw.
    assert!(first >= 10, "{counts:?}");
    for &(px, n) in &counts {
        assert_eq!(n, first, "at {px} px: {counts:?}");
    }
}
