//! **When a window has entered or left full screen**:
//! `NSWindowDidEnterFullScreenNotification` and
//! `NSWindowDidExitFullScreenNotification` ([[source: sdk27-nswindow-h]]
//! 976 and 978), each posted with the window as its object once its
//! transition is over. In full screen the title bar leaves the window:
//! "Fullscreen window toolbar is detached from window and hides/shows on
//! rollover" ([[source: sdk27-nsapplication-h]] 168). So the strip a
//! window keeps clear at its top for the title bar is kept only while the
//! window is not in full screen, and this is how baud hears the change.
//!
//! **The change is read from the notification's name**, never from the
//! window's style mask. A window can be told it is in full screen only by
//! a real transition, which takes the display, so a test posts the two
//! notifications by hand on a window that is never shown, and a block
//! that read `NSWindowStyleMask::FullScreen` would find it clear and
//! answer "windowed" to both.
//!
//! **Only the two "did" notifications.** A failed entry or exit is told
//! only to the window's delegate (`windowDidFailToEnterFullScreen:`,
//! [[source: sdk27-nswindow-h]] 864), which the window crate does not let
//! its consumer set. State taken at the "will" notification would stay
//! wrong after a failed transition; state taken at the "did" is right
//! after each one.
//!
//! No queue is given, so the block runs on the posting thread, the main
//! thread, at the end of the transition, from the run loop.
//!
//! `unsafe` is allowed here for the registration, which the bindings
//! mark so, and for the framework constants it names.
#![allow(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use macos_liquid_glass::objc2_app_kit::{
    NSWindow, NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
};
use macos_liquid_glass::objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName,
};

/// **Which way a window's full screen went.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    /// It is in full screen now: its title bar has left it.
    Entered,
    /// It is a window again, with its title bar.
    Exited,
}

impl Change {
    /// Both, in the order they are registered.
    pub const ALL: [Change; 2] = [Change::Entered, Change::Exited];

    /// **The change a notification named `name` tells**, or none for any
    /// other name.
    pub fn of(name: &NSNotificationName) -> Option<Change> {
        Change::ALL.into_iter().find(|c| c.name() == name)
    }

    /// The notification AppKit posts for it.
    pub fn name(self) -> &'static NSNotificationName {
        // SAFETY: framework constants, valid strings for the process's life.
        unsafe {
            match self {
                Change::Entered => NSWindowDidEnterFullScreenNotification,
                Change::Exited => NSWindowDidExitFullScreenNotification,
            }
        }
    }
}

/// The observers of one window's full screen; dropping it removes them,
/// so a window that has gone is heard no more.
#[must_use = "dropping it stops hearing the window's full screen"]
pub struct FullScreen {
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

/// **Calls `then` with each change of `window`'s full screen**, read from
/// the notification's name, until the returned [`FullScreen`] drops.
pub fn when_full_screen(window: &NSWindow, then: impl Fn(Change) + 'static) -> FullScreen {
    let block = RcBlock::new(move |n: NonNull<NSNotification>| {
        // SAFETY: the centre hands the block the notification it posts,
        // alive for the call.
        let n = unsafe { n.as_ref() };
        if let Some(change) = Change::of(&n.name()) {
            crate::unwind::guard("a window's full-screen observer", (), || then(change));
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    let object: &AnyObject = window;
    let tokens = Change::ALL
        .into_iter()
        .map(|c| {
            // SAFETY: the name is a framework constant; the object filter
            // is this window, and no queue, so the block runs on the
            // posting thread -- the main thread, where a window's full
            // screen changes -- which is what makes a block over
            // main-thread state sound here.
            unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(c.name()),
                    Some(object),
                    None,
                    &block,
                )
            }
        })
        .collect();
    FullScreen { tokens }
}

impl Drop for FullScreen {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for token in &self.tokens {
            let observer: &AnyObject = token.as_ref();
            // SAFETY: each token is one `addObserverForName:` returned, an
            // object the centre registered.
            unsafe { center.removeObserver(observer) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macos_liquid_glass::objc2_app_kit::{
        NSWindowDidResizeNotification, NSWindowWillCloseNotification,
    };

    /// **Each name tells its own change, and no other name tells one**:
    /// entering is the "did enter" notification, leaving the "did exit",
    /// and a resize or a close is neither. Perturbation: swap the two arms
    /// of `name` and the first two checks fail; answer `Exited` for an
    /// unknown name and the last two do.
    #[test]
    fn each_full_screen_notification_names_its_own_change() {
        // SAFETY: framework constants.
        let (enter, exit, resize, close) = unsafe {
            (
                NSWindowDidEnterFullScreenNotification,
                NSWindowDidExitFullScreenNotification,
                NSWindowDidResizeNotification,
                NSWindowWillCloseNotification,
            )
        };
        assert_eq!(Change::of(enter), Some(Change::Entered));
        assert_eq!(Change::of(exit), Some(Change::Exited));
        assert_eq!(Change::of(resize), None);
        assert_eq!(Change::of(close), None);
    }

    /// **The block decides from the notification's name, not the style
    /// mask**, and both names are registered: a window that is never
    /// shown cannot be put in full screen, so a test can only post the
    /// notifications, and a block that read the style mask would answer
    /// "windowed" to both. Perturbation: read `styleMask()` in the block
    /// and the second check fails.
    #[test]
    fn the_block_reads_the_name_and_registers_both() {
        let src = include_str!("full_screen.rs");
        let body = src
            .split_once(concat!("pub fn when_full_screen(", "window: &NSWindow"))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(b, _)| b)
            .expect("when_full_screen");
        assert!(
            body.contains(concat!("Change::of(&n.", "name())")),
            "{body}"
        );
        assert!(!body.contains(concat!("style", "Mask")), "{body}");
        assert!(body.contains(concat!("Change::", "ALL")), "{body}");
        assert!(src.contains(concat!("NSWindowDidEnter", "FullScreenNotification,")));
        assert!(src.contains(concat!("NSWindowDidExit", "FullScreenNotification,")));
    }
}
