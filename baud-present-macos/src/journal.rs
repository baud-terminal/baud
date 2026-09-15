//! What the present path did, counted -- so "the screen doubled" and "there
//! are freezes" become numbers rather than reports.
//!
//! A frame is committed on the main thread and lands on the GPU's completion
//! thread, which is where the count is taken, so every counter here is an
//! atomic and every write is a `fetch_add`. Nothing is published through
//! them, so `Relaxed` is the whole ordering argument: a reader wants the
//! values, and no memory is reached by way of them.
//!
//! **Counted on the landing thread, which under-counts and cannot
//! over-count.** The comparison reads the epoch from the GPU's thread, so a
//! bump that has happened on the main thread but has not yet been observed
//! reads as *current* and the frame is counted `shown`. That is a logical
//! race and no atomic ordering fixes it -- ordering makes a write visible in
//! a defined sequence, not promptly. It is the reason the eventual decision
//! belongs on the thread that owns the geometry. For an *instrument* it is
//! sound in the direction that matters: a non-zero `superseded` is proof the
//! bug fires, and a zero is not proof that it does not.

use baud_domain::{Fate, FrameGen, SwapEpoch, SwapGen};
use core::time::Duration;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// One frame's identity, as it crosses to the GPU and back.
///
/// `Copy` plain data: it rides in the completion closure's capture, so it
/// must not borrow anything that a resize could free underneath it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameTag {
    /// Which frame, for the journal to name.
    pub frame: FrameGen,
    /// Which surface set it was drawn into -- the discriminant.
    pub swap: SwapGen,
    /// Which set of the per-frame buffers it was built into, so the set
    /// is spoken for until this frame lands. `None` for a path that
    /// builds into nothing the caller owns -- the instance and CPU paths
    /// convert on the main thread and hold no set.
    pub set: Option<SetIx>,
}

/// Which of the per-frame buffer sets a frame is built into.
///
/// The buffers are the caller's -- the resolved styles and the overlay
/// mask, which live in the app -- so this names a set rather than holding
/// one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetIx(pub usize);

/// Whether a set has a frame in it that has not landed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetState {
    /// Nothing is reading it: safe to build into.
    Free,
    /// A committed frame has not landed yet, so the GPU may be reading it.
    Held,
}

/// Which sets frames that have not landed were committed with.
///
/// A frame is built into a set, commits with it, and frees it when it
/// lands. **Handing a new frame a set an unlanded frame holds is the write
/// race itself**: the CPU would write the buffer the GPU is reading. This
/// is the one place that decides, so the app and the tests that pin it
/// cannot drift apart -- the reason it lives here rather than beside the
/// buffers.
///
/// The ordering argument is the module's, in the safe direction. A set is
/// taken on the main thread and freed on the GPU's completion thread, so a
/// free that has happened and has not yet been observed reads as *held*
/// and the main thread skips a set that is already free. It cannot read a
/// held set as free, because the main thread is the one that held it.
pub struct Wheel {
    /// The frame holding each set, or zero. A committed frame is never
    /// [`FrameGen::FIRST`] -- the presenter advances before it tags -- so
    /// zero is free rather than a frame number.
    held: [AtomicU64; Wheel::SETS],
    next: AtomicUsize,
}

impl Default for Wheel {
    fn default() -> Wheel {
        Wheel {
            held: std::array::from_fn(|_| AtomicU64::new(0)),
            next: AtomicUsize::new(0),
        }
    }
}

impl Wheel {
    /// One set a frame that can be in flight, plus room. Three, which is
    /// what the surfaces and the instance buffer already ring at.
    pub const SETS: usize = 3;

    /// The set to build the next frame into: the next one round, or the
    /// first free one after it. `None` while every set is held, which
    /// means there is nowhere to build that a live frame is not reading
    /// and the caller must not draw.
    pub fn building(&self) -> Option<SetIx> {
        let at = self.next.load(Ordering::Relaxed) % Wheel::SETS;
        (0..Wheel::SETS)
            .map(|step| (at + step) % Wheel::SETS)
            .find(|&ix| self.held[ix].load(Ordering::Relaxed) == 0)
            .map(SetIx)
    }

    /// `frame` is in flight, built into `set`, so nothing may write that
    /// set until it lands.
    pub fn committed(&self, set: SetIx, frame: FrameGen) {
        if let Some(h) = self.held.get(set.0) {
            h.store(frame.value(), Ordering::Relaxed);
        }
        self.next
            .store((set.0 + 1) % Wheel::SETS, Ordering::Relaxed);
    }

    /// `frame` landed, so whatever set it held is free again.
    pub fn landed(&self, frame: FrameGen) {
        for h in &self.held {
            let _ = h.compare_exchange(frame.value(), 0, Ordering::Relaxed, Ordering::Relaxed);
        }
    }

    /// Whether a set is safe to build into.
    pub fn state(&self, set: SetIx) -> SetState {
        match self.held.get(set.0).map(|h| h.load(Ordering::Relaxed)) {
            Some(0) | None => SetState::Free,
            Some(_) => SetState::Held,
        }
    }
}

/// Whether the memory a frame reads that has **no ring** may be written
/// now: the terminal's grid, which the cell shader reads in place, and the
/// atlas sheets every GPU path samples.
///
/// There is one of each, so a mutation cannot move aside the way a ringed
/// buffer can. Ringing either is the wrong price -- the grid is megabytes
/// and the atlas is two textures, against an event a keypress or a drag
/// step produces -- so the *request* waits instead, for a completion that
/// is already on its way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reshape {
    /// Nothing is reading it. Reflow, rescale, do as you like.
    Now,
    /// A frame in flight is reading it.
    Wait,
}

/// How many frames reached the layer, and how many should not have.
#[derive(Default)]
pub struct Journal {
    shown: AtomicU64,
    superseded: AtomicU64,
    landed: AtomicU64,
    /// Frames committed and not yet landed. Every path counts here, not
    /// just the one that holds a buffer set: an instance-path frame holds
    /// no set and still samples the atlas.
    flying: AtomicU64,
    deferred: AtomicU64,
    /// The last completed frame's GPU nanoseconds.
    gpu_nanos: AtomicU64,
    /// Which per-frame buffer sets are spoken for. It lives here because
    /// this is the one structure both threads already hold: the main
    /// thread takes a set, and the completion that lands a frame frees it
    /// in the same call that counts it.
    wheel: Wheel,
}

/// The counts, taken together for a report.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Counts {
    /// Completions seen, whatever became of them.
    pub landed: u64,
    /// Frames whose surface set was still current when they landed.
    pub shown: u64,
    /// Frames drawn into a surface set that had been replaced. **This is
    /// the doubled image on a resize and the old font size on a zoom.**
    pub superseded: u64,
    /// Reshapes -- a reflow or a rescale -- that waited for a frame to
    /// land rather than writing memory it was reading.
    pub deferred: u64,
    /// The last completed frame's own start-to-end on the GPU, from the
    /// stamps Metal puts on the command buffer. Recorded where it exists:
    /// on the completion, by the handler that already runs there. Nothing
    /// times it, and no frame reports a zero it has not earned.
    pub gpu: Duration,
}

impl Journal {
    /// Records what a landed frame earned, and answers it so a caller can
    /// act on it. In the observing phase every caller shows the frame
    /// regardless, which is what makes the count a measurement of the bug
    /// rather than of the fix.
    /// A frame is in flight: it holds `set` if it was built into one, and
    /// it reads the unringed memory either way. The one place a commit is
    /// recorded, so the rule the app runs is the rule the tests pin.
    pub fn commit(&self, set: Option<SetIx>, frame: FrameGen) {
        if let Some(set) = set {
            self.wheel.committed(set, frame);
        }
        self.flying.fetch_add(1, Ordering::Relaxed);
    }

    /// Whether the unringed memory may be written now.
    ///
    /// **Asking is what a caller does immediately before reshaping**, so an
    /// answer of `Wait` is a deferral and is counted as one. That is a
    /// query with a side effect on purpose: it makes "a resize waited"
    /// a number in the frame report rather than a thing to believe.
    pub fn reshape(&self) -> Reshape {
        match self.flying.load(Ordering::Relaxed) {
            0 => Reshape::Now,
            _ => {
                self.deferred.fetch_add(1, Ordering::Relaxed);
                Reshape::Wait
            }
        }
    }

    pub fn land(&self, tag: FrameTag, epoch: &SwapEpoch, gpu: Duration) -> Fate {
        let fate = baud_domain::fate(tag.swap, epoch.current());
        self.gpu_nanos.store(
            u64::try_from(gpu.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        // Whatever became of it, the buffers it was built into are no
        // longer being read, and neither is the grid or the atlas.
        if tag.set.is_some() {
            self.wheel.landed(tag.frame);
        }
        self.flying
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
        self.landed.fetch_add(1, Ordering::Relaxed);
        match fate {
            Fate::Current => self.shown.fetch_add(1, Ordering::Relaxed),
            Fate::Superseded { .. } => self.superseded.fetch_add(1, Ordering::Relaxed),
        };
        fate
    }

    /// The sets, for a caller about to build a frame.
    pub fn wheel(&self) -> &Wheel {
        &self.wheel
    }

    /// What has happened so far.
    pub fn counts(&self) -> Counts {
        Counts {
            landed: self.landed.load(Ordering::Relaxed),
            shown: self.shown.load(Ordering::Relaxed),
            superseded: self.superseded.load(Ordering::Relaxed),
            deferred: self.deferred.load(Ordering::Relaxed),
            gpu: Duration::from_nanos(self.gpu_nanos.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameTag, Journal};
    use baud_domain::{Fate, FrameGen, SwapEpoch};
    use core::time::Duration;

    #[test]
    fn a_frame_that_outlives_its_surfaces_is_counted_superseded() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        let tag = FrameTag {
            frame: FrameGen::FIRST,
            swap: epoch.current(),
            set: None,
        };
        // Landing before anything moved: current, and shown.
        assert_eq!(j.land(tag, &epoch, Duration::ZERO), Fate::Current);
        assert_eq!(j.counts().shown, 1);
        assert_eq!(j.counts().superseded, 0);

        // A resize replaces the surface set while this frame is in flight.
        epoch.bump();
        assert!(matches!(
            j.land(tag, &epoch, Duration::ZERO),
            Fate::Superseded { .. }
        ));
        let c = j.counts();
        assert_eq!((c.landed, c.shown, c.superseded), (2, 1, 1));

        // The control that separates "the rule works" from "the rule
        // rejects everything": a frame drawn for the new set still counts
        // shown. Without this, a guard that discarded every frame -- a
        // black window -- would pass the assertion above.
        let fresh = FrameTag {
            frame: tag.frame.next(),
            swap: epoch.current(),
            set: None,
        };
        assert_eq!(j.land(fresh, &epoch, Duration::ZERO), Fate::Current);
        assert_eq!(j.counts().shown, 2);
    }
}

/// Whether a landed frame reaches the layer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    /// Set it as the layer's contents.
    Now,
    /// Drop it. The surfaces it drew into are not the ones the layer shows
    /// from any more, so showing it would put an older image over a newer.
    Skip,
}

/// The one place that decides, so the completion path and the tests that
/// pin it cannot drift apart.
///
/// **A frame is shown exactly while the surfaces it drew into are the ones
/// the layer shows from.** A superseded frame is dropped: it was drawn for
/// a geometry that no longer exists, so putting it on the layer replaces a
/// current image with an older, differently-sized one -- and
/// `contentsGravity` pins it to the top-left rather than stretching it, so
/// what the owner saw was two images at once. No frame the layer shows may
/// be a size behind.
///
/// The discard is safe because it is a *generation* comparison rather than
/// a size: a window dragged from A to B and back to A leaves two surface
/// sets that compare equal by size, and one counter cannot mistake them.
/// The failure where the two never agree and every frame is discarded --
/// a black window -- is what `a_steady_surface_set_shows_every_frame`
/// exists to catch, and it is why this cannot be tested by the discard
/// alone.
pub fn admits(fate: Fate) -> Show {
    match fate {
        Fate::Current => Show::Now,
        Fate::Superseded { .. } => Show::Skip,
    }
}
