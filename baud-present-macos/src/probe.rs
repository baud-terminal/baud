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
use macos_liquid_glass::objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSPoint};
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
    /// A link opened (`link::workspace`).
    Open,
    /// A file revealed in Finder (`link::workspace`).
    Reveal,
    /// A `[link.*]` command started.
    Command,
    /// The pasteboard written (`clipboard::write_text`).
    Copy,
    /// The pasteboard emptied (`clipboard::clear`).
    Clear,
    /// The find pasteboard written (`clipboard::write_find`).
    FindText,
    /// Words to VoiceOver (`announce::announce`).
    Announce,
    /// Permission to notify asked (`notify::Center`).
    Authorize,
    /// A notification posted.
    Notice,
    /// A notification removed.
    Unnotice,
    /// The Dock tile's bar shown or taken down.
    DockBar,
    /// The Dock icon asked to bounce (`notify::attention`).
    Attention,
    /// That request cancelled (`notify::cancel_attention`).
    Unattention,
    /// The system beep (`bell::beep`).
    Beep,
    /// A window raised, no focus taken (`window::order_front`).
    Front,
    /// A window raised and made key (`window::make_key_and_order_front`).
    KeyFront,
    /// The application activated (`window::activate`).
    Activate,
    /// The application's Dock icon set (`window::set_icon`).
    Icon,
    /// A window restored from the Dock (`window::deminiaturize`).
    Deminiaturize,
    /// A window put in the Dock (`window::miniaturize`).
    Miniaturize,
    /// A window put behind the others (`window::order_back`).
    Back,
    /// A window into full screen or out of it
    /// (`window::toggle_full_screen`).
    FullScreen,
    /// A window's frame set (`window::set_frame`).
    Frame,
    /// A new window opened, running a program (`window::open_running`).
    OpenWindow,
}

/// Each kind's word in a [`Record::line`], one table both ways.
const KINDS: [(EffectKind, &str); 24] = [
    (EffectKind::Open, "open"),
    (EffectKind::Reveal, "reveal"),
    (EffectKind::Command, "command"),
    (EffectKind::Copy, "copy"),
    (EffectKind::Clear, "clear"),
    (EffectKind::FindText, "find-text"),
    (EffectKind::Announce, "announce"),
    (EffectKind::Authorize, "authorize"),
    (EffectKind::Notice, "notice"),
    (EffectKind::Unnotice, "unnotice"),
    (EffectKind::DockBar, "dock-bar"),
    (EffectKind::Attention, "attention"),
    (EffectKind::Unattention, "unattention"),
    (EffectKind::Beep, "beep"),
    (EffectKind::Front, "front"),
    (EffectKind::KeyFront, "key-front"),
    (EffectKind::Activate, "activate"),
    (EffectKind::Icon, "icon"),
    (EffectKind::Deminiaturize, "deminiaturize"),
    (EffectKind::Miniaturize, "miniaturize"),
    (EffectKind::Back, "back"),
    (EffectKind::FullScreen, "full-screen"),
    (EffectKind::Frame, "frame"),
    (EffectKind::OpenWindow, "open-window"),
];

impl EffectKind {
    /// The kind's word.
    pub fn word(self) -> &'static str {
        KINDS
            .iter()
            .find(|(k, _)| *k == self)
            .map_or("?", |(_, w)| w)
    }

    /// The kind a word names.
    pub fn of(word: &str) -> Option<EffectKind> {
        KINDS.iter().find(|(_, w)| *w == word).map(|(k, _)| *k)
    }
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
        let kind = self.kind.word();
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
        let kind = EffectKind::of(w.next()?)?;
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

/// Whether a turn's door (`turn::soon`, `turn::later`) hands over
/// inline: the rig's controls, which put the effect back where it was
/// before the fix.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inline {
    /// On a main-queue turn of its own, as shipped.
    No,
    /// Inside the callback that asked: the click's answer, on `wake`.
    InCallback,
    /// Parked, and run by the next `frame` (`run_parked`): the crash's own
    /// shape, the reveal made inside the frame.
    InFrame,
}

/// The bound on the nested call: reaching it is a failure, never a wait.
const BOUND: Duration = Duration::from_secs(2);

thread_local! {
    static AT: Cell<(Where, Responder)> = const { Cell::new((Where::InTurn, Responder::Free)) };
    static WAKES: Cell<Wakes> = const { Cell::new(Wakes::NONE) };
    static WAKER: Cell<Option<Waker>> = const { Cell::new(None) };
    static LOG: RefCell<Vec<Record>> = const { RefCell::new(Vec::new()) };
    static INLINE: Cell<Inline> = const { Cell::new(Inline::No) };
    static PARKED: RefCell<Vec<Box<dyn FnOnce()>>> = const { RefCell::new(Vec::new()) };
    static KEY: Cell<Key> = const { Cell::new(Key::No) };
}

/// Hold `run` for the next frame ([`Inline::InFrame`]).
pub fn park(run: Box<dyn FnOnce()>) {
    PARKED.with_borrow_mut(|p| p.push(run));
}

/// Run what [`park`] held: called by the Driver's `frame`, under the probe
/// only. Taken first, so an effect that parks another leaves it a frame.
pub fn run_parked() {
    let parked = PARKED.with_borrow_mut(core::mem::take);
    for run in parked {
        run();
    }
}

/// **Whether baud believes its window is key**: the event rig's focus
/// door. The rig's window is never made key, so without it every left
/// press is the click that brings the window forward and a link's
/// Cmd-click is never armed. It changes what baud believes, read by
/// `poll_focus` alone (baud-app's source check), and never AppKit's key
/// state, which the rig checks false after every iteration.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    /// What AppKit says, alone.
    No,
    /// Key, whatever AppKit says.
    Yes,
}

/// Set whether baud believes its window is key.
pub fn set_key(key: Key) {
    KEY.with(|k| k.set(key));
}

/// Whether baud believes its window is key.
pub fn key() -> Key {
    KEY.with(Cell::get)
}

/// Set whether a turn's door hands over inline.
pub fn set_inline(inline: Inline) {
    INLINE.with(|i| i.set(inline));
}

/// Whether a turn's door hands over inline.
pub fn inline() -> Inline {
    INLINE.with(Cell::get)
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
    // Inside a responder call the window crate holds the wake until the
    // call returns, so the wait there is one nested call -- which runs the
    // queued perform, ready since the join -- and not the bound.
    let bound = match responder {
        Responder::Held => Duration::ZERO,
        Responder::Free => BOUND,
    };
    let nested = match MainThreadMarker::new() {
        None => Nested::Never,
        Some(mtm) => {
            let app = NSApplication::sharedApplication(mtm);
            let start = Instant::now();
            let mut first = true;
            loop {
                if wakes() > before {
                    break Nested::Delivered;
                }
                if !first && start.elapsed() >= bound {
                    break Nested::Never;
                }
                first = false;
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

/// **A pointer gesture sent to the view**: an `NSEvent` made here and
/// handed to the view's own `mouseDown:`, `mouseDragged:` or `mouseUp:`,
/// the methods AppKit calls with a click's events. It is never posted to
/// the application or the window server (`postEvent:`, `CGEventPost`), so
/// it reaches nothing but this view, and nothing takes focus.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    Down,
    Dragged,
    Up,
}

/// Whether Command is held with a [`Press`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Chord {
    Plain,
    Command,
}

/// Send `press` at `at`, a point in the view's own (flipped) coordinates,
/// to `view`'s own method for it.
pub fn press(view: &DrawableView, press: Press, at: NSPoint, chord: Chord) {
    use macos_liquid_glass::objc2::runtime::AnyObject;
    use macos_liquid_glass::objc2::{class, rc::Retained as R};
    use macos_liquid_glass::objc2_app_kit::{NSEventModifierFlags, NSEventType};
    let in_window = view.convertPoint_toView(at, None);
    let number = view.window().map_or(0, |w| w.windowNumber());
    let kind = match press {
        Press::Down => NSEventType::LeftMouseDown,
        Press::Dragged => NSEventType::LeftMouseDragged,
        Press::Up => NSEventType::LeftMouseUp,
    };
    let flags = match chord {
        Chord::Plain => NSEventModifierFlags(0),
        Chord::Command => NSEventModifierFlags::Command,
    };
    // SAFETY: `+mouseEventWithType:location:modifierFlags:timestamp:
    // windowNumber:context:eventNumber:clickCount:pressure:`, with a nil
    // context, which the header asks for; nullable.
    let event: Option<R<AnyObject>> = unsafe {
        msg_send![
            class!(NSEvent),
            mouseEventWithType: kind,
            location: in_window,
            modifierFlags: flags,
            timestamp: 0.0f64,
            windowNumber: number,
            context: Option::<&AnyObject>::None,
            eventNumber: 0isize,
            clickCount: 1isize,
            pressure: 1.0f32
        ]
    };
    let Some(event) = event else {
        return;
    };
    // SAFETY: the view's own event methods, each taking an `NSEvent`.
    unsafe {
        let () = match press {
            Press::Down => msg_send![view, mouseDown: &*event],
            Press::Dragged => msg_send![view, mouseDragged: &*event],
            Press::Up => msg_send![view, mouseUp: &*event],
        };
    }
}

/// Send the view a menu item's action, `copy:` and the like, as the menu
/// sends it to the first responder: the view's own method, a nil sender.
pub fn menu(view: &DrawableView, action: macos_liquid_glass::objc2::runtime::Sel) {
    use macos_liquid_glass::objc2::runtime::AnyObject;
    // SAFETY: `-performSelector:withObject:`, the action an `IBAction`
    // taking its sender. An action returns nothing, so what comes back is
    // no object: read as a bare pointer, never retained.
    let _: *mut AnyObject =
        unsafe { msg_send![view, performSelector: action, withObject: Option::<&AnyObject>::None] };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kind is in the table once, and its word reads back as it.
    /// The match has no `_` arm, so a new kind does not compile until it
    /// has a place here.
    #[test]
    fn every_kind_has_one_word() {
        /// A kind's place in the table.
        #[derive(PartialEq, Eq, Debug)]
        struct Place(usize);
        fn place(k: EffectKind) -> Place {
            match k {
                EffectKind::Open => Place(0),
                EffectKind::Reveal => Place(1),
                EffectKind::Command => Place(2),
                EffectKind::Copy => Place(3),
                EffectKind::Clear => Place(4),
                EffectKind::FindText => Place(5),
                EffectKind::Announce => Place(6),
                EffectKind::Authorize => Place(7),
                EffectKind::Notice => Place(8),
                EffectKind::Unnotice => Place(9),
                EffectKind::DockBar => Place(10),
                EffectKind::Attention => Place(11),
                EffectKind::Unattention => Place(12),
                EffectKind::Beep => Place(13),
                EffectKind::Front => Place(14),
                EffectKind::KeyFront => Place(15),
                EffectKind::Activate => Place(16),
                EffectKind::Icon => Place(17),
                EffectKind::Deminiaturize => Place(18),
                EffectKind::Miniaturize => Place(19),
                EffectKind::Back => Place(20),
                EffectKind::FullScreen => Place(21),
                EffectKind::Frame => Place(22),
                EffectKind::OpenWindow => Place(23),
            }
        }
        for (i, (k, w)) in KINDS.iter().enumerate() {
            assert_eq!(place(*k), Place(i), "{k:?}");
            assert_eq!((k.word(), EffectKind::of(w)), (*w, Some(*k)));
        }
        let r = Record {
            kind: EffectKind::KeyFront,
            at: Where::InTurn,
            responder: Responder::Free,
            nested: Nested::Delivered,
        };
        assert_eq!(Record::parse(&r.line()), Some(r));
    }
}
