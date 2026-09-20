//! Step 3 of the sfte plan -- the frame path, in the only form that is
//! satisfiable. **Pixel equality is out for four independent reasons**: sfte
//! blends `>> 8` where baud divides by 255, `stb_truetype` is not
//! `baud-font`, sfte has zero GPOS/COLR/CBDT/sbix, and it draws box drawing
//! and braille procedurally. So the comparison is not the pixels but the cell
//! **geometry**: at a font size matched by `sfte_px = baud_px * (ascender -
//! descender) / upem`, both derivations run at the same scale `px / upem`,
//! and the only thing left to differ is the rounding.
//!
//! **This is not an external oracle**, and it must not be reported as one --
//! that is the plan's own instruction. sfte's formula is *transcribed* here
//! from its pinned source, cited by line, and recomputed from it in Rust; a
//! transcribed rule checked against baud is baud checking itself, the
//! guessed-table shape this repo records. What it *is* is a check that two
//! independently-derived cell formulas agree -- baud's real `Metrics::of` and
//! sfte's, the second reproducible from `docs/research/sources/sfte.h`.
//!
//! sfte's cell, from that pinned header:
//!   scale   = px / (ascent_u - descent_u)            [`ScaleForPixelHeight`, :35563, and stb :937]
//!   ascent  = (int)(ascent_u   * scale + 0.5f)       [:35774]
//!   descent = (int)(descent_u  * scale - 0.5f)       [:35775, descent_u is native-negative]
//!   line_gap= (int)(line_gap_u * scale + 0.5f)       [:35776]
//!   cell_h  = ascent - descent + line_gap            [:35778 -- rounds each vmetric separately]
//!   cell_w  = (int)('M'.advance_u * scale + 0.5f)    [:35714 g->xadvance, :35786 -> int16 cell_width]
//! sfte reads its vmetrics through `stbtt_GetFontVMetrics`, which is `hhea`,
//! the same table `baud-font`'s `line_metrics()` reads. `baud`'s `Metrics::of`
//! rounds the height **sum** once (not each vmetric) and rounds `'M'`'s
//! scaled advance. So with the scale matched, the whole of the difference is
//! sum-rounding against component-rounding.

mod support;
use baud_domain::Codepoint;
use baud_font::Face;
use baud_render::{Metrics, PxSize};
use support::*;

/// One size where the two cell derivations disagree: the pixel size, then
/// baud's `(width, height)` and sfte's `(width, height)`.
type Disagreement = (u16, (i64, i64), (i64, i64));

#[test]
fn baud_and_sftes_cell_derivations_agree_at_matched_scale() {
    let bytes = font("JetBrainsMono-Regular.ttf");
    let face = Face::parse(&bytes).unwrap();

    // The raw `hhea` metrics and `'M'`'s advance, in font units -- the same
    // inputs both cell formulas start from.
    let upem = f32::from(face.units_per_em().value());
    let lm = face.line_metrics();
    let asc_u = f32::from(lm.ascender.value());
    let desc_u = f32::from(lm.descender.value());
    let gap_u = f32::from(lm.line_gap.value());
    let m = face.glyph(Codepoint::from_char('M')).unwrap();
    let adv_u = f32::from(face.advance(m).unwrap().value());

    // 33 integer sizes, 8..=40, spanning what a terminal uses and including
    // baud's own 14.0. The count "33" is the plan's; the *agreement* is not
    // taken on faith -- it is computed here and pinned to what it comes out.
    let sizes: Vec<u16> = (8..=40).collect();
    assert_eq!(sizes.len(), 33, "the size sweep is 33 wide");

    let mut agree = 0usize;
    let mut disagree: Vec<Disagreement> = Vec::new();
    for &px in &sizes {
        let pf = f32::from(px);

        // baud's *actual* cell, through the real code the renderer uses.
        let bm = Metrics::of(&face, PxSize::of(pf).unwrap()).unwrap();
        let baud = (i64::from(bm.cell_w.value()), i64::from(bm.cell_h.value()));

        // sfte's cell, transcribed above, at the matched scale `px/upem`
        // (which is what sfte's `px/(ascent-descent)` collapses to once the
        // size is mapped by the plan's formula). C `(int)` truncates toward
        // zero; `as i64` matches that for these finite, in-range values and is
        // the widening the audit does not count, unlike an `as i32`.
        let s = pf / upem;
        let asc = (asc_u * s + 0.5) as i64;
        let desc = (desc_u * s - 0.5) as i64;
        let gap = (gap_u * s + 0.5) as i64;
        let sfte = ((adv_u * s + 0.5) as i64, asc - desc + gap);

        if baud == sfte {
            agree += 1;
        } else {
            disagree.push((px, baud, sfte));
        }
    }

    // A positive control: the sweep must actually have matched something, or
    // a broken transcription that never agrees would read as "0 disagree".
    assert!(
        agree > 0,
        "the two derivations agree at no size -- the transcription is broken, \
         not the formulas: {disagree:?}"
    );

    // Pinned to the measured result, **not** to the plan's estimate. The
    // plan predicted "31 of 33 agree"; measured, it is **22 of 33**, with 11
    // one-pixel height disagreements. The plan's number was an estimate and
    // this is the measurement -- the divergence is real and larger than the
    // estimate implied. Every disagreement is a size where baud's sum-rounding
    // of the height differs from sfte's rounding of each vmetric, listed with
    // both cells so the rounding case is visible rather than hidden behind a
    // count. If baud's cell rounding ever changes, this moves.
    assert_eq!(
        (agree, disagree.len()),
        (22, 11),
        "cell-geometry agreement changed; disagreements (px, baud (w,h), sfte (w,h)): {disagree:?}"
    );
    // Every disagreement is height-only and one pixel apart -- the
    // component-vs-sum rounding, never the width (baud is +1 at the small
    // sizes and -1 at the large, where the rounding crosses over).
    for &(px, (bw, bh), (sw, sh)) in &disagree {
        assert_eq!(bw, sw, "the widths agree at {px} px: {bw} vs {sw}");
        assert_eq!(
            (bh - sh).abs(),
            1,
            "the heights differ by one pixel at {px} px"
        );
    }
}
