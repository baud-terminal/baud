//! Which face draws a marked cell, and what becomes of a mark no face draws.
//!
//! `missing_mark.rs` is the bug: a cluster's face was picked from its base
//! alone, and a mark that face lacks became a `.notdef` that JetBrains Mono
//! classes as a base with a full advance, so the box drew one cell right and
//! every later mark landed over it. These pin the rule that replaced it
//! (Jev, 0.99): a marked cell is drawn from the first face, its base's face
//! and then the chain, that has the letter and every mark after the
//! shaper's normalisation; when none has, its base's face draws it and the
//! marks it lacks are dropped and counted. A default-ignorable -- a joiner,
//! a variation selector -- is never a mark a face must have.
//!
//! The fonts: JetBrains Mono lacks U+0305, U+0316; Fira Code lacks U+0309,
//! U+0316; each test asserts the premise it rests on.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, GlyphId, ReadLen, Rows};
use baud_font::Face;
use baud_render::{
    CellSpan, ClusterGlyph, Damage, FaceStyle, GlyphMap, GlyphSource, Instance, Overlay, Renderer,
    Resolved, Scene, StyleStamp,
};
use support::*;

fn cp(c: u32) -> Codepoint {
    Codepoint::new(c).unwrap()
}

fn cps(s: &[u32]) -> Vec<Codepoint> {
    s.iter().map(|&c| cp(c)).collect()
}

fn cluster(r: &mut Renderer<'_, '_>, s: &[u32]) -> Vec<ClusterGlyph> {
    r.cluster(&cps(s), FaceStyle::Regular, CellSpan::One)
}

/// The face index each glyph is drawn from.
fn faces_of(glyphs: &[ClusterGlyph]) -> Vec<usize> {
    glyphs
        .iter()
        .map(|g| match g.source {
            GlyphSource::Face(f) => f.index(),
            GlyphSource::Procedural => usize::MAX,
        })
        .collect()
}

/// How many marks the renderer has left out since it was made.
fn unmapped(r: &Renderer<'_, '_>) -> u64 {
    r.marks_unmapped().value()
}

/// The marks `cluster` leaves out of `s`, by the counter.
fn dropped_by(r: &mut Renderer<'_, '_>, s: &[u32]) -> (Vec<ClusterGlyph>, u64) {
    let before = unmapped(r);
    let got = cluster(r, s);
    (got, unmapped(r) - before)
}

fn ids(glyphs: &[ClusterGlyph]) -> Vec<GlyphId> {
    glyphs.iter().map(|g| g.id).collect()
}

/// Where `id` is placed in a cluster's output, in pixels from the cell
/// origin.
fn placed(glyphs: &[ClusterGlyph], id: GlyphId) -> Option<(i32, i32)> {
    glyphs.iter().find(|g| g.id == id).map(|g| (g.dx, g.dy))
}

fn jbm_and_fira() -> (Vec<u8>, Vec<u8>) {
    (
        font("JetBrainsMono-Regular.ttf"),
        font("FiraCode-Regular.ttf"),
    )
}

/// The premises every stack test rests on, asserted where they are used.
fn premises(jbm: &Face<'_>, fira: &Face<'_>) {
    for c in [0x71, 0x301, 0x309] {
        assert!(jbm.glyph(cp(c)).is_some(), "JetBrains Mono maps U+{c:04X}");
    }
    for c in [0x305, 0x316] {
        assert!(jbm.glyph(cp(c)).is_none(), "JetBrains Mono lacks U+{c:04X}");
    }
    for c in [0x71, 0x305] {
        assert!(fira.glyph(cp(c)).is_some(), "Fira Code maps U+{c:04X}");
    }
    for c in [0x309, 0x316] {
        assert!(fira.glyph(cp(c)).is_none(), "Fira Code lacks U+{c:04X}");
    }
}

/// **A mark no face draws is dropped, not boxed, and counted**, and the
/// marks after it sit where they sit without it. Before, `q` + U+0316 +
/// U+0301 on JetBrains Mono shaped to `q`, `.notdef` one cell right, and
/// the acute over the box: the acute's offset is the measure, and the
/// counter says exactly one mark was left out each time.
#[test]
fn a_mark_no_face_draws_is_dropped_not_boxed() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    assert!(faces[0].glyph(cp(0x316)).is_none(), "the premise");
    let acute = faces[0].glyph(cp(0x301)).unwrap();
    let mut r = renderer(&faces);
    let (alone, none) = dropped_by(&mut r, &[0x71, 0x301]);
    assert_eq!(none, 0, "the control: nothing left out of q + U+0301");
    let at = placed(&alone, acute).expect("the control: q + U+0301 draws the acute");
    for typed in [[0x71, 0x316, 0x301], [0x71, 0x301, 0x316]] {
        let (got, n) = dropped_by(&mut r, &typed);
        assert_eq!(n, 1, "{typed:x?}: U+0316 left out, and counted once");
        assert!(
            !ids(&got).contains(&GlyphId::NOTDEF),
            "{typed:x?}: a missing-glyph box drew: {:?}",
            ids(&got)
        );
        assert_eq!(
            ids(&got),
            ids(&alone),
            "{typed:x?}: the letter and the acute, and nothing else"
        );
        assert_eq!(
            placed(&got, acute),
            Some(at),
            "{typed:x?}: the acute sits where it sits alone"
        );
    }
}

/// **A mark the letter's face lacks moves the cluster to a face that has
/// both**: `q` + U+0305 over [JetBrains Mono, Fira Code] is Fira Code's `q`
/// and overline, while `q` + U+0301, which JetBrains Mono has, stays on it.
#[test]
fn a_mark_the_letters_face_lacks_moves_the_cluster_to_a_face_with_both() {
    let (j, f) = jbm_and_fira();
    let faces = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&faces[0], &faces[1]);
    let mut r = renderer(&faces);
    let (control, none) = dropped_by(&mut r, &[0x71, 0x301]);
    assert_eq!(
        faces_of(&control),
        vec![0, 0],
        "the control: q + U+0301 is JetBrains Mono's"
    );
    assert_eq!(none, 0);
    let (got, none) = dropped_by(&mut r, &[0x71, 0x305]);
    assert_eq!(faces_of(&got), vec![1, 1], "q + U+0305 is Fira Code's");
    assert_eq!(none, 0, "a face had it all: nothing left out");
    let fira_q = faces[1].glyph(cp(0x71)).unwrap();
    let overline = faces[1].glyph(cp(0x305)).unwrap();
    assert_eq!(ids(&got), vec![fira_q, overline]);
}

/// **When no face has every mark, the letter's face keeps the cluster** and
/// draws the marks it has: `q` + U+0305 + U+0309 over [JetBrains Mono, Fira
/// Code] -- Fira Code lacks the hook, JetBrains Mono the overline -- is
/// JetBrains Mono's `q` and hook, the hook where it sits alone, and no box.
#[test]
fn with_no_face_for_every_mark_the_letters_face_keeps_the_cluster() {
    let (j, f) = jbm_and_fira();
    let faces = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&faces[0], &faces[1]);
    let hook = faces[0].glyph(cp(0x309)).unwrap();
    let mut r = renderer(&faces);
    let alone = cluster(&mut r, &[0x71, 0x309]);
    let at = placed(&alone, hook).expect("the control: q + U+0309 draws the hook");
    let (got, n) = dropped_by(&mut r, &[0x71, 0x305, 0x309]);
    assert_eq!(faces_of(&got), vec![0, 0], "JetBrains Mono keeps it");
    assert_eq!(n, 1, "the overline left out, and counted once");
    assert_eq!(ids(&got), ids(&alone), "the letter and the hook, no box");
    assert_eq!(
        placed(&got, hook),
        Some(at),
        "the hook sits where it sits alone"
    );
}

/// **A default-ignorable is not a mark a face must have.** A joiner or a
/// variation selector riding on `q` + U+0301 leaves it on JetBrains Mono
/// and draws exactly what `q` + U+0301 draws -- Ghostty and foot fell back,
/// or drew U+FFFD, for the joiners and the marks of direction a face did not
/// map (note AB, section 8). The second stack is
/// the one where requiring them would show: STIX Two Math maps the joiners
/// JetBrains Mono lacks, as well as `q` and the acute, so a rule that asked
/// for them would move the cluster to it.
#[test]
fn an_ignorable_is_not_a_mark_a_face_must_have() {
    let j = font("JetBrainsMono-Regular.ttf");
    let stix = std::fs::read(format!(
        "{}/../baud-app/fonts/stix-two-math/STIXTwoMath-Regular.otf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [Face::parse(&j).unwrap(), Face::parse(&stix).unwrap()];
    for c in [0x71, 0x301, 0x200C, 0x200D] {
        assert!(
            faces[1].glyph(cp(c)).is_some(),
            "the premise: STIX maps U+{c:04X}"
        );
    }
    for c in [0x200C, 0x200D] {
        assert!(
            faces[0].glyph(cp(c)).is_none(),
            "the premise: JBM lacks U+{c:04X}"
        );
    }
    let mut r = renderer(&faces);
    let control = cluster(&mut r, &[0x71, 0x301]);
    assert_eq!(
        faces_of(&control),
        vec![0, 0],
        "the control: JetBrains Mono's"
    );
    for typed in [[0x71, 0x301, 0x200D], [0x71, 0x301, 0x200C]] {
        let (got, n) = dropped_by(&mut r, &typed);
        assert_eq!(
            faces_of(&got),
            vec![0, 0],
            "{typed:x?}: still JetBrains Mono's"
        );
        assert_eq!(n, 0, "{typed:x?}: an ignorable is never left out");
        assert_eq!(
            ids(&got),
            ids(&control),
            "{typed:x?}: what q + U+0301 draws"
        );
    }
}

/// The same over [JetBrains Mono, Fira Code], neither of which maps a joiner,
/// a selector or a mark of direction: nothing moves and nothing is left out.
/// These clusters are covered, so this pins the face, not the drop; the
/// drop's guard is the next test's.
#[test]
fn an_ignorable_no_face_maps_is_not_left_out() {
    let (j, f) = jbm_and_fira();
    let faces = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&faces[0], &faces[1]);
    let mut r = renderer(&faces);
    let control = cluster(&mut r, &[0x71, 0x301]);
    for typed in [
        [0x71, 0x301, 0x200D],
        [0x71, 0x301, 0xFE0F],
        [0x71, 0x301, 0x200F],
    ] {
        let (got, n) = dropped_by(&mut r, &typed);
        assert_eq!(faces_of(&got), vec![0, 0], "{typed:x?}: JetBrains Mono's");
        assert_eq!(n, 0, "{typed:x?}: an ignorable is never left out");
        assert_eq!(
            ids(&got),
            ids(&control),
            "{typed:x?}: what q + U+0301 draws"
        );
    }
}

/// **An ignorable rides through a drop uncounted.** The test above never
/// reaches the drop: JetBrains Mono draws `q` + U+0301 whole, so those
/// clusters are covered and nothing is asked what to leave out. Here U+0316,
/// which no face maps, makes each cluster partial, so the joiner, the
/// selector and the mark of direction stand beside a mark that is dropped:
/// exactly one codepoint is counted, and what draws is `q` + U+0301's.
#[test]
fn an_ignorable_beside_a_dropped_mark_is_not_counted() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for c in [0x316, 0x200D, 0xFE0F, 0x200F] {
        assert!(
            faces[0].glyph(cp(c)).is_none(),
            "the premise: JetBrains Mono lacks U+{c:04X}"
        );
    }
    let mut r = renderer(&faces);
    let control = cluster(&mut r, &[0x71, 0x301]);
    for typed in [
        [0x71, 0x316, 0x301, 0x200D],
        [0x71, 0x316, 0x301, 0xFE0F],
        [0x71, 0x316, 0x301, 0x200F],
    ] {
        let (got, n) = dropped_by(&mut r, &typed);
        assert_eq!(n, 1, "{typed:x?}: U+0316 alone left out");
        assert_eq!(
            ids(&got),
            ids(&control),
            "{typed:x?}: what q + U+0301 draws, no box"
        );
    }
}

/// **A base the face lacks comes apart until nothing it leaves is missing.**
/// U+1F82 is U+1F02 + U+0345, U+1F02 is U+1F00 + U+0300, and U+1F00 is
/// alpha + U+0313: three levels, of which the shaper's normalisation takes
/// two, so the first pass leaves U+1F00 as the base. JetBrains Mono has
/// alpha, the grave and the acute and none of the rest. Judged once, the
/// drop left U+1F00 in place, the shaper took it apart again, and U+0313
/// came out a `.notdef` one cell right that nothing counted; now what is
/// left is normalised and judged again until nothing more is dropped, so
/// both clusters draw what alpha with the marks the face has draws, and
/// every mark the face lacks is counted.
#[test]
fn a_base_the_face_lacks_comes_apart_until_every_mark_left_is_drawn() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    for c in [0x1F82, 0x1F02, 0x1F00, 0x313, 0x345, 0x316] {
        assert!(
            faces[0].glyph(cp(c)).is_none(),
            "the premise: JetBrains Mono lacks U+{c:04X}"
        );
    }
    for c in [0x3B1, 0x300, 0x301] {
        assert!(
            faces[0].glyph(cp(c)).is_some(),
            "the premise: JetBrains Mono maps U+{c:04X}"
        );
    }
    let mut r = renderer(&faces);
    for (typed, left, n) in [
        (&[0x1F82, 0x301][..], &[0x3B1, 0x300, 0x301][..], 2),
        (&[0x1F82, 0x316], &[0x3B1, 0x300], 3),
    ] {
        let (control, none) = dropped_by(&mut r, left);
        assert_eq!(none, 0, "the control {left:x?}: nothing left out");
        let (got, k) = dropped_by(&mut r, typed);
        assert!(
            !ids(&got).contains(&GlyphId::NOTDEF),
            "{typed:x?}: a missing-glyph box drew: {:?}",
            ids(&got)
        );
        assert_eq!(k, n, "{typed:x?}: every mark the face lacks, counted");
        assert_eq!(
            got.iter().map(|g| (g.id, g.dx, g.dy)).collect::<Vec<_>>(),
            control
                .iter()
                .map(|g| (g.id, g.dx, g.dy))
                .collect::<Vec<_>>(),
            "{typed:x?}: drawn as {left:x?}, every glyph where it sits there"
        );
    }
}

/// **A styled face is not in the chain for a regular cluster either**: with
/// Fira Code as the bold face behind JetBrains Mono, a regular `q` + U+0305
/// -- which Fira Code could draw whole -- stays regular, its overline
/// dropped and counted, while a bold one is Fira Code's. The test above
/// cannot show this half: its bold face is last and lacks the mark, so it is
/// never the first face to cover anything.
#[test]
fn a_regular_cluster_never_moves_to_a_styled_face() {
    use baud_render::{Blend, FaceRole, PxSize, Stack, Theme};
    let (j, f) = jbm_and_fira();
    let faces = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&faces[0], &faces[1]);
    let roles = [FaceRole::Fallback, FaceRole::Style(FaceStyle::Bold)];
    let mut r = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let before = unmapped(&r);
    let bold = r.cluster(&cps(&[0x71, 0x305]), FaceStyle::Bold, CellSpan::One);
    assert_eq!(
        faces_of(&bold),
        vec![1, 1],
        "the control: bold is Fira Code's"
    );
    assert_eq!(unmapped(&r), before, "the control: nothing left out");
    let (got, n) = dropped_by(&mut r, &[0x71, 0x305]);
    assert_eq!(faces_of(&got), vec![0], "regular stays on JetBrains Mono");
    assert_eq!(n, 1, "its overline left out, and counted");
}

/// **A pair the face has precomposed still composes**: `a` + U+0301 is the
/// one glyph JetBrains Mono has for U+00E1.
#[test]
fn a_precomposable_pair_still_composes() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let a_acute = faces[0]
        .glyph(cp(0xE1))
        .expect("JetBrains Mono maps U+00E1");
    let mut r = renderer(&faces);
    let (got, n) = dropped_by(&mut r, &[0x61, 0x301]);
    assert_eq!(ids(&got), vec![a_acute]);
    assert_eq!(faces_of(&got), vec![0]);
    assert_eq!(n, 0);
}

/// **A face is judged on what the shaper maps, not on what was typed**:
/// Fira Code has `ą` (U+0105) and not the ogonek U+0328, so `a` + U+0328
/// composes on it and draws whole -- a judgement on the typed codepoints
/// would have moved it to JetBrains Mono for a mark Fira Code never draws.
#[test]
fn a_face_is_judged_after_the_shapers_normalisation() {
    let (j, f) = jbm_and_fira();
    let faces = [Face::parse(&f).unwrap(), Face::parse(&j).unwrap()];
    assert!(
        faces[0].glyph(cp(0x328)).is_none(),
        "the premise: Fira Code lacks U+0328"
    );
    let composed = faces[0]
        .glyph(cp(0x105))
        .expect("the premise: Fira Code maps U+0105");
    assert!(
        faces[1].glyph(cp(0x328)).is_some(),
        "and JetBrains Mono maps U+0328"
    );
    let mut r = renderer(&faces);
    let (got, n) = dropped_by(&mut r, &[0x61, 0x328]);
    assert_eq!(faces_of(&got), vec![0], "Fira Code keeps it");
    assert_eq!(ids(&got), vec![composed], "as the one glyph it has");
    assert_eq!(n, 0);
}

/// **A dropped mark is as if never typed**: what is left is shaped afresh,
/// so a composition the dropped mark blocked takes place. `a` + U+0305 +
/// U+0309 on JetBrains Mono -- the overline, which it lacks, sits between
/// the letter and the hook in canonical order and blocks `ả` -- draws the
/// `ả` it has, exactly as `a` + U+0309 does.
#[test]
fn a_dropped_mark_no_longer_blocks_a_composition() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    assert!(faces[0].glyph(cp(0x305)).is_none(), "the premise");
    let composed = faces[0]
        .glyph(cp(0x1EA3))
        .expect("JetBrains Mono maps U+1EA3");
    let mut r = renderer(&faces);
    let (control, none) = dropped_by(&mut r, &[0x61, 0x309]);
    assert_eq!(
        ids(&control),
        vec![composed],
        "the control: a + U+0309 is one glyph"
    );
    assert_eq!(none, 0);
    let (got, n) = dropped_by(&mut r, &[0x61, 0x305, 0x309]);
    assert_eq!(ids(&got), ids(&control));
    assert_eq!(n, 1);
}

/// **A styled face is not in the fallback chain** (`Stack::select_cluster`):
/// a bold `q` + U+0305, which JetBrains Mono Bold lacks, is looked for in
/// the regular faces like anything else the bold face cannot draw, and drawn
/// from Fira Code; a bold `q` + U+0301 stays bold.
#[test]
fn a_bold_cluster_its_bold_face_cannot_draw_falls_to_the_chain() {
    use baud_render::{Blend, FaceRole, PxSize, Stack, Theme};
    let (j, f) = jbm_and_fira();
    // Stock JetBrains Mono Bold. The embedded Bold is this file with Hebrew
    // added, and what this test reads of it, U+0301 and U+0305, is the same
    // in both.
    let bold = std::fs::read(format!(
        "{}/../baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let faces = [
        Face::parse(&j).unwrap(),
        Face::parse(&f).unwrap(),
        Face::parse(&bold).unwrap(),
    ];
    premises(&faces[0], &faces[1]);
    assert!(
        faces[2].glyph(cp(0x305)).is_none(),
        "the premise: Bold lacks U+0305"
    );
    assert!(faces[2].glyph(cp(0x301)).is_some(), "and maps U+0301");
    let roles = [
        FaceRole::Fallback,
        FaceRole::Fallback,
        FaceRole::Style(FaceStyle::Bold),
    ];
    let mut r = Renderer::new(
        Stack::with_roles(&faces, &roles).unwrap(),
        PxSize::of(PX).unwrap(),
        Theme::dark(),
        Blend::Native,
        baud_domain::Cols::of_index(usize::from(WS.ws_col)),
        baud_domain::Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let bold_of = |r: &mut Renderer<'_, '_>, s: &[u32]| {
        faces_of(&r.cluster(&cps(s), FaceStyle::Bold, CellSpan::One))
    };
    assert_eq!(
        bold_of(&mut r, &[0x71, 0x301]),
        vec![2, 2],
        "the control: bold"
    );
    assert_eq!(bold_of(&mut r, &[0x71, 0x305]), vec![1, 1], "Fira Code's");
}

/// A frame of `text` on a fresh screen through the CPU path, cursor hidden.
fn frame(faces: &[Face<'_>], text: &str) -> (Canvas, usize, usize) {
    let mut r = renderer(faces);
    let m = r.metrics();
    let (cw, ch) = (
        usize::try_from(m.cell_w.value()).unwrap(),
        usize::try_from(m.cell_h.value()).unwrap(),
    );
    let mut canvas = Canvas::for_renderer(&r);
    replay_final(format!("\x1b[?25l{text}").as_bytes(), |t| {
        paint(&mut r, t, &mut canvas, Damage::Full);
    });
    (canvas, cw, ch)
}

/// The pixels of cell `col` on the first row.
fn cell_block(c: &(Canvas, usize, usize), col: usize) -> Vec<u32> {
    let (canvas, cw, ch) = c;
    (0..*ch)
        .flat_map(|y| (col * cw..(col + 1) * cw).map(move |x| (y, x)))
        .map(|(y, x)| canvas.pixels[y * canvas.width + x])
        .collect()
}

/// **On screen, through the run path, between plain cells**: a cluster with
/// a dropped mark draws exactly what it draws without that mark, and a
/// cluster moved to another face draws its overline in its own cell and
/// nothing in the next. Plain neighbours make every one of these split a
/// run on each side -- a run that could not advance past a cell would hang
/// here rather than fail.
#[test]
fn on_screen_a_marked_cell_draws_in_its_own_cell_between_plain_ones() {
    let data = font("JetBrainsMono-Regular.ttf");
    let alone = [Face::parse(&data).unwrap()];
    let (j, f) = jbm_and_fira();
    let both = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&both[0], &both[1]);
    // Dropped: q + U+0316 + U+0301 is q + U+0301, on either stack.
    for faces in [&alone[..], &both[..]] {
        let want = cell_block(&frame(faces, "nq\u{301}n"), 1);
        for text in ["nq\u{316}\u{301}n", "nq\u{301}\u{316}n"] {
            assert!(
                cell_block(&frame(faces, text), 1) == want,
                "{text:?} over {} faces: not what q + U+0301 draws",
                faces.len()
            );
        }
    }
    // Kept on JetBrains Mono: q + U+0305 + U+0309 is q + U+0309.
    let want = cell_block(&frame(&both, "nq\u{309}n"), 1);
    assert!(
        cell_block(&frame(&both, "nq\u{305}\u{309}n"), 1) == want,
        "q + U+0305 + U+0309: not what q + U+0309 draws"
    );
    // Moved to Fira Code: the overline draws above the letter in its own
    // cell, and the cell after it, a space, stays empty.
    let shot = frame(&both, "nq\u{305} n");
    let bg = shot.0.pixels[shot.0.width * (shot.0.height - 1)];
    let top = |block: &[u32]| block.iter().position(|&p| p != bg).map(|i| i / shot.1);
    let plain_q = top(&cell_block(&frame(&both, "nq n"), 1)).expect("q draws");
    let over = top(&cell_block(&shot, 1)).expect("q + U+0305 draws");
    assert!(
        over < plain_q,
        "the overline is above the q (top ink {over} against {plain_q})"
    );
    assert!(
        cell_block(&shot, 2).iter().all(|&p| p == bg),
        "nothing spilled into the next cell"
    );
}

/// The run path's instances and the overlay's records for a screen holding
/// `text`, from one renderer and one atlas.
fn both_paths(faces: &[Face<'_>], text: &str) -> (Vec<Instance>, Vec<Instance>) {
    let mut r = renderer(faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = format!("\x1b[?25l{text}").into_bytes();
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    let mut candidates = Vec::new();
    let mut o = Overlay::new();
    let mut mask = vec![0u8; usize::from(WS.ws_col) * usize::from(WS.ws_row)];
    let mut records = Vec::new();
    // The run path first, so every glyph either path names is in the atlas;
    // then the overlay; then the run path again over the same atlas.
    r.convert(&t, Rows::ZERO, Damage::Full);
    let view = t.view(Rows::ZERO);
    r.sync_styles(&t, &mut styles, &mut stamp);
    map.ensure(view, &styles, &mut r, &mut candidates);
    o.ensure(
        &Scene {
            term: &t,
            view,
            styles: &styles,
            cursor: None,
            highlight: None,
            map: &map,
            candidates: &candidates,
            pan: baud_font::Px::of(0),
            composition: None,
        },
        &mut r,
        &mut mask,
        &mut records,
    );
    let grown = r.atlas_stats().grown;
    r.convert(&t, Rows::ZERO, Damage::Full);
    assert_eq!(r.atlas_stats().grown, grown, "one atlas under both paths");
    (r.instances().copied().collect(), records)
}

fn key(i: &Instance) -> (i16, i16, u16, u16, u16, u16) {
    (i.x.0, i.y.0, i.w.0, i.h.0, i.u.0, i.v.0)
}

/// **The run path and the overlay draw a marked cell alike**: every record
/// the cell path's overlay draws for a marked cell is a glyph the run path
/// draws there, and there are as many as the cluster has glyphs. The ligature
/// case is the overlay's window: `-` starts JetBrains Mono's `->`, so a
/// marked `-` is gathered into a window shaped on the face its *base* maps
/// to, and must instead be left to the cluster path when its marks move it.
#[test]
fn the_run_path_and_the_overlay_draw_a_marked_cell_alike() {
    let (j, f) = jbm_and_fira();
    let both = [Face::parse(&j).unwrap(), Face::parse(&f).unwrap()];
    premises(&both[0], &both[1]);
    for (text, marked) in [
        (" q\u{301}", &[0x71, 0x301][..]),
        (" q\u{305}", &[0x71, 0x305]),
        (" q\u{316}\u{301}", &[0x71, 0x316, 0x301]),
        (" q\u{305}\u{309}", &[0x71, 0x305, 0x309]),
        (" -\u{305}>", &[0x2D, 0x305]),
        (" -\u{301}>", &[0x2D, 0x301]),
    ] {
        let (run, records) = both_paths(&both, text);
        let mut r = renderer(&both);
        let glyphs = cluster(&mut r, marked);
        let inked = glyphs.iter().filter(|g| g.slot.is_some()).count();
        assert_eq!(
            records.len(),
            inked,
            "{text:?}: the overlay drew the cluster's {inked} inked glyphs"
        );
        let mut pool: Vec<_> = run.iter().map(key).collect();
        for rec in &records {
            let k = key(rec);
            let at = pool.iter().position(|p| *p == k);
            assert!(
                at.is_some(),
                "{text:?}: the overlay drew {k:?}, which the run path does not: {pool:?}"
            );
            pool.remove(at.unwrap());
        }
    }
}

/// What the cell path's map and overlay count, alone, for a screen holding
/// `text`: the codepoints left out while they are built on a fresh renderer.
fn overlay_unmapped(faces: &[Face<'_>], text: &str) -> u64 {
    let mut r = renderer(faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = format!("\x1b[?25l{text}").into_bytes();
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut styles = vec![Resolved::default(); t.styles().len()];
    let mut stamp = StyleStamp::default();
    let mut candidates = Vec::new();
    let mut o = Overlay::new();
    let mut mask = vec![0u8; usize::from(WS.ws_col) * usize::from(WS.ws_row)];
    let mut records = Vec::new();
    let view = t.view(Rows::ZERO);
    r.sync_styles(&t, &mut styles, &mut stamp);
    let before = unmapped(&r);
    map.ensure(view, &styles, &mut r, &mut candidates);
    o.ensure(
        &Scene {
            term: &t,
            view,
            styles: &styles,
            cursor: None,
            highlight: None,
            map: &map,
            candidates: &candidates,
            pan: baud_font::Px::of(0),
            composition: None,
        },
        &mut r,
        &mut mask,
        &mut records,
    );
    unmapped(&r) - before
}

/// **The overlay counts a dropped mark once, wherever its ligature window
/// is cut.** `Renderer::window` shapes a window longer than its glyph budget
/// in chunks, each with the face's longest rule of context either side, so a
/// cell near a chunk's edge is shaped by two chunks, and only the chunk that
/// owns it may count what it drops. On JetBrains Mono a chunk is 36 cells
/// with 6 of context, so cells 30-41 and 66-77 of a long window are shaped
/// twice. A row of `-` is one window; U+0316, which no test face has, rides
/// on one of them, inside and outside those spans.
#[test]
fn the_overlay_counts_a_dropped_mark_once_across_window_chunks() {
    let (j, _) = jbm_and_fira();
    let jbm = [Face::parse(&j).unwrap()];
    assert!(
        jbm[0].glyph(cp(0x316)).is_none(),
        "premise: JBM lacks U+0316"
    );
    for k in [10, 29, 30, 35, 36, 41, 42, 66, 77, 78] {
        let mut text = "-".repeat(120);
        text.insert(k + 1, '\u{316}');
        assert_eq!(
            overlay_unmapped(&jbm, &text),
            1,
            "U+0316 on the row's cell {k}: counted once"
        );
    }
}
