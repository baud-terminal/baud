//! A colour bitmap is drawn from a strike that holds it, and `'flip'` is
//! drawn mirrored -- at the atlas, where the strike is chosen.
//!
//! **The fixture says which strike drew.** `sbix-strikes.ttf`
//! (`scripts/gen-sbix-fixture.py`) has strikes at 10, 20, 40 and 80 ppem
//! whose images paint the left half in that strike's colour and the right
//! half white. So the colour a quarter of the way in names the strike, and
//! white there with the colour three quarters in means a mirror.
//!
//! **The rule, and where it came from.** Among the strikes that hold the
//! glyph and whose PNG decodes, the smallest at or above the size in whole
//! pixels (rounded half up), else the largest. The specifications require
//! the per-glyph part; CoreText was measured taking exactly this order on
//! Apple Color Emoji, and Jev settled it at 1.00 against keeping the old
//! nearest-with-a-penalty score. Before it, the atlas chose one strike for
//! the size and drew nothing where that strike was empty.

use baud_domain::GlyphId;
use baud_font::{Face, Phase, Scale};
use baud_render::{Atlas, FaceIx, Kind, Slot};

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/fonts/sbix-strikes.ttf"
    ))
    .unwrap()
}

/// The fixture's strike colours, by ppem, and the white right half.
const STRIKES: [(u16, [u8; 3]); 4] = [
    (10, [220, 40, 40]),
    (20, [40, 200, 40]),
    (40, [40, 80, 230]),
    (80, [230, 210, 30]),
];
const WHITE: [u8; 3] = [240, 240, 240];

/// What a slot shows: which strike's colour, and whether mirrored.
#[derive(Debug, PartialEq, Eq)]
enum Drawn {
    Strike(u16, Mirrored),
    Nothing,
}

#[derive(Debug, PartialEq, Eq)]
enum Mirrored {
    No,
    Yes,
}

fn rgb_at(atlas: &Atlas, s: Slot, x: u32, y: u32) -> [u8; 3] {
    let side = atlas.color_side().value();
    let at = usize::try_from((u32::from(s.y.0) + y) * side + u32::from(s.x.0) + x).unwrap();
    let p = atlas.argb()[at];
    let [_, r, g, b] = p.to_be_bytes();
    [r, g, b]
}

fn strike_of(rgb: [u8; 3]) -> u16 {
    STRIKES
        .iter()
        .find(|(_, c)| c.iter().zip(rgb).all(|(&a, b)| a.abs_diff(b) <= 2))
        .unwrap_or_else(|| panic!("{rgb:?} is no strike's colour"))
        .0
}

fn drawn(atlas: &mut Atlas, face: &Face<'_>, glyph: u16, px: f32) -> Drawn {
    let scale = Scale::of(px, face.units_per_em());
    let Some(s) = atlas.slot(
        face,
        FaceIx::PRIMARY,
        GlyphId::from_shaper(glyph),
        scale,
        Phase::ZERO,
    ) else {
        return Drawn::Nothing;
    };
    if s.kind != Kind::Color {
        return Drawn::Nothing;
    }
    let (w, h) = (u32::from(s.w.0), u32::from(s.h.0));
    let (left, right) = (
        rgb_at(atlas, s, w / 4, h / 2),
        rgb_at(atlas, s, w * 3 / 4, h / 2),
    );
    let near = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 2);
    match (near(left, WHITE), near(right, WHITE)) {
        (false, true) => Drawn::Strike(strike_of(left), Mirrored::No),
        (true, false) => Drawn::Strike(strike_of(right), Mirrored::Yes),
        _ => panic!("glyph {glyph} at {px} px: left {left:?}, right {right:?}"),
    }
}

/// The smallest strike at or above `px` rounded half up, else the largest.
fn smallest_at_or_above(px: f32, held: &[u16]) -> u16 {
    let whole = (px + 0.5).floor();
    held.iter()
        .copied()
        .find(|&p| f32::from(p) >= whole)
        .unwrap_or(*held.last().unwrap())
}

const SIZES: [f32; 13] = [
    6.0, 10.0, 10.4, 10.5, 15.0, 20.0, 30.0, 40.0, 40.4, 40.5, 41.0, 80.0, 100.0,
];

#[test]
fn a_glyph_is_drawn_from_the_smallest_strike_at_or_above_its_size_that_holds_it() {
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    let all = [10, 20, 40, 80];
    for px in SIZES {
        let mut atlas = Atlas::new();
        let want = smallest_at_or_above(px, &all);
        // A: in every strike, drawn as stored.
        assert_eq!(
            drawn(&mut atlas, &face, 1, px),
            Drawn::Strike(want, Mirrored::No),
            "A at {px}"
        );
        // B: in the 80 strike only, so drawn from it at every size --
        // above the size, never the nearest below it.
        assert_eq!(
            drawn(&mut atlas, &face, 2, px),
            Drawn::Strike(80, Mirrored::No),
            "B at {px}"
        );
        // H: in the 10 strike only, so drawn from it, below the size.
        assert_eq!(
            drawn(&mut atlas, &face, 8, px),
            Drawn::Strike(10, Mirrored::No),
            "H at {px}"
        );
        // E: its 40 PNG does not decode, so where 40 would be chosen the
        // next strike that decodes is.
        let e = if want == 40 { 80 } else { want };
        assert_eq!(
            drawn(&mut atlas, &face, 5, px),
            Drawn::Strike(e, Mirrored::No),
            "E at {px}"
        );
        // J: a dupe of A.
        assert_eq!(
            drawn(&mut atlas, &face, 10, px),
            Drawn::Strike(want, Mirrored::No),
            "J at {px}"
        );
        // F holds nothing; I is a dupe of itself, refused after ten hops.
        assert_eq!(drawn(&mut atlas, &face, 6, px), Drawn::Nothing, "F at {px}");
        assert_eq!(drawn(&mut atlas, &face, 9, px), Drawn::Nothing, "I at {px}");
    }
}

#[test]
fn a_flip_is_its_target_mirrored_within_the_image() {
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    for px in SIZES {
        let mut atlas = Atlas::new();
        let want = smallest_at_or_above(px, &[10, 20, 40, 80]);
        // C flips A; D flips J, a dupe of A; G flips C, two flips on the way,
        // which CoreText draws mirrored once.
        for (glyph, name) in [(3, "C"), (4, "D"), (7, "G")] {
            assert_eq!(
                drawn(&mut atlas, &face, glyph, px),
                Drawn::Strike(want, Mirrored::Yes),
                "{name} at {px}"
            );
        }
    }
}

#[test]
fn a_flip_sits_at_its_targets_offsets() {
    // C's own record carries (7, 3); A's (0, 0). CoreText and FreeType both
    // place the image at the target's.
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    let mut atlas = Atlas::new();
    let scale = Scale::of(80.0, face.units_per_em());
    let at = |atlas: &mut Atlas, g: u16| {
        let s = atlas
            .slot(
                &face,
                FaceIx::PRIMARY,
                GlyphId::from_shaper(g),
                scale,
                Phase::ZERO,
            )
            .unwrap();
        (s.left, s.top, s.w, s.h)
    };
    assert_eq!(at(&mut atlas, 3), at(&mut atlas, 1));
}
