//! The cell: how wide, how tall, and where the baseline is, from the
//! primary face at the chosen size.

use baud_domain::Codepoint;
use baud_font::{Face, Phase, Px, Scale};

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
    /// Where an underline's top edge goes, in pixels below the cell's top
    /// edge, from `post`; a pixel under the baseline when the face says
    /// nothing.
    pub underline: Px,
    /// Where a strike-through's top edge goes, in pixels below the cell's
    /// top edge: a third of the ascent above the baseline.
    pub strike: Px,
    /// How thick an underline or strike is, at least a pixel.
    pub thickness: Px,
    /// Where every glyph sits inside its cell: half the slack between the
    /// rounded cell and `M`'s advance.
    pub phase: Phase,
}

impl Metrics {
    /// The cell the primary face asks for at `size`; `None` when the face
    /// maps no `M` or its metrics are degenerate.
    pub fn of(face: &Face<'_>, size: PxSize) -> Option<Metrics> {
        let scale = Scale::of(size.value(), face.units_per_em());
        let m = face.glyph(Codepoint::from_char('M'))?;
        // The default instance's: a cell must not change with a style's
        // weight.
        let advance = face.default_advance(m)?;
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
        let baseline = round(ascent)?;
        let cell_h = round(height)?;
        let (under, thick) = match face.underline() {
            Some(u) => (
                scale.px(f32::from(u.position.value())),
                scale.px(f32::from(u.thickness.value())),
            ),
            None => (-1.0, 1.0),
        };
        let thickness = round(thick).unwrap_or(1).max(1);
        // `position` is the top of the line relative to the baseline, up
        // positive; clamp it inside the cell.
        let underline =
            u32::try_from((i64::from(baseline) - under.round() as i64).max(0)).unwrap_or(0);
        let underline = underline.min(cell_h.saturating_sub(thickness));
        let strike = baseline.saturating_sub(round(ascent / 3.0).unwrap_or(1));
        let cell_w = Px::of(round(width)?);
        let phase = Phase::centring(scale, advance, cell_w)?;
        Some(Metrics {
            cell_w,
            cell_h: Px::of(cell_h),
            baseline: Px::of(baseline),
            underline: Px::of(underline),
            strike: Px::of(strike),
            thickness: Px::of(thickness),
            phase,
        })
    }
}

impl Metrics {
    /// **Where a struck link's underline goes** (`Renderer::place_links`):
    /// the face's underline, moved down to a pixel under the strike where
    /// the two would touch, and kept inside the cell. JetBrains Mono's leave
    /// a gap at every size, so it moves nothing there.
    pub fn struck_underline(&self) -> Px {
        clear_of_strike(self.underline, self.strike, self.thickness, self.cell_h)
    }
}

/// `underline`, or a pixel under the strike's bottom (`strike` +
/// `thickness`) where it would be closer than that, clamped so a line
/// `thickness` thick stays inside a cell `cell_h` tall.
fn clear_of_strike(underline: Px, strike: Px, thickness: Px, cell_h: Px) -> Px {
    let clear = strike
        .value()
        .saturating_add(thickness.value())
        .saturating_add(1);
    Px::of(
        underline
            .value()
            .max(clear)
            .min(cell_h.value().saturating_sub(thickness.value())),
    )
}

#[cfg(test)]
mod struck_tests {
    use super::{Px, clear_of_strike};

    /// **A struck link's underline never touches its strike**: metrics
    /// whose underline sits on the strike, or above it, put it a pixel
    /// under the strike's bottom; ones with a gap keep the face's place;
    /// and the line stays inside the cell when the cell is too short for
    /// the gap. Perturbation: the face's underline as it is -- the first
    /// two fail.
    #[test]
    fn a_struck_underline_clears_the_strike() {
        let px = Px::of;
        // Strike at 8, 2 thick: its bottom is 10, so the underline goes to 11.
        assert_eq!(
            clear_of_strike(px(9), px(8), px(2), px(20)),
            px(11),
            "touching"
        );
        assert_eq!(
            clear_of_strike(px(5), px(8), px(2), px(20)),
            px(11),
            "above it"
        );
        assert_eq!(
            clear_of_strike(px(14), px(8), px(2), px(20)),
            px(14),
            "a gap kept"
        );
        assert_eq!(
            clear_of_strike(px(9), px(8), px(2), px(12)),
            px(10),
            "in the cell"
        );
    }
}
