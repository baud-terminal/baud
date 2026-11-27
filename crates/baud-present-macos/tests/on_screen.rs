//! **A light's frame lands where the light is, in the terminal's view,
//! whichever window holds the light** (`baud_present_macos::hover`): read
//! on the screen through the light's own window (`on_screen`) and brought
//! into the terminal's view through the terminal's (`in_view`). In full
//! screen the title bar is a window of its own, so the lights' window is
//! not the terminal's; in a window the two are one.
//!
//! Full screen is two windows built and never shown, as on a second
//! display right of a 1512-point main one: the terminal at (1512, 0)
//! 1512x982 and the title bar at (1512, 930) 52 tall, a light at
//! (1300, 12) 20x28 in the title bar's. It must lie in the terminal's view
//! where the title bar window puts it, 930 points up, which reading the
//! light's window coordinates as the terminal's misses by those 930; off
//! the screen's corner, so each half of the route is needed. A
//! window is one window, a light in its own content: both readings agree,
//! so the route through the screen changes nothing there. A crossing heard
//! in the title bar's window is queued at its screen point too.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.
//! Perturbations: `on_screen` without `convertRectToScreen`, `in_view`
//! without `convertRectFromScreen`, and a crossing queued at its window
//! point, each red.

const NAMES: [&str; 2] = [
    "a_light_in_the_full_screen_title_bar_lands_where_its_window_puts_it",
    "a_light_in_the_terminal_window_lands_where_it_did",
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && !args.iter().any(|a| a == "--ignored") {
            for name in NAMES {
                println!("{name}: test");
            }
        }
        return;
    }
    let filter = args.iter().find(|a| !a.starts_with('-'));
    let runs = |name: &str| filter.is_none_or(|f| name.contains(f.as_str()));
    #[cfg(target_os = "macos")]
    {
        if runs(NAMES[0]) {
            a_light_in_the_full_screen_title_bar_lands_where_its_window_puts_it();
            println!("test {} ... ok", NAMES[0]);
        }
        if runs(NAMES[1]) {
            a_light_in_the_terminal_window_lands_where_it_did();
            println!("test {} ... ok", NAMES[1]);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = runs;
}

#[cfg(target_os = "macos")]
use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect, NSSize};

/// A window at `origin`, `size`, never shown.
#[cfg(target_os = "macos")]
fn window(
    mtm: macos_liquid_glass::objc2::MainThreadMarker,
    origin: NSPoint,
    size: NSSize,
    title: &str,
) -> macos_liquid_glass::window::GlassWindow {
    let w = macos_liquid_glass::window::GlassWindow::new(mtm, size, title);
    w.ns_window().setFrameOrigin(origin);
    assert!(!w.ns_window().isVisible(), "never shown");
    w
}

/// A view at `frame` in `parent`.
#[cfg(target_os = "macos")]
fn put(
    mtm: macos_liquid_glass::objc2::MainThreadMarker,
    parent: &macos_liquid_glass::objc2_app_kit::NSView,
    frame: NSRect,
) -> macos_liquid_glass::objc2::rc::Retained<macos_liquid_glass::objc2_app_kit::NSView> {
    use macos_liquid_glass::objc2::MainThreadOnly;
    use macos_liquid_glass::objc2_app_kit::NSView;
    let v = NSView::initWithFrame(NSView::alloc(mtm), frame);
    parent.addSubview(&v);
    v
}

#[cfg(target_os = "macos")]
fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn a_light_in_the_full_screen_title_bar_lands_where_its_window_puts_it() {
    use baud_present_macos::hover::{Crossings, in_view, on_screen};
    use macos_liquid_glass::objc2::msg_send;
    use macos_liquid_glass::objc2_app_kit::{
        NSApplication, NSEvent, NSEventModifierFlags, NSEventType,
    };
    use macos_liquid_glass::objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    let term = window(
        mtm,
        NSPoint::new(1512.0, 0.0),
        NSSize::new(1512.0, 982.0),
        "terminal",
    );
    let bar = window(
        mtm,
        NSPoint::new(1512.0, 930.0),
        NSSize::new(1512.0, 52.0),
        "title bar",
    );
    let content = term.ns_window().contentView().expect("a content view");
    // The terminal's view, in from its window's corner, as the grid's is.
    let view = put(mtm, &content, rect(0.0, 0.0, 1512.0, 930.0));
    let holder = bar.ns_window().contentView().expect("a content view");
    let light = put(mtm, &holder, rect(1300.0, 12.0, 20.0, 28.0));

    // Where the light is in the terminal window's coordinates, from the
    // two windows' frames alone: a window's coordinates are the screen's
    // less its frame's origin.
    let (tf, bf) = (term.ns_window().frame(), bar.ns_window().frame());
    assert_eq!(
        bf.origin.y - tf.origin.y,
        930.0,
        "the control: two windows 930 apart"
    );
    let own = light.convertRect_toView(light.bounds(), None);
    let in_term = rect(
        own.origin.x + bf.origin.x - tf.origin.x,
        own.origin.y + bf.origin.y - tf.origin.y,
        own.size.width,
        own.size.height,
    );
    let want = view.convertRect_fromView(in_term, None);

    let screen = on_screen(&light).expect("a light in a window is on the screen");
    let got = in_view(term.ns_window(), &view, screen);
    assert_eq!(got, want, "the light where the title bar's window puts it");
    let misread = view.convertRect_fromView(own, None);
    assert_ne!(
        misread, want,
        "the control: the light's window coordinates read as the terminal's miss it"
    );
    assert_eq!(
        (want.origin.y - misread.origin.y).abs(),
        930.0,
        "by the windows' distance"
    );

    // A crossing heard in the title bar's window, at its screen point.
    let crossings = Crossings::new(mtm, None);
    crossings.watch(&light);
    let areas = light.trackingAreas();
    let owner = areas
        .objectAtIndex(0)
        .owner()
        .expect("the area's owner is alive");
    let number = bar.ns_window().windowNumber();
    // SAFETY: an enter event in the title bar's window, with no context and
    // no user data, `NSEvent`'s documented constructor.
    let enter = unsafe {
        NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
            NSEventType::MouseEntered,
            NSPoint::new(1310.0, 20.0),
            NSEventModifierFlags::empty(),
            0.0,
            number,
            None,
            0,
            0,
            std::ptr::null_mut(),
        )
    }
    .expect("an event");
    assert_eq!(
        enter.window(mtm).as_deref().map(|w| w as *const _),
        Some(bar.ns_window() as *const _),
        "the control: the event is the title bar window's"
    );
    // SAFETY: the message these options make AppKit send the area's owner,
    // with an event of its kind.
    unsafe {
        let _: () = msg_send![&*owner, mouseEntered: &*enter];
    }
    assert_eq!(
        crossings.take(),
        vec![NSPoint::new(1310.0 + bf.origin.x, 20.0 + bf.origin.y)],
        "the crossing at its screen point"
    );
}

#[cfg(target_os = "macos")]
fn a_light_in_the_terminal_window_lands_where_it_did() {
    use baud_present_macos::hover::{in_view, on_screen};
    use macos_liquid_glass::objc2_app_kit::NSApplication;
    use macos_liquid_glass::objc2_foundation::MainThreadMarker;

    let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    let term = window(
        mtm,
        NSPoint::new(200.0, 140.0),
        NSSize::new(900.0, 600.0),
        "terminal",
    );
    let content = term.ns_window().contentView().expect("a content view");
    let view = put(mtm, &content, rect(0.0, 0.0, 900.0, 572.0));
    // The title bar's accessory, in the same window, above the view.
    let holder = put(mtm, &content, rect(600.0, 572.0, 300.0, 28.0));
    let light = put(mtm, &holder, rect(100.0, 0.0, 20.0, 28.0));

    let want = view.convertRect_fromView(light.convertRect_toView(light.bounds(), None), None);
    let screen = on_screen(&light).expect("a light in a window is on the screen");
    let f = term.ns_window().frame();
    assert!(
        f.origin.x != 0.0 && f.origin.y != 0.0,
        "the control: the window is off the screen's corner, so the screen's \
         coordinates are not the window's"
    );
    assert_eq!(
        in_view(term.ns_window(), &view, screen),
        want,
        "in one window the route through the screen gives what the window's coordinates gave"
    );
    let loose = {
        use macos_liquid_glass::objc2::MainThreadOnly;
        macos_liquid_glass::objc2_app_kit::NSView::initWithFrame(
            macos_liquid_glass::objc2_app_kit::NSView::alloc(mtm),
            rect(0.0, 0.0, 1.0, 1.0),
        )
    };
    assert_eq!(
        on_screen(&loose),
        None,
        "a view in no window is on no screen"
    );
}
