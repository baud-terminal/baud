//! **The effect probe**, built only under the test-only `effect-probe`
//! feature: where each effect a driver asks of the OS actually runs.
//!
//! Under the feature an effect's body -- a link opened or revealed, a
//! command started, a pasteboard written -- is this module's [`record`]
//! instead of the OS call, so a test opens nothing, starts nothing and
//! writes no pasteboard. [`record`] notes the effect's kind, whether it
//! runs inside a view's `frame` or another of its callbacks or in a turn
//! of its own ([`Where`]), and whether a responder call is on the stack
//! ([`Responder`]). Then it makes the call the reveal crash's stack shows
//! under the services handler, `-[NSApplication
//! nextEventMatchingMask:untilDate:inMode:dequeue:]` in the default mode,
//! with a `wake:` of its own queued first, until that wake is delivered:
//! inside a responder call that re-enters the view, which is the crash.
//!
//! [`Guarded`](crate::unwind::Guarded) keeps [`Where`] and [`Responder`]
//! for every responder method and counts each `wake` it delivers.

use core::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

use macos_liquid_glass::drawable::{DrawableView, Waker};
use macos_liquid_glass::objc2::MainThreadMarker;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{Message, msg_send, sel};
use macos_liquid_glass::objc2_app_kit::{NSApplication, NSEventMask};
use macos_liquid_glass::objc2_foundation::{NSDate, NSDefaultRunLoopMode};
use macos_liquid_glass::objc2_quartz_core::CADisplayLink;

/// Where a call runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    /// Inside the view's `frame`.
    InFrame,
    /// Inside another of the view's responder methods.
    InCallback,
    /// In a turn of its own, no responder method on the stack.
    InTurn,
}

/// Whether a responder method is on the stack: the view's responder is
/// borrowed for each one's whole length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Responder {
    Free,
    Held,
}

/// What an effect was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectKind {
    Open,
    Reveal,
    Command,
    Copy,
}

/// What the nested call met: the wake queued before it, delivered inside
/// it, or the bound reached with none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nested {
    Delivered,
    Never,
}

/// One effect, as it ran.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Record {
    pub kind: EffectKind,
    pub at: Where,
    pub responder: Responder,
    pub nested: Nested,
}

impl Record {
    /// The record as one line of text, for a child process to print and
    /// its parent to read back with [`Record::parse`].
    pub fn line(&self) -> String {
        let kind = match self.kind {
            EffectKind::Open => "open",
            EffectKind::Reveal => "reveal",
            EffectKind::Command => "command",
            EffectKind::Copy => "copy",
        };
        let at = match self.at {
            Where::InFrame => "in-frame",
            Where::InCallback => "in-callback",
            Where::InTurn => "in-turn",
        };
        let responder = match self.responder {
            Responder::Free => "free",
            Responder::Held => "held",
        };
        let nested = match self.nested {
            Nested::Delivered => "delivered",
            Nested::Never => "never",
        };
        format!("effect {kind} {at} {responder} {nested}")
    }

    /// What [`Record::line`] wrote, or none for any other line.
    pub fn parse(line: &str) -> Option<Record> {
        let mut w = line.split(' ');
        if w.next()? != "effect" {
            return None;
        }
        let kind = match w.next()? {
            "open" => EffectKind::Open,
            "reveal" => EffectKind::Reveal,
            "command" => EffectKind::Command,
            "copy" => EffectKind::Copy,
            _ => return None,
        };
        let at = match w.next()? {
            "in-frame" => Where::InFrame,
            "in-callback" => Where::InCallback,
            "in-turn" => Where::InTurn,
            _ => return None,
        };
        let responder = match w.next()? {
            "free" => Responder::Free,
            "held" => Responder::Held,
            _ => return None,
        };
        let nested = match w.next()? {
            "delivered" => Nested::Delivered,
            "never" => Nested::Never,
            _ => return None,
        };
        w.next().is_none().then_some(Record {
            kind,
            at,
            responder,
            nested,
        })
    }
}

/// The wakes [`Guarded`](crate::unwind::Guarded) has delivered: compared,
/// never read as a number.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Wakes(u64);

impl Wakes {
    /// None delivered.
    pub const NONE: Wakes = Wakes(0);

    /// One more.
    #[must_use]
    pub fn next(self) -> Wakes {
        Wakes(self.0 + 1)
    }
}

/// The bound on the nested call: reaching it is a failure, never a wait.
const BOUND: Duration = Duration::from_secs(2);

thread_local! {
    static AT: Cell<(Where, Responder)> = const { Cell::new((Where::InTurn, Responder::Free)) };
    static WAKES: Cell<Wakes> = const { Cell::new(Wakes::NONE) };
    static WAKER: Cell<Option<Waker>> = const { Cell::new(None) };
    static LOG: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
}

/// A responder method entered at `at`, left when this drops.
pub struct Entered((Where, Responder));

impl Drop for Entered {
    fn drop(&mut self) {
        AT.with(|a| a.set(self.0));
    }
}

/// Enter a responder method: `InFrame` for `frame`, `InCallback` for the
/// rest.
pub fn enter(at: Where) -> Entered {
    Entered(AT.with(|a| a.replace((at, Responder::Held))))
}

/// Where the main thread is now.
pub fn now() -> (Where, Responder) {
    AT.with(Cell::get)
}

/// A `wake` delivered to the responder.
pub fn woke() {
    WAKES.with(|w| w.set(w.get().next()));
}

/// The wakes delivered so far.
pub fn wakes() -> Wakes {
    WAKES.with(Cell::get)
}

/// The view whose `wake:` [`record`] queues before its nested call.
pub fn set_waker(waker: Waker) {
    WAKER.with(|w| w.set(Some(waker)));
}

/// Every effect recorded since the last take, in order.
pub fn take() -> Vec<Record> {
    LOG.with(|l| core::mem::take(&mut *l.borrow_mut()))
}

/// **The effect `kind`, recorded where it runs**, then the crash's nested
/// call: a `wake:` posted from another thread and joined, so it is
/// certainly queued, and `nextEventMatchingMask:` in the default mode,
/// which runs the main run loop, until the responder has had it or
/// [`BOUND`] has passed.
pub fn record(kind: EffectKind) {
    let (at, responder) = now();
    let before = wakes();
    if let Some(waker) = WAKER.with(Cell::get) {
        let _ = std::thread::spawn(move || waker.wake()).join();
    }
    let nested = match MainThreadMarker::new() {
        None => Nested::Never,
        Some(mtm) => {
            let app = NSApplication::sharedApplication(mtm);
            let start = Instant::now();
            loop {
                if wakes() > before {
                    break Nested::Delivered;
                }
                if start.elapsed() >= BOUND {
                    break Nested::Never;
                }
                let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
                // SAFETY: the default mode is a framework constant; not
                // dequeued, so no event is taken from the app.
                let _ = unsafe {
                    app.nextEventMatchingMask_untilDate_inMode_dequeue(
                        NSEventMask::Any,
                        Some(&until),
                        NSDefaultRunLoopMode,
                        false,
                    )
                };
            }
        }
    };
    LOG.with(|l| {
        l.borrow_mut().push(Record {
            kind,
            at,
            responder,
            nested,
        });
    });
}

/// One pass of the main run loop in the default mode, for at most
/// `seconds`, returning after the first source it handles.
pub fn pass(seconds: f64) {
    // SAFETY: the default mode is a framework constant.
    let mode = unsafe { macos_liquid_glass::objc2_core_foundation::kCFRunLoopDefaultMode };
    macos_liquid_glass::objc2_core_foundation::CFRunLoop::run_in_mode(mode, seconds, true);
}

/// **The view's frame, driven by hand**: a `CADisplayLink` targeting the
/// view's `tick:`, never added to a run loop, so it fires only when
/// [`Ticker::tick`] sends it -- the display link's own code path, with no
/// vsync to wait on and no window on screen.
pub struct Ticker {
    view: Retained<DrawableView>,
    link: Retained<CADisplayLink>,
}

impl Ticker {
    pub fn new(view: &DrawableView) -> Ticker {
        // SAFETY: the target is the view, which implements `tick:` with
        // the display link's signature and is retained beside the link.
        let link = unsafe { view.displayLinkWithTarget_selector(view, sel!(tick:)) };
        Ticker {
            view: view.retain(),
            link,
        }
    }

    /// One frame: the view's `tick:`, sent as the display link sends it.
    pub fn tick(&self) {
        // SAFETY: `tick:` is the view's own method, taking the link.
        let () = unsafe { msg_send![&*self.view, tick: &*self.link] };
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}
