//! **Find in the scrollback, for both apps**: the query, its matches over
//! the live surface and which one is current, kept across reads, and what
//! a frame draws of them -- every match marked, the current one as the
//! selection, and the find field.
//!
//! The matches come from the core's pure read (`Term::find`): canonical
//! caseless, whole clusters. **The ring is read once a query**, all of it,
//! and after that only where a read can have changed it: from the first
//! line of the one at the older of the screen's two tops to the bottom,
//! with the matches on lines that retired dropped. Nothing older is read
//! again: a line of the history changes only by an erasure, which reads it
//! all again -- with a switch of screen, a reset or a new size, whose reflow
//! renumbers the lines -- or by `CSI ? 3 J`, a selective erase of the saved
//! lines, which no reading of the core counts, so a match it wiped stays
//! listed until the query changes (the record, "Find in the scrollback, in
//! both apps"). Nothing here runs per frame.
//!
//! **The newest match is current first**, and Find Next goes to the next
//! older one, up the history, wrapping from the oldest to the newest; Find
//! Previous goes the other way. A terminal's text grows at the bottom and
//! the reader arrives there, as Ghostty's search and foot's Ctrl+R run
//! (`[[source: ghostty-search-screen-521cf2c]]`, `[[source:
//! foot-search-cb27717]]`): Jev's lean (the record, "Search in the
//! scrollback").

use baud_core::{
    Found, HistoryErasures, Needle, NeedleRefused, Resets, ScreenEpoch, SearchLines, SurfaceLine,
    Term,
};
use baud_domain::{AmbiguousWidth, Classified, Codepoint, Col, Cols, LineIdx, Row, Rows, Width};

use crate::overlay::{Caret, Composition, Hints};
use crate::units::Tally;

/// The find field's least width, in columns, where the grid has them.
pub const FIELD_COLS: usize = 32;

/// What the field says before the query.
const LABEL: &str = "Find ";

/// **The find field on the next frame**: where it starts, the characters
/// it shows -- the label, the query's tail that fits and what was found --
/// and the column the caret stands in.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Field {
    pub row: Row,
    pub col: Col,
    pub text: Vec<Codepoint>,
    pub caret: Col,
}

impl Field {
    /// The field as an overlay of characters every path draws: each in
    /// the selection's colours, the way a selected run of the grid is.
    pub fn composition(&self) -> Composition {
        Composition {
            row: self.row,
            col: self.col,
            text: self.text.clone(),
            ambiguous: AmbiguousWidth::Narrow,
            hints: vec![Hints::SELECTION; self.text.len()],
            // The field draws its own caret, a bar at `caret`
            // (`Renderer::set_find`); the terminal's cursor is not moved.
            caret: Caret::Hidden,
        }
    }
}

/// **What the find overlay draws on the next frame**: every match's part
/// of the view -- the current one is drawn as the selection, by the
/// driver -- and the field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FindLook {
    pub marks: Vec<Mark>,
    pub field: Field,
}

/// The columns `cp` takes in the field, as the grid would write it.
fn columns(cp: Codepoint) -> Cols {
    Cols::of_index(match Classified::of(cp).width(AmbiguousWidth::Narrow) {
        w if w == Width::ZERO => 0,
        w if w == Width::WIDE => 2,
        _ => 1,
    })
}

fn chars(s: &str) -> impl Iterator<Item = Codepoint> + '_ {
    s.chars().map(Codepoint::from_char)
}

/// What a list of matches was read against: a change in any of these
/// reads the ring again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Basis {
    epoch: ScreenEpoch,
    erasures: HistoryErasures,
    resets: Resets,
    geometry: (Cols, Rows),
}

impl Basis {
    fn of(term: &Term<'_>) -> Basis {
        Basis {
            epoch: term.screen_epoch(),
            erasures: term.history_erasures(),
            resets: term.resets(),
            geometry: term.geometry(),
        }
    }
}

/// Whether a match is the current one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Current {
    Yes,
    No,
}

/// **One row's part of a match, on a view**: the columns, both inclusive,
/// and whether the match is the current one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    pub row: Row,
    pub from: Col,
    pub to: Col,
    pub current: Current,
}

/// Which match is current, counted from the newest, of how many.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Position {
    pub nth: Tally,
    pub of: Tally,
}

/// The query, its matches and the current one.
#[derive(Clone, Debug, Default)]
pub struct Find {
    query: Vec<Codepoint>,
    needle: Option<Needle>,
    refused: Option<NeedleRefused>,
    found: Vec<Found>,
    current: Option<usize>,
    basis: Option<Basis>,
    /// The live surface's lines when the list was last brought up to date.
    lines: Option<SearchLines>,
}

impl Find {
    pub fn new() -> Find {
        Find::default()
    }

    /// The query as typed.
    pub fn query(&self) -> &[Codepoint] {
        &self.query
    }

    /// Why the query finds nothing without looking, if it does: empty, or
    /// longer than a needle holds.
    pub fn refused(&self) -> Option<NeedleRefused> {
        self.refused
    }

    /// Every match, oldest first.
    pub fn matches(&self) -> &[Found] {
        &self.found
    }

    /// The current match, if any.
    pub fn current(&self) -> Option<Found> {
        self.current.and_then(|i| self.found.get(i).copied())
    }

    /// The current match counted from the newest, `1` being it, and the
    /// number of matches: what the field shows.
    pub fn position(&self) -> Option<Position> {
        self.current.map(|i| Position {
            nth: Tally::of(self.found.len() - i),
            of: Tally::of(self.found.len()),
        })
    }

    /// **A new query**: the ring read for it, and the current match the
    /// newest at or above the bottom of the window `back` lines up -- the
    /// newest the reader can have seen.
    pub fn set_query(&mut self, term: &Term<'_>, query: Vec<Codepoint>, back: Rows) {
        self.query = query;
        let needle = Needle::new(&self.query);
        self.refused = needle.as_ref().err().copied();
        self.needle = needle.ok();
        self.basis = None;
        self.refresh(term);
        let lines = term.search_lines();
        let bottom = lines
            .top
            .minus(term.looking_back(back))
            .unwrap_or(LineIdx::FIRST)
            .plus(Rows::of_count(term.geometry().1.index().saturating_sub(1)));
        let seen = self.found.partition_point(|f| f.first.line <= bottom);
        self.current = match (seen, self.found.len()) {
            (_, 0) => None,
            (0, _) => Some(0),
            (n, _) => Some(n - 1),
        };
    }

    /// **Bring the matches up to date after a read**: the lines that
    /// retired dropped and the lines a read can have written read again,
    /// or the whole ring when the surface it was read against has gone.
    /// The current match stays where it was, or moves to the next newer
    /// one when its line changed under it.
    pub fn refresh(&mut self, term: &Term<'_>) {
        let Some(needle) = &self.needle else {
            self.found.clear();
            self.current = None;
            return;
        };
        let was = self.current();
        let now = Basis::of(term);
        let lines = term.search_lines();
        let from = match (self.basis, self.lines) {
            (Some(b), Some(l)) if b == now => {
                let top = l.top.min(lines.top).max(lines.oldest);
                term.line_start(top)
            }
            _ => lines.oldest,
        };
        let keep = self
            .found
            .iter()
            .position(|f| f.first.line >= from)
            .unwrap_or(self.found.len());
        self.found.truncate(keep);
        self.found.retain(|f| f.first.line >= lines.oldest);
        let found = &mut self.found;
        term.find(needle, from, lines.end, |f| found.push(f));
        self.basis = Some(now);
        self.lines = Some(lines);
        self.current = match (was, self.found.is_empty()) {
            (_, true) => None,
            (None, false) => Some(self.found.len() - 1),
            (Some(w), false) => Some(
                self.found
                    .partition_point(|f| f.first < w.first)
                    .min(self.found.len() - 1),
            ),
        };
    }

    /// Find Next: the next older match, from the oldest to the newest.
    pub fn next(&mut self) {
        let n = self.found.len();
        self.current = self.current.map(|i| (i + n - 1) % n);
    }

    /// Find Previous: the next newer match, from the newest to the oldest.
    pub fn previous(&mut self) {
        let n = self.found.len();
        self.current = self.current.map(|i| (i + 1) % n);
    }

    /// **How far back the window must be to show the current match**:
    /// `back` itself when the match is in the window already, else the
    /// window with the match's first line in its middle, as near as the
    /// history allows; `None` with no current match.
    pub fn window_for(&self, term: &Term<'_>, back: Rows) -> Option<Rows> {
        let m = self.current()?;
        let rows = term.geometry().1.index();
        let back = term.looking_back(back).index();
        // Line `l` sits at view row `l - (top - back)`: `rel` is `l - top`.
        let rel = |l: LineIdx| match term.surface_line(l) {
            Some(SurfaceLine::At(y)) => Some(isize::try_from(y.index()).unwrap_or(isize::MAX)),
            Some(SurfaceLine::Above(n)) => Some(-isize::try_from(n.index()).unwrap_or(isize::MAX)),
            None => None,
        };
        let (first, last) = (rel(m.first.line)?, rel(m.last.line)?);
        let b = isize::try_from(back).unwrap_or(0);
        let rows_i = isize::try_from(rows).unwrap_or(0);
        if first + b >= 0 && last + b < rows_i {
            return Some(Rows::of_count(back));
        }
        let want = (rows_i / 2 - first).max(0);
        Some(term.looking_back(Rows::of_count(usize::try_from(want).unwrap_or(0))))
    }

    /// **The field on a grid `cols` by `rows`**: along the top row at the
    /// right edge, [`FIELD_COLS`] wide or a third of the grid, whichever is
    /// wider, and never wider than the grid. It shows the label, as much
    /// of the query's end as fits, and what was found: which match of how
    /// many, `none` when the query finds nothing, `too long` when it is
    /// longer than a needle holds.
    pub fn field(&self, cols: Cols) -> Field {
        let width = cols.index().min(FIELD_COLS.max(cols.index() / 3));
        let status: String = match (self.refused, self.position()) {
            (Some(NeedleRefused::Empty), _) => String::new(),
            (Some(NeedleRefused::TooLong), _) => "too long".into(),
            (None, Some(p)) => format!("{} of {}", p.nth.value(), p.of.value()),
            (None, None) => "none".into(),
        };
        let fixed = 1 + LABEL.len() + 1 + status.len() + 1;
        let room = width.saturating_sub(fixed);
        // The query's tail that fits, whole characters, marks with them.
        let mut shown: Vec<Codepoint> = Vec::new();
        let mut used = 0;
        for &cp in self.query.iter().rev() {
            let w = columns(cp).index();
            if used + w > room {
                break;
            }
            used += w;
            shown.push(cp);
        }
        shown.reverse();
        while shown.first().is_some_and(|&c| columns(c).index() == 0) {
            shown.remove(0);
        }
        let pad = width.saturating_sub(1 + LABEL.len() + used + status.len() + 1);
        let mut text: Vec<Codepoint> = chars(" ").chain(chars(LABEL)).collect();
        text.extend_from_slice(&shown);
        text.extend(core::iter::repeat_n(Codepoint::from_char(' '), pad));
        text.extend(chars(&status));
        text.push(Codepoint::from_char(' '));
        let col = cols.index().saturating_sub(width);
        Field {
            row: Row::from_index(0),
            col: Col::at_index(col),
            caret: Col::at_index(col + 1 + LABEL.len() + used),
            text,
        }
    }

    /// **Every match's part of the view `back` lines up**, row by row: a
    /// match the writer wrapped across rows marks each.
    pub fn marks(&self, term: &Term<'_>, back: Rows) -> Vec<Mark> {
        let (cols, rows) = term.geometry();
        let lines = term.search_lines();
        let top = lines
            .top
            .minus(term.looking_back(back))
            .unwrap_or(LineIdx::FIRST);
        let end = top.plus(rows);
        let last_col = Col::at_index(cols.index().saturating_sub(1));
        let first = self.found.partition_point(|f| f.last.line < top);
        let mut out = Vec::new();
        for (i, f) in self.found.iter().enumerate().skip(first) {
            if f.first.line >= end {
                break;
            }
            let current = match self.current == Some(i) {
                true => Current::Yes,
                false => Current::No,
            };
            let mut line = f.first.line.max(top);
            while line <= f.last.line && line < end {
                let Some(y) = line.since(top) else { break };
                out.push(Mark {
                    row: Row::from_index(y.index()),
                    from: match line == f.first.line {
                        true => f.first.col,
                        false => Col::ZERO,
                    },
                    to: match line == f.last.line {
                        true => f.last.col,
                        false => last_col,
                    },
                    current,
                });
                line = line.next();
            }
        }
        out
    }
}
