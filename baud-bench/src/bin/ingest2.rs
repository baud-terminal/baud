//! The same measurement as `ingest`, driving the **new** core.
//!
//! Same payloads, same rep protocol, same self-timing, same power-source
//! report -- so the two numbers are comparable and the only difference is
//! which core parsed the bytes. `ingest` drives `baud-core2`.
//!
//! ```text
//! ingest2 cat-cjk 250
//! ```

use baud_core::*;

fn recording(name: &str) -> Vec<u8> {
    let p = format!("{}/../tests/recordings/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// A number recorded on battery should be re-run: the OS is free to move the
/// clock there, and it does.
fn power_source() -> String {
    std::process::Command::new("pmset")
        .args(["-g", "ps"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| match s.contains("AC Power") {
            true => "AC Power".into(),
            false => "BATTERY -- re-run on mains".into(),
        })
        .unwrap_or_else(|| "unknown".into())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let which = args.next().unwrap_or_else(|| "cat-cjk".into());
    let reps: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);

    // **Synthetic ladder, for finding the ceiling rather than comparing.**
    // Each rung adds exactly one thing to the one below it, so the step that
    // costs can be named instead of inferred. Never quote these against
    // another implementation -- they are not recordings and they resemble no
    // program.
    let mb: usize = 16;
    // A table rather than arithmetic on a byte literal: casting the index
    // down to a byte would spend this crate's raw-integer budget on a
    // benchmark payload generator.
    const LETTERS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";
    let ladder = |kind: &str| -> Vec<u8> {
        let n = mb << 20;
        let mut v = Vec::with_capacity(n);
        let mut i = 0usize;
        while v.len() < n {
            match kind {
                // Printable ASCII only: one route, one bulk writer, one cell a
                // byte, no wrap for 200 columns at a time, no scroll.
                "flat" => v.push(LETTERS[i % 26]),
                // The same, cut into 200-column lines: adds the wrap.
                "wrap" => v.push(match i % 200 {
                    199 => b'\r',
                    _ => LETTERS[i % 26],
                }),
                // Lines ending in CRLF: adds the scroll and its blanking.
                "lines" => match i % 80 {
                    78 => v.push(b'\r'),
                    79 => v.push(b'\n'),
                    _ => v.push(LETTERS[i % 26]),
                },
                // Lines with one SGR each: adds a control sequence.
                _ => match i % 80 {
                    0 => v.extend_from_slice(b"\x1b[32m"),
                    78 => v.push(b'\r'),
                    79 => v.push(b'\n'),
                    _ => v.push(LETTERS[i % 26]),
                },
            }
            i += 1;
        }
        v.truncate(n);
        v
    };

    let data = match which.as_str() {
        "flat" | "wrap" | "lines" | "sgr" => ladder(&which),
        // **Every recording, concatenated, unmodified and in order.** Not a
        // new payload: each byte is a byte some program really sent, and
        // nothing is filtered, truncated or generated. It exists to answer
        // whether a single 300-500 KB recording replayed a hundred times is
        // representative -- one stream with eight programs' worth of distinct
        // sequences, styles and scripts puts far more pressure on the tables
        // than any one of them does.
        "mix" => {
            let mut v = Vec::new();
            for name in [
                "ascii/cat-scroll.raw",
                "utf8/btop.raw",
                "ascii/git-log.raw",
                "utf8/cat-cjk.raw",
                "ascii/asciiquarium.raw",
                "utf8/vim-unicode.raw",
                "ascii/less-pager.raw",
                "utf8/less-cjk.raw",
            ] {
                v.extend_from_slice(&recording(name));
            }
            v
        }
        // **A spectrum analyser: dense redraw, block glyphs, and the one
        // recording in the corpus that sets a scrolling region.** One escape
        // per 12.1 bytes, 19.5% non-ASCII, 51,954 control sequences of which
        // 24% are absolute cursor moves -- and 83 `DECSTBM`s, which is the
        // shape `CLAUDE.md` asks for before the row map is built again.
        "cava" => recording("utf8/cava.raw"),
        "btop" => recording("utf8/btop.raw"),
        "asciiquarium" => recording("ascii/asciiquarium.raw"),
        "vim" => recording("utf8/vim-unicode.raw"),
        "cat" => recording("ascii/cat-scroll.raw"),
        "less" => recording("ascii/less-pager.raw"),
        "git-log" => recording("ascii/git-log.raw"),
        "cat-cjk" => recording("utf8/cat-cjk.raw"),
        "less-cjk" => recording("utf8/less-cjk.raw"),
        other => recording(other),
    };

    // 200 x 50, which is the geometry the recordings were captured at.
    let ws = WinSize {
        ws_row: 50,
        ws_col: 200,
    };
    let (_, rows) = ws.geometry();
    let span = RowBlock::leading(rows, Cols::from_stride(MAX_COLS));
    let mut arena = vec![0u8; ARENA];
    let mut vp = vec![PackedCell::EMPTY; span.len()];
    let mut used = vec![RowExtent::ZERO; rows.index()];
    // **An empty table is how this benchmark declines it.** The only read is
    // behind `cell-table`; passing the full 2 MiB otherwise costs 262,144
    // property-table lookups and 2 MiB of stores at construction, for a table
    // no rep will consult.
    #[cfg(feature = "cell-table")]
    let mut cells3 = vec![PackedCell::EMPTY; CELLS3];
    #[cfg(not(feature = "cell-table"))]
    let mut cells3: Vec<PackedCell> = Vec::new();
    let mut tmpl = vec![None; NSTYLE];
    let mut tabs = vec![false; MAX_COLS];
    let mut reply = vec![0u8; REPLY];
    let mut alt = vec![PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![RowExtent::ZERO; rows.index()];
    let mut t = Term::new(
        Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
        ),
        &ws,
    );

    // **The chunk is written into the arena and then parsed, which is what a
    // `read(2)` does.** Parsing the recording in place instead -- placed once,
    // never re-copied -- was tried and is *slower*: 509-518 against 517-525,
    // because 327 KB exceeds a 128 KB L1 and every block takes misses, where a
    // real read leaves its 4096 bytes hot. The copy is not a cost baud pays in
    // production, but it reproduces the cache state baud parses in, and the
    // faster number is the one that models nothing.
    //
    // The parse itself is zero-copy either way: `feed` borrows the arena and
    // `put_ascii_run` blits straight out of it.
    let mut fed = 0usize;
    let mut per_rep = Vec::with_capacity(reps);
    for _ in 0..reps {
        let t0 = std::time::Instant::now();
        let mut at = 0usize;
        while at < data.len() {
            // `READ_CHUNK` at a time, because where the read boundaries fall
            // is part of what is being measured: a whole-file `feed` would
            // give the block loop a run no `read(2)` ever hands it.
            let chunk: usize = std::env::var("CHUNK")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(READ_CHUNK);
            let n = chunk.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            #[cfg(not(feature = "harness-only"))]
            t.feed(BytePos::ZERO, ReadLen::from_index(n));
            at += n;
            fed += n;
        }
        per_rep.push((t0.elapsed().as_secs_f64(), at));
    }

    let mut rates: Vec<f64> = per_rep
        .iter()
        .map(|(s, n)| *n as f64 / *s / (1 << 20) as f64)
        .collect();
    rates.sort_by(|a, b| a.partial_cmp(b).expect("no NaN: every rep took time"));
    let (lo, med, hi) = (rates[0], rates[rates.len() / 2], rates[rates.len() - 1]);
    let (bytes_in, cells) = t.amplification();
    println!(
        "{which} new: {med:.1} MB/s [{lo:.1}-{hi:.1}] over {reps} reps, {fed} bytes, {}",
        power_source()
    );
    println!(
        "  cells {cells} written ({:.1}x per byte)",
        cells as f64 / bytes_in as f64
    );
    #[cfg(feature = "diag")]
    {
        // **Bytes an iteration is the unit the loop is designed against.**
        // A number for it belongs beside the throughput, not derived from a
        // run histogram afterwards.
        println!(
            "  loop {} iterations, {:.2} bytes/iter, {:.2} cells/iter",
            t.iters,
            bytes_in as f64 / t.iters as f64,
            cells as f64 / t.iters as f64,
        );
        println!(
            "  scrolls: {} rotated, {} copied ({:.0}% copied)",
            t.scroll_rot,
            t.scroll_copy,
            100.0 * t.scroll_copy as f64 / (t.scroll_rot + t.scroll_copy).max(1) as f64,
        );
        println!(
            "  accelerated CSI: {} tried, {} committed ({:.0}% declined)",
            t.fast_try,
            t.fast_hit,
            100.0 * (t.fast_try - t.fast_hit) as f64 / t.fast_try.max(1) as f64,
        );
        #[cfg(feature = "sgr-table")]
        println!("  SGR from the byte table: {} hits", t.sgr_hit,);
        println!(
            "  table hits {} of {} single-character runs, {} runs total",
            t.table_hits,
            t.runs[1],
            t.runs.iter().sum::<usize>(),
        );
        const NODES: [&str; 10] = [
            "Ground",
            "Utf8One",
            "Utf8Two",
            "Utf8Three",
            "Escape",
            "CsiParam",
            "CsiInter",
            "CsiIgnore",
            "Str",
            "StrEsc",
        ];
        // **Names come from the core, not from a list kept here.** Two
        // parallel arrays in this file drifted from the enums in one session
        // and reported nodes and actions that cannot occur.
        print!("  iterations by node:");
        for (name, n) in t.hist.named_nodes() {
            if n > 0 {
                print!(" {name}:{:.1}%", 100.0 * n as f64 / t.iters as f64);
            }
        }
        println!();
        print!("  iterations by action:");
        for (name, n) in t.hist.named_acts() {
            if 100.0 * n as f64 / t.iters as f64 >= 1.0 {
                print!(" {name}:{:.1}%", 100.0 * n as f64 / t.iters as f64);
            }
        }
        println!();
        let tot: usize = t.runs.iter().sum();
        print!("  runs by character count:");
        for (k, n) in t.runs.iter().enumerate() {
            print!(" {k}:{:.0}%", 100.0 * *n as f64 / tot as f64);
        }
        println!();
        print!("  runs by encoded length:");
        for (k, n) in t.run_len.iter().enumerate().skip(2) {
            print!(" {k}B:{:.0}%", 100.0 * *n as f64 / tot as f64);
        }
        println!();
    }
}
