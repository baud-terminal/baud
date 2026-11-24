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
//!
//! **The table has a second lender: a struck link's grey**
//! ([`Renderer::lend`]). It lends in the same lending, after the frozen
//! cells and from the top of the table down, one slot a source rendition,
//! the slot's word the cell's own and only its resolved value grey; a cell
//! it finds no slot for keeps its own rendition and draws the strike alone.
//! So no drawn cell is ever given another rendition's colours by it, which
//! is the rendition table's rule (DEC STD 070's SGR note 3, above) kept for
//! the frame's copy as it is for the core's.

use crate::seeded::Map;
use crate::{Orders, Renderer, Resolved, StyleStamp};
use baud_core::Term;
use baud_domain::{
    Col, Cols, FrozenSerial, PackedCell, Row, RowStyles, Rows, SlotSet, Stride, StyleId, StyleWord,
};
use core::ops::Range;
use std::rc::Rc;

/// **Where a run of cells' rendition ids name their words**: one table for
/// a stored row, a run of columns each for a display line that joins rows,
/// or the words a frame's lent slots were given.
#[derive(Clone, Copy, Debug)]
pub enum Words<'a> {
    /// Every cell of the row in the terminal's live table ([`live`]).
    Live,
    /// Every cell of the row in one table: its band's live one, or the
    /// generation its row was frozen into.
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
    pub const LIVE: Words<'static> = Words::Live;

    /// The rendition cell `cell`, at column `col` of its row, stands for.
    pub fn word(self, term: &Term<'_>, col: Col, cell: PackedCell) -> StyleWord {
        match self {
            Words::Live => live(term).word(cell.style),
            Words::Row(s) => term.style_in(s, cell),
            Words::Runs(runs) => match run_at(runs, col) {
                Some(s) => term.style_in(s, cell),
                None => live(term).word(cell.style),
            },
            Words::Lent(l) => match l.get(cell.style.index()) {
                Some(Some(w)) => *w,
                _ => live(term).word(cell.style),
            },
            Words::Word(w) => w,
        }
    }

    /// Whether every id names the live table, so a reader may keep what it
    /// reads by the slot: a lent copy is, where it lent nothing.
    pub fn is_live(self) -> bool {
        match self {
            Words::Live => true,
            Words::Row(s) => s.is_live(),
            Words::Runs(runs) => runs.iter().all(|(_, s)| s.is_live()),
            Words::Lent(l) => l.is_empty(),
            Words::Word(_) => false,
        }
    }

    /// The generation column `col`'s id names its word in, for a stored row
    /// or a display line; `None` where it names the live table, as a lent
    /// copy's does.
    fn frozen_at(self, col: Col) -> Option<baud_domain::FrozenTable<'a>> {
        let s = match self {
            Words::Row(s) => Some(s),
            Words::Runs(runs) => run_at(runs, col),
            Words::Live | Words::Lent(_) | Words::Word(_) => None,
        };
        match s {
            Some(RowStyles::Frozen(t)) => Some(t),
            _ => None,
        }
    }
}

/// **The table a renderer's live ids name**: the terminal's one band's,
/// which every live row is in while the terminal keeps one band.
pub fn live<'t>(term: &'t Term<'_>) -> RowStyles<'t> {
    RowStyles::Live(live_table(term))
}

/// That table, as a view of copied rows carries it ([`baud_domain::Viewport::new`]).
pub fn live_table<'t>(term: &'t Term<'_>) -> baud_domain::LiveTable<'t> {
    term.styles().one()
}

/// **The tables of a copy whose row `p` is in band `lines[p]`**, as a view
/// of copied rows carries them ([`baud_domain::Viewport::new_in`]); a row
/// past `lines` in the first band.
pub fn lines_table<'t>(
    term: &'t Term<'_>,
    lines: &'t [baud_domain::Band],
) -> baud_domain::LiveBands<'t> {
    let view = term.styles();
    baud_domain::LiveBands::lines(view.slots(), view.band_slots().0, lines)
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

/// The table the run holding column `col` names; `None` in no run, which
/// is live.
fn run_at<'a>(runs: &[(Range<usize>, RowStyles<'a>)], col: Col) -> Option<RowStyles<'a>> {
    runs.iter()
        .find(|(r, _)| r.contains(&col.index()))
        .map(|(_, s)| *s)
}

/// **Which lending a resolved table holds**: none, or one the renderer has
/// numbered, the same number for the same slots given for the same ids. Two
/// copies stamped alike resolve every id alike ([`StyleStamp`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Lending(crate::Tally);

/// **Which lender gave a slot**: the frozen pass, for a generation's id, or
/// the strike, for a source rendition under one `[link] missing_color`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lender {
    Frozen(FrozenSerial),
    Struck(Option<FrozenSerial>, crate::MissingPaint),
}

/// One lent slot, who lent it, and the id it was lent for.
type Given = (StyleId, Lender, StyleId);

/// Where a struck cell's id names its word -- the live table, or a
/// generation by its serial -- and the id: one strike slot each.
type Source = (Option<FrozenSerial>, StyleId);

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

/// The struck renditions a table keeps resolved between lendings.
const STRUCK_KEPT: usize = 64;

/// The lendings kept: the cell path's three sets and the CPU path's table,
/// twice over.
const LENDINGS_KEPT: usize = 8;

impl Lendings {
    /// **The number of the lending that gave `given`**, in order: the one
    /// it had, where it was seen among the last [`LENDINGS_KEPT`], else a
    /// new one, `given` copied into the list the oldest kept lending
    /// leaves. A lending seen again copies nothing.
    fn number(&mut self, given: &[Given]) -> Lending {
        if given.is_empty() {
            return Lending::default();
        }
        if let Some(i) = self.seen.iter().position(|(g, _)| g.as_slice() == given) {
            let e = self.seen.remove(i);
            let n = e.1;
            self.seen.push(e);
            return n;
        }
        self.made.bump();
        let n = Lending(self.made);
        let mut kept = match self.seen.len() == LENDINGS_KEPT {
            true => self.seen.remove(0).0,
            false => Vec::new(),
        };
        kept.clear();
        kept.extend_from_slice(given);
        self.seen.push((kept, n));
        n
    }
}

/// **What one lending works in**, kept with the table it lends so a
/// lending allocates only where it finds more than the last one did: each
/// struck cell's source rendition and the cell, the slots the frozen pass
/// gave by generation and id, the slots in the order given, and the slot
/// each source rendition was lent. Reached only through
/// [`LendScratch::begin`], which empties every one, so no lending reads
/// what the last one left.
#[derive(Default)]
struct LendScratch {
    sources: Vec<(Source, StyleWord)>,
    marked: Vec<(usize, usize, usize)>,
    given: Map<(FrozenSerial, usize), StyleId>,
    order: Vec<Given>,
    slots: Vec<Option<StyleId>>,
}

impl LendScratch {
    /// Every buffer emptied, its room kept: the one way a lending reaches
    /// them.
    fn begin(&mut self) -> &mut LendScratch {
        self.sources.clear();
        self.marked.clear();
        self.given.clear();
        self.order.clear();
        self.slots.clear();
        self
    }
}

/// **The slots one resolved table lent its frame's frozen rows**, and what
/// each held, put back before the table is brought up to date again
/// ([`Renderer::give_back`]).
#[derive(Default)]
pub struct Lent {
    slots: Vec<(StyleId, Resolved)>,
    /// The word each lent slot was given, by slot: made at the first
    /// lending, sized to the table, `None` where nothing is lent. Shared, so
    /// a reader that converts through the renderer can hold it while it
    /// does. None before that lending, so a `Lent` made empty -- each time a
    /// path takes its own out of the renderer to lend it -- allocates
    /// nothing.
    words: Option<Rc<Vec<Option<StyleWord>>>>,
    /// The resolved word of a frozen id, kept by its generation's serial,
    /// under the inputs it was resolved under: a generation's words never
    /// change, and a rebuilt one has a new serial.
    cache: Map<(FrozenSerial, usize), Resolved>,
    /// A struck rendition as the cell draws it and as the strike's rule
    /// leaves it, under the same inputs: the rule decided once a
    /// rendition while the colours in force hold, never a cell.
    ///
    /// A handful a screen, so a list, emptied past [`STRUCK_KEPT`].
    struck_cache: Vec<((StyleWord, crate::MissingPaint), (Resolved, Resolved))>,
    inputs: Option<crate::renderer::StyleInputs>,
    /// **Each cell of the copy a lending rewrote**, by row of the copy and
    /// column, with the id it held, in the order rewritten: what puts a
    /// copy back as the screen's ([`Lent::restore`]), and what a compare
    /// with a lent copy reads it through ([`Lent::rewrites`]).
    rewrote: Vec<(usize, usize, StyleId)>,
    /// Struck renditions the lending found no slot for, drawn with the
    /// strike alone.
    short: crate::Tally,
    /// Whether the frozen pass lent a slot since the last give-back.
    frozen: FrozenLent,
    /// The lending its table holds now.
    lending: Lending,
    /// The buffers the next lending works in.
    scratch: LendScratch,
}

impl Lent {
    /// Slots lent and not yet given back.
    pub fn lent(&self) -> crate::Tally {
        crate::Tally::of(self.slots.len())
    }

    /// Struck renditions the last lendings found no slot for.
    pub fn short(&self) -> crate::Tally {
        self.short
    }

    /// **What each slot lent holds**, read from `out`, the table lent from,
    /// before it is given back, into `into`, whose room is kept; and
    /// whether the frozen pass lent any.
    pub(crate) fn lent_values_into(&self, out: &[Resolved], into: &mut LentValues) {
        into.slots.clear();
        into.slots.extend(
            self.slots
                .iter()
                .filter_map(|&(k, _)| out.get(k.index()).map(|&r| (k, r))),
        );
        into.frozen = self.frozen;
    }

    /// Whether the frozen pass lent a slot since the last give-back.
    pub(crate) fn frozen_lent(&self) -> FrozenLent {
        self.frozen
    }

    /// **Whether a copy drawn under the last lending may draw otherwise
    /// under this one where its cells did not change**: a frozen row's
    /// lending, in either, whose rewritten ids a compare cannot see moved,
    /// or a slot the last lending gave holding another rendition now while
    /// a cell of `cells`, this frame's copy, names it. Any other cell that
    /// drew otherwise was rewritten, so its id differs from the last
    /// frame's and its row's compare finds it.
    pub(crate) fn moved_from(
        &self,
        was: &LentValues,
        out: &[Resolved],
        cells: &[PackedCell],
    ) -> Moved {
        let moved = was.frozen == FrozenLent::Yes
            || self.frozen == FrozenLent::Yes
            || was.slots.iter().any(|&(k, r)| {
                out.get(k.index()) != Some(&r) && cells.iter().any(|c| c.style == k)
            });
        match moved {
            true => Moved::Yes,
            false => Moved::No,
        }
    }

    /// **The lending its table holds now**: two tables holding one resolve
    /// every lent id alike ([`StyleStamp`]); none lent is the default.
    pub fn lending(&self) -> Lending {
        self.lending
    }

    /// **The cells the last lending rewrote**, by row of the copy and
    /// column, with the id each held, in the order rewritten: a compare
    /// of the copy with the screen reads each through the id it held, the
    /// last rewrite of a cell first undone.
    pub fn rewrites(&self) -> &[(usize, usize, StyleId)] {
        &self.rewrote
    }

    /// **Cell `(y, x)` of the copy given another id**, `was` the id it
    /// held: a display line joining two bands numbered its own
    /// ([`crate::band_window::Joined`]). Put back and compared through as a
    /// lending's rewrite is.
    pub(crate) fn rewrote(&mut self, y: Row, x: Col, was: StyleId) {
        self.rewrote.push((y.index(), x.index(), was));
    }

    /// **The copy the last lending rewrote, put back as the screen's**:
    /// each rewritten cell given the id it held, the last rewrite first,
    /// `stride` cells a row. For a copy no frame in flight reads.
    pub fn restore(&mut self, copy: &mut [PackedCell], stride: Stride) {
        let stride = stride.index();
        for (y, x, was) in self.rewrote.drain(..).rev() {
            if let Some(c) = copy.get_mut(y * stride + x) {
                c.style = was;
            }
        }
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
            false => Words::Lent(self.words.as_deref().map_or(&[], |w| &w[..])),
        }
    }

    /// The words, held apart from the lending: what a reader that borrows
    /// the renderer while it reads them keeps ([`Held::words`]). Nothing
    /// is held where nothing is lent.
    pub(crate) fn held(&self) -> Held {
        Held(self.words.clone().filter(|_| !self.slots.is_empty()))
    }

    /// **The words, one a slot of `table`**: made, or made again at another
    /// size, where they are not -- the first lending, or a table grown --
    /// and kept otherwise.
    fn size_words(words: &mut Option<Rc<Vec<Option<StyleWord>>>>, table: &[Resolved]) {
        if words.as_ref().is_none_or(|w| w.len() != table.len()) {
            *words = Some(Rc::new(vec![None; table.len()]));
        }
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

/// **Whether the frozen pass lent a slot** since the last give-back.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum FrozenLent {
    Yes,
    #[default]
    No,
}

/// Whether a lending moved what an unchanged cell draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Moved {
    Yes,
    No,
}

/// **What a lending's slots held**, kept across a give-back so the next
/// lending can say whether it moved anything ([`Lent::moved_from`]).
#[derive(Default)]
pub(crate) struct LentValues {
    slots: Vec<(StyleId, Resolved)>,
    frozen: FrozenLent,
}

impl LentValues {
    /// Whether the frozen pass had lent a slot.
    pub(crate) fn frozen_lent(&self) -> FrozenLent {
        self.frozen
    }
}

/// **The rows of a frame's copy, as [`Renderer::lend`] reads them**: how
/// many, whether a cell of one names a row the core froze, each row's
/// cells with where its ids name their words, and one cell to rewrite.
pub(crate) trait LendRows {
    /// The rows of the copy.
    fn rows(&self) -> Rows;
    /// Whether a cell of a row names a table the core froze.
    fn frozen(&self) -> bool;
    /// Row `y`'s cells and where they name their words, handed to `f`;
    /// none past the last row. `orders` and `term` are the frame's, for a
    /// copy that asks the layout where a row names its words.
    fn with_row<T>(
        &mut self,
        orders: &Orders,
        term: &Term<'_>,
        y: Row,
        f: impl FnOnce(&mut [PackedCell], Words<'_>) -> T,
    ) -> Option<T>;
    /// Cell `x` of row `y`, if the copy holds it.
    fn cell(&mut self, y: Row, x: Col) -> Option<&mut PackedCell>;
}

/// The rows as a caller lists them, each its cells and its words.
impl LendRows for [(&mut [PackedCell], Words<'_>)] {
    fn rows(&self) -> Rows {
        Rows::of_count(self.len())
    }

    fn frozen(&self) -> bool {
        !self.iter().all(|(_, w)| w.is_live())
    }

    fn with_row<T>(
        &mut self,
        _: &Orders,
        _: &Term<'_>,
        y: Row,
        f: impl FnOnce(&mut [PackedCell], Words<'_>) -> T,
    ) -> Option<T> {
        let (cells, words) = self.get_mut(y.index())?;
        Some(f(cells, *words))
    }

    fn cell(&mut self, y: Row, x: Col) -> Option<&mut PackedCell> {
        self.get_mut(y.index())
            .and_then(|(cells, _)| cells.get_mut(x.index()))
    }
}

/// **Where each row of a [`FlatCopy`] names its words.**
pub(crate) enum CopyWords<'s> {
    /// Every row the live table's: no cell of the copy is frozen.
    Live,
    /// The window `back` lines up in its first `window` rows, each asked
    /// of the layout a row at a time ([`Orders::row_source`]): with no
    /// display held, each one table, so nothing is made for it. The row
    /// after them `after`'s, and every later row live.
    Window {
        back: Rows,
        window: Rows,
        after: Words<'s>,
    },
    /// The window's rows as the caller found them -- a display held, whose
    /// lines name runs -- the row after them `after`'s, every later row
    /// live.
    Each {
        rows: &'s [RowSource<'s>],
        after: Words<'s>,
    },
}

/// **A frame's copy of the window, lent in place**: one slice of cells at
/// its pitch, `width` of each row lent, `rows` rows, and where each names
/// its words. No list of rows or of their sources is made to lend it.
pub(crate) struct FlatCopy<'c, 's> {
    cells: &'c mut [PackedCell],
    pitch: Stride,
    width: Cols,
    rows: Rows,
    words: CopyWords<'s>,
}

impl<'c, 's> FlatCopy<'c, 's> {
    pub(crate) fn new(
        cells: &'c mut [PackedCell],
        pitch: Stride,
        width: Cols,
        rows: Rows,
        words: CopyWords<'s>,
    ) -> FlatCopy<'c, 's> {
        FlatCopy {
            cells,
            pitch,
            width,
            rows,
            words,
        }
    }

    /// Row `y`'s lent cells, if the copy holds the row.
    fn row(&mut self, y: Row) -> Option<&mut [PackedCell]> {
        if y.index() >= self.rows.index() {
            return None;
        }
        let at = y.index().checked_mul(self.pitch.index())?;
        let cells = self.cells.get_mut(at..)?;
        let n = self.width.index().min(cells.len());
        cells.get_mut(..n)
    }
}

impl LendRows for FlatCopy<'_, '_> {
    fn rows(&self) -> Rows {
        self.rows
    }

    fn frozen(&self) -> bool {
        !matches!(self.words, CopyWords::Live)
    }

    fn with_row<T>(
        &mut self,
        orders: &Orders,
        term: &Term<'_>,
        y: Row,
        f: impl FnOnce(&mut [PackedCell], Words<'_>) -> T,
    ) -> Option<T> {
        if y.index() >= self.rows.index() {
            return None;
        }
        let at = y.index().checked_mul(self.pitch.index())?;
        let cells = self.cells.get_mut(at..)?;
        let n = self.width.index().min(cells.len());
        let cells = cells.get_mut(..n)?;
        match &self.words {
            CopyWords::Live => Some(f(cells, Words::LIVE)),
            CopyWords::Window {
                back,
                window,
                after,
            } => match y.index().cmp(&window.index()) {
                core::cmp::Ordering::Less => {
                    let source = orders.row_source(term, *back, y);
                    Some(f(cells, source.words()))
                }
                core::cmp::Ordering::Equal => Some(f(cells, *after)),
                core::cmp::Ordering::Greater => Some(f(cells, Words::LIVE)),
            },
            CopyWords::Each { rows, after } => {
                let words = match rows.get(y.index()) {
                    Some(s) => s.words(),
                    None if y.index() == rows.len() => *after,
                    None => Words::LIVE,
                };
                Some(f(cells, words))
            }
        }
    }

    fn cell(&mut self, y: Row, x: Col) -> Option<&mut PackedCell> {
        self.row(y).and_then(|cells| cells.get_mut(x.index()))
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
        lent.rewrote.clear();
        lent.frozen = FrozenLent::No;
        // The frame's readers held these words through the last frame
        // (`Renderer::set_frame_lent`, the CPU path's convert): let them go
        // first, or the words are copied whole -- 65,536 slots at the
        // app's provisioning -- to be put back.
        if self
            .frame_words
            .0
            .as_ref()
            .zip(lent.words.as_ref())
            .is_some_and(|(w, l)| Rc::ptr_eq(w, l))
        {
            self.frame_words = Held::default();
        }
        if lent.slots.is_empty() {
            lent.lending = Lending::default();
            stamp.set_lending(Lending::default());
            return;
        }
        let words = Rc::make_mut(lent.words.get_or_insert_with(Rc::default));
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
    /// slot, its generation's word resolved into it. [`Renderer::lend`]
    /// with nothing struck.
    pub fn lend_frozen(
        &mut self,
        term: &Term<'_>,
        rows: &mut [(&mut [PackedCell], Words<'_>)],
        out: &mut [Resolved],
        stamp: &mut StyleStamp,
        lent: &mut Lent,
    ) -> Lend {
        self.lend(term, rows, &[], out, stamp, lent)
    }

    /// **[`Renderer::lend_frozen`], and each struck cell drawn grey through
    /// a slot of its own** -- the second lender of the table, after the
    /// frozen cells: `struck` holds, row by row of `rows` in order, runs of
    /// the columns a link that opens nothing covers, a selected cell left
    /// out by the caller, since the selection's colours win
    /// ([`Renderer::take_struck`]).
    ///
    /// **One lending, frozen first.** The frozen pass runs as it always
    /// has: every slot a live cell names is kept from it, and it takes the
    /// free slots from the bottom up, so nothing struck can leave a frozen
    /// cell short and draw it in another rendition's colours. The strike
    /// takes what is left, from the top of the table down, and only a slot
    /// the live table holds no rendition in -- which no live cell can name,
    /// a sweep freeing only slots no cell names -- so the copy is not read
    /// cell by cell for it, and the slots it gets, and so the lending's
    /// number, hold while the screen scrolls under the same links. Where both want the last slots, the frozen pass has
    /// already taken its own: a struck cell finding none keeps the slot it
    /// had -- its own rendition, or its frozen one -- and is drawn with the
    /// strike alone ([`Lent::short`]), never in a colour not its own.
    ///
    /// **One slot a source rendition**, as the frozen pass keys a slot by
    /// its generation and id: a struck cell is keyed by where its id names
    /// its word and the id, so every cell names one slot and the frozen
    /// budget holds. The slot's word is the cell's own rendition, so a
    /// reader of the copy through [`Words::Lent`] -- a copy, a report --
    /// reads the program's rendition; only its resolved value is
    /// [`Renderer::missing_rendition`]'s, the 4.5:1 rule decided against
    /// the colours in force once a source rendition a lending, never a
    /// cell. Its shapes are the cell's own, so the run cache keys alike.
    pub fn lend(
        &mut self,
        term: &Term<'_>,
        rows: &mut [(&mut [PackedCell], Words<'_>)],
        struck: &[(Row, Range<usize>)],
        out: &mut [Resolved],
        stamp: &mut StyleStamp,
        lent: &mut Lent,
    ) -> Lend {
        self.lend_rows(term, rows, struck, out, stamp, lent)
    }

    /// **[`Renderer::lend`] over any copy's rows** ([`LendRows`]): the
    /// paths' own copies lend as a [`FlatCopy`], in place. Every buffer the
    /// lending works in is `lent`'s, emptied first and kept after
    /// ([`LendScratch`]), and its number copies the slots given only for a
    /// lending it has not seen, so a frame lent as the last allocates
    /// nothing.
    pub(crate) fn lend_rows<R: LendRows + ?Sized>(
        &mut self,
        term: &Term<'_>,
        rows: &mut R,
        struck: &[(Row, Range<usize>)],
        out: &mut [Resolved],
        stamp: &mut StyleStamp,
        lent: &mut Lent,
    ) -> Lend {
        let frozen = rows.frozen();
        let struck_any = struck.iter().any(|(_, r)| !r.is_empty());
        if !frozen && !struck_any {
            return Lend::Nothing;
        }
        // **A terminal of more bands than one lends nothing yet**: a slot lent
        // a struck cell must be free in its own row's band, and the lending
        // keeps one table's. Its struck cells keep their own renditions, as a
        // struck cell finding no slot does ([`Lent::short`]); it freezes no
        // row, its bands holding every rendition their rows name.
        if term.styles().bands().len() > 1 {
            return Lend::Nothing;
        }
        let inputs = self.style_inputs(term);
        if lent.inputs != Some(inputs) {
            lent.cache.clear();
            lent.struck_cache.clear();
            lent.inputs = Some(inputs);
        }
        Lent::size_words(&mut lent.words, out);
        let scratch = lent.scratch.begin();
        let orders = self.bidi();
        // Each struck cell by the source rendition it names, and each source
        // rendition's word, read before the frozen pass rewrites a frozen
        // cell's id: a handful a screen, so a list, its last hit first.
        if struck_any {
            let mut hit = usize::MAX;
            for (row, cols) in struck {
                let (sources, marked) = (&mut scratch.sources, &mut scratch.marked);
                rows.with_row(orders, term, *row, |cells, w| {
                    for x in cols.clone() {
                        let Some(c) = cells.get(x) else { continue };
                        let source = (w.frozen_at(Col::at_index(x)).map(|t| t.serial()), c.style);
                        let ix = match sources.get(hit) {
                            Some(e) if e.0 == source => hit,
                            _ => match sources.iter().position(|e| e.0 == source) {
                                Some(i) => i,
                                None => {
                                    sources.push((source, w.word(term, Col::at_index(x), *c)));
                                    sources.len() - 1
                                }
                            },
                        };
                        hit = ix;
                        marked.push((row.index(), x, ix));
                    }
                });
            }
        }
        let words = Rc::make_mut(lent.words.get_or_insert_with(Rc::default));
        // The slots the live cells name, and slot 0, which a frozen row's
        // default names too: read off the copy for the frozen pass, which
        // may lend a slot the live table holds and no cell of the frame
        // names. The strike lends only a slot the live table holds no
        // rendition in, which no live cell can name, so a copy with no
        // frozen row is not read for it.
        let mut named = SlotSet::new();
        named.insert(StyleId::DEFAULT);
        let count = rows.rows().index();
        for y in (0..count).filter(|_| frozen) {
            rows.with_row(orders, term, Row::from_index(y), |cells, w| match w {
                Words::Live => named.mark(cells),
                Words::Row(s) if s.is_live() => named.mark(cells),
                Words::Row(_) => {}
                _ => {
                    for (x, c) in cells.iter().enumerate() {
                        if w.frozen_at(Col::at_index(x)).is_none() {
                            named.insert(c.style);
                        }
                    }
                }
            });
        }
        let mut next = 1usize;
        let mut lend = match frozen {
            true => Lend::Rows,
            false => Lend::Nothing,
        };
        for y in (0..count).filter(|_| frozen) {
            rows.with_row(orders, term, Row::from_index(y), |cells, w| {
                if w.is_live() {
                    return;
                }
                for (x, c) in cells.iter_mut().enumerate() {
                    let Some(table) = w.frozen_at(Col::at_index(x)) else {
                        continue;
                    };
                    if c.style == StyleId::DEFAULT {
                        continue;
                    }
                    let key = (table.serial(), c.style.index());
                    let k = match scratch.given.get(&key) {
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
                            scratch.given.insert(key, k);
                            scratch
                                .order
                                .push((k, Lender::Frozen(table.serial()), c.style));
                            lent.frozen = FrozenLent::Yes;
                            named.insert(k);
                            k
                        }
                    };
                    lent.rewrote.push((y, x, c.style));
                    c.style = k;
                }
            });
        }
        // The strike's pass, over what the frozen one left, from the top:
        // a slot a source rendition, the rule decided once for each.
        let paint = self.missing_paint();
        let mut top = out.len();
        for &(source, word) in &scratch.sources {
            let found = lent.struck_cache.iter().find(|e| e.0 == (word, paint));
            let (under, res) = match found {
                Some(e) => e.1,
                None => {
                    let under = self.resolve_slot(term, word);
                    let pair = (under, self.missing_rendition(term, under));
                    if lent.struck_cache.len() >= STRUCK_KEPT {
                        lent.struck_cache.clear();
                    }
                    lent.struck_cache.push(((word, paint), pair));
                    pair
                }
            };
            // A cell the rule leaves as it is -- too little contrast,
            // concealed -- needs no slot.
            if res == under {
                scratch.slots.push(None);
                continue;
            }
            let held = |k: usize| {
                named.has(k)
                    || term
                        .styles()
                        .one()
                        .slots()
                        .get(k)
                        .is_some_and(|s| s.word().is_some())
            };
            while top > next && held(top - 1) {
                top -= 1;
            }
            if top <= next || top <= 1 {
                lent.short.bump();
                scratch.slots.push(None);
                continue;
            }
            top -= 1;
            let dst = &mut out[top];
            let k = StyleId::from_index(top);
            lent.slots.push((k, *dst));
            words[top] = Some(word);
            stamp.lent_lose(dst.shapes);
            stamp.lent_gain(res.shapes);
            *dst = res;
            scratch
                .order
                .push((k, Lender::Struck(source.0, paint), source.1));
            named.insert(k);
            scratch.slots.push(Some(k));
        }
        for &(y, x, ix) in &scratch.marked {
            let Some(k) = scratch.slots.get(ix).copied().flatten() else {
                continue;
            };
            if let Some(c) = rows.cell(Row::from_index(y), Col::at_index(x)) {
                lent.rewrote.push((y, x, c.style));
                c.style = k;
                if lend == Lend::Nothing {
                    lend = Lend::Rows;
                }
            }
        }
        lent.lending = self.lendings.number(&scratch.order);
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
        Lent::size_words(&mut lent.words, &out);
        let words = Rc::make_mut(lent.words.get_or_insert_with(Rc::default));
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

#[cfg(test)]
mod tests {
    use super::{Given, LENDINGS_KEPT, LendScratch, Lender, Lending, Lendings};
    use crate::MissingPaint;
    use baud_domain::{FrozenSerial, StyleId, StyleWord};

    /// A lending of one slot, `slot`, given a struck rendition `id`.
    fn one(slot: StyleId, id: StyleId) -> [Given; 1] {
        [(slot, Lender::Struck(None, MissingPaint::Gray), id)]
    }

    /// **A lending seen again is numbered as it was**, among the kept
    /// ones, and none is numbered for nothing given. Named break: `number`
    /// never finding a lending among the kept ones, so each is numbered
    /// anew.
    #[test]
    fn a_lending_seen_again_is_numbered_as_it_was() {
        let mut l = Lendings::default();
        let (a, b) = (
            one(StyleId::from_index(9), StyleId::from_index(1)),
            one(StyleId::from_index(9), StyleId::from_index(2)),
        );
        let (na, nb) = (l.number(&a), l.number(&b));
        assert_ne!(na, nb, "two lendings, two numbers");
        assert_ne!(na, Lending::default(), "a lending is numbered");
        assert_eq!(l.number(&a), na, "the first again");
        assert_eq!(l.number(&b), nb, "the second again");
        assert_eq!(l.number(&[]), Lending::default(), "nothing given");
    }

    /// **A lending past the kept ones is numbered anew, into the list the
    /// oldest leaves**: the oldest, given again, is another number, and
    /// the lending that took its list is found as itself after it -- the
    /// list was emptied before it was filled. Named break: the oldest's
    /// list filled without being emptied.
    #[test]
    fn a_lending_past_the_kept_ones_is_numbered_anew() {
        let mut l = Lendings::default();
        let first: Vec<Lending> = (0..LENDINGS_KEPT)
            .map(|k| l.number(&one(StyleId::from_index(100), StyleId::from_index(k + 1))))
            .collect();
        let past = one(StyleId::from_index(100), StyleId::from_index(50));
        let n = l.number(&past);
        assert!(!first.contains(&n), "a new number for a new lending");
        assert_eq!(l.number(&past), n, "and the same again");
        let oldest = one(StyleId::from_index(100), StyleId::from_index(1));
        let again = l.number(&oldest);
        assert!(
            !first.contains(&again) && again != n,
            "the oldest, past the kept ones, is numbered anew"
        );
        assert_eq!(l.number(&past), n, "the newer lending is still itself");
    }

    /// **A lending begun reads nothing the last one left**: every buffer
    /// empty, its room kept. Named break: one buffer left as it was.
    #[test]
    fn a_lending_begun_reads_nothing_the_last_one_left() {
        let mut s = LendScratch::default();
        let k = StyleId::from_index(3);
        s.sources.push(((None, k), StyleWord::DEFAULT));
        s.marked.push((1, 2, 0));
        s.order.extend(one(k, k));
        s.slots.push(Some(k));
        s.given.insert((FrozenSerial::default(), 3), k);
        let begun = s.begin();
        assert!(
            begun.sources.is_empty()
                && begun.marked.is_empty()
                && begun.given.is_empty()
                && begun.order.is_empty()
                && begun.slots.is_empty(),
            "every buffer empty"
        );
        assert!(
            begun.sources.capacity() > 0
                && begun.marked.capacity() > 0
                && begun.given.capacity() > 0
                && begun.order.capacity() > 0
                && begun.slots.capacity() > 0,
            "its room kept"
        );
    }
}
