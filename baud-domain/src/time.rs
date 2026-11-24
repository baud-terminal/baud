//! One clock for the keystroke-to-pixel instrument: mach host time, in
//! nanoseconds.
//!
//! A keystroke's legs are stamped by four different APIs, and every one of
//! them reads the same hardware counter. The SDK headers say so, each in
//! its own words:
//!
//! * `CADisplayLink.timestamp` / `targetTimestamp` -- "Time is represented
//!   using the normal Core Animation conventions, i.e. Mach host time
//!   converted to seconds." (`CADisplayLink.h`)
//! * `CACurrentMediaTime()` -- "the result of calling mach_absolute_time ()
//!   and converting the units to seconds." (`CABase.h`)
//! * `MTLCommandBuffer.GPUStartTime` / `GPUEndTime` -- "The host time in
//!   seconds that GPU starts [finishes] executing this command buffer."
//!   (`MTLCommandBuffer.h`)
//! * `CGEventTimestamp` -- "Event timestamp; roughly, nanoseconds since
//!   startup." (`CGEventTypes.h`). `NSEvent.timestamp` is the same stamp as
//!   an `NSTimeInterval`: the header (`NSEvent.h`) only types it, and the
//!   reference reads "seconds since system startup".
//! * `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)` -- the `mach_absolute_time`
//!   base in nanoseconds, which is how the app reads "now".
//!
//! So a stamp from any of them is a count of nanoseconds on one clock, and
//! [`HostTime`] is that count as a type. The instrument resolves legs of
//! tens of microseconds -- a `write(2)` on the pty, the completion handler's
//! two `performSelectorOnMainThread` calls -- inside a frame whose GPU round
//! trip is ~275 us of wall and a display period of 16.7 ms, so two stamps
//! taken against *different* bases would disagree by the boot time, ~10^5 s
//! after a day up, and a subtraction across bases must not compile. Hence a
//! newtype and not a `u64`, and hence [`HostTime::since`] and
//! [`HostTime::plus`] rather than `Sub` and `Add`: the difference of two
//! stamps is a `Duration`, never another stamp.
//!
//! `std::time::Instant` is deliberately not the type. On macOS today it
//! reads `CLOCK_UPTIME_RAW` and would agree, but nothing guarantees its base
//! is the one AppKit and Metal stamp with, and an `Instant` cannot be built
//! from an `NSEvent.timestamp` at all. The instrument rests on the SDK's
//! stated convention -- and the driver still checks it per key, requiring
//! the event stamp the app reads to fall within 100 ms after the stamp the
//! driver posted, rather than trusting it.

use core::time::Duration;

/// A moment on the host's clock: nanoseconds of mach host time.
///
/// `Default` is [`HostTime::ZERO`], so a struct of counters that derives it
/// starts where a leg that has not happened reads.
///
/// A host stamp is a point, not a span: there is no `Sub` or `Add`, so the
/// difference of two stamps is a `Duration` through [`HostTime::since`] and a
/// stamp moves only by `plus(Duration)`, never by adding another stamp.
///
/// ```compile_fail,E0369
/// let a = baud_domain::HostTime::of_nanos(2);
/// let b = baud_domain::HostTime::of_nanos(1);
/// let _ = a - b;
/// ```
///
/// The legal path:
///
/// ```
/// let a = baud_domain::HostTime::of_nanos(2);
/// let b = baud_domain::HostTime::of_nanos(1);
/// let _: Option<core::time::Duration> = a.since(b);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(transparent)]
pub struct HostTime(u64);

impl HostTime {
    /// Before any stamp: what a leg that has not happened yet reads.
    pub const ZERO: HostTime = HostTime(0);

    /// From the clock's own unit -- `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)`
    /// or a `CGEventTimestamp`, both already nanoseconds.
    pub const fn of_nanos(nanos: u64) -> HostTime {
        HostTime(nanos)
    }

    /// Back to the clock's unit, for a report line, or for the driver that
    /// aligns the stamps it posted with the ones the app read.
    pub const fn nanos(self) -> u64 {
        self.0
    }

    /// From the seconds form the frameworks hand out -- `NSEvent.timestamp`,
    /// `CADisplayLink.targetTimestamp`, `GPUEndTime`. `None` for a value that
    /// is not a time: NaN, infinite, negative, or past what a `u64` of
    /// nanoseconds holds (2^64 ns is ~585 years).
    ///
    /// The seconds form loses nothing a nanosecond can see for the first
    /// 48 days of uptime: below 2^22 s an `f64`'s ulp is 2^-30 s, 0.93 ns,
    /// so the round trip through `Duration` is exact to within one. Past
    /// that the frameworks' own stamps are coarser than a nanosecond, which
    /// is their precision rather than this conversion's, and the legs the
    /// instrument reads are microseconds.
    pub fn of_secs(secs: f64) -> Option<HostTime> {
        let d = Duration::try_from_secs_f64(secs).ok()?;
        u64::try_from(d.as_nanos()).ok().map(HostTime)
    }

    /// How long after `earlier` this is. `None` when `earlier` is later:
    /// a negative leg is a pairing mistake -- a stamp matched to the wrong
    /// key -- and not a duration, so it is refused rather than wrapped.
    pub fn since(self, earlier: HostTime) -> Option<Duration> {
        self.0.checked_sub(earlier.0).map(Duration::from_nanos)
    }

    /// This moment plus `d`, saturating at the clock's end, so a deadline
    /// computed from a stamp clamps instead of wrapping into the past.
    pub fn plus(self, d: Duration) -> HostTime {
        HostTime(
            self.0
                .saturating_add(u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)),
        )
    }
}

/// **An input event's millisecond stamp, "with an undefined base"** --
/// `wl_pointer.button`'s `time` (`[[source: wayland]]`). The base is the
/// compositor's and unknown, so a stamp means nothing alone, and the one
/// thing it offers is [`EventMillis::since`], the time between two: a
/// newtype for the reason [`HostTime`] is one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct EventMillis(u32);

impl EventMillis {
    /// From the wire's `uint`.
    pub const fn of_millis(ms: u32) -> EventMillis {
        EventMillis(ms)
    }

    /// How long after `earlier` this is, read modulo 2^32: 32 bits of
    /// milliseconds wrap after 49.7 days, and a stamp just past the wrap
    /// follows one just before (xterm handles the same rollover in
    /// `EvalSelectUnit`, `[[source: xterm-button-c]]`, one millisecond
    /// short of the modular difference).
    pub fn since(self, earlier: EventMillis) -> Duration {
        Duration::from_millis(u64::from(self.0.wrapping_sub(earlier.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::{EventMillis, HostTime};
    use core::time::Duration;

    #[test]
    fn seconds_round_trip_to_nanoseconds_within_one() {
        // A stamp the size of a day's uptime, carrying sub-microsecond
        // digits: the ulp there is 2^-36 s, 15 ps, so the only rounding is
        // `Duration`'s to the nearest nanosecond.
        let t = HostTime::of_secs(86_400.123_456_789).expect("a time");
        let want = 86_400_123_456_789;
        assert!(
            t.nanos().abs_diff(want) <= 1,
            "{} against {want}",
            t.nanos()
        );
        assert_eq!(
            HostTime::of_secs(1.5),
            Some(HostTime::of_nanos(1_500_000_000))
        );
        assert_eq!(HostTime::of_secs(0.0), Some(HostTime::ZERO));
        assert_eq!(HostTime::default(), HostTime::ZERO);
    }

    #[test]
    fn a_negative_or_nan_second_count_is_refused() {
        assert_eq!(HostTime::of_secs(-1.0), None);
        assert_eq!(HostTime::of_secs(-0.000_000_001), None);
        assert_eq!(HostTime::of_secs(f64::NAN), None);
        assert_eq!(HostTime::of_secs(f64::INFINITY), None);
        // Fits a `Duration` and not a `u64` of nanoseconds: 10^12 s is 10^21
        // ns against a ceiling of 1.8 x 10^19.
        assert_eq!(HostTime::of_secs(1.0e12), None);
    }

    #[test]
    fn since_refuses_an_earlier_that_is_later() {
        let a = HostTime::of_nanos(1_000);
        let b = HostTime::of_nanos(1_250);
        assert_eq!(b.since(a), Some(Duration::from_nanos(250)));
        assert_eq!(a.since(a), Some(Duration::ZERO));
        assert_eq!(a.since(b), None);
        assert!(a < b);
    }

    #[test]
    fn plus_saturates_at_the_clock_end() {
        let near = HostTime::of_nanos(u64::MAX - 5);
        assert_eq!(
            near.plus(Duration::from_nanos(10)),
            HostTime::of_nanos(u64::MAX)
        );
        assert_eq!(
            HostTime::ZERO.plus(Duration::from_secs(2)),
            HostTime::of_nanos(2_000_000_000)
        );
        // And the round trip through `plus` and `since` is the identity
        // where nothing saturates.
        let t = HostTime::of_nanos(7);
        let d = Duration::from_micros(3);
        assert_eq!(t.plus(d).since(t), Some(d));
    }

    /// **An event stamp's difference is read across the wrap**: 50 ms
    /// from ten before 2^32 to forty after it.
    #[test]
    fn an_event_stamp_reads_its_difference_across_the_wrap() {
        let at = EventMillis::of_millis;
        assert_eq!(at(1250).since(at(1000)), Duration::from_millis(250));
        assert_eq!(at(40).since(at(u32::MAX - 9)), Duration::from_millis(50));
        assert_eq!(at(7).since(at(7)), Duration::ZERO);
    }
}
