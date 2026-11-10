//! **When a window closes**: `NSWindowWillCloseNotification`, "a
//! notification that the window object is about to close", whose object
//! "is the `NSWindow` object that's about to close"
//! (`[[source: apple-nswindow-willclosenotification]]`). It is posted for
//! every way a window closes -- the close button, File or Window > Close
//! (`performClose:`), and `close` itself -- so one observer per window
//! hears them all, and it needs no window delegate, which the window crate
//! does not let its consumer set.
//!
//! The block runs on the posting thread, the main thread, inside the
//! close: a caller that tears the window's view down defers that to a
//! later turn of the main queue, because the close may come from inside
//! one of the view's own callbacks.
//!
//! `unsafe` is allowed here for the registration, which the bindings mark
//! so, and for the framework constant it names.
#![allow(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use macos_liquid_glass::objc2_app_kit::{
    NSWindow, NSWindowDidBecomeKeyNotification, NSWindowDidChangeOcclusionStateNotification,
    NSWindowDidResignKeyNotification, NSWindowWillCloseNotification,
};
use macos_liquid_glass::objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName,
};

/// The observer of one of a window's notifications; dropping it removes the
/// observer, so a window that has gone is heard no more.
#[must_use = "dropping it stops hearing the window"]
pub struct Closing {
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

/// **Calls `then` each time `window` is about to close**, until the
/// returned [`Closing`] drops.
pub fn when_closing(window: &NSWindow, then: impl Fn() + 'static) -> Closing {
    // SAFETY: a framework constant, a valid string for the process's life.
    hear(window, unsafe { NSWindowWillCloseNotification }, then)
}

/// **Calls `then` each time how much of `window` is on screen changes**:
/// covered, uncovered, the app hidden or shown again, until the returned
/// [`Closing`] drops.
pub fn when_occlusion_changes(window: &NSWindow, then: impl Fn() + 'static) -> Closing {
    // SAFETY: a framework constant, a valid string for the process's life.
    hear(
        window,
        unsafe { NSWindowDidChangeOcclusionStateNotification },
        then,
    )
}

/// **Calls `then` each time `window` becomes or resigns the key window**:
/// `NSWindowDidBecomeKeyNotification`, "a notification that the window
/// object became the key window", and `NSWindowDidResignKeyNotification`,
/// "a notification that the window object resigned its status as key
/// window", whose object is the window in each
/// (`[[source: apple-nswindow-didbecomekeynotification]]`,
/// `[[source: apple-nswindow-didresignkeynotification]]`); until the
/// returned pair drops.
pub fn when_key_changes(window: &NSWindow, then: impl Fn() + Clone + 'static) -> [Closing; 2] {
    // SAFETY: framework constants, valid strings for the process's life.
    [
        hear(
            window,
            unsafe { NSWindowDidBecomeKeyNotification },
            then.clone(),
        ),
        hear(window, unsafe { NSWindowDidResignKeyNotification }, then),
    ]
}

fn hear(window: &NSWindow, name: &NSNotificationName, then: impl Fn() + 'static) -> Closing {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        crate::unwind::guard(
            crate::unwind::SiteName("a window's close, occlusion or key observer"),
            (),
            &then,
        );
    });
    let center = NSNotificationCenter::defaultCenter();
    let object: &AnyObject = window;
    // SAFETY: the object filter is this window, and no queue, so the block
    // runs on the posting thread -- the main thread, where a window closes
    // and changes -- which is what makes a block over main-thread state
    // sound here.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(Some(name), Some(object), None, &block)
    };
    Closing { token }
}

impl Drop for Closing {
    fn drop(&mut self) {
        let observer: &AnyObject = self.token.as_ref();
        // SAFETY: the token is the one `addObserverForName:` returned, the
        // object the centre registered.
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
    }
}

/// **The frame rig's window told it became or resigned the key window, or
/// that its occlusion changed** (`rig_call::RigWindow`; its other events
/// post nothing): the notification AppKit posts, posted to the window's
/// observers with the window as its object, as AppKit's names it
/// (`[[source: apple-nswindow-didbecomekeynotification]]`,
/// `[[source: apple-nswindow-didresignkeynotification]]`,
/// `[[source: apple-nswindow-occlusionstate]]`). The window is not made
/// key nor ordered front; the rig checks it never is key.
#[cfg(feature = "effect-probe")]
pub fn rig_post(window: &NSWindow, told: crate::rig_call::RigWindow) {
    use crate::rig_call::RigWindow;
    // SAFETY: framework constants, valid strings for the process's life.
    let name = match told {
        RigWindow::Untold
        | RigWindow::Restyled
        | RigWindow::Refocused
        | RigWindow::DisplayOptions
        | RigWindow::PointerOver
        | RigWindow::CommandExits
        | RigWindow::Settings
        | RigWindow::HostNamed
        | RigWindow::Released => return,
        RigWindow::BecameKey => unsafe { NSWindowDidBecomeKeyNotification },
        RigWindow::ResignedKey => unsafe { NSWindowDidResignKeyNotification },
        RigWindow::Shown => unsafe { NSWindowDidChangeOcclusionStateNotification },
    };
    let object: &AnyObject = window;
    // SAFETY: a notification with this window as its object, posted on the
    // main thread, where its observers expect it.
    unsafe {
        NSNotificationCenter::defaultCenter().postNotificationName_object(name, Some(object))
    };
}
