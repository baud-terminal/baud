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

    let data = match which.as_str() {
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
    let mut t = Term::new(Storage::new(&mut arena, &mut vp, &mut used), &ws);

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
            let n = READ_CHUNK.min(data.len() - at);
            t.write_at(BytePos::ZERO, &data[at..at + n]);
            t.feed(BytePos::ZERO, ByteLen::from_index(n));
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
