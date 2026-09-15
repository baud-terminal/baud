//! **What one character shape costs, rebuilt for `baud-core`.**
//!
//! `CLAUDE.md` carries a table of per-shape figures -- a wide CJK character at
//! 1.3 ns, a ZWJ family at 52.2, a kana-plus-mark pair at 20.6 -- and says of
//! them: "**the binary is gone**, deleted with `baud-core2`, whose `Storage` and
//! `OpMode` it drove ... They are kept because the *ordering* is what they were
//! ever good for ... A replacement would be a `baud-core` harness that times one
//! shape in a loop, and it should be written before any of those numbers is
//! quoted as current."
//!
//! This is that harness. The old figures are `baud-core2`'s and are not
//! comparable to these; what carries across is the question -- **which shapes
//! are expensive, and in what order** -- because that ordering is what the
//! architecture was reasoned from.
//!
//! **Read the caveat before quoting a row.** The file's own warning about the
//! original applies unchanged: "`opcost`'s per-operation rows cannot resolve a
//! few nanoseconds. Its emoji ZWJ row read 50.6, 60.5 and 63.4 ns across builds
//! including one whose source was unchanged. It ranks shapes against each other
//! within one run; it does not compare builds." Two arms of a change belong in
//! `ingest2`, interleaved, with an A/A control.
//!
//! **The payloads are synthetic and say so.** This project's rule is to use real
//! recordings for throughput, and these are not throughput figures: they are one
//! shape repeated, to price that shape. A recording mixes shapes, which is
//! exactly what this cannot answer.

use baud_core::*;
use std::time::Instant;

/// One shape: how to build it, and what a single "operation" of it is.
struct Shape {
    name: &'static str,
    /// The unit repeated to fill the payload.
    unit: &'static str,
    /// What the per-operation column divides by -- a character, a cluster, a
    /// pair. Naming it per shape is the whole reason the old table's rows are
    /// labelled "a cluster" and "a pair" rather than sharing a unit.
    per: &'static str,
    /// How many of `per` one `unit` contains.
    ops: usize,
}

const SHAPES: &[Shape] = &[
    Shape {
        name: "plain ASCII text",
        unit: "the quick brown fox jumps over it ",
        per: "character",
        ops: 34,
    },
    Shape {
        name: "wide CJK, pure run",
        unit: "日本語版世界百科事典",
        per: "character",
        ops: 10,
    },
    Shape {
        name: "CJK interleaved with ASCII",
        unit: "日a本b語c版d世e",
        per: "character",
        ops: 10,
    },
    Shape {
        name: "CJK interrupted by one emoji",
        unit: "日本語版世🚀界百科事",
        per: "character",
        ops: 10,
    },
    Shape {
        name: "emoji ZWJ family",
        unit: "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{200d}\u{1f466}",
        per: "cluster",
        ops: 1,
    },
    Shape {
        name: "kana + combining mark",
        unit: "\u{304b}\u{3099}\u{304d}\u{3099}\u{304f}\u{3099}",
        per: "pair",
        ops: 3,
    },
    Shape {
        name: "narrow 2-byte",
        unit: "ÀÁÂÃÄÅÆÇÈÉÊËÌÍÎÏ",
        per: "character",
        ops: 16,
    },
];

/// ~1 MB of one shape, cut on a unit boundary so no character is split.
fn payload(s: &Shape) -> Vec<u8> {
    let u = s.unit.as_bytes();
    let reps = (1 << 20) / u.len();
    let mut v = Vec::with_capacity(reps * u.len());
    for _ in 0..reps {
        v.extend_from_slice(u);
    }
    v
}

fn main() {
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let reps: usize = baud_bench::args()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);

    if let Ok(o) = std::process::Command::new("pmset")
        .arg("-g")
        .arg("batt")
        .output()
    {
        if String::from_utf8_lossy(&o.stdout).contains("Battery Power") {
            println!("BATTERY -- not quotable, re-run on mains");
        }
    }
    println!("per-shape cost, {reps} reps of ~1 MB each -- synthetic payloads, one shape apiece");
    println!("ranks shapes within this run; does NOT compare builds\n");
    println!("  {:<30} {:>9}  {:>10}  unit", "shape", "MB/s", "per op");

    for s in SHAPES {
        let data = payload(s);
        let units = data.len() / s.unit.len();
        let ops = units * s.ops;

        // Warm-up, discarded: this project's first-round rule.
        let mut best = 0.0f64;
        for round in 0..reps {
            let mut store = Owned::new(&ws);
            let mut t = Term::new(store.storage(), &ws);
            let t0 = Instant::now();
            let mut at = 0usize;
            while at < data.len() {
                let n = READ_CHUNK.min(data.len() - at);
                t.write_at(BytePos::ZERO, &data[at..at + n]);
                t.feed(BytePos::ZERO, ReadLen::from_index(n));
                at += n;
            }
            let secs = t0.elapsed().as_secs_f64();
            std::hint::black_box(t.cursor());
            if round > 0 {
                let mbs = data.len() as f64 / secs / (1 << 20) as f64;
                if mbs > best {
                    best = mbs;
                }
            }
        }
        // ns per operation, from the best round: a slower round is the machine
        // doing something else, and the question is what the shape costs.
        let secs_per_mb = 1.0 / (best * (1 << 20) as f64);
        let ns = secs_per_mb * data.len() as f64 / ops as f64 * 1e9;
        println!("  {:<30} {best:>9.1}  {ns:>7.2} ns  a {}", s.name, s.per);
    }
}
