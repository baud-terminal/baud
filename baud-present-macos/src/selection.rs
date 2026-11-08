//! **The selection in a window that is not key**, in AppKit's own colours.
//!
//! AppKit draws a selection in `unemphasizedSelectedTextBackgroundColor`
//! and `unemphasizedSelectedTextColor` "when the associated
//! control/view/window is not key" (`[[source: sdk27-nscolor-h]]`
//! 232-235). Both are dynamic colours, so they are resolved under the
//! window's own appearance and read in sRGB, the surface's space: never
//! sampled from the screen. Under aqua they are `#dcdcdc` with black text
//! and under darkAqua `#464646` with white (note AZ).

use baud_domain::Chan;
use baud_domain::themefile::CellPaint;
use baud_render::{Argb, Hilite};
use block2::RcBlock;
use core::cell::Cell;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{NSAppearance, NSColor, NSColorSpace};

/// AppKit's unemphasized selection pair under `appearance`, page and text,
/// as [`baud_render::Renderer::set_inactive_selection`] takes it.
pub fn unemphasized(appearance: &NSAppearance) -> Hilite {
    let pair = Cell::new((Argb::default(), Argb::default()));
    let resolve = RcBlock::new(|| {
        crate::unwind::guard("the selection colours' resolve", (), || {
            pair.set((
                srgb(&NSColor::unemphasizedSelectedTextBackgroundColor()),
                srgb(&NSColor::unemphasizedSelectedTextColor()),
            ));
        });
    });
    appearance.performAsCurrentDrawingAppearance(&resolve);
    let (page, text) = pair.get();
    Hilite::of(Some(CellPaint::Colour(page)), Some(CellPaint::Colour(text)))
}

/// A colour in sRGB, each channel rounded to eight bits, opaque: a colour
/// with no sRGB form is black.
fn srgb(c: &Retained<NSColor>) -> Argb {
    let Some(s) = c.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
        return Argb::rgb(Chan::from_u8(0), Chan::from_u8(0), Chan::from_u8(0));
    };
    let (r, g, b) = (
        s.redComponent() as f32,
        s.greenComponent() as f32,
        s.blueComponent() as f32,
    );
    Argb::rgb(Chan::from_unit(r), Chan::from_unit(g), Chan::from_unit(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use macos_liquid_glass::objc2_foundation::NSString;

    fn named(name: &str) -> Retained<NSAppearance> {
        NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a system appearance")
    }

    fn pair(page: Argb, text: Argb) -> Hilite {
        Hilite::of(Some(CellPaint::Colour(page)), Some(CellPaint::Colour(text)))
    }

    /// **AppKit's own values, resolved headless**: aqua `#dcdcdc` on black
    /// text, darkAqua `#464646` on white, as note AZ pins them.
    #[test]
    fn the_unemphasized_pair_is_appkits_under_each_appearance() {
        assert_eq!(
            unemphasized(&named("NSAppearanceNameAqua")),
            pair(Argb(0xFF_DC_DC_DC), Argb(0xFF_00_00_00))
        );
        assert_eq!(
            unemphasized(&named("NSAppearanceNameDarkAqua")),
            pair(Argb(0xFF_46_46_46), Argb(0xFF_FF_FF_FF))
        );
    }
}
