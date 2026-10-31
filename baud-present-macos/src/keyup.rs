//! **A key let go while Command is held reaches the view.**
//!
//! `-[NSApplication sendEvent:]` keeps a key-up to itself while Command is
//! held: the window crate measured a Command-`a` reaching its view going
//! down and never coming up, where a plain `a` reaches both. A key pressed
//! without Command and let go after Command went down then sends no
//! release, and a program that asked for kitty's event types
//! (`[[source: kitty-keyboard]]`, "report key repeat and key release
//! events") would hold that key down for good.
//!
//! An application's local monitor sees an event before `sendEvent:` does,
//! so one monitor here hands each such key-up to the view's `keyUp:`, as
//! Ghostty's does, "Command keyUp events are never sent to the normal
//! responder chain so we send them here"
//! (`[[source: ghostty-d9840f3-surfaceview-appkit]]`), and kitty's
//! (`[[source: kitty-glfw-cocoa-init-851cb80]]`, lines 869-905). The
//! responder sends a release only for a key whose press reached the
//! program, so the release of a Command chord, the menu's, goes nowhere.
#![allow(unsafe_code)]

use block2::RcBlock;
use core::ptr::NonNull;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSView};

/// Whether baud's window has the keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyWindow {
    Ours,
    Other,
}

/// Whether a key-up is one AppKit keeps from the view and the monitor hands
/// to it: Command held, in baud's key window. Every other key-up AppKit
/// delivers itself, and a panel's keys stay the panel's.
pub fn forwards(flags: NSEventModifierFlags, window: KeyWindow) -> bool {
    flags.contains(NSEventModifierFlags::Command) && window == KeyWindow::Ours
}

/// One window's monitor; dropping it removes the monitor, and the view
/// it holds with it, so a closed window's key-ups are forwarded no more.
#[must_use = "dropping it stops forwarding the view's key-ups"]
pub struct KeyUps {
    monitor: Option<Retained<AnyObject>>,
}

/// **Install the monitor for `view`**, until the returned [`KeyUps`]
/// drops. One a window: each forwards only while its own window is key.
pub fn forward_command_key_ups(view: Retained<NSView>) -> KeyUps {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the handler a live event for the call.
        let e = unsafe { event.as_ref() };
        let window = match view.window().is_some_and(|w| w.isKeyWindow()) {
            true => KeyWindow::Ours,
            false => KeyWindow::Other,
        };
        if !forwards(e.modifierFlags(), window) {
            return event.as_ptr();
        }
        view.keyUp(e);
        // Delivered: AppKit would only keep it.
        core::ptr::null_mut()
    });
    // SAFETY: the handler returns the event it was given or null, the two
    // returns the binding allows.
    let monitor = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyUp, &block)
    };
    KeyUps { monitor }
}

impl Drop for KeyUps {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.take() {
            // SAFETY: the object `addLocalMonitorForEventsMatchingMask:`
            // returned, removed once.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyWindow, forwards};
    use macos_liquid_glass::objc2_app_kit::NSEventModifierFlags as F;

    /// **A key-up with Command held, in baud's key window, is forwarded**;
    /// one without Command is AppKit's to deliver, and another window's is
    /// that window's.
    #[test]
    fn only_a_command_key_up_in_our_window_is_forwarded() {
        assert!(forwards(F::Command, KeyWindow::Ours));
        assert!(forwards(F::Command | F::Shift, KeyWindow::Ours));
        assert!(!forwards(F::empty(), KeyWindow::Ours));
        assert!(!forwards(
            F::Shift | F::Option | F::Control,
            KeyWindow::Ours
        ));
        assert!(!forwards(F::Command, KeyWindow::Other));
    }

    /// **Inside a key-up monitor `NSApplication.currentEvent` is that
    /// key-up, with its Caps Lock flag**, which is where the responder
    /// reads Caps Lock for a release (`current_event_flags` in the app).
    /// Asked of AppKit itself: a probe compiled now posts a Command key-up
    /// with and without the lock to its own queue, dequeues and sends it as
    /// `-[NSApplication run]` does, and prints what its monitor saw. No
    /// window is made and the app may not activate, so nothing is shown.
    /// Perturbation: have the probe's monitor read a key-up made apart
    /// from the dispatched one and `current` reads 0.
    #[test]
    fn a_key_up_a_monitor_is_handed_is_the_current_event() {
        const PROBE: &str = r#"
            #import <AppKit/AppKit.h>
            int main(void) {
                @autoreleasepool {
                    [NSApplication sharedApplication];
                    [NSApp setActivationPolicy:NSApplicationActivationPolicyProhibited];
                    [NSApp finishLaunching];
                    __block long current = -1, caps = -1;
                    id m = [NSEvent addLocalMonitorForEventsMatchingMask:NSEventMaskKeyUp
                                                                 handler:^NSEvent *(NSEvent *e) {
                        current = [NSApp currentEvent] == e;
                        caps = ([[NSApp currentEvent] modifierFlags] & NSEventModifierFlagCapsLock) != 0;
                        return e;
                    }];
                    NSEventModifierFlags held[2] = {
                        NSEventModifierFlagCommand | NSEventModifierFlagCapsLock,
                        NSEventModifierFlagCommand,
                    };
                    for (int i = 0; i < 2; i++) {
                        NSEvent *up = [NSEvent keyEventWithType:NSEventTypeKeyUp
                            location:NSZeroPoint modifierFlags:held[i] timestamp:0
                            windowNumber:0 context:nil characters:@"a"
                            charactersIgnoringModifiers:@"a" isARepeat:NO keyCode:0];
                        [NSApp postEvent:up atStart:NO];
                        NSEvent *got = [NSApp nextEventMatchingMask:NSEventMaskAny
                            untilDate:[NSDate dateWithTimeIntervalSinceNow:2]
                            inMode:NSDefaultRunLoopMode dequeue:YES];
                        current = -1;
                        caps = -1;
                        [NSApp sendEvent:got];
                        printf("current%d=%ld\ncaps%d=%ld\n", i, current, i, caps);
                    }
                    [NSEvent removeMonitor:m];
                }
                return 0;
            }
"#;
        let dir = std::env::temp_dir().join(format!("baud-keyup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (src, bin) = (dir.join("probe.m"), dir.join("probe"));
        std::fs::write(&src, PROBE).expect("write the probe");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = std::process::Command::new(&cc)
            .args(["-fobjc-arc", "-framework", "AppKit", "-o"])
            .arg(&bin)
            .arg(&src)
            .output()
            .unwrap_or_else(|e| panic!("{cc} did not run: {e}"));
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = std::process::Command::new(&bin).output().expect("run it");
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8_lossy(&run.stdout).into_owned();
        let said: std::collections::HashMap<&str, i64> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| v.parse().ok().map(|n| (k, n)))
            .collect();
        assert_eq!(said.len(), 4, "the probe's four values: {text}");
        assert_eq!((said["current0"], said["caps0"]), (1, 1), "the lock on");
        assert_eq!((said["current1"], said["caps1"]), (1, 0), "the lock off");
    }
}
