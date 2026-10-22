//! **When a frame will be seen, from the compositor's presentation
//! feedback.** A frame is drawn when the frame callback says "it is a good
//! time to start drawing a new frame" (`[[source: wayland]]`), never on a
//! timer; what is drawn into it -- the bell's flash, the cursor's blink,
//! a trail -- is planned for the moment it reaches the screen, as the
//! macOS app plans for its display link's target. Where the compositor
//! offers `wp_presentation`, each commit asks for feedback, and
//! `presented` says when the last frame "turned into light" and "how many
//! nanoseconds after tv_sec, tv_nsec the very next output refresh may
//! occur" (`[[source: wp-presentation-time]]`): the next refresh after
//! now is that time plus whole refresh periods. The timestamps are on the
//! presentation clock, "one of the clockid_t values accepted by
//! clock_gettime()", and "Clients must be able to query the current clock
//! value directly", so the clock is read here and the two instants set
//! side by side.

use std::time::{Duration, Instant};

/// A `clockid_t`, as the compositor's `clock_id` event names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct ClockId(pub i32);

/// A moment on a clock, as the time since its zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub Duration);

impl Stamp {
    fn since(self, earlier: Stamp) -> Duration {
        self.0.saturating_sub(earlier.0)
    }
}

/// The last frame the compositor showed: when, on which clock, and how
/// long until the refresh after it -- zero where it cannot predict one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Presentation {
    pub clock: ClockId,
    pub at: Stamp,
    pub refresh: Duration,
}

impl Presentation {
    /// **The next refresh after `now`**, the moment a frame drawn now is
    /// first seen: the last presentation plus the whole refresh periods
    /// that fit before `clock_now`, plus one -- `clock_now` read on the
    /// presentation clock as `now` was taken. `None` with no refresh to
    /// count by.
    pub fn next_after(&self, clock_now: Stamp, now: Instant) -> Option<Instant> {
        if self.refresh.is_zero() {
            return None;
        }
        let elapsed = clock_now.since(self.at);
        let periods = elapsed.as_nanos() / self.refresh.as_nanos() + 1;
        let next = self
            .refresh
            .as_nanos()
            .saturating_mul(periods)
            .saturating_sub(elapsed.as_nanos());
        let next = Duration::from_nanos(u64::try_from(next).unwrap_or(u64::MAX));
        now.checked_add(next)
    }

    /// [`Presentation::next_after`] now, reading the presentation clock;
    /// `None` where the clock cannot be read here.
    pub fn next(&self) -> Option<Instant> {
        let clock_now = crate::sys::clock_now(self.clock)?;
        self.next_after(clock_now, Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// **The next refresh is the last presentation plus whole periods**:
    /// 25 ms after a frame shown at a 10 ms refresh, the next is 5 ms off;
    /// on the refresh itself, a whole period; before the frame's own
    /// time, which a clock read late cannot give, one period. No refresh,
    /// no prediction.
    #[test]
    fn the_next_refresh_is_whole_periods_after_the_last_frame() {
        let at = |ms| Stamp(Duration::from_secs(1000) + MS * ms);
        let now = Instant::now();
        let p = Presentation {
            clock: ClockId(1),
            at: at(0),
            refresh: MS * 10,
        };
        assert_eq!(p.next_after(at(25), now), Some(now + MS * 5));
        assert_eq!(p.next_after(at(30), now), Some(now + MS * 10));
        assert_eq!(p.next_after(at(3), now), Some(now + MS * 7));
        let early = Stamp(Duration::from_secs(999));
        assert_eq!(p.next_after(early, now), Some(now + MS * 10));
        let still = Presentation {
            refresh: Duration::ZERO,
            ..p
        };
        assert_eq!(still.next_after(at(25), now), None);
    }
}
