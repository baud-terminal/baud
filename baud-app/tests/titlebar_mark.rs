//! **baud's lockup and activity lights sit in the title bar, on one plate at
//! its right end, or the lockup alone beside the traffic lights with no lights
//! in the title bar, at every size the window opens at, follow the window's
//! light or dark, and the grid's top follows full screen**: headless, in real
//! windows built as `Opening::new` and `Opening::show` build baud's -- the
//! window at its default size and then at the grid's, the container at the
//! size the window was built at, the terminal's view inside it, the lockup
//! put by `baud-app/src/mark.rs` itself, the lights put, drawn and placed by
//! `baud-app/src/lights.rs` itself in the accessory `baud-app/src/strip.rs`
//! fits, then the body under them -- and never shown.
//!
//! `harness = false` because AppKit views are main-thread-only and
//! libtest runs every test on a thread of its own; this `main` is the
//! process's main thread. It answers nextest's `--list` itself.
//!
//! Each window is checked as baud has it at each step: once the lockup is
//! put, once the body (the standard material, and the glass) has the
//! container, after a resize re-places both as `Driver::relayout` does,
//! and in both appearances. **With no lights in the title bar** ([`check`])
//! the lockup must lie in its accessory's bounds,
//! whole, the smallest window's included; in the window's coordinates,
//! between the top of the content layout rect and the window's top, which
//! is the title bar, a button's gap past the zoom button, as AppKit puts a
//! leading accessory, and level with the traffic lights; the shape of its
//! picture; its mark's cell the size the mark alone was drawn at; and in
//! the title bar's own views, which it shares with the traffic lights and
//! the content does not. **With them** ([`check_panel`]) the lockup is on
//! the plate, in the lights' accessory, flush with the window's right edge,
//! in the title bar's own views too; the plate's frame, the lamps' columns,
//! their tracking areas and hover frames are `lights::panel`'s; and the
//! build's name is alone at the bar's left, on its baseline. With no plate,
//! under Aqua the lockup shows the dark word and under Dark Aqua the light
//! one, swapped on the flip with the frame unmoved; on a plate the plate's
//! lightness picks the form under either, and a change of theme swaps it with
//! no flip.
//!
//! **Full screen is not a resize.** The title bar leaves the window, into
//! a window of its own that is shown only when the pointer reaches the top
//! of the screen, so content that should go with it has to be in it: the
//! accessories' place in the title bar's views is that check, with the
//! lockup and the plate in them on either side of the notifications. A window
//! cannot be put in full screen without being shown, so the grid's top is
//! checked by posting the two notifications AppKit posts at the end of a
//! transition, through baud's own observer (`full_screen.rs`) and its own
//! response (`strip::follow`): entered, the terminal's view reaches the
//! window's top; exited, it stops the title bar's height short again.
//!
//! **A double-click on the title bar's empty part** is the binary's second
//! test ([`double_click`]): in the same windows, the view each point of the
//! bar is hit as, and what a double-click there does under each value of
//! the user's setting.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/mark.rs"]
mod mark;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/strip.rs"]
mod strip;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/full_screen_check.rs"]
mod full_screen_check;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/lights.rs"]
mod lights;

/// The face the lights' letters are drawn in, as `embedded.rs` holds it.
#[cfg(target_os = "macos")]
mod embedded {
    pub(crate) struct Font {
        pub(crate) bytes: &'static [u8],
    }
    pub(crate) const BOLD: Font = Font {
        bytes: include_bytes!("../fonts/built/Bold.ttf"),
    };
}

/// What `lights.rs` reads of the process reader, which this test has no use
/// for.
#[cfg(target_os = "macos")]
#[allow(dead_code)]
mod front {
    #[derive(Clone, Copy)]
    pub(crate) enum Reading {
        Lit,
        Refused,
        Dark,
    }
}

const NAME: &str = "the_lockup_sits_in_the_titlebar_strip";

/// The second test: [`double_click`].
const DOUBLE_CLICK: &str = "a_double_click_on_the_empty_title_bar_does_the_settings_action";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if cfg!(target_os = "macos") && !args.iter().any(|a| a == "--ignored") {
            println!("{NAME}: test");
            println!("{DOUBLE_CLICK}: test");
        }
        return;
    }
    let filter = args.iter().find(|a| !a.starts_with('-'));
    let runs = |name: &str| filter.is_none_or(|f| name.contains(f.as_str()));
    #[cfg(target_os = "macos")]
    {
        if runs(NAME) {
            run();
        }
        if runs(DOUBLE_CLICK) {
            double_click(
                MainThreadMarker::new().expect("harness = false runs main on the main thread"),
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = runs;
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

/// **A macOS 27 window's corner**, in points: its radius, which AppKit's
/// corner-adapted safe area keeps the plate's right edge in from the
/// window's.
#[cfg(target_os = "macos")]
const CORNER: CGFloat = 16.0;

/// The size a window opens at, `app.rs`'s `WINDOW_SIZE`.
#[cfg(target_os = "macos")]
const WINDOW_SIZE: NSSize = NSSize::new(960.0, 600.0);

/// The smallest window, `app.rs`'s `set_min_size`.
#[cfg(target_os = "macos")]
const SMALLEST: NSSize = NSSize::new(200.0, 100.0);

/// The longest name a build has: seven digits of hash and `.dirty`.
#[cfg(target_os = "macos")]
const VERSION: &str = "1.0.0-unstable+0123abc.dirty";

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
            let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
            let lights = strip::Accessory::put(
                mtm,
                &window,
                TitlebarSide::Trailing,
                NSSize::new(0.0, inset),
            );
            // No lights in the title bar here: no plate, and nothing in
            // their accessory.
            // Put in the form for the window's appearance as it is.
            let now = if macos_liquid_glass::is_dark(&w.effectiveAppearance()) {
                Appearance::Dark
            } else {
                Appearance::Light
            };
            assert_eq!(mark.shown(), mark::Lockup::for_appearance(now));
            let at = |step: &str| format!("{body:?} body, {content:?}, {step}");
            // The re-place `relayout` makes: the lockup centred, the
            // build's name fitted, with no plate.
            let place = || {
                w.layoutIfNeeded();
                mark.place();
                mark.fit(w, None, mark::Fitted::default());
            };
            place();
            check(w, &view, &mark, &lights, inset, &at("put"));
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
            place();
            check(w, &view, &mark, &lights, inset, &at("under the body"));
            // A resize, as a drag or a zoom gives one.
            let next = sizes[(i + 1) % sizes.len()];
            w.setContentSize(next);
            place();
            check(w, &view, &mark, &lights, inset, &at("resized"));
            follows_full_screen(w, &view, &container, inset, &at("full screen"));
            if checked == 0 {
                reads_full_screen_at_rest(w, &view, &container, &mark, &lights.view, &at("read"));
            }
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
                check(w, &view, &mark, &lights, inset, &at(name));
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
                probe(w, &mark, inset, &at("probe"));
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 8, "two bodies at four sizes");
    on_the_plate(mtm);
    theme_files_keep_their_plate(mtm);
    grey_in_a_window_not_active(mtm);
    renders(mtm);
    panel_renders(mtm);
    println!("{NAME}: ok, {checked} windows");
}

/// **The lights drawn and placed, and the lockup on their plate, as
/// `Driver::show_lights` and `Driver::place_lights` do it.**
#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
fn draw_and_place(
    w: &NSWindow,
    strip: &mut lights::Strip,
    mark: &mark::Mark,
    renderer: &baud_render::Renderer<'_, '_>,
    setting: baud_render::ActivityLights,
    shown: baud_render::Shown,
    scale: CGFloat,
    inset: CGFloat,
) {
    let wanted = lights::Drawn {
        scale,
        look: renderer.lights_look(baud_render::Glow::Full),
    };
    let _: lights::Laid = strip.show(setting, wanted, shown, renderer, strip::edges(w), inset);
    place_panel(w, strip, mark);
}

/// The re-place `Driver::relayout` makes, after the corner read again as
/// the view's resize callback reads it.
#[cfg(target_os = "macos")]
fn place_panel(w: &NSWindow, strip: &lights::Strip, mark: &mark::Mark) {
    strip::read_corner(w);
    w.layoutIfNeeded();
    mark.place();
    let plate = strip.place(strip::edges(w));
    mark.fit(w, plate, strip.fitted());
    w.layoutIfNeeded();
}

/// A renderer of `theme` over the embedded Bold.
#[cfg(target_os = "macos")]
fn renderer_of<'f, 'a>(
    faces: &'f [baud_font::Face<'a>],
    theme: baud_render::Theme,
) -> baud_render::Renderer<'f, 'a> {
    use baud_render::{Blend, PxSize, Renderer, Stack};
    Renderer::new(
        Stack::new(faces).expect("a stack"),
        PxSize::of(28.0).expect("a size"),
        theme,
        Blend::Native,
        baud_domain::Cols::of_index(6),
        baud_domain::Rows::of_index(2),
    )
    .expect("cell metrics")
}

/// **The module in real windows, through a change of setting, a resize, a
/// flip and a full screen**: see [`check_panel`]. At the app's two scales, in
/// both bodies, at the four sizes, each then resized to the next. `off`, and
/// the lights on a line of their own, take the plate down and put the lockup
/// back beside the traffic lights ([`check`] holds that), and `title_bar`
/// puts them back. A window just too narrow for the name and every light
/// gives up the name and keeps the lights; at its width plus a point it has
/// both. Perturbation: leave the lockup in the leading accessory, take no
/// plate down with the lights, leave the tracking areas on the frames they
/// had, or a `Mark::fit` that always shows the name, and this goes red.
#[cfg(target_os = "macos")]
fn on_the_plate(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Shown, Theme};
    let sizes = [
        NSSize::new(786.0, 572.0),
        WINDOW_SIZE,
        NSSize::new(1200.0, 900.0),
        SMALLEST,
    ];
    let faces = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let r = renderer_of(&faces, Theme::dark());
    let mut checked = 0;
    for body in [Body::Standard, Body::Glass] {
        for scale in [1.0, 2.0] {
            for (i, &content) in sizes.iter().enumerate() {
                let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
                let w = window.ns_window();
                if content != WINDOW_SIZE {
                    w.setContentSize(content);
                }
                let inset = strip::inset_of(w);
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
                let mark =
                    mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
                let holder = strip::Accessory::put(
                    mtm,
                    &window,
                    TitlebarSide::Trailing,
                    NSSize::new(0.0, inset),
                );
                let mut strip = lights::Strip::new(Some(holder), None, mark.size());
                let at = |step: &str| format!("{body:?} body, {content:?} at {scale}x, {step}");
                draw_and_place(
                    w,
                    &mut strip,
                    &mark,
                    &r,
                    ActivityLights::TitleBar,
                    Shown::DARK,
                    scale,
                    inset,
                );
                check_panel(w, &view, &mark, &strip, inset, scale, &at("put"));
                let glass;
                let material;
                match body {
                    Body::Standard => {
                        material =
                            MaterialSurface::new(mtm, built, Material::UnderWindowBackground);
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
                place_panel(w, &strip, &mark);
                check_panel(w, &view, &mark, &strip, inset, scale, &at("under the body"));
                let next = sizes[(i + 1) % sizes.len()];
                w.setContentSize(next);
                place_panel(w, &strip, &mark);
                check_panel(w, &view, &mark, &strip, inset, scale, &at("resized"));
                // Full screen: the title bar's views, on either side of it.
                follows_full_screen(w, &view, &container, inset, &at("full screen"));
                check_panel(
                    w,
                    &view,
                    &mark,
                    &strip,
                    inset,
                    scale,
                    &at("past full screen"),
                );
                let observed = {
                    let (win, v, c) = (w.retain(), view.retain(), container.retain());
                    full_screen::when_full_screen(w, move |change| {
                        strip::follow(change, &win, &v, &c);
                    })
                };
                post(w, Posted::Entered);
                place_panel(w, &strip, &mark);
                check_panel_views(w, &mark, &strip, &at("entered"));
                post(w, Posted::Exited);
                drop(observed);
                place_panel(w, &strip, &mark);
                check_panel(w, &view, &mark, &strip, inset, scale, &at("exited"));
                if checked == 0 {
                    let holder = strip.holder_view().expect("the accessory");
                    reads_full_screen_at_rest(w, &view, &container, &mark, &holder, &at("read"));
                }
                // A flip of appearance leaves the dark plate's light word,
                // under a light window too, and moves nothing.
                let placed = mark.view().frame();
                for name in ["NSAppearanceNameAqua", "NSAppearanceNameDarkAqua"] {
                    let a = NSAppearance::appearanceNamed(&NSString::from_str(name))
                        .expect("a stock appearance");
                    window.set_appearance(Some(&a));
                    mark.follow_window(w);
                    assert_eq!(mark.shown(), mark::Lockup::OnDark, "{}", at(name));
                    place_panel(w, &strip, &mark);
                    assert_eq!(mark.shown(), mark::Lockup::OnDark, "{}", at(name));
                    assert_eq!(mark.view().frame(), placed, "{}", at(name));
                    check_panel(w, &view, &mark, &strip, inset, scale, &at(name));
                }
                window.set_appearance(None);
                // With no lights in the title bar there is no plate, and
                // back.
                let nothing = strip::Accessory::put(
                    mtm,
                    &window,
                    TitlebarSide::Trailing,
                    NSSize::new(0.0, inset),
                );
                for setting in [
                    ActivityLights::Off,
                    ActivityLights::TopLine,
                    ActivityLights::BottomLine,
                ] {
                    draw_and_place(w, &mut strip, &mark, &r, setting, Shown::DARK, scale, inset);
                    let step = format!("{setting:?}");
                    assert!(strip.plate_view().is_none(), "{}", at(&step));
                    assert_eq!(strip.across(), 0.0, "{}", at(&step));
                    check(w, &view, &mark, &nothing, inset, &at(&step));
                    let room = strip.holder_view().expect("the accessory").frame();
                    assert_eq!(room.size.width, 0.0, "{}: empty", at(&step));
                }
                draw_and_place(
                    w,
                    &mut strip,
                    &mark,
                    &r,
                    ActivityLights::TitleBar,
                    Shown::DARK,
                    scale,
                    inset,
                );
                check_panel(w, &view, &mark, &strip, inset, scale, &at("back on"));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 16, "two bodies at two scales at four sizes");
    name_yields_first(mtm);
    the_plate_picks_the_form(mtm);
}

/// **The lockup's form on a live strip, as `Driver::show_lights` reaches
/// it**: the tick shows the look the theme now has, what the lockup is placed
/// for has moved, and the fit that follows swaps the form, with no flip of
/// the window. A light plate under a dark window takes the dark word, a dark
/// one under a light window the light word. With the plate gone, by the
/// setting or by a window too narrow for any light, the window's form again
/// and a flip follows; with it back, the plate's. Perturbation: a
/// `Strip::fitted` without the plate's colour never asks for the fit;
/// `Mark::fit` that leaves the form leaves the last one; `Mark::follow` that
/// reads the window alone takes the plate's form away on a flip.
#[cfg(target_os = "macos")]
fn the_plate_picks_the_form(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Shown, Theme};
    use mark::Lockup::{OnDark, OnLight};
    let appearance = |window: &GlassWindow, name: &str| {
        let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
        window.set_appearance(Some(&a));
    };
    let faces = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let (dark, light) = (
        renderer_of(&faces, Theme::dark()),
        renderer_of(&faces, Theme::light()),
    );
    let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
    let w = window.ns_window();
    appearance(&window, "NSAppearanceNameDarkAqua");
    let inset = strip::inset_of(w);
    let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
    let holder = strip::Accessory::put(
        mtm,
        &window,
        TitlebarSide::Trailing,
        NSSize::new(0.0, inset),
    );
    let mut strip = lights::Strip::new(Some(holder), None, mark.size());
    assert_eq!(mark.shown(), OnDark, "put, a dark window");
    // What a tick does: show the look, and fit only if the key moved. It
    // returns what `Strip::show` laid out as well, which the driver's tick
    // turns into a reading of the tips' places owed (`Tips::Owed`).
    let tick = |strip: &mut lights::Strip, r: &baud_render::Renderer<'_, '_>, w: &NSWindow| {
        let wanted = lights::Drawn {
            scale: 2.0,
            look: r.lights_look(baud_render::Glow::Full),
        };
        let laid = strip.show(
            ActivityLights::TitleBar,
            wanted,
            Shown::DARK,
            r,
            strip::edges(w),
            inset,
        );
        let moved = mark.lights_moved(strip.fitted());
        if moved {
            place_panel(w, strip, &mark);
        }
        (moved, laid)
    };
    let (moved, laid) = tick(&mut strip, &dark, w);
    assert!(moved, "the plate comes up");
    assert_eq!(
        laid,
        lights::Laid::Again,
        "the plate comes up, its views laid out"
    );
    assert_eq!(mark.shown(), OnDark, "the dark plate");
    let (moved, laid) = tick(&mut strip, &dark, w);
    assert!(!moved, "the tick after does nothing");
    assert_eq!(laid, lights::Laid::Same, "the tick after lays nothing out");
    // The theme changes under a window that does not.
    let placed = mark.view().frame();
    let first = mark.view().image().expect("a picture");
    assert!(
        tick(&mut strip, &light, w).0,
        "a theme change moves the key"
    );
    assert_eq!(mark.shown(), OnLight, "a light plate under a dark window");
    assert_eq!(mark.view().frame(), placed, "and moves nothing");
    assert!(
        mark.view().image().expect("a picture") != first,
        "the picture is swapped"
    );
    // A flip, as `apply_style` has it, leaves the plate's form.
    appearance(&window, "NSAppearanceNameAqua");
    mark.follow_window(w);
    assert_eq!(mark.shown(), OnLight, "the flip leaves it");
    assert!(tick(&mut strip, &dark, w).0, "the dark theme again");
    assert_eq!(mark.shown(), OnDark, "a dark plate under a light window");
    let opaque = baud_domain::Chan::from_u8(0xFF);
    assert_eq!(
        strip.fitted().plate,
        Some(Theme::dark().bg.with_alpha(opaque))
    );
    // The plate goes, by the setting: the window's form, a flip following.
    draw_and_place(
        w,
        &mut strip,
        &mark,
        &dark,
        ActivityLights::Off,
        Shown::DARK,
        2.0,
        inset,
    );
    assert!(!mark.on_plate());
    assert_eq!(mark.shown(), OnLight, "no plate, a light window");
    appearance(&window, "NSAppearanceNameDarkAqua");
    mark.follow_window(w);
    assert_eq!(mark.shown(), OnDark, "no plate, the flip follows");
    appearance(&window, "NSAppearanceNameAqua");
    mark.follow_window(w);
    assert_eq!(mark.shown(), OnLight, "and back");
    // It comes back: the plate's form at once.
    assert!(tick(&mut strip, &dark, w).0, "the plate again");
    assert_eq!(mark.shown(), OnDark, "the plate's, under a light window");
    // The plate goes in a window too narrow for any light, and comes back.
    w.setContentSize(NSSize::new(140.0, WINDOW_SIZE.height));
    place_panel(w, &strip, &mark);
    assert!(!mark.on_plate(), "too narrow for a plate");
    assert_eq!(mark.shown(), OnLight, "the window's");
    w.setContentSize(WINDOW_SIZE);
    place_panel(w, &strip, &mark);
    assert!(mark.on_plate(), "wide again");
    assert_eq!(mark.shown(), OnDark, "the plate's");
    window.set_appearance(None);
}

/// **A window just too narrow for the name and every light gives up the
/// name and keeps the lights**; at its width plus a point it has both, and
/// the smallest window has neither. The small system font is what the
/// composed strip draws at, so its layout is this one's. The widths are
/// the window's own: the traffic lights, and the lockup sized beside them,
/// are AppKit's and follow the SDK the binary links -- linked against the
/// 14.4 SDK the buttons end at 69 points, against 27.2 at 78 with a
/// narrower lockup, so the numbers `lights.rs` holds for a 28-point strip
/// past 69 hold only under the first. Perturbation:
/// `Mark::fit` that always shows the name fails the first half, and one that
/// forgets the accessory's width fails `check_panel`.
#[cfg(target_os = "macos")]
fn name_yields_first(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Shown, Theme};
    assert_eq!(
        macos_liquid_glass::objc2_app_kit::NSFont::smallSystemFontSize(),
        mark::VERSION_POINTS
    );
    let faces = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let r = renderer_of(&faces, Theme::dark());
    let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
    let w = window.ns_window();
    let inset = strip::inset_of(w);
    let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
    let holder = strip::Accessory::put(
        mtm,
        &window,
        TitlebarSide::Trailing,
        NSSize::new(0.0, inset),
    );
    let mut strip = lights::Strip::new(Some(holder), None, mark.size());
    draw_and_place(
        w,
        &mut strip,
        &mark,
        &r,
        ActivityLights::TitleBar,
        Shown::DARK,
        2.0,
        inset,
    );
    // A move to another screen's scale moves the plate, so the driver places
    // the lockup again. Perturbation: a key of the lights' width alone.
    assert!(!mark.lights_moved(strip.fitted()), "placed for what it is");
    let look = r.lights_look(baud_render::Glow::Full);
    let wanted = lights::Drawn { scale: 1.0, look };
    let laid = strip.show(
        ActivityLights::TitleBar,
        wanted,
        Shown::DARK,
        &r,
        strip::edges(w),
        inset,
    );
    assert_eq!(
        laid,
        lights::Laid::Again,
        "another scale draws the pictures again"
    );
    assert!(mark.lights_moved(strip.fitted()), "another scale");
    place_panel(w, &strip, &mark);
    assert!(!mark.lights_moved(strip.fitted()), "placed again");
    let across = mark.version_label().frame().size.width;
    let edges = strip::edges(w);
    let left = edges.left;
    // The plate's span from its left to the window's edge, whole at 1x and
    // so the same at every whole width: the name shows from where the plate
    // starts a gap past its end.
    let plate = strip.place(edges).expect("a plate at the window's size");
    let room = left + across + mark::VERSION_GAP + (edges.width - plate.plate_left);
    // Every light is placed from where the plate's left before rounding
    // reaches the traffic lights' end (`lights::panel`): the pad, the
    // lockup, the gap and the lamps' letters, the pad, and the right inset.
    let every = {
        use baud_render::lights::{PANEL_LOGO_GAP, PANEL_PAD};
        let c = strip.columns();
        let letters = c.letters.syn.0 + c.letters.syn.1 - c.letters.hs.0;
        let exact = 2.0 * CGFloat::from(PANEL_PAD)
            + mark.size().width
            + CGFloat::from(PANEL_LOGO_GAP)
            + letters
            + edges.right;
        (left + exact).ceil()
    };
    for (width, shown) in [
        (room.ceil(), true),
        (room.ceil() - 1.0, false),
        (every, false),
        (every - 1.0, false),
    ] {
        w.setContentSize(NSSize::new(width, WINDOW_SIZE.height));
        place_panel(w, &strip, &mark);
        assert_eq!(!mark.version_label().isHidden(), shown, "at {width} points");
        let lamp = strip.lamp_view(baud_render::Led::Hs).expect("HS's view");
        assert_eq!(!lamp.isHidden(), width >= every, "at {width} points");
        check_panel_views(w, &mark, &strip, &format!("at {width} points"));
    }
}

/// **A theme file's plate, in a window**: dark, light and mid-grey, the
/// plate's picture is drawn from its page and the socket and bevel from
/// that, so a change of theme redraws them. Pixel values are held in
/// `baud-render/tests/activity_lights.rs` and through the loader in
/// `palette.rs`. Perturbation: redraw only on a change of scale and the
/// second theme's plate is the first's.
#[cfg(target_os = "macos")]
fn theme_files_keep_their_plate(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Argb, Shown, Theme};
    let faces = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
    let w = window.ns_window();
    let inset = strip::inset_of(w);
    let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
    let holder = strip::Accessory::put(
        mtm,
        &window,
        TitlebarSide::Trailing,
        NSSize::new(0.0, inset),
    );
    let mut strip = lights::Strip::new(Some(holder), None, mark.size());
    let mut seen: Vec<_> = Vec::new();
    for (bg, form) in [
        (0xFF_1A_1B_26u32, mark::Lockup::OnDark),
        (0xFF_FD_F6_E3, mark::Lockup::OnLight),
        (0xFF_80_80_80, mark::Lockup::OnLight),
    ] {
        let mut theme = Theme::dark();
        theme.bg = Argb(bg);
        let r = renderer_of(&faces, theme);
        draw_and_place(
            w,
            &mut strip,
            &mark,
            &r,
            ActivityLights::TitleBar,
            Shown::DARK,
            2.0,
            inset,
        );
        let tiff = strip
            .plate_view()
            .and_then(|p| p.image())
            .expect("a plate's picture")
            .TIFFRepresentation();
        assert!(!seen.contains(&tiff), "{bg:08X}: the last theme's plate");
        seen.push(tiff);
        assert_eq!(mark.shown(), form, "{bg:08X}");
        check_panel_views(w, &mark, &strip, &format!("{bg:08X}"));
    }
}

/// **The module's views in the title bar's own views**: the lockup and the
/// plate and each lamp in the lights' accessory and none of them in the
/// content or left behind with the traffic lights alone; the plate under the
/// lamps; each lamp's tracking area following it.
#[cfg(target_os = "macos")]
fn check_panel_views(w: &NSWindow, mark: &mark::Mark, strip: &lights::Strip, at: &str) {
    use baud_render::Led;
    let holder = strip.holder_view().expect("the accessory");
    assert!(mark.on_plate(), "{at}: the lockup on the plate");
    let content = w.contentView().expect("a content view");
    let close = w
        .standardWindowButton(NSWindowButton::CloseButton)
        .expect("a close button");
    // SAFETY: a view's superview, read here; the window owns it.
    let parent = unsafe { mark.view().superview() }.expect("in a view");
    assert_eq!(&*parent as *const NSView, &*holder as *const NSView, "{at}");
    let plate = strip.plate_view().expect("a plate");
    // SAFETY: as above.
    let plate_in = unsafe { plate.superview() }.expect("in a view");
    assert_eq!(
        &*plate_in as *const NSView, &*holder as *const NSView,
        "{at}"
    );
    assert!(!plate.isHidden(), "{at}: the plate is up");
    assert!(
        !holder.isDescendantOf(&content),
        "{at}: the accessory is in the content"
    );
    let shared = close
        .ancestorSharedWithView(&holder)
        .unwrap_or_else(|| panic!("{at}: not in the window's views"));
    assert!(
        !content.isDescendantOf(&shared),
        "{at}: shares only the window's frame view with the traffic lights"
    );
    // The plate's view is before every lamp's in the accessory's subviews.
    let subs = holder.subviews();
    let index = |v: &NSView| {
        (0..subs.count())
            .find(|&i| std::ptr::eq(&*subs.objectAtIndex(i), v))
            .unwrap_or_else(|| panic!("{at}: a view not in the accessory"))
    };
    let under = index(plate);
    for led in Led::ALL {
        let lamp = strip.lamp_view(led).expect("a lamp's view");
        assert!(index(lamp) > under, "{at}: {led:?} under the plate");
        // One tracking area, in its visible rect, which moves with the view:
        // `baud_present_macos::hover`.
        let areas = lamp.trackingAreas();
        assert_eq!(areas.count(), 1, "{at}: {led:?}'s tracking areas");
        let options = areas.objectAtIndex(0).options();
        assert!(
            options
                .contains(macos_liquid_glass::objc2_app_kit::NSTrackingAreaOptions::InVisibleRect),
            "{at}: {led:?}: {options:?}"
        );
    }
}

/// **The module where `lights::panel` puts it, in a real window**: the
/// accessory flush with the window's right edge and holding the plate, the
/// plate's right edge 5 in from the window's and 24 tall, centred on the
/// strip, its left past the traffic lights; the lockup in it, whole, 7 from
/// its left; each lamp's frame `lights::panel`'s and its view's hover frame on
/// the screen with it, those it does not place hidden; each picture the size
/// of its frame; the build's name alone at the bar's left, in the leading
/// accessory, its baseline 18 down the strip, shown as `mark::name_shows` says;
/// and the terminal's view overlapping none of it. See the module's doc.
#[cfg(target_os = "macos")]
fn check_panel(
    w: &NSWindow,
    view: &NSView,
    mark: &mark::Mark,
    strip: &lights::Strip,
    inset: f64,
    scale: f64,
    at: &str,
) {
    use baud_render::Led;
    const EPS: f64 = 1e-6;
    w.layoutIfNeeded();
    check_panel_views(w, mark, strip, at);
    let edges = strip::edges(w);
    let width = w.frame().size.width;
    // AppKit's inset clear of the window's corner, read from the window
    // never shown: 16 on macOS 27, the corner's radius.
    assert_eq!(edges.right, CORNER, "{at}");
    let want = lights::panel(
        NSSize::new(width, inset),
        inset,
        edges.left,
        edges.right,
        scale,
        &strip.columns(),
        mark.size(),
    )
    .expect("a light fits in any window");
    let holder = strip.holder_view().expect("the accessory");
    let lw = holder.convertRect_toView(holder.bounds(), None);
    assert!(
        (lw.origin.x + lw.size.width - width).abs() < EPS,
        "{at}: the accessory {lw:?} is not flush with the right edge {width}"
    );
    let layout = w.contentLayoutRect();
    let top = layout.origin.y + layout.size.height;
    assert!(
        (w.frame().size.height - top - inset).abs() < EPS,
        "{at}: the strip is not {inset} tall"
    );
    // The plate: its picture is the bevel's margin past it.
    let plate = strip.plate_view().expect("a plate");
    let margin = lights::plate_margin(scale);
    let pw = plate.convertRect_toView(plate.bounds(), None);
    let inner = NSRect::new(
        NSPoint::new(pw.origin.x + margin, pw.origin.y + margin),
        NSSize::new(pw.size.width - 2.0 * margin, pw.size.height - 2.0 * margin),
    );
    assert!(
        (inner.origin.x + inner.size.width - (width - CORNER)).abs() <= 0.5 / scale + EPS,
        "{at}: the plate's right {inner:?} in a {width}"
    );
    // Clear of the window's corner: the curve spans its rightmost CORNER
    // points, and the plate ends left of them, to within the device pixel
    // its edge is rounded to.
    assert!(
        inner.origin.x + inner.size.width <= width - CORNER + 0.5 / scale + EPS,
        "{at}: the plate {inner:?} reaches the corner's curve in a {width}"
    );
    assert!(
        (inner.size.height - 24.0).abs() < EPS,
        "{at}: the plate's height {inner:?}"
    );
    assert!(
        (inner.origin.y - (top + (inset - 24.0) / 2.0)).abs() < EPS,
        "{at}: the plate is not centred on the strip: {inner:?} against {top}"
    );
    assert!(
        (inner.origin.x - want.plate.origin.x).abs() < EPS
            && (inner.size.width - want.plate.size.width).abs() < EPS,
        "{at}: the plate {inner:?}, the layout's {:?}",
        want.plate
    );
    assert!(
        inner.origin.x >= edges.left - EPS,
        "{at}: reaches the lights"
    );
    assert!(
        pw.origin.x >= lw.origin.x - EPS,
        "{at}: cut by its accessory"
    );
    let image = plate.image().expect("a plate's picture").size();
    assert!(
        (image.width - pw.size.width).abs() < EPS && (image.height - pw.size.height).abs() < EPS,
        "{at}: the picture {image:?} in {pw:?}"
    );
    // The lockup, whole, on it, its right form.
    let mw = mark.view().convertRect_toView(mark.view().bounds(), None);
    assert!(
        (mw.origin.x - inner.origin.x - 7.0).abs() < EPS,
        "{at}: the lockup {mw:?} on {inner:?}"
    );
    // Whole to a device pixel at the scale it was placed at: each edge on
    // one (`mark::snapped`).
    assert!(
        (mw.size.width - mark.size().width).abs() <= 1.0 / scale + EPS
            && (mw.size.height - mark.size().height).abs() <= 1.0 / scale + EPS,
        "{at}: the lockup is not whole: {mw:?}"
    );
    for edge in [
        mw.origin.x,
        mw.origin.y,
        mw.origin.x + mw.size.width,
        mw.origin.y + mw.size.height,
    ] {
        assert!(
            (edge * scale - (edge * scale).round()).abs() < EPS,
            "{at}: the lockup {mw:?} has an edge between device pixels at {scale}x"
        );
    }
    assert!(
        (mw.origin.y + mw.size.height / 2.0 - (top + inset / 2.0)).abs() <= 0.5 / scale + EPS,
        "{at}: the lockup is not centred on the strip: {mw:?}"
    );
    // The lamps.
    let frames = strip.screen_frames();
    for led in Led::ALL {
        let lamp = strip.lamp_view(led).expect("a lamp's view");
        match want.lamps.get(led) {
            Some((r, _)) => {
                assert!(!lamp.isHidden(), "{at}: {led:?} is hidden");
                let f = lamp.convertRect_toView(lamp.bounds(), None);
                assert!(
                    (f.origin.x - r.origin.x).abs() < EPS
                        && (f.size.width - r.size.width).abs() < EPS,
                    "{at}: {led:?} at {f:?}, the layout's {r:?}"
                );
                assert!(
                    (f.origin.y - top).abs() < EPS && (f.size.height - inset).abs() < EPS,
                    "{at}: {led:?} is not the strip's height: {f:?}"
                );
                let on = frames.get(led).expect("its frame on the screen");
                assert_eq!(on, w.convertRectToScreen(f), "{at}: {led:?}'s hover frame");
                let image = lamp.image().expect("its picture").size();
                assert!(
                    (image.width - f.size.width).abs() < EPS,
                    "{at}: {led:?}: {image:?} in {f:?}"
                );
            }
            None => {
                assert!(lamp.isHidden(), "{at}: {led:?} is shown");
                assert!(frames.get(led).is_none(), "{at}: {led:?}'s hover frame");
            }
        }
    }
    // The build's name, alone at the left.
    let label = mark.version_label();
    let placed = match want.first == Led::Hs {
        true => mark::Placed::Every,
        false => mark::Placed::Fewer,
    };
    let shows = mark::name_shows(
        placed,
        want.plate.origin.x,
        edges.left,
        label.frame().size.width,
    );
    assert_eq!(!label.isHidden(), shows, "{at}: the name shown as it fits");
    let leading = &mark.accessory().view;
    if shows {
        let lf = label.convertRect_toView(label.bounds(), None);
        assert!(
            (lf.origin.x - edges.left).abs() < EPS,
            "{at}: the name at {lf:?}, not past the buttons at {}",
            edges.left
        );
        let baseline = lf.origin.y + label.baselineOffsetFromBottom();
        let down = top + inset / 2.0 + strip::STRIP / 2.0 - baseline;
        assert!(
            (down - 18.0).abs() < EPS,
            "{at}: its baseline is {down} down"
        );
        assert!(
            lf.origin.x + lf.size.width + mark::VERSION_GAP <= inner.origin.x + EPS,
            "{at}: the name {lf:?} reaches the plate {inner:?}"
        );
        assert!(
            leading.frame().size.width >= lf.size.width - EPS,
            "{at}: the name is cut by its accessory"
        );
    } else {
        assert!(
            leading.frame().size.width < 1.0,
            "{at}: an empty accessory takes room: {:?}",
            leading.frame()
        );
    }
    // And the terminal's view, below the title bar, overlaps none of it.
    let v = view.convertRect_toView(view.bounds(), None);
    assert!(
        lw.origin.y >= v.origin.y + v.size.height - EPS,
        "{at}: the module {lw:?} overlaps the terminal's view {v:?}"
    );
}

/// **The view alone, drawn into a bitmap at 2x**, RGBA, premultiplied, rows
/// from the top.
#[cfg(target_os = "macos")]
fn drawn(view: &NSView) -> (usize, Vec<u8>) {
    use macos_liquid_glass::objc2::AnyThread;
    use macos_liquid_glass::objc2_app_kit::{
        NSBitmapFormat, NSBitmapImageRep, NSDeviceRGBColorSpace,
    };
    let size = view.bounds().size;
    let (pw, ph) = (
        (size.width * 2.0).round() as isize,
        (size.height * 2.0).round() as isize,
    );
    // SAFETY: no planes, so AppKit allocates; the depth and the row bytes are
    // a 32-bit RGBA bitmap's.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            core::ptr::null_mut(),
            pw,
            ph,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            NSBitmapFormat::empty(),
            4 * pw,
            32,
        )
    }
    .expect("a bitmap");
    rep.setSize(size);
    view.cacheDisplayInRect_toBitmapImageRep(view.bounds(), &rep);
    let (w, h) = (pw as usize, ph as usize);
    // SAFETY: the buffer the rep allocated, w * h * 4 bytes, read while it
    // lives.
    let bytes = unsafe { core::slice::from_raw_parts(rep.bitmapData(), w * h * 4) }.to_vec();
    (w, bytes)
}

/// How far from grey the most coloured pixel of a bitmap is.
#[cfg(target_os = "macos")]
fn chroma(bytes: &[u8]) -> u8 {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let (lo, hi) = (p[0].min(p[1]).min(p[2]), p[0].max(p[1]).max(p[2]));
            hi - lo
        })
        .max()
        .unwrap_or(0)
}

/// **The panel's pictures are colour, and AppKit greys them where they stand
/// in a window that is not active**, as every window here is. A picture in an
/// `NSImageView` in the title bar's views draws as its own luma at half
/// strength; the same picture in the same size of view in the content view
/// draws as it is: #101010 and #EFEFEF the plate, the lockup and each lit lamp
/// in colour. So a capture of these windows says nothing of colour, and the
/// grey is the view's own drawing and not the capture's. If the half of this
/// about AppKit goes red with baud unchanged, AppKit has changed: read
/// `MEASUREMENTS.md`, "The title bar is a modem's panel", again. CS's picture
/// follows `Shown` too: lit at rest, dark in `Shown::DARK`, with `Activity`
/// and the outbox behind it in `line_lamps.rs`'s test.
/// Perturbation: the plate's colour, a strip that shows every light's dark
/// picture, or CS dark at rest, and this goes red.
#[cfg(target_os = "macos")]
fn grey_in_a_window_not_active(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Led, Light, Shown, Theme};
    use macos_liquid_glass::objc2_app_kit::NSImageView;
    let face = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    for (name, theme, plate) in [
        ("NSAppearanceNameDarkAqua", Theme::dark(), 0x10u8),
        ("NSAppearanceNameAqua", Theme::light(), 0xEF),
    ] {
        let r = renderer_of(&face, theme);
        let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
        let w = window.ns_window();
        let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
        window.set_appearance(Some(&a));
        let inset = strip::inset_of(w);
        let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
        let holder = strip::Accessory::put(
            mtm,
            &window,
            TitlebarSide::Trailing,
            NSSize::new(0.0, inset),
        );
        let mut strip = lights::Strip::new(Some(holder), None, mark.size());
        let content = w.contentView().expect("a content view");
        // The same picture in a view of the same size, in the content view.
        let in_content = |view: &NSImageView| {
            let copy = NSImageView::new(mtm);
            copy.setImageScaling(view.imageScaling());
            copy.setImage(view.image().as_deref());
            copy.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), view.frame().size));
            content.addSubview(&copy);
            let (_, bytes) = drawn(&copy);
            copy.removeFromSuperview();
            bytes
        };
        let show = |shown: Shown, strip: &mut lights::Strip| {
            let setting = ActivityLights::TitleBar;
            draw_and_place(w, strip, &mark, &r, setting, shown, 2.0, inset);
        };
        show(Shown::all(Light::Lit), &mut strip);
        let view = strip.plate_view().expect("a plate");
        let centre = |(across, bytes): (usize, Vec<u8>)| -> [u8; 4] {
            let at = (bytes.len() / 4 / across / 2 * across + across / 2) * 4;
            bytes[at..at + 4].try_into().expect("a pixel")
        };
        let across = (view.bounds().size.width * 2.0).round() as usize;
        assert_eq!(
            centre((across, in_content(view))),
            [plate, plate, plate, 0xFF],
            "{name}: the plate's picture"
        );
        let [red, green, blue, alpha] = centre(drawn(view));
        assert!(
            alpha < 0xFF && red == green && green == blue,
            "{name}: the plate in the title bar, half strength: {red} {alpha}"
        );
        let mut coloured = vec![("the lockup", mark.view())];
        coloured.extend(Led::ALL.iter().map(|&led| {
            (
                led.letters(),
                strip.lamp_view(led).expect("every lamp at this size"),
            )
        }));
        for (what, view) in coloured {
            let (there, here) = (chroma(&in_content(view)), chroma(&drawn(view).1));
            assert!(there >= 64, "{name}: {what}'s picture is colour: {there}");
            assert!(here <= 1, "{name}: {what} greyed in the title bar: {here}");
        }
        let cs = |strip: &lights::Strip| chroma(&in_content(strip.lamp_view(Led::Cs).expect("CS")));
        show(Shown::rest(), &mut strip);
        assert!(cs(&strip) >= 64, "{name}: CS lit at rest: {}", cs(&strip));
        show(Shown::DARK, &mut strip);
        assert!(cs(&strip) < 64, "{name}: CS dark: {}", cs(&strip));
    }
}

/// **The title bar as AppKit draws it, whole, for each appearance at 1x and
/// 2x, at wide and narrow widths -- each side of every step the lamps drop
/// at -- at rest and with every lamp lit**, when `BAUD_PANEL_RENDERS` names
/// a folder: the window's frame view into a bitmap, its window never shown,
/// over a plain grey standing for the material, which only a window on
/// screen composites. Then the same window drawn as key in an active app
/// ([`Pretend`]), at rest and lit, so the colours AppKit greys in a window
/// that is not ([`grey_in_a_window_not_active`]) can be seen, and the
/// window in full screen at a screen's width, the plate held at the
/// windowed inset it read before.
#[cfg(target_os = "macos")]
fn panel_renders(mtm: MainThreadMarker) {
    use baud_render::{ActivityLights, Light, Shown, Theme};
    use macos_liquid_glass::objc2_app_kit::{NSColor, NSWindowStyleMask};
    let Some(dir) = std::env::var_os("BAUD_PANEL_RENDERS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    let face = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let states = [("rest", Shown::DARK), ("lit", Shown::all(Light::Lit))];
    for (name, look, grey, theme) in [
        ("NSAppearanceNameDarkAqua", "dark", 0.227, Theme::dark()),
        ("NSAppearanceNameAqua", "light", 0.925, Theme::light()),
    ] {
        let r = renderer_of(&face, theme);
        // A window of baud's at `across`, its lockup put and its lights'
        // accessory, never shown.
        let open = |across: CGFloat| {
            let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
            let w = window.ns_window();
            let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
            window.set_appearance(Some(&a));
            w.setOpaque(true);
            w.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(grey, 1.0)));
            w.setContentSize(NSSize::new(across, WINDOW_SIZE.height));
            let inset = strip::inset_of(w);
            let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
            let holder = strip::Accessory::put(
                mtm,
                &window,
                TitlebarSide::Trailing,
                NSSize::new(0.0, inset),
            );
            let strip = lights::Strip::new(Some(holder), None, mark.size());
            (window, mark, strip, inset)
        };
        for across in [1728.0, 960.0, 340.0, 315.0, 314.0, 260.0, 220.0, 184.0] {
            for (state, shown) in states {
                let (window, mark, mut strip, inset) = open(across);
                let w = window.ns_window();
                for scale in [1.0f64, 2.0] {
                    let setting = ActivityLights::TitleBar;
                    draw_and_place(w, &mut strip, &mark, &r, setting, shown, scale, inset);
                    let file = format!("appkit-{look}-{across}-{state}-{scale}x.png");
                    snap(w, inset, scale, &dir.join(file));
                }
            }
        }
        for (state, shown) in states {
            let (window, mark, mut strip, inset) = open(960.0);
            let w = window.ns_window();
            let key = Pretend::key(w, mtm);
            assert!(w.isKeyWindow(), "{look}: the window answers key");
            for scale in [1.0f64, 2.0] {
                let setting = ActivityLights::TitleBar;
                draw_and_place(w, &mut strip, &mark, &r, setting, shown, scale, inset);
                let file = format!("appkit-{look}-960-{state}-key-{scale}x.png");
                snap(w, inset, scale, &dir.join(file));
                let (_, bytes) = drawn(mark.view());
                let amber = bytes.as_chunks::<4>().0.iter().any(|p| {
                    p[0].abs_diff(0xF4) <= 8 && p[1].abs_diff(0xA3) <= 8 && p[2].abs_diff(0x2C) <= 8
                });
                assert!(amber, "{look}: the lockup's amber in a key window");
            }
            drop(key);
        }
        // Full screen at a screen's width: the corner read windowed first,
        // as a window is before it enters, then with the window giving a
        // style mask with full screen in it, as AppKit's does once entered.
        let (window, mark, mut strip, inset) = open(1728.0);
        let w = window.ns_window();
        let setting = ActivityLights::TitleBar;
        let lit = Shown::all(Light::Lit);
        draw_and_place(w, &mut strip, &mark, &r, setting, lit, 2.0, inset);
        let full = Pretend::full_screen(w);
        assert!(
            w.styleMask().contains(NSWindowStyleMask::FullScreen),
            "{look}: the window's style is full screen"
        );
        for scale in [1.0f64, 2.0] {
            draw_and_place(w, &mut strip, &mark, &r, setting, lit, scale, inset);
            let edges = strip::edges(w);
            assert_eq!(edges.right, 16.0, "{look}: full screen holds the inset");
            let file = format!("appkit-{look}-1728-lit-full-screen-{scale}x.png");
            snap(w, inset, scale, &dir.join(file));
        }
        drop(full);
    }
}

/// **`w`'s title bar, `inset` tall, drawn into a PNG at `path`** at `scale`:
/// the window's frame view into a bitmap, the window never shown.
#[cfg(target_os = "macos")]
fn snap(w: &NSWindow, inset: CGFloat, scale: CGFloat, path: &std::path::Path) {
    use macos_liquid_glass::objc2::AnyThread;
    use macos_liquid_glass::objc2_app_kit::{
        NSBitmapFormat, NSBitmapImageFileType, NSBitmapImageRep, NSDeviceRGBColorSpace,
    };
    use macos_liquid_glass::objc2_foundation::NSDictionary;
    // SAFETY: a view's superview, read and kept for this block; the window
    // that owns it lives past it.
    let frame = w
        .contentView()
        .and_then(|v| unsafe { v.superview() })
        .expect("the frame view");
    let across = frame.bounds().size.width;
    let top = frame.bounds().size.height - inset;
    let rect = NSRect::new(NSPoint::new(0.0, top), NSSize::new(across, inset));
    let (pw, ph) = (
        (across * scale).round() as isize,
        (inset * scale).round() as isize,
    );
    // SAFETY: no planes, so AppKit allocates; the depth and the row bytes
    // are a 32-bit RGBA bitmap's.
    let rep = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            core::ptr::null_mut(),
            pw,
            ph,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            NSBitmapFormat::empty(),
            4 * pw,
            32,
        )
    }
    .expect("a bitmap");
    rep.setSize(NSSize::new(across, inset));
    frame.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
    // SAFETY: an empty property list, which the header allows.
    let png = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }
    .expect("a PNG");
    std::fs::write(path, png.to_vec()).expect("written");
}

/// **A window AppKit draws as it would in a state no window never shown can
/// be in**: as the key window of an active app ([`Pretend::key`]), or in
/// full screen ([`Pretend::full_screen`]). The window's class, and the
/// application's for the first, are swapped in this process only for
/// subclasses that answer `isKeyWindow`, `isMainWindow` and `isActive`
/// yes -- and the window's own getters of its key, main and active
/// appearance, `_hasKeyAppearance` and the rest, which its title bar reads:
/// with the public three alone it still drew grey -- or give a style mask
/// with full screen in it. Nothing is ordered in,
/// activated or moved to a space -- the window is never shown and no other
/// app loses focus -- and every class is put back when it drops.
#[cfg(target_os = "macos")]
struct Pretend {
    swapped: Vec<(
        macos_liquid_glass::objc2::rc::Retained<macos_liquid_glass::objc2::runtime::AnyObject>,
        &'static macos_liquid_glass::objc2::runtime::AnyClass,
    )>,
}

#[cfg(target_os = "macos")]
thread_local! {
    /// The style mask a [`Pretend::full_screen`] window gives.
    static MASK: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

#[cfg(target_os = "macos")]
impl Pretend {
    /// `of`'s class swapped for its subclass named `prefix` and its own
    /// name, with `methods` added, made once.
    fn swap(
        &mut self,
        of: &macos_liquid_glass::objc2::runtime::AnyObject,
        prefix: &str,
        methods: impl Fn(&mut macos_liquid_glass::objc2::runtime::ClassBuilder),
    ) {
        use macos_liquid_glass::objc2::runtime::{AnyClass, AnyObject, ClassBuilder};
        let was = of.class();
        let name = std::ffi::CString::new(format!("{prefix}{}", was.name().to_string_lossy()))
            .expect("a class name");
        let class = AnyClass::get(&name).unwrap_or_else(|| {
            let mut b = ClassBuilder::new(&name, was).expect("a new class");
            methods(&mut b);
            b.register()
        });
        // SAFETY: a subclass of the object's own class that adds no ivar,
        // so the object's layout is unchanged.
        unsafe { AnyObject::set_class(of, class) };
        self.swapped.push((of.retain(), was));
    }

    fn key(window: &NSWindow, mtm: MainThreadMarker) -> Pretend {
        use macos_liquid_glass::objc2::runtime::{AnyObject, Bool, Sel};
        use macos_liquid_glass::objc2::sel;
        extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool {
            Bool::YES
        }
        let saying_yes = |sels: Vec<Sel>| {
            move |b: &mut macos_liquid_glass::objc2::runtime::ClassBuilder| {
                for &s in &sels {
                    // SAFETY: each selector a BOOL getter taking no
                    // argument, which `yes` is.
                    unsafe { b.add_method(s, yes as extern "C-unwind" fn(_, _) -> _) };
                }
            }
        };
        let mut p = Pretend {
            swapped: Vec::new(),
        };
        let window_says = saying_yes(vec![
            sel!(isKeyWindow),
            sel!(isMainWindow),
            sel!(_isKeyWindow),
            sel!(_isKeyWindowIgnoringFocus),
            sel!(_hasKeyAppearance),
            sel!(_hasMainAppearance),
            sel!(_hasActiveAppearance),
            sel!(_hasActiveAppearanceIgnoringKeyFocus),
            sel!(_hasActiveControls),
        ]);
        p.swap(window, "BaudKey", window_says);
        let app_says = saying_yes(vec![sel!(isActive)]);
        p.swap(
            &NSApplication::sharedApplication(mtm),
            "BaudActive",
            app_says,
        );
        p
    }

    fn full_screen(window: &NSWindow) -> Pretend {
        use macos_liquid_glass::objc2::runtime::{AnyObject, Sel};
        use macos_liquid_glass::objc2::sel;
        use macos_liquid_glass::objc2_app_kit::NSWindowStyleMask;
        extern "C-unwind" fn mask(_: &AnyObject, _: Sel) -> NSWindowStyleMask {
            NSWindowStyleMask(MASK.get())
        }
        MASK.set((window.styleMask() | NSWindowStyleMask::FullScreen).0);
        let mut p = Pretend {
            swapped: Vec::new(),
        };
        p.swap(window, "BaudFullScreen", |b| {
            // SAFETY: `styleMask` takes no argument and returns an
            // NSUInteger, which `mask` does.
            unsafe {
                b.add_method(sel!(styleMask), mask as extern "C-unwind" fn(_, _) -> _);
            }
        });
        p
    }
}

#[cfg(target_os = "macos")]
impl Drop for Pretend {
    fn drop(&mut self) {
        use macos_liquid_glass::objc2::runtime::AnyObject;
        for (of, was) in self.swapped.drain(..).rev() {
            // SAFETY: the class the object had, which the subclass derives
            // from without adding an ivar.
            unsafe { AnyObject::set_class(&of, was) };
        }
    }
}

/// **The title bar's left end as AppKit draws it, for each appearance at 1x
/// and 2x**, when `BAUD_ABOUT_RENDERS` names a folder: the window's frame
/// view into a bitmap, its window never shown, over a plain grey standing
/// for the material, which only a window on screen composites.
#[cfg(target_os = "macos")]
fn renders(mtm: MainThreadMarker) {
    use macos_liquid_glass::objc2::AnyThread;
    use macos_liquid_glass::objc2_app_kit::{
        NSBitmapFormat, NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSDeviceRGBColorSpace,
    };
    use macos_liquid_glass::objc2_foundation::NSDictionary;
    let Some(dir) = std::env::var_os("BAUD_ABOUT_RENDERS") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("the folder");
    const ACROSS: CGFloat = 360.0;
    for (name, look, grey) in [
        ("NSAppearanceNameDarkAqua", "dark", 0.227),
        ("NSAppearanceNameAqua", "light", 0.925),
    ] {
        let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
        let w = window.ns_window();
        let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
        window.set_appearance(Some(&a));
        w.setOpaque(true);
        w.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(grey, 1.0)));
        let inset = strip::inset_of(w);
        let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
        w.layoutIfNeeded();
        mark.place();
        mark.fit(w, None, mark::Fitted::default());
        // SAFETY: a view's superview, read and kept for this block; the
        // window that owns it lives past it.
        let frame = w
            .contentView()
            .and_then(|v| unsafe { v.superview() })
            .expect("the frame view");
        let top = frame.bounds().size.height - inset;
        let rect = NSRect::new(NSPoint::new(0.0, top), NSSize::new(ACROSS, inset));
        for scale in [1.0f64, 2.0] {
            let (pw, ph) = (
                (ACROSS * scale).round() as isize,
                (inset * scale).round() as isize,
            );
            // SAFETY: no planes, so AppKit allocates; the depth and the row
            // bytes are a 32-bit RGBA bitmap's.
            let rep = unsafe {
                NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                    NSBitmapImageRep::alloc(),
                    core::ptr::null_mut(),
                    pw,
                    ph,
                    8,
                    4,
                    true,
                    false,
                    NSDeviceRGBColorSpace,
                    NSBitmapFormat::empty(),
                    4 * pw,
                    32,
                )
            }
            .expect("a bitmap");
            rep.setSize(NSSize::new(ACROSS, inset));
            frame.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
            // SAFETY: an empty property list, which the header allows.
            let png = unsafe {
                rep.representationUsingType_properties(
                    NSBitmapImageFileType::PNG,
                    &NSDictionary::new(),
                )
            }
            .expect("a PNG");
            let path = dir.join(format!("titlebar-{look}-{}x.png", scale as u32));
            std::fs::write(&path, png.to_vec()).expect("written");
        }
    }
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

/// **The filming run's check, through a full screen and back**
/// (`full_screen_check.rs`), driven as the window would drive it but with
/// AppKit's two notifications posted by hand and the read at rest called
/// rather than waited for: armed, it waits for its file; made, it asks
/// for full screen; entered, it reads; at rest it reads and asks out;
/// exited, it reads and writes all three. The terminal's view is at the
/// top in the first two and not in the third, the lockup's and the
/// lights' accessories are read in the window, and the defaults' positive
/// control was found. Its toggles go to the main queue, which this test
/// never runs, and the check is disarmed before the test ends, so none
/// is ever made: the window is never shown.
#[cfg(target_os = "macos")]
fn reads_full_screen_at_rest(
    w: &NSWindow,
    view: &NSView,
    container: &NSView,
    mark: &mark::Mark,
    lights: &NSView,
    at: &str,
) {
    use full_screen_check::{GO, OUT, Parts, Step};
    let dir = std::env::temp_dir().join(format!("baud-fs-check-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp dir");
    let follow = {
        let (win, v, c) = (w.retain(), view.retain(), container.retain());
        full_screen::when_full_screen(w, move |change| strip::follow(change, &win, &v, &c))
    };
    full_screen_check::arm(
        Parts {
            window: w.retain(),
            view: view.retain(),
            container: container.retain(),
            lockup: Some(mark.accessory_view()),
            lights: Some(lights.retain()),
        },
        &dir,
    );
    full_screen_check::heard_dir();
    assert_eq!(
        full_screen_check::step(),
        Some(Step::Armed),
        "{at}: no file yet"
    );
    post(w, Posted::Entered);
    assert_eq!(
        full_screen_check::step(),
        Some(Step::Armed),
        "{at}: not its own"
    );
    post(w, Posted::Exited);
    std::fs::write(dir.join(GO), b"").expect("the file");
    full_screen_check::heard_dir();
    assert_eq!(full_screen_check::step(), Some(Step::Entering), "{at}");
    post(w, Posted::Entered);
    assert_eq!(full_screen_check::step(), Some(Step::Resting), "{at}");
    full_screen_check::rest_now();
    assert_eq!(full_screen_check::step(), Some(Step::Leaving), "{at}");
    assert!(!dir.join(OUT).exists(), "{at}: written before the exit");
    post(w, Posted::Exited);
    assert_eq!(full_screen_check::step(), Some(Step::Done), "{at}");
    let said = std::fs::read_to_string(dir.join(OUT)).expect("the check's file");
    full_screen_check::disarm();
    drop(follow);
    let _ = std::fs::remove_dir_all(&dir);
    let reading = |name: &str| -> String {
        let from = said.find(&format!("\"{name}\": {{")).expect(name);
        said[from..].lines().next().unwrap_or("").to_string()
    };
    for (name, top) in [("entered", true), ("at_rest", true), ("exited", false)] {
        let r = reading(name);
        assert!(
            r.contains(&format!("\"view_at_top\": {top}")),
            "{at}: {name}: {r}"
        );
        assert!(
            r.matches("\"in_window\": true").count() == 2,
            "{at}: {name}: {r}"
        );
    }
    assert!(
        said.contains("\"AppleLanguages\": \"found\""),
        "{at}: the control:\n{said}"
    );
    for key in ["AppleMenuBarVisibleInFullscreen", "_HIHideMenuBar"] {
        assert!(said.contains(&format!("\"{key}\": ")), "{at}: {key}");
    }
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
fn probe(w: &NSWindow, mark: &mark::Mark, inset: f64, at: &str) {
    let button = |b| {
        let v = w
            .standardWindowButton(b)
            .expect("a titled window has its buttons");
        v.convertRect_toView(v.bounds(), None)
    };
    eprintln!(
        "PROBE {at}: win {:?} inset {inset} close {:?} zoom {:?} mark {:?}",
        w.frame().size,
        button(NSWindowButton::CloseButton),
        button(NSWindowButton::ZoomButton),
        mark.view().convertRect_toView(mark.view().bounds(), None),
    );
}

/// **The lockup beside the traffic lights, with no lights in the title
/// bar, where baud's window has it**: see the module's doc.
#[cfg(target_os = "macos")]
fn check(
    w: &NSWindow,
    view: &NSView,
    mark: &mark::Mark,
    lights: &strip::Accessory,
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
    // The frame on whole device pixels, each edge within one of
    // the exact size centred in the accessory, and the picture scaled to
    // it on each axis, so it is drawn to the frame's edges: the frame's
    // right is the cursor's.
    let art = mark::Lockup::OnDark.art().expect("the lockup's size");
    let scale = w.backingScaleFactor();
    let exact = mark.size();
    let y = (b.size.height - exact.height) / 2.0;
    for (got, want) in [
        (m.origin.x, 0.0),
        (m.origin.x + m.size.width, exact.width),
        (m.origin.y, y),
        (m.origin.y + m.size.height, y + exact.height),
    ] {
        assert!(
            (got - want).abs() <= 1.0 / scale + EPS,
            "{at}: the frame {m:?} is not the lockup's {exact:?} to a pixel"
        );
        assert!(
            (got * scale - (got * scale).round()).abs() < EPS,
            "{at}: the frame {m:?} has an edge between device pixels at {scale}x"
        );
    }
    assert_eq!(
        mark.view().imageScaling(),
        macos_liquid_glass::objc2_app_kit::NSImageScaling::ScaleAxesIndependently,
        "{at}: the picture is not scaled to the frame's edges"
    );
    assert!(
        (mark.lockup_end() - (mw.origin.x + mw.size.width)).abs() < EPS,
        "{at}: the lockup ends at {}, its frame at {}",
        mark.lockup_end(),
        mw.origin.x + mw.size.width
    );
    assert!(!mark.on_plate(), "{at}: no plate");
    // **The build's name**: shown only while it fits past the lockup, whole
    // in the accessory beside it, on the strip's centre line, its text and
    // its size what they were put with.
    let label = mark.version_label();
    let lf = label.frame();
    let shows = mark::version_shows(
        w.frame().size.width,
        c.origin.x,
        mark.lockup_end(),
        lf.size.width,
    );
    assert_eq!(label.isHidden(), !shows, "{at}: shown as it fits");
    assert_eq!(label.stringValue().to_string(), VERSION, "{at}");
    assert!(
        (lf.origin.x - (m.size.width + mark::VERSION_GAP)).abs() < EPS,
        "{at}: the name at {lf:?}, the lockup {m:?}"
    );
    assert!(
        (lf.origin.y + lf.size.height / 2.0 - b.size.height / 2.0).abs() < 1.0,
        "{at}: the name's centre is not the strip's: {lf:?} in {b:?}"
    );
    if shows {
        assert!(
            lf.origin.x + lf.size.width <= b.size.width + EPS,
            "{at}: the name {lf:?} is outside its accessory {b:?}"
        );
    } else {
        // AppKit rounds an accessory's width out to a whole half point.
        assert!(
            (b.size.width - m.size.width).abs() < 1.0,
            "{at}: hidden, the accessory is the lockup's: {b:?}, {m:?}"
        );
    }
    // The mark's 168-unit cell at the points per unit the mark alone had:
    // its 300-unit box across a 22.4-point square, whatever the buttons'
    // size, or the bar less two points above and below where that is
    // shorter.
    let side = 22.4_f64.min(inset - 4.0).max(16.0);
    let cell = exact.height * 168.0 / art.height;
    assert!(
        (cell - side * 168.0 / 300.0).abs() < EPS,
        "{at}: the mark's cell is {cell} points, not {}",
        side * 168.0 / 300.0
    );
    // Nothing in the lights' accessory.
    let lw = lights.in_window();
    // And the terminal's view, below the title bar, overlaps neither.
    let v = view.convertRect_toView(view.bounds(), None);
    for (what, r) in [("the mark", mw), ("the lights", lw)] {
        assert!(
            r.origin.y >= v.origin.y + v.size.height - EPS,
            "{at}: {what} {r:?} overlaps the terminal's view {v:?}"
        );
    }
}

/// **A double-click on the title bar's empty part does what the user's
/// "Window title bar double-click action" says** (`baud_present_macos::
/// container`): in real windows built as `Opening` builds baud's -- the
/// container `Container::new` makes, the terminal's view in it, the build's
/// name, the lockup and the lights put by `mark.rs`, `strip.rs` and
/// `lights.rs` -- in both bodies, with the plate and without it, never
/// shown.
///
/// At each point of the bar, the view that wins the hit test is named.
/// Left of the close button, between the traffic lights, right of the zoom
/// button, the middle, between the name and the plate, and the right
/// inset with no plate, it is the container: the title bar is transparent
/// and lets clicks through to the content there. A double-click delivered
/// to it -- `mouseDown:` with a `clickCount` of 2, as AppKit hands a click
/// to the view it hit -- zooms under "Maximize", goes to the Dock under
/// "Minimize", is AppKit's `_zoomFill:` under "Fill", and under "None" and
/// under a word no release stores is passed on as a click, which the
/// window's frame answers with a drag; the unknown word is said once
/// however often it is met. The setting is put in the defaults' argument
/// domain for each case, the command line's, which the shared defaults
/// search after managed preferences and before the user's own
/// ([[source: sdk27-nsuserdefaults-h]] 22-33), so this Mac's setting never
/// decides and nothing is written to it -- a managed Mac's would, and
/// `Setting::set`'s read-back fails there.
///
/// **With no key, macOS's own default.** The argument domain cleared of the
/// key, the value read is the one in the registration domain, and the
/// double-click does what that value says, whatever it is; with the
/// registration domain holding "Minimize" (`registerDefaults:`, this
/// process only) it goes to the Dock, so a "Maximize" written into the code
/// for a missing key fails here. That the registration domain holds
/// "Maximize" is checked on its own, as the value macOS 27.3 registers.
/// The user's own preferences for every app come between the two, so the
/// test reads them, never writing them, and fails naming the key on a Mac
/// where they hold it. Fill on a window that does not answer
/// `respondsToSelector:` for `_zoomFill:` is `performZoom:`.
///
/// With the container answering no view to the hit test, the frame view's
/// hit at each empty point is printed, and is never under AppKit's
/// `NSTitlebarContainerView`: passing the point through would not reach
/// AppKit's double-click.
///
/// On the build's name, the plate and the right inset beside the plate the
/// winner is in the title bar's own views, under the view that carries
/// AppKit's double-click recognizer, and the double-click is AppKit's; the
/// recognizer acts only for a window on screen, so there this checks the
/// place and that the container takes no part, not the action. The traffic
/// lights are hit as AppKit's buttons and the plate as the strip's own
/// views (controls), a single click on the empty bar is still the window's
/// drag, and a double-click below the title bar, where the container is
/// hit with the terminal's view hidden, does nothing of the setting's.
///
/// The window's class is swapped for one that records `performZoom:`,
/// `performMiniaturize:`, `zoom:`, `miniaturize:`, `_zoomFill:` and
/// `performWindowDragWithEvent:` and does none of them, and that answers
/// every way of ordering it in, so nothing reaches the Dock or the screen;
/// the swap is checked before every delivery. Named breaks: a container
/// that is a plain `NSView` (master before this test) records only the
/// drag at every empty point; drop the `clickCount` test and the single
/// click zooms; drop the title bar's bounds and the double-click below it
/// zooms; drop the hit test and the double-click on the name, which the
/// title bar passes on to the content under it, zooms; an unknown word
/// zoomed; an unknown word not said; Fill zoomed; `_zoomFill:` called
/// without asking, and the window without it records it; a missing key
/// read as "Maximize" past the registration domain, and "Minimize"
/// registered zooms.
#[cfg(target_os = "macos")]
fn double_click(mtm: MainThreadMarker) {
    use baud_present_macos::container::Container;
    use baud_present_macos::container::{DoubleClick, SETTING, said};
    use baud_render::{ActivityLights, Shown, Theme};
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
    let _ = NSApplication::sharedApplication(mtm);
    let registered = Setting::registered();
    // The value observed on macOS 27.3, and only that: the no-key checks
    // below act on whatever the registration domain holds, not on this.
    assert_eq!(
        registered.as_deref(),
        Some("Maximize"),
        "the value AppKit registers on macOS 27.3"
    );
    assert_eq!(
        Setting::users_own(),
        None,
        "this user has {SETTING} set for every app, which a key missing from the argument \
         domain falls to before the registered value; the no-key checks need it unset, and \
         this test never writes the user's defaults"
    );
    let faces = [baud_font::Face::parse(embedded::BOLD.bytes).expect("the Bold")];
    let r = renderer_of(&faces, Theme::dark());
    let setting = Setting::kept();
    let mut delivered = 0;
    let mut wrong = Vec::new();
    for body in [Body::Standard, Body::Glass] {
        for lights in [ActivityLights::TitleBar, ActivityLights::Off] {
            let at = |step: &str| format!("{body:?} body, lights {lights:?}: {step}");
            let window = GlassWindow::new(mtm, WINDOW_SIZE, "baud");
            let w = window.ns_window();
            let inset = strip::inset_of(w);
            let built = NSRect::new(NSPoint::new(0.0, 0.0), WINDOW_SIZE);
            let container = Container::new(mtm, built);
            container.setAutoresizesSubviews(true);
            let view = NSView::initWithFrame(NSView::alloc(mtm), built);
            strip::apply(&view, &container, inset);
            view.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            container.addSubview(&view);
            window.set_content_view(&container);
            let mark = mark::Mark::put(mtm, &window, inset, VERSION).expect("the lockups decode");
            let holder = strip::Accessory::put(
                mtm,
                &window,
                TitlebarSide::Trailing,
                NSSize::new(0.0, inset),
            );
            let mut strip = lights::Strip::new(Some(holder), None, mark.size());
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
            draw_and_place(w, &mut strip, &mark, &r, lights, Shown::DARK, 2.0, inset);
            assert!(!w.isVisible(), "{}", at("never shown"));
            let frame = w.contentView().and_then(|v| unsafe { v.superview() });
            let frame = frame.expect("the window's frame view");
            let in_window = |v: &NSView| v.convertRect_toView(v.bounds(), None);
            let button = |b| {
                w.standardWindowButton(b)
                    .expect("a titled window's buttons")
            };
            let (close, mini, zoom) = (
                in_window(&button(NSWindowButton::CloseButton)),
                in_window(&button(NSWindowButton::MiniaturizeButton)),
                in_window(&button(NSWindowButton::ZoomButton)),
            );
            let bar = unsafe { button(NSWindowButton::CloseButton).superview() }
                .expect("the buttons' view");
            // AppKit's own double-click on the title bar: a two-click
            // recognizer on the view that holds the buttons' view.
            let recognizer = unsafe { bar.superview() }.is_some_and(|holder| {
                holder.gestureRecognizers().iter().any(|g| {
                    let two: isize = unsafe {
                        macos_liquid_glass::objc2::msg_send![&*g, numberOfClicksRequired]
                    };
                    g.class().name().to_bytes() == b"NSClickGestureRecognizer" && two == 2
                })
            });
            assert!(recognizer, "{}", at("AppKit's double-click recognizer"));
            let width = w.frame().size.width;
            let y = close.origin.y + close.size.height / 2.0;
            let name = in_window(mark.version_label());
            assert!(!mark.version_label().isHidden(), "{}", at("the name shows"));
            let plate = strip.plate_view().map(|p| in_window(p));
            let end = |r: NSRect| r.origin.x + r.size.width;
            let mut points = vec![
                (
                    "left of the close button",
                    close.origin.x / 2.0,
                    Wins::Container,
                ),
                (
                    "between close and minimise",
                    (end(close) + mini.origin.x) / 2.0,
                    Wins::Container,
                ),
                (
                    "between minimise and zoom",
                    (end(mini) + zoom.origin.x) / 2.0,
                    Wins::Container,
                ),
                ("right of the zoom button", end(zoom) + 2.0, Wins::Container),
                (
                    "the build's name",
                    name.origin.x + name.size.width / 2.0,
                    Wins::TitleBar,
                ),
            ];
            match plate {
                Some(p) => {
                    points.push((
                        "the middle",
                        (end(name) + p.origin.x) / 2.0,
                        Wins::Container,
                    ));
                    points.push((
                        "between the name and the plate",
                        p.origin.x - 4.0,
                        Wins::Container,
                    ));
                    points.push(("the right inset", (end(p) + width) / 2.0, Wins::TitleBar));
                    let hit = frame
                        .hitTest(NSPoint::new(p.origin.x + p.size.width / 2.0, y))
                        .expect("the plate is hit");
                    let ours = strip.plate_view().is_some_and(|v| same(v, &hit))
                        || same(mark.view(), &hit);
                    let lamp = baud_render::Led::ALL
                        .iter()
                        .any(|&l| strip.lamp_view(l).is_some_and(|v| same(v, &hit)));
                    assert!(
                        lamp || ours,
                        "{}: {}",
                        at("the plate is the strip's"),
                        hit.class().name().to_string_lossy()
                    );
                }
                None => {
                    points.push(("the middle", width / 2.0, Wins::Container));
                    points.push((
                        "between the name and the right edge",
                        (end(name) + width) / 2.0,
                        Wins::Container,
                    ));
                    points.push(("the right inset", width - 4.0, Wins::Container));
                }
            }
            for b in [
                NSWindowButton::CloseButton,
                NSWindowButton::MiniaturizeButton,
                NSWindowButton::ZoomButton,
            ] {
                let f = in_window(&button(b));
                let centre = NSPoint::new(f.origin.x + f.size.width / 2.0, y);
                let hit = frame.hitTest(centre).expect("a button is hit");
                assert!(
                    same(&hit, &button(b)),
                    "{}: {}",
                    at("a traffic light is AppKit's button"),
                    hit.class().name().to_string_lossy()
                );
            }
            {
                use macos_liquid_glass::objc2::sel;
                use macos_liquid_glass::objc2_foundation::NSObjectProtocol;
                assert!(
                    w.respondsToSelector(sel!(_zoomFill:)),
                    "{}",
                    at("the window answers for _zoomFill:, as on macOS 27.3")
                );
            }
            let recorder = Recorder::on(w, Fill::Answered);
            let event = |p: NSPoint, clicks: isize| -> Retained<NSEvent> {
                NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    NSEventType::LeftMouseDown,
                    p,
                    NSEventModifierFlags(0),
                    0.0,
                    w.windowNumber(),
                    None,
                    0,
                    clicks,
                    1.0,
                )
                .expect("a mouse event")
            };
            let empty: Vec<(&str, CGFloat)> = points
                .iter()
                .filter(|p| p.2 == Wins::Container)
                .map(|p| (p.0, p.1))
                .collect();
            for (what, x, wins) in points {
                let p = NSPoint::new(x, y);
                let step = at(what);
                let hit = frame.hitTest(p).expect("the bar is hit");
                let chain = chain_of(&hit);
                let won = if same(&hit, &container) {
                    Wins::Container
                } else if hit.isDescendantOf(&bar) {
                    Wins::TitleBar
                } else {
                    panic!("{step}: neither the container nor the title bar: {chain}")
                };
                assert_eq!(won, wins, "{step}: {chain}");
                match won {
                    Wins::TitleBar => {
                        recorder.deliver(w, &hit, &event(p, 2), Some("Maximize"), &setting);
                        assert_eq!(recorder.take(), [DRAG], "{step}: AppKit's, not ours");
                    }
                    Wins::Container => {
                        for (value, wanted) in [
                            ("Maximize", ZOOM),
                            ("Minimize", MINIATURIZE),
                            ("None", DRAG),
                            ("Fill", FILL),
                            (UNKNOWN, DRAG),
                        ] {
                            recorder.deliver(w, &hit, &event(p, 2), Some(value), &setting);
                            let done = recorder.take();
                            if done != [wanted] {
                                wrong.push(format!("{step}, {value}: {done:?}, not {wanted}"));
                            }
                            delivered += 1;
                        }
                        recorder.deliver(w, &hit, &event(p, 1), Some("Maximize"), &setting);
                        assert_eq!(recorder.take(), [DRAG], "{step}: a single click drags");
                    }
                }
                assert!(!w.isVisible(), "{step}: never shown");
            }
            // With the container answering no view, the frame view's hit at
            // each empty point is the body's, never under the title bar's
            // container: a container passing the point through would hand
            // the click to the body, not to AppKit's double-click.
            {
                let unhit = Unhit::on(&container);
                for &(what, x) in &empty {
                    let step = at(&format!("{what}, passed through"));
                    let hit = frame.hitTest(NSPoint::new(x, y));
                    let chain = hit.as_deref().map_or("nothing".to_owned(), chain_of);
                    assert!(!chain.contains(TITLE_BAR_CONTAINER), "{step}: {chain}");
                    println!("{step}: {chain}");
                }
                drop(unhit);
            }
            // Below the title bar, with the terminal's view hidden so the
            // container is the view hit there.
            view.setHidden(true);
            let below = NSPoint::new(width / 2.0, w.contentLayoutRect().size.height / 2.0);
            let hit = frame.hitTest(below).expect("the content is hit");
            assert!(same(&hit, &container), "{}", at("the container below"));
            recorder.deliver(w, &container, &event(below, 2), Some("Maximize"), &setting);
            view.setHidden(false);
            let done = recorder.take();
            assert!(
                !done.contains(&ZOOM) && !done.contains(&MINIATURIZE),
                "{}: {done:?}",
                at("below the title bar")
            );
            // No key in the argument domain: the value read is the one the
            // registration domain holds, and the double-click does what that
            // value says, whatever it is; then with the registration domain
            // holding "Minimize", in this process only, it goes to the Dock.
            let (what, x) = empty[0];
            let p = NSPoint::new(x, y);
            setting.clear();
            assert_eq!(
                Setting::read(),
                registered,
                "{}",
                at("no key: the registered value is read")
            );
            recorder.deliver(w, &container, &event(p, 2), None, &setting);
            assert_eq!(
                recorder.take(),
                [action_of(registered.as_deref())],
                "{}",
                at(&format!("{what}, no key: the registered value's action"))
            );
            {
                let _minimize = Registered::holding("Minimize");
                setting.clear();
                assert_eq!(Setting::read().as_deref(), Some("Minimize"));
                recorder.deliver(w, &container, &event(p, 2), None, &setting);
                assert_eq!(
                    recorder.take(),
                    [MINIATURIZE],
                    "{}",
                    at(&format!("{what}, no key, \"Minimize\" registered"))
                );
            }
            assert_eq!(Setting::read(), registered, "{}", at("registered put back"));
            drop(recorder);
            // Fill on a window without `_zoomFill:` is a zoom.
            let refusing = Recorder::on(w, Fill::Refused);
            refusing.deliver(w, &container, &event(p, 2), Some("Fill"), &setting);
            assert_eq!(
                refusing.take(),
                [ZOOM],
                "{}",
                at(&format!("{what}, Fill without _zoomFill:"))
            );
            drop(refusing);
            assert!(!w.isVisible(), "{}", at("never shown"));
        }
    }
    drop(setting);
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    let unknown = DoubleClick::of(Some(UNKNOWN));
    assert_eq!(
        said().iter().filter(|v| **v == unknown).count(),
        1,
        "an unknown word is said once, naming it"
    );
    assert_eq!(delivered, 2 * 5 * (6 + 7), "every empty point, every value");
    println!("{DOUBLE_CLICK}: ok, {delivered} double-clicks");
}

/// Which view a point of the title bar is hit as.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Wins {
    /// The terminal's container, under the transparent title bar.
    Container,
    /// One of the title bar's own views.
    TitleBar,
}

/// Whether `a` and `b` are one view.
#[cfg(target_os = "macos")]
fn same(a: &NSView, b: &NSView) -> bool {
    core::ptr::eq(a, b)
}

/// `view` and its superviews, by class, for a failure's message.
#[cfg(target_os = "macos")]
fn chain_of(view: &NSView) -> String {
    let mut names = Vec::new();
    let mut at = Some(view.retain());
    while let Some(v) = at {
        names.push(v.class().name().to_string_lossy().into_owned());
        at = unsafe { v.superview() };
    }
    names.join(" < ")
}

/// The class of the view AppKit holds the title bar's own views in.
#[cfg(target_os = "macos")]
const TITLE_BAR_CONTAINER: &str = "NSTitlebarContainerView";

/// **The container answering no view to every hit test**: its class
/// swapped for one whose `hitTest:` returns nil, so the frame view's hit
/// where the container was is what AppKit gives when the content passes a
/// point through. Put back when it drops.
#[cfg(target_os = "macos")]
struct Unhit {
    _swap: Pretend,
}

#[cfg(target_os = "macos")]
impl Unhit {
    fn on(view: &NSView) -> Unhit {
        use macos_liquid_glass::objc2::runtime::{AnyObject, Sel};
        use macos_liquid_glass::objc2::sel;
        extern "C-unwind" fn nothing(
            _: &AnyObject,
            _: Sel,
            _: NSPoint,
        ) -> Option<&'static AnyObject> {
            None
        }
        let mut swap = Pretend {
            swapped: Vec::new(),
        };
        swap.swap(view, "BaudUnhit", |b| {
            // SAFETY: `hitTest:` takes an NSPoint and returns a view or
            // nil, which `nothing` does.
            unsafe {
                b.add_method(
                    sel!(hitTest:),
                    nothing as extern "C-unwind" fn(_, _, _) -> _,
                );
            }
        });
        Unhit { _swap: swap }
    }
}

#[cfg(target_os = "macos")]
const ZOOM: &str = "performZoom:";
#[cfg(target_os = "macos")]
const MINIATURIZE: &str = "performMiniaturize:";
#[cfg(target_os = "macos")]
const DRAG: &str = "performWindowDragWithEvent:";
#[cfg(target_os = "macos")]
const FILL: &str = "_zoomFill:";

#[cfg(target_os = "macos")]
thread_local! {
    /// What the recording window was asked to do, in order.
    static ASKED: core::cell::RefCell<Vec<&'static str>> = const {
        core::cell::RefCell::new(Vec::new())
    };
}

/// **The user's double-click setting, as the command line would give it**:
/// the defaults' argument domain, searched after managed preferences only,
/// held to one value for a delivery and put back as it was when this
/// drops. Volatile: it is this process's alone and is never written.
#[cfg(target_os = "macos")]
struct Setting {
    was: macos_liquid_glass::objc2::rc::Retained<
        macos_liquid_glass::objc2_foundation::NSDictionary<
            NSString,
            macos_liquid_glass::objc2::runtime::AnyObject,
        >,
    >,
}

#[cfg(target_os = "macos")]
impl Setting {
    fn kept() -> Setting {
        use macos_liquid_glass::objc2_foundation::{NSArgumentDomain, NSUserDefaults};
        // SAFETY: a framework constant, valid for the process's life.
        let was = NSUserDefaults::standardUserDefaults()
            .volatileDomainForName(unsafe { NSArgumentDomain });
        Setting { was }
    }

    /// The setting read as `value` from now on.
    fn set(&self, value: &str) {
        use macos_liquid_glass::objc2::runtime::AnyObject;
        use macos_liquid_glass::objc2_foundation::{
            NSArgumentDomain, NSDictionary, NSUserDefaults,
        };
        let key = NSString::from_str(baud_present_macos::container::SETTING);
        let value = NSString::from_str(value);
        let one = NSDictionary::from_slices(&[&*key], &[&*value as &AnyObject]);
        let d = NSUserDefaults::standardUserDefaults();
        // SAFETY: a framework constant, and a dictionary of strings.
        unsafe { d.setVolatileDomain_forName(&one, NSArgumentDomain) };
        assert_eq!(
            d.stringForKey(&key).map(|s| s.to_string()),
            Some(value.to_string()),
            "the argument domain is read first"
        );
    }

    /// The argument domain as it was, without the setting: the value read
    /// from now on is the user's own, or else the registered one.
    fn clear(&self) {
        use macos_liquid_glass::objc2_foundation::{
            NSArgumentDomain, NSMutableDictionary, NSUserDefaults,
        };
        let without = NSMutableDictionary::dictionaryWithDictionary(&self.was);
        without.removeObjectForKey(&NSString::from_str(baud_present_macos::container::SETTING));
        // SAFETY: a framework constant, and the domain as it was less a key.
        unsafe {
            NSUserDefaults::standardUserDefaults()
                .setVolatileDomain_forName(&without, NSArgumentDomain);
        }
    }

    /// The setting as the shared defaults give it now, every domain
    /// searched, as `DoubleClick::read` reads it.
    fn read() -> Option<String> {
        use macos_liquid_glass::objc2_foundation::NSUserDefaults;
        NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str(baud_present_macos::container::SETTING))
            .map(|s| s.to_string())
    }

    /// The setting in the registration domain alone: what AppKit registered.
    fn registered() -> Option<String> {
        use macos_liquid_glass::objc2_foundation::{NSRegistrationDomain, NSUserDefaults};
        // SAFETY: a framework constant, valid for the process's life.
        let domain = NSUserDefaults::standardUserDefaults()
            .volatileDomainForName(unsafe { NSRegistrationDomain });
        domain
            .objectForKey(&NSString::from_str(baud_present_macos::container::SETTING))
            .and_then(|v| v.downcast::<NSString>().ok())
            .map(|s| s.to_string())
    }

    /// The setting in the user's own preferences for every app, read and
    /// never written.
    fn users_own() -> Option<String> {
        use macos_liquid_glass::objc2_foundation::{NSGlobalDomain, NSUserDefaults};
        // SAFETY: a framework constant, valid for the process's life.
        let domain = NSUserDefaults::standardUserDefaults()
            .persistentDomainForName(unsafe { NSGlobalDomain })?;
        domain
            .objectForKey(&NSString::from_str(baud_present_macos::container::SETTING))
            .map(|v| format!("{v:?}"))
    }
}

/// **The registration domain holding a value for the setting, in this
/// process only**, as AppKit registers its own (`registerDefaults:`, which
/// writes nothing to disk); the value it held is registered again when this
/// drops.
#[cfg(target_os = "macos")]
struct Registered {
    was: Option<String>,
}

#[cfg(target_os = "macos")]
impl Registered {
    fn holding(value: &str) -> Registered {
        let was = Setting::registered();
        Registered::register(value);
        assert_eq!(Setting::registered().as_deref(), Some(value));
        Registered { was }
    }

    fn register(value: &str) {
        use macos_liquid_glass::objc2::runtime::AnyObject;
        use macos_liquid_glass::objc2_foundation::{NSDictionary, NSUserDefaults};
        let key = NSString::from_str(baud_present_macos::container::SETTING);
        let value = NSString::from_str(value);
        let one = NSDictionary::from_slices(&[&*key], &[&*value as &AnyObject]);
        // SAFETY: a dictionary of strings, property-list objects.
        unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&one) };
    }
}

#[cfg(target_os = "macos")]
impl Drop for Registered {
    fn drop(&mut self) {
        if let Some(was) = self.was.as_deref() {
            Registered::register(was);
        }
    }
}

/// A word no release of the setting holds.
#[cfg(target_os = "macos")]
const UNKNOWN: &str = "a word no release stores";

/// What the recording window records for a double-click under `value`, as
/// the setting's words are documented: zoom, the Dock, Fill, or the click
/// passed on to the frame's drag.
#[cfg(target_os = "macos")]
fn action_of(value: Option<&str>) -> &'static str {
    match value {
        Some("Maximize") => ZOOM,
        Some("Minimize") => MINIATURIZE,
        Some("Fill") => FILL,
        _ => DRAG,
    }
}

/// Whether the recording window answers `respondsToSelector:` for
/// `_zoomFill:`: as every window on a macOS that has it, or as one without.
#[cfg(target_os = "macos")]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fill {
    Answered,
    Refused,
}

#[cfg(target_os = "macos")]
impl Drop for Setting {
    fn drop(&mut self) {
        use macos_liquid_glass::objc2_foundation::{NSArgumentDomain, NSUserDefaults};
        // SAFETY: a framework constant, and the domain as it was.
        unsafe {
            NSUserDefaults::standardUserDefaults()
                .setVolatileDomain_forName(&self.was, NSArgumentDomain);
        }
    }
}

/// **A window that records what it is asked to do and does none of it**:
/// its class swapped (`Pretend::swap`) for one whose zoom, miniaturize and
/// drag only record, and whose every way of being ordered in or made key
/// records too, so a double-click reaches neither the Dock nor the screen.
#[cfg(target_os = "macos")]
struct Recorder {
    _swap: Pretend,
}

/// The swapped class's name starts with this.
#[cfg(target_os = "macos")]
const RECORDING: &str = "BaudRecording";

#[cfg(target_os = "macos")]
impl Recorder {
    fn on(window: &NSWindow, fill: Fill) -> Recorder {
        use macos_liquid_glass::objc2::runtime::{AnyObject, Bool, ClassBuilder, Sel};
        use macos_liquid_glass::objc2::sel;
        fn note(s: Sel) {
            let name = s.name().to_string_lossy();
            let known = [ZOOM, MINIATURIZE, DRAG, FILL, "zoom:", "miniaturize:"];
            let word = known
                .into_iter()
                .find(|k| *k == name)
                .unwrap_or("ordered in");
            ASKED.with(|a| a.borrow_mut().push(word));
        }
        extern "C-unwind" fn one(_: &AnyObject, s: Sel, _: *mut AnyObject) {
            note(s);
        }
        extern "C-unwind" fn none(_: &AnyObject, s: Sel) {
            note(s);
        }
        extern "C-unwind" fn two(_: &AnyObject, s: Sel, _: isize, _: isize) {
            note(s);
        }
        // `respondsToSelector:` answered from the runtime for the swapped
        // class itself. Inherited, it answered no for `_zoomFill:` once the
        // class was swapped under the window's observed (KVO) class, though
        // the window answered yes before and the class has the method; so
        // both recorders answer it here, the one without Fill no for
        // `_zoomFill:`, which it still records if it is sent.
        extern "C-unwind" fn answers(this: &AnyObject, _: Sel, asked: Sel) -> Bool {
            Bool::new(this.class().responds_to(asked))
        }
        extern "C-unwind" fn answers_no_fill(this: &AnyObject, _: Sel, asked: Sel) -> Bool {
            Bool::new(asked != sel!(_zoomFill:) && this.class().responds_to(asked))
        }
        let mut swap = Pretend {
            swapped: Vec::new(),
        };
        let prefix = match fill {
            Fill::Answered => RECORDING.to_owned(),
            Fill::Refused => format!("{RECORDING}NoFill"),
        };
        swap.swap(window, &prefix, |b: &mut ClassBuilder| {
            let rts = match fill {
                Fill::Answered => answers as extern "C-unwind" fn(_, _, _) -> _,
                Fill::Refused => answers_no_fill as extern "C-unwind" fn(_, _, _) -> _,
            };
            // SAFETY: `respondsToSelector:` takes a selector and returns a
            // BOOL, which both answers do.
            unsafe { b.add_method(sel!(respondsToSelector:), rts) };
            for s in [
                sel!(performZoom:),
                sel!(performMiniaturize:),
                sel!(zoom:),
                sel!(miniaturize:),
                sel!(performWindowDragWithEvent:),
                sel!(_zoomFill:),
                sel!(orderFront:),
                sel!(makeKeyAndOrderFront:),
            ] {
                // SAFETY: each takes one object argument and returns
                // nothing, which `one` does.
                unsafe { b.add_method(s, one as extern "C-unwind" fn(_, _, _)) };
            }
            for s in [sel!(orderFrontRegardless), sel!(makeKeyWindow)] {
                // SAFETY: each takes no argument and returns nothing.
                unsafe { b.add_method(s, none as extern "C-unwind" fn(_, _)) };
            }
            // SAFETY: `orderWindow:relativeTo:` takes an NSWindowOrderingMode
            // and an NSInteger, both word-sized, and returns nothing.
            unsafe {
                b.add_method(
                    sel!(orderWindow:relativeTo:),
                    two as extern "C-unwind" fn(_, _, _, _),
                );
            }
        });
        Recorder { _swap: swap }
    }

    /// `event` sent to `view`'s `mouseDown:` with the setting read as
    /// `value`, the window's class checked first.
    fn deliver(
        &self,
        window: &NSWindow,
        view: &NSView,
        event: &macos_liquid_glass::objc2_app_kit::NSEvent,
        value: Option<&str>,
        setting: &Setting,
    ) {
        assert!(
            window
                .class()
                .name()
                .to_string_lossy()
                .starts_with(RECORDING),
            "the recorder is in place"
        );
        if let Some(v) = value {
            setting.set(v);
        }
        ASKED.with(|a| a.borrow_mut().clear());
        view.mouseDown(event);
    }

    /// What was asked since the last delivery.
    fn take(&self) -> Vec<&'static str> {
        ASKED.with(|a| core::mem::take(&mut *a.borrow_mut()))
    }
}
