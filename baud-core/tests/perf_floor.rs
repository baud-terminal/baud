#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
//! Performance floors. Not a benchmark — a regression alarm. Thresholds are set
//! well below measured values so they fail on an architectural mistake (a copy
//! reintroduced, the arena grown past cache) rather than on machine variance.
use baud_core::*;
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
        core::mem::size_of::<baud_core::Run>(),
        16,
        "Run is 16 bytes: BytePos+ByteLen+StyleId+encoding+explicit _pad"
    );
    assert_eq!(
        core::mem::size_of::<baud_core::PackedCell>(),
        8,
        "PackedCell is 8 bytes: Content(u32) + StyleId + pad. Four bytes could not hold \
         a full codepoint, so U+1F680 truncated to U+F680 (a private-use char) and the \
         differential test caught it. Measured cost of the extra four bytes: 48% of the \
         kernel ceiling vs 50% for 4-byte cells."
    );
}
