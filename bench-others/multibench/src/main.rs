mod cores;
mod counts;
mod equal;
mod ffi;
mod report;
mod screens;
mod transport;
// The guard every measurement in this repository runs first, a file of
// baud-bench's: this workspace links nothing from that one.
#[path = "../../../baud-bench/src/warm.rs"]
mod warm;

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
    warm::refuse_while_warm();
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

    // kitty cannot be a core here (its parser is a CPython object), so it has a
    // crate of its own, `bench-others/kitty-bench`, and a second implementation of
    // these generators would be a second thing to keep in sync. Dump the exact
    // bytes instead, so "same payloads" is a fact rather than a claim.
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
    // and real capture ran at 131-159 MB/s where `baud-bench`'s synthetic
    // `payload::tui` reached 229-240, on `baud-core2` through a PTY
    // (d471493). Comparisons against other implementations were made on the
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

    // **The screen check, before a single timing.** One rep of each
    // recording through baud-core and ghostty, from memory, and the two
    // final screens compared cell for cell and image for image
    // (`equal::check`), and what each stored counted (`counts`). A
    // recording whose screens or counts differ is not timed on any core
    // and prints no rate, and the run fails naming the cells or counters
    // that differ. One whose compared count is "n/a" on an arm is timed,
    // its rate printed unquotable, and fails the run too. Without
    // libghostty-vt there is nothing to compare against, so no recording
    // is timed. ghostty decodes a PNG here as its timed arm
    // does (`run_one`).
    #[cfg(have_ghostty)]
    ffi::ghostty::set_png_decoder(ffi::ghostty::PngDecoder::Registered);
    let screens = [equal::check(&stream, total), equal::check(&tui, total)];
    // Not linked, the check compares nothing: the other arms are timed and
    // ghostty's row says it was not built, so no figure is claimed for it.
    let timed = screens
        .each_ref()
        .map(|c| c.as_ref().is_none_or(equal::Compared::timed));
    let quoted = screens
        .each_ref()
        .map(|c| c.as_ref().is_some_and(equal::Compared::rated));
    // The other cores' screen gates, each recording against baud-core's
    // screen (`screens::for_core`): a core whose screen differs is still
    // timed, so every core keeps its place in the rotation, and its rate
    // is not printed.
    let gates: Vec<[Option<screens::Gate>; 2]> = (0..CORES.len())
        .map(|ci| {
            [
                screens::for_core(ci, &stream, total),
                screens::for_core(ci, &tui, total),
            ]
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
                if timed[pi] {
                    samples[ci][pi].push(run_one(ci, p, total));
                }
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
        if samples[ci]
            .iter()
            .flatten()
            .next()
            .is_some_and(|v| v.is_nan())
        {
            println!(
                "  {:<28} {:>21}  {:>21}",
                label, "n/a (lib not built)", "n/a"
            );
            continue;
        }
        let [st, tu] = &mut samples[ci];
        let gated = |pi: usize| gates[ci][pi].is_none_or(screens::Gate::rated);
        let rate = |pi: usize, v: &mut [f64]| match (timed[pi], gated(pi), quoted[pi]) {
            (false, _, _) => Rate::Untimed,
            (true, false, _) => Rate::Unequal,
            (true, true, true) => Rate::Quoted(summarize(v)),
            (true, true, false) => Rate::Unquotable(summarize(v)),
        };
        row(label, rate(0, st), rate(1, tu));
        if let [Some(s), Some(t)] = gates[ci] {
            println!(
                "  {:<28} {}; {}",
                "",
                s.says(&labels[0]),
                t.says(&labels[1])
            );
        }
        // Each image arm's counters from the screen check's rep, under its
        // row.
        let arm = match ci {
            1 => Some(equal::Arm::Baud),
            3 => Some(equal::Arm::Ghostty),
            _ => None,
        };
        if let Some(arm) = arm {
            let counters = |c: &Option<equal::Compared>| {
                c.as_ref()
                    .map_or_else(|| "n/a".to_string(), |c| c.counters(arm))
            };
            println!(
                "  {:<28} {}: {}; {}: {}",
                "",
                labels[0],
                counters(&screens[0]),
                labels[1],
                counters(&screens[1])
            );
        }
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

    // The screen check's verdict on each recording; a recording whose
    // screens or counts differ fails the run, unless equal::UNEQUAL names it
    // and why.
    println!();
    let mut failed = false;
    for (c, label) in screens.iter().zip(&labels) {
        match c {
            Some(c) => match (c.rated(), equal::known_unequal(label)) {
                // Known to differ, and still differing: no rate, the reason
                // said, and the run goes on.
                (false, Some(why)) => {
                    println!("  {label}: UNEQUAL, no rate: {why}");
                    for line in c.report(label) {
                        println!("    {line}");
                    }
                }
                (true, Some(_)) => {
                    failed = true;
                    println!(
                        "  {label}: listed as UNEQUAL, but the screens and counts agree: \
                         take it out of equal::UNEQUAL"
                    );
                }
                (rated, None) => {
                    failed |= !rated;
                    for line in c.report(label) {
                        println!("  {line}");
                    }
                }
            },
            None => {
                println!(
                    "  {label}: ghostty not linked (no GHOSTTY_LIB_DIR): no screen check, \
                     and no figure here claims ghostty's"
                );
            }
        }
    }
    if failed {
        std::process::exit(1);
    }
}
