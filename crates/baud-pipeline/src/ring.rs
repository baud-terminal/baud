//! **The read ring between the reading thread and the parsing thread**: `K`
//! slots of `S` bytes, zeroed once when the ring is made, each owned by one
//! side at a time. The reader, through its [`ReadEnd`], takes a free slot as
//! a [`Filling`], reads into it and publishes `n` bytes of it with the
//! read's [`HostTime`]; the parser, through its [`ParseEnd`], takes the
//! slots published, in the order they were published, each as a [`Ready`]
//! of exactly those `n` bytes. A `Ready`'s drop hands its slot back to the
//! reader and calls the wake the ring was made with.
//!
//! **A full ring gives the reader no slot**: [`ReadEnd::try_fill`] is `None`
//! while all `K` slots are published or held, and each refusal is counted
//! in [`ReadEnd::ring_full`]. That is the only backpressure: a reader with
//! no slot stops reading until the wake says one came back.
//!
//! **No read happens here.** The caller reads into the `&mut [u8]` a
//! `Filling` lends. Every slot was zeroed when the ring was made, so the
//! bytes lent are always initialized, and a slot is never zeroed again.
//!
//! **No `unsafe`**: a slot is an owned buffer, and it moves between the two
//! ends by value over two channels, one each way, each holding at most `K`
//! slots because there are only `K`.
//!
//! A slot is not reached after it is handed on: a `Filling` is published by
//! value,
//!
//! ```compile_fail,E0382
//! use baud_domain::{Count, HostTime, ReadLen};
//! use baud_pipeline::{ReadRing, SlotLen};
//! let ring = ReadRing::new(Count::of(2), SlotLen::new(8).unwrap(), || ()).unwrap();
//! let (mut read, _parse) = ring.split();
//! let mut f = read.try_fill().unwrap();
//! f[0] = 1;
//! let _ = f.fill(ReadLen::ONE, HostTime::ZERO);
//! f[0] = 2;
//! ```
//!
//! and a `Ready`'s bytes do not outlive it:
//!
//! ```compile_fail,E0505
//! use baud_domain::{Count, HostTime, ReadLen};
//! use baud_pipeline::{ReadRing, SlotLen};
//! let ring = ReadRing::new(Count::of(2), SlotLen::new(8).unwrap(), || ()).unwrap();
//! let (mut read, mut parse) = ring.split();
//! let _ = read.try_fill().unwrap().fill(ReadLen::ONE, HostTime::ZERO);
//! let mut r = parse.take().unwrap();
//! let bytes: &mut [u8] = &mut r;
//! drop(r);
//! bytes[0] = 0x7f;
//! ```
//!
//! The parse end fills nothing,
//!
//! ```compile_fail,E0599
//! use baud_domain::Count;
//! use baud_pipeline::{ReadRing, SlotLen};
//! let ring = ReadRing::new(Count::of(2), SlotLen::new(8).unwrap(), || ()).unwrap();
//! let (_read, mut parse) = ring.split();
//! let f = parse.try_fill();
//! ```
//!
//! and the read end takes nothing:
//!
//! ```compile_fail,E0599
//! use baud_domain::Count;
//! use baud_pipeline::{ReadRing, SlotLen};
//! let ring = ReadRing::new(Count::of(2), SlotLen::new(8).unwrap(), || ()).unwrap();
//! let (mut read, _parse) = ring.split();
//! let r = read.take();
//! ```
//!
//! The legal path, each slot reached through its own end and left before it
//! is handed on:
//!
//! ```
//! use baud_domain::{Count, HostTime, ReadLen};
//! use baud_pipeline::{ReadRing, SlotLen};
//! let ring = ReadRing::new(Count::of(2), SlotLen::new(8).unwrap(), || ()).unwrap();
//! let (mut read, mut parse) = ring.split();
//! let mut f = read.try_fill().unwrap();
//! f[0] = 0x9b;
//! assert!(f.fill(ReadLen::ONE, HostTime::of_nanos(5)).is_ok());
//! let mut r = parse.take().unwrap();
//! let bytes: &mut [u8] = &mut r;
//! bytes[0] &= 0x7f;
//! assert_eq!(&r[..], &[0x1b]);
//! assert_eq!(r.stamp(), HostTime::of_nanos(5));
//! drop(r);
//! ```

use std::cell::Cell;
use std::marker::PhantomData;

use baud_domain::{Count, HostTime, ReadLen, RingFull, SlotLen};

// The channels: loom's under `--cfg loom`, so `tests/loom.rs` sees every
// send and receive; std's bounded ones in every other build. loom's is
// unbounded, which is the same channel here: only `K` slots exist, so
// neither direction ever holds more than `K`.
#[cfg(loom)]
use loom::sync::mpsc::{Receiver, Sender, channel};
#[cfg(not(loom))]
use std::sync::mpsc::{Receiver, SyncSender as Sender, sync_channel};

/// A channel that holds `k` slots without blocking a send.
fn slots_channel<T>(k: Count) -> (Sender<T>, Receiver<T>) {
    #[cfg(loom)]
    {
        let _ = k;
        channel()
    }
    #[cfg(not(loom))]
    {
        sync_channel(k.index())
    }
}

/// One slot's memory: owned, so it is in one place at a time.
type Slot = Box<[u8]>;

/// **A slot published**: the slot, the bytes of it the read placed, and
/// the read's stamp.
struct Published {
    slot: Slot,
    n: ReadLen,
    stamp: HostTime,
}

/// **What makes an end `Send` and not `Sync`**: an end moves to its thread
/// whole, and is reached there through one `&mut` at a time.
type OneThread = PhantomData<Cell<()>>;

/// **A new ring, before its split**: its `K` slots made and zeroed, all
/// free.
pub struct ReadRing {
    read: ReadEnd,
    parse: ParseEnd,
}

/// **The reading thread's end**: lends free slots to fill and publishes
/// them. Not `Clone` and not `Sync`.
///
/// ```compile_fail,E0599
/// use baud_domain::Count;
/// let ring = baud_pipeline::ReadRing::new(
///     Count::of(1), baud_pipeline::SlotLen::new(1).unwrap(), || ()).unwrap();
/// let (read, _parse) = ring.split();
/// let twice = read.clone();
/// ```
pub struct ReadEnd {
    free: Receiver<Slot>,
    published: Sender<Published>,
    /// A slot lent and dropped unfilled, lent again before any other.
    spare: Option<Slot>,
    len: SlotLen,
    ring_full: RingFull,
    one_thread: OneThread,
}

/// **The parsing thread's end**: takes the slots published, in order. Not
/// `Clone` and not `Sync`.
///
/// ```compile_fail,E0599
/// use baud_domain::Count;
/// let ring = baud_pipeline::ReadRing::new(
///     Count::of(1), baud_pipeline::SlotLen::new(1).unwrap(), || ()).unwrap();
/// let (_read, parse) = ring.split();
/// let twice = parse.clone();
/// ```
pub struct ParseEnd {
    published: Receiver<Published>,
    free: Sender<Slot>,
    /// Called by each [`Ready`]'s drop: a slot came back.
    wake: Box<dyn Fn() + Send + Sync>,
    one_thread: OneThread,
}

/// Each end crosses to its own thread whole.
const _: () = {
    const fn send<T: Send>() {}
    send::<ReadEnd>();
    send::<ParseEnd>();
};

// Under loom, a slot left on a channel when a model ends is reported as a
// leak, so each end takes what is still on its receiving channel as it
// goes; std drops them with the receiver.
#[cfg(loom)]
impl Drop for ReadEnd {
    fn drop(&mut self) {
        while self.free.try_recv().is_ok() {}
    }
}

#[cfg(loom)]
impl Drop for ParseEnd {
    fn drop(&mut self) {
        while self.published.try_recv().is_ok() {}
    }
}

impl ReadRing {
    /// **A ring of `slots` slots of `slot_len` bytes each**, every slot
    /// zeroed here and never again; `None` for no slots. `wake` is called
    /// by each [`Ready`]'s drop, on the parsing thread, to tell the reader
    /// a slot is free.
    pub fn new(
        slots: Count,
        slot_len: SlotLen,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Option<ReadRing> {
        let k = slots.index();
        if k == 0 {
            return None;
        }
        let (free_tx, free_rx) = slots_channel(slots);
        let (pub_tx, pub_rx) = slots_channel(slots);
        for _ in 0..k {
            let slot: Slot = vec![0; slot_len.get()].into_boxed_slice();
            if free_tx.send(slot).is_err() {
                unreachable!("both halves are held here");
            }
        }
        Some(ReadRing {
            read: ReadEnd {
                free: free_rx,
                published: pub_tx,
                spare: None,
                len: slot_len,
                ring_full: RingFull::default(),
                one_thread: PhantomData,
            },
            parse: ParseEnd {
                published: pub_rx,
                free: free_tx,
                wake: Box::new(wake),
                one_thread: PhantomData,
            },
        })
    }

    /// The reading thread's end and the parsing thread's.
    pub fn split(self) -> (ReadEnd, ParseEnd) {
        (self.read, self.parse)
    }
}

impl ReadEnd {
    /// **A free slot, to fill**; `None` when all are published or held,
    /// counted in [`ReadEnd::ring_full`].
    pub fn try_fill(&mut self) -> Option<Filling<'_>> {
        let slot = match self.spare.take() {
            Some(slot) => slot,
            None => match self.free.try_recv() {
                Ok(slot) => slot,
                Err(_) => {
                    self.ring_full.add_one();
                    return None;
                }
            },
        };
        Some(Filling {
            slot: Some(slot),
            end: self,
        })
    }

    /// The count of fills refused for want of a free slot.
    pub fn ring_full(&self) -> RingFull {
        self.ring_full
    }
}

/// **A free slot while the reader fills it**: the whole slot, lent as
/// `&mut [u8]`, and published by value. Dropped unfilled, the slot stays
/// the reader's and is lent again first.
pub struct Filling<'a> {
    end: &'a mut ReadEnd,
    /// The slot, taken only by `fill` or the drop.
    slot: Option<Slot>,
}

impl<'a> Filling<'a> {
    /// **`n` bytes of the slot published** with the read's `stamp`, for the
    /// parser to take after every slot published before it. A fill past
    /// the slot's length is refused and hands the `Filling` back, nothing
    /// published. With the parse end gone the slot is dropped.
    pub fn fill(mut self, n: ReadLen, stamp: HostTime) -> Result<(), Filling<'a>> {
        if !self.end.len.holds(n) {
            return Err(self);
        }
        let Some(slot) = self.slot.take() else {
            unreachable!("a Filling's slot is taken only by fill or its drop");
        };
        let _ = self.end.published.send(Published { slot, n, stamp });
        Ok(())
    }
}

impl std::ops::Deref for Filling<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.slot {
            Some(slot) => slot,
            None => unreachable!("a Filling's slot is taken only by fill or its drop"),
        }
    }
}

impl std::ops::DerefMut for Filling<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match &mut self.slot {
            Some(slot) => slot,
            None => unreachable!("a Filling's slot is taken only by fill or its drop"),
        }
    }
}

impl Drop for Filling<'_> {
    /// **An unfilled slot kept**, lent again by the next `try_fill`.
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            self.end.spare = Some(slot);
        }
    }
}

impl ParseEnd {
    /// **The slot published first of those not yet taken**; none when
    /// every slot published has been. One at a time: the `Ready` holds
    /// this end.
    pub fn take(&mut self) -> Option<Ready<'_>> {
        let p = self.published.try_recv().ok()?;
        Some(Ready {
            end: self,
            p: Some(p),
        })
    }
}

/// **A published slot while the parser holds it**: exactly the `n` bytes
/// published, as `&mut [u8]`, and the read's stamp. Its drop hands the
/// slot back to the reader and calls the ring's wake.
pub struct Ready<'a> {
    end: &'a mut ParseEnd,
    /// The slot, taken only by the drop.
    p: Option<Published>,
}

impl Ready<'_> {
    /// The stamp of the read that filled this slot.
    pub fn stamp(&self) -> HostTime {
        match &self.p {
            Some(p) => p.stamp,
            None => unreachable!("a Ready's slot is taken only by its drop"),
        }
    }
}

impl std::ops::Deref for Ready<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match &self.p {
            Some(p) => &p.slot[..p.n.index()],
            None => unreachable!("a Ready's slot is taken only by its drop"),
        }
    }
}

impl std::ops::DerefMut for Ready<'_> {
    fn deref_mut(&mut self) -> &mut [u8] {
        match &mut self.p {
            Some(p) => &mut p.slot[..p.n.index()],
            None => unreachable!("a Ready's slot is taken only by its drop"),
        }
    }
}

impl Drop for Ready<'_> {
    /// **The slot handed back**, then the wake called. With the read end
    /// gone the slot is dropped and nothing wakes.
    fn drop(&mut self) {
        let Some(p) = self.p.take() else {
            return;
        };
        if self.end.free.send(p.slot).is_ok() {
            (self.end.wake)();
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A ring of `k` slots of `len` bytes, split, whose wake counts its
    /// calls.
    fn ring(k: Count, len: SlotLen) -> (ReadEnd, ParseEnd, Arc<AtomicUsize>) {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let ring = ReadRing::new(k, len, move || {
            w.fetch_add(1, Ordering::Relaxed);
        })
        .expect("a ring of at least one slot");
        let (read, parse) = ring.split();
        (read, parse, woken)
    }

    /// Copies `bytes` into a slot and publishes them, stamped `t`.
    fn put(read: &mut ReadEnd, bytes: &[u8], t: HostTime) {
        let mut f = read.try_fill().expect("a free slot");
        f[..bytes.len()].copy_from_slice(bytes);
        assert!(
            f.fill(ReadLen::of(bytes), t).is_ok(),
            "the bytes fit the slot"
        );
    }

    /// **Slots are taken in the order they were filled**, each with exactly
    /// the bytes and the stamp it was published with.
    ///
    /// Named break: the stamp not carried with the slot -- the first take
    /// reads `HostTime::ZERO`.
    #[test]
    fn slots_are_taken_in_the_order_filled() {
        let (mut read, mut parse, _) = ring(Count::of(4), SlotLen::new(8).unwrap());
        assert!(parse.take().is_none(), "nothing filled");
        put(&mut read, b"ab", HostTime::of_nanos(1));
        put(&mut read, b"cde", HostTime::of_nanos(2));
        put(&mut read, b"f", HostTime::of_nanos(3));
        for (bytes, t) in [(&b"ab"[..], 1), (b"cde", 2), (b"f", 3)] {
            let r = parse.take().expect("a filled slot");
            assert_eq!(&r[..], bytes);
            assert_eq!(r.stamp(), HostTime::of_nanos(t));
        }
        assert!(parse.take().is_none(), "each slot taken once");
    }

    /// **A full ring gives no slot to fill, and counts each refusal
    /// once**; a slot handed back fills again.
    ///
    /// Named break: the refusal not counted -- `ring_full` reads zero.
    #[test]
    fn a_full_ring_refuses_and_counts_each_refusal() {
        let (mut read, mut parse, woken) = ring(Count::of(2), SlotLen::new(4).unwrap());
        put(&mut read, b"a", HostTime::of_nanos(1));
        put(&mut read, b"b", HostTime::of_nanos(2));
        assert_eq!(read.ring_full().get(), 0, "nothing refused yet");
        assert!(read.try_fill().is_none(), "two of two out");
        assert!(read.try_fill().is_none(), "still two of two out");
        assert_eq!(read.ring_full().get(), 2, "one count per refusal");
        drop(parse.take().expect("the first slot"));
        assert_eq!(woken.load(Ordering::Relaxed), 1, "one slot handed back");
        assert!(read.try_fill().is_some(), "a slot came back");
        assert_eq!(read.ring_full().get(), 2, "a fill given is not counted");
    }

    /// **A fill past the slot's length is refused**, handing the slot
    /// back unpublished; a fill of exactly the length is published.
    ///
    /// Named break: `fill` not checking the length -- the 5-byte fill is
    /// published.
    #[test]
    fn a_fill_past_the_slot_is_refused() {
        let (mut read, mut parse, _) = ring(Count::of(1), SlotLen::new(4).unwrap());
        let f = read.try_fill().expect("a free slot");
        assert_eq!(f.len(), 4, "the whole slot lent");
        let f = f
            .fill(ReadLen::from_index(5), HostTime::of_nanos(1))
            .expect_err("5 bytes do not fit a slot of 4");
        assert!(parse.take().is_none(), "nothing published");
        assert!(
            f.fill(ReadLen::from_index(4), HostTime::of_nanos(2))
                .is_ok()
        );
        assert_eq!(parse.take().map(|r| r.len()), Some(4));
    }

    /// **A slot refilled at once after its `Ready` drops gives exactly the
    /// new bytes**, never the old ones past the new length.
    ///
    /// Named break: `Ready` lending the whole slot rather than its `n`
    /// bytes -- the second take reads "xyCDEF".
    #[test]
    fn a_refilled_slot_gives_only_the_new_bytes() {
        let (mut read, mut parse, _) = ring(Count::of(1), SlotLen::new(6).unwrap());
        put(&mut read, b"ABCDEF", HostTime::of_nanos(1));
        assert_eq!(&parse.take().expect("the first fill")[..], b"ABCDEF");
        put(&mut read, b"xy", HostTime::of_nanos(2));
        let refill = parse.take().map(|r| r.to_vec());
        assert_eq!(refill.as_deref(), Some(&b"xy"[..]));
    }

    /// **The slots are zeroed once, at construction**: each of the `k`
    /// slots is lent whole, `len` bytes, and zero on first use, and a
    /// slot lent again holds what was last written into it.
    ///
    /// Named break: a slot zeroed each time it is lent -- the second
    /// lend of the one slot reads zero.
    #[test]
    fn the_slots_are_zeroed_once_at_construction() {
        let ring = ReadRing::new(Count::of(3), SlotLen::new(16).unwrap(), || ()).unwrap();
        let (mut read, mut parse) = ring.split();
        for _ in 0..3 {
            let mut f = read.try_fill().expect("a free slot");
            assert_eq!(f.len(), 16);
            assert!(f.iter().all(|&b| b == 0), "zero on first use");
            f[..].fill(0xAA);
            assert!(f.fill(ReadLen::ONE, HostTime::ZERO).is_ok());
        }
        for _ in 0..3 {
            drop(parse.take().expect("a filled slot"));
        }
        for _ in 0..3 {
            let f = read.try_fill().expect("a slot handed back");
            assert!(f.iter().all(|&b| b == 0xAA), "not zeroed again");
            assert!(f.fill(ReadLen::ONE, HostTime::ZERO).is_ok());
        }
    }

    /// **A slot lent and not filled stays the reader's**: the next fill
    /// gets it, and nothing is published.
    ///
    /// Named break: an unfilled `Filling` dropping its slot -- a ring of
    /// one has nothing left to lend.
    #[test]
    fn a_slot_lent_and_not_filled_stays_the_readers() {
        let (mut read, mut parse, _) = ring(Count::of(1), SlotLen::new(4).unwrap());
        drop(read.try_fill().expect("a free slot"));
        assert!(parse.take().is_none(), "nothing published");
        assert!(read.try_fill().is_some(), "the unfilled slot lent again");
        put(&mut read, b"ok", HostTime::of_nanos(1));
        assert_eq!(&parse.take().expect("the fill")[..], b"ok");
        assert_eq!(read.ring_full().get(), 0);
    }

    /// **No ring of no slots, and no slot of no bytes.**
    #[test]
    fn no_ring_is_empty() {
        assert!(SlotLen::new(0).is_none());
        let len = SlotLen::new(1).unwrap();
        assert!(len.holds(ReadLen::ONE) && !len.holds(ReadLen::from_index(2)));
        assert!(ReadRing::new(Count::of(0), len, || ()).is_none());
    }

    /// **Every byte arrives once and in order across two threads**: the
    /// reader fills 10,000 slots (200 under Miri) of a ring of 4, each
    /// with its number in every byte it publishes, waiting when the ring
    /// is full; the parser takes each in turn and reads only its number.
    ///
    /// Named break: a slot handed back before the parser is done with it
    /// -- a take reads another slot's number.
    #[test]
    fn every_byte_arrives_once_and_in_order_across_threads() {
        const N: u32 = if cfg!(miri) { 200 } else { 10_000 };
        let (mut read, mut parse, _) = ring(Count::of(4), SlotLen::new(64).unwrap());
        let reader = std::thread::spawn(move || {
            for n in 1..=N {
                let len = usize::try_from(n % 64 + 1).unwrap();
                let mut f = loop {
                    match read.try_fill() {
                        Some(f) => break f,
                        None => std::thread::yield_now(),
                    }
                };
                f[..len].fill(u8::try_from(n % 251).unwrap());
                assert!(
                    f.fill(ReadLen::from_index(len), HostTime::of_nanos(n.into()))
                        .is_ok()
                );
            }
        });
        let mut seen = 0u32;
        while seen < N {
            match parse.take() {
                Some(r) => {
                    seen += 1;
                    assert_eq!(r.stamp(), HostTime::of_nanos(seen.into()), "out of order");
                    assert_eq!(r.len(), usize::try_from(seen % 64 + 1).unwrap());
                    let b = u8::try_from(seen % 251).unwrap();
                    assert!(r.iter().all(|&x| x == b), "slot {seen} overwritten");
                }
                None => std::thread::yield_now(),
            }
        }
        reader.join().unwrap();
        assert!(parse.take().is_none(), "nothing past the last");
    }
}
