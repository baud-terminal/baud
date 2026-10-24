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
//! than in their visual order. Arabic letters draw in the isolated forms
//! they draw in today; joining is a separate item.

use baud_core::{BidiSupport, Term};
use baud_domain::{
    Bidi, BidiLevel, BidiMirrored, BidiParagraph, BidiProps, ClusterEpoch, ClusterSlot, Codepoint,
    Col, Cols, Content, PackedCell, Row, RowExtent, RowRot, Rows, Stride, Viewport, Wrap,
    in_rtl_block, mirroring_glyph, opens_rtl,
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

/// **Which logical cell each visual cell of the window shows**, for the
/// rows that are drawn in another order: what every consumer of a frame
/// maps through. Cheap to clone: a row in its own order holds nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orders {
    rows: Vec<Option<RowOrder>>,
    cols: Cols,
    any: Any,
}

impl Default for Orders {
    fn default() -> Self {
        Orders {
            rows: Vec::new(),
            cols: Cols::of_index(0),
            any: Any::No,
        }
    }
}

impl Orders {
    /// Whether any row of the window is drawn in another order.
    pub fn any(&self) -> bool {
        self.any == Any::Yes
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

/// Whether a row holds a character that can make its paragraph reorder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Opens {
    Unknown,
    No,
    Yes,
}

/// **Each window row's answer, kept while the row is unchanged**: the rows
/// as last tested and what the test said, so a frame tests only the rows
/// that changed since -- a row is compared with its copy, a whole row at a
/// time, and read cell by cell only when it differs. A cluster's id can
/// name another cluster after a sweep, so a moved epoch tests every row
/// again.
#[derive(Default)]
struct Seen {
    cells: Vec<PackedCell>,
    opens: Vec<Opens>,
    width: Option<(Cols, ClusterEpoch)>,
}

impl Seen {
    /// Whether row `y`, `cells`, can reorder: the kept answer when the row
    /// is the one last tested, else the test, kept.
    fn opens(&mut self, term: &Term<'_>, row: Row, cells: &[PackedCell]) -> bool {
        let (y, n) = (row.index(), cells.len());
        if self.opens.len() <= y {
            self.opens.resize(y + 1, Opens::Unknown);
        }
        if self.cells.len() < (y + 1) * n {
            self.cells.resize((y + 1) * n, PackedCell::EMPTY);
        }
        let kept = &mut self.cells[y * n..(y + 1) * n];
        if self.opens[y] != Opens::Unknown && PackedCell::same_cells(kept, cells) {
            return self.opens[y] == Opens::Yes;
        }
        kept.copy_from_slice(cells);
        let yes = row_opens(term, cells);
        self.opens[y] = if yes { Opens::Yes } else { Opens::No };
        yes
    }
}

impl Reorder {
    pub fn new() -> Reorder {
        Reorder {
            orders: Orders::default(),
            seen: Seen::default(),
            unmirrored: Tally::default(),
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

    /// **Lays out the window `back` lines into the history**, as
    /// [`Term::view`] draws it. Nothing is reordered while BDSM is reset.
    pub fn lay_out(&mut self, term: &Term<'_>, back: Rows) {
        let back = term.looking_back(back);
        let vp = term.view(back);
        let nrows = vp.rows().index();
        self.orders.cols = vp.cols();
        self.orders.any = Any::No;
        self.unmirrored = Tally::default();
        self.orders.rows.clear();
        self.orders.rows.resize_with(nrows, || None);
        if term.bidi_support() == BidiSupport::Explicit {
            return;
        }
        let width = Some((vp.cols(), term.cluster_epoch()));
        if self.seen.width != width {
            self.seen.width = width;
            self.seen.opens.clear();
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
        for y in first.index()..=last.index() {
            let r = Row::from_index(y);
            drawn_opens |= self.seen.opens(term, r, vp.row(r));
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
        if !drawn_opens && !above.iter().any(|c| opens(c)) && !below.iter().any(|c| opens(c)) {
            return;
        }
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
            if let Some(level) = self.resolve(term, &rows, dir) {
                dir = match level.direction() {
                    baud_domain::BidiDirection::Rtl => BidiParagraph::Rtl,
                    baud_domain::BidiDirection::Ltr => BidiParagraph::Ltr,
                };
            }
            start = end;
        }
    }

    /// Resolves `rows` as one paragraph at `dir` and orders each drawn one;
    /// answers the paragraph's level.
    fn resolve(
        &mut self,
        term: &Term<'_>,
        rows: &[ParaRow<'_>],
        dir: BidiParagraph,
    ) -> Option<BidiLevel> {
        self.text.clear();
        self.owner.clear();
        self.rule.clear();
        let mut ranges: Vec<(core::ops::Range<usize>, Col)> = Vec::with_capacity(rows.len());
        for row in rows {
            let begin = self.text.len();
            let end = row_end(row.cells);
            for (c, &cell) in row.cells[..end.index()].iter().enumerate() {
                let at = |lead| Owner {
                    col: Col::at_index(c),
                    lead,
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
                continue;
            }
            for (v, l) in out.from.iter().enumerate() {
                if let Some(slot) = out.to.get_mut(l.index()) {
                    *slot = Col::at_index(v);
                }
            }
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

/// Whether a row holds a character that can make its paragraph reorder:
/// one test of each cell's word, and a cluster's codepoints read only for
/// a cell that holds one.
fn row_opens(term: &Term<'_>, cells: &[PackedCell]) -> bool {
    // A pass with no early exit and one load a cell from the compiled
    // block table: a row none of whose characters is in a block that
    // holds a right-to-left one, and which holds no cluster, holds nothing
    // that can reorder -- a row of Latin or of Han alike.
    let maybe = cells.iter().fold(false, |any, c| {
        any | in_rtl_block(c.content.bits()) | c.flags.is_marked()
    });
    if !maybe {
        return false;
    }
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
