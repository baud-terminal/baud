//! **A link handed to the OS from inside a view's callback is handed over
//! after it, and the nested run loop the OS spins re-enters a free view.**
//!
//! The shape of the crash it pins: a display-link tick reached
//! `NSWorkspace activateFileViewerSelectingURLs:`, AppKit's services
//! handler waited for Finder in a nested event loop, that loop ran the
//! view's queued `wake:` perform, and the window crate's view -- whose
//! responder is a `RefCell` borrowed for the whole of each callback --
//! panicked "already borrowed" into `__NSThreadPerformPerform`, which
//! aborted.
//!
//! Here the view is the crate's own `DrawableView`, made and never put in
//! a window; the callback is its `wake:`, sent as the run loop sends it;
//! and the OS is a hand that records what it was given, posts the view a
//! `wake:` through its `Waker`, and runs the run loop in the default mode
//! for 50 ms, as the services handler does. No Finder and no browser.
//!
//! Perturbation: make `link::later` call `hand` inline, as the reveal did,
//! and this test aborts with "already borrowed" at the crate's
//! `drawable.rs`.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.

const NAME: &str = "a_link_from_a_callback_is_handed_over_after_it";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && !args.iter().any(|a| a == "--ignored") {
            println!("{NAME}: test");
        }
        return;
    }
    let skipped = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .is_some_and(|filter| !NAME.contains(filter.as_str()));
    #[cfg(target_os = "macos")]
    if !skipped {
        checks::a_link_from_a_callback_is_handed_over_after_it();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = skipped;
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod checks {
    use baud_present_macos::link::{Ask, Opened, later};
    use baud_present_macos::unwind::{Caught, Guarded, caught};
    use core::cell::Cell;
    use macos_liquid_glass::drawable::{
        DrawableView, Focus, Handled, KeyPress, Mouse, Rect, Responder, Scroll, Waker,
    };
    use macos_liquid_glass::objc2::msg_send;
    use macos_liquid_glass::objc2::runtime::AnyObject;
    use macos_liquid_glass::objc2_core_foundation::{CFRunLoop, CGFloat, kCFRunLoopDefaultMode};
    use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
    use std::rc::Rc;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering::Relaxed;
    use std::sync::{Mutex, OnceLock};

    const FILE: &str = "file:///private/tmp/baud-reveal-test/a%20file.txt";

    /// The view's waker, for the hand, which is a plain `fn`.
    static WAKER: OnceLock<Waker> = OnceLock::new();
    /// What the hand was given, in order.
    static HANDED: Mutex<Vec<(Ask, String)>> = Mutex::new(Vec::new());
    /// What `done` heard, in order.
    static DONE: Mutex<Vec<Opened>> = Mutex::new(Vec::new());
    /// The responder's wakes, the nested one included.
    static WAKES: AtomicU32 = AtomicU32::new(0);
    /// The wakes counted as the hand's nested loop ended.
    static WAKES_IN_HAND: AtomicU32 = AtomicU32::new(0);

    /// The OS, as the services handler behaves: record, then wait in a
    /// nested loop that runs whatever the main run loop has queued -- here
    /// the `wake:` just posted.
    fn finder(ask: Ask, url: &str) -> Opened {
        HANDED.lock().unwrap().push((ask, url.to_owned()));
        WAKER.get().expect("the view's waker").wake();
        run(0.05);
        WAKES_IN_HAND.store(WAKES.load(Relaxed), Relaxed);
        Opened::Taken
    }

    fn run(seconds: f64) {
        // SAFETY: the default mode is a static CFString the framework owns.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, seconds, true);
    }

    /// The responder: its first wake asks for a reveal, as `pump` does from
    /// a tick; every wake is counted, the nested one included.
    struct Asker {
        handed_during: Rc<Cell<Option<usize>>>,
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
        fn frame(&mut self, _: f64) {}
        fn focus(&mut self, _: Focus) {}
        fn cursor_rect(&self) -> Rect {
            Rect::default()
        }
        fn wake(&mut self) {
            if WAKES.fetch_add(1, Relaxed) == 0 {
                let asked = later(Ask::Reveal, FILE.to_owned(), finder, |o| {
                    DONE.lock().unwrap().push(o);
                });
                assert_eq!(asked, Ok(()), "the address parses");
                self.handed_during.set(Some(HANDED.lock().unwrap().len()));
            }
        }
    }

    pub(crate) fn a_link_from_a_callback_is_handed_over_after_it() {
        let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
        let view = DrawableView::new(
            mtm,
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(40.0, 28.0)),
        );
        let handed_during = Rc::new(Cell::new(None));
        view.set_responder(Box::new(Guarded(Asker {
            handed_during: handed_during.clone(),
        })));
        assert!(WAKER.set(view.waker()).is_ok());

        // The refusals are answered at once and queue nothing.
        assert_eq!(
            later(Ask::Open, "https://a b".to_owned(), finder, |_| {}),
            Err(Opened::Unparsed)
        );

        // The callback, as the run loop delivers a perform.
        // SAFETY: `wake:` is the view's own method, taking an ignored object.
        let _: () = unsafe { msg_send![&*view, wake: core::ptr::null::<AnyObject>()] };
        assert_eq!(WAKES.load(Relaxed), 1, "the wake ran");
        assert_eq!(
            handed_during.get(),
            Some(0),
            "nothing is handed to the OS inside the callback"
        );
        assert!(
            HANDED.lock().unwrap().is_empty(),
            "nor after it, before a turn"
        );

        // Later turns of the main run loop: the queued block runs, the hand
        // spins its nested loop, and the view takes the nested wake.
        for _ in 0..40 {
            if !DONE.lock().unwrap().is_empty() {
                break;
            }
            run(0.05);
        }
        assert_eq!(
            *HANDED.lock().unwrap(),
            [(Ask::Reveal, FILE.to_owned())],
            "one reveal, of the address asked for"
        );
        assert_eq!(
            *DONE.lock().unwrap(),
            [Opened::Taken],
            "done heard the answer once"
        );
        assert_eq!(
            WAKES_IN_HAND.load(Relaxed),
            2,
            "the nested wake reached the responder inside the hand's loop"
        );
        assert_eq!(WAKES.load(Relaxed), 2, "and no other wake came");
        assert_eq!(
            caught(),
            Caught::NONE,
            "no panic was stopped: none happened"
        );
        println!("{}: ok", crate::NAME);
    }
}
