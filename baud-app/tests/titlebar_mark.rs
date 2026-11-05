//! **baud's lockup and activity lights sit in the title bar, beside the
//! traffic lights and clear of each other, at every size the window opens
//! at, follow the window's light or dark, and the grid's top follows full
//! screen**: headless, in real windows built as `Opening::new` and
//! `Opening::show` build baud's -- the window at its default size and then
//! at the grid's, the container at the size the window was built at, the
//! terminal's view inside it, the lockup put by `baud-app/src/mark.rs`
//! itself, the lights' accessory put and fitted by `baud-app/src/strip.rs`,
//! then the body under them -- and never shown.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.
//!
//! Each window is checked as baud has it at each step: once the lockup is
//! put, once the body (the standard material, and the glass) has the
//! container, after a resize re-places both as `Driver::relayout` does,
//! and in both appearances. The lockup must lie in its accessory's bounds,
//! whole, the smallest window's included; in the window's coordinates,
//! between the top of the content layout rect and the window's top, which
//! is the title bar, a button's gap past the zoom button, as AppKit puts a
//! leading accessory, and level with the traffic lights; the shape of its
//! picture; its mark's cell the size the mark alone was drawn at; and in
//! the title bar's own views, which it shares with the traffic lights and
//! the content does not. The lights' accessory is flush with the window's
//! right edge, the lights end the close button's gap short of it, and it
//! starts clear of the lockup, which in the smallest window means the
//! leftmost lights have no room. Under Aqua the lockup shows the dark
//! word and under Dark Aqua the light one, swapped on the flip with the
//! frame unmoved.
//!
//! **Full screen is not a resize.** The title bar leaves the window, into
//! a window of its own that is shown only when the pointer reaches the top
//! of the screen, so content that should go with it has to be in it: the
//! accessories' place in the title bar's views is that check. A window
//! cannot be put in full screen without being shown, so the grid's top is
//! checked by posting the two notifications AppKit posts at the end of a
//! transition, through baud's own observer (`full_screen.rs`) and its own
//! response (`strip::follow`): entered, the terminal's view reaches the
//! window's top; exited, it stops the title bar's height short again.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/mark.rs"]
mod mark;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/strip.rs"]
mod strip;

const NAME: &str = "the_lockup_sits_in_the_titlebar_strip";

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
        run();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = skipped;
}

#[cfg(target_os = "macos")]
use baud_present_macos::full_screen;
#[cfg(target_os = "macos")]
use macos_liquid_glass::{
    drawable::Appearance,
    glass::{GlassStyle, GlassSurface},
    material::{Material, MaterialSurface},
    objc2::{MainThreadOnly, Message},
    objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSAutoresizingMaskOptions, NSView,
        NSWindow, NSWindowButton,
    },
    objc2_core_foundation::CGFloat,
    objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString},
    window::{GlassWindow, TitlebarSide},
};

/// The size a window opens at, `app.rs`'s `WINDOW_SIZE`.
#[cfg(target_os = "macos")]
const WINDOW_SIZE: NSSize = NSSize::new(960.0, 600.0);

/// The smallest window, `app.rs`'s `set_min_size`.
#[cfg(target_os = "macos")]
const SMALLEST: NSSize = NSSize::new(200.0, 100.0);

/// The nine lights' span, in points. What the renderer lays them out at
/// is the renderer's (`lights::spans`); any span too long for the
/// smallest window to hold whole checks the same thing.
#[cfg(target_os = "macos")]
const SPANS_WIDTH: CGFloat = 180.25;

/// The plate's pad before the first light, `baud_render::lights::PLATE_PAD`.
#[cfg(target_os = "macos")]
const PAD: CGFloat = 4.0;

/// What the container is put in.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
enum Body {
    Standard,
    Glass,
}

#[cfg(target_os = "macos")]
fn run() {
    let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    assert_eq!(PAD, CGFloat::from(baud_render::lights::PLATE_PAD));
    // The README film's window, the default one, a pinned grid taller
    // than the default, and the smallest; each then resized to the next.
    let sizes = [
        NSSize::new(786.0, 572.0),
        WINDOW_SIZE,
        NSSize::new(1200.0, 900.0),
        SMALLEST,
    ];
    let mut checked = 0;
    for body in [Body::Standard, Body::Glass] {
        for (i, &content) in sizes.iter().enumerate() {
            let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
            let w = window.ns_window();
            if content != WINDOW_SIZE {
                w.setContentSize(content);
            }
            let inset = strip::inset_of(w);
            assert!(inset > 10.0, "a titled window has a titlebar: {inset}");
            let built = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
            let container = NSView::initWithFrame(NSView::alloc(mtm), built);
            container.setAutoresizesSubviews(true);
            let view = NSView::initWithFrame(NSView::alloc(mtm), built);
            strip::apply(&view, &container, inset);
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            container.addSubview(&view);
            window.set_content_view(&container);
            let mark = mark::Mark::put(mtm, &window, inset).expect("the lockups decode");
            let lights = strip::Accessory::put(
                mtm,
                &window,
                TitlebarSide::Trailing,
                NSSize::new(0.0, inset),
            );
            // Put in the form for the window's appearance as it is.
            let now = if macos_liquid_glass::is_dark(&w.effectiveAppearance()) {
                Appearance::Dark
            } else {
                Appearance::Light
            };
            assert_eq!(mark.shown(), mark::Lockup::for_appearance(now));
            let at = |step: &str| format!("{body:?} body, {content:?}, {step}");
            // The re-place `relayout` makes: the lockup centred, and the
            // lights' accessory fitted to the edges as they are now.
            let place = || {
                w.layoutIfNeeded();
                mark.place();
                let edges = strip::edges(w, Some(mark.end()));
                let (room, least) =
                    lights.fit_lights(edges, SPANS_WIDTH, PAD, w.backingScaleFactor());
                (room, least, edges)
            };
            let room = place();
            check(w, &view, &mark, &lights, room, inset, &at("put"));
            // The body takes the container, and both are re-placed.
            let glass;
            let material;
            match body {
                Body::Standard => {
                    material = MaterialSurface::new(mtm, built, Material::UnderWindowBackground);
                    window.set_content_view(material.view());
                    material.set_content_view(&container);
                }
                Body::Glass => {
                    glass = GlassSurface::new(mtm, built, GlassStyle::Regular, 16.0)
                        .expect("macOS 26 has NSGlassEffectView");
                    window.set_content_view(glass.view());
                    glass.set_content_view(&container);
                }
            }
            let room = place();
            check(w, &view, &mark, &lights, room, inset, &at("under the body"));
            // A resize, as a drag or a zoom gives one.
            let next = sizes[(i + 1) % sizes.len()];
            w.setContentSize(next);
            let room = place();
            check(w, &view, &mark, &lights, room, inset, &at("resized"));
            follows_full_screen(w, &view, &container, inset, &at("full screen"));
            // A flip of appearance swaps the picture and moves nothing.
            let placed = mark.view().frame();
            let mut pictures = Vec::new();
            for (name, want) in [
                ("NSAppearanceNameAqua", mark::Lockup::OnLight),
                ("NSAppearanceNameDarkAqua", mark::Lockup::OnDark),
                ("NSAppearanceNameAqua", mark::Lockup::OnLight),
            ] {
                let a = NSAppearance::appearanceNamed(&NSString::from_str(name))
                    .expect("a stock appearance");
                window.set_appearance(Some(&a));
                mark.follow_window(w);
                assert_eq!(mark.shown(), want, "{}", at(name));
                pictures.push(mark.view().image().expect("a picture"));
                check(w, &view, &mark, &lights, room, inset, &at(name));
                assert_eq!(mark.view().frame(), placed, "{}", at(name));
            }
            assert!(
                pictures[0] != pictures[1],
                "{}: the flip swapped nothing",
                at("flip")
            );
            assert!(pictures[0] == pictures[2], "{}: the flip back", at("flip"));
            window.set_appearance(None);
            if std::env::var_os("BAUD_STRIP_PROBE").is_some() {
                probe(w, &mark, &lights, room, inset, &at("probe"));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 8, "two bodies at four sizes");
    println!("{NAME}: ok, {checked} windows");
}

/// **The grid's top through a full screen and back**, driven by posting
/// AppKit's two notifications by hand to baud's observer, whose block is
/// baud's response: the terminal's view reaches the window's top once
/// entered and stops the title bar's height short once exited; an
/// observer dropped hears nothing.
#[cfg(target_os = "macos")]
fn follows_full_screen(w: &NSWindow, view: &NSView, container: &NSView, inset: f64, at: &str) {
    const EPS: f64 = 1e-6;
    let top = || {
        let f = view.frame();
        container.frame().size.height - (f.origin.y + f.size.height)
    };
    assert!(
        (top() - inset).abs() < EPS,
        "{at}: windowed, the grid's top is {}",
        top()
    );
    let observer = {
        let (win, v, c) = (w.retain(), view.retain(), container.retain());
        full_screen::when_full_screen(w, move |change| strip::follow(change, &win, &v, &c))
    };
    post(w, Posted::Entered);
    assert!(
        top().abs() < EPS,
        "{at}: entered, the grid's top is {}, not the window's",
        top()
    );
    assert!(
        (view.frame().size.width - container.frame().size.width).abs() < EPS,
        "{at}: entered, the grid is not the window's width"
    );
    post(w, Posted::Exited);
    assert!(
        (top() - strip::inset_of(w)).abs() < EPS && (top() - inset).abs() < EPS,
        "{at}: exited, the grid's top is {}, not the title bar's {inset}",
        top()
    );
    drop(observer);
    post(w, Posted::Entered);
    assert!(
        (top() - inset).abs() < EPS,
        "{at}: a dropped observer still moved the grid"
    );
}

/// The notification AppKit posts at the end of a transition.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum Posted {
    Entered,
    Exited,
}

/// **AppKit's notification for `posted`, posted by hand** for `w`, under
/// AppKit's own name rather than baud's reading of it, so a name baud
/// reads the wrong way round is caught here.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
fn post(w: &NSWindow, posted: Posted) {
    use macos_liquid_glass::objc2::runtime::AnyObject;
    use macos_liquid_glass::objc2_app_kit::{
        NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
    };
    use macos_liquid_glass::objc2_foundation::NSNotificationCenter;
    let object: &AnyObject = w;
    // SAFETY: framework constants, valid strings for the process's life,
    // and a live window as the object; the observers run on this thread
    // before the post returns.
    unsafe {
        let name = match posted {
            Posted::Entered => NSWindowDidEnterFullScreenNotification,
            Posted::Exited => NSWindowDidExitFullScreenNotification,
        };
        NSNotificationCenter::defaultCenter().postNotificationName_object(name, Some(object));
    }
}

/// **What the window has, printed** with `BAUD_STRIP_PROBE` set: the
/// positions the change reports.
#[cfg(target_os = "macos")]
fn probe(
    w: &NSWindow,
    mark: &mark::Mark,
    lights: &strip::Accessory,
    (room, least, edges): (NSSize, f64, strip::Edges),
    inset: f64,
    at: &str,
) {
    let button = |b| {
        let v = w
            .standardWindowButton(b)
            .expect("a titled window has its buttons");
        v.convertRect_toView(v.bounds(), None)
    };
    let lw = lights.in_window();
    eprintln!(
        "PROBE {at}: win {:?} inset {inset} close {:?} zoom {:?} mark {:?} lights accessory {lw:?} room {room:?} least {least} lights_left {} lights_right_edge {}",
        w.frame().size,
        button(NSWindowButton::CloseButton),
        button(NSWindowButton::ZoomButton),
        mark.view().convertRect_toView(mark.view().bounds(), None),
        mark.lights_left(),
        lw.origin.x + room.width - edges.right,
    );
}

/// **The lockup and the lights where baud's window has them**: see the
/// module's doc.
#[cfg(target_os = "macos")]
fn check(
    w: &NSWindow,
    view: &NSView,
    mark: &mark::Mark,
    lights: &strip::Accessory,
    (room, least, edges): (NSSize, f64, strip::Edges),
    inset: f64,
    at: &str,
) {
    const EPS: f64 = 1e-6;
    // The layout AppKit makes before it draws: right after the body goes
    // in, the content layout rect reads 572 points low until it runs, and
    // it places the accessories.
    w.layoutIfNeeded();
    let holder = &mark.accessory().view;
    let m = mark.view().frame();
    let b = holder.bounds();
    assert!(
        m.origin.x >= b.origin.x - EPS
            && m.origin.y >= b.origin.y - EPS
            && m.origin.x + m.size.width <= b.origin.x + b.size.width + EPS
            && m.origin.y + m.size.height <= b.origin.y + b.size.height + EPS,
        "{at}: the mark {m:?} is outside its accessory's bounds {b:?}"
    );
    let mw = mark.view().convertRect_toView(mark.view().bounds(), None);
    let layout = w.contentLayoutRect();
    let strip = layout.origin.y + layout.size.height;
    let top = w.frame().size.height;
    assert!(
        (top - strip - inset).abs() < EPS,
        "{at}: the strip is {strip}-{top}, not {inset} tall"
    );
    assert!(
        mw.origin.y >= strip - EPS && mw.origin.y + mw.size.height <= top + EPS,
        "{at}: the mark {mw:?} in the window is not in the titlebar's strip {strip}-{top}"
    );
    let button = |b| {
        let v = w
            .standardWindowButton(b)
            .expect("a titled window has its buttons");
        (v.convertRect_toView(v.bounds(), None), v)
    };
    let (c, close) = button(NSWindowButton::CloseButton);
    let (mini, _) = button(NSWindowButton::MiniaturizeButton);
    let (z, _) = button(NSWindowButton::ZoomButton);
    // AppKit's leading place: a button's gap past the zoom button.
    let gap = mini.origin.x - (c.origin.x + c.size.width);
    assert!(
        (mw.origin.x - (z.origin.x + z.size.width + gap)).abs() < EPS,
        "{at}: the mark {mw:?} is not a button's gap {gap} past the zoom button {z:?}"
    );
    let (mc, zc) = (
        mw.origin.y + mw.size.height / 2.0,
        z.origin.y + z.size.height / 2.0,
    );
    assert!(
        (mc - zc).abs() <= z.size.height / 2.0,
        "{at}: the mark's centre {mc} is not level with the traffic lights' {zc}"
    );
    // In the title bar's own views: the nearest view each shares with the
    // close button holds no part of the content, as the title bar's view
    // does not and the window's frame view, which holds both, does.
    let content = w.contentView().expect("a content view");
    for (what, v) in [
        ("the mark's accessory", holder),
        ("the lights' accessory", &lights.view),
    ] {
        assert!(
            !v.isDescendantOf(&content),
            "{at}: {what} is in the content"
        );
        let shared = close
            .ancestorSharedWithView(v)
            .unwrap_or_else(|| panic!("{at}: {what} is not in the window's views"));
        assert!(
            !content.isDescendantOf(&shared),
            "{at}: {what} shares only the window's frame view with the traffic lights"
        );
        assert!(
            (v.frame().size.height - inset).abs() < EPS,
            "{at}: {what} is {} tall, not the title bar's {inset}",
            v.frame().size.height
        );
    }
    // The picture's shape, so it is drawn to the frame's edges: the
    // frame's right is the cursor's, and the lights start a light's gap
    // past it.
    let art = mark::Lockup::OnDark.art().expect("the lockup's size");
    assert!(
        (m.size.width / m.size.height - art.width / art.height).abs() < 1e-9,
        "{at}: the frame {m:?} is not the picture's shape {art:?}"
    );
    assert!(
        (mark.end() - (mw.origin.x + mw.size.width)).abs() < EPS,
        "{at}: the lockup ends at {}, its frame at {}",
        mark.end(),
        mw.origin.x + mw.size.width
    );
    let pair = f64::from(baud_render::lights::PAIR);
    assert!(
        (mark.lights_left() - mark.end() - pair).abs() < EPS,
        "{at}: the lights start at {}, not a light's gap past {}",
        mark.lights_left(),
        mark.end()
    );
    // The mark's 168-unit cell at the points per unit the mark alone had:
    // its 300-unit box across a square 1.4 lights tall.
    let side = (z.size.height * 1.4).min(inset - 4.0).max(16.0);
    let cell = m.size.height * 168.0 / art.height;
    assert!(
        (cell - side * 168.0 / 300.0).abs() < EPS,
        "{at}: the mark's cell is {cell} points, not {}",
        side * 168.0 / 300.0
    );
    // The lights: flush right, ending the close button's gap short of the
    // edge, as the traffic lights start that gap in from the other.
    let lw = lights.in_window();
    let width = w.frame().size.width;
    assert!(
        (lw.origin.x + lw.size.width - width).abs() < EPS,
        "{at}: the lights' accessory {lw:?} is not flush with the right edge {width}"
    );
    assert!(
        (room.width - lw.size.width).abs() < EPS && (room.height - lw.size.height).abs() < EPS,
        "{at}: the room {room:?} is not the accessory's {lw:?}"
    );
    let right_end = lw.origin.x + room.width - edges.right;
    assert!(
        (right_end - (width - c.origin.x)).abs() < EPS,
        "{at}: the lights end at {right_end}, not {} in from the right",
        c.origin.x
    );
    // Clear of the lockup and the buttons: the accessory starts past
    // both, and the least x a light may take is a light's gap past the
    // lockup.
    assert!(
        lw.origin.x >= mw.origin.x + mw.size.width - EPS
            && lw.origin.x >= z.origin.x + z.size.width,
        "{at}: the lights' accessory {lw:?} reaches the lockup {mw:?} or the buttons"
    );
    assert!(
        (lw.origin.x + least - mark.lights_left()).abs() < EPS,
        "{at}: a light may start at {}, not at {}",
        lw.origin.x + least,
        mark.lights_left()
    );
    // The smallest window has no room for all of them; the others do.
    let room_for = room.width - least.max(0.0) - edges.right;
    if width <= SMALLEST.width {
        assert!(
            room_for < SPANS_WIDTH,
            "{at}: a {width}-point window holds every light"
        );
    } else {
        assert!(
            room_for >= SPANS_WIDTH - EPS,
            "{at}: a {width}-point window drops a light"
        );
    }
    // And the terminal's view, below the title bar, overlaps neither.
    let v = view.convertRect_toView(view.bounds(), None);
    for (what, r) in [("the mark", mw), ("the lights", lw)] {
        assert!(
            r.origin.y >= v.origin.y + v.size.height - EPS,
            "{at}: {what} {r:?} overlaps the terminal's view {v:?}"
        );
    }
}
