//! **What the About window says, and where its film comes from.**
//! `baud_present_macos::about` is the window; this is baud's content: the
//! README's logo film from the app bundle's Resources, the lockup Reduce
//! Motion and a bare binary have instead, the version line, how old the
//! release is, the home, the licence, and the author's picture.
//!
//! The films are not in the binary: the two are 8.9 MB, and the `baud`
//! crate, 4.4 MiB against crates.io's 10, has no room for what a window
//! opened now and then plays. `scripts/bundle-macos.sh` copies them
//! beside the icon, where Apple's bundle layout keeps resources, and the
//! picture with them (`assets/readme/author.jpg`, 400 by 400): it ships
//! with the app and is never fetched, so a bare binary has none.
//!
//! **A click on the picture goes where every link baud opens goes**: the
//! caller's opener, which the app makes of its link policy and the OS's
//! opener (`app.rs`), so the profile's address passes the same allowlist as
//! a link in the text. The link line is not that path: it is AppKit's own
//! label link, which AppKit opens itself.
//!
//! **The age line counts from the commit's time**, which `build.rs` stamps
//! (`version::RELEASED`), and is worked out each time the window opens.

#![forbid(unsafe_code)]

use baud_present_macos::about::{About, Content, Face, LOGO, Logo, Motion};
use baud_render::{Argb, splash};
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker};
use macos_liquid_glass::objc2_app_kit::NSImage;
use macos_liquid_glass::objc2_core_foundation::CGFloat;
use macos_liquid_glass::objc2_foundation::{NSBundle, NSPoint, NSRect, NSSize, NSString};
use std::cell::OnceCell;
use std::rc::Rc;

/// baud's home.
pub(crate) const LINK: &str = "github.com/baud-terminal/baud";
/// Where it goes.
pub(crate) const URL: &str = "https://github.com/baud-terminal/baud";
/// **The copyright line**: `LICENSE`'s holder and its licence, which
/// `baud-app/tests/about_window.rs` holds to that file.
pub(crate) const COPYRIGHT: &str = "Copyright \u{a9} 2026 i-am-logger \u{b7} CC BY-NC-SA 4.0";

/// **The author's profile**, where a click on the picture goes.
pub(crate) const AUTHOR_URL: &str = "https://github.com/i-am-logger";
/// The picture's tooltip: its address, as the link line shows its own.
pub(crate) const AUTHOR_TIP: &str = "github.com/i-am-logger";
/// What VoiceOver names the picture.
pub(crate) const AUTHOR_LABEL: &str = "i-am-logger on GitHub";

/// The page round the film, the colour its GIF is encoded on: GitHub's
/// dark and light (`tools/baud-pictures/src/logo.rs`).
pub(crate) const PAGE_DARK: Argb = Argb(0xFF_0D_11_17);
pub(crate) const PAGE_LIGHT: Argb = Argb(0xFF_FF_FF_FF);

/// The two lockups, as the title bar has them: what a binary with no bundle
/// shows.
const STILL_DARK: &[u8] = include_bytes!("../assets/logo/baud-lockup-dark-tight.svg");
const STILL_LIGHT: &[u8] = include_bytes!("../assets/logo/baud-lockup-light-tight.svg");

/// **Where the film draws the lockup, in its units**: the film's canvas has
/// 60 to its left and 75 above it (`logo_film.rs`), shown at a point a unit.
const FILM_LEFT: CGFloat = 60.0;
const FILM_ABOVE: CGFloat = 75.0;

/// **Where the films move, in the logo's box from its lower left, in
/// points**: every pixel either film draws off its page, over all 320
/// frames of each, lies in columns 121 to 735 and rows 128 to 444 of the
/// 840 by 580 canvas, counted from its top left, decoded with baud's own
/// GIF decoder; at two pixels a point, rounded out to whole points, that is
/// 60 to 368 across and 64 to 223 down. The 67 points under it are bare
/// page on every frame, which the picture and the lines stand in, and
/// `baud-app/tests/about_window.rs` holds both films to it.
pub(crate) const MOVES: NSRect = NSRect::new(NSPoint::new(60.0, 67.0), NSSize::new(308.0, 159.0));

/// **The tight lockup's units a unit of the lockup's**: the mark's cell, 168
/// in the one (`tools/baud-pictures`) and 46 in the other.
const TIGHT: CGFloat = 168.0 / 46.0;

/// **The still's place in the logo's box, where the film draws the lockup**:
/// a point a lockup unit, centred across the box as the lockup's ink is, and
/// on the rows of the film's mark, as the still's own box is.
fn still_at(still: &NSImage) -> NSRect {
    let size = still.size();
    let (w, h) = (size.width / TIGHT, size.height / TIGHT);
    let across = FILM_LEFT + CGFloat::from(splash::BOX.0) / 2.0;
    // The film's block starts on a whole pixel of its 2x canvas.
    let top = ((FILM_ABOVE + CGFloat::from(splash::BLOCK.top)) * 2.0).round() / 2.0;
    let rows = top + CGFloat::from(splash::BLOCK.bottom - splash::BLOCK.top) / 2.0;
    NSRect::new(
        NSPoint::new(across - w / 2.0, LOGO.height - rows - h / 2.0),
        NSSize::new(w, h),
    )
}

/// `name.kind` in the main bundle's Resources; `None` with no bundle.
fn from_bundle(name: &str, kind: &str) -> Option<Retained<NSImage>> {
    let path = NSBundle::mainBundle().pathForResource_ofType(
        Some(&NSString::from_str(name)),
        Some(&NSString::from_str(kind)),
    )?;
    NSImage::initWithContentsOfFile(NSImage::alloc(), &path)
}

/// **What the bundle carries for the window**: each film, and the author's
/// picture; each `None` from a binary with no bundle.
pub(crate) struct Bundled {
    pub(crate) film_dark: Option<Retained<NSImage>>,
    pub(crate) film_light: Option<Retained<NSImage>>,
    pub(crate) face: Option<Retained<NSImage>>,
}

impl Bundled {
    /// The main bundle's.
    fn of_main() -> Bundled {
        Bundled {
            film_dark: from_bundle("logo-dark", "gif"),
            film_light: from_bundle("logo-light", "gif"),
            face: from_bundle("author", "jpg"),
        }
    }
}

/// **The window for `build`, its commit's time stamped as `stamp`, with what
/// `bundled` carries**, a click on the picture handed to `opens`: built, not
/// shown. `None` when a lockup does not decode.
pub(crate) fn build(
    mtm: MainThreadMarker,
    build: &str,
    stamp: &str,
    bundled: Bundled,
    opens: Rc<dyn Fn(&str)>,
) -> Option<About> {
    let version = crate::version::about_line(build);
    let Bundled {
        film_dark,
        film_light,
        face,
    } = bundled;
    let still_dark = macos_liquid_glass::image_from_bytes(STILL_DARK)?;
    Some(About::new(
        mtm,
        &Content {
            still_at: still_at(&still_dark),
            moves_at: MOVES,
            on_dark: Logo {
                film: film_dark,
                still: still_dark,
            },
            on_light: Logo {
                film: film_light,
                still: macos_liquid_glass::image_from_bytes(STILL_LIGHT)?,
            },
            page_dark: PAGE_DARK,
            page_light: PAGE_LIGHT,
            version: &version,
            link: LINK,
            url: URL,
            copyright: COPYRIGHT,
            face: face.map(|image| Face {
                image,
                url: AUTHOR_URL,
                label: AUTHOR_LABEL,
                tip: AUTHOR_TIP,
                open: opens,
            }),
            released: crate::version::released(build, stamp),
        },
    ))
}

thread_local! {
    /// The window, made at the first open and kept: closed, it waits.
    static WINDOW: OnceCell<Option<About>> = const { OnceCell::new() };
}

/// **The About item's action**: the window to the front, the logo from its
/// start, or the still under Reduce Motion, the age worked out now; a click
/// on the picture handed to `opens`.
pub(crate) fn open(mtm: MainThreadMarker, opens: fn(&str)) {
    WINDOW.with(|w| {
        let about = w.get_or_init(|| {
            build(
                mtm,
                crate::version::BUILD,
                crate::version::RELEASED,
                Bundled::of_main(),
                Rc::new(opens),
            )
        });
        if let Some(about) = about {
            about.open(match macos_liquid_glass::accessibility::reduce_motion() {
                true => Motion::Still,
                false => Motion::Plays,
            });
        }
    });
}
