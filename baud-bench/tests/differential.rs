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
    let mut alt = vec![new::PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![new::RowExtent::ZERO; rows.index()];
    let mut clusters = vec![new::ClusterSlot::EMPTY; new::NCLUSTER];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
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
    let mut alt = vec![new::PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![new::RowExtent::ZERO; rows.index()];
    let mut clusters = vec![new::ClusterSlot::EMPTY; new::NCLUSTER];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
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
        // **Zero, all nine.** These were the last three: 355, 517 and 452
        // cells of clustering, closed by giving `baud-core` the UAX #29 break
        // decision on the operands `Classified::of` was already returning and
        // discarding. Every recording in the corpus now renders cell for cell
        // as `baud-core2` renders it.
        ("utf8/cat-cjk.raw", 0),
        ("utf8/vim-unicode.raw", 0),
        // `less-cjk`'s end-of-stream row is blank against blank and pins
        // nothing -- see `ENDS_BLANK` and
        // `less_cjk_agrees_where_its_content_is`, which compares it where its
        // content is.
        ("utf8/less-cjk.raw", 0),
    ] {
        let b = recording(name);
        let (x, y) = (old_screen(&b, old::READ_CHUNK), new_screen(&b, 4096));
        let d = (0..x.len()).filter(|&i| x[i] != y[i]).count();
        // **A row whose reference screen is empty cannot fail.** Comparing two
        // blank grids is the probe-that-cannot-fire shape this repo has
        // recorded four times, and the alternate-screen fix created a fresh
        // one by making a recording end on a screen with nothing on it. The
        // guard is here so the next recording that ends blank is a decision
        // rather than a discovery.
        let content = x.iter().filter(|c| **c != ' ' && **c != '\0').count();
        assert!(
            content > 0 || ENDS_BLANK.contains(&name),
            "{name}: core2's final screen is empty, so this row pins nothing"
        );
        assert_eq!(d, want, "{name}: cells whose glyph differs from core2");
    }
}

/// Recordings whose final screen is empty, so a cell-for-cell comparison of it
/// is vacuous.
///
/// **One entry, and it is a statement about the corpus rather than a
/// convenience.** `less-cjk.raw` is the only recording that leaves the
/// alternate screen, and what it leaves behind is the shell's screen before
/// `less` started, which is blank. Every CJK cluster in that recording is
/// drawn *inside* the alternate screen and is not on the grid this compares.
/// So the clustering gap is pinned by `cat-cjk` at 355 and `vim-unicode` at
/// 517, and `less-cjk` contributes nothing to it either way.
const ENDS_BLANK: [&str; 1] = ["utf8/less-cjk.raw"];

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

/// **The gap between what a recording contains and what the protocol has.**
///
/// Counted over all nine recordings in `tests/recordings/`: **zero** HT, zero
/// SO, zero SI, zero `ESC ( 0` designations, one alternate-screen *exit*, and
/// OSC in one file. So the pinned differentials above -- which are the only
/// statement anywhere that `baud-core` renders what `baud-core2` renders --
/// are silent on tabs, locking shifts, line drawing and alt-screen restore.
///
/// That is the same reasoning the wrap fixture above was written for: *a real
/// recording only covers what it happens to contain.* These drive the cases no
/// capture reaches, and they are deliberately allowed to fail: a fixture that
/// reports agreement baud-core has not earned is worth nothing.
fn diff_at(bytes: &[u8]) -> Vec<usize> {
    let (a, b) = (old_screen(bytes, old::READ_CHUNK), new_screen(bytes, 4096));
    (0..a.len()).filter(|&i| a[i] != b[i]).collect()
}

/// Render one row of each core, for a message that names what actually differs.
fn row_pair(bytes: &[u8], y: usize) -> (String, String) {
    let (a, b) = (old_screen(bytes, old::READ_CHUNK), new_screen(bytes, 4096));
    let take = |v: &Vec<char>| v[y * COLS..y * COLS + 24].iter().collect::<String>();
    (take(&a), take(&b))
}

// ── The fixture payloads ──────────────────────────────────────────────────
//
// **Named once, because two tests drive each of them.** The gap test below
// pins what the two cores disagree about at one read size;
// `every_fixture_renders_the_same_at_every_read_size` drives the identical
// bytes at eight. A payload spelled twice would let those two answer for
// different streams, which is the shape this file already records for a
// benchmark and a differential quoting the same recording name.

/// HT against three stop regimes: the default table, one stop set at column 6,
/// and no stops at all.
const TAB: &[u8] = b"a\tb\tc\r\n\x1b[3g\x1b[6GX\x1bH\x1b[1GA\tB\r\n\x1b[3gP\tQ";

/// Enter the alternate screen over two used rows, write in it, leave.
/// The `!` after the exit pins the DECRC half of `?1049l`, which does work.
const ALT: &[u8] = b"PRIMARY\r\nSECOND\x1b[?1049hALT\x1b[?1049l!";

/// Designation into each slot against invocation with SI/SO.
const CHARSET: &[u8] =
    b"\x1b)0qqq\r\n\x0eqqq\r\n\x0fqqq\r\n\x1b(0qqq\r\n\x1b(Bqqq\r\n\x1b)0q\x0eq\x0fq\r\n";

/// Every way a control string can end: BEL, ST, an embedded C0, CAN and SUB.
const OSC: &[u8] = b"\x1b]0;title\x07AAA\r\n\x1b]0;title\x1b\\BBB\r\n\x1b]8;;http://example.com\x1b\\CCC\x1b]8;;\x1b\\DDD\r\n\x1b]0;a\nb\rc\x07EEE\r\n\x1b]0;t\x18FFF\x07GGG\r\n\x1b]0;t\x1aHHH\x07III";

/// CAN and SUB inside a CSI, and CAN straight after a bare ESC.
const CANCEL: &[u8] = b"\x1b[1;5\x18~ab\r\n\x1b[1;5\x1a~cd\r\n\x1b\x18~ef";

/// **HT is dropped, and the tab machinery it should reach is already there.**
///
/// `baud-core`'s `c0()` names `0x0a`/`0x0b`/`0x0c`, `0x0d` and `0x08`, and
/// sends every other C0 to `Act::Consume` -- so the tab character is consumed
/// and the cursor does not move. The stops themselves are maintained:
/// `tab_forward`, `tab_back` and `reset_tabs` all exist, `tabs[x]` is read in
/// three places, and `HTS`/`TBC` write it. They are reachable only through
/// `CSI` -- `CHT` and `CBT` -- so the operation works and the *character* does
/// not.
///
/// `[[source: ctlseqs]]`: HT moves to the next tab stop, which defaults to
/// every eighth column.
#[test]
fn a_tab_moves_the_cursor_the_way_core2_moves_it() {
    // Three stop regimes, so a table-driven HT is told apart from `8 - (x%8)`
    // arithmetic -- which the default stops alone cannot do, because both
    // cores set a stop at every eighth column and the two agree there.
    //
    // Row 0: default stops. Row 1: `TBC 3` clears them all, `CHA 6` moves to
    // column 6, `HTS` sets a stop *there*, so the tab from column 1 lands on
    // 6 rather than on 8. Row 2: stops cleared and never re-set, so HT runs to
    // the right margin.
    let bytes = TAB;
    let d = diff_at(bytes);
    let (x, y) = row_pair(bytes, 0);
    assert!(
        d.is_empty(),
        "{} cells differ -- HT is not moving the cursor.\n  core2: {:?}\n  new  : {:?}",
        d.len(),
        x,
        y,
    );
}

/// **Leaving the alternate screen must give the primary screen back.**
///
/// `baud-core`'s `Storage` is arena, viewport, extents, template, tabs and
/// reply -- there is no second grid. `set_alt_screen(Set)` saves the cursor and
/// calls `clear_screen()` on the one buffer, and the exit arm restores the
/// cursor and nothing else, so everything written before entering is gone.
/// `baud-core2` carries `alt: [PackedCell; GRID]` beside the primary.
///
/// One recording in the corpus contains an alt-screen exit, and it is
/// `less-cjk`, which already diverges by 2,185 cells for an unrelated reason --
/// so this case has never been isolated.
/// **Pinned at the gap rather than at zero, because the gap is real.** Nine
/// cells -- `ALTERNATE` sitting where `PRIMARY` was. Fixing it means a second
/// `[PackedCell]` in `Storage`, which is a caller-provided slice and so an API
/// change across seven call sites; that belongs in its own measured commit
/// rather than tacked onto a fixture.
///
/// **This is a shipping blocker for `baud-core` alone.** Every full-screen
/// program in the corpus enters the alternate screen, and leaving it has to
/// give the shell its output back.
#[test]
fn leaving_the_alternate_screen_restores_the_primary() {
    // **Two kinds of divergence, and only a one-buffer core produces both.**
    // Row 0 is char-against-char -- `ALT` written over `PRIMARY` -- which says
    // the alternate buffer wrote into the primary. Row 1 is char-against-blank
    // -- `SECOND` gone -- which says the *entry* clear destroyed it. The `!`
    // after the exit lands at (1,6) in both cores and must stay equal: it pins
    // the DECRC half of `?1049l`, which does work.
    let bytes = ALT;
    let d = diff_at(bytes);
    let (x, y) = row_pair(bytes, 0);
    assert_eq!(
        d.len(),
        0,
        "the alternate-screen gap moved.\n  core2: {x:?}\n  new  : {y:?}",
    );
}

/// **Designating a character set is not invoking it, and `baud-core` conflates
/// them.**
///
/// `baud-core2` keeps four slots and a `gl` pointer: `ESC ( 0` writes G0,
/// `ESC ) 0` writes G1, and SO/SI move `gl` between them. `baud-core` has one
/// `charset` field -- `write.rs` matches `b'(' | b')' | b'*' | b'+'` onto it
/// and `grep` for a `GSlot` or a `g: [Charset` array over the whole crate
/// returns nothing -- so `ESC ) 0` switches the *displayed* text to line
/// drawing immediately, and SI cannot switch it back because there is nothing
/// to switch to.
///
/// Six rows, one arm each, and three of them are controls that must agree:
/// designating into G0 really does change the display, `ESC ( B` really does
/// restore ASCII, and the run in row 1 agrees by coincidence rather than by
/// correctness. `q` is U+2500 in the DEC set, so the two cores disagree about
/// a glyph rather than about a column.
///
/// Goes to zero the day `baud-core` grows the four slots. **A partial fix that
/// honours `(` and ignores `) * +` takes it to three, not zero**, which is the
/// discrimination this shape was chosen for.
#[test]
fn designating_a_charset_is_not_invoking_it() {
    let bytes = CHARSET;
    let d = diff_at(bytes);
    let (x, y) = row_pair(bytes, 0);
    assert_eq!(
        d.len(),
        0,
        "the charset gap moved.\n  core2: {x:?}\n  new  : {y:?}",
    );
}

/// **`CAN` and `SUB` end a control string, and `baud-core`'s graph has no arm
/// for either.**
///
/// `[[source: williams]]`: "CAN and SUB cancel any escape sequence".
/// `baud-core`'s `string()` had three arms -- `0x07` to Ground, `0x1b` to
/// StrEsc, everything else back to Str -- so a `CAN` inside an `OSC` was
/// swallowed as body text and the string ran on, eating the characters after
/// it until a real terminator arrived. **Pinned at 12, now 0**: the `anywhere`
/// rule in `graph.rs` closed rows 4 and 5 together with the fixture below.
///
/// **Rows 0-3 are the part worth keeping even though they always agreed.** A
/// window title changes no cell and an `OSC 8` link rides in `CellFlags`,
/// which the glyph half does not read -- so well-formed OSC is *invisible*
/// here by construction, and pinning that it consumes exactly its own bytes
/// and no more is the only thing this comparison can say about it. Rows 4 and
/// 5 were the defect, and they were visible because the swallow shifted real
/// text.
#[test]
fn a_control_string_ends_where_the_specification_says_it_does() {
    let bytes = OSC;
    let d = diff_at(bytes);
    let (x, y) = row_pair(bytes, 4);
    assert_eq!(
        d.len(),
        0,
        "the control-string gap moved.\n  core2 row4: {x:?}\n  new   row4: {y:?}",
    );
}

/// **Outside a control string the same missing rule was worse than a swallow:
/// `baud-core` performed an operation `baud-core2` cancelled.** Pinned at 9,
/// now 0.
///
/// `CAN` inside a `CSI` fell through the parameter arm, so the parser stayed
/// in `CsiParam` and consumed the *next* byte in `0x40..=0x7e` as a final. The
/// fixture uses `~` -- a final this core does not implement -- deliberately,
/// so the pin does not drift when the operation table grows. With `I` instead
/// the consequence is concrete and was measured: core2 renders `IJK` at
/// columns 0-2 where `baud-core` dispatched `CSI 1 ; 5 I` as `CHT` and
/// rendered `JK` at columns 8-9. It moved the cursor because a cancel was
/// ignored, which no amount of swallowed text is equivalent to.
///
/// `ESC CAN` was the same shape one node up: `escape()` routed a C0 through
/// `c0()` and stayed in `Escape`, so the next printable was eaten as a final.
///
/// This is the third face of one missing edge, and the one no recording could
/// ever show -- `0x18` and `0x1a` are absent from all nine. It stays at zero
/// because one arm in `one()` cannot be half-applied: the same test answers
/// for every node, so a node added later inherits the rule rather than
/// forgetting it, which is how all six forgot it in the first place.
#[test]
fn a_cancel_outside_a_string_cancels_the_sequence() {
    let bytes = CANCEL;
    let d = diff_at(bytes);
    let (x, y) = row_pair(bytes, 0);
    assert_eq!(
        d.len(),
        0,
        "the cancel gap moved.\n  core2: {x:?}\n  new  : {y:?}",
    );
}

/// A primary that has scrolled, an alternate screen that scrolls too, and the
/// exit. **This is the shape one round trip cannot see.**
const ALT_ROT: &[u8] = b"P00\r\nP01\r\nP02\r\nP03\r\nP04\r\nP05\r\nP06\r\nP07\r\nP08\r\nP09\r\nP10\r\nP11\r\nP12\r\nP13\r\nP14\r\nP15\r\nP16\r\nP17\r\nP18\r\nP19\r\nP20\r\nP21\r\nP22\r\nP23\r\nP24\r\nP25\r\nP26\r\nP27\r\nP28\r\nP29\r\nP30\r\nP31\r\nP32\r\nP33\r\nP34\r\nP35\r\nP36\r\nP37\r\nP38\r\nP39\r\nP40\r\nP41\r\nP42\r\nP43\r\nP44\r\nP45\r\nP46\r\nP47\r\nP48\r\nP49\r\nP50\r\nP51\r\nP52\r\nP53\r\nP54\r\nP55\r\nP56\r\nP57\r\nP58\r\nP59\r\n\x1b[?1049hA00\r\nA01\r\nA02\r\nA03\r\nA04\r\nA05\r\nA06\r\nA07\r\nA08\r\nA09\r\nA10\r\nA11\r\nA12\r\nA13\r\nA14\r\nA15\r\nA16\r\nA17\r\nA18\r\nA19\r\nA20\r\nA21\r\nA22\r\nA23\r\nA24\r\nA25\r\nA26\r\nA27\r\nA28\r\nA29\r\nA30\r\nA31\r\nA32\r\nA33\r\nA34\r\nA35\r\nA36\r\nA37\r\nA38\r\nA39\r\nA40\r\nA41\r\nA42\r\nA43\r\nA44\r\nA45\r\nA46\r\nA47\r\nA48\r\nA49\r\nA50\r\nA51\r\nA52\r\nA53\r\nA54\r\nA55\r\nA56\r\nA57\r\nA58\r\nA59\r\nA60\r\nA61\r\nA62\r\nA63\r\nA64\r\nA65\r\nA66\r\nA67\r\nA68\r\nA69\r\nA70\r\nA71\r\nA72\r\nA73\r\nA74\r\nA75\r\nA76\r\nA77\r\nA78\r\nA79\r\nA80\r\nA81\r\nA82\r\n\x1b[?1049l\x1b[41;1H\x1b[J!";

/// **Both surfaces scroll, so the rotation has to be parked with the cells.**
///
/// `leaving_the_alternate_screen_restores_the_primary` enters, writes and
/// leaves once, and a restore that dropped the rotation would still pass it:
/// nothing has scrolled, so screen row `y` is physical row `y` on both sides
/// and the wrong rotation is the identity.
///
/// Here the primary scrolls sixty lines onto a fifty-row screen before the
/// alternate screen is entered, and the alternate scrolls sixty more. `RowRot`
/// is what makes a physical row out of a screen row, so restoring the cells
/// under the *alternate's* rotation would bring the primary back with its rows
/// in the wrong order -- every row wrong, silently, with no crash and no cell
/// count to disagree.
///
/// This is the failure `baud-core2` records from the other side: its own
/// note says the accessor pairing a rotation with a buffer exists because of a
/// bug "before the alternate screen had a rotation", and CLAUDE.md records a
/// parallel array not moving with `rotate_left` breaking
/// `the_alt_screen_never_leaks_into_the_primary` at 20,000 cases. Here the
/// pairing is in the signature -- `View::save_to` and `View::restore_from`
/// take cells, extents and rotation together -- and this is the payload that
/// proves the signature is doing the work.
#[test]
fn a_scrolled_primary_comes_back_in_the_order_it_left() {
    let d = diff_at(ALT_ROT);
    let (x, y) = row_pair(ALT_ROT, 0);
    assert_eq!(
        d.len(),
        0,
        "the primary came back rotated wrongly.\n  core2: {x:?}\n  new  : {y:?}",
    );
}

/// **`less-cjk` compared where its content is, which is one sequence before
/// the end.**
///
/// Its row in `the_glyph_gap_is_pinned` is blank against blank and pins
/// nothing: the recording's **last eight bytes** are `ESC [ ? 1 0 4 9 l`, so
/// the final screen is the restored primary and every CJK cluster `less` drew
/// is behind it. Left at that, 84 KB of the most cluster-dense text in the
/// corpus would contribute nothing to the differential and any future change
/// that broke its rendering would be invisible.
///
/// So both cores are fed the bytes up to that exit. **This is not the prefix
/// bisect this repo records as a failure** -- nothing is written to
/// `tests/recordings/`, no throughput or `cells_written` number is taken off a
/// cut, and the recording is untouched. Two implementations receive identical
/// bytes and their cells are compared, which is what this file already does at
/// eight chunk sizes.
///
/// **452 of 1,238 non-blank cells**, and they are clustering: a ZWJ, a
/// variation selector or a combining mark takes its own cell where core2
/// attaches it to the base, and everything after it on the row shifts a
/// column. It joins `cat-cjk`'s 355 and `vim-unicode`'s 517 and goes to zero
/// with them the day `baud-core` grows a cluster machine.
#[test]
fn less_cjk_agrees_where_its_content_is() {
    let b = recording("utf8/less-cjk.raw");
    let at = (0..=b.len() - 8)
        .filter(|&i| &b[i..i + 8] == b"\x1b[?1049l")
        .next_back()
        .expect("less-cjk leaves the alternate screen");
    let (x, y) = (
        old_screen(&b[..at], old::READ_CHUNK),
        new_screen(&b[..at], 4096),
    );
    // The guard the end-of-stream row needed: a comparison with nothing in it
    // cannot fail, and that is the whole reason this test exists.
    let content = x.iter().filter(|c| **c != ' ' && **c != '\0').count();
    assert!(
        content > 1000,
        "nothing on screen at the exit: {content} cells"
    );
    let d = (0..x.len()).filter(|&i| x[i] != y[i]).count();
    assert_eq!(d, 0, "less-cjk's clustering gap moved");
}

/// **A combining mark takes no cell, and the codepoint is recoverable.**
///
/// Both halves matter and the second is the one that needed a store. Giving a
/// zero-width codepoint no cell is what stops everything after it on the row
/// sitting a column right of where `baud-core2` puts it. *Keeping* it is what
/// stops that being the defect core2 already fixed once -- its own source:
/// "every combining mark was silently thrown away: `e` + U+0301 stored `e` and
/// the accent was gone -- not mis-measured, absent, and unrecoverable."
///
/// The glyph differential cannot check the second half. It compares cell
/// content, and a continuation writes no cell, so a core that stores marks and
/// one that discards them are identical to it. **This test is the only thing
/// that can tell them apart**, which is why it asserts the codepoint by value
/// rather than asserting a count is zero -- the assertion
/// `a_zwj_family_keeps_all_of_its_codepoints` made while attaching every
/// family to the wrong cell.
#[test]
fn a_combining_mark_takes_no_cell_and_is_still_recoverable() {
    let bytes = "e\u{301}X".as_bytes();
    assert_eq!(diff_at(bytes).len(), 0, "the mark still takes a cell");

    let (marks, dropped) = new_marks(bytes, 0, 0);
    assert_eq!(marks, vec![0x301], "the accent is not recoverable");
    assert_eq!(dropped, 0, "it was dropped rather than stored");

    // The base is unchanged, so a renderer reads `e` and asks for its marks.
    // Column 1 is the `X`, which owns nothing.
    assert!(new_marks(bytes, 0, 1).0.is_empty(), "X inherited a mark");
}

/// **The store survives a scroll, and `baud-core2`'s does not survive a
/// frame.**
///
/// This is the case that decided the design. core2 keys marks on surface,
/// physical row, column and generation; `Term::viewport` un-rotates the grid
/// in place and does not move the table with it, so an adversarial review
/// reproduced a renderer reading **one frame** and the marks being gone --
/// then the next write to that address inheriting a stranger's.
///
/// Here the index rides in `CellFlags`, so it moves when the cell moves. The
/// payload scrolls the mark's row up by fifty-three lines, which is more than
/// the screen holds, so `RowRot` is non-zero and the cell is nowhere near
/// where it was written.
#[test]
fn a_mark_survives_the_grid_moving_under_it() {
    let mut v = String::from("e\u{301}\r\n");
    for i in 0..53 {
        v.push_str(&std::format!("line{i}\r\n"));
    }
    let b = v.as_bytes();
    // The mark's row has scrolled off, so nothing should be recoverable at the
    // top -- but nothing should be *wrong* there either.
    for c in 0..3 {
        assert!(
            new_marks(b, 0, c).0.is_empty(),
            "a scrolled-away mark came back at row 0 col {c}",
        );
    }

    // **Scroll first, then write the mark, then scroll again**, so `RowRot` is
    // non-zero *and* the mark's row is still on screen. A first version wrote
    // the mark and then ten lines onto a fifty-row screen -- which does not
    // scroll at all, so it asserted recovery from a grid that had not moved.
    let mut v = String::new();
    for i in 0..60 {
        v.push_str(&std::format!("pre{i}\r\n"));
    }
    v.push_str("e\u{301}\r\n");
    for i in 0..5 {
        v.push_str(&std::format!("post{i}\r\n"));
    }
    let b = v.as_bytes();
    let found: Vec<usize> = (0..ROWS)
        .filter(|&y| new_marks(b, y, 0).0 == vec![0x301])
        .collect();
    assert_eq!(
        found.len(),
        1,
        "the mark should be on exactly one row after the grid rotated, found {found:?}",
    );
}

/// The marks at `(r, c)` and how many codepoints `baud-core` refused.
fn new_marks(bytes: &[u8], r: usize, c: usize) -> (Vec<u32>, usize) {
    let ws = new::WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span = new::RowBlock::leading(rows, new::Cols::from_stride(new::MAX_COLS));
    let mut arena = vec![0u8; new::ARENA];
    let mut vp = vec![new::PackedCell::EMPTY; span.len()];
    let mut used = vec![new::RowExtent::ZERO; rows.index()];
    let mut cells3: Vec<new::PackedCell> = Vec::new();
    let mut tmpl = vec![None; new::NSTYLE];
    let mut tabs = vec![false; new::MAX_COLS];
    let mut reply = vec![0u8; new::REPLY];
    let mut alt = vec![new::PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![new::RowExtent::ZERO; rows.index()];
    let mut clusters = vec![new::ClusterSlot::EMPTY; new::NCLUSTER];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
        ),
        &ws,
    );
    t.write_at(new::BytePos::ZERO, bytes);
    t.feed(new::BytePos::ZERO, new::ReadLen::from_index(bytes.len()));
    let m: Vec<u32> = t
        .marks(new::Row::from_index(r), new::Col::at_index(c))
        .collect();
    (m, t.marks_dropped)
}

/// **The gap tests drive one read size, and it is the one where the
/// accelerated path is live.**
///
/// `diff_at` renders `baud-core` at 4096, so every pin above says the two
/// cores agree *at that chunk*. It says nothing about whether `fast_csi` and
/// `fast_esc` -- which take a whole sequence in one loop iteration -- agree
/// with the graph they are allowed to decline to. That contract is
/// **may only decline, never disagree**, and this repo has paid for the
/// alternative once: `csi_whole` leaked half-parsed parameters on the way out.
///
/// Read size is the instrument that separates them. **At `CHUNK=1` the
/// accelerated path can never fire**, because a whole sequence is never in one
/// read; at 400000 it always does. So identical cells across the range means
/// both paths render the same screen.
///
/// It matters most for exactly these payloads: `0x18` and `0x1a` appear in no
/// recording in the corpus, so the chunk sweep run over `tests/recordings/`
/// cannot say anything at all about the bytes the cancel edge changed.
#[test]
fn every_fixture_renders_the_same_at_every_read_size() {
    for (name, bytes) in [
        ("tab", TAB),
        ("alt", ALT),
        ("charset", CHARSET),
        ("osc", OSC),
        ("cancel", CANCEL),
        ("alt-rot", ALT_ROT),
    ] {
        let want = new_screen(bytes, 4096);
        for k in [1usize, 2, 3, 5, 17, 512, 4096, 400_000] {
            let got = new_screen(bytes, k);
            let d = (0..want.len()).filter(|&i| want[i] != got[i]).count();
            assert_eq!(d, 0, "{name}: chunk {k} renders differently from 4096");
        }
    }
}

/// **The key encoder is shared, so what this compares is the plumbing.**
///
/// `baud_domain::encode_key` is one function and both cores call it, which
/// makes the bytes it produces the same by construction. What is *not* shared
/// is how each core arrives at the five inputs: `baud-core` reads
/// `cold.modes` and `cold.keypad`, `baud-core2` reads its own fields, and each
/// applies DECCKM, DECKPAM, LNM, `?1036` and `?1039` through its own mode
/// path. A core that recognised `?1` and never set the bit would encode every
/// arrow key wrongly and no test of the encoder could see it.
///
/// So each row sets modes by **feeding both cores the same bytes**, then asks
/// both for the same key. That is the same instrument as the rest of this
/// file, pointed at the input side.
///
/// Two inputs are constants in `baud-core` and are excluded deliberately:
/// `S8C1T`, which it does not implement, and VT52, which it cannot enter --
/// so every row here is 7-bit and ANSI, and a row that set them would be
/// comparing a core against a feature it does not claim.
#[test]
fn both_cores_encode_a_key_the_same_way() {
    use new::{KeyCode, KeyEvent, Modifiers};
    // `(what sets the mode, what to press)`. The setup bytes go through the
    // parser, so this exercises the mode path rather than a setter.
    let rows: &[(&[u8], KeyEvent)] = &[
        (b"", KeyEvent::press(KeyCode::Up, Modifiers::NONE)),
        // DECCKM: the arrows become `SS3 A` rather than `CSI A`.
        (b"\x1b[?1h", KeyEvent::press(KeyCode::Up, Modifiers::NONE)),
        (
            b"\x1b[?1h\x1b[?1l",
            KeyEvent::press(KeyCode::Up, Modifiers::NONE),
        ),
        // A modified cursor key is `CSI 1 ; mod A` even in application mode.
        (
            b"\x1b[?1h",
            KeyEvent::press(KeyCode::Left, Modifiers::SHIFT),
        ),
        (b"", KeyEvent::press(KeyCode::Home, Modifiers::CTRL)),
        (b"", KeyEvent::press(KeyCode::End, Modifiers::NONE)),
        // The editing keypad does not follow DECCKM.
        (
            b"\x1b[?1h",
            KeyEvent::press(KeyCode::Insert, Modifiers::NONE),
        ),
        (b"", KeyEvent::press(KeyCode::Delete, Modifiers::ALT)),
        (b"", KeyEvent::press(KeyCode::PageUp, Modifiers::NONE)),
        (b"", KeyEvent::press(KeyCode::PageDown, Modifiers::SHIFT)),
        // LNM: Return sends `CR`, and `CR LF` once set.
        (b"", KeyEvent::press(KeyCode::Enter, Modifiers::NONE)),
        (
            b"\x1b[20h",
            KeyEvent::press(KeyCode::Enter, Modifiers::NONE),
        ),
        (b"", KeyEvent::press(KeyCode::Tab, Modifiers::NONE)),
        (b"", KeyEvent::press(KeyCode::Tab, Modifiers::SHIFT)),
        (b"", KeyEvent::press(KeyCode::Backspace, Modifiers::NONE)),
        (b"", KeyEvent::press(KeyCode::Escape, Modifiers::NONE)),
        // Text, and the two modifiers that change the bytes rather than
        // adding a parameter.
        (
            b"",
            KeyEvent::press(KeyCode::Char(cp('a')), Modifiers::NONE),
        ),
        (
            b"",
            KeyEvent::press(KeyCode::Char(cp('a')), Modifiers::CTRL),
        ),
        (b"", KeyEvent::press(KeyCode::Char(cp('a')), Modifiers::ALT)),
        // `?1039` altSendsEscape and `?1036` metaSendsEscape, which the two
        // cores record through different mode tables.
        (
            b"\x1b[?1039h",
            KeyEvent::press(KeyCode::Char(cp('a')), Modifiers::ALT),
        ),
        (
            b"\x1b[?1036h",
            KeyEvent::press(KeyCode::Char(cp('a')), Modifiers::META),
        ),
        (
            b"",
            KeyEvent::press(KeyCode::Char(cp('\u{e9}')), Modifiers::NONE),
        ),
        (
            b"",
            KeyEvent::press(KeyCode::Char(cp('\u{1f600}')), Modifiers::NONE),
        ),
    ];
    for (setup, e) in rows {
        let (a, b) = (core2_key(setup, *e), new_key(setup, *e));
        assert_eq!(a, b, "setup {setup:?} key {:?}", e.code);
    }
}

fn cp(c: char) -> new::Codepoint {
    new::Codepoint::new(u32::from(c)).expect("a real character")
}

/// Feed `setup` to `baud-core2`, press `e`, and return what it replies.
fn core2_key(setup: &[u8], e: new::KeyEvent) -> Vec<u8> {
    let mut store = old::Storage::boxed();
    let mut t = old::Term::new(&mut store, old::OpMode::Computed);
    let mut i = 0;
    while i < setup.len() {
        let n = old::READ_CHUNK.min(setup.len() - i);
        let mut slot = t.read_slot();
        let base = slot.base();
        let filled = slot.write(old::StreamByte::slice_from(&setup[i..i + n]));
        t.feed(base, filled.len());
        t.commit_read(filled.len());
        i += n;
    }
    t.clear_reply();
    t.key(old_event(e));
    t.reply().to_vec()
}

/// The same event, named through `baud-core2`'s re-export.
///
/// Both crates re-export the *same* `baud_domain` types, so this is an
/// identity -- written out because the two paths are spelled differently and a
/// reader should be able to see that nothing is converted.
fn old_event(e: new::KeyEvent) -> old::KeyEvent {
    e
}

/// Feed `setup` to `baud-core`, press `e`, and return what it replies.
fn new_key(setup: &[u8], e: new::KeyEvent) -> Vec<u8> {
    let ws = new::WinSize {
        ws_row: ROWS as u16,
        ws_col: COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span = new::RowBlock::leading(rows, new::Cols::from_stride(new::MAX_COLS));
    let mut arena = vec![0u8; new::ARENA];
    let mut vp = vec![new::PackedCell::EMPTY; span.len()];
    let mut used = vec![new::RowExtent::ZERO; rows.index()];
    let mut cells3: Vec<new::PackedCell> = Vec::new();
    let mut tmpl = vec![None; new::NSTYLE];
    let mut tabs = vec![false; new::MAX_COLS];
    let mut reply = vec![0u8; new::REPLY];
    let mut alt = vec![new::PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![new::RowExtent::ZERO; rows.index()];
    let mut clusters = vec![new::ClusterSlot::EMPTY; new::NCLUSTER];
    let mut t = new::Term::new(
        new::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
        ),
        &ws,
    );
    let mut i = 0;
    while i < setup.len() {
        let n = new::READ_CHUNK.min(setup.len() - i);
        t.write_at(new::BytePos::ZERO, &setup[i..i + n]);
        t.feed(new::BytePos::ZERO, new::ReadLen::from_index(n));
        i += n;
    }
    t.clear_reply();
    t.key(e);
    t.reply().to_vec()
}
