//! **baud's mark in the titlebar strip**, a light's gap right of the
//! traffic lights and centred in the titlebar's strip, which the terminal's
//! view leaves clear at the top of the container.
//!
//! The window is `FullSizeContentView`, so the container the terminal's
//! view sits in spans the whole frame, the titlebar's strip included, and
//! the strip is the container's top `inset` points. The mark is placed
//! against the container's height **as it is when placed**, never the
//! size the container was built at. A launch with `BAUD_GRID` sizes the
//! window to the grid before the container exists, and the window then
//! gives the container its own size: the README film's window was 786x572
//! points, the container was built at 960x600, and the mark, centred in
//! a 600-point container's strip, sat at 574.8-597.2, above the window's
//! top edge and out of sight for the whole take. Nothing re-placed it,
//! because a pinned grid's window is never resized.
#![forbid(unsafe_code)]

use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{NSImage, NSImageView, NSView, NSWindow, NSWindowButton};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

/// Where the mark starts, in points from the left, when the traffic
/// lights are not laid out: past where AppKit puts the zoom button.
const FALLBACK_X: CGFloat = 74.0;

/// **The mark's place in the strip**: its left edge and the side of the
/// square it is drawn in, which a resize does not change.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Place {
    pub(crate) x: CGFloat,
    pub(crate) size: CGFloat,
}

impl Place {
    /// **Beside the traffic lights**, from their frames once the window
    /// has laid them out: the lights' own gap past the zoom button, and a
    /// little larger than a button, the snug mark reading at about a
    /// light's size. `None` while they are not laid out, which a zero
    /// height says. The x is shared with the container: the titlebar
    /// view spans the window's width from its left edge.
    pub(crate) fn beside(
        close: NSRect,
        mini: NSRect,
        zoom: NSRect,
        inset: CGFloat,
    ) -> Option<Place> {
        if zoom.size.height < 1.0 {
            return None;
        }
        let gap = (mini.origin.x - (close.origin.x + close.size.width)).max(0.0);
        let size = (zoom.size.height * 1.4).min(inset - 4.0).max(16.0);
        Some(Place {
            x: zoom.origin.x + zoom.size.width + gap,
            size,
        })
    }

    /// Where the mark goes when the lights are not laid out.
    pub(crate) fn fallback(inset: CGFloat) -> Place {
        Place {
            x: FALLBACK_X,
            size: (inset - 2.0).clamp(18.0, 28.0),
        }
    }

    /// **The mark's frame in a container `height` points tall**, bottom-left
    /// origin: centred in the container's top `inset` points, the strip.
    pub(crate) fn frame(self, height: CGFloat, inset: CGFloat) -> NSRect {
        let centre = height - inset / 2.0;
        NSRect::new(
            NSPoint::new(self.x, centre - self.size / 2.0),
            NSSize::new(self.size, self.size),
        )
    }

    /// The mark's right edge, which the activity lights stop short of.
    pub(crate) fn end(self) -> CGFloat {
        self.x + self.size
    }
}

/// **The mark on screen**: its view, in the container, and its place.
pub(crate) struct Mark {
    view: Retained<NSImageView>,
    place: Place,
    inset: CGFloat,
}

impl Mark {
    /// **The mark put in `container`'s strip** of `inset` points, beside
    /// `window`'s traffic lights, and placed at once against the
    /// container's height now. Added after the terminal's view, so it is
    /// drawn above it.
    pub(crate) fn put(
        mtm: MainThreadMarker,
        window: &NSWindow,
        container: &NSView,
        inset: CGFloat,
        image: &NSImage,
    ) -> Mark {
        let lights = (
            window.standardWindowButton(NSWindowButton::CloseButton),
            window.standardWindowButton(NSWindowButton::MiniaturizeButton),
            window.standardWindowButton(NSWindowButton::ZoomButton),
        );
        let place = match lights {
            (Some(c), Some(m), Some(z)) => Place::beside(c.frame(), m.frame(), z.frame(), inset),
            _ => None,
        }
        .unwrap_or_else(|| Place::fallback(inset));
        let view = macos_liquid_glass::icon_view(mtm, image);
        // No autoresizing mask: a drag to a tiny window clamped the frame
        // and it never recovered, so the mark is placed on every resize.
        container.addSubview(&view);
        let mark = Mark { view, place, inset };
        mark.place(container);
        mark
    }

    /// **Re-placed against `container`'s height now**: at the launch, once
    /// the body has the container, and on every resize.
    pub(crate) fn place(&self, container: &NSView) {
        self.view
            .setFrame(self.place.frame(container.frame().size.height, self.inset));
    }

    /// The mark's right edge.
    pub(crate) fn end(&self) -> CGFloat {
        self.place.end()
    }

    /// The mark's view, which `baud-app/tests/titlebar_mark.rs` looks at.
    #[allow(dead_code)]
    pub(crate) fn view(&self) -> &NSImageView {
        &self.view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: CGFloat, y: CGFloat, w: CGFloat, h: CGFloat) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// AppKit's traffic lights on macOS 27 in a titled window, each in the
    /// titlebar view's own coordinates: 14x16 at x 9, 29 and 49.
    fn lights() -> (NSRect, NSRect, NSRect) {
        (
            rect(9.0, 6.0, 14.0, 16.0),
            rect(29.0, 6.0, 14.0, 16.0),
            rect(49.0, 6.0, 14.0, 16.0),
        )
    }

    /// **The mark is in the strip of the container it is in, whatever size
    /// the container was built at**: the README film's window, 786x572
    /// points with a 28-point titlebar, puts it at 69,546.8, 22.4 points
    /// square, inside 544-572 and past the zoom button's right edge at 63.
    /// The control is the size the container was built at, 960x600: the
    /// same place there is wholly above the 572-point container, which is
    /// where the film's mark was. Perturbation: centre on 600 rather than
    /// the height given and the first check fails.
    #[test]
    fn the_mark_is_in_the_strip_of_the_container_it_is_in() {
        let (c, m, z) = lights();
        let p = Place::beside(c, m, z, 28.0).unwrap();
        assert_eq!(p.x, 69.0);
        assert!((p.size - 22.4).abs() < 1e-9, "{}", p.size);
        let f = p.frame(572.0, 28.0);
        assert!(f.origin.y >= 572.0 - 28.0, "{f:?}");
        assert!(f.origin.y + f.size.height <= 572.0, "{f:?}");
        assert!(f.origin.x >= z.origin.x + z.size.width, "{f:?}");
        let built = p.frame(600.0, 28.0);
        assert!(built.origin.y > 572.0, "the control: {built:?}");
        assert!((p.end() - 91.4).abs() < 1e-9, "{}", p.end());
    }

    /// **Lights not laid out fall back to the strip's own size**, and a
    /// strip too short for a light-sized mark keeps the 16-point floor
    /// rather than panicking: `clamp` panics when its floor passes its
    /// ceiling, which a 19-point strip would make it.
    #[test]
    fn unlaid_lights_and_a_short_strip_still_place_the_mark() {
        let (c, m, _) = lights();
        assert_eq!(Place::beside(c, m, rect(49.0, 6.0, 14.0, 0.0), 28.0), None);
        assert_eq!(
            Place::fallback(28.0),
            Place {
                x: 74.0,
                size: 26.0
            }
        );
        let (c, m, z) = lights();
        assert_eq!(Place::beside(c, m, z, 19.0).map(|p| p.size), Some(16.0));
    }
}
