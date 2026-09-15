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
use std::sync::atomic::{AtomicU64, Ordering};

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
}

/// How many frames reached the layer, and how many should not have.
#[derive(Default)]
pub struct Journal {
    shown: AtomicU64,
    superseded: AtomicU64,
    landed: AtomicU64,
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
}

impl Journal {
    /// Records what a landed frame earned, and answers it so a caller can
    /// act on it. In the observing phase every caller shows the frame
    /// regardless, which is what makes the count a measurement of the bug
    /// rather than of the fix.
    pub fn land(&self, tag: FrameTag, epoch: &SwapEpoch) -> Fate {
        let fate = baud_domain::fate(tag.swap, epoch.current());
        self.landed.fetch_add(1, Ordering::Relaxed);
        match fate {
            Fate::Current => self.shown.fetch_add(1, Ordering::Relaxed),
            Fate::Superseded { .. } => self.superseded.fetch_add(1, Ordering::Relaxed),
        };
        fate
    }

    /// What has happened so far.
    pub fn counts(&self) -> Counts {
        Counts {
            landed: self.landed.load(Ordering::Relaxed),
            shown: self.shown.load(Ordering::Relaxed),
            superseded: self.superseded.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrameTag, Journal};
    use baud_domain::{Fate, FrameGen, SwapEpoch};

    #[test]
    fn a_frame_that_outlives_its_surfaces_is_counted_superseded() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        let tag = FrameTag {
            frame: FrameGen::FIRST,
            swap: epoch.current(),
        };
        // Landing before anything moved: current, and shown.
        assert_eq!(j.land(tag, &epoch), Fate::Current);
        assert_eq!(j.counts().shown, 1);
        assert_eq!(j.counts().superseded, 0);

        // A resize replaces the surface set while this frame is in flight.
        epoch.bump();
        assert!(matches!(j.land(tag, &epoch), Fate::Superseded { .. }));
        let c = j.counts();
        assert_eq!((c.landed, c.shown, c.superseded), (2, 1, 1));

        // The control that separates "the rule works" from "the rule
        // rejects everything": a frame drawn for the new set still counts
        // shown. Without this, a guard that discarded every frame -- a
        // black window -- would pass the assertion above.
        let fresh = FrameTag {
            frame: tag.frame.next(),
            swap: epoch.current(),
        };
        assert_eq!(j.land(fresh, &epoch), Fate::Current);
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
/// **It says `Now` for everything, on purpose, and that is the whole of the
/// observing phase.** The counters above are a measurement of the bug only
/// while the bug still happens: a frame that outlives its surfaces is
/// counted `superseded` *and still shown*, which is what the app does today
/// and what the owner reported seeing. Changing this function is the fix,
/// and the tests that pin the current behaviour are what say the fix worked.
pub fn admits(fate: Fate) -> Show {
    match fate {
        Fate::Current => Show::Now,
        Fate::Superseded { .. } => Show::Now,
    }
}
