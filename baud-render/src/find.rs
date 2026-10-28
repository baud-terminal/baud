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
//! again: a line of the history changes only by an erasure or by `CSI ? 3
//! J`, a selective erase of the saved lines, each counted by the core
//! (`Term::history_erasures`, `Term::history_wipes`), and either reads it
//! all again -- as a switch of screen, a reset or a new size, whose reflow
//! renumbers the lines, does. Nothing here runs per frame.
//!
//! **The newest match is current first**, and Find Next goes to the next
//! older one, up the history, wrapping from the oldest to the newest; Find
//! Previous goes the other way. A terminal's text grows at the bottom and
//! the reader arrives there, as Ghostty's search and foot's Ctrl+R run
//! (`[[source: ghostty-search-screen-521cf2c]]`, `[[source:
//! foot-search-cb27717]]`): Jev's lean (the record, "Search in the
//! scrollback").

use baud_core::{
    Found, HistoryErasures, HistoryWipes, Needle, NeedleRefused, Resets, ScreenEpoch, SearchLines,
    SurfaceLine, Term,
};
use baud_domain::{Codepoint, Col, Cols, LineIdx, Row, Rows};

use crate::overlay::{Caret, Composition, Hints, Measure};
use crate::select::WindowLine;
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
    /// The hints on each of `text`'s codepoints: the selection's colours,
    /// the query in the grid's own while it is selected ([`Query`]), and
    /// an input method's composition in its hints over them, underlined
    /// where it carries none.
    pub hints: Vec<Hints>,
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
            measure: Measure::default(),
            hints: self.hints.clone(),
            // The field draws its own caret, a bar at `caret`
            // (`Renderer::set_find`); the terminal's cursor is not moved.
            caret: Caret::Hidden,
        }
    }
}

/// Whether the field's query is selected: shown in the grid's own colours,
/// and replaced by what is typed next, as a find field reopened on the
/// last search shows it; or typed, and added to.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Query {
    #[default]
    Typed,
    Selected,
}

/// **What is being typed into the field**: the query's state, and an input
/// method's composition with its hints and its cursor in it -- drawn after
/// the query, each codepoint in its hints over the field's selection
/// colours, underlined where none are given, and the field's caret at its
/// cursor, as the terminal draws one at its own cursor (`Composition`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Typing<'a> {
    pub query: Query,
    pub composing: &'a [Codepoint],
    /// The hints on each of `composing`'s codepoints, or none at all.
    pub hints: &'a [Hints],
    pub caret: Caret,
}

/// **What the find overlay draws on the next frame**: every match's part
/// of the view -- the current one is drawn as the selection, by the
/// driver -- and the field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FindLook {
    pub marks: Vec<Mark>,
    pub field: Field,
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
    wipes: HistoryWipes,
    resets: Resets,
    geometry: (Cols, Rows),
}

impl Basis {
    fn of(term: &Term<'_>) -> Basis {
        Basis {
            epoch: term.screen_epoch(),
            erasures: term.history_erasures(),
            wipes: term.history_wipes(),
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

/// **One row's part of a match, on a view**: its line counted from the
/// view's top -- a window row, or a line past the window the display draws
/// on one of its lines ([`crate::Beyond`]) -- the columns, both inclusive,
/// and whether the match is the current one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mark {
    pub line: WindowLine,
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

    /// **Make `m` the current match**, if it is one of the matches: what a
    /// field reopened on the query it closed on starts from. `false`, and
    /// the current match left as it was, when it is not.
    pub fn select(&mut self, m: Found) -> bool {
        // The matches are in order of their first cell.
        let i = self.found.partition_point(|f| f.first < m.first);
        match self.found.get(i).filter(|f| **f == m).map(|_| i) {
            Some(i) => {
                self.current = Some(i);
                true
            }
            None => false,
        }
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
        self.field_typing(cols, Typing::default())
    }

    /// **The field with what is being typed into it** ([`Typing`]): the
    /// query, then the composition, as much of their end as fits, and the
    /// caret at the composition's cursor -- after it where the input method
    /// says nothing, hides it, or selects a range.
    pub fn field_typing(&self, cols: Cols, typing: Typing<'_>) -> Field {
        let width = cols.index().min(FIELD_COLS.max(cols.index() / 3));
        let status: String = match (self.refused, self.position()) {
            (Some(NeedleRefused::Empty), _) => String::new(),
            (Some(NeedleRefused::TooLong), _) => "too long".into(),
            (None, Some(p)) => format!("{} of {}", p.nth.value(), p.of.value()),
            (None, None) => "none".into(),
        };
        let fixed = 1 + LABEL.len() + 1 + status.len() + 1;
        let room = width.saturating_sub(fixed);
        let query = match typing.query {
            Query::Typed => Hints::SELECTION,
            Query::Selected => Hints::NONE,
        };
        // The composition's hints over the field's colours, as it is drawn
        // at the cursor: `whole` on every codepoint when it carries none,
        // and none past the last it carries.
        let rest = match typing.hints.is_empty() {
            true => Hints::WHOLE,
            false => Hints::NONE,
        };
        let composed = typing
            .hints
            .iter()
            .copied()
            .chain(core::iter::repeat(rest))
            .map(|h| Hints::SELECTION.with(h));
        let typed: Vec<(Codepoint, Hints)> = self
            .query
            .iter()
            .map(|&c| (c, query))
            .chain(typing.composing.iter().copied().zip(composed))
            .collect();
        // The caret before this many of `typed`.
        let at = self.query.len()
            + match typing.caret {
                Caret::At(n) => n.min(typing.composing.len()),
                Caret::End | Caret::Range(..) | Caret::Hidden => typing.composing.len(),
            };
        // The tail that fits, whole clusters, laid out as the field's
        // composition lays them ([`crate::overlay::clusters`]), after the
        // label's space: what joins that space is not shown.
        let cps: Vec<Codepoint> = typed.iter().map(|&(c, _)| c).collect();
        let (lead, clusters) = crate::overlay::clusters(&cps, Measure::default());
        let mut first = typed.len();
        let mut used = 0;
        let mut kept = clusters.len();
        while let Some(c) = kept.checked_sub(1).and_then(|k| clusters.get(k)) {
            let w = c.span.cells().index();
            if used + w > room {
                break;
            }
            used += w;
            first = c.codes.start;
            kept -= 1;
        }
        let first = first.max(lead);
        let shown = &typed[first..];
        // The caret before the codepoint at `at`: after every cluster that
        // ends by it, at the start of the one it is inside.
        let before: usize = clusters[kept..]
            .iter()
            .filter(|c| c.codes.end <= at)
            .map(|c| c.span.cells().index())
            .sum();
        let pad = width.saturating_sub(1 + LABEL.len() + used + status.len() + 1);
        let mut text: Vec<Codepoint> = chars(" ").chain(chars(LABEL)).collect();
        text.extend(shown.iter().map(|&(c, _)| c));
        let mut hints = vec![Hints::SELECTION; text.len() - shown.len()];
        hints.extend(shown.iter().map(|&(_, h)| h));
        text.extend(core::iter::repeat_n(Codepoint::from_char(' '), pad));
        text.extend(chars(&status));
        text.push(Codepoint::from_char(' '));
        hints.resize(text.len(), Hints::SELECTION);
        let col = cols.index().saturating_sub(width);
        Field {
            row: Row::from_index(0),
            col: Col::at_index(col),
            caret: Col::at_index(col + 1 + LABEL.len() + before),
            text,
            hints,
        }
    }

    /// **Every match's part of the view `back` lines up**, row by row: a
    /// match the writer wrapped across rows marks each. The lines past the
    /// view that the display may draw on its own ([`crate::select::Beyond::of`])
    /// are marked too, counted from the view's top.
    pub fn marks(&self, term: &Term<'_>, back: Rows) -> Vec<Mark> {
        let (cols, rows) = term.geometry();
        let lines = term.search_lines();
        let reach = crate::select::Beyond::of(term, back);
        let window = lines
            .top
            .minus(term.looking_back(back))
            .unwrap_or(LineIdx::FIRST);
        let top = window.minus(reach.above).unwrap_or(LineIdx::FIRST);
        let end = window.plus(rows).plus(reach.below);
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
                let at = match line.since(window) {
                    Some(y) => WindowLine::of(Row::from_index(y.index())),
                    None => match window.since(line) {
                        Some(k) => WindowLine::of(Row::ZERO).up(k),
                        None => break,
                    },
                };
                out.push(Mark {
                    line: at,
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
