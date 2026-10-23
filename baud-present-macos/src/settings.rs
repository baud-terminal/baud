//! **The system settings the window follows the moment they change**: a
//! setting AppKit announces with a notification is heard here, and the
//! app's next display-link tick acts on it. Nothing is polled and nothing
//! waits: the observer marks the setting heard, and the tick that takes the
//! mark reads the new value and lays the window out again.
//!
//! * **"Show scroll bars"**: `preferredScrollerStyle` "may therefore change
//!   over time, and applications should be prepared to adapt their user
//!   interfaces to the new scroller style if needed", and "If you need to be
//!   notified of changes to the preferred scroller style, you can register
//!   to receive `preferredScrollerStyleDidChangeNotification` notifications"
//!   (`[[source: apple-nsscroller-preferredscrollerstyle]]`). The
//!   notification is "Posted if the preferred scroller style changes"
//!   (`[[source: apple-nsscroller-preferredscrollerstyledidchange]]`), and
//!   the header names its object private and says to "Consult NSScroller's
//!   +preferredScrollerStyle method when this notification is received, or
//!   thereafter" (`[[source: macos-sdk-nsscroller-h]]` 92-94): so the mark
//!   carries no value, and the tick asks.
//! * **Dim Flashing Lights**: "Check the value of the Dim Flashing Lights
//!   setting at any time by using" `MADimFlashingLightsEnabled()`
//!   (`[[source: apple-mediaaccessibility-flashing-lights]]`,
//!   `[[source: apple-madimflashinglightsenabled]]`), and
//!   `kMADimFlashingLightsChangedNotification` "posts when a person changes
//!   the flashing lights setting"
//!   (`[[source: apple-kmadimflashinglightschangednotification]]`). The
//!   header calls it a "CFNotification sent to local notification center"
//!   (`[[source: macos-sdk-mavideoaccommodations-h]]` 27) and Apple's sample
//!   observes it on `NotificationCenter.default`
//!   (`[[source: apple-responding-to-flashing-lights-setting]]`): one centre,
//!   as `a_local_centre_post_reaches_the_default_centre` shows by posting
//!   on the first and hearing it on the second. Both exist from macOS
//!   13.3, under the app's floor of 26, so both are linked.
//!
//! **Observed on the default centre with no queue**, so the block runs on
//! the thread that posts, and does nothing there but set the mark: no
//! AppKit call is made from it, whichever thread that is.
//!
//! `unsafe` is allowed here for the calls the bindings mark so -- the
//! observer's registration, whose block is `Send` and `Sync` because it
//! only stores to an atomic, and the extern statics naming the
//! notifications -- and for MediaAccessibility's two symbols, which no
//! binding among this crate's dependencies declares.
#![allow(unsafe_code)]

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use macos_liquid_glass::objc2::rc::Retained;
use macos_liquid_glass::objc2::runtime::{NSObjectProtocol, ProtocolObject};
use macos_liquid_glass::objc2_app_kit::NSPreferredScrollerStyleDidChangeNotification;
use macos_liquid_glass::objc2_foundation::{
    NSNotification, NSNotificationCenter, NSNotificationName,
};

/// A setting the window follows as it changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Setting {
    /// "Show scroll bars": `NSScroller`'s preferred style.
    Scroller,
    /// Accessibility's Dim Flashing Lights.
    DimFlashingLights,
}

/// What the system asks of flashing on screen: Dim Flashing Lights on is
/// [`Flashing::Dimmed`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flashing {
    Full,
    Dimmed,
}

#[link(name = "MediaAccessibility", kind = "framework")]
unsafe extern "C" {
    /// `bool MADimFlashingLightsEnabled(void)`, `MAVideoAccommodations.h`
    /// 23.
    fn MADimFlashingLightsEnabled() -> bool;
    /// `const CFStringRef kMADimFlashingLightsChangedNotification`, 30: a
    /// `CFString`, which is the `NSString` a notification name is.
    static kMADimFlashingLightsChangedNotification: &'static NSNotificationName;
}

/// Whether Dim Flashing Lights is on, asked now: read at launch and when
/// its notification arrives, as Apple's sample reads it.
pub fn flashing() -> Flashing {
    // SAFETY: a function of no arguments the framework exports from macOS
    // 13.3, below the app's floor.
    match unsafe { MADimFlashingLightsEnabled() } {
        true => Flashing::Dimmed,
        false => Flashing::Full,
    }
}

/// The name Dim Flashing Lights' change is posted under.
fn flashing_name() -> &'static NSNotificationName {
    // SAFETY: a framework constant, a valid string for the process's life.
    unsafe { kMADimFlashingLightsChangedNotification }
}

/// Whether a setting's notification arrived since it was last taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Heard {
    Changed,
    Unchanged,
}

/// The marks the observers set and the tick takes, one a setting.
#[derive(Default)]
struct Marks {
    scroller: AtomicBool,
    flashing: AtomicBool,
}

impl Marks {
    fn of(&self, s: Setting) -> &AtomicBool {
        match s {
            Setting::Scroller => &self.scroller,
            Setting::DimFlashingLights => &self.flashing,
        }
    }
}

/// **The observers, for as long as the window lives**: one on the default
/// notification centre for each [`Setting`], removed when this is dropped.
pub struct Watch {
    marks: Arc<Marks>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
}

impl Watch {
    /// Observe every setting from now on.
    pub fn start() -> Watch {
        let marks = Arc::new(Marks::default());
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: an AppKit constant, a valid `NSString` for the process's
        // life.
        let scroller = unsafe { NSPreferredScrollerStyleDidChangeNotification };
        let observers = vec![
            observe(&center, scroller, &marks, Setting::Scroller),
            observe(&center, flashing_name(), &marks, Setting::DimFlashingLights),
        ];
        Watch { marks, observers }
    }

    /// Whether `s`'s notification arrived since the last take, and the
    /// mark cleared: one notification, or several before a tick, is one
    /// change to act on.
    pub fn take(&self, s: Setting) -> Heard {
        match self.marks.of(s).swap(false, Ordering::AcqRel) {
            true => Heard::Changed,
            false => Heard::Unchanged,
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for o in &self.observers {
            // SAFETY: each is a token `addObserverForName:...` returned on
            // this centre, removed once.
            unsafe { center.removeObserver(o.as_ref()) };
        }
    }
}

/// An observer of `name` on `center` that marks `s` heard.
fn observe(
    center: &NSNotificationCenter,
    name: &NSNotificationName,
    marks: &Arc<Marks>,
    s: Setting,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let marks = Arc::clone(marks);
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        marks.of(s).store(true, Ordering::Release);
    });
    // SAFETY: no object filter and no queue, so the block runs on the
    // posting thread; it captures an `Arc` of atomics, which is `Send` and
    // `Sync`, so running there is sound.
    unsafe { center.addObserverForName_object_queue_usingBlock(Some(name), None, None, &block) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use macos_liquid_glass::objc2_foundation::NSString;
    use std::sync::Mutex;

    /// The tests share the process's default centre, so a post one makes
    /// could land in another's watch under a threaded runner: one at a
    /// time.
    static CENTRE: Mutex<()> = Mutex::new(());

    fn alone() -> std::sync::MutexGuard<'static, ()> {
        CENTRE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Post `name` on the default centre, as AppKit does when the setting
    /// moves.
    fn post(name: &NSNotificationName) {
        // SAFETY: no object, which the header says is private anyway.
        unsafe { NSNotificationCenter::defaultCenter().postNotificationName_object(name, None) };
    }

    fn scroller_name() -> &'static NSNotificationName {
        // SAFETY: as in `Watch::start`.
        unsafe { NSPreferredScrollerStyleDidChangeNotification }
    }

    /// **The scroller's notification is heard, once, until it comes
    /// again**: posted on the default centre by its AppKit name, the mark
    /// is set, a take clears it, and two posts before a take are one
    /// change. Another name marks nothing -- the control that the observer
    /// filters by name.
    #[test]
    fn the_scroller_notification_is_heard_once_a_tick() {
        let _one = alone();
        let watch = Watch::start();
        assert_eq!(watch.take(Setting::Scroller), Heard::Unchanged);
        post(&NSString::from_str("BaudNotTheScrollerStyle"));
        assert_eq!(
            watch.take(Setting::Scroller),
            Heard::Unchanged,
            "the control: another notification marks nothing"
        );
        post(scroller_name());
        assert_eq!(watch.take(Setting::Scroller), Heard::Changed);
        assert_eq!(watch.take(Setting::Scroller), Heard::Unchanged);
        post(scroller_name());
        post(scroller_name());
        assert_eq!(watch.take(Setting::Scroller), Heard::Changed);
        assert_eq!(watch.take(Setting::Scroller), Heard::Unchanged);
    }

    /// **A dropped watch hears nothing more**: its observer is removed, so a
    /// post after the drop reaches no block, and a watch started later
    /// starts unmarked.
    #[test]
    fn a_dropped_watch_stops_observing() {
        let _one = alone();
        let watch = Watch::start();
        let marks = Arc::clone(&watch.marks);
        drop(watch);
        post(scroller_name());
        post(flashing_name());
        assert!(!marks.scroller.load(Ordering::Acquire));
        assert!(!marks.flashing.load(Ordering::Acquire));
    }

    /// **Dim Flashing Lights' notification is heard apart from the
    /// scroller's**: each marks its own setting and not the other.
    #[test]
    fn the_flashing_lights_notification_marks_its_own_setting() {
        let _one = alone();
        let watch = Watch::start();
        post(flashing_name());
        assert_eq!(watch.take(Setting::DimFlashingLights), Heard::Changed);
        assert_eq!(
            watch.take(Setting::Scroller),
            Heard::Unchanged,
            "the control: the scroller's mark is its own"
        );
        post(scroller_name());
        assert_eq!(watch.take(Setting::DimFlashingLights), Heard::Unchanged);
        assert_eq!(watch.take(Setting::Scroller), Heard::Changed);
    }

    /// **The header's local centre is the default centre the observer is
    /// on**: the name posted through `CFNotificationCenterGetLocalCenter`,
    /// as the header says MediaAccessibility posts it, reaches the observer
    /// Apple's sample registers on `NotificationCenter.default`. Posted on
    /// a centre of this test's own instead -- the control -- it does not,
    /// so the observer hears one centre only.
    #[test]
    fn a_local_centre_post_reaches_the_default_centre() {
        use objc2_core_foundation::{CFNotificationCenter, CFString};
        let _one = alone();
        let watch = Watch::start();
        // SAFETY: no object; the centre is this test's, which nothing
        // observes.
        unsafe { NSNotificationCenter::new().postNotificationName_object(flashing_name(), None) };
        assert_eq!(
            watch.take(Setting::DimFlashingLights),
            Heard::Unchanged,
            "the control: another centre's post is not heard"
        );
        // SAFETY: the name is a `CFString` toll-free bridged as an
        // `NSString`, so the pointer is one to the other type.
        let name = unsafe { &*(flashing_name() as *const NSNotificationName).cast::<CFString>() };
        let local = CFNotificationCenter::local_center().expect("the local centre");
        // SAFETY: as above.
        unsafe { local.post_notification(Some(name), core::ptr::null(), None, true) };
        assert_eq!(watch.take(Setting::DimFlashingLights), Heard::Changed);
    }

    /// **The setting reads**: a link check -- the framework loads and the
    /// call returns, whichever value this machine is set to; the value is
    /// the owner's setting, so nothing is asserted of it.
    #[test]
    fn the_flashing_lights_setting_reads() {
        let _ = flashing();
    }
}
