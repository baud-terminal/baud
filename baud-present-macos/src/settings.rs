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
//!
//! **Observed on the default centre with no queue**, so the block runs on
//! the thread that posts, and does nothing there but set the mark: no
//! AppKit call is made from it, whichever thread that is.
//!
//! `unsafe` is allowed here for the two calls the bindings mark so: the
//! observer's registration, whose block is `Send` and `Sync` because it
//! only stores to an atomic, and the extern static naming the
//! notification.
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
}

impl Marks {
    fn of(&self, s: Setting) -> &AtomicBool {
        match s {
            Setting::Scroller => &self.scroller,
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
        let observers = vec![observe(&center, scroller, &marks, Setting::Scroller)];
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
        let watch = Watch::start();
        let marks = Arc::clone(&watch.marks);
        drop(watch);
        post(scroller_name());
        assert!(!marks.scroller.load(Ordering::Acquire));
    }
}
