//! **A veil kept is never a veil stale** (FS-C16): baud's veils -- the
//! fields of the link under the pointer, the find field, the link
//! preview's bar, the hover tip's lines, the notice -- are drawn on the CPU
//! path from records kept while nothing they are drawn from moves. One
//! control a kind: a frame whose veil changed draws, pixel for pixel, what
//! a renderer of its own draws in one full frame, which has nothing kept;
//! and a steady frame composes none, the count the kept records save. The
//! input method's composition is not among what they are drawn from: one
//! that changes beside a kept veil is drawn and composes no veil, and no
//! veil is composed from it.
//!
//! **And on the cell path** (`baud_render::cell_frame::build`, as the app
//! builds it, its sets taken in turn): the same six changes, each drawn as
//! the records and the masked cells a fresh renderer's first cell frame
//! makes -- the cell path keeps its veils apart from the CPU path's, each
//! set's rendition table among what its key reads.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{
    BytePos, Codepoint, Col, Cols, PackedCell, ReadLen, Row, RowExtent, Rows, WinSize,
};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Blend, Candidate, Caret, Composition, Damage, Field, FindLook, GlyphMap, Hints, HoverPaint,
    Instance, Kind, Leaving, Lent, LinkHover, LinkLook, LinkSpan, MASK_RECORD, MapEntry, Measure,
    Overlay, PxSize, Renderer, Resolved, Stack, StyleStamp, Target, Tex, Theme, TipArea, TipLook,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 8;

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn text(s: &str) -> Vec<Codepoint> {
    s.chars().map(Codepoint::from_char).collect()
}

/// A field of `s` at `row`, `col`, every character in the message pair.
fn field(row: usize, col: usize, s: &str) -> Field {
    let text = text(s);
    Field {
        row: Row::from_index(row),
        col: Col::at_index(col),
        hints: vec![Hints::MESSAGE; text.len()],
        caret: Col::at_index(col + text.len()),
        text,
    }
}

/// **A change of a veil draws what a fresh renderer draws**: a screen of
/// text `ROWS` rows high, `rows` of them the terminal's (one fewer leaves
/// the notice its row), `before` setting a veil on the renderer, a full
/// frame and two incremental ones; then a steady frame, which composes no
/// veil -- the control that the records are kept -- and `after` changing
/// the veil, `written` fed to the terminal, and an incremental frame, which
/// draws, pixel for pixel, what a renderer of its own draws in one full
/// frame given the veils as they stand then, which has nothing kept to
/// draw stale -- the staleness control -- and other pixels than the steady
/// one -- the control that the change shows -- and composes again; and
/// what the same renderer's full frame then draws.
fn on_the_cpu_path(
    what: &str,
    rows: Rows,
    before: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    after: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    written: &[u8],
    counted: Counted,
) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let made = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(COLS),
            Rows::of_index(ROWS),
        )
        .unwrap()
    };
    let mut r = made();
    let ws = WinSize {
        ws_col: u16::try_from(COLS).unwrap(),
        ws_row: u16::try_from(rows.index()).unwrap(),
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let lines: Vec<u8> = (0..rows.index())
        .flat_map(|i| format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).into_bytes())
        .collect();
    feed(&mut t, &lines);
    before(&mut r, &t);
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    let (mut px, mut full) = (vec![0u32; n], vec![0u32; n]);
    let paint = |r: &mut Renderer<'_, '_>, t: &mut Term<'_>, px: &mut [u32], d: Damage| {
        let mut target = Target::new(px, w, w, h).unwrap();
        r.render(t, &mut target, d);
    };
    paint(&mut r, &mut t, &mut px, Damage::Full);
    for _ in 0..2 {
        paint(&mut r, &mut t, &mut px, Damage::Incremental);
    }
    let composed = r.veils_composed();
    paint(&mut r, &mut t, &mut px, Damage::Incremental);
    assert_eq!(
        r.veils_composed(),
        composed,
        "{what}: the control, a steady frame composes no veil"
    );
    let steady = px.clone();
    after(&mut r, &t);
    if !written.is_empty() {
        feed(&mut t, written);
    }
    paint(&mut r, &mut t, &mut px, Damage::Incremental);
    // The pixels first, so a veil kept stale fails here, at the staleness
    // control, before the count says why.
    let mut fresh = made();
    before(&mut fresh, &t);
    after(&mut fresh, &t);
    fresh.fit(&t);
    paint(&mut fresh, &mut t, &mut full, Damage::Full);
    assert!(
        px == full,
        "{what}: the frame after the change is a fresh renderer's pixels"
    );
    assert!(px != steady, "{what}: the control, the change is drawn");
    assert!(composed.value() > 0, "{what}: the control, a veil composed");
    match counted {
        Counted::Composed => assert!(
            r.veils_composed().value() > composed.value(),
            "{what}: the changed veil composed again"
        ),
        Counted::Kept => assert_eq!(
            r.veils_composed(),
            composed,
            "{what}: the change composed no veil"
        ),
    }
    paint(&mut r, &mut t, &mut full, Damage::Full);
    assert!(
        px == full,
        "{what}: the frame after the change is the full frame's pixels"
    );
}

/// Which path a change is drawn on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Path {
    Cpu,
    Cell,
}

/// A change of a veil drawn on `path` ([`on_the_cpu_path`],
/// [`on_the_cell_path`]).
fn a_changed_veil_is_drawn(
    path: Path,
    what: &str,
    rows: Rows,
    before: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    after: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    written: &[u8],
    counted: Counted,
) {
    match path {
        Path::Cpu => on_the_cpu_path(what, rows, before, after, written, counted),
        Path::Cell => on_the_cell_path(what, rows, before, after, written),
    }
}

/// The cell path's buffers for a terminal `rows` rows high, one set a
/// frame in flight as the app keeps them, the notice's row among them.
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
    bidi_cells: Vec<PackedCell>,
    bidi_used: Vec<RowExtent>,
    lent_cells: Vec<PackedCell>,
    lent_used: Vec<RowExtent>,
    used: Vec<RowExtent>,
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>, rows: Rows) -> Cells<'m> {
        let (stride, rows) = (t.stride().index(), rows.index());
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; COLS * (rows + 1)]),
            style_stamps: [StyleStamp::default(); SETS],
            lent: core::array::from_fn(|_| Lent::default()),
            candidates: Vec::new(),
            records: Vec::new(),
            grounds: Vec::new(),
            bidi_cells: Vec::new(),
            bidi_used: Vec::new(),
            lent_cells: Vec::new(),
            lent_used: Vec::new(),
            used: Vec::new(),
        }
    }
}

/// **What a cell frame draws baud's veils as**: each veil's record as the
/// shader reads it, its place in the atlas left out and the texels it
/// reads there in its place -- so two renderers whose atlases filled in
/// other orders compare by what they draw -- and the cells masked off the
/// shader for a record.
#[derive(PartialEq, Debug)]
struct VeilsDrawn {
    records: Vec<(Instance, Vec<u32>)>,
    masked: Vec<bool>,
}

/// **The cell frame `r` builds into set `at`**, as the app's `present`
/// builds it with no composition, prediction or slide, and the veils it
/// draws: the records between the bell's tint and the splash, at the end
/// of the frame's (`CellFrame::chrome`), the frame's writes sealed first
/// (`Term::seal_writes`).
fn cell_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    cell: &mut Cells<'_>,
    at: SetIx,
) -> VeilsDrawn {
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
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    let frame = build(
        r,
        t,
        seal,
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
            bidi_cells: &mut cell.bidi_cells,
            bidi_used: &mut cell.bidi_used,
            lent_cells: &mut cell.lent_cells,
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        || Some(at),
    )
    .expect("the set is free");
    let chrome = usize::try_from(frame.chrome.value()).unwrap();
    let tail = &cell.records[cell.records.len() - chrome..];
    let after = r.splash().len() + r.scrollbar().len() + r.screen_fade().len();
    let veils = &tail[r.bell_tint().len()..tail.len() - after];
    let atlas = r.atlas();
    let side = usize::try_from(atlas.side().value()).unwrap();
    let color_side = usize::try_from(atlas.color_side().value()).unwrap();
    let texels = |i: &Instance| -> Vec<u32> {
        let (u, v) = (usize::from(i.u.0), usize::from(i.v.0));
        let (w, h) = (usize::from(i.w.0), usize::from(i.h.0));
        let cells_of =
            |side: usize| (0..h).flat_map(move |y| (0..w).map(move |x| (v + y) * side + u + x));
        match i.kind {
            Kind::Mask => cells_of(side)
                .map(|k| atlas.pixels().get(k).copied().map_or(u32::MAX, u32::from))
                .collect(),
            Kind::Color => cells_of(color_side)
                .map(|k| atlas.argb().get(k).copied().unwrap_or(u32::MAX))
                .collect(),
            _ => Vec::new(),
        }
    };
    VeilsDrawn {
        records: veils
            .iter()
            .map(|i| {
                let placed = Instance {
                    u: Tex(0),
                    v: Tex(0),
                    ..*i
                };
                (placed, texels(i))
            })
            .collect(),
        masked: cell.overlay_mem[at.0]
            .iter()
            .map(|&m| m & MASK_RECORD != 0)
            .collect(),
    }
}

/// **A change of a veil draws, on the cell path, what a fresh renderer
/// draws** (X-1): the screen [`on_the_cpu_path`] draws, `before` setting
/// a veil, cell frames into each set in turn until every set has been
/// built from and the first again; then a steady frame, which composes no
/// veil -- the control that the records are kept across the sets, each
/// set's rendition table read under its own stamp -- and `after` changing
/// the veil, `written` fed to the terminal, and a frame into the next set,
/// which draws the veils as a renderer of its own draws them in its first
/// cell frame given the veils as they stand then, which has nothing kept
/// to draw stale -- the staleness control: the same records, reading the
/// same texels, and the same cells masked -- and not as the steady frame
/// drew them -- the control that the change shows -- and composes again;
/// and the frame into the set after, the records kept, the same again.
/// Named break: the cell path's key never moving once a veil is held.
fn on_the_cell_path(
    what: &str,
    rows: Rows,
    before: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    after: impl Fn(&mut Renderer<'_, '_>, &Term<'_>),
    written: &[u8],
) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let made = || {
        Renderer::new(
            Stack::new(&faces).unwrap(),
            PxSize::of(14.0).unwrap(),
            Theme::dark(),
            Blend::Native,
            Cols::of_index(COLS),
            Rows::of_index(ROWS),
        )
        .unwrap()
    };
    let mut r = made();
    let ws = WinSize {
        ws_col: u16::try_from(COLS).unwrap(),
        ws_row: u16::try_from(rows.index()).unwrap(),
    };
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let lines: Vec<u8> = (0..rows.index())
        .flat_map(|i| format!("\x1b[{};1Hrow {i} abcdefghijklmnopqrstuvwxyz", i + 1).into_bytes())
        .collect();
    feed(&mut t, &lines);
    before(&mut r, &t);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t, rows);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    for _ in 0..=SETS {
        cell_frame(&mut r, &mut t, &mut cells, next());
    }
    let composed = r.veils_composed();
    assert!(composed.value() > 0, "{what}: the control, a veil composed");
    let steady = cell_frame(&mut r, &mut t, &mut cells, next());
    assert_eq!(
        r.veils_composed(),
        composed,
        "{what}: the control, a steady frame composes no veil"
    );
    after(&mut r, &t);
    if !written.is_empty() {
        feed(&mut t, written);
    }
    let changed = cell_frame(&mut r, &mut t, &mut cells, next());
    // The records first, so a veil kept stale fails here, at the staleness
    // control, before the count says why.
    let mut fresh = made();
    before(&mut fresh, &t);
    after(&mut fresh, &t);
    let mut fresh_backing = GlyphMap::backing();
    let mut fresh_cells = Cells::new(&mut fresh_backing, &t, rows);
    let first = cell_frame(&mut fresh, &mut t, &mut fresh_cells, SetIx(0));
    assert_eq!(
        changed, first,
        "{what}: the cell frame after the change draws a fresh renderer's veils"
    );
    assert_ne!(changed, steady, "{what}: the control, the change is drawn");
    assert!(
        r.veils_composed().value() > composed.value(),
        "{what}: the changed veil composed again"
    );
    let kept = r.veils_composed();
    let again = cell_frame(&mut r, &mut t, &mut cells, next());
    assert_eq!(r.veils_composed(), kept, "{what}: the change composed once");
    assert_eq!(
        again, first,
        "{what}: the next set's frame draws the veils kept, as they stand"
    );
}

/// Whether the change composes a veil: a veil's own change does; the
/// input method's composition, whose records are its own and which no
/// veil reads, keeps the veil beside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Counted {
    Composed,
    Kept,
}

/// The link under the pointer, drawn as a box over `from..=to` of row 3.
fn hover(to: usize) -> LinkHover {
    LinkHover {
        spans: vec![LinkSpan {
            row: Row::from_index(2),
            from: Col::at_index(6),
            to: Col::at_index(to),
        }],
        paint: HoverPaint::Box,
    }
}

/// **The link under the pointer moved over the same row** draws its new
/// fields. Named break: the key never moving, so the kept fields are
/// drawn.
#[test]
fn a_hovered_link_that_changed_is_drawn() {
    a_hovered_link_that_changed_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_hovered_link_that_changed_is_drawn_on_the_cell_path() {
    a_hovered_link_that_changed_is_drawn_on(Path::Cell);
}

fn a_hovered_link_that_changed_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "the link under the pointer",
        Rows::of_index(ROWS),
        |r, _| r.set_link_hover(Some(&hover(12))),
        |r, _| r.set_link_hover(Some(&hover(20))),
        b"",
        Counted::Composed,
    );
}

/// **A cell written under the link under the pointer** is drawn in its
/// field: the field shows the cells' characters, so the key that holds
/// the field moves with them.
#[test]
fn a_cell_written_under_a_hovered_link_is_drawn() {
    a_cell_written_under_a_hovered_link_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_cell_written_under_a_hovered_link_is_drawn_on_the_cell_path() {
    a_cell_written_under_a_hovered_link_is_drawn_on(Path::Cell);
}

fn a_cell_written_under_a_hovered_link_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "a cell under the link under the pointer",
        Rows::of_index(ROWS),
        |r, _| r.set_link_hover(Some(&hover(12))),
        |_, _| {},
        b"\x1b[3;9HZ",
        Counted::Composed,
    );
}

/// **The find field typed into** draws its new text.
#[test]
fn a_find_field_that_changed_is_drawn() {
    a_find_field_that_changed_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_find_field_that_changed_is_drawn_on_the_cell_path() {
    a_find_field_that_changed_is_drawn_on(Path::Cell);
}

fn a_find_field_that_changed_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "the find field",
        Rows::of_index(ROWS),
        |r, t| {
            r.set_find(
                t,
                Some(FindLook {
                    marks: Vec::new(),
                    field: field(ROWS - 1, 0, "Find abc"),
                }),
            )
        },
        |r, t| {
            r.set_find(
                t,
                Some(FindLook {
                    marks: Vec::new(),
                    field: field(ROWS - 1, 0, "Find abd"),
                }),
            )
        },
        b"",
        Counted::Composed,
    );
}

/// The link preview with its bar reading `s` on row 5.
fn link(s: &str) -> LinkLook {
    LinkLook {
        spans: Vec::new(),
        bar: field(4, 2, s),
    }
}

/// **The link preview's bar naming another address** draws it.
#[test]
fn a_link_bar_that_changed_is_drawn() {
    a_link_bar_that_changed_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_link_bar_that_changed_is_drawn_on_the_cell_path() {
    a_link_bar_that_changed_is_drawn_on(Path::Cell);
}

fn a_link_bar_that_changed_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "the link preview's bar",
        Rows::of_index(ROWS),
        |r, _| r.set_link(Some(link("https://example.org/a"))),
        |r, _| r.set_link(Some(link("https://example.org/b"))),
        b"",
        Counted::Composed,
    );
}

/// The hover tip, its one line reading `s` on row 6.
fn tip(s: &str) -> TipLook {
    TipLook {
        lines: vec![field(5, 10, s)],
        area: TipArea {
            top: Row::from_index(5),
            bottom: Row::from_index(6),
            left: Col::at_index(10),
            right: Col::at_index(COLS - 1),
        },
    }
}

/// **The hover tip's line saying something else** draws it.
#[test]
fn a_tip_that_changed_is_drawn() {
    a_tip_that_changed_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_tip_that_changed_is_drawn_on_the_cell_path() {
    a_tip_that_changed_is_drawn_on(Path::Cell);
}

fn a_tip_that_changed_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "the hover tip",
        Rows::of_index(ROWS),
        |r, _| r.set_tip(Some(tip("a word's meaning"))),
        |r, _| r.set_tip(Some(tip("another meaning"))),
        b"",
        Counted::Composed,
    );
}

/// **The notice saying something else** draws it, on its own row below
/// the terminal's.
#[test]
fn a_notice_that_changed_is_drawn() {
    a_notice_that_changed_is_drawn_on(Path::Cpu);
}

/// The same on the cell path (X-1).
#[test]
fn a_notice_that_changed_is_drawn_on_the_cell_path() {
    a_notice_that_changed_is_drawn_on(Path::Cell);
}

fn a_notice_that_changed_is_drawn_on(path: Path) {
    a_changed_veil_is_drawn(
        path,
        "the notice",
        Rows::of_index(ROWS - 1),
        |r, _| r.set_notice(Some(text("config: one key unread"))),
        |r, _| r.set_notice(Some(text("config: two keys unread"))),
        b"",
        Counted::Composed,
    );
}

/// An input method's composition of `s` at row 4, column 3.
fn composition(s: &str) -> Composition {
    Composition {
        row: Row::from_index(3),
        col: Col::at_index(3),
        text: text(s),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    }
}

/// **The input method's composition changed beside a veil composes no
/// veil**: the composition's records are its own, drawn every frame, and
/// no veil's records read it, so the link preview's bar on the
/// composition's row is kept while the composition is drawn as it now
/// stands, pixel for pixel what a fresh renderer draws.
#[test]
fn a_composition_that_changed_composes_no_veil() {
    let bar = || {
        Some(LinkLook {
            spans: Vec::new(),
            bar: field(3, 20, "example.org"),
        })
    };
    a_changed_veil_is_drawn(
        Path::Cpu,
        "the composition",
        Rows::of_index(ROWS),
        |r, _| {
            r.set_link(bar());
            r.set_composition(Some(composition("ni")));
        },
        |r, _| r.set_composition(Some(composition("nih"))),
        b"",
        Counted::Kept,
    );
}

/// The arguments of the call whose text follows its opening parenthesis
/// in `s`, up to its closing one.
fn call_args(s: &str) -> &str {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => return &s[..i],
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    panic!("an unclosed call")
}

/// `args` split at its commas outside brackets, each trimmed, none empty.
fn split_args(args: &str) -> Vec<&str> {
    let (mut out, mut depth, mut from) = (Vec::new(), 0usize, 0);
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(args[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(args[from..].trim());
    out.retain(|a| !a.is_empty());
    out
}

/// **No veil is composed from the input method's composition**, so its
/// key, which holds none, cannot keep a veil stale when the composition
/// moves. Neither what the key is made from (`VeilNow`) nor the key
/// (`VeilKey`) carries one; and in each of the two places a veil is
/// composed -- the CPU path's convert and the cell path's `veils_onto`
/// -- `composition_records` is handed the veil's own field laid as the
/// screen shows it and no cursor block (`None`), and the input method's
/// composition is named nowhere in the call. The controls: two places, and
/// the ten arguments each call takes. Named break: a veil composed with a
/// block read from the composition.
#[test]
fn no_veil_is_composed_from_the_composition() {
    const RENDER: &str = include_str!("../src/renderer.rs");
    for head in ["pub(super) struct VeilNow<'a> {", "    struct VeilKey {"] {
        let at = RENDER.find(head).expect(head) + head.len();
        let body = call_args(&RENDER[at..]);
        assert!(
            !body.to_lowercase().contains("composition"),
            "{head} carries no composition:{body}"
        );
    }
    let open = "veils.records(";
    let sites: Vec<&str> = RENDER
        .match_indices(open)
        .map(|(i, _)| call_args(&RENDER[i + open.len()..]))
        .collect();
    assert_eq!(
        sites.len(),
        2,
        "the control: a veil is composed in two places"
    );
    let compose = "composition_records(";
    for site in sites {
        assert_eq!(site.matches(compose).count(), 1, "one compose:{site}");
        let at = site.find(compose).expect(compose) + compose.len();
        let args = split_args(call_args(&site[at..]));
        assert_eq!(args.len(), 10, "the control, its ten arguments: {args:?}");
        assert_eq!(
            args[2], "(&v.comp, crate::overlay::Laid::OnScreen)",
            "the veil's own field, laid as the screen shows it"
        );
        assert_eq!(args[5], "None", "no cursor block over a veil");
        assert!(
            !site
                .replace(compose, "")
                .to_lowercase()
                .contains("composition"),
            "the composition named nowhere in a veil's compose:{site}"
        );
    }
}
