//! **baud's lockup sits in the titlebar strip, beside the traffic lights and
//! above the terminal, at every size the window opens at, and follows the
//! window's light or dark**: headless, in
//! real windows built as `Opening::new` and `Opening::show` build baud's
//! -- the window at its default size and then at the grid's, the container
//! at the size the window was built at, the terminal's view inside it, the
//! lockup put by `baud-app/src/mark.rs` itself, then the body under it --
//! and never shown.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.
//!
//! Each window is checked as baud has it at each step: once the lockup is
//! put, once the body (the standard material, and the glass) has the
//! container, after a resize re-places it as `Driver::relayout` does, and
//! in both appearances. The lockup's frame must lie in its superview's
//! bounds, whole, the smallest window's included; converted to the
//! window's coordinates, between the top of the window's content layout
//! rect and the window's top, which is the titlebar's strip, and past the
//! zoom button's right edge, level with the traffic lights; the shape of
//! its picture, so the picture ends where the frame does and the activity
//! lights start a light's gap past it; its mark's cell the size the mark
//! alone was drawn at; and after the terminal's view among the
//! container's subviews, clear of its frame. Under Aqua it shows the
//! dark word and under Dark Aqua the light one, swapped on the flip with
//! the frame unmoved. Full screen is a resize to the screen's size, the
//! path the resize step takes.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/mark.rs"]
mod mark;

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
use macos_liquid_glass::{
    drawable::Appearance,
    glass::{GlassStyle, GlassSurface},
    material::{Material, MaterialSurface},
    objc2::MainThreadOnly,
    objc2_app_kit::{
        NSAppearance, NSAppearanceCustomization, NSApplication, NSAutoresizingMaskOptions, NSView,
        NSWindow, NSWindowButton,
    },
    objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString},
    window::GlassWindow,
};

/// The size a window opens at, `app.rs`'s `WINDOW_SIZE`.
#[cfg(target_os = "macos")]
const WINDOW_SIZE: NSSize = NSSize::new(960.0, 600.0);

/// The smallest window, `app.rs`'s `set_min_size`.
#[cfg(target_os = "macos")]
const SMALLEST: NSSize = NSSize::new(200.0, 100.0);

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
            let inset = {
                let h = w
                    .contentView()
                    .map_or(content.height, |v| v.frame().size.height);
                (h - w.contentLayoutRect().size.height).max(0.0)
            };
            assert!(inset > 10.0, "a titled window has a titlebar: {inset}");
            let built = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
            let container = NSView::initWithFrame(NSView::alloc(mtm), built);
            container.setAutoresizesSubviews(true);
            let view = NSView::initWithFrame(
                NSView::alloc(mtm),
                NSRect::new(
                    NSPoint::new(0.0, 0.0),
                    NSSize::new(WINDOW_SIZE.width, WINDOW_SIZE.height - inset),
                ),
            );
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            container.addSubview(&view);
            window.set_content_view(&container);
            let mark = mark::Mark::put(mtm, w, &container, inset).expect("the lockups decode");
            // Put in the form for the window's appearance as it is.
            let now = if macos_liquid_glass::is_dark(&w.effectiveAppearance()) {
                Appearance::Dark
            } else {
                Appearance::Light
            };
            assert_eq!(mark.shown(), mark::Lockup::for_appearance(now));
            let at = |step: &str| format!("{body:?} body, {content:?}, {step}");
            check(w, &container, &view, &mark, inset, &at("put"));
            // The body takes the container, and the mark is re-placed.
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
            mark.place(&container);
            check(w, &container, &view, &mark, inset, &at("under the body"));
            // A resize, as a drag, a zoom or full screen gives one, and the
            // re-place `relayout` makes.
            let next = sizes[(i + 1) % sizes.len()];
            w.setContentSize(next);
            mark.place(&container);
            check(w, &container, &view, &mark, inset, &at("resized"));
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
                check(w, &container, &view, &mark, inset, &at(name));
                assert_eq!(mark.view().frame(), placed, "{}", at(name));
            }
            assert!(
                pictures[0] != pictures[1],
                "{}: the flip swapped nothing",
                at("flip")
            );
            assert!(pictures[0] == pictures[2], "{}: the flip back", at("flip"));
            window.set_appearance(None);
            checked += 1;
        }
    }
    assert_eq!(checked, 8, "two bodies at four sizes");
    println!("{NAME}: ok, {checked} windows");
}

/// **The lockup where baud's window has it**: whole in its superview's
/// bounds, in the titlebar's strip and level with the traffic lights in
/// the window's coordinates, past the zoom button, the shape of its
/// picture, its mark the size the mark alone was, the lights a light's
/// gap past its end, and above the terminal's view.
#[cfg(target_os = "macos")]
fn check(w: &NSWindow, container: &NSView, view: &NSView, mark: &mark::Mark, inset: f64, at: &str) {
    const EPS: f64 = 1e-6;
    // The layout AppKit makes before it draws: right after the body goes
    // in, the content layout rect reads 572 points low until it runs.
    w.layoutIfNeeded();
    let m = mark.view().frame();
    let b = container.bounds();
    assert!(
        m.origin.x >= b.origin.x - EPS
            && m.origin.y >= b.origin.y - EPS
            && m.origin.x + m.size.width <= b.origin.x + b.size.width + EPS
            && m.origin.y + m.size.height <= b.origin.y + b.size.height + EPS,
        "{at}: the mark {m:?} is outside its superview's bounds {b:?}"
    );
    let mw = container.convertRect_toView(m, None);
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
    let zoom = w
        .standardWindowButton(NSWindowButton::ZoomButton)
        .expect("a titled window has a zoom button");
    let z = zoom.convertRect_toView(zoom.bounds(), None);
    assert!(
        mw.origin.x >= z.origin.x + z.size.width - EPS,
        "{at}: the mark {mw:?} is not past the zoom button {z:?}"
    );
    let (mc, zc) = (
        mw.origin.y + mw.size.height / 2.0,
        z.origin.y + z.size.height / 2.0,
    );
    assert!(
        (mc - zc).abs() <= z.size.height / 2.0,
        "{at}: the mark's centre {mc} is not level with the traffic lights' {zc}"
    );
    // The picture's shape, so it is drawn to the frame's edges: the
    // frame's right is the cursor's, and the lights start a light's gap
    // past it.
    let art = mark::Lockup::OnDark.art().expect("the lockup's size");
    assert!(
        (m.size.width / m.size.height - art.width / art.height).abs() < 1e-9,
        "{at}: the frame {m:?} is not the picture's shape {art:?}"
    );
    assert!(
        (mark.end() - (m.origin.x + m.size.width)).abs() < EPS,
        "{at}: the lockup ends at {}, its frame at {}",
        mark.end(),
        m.origin.x + m.size.width
    );
    assert!(
        (mark.lights_left() - mark.end() - f64::from(baud_render::lights::PAIR)).abs() < EPS,
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
    let subviews = container.subviews();
    let index = |v: &NSView| (0..subviews.count()).find(|&i| &*subviews.objectAtIndex(i) == v);
    let (mi, vi) = (index(mark.view()), index(view));
    assert!(
        matches!((mi, vi), (Some(a), Some(b)) if a > b),
        "{at}: the mark ({mi:?}) is not above the terminal's view ({vi:?})"
    );
    let v = view.frame();
    let apart = m.origin.y >= v.origin.y + v.size.height - EPS
        || m.origin.x >= v.origin.x + v.size.width - EPS
        || m.origin.x + m.size.width <= v.origin.x + EPS;
    assert!(
        apart,
        "{at}: the mark {m:?} overlaps the terminal's view {v:?}"
    );
}
