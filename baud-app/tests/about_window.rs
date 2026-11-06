//! **The About window, built and never shown**, and the app menu's item that
//! opens it: what it says, the logo for each appearance, the film stepped to
//! a frame and at rest, the item's action, and the renders for each
//! appearance at 1x and 2x when `BAUD_ABOUT_RENDERS` names a folder.
//!
//! `harness = false` because AppKit views are main-thread-only and libtest
//! runs every test on a thread of its own; this `main` is the process's main
//! thread. It answers nextest's `--list` itself. Nothing here orders a window
//! in, and `About::open`, which does, is not called.

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/about.rs"]
mod about;

#[cfg(target_os = "macos")]
#[allow(dead_code)]
#[path = "../src/version.rs"]
mod version;

const NAME: &str = "the_about_window_says_the_build_and_plays_the_logo";

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
        checks::run();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = skipped;
}

#[cfg(target_os = "macos")]
mod checks {
    use baud_domain::FrameNo;
    use baud_present_macos::about::{About, Clock, Motion, add_menu_item};
    use core::cell::Cell;
    use core::time::Duration;
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker, sel};
    use macos_liquid_glass::objc2_app_kit::{
        NSAppearance, NSApplication, NSBitmapFormat, NSBitmapImageFileType, NSBitmapImageRep,
        NSDeviceRGBColorSpace, NSView,
    };
    use macos_liquid_glass::objc2_foundation::{NSDictionary, NSRect, NSString};
    use std::path::PathBuf;
    use std::rc::Rc;

    const BUILD: &str = "1.0.0-beta+0123abc.dirty";
    const DARK: &str = "NSAppearanceNameDarkAqua";
    const LIGHT: &str = "NSAppearanceNameAqua";

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn film(half: &str) -> Vec<u8> {
        std::fs::read(
            workspace()
                .join("assets/readme")
                .join(format!("logo-{half}.gif")),
        )
        .expect("the README's film")
    }

    /// `view`'s picture at `scale`, as AppKit draws it into a bitmap with no
    /// window on screen.
    pub(crate) fn picture(view: &NSView, scale: f64) -> Retained<NSBitmapImageRep> {
        let b: NSRect = view.bounds();
        let w = (b.size.width * scale).round() as isize;
        let h = (b.size.height * scale).round() as isize;
        // SAFETY: no planes, so AppKit allocates; the depth and the row bytes
        // are a 32-bit RGBA bitmap's.
        let rep = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bitmapFormat_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                core::ptr::null_mut(),
                w,
                h,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                NSBitmapFormat::empty(),
                4 * w,
                32,
            )
        }
        .expect("a bitmap");
        rep.setSize(b.size);
        view.cacheDisplayInRect_toBitmapImageRep(b, &rep);
        rep
    }

    /// A bitmap's bytes, as a PNG.
    fn png(rep: &NSBitmapImageRep) -> Vec<u8> {
        // SAFETY: an empty property list, which the header allows.
        let data = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }
        .expect("a PNG");
        data.to_vec()
    }

    fn set_look(about: &About, name: &str) {
        let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
        about.window().set_appearance(Some(&a));
    }

    pub(crate) fn run() {
        let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        let (dark, light) = (film("dark"), film("light"));

        let image = |b: &[u8]| macos_liquid_glass::image_from_bytes(b);
        let about =
            crate::about::build(mtm, BUILD, image(&dark), image(&light)).expect("the window");
        let lines = about.lines();
        assert_eq!(lines[0], "Version 1.0.0-beta (0123abc.dirty)");
        assert_eq!(lines[1], "github.com/baud-terminal/baud");
        assert_eq!(
            about.link_url().as_deref(),
            Some("https://github.com/baud-terminal/baud")
        );
        assert_eq!(
            lines[2],
            "Copyright \u{a9} 2026 i-am-logger \u{b7} CC BY-NC-SA 4.0"
        );
        assert!(!about.window().is_visible(), "never shown");

        // The copyright line says what LICENSE says.
        let licence = std::fs::read_to_string(workspace().join("LICENSE")).expect("LICENSE");
        assert!(
            licence.contains("Copyright (c) 2026 i-am-logger"),
            "the holder"
        );
        assert!(licence.contains("CC BY-NC-SA 4.0"), "the licence");

        // **What each frame lasts is the file's, as baud's own decoder reads
        // it**: AppKit's durations are what a tick steps by, and a clamped
        // delay would stretch the film past WCAG's five seconds. The last
        // frame is the rest: its delay does not count.
        for (name, gif) in [(DARK, &dark), (LIGHT, &light)] {
            set_look(&about, name);
            let lasts = about.film_lasts();
            let mut from_file = Vec::new();
            let decoded = baud_domain::gif::Gif::parse(gif).expect("the GIF");
            let mut scratch = vec![0; decoded.scratch_len()];
            decoded
                .decode(&mut scratch, |frame: baud_domain::gif::GifFrame<'_>| {
                    from_file.push(u64::from(frame.gap_ms));
                    true
                })
                .expect("every frame");
            assert_eq!(lasts.len(), from_file.len(), "{name}: frames");
            let moving: f64 = lasts[..lasts.len() - 1]
                .iter()
                .map(Duration::as_secs_f64)
                .sum();
            let want = from_file[..from_file.len() - 1].iter().sum::<u64>() as f64 / 1000.0;
            assert!(
                (moving - want).abs() < 1e-3,
                "{name}: AppKit's film moves {moving} s, the file's {want} s"
            );
            assert!(moving < 5.0, "{name}: {moving} s, under WCAG's five");
        }

        let content = about.window().content_view().expect("its view");
        // **Each appearance has its own logo on its own page**, and a flip
        // swaps it with nothing moved.
        let mut pages = Vec::new();
        for (name, want) in [(DARK, (0x0d, 0x11, 0x17)), (LIGHT, (0xff, 0xff, 0xff))] {
            set_look(&about, name);
            assert!(about.has_film(), "{name}: the film decoded");
            // The corner is the page: the logo's window merges with its film.
            let rep = picture(&content, 1.0);
            let c = rep
                .colorAtX_y(2, 2)
                .expect("a pixel")
                .colorUsingColorSpace(
                    &macos_liquid_glass::objc2_app_kit::NSColorSpace::sRGBColorSpace(),
                )
                .expect("in sRGB");
            let got = (
                (c.redComponent() * 255.0).round() as u8,
                (c.greenComponent() * 255.0).round() as u8,
                (c.blueComponent() * 255.0).round() as u8,
            );
            assert_eq!(got, want, "{name}: the page");
            pages.push(png(&rep));
        }
        assert_ne!(pages[0], pages[1], "the flip swapped nothing");

        // **The film steps**: its frames differ, a frame's picture is that
        // frame's, and it comes back to the rest. Perturbation: `Film::show`
        // that sets no frame draws the same picture for all of them.
        set_look(&about, DARK);
        let rest = about.rest_frame().expect("a film");
        let logo = about.logo_view();
        let mid = FrameNo::of(rest.raw() / 2);
        let shot = |frame: FrameNo| {
            about.show_frame(frame);
            png(&picture(&logo, 1.0))
        };
        let at_rest = shot(rest);
        let moving = shot(mid);
        assert_ne!(at_rest, moving, "the middle of the film is not its rest");
        assert_eq!(shot(rest), at_rest, "and it comes back");

        // **Reduce Motion rests at once and starts no clock**; plain, the film
        // goes to its first frame. A link made for a window never on screen
        // does not tick, so what a tick does is `frame_at`'s tests' and the
        // frames' above.
        about.play(Motion::Still);
        assert_eq!(about.film_state(), (Some(rest), Clock::Stopped), "still");
        about.play(Motion::Plays);
        assert_eq!(
            about.film_state(),
            (Some(FrameNo::of(0)), Clock::Running),
            "from the first frame"
        );
        about.play(Motion::Still);
        assert_eq!(
            about.film_state(),
            (Some(rest), Clock::Stopped),
            "and the clock stopped"
        );

        // **A binary with no bundle shows the lockup still**, with the same
        // words and nothing to play.
        let bare = crate::about::build(mtm, BUILD, None, None).expect("the window");
        assert!(!bare.has_film());
        assert_eq!(bare.rest_frame(), None);
        bare.play(Motion::Plays);
        assert_eq!(
            bare.film_state(),
            (None, Clock::Stopped),
            "no clock with no film"
        );

        // **The item**: first in the app menu, a separator under it, its
        // action the hook's, reached the way a click reaches it. The window
        // crate's own menu is what it goes in.
        macos_liquid_glass::menu::install_main_menu(&app, mtm, "baud");
        let opened = Rc::new(Cell::new(0));
        let count = opened.clone();
        let target = add_menu_item(&app, mtm, "About baud", move || count.set(count.get() + 1))
            .expect("an app menu");
        let bar = app.mainMenu().expect("the bar");
        let menu = bar
            .itemAtIndex(0)
            .and_then(|i| i.submenu())
            .expect("the app menu");
        let item = menu.itemAtIndex(0).expect("the first item");
        assert_eq!(item.title().to_string(), "About baud");
        assert!(menu.itemAtIndex(1).expect("the second").isSeparatorItem());
        assert_eq!(
            menu.itemAtIndex(2).expect("the third").title().to_string(),
            "Hide baud",
            "and the crate's items after"
        );
        let action = item.action().expect("an action");
        assert_eq!(action, sel!(openAbout:));
        // SAFETY: the item's own target and action, with itself as sender.
        let sent = unsafe { app.sendAction_to_from(action, Some(&target), Some(&item)) };
        assert!(sent, "AppKit delivered it");
        assert_eq!(opened.get(), 1, "the hook ran, once");
        assert!(!about.window().is_visible(), "and this test showed nothing");

        // The renders, when asked for.
        if let Some(dir) = std::env::var_os("BAUD_ABOUT_RENDERS") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("the folder");
            for (name, look) in [(DARK, "dark"), (LIGHT, "light")] {
                set_look(&about, name);
                about.play(Motion::Still);
                for scale in [1.0, 2.0] {
                    let frame = about.window().ns_window().contentView().expect("its view");
                    let path = dir.join(format!("about-{look}-{}x.png", scale as u32));
                    std::fs::write(&path, png(&picture(&frame, scale))).expect("written");
                    println!("{}", path.display());
                }
                // And one moving frame, the plasma in it.
                about.show_frame(mid);
                let frame = about.window().content_view().expect("its view");
                let path = dir.join(format!("about-{look}-2x-mid-film.png"));
                std::fs::write(&path, png(&picture(&frame, 2.0))).expect("written");
                println!("{}", path.display());
            }
        }
        println!("{NAME}: ok", NAME = super::NAME);
    }
}
