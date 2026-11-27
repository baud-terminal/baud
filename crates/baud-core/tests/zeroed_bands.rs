//! **A band's untouched slots cost no pages**: a banded storage's rendition
//! tables are resident only where a rendition was written, after the
//! storage is made, after a change of width, and after one band is swept.
//! Read from `/proc/self/smaps`, the Rss of the mappings that name no file,
//! so Linux only; the same storage elsewhere is unmeasured.
#![cfg(target_os = "linux")]

use baud_core::*;
use baud_domain::{BytePos, Depth, ReadLen, Rows};

/// Resident anonymous memory, in KiB: the Rss of every mapping in
/// `/proc/self/smaps` that names no file.
fn anon_rss_kib() -> usize {
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
                .and_then(|n| n.parse::<usize>().ok())
                .unwrap_or(0);
        }
    }
    kib
}

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

const KIB: usize = 1024;

#[test]
fn a_band_s_untouched_slots_are_not_resident() {
    let ws = WinSize {
        ws_col: 960,
        ws_row: 254,
    };
    let depth = Depth::of(2_000);
    let p = Provision::banded(&ws, BandRows::of(Rows::of_index(254 + 2_000)))
        .expect("960x254 is admitted banded");
    let before = anon_rss_kib();
    let mut owned = Owned::with_history(p, &CacheGeometry::new(None, None, None, None), depth);
    let Footprint { cells, tables, .. } = owned.footprint();
    let (cells, tables) = (cells.index() / KIB, tables.index() / KIB);

    // Made: the cells, and of each band's table only the page its default
    // is seeded in, the log's first page and the book.
    let made = anon_rss_kib() - before;
    assert!(
        made <= cells + tables / 16 + KIB,
        "made: {made} KiB resident for {cells} KiB of cells and {tables} KiB of tables"
    );

    let mut t = Term::new(owned.storage(), &ws);
    let (cat, log) = (
        recording("ascii/cat-scroll.raw"),
        recording("ascii/git-log.raw"),
    );
    for _ in 0..4 {
        feed(&mut t, &cat);
        feed(&mut t, &log);
    }
    let narrower = WinSize {
        ws_col: ws.ws_col - ws.ws_col / 16,
        ws_row: ws.ws_row,
    };
    assert_ne!(t.resize(&narrower), Resized::BandsUnlent);
    let peak = t.frozen().counts.words_peak * core::mem::size_of::<baud_domain::StyleWord>() / KIB;

    // Resized: the tables the history's few renditions were written in,
    // emptied and filled again, not every slot of every band.
    let resized = anon_rss_kib() - before;
    assert!(
        resized <= cells + peak + tables / 8 + 2 * KIB,
        "resized: {resized} KiB resident for {cells} KiB of cells, {peak} KiB of \
         generations and {tables} KiB of tables"
    );

    // One band swept: distinct renditions over one row until its band's
    // table is full, so that table is resident and the rest stay as they
    // were. The control that the probe can see a table become resident.
    let sweeps = t.style_reclaims().sweeps;
    let mut bytes = Vec::new();
    for i in 0..40_000u32 {
        if i % 900 == 0 {
            bytes.push(b'\r');
        }
        let [_, r, g, b] = i.to_be_bytes();
        bytes.extend_from_slice(format!("\x1b[38;2;{r};{g};{b}mx").as_bytes());
    }
    let at = anon_rss_kib();
    feed(&mut t, &bytes);
    let swept = anon_rss_kib() - at;
    assert!(
        t.style_reclaims().sweeps > sweeps,
        "40,000 renditions over one row sweep its band"
    );
    let band = tables / owned_bands(tables);
    assert!(
        swept >= band / 2 && swept <= band + KIB,
        "swept: {swept} KiB made resident for one band's {band} KiB table"
    );
}

/// The bands a storage's tables are split into: each holds a table, a log
/// and a reach of `BAND_SLOTS` slots and a book.
fn owned_bands(tables_kib: usize) -> usize {
    let slot = core::mem::size_of::<baud_domain::StyleSlot>()
        + core::mem::size_of::<baud_domain::StyleId>()
        + core::mem::size_of::<baud_domain::Reach>();
    (tables_kib * KIB / (BAND_SLOTS.0 * slot)).max(1)
}
