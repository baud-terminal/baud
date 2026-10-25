//! **Right-to-left lines in their visual order**: each row of the window
//! laid out in the order the Unicode Bidirectional Algorithm, UAX #9
//! (`[[source: uax9-bidi]]`), gives its paragraph, for every draw path to
//! read.
//!
//! **Display only.** The grid keeps the order the program wrote; a frame
//! reads it through [`Reorder`], which holds, for each row whose paragraph
//! needs it, which logical cell each visual cell shows. The cursor, the
//! selection, the find marks and the link underline keep logical columns
//! and are drawn through the map; a mouse position goes back through its
//! inverse. That is terminal-wg's model ("BiDi is display-only", cells move
//! only within their row, `[[source: terminal-wg-bidi-display-only]]`) and
//! ECMA-48's IMPLICIT state, "All bi-directional aspects of data are
//! handled by the device itself" (`[[source: ecma48]]` 7.2.1), which BDSM
//! selects (`Term::bidi_support`).
//!
//! **What the algorithm is given.** A paragraph is the rows joined by soft
//! wraps; a cell gives its character and the codepoints attached to it, in
//! logical order, so the bidi controls the core stores on a cell act as
//! UAX #9 defines them (terminal-wg's "level 2"; Jev LEVEL_2, 1.00/1.00).
//! A cell moves whole, its marks with it, which is what L3 asks of a
//! renderer that draws a mark after its base. A wide character's tail is
//! not given; it follows its lead, to the lead's right. An erased cell
//! between characters is a boundary neutral, removed by X9 and drawn with
//! the character before it (Jev's lean, AS_BN, 0.85/0.89); erased cells
//! past a row's last character are left out and laid out at the line's
//! end in the paragraph direction (`[[source:
//! terminal-wg-bidi-erased-cells]]`). A paragraph longer than [`CONTEXT`]
//! codepoints is resolved over the rows drawn and as many whole rows of
//! context before and after them as fit, UAX #9 HL5's "piece of text that
//! is extracted from a longer sequence" (Jev CONTEXT_WINDOW, 0.98/0.98).
//!
//! **A table row keeps its columns.** A paragraph holding a box-drawing
//! character ([`is_column_rule`]) is a table row: each rule is given to
//! the algorithm as a paragraph separator, so each cell between two rules
//! is a paragraph of its own at its own P2-P3 level (UAX #9 section 3's
//! table cells, HL4), each cell is ordered as a line inside its own
//! columns, every rule stays where it was written, and the row is laid
//! out left to right (Jev's lean, BOX 0.71/0.91, CELL_PARAGRAPH
//! 0.52/0.22; `docs/bidi.md`).
//!
//! **What it costs a row with no right-to-left character: one test of
//! each cell's content word.** A paragraph runs the algorithm only when a
//! row of it holds a character of class R, AL or AN, or an RLE, RLO or RLI
//! ([`baud_domain::opens_rtl`]): without one, a paragraph at level 0
//! resolves every level even and L2 reverses nothing, which is UAX #9
//! section 5.1's "first test for right-to-left characters and invoke the
//! Bidirectional Algorithm only if they are present".
//!
//! **Mirroring (L4).** A character resolved right to left that has a
//! Bidi_Mirroring_Glyph draws it, and so does one whose cell carries only
//! bidi controls (an RLM after a bracket), which draw nothing of their
//! own: the drawn cell is the mirror alone. The 126 Bidi_Mirrored
//! characters with no such glyph, and a mirrored character carrying an
//! attached codepoint other than a bidi control (a combining mark such as
//! U+0338 after `<`), are drawn as they are and counted in
//! [`Reorder::unmirrored`]: two open departures from L4, until the glyph
//! key can carry a flipped glyph and the draw paths a substitute base for
//! a marked cell. Jev, asked with these counts, leans to build both
//! (below 0.9; `docs/bidi.md`).
//!
//! **Shaping stays inside a level run** (UAX #9 3.5, "limited to characters
//! within the same level run"): [`Reorder::seam`] is set before every cell
//! whose level differs from the one to its left and before every cell at
//! an odd level, so no run, ligature or contextual form crosses a level
//! run, and a right-to-left run's cells are shaped one at a time rather
//! than in their visual order.
//!
//! **Joining (`[[source: unicode-ch9]]` 9.2.2 and 9.2.3).** Each letter of
//! a cursive script is drawn in the form rules R1-R7 give it among its
//! neighbours, and a lam joined to an alef forms the obligatory lam-alef
//! ligature, centred over the two cells the pair holds (Jev CENTRED,
//! 0.96/0.97) -- over the cells from one to the other where erased cells
//! part them (Jev LIGATED_CENTRED, 0.91/0.93), and in the lam's cell where
//! a soft wrap does, the alef's cell drawing nothing (Jev's lean
//! LIGATED_LAM_ROW, 0.84/0.80). The forms are found here, once a layout, over the logical
//! text, and every draw path reads them per visual cell
//! ([`Orders::joined`]): with BDSM set, over the paragraph across its soft
//! wraps, which CSS Text 3 asks of a word an unforced break splits ("as if
//! the word were still whole", `[[source: w3c-css-text-3]]`; Jev
//! ACROSS_WRAP, 0.96/0.90), and within each level run (UAX #9 3.5); an
//! erased cell between letters is transparent, the boundary neutral it is
//! given (Jev TRANSPARENT, 0.96/0.96). With BDSM reset each row is joined
//! in the order it is displayed, the order 9.2.2 states the rules in (Jev's
//! lean DISPLAY_ORDER_JOINING, 0.66/0.58): a program that wrote Arabic in
//! visual order is drawn joined, and presentation forms, joining type U,
//! are drawn as sent. A Mongolian or Phags-pa letter is read left to right
//! there, the way those scripts run on a horizontal line
//! (`[[source: unicode-ch13]]` 13.5, `[[source: unicode-ch14]]` 14.4), and
//! joins no letter of the other reading. A letter given no form is drawn non-joining (R7),
//! which is every Arabic letter on a row this finds nothing to join.
//!
//! **What joining costs a row with no joining letter.** With BDSM set a
//! row is compared with its last copy once a frame and tested only when it
//! changed, as it was before joining existed, by the same pass with one
//! load a cell: the table it loads from, [`baud_domain::LAYOUT_BLOCKS`], is
//! the right-to-left blocks and the joining blocks together, compiled at
//! build time, so a row of Latin, Han, box drawing or braille is let go
//! after that one pass, and only a row it holds back is read for either
//! answer. With BDSM reset no row is read for joins until the renderer has
//! shaped a letter that takes a form ([`Joins`]). After that the last
//! frame's joins are kept ([`Reorder::keep_displayed`]) and found again
//! only on the rows the caller's own compare finds changed: the
//! conversion's, against the frame it converted last, or the cell path's,
//! against the copy of the screen its last frame made (research decision
//! D4), which then compares every row of a frame that changed instead of
//! stopping at the first that differs. A frame whose screen is the last's
//! reads no row for its joins. A caller with no compare of its own lays
//! out with [`Reorder::lay_out`], which compares each row with its last
//! copy.

use baud_core::{BidiSupport, Term};
use baud_domain::{
    Bidi, BidiClass, BidiLevel, BidiMirrored, BidiParagraph, BidiProps, ClusterEpoch, ClusterSlot,
    Codepoint, Col, Cols, Content, JoiningForm, JoiningType, LamAlef, PackedCell, Row, RowExtent,
    RowRot, Rows, Stride, Viewport, Wrap, in_layout_block, join, lam_alefs, mirroring_glyph,
    opens_rtl, takes_form,
};

use crate::units::Tally;

/// The most codepoints one paragraph's text is resolved over: a row at the
/// grid's widest, every cell holding a character and as many attached
/// codepoints as a cluster keeps, so a row always fits whole.
pub const CONTEXT: usize = baud_core::MAX_COLS * (1 + ClusterSlot::CAPACITY);

/// Which side of its cell a bar cursor is drawn on: the side the character
/// under it runs from, `[[source: terminal-wg-bidi-cursor]]`'s "at the
/// very least ... the right side ... when the character underneath has
/// resolved RTL directionality".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorSide {
    #[default]
    Left,
    Right,
}

/// **The paragraph level an implicit paragraph gets: found from its text**,
/// UAX #9 P2-P3 -- level 1 when its first strong letter, isolates skipped,
/// is R or AL, else level 0 -- the standard's own default and the owner's
/// answer of 2026-10-24 ("all match C besides these 2"). A right-to-left
/// paragraph's erased cells past its text are laid out before it, so its
/// rows sit against the right edge. The letter is looked for only in the
/// text the layout reads: the paragraph's rows the window draws and the
/// context around them ([`CONTEXT`]); a paragraph resolved in more than
/// one piece keeps the first piece's level for the rest (HL1).
const DIRECTION: BidiParagraph = BidiParagraph::FirstStrong;

/// The stand-in for an erased cell between characters: U+200B ZERO WIDTH
/// SPACE, of class BN and no bracket.
const ERASED: char = '\u{200B}';

/// The stand-in for a column rule: U+2029 PARAGRAPH SEPARATOR, class B,
/// which ends a paragraph (P1).
const PARAGRAPH_SEPARATOR: char = '\u{2029}';

/// **Whether a character is a column rule**, a table row's cell separator:
/// the Box Drawing block, U+2500-U+257F, which holds every vertical a
/// program draws a table's columns with (`│` U+2502, `┃` U+2503, `║`
/// U+2551, the dashed ones) and the junctions on its rule rows, and which
/// DEC Special Graphics line drawing is stored as.
fn is_column_rule(c: char) -> bool {
    matches!(c, '\u{2500}'..='\u{257F}')
}

/// The order one row is drawn in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct RowOrder {
    /// For each visual column, the logical column it shows.
    from: Vec<Col>,
    /// For each logical column, the visual column it is drawn at.
    to: Vec<Col>,
    /// For each visual column, its level after L1.
    level: Vec<BidiLevel>,
    /// The visual columns drawn with a mirrored character, and the
    /// character: drawn as a cell of its own, any bidi control its cell
    /// carried left out.
    swaps: Vec<(Col, Content)>,
}

/// Whether any row of a window is drawn in another order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Any {
    #[default]
    No,
    Yes,
}

/// Which end of a pair's cells, as drawn, a letter's cell is at: the left
/// one draws the pair, and the right one was drawn with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairEnd {
    Left,
    Right,
}

/// Which letter of a lam-alef pair a cell holds, and where the other one
/// is: the two are drawn as the ligature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairPart {
    /// The lam, its alef on the same row, the cells from one to the other,
    /// both counted -- two for adjacent cells, more where erased cells,
    /// which are transparent, lie between them -- and which end the lam is
    /// at. The ligature is centred over them (Jev CENTRED, 0.96/0.97, and
    /// LIGATED_CENTRED for a pair erased cells part, 0.91/0.93).
    Lam(Cols, PairEnd),
    /// The alef of such a pair.
    Alef(Cols, PairEnd),
    /// The lam of a pair a soft wrap parts, with its alef's cell: the
    /// ligature is drawn in the lam's cell (Jev's lean LIGATED_LAM_ROW,
    /// 0.84/0.80).
    LamWrapped(PackedCell),
    /// The alef of such a pair, with its lam's cell: its letter is drawn
    /// in the lam's ligature, and its own cell draws no glyph.
    AlefWrapped(PackedCell),
}

/// **What joining gives one visual cell**: the form its letter is drawn
/// in, and its part in a lam-alef ligature if it has one. A cell whose
/// letter is drawn non-joining, or that holds none, has no entry: R7's form
/// is what a letter draws given none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Joined {
    pub form: JoiningForm,
    pub pair: Option<(PairPart, LamAlef)>,
}

/// One row's joined cells, by visual column.
type RowJoins = Vec<Option<Joined>>;

/// **Which logical cell each visual cell of the window shows**, for the
/// rows that are drawn in another order: what every consumer of a frame
/// maps through. Cheap to clone: a row in its own order holds nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orders {
    rows: Vec<Option<RowOrder>>,
    cols: Cols,
    any: Any,
    /// Each window row's joined cells, for the rows that hold one.
    joins: Vec<Option<RowJoins>>,
    joined_any: Any,
}

impl Default for Orders {
    fn default() -> Self {
        Orders {
            rows: Vec::new(),
            cols: Cols::of_index(0),
            any: Any::No,
            joins: Vec::new(),
            joined_any: Any::No,
        }
    }
}

impl Orders {
    /// Whether any row of the window is drawn in another order.
    pub fn any(&self) -> bool {
        self.any == Any::Yes
    }

    /// Whether any cell of the window is drawn joined.
    pub fn any_joined(&self) -> bool {
        self.joined_any == Any::Yes
    }

    /// What joining gives visual column `v` of row `r`: its form and its
    /// part in a lam-alef pair, or `None` for a cell drawn as it would be
    /// alone.
    pub fn joined(&self, r: Row, v: Col) -> Option<Joined> {
        self.joins
            .get(r.index())
            .and_then(Option::as_ref)
            .and_then(|j| j.get(v.index()).copied().flatten())
    }

    /// Row `r`'s joined cells, by visual column, if it holds any: what a
    /// renderer compares from frame to frame, since a form can change with
    /// no cell of the row changing.
    pub fn joins(&self, r: Row) -> Option<&[Option<Joined>]> {
        self.joins.get(r.index()).and_then(Option::as_deref)
    }

    /// Whether row `r` is drawn in another order.
    pub fn reordered(&self, r: Row) -> bool {
        matches!(self.rows.get(r.index()), Some(Some(_)))
    }

    fn row(&self, r: Row) -> Option<&RowOrder> {
        self.rows.get(r.index()).and_then(Option::as_ref)
    }

    /// The visual column logical column `c` of row `r` is drawn at.
    pub fn visual_col(&self, r: Row, c: Col) -> Col {
        self.row(r)
            .and_then(|o| o.to.get(c.index()).copied())
            .unwrap_or(c)
    }

    /// The logical column visual column `v` of row `r` shows: a mouse
    /// position's cell.
    pub fn logical_col(&self, r: Row, v: Col) -> Col {
        self.row(r)
            .and_then(|o| o.from.get(v.index()).copied())
            .unwrap_or(v)
    }

    /// The side of its cell a bar cursor on logical column `c` of row `r`
    /// is drawn on: right where the character resolves right to left, or,
    /// on an erased cell past the text, where the paragraph does.
    pub fn side(&self, r: Row, c: Col) -> CursorSide {
        let odd = self.row(r).is_some_and(|o| {
            o.to.get(c.index())
                .and_then(|v| o.level.get(v.index()))
                .is_some_and(|l| l.direction() == baud_domain::BidiDirection::Rtl)
        });
        match odd {
            true => CursorSide::Right,
            false => CursorSide::Left,
        }
    }

    /// Whether shaping must not join visual column `v` of row `r` to the
    /// cell on its left: a level change, or a cell at an odd level.
    pub fn seam(&self, r: Row, v: Col) -> bool {
        let Some(o) = self.row(r) else {
            return false;
        };
        let at = v.index();
        match (
            at.checked_sub(1).and_then(|p| o.level.get(p)),
            o.level.get(at),
        ) {
            (Some(left), Some(here)) => {
                here.direction() == baud_domain::BidiDirection::Rtl || left != here
            }
            _ => false,
        }
    }

    /// Row `r`'s cells, `logical`, in the order they are drawn, into `out`:
    /// each visual cell the logical cell it shows, a mirrored character
    /// where L4 gives one. A row drawn in its own order is copied.
    pub fn permute(&self, r: Row, logical: &[PackedCell], out: &mut [PackedCell]) {
        match self.row(r) {
            Some(o) => {
                for (d, l) in out.iter_mut().zip(&o.from) {
                    *d = logical.get(l.index()).copied().unwrap_or(PackedCell::EMPTY);
                }
                for &(v, content) in &o.swaps {
                    if let Some(d) = out.get_mut(v.index()) {
                        d.content = content;
                        d.flags = d.flags.without_cluster();
                    }
                }
            }
            None => {
                let n = out.len().min(logical.len());
                out[..n].copy_from_slice(&logical[..n]);
            }
        }
    }

    /// The visual runs logical columns `from..=to` of row `r` are drawn
    /// in, each its first visual column and its width: one run on a row in
    /// its own order, as many as the map splits it into on another.
    pub fn runs(&self, r: Row, from: Col, to: Col) -> Vec<(Col, Cols)> {
        let Some(o) = self.row(r) else {
            return vec![(
                from,
                Cols::of_index(to.index().saturating_sub(from.index()) + 1),
            )];
        };
        let mut out: Vec<(Col, Cols)> = Vec::new();
        let mut start: Option<usize> = None;
        for (v, l) in o.from.iter().enumerate() {
            let inside = l.index() >= from.index() && l.index() <= to.index();
            match (inside, start) {
                (true, None) => start = Some(v),
                (false, Some(s)) => {
                    out.push((Col::at_index(s), Cols::of_index(v - s)));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            out.push((Col::at_index(s), Cols::of_index(o.from.len() - s)));
        }
        out
    }

    /// **The window as the cell path reads it**: the core's own view when
    /// no row is reordered, else every row copied into `cells` at the
    /// window's width, each in its drawn order, with its extent in `used`.
    pub fn window<'a>(
        &self,
        term: &'a Term<'_>,
        back: Rows,
        cells: &'a mut Vec<PackedCell>,
        used: &'a mut Vec<RowExtent>,
    ) -> Viewport<'a> {
        let vp = term.view(back);
        if self.any == Any::No {
            return vp;
        }
        let (nrows, ncols) = (vp.rows().index(), vp.cols().index());
        cells.clear();
        cells.resize(nrows * ncols, PackedCell::EMPTY);
        used.clear();
        for y in 0..nrows {
            let r = Row::from_index(y);
            self.permute(r, vp.row(r), &mut cells[y * ncols..(y + 1) * ncols]);
            let mut e = RowExtent::of(ncols).with_rendition(vp.rendition(r), vp.cols());
            if vp.wrapped(r) == Wrap::Continues {
                e = e.wrapping(ncols);
            }
            used.push(e);
        }
        Viewport::new(
            cells,
            used,
            Stride::of_cells(ncols),
            vp.cols(),
            vp.rows(),
            vp.rows(),
            RowRot::NONE,
        )
    }
}

/// Whether a codepoint of a paragraph's text is the first of its cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lead {
    First,
    Attached,
}

/// Where a codepoint of a row's text came from: its cell's column.
#[derive(Clone, Copy, Debug)]
struct Owner {
    col: Col,
    lead: Lead,
    /// The paragraph row it is in, counted from the first gathered.
    row: Row,
}

/// One row of a paragraph: its cells, and its row in the window if it is
/// drawn.
struct ParaRow<'t> {
    cells: &'t [PackedCell],
    window: Option<Row>,
}

/// How many codepoints a row gives the algorithm.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
struct Chars(usize);

/// The window's rows in their visual order, for one frame: the maps
/// ([`Orders`]) and the algorithm's working state.
pub struct Reorder {
    orders: Orders,
    seen: Seen,
    unmirrored: Tally,
    work: Work,
    /// How the joins the maps hold were found.
    last: Last,
    bidi: Box<Bidi<CONTEXT>>,
    text: Vec<char>,
    owner: Vec<Owner>,
    order: Vec<u16>,
    /// For each codepoint of the text, whether it is a column rule.
    rule: Vec<Rule>,
    /// One line piece's order, and every line's levels by text position.
    piece: Vec<u16>,
    levels: Vec<BidiLevel>,
}

/// Whether a codepoint of a paragraph's text is a column rule: a table
/// row's cell separator ([`is_column_rule`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    No,
    Yes,
}

impl Default for Reorder {
    fn default() -> Self {
        Reorder::new()
    }
}

/// **What a layout read to find its answers**: the rows it compared with
/// their last copy, and the rows it tested cell by cell for a character
/// that reorders or joins. What a frame with no row changed should cost is
/// none of the second, and with BDSM reset none of the first either.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Work {
    pub compared: Tally,
    pub tested: Tally,
}

/// How the joins the maps hold were found: by [`Reorder::lay_out`], which
/// finds every row's, or kept from the frame before by
/// [`Reorder::keep_displayed`] for a window of this size and cluster epoch,
/// the caller rejoining the rows its compare saw change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Last {
    LaidOut,
    Kept(Cols, ClusterEpoch, Rows, Changes),
}

/// **Whose compare says which rows changed** after
/// [`Reorder::keep_displayed`]: the conversion's, against the frame it
/// converted last, or the cell path's, against the copy of the screen its
/// last frame made (research decision D4). Each says what changed since
/// its own last frame, so joins kept under one are found again whole
/// under the other.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Changes {
    Converted,
    Copied,
}

/// Whether [`Reorder::keep_displayed`] kept the last frame's joins, so
/// that only the rows that changed need [`Reorder::rejoin`], or kept none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rejoin {
    Changed,
    Every,
}

/// Whether a row holds a character that can make its paragraph reorder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opens {
    Unknown,
    No,
    Yes,
}

/// **Each window row's answers, kept while the row is unchanged**: the
/// rows as last tested and what the test said, so a frame tests only the
/// rows that changed since -- a row is compared with its copy, a whole row
/// at a time, and read cell by cell only when it differs. A cluster's id
/// can name another cluster after a sweep, so a moved epoch tests every row
/// again.
#[derive(Default)]
struct Seen {
    cells: Vec<PackedCell>,
    opens: Vec<Opens>,
    /// Whether each row holds a letter that takes a joining form, found by
    /// the same test as `opens`.
    joins: Vec<Opens>,
    width: Option<(Cols, ClusterEpoch)>,
}

/// What a row holds that its layout reads: whether it can make its
/// paragraph reorder, and whether it holds a letter that takes a joining
/// form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Holds {
    opens: Opens,
    joins: Opens,
}

impl Holds {
    const NOTHING: Holds = Holds {
        opens: Opens::No,
        joins: Opens::No,
    };
}

impl Seen {
    /// What row `y`, `cells`, holds: the kept answers when the row is the
    /// one last tested, else the test, kept.
    fn holds(&mut self, term: &Term<'_>, row: Row, cells: &[PackedCell], work: &mut Work) -> Holds {
        let (y, n) = (row.index(), cells.len());
        if self.opens.len() <= y {
            self.opens.resize(y + 1, Opens::Unknown);
            self.joins.resize(y + 1, Opens::Unknown);
        }
        if self.cells.len() < (y + 1) * n {
            self.cells.resize((y + 1) * n, PackedCell::EMPTY);
        }
        let kept = &mut self.cells[y * n..(y + 1) * n];
        work.compared.bump();
        if self.opens[y] != Opens::Unknown && PackedCell::same_cells(kept, cells) {
            return Holds {
                opens: self.opens[y],
                joins: self.joins[y],
            };
        }
        kept.copy_from_slice(cells);
        work.tested.bump();
        let h = row_holds(term, cells);
        (self.opens[y], self.joins[y]) = (h.opens, h.joins);
        h
    }
}

/// **Whether a layout with BDSM reset reads rows for their joins**: not
/// until the renderer has shaped a letter that takes a joining form
/// (Joining_Type D, R or L), since a screen none of whose letters has been
/// drawn holds none to join -- what a session that never shows one pays
/// for joining with the mode reset is nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Joins {
    Unseen,
    Seen,
}

impl Reorder {
    pub fn new() -> Reorder {
        Reorder {
            orders: Orders::default(),
            seen: Seen::default(),
            unmirrored: Tally::default(),
            work: Work::default(),
            last: Last::LaidOut,
            bidi: Box::new(Bidi::new()),
            text: Vec::new(),
            owner: Vec::new(),
            order: Vec::new(),
            rule: Vec::new(),
            piece: Vec::new(),
            levels: Vec::new(),
        }
    }

    /// The maps the last layout made.
    pub fn orders(&self) -> &Orders {
        &self.orders
    }

    /// Bidi_Mirrored characters drawn unmirrored in the last layout, L4's
    /// departure: those with no Bidi_Mirroring_Glyph, and those carrying an
    /// attached codepoint other than a bidi control (a combining mark such
    /// as U+0338).
    pub fn unmirrored(&self) -> Tally {
        self.unmirrored
    }

    /// What the last layout read to find its answers.
    pub fn work(&self) -> Work {
        self.work
    }

    /// **Lays out the window `back` lines into the history**, as
    /// [`Term::view`] draws it, each row compared with its last copy once
    /// to know whether it changed. Nothing is reordered while BDSM is
    /// reset, and each row that holds a joining letter is joined as it is
    /// displayed. A caller with a compare of its own -- the conversion's,
    /// or the cell path's copy of the screen -- lays out with
    /// [`Reorder::keep_displayed`] then, and nothing here compares a row.
    pub fn lay_out(&mut self, term: &Term<'_>, back: Rows) {
        self.lay_out_as(term, back, Joins::Seen);
    }

    /// [`Reorder::lay_out`], where with BDSM reset and `joins`
    /// [`Joins::Unseen`] no row is read at all.
    pub fn lay_out_as(&mut self, term: &Term<'_>, back: Rows, joins: Joins) {
        let back = term.looking_back(back);
        let vp = term.view(back);
        let nrows = vp.rows().index();
        self.orders.cols = vp.cols();
        self.orders.any = Any::No;
        self.unmirrored = Tally::default();
        self.work = Work::default();
        self.last = Last::LaidOut;
        self.orders.rows.clear();
        self.orders.rows.resize_with(nrows, || None);
        self.forget_joins(vp.rows());
        let explicit = term.bidi_support() == BidiSupport::Explicit;
        if explicit && joins == Joins::Unseen {
            return;
        }
        let width = Some((vp.cols(), term.cluster_epoch()));
        if self.seen.width != width {
            self.seen.width = width;
            self.seen.opens.clear();
            self.seen.joins.clear();
        }
        if explicit {
            for y in 0..nrows {
                let r = Row::from_index(y);
                if self.seen.holds(term, r, vp.row(r), &mut self.work).joins == Opens::Yes {
                    self.join_displayed(term, r, vp.row(r));
                }
            }
            return;
        }
        let mut y = 0;
        while y < nrows {
            // The window's rows of one paragraph: up to a row that ends.
            let first = y;
            while y + 1 < nrows && vp.wrapped(Row::from_index(y)) == Wrap::Continues {
                y += 1;
            }
            self.paragraph(
                term,
                back,
                &vp,
                (Row::from_index(first), Row::from_index(y)),
            );
            y += 1;
        }
    }

    /// **Lays out the window with BDSM reset for a caller that knows which
    /// rows changed**: nothing is reordered, and each row's joins, which
    /// with the mode reset are its own (one row, one run), are kept from the
    /// frame before -- [`Rejoin::Changed`] -- for the caller to find again
    /// by [`Reorder::rejoin`] on each row that changed, then
    /// [`Reorder::rejoined`]. [`Rejoin::Every`] when there is no frame
    /// before to keep: the last was laid out otherwise, or at another width,
    /// height or cluster epoch, or kept for the other compare (`changes`).
    /// A row that did not change is neither compared nor tested here.
    pub fn keep_displayed(&mut self, term: &Term<'_>, back: Rows, changes: Changes) -> Rejoin {
        let vp = term.view(term.looking_back(back));
        let nrows = vp.rows().index();
        self.orders.cols = vp.cols();
        self.orders.any = Any::No;
        self.unmirrored = Tally::default();
        self.work = Work::default();
        self.orders.rows.clear();
        self.orders.rows.resize_with(nrows, || None);
        let now = Last::Kept(vp.cols(), term.cluster_epoch(), vp.rows(), changes);
        let rejoin = match self.last == now {
            true => Rejoin::Changed,
            false => {
                self.forget_joins(vp.rows());
                Rejoin::Every
            }
        };
        self.last = now;
        rejoin
    }

    /// Row `r` of the window `vp` joined again, with BDSM reset, after
    /// [`Reorder::keep_displayed`]: one test of the row, and the row joined
    /// in display order if it holds a joining letter.
    pub fn rejoin(&mut self, term: &Term<'_>, vp: &Viewport<'_>, r: Row) {
        let Some(slot) = self.orders.joins.get_mut(r.index()) else {
            return;
        };
        *slot = None;
        self.work.tested.bump();
        if row_joins(term, vp.row(r)) {
            self.join_displayed(term, r, vp.row(r));
        }
    }

    /// No row of a window `rows` high holds a joined cell: the maps
    /// emptied only where the last layout left one, or made at a new
    /// height, so a window that never held one costs one test.
    fn forget_joins(&mut self, rows: Rows) {
        let nrows = rows.index();
        if self.orders.joined_any == Any::Yes || self.orders.joins.len() != nrows {
            self.orders.joins.clear();
            self.orders.joins.resize_with(nrows, || None);
            self.orders.joined_any = Any::No;
        }
    }

    /// The rejoining after [`Reorder::keep_displayed`] done: whether any
    /// row holds a joined cell, found again.
    pub fn rejoined(&mut self) {
        self.orders.joined_any = match self.orders.joins.iter().any(Option::is_some) {
            true => Any::Yes,
            false => Any::No,
        };
    }

    /// One paragraph whose drawn rows are `first..=last`: resolved, and each
    /// of those rows ordered, if any of its rows can reorder.
    fn paragraph(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        vp: &Viewport<'_>,
        (first, last): (Row, Row),
    ) {
        let nrows = vp.rows().index();
        let budget = CONTEXT / vp.cols().index().max(1);
        let opens = |cells: &[PackedCell]| row_opens(term, cells);
        // Every drawn row is asked, so each keeps its answer for the next
        // frame.
        let mut drawn_opens = false;
        let mut drawn_joins = false;
        for y in first.index()..=last.index() {
            let r = Row::from_index(y);
            let h = self.seen.holds(term, r, vp.row(r), &mut self.work);
            drawn_opens |= h.opens == Opens::Yes;
            drawn_joins |= h.joins == Opens::Yes;
        }
        // The rows above the window the paragraph starts in, and below it
        // when the window looks back, nearest first: context, read only
        // when the paragraph reaches the window's edge -- so a paragraph
        // inside the window that holds no right-to-left character costs
        // its rows' test and nothing else.
        let mut above: Vec<&[PackedCell]> = Vec::new();
        if first.index() == 0 {
            let mut k = 1;
            while above.len() < budget {
                let want = Rows::of_index(back.index() + k);
                if term.looking_back(want) != want {
                    break;
                }
                let v = term.view(want);
                if v.wrapped(Row::ZERO) != Wrap::Continues {
                    break;
                }
                above.push(v.row(Row::ZERO));
                k += 1;
            }
        }
        let mut below: Vec<&[PackedCell]> = Vec::new();
        if last.index() + 1 == nrows && vp.wrapped(last) == Wrap::Continues {
            let mut k = 1;
            while k <= back.index() && below.len() < budget {
                let v = term.view(Rows::of_index(back.index() - k));
                let at = Row::from_index(nrows - 1);
                below.push(v.row(at));
                if v.wrapped(at) != Wrap::Continues {
                    break;
                }
                k += 1;
            }
        }
        let any_opens =
            drawn_opens || above.iter().any(|c| opens(c)) || below.iter().any(|c| opens(c));
        // A paragraph that nothing reorders but whose drawn rows hold a
        // joining letter -- Mongolian, whose class is L -- is joined in its
        // stored order, every character at the paragraph's level, and drawn
        // where it is.
        let resolves = match (any_opens, drawn_joins) {
            (true, _) => Resolves::Levels,
            (false, true) => Resolves::JoinOnly,
            (false, false) => return,
        };
        let drawn: Vec<ParaRow<'_>> = (first.index()..=last.index())
            .map(|y| ParaRow {
                cells: vp.row(Row::from_index(y)),
                window: Some(Row::from_index(y)),
            })
            .collect();
        // The drawn rows in chunks that fit, each with the context that
        // fits around it: the rows above before the first chunk, the rows
        // below after the last.
        let cap = Chars(CONTEXT);
        let mut dir = DIRECTION;
        let mut start = 0;
        while start < drawn.len() {
            let mut used = Chars::default();
            let mut end = start;
            while end < drawn.len() {
                let n = chars_of(term, drawn[end].cells);
                if end > start && Chars(used.0 + n.0) > cap {
                    break;
                }
                used = Chars(used.0 + n.0);
                end += 1;
            }
            let mut rows: Vec<ParaRow<'_>> = Vec::new();
            if start == 0 {
                let mut take = 0;
                for c in &above {
                    let n = chars_of(term, c);
                    if Chars(used.0 + n.0) > cap {
                        break;
                    }
                    used = Chars(used.0 + n.0);
                    take += 1;
                }
                rows.extend(above[..take].iter().rev().map(|c| ParaRow {
                    cells: c,
                    window: None,
                }));
            }
            rows.extend(drawn[start..end].iter().map(|r| ParaRow {
                cells: r.cells,
                window: r.window,
            }));
            if end == drawn.len() {
                for c in &below {
                    let n = chars_of(term, c);
                    if Chars(used.0 + n.0) > cap {
                        break;
                    }
                    used = Chars(used.0 + n.0);
                    rows.push(ParaRow {
                        cells: c,
                        window: None,
                    });
                }
            }
            if let Some(level) = self.resolve(term, &rows, dir, resolves) {
                dir = match level.direction() {
                    baud_domain::BidiDirection::Rtl => BidiParagraph::Rtl,
                    baud_domain::BidiDirection::Ltr => BidiParagraph::Ltr,
                };
            }
            start = end;
        }
    }

    /// Resolves `rows` as one paragraph at `dir` and orders each drawn one,
    /// or with [`Resolves::JoinOnly`] only joins it; answers the
    /// paragraph's level.
    fn resolve(
        &mut self,
        term: &Term<'_>,
        rows: &[ParaRow<'_>],
        dir: BidiParagraph,
        resolves: Resolves,
    ) -> Option<BidiLevel> {
        self.text.clear();
        self.owner.clear();
        self.rule.clear();
        let mut ranges: Vec<(core::ops::Range<usize>, Col)> = Vec::with_capacity(rows.len());
        for (p, row) in rows.iter().enumerate() {
            let begin = self.text.len();
            let end = row_end(row.cells);
            for (c, &cell) in row.cells[..end.index()].iter().enumerate() {
                let at = |lead| Owner {
                    col: Col::at_index(c),
                    lead,
                    row: Row::from_index(p),
                };
                if cell.content.is_wide_tail() {
                    continue;
                }
                if cell.content.is_empty() {
                    self.text.push(ERASED);
                    self.owner.push(at(Lead::First));
                    self.rule.push(Rule::No);
                    continue;
                }
                let base = term
                    .base_of(cell)
                    .drawn()
                    .as_codepoint()
                    .and_then(|c| char::from_u32(c.value()))
                    .unwrap_or('\u{FFFD}');
                // A column rule ends its cell's paragraph: given to the
                // algorithm as a paragraph separator (P1), so each cell is
                // resolved on its own (HL4), and drawn where it is.
                let rule = match !cell.flags.is_marked() && is_column_rule(base) {
                    true => Rule::Yes,
                    false => Rule::No,
                };
                self.text.push(match rule {
                    Rule::Yes => PARAGRAPH_SEPARATOR,
                    Rule::No => base,
                });
                self.owner.push(at(Lead::First));
                self.rule.push(rule);
                if cell.flags.is_marked() {
                    for m in term.cluster_marks(cell) {
                        if let Some(m) = char::from_u32(m) {
                            self.text.push(m);
                            self.owner.push(at(Lead::Attached));
                            self.rule.push(Rule::No);
                        }
                    }
                }
            }
            ranges.push((begin..self.text.len(), end));
        }
        let cell_of = |o: Owner| {
            let row = rows.get(o.row.index());
            let cell = row
                .and_then(|r| r.cells.get(o.col.index()))
                .copied()
                .unwrap_or(PackedCell::EMPTY);
            match row.and_then(|r| r.window) {
                Some(_) => (cell, Drawn::Yes),
                None => (cell, Drawn::No),
            }
        };
        if resolves == Resolves::JoinOnly {
            let joined = self.join_text(None, None);
            for (p, row) in rows.iter().enumerate() {
                if let Some(y) = row.window {
                    self.store_joins(y, &ranges[p].0, &joined, |c| c, cell_of);
                }
            }
            return None;
        }
        // **A table row**: a paragraph holding a column rule keeps its
        // columns. Each cell is a paragraph of its own, at the level its
        // own first strong letter gives it (UAX #9 section 3: "the text in
        // two different cells of a table will be in different
        // paragraphs"), each is ordered inside its own columns, and the
        // row runs left to right, every rule where it was written.
        let table = self.rule.contains(&Rule::Yes);
        let dir = match table {
            true => BidiParagraph::FirstStrong,
            false => dir,
        };
        let Ok(level) = self.bidi.resolve(&self.text, dir) else {
            return None;
        };
        let level = match table {
            true => BidiLevel::LTR,
            false => level,
        };
        let levels: Vec<BidiLevel> = self.bidi.levels().to_vec();
        let joined = self.join_text(Some(&levels), None);
        let rtl = level.direction() == baud_domain::BidiDirection::Rtl;
        let cols = self.orders.cols.index();
        for (p, row) in rows.iter().enumerate() {
            let Some(y) = row.window else { continue };
            let (line, end) = (ranges[p].0.clone(), ranges[p].1.index());
            let begin = line.start;
            if self.order_line(line).is_none() {
                continue;
            }
            let levels = core::mem::take(&mut self.levels);
            let mut out = RowOrder {
                from: Vec::with_capacity(cols),
                to: vec![Col::ZERO; cols],
                level: Vec::with_capacity(cols),
                swaps: Vec::new(),
            };
            // The erased cells past the text go at the line's end in the
            // paragraph direction: before the text on a right-to-left line,
            // nearest the text last.
            if rtl {
                for c in (end..cols).rev() {
                    out.from.push(Col::at_index(c));
                    out.level.push(level);
                }
            }
            let mut reorders = rtl;
            for &t in &self.order {
                let t = usize::from(t);
                let own = self.owner[t];
                if own.lead != Lead::First {
                    continue;
                }
                let l = levels[t - begin];
                let odd = l.direction() == baud_domain::BidiDirection::Rtl;
                reorders |= odd;
                out.from.push(own.col);
                out.level.push(l);
                let cell = row.cells[own.col.index()];
                if odd {
                    match mirror(term, cell) {
                        Mirror::Glyph(m) => out.swaps.push((Col::at_index(out.from.len() - 1), m)),
                        Mirror::Missing => self.unmirrored.bump(),
                        Mirror::Not => {}
                    }
                }
                if row
                    .cells
                    .get(own.col.index() + 1)
                    .is_some_and(|n| n.content.is_wide_tail())
                {
                    out.from.push(Col::at_index(own.col.index() + 1));
                    out.level.push(l);
                }
            }
            if !rtl {
                for c in end..cols {
                    out.from.push(Col::at_index(c));
                    out.level.push(level);
                }
            }
            if !reorders || out.from.len() != cols {
                self.store_joins(y, &ranges[p].0, &joined, |c| c, cell_of);
                continue;
            }
            for (v, l) in out.from.iter().enumerate() {
                if let Some(slot) = out.to.get_mut(l.index()) {
                    *slot = Col::at_index(v);
                }
            }
            let to = out.to.clone();
            self.store_joins(
                y,
                &ranges[p].0,
                &joined,
                |c| to.get(c.index()).copied().unwrap_or(c),
                cell_of,
            );
            self.orders.rows[y.index()] = Some(out);
            self.orders.any = Any::Yes;
        }
        // A table row's level is its cells' each; nothing carries on.
        (!table).then_some(level)
    }

    /// **Orders one line of the text last resolved** into `order`, with
    /// each of its positions' levels in `levels`: the whole line at once
    /// (L1-L2), or, on a table row, each run between two column rules on
    /// its own, as the line of its own paragraph, the rules kept in place
    /// at level 0. A run's trailing whitespace takes its cell's paragraph
    /// level (L1), so a right-to-left cell's text sits against the cell's
    /// right side.
    fn order_line(&mut self, line: core::ops::Range<usize>) -> Option<()> {
        let begin = line.start;
        self.order.clear();
        self.levels.clear();
        self.levels.resize(line.len(), BidiLevel::LTR);
        let mut from = begin;
        while from < line.end {
            let to = (from..line.end)
                .find(|&t| self.rule[t] == Rule::Yes)
                .unwrap_or(line.end);
            if to > from {
                self.piece.clear();
                self.piece.resize(to - from, 0);
                let levels = self.bidi.reorder(from..to, &mut self.piece).ok()?;
                self.levels[from - begin..to - begin].copy_from_slice(levels);
                self.order.extend_from_slice(&self.piece);
            }
            if to < line.end {
                self.order.push(u16::try_from(to).ok()?);
            }
            from = to + 1;
        }
        Some(())
    }

    /// **The joining forms of the paragraph text last gathered**
    /// (`self.text`, `self.owner`): rules R1-R7 over each level run in
    /// logical order (UAX #9 3.5's "limited to characters within the same
    /// level run", read from `levels`, the levels after I2, or one run
    /// with none), a character X9 removes kept in the run it sits in, and
    /// the lam-alef ligatures L1-L3 among them. Each text position's form,
    /// and each pair's lam and alef: every pair the rules form, whatever
    /// erased cells or soft wrap lie between its two letters.
    /// With `reading`, only the letters read that way join: the others
    /// are given type U, so no letter joins one of the other reading.
    fn join_text(&self, levels: Option<&[BidiLevel]>, reading: Option<Reading>) -> JoinedText {
        let n = self.text.len();
        let cps: Vec<u32> = self.text.iter().map(|&c| u32::from(c)).collect();
        let types: Vec<JoiningType> = self
            .text
            .iter()
            .map(|&c| match (JoiningType::of(u32::from(c)), reading) {
                (
                    JoiningType::DualJoining | JoiningType::RightJoining | JoiningType::LeftJoining,
                    Some(r),
                ) if Reading::of(c) != r => JoiningType::NonJoining,
                (t, _) => t,
            })
            .collect();
        let mut forms: Vec<Option<JoiningForm>> = vec![None; n];
        let mut pairs: Vec<Pair> = Vec::new();
        let removed = |i: usize| {
            matches!(
                BidiClass::of(self.text[i]),
                BidiClass::BN
                    | BidiClass::LRE
                    | BidiClass::RLE
                    | BidiClass::LRO
                    | BidiClass::RLO
                    | BidiClass::PDF
            )
        };
        let mut start = 0;
        while start < n {
            let mut end = start + 1;
            let mut level = levels.and_then(|l| (!removed(start)).then(|| l[start]));
            while end < n {
                if let Some(l) = levels.filter(|_| !removed(end)).map(|l| l[end]) {
                    match level {
                        Some(run) if run != l => break,
                        _ => level = Some(l),
                    }
                }
                end += 1;
            }
            join(&types[start..end], &mut forms[start..end]);
            lam_alefs(&cps[start..end], &forms[start..end], |i, j, kind| {
                let (lam, alef) = (start + i, start + j);
                // A pair is two cells' letters, not a mark attached to one.
                if self.owner[lam].lead == Lead::First && self.owner[alef].lead == Lead::First {
                    pairs.push(Pair {
                        lam: At(lam),
                        alef: At(alef),
                        kind,
                    });
                }
            });
            start = end;
        }
        let forms = forms
            .iter()
            .map(|f| {
                f.filter(|&f| f != JoiningForm::Isolated)
                    .map(|form| Joined { form, pair: None })
            })
            .collect();
        JoinedText { forms, pairs }
    }

    /// Stores the joins of window row `y`, whose text is `line` of
    /// `self.text`, each cell's at the visual column `to` maps its logical
    /// column to, and each pair's part: a pair on this row with the cells
    /// it spans as drawn, and a pair a soft wrap parts with the other
    /// letter's cell, which `cell_of` reads with whether its row is drawn.
    /// An alef whose lam is on a row of context, which is not drawn, keeps
    /// its final form, so its letter is not lost with the ligature. A row
    /// with none stores nothing.
    fn store_joins(
        &mut self,
        y: Row,
        line: &core::ops::Range<usize>,
        joined: &JoinedText,
        to: impl Fn(Col) -> Col,
        cell_of: impl Fn(Owner) -> (PackedCell, Drawn),
    ) {
        let cols = self.orders.cols.index();
        let mut row: Option<RowJoins> = None;
        for t in line.clone() {
            let (Some(j), Some(own)) = (joined.forms.get(t).copied().flatten(), self.owner.get(t))
            else {
                continue;
            };
            if own.lead != Lead::First {
                continue;
            }
            let v = to(own.col).index();
            let slots = row.get_or_insert_with(|| vec![None; cols]);
            if let Some(slot) = slots.get_mut(v) {
                *slot = Some(j);
            }
        }
        if let Some(slots) = row.as_mut() {
            for p in &joined.pairs {
                let (lam, alef) = (self.owner[p.lam.0], self.owner[p.alef.0]);
                let (lam_here, alef_here) = (line.contains(&p.lam.0), line.contains(&p.alef.0));
                let mut set = |at: Col, part: PairPart| {
                    if let Some(Some(j)) = slots.get_mut(to(at).index()) {
                        j.pair = Some((part, p.kind));
                    }
                };
                match (lam_here, alef_here) {
                    (true, true) => {
                        let (l, a) = (to(lam.col).index(), to(alef.col).index());
                        let span = Cols::of_index(l.abs_diff(a) + 1);
                        let (lam_end, alef_end) = match l < a {
                            true => (PairEnd::Left, PairEnd::Right),
                            false => (PairEnd::Right, PairEnd::Left),
                        };
                        set(lam.col, PairPart::Lam(span, lam_end));
                        set(alef.col, PairPart::Alef(span, alef_end));
                    }
                    (true, false) => set(lam.col, PairPart::LamWrapped(cell_of(alef).0)),
                    (false, true) => match cell_of(lam) {
                        (cell, Drawn::Yes) => set(alef.col, PairPart::AlefWrapped(cell)),
                        (_, Drawn::No) => {}
                    },
                    (false, false) => {}
                }
            }
        }
        if let Some(r) = row {
            if let Some(slot) = self.orders.joins.get_mut(y.index()) {
                *slot = Some(r);
                self.orders.joined_any = Any::Yes;
            }
        }
    }

    /// **Row `r` joined in the order it is displayed**, with BDSM reset:
    /// its cells read right to left are the logical order 9.2.2's rules,
    /// stated in display order, are restated in, so a program that wrote
    /// Arabic already reversed sees it joined. One row, one run. A letter
    /// of Mongolian or Phags-pa is read left to right instead, the way
    /// those scripts run in a horizontal line (`[[source: unicode-ch13]]`
    /// 13.5, `[[source: unicode-ch14]]` 14.4): the row is read a second
    /// time in its stored order for them, and no letter joins one of the
    /// other reading ([`Reading`]).
    fn join_displayed(&mut self, term: &Term<'_>, r: Row, cells: &[PackedCell]) {
        let end = row_end(cells).index();
        self.displayed_text(term, cells, (0..end).rev(), Reading::RightToLeft);
        let ltr = self
            .text
            .iter()
            .any(|&c| Reading::of(c) == Reading::LeftToRight);
        let joined = self.join_text(None, Some(Reading::RightToLeft));
        let line = 0..self.text.len();
        let cell_of = |o: Owner| {
            let cell = cells.get(o.col.index()).copied();
            (cell.unwrap_or(PackedCell::EMPTY), Drawn::Yes)
        };
        self.store_joins(r, &line, &joined, |c| c, cell_of);
        if !ltr {
            return;
        }
        let right_to_left = self.orders.joins.get_mut(r.index()).and_then(Option::take);
        self.displayed_text(term, cells, 0..end, Reading::LeftToRight);
        let joined = self.join_text(None, Some(Reading::LeftToRight));
        let line = 0..self.text.len();
        self.store_joins(r, &line, &joined, |c| c, cell_of);
        // Each cell's letter is one reading's, so the two never both name
        // a form for one cell.
        let Some(slot) = self.orders.joins.get_mut(r.index()) else {
            return;
        };
        match (slot.as_mut(), right_to_left) {
            (Some(now), Some(before)) => {
                for (n, b) in now.iter_mut().zip(before) {
                    if n.is_none() {
                        *n = b;
                    }
                }
            }
            (None, before) => *slot = before,
            (Some(_), None) => {}
        }
    }

    /// The text of `cells` in the order `cols` reads them, into
    /// `self.text` and `self.owner`: an erased cell a boundary neutral, a
    /// wide tail left out, and a cell's attached codepoints after its base
    /// read left to right, before it read right to left -- what follows
    /// the base in the cell is on its right.
    fn displayed_text(
        &mut self,
        term: &Term<'_>,
        cells: &[PackedCell],
        cols: impl Iterator<Item = usize>,
        reading: Reading,
    ) {
        self.text.clear();
        self.owner.clear();
        for c in cols {
            let cell = cells[c];
            let at = |lead| Owner {
                col: Col::at_index(c),
                lead,
                row: Row::ZERO,
            };
            if cell.content.is_wide_tail() {
                continue;
            }
            if cell.content.is_empty() {
                self.text.push(ERASED);
                self.owner.push(at(Lead::First));
                continue;
            }
            let base = term
                .base_of(cell)
                .drawn()
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value()))
                .unwrap_or('\u{FFFD}');
            let marks: Vec<char> = match cell.flags.is_marked() {
                true => term
                    .cluster_marks(cell)
                    .filter_map(char::from_u32)
                    .collect(),
                false => Vec::new(),
            };
            if reading == Reading::RightToLeft {
                for &m in marks.iter().rev() {
                    self.text.push(m);
                    self.owner.push(at(Lead::Attached));
                }
            }
            self.text.push(base);
            self.owner.push(at(Lead::First));
            if reading == Reading::LeftToRight {
                for &m in &marks {
                    self.text.push(m);
                    self.owner.push(at(Lead::Attached));
                }
            }
        }
    }
}

/// **Which way a row drawn as it is stored is read for its joins**, with
/// BDSM reset: right to left for every joining script but two, whose
/// letters a program writes left to right on a horizontal line -- the
/// Mongolian and Phags-pa blocks, the scripts of ArabicShaping.txt's two
/// sections that are not right to left (`[[source:
/// unicode-arabicshaping]]`; `[[source: unicode-ch13]]` 13.5: "When
/// rendering Mongolian text in a system that does not support vertical
/// layout, the text should be laid out in horizontal lines running left
/// to right"; `[[source: unicode-ch14]]` 14.4, Phags-pa "may be laid out
/// horizontally from left to right").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reading {
    RightToLeft,
    LeftToRight,
}

impl Reading {
    fn of(c: char) -> Reading {
        match c {
            '\u{1800}'..='\u{18AF}' | '\u{A840}'..='\u{A87F}' => Reading::LeftToRight,
            _ => Reading::RightToLeft,
        }
    }
}

/// A position in the paragraph text last gathered (`Reorder::text`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct At(usize);

/// A lam-alef pair the rules formed: the text positions of its lam and its
/// alef, and the ligature's form.
#[derive(Clone, Copy, Debug)]
struct Pair {
    lam: At,
    alef: At,
    kind: LamAlef,
}

/// Whether a paragraph row is drawn in the window, or read only as context.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drawn {
    Yes,
    No,
}

/// A paragraph's text joined: each position's form, and the pairs.
struct JoinedText {
    forms: Vec<Option<Joined>>,
    pairs: Vec<Pair>,
}

/// Whether a paragraph is resolved for its order and joined, or, holding no
/// character that reorders, only joined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Resolves {
    Levels,
    JoinOnly,
}

/// What L4 draws for a cell resolved right to left.
enum Mirror {
    /// Its character's Bidi_Mirroring_Glyph.
    Glyph(Content),
    /// Nothing else: it is Bidi_Mirrored with no such glyph, or carries an
    /// attached codepoint other than a bidi control (a combining mark such
    /// as U+0338), and is drawn as it is.
    Missing,
    /// Itself: it is not Bidi_Mirrored.
    Not,
}

fn mirror(term: &Term<'_>, cell: PackedCell) -> Mirror {
    let Some(c) = term
        .base_of(cell)
        .drawn()
        .as_codepoint()
        .and_then(|c| char::from_u32(c.value()))
    else {
        return Mirror::Not;
    };
    if BidiProps::of(c).mirrored != BidiMirrored::Yes {
        return Mirror::Not;
    }
    let bare = !cell.flags.is_marked()
        || term
            .cluster_marks(cell)
            .all(|m| char::from_u32(m).is_some_and(is_bidi_control));
    match (bare, mirroring_glyph(c)) {
        (true, Some(m)) => Codepoint::new(u32::from(m))
            .map_or(Mirror::Missing, |cp| Mirror::Glyph(Content::codepoint(cp))),
        _ => Mirror::Missing,
    }
}

/// **Whether a codepoint is one of UAX #9's twelve bidi controls**: ALM,
/// LRM and RLM (classes AL, L and R, so a class test misses them), the
/// embeddings and overrides with PDF, and the isolates. Each is
/// Default_Ignorable_Code_Point and draws nothing of its own, so a cell
/// drawn without them looks as it did with them.
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Where a row's text stops: past its last cell that is not erased.
fn row_end(cells: &[PackedCell]) -> Col {
    Col::at_index(
        cells
            .iter()
            .rposition(|c| !c.content.is_empty())
            .map_or(0, |k| k + 1),
    )
}

/// How many codepoints a row gives the algorithm.
fn chars_of(term: &Term<'_>, cells: &[PackedCell]) -> Chars {
    let end = row_end(cells).index();
    Chars(
        cells[..end]
            .iter()
            .filter(|c| !c.content.is_wide_tail())
            .map(|c| match c.flags.is_marked() {
                true => 1 + term.cluster_marks(*c).count(),
                false => 1,
            })
            .sum(),
    )
}

/// **What a row holds that its layout reads**: one pass with no early
/// exit and one load a cell from [`baud_domain::LAYOUT_BLOCKS`], and only
/// for a row it holds back -- one with a character in a block that holds a
/// right-to-left character or a joining letter, or one with a cluster --
/// each answer's test of its cells. A row of Latin, of Han or of box
/// drawing holds nothing.
fn row_holds(term: &Term<'_>, cells: &[PackedCell]) -> Holds {
    if !may_hold(cells) {
        return Holds::NOTHING;
    }
    let yes = |b: bool| match b {
        true => Opens::Yes,
        false => Opens::No,
    };
    Holds {
        opens: yes(opens_in(term, cells)),
        joins: yes(joins_in(term, cells)),
    }
}

/// Whether any of `cells` is in a block [`baud_domain::LAYOUT_BLOCKS`]
/// sets or holds a cluster: the one pass, which lets most rows go.
fn may_hold(cells: &[PackedCell]) -> bool {
    cells.iter().fold(false, |any, c| {
        any | in_layout_block(c.content.bits()) | c.flags.is_marked()
    })
}

/// **Whether a row holds a letter that takes a joining form**: the one
/// pass of [`may_hold`], then a test of each cell, a cluster's base read
/// for a cell that holds one -- the joining form is the base's.
fn row_joins(term: &Term<'_>, cells: &[PackedCell]) -> bool {
    may_hold(cells) && joins_in(term, cells)
}

/// Whether one of `cells` takes a joining form.
fn joins_in(term: &Term<'_>, cells: &[PackedCell]) -> bool {
    cells.iter().any(|c| {
        let w = c.content.bits();
        if w <= 0x10_FFFF {
            takes_form(w)
        } else if c.flags.is_marked() {
            term.base_of(*c)
                .drawn()
                .as_codepoint()
                .is_some_and(|b| takes_form(b.value()))
        } else {
            false
        }
    })
}

/// Whether a row holds a character that can make its paragraph reorder:
/// the one pass of [`may_hold`], then a test of each cell's word, and a
/// cluster's codepoints read only for a cell that holds one.
fn row_opens(term: &Term<'_>, cells: &[PackedCell]) -> bool {
    may_hold(cells) && opens_in(term, cells)
}

/// Whether one of `cells` can make its paragraph reorder.
fn opens_in(term: &Term<'_>, cells: &[PackedCell]) -> bool {
    cells.iter().any(|c| {
        let w = c.content.bits();
        if w <= 0x10_FFFF {
            opens_rtl(w)
        } else if c.flags.is_marked() {
            term.base_of(*c)
                .drawn()
                .as_codepoint()
                .is_some_and(|b| opens_rtl(b.value()))
                || term.cluster_marks(*c).any(opens_rtl)
        } else {
            false
        }
    })
}
