//! **The title strip and the grid's top, as AppKit lays them out windowed
//! and in full screen.**
//!
//! **What is in the strip lives in the title bar.** baud's lockup
//! (`mark.rs`) is a leading titlebar accessory and the activity lights
//! (`lights.rs`) a trailing one, each an `NSTitlebarAccessoryViewController`
//! through the window crate's `GlassWindow::add_titlebar_accessory`. A
//! leading accessory goes "adjacent and to the right of the
//! close/minimize/maximize buttons", a trailing one at the other end, and
//! AppKit flips the two for a right-to-left language
//! ([[source: macos-sdk-nstitlebaraccessoryviewcontroller-h]] 23, 27).
//! [`edges`] reads the strip left to right -- the buttons at the left, the
//! lights' end at the right -- which holds because the bundle has no
//! localization AppKit could lay out right to left:
//! `packaging/macos/Info.plist` lists no `CFBundleLocalizations` and the
//! bundle holds no `.lproj` directory. They sit in the same titlebar view
//! as the traffic lights, which in full screen AppKit takes out of the
//! window: the "fullscreen window toolbar is detached from window and
//! hides/shows on rollover", the doc of
//! `NSApplicationPresentationAutoHideToolbar`
//! ([[source: sdk27-nsapplication-h]] 168). A subview of baud's own
//! content stayed in the window under a title bar that had left it: the
//! lockup indented for buttons that were gone, and a second, empty bar
//! slid down over it on reveal. The window test checks that both are in
//! the traffic lights' view and not the content's; that AppKit shows them
//! with the traffic lights in full screen, and whether it hides that bar
//! at all, which follows the menu bar's setting, is AppKit's and is not
//! measured, since a window enters full screen only when it is shown.
//!
//! **AppKit places them, baud sizes them.** A leading accessory starts one
//! button gap past the zoom button and a trailing one is flush with the
//! window's edge; the width is the view's and AppKit sets the height to
//! the title bar's. Nothing in AppKit stops the two from running into
//! each other in a narrow window, so the lights' accessory is sized at
//! every layout to what the lights need and no wider than the room right
//! of the lockup ([`lights_width`]), and the lights that do not fit are
//! not shown, the leftmost first (`lights::placed`). Every position is
//! read from the window as it is laid out, never a constant: the title
//! bar's height, the buttons' places and the lockup's end differ with the
//! SDK an app links against.
//!
//! **The grid's top follows full screen.** Windowed, the terminal's view
//! stops the title bar's height short of the window's top, which the
//! window reports as its content less its content layout rect. In full
//! screen there is no title bar in the window, so the view takes the
//! window's whole height ([`grid_top`]). The change is heard from the
//! window's "did enter" and "did exit" notifications
//! (`baud_present_macos::full_screen`), never from a change of the
//! content layout rect: if the rect moved when the bar is revealed, the
//! grid would reflow, and the shell be told a new size, each time the
//! pointer touched the top. The revealed bar lies over the grid's top
//! rows, as it does over any full-screen window's content. **With the menu
//! bar set never to hide in full screen, the bar does not leave at rest**:
//! it stays over the grid, so a grid top of 0 puts row 0 under it. Which
//! case a Mac is in, and where the terminal's view and the content layout
//! rect are then, is read live by the demo's filming run
//! (`full_screen_check.rs`), and until a reading settles it the top here is
//! the auto-hiding case's.
//!
//! An entry costs two layouts of the grid, one at the transition's own
//! resize with the old top and one at the notification with the new; an
//! exit the same. Taking the state at the "will" notifications would save
//! one, but a failed transition is told only to a window delegate, which
//! the window crate does not let its consumer set, and the grid would stay
//! at the wrong top.
//!
//! No `crate::` path: `baud-app/tests/titlebar_mark.rs` takes this file in
//! by `#[path]`, as it takes `mark.rs`, and checks it in windows it builds
//! and never shows, the grid's top in full screen by the two
//! notifications posted by hand.
#![forbid(unsafe_code)]

use baud_present_macos::full_screen::Change;
use macos_liquid_glass::objc2::MainThreadOnly;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{
    NSTitlebarAccessoryViewController, NSView, NSWindow, NSWindowButton,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use macos_liquid_glass::window::{GlassWindow, TitlebarSide};

/// **The terminal view's top margin**, in points, in a window whose
/// content is `content_h` tall with a content layout rect `layout_h` tall:
/// none once in full screen, whatever the rect says, and the title bar's
/// height once a window again, read from the window as it is then.
pub(crate) fn grid_top(change: Change, content_h: CGFloat, layout_h: CGFloat) -> CGFloat {
    match change {
        Change::Entered => 0.0,
        Change::Exited => (content_h - layout_h).max(0.0),
    }
}

/// **The title bar's height in `window` now**: its content less its
/// content layout rect, the part of the content the title bar does not
/// cover.
pub(crate) fn inset_of(window: &NSWindow) -> CGFloat {
    let content_h = window
        .contentView()
        .map_or(window.frame().size.height, |v| v.frame().size.height);
    (content_h - window.contentLayoutRect().size.height).max(0.0)
}

/// **The terminal's `view` in `container`, `top` points short of its
/// top**, the full width: set only when it moves, since a new frame lays
/// the grid out again.
pub(crate) fn apply(view: &NSView, container: &NSView, top: CGFloat) {
    let full = container.frame().size;
    let want = NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(full.width, (full.height - top).max(0.0)),
    );
    if view.frame() != want {
        view.setFrame(want);
    }
}

/// **A change of `window`'s full screen, followed**: the terminal's
/// `view` in `container` to the top the change gives it. The driver's
/// observer calls this, and the window test does too.
pub(crate) fn follow(change: Change, window: &NSWindow, view: &NSView, container: &NSView) {
    let content_h = window
        .contentView()
        .map_or(window.frame().size.height, |v| v.frame().size.height);
    let top = grid_top(change, content_h, window.contentLayoutRect().size.height);
    apply(view, container, top);
}

/// **A view in `window`'s title bar at `side`**, `size` to begin with --
/// AppKit keeps the width and sets the height -- and the controller that
/// keeps it there, which the window also holds.
pub(crate) struct Accessory {
    pub(crate) view: Retained<NSView>,
    _controller: Retained<NSTitlebarAccessoryViewController>,
}

impl Accessory {
    pub(crate) fn put(
        mtm: MainThreadMarker,
        window: &GlassWindow,
        side: TitlebarSide,
        size: NSSize,
    ) -> Accessory {
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), size),
        );
        let controller = window.add_titlebar_accessory(&view, side);
        Accessory {
            view,
            _controller: controller,
        }
    }

    /// **`width` points wide**, at the height AppKit gave it.
    pub(crate) fn set_width(&self, width: CGFloat) {
        let now = self.view.frame().size;
        if now.width != width {
            self.view.setFrameSize(NSSize::new(width, now.height));
        }
    }

    /// Its frame in the window's coordinates.
    pub(crate) fn in_window(&self) -> NSRect {
        self.view.convertRect_toView(self.view.bounds(), None)
    }

    /// **The lights' accessory fitted to lights `spans_width` across**
    /// between `edges` ([`lights_width`]), and the room they are placed in
    /// ([`lights_room`]): at every layout, so a narrow window drops the
    /// leftmost lights rather than overlap the lockup and the buttons.
    pub(crate) fn fit_lights(
        &self,
        edges: Edges,
        spans_width: CGFloat,
        pad: CGFloat,
        scale: CGFloat,
    ) -> (NSSize, CGFloat) {
        self.set_width(lights_width(edges, spans_width, pad, scale));
        lights_room(edges, self.view.frame().size)
    }
}

/// **Where the strip's two ends are in a window**, in its points from the
/// left: its width, the least x a light may start at -- a light's gap
/// past the lockup -- and the gap kept at the right, the close button's
/// at the left.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Edges {
    pub(crate) width: CGFloat,
    pub(crate) left: CGFloat,
    pub(crate) right: CGFloat,
}

/// Where the strip's left part ends when nothing tells: past where AppKit
/// puts the zoom button.
const PAST_THE_BUTTONS: CGFloat = 74.0;

/// The gap kept at the strip's right while the close button is not laid
/// out.
const RIGHT_UNLAID: CGFloat = 12.0;

/// **The strip's edges in `window` now**: the lights start a light's gap
/// past `lockup_end`, the lockup's right edge in the window, or past the
/// zoom button with no lockup; they end the close button's distance from
/// the left short of the right.
pub(crate) fn edges(window: &NSWindow, lockup_end: Option<CGFloat>) -> Edges {
    let laid = |b: NSWindowButton| {
        window
            .standardWindowButton(b)
            .map(|v| v.frame())
            .filter(|f| f.size.height >= 1.0)
    };
    let right = laid(NSWindowButton::CloseButton).map_or(RIGHT_UNLAID, |f| f.origin.x);
    let end = lockup_end
        .or_else(|| laid(NSWindowButton::ZoomButton).map(|f| f.origin.x + f.size.width))
        .unwrap_or(PAST_THE_BUTTONS);
    Edges {
        width: window.frame().size.width,
        left: end + CGFloat::from(baud_render::lights::PAIR),
        right,
    }
}

/// **The lights' accessory's width**, in points: from the plate's `pad`
/// before the lights' span `spans_width` to the window's edge, the span
/// ending `edges.right` short of it -- but starting no further left than
/// the pad short of `edges.left`, so it never covers the lockup or the
/// buttons. Its left edge is on a whole device pixel at `scale` in the
/// window, so the lights placed in it land where the window's own strip
/// put them, and at or before the pad, so the plate is whole. None with
/// no lights.
pub(crate) fn lights_width(
    edges: Edges,
    spans_width: CGFloat,
    pad: CGFloat,
    scale: CGFloat,
) -> CGFloat {
    if spans_width <= 0.0 {
        return 0.0;
    }
    let scale = scale.max(1.0);
    let whole = |x: CGFloat| (x * scale).floor() / scale;
    let wanted = whole(edges.width - edges.right - spans_width - pad);
    let least = whole(edges.left - pad);
    (edges.width - wanted.max(least)).max(0.0)
}

/// **The room the lights are placed in, in the accessory's own points**:
/// its size, and the least x a light may start at there -- `edges.left`
/// less where the accessory starts, flush with the window's right edge.
pub(crate) fn lights_room(edges: Edges, accessory: NSSize) -> (NSSize, CGFloat) {
    (accessory, edges.left - (edges.width - accessory.width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(width: CGFloat) -> Edges {
        Edges {
            width,
            left: 129.366,
            right: 9.0,
        }
    }

    /// **In full screen the grid starts at the window's top, whatever the
    /// content layout rect says, and windowed it starts the title bar's
    /// height down**: 0 entered with a rect the content's height or 28
    /// points short, 28 exited from a 600-point content and a 572-point
    /// rect. Perturbation: follow the rect when entered -- the strip the
    /// screenshots showed -- and the second check fails; keep the launch's
    /// inset for both and the first does.
    #[test]
    fn full_screen_puts_the_grid_at_the_top_and_a_window_under_its_title_bar() {
        assert_eq!(grid_top(Change::Entered, 1085.0, 1085.0), 0.0);
        assert_eq!(grid_top(Change::Entered, 1085.0, 1057.0), 0.0);
        assert_eq!(grid_top(Change::Exited, 600.0, 572.0), 28.0);
        assert_eq!(grid_top(Change::Exited, 600.0, 568.0), 32.0);
        assert_eq!(grid_top(Change::Exited, 10.0, 20.0), 0.0);
    }

    /// **The lights' accessory is what they need, and in a narrow window
    /// no wider than the room past the lockup**: at 960 points, the span,
    /// the right gap and the pad, its left a whole device pixel; at 200,
    /// it starts the pad short of where the lights may start, which is
    /// past the lockup, and the least x in its own points is the pad.
    /// None with no lights. Perturbation: drop the `min` and the 200-point
    /// accessory covers the lockup, the second check fails.
    #[test]
    fn the_lights_accessory_is_sized_to_the_lights_and_clear_of_the_lockup() {
        let wide = lights_width(edges(960.0), 180.25, 4.0, 2.0);
        assert_eq!(wide, 193.5);
        let narrow = lights_width(edges(200.0), 180.25, 4.0, 2.0);
        assert_eq!(narrow, 75.0);
        assert!(200.0 - narrow <= 129.366 - 4.0, "{narrow}");
        assert!(200.0 - narrow > 129.366 - 4.0 - 0.5, "{narrow}");
        assert!(
            200.0 - narrow > 129.366 - 9.0,
            "past the lockup's end: {narrow}"
        );
        let (_, left) = lights_room(edges(200.0), NSSize::new(narrow, 28.0));
        assert!((left - (129.366 - (200.0 - narrow))).abs() < 1e-9);
        assert!((4.0..4.5).contains(&left), "{left}");
        assert_eq!(lights_width(edges(960.0), 0.0, 4.0, 2.0), 0.0);
        assert_eq!(lights_width(edges(100.0), 180.25, 4.0, 2.0), 0.0);
    }
}
