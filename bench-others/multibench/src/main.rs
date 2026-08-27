mod cores;
mod ffi;
mod report;
mod transport;

use cores::*;

use report::*;
use transport::*;

const COLS: usize = 200;

const ROWS: usize = 50;

const CORES: &[&str] = &[
    "ceiling (discard, no parse)",
    "baud (spans)",
    "alacritty_terminal",
    "ghostty (libghostty-vt)",
    "vt100 crate",
    "wezterm (full terminal)",
    "termwiz (wezterm) parse only",
    "baud (cells, computed)",
    "baud (cells, templated)",
];

fn main() {
    let total = 32usize << 20;

    let mut stream = Vec::new();
    const W: [&str; 8] = [
        "static", "inline", "struct", "return", "const", "size_t", "render", "parse",
    ];
    let mut col = 0;
    while stream.len() < (1 << 19) - 100 {
        if col > 70 {
            stream.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        if stream.len() % 37 == 0 {
            stream.extend_from_slice(b"\x1b[32m");
            continue;
        }
        let w = W[stream.len() % 8].as_bytes();
        stream.extend_from_slice(w);
        col += w.len();
        stream.push(b' ');
        col += 1;
    }
    // Pure printable text, no escapes at all. The streaming payload above still
    // emits SGR every 37 bytes, so it exercises the state machine as well as the
    // scanner. This one is scanner-only, and is the control for "is the gap
    // between a scalar and a vectorised scanner?" -- if a lead widens here
    // relative to `stream`, scanning is where it comes from.
    if std::env::var("PURE").is_ok() {
        stream.clear();
        let mut col = 0;
        while stream.len() < (1 << 19) - 100 {
            if col > 70 {
                stream.extend_from_slice(b"\r\n");
                col = 0;
                continue;
            }
            let w = W[stream.len() % 8].as_bytes();
            stream.extend_from_slice(w);
            col += w.len();
            stream.push(b' ');
            col += 1;
        }
    }

    let mut tui = Vec::new();
    let mut s = 0u64;
    while tui.len() < (1 << 19) - 200 {
        tui.extend_from_slice(format!("\x1b[{};{}H", s % 50 + 1, 1 + (s * 37) % 120).as_bytes());
        for _ in 0..6 {
            tui.extend_from_slice(b"pub struct ")
        }
        tui.extend_from_slice(b"\x1b[K");
        s += 1;
    }

    // The kitty harness cannot be a Rust core (its parser is a CPython object),
    // so it runs in Python — and a second implementation of these generators
    // would be a second thing to keep in sync. Dump the exact bytes instead, so
    // "same payloads" is a fact rather than a claim.
    if let Ok(dir) = std::env::var("DUMP_PAYLOADS") {
        std::fs::write(format!("{dir}/stream.bin"), &stream).expect("write stream.bin");
        std::fs::write(format!("{dir}/tui.bin"), &tui).expect("write tui.bin");
        eprintln!(
            "wrote {}/stream.bin ({} B) and {}/tui.bin ({} B)",
            dir,
            stream.len(),
            dir,
            tui.len()
        );
        return;
    }

    const REPS: usize = 9;

    // Interleaved and rotated, not blocked. Rep-major order means thermal drift
    // reaches every core equally; rotating the starting index by the rep number
    // means no core keeps a fixed position across the run. Blocked ordering is
    // what let a row inserted above baud move it 16%.
    let payloads: [&Vec<u8>; 2] = [&stream, &tui];
    let mut samples = vec![[Vec::with_capacity(REPS), Vec::with_capacity(REPS)]; CORES.len()];
    for rep in 0..REPS {
        for k in 0..CORES.len() {
            let ci = (k + rep) % CORES.len();
            for (pi, p) in payloads.iter().enumerate() {
                samples[ci][pi].push(run_one(ci, p, total));
            }
        }
    }

    println!(
        "  same payloads, {}, {CHUNK}-byte reads, median of {REPS}, interleaved and rotated\n",
        if from_memory() {
            "FROM MEMORY (no PTY, no syscalls)"
        } else {
            "through a real PTY"
        }
    );
    println!(
        "  {:<28} {:>21}  {:>21}",
        "core", "streaming median [min-max]", "TUI median [min-max]"
    );
    for (ci, label) in CORES.iter().enumerate() {
        if samples[ci][0][0].is_nan() {
            println!(
                "  {:<28} {:>21}  {:>21}",
                label, "n/a (lib not built)", "n/a"
            );
            continue;
        }
        let st = summarize(&mut samples[ci][0]);
        let tu = summarize(&mut samples[ci][1]);
        row(label, st, tu);
    }
}
