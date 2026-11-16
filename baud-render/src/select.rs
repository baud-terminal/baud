//! Selection: a range of cells over the live surface's lines, made with the
//! mouse and read back as text. This is the model, platform-free, and the extraction.
//!
//! An endpoint names an **absolute line** -- the count of rows the surface's
//! rotation had advanced when the line was the top of the live screen -- and
//! a column, so a selection survives output scrolling underneath it: the
//! line keeps its number as it moves up the ring, and a view `back` lines up
//! shows lines `scrolled - back ..`. The oldest retained line is
//! `scrolled - history`; an endpoint older than that is clipped to it.
//!
//! Each endpoint is a *span* rather than a point: a click's cell is its lead
//! and its tail, a double click's is the word, a triple click's the line.
//! The selection is then from the earliest span start to the latest span
//! end, which is how a drag after a double click extends by whole words.
//!
//! A selection is a **stream** -- contiguous in reading order -- or, with
//! Option held at the press, a **block**: every row between the two
//! endpoints, over one range of columns taken apart from the lines.

use baud_core::{SurfaceLine, Term, TextMoved, WatchedLines};
use baud_domain::{Col, Cols, Content, CopyDirection, PackedCell, Row, Rows, Viewport, Wrap};

use std::rc::Rc;

use crate::Reorder;

/// A line of the live surface, numbered from the start of the session: the
/// rotation's advance at the moment the line was the top of the screen.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct AbsLine(u64);

impl AbsLine {
    /// The line `n` rows below the session's first.
    pub fn at(n: Rows) -> AbsLine {
        AbsLine(u64::try_from(n.index()).unwrap_or(u64::MAX))
    }

    fn plus(self, rows: Rows) -> AbsLine {
        AbsLine(
            self.0
                .saturating_add(u64::try_from(rows.index()).unwrap_or(u64::MAX)),
        )
    }

    fn minus(self, rows: Rows) -> AbsLine {
        AbsLine(
            self.0
                .saturating_sub(u64::try_from(rows.index()).unwrap_or(u64::MAX)),
        )
    }

    /// The line after this one.
    pub fn below(self) -> AbsLine {
        self.plus(Rows::of_count(1))
    }

    /// The line before this one; `None` above the session's first.
    pub fn above(self) -> Option<AbsLine> {
        self.0.checked_sub(1).map(AbsLine)
    }
}

/// How far the live surface's rotation has advanced since the session
/// began: the absolute line of the live screen's top row. The driver adds
/// every advance `pump` observes, the count the scrollback window already
/// reads to stay on its content.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Scrolled(u64);

impl Scrolled {
    pub const START: Scrolled = Scrolled(0);

    pub fn advanced(self, rows: Rows) -> Scrolled {
        Scrolled(
            self.0
                .saturating_add(u64::try_from(rows.index()).unwrap_or(u64::MAX)),
        )
    }

    /// The count moved back `rows`: lines the live surface took back, so
    /// the line numbers they had name the rows they came back to. `None`
    /// past the session's first line, which a scroll down on a terminal
    /// that has retired fewer lines than it takes back reaches: no number
    /// is left for the rows it brings in.
    pub fn retreated(self, rows: Rows) -> Option<Scrolled> {
        let n = u64::try_from(rows.index()).ok()?;
        self.0.checked_sub(n).map(Scrolled)
    }

    /// Where `line` lies on the live surface whose top is this count: a row
    /// of the screen, numbered from the top, or a line of the history that
    /// many lines above it; `None` below the screen's `rows`.
    pub fn surface_line(self, line: AbsLine, rows: Rows) -> Option<SurfaceLine> {
        let gap = |a: u64, b: u64| usize::try_from(a - b).unwrap_or(usize::MAX);
        match line.0 < self.0 {
            true => Some(SurfaceLine::Above(Rows::of_count(gap(self.0, line.0)))),
            false => {
                let y = gap(line.0, self.0);
                (y < rows.index()).then(|| SurfaceLine::At(Row::from_index(y)))
            }
        }
    }

    /// The absolute line `at` names on the live surface whose top is this
    /// count: [`Scrolled::surface_line`]'s inverse.
    pub fn line_of(self, at: SurfaceLine) -> AbsLine {
        match at {
            SurfaceLine::At(y) => self.line(Rows::ZERO, y),
            SurfaceLine::Above(n) => self.top(n),
        }
    }

    /// The absolute line at the top of a view `back` lines up.
    pub fn top(self, back: Rows) -> AbsLine {
        AbsLine(self.0).minus(back)
    }

    /// The absolute line at screen row `y` of a view `back` lines up.
    pub fn line(self, back: Rows, y: Row) -> AbsLine {
        self.top(back).plus(Rows::of_count(y.index()))
    }

    /// **The row of a view `back` lines up that shows `line`**, of its
    /// `rows`; none off it.
    pub fn window_row(self, back: Rows, line: AbsLine, rows: Rows) -> Option<Row> {
        let top = self.top(back);
        let k = line.0.checked_sub(top.0)?;
        let y = usize::try_from(k).ok()?;
        (y < rows.index()).then(|| Row::from_index(y))
    }

    /// The oldest line the ring still holds, with `retained` lines of
    /// history behind the live screen.
    pub fn oldest(self, retained: Rows) -> AbsLine {
        AbsLine(self.0).minus(retained)
    }
}

/// **A stored line counted from the window's top**: a row of the window,
/// or a line that many above its top or past its last row. The display's
/// word wrap ([`crate::rebreak`]) draws cells of such lines on the window's
/// own: the rows of its first paragraph above it on its first line, and,
/// while it looks back, the rows of its last paragraph below it on its
/// last; a selection, a find mark and a pointer name them by this.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct WindowLine(isize);

impl WindowLine {
    /// Window row `r`.
    pub fn of(r: Row) -> WindowLine {
        WindowLine(isize::try_from(r.index()).unwrap_or(isize::MAX))
    }

    /// The line `n` above this one.
    pub fn up(self, n: Rows) -> WindowLine {
        WindowLine(
            self.0
                .saturating_sub(isize::try_from(n.index()).unwrap_or(isize::MAX)),
        )
    }

    /// The line `n` below this one.
    pub fn down(self, n: Rows) -> WindowLine {
        WindowLine(
            self.0
                .saturating_add(isize::try_from(n.index()).unwrap_or(isize::MAX)),
        )
    }

    /// The window row this line is, of a window `rows` tall; none above its
    /// top or past its last row.
    pub fn row(self, rows: Rows) -> Option<Row> {
        usize::try_from(self.0)
            .ok()
            .filter(|&y| y < rows.index())
            .map(Row::from_index)
    }

    /// How many lines this is below `earlier`; none above it.
    pub fn since(self, earlier: WindowLine) -> Option<Rows> {
        let d = self.0.checked_sub(earlier.0)?;
        usize::try_from(d).ok().map(Rows::of_count)
    }

    /// Absolute line `line` counted from a window whose top is `top`.
    pub fn between(top: AbsLine, line: AbsLine) -> WindowLine {
        let d = i128::from(line.0) - i128::from(top.0);
        WindowLine(isize::try_from(d).unwrap_or(match d < 0 {
            true => isize::MIN,
            false => isize::MAX,
        }))
    }

    /// The absolute line this is on a window whose top is `top`; none
    /// before the session's first.
    pub fn line(self, top: AbsLine) -> Option<AbsLine> {
        let d = i128::from(top.0) + i128::try_from(self.0).ok()?;
        u64::try_from(d).ok().map(AbsLine)
    }
}

/// **How far past the window the display may draw a stored line on it**:
/// the rows above the window that its first paragraph runs on from, and,
/// while the window looks back, the rows below it that its last paragraph
/// runs on into, each at most the paragraph's bound the layout reads
/// ([`crate::rebreak`]). Read off the terminal, so a frame asks it of the
/// window it draws rather than of the last one laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Beyond {
    pub above: Rows,
    pub below: Rows,
}

impl Beyond {
    /// The window's own rows and nothing past them.
    pub const NONE: Beyond = Beyond {
        above: Rows::ZERO,
        below: Rows::ZERO,
    };

    /// **The lines past the window `back` lines up**, as [`crate::rebreak`]
    /// reads its context: up while the row above wraps onto the next, and
    /// down from a last row that wraps, through the live screen's rows the
    /// window is short of.
    pub fn of(term: &Term<'_>, back: Rows) -> Beyond {
        let view = term.view(back);
        let (rows, cols) = (view.rows().index(), view.cols().index());
        let budget = crate::CONTEXT / cols.max(1);
        let mut above = 0;
        loop {
            let want = Rows::of_count(back.index() + above + 1);
            if above >= budget || term.looking_back(want) != want {
                break;
            }
            if term.view(want).wrapped(Row::ZERO) != Wrap::Continues {
                break;
            }
            above += 1;
        }
        let mut below = 0;
        let last = Row::from_index(rows - 1);
        if view.wrapped(last) == Wrap::Continues {
            while below < back.index() && below < budget {
                below += 1;
                let v = term.view(Rows::of_count(back.index() - below));
                if v.wrapped(last) != Wrap::Continues {
                    break;
                }
            }
        }
        Beyond {
            above: Rows::of_count(above),
            below: Rows::of_count(below),
        }
    }
}

/// A cell of the live surface, by absolute line and column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub line: AbsLine,
    pub col: Col,
}

impl Point {
    pub fn new(line: AbsLine, col: Col) -> Point {
        Point { line, col }
    }
}

impl PartialOrd for Point {
    fn partial_cmp(&self, other: &Point) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Point {
    fn cmp(&self, other: &Point) -> core::cmp::Ordering {
        self.line
            .cmp(&other.line)
            .then(self.col.index().cmp(&other.col.index()))
    }
}

/// One endpoint's extent: a cell with its wide tail, a word, or a line.
/// Both ends inclusive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    pub start: Point,
    pub end: Point,
}

impl Span {
    /// The cell at `col` of `row`, snapped so a wide character is never
    /// half-selected: a tail starts at its lead, a lead ends at its tail.
    pub fn cell(row: &[PackedCell], line: AbsLine, col: Col) -> Span {
        let start = snap_start(row, col);
        let end = snap_end(row, col);
        Span {
            start: Point::new(line, start),
            end: Point::new(line, end),
        }
    }

    /// The word under `col` of `row`: the maximal run of cells that are
    /// not empty, not whitespace and not delimiters. A cell that is none
    /// of those -- a delimiter or a blank under the pointer -- is a span
    /// of itself, which is what a double click on a space selects.
    ///
    /// `term` is what `row` was read from, in the same borrow: a cell with
    /// continuations holds its slot, and its base is read back from there.
    pub fn word(term: &Term<'_>, row: &[PackedCell], line: AbsLine, col: Col) -> Span {
        let c = col.index().min(row.len().saturating_sub(1));
        if row.is_empty() || !is_word(term, row, Col::at_index(c)) {
            return Span::cell(row, line, col);
        }
        let mut start = c;
        while start > 0 && is_word(term, row, Col::at_index(start - 1)) {
            start -= 1;
        }
        let mut end = c;
        while end + 1 < row.len() && is_word(term, row, Col::at_index(end + 1)) {
            end += 1;
        }
        Span {
            start: Point::new(line, Col::at_index(start)),
            end: Point::new(line, Col::at_index(end)),
        }
    }

    /// The line through screen row `y` of `view`: the row and every row
    /// the wrap flag joins to it, before and after, within the view.
    pub fn line(view: &Viewport<'_>, top: AbsLine, y: Row) -> Span {
        let rows = view.rows().index();
        let mut first = y.index();
        while first > 0 && matches!(view.wrapped(Row::from_index(first - 1)), Wrap::Continues) {
            first -= 1;
        }
        let mut last = y.index();
        while last + 1 < rows && matches!(view.wrapped(Row::from_index(last)), Wrap::Continues) {
            last += 1;
        }
        let end_col = Col::at_index(view.cols().index().saturating_sub(1));
        Span {
            start: Point::new(top.plus(Rows::of_count(first)), Col::ZERO),
            end: Point::new(top.plus(Rows::of_count(last)), end_col),
        }
    }

    /// **The line through stored line `line`**, read by number: the row and
    /// every row the wrap flag joins to it, before and after, each way as
    /// far as the display reads a paragraph ([`Beyond::of`]'s bound). What a
    /// triple click over a cell the display draws from a line past the
    /// window selects.
    pub fn line_at(term: &Term<'_>, scrolled: Scrolled, line: AbsLine) -> Span {
        let cols = term.geometry().0.index();
        let budget = crate::CONTEXT / cols.max(1);
        let joins = |l: AbsLine| {
            row_of(term, scrolled, l).is_some_and(|(_, w)| matches!(w, Wrap::Continues))
        };
        let mut first = line;
        for _ in 0..budget {
            match first.above() {
                Some(up) if joins(up) => first = up,
                _ => break,
            }
        }
        let mut last = line;
        for _ in 0..budget {
            if !joins(last) || row_of(term, scrolled, last.below()).is_none() {
                break;
            }
            last = last.below();
        }
        Span {
            start: Point::new(first, Col::ZERO),
            end: Point::new(last, Col::at_index(cols.saturating_sub(1))),
        }
    }
}

/// What a press selects, and what a drag after it extends by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grain {
    Cell,
    Word,
    Line,
}

/// Whether a selection runs in reading order or is a rectangle of columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Region {
    /// Contiguous, from one cell to another in row-major order.
    Stream,
    /// Every row between the endpoints, over one range of columns.
    Block,
}

/// What a left press does to the selection on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// Start a stream at the press.
    Stream,
    /// Start a block at the press, whatever is on screen: Option held.
    Block,
    /// Move the head of what is on screen, keeping its anchor, grain and
    /// region -- or start a stream when nothing is: Shift held.
    Extend,
}

/// What clipping to the ring's oldest line left of a selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Clip {
    Kept,
    Cleared,
}

/// A selection: the span the press made and the span the head is at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    anchor: Span,
    head: Span,
    grain: Grain,
    region: Region,
}

impl Selection {
    /// A stream selection pressed at `span`.
    pub fn at(span: Span, grain: Grain) -> Selection {
        Selection {
            anchor: span,
            head: span,
            grain,
            region: Region::Stream,
        }
    }

    /// A block selection pressed at `span`: Option held at the press.
    pub fn block(span: Span, grain: Grain) -> Selection {
        Selection {
            region: Region::Block,
            ..Selection::at(span, grain)
        }
    }

    /// The selection a left press leaves behind, given what was on screen:
    /// `span_at` makes the span under the pointer for a grain, so an
    /// extension takes the grain the selection was made with.
    pub fn pressed(
        current: Option<Selection>,
        press: Press,
        grain: Grain,
        span_at: impl Fn(Grain) -> Span,
    ) -> Selection {
        match (press, current) {
            (Press::Block, _) => Selection::block(span_at(grain), grain),
            (Press::Extend, Some(mut sel)) => {
                sel.extend(span_at(sel.grain));
                sel
            }
            _ => Selection::at(span_at(grain), grain),
        }
    }

    pub fn grain(&self) -> Grain {
        self.grain
    }

    pub fn region(&self) -> Region {
        self.region
    }

    /// Move the head; the anchor stays.
    pub fn extend(&mut self, head: Span) {
        self.head = head;
    }

    /// A single click that never moved: the press and the head are one
    /// cell, which is not a selection a user meant.
    pub fn is_click(&self) -> bool {
        self.grain == Grain::Cell && self.anchor == self.head
    }

    /// The range. A stream's is the earliest start to the latest end in
    /// row-major order. A block's is its two corners, the lines and the
    /// columns taken apart: the top line and the smaller start column,
    /// then the bottom line and the larger end column -- so a drag up and
    /// to the right, where the earlier point is the one further right, is
    /// not read with its columns backwards.
    pub fn ordered(&self) -> (Point, Point) {
        match self.region {
            Region::Stream => {
                let start = self.anchor.start.min(self.head.start);
                let end = self.anchor.end.max(self.head.end);
                (start, end)
            }
            Region::Block => {
                let (a, h) = (self.anchor, self.head);
                let left = a.start.col.index().min(h.start.col.index());
                let right = a.end.col.index().max(h.end.col.index());
                (
                    Point::new(a.start.line.min(h.start.line), Col::at_index(left)),
                    Point::new(a.end.line.max(h.end.line), Col::at_index(right)),
                )
            }
        }
    }

    /// Whether any line from `from` through `to` holds part of the selection.
    pub fn touches(&self, from: AbsLine, to: AbsLine) -> bool {
        let (start, end) = self.ordered();
        start.line <= to && end.line >= from
    }

    /// Move any endpoint older than `oldest` -- a line the ring has
    /// recycled -- to the oldest line: a stream's to its first column, a
    /// block's in the column it had, since a block's columns do not depend
    /// on its lines. Both gone is no selection.
    pub fn clip(&mut self, oldest: AbsLine) -> Clip {
        let (_, end) = self.ordered();
        if end.line < oldest {
            return Clip::Cleared;
        }
        let region = self.region;
        for span in [&mut self.anchor, &mut self.head] {
            for p in [&mut span.start, &mut span.end] {
                if p.line < oldest {
                    *p = match region {
                        Region::Stream => Point::new(oldest, Col::ZERO),
                        Region::Block => Point::new(oldest, p.col),
                    };
                }
            }
        }
        Clip::Kept
    }

    /// **Carry the selection with its text across a read**, by what the
    /// core's watch on its lines says (`Term::watched_lines`), the count
    /// that numbers it already moved to `scrolled`: every endpoint moved as
    /// its text moved against the count, then held to the lines that stay
    /// -- a stream's cut start to its line's first column and its cut end
    /// to `last_col`, a block's in the column it had, as [`Selection::clip`]
    /// holds one. A read that moved no rows in place moves nothing.
    pub fn follow(&mut self, scrolled: Scrolled, watched: WatchedLines, last_col: Col) {
        let (first, last) = (
            scrolled.line_of(watched.first),
            scrolled.line_of(watched.last),
        );
        let region = self.region;
        for span in [&mut self.anchor, &mut self.head] {
            for p in [&mut span.start, &mut span.end] {
                let line = match watched.moved {
                    TextMoved::Up(k) => p.line.minus(k),
                    TextMoved::Down(k) => p.line.plus(k),
                };
                *p = match (line < first, line > last, region) {
                    (true, _, Region::Stream) => Point::new(first, Col::ZERO),
                    (_, true, Region::Stream) => Point::new(last, last_col),
                    (true, _, Region::Block) => Point::new(first, p.col),
                    (_, true, Region::Block) => Point::new(last, p.col),
                    (false, false, _) => Point::new(line, p.col),
                };
            }
        }
    }

    /// Where the selection falls on a view whose top row is `top`, in the
    /// view's screen coordinates, clipped to the screen; `None` when none
    /// of it is on this screen ([`Selection::highlight_reaching`] with no
    /// line past it).
    pub fn highlight(&self, top: AbsLine, rows: Rows, cols: Cols) -> Option<Highlight> {
        self.highlight_reaching(top, rows, cols, Beyond::NONE)
    }

    /// **Where the selection falls on a view whose top row is `top`, and on
    /// the lines `beyond` it** ([`Beyond::of`]) that the display may draw
    /// on the window's own: its corners clipped to the screen, and its ends
    /// unclipped, which a cell of such a line is asked by
    /// ([`Highlight::selects_line`]); `None` when none of it is on the
    /// screen or on those lines. Selected only on lines past the window,
    /// its corners are none of the window's ([`Highlight::covers_row`]).
    pub fn highlight_reaching(
        &self,
        top: AbsLine,
        rows: Rows,
        cols: Cols,
        beyond: Beyond,
    ) -> Option<Highlight> {
        let (start, end) = self.ordered();
        let last_line = top.plus(Rows::of_count(rows.index().saturating_sub(1)));
        if end.line < top.minus(beyond.above)
            || start.line > last_line.plus(beyond.below)
            || rows.index() == 0
        {
            return None;
        }
        let last_col = Col::at_index(cols.index().saturating_sub(1));
        let col = |c: Col| Col::at_index(c.index().min(last_col.index()));
        let ends = (
            (WindowLine::between(top, start.line), col(start.col)),
            (WindowLine::between(top, end.line), col(end.col)),
        );
        if end.line < top || start.line > last_line {
            return Some(Highlight {
                from: (Row::from_index(1), Col::ZERO),
                to: (Row::ZERO, Col::ZERO),
                region: self.region,
                ends,
            });
        }
        if self.region == Region::Block {
            // Rows clipped to the view, and the same columns on every row.
            let first = start.line.max(top);
            let last = end.line.min(last_line);
            return Some(Highlight {
                from: (row_in(top, first), col(start.col)),
                to: (row_in(top, last), col(end.col)),
                region: Region::Block,
                ends,
            });
        }
        let from = if start.line < top {
            (Row::ZERO, Col::ZERO)
        } else {
            (row_in(top, start.line), col(start.col))
        };
        let to = if end.line > last_line {
            (Row::from_index(rows.index() - 1), last_col)
        } else {
            (row_in(top, end.line), col(end.col))
        };
        Some(Highlight {
            from,
            to,
            region: Region::Stream,
            ends,
        })
    }

    /// The selected text, as the spec extracts it: a wide tail contributes
    /// nothing, an empty cell before the row's last selected content is a
    /// space and after it is dropped, marks follow their base, a row the
    /// writer wrapped joins the next without a newline, and no newline
    /// ends the last row. A block reads each row over its columns alone,
    /// and every one of its row boundaries is a newline, the wrapped ones
    /// included: the writer's wrap is cut by the rectangle like any row.
    ///
    /// **Under `?96` DECRLCM** (`Term::copy_direction`) each row gives the
    /// same cells in the order they are drawn from its right edge to its
    /// left ([`drawn_from_right`]), the rows in their order: "ABCD" copies
    /// as "DCBA" (`[[source: vt510-decrlcm]]`), and a Hebrew line UAX #9
    /// draws right to left in the order it reads. With the display's wrap
    /// at its default, on ([`Selection::text_as`]).
    pub fn text(&self, term: &Term<'_>, scrolled: Scrolled) -> String {
        self.text_as(term, scrolled, crate::WordWrap::On)
    }

    /// [`Selection::text`] under the display's wrap `wrap`, the
    /// `wrap_words` setting the window is drawn with: under `?96`, a
    /// selection over a finished paragraph the display draws at its words
    /// ([`crate::rebreak`]) reads each of that paragraph's display lines,
    /// the lines as they are drawn, from its right edge -- the same cells,
    /// a line's cells drawn there whichever row stores them. A stream joins
    /// the lines as a soft wrap joins rows; a block, which breaks at every
    /// line, reads the lines it selects a cell of, a newline between each
    /// two. A
    /// paragraph the window [`Term::window_back`] shows is read as that
    /// window draws it: at the top of what the history keeps, where the
    /// window keeps a paragraph at the terminal's cut, by its stored rows.
    pub fn text_as(&self, term: &Term<'_>, scrolled: Scrolled, wrap: crate::WordWrap) -> String {
        let (start, end) = self.ordered();
        let block = self.region == Region::Block;
        let mut out = String::new();
        let mut drawn = match term.copy_direction() {
            CopyDirection::LeftToRight => None,
            CopyDirection::RightToLeft => Some(Drawn::new()),
        };
        // The last line of a paragraph found drawn as stored, so its other
        // lines are not asked again.
        let mut plain: Option<AbsLine> = None;
        let mut line = start.line;
        loop {
            let asks = wrap == crate::WordWrap::On && plain.is_none_or(|p| line > p);
            if let (Some(d), true) = (drawn.as_mut(), asks) {
                match d.rebroken(term, scrolled, line, (start, end), self.region, &mut out) {
                    Para::Drawn(last) => {
                        if last == end.line {
                            break;
                        }
                        out.push('\n');
                        line = last.plus(Rows::of_count(1));
                        continue;
                    }
                    Para::Stored(last) => plain = Some(last),
                }
            }
            if let Some((row, wrap)) = row_of(term, scrolled, line) {
                let first = if block || line == start.line {
                    start.col.index()
                } else {
                    0
                };
                let last = if block || line == end.line {
                    end.col.index().min(row.len().saturating_sub(1))
                } else {
                    row.len().saturating_sub(1)
                };
                if first <= last && !row.is_empty() {
                    let cells = &row[first..=last];
                    if let Some(used) = cells.iter().rposition(|c| !c.content.is_empty()) {
                        match drawn.as_mut() {
                            None => {
                                for cell in &cells[..=used] {
                                    push_cell(&mut out, term, *cell);
                                }
                            }
                            Some(d) => {
                                for c in
                                    d.read_right(term, scrolled, line, Cols::of_index(row.len()))
                                {
                                    let c = c.index();
                                    if c >= first && c <= first + used {
                                        push_cell(&mut out, term, row[c]);
                                    }
                                }
                            }
                        }
                    }
                }
                if line != end.line && (block || matches!(wrap, Wrap::Ended)) {
                    out.push('\n');
                }
            } else if line != end.line {
                out.push('\n');
            }
            if line == end.line {
                break;
            }
            line = line.plus(Rows::of_count(1));
        }
        out
    }
}

/// A selection on one view, in screen coordinates, both ends inclusive:
/// a stream's in row-major order, a block's its top-left and bottom-right
/// corners. What the renderers draw from. The region is part of equality,
/// so the renderer's damage repaints a switch between the two over the
/// same corners.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Highlight {
    /// The corners clipped to the window's rows: what the cell shader's
    /// uniforms carry. Where none of the window's rows is selected, `from`
    /// is after `to`, so no cell of the window is between them.
    pub from: (Row, Col),
    pub to: (Row, Col),
    pub region: Region,
    /// **The ends, unclipped**: each line counted from the window's top,
    /// above it or past its last row included, which every cell is asked
    /// by -- the window's, and a cell the display draws on a window line
    /// from a line past it.
    pub ends: ((WindowLine, Col), (WindowLine, Col)),
}

impl Highlight {
    /// A selection of the window's rows alone, from `from` to `to`.
    pub fn new(from: (Row, Col), to: (Row, Col), region: Region) -> Highlight {
        Highlight {
            from,
            to,
            region,
            ends: (
                (WindowLine::of(from.0), from.1),
                (WindowLine::of(to.0), to.1),
            ),
        }
    }

    /// Whether the cell at `(r, c)` is inside the selection's geometry.
    pub fn contains(&self, r: Row, c: Col) -> bool {
        self.contains_line(WindowLine::of(r), c)
    }

    /// Whether column `c` of stored line `l`, the window's or one past it,
    /// is inside the selection's geometry.
    pub fn contains_line(&self, l: WindowLine, c: Col) -> bool {
        let ((fl, fc), (tl, tc)) = self.ends;
        match self.region {
            Region::Stream => {
                let p = (l, c.index());
                p >= (fl, fc.index()) && p <= (tl, tc.index())
            }
            Region::Block => {
                l >= fl && l <= tl && c.index() >= fc.index() && c.index() <= tc.index()
            }
        }
    }

    /// Whether the cell at `(r, c)`, holding `content`, is drawn selected:
    /// a wide tail takes its lead's answer, so a character is never half
    /// selected where a block's edge cuts it -- one straddling the left
    /// edge is out, one straddling the right edge is in whole. **Every
    /// render path asks this**, and the cell shader's `selected()` spells
    /// the same rule; the pixel oracle compares the two.
    pub fn selects(&self, r: Row, c: Col, content: Content) -> bool {
        self.selects_line(WindowLine::of(r), c, content)
    }

    /// [`Highlight::selects`] for column `c` of stored line `l`, the
    /// window's or one past it that the display draws on a window line.
    pub fn selects_line(&self, l: WindowLine, c: Col, content: Content) -> bool {
        let c = if content.is_wide_tail() && c.index() > 0 {
            Col::at_index(c.index() - 1)
        } else {
            c
        };
        self.contains_line(l, c)
    }

    /// Whether any cell of screen row `r` is selected.
    pub fn covers_row(&self, r: Row) -> bool {
        r.index() >= self.from.0.index() && r.index() <= self.to.0.index()
    }
}

fn row_in(top: AbsLine, line: AbsLine) -> Row {
    Row::from_index(usize::try_from(line.0.saturating_sub(top.0)).unwrap_or(usize::MAX))
}

/// **Each copied row's cells in the order they are drawn, from the right
/// edge**, for `?96`: the layout every draw path reads ([`Reorder`]),
/// taken over the window that shows the line, the live screen's kept for
/// its rows; and a finished paragraph the display draws at its words, its
/// lines laid out alone ([`crate::rebreak::paragraph_display`]).
struct Drawn {
    live: Option<Reorder>,
    para: Option<(Reorder, crate::rebreak::Breaker)>,
    /// The window [`Term::window_back`] shows, as the display draws it.
    window: Option<Option<Rc<crate::rebreak::Display>>>,
}

/// What [`Drawn::rebroken`] did with a line's paragraph: read it as the
/// display draws it, through its last line read, or found it drawn as
/// stored, through its last line.
enum Para {
    Drawn(AbsLine),
    Stored(AbsLine),
}

impl Drawn {
    fn new() -> Drawn {
        Drawn {
            live: None,
            para: None,
            window: None,
        }
    }

    /// **Whether the window shown draws the paragraph of stored lines
    /// `first` to `last` as the terminal cut it**, where it shows one of
    /// them: at the top of what the history keeps, a paragraph re-broken
    /// alone could be one the window keeps at its cut. None where the
    /// window shows none of its lines.
    fn window_keeps_cut(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        (first, last): (AbsLine, AbsLine),
    ) -> Option<bool> {
        let back = term.window_back();
        let top = scrolled.top(back);
        let nrows = u64::try_from(term.view(back).rows().index()).unwrap_or(u64::MAX);
        let y = (first.0..=last.0).find_map(|l| l.checked_sub(top.0).filter(|&y| y < nrows))?;
        let y = Row::from_index(usize::try_from(y).unwrap_or(usize::MAX));
        let shown = self.window.get_or_insert_with(|| {
            let mut work = crate::rebreak::Breaker::default();
            crate::rebreak::lay_out(term, back, crate::WordWrap::On, &mut work)
        });
        // No display: the window draws every row as stored.
        Some(shown.as_ref().is_none_or(|d| d.keeps_cut(y)))
    }

    /// **The selected cells of `line`'s paragraph, line by display line**,
    /// where the display draws it at its words: each display line's cells
    /// from its right edge to its left, the cells a selection from `start`
    /// to `end` of `region` selects -- on each stored row, those before its
    /// last selected content, as the stored rows' copy takes them. A
    /// stream puts no break between the lines, as a soft wrap joins rows;
    /// a block reads the lines it selects a cell of, a newline between
    /// each two. Nothing is written where the
    /// paragraph is drawn as stored: not finished (in the history, or
    /// above the latest prompt while the shell waits at it), or one the
    /// display keeps at the terminal's cut -- the window shown, where it
    /// shows the paragraph.
    fn rebroken(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        line: AbsLine,
        (start, end): (Point, Point),
        region: Region,
        out: &mut String,
    ) -> Para {
        let Some((here, mark, past)) = marked_row(term, scrolled, line) else {
            return Para::Stored(line);
        };
        let cols = here.len().max(1);
        let budget = crate::CONTEXT / cols + 1;
        // The paragraph's rows: up while the row above wraps, down while
        // the row wraps.
        let mut first = line;
        let mut up: Vec<(&[PackedCell], baud_domain::RowExtent, baud_domain::PastEdge)> =
            Vec::new();
        while let Some(prev) = first.0.checked_sub(1).map(AbsLine) {
            match marked_row(term, scrolled, prev) {
                Some((cells, m, p)) if m.wrapped() && up.len() < budget => {
                    up.push((cells, m, p));
                    first = prev;
                }
                _ => break,
            }
        }
        up.reverse();
        let mut rows = up;
        rows.push((here, mark, past));
        let mut last = line;
        while rows.last().is_some_and(|(_, m, _)| m.wrapped()) && rows.len() <= budget {
            let next = last.plus(Rows::of_count(1));
            match marked_row(term, scrolled, next) {
                Some(r) => {
                    rows.push(r);
                    last = next;
                }
                None => break,
            }
        }
        // Finished: its last row above the latest prompt's live row.
        let live = i128::from(last.0) - i128::from(scrolled.0);
        let finished =
            live < i128::from(i64::try_from(crate::rebreak::finished_before(term)).unwrap_or(0));
        let shown_to = last.min(end.line);
        if !finished || self.window_keeps_cut(term, scrolled, (first, last)) == Some(true) {
            return Para::Stored(last);
        }
        let (order, breaker) = self
            .para
            .get_or_insert_with(|| (Reorder::new(), crate::rebreak::Breaker::default()));
        let Some(d) = crate::rebreak::paragraph_display(term, &rows, breaker) else {
            return Para::Stored(last);
        };
        order.lay_out_display(term, d.clone());
        // Each stored row's selected columns, as the stored rows' copy
        // takes them.
        let taken: Vec<Option<(usize, usize)>> = rows
            .iter()
            .enumerate()
            .map(|(p, (cells, _, _))| {
                let at = AbsLine(first.0 + u64::try_from(p).unwrap_or(u64::MAX));
                if at < start.line || at > end.line || cells.is_empty() {
                    return None;
                }
                let block = region == Region::Block;
                let from = if block || at == start.line {
                    start.col.index()
                } else {
                    0
                };
                let to = match block || at == end.line {
                    true => end.col.index().min(cells.len() - 1),
                    false => cells.len() - 1,
                };
                if from > to {
                    return None;
                }
                let used = cells[from..=to]
                    .iter()
                    .rposition(|c| !c.content.is_empty())?;
                Some((from, from + used))
            })
            .collect();
        let n = d.window(crate::frozen::live_table(term)).rows().index();
        // A block's lines are those it selects a cell of, a newline between
        // each two.
        let mut lines = 0;
        for y in 0..n {
            let y = Row::from_index(y);
            let mut selects = false;
            let mut line = String::new();
            for v in (0..cols).rev() {
                let l = order.orders().logical_col(y, Col::at_index(v));
                let Some((r, c)) = d.stored(y, l) else {
                    continue;
                };
                let (p, c) = (r.index(), c.index());
                if taken
                    .get(p)
                    .copied()
                    .flatten()
                    .is_some_and(|(a, b)| c >= a && c <= b)
                {
                    selects = true;
                    push_cell(&mut line, term, rows[p].0[c]);
                }
            }
            if region == Region::Block && selects && lines > 0 {
                out.push('\n');
            }
            if selects {
                lines += 1;
            }
            out.push_str(&line);
        }
        Para::Drawn(shown_to)
    }

    /// The logical columns of `line`, `cols` wide, from its rightmost drawn
    /// cell to its leftmost: a wide character's tail before its lead, which
    /// gives nothing ([`push_cell`]).
    fn read_right(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        line: AbsLine,
        cols: Cols,
    ) -> Vec<Col> {
        // By stored rows: a copy reads the rows as the core holds them, so
        // the display's own wrap, which moves cells between rows, is off.
        let mut own = Reorder::new();
        own.set_word_wrap(crate::WordWrap::Off);
        let (o, r) = match line.0.checked_sub(scrolled.0) {
            Some(y) => {
                let o = self.live.get_or_insert_with(|| {
                    let mut o = Reorder::new();
                    o.set_word_wrap(crate::WordWrap::Off);
                    o.lay_out(term, Rows::ZERO);
                    o
                });
                (
                    &*o,
                    Row::from_index(usize::try_from(y).unwrap_or(usize::MAX)),
                )
            }
            None => {
                let back =
                    Rows::of_count(usize::try_from(scrolled.0 - line.0).unwrap_or(usize::MAX));
                own.lay_out(term, back);
                (&own, Row::ZERO)
            }
        };
        drawn_from_right(o, r, cols)
    }
}

/// Row `r`'s logical columns, `cols` of them, in the order `o` draws them,
/// read from the right: on a row nothing reorders, its columns from the
/// last to the first.
fn drawn_from_right(o: &Reorder, r: Row, cols: Cols) -> Vec<Col> {
    (0..cols.index())
        .rev()
        .map(|v| o.orders().logical_col(r, Col::at_index(v)))
        .collect()
}

/// The cells of absolute line `line` and how the row ended, or `None`
/// when the line is past the live screen or the ring no longer holds it.
///
/// **The character under display column `col` of view row `y`**: the column
/// itself on a single-width line, and on a line that is not half of it,
/// since such a line draws each character over two columns and is addressed
/// by its characters (`[[source: dec-std-070]]` § 5.6.1, "the character in
/// column 5 ... will still be addressed as column 5"). Where a pointer
/// becomes a cell for a selection and a link, so a selection's points are
/// the lines' own columns and its text reads them as they are; xterm halves
/// the columns as it copies (`SaveText`, `[[source: xterm-button]]`), and a
/// mouse report keeps the display column, as xterm's does.
pub fn char_col(view: &Viewport<'_>, y: Row, col: Col) -> Col {
    match view.rendition(y).is_double() {
        true => Col::at_index(col.index() / 2),
        false => col,
    }
}

/// The one reader of a line by number, for a selection's text and for a
/// bare URI's walk across the rows a soft wrap joined.
pub fn row_of<'t>(
    term: &'t Term<'_>,
    scrolled: Scrolled,
    line: AbsLine,
) -> Option<(&'t [PackedCell], Wrap)> {
    let live = term.viewport();
    if line.0 >= scrolled.0 {
        let y = usize::try_from(line.0 - scrolled.0).ok()?;
        if y >= live.rows().index() {
            return None;
        }
        let y = Row::from_index(y);
        return Some((live.row(y), live.wrapped(y)));
    }
    // A line the window can reach, by the core's clamp: the history count
    // is the primary's whatever screen is up, and the alternate screen
    // reaches none of it -- its view clamps to the live screen, whose top
    // row a line above it would otherwise read again.
    let back = Rows::of_count(usize::try_from(scrolled.0 - line.0).ok()?);
    if term.looking_back(back) != back {
        return None;
    }
    let view = term.view(back);
    Some((view.row(Row::ZERO), view.wrapped(Row::ZERO)))
}

/// **Which table [`row_of`]'s row names its rendition ids in**: the live
/// table, or the generation the core froze the line into
/// (`Viewport::styles`). Read beside the row by a reader of its renditions.
pub fn row_styles_of<'t>(
    term: &'t Term<'_>,
    scrolled: Scrolled,
    line: AbsLine,
) -> baud_domain::RowStyles<'t> {
    if line.0 >= scrolled.0 {
        let Ok(y) = usize::try_from(line.0 - scrolled.0) else {
            return crate::frozen::live(term);
        };
        return term.viewport().styles(Row::from_index(y));
    }
    let back = Rows::of_count(usize::try_from(scrolled.0 - line.0).unwrap_or(0));
    match term.looking_back(back) == back {
        true => term.view(back).styles(Row::ZERO),
        false => crate::frozen::live(term),
    }
}

/// [`row_of`] with the row's mark -- its paragraph properties, rendition
/// and wrap -- and whether it holds text past the live width, as the
/// display's layout reads a row.
fn marked_row<'t>(
    term: &'t Term<'_>,
    scrolled: Scrolled,
    line: AbsLine,
) -> Option<(
    &'t [PackedCell],
    baud_domain::RowExtent,
    baud_domain::PastEdge,
)> {
    let live = term.viewport();
    if line.0 >= scrolled.0 {
        let y = usize::try_from(line.0 - scrolled.0).ok()?;
        if y >= live.rows().index() {
            return None;
        }
        let y = Row::from_index(y);
        return Some((
            live.row(y),
            crate::rebreak::mark_of(&live, y),
            live.text_past_edge(y),
        ));
    }
    let back = Rows::of_count(usize::try_from(scrolled.0 - line.0).ok()?);
    if term.looking_back(back) != back {
        return None;
    }
    let view = term.view(back);
    Some((
        view.row(Row::ZERO),
        crate::rebreak::mark_of(&view, Row::ZERO),
        view.text_past_edge(Row::ZERO),
    ))
}

fn push_cell(out: &mut String, term: &Term<'_>, cell: PackedCell) {
    if cell.content.is_wide_tail() {
        return;
    }
    // A combining mark stored alone copies as the mark, the base it is
    // drawn on being no character the program wrote (`Content::DEFECTIVE`).
    let base = term.base_of(cell);
    if base != Content::DEFECTIVE {
        let Some(cp) = base.as_codepoint() else {
            out.push(' ');
            return;
        };
        if let Some(ch) = char::from_u32(cp.value()) {
            out.push(ch);
        }
    }
    for mark in term.cluster_marks(cell) {
        if let Some(ch) = char::from_u32(mark) {
            out.push(ch);
        }
    }
}

/// The characters a word stops at, besides whitespace and an empty cell.
const DELIMITERS: &str = ",│`|:\"' ()[]{}<>\t";

fn is_word(term: &Term<'_>, row: &[PackedCell], at: Col) -> bool {
    let i = at.index();
    let cell = row[i];
    if cell.content.is_wide_tail() {
        return i > 0 && is_word(term, row, Col::at_index(i - 1));
    }
    let base = term.base_of(cell);
    // A mark stored alone is a character the program wrote, and no
    // whitespace or delimiter (`Content::DEFECTIVE`).
    if base == Content::DEFECTIVE {
        return true;
    }
    let Some(cp) = base.as_codepoint() else {
        return false;
    };
    match char::from_u32(cp.value()) {
        Some(ch) => !ch.is_whitespace() && !DELIMITERS.contains(ch),
        None => false,
    }
}

fn snap_start(row: &[PackedCell], col: Col) -> Col {
    let c = col.index().min(row.len().saturating_sub(1));
    if c > 0 && row.get(c).is_some_and(|cell| cell.content.is_wide_tail()) {
        Col::at_index(c - 1)
    } else {
        Col::at_index(c)
    }
}

fn snap_end(row: &[PackedCell], col: Col) -> Col {
    let c = col.index().min(row.len().saturating_sub(1));
    if row
        .get(c + 1)
        .is_some_and(|cell| cell.content.is_wide_tail())
    {
        Col::at_index(c + 1)
    } else {
        Col::at_index(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_domain::{CellFlags, Codepoint, StyleId};

    fn glyph(ch: char) -> PackedCell {
        PackedCell::glyph(
            Codepoint::new(u32::from(ch)).unwrap(),
            StyleId::DEFAULT,
            CellFlags::NONE,
        )
    }

    fn row(text: &str) -> Vec<PackedCell> {
        let mut out = Vec::new();
        for ch in text.chars() {
            match ch {
                '_' => out.push(PackedCell::EMPTY),
                '#' => out.push(PackedCell::wide_tail(StyleId::DEFAULT, CellFlags::NONE)),
                c => out.push(glyph(c)),
            }
        }
        out
    }

    #[test]
    fn a_wide_character_is_never_half_selected() {
        // `a` then a wide `x` with its tail, then `b`.
        let r = row("ax#b");
        let l = AbsLine::at(Rows::of_count(3));
        let on_tail = Span::cell(&r, l, Col::at_index(2));
        assert_eq!(on_tail.start.col.index(), 1);
        assert_eq!(on_tail.end.col.index(), 2);
        let on_lead = Span::cell(&r, l, Col::at_index(1));
        assert_eq!((on_lead.start.col.index(), on_lead.end.col.index()), (1, 2));
        let plain = Span::cell(&r, l, Col::at_index(3));
        assert_eq!((plain.start.col.index(), plain.end.col.index()), (3, 3));
    }

    #[test]
    fn a_word_stops_at_delimiters_blanks_and_empty_cells() {
        let r = row("foo_bar(baz) qux");
        let l = AbsLine::at(Rows::of_count(0));
        let ws = baud_core::WinSize {
            ws_row: 1,
            ws_col: 20,
        };
        let mut st = baud_core::Owned::new(&ws);
        let t = Term::new(st.storage(), &ws);
        let w = |c: usize| {
            let s = Span::word(&t, &r, l, Col::at_index(c));
            (s.start.col.index(), s.end.col.index())
        };
        assert_eq!(w(1), (0, 2));
        assert_eq!(w(5), (4, 6));
        assert_eq!(w(9), (8, 10));
        // A delimiter under the pointer is itself.
        assert_eq!(w(7), (7, 7));
        // A word carries its wide tail.
        let wide = row("ab#c d");
        let s = Span::word(&t, &wide, l, Col::at_index(0));
        assert_eq!((s.start.col.index(), s.end.col.index()), (0, 3));
    }

    #[test]
    fn the_range_is_the_earliest_start_to_the_latest_end() {
        let l0 = AbsLine::at(Rows::of_count(10));
        let l1 = AbsLine::at(Rows::of_count(12));
        let a = Span {
            start: Point::new(l1, Col::at_index(4)),
            end: Point::new(l1, Col::at_index(6)),
        };
        let b = Span {
            start: Point::new(l0, Col::at_index(8)),
            end: Point::new(l0, Col::at_index(9)),
        };
        let mut s = Selection::at(a, Grain::Word);
        assert!(!s.is_click());
        s.extend(b);
        let (start, end) = s.ordered();
        assert_eq!((start.line, start.col.index()), (l0, 8));
        assert_eq!((end.line, end.col.index()), (l1, 6));
        let click = Selection::at(Span::cell(&row("abc"), l0, Col::at_index(1)), Grain::Cell);
        assert!(click.is_click());
    }

    #[test]
    fn the_highlight_is_the_view_s_share_of_the_range() {
        let s = Selection::at(
            Span {
                start: Point::new(AbsLine::at(Rows::of_count(5)), Col::at_index(3)),
                end: Point::new(AbsLine::at(Rows::of_count(8)), Col::at_index(2)),
            },
            Grain::Cell,
        );
        let rows = Rows::of_count(4);
        let cols = Cols::of_index(10);
        // The view holds lines 6..=9: the start is above it, the end inside.
        let h = s
            .highlight(AbsLine::at(Rows::of_count(6)), rows, cols)
            .unwrap();
        assert_eq!(h.from, (Row::ZERO, Col::ZERO));
        assert_eq!(h.to, (Row::from_index(2), Col::at_index(2)));
        assert!(h.contains(Row::from_index(1), Col::at_index(9)));
        assert!(!h.contains(Row::from_index(2), Col::at_index(3)));
        // Lines 2..=5: only the start's line, to the row's end.
        let h = s
            .highlight(AbsLine::at(Rows::of_count(2)), rows, cols)
            .unwrap();
        assert_eq!(h.from, (Row::from_index(3), Col::at_index(3)));
        assert_eq!(h.to, (Row::from_index(3), Col::at_index(9)));
        // Lines 9..=12: none of it.
        assert!(
            s.highlight(AbsLine::at(Rows::of_count(9)), rows, cols)
                .is_none()
        );
    }

    #[test]
    fn a_recycled_line_clips_the_range_and_a_gone_range_clears_it() {
        let mut s = Selection::at(
            Span {
                start: Point::new(AbsLine::at(Rows::of_count(5)), Col::at_index(3)),
                end: Point::new(AbsLine::at(Rows::of_count(8)), Col::at_index(2)),
            },
            Grain::Cell,
        );
        assert_eq!(s.clip(AbsLine::at(Rows::of_count(7))), Clip::Kept);
        let (start, _) = s.ordered();
        assert_eq!(
            (start.line, start.col.index()),
            (AbsLine::at(Rows::of_count(7)), 0)
        );
        assert_eq!(s.clip(AbsLine::at(Rows::of_count(9))), Clip::Cleared);
    }

    /// **A carried selection moves with its text and is held to the lines
    /// that stay**: a stream over lines 5, column 3, to 8, column 2, moved
    /// up two with lines 3 to 5 left, keeps line 3 from column 3 and is cut
    /// at 5's last column; moved down two with lines 8 to 10 left, its start
    /// is cut to line 8's first column and it keeps 10, column 2. A block
    /// keeps the columns it had at a cut end. Unmoved and uncut, nothing
    /// changes.
    #[test]
    fn a_carried_selection_moves_with_its_text_and_keeps_the_lines_that_stay() {
        use baud_core::{SurfaceLine, TextMoved, WatchedLines};
        let line = |n| AbsLine::at(Rows::of_count(n));
        let span = Span {
            start: Point::new(line(5), Col::at_index(3)),
            end: Point::new(line(8), Col::at_index(2)),
        };
        let at = |y| SurfaceLine::At(Row::from_index(y));
        let scrolled = Scrolled::START;
        let last = Col::at_index(19);
        let ends = |s: Selection| {
            let (a, b) = s.ordered();
            ((a.line, a.col.index()), (b.line, b.col.index()))
        };
        let watched = |first, last_line, moved| WatchedLines {
            first: at(first),
            last: at(last_line),
            moved,
        };
        let mut s = Selection::at(span, Grain::Cell);
        s.follow(
            scrolled,
            watched(3, 5, TextMoved::Up(Rows::of_count(2))),
            last,
        );
        assert_eq!(
            ends(s),
            ((line(3), 3), (line(5), 19)),
            "up two, cut at the end"
        );
        let mut s = Selection::at(span, Grain::Cell);
        s.follow(
            scrolled,
            watched(8, 10, TextMoved::Down(Rows::of_count(2))),
            last,
        );
        assert_eq!(
            ends(s),
            ((line(8), 0), (line(10), 2)),
            "down two, cut at the start"
        );
        let mut s = Selection::block(span, Grain::Cell);
        s.follow(
            scrolled,
            watched(3, 5, TextMoved::Up(Rows::of_count(2))),
            last,
        );
        assert_eq!(
            ends(s),
            ((line(3), 3), (line(5), 2)),
            "a block keeps its columns"
        );
        let mut s = Selection::block(span, Grain::Cell);
        s.follow(
            scrolled,
            watched(8, 10, TextMoved::Down(Rows::of_count(2))),
            last,
        );
        assert_eq!(ends(s), ((line(8), 3), (line(10), 2)), "at either end");
        let mut s = Selection::at(span, Grain::Cell);
        s.follow(scrolled, watched(5, 8, TextMoved::Down(Rows::ZERO)), last);
        assert_eq!(
            ends(s),
            ((line(5), 3), (line(8), 2)),
            "the control: unmoved"
        );
    }

    /// A selection over lines 5 to 8 touches any range that shares a line
    /// with it, at either end, and none that ends before it or starts after.
    #[test]
    fn a_selection_touches_the_lines_it_shares() {
        let line = |n| AbsLine::at(Rows::of_count(n));
        let s = Selection::at(
            Span {
                start: Point::new(line(5), Col::at_index(3)),
                end: Point::new(line(8), Col::at_index(2)),
            },
            Grain::Cell,
        );
        assert!(s.touches(line(8), line(20)), "shares its last line");
        assert!(s.touches(line(0), line(5)), "shares its first line");
        assert!(s.touches(line(6), line(6)), "inside it");
        assert!(!s.touches(line(9), line(20)), "after it");
        assert!(!s.touches(line(0), line(4)), "before it");
    }

    #[test]
    fn a_block_takes_its_columns_apart_from_its_lines() {
        let r = row(&"x".repeat(40));
        let at = |line: usize, col: usize| {
            Span::cell(&r, AbsLine::at(Rows::of_count(line)), Col::at_index(col))
        };
        // Anchor and head at opposite corners of one rectangle, dragged in
        // each of the four directions: every one is lines 5..=10 and
        // columns 2..=30. Up-right and down-left are the two a row-major
        // order reads with its columns backwards.
        let corners = [
            ((5, 2), (10, 30)),
            ((10, 30), (5, 2)),
            ((10, 2), (5, 30)),
            ((5, 30), (10, 2)),
        ];
        for (a, h) in corners {
            let mut s = Selection::block(at(a.0, a.1), Grain::Cell);
            s.extend(at(h.0, h.1));
            let (start, end) = s.ordered();
            assert_eq!(
                (start.line, start.col.index(), end.line, end.col.index()),
                (
                    AbsLine::at(Rows::of_count(5)),
                    2,
                    AbsLine::at(Rows::of_count(10)),
                    30
                ),
                "anchor {a:?}, head {h:?}"
            );
        }
        // A stream over the up-right pair reads row-major, which is the
        // order a block must not take.
        let mut s = Selection::at(at(10, 2), Grain::Cell);
        s.extend(at(5, 30));
        let (start, end) = s.ordered();
        assert_eq!((start.col.index(), end.col.index()), (30, 2));
    }

    #[test]
    fn a_block_is_a_rectangle_and_its_corner_snap_widens_every_row() {
        // The anchor presses a wide character's lead at column 4, so its
        // span takes the tail at 5; the head is on narrow text at 2.
        let wide = row("abcdx#gh");
        let narrow = row(&"x".repeat(12));
        let mut s = Selection::block(
            Span::cell(&wide, AbsLine::at(Rows::of_count(0)), Col::at_index(4)),
            Grain::Cell,
        );
        s.extend(Span::cell(
            &narrow,
            AbsLine::at(Rows::of_count(3)),
            Col::at_index(2),
        ));
        let h = s
            .highlight(
                AbsLine::at(Rows::of_count(0)),
                Rows::of_count(4),
                Cols::of_index(12),
            )
            .unwrap();
        assert_eq!(h.region, Region::Block);
        assert_eq!(
            (h.from, h.to),
            (
                (Row::ZERO, Col::at_index(2)),
                (Row::from_index(3), Col::at_index(5))
            )
        );
        // Column 5 on a row that is no corner's: the snap widened it.
        assert!(h.contains(Row::from_index(2), Col::at_index(5)));
        // A middle row stops at the columns, where a stream takes it whole.
        assert!(!h.contains(Row::from_index(1), Col::at_index(9)));
        assert!(!h.contains(Row::from_index(1), Col::at_index(1)));
        let stream = Highlight {
            region: Region::Stream,
            ..h
        };
        assert!(stream.contains(Row::from_index(1), Col::at_index(9)));
        // The region is part of equality, so damage repaints a switch
        // between the two over the same corners.
        assert_ne!(stream, h);
    }

    #[test]
    fn a_wide_tail_takes_its_leads_selection() {
        let h = Highlight::new(
            (Row::ZERO, Col::at_index(2)),
            (Row::from_index(3), Col::at_index(2)),
            Region::Block,
        );
        let tail = Content::WIDE_TAIL;
        let plain = glyph('x').content;
        // A tail at the left edge, its lead outside: out.
        assert!(h.contains(Row::from_index(1), Col::at_index(2)));
        assert!(!h.selects(Row::from_index(1), Col::at_index(2), tail));
        // A tail past the right edge, its lead inside: in.
        assert!(!h.contains(Row::from_index(2), Col::at_index(3)));
        assert!(h.selects(Row::from_index(2), Col::at_index(3), tail));
        // Anything else answers for its own column.
        assert!(!h.selects(Row::from_index(2), Col::at_index(3), plain));
        assert!(h.selects(Row::from_index(2), Col::at_index(2), plain));
        // A tail in column 0 cannot have a lead before it, and answers for
        // itself rather than wrapping.
        let edge = Highlight::new(
            (Row::ZERO, Col::ZERO),
            (Row::ZERO, Col::ZERO),
            Region::Block,
        );
        assert!(edge.selects(Row::ZERO, Col::ZERO, tail));
    }

    #[test]
    fn a_clipped_block_keeps_its_columns() {
        let r = row(&"x".repeat(20));
        let mut s = Selection::block(
            Span::cell(&r, AbsLine::at(Rows::of_count(5)), Col::at_index(7)),
            Grain::Cell,
        );
        s.extend(Span::cell(
            &r,
            AbsLine::at(Rows::of_count(9)),
            Col::at_index(3),
        ));
        assert_eq!(s.clip(AbsLine::at(Rows::of_count(7))), Clip::Kept);
        let (start, end) = s.ordered();
        assert_eq!(
            (start.line, start.col.index(), end.col.index()),
            (AbsLine::at(Rows::of_count(7)), 3, 7)
        );
        assert_eq!(s.clip(AbsLine::at(Rows::of_count(10))), Clip::Cleared);
    }

    #[test]
    fn a_press_starts_a_stream_a_block_or_extends_what_is_there() {
        let r = row(&"x".repeat(20));
        let span = |c: usize| Span::cell(&r, AbsLine::at(Rows::of_count(0)), Col::at_index(c));
        let stream = Selection::pressed(None, Press::Stream, Grain::Cell, |_| span(2));
        assert_eq!(stream.region(), Region::Stream);
        // Extend keeps the anchor and the region of what is on screen.
        let block = Selection::pressed(None, Press::Block, Grain::Cell, |_| span(2));
        let extended = Selection::pressed(Some(block), Press::Extend, Grain::Cell, |_| span(9));
        assert_eq!(extended.region(), Region::Block);
        assert_eq!(
            (
                extended.ordered().0.col.index(),
                extended.ordered().1.col.index()
            ),
            (2, 9)
        );
        // A block press starts over, whatever is on screen.
        let fresh = Selection::pressed(Some(extended), Press::Block, Grain::Cell, |_| span(5));
        assert!(fresh.is_click());
        assert_eq!(fresh.region(), Region::Block);
        // Extend with nothing on screen starts a stream.
        let none = Selection::pressed(None, Press::Extend, Grain::Cell, |_| span(1));
        assert_eq!(none.region(), Region::Stream);
        // And an extension asks for the span in the grain the selection
        // was made with, not the grain of the click that extends it.
        let word = Selection::pressed(None, Press::Stream, Grain::Word, |_| span(2));
        let _ = Selection::pressed(Some(word), Press::Extend, Grain::Cell, |g| {
            assert_eq!(g, Grain::Word);
            span(9)
        });
    }

    #[test]
    fn scrolled_names_the_lines_a_view_shows() {
        let s = Scrolled::START.advanced(Rows::of_count(100));
        assert_eq!(s.top(Rows::ZERO), AbsLine::at(Rows::of_count(100)));
        assert_eq!(s.top(Rows::of_count(30)), AbsLine::at(Rows::of_count(70)));
        assert_eq!(
            s.line(Rows::of_count(30), Row::from_index(4)),
            AbsLine::at(Rows::of_count(74))
        );
        assert_eq!(
            s.oldest(Rows::of_count(60)),
            AbsLine::at(Rows::of_count(40))
        );
        assert_eq!(
            Scrolled::START.oldest(Rows::of_count(5)),
            AbsLine::at(Rows::of_count(0))
        );
    }
}
