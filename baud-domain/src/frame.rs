//! Which frame, and which set of surfaces it was drawn for.
//!
//! A frame is committed to the GPU on one thread and lands on another, and
//! the geometry can move in between. Without an identity riding along, a
//! completion is indistinguishable from the newest one, so a frame drawn
//! before a resize can reach the layer after the frame drawn *for* the
//! resize and overwrite it -- the doubled image on a grow, the old font
//! size on a zoom.
//!
//! Two generations, deliberately distinct types over the same
//! representation, because they answer different questions and the wrong
//! one must not compile:
//!
//! * [`FrameGen`] names a frame. It is what a journal reports, and it is
//!   load-bearing only once more than one frame is ever in flight.
//! * [`SwapGen`] names a *set of surfaces*. It is the discriminant: a frame
//!   may be shown exactly when the surfaces it drew into are still the ones
//!   the layer is showing from.
//!
//! **The discriminant is a generation and not a size**, which is not a
//! detail. A window dragged from A to B and back to A while one frame is in
//! flight leaves two different surface sets that compare *equal by size*, so
//! a size test would admit a frame drawn into surfaces that have since been
//! freed and rebuilt. A counter cannot do that. It is also one comparison of
//! a value against itself rather than two sizes computed by different code
//! at different times, so the failure where the two never agree -- and every
//! frame is discarded, and the window is black -- does not exist.

/// Which frame. Monotonic for the life of the process; only [`FrameGen::next`]
/// advances it, so a caller cannot invent one or move it backwards.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct FrameGen(u64);

impl FrameGen {
    /// The frame before any has been drawn.
    pub const FIRST: FrameGen = FrameGen(0);

    /// The next frame after this one.
    pub fn next(self) -> FrameGen {
        FrameGen(self.0.wrapping_add(1))
    }

    /// For the journal, which reports it and does not compute with it.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Which set of surfaces. Bumped whenever the surfaces a presenter draws
/// into are built or rebuilt, so a frame that names an older one was drawn
/// into memory that is no longer what the layer shows from.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct SwapGen(u64);

impl SwapGen {
    /// The first surface set a presenter builds.
    pub const FIRST: SwapGen = SwapGen(0);

    /// The generation after a rebuild.
    pub fn next(self) -> SwapGen {
        SwapGen(self.0.wrapping_add(1))
    }

    /// For the journal.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// What a landed frame earns: the surfaces it drew into are still current,
/// or a newer set replaced them while it was in flight.
///
/// A total enum rather than a `bool`, so neither outcome can be spelled as
/// the other and a third can be added without every call site compiling
/// unchanged.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fate {
    /// Show it.
    Current,
    /// Drop it: the surface set it names has been replaced.
    Superseded {
        /// What the frame was drawn for.
        drawn: SwapGen,
        /// What the presenter draws for now.
        wanted: SwapGen,
    },
}

/// Whether a frame drawn for `drawn` may be shown when the presenter is on
/// `wanted`.
///
/// The whole rule, as a function of two values, so it has a test that needs
/// no GPU, no layer and no window -- which is what lets the bug be proven
/// rather than described.
pub fn fate(drawn: SwapGen, wanted: SwapGen) -> Fate {
    match drawn == wanted {
        true => Fate::Current,
        false => Fate::Superseded { drawn, wanted },
    }
}

#[cfg(test)]
mod tests {
    use super::{Fate, FrameGen, SwapGen, fate};

    #[test]
    fn a_frame_is_current_exactly_while_its_surfaces_are() {
        let a = SwapGen::FIRST;
        let b = a.next();
        assert_eq!(fate(a, a), Fate::Current);
        assert_eq!(fate(b, b), Fate::Current);
        // Drawn before a rebuild, landing after it.
        assert_eq!(
            fate(a, b),
            Fate::Superseded {
                drawn: a,
                wanted: b
            }
        );
        // And the case a size comparison cannot see: a presenter rebuilt
        // twice is not the one that was rebuilt once, however the two
        // happen to be shaped.
        let c = b.next();
        assert_eq!(
            fate(a, c),
            Fate::Superseded {
                drawn: a,
                wanted: c
            }
        );
        assert_ne!(a, c);
    }

    #[test]
    fn an_epoch_supersedes_every_frame_drawn_for_the_set_it_replaced() {
        let e = super::SwapEpoch::new();
        let drawn = e.current();
        assert_eq!(fate(drawn, e.current()), Fate::Current);
        let after = e.bump();
        assert_eq!(e.current(), after);
        assert!(matches!(fate(drawn, e.current()), Fate::Superseded { .. }));
        // And a frame drawn for the new set is current again, so the rule
        // does not simply reject everything once anything has moved -- the
        // failure that would show as a black window.
        assert_eq!(fate(e.current(), e.current()), Fate::Current);
    }

    #[test]
    fn the_two_generations_are_not_interchangeable() {
        // `FrameGen` and `SwapGen` are distinct types over the same
        // representation on purpose: `fate(frame_gen, swap_gen)` does not
        // compile, which is the point of having both. What is checkable
        // here is that each advances on its own.
        assert_eq!(FrameGen::FIRST.next().value(), 1);
        assert_eq!(SwapGen::FIRST.next().value(), 1);
        assert!(FrameGen::FIRST < FrameGen::FIRST.next());
        assert!(SwapGen::FIRST < SwapGen::FIRST.next());
    }
}

/// Which surface set a presenter draws into now, shared with the frames it
/// has in flight.
///
/// It lives here rather than beside the presenter so that [`SwapGen`]'s
/// field stays private: a generation can be read and advanced, and cannot be
/// invented from an integer by a caller that happens to have one.
///
/// `Relaxed` throughout, and that is the whole of the ordering argument.
/// Nothing is published *through* this value -- it guards no memory, and the
/// surfaces it names are reached by other means. What a reader needs is the
/// value itself, and every write is a single `fetch_add` from the thread
/// that owns the geometry.
pub struct SwapEpoch(core::sync::atomic::AtomicU64);

impl SwapEpoch {
    /// An epoch on its first surface set.
    pub fn new() -> SwapEpoch {
        SwapEpoch(core::sync::atomic::AtomicU64::new(SwapGen::FIRST.0))
    }

    /// What a frame drawn now would be drawn for.
    pub fn current(&self) -> SwapGen {
        SwapGen(self.0.load(core::sync::atomic::Ordering::Relaxed))
    }

    /// A new surface set exists; every frame in flight for the old one is
    /// now superseded. Returns what the new generation is.
    pub fn bump(&self) -> SwapGen {
        SwapGen(self.0.fetch_add(1, core::sync::atomic::Ordering::Relaxed) + 1)
    }
}

impl Default for SwapEpoch {
    fn default() -> SwapEpoch {
        SwapEpoch::new()
    }
}
