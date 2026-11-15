//! **A sized line on the cell path is kept by where its row is stored**
//! (FS-C5): a frame's build (`baud_render::cell_frame::build`) keys each
//! double-width or double-height line drawn as stored by the row it is
//! stored in and the seal its driver closed the frame's writes under, so a
//! steady frame converts no sized line and reads none of its cells; a line
//! is converted again when its row is written, and when another stored
//! row comes to its place with nothing written.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, PackedCell, ReadLen, RowExtent};
use baud_font::{Face, Px};
use baud_render::cell_frame::{CellInputs, CellParts, build};
use baud_render::screens::{SETS, Screens, SetIx};
use baud_render::{
    Candidate, GlyphMap, Instance, Leaving, Lent, MapEntry, Overlay, Renderer, Resolved, StyleStamp,
};
use support::{WS, font, renderer};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
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
    fn new(backing: &'m mut [MapEntry], t: &Term<'_>) -> Cells<'m> {
        let (stride, cols, rows) = (
            t.stride().index(),
            usize::from(WS.ws_col),
            usize::from(WS.ws_row),
        );
        Cells {
            map: GlyphMap::over(backing),
            exceptions: Overlay::new(),
            screens: Screens::new(),
            copies_mem: core::array::from_fn(|_| vec![PackedCell::EMPTY; (rows + 2) * stride]),
            styles_mem: core::array::from_fn(|_| vec![Resolved::default(); t.styles().len()]),
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

/// **One cell frame into `at`, as the app's `present` builds it**: the
/// frame's writes sealed first, then the frame built with no composition,
/// prediction or slide. How many sized lines it converted.
fn frame(r: &mut Renderer<'_, '_>, t: &mut Term<'_>, cell: &mut Cells<'_>, at: SetIx) -> u64 {
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
    let before = r.sized_converted().value();
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
        || Some(at),
    )
    .expect("the set is free");
    r.sized_converted().value() - before
}

/// **A sized line is converted again when its row is written, and not
/// before**: a DECDWL line and a DECDHL pair, the frames into each set in
/// turn until every set was built from; then a steady frame, which
/// converts none -- the control that the lines' other inputs hold -- a
/// cell written on the DECDWL line with the cursor put back, which
/// converts that line alone, and a frame after it, which converts none.
/// Named break: the line's row held whatever was written to it since the
/// key's seal.
#[test]
fn a_sized_line_is_converted_again_when_its_row_is_written_and_not_before() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[2;1H\x1b#6wide line\x1b[5;1H\x1b#3tall\x1b[6;1H\x1b#4tall\x1b[1;1H",
    );
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        3,
        "the first frame converts the three sized lines"
    );
    for _ in 0..SETS {
        frame(&mut r, &mut t, &mut cells, next());
    }
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        0,
        "the control: a steady frame converts no sized line"
    );
    feed(&mut t, b"\x1b7\x1b[2;3HX\x1b8");
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        1,
        "the written line alone converted again"
    );
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        0,
        "and its records kept after"
    );
}

/// **A sized line is converted again when another stored row comes to
/// its place**: two DECDWL lines of other text at rows 3 and 4, the
/// frames until every set was built from and a steady one, which converts
/// none -- the control -- then the screen scrolled up a line by a line
/// feed at the bottom, which writes only the row it brings in: rows 2 and
/// 3 now show the two lines, row 2 a sized line it was not and row 3 the
/// other line's row, so both are converted. Named break: the line held by
/// its generation alone, whichever stored row is drawn in its place.
#[test]
fn a_sized_line_is_converted_again_when_another_stored_row_comes_to_its_place() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).unwrap()];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[3;1H\x1b#6upper line\x1b[4;1H\x1b#6lower line\x1b[1;1H",
    );
    let mut backing = GlyphMap::backing();
    let mut cells = Cells::new(&mut backing, &t);
    let mut sets = (0..).map(|k| SetIx(k % SETS));
    let mut next = || sets.next().expect("a set");
    for _ in 0..=SETS {
        frame(&mut r, &mut t, &mut cells, next());
    }
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        0,
        "the control: a steady frame converts no sized line"
    );
    feed(
        &mut t,
        format!("\x1b7\x1b[{};1H\n\x1b8", WS.ws_row).as_bytes(),
    );
    assert_eq!(
        frame(&mut r, &mut t, &mut cells, next()),
        2,
        "both lines that moved converted again"
    );
}
