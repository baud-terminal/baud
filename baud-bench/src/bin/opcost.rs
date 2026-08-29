//! What does one operation cost?
//!
//! Every other instrument in this repo answers a different question. The
//! counters say *what happened* -- dispatches, cells, scrolls -- and never how
//! long any of it took. `ingest` says how fast a whole payload ran and leaves
//! the attribution to arithmetic. A sampling profile names the **function** the
//! time was in, which is not the same as the operation: this session read
//! `dispatch` at 30% and found that skipping the work it appeared to do was
//! worth 1%, because the arms were inlined into it.
//!
//! Two ceiling arms in the same session measured the wrong thing outright.
//! Stubbing `apply_sgr` looked like +22% and was mostly blanking getting
//! cheaper, because a frozen rendition means a frozen blank.
//!
//! So this measures operations directly. Each row is a payload built from one
//! operation repeated, driven through the real ingest path, timed in process.
//! The cost of an operation is the difference between its row and the row that
//! carries only its bytes -- so `sgr` is priced against the text it is embedded
//! in, not against nothing.
//!
//! ```text
//! opcost            # every operation, 40 reps
//! opcost 200        # more reps
//! ```

use baud_core::{OpMode, Storage, StreamByte, Term};

const TARGET: usize = 1 << 21;

/// A payload built by repeating `unit` until it is about `TARGET` bytes, with
/// `per` operations per repetition.
struct Op {
    name: &'static str,
    unit: String,
    /// Operations in one `unit`, for the per-operation column.
    per: usize,
    /// Bytes of `unit` that are *not* the operation, so a row can be priced
    /// against the plain-text row rather than against zero.
    text: usize,
}

fn op(name: &'static str, unit: &str, per: usize, text: usize) -> Op {
    Op {
        name,
        unit: unit.to_string(),
        per,
        text,
    }
}

fn build(o: &Op) -> Vec<u8> {
    let mut v = Vec::with_capacity(TARGET + o.unit.len());
    while v.len() < TARGET {
        v.extend_from_slice(o.unit.as_bytes());
    }
    v
}

fn run(data: &[u8], mode: OpMode, reps: usize) -> f64 {
    let mut store = Storage::boxed();
    let mut t = Term::new(&mut store, mode);
    let mut best = f64::MAX;
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
        }
        let s = t0.elapsed().as_secs_f64();
        // **Minimum, not median.** Every row here is the same shape of work, so
        // the fastest observed run is the one least disturbed by the rest of
        // the machine -- and comparing minima across rows compares like with
        // like. `ingest` quotes medians because it is comparing *builds*, where
        // a lucky minimum would flatter one of them.
        best = best.min(s);
    }
    data.len() as f64 / best / (1 << 20) as f64
}

fn main() {
    let reps: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);

    // 60 columns of text, which is what a line of `cat` output looks like on a
    // 200-column grid, so every row carries a comparable amount of text.
    let text60: String = "abcdefghij".repeat(6);
    let ops = vec![
        op("text (no escapes)", &format!("{text60}\r\n"), 0, 62),
        op("SGR, 16-colour", &format!("\x1b[32m{text60}\r\n"), 1, 62),
        op(
            "SGR, truecolour",
            &format!("\x1b[38;2;10;20;30m{text60}\r\n"),
            1,
            62,
        ),
        op("SGR, reset only", &format!("\x1b[0m{text60}\r\n"), 1, 62),
        op("CUP", &format!("\x1b[10;20H{text60}\r\n"), 1, 62),
        op("EL to end", &format!("\x1b[K{text60}\r\n"), 1, 62),
        op("ED to end", &format!("\x1b[J{text60}\r\n"), 1, 62),
        op("IL", &format!("\x1b[L{text60}\r\n"), 1, 62),
        op("DCH", &format!("\x1b[3P{text60}\r\n"), 1, 62),
        // A scroll, priced by line length. Every row above ends its line, so
        // every row scrolls; these two isolate what the *line* costs by
        // changing only how much text precedes the newline. The difference
        // between them, divided by the 180 columns between them, is the cost
        // of a column of blanking -- and the 20-column row is close to the
        // per-scroll floor.
        op(
            "LF, 20 columns",
            &format!("{}\n", "abcdefghij".repeat(2)),
            1,
            21,
        ),
        op(
            "LF, 200 columns",
            &format!("{}\n", "abcdefghij".repeat(20)),
            1,
            201,
        ),
        op(
            "wide CJK",
            &format!("{}\r\n", "\u{4e00}".repeat(30)),
            30,
            92,
        ),
        op(
            "narrow 2-byte",
            &format!("{}\r\n", "\u{00e9}".repeat(60)),
            60,
            122,
        ),
        op(
            "emoji (astral)",
            &format!("{}\r\n", "\u{1f680}".repeat(30)),
            30,
            122,
        ),
        // **The shapes `cat-cjk.raw` is actually made of.** The rows above are
        // each one character repeated, which is the "payloads that don't
        // exercise the case" trap in miniature: the bulk wide path reaches
        // 1175 MB/s on pure CJK and the recording reaches 380, and nothing
        // above prices the difference. The recording carries 15,204
        // Extended_Pictographic characters, 5,145 of them ZWJ, interleaved
        // with CJK and 39% ASCII -- and every emoji costs one gate refusal,
        // so the interleaving is the cost rather than any single character.
        op(
            "CJK, ascii-interleaved",
            &format!(
                "{} test {} ok\r\n",
                "\u{4e00}".repeat(8),
                "\u{4e8c}".repeat(8)
            ),
            16,
            30,
        ),
        op(
            "CJK, emoji-interrupted",
            &format!(
                "{}\u{1f680}{}\r\n",
                "\u{4e00}".repeat(8),
                "\u{4e8c}".repeat(8)
            ),
            16,
            6,
        ),
        // A ZWJ family: four pictographs joined by three ZWJ, one cluster and
        // two columns. The most expensive shape the cluster machine has.
        op(
            "emoji ZWJ family",
            &format!(
                "{}\r\n",
                "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}".repeat(6)
            ),
            6,
            2,
        ),
        // Combining marks: one base plus one mark, so the cluster machine runs
        // on every character and the cell write is one column.
        op(
            "kana + combining mark",
            &format!("{}\r\n", "\u{304b}\u{3099}".repeat(30)),
            30,
            2,
        ),
    ];

    println!("  one operation at a time, {reps} reps, best of each, 2 MiB per row");
    println!("  ns/op is the row's cost above the plain-text row, per operation\n");
    println!(
        "  {:<20} {:>10} {:>10} {:>12} {:>10}",
        "operation", "runs MB/s", "cells MB/s", "bytes/op", "ns/op"
    );

    let mut base_ns_per_byte = 0.0f64;
    for (i, o) in ops.iter().enumerate() {
        let d = build(o);
        let runs = run(&d, OpMode::Runs, reps);
        let cells = run(&d, OpMode::Computed, reps);
        let ns_per_byte = 1e9 / (cells * (1 << 20) as f64);
        if i == 0 {
            base_ns_per_byte = ns_per_byte;
        }
        let (bytes_per_op, ns) = match o.per {
            0 => (0.0, 0.0),
            n => {
                let unit = o.unit.len() as f64;
                let ops_per_unit = n as f64;
                // Cost of the whole unit, minus what the same bytes of plain
                // text would have cost, spread over the operations in it.
                let unit_ns = unit * ns_per_byte;
                let text_ns = o.text as f64 * base_ns_per_byte;
                (
                    (unit - o.text as f64) / ops_per_unit,
                    (unit_ns - text_ns) / ops_per_unit,
                )
            }
        };
        println!(
            "  {:<20} {:>10.1} {:>10.1} {:>12.1} {:>10.1}",
            o.name, runs, cells, bytes_per_op, ns
        );
    }
}
