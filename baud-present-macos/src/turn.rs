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
//!
//! Under the test-only `effect-probe` feature, [`crate::probe::Inline`]
//! makes both doors run inline -- in the callback that asked, or parked for
//! the next frame -- which is how the event rig's controls put an effect
//! back where the crash had it.
#![allow(unsafe_code)]

use core::cell::Cell;
use core::ffi::c_void;
use core::marker::PhantomData;

use crate::unwind::{SiteName, guard};

/// **Proof of a main-queue turn of its own.** Zero-sized, and made only
/// here, by a turn [`soon`] or [`later`] scheduled; every desktop effect
/// takes one.
pub struct Turn<'t>(PhantomData<&'t mut ()>);

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
        crate::probe::Inline::No => {}
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
            run(&Turn(PhantomData));
        }
    });
}

/// `f` on a main-queue turn of its own, after the current callback
/// returns; boxed once, so for a user's one-off action, never per frame.
pub fn later(site: SiteName, f: impl FnOnce(&Turn<'_>) + Send + 'static) {
    let run = move || guard(site, (), || f(&Turn(PhantomData)));
    #[cfg(feature = "effect-probe")]
    match crate::probe::inline() {
        crate::probe::Inline::No => {}
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

    /// **The installed function runs with a `Turn`, and only it**: `turn`,
    /// called as libdispatch calls it, runs what `install` set, once.
    #[test]
    fn a_turn_runs_what_was_installed() {
        thread_local! {
            static RAN: Cell<u32> = const { Cell::new(0) };
        }
        fn counted(_: &Turn<'_>) {
            RAN.with(|r| r.set(r.get() + 1));
        }
        turn(core::ptr::null_mut());
        assert_eq!(RAN.with(Cell::get), 0, "nothing installed, nothing run");
        install(counted);
        turn(core::ptr::null_mut());
        assert_eq!(RAN.with(Cell::get), 1);
    }
}
