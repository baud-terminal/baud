//! **The event rig's child** (`tests/event_rig.rs`), built only under the
//! test-only `effect-probe` feature: a real `DrawableView` with baud's
//! `Driver` as its responder, in a process that shows nothing.
//!
//! `baud --event-rig <path|type|link|select|copy-menu> <arg> <iterations>
//! <tick|no-tick|tick-inline|tick-in-frame> <kind,kind...>` -- the last
//! the effects each iteration waits for, by their probe words, and a call
//! of any other shape exits 2 -- makes the window and never
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
//! the effect queue's turn runs inline, in the callback that asked or in the
//! next frame.

use super::*;
use baud_present_macos::probe;
use macos_liquid_glass::objc2::sel;
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
    /// A pointer gesture over what this line, typed once first, drew.
    Gesture(Gesture, Vec<u8>),
}

/// A gesture over the screen's first row, sent to the view's own methods
/// (`probe::press`, `probe::menu`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gesture {
    /// Cmd-click on the second cell, a link's.
    LinkClick,
    /// A drag from the first cell to the fourth.
    Select,
    /// That drag, then the Edit menu's Copy.
    CopyMenu,
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
    let [cause, arg, iterations, ticks, each] = match call(&args) {
        Call::NotRig => return None,
        Call::Malformed => return Some(std::process::ExitCode::from(2)),
        Call::Rig(parts) => parts,
    };
    let act = match cause {
        "path" => Act::Click(arg.as_bytes().to_vec()),
        "type" => Act::Type([arg.as_bytes(), b"\n"].concat()),
        "link" => Act::Gesture(Gesture::LinkClick, [arg.as_bytes(), b"\n"].concat()),
        "select" => Act::Gesture(Gesture::Select, [arg.as_bytes(), b"\n"].concat()),
        "copy-menu" => Act::Gesture(Gesture::CopyMenu, [arg.as_bytes(), b"\n"].concat()),
        _ => return Some(std::process::ExitCode::from(2)),
    };
    let ticks = match ticks {
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
    // The effects each iteration waits for, by their words: `copy,announce`.
    let Some(each) = each
        .split(',')
        .map(probe::EffectKind::of)
        .collect::<Option<Vec<_>>>()
    else {
        return Some(std::process::ExitCode::from(2));
    };
    Some(run(act, iterations, ticks, &each))
}

/// A printed path's setup: a clear of the pasteboard, read by the shell.
const READY: &[u8] = b"printf '\\033]52;c;!\\007'\n";

/// What a process's arguments ask of the rig.
#[derive(PartialEq, Eq, Debug)]
enum Call<'a> {
    /// Not a rig call: a plain launch.
    NotRig,
    /// `--event-rig` of the wrong shape: refused, never taken for a plain
    /// launch, which would open a window.
    Malformed,
    /// Its five arguments.
    Rig([&'a str; 5]),
}

fn call(args: &[String]) -> Call<'_> {
    match args {
        [flag, ..] if flag != "--event-rig" => Call::NotRig,
        [] => Call::NotRig,
        [_, cause, arg, iterations, ticks, each] => {
            Call::Rig([cause, arg, iterations, ticks, each].map(String::as_str))
        }
        _ => Call::Malformed,
    }
}

/// Which iteration a gesture is.
#[derive(Clone, Copy)]
struct Turn(u16);

/// `g` sent to `view`, over the first row's first four cells. A
/// selection's drag ends on the fourth cell one iteration and the third
/// the next, so each makes a selection the last did not.
fn gesture(
    view: &DrawableView,
    g: Gesture,
    [first, second, third, fourth]: [NSPoint; 4],
    at: Turn,
) {
    use probe::{Chord, Press, press};
    match g {
        Gesture::LinkClick => {
            press(view, Press::Down, second, Chord::Command);
            press(view, Press::Up, second, Chord::Command);
        }
        Gesture::Select => drag(view, first, [fourth, third][usize::from(at.0 % 2)]),
        // The selection made once, by the first iteration's drag, and
        // copied by each iteration's Copy.
        Gesture::CopyMenu => {
            if at.0 == 0 {
                drag(view, first, fourth);
            }
            probe::menu(view, sel!(copy:));
        }
    }
}

/// A left-button drag from `from` to `to`.
fn drag(view: &DrawableView, from: NSPoint, to: NSPoint) {
    use probe::{Chord, Press, press};
    press(view, Press::Down, from, Chord::Plain);
    press(view, Press::Dragged, to, Chord::Plain);
    press(view, Press::Up, to, Chord::Plain);
}

/// A `wake:` posted from another thread and joined: certainly queued.
fn post(waker: Waker) {
    let _ = std::thread::spawn(move || waker.wake()).join();
}

/// Whether `got` holds each of `each`, as many times as `each` names it.
fn holds(got: &[probe::Record], each: &[probe::EffectKind]) -> bool {
    each.iter().all(|k| {
        let want = each.iter().filter(|e| *e == k).count();
        got.iter().filter(|r| r.kind == *k).count() >= want
    })
}

fn run(
    act: Act,
    iterations: Iterations,
    ticks: Ticks,
    each: &[probe::EffectKind],
) -> std::process::ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::process::ExitCode::from(3);
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    crate::os_effect::install();
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
    let cells = [0, 1, 2, 3].map(|col| driver.rig_point(Row::ZERO, Col::at_index(col)));
    view.set_responder(Box::new(baud_present_macos::unwind::Guarded(driver)));
    // The view the window's first responder, which tells baud it has the
    // keys and the pointer (`becomeFirstResponder`, `Focus::Gained`): a
    // link's Cmd-click is armed only then. The window is not made key,
    // nor shown, by it; the checks below say so every iteration.
    let _ = window.ns_window().makeFirstResponder(Some(&view));
    probe::set_waker(view.waker());
    // A gesture's screen, drawn once: the line typed, and its last effect,
    // a clear of the pasteboard (`OSC 52`) printed after what the gesture
    // is over, is the sign it has been read -- a clear, which says nothing,
    // so no announcement of the setup's is left to fall in an iteration. A
    // count, never a time; the bound is a failure.
    //
    // A printed path's click has the clear alone: the shell read it, so
    // the shell is the terminal's foreground program, which the paths
    // worker asks of the first click. A click before then -- the shell not
    // yet started, under load -- is answered unchecked, and said so.
    let ticker = probe::Ticker::new(&view);
    let setup = match &act {
        Act::Gesture(_, setup) => Some(setup.clone()),
        Act::Click(_) => Some(READY.to_vec()),
        Act::Type(_) => None,
    };
    if let Some(setup) = setup {
        TYPED.with(|c| *c.borrow_mut() = Some(setup));
        post(view.waker());
        let start = Instant::now();
        let mut ready = false;
        while !ready && start.elapsed() < BOUND {
            // A control's turn parked for the frame runs on a tick.
            if ticks == Ticks::Driven {
                ticker.tick();
            }
            probe::pass(0.01);
            ready = probe::take()
                .iter()
                .any(|r| r.kind == probe::EffectKind::Clear);
        }
        if !ready {
            println!("the row's screen was never drawn");
            return std::process::ExitCode::from(1);
        }
    }
    let mut failed = false;
    for i in 0..iterations.0 {
        // Kept, never dropped: whatever an iteration's records miss is the
        // parent's to see.
        let mut got = probe::take();
        match &act {
            Act::Click(file) => CLICK.with(|c| *c.borrow_mut() = Some(file.clone())),
            Act::Type(line) => TYPED.with(|c| *c.borrow_mut() = Some(line.clone())),
            Act::Gesture(g, _) => gesture(&view, *g, cells, Turn(i)),
        }
        post(view.waker());
        let start = Instant::now();
        // **Every effect the iteration expects, not the first record**: an
        // effect asked by another's outcome -- a copy's words, on the wake
        // after the copy's turn -- runs a turn later, and a load can put
        // that turn after the passes below, in the next iteration, where
        // the first record ended the wait before that iteration's own
        // effect had run.
        while !holds(&got, each) && start.elapsed() < BOUND {
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
            got.extend(probe::take());
        }
        // Whatever else is ready runs, so a second effect would be seen.
        for _ in 0..64 {
            probe::pass(0.0);
        }
        got.extend(probe::take());
        // A program's rows only: a click rings no bell.
        while let (Act::Type(_) | Act::Gesture(..), Some(left)) =
            (&act, SPACING.checked_sub(start.elapsed()))
        {
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
    // No panic stopped anywhere: a borrow taken twice in a nested turn
    // (the effect queue's re-entry) would be one, swallowed by its guard.
    if baud_present_macos::unwind::caught() != baud_present_macos::unwind::Caught::NONE {
        println!("a panic was caught");
        failed = true;
    }
    match failed {
        true => std::process::ExitCode::from(1),
        false => std::process::ExitCode::SUCCESS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A rig call of the wrong shape never launches baud**: a child given
    /// one argument too few or too many by a script that fell behind would
    /// otherwise open a real window. The control is the plain launch and
    /// the call of the right shape. Perturbation: answer `NotRig` for a
    /// short call.
    #[test]
    fn a_malformed_rig_call_is_refused() {
        let of = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(call(&of(&[])), Call::NotRig);
        assert_eq!(call(&of(&["README.md"])), Call::NotRig);
        let short = of(&["--event-rig", "path", "/a", "3", "tick"]);
        assert_eq!(call(&short), Call::Malformed);
        let long = of(&["--event-rig", "path", "/a", "3", "tick", "copy", "x"]);
        assert_eq!(call(&long), Call::Malformed);
        assert_eq!(call(&of(&["--event-rig"])), Call::Malformed);
        let right = of(&["--event-rig", "path", "/a", "3", "tick", "copy"]);
        assert_eq!(call(&right), Call::Rig(["path", "/a", "3", "tick", "copy"]));
    }
}
