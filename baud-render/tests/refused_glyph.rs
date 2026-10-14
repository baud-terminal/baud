//! What a cell shows for a glyph its face refuses.
//!
//! A `glyf` composite whose first component is placed by matched points is
//! refused (`Drawn::Refused`): the face draws nothing for it. The cell must
//! still show something, because TUS § 5.3 says a character an
//! implementation cannot render "can only be indicated by some
//! placeholder" (`[[source: unicode18-ch5]]`), and a silent blank is a
//! defect here (e0a611d). So the stack passes over a face whose mapped
//! glyph is refused, to the next face that carries the character, as it
//! passes over one that does not map it; when no other face does, the cell
//! draws the primary's `.notdef`. A colour glyph is the exception: the atlas
//! draws its strike image or its `COLR` layers before its outline, so a
//! refused outline does not stop it drawing, and the stack keeps its face.
//!
//! The fonts: `var/component-cases.ttf` maps `S` to `anchfirst` and
//! `var/vertical-cases.ttf` maps `L` to `firstm`, both refused; the second's
//! `.notdef` is a box, so a placeholder drawn from it has ink.
//! `var/colour-refused-cases.ttf` maps `U+25B6` to `play`, a `COLR` glyph
//! whose own outline is refused. Stock JetBrains Mono carries all three
//! characters, as text, and refuses nothing.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, GlyphId, ReadLen, Rows, WinSize};
use baud_font::{Bounds, Drawn, Face, Phase, Scale};
use baud_render::{
    Atlas, Blend, CellSpan, Damage, FaceIx, FaceRole, FaceStyle, Kind, Presentation, PxSize,
    Renderer, Slot, Stack, Theme,
};

const PX: f32 = 16.0;

fn file(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn component_cases() -> Vec<u8> {
    file("baud-font/tests/var/component-cases.ttf")
}

fn vertical_cases() -> Vec<u8> {
    file("baud-font/tests/var/vertical-cases.ttf")
}

fn colour_refused_cases() -> Vec<u8> {
    file("baud-font/tests/var/colour-refused-cases.ttf")
}

fn jetbrains_mono() -> Vec<u8> {
    file("baud-app/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf")
}

fn cp(c: char) -> Codepoint {
    Codepoint::new(u32::from(c)).unwrap()
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

/// The premise every test here rests on: the fixture refuses the glyph it
/// maps `c` to, and draws its control, `A`.
fn refused<'a>(face: &Face<'a>, c: char) -> GlyphId {
    let g = face.glyph(cp(c)).unwrap();
    assert_eq!(face.outline(g, &mut Bounds::new()), Drawn::Refused, "{c}");
    let a = face.glyph(cp('A')).unwrap();
    assert_ne!(
        face.outline(a, &mut Bounds::new()),
        Drawn::Refused,
        "the control"
    );
    g
}

/// **A refused glyph falls back to the next face that carries the
/// character**, as a character the face does not map does: `S` on a stack
/// of the component cases and JetBrains Mono is JetBrains Mono's `S`, by
/// every selection, through the renderer's table, and in the cell's mask,
/// texel for texel. The control: `A`, which the fixture draws, stays on the
/// fixture. The same for `L` and `firstm` in the vertical cases. The table
/// counts each character it passed over once, when its block is decided:
/// `S` alone in the component cases; `L` and `M` (`vnest`, refused one
/// level down) in the vertical cases.
#[test]
fn a_refused_glyph_falls_back_to_the_next_face_that_carries_it() {
    let jbm = jetbrains_mono();
    for (fixture, c, passed) in [(component_cases(), 'S', 1), (vertical_cases(), 'L', 2)] {
        let faces = [Face::parse(&fixture).unwrap(), Face::parse(&jbm).unwrap()];
        refused(&faces[0], c);
        let next = FaceIx::PRIMARY;
        let stack = Stack::new(&faces).unwrap();
        let fallback = stack.each().nth(1).unwrap();
        assert_ne!(fallback, next);
        assert_eq!(stack.select(cp('A')), next, "the control");
        assert_eq!(stack.select(cp(c)), fallback, "{c}: select");
        assert_eq!(
            stack.select_styled(cp(c), FaceStyle::Regular),
            fallback,
            "{c}: select_styled"
        );
        assert_eq!(
            stack.select_presented(cp(c), FaceStyle::Regular, Presentation::Text),
            fallback,
            "{c}: select_presented"
        );
        let mut r = renderer(stack);
        assert_eq!(
            r.face_for(&[cp('A')], FaceStyle::Regular),
            next,
            "the control"
        );
        assert_eq!(
            r.face_for(&[cp(c)], FaceStyle::Regular),
            fallback,
            "{c}: the table"
        );
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
            "{c}: JetBrains Mono's mask"
        );
        assert_eq!(r.atlas_stats().refused.value(), 0, "{c}: never asked");
        r.single(cp(c), FaceStyle::Regular, CellSpan::One);
        assert_eq!(r.face_table().refused().value(), passed, "{c}: once");
    }
}

/// **A styled face that refuses a glyph falls back to the chain**, as one
/// that does not map the character does: bold `S` with the component cases
/// as the Bold face is drawn from JetBrains Mono, the regular face. The
/// control: bold `A` is the Bold face's.
#[test]
fn a_styled_face_that_refuses_a_glyph_falls_back_to_the_chain() {
    let (jbm, fixture) = (jetbrains_mono(), component_cases());
    let faces = [Face::parse(&jbm).unwrap(), Face::parse(&fixture).unwrap()];
    refused(&faces[1], 'S');
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    let stack = Stack::with_roles(&faces, &roles).unwrap();
    let bold = stack.each().nth(1).unwrap();
    assert_eq!(
        stack.select_styled(cp('A'), FaceStyle::Bold),
        bold,
        "the control"
    );
    assert_eq!(
        stack.select_styled(cp('S'), FaceStyle::Bold),
        FaceIx::PRIMARY
    );
    let mut r = renderer(stack);
    assert_eq!(r.face_for(&[cp('A')], FaceStyle::Bold), bold, "the control");
    assert_eq!(r.face_for(&[cp('S')], FaceStyle::Bold), FaceIx::PRIMARY);
    assert_eq!(r.face_table().refused().value(), 1, "passed over once");
}

/// **A refused glyph no other face carries draws the primary's
/// `.notdef`**, never blank: `L` on a stack of the vertical cases alone
/// takes the primary, whose glyph for it is refused, and the cell's slot is
/// the primary's `.notdef` box, texel for texel. Counted once however often
/// it is asked, by the atlas, which drew the box; the table, which kept the
/// primary for `L` and `M`, counts neither.
#[test]
fn a_refused_glyph_no_other_face_carries_draws_the_primarys_notdef() {
    let fixture = vertical_cases();
    let faces = [Face::parse(&fixture).unwrap()];
    let firstm = refused(&faces[0], 'L');
    let mut r = renderer(Stack::new(&faces).unwrap());
    for _ in 0..2 {
        let single = r
            .single(cp('L'), FaceStyle::Regular, CellSpan::One)
            .expect("the cell draws");
        assert_eq!((single.face, single.glyph), (FaceIx::PRIMARY, firstm));
        let slot = single.slot.expect("never blank");
        let phase = r.phase_in(FaceIx::PRIMARY, cp('L'), CellSpan::One);
        assert_eq!(
            (texels(r.atlas(), slot), slot.w.0, slot.h.0),
            mask(&faces[0], GlyphId::NOTDEF, phase),
            "the primary's .notdef"
        );
        assert_eq!(r.atlas_stats().refused.value(), 1, "counted once");
        assert_eq!(r.face_table().refused().value(), 0, "kept, not passed over");
    }
    // The control: `A` draws its own glyph, not the box.
    let a = r
        .single(cp('A'), FaceStyle::Regular, CellSpan::One)
        .and_then(|s| s.slot)
        .unwrap();
    let phase = r.phase_in(FaceIx::PRIMARY, cp('A'), CellSpan::One);
    assert_ne!(
        texels(r.atlas(), a),
        mask(&faces[0], GlyphId::NOTDEF, phase).0,
        "the control"
    );
}

/// How many of the pixels `text` paints have a hue, on a stack of the colour
/// cases and JetBrains Mono: a colour glyph's layers (red and blue) have
/// many, a text glyph drawn in the theme's grey none.
fn hued(text: &str) -> usize {
    let (fixture, jbm) = (colour_refused_cases(), jetbrains_mono());
    let faces = [Face::parse(&fixture).unwrap(), Face::parse(&jbm).unwrap()];
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
    c.pixels
        .iter()
        .filter(|&&p| {
            let ch = [(p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff];
            ch.iter().max().unwrap() - ch.iter().min().unwrap() > 24
        })
        .count()
}

/// **A colour glyph whose outline is refused still draws in colour**, so
/// the stack keeps its face: the atlas draws a `COLR` glyph's layers before
/// its outline. `U+25B6` with `U+FE0F` on a stack of the colour cases and
/// JetBrains Mono is the colour face's -- by `select_presented` and the
/// table -- and its slot is a colour image, nothing refused; `select`,
/// the first face that carries it, keeps the colour face too. Without the
/// selector it asks for text (`EmojiDefault::Font`, the default; `U+25B6`
/// is not Emoji_Presentation) and JetBrains Mono's text glyph draws. Nothing
/// is passed over in `FaceTable::refused`. On the pixel path,
/// `U+25B6 U+FE0F` paints hued pixels and `U+25B6` none.
#[test]
fn a_colour_glyph_whose_outline_is_refused_still_draws_in_colour() {
    let (fixture, jbm) = (colour_refused_cases(), jetbrains_mono());
    let faces = [Face::parse(&fixture).unwrap(), Face::parse(&jbm).unwrap()];
    let (play, vs16) = (cp('\u{25b6}'), cp('\u{fe0f}'));
    // The premises: drawn by a font, not procedurally; the fixture's glyph
    // is refused and colour; JetBrains Mono's is text.
    assert!(!baud_render::boxes::covers(play));
    let g = refused(&faces[0], '\u{25b6}');
    assert!(faces[0].refuses(g) && faces[0].is_colour(g));
    let text = faces[1].glyph(play).unwrap();
    assert!(!faces[1].is_colour(text));
    let stack = Stack::new(&faces).unwrap();
    let fallback = stack.each().nth(1).unwrap();
    let regular = FaceStyle::Regular;
    assert_eq!(stack.select(play), FaceIx::PRIMARY, "select");
    assert_eq!(
        stack.select_styled(play, regular),
        FaceIx::PRIMARY,
        "select_styled"
    );
    assert_eq!(
        stack.select_presented(play, regular, Presentation::Colour),
        FaceIx::PRIMARY,
        "select_presented, colour"
    );
    assert_eq!(
        stack.select_presented(play, regular, Presentation::Text),
        fallback,
        "select_presented, text"
    );
    let mut r = renderer(stack);
    assert_eq!(
        r.face_for(&[play, vs16], regular),
        FaceIx::PRIMARY,
        "the table, with U+FE0F"
    );
    assert_eq!(r.face_for(&[play], regular), fallback, "the table, bare");
    assert_eq!(r.face_table().refused().value(), 0, "nothing passed over");
    let mut atlas = Atlas::new();
    let slot = atlas
        .slot(
            &faces[0],
            FaceIx::PRIMARY,
            g,
            Scale::of(PX, faces[0].units_per_em()),
            Phase::ZERO,
        )
        .expect("the layers have ink");
    assert_eq!(slot.kind, Kind::Color, "drawn from its layers");
    assert_eq!(atlas.stats().refused.value(), 0, "nothing refused");
    assert!(hued("\u{25b6}\u{fe0f}") > 0, "U+FE0F draws the layers");
    assert_eq!(hued("\u{25b6}"), 0, "bare, JetBrains Mono's text glyph");
}
