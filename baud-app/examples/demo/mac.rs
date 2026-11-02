//! **Everything that touches the desktop**, on macOS: the window list,
//! the frontmost application, the Accessibility check and the events
//! themselves. Only [`Poster`] posts, and it can be made only once the
//! process is trusted; `--dry` never makes one.

use std::cell::Cell;
use std::process::{Command, Stdio};
use std::time::Duration;

use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFRunLoop, CFString, CFType, CGPoint,
    kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventSource, CGEventSourceStateID, CGEventTapLocation, CGEventType,
    CGMouseButton, CGScrollEventUnit, CGWindowListCopyWindowInfo, CGWindowListOption,
    kCGNullWindowID, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer, kCGWindowOwnerName,
    kCGWindowOwnerPID,
};

use crate::guard::{self, OnScreen, Rect, Window};
use crate::keys::{Held, Key};
use crate::session::{WHEEL_GAP_MS, WHEEL_SETTLE_MS};

/// CoreFoundation's `Boolean`, an `unsigned char`; the probe holds it.
type Boolean = u8;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// `extern Boolean AXIsProcessTrusted (void)`: "Returns whether the
    /// current process is a trusted accessibility client"
    /// (`[[source: macos-sdk-axuielement-h]]`). Never the `WithOptions`
    /// form, whose prompt option puts up a dialog.
    fn AXIsProcessTrusted() -> Boolean;
}

/// Whether this process may post events: Accessibility trust, which macOS
/// grants the terminal application the example runs in.
pub fn trusted() -> bool {
    // SAFETY: it takes nothing and returns a `Boolean`, the type the probe
    // holds to the header.
    unsafe { AXIsProcessTrusted() != 0 }
}

/// **Whether this process may film the screen**, asking nothing:
/// `CGPreflightScreenCaptureAccess` "Returns whether the current process
/// already has permission to capture the contents of the screen, without
/// prompting the user" (`[[source: macos-sdk-cgwindow-h]]`). macOS grants
/// it to the terminal application the example runs in, the grant the
/// recorder `--record` starts from it needs.
pub fn may_record() -> bool {
    objc2_core_graphics::CGPreflightScreenCaptureAccess()
}

/// **The windows the window server lists**, front to back
/// (`[[source: macos-sdk-cgwindow-h]]`): on screen only, or every one.
pub fn windows(on_screen_only: bool) -> Vec<Window> {
    let option = if on_screen_only {
        CGWindowListOption::OptionOnScreenOnly
    } else {
        CGWindowListOption::OptionAll
    };
    let Some(list) = CGWindowListCopyWindowInfo(option, kCGNullWindowID) else {
        return Vec::new();
    };
    // SAFETY: the header returns "an array of window dictionaries", each
    // keyed by the `kCGWindow...` strings.
    let list: CFRetained<CFArray<CFDictionary<CFString, CFType>>> =
        unsafe { CFRetained::cast_unchecked(list) };
    // SAFETY: constant strings CoreGraphics defines, only read.
    let (pid_key, name_key, layer_key, on_key, bounds_key) = unsafe {
        (
            kCGWindowOwnerPID,
            kCGWindowOwnerName,
            kCGWindowLayer,
            kCGWindowIsOnscreen,
            kCGWindowBounds,
        )
    };
    let corner = ["X", "Y", "Width", "Height"].map(CFString::from_static_str);
    let number = |d: &CFDictionary<CFString, CFType>, k: &CFString| {
        d.get(k).and_then(|v| v.downcast::<CFNumber>().ok())
    };
    list.iter()
        .map(|d| {
            // SAFETY: of `kCGWindowBounds` "The value of this key is a
            // CFDictionary", the rectangle's four numbers keyed by strings.
            let bounds = d
                .get(bounds_key)
                .and_then(|v| v.downcast::<CFDictionary>().ok())
                .map(|b| unsafe {
                    CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(b)
                });
            let side = |i: usize| {
                bounds
                    .as_ref()
                    .and_then(|b| number(b, &corner[i]))
                    .and_then(|n| n.as_f64())
                    .unwrap_or(0.0)
            };
            Window {
                owner: number(&d, pid_key).and_then(|n| n.as_i32()).unwrap_or(-1),
                owner_name: d
                    .get(name_key)
                    .and_then(|v| v.downcast::<CFString>().ok())
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                layer: number(&d, layer_key).and_then(|n| n.as_i64()).unwrap_or(-1),
                on_screen: match d.get(on_key).and_then(|v| v.downcast::<CFBoolean>().ok()) {
                    Some(b) if b.as_bool() => OnScreen::Yes,
                    _ => OnScreen::No,
                },
                bounds: Rect {
                    x: side(0),
                    y: side(1),
                    w: side(2),
                    h: side(3),
                },
            }
        })
        .collect()
}

/// **The frontmost application's pid, read after a turn of the run
/// loop.** AppKit keeps an application's changing properties "until the
/// next turn of the main run loop in a common mode"
/// (`[[source: macos-sdk-nsrunningapplication-h]]`), so the default mode
/// is run once, here on the main thread, before every read.
pub fn front_app() -> Option<i32> {
    // SAFETY: a constant CoreFoundation defines, only read.
    let mode = unsafe { kCFRunLoopDefaultMode };
    let _ = CFRunLoop::run_in_mode(mode, 0.0, false);
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|a| a.processIdentifier())
}

/// The owner of the frontmost ordinary window.
pub fn front_owner() -> Option<i32> {
    windows(true).iter().find(|w| w.layer == 0).map(|w| w.owner)
}

/// **Bring `pid`'s window to the front** as the first recording did:
/// System Events, asked to make the process frontmost.
pub fn raise(pid: i32) {
    let script = format!(
        "tell application \"System Events\" to set frontmost of (first process whose unix id is {pid}) to true"
    );
    let _ = Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// **What posts keys and wheel turns into baud's window**, through the
/// HID event tap, with the U.S. layout's key codes. Every event is guarded
/// ([`guard::keys`], [`guard::wheel`]): the first refusal stops the
/// session having posted nothing more.
pub struct Poster {
    pid: i32,
    source: CFRetained<CGEventSource>,
    posted: Cell<usize>,
}

impl Poster {
    /// A poster for `pid`, only while this process is trusted.
    pub fn new(pid: i32) -> Result<Poster, String> {
        if !trusted() {
            return Err("this process is not trusted for Accessibility".to_string());
        }
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok_or("CoreGraphics made no event source")?;
        Ok(Poster {
            pid,
            source,
            posted: Cell::new(0),
        })
    }

    fn refused(&self, why: guard::Refused) -> String {
        format!("REFUSED: {}; posted {} events", why.0, self.posted.get())
    }

    /// One key, down and up, held `hold` between them; the guard asked
    /// before each.
    pub fn key(&self, k: Key, hold: Duration) -> Result<(), String> {
        let mut units = [0u16; 2];
        let text: &[u16] = k.text.encode_utf16(&mut units);
        let flags = match k.held {
            Held::Nothing => CGEventFlags::empty(),
            Held::Shift => CGEventFlags::MaskShift,
        };
        for down in [true, false] {
            guard::keys(self.pid, &windows(true), front_app()).map_err(|r| self.refused(r))?;
            let e = CGEvent::new_keyboard_event(Some(&self.source), k.code, down)
                .ok_or("CoreGraphics made no key event")?;
            CGEvent::set_flags(Some(&e), flags);
            // SAFETY: the pointer and the length are the slice's own, alive
            // for the call, which copies the characters.
            unsafe {
                CGEvent::keyboard_set_unicode_string(Some(&e), text.len() as _, text.as_ptr())
            };
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e));
            if down {
                std::thread::sleep(hold);
            }
        }
        self.posted.set(self.posted.get() + 1);
        Ok(())
    }

    /// **The wheel at `at`**: the pointer moved there, then `turns` events
    /// of `lines` lines, each through `CGEventCreateScrollWheelEvent2`, the
    /// fixed-argument form (`[[source: macos-sdk-cgevent-h]]`), the guard
    /// asked before every one.
    pub fn wheel(&self, at: (f64, f64), turns: i32, lines: i32) -> Result<(), String> {
        let p = CGPoint { x: at.0, y: at.1 };
        let check =
            || guard::wheel(self.pid, &windows(true), front_app(), at).map_err(|r| self.refused(r));
        check()?;
        let moved = CGEvent::new_mouse_event(
            Some(&self.source),
            CGEventType::MouseMoved,
            p,
            CGMouseButton::Left,
        )
        .ok_or("CoreGraphics made no pointer event")?;
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&moved));
        std::thread::sleep(Duration::from_millis(WHEEL_SETTLE_MS));
        for _ in 0..turns.unsigned_abs() {
            let e = CGEvent::new_scroll_wheel_event2(
                Some(&self.source),
                CGScrollEventUnit::Line,
                1,
                lines * turns.signum(),
                0,
                0,
            )
            .ok_or("CoreGraphics made no wheel event")?;
            CGEvent::set_location(Some(&e), p);
            check()?;
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e));
            self.posted.set(self.posted.get() + 1);
            std::thread::sleep(Duration::from_millis(WHEEL_GAP_MS));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{RETURN, RETURN_NAME, US};

    /// **The key codes, and the trust call's type, are the SDK headers'
    /// own**: a C probe compiled against `Carbon.h` and
    /// `ApplicationServices.h` prints every `kVK_` name the U.S. table
    /// gives, `kVK_Return`, and `sizeof(Boolean)`, and refuses to compile
    /// unless `AXIsProcessTrusted` is `Boolean (void)`. A name the header
    /// does not have fails the compile; a code that differs fails here.
    /// The control: the probe printed one value a line it was given.
    /// Perturbation: give `p` the code 0x22 and its row fails.
    #[test]
    fn the_key_codes_are_the_headers_own() {
        let mut src = String::from(
            "#include <stdio.h>\n#include <Carbon/Carbon.h>\n\
             #include <ApplicationServices/ApplicationServices.h>\n\
             _Static_assert(__builtin_types_compatible_p(__typeof__(AXIsProcessTrusted), \
             Boolean (void)), \"AXIsProcessTrusted is Boolean (void)\");\n\
             int main(void) {\n  printf(\"Boolean.size=%d\\n\", (int)sizeof(Boolean));\n",
        );
        let names: Vec<&str> = US.iter().map(|(_, n, _)| *n).chain([RETURN_NAME]).collect();
        for n in &names {
            src.push_str(&format!("  printf(\"{n}=%d\\n\", (int){n});\n"));
        }
        src.push_str("  return 0;\n}\n");
        let dir = std::env::temp_dir().join(format!("baud-demo-probe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (c, bin) = (dir.join("probe.c"), dir.join("probe"));
        std::fs::write(&c, src).expect("write the probe");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = Command::new(&cc)
            .arg("-o")
            .arg(&bin)
            .arg(&c)
            .output()
            .expect("cc");
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let run = Command::new(&bin).output().expect("run the probe");
        let _ = std::fs::remove_dir_all(&dir);
        let text = String::from_utf8_lossy(&run.stdout).into_owned();
        let h: std::collections::HashMap<&str, i64> = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter_map(|(k, v)| v.parse().ok().map(|n| (k, n)))
            .collect();
        assert_eq!(h.len(), names.len() + 1, "one value a line: {text}");
        assert_eq!(h["Boolean.size"], std::mem::size_of::<Boolean>() as i64);
        for (ch, name, code) in US {
            assert_eq!(h[name], i64::from(code), "{ch:?} is {name}");
        }
        assert_eq!(h[RETURN_NAME], i64::from(RETURN.code));
    }
}
