//! Selection: a range of cells over the live surface's lines, made with the
//! mouse and read back as text. `docs/research/notes/k-selection.md` is the
//! spec; this is the model, platform-free, and the extraction.
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

use baud_core::Term;
use baud_domain::{Col, Cols, PackedCell, Row, Rows, Viewport, Wrap};

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

    /// The absolute line at the top of a view `back` lines up.
    pub fn top(self, back: Rows) -> AbsLine {
        AbsLine(self.0).minus(back)
    }

    /// The absolute line at screen row `y` of a view `back` lines up.
    pub fn line(self, back: Rows, y: Row) -> AbsLine {
        self.top(back).plus(Rows::of_count(y.index()))
    }

    /// The oldest line the ring still holds, with `retained` lines of
    /// history behind the live screen.
    pub fn oldest(self, retained: Rows) -> AbsLine {
        AbsLine(self.0).minus(retained)
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
    pub fn word(row: &[PackedCell], line: AbsLine, col: Col) -> Span {
        let c = col.index().min(row.len().saturating_sub(1));
        if row.is_empty() || !is_word(row, Col::at_index(c)) {
            return Span::cell(row, line, col);
        }
        let mut start = c;
        while start > 0 && is_word(row, Col::at_index(start - 1)) {
            start -= 1;
        }
        let mut end = c;
        while end + 1 < row.len() && is_word(row, Col::at_index(end + 1)) {
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
}

/// What a press selects, and what a drag after it extends by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grain {
    Cell,
    Word,
    Line,
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
}

impl Selection {
    pub fn at(span: Span, grain: Grain) -> Selection {
        Selection {
            anchor: span,
            head: span,
            grain,
        }
    }

    pub fn grain(&self) -> Grain {
        self.grain
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

    /// The range, earliest start to latest end.
    pub fn ordered(&self) -> (Point, Point) {
        let start = self.anchor.start.min(self.head.start);
        let end = self.anchor.end.max(self.head.end);
        (start, end)
    }

    /// Move any endpoint older than `oldest` -- a line the ring has
    /// recycled -- to the oldest line's first column. Both gone is no
    /// selection.
    pub fn clip(&mut self, oldest: AbsLine) -> Clip {
        let (_, end) = self.ordered();
        if end.line < oldest {
            return Clip::Cleared;
        }
        for span in [&mut self.anchor, &mut self.head] {
            for p in [&mut span.start, &mut span.end] {
                if p.line < oldest {
                    *p = Point::new(oldest, Col::ZERO);
                }
            }
        }
        Clip::Kept
    }

    /// Where the selection falls on a view whose top row is `top`, in the
    /// view's screen coordinates, clipped to the screen; `None` when none
    /// of it is on this screen.
    pub fn highlight(&self, top: AbsLine, rows: Rows, cols: Cols) -> Option<Highlight> {
        let (start, end) = self.ordered();
        let last_line = top.plus(Rows::of_count(rows.index().saturating_sub(1)));
        if end.line < top || start.line > last_line || rows.index() == 0 {
            return None;
        }
        let last_col = Col::at_index(cols.index().saturating_sub(1));
        let from = if start.line < top {
            (Row::ZERO, Col::ZERO)
        } else {
            (
                row_in(top, start.line),
                Col::at_index(start.col.index().min(last_col.index())),
            )
        };
        let to = if end.line > last_line {
            (Row::from_index(rows.index() - 1), last_col)
        } else {
            (
                row_in(top, end.line),
                Col::at_index(end.col.index().min(last_col.index())),
            )
        };
        Some(Highlight { from, to })
    }

    /// The selected text, as the spec extracts it: a wide tail contributes
    /// nothing, an empty cell before the row's last selected content is a
    /// space and after it is dropped, marks follow their base, a row the
    /// writer wrapped joins the next without a newline, and no newline
    /// ends the last row.
    pub fn text(&self, term: &Term<'_>, scrolled: Scrolled) -> String {
        let (start, end) = self.ordered();
        let mut out = String::new();
        let mut line = start.line;
        loop {
            if let Some((row, wrap)) = row_of(term, scrolled, line) {
                let first = if line == start.line {
                    start.col.index()
                } else {
                    0
                };
                let last = if line == end.line {
                    end.col.index().min(row.len().saturating_sub(1))
                } else {
                    row.len().saturating_sub(1)
                };
                if first <= last && !row.is_empty() {
                    let cells = &row[first..=last];
                    if let Some(used) = cells.iter().rposition(|c| !c.content.is_empty()) {
                        for cell in &cells[..=used] {
                            push_cell(&mut out, term, *cell);
                        }
                    }
                }
                if line != end.line && matches!(wrap, Wrap::Ended) {
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

/// A selection on one view, in screen coordinates, both ends inclusive in
/// row-major order. What the renderers draw from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Highlight {
    pub from: (Row, Col),
    pub to: (Row, Col),
}

impl Highlight {
    pub fn contains(&self, r: Row, c: Col) -> bool {
        let p = (r.index(), c.index());
        let from = (self.from.0.index(), self.from.1.index());
        let to = (self.to.0.index(), self.to.1.index());
        p >= from && p <= to
    }

    /// Whether any cell of screen row `r` is selected.
    pub fn covers_row(&self, r: Row) -> bool {
        r.index() >= self.from.0.index() && r.index() <= self.to.0.index()
    }
}

fn row_in(top: AbsLine, line: AbsLine) -> Row {
    Row::from_index(usize::try_from(line.0.saturating_sub(top.0)).unwrap_or(usize::MAX))
}

/// The cells of absolute line `line` and how the row ended, or `None`
/// when the line is past the live screen or the ring no longer holds it.
fn row_of<'t>(
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
    let back = usize::try_from(scrolled.0 - line.0).ok()?;
    if back > term.scrollback().len() {
        return None;
    }
    let view = term.view(Rows::of_count(back));
    Some((view.row(Row::ZERO), view.wrapped(Row::ZERO)))
}

fn push_cell(out: &mut String, term: &Term<'_>, cell: PackedCell) {
    if cell.content.is_wide_tail() {
        return;
    }
    let Some(cp) = cell.content.as_codepoint() else {
        out.push(' ');
        return;
    };
    if let Some(ch) = char::from_u32(cp.value()) {
        out.push(ch);
    }
    for mark in term.cluster_marks(cell) {
        if let Some(ch) = char::from_u32(mark) {
            out.push(ch);
        }
    }
}

/// The characters a word stops at, besides whitespace and an empty cell.
const DELIMITERS: &str = ",│`|:\"' ()[]{}<>\t";

fn is_word(row: &[PackedCell], at: Col) -> bool {
    let i = at.index();
    let cell = row[i];
    if cell.content.is_wide_tail() {
        return i > 0 && is_word(row, Col::at_index(i - 1));
    }
    let Some(cp) = cell.content.as_codepoint() else {
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
        let w = |c: usize| {
            let s = Span::word(&r, l, Col::at_index(c));
            (s.start.col.index(), s.end.col.index())
        };
        assert_eq!(w(1), (0, 2));
        assert_eq!(w(5), (4, 6));
        assert_eq!(w(9), (8, 10));
        // A delimiter under the pointer is itself.
        assert_eq!(w(7), (7, 7));
        // A word carries its wide tail.
        let wide = row("ab#c d");
        let s = Span::word(&wide, l, Col::at_index(0));
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
