//! **When what XTWINOPS's reports answer from may have changed**: the
//! seven notifications after which a window's state, its frame, its screen
//! or its backing scale can differ from what was last read.
//!
//! - `NSWindowDidMoveNotification`, "A notification that the window object
//!   moved", sent only "when the window that moved didn’t also change size"
//!   (`[[source: apple-nswindow-didmovenotification]]`): a move that also
//!   resizes is heard as the next.
//! - `NSWindowDidResizeNotification`, "A notification that the window
//!   object size changed" (`[[source: apple-nswindow-didresizenotification]]`).
//! - `NSWindowDidChangeScreenNotification`, "A notification that a portion
//!   of the window object’s frame moved onto or off of a screen"
//!   (`[[source: apple-nswindow-didchangescreennotification]]`).
//! - `NSWindowDidChangeBackingPropertiesNotification`, "A notification that
//!   the window object backing properties changed", the scale every pixel
//!   fact is multiplied by
//!   (`[[source: apple-nswindow-didchangebackingpropertiesnotification]]`).
//! - `NSWindowDidMiniaturizeNotification` and
//!   `NSWindowDidDeminiaturizeNotification`, the window minimized and no
//!   longer (`[[source: apple-nswindow-didminiaturizenotification]]`,
//!   `[[source: apple-nswindow-diddeminiaturizenotification]]`).
//! - `NSApplicationDidChangeScreenParametersNotification`, "Posted when the
//!   configuration of the displays attached to the computer is changed",
//!   which moves the primary screen's top every position is measured from
//!   (`[[source: apple-nsapplication-didchangescreenparametersnotification]]`).
//!   It is the application's, posted with the shared application as its
//!   object, so it is observed with no object: each window's watch hears it.
//!
//! None of the pages says whether a move or a resize posts during a live
//! drag or only at its end, so the facts a drag moves -- the window's
//! position, the text area's and the window's size -- are read at the
//! question as well (baud-app's `report_now`, under `WindowHold::On`).
//!
//! The block runs on the posting thread, the main thread, and may run
//! inside one of the view's own callbacks: it marks and wakes, nothing more,
//! as `closing.rs`'s observers do.
//!
//! `unsafe` is allowed here for the registration, which the bindings mark
//! so, and for the framework constants it names.
#![allow(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use macos_liquid_glass::objc2_app_kit::{
    NSApplicationDidChangeScreenParametersNotification, NSWindow,
    NSWindowDidChangeBackingPropertiesNotification, NSWindowDidChangeScreenNotification,
    NSWindowDidDeminiaturizeNotification, NSWindowDidMiniaturizeNotification,
    NSWindowDidMoveNotification, NSWindowDidResizeNotification,
};
use macos_liquid_glass::objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName,
};

/// **A notification after which a window's facts may have changed.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    /// Moved, its size kept.
    Moved,
    /// Resized, and moved too where its top-left moved.
    Resized,
    /// Moved onto or off of a screen.
    ChangedScreen,
    /// Its backing scale or colour space changed.
    ChangedBacking,
    /// Minimized.
    Miniaturized,
    /// No longer minimized.
    Deminiaturized,
    /// The displays' configuration changed.
    ScreenParameters,
}

/// **Who posts a [`Notice`]**: the window, as its object, or the
/// application, with the shared application as its object.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Poster {
    Window,
    Application,
}

impl Notice {
    /// Every one, in the order they are registered.
    pub const ALL: [Notice; 7] = [
        Notice::Moved,
        Notice::Resized,
        Notice::ChangedScreen,
        Notice::ChangedBacking,
        Notice::Miniaturized,
        Notice::Deminiaturized,
        Notice::ScreenParameters,
    ];

    /// The notification's name, AppKit's constant.
    pub fn name(self) -> &'static NSNotificationName {
        // SAFETY: framework constants, valid strings for the process's life.
        unsafe {
            match self {
                Notice::Moved => NSWindowDidMoveNotification,
                Notice::Resized => NSWindowDidResizeNotification,
                Notice::ChangedScreen => NSWindowDidChangeScreenNotification,
                Notice::ChangedBacking => NSWindowDidChangeBackingPropertiesNotification,
                Notice::Miniaturized => NSWindowDidMiniaturizeNotification,
                Notice::Deminiaturized => NSWindowDidDeminiaturizeNotification,
                Notice::ScreenParameters => NSApplicationDidChangeScreenParametersNotification,
            }
        }
    }

    /// Who posts it, and so which object it is observed with.
    pub fn poster(self) -> Poster {
        match self {
            Notice::Moved
            | Notice::Resized
            | Notice::ChangedScreen
            | Notice::ChangedBacking
            | Notice::Miniaturized
            | Notice::Deminiaturized => Poster::Window,
            Notice::ScreenParameters => Poster::Application,
        }
    }
}

/// **The observers of one window's facts**: one for each [`Notice`];
/// dropping it removes them, so a window that has gone is heard no more.
#[must_use = "dropping it stops hearing the window"]
pub struct WindowWatch {
    tokens: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

/// **Calls `then` after each of [`Notice::ALL`]**: the six `window` posts,
/// and the displays' change, whoever's window, until the returned
/// [`WindowWatch`] drops.
pub fn when_window_changes(window: &NSWindow, then: impl Fn() + 'static) -> WindowWatch {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        crate::unwind::guard(
            crate::unwind::SiteName("a window's facts observer"),
            (),
            &then,
        );
    });
    let center = NSNotificationCenter::defaultCenter();
    let window: &AnyObject = window;
    let tokens = Notice::ALL
        .into_iter()
        .map(|n| {
            let object = match n.poster() {
                Poster::Window => Some(window),
                Poster::Application => None,
            };
            // SAFETY: the name is a framework constant; the object filter
            // is this window, or none for the application's, and no queue,
            // so the block runs on the posting thread -- the main thread,
            // where a window and the displays change -- which is what makes
            // a block over main-thread state sound here.
            unsafe {
                center.addObserverForName_object_queue_usingBlock(
                    Some(n.name()),
                    object,
                    None,
                    &block,
                )
            }
        })
        .collect();
    WindowWatch { tokens }
}

impl Drop for WindowWatch {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for token in &self.tokens {
            let observer: &AnyObject = token.as_ref();
            // SAFETY: each token is one `addObserverForName:` returned, an
            // object the centre registered.
            unsafe { center.removeObserver(observer) };
        }
    }
}

/// **What the frame rig posts for each [`Notice`]**: AppKit's constant
/// and its poster, from a table of the rig's own, never through
/// [`Notice::name`] or [`Notice::poster`], the mapping the rig's rows
/// test: a wrong name in that mapping leaves the rig's post unheard and
/// its row red.
///
/// Nothing for [`Notice::Moved`]: the rig moves the window by its origin
/// alone, and the move AppKit posts for that is the one heard
/// (`[[source: apple-nswindow-didmovenotification]]`).
#[cfg(any(test, feature = "effect-probe"))]
fn rig_posts(notice: Notice) -> Option<(&'static NSNotificationName, Poster)> {
    // SAFETY: framework constants, valid strings for the process's life.
    let posted = unsafe {
        match notice {
            Notice::Moved => return None,
            Notice::Resized => (NSWindowDidResizeNotification, Poster::Window),
            Notice::ChangedScreen => (NSWindowDidChangeScreenNotification, Poster::Window),
            Notice::ChangedBacking => (
                NSWindowDidChangeBackingPropertiesNotification,
                Poster::Window,
            ),
            Notice::Miniaturized => (NSWindowDidMiniaturizeNotification, Poster::Window),
            Notice::Deminiaturized => (NSWindowDidDeminiaturizeNotification, Poster::Window),
            Notice::ScreenParameters => (
                NSApplicationDidChangeScreenParametersNotification,
                Poster::Application,
            ),
        }
    };
    Some(posted)
}

/// **The frame rig's window told one of its facts changed**
/// (`rig_call::RigWindow`): the notification posted as AppKit posts it,
/// named by `rig_posts`, with the window as its object, or the shared
/// application for the displays' change. The window is never shown, and
/// nothing it reports is changed by the post; the rig moves it first for
/// [`Notice::Moved`].
#[cfg(feature = "effect-probe")]
pub fn rig_post(window: &NSWindow, notice: Notice) {
    use macos_liquid_glass::objc2::MainThreadMarker;
    use macos_liquid_glass::objc2_app_kit::NSApplication;
    use macos_liquid_glass::objc2_foundation::NSObject;
    fn object(o: &NSObject) -> &AnyObject {
        o
    }
    let Some((name, poster)) = rig_posts(notice) else {
        return;
    };
    let app = MainThreadMarker::new().map(NSApplication::sharedApplication);
    let object = match (poster, app.as_deref()) {
        (Poster::Window, _) => Some(object(window)),
        (Poster::Application, Some(a)) => Some(object(a)),
        (Poster::Application, None) => None,
    };
    // SAFETY: a framework constant posted with its object, on the main
    // thread, where its observers expect it.
    unsafe { NSNotificationCenter::defaultCenter().postNotificationName_object(name, object) };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Each of the seven is observed, once, with its poster**: every
    /// name AppKit gives them is the name of exactly one [`Notice`], and
    /// only the displays' change is observed with no object. Perturbation:
    /// `Notice::Moved` named as the resize, and the move is heard by no
    /// observer, the resize twice.
    #[test]
    fn each_window_notification_is_observed_once() {
        // SAFETY: framework constants.
        let names: [&NSNotificationName; 7] = unsafe {
            [
                NSWindowDidMoveNotification,
                NSWindowDidResizeNotification,
                NSWindowDidChangeScreenNotification,
                NSWindowDidChangeBackingPropertiesNotification,
                NSWindowDidMiniaturizeNotification,
                NSWindowDidDeminiaturizeNotification,
                NSApplicationDidChangeScreenParametersNotification,
            ]
        };
        for name in names {
            let by = Notice::ALL.iter().filter(|n| n.name() == name).count();
            assert_eq!(by, 1, "{name} is observed {by} times");
        }
        let applications: Vec<Notice> = Notice::ALL
            .into_iter()
            .filter(|n| n.poster() == Poster::Application)
            .collect();
        assert_eq!(applications, [Notice::ScreenParameters]);
    }

    /// **The watch registers every [`Notice`], each with its poster's
    /// object**: the registration walks [`Notice::ALL`], names each by
    /// [`Notice::name`] and filters by [`Notice::poster`], and nothing
    /// skips one. Perturbation: a `.filter(` on the walk, and the source
    /// check fails.
    #[test]
    fn the_watch_registers_every_notice() {
        let src = include_str!("window_facts.rs");
        let body = src
            .split_once(concat!("pub fn when_window_", "changes(window: &NSWindow"))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(b, _)| b)
            .expect("when_window_changes");
        assert!(body.contains(concat!("Notice::", "ALL\n")), "{body}");
        assert!(body.contains(concat!("Some(n.", "name())")), "{body}");
        assert!(body.contains(concat!("match n.", "poster()")), "{body}");
        assert!(!body.contains(concat!(".fil", "ter(")), "{body}");
        assert!(!body.contains(concat!(".ski", "p(")), "{body}");
    }

    /// **The rig posts AppKit's names from its own table**: each name
    /// `rig_posts` gives is the one AppKit spells for that notice, posted
    /// by the poster its page names, and `rig_post` names nothing through
    /// [`Notice::name`] or [`Notice::poster`]. The move posts
    /// nothing: AppKit's own post of it is heard. Perturbations:
    /// the table's resize named as the move, and the spelling check fails;
    /// `rig_post` posting `notice.name()`, and the source check fails.
    #[test]
    fn the_rig_posts_names_of_its_own() {
        let spelled = [
            (Notice::Moved, None),
            (
                Notice::Resized,
                Some(("NSWindowDidResizeNotification", Poster::Window)),
            ),
            (
                Notice::ChangedScreen,
                Some(("NSWindowDidChangeScreenNotification", Poster::Window)),
            ),
            (
                Notice::ChangedBacking,
                Some((
                    "NSWindowDidChangeBackingPropertiesNotification",
                    Poster::Window,
                )),
            ),
            (
                Notice::Miniaturized,
                Some(("NSWindowDidMiniaturizeNotification", Poster::Window)),
            ),
            (
                Notice::Deminiaturized,
                Some(("NSWindowDidDeminiaturizeNotification", Poster::Window)),
            ),
            (
                Notice::ScreenParameters,
                Some((
                    "NSApplicationDidChangeScreenParametersNotification",
                    Poster::Application,
                )),
            ),
        ];
        assert_eq!(spelled.map(|(n, _)| n), Notice::ALL);
        for (notice, spelling) in spelled {
            let posted = rig_posts(notice).map(|(name, by)| (name.to_string(), by));
            let spelled = spelling.map(|(s, by)| (s.to_owned(), by));
            assert_eq!(posted, spelled, "{notice:?}");
        }
        let src = include_str!("window_facts.rs");
        let body = src
            .split_once(concat!("pub fn rig_", "post(window: &NSWindow"))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(b, _)| b)
            .expect("rig_post");
        assert!(body.contains(concat!("rig_", "posts(notice)")), "{body}");
        assert!(!body.contains(concat!(".na", "me()")), "{body}");
        assert!(!body.contains(concat!(".pos", "ter()")), "{body}");
    }
}
