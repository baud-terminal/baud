//! **What a frame changed of the one before it, in grid rows**: everything,
//! or these rows and no pixel outside them. A GPU path that keeps its
//! surfaces draws only the rows a surface is missing -- the union over every
//! frame since it was last drawn into -- and loads the rest, as the Wayland
//! presenter copies only a buffer's stale strips (`baud-present-wayland`'s
//! `damage.rs`). Only a frame where nothing but the cursor, its blink
//! animation, its trail or what moves with it -- the cursor guide, and the
//! badge's cut round a block -- changed names rows; any other change is
//! [`Redraw::Whole`].

use baud_domain::{Row, Rows};

/// How many grid rows a [`RowSet`] can name: the most a screen has, 150 at
/// `MAX_GRID`, its status line and a smooth scroll's leaving row fit. A row
/// past it makes the frame [`Redraw::Whole`].
pub const REDRAW_ROWS: usize = 256;

const WORDS: usize = REDRAW_ROWS / 64;

/// A set of grid rows, one bit a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RowSet([u64; WORDS]);

impl RowSet {
    /// No row.
    pub const NONE: RowSet = RowSet([0; WORDS]);

    /// The set with `r` in it, or `None` past [`REDRAW_ROWS`].
    pub fn with(self, r: Row) -> Option<RowSet> {
        let i = r.index();
        let mut words = self.0;
        *words.get_mut(i / 64)? |= 1 << (i % 64);
        Some(RowSet(words))
    }

    /// The rows `from..to`, or `None` when one is past [`REDRAW_ROWS`].
    pub fn span(self, from: Row, to: Row) -> Option<RowSet> {
        (from.index()..to.index()).try_fold(self, |s, i| s.with(Row::from_index(i)))
    }

    pub fn union(self, other: RowSet) -> RowSet {
        let mut words = self.0;
        for (w, o) in words.iter_mut().zip(other.0) {
            *w |= o;
        }
        RowSet(words)
    }

    pub fn has(self, r: Row) -> bool {
        let i = r.index();
        self.0.get(i / 64).is_some_and(|w| w & (1 << (i % 64)) != 0)
    }

    pub fn is_empty(self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    /// How many rows it names.
    pub fn len(self) -> Rows {
        Rows::of_index(
            self.0
                .iter()
                .map(|w| usize::try_from(w.count_ones()).unwrap_or(0))
                .sum(),
        )
    }

    /// Each run of consecutive rows, top to bottom, as its first row and
    /// how many.
    pub fn runs(self) -> impl Iterator<Item = (Row, Rows)> {
        let mut r = 0usize;
        core::iter::from_fn(move || {
            while r < REDRAW_ROWS && !self.has(Row::from_index(r)) {
                r += 1;
            }
            let start = r;
            while r < REDRAW_ROWS && self.has(Row::from_index(r)) {
                r += 1;
            }
            (start < r).then(|| (Row::from_index(start), Rows::of_index(r - start)))
        })
    }
}

/// What a frame changed of the one before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Redraw {
    /// Anything: a frame whose every pixel is drawn.
    #[default]
    Whole,
    /// These rows, and no pixel outside them; none at all is a frame the
    /// same as the last.
    Rows(RowSet),
}

impl Redraw {
    /// A frame that changed nothing.
    pub const NOTHING: Redraw = Redraw::Rows(RowSet::NONE);

    /// What two frames in turn changed together.
    pub fn union(self, other: Redraw) -> Redraw {
        match (self, other) {
            (Redraw::Rows(a), Redraw::Rows(b)) => Redraw::Rows(a.union(b)),
            _ => Redraw::Whole,
        }
    }

    /// `self` and the row `r`, whole past [`REDRAW_ROWS`].
    pub fn with(self, r: Row) -> Redraw {
        match self {
            Redraw::Rows(s) => s.with(r).map_or(Redraw::Whole, Redraw::Rows),
            Redraw::Whole => Redraw::Whole,
        }
    }

    /// `self` and the rows `from..to`, whole past [`REDRAW_ROWS`].
    pub fn with_span(self, from: Row, to: Row) -> Redraw {
        match self {
            Redraw::Rows(s) => s.span(from, to).map_or(Redraw::Whole, Redraw::Rows),
            Redraw::Whole => Redraw::Whole,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_are_each_stretch_of_rows_in_order() {
        let s = RowSet::NONE
            .span(Row::from_index(2), Row::from_index(5))
            .and_then(|s| s.with(Row::from_index(9)))
            .and_then(|s| s.with(Row::from_index(255)))
            .unwrap();
        let runs: Vec<_> = s.runs().map(|(r, n)| (r.index(), n.index())).collect();
        assert_eq!(runs, [(2, 3), (9, 1), (255, 1)]);
        assert_eq!(s.len(), Rows::of_index(5));
        assert!(RowSet::NONE.with(Row::from_index(REDRAW_ROWS)).is_none());
    }

    #[test]
    fn a_whole_frame_absorbs_any_rows() {
        let one = Redraw::NOTHING.with(Row::from_index(3));
        assert_eq!(one.union(Redraw::Whole), Redraw::Whole);
        assert_eq!(Redraw::Whole.union(one), Redraw::Whole);
        assert_eq!(
            Redraw::NOTHING.with(Row::from_index(REDRAW_ROWS)),
            Redraw::Whole
        );
        assert_eq!(one.union(Redraw::NOTHING), one);
    }
}
