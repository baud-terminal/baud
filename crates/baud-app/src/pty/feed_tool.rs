//! **The window's feed from its pty, timed with no window.** A child runs
//! `cat` of a recording on a pty the window's own reader reads
//! ([`Pty::spawn`]), and the turns drain it as the macOS pump does: one
//! [`Pty::drain`] a wake, every chunk fed through [`reads::paced`] into a
//! core of the window's size, the turn's chunks held until the turn ends or
//! given back as soon as they are fed, and a fixed wait after any turn that
//! fed something, standing in for the frame it owes.
//!
//! It prints the rate, the reads and how many chunks each drain took, and
//! the core's cells and screen checksum, so two builds of the reader are
//! compared on equal work: the same cells and the same checksum.
//!
//! ```text
//! PUMP_FEED=<recording> [PUMP_FEED_HOLD_MS=0,2,8] [PUMP_FEED_GIVE=end|fed] \
//!   [PUMP_FEED_SIZE=80x24] [PUMP_FEED_REPS=1] cargo test --release -p baud --bin baud \
//!   pty::feed_tool::the_feed_timed -- --ignored --nocapture
//! ```
//!
//! `PUMP_FEED_REPS` has `cat` write the recording that many times over, one
//! run, so a short recording streams long enough to time.
//!
//! One line a hold:
//!
//! ```text
//! pump-feed rec=<name> give=end hold_ms=2 bytes=.. secs=.. mb_s=.. turns=..
//!   reads=.. read_max=.. drain_max=.. drain_median=.. at_max=.. cells=.. checksum=..
//! ```

use super::{Pty, ShellStart, Stream, WinSize};
use crate::hostlog::NoLog;
use crate::reads::{Pace, paced};
use baud_core::{Owned, Term};
use baud_font::Px;
use std::ffi::OsString;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// When a turn gives its chunks back: at the end of the turn, after the
/// wait for its frame, or as soon as they are fed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Give {
    End,
    Fed,
}

impl Give {
    fn of(s: &str) -> Option<Give> {
        match s {
            "end" => Some(Give::End),
            "fed" => Some(Give::Fed),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Give::End => "end",
            Give::Fed => "fed",
        }
    }
}

/// A number of chunks, or of drains.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub(super) struct Count(pub(super) usize);

impl std::fmt::Display for Count {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// How many chunks each drain that took any took.
#[derive(Default, Debug)]
pub(super) struct Tally {
    per_drain: Vec<Count>,
}

impl Tally {
    pub(super) fn drained(&mut self, chunks: Count) {
        if chunks > Count(0) {
            self.per_drain.push(chunks);
        }
    }

    pub(super) fn turns(&self) -> Count {
        Count(self.per_drain.len())
    }

    pub(super) fn max(&self) -> Count {
        self.per_drain.iter().copied().max().unwrap_or_default()
    }

    /// The drains that took as many chunks as the most any drain took.
    pub(super) fn at_max(&self) -> Count {
        let max = self.max();
        Count(self.per_drain.iter().filter(|&&n| n == max).count())
    }

    /// The middle drain's count, the lower of the two middles when even.
    pub(super) fn median(&self) -> Count {
        let mut sorted = self.per_drain.clone();
        sorted.sort_unstable();
        match sorted.len() {
            0 => Count(0),
            n => sorted[(n - 1) / 2],
        }
    }
}

/// What one run fed and how long it took.
pub(super) struct Run {
    pub(super) bytes: usize,
    pub(super) reads: usize,
    pub(super) read_max: usize,
    pub(super) secs: f64,
    pub(super) tally: Tally,
    pub(super) cells: usize,
    pub(super) checksum: String,
}

/// The core's checksum of its whole screen, as its DECCKSR reply carries it.
const CHECKSUM_ID: u16 = 4242;

/// Runs `argv` on a pty of `size` and feeds what it writes as the pump
/// does, holding each turn `hold` after a turn that fed something.
pub(super) fn run(argv: &[&str], size: &WinSize, hold: Duration, give: Give) -> Run {
    let woken = Arc::new((Mutex::new(false), Condvar::new()));
    let waker = Arc::clone(&woken);
    let given = argv.iter().map(OsString::from).collect();
    let start = ShellStart {
        integration: crate::shell::Integration::Off,
        ssh_env: crate::shell::SshEnv::Off,
        program: Some("/bin/sh".into()),
        run: None,
    }
    .running(crate::rules::Run::given(given).expect("a program"));
    let mut storage = Owned::new(size);
    let mut term = Term::new(storage.storage(), size);
    let mut pace: Pace = Pace::new();
    let mut replies = Vec::new();
    let mut took = Vec::new();
    let mut tally = Tally::default();
    let (mut bytes, mut reads, mut read_max) = (0, 0, 0);
    let mut first: Option<Instant> = None;
    let pty = Pty::spawn(size, Px::of(8), Px::of(16), None, &start, move || {
        let (flag, cv) = &*waker;
        *flag.lock().expect("the wake flag") = true;
        cv.notify_one();
    })
    .expect("a pty");
    let ended = loop {
        {
            let (flag, cv) = &*woken;
            let mut up = flag.lock().expect("the wake flag");
            if !*up {
                // A reader that has ended wakes no one: the bound lets the
                // turn drain and see the end.
                up = cv
                    .wait_timeout(up, Duration::from_millis(100))
                    .expect("the wake flag")
                    .0;
            }
            *up = false;
        }
        let drained = pty.drain();
        tally.drained(Count(drained.chunks.len()));
        if !drained.chunks.is_empty() {
            first.get_or_insert_with(Instant::now);
        }
        for chunk in &drained.chunks {
            bytes += chunk.bytes.len();
            reads += 1;
            read_max = read_max.max(chunk.bytes.len());
        }
        took.clear();
        paced(
            &mut term,
            &mut pace,
            Instant::now(),
            drained.chunks.iter().map(|chunk| (&chunk.bytes[..], ())),
            |b| replies.extend_from_slice(b),
            |_, _| {},
            &mut NoLog,
            &mut took,
        );
        let fed = !drained.chunks.is_empty();
        let stream = drained.stream;
        let held = match give {
            Give::Fed => {
                drop(drained);
                None
            }
            Give::End => Some(drained),
        };
        if fed && !hold.is_zero() {
            std::thread::sleep(hold);
        }
        drop(held);
        if stream == Stream::Closed {
            break Instant::now();
        }
    };
    let secs = first.map_or(0.0, |f| ended.duration_since(f).as_secs_f64());
    drop(pty);
    let cells = term.amplification().1;
    replies.clear();
    let ask = format!("\x1b[{CHECKSUM_ID};1;1;1;{};{}*y", size.ws_row, size.ws_col);
    took.clear();
    paced(
        &mut term,
        &mut pace,
        Instant::now(),
        [(ask.as_bytes(), ())],
        |b| replies.extend_from_slice(b),
        |_, _| {},
        &mut NoLog,
        &mut took,
    );
    let checksum = String::from_utf8_lossy(&replies)
        .rsplit('~')
        .next()
        .map(|s| s.trim_end_matches("\x1b\\").to_string())
        .unwrap_or_default();
    Run {
        bytes,
        reads,
        read_max,
        secs,
        tally,
        cells,
        checksum,
    }
}

/// **The tool**: `PUMP_FEED` names the recording; see the module.
#[test]
#[ignore = "a tool: PUMP_FEED=<recording> times the window's feed from its pty"]
fn the_feed_timed() {
    let rec = std::env::var("PUMP_FEED").expect("PUMP_FEED names a recording");
    let holds = std::env::var("PUMP_FEED_HOLD_MS").unwrap_or_else(|_| "0,2,8".into());
    let give = std::env::var("PUMP_FEED_GIVE").unwrap_or_else(|_| "end".into());
    let give = Give::of(&give).expect("PUMP_FEED_GIVE is end or fed");
    let reps: usize = std::env::var("PUMP_FEED_REPS")
        .map_or(1, |r| r.parse().expect("PUMP_FEED_REPS is a count"));
    let size = std::env::var("PUMP_FEED_SIZE").unwrap_or_else(|_| "80x24".into());
    let (cols, rows) = size.split_once('x').expect("PUMP_FEED_SIZE is COLSxROWS");
    let size = WinSize {
        ws_col: cols.parse().expect("columns"),
        ws_row: rows.parse().expect("rows"),
    };
    let name = std::path::Path::new(&rec)
        .file_name()
        .map_or_else(|| rec.clone(), |n| n.to_string_lossy().into_owned());
    for h in holds.split(',') {
        let ms: u64 = h.trim().parse().expect("a hold in milliseconds");
        let mut argv = vec!["/bin/cat"];
        argv.extend(std::iter::repeat_n(rec.as_str(), reps));
        let r = run(&argv, &size, Duration::from_millis(ms), give);
        let mb_s = match r.secs > 0.0 {
            true => r.bytes as f64 / r.secs / 1e6,
            false => 0.0,
        };
        println!(
            "pump-feed rec={name} reps={reps} give={} hold_ms={ms} bytes={} secs={:.4} mb_s={mb_s:.2} \
             turns={} reads={} read_max={} drain_max={} drain_median={} at_max={} \
             cells={} checksum={}",
            give.name(),
            r.bytes,
            r.secs,
            r.tally.turns(),
            r.reads,
            r.read_max,
            r.tally.max(),
            r.tally.median(),
            r.tally.at_max(),
            r.cells,
            r.checksum,
        );
    }
}

/// **The tally reads a known run of drains right**: the most any drain
/// took, how many took that many, the middle count, and no turn for a
/// drain that took nothing.
#[test]
fn the_tally_reads_a_known_run_of_drains() {
    let mut t = Tally::default();
    for n in [3, 32, 0, 1, 32, 32, 0] {
        t.drained(Count(n));
    }
    assert_eq!(t.turns(), Count(5), "a drain of nothing is no turn");
    assert_eq!(t.max(), Count(32));
    assert_eq!(t.at_max(), Count(3));
    assert_eq!(t.median(), Count(32));
    let empty = Tally::default();
    assert_eq!(
        (empty.turns(), empty.max(), empty.median()),
        (Count(0), Count(0), Count(0))
    );
}

/// **The tool feeds every byte the child wrote, and its core's screen is a
/// core's fed the same bytes whole**: a file with no line feed, so the
/// tty passes it unchanged, wrapping and scrolling at 80 columns, run
/// through the pty with a hold and either give, reads the file's length,
/// and the checksum and cells of a core fed the file in one read. The
/// control: a file one byte different reads a different checksum.
#[test]
fn the_feed_reads_a_file_whole_and_its_screen_matches() {
    let dir = std::env::temp_dir().join(format!("baud-feed-tool-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let body: Vec<u8> = b"abcdefghijklmnopqrstuvwxyz"
        .iter()
        .cycle()
        .take(9000)
        .copied()
        .collect();
    let mut other = body.clone();
    other[8990] = b'Z';
    let size = WinSize {
        ws_col: 80,
        ws_row: 24,
    };
    let reference = |bytes: &[u8]| -> (usize, String) {
        // The reference is a core fed the file in one read, run through
        // the same checksum ask as the tool's.
        let mut storage = Owned::new(&size);
        let mut term = Term::new(storage.storage(), &size);
        let mut pace: Pace = Pace::new();
        let mut replies = Vec::new();
        let ask = format!("\x1b[{CHECKSUM_ID};1;1;1;{};{}*y", size.ws_row, size.ws_col);
        for read in [bytes, ask.as_bytes()] {
            paced(
                &mut term,
                &mut pace,
                Instant::now(),
                [(read, ())],
                |b| replies.extend_from_slice(b),
                |_, _| {},
                &mut NoLog,
                &mut Vec::new(),
            );
        }
        let cells = term.amplification().1;
        let sum = String::from_utf8_lossy(&replies)
            .rsplit('~')
            .next()
            .map(|s| s.trim_end_matches("\x1b\\").to_string())
            .unwrap_or_default();
        (cells, sum)
    };
    let (cells, sum) = reference(&body);
    let (_, other_sum) = reference(&other);
    assert!(!sum.is_empty(), "the core answers the checksum");
    assert_ne!(sum, other_sum, "the control: one byte moves the checksum");
    let path = dir.join("body");
    std::fs::write(&path, &body).expect("the file");
    let path = path.to_str().expect("a path");
    for give in [Give::End, Give::Fed] {
        let r = run(&["/bin/cat", path], &size, Duration::from_millis(1), give);
        assert_eq!(r.bytes, body.len(), "{give:?}: every byte");
        assert!(r.reads >= 1 && r.tally.turns() >= Count(1), "{give:?}");
        assert_eq!(r.cells, cells, "{give:?}: the cells of one read");
        assert_eq!(r.checksum, sum, "{give:?}: the screen of one read");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
