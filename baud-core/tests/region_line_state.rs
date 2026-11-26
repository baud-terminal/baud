//! **A line's per-row state moves with it in a region, on both screens.**
//!
//! The sibling of `mixed_line_movement_agrees_with_an_independent_model`
//! (`tests/vt100/scrolling.rs`): the same independent `Vec`-of-lines model,
//! applied by the definitions of the operations with no knowledge of how
//! baud stores a row, now carrying what each line holds besides its text --
//! its line size (DECDWL, DECDHL), whether it wraps onto the next, the
//! shell's prompt marks on it (the block's first row, a later line's, the
//! input's) and, on the primary, the cells it holds past a narrower edge
//! -- and run on both screens through every mover a region has: SU and SD,
//! IL and DL, IND, NEL and RI at the region's edge, a top region's scroll
//! both ways it is made, and the prompt walking down to the last row.
//!
//! What the model does, and where it comes from:
//!
//! * **Line size travels with its line**, and a line a move opens is
//!   single-width: `[[source: dec-std-070]]` pp. 5-210 and 5-211, "Line and
//!   Character Renditions are scrolled with the data" and "A new
//!   single-width line with all character positions empty is inserted".
//! * **The wrap flag travels with its line**, and an opened line does not
//!   wrap.
//! * **Cells past a narrower edge travel with their line** on the primary,
//!   and an opened line holds none.
//! * **A prompt mark travels with its line.** A region's lines move as a
//!   rotation: a line that leaves at one end gives its row to the line the
//!   move opens at the other, and a mark on it names that opened line. On
//!   the primary a top region's lines go to the history and their marks
//!   with them, off the screen.
//! * **The margin bell is disarmed when its line moves**, and stays armed
//!   when a move leaves its line where it is.

mod support;

use baud_core::*;
use baud_domain::PromptAtBottom;

const ROWS: usize = 12;
const COLS: usize = 20;
/// `ROWS` by `COLS`.
const WS: WinSize = WinSize {
    ws_row: 12,
    ws_col: 20,
};

/// Which screen a run is on, and how it is entered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum On {
    Primary,
    Alternate,
}

impl On {
    fn enter(self) -> &'static [u8] {
        match self {
            On::Primary => b"",
            On::Alternate => b"\x1b[?1049h",
        }
    }
}

/// A prompt mark the shell set on a line: the block's first row, a later
/// line's, the input's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mark {
    Block,
    Later,
    Input,
}

/// One line as the model holds it.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Line {
    text: String,
    size: LineRendition,
    wrap: Wrap,
    marks: Vec<Mark>,
}

impl Line {
    fn blank() -> Line {
        Line {
            text: String::new(),
            size: LineRendition::Single,
            wrap: Wrap::Ended,
            marks: Vec::new(),
        }
    }
    /// The line a move opens in the row `self` left: empty, single-width,
    /// ending, and with the marks that named the row.
    fn opened(self) -> Line {
        Line {
            marks: self.marks,
            ..Line::blank()
        }
    }
}

/// The screen, line by line, scrolled by the book.
struct Model {
    on: On,
    lines: Vec<Line>,
    top: Row,
    bot: Row,
    cy: Row,
}

impl Model {
    /// The band `a..=b` moves up `n`: the top `n` lines leave, and lines
    /// open at the bottom. A top region's lines go to the primary's history.
    fn up_in(&mut self, a: Row, b: Row, n: Rows) {
        let (a, b) = (a.index(), b.index());
        let retires = self.on == On::Primary && a == 0;
        for _ in 0..n.index().min(b + 1 - a) {
            let gone = self.lines.remove(a);
            let opened = match retires {
                true => Line::blank(),
                false => gone.opened(),
            };
            self.lines.insert(b, opened);
        }
    }
    /// The band `a..=b` moves down `n`: lines open at the top.
    fn down_in(&mut self, a: Row, b: Row, n: Rows) {
        let (a, b) = (a.index(), b.index());
        for _ in 0..n.index().min(b + 1 - a) {
            let gone = self.lines.remove(b);
            self.lines.insert(a, gone.opened());
        }
    }
    fn up(&mut self, n: Rows) {
        self.up_in(self.top, self.bot, n);
    }
    fn down(&mut self, n: Rows) {
        self.down_in(self.top, self.bot, n);
    }
    fn insert(&mut self, n: Rows) {
        if (self.top..=self.bot).contains(&self.cy) {
            self.down_in(self.cy, self.bot, n);
        }
    }
    /// `DL` at the cursor: the band from it moves up, no line retiring.
    fn delete(&mut self, n: Rows) {
        if (self.top..=self.bot).contains(&self.cy) {
            let (a, b) = (self.cy.index(), self.bot.index());
            for _ in 0..n.index().min(b + 1 - a) {
                let gone = self.lines.remove(a);
                self.lines.insert(b, gone.opened());
            }
        }
    }
    fn region(&mut self, top: Row, bot: Row) {
        (self.top, self.bot, self.cy) = (top, bot, Row::ZERO);
    }
    fn mark_row(&self, k: Mark) -> Option<Row> {
        self.lines
            .iter()
            .position(|l| l.marks.contains(&k))
            .map(Row::from_index)
    }
}

/// What a reader of the terminal sees of one row.
fn seen_line(t: &Term<'_>, r: Row) -> (String, LineRendition, Wrap) {
    let text = t
        .row(r)
        .expect("a row on the grid")
        .iter()
        .filter_map(|c| {
            c.content
                .as_codepoint()
                .and_then(|x| char::from_u32(x.value()))
        })
        .collect::<String>()
        .trim_end()
        .to_string();
    (
        text,
        t.line_rendition(r).expect("a row on the grid"),
        t.viewport().wrapped(r),
    )
}

/// The prompt marks as the terminal reports them: the block's first row,
/// and, through a click's report, the later line's and the input's.
type Marks = (Option<Row>, Option<(Option<Row>, Option<Row>)>);

fn seen_marks(t: &Term<'_>) -> Marks {
    let click = t.prompt_click().map(|c| (c.later, c.input.map(|(r, _)| r)));
    (t.prompt_row(), click)
}

fn model_marks(m: &Model) -> Marks {
    let block = m.mark_row(Mark::Block);
    (
        block,
        block.map(|_| (m.mark_row(Mark::Later), m.mark_row(Mark::Input))),
    )
}

/// Every row and the marks against the model, after step `step`.
fn agree(t: &Term<'_>, m: &Model, step: &str) {
    for (y, want) in m.lines.iter().enumerate() {
        let (text, size, wrap) = seen_line(t, Row::from_index(y));
        assert_eq!(
            (&text, size, wrap),
            (&want.text, want.size, want.wrap),
            "{:?}, after {step}: row {y}",
            m.on
        );
    }
    assert_eq!(
        seen_marks(t),
        model_marks(m),
        "{:?}, after {step}: the prompt marks (block, (later, input))",
        m.on
    );
}

/// The screen every run starts from, with the model of it. Five line feeds
/// on the last row first, so the alternate ring has turned and a region's rows
/// straddle its end. Then:
///
/// * a text on every row, `L00` to `L11`;
/// * row 2 wraps onto row 3;
/// * row 5 is double-width, rows 7 and 8 a double-height pair;
/// * the block's first row on row 11, a later line's on row 6, the input's
///   on row 9, at a prompt that asks for clicks.
fn drawn(t: &mut Term<'_>, on: On) -> Model {
    support::feed(t, on.enter());
    support::feed(t, format!("\x1b[{ROWS};1H\n\n\n\n\n").as_bytes());
    let mut lines = vec![Line::blank(); ROWS];
    let mut b = Vec::new();
    let mut y = 0;
    while y < ROWS {
        b.extend_from_slice(format!("\x1b[{};1H", y + 1).as_bytes());
        if y == 2 {
            // Twenty cells and then the next line's: row 2 wraps.
            let wrapped = format!("L02{}", "w".repeat(COLS - 3));
            b.extend_from_slice(format!("{wrapped}L03").as_bytes());
            lines[2] = Line {
                text: wrapped,
                wrap: Wrap::Continues,
                ..Line::blank()
            };
            lines[3] = Line {
                text: "L03".into(),
                ..Line::blank()
            };
            y += 2;
            continue;
        }
        let text = format!("L{y:02}");
        b.extend_from_slice(text.as_bytes());
        let size = match y {
            5 => LineRendition::DoubleWidth,
            7 => LineRendition::DoubleTop,
            8 => LineRendition::DoubleBottom,
            _ => LineRendition::Single,
        };
        b.extend_from_slice(match size {
            LineRendition::DoubleWidth => b"\x1b#6",
            LineRendition::DoubleTop => b"\x1b#3",
            LineRendition::DoubleBottom => b"\x1b#4",
            LineRendition::Single => b"",
        });
        lines[y] = Line {
            text,
            size,
            ..Line::blank()
        };
        y += 1;
    }
    b.extend_from_slice(b"\x1b[12;1H\x1b]133;A;cl=line\x07");
    b.extend_from_slice(b"\x1b[7;1H\x1b]133;A;k=s\x07");
    b.extend_from_slice(b"\x1b[10;3H\x1b]133;B\x07");
    support::feed(t, &b);
    lines[11].marks.push(Mark::Block);
    lines[6].marks.push(Mark::Later);
    lines[9].marks.push(Mark::Input);
    Model {
        on,
        lines,
        top: Row::ZERO,
        bot: Row::from_index(ROWS - 1),
        cy: Row::from_index(9),
    }
}

/// One step of a script: bytes to feed, and what the model does with them.
type Step = (&'static str, &'static str, fn(&mut Model));

/// Every mover a region has, in a mid-screen region and in a top region
/// short enough that the alternate screen copies it and tall enough that
/// it rotates the ring and puts the rows under it back.
const SCRIPT: &[Step] = &[
    ("region 3;10", "\x1b[3;10r", |m| {
        m.region(Row::from_index(2), Row::from_index(9))
    }),
    ("SU 2", "\x1b[2S", |m| m.up(Rows::of_count(2))),
    ("SD 1", "\x1b[T", |m| m.down(Rows::of_count(1))),
    ("IL 2 on row 6", "\x1b[6;1H\x1b[2L", |m| {
        m.cy = Row::from_index(5);
        m.insert(Rows::of_count(2))
    }),
    ("DL 3 on row 6", "\x1b[3M", |m| m.delete(Rows::of_count(3))),
    ("LF at the bottom margin", "\x1b[10;1H\n", |m| {
        m.cy = Row::from_index(9);
        m.up(Rows::of_count(1))
    }),
    ("IND at the bottom margin", "\x1bD", |m| {
        m.up(Rows::of_count(1))
    }),
    ("NEL at the bottom margin", "\x1bE", |m| {
        m.up(Rows::of_count(1))
    }),
    ("RI at the top", "\x1b[3;1H\x1bM", |m| {
        m.cy = Row::from_index(2);
        m.down(Rows::of_count(1))
    }),
    ("DL 1 on row 4", "\x1b[4;1H\x1b[M", |m| {
        m.cy = Row::from_index(3);
        m.delete(Rows::of_count(1))
    }),
    ("region 1;5", "\x1b[1;5r", |m| {
        m.region(Row::from_index(0), Row::from_index(4))
    }),
    ("SU 2, a short top region", "\x1b[2S", |m| {
        m.up(Rows::of_count(2))
    }),
    ("region 1;9", "\x1b[1;9r", |m| {
        m.region(Row::from_index(0), Row::from_index(8))
    }),
    ("SU 1, a tall top region", "\x1b[S", |m| {
        m.up(Rows::of_count(1))
    }),
    (
        "LF at the bottom of a tall top region",
        "\x1b[9;1H\n",
        |m| {
            m.cy = Row::from_index(8);
            m.up(Rows::of_count(1))
        },
    ),
    ("SD 2, a top region", "\x1b[2T", |m| {
        m.down(Rows::of_count(2))
    }),
    ("IL 1 on row 1", "\x1b[1;1H\x1b[L", |m| {
        m.cy = Row::from_index(0);
        m.insert(Rows::of_count(1))
    }),
    ("DL 2 on row 3", "\x1b[3;1H\x1b[2M", |m| {
        m.cy = Row::from_index(2);
        m.delete(Rows::of_count(2))
    }),
];

/// **Per-row state moves with its line in a region, on both screens**:
/// [`drawn`], then every step of [`SCRIPT`], each checked against the
/// model, and last the prompt walking down to the last row, which only the
/// primary's full screen does.
///
/// Named break: leave the shell's marks on the ring rows they were set on
/// while a region's copy moves the text -- the alternate screen's input
/// mark stays on row 9 after SU 2 and the model has it on row 7; on the
/// primary a top region's rotation moves the block's first row off row 11
/// while the line under the region stays there.
#[test]
fn per_row_state_moves_with_its_line_in_a_region_on_both_screens() {
    for on in [On::Primary, On::Alternate] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.set_prompt_at_bottom(PromptAtBottom::On);
        let mut m = drawn(&mut t, on);
        agree(&t, &m, "the drawing");
        if on == On::Alternate {
            // The reach: the region 3;10, rows 2 to 9, crosses the ring's
            // last row.
            let (rot, ring) = (t.rotation().index(), t.ring().index());
            assert!(rot != 0, "the alternate ring has turned");
            assert!(
                (rot + 2) % ring > (rot + 9) % ring,
                "rows 2 to 9 straddle the ring's end: rotation {rot}, ring {ring}"
            );
        }
        for (name, bytes, apply) in SCRIPT {
            support::feed(&mut t, bytes.as_bytes());
            apply(&mut m);
            agree(&t, &m, name);
        }
        // The prompt walks down: the command done, the full screen, the
        // rows from 7 on erased, a new block's first row on row 5 and its
        // input on row 6. The primary moves rows 5 and 6 down over the five
        // blank rows to the last two; the alternate screen moves nothing.
        support::feed(
            &mut t,
            b"\x1b]133;D\x07\x1b[r\x1b[7;1H\x1b[J\x1b[5;1H\x1b]133;A;cl=line\x07\x1b[6;1H\x1b]133;B\x07",
        );
        for l in &mut m.lines[6..] {
            *l = Line::blank();
        }
        for l in &mut m.lines {
            l.marks.clear();
        }
        m.lines[4].marks.push(Mark::Block);
        m.lines[5].marks.push(Mark::Input);
        m.region(Row::ZERO, Row::from_index(ROWS - 1));
        if on == On::Primary {
            m.down_in(
                Row::from_index(4),
                Row::from_index(ROWS - 1),
                Rows::of_count(ROWS - 6),
            );
        }
        agree(&t, &m, "the prompt walking down");
    }
}

/// **The cells a line holds past a narrower edge go with it** on the
/// primary, through every mover of [`SCRIPT`]: thirty columns written on
/// each row at a prompt, the screen narrowed to twenty there, the script
/// run, and the screen grown back to thirty, where each row shows the
/// line the model put there whole, its ten cells past the edge included,
/// and a line a move opened is blank to its end.
///
/// Named break: leave the cells past the edge on the row a copy moved the
/// line from -- the grown screen shows a line's last ten cells on another
/// line's row.
#[test]
fn cells_past_a_narrower_edge_move_with_their_line() {
    let wide = WinSize {
        ws_row: 12,
        ws_col: 30,
    };
    let mut owned = Owned::new(&wide);
    let mut t = Term::new(owned.storage(), &wide);
    let mut b = b"\x1b]133;A\x07".to_vec();
    let mut m = Model {
        on: On::Primary,
        lines: Vec::new(),
        top: Row::ZERO,
        bot: Row::from_index(ROWS - 1),
        cy: Row::ZERO,
    };
    for y in 0..ROWS {
        let text = format!(
            "L{y:02}{}",
            char::from(b"abcdefghijkl"[y]).to_string().repeat(27)
        );
        b.extend_from_slice(format!("\x1b[{};1H{text}", y + 1).as_bytes());
        m.lines.push(Line {
            text,
            ..Line::blank()
        });
    }
    b.extend_from_slice(b"\x1b[12;1H");
    support::feed(&mut t, &b);
    // Row 11, the prompt's, is the block a change of width keeps whole.
    m.lines[11] = Line::blank();
    support::feed(&mut t, b"\x1b[12;1H\x1b[K");
    assert_eq!(t.resize(&WS), Resized::Ok);
    for (name, bytes, apply) in SCRIPT {
        support::feed(&mut t, bytes.as_bytes());
        apply(&mut m);
        assert!(
            t.past_edge_check().sound(),
            "after {name}: the past-edge flags"
        );
    }
    // A fresh block from row 1, so the growth keeps every screen row whole.
    support::feed(&mut t, b"\x1b]133;D\x07\x1b[r\x1b[1;1H\x1b]133;A\x07");
    assert_eq!(t.resize(&wide), Resized::Ok);
    for (y, want) in m.lines.iter().enumerate() {
        let (text, _, _) = seen_line(&t, Row::from_index(y));
        assert_eq!(text, want.text, "row {y} grown back to thirty");
    }
}

/// **The margin bell is disarmed when its line moves, and kept when a move
/// leaves its line where it is**, for every mover on both screens: armed
/// by a keystroke on the cursor's row, the move made, and a character
/// printed in column 72 of that row rings it only if the line is still
/// the one it was armed on.
///
/// Named break: keep the bell armed through a move of the band it lies in
/// -- every "in the band" case rings once.
#[test]
fn the_margin_bell_is_disarmed_when_its_line_moves() {
    const BELL: WinSize = WinSize {
        ws_row: 12,
        ws_col: 80,
    };
    // (what, region, cursor row (1-based), the move, whether its line moved)
    let cases: &[(&str, &str, usize, &str, bool)] = &[
        ("SU, in the band", "\x1b[3;10r", 5, "\x1b[2S", true),
        ("SU, above the band", "\x1b[3;10r", 2, "\x1b[2S", false),
        ("SD, in the band", "\x1b[3;10r", 5, "\x1b[T", true),
        ("SD, below the band", "\x1b[3;10r", 11, "\x1b[T", false),
        ("IL", "\x1b[3;10r", 5, "\x1b[L\x1b[72G", true),
        ("DL", "\x1b[3;10r", 5, "\x1b[M\x1b[72G", true),
        ("IND at the bottom margin", "\x1b[3;10r", 10, "\x1bD", true),
        (
            "NEL at the bottom margin",
            "\x1b[3;10r",
            10,
            "\x1bE\x1b[72G",
            true,
        ),
        ("RI at the top", "\x1b[3;10r", 3, "\x1bM", true),
        ("SU, a short top region", "\x1b[1;5r", 3, "\x1b[S", true),
        (
            "SU, under a short top region",
            "\x1b[1;5r",
            8,
            "\x1b[S",
            false,
        ),
        ("SU, a tall top region", "\x1b[1;9r", 3, "\x1b[S", true),
        (
            "SU, under a tall top region",
            "\x1b[1;9r",
            11,
            "\x1b[S",
            false,
        ),
    ];
    let mut bad = Vec::new();
    for on in [On::Primary, On::Alternate] {
        for (what, region, row, mv, moved) in cases {
            let rings = |with_move: bool| {
                support::with_term(&BELL, |t| {
                    support::feed(t, on.enter());
                    support::feed(t, b"\x1b[?44h\x1b[5 u");
                    support::feed(t, region.as_bytes());
                    support::feed(t, format!("\x1b[{row};72H").as_bytes());
                    assert!(t.key(KeyEvent::press(
                        KeyCode::Char(Codepoint::from_char('a')),
                        Modifiers::NONE
                    )));
                    t.clear_reply();
                    if with_move {
                        support::feed(t, mv.as_bytes());
                    }
                    support::feed(t, b"a");
                    t.margin_bells
                })
            };
            assert_eq!(rings(false), 1, "{on:?}, {what}: the control rings");
            let want = usize::from(!moved);
            let got = rings(true);
            if got != want {
                bad.push(format!("{on:?}, {what}: rang {got}, not {want}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// **The alternate screen's lines and their state survive a visit to the
/// primary under `?47`**, which keeps the alternate screen's rows: in,
/// region scrolls there, out, a scroll of the primary, and back, and every
/// row of the alternate screen is the model's, text, size and wrap.
///
/// Named break: let the primary's scroll turn the alternate ring, or leave
/// the alternate screen's rows numbered by the primary's rotation -- the
/// alternate rows come back rotated by the primary's line feeds.
#[test]
fn the_alternate_screen_keeps_its_lines_through_a_visit_to_the_primary() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    support::feed(&mut t, b"\x1b[?47h");
    let mut m = drawn(&mut t, On::Alternate);
    // The marks are the shell's, and a return under `?47` starts its state
    // fresh: only the rows are compared here.
    for (_, bytes, apply) in &SCRIPT[..10] {
        support::feed(&mut t, bytes.as_bytes());
        apply(&mut m);
    }
    support::feed(&mut t, b"\x1b[r\x1b[?47l");
    support::feed(
        &mut t,
        format!("\x1b[{ROWS};1H\n\n\n\x1b[2;5r\x1b[S\x1b[r").as_bytes(),
    );
    support::feed(&mut t, b"\x1b[?47h");
    for (y, want) in m.lines.iter().enumerate() {
        let (text, size, wrap) = seen_line(&t, Row::from_index(y));
        assert_eq!(
            (&text, size, wrap),
            (&want.text, want.size, want.wrap),
            "the alternate screen's row {y} after the visit"
        );
    }
}
