//! Feed sfte alone, the way `multibench` does, and report MiB/s -- the arm to
//! profile (`sample <pid>`) and the one that priced the damage-marking
//! ceiling: on `cat-scroll.raw` the base build reads ~12 MiB/s and
//! `SFTE_CEILING=no-dirty-rows cargo build ...` reads ~80, with the oracle's
//! tests green under the arm because the cells are byte-identical.
//!
//!     cargo run --release -p baud-sfte-difftest --example sfte_only -- tools/tests/recordings/ascii/cat-scroll.raw
use baud_sfte_difftest::*;
fn main() {
    set_locale();
    let path = std::env::args()
        .nth(1)
        .unwrap_or("tools/tests/recordings/ascii/less-pager.raw".into());
    let total: usize = std::env::var("TOTAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8 << 20);
    let bytes = std::fs::read(&path).unwrap();
    let mut t = Sfte::new();
    let t0 = std::time::Instant::now();
    let mut got = 0;
    while got < total {
        let mut o = 0;
        while o < bytes.len() && got < total {
            let n = 4096.min(bytes.len() - o).min(total - got);
            t.feed(&bytes[o..o + n]);
            o += n;
            got += n;
        }
    }
    let s = t0.elapsed().as_secs_f64();
    println!(
        "{path}: {} MiB in {:.2} s = {:.1} MiB/s",
        total >> 20,
        s,
        got as f64 / s / 1048576.0
    );
}
