#![deny(dead_code)]
// A test whose `#[test]` is lost still compiles, still satisfies
// `every_test_named_in_the_docs_exists` -- which looks for `fn <name>`, not for
// the attribute -- and silently stops running. That happened to
// the blanking test in `amplification.rs` -- then named
// `blanking_writes_the_whole_row_and_that_is_the_cheaper_choice`, and since
// renamed because the decision it pinned was reversed by measurement -- whose
// assertion was wrong for two commits without anyone finding out. The
// compiler's own dead-code analysis is the check that catches it.
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

/// **Blanking stops at the used extent, and in `Runs` mode that is nothing.**
///
/// This test asserted the opposite for most of the project's life -- that every
/// scroll blanks its full incoming row and that "the read needed to avoid it
/// costs more than the writes it saves". The decision it pinned has been
/// reversed by measurement, and the reversal is worth reading before anyone
/// re-argues it:
///
/// * The old figure came from **scan**-before-blank, which reads the row
///   looking for a cell differing from the blank. That lost twice, at -24% and
///   -66%, and `cells_blanked` came out *identical* both times: the blank
///   carries the current style, so on any payload emitting SGR the scan runs to
///   the end of every row and saves nothing.
/// * A high-water **mark** cannot have that failure. It records where glyphs
///   reached, which no rendition change can alter, and the "has the template
///   changed" test is one load of the row's last cell rather than a scan.
///
/// The synthetic workload here is the extreme case and shows why the
/// distinction matters: 60 columns of a 200-column grid, in `OpMode::Runs`
/// where the viewport cells are **never materialised at all**. Every mark stays
/// at zero, so every scroll blanks nothing, and the only cells written are the
/// 50-row startup clear. 4,000,200 becomes 10,000.
///
/// **Do not re-implement the scan.** `MEASUREMENTS.md` carries the counts:
/// a mark skips 72-82% of blanking on every real scrolling recording and 16%
/// on `payload::streaming`, and a scan skips none of either.
#[test]
fn blanking_stops_at_the_used_extent() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Runs);
    let mut v = Vec::new();
    for _ in 0..20_000 {
        v.extend(std::iter::repeat_n(b'x', 60));
        v.extend_from_slice(b"\r\n");
    }
    let mut i = 0;
    while i < v.len() {
        let n = READ_CHUNK.min(v.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&v[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }

    // 50 rows of 200 for the startup clear, and nothing else. In `Runs` the
    // text lives in the span table and the cells behind these rows were never
    // written, so their marks never left zero and 19,951 scrolls blank no cells
    // at all. The old assertion here was `20_001 * 200`.
    assert_eq!(
        t.cells_blanked.count(),
        50 * 200,
        "in Runs mode nothing materialises into the viewport, so every mark \
         stays at zero and only the startup clear writes blanks"
    );
    assert!(
        t.runs_written.count() > 20_000,
        "and the text went to the span table instead"
    );
}

/// **The same workload in `Computed`, where the cells really are written.**
///
/// `Runs` above is the extreme; this is the ordinary case, and it is the one
/// the 72-82% in `MEASUREMENTS.md` describes. 60 columns are written per line
/// and 200 are blanked without a mark, so the mark should remove roughly 70% of
/// the blanking rather than all of it.
#[test]
fn blanking_in_computed_mode_stops_at_the_text() {
    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut v = Vec::new();
    for _ in 0..2_000 {
        v.extend(std::iter::repeat_n(b'x', 60));
        v.extend_from_slice(b"\r\n");
    }
    let mut i = 0;
    while i < v.len() {
        let n = READ_CHUNK.min(v.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let f = slot.write(StreamByte::slice_from(&v[i..i + n]));
        t.feed(base, f.len());
        t.commit_read(f.len());
        i += n;
    }

    // Without the mark this is 2001 * 200 = 400,200. With it, each scrolled row
    // has 60 columns of text, plus the 50-row startup clear at full width.
    let without = 2_001 * 200;
    let got = t.cells_blanked.count();
    assert!(
        got < without / 2,
        "the mark should remove most of the blanking: {got} cells blanked \
         against {without} without it"
    );
    assert!(
        got > 1_950 * 60,
        "but not more than the text occupies -- {got} is below the 60 columns \
         per scrolled row that genuinely have to be cleared, which would mean \
         rows are being skipped that still hold text"
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
///
/// Text is a fourth exit since `fast_utf8`: a whole multi-byte character
/// decoded without a dispatch per byte. Before it, every CJK codepoint took
/// three trips through `control` and landed in `slow`.
#[test]
fn real_capture_is_mostly_escape_sequences_not_text() {
    for (name, min_seq) in [
        ("utf8/btop.raw", 0.5),
        ("ascii/asciiquarium.raw", 0.6),
        ("utf8/vim-unicode.raw", 0.3),
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

        let (bulk, seq, slow, text) = t.path_split();
        // **Four exits now, not three.** `fast_utf8` decodes a whole
        // multi-byte character in place, so its bytes are neither an ASCII
        // run nor byte-at-a-time. This assertion is what caught them going
        // uncounted: btop's `slow` fell from 15% to 0.2% and the missing 14.5%
        // matched `utf8_share` exactly.
        assert!(
            (bulk + seq + slow + text - 1.0).abs() < 0.01,
            "{name}: the four paths must account for every byte, got \
             {bulk:.3} + {seq:.3} + {slow:.3} + {text:.3}"
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
        ("ascii/asciiquarium.raw", 0.95, 1.01),
        ("utf8/btop.raw", 0.60, 0.85),
        ("utf8/vim-unicode.raw", 0.20, 0.50),
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

/// **Every written cell is attributed to one of the three paths.**
///
/// `cells_written` gives amplification and not its shape, and the three paths
/// cost very different amounts per cell: on the streaming payload `fill`
/// writes 73% of the cells in 17% of the profile while `put_ascii_run` writes
/// 27% in 33%, which is about 5.3x per cell for the same eight bytes. That
/// ratio is only visible because the counts are attributed.
///
/// The invariant is the same one `path_split` carries: a write nobody counted
/// is a write nobody will think to look for. This is what catches a fourth
/// write site being added without a counter.
#[test]
fn cell_attribution_accounts_for_every_write() {
    for (name, mode) in [("computed", OpMode::Computed), ("runs", OpMode::Runs)] {
        let mut s = Storage::boxed();
        let mut t = Term::new(&mut s, mode);
        // Text, a scroll, an erase, a wide character and a repeat: one of each
        // kind of write, so a missing counter on any path shows up here.
        let mut v = Vec::new();
        for _ in 0..80 {
            v.extend_from_slice("hello world ".as_bytes());
            v.extend_from_slice("世界".as_bytes());
            v.extend_from_slice(b"\x1b[K\r\n");
        }
        v.extend_from_slice(b"\x1b[2J\x1b[5;5Hx\x1b[10b");
        // **A partial scroll region, which takes the copying path.** That path
        // calls `View::delete_lines_in` in `baud-domain`, where the counters do
        // not reach, and nothing counted the rows it moves and blanks -- so
        // every amplification figure for an application using DECSTBM was
        // understated. Without this the gap is invisible: vim sets a region
        // equal to the whole screen, which takes the O(1) ring path instead.
        v.extend_from_slice(b"\x1b[5;15r");
        for _ in 0..40 {
            v.extend_from_slice(b"\x1b[15Hscrolled\r\n");
        }
        v.extend_from_slice(b"\x1b[r");
        let mut i = 0;
        while i < v.len() {
            let n = READ_CHUNK.min(v.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&v[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }

        let total = t.cells_written.count();
        let parts = t.cells_bulk.count() + t.cells_single.count() + t.cells_fill.count();
        assert!(total > 0, "{name}: the payload wrote no cells");
        assert_eq!(
            parts, total,
            "{name}: attribution sums to {parts} against {total} written -- a \
             write site has no counter"
        );
        let (b, s2, f) = t.cell_attribution();
        assert!(
            (b + s2 + f - 1.0).abs() < 1e-9,
            "{name}: shares sum to {}",
            b + s2 + f
        );
    }
}

/// **A scroll region's cell writes are counted, against arithmetic rather than
/// against the other counters.**
///
/// `cell_attribution_accounts_for_every_write` cannot catch this and it is
/// worth saying why: it asserts the three buckets sum to `cells_written`, so a
/// path that increments *neither* stays perfectly consistent while being
/// invisible. Internal consistency is not completeness.
///
/// `View::delete_lines_in` lives in `baud-domain`, where the counters do not
/// reach, and for the whole life of this file it moved and blanked cells that
/// nothing counted -- so amplification was understated for every application
/// using DECSTBM, which is most TUIs. The full-screen path was counted, which
/// is why it went unnoticed: vim sets a region equal to the whole screen and
/// takes the O(1) ring instead.
///
/// So this counts in the other direction: a region of known height, a known
/// number of scrolls, and the cells that must follow.
#[test]
fn a_region_scroll_counts_the_cells_it_moves_and_blanks() {
    const TOP: usize = 5; // 1-based, as DECSTBM takes them
    const BOT: usize = 15;
    const SCROLLS: usize = 40;
    let height = BOT - TOP + 1;

    let mut s = Storage::boxed();
    let mut t = Term::new(&mut s, OpMode::Computed);
    let mut v = Vec::new();
    v.extend_from_slice(format!("\x1b[{TOP};{BOT}r").as_bytes());
    let feed_all = |t: &mut Term, v: &[u8]| {
        let mut i = 0;
        while i < v.len() {
            let n = READ_CHUNK.min(v.len() - i);
            let mut slot = t.read_slot();
            let base = slot.base();
            let f = slot.write(StreamByte::slice_from(&v[i..i + n]));
            t.feed(base, f.len());
            t.commit_read(f.len());
            i += n;
        }
    };
    feed_all(&mut t, &v);
    let before = t.cells_written.count();
    let scrolls_before = t.scrolls.count();

    // Park the cursor on the region's last row and newline off it, which is
    // the only thing that scrolls a region.
    let mut w = Vec::new();
    for _ in 0..SCROLLS {
        w.extend_from_slice(format!("\x1b[{BOT}H\n").as_bytes());
    }
    feed_all(&mut t, &w);

    let (cols, _rows) = t.geometry();
    let cols = cols.index();
    // Each scroll copies `height - 1` rows up and blanks one.
    let want = SCROLLS * height * cols;
    assert_eq!(
        t.cells_written.count() - before,
        want as u64,
        "a {height}-row region scrolled {SCROLLS} times moves and blanks \
         {want} cells; `View::delete_lines_in` does the work in baud-domain \
         where the counters do not reach, so this is the direction that \
         catches it"
    );
    assert_eq!(t.scrolls.count() - scrolls_before, SCROLLS as u64);

    // **`IL` and `DL` take the same uncounted route and no payload reaches
    // them.** vim reports zero scrolls on its own recording -- it redraws by
    // erasing and rewriting rather than scrolling -- so without this the three
    // remaining call sites of `insert_lines_in`/`delete_lines_in` would be
    // covered by nothing at all.
    let before = t.cells_written.count();
    let mut il = Vec::new();
    for _ in 0..10 {
        il.extend_from_slice(format!("\x1b[{TOP}H\x1b[L").as_bytes()); // IL
        il.extend_from_slice(format!("\x1b[{TOP}H\x1b[M").as_bytes()); // DL
    }
    feed_all(&mut t, &il);
    // Each of the twenty operations moves `height - 1` rows and blanks one.
    assert_eq!(
        t.cells_written.count() - before,
        (20 * height * cols) as u64,
        "IL and DL move and blank a region-sized block each, through the same \
         `baud-domain` helpers the counters do not reach"
    );
}

/// **The UTF-8 path carries CJK capture, and the cell path is per-codepoint.**
///
/// The companion to `real_capture_is_mostly_escape_sequences_not_text`, whose
/// thresholds are shaped for escape-dense capture and are the wrong way round
/// here: `cat-cjk.raw` has **no escape sequences at all**, and its bytes go
/// almost entirely through `fast_utf8`.
///
/// Two things are pinned, and both were invisible before these recordings
/// existed:
///
/// * **`text` dominates.** If `fast_utf8` stopped being reached, these bytes
///   would fall back to three trips through `control` each and land in `slow`,
///   which is what they did before it existed. That regression would not show
///   up in any other test here.
/// * **`single` is a large share of cells.** A wide character cannot use
///   `put_ascii_run`, so it is written one codepoint at a time. ASCII capture
///   reports 0% single; this is what the 4x gap between `cat-cjk` and
///   `cat-scroll` is made of, and it is the remaining lead.
#[test]
fn the_utf8_path_carries_cjk_capture() {
    for (name, min_text, min_single) in [
        // Thresholds set from measurement, below the observed 81.2% and
        // 62.8%: the balance is the ASCII line prefixes, which correctly go to
        // the bulk scanner rather than through here.
        ("utf8/cat-cjk.raw", 0.75, 0.05),
        ("utf8/less-cjk.raw", 0.5, 0.05),
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

        let (bulk, seq, slow, text) = t.path_split();
        assert!(
            (bulk + seq + slow + text - 1.0).abs() < 0.01,
            "{name}: the four paths must account for every byte, got \
             {bulk:.3} + {seq:.3} + {slow:.3} + {text:.3}"
        );
        assert!(
            text > min_text,
            "{name}: the UTF-8 path took {:.1}% of bytes, expected over \
             {:.0}%. If this fell, `fast_utf8` stopped being reached and every \
             codepoint is back to three trips through `control`",
            text * 100.0,
            min_text * 100.0
        );
        assert!(
            slow < 0.05,
            "{name}: {:.1}% of bytes went byte-at-a-time, which is where CJK \
             used to go and must not go again",
            slow * 100.0
        );

        let (_, single, _) = t.cell_attribution();
        assert!(
            single > min_single,
            "{name}: only {:.1}% of cells came from the per-codepoint path. \
             A wide character cannot use `put_ascii_run`, so a low figure here \
             means the recording stopped carrying wide characters",
            single * 100.0
        );
    }
}
