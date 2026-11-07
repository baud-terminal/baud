//! **The About window, built and never shown**, and the app menu's item that
//! opens it: what it says, the logo for each appearance, the film stepped to
//! a frame and coming round, its pause, the item's action, and the renders for
//! each appearance at 1x and 2x when `BAUD_ABOUT_RENDERS` names a folder, with
//! the whole film as raw frames for the flash census when `BAUD_ABOUT_FILM`
//! does.
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
    use baud_present_macos::about::{About, Clock, Motion, Seen, Shows, add_menu_item, frame_at};
    use core::cell::Cell;
    use core::time::Duration;
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker, sel};
    use macos_liquid_glass::objc2_app_kit::{
        NSAccessibility, NSAppearance, NSApplication, NSBitmapFormat, NSBitmapImageFileType,
        NSBitmapImageRep, NSDeviceRGBColorSpace, NSEvent, NSEventModifierFlags, NSEventType,
        NSView,
    };
    use macos_liquid_glass::objc2_foundation::{NSDictionary, NSPoint, NSRect, NSString};
    use std::path::PathBuf;
    use std::rc::Rc;

    const BUILD: &str = "1.0.0-beta+0123abc.dirty";
    const DARK: &str = "NSAppearanceNameDarkAqua";
    const LIGHT: &str = "NSAppearanceNameAqua";
    /// Nineteen of the film's 250 ms blink phases, which end with the block
    /// shown: the frame before this time has it, and the frame after does not.
    const BLOCK_SHOWN_TO: u64 = 4750;

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

    /// A left click at `at` in the window, as AppKit makes one, sent to `view`.
    fn click(view: &NSView, at: NSPoint) {
        let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::LeftMouseDown,
            at,
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            1,
            1.0,
        )
        .expect("the event");
        view.mouseDown(&event);
    }

    /// A key down of `text`, as AppKit makes one, sent to `view`.
    fn press(view: &NSView, text: &str) {
        let text = NSString::from_str(text);
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            &text,
            &text,
            false,
            0,
        )
        .expect("the event");
        view.keyDown(&event);
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
                    .as_chunks::<4>()
                    .0
                    .iter()
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
        for (i, p) in px.as_chunks::<3>().0.iter().enumerate() {
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
        let mut shown_to = FrameNo::of(0);

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
        assert!(
            licence.contains("Creative Commons Attribution-NonCommercial-ShareAlike 4.0"),
            "the licence"
        );

        // **What each frame lasts is the file's, as baud's own decoder reads
        // it**: AppKit's durations are what a tick steps by, and a clamped
        // delay would run the film slower than the file. **After the last
        // frame comes the first**, as the file's own count of zero has it.
        // Perturbation: `frame_at` that stops on the last frame fails the
        // last assert.
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
            let whole: Duration = lasts.iter().sum();
            let last = FrameNo::of(u32::try_from(lasts.len() - 1).expect("a frame"));
            let just = whole - Duration::from_millis(1);
            assert_eq!(frame_at(&lasts, just), last, "{name}: the last frame");
            assert_eq!(frame_at(&lasts, whole), FrameNo::of(0), "{name}: round");
            let phases = Duration::from_millis(BLOCK_SHOWN_TO);
            // To within a millisecond: AppKit gives each frame's time as a
            // double, so the frames that end at 4.75 s sum a hair short of it.
            let (mut end, mut frames) = (Duration::ZERO, 0);
            while end + Duration::from_millis(1) < phases {
                end += lasts[frames];
                frames += 1;
            }
            shown_to = FrameNo::of(u32::try_from(frames).expect("a frame"));
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
        let mid = FrameNo::of(shown_to.raw() / 2);
        let shot = |frame: FrameNo| {
            about.show_frame(frame);
            png(&picture(&logo, 1.0))
        };
        let first = shot(FrameNo::of(0));
        let moving = shot(mid);
        assert_ne!(first, moving, "the middle of the film is not its start");
        assert_eq!(shot(FrameNo::of(0)), first, "and it comes back");

        // **Reduce Motion shows the still at once and starts no clock**;
        // plain, the film goes to its first frame with its clock running. A
        // click or Space pauses it and plays it on, it stops drawing out of
        // sight and goes on when seen again, and a flip keeps it going.
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
            // **The pause, WCAG 2.2.2's mechanism, and being out of sight**:
            // each stops the clock and neither frees it alone. Perturbation:
            // `pace` reading only `hold` fails the last two.
            let running = (Some(FrameNo::of(0)), Clock::Running);
            let held = (Some(FrameNo::of(0)), Clock::Stopped);
            about.pause_or_play();
            assert_eq!(about.film_state(), held, "{name}: paused");
            about.pause_or_play();
            assert_eq!(about.film_state(), running, "{name}: on again");
            about.set_seen(Seen::No);
            assert_eq!(about.film_state(), held, "{name}: out of sight");
            about.pause_or_play();
            about.set_seen(Seen::Yes);
            assert_eq!(about.film_state(), held, "{name}: seen, and still paused");
            about.pause_or_play();
            assert_eq!(about.film_state(), running, "{name}: seen and free");
            // **A flip while it plays follows the look**, on the film, with
            // the clock going. Perturbation: a `follow` that rests on the
            // still fails the first.
            let film = png(&picture(&content, 2.0));
            set_look(&about, other);
            assert_eq!(about.film_state(), running, "{name}: a flip");
            assert_ne!(png(&picture(&content, 2.0)), film, "{name}: its look");
            set_look(&about, name);
            // **A flip on the still follows the look too**, and comes back.
            about.play(Motion::Still);
            set_look(&about, other);
            let flipped = png(&picture(&content, 2.0));
            assert_ne!(flipped, still, "{name}: a flip on the still");
            assert_eq!(about.shows(), Shows::Still, "{name}: still the still");
            set_look(&about, name);
            assert_eq!(png(&picture(&content, 2.0)), still, "{name}: and back");
        }

        // **A click on the film and Space reach the pause**, and a click on
        // the lines and any other key do not, the events as AppKit makes
        // them. Perturbation: `on_film` that is always true fails the click
        // on the lines; a `key_down` that takes every key fails the `a`.
        set_look(&about, DARK);
        about.play(Motion::Plays);
        if about.film_state().1 == Clock::Stopped {
            about.pause_or_play();
        }
        let running = (Some(FrameNo::of(0)), Clock::Running);
        let held = (Some(FrameNo::of(0)), Clock::Stopped);
        click(&content, NSPoint::new(5.0, 5.0));
        assert_eq!(about.film_state(), running, "a click on the lines");
        press(&content, "a");
        assert_eq!(about.film_state(), running, "another key");
        click(&content, NSPoint::new(210.0, 200.0));
        assert_eq!(about.film_state(), held, "a click on the film");
        press(&content, " ");
        assert_eq!(about.film_state(), running, "Space");
        // Under Reduce Motion neither has anything to pause.
        about.play(Motion::Still);
        press(&content, " ");
        assert_eq!(about.film_state(), (None, Clock::Stopped), "the still");

        // **A point on the film is the view's own**, not the image view's
        // above it, so a real click reaches the pause; and VoiceOver has the
        // pause as the logo's action while the film shows, and none on the
        // still. Perturbation: no `hitTest:` gives the image view.
        about.play(Motion::Plays);
        let on_film = NSPoint::new(210.0, 200.0);
        let hit = content.hitTest(on_film).expect("a view");
        assert!(
            core::ptr::eq(&*hit, &*content),
            "the film's point is the view's"
        );
        let actions = |about: &About| {
            about
                .logo_view()
                .accessibilityCustomActions()
                .map_or(0, |a| a.count())
        };
        assert_eq!(actions(&about), 1, "VoiceOver's pause");
        // **The pause's tooltip is the view's, over the film**, where a click
        // on the film lands, and the image view has none to compete with it.
        // Perturbation: the tooltip back on the image view fails the first.
        assert_eq!(about.logo_view().toolTip(), None, "none on the image view");
        let (text, over) = about.pause_tip().expect("the film's tooltip");
        assert_eq!(text, "Click or press Space to pause or play");
        let middle = NSPoint::new(
            over.origin.x + over.size.width / 2.0,
            over.origin.y + over.size.height / 2.0,
        );
        let hit = content.hitTest(middle).expect("a view");
        assert!(
            core::ptr::eq(&*hit, &*content),
            "the tooltip's point is the view's"
        );
        about.play(Motion::Still);
        assert_eq!(actions(&about), 0, "none on the still");
        assert_eq!(about.pause_tip(), None, "no tooltip on the still");

        // **A close ends the clock, and a pause holds for the app's run**:
        // opened again, the film is on its first frame, paused. Perturbation:
        // no `when_closing` leaves the clock running after the close; a start
        // that never paces its new link fails "opened again, paused".
        about.play(Motion::Plays);
        about.pause_or_play();
        about.window().ns_window().close();
        assert_eq!(about.film_state().1, Clock::Stopped, "closed");
        about.play(Motion::Plays);
        assert_eq!(about.film_state(), held, "opened again, paused");
        about.pause_or_play();
        assert_eq!(about.film_state(), running, "and on");

        // **The still is where the film draws the lockup**: on a frame with
        // the block shown the amber, the ❯'s left to the block's right, is
        // where the still has it, within a point at 2x. Perturbation: the
        // still filling the logo's box, as a bare window's used to, is 2.16
        // times the film's lockup.
        for name in [DARK, LIGHT] {
            set_look(&about, name);
            about.play(Motion::Still);
            let still = amber_box(&rgb(&picture(&content, 2.0))).expect("the still's amber");
            let frame = |f: FrameNo| {
                about.show_frame(f);
                rgb(&picture(&content, 2.0))
            };
            let last = amber_box(&frame(FrameNo::of(shown_to.raw() - 1))).expect("the amber");
            let next = amber_box(&frame(shown_to)).expect("the ❯");
            println!("{name}: the amber, the still's {still:?}, the film's {last:?}");
            for (a, b) in still.iter().zip(&last) {
                assert!(
                    a.abs_diff(*b) <= 2,
                    "{name}: the still's amber {still:?} is not the film's {last:?}"
                );
            }
            assert!(
                next[2] + 8 < last[2],
                "{name}: the block is hidden on the next: {next:?} against {last:?}"
            );
        }
        // **The whole film, as the flash census reads it through the window**,
        // when asked for: `flash_census` over one pass, which repeats.
        if let Some(dir) = std::env::var_os("BAUD_ABOUT_FILM") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("the folder");
            for (name, look) in [(DARK, "dark"), (LIGHT, "light")] {
                set_look(&about, name);
                let lasts = about.film_lasts();
                let mut frames = Vec::new();
                let mut repeats = String::new();
                let mut size = (0, 0);
                for (f, last) in lasts.iter().enumerate() {
                    about.show_frame(FrameNo::of(u32::try_from(f).expect("a frame")));
                    let (w, h, px) = rgb(&picture(&content, 2.0));
                    size = (w, h);
                    frames.extend_from_slice(&px);
                    let cs = (last.as_secs_f64() * 100.0).round() as u32;
                    repeats.push_str(&format!("{cs}\n"));
                }
                let rgb_at = dir.join(format!("film-{look}.rgb"));
                std::fs::write(&rgb_at, &frames).expect("written");
                let at = dir.join(format!("film-{look}.repeats"));
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
