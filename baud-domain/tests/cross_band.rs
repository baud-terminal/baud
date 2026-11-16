//! **Every row-to-row copy hands its [`CrossBand`] the cells it wrote**, with
//! the physical rows they came from and went to: [`View::copy_row`],
//! [`View::copy_past_edge`], [`View::insert_lines_past_edge`],
//! [`View::delete_lines_past_edge`], [`View::insert_lines_in`],
//! [`View::delete_lines_in`] and [`View::scroll_rect`]. The recorder below
//! rewrites each cell it is handed to the letter of the row it reached, so a
//! row whose copied cells read that letter afterwards was handed over whole,
//! and one that still reads its source's letter was not.

use baud_domain::*;

const STRIDE: usize = 12;
const COLS: usize = 8;
const ROWS: usize = 5;

/// What one translation was told: the rows, and how many cells.
#[derive(Debug, PartialEq, Eq)]
struct Told {
    from: usize,
    to: usize,
    cells: usize,
}

/// Records each translation and rewrites its cells to the letter of the
/// row they reached, `A` for row 0.
#[derive(Default)]
struct Recorder(Vec<Told>);

impl CrossBand for Recorder {
    fn translate(&mut self, from: PhysRow, to: PhysRow, mut cells: RowMut<'_>) {
        self.0.push(Told {
            from: from.index(),
            to: to.index(),
            cells: cells.len(),
        });
        for c in cells.iter_mut() {
            *c = glyph(upper(to.index()));
        }
    }

    fn rotated(&mut self, _: PhysRow) {}
}

fn glyph(c: char) -> PackedCell {
    PackedCell::glyph(Codepoint::from_char(c), StyleId::DEFAULT, CellFlags::NONE)
}

fn lower(r: usize) -> char {
    char::from(b'a' + u8::try_from(r).expect("a row under 26"))
}

fn upper(r: usize) -> char {
    char::from(b'A' + u8::try_from(r).expect("a row under 26"))
}

/// Whether row `r`'s first cells read `letters`, one a cell.
fn reads(v: &View<'_>, r: usize, letters: &str) -> bool {
    let row = v.row_slice(Row::from_index(r));
    letters
        .chars()
        .enumerate()
        .all(|(x, c)| row[x].content == Content::codepoint(Codepoint::from_char(c)))
}

/// A screen of `ROWS` rows `COLS` wide on a `STRIDE`-cell pitch, no
/// rotation, row `r` written with `COLS` of its lower-case letter, and
/// `f` run over it.
fn with_screen(f: impl FnOnce(&mut View<'_>)) {
    let mut cells = vec![PackedCell::EMPTY; STRIDE * ROWS];
    let mut used = vec![RowMeta::ZERO; ROWS];
    let (mut ac, mut au) = (vec![], vec![]);
    let mut store = SurfaceStore::new(
        Stride::of_cells(STRIDE),
        (&mut cells, &mut used),
        (&mut ac, &mut au),
    );
    let mut v = View::new(
        store.stamped(Which::Primary),
        Cols::of_index(COLS),
        Rows::of_index(ROWS),
        Rows::of_index(ROWS),
        RowRot::NONE,
    );
    for r in 0..ROWS {
        for x in 0..COLS {
            v.put(Row::from_index(r), Col::at_index(x), glyph(lower(r)));
        }
    }
    f(&mut v);
}

fn told(from: usize, to: usize, cells: usize) -> Told {
    Told { from, to, cells }
}

#[test]
fn delete_lines_hands_over_each_row_it_moves_up() {
    with_screen(|v| {
        let mut x = Recorder::default();
        v.delete_lines_in(
            Row::ZERO,
            Count::of(2),
            Row::from_index(ROWS - 1),
            PackedCell::EMPTY,
            &mut x,
        );
        assert_eq!(x.0, [told(2, 0, COLS), told(3, 1, COLS), told(4, 2, COLS)]);
        assert!(reads(v, 0, "AAAAAAAA"), "row 0");
        assert!(reads(v, 2, "CCCCCCCC"), "row 2");
    });
}

#[test]
fn insert_lines_hands_over_each_row_it_moves_down() {
    with_screen(|v| {
        let mut x = Recorder::default();
        v.insert_lines_in(
            Row::ZERO,
            Count::of(1),
            Row::from_index(ROWS - 1),
            PackedCell::EMPTY,
            &mut x,
        );
        assert_eq!(
            x.0,
            [
                told(3, 4, COLS),
                told(2, 3, COLS),
                told(1, 2, COLS),
                told(0, 1, COLS)
            ]
        );
        assert!(reads(v, 4, "EEEEEEEE"), "row 4");
        assert!(reads(v, 1, "BBBBBBBB"), "row 1");
    });
}

#[test]
fn copy_row_hands_over_what_it_wrote() {
    with_screen(|v| {
        let mut x = Recorder::default();
        let n = v.copy_row(Row::from_index(1), Row::from_index(3), &mut x);
        assert_eq!(x.0, [told(1, 3, n)]);
        assert!(reads(v, 3, &"D".repeat(n)), "row 3");
        let mut x = Recorder::default();
        assert_eq!(
            v.copy_row(Row::from_index(2), Row::from_index(2), &mut x),
            0
        );
        assert!(x.0.is_empty(), "a row onto itself copies nothing");
    });
}

#[test]
fn scroll_rect_hands_over_the_columns_it_moves() {
    with_screen(|v| {
        let mut x = Recorder::default();
        v.scroll_rect(
            Row::ZERO,
            Row::from_index(2),
            Col::at_index(1),
            Col::at_index(4),
            Count::of(1),
            BandMove::Up,
            PackedCell::EMPTY,
            &mut x,
        );
        assert_eq!(x.0, [told(1, 0, 3), told(2, 1, 3)]);
        assert!(reads(v, 0, "aAAAa"), "row 0");
        assert!(reads(v, 1, "bBBBb"), "row 1");
    });
}

/// The past-edge movers, over rows every one of which holds something past
/// the live edge, to the pitch.
#[test]
fn the_past_edge_movers_hand_over_the_cells_past_the_edge() {
    let past_w = STRIDE - COLS;
    with_screen(|v| {
        let mut flags = [PastEdge::Held; ROWS];
        let mut held = Rows::of_index(ROWS);
        let mut past = PastRows {
            flags: &mut flags,
            held: &mut held,
        };
        let reach = Cols::of_index(STRIDE);
        let mut x = Recorder::default();
        v.copy_past_edge(Row::ZERO, Row::from_index(2), reach, &mut past, &mut x);
        assert_eq!(x.0, [told(1, 2, past_w), told(0, 1, past_w)]);
        let mut x = Recorder::default();
        v.insert_lines_past_edge(
            Row::ZERO,
            Count::of(1),
            Row::from_index(2),
            reach,
            &mut past,
            &mut x,
        );
        assert_eq!(x.0, [told(1, 2, past_w), told(0, 1, past_w)]);
        let mut x = Recorder::default();
        v.delete_lines_past_edge(
            Row::ZERO,
            Count::of(1),
            Row::from_index(2),
            reach,
            &mut past,
            &mut x,
        );
        assert_eq!(x.0, [told(1, 0, past_w), told(2, 1, past_w)]);
    });
}

/// The no-op is zero-sized, so it costs a mover no argument at all.
#[test]
fn one_table_is_zero_sized() {
    assert_eq!(core::mem::size_of::<OneTable>(), 0);
}

/// Records the top each turn of the ring reports; no mover runs.
#[derive(Default)]
struct Turns(Vec<usize>);

impl CrossBand for Turns {
    fn translate(&mut self, _: PhysRow, _: PhysRow, _: RowMut<'_>) {
        unreachable!("a turn copies no cell");
    }

    fn rotated(&mut self, top: PhysRow) {
        self.0.push(top.index());
    }
}

/// **Every turn of the ring says which physical row is the screen's first
/// now**: a scroll up by one and by two, back by one, and the reset an
/// alternate screen's entry makes, each once.
#[test]
fn each_turn_of_the_ring_reports_its_top() {
    with_screen(|v| {
        let mut x = Turns::default();
        v.advance_rot(1, &mut x);
        v.advance_rot(2, &mut x);
        v.retreat_rot(1, &mut x);
        v.reset_rot(&mut x);
        assert_eq!(x.0, [1, 3, 2, 0]);
    });
}
