//! **Everything that touches the desktop**, on macOS: the window list,
//! the frontmost application, the Accessibility check and the events
//! themselves, and the session those events would land in. Only [`Poster`]
//! posts, and it can be made only once the process is trusted; `--dry` never
//! makes one.

use std::cell::Cell;
use std::ffi::c_void;
use std::process::{Command, Stdio};
use std::ptr::{NonNull, null_mut};
use std::time::Duration;

use objc2_app_kit::NSWorkspace;
use objc2_core_foundation::{
    CFAllocator, CFArray, CFBoolean, CFData, CFDictionary, CFNumber, CFPropertyListCreateWithData,
    CFPropertyListMutabilityOptions, CFRetained, CFRunLoop, CFString, CFType, CGPoint, Type,
    kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGDisplayIsActive, CGDisplayIsAsleep, CGEvent, CGEventFlags, CGEventSource,
    CGEventSourceStateID, CGEventTapLocation, CGEventType, CGMainDisplayID, CGMouseButton,
    CGScrollEventUnit, CGSessionCopyCurrentDictionary, CGWindowListCopyWindowInfo,
    CGWindowListOption, kCGNullWindowID, kCGWindowBounds, kCGWindowIsOnscreen, kCGWindowLayer,
    kCGWindowOwnerName, kCGWindowOwnerPID,
};

use crate::guard::{self, Console, Monitor, OnScreen, Reading, Rect, Window};
use crate::keys::{Held, Key};
use crate::session::{DRAG_GAP_MS, DRAG_MOVES, DRAG_PAUSE_MS, WHEEL_GAP_MS, WHEEL_SETTLE_MS};
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::ProtocolObject;
use macos_liquid_glass::objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use macos_liquid_glass::objc2_foundation::{NSArray, NSData, NSString};

/// **A key's event, not posted.** A key under Control is given no text of
/// its own: read back through `NSEvent`, an unposted Ctrl-d with none has
/// `characters` U+0004 and `charactersIgnoringModifiers` `d`, as a real
/// press has, while one given U+0004 has U+0004 for both, which loses the
/// key the app's `chord` names it by, and one given `d` sends `d` as text.
/// Command itself ([`crate::keys::COMMAND`]) is a modifier's key: its press
/// carries Command, its release nothing, and neither any text.
fn key_event(source: &CGEventSource, k: Key, down: bool) -> Result<CFRetained<CGEvent>, String> {
    let e = CGEvent::new_keyboard_event(Some(source), k.code, down)
        .ok_or("CoreGraphics made no key event")?;
    let modifier = k == crate::keys::COMMAND;
    let flags = match k.held {
        Held::Nothing => CGEventFlags::empty(),
        Held::Shift => CGEventFlags::MaskShift,
        Held::Control => CGEventFlags::MaskControl,
        Held::Command if modifier && !down => CGEventFlags::empty(),
        Held::Command => CGEventFlags::MaskCommand,
        Held::Arrow => CGEventFlags::MaskNumericPad | CGEventFlags::MaskSecondaryFn,
    };
    CGEvent::set_flags(Some(&e), flags);
    if k.held != Held::Control && !modifier {
        let mut units = [0u16; 2];
        let text: &[u16] = k.text.encode_utf16(&mut units);
        // SAFETY: the pointer and the length are the slice's own, alive
        // for the call, which copies the characters.
        unsafe { CGEvent::keyboard_set_unicode_string(Some(&e), text.len() as _, text.as_ptr()) };
    }
    Ok(e)
}

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

/// `SIGHUP`, `SIGINT` and `SIGTERM` in `<sys/signal.h>`; the probe holds them.
const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

extern "C" {
    /// `void (*signal(int, void (*)(int)))(int)`; the probe holds the type
    /// to `<signal.h>`.
    fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
}

/// **Ctrl-C's handler**: it stores to a lock-free atomic and does nothing
/// else, which a handler may (`[[source: posix-xsh-chap02]]`, 2.4.3).
extern "C" fn on_interrupt(_: i32) {
    crate::container::INTERRUPTED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// **Ctrl-C sets [`crate::container::INTERRUPTED`]** rather than ending
/// the process, so the session's waits return through the drops that
/// remove the container and delete its VM. A handler, not `SIG_IGN`:
/// a caught signal is reset to its default in a program the demo starts
/// (`[[source: posix-exec]]`), so zsh, ssh and baud still take Ctrl-C.
pub fn catch_interrupt() {
    // SAFETY: `on_interrupt` is `void (int)`, the type the probe holds
    // `signal`'s handler to, and touches only an atomic.
    unsafe {
        signal(SIGINT, on_interrupt);
        // A closed terminal and a harness stopping it, as likely as Ctrl-C.
        signal(SIGHUP, on_interrupt);
        signal(SIGTERM, on_interrupt);
    }
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

/// an IOKit registry entry, `mach_port_t` and `IOOptionBits`, each 32 bits;
/// the probe holds them.
type IoEntry = u32;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    /// `const mach_port_t kIOMainPortDefault`
    /// (`[[source: macos-sdk-iokitlib-h]]`).
    static kIOMainPortDefault: u32;
    /// "A handle to the IORegistryEntry root instance, to be released with
    /// IOObjectRelease by the caller, or MACH_PORT_NULL on failure."
    fn IORegistryGetRootEntry(main_port: u32) -> IoEntry;
    /// "A CF container is created and returned the caller on success. The
    /// caller should release with CFRelease."
    fn IORegistryEntryCreateCFProperty(
        entry: IoEntry,
        key: &CFString,
        allocator: Option<&CFAllocator>,
        options: u32,
    ) -> Option<NonNull<CFType>>;
    fn IOObjectRelease(object: IoEntry) -> i32;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    /// `extern Boolean IsSecureEventInputEnabled(void)`: "whether secure
    /// event input is enabled by any process, not just the current process"
    /// (`[[source: macos-sdk-carboneventscore-h]]`).
    fn IsSecureEventInputEnabled() -> Boolean;
}

extern "C" {
    /// `int proc_listallpids(void *buffer, int buffersize)`
    /// (`[[source: macos-sdk-libproc-h]]`).
    fn proc_listallpids(buffer: *mut c_void, buffersize: i32) -> i32;
    /// `int proc_name(int pid, void *buffer, uint32_t buffersize)`, the
    /// same header's.
    fn proc_name(pid: i32, buffer: *mut c_void, buffersize: u32) -> i32;
}

/// `uid_t`, 32 bits; the probe holds it.
type Uid = u32;

/// `struct passwd` as far as its first member, `char *pw_name`, the one
/// read; the probe holds it at offset 0. Never made, only read through the
/// pointer `getpwuid` returns.
#[repr(C)]
struct Passwd {
    pw_name: *const std::ffi::c_char,
}

extern "C" {
    /// `uid_t getuid(void)` (`<unistd.h>`).
    fn getuid() -> Uid;
    /// `struct passwd *getpwuid(uid_t)` (`<pwd.h>`): the entry, in storage
    /// the next call may overwrite, or null.
    fn getpwuid(uid: Uid) -> *const Passwd;
}

/// **The account's name**, `getpwuid(getuid())`'s `pw_name`: what
/// `LOGNAME` and `USER` hold in a baud the Dock starts, which launchd gives
/// them. Without them the session's zsh sets `LOGNAME` from `getlogin()`,
/// which in a process started under another login session reads that
/// session's name; starship then prints the account's name on a line of its
/// own, as a user other than the one logged in. Copied out at once, before
/// anything can call `getpwuid` again.
pub fn account() -> Result<String, String> {
    // SAFETY: `getuid` takes nothing and cannot fail; `getpwuid` takes any
    // uid and returns null or an entry that stays valid until the next call,
    // and nothing else in the demo calls it.
    let entry = unsafe { getpwuid(getuid()) };
    if entry.is_null() {
        return Err("getpwuid: no entry for this process's uid".to_string());
    }
    // SAFETY: `entry` is non-null and points at a `struct passwd`, whose
    // first member is `pw_name` (the probe holds the offset).
    let name = unsafe { (*entry).pw_name };
    if name.is_null() {
        return Err("getpwuid: an entry with no name".to_string());
    }
    // SAFETY: `pw_name` is a NUL-terminated string in the entry's storage,
    // read before any other call can overwrite it.
    let name = unsafe { std::ffi::CStr::from_ptr(name) };
    name.to_str()
        .map(str::to_string)
        .map_err(|_| "getpwuid: a name that is not UTF-8".to_string())
}

/// **baud's environment with who the shell runs as, as launchd tells a
/// Dock-started baud**: `env` ([`crate::scratch::env`]), then `LOGNAME` and
/// `USER`, each `name`. Given to baud's process alone and never written into
/// the scratch root's `env` file or any other file.
pub fn with_account(env: &[(String, String)], name: &str) -> Vec<(String, String)> {
    let mut all = env.to_vec();
    all.extend(["LOGNAME", "USER"].map(|k| (k.to_string(), name.to_string())));
    all
}

/// The registry property the kernel keeps the lock in, on the root. In no SDK
/// header; `ioreg -n Root -d1` shows it.
const IO_CONSOLE_LOCKED: &str = "IOConsoleLocked";
/// The session dictionary's lock, in no SDK header, and absent while the
/// screen is unlocked.
const SCREEN_IS_LOCKED: &str = "CGSSessionScreenIsLocked";
/// `kCGSessionOnConsoleKey`, a `CFSTR` macro in
/// `[[source: macos-sdk-cgsession-h]]`, so the crate has no constant for it;
/// the probe holds the spelling to the header.
const ON_CONSOLE: &str = "kCGSSessionOnConsoleKey";
/// `ScreenSaverEngine.app`'s executable, whose name is 17 characters.
const SAVER: &str = "ScreenSaverEngine";

/// A CFBoolean as a reading, and anything else as none.
fn boolean(v: Option<CFRetained<CFType>>) -> Reading {
    match v.map(|v| v.downcast::<CFBoolean>()) {
        Some(Ok(b)) if b.as_bool() => Reading::Yes,
        Some(Ok(_)) => Reading::No,
        Some(Err(_)) | None => Reading::Unread,
    }
}

/// **`IOConsoleLocked` on the registry's root**, unread when there is no root
/// or no such property.
fn registry_locked() -> Reading {
    let key = CFString::from_static_str(IO_CONSOLE_LOCKED);
    // SAFETY: the types are the header's, which the probe holds; the root
    // is released here, as its header says, and the property, returned
    // created, goes into a `CFRetained` that releases it.
    unsafe {
        let root = IORegistryGetRootEntry(kIOMainPortDefault);
        if root == 0 {
            return Reading::Unread;
        }
        let value =
            IORegistryEntryCreateCFProperty(root, &key, None, 0).map(|v| CFRetained::from_raw(v));
        IOObjectRelease(root);
        boolean(value)
    }
}

/// **The window server's session**: whether it says the screen is locked and
/// whether it is on the console. No dictionary -- "the caller is not running
/// within a Quartz GUI session or the window server is disabled"
/// (`[[source: macos-sdk-cgsession-h]]`) -- reads neither. The lock's key is
/// absent while unlocked, so absent is `No`.
fn session(d: Option<CFRetained<CFDictionary>>) -> (Reading, Reading) {
    let Some(d) = d else {
        return (Reading::Unread, Reading::Unread);
    };
    // SAFETY: the header's dictionary is keyed by strings.
    let d = unsafe { CFRetained::cast_unchecked::<CFDictionary<CFString, CFType>>(d) };
    let locked = match d.get(&CFString::from_static_str(SCREEN_IS_LOCKED)) {
        Some(v) => boolean(Some(v)),
        None => Reading::No,
    };
    (
        locked,
        boolean(d.get(&CFString::from_static_str(ON_CONSOLE))),
    )
}

/// **Whether a process of exactly this name exists**, in this process and by
/// no subprocess: `/usr/bin/pgrep` took 20 ms here, against a key held 16. A
/// list that may have been cut short reads as no answer, and a process that
/// gives no name is passed over, it having exited or being another user's.
/// The header calls these "private interfaces", so a list in which no process
/// gave a name reads as no answer too.
fn process_named(wanted: &str) -> Reading {
    let mut pids = vec![0i32; 1 << 14];
    let room = pids.len();
    // SAFETY: the buffer and its size in bytes are the vector's own.
    let n = unsafe { proc_listallpids(pids.as_mut_ptr().cast(), (room * 4) as i32) };
    let Ok(n) = usize::try_from(n) else {
        return Reading::Unread;
    };
    if n == 0 || n >= room {
        return Reading::Unread;
    }
    // 2 * MAXCOMLEN + 1: `proc_name` gives the long name, not `p_comm`'s 16.
    let mut name = [0u8; 33];
    let mut named = false;
    for &pid in &pids[..n] {
        // SAFETY: the buffer and its size are the array's own.
        let len = unsafe { proc_name(pid, name.as_mut_ptr().cast(), name.len() as u32) };
        if let Ok(len @ 1..) = usize::try_from(len) {
            named = true;
            if name.get(..len) == Some(wanted.as_bytes()) {
                return Reading::Yes;
            }
        }
    }
    if named { Reading::No } else { Reading::Unread }
}

/// What a call about display `main` said, and no answer when there is no
/// main display to ask about (`kCGNullDirectDisplay`, 0).
fn display(main: u32, is: bool) -> Reading {
    match (main, is) {
        (0, _) => Reading::Unread,
        (_, true) => Reading::Yes,
        (_, false) => Reading::No,
    }
}

/// **The session a posted key would land in, read now**, each state from its
/// own call and none of them a window's: see [`Console`]'s fields.
pub fn console() -> Console {
    let (session_locked, on_console) = session(CGSessionCopyCurrentDictionary());
    let main = CGMainDisplayID();
    // SAFETY: it takes nothing and returns a `Boolean`, the type the probe
    // holds to the header.
    let secure = unsafe { IsSecureEventInputEnabled() };
    Console {
        registry_locked: registry_locked(),
        session_locked,
        on_console,
        secure_input: if secure != 0 {
            Reading::Yes
        } else {
            Reading::No
        },
        display_asleep: display(main, CGDisplayIsAsleep(main)),
        display_active: display(main, CGDisplayIsActive(main)),
        saver: process_named(SAVER),
    }
}

/// **`com.apple.spaces`'s `Monitors`**, from what `defaults export` wrote:
/// each one's `Display Identifier` and its `Current Space`'s `type`, by
/// CoreFoundation's own reader of a property list. `None` when it is not of
/// that shape.
fn monitors(plist: &[u8]) -> Option<Vec<Monitor>> {
    type Dict = CFDictionary<CFString, CFType>;
    fn dict<T: Type + 'static>(v: CFRetained<T>) -> Option<CFRetained<Dict>> {
        let d = v.downcast::<CFDictionary>().ok()?;
        // SAFETY: a property list's dictionaries are keyed by strings.
        Some(unsafe { CFRetained::cast_unchecked::<Dict>(d) })
    }
    let child = |d: &Dict, k: &'static str| d.get(&CFString::from_static_str(k));
    let data = CFData::from_bytes(plist);
    // SAFETY: the data is alive for the call, and a null format and error
    // are what the function's documentation allows for what is not wanted.
    let root = unsafe {
        CFPropertyListCreateWithData(
            None,
            Some(&data),
            CFPropertyListMutabilityOptions::Immutable.0,
            null_mut(),
            null_mut(),
        )
    }?;
    let top = dict(root)?;
    let config = dict(child(&top, "SpacesDisplayConfiguration")?)?;
    let data = dict(child(&config, "Management Data")?)?;
    let list = child(&data, "Monitors")?.downcast::<CFArray>().ok()?;
    // SAFETY: `Monitors` is a list of dictionaries in the file this reads,
    // and each is asked of its type below.
    let list = unsafe { CFRetained::cast_unchecked::<CFArray<CFType>>(list) };
    list.iter()
        .map(|m| {
            let m = dict(m)?;
            let display = child(&m, "Display Identifier")?
                .downcast::<CFString>()
                .ok()?
                .to_string();
            let current = child(&m, "Current Space")
                .and_then(dict)
                .and_then(|s| child(&s, "type"))
                .and_then(|t| t.downcast::<CFNumber>().ok())
                .and_then(|n| n.as_i64());
            Some(Monitor { display, current })
        })
        .collect()
}

/// **The main display's current Space's `type`**, from
/// `defaults export com.apple.spaces -`, which only reads; `None` when that
/// says nothing it can use.
pub fn main_space() -> Option<i64> {
    let out = Command::new("/usr/bin/defaults")
        .args(["export", "com.apple.spaces", "-"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    guard::main_space(&monitors(&out.stdout)?)
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

/// **What [`Poster`] asks and does**: every read of the desktop it decides
/// on, and every event it posts, so a test can hand it a desktop that changes
/// between two events.
pub trait Desk {
    fn console(&self) -> Console;
    fn windows(&self) -> Vec<Window>;
    fn front_app(&self) -> Option<i32>;
    fn key(&self, k: Key, down: bool) -> Result<(), String>;
    fn pointer(&self, at: (f64, f64)) -> Result<(), String>;
    fn wheel(&self, at: (f64, f64), lines: i32) -> Result<(), String>;
    /// The left button at `at`, pressed or let go, with `with` held.
    fn button(&self, at: (f64, f64), down: bool, with: Click) -> Result<(), String>;
    /// The pointer moved to `at` with the left button held.
    fn dragged(&self, at: (f64, f64)) -> Result<(), String>;
}

/// **What a click holds down beside its button**: nothing, or Command, set
/// on the button's events themselves as their flags, as a click made with
/// the key held carries them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Plain,
    Command,
}

/// **This desktop**, through the HID event tap, with the U.S. layout's key
/// codes.
pub struct Live {
    source: CFRetained<CGEventSource>,
}

impl Desk for Live {
    fn console(&self) -> Console {
        console()
    }

    fn windows(&self) -> Vec<Window> {
        windows(true)
    }

    fn front_app(&self) -> Option<i32> {
        front_app()
    }

    fn key(&self, k: Key, down: bool) -> Result<(), String> {
        let e = key_event(&self.source, k, down)?;
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e));
        Ok(())
    }

    fn pointer(&self, at: (f64, f64)) -> Result<(), String> {
        self.mouse(CGEventType::MouseMoved, at, Click::Plain)
    }

    fn button(&self, at: (f64, f64), down: bool, with: Click) -> Result<(), String> {
        let kind = match down {
            true => CGEventType::LeftMouseDown,
            false => CGEventType::LeftMouseUp,
        };
        self.mouse(kind, at, with)
    }

    fn dragged(&self, at: (f64, f64)) -> Result<(), String> {
        self.mouse(CGEventType::LeftMouseDragged, at, Click::Plain)
    }

    /// One wheel event, through `CGEventCreateScrollWheelEvent2`, the
    /// fixed-argument form (`[[source: macos-sdk-cgevent-h]]`).
    fn wheel(&self, at: (f64, f64), lines: i32) -> Result<(), String> {
        let e = CGEvent::new_scroll_wheel_event2(
            Some(&self.source),
            CGScrollEventUnit::Line,
            1,
            lines,
            0,
            0,
        )
        .ok_or("CoreGraphics made no wheel event")?;
        CGEvent::set_location(Some(&e), CGPoint { x: at.0, y: at.1 });
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e));
        Ok(())
    }
}

impl Live {
    /// One pointer event of `kind` at `at`, `with`'s key in its flags,
    /// through the HID event tap.
    fn mouse(&self, kind: CGEventType, at: (f64, f64), with: Click) -> Result<(), String> {
        let e = CGEvent::new_mouse_event(
            Some(&self.source),
            kind,
            CGPoint { x: at.0, y: at.1 },
            CGMouseButton::Left,
        )
        .ok_or("CoreGraphics made no pointer event")?;
        if with == Click::Command {
            CGEvent::set_flags(Some(&e), CGEventFlags::MaskCommand);
        }
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&e));
        Ok(())
    }
}

/// **The system's pasteboard, as a take keeps it**: what it holds read and
/// written back, and the count that says something wrote to it, so a test can
/// hand [`Kept`] a pasteboard no one else uses.
pub trait Board {
    type Saved;
    /// `changeCount`.
    fn count(&self) -> isize;
    /// What it holds, and the names of the types that could not be read.
    fn save(&self) -> (Self::Saved, Vec<String>);
    /// `saved` written back, and the names of the types it would not take.
    fn restore(&self, saved: &Self::Saved) -> Vec<String>;
}

/// **The general pasteboard**: every item, every type, each as its bytes, a
/// concealed type's as any other's. It holds them in memory and nowhere else.
pub struct General;

impl Board for General {
    type Saved = Vec<Vec<(Retained<NSString>, Retained<NSData>)>>;

    fn count(&self) -> isize {
        NSPasteboard::generalPasteboard().changeCount()
    }

    fn save(&self) -> (Self::Saved, Vec<String>) {
        let mut lost = Vec::new();
        let items = NSPasteboard::generalPasteboard().pasteboardItems();
        let saved = items.map_or(Vec::new(), |items| {
            items
                .iter()
                .map(|item| {
                    let mut kept = Vec::new();
                    for kind in item.types().iter() {
                        match item.dataForType(&kind) {
                            Some(data) => kept.push((kind, data)),
                            None => lost.push(kind.to_string()),
                        }
                    }
                    kept
                })
                .collect()
        });
        (saved, lost)
    }

    fn restore(&self, saved: &Self::Saved) -> Vec<String> {
        let mut lost = Vec::new();
        let writers: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = saved
            .iter()
            .map(|types| {
                let item = NSPasteboardItem::new();
                for (kind, data) in types {
                    if !item.setData_forType(data, kind) {
                        lost.push(kind.to_string());
                    }
                }
                ProtocolObject::from_retained(item)
            })
            .collect();
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        if !writers.is_empty() && !board.writeObjects(&NSArray::from_retained_slice(&writers)) {
            lost.push("every type: the pasteboard took no item".to_string());
        }
        lost
    }
}

/// How long a drag's copy may take to reach the pasteboard after its release.
const FILM_COPY_WAIT: Duration = Duration::from_millis(300);

/// **The pasteboard as it was before a take, put back when the take ends**,
/// on every way out, a refusal, an error and a panic included, by `Drop`: and
/// only if the last write is still the film's own, so anything a person
/// copied during the take survives (`guard::after_take`). What it holds it
/// holds in memory, is `Debug` to no one and is written to no log or file;
/// only the names of types it cannot restore are said.
pub struct Kept<B: Board> {
    board: B,
    saved: B::Saved,
    before: isize,
    own: Cell<isize>,
    unread: Vec<String>,
}

impl<B: Board> Kept<B> {
    /// What `board` holds now.
    pub fn of(board: B) -> Kept<B> {
        let before = board.count();
        let (saved, unread) = board.save();
        Kept {
            board,
            saved,
            before,
            own: Cell::new(before),
            unread,
        }
    }

    /// The count now, to hand [`Kept::film_copied`] after a gesture.
    pub fn count(&self) -> isize {
        self.board.count()
    }

    /// **A gesture of the film's that may have copied is over**: a count one
    /// past `before`, itself the film's own, within [`FILM_COPY_WAIT`], is the
    /// film's. Any other is left unclaimed, so what stands then stays.
    pub fn film_copied(&self, before: isize) {
        if before != self.own.get() {
            return;
        }
        let begun = std::time::Instant::now();
        while begun.elapsed() < FILM_COPY_WAIT {
            if self.board.count() == before + 1 {
                self.own.set(before + 1);
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// **The take is over**: what was done, in words that name no contents.
    fn finish(&self) -> Vec<String> {
        let now = self.board.count();
        let mut said = Vec::new();
        match guard::after_take(self.before, self.own.get(), now) {
            guard::AfterTake::Untouched => {}
            guard::AfterTake::Theirs => {
                said.push("something else copied during the take, so it stays".to_string());
            }
            guard::AfterTake::FilmsOwn => {
                let refused = self.board.restore(&self.saved);
                said.push("put back as it was before the take".to_string());
                for kind in self.unread.iter().chain(&refused) {
                    said.push(format!("{kind} could not be restored and is gone"));
                }
            }
        }
        said
    }
}

impl<B: Board> Drop for Kept<B> {
    fn drop(&mut self) {
        for line in self.finish() {
            println!("pasteboard: {line}");
        }
    }
}

/// **What posts keys and wheel turns into baud's window.** Every event is
/// guarded ([`guard::console`], then [`guard::keys`] or [`guard::wheel`]): the
/// first refusal stops the session having posted nothing more.
pub struct Poster<D> {
    pid: i32,
    desk: D,
    posted: Cell<usize>,
}

impl Poster<Live> {
    /// A poster for `pid`, only while this process is trusted.
    pub fn new(pid: i32) -> Result<Poster<Live>, String> {
        if !trusted() {
            return Err("this process is not trusted for Accessibility".to_string());
        }
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
            .ok_or("CoreGraphics made no event source")?;
        Ok(Poster {
            pid,
            desk: Live { source },
            posted: Cell::new(0),
        })
    }
}

impl<D: Desk> Poster<D> {
    fn refused(&self, why: guard::Refused) -> String {
        format!("REFUSED: {}; posted {} events", why.0, self.posted.get())
    }

    /// The session first, as nothing the window list says is about it.
    fn session_ok(&self) -> Result<(), String> {
        guard::console(&self.desk.console()).map_err(|r| self.refused(r))
    }

    /// One key, down and up, held `hold` between them; the guard asked
    /// before each.
    pub fn key(&self, k: Key, hold: Duration) -> Result<(), String> {
        for down in [true, false] {
            self.session_ok()?;
            guard::keys(self.pid, &self.desk.windows(), self.desk.front_app())
                .map_err(|r| self.refused(r))?;
            self.desk.key(k, down)?;
            if down {
                std::thread::sleep(hold);
            }
        }
        self.posted.set(self.posted.get() + 1);
        Ok(())
    }

    /// **The wheel at `at`**: the pointer moved there, then `turns` events
    /// of `lines` lines, the guard asked before every one.
    pub fn wheel(&self, at: (f64, f64), turns: i32, lines: i32) -> Result<(), String> {
        let check = || self.pointer_ok(at);
        check()?;
        self.desk.pointer(at)?;
        std::thread::sleep(Duration::from_millis(WHEEL_SETTLE_MS));
        for _ in 0..turns.unsigned_abs() {
            check()?;
            self.desk.wheel(at, lines * turns.signum())?;
            self.posted.set(self.posted.get() + 1);
            std::thread::sleep(Duration::from_millis(WHEEL_GAP_MS));
        }
        Ok(())
    }

    /// The session first, then that `at` is in a window of baud's with
    /// nothing in front of it: what every pointer event asks.
    fn pointer_ok(&self, at: (f64, f64)) -> Result<(), String> {
        self.session_ok()?;
        guard::wheel(self.pid, &self.desk.windows(), self.desk.front_app(), at)
            .map_err(|r| self.refused(r))
    }

    /// **The pointer rested at `at`**, the guard asked first.
    pub fn hover(&self, at: (f64, f64)) -> Result<(), String> {
        self.pointer_ok(at)?;
        self.desk.pointer(at)?;
        self.posted.set(self.posted.get() + 1);
        std::thread::sleep(Duration::from_millis(WHEEL_SETTLE_MS));
        Ok(())
    }

    /// **A drag from `from` to `to`**: the pointer moved there, the button
    /// pressed, [`DRAG_MOVES`] moves and the button let go, the guard asked
    /// before every one. A refusal posts nothing more, the release included:
    /// the press stands, as a refused key's up does, since what refused may
    /// be another application in front, which a release would land in.
    pub fn drag(&self, from: (f64, f64), to: (f64, f64)) -> Result<(), String> {
        self.pointer_ok(from)?;
        self.desk.pointer(from)?;
        std::thread::sleep(Duration::from_millis(DRAG_PAUSE_MS));
        self.pointer_ok(from)?;
        self.desk.button(from, true, Click::Plain)?;
        for i in 1..=DRAG_MOVES {
            let p = f64::from(i) / f64::from(DRAG_MOVES);
            let at = (from.0 + (to.0 - from.0) * p, from.1 + (to.1 - from.1) * p);
            std::thread::sleep(Duration::from_millis(DRAG_GAP_MS));
            self.pointer_ok(at)?;
            self.desk.dragged(at)?;
        }
        std::thread::sleep(Duration::from_millis(DRAG_PAUSE_MS));
        self.pointer_ok(to)?;
        self.desk.button(to, false, Click::Plain)?;
        self.posted.set(self.posted.get() + 1);
        Ok(())
    }

    /// **A Command-click at `at`**: the pointer moved there and let settle,
    /// Command pressed, the button pressed and let go with Command in their
    /// flags, and Command let go, the guard asked before every one. A
    /// refusal posts nothing more, the key's up included, as [`Self::drag`]
    /// leaves its press.
    pub fn cmd_click(&self, at: (f64, f64)) -> Result<(), String> {
        let settle = Duration::from_millis(WHEEL_SETTLE_MS);
        self.pointer_ok(at)?;
        self.desk.pointer(at)?;
        std::thread::sleep(settle);
        self.pointer_ok(at)?;
        self.desk.key(crate::keys::COMMAND, true)?;
        std::thread::sleep(settle);
        for down in [true, false] {
            self.pointer_ok(at)?;
            self.desk.button(at, down, Click::Command)?;
            std::thread::sleep(settle);
        }
        self.pointer_ok(at)?;
        self.desk.key(crate::keys::COMMAND, false)?;
        self.posted.set(self.posted.get() + 1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{
        COMMAND, COMMAND_NAME, DOWN, DOWN_NAME, ESCAPE, ESCAPE_NAME, RETURN, RETURN_NAME, UP,
        UP_NAME, US,
    };

    /// **Ctrl-C, a closed terminal and `SIGTERM` set the flag and the process
    /// lives**: the handler installed, each real signal raised against this
    /// process sets [`crate::container::INTERRUPTED`]. Without the handler the
    /// signal's default ends the test binary. The signals and `signal`'s type
    /// are `<signal.h>`'s, by a C probe that refuses to compile otherwise.
    #[test]
    fn ctrl_c_sets_the_flag_and_the_process_lives() {
        let src = format!(
            "#include <signal.h>\n\
             _Static_assert(SIGHUP == {SIGHUP}, \"SIGHUP\");\n\
             _Static_assert(SIGINT == {SIGINT}, \"SIGINT\");\n\
             _Static_assert(SIGTERM == {SIGTERM}, \"SIGTERM\");\n\
             _Static_assert(__builtin_types_compatible_p(__typeof__(signal), \
             void (*(int, void (*)(int)))(int)), \"signal's type\");\n\
             int main(void) {{ return 0; }}\n"
        );
        let dir = std::env::temp_dir().join(format!("baud-demo-sig-{}", std::process::id()));
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
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "the probe did not compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        extern "C" {
            fn raise(sig: i32) -> i32;
        }
        catch_interrupt();
        for sig in [SIGINT, SIGHUP, SIGTERM] {
            crate::container::INTERRUPTED.store(false, std::sync::atomic::Ordering::Relaxed);
            // SAFETY: `int raise(int)`; the handler it runs touches an atomic.
            assert_eq!(unsafe { raise(sig) }, 0);
            assert!(
                crate::container::INTERRUPTED.load(std::sync::atomic::Ordering::Relaxed),
                "{sig}"
            );
        }
    }

    /// **The key codes, and the trust call's type, are the SDK headers'
    /// own**: a C probe compiled against `Carbon.h` and
    /// `ApplicationServices.h` prints every `kVK_` name the U.S. table
    /// gives, `kVK_Return`, `kVK_Escape` and `sizeof(Boolean)`, and
    /// refuses to compile unless `AXIsProcessTrusted` is `Boolean (void)`.
    /// A name the header does not have fails the compile; a code that
    /// differs fails here. The control: the probe printed one value a line
    /// it was given.
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
        let names: Vec<&str> = US
            .iter()
            .map(|(_, n, _)| *n)
            .chain([RETURN_NAME, ESCAPE_NAME, UP_NAME, DOWN_NAME, COMMAND_NAME])
            .collect();
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
        assert_eq!(h[ESCAPE_NAME], i64::from(ESCAPE.code));
        assert_eq!(h[UP_NAME], i64::from(UP.code));
        assert_eq!(h[DOWN_NAME], i64::from(DOWN.code));
        assert_eq!(h[COMMAND_NAME], i64::from(COMMAND.code));
    }

    /// **The session's calls and the one key the crate has no constant for
    /// are the SDK headers' own**: a C probe refuses to compile unless each
    /// call has the type `mac.rs` declares, and prints
    /// `kCGSessionOnConsoleKey`'s spelling, `kIOMainPortDefault`'s size,
    /// `proc_name`'s longest name, `uid_t`'s size and where `pw_name` sits
    /// in `struct passwd`, first, and its size. The control: the probe printed one value
    /// a line it was given. Perturbation: spell [`ON_CONSOLE`] without its
    /// second `S`, and it fails here rather than reading as unread.
    #[test]
    fn the_session_calls_are_the_headers_own() {
        let src = r#"#include <stdio.h>
#include <libproc.h>
#include <sys/proc_info.h>
#include <Carbon/Carbon.h>
#include <CoreGraphics/CoreGraphics.h>
#include <IOKit/IOKitLib.h>
#include <pwd.h>
#include <stddef.h>
#include <unistd.h>
#define SAME(f, t) _Static_assert(__builtin_types_compatible_p(__typeof__(f), t), #f)
SAME(IsSecureEventInputEnabled, Boolean (void));
SAME(EnableSecureEventInput, OSStatus (void));
SAME(DisableSecureEventInput, OSStatus (void));
SAME(IORegistryGetRootEntry, io_registry_entry_t (mach_port_t));
SAME(IORegistryEntryCreateCFProperty,
     CFTypeRef (io_registry_entry_t, CFStringRef, CFAllocatorRef, IOOptionBits));
SAME(IOObjectRelease, kern_return_t (io_object_t));
SAME(proc_listallpids, int (void *, int));
SAME(proc_name, int (int, void *, uint32_t));
SAME(getuid, uid_t (void));
SAME(getpwuid, struct passwd *(uid_t));
int main(void) {
  char on[64];
  CFStringGetCString(kCGSessionOnConsoleKey, on, sizeof on, kCFStringEncodingUTF8);
  printf("on_console=%s\n", on);
  printf("entry=%d\n", (int)sizeof(io_registry_entry_t));
  printf("port=%d\n", (int)sizeof(kIOMainPortDefault));
  printf("options=%d\n", (int)sizeof(IOOptionBits));
  printf("name=%d\n", 2 * MAXCOMLEN + 1);
  printf("uid=%d\n", (int)sizeof(uid_t));
  printf("pw_name.at=%d\n", (int)offsetof(struct passwd, pw_name));
  printf("pw_name.size=%d\n", (int)sizeof(((struct passwd *)0)->pw_name));
  return 0;
}
"#;
        let dir = std::env::temp_dir().join(format!("baud-demo-session-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let (c, bin) = (dir.join("probe.c"), dir.join("probe"));
        std::fs::write(&c, src).expect("write the probe");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        let out = Command::new(&cc)
            .arg("-framework")
            .arg("CoreFoundation")
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
        let h: std::collections::HashMap<&str, &str> =
            text.lines().filter_map(|l| l.split_once('=')).collect();
        assert_eq!(h.len(), 8, "one value a line: {text}");
        assert_eq!(h["on_console"], ON_CONSOLE);
        assert_eq!(h["entry"], "4");
        assert_eq!(h["port"], "4");
        assert_eq!(h["options"], "4");
        assert_eq!(h["name"], "33");
        assert_eq!(h["uid"], std::mem::size_of::<Uid>().to_string());
        assert_eq!(
            h["pw_name.at"], "0",
            "pw_name is struct passwd's first member"
        );
        assert_eq!(h["pw_name.size"], std::mem::size_of::<Passwd>().to_string());
    }

    /// **The session's shell runs as the account, whatever login session
    /// started the demo**: [`account`] is the name `id -un -r` prints for
    /// this process's uid, and a zsh given [`with_account`]'s environment,
    /// the scratch env as the session gives it, reads `LOGNAME` and `USER`
    /// as that name. starship prints its user line when `LOGNAME` names
    /// another user than the one it runs as, which a zsh without the keys
    /// makes happen: it sets `LOGNAME` from `getlogin()`, and a demo started
    /// under another login session (an agent's, which reads `root`) put
    /// the account's name on every prompt of headless run 7. The control:
    /// a zsh told `LOGNAME=nobody` reads `nobody`, so the probe reads what
    /// the shell was given. Perturbation: return `env.to_vec()` from
    /// [`with_account`], and the keys are missing from the list, and under
    /// such a login session zsh reads its name instead.
    #[test]
    fn the_sessions_shell_runs_as_the_account() {
        let name = account().expect("this process's account");
        let id = Command::new("/usr/bin/id")
            .args(["-u", "-n", "-r"])
            .env_clear()
            .output()
            .expect("id");
        assert_eq!(String::from_utf8_lossy(&id.stdout).trim(), name);
        let at = crate::scratch::Layout {
            root: std::path::PathBuf::from("/t/r"),
        };
        let env = with_account(&crate::scratch::env(&at, "/usr/bin:/bin"), &name);
        for k in ["LOGNAME", "USER"] {
            assert!(
                env.iter().any(|(key, v)| key == k && *v == name),
                "{k} in {env:?}"
            );
        }
        let zsh = |env: &[(String, String)]| {
            let out = Command::new("/bin/zsh")
                .args(["-f", "-c", "print -r -- \"$LOGNAME|$USER\""])
                .env_clear()
                .envs(env.iter().map(|(k, v)| (k, v)))
                .output()
                .expect("zsh");
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        assert_eq!(zsh(&env), format!("{name}|{name}"));
        let told: Vec<(String, String)> = env
            .iter()
            .map(|(k, v)| match k.as_str() {
                "LOGNAME" => (k.clone(), "nobody".to_string()),
                _ => (k.clone(), v.clone()),
            })
            .collect();
        assert_eq!(zsh(&told), format!("nobody|{name}"), "the control");
    }

    /// **A desktop whose session can change between two events**, and the
    /// order things were asked and done in.
    struct Fake {
        /// The read from which the screen is locked.
        locked_from: usize,
        reads: Cell<usize>,
        front: i32,
        did: std::cell::RefCell<Vec<Did>>,
    }

    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    enum Did {
        Read,
        Down,
        Up,
        Pointer,
        Wheel,
        Press,
        Release,
        CommandPress,
        CommandRelease,
        Dragged,
    }
    use Did::{
        CommandPress, CommandRelease, Down, Dragged, Pointer, Press, Read, Release, Up, Wheel,
    };

    const BAUD: i32 = 500;

    impl Fake {
        fn locked_from(read: usize) -> Poster<Fake> {
            Poster {
                pid: BAUD,
                desk: Fake {
                    locked_from: read,
                    reads: Cell::new(0),
                    front: BAUD,
                    did: Default::default(),
                },
                posted: Cell::new(0),
            }
        }
    }

    impl Desk for Fake {
        fn console(&self) -> Console {
            let n = self.reads.get();
            self.reads.set(n + 1);
            self.did.borrow_mut().push(Read);
            Console {
                registry_locked: if n >= self.locked_from {
                    Reading::Yes
                } else {
                    Reading::No
                },
                session_locked: Reading::No,
                on_console: Reading::Yes,
                secure_input: Reading::No,
                display_asleep: Reading::No,
                display_active: Reading::Yes,
                saver: Reading::No,
            }
        }

        fn windows(&self) -> Vec<Window> {
            vec![Window {
                owner: self.front,
                owner_name: "front".to_string(),
                layer: 0,
                on_screen: OnScreen::Yes,
                bounds: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
            }]
        }

        fn front_app(&self) -> Option<i32> {
            Some(self.front)
        }

        fn key(&self, _: Key, down: bool) -> Result<(), String> {
            self.did.borrow_mut().push(if down { Down } else { Up });
            Ok(())
        }

        fn pointer(&self, _: (f64, f64)) -> Result<(), String> {
            self.did.borrow_mut().push(Pointer);
            Ok(())
        }

        fn wheel(&self, _: (f64, f64), _: i32) -> Result<(), String> {
            self.did.borrow_mut().push(Wheel);
            Ok(())
        }

        fn button(&self, _: (f64, f64), down: bool, with: Click) -> Result<(), String> {
            self.did.borrow_mut().push(match (down, with) {
                (true, Click::Plain) => Press,
                (false, Click::Plain) => Release,
                (true, Click::Command) => CommandPress,
                (false, Click::Command) => CommandRelease,
            });
            Ok(())
        }

        fn dragged(&self, _: (f64, f64)) -> Result<(), String> {
            self.did.borrow_mut().push(Dragged);
            Ok(())
        }
    }

    const FOREVER: usize = usize::MAX;

    /// **The session is read before a key's down and again before its up**,
    /// and a screen that locks between them stops the up. Perturbation:
    /// take the read out of `key`, or read once outside its loop, and the
    /// order differs.
    #[test]
    fn every_key_event_reads_the_session_first() {
        let r = crate::keys::key_for('R').unwrap();
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.key(r, Duration::ZERO), Ok(()));
        assert_eq!(*p.desk.did.borrow(), [Read, Down, Read, Up]);

        let p = Fake::locked_from(0);
        let said = p.key(r, Duration::ZERO).unwrap_err();
        assert!(said.contains("the screen is locked"), "{said}");
        assert!(said.contains("posted 0 events"), "{said}");
        assert_eq!(*p.desk.did.borrow(), [Read], "nothing was posted");

        let p = Fake::locked_from(1);
        assert!(p.key(r, Duration::ZERO).is_err());
        assert_eq!(*p.desk.did.borrow(), [Read, Down, Read], "no up");
    }

    /// **Every wheel event reads the session first**, the pointer's move
    /// among them, and a screen that locks after the second turn leaves
    /// exactly two posted. Perturbation: take the read out of `wheel`'s
    /// check and the order differs.
    #[test]
    fn every_wheel_event_reads_the_session_first() {
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.wheel((50.0, 50.0), 3, 1), Ok(()));
        assert_eq!(
            *p.desk.did.borrow(),
            [Read, Pointer, Read, Wheel, Read, Wheel, Read, Wheel]
        );

        let p = Fake::locked_from(0);
        assert!(p.wheel((50.0, 50.0), 3, 1).is_err());
        assert_eq!(*p.desk.did.borrow(), [Read], "the pointer did not move");

        let p = Fake::locked_from(3);
        let said = p.wheel((50.0, 50.0), 3, 1).unwrap_err();
        assert!(said.contains("posted 2 events"), "{said}");
        assert_eq!(
            *p.desk.did.borrow(),
            [Read, Pointer, Read, Wheel, Read, Wheel, Read]
        );
    }

    /// **Every hover event reads the session first**, and a locked screen
    /// posts nothing. Perturbation: take `pointer_ok` out of `hover`.
    #[test]
    fn every_hover_event_reads_the_session_first() {
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.hover((50.0, 50.0)), Ok(()));
        assert_eq!(*p.desk.did.borrow(), [Read, Pointer]);

        let p = Fake::locked_from(0);
        assert!(p.hover((50.0, 50.0)).is_err());
        assert_eq!(*p.desk.did.borrow(), [Read], "the pointer did not move");
    }

    /// **Every drag event reads the session first**, the press, each move
    /// and the release, and a screen that locks partway posts nothing more,
    /// the release included. Perturbation: read once outside the moves' loop;
    /// post the release on a refusal.
    #[test]
    fn every_drag_event_reads_the_session_first() {
        let (from, to) = ((10.0, 10.0), (60.0, 60.0));
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.drag(from, to), Ok(()));
        let mut want = vec![Read, Pointer, Read, Press];
        for _ in 0..DRAG_MOVES {
            want.extend([Read, Dragged]);
        }
        want.extend([Read, Release]);
        assert_eq!(*p.desk.did.borrow(), want);

        let p = Fake::locked_from(1);
        assert!(p.drag(from, to).is_err());
        assert_eq!(*p.desk.did.borrow(), [Read, Pointer, Read], "no press");

        let p = Fake::locked_from(3);
        let said = p.drag(from, to).unwrap_err();
        assert!(said.contains("posted 0 events"), "{said}");
        assert_eq!(
            *p.desk.did.borrow(),
            [Read, Pointer, Read, Press, Read, Dragged, Read],
            "no release"
        );
    }

    /// **A Command-click is Command down, the button down and up with
    /// Command in their flags, and Command up, each event read first**, the
    /// pointer moved there before; and a screen that locks partway posts
    /// nothing more, Command's up included. The control: a drag's press and
    /// release carry no Command. Perturbation: post the button with
    /// `Click::Plain` and the order differs.
    #[test]
    fn a_command_click_holds_command_over_the_button() {
        let at = (40.0, 40.0);
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.cmd_click(at), Ok(()));
        assert_eq!(
            *p.desk.did.borrow(),
            [
                Read,
                Pointer,
                Read,
                Down,
                Read,
                CommandPress,
                Read,
                CommandRelease,
                Read,
                Up
            ]
        );
        let p = Fake::locked_from(3);
        assert!(p.cmd_click(at).is_err());
        assert_eq!(
            *p.desk.did.borrow(),
            [Read, Pointer, Read, Down, Read, CommandPress, Read],
            "no release, no Command up"
        );
        let p = Fake::locked_from(FOREVER);
        assert_eq!(p.drag(at, (60.0, 60.0)), Ok(()));
        let did = p.desk.did.borrow();
        assert!(
            did.contains(&Press) && did.contains(&Release),
            "the control"
        );
        assert!(!did.contains(&CommandPress));
    }

    /// **A pasteboard no one else uses**, whose writes a test makes: what it
    /// holds, its count, and each restore.
    struct FakeBoard {
        held: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
        count: std::rc::Rc<Cell<isize>>,
        refuses: Vec<String>,
    }

    impl Board for FakeBoard {
        type Saved = Vec<u8>;

        fn count(&self) -> isize {
            self.count.get()
        }

        fn save(&self) -> (Vec<u8>, Vec<String>) {
            (self.held.borrow().clone(), Vec::new())
        }

        fn restore(&self, saved: &Vec<u8>) -> Vec<String> {
            *self.held.borrow_mut() = saved.clone();
            self.count.set(self.count.get() + 1);
            self.refuses.clone()
        }
    }

    type Shared = (
        std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
        std::rc::Rc<Cell<isize>>,
    );

    fn kept(refuses: &[&str]) -> (Kept<FakeBoard>, Shared) {
        let held = std::rc::Rc::new(std::cell::RefCell::new(b"secret".to_vec()));
        let count = std::rc::Rc::new(Cell::new(7));
        let board = FakeBoard {
            held: held.clone(),
            count: count.clone(),
            refuses: refuses.iter().map(|s| (*s).to_string()).collect(),
        };
        (Kept::of(board), (held, count))
    }

    fn write(shared: &Shared, what: &[u8]) {
        *shared.0.borrow_mut() = what.to_vec();
        shared.1.set(shared.1.get() + 1);
    }

    /// **The film's own copy goes back, on every way out**: dropped without a
    /// word from the take, as a refusal or a panic drops it. Perturbation:
    /// take the `Drop` away, so an early return leaves the film's copy.
    #[test]
    fn the_films_copy_is_put_back_however_the_take_ends() {
        let (k, shared) = kept(&[]);
        let before = k.count();
        write(&shared, b"the film's");
        k.film_copied(before);
        drop(k);
        assert_eq!(*shared.0.borrow(), b"secret");
        let (k, shared) = kept(&[]);
        let before = k.count();
        write(&shared, b"the film's");
        k.film_copied(before);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _k = k;
            panic!("the take went wrong");
        }));
        assert!(unwound.is_err());
        assert_eq!(*shared.0.borrow(), b"secret", "through a panic too");
    }

    /// **What a person copied survives**, before the film's copy or after,
    /// and a pasteboard nothing wrote to is not written. Perturbation: restore
    /// on any change of the count.
    #[test]
    fn a_persons_copy_survives_the_take() {
        let (k, shared) = kept(&[]);
        write(&shared, b"theirs");
        assert_eq!(k.finish().len(), 1, "it says so");
        drop(k);
        assert_eq!(*shared.0.borrow(), b"theirs", "no film copy at all");

        let (k, shared) = kept(&[]);
        let before = k.count();
        write(&shared, b"the film's");
        k.film_copied(before);
        write(&shared, b"theirs");
        drop(k);
        assert_eq!(*shared.0.borrow(), b"theirs", "after the film's");

        let (k, shared) = kept(&[]);
        let before = k.count();
        write(&shared, b"theirs");
        k.film_copied(before + 1);
        drop(k);
        assert_eq!(*shared.0.borrow(), b"theirs", "before a gesture");

        let (k, shared) = kept(&[]);
        assert!(k.finish().is_empty(), "nothing wrote, nothing said");
        assert_eq!(shared.1.get(), 7, "and nothing written");
    }

    /// **A type that cannot go back is said, by name, and no contents are.**
    /// Perturbation: put the saved bytes in the line.
    #[test]
    fn a_type_that_cannot_go_back_is_said_and_no_contents_are() {
        let (k, shared) = kept(&["public.tiff"]);
        let before = k.count();
        write(&shared, b"the film's");
        k.film_copied(before);
        let said = k.finish().join("\n");
        assert!(said.contains("public.tiff"), "{said}");
        assert!(!said.contains("secret"), "{said}");
    }

    /// **The session's read is added to the window's, not put in its place**:
    /// a session all clear with another application in front still posts
    /// nothing: a key, a wheel, a hover and a drag each read the session
    /// once and are refused there. Perturbation: drop `guard::keys` from
    /// `key`, or `guard::wheel` from `pointer_ok`.
    #[test]
    fn the_window_guard_still_stands() {
        let mut p = Fake::locked_from(FOREVER);
        p.desk.front = 77;
        let r = crate::keys::key_for('R').unwrap();
        assert!(p.key(r, Duration::ZERO).unwrap_err().contains("pid 77's"));
        assert!(
            p.wheel((50.0, 50.0), 1, 1)
                .unwrap_err()
                .contains("pid 77's")
        );
        assert!(p.hover((50.0, 50.0)).unwrap_err().contains("pid 77's"));
        assert!(
            p.drag((10.0, 10.0), (50.0, 50.0))
                .unwrap_err()
                .contains("pid 77's")
        );
        assert_eq!(*p.desk.did.borrow(), [Read, Read, Read, Read]);
    }

    /// **The process search finds a name past 16 characters, exactly**: this
    /// test's own binary, `demo-` and a hash, stands in for the 17 characters
    /// of `ScreenSaverEngine`, which cannot be started here. Cut to 16 it is
    /// not found, a name no process has is not found. The control is the
    /// finding. Perturbation: a buffer of 17 for `proc_name` and the first
    /// assertion fails.
    #[test]
    fn the_process_search_finds_a_long_name_exactly() {
        let exe = std::env::current_exe().expect("this test's path");
        let own = exe.file_name().expect("a name").to_string_lossy();
        assert!(own.len() > 16 && own.len() <= 32, "{own}");
        assert_eq!(process_named(&own), Reading::Yes, "{own}");
        assert_eq!(process_named(&own[..16]), Reading::No, "{own}");
        assert_eq!(process_named("baud-demo-no-such-process"), Reading::No);
    }

    /// **A boolean property reads as itself and anything else as no answer**,
    /// the branch `IOConsoleLocked` absent, a session's on-console key absent
    /// or either of another type takes. Perturbation: read `None` as `No`.
    #[test]
    fn a_property_that_is_no_boolean_is_no_answer() {
        assert_eq!(boolean(None), Reading::Unread);
        let any = |v: &CFType| Some(v.retain());
        assert_eq!(boolean(any(CFBoolean::new(true))), Reading::Yes);
        assert_eq!(boolean(any(CFBoolean::new(false))), Reading::No);
        let text = CFString::from_static_str("true");
        assert_eq!(boolean(any(&text)), Reading::Unread);
    }

    /// **The session dictionary: the lock's key absent is not locked, its
    /// on-console key absent is no answer, and no dictionary at all answers
    /// neither.** The first row is what an unlocked Mac reads, so reading
    /// the absent lock as no answer would refuse every take. Perturbation:
    /// read the absent lock through `boolean` and that row is `Unread`; read
    /// no dictionary as `No` and the last row passes.
    #[test]
    fn the_session_dictionary_reads_as_its_keys_say() {
        let of = |pairs: &[(&'static str, &CFType)]| {
            let keys: Vec<CFRetained<CFString>> = pairs
                .iter()
                .map(|(k, _)| CFString::from_static_str(k))
                .collect();
            let keys: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
            let values: Vec<&CFType> = pairs.iter().map(|(_, v)| *v).collect();
            let d = CFDictionary::<CFString, CFType>::from_slices(&keys, &values);
            // SAFETY: from a dictionary to itself, its types forgotten.
            session(Some(unsafe {
                CFRetained::cast_unchecked::<CFDictionary>(d)
            }))
        };
        let (yes, no): (&CFType, &CFType) = (CFBoolean::new(true), CFBoolean::new(false));
        let text = CFString::from_static_str("true");
        let (y, n, u) = (Reading::Yes, Reading::No, Reading::Unread);
        assert_eq!(of(&[(ON_CONSOLE, yes)]), (n, y));
        assert_eq!(of(&[(ON_CONSOLE, no)]), (n, n));
        assert_eq!(of(&[(ON_CONSOLE, yes), (SCREEN_IS_LOCKED, yes)]), (y, y));
        assert_eq!(of(&[(ON_CONSOLE, yes), (SCREEN_IS_LOCKED, no)]), (n, y));
        assert_eq!(of(&[(ON_CONSOLE, yes), (SCREEN_IS_LOCKED, &text)]), (u, y));
        assert_eq!(of(&[(ON_CONSOLE, &text)]), (n, u));
        assert_eq!(of(&[]), (n, u));
        assert_eq!(session(None), (u, u));
    }

    /// **No main display is no answer**, whatever the call about it said.
    /// Perturbation: drop the first arm and display 0 reads asleep or not.
    #[test]
    fn no_main_display_is_no_answer() {
        assert_eq!(display(1, true), Reading::Yes);
        assert_eq!(display(1, false), Reading::No);
        assert_eq!(display(0, true), Reading::Unread);
        assert_eq!(display(0, false), Reading::Unread);
    }

    /// `com.apple.spaces` in the shape `defaults export` writes it, with
    /// invented identifiers: the main display's `Current Space` of `type`
    /// `$CURRENT`, its `Spaces` of 0 and 4, and a display out of use, which
    /// has only a `Collapsed Space`.
    const SPACES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>SpacesDisplayConfiguration</key><dict>
<key>Management Data</key><dict>
<key>Management Mode</key><integer>1</integer>
<key>Monitors</key><array>
<dict>
<key>Current Space</key><dict><key>id64</key><integer>1</integer><key>type</key><integer>$CURRENT</integer></dict>
<key>Display Identifier</key><string>Main</string>
<key>Spaces</key><array>
<dict><key>id64</key><integer>1</integer><key>type</key><integer>0</integer></dict>
<dict><key>id64</key><integer>2</integer><key>type</key><integer>4</integer></dict>
</array>
</dict>
<dict>
<key>Collapsed Space</key><dict><key>id64</key><integer>3</integer><key>type</key><integer>0</integer></dict>
<key>Display Identifier</key><string>00000000-0000-0000-0000-000000000000</string>
</dict>
</array>
</dict></dict></dict></plist>
"#;

    /// **A desktop with full-screen Spaces beside it reads as a desktop, and
    /// a full-screen current Space as one**: `Current Space` is read and not
    /// the `Spaces` list. What is no such property list reads as nothing.
    /// Perturbation: read `Spaces`'s last rather than `Current Space` and
    /// the first case reads 4.
    #[test]
    fn the_spaces_preference_is_read_for_the_current_space() {
        let read = |current: &str| {
            let text = SPACES.replace("$CURRENT", current);
            monitors(text.as_bytes()).expect("the file's shape")
        };
        let on_desk = read("0");
        assert_eq!(on_desk.len(), 2);
        assert_eq!(on_desk[1].current, None, "a display out of use");
        assert_eq!(guard::main_space(&on_desk), Some(0));
        assert_eq!(guard::space(guard::main_space(&on_desk)), Ok(()));
        let on_full = read("4");
        assert_eq!(guard::main_space(&on_full), Some(guard::FULL_SCREEN_SPACE));
        assert!(guard::space(guard::main_space(&on_full)).is_err());
        assert_eq!(monitors(b"not a property list"), None);
        assert_eq!(
            monitors(b"<plist version=\"1.0\"><dict></dict></plist>"),
            None
        );
    }

    /// **What the real calls read on this Mac, and how long**, printed and
    /// asserted of nothing, for it is whatever this Mac is in: a default test
    /// that said "not locked" would go red with a password manager open. Run
    /// by hand: `cargo test -p baud --example demo -- --ignored --nocapture
    /// the_real_readings`.
    #[test]
    #[ignore = "reads this Mac's session and asserts nothing; for a record"]
    fn the_real_readings() {
        let c = console();
        println!("{c:?}");
        println!("session: {}", guard::describe(&c));
        println!("main Space type: {:?}", main_space());
        let mut each: Vec<Duration> = (0..200)
            .map(|_| {
                let t = std::time::Instant::now();
                let _ = console();
                t.elapsed()
            })
            .collect();
        each.sort();
        println!(
            "console(), 200 reads: median {:?}, slowest {:?}",
            each[100], each[199]
        );
    }

    /// The variable that makes [`secure_input_turned_on_is_refused`] the
    /// child that turns it on.
    const SECURE_CHILD: &str = "BAUD_DEMO_SECURE_INPUT_CHILD";

    /// **The positive control of the secure-input check**: a child process
    /// turns secure input on, and through this file's own extern and
    /// [`console`] and [`guard::console`] it reads on and is refused for it;
    /// off again, it reads as it did before. Secure input is the whole
    /// Mac's, so this is never a default test: every run of the suite would
    /// turn it on for whatever else is running. Run by hand:
    /// `cargo test -p baud --example demo -- --ignored --exact
    /// mac::tests::secure_input_turned_on_is_refused`.
    ///
    /// It is a child because the state goes when a process does: "If your
    /// application crashes, secure event input will automatically be
    /// disabled if no other application has enabled it"
    /// (`[[source: macos-sdk-carboneventscore-h]]`). A guard turns it off
    /// on every path that unwinds.
    #[test]
    #[ignore = "turns secure input on for the whole Mac, for an instant"]
    fn secure_input_turned_on_is_refused() {
        extern "C" {
            fn EnableSecureEventInput() -> i32;
            fn DisableSecureEventInput() -> i32;
        }
        // SAFETY: each takes nothing and returns an `OSStatus`, the probe's
        // types, and the extern above takes nothing and returns a `Boolean`.
        let on = || unsafe { IsSecureEventInputEnabled() != 0 };
        if std::env::var_os(SECURE_CHILD).is_some() {
            struct Off;
            impl Drop for Off {
                fn drop(&mut self) {
                    // SAFETY: as above; one disable for the one enable.
                    unsafe { DisableSecureEventInput() };
                }
            }
            // SAFETY: as above.
            assert_eq!(unsafe { EnableSecureEventInput() }, 0, "noErr");
            let off = Off;
            let seen = on();
            let read = console();
            let said = guard::console(&read);
            drop(off);
            assert!(seen, "turned on, it reads off");
            assert_eq!(read.secure_input, Reading::Yes);
            let why = said.expect_err("refused").0;
            println!("refused: {why}");
            assert!(why.contains("secure input is on"), "{why}");
            return;
        }
        let before = on();
        assert!(
            !before,
            "something holds secure input now, so this cannot tell on from off"
        );
        let out = Command::new(std::env::current_exe().expect("this test"))
            .args(["--ignored", "--exact", "--nocapture"])
            .arg("mac::tests::secure_input_turned_on_is_refused")
            .env(SECURE_CHILD, "1")
            .output()
            .expect("the child");
        let text = String::from_utf8_lossy(&out.stdout);
        println!("{text}");
        assert!(out.status.success(), "{text}");
        assert!(text.contains("1 passed"), "the child ran it: {text}");
        assert_eq!(on(), before, "the child's exit left it as it was");
    }

    /// **Ctrl-d's event, not posted, carries Control and no text of its
    /// own**, which `NSEvent` fills in as a real press has it (`key_event`);
    /// `R` keeps the text it was given. Nothing is posted and no window is
    /// asked for. The
    /// control: an event left at no flags has none of Control. Perturbation:
    /// map `Held::Control` to no flag and the first assertion fails.
    #[test]
    fn a_control_key_leaves_its_text_to_the_system() {
        let source = CGEventSource::new(CGEventSourceStateID::Private).expect("an event source");
        let text = |e: &CGEvent| {
            let mut units = [0u16; 4];
            let mut n = 0;
            // SAFETY: the two pointers are this frame's, the array holds the
            // four units the call is told of, and it writes no more.
            unsafe {
                CGEvent::keyboard_get_unicode_string(Some(e), 4, &mut n, units.as_mut_ptr());
            }
            String::from_utf16_lossy(&units[..n as usize])
        };
        let control = |e: &CGEvent| CGEvent::flags(Some(e)).contains(CGEventFlags::MaskControl);
        let d = crate::keys::control_key('d').unwrap();
        let e = key_event(&source, d, true).unwrap();
        assert!(control(&e), "{:?}", CGEvent::flags(Some(&e)));
        assert_eq!(text(&e), "", "the system's to fill in");
        let r = crate::keys::key_for('R').unwrap();
        let e = key_event(&source, r, true).unwrap();
        assert!(!control(&e), "the control");
        assert_eq!(text(&e), "R");
    }
}
