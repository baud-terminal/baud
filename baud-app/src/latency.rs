//! The keystroke-to-pixel instrument: one record per key, stamped as the
//! key crosses each stage of this process, and one line to stderr per key
//! once the last stage is known. Row 0 of `docs/research/notes/n-make-faster.md`;
//! the design and what it first read are `docs/research/notes/o-keystroke-latency.md`.
//!
//! **The clock.** Every stamp is a [`HostTime`] -- mach host time in
//! nanoseconds -- whether it came from `NSEvent.timestamp`, from the pty
//! reader, from `clock_gettime_nsec_np(CLOCK_UPTIME_RAW)`, from Metal's
//! `GPUEndTime` or from the display link's target, because the SDK documents
//! every one of those on that single base (`baud_domain::time` quotes each
//! header). So a leg is one stamp less another from a different API, and it
//! means what it says rather than the difference of two boot times. The one
//! stamp whose base Apple does not name, the event's, was measured instead:
//! a posted event's stamp arrives in `NSEvent.timestamp` unchanged, and the
//! driver checks it per key.
//!
//! **The pairing rule.** A stage stamps *every open record that has reached
//! the stage before it and not this one*: `written` every record with no
//! `written`, `echo` every record handled before the read and with no
//! `echo`, and so on down the list. With one frame in flight and keys slower
//! than a frame, one key is one echo is one frame and every stamp lands on
//! exactly one record; under typeahead several keys share an echo read and
//! a frame and each of them gets the same stamp, which the driver reports as
//! keys that shared a frame. That is honest and simple, and it is why there
//! is no per-frame identity threaded through the app.
//!
//! **Off by default**, enabled by `BAUD_KEYLAT=1`: the stages are the hot
//! path -- `pump`, `present`, every key -- and the instrument must change
//! nothing a user sees and nothing the frame benches measure. Off, every
//! method is one test of an `Option` and returns; the stamps are handed in
//! as closures so that not even the clock is read. On, a key costs a clock
//! read per stage and a queue of at most [`CAP`] records.
//!
//! The legs, in order, and where each is taken (`app.rs`):
//!
//! | leg | when |
//! |---|---|
//! | `event` | `NSEvent.timestamp` of the key event, or `handled` if none |
//! | `handled` | `Responder::key`, `text` or `command` sending the key |
//! | `written` | `Pty::write` returned with bytes written: the echo is requested |
//! | `echo` | the pty reader's stamp on the first `read(2)` after `handled` |
//! | `fed` | the core has been fed that read, in `pump` |
//! | `commit` | the first frame committed after `fed`, stamped before the draw call |
//! | `landed` | that frame's `GPUEndTime`, from the journal; `-` on the CPU path or when refused |
//! | `shown` | the surface set on the layer: `Responder::presented`, fired by the view where it commits the `CATransaction` |
//! | `vsync` | the **early** bound of the pixel time: the display-link target announced before `shown` |
//! | `vsync_hi` | the **late** bound: the target after it |
//!
//! The two bounds are what the display link can say. Its tick fires just
//! after a vsync and announces the target the next vsync is due at; a
//! surface set before that target is on the panel at it when the commit
//! met the compositor's deadline, and at the one after when it did not.
//! Which of the two the panel agrees with is not decidable from inside the
//! process, so both are printed and `scripts/key-pixel.swift` -- the window
//! server's own display time of the changed frame -- is what adjudicates.
//!
//! **The echo guard is `handled`, not `written`.** A read stamped on the
//! reader thread can precede the main thread's post-write stamp: the line
//! discipline echoes inside the master's `write(2)` and wakes the reader
//! before the writer's syscall returns, so under a preemption the echo
//! would be refused and the key would absorb the *next* key's echo instead.
//! A read cannot precede the responder that asked for it, so that is the
//! bound; what it admits is unrelated output arriving in the microseconds
//! between the responder running and the write returning, which `cat` never
//! produces and a shell rarely does.

use baud_domain::HostTime;
use baud_render::Tally;
use std::time::Duration;

/// A record older than this since its `event` is emitted with the legs it
/// has and `-` for the rest: a key whose child never echoed, or whose echo
/// drew no frame.
const EXPIRE: Duration = Duration::from_secs(2);

/// The most records open at once. Reaching it means ticks stopped for
/// longer than [`EXPIRE`] while keys kept arriving; the oldest is emitted
/// as expired and dropped so the queue is bounded.
const CAP: usize = 256;

/// A GPU end stamp further than this before the surface was set is not a
/// landing on this clock: a `GPUEndTime` of zero (Metal before completion,
/// or a conversion that refused), or a stamp on some other base. A frame's
/// round trip is ~275 us and the display period 16.7 ms, so a second is
/// generous by a thousand.
const SANE_LANDING: Duration = Duration::from_secs(1);

/// Whether the instrument records anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Switch {
    Off,
    On,
}

/// What [`Latency::presented`] made of the GPU end stamp it was handed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ended {
    /// On the clock, and before the surface was set: `landed` is stamped.
    OnClock,
    /// Zero, after `shown`, or more than [`SANE_LANDING`] before it: the
    /// key still completes, with `landed` left `-`, and the app says so.
    Refused,
}

/// Whether a key's bytes went to the child: what the `written` leg is
/// waiting for, has, or will never have.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Written {
    /// The key was handled and the write has not returned.
    Pending,
    /// Nothing went out -- the core encoded the key to nothing, or the
    /// kernel refused the write -- so no echo is owed and none is taken.
    Never,
    /// The write returned with every byte gone.
    At(HostTime),
}

/// The journal: `None` when off, so every method is one test.
pub struct Latency {
    on: Option<Open>,
}

/// The open records and the running count, present only when on.
struct Open {
    seq: Tally,
    records: Vec<Record>,
    /// Lines whose record was closed outside a tick -- the cap's eviction
    /// -- waiting for the next tick to print them in order.
    lines: Vec<String>,
    /// The display-link target the last tick announced: the vsync a
    /// surface set before it is due at.
    announced: Option<HostTime>,
    /// The step between the last two announcements that was not a gap:
    /// the display period, as the link delivers it.
    period: Option<Duration>,
}

/// One key, from its event to the vsync bounds after it was shown.
struct Record {
    seq: Tally,
    event: HostTime,
    handled: HostTime,
    written: Written,
    echo: Option<HostTime>,
    fed: Option<HostTime>,
    committed: Option<HostTime>,
    landed: Option<HostTime>,
    shown: Option<HostTime>,
    vsync: Option<HostTime>,
    vsync_hi: Option<HostTime>,
    /// Whether the vsync bounds can be taken from the ticks that follow
    /// `shown`, or a tick was dropped in between.
    bounds: Bounds,
}

/// Whether a record's vsync bounds are what the ticks say. A tick dropped
/// between the record being shown and its bounds being taken means the
/// target the next tick announced is not the vsync after `shown`, and
/// neither bound is known.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Bounds {
    FromTicks,
    Gapped,
}

impl Record {
    fn open(seq: Tally, event: HostTime, handled: HostTime) -> Record {
        Record {
            seq,
            event,
            handled,
            written: Written::Pending,
            echo: None,
            fed: None,
            committed: None,
            landed: None,
            shown: None,
            vsync: None,
            vsync_hi: None,
            bounds: Bounds::FromTicks,
        }
    }

    fn written_at(&self) -> Option<HostTime> {
        match self.written {
            Written::At(t) => Some(t),
            Written::Pending | Written::Never => None,
        }
    }

    /// A leg as the line prints it: microseconds after `event` with one
    /// decimal and a leading `+`, or `-` for a leg that never happened --
    /// including one stamped *before* the event, which is a pairing
    /// mistake and not a duration.
    fn offset(&self, at: Option<HostTime>) -> String {
        match at.and_then(|t| t.since(self.event)) {
            Some(d) => format!("+{:.1}", d.as_secs_f64() * 1e6),
            None => "-".to_string(),
        }
    }

    /// The line the driver parses: `event` absolute in nanoseconds, so it
    /// can align the stamp it posted; every other field an offset.
    fn line(&self) -> String {
        format!(
            "keylat: seq={} event={} handled={} written={} echo={} fed={} commit={} landed={} shown={} vsync={} vsync_hi={}",
            self.seq.value(),
            self.event.nanos(),
            self.offset(Some(self.handled)),
            self.offset(self.written_at()),
            self.offset(self.echo),
            self.offset(self.fed),
            self.offset(self.committed),
            self.offset(self.landed),
            self.offset(self.shown),
            self.offset(self.vsync),
            self.offset(self.vsync_hi),
        )
    }

    /// The surface this key's frame is in is on the layer: the early vsync
    /// bound is the announced target if the surface beat it, else nothing
    /// yet, and the next tick fills whichever bound is missing.
    fn shown_at(&mut self, shown: HostTime, announced: Option<HostTime>) {
        self.shown = Some(shown);
        self.vsync = announced.filter(|&t| shown < t);
    }
}

impl Latency {
    /// On when `BAUD_KEYLAT=1`, off otherwise.
    pub fn from_env() -> Latency {
        Latency::new(match std::env::var("BAUD_KEYLAT").as_deref() {
            Ok("1") => Switch::On,
            _ => Switch::Off,
        })
    }

    pub fn new(switch: Switch) -> Latency {
        Latency {
            on: match switch {
                Switch::Off => None,
                Switch::On => Some(Open {
                    seq: Tally::default(),
                    records: Vec::new(),
                    lines: Vec::new(),
                    announced: None,
                    period: None,
                }),
            },
        }
    }

    pub fn on(&self) -> Switch {
        match self.on {
            Some(_) => Switch::On,
            None => Switch::Off,
        }
    }

    /// A key the responder is about to send: opens a record. `event` is
    /// the key event's own stamp, when there is one; `handled` is now.
    pub fn key(
        &mut self,
        event: impl FnOnce() -> Option<HostTime>,
        handled: impl FnOnce() -> HostTime,
    ) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let handled = handled();
        let event = event().unwrap_or(handled);
        j.seq.bump();
        if j.records.len() >= CAP {
            let oldest = j.records.remove(0);
            j.lines.push(oldest.line());
        }
        j.records.push(Record::open(j.seq, event, handled));
    }

    /// The pty write returned with every byte gone: every record still
    /// waiting for its write.
    pub fn written(&mut self, at: impl FnOnce() -> HostTime) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let at = at();
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.written == Written::Pending)
        {
            r.written = Written::At(at);
        }
    }

    /// Nothing went to the child for the key just handled -- the core
    /// encoded it to nothing, or the kernel refused the write -- so every
    /// record still waiting for its write will never have one. Without
    /// this the *next* key's write would stamp it and its echo would be
    /// taken twice.
    pub fn unsent(&mut self) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.written == Written::Pending)
        {
            r.written = Written::Never;
        }
    }

    /// The reads a drain found, in order, by the reader's stamp on each:
    /// every written record with no `echo` takes the first read stamped at
    /// or after its `handled` -- a read that was already in the channel
    /// when the key was handled is not its echo.
    pub fn echoed(&mut self, reads: impl IntoIterator<Item = HostTime>) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        for at in reads {
            for r in j
                .records
                .iter_mut()
                .filter(|r| r.written_at().is_some() && r.echo.is_none())
            {
                if r.handled <= at {
                    r.echo = Some(at);
                }
            }
        }
    }

    /// The core has been fed the reads: every record with `echo` and no
    /// `fed`.
    pub fn fed(&mut self, at: impl FnOnce() -> HostTime) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let at = at();
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.echo.is_some() && r.fed.is_none())
        {
            r.fed = Some(at);
        }
    }

    /// A stamp read now, when on, for a caller that must take it *before*
    /// a call whose outcome decides whether it is used -- the commit stamp
    /// before the draw call, which may answer `Busy`. [`HostTime::ZERO`]
    /// when off, and then never used, because every method returns first.
    pub fn stamp(&self, now: impl FnOnce() -> HostTime) -> HostTime {
        match self.on {
            Some(_) => now(),
            None => HostTime::ZERO,
        }
    }

    /// A frame was committed to the GPU: every record with `fed` and no
    /// `commit`. `at` is read *before* the draw call, so it is provably
    /// before the GPU could have finished the frame -- a stamp taken after
    /// the call, under a preemption longer than the GPU's ~100 us of work,
    /// would read later than the frame's own end and be paired with the
    /// next one.
    pub fn committed(&mut self, at: impl FnOnce() -> HostTime) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let at = at();
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.fed.is_some() && r.committed.is_none())
        {
            r.committed = Some(at);
        }
    }

    /// A frame's surface is on the layer as of `shown`, and the GPU
    /// finished it at `ended` by the journal: every committed record not
    /// yet shown, less any committed *after* the GPU ended -- that one is
    /// the next frame's, which a tick can commit between the completion and
    /// this call. An `ended` that is not a landing on this clock (see
    /// [`SANE_LANDING`]) is refused: the records are still shown, `landed`
    /// stays `-`, and the caller is told so it can say so.
    pub fn presented(&mut self, ended: HostTime, shown: HostTime) -> Ended {
        let Some(j) = self.on.as_mut() else {
            return Ended::OnClock;
        };
        let sane =
            ended != HostTime::ZERO && shown.since(ended).is_some_and(|late| late < SANE_LANDING);
        let announced = j.announced;
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.committed.is_some() && r.shown.is_none())
        {
            if sane && r.committed.is_some_and(|c| c > ended) {
                continue;
            }
            r.landed = sane.then_some(ended);
            r.shown_at(shown, announced);
        }
        if sane { Ended::OnClock } else { Ended::Refused }
    }

    /// The CPU path: the surface was set on the layer synchronously, so
    /// `commit` and `shown` are one moment and nothing lands on a GPU.
    pub fn shown_cpu(&mut self, at: impl FnOnce() -> HostTime) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let at = at();
        let announced = j.announced;
        for r in j
            .records
            .iter_mut()
            .filter(|r| r.fed.is_some() && r.committed.is_none())
        {
            r.committed = Some(at);
            r.shown_at(at, announced);
        }
    }

    /// A display-link tick announced `target`: the vsync a surface set from
    /// now until the next tick is due at. Called at the top of the tick,
    /// before anything this tick presents.
    ///
    /// A step of more than one and a half periods since the last
    /// announcement is a dropped tick -- the link skips a callback when the
    /// previous one had not returned by the next vsync -- and a record shown
    /// in that gap would take a target one period too late as its early
    /// bound. Such records are marked and emitted with both bounds `-`
    /// rather than a plausible number.
    pub fn announced(&mut self, target: impl FnOnce() -> Option<HostTime>) {
        let Some(j) = self.on.as_mut() else {
            return;
        };
        let Some(t) = target() else {
            return;
        };
        if let Some(step) = j.announced.and_then(|prev| t.since(prev)) {
            match j.period {
                Some(p) if step > p + p / 2 => {
                    for r in j
                        .records
                        .iter_mut()
                        .filter(|r| r.shown.is_some() && r.vsync_hi.is_none())
                    {
                        r.bounds = Bounds::Gapped;
                    }
                }
                _ => j.period = Some(step),
            }
        }
        j.announced = Some(t);
    }

    /// The same tick, after everything it presented: a record shown after
    /// the last announcement takes this target as its early bound, a record
    /// with its early bound takes this target as its late one, and a record
    /// with both is complete. Prints the completed and the expired records,
    /// in order, and drops them. `None` from `target` -- a stamp that is not
    /// a time -- is a tick that stamps nothing.
    pub fn tick(&mut self, target: impl FnOnce() -> Option<HostTime>) {
        if self.on.is_none() {
            return;
        }
        let Some(target) = target() else {
            return;
        };
        for line in self.settle(target) {
            eprintln!("{line}");
        }
    }

    /// What [`Latency::tick`] does before printing, so a test reads the
    /// lines rather than stderr.
    fn settle(&mut self, target: HostTime) -> Vec<String> {
        let Some(j) = self.on.as_mut() else {
            return Vec::new();
        };
        for r in j.records.iter_mut().filter(|r| r.vsync_hi.is_none()) {
            match r.vsync {
                Some(lo) if lo < target => r.vsync_hi = Some(target),
                Some(_) => {}
                None if r.shown.is_some_and(|s| s < target) => r.vsync = Some(target),
                None => {}
            }
        }
        let mut out = std::mem::take(&mut j.lines);
        j.records.retain_mut(|r| {
            if r.bounds == Bounds::Gapped {
                r.vsync = None;
                r.vsync_hi = None;
                out.push(r.line());
                return false;
            }
            let expired = target.since(r.event).is_some_and(|age| age > EXPIRE);
            if r.vsync_hi.is_some() || expired {
                out.push(r.line());
                return false;
            }
            true
        });
        out
    }

    /// Records opened and not yet emitted; zero when off.
    pub fn open(&self) -> Tally {
        self.on
            .as_ref()
            .map_or_else(Tally::default, |j| Tally::of(j.records.len()))
    }

    /// The line the five-second report adds when the instrument is on.
    pub fn report(&self) -> Option<String> {
        self.on
            .as_ref()
            .map(|_| format!("keylat: {} keys open", self.open().value()))
    }
}

#[cfg(test)]
mod tests {
    use super::{Ended, Latency, Record, Switch, Written};
    use baud_domain::HostTime;
    use baud_render::Tally;
    use std::time::Duration;

    /// `after` one second of uptime, so the offsets are round.
    fn t(after: Duration) -> HostTime {
        HostTime::of_nanos(1_000_000_000).plus(after)
    }

    /// Microseconds as a `Duration`; a macro rather than a function so the
    /// test file adds no raw-integer signature to the crate's count.
    macro_rules! us {
        ($n:expr) => {
            Duration::from_micros($n)
        };
    }

    #[test]
    fn a_key_completes_through_every_leg_and_emits_exactly_one_line() {
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(8_333))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(300)));
        l.written(|| t(us!(350)));
        l.echoed([t(us!(360))]);
        l.fed(|| t(us!(365)));
        l.committed(|| t(us!(370)));
        assert_eq!(l.presented(t(us!(380)), t(us!(385))), Ended::OnClock);
        // The early bound is the announced target; the late one needs the
        // next tick, so nothing is emitted at this one.
        assert!(l.settle(t(us!(8_333))).is_empty());
        assert_eq!(l.open().value(), 1);
        let lines = l.settle(t(us!(16_666)));
        assert_eq!(
            lines,
            vec![
                "keylat: seq=1 event=1000000000 handled=+300.0 written=+350.0 echo=+360.0 \
                 fed=+365.0 commit=+370.0 landed=+380.0 shown=+385.0 vsync=+8333.0 \
                 vsync_hi=+16666.0"
            ]
        );
        assert_eq!(l.open().value(), 0);
        assert_eq!(l.report().as_deref(), Some("keylat: 0 keys open"));
    }

    #[test]
    fn a_key_with_no_echo_expires_after_two_seconds_with_dashes() {
        let mut l = Latency::new(Switch::On);
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        assert!(l.settle(t(Duration::from_secs(1))).is_empty());
        assert_eq!(l.open().value(), 1);
        let lines = l.settle(t(Duration::from_millis(2_500)));
        assert_eq!(
            lines,
            vec![
                "keylat: seq=1 event=1000000000 handled=+100.0 written=+120.0 echo=- fed=- \
                 commit=- landed=- shown=- vsync=- vsync_hi=-"
            ]
        );
        assert_eq!(l.open().value(), 0);
    }

    #[test]
    fn two_keys_before_one_echo_both_get_the_echo_and_the_frame() {
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(1_000))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        l.key(|| Some(t(us!(10))), || t(us!(130)));
        l.written(|| t(us!(150)));
        l.echoed([t(us!(400))]);
        l.fed(|| t(us!(410)));
        l.committed(|| t(us!(420)));
        l.presented(t(us!(700)), t(us!(710)));
        let lines = l.settle(t(us!(9_333)));
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "keylat: seq=1 event=1000000000 handled=+100.0 written=+120.0 echo=+400.0 \
             fed=+410.0 commit=+420.0 landed=+700.0 shown=+710.0 vsync=+1000.0 \
             vsync_hi=+9333.0"
        );
        // The second key's offsets are from its own, later event; the
        // stamps it shares are the same absolute moments.
        assert_eq!(
            lines[1],
            "keylat: seq=2 event=1000010000 handled=+120.0 written=+140.0 echo=+390.0 \
             fed=+400.0 commit=+410.0 landed=+690.0 shown=+700.0 vsync=+990.0 \
             vsync_hi=+9323.0"
        );
    }

    #[test]
    fn the_cpu_path_completes_without_landing() {
        let mut l = Latency::new(Switch::On);
        l.key(|| None, || t(us!(100)));
        l.written(|| t(us!(120)));
        l.echoed([t(us!(300))]);
        l.fed(|| t(us!(310)));
        // In the app the tick announces, pumps -- which presents on this
        // path -- and then settles with the same target.
        l.announced(|| Some(t(us!(8_000))));
        l.shown_cpu(|| t(us!(500)));
        assert!(l.settle(t(us!(8_000))).is_empty());
        let lines = l.settle(t(us!(16_333)));
        // No event stamp: `event` is `handled`, so `handled` is +0.0.
        assert_eq!(
            lines,
            vec![
                "keylat: seq=1 event=1000100000 handled=+0.0 written=+20.0 echo=+200.0 \
                 fed=+210.0 commit=+400.0 landed=- shown=+400.0 vsync=+7900.0 \
                 vsync_hi=+16233.0"
            ]
        );
    }

    #[test]
    fn off_stores_nothing_and_never_reads_the_clock() {
        let mut l = Latency::new(Switch::Off);
        let never = || -> HostTime { panic!("the clock was read while off") };
        l.key(|| panic!("the event was read while off"), never);
        l.written(never);
        l.echoed([t(us!(1))]);
        l.fed(never);
        l.committed(never);
        l.unsent();
        assert_eq!(l.presented(t(us!(2)), t(us!(3))), Ended::OnClock);
        l.shown_cpu(never);
        l.announced(|| panic!("the target was converted while off"));
        l.tick(|| panic!("the target was converted while off"));
        assert_eq!(l.open().value(), 0);
        assert!(l.settle(t(Duration::from_secs(5))).is_empty());
        assert_eq!(l.report(), None);
        assert_eq!(l.on(), Switch::Off);
    }

    #[test]
    fn a_read_stamped_before_the_key_was_handled_is_not_its_echo() {
        // Output the child sent on its own, already in the channel.
        let mut l = Latency::new(Switch::On);
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(350)));
        l.echoed([t(us!(90))]);
        l.fed(|| t(us!(345)));
        let lines = l.settle(t(Duration::from_millis(2_500)));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains(" echo=- fed=- "), "{}", lines[0]);
        // A read the reader stamped after the key was handled but before
        // the writer's own post-write stamp *is* the echo: the line
        // discipline echoes inside the write and the reader can be first.
        let mut l = Latency::new(Switch::On);
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(350)));
        l.echoed([t(us!(90)), t(us!(340))]);
        l.fed(|| t(us!(365)));
        let lines = l.settle(t(Duration::from_millis(2_500)));
        assert!(
            lines[0].contains(" echo=+340.0 fed=+365.0 "),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_key_the_core_encodes_to_nothing_takes_no_echo() {
        let mut l = Latency::new(Switch::On);
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        // The reply was empty: nothing was written and none will be. The
        // next key's echo is not this record's, however late it is.
        l.unsent();
        l.echoed([t(us!(400))]);
        l.fed(|| t(us!(410)));
        let lines = l.settle(t(Duration::from_millis(2_500)));
        assert!(
            lines[0].contains(" written=- echo=- fed=- "),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_key_the_core_encodes_to_nothing_is_not_written_by_the_next_key() {
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(8_333))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.unsent();
        // The next key is written, echoed, drawn and shown.
        l.key(|| Some(t(us!(5_000))), || t(us!(5_100)));
        l.written(|| t(us!(5_130)));
        l.echoed([t(us!(5_150))]);
        l.fed(|| t(us!(5_160)));
        l.committed(|| t(us!(5_200)));
        l.presented(t(us!(5_500)), t(us!(5_600)));
        l.settle(t(us!(8_333)));
        let mut lines = l.settle(t(us!(16_666)));
        // The first key is still open: unsent, unechoed, and it expires.
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("keylat: seq=2 "), "{}", lines[0]);
        assert!(
            lines[0].contains(" written=+130.0 echo=+150.0 "),
            "{}",
            lines[0]
        );
        lines = l.settle(t(Duration::from_millis(2_500)));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].starts_with("keylat: seq=1 "), "{}", lines[0]);
        assert!(
            lines[0].contains(" written=- echo=- fed=- commit=- "),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_dropped_tick_voids_the_vsync_bounds() {
        let mut l = Latency::new(Switch::On);
        // Two regular ticks establish the period.
        l.announced(|| Some(t(us!(8_333))));
        l.announced(|| Some(t(us!(16_666))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        l.echoed([t(us!(300))]);
        l.fed(|| t(us!(310)));
        l.committed(|| t(us!(9_000)));
        // Shown after the announced target: the early bound waits for the
        // next tick -- which never comes; the one after is a gap.
        l.presented(t(us!(17_000)), t(us!(17_100)));
        l.announced(|| Some(t(us!(33_332))));
        let lines = l.settle(t(us!(33_332)));
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].ends_with(" shown=+17100.0 vsync=- vsync_hi=-"),
            "{}",
            lines[0]
        );
        // The period survives the gap: a regular tick after it is not one.
        l.key(|| Some(t(us!(40_000))), || t(us!(40_100)));
        l.written(|| t(us!(40_120)));
        l.echoed([t(us!(40_300))]);
        l.fed(|| t(us!(40_310)));
        l.committed(|| t(us!(40_400)));
        l.announced(|| Some(t(us!(41_665))));
        l.presented(t(us!(40_700)), t(us!(40_800)));
        l.settle(t(us!(41_665)));
        l.announced(|| Some(t(us!(49_998))));
        let lines = l.settle(t(us!(49_998)));
        assert!(
            lines[0].ends_with(" vsync=+1665.0 vsync_hi=+9998.0"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_frame_committed_after_the_gpu_ended_is_not_that_landing() {
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(8_333))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        l.echoed([t(us!(300))]);
        l.fed(|| t(us!(310)));
        l.committed(|| t(us!(500)));
        // The previous frame's presentation, whose GPU end precedes this
        // commit.
        assert_eq!(l.presented(t(us!(450)), t(us!(460))), Ended::OnClock);
        assert_eq!(l.open().value(), 1);
        assert!(l.settle(t(us!(470))).is_empty());
        // This frame's.
        l.presented(t(us!(800)), t(us!(810)));
        l.settle(t(us!(8_333)));
        let lines = l.settle(t(us!(16_666)));
        assert!(
            lines[0].contains(" commit=+500.0 landed=+800.0 shown=+810.0 vsync=+8333.0 "),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn a_landing_off_the_clock_is_refused_and_the_key_still_completes() {
        for ended in [
            // Metal's zero, or a conversion that refused.
            HostTime::ZERO,
            // After the surface was set: not this clock.
            t(us!(900)),
            // A second and more before it: not this frame.
            t(Duration::ZERO),
        ] {
            let mut l = Latency::new(Switch::On);
            l.announced(|| Some(t(us!(8_333))));
            l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
            l.written(|| t(us!(120)));
            l.echoed([t(us!(300))]);
            l.fed(|| t(us!(310)));
            l.committed(|| t(us!(500)));
            // The last case is 810 us after an `ended` at the event, which
            // is sane; push `shown` past the bound instead.
            let shown = if ended == t(Duration::ZERO) {
                t(Duration::from_millis(1_500))
            } else {
                t(us!(810))
            };
            assert_eq!(l.presented(ended, shown), Ended::Refused, "{ended:?}");
            // Two ticks: the early bound (already the announced target for
            // a surface set before it) and the late one.
            let mut lines = l.settle(t(Duration::from_millis(1_600)));
            lines.extend(l.settle(t(Duration::from_millis(1_700))));
            assert_eq!(lines.len(), 1, "{ended:?}");
            assert!(lines[0].contains(" landed=- shown=+"), "{}", lines[0]);
            assert!(!lines[0].contains(" vsync=- "), "{}", lines[0]);
        }
    }

    #[test]
    fn the_vsync_bounds_are_the_announced_target_and_the_one_after() {
        // Shown before the announced target: that target is the early
        // bound and the next tick's is the late one.
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(8_333))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        l.echoed([t(us!(300))]);
        l.fed(|| t(us!(310)));
        l.committed(|| t(us!(400)));
        l.presented(t(us!(700)), t(us!(5_000)));
        assert!(l.settle(t(us!(8_333))).is_empty());
        let lines = l.settle(t(us!(16_666)));
        assert!(
            lines[0].ends_with(" vsync=+8333.0 vsync_hi=+16666.0"),
            "{}",
            lines[0]
        );
        // Shown after a stale announcement -- ticks paused -- takes the
        // next tick's target as the early bound and the one after as late.
        let mut l = Latency::new(Switch::On);
        l.announced(|| Some(t(us!(4_000))));
        l.key(|| Some(t(Duration::ZERO)), || t(us!(100)));
        l.written(|| t(us!(120)));
        l.echoed([t(us!(300))]);
        l.fed(|| t(us!(310)));
        l.committed(|| t(us!(400)));
        l.presented(t(us!(700)), t(us!(5_000)));
        assert!(l.settle(t(us!(12_333))).is_empty());
        let lines = l.settle(t(us!(20_666)));
        assert!(
            lines[0].ends_with(" vsync=+12333.0 vsync_hi=+20666.0"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn the_queue_is_capped_and_the_evicted_record_is_still_printed() {
        let mut l = Latency::new(Switch::On);
        for _ in 0..super::CAP {
            l.key(|| Some(t(Duration::ZERO)), || t(us!(1)));
        }
        assert_eq!(l.open().value(), 256);
        l.key(|| Some(t(Duration::ZERO)), || t(us!(2)));
        assert_eq!(l.open().value(), 256);
        // Nothing completed: the one line is the evicted first record.
        let lines = l.settle(t(us!(3)));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("keylat: seq=1 "), "{}", lines[0]);
    }

    /// The driver's self-test fixture is parsed by `scripts/key-latency.py`
    /// and its first record has to be what `Record::line` emits, or the two
    /// halves of the instrument can drift apart while both suites stay
    /// green. Rebuilding the record from the fixture's own numbers and
    /// asking for its line is what holds them equal.
    #[test]
    fn the_fixture_s_first_record_is_what_the_line_emits() {
        let fixture = include_str!("../../tests/keylat/keylat-fixture.txt");
        let line = fixture
            .lines()
            .find(|l| l.starts_with("keylat: seq="))
            .expect("the fixture carries a record");
        let field = |name: &str| -> &str {
            let key = format!(" {name}=");
            let at = line
                .find(&key)
                .unwrap_or_else(|| panic!("{name} in {line}"))
                + key.len();
            line[at..].split(' ').next().expect("a value")
        };
        let event = HostTime::of_nanos(field("event").parse().expect("event ns"));
        let leg = |name: &str| -> Option<HostTime> {
            let v = field(name);
            if v == "-" {
                return None;
            }
            // `+123.4`: whole microseconds and one decimal, both integers,
            // so the nanoseconds are exact rather than a float's rounding.
            let (whole, tenth) = v
                .trim_start_matches('+')
                .split_once('.')
                .expect("one decimal");
            let micros: u64 = whole.parse().expect("microseconds");
            let tenths: u64 = tenth.parse().expect("a tenth");
            Some(event.plus(Duration::from_nanos(micros * 1000 + tenths * 100)))
        };
        let mut seq = Tally::default();
        for _ in 0..field("seq").parse::<usize>().expect("a seq") {
            seq.bump();
        }
        let handled = leg("handled").expect("handled is never -");
        let mut r = Record::open(seq, event, handled);
        r.written = leg("written").map_or(Written::Never, Written::At);
        r.echo = leg("echo");
        r.fed = leg("fed");
        r.committed = leg("commit");
        r.landed = leg("landed");
        r.shown = leg("shown");
        r.vsync = leg("vsync");
        r.vsync_hi = leg("vsync_hi");
        assert_eq!(r.line(), line);
    }
}
