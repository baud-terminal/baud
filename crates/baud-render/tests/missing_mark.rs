//! A mark the face lacks takes no other mark down with it.
//!
//! Found by the Hebrew design study's critic (2026-10-07): when a letter
//! carries a point the font lacks, the points that sort after it in canonical
//! order were lost too, and nothing counted the loss -- `shin + sin dot +
//! hiriq` drew a bare shin with the sin dot gone. Hebrew with nikud needs
//! every point the font has to draw, whatever else rides on the letter.
//!
//! JetBrains Mono shows it without a Hebrew font: it maps the acute accent
//! U+0301 (combining class 230) and not U+0316, the grave below (class 220),
//! which sorts before it. The base is `q`, which has no precomposed form
//! with either mark, so the acute stays a mark glyph rather than composing
//! into a single letter (as `a` + U+0301 does, into `á`).

mod support;

use baud_domain::Codepoint;
use baud_font::Face;
use baud_render::{CellSpan, Damage, FaceStyle};
use support::*;

fn cps(s: &[u32]) -> Vec<Codepoint> {
    s.iter().map(|&c| Codepoint::new(c).unwrap()).collect()
}

#[test]
fn a_missing_mark_takes_no_other_mark_with_it() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let acute = faces[0]
        .glyph(Codepoint::new(0x301).unwrap())
        .expect("JetBrains Mono maps U+0301");
    assert!(
        faces[0].glyph(Codepoint::new(0x316).unwrap()).is_none(),
        "the premise: JetBrains Mono does not map U+0316"
    );
    let mut r = renderer(&faces);
    let ids = |r: &mut baud_render::Renderer<'_, '_>, s: &[u32]| -> Vec<_> {
        r.cluster(&cps(s), FaceStyle::Regular, CellSpan::One)
            .into_iter()
            .map(|g| g.id)
            .collect()
    };
    // The control: the acute alone on its letter draws.
    let alone = ids(&mut r, &[0x71, 0x301]);
    assert!(
        alone.contains(&acute),
        "the control: q + U+0301 draws the acute ({acute:?}); got {alone:?}"
    );
    // The acute still draws when a mark the face lacks rides with it, in
    // either order it was typed.
    for typed in [[0x71, 0x301, 0x316], [0x71, 0x316, 0x301]] {
        let got = ids(&mut r, &typed);
        assert!(
            got.contains(&acute),
            "{typed:x?}: the acute ({acute:?}) was lost because U+0316 is missing; got {got:?}"
        );
    }
}

/// Draw `text` on a fresh screen and return, for the first cell, the topmost
/// row that carries ink: the acute sits above the `q`, so its ink is what
/// makes the row smaller.
fn top_ink_in_first_cell(faces: &[Face<'_>], text: &str) -> Option<usize> {
    let mut r = renderer(faces);
    let cell_w = usize::try_from(r.metrics().cell_w.value()).unwrap();
    let mut canvas = Canvas::for_renderer(&r);
    replay_final(text.as_bytes(), |t| {
        paint(&mut r, t, &mut canvas, Damage::Full);
    });
    let bg = canvas.pixels[canvas.width * (canvas.height - 1)];
    (0..canvas.height).find(|&y| (0..cell_w).any(|x| canvas.pixels[y * canvas.width + x] != bg))
}

/// **And it draws**: through the core and the CPU path, the acute's ink is
/// still above the `q` when a mark the face lacks rides with it. At 1392223,
/// before a cluster's face was chosen for the whole cluster, the acute
/// vanished and a missing-glyph box drew instead, in either order (measured
/// 2026-10-07 with the scratch renderer).
#[test]
fn a_missing_mark_takes_no_other_mark_with_it_on_screen() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let bare = top_ink_in_first_cell(&faces, "q").expect("q draws");
    let control = top_ink_in_first_cell(&faces, "q\u{301}").expect("q + acute draws");
    assert!(
        control < bare,
        "the control: the acute raises the cell's top ink ({control} against {bare} for q alone)"
    );
    for text in ["q\u{301}\u{316}", "q\u{316}\u{301}"] {
        let top = top_ink_in_first_cell(&faces, text).expect("draws");
        assert!(
            top <= control,
            "{text:?}: the acute is gone (top ink {top}, with the acute {control}, q alone {bare})"
        );
    }
}
