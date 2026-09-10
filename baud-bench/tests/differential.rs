//! The new core against `baud-core2`, screen for screen.
//!
//! **This is the only check that prices the new core's rendering against a
//! mature implementation rather than against nine hand-written cases.** It
//! runs on `utf8/cat-cjk.raw` because that recording carries **zero escape
//! sequences** -- the route histogram says so -- and the new core implements
//! no escapes yet. On that one payload the two cores are asked the same
//! question: text, newlines, wide characters, wrapping and scrolling.
//!
//! It is deliberately not run on the other seven recordings. There the diff
//! would be dominated by the ~149 VT operations core2 has and the new core
//! does not, which is a known gap and not a finding.

use baud_core as new;
use baud_core2 as old;

const COLS: usize = 200;
const ROWS: usize = 50;

fn recording(name: &str) -> Vec<u8> {
    let p = format!("{}/../tests/recordings/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
}

/// A screen as characters, with the wide tail written as `\0` so a
/// misplaced tail is a diff rather than something that reads as a space.
fn cell(content: old::Content) -> char {
    content
        .as_codepoint()
        .and_then(|p| char::from_u32(p.value()))
        .unwrap_or(match content.is_wide_tail() {
            true => '\0',
            false => ' ',
        })
}

fn old_screen(bytes: &[u8], chunk: usize) -> Vec<char> {
    let mut store = old::Storage::boxed();
    let mut t = old::Term::new(&mut store, old::OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(old::StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    (0..ROWS)
        .flat_map(|y| t.row(old::Row::from_index(y))[..COLS].to_vec())
        .map(|c| cell(c.content))
        .collect()
}

fn new_screen(bytes: &[u8], chunk: usize) -> Vec<char> {
    let ws = new::WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span = new::RowBlock::leading(rows, new::Cols::from_stride(new::MAX_COLS));
    let mut arena = vec![0u8; new::ARENA];
    let mut vp = vec![new::PackedCell::EMPTY; span.len()];
    let mut used = vec![new::RowExtent::ZERO; rows.index()];
    let mut cells3 = vec![new::PackedCell::EMPTY; new::CELLS3];
    let mut tmpl = vec![None; new::NSTYLE];
    let mut tabs = vec![false; new::MAX_COLS];
    let mut reply = vec![0u8; new::REPLY];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
        ),
        &ws,
    );
    let mut i = 0;
    while i < bytes.len() {
        let n = chunk.min(bytes.len() - i);
        t.write_at(new::BytePos::ZERO, &bytes[i..i + n]);
        t.feed(new::BytePos::ZERO, new::ReadLen::from_index(n));
        i += n;
    }
    // **Indexed by the core's stride, not by `MAX_COLS`.** The grid is
    // contiguous at the live width now, and a renderer that assumed the
    // maximum would read every row from the wrong place.
    let w = t.stride().index();
    let vp = t.viewport();
    (0..ROWS)
        .flat_map(|y| &vp[y * w..y * w + COLS])
        .map(|c| cell(c.content))
        .collect()
}

/// **The screens must match, cell for cell.**
///
/// A failure here is worth more than any timing: the new core is 1.7x core2 on
/// this recording and that number means nothing if the two are not rendering
/// the same thing.
#[test]
fn the_new_core_renders_cat_scroll_exactly_as_core2_does() {
    let bytes = recording("ascii/cat-scroll.raw");
    let (a, b) = (
        old_screen(&bytes, old::READ_CHUNK),
        new_screen(&bytes, 4096),
    );
    assert_eq!(a.len(), b.len());
    let diffs: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    let mut kinds = std::collections::BTreeMap::new();
    for &i in &diffs {
        *kinds.entry((a[i], b[i])).or_insert(0usize) += 1;
    }
    for ((x, y), n) in &kinds {
        println!("  {n:5}  core2 {:?}  new {:?}", x, y);
    }
    assert!(
        diffs.is_empty(),
        "{} of {} cells differ",
        diffs.len(),
        a.len()
    );
}

/// **Wrapping, against core2 rather than against an argument.**
///
/// `cat-scroll.raw` never writes a line wider than the screen, so it agrees
/// with core2 whether or not the blitter wraps -- it passed on the build that
/// clipped and dropped the tail. That is worth knowing about a differential:
/// **a real recording only covers what it happens to contain.**
///
/// So the wrap case is driven by bytes written for it: lines longer than the
/// 200-column screen, lines landing exactly on the margin, wide characters
/// straddling it, and a carriage return back over wrapped text. This is a test
/// fixture and not a payload -- nothing here is timed.
#[test]
fn a_line_wider_than_the_screen_wraps_the_way_core2_wraps_it() {
    let mut bytes = Vec::new();
    for extra in [1usize, 5, 199, 200, 201, 401] {
        bytes.extend(std::iter::repeat_n(b'x', COLS + extra));
        bytes.push(b'\n');
    }
    // Exactly the margin, then more text: the deferred-wrap case.
    bytes.extend(std::iter::repeat_n(b'y', COLS));
    bytes.extend(b"tail\n");
    // A wide character that cannot fit in the last column.
    bytes.extend(std::iter::repeat_n(b'z', COLS - 1));
    bytes.extend("世界".as_bytes());
    bytes.push(b'\n');
    // Back over wrapped text.
    bytes.extend(std::iter::repeat_n(b'w', COLS + 20));
    bytes.extend(b"\rover\n");
    let (a, b) = (
        old_screen(&bytes, old::READ_CHUNK),
        new_screen(&bytes, 4096),
    );
    let diffs: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).collect();
    assert!(
        diffs.is_empty(),
        "{} cells differ; first at row {} col {}: core2 {:?}, new {:?}",
        diffs.len(),
        diffs[0] / COLS,
        diffs[0] % COLS,
        a[diffs[0]],
        b[diffs[0]],
    );
}

/// **Cursor addressing and erasure, against core2 rather than against my own
/// other build.** The two CSI paths in the new core -- a whole sequence in one
/// step when the read carries it, byte at a time when it does not -- disagreed
/// on `ESC [ 30;123 H` followed by `ESC [ K`, and comparing them to each other
/// says only that one is wrong.
#[test]
fn cursor_and_erase_agree_with_core2() {
    let mut b = Vec::new();
    b.extend(b"hello world this is a line of text to erase parts of\r\n");
    b.extend(b"\x1b[30;123Hplaced\x1b[K");
    b.extend(b"\x1b[1;1Htop\x1b[3;5Hindent\x1b[2K");
    b.extend(b"\x1b[10;10H\x1b[Jcleared below");
    b.extend(b"\x1b[5;5H\x1b[1Kstart-erased");
    b.extend(b"\x1b[7d\x1b[20Gcolumn\x1b[A\x1b[B\x1b[C\x1b[D");
    let (x, y) = (old_screen(&b, old::READ_CHUNK), new_screen(&b, 4096));
    let d: Vec<usize> = (0..x.len()).filter(|&i| x[i] != y[i]).collect();
    assert!(
        d.is_empty(),
        "{} cells differ; first at row {} col {}: core2 {:?}, new {:?}",
        d.len(),
        d[0] / COLS,
        d[0] % COLS,
        x[d[0]],
        y[d[0]],
    );
}

/// **The gaps, pinned as numbers so they cannot drift in silence.**
///
/// Both halves of a cell are compared: the glyph, and the rendition it is
/// drawn in. The version of this check that compared only glyphs passed
/// happily while `asciiquarium.raw` came out with every one of its 10,000
/// cells missing its colour, and the throughput figures taken beside it looked
/// like a comparison.
///
/// **Renditions are resolved, not compared as ids.** A `StyleId` is an index
/// into a table that evicts on collision, so two cores agreeing on the index
/// is not the same as agreeing on the colour.
#[test]
fn the_rendition_gap_is_pinned() {
    // **Every recording, not the six this covered.** `cat-cjk`, `less-cjk` and
    // `cava` were omitted, and the first two are the worst recordings in the
    // corpus by glyph gap -- so the rendition half was silent on exactly the
    // rows most likely to move. All nine are zero, so widening it costs
    // nothing and it was never a judgement about which rows could pass.
    for (name, want) in [
        ("ascii/asciiquarium.raw", 0usize),
        ("ascii/cat-scroll.raw", 0),
        ("ascii/git-log.raw", 0),
        ("ascii/less-pager.raw", 0),
        ("utf8/btop.raw", 0),
        ("utf8/cat-cjk.raw", 0),
        ("utf8/cava.raw", 0),
        ("utf8/less-cjk.raw", 0),
        ("utf8/vim-unicode.raw", 0),
    ] {
        let b = recording(name);
        let (x, y) = (old_styles(&b), new_styles(&b));
        let d = (0..x.len()).filter(|&i| x[i] != y[i]).count();
        assert_eq!(d, want, "{name}: cells whose rendition differs from core2");
    }
}

/// Every cell's resolved rendition, from `baud-core2`.
fn old_styles(bytes: &[u8]) -> Vec<old::StyleWord> {
    let mut store = old::Storage::boxed();
    let mut t = old::Term::new(&mut store, old::OpMode::Computed);
    let mut i = 0;
    while i < bytes.len() {
        let n = old::READ_CHUNK.min(bytes.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(old::StreamByte::slice_from(&bytes[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    let cells: Vec<old::PackedCell> = (0..ROWS)
        .flat_map(|y| t.row(old::Row::from_index(y))[..COLS].to_vec())
        .collect();
    cells.iter().map(|c| t.style_of(*c)).collect()
}

/// The same, from the new core.
fn new_styles(bytes: &[u8]) -> Vec<old::StyleWord> {
    let ws = new::WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span = new::RowBlock::leading(rows, new::Cols::from_stride(new::MAX_COLS));
    let mut arena = vec![0u8; new::ARENA];
    let mut vp = vec![new::PackedCell::EMPTY; span.len()];
    let mut used = vec![new::RowExtent::ZERO; rows.index()];
    let mut cells3 = vec![new::PackedCell::EMPTY; new::CELLS3];
    let mut tmpl = vec![None; new::NSTYLE];
    let mut tabs = vec![false; new::MAX_COLS];
    let mut reply = vec![0u8; new::REPLY];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
        ),
        &ws,
    );
    let mut i = 0;
    while i < bytes.len() {
        let n = new::READ_CHUNK.min(bytes.len() - i);
        t.write_at(new::BytePos::ZERO, &bytes[i..i + n]);
        t.feed(new::BytePos::ZERO, new::ReadLen::from_index(n));
        i += n;
    }
    let w = t.stride().index();
    let cells: Vec<new::PackedCell> = (0..ROWS)
        .flat_map(|y| {
            let vp = t.viewport();
            vp[y * w..y * w + COLS].to_vec()
        })
        .collect();
    cells.iter().map(|c| t.style_of(*c)).collect()
}

/// The glyph gap, likewise pinned. `btop.raw` matching exactly is real and is
/// **only about the glyphs** -- see `the_rendition_gap_is_pinned` for the half
/// this comparison cannot see.
#[test]
fn the_glyph_gap_is_pinned() {
    // **This asserted nothing until now.** The body ended `let _ = want;`, so
    // it printed the gap and passed green while three recordings diverged by
    // 3,057 cells between them -- under a doc comment claiming the gaps were
    // "pinned as numbers so they cannot drift in silence". A gate that cannot
    // fail is the "probe that cannot fire" failure this repo already records
    // three times, and it is the one that had been quoted all session as
    // evidence the new core renders what `baud-core2` renders.
    //
    // **The three non-zero rows are the cluster machine and nothing else.**
    // `baud-core` has no `ClusterBreak`, no `is_break` and no marks table, so
    // a ZWJ joiner, a variation selector or a combining mark takes its own
    // cell where core2 attaches it to the base, and every cell after it on the
    // row shifts a column. They are pinned at what they *are* rather than at
    // zero, so the number moves the day clustering lands -- in either
    // direction. A row that goes up is a regression; a row that goes down
    // needs its new value written here.
    for (name, want) in [
        ("ascii/asciiquarium.raw", 0usize),
        ("ascii/cat-scroll.raw", 0),
        ("ascii/git-log.raw", 0),
        ("ascii/less-pager.raw", 0),
        ("utf8/btop.raw", 0),
        ("utf8/cava.raw", 0),
        // The clustering three. Zero when `baud-core` grows a cluster machine.
        ("utf8/cat-cjk.raw", 355),
        ("utf8/less-cjk.raw", 2185),
        ("utf8/vim-unicode.raw", 517),
    ] {
        let b = recording(name);
        let (x, y) = (old_screen(&b, old::READ_CHUNK), new_screen(&b, 4096));
        let d = (0..x.len()).filter(|&i| x[i] != y[i]).count();
        assert_eq!(d, want, "{name}: cells whose glyph differs from core2");
    }
}

/// **A scrolling region that does not span the screen, against core2.**
///
/// The region scroll used to copy every row inside the region. A region
/// anchored at the top is now a rotation plus putting the rows *below* it
/// back, which is one row for the shape every status-line application uses --
/// and a rotation permutes the whole grid, so getting the repair wrong shows
/// up as rows appearing in the wrong order rather than as a crash.
///
/// Nothing in the corpus reaches this: every recording is measured alone from
/// a fresh terminal, so `DECSTBM` is never left set. It took concatenating the
/// recordings into one stream to find that scrolls were 100% copying.
#[test]
fn a_scroll_region_with_a_status_line_agrees_with_core2() {
    for (top, bot) in [(1usize, 49usize), (1, 48), (3, 47), (1, 25), (10, 40)] {
        let mut b = Vec::new();
        // Set the region, then fill well past its height so it scrolls many
        // times, then write outside it so the rows that must not move are
        // checked too.
        b.extend(format!("\x1b[{top};{bot}r").into_bytes());
        for i in 0..120 {
            b.extend(format!("line {i:03} in the region\r\n").into_bytes());
        }
        // The status line under the region, addressed absolutely.
        b.extend(format!("\x1b[{};1Hstatus line stays put", ROWS).into_bytes());
        // And a header above it when the region does not start at the top.
        b.extend(b"\x1b[1;1Hheader".to_vec());
        let (x, y) = (old_screen(&b, old::READ_CHUNK), new_screen(&b, 4096));
        let d: Vec<usize> = (0..x.len()).filter(|&i| x[i] != y[i]).collect();
        assert!(
            d.is_empty(),
            "region {top};{bot}: {} cells differ; first at row {} col {}: core2 {:?}, new {:?}",
            d.len(),
            d[0] / COLS,
            d[0] % COLS,
            x[d[0]],
            y[d[0]],
        );
    }
}
