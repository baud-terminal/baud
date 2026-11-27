//! **What a terminal's storage costs in memory**, for one shape, one depth
//! and one mode a run: `memory <cols> <rows> <depth> <single|banded>`.
//!
//! Printed for each: what the storage allocates for its cells, its
//! rendition tables and its generations (`Owned::footprint`), and the
//! resident anonymous memory (`/proc/self/smaps`, Rss over the mappings
//! with no file) after the storage is made, after real recordings have
//! (cat-scroll and git-log four times, then btop, which stays up) filled
//! its history, and
//! after one change of width -- with the words the
//! pool's generations held at most. One case a process, so no case reads
//! another's pages. Banded, the band is the most rows its table holds.

use baud_core::{BandRows, Footprint, Owned, Provision, Term};
use baud_domain::{BytePos, Depth, ReadLen, Rows, WinSize};

fn recording(name: &str) -> Vec<u8> {
    let p = format!(
        "{}/../../tools/tests/recordings/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for piece in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

/// Resident anonymous memory, in KiB: the Rss of every mapping in
/// `/proc/self/smaps` that names no file.
fn anon_rss_kib() -> u64 {
    let smaps = std::fs::read_to_string("/proc/self/smaps").expect("/proc/self/smaps");
    let mut anon = false;
    let mut kib = 0;
    for line in smaps.lines() {
        let mut words = line.split_whitespace();
        let first = words.next().unwrap_or("");
        if first.contains('-') && first.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            let path = line.split_whitespace().nth(5).unwrap_or("");
            anon = path.is_empty() || path == "[heap]";
        } else if first == "Rss:" && anon {
            kib += words
                .next()
                .and_then(|n| n.parse::<u64>().ok())
                .unwrap_or(0);
        }
    }
    kib
}

fn mib(bytes: usize) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

fn kib_mib(kib: u64) -> String {
    format!("{:.1}", kib as f64 / 1024.0)
}

fn main() {
    baud_bench::warm::refuse_while_warm();
    let mut args = std::env::args().skip(1);
    let mut next = |what: &str| {
        args.next()
            .unwrap_or_else(|| panic!("memory <cols> <rows> <depth> <single|banded>: no {what}"))
    };
    let cols: u16 = next("cols").parse().expect("cols");
    let rows: u16 = next("rows").parse().expect("rows");
    let depth = Depth::of(next("depth").parse::<usize>().expect("depth"));
    let mode = next("mode");
    let ws = WinSize {
        ws_col: cols,
        ws_row: rows,
    };
    let geom = baud_bench::host_geometry();
    let before = anon_rss_kib();
    let p = match mode.as_str() {
        "single" => {
            Provision::of(&ws).unwrap_or_else(|_| panic!("{cols}x{rows} is past one table"))
        }
        "banded" => Provision::banded(
            &ws,
            BandRows::of(Rows::of_index(usize::from(rows) + depth.lines())),
        )
        .unwrap_or_else(|_| panic!("{cols}x{rows} is refused banded")),
        m => panic!("mode {m}: single or banded"),
    };
    let mut owned = Owned::with_history(p, &geom, depth);
    let Footprint {
        cells,
        tables,
        pool,
    } = owned.footprint();
    let band = owned.band_rows().rows().index();
    let made = anon_rss_kib();
    let (fed, resized, peak) = {
        let mut t = Term::new(owned.storage(), &ws);
        // cat-scroll and git-log fill the primary's history, git-log in
        // colour; then btop, which leaves its alternate screen up, as a
        // window is resized while a program runs on it.
        let cat = recording("ascii/cat-scroll.raw");
        let log = recording("ascii/git-log.raw");
        for _ in 0..4 {
            feed(&mut t, &cat);
            feed(&mut t, &log);
        }
        feed(&mut t, &recording("utf8/btop.raw"));
        let fed = anon_rss_kib();
        let narrower = WinSize {
            ws_col: cols - cols / 16,
            ws_row: rows,
        };
        let _ = t.resize(&narrower);
        let resized = anon_rss_kib();
        (fed, resized, t.frozen().counts.words_peak)
    };
    println!(
        "{cols}x{rows} depth {} {mode} band {band}: allocated cells {} tables {} pool {} MiB; \
         resident +{} made, +{} fed, +{} resized MiB; pool words at most {peak} ({} MiB)",
        depth.lines(),
        mib(cells.index()),
        mib(tables.index()),
        mib(pool.index()),
        kib_mib(made - before),
        kib_mib(fed - before),
        kib_mib(resized - before),
        mib(peak * core::mem::size_of::<baud_domain::StyleWord>()),
    );
}
