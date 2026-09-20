//! Replay a recording exactly as `multibench` does -- `TOTAL` bytes (default
//! 32 MiB), 4096-byte chunks, wrapping the recording -- and compare the two
//! screens at the end. The differential's own test compares at every read
//! boundary of ONE pass; this is what found that a *replayed* pass differs:
//! `less-pager.raw` never leaves the alternate screen, so the second pass
//! re-enters it, and sfte clears on re-entry where baud and alacritty do not.
//!
//!     cargo run --release -p baud-sfte-difftest --example cut -- tests/recordings/ascii/less-pager.raw
use baud_core::{CacheGeometry, Depth, Owned};
use baud_sfte_difftest::*;
fn main() {
    set_locale();
    let path = std::env::args()
        .nth(1)
        .unwrap_or("tests/recordings/ascii/less-pager.raw".into());
    let total: usize = std::env::var("TOTAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32 << 20);
    let bytes = std::fs::read(&path).unwrap();
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(2_000));
    let mut ours = Ours::new(&mut owned);
    let mut theirs = Sfte::new();
    let mut norm = Normalised::new();
    let mut got = 0;
    while got < total {
        let mut o = 0;
        while o < bytes.len() && got < total {
            let n = 4096.min(bytes.len() - o).min(total - got);
            ours.feed(&bytes[o..o + n]);
            theirs.feed(&bytes[o..o + n]);
            o += n;
            got += n;
        }
    }
    let (a, b) = (ours.screen(), theirs.screen(&mut norm));
    let diffs: Vec<_> = a
        .cells
        .iter()
        .zip(b.cells.iter())
        .enumerate()
        .filter(|(_, (x, y))| x != y)
        .collect();
    println!(
        "{path}: total {total}: {} cells differ; cursor ours {:?} sfte {:?}; reply ours {} B sfte {} B; digests {:x} {:x}",
        diffs.len(),
        (a.cursor.0.index(), a.cursor.1.index()),
        (b.cursor.0.index(), b.cursor.1.index()),
        a.reply.len(),
        b.reply.len(),
        a.digest(),
        b.digest()
    );
    for (i, (x, y)) in diffs.iter().take(8) {
        println!(
            "  r{} c{}: ours {:?}/{:?} sfte {:?}/{:?}",
            i / 200,
            i % 200,
            x.ch,
            x.bg,
            y.ch,
            y.bg
        );
    }
}
