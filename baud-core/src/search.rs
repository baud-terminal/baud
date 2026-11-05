//! **Search in the scrollback**: a query's matches over the lines of the
//! live surface -- the retained history and the screen on the primary, the
//! screen alone on the alternate, which keeps none.
//!
//! A pure read of the ring, off the per-byte path: nothing here is reached
//! by a feed, and nothing is kept. The embedder asks for the matches of the
//! lines it has not read, after a read moved some, and keeps the list
//! itself; a line is named by its [`LineIdx`], the absolute number
//! `Book::top` counts, so a match keeps its number as output scrolls its
//! line up the ring, and is gone when the line retires past
//! [`SearchLines::oldest`].
//!
//! **The text a search reads is the text a selection copies**: a row to its last cell with
//! content, an empty cell before it read as a space, a wide character's
//! tail as nothing, a cluster's marks with their base, and a row the writer
//! wrapped joined to the next. A match never crosses a row that ended at a
//! newline. What matches is `baud_domain`'s `Needle`: canonical caseless,
//! whole clusters.

use baud_domain::{
    ClusterKey, Codepoint, Col, Content, LineIdx, Matcher, Needle, PackedCell, Row, Rows, Wrap,
};

use crate::term::{Screen, SurfaceLine, Term};

/// A cell of the live surface a search names: its line and its column.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Spot {
    pub line: LineIdx,
    pub col: Col,
}

/// **One match**: its first cell and its last, both inclusive, the last
/// a wide character's tail when the match ends on one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Found {
    pub first: Spot,
    pub last: Spot,
}

/// The lines a search reads: the oldest the ring retains, the screen's top
/// row, and one past its bottom row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SearchLines {
    pub oldest: LineIdx,
    pub top: LineIdx,
    pub end: LineIdx,
}

/// What an empty cell inside a line reads as.
const SPACE: Codepoint = Codepoint::from_char(' ');

impl Term<'_> {
    /// **The lines of the live surface a search reads**: on the primary,
    /// the history it retains and the screen, numbered as
    /// [`Term::history_mark`] numbers the window's top; on the alternate
    /// screen, its rows alone, in the numbering an image placed there
    /// takes.
    pub fn search_lines(&self) -> SearchLines {
        let book = &self.store.book;
        let (top, hist) = match self.cold.screen {
            Screen::Normal => (book.top, book.hist),
            Screen::Alternate => (book.alt_top, Rows::ZERO),
        };
        SearchLines {
            oldest: top.minus(hist).unwrap_or(LineIdx::FIRST),
            top,
            end: top.plus(self.rows),
        }
    }

    /// A line's cells, to the mark on a line of the history, and how it
    /// ended; `None` outside [`Term::search_lines`].
    fn search_row(&self, line: LineIdx) -> Option<(&[PackedCell], Wrap)> {
        let lines = self.search_lines();
        if line < lines.oldest {
            return None;
        }
        match line.since(lines.top) {
            Some(y) if y.index() < self.rows.index() => {
                let (vp, y) = (self.viewport(), Row::from_index(y.index()));
                Some((vp.row(y), vp.wrapped(y)))
            }
            Some(_) => None,
            None => {
                let back = lines.top.since(line)?;
                self.scrollback()
                    .line(baud_domain::Back::of(back.index().checked_sub(1)?))
            }
        }
    }

    /// The first line of the one `line` is part of: back over every line
    /// the writer wrapped onto the next, to the oldest retained.
    pub fn line_start(&self, line: LineIdx) -> LineIdx {
        let oldest = self.search_lines().oldest;
        let mut at = line.max(oldest);
        while let Some(prev) = at.minus(Rows::of_count(1)).filter(|p| *p >= oldest) {
            match self.search_row(prev) {
                Some((_, Wrap::Continues)) => at = prev,
                _ => break,
            }
        }
        at
    }

    /// **Every match of `needle` in the lines that begin at `from` or
    /// after and before `to`**, in reading order, to `each`: a line is the
    /// rows the writer wrapped into one, read whole, so `from` should be a
    /// line's first row ([`Term::line_start`]) and the last line read runs
    /// past `to` to its end. Lines older than the ring retains are not read.
    pub fn find(&self, needle: &Needle, from: LineIdx, to: LineIdx, mut each: impl FnMut(Found)) {
        let lines = self.search_lines();
        let mut line = from.max(lines.oldest);
        let start = Spot {
            line,
            col: Col::ZERO,
        };
        let mut m = Matcher::new(needle, (start, start));
        let mut found = |a: (Spot, Spot), b: (Spot, Spot)| {
            each(Found {
                first: a.0,
                last: b.1,
            })
        };
        while line < lines.end {
            let Some((cells, wrap)) = self.search_row(line) else {
                break;
            };
            let used = cells
                .iter()
                .rposition(|c| !c.content.is_empty())
                .map_or(0, |u| u + 1);
            for (x, cell) in cells[..used].iter().enumerate() {
                if cell.content.is_wide_tail() {
                    continue;
                }
                let span = match cells.get(x + 1).is_some_and(|c| c.content.is_wide_tail()) {
                    true => 1,
                    false => 0,
                };
                let at = (
                    Spot {
                        line,
                        col: Col::at_index(x),
                    },
                    Spot {
                        line,
                        col: Col::at_index(x + span),
                    },
                );
                // A mark stored alone reads as the mark, as a copy returns it
                // (`Content::DEFECTIVE`), its marks after the first as marks.
                let mut marks = self.cluster_marks(*cell);
                let base = self.base_of(*cell);
                let base = match base == Content::DEFECTIVE {
                    true => marks.next().and_then(Codepoint::new),
                    false => base.as_codepoint(),
                };
                let key = match base {
                    Some(base) => ClusterKey::cluster(base, marks),
                    None => ClusterKey::cluster(SPACE, core::iter::empty()),
                };
                m.cluster(key.as_slice(), at, &mut found);
            }
            line = line.next();
            if wrap == Wrap::Ended {
                m.line_end();
                if line >= to {
                    break;
                }
            }
        }
    }

    /// Where `line` lies on the live surface: a row of the screen, or a
    /// line of the history that many lines above the screen's top --
    /// `Above(1)` the newest -- or `None` outside [`Term::search_lines`].
    /// The answer a selection and a window over the history are numbered
    /// by, so an embedder places a match as it places either.
    pub fn surface_line(&self, line: LineIdx) -> Option<SurfaceLine> {
        let l = self.search_lines();
        if line < l.oldest || line >= l.end {
            return None;
        }
        match line.since(l.top) {
            Some(y) => Some(SurfaceLine::At(Row::from_index(y.index()))),
            None => l.top.since(line).map(SurfaceLine::Above),
        }
    }
}
