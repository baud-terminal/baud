//! `CBLC` sizes at the atlas, where a strike is chosen: a refused size is
//! never drawn from, a glyph past the face draws no bitmap, an overlapping
//! size draws from the first record that holds the glyph, and format 4's
//! extra pair names no glyph (`baud-font/tests/cblc.rs` holds the rules and
//! their source).
//!
//! **The fixture says which size drew.** `cblc-cases.ttf`
//! (written by `baud-font/tests/cblc.rs`) paints each size's images left half in
//! the size's colour and right half white, as `sbix-strikes.ttf` does:
//! 20 ppem green, 30 red (refused: its firsts decrease), 40 blue (refused:
//! its records run past the table), 50 cyan and purple (two overlapping
//! records), 60 magenta, 80 yellow (format 4). The fixture has 16 glyphs and
//! no outlines, so a glyph no size draws comes out as nothing.

use baud_domain::GlyphId;
use baud_font::{Face, Phase, Scale};
use baud_render::{Atlas, FaceIx, Kind, Slot};

fn cases() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/bitmap/cblc-cases.ttf"
    ))
    .unwrap()
}

const COLOURS: [(&str, [u8; 3]); 7] = [
    ("green", [40, 200, 40]),
    ("red", [220, 40, 40]),
    ("blue", [40, 80, 230]),
    ("cyan", [40, 200, 200]),
    ("purple", [150, 60, 200]),
    ("magenta", [200, 60, 200]),
    ("yellow", [230, 210, 30]),
];
const WHITE: [u8; 3] = [240, 240, 240];

#[derive(Debug, PartialEq, Eq)]
enum Drawn {
    Colour(&'static str),
    Nothing,
}

fn rgb_at(atlas: &Atlas, s: Slot, x: u32, y: u32) -> [u8; 3] {
    let side = atlas.color_side().value();
    let at = usize::try_from((u32::from(s.y.0) + y) * side + u32::from(s.x.0) + x).unwrap();
    let [_, r, g, b] = atlas.argb()[at].to_be_bytes();
    [r, g, b]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 2)
}

/// What glyph `glyph` draws as at `px`, in an atlas of its own: an atlas
/// serves one cell size and keys its slots by glyph alone, so a second size
/// asked of the same atlas would be handed the first size's slot.
fn drawn(face: &Face<'_>, glyph: u16, px: f32) -> Drawn {
    let mut atlas = Atlas::new();
    let atlas = &mut atlas;
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
    assert!(
        near(right, WHITE),
        "glyph {glyph} at {px} px: the right half is {right:?}, not white"
    );
    let (name, _) = COLOURS
        .iter()
        .find(|(_, c)| near(*c, left))
        .unwrap_or_else(|| panic!("glyph {glyph} at {px} px: {left:?} is no size's colour"));
    Drawn::Colour(name)
}

#[test]
fn a_refused_size_is_never_drawn_from() {
    let data = cases();
    let face = Face::parse(&data).unwrap();
    assert_eq!(drawn(&face, 1, 20.0), Drawn::Colour("green"), "the control");
    // 30 and 40 px are the refused sizes' own: the next size that holds the
    // glyph, at or above, is 50 ppem's first record.
    assert_eq!(drawn(&face, 1, 30.0), Drawn::Colour("cyan"), "30 px");
    assert_eq!(drawn(&face, 1, 40.0), Drawn::Colour("cyan"), "40 px");
    // Glyph 8 is in the refused 30 ppem size's first record, so a reader that
    // searched that size anyway would draw it red.
    assert_eq!(
        drawn(&face, 8, 30.0),
        Drawn::Colour("cyan"),
        "glyph 8 at 30 px"
    );
}

#[test]
fn a_glyph_past_the_face_draws_no_bitmap() {
    let data = cases();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        drawn(&face, 15, 60.0),
        Drawn::Colour("magenta"),
        "the control"
    );
    assert_eq!(
        drawn(&face, 17, 60.0),
        Drawn::Nothing,
        "glyph 17 is past the face's 16"
    );
}

#[test]
fn an_overlapping_size_draws_from_the_first_record_holding_the_glyph() {
    let data = cases();
    let face = Face::parse(&data).unwrap();
    // Record 0 holds 1..8 in cyan, record 1 holds 5..12 in purple.
    assert_eq!(drawn(&face, 6, 50.0), Drawn::Colour("cyan"), "glyph 6");
    assert_eq!(drawn(&face, 8, 50.0), Drawn::Colour("cyan"), "glyph 8");
    assert_eq!(drawn(&face, 11, 50.0), Drawn::Colour("purple"), "glyph 11");
}

#[test]
fn the_sentinel_names_no_glyph() {
    let data = cases();
    let face = Face::parse(&data).unwrap();
    assert_eq!(
        drawn(&face, 13, 60.0),
        Drawn::Colour("magenta"),
        "the control"
    );
    // The 80 ppem size's twelve real pairs stop at glyph 12; its extra pair
    // names glyph 13 only to say where glyph 12's image ends.
    assert_eq!(drawn(&face, 13, 80.0), Drawn::Colour("magenta"), "80 px");
}

/// The Noto subset as two sizes: 109 ppem with records 0-3, and 54 ppem with
/// all five records and the first two swapped, so its firsts decrease.
fn two_sizes() -> Vec<u8> {
    let data = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/fonts/NotoColorEmoji-subset.ttf"
    ))
    .unwrap();
    let n = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let r = (0..n)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == b"CBLC")
        .unwrap();
    let word = |at: usize| {
        usize::try_from(u32::from_be_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
    };
    let c = &data[word(r + 8)..word(r + 8) + word(r + 12)];
    let (size, block) = (&c[8..56], &c[56..288]);
    let mut cblc = Vec::new();
    cblc.extend_from_slice(&0x0003_0000u32.to_be_bytes());
    cblc.extend_from_slice(&2u32.to_be_bytes());
    let mut s0 = size.to_vec();
    s0[0..4].copy_from_slice(&104u32.to_be_bytes());
    s0[8..12].copy_from_slice(&4u32.to_be_bytes());
    let mut s1 = size.to_vec();
    s1[0..4].copy_from_slice(&u32::try_from(104 + block.len()).unwrap().to_be_bytes());
    s1[44] = 54;
    s1[45] = 54;
    cblc.extend_from_slice(&s0);
    cblc.extend_from_slice(&s1);
    cblc.extend_from_slice(block);
    let mut b1 = block.to_vec();
    b1[0..8].copy_from_slice(&block[8..16]);
    b1[8..16].copy_from_slice(&block[0..8]);
    cblc.extend_from_slice(&b1);
    let mut out = data.clone();
    out.resize(out.len().next_multiple_of(4), 0);
    let at = u32::try_from(out.len()).unwrap();
    out.extend_from_slice(&cblc);
    out[r + 8..r + 12].copy_from_slice(&at.to_be_bytes());
    out[r + 12..r + 16].copy_from_slice(&u32::try_from(cblc.len()).unwrap().to_be_bytes());
    out
}

#[test]
fn a_refused_noto_size_is_never_drawn_from() {
    let data = two_sizes();
    let face = Face::parse(&data).unwrap();
    let mut atlas = Atlas::new();
    let scale = Scale::of(54.0, face.units_per_em());
    // Drawn from 109 ppem and scaled to 54 px: 136 x 128 becomes 67 x 63. The
    // refused 54 ppem size would have drawn it at its own 136 x 128.
    let s = atlas
        .slot(
            &face,
            FaceIx::PRIMARY,
            GlyphId::from_shaper(4),
            scale,
            Phase::ZERO,
        )
        .expect("glyph 4 draws from 109 ppem");
    assert_eq!((s.w.0, s.h.0), (67, 63), "glyph 4's slot");
    assert!(
        atlas
            .slot(
                &face,
                FaceIx::PRIMARY,
                GlyphId::from_shaper(40),
                scale,
                Phase::ZERO
            )
            .is_none(),
        "glyph 40 is only in the refused size"
    );
}
