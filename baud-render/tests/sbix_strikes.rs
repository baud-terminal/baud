//! A colour bitmap is drawn from a strike that holds it, and `'flip'` is
//! drawn mirrored -- at the atlas, where the strike is chosen.
//!
//! **The fixture says which strike drew.** `sbix-strikes.ttf`
//! (`scripts/gen-sbix-fixture.py`) has strikes at 10, 20, 26, 40, 52 and 80 ppem
//! whose images -- all but the two ramps and the unreadable PNGs -- paint
//! the left half in that strike's colour and the right half white. So the
//! colour a quarter of the way in names the strike, and white there with the
//! colour three quarters in means a mirror.
//!
//! Every case the generator lists is drawn here -- of the hop chain, R1, R2,
//! R9 and R10: the strike rule over
//! glyphs held in every strike, one, two non-adjacent ones and every strike
//! but where the PNG is unreadable; the mirror over flips, chains of either
//! kind up to the shared bound of ten records, offsets and a ramp with no
//! symmetry; and the one gap, a glyph whose every image is unreadable.
//!
//! **The rule, and where it came from.** Among the strikes that hold the
//! glyph and whose PNG decodes, the smallest at or above the size in whole
//! pixels (rounded half up), else the largest. The specifications require
//! the per-glyph part; CoreText was measured taking this order on Apple
//! Color Emoji for the square it draws, which below 24 px on the system font
//! is larger than the size and is not followed. Jev chose it at 1.00 against
//! keeping the old nearest-with-a-penalty score, and at 0.98 asked again
//! with that enlargement in its context. Before it, the atlas chose one strike for
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
const STRIKES: [(u16, [u8; 3]); 6] = [
    (10, [220, 40, 40]),
    (20, [40, 200, 40]),
    (26, [200, 60, 200]),
    (40, [40, 80, 230]),
    (52, [40, 200, 200]),
    (80, [230, 210, 30]),
];
const HELD: [u16; 6] = [10, 20, 26, 40, 52, 80];
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

/// Either side of the strikes, and the half pixels: 26.5 and 52.5 come back
/// through an 800-unit em's f32 scale a hair under the half, so they are
/// where a careless round-half-up picks the strike below.
const SIZES: [f32; 16] = [
    6.0, 10.0, 10.4, 10.5, 15.0, 20.0, 26.4, 26.5, 30.0, 40.0, 40.4, 40.5, 41.0, 52.5, 80.0, 100.0,
];

#[test]
fn a_glyph_is_drawn_from_the_smallest_strike_at_or_above_its_size_that_holds_it() {
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    for px in SIZES {
        let mut atlas = Atlas::new();
        let want = smallest_at_or_above(px, &HELD);
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
        let e = if want == 40 { 52 } else { want };
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
        // M: in the 20 and 52 strikes only. From 26 px to under 36 px the
        // nearest holder is 20 and the rule takes 52.
        assert_eq!(
            drawn(&mut atlas, &face, 13, px),
            Drawn::Strike(smallest_at_or_above(px, &[20, 52]), Mirrored::No),
            "M at {px}"
        );
        // R10: a dupe of A.
        assert_eq!(
            drawn(&mut atlas, &face, 26, px),
            Drawn::Strike(want, Mirrored::No),
            "R10 at {px}"
        );
        // F holds nothing; I is a dupe of itself and L a flip of itself; R1
        // needs eleven records where the bound is ten.
        for (glyph, name) in [(6, "F"), (9, "I"), (12, "L"), (17, "R1")] {
            assert_eq!(
                drawn(&mut atlas, &face, glyph, px),
                Drawn::Nothing,
                "{name} at {px}"
            );
        }
    }
}

/// Every texel of a colour slot, as `0xAARRGGBB`.
fn texels(atlas: &Atlas, s: Slot) -> Vec<u32> {
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

#[test]
fn a_flip_mirrors_the_image_before_it_is_scaled() {
    // U flips T's ramp and S stores the same ramp mirrored, so U must draw
    // as S does, texel for texel, at every size. At a strike's own size the
    // image is drawn as stored, so that is the mirror itself; elsewhere it
    // is the mirror taken before the box filter, whose boxes are not
    // symmetric about the image's middle.
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    for px in SIZES.iter().chain(&[13.0, 33.0, 57.0, 71.0]) {
        let mut atlas = Atlas::new();
        let scale = Scale::of(*px, face.units_per_em());
        let mut at = |g: u16| {
            atlas
                .slot(
                    &face,
                    FaceIx::PRIMARY,
                    GlyphId::from_shaper(g),
                    scale,
                    Phase::ZERO,
                )
                .unwrap_or_else(|| panic!("glyph {g} at {px} px"))
        };
        let (t, u, s) = (at(27), at(28), at(29));
        assert_eq!(
            (u.kind, u.w, u.h, u.left, u.top),
            (s.kind, s.w, s.h, s.left, s.top),
            "U and S at {px}"
        );
        assert!(texels(&atlas, u) == texels(&atlas, s), "U is not S at {px}");
        // The control: the ramp is not its own mirror at this size.
        assert!(texels(&atlas, t) != texels(&atlas, s), "T is S at {px}");
    }
}

#[test]
fn a_glyph_whose_every_image_is_unreadable_is_colour_and_draws_nothing() {
    // Q has a PNG in every strike and none decodes. The face calls it colour,
    // because colour is asked of the records and not of the pixels, and the
    // atlas draws nothing for it: a gap, pinned rather than built.
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    assert!(face.is_colour(GlyphId::from_shaper(16)));
    for px in SIZES {
        let mut atlas = Atlas::new();
        assert_eq!(drawn(&mut atlas, &face, 16, px), Drawn::Nothing, "{px}");
    }
}

#[test]
fn a_flip_is_its_target_mirrored_within_the_image() {
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    for px in SIZES {
        let mut atlas = Atlas::new();
        let want = smallest_at_or_above(px, &HELD);
        // C flips A; D flips J, a dupe of A; G flips C, two flips on the way,
        // which CoreText draws mirrored once; K dupes C, a flip second; R2
        // and R9 are the hop chain at its longest and shortest flips.
        for (glyph, name) in [
            (3, "C"),
            (4, "D"),
            (7, "G"),
            (11, "K"),
            (18, "R2"),
            (25, "R9"),
        ] {
            assert_eq!(
                drawn(&mut atlas, &face, glyph, px),
                Drawn::Strike(want, Mirrored::Yes),
                "{name} at {px}"
            );
        }
    }
}

#[test]
fn a_bitmap_sits_at_its_own_offsets_and_a_flip_at_its_targets() {
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
    // N's own offsets are (5, -4): its image's bottom-left corner 5 px right
    // of the origin and 4 below it, so 5 px right of A's and 4 px lower. P
    // flips N from (1, 1) and sits at N's.
    let (a, n) = (at(&mut atlas, 1), at(&mut atlas, 14));
    assert_eq!((n.0 - a.0, n.1 - a.1), (5, 4));
    assert_eq!(at(&mut atlas, 15), n);
    // Drawn from a strike at another size, the offsets scale with the image:
    // 30 px takes the 40 strike, a factor of 0.75, so (5, -4) becomes 3.75
    // and -3, which round to 4 and -3. Neither lands on a half, where the
    // direction of rounding is unmeasured. An atlas holds one size, so this
    // one is new.
    let mut atlas = Atlas::new();
    let scale = Scale::of(30.0, face.units_per_em());
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
        (s.left, s.top)
    };
    let (a, n) = (at(&mut atlas, 1), at(&mut atlas, 14));
    assert_eq!((n.0 - a.0, n.1 - a.1), (4, 3));
    assert_eq!(at(&mut atlas, 15), n);
}
