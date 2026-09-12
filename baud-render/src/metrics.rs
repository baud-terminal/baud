//! The cell: how wide, how tall, and where the baseline is, from the
//! primary face at the chosen size.

use baud_domain::Codepoint;
use baud_font::{Face, Px, Scale};

/// A font size in pixels per em.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PxSize(f32);

impl PxSize {
    pub fn of(px: f32) -> Option<PxSize> {
        (px.is_finite() && px > 0.0 && px <= 512.0).then_some(PxSize(px))
    }

    pub fn value(self) -> f32 {
        self.0
    }
}

/// The grid's cell geometry in whole pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    /// A cell's width: the primary face's advance for `M`, rounded.
    pub cell_w: Px,
    /// A cell's height: ascender to descender plus the line gap, rounded.
    pub cell_h: Px,
    /// The baseline, in pixels below the cell's top edge.
    pub baseline: Px,
}

impl Metrics {
    /// The cell the primary face asks for at `size`; `None` when the face
    /// maps no `M` or its metrics are degenerate.
    pub fn of(face: &Face<'_>, size: PxSize) -> Option<Metrics> {
        let scale = Scale::of(size.value(), face.units_per_em());
        let m = face.glyph(Codepoint::from_char('M'))?;
        let advance = face.advance(m)?;
        let width = scale.px(f32::from(advance.value()));
        let lm = face.line_metrics();
        let ascent = scale.px(f32::from(lm.ascender.value()));
        let descent = scale.px(f32::from(lm.descender.value()));
        let gap = scale.px(f32::from(lm.line_gap.value()));
        let height = ascent - descent + gap;
        let round = |v: f32| -> Option<u32> {
            let r = v.round();
            (r.is_finite() && (1.0..=4096.0).contains(&r))
                .then(|| u32::try_from(r as i64).unwrap_or(4096))
        };
        Some(Metrics {
            cell_w: Px::of(round(width)?),
            cell_h: Px::of(round(height)?),
            baseline: Px::of(round(ascent)?),
        })
    }
}
