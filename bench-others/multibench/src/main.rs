mod cores;
mod ffi;
mod report;
mod transport;

use cores::*;

use report::*;
use transport::*;

const COLS: usize = 200;

const ROWS: usize = 50;

// **Position is the arm**: `cores::run_one` matches on this index, so the order
// here and the order there are one fact spelled twice. `baud-core2`'s three
// entries -- spans, cells-computed, cells-templated -- went with the crate, and
// what was "baud-core (new)" at index 9 is index 1 now. There is one baud, so it
// no longer needs a qualifier.
const CORES: &[&str] = &[
    "ceiling (discard, no parse)",
    "baud-core",
    "alacritty_terminal",
    "ghostty (libghostty-vt)",
    "vt100 crate",
    "wezterm (full terminal)",
    "termwiz (wezterm) parse only",
    // **Index 7, and adding it re-took the whole table.** The sweep rotates
    // the starting core by the rep number, so an eighth entry changes every
    // core's position distribution -- measured at 16% for baud when a row
    // was inserted above it -- and no sfte row is ever printed beside a baud
    // row from a previous run.
    "sfte (C11, via baud-sfte-difftest)",
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

    // **Real capture, when asked for.** The two payloads above are synthetic,
    // and `docs/record.md` ("Replay real capture instead of `payload::tui`")
    // records that real capture ran at 131-159 MB/s where `baud-bench`'s
    // synthetic `payload::tui` reached 229-240, on `baud-core2` through a PTY
    // (9775b79). Comparisons against other implementations were made on the
    // synthetic pair above until this switch, which made those ratios a
    // statement about synthetic input.
    //
    // `RECORDINGS="a.raw,b.raw"` swaps in two files from
    // `tests/recordings/`, so the same interleaved, rotated harness
    // drives every core with bytes a real program actually emitted. The names
    // are printed in the header, because a table whose columns are "streaming"
    // and "TUI" while holding something else is worse than no table.
    let (stream, tui, labels) = match std::env::var("RECORDINGS") {
        Ok(v) => {
            let names: Vec<&str> = v.split(',').map(str::trim).collect();
            assert_eq!(names.len(), 2, "RECORDINGS wants exactly two names");
            let load = |n: &str| {
                let p = format!("{}/../../tests/recordings/{n}", env!("CARGO_MANIFEST_DIR"));
                std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
            };
            let (a, b) = (load(names[0]), load(names[1]));
            let l = [
                names[0].rsplit('/').next().unwrap_or(names[0]).to_string(),
                names[1].rsplit('/').next().unwrap_or(names[1]).to_string(),
            ];
            (a, b, l)
        }
        Err(_) => (stream, tui, ["streaming".into(), "TUI redraw".into()]),
    };

    const REPS: usize = 9;

    // **sfte's locale is process-global and asserted once.** Its width is
    // libc `wcwidth`, and `Sfte::new` does not set the locale itself because
    // `setlocale` is not per-context; a wrong table here is a different
    // program on every UTF-8 row, silently.
    let locale = baud_sfte_difftest::set_locale();

    // **The admissibility gate, before a single timing.** One full rep of the
    // same replay through baud-core and sfte, and their screen digests must
    // agree for the sfte row to be like-for-like on that payload. It is run
    // first and printed in the row, so a reader cannot take a ratio off a row
    // whose two arms drew different screens.
    let admissible: Vec<bool> = [&stream, &tui]
        .iter()
        .map(|p| {
            let (a, b) = sfte_admissible(p, total);
            a == b
        })
        .collect();

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
        "  same payloads, {}, {CHUNK}-byte reads, median of {REPS}, interleaved and rotated",
        if from_memory() {
            "FROM MEMORY (no PTY, no syscalls)"
        } else {
            "through a real PTY"
        }
    );
    println!(
        "  scrollback: 10,000 lines on every core but sfte, which is compiled at {}; sfte locale {locale}\n",
        SFTE_SCROLLBACK
    );
    println!(
        "  {:<28} {:>21}  {:>21}",
        "core",
        format!("{} median [min-max]", labels[0]),
        format!("{} median [min-max]", labels[1])
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
        if ci == 7 {
            // The gate's verdict, under the row it governs: a payload on
            // which the two arms drew different screens is named, and its
            // sfte figure is a figure for a different program.
            let verdict = |ok: bool, name: &str| {
                if ok {
                    format!("{name}: same screen as baud-core")
                } else {
                    format!("{name}: DIFFERENT SCREEN -- not like-for-like")
                }
            };
            println!(
                "  {:<28} {}; {}",
                "",
                verdict(admissible[0], &labels[0]),
                verdict(admissible[1], &labels[1])
            );
        }
    }
}
