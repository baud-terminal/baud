//! The child's reads into the core, and the core's answers out behind them.
//! Pure -- reads in, a `Term` and a sink, bytes out -- because both drivers'
//! loops are platform-bound and no test reaches them, the shape `wheel.rs`
//! took for the wheel.

use baud_core::{BytePos, PrinterAsked, ReadLen, Rest, Term};
use baud_domain::RegisInput;
use baud_domain::{DecodeBudget, DecodeProgress};
use std::time::{Duration, Instant};

/// [`paced`] with no delay to hold: what a test feeds, its terminal's
/// `RegisHold` off.
#[cfg(test)]
pub fn feed<'r>(
    term: &mut Term<'_>,
    reads: impl IntoIterator<Item = &'r [u8]>,
    sink: impl FnMut(&[u8]),
    after: impl FnMut(&mut Term<'_>),
    log: &mut impl crate::hostlog::Tee,
) {
    let mut pace = Pace::new();
    let _ = paced(term, &mut pace, Instant::now(), reads, sink, after, log);
    assert!(
        pace.deadline().is_none(),
        "a delay held: feed it with `paced`"
    );
}

/// **The reads a ReGIS delay holds back, and when they go on.** An `S(T)`
/// suspends interpretation for its ticks, "measured from the time the
/// S(T<n>) is interpreted by the device" (`[[source: dec-std-070]]` 070-8
/// § 8.7.1.4.3), and under `RegisHold::On` the core stops the read there
/// (`Term::take_held`, `Term::take_regis_wait`). What is left of the read
/// and every read after it wait here, in order, until the deadline, which
/// the driver's loop schedules -- the display link's tick on macOS, the
/// `poll` timeout on Linux -- and nothing sleeps.
///
/// **Bounded at [`HELD_MAX`]**: a program that keeps writing through a
/// long delay ends it early once that much waits, counted in `cut`.
///
/// **And a one-shot ReGIS input** (`Term::regis_input`): "no further data
/// from the application (screen output) is processed until the locator
/// input is complete" (`[[source: dec-std-070]]` 070-13 § 5.1). The reads
/// wait with no deadline until a key or a button ends the input, and are
/// fed by the next call after it, which [`Pace::released`] tells a driver
/// to make at once. Past [`HELD_MAX`] the input is given up with the empty
/// report (`Term::regis_input_abandon`), counted in the core.
///
/// **And the reads behind a DSR `?15` the core holds** (`Term::printer_asked`,
/// under `PrinterHold::On`): they wait until the driver, which reads its
/// printer list off the main thread, answers it through `Term::set_printer`,
/// or at [`PRINTER_WAIT`] are answered from the printer it last had.
pub struct Pace {
    held: std::collections::VecDeque<Vec<u8>>,
    until: Option<Instant>,
    input: Input,
    why: Why,
    /// Delays ended early by [`HELD_MAX`].
    pub cut: DelaysCut,
}

/// Whether the held reads wait on a one-shot ReGIS input.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Input {
    Free,
    Waits,
}

/// What holds the reads while [`Pace`] has a deadline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Why {
    Delay,
    Printer,
}

/// How long a held DSR `?15` waits for the driver's look at its printers
/// before it is answered from the last one: `printerNames` took 0.3-0.8
/// ms a call off a process's first, 3.5-8.6 ms on its first with the
/// print system's daemon running, and 40-78 ms, inside this wait, when
/// that call had to start the daemon.
pub const PRINTER_WAIT: Duration = Duration::from_millis(100);

/// How many delays [`HELD_MAX`] ended early.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DelaysCut(usize);

impl DelaysCut {
    pub const NONE: DelaysCut = DelaysCut(0);
}

/// The bytes a delay may hold before it ends early: four mebibytes, 1,024
/// reads of `READ_CHUNK`.
pub const HELD_MAX: usize = 4 << 20;

impl Pace {
    pub fn new() -> Pace {
        Pace {
            held: std::collections::VecDeque::new(),
            until: None,
            input: Input::Free,
            why: Why::Delay,
            cut: DelaysCut::NONE,
        }
    }

    /// Whether the reads wait for the driver's printer.
    #[cfg(test)]
    pub fn waits_for_printer(&self) -> bool {
        self.until.is_some() && self.why == Why::Printer
    }

    /// When the held reads go on, while a delay holds them.
    #[cfg(any(target_os = "linux", test))]
    pub fn deadline(&self) -> Option<Instant> {
        self.until
    }

    /// Whether a delay's held reads are owed a feed at `now`.
    #[cfg(any(target_os = "linux", test))]
    pub fn due(&self, now: Instant) -> bool {
        self.until.is_some_and(|t| t <= now)
    }

    /// **Whether a one-shot input the reads waited on has ended**, so they
    /// are owed a feed now.
    #[cfg(any(target_os = "linux", test))]
    pub fn released(&self, term: &Term<'_>) -> bool {
        self.input == Input::Waits && term.regis_input() != RegisInput::OneShot
    }

    fn keep(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            self.held.push_back(bytes.to_vec());
        }
    }
}

/// Whether [`paced`] fed the core anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fed {
    Nothing,
    Some,
}

impl Fed {
    fn or(self, other: Fed) -> Fed {
        match (self, other) {
            (Fed::Nothing, Fed::Nothing) => Fed::Nothing,
            _ => Fed::Some,
        }
    }
}

/// Feed each read to the core, in order, and hand `sink` every byte it
/// answered, behind whatever it already owed.
///
/// **The reply buffer is drained after every read**, not once after the
/// batch, and once before the first, so answers an earlier writer left owed
/// do not share it with the first read's. The macOS pump fed every read the
/// reader thread had queued and then drained once, so the answers of all
/// but the first reads were dropped: a burst of `OSC 4` palette queries,
/// one to each 600 bytes of drawing, had 10 of 256 answered. **And inside
/// a read, wherever the core stops it for its answers to go out**
/// ([`hold_replies`]), so one read that asks more than the buffer holds is
/// answered whole too. `sink` is the outbox's `push`, which makes no
/// system call, so draining a part at a time writes no more often than
/// draining a batch did.
///
/// **`after` sees the core after each read**, for what is an event rather
/// than state: an `OSC 52` write (`clip.rs`). The core keeps only the last
/// write it finished, so a batch whose reads each finish one would hand a
/// poll after the batch only the last; a title is state and is read once
/// after the batch, beside this.
///
/// **And the rest of a read a page-width request held** (`Term::take_held`,
/// under `WidthHold::On`): `after` sees the core at the request, resizes it
/// if it honours one, and the rest of the read is fed after -- so a program
/// that asks for 132 columns and draws in the same read draws at 132.
///
/// **And a ReGIS delay** (`Term::take_regis_wait`, under
/// `RegisHold::On`): the reads it holds wait in `pace` until its deadline,
/// and are fed first, in order, by the first call at or after it. `now` is
/// the call's time, which a delay met here is measured from.
///
/// **And a DSR `?15` under `PrinterHold::On`**: the core stops the read
/// there with the answer owed, `after` asks the driver's printer list off
/// the main thread, and the rest of the read and every read after it wait
/// in `pace` until the driver answers through `Term::set_printer` or
/// [`PRINTER_WAIT`] passes; the answer goes to `sink` ahead of them.
///
/// **And a program's log** (`hostlog.rs`): the core stops a read at each
/// change of `?46` too, so each part is handed to `log` if the core was
/// logging when it was fed, and `log` follows the core after it -- the
/// bytes from the one after `?46 h` up to the end of `?46 l`, as xterm
/// logs them. A part a delay holds is logged when it is fed.
pub fn paced<'r>(
    term: &mut Term<'_>,
    pace: &mut Pace,
    now: Instant,
    reads: impl IntoIterator<Item = &'r [u8]>,
    mut sink: impl FnMut(&[u8]),
    mut after: impl FnMut(&mut Term<'_>),
    log: &mut impl crate::hostlog::Tee,
) -> Fed {
    printer_wait(term, pace, now);
    owed(term, &mut sink);
    log.follow(term);
    if pace.input == Input::Waits {
        let waiting: usize = pace.held.iter().map(Vec::len).sum();
        if term.regis_input() == RegisInput::OneShot {
            if waiting <= HELD_MAX {
                for bytes in reads {
                    pace.keep(bytes);
                }
                return Fed::Nothing;
            }
            term.regis_input_abandon();
            owed(term, &mut sink);
        }
        pace.input = Input::Free;
    }
    if let Some(t) = pace.until {
        let waiting: usize = pace.held.iter().map(Vec::len).sum();
        if now < t && waiting <= HELD_MAX {
            for bytes in reads {
                pace.keep(bytes);
            }
            return Fed::Nothing;
        }
        if now < t {
            pace.cut = DelaysCut(pace.cut.0 + 1);
        }
        pace.until = None;
    }
    let held = core::mem::take(&mut pace.held);
    let mut fed = Fed::Nothing;
    for bytes in held.iter() {
        fed = paced_one(term, pace, now, bytes, &mut sink, &mut after, log).or(fed);
    }
    for bytes in reads {
        fed = paced_one(term, pace, now, bytes, &mut sink, &mut after, log).or(fed);
    }
    fed
}

/// **A held DSR `?15`'s wait ends**: answered by the driver, or past
/// [`PRINTER_WAIT`] or [`HELD_MAX`] answered here from the printer the core
/// last had, so a driver whose look never lands holds nothing for long.
fn printer_wait(term: &mut Term<'_>, pace: &mut Pace, now: Instant) {
    let Some(t) = pace.until.filter(|_| pace.why == Why::Printer) else {
        return;
    };
    let waiting: usize = pace.held.iter().map(Vec::len).sum();
    if term.printer_asked() == PrinterAsked::Owed && (now >= t || waiting > HELD_MAX) {
        let last = term.printer();
        term.set_printer(last);
    }
    if term.printer_asked() == PrinterAsked::No {
        pace.until = None;
    }
}

/// One read for [`paced`]: held behind a delay this call met, or fed, and
/// what is left of it held if it meets one.
fn paced_one(
    term: &mut Term<'_>,
    pace: &mut Pace,
    now: Instant,
    bytes: &[u8],
    sink: &mut impl FnMut(&[u8]),
    after: &mut impl FnMut(&mut Term<'_>),
    log: &mut impl crate::hostlog::Tee,
) -> Fed {
    if bytes.is_empty() {
        return Fed::Nothing;
    }
    // A delay or a one-shot input met in this call holds the rest of the
    // batch behind it.
    if pace.until.is_some() || pace.input == Input::Waits {
        pace.keep(bytes);
        return Fed::Nothing;
    }
    term.write_at(BytePos::ZERO, bytes);
    let mut next = Some(Rest {
        at: BytePos::ZERO,
        len: ReadLen::from_index(bytes.len()),
    });
    while let Some(r) = next {
        let logging = term.log_mode();
        term.feed(r.at, r.len);
        owed(term, sink);
        next = term.take_held();
        if logging == baud_core::LogMode::Started {
            let end = next.map_or(r.at.index() + r.len.index(), |n| n.at.index());
            log.take(&bytes[r.at.index()..end]);
        }
        log.follow(term);
        after(term);
        if let Some(w) = term.take_regis_wait() {
            pace.until = Some(now + Duration::from_micros(w.micros()));
            pace.why = Why::Delay;
            if let Some(r) = next.take() {
                pace.keep(&bytes[r.at.index()..r.at.index() + r.len.index()]);
            }
        }
        // `after` saw the question and asked the driver's printer list.
        if term.printer_asked() == PrinterAsked::Owed {
            pace.until = Some(now + PRINTER_WAIT);
            pace.why = Why::Printer;
            if let Some(r) = next.take() {
                pace.keep(&bytes[r.at.index()..r.at.index() + r.len.index()]);
            }
        } else if pace.until.is_none() && term.regis_input() == RegisInput::OneShot {
            // A delay taken in this feed holds the reads first.
            pace.input = Input::Waits;
            if let Some(r) = next.take() {
                pace.keep(&bytes[r.at.index()..r.at.index() + r.len.index()]);
            }
        }
    }
    Fed::Some
}

/// **A read stops for its answers to go out** (`Term::set_reply_hold`),
/// and [`paced`] sends them and feeds the rest: one read of questions that
/// owes more than the core's reply buffer holds -- herdr's palette, a
/// DECRQSS for each seven bytes -- is answered whole and in order, where
/// the answers past the buffer were dropped. Both drivers set it.
pub fn hold_replies(term: &mut Term<'_>) {
    term.set_reply_hold(baud_core::ReplyHold::On);
}

/// One input event's answer -- a key, a focus report -- queued into an empty
/// buffer, and handed to `sink` behind everything the core already owed.
/// Returns what `queue` said: whether the event put anything in.
///
/// **Drained first, so the event always fits.** A key is at most
/// `KeyBytes::MAX_LEN` = 48 bytes and a report `Report::MAX_LEN` = 29, both
/// under `REPLY` (asserted below), so into an empty buffer neither can
/// overflow, and a `false` is only an event that sends nothing: a key
/// ctlseqs gives no encoding, a repeat DECARM refuses, a focus change the
/// application did not ask for. That keeps every key without a retry, which
/// could not tell those apart from a full buffer -- `Term::key` answers
/// `false` for all of them. The Linux loop queued keys behind a read's
/// answers and drained once after both, and behind 252 bytes of them a
/// six-byte key was dropped.
pub fn input(
    term: &mut Term<'_>,
    queue: impl FnOnce(&mut Term<'_>) -> bool,
    mut sink: impl FnMut(&[u8]),
) -> bool {
    owed(term, &mut sink);
    let queued = queue(term);
    owed(term, &mut sink);
    queued
}

const _: () = assert!(baud_domain::KeyBytes::MAX_LEN <= baud_core::REPLY);
const _: () = assert!(baud_domain::Report::MAX_LEN <= baud_core::REPLY);

/// The core's reply buffer to `sink`, and emptied.
pub fn owed(term: &mut Term<'_>, sink: &mut impl FnMut(&[u8])) {
    if !term.reply().is_empty() {
        sink(term.reply());
        term.clear_reply();
    }
}

/// How much of a deferred kitty decode one turn of a driver's loop does: a
/// MiB of compressed input fed or of scanlines made. mosaic-blue's
/// 1920x1080 decode is 31 ms over 60 steps of a quarter of this
/// (`baud-bench`'s `kitty_image`), so a turn spends about 2 ms of a 120 Hz
/// frame's 8.3.
pub const KITTY_DECODE_STEP: DecodeBudget = DecodeBudget::MIB;

/// Whether a step changed what the images show: a picture completed, or a
/// failed one freed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shown {
    Same,
    Changed,
}

/// **One step of a deferred kitty decode** (note V, V19 step 3: deferred
/// inside the core, in bounded steps): a picture is placed when its size
/// is known and its pixels are made here, [`KITTY_DECODE_STEP`] a call.
/// The macOS loop calls it on each display-link tick and the Linux loop on
/// each turn, between reads; the reply a decode owes is in the core's reply
/// buffer for the caller's flush. A command that arrives first finishes the
/// decode itself, so a late step only shows it late.
pub fn decode_step(term: &mut Term<'_>) -> (DecodeProgress, Shown) {
    let epoch = term.images().generation();
    let progress = term.decode_step(KITTY_DECODE_STEP);
    match term.images().generation() == epoch {
        true => (progress, Shown::Same),
        false => (progress, Shown::Changed),
    }
}

/// **How long a loop that waits on descriptors may block while a decode is
/// pending: not at all.** The next turn is scheduled at once, so the step
/// runs between reads rather than after the next one, and nothing sleeps;
/// with nothing pending the wait is the loop's own. The macOS loop waits on
/// no descriptor: its display link is the turn.
#[cfg(any(target_os = "linux", test))]
pub fn decode_wait(progress: DecodeProgress) -> Option<std::time::Duration> {
    match progress {
        DecodeProgress::More => Some(std::time::Duration::ZERO),
        DecodeProgress::Idle => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, READ_CHUNK, REPLY, WinSize};
    use std::time::Duration;

    /// A log that keeps each part it is handed, and the core's mode each
    /// time it was told to follow it.
    #[derive(Default)]
    struct Parts {
        parts: Vec<Vec<u8>>,
        follows: Vec<baud_core::LogMode>,
    }

    impl crate::hostlog::Tee for Parts {
        fn take(&mut self, part: &[u8]) {
            self.parts.push(part.to_vec());
        }
        fn follow(&mut self, t: &mut Term<'_>) {
            self.follows.push(t.log_mode());
        }
    }

    /// **Each part of a read goes to the log by the mode it was fed
    /// under**: the core stops the read at each `?46` change, so the part
    /// ending in `?46 h` is not the log's and the one ending in `?46 l` is,
    /// and the log follows the core before the batch and after every part.
    #[test]
    fn each_part_goes_to_the_log_by_the_mode_it_was_fed_under() {
        let ws = WinSize {
            ws_row: 4,
            ws_col: 20,
        };
        let mut s = Owned::new(&ws);
        let mut t = Term::new(s.storage(), &ws);
        t.set_logging(baud_core::Logging::On);
        let mut log = Parts::default();
        feed(
            &mut t,
            [&b"a\x1b[?46hb\x1b[?46lc"[..], b"d"],
            |_| {},
            |_| {},
            &mut log,
        );
        assert_eq!(log.parts, [b"b\x1b[?46l".to_vec()]);
        use baud_core::LogMode::{Started, Stopped};
        assert_eq!(
            log.follows,
            [Stopped, Started, Stopped, Stopped, Stopped],
            "before the batch, and after each of the four parts"
        );
    }

    const WS: WinSize = WinSize {
        ws_row: 40,
        ws_col: 120,
    };

    /// **Every palette query is answered, in order, however many reads the
    /// pump takes at once.** herdr 0.9.3 asks `OSC 4 ; n ; ?` for every `n`
    /// at startup and after a resize. Here each query sits in 600 bytes of
    /// drawing, so no read of `READ_CHUNK` holds more answers than the
    /// reply buffer -- the test asserts that -- and a loss can only
    /// be the pump's. The macOS pump fed every queued read and drained once,
    /// so the answers to every read but the first few were dropped.
    #[test]
    fn a_burst_of_reads_has_every_palette_query_answered_in_order() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let table: [(u8, u8, u8); 256] = core::array::from_fn(|i| {
            let i = u8::try_from(i).expect("256 entries");
            (i, 255 - i, i ^ 0x5a)
        });
        t.set_default_palette(&table);
        // Even indices end in `BEL`, odd in `ST`: the answer carries the
        // query's own terminator.
        let (mut stream, mut want, mut ends) = (Vec::new(), Vec::new(), Vec::new());
        for (n, &(r, g, b)) in table.iter().enumerate() {
            let end = if n % 2 == 0 { "\x07" } else { "\x1b\\" };
            stream.extend_from_slice(format!("\x1b]4;{n};?{end}").as_bytes());
            ends.push(stream.len());
            stream.extend_from_slice(&[b'x'; 600]);
            want.extend_from_slice(
                format!("\x1b]4;{n};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}{end}")
                    .as_bytes(),
            );
        }
        // The precondition: the queries that complete in any one read
        // answer in at most `REPLY` bytes, at the longest answer's 28.
        let longest = "\x1b]4;255;rgb:ffff/ffff/ffff\x1b\\".len();
        let mut at = 0;
        for read in stream.chunks(READ_CHUNK) {
            let done = ends
                .iter()
                .filter(|&&e| e > at && e <= at + read.len())
                .count();
            assert!(done * longest <= REPLY, "a read holds {done} queries");
            at += read.len();
        }
        let before = t.reply_overflows;
        let mut child = Vec::new();
        feed(
            &mut t,
            stream.chunks(READ_CHUNK),
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        let answered = child.windows(4).filter(|w| w == b"\x1b]4;").count();
        assert_eq!(answered, 256, "every index is answered");
        assert_eq!(child, want, "in order, each with its query's terminator");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
        assert!(t.reply().is_empty(), "nothing is left behind");
    }

    /// **One read that owes more than the reply buffer holds is answered
    /// whole and in order**: a DECRQSS for each seven bytes of four reads,
    /// 75 bytes an answer: 2,340 questions owe 175,500 bytes, over eight
    /// times the buffer.
    /// The control is the same reads with the hold off, which drop.
    #[test]
    fn a_read_of_questions_owing_more_than_the_buffer_is_answered_whole() {
        let rich = b"\x1b[1;2;3;4:3;5;7;8;9;53;58:2::1:2:3;38:2::100:200:255;48:2::10:20:30m";
        let query = b"\x1bP$qm\x1b\\";
        let stream = query.repeat(4 * READ_CHUNK / query.len());
        let run = |hold: bool| {
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            if hold {
                hold_replies(&mut t);
            }
            t.write_at(BytePos::ZERO, rich);
            t.feed(BytePos::ZERO, ReadLen::from_index(rich.len()));
            t.write_at(BytePos::ZERO, query);
            t.feed(BytePos::ZERO, ReadLen::from_index(query.len()));
            let one = t.reply().to_vec();
            t.clear_reply();
            let mut child = Vec::new();
            feed(
                &mut t,
                stream.chunks(READ_CHUNK),
                |b| child.extend_from_slice(b),
                |_| {},
                &mut crate::hostlog::NoLog,
            );
            (one, child, t.reply_overflows)
        };
        let (one, child, lost) = run(true);
        let n = stream.len() / query.len();
        assert!(
            n * one.len() > 3 * REPLY,
            "the reads owe {} bytes",
            n * one.len()
        );
        assert_eq!(child, one.repeat(n), "every answer, whole and in order");
        assert_eq!(lost, 0, "and none dropped");
        let (_, child, lost) = run(false);
        assert!(lost > 0 && child.len() < one.len() * n, "the control drops");
    }

    /// **What the core already owed goes ahead of a read's answers, and out
    /// of their way.** Answers left pending by an earlier writer -- 228 bytes
    /// of `DSR 5` here -- plus a read's two palette answers, 25 and 26 bytes,
    /// are more than the buffer holds, though either alone fits; `feed`
    /// drains before the first read as well as after each.
    #[test]
    fn what_the_core_already_owed_goes_ahead_of_a_reads_answers() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_default_palette(&[(0x12, 0x34, 0x56); 256]);
        let earlier = b"\x1b[5n".repeat(57);
        t.write_at(BytePos::ZERO, &earlier);
        t.feed(BytePos::ZERO, ReadLen::from_index(earlier.len()));
        assert_eq!(t.reply().len(), 228, "the answers are pending");
        let before = t.reply_overflows;
        let read = b"\x1b]4;1;?\x07\x1b]4;2;?\x1b\\";
        let mut child = Vec::new();
        feed(
            &mut t,
            [read.as_slice()],
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        let mut want = b"\x1b[0n".repeat(57);
        want.extend_from_slice(b"\x1b]4;1;rgb:1212/3434/5656\x07");
        want.extend_from_slice(b"\x1b]4;2;rgb:1212/3434/5656\x1b\\");
        assert_eq!(child, want, "the earlier answers, then the read's");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
    }

    /// **A read a page-width request held is fed on after `after`**, which
    /// resized the core between the two: the mark drawn after the request
    /// lands at the width asked for. The control: `after` runs twice, at the
    /// request and after the rest, and the width it saw first is the old one.
    #[test]
    fn the_rest_of_a_held_read_is_fed_after_the_embedder_resizes() {
        use baud_core::{Row, WidthHold};
        let room = WinSize {
            ws_row: 40,
            ws_col: 132,
        };
        let mut s = Owned::new(&room);
        let mut t = Term::new(s.storage(), &WS);
        t.set_width_hold(WidthHold::On);
        let read = b"\x1b[132$|\x1b[1;132HX";
        let mut widths = Vec::new();
        feed(
            &mut t,
            [read.as_slice()],
            |_| {},
            |t| {
                widths.push(t.geometry().0.index());
                if t.geometry().0.index() != 132 {
                    assert_eq!(t.resize(&room), baud_core::Resized::Ok);
                }
            },
            &mut crate::hostlog::NoLog,
        );
        assert_eq!(widths, [120, 132], "at the request, then after the rest");
        let row = t.row(Row::ZERO).expect("row 0");
        assert_eq!(
            row[131].content.as_codepoint().map(|c| c.value()),
            Some(u32::from(b'X'))
        );
    }

    /// Row 0's text.
    fn row0(t: &Term<'_>) -> String {
        t.row(baud_core::Row::ZERO)
            .expect("row 0")
            .iter()
            .filter_map(|c| c.content.as_codepoint())
            .filter_map(|c| char::from_u32(c.value()))
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// **A delay a macrograph meets after its reports stopped it still
    /// holds the reads behind it.** The reports fill the buffer past
    /// `REPLY_MOST` before the `S(T60)`, so the read stops inside the
    /// playing and `Term::take_held` plays the rest on, delay and all: the
    /// 600 reports before it go out at once, the five after it and the text
    /// after the envelope only once its second has passed.
    #[test]
    fn a_delay_in_a_macrograph_its_reports_stopped_still_holds() {
        let far = b"\x1bPpS(A[-9999999,-9999999][9999999,9999999])P[-9999999,-9999999]";
        let r = |n: usize| b"R(P)".repeat(n);
        let read = [
            far.as_slice(),
            b"@:A",
            &r(600),
            b"S(T60)",
            &r(5),
            b"@;@A\x1b\\AB",
        ]
        .concat();
        let report: &[u8] = b"[-9999999,-9999999]\r";
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        hold_replies(&mut t);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let mut child = Vec::new();
        let t0 = Instant::now();
        let mut run = |t: &mut Term<'_>, pace: &mut Pace, now, reads: &[&[u8]]| {
            paced(
                t,
                pace,
                now,
                reads.iter().copied(),
                |b| child.extend_from_slice(b),
                |_| {},
                &mut crate::hostlog::NoLog,
            );
            // What the driver's tick flushes (`flush_reply`).
            child.extend_from_slice(t.reply());
            t.clear_reply();
            child.len()
        };
        let sent = run(&mut t, &mut pace, t0, &[&read]);
        assert!(600 * report.len() > REPLY / 2, "the reports stop the read");
        assert_eq!(pace.deadline(), Some(t0 + Duration::from_secs(1)));
        assert_eq!(sent, 600 * report.len(), "the reports before the delay");
        assert_eq!(row0(&t), "", "nothing after the delay yet");
        let done = t0 + Duration::from_secs(1);
        let sent = run(&mut t, &mut pace, done, &[]);
        assert_eq!(sent, 605 * report.len(), "and the five after it");
        assert_eq!(row0(&t), "AB");
        assert_eq!(t.reply_overflows, 0);
        assert!(child.chunks(report.len()).all(|c| c == report));
    }

    /// **A ReGIS delay holds the rest of its read and every read after it
    /// until its deadline**, then feeds them in order: the text after
    /// `S(T60)` is not on the screen half a second in, and is a second in.
    /// The control: unpaced, the same reads end on the same screen.
    #[test]
    fn a_delay_holds_the_reads_behind_it_until_its_deadline() {
        let reads: [&[u8]; 2] = [b"\x1bPpS(T60)\x1b\\AB", b"CD"];
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let t0 = Instant::now();
        let fed = paced(
            &mut t,
            &mut pace,
            t0,
            reads,
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!(fed, Fed::Some);
        assert_eq!(pace.deadline(), Some(t0 + Duration::from_secs(1)));
        assert_eq!(row0(&t), "", "nothing after the delay yet");
        let half = t0 + Duration::from_millis(500);
        assert!(!pace.due(half));
        let fed = paced(
            &mut t,
            &mut pace,
            half,
            [&b"EF"[..]],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, row0(&t).as_str()), (Fed::Nothing, ""));
        let done = t0 + Duration::from_secs(1);
        assert!(pace.due(done));
        let fed = paced(
            &mut t,
            &mut pace,
            done,
            [],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, row0(&t).as_str()), (Fed::Some, "ABCDEF"));
        assert_eq!(pace.deadline(), None);
        // The control.
        let mut s2 = Owned::new(&WS);
        let mut u = Term::new(s2.storage(), &WS);
        feed(
            &mut u,
            reads.into_iter().chain([&b"EF"[..]]),
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!(row0(&u), "ABCDEF");
        assert_eq!(pace.cut, DelaysCut::NONE);
    }

    /// **A one-shot ReGIS input holds the rest of its read and every read
    /// after it until a key ends it**, with no deadline, and the next call
    /// feeds them: "no further data from the application (screen output)
    /// is processed until the locator input is complete" (070-13 § 5.1).
    /// The report goes before any of the held output.
    #[test]
    fn a_one_shot_input_holds_the_reads_until_a_key_ends_it() {
        let reads: [&[u8]; 2] = [b"\x1bPpR(P(I))\x1b\\AB", b"CD"];
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let t0 = Instant::now();
        let mut child = Vec::new();
        paced(
            &mut t,
            &mut pace,
            t0,
            reads,
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((row0(&t).as_str(), child.as_slice()), ("", &b""[..]));
        assert_eq!(pace.deadline(), None, "no deadline: the user ends it");
        assert!(!pace.released(&t));
        let later = t0 + Duration::from_secs(60);
        let fed = paced(
            &mut t,
            &mut pace,
            later,
            [&b"EF"[..]],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, row0(&t).as_str()), (Fed::Nothing, ""));
        let key = baud_core::KeyEvent::press(
            baud_domain::KeyCode::Char(baud_domain::Codepoint::from_char('q')),
            baud_domain::Modifiers::NONE,
        );
        input(&mut t, |t| t.key(key), |b| child.extend_from_slice(b));
        assert_eq!(child, b"q[0,0]\r");
        assert!(pace.released(&t), "owed a feed now");
        let fed = paced(
            &mut t,
            &mut pace,
            later,
            [],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, row0(&t).as_str()), (Fed::Some, "ABCDEF"));
        assert!(!pace.released(&t));
    }

    /// **A program writing more than [`HELD_MAX`] while one-shot input
    /// waits gives the input up** with the empty report, a carriage return
    /// alone (070-8 § 8.8.5.3), counted in the core, and its output is fed.
    #[test]
    fn a_one_shot_input_is_given_up_past_the_bound() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let t0 = Instant::now();
        let mut child = Vec::new();
        paced(
            &mut t,
            &mut pace,
            t0,
            [&b"\x1bPpR(P(I))\x1b\\"[..]],
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        let chunk = vec![b'x'; READ_CHUNK];
        let n = HELD_MAX / READ_CHUNK + 1;
        paced(
            &mut t,
            &mut pace,
            t0,
            (0..n).map(|_| &chunk[..]),
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!(child, b"", "at the bound, still waiting");
        let fed = paced(
            &mut t,
            &mut pace,
            t0,
            [],
            |b| child.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, child.as_slice()), (Fed::Some, &b"\r"[..]));
        assert_eq!(t.regis_counts().inputs_abandoned, 1);
        assert_eq!(row0(&t).len(), WS.ws_col.into(), "the output fed");
    }

    /// **A held DSR `?15` waits for the driver's printer, and the reads
    /// behind it wait for the answer**: nothing is answered or drawn until
    /// the driver says what it found, and then `?10n` goes out ahead of the
    /// text after the question, which lands in order. The control: a
    /// driver that never answers is answered at [`PRINTER_WAIT`] from the
    /// printer the core last had, `?13n` with none since power-up.
    #[test]
    fn a_held_printer_question_waits_for_the_drivers_answer() {
        for answer in [Some(baud_core::PrinterPort::Ready), None] {
            let mut s = Owned::new(&WS);
            let mut t = Term::new(s.storage(), &WS);
            t.set_printer_hold(baud_core::PrinterHold::On);
            let mut pace = Pace::new();
            let mut out = Vec::new();
            let t0 = Instant::now();
            let reads: [&[u8]; 2] = [b"\x1b[?15nAB", b"CD"];
            let mut asked = 0;
            paced(
                &mut t,
                &mut pace,
                t0,
                reads,
                |b| out.extend_from_slice(b),
                |t| asked += usize::from(t.printer_asked() == PrinterAsked::Owed),
                &mut crate::hostlog::NoLog,
            );
            assert_eq!(asked, 1, "the driver is asked once");
            assert!(pace.waits_for_printer());
            assert_eq!((out.as_slice(), row0(&t).as_str()), (&b""[..], ""));
            let early = t0 + PRINTER_WAIT / 2;
            paced(
                &mut t,
                &mut pace,
                early,
                [&b"EF"[..]],
                |b| out.extend_from_slice(b),
                |_| {},
                &mut crate::hostlog::NoLog,
            );
            assert_eq!(row0(&t), "", "{answer:?}: still waiting");
            let at = match answer {
                Some(port) => {
                    t.set_printer(port);
                    early
                }
                None => t0 + PRINTER_WAIT,
            };
            let fed = paced(
                &mut t,
                &mut pace,
                at,
                [],
                |b| out.extend_from_slice(b),
                |_| {},
                &mut crate::hostlog::NoLog,
            );
            let want: &[u8] = match answer {
                Some(_) => b"\x1b[?10n",
                None => b"\x1b[?13n",
            };
            assert_eq!((fed, out.as_slice()), (Fed::Some, want), "{answer:?}");
            assert_eq!(row0(&t), "ABCDEF", "{answer:?}");
            assert!(!pace.waits_for_printer());
            assert_eq!(t.printer_asked(), PrinterAsked::No);
        }
    }

    /// **A program writing through a long delay ends it early** once more
    /// than [`HELD_MAX`] bytes wait, and it is counted.
    #[test]
    fn a_delay_ends_early_past_its_bound() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        t.set_regis_hold(baud_core::RegisHold::On);
        let mut pace = Pace::new();
        let t0 = Instant::now();
        paced(
            &mut t,
            &mut pace,
            t0,
            [&b"\x1bPpS(T32767)\x1b\\"[..]],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        let chunk = vec![b'x'; READ_CHUNK];
        let n = HELD_MAX / READ_CHUNK + 1;
        let fed = paced(
            &mut t,
            &mut pace,
            t0,
            (0..n).map(|_| &chunk[..]),
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, pace.cut), (Fed::Nothing, DelaysCut::NONE));
        let fed = paced(
            &mut t,
            &mut pace,
            t0,
            [],
            |_| {},
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        assert_eq!((fed, pace.cut), (Fed::Some, DelaysCut(1)));
        assert_eq!(pace.deadline(), None);
    }

    /// **A key behind answers that nearly fill the buffer is kept.** The
    /// Linux loop fed a read, then queued the keys that arrived beside it,
    /// and drained once after both: behind 63 `DSR 5` answers -- 252 bytes --
    /// a six-byte arrow key, `CSI 1 ; 6 A`, did not fit, was dropped and counted, and
    /// `Term::key`'s `false` was ignored. Here the answers are left pending,
    /// the way any writer that did not drain would leave them.
    #[test]
    fn a_key_behind_pending_answers_reaches_the_child() {
        use baud_core::{KeyCode, KeyEvent, Modifiers};
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let read = b"\x1b[5n".repeat(63);
        t.write_at(BytePos::ZERO, &read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
        assert_eq!(t.reply().len(), 252, "the answers are pending");
        let before = t.reply_overflows;
        let mut child = Vec::new();
        let up = KeyEvent::press(KeyCode::Up, Modifiers::SHIFT.with(Modifiers::CTRL));
        let queued = input(&mut t, |t| t.key(up), |b| child.extend_from_slice(b));
        assert!(queued, "the key is queued");
        let mut want = b"\x1b[0n".repeat(63);
        want.extend_from_slice(b"\x1b[1;6A");
        assert_eq!(child, want, "the answers, then the key");
        assert_eq!(t.reply_overflows, before, "and nothing overflowed");
    }

    /// Base64, padded, for the kitty payload below.
    fn b64(data: &[u8]) -> Vec<u8> {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for q in data.chunks(3) {
            let n = q
                .iter()
                .enumerate()
                .fold(0u32, |n, (k, &b)| n | u32::from(b) << (16 - 8 * k));
            for k in 0..4 {
                let six = usize::try_from((n >> (18 - 6 * k)) & 63).unwrap_or(0);
                out.push(if k <= q.len() { A[six] } else { b'=' });
            }
        }
        out
    }

    /// **A transmitted PNG completes with no display link**: fed the way
    /// both drivers feed, it is placed and not shown, and [`decode_step`]
    /// called as the Linux loop calls it -- each turn, the wait not blocking
    /// while [`decode_wait`] says so -- shows it and owes its `OK`.
    #[test]
    fn a_kitty_png_completes_through_the_loops_step() {
        const PNG: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../baud-font/tests/png/rgba16-33x17-n.png"
        ));
        let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(1 << 12));
        let mut t = Term::new(owned.storage(), &WS);
        let mut cmd = b"\x1b_Ga=T,f=100,i=7;".to_vec();
        cmd.extend(b64(PNG));
        cmd.extend_from_slice(b"\x1b\\");
        let mut out = Vec::new();
        feed(
            &mut t,
            cmd.chunks(READ_CHUNK),
            |b| out.extend_from_slice(b),
            |_| {},
            &mut crate::hostlog::NoLog,
        );
        let (_, p) = t.images().placements().next().expect("placed at once");
        let image = p.image;
        assert!(
            t.images().image(image).is_none(),
            "not shown before its steps"
        );
        assert!(out.is_empty(), "nothing owed yet: {out:?}");
        let mut turns = 0;
        let mut shown = Shown::Same;
        let mut wait = Some(Duration::ZERO);
        while wait == Some(Duration::ZERO) {
            let (progress, s) = decode_step(&mut t);
            if s == Shown::Changed {
                shown = s;
            }
            owed(&mut t, &mut |b: &[u8]| out.extend_from_slice(b));
            wait = decode_wait(progress);
            turns += 1;
            assert!(turns < 100, "a decode that never ends");
        }
        assert!(turns >= 2, "stepped, not decoded in the feed: {turns}");
        assert_eq!(shown, Shown::Changed, "the frame is owed");
        let v = t.images().image(image).expect("shown");
        assert_eq!((v.width.raw(), v.height.raw()), (33, 17));
        assert_eq!(out, b"\x1b_Gi=7;OK\x1b\\");
        assert_eq!(decode_wait(DecodeProgress::Idle), None, "idle waits");
    }
}
