//! **Every input the kept bidirectional layout depends on is drawn the
//! next frame** (`baud_render::Reorder`, kept a paragraph at a time while
//! no row it read was written). Each case draws a full frame and two
//! incremental ones, checks its control (the operation changes what a
//! fresh renderer draws), checks its isolation against the seal taken just
//! before it (what it wrote and did not), applies it, and draws two
//! incremental frames, each equal to a fresh renderer's full frame, pixels
//! and grounds, with the kept order equal to the fresh renderer's.
//!
//! **And on the cell path** where a case changes the cells a copy of the
//! screen holds (`baud_render::cell_frame::build`, as the app builds it,
//! its sets taken in turn): the frames after the operation are new screens
//! whose sets hold the window and the status line as a fresh layout draws
//! them (`Orders::window_kept` into a new `DrawnRows`,
//! `Orders::status_line`), so a set's key and the renderer's own copy in
//! drawn order are each held to what the layout laid out again with no
//! cell written.

mod support;

use baud_core::{BidiSupport, Owned, Term};
use baud_domain::{
    BytePos, Chan, Codepoint, Col, Cols, PackedCell, ReadLen, Row, RowExtent, Rows, Seal, Video,
    WinSize, WindowAt, Wrap,
};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{Built, SETS, Screens, SetIx};
use baud_render::{
    AppBg, Blend, Candidate, Caret, Composition, Current, Damage, DrawnRows, FaceRole, FaceStyle,
    Field, FindLook, FontSlot, GlyphMap, Ground, Hints, Instance, Joins, Leaving, Lent, LinkLook,
    LinkSpan, MapEntry, Mark, Measure, Orders, Overlay, PictureBg, Pictures, PxSize, Redraw,
    Renderer, Reorder, Resolved, Stack, StyleStamp, Target, TextFace, Theme, WindowLine, WordWrap,
    Written,
};
use support::font;

const COLS: usize = 40;
const ROWS: usize = 10;
const WS: WinSize = WinSize {
    ws_col: 40,
    ws_row: 10,
};

const HIDE: &str = "\x1b[?25l";

const PROMPT: &str = "\x1b]133;A\x07$ ";

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for read in bytes.chunks(4096) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

/// Forty columns of digits and spaces: no strong letter.
fn digits() -> String {
    format!("{}4", "12 ".repeat(13))
}

/// Forty columns whose only strong letters are Hebrew, first.
fn hebrew_row() -> String {
    format!("\u{5d0}\u{5d1}\u{5d2} {}", "1 ".repeat(18))
}

/// Paragraph A on rows 2-3, row 2 digits and spaces laid out right to left
/// by row 3's Hebrew (UAX #9 P2-P3); a Latin row 4; paragraph B, Hebrew
/// wrapped over rows 6-7.
fn two_paragraphs() -> String {
    let b = "\u{5d0}\u{5d1}\u{5d2}\u{5d3} ".repeat(9);
    format!(
        "{HIDE}\x1b[3;1H{}\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}\x1b[5;1Hplain latin\x1b[7;1H{b}",
        digits()
    )
}

/// Rows 3-6 one paragraph whose only strong letter is row 3's Hebrew.
fn four_rows() -> String {
    format!(
        "{HIDE}\x1b[4;1H{}{}{}12 34",
        hebrew_row(),
        digits(),
        digits()
    )
}

/// The faces every case draws with, and on macOS one that shapes Arabic.
fn faces_data() -> Vec<Vec<u8>> {
    vec![font("JetBrainsMono-Regular.ttf")]
}

/// What a renderer of a case is set to beyond the screen: its size, a
/// composition, whether it shows a find mark and a link underline, the
/// page it draws on, and how its faces are stacked.
#[derive(Clone)]
struct Setting {
    size: f32,
    wrap: WordWrap,
    composition: Option<(Row, Col, &'static str)>,
    marks: Marks,
    page: Page,
    faces: Faces,
}

/// **The page a renderer draws on**: opaque, which finds no picture drawn
/// in text, or dimmed below opaque with pictures drawn solid, which finds
/// them (`Theme::finds_pictures`), as the app's default page does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Opaque,
    Dimmed,
}

impl Page {
    fn theme(self) -> Theme {
        match self {
            Page::Opaque => Theme::dark(),
            Page::Dimmed => Theme {
                bg_alpha: Chan::from_u8(56),
                app_bg: AppBg::Dimmed,
                picture_bg: PictureBg::Opaque,
                ..Theme::dark()
            },
        }
    }
}

/// Whether a renderer shows a find mark and a link underline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Marks {
    None,
    Shown,
}

/// **The faces a renderer draws with, and how they are stacked**:
/// `Regular`, the faces a case is handed as a chain of fallbacks
/// (`Stack::new`), no face in a style of its own, so an italic cell is
/// drawn upright from the regular face and SAPV 17 leans it neither way;
/// `RegularItalic`, the app's built Regular and Italic, the Italic in the
/// italic's role (`Stack::with_roles`, as `composition.rs`'s SAPV 17 case
/// stacks them), so an italic cell SAPV 17 slants leans by the Italic's
/// angle (`Stack::lean`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Faces {
    Regular,
    RegularItalic,
}

impl Faces {
    /// The faces' data: the test face for `Regular`, the app's built pair
    /// for `RegularItalic`.
    fn data(self) -> Vec<Vec<u8>> {
        match self {
            Faces::Regular => faces_data(),
            Faces::RegularItalic => ["Regular", "Italic"]
                .iter()
                .map(|name| {
                    std::fs::read(format!(
                        "{}/../baud-app/fonts/built/{name}.ttf",
                        env!("CARGO_MANIFEST_DIR")
                    ))
                    .unwrap()
                })
                .collect(),
        }
    }

    /// `faces` stacked as these faces are.
    fn stack<'f, 'a>(self, faces: &'f [Face<'a>]) -> Stack<'f, 'a> {
        match self {
            Faces::Regular => Stack::new(faces).unwrap(),
            Faces::RegularItalic => Stack::with_roles(
                faces,
                &[FaceRole::Fallback, FaceRole::Style(FaceStyle::Italic)],
            )
            .expect("a regular face and an italic one"),
        }
    }
}

fn made<'f, 'a>(faces: &'f [Face<'a>], s: &Setting, t: &Term<'_>) -> Renderer<'f, 'a> {
    let mut r = Renderer::new(
        s.faces.stack(faces),
        PxSize::of(14.0).unwrap(),
        s.page.theme(),
        Blend::Native,
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
    )
    .unwrap();
    apply(&mut r, s, t);
    r
}

/// The find field on the last row, a mark on row 2's first five columns,
/// and a link underline under columns 3-8 of row 2.
fn looks() -> (FindLook, LinkLook) {
    let field = Field {
        row: Row::from_index(ROWS - 1),
        col: Col::ZERO,
        text: vec![Codepoint::from_char('F')],
        hints: vec![Hints::default()],
        caret: Col::at_index(1),
    };
    let find = FindLook {
        marks: vec![Mark {
            line: WindowLine::of(Row::from_index(2)),
            from: Col::ZERO,
            to: Col::at_index(5),
            current: Current::No,
        }],
        field: field.clone(),
    };
    let link = LinkLook {
        spans: vec![LinkSpan {
            row: Row::from_index(2),
            from: Col::at_index(3),
            to: Col::at_index(9),
        }],
        bar: field,
    };
    (find, link)
}

fn apply(r: &mut Renderer<'_, '_>, s: &Setting, t: &Term<'_>) {
    if (s.size - 14.0).abs() > f32::EPSILON {
        r.rescale(PxSize::of(s.size).unwrap()).unwrap();
    }
    r.set_word_wrap(s.wrap);
    apply_composition(r, s.composition);
    if s.marks == Marks::Shown {
        let (find, link) = looks();
        r.set_find(t, Some(find));
        r.set_link(Some(link));
    }
}

#[derive(Clone, PartialEq)]
struct Drawn {
    pixels: Vec<u32>,
    grounds: Vec<Ground>,
}

fn draw(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    back: Rows,
    px: &mut Vec<u32>,
    damage: Damage,
) -> Drawn {
    r.fit(t);
    let (w, h) = r.pixel_size();
    let n = usize::try_from(w.value() * h.value()).unwrap();
    if px.len() != n {
        *px = vec![0; n];
    }
    {
        let mut target = Target::new(&mut px[..], w, w, h).unwrap();
        r.render_at(t, back, &mut target, damage);
    }
    let mut grounds = Vec::new();
    r.grounds(&mut grounds);
    Drawn {
        pixels: px.clone(),
        grounds,
    }
}

/// A fresh renderer's full frame, and its order after one more frame,
/// once whatever the first frame shaped is read.
fn fresh(faces: &[Face<'_>], s: &Setting, t: &mut Term<'_>, back: Rows) -> (Drawn, Orders) {
    let mut r = made(faces, s, t);
    let mut px = Vec::new();
    let full = draw(&mut r, t, back, &mut px, Damage::Full);
    let _ = draw(&mut r, t, back, &mut px, Damage::Incremental);
    (full, r.bidi().clone())
}

enum Step {
    Bytes(String),
    Wrap(WordWrap),
    Compose(Option<(Row, Col, &'static str)>),
    Rescale(f32),
}

impl Step {
    fn apply(&self, t: &mut Term<'_>, r: &mut Renderer<'_, '_>, s: &mut Setting) {
        match self {
            Step::Bytes(b) => feed(t, b.as_bytes()),
            Step::Wrap(w) => {
                s.wrap = *w;
                r.set_word_wrap(*w);
            }
            Step::Compose(c) => {
                s.composition = *c;
                apply_composition(r, *c);
            }
            Step::Rescale(size) => {
                s.size = *size;
                r.rescale(PxSize::of(*size).unwrap()).unwrap();
            }
        }
    }
}

fn apply_composition(r: &mut Renderer<'_, '_>, c: Option<(Row, Col, &'static str)>) {
    r.set_composition(composition_of(c));
}

/// The composition a case's setting shows, as the app hands it over.
fn composition_of(c: Option<(Row, Col, &'static str)>) -> Option<Composition> {
    c.map(|(row, col, text)| Composition {
        row,
        col,
        text: text.chars().map(Codepoint::from_char).collect(),
        measure: Measure::default(),
        hints: Vec::new(),
        caret: Caret::End,
    })
}

/// How a case's control knows the operation did what it is there for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shows {
    /// The picture changed.
    Drawn,
    /// Nothing: a round trip ends on the picture it began on.
    Same,
}

/// What a case's isolation reads: the seal taken before the operation, the
/// window's physical rows then, and how far it looks back.
struct Was {
    seal: Seal,
    at: WindowAt,
    back: Rows,
}

/// One input the kept layout depends on.
struct Case {
    what: &'static str,
    setup: String,
    back: Rows,
    setting: Setting,
    steps: Vec<Step>,
    shows: Shows,
    alone: fn(&Term<'_>, &Was) -> Result<(), String>,
}

const PLAIN: Setting = Setting {
    size: 14.0,
    wrap: WordWrap::On,
    composition: None,
    marks: Marks::None,
    page: Page::Opaque,
    faces: Faces::Regular,
};

/// The window's rows written after `w`'s seal.
fn written(t: &Term<'_>, w: &Was) -> Vec<usize> {
    let g = t.row_gens(w.back);
    (0..g.window().rows.index())
        .filter(|&y| g.after(Row::from_index(y), &w.seal))
        .collect()
}

/// `Ok` where the isolation `what` holds, else what failed.
fn isolated(holds: Option<()>, what: &str) -> Result<(), String> {
    holds.ok_or_else(|| what.to_string())
}

/// Whether the window shows the physical rows it showed.
fn same_rows(t: &Term<'_>, w: &Was) -> bool {
    t.row_gens(w.back).window() == w.at
}

/// Nothing written since the seal.
fn untouched(t: &Term<'_>, w: &Was) -> Result<(), String> {
    isolated((!t.wrote_since(&w.seal)).then_some(()), "nothing written")
}

/// **One case on the CPU path.**
fn on_the_cpu_path(case: &Case, data: &[Vec<u8>]) {
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, case.setup.as_bytes());
    let back = case.back;
    let mut s = case.setting.clone();
    let mut r = made(&faces, &s, &t);
    let mut px = Vec::new();
    draw(&mut r, &mut t, back, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
    }
    let (before, _) = fresh(&faces, &s, &mut t, back);
    let was = Was {
        seal: t.sealed(),
        at: t.row_gens(back).window(),
        back,
    };
    for (k, step) in case.steps.iter().enumerate() {
        if k > 0 {
            draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        }
        step.apply(&mut t, &mut r, &mut s);
    }
    let (after, _) = fresh(&faces, &s, &mut t, back);
    match case.shows {
        Shows::Drawn => assert!(
            after.pixels != before.pixels,
            "{}: the control, the operation is drawn",
            case.what
        ),
        Shows::Same => assert!(
            after.pixels == before.pixels,
            "{}: the control, the round trip ends where it began",
            case.what
        ),
    }
    if let Err(e) = (case.alone)(&t, &was) {
        panic!("{}: the isolation, {e}", case.what);
    }
    let mut wrong = Vec::new();
    for frame in ["the frame after", "the frame after that"] {
        let now = draw(&mut r, &mut t, back, &mut px, Damage::Incremental);
        let (full, order) = fresh(&faces, &s, &mut t, back);
        if now.pixels != full.pixels {
            wrong.push(format!("{frame}: pixels"));
        }
        if now.grounds != full.grounds {
            wrong.push(format!("{frame}: grounds"));
        }
        if *r.bidi() != order {
            wrong.push(format!("{frame}: the order"));
        }
    }
    assert!(wrong.is_empty(), "{} drawn stale: {wrong:?}", case.what);
}

fn run(case: Case) {
    on_the_cpu_path(&case, &case.setting.faces.data());
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
}

impl<'m> Cells<'m> {
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let stride = t.stride().index();
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (ROWS + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().len()]),
            overlay_mem: core::array::from_fn(|_| vec![0u8; COLS * (ROWS + 1)]),
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

/// [`cell_frame_on`] into set `at`, which is free.
fn cell_frame(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    shown: (Rows, &Option<Composition>),
    cell: &mut Cells<'_>,
    at: SetIx,
) -> Built {
    cell_frame_on(r, t, shown, cell, Some(at)).expect("the set is free")
}

/// **The cell frame `r` builds into set `at`**, the window `back` lines
/// into the history, as the app's `present` builds it with `composition`
/// shown, which `r` was handed too, and no prediction or slide, the
/// frame's writes sealed first. With no set free (`at` none) the build
/// returns after its layout, and nothing is built.
fn cell_frame_on(
    r: &mut Renderer<'_, '_>,
    t: &mut Term<'_>,
    (back, composition): (Rows, &Option<Composition>),
    cell: &mut Cells<'_>,
    at: Option<SetIx>,
) -> Option<Built> {
    let seal = t.seal_writes();
    let t = &*t;
    r.fit(t);
    r.set_images(t, back);
    r.splash_yields_to(t, Px::of(0));
    r.place_marks(t, back, Leaving::None, Px::of(0));
    r.place_lights(t);
    let inputs = CellInputs {
        back,
        predictions: core::iter::empty(),
        live: None,
        composition,
        highlight: None,
        leaving: Leaving::None,
        pan: Px::of(0),
    };
    let mut copies = cell.copies_mem.each_mut().map(|c| &mut c[..]);
    let mut styles = cell.styles_mem.each_mut().map(|c| &mut c[..]);
    let mut overlay = cell.overlay_mem.each_mut().map(|c| &mut c[..]);
    build(
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
            lent_cells: &mut cell.lent_cells,
            lent_used: &mut cell.lent_used,
        },
        &mut cell.used,
        move || at,
    )
    .map(|f| f.built)
}

/// **What a set's copy holds as a fresh layout draws it**: the window
/// `back` lines into the history in drawn order (`Orders::window_kept`
/// into a new `DrawnRows`), then the status line's row
/// (`Orders::status_line`), each at the copy's pitch, the columns past the
/// window's left out. A composition shown on the live window is read as
/// its text written, in the rendition of the cell it starts on, as the
/// renderer writes it over the window.
fn fresh_copy(t: &Term<'_>, s: &Setting, back: Rows) -> Vec<Vec<PackedCell>> {
    let mut o = Reorder::new();
    o.set_word_wrap(s.wrap);
    match composition_of(s.composition).filter(|_| t.looking_back(back).index() == 0) {
        Some(c) => {
            let vp = t.view(Rows::ZERO);
            let style = vp
                .row(c.row)
                .get(c.col.index())
                .map_or(PackedCell::EMPTY.style, |cell| cell.style);
            let mut written = Written::default();
            written.write(&vp, c.row, c.col, &c.written(c.width_on(&vp), style));
            o.lay_out_written(t, back, Joins::Seen, &written);
        }
        None => o.lay_out(t, back),
    }
    let mut drawn = DrawnRows::default();
    let view = o.orders().window_kept(t, back, &mut drawn);
    let (cols, rows) = (view.cols().index(), view.rows().index());
    let mut out: Vec<Vec<PackedCell>> = (0..rows)
        .map(|y| view.row(Row::from_index(y))[..cols].to_vec())
        .collect();
    if let Some(line) = t.status_line() {
        let mut drawn = Vec::new();
        let line = o
            .orders()
            .status_line(Row::from_index(rows), line, &mut drawn);
        out.push(line.cells[..cols.min(line.cells.len())].to_vec());
    }
    out
}

/// What set `at`'s copy holds, row for row, as [`fresh_copy`] reads it.
fn held_copy(
    cell: &Cells<'_>,
    t: &Term<'_>,
    at: SetIx,
    rows: &[Vec<PackedCell>],
) -> Vec<Vec<PackedCell>> {
    let stride = t.stride().index();
    let copy = &cell.copies_mem[at.0];
    rows.iter()
        .enumerate()
        .map(|(y, r)| copy[y * stride..y * stride + r.len()].to_vec())
        .collect()
}

/// **One case on the cell path**: a frame into each set in turn until
/// every set was built from, and a steady one, the last screen -- the
/// control -- then the operation and two frames into the sets in turn,
/// each a new screen or the last as the copy says, whose set holds the
/// window and the status line as a fresh layout draws them.
fn on_the_cell_path(case: &Case, data: &[Vec<u8>]) {
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, case.setup.as_bytes());
    let back = case.back;
    let mut s = case.setting.clone();
    let mut r = made(&faces, &s, &t);
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut order = (0..).map(|k| SetIx(k % SETS));
    let mut next = || order.next().expect("a set");
    for _ in 0..=SETS {
        let shown = composition_of(s.composition);
        cell_frame(&mut r, &mut t, (back, &shown), &mut cells, next());
    }
    let shown = composition_of(s.composition);
    if cell_frame(&mut r, &mut t, (back, &shown), &mut cells, next()) != Built::Last {
        panic!(
            "{}: the control, a steady frame is the last screen",
            case.what
        );
    }
    let before = fresh_copy(&t, &s, back);
    for (k, step) in case.steps.iter().enumerate() {
        if k > 0 {
            let shown = composition_of(s.composition);
            cell_frame(&mut r, &mut t, (back, &shown), &mut cells, next());
        }
        step.apply(&mut t, &mut r, &mut s);
    }
    let after = fresh_copy(&t, &s, back);
    assert!(
        before != after,
        "{}: the control, the operation moves what a copy holds",
        case.what
    );
    let mut wrong = Vec::new();
    for frame in ["the frame after", "the frame after that"] {
        let at = next();
        let shown = composition_of(s.composition);
        let built = cell_frame(&mut r, &mut t, (back, &shown), &mut cells, at);
        let now = held_copy(&cells, &t, at, &after);
        if frame == "the frame after" && built != Built::New {
            wrong.push(format!("{frame}: built from the last screen"));
        }
        for (y, (held, fresh)) in now.iter().zip(&after).enumerate() {
            if held != fresh {
                wrong.push(format!("{frame}: row {y}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} on the cell path, drawn stale: {wrong:?}",
        case.what
    );
}

/// [`run`], and on the cell path too.
fn run_both(case: Case) {
    let data = case.setting.faces.data();
    on_the_cpu_path(&case, &data);
    on_the_cell_path(&case, &data);
}

/// **S1: a row another row of its paragraph orders is drawn again** (UAX
/// #9 P2-P3): row 2, digits and spaces, laid out right to left by row 3's
/// Hebrew; row 3's Hebrew written over in Latin. The isolation: row 3
/// alone written. Named breaks: only the written row laid out again, which
/// keeps row 2 reversed; and on the cell path, row 2, laid out again with
/// none of its cells written, kept by a set's key or by the renderer's copy
/// in drawn order. Its W7 twin is `stale_rows.rs`'s
/// `a_row_another_row_reorders_is_drawn_again`.
#[test]
fn a_row_its_paragraphs_first_strong_letter_orders_is_drawn_again() {
    run_both(Case {
        what: "the paragraph's first strong letter",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[4;1Habc de".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
}

/// **S2: a split paragraph's lower part is drawn in its own order**: rows
/// 3-6 one paragraph, its only strong letter row 3's Hebrew, ended at row
/// 4 by a line feed from it. The isolation: row 4 alone written, and it no
/// longer continues. Named break: a unit laid out again only when a row of
/// its own was written, which keeps rows 5-6 reversed.
#[test]
fn a_split_paragraphs_lower_part_is_drawn_in_its_own_order() {
    run_both(Case {
        what: "a split paragraph",
        setup: four_rows(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[5;1H\n".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![4]
                    && t.view(w.back).wrapped(Row::from_index(4)) == Wrap::Ended)
                    .then_some(()),
                "row 4 alone written, and ended",
            )
        },
    });
}

/// **S3: the line above the window a paragraph continues from is read**:
/// the window's first row continues a paragraph whose only strong letter is
/// Hebrew on the line above it, in the history, which `CSI ? 3 J` erases in
/// place. The isolation: no window row written, the history line written,
/// and still held. Named break: the rows above not read.
#[test]
fn a_paragraphs_line_above_the_window_is_read() {
    run_both(Case {
        what: "a line above the window",
        setup: format!(
            "{HIDE}\x1b[1;1H{}{}12 34\x1b[10;1H\n",
            hebrew_row(),
            digits()
        ),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[?3J".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            let one = Rows::of_index(1);
            isolated(
                (written(t, w).is_empty()
                    && t.looking_back(one) == one
                    && t.row_gens(one).after(Row::ZERO, &w.seal))
                .then_some(()),
                "no window row written, the line above written in place",
            )
        },
    });
}

/// **S4: the line below a window held back a paragraph runs onto is
/// read**: the window two lines back, its last row's paragraph running
/// onto the row below it, whose Hebrew is its only strong letter; that row
/// written Latin. The isolation: no window row written, the row below it
/// written. Named break: the rows below not read.
#[test]
fn a_paragraphs_line_below_the_window_is_read() {
    run_both(Case {
        what: "a line below the window",
        setup: format!(
            "{HIDE}\x1b[10;1H\n\n\n\x1b[8;1H{}\u{5d0}\u{5d1}\u{5d2}",
            digits()
        ),
        back: Rows::of_count(2),
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[9;1Habc".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty()
                    && t.row_gens(Rows::of_count(1))
                        .after(Row::from_index(ROWS - 1), &w.seal))
                .then_some(()),
                "no window row written, the row below it written",
            )
        },
    });
}

/// **S5: a scroll under a window held back is drawn**: the window five
/// lines back over rows that alternate Hebrew and Latin; a line feed at the
/// live bottom moves it one physical row and writes none of the rows it
/// then shows. Named break: the window's physical rows out of the key.
#[test]
fn a_scroll_under_a_held_window_is_drawn() {
    let mut setup = String::from(HIDE);
    for i in 0..20 {
        match i % 2 {
            0 => setup.push_str(&format!("\u{5d0}\u{5d1} 12 ab {i}\r\n")),
            _ => setup.push_str(&format!("line {i}\r\n")),
        }
    }
    run_both(Case {
        what: "a scroll under a held window",
        setup,
        back: Rows::of_count(5),
        setting: PLAIN,
        steps: vec![Step::Bytes("\r\n".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty() && !same_rows(t, w)).then_some(()),
                "no row of the new window written, the window moved",
            )
        },
    });
}

/// **S6: each line property a row's mark holds is drawn**: DECRLM and a
/// letter, and DECDWL, on the Latin row; `?2501` reset at paragraph A's
/// first position, which with no path selected lays A out left to right
/// outright. The isolation: a window row written by each. Named break: an
/// answer from the key alone, no row's generation read.
#[test]
fn each_line_property_is_drawn() {
    let steps = [
        ("DECRLM and a letter", "\x1b[5;20H\x1b[?34hx\x1b[?34l"),
        ("DECDWL", "\x1b[5;1H\x1b#6"),
        ("?2501 reset at a paragraph's start", "\x1b[3;1H\x1b[?2501l"),
    ];
    for (what, bytes) in steps {
        run(Case {
            what,
            setup: two_paragraphs(),
            back: Rows::ZERO,
            setting: PLAIN,
            steps: vec![Step::Bytes(bytes.into())],
            shows: Shows::Drawn,
            alone: |t, w| {
                isolated(
                    (!written(t, w).is_empty()).then_some(()),
                    "a window row written",
                )
            },
        });
    }
}

/// **S7: BDSM, which writes no window row, is drawn**: reset over a screen
/// laid out right to left, and set over one laid out with it reset. The
/// isolation: the mode changed and no window row was written (a feed
/// borrows the screen to write, so a surface reads as written). No break
/// of the key reaches this case alone: here a change of BDSM also moves
/// whether joins are read, or, once a joining letter is shaped, the pass
/// (`Reorder::keep_displayed` with it reset), so `bidi_kept.rs`'s U6 holds
/// BDSM's place in the key.
#[test]
fn bdsm_is_drawn() {
    run_both(Case {
        what: "BDSM reset",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[8l".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (t.bidi_support() == BidiSupport::Explicit && written(t, w).is_empty())
                    .then_some(()),
                "BDSM reset, no window row written",
            )
        },
    });
    run_both(Case {
        what: "BDSM set",
        setup: format!("\x1b[8l{}", two_paragraphs()),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[8h".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (t.bidi_support() == BidiSupport::Implicit && written(t, w).is_empty())
                    .then_some(()),
                "BDSM set, no window row written",
            )
        },
    });
}

/// **S8: the status line's own layout follows its plane**: a host-written
/// status line holding Hebrew, written Latin under DECSASD. The isolation:
/// its plane written, no window row. Named break: the status line kept on
/// its key alone.
#[test]
fn the_status_lines_layout_follows_its_plane() {
    let setup = format!(
        "{}\x1b[2$~\x1b[1$}}\u{5d0}\u{5d1}\u{5d2} 12 ab\x1b[0$}}",
        two_paragraphs()
    );
    run_both(Case {
        what: "the status line written",
        setup,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[1$}\rxyz\x1b[0$}".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty() && t.status_gen().is_some_and(|g| g.after(&w.seal)))
                    .then_some(()),
                "the status line written, no window row",
            )
        },
    });
}

/// **D4: the status line's drawn order follows its layout where its plane
/// was not written**: a status line whose path SCP set right to left under
/// DECSASD, holding Hebrew then Latin, is drawn reversed whole with BDSM
/// reset and with its Latin run left to right with it set; BDSM set. The
/// isolation: no window row and not the status line's plane written. Named
/// break: the status line's copy in drawn order kept on its plane's
/// generation alone, not its row of the maps.
#[test]
fn the_status_lines_order_follows_bdsm() {
    let setup = format!(
        "\x1b[8l{}\x1b[2$~\x1b[1$}}\x1b[2 k\u{5d0}\u{5d1}\u{5d2} 12 ab\x1b[0$}}",
        two_paragraphs()
    );
    run_both(Case {
        what: "the status line under BDSM set",
        setup,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[8h".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w).is_empty() && t.status_gen().is_some_and(|g| !g.after(&w.seal)))
                    .then_some(()),
                "no window row and not the status line written",
            )
        },
    });
}

/// **D6: a row's mirrored cells follow its paragraph's direction**: row 2
/// forty N-ARY SUMMATIONs, Bidi_Mirrored with no mirroring glyph, so drawn
/// flipped at a right-to-left level, soft-wrapped into row 3's Hebrew;
/// row 3 written Latin. Row 2's cells, the same character either way
/// round, are drawn at the same columns in either order: only its mirrors
/// moved. The isolation: row 3 alone written. Named break: the conversion
/// asking no row whether its maps were laid out again since it drew them.
#[test]
fn a_rows_mirrors_follow_its_paragraph_with_no_cell_of_it_written() {
    run(Case {
        what: "the mirrors of a row",
        setup: format!(
            "{HIDE}\x1b[3;1H{}\u{5d0}\u{5d1}\u{5d2}",
            "\u{2211}".repeat(COLS)
        ),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[4;1Habc".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
}

/// Row `y` of a fresh renderer's full frame of `bytes` under `s`, drawn
/// with `faces`.
fn fresh_row(faces: &[Face<'_>], s: &Setting, bytes: &str, y: usize) -> Vec<u32> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes.as_bytes());
    let mut r = made(faces, s, &t);
    let drawn = draw(&mut r, &mut t, Rows::ZERO, &mut Vec::new(), Damage::Full);
    let w = usize::try_from(r.pixel_size().0.value()).unwrap();
    let h = usize::try_from(r.metrics().cell_h.value()).unwrap();
    drawn.pixels[y * h * w..(y + 1) * h * w].to_vec()
}

/// **D7: a row's slanted cells follow its paragraph's direction**: row 2
/// forty vertical lines, neither mirrored nor strong, written italic under
/// SAPV 17, which slants an italic cell
/// left at a right-to-left level (`[[source: ecma48]]` 8.3.108),
/// soft-wrapped into row 3's Hebrew; row 3 written Latin. Row 2's cells
/// are the same at every column in either order: only its slants moved.
/// Drawn with the built Regular and Italic (`Faces::RegularItalic`), so an
/// italic cell has a face that leans. The isolation: row 3 alone written.
/// The controls, on the case's own setting: its stack leans an italic of
/// the primary face, and a fresh frame's row 2 differs between row 3
/// Hebrew and row 3 Latin, where with faces that lean nothing it is the
/// same (so what is compared is row 2 alone). Named break: the conversion
/// asking no row whether its maps were laid out again since it drew them.
#[test]
fn a_rows_slants_follow_its_paragraph_with_no_cell_of_it_written() {
    let setup = format!(
        "{HIDE}\x1b[3;1H\x1b[17 ]\x1b[3m{}\x1b[0m\x1b[0 ]\u{5d0}\u{5d1}\u{5d2}",
        "|".repeat(COLS)
    );
    let write = "\x1b[4;1Habc";
    let slanted = Setting {
        faces: Faces::RegularItalic,
        ..PLAIN
    };
    let data = slanted.faces.data();
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let italic = TextFace {
        style: FaceStyle::Italic,
        font: FontSlot::PRIMARY,
    };
    assert!(
        slanted.faces.stack(&faces).lean(italic).is_some(),
        "the control: the case's faces lean an italic of the primary face"
    );
    let latin = format!("{setup}{write}");
    let upright_data = PLAIN.faces.data();
    let upright: Vec<Face<'_>> = upright_data
        .iter()
        .map(|d| Face::parse(d).unwrap())
        .collect();
    assert!(
        fresh_row(&upright, &PLAIN, &setup, 2) == fresh_row(&upright, &PLAIN, &latin, 2),
        "the control's band: with faces that lean nothing, row 2 is drawn alike under row 3 \
         Hebrew and row 3 Latin, so the band holds none of row 3"
    );
    assert!(
        fresh_row(&faces, &slanted, &setup, 2) != fresh_row(&faces, &slanted, &latin, 2),
        "the control: row 2 drawn otherwise under row 3 Hebrew than under row 3 Latin"
    );
    run(Case {
        what: "the slants of a row",
        setup,
        back: Rows::ZERO,
        setting: slanted,
        steps: vec![Step::Bytes(write.into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
}

/// **S10: the join reach is part of what a layout keeps**: a joined Arabic
/// paragraph laid out with no reach, then with a reach of two, nothing
/// written between. The control: a fresh layout's maps differ between the
/// two reaches. Named break: the reach out of the key.
#[test]
fn the_join_reach_is_kept_by() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        "\u{628}\u{628}\u{628} \u{628}\u{628}\u{628}".as_bytes(),
    );
    let laid = |reach: u32, t: &Term<'_>| {
        let mut o = Reorder::new();
        o.set_reach(baud_font::bytes::Count::of(reach));
        o.lay_out(t, Rows::ZERO);
        o.orders().clone()
    };
    assert!(
        laid(0, &t) != laid(2, &t),
        "the control: the reach is drawn"
    );
    let mut o = Reorder::new();
    let _ = t.seal_writes();
    o.lay_out(&t, Rows::ZERO);
    let seal = t.sealed();
    let _ = t.seal_writes();
    o.set_reach(baud_font::bytes::Count::of(2u32));
    o.lay_out(&t, Rows::ZERO);
    assert!(!t.wrote_since(&seal), "the isolation: nothing written");
    assert!(*o.orders() == laid(2, &t), "the kept maps at a new reach");
}

/// **S11: a display held is drawn when it is made again or let go**: word
/// wrap on over a right-to-left paragraph the display re-breaks; a cell of
/// it written, which re-breaks it elsewhere, and, separately, word wrap
/// turned off. Named break: the display out of the key.
#[test]
fn a_held_display_is_drawn_when_made_again_or_let_go() {
    let words = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} ".repeat(10);
    let setup = format!("{HIDE}\x1b[3;1H{words}\x1b[10;1H{PROMPT}");
    run_both(Case {
        what: "the display made again",
        setup: setup.clone(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[3;5HQQQQQQQQ".into())],
        shows: Shows::Drawn,
        alone: |t, w| isolated((written(t, w).contains(&2)).then_some(()), "row 2 written"),
    });
    run_both(Case {
        what: "the display let go",
        setup,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Wrap(WordWrap::Off)],
        shows: Shows::Drawn,
        alone: untouched,
    });
}

/// **S12: a composition over a paragraph is laid out while shown, and the
/// paragraph again the frame after**: row 3's Hebrew written over by a
/// composition for a frame, then none. The isolation: nothing written.
/// The composition shown on the cell path too, its copy held to a fresh
/// layout's over the composition's text written (`fresh_copy`); its let
/// go ends on the picture it began on, which that path's control, a copy
/// that moved, does not take. Named breaks: the rows written over at the
/// last layout not laid out again; and a copy kept whole while the maps
/// moved with nothing written.
#[test]
fn a_composition_over_a_paragraph_is_laid_out_and_let_go() {
    let over = Some((Row::from_index(3), Col::ZERO, "abcdef"));
    run_both(Case {
        what: "a composition shown",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Compose(over)],
        shows: Shows::Drawn,
        alone: untouched,
    });
    run(Case {
        what: "a composition let go",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Compose(over), Step::Compose(None)],
        shows: Shows::Same,
        alone: untouched,
    });
}

/// **S13: what a row's renditions carry is drawn**: SAPV 2 over paragraph
/// A's digits, and an SDS opened and closed on the Latin row. A variant
/// and a string are each begun first, on row 9 (`begun`), so the layout's
/// key does not move with the operation and the row's generation is what
/// finds it. The isolation: the row written. Named break: an answer from
/// the key alone.
#[test]
fn what_a_rows_renditions_carry_is_drawn() {
    let begun = format!(
        "{}\x1b[10;1H\x1b[2 ]9\x1b[0 ] \x1b[1]z\x1b[0]",
        two_paragraphs()
    );
    run(Case {
        what: "SAPV over the digits",
        setup: begun.clone(),
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[3;1H\x1b[2 ]12 34\x1b[0 ]".into())],
        shows: Shows::Drawn,
        alone: |t, w| isolated((written(t, w).contains(&2)).then_some(()), "row 2 written"),
    });
    run(Case {
        what: "a string on the Latin row",
        setup: begun,
        back: Rows::ZERO,
        setting: PLAIN,
        steps: vec![Step::Bytes("\x1b[5;1H\x1b[1]xyz\x1b[0]".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![4]).then_some(()),
                "row 4 alone written",
            )
        },
    });
}

/// **S14: the find marks and the link underline follow the order and the
/// metrics**: a find mark and a link underline on row 2, which row 3's
/// Hebrew written Latin puts in its own order; and, separately, the faces
/// set at another size with nothing written. Named breaks: the marks kept
/// whatever the layout answered, and kept without the metrics.
#[test]
fn the_marks_follow_the_order_and_the_metrics() {
    let marked = Setting {
        marks: Marks::Shown,
        ..PLAIN
    };
    run(Case {
        what: "the marks over a row put in another order",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: marked.clone(),
        steps: vec![Step::Bytes("\x1b[4;1Habc de".into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
    run(Case {
        what: "the marks at another size",
        setup: two_paragraphs(),
        back: Rows::ZERO,
        setting: marked,
        steps: vec![Step::Rescale(16.0)],
        shows: Shows::Drawn,
        alone: untouched,
    });
}

/// **With BDSM reset a row joins alone**: an Arabic word soft-wrapped over
/// rows 2 and 3, and a letter written in row 3 only. Row 2's forms are a
/// fresh renderer's: each row is a unit of its own with the mode reset, and
/// its joins read its own cells. The isolation: row 3 alone written. Named
/// break: the rejoin limited to the written rows; this case stays green
/// under it where a row's joins read only its own cells, as the control
/// that they do. On Courier New, macOS's, whose `GSUB` draws the forms.
#[cfg(target_os = "macos")]
#[test]
fn with_bdsm_reset_a_word_wrapped_over_two_rows_joins_each_alone() {
    let courier = std::fs::read("/System/Library/Fonts/Supplemental/Courier New.ttf")
        .expect("Courier New, which macOS ships");
    let data = vec![font("JetBrainsMono-Regular.ttf"), courier];
    on_the_cpu_path(
        &Case {
            what: "a word wrapped over two rows",
            setup: format!("{HIDE}\x1b[8l\x1b[3;37H{}", "\u{628}".repeat(6)),
            back: Rows::ZERO,
            setting: PLAIN,
            steps: vec![Step::Bytes("\x1b[4;1H\u{627}".into())],
            shows: Shows::Drawn,
            alone: |t, w| {
                isolated(
                    (written(t, w) == vec![3]).then_some(()),
                    "row 3 alone written",
                )
            },
        },
        &data,
    );
}

/// **The leaving row's layout follows a write** (`Reorder::lay_out_top`,
/// the layout a smooth scroll's slide draws its leaving row by, and which
/// `Renderer::swap_leaving_layout` exchanges whole with the window's): the
/// first paragraph of the window one line up, which runs from the history
/// onto the screen, written over on the screen between two layouts. The
/// control: the maps differ. Named break: an answer from the key alone.
#[test]
fn the_leaving_rows_layout_follows_a_write() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        format!("\x1b[1;1H{}{}12 34\x1b[10;1H\n", hebrew_row(), digits()).as_bytes(),
    );
    let up = Rows::of_index(1);
    let top = |t: &Term<'_>| {
        let mut o = Reorder::new();
        o.lay_out_top(t, up, Joins::Seen);
        o.orders().clone()
    };
    let mut o = Reorder::new();
    let _ = t.seal_writes();
    o.lay_out_top(&t, up, Joins::Seen);
    let before = top(&t);
    feed(&mut t, b"\x1b[1;1Habc def");
    let _ = t.seal_writes();
    o.lay_out_top(&t, up, Joins::Seen);
    let after = top(&t);
    assert!(before != after, "the control: the write moves the maps");
    assert!(*o.orders() == after, "the kept maps follow the write");
}

/// **D8: a picture drawn in text over a row drawn in another order is
/// found where the row is drawn**: row 2 twenty cells each on a background
/// of its own, a picture drawn in text (`baud_render::Pictures`), then
/// digits and spaces, soft-wrapped into row 3's Hebrew, so row 2 is drawn
/// right to left and the picture at its right; row 3's first word written
/// Latin, which draws row 2 in its own order and the picture at its left,
/// and leaves row 3's last word Hebrew, so a row of the window is still
/// drawn in another order. On a dimmed page, which finds pictures and
/// draws them solid: the CPU path's picture finder reads the window in
/// drawn order (`Orders::window_kept`), the renderer's kept copy while any
/// row of the window is drawn in another order. The isolation: row 3
/// alone written. The controls: the page finds pictures, row 2 is drawn in
/// another order, the picture is found, and after the write row 2 is in
/// its own order, row 3 in another, and the window holds a reordered row.
/// Named break: the renderer's copy in drawn order kept without the
/// layout's seal.
#[test]
fn a_picture_over_a_row_drawn_in_another_order_is_found_where_it_is_drawn() {
    let picture: String = (0..20u8)
        .map(|i| format!("\x1b[48;2;{};{};90m ", 40 + i * 10, 200 - i * 5))
        .collect();
    let setup = format!(
        "{HIDE}\x1b[3;1H{picture}\x1b[0m{}\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4}",
        "1 ".repeat(10)
    );
    let dimmed = Setting {
        page: Page::Dimmed,
        ..PLAIN
    };
    assert!(
        dimmed.page.theme().finds_pictures(),
        "the control: the page finds pictures"
    );
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(
        o.orders().reordered(Row::from_index(2)),
        "the control: row 2 drawn in another order"
    );
    let mut fresh = DrawnRows::default();
    let drawn = o.orders().window_kept(&t, Rows::ZERO, &mut fresh);
    assert_eq!(
        Pictures::new().find(&t, &drawn, Video::Normal).value(),
        20,
        "the control: the picture found"
    );
    let write = "\x1b[4;1Habc";
    feed(&mut t, write.as_bytes());
    let mut o = Reorder::new();
    o.lay_out(&t, Rows::ZERO);
    assert!(
        !o.orders().reordered(Row::from_index(2))
            && o.orders().reordered(Row::from_index(3))
            && o.orders().any(),
        "the control: after the write row 2 is in its own order, row 3 in another, and the \
         window holds a reordered row"
    );
    run(Case {
        what: "a picture over a row drawn in another order",
        setup,
        back: Rows::ZERO,
        setting: dimmed,
        steps: vec![Step::Bytes(write.into())],
        shows: Shows::Drawn,
        alone: |t, w| {
            isolated(
                (written(t, w) == vec![3]).then_some(()),
                "row 3 alone written",
            )
        },
    });
}

/// **A row laid out again in its own order, as it was, is not repainted**
/// (the CPU path): one paragraph over rows 2 and 3, row 2 all Latin
/// soft-wrapped onto row 3, Latin then Hebrew, so row 2 is in its own order
/// and row 3 in another; a write in row 3, the cursor put back, lays both
/// out again, and the frame after repaints row 3 alone, its pixels a
/// fresh renderer's. The controls: row 2 in its own order, row 3 in
/// another, and the frame lays out both rows again (`Renderer::bidi_work`).
/// Named break: every row a relay lifted stamped at its end, which
/// repaints row 2 whole.
#[test]
fn a_row_laid_out_again_in_its_own_order_is_not_repainted() {
    let data = faces_data();
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        format!(
            "{HIDE}\x1b[3;1H{}xyz \u{5d0}\u{5d1}\u{5d2}",
            "abc ".repeat(10)
        )
        .as_bytes(),
    );
    let mut r = made(&faces, &PLAIN, &t);
    let mut px = Vec::new();
    draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Full);
    for _ in 0..2 {
        draw(&mut r, &mut t, Rows::ZERO, &mut px, Damage::Incremental);
    }
    assert!(
        !r.bidi().reordered(Row::from_index(2)) && r.bidi().reordered(Row::from_index(3)),
        "the control: row 2 in its own order, row 3 in another"
    );
    feed(&mut t, b"\x1b7\x1b[4;1Hpq\x1b8");
    r.fit(&t);
    let (w, h) = r.pixel_size();
    let f = {
        let mut target = Target::new(&mut px[..], w, w, h).unwrap();
        r.render_at(&mut t, Rows::ZERO, &mut target, Damage::Incremental)
    };
    assert_eq!(
        r.bidi_work().laid.value(),
        2,
        "the control: rows 2 and 3 laid out again"
    );
    assert_eq!(f.rows_dirty.value(), 1, "row 3 alone repainted");
    let (full, _) = fresh(&faces, &PLAIN, &mut t, Rows::ZERO);
    assert!(px == full.pixels, "the frame is a fresh renderer's");
}

/// **A cell frame with no free set leaves every row's joins owed**
/// (`cell_frame::build`, as the app builds it, with BDSM reset): Arabic on
/// row 4, joined; a composition over its letters in Latin for two frames,
/// which reads the row as unjoined; the composition let go and a frame
/// with no set free, which returns after its layout answered that every
/// row's joins be found again; then a frame into a set. That frame finds
/// every row's joins again (`Renderer::bidi_work`), so row 4 is joined as
/// a fresh layout joins it and the set's copy is a fresh layout's; the
/// frame after keeps them, finding fewer. Row 4's cells are the same in
/// every copy, so no compare finds it changed: only the answer, owed,
/// finds its joins again. The controls: the joins open, row 4 joined, then
/// read as its Latin under the composition, nothing built with no set
/// free, and a fresh layout joins row 4. Named breaks: the key kept when
/// the answer is given, not when its rejoin ends, which finds row 4's
/// joins on no frame; and a rejoin of no row ended above `building()?`,
/// which leaves the answer given on every frame.
#[test]
fn a_cell_frame_with_no_free_set_leaves_every_rows_joins_owed() {
    let data = faces_data();
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        format!("{HIDE}\x1b[8l\x1b[5;1H\u{628}\u{64a}\u{62a}").as_bytes(),
    );
    let four = Row::from_index(4);
    let mut r = made(&faces, &PLAIN, &t);
    // A composition that holds a joining letter opens the joins, whatever
    // the faces draw.
    apply_composition(
        &mut r,
        Some((Row::from_index(8), Col::ZERO, "\u{628}\u{628}")),
    );
    apply_composition(&mut r, None);
    assert_eq!(r.joins_seen(), Joins::Seen, "the control: the joins open");
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut order = (0..).map(|k| SetIx(k % SETS));
    let mut next = || order.next().expect("a set");
    let none: Option<Composition> = None;
    for _ in 0..=SETS {
        cell_frame(&mut r, &mut t, (Rows::ZERO, &none), &mut cells, next());
    }
    assert!(
        r.bidi().row_joins(four).is_some(),
        "the control: row 4 joined"
    );
    let latin = Some((four, Col::ZERO, "abc"));
    apply_composition(&mut r, latin);
    let shown = composition_of(latin);
    for _ in 0..2 {
        cell_frame(&mut r, &mut t, (Rows::ZERO, &shown), &mut cells, next());
    }
    assert!(
        r.bidi().row_joins(four).is_none(),
        "the control: row 4 read as its Latin"
    );
    apply_composition(&mut r, None);
    assert!(
        cell_frame_on(&mut r, &mut t, (Rows::ZERO, &none), &mut cells, None).is_none(),
        "the control: no set free, nothing built"
    );
    let at = next();
    cell_frame(&mut r, &mut t, (Rows::ZERO, &none), &mut cells, at);
    let rows = u64::from(WS.ws_row);
    assert_eq!(
        r.bidi_work().tested.value(),
        rows,
        "the frame after one with no set free finds every row's joins again"
    );
    let mut fresh = Reorder::new();
    fresh.lay_out(&t, Rows::ZERO);
    assert!(
        fresh.orders().row_joins(four).is_some(),
        "the control: a fresh layout joins row 4"
    );
    assert!(
        r.bidi().row_joins(four).is_some(),
        "row 4 joined, as a fresh layout joins it"
    );
    let after = fresh_copy(&t, &PLAIN, Rows::ZERO);
    assert!(
        held_copy(&cells, &t, at, &after) == after,
        "the set's copy is a fresh layout's"
    );
    cell_frame(&mut r, &mut t, (Rows::ZERO, &none), &mut cells, next());
    let found = r.bidi_work().tested.value();
    assert!(
        found < rows,
        "the frame after that keeps the joins: {found} of {rows} rows found again"
    );
}

/// **One way a paragraph is laid out again as it was**, as
/// [`converts_no_row`] draws it: the screen, the composition shown, the
/// bytes of the frame, the row it reorders, and whether a display holds the
/// window.
struct Again {
    what: &'static str,
    setup: String,
    composition: Option<(Row, Col, &'static str)>,
    again: &'static str,
    row: Row,
    held: Held,
}

/// Whether the window is drawn as stored or a display re-breaks it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Held {
    Stored,
    Display,
}

/// **A paragraph laid out again as it was converts no row**
/// (`Renderer::convert`, as the instance path converts, and
/// `Renderer::redraw`): a frame that lays a paragraph out again whose maps
/// come out as they were changes no row: its cells, its order and its
/// joins are the last frame's. Each frame is sealed first, as the app's
/// frame seals it. The instance path draws a frame by rows only where
/// nothing but the cursor's own rows changed (`Gpu::draw_since`), so a row
/// read as laid out again draws the frame whole. The controls: the case's
/// row is drawn in another order; where a display holds the window, it is
/// held, and after the frame another display is (a display made again);
/// the frame lays the paragraph out again (`Renderer::bidi_work`); and a
/// letter written otherwise draws the frame whole.
fn converts_no_row(case: Again) {
    // One frame as the instance path draws it: the writes sealed, the
    // window converted, and what it changed of the frame before.
    fn frame(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, damage: Damage) -> Redraw {
        let _ = t.seal_writes();
        let _ = r.convert(t, Rows::ZERO, damage);
        r.take_redraw()
    }
    let Again {
        what,
        setup,
        composition,
        again,
        row,
        held,
    } = case;
    let data = faces_data();
    let faces: Vec<Face<'_>> = data.iter().map(|d| Face::parse(d).unwrap()).collect();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, setup.as_bytes());
    let setting = Setting {
        composition,
        ..PLAIN
    };
    let mut r = made(&faces, &setting, &t);
    let _ = frame(&mut r, &mut t, Damage::Full);
    for _ in 0..2 {
        let _ = frame(&mut r, &mut t, Damage::Incremental);
    }
    assert!(
        r.bidi().reordered(row),
        "{what}, the control: row {} drawn in another order",
        row.index()
    );
    let before = r.bidi().display().map(core::ptr::from_ref);
    if held == Held::Display {
        assert!(before.is_some(), "{what}, the control: a display held");
    }
    feed(&mut t, again.as_bytes());
    let redraw = frame(&mut r, &mut t, Damage::Incremental);
    if held == Held::Display {
        let now = r.bidi().display().map(core::ptr::from_ref);
        assert!(
            now.is_some() && now != before,
            "{what}, the control: another display held"
        );
    }
    assert!(
        r.bidi_work().laid.value() > 0,
        "{what}, the control: the paragraph laid out again"
    );
    assert_eq!(redraw, Redraw::NOTHING, "{what}: no row");
    feed(
        &mut t,
        format!("\x1b7\x1b[{};1Hz\x1b8", row.index() + 1).as_bytes(),
    );
    assert_eq!(
        frame(&mut r, &mut t, Damage::Incremental),
        Redraw::Whole,
        "{what}, the control: a letter written otherwise draws the frame whole"
    );
}

/// **A row drawn in another order written again with the letter it held
/// converts no row** ([`converts_no_row`]): two paragraphs, row 7's first
/// letter written as it was. Named breaks: every row a relay lifted
/// stamped at its end, whatever the layout wrote; a lifted row's writes
/// stamped while the relay runs.
#[test]
fn a_row_drawn_in_another_order_written_as_it_was_converts_no_row() {
    converts_no_row(Again {
        what: "a row drawn in another order written as it was",
        setup: two_paragraphs(),
        composition: None,
        again: "\x1b7\x1b[7;1H\u{5d0}\x1b8",
        row: Row::from_index(6),
        held: Held::Stored,
    });
}

/// **A held display made again as it was converts no row**
/// ([`converts_no_row`]): word wrap on over a right-to-left paragraph the
/// display re-breaks, finished by a prompt, its first letter written as it
/// was, which makes the display again, another one equal to it. Named
/// breaks: every row a relay lifted stamped at its end, whatever the
/// layout wrote; the display compared by its `Rc` at the relay's end, not
/// by value.
#[test]
fn a_held_display_made_again_as_it_was_converts_no_row() {
    let words = "\u{5d0}\u{5d1}\u{5d2} \u{5d3}\u{5d4} ".repeat(10);
    converts_no_row(Again {
        what: "a held display made again as it was",
        setup: format!("{HIDE}\x1b[3;1H{words}\x1b[10;1H{PROMPT}"),
        composition: None,
        again: "\x1b7\x1b[3;1H\u{5d0}\x1b8",
        row: Row::from_index(2),
        held: Held::Display,
    });
}

/// **A composition over a row drawn in another order shown again converts
/// no row** ([`converts_no_row`]): two paragraphs, a composition at row 7,
/// read as written on every frame, so its paragraph is laid out again on
/// each, with nothing written. Named breaks: every row a relay lifted
/// stamped at its end, whatever the layout wrote; a lifted row's writes
/// stamped while the relay runs.
#[test]
fn a_composition_shown_again_converts_no_row() {
    converts_no_row(Again {
        what: "a composition shown again",
        setup: two_paragraphs(),
        composition: Some((Row::from_index(6), Col::at_index(10), "xy")),
        again: "",
        row: Row::from_index(6),
        held: Held::Stored,
    });
}
