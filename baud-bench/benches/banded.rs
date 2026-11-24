//! **What banded storage costs in time, against one table, through one
//! harness**: both arms are an `Owned` at the same shape, so a difference
//! is the mode's.
//!
//! * `banded ingest <payload> <reps> <cols> <rows> <single|N> <64|128>` --
//!   a recording fed `reps` times at a fixed cache line, one table or bands
//!   of `N` rows: cells written and refusals first, then the median MB/s and
//!   its range over the reps.
//! * `banded lf <payload> <cols> <rows> <depth> <single|banded>` -- the
//!   recording fed a line at a time, each line through its line feed timed:
//!   cells written first, then p50, p99 and max; then the minor faults
//!   taken while feeding, the lines that took any, the lines on which the
//!   current band's table was swept, and the worst line's faults and sweep.
//! * `banded resize <cols> <rows> <depth> <single|banded> <n>` -- the history
//!   filled from real recordings (cat-scroll and git-log four times), then
//!   the width changed `n` times, `cols` and 15/16 of it in turn: p50 and
//!   max of one change.
//!
//! Banded without `N`, the band is the most rows its table holds.

use baud_core::{BandRows, Owned, Provision, Resized, Term};
use baud_domain::{BytePos, CacheGeometry, Depth, LineSize, ReadLen, Rows, WinSize};
use std::time::{Duration, Instant};

fn recording(name: &str) -> Vec<u8> {
    let p = format!("{}/../tests/recordings/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn payload(name: &str) -> Vec<u8> {
    match name {
        "cat" => recording("ascii/cat-scroll.raw"),
        "git-log" => recording("ascii/git-log.raw"),
        "less" => recording("ascii/less-pager.raw"),
        "cat-cjk" => recording("utf8/cat-cjk.raw"),
        "vim" => recording("utf8/vim-unicode.raw"),
        "btop" => recording("utf8/btop.raw"),
        p => panic!("payload {p}: cat, git-log, less, cat-cjk, vim or btop"),
    }
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// One table, or bands of `band` rows (`None`: the most a table holds).
fn provision(ws: &WinSize, banded: Option<Option<usize>>, depth: Depth) -> Provision {
    match banded {
        None => Provision::of(ws)
            .unwrap_or_else(|_| panic!("{}x{} is past one table", ws.ws_col, ws.ws_row)),
        Some(band) => {
            let rows = band.unwrap_or(usize::from(ws.ws_row) + depth.lines());
            Provision::banded(ws, BandRows::of(Rows::of_index(rows)))
                .unwrap_or_else(|_| panic!("{}x{} is refused banded", ws.ws_col, ws.ws_row))
        }
    }
}

/// The median of `sorted`.
fn p50(sorted: &[Duration]) -> Duration {
    sorted[(sorted.len() - 1) / 2]
}

/// The value 99% of the way up `sorted`.
fn p99(sorted: &[Duration]) -> Duration {
    sorted[(sorted.len() - 1) * 99 / 100]
}

/// One line fed: its time, the minor faults taken while it was fed, and
/// whether the current band's table was swept.
struct Line {
    took: Duration,
    faults: Faults,
    swept: Swept,
}

/// Whether the current band's table was swept while a line was fed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Swept {
    No,
    Yes,
}

/// The process's minor page faults so far (`getrusage`), read outside
/// the timed region.
fn minor_faults() -> Faults {
    let mut ru = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: `ru` is a valid, writable rusage; getrusage fills it.
    let r = unsafe { libc::getrusage(libc::RUSAGE_SELF, ru.as_mut_ptr()) };
    assert_eq!(r, 0, "getrusage");
    // SAFETY: getrusage succeeded, so `ru` is initialised.
    Faults(unsafe { ru.assume_init() }.ru_minflt)
}

/// A count of minor page faults.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Faults(i64);

impl core::ops::Sub for Faults {
    type Output = Faults;
    fn sub(self, then: Faults) -> Faults {
        Faults(self.0 - then.0)
    }
}

fn us(d: Duration) -> String {
    format!("{:.1}", d.as_secs_f64() * 1e6)
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize, what: &str| -> &str {
        args.get(i)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("banded: no {what}"))
    };
    let ws_at = |i: usize| WinSize {
        ws_col: arg(i, "cols").parse().expect("cols"),
        ws_row: arg(i + 1, "rows").parse().expect("rows"),
    };
    match arg(0, "subcommand") {
        "ingest" => {
            let name = arg(1, "payload");
            let reps: usize = arg(2, "reps").parse().expect("reps");
            let ws = ws_at(3);
            let banded = match arg(5, "single or N") {
                "single" => None,
                n => Some(Some(n.parse::<usize>().expect("band rows"))),
            };
            let line: usize = arg(6, "64 or 128").parse().expect("line");
            let geom = CacheGeometry::new(Some(LineSize::of(line)), None, None, None);
            let mut owned = Owned::with_geometry(provision(&ws, banded, Depth::DEFAULT), &geom);
            let band = owned.band_rows().rows().index();
            let bytes = payload(name);
            let mut t = Term::new(owned.storage(), &ws);
            let mut rates = Vec::with_capacity(reps);
            for _ in 0..reps {
                let at = Instant::now();
                feed(&mut t, &bytes);
                rates.push(bytes.len() as f64 / at.elapsed().as_secs_f64() / 1e6);
            }
            rates.sort_by(f64::total_cmp);
            let (_, cells) = t.amplification();
            println!(
                "{name} {}x{} {} line {line} band {band}: cells {cells} refusals {} | {:.1} MB/s [{:.1}-{:.1}] over {reps} reps",
                ws.ws_col,
                ws.ws_row,
                arg(5, ""),
                t.style_reclaims().refusals,
                rates[reps / 2],
                rates[0],
                rates[reps - 1],
            );
        }
        "lf" => {
            let name = arg(1, "payload");
            let ws = ws_at(2);
            let depth = Depth::of(arg(4, "depth").parse().expect("depth"));
            let banded = match arg(5, "single or banded") {
                "single" => None,
                _ => Some(None),
            };
            let mut owned = Owned::with_history(
                provision(&ws, banded, depth),
                &baud_bench::host_geometry(),
                depth,
            );
            let bytes = payload(name);
            let mut t = Term::new(owned.storage(), &ws);
            let mut lines = Vec::new();
            let faults_at = minor_faults();
            for line in bytes.split_inclusive(|&b| b == b'\n') {
                let (faults, sweeps) = (minor_faults(), t.style_reclaims().sweeps);
                let at = Instant::now();
                feed(&mut t, line);
                let took = at.elapsed();
                lines.push(Line {
                    took,
                    faults: minor_faults() - faults,
                    swept: match t.style_reclaims().sweeps > sweeps {
                        true => Swept::Yes,
                        false => Swept::No,
                    },
                });
            }
            let faults = minor_faults() - faults_at;
            let faulted = lines.iter().filter(|l| l.faults > Faults(0)).count();
            let swept = lines.iter().filter(|l| l.swept == Swept::Yes).count();
            let mut times: Vec<Duration> = lines.iter().map(|l| l.took).collect();
            times.sort();
            let worst = lines.iter().max_by_key(|l| l.took).expect("a line");
            let (_, cells) = t.amplification();
            println!(
                "{name} {}x{} depth {} {}: cells {cells} refusals {} | per line p50 {} p99 {} max {} us over {} lines | \
                 minor faults {} on {faulted} lines, sweeps on {swept} lines; worst line {} faults{}",
                ws.ws_col,
                ws.ws_row,
                depth.lines(),
                arg(5, ""),
                t.style_reclaims().refusals,
                us(p50(&times)),
                us(p99(&times)),
                us(*times.last().expect("a line")),
                times.len(),
                faults.0,
                worst.faults.0,
                match worst.swept {
                    Swept::Yes => ", swept",
                    Swept::No => "",
                },
            );
        }
        "resize" => {
            let ws = ws_at(1);
            let depth = Depth::of(arg(3, "depth").parse().expect("depth"));
            let banded = match arg(4, "single or banded") {
                "single" => None,
                _ => Some(None),
            };
            let n: usize = arg(5, "n").parse().expect("n");
            let mut owned = Owned::with_history(
                provision(&ws, banded, depth),
                &baud_bench::host_geometry(),
                depth,
            );
            let mut t = Term::new(owned.storage(), &ws);
            let (cat, log) = (
                recording("ascii/cat-scroll.raw"),
                recording("ascii/git-log.raw"),
            );
            for _ in 0..4 {
                feed(&mut t, &cat);
                feed(&mut t, &log);
            }
            let narrower = WinSize {
                ws_col: ws.ws_col - ws.ws_col / 16,
                ws_row: ws.ws_row,
            };
            let mut times = Vec::with_capacity(n);
            for i in 0..n {
                let to = if i % 2 == 0 { &narrower } else { &ws };
                let at = Instant::now();
                let r = t.resize(to);
                times.push(at.elapsed());
                assert!(r != Resized::BandsUnlent, "a change of width refused");
            }
            times.sort();
            println!(
                "resize {}x{} depth {} {}: rows frozen {} | per change p50 {} max {} us over {n}",
                ws.ws_col,
                ws.ws_row,
                depth.lines(),
                arg(4, ""),
                t.frozen().counts.rows_frozen,
                us(p50(&times)),
                us(*times.last().expect("a change")),
            );
        }
        s => panic!("subcommand {s}: ingest, lf or resize"),
    }
}
