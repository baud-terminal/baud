//! **The trail `frame_gpu`'s trail arm draws**: one cursor jump across the
//! screen, the same on every frame and every round, each effect at the same
//! fraction of its own life -- so its line prices one effect's frame, and a
//! later run's line for the same name prices the same frame.
//!
//! The look is built as the app's trail driver builds an effect's: the quad
//! on the segment from the cell the cursor left to the one it reached, the
//! ignition lit at the centre of the first on the first seed, riding the
//! quad at full strength.

use baud_core::CursorStyle;
use baud_domain::{Col, Cols, Row, Rows};
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{Pt, Renderer, Segment, Shape, Smear, Strength, TrailLook};

/// The fraction of an effect's life ([`Effect::end`]) the arm draws it at:
/// every effect is still burning there, the shortest-lived included.
pub const AT: f32 = 0.25;

/// **The jump**: from the third row and the third column to the third row
/// from the bottom and the tenth column from the right, the screen's long
/// diagonal; a screen too small for either margin takes its first cell.
pub fn jump(cols: Cols, rows: Rows) -> [(Row, Col); 2] {
    [
        (Row::from_index(2), Col::at_index(2)),
        (
            Row::from_index(rows.index().saturating_sub(3)),
            Col::at_index(cols.index().saturating_sub(10)),
        ),
    ]
}

/// **`effect` over the jump**, at [`AT`] of its life, in `r`'s pixels.
pub fn look(r: &Renderer<'_, '_>, effect: Effect, [from, to]: [(Row, Col); 2]) -> TrailLook {
    let style = CursorStyle::SteadyBlock;
    let (a, b) = (
        r.cursor_rect(from.0, from.1, style),
        r.cursor_rect(to.0, to.1, style),
    );
    let unit = u16::try_from(r.metrics().cell_h.value()).map_or(f32::MAX, f32::from);
    let tail = Pt {
        x: (a.left + a.right) * 0.5,
        y: (a.top + a.bottom) * 0.5,
    };
    let ignition = Ignition::ignite(tail, b, Seed(1), unit);
    let age = AT * effect.end(&ignition);
    TrailLook::Vfx(Vfx {
        effect,
        emitter: Smear {
            corners: Segment::new(a.corners(), b).at(age),
            hole: b,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: ignition.at(age),
        axis: ignition.dir,
    })
}

/// The name an effect's line carries: the config file's name for it, so
/// two runs' lines pair by it.
pub fn name(effect: Option<Effect>) -> &'static str {
    match effect {
        None => "off",
        Some(Effect::Fire) => "fire",
        Some(Effect::Plasma) => "plasma",
        Some(Effect::Lightning) => "lightning",
        Some(Effect::Ice) => "ice",
        Some(Effect::Ufo) => "ufo",
        Some(Effect::Orb) => "orb",
        Some(Effect::Liquid) => "liquid",
    }
}
