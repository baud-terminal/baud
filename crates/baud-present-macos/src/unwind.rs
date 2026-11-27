//! **No panic unwinds into AppKit or libdispatch.**
//!
//! Every way the OS calls into baud -- a view's responder method, a class's
//! action, a notification's block, a queued block, a dispatch source's
//! handler -- is a frame of Objective-C or C below the Rust that runs. A
//! panic that unwinds out of it is caught there as a foreign exception, and
//! dropping one aborts the process: `__NSThreadPerformPerform` catching a
//! panic out of a performed `wake:` is how a Finder reveal ended the app.
//! A block libdispatch runs, or an `extern "C"` handler, aborts sooner,
//! because neither may unwind at all.
//!
//! So each entry runs through [`guard`]: a panic stops at the entry, is
//! counted, is said once per entry on stderr (the panic hook has already
//! printed its message and location), and the entry answers a default
//! that does nothing. [`Guarded`] is the same for every method of a
//! view's [`Responder`].

use core::panic::AssertUnwindSafe;
use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use macos_liquid_glass::drawable::{
    Appearance, Focus, Handled, KeyPress, ModifierChange, Mouse, Rect, Responder, Scroll,
};
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::NSSize;

/// Panics stopped at an entry since the process began.
static CAUGHT: AtomicU64 = AtomicU64::new(0);

/// How many panics [`guard`] has stopped: compared, never read as a number.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Caught(u64);

impl Caught {
    /// None stopped.
    pub const NONE: Caught = Caught(0);
}

/// **An entry the OS calls into baud through**, by the name a stopped
/// panic says: the event rig's row names (`crates/baud-app/tests/event_rig.rs`)
/// are of this type, so a guarded entry and a row are named alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SiteName(pub &'static str);

impl core::fmt::Display for SiteName {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

/// The entries that have said so on stderr, so each says it once.
static SAID: Mutex<Vec<SiteName>> = Mutex::new(Vec::new());

/// How many panics [`guard`] has stopped.
pub fn caught() -> Caught {
    Caught(CAUGHT.load(Ordering::Relaxed))
}

/// Run `f`, the body of the entry `site`; a panic in it answers `default`
/// instead of leaving the entry. Each entry says its first panic on
/// stderr; after that the count of every panic stopped is said each time
/// it reaches a power of two, so an entry that panics on every frame is
/// not silent after its first.
pub fn guard<R>(site: SiteName, default: R, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            let count = CAUGHT.fetch_add(1, Ordering::Relaxed) + 1;
            let first = SAID.lock().map_or(true, |mut said| {
                let first = !said.contains(&site);
                if first {
                    said.push(site);
                }
                first
            });
            if first {
                eprintln!(
                    "baud: a panic in {site} was stopped there rather than unwind into the OS"
                );
            } else if count.is_power_of_two() {
                eprintln!("baud: {count} panics stopped at the OS's entries, the latest in {site}");
            }
            default
        }
    }
}

/// Under the `effect-probe` feature, where the method runs, kept until it
/// returns ([`crate::probe::enter`]); nothing otherwise.
macro_rules! entered {
    ($at:ident) => {
        #[cfg(feature = "effect-probe")]
        let _entered = crate::probe::enter(crate::probe::Where::$at);
    };
}

/// **What a responder does once each of its calls has returned**: the
/// display link paused, or its restart scheduled, from what the call left
/// (baud-app's `Driver::settle`, `frames.rs`). [`Guarded`] calls it after
/// every method but `cursor_rect`, which changes nothing, so no early
/// return inside a method skips it.
pub trait Settles {
    fn settle(&mut self);
}

/// **A [`Responder`] whose every method runs through [`guard`]**, and then
/// settles ([`Settles`]). What the view is given: a key the inner one
/// panicked on is consumed, and nothing else answers anything.
pub struct Guarded<R>(pub R);

impl<R: Settles> Guarded<R> {
    /// The inner responder settled, through a guard of its own.
    fn settled(&mut self) {
        guard(SiteName("the view's settle"), (), || self.0.settle());
    }
}

impl<R: Responder + Settles> Responder for Guarded<R> {
    fn key(&mut self, key: &KeyPress) -> Handled {
        entered!(InCallback);
        let handled = guard(SiteName("the view's key"), Handled::Consumed, || {
            self.0.key(key)
        });
        self.settled();
        handled
    }
    fn key_released(&mut self, key: &KeyPress) {
        entered!(InCallback);
        guard(SiteName("the view's key_released"), (), || {
            self.0.key_released(key)
        });
        self.settled();
    }
    fn modifiers_changed(&mut self, change: &ModifierChange) {
        entered!(InCallback);
        guard(SiteName("the view's modifiers_changed"), (), || {
            self.0.modifiers_changed(change);
        });
        self.settled();
    }
    fn text(&mut self, text: &str) {
        entered!(InCallback);
        guard(SiteName("the view's text"), (), || self.0.text(text));
        self.settled();
    }
    fn command(&mut self, selector: &str) {
        entered!(InCallback);
        guard(SiteName("the view's command"), (), || {
            self.0.command(selector)
        });
        self.settled();
    }
    fn marked_text(&mut self, text: &str) {
        entered!(InCallback);
        guard(SiteName("the view's marked_text"), (), || {
            self.0.marked_text(text)
        });
        self.settled();
    }
    fn mouse(&mut self, event: &Mouse) {
        entered!(InCallback);
        guard(SiteName("the view's mouse"), (), || self.0.mouse(event));
        self.settled();
    }
    fn scroll(&mut self, event: &Scroll) {
        entered!(InCallback);
        guard(SiteName("the view's scroll"), (), || self.0.scroll(event));
        self.settled();
    }
    fn resized(&mut self, size: NSSize, scale: CGFloat) {
        entered!(InCallback);
        guard(SiteName("the view's resized"), (), || {
            self.0.resized(size, scale)
        });
        self.settled();
    }
    fn frame(&mut self, target: f64) {
        entered!(InFrame);
        guard(SiteName("the view's frame"), (), || self.0.frame(target));
        self.settled();
    }
    fn focus(&mut self, focus: Focus) {
        entered!(InCallback);
        guard(SiteName("the view's focus"), (), || self.0.focus(focus));
        self.settled();
    }
    fn cursor_rect(&self) -> Rect {
        entered!(InCallback);
        guard(SiteName("the view's cursor_rect"), Rect::default(), || {
            self.0.cursor_rect()
        })
    }
    fn wake(&mut self) {
        entered!(InCallback);
        #[cfg(feature = "effect-probe")]
        crate::probe::woke();
        guard(SiteName("the view's wake"), (), || self.0.wake());
        self.settled();
    }
    fn presented(&mut self) {
        entered!(InCallback);
        guard(SiteName("the view's presented"), (), || self.0.presented());
        self.settled();
    }
    fn appearance(&mut self, appearance: Appearance) {
        entered!(InCallback);
        guard(SiteName("the view's appearance"), (), || {
            self.0.appearance(appearance)
        });
        self.settled();
    }
}
