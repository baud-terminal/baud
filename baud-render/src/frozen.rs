//! **A frozen row drawn in its own renditions**, through slots of the
//! frame's resolved table that no row of the frame names.
//!
//! The core freezes what lies off the active screen when its rendition table
//! fills (`baud-core`'s `freeze.rs`): a row behind the window then names ids
//! in its generation's words, not in the table, and the window scrolled back
//! over it shows cells whose ids a reader of the table alone resolves to
//! another rendition. `[[source: dec-std-070]]`'s SGR, page 5-104, note 3:
//! "In no way does the selection of current rendition affect the rendition of
//! characters already entered into the display."
//!
//! **The frame's copy is the renderer's, and a frame names at most one cell's
//! rendition a cell.** Every path draws a copy of the window: the cell path
//! its set's (`Screens::take`), the CPU and instance paths their own row
//! buffer. A reader of a copy reads each copied cell's id in one resolved
//! table. A window holds at most 61,200 cells at the app's provisioning, its
//! status row and its leaving row 816 more, and the table 65,536 slots, so
//! the slots the frame's live cells name leave room for every rendition its
//! frozen cells name. Each frozen cell is given such a slot, its
//! generation's word resolved into it and its copied id rewritten, and the
//! slot's own value is kept to put back before the table is brought up to
//! date again. A display line that joins two stored rows
//! ([`crate::rebreak`]) names its cells' ids in each one's own table, so its
//! sources are runs of columns ([`Words::Runs`]).
//!
//! **A reader of a word reads it where its id names it** ([`Words`]): a
//! stored row through its own table (`Viewport::styles`), a display line
//! through each run's, and a lent copy through the words its lent slots
//! were given. The ids a lending gives mean what they mean only under that
//! lending, so the table's stamp carries it ([`StyleStamp`]), and a reader
//! that keeps anything by a copied cell keeps the stamp beside it.

use crate::{Renderer, Resolved, StyleStamp};
use baud_core::Term;
use baud_domain::{Col, FrozenSerial, PackedCell, RowStyles, SlotSet, StyleId, StyleWord};
use core::ops::Range;
use std::collections::HashMap;
use std::rc::Rc;

/// **Where a run of cells' rendition ids name their words**: one table for
/// a stored row, a run of columns each for a display line that joins rows,
/// or the words a frame's lent slots were given.
#[derive(Clone, Copy, Debug)]
pub enum Words<'a> {
    /// Every cell of the row in one table: the live one, or the generation
    /// its row was frozen into.
    Row(RowStyles<'a>),
    /// Runs of columns, each in its own stored row's table: a display line
    /// that joins rows ([`crate::rebreak`]). A column in no run is live.
    Runs(&'a [(Range<usize>, RowStyles<'a>)]),
    /// A copy whose frozen cells were lent slots of its table ([`Lent`]):
    /// a lent slot names the word it was lent for, by slot, any other the
    /// live table's. Empty where nothing is lent.
    Lent(&'a [Option<StyleWord>]),
    /// One cell whose word was read where its row named it: a cell of a
    /// joined letter's word, kept by the layout ([`crate::bidi::WordCell`]).
    Word(StyleWord),
}

impl<'a> Words<'a> {
    /// Every id in the live table.
    pub const LIVE: Words<'static> = Words::Row(RowStyles::Live);

    /// The rendition cell `cell`, at column `col` of its row, stands for.
    pub fn word(self, term: &Term<'_>, col: Col, cell: PackedCell) -> StyleWord {
        match self {
            Words::Row(s) => term.style_in(s, cell),
            Words::Runs(runs) => term.style_in(run_at(runs, col), cell),
            Words::Lent(l) => match l.get(cell.style.index()) {
                Some(Some(w)) => *w,
                _ => term.style_of(cell),
            },
            Words::Word(w) => w,
        }
    }

    /// Whether every id names the live table, so a reader may keep what it
    /// reads by the slot: a lent copy is, where it lent nothing.
    pub fn is_live(self) -> bool {
        match self {
            Words::Row(s) => s.is_live(),
            Words::Runs(runs) => runs.iter().all(|(_, s)| s.is_live()),
            Words::Lent(l) => l.is_empty(),
            Words::Word(_) => false,
        }
    }

    /// The table column `col`'s id names, for a stored row or a display
    /// line; a lent copy's names the live table.
    fn styles_at(self, col: Col) -> RowStyles<'a> {
        match self {
            Words::Row(s) => s,
            Words::Runs(runs) => run_at(runs, col),
            Words::Lent(_) | Words::Word(_) => RowStyles::Live,
        }
    }
}

/// **Where a row's cells name their ids, kept with the row**: one table,
/// or a display line's runs, owned so a reader can keep it beside the
/// row's cells while it reads them ([`Words`]).
#[derive(Clone, Debug)]
pub enum RowSource<'t> {
    One(RowStyles<'t>),
    Runs(Vec<(Range<usize>, RowStyles<'t>)>),
}

impl RowSource<'_> {
    pub fn words(&self) -> Words<'_> {
        match self {
            RowSource::One(s) => Words::Row(*s),
            RowSource::Runs(r) => Words::Runs(r),
        }
    }
}

fn run_at<'a>(runs: &[(Range<usize>, RowStyles<'a>)], col: Col) -> RowStyles<'a> {
    runs.iter()
        .find(|(r, _)| r.contains(&col.index()))
        .map_or(RowStyles::Live, |(_, s)| *s)
}

/// **Which lending a resolved table holds**: none, or one the renderer has
/// numbered, the same number for the same slots given for the same ids. Two
/// copies stamped alike resolve every id alike ([`StyleStamp`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct Lending(crate::Tally);

/// One lent slot and the frozen id it was lent for.
type Given = (StyleId, FrozenSerial, StyleId);

/// **The lendings the renderer has numbered**, newest last: each the slots it
/// gave, in order. A table lent the same slots for the same ids again is
/// stamped with the same number, so a cache kept across the cell path's
/// sets, which take turns, is not walked again for a lending it has seen.
#[derive(Default)]
pub(crate) struct Lendings {
    seen: Vec<(Vec<Given>, Lending)>,
    /// The lendings numbered so far.
    made: crate::Tally,
}

/// The lendings kept: the cell path's three sets and the CPU path's table,
/// twice over.
const LENDINGS_KEPT: usize = 8;

impl Lendings {
    fn number(&mut self, given: Vec<Given>) -> Lending {
        if given.is_empty() {
            return Lending::default();
        }
        if let Some(i) = self.seen.iter().position(|(g, _)| *g == given) {
            let e = self.seen.remove(i);
            let n = e.1;
            self.seen.push(e);
            return n;
        }
        self.made.bump();
        let n = Lending(self.made);
        if self.seen.len() == LENDINGS_KEPT {
            self.seen.remove(0);
        }
        self.seen.push((given, n));
        n
    }
}

/// **The slots one resolved table lent its frame's frozen rows**, and what
/// each held, put back before the table is brought up to date again
/// ([`Renderer::give_back`]).
#[derive(Default)]
pub struct Lent {
    slots: Vec<(StyleId, Resolved)>,
    /// The word each lent slot was given, by slot: sized to the table at
    /// the first lending, `None` where nothing is lent. Shared, so a reader
    /// that converts through the renderer can hold it while it does.
    words: Rc<Vec<Option<StyleWord>>>,
    /// The resolved word of a frozen id, kept by its generation's serial,
    /// under the inputs it was resolved under: a generation's words never
    /// change, and a rebuilt one has a new serial.
    cache: HashMap<(FrozenSerial, usize), Resolved>,
    inputs: Option<crate::renderer::StyleInputs>,
    /// The lending its table holds now.
    lending: Lending,
}

impl Lent {
    /// Slots lent and not yet given back.
    pub fn lent(&self) -> crate::Tally {
        crate::Tally::of(self.slots.len())
    }

    /// **The rendition a cell of a lent copy stands for**: the word its slot
    /// was lent for, or the live table's where it was lent none.
    pub fn word(&self, term: &Term<'_>, cell: PackedCell) -> StyleWord {
        self.words().word(term, Col::ZERO, cell)
    }

    /// Where a cell of the lent copy names its word ([`Words::Lent`]).
    pub fn words(&self) -> Words<'_> {
        match self.slots.is_empty() {
            true => Words::Lent(&[]),
            false => Words::Lent(&self.words[..]),
        }
    }

    /// The words, held apart from the lending: what a reader that borrows
    /// the renderer while it reads them keeps ([`Held::words`]). Nothing
    /// is held where nothing is lent.
    pub(crate) fn held(&self) -> Held {
        Held((!self.slots.is_empty()).then(|| self.words.clone()))
    }
}

/// **A lending's words, held apart from it** ([`Lent::held`]).
#[derive(Clone, Default)]
pub(crate) struct Held(Option<Rc<Vec<Option<StyleWord>>>>);

impl Held {
    pub(crate) fn words(&self) -> Words<'_> {
        Words::Lent(self.0.as_deref().map_or(&[], |w| &w[..]))
    }
}

impl core::fmt::Debug for Lent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Lent")
            .field("lent", &self.slots.len())
            .field("lending", &self.lending)
            .finish()
    }
}

/// What lending did to a frame's copy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lend {
    /// No cell of the frame is frozen.
    Nothing,
    /// Frozen cells rewritten to lent slots.
    Rows,
    /// More renditions than the slots the frame leaves free: the cells past
    /// them keep their ids. Not reached at the app's provisioning.
    Short,
}

impl Renderer<'_, '_> {
    /// **The slots `lent` lent its last frame, put back** into `out`, the
    /// resolved table they were lent from, before the table is brought up to
    /// date for the next frame built into it.
    pub fn give_back(&mut self, out: &mut [Resolved], stamp: &mut StyleStamp, lent: &mut Lent) {
        let words = Rc::make_mut(&mut lent.words);
        for (k, was) in lent.slots.drain(..).rev() {
            if let Some(w) = words.get_mut(k.index()) {
                *w = None;
            }
            if let Some(dst) = out.get_mut(k.index()) {
                stamp.lent_lose(dst.shapes);
                stamp.lent_gain(was.shapes);
                *dst = was;
            }
        }
        lent.lending = Lending::default();
        stamp.set_lending(Lending::default());
    }

    /// **Each frozen cell of a frame's copy drawn through a slot of `out` no
    /// cell of the frame names**: `rows` is every row the frame copied, its
    /// cells and where its source names their ids ([`Words::Row`], or
    /// [`Words::Runs`] for a display line); a frozen cell is given a lent
    /// slot, its generation's word resolved into it.
    pub fn lend_frozen(
        &mut self,
        term: &Term<'_>,
        rows: &mut [(&mut [PackedCell], Words<'_>)],
        out: &mut [Resolved],
        stamp: &mut StyleStamp,
        lent: &mut Lent,
    ) -> Lend {
        if rows.iter().all(|(_, w)| w.is_live()) {
            return Lend::Nothing;
        }
        let inputs = self.style_inputs(term);
        if lent.inputs != Some(inputs) {
            lent.cache.clear();
            lent.inputs = Some(inputs);
        }
        if lent.words.len() != out.len() {
            lent.words = Rc::new(vec![None; out.len()]);
        }
        let words = Rc::make_mut(&mut lent.words);
        // The slots the live cells name, and slot 0, which a frozen row's
        // default names too.
        let mut named = SlotSet::new();
        named.insert(StyleId::DEFAULT);
        for (cells, w) in rows.iter() {
            match w {
                Words::Row(s) if s.is_live() => named.mark(cells),
                Words::Row(_) => {}
                _ => {
                    for (x, c) in cells.iter().enumerate() {
                        if w.styles_at(Col::at_index(x)).is_live() {
                            named.insert(c.style);
                        }
                    }
                }
            }
        }
        let mut given: HashMap<(FrozenSerial, usize), StyleId> = HashMap::new();
        let mut order: Vec<Given> = Vec::new();
        let mut next = 1usize;
        let mut lend = Lend::Rows;
        for (cells, w) in rows.iter_mut() {
            let w = *w;
            if w.is_live() {
                continue;
            }
            for (x, c) in cells.iter_mut().enumerate() {
                let RowStyles::Frozen(table) = w.styles_at(Col::at_index(x)) else {
                    continue;
                };
                if c.style == StyleId::DEFAULT {
                    continue;
                }
                let key = (table.serial(), c.style.index());
                let k = match given.get(&key) {
                    Some(k) => *k,
                    None => {
                        while next < out.len() && named.has(next) {
                            next += 1;
                        }
                        let Some(dst) = out.get_mut(next) else {
                            lend = Lend::Short;
                            continue;
                        };
                        let word = table.word(c.style);
                        let res = *lent
                            .cache
                            .entry(key)
                            .or_insert_with(|| self.resolve_slot(term, word));
                        let k = StyleId::from_index(next);
                        lent.slots.push((k, *dst));
                        words[next] = Some(word);
                        stamp.lent_lose(dst.shapes);
                        stamp.lent_gain(res.shapes);
                        *dst = res;
                        given.insert(key, k);
                        order.push((k, table.serial(), c.style));
                        named.insert(k);
                        k
                    }
                };
                c.style = k;
            }
        }
        lent.lending = self.lendings.number(order);
        stamp.set_lending(lent.lending);
        lend
    }

    /// **The frame being drawn reads its copy through `lent`'s words**: the
    /// cell path's set, whose copy the walk and the overlay read
    /// ([`Words::Lent`]). The CPU and instance paths' own lending sets it
    /// for theirs.
    pub fn set_frame_lent(&mut self, lent: &Lent) {
        self.frame_words = lent.held();
        self.frame_slots.clear();
        self.frame_slots.extend(lent.slots.iter().map(|&(k, _)| k));
        self.frame_lending = lent.lending;
    }

    /// The words the frame's copy names in its lent slots.
    pub(crate) fn frame_held(&self) -> Held {
        self.frame_words.clone()
    }

    /// **The slots the frame's table lent, lent this path's own table
    /// too**, with the words they were lent for ([`Renderer::set_frame_lent`]):
    /// what a reader of the frame's lent copy that converts through this
    /// path's table reads it through ([`Renderer::sized_lines`]). `table` is
    /// the frame's; this path's is up to date with it but for the slots
    /// lent, as two copies synced in one frame are. Given back as this
    /// path's own lending.
    pub(crate) fn adopt_frame(&mut self, table: &[Resolved]) {
        if self.frame_slots.is_empty() {
            return;
        }
        let (mut out, mut lent) = (
            core::mem::take(&mut self.styles),
            core::mem::take(&mut self.cpu_lent),
        );
        let mut stamp = self.styles_stamp;
        if lent.words.len() != out.len() {
            lent.words = Rc::new(vec![None; out.len()]);
        }
        let words = Rc::make_mut(&mut lent.words);
        let from = self.frame_words.0.as_deref().map_or(&[][..], |w| &w[..]);
        for &k in &self.frame_slots {
            let (Some(dst), Some(&res)) = (out.get_mut(k.index()), table.get(k.index())) else {
                continue;
            };
            lent.slots.push((k, *dst));
            words[k.index()] = from.get(k.index()).copied().flatten();
            stamp.lent_lose(dst.shapes);
            stamp.lent_gain(res.shapes);
            *dst = res;
        }
        lent.lending = self.frame_lending;
        stamp.set_lending(self.frame_lending);
        (self.styles, self.styles_stamp, self.cpu_lent) = (out, stamp, lent);
    }
}
