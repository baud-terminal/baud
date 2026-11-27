//! **The Linux driver's read turn, fed from a recording** (the test-only
//! `turn-bench` feature): `baud --turn-bench RECORDING CUTS` builds the
//! terminal as `linux::run` does, with no window and no pty, gathers the
//! reads `CUTS` lists into the batches `pty.rs`'s drain makes, feeds each
//! through the turn `linux.rs` runs on a readable master, prints what it
//! counted, and exits.
//!
//! `CUTS` is one read a line: its length in bytes, what `read(2)` returned
//! on the master, or `-` for a read that found nothing (`EAGAIN`), which
//! ends the batch. The lengths must add up to the recording's.
//!
//! The turn is two out-of-line functions, [`sniff`] and [`paced`], whose
//! symbols (`baud::turn_bench::sniff`, `baud::turn_bench::paced`) a
//! callgrind `--toggle-collect` names. Every window-side action of the
//! read's `after` is a counter here, and each is cleared from the core as
//! the driver clears it.
//!
//! The turn reads no clock: each batch is fed at the bench's start plus
//! [`STEP`] for every batch before it, so the pace's decisions, and the
//! instructions the turn runs, are the same on every run.

use crate::probe::{GroupAsk, ReadLinked, Sampler, Watched, Writers};
use crate::pty::{READ_BATCH, READ_CHUNK};
use crate::reads::Pace;
use baud_core::{Owned, Term};
use baud_domain::{Row, WinSize};
use std::ffi::OsString;
use std::ops::Range;
use std::time::{Duration, Instant};
use zerocopy::IntoBytes;

/// The flag that asks for the bench.
const FLAG: &str = "--turn-bench";

/// The time between two batches on the bench's clock.
const STEP: Duration = Duration::from_millis(1);

/// What a read is fed with, as the Linux driver tags it.
type Taken = (Writers, ReadLinked);

/// The grid the recordings were made at.
const GRID: WinSize = WinSize {
    ws_col: 200,
    ws_row: 50,
};

/// One line of the cuts file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cut {
    /// A read that returned this many bytes.
    Read(usize),
    /// A read that found nothing: the drain ends there.
    Again,
}

/// How many of something.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Count(usize);

/// **One turn's bytes**: where they lie in the recording and how many reads
/// filled them.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Batch {
    bytes: Range<usize>,
    reads: Count,
}

/// **The reads gathered into turns as the drain gathers them**
/// (`pty::drain_master`): before each read, a batch with no room for one
/// more [`READ_CHUNK`] within [`READ_BATCH`] ends; a [`Cut::Again`] ends it
/// too; and the last ends with the cuts. A drain that read nothing feeds
/// nothing, so it is no batch.
fn batches(cuts: &[Cut]) -> Vec<Batch> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut at = 0;
    let mut reads = Count::default();
    let mut close = |start: &mut usize, at: &mut usize, reads: &mut Count| {
        if reads.0 > 0 {
            out.push(Batch {
                bytes: *start..*start + *at,
                reads: *reads,
            });
        }
        *start += *at;
        *at = 0;
        *reads = Count::default();
    };
    let mut cuts = cuts.iter();
    loop {
        if at + READ_CHUNK > READ_BATCH {
            close(&mut start, &mut at, &mut reads);
            continue;
        }
        match cuts.next() {
            Some(Cut::Read(n)) => {
                at += n;
                reads.0 += 1;
            }
            Some(Cut::Again) => close(&mut start, &mut at, &mut reads),
            None => {
                close(&mut start, &mut at, &mut reads);
                break;
            }
        }
    }
    out
}

/// **The cuts file read**: a length from 1 to [`READ_CHUNK`] a line, or
/// `-`.
fn parse_cuts(text: &str) -> Result<Vec<Cut>, String> {
    text.lines()
        .enumerate()
        .map(|(i, line)| match line.trim() {
            "-" => Ok(Cut::Again),
            l => match l.parse() {
                Ok(n) if (1..=READ_CHUNK).contains(&n) => Ok(Cut::Read(n)),
                _ => Err(format!(
                    "line {}: {l:?} is neither a read of 1 to {READ_CHUNK} bytes nor -",
                    i + 1
                )),
            },
        })
        .collect()
}

/// **The sniff of one batch**, as the driver's readable-master turn does it:
/// the read asked about, and the writers taken with it. No master's group
/// is known here, as for a master with no foreground group.
#[inline(never)]
fn sniff(groups: &mut Sampler, bytes: &[u8], now: Instant) -> Taken {
    match groups.read(now, bytes) {
        GroupAsk::Yes(l) => (groups.writers(None, Watched::Nothing), l),
        GroupAsk::No => (groups.unasked(), ReadLinked::No),
    }
}

/// **One batch fed**, as the driver's `feed` calls `reads::paced`, with the
/// pace kept across batches and `took` cleared each.
#[inline(never)]
fn paced(
    term: &mut Term<'_>,
    pace: &mut Pace<Taken>,
    now: Instant,
    (bytes, tag): (&[u8], Taken),
    side: &mut Side,
    took: &mut Vec<Taken>,
) {
    took.clear();
    let Side {
        stubs,
        replied,
        hostlog,
    } = side;
    crate::reads::paced(
        term,
        pace,
        now,
        [(bytes, tag)],
        |b| replied.0 += b.len(),
        |t, _| stubs.after(t),
        hostlog,
        took,
    );
}

/// What a read's `after` reaches outside the core, each a counter here.
struct Stubs {
    clips: crate::clip::Clips,
    osc52: crate::config::Osc52,
    widths: crate::width::Widths,
    prints: crate::print::Prints,
    copies: Count,
    width_requests: Count,
    font_requests: Count,
    window_requests: Count,
    notices: Count,
    print_jobs: Count,
}

impl Stubs {
    /// The driver's `after`, each window-side action counted.
    fn after(&mut self, t: &mut Term<'_>) {
        if self.clips.poll(t, self.osc52).is_some() {
            self.copies.0 += 1;
        }
        if self.widths.poll(t, crate::width::Host::NoResize).is_some() {
            self.width_requests.0 += 1;
        }
        self.font_requests.0 += t.font_requests().len();
        t.clear_font_requests();
        self.window_requests.0 += t.window_requests().len();
        t.clear_window_requests();
        self.notices.0 += t.notices().count();
        self.print_jobs.0 += t.print_jobs().count();
        self.prints.take(t);
        t.clear_notices();
    }
}

/// What a batch's feed writes besides the core: the stubs, the reply bytes
/// and the host log.
struct Side {
    stubs: Stubs,
    replied: Count,
    hostlog: crate::hostlog::HostLog,
}

/// **The visible screen as one FNV-1a 64 hash**: each row's cells, each as
/// its content, its rendition resolved through its row's table and its
/// flags, then the cursor's row and column. The same function is in
/// `baud-bench/benches/ingest2.rs`, so the two can be compared.
struct ScreenHash(u64);

impl std::fmt::Display for ScreenHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

fn screen_hash(t: &Term<'_>) -> ScreenHash {
    let mut h = ScreenHash(0xcbf2_9ce4_8422_2325);
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h.0 ^= u64::from(*b);
            h.0 = h.0.wrapping_mul(0x0100_0000_01b3);
        }
    };
    let (_, rows) = t.geometry();
    let mut y = Some(Row::ZERO);
    while let Some(r) = y {
        let styles = t.row_styles(r);
        for cell in t.row(r).expect("a row within the geometry") {
            eat(cell.content.as_bytes());
            eat(t.style_in(styles, *cell).as_bytes());
            eat(cell.flags.as_bytes());
        }
        y = r.next(rows);
    }
    let (row, col) = t.cursor();
    eat(&u64::try_from(row.index()).unwrap_or(u64::MAX).to_le_bytes());
    eat(&u64::try_from(col.index()).unwrap_or(u64::MAX).to_le_bytes());
    h
}

/// **`main`'s door**: with `--turn-bench` first, the bench runs and the
/// process exits, 2 on a bad argument or file; otherwise this returns.
pub(crate) fn if_asked() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if args.first().is_none_or(|a| a != FLAG) {
        return;
    }
    match run(&args[1..]) {
        Ok(()) => std::process::exit(0),
        Err(why) => {
            eprintln!("baud: {FLAG}: {why}");
            std::process::exit(2)
        }
    }
}

fn run(args: &[OsString]) -> Result<(), String> {
    let [recording, cuts] = args else {
        return Err("takes RECORDING CUTS".into());
    };
    let data = std::fs::read(recording).map_err(|e| format!("{}: {e}", recording.display()))?;
    let text = std::fs::read_to_string(cuts).map_err(|e| format!("{}: {e}", cuts.display()))?;
    let cuts = parse_cuts(&text)?;
    let sum: usize = cuts
        .iter()
        .map(|c| match c {
            Cut::Read(n) => *n,
            Cut::Again => 0,
        })
        .sum();
    if sum != data.len() {
        return Err(format!(
            "the cuts add up to {sum} bytes and the recording holds {}",
            data.len()
        ));
    }
    let turns = batches(&cuts);

    // The terminal as `linux::run` builds it, with the default config's
    // budgets; only what needs a window, a bus or a display is left out.
    let cfg = crate::config::Config::defaults();
    let owned: &'static mut Owned = Box::leak(Box::new(
        Owned::with_geometry(&crate::host::MAX_GRID, &crate::host::host_geometry())
            .with_images(cfg.images)
            .with_clipboard(cfg.clipboard.budget)
            .with_printer(crate::print::budget(cfg.printing))
            .with_notices(crate::notify::BUDGET),
    ));
    let mut term = Term::new(owned.storage(), &GRID);
    term.set_kitty_files(Box::leak(Box::new(crate::kitty_files::LocalFiles::new())));
    term.set_regis_font(crate::regis_font::glyph);
    term.set_regis_hold(baud_core::RegisHold::On);
    crate::reads::hold_replies(&mut term);
    if let Some(release) = crate::pages::release() {
        term.set_page_release(release);
    }

    let mut groups = Sampler::for_shell(None);
    let mut pace: Pace<Taken> = Pace::new();
    let mut took = Vec::new();
    let mut side = Side {
        stubs: Stubs {
            clips: crate::clip::Clips::for_wayland(&term),
            osc52: cfg.clipboard.osc52,
            widths: crate::width::Widths::new(&term),
            prints: crate::print::Prints::new(),
            copies: Count::default(),
            width_requests: Count::default(),
            font_requests: Count::default(),
            window_requests: Count::default(),
            notices: Count::default(),
            print_jobs: Count::default(),
        },
        replied: Count::default(),
        hostlog: crate::hostlog::HostLog::new(crate::pty::start_dir()),
    };
    // The buffer a drain fills, kept across turns as the driver keeps it.
    let mut buf: Vec<u8> = Vec::with_capacity(READ_BATCH);
    let mut reads = Count::default();
    let mut held = Count::default();
    let mut now = Instant::now();
    for b in &turns {
        buf.clear();
        buf.extend_from_slice(&data[b.bytes.clone()]);
        reads.0 += b.reads.0;
        let tag = sniff(&mut groups, &buf, now);
        paced(&mut term, &mut pace, now, (&buf, tag), &mut side, &mut took);
        if pace.holds() {
            held.0 += 1;
        }
        now += STEP;
    }

    let s = &side.stubs;
    println!("bytes: {}", data.len());
    println!("reads: {}", reads.0);
    println!("batches: {}", turns.len());
    println!("osc 52 copies: {}", s.copies.0);
    println!("width requests: {}", s.width_requests.0);
    println!("font requests: {}", s.font_requests.0);
    println!("window requests: {}", s.window_requests.0);
    println!("notices: {}", s.notices.0);
    println!("print jobs: {}", s.print_jobs.0);
    println!("reply bytes: {}", side.replied.0);
    println!("pace held: {}", held.0);
    println!("screen hash: {}", screen_hash(&term));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(bytes: Range<usize>, reads: Count) -> Batch {
        Batch { bytes, reads }
    }

    /// **A batch holds reads while one more [`READ_CHUNK`] fits in
    /// [`READ_BATCH`]**: sixteen full reads fill one exactly, and the
    /// seventeenth starts the next.
    #[test]
    fn full_reads_fill_a_batch_at_sixteen() {
        assert_eq!(
            batches(&vec![Cut::Read(READ_CHUNK); 17]),
            [
                batch(0..READ_BATCH, Count(16)),
                batch(READ_BATCH..READ_BATCH + READ_CHUNK, Count(1))
            ]
        );
    }

    /// **The boundary is the room for a whole read, not the bytes held**:
    /// 1024-byte reads stop at 61, at 62,464 bytes, since a 62nd read could
    /// return a full chunk past 64 KiB.
    #[test]
    fn short_reads_stop_where_a_full_one_would_not_fit() {
        let b = batches(&vec![Cut::Read(1024); 62]);
        assert_eq!(
            b,
            [
                batch(0..61 * 1024, Count(61)),
                batch(61 * 1024..62 * 1024, Count(1))
            ]
        );
    }

    /// **`-` ends a batch early**, and a `-` with nothing read before it is
    /// a wake that feeds nothing, so no batch.
    #[test]
    fn again_ends_a_batch_and_an_empty_drain_is_none() {
        let cuts = [
            Cut::Again,
            Cut::Read(10),
            Cut::Read(20),
            Cut::Again,
            Cut::Again,
            Cut::Read(5),
        ];
        assert_eq!(
            batches(&cuts),
            [batch(0..30, Count(2)), batch(30..35, Count(1))]
        );
    }

    /// **A full batch ends before the `-` after it is read**, as the drain
    /// checks for room before it reads: the `-` is the next drain's, which
    /// finds nothing.
    #[test]
    fn a_full_batch_ends_before_the_again_after_it() {
        let mut cuts = vec![Cut::Read(READ_CHUNK); 16];
        cuts.push(Cut::Again);
        cuts.push(Cut::Read(7));
        assert_eq!(
            batches(&cuts),
            [
                batch(0..READ_BATCH, Count(16)),
                batch(READ_BATCH..READ_BATCH + 7, Count(1))
            ]
        );
    }

    /// No reads, no batches.
    #[test]
    fn no_cuts_no_batches() {
        assert_eq!(batches(&[]), []);
    }

    /// **The cuts file**: lengths, `-`, and a refusal naming the line for
    /// a length out of range or anything else.
    #[test]
    fn the_cuts_file_takes_lengths_and_dashes() {
        assert_eq!(
            parse_cuts("1024\n-\n4096\n"),
            Ok(vec![Cut::Read(1024), Cut::Again, Cut::Read(READ_CHUNK)])
        );
        assert!(parse_cuts("0\n").is_err_and(|e| e.starts_with("line 1:")));
        let past = format!("1\n{}\n", READ_CHUNK + 1);
        assert!(parse_cuts(&past).is_err_and(|e| e.starts_with("line 2:")));
        assert!(parse_cuts("x\n").is_err());
    }
}
