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
        // A panic in the view's key-up stops here, and the event was
        // delivered as far as it got.
        crate::unwind::guard(crate::unwind::SiteName("the key-up monitor"), (), || {
            view.keyUp(e)
        });
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

    /// **Whether a test that needs the window server can be asked here.**
    #[derive(Debug, PartialEq, Eq)]
    enum Asked {
        Here,
        Excluded(&'static str),
    }

    /// **Whether the probe found a login session**, its `session=` line.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Session {
        Present,
        Absent,
    }

    /// What the probe's session means: an app's event queue is the window
    /// server's, so with no login session a posted event never comes back
    /// and nothing the probe prints says anything about AppKit.
    fn asked(session: Session) -> Asked {
        match session {
            Session::Present => Asked::Here,
            Session::Absent => {
                Asked::Excluded("no login session: an app's event queue is the window server's")
            }
        }
    }

    /// **Says that `test` was not asked, and why**: the line
    /// `EXCLUDED (<why>): <test>` on stderr, and, where the run names a
    /// directory for them (`BAUD_EXCLUDED_DIR`), a file named for the test
    /// holding the reason, which the run's own count reads, so the test
    /// gives the same answer wherever it runs: asked, or excluded by name.
    fn exclude(test: &str, why: &str, dir: Option<&std::path::Path>) {
        eprintln!("EXCLUDED ({why}): {test}");
        if let Some(dir) = dir {
            std::fs::create_dir_all(dir).expect("the exclusions directory");
            std::fs::write(dir.join(test), why).expect("the exclusion");
        }
    }

    /// **A run with no login session is excluded, not judged**: the
    /// probe's `session=0` is an exclusion with its reason, written where
    /// the run reads exclusions, and `session=1` is a run.
    #[test]
    fn a_run_with_no_login_session_is_excluded_not_judged() {
        assert_eq!(asked(Session::Present), Asked::Here);
        let Asked::Excluded(why) = asked(Session::Absent) else {
            panic!("no session is not an exclusion");
        };
        assert!(why.contains("no login session"), "{why}");
        let dir = std::env::temp_dir().join(format!("baud-excluded-{}", std::process::id()));
        exclude("a::test", why, Some(&dir));
        let said = std::fs::read_to_string(dir.join("a::test")).expect("the exclusion file");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(said, why);
    }

    /// **Inside a key-up monitor `NSApplication.currentEvent` is that
    /// key-up, with its Caps Lock flag**, which is where the responder
    /// reads Caps Lock for a release (`current_event_flags` in the app).
    /// Asked of AppKit itself: a probe compiled now posts a Command key-up
    /// with and without the lock to its own queue, dequeues and sends it as
    /// `-[NSApplication run]` does, and prints what its monitor saw. No
    /// window is made and the app may not activate, so nothing is shown.
    /// An app's event queue is the window server's, so the probe needs the
    /// login session's: it says whether it has one
    /// (`CGSessionCopyCurrentDictionary`) and whether each posted key-up
    /// came back, and a run without one fails on that, not on a value the
    /// monitor never set.
    /// Perturbation: have the probe's monitor read a key-up made apart
    /// from the dispatched one and `current` reads 0.
    #[test]
    fn a_key_up_a_monitor_is_handed_is_the_current_event() {
        const PROBE: &str = r#"
            #import <AppKit/AppKit.h>
            #import <CoreGraphics/CoreGraphics.h>
            int main(void) {
                @autoreleasepool {
                    CFDictionaryRef session = CGSessionCopyCurrentDictionary();
                    printf("session=%d\n", session != NULL);
                    if (session != NULL) CFRelease(session);
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
                        printf("got%d=%d\n", i, got != nil);
                        current = -1;
                        caps = -1;
                        if (got != nil) [NSApp sendEvent:got];
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
            .args([
                "-fobjc-arc",
                "-framework",
                "AppKit",
                "-framework",
                "CoreGraphics",
                "-o",
            ])
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
        assert_eq!(said.len(), 7, "the probe's seven values: {text}");
        let session = match said["session"] {
            1 => Session::Present,
            _ => Session::Absent,
        };
        if let Asked::Excluded(why) = asked(session) {
            exclude(
                "keyup::tests::a_key_up_a_monitor_is_handed_is_the_current_event",
                why,
                std::env::var_os("BAUD_EXCLUDED_DIR")
                    .as_deref()
                    .map(std::path::Path::new),
            );
            return;
        }
        assert_eq!(
            (said["got0"], said["got1"]),
            (1, 1),
            "a posted key-up did not come back from the queue"
        );
        assert_eq!((said["current0"], said["caps0"]), (1, 1), "the lock on");
        assert_eq!((said["current1"], said["caps1"]), (1, 0), "the lock off");
    }
}
