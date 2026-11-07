//! **A word to VoiceOver**: AppKit's announcement, which a screen reader
//! speaks and nothing else hears. It draws nothing and asks for no focus, and
//! with no screen reader running AppKit has no one to tell. `NSApp` is the
//! element it comes from, there being no accessibility element for the grid.
//!
//! `unsafe` for the framework's constants and the post, each named below.
#![allow(unsafe_code)]

use macos_liquid_glass::objc2::MainThreadMarker;
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityPriorityKey,
    NSAccessibilityPriorityLevel, NSApplication,
};
use macos_liquid_glass::objc2_foundation::{NSDictionary, NSNumber, NSString};

/// **`words` spoken**, at high priority so a quick copy's words are not lost
/// behind whatever VoiceOver was saying. The caller paces it (`copied.rs`).
pub fn announce(mtm: MainThreadMarker, words: &str) {
    let app = NSApplication::sharedApplication(mtm);
    let words = NSString::from_str(words);
    let priority = NSNumber::new_isize(NSAccessibilityPriorityLevel::High.0);
    let (said, level): (&AnyObject, &AnyObject) = (&words, &priority);
    // SAFETY: the two keys and the notification are framework constants,
    // valid strings for the process's life. The header has the announcement a
    // string and the priority a number, and the element is the application.
    unsafe {
        let info: macos_liquid_glass::objc2::rc::Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(
                &[NSAccessibilityAnnouncementKey, NSAccessibilityPriorityKey],
                &[said, level],
            );
        NSAccessibilityPostNotificationWithUserInfo(
            &app,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&info),
        );
    }
}
