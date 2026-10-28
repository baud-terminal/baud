//! **A finished paragraph drawn at its words**: the display's own wrap of
//! a paragraph no program can still write to, where the terminal's wrap
//! cut it at the last column.
//!
//! **What a reader sees.** With `wrap_words = "on"`, the default, a word
//! that does not fit at the end of a line moves whole to the next one, so
//! no word is split across two lines (the owner, 2026-10-27: "when we do
//! wrapping, we do not break words , the word drop for the next line").
//! A word wider than the whole line is the one exception: it breaks at the
//! line's edge, UAX #14's emergency break (`[[source: uax14]]` section 3,
//! "line breaks may be placed with no regard to the ordinary line breaking
//! behavior"). In a paragraph that mixes directions, a run of the other
//! direction that the wrap would split -- English the wrap would cut in a
//! Hebrew paragraph -- is given lines of its own instead, laid out in its
//! own direction against its own language's edge, and the paragraph's
//! language resumes on a fresh line after it. A run the wrap breaks before
//! rather than inside stays inline, as a word does: one word, or several
//! that fit whole on the next line (the owner, 2026-10-27, on
//! `bd/owner-runs`' B and own-direction panels: "this is what i like").
//!
//! **Where the breaks come from.** Every break is a UAX #14 break
//! opportunity (`baud_domain::line_breaks`, which passes
//! `LineBreakTest.txt`), and the choice among them is the greedy one: as
//! many words as fit, a line's trailing spaces left to hang past the edge
//! (UAX #14 leaves the selection "in the domain of higher level software",
//! section 1). An erased cell between characters is given to the breaker
//! as a space: blank on the screen, and a place a line may end. A run's
//! ends snap to the opportunity before its first character and after its
//! last, so the comma after "middle" stays with "middle".
//!
//! **What is never re-broken.** Text a program can still write to: the
//! alternate screen, the line being typed, the rows of a command still
//! running, and, without the shell's `OSC 133` marks, every row of the
//! live screen. A finished paragraph is one wholly above the latest prompt
//! the shell marked while it waits at that prompt, or wholly in the
//! history. A table row (a box-drawing rule), a line not single-width, a
//! row DECRLM wrote, a row that wrapped at DECSLRM side margins
//! (`baud_domain::WrappedAt`; by baud's reading its cells outside the
//! margins are no part of its paragraph), a paragraph holding an SDS or SRS string, and one
//! longer than [`crate::CONTEXT`] codepoints keep the terminal's cut.
//!
//! **Where the extra lines go.** A paragraph drawn on more lines than it
//! is stored in pushes the text above it up: the window's last row stays
//! where it is, so the cursor, the prompt and every row a program can
//! address keep their rows, and the lines pushed above the window are read
//! by scrolling back, as any line above it is. At the top of what the
//! history keeps, where there is nothing above to scroll to, the oldest
//! re-broken paragraphs in the window keep the terminal's cut until the
//! rest fits, so no line is pushed where it cannot be read. A paragraph
//! whose words need fewer lines than it is stored in -- trailing spaces
//! the terminal wrapped onto a row of their own, hanging -- keeps the
//! terminal's cut, so the window's last row stays its last and no line
//! made only of rows above the window is drawn in the gap, where a mouse
//! report, which names the window's rows, could not name it.
//!
//! **Cells of lines past the window.** The window's first line can hold
//! cells of its first paragraph's rows above the window, and, while the
//! window looks back, its last line cells of its last paragraph's rows
//! below it. A selection, a find mark and a pointer that selects name each
//! by its stored line counted from the window's top
//! ([`crate::WindowLine`], [`Display::stored_line`]), so every cell drawn
//! can be selected and marked; a mouse report names the nearest window
//! cell, since a program addresses no line past the window.
//!
//! **What does not change.** The grid keeps what the program wrote: the
//! cursor, a cursor position report, a copy (the stored text in logical
//! order, soft wraps joined, as before), a selection's ends and a mouse
//! report all name stored cells, and the display maps each to where it is
//! drawn ([`Display`]). `baud-core` is untouched: this is layout, read off
//! the window once a frame, never on the path that writes the grid.

use std::rc::Rc;

use baud_core::{BidiSupport, PromptLine, Screen, Term, Zone};
use baud_domain::{
    Autodetect, Bidi, BidiClass, BidiDirection, BidiLevel, BidiProps, CharacterPath, Col, Cols,
    LineBreak, LineRendition, PackedCell, Row, RowExtent, RowRot, RowWritten, Rows, Stride,
    Viewport, Wrap, WrappedAt, is_table_rule, line_breaks,
};

use crate::select::WindowLine;
use crate::units::Tally;

/// **Whether the display wraps a finished paragraph at its words**, the
/// `wrap_words` setting: on by default (the owner, 2026-10-27: "and warp
/// should be default on"); off draws every row as the terminal cut it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WordWrap {
    #[default]
    On,
    Off,
}

/// How many columns one codepoint of a paragraph's text takes: none for a
/// codepoint attached to its cell's first, one or two for a cell's first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Span {
    Attached,
    One,
    Two,
}

/// A count of columns that may be none, as [`Span::cols`] gives it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Width(usize);

impl Span {
    fn cols(self) -> Width {
        Width(match self {
            Span::Attached => 0,
            Span::One => 1,
            Span::Two => 2,
        })
    }
}

/// Which way a display line runs and against which edge it sits: the
/// paragraph's, or a moved run's own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Kind {
    /// A line of the paragraph's own language, at its level.
    Para,
    /// A line of a run the wrap would split, in the run's direction.
    Run(BidiDirection),
}

/// Whether a display line continues onto the next as the same piece: the
/// paragraph's text across a word wrap, or a run's across its own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Piece {
    Continues,
    Ends,
}

/// **One display line**: the logical range of the paragraph's text it
/// shows, which way it runs, and whether the piece goes on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Line {
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) kind: Kind,
    pub(crate) piece: Piece,
}

/// What [`break_lines`] did that a reader of its lines cannot see.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Broken {
    /// Words wider than the whole line, broken at its edge.
    pub(crate) emergency: Tally,
    /// Runs given lines of their own.
    pub(crate) moved: Tally,
}

/// Whether a codepoint hangs past a line's end: a space, the stand-in an
/// erased cell is given.
fn hangs(c: char) -> bool {
    c == ' '
}

/// **The paragraph's text broken into display lines `cols` wide**: a
/// greedy fill at UAX #14's opportunities in `breaks` (one more than the
/// text: `breaks[p]` is a break allowed before `text[p]`), a line's
/// trailing spaces left out of its width, a word wider than the line
/// broken at its edge between cells, and each run in `runs` the fill would
/// split -- its first word fitting the rest of the line and the whole run
/// not, or the run wider than a line -- given lines of its own in its
/// direction, the paragraph resuming on a fresh line after it.
pub(crate) fn break_lines(
    text: &[char],
    spans: &[Span],
    breaks: &[LineBreak],
    runs: &[(usize, usize, BidiDirection)],
    cols: Cols,
) -> (Vec<Line>, Broken) {
    let len = text.len();
    let cols = cols.index().max(1);
    let mut broken = Broken::default();
    let mut lines: Vec<Line> = Vec::new();
    let allowed = |p: usize| p == 0 || p == len || breaks[p] != LineBreak::Prohibited;
    // The width of `a..b`, and that width without its trailing spaces.
    let width = |a: usize, b: usize| spans[a..b].iter().map(|s| s.cols().0).sum::<usize>();
    let trimmed = |a: usize, b: usize| {
        let end = (a..b)
            .rev()
            .find(|&t| spans[t] != Span::Attached && !hangs(text[t]))
            .map_or(a, |t| t + 1);
        let end = (end..b).find(|&t| spans[t] != Span::Attached).unwrap_or(b);
        width(a, end)
    };
    // The next opportunity after `p`.
    let next = |p: usize| (p + 1..=len).find(|&q| allowed(q)).unwrap_or(len);
    let mut start = 0;
    let mut used = 0usize;
    let mut empty = true;
    let close = |lines: &mut Vec<Line>, from: usize, to: usize, kind: Kind, piece: Piece| {
        if to > from {
            lines.push(Line {
                from,
                to,
                kind,
                piece,
            });
        }
    };
    // Lays `a..b`, wider than a line, out cell by cell from a fresh line:
    // every full line closed, and where the rest starts answered.
    let emergency =
        |lines: &mut Vec<Line>, a: usize, b: usize, kind: Kind, broken: &mut Broken| -> usize {
            broken.emergency.bump();
            let mut from = a;
            let mut w = 0usize;
            let mut t = a;
            while t < b {
                let s = spans[t].cols().0;
                if w + s > cols && t > from && !hangs(text[t]) {
                    close(lines, from, t, kind, Piece::Continues);
                    from = t;
                    w = 0;
                }
                w += s;
                t += 1;
                while t < b && spans[t] == Span::Attached {
                    t += 1;
                }
            }
            from
        };
    let mut p = 0;
    let mut runs = runs.iter().peekable();
    while p < len {
        // A run starting here is laid out as one unit.
        while runs.peek().is_some_and(|r| r.0 < p) {
            runs.next();
        }
        if let Some(&&(s, e, dir)) = runs.peek().filter(|r| r.0 == p) {
            runs.next();
            let need = trimmed(s, e);
            let fits = match empty {
                true => need <= cols,
                false => used + need <= cols,
            };
            if fits {
                used = match empty {
                    true => width(s, e),
                    false => used + width(s, e),
                };
                empty = false;
                p = e;
                continue;
            }
            // **A run whose first word misses the line** is broken before,
            // not inside: where it fits a whole line it drops to the next
            // one inline, as a word does, the paragraph going on after it.
            let first = next(s).min(e);
            if !empty && used + trimmed(s, first) > cols && need <= cols {
                close(&mut lines, start, s, Kind::Para, Piece::Continues);
                start = s;
                used = width(s, e);
                p = e;
                continue;
            }
            // **The wrap would split it**: the paragraph's line closed,
            // the run on lines of its own, the paragraph on a fresh one.
            broken.moved.bump();
            if !empty {
                close(&mut lines, start, s, Kind::Para, Piece::Ends);
            }
            let kind = Kind::Run(dir);
            let mut from = s;
            let mut w = 0usize;
            let mut q = s;
            let mut line_empty = true;
            while q < e {
                let r = next(q).min(e);
                let need = trimmed(q, r);
                if !line_empty && w + need > cols {
                    close(&mut lines, from, q, kind, Piece::Continues);
                    from = q;
                    w = 0;
                    line_empty = true;
                }
                if line_empty && need > cols {
                    from = emergency(&mut lines, q, r, kind, &mut broken);
                    w = width(from, r);
                } else {
                    w += width(q, r);
                }
                line_empty = false;
                q = r;
            }
            close(&mut lines, from, e, kind, Piece::Ends);
            start = e;
            used = 0;
            empty = true;
            p = e;
            continue;
        }
        let q = next(p);
        // A word ends where the next run starts, if it starts first.
        let q = runs.peek().map_or(q, |r| q.min(r.0.max(p + 1)));
        let need = trimmed(p, q);
        if !empty && used + need > cols {
            close(&mut lines, start, p, Kind::Para, Piece::Continues);
            start = p;
            used = 0;
            empty = true;
        }
        if empty && need > cols {
            start = emergency(&mut lines, p, q, Kind::Para, &mut broken);
            used = width(start, q);
        } else {
            used += width(p, q);
        }
        empty = false;
        p = q;
    }
    close(&mut lines, start, len, Kind::Para, Piece::Ends);
    (lines, broken)
}

/// **The runs a paragraph's other direction holds**: each maximal stretch
/// of its text above the paragraph's level that holds a strong letter of
/// the other direction -- so a number in a Hebrew sentence, level 2 and no
/// letter, is no run -- its start moved back to the opportunity at or
/// before it and its end on to the one at or after it, runs the moves join
/// merged, each with its first strong letter's direction.
pub(crate) fn foreign_runs(
    text: &[char],
    levels: &[BidiLevel],
    paragraph: BidiLevel,
    breaks: &[LineBreak],
) -> Vec<(usize, usize, BidiDirection)> {
    let len = text.len();
    let allowed = |p: usize| p == 0 || p == len || breaks[p] != LineBreak::Prohibited;
    let rtl = paragraph.direction() == BidiDirection::Rtl;
    let strong = |c: char| match BidiProps::of(c).class {
        BidiClass::L => Some(BidiDirection::Ltr),
        BidiClass::R | BidiClass::AL => Some(BidiDirection::Rtl),
        _ => None,
    };
    let foreign = match rtl {
        true => BidiDirection::Ltr,
        false => BidiDirection::Rtl,
    };
    let mut out: Vec<(usize, usize, BidiDirection)> = Vec::new();
    let mut i = 0;
    while i < len {
        if levels[i].number() <= paragraph.number() {
            i += 1;
            continue;
        }
        let s = i;
        while i < len && levels[i].number() > paragraph.number() {
            i += 1;
        }
        if !text[s..i].iter().any(|&c| strong(c) == Some(foreign)) {
            continue;
        }
        let dir = text[s..i]
            .iter()
            .find_map(|&c| strong(c))
            .unwrap_or(foreign);
        let a = (0..=s).rev().find(|&p| allowed(p)).unwrap_or(0);
        let b = (i..=len).find(|&p| allowed(p)).unwrap_or(len);
        match out.last_mut() {
            Some(last) if last.1 > a => last.1 = last.1.max(b),
            _ => out.push((a, b, dir)),
        }
    }
    out
}

/// One row of a paragraph as the display reads it: its cells, its mark,
/// and its row in the window, if it is one.
#[derive(Clone, Copy)]
struct Source<'t> {
    cells: &'t [PackedCell],
    used: RowExtent,
    window: Option<Row>,
}

/// Where a codepoint of a paragraph's text came from: its source row and
/// column, and how many columns it takes.
#[derive(Clone, Copy)]
struct At {
    row: SourceIx,
    col: Col,
    span: Span,
}

/// A source row's place among its paragraph's rows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SourceIx(usize);

/// A display line before it is placed: a source row as it is, or a range
/// of a re-broken paragraph's text.
#[derive(Clone)]
enum Drawn {
    Stored(usize),
    Text(Line),
}

/// One paragraph of the window: its source rows, from the first above the
/// window to the last below it, and the lines it is drawn on.
struct Group<'t> {
    rows: Vec<Source<'t>>,
    lines: Vec<Drawn>,
    text: Vec<char>,
    at: Vec<At>,
    level: BidiLevel,
    rebroken: Rebroken,
    broken: Broken,
}

impl Group<'_> {
    /// Draws the paragraph as the terminal cut it.
    fn keep_cut(&mut self) {
        self.rebroken = Rebroken::No;
        self.lines = (0..self.rows.len()).map(Drawn::Stored).collect();
    }
}

/// Whether a paragraph is drawn on lines of its own or as stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rebroken {
    No,
    Yes,
}

/// **The window as the display draws it**: each display line's cells in
/// their logical order and its mark, the lines above and below the window
/// a layout reads as its paragraphs' context, and the map between a stored
/// cell of the window and the display cell it is drawn in.
#[derive(Debug, PartialEq, Eq)]
pub struct Display {
    cols: Cols,
    rows: Rows,
    /// The grid's lines above the window's first.
    above: Rows,
    cells: Vec<PackedCell>,
    used: Vec<RowExtent>,
    /// For each window display cell, the stored cell it shows: a window
    /// row's, or one of a line past the window that a paragraph runs on
    /// from or into.
    from: Vec<Option<(WindowLine, Col)>>,
    /// For each stored window cell, the window display cell it is drawn in.
    to: Vec<Option<(Row, Col)>>,
    /// **The lines past the window drawn on its lines**: how many above its
    /// top and below its last row, and for each of their cells, by line
    /// and column, the window display cell it is drawn in -- the
    /// reverse of `from` for those lines, so a mark of one is found by
    /// index.
    past: (Rows, Rows),
    past_to: Vec<Option<(Row, Col)>>,
    /// For each window display line drawn as a stored row, that row.
    same: Vec<Option<Row>>,
    /// **Where each stored row of the window's paragraphs starts**, the
    /// rows above the window and below it included: its row counted from
    /// the window's top, negative above it, and the display line its first
    /// cell is drawn on, counted the same way; in stored order.
    starts: Vec<(isize, isize)>,
    rebroken: Tally,
    moved: Tally,
    emergency: Tally,
}

impl Display {
    /// The window's display lines, in their logical order, as a view: its
    /// rows above the window read by [`Viewport::back`], those below past
    /// its last row.
    pub fn window(&self) -> Viewport<'_> {
        let total = self.used.len();
        Viewport::new(
            &self.cells,
            &self.used,
            Stride::of_cells(self.cols.index()),
            self.cols,
            self.rows,
            Rows::of_index(total),
            RowRot::NONE.advance(self.above.index(), Rows::of_index(total)),
        )
    }

    /// The grid's lines above the window and below it.
    pub(crate) fn around(&self) -> (Rows, Rows) {
        let below = self.used.len() - self.above.index() - self.rows.index();
        (self.above, Rows::of_count(below))
    }

    /// The stored window cell display column `x` of display row `y` shows,
    /// in its logical order; none for padding, or a cell from a row
    /// outside the window ([`Display::stored_line`] names that one).
    pub fn stored(&self, y: Row, x: Col) -> Option<(Row, Col)> {
        let (l, c) = self.stored_line(y, x)?;
        l.row(self.rows).map(|r| (r, c))
    }

    /// **The stored cell display column `x` of display row `y` shows**, in
    /// its logical order, by its line counted from the window's top: a
    /// window row's, or a line's past the window that a paragraph runs on
    /// from or into; none for padding.
    pub fn stored_line(&self, y: Row, x: Col) -> Option<(WindowLine, Col)> {
        let n = self.cols.index();
        if x.index() >= n {
            return None;
        }
        self.from.get(y.index() * n + x.index()).copied().flatten()
    }

    /// **The display cell stored cell `(l, c)` is drawn in**, by its
    /// logical column, for a line of the window or one past it: none where
    /// it is not drawn on a window line.
    pub fn shown_line(&self, l: WindowLine, c: Col) -> Option<(Row, Col)> {
        if let Some(r) = l.row(self.rows) {
            return self.shown(r, c);
        }
        let n = self.cols.index();
        if c.index() >= n {
            return None;
        }
        let (above, below) = self.past;
        let last = WindowLine::of(Row::from_index(self.rows.index() - 1));
        let i = match WindowLine::of(Row::ZERO).since(l) {
            Some(k) => above.index().checked_sub(k.index())?,
            None => {
                let k = l.since(last)?.index();
                (k <= below.index()).then(|| above.index() + k - 1)?
            }
        };
        self.past_to.get(i * n + c.index()).copied().flatten()
    }

    /// The display cell stored cell `(r, c)` of the window is drawn in, by
    /// its logical column; none where it is pushed out of the window or
    /// hangs past a line's end.
    pub fn shown(&self, r: Row, c: Col) -> Option<(Row, Col)> {
        let n = self.cols.index();
        if c.index() >= n {
            return None;
        }
        self.to.get(r.index() * n + c.index()).copied().flatten()
    }

    /// The display row stored row `r` of the window starts on: its own
    /// line, or the line its first drawn cell is on.
    pub fn row_of(&self, r: Row) -> Option<Row> {
        if let Some(y) = self.same.iter().position(|&s| s == Some(r)) {
            return Some(Row::from_index(y));
        }
        let n = self.cols.index();
        (0..n).find_map(|c| self.shown(r, Col::at_index(c)).map(|(y, _)| y))
    }

    /// **How many lines the display moves stored row `at` by**, a row of
    /// the window or one that many lines above it: down where positive, up
    /// where negative. A row of the window's paragraphs moves
    /// to the line its first cell is drawn on; a row above them all moves
    /// as the first of them does, since the display pushes the text above
    /// the window up as a block, as a scroll moves it; a row the layout
    /// gave no line of its own, as the nearest row above it that has one.
    pub(crate) fn shift(&self, at: crate::renderer::Anchor) -> isize {
        let r = match at {
            crate::renderer::Anchor::At(y) => isize::try_from(y.index()).unwrap_or(isize::MAX),
            crate::renderer::Anchor::Above(n) => -isize::try_from(n.index()).unwrap_or(isize::MAX),
        };
        let at = match self.starts.iter().rposition(|&(s, _)| s <= r) {
            Some(i) => self.starts[i],
            None => match self.starts.first() {
                Some(&first) => first,
                None => return 0,
            },
        };
        at.1 - at.0
    }

    /// The stored row display row `y` is a copy of, where it is one.
    pub fn same(&self, y: Row) -> Option<Row> {
        self.same.get(y.index()).copied().flatten()
    }

    /// Paragraphs re-broken, runs given lines of their own, and words
    /// broken at a line's edge, in the window's layout.
    pub fn counts(&self) -> (Tally, Tally, Tally) {
        (self.rebroken, self.moved, self.emergency)
    }
}

/// **The working state the display's layout keeps between frames**: the
/// algorithm's, and the last layout with what it read, so a frame whose
/// window and context are the last frame's is answered without laying
/// anything out again.
pub struct Breaker {
    bidi: Box<Bidi<{ crate::CONTEXT }>>,
    last: Option<Kept>,
}

impl Default for Breaker {
    fn default() -> Self {
        Breaker {
            bidi: Box::new(Bidi::new()),
            last: None,
        }
    }
}

/// What a layout depended on beyond the rows it read: the window's place,
/// size and finished rows, and the tables its cells name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Key {
    back: Rows,
    rows: Rows,
    cols: Cols,
    live: isize,
    epoch: baud_domain::ClusterEpoch,
    strings: Strings,
    top: Top,
}

/// Whether an SDS or SRS string has been begun.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Strings {
    Unseen,
    Seen,
}

/// Whether the window's top is the first line the history keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Top {
    Below,
    First,
}

/// **The last layout and every row it read**: the window's, and the
/// rows above and below it its context reached, each as it was read, with
/// the next one past them, which said where the context stopped.
struct Kept {
    key: Key,
    window: Vec<PackedCell>,
    marks: Vec<RowExtent>,
    up: Vec<(Vec<PackedCell>, RowExtent)>,
    down: Vec<(Vec<PackedCell>, RowExtent)>,
    shown: Option<Rc<Display>>,
}

/// Row `k` above the window `back` lines up, if the history holds it.
fn up_row<'t>(term: &'t Term<'_>, back: Rows, k: usize) -> Option<(&'t [PackedCell], RowExtent)> {
    let want = Rows::of_index(back.index() + k);
    (term.looking_back(want) == want).then(|| {
        let v = term.view(want);
        (v.row(Row::ZERO), mark_of(&v, Row::ZERO))
    })
}

/// Row `k` below the window `back` lines up, if it is on the live screen.
fn down_row<'t>(term: &'t Term<'_>, back: Rows, k: usize) -> Option<(&'t [PackedCell], RowExtent)> {
    let nrows = term.view(back).rows().index();
    (k <= back.index()).then(|| {
        let v = term.view(Rows::of_count(back.index() - k));
        let at = Row::from_index(nrows - 1);
        (v.row(at), mark_of(&v, at))
    })
}

impl Kept {
    /// Whether the window and its context read now as they did.
    fn holds(&self, term: &Term<'_>, key: Key, vp: &Viewport<'_>) -> bool {
        let n = key.cols.index();
        self.key == key
            && (0..key.rows.index()).all(|y| {
                let r = Row::from_index(y);
                mark_of(vp, r) == self.marks[y]
                    && PackedCell::same_cells(vp.row(r), &self.window[y * n..(y + 1) * n])
            })
            && self.up.iter().enumerate().all(|(i, (c, m))| {
                up_row(term, key.back, i + 1)
                    .is_some_and(|(cells, mark)| mark == *m && PackedCell::same_cells(cells, c))
            })
            && self.down.iter().enumerate().all(|(i, (c, m))| {
                down_row(term, key.back, i + 1)
                    .is_some_and(|(cells, mark)| mark == *m && PackedCell::same_cells(cells, c))
            })
    }
}

/// The stand-in an erased cell between characters is given UAX #9: U+200B
/// ZERO WIDTH SPACE, of class BN, as [`crate::bidi`] gives it.
const ERASED: char = '\u{200B}';

/// A stored row's mark as a view reads it: its paragraph properties, its
/// rendition and whether it wraps.
pub(crate) fn mark_of(vp: &Viewport<'_>, r: Row) -> RowExtent {
    let cols = vp.cols();
    let e = RowExtent::of(cols.index())
        .with_bidi(vp.bidi(r))
        .with_rendition(vp.rendition(r), cols);
    match vp.wrapped(r) {
        Wrap::Continues => e.wrapping(cols.index()),
        Wrap::Ended => e,
    }
}

/// **The window line the latest prompt starts on, counted from the live
/// screen's top**: rows above it finished. The live screen's top while the
/// shell is not at a prompt, or sends no marks: only the history is
/// finished then. Negative where the prompt began above the live screen.
pub(crate) fn finished_before(term: &Term<'_>) -> isize {
    if term.primary_zone() != Zone::Prompt {
        return 0;
    }
    if let Some(r) = term.prompt_row() {
        return isize::try_from(r.index()).unwrap_or(0);
    }
    match term.prompts().last().map(|m| m.line) {
        Some(PromptLine::Back(n)) => -isize::try_from(n.index()).unwrap_or(0),
        Some(PromptLine::Screen(r)) => isize::try_from(r.index()).unwrap_or(0),
        None => 0,
    }
}

/// **The window `back` lines up as the display draws it**, or `None`
/// where every row is drawn as stored: the setting off, the alternate
/// screen, BDSM reset, or no finished paragraph in the window that wraps.
pub(crate) fn lay_out(
    term: &Term<'_>,
    back: Rows,
    wrap: WordWrap,
    work: &mut Breaker,
) -> Option<Rc<Display>> {
    if wrap == WordWrap::Off
        || term.screen() != Screen::Normal
        || term.bidi_support() != BidiSupport::Implicit
    {
        return None;
    }
    let back = term.looking_back(back);
    let vp = term.view(back);
    let (nrows, ncols) = (vp.rows().index(), vp.cols().index());
    if nrows == 0 || ncols == 0 {
        return None;
    }
    let budget = crate::CONTEXT / ncols;
    // Window row `y` is finished where its live row, `y - back`, is above
    // the latest prompt's.
    let live = finished_before(term);
    let back_i = isize::try_from(back.index()).unwrap_or(isize::MAX);
    let fin_end =
        usize::try_from((back_i + live).clamp(0, isize::try_from(nrows).unwrap_or(0))).unwrap_or(0);
    if fin_end == 0 {
        return None;
    }
    let above_wraps = || {
        let up = Rows::of_index(back.index() + 1);
        term.looking_back(up) == up && term.view(up).wrapped(Row::ZERO) == Wrap::Continues
    };
    let any =
        (0..fin_end).any(|y| vp.wrapped(Row::from_index(y)) == Wrap::Continues) || above_wraps();
    if !any {
        return None;
    }
    let top_of_history = {
        let up = Rows::of_index(back.index() + 1);
        term.looking_back(up) != up
    };
    let key = Key {
        back,
        rows: vp.rows(),
        cols: vp.cols(),
        live,
        epoch: term.cluster_epoch(),
        strings: match term.strings_seen() {
            true => Strings::Seen,
            false => Strings::Unseen,
        },
        top: match top_of_history {
            true => Top::First,
            false => Top::Below,
        },
    };
    // Not once a string has been begun: a rendition rewritten under an
    // unchanged cell can move it into or out of one.
    if let Some(kept) = work
        .last
        .as_ref()
        .filter(|k| key.strings == Strings::Unseen && k.holds(term, key, &vp))
    {
        return kept.shown.clone();
    }
    let shown = place_window(term, &vp, back, key, work);
    // What it read, kept for the next frame's compare: the window, and the
    // context rows each group reached with the one past them.
    let budget = Rows::of_index(budget);
    let (up, down) = (
        context_up(term, back, budget),
        context_down(term, back, &vp, budget),
    );
    work.last = Some(Kept {
        key,
        window: (0..nrows)
            .flat_map(|y| vp.row(Row::from_index(y)).iter().copied())
            .collect(),
        marks: (0..nrows)
            .map(|y| mark_of(&vp, Row::from_index(y)))
            .collect(),
        up,
        down,
        shown: shown.clone(),
    });
    shown
}

/// **One finished paragraph drawn at its words, alone**: its stored rows,
/// `rows`, top down, each with its mark, re-broken as the window's layout
/// re-breaks a paragraph ([`rebreak`]), its lines the display's rows and
/// each of its rows a window row, counted from its first. `None` where the
/// display draws it as stored: the alternate screen, BDSM reset, a single
/// row, a paragraph [`rebreak`] keeps, or one whose words need fewer lines
/// than it is stored in, which the window keeps at its cut. Whether it is
/// finished is the caller's to ask. What a copy in drawn order reads its
/// lines from (`?96`).
pub(crate) fn paragraph_display(
    term: &Term<'_>,
    rows: &[(&[PackedCell], RowExtent)],
    work: &mut Breaker,
) -> Option<Rc<Display>> {
    if term.screen() != Screen::Normal
        || term.bidi_support() != BidiSupport::Implicit
        || rows.len() < 2
    {
        return None;
    }
    let cols = Cols::of_index(rows[0].0.len());
    let mut g = Group {
        rows: rows
            .iter()
            .enumerate()
            .map(|(p, &(cells, used))| Source {
                cells,
                used,
                window: Some(Row::from_index(p)),
            })
            .collect(),
        lines: Vec::new(),
        text: Vec::new(),
        at: Vec::new(),
        level: BidiLevel::LTR,
        rebroken: Rebroken::No,
        broken: Broken::default(),
    };
    rebreak(term, &mut g, cols, work);
    if g.rebroken == Rebroken::No || g.lines.len() < g.rows.len() {
        return None;
    }
    let n = g.lines.len();
    let placed = Placed {
        all: (0..n).map(|li| (0, li)).collect(),
        window: (0, n),
        lost: Lost::No,
    };
    Some(Rc::new(build(&[g], &placed, cols, Rows::of_index(n))))
}

/// The rows above the window its first paragraph reaches, with the one
/// that ends the reach, as [`lay_out`] reads them.
fn context_up(term: &Term<'_>, back: Rows, budget: Rows) -> Vec<(Vec<PackedCell>, RowExtent)> {
    let mut out = Vec::new();
    let mut k = 1;
    while let Some((cells, mark)) = up_row(term, back, k) {
        out.push((cells.to_vec(), mark));
        if !mark.wrapped() || out.len() > budget.index() {
            break;
        }
        k += 1;
    }
    out
}

/// The rows below the window its last paragraph reaches, as [`lay_out`]
/// reads them.
fn context_down(
    term: &Term<'_>,
    back: Rows,
    vp: &Viewport<'_>,
    budget: Rows,
) -> Vec<(Vec<PackedCell>, RowExtent)> {
    let mut out = Vec::new();
    let last = Row::from_index(vp.rows().index() - 1);
    if vp.wrapped(last) != Wrap::Continues {
        return out;
    }
    let mut k = 1;
    while let Some((cells, mark)) = down_row(term, back, k) {
        out.push((cells.to_vec(), mark));
        if !mark.wrapped() || out.len() > budget.index() {
            break;
        }
        k += 1;
    }
    out
}

/// **The window's display**, laid out: [`lay_out`]'s work past its cache.
fn place_window(
    term: &Term<'_>,
    vp: &Viewport<'_>,
    back: Rows,
    key: Key,
    work: &mut Breaker,
) -> Option<Rc<Display>> {
    let (nrows, ncols) = (vp.rows().index(), vp.cols().index());
    let budget = crate::CONTEXT / ncols;
    let back_i = isize::try_from(back.index()).unwrap_or(isize::MAX);
    let live = key.live;
    let finished = |y: isize| y - back_i < live;
    // The window's paragraphs, top down, each with its rows above and
    // below the window.
    let mut groups: Vec<Group<'_>> = Vec::new();
    let mut y = 0;
    while y < nrows {
        let first = y;
        while y + 1 < nrows && vp.wrapped(Row::from_index(y)) == Wrap::Continues {
            y += 1;
        }
        let last = y;
        y += 1;
        let mut rows: Vec<Source<'_>> = Vec::new();
        let mut whole = true;
        if first == 0 {
            let mut k = 1;
            loop {
                let want = Rows::of_index(back.index() + k);
                if term.looking_back(want) != want {
                    break;
                }
                let v = term.view(want);
                if v.wrapped(Row::ZERO) != Wrap::Continues {
                    break;
                }
                if rows.len() >= budget {
                    whole = false;
                    break;
                }
                rows.push(Source {
                    cells: v.row(Row::ZERO),
                    used: mark_of(&v, Row::ZERO),
                    window: None,
                });
                k += 1;
            }
            rows.reverse();
        }
        for r in first..=last {
            let row = Row::from_index(r);
            rows.push(Source {
                cells: vp.row(row),
                used: mark_of(vp, row),
                window: Some(row),
            });
        }
        // Its last row's live position: below the window when it runs on.
        let mut end = isize::try_from(last).unwrap_or(0);
        if last + 1 == nrows && vp.wrapped(Row::from_index(last)) == Wrap::Continues {
            let mut k = 1;
            loop {
                if k > back.index() {
                    whole = false;
                    break;
                }
                if rows.len() >= budget {
                    whole = false;
                    break;
                }
                let v = term.view(Rows::of_count(back.index() - k));
                let at = Row::from_index(nrows - 1);
                rows.push(Source {
                    cells: v.row(at),
                    used: mark_of(&v, at),
                    window: None,
                });
                end += 1;
                if v.wrapped(at) != Wrap::Continues {
                    break;
                }
                k += 1;
            }
        }
        let mut g = Group {
            lines: Vec::new(),
            text: Vec::new(),
            at: Vec::new(),
            level: BidiLevel::LTR,
            rebroken: Rebroken::No,
            broken: Broken::default(),
            rows,
        };
        if whole && finished(end) && g.rows.len() >= 2 {
            rebreak(term, &mut g, Cols::of_index(ncols), work);
        }
        if g.rebroken == Rebroken::No {
            g.keep_cut();
        }
        groups.push(g);
    }
    let top_of_history = key.top == Top::First;
    loop {
        let placed = place(&groups, vp.rows());
        // At the top of what the history keeps a line pushed above the
        // window could not be scrolled to: the topmost re-broken paragraphs
        // there keep the stored cut until the rest fits.
        if top_of_history && placed.lost == Lost::Yes {
            let Some(g) = groups.iter_mut().find(|g| g.rebroken == Rebroken::Yes) else {
                break;
            };
            g.keep_cut();
            continue;
        }
        // The window is full when the lines from the first one holding a
        // window row fill it: no line made only of rows from above the
        // window is drawn in it. Counting every line of `all` read a first
        // paragraph that runs on from the history as filling the window,
        // and drew a history row in the gap a shrinking paragraph left.
        let first_window_line = placed
            .all
            .iter()
            .position(|&(gi, li)| {
                line_rows(&groups[gi], &groups[gi].lines[li])
                    .iter()
                    .any(Option::is_some)
            })
            .unwrap_or(0);
        if placed.window.1 >= first_window_line + nrows {
            break;
        }
        // **Fewer lines than rows**: the topmost paragraph drawn on fewer
        // lines than it is stored in keeps the stored cut, until the
        // window's last row is its last again; a row from above the window
        // drawn in the gap is one a mouse report cannot name.
        let Some(g) = groups
            .iter_mut()
            .find(|g| g.rebroken == Rebroken::Yes && g.lines.len() < g.rows.len())
        else {
            break;
        };
        g.keep_cut();
    }
    if groups.iter().all(|g| g.rebroken == Rebroken::No) {
        return None;
    }
    let placed = place(&groups, vp.rows());
    Some(Rc::new(build(
        &groups,
        &placed,
        Cols::of_index(ncols),
        vp.rows(),
    )))
}

/// **Re-breaks one finished paragraph**, or leaves it as stored where it
/// is not one the display re-breaks or where its words already fall at
/// the stored cut.
fn rebreak(term: &Term<'_>, g: &mut Group<'_>, cols: Cols, work: &mut Breaker) {
    let strings = term.strings_seen();
    for (p, row) in g.rows.iter().enumerate() {
        if row.used.rendition() != LineRendition::Single
            || row.used.bidi().written() == RowWritten::RightToLeft
            || row.used.bidi().wrapped_at() == WrappedAt::Margins
        {
            return;
        }
        let last = p + 1 == g.rows.len();
        let end = match last {
            true => row
                .cells
                .iter()
                .rposition(|c| !c.content.is_empty())
                .map_or(0, |k| k + 1),
            false => row.cells.len(),
        };
        for (c, &cell) in row.cells[..end].iter().enumerate() {
            if cell.content.is_wide_tail() {
                continue;
            }
            if strings && crate::bidi::string_of(term, cell) != baud_domain::StringId::NONE {
                return;
            }
            if cell.content.is_empty() {
                g.text.push(ERASED);
                g.at.push(At {
                    row: SourceIx(p),
                    col: Col::at_index(c),
                    span: Span::One,
                });
                continue;
            }
            let base = term
                .base_of(cell)
                .drawn()
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or('\u{FFFD}');
            if !cell.flags.is_marked() && is_table_rule(base) {
                return;
            }
            let wide = row
                .cells
                .get(c + 1)
                .is_some_and(|n| n.content.is_wide_tail());
            g.text.push(base);
            g.at.push(At {
                row: SourceIx(p),
                col: Col::at_index(c),
                span: match wide {
                    true => Span::Two,
                    false => Span::One,
                },
            });
            if cell.flags.is_marked() {
                for m in term.cluster_marks(cell) {
                    if let Some(m) = char::from_u32(m) {
                        g.text.push(m);
                        g.at.push(At {
                            row: SourceIx(p),
                            col: Col::at_index(c),
                            span: Span::Attached,
                        });
                    }
                }
            }
        }
    }
    if g.text.is_empty() || g.text.len() > crate::CONTEXT {
        return;
    }
    // Its level and its levels, as the layout resolves them: P2-P3 with
    // the first row's properties, only where a character can reorder it.
    let path = g.rows[0].used.bidi();
    let opens = g.text.iter().any(|&c| baud_domain::opens_rtl(u32::from(c)))
        || path.path() == CharacterPath::RightToLeft;
    let (level, levels) = match opens {
        true => match work.bidi.resolve(&g.text, path.paragraph()) {
            Ok(l) => (l, Some(work.bidi.levels().to_vec())),
            Err(_) => return,
        },
        false => (BidiLevel::LTR, None),
    };
    g.level = level;
    // UAX #14 over the text, each erased cell a space.
    let spaced: Vec<char> = g
        .text
        .iter()
        .map(|&c| match c {
            ERASED => ' ',
            c => c,
        })
        .collect();
    let mut breaks = vec![LineBreak::Prohibited; spaced.len() + 1];
    line_breaks(&spaced, |p, b| breaks[p] = b);
    // Never inside a cell.
    for (p, a) in g.at.iter().enumerate() {
        if a.span == Span::Attached {
            breaks[p] = LineBreak::Prohibited;
        }
    }
    let runs = match &levels {
        Some(l) => foreign_runs(&spaced, l, level, &breaks),
        None => Vec::new(),
    };
    let spans: Vec<Span> = g.at.iter().map(|a| a.span).collect();
    let (lines, broken) = break_lines(&spaced, &spans, &breaks, &runs, cols);
    // The stored cut again: drawn as stored.
    let cut: Vec<usize> = (0..g.rows.len())
        .map(|r| {
            g.at.iter()
                .position(|a| a.row.0 >= r)
                .unwrap_or(g.text.len())
        })
        .collect();
    let same = lines.len() == g.rows.len()
        && lines
            .iter()
            .zip(&cut)
            .all(|(l, &c)| l.from == c && l.kind == Kind::Para);
    if same {
        return;
    }
    g.rebroken = Rebroken::Yes;
    g.broken = broken;
    g.lines = lines.into_iter().map(Drawn::Text).collect();
}

/// Which lines of the window's paragraphs the window shows.
struct Placed {
    /// Every line, top down, as (group, line) pairs.
    all: Vec<(usize, usize)>,
    /// The index in `all` of the window's first line and one past its last.
    window: (usize, usize),
    /// Whether a line above the window holds a cell of a window row.
    lost: Lost,
}

/// Whether a window row's cell is on a line pushed above the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lost {
    No,
    Yes,
}

/// The window rows a line draws a cell of.
fn line_rows(g: &Group<'_>, d: &Drawn) -> Vec<Option<Row>> {
    match d {
        Drawn::Stored(r) => vec![g.rows[*r].window],
        Drawn::Text(l) => {
            let mut out: Vec<Option<Row>> = g.at[l.from..l.to]
                .iter()
                .map(|a| g.rows[a.row.0].window)
                .collect();
            out.dedup();
            out
        }
    }
}

/// **The window's lines, bottom-anchored**: the window ends with the line
/// that holds the last cell of its last row, and takes as many lines above
/// it as it has rows.
fn place(groups: &[Group<'_>], rows: Rows) -> Placed {
    let nrows = rows.index();
    let all: Vec<(usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(gi, g)| (0..g.lines.len()).map(move |li| (gi, li)))
        .collect();
    let last = Row::from_index(nrows - 1);
    let anchor = all
        .iter()
        .rposition(|&(gi, li)| line_rows(&groups[gi], &groups[gi].lines[li]).contains(&Some(last)))
        .unwrap_or(all.len().saturating_sub(1));
    let end = anchor + 1;
    let start = end.saturating_sub(nrows);
    let lost = match all[..start].iter().any(|&(gi, li)| {
        line_rows(&groups[gi], &groups[gi].lines[li])
            .iter()
            .any(Option::is_some)
    }) {
        true => Lost::Yes,
        false => Lost::No,
    };
    Placed {
        all,
        window: (start, end),
        lost,
    }
}

/// **Where each stored row of the window's paragraphs starts** on the
/// display ([`Display::shift`]): for every line of `placed`, each source
/// row it draws a cell of the first time one does, both counted from the
/// window's top.
fn starts(groups: &[Group<'_>], placed: &Placed) -> Vec<(isize, isize)> {
    let start = isize::try_from(placed.window.0).unwrap_or(0);
    let mut out: Vec<(isize, isize)> = Vec::new();
    let mut seen: Vec<Vec<bool>> = groups.iter().map(|g| vec![false; g.rows.len()]).collect();
    for (i, &(gi, li)) in placed.all.iter().enumerate() {
        let g = &groups[gi];
        // The group's rows are consecutive: each counted from the window's
        // top through the first that is a window row.
        let Some((p0, r0)) = g
            .rows
            .iter()
            .enumerate()
            .find_map(|(p, s)| s.window.map(|r| (p, r.index())))
        else {
            continue;
        };
        let stored = |p: usize| {
            isize::try_from(r0).unwrap_or(0) + isize::try_from(p).unwrap_or(0)
                - isize::try_from(p0).unwrap_or(0)
        };
        let shown = isize::try_from(i).unwrap_or(0) - start;
        let mut mark = |p: usize| {
            if !seen[gi][p] {
                seen[gi][p] = true;
                out.push((stored(p), shown));
            }
        };
        match &g.lines[li] {
            Drawn::Stored(p) => mark(*p),
            Drawn::Text(l) => {
                for a in &g.at[l.from..l.to] {
                    mark(a.row.0);
                }
            }
        }
    }
    out.sort_by_key(|&(s, _)| s);
    out
}

/// **The display grid**: the lines of the window's first paragraph above
/// it, the window's lines, and the lines of its last paragraph below it,
/// with the map between stored and display cells.
fn build(groups: &[Group<'_>], placed: &Placed, cols: Cols, rows: Rows) -> Display {
    let (start, end) = placed.window;
    let n = cols.index();
    let nrows = rows.index();
    // Context: the lines of the window's first and last paragraphs outside
    // it, and at least one line above for a smooth scroll's leaving row.
    let first_group = placed.all.get(start).map_or(0, |&(g, _)| g);
    let mut top = start;
    while top > 0 && placed.all[top - 1].0 == first_group {
        top -= 1;
    }
    if top == start && start > 0 {
        top = start - 1;
    }
    let last_group = placed.all.get(end.saturating_sub(1)).map_or(0, |&(g, _)| g);
    let mut bottom = end;
    while bottom < placed.all.len() && placed.all[bottom].0 == last_group {
        bottom += 1;
    }
    let lines = &placed.all[top..bottom];
    let total = lines.len().max(start - top + nrows);
    let mut d = Display {
        cols,
        rows,
        above: Rows::of_count(start - top),
        cells: vec![PackedCell::EMPTY; total * n],
        used: vec![RowExtent::of(n); total],
        from: vec![None; nrows * n],
        to: vec![None; nrows * n],
        past: (Rows::ZERO, Rows::ZERO),
        past_to: Vec::new(),
        same: vec![None; nrows],
        starts: starts(groups, placed),
        rebroken: Tally::of(
            groups
                .iter()
                .filter(|g| g.rebroken == Rebroken::Yes)
                .count(),
        ),
        moved: Tally::default(),
        emergency: Tally::default(),
    };
    // Each group's rows by line counted from the window's top, so a cell
    // of a row past the window is named on a window line as well.
    let lines_of: Vec<Vec<Option<WindowLine>>> = groups.iter().map(window_lines).collect();
    // The cells of lines past the window drawn on window lines: the line,
    // its column, and the display cell.
    let mut past: Vec<(WindowLine, Col, Row, Col)> = Vec::new();
    let last = WindowLine::of(Row::from_index(nrows - 1));
    type Maps<'m> = (
        &'m mut [Option<(WindowLine, Col)>],
        &'m mut [Option<(Row, Col)>],
    );
    let mut name = |(from, to): Maps<'_>, (y, x): (Row, usize), (l, c): (WindowLine, usize)| {
        from[y.index() * n + x] = Some((l, Col::at_index(c)));
        match l.row(rows) {
            Some(sr) => to[sr.index() * n + c] = Some((y, Col::at_index(x))),
            None => past.push((l, Col::at_index(c), y, Col::at_index(x))),
        }
    };
    for (k, &(gi, li)) in lines.iter().enumerate() {
        let g = &groups[gi];
        // The display line's window row, where it is one.
        let wy = (top + k)
            .checked_sub(start)
            .filter(|&y| y < nrows)
            .map(Row::from_index);
        let row = &mut d.cells[k * n..(k + 1) * n];
        match &g.lines[li] {
            Drawn::Stored(r) => {
                let src = &g.rows[*r];
                let m = n.min(src.cells.len());
                row[..m].copy_from_slice(&src.cells[..m]);
                d.used[k] = src.used;
                if let (Some(y), Some(sr)) = (wy, src.window) {
                    d.same[y.index()] = Some(sr);
                }
                if let (Some(y), Some(l)) = (wy, lines_of[gi][*r]) {
                    for c in 0..m {
                        name((&mut d.from, &mut d.to), (y, c), (l, c));
                    }
                }
            }
            Drawn::Text(l) => {
                if let Kind::Run(_) = l.kind {
                    d.moved.bump();
                }
                let mut x = 0;
                for t in l.from..l.to {
                    let a = g.at[t];
                    if a.span == Span::Attached {
                        continue;
                    }
                    let w = a.span.cols().0;
                    if x + w > n {
                        break;
                    }
                    let src = &g.rows[a.row.0];
                    for i in 0..w {
                        let c = a.col.index() + i;
                        row[x + i] = src.cells.get(c).copied().unwrap_or(PackedCell::EMPTY);
                        if let (Some(y), Some(l)) = (wy, lines_of[gi][a.row.0]) {
                            name((&mut d.from, &mut d.to), (y, x + i), (l, c));
                        }
                    }
                    x += w;
                }
                let dir = match l.kind {
                    Kind::Para => g.level.direction(),
                    Kind::Run(dir) => dir,
                };
                let path = match dir {
                    BidiDirection::Rtl => CharacterPath::RightToLeft,
                    BidiDirection::Ltr => CharacterPath::LeftToRight,
                };
                let bidi = g.rows[0]
                    .used
                    .bidi()
                    .with_path(path)
                    .with_autodetect(Autodetect::Off);
                let e = RowExtent::of(n).with_bidi(bidi);
                d.used[k] = match l.piece {
                    Piece::Continues => e.wrapping(n),
                    Piece::Ends => e,
                };
            }
        }
    }
    for g in groups.iter().filter(|g| g.rebroken == Rebroken::Yes) {
        d.emergency.add(g.broken.emergency);
    }
    // The reverse of `from` for the lines past the window, by index.
    let top = WindowLine::of(Row::ZERO);
    let above = past
        .iter()
        .filter_map(|p| top.since(p.0))
        .max()
        .unwrap_or(Rows::ZERO);
    let below = past
        .iter()
        .filter_map(|p| p.0.since(last))
        .max()
        .unwrap_or(Rows::ZERO);
    d.past = (above, below);
    d.past_to = vec![None; (above.index() + below.index()) * n];
    for (l, c, y, x) in past {
        let i = match top.since(l) {
            Some(k) => above.index() - k.index(),
            None => l.since(last).map_or(0, |k| above.index() + k.index() - 1),
        };
        if c.index() < n {
            d.past_to[i * n + c.index()] = Some((y, x));
        }
    }
    d
}

/// **Each of a paragraph's rows by its line counted from the window's
/// top**: its rows are consecutive, so each is counted from the first that
/// is a window row; none for a group with no window row.
fn window_lines(g: &Group<'_>) -> Vec<Option<WindowLine>> {
    let Some((p0, r0)) = g
        .rows
        .iter()
        .enumerate()
        .find_map(|(p, s)| s.window.map(|r| (p, r)))
    else {
        return vec![None; g.rows.len()];
    };
    let at = WindowLine::of(r0);
    (0..g.rows.len())
        .map(|p| {
            Some(match p < p0 {
                true => at.up(Rows::of_count(p0 - p)),
                false => at.down(Rows::of_count(p - p0)),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(s: &str) -> (Vec<char>, Vec<Span>) {
        let t: Vec<char> = s.chars().collect();
        let sp = t.iter().map(|_| Span::One).collect();
        (t, sp)
    }

    fn breaks(t: &[char]) -> Vec<LineBreak> {
        let mut b = vec![LineBreak::Prohibited; t.len() + 1];
        line_breaks(t, |p, x| b[p] = x);
        b
    }

    fn texts(t: &[char], lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| t[l.from..l.to].iter().collect())
            .collect()
    }

    /// **No word is split**: a word that does not fit drops whole to the
    /// next line, its space left hanging on the line it leaves.
    #[test]
    fn a_word_that_does_not_fit_drops_whole_to_the_next_line() {
        let (t, sp) = spans("the quick brown fox");
        let (lines, b) = break_lines(&t, &sp, &breaks(&t), &[], Cols::of_index(12));
        assert_eq!(texts(&t, &lines), ["the quick ", "brown fox"]);
        assert_eq!(b.emergency, Tally::default());
    }

    /// A line's trailing space does not count against its width: "abcd "
    /// fits a line four wide.
    #[test]
    fn a_trailing_space_hangs_past_the_edge() {
        let (t, sp) = spans("abcd efgh");
        let (lines, _) = break_lines(&t, &sp, &breaks(&t), &[], Cols::of_index(4));
        assert_eq!(texts(&t, &lines), ["abcd ", "efgh"]);
    }

    /// **A word wider than the line breaks at its edge**, the one break
    /// that is not an opportunity.
    #[test]
    fn a_word_wider_than_the_line_breaks_at_its_edge() {
        let (t, sp) = spans("ab abcdefghij cd");
        let (lines, b) = break_lines(&t, &sp, &breaks(&t), &[], Cols::of_index(4));
        assert_eq!(texts(&t, &lines), ["ab ", "abcd", "efgh", "ij ", "cd"]);
        assert_eq!(b.emergency, Tally::of(1));
    }
}
