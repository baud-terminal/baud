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
//!
//! **A refresh the frame in flight holds is not the next frame's.** weston
//! sends a frame's callback once it has latched it and its `presented`
//! only once it is shown (`[[source: weston-16-compositor]]`), so the next
//! frame is drawn while the next refresh is already taken: until that
//! `presented` (or `discarded`) comes, the next frame is planned for no
//! earlier than the refresh after the one the frame in flight could first
//! reach, and two after the last frame shown. Measured on weston 16, where
//! every frame was planned a refresh early without it (note BE).
//!
//! **A frame drawn for a moment is held until it**, where the compositor
//! offers `commit-timing-v1`: a frame whose picture is a function of the
//! moment it is seen -- a trail, a slide, a fade -- is "presented as
//! closely as possible to, but not before, the specified time", "in the
//! domain of the compositor's presentation clock" (`[[source:
//! wp-commit-timing-v1]]`). The time is half a refresh before the refresh
//! nearest the moment it was drawn for: shown at that refresh, never one
//! earlier, and never one later for a prediction off by less than half a
//! period, which weston's own timing can be (note BE).

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

    /// The protocol's spelling, `tv_sec_hi`, `tv_sec_lo` and `tv_nsec`:
    /// the 64-bit seconds in two halves and the nanoseconds, always under
    /// a second.
    pub fn wire(self) -> [u32; 3] {
        let secs = self.0.as_secs();
        let hi = u32::try_from(secs >> 32).unwrap_or(u32::MAX);
        let lo = u32::try_from(secs & u64::from(u32::MAX)).unwrap_or(u32::MAX);
        [hi, lo, self.0.subsec_nanos()]
    }
}

/// Whether a frame committed after the last one shown is still unseen: the
/// compositor has said neither `presented` nor `discarded` of it, so it
/// holds a refresh -- the first it could reach, where that was known when
/// it was committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unseen {
    None,
    Frame(Option<Stamp>),
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
    /// **The first refresh a frame committed at `clock_now` can reach**,
    /// on the presentation clock: the last presentation plus the whole
    /// refresh periods that fit before `clock_now`, plus one -- and, with
    /// a frame still unseen, which takes a refresh after the last shown,
    /// at least two, and at least the one after the refresh it holds.
    /// `None` with no refresh to count by.
    pub fn refresh_after(&self, clock_now: Stamp, unseen: Unseen) -> Option<Stamp> {
        if self.refresh.is_zero() {
            return None;
        }
        let elapsed = clock_now.since(self.at);
        let periods = elapsed.as_nanos() / self.refresh.as_nanos() + 1;
        let (periods, held) = match unseen {
            Unseen::None => (periods, None),
            Unseen::Frame(held) => (periods.max(2), held),
        };
        let ahead = self.refresh.as_nanos().saturating_mul(periods);
        let ahead = Duration::from_nanos(u64::try_from(ahead).unwrap_or(u64::MAX));
        let next = Stamp(self.at.0.saturating_add(ahead));
        let past = held.map(|t| Stamp(t.0.saturating_add(self.refresh)));
        Some(past.map_or(next, |p| p.max(next)))
    }

    /// **When a frame drawn now is first seen**: [`Presentation::refresh_after`]
    /// as an instant, `clock_now` read on the presentation clock as `now`
    /// was taken -- and read no earlier than the last presentation, which
    /// only a clock read out of order could be.
    pub fn next_after(&self, clock_now: Stamp, now: Instant, unseen: Unseen) -> Option<Instant> {
        let next = self.refresh_after(clock_now, unseen)?;
        now.checked_add(next.since(clock_now.max(self.at)))
    }

    /// **The commit timestamp of a frame drawn for `at`**, on the
    /// presentation clock: the refresh nearest `at` -- the last
    /// presentation plus whole periods, `at` read against the clock as
    /// `now` was against `clock_now` -- less half a period. `None` with no
    /// refresh to count by.
    pub fn stamp_for(&self, at: Instant, clock_now: Stamp, now: Instant) -> Option<Stamp> {
        let r = i128::try_from(self.refresh.as_nanos()).ok()?;
        if r == 0 {
            return None;
        }
        let nanos = |d: Duration| i128::try_from(d.as_nanos()).unwrap_or(i128::MAX);
        let ahead = match at.checked_duration_since(now) {
            Some(d) => nanos(d),
            None => -nanos(now.duration_since(at)),
        };
        let from_last = nanos(clock_now.0) + ahead - nanos(self.at.0);
        let nearest = nanos(self.at.0) + (from_last + r / 2).div_euclid(r) * r;
        let stamp = u64::try_from((nearest - r / 2).max(0)).ok()?;
        Some(Stamp(Duration::from_nanos(stamp)))
    }

    /// [`Presentation::next_after`] now, reading the presentation clock;
    /// `None` where the clock cannot be read here.
    pub fn next(&self, unseen: Unseen) -> Option<Instant> {
        let clock_now = crate::sys::clock_now(self.clock)?;
        self.next_after(clock_now, Instant::now(), unseen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// **The next refresh is the last presentation plus whole periods**:
    /// 25 ms after a frame shown at a 10 ms refresh, the next is 5 ms off;
    /// on the refresh itself, a whole period; before the frame's own
    /// time, which a clock read late cannot give, one period. With a frame
    /// unseen, never sooner than the second refresh after the last shown:
    /// 3 ms after it, 17 ms off; 25 ms after it, still the next; and never
    /// sooner than the refresh after the one the unseen frame holds: 25 ms
    /// after, with it holding the 30 ms refresh, 15 ms off. No refresh, no
    /// prediction.
    #[test]
    fn the_next_refresh_is_whole_periods_after_the_last_frame() {
        let at = |ms| Stamp(Duration::from_secs(1000) + MS * ms);
        let now = Instant::now();
        let p = Presentation {
            clock: ClockId(1),
            at: at(0),
            refresh: MS * 10,
        };
        let none = Unseen::None;
        assert_eq!(p.next_after(at(25), now, none), Some(now + MS * 5));
        assert_eq!(p.next_after(at(30), now, none), Some(now + MS * 10));
        assert_eq!(p.next_after(at(3), now, none), Some(now + MS * 7));
        let early = Stamp(Duration::from_secs(999));
        assert_eq!(p.next_after(early, now, none), Some(now + MS * 10));
        let unseen = Unseen::Frame(None);
        assert_eq!(p.next_after(at(3), now, unseen), Some(now + MS * 17));
        assert_eq!(p.refresh_after(at(3), unseen), Some(at(20)));
        assert_eq!(p.next_after(at(25), now, unseen), Some(now + MS * 5));
        let holding = Unseen::Frame(Some(at(30)));
        assert_eq!(p.next_after(at(25), now, holding), Some(now + MS * 15));
        assert_eq!(p.refresh_after(at(3), holding), Some(at(40)));
        let still = Presentation {
            refresh: Duration::ZERO,
            ..p
        };
        assert_eq!(still.next_after(at(25), now, none), None);
    }

    /// **A frame's timestamp is half a period before the refresh nearest
    /// the moment it was drawn for**, on the presentation clock: 25 ms
    /// after a frame shown at a 10 ms refresh, a frame drawn for the next
    /// refresh, 5 ms ahead, is stamped at 25 ms, and so is one whose
    /// moment was read a few microseconds off; one drawn for the refresh
    /// after, at 35 ms; one drawn for a moment already past, at the
    /// refresh nearest it. No refresh, no stamp. The protocol's three
    /// words split the seconds at 32 bits.
    #[test]
    fn a_frame_is_stamped_half_a_period_before_its_refresh() {
        let base = Duration::from_secs(1000);
        let at = |ms| Stamp(base + MS * ms);
        let now = Instant::now() + Duration::from_secs(1);
        let p = Presentation {
            clock: ClockId(1),
            at: at(0),
            refresh: MS * 10,
        };
        let us = Duration::from_micros(1);
        assert_eq!(p.stamp_for(now + MS * 5, at(25), now), Some(at(25)));
        assert_eq!(
            p.stamp_for(now + MS * 5 + us * 3, at(25), now),
            Some(at(25))
        );
        assert_eq!(
            p.stamp_for(now + MS * 5 - us * 3, at(25), now),
            Some(at(25))
        );
        assert_eq!(p.stamp_for(now + MS * 15, at(25), now), Some(at(35)));
        assert_eq!(p.stamp_for(now - MS * 4, at(25), now), Some(at(15)));
        let still = Presentation {
            refresh: Duration::ZERO,
            ..p
        };
        assert_eq!(still.stamp_for(now, at(25), now), None);
        let t = Stamp(Duration::new((7 << 32) + 9, 123));
        assert_eq!(t.wire(), [7, 9, 123]);
    }
}
