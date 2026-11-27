//! **The frame handoff between the ingest thread and the frame thread**:
//! three slots, each owned by one side at a time. The ingest thread, through
//! its [`IngestEnd`], fills the back slot and publishes it, which swaps it
//! with the shared slot; the frame thread, through its [`FrameEnd`], takes
//! the shared slot when one was published after its last take, which swaps
//! it with the front slot. Neither side waits for the other.
//!
//! **One fill for each slot taken**: [`IngestEnd::taken`] says whether the
//! slot last published has been taken, so a filler that publishes only
//! then fills at most once for each slot the frame thread takes, however
//! far its input outruns the frames. [`FrameEnd::want`] calls the wake
//! the handoff was made with, so a filler waiting on its input learns that
//! a frame is due.
//!
//! **A publish replaces a slot not yet taken**, so a slot holds the whole
//! state as it was filled, never a change from the slot before it: the
//! frame thread may skip any number of publishes.
//!
//! **The one `unsafe`** is the slot's deref in [`Filling`] and [`Ready`],
//! through `slot` and `slot_mut`, and the `Sync` on the shared state it
//! rests on. The back index, the
//! front index and the shared index are always three different slots: only
//! [`Filling::publish`] and [`FrameEnd::take`] change them, each by one
//! atomic swap of its own index with the shared one, and each end is
//! neither `Clone` nor `Sync`, so it is reached through one `&mut` at a
//! time.

use std::cell::Cell;
use std::marker::PhantomData;

// The shared state's types: loom's under `--cfg loom`, so `tests/loom.rs`
// sees every access to a slot, every swap of `shared` and every end's
// drop; std's in every other build.
#[cfg(loom)]
use loom::{
    cell::UnsafeCell,
    sync::Arc,
    sync::atomic::{AtomicU8, Ordering},
};
#[cfg(not(loom))]
use std::{
    cell::UnsafeCell,
    sync::Arc,
    sync::atomic::{AtomicU8, Ordering},
};

/// **A slot, to read.**
///
/// # Safety
///
/// The caller's end holds the slot (the module's rule), and no `&mut` to
/// it is live for the reference's life.
#[cfg(not(loom))]
unsafe fn slot<S>(cell: &UnsafeCell<S>) -> &S {
    // SAFETY: the caller's.
    unsafe { &*cell.get() }
}

/// **A slot, to write.**
///
/// # Safety
///
/// The caller's end holds the slot (the module's rule), and no other
/// reference to it is live for the reference's life.
#[cfg(not(loom))]
#[allow(clippy::mut_from_ref)]
unsafe fn slot_mut<S>(cell: &UnsafeCell<S>) -> &mut S {
    // SAFETY: the caller's.
    unsafe { &mut *cell.get() }
}

/// **A slot, to read**, the read recorded where loom checks it.
///
/// # Safety
///
/// As the std build's.
#[cfg(loom)]
unsafe fn slot<S>(cell: &UnsafeCell<S>) -> &S {
    // SAFETY: the caller's.
    cell.with(|p| unsafe { &*p })
}

/// **A slot, to write**, the write recorded where loom checks it.
///
/// # Safety
///
/// As the std build's.
#[cfg(loom)]
#[allow(clippy::mut_from_ref)]
unsafe fn slot_mut<S>(cell: &UnsafeCell<S>) -> &mut S {
    // SAFETY: the caller's.
    cell.with_mut(|p| unsafe { &mut *p })
}

/// The shared slot's index, in the low two bits.
const INDEX: u8 = 0b011;
/// Set by a publish, cleared by the take that follows it.
const FRESH: u8 = 0b100;

struct Handoff<S> {
    slots: [UnsafeCell<S>; 3],
    shared: AtomicU8,
    /// Called by each [`FrameEnd::want`]: a frame is due.
    wake: Box<dyn Fn() + Send + Sync>,
}

// SAFETY: each slot is reached by exactly one thread at a time, by the index
// exclusivity the module states, and the guards that reach it are not
// `Sync`, so a slot moves between threads and is shared by them only as an
// `&S`, which crosses a thread only when `S` is `Sync`: `S: Send` is the
// bound that needs. A slot crosses threads
// by the AcqRel swap of `shared`, so what one side wrote is seen by the
// other. `shared` and `wake` are `Sync` themselves.
unsafe impl<S: Send> Sync for Handoff<S> {}

/// **The ingest thread's end: the back slot it fills.** One end, never two: it is
/// not `Clone`, so it cannot be handed to a second thread.
///
/// ```compile_fail,E0599
/// let (ingest, _frame) = baud_pipeline::handoff::handoff(|| 0u8, || ());
/// let twice = ingest.clone();
/// ```
pub struct IngestEnd<S> {
    h: Arc<Handoff<S>>,
    back: u8,
    one_thread: OneThread,
}

/// **The frame thread's end: the front slot it draws from.** Not `Clone`.
///
/// ```compile_fail,E0599
/// let (_ingest, frame) = baud_pipeline::handoff::handoff(|| 0u8, || ());
/// let twice = frame.clone();
/// ```
pub struct FrameEnd<S> {
    h: Arc<Handoff<S>>,
    front: u8,
    one_thread: OneThread,
}

/// **What makes an end, and so each guard, `Send` and not `Sync`**: an end
/// moves to its thread whole, and a guard holding it by `&mut` cannot be
/// shared between two threads, which would reach one slot from both.
type OneThread = PhantomData<Cell<()>>;

/// Each end crosses to its own thread whole: `Send` for any `Send` slot.
const _: () = {
    const fn send<T: Send>() {}
    const fn ends<S: Send>() {
        send::<IngestEnd<S>>();
        send::<FrameEnd<S>>();
    }
    ends::<[u64; 4]>();
};

/// **A new handoff, its three slots made by `make`**: back 0, shared 1,
/// front 2, nothing published. `wake` is called by each
/// [`FrameEnd::want`], to tell the ingest thread a frame is due.
pub fn handoff<S>(
    make: impl Fn() -> S,
    wake: impl Fn() + Send + Sync + 'static,
) -> (IngestEnd<S>, FrameEnd<S>) {
    let h = Arc::new(Handoff {
        slots: [
            UnsafeCell::new(make()),
            UnsafeCell::new(make()),
            UnsafeCell::new(make()),
        ],
        shared: AtomicU8::new(1),
        wake: Box::new(wake),
    });
    (
        IngestEnd {
            h: Arc::clone(&h),
            back: 0,
            one_thread: PhantomData,
        },
        FrameEnd {
            h,
            front: 2,
            one_thread: PhantomData,
        },
    )
}

impl<S> IngestEnd<S> {
    /// **Whether the slot last published has been taken**, so a fill now
    /// is drawn rather than replaced: true before the first publish.
    pub fn taken(&self) -> bool {
        self.h.shared.load(Ordering::Acquire) & FRESH == 0
    }

    /// The back slot, to fill.
    pub fn filling(&mut self) -> Filling<'_, S> {
        Filling { end: self }
    }
}

/// **The back slot while the ingest thread fills it**: published by value, so it
/// cannot be reached after.
///
/// ```compile_fail,E0382
/// let (mut ingest, _frame) = baud_pipeline::handoff::handoff(|| 0u8, || ());
/// let mut s = ingest.filling();
/// *s = 1;
/// s.publish();
/// *s = 2;
/// ```
///
/// Reached from one thread at a time: a guard shared between two threads
/// does not compile, whatever the slot.
///
/// ```compile_fail,E0277
/// let (mut ingest, _frame) =
///     baud_pipeline::handoff::handoff(|| std::cell::Cell::new(0u8), || ());
/// let s = ingest.filling();
/// std::thread::scope(|t| {
///     t.spawn(|| s.set(1));
///     s.set(2);
/// });
/// ```
pub struct Filling<'a, S> {
    end: &'a mut IngestEnd<S>,
}

impl<S> Filling<'_, S> {
    /// **The slot handed to the frame thread**: it becomes the shared slot, marked
    /// fresh, and the shared slot it replaces, taken or not, is the new
    /// back.
    pub fn publish(self) {
        let old = self
            .end
            .h
            .shared
            .swap(self.end.back | FRESH, Ordering::AcqRel);
        self.end.back = old & INDEX;
    }
}

impl<S> std::ops::Deref for Filling<'_, S> {
    type Target = S;
    fn deref(&self) -> &S {
        // SAFETY: the back slot is the ingest end's alone (the module's rule), and
        // this guard holds its end mutably.
        unsafe { slot(&self.end.h.slots[usize::from(self.end.back)]) }
    }
}

impl<S> std::ops::DerefMut for Filling<'_, S> {
    fn deref_mut(&mut self) -> &mut S {
        // SAFETY: as `deref`.
        unsafe { slot_mut(&self.end.h.slots[usize::from(self.end.back)]) }
    }
}

impl<S> FrameEnd<S> {
    /// **A frame is due**: calls the wake the handoff was made with, so a
    /// filler waiting on its input publishes what it holds.
    pub fn want(&self) {
        (self.h.wake)();
    }

    /// **The slot published after the last take**, now the front; none
    /// when nothing was.
    pub fn take(&mut self) -> Option<Ready<'_, S>> {
        if self.h.shared.load(Ordering::Acquire) & FRESH == 0 {
            return None;
        }
        let old = self.h.shared.swap(self.front, Ordering::AcqRel);
        self.front = old & INDEX;
        Some(Ready { end: self })
    }
}

/// **The front slot while the frame thread draws from it**, the frame
/// end's alone, so it may be written as well as read. One at a time: a
/// second take while one is held does not compile.
///
/// ```compile_fail,E0499
/// let (_ingest, mut frame) = baud_pipeline::handoff::handoff(|| 0u8, || ());
/// let a = frame.take();
/// let b = frame.take();
/// drop(a);
/// ```
///
/// Reached from one thread at a time, as [`Filling`] is.
///
/// ```compile_fail,E0277
/// let (_ingest, mut frame) =
///     baud_pipeline::handoff::handoff(|| std::cell::Cell::new(0u8), || ());
/// let r = frame.take().unwrap();
/// std::thread::scope(|t| {
///     t.spawn(|| r.set(1));
///     r.set(2);
/// });
/// ```
pub struct Ready<'a, S> {
    end: &'a mut FrameEnd<S>,
}

impl<S> std::ops::Deref for Ready<'_, S> {
    type Target = S;
    fn deref(&self) -> &S {
        // SAFETY: the front slot is the frame end's alone (the module's rule), and
        // this guard holds its end mutably.
        unsafe { slot(&self.end.h.slots[usize::from(self.end.front)]) }
    }
}

impl<S> std::ops::DerefMut for Ready<'_, S> {
    fn deref_mut(&mut self) -> &mut S {
        // SAFETY: as `deref`.
        unsafe { slot_mut(&self.end.h.slots[usize::from(self.end.front)]) }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A slot written with its sequence number first and last: a take that
    /// sees two different numbers saw a slot half written.
    #[derive(Default)]
    struct Seq {
        first: u64,
        body: [u64; 32],
        last: u64,
    }

    /// **Every slot taken was published whole, and in order**: one thread
    /// publishes 10,000 slots numbered 1.. (200 under Miri), the other takes
    /// as it can; each take reads one number throughout and a number above
    /// the last it read, and the last slot published is taken.
    #[test]
    fn every_slot_taken_was_published_whole_and_in_order() {
        const N: u64 = if cfg!(miri) { 200 } else { 10_000 };
        let (mut ingest, mut frame) = handoff(Seq::default, || ());
        let writer = std::thread::spawn(move || {
            for n in 1..=N {
                let mut s = ingest.filling();
                s.first = n;
                s.body = [n; 32];
                s.last = n;
                s.publish();
            }
        });
        let mut seen = 0;
        while seen < N {
            match frame.take() {
                Some(s) => {
                    assert_eq!(s.first, s.last, "a slot half written");
                    assert!(s.body.iter().all(|&b| b == s.first), "a slot half written");
                    assert!(s.first > seen, "{} after {seen}", s.first);
                    seen = s.first;
                }
                None => std::hint::spin_loop(),
            }
        }
        writer.join().unwrap();
    }

    /// **A fill waits for the take of the last**: published, it is not
    /// taken until the frame end takes it, and then it is; nothing unpublished is
    /// taken, and a want calls the wake once.
    #[test]
    fn a_fill_waits_for_the_take_of_the_last() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let (mut ingest, mut frame) = handoff(
            || 0u8,
            move || {
                w.fetch_add(1, Ordering::Relaxed);
            },
        );
        assert!(frame.take().is_none(), "nothing published");
        assert!(ingest.taken(), "nothing to wait for");
        let mut s = ingest.filling();
        *s = 7;
        s.publish();
        assert!(!ingest.taken(), "published, not taken");
        assert_eq!(frame.take().map(|r| *r), Some(7));
        assert!(ingest.taken());
        assert!(frame.take().is_none(), "taken once");
        frame.want();
        assert_eq!(woken.load(Ordering::Relaxed), 1);
    }
}
