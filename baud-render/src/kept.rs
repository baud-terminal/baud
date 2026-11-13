//! **Each row's Devanagari words, kept while the row is the same**
//! (`crate::word`): the syllables the core parted on it, its words, and
//! each word laid -- the instance it is drawn at, its pen's start, each
//! syllable's pen boundary, its ink, and each glyph's slot and pen -- so a
//! frame draws an unchanged row's words from what an earlier frame laid,
//! in this frame's light, and lays none. The cursor and the selection
//! light a word over its syllables' spans and are read by the paint, never
//! by the layout, so a cursor moving over a row lays nothing.
//!
//! **A row is found by its cells, wherever on the screen it is**, and by
//! the order it is drawn in where UAX #9 reorders it: first the row the
//! same window row found last, compared whole as the layout's own test
//! keeps a row's answers (`crate::bidi`'s `Seen`); else the kept row whose
//! fold of the cells matches, compared whole, which is how a scrolled row
//! is found -- a fold of its content, as the run cache keys a run
//! (`crate::cache`); a row of another width holds other cells. What a
//! word is laid from besides its row's cells is in the [`Stamp`] -- the
//! cluster store's epoch, the cell's geometry, the atlas's sheets, the face
//! stack and, once SAPV has been seen, the rendition table -- and a stamp
//! that moves drops every row.
//! The modes that decide where a syllable is parted (`?2027`,
//! `join_spacing_marks`) are in the cells: the core parts by them and the
//! renderer re-forms by one rule whatever they are. Each word is laid
//! again where the word before it ends elsewhere, or a cell's face is
//! another: a syllable is shaped in its first cell's face, and each glyph
//! drawn from its own cell's where the two number their glyphs alike.
//!
//! **Bounded at twice the window's rows**, so a scroll either way finds
//! every row it moved; past that the row used longest ago goes, and a
//! window made smaller lets the rows used longest ago go until twice its
//! new rows are kept (`KeptRows::fit`). Kept, a row
//! costs its cells, eight bytes a cell, its syllables and words, and each
//! glyph's place (`Renderer::word_glyph_places`); the shaped runs stay in
//! the run cache, which a word is laid through, and are not copied here.

use crate::renderer::WordLayout;
use crate::stack::TextFace;
use crate::syllable::Syllable;
use crate::units::{Digest, Tally};
use crate::word::Word;
use baud_domain::{ClusterEpoch, Col, PackedCell, Row, Rows, StyleWrites, TableSerial};
use std::rc::Rc;

/// **What a word is laid from besides its row's cells**: the cluster
/// store's epoch -- a cell's id names another cluster after a sweep -- the
/// cell's geometry and the size the faces are drawn at, the atlas's sheets,
/// which a glyph's slot is on, the face stack, which moves with the theme
/// generation, and while SAPV has been seen the rendition table, whose
/// words decide which vowels a cell draws (`crate::renderer::drawn_marks`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Stamp {
    pub(crate) clusters: ClusterEpoch,
    pub(crate) metrics: crate::Metrics,
    pub(crate) scale: Option<baud_font::Scale>,
    pub(crate) grown: Tally,
    pub(crate) theme: crate::renderer::ThemeGen,
    /// The table's writes, and the lending its lent ids name
    /// (`crate::frozen`), once SAPV makes a cell's marks its rendition's.
    pub(crate) styles: Option<(TableSerial, StyleWrites, crate::frozen::Lending)>,
}

/// **A kept row**: where it is among [`KeptRows`]', and which row kept
/// there it is -- each row kept is numbered, so a row kept in another's
/// place, or kept again after a stamp moved, is never taken for the one
/// before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct KeptIx {
    at: RowSlot,
    made: Tally,
}

/// A place among [`KeptRows`]'.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RowSlot(usize);

/// A word's place among its row's, left to right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct WordIx(pub(crate) usize);

/// **How a row asked for was found**: kept, where the same window row
/// found it last; kept, elsewhere on the screen or not on it last; or found
/// on its cells now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Asked {
    Here,
    Moved,
    Found,
}

/// A row's cells folded, four lanes at a time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct RowKey(u64);

impl RowKey {
    /// Each cell's content and rendition, folded in four lanes so no lane
    /// waits on another's multiply; the flags are left to the compare.
    fn of(cells: &[PackedCell]) -> RowKey {
        // Every cell read to find the row kept, as a compare reads it.
        crate::counters::add(
            crate::counters::Counter::CellsCompared,
            crate::counters::Site::Kept,
            crate::Tally::of(cells.len()),
        );
        let word = |c: &PackedCell| {
            u64::from(c.content.bits()) | (u64::try_from(c.style.index()).unwrap_or(u64::MAX) << 32)
        };
        let mut lanes = [Digest::start(); 4];
        let (chunks, rest) = cells.as_chunks::<4>();
        for c in chunks {
            for (l, cell) in lanes.iter_mut().zip(c) {
                *l = l.fold(word(cell));
            }
        }
        let mut d = lanes.iter().fold(Digest::start(), |d, l| d.fold(l.value()));
        for cell in rest {
            d = d.fold(word(cell));
        }
        RowKey(
            d.fold(u64::try_from(cells.len()).unwrap_or(u64::MAX))
                .value(),
        )
    }
}

/// The pen the word before a word ends at, to the bit, which a word that
/// fits at no width closes on (`crate::word::lay`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Before(Option<u32>);

impl Before {
    fn of(prev: Option<f32>) -> Before {
        Before(prev.map(f32::to_bits))
    }
}

/// One word of a kept row, laid after the word before it ended at
/// `before` and each cell in its face of `styles`: `None` where it could
/// not be laid.
struct KeptWord {
    before: Before,
    styles: Vec<TextFace>,
    layout: Option<WordLayout>,
}

/// **The syllables and words of a row**, as the walk and the word pass
/// found them ([`crate::syllable::walk`], [`crate::word::of_row`]).
#[derive(PartialEq, Eq, Debug)]
pub(crate) struct Found {
    pub(crate) parted: Vec<Syllable>,
    pub(crate) words: Vec<Word>,
}

/// One kept row: its cells, when it was last asked for, what was found on
/// it and its words laid.
struct KeptRow {
    key: RowKey,
    made: Tally,
    cells: Vec<PackedCell>,
    /// The logical column each visual column shows, where the row is drawn
    /// in another order; empty where it is drawn in its own.
    order: Vec<Col>,
    used: Tally,
    found: Rc<Found>,
    laid: Vec<Option<KeptWord>>,
}

/// **The rows kept**, and which each window row found last.
#[derive(Default)]
pub(crate) struct KeptRows {
    rows: Vec<KeptRow>,
    last: Vec<Option<RowSlot>>,
    stamp: Option<Stamp>,
    clock: Tally,
    made: Tally,
}

impl KeptRows {
    /// **Window row `at`, holding `cells` drawn in `order`** -- the logical
    /// column each visual column shows, empty for a row drawn in its own:
    /// the kept row with those cells in that order, or a new one of what
    /// `find` finds on them, kept in place of the row used longest ago once
    /// `rows` twice over are kept. A `stamp` other than the one the rows
    /// were kept under drops them all first. A debug build finds a kept
    /// row's syllables and words again and requires them to be the ones
    /// kept.
    pub(crate) fn row(
        &mut self,
        stamp: Stamp,
        (at, rows): (Row, usize),
        (cells, order): (&[PackedCell], &[Col]),
        find: impl FnOnce() -> Found,
    ) -> (KeptIx, Rc<Found>, Asked) {
        if self.stamp != Some(stamp) {
            self.rows.clear();
            self.last.clear();
            self.stamp = Some(stamp);
        }
        self.clock.bump();
        let y = at.index();
        if self.last.len() <= y {
            self.last.resize(y + 1, None);
        }
        let same = |row: &KeptRow| {
            crate::counters::same_cells(crate::counters::Site::Kept, &row.cells, cells)
                && row.order == order
        };
        let hinted = self.last[y].filter(|k| self.rows.get(k.0).is_some_and(same));
        // A row not where it was is folded, and found by its fold.
        let mut key = None;
        let found = match hinted {
            Some(k) => Some((k, Asked::Here)),
            None => {
                let k = *key.get_or_insert_with(|| RowKey::of(cells));
                self.rows
                    .iter()
                    .position(|row| row.key == k && same(row))
                    .map(|i| (RowSlot(i), Asked::Moved))
            }
        };
        let (k, asked) = match found {
            Some((k, asked)) => {
                debug_assert!(
                    self.rows.get(k.0).is_some_and(|row| *row.found == find()),
                    "a kept row's syllables and words are the ones its cells hold"
                );
                (k, asked)
            }
            None => {
                let found = Rc::new(find());
                let laid = (0..found.words.len()).map(|_| None).collect();
                self.made.bump();
                let row = KeptRow {
                    key: key.unwrap_or_else(|| RowKey::of(cells)),
                    made: self.made,
                    cells: cells.to_vec(),
                    order: order.to_vec(),
                    used: self.clock,
                    found,
                    laid,
                };
                let k = match self.rows.len() < rows.saturating_mul(2).max(1) {
                    true => {
                        self.rows.push(row);
                        RowSlot(self.rows.len() - 1)
                    }
                    false => {
                        let oldest = self
                            .rows
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, r)| r.used.value())
                            .map_or(0, |(i, _)| i);
                        self.rows[oldest] = row;
                        RowSlot(oldest)
                    }
                };
                (k, Asked::Found)
            }
        };
        self.last[y] = Some(k);
        let row = &mut self.rows[k.0];
        row.used = self.clock;
        let ix = KeptIx {
            at: k,
            made: row.made,
        };
        (ix, Rc::clone(&row.found), asked)
    }

    /// **The window is now `rows` rows**: past twice that many kept, the
    /// rows used longest ago go, and which row each window row found last
    /// is forgotten, the rows left kept in other places.
    pub(crate) fn fit(&mut self, rows: Rows) {
        let most = rows.index().saturating_mul(2).max(1);
        if self.rows.len() > most {
            self.rows
                .sort_unstable_by_key(|row| core::cmp::Reverse(row.used.value()));
            self.rows.truncate(most);
            self.last.clear();
        }
        self.last.truncate(rows.index());
    }

    /// How many rows are kept.
    #[cfg(test)]
    pub(crate) fn held(&self) -> Rows {
        Rows::of_index(self.rows.len())
    }

    /// Kept row `k`, where it is still the row `k` names.
    fn get(&self, k: KeptIx) -> Option<&KeptRow> {
        self.rows.get(k.at.0).filter(|row| row.made == k.made)
    }

    /// Whether word `i` of kept row `k` was laid after a word ending at
    /// `prev`, each cell in its face of `styles`: a cell's rendition
    /// names its slot, and a slot written again names another face.
    pub(crate) fn has_word(
        &self,
        (k, i): (KeptIx, WordIx),
        prev: Option<f32>,
        styles: &[TextFace],
    ) -> bool {
        self.get(k)
            .and_then(|row| row.laid.get(i.0))
            .and_then(Option::as_ref)
            .is_some_and(|w| w.before == Before::of(prev) && w.styles == styles)
    }

    /// Word `i` of kept row `k`, laid after a word ending at `prev`, each
    /// cell in its face of `styles`, kept.
    pub(crate) fn keep_word(
        &mut self,
        (k, i): (KeptIx, WordIx),
        prev: Option<f32>,
        styles: &[TextFace],
        layout: Option<WordLayout>,
    ) {
        let slot = self
            .rows
            .get_mut(k.at.0)
            .filter(|row| row.made == k.made)
            .and_then(|row| row.laid.get_mut(i.0));
        if let Some(slot) = slot {
            *slot = Some(KeptWord {
                before: Before::of(prev),
                styles: styles.to_vec(),
                layout,
            });
        }
    }

    /// Word `i` of kept row `k` as it was laid: `None` where it was not, or
    /// could not be.
    pub(crate) fn word(&self, (k, i): (KeptIx, WordIx)) -> Option<&WordLayout> {
        self.get(k)
            .and_then(|row| row.laid.get(i.0))
            .and_then(Option::as_ref)
            .and_then(|w| w.layout.as_ref())
    }
}
