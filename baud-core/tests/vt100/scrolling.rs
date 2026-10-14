//! Vertical movement and scrolling: DECSTBM, IND, RI, NEL, LF, IL, DL, SU, SD.
//!
//! These operations are one feature, not eight. Every one of them either
//! respects the scrolling region or corrupts the screen, and three of them
//! (DL, DECSTBM, SM/RM) were previously *parsed and dropped* -- accepted by the
//! parser, discarded by `dispatch`, with no way for an application to notice.
//!
//! Assertions compare rendered rows rather than internal state, so a
//! representation change cannot make a test vacuously pass.

use crate::support;
use baud_core::*;

/// One step of a script: bytes to feed, and what the model does with them.
///
/// Named because the tuple-in-slice spelling is past clippy's complexity bar,
/// and both scripts below are the same shape.
type Step = (&'static str, fn(&mut Model));

const COLS: usize = 200;

/// The rendered rows of a run, as comparable values.
fn rows_of(bytes: &[u8]) -> Vec<Vec<PackedCell>> {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (_, rows) = t.geometry();
    (0..rows.index())
        .map(|y| {
            t.row(Row::from_index(y))
                .expect("a row on the grid")
                .to_vec()
        })
        .collect()
}

fn cursor_of(bytes: &[u8]) -> (usize, usize) {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);
    support::feed(&mut t, bytes);
    let (r, c) = t.cursor();
    (r.index(), c.index())
}

// **`OpMode` does not exist in `baud-core`.** core2 had three -- `Runs`
// stored spans and materialised later, `Computed` and `Templated` wrote cells
// -- and this core has only the materialising one, because the span path was
// built here, measured at +50% on two recordings and -5-10% on six, and
// reverted. So every test that swept the three now runs once, and the ones
// that were *about* `Runs` are retired below rather than silently collapsed.

// ── column semantics ────────────────────────────────────────────────────────

/// IND (`ESC D`) is *index*: down one line, column untouched.
///
/// This routed through a `newline()` that reset the column, making IND behave
/// as NEL. `[[source: ctlseqs]]` separates them precisely.
#[test]
fn ind_preserves_the_column() {
    {
        assert_eq!(
            cursor_of(b"abcde\x1bD"),
            (1, 5),
            "IND must move down and leave the column alone"
        );
    }
}

/// NEL (`ESC E`) is CR + IND.
#[test]
fn nel_returns_to_column_one() {
    {
        assert_eq!(cursor_of(b"abcde\x1bE"), (1, 0));
    }
}

/// LF moves down only, because LNM (mode 20) is reset by default.
#[test]
fn lf_does_not_return_to_column_one_by_default() {
    {
        assert_eq!(
            cursor_of(b"abcde\n"),
            (1, 5),
            "LF with LNM reset must not carriage-return"
        );
    }
}

/// With LNM set (`CSI 20 h`) it does. That is the whole difference.
#[test]
fn lf_returns_to_column_one_when_lnm_is_set() {
    {
        assert_eq!(cursor_of(b"\x1b[20habcde\n"), (1, 0));
        // and resetting it restores the default
        assert_eq!(cursor_of(b"\x1b[20h\x1b[20labcde\n"), (1, 5));
    }
}

// ── DECSTBM ─────────────────────────────────────────────────────────────────

/// `CSI r` sets the region and homes the cursor to the page origin obeying
/// DECOM: "First column of first line if Origin Mode is in the reset
/// (Absolute) state; Top and Left Margins if Origin Mode is in the set
/// (Displaced) state" (`[[source: dec-std-070]]` DECSTBM note 1, p. 5-25).
/// It went to the region's top row with DECOM reset until 2026-10-13.
#[test]
fn decstbm_homes_the_cursor_to_the_page_origin() {
    assert_eq!(cursor_of(b"\x1b[4;4H\x1b[3;6r"), (0, 0));
    assert_eq!(cursor_of(b"\x1b[?6h\x1b[4;4H\x1b[3;6r"), (2, 0));
}

/// A region of fewer than two lines is rejected, leaving the previous one.
#[test]
fn decstbm_rejects_a_degenerate_region() {
    {
        // 4;4 is one line: ignored, so the cursor is not homed to row 3.
        assert_eq!(cursor_of(b"\x1b[4;4r"), (0, 0));
    }
}

/// Bare `CSI r` means the whole screen, which is not the same as `CSI 1;1 r`.
///
/// `OneBased::from_param` clamps an omitted parameter to 1, so the two were
/// indistinguishable until the bottom became an `Option`.
#[test]
fn bare_decstbm_resets_to_full_screen() {
    {
        // Shrink, then reset, then check a line feed at the old region bottom
        // does NOT scroll -- it would if the region were still 2..4.
        let with_reset = rows_of(b"\x1b[3;5r\x1b[r\x1b[5;1HX\n\nY");
        let never_set = rows_of(b"\x1b[5;1HX\n\nY");
        assert_eq!(
            with_reset, never_set,
            "bare CSI r must restore the full-screen region"
        );
    }
}

/// Where `Y` and `X` are, 0-based, and every reply, in order.
type Landed = (Option<(usize, usize)>, Option<(usize, usize)>, Vec<u8>);

/// 20 by 8, for the zero-bottom cases: what `Y`, `X` and every reply say
/// after `bytes`, at every read size, each required to agree with the whole.
fn small_at_every_read(bytes: &[u8]) -> Landed {
    const WS: WinSize = WinSize {
        ws_row: 8,
        ws_col: 20,
    };
    let at = |read: usize| {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        let mut reply = Vec::new();
        for piece in bytes.chunks(read) {
            t.write_at(BytePos::ZERO, piece);
            t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
            reply.extend_from_slice(t.reply());
            t.clear_reply();
        }
        let find = |ch: u32| {
            (0..8).find_map(|y| {
                t.row(Row::from_index(y)).expect("a row on the grid")[..20]
                    .iter()
                    .position(|c| c.content.as_codepoint().map(|cp| cp.value()) == Some(ch))
                    .map(|x| (y, x))
            })
        };
        (find(u32::from('Y')), find(u32::from('X')), reply)
    };
    let whole = at(bytes.len());
    for read in 1..bytes.len() {
        assert_eq!(at(read), whole, "reads of {read} differ: {bytes:?}");
    }
    whole
}

/// Under DECOM, `CUP 99;1` stops at the bottom margin; `X` and `CPR` say where.
const BOTTOM_PROBE: &[u8] = b"\x1b[?6h\x1b[99;1HX\x1b[6n";

fn with_probe(head: &[u8]) -> Vec<u8> {
    [head, BOTTOM_PROBE].concat()
}

/// **An explicit 0 bottom margin is the last line**, as an omitted one is.
/// `[[source: dec-std-070]]` DECSTBM: "default Pb: last line" (p. 5-25), and
/// its algorithm reads a 0 as omitted, "if (parv[1] == 0) parv[1] =
/// lines_per_page;" (p. 5-26). `[[source: vt510-decstbm]]`: "Default: Pb =
/// current number of lines per screen"; `[[source: vt100-ug-ch3]]` defines a
/// default as "assumed when no explicit value, or a value of 0, is
/// specified", and its DECSTBM's is "the entire screen (no margins)";
/// `[[source: ctlseqs]]`: "default = full size of window". The bottom
/// decoded as line 1 until 2026-10-13, so `CSI 2;0 r` was rejected and did
/// not home: parsed and dropped.
#[test]
fn a_zero_bottom_margin_is_the_last_line() {
    let (y, x, reply) = small_at_every_read(&with_probe(b"\x1b[4;4H\x1b[2;0rY"));
    assert_eq!(y, Some((0, 0)), "homed");
    assert_eq!(
        (x, reply.as_slice()),
        (Some((7, 0)), &b"\x1b[7;2R"[..]),
        "region 2..8"
    );
    // Over a region already set, it replaces it.
    let (_, x, reply) = small_at_every_read(&with_probe(b"\x1b[2;5r\x1b[2;0r"));
    assert_eq!(
        (x, reply.as_slice()),
        (Some((7, 0)), &b"\x1b[7;2R"[..]),
        "2..5 to 2..8"
    );
}

/// `CSI 0;0 r` is the whole screen: a 0 top is line 1 and a 0 bottom the last.
#[test]
fn a_zero_zero_region_is_the_whole_screen() {
    let (_, x, reply) = small_at_every_read(&with_probe(b"\x1b[2;5r\x1b[0;0r"));
    assert_eq!((x, reply.as_slice()), (Some((7, 0)), &b"\x1b[8;2R"[..]));
}

/// **A bottom past the screen leaves the region alone**, and does not home.
/// `[[source: dec-std-070]]` DECSTBM note 3, p. 5-25: "If the value
/// specified for the Bottom Margin is greater than the number of lines in
/// the Logical Display Page, this control will be ignored (not executed)",
/// and its algorithm, p. 5-26, sets the margins only when `parv[1] <=
/// lines_per_page`. It was clamped to the last line until 2026-10-13, as
/// xterm and ghostty do (`bench-others/ghostty-oracle` pins theirs).
#[test]
fn a_bottom_margin_past_the_screen_leaves_the_region_alone() {
    let (y, x, reply) = small_at_every_read(&with_probe(b"\x1b[2;5r\x1b[4;4H\x1b[2;9rY"));
    assert_eq!(y, Some((3, 3)), "not homed");
    assert_eq!(
        (x, reply.as_slice()),
        (Some((4, 0)), &b"\x1b[4;2R"[..]),
        "2..5 kept"
    );
    // The control: the last line itself is a bottom margin.
    let (_, x, reply) = small_at_every_read(&with_probe(b"\x1b[2;5r\x1b[2;8r"));
    assert_eq!(
        (x, reply.as_slice()),
        (Some((7, 0)), &b"\x1b[7;2R"[..]),
        "2..8"
    );
}

/// The control: an omitted bottom was already the last line.
#[test]
fn an_omitted_bottom_margin_is_the_last_line() {
    let (_, x, reply) = small_at_every_read(&with_probe(b"\x1b[2;5r\x1b[2r"));
    assert_eq!((x, reply.as_slice()), (Some((7, 0)), &b"\x1b[7;2R"[..]));
}

/// A line feed at the region bottom scrolls the region and nothing else.
#[test]
fn lf_at_region_bottom_scrolls_only_the_region() {
    {
        // Region rows 2..4 (1-based 3..5). Put a marker outside it, at row 0,
        // and fill the region so a scroll is observable.
        let bytes = b"\x1b[1;1HTOP\x1b[3;5r\x1b[3;1Ha\nb\nc\nd";
        let r = rows_of(bytes);
        let untouched = rows_of(b"\x1b[1;1HTOP");
        assert_eq!(
            r[0], untouched[0],
            "row 0 is outside the region and must not move"
        );
        for y in 5..r.len() {
            assert_eq!(r[y], untouched[y], "row {y} is below the region");
        }
    }
}

/// A region that ends on the screen's last row but starts below its top is
/// a region, not the screen: an `LF` or `IND` at its bottom scrolls rows
/// 5..50 and leaves the four header rows above it. The full-screen fast
/// path tested only the bottom, so this shape rotated the whole screen and
/// retired the headers into the history. foot scrolls the region here
/// (`[[source: foot-linefeed]]`, `term_linefeed` at `scroll_region.end - 1`),
/// and so does alacritty, which is what `baud-difftest` compares against.
#[test]
fn lf_at_the_bottom_of_a_region_ending_on_the_last_row_scrolls_only_the_region() {
    let heads = b"\x1b[1;1HH1\x1b[2;1HH2\x1b[3;1HH3\x1b[4;1HH4";
    for step in [&b"\n"[..], b"\x1bD"] {
        let mut bytes = heads.to_vec();
        bytes.extend(b"\x1b[5;50r\x1b[50;1HLAST");
        for _ in 0..3 {
            bytes.extend(step);
        }
        let mut want = heads.to_vec();
        want.extend(b"\x1b[47;1HLAST");
        assert_eq!(
            rows_of(&bytes),
            rows_of(&want),
            "{step:?} x3: the headers stay and LAST is three rows up"
        );
    }
    // A wrap off the region's bottom row takes the same path: `B` wraps from
    // the last column, the region scrolls one row, and the headers stay.
    let mut bytes = heads.to_vec();
    bytes.extend(b"\x1b[5;50r\x1b[50;1HLAST\x1b[50;200HAB");
    let mut want = heads.to_vec();
    want.extend(b"\x1b[49;1HLAST\x1b[49;200HA\x1b[50;1HB");
    assert_eq!(
        rows_of(&bytes),
        rows_of(&want),
        "a wrap at the bottom scrolls the region and not the headers"
    );
}

/// An `LF` on the screen's last row, below a region, does nothing: the
/// cursor is outside the region, so there is nothing to scroll, and it is
/// already on the last row, so there is nowhere to move. foot moves the
/// cursor down clamped to the screen (`term_cursor_down`), and alacritty
/// does the same. It had rotated the whole screen, taking row 1 into the
/// history and moving the status line up into the region.
#[test]
fn lf_on_the_last_row_below_a_region_does_not_scroll() {
    let bytes = b"\x1b[1;1HH1\x1b[1;49r\x1b[50;1HST\nXY";
    assert_eq!(
        rows_of(bytes),
        rows_of(b"\x1b[1;1HH1\x1b[50;1HSTXY"),
        "nothing moved, and XY followed ST on the last row"
    );
    assert_eq!(cursor_of(bytes), (49, 4));
}

/// RI at the region top scrolls the region down, not the screen.
#[test]
fn ri_at_region_top_scrolls_only_the_region() {
    {
        let bytes = b"\x1b[1;1HTOP\x1b[3;5r\x1b[3;1H\x1bM";
        let r = rows_of(bytes);
        let untouched = rows_of(b"\x1b[1;1HTOP");
        assert_eq!(r[0], untouched[0], "RI must not scroll row 0");
        for y in 5..r.len() {
            assert_eq!(r[y], untouched[y], "row {y} is below the region");
        }
    }
}

// ── DL / IL / SU / SD ───────────────────────────────────────────────────────

/// DL was parsed and dropped. It must actually delete.
#[test]
fn dl_deletes_lines() {
    {
        let after = rows_of(b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[1;1H\x1b[M");
        let want = rows_of(b"\x1b[1;1Hbbb\r\nccc");
        assert_eq!(after, want, "CSI M must delete the cursor's line");
    }
}

/// DL with the cursor outside the region does nothing.
#[test]
fn dl_outside_the_region_is_a_no_op() {
    {
        let bytes = b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[2;4r\x1b[1;1H\x1b[M";
        let after = rows_of(bytes);
        let want = rows_of(b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[2;4r\x1b[1;1H");
        assert_eq!(after, want);
    }
}

/// DL with a count larger than the region must clear it, not panic.
///
/// `delete_lines_in` computed `bot - n` on usize, which underflows when the
/// whole region is deleted from row 0.
#[test]
fn dl_with_a_huge_count_does_not_panic() {
    {
        support::with_corpus(|t| support::feed(t, b"\x1b[1;1Hx\x1b[99999M"));
        support::with_corpus(|t| support::feed(t, b"\x1b[2;5r\x1b[2;1Hx\x1b[99999M"));
        support::with_corpus(|t| support::feed(t, b"\x1b[1;1Hx\x1b[99999L"));
    }
}

/// SU and SD scroll the region without moving the cursor.
#[test]
fn su_and_sd_leave_the_cursor_alone() {
    {
        assert_eq!(cursor_of(b"\x1b[5;7H\x1b[2S"), (4, 6), "SU");
        assert_eq!(cursor_of(b"\x1b[5;7H\x1b[2T"), (4, 6), "SD");
    }
}

/// SU by one is the same screen as a line feed at the region bottom.
#[test]
fn su_agrees_with_a_line_feed_at_the_bottom() {
    {
        let via_su = rows_of(b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[S");
        let via_lf = rows_of(b"\x1b[1;1Haaa\r\nbbb\r\nccc\x1b[50;1H\n");
        assert_eq!(via_su, via_lf);
    }
}

/// IL must stop at the region bottom rather than the screen bottom.
#[test]
fn il_is_bounded_by_the_region() {
    {
        let bytes = b"\x1b[1;1Haaa\r\nbbb\r\nccc\r\nddd\x1b[1;3r\x1b[1;1H\x1b[L";
        let r = rows_of(bytes);
        let untouched = rows_of(b"\x1b[1;1Haaa\r\nbbb\r\nccc\r\nddd");
        assert_eq!(
            r[3], untouched[3],
            "row 3 is below a 1..3 region and must not be pushed"
        );
    }
}

// ── reset ───────────────────────────────────────────────────────────────────

/// RIS restores the full-screen region and the default modes.
///
/// A region surviving a reset is how an application ends up drawing into a
/// window it never asked for.
#[test]
fn ris_restores_the_region_and_modes() {
    {
        let after = rows_of(b"\x1b[3;5r\x1b[20h\x1bc\x1b[5;1HX\n\nY");
        let fresh = rows_of(b"\x1b[5;1HX\n\nY");
        assert_eq!(after, fresh, "RIS must restore region and LNM");
    }
}

/// Every row is `COLS` wide, so the row-slice assertions above compare like
/// with like. A stride change that broke this would make them vacuous.
#[test]
fn rows_are_full_width() {
    let r = rows_of(b"x");
    assert!(!r.is_empty());
    for row in &r {
        assert_eq!(row.len(), COLS);
    }
}

// ── the full-screen case, which had no content test ─────────────────────────

/// **A full-screen scroll moves the rendered grid.**
///
/// The tests above it either scroll a *region* set with DECSTBM, which takes
/// the copying path, or scroll the full screen without reading what it
/// leaves: `su_and_sd_leave_the_cursor_alone` reads the cursor and
/// `su_agrees_with_a_line_feed_at_the_bottom` compares two full-screen paths
/// with each other. The full-screen case takes a different path --
/// `scroll_region_up` advances the grid's rotation a row instead of moving
/// the rows -- and nothing checked what it left on the screen.
///
/// In `baud-core2` it left the screen alone. Its ring advance moved `head`,
/// which indexed the **run** table; `vp` was indexed by screen row and nothing
/// re-derived it, so in `Computed` and `Templated` the content never moved and
/// only the bottom row was blanked. That is the most common scroll there is:
/// every line of output past the last row.
#[test]
fn a_full_screen_scroll_moves_the_grid() {
    {
        let ws = support::CORPUS;
        let mut st = Owned::new(&ws);
        let mut t = Term::new(st.storage(), &ws);
        let rows = t.geometry().1.index();
        // One distinct letter per row, then one more line to force a scroll.
        let mut bytes = Vec::new();
        for i in 0..rows {
            bytes.push(b'a' + (i % 26) as u8);
            bytes.extend_from_slice(b"\r\n");
        }
        t.write_at(BytePos::ZERO, &bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index((bytes).len()));

        let col0: Vec<char> = (0..4)
            .map(|y| {
                t.row(Row::from_index(y)).expect("a row on the grid")[0]
                    .content
                    .as_codepoint()
                    .map_or('.', |x| char::from_u32(x.value()).unwrap_or('?'))
            })
            .collect();
        assert_eq!(
            col0,
            vec!['b', 'c', 'd', 'e'],
            "after one scroll the top row must be the second line"
        );
        // And the row that scrolled in is blank.
        let last = t.row(Row::from_index(rows - 1)).expect("a row on the grid")[0];
        assert!(last.content.is_empty(), "the row scrolled in must be blank");
    }
}

/// `CSI S` scrolls the full screen too, by the same path.
#[test]
fn su_scrolls_the_full_screen() {
    {
        let ws = support::CORPUS;
        let mut st = Owned::new(&ws);
        let mut t = Term::new(st.storage(), &ws);
        t.write_at(BytePos::ZERO, b"one\r\ntwo\r\nthree\x1b[1S");
        t.feed(
            BytePos::ZERO,
            ReadLen::from_index((b"one\r\ntwo\r\nthree\x1b[1S").len()),
        );
        let row0: String = t.row(Row::from_index(0)).expect("a row on the grid")[..3]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .map_or('.', |x| char::from_u32(x.value()).unwrap_or('?'))
            })
            .collect();
        assert_eq!(row0, "two", "SU 1 brings the second line to the top");
    }
}

// ---------------------------------------------------------------------------
// An independent model, for the operations that move whole lines.
//
// The nineteen tests above check one operation at a time against a hand-written
// expectation. That is the right shape for pinning semantics and the wrong
// shape for protecting a change to *how* lines move: a permutation bug shows up
// after a mixed sequence, not after a single `IL`.
//
// So this models the screen as rows of text and applies the same operations by
// the definitions in `[[source: ctlseqs]]`, independently of how baud stores
// anything. It exists to be the oracle for making region scroll and `IL`/`DL`
// zero-copy -- `less-pager.raw` is 22x bound by those moves -- and it is worth
// having whether or not that lands.

/// One screen of text, scrolled by the book.
struct Model {
    rows: Vec<String>,
    top: usize,
    bot: usize,
    cy: usize,
}

impl Model {
    fn new(rows: usize) -> Model {
        Model {
            rows: vec![String::new(); rows],
            top: 0,
            bot: rows - 1,
            cy: 0,
        }
    }
    /// Scroll the region up by `n`: the top `n` lines leave, blanks enter at
    /// the bottom.
    fn up(&mut self, n: usize) {
        let n = n.min(self.bot + 1 - self.top);
        for _ in 0..n {
            self.rows.remove(self.top);
            self.rows.insert(self.bot, String::new());
        }
    }
    /// The reverse: blanks enter at the top.
    fn down(&mut self, n: usize) {
        let n = n.min(self.bot + 1 - self.top);
        for _ in 0..n {
            self.rows.remove(self.bot);
            self.rows.insert(self.top, String::new());
        }
    }
    /// `IL` at the cursor, bounded by the region.
    fn insert(&mut self, n: usize) {
        if self.cy < self.top || self.cy > self.bot {
            return;
        }
        let n = n.min(self.bot + 1 - self.cy);
        for _ in 0..n {
            self.rows.remove(self.bot);
            self.rows.insert(self.cy, String::new());
        }
    }
    /// `DL` at the cursor, bounded by the region.
    fn delete(&mut self, n: usize) {
        if self.cy < self.top || self.cy > self.bot {
            return;
        }
        let n = n.min(self.bot + 1 - self.cy);
        for _ in 0..n {
            self.rows.remove(self.cy);
            self.rows.insert(self.bot, String::new());
        }
    }
}

/// **Mixed line movement agrees with an independent model.**
///
/// The sequence is deterministic rather than random: a property test here would
/// need a generator for six operations with region and cursor state, and the
/// value is in covering the *interactions* -- a region set, lines inserted
/// inside it, the region moved, more deleted -- which a fixed script states
/// more clearly than a shrunk counterexample would.
#[test]
fn mixed_line_movement_agrees_with_an_independent_model() {
    const ROWS: usize = 50;
    let mut m = Model::new(ROWS);
    let mut b: Vec<u8> = Vec::new();

    // Fill every row with a distinguishable marker, so a row that moves to the
    // wrong place is visible rather than merely blank.
    for y in 0..ROWS {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        let text = format!("row{y:02}");
        b.extend_from_slice(text.as_bytes());
        m.rows[y] = text;
    }

    // A region, then every line-moving operation inside and across it.
    let script: &[Step] = &[
        ("\x1b[5;40r", |m| {
            m.top = 4;
            m.bot = 39;
            m.cy = 4;
        }),
        ("\x1b[10;1H", |m| m.cy = 9),
        ("\x1b[3L", |m| m.insert(3)),
        ("\x1b[2M", |m| m.delete(2)),
        ("\x1b[4S", |m| m.up(4)),
        ("\x1b[2T", |m| m.down(2)),
        ("\x1b[38;1H", |m| m.cy = 37),
        ("\x1b[5L", |m| m.insert(5)),
        ("\x1b[7M", |m| m.delete(7)),
        ("\x1b[1;50r", |m| {
            m.top = 0;
            m.bot = 49;
            m.cy = 0;
        }),
        ("\x1b[20;1H", |m| m.cy = 19),
        ("\x1b[6M", |m| m.delete(6)),
        ("\x1b[3L", |m| m.insert(3)),
    ];
    for (bytes, apply) in script {
        b.extend_from_slice(bytes.as_bytes());
        apply(&mut m);
    }

    let rows = rows_of(&b);
    for (y, want) in m.rows.iter().enumerate() {
        let got: String = rows[y]
            .iter()
            .filter_map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|x| char::from_u32(x.value()))
            })
            .collect::<String>()
            .trim_end()
            .to_string();
        assert_eq!(
            &got, want,
            "row {y} after the script: baud has {got:?}, the model says {want:?}"
        );
    }
}

/// **The same script, in the alternate screen.**
///
/// This file had no alternate-screen scroll test at all, which is precisely the
/// gap that hid the original full-screen bug: `docs/record.md` ("Full-screen
/// scroll: the grid is rotated, not copied") records that every test here
/// "sets a region with DECSTBM first, which takes the copying path,
/// so the most common scroll there is had no content test at all". The
/// alternate screen is the same hole one screen over -- and it is where every
/// full-screen application lives, so it is the screen a pager, an editor or a
/// TUI actually scrolls on.
///
/// Written *before* the alternate screen gets a rotation fast path, and green
/// on the copying path, so that a green result afterwards is evidence about the
/// rotation rather than about the test.
///
/// core2 ran this in `OpMode::Computed`, not `Runs`, whose viewport reported
/// representation rather than rendered content (`git show b10c006^:CLAUDE.md`,
/// "Tests", lines 698-705). baud-core has one mode (see the top of this file).
#[test]
fn mixed_line_movement_in_the_alternate_screen_agrees_with_the_model() {
    const ROWS: usize = 50;
    let mut m = Model::new(ROWS);
    // `?1049 h` first: everything after it happens on the alternate buffer,
    // and entering it clears the screen, so the markers must be written after.
    let mut b: Vec<u8> = b"\x1b[?1049h".to_vec();

    for y in 0..ROWS {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        let text = format!("row{y:02}");
        b.extend_from_slice(text.as_bytes());
        m.rows[y] = text;
    }

    // Plain `LF` at the last row is the case the recording is full of -- 638 of
    // them in `less-pager.raw` -- and the one no test reached. It scrolls the
    // full screen with no region set, which is the branch a fast path takes.
    let script: &[Step] = &[
        // **`RI` first, while every row still holds text.** Done after a few
        // newlines instead, the row the bottom recycles to the top is already
        // blank, so blanking the *wrong* row leaves a blank either way and the
        // assertion cannot tell. That is not hypothetical: this test passed
        // with `fill_row_counted(last)` in place of `fill_row_counted(ZERO)`
        // until the script was reordered.
        ("\x1b[1;1H", |m| m.cy = 0),
        ("\x1bM", |m| m.down(1)),
        ("\x1bM", |m| m.down(1)),
        ("\x1b[50;1H", |m| m.cy = 49),
        ("\n", |m| m.up(1)),
        ("\n", |m| m.up(1)),
        ("\n", |m| m.up(1)),
        // And `RI` again once the bottom is blank, so both orderings are
        // covered rather than swapped for each other.
        ("\x1b[1;1H", |m| m.cy = 0),
        ("\x1bM", |m| m.down(1)),
        // Then the region operations, to prove the fast path and the copying
        // path agree with each other inside one session.
        ("\x1b[5;40r", |m| {
            m.top = 4;
            m.bot = 39;
            m.cy = 4;
        }),
        ("\x1b[10;1H", |m| m.cy = 9),
        ("\x1b[3L", |m| m.insert(3)),
        ("\x1b[2M", |m| m.delete(2)),
        ("\x1b[4S", |m| m.up(4)),
        ("\x1b[2T", |m| m.down(2)),
        // Back to the full screen, then more plain newlines: a fast path that
        // is only correct from a clean start would pass without this.
        ("\x1b[1;50r", |m| {
            m.top = 0;
            m.bot = 49;
            m.cy = 0;
        }),
        ("\x1b[50;1H", |m| m.cy = 49),
        ("\n", |m| m.up(1)),
        ("\n", |m| m.up(1)),
        ("\x1b[20;1H", |m| m.cy = 19),
        ("\x1b[6M", |m| m.delete(6)),
        ("\x1b[3L", |m| m.insert(3)),
        ("\x1b[50;1H", |m| m.cy = 49),
        ("\n", |m| m.up(1)),
    ];
    for (bytes, apply) in script {
        b.extend_from_slice(bytes.as_bytes());
        apply(&mut m);
    }

    let rows = rows_of(&b);
    for (y, want) in m.rows.iter().enumerate() {
        let got: String = rows[y]
            .iter()
            .filter_map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|x| char::from_u32(x.value()))
            })
            .collect::<String>()
            .trim_end()
            .to_string();
        assert_eq!(
            &got, want,
            "alternate screen, row {y} after the script: baud has {got:?}, \
             the model says {want:?}"
        );
    }
}
/// **A round trip through the alternate screen leaves the primary alone.**
///
/// Fifty marked rows on the primary, then `?1049 h` and five line-moving
/// shapes on the alternate screen -- `SU` inside a 5-40 region, `IL` and `DL`
/// inside it, `LF` at the bottom of the full screen and `RI` at its top (not
/// `SD`, `IND`, `NEL`, DECIC or DECDC) --
/// then `?1049 l`, and every primary row must read back as it was written.
/// In this core the two screens are two surfaces: `set_alt_screen` (`ops.rs`)
/// swaps the view onto the alternate one and parks the primary's cells, ring
/// and rotation in `other` until the exit swaps them back, so this fails if
/// any of those line-moves writes through to the parked surface. It reads the
/// rendered rows only. The primary never scrolls here, so its rotation is
/// zero on both sides of the trip and a restore that dropped or reset it (to
/// zero) would pass; nor does it read the primary's history, which `retire`
/// leaves alone while the alternate screen is up.
/// `a_scrolled_primary_comes_back_in_the_order_it_left` (`xterm/alt_screen.rs`)
/// scrolls the primary first and reads both.
///
/// It was written for a `baud-core2` bug in a run table this core does not
/// have: every copying line-move in core2 ran its run-table invalidation
/// unconditionally, `promoted[y] = true` by screen row and
/// `nrun[head.plus(y)] = RunCount::ZERO` through the primary's ring head, and
/// `head` did not move during an alternate-screen session, so a region scroll
/// there wrote over the primary's bookkeeping. No public accessor showed that
/// state: in core2's Runs mode a viewport showed representation, so every
/// case came back empty (`git show b10c006^:CLAUDE.md`, lines 698-705). The
/// test checked the rendered content then, as it does now.
#[test]
fn a_round_trip_through_the_alternate_screen_preserves_the_primary() {
    const ROWS: usize = 50;
    let mut b: Vec<u8> = Vec::new();
    for y in 0..ROWS {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        b.extend_from_slice(format!("row{y:02}").as_bytes());
    }
    // Into the alternate screen, then every line-moving shape it has: a
    // region scroll (the copying path), a full-screen scroll (the rotation),
    // and `IL`/`DL` inside a region.
    b.extend_from_slice(b"\x1b[?1049h");
    b.extend_from_slice(b"\x1b[5;40r");
    for _ in 0..8 {
        b.extend_from_slice(b"\x1b[S");
    }
    b.extend_from_slice(b"\x1b[10;1H\x1b[4L\x1b[3M");
    b.extend_from_slice(b"\x1b[1;50r");
    b.extend_from_slice(b"\x1b[50;1H\n\n\n");
    b.extend_from_slice(b"\x1b[1;1H\x1bM\x1bM");
    b.extend_from_slice(b"\x1b[?1049l");

    let rows = rows_of(&b);
    for (y, row) in rows.iter().enumerate().take(ROWS) {
        let got: String = row
            .iter()
            .filter_map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|x| char::from_u32(x.value()))
            })
            .collect::<String>()
            .trim_end()
            .to_string();
        assert_eq!(
            got,
            format!("row{y:02}"),
            "primary row {y} after a full session in the alternate screen"
        );
    }
}
/// **Every erase works on the alternate screen.**
///
/// `HELLO WORLD` on the alternate screen, then six erases from column 6, each
/// with the first twelve columns it must leave stated. This core erases the
/// same way on either screen -- the view holds whichever surface is up -- so
/// what this pins is that the surface erased is the one shown, on the screen
/// every full-screen application does its erasing on.
///
/// The name is `baud-core2`'s bug. core2's `erase_line` took a fast path on a
/// line its `promoted` flag called unpromoted -- erase-to-end was the
/// truncation of the line's span list, not a fill -- and read that flag raw,
/// though it belonged to the primary screen. On the alternate screen every row
/// qualified, so `CSI K` zeroed a run count belonging to the primary
/// underneath and returned **without erasing anything**: `HELLO WORLD`
/// survived an erase-to-end from column 6. Nothing caught it because every
/// erase test then ran on the primary. core2 ran each case in its `Computed`
/// mode as well as in `Runs`; this core has one mode, and each case runs once.
#[test]
fn erases_on_the_alternate_screen_are_not_swallowed_by_the_span_path() {
    // A blank is rendered as `.` rather than dropped. `Content::EMPTY` is a
    // reserved value above U+10FFFF and deliberately *not* U+0020, so the
    // usual `filter_map(as_codepoint)` projection deletes blanks instead of
    // showing them -- which makes "erased the wrong columns" and "erased the
    // right ones" produce the same string whenever the blanks are not
    // trailing. The first draft of this test asserted `"      WORLD"` and got
    // `"WORLD"` for that reason.
    //
    // (name, bytes after the text, expected first 12 columns)
    let cases: &[(&str, &[u8], &str)] = &[
        ("EL to end", b"\x1b[1;6H\x1b[K", "HELLO......."),
        ("EL to start", b"\x1b[1;6H\x1b[1K", "......WORLD."),
        ("EL whole line", b"\x1b[1;6H\x1b[2K", "............"),
        ("ED to end", b"\x1b[1;6H\x1b[J", "HELLO......."),
        ("ED whole screen", b"\x1b[1;6H\x1b[2J", "............"),
        ("ECH 3", b"\x1b[1;6H\x1b[3X", "HELLO...RLD."),
    ];
    for (name, tail, want) in cases {
        {
            let mut b: Vec<u8> = b"\x1b[?1049h\x1b[1;1HHELLO WORLD".to_vec();
            b.extend_from_slice(tail);
            let rows = rows_of(&b);
            let got: String = rows[0]
                .iter()
                .take(12)
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|x| char::from_u32(x.value()))
                        .unwrap_or('.')
                })
                .collect();
            assert_eq!(
                got, *want,
                "{name} on the alternate screen: got {got:?}, want {want:?}"
            );
        }
    }
}

/// **`IND` on the alternate screen leaves the primary's rows alone.**
///
/// Fifty marked rows on the primary, then sixty `IND` from the bottom row of
/// the alternate screen, which scroll it past a full screen; after `?1049 l`
/// every primary row must read back as it was written. Asserted on the
/// rendered rows, as the round trip above is, and
/// `scrolling_the_alt_screen_does_not_disturb_the_primary`
/// (`xterm/alt_screen.rs`) asks the same of 900 lines fed through the
/// alternate screen, comparing the whole primary before and after. The
/// primary never scrolls in either, so its rotation is zero on both sides;
/// `a_scrolled_primary_comes_back_in_the_order_it_left` is a trip that
/// scrolls it first.
///
/// It was written for a `baud-core2` bug. core2's `index` (IND, in
/// `baud-core2/src/cursor.rs`) cleared the
/// destination line's spans, which was load-bearing for its chunk invariance,
/// and indexed that line with `line()` -- `head.plus(cy)` -- where `head` was
/// the primary's ring position, unmoved for the whole alternate-screen
/// session. A screenful of `IND` in the alternate screen therefore zeroed a
/// screenful of the primary's run counts.
#[test]
fn index_on_the_alternate_screen_leaves_the_primary_intact() {
    const ROWS: usize = 50;
    let mut b: Vec<u8> = Vec::new();
    for y in 0..ROWS {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        b.extend_from_slice(format!("row{y:02}").as_bytes());
    }
    b.extend_from_slice(b"\x1b[?1049h\x1b[50;1H");
    for _ in 0..60 {
        b.extend_from_slice(b"\x1bD");
    }
    b.extend_from_slice(b"\x1b[?1049l");

    let rows = rows_of(&b);
    for (y, row) in rows.iter().enumerate().take(ROWS) {
        let got: String = row
            .iter()
            .filter_map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|x| char::from_u32(x.value()))
            })
            .collect::<String>()
            .trim_end()
            .to_string();
        assert_eq!(
            got,
            format!("row{y:02}"),
            "primary row {y} after 60 IND in the alternate screen"
        );
    }
}

/// Row 0 rendered with blanks as `.` and wide tails as `#`.
///
/// **Blanks must be visible.** `Content::EMPTY` is a reserved value above
/// U+10FFFF and deliberately not U+0020, so the usual
/// `filter_map(as_codepoint)` projection *deletes* blanks rather than showing
/// them -- which makes "erased the wrong columns" and "erased the right ones"
/// produce the same string whenever the blanks are not trailing.
fn wide_row(bytes: &[u8], n: usize) -> String {
    let rows = rows_of(bytes);
    rows[0][..n]
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(x) => char::from_u32(x.value()).unwrap_or('?'),
            None if c.content.is_wide_tail() => '#',
            None => '.',
        })
        .collect()
}

/// **No operation may split a double-width character.**
///
/// A wide character is a lead cell plus a `WIDE_TAIL`. An operation whose
/// boundary falls between them leaves an orphan: a lead claiming two columns
/// that owns one, or a tail with nothing in front of it. A renderer has no
/// coherent way to draw either.
///
/// `[[source: ghostty-screen]]` states the rule in `eraseChars` -- "if our last
/// cell is a wide char then we need to also clear the cell beyond it since we
/// can't just split a wide char" -- and applies it again in `deleteChars` and
/// `insertBlanks`.
///
/// **Every one of these cases was wrong**, and none was caught, because the
/// entire operation suite ran on one-column text: one test in the whole of
/// `vt100/` used a wide character before this. The recordings had the same hole
/// -- btop is 14.5% multi-byte with *zero* wide codepoints, and vim was the
/// only file with any, and it never scrolls.
#[test]
fn no_operation_splits_a_double_width_character() {
    const BASE: &str = "\x1b[1;1H日本語ab";
    // (name, sequence after the text, expected first ten columns)
    let cases: &[(&str, &str, &str)] = &[
        ("baseline", "", "日#本#語#ab.."),
        // EL from inside the first character takes the whole of it.
        ("EL to end, from a tail", "\x1b[1;2H\x1b[K", ".........."),
        ("EL to end, from a lead", "\x1b[1;3H\x1b[K", "日#........"),
        // EL-to-start is inclusive of the cursor, so from a lead it must also
        // take that lead's tail: columns 1-4, not 1-3.
        (
            "EL to start, from a tail",
            "\x1b[1;2H\x1b[1K",
            "..本#語#ab..",
        ),
        (
            "EL to start, from a lead",
            "\x1b[1;3H\x1b[1K",
            "....語#ab..",
        ),
        // ECH of one cell erases the character it lands in, both halves.
        ("ECH 1 on a lead", "\x1b[1;1H\x1b[1X", "..本#語#ab.."),
        ("ECH 1 on a tail", "\x1b[1;2H\x1b[1X", "..本#語#ab.."),
        // DCH shifts left; deleting half a pair would drag a lone tail in.
        ("DCH 1 on a lead", "\x1b[1;1H\x1b[1P", "本#語#ab...."),
        ("DCH 1 on a tail", "\x1b[1;2H\x1b[1P", "本#語#ab...."),
        // ICH shifts right and must move whole characters, never insert
        // between a lead and its tail.
        ("ICH 1 on a lead", "\x1b[1;1H\x1b[1@", ".日#本#語#ab."),
        ("ICH 1 on a tail", "\x1b[1;2H\x1b[1@", ".日#本#語#ab."),
    ];
    for (name, seq, want) in cases {
        let bytes = format!("{BASE}{seq}");
        let got = wide_row(bytes.as_bytes(), 10);
        assert_eq!(&got, want, "{name}: got {got:?}, want {want:?}");
        // The invariant behind every row above, stated once so a new case
        // cannot be added with a hand-written expectation that is itself wrong:
        // a tail has a lead before it, and no lead is left without its tail.
        let cells: Vec<char> = got.chars().collect();
        for (i, c) in cells.iter().enumerate() {
            if *c == '#' {
                assert!(
                    i > 0 && cells[i - 1] != '#' && cells[i - 1] != '.',
                    "{name}: orphaned wide tail at column {i} in {got:?}"
                );
            }
        }
    }
}

/// **Overwriting half a character clears the other half.**
///
/// The companion to `no_operation_splits_a_double_width_character`, and a
/// distinct class: a range operation can be *widened* to cover whole
/// characters, but a write has its own length and cannot. The partner is
/// cleared instead.
///
/// Written for `baud-core2`, whose ASCII never reached `put_cp` -- it went
/// through `blit` and `put_ascii_run` -- so fixing the per-codepoint path
/// alone would have fixed the rare case and left essentially all real text
/// broken. In this core every case here writes one character: `Paint::piece`
/// hands a run of up to `SHORT_TEXT` (two) bytes to `Paint::glyph`, and
/// `put_one` passes `Ａ` to `write.rs`'s `glyph` and so to `Paint::glyph`, or
/// under `cell-table` straight to `Paint::indexed`. Every case lands before
/// the row's written mark, so all four end in `View::put_glyph`. A run of
/// three or more takes `Paint::blit` into `View::put_ascii_run`, whose own
/// orphan repair this test does not reach.
#[test]
fn overwriting_half_a_wide_character_clears_the_other_half() {
    const BASE: &str = "\x1b[1;1H日本語ab";
    // `Ａ` is U+FF21, fullwidth: a *wide* character landing on a tail orphans
    // the lead behind it and the tail of whatever it covers, both at once.
    let cases: &[(&str, &str, &str)] = &[
        ("ASCII over a lead", "\x1b[1;1HX", "X.本#語#ab.."),
        ("ASCII over a tail", "\x1b[1;2HX", ".X本#語#ab.."),
        // Exactly covers the pair it replaces, so nothing is orphaned: the
        // rule is "clear what is cut", not "clear a neighbour".
        ("wide over a lead", "\x1b[1;1H\u{ff21}", "Ａ#本#語#ab.."),
        ("wide over a tail", "\x1b[1;2H\u{ff21}", ".Ａ#.語#ab.."),
    ];
    for (name, seq, want) in cases {
        let bytes = format!("{BASE}{seq}");
        let got = wide_row(bytes.as_bytes(), 10);
        assert_eq!(&got, want, "{name}: got {got:?}, want {want:?}");
        let cells: Vec<char> = got.chars().collect();
        for (i, c) in cells.iter().enumerate() {
            if *c == '#' {
                assert!(
                    i > 0 && cells[i - 1] != '#' && cells[i - 1] != '.',
                    "{name}: orphaned wide tail at column {i} in {got:?}"
                );
            }
        }
    }
}
