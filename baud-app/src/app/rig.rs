//! **The event rig's child** (`tests/event_rig.rs`), built only under the
//! test-only `effect-probe` feature: a real `DrawableView` with baud's
//! `Driver` as its responder, in a process that shows nothing.
//!
//! `baud --event-rig <file> <iterations> <tick|no-tick|tick-inline>`
//! makes the window
//! and never orders it in, under the prohibited activation policy, so
//! there is no Dock icon and nothing takes focus. Each iteration clicks
//! `file` as a printed path: the click is taken on the view's `wake`, and
//! the paths worker's real thread checks the file and wakes the view with
//! its answer. Under `tick`, each pass also posts a `wake:` from another
//! thread, joined so it is queued, and sends the view's own `tick:` with a
//! `CADisplayLink` that is never added to a run loop: the display link's
//! code path, with no vsync to wait on. The effect the click comes to is
//! recorded where it runs (`baud_present_macos::probe`), and each record
//! is printed as a line for the parent; an iteration with none prints
//! `none`. `tick-inline` is the control: `link::later` hands over inline,
//! so a reveal runs inside the callback that asked, as it did before
//! `later`.

use super::*;
use baud_present_macos::probe;
use std::cell::RefCell;

/// Whether each pass drives the view's frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ticks {
    Driven,
    None,
}

thread_local! {
    /// The path the next `wake` clicks.
    static CLICK: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// The click a `wake` takes, if the rig queued one.
pub(super) fn take_click() -> Option<Vec<u8>> {
    CLICK.with(|c| c.borrow_mut().take())
}

/// How many times the rig clicks.
#[derive(Clone, Copy)]
struct Iterations(u16);

/// The bound on one iteration: reaching it is that iteration's failure.
const BOUND: Duration = Duration::from_secs(2);

/// `--event-rig` and its arguments, or none when the process was not
/// started as the rig.
pub(crate) fn asked() -> Option<std::process::ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [flag, file, iterations, ticks] = args.as_slice() else {
        return None;
    };
    if flag != "--event-rig" {
        return None;
    }
    let ticks = match ticks.as_str() {
        "tick" => Ticks::Driven,
        "no-tick" => Ticks::None,
        // The control: the hand-over made inline, in the callback.
        "tick-inline" => {
            probe::set_inline(probe::Inline::Yes);
            Ticks::Driven
        }
        _ => return Some(std::process::ExitCode::from(2)),
    };
    let Ok(iterations) = iterations.parse().map(Iterations) else {
        return Some(std::process::ExitCode::from(2));
    };
    Some(run(file.as_bytes().to_vec(), iterations, ticks))
}

/// A `wake:` posted from another thread and joined: certainly queued.
fn post(waker: Waker) {
    let _ = std::thread::spawn(move || waker.wake()).join();
}

fn run(file: Vec<u8>, iterations: Iterations, ticks: Ticks) -> std::process::ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::process::ExitCode::from(3);
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let size = WINDOW_SIZE;
    let scale = 2.0;
    let inset = predicted_inset(mtm, size);
    let shell = Shell::start(scale, size, inset, None);
    let Opening {
        mut driver,
        view,
        window,
        ..
    } = Opening::new(mtm, size, scale, inset, shell, None);
    driver.pump(Trigger::Opening);
    view.set_responder(Box::new(baud_present_macos::unwind::Guarded(driver)));
    probe::set_waker(view.waker());
    let ticker = probe::Ticker::new(&view);
    let mut failed = false;
    for _ in 0..iterations.0 {
        let _ = probe::take();
        CLICK.with(|c| *c.borrow_mut() = Some(file.clone()));
        post(view.waker());
        let start = Instant::now();
        let mut got = Vec::new();
        while got.is_empty() && start.elapsed() < BOUND {
            if ticks == Ticks::Driven {
                // Queued, and shown not yet delivered: nothing runs the
                // run loop between the post and the frame.
                let before = probe::wakes();
                post(view.waker());
                if probe::wakes() != before {
                    println!("wake delivered before the frame");
                    failed = true;
                }
                ticker.tick();
            }
            probe::pass(0.01);
            got = probe::take();
        }
        // Whatever else is ready runs, so a second effect would be seen.
        for _ in 0..64 {
            probe::pass(0.0);
        }
        got.extend(probe::take());
        if got.is_empty() {
            println!("none");
        }
        for r in &got {
            println!("{}", r.line());
        }
    }
    if window.ns_window().isVisible() {
        println!("the window was shown");
        failed = true;
    }
    match failed {
        true => std::process::ExitCode::from(1),
        false => std::process::ExitCode::SUCCESS,
    }
}
