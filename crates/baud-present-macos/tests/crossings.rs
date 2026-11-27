//! **A light's tracking area queues each crossing at the pointer's point**
//! (`baud_present_macos::hover`): a view watched carries one area,
//! entered and exited always, in its visible rect, and what AppKit sends
//! that area's owner -- `mouseEntered:` and `mouseExited:`, each with its
//! event -- is queued at the event's point, in order, and emptied by
//! `take`; with no window, as these events have, the point as given (a
//! window's goes to the screen, `tests/on_screen.rs`). The view is made
//! and never put in a window, so nothing is shown; the events are built
//! by `NSEvent`'s own constructor and sent to the area's own owner, as
//! AppKit sends them.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.
//! Perturbations: watch with `ActiveInKeyWindow` and the options' check
//! fails; queue nothing on an exit and the order's fails.

const NAME: &str = "a_watched_view_queues_each_crossing_at_its_point";

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
        a_watched_view_queues_each_crossing_at_its_point();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = skipped;
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn a_watched_view_queues_each_crossing_at_its_point() {
    use baud_present_macos::hover::Crossings;
    use macos_liquid_glass::objc2::{MainThreadOnly, msg_send};
    use macos_liquid_glass::objc2_app_kit::{
        NSEvent, NSEventModifierFlags, NSEventType, NSTrackingAreaOptions, NSView,
    };
    use macos_liquid_glass::objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

    let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(40.0, 28.0)),
    );
    let crossings = Crossings::new(mtm, None);
    assert!(view.trackingAreas().is_empty(), "the control: none before");
    crossings.watch(&view);
    let areas = view.trackingAreas();
    assert_eq!(areas.len(), 1, "one area per watched view");
    let area = areas.objectAtIndex(0);
    assert_eq!(
        area.options(),
        NSTrackingAreaOptions::MouseEnteredAndExited
            | NSTrackingAreaOptions::ActiveAlways
            | NSTrackingAreaOptions::InVisibleRect,
        "entered and exited, always, in the visible rect"
    );
    let owner = area.owner().expect("the area's owner is alive");
    let event = |kind, x, y| {
        // SAFETY: an enter or exit event with no window, no context and no
        // user data, `NSEvent`'s documented constructor.
        unsafe {
            NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
                kind,
                NSPoint::new(x, y),
                NSEventModifierFlags::empty(),
                0.0,
                0,
                None,
                0,
                0,
                std::ptr::null_mut(),
            )
        }
        .expect("an event")
    };
    let (enter, exit) = (
        event(NSEventType::MouseEntered, 12.0, 700.0),
        event(NSEventType::MouseExited, 13.5, 690.0),
    );
    // SAFETY: the two messages these options make AppKit send the area's
    // owner, each with an event of its kind.
    unsafe {
        let _: () = msg_send![&*owner, mouseEntered: &*enter];
        let _: () = msg_send![&*owner, mouseExited: &*exit];
    }
    assert_eq!(
        crossings.take(),
        vec![NSPoint::new(12.0, 700.0), NSPoint::new(13.5, 690.0)],
        "each crossing at its point, in order"
    );
    assert!(crossings.take().is_empty(), "taken");
    println!("test {NAME} ... ok");
}
