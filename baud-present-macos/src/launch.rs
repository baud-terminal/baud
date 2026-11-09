//! **Once the app has finished launching**: a one-shot call at the end of
//! `-[NSApplication finishLaunching]`, which `run()` "calls ... before it
//! starts the event loop" ([[source: apple-nsapplication-finishlaunching]]),
//! through the notification it posts there "to indicate that the app has
//! completed launching and is ready to run", on the main actor
//! ([[source: apple-nsapplication-didfinishlaunchingnotification]]).
//!
//! The window is shown from here (note AJ, section 16): the display link
//! that starts the launch logo ticks only for a view on a display
//! ([[source: sdk27-nsview-h]]), and `finishLaunching`, which activates the
//! app, comes between a launch that ends before `run()` and the event loop,
//! so a window shown before `run()` stood on screen through it with no
//! logo.
//!
//! **Before the first window**, the defaults the app registers
//! ([`register_defaults`]).
//!
//! `unsafe` is allowed here for the registrations, which the bindings mark
//! so, and for the framework constant it names.
#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2_app_kit::NSApplicationDidFinishLaunchingNotification;
use macos_liquid_glass::objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSString, NSUserDefaults,
};

/// The key AppKit reads for press-and-hold: on, holding a letter opens the
/// accent picker; off, the key repeats.
pub const PRESS_AND_HOLD: &str = "ApplePressAndHoldEnabled";

/// **A held key repeats**: registers `ApplePressAndHoldEnabled = NO`, so
/// holding a letter sends its repeats to the program rather than opening
/// the system's accent picker over the terminal, as kitty, iTerm2 and
/// ghostty register it at launch. Call before the first window can take a
/// key.
///
/// It goes in the registration domain, the process's own volatile
/// defaults, which every domain a user writes is searched before: nothing
/// is written to disk, and a user who wants the picker back sets
/// `defaults write dev.baud.terminal ApplePressAndHoldEnabled -bool YES`,
/// which wins over this.
pub fn register_defaults() {
    let key = NSString::from_str(PRESS_AND_HOLD);
    let off = NSNumber::new_bool(false);
    let value: &AnyObject = &off;
    let defaults = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[value]);
    // SAFETY: the one key is an NSString and the one value an NSNumber,
    // property-list types, as `registerDefaults:` requires of its
    // dictionary.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

/// **Calls `then` once, when the app has finished launching**: register
/// before `-[NSApplication run]`. The observer is never removed: it fires
/// once, at the one launch a process has, and holds nothing after.
pub fn when_launched(then: impl FnOnce() + 'static) {
    let once = RefCell::new(Some(then));
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        let f = once.borrow_mut().take();
        if let Some(f) = f {
            crate::unwind::guard(crate::unwind::SiteName("the launch observer"), (), f);
        }
    });
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: a framework constant, a valid string for the process's life;
    // no object filter and no queue, so the block runs on the posting
    // thread, the main thread for this notification -- which is what makes
    // a block over main-thread state sound here.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSApplicationDidFinishLaunchingNotification),
            None,
            None,
            &block,
        )
    };
    // The centre holds the observer until it is removed, and it never is:
    // the token's own release would not remove it, and nothing after the
    // launch needs it gone.
    core::mem::forget(token);
}
