//! **Effect frames at half the refresh rate when one costs more than the
//! refresh** (row EP1): a trail's, a smear's or the fireworks' frame that
//! misses its refresh is shown late and every frame after it queues behind,
//! so the motion stutters; drawn on every second refresh, on time, it looks
//! smoother. The animation's clock is unchanged -- each frame is drawn for
//! the moment it is seen -- so pacing samples the motion and does not slow
//! it, and a frame owed for anything else is drawn at once.
//!
//! [`EffectPacer`] is the decision, a pure function of the frames it is fed:
//! each effect frame's cost (the turn's frame production, on whichever arm
//! drew it), whether presentation feedback said it missed, and the refresh
//! period. Unknown period: never paced.

use core::time::Duration;

pub use baud_render::Cold;

/// Which arm drew a frame: each keeps its own decision, so a stretch on one
/// does not teach the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacedArm {
    Gpu,
    Cpu,
}

/// How many refreshes an effect frame is drawn on: every one, or every
/// second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divisor {
    Full,
    Half,
}

/// Whether presentation feedback said a frame was shown two or more periods
/// after the moment it was drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missed {
    Yes,
    No,
}

/// A side of the frame, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pixels(pub u32);

/// What an effect frame draws: a trail, or the fireworks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paced {
    Trail(crate::config::CursorTrail),
    Fireworks,
}

/// **What a decision is kept for**: the arm, what the frame draws,
/// the frame's size in pixels and the scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub arm: PacedArm,
    pub drawn: Paced,
    pub size: (Pixels, Pixels),
    pub scale: f64,
}

/// **Every refresh to every second** once the average cost passes this
/// share of the period: a frame that close to the refresh misses it on the
/// next wake-up's jitter.
pub const HALVE_AT: f64 = 0.9;

/// **Every second refresh back to every one** once the average cost has
/// stayed under this share of the period for [`HOLD`] frames with no miss.
/// The gap to [`HALVE_AT`] keeps a cost between the two from flapping.
pub const FULL_BELOW: f64 = 0.6;

/// The effect frames the cost must stay under [`FULL_BELOW`] for.
pub const HOLD: Frames = Frames(30);

/// **Misses that halve whatever the cost**: this many of the last
/// [`WINDOW`] effect frames.
pub const MISSES: Frames = Frames(2);

/// The effect frames [`MISSES`] are counted over.
pub const WINDOW: Frames = Frames(8);

/// **How the average follows a frame's cost**: an eighth of the way.
pub const ALPHA: f64 = 1.0 / 8.0;

/// **The decisions kept**, the most recently used: a key past them starts
/// at every refresh.
pub const KEPT: usize = 8;

/// A count of effect frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Frames(pub u32);

/// The last [`WINDOW`] frames' misses, the newest in the low bit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct History(u8);

impl History {
    fn push(self, m: Missed) -> History {
        History((self.0 << 1) | u8::from(m == Missed::Yes))
    }

    fn count(self) -> Frames {
        let mask = u8::MAX.checked_shr(8 - WINDOW.0.min(8)).unwrap_or(0);
        Frames((self.0 & mask).count_ones())
    }
}

/// **The pacing decision**, fed one effect frame at a time: for the key it
/// was last fed, the average cost, the divisor, the misses and how long the
/// cost has stayed low; and the divisors of the keys before it, most
/// recently used last.
pub struct EffectPacer {
    key: Option<Key>,
    ewma: Option<Duration>,
    divisor: Divisor,
    misses: History,
    low: Frames,
    kept: Vec<(Key, Divisor)>,
}

impl EffectPacer {
    pub fn new() -> EffectPacer {
        EffectPacer {
            key: None,
            ewma: None,
            divisor: Divisor::Full,
            misses: History::default(),
            low: Frames(0),
            kept: Vec::with_capacity(KEPT),
        }
    }

    /// The divisor now.
    pub fn divisor(&self) -> Divisor {
        self.divisor
    }

    /// **One effect frame**: its key, what it cost, whether it missed,
    /// whether its build drew into the atlas, and the refresh period if one
    /// is known. Returns the divisor when this frame changed it. A new key
    /// starts at its kept divisor, or every refresh, with no history
    /// carried over from the key before. A cold build's cost is the
    /// atlas's, not the effect's: it takes up its key and is left out of
    /// the average, the misses and the hold.
    pub fn frame(
        &mut self,
        key: Key,
        cost: Duration,
        missed: Missed,
        cold: Cold,
        period: Option<Duration>,
    ) -> Option<Divisor> {
        let before = self.divisor;
        if self.key != Some(key) {
            self.switch(key);
        }
        if cold == Cold::Yes {
            return (before != self.divisor).then_some(self.divisor);
        }
        let Some(period) = period else {
            self.divisor = Divisor::Full;
            return (before != self.divisor).then_some(self.divisor);
        };
        let ewma = match self.ewma {
            None => cost,
            Some(e) => e.mul_f64(1.0 - ALPHA) + cost.mul_f64(ALPHA),
        };
        self.ewma = Some(ewma);
        self.misses = self.misses.push(missed);
        let share = ewma.as_secs_f64() / period.as_secs_f64().max(f64::MIN_POSITIVE);
        self.low = match (share < FULL_BELOW, missed) {
            (true, Missed::No) => Frames(self.low.0.saturating_add(1)),
            _ => Frames(0),
        };
        self.divisor = match self.divisor {
            Divisor::Full if share > HALVE_AT || self.misses.count() >= MISSES => Divisor::Half,
            Divisor::Half if self.low >= HOLD => Divisor::Full,
            d => d,
        };
        if self.divisor == Divisor::Half && before == Divisor::Full {
            self.low = Frames(0);
        }
        (before != self.divisor).then_some(self.divisor)
    }

    /// **A new key**: the old one's divisor kept, at most [`KEPT`] of them,
    /// the least recently used let go; the new one's taken up, with a fresh
    /// average, misses and hold.
    fn switch(&mut self, key: Key) {
        if let Some(old) = self.key {
            self.kept.retain(|(k, _)| *k != old);
            self.kept.push((old, self.divisor));
            if self.kept.len() > KEPT {
                self.kept.remove(0);
            }
        }
        self.divisor = self
            .kept
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(Divisor::Full, |&(_, d)| d);
        self.key = Some(key);
        self.ewma = None;
        self.misses = History::default();
        self.low = Frames(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HZ240: Duration = Duration::from_nanos(4_166_667);
    const HZ60: Duration = Duration::from_nanos(16_666_667);

    fn key(arm: PacedArm, size: (Pixels, Pixels)) -> Key {
        Key {
            arm,
            drawn: Paced::Trail(crate::config::CursorTrail::Plasma),
            size,
            scale: 1.0,
        }
    }

    const K: Key = Key {
        arm: PacedArm::Cpu,
        drawn: Paced::Trail(crate::config::CursorTrail::Plasma),
        size: (Pixels(3840), Pixels(2160)),
        scale: 1.0,
    };

    /// A share of the period.
    #[derive(Clone, Copy)]
    struct Share(f64);

    fn times(p: Duration, f: Share) -> Duration {
        p.mul_f64(f.0)
    }

    /// Frames at `f` of the period, each fed; the divisor after each.
    fn run(e: &mut EffectPacer, n: Frames, f: Share, period: Duration) -> Vec<Divisor> {
        (0..n.0)
            .map(|_| {
                e.frame(K, times(period, f), Missed::No, Cold::No, Some(period));
                e.divisor()
            })
            .collect()
    }

    /// **A frame at half the period keeps every refresh**, for a thousand
    /// frames.
    #[test]
    fn a_frame_well_inside_the_period_keeps_every_refresh() {
        let mut e = EffectPacer::new();
        assert!(
            run(&mut e, Frames(1000), Share(0.5), HZ240)
                .iter()
                .all(|&d| d == Divisor::Full)
        );
    }

    /// **A frame past the period halves within eight frames and stays
    /// halved.** Named break: a pacer that never paces.
    #[test]
    fn a_frame_past_the_period_halves_and_stays() {
        let mut e = EffectPacer::new();
        let ds = run(&mut e, Frames(200), Share(1.2), HZ240);
        let first = ds.iter().position(|&d| d == Divisor::Half).expect("halved");
        assert!(first < 8, "halved at frame {first}");
        assert!(ds[first..].iter().all(|&d| d == Divisor::Half), "and stays");
    }

    /// **A cost swinging between 0.6 and 0.9 of the period switches at most
    /// once in a thousand frames**: the gap between the two thresholds and
    /// the hold keep it from flapping.
    #[test]
    fn a_cost_between_the_thresholds_does_not_flap() {
        let mut e = EffectPacer::new();
        let mut switches = 0;
        for i in 0..1000 {
            let f = Share(if i % 2 == 0 { 0.6 } else { 0.9 });
            switches += usize::from(
                e.frame(K, times(HZ240, f), Missed::No, Cold::No, Some(HZ240))
                    .is_some(),
            );
        }
        assert!(switches <= 1, "{switches} switches");
    }

    /// **Back to every refresh only after thirty frames under 0.6 of the
    /// period**: 1.2 of it for fifty frames halves; at 0.4 the average
    /// falls under 0.6 after a dozen frames, and the divisor holds for the
    /// thirty after that.
    #[test]
    fn recovery_waits_thirty_frames_under_the_low_threshold() {
        let mut e = EffectPacer::new();
        run(&mut e, Frames(50), Share(1.2), HZ240);
        assert_eq!(e.divisor(), Divisor::Half);
        let ds = run(&mut e, Frames(80), Share(0.4), HZ240);
        let back = ds
            .iter()
            .position(|&d| d == Divisor::Full)
            .expect("recovered");
        assert!(back >= 30, "recovered after {back} frames");
        assert!(back < 60, "recovered after {back} frames");
        assert!(ds[back..].iter().all(|&d| d == Divisor::Full));
    }

    /// **Two misses in eight frames halve, though each frame cost little**;
    /// one does not.
    #[test]
    fn misses_alone_halve() {
        let mut e = EffectPacer::new();
        let cheap = times(HZ240, Share(0.3));
        e.frame(K, cheap, Missed::Yes, Cold::No, Some(HZ240));
        for _ in 0..3 {
            e.frame(K, cheap, Missed::No, Cold::No, Some(HZ240));
        }
        assert_eq!(e.divisor(), Divisor::Full, "one miss");
        e.frame(K, cheap, Missed::Yes, Cold::No, Some(HZ240));
        assert_eq!(e.divisor(), Divisor::Half, "two in eight");
    }

    /// **A change of period is weighed at once**: halved at 240 Hz by a
    /// frame of 5 ms, the same frame at 60 Hz is well inside, and every
    /// refresh returns after the hold.
    #[test]
    fn a_change_of_period_is_weighed_again() {
        let mut e = EffectPacer::new();
        let cost = Duration::from_millis(5);
        for _ in 0..20 {
            e.frame(K, cost, Missed::No, Cold::No, Some(HZ240));
        }
        assert_eq!(e.divisor(), Divisor::Half);
        for _ in 0..40 {
            e.frame(K, cost, Missed::No, Cold::No, Some(HZ60));
        }
        assert_eq!(e.divisor(), Divisor::Full);
    }

    /// **With no period known nothing is paced**, whatever a frame costs.
    #[test]
    fn an_unknown_period_never_paces() {
        let mut e = EffectPacer::new();
        for _ in 0..100 {
            assert_eq!(
                e.frame(K, Duration::from_millis(50), Missed::Yes, Cold::No, None),
                None
            );
        }
        assert_eq!(e.divisor(), Divisor::Full);
    }

    /// **The thresholds, one a direction, as named**: halved just past
    /// [`HALVE_AT`] and not just under it; back to every refresh under
    /// [`FULL_BELOW`] after exactly [`HOLD`] frames and not one sooner.
    #[test]
    fn the_thresholds_are_the_named_ones() {
        let mut e = EffectPacer::new();
        run(&mut e, Frames(50), Share(HALVE_AT - 0.01), HZ240);
        assert_eq!(e.divisor(), Divisor::Full, "just under HALVE_AT");
        let mut e = EffectPacer::new();
        run(&mut e, Frames(1), Share(HALVE_AT + 0.01), HZ240);
        assert_eq!(e.divisor(), Divisor::Half, "just past HALVE_AT");
        let mut e = EffectPacer::new();
        run(&mut e, Frames(1), Share(2.0), HZ240);
        // From a fresh average at the low cost: the same key again.
        e.switch(key(PacedArm::Gpu, (Pixels(1), Pixels(1))));
        e.switch(K);
        assert_eq!(e.divisor(), Divisor::Half, "kept");
        let ds = run(&mut e, HOLD, Share(FULL_BELOW - 0.01), HZ240);
        assert_eq!(ds[ds.len() - 2], Divisor::Half, "one short of HOLD");
        assert_eq!(ds[ds.len() - 1], Divisor::Full, "at HOLD");
    }

    /// **The decisions kept are bounded**: a thousand sizes, each halved,
    /// keep at most [`KEPT`], the most recent; the first is let go and
    /// starts at every refresh.
    #[test]
    fn the_decisions_kept_are_bounded() {
        let mut e = EffectPacer::new();
        let slow = times(HZ240, Share(1.2));
        for w in 0..1000u32 {
            e.frame(
                key(PacedArm::Cpu, (Pixels(w + 1), Pixels(100))),
                slow,
                Missed::No,
                Cold::No,
                Some(HZ240),
            );
        }
        assert!(e.kept.len() <= KEPT, "{} kept", e.kept.len());
        e.frame(
            key(PacedArm::Cpu, (Pixels(999), Pixels(100))),
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Half, "a recent one kept");
        e.frame(
            key(PacedArm::Cpu, (Pixels(1), Pixels(100))),
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Full, "the first let go");
    }

    /// **A decision is kept per arm, effect, size and scale**: halved on the
    /// CPU arm at 4K, the GPU arm starts at every refresh, and back on the
    /// CPU arm at 4K the half is taken up again at once; the fireworks at
    /// the same arm and size start at every refresh too; a new size starts
    /// afresh.
    #[test]
    fn a_decision_is_kept_per_arm_and_size() {
        let mut e = EffectPacer::new();
        let (cpu, gpu) = (
            key(PacedArm::Cpu, (Pixels(3840), Pixels(2160))),
            key(PacedArm::Gpu, (Pixels(3840), Pixels(2160))),
        );
        let slow = times(HZ240, Share(1.2));
        for _ in 0..10 {
            e.frame(cpu, slow, Missed::No, Cold::No, Some(HZ240));
        }
        assert_eq!(e.divisor(), Divisor::Half);
        e.frame(
            gpu,
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Full, "the GPU arm learns its own");
        e.frame(
            cpu,
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Half, "the CPU arm's kept");
        let fireworks = Key {
            drawn: Paced::Fireworks,
            ..cpu
        };
        e.frame(
            fireworks,
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Full, "the fireworks learn their own");
        e.frame(
            cpu,
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Half, "the trail's kept");
        e.frame(
            key(PacedArm::Cpu, (Pixels(1920), Pixels(1080))),
            times(HZ240, Share(0.2)),
            Missed::No,
            Cold::No,
            Some(HZ240),
        );
        assert_eq!(e.divisor(), Divisor::Full, "a new size afresh");
    }

    /// **One cold build keeps every refresh, and a warm one as slow does
    /// not**: a 48 ms frame against a 60 Hz period that drew into the atlas
    /// leaves the divisor at every refresh and the average unmoved, so a
    /// warm frame at 0.8 of the period after it keeps it there too. The
    /// control: the same frame built warm halves at once.
    #[test]
    fn a_cold_frame_is_left_out_of_the_average() {
        let slow = Duration::from_millis(48);
        let mut e = EffectPacer::new();
        assert_eq!(e.frame(K, slow, Missed::No, Cold::Yes, Some(HZ60)), None);
        assert_eq!(e.divisor(), Divisor::Full, "the cold frame");
        e.frame(K, times(HZ60, Share(0.8)), Missed::No, Cold::No, Some(HZ60));
        assert_eq!(e.divisor(), Divisor::Full, "the warm frame after it");
        let mut e = EffectPacer::new();
        assert_eq!(
            e.frame(K, slow, Missed::No, Cold::No, Some(HZ60)),
            Some(Divisor::Half),
            "the control"
        );
    }
}
