//! **The terminal's container, and a double-click on the title bar's empty
//! part**: the view the window's body holds, with the terminal's view inside
//! it below the title bar, which does what the user's "Window title bar
//! double-click action" setting says when the title bar is double-clicked
//! where nothing of AppKit's own is.
//!
//! **Why the container is hit there.** The window crate's titled window has
//! `NSWindowStyleMaskFullSizeContentView`, so the content runs under the
//! title bar ([[source: sdk27-nswindow-h]] 48), and `titlebarAppearsTransparent`,
//! which lets "click through" happen (308): a point in the title bar where
//! none of its own views is -- left of the close button, between the traffic
//! lights, the stretch past them, the gap before the activity lights' plate
//! -- is hit-tested (`NSView.h`'s `hitTest:`, [[source: sdk27-nsview-h]] 220)
//! into the content, and the content there is this view, the full width and
//! height of the body, the terminal's view stopping the title bar's height
//! short of its top. The title bar's own views -- the traffic lights, and the
//! accessories ([[source: macos-sdk-nstitlebaraccessoryviewcontroller-h]] 23)
//! that hold the build's name and the plate -- are AppKit's to hit, and a
//! double-click on them is AppKit's own title bar's. A click passed on
//! from here goes up the responder chain to the window's frame, which drags
//! the window (`performWindowDragWithEvent:`, [[source: sdk27-nswindow-h]]
//! 661), so a press and drag on the empty bar moves the window; but the
//! title bar's double-click is AppKit's only for a click hit-tested into the
//! title bar's views, so on the empty bar it did nothing. Were this view to
//! answer no view to the hit test, the frame view's hit there is the body's
//! (`baud-app/tests/titlebar_mark.rs` names it), never the title bar's, so
//! passing the point through would not reach AppKit's double-click either.
//!
//! **The setting decides, read on every double-click.** The Desktop & Dock
//! pane offers Fill, Zoom, Minimize and No Action
//! ([[source: apple-support-mac-desktop-dock-settings]], "Window title bar
//! double-click action"). It is read under `AppleActionOnDoubleClick` from
//! the shared defaults, whose search list runs from managed preferences
//! and the command line's arguments through the preferences "global to
//! all apps for the current user" to the registered ones
//! ([[source: sdk27-nsuserdefaults-h]] 22-33, 153). A user who never chose
//! has no value of their own, and the value read is the one AppKit
//! registers for the process, read where it is at run time and never
//! written in here. Each stored word is its own [`DoubleClick`]: the words
//! are those Chromium, Firefox and iTerm2 read for the same setting
//! ([[source: chromium-native-widget-mac-nswindow-e44f559]] 776-787,
//! [[source: firefox-nscocoautils-293c306]] 1215-1263,
//! [[source: iterm2-nswindow-iterm-2a8d67e]] 27-46); the older
//! `AppleMiniaturizeOnDoubleClick`, which none of them reads, is not read.
//! "Minimize" is `NSWindow`'s `performMiniaturize:` and "Maximize" its
//! `performZoom:` ([[source: sdk27-nswindow-h]] 491-492), and "None" is
//! nothing, the click passed on as any click there is. A double-click is
//! an event whose `clickCount` is 2 ([[source: macos-sdk-nsevent-h]] 329).
//!
//! **A zoom goes to the standard frame.** `zoom:` takes "a 'best fit'
//! frame" from `windowWillUseStandardFrame:defaultFrame:` when the window's
//! delegate or class has it, and otherwise "a default frame [that] nearly
//! fills the current screen" ([[source: apple-nswindow-zoom]]), "reduced on
//! the top to leave room for the menu bar"
//! ([[source: apple-nswindowdelegate-windowwillusestandardframe]]); baud
//! gives no standard frame of its own and sets no resize increments, so
//! the zoom is the screen's. `sdk_gates` (`baud-app/src/sdk_gates.rs`)
//! fails when either is added, which would make it something else.
//!
//! **Fill is AppKit's own, and private.** No header names a fill action.
//! AppKit's is `_zoomFill:`, and all three of the programs above call it,
//! each only after `respondsToSelector:` says the window has it; so does
//! `fill` here, with `nil` for the sender as Chromium and Firefox pass, and
//! with `performZoom:` when the window does not have it. It is the one
//! call of it in baud, and `sdk_gates` holds it to that one guarded site.
//!
//! **A word baud has no action for does nothing**, as Chromium leaves it
//! ("unrecognized value => do nothing"): it is a choice baud cannot carry
//! out, and neither zooming nor minimising is it. So does no value at all,
//! with not even AppKit's registered one there. Each is said once on stderr,
//! naming the word, so a double-click that does nothing is never silent.
//!
//! **Only where it is the view hit.** A click on the title bar's own views
//! reaches this view as well: a double-click sent to the build's name, as
//! AppKit sends a click to the view it hit, is passed on from the title bar
//! to the content under it (`baud-app/tests/titlebar_mark.rs`). The
//! double-click there is AppKit's own, its recognizer's, so this view acts
//! only when the window's frame view hit-tests the point to it, and the
//! setting's action is never done twice.
//!
//! Only a point above the window's content layout rect, the part of the
//! window the title bar covers ([[source: sdk27-nswindow-h]] 314), is the
//! title bar's: a double-click anywhere else the container is hit is passed
//! on as before. The terminal's own view never passes a click to it, and in
//! full screen it covers the container's whole height.
#![allow(unsafe_code)]

use std::sync::Mutex;

use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::AnyObject;
use macos_liquid_glass::objc2::{MainThreadOnly, define_class, msg_send, sel};
use macos_liquid_glass::objc2_app_kit::{NSEvent, NSView, NSWindow};
use macos_liquid_glass::objc2_foundation::{
    MainThreadMarker, NSObjectProtocol, NSPoint, NSRect, NSString, NSUserDefaults,
};

/// The user's defaults key the setting is read under.
pub const SETTING: &str = "AppleActionOnDoubleClick";

/// **A word the setting holds that baud has no action for**, as stored.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Word(String);

impl Word {
    /// The word as the defaults hold it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// **What a double-click on the title bar does**, one arm for each value
/// the setting can hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DoubleClick {
    /// "Maximize", the pane's Zoom: `performZoom:`.
    Zoom,
    /// "Fill": AppKit's `_zoomFill:` where the window has it, else
    /// `performZoom:` (`fill`).
    Fill,
    /// "Minimize": `performMiniaturize:`.
    Minimize,
    /// "None", the pane's No Action: nothing.
    Nothing,
    /// No value anywhere the defaults search, AppKit's registered one
    /// included: nothing, said once.
    Unset,
    /// Any other word: nothing, said once, naming it.
    Unknown(Word),
}

/// The values said on stderr, so each is said once a process.
static SAID: Mutex<Vec<DoubleClick>> = Mutex::new(Vec::new());

/// **The values said so far**, in the order they were said: each value
/// that gave a double-click nothing to do, once.
pub fn said() -> Vec<DoubleClick> {
    SAID.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

impl DoubleClick {
    /// The action for the setting's value, as read.
    pub fn of(setting: Option<&str>) -> DoubleClick {
        match setting {
            Some("Maximize") => DoubleClick::Zoom,
            Some("Fill") => DoubleClick::Fill,
            Some("Minimize") => DoubleClick::Minimize,
            Some("None") => DoubleClick::Nothing,
            Some(word) => DoubleClick::Unknown(Word(word.to_owned())),
            None => DoubleClick::Unset,
        }
    }

    /// The action the shared defaults give now: the user's own value, or
    /// else the one AppKit registered.
    pub fn read() -> DoubleClick {
        let value = NSUserDefaults::standardUserDefaults()
            .stringForKey(&NSString::from_str(SETTING))
            .map(|s| s.to_string());
        DoubleClick::of(value.as_deref())
    }

    /// Done on `window`; whether there was anything to do.
    fn on(&self, window: &NSWindow) -> bool {
        match self {
            DoubleClick::Zoom => window.performZoom(None),
            DoubleClick::Fill => fill(window),
            DoubleClick::Minimize => window.performMiniaturize(None),
            DoubleClick::Nothing => return false,
            DoubleClick::Unset | DoubleClick::Unknown(_) => {
                self.say();
                return false;
            }
        }
        true
    }

    /// This value said on stderr, the first time it gives nothing to do.
    fn say(&self) {
        let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
        if said.contains(self) {
            return;
        }
        match self {
            DoubleClick::Unknown(word) => eprintln!(
                "baud: the title bar's double-click setting {SETTING} is {:?}, which baud has \
                 no action for; a double-click on the title bar does nothing",
                word.as_str()
            ),
            _ => eprintln!(
                "baud: the title bar's double-click setting {SETTING} has no value, not even \
                 the one AppKit registers; a double-click on the title bar does nothing"
            ),
        }
        said.push(self.clone());
    }
}

/// **Fill, as AppKit does it**: its private `_zoomFill:`, only when the
/// window answers `respondsToSelector:` for it, as Chromium, Firefox and
/// iTerm2 each guard it (the module's sources), with nil for the sender as
/// the first two pass; `performZoom:` when it does not. The one call of the
/// private selector in baud (`sdk_gates`).
fn fill(window: &NSWindow) {
    if window.respondsToSelector(sel!(_zoomFill:)) {
        // SAFETY: the window answers for `_zoomFill:`, which takes one
        // sender object, nil here, and returns nothing.
        let () = unsafe { msg_send![window, _zoomFill: None::<&AnyObject>] };
    } else {
        window.performZoom(None);
    }
}

/// **Whether `at`, in a window's points, is in its title bar**: at or above
/// the top of its content layout rect `layout`, in the same points.
pub fn in_title_bar(at: NSPoint, layout: NSRect) -> bool {
    at.y >= layout.origin.y + layout.size.height
}

define_class!(
    /// The terminal's container: a plain view but for a double-click on the
    /// title bar ([`Container::new`]).
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    pub struct Container;

    impl Container {
        /// A double-click in the title bar does the setting's action; any
        /// other click, and a double-click the setting gives nothing to do,
        /// goes where a click on a plain view goes.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let done = crate::unwind::guard(
                crate::unwind::SiteName("the title bar's double-click"),
                false,
                || self.double_click(event),
            );
            if !done {
                // SAFETY: `NSResponder`'s own, with the event it was sent.
                let () = unsafe { msg_send![super(self), mouseDown: event] };
            }
        }
    }

    unsafe impl NSObjectProtocol for Container {}
);

impl Container {
    /// **A container `frame` in size**, as the window's body takes it.
    pub fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Container> {
        let this = Container::alloc(mtm).set_ivars(());
        // SAFETY: `NSView`'s designated initialiser, with a frame.
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// **Whether this view is the one a click at `at`, in the window's
    /// points, is hit-tested to**: not when one of the title bar's own views
    /// is there, though the click on it reaches this view by the title bar
    /// passing it on. The frame view is the content view's superview and
    /// has none, so it takes the point in the window's own.
    fn is_hit(&self, window: &NSWindow, at: NSPoint) -> bool {
        // SAFETY: `superview` is a plain getter; the window holds the views.
        let frame = window.contentView().and_then(|c| unsafe { c.superview() });
        frame
            .and_then(|f| f.hitTest(at))
            .is_some_and(|v| core::ptr::eq(&*v, &**self))
    }

    /// The setting's action, if `event` is a double-click in the title bar
    /// of the window this view is in; whether it did one.
    fn double_click(&self, event: &NSEvent) -> bool {
        if event.clickCount() != 2 {
            return false;
        }
        let Some(window) = self.window() else {
            return false;
        };
        let at = event.locationInWindow();
        if !in_title_bar(at, window.contentLayoutRect()) || !self.is_hit(&window, at) {
            return false;
        }
        DoubleClick::read().on(&window)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macos_liquid_glass::objc2_foundation::NSSize;

    /// **Each word of the setting is its own action**: "Maximize" a zoom,
    /// "Fill" a fill, "Minimize" to the Dock, "None" nothing, no value at
    /// all its own arm and not a zoom, and any other word kept as stored.
    /// Named breaks: no value read as "Maximize", and the fifth check
    /// fails; any other word zoomed, and the sixth does; "Fill" a zoom,
    /// and the second does.
    #[test]
    fn each_word_of_the_setting_is_its_action() {
        assert_eq!(DoubleClick::of(Some("Maximize")), DoubleClick::Zoom);
        assert_eq!(DoubleClick::of(Some("Fill")), DoubleClick::Fill);
        assert_eq!(DoubleClick::of(Some("Minimize")), DoubleClick::Minimize);
        assert_eq!(DoubleClick::of(Some("None")), DoubleClick::Nothing);
        assert_eq!(DoubleClick::of(None), DoubleClick::Unset);
        assert_eq!(
            DoubleClick::of(Some("Tile")),
            DoubleClick::Unknown(Word("Tile".to_owned()))
        );
        assert_eq!(
            DoubleClick::of(Some("maximize")),
            DoubleClick::Unknown(Word("maximize".to_owned())),
            "the words are matched as stored"
        );
    }

    /// **A value with nothing to do is said once**: twice through, it is
    /// in [`said`] once, and a second word is said too. Named break: say it
    /// every time, and the first count is two.
    #[test]
    fn a_value_with_nothing_to_do_is_said_once() {
        let first = DoubleClick::of(Some("a word said once"));
        let second = DoubleClick::of(Some("a second word"));
        first.say();
        first.say();
        second.say();
        let said = said();
        assert_eq!(said.iter().filter(|v| **v == first).count(), 1);
        assert_eq!(said.iter().filter(|v| **v == second).count(), 1);
    }

    /// **The title bar is what lies above the content layout rect**: a
    /// 600-point window whose rect stops 32 points short of its top has its
    /// title bar from 568 up, and 567.9 is the content's. Named break: test
    /// against the rect's origin, and 300 is the title bar's.
    #[test]
    fn the_title_bar_is_above_the_content_layout_rect() {
        let layout = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(960.0, 568.0));
        assert!(in_title_bar(NSPoint::new(480.0, 584.0), layout));
        assert!(in_title_bar(NSPoint::new(0.0, 568.0), layout));
        assert!(!in_title_bar(NSPoint::new(480.0, 567.9), layout));
        assert!(!in_title_bar(NSPoint::new(480.0, 300.0), layout));
    }
}
