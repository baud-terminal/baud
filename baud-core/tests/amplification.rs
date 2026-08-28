//! Rule #1, as a test rather than a sentence.
//!
//! `README.md` opens by saying every result here is predicted by two
//! quantities, the first being **write amplification <= 1**. It had no
//! measurement: the instrumentation that fed it printed `inf x amplification`
//! from counters nothing updated, and was removed rather than repaired. The
//! number the architecture is justified by has been an assertion since.
//!
//! What it is about is the **span** table. A `Run` costs `size_of::<Run>()`
//! however many bytes of text it covers, so it amortises. A cell mode is not
//! competing and cannot: one ASCII byte becomes an eight-byte `PackedCell`, so
//! 8x is a floor, and reading the rule as covering that would make it
//! unsatisfiable by construction.

use baud_core::*;

fn drive(bytes: &[u8], mode: OpMode) -> (f64, f64, f64) {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, mode);
    let mut i = 0;
    while i < bytes.len() {
        let n = READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }
    let (cell, blank) = t.cell_amplification();
    (t.span_amplification(), cell, blank)
}

fn lines(n: usize, width: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..n {
        v.extend(std::iter::repeat_n(b'x', width));
        v.extend_from_slice(b"\r\n");
    }
    v
}

/// **Span storage stays under one byte written per byte ingested.**
///
/// The rule, executable. If a `Run` grows, or runs stop amortising because
/// something splits them, this is what says so.
#[test]
fn span_amplification_is_under_one() {
    for (name, payload) in [
        ("60-column lines", lines(1000, 60)),
        ("full-width lines", lines(500, 200)),
        ("no newlines at all", vec![b'y'; 60_000]),
    ] {
        let (span, _, _) = drive(&payload, OpMode::Runs);
        assert!(
            span < 1.0,
            "{name}: span amplification {span:.3}x exceeds rule #1"
        );
        assert!(
            span > 0.0,
            "{name}: nothing counted -- the probe cannot fire"
        );
    }
}

/// A cell mode stores no spans, and that is not a failure to measure.
#[test]
fn a_cell_mode_writes_no_spans() {
    let (span, cell, _) = drive(&lines(500, 60), OpMode::Computed);
    assert_eq!(span, 0.0, "Computed materialises rather than storing runs");
    assert!(cell > 0.0, "and it does write cells");
}

/// **A cell costs exactly eight bytes per ASCII byte, and nothing else does.**
///
/// The floor that makes reading rule #1 as covering cell writes a category
/// error: `PackedCell` is eight bytes and an ASCII character is one, so a
/// materialising mode cannot be under 8x however good it is.
///
/// Asserted on the raw counters rather than the ratio, because the ratio has a
/// fixed term in it: `Term::new` blanks the whole grid, which is
/// `rows * cols` = 10,000 cells whatever the input length. Against a small
/// payload that startup cost swamps everything -- 8,000 bytes measured 10x of
/// blanking and nothing had scrolled. The counters have no such artifact.
#[test]
fn materialising_a_cell_costs_eight_bytes_per_ascii_byte() {
    let payload = vec![b'y'; 8_000];
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut i = 0;
    while i < payload.len() {
        let n = READ_CHUNK.min(payload.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&payload[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }

    let text = t.cells_written.count() - t.cells_blanked.count();
    assert_eq!(
        text,
        t.bytes_in.count(),
        "one ASCII byte in, one cell out -- no more and no fewer"
    );
    assert_eq!(
        t.cells_blanked.count(),
        10_000,
        "and the only blanking here is the startup clear: 50 rows x 200 cols, \
         paid once, which is why this test reads counters and not a ratio"
    );
}

/// **Blanking on scroll costs more than the text does.**
///
/// The finding that restoring the measurement produced. Scrolling blanks the
/// incoming row across the full width, so a payload of 60-column lines on a
/// 200-column grid blanks 200 cells to store 60 characters -- and the blanking
/// is roughly three times the cost of the text.
///
/// Pinned as a *ratio* rather than an absolute so it survives a geometry
/// change, and asserted loosely because the point is the order of magnitude,
/// not the digits. If blanking ever stops dominating, this failing is the
/// news.
#[test]
fn scroll_blanking_dominates_the_cell_budget() {
    let (_, cell, blank) = drive(&lines(1000, 60), OpMode::Computed);
    assert!(
        blank / cell > 0.5,
        "blanking is {blank:.1}x of {cell:.1}x total -- it used to be the \
         majority, and if it no longer is, something changed"
    );
    let text = cell - blank;
    assert!(
        blank > text * 2.0,
        "blanking {blank:.1}x against text {text:.1}x: the incoming row is \
         blanked across the full width regardless of how much of it was used"
    );
}

/// **Where real input actually goes, which is not where the optimisation is.**
///
/// `feed_with` has three exits: a bulk printable run through the SIMD scanner,
/// a complete escape sequence recognised in one go, and everything else one
/// byte at a time through `control`.
///
/// On real capture the bulk scanner -- the one with the AVX2 path and the
/// `_mm256` intrinsics this repo guards in CI -- handles **10-27%** of bytes.
/// The escape-sequence path handles 44-80%. That is worth pinning, because the
/// intuition that a terminal is mostly text is what a synthetic payload
/// confirms and real capture does not: btop is one escape every 16.5 bytes and
/// asciiquarium one every 5.4.
///
/// The fast path is not *missing*, either -- btop takes it 31,606 times
/// against 31,683 escapes, 99.8%. The cost is inside it.
#[test]
fn real_capture_is_mostly_escape_sequences_not_text() {
    for (name, min_seq) in [
        ("btop.raw", 0.5),
        ("asciiquarium.raw", 0.6),
        ("vim-unicode.raw", 0.3),
    ] {
        let d = std::fs::read(format!(
            "{}/tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect(name);
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < d.len() {
            let n = READ_CHUNK.min(d.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&d[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }

        let (bulk, seq, slow) = t.path_split();
        assert!(
            (bulk + seq + slow - 1.0).abs() < 0.01,
            "{name}: the three paths must account for every byte, got \
             {bulk:.3} + {seq:.3} + {slow:.3}"
        );
        assert!(
            seq > min_seq,
            "{name}: sequences took {:.1}% of bytes, expected over {:.0}% -- \
             if this dropped, either the recording changed or the fast path \
             stopped being taken",
            seq * 100.0,
            min_seq * 100.0
        );
        assert!(
            bulk < 0.4,
            "{name}: bulk text took {:.1}% -- real capture is not mostly text, \
             and a payload where it is does not resemble this",
            bulk * 100.0
        );
    }
}

/// **How much of a row carries content, which is what blanking wastes.**
///
/// Scrolling blanks the incoming row across the full width. Whether that is
/// waste depends entirely on the workload, and the three recordings disagree
/// completely -- which is why this is measured per recording rather than
/// argued once:
///
/// | recording | mean used width | waste |
/// |---|---|---|
/// | asciiquarium | 200 of 200 | none |
/// | btop | 144 of 200 | 28%, but it never scrolls |
/// | vim | 70 of 200 | 65% |
///
/// So narrowing the blank would halve vim's cell traffic and do nothing at all
/// for asciiquarium. A single number for "the cost of blanking" would have
/// hidden that, and the synthetic payload -- 60-column lines on a 200-column
/// grid -- resembles only the vim case.
#[test]
fn how_much_of_a_row_is_actually_used() {
    for (name, lo, hi) in [
        ("asciiquarium.raw", 0.95, 1.01),
        ("btop.raw", 0.60, 0.85),
        ("vim-unicode.raw", 0.20, 0.50),
    ] {
        let d = std::fs::read(format!(
            "{}/tests/recordings/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect(name);
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, OpMode::Computed);
        let mut i = 0;
        while i < d.len() {
            let n = READ_CHUNK.min(d.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&d[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }

        let (cols, rows) = t.geometry();
        let stride = t.stride().index();
        let vp = t.viewport();
        let used: usize = (0..rows.index())
            .map(|y| {
                vp[y * stride..y * stride + cols.index()]
                    .iter()
                    .rposition(|c| *c != PackedCell::EMPTY)
                    .map_or(0, |p| p + 1)
            })
            .sum();
        let share = used as f64 / (rows.index() * cols.index()) as f64;
        assert!(
            share > lo && share < hi,
            "{name}: rows are {:.0}% used, expected {:.0}-{:.0}%. This is the \
             headroom a narrower blank would recover, and it is workload \
             specific -- if it moved, the recording changed.",
            share * 100.0,
            lo * 100.0,
            hi * 100.0
        );
    }
}
