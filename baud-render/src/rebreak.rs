//! **A finished paragraph drawn at its words**: the display's own wrap of
//! a paragraph no program can still write to, where the terminal's wrap
//! cut it at the last column.
//!
//! **What a reader sees.** With `wrap_words = "on"`, the default, a word
//! that does not fit at the end of a line moves whole to the next one, so
//! no word is split across two lines. A word wider than the whole line is
//! the one exception: it breaks at the line's edge, UAX #14's emergency
//! break (`[[source: uax14]]` section 3, "line breaks may be placed with
//! no regard to the ordinary line breaking behavior"). In a paragraph that
//! mixes directions, a run of the other direction that the wrap would
//! split -- English the wrap would cut in a Hebrew paragraph -- is given
//! lines of its own instead, laid out in its own direction against its own
//! language's edge, and the paragraph's language resumes on a fresh line
//! after it. A run the wrap breaks before rather than inside stays inline,
//! as a word does: one word, or several that fit whole on the next line
//! (`bd/owner-runs`' B and own-direction panels).
//!
//! **Where the breaks come from.** Every break is a UAX #14 break
//! opportunity under one disclosed tailoring for paths
//! (`baud_domain::line_breaks_in_paths`: a path such as `/home/jordan`
//! breaks before its separators, `/home` then `/jordan`, and moves to the
//! next line whole after a space; outside a path every opportunity is the
//! default algorithm's, which passes `LineBreakTest.txt`), and the choice
//! among them is the greedy one: as
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
//! margins are no part of its paragraph), a paragraph holding an SDS or SRS string, one
//! longer than [`crate::CONTEXT`] codepoints, and one with a row holding
//! text past the live width keep the terminal's cut. The last is a row a
//! narrowing kept whole -- the shell's block while it waits at a marked
//! prompt, whose rows it will redraw (`Term::resize`), finished once the
//! line typed there runs -- which keeps its cells past the new edge to
//! show again: read from the cells the window shows, the paragraph's text
//! would join each row's first part to the next row's and draw words the
//! program never wrote. The output above the block is re-cut by the
//! narrowing and holds nothing past the edge.
//!
//! **Where the extra lines go.** A paragraph drawn on more lines than it
//! is stored in pushes the text above it up: the window's last row stays
//! where it is, so the cursor, the prompt and every row a program can
//! address keep their rows, and the lines pushed above the window are read
//! by scrolling back, as any line above it is. At the top of what the
//! history keeps, where there is nothing above to scroll to, the oldest
//! re-broken paragraphs in the window keep the terminal's cut until the
//! rest fits, so no line is pushed where it cannot be read. Spaces a
//! program wrote after its last word -- an underlined header padded past
//! the width -- are laid on lines of their own, as the terminal laid them,
//! where hanging them would leave the paragraph on fewer lines than it is
//! stored in ([`unhang`]). A paragraph whose words still need fewer lines
//! keeps the terminal's cut, so the window's last row stays its last and
//! no line made only of rows above the window is drawn in the gap, where a
//! mouse report, which names the window's rows, could not name it.
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
    LineBreak, LineRendition, PackedCell, PastEdge, Row, RowExtent, RowRot, RowStyles, RowWritten,
    Rows, Stride, Viewport, Wrap, WrappedAt, is_table_rule, line_breaks_in_paths,
};

use crate::frozen::Words;
use crate::select::WindowLine;
use crate::units::Tally;

/// **Whether the display wraps a finished paragraph at its words**, the
/// `wrap_words` setting: on by default; off draws every row as the
/// terminal cut it.
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

/// **Trailing spaces laid on lines of their own, as the terminal laid
/// them**, where hanging them leaves a paragraph on fewer lines than it is
/// stored in (`want`): the spaces a program wrote after its last word --
/// an underlined header padded past the width, a coloured bar -- are its
/// text, and hung past the edge their rendition is not drawn. So the last
/// line's spaces after its last word fill it to the edge, and the rest go
/// on lines of their own, each as wide as the line, until the paragraph
/// has the lines it is stored in; what is left hangs on the last. Lines are
/// left as they are where its last line is a run's or it has too few
/// spaces to reach `want`.
fn unhang(lines: &mut Vec<Line>, text: &[char], spans: &[Span], cols: Cols, want: Rows) {
    let (cols, want) = (cols.index().max(1), want.index());
    let Some(&last) = lines.last() else {
        return;
    };
    if last.kind != Kind::Para {
        return;
    }
    let mut out: Vec<Line> = Vec::new();
    let (mut from, mut w) = (last.from, 0usize);
    let mut t = last.from;
    while t < last.to && lines.len() - 1 + out.len() + 1 < want {
        let s = spans[t].cols().0;
        if w + s > cols {
            if !hangs(text[t]) {
                // A cut inside the words: only spaces are moved.
                return;
            }
            out.push(Line {
                from,
                to: t,
                kind: Kind::Para,
                piece: Piece::Continues,
            });
            from = t;
            w = 0;
        }
        w += s;
        t += 1;
    }
    if out.is_empty() {
        return;
    }
    out.push(Line {
        from,
        to: last.to,
        kind: Kind::Para,
        piece: last.piece,
    });
    if lines.len() - 1 + out.len() != want {
        return;
    }
    lines.pop();
    lines.extend(out);
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
    /// Whether the row holds text past the live width
    /// (`Viewport::text_past_edge`): its cells here are not all its text.
    past: PastEdge,
    window: Option<Row>,
    /// The table its cells name their rendition ids in (`Viewport::styles`).
    styles: RowStyles<'t>,
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
    /// **The direction each line the display lays out runs**, by the same
    /// index as `used`: a line of a paragraph drawn at its words, at its
    /// paragraph's level or its moved run's direction; none for a line
    /// drawn as a stored row.
    runs: Vec<Option<BidiDirection>>,
    /// **The direction of each line that continues a paragraph the display
    /// re-wrapped**, by the same index as `used`: every line of such a
    /// paragraph but its first, the line [`crate::wrapmark`] marks; none
    /// for a paragraph's first line and a line drawn as a stored row.
    continues: Vec<Option<BidiDirection>>,
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
    /// **The stored lines each display line's cells come from**, by the same
    /// index as `used`, as runs of its columns in their logical order: what
    /// a reader of a cell's rendition reads it through, since a line that
    /// joins two rows names each one's ids in its own table
    /// (`Viewport::styles`).
    sources: Vec<Vec<(core::ops::Range<usize>, WindowLine)>>,
    /// **For each stored window row, whether its paragraph is drawn at its
    /// words or as the terminal cut it**, wherever its lines are placed:
    /// in the window or pushed above it.
    rows_drawn: Vec<Rebroken>,
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
    /// its last row, its live rows in `live`.
    pub fn window<'s>(&'s self, live: baud_domain::LiveTable<'s>) -> Viewport<'s> {
        let total = self.used.len();
        Viewport::new(
            &self.cells,
            &self.used,
            Stride::of_cells(self.cols.index()),
            self.cols,
            self.rows,
            Rows::of_index(total),
            RowRot::NONE.advance(self.above.index(), Rows::of_index(total)),
            live,
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
        let r = Self::stored_of(at);
        let at = match self.starts.iter().rposition(|&(s, _)| s <= r) {
            Some(i) => self.starts[i],
            None => match self.starts.first() {
                Some(&first) => first,
                None => return 0,
            },
        };
        at.1 - at.0
    }

    /// Stored row `at` counted from the window's top, negative above it.
    fn stored_of(at: crate::renderer::Anchor) -> isize {
        match at {
            crate::renderer::Anchor::At(y) => isize::try_from(y.index()).unwrap_or(isize::MAX),
            crate::renderer::Anchor::Above(n) => -isize::try_from(n.index()).unwrap_or(isize::MAX),
        }
    }

    /// **The direction of the line stored row `at` is drawn on**, the line
    /// [`Display::shift`] moves it to, where the display lays that line out
    /// at its words: its paragraph's level, or its moved run's direction.
    /// None where the line is drawn as a stored row, or not held: that
    /// row's own paragraph says then.
    pub(crate) fn line_direction(&self, at: crate::renderer::Anchor) -> Option<BidiDirection> {
        let line = Self::stored_of(at) + self.shift(at);
        let k = usize::try_from(line + isize::try_from(self.above.index()).ok()?).ok()?;
        self.runs.get(k).copied().flatten()
    }

    /// **Where the cells of display line `y` of the window name their ids**,
    /// a line above the window or below it by its place in
    /// [`Display::window`]'s rows (negative above): runs of its columns in
    /// their logical order, each through the table its stored line names
    /// them in, for a display laid out at `back` over `term`.
    pub(crate) fn line_sources<'t>(
        &self,
        term: &'t Term<'_>,
        back: Rows,
        y: isize,
    ) -> Vec<(core::ops::Range<usize>, RowStyles<'t>)> {
        let k = y + isize::try_from(self.above.index()).unwrap_or(0);
        let Some(runs) = usize::try_from(k).ok().and_then(|k| self.sources.get(k)) else {
            return Vec::new();
        };
        runs.iter()
            .map(|(r, l)| (r.clone(), line_styles(term, back, self.rows, *l)))
            .collect()
    }

    /// **The direction of display line `at` where it continues a
    /// paragraph the display re-wrapped at its words**, a row of the
    /// window or a line that many above its first: every line of such a
    /// paragraph but its first. None for a paragraph's first line, a line
    /// drawn as the terminal cut it, and a line the display does not hold.
    pub(crate) fn continues(&self, at: crate::renderer::Anchor) -> Option<BidiDirection> {
        let k = match at {
            crate::renderer::Anchor::At(y) => self.above.index().checked_add(y.index()),
            crate::renderer::Anchor::Above(n) => self.above.index().checked_sub(n.index()),
        }?;
        self.continues.get(k).copied().flatten()
    }

    /// Whether any cell of the display's window lines, or of the lines
    /// above and below them, comes from a row the core froze
    /// (`Viewport::styles`).
    pub(crate) fn any_frozen(&self, term: &Term<'_>, back: Rows) -> bool {
        term.view(back).any_frozen()
            && self
                .sources
                .iter()
                .flatten()
                .any(|(_, l)| !line_styles(term, back, self.rows, *l).is_live())
    }

    /// The stored row display row `y` is a copy of, where it is one.
    pub fn same(&self, y: Row) -> Option<Row> {
        self.same.get(y.index()).copied().flatten()
    }

    /// **Whether stored window row `r`'s paragraph is drawn as the
    /// terminal cut it**, its lines in the window or pushed above it: what
    /// a copy in drawn order (`?96`) asks of the window that shows it.
    pub(crate) fn keeps_cut(&self, r: Row) -> bool {
        self.rows_drawn.get(r.index()) != Some(&Rebroken::Yes)
    }

    /// Paragraphs re-broken, runs given lines of their own, and words
    /// broken at a line's edge, in the window's layout.
    pub fn counts(&self) -> (Tally, Tally, Tally) {
        (self.rebroken, self.moved, self.emergency)
    }
}

/// **Which table stored line `l` names its cells' rendition ids in**, a
/// line counted from the top of the window `rows` tall `back` lines into the
/// history: a window row's, or a line's above or below it, as `term`
/// holds it (`Viewport::styles`). A line `term` no longer holds is live.
pub(crate) fn line_styles<'t>(
    term: &'t Term<'_>,
    back: Rows,
    rows: Rows,
    l: WindowLine,
) -> RowStyles<'t> {
    if let Some(r) = l.row(rows) {
        return term.view(back).styles(r);
    }
    if let Some(k) = WindowLine::of(Row::ZERO).since(l) {
        let want = Rows::of_index(back.index() + k.index());
        return match term.looking_back(want) == want {
            true => term.view(want).styles(Row::ZERO),
            false => crate::frozen::live(term),
        };
    }
    let Some(last) = rows.index().checked_sub(1).map(Row::from_index) else {
        return crate::frozen::live(term);
    };
    match l.since(WindowLine::of(last)) {
        Some(k) if k.index() <= back.index() => term
            .view(Rows::of_count(back.index() - k.index()))
            .styles(last),
        _ => crate::frozen::live(term),
    }
}

/// **The working state the display's layout keeps between frames**: the
/// algorithm's, and the last layout under the seal its rows were read under
/// (`kept::Laid`), so a frame that wrote no row it read is answered
/// without laying anything out again or reading a cell.
pub struct Breaker {
    bidi: Box<Bidi<{ crate::CONTEXT }>>,
    last: Option<kept::Laid>,
    /// What the asks did, for the tests in this file, which run without the
    /// `counters` feature: counted from the same numbers as the counters.
    #[cfg(test)]
    asked: kept::Asked,
}

impl Default for Breaker {
    fn default() -> Self {
        Breaker {
            bidi: Box::new(Bidi::new()),
            last: None,
            #[cfg(test)]
            asked: kept::Asked::default(),
        }
    }
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

/// The mark of row `k` above the window `back` lines up, if the history
/// holds it.
fn up_mark(term: &Term<'_>, back: Rows, k: Rows) -> Option<RowExtent> {
    let want = Rows::of_index(back.index() + k.index());
    (term.looking_back(want) == want).then(|| mark_of(&term.view(want), Row::ZERO))
}

/// The mark of row `k` below the window `back` lines up, if it is on the
/// live screen.
fn down_mark(term: &Term<'_>, back: Rows, k: Rows) -> Option<RowExtent> {
    let nrows = term.view(back).rows().index();
    (k.index() <= back.index()).then(|| {
        let v = term.view(Rows::of_count(back.index() - k.index()));
        mark_of(&v, Row::from_index(nrows - 1))
    })
}

/// The stand-in an erased cell between characters is given UAX #9: U+200B
/// ZERO WIDTH SPACE, of class BN, as [`crate::bidi`] gives it.
const ERASED: char = '\u{200B}';

/// A stored row's mark as a view reads it: its paragraph properties, the
/// side margins it wrapped at, its rendition and whether it wraps.
pub(crate) fn mark_of(vp: &Viewport<'_>, r: Row) -> RowExtent {
    let cols = vp.cols();
    let e = RowExtent::of(cols.index())
        .with_bidi(vp.bidi(r))
        .with_rendition(vp.rendition(r), cols);
    let e = match vp.margins(r) {
        Some(m) => e.with_margins(m),
        None => e,
    };
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
    let key = kept::Key {
        at: term.row_gens(back).window(),
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
    // **The last layout, where no row it read was written since the seal
    // it was read under** ([`kept::Laid::answer`]): four compares while no
    // surface was written, one generation a row read while one was. A
    // string's bounds are its cells' words, which only a stamped store
    // moves, so a screen that has begun one is answered the same way.
    if let Some(laid) = work.last.as_mut() {
        let (answer, read) = laid.answer(term, &key);
        crate::counters::add(
            crate::counters::Counter::GensRead,
            crate::counters::Site::Rebreak,
            read,
        );
        #[cfg(test)]
        work.asked.gens.add(read);
        if let kept::Answer::Holds(shown) = answer {
            return shown;
        }
    }
    let shown = place_window(term, &vp, back, key, work);
    crate::counters::add(
        crate::counters::Counter::DisplaysLaid,
        crate::counters::Site::Rebreak,
        Tally::of(1),
    );
    #[cfg(test)]
    work.asked.laid.bump();
    // How far it read past the window, kept with it: the rows above and
    // below that its groups' walks reached, the one that ended each walk
    // included.
    let budget = Rows::of_index(budget);
    let reach = kept::Reach {
        up: reach_up(term, back, budget),
        down: reach_down(term, back, &vp, budget),
    };
    work.last = Some(kept::Laid::new(term, key, reach, shown.clone()));
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
    rows: &[(&[PackedCell], RowExtent, PastEdge)],
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
            .map(|(p, &(cells, used, past))| Source {
                cells,
                used,
                past,
                window: Some(Row::from_index(p)),
                styles: crate::frozen::live(term),
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

/// **How many rows above the window its first paragraph's context reads**:
/// those [`place_window`]'s walk up reads, the one that ends the walk
/// included, as many as the budget holds and one more.
fn reach_up(term: &Term<'_>, back: Rows, budget: Rows) -> Rows {
    let mut n = 0;
    while let Some(mark) = up_mark(term, back, Rows::of_count(n + 1)) {
        n += 1;
        if !mark.wrapped() || n > budget.index() {
            break;
        }
    }
    Rows::of_count(n)
}

/// **How many rows below the window its last paragraph's context reads**,
/// counted as [`reach_up`] counts them: none where the window's last row
/// ends its paragraph.
fn reach_down(term: &Term<'_>, back: Rows, vp: &Viewport<'_>, budget: Rows) -> Rows {
    let last = Row::from_index(vp.rows().index() - 1);
    if vp.wrapped(last) != Wrap::Continues {
        return Rows::ZERO;
    }
    let mut n = 0;
    while let Some(mark) = down_mark(term, back, Rows::of_count(n + 1)) {
        n += 1;
        if !mark.wrapped() || n > budget.index() {
            break;
        }
    }
    Rows::of_count(n)
}

/// **The window's display**, laid out: [`lay_out`]'s work past its cache.
fn place_window(
    term: &Term<'_>,
    vp: &Viewport<'_>,
    back: Rows,
    key: kept::Key,
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
                    past: v.text_past_edge(Row::ZERO),
                    window: None,
                    styles: v.styles(Row::ZERO),
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
                past: vp.text_past_edge(row),
                window: Some(row),
                styles: vp.styles(row),
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
                    past: v.text_past_edge(at),
                    window: None,
                    styles: v.styles(at),
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
            || row.past == PastEdge::Held
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
            if strings
                && crate::bidi::string_of(term, Words::Row(row.styles), Col::at_index(c), cell)
                    != baud_domain::StringId::NONE
            {
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
    // UAX #14 over the text, each erased cell a space, with the path
    // tailoring: a path breaks before its separators.
    let spaced: Vec<char> = g
        .text
        .iter()
        .map(|&c| match c {
            ERASED => ' ',
            c => c,
        })
        .collect();
    let mut breaks = vec![LineBreak::Prohibited; spaced.len() + 1];
    line_breaks_in_paths(&spaced, |p, b| breaks[p] = b);
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
    let (mut lines, broken) = break_lines(&spaced, &spans, &breaks, &runs, cols);
    if lines.len() < g.rows.len() {
        unhang(
            &mut lines,
            &spaced,
            &spans,
            cols,
            Rows::of_index(g.rows.len()),
        );
    }
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
        runs: vec![None; total],
        continues: vec![None; total],
        from: vec![None; nrows * n],
        to: vec![None; nrows * n],
        past: (Rows::ZERO, Rows::ZERO),
        past_to: Vec::new(),
        same: vec![None; nrows],
        sources: vec![Vec::new(); total],
        rows_drawn: vec![Rebroken::No; nrows],
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
                if let Some(l) = lines_of[gi][*r] {
                    d.sources[k].push((0..m, l));
                }
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
                    if let Some(l) = lines_of[gi][a.row.0] {
                        match d.sources[k].last_mut() {
                            Some((run, at)) if *at == l && run.end == x => run.end = x + w,
                            _ => d.sources[k].push((x..x + w, l)),
                        }
                    }
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
                d.runs[k] = Some(dir);
                if li > 0 {
                    d.continues[k] = Some(dir);
                }
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
    for g in groups {
        for r in g.rows.iter().filter_map(|s| s.window) {
            if let Some(at) = d.rows_drawn.get_mut(r.index()) {
                *at = g.rebroken;
            }
        }
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

/// **The last layout, answered through the write generations of the rows
/// it read** (FS-C3). The cache this replaced compared every cell and mark
/// of the window and of each row its context reached, on every ask, up to
/// four a frame; this asks nothing while no surface was written since the
/// seal the layout was read under, and each row's generation once a seal
/// while one was.
///
/// The fields are private to this module, so a kept display is reached
/// only through [`Laid::answer`], which reads the generations: an answer
/// from the key alone cannot be written in [`super::lay_out`].
mod kept {
    use std::rc::Rc;

    use baud_core::Term;
    use baud_domain::{ClusterEpoch, Cols, Row, Rows, Seal, WindowAt};

    use super::{Display, Strings, Top};
    use crate::units::Tally;

    /// **What a layout depended on beyond the cells and marks of the rows
    /// it read**: which physical rows the window shows, how far it looks
    /// back, its size and finished rows, the cluster slots its cells name,
    /// whether a string was ever begun, and whether its top is the first
    /// line the history keeps.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct Key {
        /// The window's physical rows (`Term::row_gens`): a scroll under a
        /// window held back moves it one ring row and writes none of the
        /// rows it then shows.
        pub(super) at: WindowAt,
        pub(super) back: Rows,
        pub(super) rows: Rows,
        pub(super) cols: Cols,
        pub(super) live: isize,
        pub(super) epoch: ClusterEpoch,
        pub(super) strings: Strings,
        pub(super) top: Top,
    }

    /// How far a layout's context read past the window: the rows above it
    /// and below it, the row that ended each walk included.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct Reach {
        pub(super) up: Rows,
        pub(super) down: Rows,
    }

    /// What an ask of the last layout found.
    pub(super) enum Answer {
        /// It holds: the display it kept, none where it drew every row as
        /// stored.
        Holds(Option<Rc<Display>>),
        /// It was laid out for another key, or a row it read was written:
        /// the window is laid out again.
        Again,
    }

    /// Whether a row a layout read was written after its seal.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Written {
        Yes,
        No,
    }

    /// **The last layout**: its key, the seal its rows were read under, how
    /// far its context read, and what it showed.
    pub(super) struct Laid {
        key: Key,
        under: Seal,
        reach: Reach,
        shown: Option<Rc<Display>>,
    }

    impl Laid {
        /// A layout just made for `key`, its rows read under
        /// `term.sealed()`.
        pub(super) fn new(
            term: &Term<'_>,
            key: Key,
            reach: Reach,
            shown: Option<Rc<Display>>,
        ) -> Laid {
            Laid {
                key,
                under: term.sealed(),
                reach,
                shown,
            }
        }

        /// **The kept display, where it holds for `key` now**, and how many
        /// rows' generations the answer read. Another key lays out again.
        /// Then, while no surface was written since the seal the layout is
        /// kept under, it holds, and no row is read. Otherwise each row the
        /// layout read is asked whether it was written since, and the first
        /// that was lays out again.
        ///
        /// Where none was, the layout is kept under the seal a reader names
        /// now, `term.sealed()`, so the next ask reads no row: every row it
        /// read carries a generation at or below the old seal's, which is
        /// at or below the new one's, and a store to any of them after this
        /// carries a later one. A store since the frame's last seal carries
        /// the open generation, which the new seal does not cover, so an
        /// ask before the frame seals reads the rows again -- the safe
        /// direction, and the reason each driver seals before it asks.
        pub(super) fn answer(&mut self, term: &Term<'_>, key: &Key) -> (Answer, Tally) {
            if self.key != *key {
                return (Answer::Again, Tally::ZERO);
            }
            if !term.wrote_since(&self.under) {
                return (Answer::Holds(self.shown.clone()), Tally::ZERO);
            }
            let (read, written) = self.reread(term);
            if written == Written::Yes {
                return (Answer::Again, read);
            }
            self.under = term.sealed();
            (Answer::Holds(self.shown.clone()), read)
        }

        /// Each row the layout read, asked whether it was written after
        /// `under` -- the window's top down, then the rows above it, then
        /// those below -- until one was: how many were asked, and whether
        /// one was written. A row above that the history no longer holds
        /// reads as written.
        fn reread(&self, term: &Term<'_>) -> (Tally, Written) {
            let back = self.key.back;
            let rows = self.key.rows.index();
            let window = term.row_gens(back);
            if let Some(y) = (0..rows).find(|&y| window.after(Row::from_index(y), &self.under)) {
                return (Tally::of(y + 1), Written::Yes);
            }
            let up = self.reach.up.index();
            for k in 1..=up {
                let want = Rows::of_index(back.index() + k);
                if term.looking_back(want) != want {
                    return (Tally::of(rows + k - 1), Written::Yes);
                }
                if term.row_gens(want).after(Row::ZERO, &self.under) {
                    return (Tally::of(rows + k), Written::Yes);
                }
            }
            let last = Row::from_index(rows.saturating_sub(1));
            let down = self.reach.down.index();
            for k in 1..=down {
                let Some(below) = back.index().checked_sub(k) else {
                    return (Tally::of(rows + up + k - 1), Written::Yes);
                };
                if term
                    .row_gens(Rows::of_count(below))
                    .after(last, &self.under)
                {
                    return (Tally::of(rows + up + k), Written::Yes);
                }
            }
            (Tally::of(rows + up + down), Written::No)
        }
    }

    /// **What one breaker's asks did**, for the tests in this file, which
    /// run without the `counters` feature: the layouts made past the cache
    /// and the rows whose generation a key read, counted from the numbers
    /// [`crate::counters::Counter::DisplaysLaid`] and
    /// [`crate::counters::Counter::GensRead`] are counted from.
    #[cfg(test)]
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub(super) struct Asked {
        pub(super) laid: Tally,
        pub(super) gens: Tally,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::{BytePos, ReadLen, Seal, WinSize, line_breaks};

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

    const WS: WinSize = WinSize {
        ws_col: 40,
        ws_row: 10,
    };

    /// The window's rows.
    const ROWS: usize = 10;

    /// A paragraph the terminal's wrap cuts inside a word, "do|g" at the
    /// fortieth column, which the display draws at its words on the same
    /// two lines, "dog" moved whole to the second.
    const PARAGRAPH: &str = "a quick brown fox jumps over the lazy dog and then runs away";

    fn feed(t: &mut Term<'_>, bytes: &[u8]) {
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    }

    /// **A screen whose display is held**, after `first`: a status line
    /// the host writes, the paragraph on rows 3 and 4, and the shell's
    /// prompt mark at row 10, column 1, which finishes every row above it.
    /// No history: the window's context reads no row past it.
    fn held(first: &str) -> Vec<u8> {
        format!("{first}\x1b[2$~\x1b[3;1H{PARAGRAPH}\x1b[10;1H\x1b]133;A\x07").into_bytes()
    }

    /// What `b`'s asks did since `then`: the layouts made, and the rows
    /// whose generation a key read.
    fn since(b: &Breaker, then: kept::Asked) -> (u64, u64) {
        (
            b.asked.laid.value() - then.laid.value(),
            b.asked.gens.value() - then.gens.value(),
        )
    }

    /// One ask of `b` for the live window, as a frame's readers ask it:
    /// what it did, and its answer.
    fn ask(t: &Term<'_>, b: &mut Breaker) -> ((u64, u64), Option<Rc<Display>>) {
        let then = b.asked;
        let shown = lay_out(t, Rows::ZERO, WordWrap::On, b);
        (since(b, then), shown)
    }

    /// **One frame as `App::present` asks**: the writes sealed, then a
    /// frame's four asks -- the images', the wrap marks', the status
    /// marks' and the layout's.
    fn frame(t: &mut Term<'_>, b: &mut Breaker) -> ((u64, u64), Option<Rc<Display>>) {
        let _ = t.seal_writes();
        let then = b.asked;
        let mut shown = None;
        for _ in 0..4 {
            shown = lay_out(t, Rows::ZERO, WordWrap::On, b);
        }
        (since(b, then), shown)
    }

    /// Whether a row of the live window was written after `seal`.
    fn window_written(t: &Term<'_>, seal: &Seal) -> bool {
        let g = t.row_gens(Rows::ZERO);
        (0..ROWS).any(|y| g.after(Row::from_index(y), seal))
    }

    /// The held screen, its first frame laid out, and the breaker.
    fn laid_out(owned: &mut Owned, first: &str, f: impl FnOnce(&mut Term<'_>, &mut Breaker)) {
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &held(first));
        let mut b = Breaker::default();
        let (did, shown) = frame(&mut t, &mut b);
        assert!(shown.is_some(), "the control: the display is held");
        assert_eq!(did, (1, 0), "the first frame lays it out once");
        assert!(
            t.looking_back(Rows::of_index(1)) != Rows::of_index(1),
            "no history above the window"
        );
        f(&mut t, &mut b);
    }

    /// **U1: a frame with nothing written lays out nothing and reads no
    /// row's generation** (FS-C3): the first frame lays the display out
    /// once for its four asks, and every frame after answers all four from
    /// the seal alone. The control: the display is held. Named break: an
    /// answer that never holds lays out on each ask, four a frame.
    #[test]
    fn a_frame_with_nothing_written_lays_out_and_reads_nothing() {
        let mut owned = Owned::new(&WS);
        laid_out(&mut owned, "", |t, b| {
            for k in 2..=4 {
                let (did, shown) = frame(t, b);
                assert!(shown.is_some(), "frame {k}: the display is held");
                assert_eq!(did, (0, 0), "frame {k}");
            }
        });
    }

    /// **U2: a write outside what the display read is read once a frame**:
    /// the status line written, the next frame reads each window row's
    /// generation once -- the window is the live screen, with no history
    /// above it and nothing below, so its context read no other row -- and
    /// lays nothing out; the frame after reads nothing. The isolation: the
    /// status line was written after the seal, and no window row was.
    /// Named break: the layout not kept again under the seal, which reads
    /// every row on every ask, four times a frame, and again the frame
    /// after.
    #[test]
    fn a_write_outside_what_the_display_read_is_read_once_a_frame() {
        let mut owned = Owned::new(&WS);
        laid_out(&mut owned, "", |t, b| {
            let seal = t.sealed();
            feed(t, b"\x1b[1$}X\x1b[0$}");
            assert!(
                t.status_gen().is_some_and(|s| s.after(&seal)) && !window_written(t, &seal),
                "the isolation: the status line written, and no window row"
            );
            let (did, shown) = frame(t, b);
            assert!(shown.is_some(), "the display is held");
            assert_eq!(did, (0, u64::from(WS.ws_row)), "the frame after the write");
            assert_eq!(frame(t, b).0, (0, 0), "the frame after that");
        });
    }

    /// **U3: a write in a row the display read lays it out again, once a
    /// frame**: a cell written in the paragraph's second row, the window's
    /// fourth; the next frame reads the window's generations down to it,
    /// four, and lays the display out once, a new one; the frame after
    /// does neither. Named breaks: an answer that never holds lays out four
    /// times; a key answered without its rows' generations lays out none
    /// and keeps the display the write left stale.
    #[test]
    fn a_write_in_a_row_the_display_read_lays_it_out_once_a_frame() {
        let mut owned = Owned::new(&WS);
        laid_out(&mut owned, "", |t, b| {
            let (_, before) = ask(t, b);
            let before = before.expect("the display is held");
            let seal = t.sealed();
            feed(t, b"\x1b[4;5HX");
            assert!(
                t.row_gens(Rows::ZERO).after(Row::from_index(3), &seal),
                "the isolation: the paragraph's second row written"
            );
            let (did, after) = frame(t, b);
            let after = after.expect("the display is held");
            assert_eq!(did, (1, 4), "the frame after the write");
            assert!(!Rc::ptr_eq(&before, &after), "a display laid out again");
            assert_eq!(frame(t, b).0, (0, 0), "the frame after that");
        });
    }

    /// **U4: an ask before the frame's seal reads the rows again after
    /// it**: the status line written, two asks before the seal each read
    /// every window row -- each keeps the layout under the seal a reader on
    /// `&Term` can name, which the write is after -- the first ask after
    /// the seal reads them once more and keeps it past the write, and the
    /// next reads none. The over-report a driver that asks before it seals
    /// pays, and why every driver seals first (`caret_frame.rs`,
    /// `every_driver_seals_once_a_frame_before_it_builds`).
    #[test]
    fn an_ask_before_the_seal_reads_again_after_it() {
        let rows = u64::from(WS.ws_row);
        let mut owned = Owned::new(&WS);
        laid_out(&mut owned, "", |t, b| {
            feed(t, b"\x1b[1$}X\x1b[0$}");
            assert_eq!(ask(t, b).0, (0, rows), "the first ask before the seal");
            assert_eq!(ask(t, b).0, (0, rows), "the second ask before the seal");
            let _ = t.seal_writes();
            assert_eq!(ask(t, b).0, (0, rows), "the first ask after it");
            assert_eq!(ask(t, b).0, (0, 0), "the next");
        });
    }

    /// **U5: a string opened and closed outside what the display read
    /// keeps it** (the strings' control, lifted with the bypass that laid
    /// out every ask once a string was begun): on a screen that began a
    /// string before its first layout, an SDS opened and closed on the
    /// status line; the next frame reads the window's rows once, lays
    /// nothing out and answers the display it kept. The isolation: the
    /// status line written, and no window row. That it is drawn as a fresh
    /// layout draws it is `stale_display.rs`'s.
    #[test]
    fn a_string_opened_outside_what_the_display_read_keeps_it() {
        let mut owned = Owned::new(&WS);
        laid_out(&mut owned, "\x1b[1;1H\x1b[1]abc\x1b[0]", |t, b| {
            assert!(t.strings_seen(), "a string was begun before the layout");
            let (_, before) = ask(t, b);
            let before = before.expect("the display is held");
            let seal = t.sealed();
            feed(t, b"\x1b[1$}\x1b[1]abc\x1b[0]\x1b[0$}");
            assert!(
                t.status_gen().is_some_and(|s| s.after(&seal)) && !window_written(t, &seal),
                "the isolation: the status line written, and no window row"
            );
            let (did, after) = frame(t, b);
            assert_eq!(did, (0, u64::from(WS.ws_row)), "the frame after the string");
            let after = after.expect("the display is held");
            assert!(Rc::ptr_eq(&before, &after), "the display kept");
        });
    }
}
