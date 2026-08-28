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
//! No fork, no pty, no `read(2)`, no timing, no printing inside the loop: the
//! process is the measurement, so `xctrace` counters over the whole run are
//! counters over the ingest of one payload.
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

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "streaming".into());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

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
    let mut t = Term::new(&mut store, OpMode::Runs);
    let mut fed = 0usize;
    for _ in 0..reps {
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
    }
    // Printed once, after the loop, so the ingest path is all the trace sees.
    // Also stops the optimiser deciding none of this was observable.
    println!("{which}: {fed} bytes, utf8 {:.1}%", t.utf8_share() * 100.0);
}
