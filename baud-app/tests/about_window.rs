//! **The About window, built and never shown**, and the app menu's item that
//! opens it: what it says, the logo for each appearance, the film stepped to
//! a frame and coming round, its pause, the author's picture and its click,
//! the release's age, the window with neither, the item's action, and the renders for
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
    use crate::about::Bundled;
    use baud_domain::FrameNo;
    use baud_present_macos::about::{
        ACKNOWLEDGEMENTS, About, Ages, Clock, FACE, GAP, LOGO, Motion, Seen, Shows, add_menu_item,
        frame_at,
    };
    use core::cell::{Cell, RefCell};
    use core::time::Duration;
    use macos_liquid_glass::objc2::rc::Retained;
    use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker, sel};
    use macos_liquid_glass::objc2_app_kit::{
        NSAccessibility, NSAppearance, NSApplication, NSBitmapFormat, NSBitmapImageFileType,
        NSBitmapImageRep, NSDeviceRGBColorSpace, NSEvent, NSEventModifierFlags, NSEventType,
        NSView,
    };
    use macos_liquid_glass::objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize, NSString};
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::time::SystemTime;

    const BUILD: &str = "1.0.0-beta+0123abc.dirty";
    /// A commit time as `build.rs` stamps it: 936f9c19's.
    const STAMP: &str = "1794042906";
    /// The profile a click on the picture goes to.
    const PROFILE: &str = "https://github.com/i-am-logger";
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

    /// The author's picture, as the bundle carries it.
    fn face() -> Option<Retained<macos_liquid_glass::objc2_app_kit::NSImage>> {
        let bytes =
            std::fs::read(workspace().join("assets/readme/author.jpg")).expect("the picture");
        macos_liquid_glass::image_from_bytes(&bytes)
    }

    /// What an opener was handed, in order.
    type Got = Rc<RefCell<Vec<String>>>;

    /// An opener, as the About window takes one.
    type Opener = Rc<dyn Fn(&str)>;

    /// An opener that opens nothing and keeps what it was handed.
    fn recorder() -> (Got, Opener) {
        let got = Rc::new(RefCell::new(Vec::new()));
        let keep = got.clone();
        (
            got,
            Rc::new(move |url: &str| keep.borrow_mut().push(url.to_owned())),
        )
    }

    /// What the bundle carries, or nothing.
    fn bundled(
        films: Option<(&[u8], &[u8])>,
        face: Option<Retained<macos_liquid_glass::objc2_app_kit::NSImage>>,
    ) -> Bundled {
        let image = |b: &[u8]| macos_liquid_glass::image_from_bytes(b);
        Bundled {
            film_dark: films.and_then(|(d, _)| image(d)),
            film_light: films.and_then(|(_, l)| image(l)),
            face,
        }
    }

    /// A left click at `at` in the window, as AppKit makes one, sent to `view`.
    fn click(view: &NSView, at: NSPoint) {
        view.mouseDown(&mouse(NSEventType::LeftMouseDown, at));
    }

    /// A left button's `kind` at `at` in the window, as AppKit makes one.
    fn mouse(kind: NSEventType, at: NSPoint) -> Retained<NSEvent> {
        NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            kind,
            at,
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            1,
            1.0,
        )
        .expect("the event")
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

        let (_, opens) = recorder();
        let about = crate::about::build(
            mtm,
            BUILD,
            "",
            bundled(Some((dark.as_slice(), light.as_slice())), None),
            opens.clone(),
        )
        .expect("the window");
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
        the_acknowledgements_line_is_a_button_that_names_the_offline_route(&about);
        the_acknowledgements_line_opens_third_party_md_at_the_build(mtm);

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
        let bare = crate::about::build(mtm, BUILD, "", bundled(None, None), opens.clone())
            .expect("the window");
        the_acknowledgements_line_is_a_button_that_names_the_offline_route(&bare);
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

        the_films_move_only_where_the_pause_is(&dark, &light);
        the_picture_sits_beside_the_lines_round_and_off_the_film(mtm, &dark, &light);
        the_block_is_centred_for_a_long_and_a_short_version(mtm);
        a_click_on_the_picture_opens_its_profile(mtm);
        the_age_is_worked_out_at_each_open_and_only_with_a_commit(mtm);
        the_bare_window_has_neither_picture_nor_age(&bare);

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
        // The window as a bundled build shows it: the films, the picture and
        // an age line, read 3 hours after the commit.
        if let Some(dir) = std::env::var_os("BAUD_ABOUT_RENDERS") {
            let dir = PathBuf::from(dir);
            std::fs::create_dir_all(&dir).expect("the folder");
            let (_, opens) = recorder();
            let shown = crate::about::build(
                mtm,
                BUILD,
                STAMP,
                bundled(Some((dark.as_slice(), light.as_slice())), face()),
                opens,
            )
            .expect("the window");
            let made = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_042_906);
            shown.age_at(made + Duration::from_secs(3 * 3600));
            for (name, look) in [(DARK, "dark"), (LIGHT, "light")] {
                set_look(&shown, name);
                shown.play(Motion::Still);
                for scale in [1.0, 2.0] {
                    let frame = shown.window().ns_window().contentView().expect("its view");
                    let path = dir.join(format!("about-{look}-{}x.png", scale as u32));
                    std::fs::write(&path, png(&picture(&frame, scale))).expect("written");
                    println!("{}", path.display());
                }
                // And one moving frame, the plasma in it.
                shown.show_frame(mid);
                let frame = shown.window().content_view().expect("its view");
                let path = dir.join(format!("about-{look}-2x-mid-film.png"));
                std::fs::write(&path, png(&picture(&frame, 2.0))).expect("written");
                println!("{}", path.display());
            }
        }
        println!("{NAME}: ok", NAME = super::NAME);
    }

    /// A pixel of `shot` at `at`, in the content's points from its lower
    /// left, at `scale`.
    fn pixel(shot: &(usize, usize, Vec<u8>), scale: f64, at: NSPoint) -> [u8; 3] {
        let (w, h, px) = shot;
        let x = (at.x * scale).floor() as usize;
        let y = h - 1 - (at.y * scale).floor() as usize;
        let i = (y * w + x) * 3;
        [px[i], px[i + 1], px[i + 2]]
    }

    /// The top of `r`.
    fn top(r: &NSRect) -> f64 {
        r.origin.y + r.size.height
    }

    /// Whether `a` and `b` share any area.
    fn overlap(a: &NSRect, b: &NSRect) -> bool {
        a.origin.x < b.origin.x + b.size.width
            && b.origin.x < a.origin.x + a.size.width
            && a.origin.y < top(b)
            && b.origin.y < top(a)
    }

    /// **The author's picture as option 1 has it, under where the film
    /// moves**: 96 points and round, the lines 14 or 15 points to its right,
    /// left-aligned and centred on its middle; the pause and its tooltip
    /// only where the film moves, `MOVES` in the film's box; the picture's
    /// top `GAP` under it, the lines under it too and neither on it; the
    /// window as tall as that and no taller. Perturbation: a pause over the
    /// film's whole box fails the pause rect and puts the picture on it; the
    /// band under the box's bottom, as before, fails the height; a square
    /// picture fails the corner.
    fn the_picture_sits_beside_the_lines_round_and_off_the_film(
        mtm: MainThreadMarker,
        dark: &[u8],
        light: &[u8],
    ) {
        let (_, opens) = recorder();
        let signed = crate::about::build(
            mtm,
            BUILD,
            STAMP,
            bundled(Some((dark, light)), face()),
            opens,
        )
        .expect("the window");
        let l = signed.layout();
        assert_eq!((FACE, GAP), (96.0, 8.0));
        let moves = crate::about::MOVES;
        // 18 of margin, the picture, the gap, and the box from where the
        // film stops moving to its top: 18 + 96 + 8 + 223.
        assert_eq!(l.content, NSSize::new(420.0, 345.0), "the window");
        assert_eq!(
            l.film,
            NSRect::new(NSPoint::new(0.0, 55.0), LOGO),
            "the box"
        );
        assert_eq!(
            l.pause,
            NSRect::new(NSPoint::new(60.0, 122.0), NSSize::new(308.0, 159.0)),
            "where the film moves"
        );
        assert_eq!(l.pause.size, moves.size);
        signed.play(Motion::Plays);
        let (_, over) = signed.pause_tip().expect("the film's tooltip");
        assert_eq!(over, l.pause, "the tooltip is the pause's");
        let face = l.face.expect("the picture");
        assert_eq!(face.size, NSSize::new(FACE, FACE));
        assert_eq!(face.origin.y, 18.0, "on the margin");
        assert_eq!(l.pause.origin.y - top(&face), GAP, "the picture's gap");
        assert_eq!(
            l.lines.len(),
            5,
            "the version, the age, the link, the copyright and the acknowledgements line"
        );
        for r in &l.lines {
            assert_eq!(r.origin.x, l.lines[0].origin.x, "{r:?}: one column");
            let gap = r.origin.x - (face.origin.x + FACE);
            assert!(gap == 14.0 || gap == 15.0, "{r:?}: a gap of {gap}");
            assert!(r.origin.y >= 18.0, "{r:?}: over the margin");
            assert!(
                l.pause.origin.y - top(r) >= GAP,
                "{r:?}: under where the film moves"
            );
            assert!(!overlap(r, &l.pause), "{r:?}: on the pause");
            assert!(!overlap(r, &face), "{r:?}: on the picture");
        }
        assert!(!overlap(&face, &l.pause), "the picture on the pause");
        let middle = (top(&l.lines[0]) + l.lines[4].origin.y) / 2.0;
        let centre = face.origin.y + FACE / 2.0;
        assert!(
            (middle - centre).abs() <= 0.5,
            "the lines' middle {middle}, the picture's {centre}"
        );
        // A click on the film's bare band, beside the picture, is no pause.
        let content = signed.window().content_view().expect("its view");
        let band = NSPoint::new(400.0, l.pause.origin.y - 2.0);
        let before = signed.film_state();
        click(&content, band);
        assert_eq!(
            signed.film_state(),
            before,
            "a click under where the film moves"
        );
        // Round, and the photo inside: 3 points in from the square's corners
        // is the page, its middle and 3 points in from its left and right
        // edges the picture's own pixels (the file's, scaled to 192, are
        // 7 to 13 levels off the dark page there and far off the light).
        let (fx, fy) = (face.origin.x, face.origin.y);
        for name in [DARK, LIGHT] {
            set_look(&signed, name);
            signed.play(Motion::Still);
            let view = signed.window().content_view().expect("its view");
            let shot = rgb(&picture(&view, 2.0));
            // The page as this render has it: the content's corner.
            let page = pixel(&shot, 2.0, NSPoint::new(2.0, 2.0));
            let near = |a: [u8; 3]| a.iter().zip(&page).all(|(x, y)| x.abs_diff(*y) <= 1);
            for (x, y) in [(3.0, 3.0), (93.0, 3.0), (3.0, 93.0), (93.0, 93.0)] {
                let p = pixel(&shot, 2.0, NSPoint::new(fx + x, fy + y));
                assert!(
                    near(p),
                    "{name}: ({x}, {y}) in is {p:?}, not the page: the picture is not round"
                );
            }
            for (x, y) in [(48.0, 48.0), (3.0, 48.0), (93.0, 48.0)] {
                let p = pixel(&shot, 2.0, NSPoint::new(fx + x, fy + y));
                assert!(
                    !near(p),
                    "{name}: ({x}, {y}) in is the page: the picture is not drawn"
                );
            }
        }
    }

    /// **The picture and the lines centred as one block, the margins whole
    /// and equal**: the space left of the picture is exactly the space right
    /// of the lines' column, which is the longest line's text rounded up to a
    /// whole point, in both looks, for a long version with an age line and a
    /// short one without; the gap from the picture is 14 or 15, taking the
    /// odd point; every line's text fits its frame. (The copyright line, 269
    /// points at 11, is the longest in both today, so the block is 379 in
    /// 420: margins 20 and 20, gap 15.) Perturbation: the old rounding of
    /// the half-point margin gives 21 and 20; the picture back at the
    /// margin's corner gives 18 and 23.
    fn the_block_is_centred_for_a_long_and_a_short_version(mtm: MainThreadMarker) {
        let (_, opens) = recorder();
        for (build, stamp) in [
            ("1.0.0-beta.12+0123abc.dirty", STAMP),
            ("1.0.0+unknown", ""),
        ] {
            let about =
                crate::about::build(mtm, build, stamp, bundled(None, face()), opens.clone())
                    .expect("the window");
            for name in [DARK, LIGHT] {
                set_look(&about, name);
                let l = about.layout();
                let face = l.face.expect("the picture");
                let longest = l.text.iter().copied().fold(0.0, f64::max);
                let column = &l.lines[0];
                assert_eq!(column.size.width, longest.ceil(), "{build}: the column");
                let left = face.origin.x;
                let right = l.content.width - (column.origin.x + column.size.width);
                assert_eq!(left, right, "{build} {name}: {left} left, {right} right");
                assert_eq!(left, left.round(), "{build} {name}: a whole margin");
                let gap = column.origin.x - (face.origin.x + FACE);
                assert!(gap == 14.0 || gap == 15.0, "{build} {name}: a gap of {gap}");
                for (r, w) in l.lines.iter().zip(&l.text) {
                    assert!(
                        *w <= r.size.width,
                        "{build}: a line's text {w} past its frame {r:?}"
                    );
                }
            }
        }
    }

    /// **The films move only where the pause is**: every pixel of every
    /// frame of both films outside `MOVES` is its page, so the band the
    /// picture and the lines stand in is bare and nothing covers motion.
    /// Perturbation: a `MOVES` a point shorter at the bottom fails both.
    fn the_films_move_only_where_the_pause_is(dark: &[u8], light: &[u8]) {
        let m = crate::about::MOVES;
        // In the canvas's pixels from its top left, two to a point.
        let (x0, x1) = (
            (m.origin.x * 2.0) as usize,
            ((m.origin.x + m.size.width) * 2.0) as usize,
        );
        let y1 = ((LOGO.height - m.origin.y) * 2.0) as usize;
        let y0 = ((LOGO.height - m.origin.y - m.size.height) * 2.0) as usize;
        for (name, gif) in [("dark", dark), ("light", light)] {
            let decoded = baud_domain::gif::Gif::parse(gif).expect("the GIF");
            let w = decoded.size().width as usize;
            let mut scratch = vec![0; decoded.scratch_len()];
            let (mut frames, mut off) = (0, 0usize);
            decoded
                .decode(&mut scratch, |frame: baud_domain::gif::GifFrame<'_>| {
                    let page = &frame.rgba[..4];
                    for (y, row) in frame.rgba.chunks_exact(w * 4).enumerate() {
                        for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
                            let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                            if !inside && px != page {
                                off += 1;
                            }
                        }
                    }
                    frames += 1;
                    true
                })
                .expect("every frame");
            assert_eq!(frames, 320, "{name}: every frame");
            assert_eq!(
                off, 0,
                "{name}: pixels off the page outside where the film moves"
            );
        }
    }

    /// **A click on the picture goes to the opener with the profile**, the
    /// press and release as AppKit makes them, through a seam that opens
    /// nothing; a release dragged off it does nothing; VoiceOver has it as
    /// a button with its label, and its press does what the click does;
    /// and a click on it does not pause the film. Perturbation: a `mouse_up`
    /// that opens wherever it is released fails the drag; no `hitTest:`
    /// reaching the picture fails the first.
    fn a_click_on_the_picture_opens_its_profile(mtm: MainThreadMarker) {
        let (got, opens) = recorder();
        let (dark, light) = (film("dark"), film("light"));
        let signed = crate::about::build(
            mtm,
            BUILD,
            STAMP,
            bundled(Some((dark.as_slice(), light.as_slice())), face()),
            opens,
        )
        .expect("the window");
        let content = signed.window().content_view().expect("its view");
        let face = signed.face_view().expect("the picture");
        let at = signed.layout().face.expect("its frame");
        let middle = NSPoint::new(at.origin.x + FACE / 2.0, at.origin.y + FACE / 2.0);
        let hit = content.hitTest(middle).expect("a view");
        assert!(
            core::ptr::eq(&*hit, &*face),
            "a click there is the picture's"
        );
        signed.play(Motion::Plays);
        let before = signed.film_state();
        face.mouseDown(&mouse(NSEventType::LeftMouseDown, middle));
        face.mouseUp(&mouse(NSEventType::LeftMouseUp, middle));
        assert_eq!(*got.borrow(), [PROFILE], "the click");
        assert_eq!(signed.film_state(), before, "and the film plays on");
        face.mouseDown(&mouse(NSEventType::LeftMouseDown, middle));
        face.mouseUp(&mouse(NSEventType::LeftMouseUp, NSPoint::new(300.0, 300.0)));
        assert_eq!(got.borrow().len(), 1, "a release off it opens nothing");
        assert_eq!(
            face.accessibilityLabel().map(|s| s.to_string()).as_deref(),
            Some("i-am-logger on GitHub")
        );
        assert_eq!(
            face.accessibilityRole().map(|s| s.to_string()).as_deref(),
            Some("AXButton")
        );
        assert!(face.isAccessibilityElement());
        assert!(face.accessibilityPerformPress(), "VoiceOver's press");
        assert_eq!(*got.borrow(), [PROFILE, PROFILE], "the press");
        assert_eq!(
            face.toolTip().map(|s| s.to_string()).as_deref(),
            Some("github.com/i-am-logger")
        );
        // The still: a start invalidates the film's display link first.
        signed.play(Motion::Still);
    }

    /// **The age under the version, worked out each time the window opens**,
    /// in the words the window's own formatter gives, the user's: a window
    /// kept for the app's run says a later age at a later open. And none at
    /// all for `+unknown` or with no time, the other lines closing up round
    /// the picture. Perturbation: words made once in `About::new` and never
    /// again fail the second open; an age shown for `+unknown` fails the
    /// third window.
    fn the_age_is_worked_out_at_each_open_and_only_with_a_commit(mtm: MainThreadMarker) {
        let (_, opens) = recorder();
        let signed = crate::about::build(mtm, BUILD, STAMP, bundled(None, face()), opens.clone())
            .expect("the window");
        let made = SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_042_906);
        let user = Ages::of_user();
        for later in [
            Duration::from_secs(3 * 3600),
            Duration::from_secs(5 * 86_400),
        ] {
            signed.age_at(made + later);
            assert_eq!(
                signed.age_line(),
                Some(user.words(made, made + later)),
                "{later:?} on"
            );
        }
        let at = |d| {
            signed.age_at(made + d);
            signed.age_line()
        };
        assert_ne!(
            at(Duration::from_secs(3 * 3600)),
            at(Duration::from_secs(5 * 86_400))
        );
        let l = signed.layout();
        let version = &l.lines[0];
        let age = &l.lines[1];
        assert!(
            age.origin.y + age.size.height <= version.origin.y,
            "under the version"
        );
        assert_eq!(signed.lines()[0], "Version 1.0.0-beta (0123abc.dirty)");
        for (build, stamp) in [("1.0.0-beta+unknown", STAMP), (BUILD, "")] {
            let none = crate::about::build(mtm, build, stamp, bundled(None, face()), opens.clone())
                .expect("the window");
            none.age_at(made + Duration::from_secs(60));
            assert_eq!(none.age_line(), None, "{build} {stamp:?}: no age");
            let l = none.layout();
            assert_eq!(l.lines.len(), 4, "{build}: four lines");
            let middle = (top(&l.lines[0]) + l.lines[3].origin.y) / 2.0;
            let face = l.face.expect("the picture");
            let centre = face.origin.y + FACE / 2.0;
            assert!(
                (middle - centre).abs() <= 0.5,
                "{build}: centred on the picture, {middle} and {centre}"
            );
        }
    }

    /// **The acknowledgements line is a button, and its tooltip names the
    /// offline route**: titled and named [`ACKNOWLEDGEMENTS`], an AXButton
    /// with its action and target, after the window's first responder in
    /// the key view loop, so Tab reaches it, and its tooltip and help name
    /// `baud --credits`. Built and read, never shown. Perturbation: the
    /// line made a label fails the role; the tooltip without the command
    /// fails the offline route.
    fn the_acknowledgements_line_is_a_button_that_names_the_offline_route(about: &About) {
        let line = about.acknowledgements_line();
        assert_eq!(line.title().to_string(), ACKNOWLEDGEMENTS);
        assert_eq!(
            line.accessibilityRole().map(|s| s.to_string()).as_deref(),
            Some("AXButton"),
            "VoiceOver has it as a button"
        );
        assert_eq!(
            line.accessibilityLabel().map(|s| s.to_string()).as_deref(),
            Some(ACKNOWLEDGEMENTS),
            "named by the one word"
        );
        let tip = line.toolTip().map(|s| s.to_string()).unwrap_or_default();
        assert!(
            tip.contains("`baud --credits`"),
            "the offline route: {tip:?}"
        );
        assert_eq!(
            line.accessibilityHelp().map(|s| s.to_string()).as_deref(),
            Some(tip.as_str()),
            "and VoiceOver's help says it"
        );
        assert_eq!(
            line.action(),
            Some(sel!(openAcknowledgements:)),
            "its action"
        );
        assert!(
            line.target().is_some(),
            "and the target that holds the opener"
        );
        assert!(line.isEnabled());
        let first = about.window().content_view().expect("its view");
        // SAFETY: a read of the loop AppKit keeps.
        let next = unsafe { first.nextKeyView() }.expect("a next key view");
        assert!(
            Retained::as_ptr(&next).cast::<u8>() == Retained::as_ptr(&line).cast::<u8>(),
            "Tab reaches the acknowledgements line"
        );
        let l = about.layout();
        let last = l.lines.last().expect("the acknowledgements line");
        assert_eq!(last.origin.x, l.lines[0].origin.x, "in the lines' column");
        assert!(last.origin.y >= 18.0, "over the margin");
    }

    /// **A press of the acknowledgements line asks to open THIRD_PARTY.md
    /// at the build's tree, and only that**, through the opener the window
    /// is given, here one that records, so no browser opens and no window is
    /// shown: at the commit for a build that names one, clean or dirty, and
    /// at the release tag `release-plz.toml` names for a build with no git
    /// (`+unknown`, a crates.io install), never with an empty ref.
    /// Perturbation: the ref taken whole from the identifier fails the dirty
    /// build; the tag as `v<version>` fails the build with no git.
    fn the_acknowledgements_line_opens_third_party_md_at_the_build(mtm: MainThreadMarker) {
        let plz = std::fs::read_to_string(workspace().join("release-plz.toml"))
            .expect("release-plz.toml");
        assert!(
            plz.contains("name = \"baud\"\ngit_tag_name = \"baud-v{{ version }}\""),
            "the baud package's tag name"
        );
        let blob = "https://github.com/baud-terminal/baud/blob";
        for (build, at) in [
            ("1.0.0-beta.3+0123abc", "0123abc"),
            ("1.0.0-beta.3+0123abc.dirty", "0123abc"),
            ("1.0.0-beta.3+unknown", "baud-v1.0.0-beta.3"),
        ] {
            let (got, opens) = recorder();
            let about = crate::about::build(mtm, build, "", bundled(None, None), opens)
                .expect("the window");
            assert!(
                got.borrow().is_empty(),
                "{build}: nothing opened before a press"
            );
            // SAFETY: a press as AppKit makes one, with no sender.
            unsafe { about.acknowledgements_line().performClick(None) };
            assert_eq!(
                *got.borrow(),
                [format!("{blob}/{at}/THIRD_PARTY.md")],
                "{build}"
            );
            assert!(!about.window().is_visible(), "never shown");
        }
    }

    /// **A binary with no bundle: no picture and, with no stamp, no age**:
    /// the three lines centred across the window, the band under where the
    /// film moves only their height, `GAP` under it, and nothing drawn
    /// left of them. Perturbation: a layout that keeps the picture's column
    /// with no picture fails the lines' `x`; a band kept at the picture's
    /// 96 fails the gap.
    fn the_bare_window_has_neither_picture_nor_age(bare: &About) {
        assert!(bare.face_view().is_none(), "no picture");
        assert_eq!(bare.age_line(), None, "no age");
        let l = bare.layout();
        assert_eq!(l.face, None);
        let block = top(&l.lines[0]) - l.lines[3].origin.y;
        assert_eq!(l.lines[3].origin.y, 18.0, "on the margin");
        assert_eq!(l.pause.origin.y - top(&l.lines[0]), GAP, "the gap");
        assert_eq!(
            l.film.origin.y + LOGO.height,
            l.content.height,
            "the box at the top"
        );
        assert_eq!(l.content.height, 18.0 + block + GAP + 223.0, "the window");
        assert_eq!(
            l.lines.len(),
            4,
            "three lines and the acknowledgements line"
        );
        for r in &l.lines {
            assert_eq!(
                (r.origin.x, r.size.width),
                (0.0, 420.0),
                "{r:?}: across the window"
            );
        }
        set_look(bare, DARK);
        bare.play(Motion::Still);
        let view = bare.window().content_view().expect("its view");
        let shot = rgb(&picture(&view, 2.0));
        let page = pixel(&shot, 2.0, NSPoint::new(2.0, 2.0));
        for at in [NSPoint::new(40.0, 25.0), NSPoint::new(40.0, 60.0)] {
            assert_eq!(pixel(&shot, 2.0, at), page, "{at:?}: the page");
        }
    }
}
