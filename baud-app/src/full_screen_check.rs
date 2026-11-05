//! **The window's full screen read at rest, for the README's filming run
//! alone.** The grid's top in full screen (`strip.rs`) rests on what AppKit
//! does with the title bar there, and a window enters full screen only
//! when it is shown, so no test can read it. The demo's `--record` run
//! shows the window anyway; once the film has its last frame, this reads
//! the window through one full screen and back and writes what it read.
//!
//! **Armed by [`ENV`] alone**, which names a directory: the demo sets it on
//! the filming run and on no other, and `--headless` opens no window. The
//! directory is watched by the kernel -- a dispatch source of type
//! `DISPATCH_SOURCE_TYPE_VNODE`, "a file descriptor for events defined by
//! dispatch_source_vnode_flags_t", with `DISPATCH_VNODE_WRITE`, "The
//! filesystem object data changed" (`[[source:
//! macos-sdk-dispatch-source-h]]` 192-198, 294-295, 317), on a descriptor
//! opened `O_EVTONLY` -- so the demo starts it by making [`GO`] there, and
//! nothing polls. No signal: a signal ignored or blocked here would be
//! ignored or blocked in every program the shell starts.
//!
//! **What it does is the window's own**: `toggleFullScreen:`, never a key,
//! a click or the pointer; at the "did enter" notification
//! (`baud_present_macos::full_screen`) a reading; [`REST`] later, the bar
//! having had time to hide, a second; then `toggleFullScreen:` back, and at
//! the "did exit" a third, and [`OUT`] written with all three and the menu
//! bar's defaults. Each step is scheduled on the main queue, none waited
//! for.
//!
//! **A reading** is the lockup's and the lights' accessory views -- hidden
//! or not, and whether they are in the window or in another, the detached
//! full-screen bar, and that window's visibility and alpha; the terminal's
//! view's frame in its container, and whether its top is the container's;
//! the window's content layout rect and its content view's height; and
//! whether the window's style mask says full screen. The defaults are read
//! as `NSUserDefaults` answers them, each named key's value or `null`:
//! `AppleMenuBarVisibleInFullscreen` and `_HIHideMenuBar`, so the run says
//! which case of the menu bar it covered, and `AppleLanguages`, a key every
//! account has, as the positive control that the reading works -- only
//! whether it was found, not what it holds.
//!
//! No `crate::` path, so `baud-app/tests/titlebar_mark.rs` takes this file
//! in by `#[path]` and drives [`Check`] in a window it never shows,
//! through the two notifications posted by hand.
#![allow(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use baud_present_macos::full_screen::Change;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2_app_kit::{NSView, NSWindow, NSWindowStyleMask};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{NSRect, NSString, NSUserDefaults};

/// The variable that arms the check: the directory it is started from and
/// writes to.
pub(crate) const ENV: &str = "BAUD_FULL_SCREEN_CHECK";

/// The file whose making in that directory starts it.
pub(crate) const GO: &str = "full-screen.go";

/// The file it writes there.
pub(crate) const OUT: &str = "full-screen.json";

/// How long the window is left in full screen before the reading at rest:
/// the bar AppKit shows on entering has hidden by then.
pub(crate) const REST: Duration = Duration::from_secs(2);

/// The defaults read, the last the positive control.
pub(crate) const KEYS: [&str; 3] = [
    "AppleMenuBarVisibleInFullscreen",
    "_HIHideMenuBar",
    "AppleLanguages",
];

/// `DISPATCH_VNODE_WRITE`, `[[source: macos-sdk-dispatch-source-h]]` 317.
const DISPATCH_VNODE_WRITE: usize = 0x2;

/// **What a reading looks at**: the window, the terminal's view and the
/// container it fills, and the two accessory views where there are any.
pub(crate) struct Parts {
    pub(crate) window: Retained<NSWindow>,
    pub(crate) view: Retained<NSView>,
    pub(crate) container: Retained<NSView>,
    pub(crate) lockup: Option<Retained<NSView>>,
    pub(crate) lights: Option<Retained<NSView>>,
}

/// An accessory view as read.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Accessory {
    /// `isHiddenOrHasHiddenAncestor`.
    pub(crate) hidden: bool,
    /// Whether its window is the terminal's window.
    pub(crate) in_window: bool,
    /// Whether the window it is in is visible, and its alpha.
    pub(crate) bar_visible: bool,
    pub(crate) bar_alpha: CGFloat,
}

/// **One reading of the window.**
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Reading {
    pub(crate) style_full_screen: bool,
    pub(crate) view: NSRect,
    pub(crate) container_height: CGFloat,
    pub(crate) layout: NSRect,
    pub(crate) content_height: CGFloat,
    pub(crate) lockup: Option<Accessory>,
    pub(crate) lights: Option<Accessory>,
}

impl Reading {
    /// Whether the terminal's view reaches its container's top, to half a
    /// point.
    pub(crate) fn at_top(&self) -> bool {
        (self.view.origin.y + self.view.size.height - self.container_height).abs() < 0.5
    }
}

fn accessory(v: &NSView, window: &NSWindow) -> Accessory {
    let bar = v.window();
    Accessory {
        hidden: v.isHiddenOrHasHiddenAncestor(),
        in_window: bar.as_deref().is_some_and(|w| std::ptr::eq(w, window)),
        bar_visible: bar.as_deref().is_some_and(NSWindow::isVisible),
        bar_alpha: bar.as_deref().map_or(0.0, NSWindow::alphaValue),
    }
}

/// **The window read now.**
pub(crate) fn read(p: &Parts) -> Reading {
    let w = &*p.window;
    Reading {
        style_full_screen: w.styleMask().contains(NSWindowStyleMask::FullScreen),
        view: p.view.frame(),
        container_height: p.container.frame().size.height,
        layout: w.contentLayoutRect(),
        content_height: w
            .contentView()
            .map_or(w.frame().size.height, |v| v.frame().size.height),
        lockup: p.lockup.as_deref().map(|v| accessory(v, w)),
        lights: p.lights.as_deref().map(|v| accessory(v, w)),
    }
}

/// **Each of [`KEYS`] as `NSUserDefaults` answers it**: its value as an
/// integer, which a boolean reads as 0 or 1, `None` where the key is
/// absent. The positive control's
/// value is not kept, only that it was found.
pub(crate) fn defaults() -> Vec<(&'static str, Option<String>)> {
    let d = NSUserDefaults::standardUserDefaults();
    KEYS.iter()
        .enumerate()
        .map(|(i, k)| {
            let key = NSString::from_str(k);
            let said = d.objectForKey(&key).map(|_| match i {
                2 => "found".to_string(),
                _ => d.integerForKey(&key).to_string(),
            });
            (*k, said)
        })
        .collect()
}

fn rect(r: NSRect) -> String {
    format!(
        "{{\"x\": {}, \"y\": {}, \"width\": {}, \"height\": {}}}",
        r.origin.x, r.origin.y, r.size.width, r.size.height
    )
}

fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn accessory_json(a: Option<Accessory>) -> String {
    match a {
        None => "null".to_string(),
        Some(a) => format!(
            "{{\"hidden\": {}, \"in_window\": {}, \"bar_visible\": {}, \"bar_alpha\": {}}}",
            a.hidden, a.in_window, a.bar_visible, a.bar_alpha
        ),
    }
}

fn reading_json(r: &Reading) -> String {
    format!(
        "{{\"style_full_screen\": {}, \"view\": {}, \"container_height\": {}, \"view_at_top\": {}, \
         \"content_layout_rect\": {}, \"content_height\": {}, \"lockup\": {}, \"lights\": {}}}",
        r.style_full_screen,
        rect(r.view),
        r.container_height,
        r.at_top(),
        rect(r.layout),
        r.content_height,
        accessory_json(r.lockup),
        accessory_json(r.lights)
    )
}

/// **The file [`OUT`] holds**: the three readings and the defaults.
pub(crate) fn json(
    entered: &Reading,
    at_rest: &Reading,
    exited: &Reading,
    defaults: &[(&str, Option<String>)],
) -> String {
    let d: Vec<String> = defaults
        .iter()
        .map(|(k, v)| {
            format!(
                "    {}: {}",
                quoted(k),
                v.as_deref().map_or("null".to_string(), quoted)
            )
        })
        .collect();
    format!(
        "{{\n  \"entered\": {},\n  \"at_rest\": {},\n  \"exited\": {},\n  \"defaults\": {{\n{}\n  }}\n}}\n",
        reading_json(entered),
        reading_json(at_rest),
        reading_json(exited),
        d.join(",\n")
    )
}

/// Where the check is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Step {
    /// Waiting for [`GO`].
    Armed,
    /// Asked into full screen, waiting for "did enter".
    Entering,
    /// In full screen, waiting [`REST`].
    Resting,
    /// Asked out, waiting for "did exit".
    Leaving,
    /// Written, or given up.
    Done,
}

/// What the glue does next.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum Act {
    Nothing,
    /// `toggleFullScreen:`, on a later turn of the main queue.
    Toggle,
    /// Read again after this long.
    ReadAfter(Duration),
    /// Write the three readings: entered, at rest, exited.
    Write(Box<[Reading; 3]>),
}

/// **The check's steps**, each answering what to do next; the glue
/// ([`arm`]) does it, and a test drives it with notifications posted by
/// hand.
pub(crate) struct Check {
    step: Step,
    entered: Option<Reading>,
    at_rest: Option<Reading>,
}

impl Check {
    pub(crate) fn new() -> Check {
        Check {
            step: Step::Armed,
            entered: None,
            at_rest: None,
        }
    }

    pub(crate) fn step(&self) -> Step {
        self.step
    }

    /// [`GO`] was made: into full screen, once.
    pub(crate) fn go(&mut self) -> Act {
        match self.step {
            Step::Armed => {
                self.step = Step::Entering;
                Act::Toggle
            }
            _ => Act::Nothing,
        }
    }

    /// A change of the window's full screen, and the window read at it.
    /// Only the change each step waits for moves it on; a change the check
    /// did not ask for is not taken as its own.
    pub(crate) fn heard(&mut self, change: Change, now: Reading) -> Act {
        match (self.step, change) {
            (Step::Entering, Change::Entered) => {
                self.entered = Some(now);
                self.step = Step::Resting;
                Act::ReadAfter(REST)
            }
            (Step::Leaving, Change::Exited) => {
                self.step = Step::Done;
                match (self.entered, self.at_rest) {
                    (Some(e), Some(r)) => Act::Write(Box::new([e, r, now])),
                    _ => Act::Nothing,
                }
            }
            _ => Act::Nothing,
        }
    }

    /// [`REST`] has passed in full screen: read, and out again.
    pub(crate) fn rested(&mut self, now: Reading) -> Act {
        match self.step {
            Step::Resting => {
                self.at_rest = Some(now);
                self.step = Step::Leaving;
                Act::Toggle
            }
            _ => Act::Nothing,
        }
    }
}

/// The armed check: its state, what it reads, where it writes, and the two
/// things that hear the window and the directory, dropped with it.
struct Live {
    check: Check,
    parts: Parts,
    dir: PathBuf,
    _full_screen: baud_present_macos::full_screen::FullScreen,
    source: Option<dispatch2::DispatchRetained<dispatch2::DispatchSource>>,
}

/// **The directory's watch cancelled with the check**: the cancel is
/// asynchronous, and "Closing the descriptor or port before the
/// cancellation handler is invoked may result in a race condition"
/// (`[[source: macos-sdk-dispatch-source-h]]` 461-468), so the descriptor
/// is the source's context and its cancel handler closes it.
impl Drop for Live {
    fn drop(&mut self) {
        if let Some(s) = &self.source {
            s.cancel();
        }
    }
}

/// The source's cancel handler: "submitted ... once the source's event
/// handler has finished" (`[[source: macos-sdk-dispatch-source-h]]`
/// 523-525), it closes the directory's descriptor.
extern "C" fn on_cancel(ctx: *mut core::ffi::c_void) {
    // SAFETY: the box `arm` made the source's context, freed only here,
    // once, after its last event handler.
    drop(unsafe { Box::from_raw(ctx.cast::<std::fs::File>()) });
}

std::thread_local! {
    /// The one check of the process, on the main thread.
    static LIVE: std::cell::RefCell<Option<Live>> = const { std::cell::RefCell::new(None) };
}

/// **Does `act`**, each on the main queue: a toggle on a later turn, so no
/// notification's handler is inside a transition it starts; a reading
/// after its wait; the file written.
fn run(act: Act) {
    match act {
        Act::Nothing => {}
        Act::Toggle => dispatch2::DispatchQueue::main().exec_async(|| {
            LIVE.with_borrow(|l| {
                if let Some(l) = l {
                    l.parts.window.toggleFullScreen(None);
                }
            });
        }),
        Act::ReadAfter(d) => {
            let Ok(when) = dispatch2::DispatchTime::try_from(d) else {
                return;
            };
            let _ = dispatch2::DispatchQueue::main().after(when, || {
                let next = LIVE.with_borrow_mut(|l| {
                    l.as_mut().map_or(Act::Nothing, |l| {
                        let now = read(&l.parts);
                        l.check.rested(now)
                    })
                });
                run(next);
            });
        }
        Act::Write(r) => LIVE.with_borrow(|l| {
            if let Some(l) = l {
                let _ = std::fs::write(l.dir.join(OUT), json(&r[0], &r[1], &r[2], &defaults()));
            }
        }),
    }
}

/// The directory changed: [`GO`] there starts the check.
extern "C" fn on_dir(_: *mut core::ffi::c_void) {
    let act = LIVE.with_borrow_mut(|l| match l {
        Some(l) if l.dir.join(GO).exists() => l.check.go(),
        _ => Act::Nothing,
    });
    run(act);
}

/// **The check armed for `parts` from `dir`**: the window heard through
/// its full screen, and `dir` watched for [`GO`]. Once a process.
pub(crate) fn arm(parts: Parts, dir: &Path) {
    use dispatch2::DispatchObject;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let full_screen = baud_present_macos::full_screen::when_full_screen(&parts.window, |change| {
        let act = LIVE.with_borrow_mut(|l| {
            l.as_mut().map_or(Act::Nothing, |l| {
                let now = read(&l.parts);
                l.check.heard(change, now)
            })
        });
        run(act);
    });
    // `O_EVTONLY`, the descriptor for events alone (`sys.rs`).
    let dir_fd = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x8000)
        .open(dir)
        .ok();
    let source = dir_fd.and_then(|f| {
        let handle = usize::try_from(f.as_raw_fd()).ok()?;
        // SAFETY: the vnode source's type, a descriptor its context holds
        // open until the cancel handler has run, the write mask, and the
        // main queue, which the source retains.
        let s = unsafe {
            dispatch2::DispatchSource::new(
                (&raw const dispatch2::_dispatch_source_type_vnode).cast_mut(),
                handle,
                DISPATCH_VNODE_WRITE,
                Some(dispatch2::DispatchQueue::main()),
            )
        };
        let ctx = Box::into_raw(Box::new(f));
        // SAFETY: the context is the box `on_cancel` frees, valid until
        // then; `on_dir` does not read it.
        unsafe { s.set_context(ctx.cast()) };
        s.set_event_handler_f(on_dir);
        s.set_cancel_handler_f(on_cancel);
        s.activate();
        Some(s)
    });
    LIVE.with_borrow_mut(|l| {
        *l = Some(Live {
            check: Check::new(),
            parts,
            dir: dir.to_path_buf(),
            _full_screen: full_screen,
            source,
        });
    });
}

/// **The check dropped**, its observer and its watch with it: a step it
/// had scheduled finds none and does nothing.
#[allow(dead_code, reason = "baud-app/tests/titlebar_mark.rs calls it")]
pub(crate) fn disarm() {
    LIVE.with_borrow_mut(|l| *l = None);
}

/// The check's step now, for a test; `None` when none is armed.
pub(crate) fn step() -> Option<Step> {
    LIVE.with_borrow(|l| l.as_ref().map(|l| l.check.step()))
}

/// **What the armed check would do if [`GO`] were heard now**, for a test:
/// the directory's handler, called by hand.
#[allow(dead_code, reason = "baud-app/tests/titlebar_mark.rs calls it")]
pub(crate) fn heard_dir() {
    on_dir(core::ptr::null_mut());
}

/// **The read at rest, by hand**, for a test that does not wait [`REST`].
#[allow(dead_code, reason = "baud-app/tests/titlebar_mark.rs calls it")]
pub(crate) fn rest_now() {
    let next = LIVE.with_borrow_mut(|l| {
        l.as_mut().map_or(Act::Nothing, |l| {
            let now = read(&l.parts);
            l.check.rested(now)
        })
    });
    run(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use macos_liquid_glass::objc2_foundation::{NSPoint, NSSize};

    fn reading(top: CGFloat) -> Reading {
        Reading {
            style_full_screen: false,
            view: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0 - top)),
            container_height: 600.0,
            layout: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0 - top)),
            content_height: 600.0,
            lockup: None,
            lights: None,
        }
    }

    /// **The steps go armed, entering, resting, leaving, done, each on the
    /// one event it waits for**: `GO` toggles once, "did enter" schedules
    /// the read at rest, the read toggles back, "did exit" writes all
    /// three. The controls: a "did exit" while entering, a second `GO` and
    /// a read at rest before entering do nothing. Perturbation: answer
    /// `Write` at "did enter" and the third reading is never taken.
    #[test]
    fn the_check_takes_each_step_on_its_own_event() {
        let mut c = Check::new();
        assert_eq!(c.rested(reading(28.0)), Act::Nothing, "not in full screen");
        assert_eq!(c.go(), Act::Toggle);
        assert_eq!(c.go(), Act::Nothing, "once");
        assert_eq!(c.heard(Change::Exited, reading(28.0)), Act::Nothing);
        assert_eq!(c.step(), Step::Entering);
        assert_eq!(c.heard(Change::Entered, reading(0.0)), Act::ReadAfter(REST));
        assert_eq!(c.rested(reading(0.0)), Act::Toggle);
        assert_eq!(
            c.heard(Change::Exited, reading(28.0)),
            Act::Write(Box::new([reading(0.0), reading(0.0), reading(28.0)]))
        );
        assert_eq!(c.step(), Step::Done);
        assert_eq!(c.heard(Change::Entered, reading(0.0)), Act::Nothing);
    }

    /// **The file names each reading, the view's top and the defaults**,
    /// absent keys as `null`. The control: a reading 28 points short of
    /// the top says so.
    #[test]
    fn the_file_says_what_was_read() {
        let d = [
            ("AppleMenuBarVisibleInFullscreen", None),
            ("_HIHideMenuBar", Some("0".to_string())),
            ("AppleLanguages", Some("found".to_string())),
        ];
        let j = json(&reading(0.0), &reading(0.0), &reading(28.0), &d);
        for part in [
            "\"entered\": {\"style_full_screen\": false",
            "\"view_at_top\": true",
            "\"view_at_top\": false",
            "\"content_layout_rect\": {\"x\": 0, \"y\": 0, \"width\": 800, \"height\": 572}",
            "\"AppleMenuBarVisibleInFullscreen\": null",
            "\"_HIHideMenuBar\": \"0\"",
            "\"AppleLanguages\": \"found\"",
        ] {
            assert!(j.contains(part), "{part}:\n{j}");
        }
        assert!(reading(0.0).at_top());
        assert!(!reading(28.0).at_top(), "the control");
        assert_eq!(j.matches("\"view_at_top\": true").count(), 2);
    }
}
