//! **What a window owes its display link**: a tick at every vsync while a
//! frame or a tick is owed, a restart at the soonest deadline, or nothing.
//!
//! The decision is [`owes`], a pure function of what the turn left, so it
//! is tested as a table; the driver's `settle` reads it at the tail of
//! every pump, frame and responder call and pauses the link or schedules
//! its restart through the one door (`baud_present_macos::display_link`).
//! A frame owed starts the link where it is owed (the driver's
//! `owe_frame`), so a turn that never settles still draws.

use std::time::Duration;

use baud_domain::HostTime;

use crate::pty::Owed;

/// **A poll the tick still makes**, with no event to raise it yet: while
/// any is held the link runs, so the poll keeps its pace. Each is named
/// for the row that moves it to its event, and removing a variant is that
/// row's change. None is left: FS-A5, the notification center's news, was
/// the last, and a poll added to the tick again comes back as a variant
/// here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Holder {}

impl Holder {
    /// Every holder, in row order.
    pub(crate) const ALL: [Holder; 0] = [];

    /// The row's name, as the event rig prints it.
    #[cfg(any(feature = "effect-probe", test))]
    pub(crate) fn word(self) -> &'static str {
        match self {}
    }
}

/// Motion that owes every frame while it runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moving {
    /// A cursor trail's sweep, or a cursor transition.
    Trail,
    /// Smooth scroll's slide toward settled.
    Slide,
    /// The launch splash playing or fading.
    Splash,
    /// iTerm2's fireworks.
    Fireworks,
    /// The cursor quad in flight.
    Chase,
    /// The scrollbar's fade.
    Reveal,
}

/// A clock a timed effect keeps: when it changes what is drawn, the link
/// must tick again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Deadline {
    /// The next flip of a blink rate on screen (SGR 5 and 6, and a ReGIS
    /// page's blinking pixels with the slow rate).
    TextBlink,
    /// The cursor's next blink flip.
    Caret,
    /// A visual bell's end.
    Bell,
    /// A copy's words or pulse ending.
    Copied,
    /// The cursor quad's dwell running out.
    Chase,
    /// The scrollbar's hold before it fades.
    Reveal,
    /// The splash's reduced-motion hold.
    Splash,
    /// A kitty animation's next frame.
    Images,
    /// Predictions outstanding past their bound.
    Predictions,
    /// The indicator line's printer field, at its pace.
    Indicator,
    /// The five-second report, while the log shows it.
    Report,
    /// The activity lights' next step: a light to change at once, or a
    /// hold or dwell ending.
    Lights,
}

/// Why the link runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Why {
    /// A frame is owed and not yet drawn, a synchronised update holding it
    /// included (its cap bounds the hold).
    Owed,
    /// A frame was presented, and the tick after it is owed: the latency
    /// record takes that tick's target as its late bound.
    Presented,
    /// A paused link run again at a deadline, owed the tick that reaches
    /// it ([`Restart::Ticking`]).
    Restarted,
    /// Motion that owes every frame.
    Moving(Moving),
    /// A poll that is still the tick's.
    Held(Holder),
    /// **Bytes owed to the child that the kernel has not taken**: a write
    /// it took in part or not at all (`Wrote::Part`, `Wrote::Nothing`),
    /// its slave's input queue full. Each tick's pump writes again, so the
    /// rest goes once the child reads, and the link pauses once the outbox
    /// is empty. FS-B2's interim: a source that wakes the window when the
    /// master has room is the event this tick stands for, and a link a
    /// covered window has paused retries nothing until another turn.
    Unwritten,
    /// A deadline reached, or within a frame of now: the tick whose target
    /// reaches it is the next one.
    Due(Deadline),
}

/// **A paused link's restart**: none, scheduled for a deadline, or run at
/// it and owed the tick that draws what the deadline changed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Restart {
    Idle,
    /// Scheduled for the deadline, its wake that much early: the frame
    /// interval when it was scheduled, kept, since the interval may be
    /// measured again before the wake lands.
    At(HostTime, Duration),
    Ticking,
}

/// What a wake did to the restart it may have been scheduled for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Landed {
    /// The deadline is within the lead the wake was scheduled with: run the
    /// link, and owe the tick that reaches it.
    Run,
    /// Not this restart's wake, or none is scheduled: nothing.
    Nothing,
}

impl Restart {
    /// **A restart scheduled for `at`**, at `now`, a frame `interval`
    /// early, so the first tick the link delivers is the one whose target
    /// reaches it: how long until its wake, or `None` while an earlier
    /// restart still ahead stands, its own settle scheduling the later one.
    /// A restart already past is scheduled again, its wake having landed
    /// before it was within its lead.
    pub(crate) fn schedule(
        &mut self,
        at: HostTime,
        now: HostTime,
        interval: Duration,
    ) -> Option<Duration> {
        if let Restart::At(r, _) = *self {
            if r <= at && now < r {
                return None;
            }
        }
        *self = Restart::At(at, interval);
        Some(at.since(now).unwrap_or_default().saturating_sub(interval))
    }

    /// **A wake, at `now`**: the restart run once its deadline is within the
    /// lead it was scheduled with, and the tick owed until a frame has run
    /// ([`Restart::ticked`]), so a wake that lands after the deadline still
    /// draws what it changed.
    pub(crate) fn landed(&mut self, now: HostTime) -> Landed {
        match *self {
            Restart::At(at, early) if at <= now.plus(early) => {
                *self = Restart::Ticking;
                Landed::Run
            }
            Restart::Idle | Restart::At(..) | Restart::Ticking => Landed::Nothing,
        }
    }

    /// A frame ran: a restart's owed tick is paid.
    pub(crate) fn ticked(&mut self) {
        if *self == Restart::Ticking {
            *self = Restart::Idle;
        }
    }

    /// Whether a restart's tick is owed.
    pub(crate) fn owes_tick(self) -> bool {
        self == Restart::Ticking
    }
}

/// **The frame interval, measured between two ticks in a row**: the step
/// from the tick before when there was one in a row and the step is a
/// plausible frame, 1 ms to 100 ms. A pause forgets the tick before, so the
/// first tick after it measures nothing: the time paused is not a frame.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct TickClock {
    last: Option<f64>,
}

impl TickClock {
    /// No tick yet.
    pub(crate) const NEW: TickClock = TickClock { last: None };

    /// A tick due at `target`, the display link's seconds: the interval
    /// since the tick before it, if that was one in a row.
    pub(crate) fn tick(&mut self, target: f64) -> Option<Duration> {
        let step = self
            .last
            .map(|last| target - last)
            .filter(|dt| *dt > 0.001 && *dt < 0.1)
            .map(Duration::from_secs_f64);
        self.last = Some(target);
        step
    }

    /// The link paused: the next tick has no tick in a row before it.
    pub(crate) fn paused(&mut self) {
        self.last = None;
    }
}

/// What the link owes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Owes {
    /// Run: a tick at every vsync.
    Frame(Why),
    /// Pause, and run again a frame before this deadline.
    At(HostTime, Deadline),
    /// Pause.
    Nothing,
}

/// **What the link owes at `now`**: a frame when `frame` names why one is
/// owed; else a frame while the outbox holds bytes the kernel has not
/// taken (`owed`, [`Why::Unwritten`]); else the soonest deadline -- a frame
/// if it is reached or less than a frame `interval` away, since a restart
/// scheduled a frame early would land at once and find it still ahead,
/// else a restart at it; else nothing.
pub(crate) fn owes(
    frame: Option<Why>,
    owed: Owed,
    deadlines: &[(Deadline, Option<HostTime>)],
    now: HostTime,
    interval: Duration,
) -> Owes {
    if let Some(why) = frame {
        return Owes::Frame(why);
    }
    if owed == Owed::Bytes {
        return Owes::Frame(Why::Unwritten);
    }
    let soonest = deadlines
        .iter()
        .filter_map(|&(d, t)| t.map(|t| (t, d)))
        .min_by_key(|&(t, _)| t);
    match soonest {
        None => Owes::Nothing,
        Some((t, d)) if t <= now.plus(interval) => Owes::Frame(Why::Due(d)),
        Some((t, d)) => Owes::At(t, d),
    }
}

/// **What a settle does with the link**, from what it owes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Act {
    /// Run it: a frame is owed, whoever owes it. The door's run is
    /// idempotent, so a link already running is sent nothing, and a frame
    /// owed by no one who runs the link -- a deadline within one interval,
    /// a latency record open -- is not left paused for good.
    Run,
    /// Pause it, and run it again a frame before this deadline.
    PauseUntil(HostTime),
    /// Pause it.
    Pause,
}

/// What a settle does when the link owes `owes`.
pub(crate) fn act(owes: Owes) -> Act {
    match owes {
        Owes::Frame(_) => Act::Run,
        Owes::At(at, _) => Act::PauseUntil(at),
        Owes::Nothing => Act::Pause,
    }
}

/// **The effects' clock**: the later of the host's time now and the
/// display link's last target. On a tick the target, the vsync the frame
/// is for, is the later; between ticks, and after a pause above all, the
/// target is stale and the host's time is now. What a timed effect starts
/// -- a reveal's hold, a transition, a trail -- starts at this, so it does
/// not start in the past and end at once.
pub(crate) fn effects_now(host: HostTime, target: Option<HostTime>) -> HostTime {
    target.map_or(host, |t| t.max(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(8);

    /// A case: why a frame is owed, the deadlines, and what is owed.
    type Case<'a> = (Option<Why>, &'a [(Deadline, Option<HostTime>)], Owes);

    /// **The table**: a frame owed runs the link whatever the deadlines;
    /// with none owed the soonest deadline decides -- a frame once it is
    /// within one interval, a restart at it beyond -- and with neither the
    /// link pauses. Breaks: `owes` ignoring the interval, and a deadline
    /// 4 ms ahead schedules a restart that lands at once; `owes` taking the
    /// latest deadline, and the caret's flip waits for the report. Every
    /// reason a frame is owed is in it but [`Why::Held`], which no holder
    /// is left to make, and [`Why::Unwritten`], which the outbox makes
    /// (`bytes_the_kernel_did_not_take_keep_the_link_ticking`); a deadline
    /// exactly one interval ahead is a frame: break `<=` to `<` and it is
    /// restarted at once.
    #[test]
    fn what_the_link_owes() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let now = ms(1_000);
        let cases: [Case<'_>; 12] = [
            (None, &[], Owes::Nothing),
            (None, &[(Deadline::Caret, None)], Owes::Nothing),
            (
                Some(Why::Owed),
                &[(Deadline::Caret, Some(ms(1_300)))],
                Owes::Frame(Why::Owed),
            ),
            (
                None,
                &[(Deadline::Caret, Some(ms(1_333)))],
                Owes::At(ms(1_333), Deadline::Caret),
            ),
            (
                None,
                &[
                    (Deadline::Report, Some(ms(5_000))),
                    (Deadline::Caret, Some(ms(1_333))),
                    (Deadline::Bell, None),
                ],
                Owes::At(ms(1_333), Deadline::Caret),
            ),
            (
                None,
                &[(Deadline::Bell, Some(ms(1_004)))],
                Owes::Frame(Why::Due(Deadline::Bell)),
            ),
            (
                None,
                &[(Deadline::Copied, Some(ms(900)))],
                Owes::Frame(Why::Due(Deadline::Copied)),
            ),
            (Some(Why::Presented), &[], Owes::Frame(Why::Presented)),
            (
                Some(Why::Restarted),
                &[(Deadline::Caret, Some(ms(1_300)))],
                Owes::Frame(Why::Restarted),
            ),
            (
                Some(Why::Moving(Moving::Slide)),
                &[],
                Owes::Frame(Why::Moving(Moving::Slide)),
            ),
            (
                None,
                &[(Deadline::Bell, Some(ms(1_008)))],
                Owes::Frame(Why::Due(Deadline::Bell)),
            ),
            (
                None,
                &[(Deadline::Bell, Some(ms(1_009)))],
                Owes::At(ms(1_009), Deadline::Bell),
            ),
        ];
        for (frame, deadlines, want) in cases {
            assert_eq!(
                owes(frame, Owed::Nothing, deadlines, now, FRAME),
                want,
                "{frame:?} {deadlines:?}"
            );
        }
    }

    /// **A restart's wake lands by the lead it was scheduled with**, the
    /// interval measured since being another: scheduled at an 8 ms interval
    /// for a deadline 100 ms ahead, the wake comes at 92 ms, and the link
    /// runs -- `landed` takes no interval, so one measured since cannot
    /// reach it. The row that found it: the
    /// caret's, where a wake that read the interval measured since found
    /// the deadline not yet within it, did nothing, and left the link
    /// paused for good. Break: `landed` comparing with no lead (the
    /// interval it read gone), and the 92 ms wake runs nothing.
    #[test]
    fn a_restarts_wake_lands_by_the_lead_it_was_scheduled_with() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let mut r = Restart::Idle;
        let wait = r.schedule(ms(100), ms(0), FRAME);
        assert_eq!(wait, Some(Duration::from_millis(92)));
        assert_eq!(r.landed(ms(92)), Landed::Run);
        assert!(r.owes_tick(), "the tick owed");
        // The control: a wake before the lead is not this restart's.
        let mut r = Restart::Idle;
        let _ = r.schedule(ms(100), ms(0), FRAME);
        assert_eq!(r.landed(ms(50)), Landed::Nothing, "too early");
        assert!(!r.owes_tick());
    }

    /// **A wake that lands after its deadline still owes the tick**, so the
    /// flip the deadline made is drawn before the link pauses again: the
    /// restart holds the link ([`Restart::owes_tick`]) until a frame runs.
    /// Break: `landed` going straight back to idle, and the settle after the
    /// late wake pauses with the flip undrawn.
    #[test]
    fn a_late_wake_still_owes_the_tick_that_draws_its_deadline() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let mut r = Restart::Idle;
        let _ = r.schedule(ms(100), ms(0), FRAME);
        assert_eq!(r.landed(ms(150)), Landed::Run, "late, and run");
        assert!(r.owes_tick(), "the tick owed until a frame runs");
        assert_eq!(r.landed(ms(151)), Landed::Nothing, "run once");
        r.ticked();
        assert!(!r.owes_tick(), "paid by the frame");
    }

    /// **One restart scheduled at a time, the soonest**: a later deadline
    /// waits behind one still ahead; an earlier one takes its place; one
    /// already past is scheduled again.
    #[test]
    fn the_soonest_restart_is_the_one_scheduled() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let mut r = Restart::Idle;
        assert!(r.schedule(ms(100), ms(0), FRAME).is_some());
        assert_eq!(r.schedule(ms(200), ms(10), FRAME), None, "later waits");
        assert!(
            r.schedule(ms(50), ms(10), FRAME).is_some(),
            "earlier takes it"
        );
        assert_eq!(r, Restart::At(ms(50), FRAME));
        assert!(r.schedule(ms(300), ms(60), FRAME).is_some(), "past: again");
    }

    /// **A pause forgets the tick clock**: two ticks 8 ms apart measure
    /// 8 ms; after a pause the first tick, 42 ms on, measures nothing, and
    /// the next, 8 ms after it, measures 8 ms again. Break: `paused` keeping
    /// the tick before, and the time paused is read as a 42 ms frame.
    #[test]
    fn a_pause_forgets_the_tick_clock() {
        let mut c = TickClock::NEW;
        assert_eq!(c.tick(1.000), None, "no tick before the first");
        assert_eq!(c.tick(1.008), Some(Duration::from_secs_f64(0.008)));
        c.paused();
        assert_eq!(c.tick(1.050), None, "the pause is not a frame");
        let next = c.tick(1.058).expect("a tick in a row");
        assert!((next.as_secs_f64() - 0.008).abs() < 1e-9, "{next:?}");
        // The control: without the pause the gap is read, being a step under
        // 100 ms.
        let mut c = TickClock::NEW;
        let _ = c.tick(1.008);
        assert!(c.tick(1.050).is_some());
    }

    /// Every holder has its own row's name, and `ALL` lists each once.
    #[test]
    fn each_holder_names_its_own_row() {
        let words: Vec<&str> = Holder::ALL.iter().map(|h| h.word()).collect();
        let mut unique = words.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), Holder::ALL.len(), "{words:?}");
        assert!(words.iter().all(|w| w.starts_with("FS-A")));
    }

    /// **A settle runs the link for a frame no one who runs it owes**: a
    /// deadline within one interval of a paused settle -- a late printer
    /// read under the indicator line -- and a latency record open, whose
    /// expiry only a tick reads, each run it; a deadline beyond pauses it
    /// until a frame before, and nothing pauses it. Break: the settle's
    /// frame arm pausing, and both stall with nothing to tick again.
    #[test]
    fn a_settle_runs_the_link_for_a_frame_no_one_owes() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let now = ms(1_000);
        let due = owes(
            None,
            Owed::Nothing,
            &[(Deadline::Indicator, Some(ms(1_004)))],
            now,
            FRAME,
        );
        assert_eq!(due, Owes::Frame(Why::Due(Deadline::Indicator)));
        assert_eq!(act(due), Act::Run, "a deadline within one interval");
        let open = owes(Some(Why::Presented), Owed::Nothing, &[], now, FRAME);
        assert_eq!(act(open), Act::Run, "a latency record open");
        // The controls.
        let later = owes(
            None,
            Owed::Nothing,
            &[(Deadline::Caret, Some(ms(1_333)))],
            now,
            FRAME,
        );
        assert_eq!(act(later), Act::PauseUntil(ms(1_333)));
        assert_eq!(act(owes(None, Owed::Nothing, &[], now, FRAME)), Act::Pause);
    }

    /// **Bytes the kernel did not take keep the link ticking** (FS-B2's
    /// interim): a reply written in part, its rest still in the outbox,
    /// owes a frame with no other reason and ahead of a deadline, so each
    /// tick's pump writes again until the child has read enough; once the
    /// outbox is empty the link pauses. Since the link paused when nothing
    /// owed a frame (32c9ad57), the rest waited for the next turn of any
    /// kind, and a child blocked reading it, on a still window, waited for
    /// ever. The driver hands `owes` the outbox's own reading, and lets the
    /// outbox go once the child's stream has closed, so a window that
    /// outlives its child does not tick for bytes no one can take. Breaks:
    /// `owes` ignoring `owed`, and the rest owes nothing; the driver passing
    /// `Owed::Nothing`, or the closed stream keeping the outbox, and the
    /// source checks fail.
    #[test]
    fn bytes_the_kernel_did_not_take_keep_the_link_ticking() {
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let now = ms(1_000);
        let caret = [(Deadline::Caret, Some(ms(1_333)))];
        let owed = owes(None, Owed::Bytes, &[], now, FRAME);
        assert_eq!(owed, Owes::Frame(Why::Unwritten));
        assert_eq!(act(owed), Act::Run, "the tick that writes again");
        assert_eq!(
            owes(None, Owed::Bytes, &caret, now, FRAME),
            Owes::Frame(Why::Unwritten),
            "ahead of a deadline"
        );
        assert_eq!(
            owes(Some(Why::Owed), Owed::Bytes, &caret, now, FRAME),
            Owes::Frame(Why::Owed),
            "a frame owed names its own reason"
        );
        // The controls: the outbox empty, the link pauses or waits for the
        // deadline.
        assert_eq!(owes(None, Owed::Nothing, &[], now, FRAME), Owes::Nothing);
        assert_eq!(
            owes(None, Owed::Nothing, &caret, now, FRAME),
            Owes::At(ms(1_333), Deadline::Caret)
        );
        // The outbox's own reading: bytes pushed are owed, and let go are not.
        let mut outbox = crate::pty::Outbox::default();
        assert_eq!(outbox.owed(), Owed::Nothing);
        outbox.push(b"\x1b[?1;2c");
        assert_eq!(outbox.owed(), Owed::Bytes);
        outbox.let_go();
        assert_eq!(outbox.owed(), Owed::Nothing);
        // The driver's reading, and the closed stream's.
        const APP: &str = include_str!("app.rs");
        let driver = APP
            .split_once("    fn owes(&self, now: HostTime) -> crate::frames::Owes {")
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .expect("Driver::owes")
            .0;
        assert!(
            driver.contains(
                "crate::frames::owes(frame, app.outbox.owed(), &deadlines, now, app.interval)"
            ),
            "{driver}"
        );
        let closed = APP
            .split_once("    fn pump(&mut self, trigger: Trigger) {")
            .and_then(|(_, pump)| pump.split_once("if drained.stream == Stream::Closed {"))
            .and_then(|(_, arm)| arm.split_once("\n        }\n"))
            .expect("the closed stream's arm")
            .0;
        assert!(closed.contains("self.app.outbox.let_go();"), "{closed}");
    }

    /// **A reveal after a paused gap holds for its whole hold**: the link
    /// paused for ten seconds, its last target that old, and a wheel scroll
    /// reveals the thumb at the effects' clock, which is the host's time,
    /// so it holds `HOLD` from now and is seen; on a tick, the target ahead
    /// of the host's time is the clock. Break: the clock being the target
    /// alone, and the reveal's hold ended ten seconds ago -- the thumb
    /// never shows.
    #[test]
    fn a_reveal_after_a_paused_gap_holds_for_its_whole_hold() {
        use crate::reveal::{HOLD, Pending, Reveal};
        let ms = |n| HostTime::ZERO.plus(Duration::from_millis(n));
        let now = ms(20_000);
        let stale = Some(ms(10_000));
        let mut reveal = Reveal::HIDDEN;
        reveal.reveal(effects_now(now, stale));
        assert_eq!(reveal.pending(), Pending::At(now.plus(HOLD)));
        reveal.step(now);
        assert_eq!(reveal.pending(), Pending::At(now.plus(HOLD)), "held");
        // On a tick the target, a vsync ahead, is the clock.
        let ahead = Some(ms(20_008));
        assert_eq!(effects_now(now, ahead), ms(20_008));
    }

    /// **`Holder::ALL` is every holder, each once**: the match below has
    /// no `_` arm, so a holder added and left out of `ALL` does not
    /// compile until it has a place here, and `ALL` must hold each place.
    /// With no holder left the match has no arm, and `ALL` holds none.
    #[test]
    fn every_holder_is_in_all() {
        /// A holder's place.
        #[derive(PartialEq, Eq, Debug, PartialOrd, Ord, Clone, Copy)]
        struct Place(usize);
        fn place(h: Holder) -> Place {
            match h {}
        }
        let places: Vec<Place> = Holder::ALL.iter().map(|&h| place(h)).collect();
        let want: Vec<Place> = (0..0).map(Place).collect();
        assert_eq!(places, want);
    }

    /// **The event rig's `HOLDERS` is every holder's name**, in `ALL`'s
    /// order: the frame rows read their holder lists against it, so a name
    /// it leaves out is a poll no row can see. Read from the test's source,
    /// which is another binary. Break: `FS-A5` left out of `HOLDERS`.
    #[test]
    fn the_rigs_holders_are_every_holder() {
        const RIG: &str = include_str!("../tests/event_rig.rs");
        let head = "const HOLDERS: &[&str] = &[";
        let at = RIG.find(head).expect("HOLDERS") + head.len();
        let list = &RIG[at..at + RIG[at..].find("];").expect("its end")];
        let named: Vec<&str> = list
            .split(',')
            .map(|w| w.trim().trim_matches('"'))
            .filter(|w| !w.is_empty())
            .collect();
        let words: Vec<&str> = Holder::ALL.iter().map(|h| h.word()).collect();
        assert_eq!(named, words);
    }
}
