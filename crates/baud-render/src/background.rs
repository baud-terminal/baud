//! **The background word of every cell, and which rows of them changed**
//! since a presenter last took them ([`Backgrounds::take_dirty`]). Every
//! write goes through a method here, and each marks the rows it changed,
//! so a presenter that uploads only the rows marked holds every word
//! written. A write of the word a cell already holds marks nothing.

use crate::color::Argb;
use baud_domain::{Cols, Row, Rows};
use core::ops::Range;

/// **The rows of the background words that changed since the last take**
/// ([`crate::Renderer::take_background_dirty`]): none, rows `first..end`
/// of the buffer [`crate::Renderer::background`] reads, or all of them, as
/// after a resize.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BackgroundDirty {
    #[default]
    Clean,
    Rows {
        first: Row,
        end: Row,
    },
    Whole,
}

impl BackgroundDirty {
    /// This and rows `first..end` together.
    fn with_rows(self, first: Row, end: Row) -> BackgroundDirty {
        match self {
            BackgroundDirty::Clean => BackgroundDirty::Rows { first, end },
            BackgroundDirty::Rows { first: a, end: b } => BackgroundDirty::Rows {
                first: Row::from_index(a.index().min(first.index())),
                end: Row::from_index(b.index().max(end.index())),
            },
            BackgroundDirty::Whole => BackgroundDirty::Whole,
        }
    }
}

/// **The background words, `cols` a row, and their change since the last
/// take.** Its words are written only through its own methods.
pub(crate) struct Backgrounds {
    words: Vec<Argb>,
    cols: Cols,
    dirty: BackgroundDirty,
}

impl Backgrounds {
    /// No words yet; the first take reads all of them changed.
    pub(crate) fn new() -> Backgrounds {
        Backgrounds {
            words: Vec::new(),
            cols: Cols::of_index(0),
            dirty: BackgroundDirty::Whole,
        }
    }

    /// `cols` by `rows` words, each the default; all of them changed.
    pub(crate) fn reset(&mut self, cols: Cols, rows: Rows) {
        self.words = vec![Argb(0); cols.index() * rows.index()];
        self.cols = cols;
        self.dirty = BackgroundDirty::Whole;
    }

    /// Every word, row-major.
    pub(crate) fn words(&self) -> &[Argb] {
        &self.words
    }

    /// The words `at` each made `v`, their rows marked where one changes.
    pub(crate) fn fill(&mut self, at: Range<usize>, v: Argb) {
        if self.words[at.clone()].iter().any(|&w| w != v) {
            self.words[at.clone()].fill(v);
            self.mark(at);
        }
    }

    /// The words `at` made `v`'s, their rows marked where one changes.
    pub(crate) fn copy(&mut self, at: Range<usize>, v: &[Argb]) {
        if self.words[at.clone()] != *v {
            self.words[at.clone()].copy_from_slice(v);
            self.mark(at);
        }
    }

    /// What changed since the last take, which then reads none.
    pub(crate) fn take_dirty(&mut self) -> BackgroundDirty {
        core::mem::take(&mut self.dirty)
    }

    /// The rows the words `at` lie on marked changed.
    fn mark(&mut self, at: Range<usize>) {
        let cols = self.cols.index();
        if at.is_empty() || cols == 0 {
            return;
        }
        let (first, end) = (at.start / cols, (at.end - 1) / cols + 1);
        self.dirty = self
            .dirty
            .with_rows(Row::from_index(first), Row::from_index(end));
    }
}
