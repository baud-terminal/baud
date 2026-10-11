//! A face read at a variation, through the renderer.
//!
//! `baud_font::Face::outline` refuses a glyph whose outline varies at the
//! face's coordinates -- `Drawn::NeedsScratch`, nothing drawn, never the
//! default instance in its place -- and `Face::outline_at` draws it with
//! caller scratch. The atlas and the shaper's glyph boxes took anything but
//! `Drawn::Outline` for a glyph with no ink, so a varied face drew blanks
//! and placed no fallback mark. These hold the atlas drawing the varied
//! outline, the renderer placing a fallback mark against varied boxes, and
//! a glyph past the scratch budget refused and counted rather than taken
//! for blank.
//!
//! The fonts are baud's own: `var/ink-cases.ttf` (`scripts/ink-cases-font.py`)
//! has no `GPOS`, so its marks take the fallback, and carries `heavy`, whose
//! 240,000 flattened points are past `VAR_POINTS_MAX`.

mod support;

use baud_domain::{Codepoint, GlyphId};
use baud_font::var::{Fixed, Variation};
use baud_font::{Bounds, Canvas, Drawn, Face, Fit, Phase, Rasterizer, Scale, Tag, VarPoint};
use baud_render::{Atlas, CellSpan, FaceIx, FaceStyle, Kind, Slot, VAR_POINTS_MAX};
use support::renderer;

fn ink_cases() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/var/ink-cases.ttf"
    ))
    .unwrap()
}

fn at_weight(face: &mut Face<'_>, wght: i32) {
    face.set_variations(&[Variation {
        tag: Tag::from_bytes(*b"wght"),
        value: Fixed(wght << 16),
    }]);
}

fn glyph(face: &Face<'_>, c: u32) -> GlyphId {
    face.glyph(Codepoint::new(c).unwrap()).unwrap()
}

/// `g`'s coverage mask drawn straight from `Face::outline_at` with all the
/// scratch it asks for: what the atlas must hold for it.
fn mask_of(face: &Face<'_>, g: GlyphId, scale: Scale) -> (Vec<u8>, u32, u32) {
    let need = usize::try_from(face.outline_need(g).value()).unwrap();
    let mut var = vec![VarPoint::EMPTY; need];
    let mut b = Bounds::new();
    assert_eq!(face.outline_at(g, &mut var, &mut b), Drawn::Outline);
    let fit = Fit::at(&b, scale, Phase::ZERO).unwrap();
    let mut scratch = vec![0f32; fit.extent.scratch().unwrap()];
    let canvas = Canvas::new(fit.extent, &mut scratch).unwrap();
    let mut r = Rasterizer::new(canvas, fit.transform);
    assert_eq!(face.outline_at(g, &mut var, &mut r), Drawn::Outline);
    let mut cov = vec![0u8; fit.extent.cells().unwrap()];
    r.finish().coverage(&mut cov).unwrap();
    (cov, fit.extent.w.value(), fit.extent.h.value())
}

/// The texels of `s` in the atlas's mask sheet, row by row.
fn texels(atlas: &Atlas, s: Slot) -> Vec<u8> {
    let side = usize::try_from(atlas.side().value()).unwrap();
    let (x, y) = (usize::from(s.x.0), usize::from(s.y.0));
    let (w, h) = (usize::from(s.w.0), usize::from(s.h.0));
    (0..h)
        .flat_map(|row| atlas.pixels()[(y + row) * side + x..][..w].iter().copied())
        .collect()
}

/// **The atlas draws a varied glyph's varied outline**, exactly what
/// `outline_at` gives the rasteriser, and not the default instance: `a` at
/// wght 900 is taller and wider than at the default, and the two masks
/// differ. Before, `Face::outline` refused it, the atlas read an empty box
/// and memoised the glyph as blank.
#[test]
fn the_atlas_draws_a_varied_glyph_at_its_coordinates() {
    let data = ink_cases();
    let plain = Face::parse(&data).unwrap();
    let mut face = Face::parse(&data).unwrap();
    at_weight(&mut face, 900);
    let a = glyph(&face, 0x61);
    let scale = Scale::of(40.0, face.units_per_em());
    assert!(
        face.outline_need(a).value() > 0,
        "the premise: `a` varies at wght 900"
    );
    let (default, _, _) = mask_of(&plain, a, scale);
    let (want, w, h) = mask_of(&face, a, scale);
    assert_ne!(default, want, "the control: the variation moves `a`");

    let mut atlas = Atlas::new();
    let s = atlas
        .slot(&face, FaceIx::PRIMARY, a, scale, Phase::ZERO)
        .expect("a varied glyph draws");
    assert_eq!(s.kind, Kind::Mask);
    assert_eq!((u32::from(s.w.0), u32::from(s.h.0)), (w, h));
    assert_eq!(
        texels(&atlas, s),
        want,
        "the varied outline, texel for texel"
    );
    assert_eq!(atlas.stats().refused.value(), 0);

    // The default face through the same atlas is the default outline.
    let mut atlas = Atlas::new();
    let s = atlas
        .slot(&plain, FaceIx::PRIMARY, a, scale, Phase::ZERO)
        .unwrap();
    assert_eq!(texels(&atlas, s), default);
}

/// **A glyph past the scratch budget is refused, counted once, and never
/// taken for blank.** `heavy` flattens to 240,000 points, past
/// `VAR_POINTS_MAX` (and past HarfBuzz's own 200,000-point bound): at wght
/// 900 the atlas draws nothing for it and counts it, and asking again is
/// answered from the memo without counting it twice. The control: at the
/// default instance it draws without scratch, and nothing is refused.
#[test]
fn a_glyph_past_the_scratch_budget_is_refused_and_counted_once() {
    let data = ink_cases();
    let plain = Face::parse(&data).unwrap();
    let mut face = Face::parse(&data).unwrap();
    at_weight(&mut face, 900);
    let heavy = glyph(&face, 0x48);
    let scale = Scale::of(14.0, face.units_per_em());
    assert!(
        face.outline_need(heavy).value() > VAR_POINTS_MAX,
        "the premise: {} points",
        face.outline_need(heavy).value()
    );

    let mut atlas = Atlas::new();
    assert!(
        atlas
            .slot(&plain, FaceIx::PRIMARY, heavy, scale, Phase::ZERO)
            .is_some(),
        "the control: the default instance draws it"
    );
    assert_eq!(atlas.stats().refused.value(), 0, "the control");

    let mut atlas = Atlas::new();
    for _ in 0..2 {
        assert_eq!(
            atlas.slot(&face, FaceIx::PRIMARY, heavy, scale, Phase::ZERO),
            None
        );
        assert_eq!(atlas.stats().refused.value(), 1, "counted once");
    }
    // A glyph within the budget still draws after the refusal.
    let a = glyph(&face, 0x61);
    assert!(
        atlas
            .slot(&face, FaceIx::PRIMARY, a, scale, Phase::ZERO)
            .is_some()
    );
    assert_eq!(atlas.stats().refused.value(), 1);
}

/// **A layered glyph paints its layers at the face's coordinates, and one
/// whose layer is past the budget is refused whole.** `colr` paints `a`
/// and the acute, which move at wght 900, so its image there differs from
/// the default one in size; `colrheavy` paints `heavy` under `a`, and
/// rather than paint `a` alone it is refused and counted -- once, though
/// its own outline, which is empty, is then tried and draws nothing.
#[test]
fn a_layered_glyph_paints_varied_layers_or_is_refused_whole() {
    let data = ink_cases();
    let plain = Face::parse(&data).unwrap();
    let mut face = Face::parse(&data).unwrap();
    at_weight(&mut face, 900);
    let scale = Scale::of(40.0, face.units_per_em());
    let (colr, heavy) = (glyph(&face, 0x43), glyph(&face, 0x44));

    let mut atlas = Atlas::new();
    let default = atlas
        .slot(&plain, FaceIx::PRIMARY, colr, scale, Phase::ZERO)
        .expect("the control: the default instance paints it");
    let mut atlas = Atlas::new();
    let varied = atlas
        .slot(&face, FaceIx::PRIMARY, colr, scale, Phase::ZERO)
        .expect("a varied layered glyph paints");
    assert_eq!(varied.kind, Kind::Color);
    assert_ne!(
        (varied.w, varied.h),
        (default.w, default.h),
        "the layers' varied box, not the default's"
    );
    assert_eq!(atlas.stats().refused.value(), 0);

    assert_eq!(
        atlas.slot(&face, FaceIx::PRIMARY, heavy, scale, Phase::ZERO),
        None
    );
    assert_eq!(atlas.stats().refused.value(), 1, "refused whole, once");
    assert_eq!(atlas.stats().coloured.value(), 1, "only `colr` was painted");
}

/// **The renderer places a fallback mark against varied boxes.**
/// `var/ink-cases.ttf` has no `GPOS`, so an acute on `a` is placed against
/// the two glyphs' ink boxes, which move with the weight: HarfBuzz 12.3.0
/// puts it at x -630, y 112 from its pen at wght 900 and at x -600, y 12 at
/// the default (`baud-font/tests/var/marks.txt`), the pen one 600-unit
/// advance along. Before, the shaper's box was refused at a variation, so
/// the mark stayed at the pen. The renderer grows its scratch to the need
/// the shaper names and shapes again; nothing is refused.
#[test]
fn the_renderer_places_a_fallback_mark_against_varied_boxes() {
    let data = ink_cases();
    let cps = [0x61, 0x301].map(|c| Codepoint::new(c).unwrap());
    for (wght, dx, dy) in [(None, 600 - 600, 12), (Some(900), 600 - 630, 112)] {
        let mut face = Face::parse(&data).unwrap();
        if let Some(w) = wght {
            at_weight(&mut face, w);
        }
        let acute = glyph(&face, 0x301);
        let scale = Scale::of(support::PX, face.units_per_em());
        let faces = [face];
        let mut r = renderer(&faces);
        let got = r.cluster(&cps, FaceStyle::Regular, CellSpan::One);
        let mark = got.iter().find(|g| g.id == acute).expect("the acute");
        let px = |u: i32| scale.px(u as f32).round() as i32;
        assert_eq!((mark.dx, mark.dy), (px(dx), px(dy)), "wght {wght:?}");
        assert_eq!(r.ink_refused().value(), 0, "wght {wght:?}");
    }
}

/// **A base box past the budget is refused and counted, and the mark on
/// it still gets its box.** `heavy` (U+0048 in the fixture) under an acute
/// at wght 900: the first pass refuses both boxes, the scratch grows to
/// the budget rather than to `heavy`'s need, and the second pass refuses
/// `heavy` alone -- one box counted, not two. The control: at the default
/// instance nothing needs scratch and nothing is refused.
#[test]
fn a_base_box_past_the_budget_is_refused_and_counted() {
    let data = ink_cases();
    let cps = [0x48, 0x301].map(|c| Codepoint::new(c).unwrap());
    for (wght, refused) in [(None, 0), (Some(900), 1)] {
        let mut face = Face::parse(&data).unwrap();
        if let Some(w) = wght {
            at_weight(&mut face, w);
        }
        let faces = [face];
        let mut r = renderer(&faces);
        r.cluster(&cps, FaceStyle::Regular, CellSpan::One);
        assert_eq!(r.ink_refused().value(), refused, "wght {wght:?}");
    }
}
