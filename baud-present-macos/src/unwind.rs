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

/// The entries that have said so on stderr, so each says it once.
static SAID: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// How many panics [`guard`] has stopped.
pub fn caught() -> u64 {
    CAUGHT.load(Ordering::Relaxed)
}

/// Run `f`, the body of the entry named `site`; a panic in it answers
/// `default` instead of leaving the entry.
pub fn guard<R>(site: &'static str, default: R, f: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => {
            CAUGHT.fetch_add(1, Ordering::Relaxed);
            let first = SAID.lock().map_or(true, |mut said| {
                let first = !said.contains(&site);
                if first {
                    said.push(site);
                }
                first
            });
            if first {
                eprintln!(
                    "baud: a panic in {site} was stopped there rather than unwind into the OS; \
                     later ones there are counted, not said"
                );
            }
            default
        }
    }
}

/// **A [`Responder`] whose every method runs through [`guard`].** What the
/// view is given: a key the inner one panicked on is consumed, and nothing
/// else answers anything.
pub struct Guarded<R>(pub R);

impl<R: Responder> Responder for Guarded<R> {
    fn key(&mut self, key: &KeyPress) -> Handled {
        guard("the view's key", Handled::Consumed, || self.0.key(key))
    }
    fn key_released(&mut self, key: &KeyPress) {
        guard("the view's key_released", (), || self.0.key_released(key));
    }
    fn modifiers_changed(&mut self, change: &ModifierChange) {
        guard("the view's modifiers_changed", (), || {
            self.0.modifiers_changed(change);
        });
    }
    fn text(&mut self, text: &str) {
        guard("the view's text", (), || self.0.text(text));
    }
    fn command(&mut self, selector: &str) {
        guard("the view's command", (), || self.0.command(selector));
    }
    fn marked_text(&mut self, text: &str) {
        guard("the view's marked_text", (), || self.0.marked_text(text));
    }
    fn mouse(&mut self, event: &Mouse) {
        guard("the view's mouse", (), || self.0.mouse(event));
    }
    fn scroll(&mut self, event: &Scroll) {
        guard("the view's scroll", (), || self.0.scroll(event));
    }
    fn resized(&mut self, size: NSSize, scale: CGFloat) {
        guard("the view's resized", (), || self.0.resized(size, scale));
    }
    fn frame(&mut self, target: f64) {
        guard("the view's frame", (), || self.0.frame(target));
    }
    fn focus(&mut self, focus: Focus) {
        guard("the view's focus", (), || self.0.focus(focus));
    }
    fn cursor_rect(&self) -> Rect {
        guard("the view's cursor_rect", Rect::default(), || {
            self.0.cursor_rect()
        })
    }
    fn wake(&mut self) {
        guard("the view's wake", (), || self.0.wake());
    }
    fn presented(&mut self) {
        guard("the view's presented", (), || self.0.presented());
    }
    fn appearance(&mut self, appearance: Appearance) {
        guard("the view's appearance", (), || {
            self.0.appearance(appearance)
        });
    }
}
