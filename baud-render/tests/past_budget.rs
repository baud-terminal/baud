//! What a cell shows for a glyph past the scratch budget.
//!
//! A glyph drawn through scratch -- every TrueType glyph of a face read at
//! a variation, and a composite that places a component by matched points
//! at any instance -- needs one slot of scratch a point, and the atlas
//! grows its scratch no further than `VAR_POINTS_MAX`, HarfBuzz's own
//! 200,000-point bound. A glyph past it cannot be drawn. The cell must
//! still show something, because TUS § 5.3 says a character an
//! implementation cannot render "can only be indicated by some
//! placeholder" (`[[source: unicode18-ch5]]`), and a silent blank is a
//! defect here (e0a611d). So a glyph past the budget is treated as a
//! refused one is (`refused_glyph.rs`): the stack passes over its face for
//! the next face that carries the character, and when none does, the cell
//! draws the primary's `.notdef`.
//!
//! The font is baud's own, `var/budget-cases.ttf`
//! (`scripts/budget-cases-font.py`): `H` is `heavy`, 240,000 points
//! flattened, past the budget at a variation and streamed without scratch
//! at the default instance; `K` is `heavym`, `heavy` and a component
//! placed by matched points, past the budget at every instance; `D` and
//! `E` are `COLR` glyphs painting `heavy`, whose own outlines are empty
//! and 240,000 points; `U+E123` and `U+E124` are `COLR` glyphs over `a`'s
//! box, one painting `heavy` and one a layer the face refuses; its
//! `.notdef` is a box with a hole. A colour glyph whose layers cannot draw
//! is passed over when its own outline is empty or past the budget, and
//! draws that outline when it has one. Stock JetBrains Mono carries `H`,
//! `K`, `D` and `E`, as text, and draws them without scratch; it maps
//! neither `U+E123` nor `U+E124`.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, GlyphId, ReadLen, Rows, WinSize};
use baud_font::var::{Fixed, Variation};
use baud_font::{Face, Phase, Scale, Tag};
use baud_render::{
    Atlas, Blend, CellSpan, Damage, FaceIx, FaceStyle, Presentation, PxSize, Renderer, Slot, Stack,
    Theme, VAR_POINTS_MAX,
};

const PX: f32 = 16.0;

fn file(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn budget_cases() -> Vec<u8> {
    file("baud-font/tests/var/budget-cases.ttf")
}

fn jetbrains_mono() -> Vec<u8> {
    file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
}

fn cp(c: char) -> Codepoint {
    Codepoint::new(u32::from(c)).unwrap()
}

/// The fixture, at wght 900 or at its default instance.
fn fixture(data: &[u8], wght: Option<i32>) -> Face<'_> {
    let mut face = Face::parse(data).unwrap();
    if let Some(w) = wght {
        face.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wght"),
            value: Fixed(w << 16),
        }]);
    }
    face
}

fn renderer<'f, 'a>(stack: Stack<'f, 'a>) -> Renderer<'f, 'a> {
    Renderer::new(
        stack,
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(8),
        Rows::of_index(2),
    )
    .unwrap()
}

/// The texels of `s` in `atlas`'s mask sheet, row by row.
fn texels(atlas: &Atlas, s: Slot) -> Vec<u8> {
    let side = usize::try_from(atlas.side().value()).unwrap();
    let (x, y) = (usize::from(s.x.0), usize::from(s.y.0));
    let (w, h) = (usize::from(s.w.0), usize::from(s.h.0));
    (0..h)
        .flat_map(|row| atlas.pixels()[(y + row) * side + x..][..w].iter().copied())
        .collect()
}

/// `g`'s mask in `face` drawn by an atlas of its own: the oracle a cell's
/// slot is held to.
fn mask(face: &Face<'_>, g: GlyphId, phase: Phase) -> (Vec<u8>, u16, u16) {
    let mut atlas = Atlas::new();
    let s = atlas
        .slot(
            face,
            FaceIx::PRIMARY,
            g,
            Scale::of(PX, face.units_per_em()),
            phase,
        )
        .expect("the oracle glyph has ink");
    (texels(&atlas, s), s.w.0, s.h.0)
}

/// The premise: the glyph `c` maps to, or a layer it paints, needs more
/// scratch than the budget, and the face does not refuse it -- so it is
/// the budget, and not a refusal, that stops it.
fn past(face: &Face<'_>, c: char) -> GlyphId {
    let g = face.glyph(cp(c)).unwrap();
    assert!(!face.refuses(g), "{c}: not refused");
    let need = |g: GlyphId| face.outline_need(g).value();
    let layered = face
        .layers(g)
        .is_some_and(|ls| ls.into_iter().any(|l| need(l.glyph) > VAR_POINTS_MAX));
    assert!(
        need(g) > VAR_POINTS_MAX || layered,
        "{c}: {} points",
        need(g)
    );
    g
}

/// **A glyph past the budget falls back to the next face that carries the
/// character**, as a refused glyph does: on a stack of the budget cases at
/// wght 900 and JetBrains Mono, `H`, `K`, `D` and `E` are JetBrains
/// Mono's, by every selection, through the renderer's table, and in the
/// cell's mask, texel for texel; nothing is asked of the fixture, so the
/// atlas refuses nothing. `D` and `E` are colour, but their layers cannot
/// draw, so they are passed over in both presentations. The table counts
/// each character it passed over once, when its block is decided. The
/// controls: `a`, which draws within the budget, stays on the fixture; at
/// the default instance `H` streams and the layers of `D` and `E` draw,
/// so all three stay, and `K`, whose matched component is drawn through
/// scratch at every instance, is passed over there too.
#[test]
fn a_glyph_past_the_budget_falls_back_to_the_next_face_that_carries_it() {
    let (data, jbm) = (budget_cases(), jetbrains_mono());
    for (wght, moved, kept) in [
        (Some(900), &['H', 'K', 'D', 'E'][..], &['a'][..]),
        (None, &['K'][..], &['a', 'H', 'D', 'E'][..]),
    ] {
        let faces = [fixture(&data, wght), Face::parse(&jbm).unwrap()];
        for &c in moved {
            past(&faces[0], c);
        }
        let stack = Stack::new(&faces).unwrap();
        let fallback = stack.each().nth(1).unwrap();
        assert_ne!(fallback, FaceIx::PRIMARY);
        for &c in kept {
            assert_eq!(
                stack.select(cp(c)),
                FaceIx::PRIMARY,
                "{c} {wght:?}: control"
            );
        }
        for &c in moved {
            assert_eq!(stack.select(cp(c)), fallback, "{c} {wght:?}: select");
            assert_eq!(
                stack.select_styled(cp(c), FaceStyle::Regular),
                fallback,
                "{c} {wght:?}: select_styled"
            );
            for p in [Presentation::Text, Presentation::Colour] {
                assert_eq!(
                    stack.select_presented(cp(c), FaceStyle::Regular, p),
                    fallback,
                    "{c} {wght:?}: select_presented {p:?}"
                );
            }
        }
        let mut r = renderer(stack);
        let vs16 = cp('\u{fe0f}');
        for &c in kept {
            // A colour glyph is the colour presentation's: asked for with
            // `U+FE0F`, since bare it asks for text.
            let colour = faces[0].is_colour(faces[0].glyph(cp(c)).unwrap());
            let cps = if colour {
                vec![cp(c), vs16]
            } else {
                vec![cp(c)]
            };
            assert_eq!(
                r.face_for(&cps, FaceStyle::Regular),
                FaceIx::PRIMARY,
                "{c} {wght:?}: control"
            );
        }
        for &c in moved {
            for cps in [vec![cp(c)], vec![cp(c), vs16]] {
                assert_eq!(
                    r.face_for(&cps, FaceStyle::Regular),
                    fallback,
                    "{c} {wght:?}: the table, {} codepoints",
                    cps.len()
                );
            }
            let single = r
                .single(cp(c), FaceStyle::Regular, CellSpan::One)
                .expect("the cell draws");
            let want = faces[1].glyph(cp(c)).unwrap();
            assert_eq!((single.face, single.glyph), (fallback, want), "{c}");
            let slot = single.slot.expect("never blank");
            let phase = r.phase_in(fallback, cp(c), CellSpan::One);
            assert_eq!(
                (texels(r.atlas(), slot), slot.w.0, slot.h.0),
                mask(&faces[1], want, phase),
                "{c} {wght:?}: JetBrains Mono's mask"
            );
        }
        assert_eq!(r.atlas_stats().refused.value(), 0, "{wght:?}: never asked");
        assert_eq!(
            r.face_table().refused().value(),
            u64::try_from(moved.len()).unwrap(),
            "{wght:?}: once each"
        );
    }
}

/// **A glyph past the budget no other face carries draws the primary's
/// `.notdef`**, never blank: on a stack of the budget cases alone at wght
/// 900, `H`, `K`, `D` and `E` take the primary, and each cell's slot is the
/// primary's `.notdef` box, texel for texel. `D`'s layers cannot draw and
/// its own outline is empty, so it shows the box too. Counted once per
/// refusal however often they are asked, by the atlas, which drew the box:
/// once for `H`, `K` and `D` each, and twice for `E`, whose layers and own
/// outline are refused in turn; the table, which kept the primary, counts
/// none. The controls: `a` draws its own glyph, not the box; at the
/// default instance `H` draws its own, which streams, while `K` still
/// shows the box.
#[test]
fn a_glyph_past_the_budget_no_other_face_carries_draws_the_primarys_notdef() {
    let data = budget_cases();
    for (wght, boxed, own, refused) in [
        (Some(900), &['H', 'K', 'D', 'E'][..], &['a'][..], 5),
        (None, &['K'][..], &['a', 'H'][..], 1),
    ] {
        let faces = [fixture(&data, wght)];
        for &c in boxed {
            past(&faces[0], c);
        }
        let mut r = renderer(Stack::new(&faces).unwrap());
        for _ in 0..2 {
            for &c in boxed {
                let single = r
                    .single(cp(c), FaceStyle::Regular, CellSpan::One)
                    .expect("the cell draws");
                let g = faces[0].glyph(cp(c)).unwrap();
                assert_eq!((single.face, single.glyph), (FaceIx::PRIMARY, g), "{c}");
                let slot = single.slot.expect("never blank");
                let phase = r.phase_in(FaceIx::PRIMARY, cp(c), CellSpan::One);
                assert_eq!(
                    (texels(r.atlas(), slot), slot.w.0, slot.h.0),
                    mask(&faces[0], GlyphId::NOTDEF, phase),
                    "{c} {wght:?}: the primary's .notdef"
                );
            }
            assert_eq!(
                r.atlas_stats().refused.value(),
                refused,
                "{wght:?}: counted once"
            );
            assert_eq!(r.face_table().refused().value(), 0, "kept, not passed over");
        }
        for &c in own {
            let s = r
                .single(cp(c), FaceStyle::Regular, CellSpan::One)
                .and_then(|s| s.slot)
                .unwrap();
            let phase = r.phase_in(FaceIx::PRIMARY, cp(c), CellSpan::One);
            assert_ne!(
                texels(r.atlas(), s),
                mask(&faces[0], GlyphId::NOTDEF, phase).0,
                "{c} {wght:?}: the control"
            );
        }
    }
}

/// **A colour glyph whose layers cannot draw draws its own outline when it
/// has one**, as the `COLR` text allows ("If the base glyph ID has an
/// outline, that may be rendered as a non-color glyph instead",
/// `[[source: ms-opentype-colr]]`): beside JetBrains Mono, which maps
/// neither character, `U+E123` (`colrink`, a layer past the budget at
/// wght 900) and `U+E124` (`colrref`, a layer the face refuses, at the
/// default instance) stay on the fixture by every selection and through
/// the table with and without `U+FE0F`, and the cell is a mask of the
/// glyph's own outline, `a`'s box, texel for texel against `a` drawn by an
/// atlas of its own, not either face's `.notdef`. The atlas counts the
/// refused layer once, the table nothing. The control: at the default
/// instance `colrink`'s layers draw, and its slot is a colour image.
#[test]
fn a_colour_glyph_whose_layers_cannot_draw_draws_its_own_outline() {
    let (data, jbm) = (budget_cases(), jetbrains_mono());
    let plain = fixture(&data, None);
    let a = plain.glyph(cp('a')).unwrap();
    let vs16 = cp('\u{fe0f}');
    for (c, wght) in [('\u{e123}', Some(900)), ('\u{e124}', None)] {
        let faces = [Face::parse(&jbm).unwrap(), fixture(&data, wght)];
        assert!(
            faces[0].glyph(cp(c)).is_none(),
            "{c:?}: JetBrains Mono lacks it"
        );
        let g = faces[1].glyph(cp(c)).unwrap();
        let cannot =
            |g: GlyphId| faces[1].refuses(g) || faces[1].outline_need(g).value() > VAR_POINTS_MAX;
        let layers = faces[1].layers(g).expect("a COLR glyph");
        assert!(
            layers.into_iter().any(|l| cannot(l.glyph)),
            "{c:?}: a layer cannot draw"
        );
        assert!(!cannot(g), "{c:?}: its outline can");
        let stack = Stack::new(&faces).unwrap();
        let fixture_ix = stack.each().nth(1).unwrap();
        assert_ne!(fixture_ix, FaceIx::PRIMARY);
        assert_eq!(stack.select(cp(c)), fixture_ix, "{c:?}: select");
        assert_eq!(
            stack.select_styled(cp(c), FaceStyle::Regular),
            fixture_ix,
            "{c:?}: select_styled"
        );
        for p in [Presentation::Text, Presentation::Colour] {
            assert_eq!(
                stack.select_presented(cp(c), FaceStyle::Regular, p),
                fixture_ix,
                "{c:?}: select_presented {p:?}"
            );
        }
        let mut r = renderer(stack);
        for cps in [vec![cp(c)], vec![cp(c), vs16]] {
            assert_eq!(
                r.face_for(&cps, FaceStyle::Regular),
                fixture_ix,
                "{c:?}: the table, {} codepoints",
                cps.len()
            );
        }
        let single = r
            .single(cp(c), FaceStyle::Regular, CellSpan::One)
            .expect("the cell draws");
        assert_eq!((single.face, single.glyph), (fixture_ix, g), "{c:?}");
        let slot = single.slot.expect("never blank");
        assert_eq!(slot.kind, baud_render::Kind::Mask, "{c:?}: as text");
        let phase = r.phase_in(fixture_ix, cp(c), CellSpan::One);
        let own = mask(&plain, a, phase);
        assert_ne!(
            own,
            mask(&faces[1], GlyphId::NOTDEF, phase),
            "the premise: a's box is not the fixture's .notdef"
        );
        assert_ne!(
            own,
            mask(&faces[0], GlyphId::NOTDEF, phase),
            "the premise: a's box is not JetBrains Mono's .notdef"
        );
        assert_eq!(
            (texels(r.atlas(), slot), slot.w.0, slot.h.0),
            own,
            "{c:?}: its own outline"
        );
        assert_eq!(r.atlas_stats().refused.value(), 1, "{c:?}: the layer");
        assert_eq!(r.face_table().refused().value(), 0, "{c:?}: kept");
    }
    let mut atlas = Atlas::new();
    let ink = plain.glyph(cp('\u{e123}')).unwrap();
    let slot = atlas
        .slot(
            &plain,
            FaceIx::PRIMARY,
            ink,
            Scale::of(PX, plain.units_per_em()),
            Phase::ZERO,
        )
        .expect("its layers draw");
    assert_eq!(slot.kind, baud_render::Kind::Color, "the control");
    assert_eq!(atlas.stats().refused.value(), 0);
}

/// **A colour glyph whose layers draw within the budget is the colour
/// face's**, the control for the passing over above: at the default
/// instance `D` with `U+FE0F` is the fixture's, and its slot is a colour
/// image, nothing refused; bare, it asks for text and JetBrains Mono's
/// `D` draws, as it did before a budget was tested.
#[test]
fn a_colour_glyph_whose_layers_draw_is_the_colour_faces() {
    let (data, jbm) = (budget_cases(), jetbrains_mono());
    let faces = [fixture(&data, None), Face::parse(&jbm).unwrap()];
    let d = faces[0].glyph(cp('D')).unwrap();
    assert!(faces[0].is_colour(d));
    let stack = Stack::new(&faces).unwrap();
    let fallback = stack.each().nth(1).unwrap();
    let mut r = renderer(stack);
    let vs16 = cp('\u{fe0f}');
    assert_eq!(
        r.face_for(&[cp('D'), vs16], FaceStyle::Regular),
        FaceIx::PRIMARY
    );
    assert_eq!(r.face_for(&[cp('D')], FaceStyle::Regular), fallback, "bare");
    let mut atlas = Atlas::new();
    let slot = atlas
        .slot(
            &faces[0],
            FaceIx::PRIMARY,
            d,
            Scale::of(PX, faces[0].units_per_em()),
            Phase::ZERO,
        )
        .expect("its layers draw");
    assert_eq!(slot.kind, baud_render::Kind::Color);
    assert_eq!(atlas.stats().refused.value(), 0);
}

/// The canvas `text` paints on a stack of the budget cases alone at wght
/// 900, through the CPU path, and how many of its pixels differ from the
/// background, read from its last pixel, a cell nothing is written to.
fn painted(text: &str) -> (Vec<u32>, usize) {
    let data = budget_cases();
    let faces = [fixture(&data, Some(900))];
    let ws = WinSize {
        ws_row: 2,
        ws_col: 8,
    };
    let mut r = renderer(Stack::new(&faces).unwrap());
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let bytes = format!("{text}\x1b[?25l");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let bg = c.pixels[c.pixels.len() - 1];
    let lit = c.pixels.iter().filter(|&&p| p != bg).count();
    (c.pixels, lit)
}

/// **On the pixel path a glyph past the budget paints the `.notdef`**:
/// the same canvas, pixel for pixel, as a character the face does not map
/// paints in its place (`Z`, which the primary's `.notdef` draws), and not
/// a blank one.
#[test]
fn a_glyph_past_the_budget_paints_the_notdef() {
    let (notdef, lit) = painted("Z");
    assert!(lit > 0, "the control: the .notdef has ink");
    for c in ["H", "K", "D", "E"] {
        assert!(painted(c).0 == notdef, "{c}: the canvas differs");
    }
}
