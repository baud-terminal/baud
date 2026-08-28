//! One payload, in memory, N times. Nothing else.
//!
//! Exists because `baud-bench` cannot answer "what did *this* payload cost":
//! it forks a child, opens a pty and runs every payload and every recording in
//! one process, so a counter trace over it is dominated by whatever is
//! largest. Three separate questions have now needed a workload with one
//! variable in it -- sizing the UTF-8 decode path, and twice trying to explain
//! the layout anomalies -- and each time the answer was "the harness cannot
//! produce that".
//!
//! No fork, no pty, no `read(2)` and no printing inside the loop: the process
//! is the measurement, so `xctrace` counters over the whole run are counters
//! over the ingest of one payload.
//!
//! **It times itself, and reports the power source.** Timing the process from
//! outside charges the measurement for process spawn, page-in and the
//! allocation of a 24 MB `Storage`; at the sizes these A/Bs need that was a
//! large part of a +-9% spread. Timing inside, per rep, and reporting the
//! median removes it and gives a distribution rather than one number.
//!
//! The power source is printed because it decides whether the result means
//! anything: `/usr/bin/time` measures a clock the OS is free to move, and a
//! laptop on battery moves it. A number recorded here without `AC Power` next
//! to it should be re-run before it is relied on.
//!
//! ```text
//! ingest streaming 40      # payload::streaming, 40 passes
//! ingest vim 20            # tests/recordings/vim-unicode.raw
//! ```

#[path = "../payload.rs"]
mod payload;

use baud_core::{OpMode, Storage, StreamByte, Term};

fn recording(name: &str) -> Vec<u8> {
    let p = format!(
        "{}/../baud-core/tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// `AC Power`, `Battery Power`, or `unknown`.
///
/// Not decoration. A measurement taken on battery is one the OS was free to
/// reprice, and this project has already recorded a 1.7% result that a
/// frequency-invariant metric would not reproduce.
fn power_source() -> String {
    std::process::Command::new("/usr/bin/pmset")
        .args(["-g", "batt"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            s.lines()
                .next()
                .and_then(|l| l.split('\'').nth(1))
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown".into())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "streaming".into());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    // The anomalies do not all live on the same path: `036a432` was recorded
    // on spans and `a4cecba` on cells, so the mode is an argument.
    let mode = args.next().unwrap_or_else(|| "runs".into());
    let op = match mode.as_str() {
        "cells" => OpMode::Computed,
        "templated" => OpMode::Templated,
        _ => OpMode::Runs,
    };

    let mut seq = 0u64;
    let data = match which.as_str() {
        "streaming" => payload::streaming(1 << 20, &mut seq),
        "tui" => payload::tui(1 << 19, &mut seq),
        "cjk" => payload::cjk(1 << 20, &mut seq),
        "btop" => recording("btop.raw"),
        "asciiquarium" => recording("asciiquarium.raw"),
        "vim" => recording("vim-unicode.raw"),
        other => panic!("unknown payload {other}"),
    };

    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, op);
    let mut fed = 0usize;
    let mut per_rep = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t0 = std::time::Instant::now();
        let mut at = 0usize;
        while at < data.len() {
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&data[at..]));
            let n = f.len();
            t.feed(base, n);
            t.commit_read(n);
            at += n.index();
            fed += n.index();
        }
        per_rep.push((t0.elapsed().as_secs_f64(), at));
    }
    // Printed once, after the loop, so the ingest path is all a trace sees.
    // Also stops the optimiser deciding none of this was observable.
    let mut rates: Vec<f64> = per_rep
        .iter()
        .map(|(s, n)| *n as f64 / *s / (1 << 20) as f64)
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).expect("no NaN: every rep took time"));
    let (lo, med, hi) = (rates[0], rates[rates.len() / 2], rates[rates.len() - 1]);
    let (bulk, single, fill) = t.cell_attribution();
    println!(
        "{which} {mode}: {med:.1} MB/s [{lo:.1}-{hi:.1}] over {reps} reps, \
         {fed} bytes, utf8 {:.1}%, {}",
        t.utf8_share() * 100.0,
        power_source(),
    );
    // Where the cells came from. `cells_written` alone gives amplification and
    // not its shape, and the three paths cost very different amounts per cell.
    println!(
        "  cells {} written ({:.1}x per byte): bulk {:.0}%, single {:.0}%, \
         fill {:.0}%",
        t.cells_written.count(),
        t.cells_written.count() as f64 / fed as f64,
        bulk * 100.0,
        single * 100.0,
        fill * 100.0,
    );
    println!(
        "  {} dispatches, one per {:.0} bytes; {} scrolls blanking {} cells",
        t.dispatches.count(),
        fed as f64 / t.dispatches.count().max(1) as f64,
        t.scrolls.count(),
        t.cells_scroll.count(),
    );
}
