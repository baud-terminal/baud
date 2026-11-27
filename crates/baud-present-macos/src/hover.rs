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

/// **Whether the window holding the watched views slid** since the last
/// [`Moves::slid`]: in full screen the title bar is a window of its own,
/// which slides down over the content on reveal and up again, under a
/// pointer that need not move (`[[source: sdk27-nsapplication-h]]` 168).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slid {
    No,
    Yes,
}

/// What the move observer heard, and whom it wakes.
struct MovesHeard {
    slid: std::cell::Cell<Slid>,
    waker: Option<Waker>,
}

/// **The moves of the window that holds `view`, while it is not
/// `terminal`**: `NSWindowDidMoveNotification` (`[[source:
/// sdk27-nswindow-h]]` 946) for any window, kept only when its object is
/// the window `view` is in at that moment and that window is not the
/// terminal's -- so the title bar's own window in full screen, which AppKit
/// makes and baud cannot name ahead, and never the terminal's window, whose
/// move moves the lights and the view together. Removed on drop.
pub struct Moves {
    heard: std::rc::Rc<MovesHeard>,
    token: Retained<
        macos_liquid_glass::objc2::runtime::ProtocolObject<
            dyn macos_liquid_glass::objc2::runtime::NSObjectProtocol,
        >,
    >,
}

impl Moves {
    pub fn follow(view: &NSView, terminal: &NSWindow, waker: Option<Waker>) -> Moves {
        use macos_liquid_glass::objc2::rc::Weak;
        let heard = std::rc::Rc::new(MovesHeard {
            slid: std::cell::Cell::new(Slid::No),
            waker,
        });
        let (v, t, h) = (Weak::new(view), Weak::new(terminal), heard.clone());
        let block = block2::RcBlock::new(
            move |n: std::ptr::NonNull<macos_liquid_glass::objc2_foundation::NSNotification>| {
                // SAFETY: the centre hands the block the notification it
                // posts, alive for the call.
                let n = unsafe { n.as_ref() };
                crate::unwind::guard(
                    crate::unwind::SiteName("the lights' window's move"),
                    (),
                    || {
                        let (Some(view), Some(object)) = (v.load(), n.object()) else {
                            return;
                        };
                        let Some(holder) = view.window() else {
                            return;
                        };
                        let terminal = t.load();
                        let as_obj =
                            |w: &NSWindow| -> *const macos_liquid_glass::objc2::runtime::AnyObject {
                                let o: &macos_liquid_glass::objc2::runtime::AnyObject = w;
                                o
                            };
                        let move_is =
                            judged(&object, as_obj(&holder), terminal.as_deref().map(as_obj));
                        on_move(&h.slid, move_is, || {
                            if let Some(w) = &h.waker {
                                w.wake();
                            }
                        });
                    },
                );
            },
        );
        let center = macos_liquid_glass::objc2_foundation::NSNotificationCenter::defaultCenter();
        // SAFETY: the name is a framework constant; no object filter and no
        // queue, so the block runs on the posting thread, the main thread,
        // where windows move.
        let token = unsafe {
            center.addObserverForName_object_queue_usingBlock(
                Some(macos_liquid_glass::objc2_app_kit::NSWindowDidMoveNotification),
                None,
                None,
                &block,
            )
        };
        Moves { heard, token }
    }

    /// Whether it slid since the last call.
    pub fn slid(&self) -> Slid {
        self.heard.slid.replace(Slid::No)
    }
}

impl Drop for Moves {
    fn drop(&mut self) {
        let center = macos_liquid_glass::objc2_foundation::NSNotificationCenter::defaultCenter();
        let observer: &macos_liquid_glass::objc2::runtime::AnyObject = self.token.as_ref();
        // SAFETY: the token is the one `addObserverForName:` returned.
        unsafe { center.removeObserver(observer) };
    }
}

/// **Whether a window's move is a slide of the lights' window**: the
/// posted object is the window holding the lights now, and that window is
/// not the terminal's. By identity alone, so a test reaches it with plain
/// objects (an `NSWindow` is made only on the main thread, which the
/// library's tests are not on).
fn judged(
    posted: &macos_liquid_glass::objc2::runtime::AnyObject,
    holder: *const macos_liquid_glass::objc2::runtime::AnyObject,
    terminal: Option<*const macos_liquid_glass::objc2::runtime::AnyObject>,
) -> Slid {
    match (std::ptr::eq(posted, holder), terminal == Some(holder)) {
        (true, false) => Slid::Yes,
        (true, true) | (false, _) => Slid::No,
    }
}

/// **What a judged move does**: a slide is kept and wakes the owner once;
/// any other move -- every drag of every baud window reaches this observer,
/// which listens to all of them -- keeps nothing and wakes no one.
fn on_move(slid: &std::cell::Cell<Slid>, move_is: Slid, wake: impl FnOnce()) {
    if move_is == Slid::Yes {
        slid.set(Slid::Yes);
        wake();
    }
}

#[cfg(test)]
mod slid_tests {
    use super::{Slid, judged, on_move};
    use std::cell::Cell;

    /// **Only a slide wakes**: of a slide and two moves that are not, one
    /// wake and one kept slide. Break: wake on every move, and this counts
    /// three.
    #[test]
    fn only_a_slide_is_kept_and_wakes() {
        let (slid, wakes) = (Cell::new(Slid::No), Cell::new(0_u32));
        on_move(&slid, Slid::No, || wakes.set(wakes.get() + 1));
        assert_eq!(
            (slid.get(), wakes.get()),
            (Slid::No, 0),
            "a move that is not a slide"
        );
        on_move(&slid, Slid::Yes, || wakes.set(wakes.get() + 1));
        on_move(&slid, Slid::No, || wakes.set(wakes.get() + 1));
        assert_eq!(
            (slid.get(), wakes.get()),
            (Slid::Yes, 1),
            "one slide among moves"
        );
    }

    /// **The observer is removed with its owner**: Moves' drop hands the
    /// centre back the token `addObserverForName:` returned (the
    /// full-screen observer's pattern), so a closed window's observer
    /// judges no later move.
    #[test]
    fn the_move_observer_is_removed_on_drop() {
        let src = include_str!("hover.rs");
        let drop = src
            .split("impl Drop for Moves {")
            .nth(1)
            .expect("Moves has a Drop");
        let body = drop.split("\n}\n").next().expect("the drop's body");
        assert!(
            body.contains("center.removeObserver(observer)"),
            "the drop removes the token"
        );
        assert!(
            body.contains("self.token"),
            "the token it removes is Moves' own"
        );
    }
    use macos_liquid_glass::objc2::runtime::{AnyObject, NSObject};

    /// **A move is a slide only of the lights' own window, never the
    /// terminal's**: the title bar's window in full screen slid; the
    /// terminal's window, holding the lights while windowed, moved them
    /// with the view; another window is no one's. Break: drop the
    /// terminal's test and the windowed move reads as a slide.
    #[test]
    fn a_move_is_a_slide_only_of_the_lights_own_window() {
        let (bar, terminal, other) = (NSObject::new(), NSObject::new(), NSObject::new());
        let o = |x: &NSObject| -> *const AnyObject {
            let a: &AnyObject = x;
            a
        };
        assert_eq!(
            judged(&bar, o(&bar), Some(o(&terminal))),
            Slid::Yes,
            "full screen's bar"
        );
        assert_eq!(
            judged(&terminal, o(&terminal), Some(o(&terminal))),
            Slid::No,
            "windowed"
        );
        assert_eq!(
            judged(&other, o(&bar), Some(o(&terminal))),
            Slid::No,
            "another window"
        );
        assert_eq!(
            judged(&bar, o(&bar), None),
            Slid::Yes,
            "the terminal's window gone"
        );
    }
}
