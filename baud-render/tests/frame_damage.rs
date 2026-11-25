//! **A frame is built on its damage** (`Renderer::damage`): what the
//! terminal changed since the last frame built, by either path -- every
//! pixel for a reason the damage names, else the rows written -- and the
//! rows the renderer's own animations moved with no cell written
//! (`Renderer::animated`). One row written takes one row from the
//! terminal on both paths, and an animation over a still screen is drawn
//! where it is and where it was.

use baud_core::{NCLUSTER, Owned, Term};
use baud_domain::{
    BytePos, ClusterSlot, ClusterStore, Codepoint, Cols, Content, CursorChange, DirtyRows,
    FrameDamage, OverlayChanges, PackedCell, ReadLen, Row, RowExtent, Rows, WholeWhy, WinSize,
};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build_with};
use baud_render::counters::{self, Counter, Counts, Site};
use baud_render::fireworks::{BAUD, Form, HOLD, ORIGINS, Show};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Blend, Blink, BlinkPhases, Candidate, Damage, GlyphMap, Instance, Leaving, Lent, MapEntry,
    Overlay, Pt, PxSize, Renderer, Resolved, Stack, StyleStamp, Target, Theme,
};

/// A screen whose rendition table is its smallest, 1024 slots.
const SMALL: WinSize = WinSize {
    ws_col: 40,
    ws_row: 8,
};

/// The fireworks' screen, wide enough for a show's reach.
const WIDE: WinSize = WinSize {
    ws_col: 80,
    ws_row: 24,
};

/// The cursor hidden, so no frame here draws a row again for it.
const HIDE: &[u8] = b"\x1b[?25l";

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
    }
}

fn font() -> Vec<u8> {
    std::fs::read(format!(
        "{}/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn renderer<'f>(faces: &'f [Face<'f>], ws: WinSize) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap()
}

/// **One CPU frame into `px`**, which holds the last, sealed first as a
/// driver seals, and what it counted.
fn cpu(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut Vec<u32>, damage: Damage) -> Counts {
    r.fit(t);
    let (w, h) = r.pixel_size();
    px.resize(usize::try_from(w.value() * h.value()).unwrap(), 0);
    let _ = counters::take();
    let mut target = Target::new(&mut px[..], w, w, h).unwrap();
    let seal = t.seal_writes();
    r.render(t, seal, &mut target, damage);
    counters::take()
}

/// The cell path's buffers, one set a frame in flight, as the app keeps
/// them.
struct Cells<'m> {
    map: GlyphMap<'m>,
    exceptions: Overlay,
    screens: Screens,
    copies_mem: [Vec<PackedCell>; SETS],
    styles_mem: [Vec<Resolved>; SETS],
    overlay_mem: [Vec<u8>; SETS],
    style_stamps: [StyleStamp; SETS],
    lent: [Lent; SETS],
    candidates: Vec<Candidate>,
    records: Vec<Instance>,
    grounds: Vec<Instance>,
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
    next: SetIx,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let (stride, (cols, rows)) = (t.stride().index(), t.geometry());
        let (cols, rows) = (cols.index(), rows.index());
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().one().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; cols * (rows + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
            next: SetIx(0),
        }
    }
}

/// **One cell frame into the next set in turn, as the app's `present`
/// builds it**: the frame's writes sealed first, its damage read, and the
/// frame built with no composition, prediction or slide. What it counted.
fn cell(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, cell: &mut Cells<'_>) -> Counts {
    let seal = t.seal_writes();
    let t = &*t;
    let back = t.window_back();
    r.fit(t);
    r.set_images(t, back);
    r.splash_yields_to(t, Px::of(0));
    r.place_marks(t, back, Leaving::None, Px::of(0));
    r.place_lights(t);
    let live = t
        .cursor_visible()
        .then(|| t.cursor())
        .map(|(row, col)| (row, col, t.cursor_style()));
    let inputs = CellInputs {
        back,
        predictions: core::iter::empty(),
        live,
        composition: &None,
        highlight: None,
        leaving: Leaving::None,
        pan: Px::of(0),
    };
    let at = cell.next;
    cell.next = SetIx((at.0 + 1) % SETS);
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    let damage = r.damage(t, back);
    let _ = counters::take();
    build_with(
        r,
        t,
        seal,
        &damage,
        &inputs,
        &mut CellParts {
            map: &mut cell.map,
            exceptions: &mut cell.exceptions,
            screens: &mut cell.screens,
            copies_mem: &mut copies,
            styles_mem: &mut styles,
            overlay_mem: &mut overlay,
            style_stamps: &mut cell.style_stamps,
            lent: &mut cell.lent,
            candidates: &mut cell.candidates,
            records: &mut cell.records,
            grounds: &mut cell.grounds,
            lent_cells: &mut cell.lent_cells,
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        || Some(at),
    )
    .expect("the set is free");
    counters::take()
}

/// The rows of `set`, top to bottom.
fn listed(set: DirtyRows) -> Vec<usize> {
    set.iter().map(Row::index).collect()
}

/// **The damage a CPU frame reads after `change`**, on a [`SMALL`]
/// screen written with `setup`, drawn whole and then once more, nothing
/// written between.
fn damage_after(setup: &[u8], change: impl FnOnce(&mut Term<'_>)) -> FrameDamage {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, SMALL);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    feed(&mut t, HIDE);
    feed(&mut t, setup);
    let mut px = Vec::new();
    cpu(&mut r, &mut t, &mut px, Damage::Full);
    cpu(&mut r, &mut t, &mut px, Damage::Incremental);
    change(&mut t);
    r.damage(&t, Rows::ZERO)
}

/// `n` distinct renditions through the one cell at row 5, column 1.
fn churn(n: usize) -> Vec<u8> {
    let mut out = b"\x1b[5;1H".to_vec();
    for i in 0..n {
        out.extend(format!("\x1b[0;38;2;9;{};{}mC\x08", (i >> 8) & 255, i & 255).as_bytes());
    }
    out.extend(b"\x1b[0m\x1b[H");
    out
}

/// **A cluster written at one cell for every slot of the cluster store,
/// then one more**: each written over the last, so none is named once
/// the next is written, and the store, every slot used, sweeps the dead
/// at the last. A slot is reached by a letter and a mark found to start
/// there, read from an empty store of the terminal's size, which is the
/// store the terminal interns into.
fn sweep_clusters() -> Vec<u8> {
    let bases = ('a'..='z')
        .chain('A'..='Z')
        .chain('\u{3b1}'..='\u{3c9}')
        .chain('\u{430}'..='\u{44f}');
    let marks = (0x0300..=0x036F).filter_map(char::from_u32);
    let mut by: Vec<Option<(char, char)>> = vec![None; NCLUSTER];
    let mut spare = None;
    for (b, m) in bases.flat_map(|b| marks.clone().map(move |m| (b, m))) {
        let mut slots = vec![ClusterSlot::EMPTY; NCLUSTER];
        let mut store = ClusterStore::new(&mut slots);
        let cp = Codepoint::new(u32::from(b)).expect("a codepoint");
        let at = store
            .intern(Content::codepoint(cp), &[u32::from(m)])
            .index()
            .checked_sub(1);
        match at.and_then(|k| by.get_mut(k)) {
            Some(slot @ None) => *slot = Some((b, m)),
            _ => spare = Some((b, m)),
        }
    }
    let mut out = Vec::new();
    for (b, m) in by.into_iter().flatten().chain(spare) {
        out.extend(format!("\x1b[6;1H{b}{m}").as_bytes());
    }
    out.extend(b"\x1b[2K\x1b[H");
    out
}

/// **The damage names what changed**: nothing written is no row, the
/// cursor where it was and no overlay; one cell written on row 3 is row
/// 3 alone; a resize, a switch to the alternate screen, a palette entry
/// set, a sweep of the cluster store and a sweep of the rendition table
/// each make the frame whole for their own reason. The controls: each
/// change moved what it is named for.
#[test]
fn damage_names_what_changed() {
    // No frame drawn yet: the first frame is drawn whole, asked for.
    {
        let data = font();
        let faces = [Face::parse(&data).unwrap()];
        let r = renderer(&faces, SMALL);
        let mut owned = Owned::new(&SMALL);
        let t = Term::new(owned.storage(), &SMALL);
        assert_eq!(
            r.damage(&t, Rows::ZERO),
            FrameDamage::Whole(WholeWhy::Requested)
        );
    }
    let still = damage_after(b"some text", |_| {});
    assert_eq!(
        still,
        FrameDamage::Rows {
            rows: DirtyRows::NONE,
            cursor: CursorChange::Same,
            overlays: OverlayChanges::NONE,
        },
        "nothing written"
    );
    let one = damage_after(b"some text", |t| feed(t, b"\x1b[4;5HX\x1b[1;10H"));
    match one {
        FrameDamage::Rows { rows, .. } => assert_eq!(listed(rows), [3], "row 3 alone"),
        other => panic!("one row written: {other:?}"),
    }
    assert_eq!(
        damage_after(b"some text", |t| {
            let _ = t.resize(&WinSize {
                ws_col: 30,
                ws_row: 8,
            });
        }),
        FrameDamage::Whole(WholeWhy::Resized)
    );
    assert_eq!(
        damage_after(b"some text", |t| feed(t, b"\x1b[?1049h")),
        FrameDamage::Whole(WholeWhy::Screen)
    );
    assert_eq!(
        damage_after(b"some text", |t| feed(t, b"\x1b]4;1;rgb:ff/00/00\x1b\\")),
        FrameDamage::Whole(WholeWhy::Colours)
    );
    assert_eq!(
        damage_after(b"some text", |t| {
            let (epoch, resets) = (t.cluster_epoch(), t.resets());
            feed(t, &sweep_clusters());
            assert_ne!(t.cluster_epoch(), epoch, "the control: the store swept");
            assert_eq!(t.resets(), resets, "the control: no reset");
        }),
        FrameDamage::Whole(WholeWhy::Clusters)
    );
    assert_eq!(
        damage_after(b"some text", |t| {
            assert_eq!(t.styles().one().len(), 1024, "the control: the table");
            let was = t.style_reclaims();
            feed(t, &churn(1100));
            assert!(
                t.style_reclaims().recolours_since(was),
                "the control: the table swept: {:?}",
                t.style_reclaims()
            );
        }),
        FrameDamage::Whole(WholeWhy::Renditions)
    );
}

/// **One row written takes one row from the terminal on both paths**:
/// on the CPU path the frame after one cell written on row 3 takes row 3
/// alone into its copy and converts its 40 cells; on the cell path the
/// frame after it, into a set that held the screen before the write,
/// takes row 3 alone into the set and copies its 40 cells. The controls:
/// the frame before the write takes none on either path.
#[test]
fn one_row_written_converts_one_row() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let cols = u64::from(SMALL.ws_col);
    let write = b"\x1b[4;5HX\x1b[1;10H";

    let mut r = renderer(&faces, SMALL);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    feed(&mut t, HIDE);
    feed(&mut t, b"some text\r\nmore text");
    let mut px = Vec::new();
    cpu(&mut r, &mut t, &mut px, Damage::Full);
    let before = cpu(&mut r, &mut t, &mut px, Damage::Incremental);
    feed(&mut t, write);
    let after = cpu(&mut r, &mut t, &mut px, Damage::Incremental);
    let at = |c: &Counts, site, counter| c.at(site, counter).value();
    assert_eq!(
        at(&before, Site::Convert, Counter::RowsTaken),
        0,
        "cpu: none before"
    );
    assert_eq!(
        at(&after, Site::Convert, Counter::RowsTaken),
        1,
        "cpu: row 3 taken"
    );
    assert_eq!(
        at(&after, Site::Convert, Counter::CellsConverted),
        cols,
        "cpu: row 3 converted"
    );

    let mut r = renderer(&faces, SMALL);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    feed(&mut t, HIDE);
    feed(&mut t, b"some text\r\nmore text");
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    for _ in 0..=SETS {
        cell(&mut r, &mut t, &mut cells);
    }
    let before = cell(&mut r, &mut t, &mut cells);
    feed(&mut t, write);
    let after = cell(&mut r, &mut t, &mut cells);
    assert_eq!(
        at(&before, Site::Screens, Counter::RowsTaken),
        0,
        "cell: none before"
    );
    assert_eq!(
        at(&after, Site::Screens, Counter::RowsTaken),
        1,
        "cell: row 3 taken"
    );
    assert_eq!(
        at(&after, Site::Screens, Counter::CellsCopied),
        cols,
        "cell: row 3 copied"
    );
}

/// **The pairs a whole walk of the glyph map lists over `t`'s window**,
/// through a map of its own, in the rendition table the last cell frame
/// drew with.
fn whole_walk(r: &mut Renderer<'_, '_>, t: &Term<'_>, cells: &Cells<'_>) -> Vec<Candidate> {
    let last = (cells.next.0 + SETS - 1) % SETS;
    let mut backing = GlyphMap::backing();
    let mut map = GlyphMap::over(&mut backing);
    let mut found = Vec::new();
    map.ensure(
        t,
        t.view(t.window_back()),
        &cells.styles_mem[last],
        r,
        &mut found,
    );
    found
}

/// **One row written walks one row of the glyph map** on the cell path,
/// and the pairs the walk keeps are a whole walk's: rows holding pairs
/// above and below it keep theirs, a pair written in is listed in its
/// row's place, and a pair written over is gone. A frame with nothing
/// written walks none.
#[test]
fn one_row_written_walks_one_row() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces, SMALL);
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    feed(&mut t, HIDE);
    feed(&mut t, b"a -> b\r\n\r\nc != d\r\n\r\n\r\ne <= f");
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    for _ in 0..=SETS {
        cell(&mut r, &mut t, &mut cells);
    }
    let walked = |cells: &Cells<'_>| cells.map.stats().rows_walked.value();
    let rows_of = |c: &[Candidate]| {
        let mut rows: Vec<usize> = c.iter().map(|c| c.row.index()).collect();
        rows.dedup();
        rows
    };
    assert_eq!(
        rows_of(&cells.candidates),
        [0, 2, 5],
        "a pair on each written row"
    );
    let was = walked(&cells);
    cell(&mut r, &mut t, &mut cells);
    assert_eq!(walked(&cells) - was, 0, "nothing written walks no row");
    for (write, rows) in [
        (&b"\x1b[4;3H=>"[..], &[0, 2, 3, 5][..]),
        (b"\x1b[3;3Hab", &[0, 3, 5]),
        (b"\x1b[2;1Hx -- y", &[0, 1, 3, 5]),
    ] {
        feed(&mut t, write);
        let was = walked(&cells);
        cell(&mut r, &mut t, &mut cells);
        assert_eq!(
            walked(&cells) - was,
            1,
            "{}: one row walked",
            String::from_utf8_lossy(write)
        );
        assert_eq!(
            rows_of(&cells.candidates),
            rows,
            "{}: the rows holding pairs",
            String::from_utf8_lossy(write)
        );
        assert_eq!(
            cells.candidates,
            whole_walk(&mut r, &t, &cells),
            "{}: a whole walk's pairs",
            String::from_utf8_lossy(write)
        );
    }
}

/// The show `age` seconds in, every burst and the hole at `at`.
fn show(age: f32, at: Pt) -> Show {
    Show {
        age,
        hold: HOLD,
        origins: [at; ORIGINS],
        hole: at,
        scale: 1.0,
        seed: baud_render::effect::Seed(41),
        look: BAUD,
        form: Form::Moving,
    }
}

/// **The rows the effect's records are on**, by the cell height.
fn footprint(r: &Renderer<'_, '_>) -> DirtyRows {
    let h = i64::from(r.metrics().cell_h.value());
    let rows = i64::try_from(r.grid().1.index()).unwrap();
    let mut set = DirtyRows::NONE;
    for q in r.trail() {
        let (top, bottom) = (i64::from(q.y.0), i64::from(q.y.0) + i64::from(q.h.0));
        if bottom <= top {
            continue;
        }
        for y in (top.max(0) / h)..((bottom + h - 1) / h).min(rows) {
            set = set
                .with(Row::from_index(usize::try_from(y).unwrap()))
                .unwrap();
        }
    }
    set
}

/// Whether every row of `part` is in `whole`.
fn within(part: DirtyRows, whole: DirtyRows) -> bool {
    part.union(whole) == whole
}

/// **An effect over a still screen is drawn where it is and where it
/// was, on both paths**: fireworks burning over a screen nothing writes,
/// each frame's animated rows hold the rows its stars are on and the rows
/// they were on the frame before, and the frame after the show ends holds
/// the rows the last stars were on. On the CPU path each frame drawn on
/// the last equals the same frame drawn whole. The controls: the stars
/// are on some row, and the screen's damage names no row throughout.
#[test]
fn an_effect_over_a_still_screen_is_drawn_where_it_is_and_where_it_was() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WIDE);
    let mut t = Term::new(owned.storage(), &WIDE);
    feed(&mut t, HIDE);
    feed(&mut t, b"\x1b[2;1Hsome text on a row\x1b[12;40H");
    let mut r = renderer(&faces, WIDE);
    let mut whole = renderer(&faces, WIDE);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut c = renderer(&faces, WIDE);
    let m = r.metrics();
    let px_of = |v: u32| f32::from(u16::try_from(v).unwrap());
    let at = Pt {
        x: 39.5 * px_of(m.cell_w.value()),
        y: 11.5 * px_of(m.cell_h.value()),
    };
    let (mut px, mut full) = (Vec::new(), Vec::new());
    cpu(&mut r, &mut t, &mut px, Damage::Full);
    cell(&mut c, &mut t, &mut cells);
    let still =
        |d: FrameDamage| matches!(d, FrameDamage::Rows { rows, .. } if rows == DirtyRows::NONE);
    let end = HOLD + baud_render::fireworks::FADE;
    let mut was = (DirtyRows::NONE, DirtyRows::NONE);
    let mut age = 1.0 / 6.0;
    let mut lit = 0;
    while age < end + 0.1 {
        let s = [show(age.min(end), at)];
        let shows: &[Show] = if age < end + 0.05 { &s } else { &[] };
        for x in [&mut r, &mut whole, &mut c] {
            x.set_fireworks(shows);
        }
        assert!(
            still(r.damage(&t, Rows::ZERO)),
            "the control: nothing written"
        );
        cpu(&mut r, &mut t, &mut px, Damage::Incremental);
        cpu(&mut whole, &mut t, &mut full, Damage::Full);
        cell(&mut c, &mut t, &mut cells);
        let now = (footprint(&r), footprint(&c));
        lit += usize::from(now.0 != DirtyRows::NONE);
        let cpu_rows = now.0.union(was.0);
        let cell_rows = now.1.union(was.1);
        assert!(
            within(cpu_rows, r.animated().grid),
            "cpu at {age}: {:?} not in {:?}",
            listed(cpu_rows),
            listed(r.animated().grid)
        );
        assert!(
            within(cell_rows, c.animated().grid),
            "cell at {age}: {:?} not in {:?}",
            listed(cell_rows),
            listed(c.animated().grid)
        );
        assert!(px == full, "cpu at {age}: the frame is the whole frame's");
        was = now;
        age += 1.0 / 10.0;
    }
    assert!(lit > 3, "the control: the stars were drawn on {lit} frames");
    assert_eq!(was, (DirtyRows::NONE, DirtyRows::NONE), "the show ended");
}

/// **Text whose blink phase flips is drawn again where it is, on both
/// paths**: blinking text on row 2 of a still screen, its slow phase
/// flipped with nothing written, names row 2 among the animated rows --
/// row 2 alone on the CPU path, which reads blink a row at a time -- and
/// the CPU frame drawn on the last equals the same frame drawn whole. The
/// controls: the screen's damage names no row, and the flip changes what
/// the whole frame draws.
#[test]
fn text_whose_blink_flips_is_drawn_again_where_it_is() {
    let data = font();
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&SMALL);
    let mut t = Term::new(owned.storage(), &SMALL);
    feed(&mut t, HIDE);
    feed(&mut t, b"plain\x1b[3;1H\x1b[5mblinking\x1b[0m\x1b[H");
    let mut r = renderer(&faces, SMALL);
    let mut whole = renderer(&faces, SMALL);
    let mut c = renderer(&faces, SMALL);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let (mut px, mut full) = (Vec::new(), Vec::new());
    cpu(&mut r, &mut t, &mut px, Damage::Full);
    cpu(&mut whole, &mut t, &mut full, Damage::Full);
    let shown = full.clone();
    for _ in 0..=SETS {
        cell(&mut c, &mut t, &mut cells);
    }
    let hidden = BlinkPhases {
        slow: Blink::Hidden,
        rapid: Blink::Shown,
    };
    for x in [&mut r, &mut whole, &mut c] {
        x.set_blink(hidden);
    }
    assert!(
        matches!(r.damage(&t, Rows::ZERO), FrameDamage::Rows { rows, .. } if rows == DirtyRows::NONE),
        "the control: nothing written"
    );
    cpu(&mut r, &mut t, &mut px, Damage::Incremental);
    cpu(&mut whole, &mut t, &mut full, Damage::Full);
    cell(&mut c, &mut t, &mut cells);
    assert!(full != shown, "the control: the flip hides the text");
    assert_eq!(listed(r.animated().grid), [2], "cpu: row 2 alone");
    assert!(px == full, "cpu: the frame is the whole frame's");
    assert!(
        c.animated().grid.contains(Row::from_index(2)),
        "cell: row 2 among {:?}",
        listed(c.animated().grid)
    );
}
