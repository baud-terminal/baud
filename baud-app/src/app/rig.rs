//! **The event rig's child** (`tests/event_rig.rs`), built only under the
//! test-only `effect-probe` feature: a real `DrawableView` with baud's
//! `Driver` as its responder, in a process that shows nothing.
//!
//! `baud --event-rig <path|type> <arg> <iterations>
//! <tick|no-tick|tick-inline|tick-in-frame>` makes the window and never
//! orders it in, under the prohibited activation policy, so there is no
//! Dock icon and nothing takes focus; after every iteration the window is
//! checked never shown and never key. Each iteration does the row's act,
//! taken on the view's `wake`:
//! - `path`: clicks `arg` as a printed path, and the paths worker's real
//!   thread checks the file and wakes the view with its answer;
//! - `type`: sends `arg` and a newline to the shell as typed keys are
//!   sent, and what the shell runs writes back through the pty, as a
//!   program's output does.
//!
//! Under `tick`, each pass also posts a `wake:` from another thread,
//! joined so it is queued, and sends the view's own `tick:` with a
//! `CADisplayLink` that is never added to a run loop: the display link's
//! code path, with no vsync to wait on. The effects the act comes to are
//! recorded where they run (`baud_present_macos::probe`), and each record
//! is printed as a line for the parent; an iteration with none prints
//! `none`. `tick-inline` and `tick-in-frame` are the controls:
//! `link::later` hands over inline, in the callback that asked or in the
//! next frame.

use super::*;
use baud_present_macos::probe;
use std::cell::RefCell;

/// Whether each pass drives the view's frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ticks {
    Driven,
    None,
}

/// What each iteration does.
#[derive(Clone)]
enum Act {
    /// Click this file as a printed path.
    Click(Vec<u8>),
    /// Type this line to the shell, as keys are sent: what it runs writes
    /// back through the pty, as a program's output does.
    Type(Vec<u8>),
}

thread_local! {
    /// The path the next `wake` clicks.
    static CLICK: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// The line the next `wake` sends the shell.
    static TYPED: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
}

/// The click a `wake` takes, if the rig queued one.
pub(super) fn take_click() -> Option<Vec<u8>> {
    CLICK.with(|c| c.borrow_mut().take())
}

/// The line a `wake` sends the shell, if the rig queued one.
pub(super) fn take_typed() -> Option<Vec<u8>> {
    TYPED.with(|c| c.borrow_mut().take())
}

/// How many times the rig clicks.
#[derive(Clone, Copy)]
struct Iterations(u16);

/// The bound on one iteration: reaching it is that iteration's failure.
const BOUND: Duration = Duration::from_secs(2);

/// **An iteration's shortest length**, its tail spent running the run
/// loop: longer than the frame interval within which bells ring once
/// (`bell.rs`, `Ringer::hear`). An undriven view keeps the interval it
/// starts with, 8.3 ms, and the bell's rows are undriven, so a bell each
/// iteration is a ring each iteration. A typed line's rows only. A floor on time passed, never a
/// wait for something to happen: what every iteration waits for is its
/// records, under [`BOUND`].
const SPACING: Duration = Duration::from_millis(50);

/// `--event-rig` and its arguments, or none when the process was not
/// started as the rig.
pub(crate) fn asked() -> Option<std::process::ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [flag, cause, arg, iterations, ticks] = args.as_slice() else {
        return None;
    };
    if flag != "--event-rig" {
        return None;
    }
    let act = match cause.as_str() {
        "path" => Act::Click(arg.as_bytes().to_vec()),
        "type" => Act::Type([arg.as_bytes(), b"\n"].concat()),
        _ => return Some(std::process::ExitCode::from(2)),
    };
    let ticks = match ticks.as_str() {
        "tick" => Ticks::Driven,
        "no-tick" => Ticks::None,
        // The controls: the hand-over made inline, in the callback, or in
        // the next frame.
        "tick-inline" => {
            probe::set_inline(probe::Inline::InCallback);
            Ticks::Driven
        }
        "tick-in-frame" => {
            probe::set_inline(probe::Inline::InFrame);
            Ticks::Driven
        }
        _ => return Some(std::process::ExitCode::from(2)),
    };
    let Ok(iterations) = iterations.parse().map(Iterations) else {
        return Some(std::process::ExitCode::from(2));
    };
    Some(run(act, iterations, ticks))
}

/// A `wake:` posted from another thread and joined: certainly queued.
fn post(waker: Waker) {
    let _ = std::thread::spawn(move || waker.wake()).join();
}

fn run(act: Act, iterations: Iterations, ticks: Ticks) -> std::process::ExitCode {
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
        match &act {
            Act::Click(file) => CLICK.with(|c| *c.borrow_mut() = Some(file.clone())),
            Act::Type(line) => TYPED.with(|c| *c.borrow_mut() = Some(line.clone())),
        }
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
        // A program's rows only: a click rings no bell.
        while let (Act::Type(_), Some(left)) = (&act, SPACING.checked_sub(start.elapsed())) {
            probe::pass(left.as_secs_f64());
        }
        got.extend(probe::take());
        if got.is_empty() {
            println!("none");
        }
        for r in &got {
            println!("{}", r.line());
        }
        // Nothing reaches the screen: the window is never shown, never
        // key, after every iteration.
        if window.ns_window().isVisible() {
            println!("the window was shown");
            failed = true;
        }
        if window.ns_window().isKeyWindow() {
            println!("the window was made key");
            failed = true;
        }
    }
    match failed {
        true => std::process::ExitCode::from(1),
        false => std::process::ExitCode::SUCCESS,
    }
}
