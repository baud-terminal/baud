//! **A caret frame over a struck link allocates what the same frame with
//! no link struck allocates, and a caret frame over a window the core froze
//! what the same screen's live window's does** (FS-C14), on the CPU path,
//! which the Linux driver draws with, and on the cell path, built into the
//! app's sets in turn. The struck cells are kept while nothing they are
//! found from moved, and the lending of a frame's table to its frozen and
//! struck cells runs on buffers it keeps, so a caret frame -- the cursor
//! moved, nothing written -- allocates nothing for either.
//!
//! Allocations are counted on the thread that makes them and in the
//! process as a whole, and **every frame counted is the process's only
//! work**: each test runs in a process of its own, this binary run again
//! for that test alone on one thread ([`isolated`]), and a counted frame
//! fails unless every allocation the process made in it was this
//! thread's ([`heap::take`]) -- so a frame whose work moved to another
//! thread would be seen, not counted as nothing. Every frame is drawn on
//! that thread, the renderer's effects held to one
//! ([`Renderer::set_threads`]). The guard's control: a thread spawned in a
//! counted span is seen allocating there ([`guard_control`]). Each arm's
//! control says the counter sees the frames it counts -- the first, full,
//! frame allocates -- and that the lending it prices happened, on its own
//! path: on the cell path the struck link is lent a slot, and none is
//! short, and the frozen window's cells are lent theirs; on the CPU path
//! the struck cells are found and the struck text drawn in the lent
//! colour. Each test is declared through [`alone`], so the name it is run
//! again under is the one it is declared with.
//!
//! **A strike on a row drawn in another order is found once**, and its
//! caret frames find it no more, on both paths: the struck cells are kept
//! while the layout laid out no row otherwise since they were found
//! ([`Renderer::struck_finds`]). **The struck cells' generation moves when
//! they come out otherwise, and only then** ([`Renderer::struck_gen`]); it
//! reads true while a lending has the cells out, a renderer made again
//! starts at one no reader of another holds, and cells lost -- taken and
//! never put back -- move it.
//!
//! **A frozen window with a display held is measured, not held to a
//! figure**: its caret frames' allocations are printed beside the same
//! window drawn as the terminal cut it, which holds no display (FS-C14
//! 3a.1 owes it the frozen window's figure).

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Col, Cols, PackedCell, ReadLen, Row, RowExtent, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build_with};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Argb, Blend, Candidate, CopyOrder, Damage, GlyphMap, HoverPaint, Instance, Leaving, Lent,
    LineStyle, LinkHover, LinkLine, LinkSpan, MapEntry, MissingPaint, Overlay, PxSize, Renderer,
    Resolved, Stack, StyleStamp, Tally, Target, Theme, WordWrap,
};
use core::ops::Range;
use support::font;

#[global_allocator]
static HEAP: heap::Counting = heap::Counting;

/// **Every allocation, counted on the thread that made it and in the
/// process.** The system's allocator does the work; this counts each
/// `alloc`, and `alloc_zeroed` and `realloc`, which reach it through the
/// trait's own defaults. An atomic and `const` thread-locals with nothing
/// to drop, so counting allocates nothing.
mod heap {
    use baud_render::Tally;
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub struct Counting;

    /// The allocations every thread of the process made.
    static PROCESS: AtomicU64 = AtomicU64::new(0);

    std::thread_local! {
        static THIS_THREAD: Cell<u64> = const { Cell::new(0) };
        /// [`PROCESS`] when this thread's span started.
        static SPAN_FROM: Cell<u64> = const { Cell::new(0) };
    }

    // SAFETY: every call is forwarded to the system allocator with the
    // layout and pointer it was given; the counts are an atomic add and a
    // plain store.
    #[allow(unsafe_code)]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            PROCESS.fetch_add(1, Ordering::Relaxed);
            let _ = THIS_THREAD.try_with(|c| c.set(c.get().wrapping_add(1)));
            // SAFETY: the caller's contract, passed on.
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: the caller's contract, passed on.
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    fn tally(n: u64) -> Tally {
        Tally::of(usize::try_from(n).unwrap_or(usize::MAX))
    }

    /// A span counted from here: this thread's count and the process's
    /// started again.
    pub fn start() {
        THIS_THREAD.with(|c| c.set(0));
        SPAN_FROM.with(|c| c.set(PROCESS.load(Ordering::Relaxed)));
    }

    /// The allocations this thread made since [`start`], and the process.
    pub fn counts() -> (Tally, Tally) {
        let this = THIS_THREAD.with(Cell::get);
        let all = PROCESS
            .load(Ordering::Relaxed)
            .wrapping_sub(SPAN_FROM.with(Cell::get));
        (tally(this), tally(all))
    }

    /// **The allocations this thread made since [`start`]**, once the
    /// guard holds: the process made none but those. A frame whose work
    /// another thread did fails here rather than count it as none.
    pub fn take() -> Tally {
        let (this, all) = counts();
        assert_eq!(
            all, this,
            "the guard: a counted span's allocations, the process's and this thread's"
        );
        this
    }
}

/// The variable that tells this binary, run again, which test to run.
const ALONE: &str = "BAUD_STRUCK_CARET_ALONE";

/// **`body` as the only work of a process**: this test binary run again
/// for the test `name` alone, on one thread (`--exact`,
/// `--test-threads=1`), and `body` run there after [`guard_control`], so
/// the process counts no other test's allocations in a frame
/// ([`heap::take`]). The run's output is printed here, and it must pass,
/// and pass one test: a `name` that names none fails rather than pass
/// with nothing run.
fn isolated(name: &str, body: impl FnOnce()) {
    if std::env::var_os(ALONE).is_some_and(|v| v.to_str() == Some(name)) {
        guard_control();
        body();
        return;
    }
    let exe = std::env::current_exe().expect("this test's binary");
    let out = std::process::Command::new(exe)
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env(ALONE, name)
        .output()
        .expect("this test's binary runs again");
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    print!("{stdout}");
    eprint!("{stderr}");
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{name}, run alone: {}",
        out.status
    );
}

/// **The control that the guard sees work done on another thread**: a
/// thread spawned in a counted span allocates there, and the process's
/// count is more than this thread's.
fn guard_control() {
    heap::start();
    let made = std::thread::scope(|s| {
        s.spawn(|| std::hint::black_box(Vec::<u8>::with_capacity(64)).capacity())
            .join()
            .expect("the control's thread")
    });
    let (this, all) = heap::counts();
    assert!(
        made >= 64 && all.value() > this.value(),
        "the control: the guard sees another thread's allocation: process {} this thread {}",
        all.value(),
        this.value()
    );
}

/// **A test whose body runs as the only work of a process**
/// ([`isolated`]): the test is declared here, its doc with it, and run
/// again under the name it is declared with (`stringify!`), so the name
/// the process is asked to run and the test that asks cannot differ.
macro_rules! alone {
    ($(#[$doc:meta])* fn $name:ident() => $body:path) => {
        $(#[$doc])*
        #[test]
        fn $name() {
            isolated(stringify!($name), $body);
        }
    };
}

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

/// The struck link's row, and its cells on it: `https://example.org/x`,
/// in red.
const LINK_ROW: usize = 2;
const LINK: (usize, usize) = (4, 24);

/// The other link's row, in palette colour 4.
const SECOND_ROW: usize = 5;

/// A `[link] missing_color` that reads 4.5:1 on the dark page, so a struck
/// cell no selection covers is lent a slot.
const PINK: MissingPaint = MissingPaint::Colour(Argb(0xFFFF_8080));

/// The first caret frames, which build what the later ones keep: one a
/// set of the cell path's, and one more.
const WARM: usize = SETS + 1;

/// The caret frames counted after them.
const COUNTED: usize = 2 * SETS;

/// `bytes` fed to the terminal, in reads no longer than the app's.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// **The screen**: a row of text on every row, a red link on
/// [`LINK_ROW`] and a palette-blue one on [`SECOND_ROW`], and the cursor
/// shown as a steady block, so no phase of a blink tells one frame from
/// another.
fn screen() -> Vec<u8> {
    let mut out = b"\x1b[?25l".to_vec();
    for i in 0..ROWS {
        out.extend(format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).as_bytes());
    }
    out.extend(
        format!(
            "\x1b[{};1H\x1b[2Ksee \x1b[38;2;255;0;0mhttps://example.org/x\x1b[m here",
            LINK_ROW + 1
        )
        .as_bytes(),
    );
    out.extend(
        format!(
            "\x1b[{};1H\x1b[2Kand \x1b[34mhttps://example.org/y\x1b[m too",
            SECOND_ROW + 1
        )
        .as_bytes(),
    );
    out.extend(b"\x1b[2 q\x1b[?25h");
    out
}

/// The link on `row`, at [`LINK`]'s columns, in its cells' rendition.
fn link_on(t: &Term<'_>, row: usize) -> LinkLine {
    let r = Row::from_index(row);
    let cell = t.view(Rows::ZERO).row(r)[LINK.0];
    LinkLine {
        row: r,
        from: Col::at_index(LINK.0),
        to: Col::at_index(LINK.1),
        word: t.styles().one().word_of(cell),
    }
}

/// Whether the first link is struck.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strike {
    First,
    None,
}

/// Both links set at rest, the first struck where `strike` says, the
/// strike's text in [`PINK`]: as a driver sets them each frame.
fn set_links(r: &mut Renderer<'_, '_>, t: &Term<'_>, strike: Strike) {
    let lines = [link_on(t, LINK_ROW), link_on(t, SECOND_ROW)];
    r.set_link_lines(&lines, LineStyle::Solid);
    r.set_link_missing(PINK);
    match strike {
        Strike::First => r.set_link_strikes(&lines[..1], &[]),
        Strike::None => r.set_link_strikes(&[], &[]),
    }
}

/// A renderer over `faces` for the grid of `ws`, drawing on one thread,
/// the one that counts its allocations.
fn made<'f, 'a>(faces: &'f [Face<'a>], ws: WinSize) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(ws.ws_col)),
        Rows::of_index(usize::from(ws.ws_row)),
    )
    .unwrap();
    r.set_threads(core::num::NonZeroUsize::MIN);
    r
}

/// **The control that the counter sees a frame's allocations**: the
/// first frame, full, allocates.
fn counted(first: Tally, what: &str) {
    assert!(
        first.value() > 0,
        "the control: the {what} path's first frame is seen to allocate"
    );
}

/// The cursor moved to column `k` of the last row: a caret frame's one
/// change, which writes no row.
fn caret(t: &mut Term<'_>, k: usize) {
    feed(t, format!("\x1b[{ROWS};{}H", k % COLS + 1).as_bytes());
}

/// **One CPU frame of the window `back` lines up onto `px`**, the window's
/// writes sealed first as a driver seals them, and the allocations the
/// frame alone made.
fn cpu_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Tally {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    let mut target = Target::new(&mut px[..], w, w, h).unwrap();
    heap::start();
    let seal = t.seal_writes();
    r.render_at(t, seal, back, &mut target, damage);
    heap::take()
}

/// The cell path's buffers, one set a frame in flight as the app keeps
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
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>, ws: WinSize) -> Cells<'m> {
        let (cols, rows) = (usize::from(ws.ws_col), usize::from(ws.ws_row));
        let stride = t.stride().index();
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
        }
    }
}

/// **The cell frame `r` builds into set `at`** over the window `back`
/// lines up, as the app's `present` builds it with no composition,
/// prediction or slide, the frame's writes sealed first
/// (`Term::seal_writes`): the allocations the build alone made, and the
/// slots its set lent and left short.
fn cell_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    cell: &mut Cells<'_>,
    at: SetIx,
) -> (Tally, Tally, Tally) {
    let seal = t.seal_writes();
    let t = &*t;
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
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    let damage = r.damage(t, inputs.back);
    heap::start();
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
    let made = heap::take();
    (made, cell.lent[at.0].lent(), cell.lent[at.0].short())
}

/// **The struck link's text above its strike**, as `px` holds it: the
/// pixel rows of [`LINK_ROW`] over [`LINK`]'s cells from the top of the row
/// to the strike's, which neither the strike nor any line under the text
/// reaches, so they are drawn by the cells alone -- in the colour a lent
/// slot gives a struck cell's text, or in its own.
fn above_strike(r: &Renderer<'_, '_>, px: &[u32]) -> Vec<u32> {
    let m = r.metrics();
    let px_of = |v: u32| usize::try_from(v).unwrap();
    let (cw, ch, strike) = (
        px_of(m.cell_w.value()),
        px_of(m.cell_h.value()),
        px_of(m.strike.value()),
    );
    let stride = px_of(r.pixel_size().0.value());
    let top = LINK_ROW * ch;
    (top..top + strike)
        .flat_map(|y| (LINK.0 * cw..(LINK.1 + 1) * cw).map(move |x| px[y * stride + x]))
        .collect()
}

/// What [`struck_cpu`] found: the allocations of each counted caret
/// frame, the struck cells' finds the first, full, frame made, and the
/// struck link's text above its strike after each counted frame
/// ([`above_strike`]).
struct CpuRun {
    made: Vec<Tally>,
    first_finds: Tally,
    text: Vec<Vec<u32>>,
}

/// The CPU path's caret frames on [`screen`], the first link struck where
/// `strike` says.
fn struck_cpu(strike: Strike) -> CpuRun {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &screen());
    let mut r = made(&faces, WS);
    r.fit(&t);
    set_links(&mut r, &t, strike);
    let mut px = Vec::new();
    counted(
        cpu_frame(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full),
        "CPU",
    );
    let first_finds = r.struck_finds();
    let (mut allocs, mut text) = (Vec::new(), Vec::new());
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        set_links(&mut r, &t, strike);
        let got = cpu_frame(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
        if k >= WARM {
            allocs.push(got);
            text.push(above_strike(&r, &px));
        }
    }
    CpuRun {
        made: allocs,
        first_finds,
        text,
    }
}

/// The cell path's caret frames on [`screen`], the first link struck where
/// `strike` says, each into the next set in turn: the allocations of each
/// counted one, and the slots its set lent and left short.
fn struck_cell(strike: Strike) -> Vec<(Tally, Tally, Tally)> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &screen());
    let mut r = made(&faces, WS);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    let mut out = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        set_links(&mut r, &t, strike);
        let got = cell_frame(&mut r, &mut t, Rows::ZERO, &mut cells, SetIx(k % SETS));
        if k == 0 {
            counted(got.0, "cell");
        }
        if k >= WARM {
            out.push(got);
        }
    }
    out
}

alone! {
    /// **A struck link costs a CPU caret frame no allocation**: each caret
    /// frame over the struck link allocates what the same frame with no
    /// link struck allocates, and that is none. The controls, on the CPU
    /// path itself: its first frame finds the struck cells, and after every
    /// counted caret frame the struck link's text above its strike is drawn
    /// otherwise than the same text unstruck -- the lending's colour, which
    /// only a lent slot gives a struck cell -- so the frames counted lent
    /// the strike. Named breaks, each a buffer the CPU lending keeps made
    /// afresh each frame: the struck cells (a `StruckCells` made new at
    /// every take), the window's row sources (collected with no display
    /// held), the scratch `LendScratch::begin` empties (made new), the
    /// lending's number cloning the slots it gave (`Lendings::number`
    /// copying them for a lending it has seen), and the slots' values read
    /// before a give-back (`Lent::lent_values_into` collecting a new list).
    /// And `Lent`'s words made at every take of the CPU `Lent` (a new
    /// `Rc` in `Lent::default`): both arms make it, so the struck arm
    /// equals the unstruck, and the unstruck arm's literal 0 goes red.
    fn a_struck_link_costs_a_cpu_caret_frame_no_allocation() => struck_cpu_alone
}

/// [`a_struck_link_costs_a_cpu_caret_frame_no_allocation`], run alone.
fn struck_cpu_alone() {
    let (struck, none) = (struck_cpu(Strike::First), struck_cpu(Strike::None));
    assert!(
        struck.first_finds.value() > 0,
        "the control: the CPU path's first frame finds the struck cells"
    );
    for (k, (s, n)) in struck.text.iter().zip(&none.text).enumerate() {
        assert_ne!(
            s, n,
            "the control: CPU caret frame {k} draws the struck link's text as the unstruck one"
        );
    }
    let got: Vec<String> = struck
        .made
        .iter()
        .zip(&none.made)
        .map(|(s, n)| format!("{}/{}", s.value(), n.value()))
        .collect();
    println!(
        "cpu caret frames, allocations struck/unstruck: {}",
        got.join(" ")
    );
    assert!(
        none.made.iter().all(|n| n.value() == 0),
        "cpu caret frames with no link struck allocate: {}",
        got.join(" ")
    );
    assert_eq!(
        struck.made,
        none.made,
        "cpu caret frames allocate otherwise struck/unstruck: {}",
        got.join(" ")
    );
}

alone! {
    /// **A struck link costs a cell caret frame no allocation**: each caret
    /// frame over the struck link, built into the next set in turn,
    /// allocates what the same frame with no link struck allocates. The
    /// controls: every struck frame lends the link a slot, none short, and
    /// no unstruck frame lends one. Named breaks, each a buffer the cell
    /// path's lending keeps made afresh each frame: the struck cells (a
    /// `StruckCells` made new at every take), the window's row sources
    /// (collected with no display held), the scratch `LendScratch::begin`
    /// empties (made new), and the lending's number cloning the slots it
    /// gave. The slots' values read before a give-back are the CPU path's
    /// alone (`Lent::lent_values_into`), so no break of them reaches this.
    fn a_struck_link_costs_a_cell_caret_frame_no_allocation() => struck_cell_alone
}

/// [`a_struck_link_costs_a_cell_caret_frame_no_allocation`], run alone.
fn struck_cell_alone() {
    let (struck, none) = (struck_cell(Strike::First), struck_cell(Strike::None));
    for (k, ((_, s_lent, s_short), (_, n_lent, _))) in struck.iter().zip(&none).enumerate() {
        assert!(
            s_lent.value() > 0 && s_short.value() == 0,
            "the control: cell caret frame {k} lends the struck link a slot, none short"
        );
        assert_eq!(
            n_lent.value(),
            0,
            "the control: cell caret frame {k} with no link struck lends nothing"
        );
    }
    let got: Vec<String> = struck
        .iter()
        .zip(&none)
        .map(|(s, n)| format!("{}/{}", s.0.value(), n.0.value()))
        .collect();
    println!(
        "cell caret frames, allocations struck/unstruck: {}",
        got.join(" ")
    );
    let (s, n): (Vec<Tally>, Vec<Tally>) = (
        struck.iter().map(|c| c.0).collect(),
        none.iter().map(|c| c.0).collect(),
    );
    assert_eq!(
        s,
        n,
        "cell caret frames allocate otherwise struck/unstruck: {}",
        got.join(" ")
    );
}

/// **[`screen`] with the struck link's row drawn in another order**: three
/// Hebrew letters over its first word, and the bidirectional order drawn
/// (BDSM set), so the row's paragraph runs right to left.
fn reordered_screen() -> Vec<u8> {
    let mut out = b"\x1b[8h".to_vec();
    out.extend(screen());
    out.extend(format!("\x1b[{};1H\u{5D0}\u{5D1}\u{5D2}", LINK_ROW + 1).as_bytes());
    out
}

alone! {
    /// **A strike on a row drawn in another order costs a caret frame no
    /// find**, on both paths: the struck cells found on the first frame are
    /// kept while no row's order and no display was laid out otherwise
    /// since, as a row in its own order keeps them. The controls: the struck
    /// row is drawn in another order, the first frame finds the struck
    /// cells, and every cell frame lends them a slot, none short. Named
    /// break: the struck cells found again every frame a strike's row is
    /// drawn in another order.
    fn a_strike_on_a_reordered_row_costs_a_caret_frame_no_find() => reordered_alone
}

/// [`a_strike_on_a_reordered_row_costs_a_caret_frame_no_find`], run alone.
fn reordered_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let row = Row::from_index(LINK_ROW);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &reordered_screen());
    let mut r = made(&faces, WS);
    r.fit(&t);
    set_links(&mut r, &t, Strike::First);
    let mut px = Vec::new();
    cpu_frame(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full);
    assert!(
        r.bidi().reordered(row),
        "the control: the struck row is drawn in another order on the CPU path"
    );
    assert!(
        r.struck_finds().value() > 0,
        "the control: the CPU path's first frame finds the struck cells"
    );
    let mut cpu = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        set_links(&mut r, &t, Strike::First);
        let was = r.struck_finds();
        cpu_frame(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
        if k >= WARM {
            cpu.push(r.struck_finds().value() - was.value());
        }
    }
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &reordered_screen());
    let mut r = made(&faces, WS);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    let mut cell = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        set_links(&mut r, &t, Strike::First);
        let was = r.struck_finds();
        let (_, lent, short) = cell_frame(&mut r, &mut t, Rows::ZERO, &mut cells, SetIx(k % SETS));
        assert!(
            lent.value() > 0 && short.value() == 0,
            "the control: cell frame {k} lends the struck link a slot, none short"
        );
        if k == 0 {
            assert!(
                r.bidi().reordered(row),
                "the control: the struck row is drawn in another order on the cell path"
            );
            assert!(
                r.struck_finds().value() > was.value(),
                "the control: the cell path's first frame finds the struck cells"
            );
        }
        if k >= WARM {
            cell.push(r.struck_finds().value() - was.value());
        }
    }
    println!("caret frames over a strike on a reordered row, finds: cpu {cpu:?}; cell {cell:?}");
    assert!(
        cpu.iter().chain(&cell).all(|&n| n == 0),
        "caret frames over a strike on a reordered row find it again: cpu {cpu:?}; cell {cell:?}"
    );
}

/// **[`screen`] with both links in the struck one's rendition**: the
/// second written again in the first's red, so a strike moved from one to
/// the other moves no rendition.
fn one_rendition_screen() -> Vec<u8> {
    let mut out = screen();
    out.extend(
        format!(
            "\x1b[{};1H\x1b[2Kand \x1b[38;2;255;0;0mhttps://example.org/y\x1b[m too",
            SECOND_ROW + 1
        )
        .as_bytes(),
    );
    out
}

/// Which of the two links a [`struck_frame`] strikes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Struck {
    First,
    Second,
}

/// **One cell frame into set `at`**, both links set at rest and the one
/// `struck` names struck, its text in [`PINK`], as a driver sets them each
/// frame. The control: the frame lends the struck link a slot, and none is
/// short.
fn struck_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    cells: &mut Cells<'_>,
    struck: Struck,
    at: SetIx,
) {
    let lines = [link_on(t, LINK_ROW), link_on(t, SECOND_ROW)];
    r.set_link_lines(&lines, LineStyle::Solid);
    r.set_link_missing(PINK);
    let one = match struck {
        Struck::First => 0,
        Struck::Second => 1,
    };
    r.set_link_strikes(&lines[one..=one], &[]);
    let (_, lent, short) = cell_frame(r, t, Rows::ZERO, cells, at);
    assert!(
        lent.value() > 0 && short.value() == 0,
        "the control: the frame into {at:?} lends the {struck:?} link a slot, none short"
    );
}

alone! {
    /// **The struck cells' generation moves when they come out otherwise,
    /// and only then** ([`Renderer::struck_gen`]), on the cell path, whose
    /// rows a reader keeps: a find that gives the same cells -- the hover
    /// moved onto the other link, which moves the driver's inputs -- leaves
    /// it, and the strike moved to the other link's row, in the same
    /// rendition, moves it. The controls: both links are in one rendition,
    /// each step finds the struck cells again, and every frame lends the
    /// struck link a slot, none short. Named breaks: the generation moved
    /// on every find; the generation never moved.
    fn the_struck_generation_moves_when_the_struck_cells_do() => generation_alone
}

/// [`the_struck_generation_moves_when_the_struck_cells_do`], run alone.
fn generation_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &one_rendition_screen());
    let second = link_on(&t, SECOND_ROW);
    assert_eq!(
        link_on(&t, LINK_ROW).word,
        second.word,
        "the control: both links in one rendition"
    );
    let mut r = made(&faces, WS);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    caret(&mut t, 0);
    struck_frame(&mut r, &mut t, &mut cells, Struck::First, SetIx(0));
    let (was, finds) = (r.struck_gen(), r.struck_finds());
    r.set_link_hover(Some(&LinkHover {
        spans: vec![LinkSpan {
            row: second.row,
            from: second.from,
            to: second.to,
        }],
        paint: HoverPaint::Line,
    }));
    caret(&mut t, 1);
    struck_frame(&mut r, &mut t, &mut cells, Struck::First, SetIx(1 % SETS));
    assert!(
        r.struck_finds().value() > finds.value(),
        "the control: the hover moved finds the struck cells again"
    );
    let (hovered, finds) = (r.struck_gen(), r.struck_finds());
    caret(&mut t, 2);
    struck_frame(&mut r, &mut t, &mut cells, Struck::Second, SetIx(2 % SETS));
    assert!(
        r.struck_finds().value() > finds.value(),
        "the control: the strike moved finds the struck cells again"
    );
    let moved = r.struck_gen();
    println!("struck generation: first {was:?}, hover moved {hovered:?}, strike moved {moved:?}");
    assert_eq!(
        hovered, was,
        "a find that gives the same struck cells moves their generation"
    );
    assert_ne!(
        moved, was,
        "a strike moved to another row leaves its generation"
    );
}

/// The struck cells `r` holds for the window `t` shows, as the cell path
/// takes them, taken out and put back.
fn struck_runs(r: &mut Renderer<'_, '_>, t: &Term<'_>) -> Vec<(Row, Range<usize>)> {
    let view = t.view(Rows::ZERO);
    let struck = r.take_struck(t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let runs = struck.cells().to_vec();
    r.put_struck(struck);
    runs
}

alone! {
    /// **The struck cells' generation reads true while a lending has them
    /// out** ([`Renderer::take_struck`]): the strike moved to the other
    /// link's row and the cells taken out, the generation read before they
    /// are put back is the one read after it, and not the one read before
    /// the move; taken out again with nothing moved, it reads what it read.
    /// The controls: the move finds the cells again and the cells taken out
    /// are on the other link's row; the second take finds none. Named
    /// break: the generation read from the cells the renderer holds, which
    /// while they are out are none.
    fn the_struck_generation_reads_true_while_the_cells_are_out() => out_alone
}

/// [`the_struck_generation_reads_true_while_the_cells_are_out`], run alone.
fn out_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &one_rendition_screen());
    let mut r = made(&faces, WS);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    caret(&mut t, 0);
    struck_frame(&mut r, &mut t, &mut cells, Struck::First, SetIx(0));
    let before = r.struck_gen();
    let lines = [link_on(&t, LINK_ROW), link_on(&t, SECOND_ROW)];
    r.set_link_strikes(&lines[1..], &[]);
    let view = t.view(Rows::ZERO);
    let finds = r.struck_finds();
    let struck = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let out = r.struck_gen();
    let rows: Vec<Row> = struck.cells().iter().map(|c| c.0).collect();
    r.put_struck(struck);
    let back = r.struck_gen();
    assert!(
        r.struck_finds().value() > finds.value(),
        "the control: the strike moved finds the struck cells again"
    );
    assert_eq!(
        rows,
        vec![Row::from_index(SECOND_ROW)],
        "the control: the cells taken out are on the other link's row"
    );
    let finds = r.struck_finds();
    let struck = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let again = r.struck_gen();
    r.put_struck(struck);
    assert_eq!(
        r.struck_finds(),
        finds,
        "the control: taken out again with nothing moved, the struck cells are kept"
    );
    println!(
        "struck generation: before the move {before:?}, out {out:?}, back {back:?}, out again \
         {again:?}"
    );
    assert_ne!(
        out, before,
        "the struck generation read while the moved cells are out is the one before the move"
    );
    assert_eq!(
        out, back,
        "the struck generation read while the cells are out is not the one they are put back with"
    );
    assert_eq!(
        again, back,
        "kept struck cells taken out read another generation"
    );
}

alone! {
    /// **A renderer made again starts at a struck generation no reader of
    /// another holds**: two renderers made over one screen read other
    /// generations, and so they do after a frame each, the one striking the
    /// first link and the other the second -- so a reader that kept what it
    /// made from the one renderer's struck cells cannot take the other's
    /// for them. The controls: the two renderers' struck cells differ, and
    /// each frame lends its struck link a slot, none short. Named break:
    /// each renderer numbering its struck cells' generations from one
    /// start.
    fn a_renderer_made_again_starts_a_struck_generation_no_reader_holds() => again_alone
}

/// [`a_renderer_made_again_starts_a_struck_generation_no_reader_holds`],
/// run alone.
fn again_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &one_rendition_screen());
    let (mut one, mut two) = (made(&faces, WS), made(&faces, WS));
    let made_gens = (one.struck_gen(), two.struck_gen());
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    let mut backing_two = GlyphMap::backing();
    let mut cells_two = Cells::new(&mut backing_two, &t, WS);
    caret(&mut t, 0);
    struck_frame(&mut one, &mut t, &mut cells, Struck::First, SetIx(0));
    struck_frame(&mut two, &mut t, &mut cells_two, Struck::Second, SetIx(0));
    let (runs_one, runs_two) = (struck_runs(&mut one, &t), struck_runs(&mut two, &t));
    assert_ne!(
        runs_one, runs_two,
        "the control: the two renderers' struck cells differ"
    );
    let drawn_gens = (one.struck_gen(), two.struck_gen());
    println!("struck generations of two renderers: made {made_gens:?}, drawn {drawn_gens:?}");
    assert_ne!(
        made_gens.0, made_gens.1,
        "two renderers made read one struck generation"
    );
    assert_ne!(
        drawn_gens.0, drawn_gens.1,
        "two renderers whose struck cells differ read one struck generation"
    );
}

alone! {
    /// **Struck cells lost move their generation**: taken out and never put
    /// back -- as a panic between a lending's take and its put leaves them,
    /// and a renderer drawn on after one -- the strikes then cleared, the
    /// next take finds the cells again from nothing, and the generation it
    /// leaves is not the one that named the lost cells: a find from nothing
    /// that gives none compares as the same as nothing, so only the loss
    /// itself can move it. The controls: the lost cells were on the struck
    /// link's row, the next take finds them again, and finds none. Named
    /// break: the generation left as it was when the cells are found lost.
    fn struck_cells_lost_move_their_generation() => lost_alone
}

/// [`struck_cells_lost_move_their_generation`], run alone.
fn lost_alone() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &one_rendition_screen());
    let mut r = made(&faces, WS);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, WS);
    caret(&mut t, 0);
    struck_frame(&mut r, &mut t, &mut cells, Struck::First, SetIx(0));
    let view = t.view(Rows::ZERO);
    let held = r.struck_gen();
    let lost = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let rows: Vec<Row> = lost.cells().iter().map(|c| c.0).collect();
    drop(lost);
    r.set_link_strikes(&[], &[]);
    let finds = r.struck_finds();
    let found = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let after = r.struck_gen();
    let none = found.cells().is_empty();
    r.put_struck(found);
    assert_eq!(
        rows,
        vec![Row::from_index(LINK_ROW)],
        "the control: the cells lost are on the struck link's row"
    );
    assert!(
        r.struck_finds().value() > finds.value() && none,
        "the control: the take after the loss finds the cells again, and none"
    );
    println!("struck generation: held {held:?}, after the loss {after:?}");
    assert_ne!(
        after, held,
        "struck cells lost and found again from nothing keep the generation that named them"
    );
}

/// **The struck cells put back twice for one take is a fault a debug build
/// stops at** ([`Renderer::put_struck`]): a CPU frame drawn over the struck
/// link, then two takes, the second finding the first's cells out and so
/// lost, and both put back. Not run alone and counting nothing, so its
/// frame is drawn without the counted helpers, whose guard reads the
/// process's other tests. Named break: the assertion left out of
/// `put_struck`.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "the struck cells put back twice for one take")]
fn struck_cells_put_back_twice_panic_in_a_debug_build() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, &screen());
    let mut r = made(&faces, WS);
    r.fit(&t);
    set_links(&mut r, &t, Strike::First);
    let (w, h) = r.pixel_size();
    let mut px = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let seal = t.seal_writes();
    r.render_at(
        &t,
        seal,
        Rows::ZERO,
        &mut Target::new(&mut px[..], w, w, h).unwrap(),
        Damage::Full,
    );
    let view = t.view(Rows::ZERO);
    let one = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    let two = r.take_struck(&t, Rows::ZERO, &view, view.rows(), CopyOrder::Stored);
    r.put_struck(one);
    r.put_struck(two);
}

/// A screen of 40x24 whose history the core froze, a rendition a cell.
const TALL: WinSize = WinSize {
    ws_col: 40,
    ws_row: 24,
};

/// Colour 8 a light gray, as a theme with a light gray 8 sets it.
const GRAY: &[u8] = b"\x1b]4;8;rgb:c0/c0/c0\x1b\\";

/// `lines` lines of 40 cells in dark truecolour grounds, each cell its own
/// rendition.
fn flood(lines: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for l in 0..lines {
        for x in 0..40 {
            let i = l * 40 + x;
            out.extend(format!("\x1b[0;48;2;{};{};20mq", i % 41, (i / 41) % 41).as_bytes());
        }
        out.extend(b"\x1b[0m\r\n");
    }
    out
}

/// **[`TALL`]'s history frozen**, sixty lines of [`flood`] written past
/// the table's room, and how far back the window looks to show
/// twenty-four lines of it.
fn frozen_history(t: &mut Term<'_>) -> Rows {
    feed(t, GRAY);
    feed(t, &flood(60));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    Rows::of_count(60 + 1 - 24)
}

/// Whether a window looks back over the frozen history or is the live
/// screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Window {
    Frozen,
    Live,
}

/// The window [`frozen_history`] leaves, looked back or live, on a
/// renderer of its own: whether its layout holds a row in another order or
/// a display, which its window holds no picture of, and whether it shows a
/// frozen row.
fn premises(r: &Renderer<'_, '_>, t: &Term<'_>, back: Rows) -> (bool, bool) {
    let rows = usize::from(TALL.ws_row);
    let plain =
        r.bidi().display().is_none() && (0..rows).all(|y| !r.bidi().reordered(Row::from_index(y)));
    (plain, r.bidi().window_frozen(t, back))
}

/// The CPU path's caret frames on the frozen screen, the window as
/// `window` says: the allocations of each counted one.
fn frozen_cpu(window: Window) -> Vec<Tally> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = match (frozen_history(&mut t), window) {
        (back, Window::Frozen) => back,
        (_, Window::Live) => Rows::ZERO,
    };
    let mut r = made(&faces, TALL);
    let mut px = Vec::new();
    counted(
        cpu_frame(&mut r, &mut t, back, &mut px, Damage::Full),
        "CPU",
    );
    let (plain, frozen) = premises(&r, &t, back);
    assert!(plain, "the control: no row in another order, no display");
    assert_eq!(
        frozen,
        window == Window::Frozen,
        "the control: the {window:?} window shows a frozen row"
    );
    let mut out = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        let made = cpu_frame(&mut r, &mut t, back, &mut px, Damage::Incremental);
        if k >= WARM {
            out.push(made);
        }
    }
    out
}

/// The cell path's caret frames on the frozen screen, the window as
/// `window` says, each into the next set in turn: the allocations of each
/// counted one, and the slots its set lent.
fn frozen_cell(window: Window) -> Vec<(Tally, Tally, Tally)> {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = match (frozen_history(&mut t), window) {
        (back, Window::Frozen) => back,
        (_, Window::Live) => Rows::ZERO,
    };
    let mut r = made(&faces, TALL);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, TALL);
    let mut out = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        let got = cell_frame(&mut r, &mut t, back, &mut cells, SetIx(k % SETS));
        if k == 0 {
            counted(got.0, "cell");
            let (plain, frozen) = premises(&r, &t, back);
            assert!(plain, "the control: no row in another order, no display");
            assert_eq!(
                frozen,
                window == Window::Frozen,
                "the control: the {window:?} window shows a frozen row"
            );
        }
        if k >= WARM {
            out.push(got);
        }
    }
    out
}

alone! {
    /// **A frozen window costs a caret frame no allocation** (C19's frozen
    /// fold): on both paths, each caret frame over a window that looks back
    /// over history the core froze -- no row of it in another order, and no
    /// display held -- allocates what the same screen's live window
    /// allocates. The controls: the window shows frozen rows and the live
    /// one none, and every cell frame over the frozen one lends them slots.
    /// Named breaks, each made afresh every frame on both paths: the
    /// window's row sources (collected with no display held), and the
    /// frozen pass's map of the slots it gave (`LendScratch::begin` making
    /// `given` new rather than emptying it).
    fn a_frozen_window_costs_a_caret_frame_no_allocation() => frozen_alone
}

/// [`a_frozen_window_costs_a_caret_frame_no_allocation`], run alone.
fn frozen_alone() {
    let (frozen, live) = (frozen_cell(Window::Frozen), frozen_cell(Window::Live));
    for (k, (_, lent, _)) in frozen.iter().enumerate() {
        assert!(
            lent.value() > 0,
            "the control: cell caret frame {k} lends the frozen rows slots"
        );
    }
    let cell: Vec<String> = frozen
        .iter()
        .zip(&live)
        .map(|(f, l)| format!("{}/{}", f.0.value(), l.0.value()))
        .collect();
    let (f_cpu, l_cpu) = (frozen_cpu(Window::Frozen), frozen_cpu(Window::Live));
    let cpu: Vec<String> = f_cpu
        .iter()
        .zip(&l_cpu)
        .map(|(f, l)| format!("{}/{}", f.value(), l.value()))
        .collect();
    println!(
        "caret frames, allocations frozen/live: cpu {}; cell {}",
        cpu.join(" "),
        cell.join(" ")
    );
    let (f_cell, l_cell): (Vec<Tally>, Vec<Tally>) = (
        frozen.iter().map(|c| c.0).collect(),
        live.iter().map(|c| c.0).collect(),
    );
    assert!(
        f_cpu == l_cpu && f_cell == l_cell,
        "caret frames allocate otherwise frozen/live: cpu {}; cell {}",
        cpu.join(" "),
        cell.join(" ")
    );
}

/// The paragraph [`frozen_cut_history`] writes, 60 cells the terminal's
/// cut splits inside a word: "do|g" at the fortieth column.
const CUT: &[u8] = b"a quick brown fox jumps over the lazy dog and then runs away\r\n";

/// **[`TALL`]'s history frozen with a paragraph the terminal's cut splits
/// inside a word**: ten lines of [`flood`], [`CUT`] and forty-eight more,
/// and how far back the window looks to show the oldest twenty-four lines,
/// the paragraph among them.
fn frozen_cut_history(t: &mut Term<'_>) -> Rows {
    feed(t, GRAY);
    feed(t, &flood(10));
    feed(t, CUT);
    feed(t, &flood(48));
    assert!(t.style_reclaims().rewrites > 0, "the control: it froze");
    Rows::of_count(60 + 1 - 24)
}

/// Whether [`frozen_cut_history`]'s paragraph is drawn at its words, which
/// holds a display, or as the terminal cut it, which holds none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Display,
    Cut,
}

impl Held {
    /// The word wrap that draws it so.
    fn wrap(self) -> WordWrap {
        match self {
            Held::Display => WordWrap::On,
            Held::Cut => WordWrap::Off,
        }
    }
}

/// What a run over [`frozen_cut_history`]'s window found after its first
/// frame: whether a display was held ([`Held::Display`]) or not, whether
/// the window shows a frozen row ([`Window::Frozen`]) or none, and what
/// that frame allocated.
#[derive(Clone, Copy, Debug)]
struct Facts {
    held: Held,
    shows: Window,
    first: Tally,
}

/// [`Facts`] of `r`'s layout over the window `back` lines up, its first
/// frame having allocated `first`.
fn facts(r: &Renderer<'_, '_>, t: &Term<'_>, back: Rows, first: Tally) -> Facts {
    Facts {
        held: match r.bidi().display() {
            Some(_) => Held::Display,
            None => Held::Cut,
        },
        shows: match r.bidi().window_frozen(t, back) {
            true => Window::Frozen,
            false => Window::Live,
        },
        first,
    }
}

/// The CPU path's caret frames over [`frozen_cut_history`]'s window, its
/// paragraph drawn as `held` says: what the first frame found, and the
/// allocations of each counted one.
fn held_cpu(held: Held) -> (Facts, Vec<Tally>) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = frozen_cut_history(&mut t);
    let mut r = made(&faces, TALL);
    r.set_word_wrap(held.wrap());
    let mut px = Vec::new();
    let first = cpu_frame(&mut r, &mut t, back, &mut px, Damage::Full);
    let found = facts(&r, &t, back, first);
    let mut out = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        let made = cpu_frame(&mut r, &mut t, back, &mut px, Damage::Incremental);
        if k >= WARM {
            out.push(made);
        }
    }
    (found, out)
}

/// The cell path's caret frames over [`frozen_cut_history`]'s window, its
/// paragraph drawn as `held` says, each into the next set in turn: what
/// the first frame found, and the allocations of each counted one and the
/// slots its set lent.
fn held_cell(held: Held) -> (Facts, Vec<(Tally, Tally, Tally)>) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut owned = Owned::new(&TALL);
    let mut t = Term::new(owned.storage(), &TALL);
    let back = frozen_cut_history(&mut t);
    let mut r = made(&faces, TALL);
    r.set_word_wrap(held.wrap());
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, TALL);
    let mut found = None;
    let mut out = Vec::new();
    for k in 0..WARM + COUNTED {
        caret(&mut t, k);
        let got = cell_frame(&mut r, &mut t, back, &mut cells, SetIx(k % SETS));
        if k == 0 {
            found = Some(facts(&r, &t, back, got.0));
        }
        if k >= WARM {
            out.push(got);
        }
    }
    (found.expect("a first frame"), out)
}

/// The allocations of `a` and `b`, frame by frame, as `a/b`.
fn pairs(a: &[Tally], b: &[Tally]) -> String {
    let each: Vec<String> = a
        .iter()
        .zip(b)
        .map(|(x, y)| format!("{}/{}", x.value(), y.value()))
        .collect();
    each.join(" ")
}

alone! {
    /// **A frozen window with a display held has its caret frames'
    /// allocations counted, and printed rather than held to a figure**
    /// (FS-C14 3a.1 owes the frozen window under a display a figure),
    /// beside the same window drawn as the terminal cut it, which holds no
    /// display. The controls: a display is held over the one window and not
    /// the other, both show a frozen row, each path's first frame is seen
    /// to allocate, and every cell frame lends the frozen rows slots.
    fn a_frozen_window_under_a_held_display_has_its_caret_frames_counted() => held_alone
}

/// [`a_frozen_window_under_a_held_display_has_its_caret_frames_counted`],
/// run alone.
fn held_alone() {
    let (cpu_held, cpu_cut) = (held_cpu(Held::Display), held_cpu(Held::Cut));
    let (cell_held, cell_cut) = (held_cell(Held::Display), held_cell(Held::Cut));
    for (held, path, found) in [
        (Held::Display, "CPU", cpu_held.0),
        (Held::Cut, "CPU", cpu_cut.0),
        (Held::Display, "cell", cell_held.0),
        (Held::Cut, "cell", cell_cut.0),
    ] {
        assert_eq!(
            (found.held, found.shows),
            (held, Window::Frozen),
            "the control: the {held:?} window on the {path} path holds a display as it says, and \
             shows a frozen row: {found:?}"
        );
        counted(found.first, path);
    }
    for (k, (_, lent, _)) in cell_held.1.iter().chain(&cell_cut.1).enumerate() {
        assert!(
            lent.value() > 0,
            "the control: cell caret frame {k} lends the frozen rows slots"
        );
    }
    let cell = |v: &[(Tally, Tally, Tally)]| -> Vec<Tally> { v.iter().map(|c| c.0).collect() };
    println!(
        "caret frames over a frozen window, allocations display held/cut: cpu {}; cell {}",
        pairs(&cpu_held.1, &cpu_cut.1),
        pairs(&cell(&cell_held.1), &cell(&cell_cut.1))
    );
}
