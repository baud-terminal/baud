//! **The rendition table's generations**: what lies off the active screen is
//! frozen when the table fills, and no slot a drawn cell names is ever given
//! another rendition.
//!
//! `[[source: dec-std-070]]`'s SGR, page 5-104, note 3: "In no way does the
//! selection of current rendition affect the rendition of characters already
//! entered into the display." `[[source: itu-t416]]` § 8.1.7 gives each character
//! its own foreground and background, and `[[source: ecma48]]` § 8.3.117 sets a
//! rendition "for subsequent text".
//!
//! **Each scenario is a differential.** The same bytes and resizes go to a
//! terminal whose table has the 1,024-slot floor, lent generations by hand as
//! an embedder without an allocator lends every buffer, and to one whose
//! table has 65,536 slots and room for every rendition the scenario names, so
//! it never freezes. Every cell of every retained line and of both screens
//! must resolve, through its row's table, to the rendition the reference
//! reads -- and the small one must have frozen, or the scenario tested
//! nothing.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;
use baud_domain::{
    Back, ByteLen, BytePos, CacheGeometry, ClusterSlot, ColorWord, Content, Depth, FreezeCounts,
    FrozenBook, FrozenBudget, FrozenBuf, FrozenGen, ImageSlot, PackedCell, PastEdge, PlacementId,
    PlacementSlot, Reach, ReadLen, Row, Rows, StyleId, StyleWord,
};

/// Every buffer a `Storage` borrows, lent by hand, at a chosen table size and
/// with or without generations: the embedder that places its memory itself.
struct Lent {
    arena: Vec<u8>,
    vp: Vec<PackedCell>,
    used: Vec<RowMeta>,
    past: Vec<PastEdge>,
    cells3: Vec<PackedCell>,
    tmpl: Vec<Option<StyleWord>>,
    order: Vec<StyleId>,
    book: StyleBook,
    reach: Vec<Reach>,
    tabs: Vec<bool>,
    reply: Vec<u8>,
    alt: Vec<PackedCell>,
    alt_used: Vec<RowMeta>,
    clusters: Vec<ClusterSlot>,
    pending: Vec<u8>,
    title: Vec<u8>,
    icon: Vec<u8>,
    titles: Vec<u8>,
    cwd: Vec<u8>,
    palette: Vec<ColorWord>,
    links: Vec<u8>,
    link_len: Vec<ByteLen>,
    clip: Vec<u8>,
    img_pixels: Vec<baud_domain::Pixel>,
    img_slots: Vec<ImageSlot>,
    img_places: Vec<PlacementSlot>,
    img_index: Vec<PlacementId>,
    sixel: Box<baud_domain::SixelStream>,
    regis: Box<baud_domain::Regis>,
    kitty: Box<baud_domain::KittyStream>,
    colours: Box<KittyColours>,
    kitty_scratch: Vec<u8>,
    status: Vec<PackedCell>,
    status_used: Vec<RowMeta>,
    words: Vec<StyleWord>,
    gens: Vec<FrozenGen>,
    tags: Vec<Option<baud_domain::RowFrozen>>,
    spare: Vec<Option<baud_domain::GenIdx>>,
    map: Vec<baud_domain::Remap>,
    frozen: FrozenBook,
    stride: baud_domain::Stride,
    depth: Depth,
}

/// Whether a storage is lent generations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pool {
    Lent,
    None,
}

impl Lent {
    fn new(ws: &WinSize, depth: Depth, slots: usize, pool: Pool) -> Lent {
        let (cols, rows) = ws.geometry();
        let stride = grid_stride(&CacheGeometry::UNKNOWN, cols);
        let ring_rows = Rows::of_index(rows.index() + depth.lines());
        let ring = ring_rows.index() * stride.index();
        let span = rows.index() * stride.index();
        let budget = match pool {
            Pool::Lent => FrozenBudget::of(ring_rows, stride, slots),
            Pool::None => FrozenBudget::NONE,
        };
        Lent {
            arena: vec![0; ARENA],
            vp: vec![PackedCell::EMPTY; ring],
            used: vec![RowMeta::ZERO; ring_rows.index()],
            past: vec![PastEdge::Clear; ring_rows.index()],
            cells3: Vec::new(),
            tmpl: vec![None; slots],
            order: vec![StyleId::DEFAULT; slots],
            book: StyleBook::new(),
            reach: vec![Reach::ZERO; slots],
            tabs: vec![false; MAX_COLS],
            reply: vec![0; REPLY],
            alt: vec![PackedCell::EMPTY; span],
            alt_used: vec![RowMeta::ZERO; rows.index()],
            clusters: vec![ClusterSlot::EMPTY; NCLUSTER],
            pending: vec![0; OSC_MAX],
            title: vec![0; OSC_MAX],
            icon: vec![0; OSC_MAX],
            titles: vec![0; TITLE_STACK],
            cwd: vec![0; OSC_MAX],
            palette: vec![ColorWord::DEFAULT; PALETTE_LEN],
            links: vec![0; NLINK * LINK_MAX],
            link_len: vec![ByteLen::ZERO; NLINK],
            clip: Vec::new(),
            img_pixels: Vec::new(),
            img_slots: Vec::new(),
            img_places: Vec::new(),
            img_index: vec![PlacementId::NONE; PlacementId::index_len(0)],
            sixel: Box::new(baud_domain::SixelStream::NEW),
            regis: Box::new(baud_domain::Regis::NEW),
            kitty: Box::new(baud_domain::KittyStream::NEW),
            colours: Box::new(KittyColours::NEW),
            kitty_scratch: Vec::new(),
            status: vec![PackedCell::EMPTY; stride.index()],
            status_used: vec![RowMeta::ZERO; 1],
            words: vec![StyleWord::DEFAULT; budget.words],
            gens: vec![FrozenGen::EMPTY; budget.gens.max(1)],
            tags: vec![None; ring_rows.index()],
            spare: vec![None; ring_rows.index()],
            map: vec![baud_domain::Remap::UNSET; slots],
            frozen: FrozenBook::NEW,
            stride,
            depth,
        }
    }

    fn term(&mut self, ws: &WinSize) -> Term<'_> {
        let storage = Storage::new(
            &mut self.arena,
            self.stride,
            self.depth,
            &mut self.vp,
            &mut self.used,
            &mut self.past,
            &mut self.cells3,
            StyleTable {
                slots: &mut self.tmpl,
                order: &mut self.order,
                book: &mut self.book,
                reach: &mut self.reach,
            },
            &mut self.tabs,
            &mut self.reply,
            &mut self.alt,
            &mut self.alt_used,
            &mut self.clusters,
            StrBuf::new(
                &mut self.pending,
                &mut self.title,
                &mut self.icon,
                &mut self.palette,
                &mut self.links,
                &mut self.link_len,
                &mut self.clip,
                &mut self.sixel,
                &mut self.regis,
                &mut self.kitty,
                &mut self.colours,
                &mut self.kitty_scratch,
                &mut self.titles,
                &mut self.cwd,
            ),
            baud_domain::ImageStore::new(
                &mut self.img_pixels,
                &mut self.img_slots,
                &mut self.img_places,
                &mut self.img_index,
            ),
        )
        .with_status_line(StatusBuf::new(&mut self.status, &mut self.status_used))
        .with_frozen(FrozenBuf {
            words: &mut self.words,
            gens: &mut self.gens,
            tags: &mut self.tags,
            spare: &mut self.spare,
            map: &mut self.map,
            book: &mut self.frozen,
        });
        Term::new(storage, ws)
    }
}

/// One step of a scenario.
#[derive(Clone, Debug)]
enum Step {
    Bytes(Vec<u8>),
    /// Columns, then rows.
    Resize(u16, u16),
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    feed_by(t, bytes, READ_CHUNK);
}

fn feed_by(t: &mut Term<'_>, bytes: &[u8], read: usize) {
    for piece in bytes.chunks(read) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
        t.clear_reply();
    }
}

fn run(t: &mut Term<'_>, steps: &[Step]) {
    for s in steps {
        match s {
            Step::Bytes(b) => feed(t, b),
            Step::Resize(cols, rows) => {
                let _ = t.resize(&WinSize {
                    ws_col: *cols,
                    ws_row: *rows,
                });
            }
        }
    }
}

/// What a terminal shows: every retained line of the primary, oldest first,
/// then the screen up, each cell's content and the rendition its row's table
/// resolves its id to.
fn read(t: &Term<'_>) -> Vec<Vec<(Content, StyleWord)>> {
    let mut out = Vec::new();
    let sb = t.scrollback();
    for back in (0..sb.len()).rev() {
        let back = Back::of(back);
        let (cells, _) = sb.line(back).expect("a retained line");
        let table = sb.styles(back);
        out.push(
            cells
                .iter()
                .map(|c| (c.content, table.word(t.styles(), c.style)))
                .collect(),
        );
    }
    let view = t.viewport();
    for y in 0..view.rows().index() {
        let r = Row::from_index(y);
        let table = view.styles(r);
        out.push(
            view.row(r)
                .iter()
                .map(|c| (c.content, table.word(t.styles(), c.style)))
                .collect(),
        );
    }
    out
}

/// The scenario on a 1,024-slot table lent generations and on a 65,536-slot
/// reference, compared as it ends and again on the other screen, entered with
/// `?47` and left; the small one's counts.
fn differential(ws: &WinSize, depth: Depth, steps: &[Step]) -> Seen {
    let mut small = Lent::new(ws, depth, NSTYLE, Pool::Lent);
    let mut big = Lent::new(ws, depth, NSTYLE_MAX, Pool::None);
    let mut s = small.term(ws);
    let mut b = big.term(ws);
    run(&mut s, steps);
    run(&mut b, steps);
    compare(&s, &b, "as it ended");
    // What the reference reads names this many renditions: past the small
    // table's slots, the small one could not have held them all.
    let distinct = read(&b)
        .into_iter()
        .flatten()
        .map(|c| zerocopy::IntoBytes::as_bytes(&c.1).to_vec())
        .collect::<std::collections::HashSet<_>>()
        .len();
    let other = match s.screen() {
        Screen::Normal => b"\x1b[?47h".as_slice(),
        Screen::Alternate => b"\x1b[?47l".as_slice(),
    };
    feed(&mut s, other);
    feed(&mut b, other);
    compare(&s, &b, "on the other screen");
    assert_eq!(s.style_reclaims().refusals, 0, "the small table refused");
    assert_eq!(b.style_reclaims().refusals, 0, "the reference refused");
    Seen {
        counts: s.frozen().counts,
        distinct: baud_domain::Cells::of(distinct),
    }
}

/// What a differential saw: the small table's counts, and how many
/// renditions the reference's cells name.
#[derive(Debug)]
struct Seen {
    counts: FreezeCounts,
    distinct: baud_domain::Cells,
}

impl Seen {
    /// The control every flood needs: more renditions than the small table
    /// holds, and a freeze.
    fn past_the_table(self) -> FreezeCounts {
        let n = self.distinct.index();
        assert!(n > NSTYLE, "only {n} renditions: {self:?}");
        assert!(self.counts.freezes > 0, "nothing froze: {self:?}");
        self.counts
    }
}

fn compare(s: &Term<'_>, b: &Term<'_>, when: &str) {
    let (rs, rb) = (read(s), read(b));
    assert_eq!(rs.len(), rb.len(), "{when}: lines retained");
    let mut wrong = 0;
    let mut first = None;
    for (i, (ls, lb)) in rs.iter().zip(&rb).enumerate() {
        assert_eq!(ls.len(), lb.len(), "{when}: line {i}'s length");
        for (x, (cs, cb)) in ls.iter().zip(lb).enumerate() {
            assert_eq!(cs.0, cb.0, "{when}: line {i} column {x}'s content");
            if cs.1 != cb.1 {
                wrong += 1;
                first = first.or(Some((i, x)));
            }
        }
    }
    assert_eq!(
        wrong, 0,
        "{when}: {wrong} cells in another rendition, first {first:?}"
    );
}

/// Rendition `i` of a set as rich as a picture tool's and richer: an indexed
/// foreground and background, bold on every other, and an underline in an
/// `SGR 58` colour, so every `i` below 65,536 is its own.
fn rendition(i: usize) -> String {
    let b = i.to_le_bytes();
    let bold = match b[0] & 1 {
        0 => "",
        _ => "1;",
    };
    format!(
        "\x1b[0;{bold}4;38;5;{};48;5;{};58;5;{}m",
        b[0],
        b[1],
        b[0] ^ b[1]
    )
}

/// `lines` lines of `cols` cells, each in a rendition of its own from
/// `first` on, each ended by a newline in the default.
fn flood(first: usize, lines: usize, cols: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..cols {
            out.extend(rendition(first + l * cols + x).as_bytes());
            out.push(b'a' + u8::try_from((l + x) % 26).unwrap_or(0));
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

const SMALL: WinSize = WinSize {
    ws_row: 10,
    ws_col: 40,
};

/// **S1: a history of four hundred lines, every cell its own rendition**, a
/// sixteenth of which the table can hold. The eviction this replaced read
/// back 94,465 of 160,000 such cells in another's rendition.
#[test]
fn every_history_cell_keeps_its_rendition_past_many_tables() {
    let c =
        differential(&SMALL, Depth::of(400), &[Step::Bytes(flood(0, 400, 40))]).past_the_table();
    assert!(c.freezes > 3 && c.rows_frozen > 300, "the control: {c:?}");
}

/// **S2: the primary's history, then the alternate screen flooded, and
/// back.** The primary's screen is frozen while the alternate screen fills
/// the table and interned again on the way back.
#[test]
fn the_primary_keeps_its_renditions_through_the_alternate_screen() {
    let c = differential(
        &SMALL,
        Depth::of(200),
        &[
            Step::Bytes(flood(0, 200, 40)),
            Step::Bytes(b"\x1b[?1049h".to_vec()),
            Step::Bytes(flood(9_000, 149, 40)),
            Step::Bytes(b"\x1b[?1049l".to_vec()),
        ],
    )
    .past_the_table();
    assert!(
        c.rows_thawed >= 10,
        "the control: the screen came back: {c:?}"
    );
}

/// **`?47`: the alternate screen kept as it was left, frozen while the
/// primary fills the table, and interned again when it is shown.**
#[test]
fn the_alternate_screen_kept_by_47_comes_back_in_its_renditions() {
    let c = differential(
        &SMALL,
        Depth::of(100),
        &[
            Step::Bytes(b"\x1b[?47h".to_vec()),
            Step::Bytes(flood(20_000, 9, 40)),
            Step::Bytes(b"\x1b[?47l".to_vec()),
            Step::Bytes(flood(0, 100, 40)),
            Step::Bytes(b"\x1b[?47h".to_vec()),
        ],
    )
    .past_the_table();
    assert!(c.rows_thawed > 0 && c.freezes > 0, "the control: {c:?}");
}

/// Twenty reverse indexes at the top, each taking the newest line back, then
/// a line written over each and the lot retired again: S3 with a rendition a
/// cell (`distinct`), S3b with one for the twenty, so no freeze runs between
/// the take-back and the retire.
fn take_back(distinct: Distinct) -> Vec<Step> {
    let mut over = Vec::new();
    for l in 0..20 {
        over.extend(b"\x1b[H\x1bM");
        for x in 0..40 {
            let r = match distinct {
                Distinct::Each => rendition(30_000 + l * 40 + x),
                Distinct::One => rendition(40_000),
            };
            over.extend(r.as_bytes());
            over.push(b'r');
        }
    }
    over.extend(b"\x1b[0m\x1b[10;1H");
    over.extend([b'\n'; 20]);
    vec![Step::Bytes(flood(0, 300, 40)), Step::Bytes(over)]
}

#[derive(Clone, Copy)]
enum Distinct {
    Each,
    One,
}

/// **S3: lines taken back by a reverse index, rewritten and retired again
/// read their new renditions**, not the generation they were frozen into.
#[test]
fn a_line_taken_back_and_retired_again_reads_its_new_renditions() {
    let c = differential(&SMALL, Depth::of(300), &take_back(Distinct::Each)).past_the_table();
    assert!(c.freezes > 0, "the control: {c:?}");
}

/// **S3b: the same with no freeze between the take-back and the retire**, so
/// only the cleared tags keep the old generation from claiming the lines.
#[test]
fn a_line_taken_back_without_a_freeze_after_reads_its_new_rendition() {
    let c = differential(&SMALL, Depth::of(300), &take_back(Distinct::One)).past_the_table();
    assert!(c.freezes > 0, "the control: {c:?}");
}

/// **Rows a reverse scroll pushes past the bottom at line zero are frozen,
/// renumbered with every further reverse scroll, and come back when the
/// window grows.** No line has retired, so the numbering cannot follow the
/// rotation, and each row holds the line after the one it held.
#[test]
fn rows_pushed_past_the_bottom_at_line_zero_come_back_in_their_renditions() {
    let mut steps = vec![Step::Resize(40, 5)];
    let mut b = Vec::new();
    for l in 0..60 {
        b.extend(b"\x1b[H");
        for x in 0..40 {
            b.extend(rendition(l * 40 + x).as_bytes());
            b.push(b'h');
        }
        b.extend(b"\x1b[0m\x1b[H\x1bM");
    }
    // The alternate screen filled and left, then the screen written over in
    // place, so the table passes its load again and freezes the rows pushed
    // last, which the growth shows.
    b.extend(b"\x1b[?47h\x1b[H");
    for i in 0..400 {
        b.extend(rendition(7_000 + i).as_bytes());
        b.push(b'z');
    }
    b.extend(b"\x1b[0m\x1b[?47l\x1b[H");
    for i in 0..200 {
        b.extend(rendition(5_000 + i).as_bytes());
        b.push(b'o');
    }
    steps.push(Step::Bytes(b));
    steps.push(Step::Resize(40, 10));
    let seen = differential(&SMALL, Depth::of(100), &steps);
    assert!(
        seen.distinct.index() > 300,
        "the control: the rows came back coloured: {seen:?}"
    );
    let c = seen.counts;
    assert!(c.freezes > 0 && c.rows_thawed > 0, "the control: {c:?}");
}

/// **A taller window pulls frozen history back onto the screen, thawed**, and
/// a shorter one cuts rows that come back with it.
#[test]
fn a_resize_brings_frozen_rows_back_onto_the_screen() {
    let tall = WinSize {
        ws_row: 10,
        ws_col: 40,
    };
    let c = differential(
        &tall,
        Depth::of(200),
        &[
            Step::Resize(40, 4),
            Step::Bytes(flood(0, 200, 40)),
            Step::Bytes(b"\x1b[2;1H".to_vec()),
            Step::Resize(40, 3),
            Step::Bytes(flood(10_000, 40, 40)),
            Step::Resize(40, 10),
        ],
    )
    .past_the_table();
    assert!(c.rows_thawed >= 5, "the control: rows came back: {c:?}");
}

/// **A change of width re-cuts frozen history**: lines wrapped across a
/// freeze, part in a generation and part live, are made one before the
/// re-cut joins their rows, and the rows that land on the screen thawed.
#[test]
fn a_re_cut_keeps_every_frozen_cell_in_its_rendition() {
    let mut wrapped = Vec::new();
    for i in 0..6_000 {
        wrapped.extend(rendition(i).as_bytes());
        wrapped.push(b'w');
        if i % 230 == 229 {
            wrapped.extend(b"\x1b[0m\r\n");
        }
    }
    let c = differential(
        &SMALL,
        Depth::of(200),
        &[
            Step::Bytes(wrapped),
            Step::Resize(23, 10),
            Step::Resize(37, 10),
        ],
    )
    .past_the_table();
    assert!(c.freezes > 0, "the control: {c:?}");
}

/// **A line naming more renditions across its tables than a generation
/// holds is cut where its table changes, not joined**, and every cell keeps
/// its rendition in reading order: the re-cut differs from one that joins,
/// so the cells are read in order rather than row by row.
#[test]
fn a_line_too_many_renditions_for_one_generation_is_cut_not_recoloured() {
    let mut one_line = Vec::new();
    for i in 0..3_000 {
        one_line.extend(rendition(i).as_bytes());
        one_line.push(b'L');
    }
    one_line.extend(b"\x1b[0m\r\n");
    let steps = [Step::Bytes(one_line), Step::Resize(30, 10)];
    let mut small = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::Lent);
    let mut big = Lent::new(&SMALL, Depth::of(200), NSTYLE_MAX, Pool::None);
    let mut s = small.term(&SMALL);
    let mut b = big.term(&SMALL);
    run(&mut s, &steps);
    run(&mut b, &steps);
    let ordered = |t: &Term<'_>| -> Vec<(Content, StyleWord)> {
        read(t)
            .into_iter()
            .flatten()
            .filter(|c| c.0 != Content::EMPTY)
            .collect()
    };
    let (os, ob) = (ordered(&s), ordered(&b));
    assert_eq!(os.len(), ob.len(), "every character kept");
    let wrong = os.iter().zip(&ob).filter(|(a, b)| a != b).count();
    assert_eq!(wrong, 0, "cells in another rendition");
    let c = s.frozen().counts;
    assert!(c.lines_split > 0, "the control: the line was cut: {c:?}");
}

/// **`ED 3` frees the generations at once**: the history is gone, and so are
/// the words that named it.
#[test]
fn erasing_the_history_frees_its_generations_at_once() {
    let mut small = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::Lent);
    let mut t = small.term(&SMALL);
    feed(&mut t, &flood(0, 200, 40));
    let before = t.frozen();
    assert!(
        before.words_held > 0 && before.gens_alive > 0,
        "the control: {before:?}"
    );
    feed(&mut t, b"\x1b[3J");
    let after = t.frozen();
    assert_eq!((after.words_held, after.gens_alive), (0, 0), "{after:?}");
}

/// **`RIS` frees every generation at once**, the alternate screen's too: it
/// erases the ring and the alternate screen.
#[test]
fn a_reset_frees_every_generation_at_once() {
    let mut small = Lent::new(&SMALL, Depth::of(100), NSTYLE, Pool::Lent);
    let mut t = small.term(&SMALL);
    feed(&mut t, b"\x1b[?47h");
    feed(&mut t, &flood(20_000, 9, 40));
    feed(&mut t, b"\x1b[?47l");
    feed(&mut t, &flood(0, 100, 40));
    let before = t.frozen();
    assert!(before.gens_alive > 0, "the control: {before:?}");
    feed(&mut t, b"\x1bc");
    let after = t.frozen();
    assert_eq!((after.words_held, after.gens_alive), (0, 0), "{after:?}");
}

/// **A full table while the status line is written freezes the main
/// display's history**, read through the display parked while the line is
/// written, and the line's own row stays live.
#[test]
fn a_full_table_while_the_status_line_is_written_freezes_the_main_display() {
    let mut on_status = flood(0, 120, 40);
    // DECSSDT 2, the host-writable line; DECSASD 1, written.
    on_status.extend(b"\x1b[2$~\x1b[1$}");
    for i in 0..3_000 {
        on_status.extend(rendition(50_000 + i).as_bytes());
        on_status.extend(b"S\x08");
    }
    on_status.extend(b"\x1b[0$}");
    on_status.extend(flood(60_000, 40, 40));
    let c = differential(&SMALL, Depth::of(200), &[Step::Bytes(on_status)]).past_the_table();
    assert!(c.freezes > 0, "the control: {c:?}");
}

/// **What a table with no slot left does: it refuses, and no drawn cell
/// changes.** A storage lent no generations, a history of more renditions
/// than the table has slots: the renditions past it are refused, counted,
/// and every cell already drawn reads as it did when it was drawn.
#[test]
fn a_storage_lent_no_generations_refuses_and_recolours_nothing() {
    let mut lent = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::None);
    let mut t = lent.term(&SMALL);
    feed(&mut t, &flood(0, 20, 40));
    // The history so far, which the next lines leave the oldest.
    let kept = t.scrollback().len();
    let drawn = read(&t);
    feed(&mut t, &flood(800, 40, 40));
    let r = t.style_reclaims();
    assert!(r.refusals > 0, "the control: it refused: {r:?}");
    assert_eq!(t.frozen(), baud_domain::FrozenReport::default());
    let now = read(&t);
    assert!(kept > 5, "the control: the history held lines");
    assert_eq!(&now[..kept], &drawn[..kept], "a drawn cell changed");
}

/// **A screen past the table's bound refuses what it cannot hold, and no
/// drawn cell changes.** 400 by 170 at its pitch is more cells than 65,536
/// slots less the status row and the three reserved, so one screen of
/// renditions overruns the table even with every line off it frozen.
#[test]
fn a_screen_past_the_bound_refuses_and_recolours_nothing() {
    let ws = WinSize {
        ws_row: 170,
        ws_col: 400,
    };
    support::with_term(&ws, |t| {
        assert_eq!(t.styles().len(), NSTYLE_MAX);
        let mut first = Vec::new();
        for i in 0..40_000 {
            first.extend(rendition(i).as_bytes());
            first.push(b'p');
        }
        feed(t, &first);
        let drawn: Vec<_> = (0..100)
            .map(|y| {
                t.row(Row::from_index(y))
                    .expect("a row")
                    .iter()
                    .map(|c| t.style_of(*c))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut more = Vec::new();
        for i in 0..28_000 {
            more.extend(rendition(40_000 + i).as_bytes());
            more.push(b'q');
        }
        feed(t, &more);
        assert!(t.style_reclaims().refusals > 0, "the control: it refused");
        for (y, row) in drawn.iter().enumerate() {
            let now: Vec<_> = t
                .row(Row::from_index(y))
                .expect("a row")
                .iter()
                .map(|c| t.style_of(*c))
                .collect();
            assert_eq!(&now, row, "row {y} changed");
        }
    });
}

/// **`Term::line_styles` reads an absolute line through its table**, the
/// same as the scrollback reads it, and a line the ring no longer holds as
/// live.
#[test]
fn an_absolute_line_reads_through_its_generation() {
    let mut small = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::Lent);
    let mut t = small.term(&SMALL);
    feed(&mut t, &flood(0, 200, 40));
    let sb = t.scrollback();
    let mut frozen = 0;
    for back in 0..sb.len() {
        let line = t
            .search_lines()
            .top
            .minus(Rows::of_count(back + 1))
            .expect("a line");
        let (a, b) = (sb.styles(Back::of(back)), t.line_styles(line));
        assert_eq!(a.is_live(), b.is_live(), "line {back} back");
        if let (baud_domain::RowStyles::Frozen(x), baud_domain::RowStyles::Frozen(y)) = (a, b) {
            assert_eq!(x.serial(), y.serial());
            frozen += 1;
        }
    }
    assert!(frozen > 0, "the control: some line is frozen");
}

/// **The prompt's retreat takes frozen rows back and erases them**, and the
/// rows it takes back read their new renditions when they retire again --
/// with no freeze between, so only their cleared tags keep the generation
/// they were frozen into from claiming them.
#[test]
fn rows_the_prompts_retreat_takes_back_read_their_new_renditions() {
    let mut b = flood(0, 100, 40);
    // A prompt block of fourteen lines, five of them pushed into the history.
    b.extend(b"\x1b]133;A\x07");
    b.extend(flood(10_000, 14, 40));
    // The shell clears below the input, and redraws from the block's start.
    b.extend(b"\x1b[6;1H\x1b[J\x1b]133;A\x07");
    b.extend(rendition(40_000).as_bytes());
    b.extend(b"redrawn prompt\r\nand\r\nmore\x1b[0m");
    b.extend(b"\x1b[10;1H");
    b.extend([b'\n'; 12]);
    let c = differential(&SMALL, Depth::of(200), &[Step::Bytes(b)]).past_the_table();
    assert!(c.freezes > 0, "the control: {c:?}");
}

/// **`?1049` clears an alternate screen that was frozen, and it names no
/// generation after**: written while up, left, and shown again by `?47`, it
/// reads what was written while it was up, through the table, and a sweep
/// on the primary between keeps its slots.
#[test]
fn an_alternate_screen_cleared_by_1049_names_no_generation() {
    let mut b = b"\x1b[?47h".to_vec();
    b.extend(flood(20_000, 9, 40));
    b.extend(b"\x1b[?47l");
    b.extend(flood(0, 100, 40));
    b.extend(b"\x1b[?1049h");
    b.extend(flood(30_000, 9, 40));
    b.extend(b"\x1b[?1049l");
    b.extend(flood(40_000, 100, 40));
    let c = differential(&SMALL, Depth::of(100), &[Step::Bytes(b)]).past_the_table();
    assert!(c.freezes > 1, "the control: {c:?}");
}

/// **Where the reads fall changes nothing**: S1, S2 and S3b fed at reads of
/// 1, 3 and 4096 bytes and split in two at a third of the stream read the
/// same cells in the same renditions as one read each, through every
/// table, and freeze the same rows.
#[test]
fn the_generations_do_not_depend_on_where_the_reads_fell() {
    let s2 = {
        let mut b = flood(0, 200, 40);
        b.extend(b"\x1b[?1049h");
        b.extend(flood(9_000, 149, 40));
        b.extend(b"\x1b[?1049l");
        b
    };
    let s3b: Vec<u8> = take_back(Distinct::One)
        .into_iter()
        .flat_map(|s| match s {
            Step::Bytes(b) => b,
            Step::Resize(..) => Vec::new(),
        })
        .collect();
    for (name, bytes) in [("S1", flood(0, 400, 40)), ("S2", s2), ("S3b", s3b)] {
        let whole = {
            let mut l = Lent::new(&SMALL, Depth::of(400), NSTYLE, Pool::Lent);
            let mut t = l.term(&SMALL);
            feed_by(&mut t, &bytes, bytes.len());
            (read(&t), t.frozen().counts.rows_frozen)
        };
        assert!(whole.1 > 0, "{name}: the control: it froze");
        for read_size in [1usize, 3, 4096] {
            let mut l = Lent::new(&SMALL, Depth::of(400), NSTYLE, Pool::Lent);
            let mut t = l.term(&SMALL);
            feed_by(&mut t, &bytes, read_size);
            assert!(read(&t) == whole.0, "{name}: reads of {read_size}");
        }
        let mut l = Lent::new(&SMALL, Depth::of(400), NSTYLE, Pool::Lent);
        let mut t = l.term(&SMALL);
        let (a, b) = bytes.split_at(bytes.len() / 3);
        feed_by(&mut t, a, a.len());
        feed_by(&mut t, b, b.len());
        assert!(read(&t) == whole.0, "{name}: two reads");
    }
}

/// **A string a frozen row's cells are written in is kept by the string
/// table's sweep**: the row names it through its generation's words, which
/// the table's slots no longer hold. Freed, the next string would take its
/// entry and the frozen cells would read as written in that one.
#[test]
fn a_frozen_rows_string_survives_the_string_tables_sweep() {
    use baud_domain::{StringEntry, StringId, StringKind};
    let mut l = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::Lent);
    let mut t = l.term(&SMALL);
    let mut b = rendition(60_000).into_bytes();
    b.extend(b"\x1b[1[reversed\x1b[0[\x1b[0m\r\n");
    b.extend(flood(0, 120, 40));
    feed(&mut t, &b);
    let sb = t.scrollback();
    let oldest = Back::of(sb.len() - 1);
    let (cells, _) = sb.line(oldest).expect("the first line");
    let styles = sb.styles(oldest);
    assert!(!styles.is_live(), "the control: the line is frozen");
    let string_of = |t: &Term<'_>| -> StringId {
        let sb = t.scrollback();
        let (cells, _) = sb.line(oldest).expect("the first line");
        sb.styles(oldest).word(t.styles(), cells[0].style).string()
    };
    let _ = (cells, styles);
    let s = string_of(&t);
    assert!(matches!(
        t.string_entry(s),
        Some(StringEntry {
            kind: StringKind::Reversed,
            ..
        })
    ));
    // Three hundred directed strings, each over the last: the table sweeps.
    let refused = t.strings_refused();
    for _ in 0..300 {
        feed(&mut t, b"\x1b[10;1H\x1b[2]a\x1b[0]");
    }
    assert_eq!(t.strings_refused(), refused, "the control: none refused");
    assert!(
        matches!(
            t.string_entry(string_of(&t)),
            Some(StringEntry {
                kind: StringKind::Reversed,
                ..
            })
        ),
        "the frozen row's string was freed and taken"
    );
}

/// **A kitty placeholder in a frozen row reads its image from its own
/// colours**: the ids ride in the foreground and the underline colour, which
/// a frozen row names through its generation's words.
#[test]
fn a_placeholder_in_a_frozen_row_reads_its_ids_through_its_generation() {
    let mut l = Lent::new(&SMALL, Depth::of(200), NSTYLE, Pool::Lent);
    let mut t = l.term(&SMALL);
    // U+10EEEE in image 7's foreground and placement 3's underline colour.
    let mut b = b"\x1b[0;38;5;7;58;5;3m\xf4\x8e\xbb\xae\x1b[0m\r\n".to_vec();
    b.extend(flood(0, 120, 40));
    feed(&mut t, &b);
    let sb = t.scrollback();
    let oldest = Back::of(sb.len() - 1);
    let (cells, _) = sb.line(oldest).expect("the first line");
    let styles = sb.styles(oldest);
    assert!(!styles.is_live(), "the control: the line is frozen");
    let ids = |s: baud_domain::RowStyles<'_>| {
        t.placeholder_runs(&cells[..1], s)
            .map(|r| (r.image, r.placement))
            .collect::<Vec<_>>()
    };
    let through = ids(styles);
    assert_eq!(through.len(), 1, "one run");
    assert_ne!(
        through,
        ids(baud_domain::RowStyles::Live),
        "the control: the live table reads another image"
    );
    let w = t.style_in(styles, cells[0]);
    assert_eq!(w.fg(), ColorWord::indexed(baud_domain::Chan::from_u8(7)));
}

/// **A screen that comes back fills the table to its bound, and nothing is
/// refused**: a 24-row screen of renditions of their own is most of a
/// 1,024-slot table, so the alternate screen's, frozen off it, and the
/// primary's coming back cannot both hold slots. A sweep that read the
/// incoming rows' frozen ids as slots kept as many of the outgoing screen's
/// as share their numbers, and the thaw ran out of room.
#[test]
fn a_screen_coming_back_fills_the_table_without_refusing() {
    let tall = WinSize {
        ws_row: 24,
        ws_col: 40,
    };
    let mut b = flood(0, 24, 40);
    b.extend(b"\x1b[?1049h");
    b.extend(flood(5_000, 23, 40));
    b.extend(b"\x1b[?1049l");
    // The control: the two screens name 800 renditions each, more than the
    // table's slots between them.
    let c = differential(&tall, Depth::of(50), &[Step::Bytes(b)]).counts;
    assert!(
        c.rows_thawed >= 23,
        "the control: the screen came back: {c:?}"
    );
    // Renditions churned over one cell after the flood, so the table passes
    // its load and the newest history lines are frozen before the growth
    // pulls them back.
    let mut churn = b"\x1b[1;1H".to_vec();
    for i in 0..700 {
        churn.extend(rendition(20_000 + i).as_bytes());
        churn.extend(b"X\x08");
    }
    let steps = [
        Step::Resize(40, 10),
        Step::Bytes(flood(0, 60, 40)),
        Step::Bytes(flood(9_000, 10, 40)),
        Step::Bytes(churn),
        Step::Resize(40, 24),
    ];
    let c = differential(&tall, Depth::of(100), &steps).past_the_table();
    assert!(c.rows_thawed >= 9, "the control: rows came back: {c:?}");
}
