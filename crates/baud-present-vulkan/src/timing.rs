//! **What a frame cost the GPU**, read from queries the cell pass writes
//! (`cells.rs`): the span from the command buffer's start to the end of its
//! render pass, and inside it each effect trail's draw, timed by a pair of
//! timestamps and counted by its fragment-shader invocations where the
//! device offers pipeline statistics. The results are read after the fence
//! the frame already waits on, so reading them waits on nothing.
//!
//! A timestamp written at `BOTTOM_OF_PIPE` inside a render pass is taken
//! once every command before it has finished, so a pair around a draw
//! spans that draw and whatever was still running when it began: an upper
//! bound on the draw alone, and the effect's share of the frame where the
//! effect dominates it.

use crate::vk::Has;
use core::time::Duration;

/// The most trail draws a frame times; a frame with more times the first.
pub const EFFECT_SLOTS: usize = 16;

/// Timestamps a frame writes: its start, its pass's end, and a pair for
/// each timed trail draw.
pub const STAMPS: usize = 2 + 2 * EFFECT_SLOTS;

/// What the device can time.
#[derive(Clone, Copy, Debug)]
pub struct Clock {
    /// Nanoseconds a timestamp tick is (`VkPhysicalDeviceLimits::timestampPeriod`).
    pub period: f32,
    /// Whether the queue family's timestamps carry any valid bits.
    pub stamps: Has,
    /// Whether pipeline statistics queries are enabled.
    pub statistics: Has,
}

impl Clock {
    /// `ticks` of this device's clock as time.
    pub fn of(self, ticks: Ticks) -> Duration {
        Duration::from_nanos((ticks.0 as f64 * f64::from(self.period)) as u64)
    }
}

/// Ticks of a device's timestamp clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ticks(pub u64);

/// Fragment-shader invocations counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fragments(pub u64);

/// **What the last frame cost the GPU.**
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuTimes {
    /// From the command buffer's start to the end of its render pass.
    pub pass: Duration,
    /// The trail's draws, summed, where the frame drew a trail.
    pub effect: Option<Duration>,
    /// Their fragment-shader invocations, where the device counts them.
    pub effect_fragments: Option<Fragments>,
    /// How many trail draws were timed.
    pub effect_draws: Draws,
}

/// A count of draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Draws(pub usize);

/// **The times `stamps` say**, the frame having timed `draws` trail draws
/// and counted `fragments`: the pass from stamps 0 to 1, each draw from
/// stamp `2 + 2i` to `3 + 2i`.
pub fn read(
    clock: Clock,
    stamps: &[Ticks],
    draws: Draws,
    fragments: Option<Fragments>,
) -> GpuTimes {
    let span = |a: usize, b: usize| match (stamps.get(a), stamps.get(b)) {
        (Some(a), Some(b)) => clock.of(Ticks(b.0.saturating_sub(a.0))),
        _ => Duration::ZERO,
    };
    let n = draws.0.min(EFFECT_SLOTS);
    let effect = (n > 0).then(|| (0..n).map(|i| span(2 + 2 * i, 3 + 2 * i)).sum());
    GpuTimes {
        pass: span(0, 1),
        effect,
        effect_fragments: fragments.filter(|_| n > 0),
        effect_draws: Draws(n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The spans are read between the right stamps**, at the device's
    /// period, the effect summed over its draws and absent with none.
    #[test]
    fn the_spans_are_read_at_the_devices_period() {
        let clock = Clock {
            period: 2.0,
            stamps: Has::Yes,
            statistics: Has::Yes,
        };
        let stamps = [10, 1010, 100, 150, 300, 400].map(Ticks);
        let t = read(clock, &stamps, Draws(2), Some(Fragments(7)));
        assert_eq!(t.pass, Duration::from_nanos(2000));
        assert_eq!(t.effect, Some(Duration::from_nanos(300)));
        assert_eq!(t.effect_fragments, Some(Fragments(7)));
        let none = read(clock, &stamps, Draws(0), Some(Fragments(7)));
        assert_eq!((none.effect, none.effect_fragments), (None, None));
    }
}
