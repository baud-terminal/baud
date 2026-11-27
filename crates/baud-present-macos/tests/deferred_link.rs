//! **A link handed to the OS from the view's frame is handed over after
//! it, and a nested run loop inside a callback cannot re-enter the view.**
//!
//! The shape of the reveal crash: a display-link tick reached
//! `NSWorkspace activateFileViewerSelectingURLs:`, AppKit's services
//! handler waited for Finder in a nested event loop, that loop ran the
//! view's queued `wake:` perform, and the window crate's view panicked
//! "already borrowed" into `__NSThreadPerformPerform`, which aborted.
//!
//! The view is the crate's own `DrawableView`, made and never put in a
//! window. The frame is its `tick:`, sent with a `CADisplayLink` that is
//! never added to a run loop. The OS is a hand that records what it was
//! given, posts the view a `wake:` from another thread (joined, so it is
//! queued), and makes the services handler's own call,
//! `nextEventMatchingMask:untilDate:inMode:dequeue:NO` in the default
//! mode. No Finder and no browser. 100 iterations each.
//!
//! Two checks:
//! - **Handed over after the frame** (`turn::later`): nothing is handed
//!   over during `tick:`; on a later turn exactly one reveal of the
//!   address is, with the responder free, and the nested `wake:` reaches
//!   the responder inside the hand's own call.
//! - **The window crate's guard, the control**: the same hand called
//!   inline from the frame, as the reveal was before it had a turn. Since
//!   macos-liquid-glass 1.0.0-beta.9 the nested `wake:` is held while the
//!   frame runs and delivered once as it returns, and nothing panics. Run
//!   against 1.0.0-beta.8, this arm aborts with "RefCell already
//!   borrowed" at the crate's `drawable.rs:936`, the crash itself, while
//!   the first check passes there too: the deferral alone stops it.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.

const DEFERRED: &str = "a_link_from_the_frame_is_handed_over_after_it";
const HELD: &str = "a_wake_nested_in_the_frame_is_held_until_it_returns";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && !args.iter().any(|a| a == "--ignored") {
            println!("{DEFERRED}: test");
            println!("{HELD}: test");
        }
        return;
    }
    let filter = args.iter().find(|a| !a.starts_with('-'));
    let runs = |name: &str| filter.is_none_or(|f| name.contains(f.as_str()));
    #[cfg(target_os = "macos")]
    {
        if runs(DEFERRED) {
            checks::handed_over_after_the_frame();
            println!("{DEFERRED}: ok");
        }
        if runs(HELD) {
            checks::a_nested_wake_is_held();
            println!("{HELD}: ok");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = runs;
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod checks {
    use baud_present_macos::link::{Ask, Opened, checks};
    use baud_present_macos::turn;
    use baud_present_macos::unwind::SiteName;
    use baud_present_macos::unwind::{Caught, Guarded, Settles, caught};
    use core::cell::Cell;
    use macos_liquid_glass::drawable::{
        DrawableView, Focus, Handled, KeyPress, Mouse, Rect, Responder, Scroll, Waker,
    };
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2::{msg_send, sel};
    use macos_liquid_glass::objc2_app_kit::{NSApplication, NSEventMask};
    use macos_liquid_glass::objc2_core_foundation::{CFRunLoop, CGFloat, kCFRunLoopDefaultMode};
    use macos_liquid_glass::objc2_foundation::{
        MainThreadMarker, NSDate, NSDefaultRunLoopMode, NSPoint, NSRect, NSSize,
    };
    use macos_liquid_glass::objc2_quartz_core::CADisplayLink;
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering::Relaxed;
    use std::time::{Duration, Instant};

    const FILE: &str = "file:///private/tmp/baud-reveal-test/a%20file.txt";
    const ITERATIONS: u32 = 100;
    /// The bound on a wait: reaching it is a failure, never a pace.
    const BOUND: Duration = Duration::from_secs(2);

    /// Whether a responder method is on the stack when the hand runs.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Call {
        Free,
        Held,
    }

    /// How the frame hands the reveal over.
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Hands {
        Later,
        Inline,
    }

    /// The view's waker, for the hand, which is a plain `fn`: the current
    /// check's view.
    static WAKER: Mutex<Option<Waker>> = Mutex::new(None);
    /// What the hand was given, in order.
    static HANDED: Mutex<Vec<(Ask, String)>> = Mutex::new(Vec::new());
    /// What `done` heard, in order.
    static DONE: Mutex<Vec<Opened>> = Mutex::new(Vec::new());
    /// The responder's wakes, nested ones included.
    static WAKES: AtomicU32 = AtomicU32::new(0);
    /// The wakes counted as the hand's nested call ended.
    static WAKES_IN_HAND: AtomicU32 = AtomicU32::new(0);
    /// Where the hand ran.
    static IN_HAND: Mutex<Option<Call>> = Mutex::new(None);

    thread_local! {
        /// Whether a responder method is on the stack.
        static IN_CALL: Cell<Call> = const { Cell::new(Call::Free) };
    }

    /// One `nextEventMatchingMask:` in the default mode, not dequeuing:
    /// the services handler's own call, which runs the main run loop.
    fn nested(until: f64) {
        let mtm = MainThreadMarker::new().expect("the main thread");
        let app = NSApplication::sharedApplication(mtm);
        let date = NSDate::dateWithTimeIntervalSinceNow(until);
        // SAFETY: the default mode is a framework constant; not dequeued,
        // so no event is taken.
        let _ = unsafe {
            app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&date),
                NSDefaultRunLoopMode,
                false,
            )
        };
    }

    /// A `wake:` posted from another thread and joined: certainly queued.
    fn post() {
        let waker = WAKER.lock().unwrap().expect("the view's waker");
        std::thread::spawn(move || waker.wake())
            .join()
            .expect("posted");
    }

    fn noted(ask: Ask, url: &str) {
        HANDED.lock().unwrap().push((ask, url.to_owned()));
        *IN_HAND.lock().unwrap() = Some(IN_CALL.with(Cell::get));
    }

    /// The OS, as the services handler behaves: record, queue a `wake:`,
    /// then wait in the nested call until it is delivered or the bound.
    fn finder(ask: Ask, url: &str) -> Opened {
        noted(ask, url);
        let before = WAKES.load(Relaxed);
        post();
        let start = Instant::now();
        while WAKES.load(Relaxed) == before && start.elapsed() < BOUND {
            nested(0.01);
        }
        WAKES_IN_HAND.store(WAKES.load(Relaxed), Relaxed);
        Opened::Taken
    }

    /// The same, inline in a callback: one nested call, which runs the
    /// queued perform -- it is ready -- and returns at its date.
    fn finder_inline(ask: Ask, url: &str) -> Opened {
        noted(ask, url);
        post();
        nested(0.05);
        WAKES_IN_HAND.store(WAKES.load(Relaxed), Relaxed);
        Opened::Taken
    }

    fn run(seconds: f64) {
        // SAFETY: the default mode is a static CFString the framework owns.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, seconds, true);
    }

    /// The responder: its frame asks for a reveal, as `pump` did from a
    /// tick; every wake is counted.
    struct Asker {
        hands: Hands,
        handed_during: Rc<Cell<Option<usize>>>,
    }

    /// A responder method on the stack, for the hand to read.
    struct InCall;

    impl InCall {
        fn enter() -> InCall {
            IN_CALL.with(|c| c.set(Call::Held));
            InCall
        }
    }

    impl Drop for InCall {
        fn drop(&mut self) {
            IN_CALL.with(|c| c.set(Call::Free));
        }
    }

    /// Nothing to settle: this view has no display link.
    impl Settles for Asker {
        fn settle(&mut self) {}
    }

    impl Responder for Asker {
        fn key(&mut self, _: &KeyPress) -> Handled {
            Handled::Consumed
        }
        fn text(&mut self, _: &str) {}
        fn command(&mut self, _: &str) {}
        fn marked_text(&mut self, _: &str) {}
        fn mouse(&mut self, _: &Mouse) {}
        fn scroll(&mut self, _: &Scroll) {}
        fn resized(&mut self, _: NSSize, _: CGFloat) {}
        fn frame(&mut self, _: f64) {
            let _call = InCall::enter();
            match self.hands {
                Hands::Later => {
                    assert_eq!(checks(FILE), Ok(()), "the address parses");
                    turn::later(SiteName("the test's reveal"), |_turn| {
                        let o = finder(Ask::Reveal, FILE);
                        DONE.lock().unwrap().push(o);
                    });
                }
                Hands::Inline => {
                    let o = finder_inline(Ask::Reveal, FILE);
                    DONE.lock().unwrap().push(o);
                }
            }
            self.handed_during.set(Some(HANDED.lock().unwrap().len()));
        }
        fn focus(&mut self, _: Focus) {}
        fn cursor_rect(&self) -> Rect {
            Rect::default()
        }
        fn wake(&mut self) {
            WAKES.fetch_add(1, Relaxed);
        }
    }

    type Rig = (
        Retained<DrawableView>,
        Retained<CADisplayLink>,
        Rc<Cell<Option<usize>>>,
    );

    /// The view, its responder, and a display link that fires only when
    /// sent.
    fn view(hands: Hands) -> Rig {
        let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
        let _ = NSApplication::sharedApplication(mtm);
        let view = DrawableView::new(
            mtm,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(40.0, 28.0)),
        );
        let handed_during = Rc::new(Cell::new(None));
        view.set_responder(Box::new(Guarded(Asker {
            hands,
            handed_during: handed_during.clone(),
        })));
        *WAKER.lock().unwrap() = Some(view.waker());
        // SAFETY: the target is the view, which implements `tick:` with the
        // display link's signature; never added to a run loop.
        let link = unsafe { view.displayLinkWithTarget_selector(&view, sel!(tick:)) };
        (view, link, handed_during)
    }

    fn reset() {
        HANDED.lock().unwrap().clear();
        DONE.lock().unwrap().clear();
        WAKES.store(0, Relaxed);
        WAKES_IN_HAND.store(0, Relaxed);
        *IN_HAND.lock().unwrap() = None;
    }

    pub(crate) fn handed_over_after_the_frame() {
        // A refusal is answered at once, before anything is asked.
        assert_eq!(checks("https://a b"), Err(Opened::Unparsed));
        let (view, link, handed_during) = view(Hands::Later);
        for i in 0..ITERATIONS {
            reset();
            // SAFETY: `tick:` is the view's own method, taking the link.
            let () = unsafe { msg_send![&*view, tick: &*link] };
            assert_eq!(
                handed_during.get(),
                Some(0),
                "{i}: nothing handed over inside the frame"
            );
            assert!(HANDED.lock().unwrap().is_empty(), "{i}: nor before a turn");
            let start = Instant::now();
            while DONE.lock().unwrap().is_empty() && start.elapsed() < BOUND {
                run(0.01);
            }
            assert_eq!(
                *HANDED.lock().unwrap(),
                [(Ask::Reveal, FILE.to_owned())],
                "{i}: one reveal, of the address asked for"
            );
            assert_eq!(*DONE.lock().unwrap(), [Opened::Taken], "{i}: heard once");
            assert_eq!(*IN_HAND.lock().unwrap(), Some(Call::Free), "{i}: free");
            assert_eq!(
                WAKES_IN_HAND.load(Relaxed),
                1,
                "{i}: the nested wake reached the responder inside the hand's call"
            );
            assert_eq!(WAKES.load(Relaxed), 1, "{i}: and no other wake came");
        }
        link.invalidate();
        assert_eq!(
            caught(),
            Caught::NONE,
            "no panic was stopped: none happened"
        );
    }

    pub(crate) fn a_nested_wake_is_held() {
        let (view, link, handed_during) = view(Hands::Inline);
        for i in 0..ITERATIONS {
            reset();
            // SAFETY: as above.
            let () = unsafe { msg_send![&*view, tick: &*link] };
            assert_eq!(
                handed_during.get(),
                Some(1),
                "{i}: handed over in the frame"
            );
            assert_eq!(*IN_HAND.lock().unwrap(), Some(Call::Held), "{i}: in a call");
            assert_eq!(
                WAKES_IN_HAND.load(Relaxed),
                0,
                "{i}: the nested wake was not delivered into the busy responder"
            );
            assert_eq!(
                WAKES.load(Relaxed),
                1,
                "{i}: and was delivered once, as the frame returned"
            );
        }
        link.invalidate();
        assert_eq!(caught(), Caught::NONE, "nothing panicked");
    }
}
