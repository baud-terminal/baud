//! **The pointer crossing a view's edge, heard for a view baud does not
//! subclass**: the activity lights in the title strip are AppKit's image
//! views, and the terminal view's own tracking area ends at its bounds, so
//! nothing told baud the pointer was over a light. One tracking area per
//! watched view, all owned by one [`Crossings`], hears each entry and exit.
//!
//! What AppKit promises, from `NSTrackingArea.h`
//! (`[[source: macos-sdk-nstrackingarea-h]]`):
//! `NSTrackingMouseEnteredAndExited` -- "owner receives mouseEntered when
//! mouse enters area, and mouseExited when mouse leaves area";
//! `NSTrackingActiveAlways` -- "owner receives mouseEntered/Exited or
//! mouseMoved regardless of activation", as the terminal view's own area
//! is; `NSTrackingInVisibleRect` -- "tracking occurs in visibleRect of view
//! and rect is ignored", so a view re-placed on a resize, or hidden, needs
//! no new area; and the owner is `weak`, so [`Crossings`] holds it.
//!
//! **What is heard is a place, not a view**: each crossing is the pointer's
//! point on the screen, queued for the next turn and the responder woken,
//! and the app finds which light, if any, is under it from the lights'
//! frames. The hover logic therefore does not depend on which view hosts
//! the lights.
//!
//! **On the screen, because the lights and the terminal can be in two
//! windows**: in full screen the title bar, and the lights' accessory in
//! it, leave the terminal's window for one of their own --
//! `NSApplicationPresentationAutoHideToolbar`, "Fullscreen window toolbar
//! is detached from window and hides/shows on rollover"
//! (`[[source: sdk27-nsapplication-h]]`, line 168). A view's rect in its
//! window's coordinates is in that window's, so a light's frame
//! ([`on_screen`]) and a crossing's point go to the screen through the
//! window that holds them, and come into the terminal's view through the
//! terminal's ([`in_view`]). In a window the two windows are one and the
//! route gives what the window's coordinates alone gave.
//!
//! `unsafe` is allowed here, as in [`crate::notify`], for the class the
//! tracking areas send their messages to and for the tracking area's
//! constructor, which the bindings mark so.
#![allow(unsafe_code)]

use std::cell::RefCell;

use macos_liquid_glass::drawable::Waker;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::NSObject;
use macos_liquid_glass::objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use macos_liquid_glass::objc2_app_kit::{
    NSEvent, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
};
use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect};

/// What the owner heard, and whom it wakes.
struct Heard {
    points: RefCell<Vec<NSPoint>>,
    waker: Option<Waker>,
}

define_class!(
    /// The tracking areas' owner.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Heard]
    struct Watcher;

    impl Watcher {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, event: &NSEvent) {
            crate::unwind::guard(crate::unwind::SiteName("a light's entry"), (), || self.heard(event));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, event: &NSEvent) {
            crate::unwind::guard(crate::unwind::SiteName("a light's exit"), (), || self.heard(event));
        }
    }
);

impl Watcher {
    fn heard(&self, event: &NSEvent) {
        // The event's point is in its window's coordinates, the window of
        // the view crossed, which in full screen is not the terminal's:
        // to the screen through that window. With no window, as an event
        // built with none has, the point is kept as given.
        let at = event.locationInWindow();
        let at = match event.window(self.mtm()) {
            Some(w) => w.convertPointToScreen(at),
            None => at,
        };
        self.ivars().points.borrow_mut().push(at);
        if let Some(w) = &self.ivars().waker {
            w.wake();
        }
    }
}

/// **The crossings of the views it watches**: one owner for every
/// tracking area, alive as long as this is.
pub struct Crossings {
    owner: Retained<Watcher>,
}

impl Crossings {
    /// An owner that wakes `waker` at each crossing; none, for a test.
    pub fn new(mtm: MainThreadMarker, waker: Option<Waker>) -> Crossings {
        let this = Watcher::alloc(mtm).set_ivars(Heard {
            points: RefCell::new(Vec::new()),
            waker,
        });
        // SAFETY: `NSObject`'s `init`.
        let owner: Retained<Watcher> = unsafe { msg_send![super(this), init] };
        Crossings { owner }
    }

    /// **Hear the pointer cross `view`'s visible rect**, entering and
    /// leaving, whether or not the app is active.
    pub fn watch(&self, view: &NSView) {
        let options = NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect;
        // SAFETY: the owner answers `mouseEntered:` and `mouseExited:`, the
        // two messages these options send, and is kept alive by `self`;
        // the area holds it weakly. The rect is ignored under
        // `InVisibleRect`.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                view.bounds(),
                options,
                Some(&self.owner),
                None,
            )
        };
        view.addTrackingArea(&area);
    }

    /// **The screen points of the crossings heard since the last call**, in
    /// the order they came.
    pub fn take(&self) -> Vec<NSPoint> {
        std::mem::take(&mut *self.owner.ivars().points.borrow_mut())
    }
}

/// **`view`'s bounds on the screen**, through the window that holds it --
/// in full screen the title bar's own, for a view in the title bar; none
/// for a view in no window.
pub fn on_screen(view: &NSView) -> Option<NSRect> {
    let window = view.window()?;
    Some(window.convertRectToScreen(view.convertRect_toView(view.bounds(), None)))
}

/// **A rect on the screen in `view`'s coordinates**, through `window`, the
/// window `view` is in: where a rect [`on_screen`] gave lies in the
/// terminal's view, whichever window it was read from.
pub fn in_view(window: &NSWindow, view: &NSView, screen: NSRect) -> NSRect {
    view.convertRect_fromView(window.convertRectFromScreen(screen), None)
}
