//! `Face::refuses_past`: whether a glyph cannot be drawn with a given
//! budget of scratch.
//!
//! A renderer caps the scratch it grows for one outline, and a glyph past
//! the cap cannot be drawn; it then shows another face's glyph or a
//! `.notdef`, and must know which glyphs those are before it asks for
//! them. `refuses_past` answers that from one walk of a composite and none
//! of a simple glyph, and is held here to the definition it shortcuts --
//! `refuses(g) || outline_need(g) > budget` -- over every glyph of every
//! committed variable fixture and the variable faces in `tests/fonts`, at
//! the default instance and at each axis's extremes, against budgets at
//! the edges a shortcut could get wrong: the phantoms alone, a simple
//! glyph's most, HarfBuzz's 200,000, and one either side of each glyph's
//! own need.

use baud_domain::GlyphId;
use baud_font::bytes::{Count, Index};
use baud_font::var::Variation;
use baud_font::{Face, VarPoint};

const FACES: [&str; 11] = [
    "var/budget-cases.ttf",
    "var/ink-cases.ttf",
    "var/component-cases.ttf",
    "var/vertical-cases.ttf",
    "var/gvar-cases.ttf",
    "var/colour-refused-cases.ttf",
    "var/bearing-cases.ttf",
    "var/bearing-cases-bit1.ttf",
    "fonts/JetBrainsMono-Variable.ttf",
    "fonts/JetBrainsMono-Italic-Variable.ttf",
    "fonts/FiraCode-VF.ttf",
];

/// Each axis at its minimum and at its maximum, the rest at the default:
/// the instances a fixture's samples sit at.
fn instances(face: &Face<'_>) -> Vec<Option<Variation>> {
    let mut out = vec![None];
    let mut i = Index::ZERO;
    while let Some(axis) = face.axis(i) {
        for value in [axis.min, axis.max] {
            out.push(Some(Variation {
                tag: axis.tag,
                value,
            }));
        }
        i = i.next();
    }
    out
}

#[test]
fn refuses_past_is_refuses_or_a_need_past_the_budget() {
    let (mut glyphs, mut past, mut refused, mut walked, mut full) = (0u32, 0u32, 0u32, 0u32, 0u32);
    for name in FACES {
        let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
        let data = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let plain = Face::parse(&data).unwrap();
        for at in instances(&plain) {
            let mut face = Face::parse(&data).unwrap();
            if let Some(v) = at {
                face.set_variations(&[v]);
            }
            for g in 0..face.glyph_count().value() {
                let g = GlyphId::from_shaper(g);
                let need = face.outline_need(g).value();
                let mut budgets = vec![0, 3, 4, 5, 65_539, 65_540, 65_541, 200_000, u32::MAX];
                budgets.extend([need.saturating_sub(1), need, need.saturating_add(1)]);
                for b in budgets {
                    let want = face.refuses(g) || need > b;
                    assert_eq!(
                        face.refuses_past(g, Count::of(b)),
                        want,
                        "{name} {at:?} glyph {} budget {b}: need {need}",
                        g.index()
                    );
                }
                glyphs += 1;
                past += u32::from(need > 200_000);
                refused += u32::from(face.refuses(g));
                walked += u32::from(need > 0);
                full += u32::from(need == 65_540);
            }
        }
    }
    // The positive controls: the comparison saw glyphs past HarfBuzz's
    // bound, refused glyphs, glyphs drawn through scratch at all, and the
    // budget cases' `full` at each extreme, a simple glyph at the most a
    // simple glyph can need -- where a shortcut that skips the walk of a
    // simple glyph stops being exact.
    assert!(past >= 4, "{past} glyph instances past 200,000");
    assert_eq!(full, 2, "full at wght 100 and 900");
    assert!(
        refused > 0 && walked > glyphs / 2,
        "{refused} {walked} {glyphs}"
    );
}

/// The scratch's unit, which `VAR_POINTS_MAX`'s doc prices at 28 bytes.
#[test]
fn a_scratch_point_is_28_bytes() {
    assert_eq!(core::mem::size_of::<VarPoint>(), 28);
}
