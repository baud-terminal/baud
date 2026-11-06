//! **What the About window says, and where its film comes from.**
//! `baud_present_macos::about` is the window; this is baud's content: the
//! README's logo film from the app bundle's Resources, the lockup a bare
//! binary has instead, the version line, the home and the licence.
//!
//! The films are not in the binary: the two are 5.3 MB, and the `baud`
//! crate, 4.4 MiB against crates.io's 10, has no room for what a window
//! opened now and then plays. `scripts/bundle-macos.sh` copies them
//! beside the icon, where Apple's bundle layout keeps resources.

#![forbid(unsafe_code)]

use baud_present_macos::about::{About, Content, Logo, Motion};
use baud_render::Argb;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::{AnyThread, MainThreadMarker};
use macos_liquid_glass::objc2_app_kit::NSImage;
use macos_liquid_glass::objc2_foundation::{NSBundle, NSString};
use std::cell::OnceCell;

/// baud's home.
pub(crate) const LINK: &str = "github.com/baud-terminal/baud";
/// Where it goes.
pub(crate) const URL: &str = "https://github.com/baud-terminal/baud";
/// **The copyright line**: `LICENSE`'s holder and its licence, which
/// `baud-app/tests/about_window.rs` holds to that file.
pub(crate) const COPYRIGHT: &str = "Copyright \u{a9} 2026 i-am-logger \u{b7} CC BY-NC-SA 4.0";

/// The page round the film, the colour its GIF is encoded on: GitHub's
/// dark and light (`tools/baud-pictures/src/logo.rs`).
pub(crate) const PAGE_DARK: Argb = Argb(0xFF_0D_11_17);
pub(crate) const PAGE_LIGHT: Argb = Argb(0xFF_FF_FF_FF);

/// The two lockups, as the title bar has them: what a binary with no bundle
/// shows.
const STILL_DARK: &[u8] = include_bytes!("../assets/logo/baud-lockup-dark-tight.svg");
const STILL_LIGHT: &[u8] = include_bytes!("../assets/logo/baud-lockup-light-tight.svg");

/// `name.gif` in the main bundle's Resources; `None` with no bundle.
fn from_bundle(name: &str) -> Option<Retained<NSImage>> {
    let path = NSBundle::mainBundle().pathForResource_ofType(
        Some(&NSString::from_str(name)),
        Some(&NSString::from_str("gif")),
    )?;
    NSImage::initWithContentsOfFile(NSImage::alloc(), &path)
}

/// **The window for `build`, with `film_dark` and `film_light` if there are
/// films**: built, not shown. `None` when a lockup does not decode.
pub(crate) fn build(
    mtm: MainThreadMarker,
    build: &str,
    film_dark: Option<Retained<NSImage>>,
    film_light: Option<Retained<NSImage>>,
) -> Option<About> {
    let version = crate::version::about_line(build);
    Some(About::new(
        mtm,
        &Content {
            on_dark: Logo {
                film: film_dark,
                still: macos_liquid_glass::image_from_bytes(STILL_DARK)?,
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
        },
    ))
}

thread_local! {
    /// The window, made at the first open and kept: closed, it waits.
    static WINDOW: OnceCell<Option<About>> = const { OnceCell::new() };
}

/// **The About item's action**: the window to the front, the logo from its
/// start, or at rest under Reduce Motion.
pub(crate) fn open(mtm: MainThreadMarker) {
    WINDOW.with(|w| {
        let about = w.get_or_init(|| {
            build(
                mtm,
                crate::version::BUILD,
                from_bundle("logo-dark"),
                from_bundle("logo-light"),
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
