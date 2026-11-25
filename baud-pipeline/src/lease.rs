//! **What the frame thread holds of the ingest thread's, until the GPU is
//! done with it**: a [`Lease`], owned rather than borrowed, because the GPU
//! reads what was lent until its completion, which outlives any borrow. A
//! lease moves into that completion, on whatever thread runs it, and its
//! drop hands the item back: a [`Returned`] goes onto a bounded ring from
//! the frame side to the ingest side, and the wake the [`Lender`] was made
//! with is called.
//!
//! **The ingest side gets an item back only through a `Returned`**, and a
//! `Returned` is made only by a lease's drop, so nothing lent is reused
//! while a lease of it is held.
//!
//! **The ring never fills.** A [`Lender`] of capacity `N` lends at most `N`
//! items at once, counting a lease as out from [`Lender::lend`] until its
//! `Returned` is drained by [`Lender::returned`], and the ring holds `N`.
//! Every `Returned` on the ring is of a lease still counted as out, so a
//! drop's push always finds room and never waits. A lend past `N` is
//! refused and hands the item back.
//!
//! Made outside a lease's drop, a `Returned` does not compile:
//!
//! ```compile_fail,E0599
//! let back = baud_pipeline::lease::Returned::<u8>::new();
//! ```
//!
//! ```compile_fail,E0451
//! let back = baud_pipeline::lease::Returned { item: 0u8 };
//! ```
//!
//! An item is freed only from its `Returned`:
//!
//! ```compile_fail,E0308
//! let mut lender = baud_pipeline::lease::lender::<u8, 1>(|| ());
//! let item = lender.free(7u8);
//! ```
//!
//! A lease is not cloned:
//!
//! ```compile_fail,E0599
//! let mut lender = baud_pipeline::lease::lender::<u8, 1>(|| ());
//! let lease = lender.lend(7).unwrap();
//! let twice = lease.clone();
//! ```
//!
//! What a lease holds is not written through it:
//!
//! ```compile_fail,E0614
//! let mut lender = baud_pipeline::lease::lender::<u8, 1>(|| ());
//! let mut lease = lender.lend(7).unwrap();
//! let item: &mut u8 = &mut *lease;
//! ```
//!
//! ```compile_fail,E0594
//! let mut lender = baud_pipeline::lease::lender::<u8, 1>(|| ());
//! let lease = lender.lend(7).unwrap();
//! *lease.get() = 8;
//! ```
//!
//! and not copied out of it whole:
//!
//! ```compile_fail,E0599
//! let mut lender = baud_pipeline::lease::lender::<Vec<u8>, 1>(|| ());
//! let lease = lender.lend(vec![7]).unwrap();
//! let copy = lease.to_vec();
//! ```

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

/// **The ring's sending half and the wake**, shared by every lease of one
/// lender.
struct Back<T> {
    ring: SyncSender<Returned<T>>,
    wake: Box<dyn Fn() + Send + Sync>,
}

/// **An item lent to the frame side, owned until it is dropped.** Not
/// `Clone`, with no `Deref` and no `&mut` access: it is read through
/// [`Lease::get`] and given back by its drop.
pub struct Lease<T: Send + 'static> {
    item: Option<T>,
    back: Arc<Back<T>>,
}

/// **An item given back by its lease's drop**: the one thing
/// [`Lender::free`] takes. Made only by [`Lease`]'s drop.
#[must_use = "a Returned dropped unfreed drops its item without freeing it"]
pub struct Returned<T> {
    item: T,
}

/// **The ingest side's end**: lends at most `N` items at once and drains
/// what their leases give back. Not `Clone` and not `Sync`.
pub struct Lender<T: Send + 'static, const N: usize> {
    back: Arc<Back<T>>,
    ring: Receiver<Returned<T>>,
    /// Leases lent and not yet drained as a `Returned`, at most `N`.
    out: usize,
}

/// A lease moves into a completion on another thread: `Send` and
/// `'static` for any `Send + 'static` item.
const _: () = {
    const fn sent<T: Send + 'static>() {}
    sent::<Lease<[u64; 4]>>();
};

/// **A new lender of capacity `N`**, with a ring of `N`: `wake` is called
/// once for each lease dropped while the lender lives.
pub fn lender<T: Send + 'static, const N: usize>(
    wake: impl Fn() + Send + Sync + 'static,
) -> Lender<T, N> {
    const { assert!(N > 0, "a lender lends at least one item") };
    let (ring, drained) = mpsc::sync_channel(N);
    Lender {
        back: Arc::new(Back {
            ring,
            wake: Box::new(wake),
        }),
        ring: drained,
        out: 0,
    }
}

impl<T: Send + 'static, const N: usize> Lender<T, N> {
    /// **`item` lent**, or handed back when `N` leases are out.
    pub fn lend(&mut self, item: T) -> Result<Lease<T>, T> {
        if self.out == N {
            return Err(item);
        }
        self.out += 1;
        Ok(Lease {
            item: Some(item),
            back: Arc::clone(&self.back),
        })
    }

    /// **The next item given back**, if a lease has been dropped after
    /// the last.
    pub fn returned(&mut self) -> Option<Returned<T>> {
        let back = self.ring.try_recv().ok()?;
        self.out -= 1;
        Some(back)
    }

    /// **The item a lease gave back**, free to reuse.
    pub fn free(&mut self, back: Returned<T>) -> T {
        back.item
    }
}

impl<T: Send + 'static> Lease<T> {
    /// The item lent, to read.
    pub fn get(&self) -> &T {
        match &self.item {
            Some(item) => item,
            None => unreachable!("a lease's item is taken only by its drop"),
        }
    }
}

impl<T: Send + 'static> Drop for Lease<T> {
    /// **The item given back**: pushed onto the ring as a `Returned`, then
    /// the wake called. The push never waits: the ring holds every lease
    /// its lender has out. With the lender gone the item is dropped here
    /// and nothing wakes.
    fn drop(&mut self) {
        let Some(item) = self.item.take() else {
            return;
        };
        match self.back.ring.try_send(Returned { item }) {
            Ok(()) => (self.back.wake)(),
            Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                unreachable!("the ring holds every lease out, and this one is")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A lender of `N` whose wake counts its calls.
    fn counted<T: Send + 'static, const N: usize>() -> (Lender<T, N>, Arc<AtomicUsize>) {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let lender = lender(move || {
            w.fetch_add(1, Ordering::Relaxed);
        });
        (lender, woken)
    }

    /// **A lease dropped on another thread arrives as a `Returned`**, and
    /// frees the item lent; nothing more arrives after it.
    ///
    /// Named break: the drop not pushing onto the ring -- nothing arrives.
    #[test]
    fn a_drop_on_another_thread_arrives_as_a_returned() {
        let (mut lender, _) = counted::<Vec<u8>, 2>();
        let lease = lender.lend(vec![7, 8]).unwrap();
        assert_eq!(lease.get(), &[7, 8]);
        std::thread::spawn(move || drop(lease)).join().unwrap();
        let back = lender.returned().expect("the lease's item, given back");
        assert_eq!(lender.free(back), [7, 8]);
        assert!(lender.returned().is_none(), "one drop, one Returned");
    }

    /// **The wake fires once for each drop.**
    ///
    /// Named break: the wake called on lend as well as on drop -- the count
    /// reads two after the first drop.
    #[test]
    fn the_wake_fires_once_per_drop() {
        let (mut lender, woken) = counted::<u8, 3>();
        let leases = [
            lender.lend(1).unwrap(),
            lender.lend(2).unwrap(),
            lender.lend(3).unwrap(),
        ];
        assert_eq!(woken.load(Ordering::Relaxed), 0, "nothing dropped");
        for (n, lease) in (1..).zip(leases) {
            drop(lease);
            assert_eq!(woken.load(Ordering::Relaxed), n);
        }
    }

    /// **With a lease held, nothing is returned and nothing wakes**; once
    /// it is dropped, its item is.
    ///
    /// Named break: `lend` pushing a `Returned` as it lends -- one arrives
    /// while the lease is held.
    #[test]
    fn with_a_lease_held_nothing_is_returned() {
        let (mut lender, woken) = counted::<u8, 1>();
        let lease = lender.lend(7).unwrap();
        assert!(lender.returned().is_none(), "the lease is held");
        assert_eq!(woken.load(Ordering::Relaxed), 0);
        drop(lease);
        let back = lender.returned().expect("dropped, so given back");
        assert_eq!(lender.free(back), 7);
    }

    /// **A lend past the capacity is refused and hands the item back**,
    /// and lends again once a `Returned` is drained -- not once the lease
    /// is dropped, because its `Returned` still holds room on the ring.
    ///
    /// Named break: `out` counted down at the drop rather than at the
    /// drain -- the second lend succeeds before the drain, and a ring of
    /// one would then hold two.
    #[test]
    fn a_lend_past_the_capacity_is_refused() {
        let (mut lender, _) = counted::<u8, 1>();
        let lease = lender.lend(1).unwrap();
        assert_eq!(lender.lend(2).err(), Some(2), "one out of one");
        drop(lease);
        assert_eq!(lender.lend(2).err(), Some(2), "dropped, not yet drained");
        let back = lender.returned().unwrap();
        assert_eq!(lender.free(back), 1);
        assert_eq!(lender.lend(2).map(|l| *l.get()).ok(), Some(2));
    }

    /// **A lease dropped after its lender drops its item and wakes
    /// nothing**: there is no one to give it back to.
    ///
    /// Named break: the drop waking whatever the push's result -- the wake
    /// count reads one.
    #[test]
    fn a_drop_after_the_lender_is_gone_drops_the_item() {
        let (mut lender, woken) = counted::<Arc<()>, 1>();
        let item = Arc::new(());
        let lease = lender.lend(Arc::clone(&item)).unwrap();
        drop(lender);
        drop(lease);
        assert_eq!(Arc::strong_count(&item), 1, "the lent item dropped");
        assert_eq!(woken.load(Ordering::Relaxed), 0);
    }

    /// **Every lease dropped across threads comes back once**: 64 leases,
    /// out at once, dropped by 8 threads 8 each, for 200 rounds; each round
    /// drains exactly the 64 items lent, each once, and the wake fires 64
    /// times. A full ring would panic in a drop and fail the join.
    ///
    /// Named break: the ring made one smaller than the capacity -- the last
    /// drop of a round finds it full.
    #[test]
    fn every_lease_dropped_across_threads_comes_back_once() {
        const ROUNDS: usize = 200;
        let (mut lender, woken) = counted::<u16, 64>();
        for round in 1..=ROUNDS {
            let mut leases: Vec<Lease<u16>> = (0..64).map(|n| lender.lend(n).unwrap()).collect();
            assert!(lender.lend(64).is_err(), "64 out of 64");
            let threads: Vec<_> = (0..8)
                .map(|_| {
                    let mine: Vec<Lease<u16>> = leases.drain(..8).collect();
                    std::thread::spawn(move || drop(mine))
                })
                .collect();
            for t in threads {
                t.join().unwrap();
            }
            let mut seen = [false; 64];
            while let Some(back) = lender.returned() {
                let n = usize::from(lender.free(back));
                assert!(!seen[n], "{n} given back twice");
                seen[n] = true;
            }
            assert!(seen.iter().all(|&s| s), "every item given back");
            assert_eq!(woken.load(Ordering::Relaxed), 64 * round);
        }
    }
}
