//! What the present path did, counted -- so "the screen doubled" and "there
//! are freezes" become numbers rather than reports.
//!
//! A frame is committed on the main thread and lands on the GPU's completion
//! thread, which is where the count is taken, so every counter here is an
//! atomic and every write is a `fetch_add`. The counts publish nothing, so
//! they are `Relaxed`: a reader wants the values, and no memory is reached
//! by way of them. **Two words do publish something**: the [`Wheel`]'s
//! held sets and the frames still flying each say "the GPU is done with
//! this memory, write it", so a landing stores them `Release` and the
//! main thread loads them `Acquire` before it writes what they guard.
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

use baud_domain::{Fate, FrameGen, HostTime, SwapEpoch, SwapGen};
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

/// What a completed frame reports: taken in the completion handler from a
/// status read, and nowhere else (`crate::metal`'s commit module).
pub use crate::metal::Landing;

use crate::redraw::{SurfaceIx, Surfaces};

/// Which of the per-frame buffer sets a frame is built into, named where
/// the copy of the screen each set holds is kept (`baud_render::screens`).
pub use baud_render::screens::SetIx;

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
/// **The free is a release and the look an acquire.** A set is taken on
/// the main thread and freed on the GPU's completion thread, which Metal
/// calls "after the GPU finishes executing the command buffer"
/// (`[[source: apple-addcompletedhandler]]`). The main thread then writes
/// the set's buffers, and a CPU write is coherent with the GPU only at
/// command buffer boundaries (`[[source: apple-sdk27-mtlresource-h]]`,
/// line 62), so the writes must come after the landing that freed it.
/// With `Relaxed` on both sides nothing in Rust's memory model puts them
/// there: a `Relaxed` load reading zero orders nothing after it, and the
/// writes may be done before the load in the model's terms. A `Release`
/// free read by an `Acquire` look makes "all subsequent operations become
/// ordered after that store" (`[[source: rust-core-sync-atomic]]`, the
/// `Ordering::Acquire` text), so every write into a set happens after the
/// completion that gave it back. No torn frame was ever seen from this:
/// it closes a gap in the model, not an observed bug.
///
/// No loom or stress test is kept for it: the race is between the CPU's
/// stores and the GPU's reads, which no Rust memory-model checker sees,
/// and a stress test that passes under `Relaxed` on this machine is a
/// probe that cannot fire.
///
/// A free that has happened and has not yet been observed reads as *held*
/// and the main thread skips a set that is already free -- the safe
/// direction. It cannot read a held set as free, because the main thread
/// is the one that held it.
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
    /// **Two, and the two are derived rather than chosen.** Research
    /// decision D14 is one frame in flight, so at most one set is ever
    /// held: one for the frame the GPU is reading and one for the frame
    /// being built is the whole requirement, and a third would only ever
    /// hold memory nothing reads.
    ///
    /// It was three because the surfaces ring at three, which is a fact
    /// about the *layer* -- the compositor may hold one while another is
    /// shown -- and not about these buffers. Copying a number across from
    /// a structure that has a different reason for it is how it got here.
    pub const SETS: usize = baud_render::screens::SETS;

    /// The set to build the next frame into: the next one round, or the
    /// first free one after it. `None` while every set is held, which
    /// means there is nowhere to build that a live frame is not reading
    /// and the caller must not draw.
    pub fn building(&self) -> Option<SetIx> {
        // `next` is the main thread's alone; the look at `held` is the
        // acquire the free's release pairs with.
        let at = self.next.load(Ordering::Relaxed) % Wheel::SETS;
        (0..Wheel::SETS)
            .map(|step| (at + step) % Wheel::SETS)
            .find(|&ix| self.held[ix].load(Ordering::Acquire) == 0)
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
            // `Release` on the free: what the main thread writes after it
            // sees the zero happens after this landing. A compare that
            // fails frees nothing and publishes nothing.
            let _ = h.compare_exchange(frame.value(), 0, Ordering::Release, Ordering::Relaxed);
        }
    }

    /// Whether a set is safe to build into.
    pub fn state(&self, set: SetIx) -> SetState {
        match self.held.get(set.0).map(|h| h.load(Ordering::Acquire)) {
            Some(0) | None => SetState::Free,
            Some(_) => SetState::Held,
        }
    }
}

/// Whether the memory a frame reads that has **no ring** may be written
/// now: the atlas sheets every GPU path samples. The terminal's grid was
/// the other until research decision D4; a frame reads its own set's copy
/// of the window since, so a reflow has nothing in flight to collide with.
///
/// There is one atlas, so a mutation cannot move aside the way a ringed
/// buffer can. Ringing it is the wrong price -- two textures, against an
/// event a keypress or a drag step produces -- so the *request* waits
/// instead, for a completion that is already on its way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reshape {
    /// Nothing is reading it. Reflow, rescale, do as you like.
    Now,
    /// A frame in flight is reading it.
    Wait,
}

/// The ring of presented frames' GPU ends, see [`Journal`].
const ENDS: usize = 8;

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
    /// Frames tagged and then never committed: see [`Journal::abandon`].
    abandoned: AtomicU64,
    /// Frames whose completion said they failed: see [`Journal::fail`].
    failed: AtomicU64,
    /// The surfaces a failed frame went to, as [`Surfaces`]' bits, until
    /// the presenter takes them ([`Journal::take_lost`]).
    lost: AtomicU64,
    /// The last completed frame's GPU nanoseconds.
    gpu_nanos: AtomicU64,
    /// The GPU end of every frame handed to the layer, in landing order,
    /// for the main thread to take one per `presentSurface:` -- so a
    /// present is paired with its own frame's end rather than with the
    /// journal's last, which under typeahead can already be a later
    /// frame's when two complete before one present is processed. A ring
    /// of [`ENDS`]: one frame in flight (D14) means at most one is ever
    /// waiting, and eight is a bound rather than a budget.
    ends: [AtomicU64; ENDS],
    ends_in: AtomicUsize,
    ends_out: AtomicUsize,
    /// When the last completed frame ended on the GPU, as host nanoseconds.
    /// Beside `gpu_nanos` because it arrives with it: one landing stores
    /// both, and a reader on the main thread takes them on the wake the
    /// completion sends.
    ended_nanos: AtomicU64,
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
    /// Frames tagged for a draw that committed nothing -- the GPU was
    /// still reading the last one, or a resource could not be made -- and
    /// given back, so that no reshape waits for a landing that cannot come.
    pub abandoned: u64,
    /// Frames whose completion said the GPU did not draw them: neither
    /// landed nor shown, their surface left to be drawn whole
    /// ([`Journal::fail`]).
    pub failed: u64,
    /// The last completed frame's own start-to-end on the GPU, from the
    /// stamps Metal puts on the command buffer. Recorded where it exists:
    /// on the completion, by the handler that already runs there. Nothing
    /// times it, and no frame reports a zero it has not earned.
    pub gpu: Duration,
    /// When the last landed frame finished on the GPU, on the host clock;
    /// [`HostTime::ZERO`] before any has. The keystroke instrument reads it
    /// on the wake the completion sends -- the same run-loop turn that set
    /// the surface on the layer -- so the frame's end can be paired with the
    /// key that asked for it and with the display-link target after it.
    pub ended: HostTime,
}

impl Journal {
    /// A frame is in flight: it holds `set` if it was built into one, and
    /// it reads the unringed memory either way. The one place a commit is
    /// recorded, so the rule the app runs is the rule the tests pin.
    pub fn commit(&self, set: Option<SetIx>, frame: FrameGen) {
        if let Some(set) = set {
            self.wheel.committed(set, frame);
        }
        self.flying.fetch_add(1, Ordering::Relaxed);
    }

    /// Gives back a frame [`Journal::commit`] recorded and the GPU never
    /// took: the draw found a frame still in flight and answered `Busy`, or
    /// could not make a resource, so no completion will ever land it. Its
    /// set is free and it no longer counts as flying.
    ///
    /// **Without it one such draw broke zoom for the life of the window.**
    /// The tag was minted before the GPU's busy check, so a `present` that
    /// arrived mid-frame -- a relayout, a theme flip -- left `flying` at one
    /// with nothing to land it; every [`Journal::reshape`] after that
    /// answered `Wait`, and every zoom deferred itself forever while the
    /// frames, paced by the GPU's own counter, went on drawing.
    pub fn abandon(&self, tag: FrameTag) {
        if tag.set.is_some() {
            self.wheel.landed(tag.frame);
        }
        self.flying
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
        self.abandoned.fetch_add(1, Ordering::Relaxed);
    }

    /// **Lands a frame drawn into no surface** -- a screen switch's kept
    /// frame, which nothing presents: its set is free and it no longer
    /// counts as flying, as [`Journal::land`] has it, and it is counted
    /// as no frame landed or shown and queues no end for a present to take.
    pub fn land_kept(&self, tag: FrameTag) {
        if tag.set.is_some() {
            self.wheel.landed(tag.frame);
        }
        self.flying
            .try_update(Ordering::Release, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
    }

    /// **A frame whose completion said it failed**: its target holds
    /// undefined content, so it is not presented, and the surface it went
    /// to, if any, is marked for the presenter to draw whole when it comes
    /// round ([`Journal::take_lost`]). Its set is free and it no longer
    /// counts as flying, as [`Journal::land`] has it; it is counted failed,
    /// not landed, and queues no end for a present to take.
    pub fn fail(&self, tag: FrameTag, surface: Option<SurfaceIx>) {
        if let Some(s) = surface {
            self.lost
                .fetch_or(Surfaces::NONE.with(s).0, Ordering::Release);
        }
        if tag.set.is_some() {
            self.wheel.landed(tag.frame);
        }
        self.flying
            .try_update(Ordering::Release, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
        self.failed.fetch_add(1, Ordering::Relaxed);
    }

    /// The surfaces failed frames went to since the last take, cleared.
    pub fn take_lost(&self) -> Surfaces {
        Surfaces(self.lost.swap(0, Ordering::Acquire))
    }

    /// Whether the unringed memory may be written now.
    ///
    /// **Asking is what a caller does immediately before reshaping**, so an
    /// answer of `Wait` is a deferral and is counted as one. That is a
    /// query with a side effect on purpose: it makes "a resize waited"
    /// a number in the frame report rather than a thing to believe.
    pub fn reshape(&self) -> Reshape {
        // `Acquire`, against the landing's `Release`: a reshape told `Now`
        // writes the atlas after the landing that let it go, as a frame's
        // set is written after the free (see [`Wheel`]).
        match self.flying.load(Ordering::Acquire) {
            0 => Reshape::Now,
            _ => {
                self.deferred.fetch_add(1, Ordering::Relaxed);
                Reshape::Wait
            }
        }
    }

    /// Records what a landed frame earned, and answers it so a caller can
    /// act on it. In the observing phase every caller shows the frame
    /// regardless, which is what makes the count a measurement of the bug
    /// rather than of the fix.
    pub fn land(&self, tag: FrameTag, epoch: &SwapEpoch, landing: Landing) -> Fate {
        let fate = baud_domain::fate(tag.swap, epoch.current());
        self.gpu_nanos.store(
            u64::try_from(landing.gpu().as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
        // `Release`, against the `Acquire` in `counts`: a reader that sees
        // the landing count this store precedes is guaranteed this end
        // stamp and not the previous frame's. The count itself stays the
        // module's `Relaxed` -- it publishes nothing -- and the surface it
        // names travels by `performSelectorOnMainThread`, which orders the
        // present after the landing on its own.
        self.ended_nanos
            .store(landing.ended().nanos(), Ordering::Release);
        // Whatever became of it, the buffers it was built into are no
        // longer being read, and neither is the grid or the atlas.
        if tag.set.is_some() {
            self.wheel.landed(tag.frame);
        }
        self.flying
            .try_update(Ordering::Release, Ordering::Relaxed, |n| {
                Some(n.saturating_sub(1))
            })
            .ok();
        self.landed.fetch_add(1, Ordering::Relaxed);
        match fate {
            Fate::Current => self.shown.fetch_add(1, Ordering::Relaxed),
            Fate::Superseded { .. } => self.superseded.fetch_add(1, Ordering::Relaxed),
        };
        if admits(fate) == Show::Now {
            // Queued before the present that carries the surface is posted,
            // so the main thread that takes it finds it: the slot first,
            // then the count with `Release`, against `presented_end`'s
            // `Acquire`.
            let at = self.ends_in.load(Ordering::Relaxed);
            if let Some(slot) = self.ends.get(at % ENDS) {
                slot.store(landing.ended().nanos(), Ordering::Relaxed);
            }
            self.ends_in.store(at.wrapping_add(1), Ordering::Release);
        }
        fate
    }

    /// The GPU end of the frame the main thread is presenting: the oldest
    /// queued by [`Journal::land`] and not yet taken. `None` when nothing
    /// is queued -- a present with no landing behind it, which the caller
    /// reports rather than pairs.
    pub fn presented_end(&self) -> Option<HostTime> {
        let out = self.ends_out.load(Ordering::Relaxed);
        let queued = self.ends_in.load(Ordering::Acquire);
        if out == queued {
            return None;
        }
        let nanos = self.ends.get(out % ENDS)?.load(Ordering::Relaxed);
        self.ends_out.store(out.wrapping_add(1), Ordering::Relaxed);
        Some(HostTime::of_nanos(nanos))
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
            abandoned: self.abandoned.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            gpu: Duration::from_nanos(self.gpu_nanos.load(Ordering::Relaxed)),
            ended: HostTime::of_nanos(self.ended_nanos.load(Ordering::Acquire)),
        }
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
/// what a user saw was two images at once. No frame the layer shows may be
/// a size behind.
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

#[cfg(test)]
mod tests {
    use super::{FrameTag, Journal, Landing, Reshape, SetState};
    use baud_domain::{Fate, FrameGen, HostTime, SwapEpoch};
    use core::time::Duration;

    /// A landing whose timing the test does not examine.
    fn quiet() -> Landing {
        Landing::stamped(Duration::ZERO, HostTime::ZERO)
    }

    /// The defect behind "zoom stopped working": a draw that committed
    /// nothing left its tag flying for good, and every reshape after it
    /// waited. Given back, the set and the reshape are free again, and it is
    /// not counted as a landing.
    #[test]
    fn a_tag_the_gpu_never_took_is_given_back() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        let set = j.wheel().building().expect("a free set");
        let frame = FrameGen::FIRST.next();
        j.commit(Some(set), frame);
        assert_eq!(
            j.reshape(),
            Reshape::Wait,
            "a tagged frame counts as flying"
        );
        assert_eq!(j.wheel().state(set), SetState::Held);
        j.abandon(FrameTag {
            frame,
            swap: epoch.current(),
            set: Some(set),
        });
        assert_eq!(
            j.reshape(),
            Reshape::Now,
            "nothing is reading once it is back"
        );
        assert_eq!(j.wheel().state(set), SetState::Free, "and its set is free");
        let c = j.counts();
        assert_eq!(
            (c.landed, c.shown, c.abandoned),
            (0, 0, 1),
            "an abandoned frame is not a landing"
        );
    }

    /// The control: giving one frame back leaves a frame that is really in
    /// flight reading, and holding its own set, until it lands -- so the
    /// fix cannot pass by freeing everything.
    #[test]
    fn giving_one_tag_back_leaves_a_live_frame_reading() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        let tag = |frame, set| FrameTag {
            frame,
            swap: epoch.current(),
            set: Some(set),
        };
        let live = FrameGen::FIRST.next();
        let first = j.wheel().building().expect("a set");
        j.commit(Some(first), live);
        let dud = live.next();
        let second = j.wheel().building().expect("another");
        j.commit(Some(second), dud);
        j.abandon(tag(dud, second));
        assert_eq!(j.wheel().state(second), SetState::Free);
        assert_eq!(
            j.wheel().state(first),
            SetState::Held,
            "the live frame keeps its set"
        );
        assert_eq!(j.reshape(), Reshape::Wait, "and is still reading");
        j.land(tag(live, first), &epoch, quiet());
        assert_eq!(j.reshape(), Reshape::Now);
        assert_eq!(j.counts().shown, 1);
    }

    /// **A kept frame lands into no surface**: its set is free and the
    /// reshape no longer waits on it once it lands, and it is no frame
    /// landed or shown and queues no end, so the next present takes the
    /// end of its own frame. The control: a frame landed after it is
    /// counted, and its end is the one taken.
    #[test]
    fn a_kept_frame_lands_into_no_surface() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        let set = j.wheel().building().expect("a free set");
        let frame = FrameGen::FIRST.next();
        j.commit(Some(set), frame);
        assert_eq!(j.reshape(), Reshape::Wait, "a kept frame is reading");
        assert_eq!(j.wheel().state(set), SetState::Held);
        j.land_kept(FrameTag {
            frame,
            swap: epoch.current(),
            set: Some(set),
        });
        assert_eq!(
            j.reshape(),
            Reshape::Now,
            "nothing is reading once it lands"
        );
        assert_eq!(j.wheel().state(set), SetState::Free, "and its set is free");
        let c = j.counts();
        assert_eq!(
            (c.landed, c.shown, c.abandoned),
            (0, 0, 0),
            "a kept frame is no frame landed, shown or given back"
        );
        assert_eq!(j.presented_end(), None, "and queues no end");
        let next = FrameTag {
            frame: frame.next(),
            swap: epoch.current(),
            set: None,
        };
        j.commit(None, next.frame);
        j.land(
            next,
            &epoch,
            Landing::stamped(Duration::from_micros(30), HostTime::of_nanos(500)),
        );
        assert_eq!(j.counts().shown, 1);
        assert_eq!(j.presented_end(), Some(HostTime::of_nanos(500)));
        assert_eq!(j.reshape(), Reshape::Now);
    }

    #[test]
    fn a_present_takes_its_own_frame_s_end_in_landing_order() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        assert_eq!(j.presented_end(), None, "nothing has landed");
        let tag = FrameTag {
            frame: FrameGen::FIRST.next(),
            swap: epoch.current(),
            set: None,
        };
        for end in [100, 200] {
            j.land(
                tag,
                &epoch,
                Landing::stamped(Duration::from_micros(50), HostTime::of_nanos(end)),
            );
        }
        // A superseded frame is never presented, so it queues no end.
        epoch.bump();
        j.land(
            tag,
            &epoch,
            Landing::stamped(Duration::from_micros(50), HostTime::of_nanos(300)),
        );
        assert_eq!(j.presented_end(), Some(HostTime::of_nanos(100)));
        assert_eq!(j.presented_end(), Some(HostTime::of_nanos(200)));
        assert_eq!(j.presented_end(), None);
        // The last-value slot still reports the last landing, superseded
        // or not: it is the report's, not the pairing's.
        assert_eq!(j.counts().ended, HostTime::of_nanos(300));
    }

    #[test]
    fn a_landing_reports_when_the_gpu_finished() {
        let epoch = SwapEpoch::new();
        let j = Journal::default();
        assert_eq!(j.counts().ended, HostTime::ZERO, "nothing has landed");
        let tag = FrameTag {
            frame: FrameGen::FIRST.next(),
            swap: epoch.current(),
            set: None,
        };
        let landing =
            Landing::stamped(Duration::from_micros(90), HostTime::of_nanos(1_234_567_890));
        assert_eq!(j.land(tag, &epoch, landing), Fate::Current);
        let c = j.counts();
        assert_eq!(c.gpu, Duration::from_micros(90));
        assert_eq!(c.ended, HostTime::of_nanos(1_234_567_890));
        // The next landing replaces both, and the end never moves backwards
        // on its own: it is the last frame's, whatever the previous read.
        let later = Landing::stamped(Duration::from_micros(40), HostTime::of_nanos(1_234_900_000));
        j.land(tag, &epoch, later);
        assert_eq!(j.counts().ended, HostTime::of_nanos(1_234_900_000));
        assert_eq!(j.counts().gpu, Duration::from_micros(40));
    }

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
        assert_eq!(j.land(tag, &epoch, quiet()), Fate::Current);
        assert_eq!(j.counts().shown, 1);
        assert_eq!(j.counts().superseded, 0);

        // A resize replaces the surface set while this frame is in flight.
        epoch.bump();
        assert!(matches!(
            j.land(tag, &epoch, quiet()),
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
        assert_eq!(j.land(fresh, &epoch, quiet()), Fate::Current);
        assert_eq!(j.counts().shown, 2);
    }
}
