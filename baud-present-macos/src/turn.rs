//! **A main-queue turn of its own: the one place an effect may run.**
//!
//! macos-liquid-glass holds the view's responder borrow across every
//! responder method -- `tick`, keys, the pointer, `wake` -- so an effect
//! that spins a nested run loop from inside one (a reveal in Finder, a
//! modal panel) re-enters the view, which was the reveal crash. Every
//! function in this crate that asks something of the desktop takes a
//! [`Turn`], and a `Turn` has no public constructor: the only way to one is
//! a main-queue turn that [`soon`] or [`later`] schedules, which runs after
//! the callback that asked has returned, with no responder borrow held. So
//! the compiler refuses an effect made from a frame, a pump or a responder
//! method, none of which can name a `Turn`.
//!
//! - [`soon`] is the effect queue's door: it posts the one function
//!   [`install`] set, with no closure and nothing boxed, so asking costs
//!   no allocation (`os_effect.rs` in baud-app schedules it once per batch).
//! - [`later`] is the door for a one-off closure, boxed once per call: a
//!   window operation, the print panel, a window opened from a menu.
//! - [`after`] is [`later`] once a time has passed: a notification's
//!   expiry.
//!
//! Under the test-only `effect-probe` feature, [`crate::probe::Inline`]
//! makes all three doors run inline -- in the callback that asked, or parked for
//! the next frame -- which is how the event rig's controls put an effect
//! back where the crash had it.
#![allow(unsafe_code)]

use core::cell::Cell;
use core::ffi::c_void;
use core::marker::PhantomData;

use macos_liquid_glass::objc2::MainThreadMarker;

use crate::unwind::{SiteName, guard};

/// **Proof of a main-queue turn of its own.** Zero-sized, and made only
/// here, by a turn [`soon`], [`later`] or [`after`] scheduled, and only on
/// the main thread: it carries the main thread's marker, checked when the
/// turn is made, so every desktop effect takes one and reads the marker
/// from it ([`Turn::main_thread`]).
///
/// **Neither `Send` nor `Sync`**: the marker keeps it on the main thread, so a thread started inside a turn -- a scoped one
/// included, which could borrow it -- cannot take it there and make an
/// effect off the main queue.
///
/// ```compile_fail,E0277
/// fn sent<T: Send>() {}
/// sent::<baud_present_macos::turn::Turn<'static>>();
/// ```
///
/// ```compile_fail,E0277
/// fn shared<T: Sync>() {}
/// shared::<baud_present_macos::turn::Turn<'static>>();
/// ```
pub struct Turn<'t>(PhantomData<&'t mut ()>, MainThreadMarker);

impl Turn<'_> {
    /// The main thread's marker, for an AppKit call that asks for one: a
    /// turn is made only on the main thread, checked then.
    pub fn main_thread(&self) -> MainThreadMarker {
        self.1
    }
}

/// **`f` with a turn, if this is the main thread**: the check every turn
/// is made through. Off it, which only a test reaches, `f` does not run.
fn on_main(f: impl FnOnce(&Turn<'_>)) {
    if let Some(mtm) = MainThreadMarker::new() {
        f(&Turn(PhantomData, mtm));
    }
}

thread_local! {
    /// What [`soon`]'s turn runs, set once by [`install`].
    static RUN: Cell<Option<fn(&Turn<'_>)>> = const { Cell::new(None) };
}

/// The function [`soon`]'s turns run: the effect queue's runner. Set once,
/// at startup; a second call replaces it.
pub fn install(run: fn(&Turn<'_>)) {
    RUN.with(|r| r.set(Some(run)));
}

/// A turn of [`install`]'s function, posted to the main queue now and run
/// after the current callback returns. One plain function and a null
/// context: nothing is boxed or allocated.
pub fn soon() {
    #[cfg(feature = "effect-probe")]
    match crate::probe::inline() {
        crate::probe::Inline::No | crate::probe::Inline::AfterInCallback => {}
        crate::probe::Inline::InCallback => {
            turn(core::ptr::null_mut());
            return;
        }
        crate::probe::Inline::InFrame => {
            crate::probe::park(Box::new(|| turn(core::ptr::null_mut())));
            return;
        }
    }
    // SAFETY: `dispatch_async_f` on the main queue, with a null context,
    // which `turn` never reads, and a function that lives for the process.
    unsafe {
        dispatch2::DispatchQueue::main().exec_async_f(core::ptr::null_mut(), turn);
    }
}

/// [`soon`]'s turn: the installed function, with a `Turn`, behind a guard,
/// so a panic in it is stopped here and named, never let into
/// libdispatch, which would abort.
extern "C" fn turn(_context: *mut c_void) {
    guard(SOON, (), || {
        if let Some(run) = RUN.with(Cell::get) {
            on_main(run);
        }
    });
}

/// `f` on a main-queue turn of its own, after the current callback
/// returns; boxed once, so for a user's one-off action, never per frame.
pub fn later(site: SiteName, f: impl FnOnce(&Turn<'_>) + Send + 'static) {
    let run = move || guard(site, (), || on_main(f));
    #[cfg(feature = "effect-probe")]
    match crate::probe::inline() {
        crate::probe::Inline::No | crate::probe::Inline::AfterInCallback => {}
        crate::probe::Inline::InCallback => {
            run();
            return;
        }
        crate::probe::Inline::InFrame => {
            crate::probe::park(Box::new(run));
            return;
        }
    }
    dispatch2::DispatchQueue::main().exec_async(run);
}

/// **`f` on a main-queue turn of its own once `when` has come**: [`later`],
/// delayed. Boxed once, so for a program's or a user's one-off action,
/// never per frame. Under the probe's inline modes the delay is not
/// honoured: `f` runs in the callback that asked, or in the next frame,
/// as [`later`]'s would; under `AfterInCallback` this door alone runs in
/// the callback, the expiry's control.
pub fn after(
    when: dispatch2::DispatchTime,
    site: SiteName,
    f: impl FnOnce(&Turn<'_>) + Send + 'static,
) {
    let run = move || guard(site, (), || on_main(f));
    #[cfg(feature = "effect-probe")]
    match crate::probe::inline() {
        crate::probe::Inline::No => {}
        crate::probe::Inline::InCallback | crate::probe::Inline::AfterInCallback => {
            run();
            return;
        }
        crate::probe::Inline::InFrame => {
            crate::probe::park(Box::new(run));
            return;
        }
    }
    let _ = dispatch2::DispatchQueue::main().after(when, run);
}

/// The guard's name for [`soon`]'s turns.
const SOON: SiteName = SiteName("the effect queue's turn");

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Turn` is the size of nothing: the proof costs no bytes to pass.
    #[test]
    fn a_turn_is_zero_sized() {
        assert_eq!(core::mem::size_of::<Turn<'_>>(), 0);
    }

    /// **Off the main thread no turn is made**: `turn`, called as
    /// libdispatch calls it but on a test's thread, runs nothing, installed
    /// or not, and neither does a closure handed to the check every turn
    /// is made through. On the main thread every turn runs, which every
    /// green row of the event rig shows. Named break: make the turn without
    /// the check, and the installed function runs here.
    #[test]
    fn off_the_main_thread_no_turn_is_made() {
        thread_local! {
            static RAN: Cell<u32> = const { Cell::new(0) };
        }
        fn counted(_: &Turn<'_>) {
            RAN.with(|r| r.set(r.get() + 1));
        }
        assert!(MainThreadMarker::new().is_none(), "a test's own thread");
        turn(core::ptr::null_mut());
        install(counted);
        turn(core::ptr::null_mut());
        on_main(counted);
        assert_eq!(RAN.with(Cell::get), 0, "nothing run off the main thread");
    }
}
