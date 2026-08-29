#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! Performance floors. Not a benchmark — a regression alarm. Thresholds are set
//! well below measured values so they fail on an architectural mistake (a copy
//! reintroduced, the arena grown past cache) rather than on machine variance.
use baud_core2::*;
use std::time::Instant;

/// `Storage::zeroed()` builds a ~1.7 MB value on the stack before `Box::new`
/// moves it. Fine for `static` placement on bare metal, fatal on a test thread.
/// Until the core gains an in-place constructor, allocate zeroed heap directly.
fn boxed_storage() -> Box<Storage> {
    Storage::boxed()
}

fn payload(n: usize) -> Vec<u8> {
    const W: [&str; 8] = [
        "static", "inline", "struct", "return", "const", "size_t", "render", "parse",
    ];
    let mut b = Vec::with_capacity(n);
    let mut col = 0;
    while b.len() < n {
        if col > 70 {
            // CRLF, not a bare LF. The PTY here is in raw mode, so cfmakeraw has
            // disabled ONLCR and nothing translates it -- and LF is index only,
            // since LNM is reset by default. A bare LF would march the text
            // rightwards until it wrapped, which is not what a terminal receives.
            b.extend_from_slice(b"\r\n");
            col = 0;
            continue;
        }
        if b.len() % 37 == 0 {
            b.extend_from_slice(b"\x1b[32m");
            continue;
        }
        let s = W[b.len() % 8].as_bytes();
        b.extend_from_slice(s);
        col += s.len();
        b.push(b' ');
        col += 1;
    }
    b
}

/// A reference workload, to divide the absolute numbers out.
///
/// **This is what makes a tight floor possible.** An absolute threshold has to
/// survive the slowest machine that will ever run it, so `spans_ingest_floor`
/// sits at 100 MB/s against a real value near 1300 -- it catches a
/// reintroduced per-cell copy and nothing smaller. `6f2499a` cost **66%** of
/// streaming throughput, taking spans to ~560, and passed that floor with 5.6x
/// to spare.
///
/// Dividing by a reference measured in the same process on the same data
/// cancels the machine out: a slow CPU slows both. What is left is a ratio
/// that can carry a floor near its real value.
///
/// `copy_from_slice` over the same buffer in the same chunk sizes, because it
/// is the cheapest thing that touches every byte, and it is not code baud can
/// regress.
fn reference_mbps(data: &[u8]) -> f64 {
    // `black_box` on both ends, because the first attempt at this reported
    // 3,692,307 MB/s -- 24 MB in 6.5 us, which is not a memory system, it is a
    // loop the optimiser deleted. The guard against that was
    // `assert!(x != 0xff || x == 0xff)`, a tautology the compiler is free to
    // see through, which is worth recording as the way not to do it.
    let t0 = Instant::now();
    let mut acc = 0u64;
    for _ in 0..3 {
        for &b in std::hint::black_box(data) {
            acc = acc.wrapping_add(u64::from(b));
        }
    }
    std::hint::black_box(acc);
    3.0 * data.len() as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0
}

fn throughput(mode: OpMode) -> f64 {
    let data = payload(8 << 20);
    let mut s = boxed_storage();
    let mut t = Term::new(&mut s, mode);
    let t0 = Instant::now();
    let mut i = 0;
    while i < data.len() {
        let n = READ_CHUNK.min(data.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(StreamByte::slice_from(&data[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    data.len() as f64 / t0.elapsed().as_secs_f64() / 1_048_576.0
}

/// Spans measured ~280 MB/s through a real PTY, higher without one. A floor of
/// 100 MB/s catches "someone reintroduced a per-cell copy", not jitter.
#[test]
fn spans_ingest_floor() {
    let v = throughput(OpMode::Runs);
    assert!(
        v > 100.0,
        "spans ingest {v:.1} MB/s, floor 100 — a copy was reintroduced?"
    );
}

/// Cell modes are structurally slower; floor set proportionally.
///
/// **This floor is load-bearing and it caught a real regression.** Full-screen
/// scrolling used to advance `head` -- which indexes the *run* table -- and
/// leave `vp` alone, so `Computed` and `Templated` never scrolled: old text
/// stayed put and only the bottom row blanked. Correcting it by copying the
/// grid dropped cell ingest to 38 MB/s and tripped this test, because a
/// streaming payload scrolls every screenful and the copy is write
/// amplification of roughly 20:1 against rule #1.
///
/// The fix that gets both is `RowRot`: a scroll rotates an index, and
/// `Term::viewport` undoes the rotation once when a renderer asks for a
/// contiguous slice. Ingest never pays, and the measured result is ~860 MB/s
/// -- the same as the version that did not scroll at all.
#[test]
fn cells_ingest_floor() {
    let v = throughput(OpMode::Computed);
    assert!(v > 70.0, "cell ingest {v:.1} MB/s, floor 70");
}

/// Storage must stay cache-resident. The arena-size sweep peaked at 1 MiB and
/// went negative by 64 MiB; this guards someone "improving" it.
#[test]
fn storage_stays_small() {
    let n = std::mem::size_of::<Storage>();
    assert!(
        n < 24 << 20,
        "Storage is {} MiB. Most of it is the over-provisioned viewport, which costs \
         nothing until touched — but the *arena* inside it must stay 1 MiB, since \
         the size sweep peaked there and went negative by 64 MiB.",
        n >> 20
    );
}

/// The core allocates nothing per byte. Storage is one fixed block.
#[test]
fn storage_is_one_block() {
    assert_eq!(
        core::mem::size_of::<baud_core2::Run>(),
        16,
        "Run is 16 bytes: BytePos+ByteLen+StyleId+encoding+explicit _pad"
    );
    assert_eq!(
        core::mem::size_of::<baud_core2::PackedCell>(),
        8,
        "PackedCell is 8 bytes: Content(u32) + StyleId + pad. Four bytes could not hold \
         a full codepoint, so U+1F680 truncated to U+F680 (a private-use char) and the \
         differential test caught it. Measured cost of the extra four bytes: 48% of the \
         kernel ceiling vs 50% for 4-byte cells."
    );
}

/// **The gate `6f2499a` needed and did not have.**
///
/// Ingest throughput as a fraction of `reference_mbps`, which cancels the
/// machine out: a slower CPU slows both.
///
/// The arithmetic behind the floor, so it can be re-derived rather than
/// trusted. On an M5 Max the reference is ~60,000 MB/s and spans ~1,100, a
/// ratio of **0.018**. `6f2499a` left spans at ~560, which against the same
/// reference is **0.0093**. The floor is **0.012** -- 33% below the healthy
/// ratio and 29% above the broken one.
///
/// That is a much weaker requirement than an absolute floor makes: it
/// tolerates any machine whose ingest keeps two thirds of its current speed
/// *relative to a linear scan of the same bytes*, rather than any machine
/// faster than a fixed MB/s.
#[test]
fn spans_ingest_holds_its_ratio_to_a_reference() {
    let data = payload(8 << 20);
    let r = reference_mbps(&data);
    let v = throughput(OpMode::Runs);
    let ratio = v / r;
    assert!(
        ratio > 0.012,
        "spans {v:.1} MB/s against reference {r:.1} MB/s is {ratio:.3}, floor \
         0.012 -- an absolute floor cannot catch this class: `6f2499a` cost \
         66% of streaming and cleared the 100 MB/s floor with 5.6x to spare"
    );
}
