//! **The About window, built and never shown**, and the app menu's item that
//! opens it: what it says, the logo for each appearance, the film stepped to
//! a frame and cut to the still, the item's action, and the renders for each
//! appearance at 1x and 2x when `BAUD_ABOUT_RENDERS` names a folder, with the
//! film up to the cut and the still as raw frames for the flash census when
//! `BAUD_ABOUT_CUT` does.
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
    use baud_present_macos::about::{
        About, Clock, Motion, PLAYS, Playing, Shows, add_menu_item, frame_at,
    };
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

    /// The README's film: the committed one, or under `BAUD_README_FILES`
    /// the one `baud-pictures logo --out` wrote there.
    fn film(half: &str) -> Vec<u8> {
        let dir = std::env::var_os("BAUD_README_FILES")
            .map_or_else(|| workspace().join("assets/readme"), PathBuf::from);
        std::fs::read(dir.join(format!("logo-{half}.gif"))).expect("the README's film")
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

    /// A bitmap's pixels, red, green and blue, row after row from the top.
    fn rgb(rep: &NSBitmapImageRep) -> (usize, usize, Vec<u8>) {
        let (w, h) = (rep.pixelsWide() as usize, rep.pixelsHigh() as usize);
        let stride = rep.bytesPerRow() as usize;
        let data = rep.bitmapData();
        assert!(!data.is_null(), "the bitmap's bytes");
        // SAFETY: AppKit's own buffer for a bitmap with no planes, `stride`
        // bytes a row for `h` rows, alive as long as `rep`.
        let px = unsafe { core::slice::from_raw_parts(data, stride * h) };
        let mut out = Vec::with_capacity(w * h * 3);
        for row in px.chunks_exact(stride) {
            out.extend(
                row[..w * 4]
                    .chunks_exact(4)
                    .flat_map(|p| [p[0], p[1], p[2]]),
            );
        }
        (w, h, out)
    }

    /// **The box of the pixels within 16 levels of the amber**: left, top,
    /// right and bottom, or `None`.
    fn amber_box((w, _, px): &(usize, usize, Vec<u8>)) -> Option<[usize; 4]> {
        let near = |p: &[u8]| {
            [0xF4u8, 0xA3, 0x2C]
                .iter()
                .zip(p)
                .all(|(a, b)| a.abs_diff(*b) <= 16)
        };
        let mut b: Option<[usize; 4]> = None;
        for (i, p) in px.chunks_exact(3).enumerate() {
            if near(p) {
                let (x, y) = (i % w, i / w);
                b = Some(match b {
                    None => [x, y, x, y],
                    Some([l, t, r, bo]) => [l.min(x), t.min(y), r.max(x), bo.max(y)],
                });
            }
        }
        b
    }

    fn set_look(about: &About, name: &str) {
        let a = NSAppearance::appearanceNamed(&NSString::from_str(name)).expect("a stock one");
        about.window().set_appearance(Some(&a));
    }

    pub(crate) fn run() {
        let mtm = MainThreadMarker::new().expect("harness = false runs main on the main thread");
        let app = NSApplication::sharedApplication(mtm);
        let (dark, light) = (film("dark"), film("light"));
        let mut cut = FrameNo::of(0);

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
        // delay would stretch the film past WCAG's five seconds. **The window
        // plays under five seconds and cuts on a frame's end**: the file
        // repeats for longer, and a cut inside a frame would show that frame
        // short. Perturbation: a `PLAYS` of 5 s fails the first, and of
        // 4.76 s the second.
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
            let moving: f64 = lasts.iter().map(Duration::as_secs_f64).sum();
            let want = from_file.iter().sum::<u64>() as f64 / 1000.0;
            assert!(
                (moving - want).abs() < 1e-3,
                "{name}: AppKit's film runs {moving} s, the file's {want} s"
            );
            assert!(
                PLAYS.as_secs_f64() < 5.0,
                "{name}: {PLAYS:?}, under WCAG's five"
            );
            let (mut end, mut frames) = (0.0, 0);
            while end + 1e-3 < PLAYS.as_secs_f64() {
                end += lasts[frames].as_secs_f64();
                frames += 1;
            }
            assert!(frames < lasts.len(), "{name}: the film ends at the cut");
            assert!(
                (end - PLAYS.as_secs_f64()).abs() < 1e-3,
                "{name}: the cut at {PLAYS:?} is inside a frame, which ends at {end} s"
            );
            cut = FrameNo::of(u32::try_from(frames).expect("a frame"));
            let last = FrameNo::of(cut.raw() - 1);
            let just = PLAYS - Duration::from_millis(1);
            assert_eq!(frame_at(&lasts, PLAYS, just), (last, Playing::Yes));
            assert_eq!(frame_at(&lasts, PLAYS, PLAYS).1, Playing::Over);
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
        // frame's, and it comes back. Perturbation: `Film::show` that sets no
        // frame draws the same picture for all of them.
        set_look(&about, DARK);
        let logo = about.logo_view();
        let mid = FrameNo::of(cut.raw() / 2);
        let shot = |frame: FrameNo| {
            about.show_frame(frame);
            png(&picture(&logo, 1.0))
        };
        let first = shot(FrameNo::of(0));
        let moving = shot(mid);
        assert_ne!(first, moving, "the middle of the film is not its start");
        assert_eq!(shot(FrameNo::of(0)), first, "and it comes back");

        // **Reduce Motion shows the still at once and starts no clock**;
        // plain, the film goes to its first frame; the cut stops the clock on
        // the still, whichever appearance it ends in and through a flip after.
        // A link made for a window never on screen does not tick, so what a
        // tick does is `frame_at`'s and the cut's.
        // A binary with no bundle shows the same picture, with nothing to
        // play. Perturbation: rest on `film.last()` and the film's last
        // frame, plasma and all, is not the bare window's.
        let bare = crate::about::build(mtm, BUILD, None, None).expect("the window");
        assert!(!bare.has_film());
        bare.play(Motion::Plays);
        assert_eq!(bare.shows(), Shows::Still);
        assert_eq!(
            bare.film_state(),
            (None, Clock::Stopped),
            "no clock with no film"
        );
        let bare_view = bare.window().content_view().expect("its view");
        for (name, other) in [(DARK, LIGHT), (LIGHT, DARK)] {
            set_look(&about, name);
            set_look(&bare, name);
            about.play(Motion::Still);
            assert_eq!(about.shows(), Shows::Still, "{name}");
            assert_eq!(about.film_state(), (None, Clock::Stopped), "{name}: still");
            let still = png(&picture(&content, 2.0));
            assert_eq!(
                still,
                png(&picture(&bare_view, 2.0)),
                "{name}: the bare window's picture"
            );
            about.play(Motion::Plays);
            assert_eq!(
                about.film_state(),
                (Some(FrameNo::of(0)), Clock::Running),
                "{name}: from the first frame"
            );
            assert_ne!(
                png(&picture(&content, 2.0)),
                still,
                "{name}: the film shows"
            );
            about.cut();
            assert_eq!(about.film_state(), (None, Clock::Stopped), "{name}: cut");
            assert_eq!(png(&picture(&content, 2.0)), still, "{name}: the still");
            set_look(&about, other);
            let flipped = png(&picture(&content, 2.0));
            assert_ne!(flipped, still, "{name}: a flip after the cut");
            assert_eq!(about.shows(), Shows::Still, "{name}: still the still");
            set_look(&about, name);
            assert_eq!(png(&picture(&content, 2.0)), still, "{name}: and back");
        }

        // **The cut moves only the plasma**: on the film's last frame the
        // block is shown, and on the next it is hidden, so the cut ends a
        // phase; and the amber, the ❯'s left to the block's right, is where
        // the still has it, within a point at 2x. Perturbation: the still
        // filling the logo's box, as a bare window's used to, is 2.16 times
        // the film's lockup.
        for name in [DARK, LIGHT] {
            set_look(&about, name);
            about.play(Motion::Still);
            let still = amber_box(&rgb(&picture(&content, 2.0))).expect("the still's amber");
            let frame = |f: FrameNo| {
                about.show_frame(f);
                rgb(&picture(&content, 2.0))
            };
            let last = amber_box(&frame(FrameNo::of(cut.raw() - 1))).expect("the amber");
            let next = amber_box(&frame(cut)).expect("the ❯");
            println!("{name}: the amber, the still's {still:?}, the film's {last:?}");
            for (a, b) in still.iter().zip(&last) {
                assert!(
                    a.abs_diff(*b) <= 2,
                    "{name}: the still's amber {still:?} is not the film's {last:?}"
                );
            }
            assert!(
                next[2] + 8 < last[2],
                "{name}: the block is hidden after the cut's frame: {next:?} against {last:?}"
            );
        }
        // **The film up to the cut and the still, as the flash census reads
        // them**, when asked for: `flash_census` over one pass, the still held.
        if let Some(dir) = std::env::var_os("BAUD_ABOUT_CUT") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("the folder");
            for (name, look) in [(DARK, "dark"), (LIGHT, "light")] {
                set_look(&about, name);
                let lasts = about.film_lasts();
                let mut frames = Vec::new();
                let mut repeats = String::new();
                let mut size = (0, 0);
                for f in 0..cut.raw() {
                    about.show_frame(FrameNo::of(f));
                    let (w, h, px) = rgb(&picture(&content, 2.0));
                    size = (w, h);
                    frames.extend_from_slice(&px);
                    let cs = (lasts[f as usize].as_secs_f64() * 100.0).round() as u32;
                    repeats.push_str(&format!("{cs}\n"));
                }
                about.play(Motion::Still);
                frames.extend_from_slice(&rgb(&picture(&content, 2.0)).2);
                repeats.push_str("1\n");
                let rgb_at = dir.join(format!("cut-{look}.rgb"));
                std::fs::write(&rgb_at, &frames).expect("written");
                let at = dir.join(format!("cut-{look}.repeats"));
                std::fs::write(&at, repeats).expect("written");
                println!("{} {} {}", rgb_at.display(), size.0, size.1);
            }
        }

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
