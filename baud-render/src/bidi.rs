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
//! **A line's paragraph properties** (SCP, SPD, `?2501`, `?2500`;
//! `baud_core`'s line rules keep them on every row of a paragraph): with
//! BDSM set and autodetection on, the level P2-P3 finds, falling back to
//! the path; with it off, the path outright (UAX #9 HL1); with BDSM reset a
//! right-to-left line is drawn in reverse (`Reorder::reverse_paths`). With
//! `?2500` a Box Drawing character drawn right to left is drawn as its
//! counterpart (`baud_domain::box_counterpart`).
//!
//! **A table row keeps its columns.** A paragraph holding a box-drawing
//! character ([`baud_domain::is_table_rule`]) is a table row: each rule is given to
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
//! **Mirroring (L4).** Every Bidi_Mirrored character resolved right to
//! left is drawn mirrored. One that has a Bidi_Mirroring_Glyph draws it,
//! and so does one whose cell carries only bidi controls (an RLM after a
//! bracket), which draw nothing of their own: the drawn cell is the mirror
//! alone. One that has a mark that draws keeps it, the mirror its base
//! ([`Mirrored::Under`]), as VTE's `_vte_unistr_replace_base()` does: `<`
//! and U+0338 draw as `>` and U+0338, as U+226E's mirror U+226F does. The
//! 126 with no such glyph, for which BidiMirroring.txt leaves it "up to the
//! rendering system to provide mirrored glyphs"
//! (`[[source: unicode-bidimirroring]]`), draw their own glyph flipped left
//! to right in their span ([`Mirrored::Flipped`]), a mark they carry kept
//! unflipped at the mirrored place.
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

use baud_core::{BidiSupport, StatusLine, Term};
use baud_domain::{
    Bidi, BidiClass, BidiLevel, BidiMirrored, BidiParagraph, BidiProps, BoxMirror, CharacterPath,
    ClusterEpoch, ClusterSlot, Codepoint, Col, Cols, Content, JoiningForm, JoiningType, LamAlef,
    LineBidi, PackedCell, Row, RowExtent, RowRot, Rows, Stride, Viewport, Wrap, box_counterpart,
    in_layout_block, is_table_rule, join, lam_alefs, mirroring_glyph, opens_rtl, takes_form,
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

/// The stand-in for an erased cell between characters: U+200B ZERO WIDTH
/// SPACE, of class BN and no bracket.
const ERASED: char = '\u{200B}';

/// The stand-in for a column rule: U+2029 PARAGRAPH SEPARATOR, class B,
/// which ends a paragraph (P1).
const PARAGRAPH_SEPARATOR: char = '\u{2029}';

/// The order one row is drawn in.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// The visual columns drawn mirrored some other way, left to right:
    /// flipped, or a mirror under the cell's marks.
    mirrors: Vec<(Col, Mirrored)>,
    /// The columns the row is laid out over: the screen's, or on a line
    /// that is not single-width its half, each of its characters drawn
    /// over two of the screen's columns.
    drawn: Cols,
}

/// **How a cell L4 mirrors is drawn, where its content alone cannot say**
/// ([`Orders::mirrored`]): a cell drawn as its Bidi_Mirroring_Glyph alone
/// holds that glyph instead ([`Orders::permute`]) and needs neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mirrored {
    /// The cell's base, one of the 126 Bidi_Mirrored characters with no
    /// Bidi_Mirroring_Glyph, drawn flipped left to right in its span; a
    /// mark it carries is drawn unflipped where the flip puts it.
    Flipped,
    /// The cell's base drawn as its Bidi_Mirroring_Glyph, under the marks
    /// the cell carries.
    Under(Codepoint),
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

    /// **How visual column `v` of row `r` is mirrored**, where its
    /// content does not say so: flipped, or a mirror under its marks.
    pub fn mirrored(&self, r: Row, v: Col) -> Option<Mirrored> {
        self.row(r)
            .and_then(|o| o.mirrors.iter().find(|&&(c, _)| c == v).map(|&(_, m)| m))
    }

    /// Row `r`'s mirrored cells, by visual column, left to right: none on
    /// a row in its own order.
    pub fn mirrors(&self, r: Row) -> &[(Col, Mirrored)] {
        self.row(r).map_or(&[], |o| o.mirrors.as_slice())
    }

    /// **The character visual column `v` of row `r` is drawn as, where L4
    /// gives it a mirror its content does not hold** ([`Orders::permute`]
    /// draws it so): what a record drawn there in place of the cell draws.
    pub fn swapped(&self, r: Row, v: Col) -> Option<Content> {
        self.row(r)
            .and_then(|o| o.swaps.iter().find(|&&(c, _)| c == v).map(|&(_, c)| c))
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

    /// **The column a pointer over screen column `col` of row `r` names**,
    /// as a mouse report and a selection read it: the screen column of the
    /// character drawn under it. On a row drawn in its own order, `col`; on
    /// a line that is not single-width, whose characters each cover two of
    /// the screen's columns, the logical character the drawn one under the
    /// pointer shows, at the same half of its two columns, as a report
    /// keeps the display column (`crate::char_col` halves it again).
    pub fn pointer_col(&self, r: Row, col: Col) -> Col {
        let Some(o) = self.row(r) else {
            return col;
        };
        match o.drawn.index() < self.cols.index() {
            true => {
                let (v, half) = (col.index() / 2, col.index() % 2);
                let l = self.logical_col(r, Col::at_index(v)).index();
                Col::at_index(l * 2 + half)
            }
            false => self.logical_col(r, col),
        }
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

    /// **The status line as the cell path reads it**: `line` itself while
    /// its row, the maps' row `r` after the window's, is in its own order,
    /// else its cells copied into `cells` in their drawn order.
    pub fn status_line<'a>(
        &self,
        r: Row,
        line: StatusLine<'a>,
        cells: &'a mut Vec<PackedCell>,
    ) -> StatusLine<'a> {
        if !self.reordered(r) {
            return line;
        }
        cells.clear();
        cells.resize(line.cells.len(), PackedCell::EMPTY);
        self.permute(r, line.cells, cells);
        StatusLine { cells, ..line }
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

/// **Cells a layout reads in place of the core's**: an input method's
/// composition and a predicted echo, each drawn as the same text written
/// there would be drawn (Jev AS_WRITTEN, 0.98 in both orders for the
/// composition, 1.00 in both for the echo). The row they are on is laid out
/// with their characters in the cells they cover, so it is drawn in the
/// order a write would give it: a Latin letter typed after Latin text in a
/// right-to-left paragraph lands right of that text, which moves left to
/// make room, as the echo will draw it. Empty on nearly every frame, and
/// then a layout reads the core's cells alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    rows: Vec<(Row, Vec<PackedCell>)>,
}

impl Written {
    /// Nothing written over any row.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Whether nothing is written over any row.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// **`cells` written over row `r` of `vp` from column `at`**, over what
    /// an earlier write left there: each cell as a write leaves it, a wide
    /// character's tail after its lead. A wide character the write covers
    /// half of is erased whole, as the core erases it, and a cell past the
    /// row's end is not written.
    ///
    /// Out of line, as [`crate::Composition::written`] is: both run only
    /// while a composition or an echo is shown, and inlined they made the
    /// layout [`crate::Renderer::convert`] calls too large to be inlined
    /// into it.
    #[inline(never)]
    pub fn write(&mut self, vp: &Viewport<'_>, r: Row, at: Col, cells: &[PackedCell]) {
        if r.index() >= vp.rows().index() || cells.is_empty() {
            return;
        }
        let ncols = vp.cols().index();
        let i = match self.rows.iter().position(|&(w, _)| w == r) {
            Some(i) => i,
            None => {
                let row = vp.row(r);
                self.rows.push((r, row[..ncols.min(row.len())].to_vec()));
                self.rows.len() - 1
            }
        };
        let row = &mut self.rows[i].1;
        let (from, to) = (at.index(), (at.index() + cells.len()).min(row.len()));
        if from >= to {
            return;
        }
        if row[from].content.is_wide_tail() && from > 0 {
            row[from - 1] = PackedCell {
                content: Content::EMPTY,
                ..row[from - 1]
            };
        }
        if let Some(next) = row.get_mut(to).filter(|c| c.content.is_wide_tail()) {
            *next = PackedCell {
                content: Content::EMPTY,
                ..*next
            };
        }
        row[from..to].copy_from_slice(&cells[..to - from]);
    }
}

/// **Rows a layout reads in place of the core's**: [`Written`]'s, or
/// none -- `Unwritten`, which a layout over the core's cells alone is
/// compiled for, so it reads each row as the window holds it and asks
/// nothing else.
trait Over {
    /// Row `r` as written, if anything is written over it.
    fn row(&self, r: Row) -> Option<&[PackedCell]>;
}

impl Over for Written {
    fn row(&self, r: Row) -> Option<&[PackedCell]> {
        self.rows
            .iter()
            .find(|&&(w, _)| w == r)
            .map(|(_, c)| c.as_slice())
    }
}

/// Nothing written over any row.
struct Unwritten;

impl Over for Unwritten {
    #[inline(always)]
    fn row(&self, _: Row) -> Option<&[PackedCell]> {
        None
    }
}

/// How much of a window a layout lays out: every row, or only the
/// paragraph its first row is in -- the row a smooth scroll's slide draws
/// above the window, laid out as it was when it was the window's first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Extent {
    Window,
    Top,
}

/// The window's rows in their visual order, for one frame: the maps
/// ([`Orders`]) and the algorithm's working state.
pub struct Reorder {
    orders: Orders,
    seen: Seen,
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
    /// Each window row's paragraph properties at the last
    /// [`Reorder::keep_displayed`].
    paths: Vec<LineBidi>,
    /// The status line's row as last tested ([`Reorder::lay_out_status`]).
    status_seen: Seen,
}

/// Where a paragraph's context lies: in the rows around the window, read
/// from the history and the screen, or nowhere -- the status line, a
/// line of its own below the main display.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    Window,
    None,
}

/// Whether a codepoint of a paragraph's text is a column rule: a table
/// row's cell separator ([`is_table_rule`]).
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
            work: Work::default(),
            last: Last::LaidOut,
            bidi: Box::new(Bidi::new()),
            text: Vec::new(),
            owner: Vec::new(),
            order: Vec::new(),
            rule: Vec::new(),
            piece: Vec::new(),
            levels: Vec::new(),
            paths: Vec::new(),
            status_seen: Seen::default(),
        }
    }

    /// The maps the last layout made.
    pub fn orders(&self) -> &Orders {
        &self.orders
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
        self.lay_out_over(term, back, joins, &Unwritten, Extent::Window);
    }

    /// [`Reorder::lay_out_as`], each row `written` covers read as written
    /// ([`Written`]).
    pub fn lay_out_written(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        joins: Joins,
        written: &Written,
    ) {
        self.lay_out_over(term, back, joins, written, Extent::Window);
    }

    /// **Only the paragraph the window's first row is in**, laid out as
    /// [`Reorder::lay_out_as`] lays it out: row 0 of the maps is that row
    /// as a window `back` lines into the history draws it, and the other
    /// rows are left in their own order. What a smooth scroll's leaving row
    /// is drawn by, laid out over the window one line up.
    pub fn lay_out_top(&mut self, term: &Term<'_>, back: Rows, joins: Joins) {
        self.lay_out_over(term, back, joins, &Unwritten, Extent::Top);
    }

    // Inlined into each caller, so its `extent` is a constant there.
    #[inline(always)]
    fn lay_out_over(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        joins: Joins,
        written: &impl Over,
        extent: Extent,
    ) {
        self.lay_out_window(term, back, joins, written, extent);
        if extent == Extent::Window {
            self.lay_out_status(term, joins);
        }
    }

    /// **The status line, laid out as a line of its own** (Jev
    /// OWN_PARAGRAPH, 0.94/0.97): one row, so one paragraph, under the
    /// terminal's BDSM and the line properties SCP and `?2501` gave it
    /// while DECSASD selected it ([`Term::status_view`]) -- ECMA-48's
    /// IMPLICIT, "All bi-directional aspects of data are handled by the
    /// device itself" (`[[source: ecma48]]` 7.2.1), on the one line the
    /// main display's rows do not hold. Its order is the maps' row after
    /// the window's, where every draw path reads the status line's row.
    /// Nothing while no status line is shown.
    fn lay_out_status(&mut self, term: &Term<'_>, joins: Joins) {
        let Some(line) = term.status_view() else {
            return;
        };
        let at = self.orders.rows.len();
        let main = core::mem::take(&mut self.orders);
        let seen = core::mem::replace(&mut self.seen, core::mem::take(&mut self.status_seen));
        self.orders.cols = line.cols();
        self.orders.rows.push(None);
        self.orders.joins.push(None);
        let width = Some((line.cols(), term.cluster_epoch()));
        if self.seen.width != width {
            self.seen.width = width;
            self.seen.opens.clear();
            self.seen.joins.clear();
        }
        match term.bidi_support() {
            BidiSupport::Explicit => {
                self.reverse_paths(term, &line);
                if joins == Joins::Seen
                    && self
                        .seen
                        .holds(term, Row::ZERO, line.row(Row::ZERO), &mut self.work)
                        .joins
                        == Opens::Yes
                {
                    self.join_shown(term, Row::ZERO, line.row(Row::ZERO));
                }
            }
            BidiSupport::Implicit => self.paragraph(
                term,
                Rows::ZERO,
                &line,
                &Unwritten,
                (Row::ZERO, Row::ZERO),
                Context::None,
            ),
        }
        let status = core::mem::replace(&mut self.orders, main);
        self.status_seen = core::mem::replace(&mut self.seen, seen);
        let (Some(row), Some(joined)) = (
            status.rows.into_iter().next(),
            status.joins.into_iter().next(),
        ) else {
            return;
        };
        self.orders.rows.resize_with(at + 1, || None);
        self.orders.joins.resize_with(at + 1, || None);
        if row.is_some() {
            self.orders.any = Any::Yes;
        }
        if joined.is_some() {
            self.orders.joined_any = Any::Yes;
        }
        self.orders.rows[at] = row;
        self.orders.joins[at] = joined;
    }

    // The window's rows, without the status line.
    #[inline(always)]
    fn lay_out_window(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        joins: Joins,
        written: &impl Over,
        extent: Extent,
    ) {
        let back = term.looking_back(back);
        let vp = term.view(back);
        let nrows = vp.rows().index();
        self.orders.cols = vp.cols();
        self.orders.any = Any::No;
        self.work = Work::default();
        self.last = Last::LaidOut;
        self.orders.rows.clear();
        self.orders.rows.resize_with(nrows, || None);
        self.forget_joins(vp.rows());
        let explicit = term.bidi_support() == BidiSupport::Explicit;
        if explicit {
            self.reverse_paths(term, &vp);
        }
        if explicit && joins == Joins::Unseen {
            return;
        }
        let width = Some((vp.cols(), term.cluster_epoch()));
        if self.seen.width != width {
            self.seen.width = width;
            self.seen.opens.clear();
            self.seen.joins.clear();
        }
        let row_of = |r: Row| written.row(r).unwrap_or_else(|| vp.row(r));
        if explicit {
            let n = match extent {
                Extent::Window => nrows,
                Extent::Top => nrows.min(1),
            };
            for y in 0..n {
                let r = Row::from_index(y);
                if self.seen.holds(term, r, row_of(r), &mut self.work).joins == Opens::Yes {
                    self.join_shown(term, r, row_of(r));
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
                written,
                (Row::from_index(first), Row::from_index(y)),
                Context::Window,
            );
            if extent == Extent::Top {
                return;
            }
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
        self.work = Work::default();
        self.orders.rows.clear();
        self.orders.rows.resize_with(nrows, || None);
        // The status line's joins, after the window's, are found again
        // below whatever the window keeps.
        self.orders.joins.truncate(nrows);
        self.reverse_paths(term, &vp);
        let now = Last::Kept(vp.cols(), term.cluster_epoch(), vp.rows(), changes);
        // A row whose path changed is shown in another order with the same
        // cells, which no caller's compare sees: its joins are found again.
        let paths = (0..nrows).map(|y| vp.bidi(Row::from_index(y)));
        let same_paths = self.paths.len() == nrows && self.paths.iter().copied().eq(paths.clone());
        self.paths.clear();
        self.paths.extend(paths);
        let rejoin = match self.last == now && same_paths {
            true => Rejoin::Changed,
            false => {
                self.forget_joins(vp.rows());
                Rejoin::Every
            }
        };
        self.last = now;
        self.lay_out_status(term, Joins::Seen);
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
            self.join_shown(term, r, vp.row(r));
        }
    }

    /// **With BDSM reset, each row whose line runs right to left drawn in
    /// reverse**: terminal-wg's explicit RTL, "Cells are laid out in the
    /// exact reverse order than in explicit LTR. The model's first column is
    /// displayed in the rightmost cell ... Mirrorable glyphs are mirrored.
    /// The I-Beam shaped cursor is displayed on the right side of its cell"
    /// (`[[source: terminal-wg-bidi-basic-modes]]`). A wide character's two
    /// cells keep their order, its lead on the left. Every cell is at level
    /// 1, so each is shaped on its own and a bar cursor sits on its right.
    /// On a line that is not single-width the rightmost cell is its half's
    /// last (`[[source: dec-std-070]]` § 5.14.1), and the columns past it
    /// follow in order. One read of each row's path a frame, and the cells
    /// of the rows that hold one.
    fn reverse_paths(&mut self, term: &Term<'_>, vp: &Viewport<'_>) {
        let cols = vp.cols().index();
        for y in 0..vp.rows().index() {
            let r = Row::from_index(y);
            let line = vp.bidi(r);
            if line.path() != CharacterPath::RightToLeft {
                continue;
            }
            let cells = vp.row(r);
            let width = vp.rendition(r).end_of_line(vp.cols()).index().min(cols);
            let mut out = RowOrder {
                from: Vec::with_capacity(cols),
                to: vec![Col::ZERO; cols],
                level: vec![BidiLevel::RTL; cols],
                swaps: Vec::new(),
                mirrors: Vec::new(),
                drawn: Cols::of_index(width),
            };
            let mut c = width;
            while c > 0 {
                c -= 1;
                match c > 0 && cells[c].content.is_wide_tail() {
                    true => {
                        out.from.push(Col::at_index(c - 1));
                        out.from.push(Col::at_index(c));
                        c -= 1;
                    }
                    false => out.from.push(Col::at_index(c)),
                }
            }
            out.from.extend((width..cols).map(Col::at_index));
            for (v, l) in out.from.iter().enumerate() {
                let v = Col::at_index(v);
                if let Some(slot) = out.to.get_mut(l.index()) {
                    *slot = v;
                }
                let cell = cells[l.index()];
                if cell.content.is_wide_tail() {
                    continue;
                }
                match mirror(term, cell, line.box_mirror()) {
                    Mirror::Glyph(m) => out.swaps.push((v, m)),
                    Mirror::Flipped(alone) => {
                        if let Some(base) = alone {
                            out.swaps.push((v, base));
                        }
                        out.mirrors.push((v, Mirrored::Flipped));
                    }
                    Mirror::Under(m) => out.mirrors.push((v, Mirrored::Under(m))),
                    Mirror::Not => {}
                }
            }
            self.orders.rows[y] = Some(out);
            self.orders.any = Any::Yes;
        }
    }

    /// Row `r`, `cells` as stored, joined in the order it is shown with
    /// BDSM reset: as stored, or reversed on a right-to-left line, whose
    /// joins are then found over the cells as [`Reorder::reverse_paths`]
    /// lays them out, by the visual column every draw path reads.
    fn join_shown(&mut self, term: &Term<'_>, r: Row, cells: &[PackedCell]) {
        if !self.orders.reordered(r) {
            return self.join_displayed(term, r, cells);
        }
        let mut shown = vec![PackedCell::EMPTY; cells.len()];
        self.orders.permute(r, cells, &mut shown);
        self.join_displayed(term, r, &shown);
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
        written: &impl Over,
        (first, last): (Row, Row),
        context: Context,
    ) {
        let row_of = |r: Row| written.row(r).unwrap_or_else(|| vp.row(r));
        let nrows = vp.rows().index();
        let budget = CONTEXT / vp.cols().index().max(1);
        let opens = |cells: &[PackedCell]| row_opens(term, cells);
        // Every drawn row is asked, so each keeps its answer for the next
        // frame.
        let mut drawn_opens = false;
        let mut drawn_joins = false;
        for y in first.index()..=last.index() {
            let r = Row::from_index(y);
            let h = self.seen.holds(term, r, row_of(r), &mut self.work);
            drawn_opens |= h.opens == Opens::Yes;
            drawn_joins |= h.joins == Opens::Yes;
        }
        // The rows above the window the paragraph starts in, and below it
        // when the window looks back, nearest first: context, read only
        // when the paragraph reaches the window's edge -- so a paragraph
        // inside the window that holds no right-to-left character costs
        // its rows' test and nothing else.
        let mut above: Vec<&[PackedCell]> = Vec::new();
        if first.index() == 0 && context == Context::Window {
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
        if last.index() + 1 == nrows
            && vp.wrapped(last) == Wrap::Continues
            && context == Context::Window
        {
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
        // **The paragraph's character path** (SCP, SPD): every row of a
        // paragraph holds it, so its first drawn row says. A path set
        // outright is the paragraph's level (UAX #9 HL1), and a right-to-left
        // one lays a paragraph out against the right edge whatever its
        // letters, so it is resolved even when nothing in it opens.
        let path = vp.bidi(first);
        let any_opens = path.path() == CharacterPath::RightToLeft
            || drawn_opens
            || above.iter().any(|c| opens(c))
            || below.iter().any(|c| opens(c));
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
                cells: row_of(Row::from_index(y)),
                window: Some(Row::from_index(y)),
            })
            .collect();
        // The drawn rows in chunks that fit, each with the context that
        // fits around it: the rows above before the first chunk, the rows
        // below after the last.
        let cap = Chars(CONTEXT);
        // **The level its properties give it** ([`LineBidi::paragraph`]):
        // at power-on, with no path set and autodetection on, the level
        // UAX #9 P2-P3 finds -- 1 when its first strong letter, isolates
        // skipped, is R or AL, else 0 -- the owner's answer of 2026-10-24
        // ("all match C besides these 2"); a right-to-left paragraph's
        // erased cells past its text are laid out before it, so its rows
        // sit against the right edge. The letter is looked for only in the
        // text the layout reads, the paragraph's drawn rows and the context
        // around them ([`CONTEXT`]); a paragraph resolved in more than one
        // piece keeps the first piece's level for the rest (HL1).
        let mut dir = path.paragraph();
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
            if let Some(level) = self.resolve(term, vp, &rows, (dir, path), resolves) {
                dir = match level.direction() {
                    baud_domain::BidiDirection::Rtl => BidiParagraph::Rtl,
                    baud_domain::BidiDirection::Ltr => BidiParagraph::Ltr,
                };
            }
            start = end;
        }
    }

    /// Resolves `rows` as one paragraph at `dir` and orders each drawn one,
    /// row `y` of the window `vp` over the columns its rendition draws, or
    /// with [`Resolves::JoinOnly`] only joins it; answers the paragraph's
    /// level.
    fn resolve(
        &mut self,
        term: &Term<'_>,
        vp: &Viewport<'_>,
        rows: &[ParaRow<'_>],
        (dir, path): (BidiParagraph, LineBidi),
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
                let rule = match !cell.flags.is_marked() && is_table_rule(base) {
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
        // row runs left to right, every rule where it was written. A path
        // SCP or SPD set is each cell's level, HL1 on each cell's paragraph
        // (Jev's lean, CELLS_AT_SCP, 0.57/0.71).
        let table = self.rule.contains(&Rule::Yes);
        let dir = match table {
            true => path.paragraph(),
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
            // **Each display line over the columns it is drawn in** (UAX #9
            // 3.4, "for each line"): on a line that is not single-width,
            // its half (`[[source: dec-std-070]]` § 5.14.1, `end_line /=
            // 2`; `[[source: vt510-decdwl]]`), so a right-to-left line
            // runs against the half's right edge. The columns past it,
            // which nothing is written in or drawn from, follow in order.
            let width = vp.rendition(y).end_of_line(vp.cols()).index().min(cols);
            let end = ranges[p].1.index().min(width);
            let line = ranges[p].0.clone();
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
                mirrors: Vec::new(),
                drawn: Cols::of_index(width),
            };
            // The erased cells past the text go at the line's end in the
            // paragraph direction: before the text on a right-to-left line,
            // nearest the text last.
            if rtl {
                for c in (end..width).rev() {
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
                    let v = Col::at_index(out.from.len() - 1);
                    match mirror(term, cell, path.box_mirror()) {
                        Mirror::Glyph(m) => out.swaps.push((v, m)),
                        Mirror::Flipped(alone) => {
                            if let Some(base) = alone {
                                out.swaps.push((v, base));
                            }
                            out.mirrors.push((v, Mirrored::Flipped));
                        }
                        Mirror::Under(m) => out.mirrors.push((v, Mirrored::Under(m))),
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
            let past = match rtl {
                true => width,
                false => end,
            };
            for c in past..cols {
                out.from.push(Col::at_index(c));
                out.level.push(level);
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
    /// Its character's Bidi_Mirroring_Glyph, alone: the cell carries no
    /// mark that draws.
    Glyph(Content),
    /// Its glyph flipped, for a character with no Bidi_Mirroring_Glyph:
    /// with the base alone to draw where the cell carries only bidi
    /// controls, and `None` where its content is already that or it
    /// carries a mark that draws.
    Flipped(Option<Content>),
    /// Its Bidi_Mirroring_Glyph under the marks the cell carries.
    Under(Codepoint),
    /// Itself: it is not Bidi_Mirrored.
    Not,
}

fn mirror(term: &Term<'_>, cell: PackedCell, boxes: BoxMirror) -> Mirror {
    let Some(c) = term
        .base_of(cell)
        .drawn()
        .as_codepoint()
        .and_then(|c| char::from_u32(c.value()))
    else {
        return Mirror::Not;
    };
    // **`?2500`: a Box Drawing character drawn right to left is drawn as
    // its counterpart** (`[[source: terminal-wg-bidi-misc]]`, "mirror the
    // U+2500 .. U+257F glyphs in RTL"; UAX #9 HL6), under its marks as a
    // mirrored character is. Only one drawn right to left: L4's resolved R,
    // with HL6 widening which characters (Jev's lean, RESOLVED_RTL,
    // 0.60/0.74 then 0.88/0.92), so a table row's rules, kept where they
    // were written at the left-to-right level, are drawn as written.
    let counterpart = match boxes {
        BoxMirror::On => box_counterpart(c).filter(|m| *m != c),
        BoxMirror::Off => None,
    };
    if counterpart.is_none() && BidiProps::of(c).mirrored != BidiMirrored::Yes {
        return Mirror::Not;
    }
    let bare = !cell.flags.is_marked()
        || term
            .cluster_marks(cell)
            .all(|m| char::from_u32(m).is_some_and(is_bidi_control));
    let glyph = counterpart
        .or_else(|| mirroring_glyph(c))
        .and_then(|m| Codepoint::new(u32::from(m)));
    match (bare, glyph) {
        (true, Some(m)) => Mirror::Glyph(Content::codepoint(m)),
        (false, Some(m)) => Mirror::Under(m),
        (true, None) => Mirror::Flipped(
            cell.flags
                .is_marked()
                .then(|| Codepoint::new(u32::from(c)).map(Content::codepoint))
                .flatten(),
        ),
        (false, None) => Mirror::Flipped(None),
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
