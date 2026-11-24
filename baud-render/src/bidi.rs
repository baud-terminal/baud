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
//! UAX #9 defines them (terminal-wg's "level 2"). A cell moves whole, its
//! marks with it, which is what L3 asks of a renderer that draws a mark
//! after its base. A wide character's tail is not given; it follows its
//! lead, to the lead's right. An erased cell between characters is a
//! boundary neutral, removed by X9 and drawn with the character before it;
//! erased cells past a row's last character are left out and laid out at
//! the line's end in the paragraph direction
//! (`[[source: terminal-wg-bidi-erased-cells]]`). A paragraph longer than
//! [`CONTEXT`] codepoints is resolved over the rows drawn and as many
//! whole rows of context before and after them as fit, UAX #9 HL5's "piece
//! of text that is extracted from a longer sequence".
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
//! character ([`baud_domain::is_table_rule`]) is a table row: each rule
//! is given to the algorithm as a paragraph separator, so each cell
//! between two rules is a paragraph of its own at its own P2-P3 level
//! (UAX #9 section 3's table cells, HL4), each cell is ordered as a
//! line inside its own columns, every rule stays where it was written,
//! and the row is laid out left to right (`docs/bidi.md`).
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
//! ligature, centred over the two cells the pair holds -- over the cells
//! from one to the other where erased cells part them, and in the lam's
//! cell where a soft wrap does, the alef's cell drawing nothing. The forms
//! are found here, once a layout, over the logical text, and every draw
//! path reads them per visual cell ([`Orders::joined`]): with BDSM set,
//! over the paragraph across its soft wraps, which CSS Text 3 asks of a
//! word an unforced break splits ("as if the word were still whole",
//! `[[source: w3c-css-text-3]]`), and within each level run (UAX #9 3.5);
//! an erased cell between letters is transparent, the boundary neutral it
//! is given. With BDSM reset each row is joined in the order it is
//! displayed, the order 9.2.2 states the rules in: a program that wrote
//! Arabic in visual order is drawn joined, and presentation forms, joining
//! type U, are drawn as sent. A Mongolian or Phags-pa letter is read left
//! to right there, the way those scripts run on a horizontal line
//! (`[[source: unicode-ch13]]` 13.5, `[[source: unicode-ch14]]` 14.4), and
//! joins no letter of the other reading. A letter given no form is drawn
//! non-joining (R7), which is every Arabic letter on a row this finds
//! nothing to join.
//!
//! **What a frame costs the layout.** The last layout is kept a unit at a
//! time -- a paragraph, or with BDSM reset a row -- while no row the unit
//! read was written since the seal it was read under (`Term::seal_writes`):
//! a frame on which nothing was written reads no row and lays nothing out,
//! and one on which a row was asks each row's write generation once and
//! lays out again, whole, every paragraph that read a written row
//! (`kept`). **Each row's maps carry the generation that wrote them**
//! (`maps`): a reader keeps a seal beside what it drew through them
//! ([`Orders::laid_seal`]) and asks which rows were laid out again since
//! ([`Orders::laid_after`]), so a frame that laid nothing out compares and
//! copies no row of its maps or of the window in drawn order
//! (`Orders::window_kept`). **What joining costs a row with no joining
//! letter.** A row
//! laid out again is tested once, by the same pass with one load a cell:
//! the table it loads from, [`baud_domain::LAYOUT_BLOCKS`], is the
//! right-to-left blocks and the joining blocks together, compiled at
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
//! out with [`Reorder::lay_out`], which keeps each unit by the write
//! generations of the rows it read.

use baud_core::{BidiSupport, StatusLine, Term};
use baud_domain::{Attrs, DecimalMark, Mirroring, Slant, StyleWord, Variants, forced_form};
use baud_domain::{
    Bidi, BidiClass, BidiDirection, BidiLevel, BidiMirrored, BidiParagraph, BidiProps, BoxMirror,
    CharacterPath, ClusterEpoch, ClusterSlot, Codepoint, Col, Cols, Content, JoiningForm,
    JoiningType, LamAlef, LineBidi, PackedCell, Row, RowBlocks, RowExtent, RowRot, RowWritten,
    Rows, STRING_DEPTH, Seal, Stride, StringId, StyleId, StyleWrites, TableSerial, Viewport,
    WindowAt, Wrap, alaphs, box_counterpart, chain_runs, in_layout_block, is_table_rule, join,
    lam_alefs, mirroring_glyph, opens_rtl, order_strings, string_chain, takes_form,
};

use std::rc::Rc;

use crate::frozen::{RowSource, Words};
use crate::rebreak::{Breaker, Display, WordWrap};
use crate::select::{Highlight, WindowLine};
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

/// The stand-ins for an SDS or SRS string's edges, given to the algorithm
/// with BDSM set: U+2066 LEFT-TO-RIGHT ISOLATE, U+2067 RIGHT-TO-LEFT
/// ISOLATE and U+2069 POP DIRECTIONAL ISOLATE.
const LEFT_TO_RIGHT_ISOLATE: char = '\u{2066}';
const RIGHT_TO_LEFT_ISOLATE: char = '\u{2067}';
const POP_DIRECTIONAL_ISOLATE: char = '\u{2069}';

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
    /// The visual columns whose italic SAPV 17 slants left
    /// ([`Orders::slants`]), left to right.
    slants: Vec<Col>,
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
    /// at. The ligature is centred over them, a pair erased cells part
    /// included.
    Lam(Cols, PairEnd),
    /// The alef of such a pair.
    Alef(Cols, PairEnd),
    /// The lam of a pair a soft wrap parts, with its alef's cell: the
    /// ligature is drawn in the lam's cell.
    LamWrapped(PackedCell),
    /// The alef of such a pair, with its lam's cell: its letter is drawn
    /// in the lam's ligature, and its own cell draws no glyph.
    AlefWrapped(PackedCell),
}

/// **What joining gives one visual cell**: the form its letter is drawn
/// in, its part in a lam-alef ligature if it has one, and where the cells
/// of its word around it lie, which a face's contextual lookups read. A
/// cell whose letter is drawn non-joining, or that holds none, has no
/// entry: R7's form is what a letter draws given none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Joined {
    pub form: JoiningForm,
    pub pair: Option<(PairPart, LamAlef)>,
    pub around: Around,
}

/// **One cell of a joined cell's word**, read as the context its shaping
/// reads: the cell as the grid holds it, and the form its letter takes, if
/// it takes one other than Xn. A cell of a row the core froze keeps its
/// word, read where its row names it (`crate::frozen`): its id names
/// another table than the frame's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WordCell {
    pub cell: PackedCell,
    pub form: Option<JoiningForm>,
    pub word: Option<StyleWord>,
}

impl WordCell {
    /// Where its id names its word.
    pub(crate) fn words(&self) -> Words<'static> {
        match self.word {
            Some(w) => Words::Word(w),
            None => Words::LIVE,
        }
    }
}

/// A position in a row's [`WordCell`]s.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct WordIx(usize);

/// **Where a joined cell's word lies in its row's [`WordCell`]s**: the
/// cells before it in its logical order, from `lo`, the cell itself at
/// `at`, and the cells after it, up to `hi`; as many either side as the
/// layout's reach ([`Reorder::set_reach`]), within its level run, the
/// erased cells between letters left out. Empty with a reach of none.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Around {
    lo: WordIx,
    at: WordIx,
    hi: WordIx,
}

/// **One row's joined cells, by visual column, and the cells of their
/// words**, which may lie on the rows a soft wrap joins it to.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RowJoins {
    cells: Vec<Option<Joined>>,
    words: Vec<WordCell>,
}

impl RowJoins {
    /// The joined cells, by visual column.
    pub fn cells(&self) -> &[Option<Joined>] {
        &self.cells
    }

    /// The cells of `j`'s word before it, in logical order, the nearest
    /// last, and those after it, the nearest first.
    pub fn around(&self, j: &Joined) -> (&[WordCell], &[WordCell]) {
        let Around { lo, at, hi } = j.around;
        let before = self.words.get(lo.0..at.0).unwrap_or(&[]);
        let after = self.words.get(at.0 + 1..hi.0).unwrap_or(&[]);
        (before, after)
    }
}

/// **Each window row's maps and the generation that last wrote them**:
/// every write to a row's order or joins, or to the display the maps are
/// laid over, goes through a method here that stamps the row, so a reader
/// asks "were this row's maps written since I drew it" ([`LaidSeal`])
/// instead of copying them and comparing them the next frame. The fields
/// are this module's, so no write can miss its stamp. A layout that lays
/// rows out again lifts their maps out first and compares them at its end
/// ([`relay_quietly`]), so a row laid out again as it was is not stamped.
mod maps {
    use super::{RowJoins, RowOrder};
    use crate::rebreak::Display;
    use baud_domain::{Row, Rows};
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A count of the map writes of one [`Maps`]; a row's stamp is the
    /// write that last wrote it.
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
    struct LayGen(u32);

    /// **Which maps a generation counts in**: fresh for each [`Maps`] made,
    /// and again when its count wraps, so a seal from another layout -- the
    /// leaving row's, swapped in for a slide, or a renderer's made again --
    /// or from before a wrap reads every row as written.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct LayEpoch(u64);

    impl LayEpoch {
        fn fresh() -> LayEpoch {
            static NEXT: AtomicU64 = AtomicU64::new(1);
            LayEpoch(NEXT.fetch_add(1, Ordering::Relaxed))
        }
    }

    /// **What a reader keeps beside what it drew from the maps**
    /// ([`super::Orders::laid_seal`]): every map write after it reads as
    /// after it ([`super::Orders::laid_after`]). Not comparable: what a
    /// seal says is asked of the maps, never of another seal.
    #[derive(Clone, Copy, Debug)]
    pub struct LaidSeal {
        closed: LayGen,
        epoch: LayEpoch,
    }

    /// **The token of a relay** ([`relay_quietly`]): only a relay makes
    /// one, so a row's maps are lifted out ([`Maps::lift_row`],
    /// [`Maps::lift_order`], [`Maps::lift_all`]) only where the relay's end
    /// compares them with what the layout wrote.
    pub(super) struct Relay {
        _only: (),
    }

    /// Which of a row's maps a relay lays out again.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Lifting {
        /// Its order and its joins.
        Both,
        /// Its order alone: its joins stay in place, found again by a
        /// rejoin of their own ([`rejoin_quietly`]).
        Order,
    }

    /// Which of a row's maps a write wrote.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Map {
        Order,
        Joins,
        Both,
    }

    /// **A row's joins before a relay**: what they were, where the relay
    /// lays them out again, or left in place.
    #[derive(Debug)]
    enum JoinsWas {
        InPlace,
        Was(Option<RowJoins>),
    }

    /// **A row's maps before a relay laid it out again**, by index: not
    /// lifted, its writes stamped as any write is; or lifted, what it held.
    #[derive(Debug)]
    enum Lift {
        Kept,
        Lifted(Option<RowOrder>, JoinsWas),
    }

    /// The display before a relay that laid every row out again: untouched,
    /// or the one it replaced, compared at the relay's end.
    #[derive(Debug)]
    enum DisplayWas {
        Untouched,
        Was(Option<Rc<Display>>),
    }

    /// Whether a relay is running, and whether it has lifted anything:
    /// a relay that lifted nothing, as on a frame whose layout keeps every
    /// unit, ends with no row read.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Relaying {
        No,
        Running,
        Lifted,
    }

    /// The maps of a window's rows, the display they are laid over, and
    /// each row's stamp. A clone is maps of its own (its `Clone`).
    #[derive(Debug)]
    pub(super) struct Maps {
        rows: Vec<Option<RowOrder>>,
        joins: Vec<Option<RowJoins>>,
        display: Option<Rc<Display>>,
        /// Each row's stamp, by index: a row never written has none.
        stamps: Vec<LayGen>,
        /// The last write's generation.
        now: LayGen,
        epoch: LayEpoch,
        /// The row a rejoin is finding again, whose joins' writes are
        /// stamped once, at its end, and only if they changed: set and
        /// cleared by [`rejoin_quietly`] alone, around the rejoin.
        quiet: Option<Row>,
        /// **Each row's maps as they stood before the running relay laid
        /// it out again**, by index ([`relay_quietly`]): a lifted row's
        /// writes are not stamped while the relay runs, and at its end the
        /// row is stamped only where its maps came out otherwise. Every
        /// row [`Lift::Kept`] outside a relay; kept across relays, so a
        /// relay allocates none.
        lifted: Vec<Lift>,
        /// The display a relay that laid every row out again replaced.
        display_was: DisplayWas,
        /// Whether a relay is running: set and cleared by
        /// [`relay_quietly`] alone.
        relaying: Relaying,
    }

    /// The maps compare by what they hold, not by when it was written: a
    /// kept layout equals a fresh one.
    impl PartialEq for Maps {
        fn eq(&self, other: &Maps) -> bool {
            self.rows == other.rows && self.joins == other.joins && self.display == other.display
        }
    }

    impl Eq for Maps {}

    /// **A clone is maps of its own**: the same orders, joins and display
    /// under a fresh epoch and no stamps, so a seal taken from the maps it
    /// was cloned from -- by the overlay's copy of a frame's order, or the
    /// pointer's (`Seat::drawn` in the app) -- reads every row as written,
    /// never as nothing moved. Its stamps are not copied: it reads as all
    /// written against any seal but its own. No relay runs on it.
    impl Clone for Maps {
        fn clone(&self) -> Maps {
            Maps {
                rows: self.rows.clone(),
                joins: self.joins.clone(),
                display: self.display.clone(),
                ..Maps::default()
            }
        }
    }

    impl Default for Maps {
        fn default() -> Maps {
            Maps {
                rows: Vec::new(),
                joins: Vec::new(),
                display: None,
                stamps: Vec::new(),
                now: LayGen(0),
                epoch: LayEpoch::fresh(),
                quiet: None,
                lifted: Vec::new(),
                display_was: DisplayWas::Untouched,
                relaying: Relaying::No,
            }
        }
    }

    impl Maps {
        /// **Row `y`'s joins written**: stamped, unless a rejoin is finding
        /// them again, which stamps the row at its end if they changed, or
        /// a relay lifted them ([`Maps::stamp_of`]).
        fn stamp_joins(&mut self, y: Row) {
            if self.quiet != Some(y) {
                self.stamp_of(y, Map::Joins);
            }
        }

        /// **`map` of row `y` written**: stamped, unless a relay is running
        /// and lifted every map the write wrote, whose end compares them
        /// ([`relay_quietly`]).
        fn stamp_of(&mut self, y: Row, map: Map) {
            let lifted = self.relaying == Relaying::Lifted
                && matches!(
                    (self.lifted.get(y.index()), map),
                    (Some(Lift::Lifted(..)), Map::Order)
                        | (
                            Some(Lift::Lifted(_, JoinsWas::Was(_))),
                            Map::Joins | Map::Both
                        )
                );
            if !lifted {
                self.stamp(y);
            }
        }

        /// **Row `y` written**: a generation of its own, later than any
        /// seal taken before, stamped on it.
        fn stamp(&mut self, y: Row) {
            if self.now.0 == u32::MAX {
                self.epoch = LayEpoch::fresh();
                self.now = LayGen(0);
                self.stamps.fill(LayGen(0));
            }
            self.now = LayGen(self.now.0 + 1);
            let at = y.index();
            if self.stamps.len() <= at {
                self.stamps.resize(at + 1, LayGen(0));
            }
            self.stamps[at] = self.now;
        }

        /// `map` of the rows from `from` down to `to`, counted from the
        /// window's top, each written ([`Maps::stamp_of`]).
        fn stamp_span(&mut self, from: Rows, to: Rows, map: Map) {
            for y in from.index()..to.index() {
                self.stamp_of(Row::from_index(y), map);
            }
        }

        /// Each row's order, by index.
        pub(super) fn rows(&self) -> &[Option<RowOrder>] {
            &self.rows
        }

        /// Each row's joins, by index.
        pub(super) fn joins(&self) -> &[Option<RowJoins>] {
            &self.joins
        }

        /// The display the maps are laid over, if any.
        pub(super) fn display(&self) -> Option<&Rc<Display>> {
            self.display.as_ref()
        }

        /// **Row `y` drawn in `order`**, where the maps hold the row:
        /// stamped. Answers whether they hold it.
        pub(super) fn set_row(&mut self, y: Row, order: RowOrder) -> bool {
            if y.index() >= self.rows.len() {
                return false;
            }
            self.rows[y.index()] = Some(order);
            self.stamp_of(y, Map::Order);
            true
        }

        /// **Row `y`'s order, to change in place**, where it holds one:
        /// stamped. None, and nothing stamped, for a row in its own order.
        pub(super) fn row_mut(&mut self, y: Row) -> Option<&mut RowOrder> {
            if !self.rows.get(y.index()).is_some_and(Option::is_some) {
                return None;
            }
            self.stamp_of(y, Map::Order);
            self.rows.get_mut(y.index()).and_then(Option::as_mut)
        }

        /// **Row `y`'s order set to `order`, the maps grown to hold it**:
        /// each row gained stamped, and `y` unless it was and stays in its
        /// own order.
        pub(super) fn set_row_grown(&mut self, y: Row, order: Option<RowOrder>) {
            let had = Rows::of_index(self.rows.len());
            if had.index() <= y.index() {
                self.rows.resize_with(y.index() + 1, || None);
                self.stamp_span(had, Rows::of_index(y.index() + 1), Map::Order);
            }
            if self.rows[y.index()].is_none() && order.is_none() {
                return;
            }
            self.rows[y.index()] = order;
            self.stamp_of(y, Map::Order);
        }

        /// **Row `y`'s order, to change in place, made `made()` where it
        /// held none**, the maps grown to hold it: stamped, each row
        /// gained too.
        pub(super) fn row_made(
            &mut self,
            y: Row,
            made: impl FnOnce() -> RowOrder,
        ) -> &mut RowOrder {
            let had = Rows::of_index(self.rows.len());
            if had.index() <= y.index() {
                self.rows.resize_with(y.index() + 1, || None);
                self.stamp_span(had, Rows::of_index(y.index()), Map::Order);
            }
            self.stamp_of(y, Map::Order);
            self.rows[y.index()].get_or_insert_with(made)
        }

        /// **Row `y`'s joins set to `joins`**, where the maps hold the row:
        /// stamped, unless a rejoin is finding them again. Answers whether
        /// they hold it.
        pub(super) fn set_joins(&mut self, y: Row, joins: RowJoins) -> bool {
            if y.index() >= self.joins.len() {
                return false;
            }
            self.joins[y.index()] = Some(joins);
            self.stamp_joins(y);
            true
        }

        /// **Row `y`'s joins taken out**: stamped only where it held some,
        /// unless a rejoin is finding them again.
        pub(super) fn take_joins(&mut self, y: Row) -> Option<RowJoins> {
            let was = self.joins.get_mut(y.index()).and_then(Option::take)?;
            self.stamp_joins(y);
            Some(was)
        }

        /// **Row `y`'s joins, to change in place**, where it holds some:
        /// stamped, unless a rejoin is finding them again.
        pub(super) fn joins_mut(&mut self, y: Row) -> Option<&mut RowJoins> {
            if !self.joins.get(y.index()).is_some_and(Option::is_some) {
                return None;
            }
            self.stamp_joins(y);
            self.joins.get_mut(y.index()).and_then(Option::as_mut)
        }

        /// **Row `y`'s joins set to `joins`, the maps grown to hold it**:
        /// each row gained stamped, and `y` unless it held none and holds
        /// none, or a rejoin is finding them again.
        pub(super) fn set_joins_grown(&mut self, y: Row, joins: Option<RowJoins>) {
            let had = Rows::of_index(self.joins.len());
            if had.index() <= y.index() {
                self.joins.resize_with(y.index() + 1, || None);
                self.stamp_span(had, Rows::of_index(y.index() + 1), Map::Joins);
            }
            if self.joins[y.index()].is_none() && joins.is_none() {
                return;
            }
            self.joins[y.index()] = joins;
            self.stamp_joins(y);
        }

        /// **The joins grown to hold `n` rows**, each row gained stamped;
        /// those past `n` -- the status line's, after the window's -- kept.
        pub(super) fn grow_joins(&mut self, n: Rows) {
            let had = Rows::of_index(self.joins.len());
            if had.index() < n.index() {
                self.joins.resize_with(n.index(), || None);
                self.stamp_span(had, n, Map::Joins);
            }
        }

        /// Both maps grown to hold `n` rows, each row they gain stamped.
        pub(super) fn grow(&mut self, n: Rows) {
            let had = Rows::of_index(self.rows.len().min(self.joins.len()));
            if self.rows.len() < n.index() {
                self.rows.resize_with(n.index(), || None);
            }
            if self.joins.len() < n.index() {
                self.joins.resize_with(n.index(), || None);
            }
            self.stamp_span(had, n, Map::Both);
        }

        /// No row holds a joined cell, `n` rows of none: every row stamped.
        pub(super) fn clear_joins(&mut self, n: Rows) {
            let had = Rows::of_index(self.joins.len());
            self.joins.clear();
            self.joins.resize_with(n.index(), || None);
            self.stamp_span(
                Rows::ZERO,
                Rows::of_index(had.index().max(n.index())),
                Map::Joins,
            );
        }

        /// The joins past row `n` let go, each stamped.
        pub(super) fn truncate_joins(&mut self, n: Rows) {
            let had = Rows::of_index(self.joins.len());
            if had.index() > n.index() {
                self.joins.truncate(n.index());
                self.stamp_span(n, had, Map::Joins);
            }
        }

        /// The orders and joins past row `n` let go, each stamped.
        pub(super) fn truncate(&mut self, n: Rows) {
            let had = Rows::of_index(self.rows.len().max(self.joins.len()));
            if had.index() > n.index() {
                self.rows.truncate(n.index());
                self.joins.truncate(n.index());
                self.stamp_span(n, had, Map::Both);
            }
        }

        /// **Row `y`'s order and joins lifted out to be laid out again** by
        /// the relay `_`: the row left in its own order and unjoined,
        /// nothing stamped, and at the relay's end stamped only where the
        /// layout wrote it otherwise.
        pub(super) fn lift_row(&mut self, y: Row, _: &Relay) {
            self.lift(y, Lifting::Both);
        }

        /// **Row `y`'s order lifted out** as [`Maps::lift_row`] lifts it,
        /// its joins left in place for a rejoin of their own.
        pub(super) fn lift_order(&mut self, y: Row, _: &Relay) {
            self.lift(y, Lifting::Order);
        }

        /// **Every row's maps lifted out to be laid out again**, `lifting`
        /// of each, the orders made `n` rows of none over `display`: at the
        /// relay `_`'s end every row is stamped if the display came out
        /// otherwise, else each row only where the layout wrote it
        /// otherwise. A row the maps gain is stamped now.
        pub(super) fn lift_all(
            &mut self,
            n: Rows,
            display: Option<Rc<Display>>,
            lifting: Lifting,
            _: &Relay,
        ) {
            let had = Rows::of_index(self.rows.len().max(self.joins.len()));
            for y in 0..had.index() {
                self.lift(Row::from_index(y), lifting);
            }
            self.rows.clear();
            self.rows.resize_with(n.index(), || None);
            if let DisplayWas::Untouched = self.display_was {
                self.display_was = DisplayWas::Was(self.display.take());
            }
            self.display = display;
            self.relaying = Relaying::Lifted;
            self.stamp_span(had, n, Map::Both);
        }

        /// Row `y`'s maps, `lifting` of them, moved out into its slot of
        /// `lifted`; a row lifted already in this relay keeps what it held
        /// first.
        fn lift(&mut self, y: Row, lifting: Lifting) {
            debug_assert!(self.relaying != Relaying::No, "a lift outside a relay");
            self.relaying = Relaying::Lifted;
            let at = y.index();
            if self.lifted.len() <= at {
                self.lifted.resize_with(at + 1, || Lift::Kept);
            }
            if let Lift::Lifted(..) = self.lifted[at] {
                return;
            }
            let order = self.rows.get_mut(at).and_then(Option::take);
            let joins = match lifting {
                Lifting::Both => JoinsWas::Was(self.joins.get_mut(at).and_then(Option::take)),
                Lifting::Order => JoinsWas::InPlace,
            };
            self.lifted[at] = Lift::Lifted(order, joins);
        }

        /// **The relay ended**: each row it lifted compared with what the
        /// layout wrote, and stamped only where they differ -- every row
        /// lifted, where the display differs by value from the one it
        /// replaced. It leaves no row lifted and the display untouched
        /// whatever it finds running: only a relay that lifted nothing
        /// still reads [`Relaying::Running`], since no relay sets it over
        /// [`Relaying::Lifted`] ([`relay_quietly`]), and that one reads no
        /// row.
        fn settle_relay(&mut self) {
            let relaying = core::mem::replace(&mut self.relaying, Relaying::No);
            let display_was = core::mem::replace(&mut self.display_was, DisplayWas::Untouched);
            if relaying == Relaying::Running {
                return;
            }
            let display_moved = match display_was {
                DisplayWas::Untouched => false,
                DisplayWas::Was(was) => was != self.display,
            };
            let mut lifted = core::mem::take(&mut self.lifted);
            for (y, lift) in lifted.iter_mut().enumerate() {
                let Lift::Lifted(order, joins) = core::mem::replace(lift, Lift::Kept) else {
                    continue;
                };
                let moved = display_moved
                    || self.rows.get(y) != Some(&order)
                    || match joins {
                        JoinsWas::InPlace => false,
                        JoinsWas::Was(j) => self.joins.get(y) != Some(&j),
                    };
                if moved {
                    self.stamp(Row::from_index(y));
                }
            }
            self.lifted = lifted;
        }

        /// **A relay or a rejoin a panic left** ([`Mend`]): a fresh epoch,
        /// so every seal taken before reads every row as written and a
        /// reader draws whole from the maps as the panic left them; and no
        /// row lifted or quiet, no display kept and no relay running, so
        /// the next layout runs and stamps as any does.
        fn unwound(&mut self) {
            self.epoch = LayEpoch::fresh();
            self.quiet = None;
            self.relaying = Relaying::No;
            self.display_was = DisplayWas::Untouched;
            for lift in &mut self.lifted {
                *lift = Lift::Kept;
            }
        }

        /// The first row's order and joins, the maps let go: a line laid
        /// out on maps of its own.
        pub(super) fn into_first(self) -> Option<(Option<RowOrder>, Option<RowJoins>)> {
            Some((
                self.rows.into_iter().next()?,
                self.joins.into_iter().next()?,
            ))
        }

        /// **The seal a reader keeps**: never taken inside a rejoin, whose
        /// row's joins writes are not stamped until it ends.
        pub(super) fn seal(&self) -> LaidSeal {
            debug_assert!(self.quiet.is_none(), "a seal taken inside a rejoin");
            debug_assert!(self.relaying == Relaying::No, "a seal taken inside a relay");
            LaidSeal {
                closed: self.now,
                epoch: self.epoch,
            }
        }

        /// Whether row `y`'s maps were written after `kept`, or `kept` is
        /// another layout's or from before a wrap.
        pub(super) fn after(&self, y: Row, kept: &LaidSeal) -> bool {
            kept.epoch != self.epoch || self.stamps.get(y.index()).is_some_and(|g| *g > kept.closed)
        }

        /// Whether any row's maps were written after `kept`.
        pub(super) fn moved(&self, kept: &LaidSeal) -> bool {
            kept.epoch != self.epoch || self.now > kept.closed
        }
    }

    /// **What holds the [`Maps`] a rejoin writes through**
    /// ([`rejoin_quietly`]): the layout, whose rejoin finds the joins.
    pub(super) trait HoldsMaps {
        /// The maps, to write.
        fn maps_mut(&mut self) -> &mut Maps;
    }

    /// Which call a [`Mend`] stands beside.
    #[derive(Clone, Copy, Debug)]
    enum Mending {
        /// [`relay_quietly`]: open while a relay is running.
        Relay,
        /// [`rejoin_quietly`]: open while a row is quiet.
        Rejoin,
    }

    /// **What a relay or a rejoin lends its layout, put right if the
    /// layout panics**: the app catches a frame's panic and draws on with
    /// the same renderer (`baud-present-macos`'s `unwind.rs`), and a relay
    /// or a rejoin left open would leave its rows changed and unstamped, a
    /// stale draw nothing reports, and every later relay or seal refused.
    /// Dropped with its call still open -- the relay running, the row
    /// quiet, which only a panic leaves -- it takes a fresh epoch and
    /// closes it ([`Maps::unwound`]); after a call that ended, nothing.
    struct Mend<'a, T: HoldsMaps> {
        owner: &'a mut T,
        mending: Mending,
    }

    impl<T: HoldsMaps> Drop for Mend<'_, T> {
        fn drop(&mut self) {
            let m = self.owner.maps_mut();
            let open = match self.mending {
                Mending::Relay => m.relaying != Relaying::No,
                Mending::Rejoin => m.quiet.is_some(),
            };
            if open {
                m.unwound();
            }
        }
    }

    /// **Row `y`'s joins found again by `find`**
    /// ([`super::Reorder::rejoin`]): moved out, not copied; each write
    /// `find` makes to the row's joins unstamped; and at its end the row
    /// stamped once, only when they are not what was moved out, so a frame
    /// that finds every row's joins again -- a composition with BDSM reset
    /// -- marks none whose joins it found the same. The lift and the settle
    /// are this one call around `find`, so no return between them and no
    /// settle of another row can leave a row's later joins writes
    /// unstamped; a panic inside `find` leaves the row quiet no longer and
    /// every seal taken before reading every row as written ([`Mend`]).
    /// None, and `find` not run, where the maps do not hold the row.
    pub(super) fn rejoin_quietly<T: HoldsMaps, R>(
        owner: &mut T,
        y: Row,
        find: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let m = owner.maps_mut();
        debug_assert!(m.quiet.is_none(), "a rejoin inside a rejoin");
        let was = m.joins.get_mut(y.index())?.take();
        m.quiet = Some(y);
        let mend = Mend {
            owner,
            mending: Mending::Rejoin,
        };
        let found = find(&mut *mend.owner);
        let m = mend.owner.maps_mut();
        m.quiet = None;
        if m.joins.get(y.index()) != Some(&was) {
            m.stamp(y);
        }
        Some(found)
    }

    /// **A layout that lays rows out again, `lay_out`, around which each
    /// row it lifts is compared** ([`super::Reorder::lay_out`],
    /// [`super::Reorder::keep_displayed`]): `lay_out` lifts each row it lays
    /// out again ([`Maps::lift_row`], [`Maps::lift_order`],
    /// [`Maps::lift_all`]) with the token this hands it, its writes to
    /// those rows are not stamped, and at its end each is stamped only
    /// where its order or joins, or the display, came out otherwise -- a
    /// frame that lays a paragraph out again as it was, its rows written
    /// as they were or a composition over it shown again, marks none of
    /// its rows. The lift and the compare are this one call around the
    /// layout, so a layout that returns leaves no lifted row uncompared;
    /// one that panics leaves no row lifted and every seal taken before
    /// reading every row as written ([`Mend`]). A relay begun inside
    /// another, a debug assertion, keeps what the other lifted for its
    /// end to compare.
    pub(super) fn relay_quietly<T: HoldsMaps, R>(
        owner: &mut T,
        lay_out: impl FnOnce(&mut T, &Relay) -> R,
    ) -> R {
        let m = owner.maps_mut();
        debug_assert!(
            m.relaying == Relaying::No && m.quiet.is_none(),
            "a relay inside a relay or a rejoin"
        );
        if m.relaying == Relaying::No {
            m.relaying = Relaying::Running;
        }
        let mend = Mend {
            owner,
            mending: Mending::Relay,
        };
        let laid = lay_out(&mut *mend.owner, &Relay { _only: () });
        mend.owner.maps_mut().settle_relay();
        laid
    }

    #[cfg(test)]
    mod tests {
        use super::{
            DisplayWas, HoldsMaps, LayGen, Lift, Lifting, Maps, Relaying, RowJoins, rejoin_quietly,
            relay_quietly,
        };
        use baud_domain::{Row, Rows};
        use std::panic::{AssertUnwindSafe, catch_unwind};

        /// Maps that hold themselves, for a rejoin with no layout around it.
        impl HoldsMaps for Maps {
            fn maps_mut(&mut self) -> &mut Maps {
                self
            }
        }

        /// The rows of `m` written after `seal`, of the first `n`.
        fn after(m: &Maps, seal: &super::LaidSeal, n: Rows) -> Vec<usize> {
            (0..n.index())
                .filter(|&y| m.after(Row::from_index(y), seal))
                .collect()
        }

        /// Whether `m` holds no relay or rejoin open: none running, no row
        /// quiet or lifted, no display kept.
        fn closed(m: &Maps) -> bool {
            m.relaying == Relaying::No
                && m.quiet.is_none()
                && matches!(m.display_was, DisplayWas::Untouched)
                && m.lifted.iter().all(|l| matches!(l, Lift::Kept))
        }

        /// A layout that panics, as `what`.
        fn panics(what: &str) {
            panic!("{what}");
        }

        /// **The wrap**: a write past the last generation draws a fresh
        /// epoch, counts from one again and zeroes every row's stamp, so a
        /// seal from before it reads every row as written -- rows the write
        /// did not touch among them -- and the maps as moved. The control:
        /// before the wrap, the seal read the one row written after it.
        /// Named break: the wrap without a fresh epoch.
        #[test]
        fn a_write_past_the_last_generation_reads_every_row_as_written() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(4));
            m.now = LayGen(u32::MAX - 1);
            let seal = m.seal();
            m.stamp(Row::from_index(1));
            assert_eq!(
                after(&m, &seal, Rows::of_index(4)),
                vec![1],
                "the control: row 1 alone"
            );
            let old = m.epoch;
            m.stamp(Row::from_index(2));
            assert!(m.epoch != old, "the wrap draws a fresh epoch");
            assert_eq!(m.now, LayGen(1), "and counts from one");
            assert_eq!(
                after(&m, &seal, Rows::of_index(4)),
                vec![0, 1, 2, 3],
                "every row"
            );
            assert!(m.moved(&seal), "and the maps moved");
        }

        /// **A clone is maps of its own**: the same orders, joins and
        /// display, so it compares equal (the control), and a seal of the
        /// maps it was cloned from reads every row of it as written and it
        /// as moved, where the maps themselves read none. Named break: the
        /// clone's epoch copied.
        #[test]
        fn a_seal_of_the_maps_cloned_reads_every_row_of_the_clone_as_written() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let seal = m.seal();
            let c = m.clone();
            assert!(c == m, "the control: the same maps");
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                Vec::<usize>::new(),
                "the maps"
            );
            assert!(!m.moved(&seal), "the maps did not move");
            assert_eq!(
                after(&c, &seal, Rows::of_index(3)),
                vec![0, 1, 2],
                "the clone"
            );
            assert!(c.moved(&seal), "the clone moved");
        }

        /// **A rejoin's row is quiet only inside it** (`rejoin_quietly`):
        /// inside, a write to the row's joins is not stamped; at its end the
        /// row is stamped, its joins having changed, none to some (the
        /// control); a rejoin that finds them the same stamps nothing; and
        /// after it a write to the row's joins is stamped. Named break: the
        /// row left quiet at a rejoin's end.
        #[test]
        fn a_rejoins_row_is_quiet_only_inside_it() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let y = Row::from_index(1);
            let seal = m.seal();
            let inside = rejoin_quietly(&mut m, y, |m| {
                let _ = m.set_joins(y, RowJoins::default());
                m.after(y, &seal)
            });
            assert_eq!(inside, Some(false), "inside, the write not stamped");
            assert!(
                m.after(y, &seal),
                "the control: at its end, stamped, its joins changed"
            );
            let seal = m.seal();
            let _ = rejoin_quietly(&mut m, y, |m| {
                let _ = m.set_joins(y, RowJoins::default());
            });
            assert!(!m.after(y, &seal), "found the same: not stamped");
            let _ = m.take_joins(y);
            assert!(m.after(y, &seal), "after it, a write stamped");
        }

        /// **A relay stamps only the rows it lifted and wrote otherwise**
        /// (`relay_quietly`): rows 1 and 2 hold joins; a relay lifts both,
        /// writes row 1's back as they were and leaves row 2 unjoined. Row
        /// 2 alone reads as written, and the maps as moved; inside the
        /// relay neither write was stamped. A relay that lifts row 1 and
        /// writes it as it was marks nothing (the maps did not move), and a
        /// write after a relay is stamped. Named break: every lifted row
        /// stamped at the relay's end, whatever the layout wrote.
        #[test]
        fn a_relay_stamps_only_the_rows_it_wrote_otherwise() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(4));
            let (one, two) = (Row::from_index(1), Row::from_index(2));
            let _ = m.set_joins(one, RowJoins::default());
            let _ = m.set_joins(two, RowJoins::default());
            let seal = m.seal();
            let inside = relay_quietly(&mut m, |m, relay| {
                m.lift_row(one, relay);
                m.lift_row(two, relay);
                let _ = m.set_joins(one, RowJoins::default());
                (m.after(one, &seal), m.after(two, &seal))
            });
            assert_eq!(inside, (false, false), "inside, nothing stamped");
            assert_eq!(after(&m, &seal, Rows::of_index(4)), vec![2], "row 2 alone");
            assert!(m.moved(&seal), "the control: the maps moved");
            let seal = m.seal();
            relay_quietly(&mut m, |m, relay| {
                m.lift_row(one, relay);
                let _ = m.set_joins(one, RowJoins::default());
            });
            assert_eq!(
                after(&m, &seal, Rows::of_index(4)),
                Vec::<usize>::new(),
                "laid out as it was: no row"
            );
            assert!(!m.moved(&seal), "and the maps did not move");
            let _ = m.take_joins(one);
            assert!(m.after(one, &seal), "after it, a write stamped");
        }

        /// **A relay a panic left reads every row as written**
        /// (`relay_quietly`, its `Mend`): a layout that lifts row 1 and
        /// panics, caught as the app catches a frame's panic. A seal taken
        /// before reads every row as written, rows the layout never reached
        /// among them, and no relay is left open; then a relay that lays
        /// row 1 out as the panic left it marks no row, and one that writes
        /// it otherwise marks it alone. The control: the layout panicked.
        /// Named breaks: the mend without a fresh epoch; the mend leaving
        /// the relay running.
        #[test]
        fn a_relay_a_panic_left_reads_every_row_as_written() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let one = Row::from_index(1);
            let _ = m.set_joins(one, RowJoins::default());
            let seal = m.seal();
            let caught = catch_unwind(AssertUnwindSafe(|| {
                relay_quietly(&mut m, |m, relay| {
                    m.lift_row(one, relay);
                    panics("a layout that panics");
                })
            }));
            assert!(caught.is_err(), "the control: the layout panicked");
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                vec![0, 1, 2],
                "every row read as written"
            );
            assert!(closed(&m), "no relay left open");
            let seal = m.seal();
            relay_quietly(&mut m, |m, relay| {
                m.lift_row(one, relay);
            });
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                Vec::<usize>::new(),
                "the next relay, as the panic left it: no row"
            );
            relay_quietly(&mut m, |m, relay| {
                m.lift_row(one, relay);
                let _ = m.set_joins(one, RowJoins::default());
            });
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                vec![1],
                "and written otherwise, row 1 alone"
            );
            assert!(closed(&m), "and closed at its end");
        }

        /// **A rejoin a panic left reads every row as written**
        /// (`rejoin_quietly`, its `Mend`): a finding that writes row 1's
        /// joins and panics. A seal taken before reads every row as
        /// written, the row is quiet no longer, and a write to its joins
        /// after it is stamped. The control: the finding panicked. Named
        /// breaks: as the relay's.
        #[test]
        fn a_rejoin_a_panic_left_reads_every_row_as_written() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let one = Row::from_index(1);
            let seal = m.seal();
            let caught = catch_unwind(AssertUnwindSafe(|| {
                rejoin_quietly(&mut m, one, |m| {
                    let _ = m.set_joins(one, RowJoins::default());
                    panics("a finding that panics");
                })
            }));
            assert!(caught.is_err(), "the control: the finding panicked");
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                vec![0, 1, 2],
                "every row read as written"
            );
            assert!(closed(&m), "no rejoin left open");
            let seal = m.seal();
            let _ = m.take_joins(one);
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                vec![1],
                "after it, a write to the row's joins stamped"
            );
        }

        /// **A row lifted twice in one relay is compared with what it held
        /// first** (`Maps::lift`), as an answer to lay the window out again
        /// lifts the status line's row with every row and the status line's
        /// layout lifts it again: the status row, joined, lifted by
        /// `lift_all` and again by `lift_row`, then laid out in its own
        /// order and unjoined. It alone reads as written. The control: its
        /// joins came out none. Named break: a second lift taking what the
        /// first left.
        #[test]
        fn a_row_lifted_twice_in_a_relay_is_compared_with_what_it_held_first() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let status = Row::from_index(3);
            m.set_joins_grown(status, Some(RowJoins::default()));
            let seal = m.seal();
            relay_quietly(&mut m, |m, relay| {
                m.lift_all(Rows::of_index(3), None, Lifting::Both, relay);
                m.lift_row(status, relay);
                m.truncate(Rows::of_index(3));
                m.set_row_grown(status, None);
                m.set_joins_grown(status, None);
            });
            assert_eq!(
                m.joins().get(3),
                Some(&None),
                "the control: the status row unjoined"
            );
            assert_eq!(
                after(&m, &seal, Rows::of_index(4)),
                vec![3],
                "the status row alone"
            );
        }

        /// **A relay that lays every row out again as it was marks no row**
        /// (`Maps::lift_all`, as an answer to lay the window out again
        /// lifts it): every row lifted and the maps made again, the same
        /// rows over the same display (none), marks none; made again a row
        /// taller, the row gained alone (the control). Named break: every
        /// row stamped as the maps are made again, as a reset did.
        #[test]
        fn a_relay_of_every_row_marks_none_over_the_same_display() {
            let mut m = Maps::default();
            m.grow(Rows::of_index(3));
            let seal = m.seal();
            relay_quietly(&mut m, |m, relay| {
                m.lift_all(Rows::of_index(3), None, Lifting::Both, relay);
            });
            assert_eq!(
                after(&m, &seal, Rows::of_index(3)),
                Vec::<usize>::new(),
                "the same rows: none"
            );
            let seal = m.seal();
            relay_quietly(&mut m, |m, relay| {
                m.lift_all(Rows::of_index(4), None, Lifting::Both, relay);
            });
            assert_eq!(
                after(&m, &seal, Rows::of_index(4)),
                vec![3],
                "the control: the row gained alone"
            );
        }
    }
}

pub use maps::LaidSeal;

/// **A window in the order each row is drawn in, kept from frame to frame**
/// ([`Orders::window_kept`]), with what it was drawn under: the window's
/// physical rows, the core's seal and the layout's, the display, and its
/// size. Owned by the renderer and lent by move, the buffer and its key
/// together, so no key outlives the cells it describes. A new one holds no
/// key, so the first read copies every row: the copy a fresh layout reads.
#[derive(Default)]
pub struct DrawnRows {
    cells: Vec<PackedCell>,
    used: Vec<RowExtent>,
    key: Option<DrawnKey>,
}

/// What a [`DrawnRows`] copy was drawn under.
struct DrawnKey {
    at: WindowAt,
    seal: Seal,
    laid: LaidSeal,
    shown: Option<Rc<Display>>,
    cols: Cols,
    rows: Rows,
}

/// **The status line in its drawn order, kept** ([`Orders::status_kept`]),
/// with the core's seal, the layout's and the line's width it was drawn
/// under.
#[derive(Default)]
pub(crate) struct DrawnLine {
    cells: Vec<PackedCell>,
    key: Option<(Seal, LaidSeal, usize)>,
}

/// **The window and the status line in their drawn order**, as the cell
/// path and the CPU path's picture finder read them, each kept with its
/// key.
#[derive(Default)]
pub(crate) struct DrawnOrder {
    pub(crate) window: DrawnRows,
    pub(crate) status: DrawnLine,
}

/// Whether two layouts drew the same display, or both none: the same
/// `Rc`, never a value at a freed address.
pub(crate) fn same_shown(a: Option<&Rc<Display>>, b: Option<&Rc<Display>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// **Which logical cell each visual cell of the window shows**, for the
/// rows that are drawn in another order: what every consumer of a frame
/// maps through. Cheap to clone: a row in its own order holds nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Orders {
    /// **Each window row's order and joined cells, and the window as the
    /// display wraps it**, where a finished paragraph is drawn at its words
    /// ([`crate::rebreak`]): every row of these maps is then a display
    /// line, and a stored cell is found in it through [`Orders::shown_at`].
    ///
    /// Each row is stamped when it is written ([`Orders::laid_after`]).
    maps: maps::Maps,
    cols: Cols,
    any: Any,
    joined_any: Any,
    /// **Whether each window row may hold a syllable or a word**
    /// (`crate::syllable::may_hold`), as this layout's test of the row
    /// found it in the pass it made for its own answers, the row read from
    /// the window: [`Opens::Unknown`] for a row this layout did not ask, or
    /// asked as an input method's text or an echo writes it, which a
    /// caller then tests itself.
    syllables: Vec<Opens>,
}

impl Default for Orders {
    fn default() -> Self {
        Orders {
            maps: maps::Maps::default(),
            cols: Cols::of_index(0),
            any: Any::No,
            joined_any: Any::No,
            syllables: Vec::new(),
        }
    }
}

impl Orders {
    /// Whether any row is drawn in another order, and whether any holds a
    /// joined cell, found again over the maps: after a layout that laid
    /// some units out again and kept the rest.
    fn recount(&mut self) {
        self.any = match self.maps.rows().iter().any(Option::is_some) {
            true => Any::Yes,
            false => Any::No,
        };
        self.joined_any = match self.maps.joins().iter().any(Option::is_some) {
            true => Any::Yes,
            false => Any::No,
        };
    }

    /// Whether any row of the window is drawn in another order.
    pub fn any(&self) -> bool {
        self.any == Any::Yes
    }

    /// **Whether window row `r` may hold a syllable or a word**, as this
    /// layout's test of it found: `Unknown` where it did not ask, or read
    /// the row as something written over it.
    pub(crate) fn syllables(&self, r: Row) -> Opens {
        self.syllables.get(r.index()).copied().unwrap_or_default()
    }

    /// Row `r`'s answer `h` kept for [`Orders::syllables`], where the test
    /// read the row from the window: a row `written` covers holds another's
    /// cells.
    fn keep_syllables(&mut self, r: Row, written: &impl Over, h: Holds) {
        if written.row(r).is_none() {
            if let Some(s) = self.syllables.get_mut(r.index()) {
                *s = h.syllables;
            }
        }
    }

    /// Whether any cell of the window is drawn joined.
    pub fn any_joined(&self) -> bool {
        self.joined_any == Any::Yes
    }

    /// **The window as the display wraps it**, where a finished paragraph
    /// is drawn at its words ([`crate::rebreak`]); none where every row is
    /// drawn as stored.
    pub fn display(&self) -> Option<&Display> {
        self.maps.display().map(|d| &**d)
    }

    /// The display, held: what a caller reads the window from while it
    /// lays the renderer out again.
    pub(crate) fn display_held(&self) -> Option<Rc<Display>> {
        self.maps.display().cloned()
    }

    /// **The stored window cell logical column `l` of display row `y`
    /// shows**: the same cell where nothing is re-broken; none for a
    /// re-broken line's padding.
    pub fn stored_at(&self, y: Row, l: Col) -> Option<(Row, Col)> {
        match self.maps.display() {
            Some(d) => d.stored(y, l),
            None => Some((y, l)),
        }
    }

    /// **The display row and logical column stored cell `(r, c)` of the
    /// window is drawn at**: the same cell where nothing is re-broken; none
    /// where a re-broken paragraph pushed it out of the window.
    pub fn shown_at(&self, r: Row, c: Col) -> Option<(Row, Col)> {
        match self.maps.display() {
            Some(d) => d.shown(r, c),
            None => Some((r, c)),
        }
    }

    /// **The stored cell logical column `l` of display row `y` shows, by
    /// its line counted from the window's top**: the window's cell where
    /// nothing is re-broken; on a re-broken line, the cell it draws, of a
    /// window row or of a line past the window its paragraph runs on from
    /// or into; none for padding.
    pub fn stored_line_at(&self, y: Row, l: Col) -> Option<(WindowLine, Col)> {
        match self.maps.display() {
            Some(d) => d.stored_line(y, l),
            None => Some((WindowLine::of(y), l)),
        }
    }

    /// The display row stored row `r` of the window starts on.
    pub fn row_of(&self, r: Row) -> Option<Row> {
        match self.maps.display() {
            Some(d) => d.row_of(r),
            None => Some(r),
        }
    }

    /// **The stored cell a pointer over visual column `col` of display row
    /// `y` names**, as a mouse report and a selection read it: the logical
    /// cell under it ([`Orders::pointer_col`]), then the stored cell that
    /// shows; over a re-broken line's padding, the stored cell nearest it in
    /// the line's logical order.
    pub fn pointer(&self, y: Row, col: Col) -> (Row, Col) {
        let l = self.pointer_col(y, col);
        let Some(d) = self.maps.display() else {
            return (y, l);
        };
        if let Some(at) = d.stored(y, l) {
            return at;
        }
        let n = self.cols.index();
        (0..l.index())
            .rev()
            .chain(l.index() + 1..n)
            .find_map(|x| d.stored(y, Col::at_index(x)))
            .unwrap_or((y, l))
    }

    /// **The stored cell a selection made by a pointer over visual column
    /// `col` of display row `y` names**: [`Orders::pointer`]'s, but a cell
    /// the display draws from a line past the window is named by that line
    /// ([`Orders::stored_line_at`]), so a selection reaches every cell drawn.
    /// A mouse report keeps [`Orders::pointer`]'s window cell: a program
    /// addresses no line past the window.
    pub fn pointer_line(&self, y: Row, col: Col) -> (WindowLine, Col) {
        let l = self.pointer_col(y, col);
        let Some(d) = self.maps.display() else {
            return (WindowLine::of(y), l);
        };
        if let Some(at) = d.stored_line(y, l) {
            return at;
        }
        let n = self.cols.index();
        (0..l.index())
            .rev()
            .chain(l.index() + 1..n)
            .find_map(|x| d.stored_line(y, Col::at_index(x)))
            .unwrap_or((WindowLine::of(y), l))
    }

    /// **Whether logical column `l` of display row `y`, holding `content`,
    /// is drawn selected** by `h`, a selection kept by stored cells: the
    /// stored cell it shows asked, a line's past the window included;
    /// padding never is.
    pub fn selects(&self, h: &Highlight, y: Row, l: Col, content: Content) -> bool {
        match self.stored_line_at(y, l) {
            Some((line, c)) => h.selects_line(line, c, content),
            None => false,
        }
    }

    /// **The visual runs stored columns `from..=to` of window row `r` are
    /// drawn in**, each its display row, first visual column and width:
    /// [`Orders::runs`] where nothing is re-broken; on a re-broken
    /// paragraph, the runs of each display line its cells are drawn on.
    pub fn stored_runs(&self, r: Row, from: Col, to: Col) -> Vec<(Row, Col, Cols)> {
        self.line_runs(WindowLine::of(r), from, to)
    }

    /// [`Orders::stored_runs`] for stored line `line`, the window's or one
    /// past it: where a line past the window is drawn on a window line,
    /// the runs its cells are drawn in there; none where it is not drawn.
    pub fn line_runs(&self, line: WindowLine, from: Col, to: Col) -> Vec<(Row, Col, Cols)> {
        let Some(d) = self.maps.display() else {
            // A line past a laid-out window is drawn on none of its rows.
            let Some(r) = line
                .since(WindowLine::of(Row::ZERO))
                .map(|k| Row::from_index(k.index()))
                .filter(|r| self.maps.rows().is_empty() || r.index() < self.maps.rows().len())
            else {
                return Vec::new();
            };
            return self
                .runs(r, from, to)
                .into_iter()
                .map(|(at, n)| (r, at, n))
                .collect();
        };
        let mut shown: Vec<(Row, Col)> = (from.index()..=to.index())
            .filter_map(|c| d.shown_line(line, Col::at_index(c)))
            .collect();
        shown.sort_by_key(|&(y, x)| (y.index(), x.index()));
        let mut out = Vec::new();
        let mut i = 0;
        while i < shown.len() {
            let (y, a) = shown[i];
            let mut j = i;
            while j + 1 < shown.len()
                && shown[j + 1].0 == y
                && shown[j + 1].1.index() == shown[j].1.index() + 1
            {
                j += 1;
            }
            for (at, n) in self.runs(y, a, shown[j].1) {
                out.push((y, at, n));
            }
            i = j + 1;
        }
        out
    }

    /// What joining gives visual column `v` of row `r`: its form and its
    /// part in a lam-alef pair, or `None` for a cell drawn as it would be
    /// alone.
    pub fn joined(&self, r: Row, v: Col) -> Option<Joined> {
        self.maps
            .joins()
            .get(r.index())
            .and_then(Option::as_ref)
            .and_then(|j| j.cells.get(v.index()).copied().flatten())
    }

    /// Row `r`'s joined cells, by visual column, if it holds any.
    pub fn joins(&self, r: Row) -> Option<&[Option<Joined>]> {
        self.row_joins(r).map(RowJoins::cells)
    }

    /// Row `r`'s joined cells and the cells of their words, if it holds
    /// any: what a renderer compares from frame to frame, since a form, or
    /// a letter its shaping reads on the row a soft wrap joins, can change
    /// with no cell of the row changing.
    pub fn row_joins(&self, r: Row) -> Option<&RowJoins> {
        self.maps.joins().get(r.index()).and_then(Option::as_ref)
    }

    /// The cells of the word around the joined cell `j` of row `r`
    /// ([`RowJoins::around`]); none where the row holds no joins.
    pub fn around(&self, r: Row, j: &Joined) -> (&[WordCell], &[WordCell]) {
        self.row_joins(r).map_or((&[], &[]), |row| row.around(j))
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

    /// **Row `r`'s visual columns whose italic leans left**, left to right:
    /// a cell at a right-to-left level written under SAPV 17, "when the
    /// string direction is right-to-left, the italicized characters are
    /// slanted to the left" (`[[source: ecma48]]` 8.3.108). The cell is
    /// drawn as its upright glyph sheared left; none on a row in its own
    /// order, which is left to right.
    pub fn slants(&self, r: Row) -> &[Col] {
        self.row(r).map_or(&[], |o| o.slants.as_slice())
    }

    /// Whether visual column `v` of row `r` leans left ([`Orders::slants`]).
    pub fn slanted(&self, r: Row, v: Col) -> bool {
        self.slants(r).contains(&v)
    }

    /// Whether visual column `v` of row `r` is at a right-to-left level:
    /// an odd one after L1. None is on a row in its own order.
    pub(crate) fn right_to_left(&self, r: Row, v: Col) -> bool {
        self.row(r)
            .and_then(|o| o.level.get(v.index()))
            .is_some_and(|l| l.number() % 2 == 1)
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
        matches!(self.maps.rows().get(r.index()), Some(Some(_)))
    }

    fn row(&self, r: Row) -> Option<&RowOrder> {
        self.maps.rows().get(r.index()).and_then(Option::as_ref)
    }

    /// **The logical column each visual column of row `r` shows**, where
    /// the row is drawn in another order: none where it is drawn in its
    /// own.
    pub(crate) fn logical_cols(&self, r: Row) -> Option<&[Col]> {
        self.row(r).map(|o| &o.from[..])
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
        crate::counters::add(
            crate::counters::Counter::CellsCopied,
            crate::counters::Site::Layout,
            Tally::of(line.cells.len()),
        );
        StatusLine { cells, ..line }
    }

    /// **Where each cell of window row `y`, in its logical order, names its
    /// rendition id**, the window `back` lines into the history: a stored
    /// row through its own table (`Viewport::styles`), a display line
    /// through each run of the stored lines it shows (`crate::rebreak`).
    pub(crate) fn row_source_logical<'t>(
        &self,
        term: &'t Term<'_>,
        back: Rows,
        y: Row,
    ) -> RowSource<'t> {
        let back = term.looking_back(back);
        match self.maps.display() {
            Some(d) => RowSource::Runs(d.line_sources(term, back, WindowLine::of(y))),
            None => RowSource::One(term.view(back).styles(y)),
        }
    }

    /// **Where each cell of window row `y` names its rendition id, in the
    /// order it is drawn**: [`Orders::row_source_logical`]'s runs at the
    /// columns this row's order draws their cells at, so a copy of the
    /// window in drawn order ([`Orders::window_kept`]) is read cell for cell.
    pub fn row_source<'t>(&self, term: &'t Term<'_>, back: Rows, y: Row) -> RowSource<'t> {
        let src = self.row_source_logical(term, back, y);
        let RowSource::Runs(runs) = &src else {
            return src;
        };
        if !self.reordered(y) {
            return src;
        }
        let mut out: Vec<(core::ops::Range<usize>, baud_domain::RowStyles<'t>)> = Vec::new();
        for v in 0..self.cols.index() {
            let l = self.logical_col(y, Col::at_index(v)).index();
            let s = runs
                .iter()
                .find(|(r, _)| r.contains(&l))
                .map_or(crate::frozen::live(term), |(_, s)| *s);
            match out.last_mut() {
                Some((r, last)) if r.end == v && last.serial() == s.serial() => r.end = v + 1,
                _ => out.push((v..v + 1, s)),
            }
        }
        RowSource::Runs(out)
    }

    /// **Whether a cell of the window `back` lines up names a row the core
    /// froze** (`crate::frozen`), a display line's included: no row at all
    /// while the core has frozen nothing, which one test answers.
    pub fn window_frozen(&self, term: &Term<'_>, back: Rows) -> bool {
        let back = term.looking_back(back);
        let vp = term.view(back);
        if !vp.any_frozen() {
            return false;
        }
        match self.maps.display() {
            Some(d) => d.any_frozen(term, back),
            None => (0..vp.rows().index()).any(|y| !vp.styles(Row::from_index(y)).is_live()),
        }
    }

    /// **The seal a reader keeps beside what it drew from these maps**:
    /// every map write after it -- a row's order or joins laid out again
    /// otherwise than they were, or another display -- reads as after it
    /// ([`Orders::laid_after`]); a row laid out again as it was does not
    /// (`maps::relay_quietly`).
    pub fn laid_seal(&self) -> LaidSeal {
        self.maps.seal()
    }

    /// **Whether row `r`'s maps were written after `kept`** -- the status
    /// line's at the row after the window's -- or `kept` is another
    /// layout's: what a reader asks in place of copying a row's maps and
    /// comparing them the next frame.
    pub fn laid_after(&self, r: Row, kept: &LaidSeal) -> bool {
        self.maps.after(r, kept)
    }

    /// Whether any row's maps, or the display, were written after `kept`;
    /// one compare.
    pub fn moved_since(&self, kept: &LaidSeal) -> bool {
        self.maps.moved(kept)
    }

    /// The display the maps are laid over, as held: what a key compares by
    /// `Rc::ptr_eq`.
    pub(crate) fn display_rc(&self) -> Option<&Rc<Display>> {
        self.maps.display()
    }

    /// **The window as the cell path reads it, kept in `kept` from the
    /// last frame**: the core's own view when no row is reordered and no
    /// display held, else the window in drawn order, each row permuted
    /// again only where its maps were laid out or its cells written since
    /// the copy was made -- none on a frame where nothing was written.
    /// Every row again for another window, size or display, and into a new
    /// [`DrawnRows`], which is the fresh copy a check compares a kept one
    /// with. The only copy of the window in drawn order: a driver keeps
    /// its `kept` from frame to frame (`crate::cell_frame::build` lends
    /// the renderer's).
    pub fn window_kept<'a>(
        &self,
        term: &'a Term<'_>,
        back: Rows,
        kept: &'a mut DrawnRows,
    ) -> Viewport<'a> {
        let held = self.maps.display().cloned();
        if held.is_none() && self.any == Any::No {
            return term.view(back);
        }
        let gens = term.row_gens(back);
        let at = gens.window();
        let src = match &held {
            Some(d) => d.window(term),
            None => term.view(back),
        };
        let (nrows, ncols) = (src.rows().index(), src.cols().index());
        let whole = kept.key.as_ref().is_none_or(|k| {
            k.at != at
                || k.cols != src.cols()
                || k.rows != src.rows()
                || !same_shown(k.shown.as_ref(), held.as_ref())
        }) || kept.cells.len() != nrows * ncols
            || kept.used.len() != nrows;
        if whole {
            kept.cells.clear();
            kept.cells.resize(nrows * ncols, PackedCell::EMPTY);
            kept.used.clear();
            kept.used.resize(nrows, RowExtent::of(ncols));
        }
        let mut copied = 0;
        for y in 0..nrows {
            let r = Row::from_index(y);
            let again = whole
                || kept
                    .key
                    .as_ref()
                    .is_none_or(|k| self.laid_after(r, &k.laid) || gens.after(r, &k.seal));
            if !again {
                continue;
            }
            self.permute(r, src.row(r), &mut kept.cells[y * ncols..(y + 1) * ncols]);
            let mut e = RowExtent::of(ncols).with_rendition(src.rendition(r), src.cols());
            if src.wrapped(r) == Wrap::Continues {
                e = e.wrapping(ncols);
            }
            kept.used[y] = e;
            copied += ncols;
        }
        crate::counters::add(
            crate::counters::Counter::CellsCopied,
            crate::counters::Site::Layout,
            Tally::of(copied),
        );
        kept.key = Some(DrawnKey {
            at,
            seal: term.sealed(),
            laid: self.laid_seal(),
            shown: held.clone(),
            cols: src.cols(),
            rows: src.rows(),
        });
        let kept: &'a DrawnRows = kept;
        let out = Viewport::new_in(
            &kept.cells,
            &kept.used,
            Stride::of_cells(ncols),
            src.cols(),
            src.rows(),
            src.rows(),
            RowRot::NONE,
            crate::frozen::lines_table(
                term,
                kept.key
                    .as_ref()
                    .and_then(|k| k.shown.as_deref())
                    .map_or(&[], Display::window_bands),
            ),
        );
        match held {
            Some(_) => out,
            None => out.styled_from(&term.view(back)),
        }
    }

    /// **[`Orders::status_line`], kept in `kept` from the last frame**: the
    /// line itself while its row, the maps' row `r` after the window's, is
    /// in its own order, else its cells in drawn order, permuted again only
    /// when its maps were laid out again or its width moved. Its plane's
    /// generation (`StatusGen`) is read too, a belt with no red: every
    /// write to the status plane lays the line out again (`kept::Line`
    /// keys on it) and so stamps its row.
    pub(crate) fn status_kept<'a>(
        &self,
        term: &Term<'_>,
        r: Row,
        line: StatusLine<'a>,
        kept: &'a mut DrawnLine,
    ) -> StatusLine<'a> {
        if !self.reordered(r) {
            return line;
        }
        let n = line.cells.len();
        let again = kept.cells.len() != n
            || kept.key.as_ref().is_none_or(|(seal, laid, len)| {
                *len != n
                    || self.laid_after(r, laid)
                    || term.status_gen().is_none_or(|g| g.after(seal))
            });
        if again {
            kept.cells.clear();
            kept.cells.resize(n, PackedCell::EMPTY);
            self.permute(r, line.cells, &mut kept.cells);
            crate::counters::add(
                crate::counters::Counter::CellsCopied,
                crate::counters::Site::Layout,
                Tally::of(n),
            );
        }
        kept.key = Some((term.sealed(), self.laid_seal(), n));
        let kept: &'a DrawnLine = kept;
        StatusLine {
            cells: &kept.cells,
            ..line
        }
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
    /// The side margins the row wrapped at, where it wrapped at margins
    /// narrower than the page ([`baud_domain::WrapMargins`]).
    margins: Option<baud_domain::WrapMargins>,
    /// Where its cells name their ids (`crate::frozen`).
    words: Words<'t>,
}

/// **A run of a row's cells outside the side margins its paragraph is
/// confined to** ([`paragraph_columns`]): a paragraph of its own, its row,
/// its text, and its columns `from..to`, its text ending at `end`.
struct Alone {
    row: Row,
    text: core::ops::Range<usize>,
    from: Col,
    end: Col,
    to: Col,
}

/// **The columns of each paragraph row its paragraph holds**, `from..to`:
/// the whole row, or, where the row or the one before it wrapped at side
/// margins narrower than the page, the columns inside those margins -- the
/// row's own where it wrapped at them, else the row before's, whose wrap
/// started it at their left margin. DECSLRM "sets the left and right
/// margins to define the scrolling region" (`[[source: vt510-decslrm]]`)
/// and says nothing of the cells outside them; that they are no part of
/// the paragraph is baud's reading, which the display's word wrap keeps
/// too ([`baud_domain::WrappedAt`]). Margins a change of width left past
/// the row, and a pair one column wide or less, which DECSLRM never sets
/// -- what a change of width that re-cuts the row leaves of them -- give
/// the whole row.
fn paragraph_columns(rows: &[ParaRow<'_>]) -> Vec<(usize, usize)> {
    rows.iter()
        .enumerate()
        .map(|(k, row)| {
            let n = row.cells.len();
            let before = k.checked_sub(1).and_then(|j| rows[j].margins);
            match row.margins.or(before) {
                Some(m) if m.left().index() < m.right().index() && m.left().index() < n => {
                    (m.left().index(), (m.right().index() + 1).min(n))
                }
                _ => (0, n),
            }
        })
        .collect()
}

/// How many codepoints a row gives the algorithm.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
struct Chars(usize);

/// **Cells a layout reads in place of the core's**: an input method's
/// composition and a predicted echo, each drawn as the same text written
/// there would be drawn. The row they are on is laid out with their
/// characters in the cells they cover, so it is drawn in the order a write
/// would give it: a Latin letter typed after Latin text in a right-to-left
/// paragraph lands right of that text, which moves left to make room, as
/// the echo will draw it. Empty on nearly every frame, and then a layout
/// reads the core's cells alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    rows: Vec<(Row, Vec<PackedCell>)>,
}

impl Written {
    /// Nothing written over any row, as a constant.
    pub const NOTHING: Written = Written { rows: Vec::new() };

    /// Nothing written over any row.
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// Row `r` as written, if anything is written over it.
    pub(crate) fn cells(&self, r: Row) -> Option<&[PackedCell]> {
        Over::row(self, r)
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

    /// Each row anything is written over, into `out`: what a kept layout
    /// lays out again this frame and the next (`kept::Laid::answer`).
    fn rows_into(&self, out: &mut Vec<Row>);
}

impl Over for Written {
    fn row(&self, r: Row) -> Option<&[PackedCell]> {
        self.rows
            .iter()
            .find(|&&(w, _)| w == r)
            .map(|(_, c)| c.as_slice())
    }

    fn rows_into(&self, out: &mut Vec<Row>) {
        out.extend(self.rows.iter().map(|&(r, _)| r));
    }
}

/// Nothing written over any row.
struct Unwritten;

impl Over for Unwritten {
    #[inline(always)]
    fn row(&self, _: Row) -> Option<&[PackedCell]> {
        None
    }

    #[inline(always)]
    fn rows_into(&self, _: &mut Vec<Row>) {}
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
    /// Which slots of the rendition table name a string ([`Strung`]).
    strung: Strung,
    work: Work,
    /// **The last layout of the window, kept per paragraph** while no row
    /// it read was written (`kept::Laid`), and the status line's
    /// (`kept::Line`).
    laid: kept::Laid,
    status: kept::Line,
    /// The units a layout lays out again, and the rows written over at
    /// it: scratch, kept so a frame allocates neither.
    out: Vec<kept::Para>,
    written_now: Vec<Row>,
    /// How the joins the maps hold were found.
    last: Last,
    /// **The key a [`Rejoin::Every`] was answered under**, kept as `last`
    /// only once the caller's rejoin of every row ends
    /// ([`Reorder::rejoined`]).
    owed: Option<Last>,
    /// **Each window row, rejoined since that answer or not**
    /// ([`Reorder::rejoin`]): the key is kept only once every row was, so
    /// a caller that rejoins only the rows it saw change under that answer
    /// is answered it again. Kept across frames, so a frame allocates
    /// nothing.
    owing: Vec<Owing>,
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
    /// How many cells of its word either side of a joined cell are kept
    /// with it ([`Reorder::set_reach`]).
    reach: baud_font::bytes::Count,
    /// Whether a finished paragraph is drawn at its words, and the working
    /// state of the layout that draws it ([`crate::rebreak`]).
    wrap: WordWrap,
    breaker: Breaker,
    /// **Where the rows laid out name their cells' ids**: each its own
    /// table, or, for the window's display, each display line's runs of
    /// the stored lines it shows, `back` lines into the history.
    sourced: Sourced,
}

/// Whether a layout's rows are stored rows, or the window's display lines.
#[derive(Clone, Default)]
enum Sourced {
    #[default]
    Rows,
    Window(Rc<Display>, Rows),
}

/// Where a paragraph's context lies: in the rows around the window, read
/// from the history and the screen, or nowhere -- the status line, a
/// line of its own below the main display.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Context {
    Window,
    /// The display's own lines around the window ([`Display::window`]):
    /// as many above it and below it as it holds.
    Display(Rows, Rows),
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

/// **What a layout read to find its answers**: the rows whose write
/// generation it asked ([`baud_domain::RowGens::after`], the status line's
/// [`baud_domain::StatusGen::after`] one), the rows it tested cell by cell
/// for a character that reorders or joins, and the rows of the units it
/// laid out again -- each row of a paragraph laid again, and the status
/// line's. What a frame with no row written costs is none of the three.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Work {
    pub gens: Tally,
    pub tested: Tally,
    pub laid: Tally,
}

/// How the joins the maps hold were found: by [`Reorder::lay_out`], which
/// finds every row's, or kept from the frame before by
/// [`Reorder::keep_displayed`] for a window of this size and cluster epoch,
/// the caller rejoining the rows its compare saw change, and whether any
/// row was read as written ([`Written`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Last {
    LaidOut,
    Kept(Cols, ClusterEpoch, Rows, Changes, Any),
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

/// Whether a window row was rejoined since an answer of [`Rejoin::Every`]
/// (`Reorder::owing`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owing {
    Owed,
    Rejoined,
}

/// The layout holds the maps its rejoin writes through.
impl maps::HoldsMaps for Reorder {
    fn maps_mut(&mut self) -> &mut maps::Maps {
        &mut self.orders.maps
    }
}

/// Whether a row holds a character that can make its paragraph reorder.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) enum Opens {
    #[default]
    Unknown,
    No,
    Yes,
}

/// **Which slots of the rendition table name a string**, kept from the
/// table's log of writes: a row a layout lays out is read for its strings
/// one bit a cell, from a set that stays in the first level of cache where
/// the table itself does not, and not at all while no slot names one. A
/// row's own answers are not kept: a row is tested only when the unit it
/// is in is laid out again (`kept::Laid`), which is when a row that unit
/// read was written.
#[derive(Default)]
struct Strung {
    /// The rendition table and its count of writes `string_slots` were
    /// found against ([`Strung::styles_moved`]).
    styles: Option<(TableSerial, StyleWrites)>,
    string_slots: Vec<u64>,
    string_slots_any: Opens,
}

/// What a row holds that its layout reads: whether it can make its
/// paragraph reorder, and whether it holds a letter that takes a joining
/// form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Holds {
    opens: Opens,
    joins: Opens,
    /// Whether it holds a cell written in an SDS or SRS string: `No`
    /// until one has been begun ([`Term::strings_seen`]).
    strung: Opens,
    /// Whether it may hold a syllable or a word
    /// (`crate::syllable::may_hold`), from the same pass.
    syllables: Opens,
}

impl Holds {
    const NOTHING: Holds = Holds {
        opens: Opens::No,
        joins: Opens::No,
        strung: Opens::No,
        syllables: Opens::No,
    };
}

impl Strung {
    /// **The slots that name a string, brought up to the table**: each
    /// slot written is read again, or, for another table or more writes
    /// than the log holds, every slot. Nothing while no string has been
    /// begun. Once a frame that lays a row out, before the rows are read;
    /// a slot written since names a string only through a cell written
    /// with it, which stamps that cell's row.
    fn styles_moved(&mut self, term: &Term<'_>) {
        if !term.strings_seen() {
            self.styles = None;
            return;
        }
        let now = (term.style_table(), term.style_writes());
        let slots = term.styles().one().slots();
        let names = |k: usize| {
            slots
                .get(k)
                .and_then(|s| s.word())
                .is_some_and(|w| !w.string().is_none())
        };
        let replay = match self.styles {
            Some((table, at)) if table == now.0 => {
                if at == now.1 {
                    return;
                }
                term.styles_written(at)
            }
            _ => baud_core::Written::Every,
        };
        match replay {
            baud_core::Written::Slots(parts) => {
                for &k in parts.iter().flat_map(|p| p.iter()) {
                    let on = match names(k.index()) {
                        true => Opens::Yes,
                        false => Opens::No,
                    };
                    self.mark_slot(k, on);
                }
            }
            baud_core::Written::Every => {
                self.string_slots.clear();
                self.string_slots.resize(slots.len().div_ceil(64), 0);
                for k in (0..slots.len()).filter(|&k| names(k)) {
                    self.mark_slot(StyleId::from_index(k), Opens::Yes);
                }
            }
        }
        self.string_slots_any = match self.string_slots.iter().any(|&w| w != 0) {
            true => Opens::Yes,
            false => Opens::No,
        };
        self.styles = Some(now);
    }

    /// Slot `k` naming a string or not, in [`Strung::string_slots`].
    fn mark_slot(&mut self, k: StyleId, names: Opens) {
        let (w, b) = (k.index() / 64, 1u64 << (k.index() % 64));
        if self.string_slots.len() <= w {
            self.string_slots.resize(w + 1, 0);
        }
        match names {
            Opens::Yes => self.string_slots[w] |= b,
            _ => self.string_slots[w] &= !b,
        }
    }

    /// **Whether a row holds a cell written in an SDS or SRS string**, by
    /// [`Strung::string_slots`]: none until one has been begun, and none
    /// while no slot names one, so a session that never sends one reads no
    /// cell for it.
    fn row_strung(&self, term: &Term<'_>, words: Words<'_>, cells: &[PackedCell]) -> Opens {
        if !term.strings_seen() {
            return Opens::No;
        }
        // A frozen row's ids name its generation's words, which the slots'
        // bits do not cover: its cells read one by one.
        if !words.is_live() {
            return match cells
                .iter()
                .enumerate()
                .any(|(x, &c)| string_of(term, words, Col::at_index(x), c) != StringId::NONE)
            {
                true => Opens::Yes,
                false => Opens::No,
            };
        }
        if self.string_slots_any != Opens::Yes {
            return Opens::No;
        }
        let named = |c: &PackedCell| {
            let k = c.style.index();
            self.string_slots
                .get(k / 64)
                .is_some_and(|w| w & (1u64 << (k % 64)) != 0)
        };
        match cells.iter().any(named) {
            true => Opens::Yes,
            false => Opens::No,
        }
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
            strung: Strung::default(),
            work: Work::default(),
            laid: kept::Laid::default(),
            status: kept::Line::default(),
            out: Vec::new(),
            written_now: Vec::new(),
            last: Last::LaidOut,
            owed: None,
            owing: Vec::new(),
            bidi: Box::new(Bidi::new()),
            text: Vec::new(),
            owner: Vec::new(),
            order: Vec::new(),
            rule: Vec::new(),
            piece: Vec::new(),
            levels: Vec::new(),
            paths: Vec::new(),
            reach: baud_font::bytes::Count::ZERO,
            wrap: WordWrap::default(),
            breaker: Breaker::default(),
            sourced: Sourced::Rows,
        }
    }

    /// **Whether a finished paragraph is drawn at its words**
    /// ([`WordWrap`]): the layouts after this follow it.
    pub fn set_word_wrap(&mut self, wrap: WordWrap) {
        self.wrap = wrap;
    }

    /// **The display of the window `back` lines up as the next layout will
    /// draw it** ([`crate::rebreak`]), for a reader that runs before that
    /// layout: kept from the last frame where nothing it read changed.
    pub fn display_now(&mut self, term: &Term<'_>, back: Rows) -> Option<Rc<Display>> {
        crate::rebreak::lay_out(term, term.looking_back(back), self.wrap, &mut self.breaker)
    }

    /// The window `back` lines up as the last layout drew it: the display's
    /// lines where it re-broke a paragraph, else the core's own view.
    pub fn view<'a>(&'a self, term: &'a Term<'_>, back: Rows) -> Viewport<'a> {
        match self.orders.maps.display() {
            Some(d) => d.window(term),
            None => term.view(term.looking_back(back)),
        }
    }

    /// **How many cells of its word either side of a joined cell its
    /// shaping reads**: a renderer's widest [`baud_font::joined_reach`]
    /// among its faces, so every face's contextual lookups across letters
    /// meet the letters they read. The layouts after this keep that many
    /// ([`Joined::around`]); none, the default, keeps none.
    pub fn set_reach(&mut self, reach: baud_font::bytes::Count) {
        self.reach = reach;
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
    /// [`Term::view`] draws it, each paragraph kept from the last layout
    /// while no row it read was written (`kept::Laid::answer`). Nothing is reordered while BDSM is
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

    // Inlined into each caller, so its `extent` is a constant there. One
    // relay around the window's layout, its variants and the status line's
    // (`maps::relay_quietly`): each row laid out again is stamped only
    // where its maps came out otherwise.
    #[inline(always)]
    fn lay_out_over(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        joins: Joins,
        written: &impl Over,
        extent: Extent,
    ) {
        maps::relay_quietly(self, |o, relay| {
            o.lay_out_relayed(term, back, joins, written, extent, relay);
        });
    }

    /// [`Reorder::lay_out_over`] inside its relay.
    #[inline(always)]
    fn lay_out_relayed(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        joins: Joins,
        written: &impl Over,
        extent: Extent,
        relay: &maps::Relay,
    ) {
        let (answer, nrows) = self.lay_out_window(term, back, joins, written, extent, relay);
        // SAPV's variants on the rows laid out again alone: a kept row
        // holds its own already, and they are not made twice.
        if answer != kept::Answer::Holds && term.variants_seen() {
            let held = self.orders.maps.display().cloned();
            let vp = match &held {
                Some(d) => d.window(term),
                None => term.view(term.looking_back(back)),
            };
            let n = match extent {
                Extent::Window => vp.rows(),
                Extent::Top => Rows::of_index(vp.rows().index().min(1)),
            };
            let units = core::mem::take(&mut self.out);
            self.vary_rows(term, &vp, n, &units, written);
            self.out = units;
        }
        if extent == Extent::Window {
            self.lay_out_status(term, joins, nrows, relay);
        }
    }

    /// Row `cells`, its ids named in `words`, tested once for what its
    /// layout reads.
    fn test_row(&mut self, term: &Term<'_>, words: Words<'_>, cells: &[PackedCell]) -> Holds {
        self.work.tested.bump();
        Holds {
            strung: self.strung.row_strung(term, words, cells),
            ..row_holds(term, cells)
        }
    }

    /// **The status line, laid out as a line of its own**: one row, so
    /// one paragraph, under the terminal's BDSM and the line properties
    /// SCP and `?2501` gave it while DECSASD selected it
    /// ([`Term::status_view`]) -- ECMA-48's IMPLICIT, "All
    /// bi-directional aspects of data are handled by the device itself"
    /// (`[[source: ecma48]]` 7.2.1), on the one line the main display's
    /// rows do not hold. Its order is the maps' row after the window's
    /// `nrows`, where every draw path reads the status line's row. Nothing
    /// while no status line is shown. Kept while its plane was not written
    /// since it was laid out (`kept::Line`).
    fn lay_out_status(
        &mut self,
        term: &Term<'_>,
        joins: Joins,
        nrows: Rows,
        relay: &maps::Relay,
    ) -> kept::Answer {
        let Some(line) = term.status_view() else {
            // A status line let go takes its order with it.
            let gone = self.orders.maps.rows().len() > nrows.index()
                || self.orders.maps.joins().len() > nrows.index();
            self.orders.maps.truncate(nrows);
            self.status.forget();
            return match gone {
                true => kept::Answer::Again,
                false => kept::Answer::Holds,
            };
        };
        let key = kept::LineKey {
            at: nrows,
            cols: line.cols(),
            support: term.bidi_support(),
            joins: kept::read_joins(term, joins),
            reach: self.reach,
            epoch: term.cluster_epoch(),
            strings: kept::Begun::strings(term),
            variants: kept::Begun::variants(term),
        };
        let (answer, read) = self.status.answer(term, &key);
        crate::counters::add(
            crate::counters::Counter::GensRead,
            crate::counters::Site::Layout,
            read,
        );
        self.work.gens.add(read);
        if answer == kept::Answer::Holds {
            return answer;
        }
        // Its row of the maps lifted, so a line laid out again as it was
        // marks nothing.
        self.orders
            .maps
            .lift_row(Row::from_index(nrows.index()), relay);
        self.orders.maps.truncate(nrows);
        // Its row is the status line's own, read through the live table.
        let sourced = core::mem::take(&mut self.sourced);
        self.lay_out_status_row(term, joins, &line);
        self.sourced = sourced;
        self.work.laid.bump();
        self.status.settle(term, key);
        answer
    }

    fn lay_out_status_row<'t>(&mut self, term: &'t Term<'_>, joins: Joins, line: &Viewport<'t>) {
        let at = self.orders.maps.rows().len();
        let main = core::mem::take(&mut self.orders);
        self.strung.styles_moved(term);
        self.orders.cols = line.cols();
        self.orders.maps.grow(Rows::of_index(1));
        match term.bidi_support() {
            BidiSupport::Explicit => {
                self.reverse_paths(term, line, &Unwritten);
                if joins == Joins::Seen
                    && self.test_row(term, Words::LIVE, line.row(Row::ZERO)).joins == Opens::Yes
                {
                    self.join_shown(term, (Row::ZERO, Words::LIVE), line.row(Row::ZERO));
                }
            }
            BidiSupport::Implicit => {
                self.paragraph(
                    term,
                    Rows::ZERO,
                    line,
                    &Unwritten,
                    (Row::ZERO, Row::ZERO),
                    Context::None,
                );
            }
        }
        if term.variants_seen() {
            let one = [kept::Para::one(Row::ZERO)];
            self.vary_rows(term, line, line.rows(), &one, &Unwritten);
        }
        let status = core::mem::replace(&mut self.orders, main);
        let Some((row, joined)) = status.maps.into_first() else {
            return;
        };
        if row.is_some() {
            self.orders.any = Any::Yes;
        }
        if joined.is_some() {
            self.orders.joined_any = Any::Yes;
        }
        self.orders.maps.set_row_grown(Row::from_index(at), row);
        self.orders
            .maps
            .set_joins_grown(Row::from_index(at), joined);
    }

    /// **Where row `y` of the view laid out names its cells' ids**, `y`
    /// counted from the window's top, negative above it: a stored row
    /// through `vp`'s own table for it (`Viewport::styles`), a display line
    /// of the window through each run's.
    fn row_source<'v>(
        &self,
        term: &'v Term<'_>,
        vp: &Viewport<'v>,
        y: WindowLine,
    ) -> RowSource<'v> {
        match &self.sourced {
            Sourced::Window(d, back) => RowSource::Runs(d.line_sources(term, *back, y)),
            Sourced::Rows => {
                let top = WindowLine::of(Row::ZERO);
                match y.since(top) {
                    Some(down) => RowSource::One(vp.styles(Row::from_index(down.index()))),
                    None => {
                        let up = top.since(y).map_or(usize::MAX, Rows::index);
                        RowSource::One(vp.back(up).styles(Row::ZERO))
                    }
                }
            }
        }
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
        relay: &maps::Relay,
    ) -> (kept::Answer, Rows) {
        let back = term.looking_back(back);
        // **The display's own wrap** of the finished paragraphs in the
        // window, where it re-breaks one: every row below is then one of its
        // lines, read with the lines around the window it holds.
        let held = crate::rebreak::lay_out(term, back, self.wrap, &mut self.breaker);
        self.sourced = match &held {
            Some(d) => Sourced::Window(d.clone(), back),
            None => Sourced::Rows,
        };
        self.lay_out_held(term, back, held, joins, written, extent, relay)
    }

    /// **Lays out `d`, a display of one paragraph alone**
    /// ([`crate::rebreak::paragraph_display`]), as the window's layout lays
    /// out a display: each of its lines in the order it is drawn in. What a
    /// copy in drawn order reads a re-broken paragraph's lines by (`?96`).
    /// Laid out whole each time: nothing of another layout is kept for it.
    pub(crate) fn lay_out_display(&mut self, term: &Term<'_>, d: Rc<Display>) {
        self.sourced = Sourced::Rows;
        self.laid.forget();
        maps::relay_quietly(self, |o, relay| {
            o.lay_out_held(
                term,
                Rows::ZERO,
                Some(d),
                Joins::Unseen,
                &Unwritten,
                Extent::Window,
                relay,
            )
        });
    }

    /// [`Reorder::lay_out_window`] past its display: the window's rows, or
    /// `held`'s lines where it is one. **Each unit the last layout laid out
    /// is kept while no row it read was written** (`kept::Laid::answer`):
    /// a paragraph, or with BDSM reset a row. A unit is laid out whole, its
    /// maps cleared first, so no row of a paragraph is ever laid out alone.
    /// Answers what was kept, and the window's height.
    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn lay_out_held(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        held: Option<Rc<Display>>,
        joins: Joins,
        written: &impl Over,
        extent: Extent,
        relay: &maps::Relay,
    ) -> (kept::Answer, Rows) {
        let (vp, around) = match &held {
            Some(d) => {
                let (up, down) = d.around();
                (d.window(term), Context::Display(up, down))
            }
            None => (term.view(back), Context::Window),
        };
        let nrows = vp.rows().index();
        self.work = Work::default();
        self.last = Last::LaidOut;
        self.owed = None;
        let explicit = term.bidi_support() == BidiSupport::Explicit;
        let key = kept::Key {
            pass: kept::Pass::Window(extent),
            at: term.row_gens(back).window(),
            back,
            rows: vp.rows(),
            cols: vp.cols(),
            support: term.bidi_support(),
            joins: kept::read_joins(term, joins),
            reach: self.reach,
            epoch: term.cluster_epoch(),
            strings: kept::Begun::strings(term),
            variants: kept::Begun::variants(term),
            shown: match &held {
                Some(d) => kept::Shown::Display(d.clone()),
                None => kept::Shown::Stored,
            },
        };
        let cut = match (explicit, extent) {
            (true, _) => kept::Cut::Rows,
            (false, Extent::Window) => kept::Cut::Paragraphs,
            (false, Extent::Top) => kept::Cut::First,
        };
        let answer = self.ask(term, &key, &vp, written, cut);
        match answer {
            kept::Answer::Holds => return (answer, vp.rows()),
            kept::Answer::Again => {
                self.orders
                    .maps
                    .lift_all(vp.rows(), held.clone(), maps::Lifting::Both, relay);
                self.orders.cols = vp.cols();
                self.orders.any = Any::No;
                self.orders.syllables.clear();
                self.orders.syllables.resize(nrows, Opens::Unknown);
                self.forget_joins(vp.rows());
                self.status.forget();
                kept::cut(&vp, cut, &mut self.out);
            }
            kept::Answer::Units => {
                // Each row of a unit laid out again lets go of its maps,
                // lifted out and compared at the relay's end: a row that
                // comes out in the order and with the joins it held, as it
                // was, reads as not laid out again.
                for y in self.out.iter().flat_map(kept::Para::rows) {
                    self.orders.maps.lift_row(Row::from_index(y), relay);
                    if let Some(s) = self.orders.syllables.get_mut(y) {
                        *s = Opens::Unknown;
                    }
                }
            }
        }
        self.strung.styles_moved(term);
        let mut units = core::mem::take(&mut self.out);
        let row_of = |r: Row| written.row(r).unwrap_or_else(|| vp.row(r));
        let n = match extent {
            Extent::Window => nrows,
            Extent::Top => nrows.min(1),
        };
        for u in units.iter_mut() {
            self.work.laid.add(Tally::of(u.rows().len()));
            if explicit {
                let r = u.first;
                self.reverse_path(term, &vp, written, r);
                if joins == Joins::Unseen || r.index() >= n {
                    continue;
                }
                let src = self.row_source(term, &vp, WindowLine::of(r));
                let h = self.test_row(term, src.words(), row_of(r));
                self.orders.keep_syllables(r, written, h);
                if h.joins == Opens::Yes {
                    self.join_shown(term, (r, src.words()), row_of(r));
                }
                continue;
            }
            u.reach = self.paragraph(term, back, &vp, written, (u.first, u.last), around);
        }
        if answer == kept::Answer::Units {
            self.orders.recount();
        }
        self.laid
            .settle(term, &key, &units, &self.written_now, answer);
        self.out = units;
        (answer, vp.rows())
    }

    /// **What the last layout keeps for `key`**, its generations read and
    /// counted, the units to lay out again left in `out`
    /// (`kept::Laid::answer`).
    fn ask(
        &mut self,
        term: &Term<'_>,
        key: &kept::Key,
        vp: &Viewport<'_>,
        written: &dyn Over,
        cut: kept::Cut,
    ) -> kept::Answer {
        self.written_now.clear();
        written.rows_into(&mut self.written_now);
        let (answer, read) = self
            .laid
            .answer(term, key, vp, &self.written_now, cut, &mut self.out);
        crate::counters::add(
            crate::counters::Counter::GensRead,
            crate::counters::Site::Layout,
            read,
        );
        self.work.gens.add(read);
        answer
    }

    /// **Lays out the window with BDSM reset for a caller that knows which
    /// rows changed**: nothing is reordered, and each row's joins, which
    /// with the mode reset are its own (one row, one run), are kept from the
    /// frame before -- [`Rejoin::Changed`] -- for the caller to find again
    /// by [`Reorder::rejoin`] on each row that changed, then
    /// [`Reorder::rejoined`]. [`Rejoin::Every`] when there is no frame
    /// before to keep: the last was laid out otherwise, or at another width,
    /// height or cluster epoch, or kept for the other compare (`changes`),
    /// or the last answer was [`Rejoin::Every`] and its rejoin did not end
    /// in [`Reorder::rejoined`] with every window row rejoined. The caller
    /// then finds every row's joins again, each against those it held, and
    /// the status line's are its own layout's. A row that did not change is neither compared nor tested here, and
    /// each row's order is kept while the row was not written
    /// (`kept::Laid::answer`).
    ///
    /// Each row `written` covers is read as written, so a composition on a
    /// line drawn in reverse is mirrored as its text written will be; on a
    /// frame that reads one, or follows one that did, the row's order may
    /// have moved under unchanged cells, and the answer is
    /// [`Rejoin::Every`].
    pub fn keep_displayed(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        changes: Changes,
        written: &Written,
    ) -> Rejoin {
        maps::relay_quietly(self, |o, relay| {
            o.keep_relayed(term, back, changes, written, relay)
        })
    }

    /// [`Reorder::keep_displayed`] inside its relay
    /// (`maps::relay_quietly`): each row whose order it lays out again is
    /// stamped only where it came out otherwise.
    fn keep_relayed(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        changes: Changes,
        written: &Written,
        relay: &maps::Relay,
    ) -> Rejoin {
        let back = term.looking_back(back);
        let vp = term.view(back);
        let nrows = vp.rows().index();
        self.sourced = Sourced::Rows;
        self.work = Work::default();
        self.owed = None;
        // No row is tested here: a caller tests each row for its syllables.
        self.orders.syllables.clear();
        let key = kept::Key {
            pass: kept::Pass::Kept(changes),
            at: term.row_gens(back).window(),
            back,
            rows: vp.rows(),
            cols: vp.cols(),
            support: term.bidi_support(),
            joins: Joins::Seen,
            reach: self.reach,
            epoch: term.cluster_epoch(),
            strings: kept::Begun::strings(term),
            variants: kept::Begun::variants(term),
            shown: kept::Shown::Stored,
        };
        let answer = self.ask(term, &key, &vp, written, kept::Cut::Rows);
        match answer {
            kept::Answer::Holds => {}
            kept::Answer::Again => {
                self.orders
                    .maps
                    .lift_all(vp.rows(), None, maps::Lifting::Order, relay);
                self.orders.cols = vp.cols();
                self.orders.any = Any::No;
                // The status line's joins, after the window's, are found
                // again below whatever the window keeps.
                self.orders.maps.truncate_joins(vp.rows());
                self.status.forget();
                kept::cut(&vp, kept::Cut::Rows, &mut self.out);
            }
            kept::Answer::Units => {
                for y in self.out.iter().flat_map(kept::Para::rows) {
                    self.orders.maps.lift_order(Row::from_index(y), relay);
                }
            }
        }
        if answer != kept::Answer::Holds {
            self.strung.styles_moved(term);
            let units = core::mem::take(&mut self.out);
            for u in &units {
                self.work.laid.bump();
                self.reverse_path(term, &vp, written, u.first);
            }
            // SAPV's substitutions and slants, which live in the orders
            // made again above; its forms live in the joins, kept or found
            // again by `rejoin`.
            if term.variants_seen() {
                self.vary_kept(term, &vp, &units, written);
            }
            if answer == kept::Answer::Units {
                self.orders.recount();
            }
            self.laid
                .settle(term, &key, &units, &self.written_now, answer);
            self.out = units;
        }
        let read = match written.is_empty() {
            true => Any::No,
            false => Any::Yes,
        };
        let now = Last::Kept(vp.cols(), term.cluster_epoch(), vp.rows(), changes, read);
        // A row whose path changed is shown in another order with the same
        // cells, which no caller's compare sees: its joins are found again.
        let paths = (0..nrows).map(|y| vp.bidi(Row::from_index(y)));
        let same_paths = self.paths.len() == nrows && self.paths.iter().copied().eq(paths.clone());
        self.paths.clear();
        self.paths.extend(paths);
        let rejoin = match self.last == now && same_paths && read == Any::No {
            true => Rejoin::Changed,
            false => {
                // Every row's joins are found again by the caller, each
                // against the joins it held ([`Reorder::rejoin`]): left in
                // place, so a row whose joins come out the same is not
                // read as laid out again. The status line's, after the
                // window's, are its own layout's (`kept::Line`) and stay.
                self.orders.maps.grow_joins(vp.rows());
                // **Owed until that rejoin ends** ([`Reorder::rejoined`])
                // with every window row rejoined: a caller that returns
                // before it, or rejoins only some rows, is answered this
                // again, so no row keeps joins found under another key.
                self.last = Last::LaidOut;
                self.owed = Some(now);
                self.owing.clear();
                self.owing.resize(nrows, Owing::Owed);
                Rejoin::Every
            }
        };
        self.lay_out_status(term, Joins::Seen, vp.rows(), relay);
        rejoin
    }

    /// Row `r` of the window `vp` joined again, with BDSM reset, after
    /// [`Reorder::keep_displayed`]: one test of the row, and the row joined
    /// in display order if it holds a joining letter -- the row as
    /// `written` covers it ([`Written`]), so a composition joins, and takes
    /// SAPV's forms, as its text written will, and the letters beside it
    /// join it.
    ///
    /// The row's joins are moved out first and compared with those found
    /// again, and the row reads as laid out again
    /// ([`Orders::laid_after`]) only when they differ: one call around the
    /// finding (`maps::rejoin_quietly`), so nothing between the two can
    /// leave the row's later joins writes unstamped. Once it ends the row
    /// counts toward an answer of [`Rejoin::Every`] ([`Reorder::rejoined`]).
    pub fn rejoin(&mut self, term: &Term<'_>, vp: &Viewport<'_>, r: Row, written: &Written) {
        let found = maps::rejoin_quietly(self, r, |o| {
            o.work.tested.bump();
            let cells = written.row(r).unwrap_or_else(|| vp.row(r));
            if row_joins(term, cells) {
                o.join_shown(term, (r, Words::Row(vp.styles(r))), cells);
            }
            if term.variants_seen() {
                o.vary_joins(term, vp, r, written);
            }
        });
        if let Some(o) = found.and(self.owing.get_mut(r.index())) {
            *o = Owing::Rejoined;
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
    /// of the rows that hold one -- as written ([`Written`]), so a
    /// composition or an echo is mirrored as its text written will be.
    /// Those rows by a trait object, not a type parameter: a second copy
    /// of this function moved what the layout inlines around the
    /// algorithm, and btop's frames retired 0.2% more instructions with no
    /// row drawn in reverse.
    fn reverse_paths(&mut self, term: &Term<'_>, vp: &Viewport<'_>, written: &dyn Over) {
        for y in 0..vp.rows().index() {
            self.reverse_path(term, vp, written, Row::from_index(y));
        }
    }

    /// [`Reorder::reverse_paths`] on row `r` alone: with BDSM reset each
    /// row is a unit of its own, laid out again only when it was written.
    fn reverse_path(&mut self, term: &Term<'_>, vp: &Viewport<'_>, written: &dyn Over, r: Row) {
        let cols = vp.cols().index();
        let strings = term.strings_seen();
        let entry = |id: StringId| term.string_entry(id);
        let line = vp.bidi(r);
        // A row DECRLM wrote is drawn as its cells hold.
        if line.written() == RowWritten::RightToLeft {
            return;
        }
        let cells = written.row(r).unwrap_or_else(|| vp.row(r));
        let words = Words::Row(vp.styles(r));
        // One bit a cell, from the slots that name a string.
        let strung = strings && self.strung.row_strung(term, words, cells) == Opens::Yes;
        let base = match line.path() {
            CharacterPath::RightToLeft => BidiDirection::Rtl,
            _ => BidiDirection::Ltr,
        };
        if base == BidiDirection::Ltr && !strung {
            return;
        }
        let width = vp.rendition(r).end_of_line(vp.cols()).index().min(cols);
        // **TR/53's order**: the row reversed on a right-to-left path,
        // and each SDS or SRS string in its own direction inside it
        // ([`baud_domain::order_strings`]).
        let ids: Vec<StringId> = match strung {
            true => cells
                .iter()
                .enumerate()
                .map(|(x, &c)| string_of(term, words, Col::at_index(x), c))
                .collect(),
            false => vec![StringId::NONE; cells.len()],
        };
        let mut from = vec![0u16; cols];
        let mut runs = vec![BidiDirection::Ltr; cols];
        let tail = |c: usize| cells.get(c).is_some_and(|x| x.content.is_wide_tail());
        order_strings(&ids, tail, width, base, &entry, &mut from, &mut runs);
        if base == BidiDirection::Ltr
            && runs.iter().all(|&d| d == BidiDirection::Ltr)
            && from.iter().enumerate().all(|(v, &l)| usize::from(l) == v)
        {
            return;
        }
        let level = |d: BidiDirection| match d {
            BidiDirection::Rtl => BidiLevel::RTL,
            BidiDirection::Ltr => BidiLevel::LTR,
        };
        let mut out = RowOrder {
            from: from
                .iter()
                .map(|&l| Col::at_index(usize::from(l)))
                .collect(),
            to: vec![Col::ZERO; cols],
            level: from.iter().map(|&l| level(runs[usize::from(l)])).collect(),
            swaps: Vec::new(),
            mirrors: Vec::new(),
            slants: Vec::new(),
            drawn: Cols::of_index(width),
        };
        for (v, l) in out.from.iter().enumerate() {
            let v = Col::at_index(v);
            if let Some(slot) = out.to.get_mut(l.index()) {
                *slot = v;
            }
            let cell = cells[l.index()];
            if cell.content.is_wide_tail() || runs[l.index()] == BidiDirection::Ltr {
                continue;
            }
            let span = crate::cellmap::CellSpan::at(cells, *l);
            let m = match unmirrored(term, words, *l, cell) {
                true => Mirror::Not,
                false => mirror(term, cell, (line.box_mirror(), span)),
            };
            match m {
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
        if self.orders.maps.set_row(r, out) {
            self.orders.any = Any::Yes;
        }
    }

    /// Row `r`, `cells` as stored, joined in the order it is shown with
    /// BDSM reset: as stored, or reversed on a right-to-left line, whose
    /// joins are then found over the cells as [`Reorder::reverse_paths`]
    /// lays them out, by the visual column every draw path reads.
    fn join_shown(&mut self, term: &Term<'_>, (r, words): (Row, Words<'_>), cells: &[PackedCell]) {
        crate::counters::add(
            crate::counters::Counter::BidiRowsLaid,
            crate::counters::Site::Layout,
            Tally::of(1),
        );
        if !self.orders.reordered(r) {
            return self.join_displayed(term, (r, words), cells);
        }
        let mut shown = vec![PackedCell::EMPTY; cells.len()];
        self.orders.permute(r, cells, &mut shown);
        self.join_displayed(term, (r, words), &shown);
    }

    /// **The rows `first..=last` of a paragraph DECRLM wrote, joined as
    /// shown**, read from the right as with BDSM reset: the order DECRLM
    /// wrote them in. Out of line, so the paragraph's own layout keeps its
    /// shape: inlined, the frames of btop, cat-cjk, vim and less-cjk retired
    /// 0.08-0.13% more instructions, none with a row DECRLM wrote.
    #[cold]
    #[inline(never)]
    fn join_leftward(
        &mut self,
        term: &Term<'_>,
        vp: &Viewport<'_>,
        written: &impl Over,
        (first, last): (Row, Row),
    ) {
        for y in first.index()..=last.index() {
            let r = Row::from_index(y);
            let cells = written.row(r).unwrap_or_else(|| vp.row(r));
            let src = self.row_source(term, vp, WindowLine::of(Row::from_index(y)));
            if self.test_row(term, src.words(), cells).joins == Opens::Yes {
                self.join_shown(term, (r, src.words()), cells);
            }
        }
    }

    /// No row of a window `rows` high holds a joined cell: the maps
    /// emptied only where the last layout left one, or made at a new
    /// height, so a window that never held one costs one test.
    fn forget_joins(&mut self, rows: Rows) {
        let nrows = rows.index();
        if self.orders.joined_any == Any::Yes || self.orders.maps.joins().len() != nrows {
            self.orders.maps.clear_joins(rows);
            self.orders.joined_any = Any::No;
        }
    }

    /// **The rejoining after [`Reorder::keep_displayed`] done**: whether
    /// any row holds a joined cell, found again, and after
    /// [`Rejoin::Every`] the key it was answered under kept, so the next
    /// frame keeps these joins -- only when every window row was rejoined
    /// since that answer ([`Reorder::rejoin`]). Until then, or if a row was
    /// not, the next frame is answered [`Rejoin::Every`] again.
    pub fn rejoined(&mut self) {
        if let Some(now) = self
            .owed
            .take()
            .filter(|_| self.owing.iter().all(|o| *o == Owing::Rejoined))
        {
            self.last = now;
        }
        self.orders.joined_any = match self.orders.maps.joins().iter().any(Option::is_some) {
            true => Any::Yes,
            false => Any::No,
        };
    }

    /// One paragraph whose drawn rows are `first..=last`: resolved, and each
    /// of those rows ordered, if any of its rows can reorder. Answers how
    /// many stored rows above and below the window its context read, the
    /// row that ended each walk included: what keeps it (`kept::Reach`).
    fn paragraph<'v>(
        &mut self,
        term: &'v Term<'_>,
        back: Rows,
        vp: &Viewport<'v>,
        written: &impl Over,
        (first, last): (Row, Row),
        context: Context,
    ) -> kept::Reach {
        let row_of = |r: Row| written.row(r).unwrap_or_else(|| vp.row(r));
        let nrows = vp.rows().index();
        // **A paragraph a row of which DECRLM wrote is drawn as its cells
        // hold** (`baud_domain::RowWritten`): its cells hold the order a
        // VT510 shows, which UAX #9 would reverse. Its rows join as shown,
        // read from the right, as with BDSM reset: the order DECRLM wrote
        // them in.
        if (first.index()..=last.index())
            .any(|y| vp.bidi(Row::from_index(y)).written() == RowWritten::RightToLeft)
        {
            self.join_leftward(term, vp, written, (first, last));
            return kept::Reach::NONE;
        }
        let budget = CONTEXT / vp.cols().index().max(1);
        let opens = |cells: &[PackedCell]| row_opens(term, cells);
        // Every drawn row is asked, so each keeps its answer for the next
        // frame.
        let mut drawn_opens = false;
        let mut drawn_joins = false;
        let mut drawn_strung = false;
        // Where each drawn row names its cells' ids (`crate::frozen`).
        let sources: Vec<RowSource<'v>> = (first.index()..=last.index())
            .map(|y| self.row_source(term, vp, WindowLine::of(Row::from_index(y))))
            .collect();
        for y in first.index()..=last.index() {
            let r = Row::from_index(y);
            let words = sources[y - first.index()].words();
            let h = self.test_row(term, words, row_of(r));
            self.orders.keep_syllables(r, written, h);
            drawn_opens |= h.opens == Opens::Yes;
            drawn_joins |= h.joins == Opens::Yes;
            drawn_strung |= h.strung == Opens::Yes;
        }
        // The rows above the window the paragraph starts in, and below it
        // when the window looks back, nearest first: context, read only
        // when the paragraph reaches the window's edge -- so a paragraph
        // inside the window that holds no right-to-left character costs
        // its rows' test and nothing else.
        type ContextRows<'c> = Vec<(
            &'c [PackedCell],
            Option<baud_domain::WrapMargins>,
            RowSource<'c>,
        )>;
        let mut above: ContextRows<'_> = Vec::new();
        if let (0, Context::Display(up, _)) = (first.index(), context) {
            let mut k = 1;
            while k <= up.index() && above.len() < budget {
                let v = vp.back(k);
                if v.wrapped(Row::ZERO) != Wrap::Continues {
                    break;
                }
                let src =
                    self.row_source(term, vp, WindowLine::of(Row::ZERO).up(Rows::of_count(k)));
                above.push((v.row(Row::ZERO), v.margins(Row::ZERO), src));
                k += 1;
            }
        }
        // The stored rows the walks read, for what keeps this layout.
        let mut reach = kept::Reach::NONE;
        if first.index() == 0 && context == Context::Window {
            let mut k = 1;
            while above.len() < budget {
                let want = Rows::of_index(back.index() + k);
                if term.looking_back(want) != want {
                    break;
                }
                let v = term.view(want);
                reach.up = Rows::of_count(k);
                if v.wrapped(Row::ZERO) != Wrap::Continues {
                    break;
                }
                let src = RowSource::One(v.styles(Row::ZERO));
                above.push((v.row(Row::ZERO), v.margins(Row::ZERO), src));
                k += 1;
            }
        }
        let mut below: ContextRows<'_> = Vec::new();
        if let Context::Display(_, down) = context {
            let mut k = 1;
            while last.index() + 1 == nrows
                && vp.wrapped(Row::from_index(nrows - 1 + k - 1)) == Wrap::Continues
                && k <= down.index()
                && below.len() < budget
            {
                let at = Row::from_index(nrows - 1 + k);
                let src = self.row_source(term, vp, WindowLine::of(at));
                below.push((vp.row(at), vp.margins(at), src));
                k += 1;
            }
        }
        if last.index() + 1 == nrows
            && vp.wrapped(last) == Wrap::Continues
            && context == Context::Window
        {
            let mut k = 1;
            while k <= back.index() && below.len() < budget {
                // `of_count`: at `k == back` the live screen, view 0.
                let v = term.view(Rows::of_count(back.index() - k));
                let at = Row::from_index(nrows - 1);
                reach.down = Rows::of_count(k);
                below.push((v.row(at), v.margins(at), RowSource::One(v.styles(at))));
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
        // A row holding an SDS or SRS string is resolved too: a string may
        // reorder text of class L alone.
        let strung = drawn_strung;
        let any_opens = path.path() == CharacterPath::RightToLeft
            || strung
            || drawn_opens
            || above.iter().any(|c| opens(c.0))
            || below.iter().any(|c| opens(c.0));
        // A paragraph that nothing reorders but whose drawn rows hold a
        // joining letter -- Mongolian, whose class is L -- is joined in its
        // stored order, every character at the paragraph's level, and drawn
        // where it is.
        let resolves = match (any_opens, drawn_joins) {
            (true, _) => Resolves::Levels,
            (false, true) => Resolves::JoinOnly,
            (false, false) => return reach,
        };
        let drawn: Vec<ParaRow<'_>> = (first.index()..=last.index())
            .map(|y| ParaRow {
                cells: row_of(Row::from_index(y)),
                window: Some(Row::from_index(y)),
                margins: vp.margins(Row::from_index(y)),
                words: sources[y - first.index()].words(),
            })
            .collect();
        // The drawn rows in chunks that fit, each with the context that
        // fits around it: the rows above before the first chunk, the rows
        // below after the last.
        let cap = Chars(CONTEXT);
        // **The level its properties give it** ([`LineBidi::paragraph`]):
        // at power-on, with no path set and autodetection on, the level UAX
        // #9 P2-P3 finds -- 1 when its first strong letter, isolates
        // skipped, is R or AL, else 0; a right-to-left
        // paragraph's erased cells past its text are laid out before it, so
        // its rows sit against the right edge. The letter is looked for
        // only in the text the layout reads, the paragraph's drawn rows and
        // the context around them ([`CONTEXT`]); a paragraph resolved in
        // more than one piece keeps the first piece's level for the rest
        // (HL1).
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
                    let n = chars_of(term, c.0);
                    if Chars(used.0 + n.0) > cap {
                        break;
                    }
                    used = Chars(used.0 + n.0);
                    take += 1;
                }
                rows.extend(above[..take].iter().rev().map(|(c, m, src)| ParaRow {
                    cells: c,
                    window: None,
                    margins: *m,
                    words: src.words(),
                }));
            }
            rows.extend(drawn[start..end].iter().map(|r| ParaRow {
                cells: r.cells,
                window: r.window,
                margins: r.margins,
                words: r.words,
            }));
            if end == drawn.len() {
                for (c, m, src) in &below {
                    let n = chars_of(term, c);
                    if Chars(used.0 + n.0) > cap {
                        break;
                    }
                    used = Chars(used.0 + n.0);
                    rows.push(ParaRow {
                        cells: c,
                        window: None,
                        margins: *m,
                        words: src.words(),
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
        reach
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
        #[cfg(feature = "counters")]
        crate::counters::add(
            crate::counters::Counter::BidiRowsLaid,
            crate::counters::Site::Layout,
            Tally::of(rows.iter().filter(|r| r.window.is_some()).count()),
        );
        self.text.clear();
        self.owner.clear();
        self.rule.clear();
        let mut ranges: Vec<(core::ops::Range<usize>, Col)> = Vec::with_capacity(rows.len());
        // **SDS's and SRS's strings, each an isolate of its direction**
        // (UAX #9 HL3): where a cell's chain of strings differs from
        // the last cell's, a PDI for each string left and an LRI or RLI
        // for each entered, a reversed string's the direction opposite
        // to the one around it, the line's path at the top. An isolate
        // still open at the paragraph's end ends there (BD9).
        let strings = term.strings_seen();
        // Each row's cells read for their strings only where a slot names
        // one, one bit a cell.
        let strung: Vec<bool> = match strings {
            true => rows
                .iter()
                .map(|row| self.strung.row_strung(term, row.words, row.cells) == Opens::Yes)
                .collect(),
            false => Vec::new(),
        };
        let entry = |id: StringId| term.string_entry(id);
        let top = match path.path() {
            CharacterPath::RightToLeft => BidiDirection::Rtl,
            _ => BidiDirection::Ltr,
        };
        let mut open = [StringId::NONE; STRING_DEPTH];
        let mut open_n = 0;
        let mut last: Option<Owner> = None;
        // **Each row's cells in its paragraph's columns**
        // ([`paragraph_columns`]), as `(row, from, its text's end, to)`, in
        // row order; then, where a row's paragraph is confined to side
        // margins, each run of its cells outside them, after a paragraph
        // separator, so each is a paragraph of its own (P1), ordered in its
        // own columns as a table row's cells are.
        let columns = paragraph_columns(rows);
        let text_end =
            |cells: &[PackedCell], (a, b): (usize, usize)| a + row_end(&cells[a..b]).index();
        let mut pieces: Vec<(usize, usize, usize, usize)> = columns
            .iter()
            .enumerate()
            .map(|(p, &(a, b))| (p, a, text_end(rows[p].cells, (a, b)), b))
            .collect();
        for (p, (&(a, b), row)) in columns.iter().zip(rows).enumerate() {
            for (from, to) in [(0, a), (b, row.cells.len())] {
                if from < to && (a, b) != (0, row.cells.len()) {
                    pieces.push((p, from, text_end(row.cells, (from, to)), to));
                }
            }
        }
        // The runs outside the margins: their row, text and columns.
        let mut outside: Vec<Alone> = Vec::new();
        for (k, &(p, from, end, to)) in pieces.iter().enumerate() {
            let row = &rows[p];
            let alone = k >= rows.len();
            if alone {
                // A paragraph's end ends every isolate in it (BD9).
                open_n = 0;
                self.text.push(PARAGRAPH_SEPARATOR);
                self.owner.push(Owner {
                    col: Col::at_index(from),
                    lead: Lead::Attached,
                    row: Row::from_index(p),
                });
                self.rule.push(Rule::No);
            }
            let begin = self.text.len();
            for (c, &cell) in row.cells[from..end].iter().enumerate() {
                let c = from + c;
                let at = |lead| Owner {
                    col: Col::at_index(c),
                    lead,
                    row: Row::from_index(p),
                };
                if cell.content.is_wide_tail() {
                    continue;
                }
                if strings {
                    let mut chain = [StringId::NONE; STRING_DEPTH];
                    // A row with no string's cells are in none.
                    let n = match strung.get(p).copied().unwrap_or(false) {
                        true => string_chain(
                            string_of(term, row.words, Col::at_index(c), cell),
                            &entry,
                            &mut chain,
                        ),
                        false => 0,
                    };
                    let same = (0..n.min(open_n))
                        .take_while(|&i| chain[i] == open[i])
                        .count();
                    for _ in same..open_n {
                        self.text.push(POP_DIRECTIONAL_ISOLATE);
                        self.owner.push(match last {
                            Some(o) => Owner {
                                lead: Lead::Attached,
                                ..o
                            },
                            None => at(Lead::Attached),
                        });
                        self.rule.push(Rule::No);
                    }
                    for i in same..n {
                        self.text.push(match chain_runs(&chain[..=i], &entry, top) {
                            BidiDirection::Ltr => LEFT_TO_RIGHT_ISOLATE,
                            BidiDirection::Rtl => RIGHT_TO_LEFT_ISOLATE,
                        });
                        self.owner.push(at(Lead::Attached));
                        self.rule.push(Rule::No);
                    }
                    open = chain;
                    open_n = n;
                    last = Some(at(Lead::First));
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
            match alone {
                false => ranges.push((begin..self.text.len(), Col::at_index(end))),
                true => outside.push(Alone {
                    row: Row::from_index(p),
                    text: begin..self.text.len(),
                    from: Col::at_index(from),
                    end: Col::at_index(end),
                    to: Col::at_index(to),
                }),
            }
        }
        // Each row's text: its paragraph's, then the runs outside it.
        let lines_of = |p: usize| -> Vec<core::ops::Range<usize>> {
            core::iter::once(ranges[p].0.clone())
                .chain(
                    outside
                        .iter()
                        .filter(|o| o.row.index() == p)
                        .map(|o| o.text.clone()),
                )
                .collect()
        };
        let cell_of = |o: Owner| {
            let row = rows.get(o.row.index());
            let cell = row
                .and_then(|r| r.cells.get(o.col.index()))
                .copied()
                .unwrap_or(PackedCell::EMPTY);
            let words = row.map_or(Words::LIVE, |r| r.words);
            match row.and_then(|r| r.window) {
                Some(_) => (cell, Drawn::Yes, words),
                None => (cell, Drawn::No, words),
            }
        };
        if resolves == Resolves::JoinOnly {
            let joined = self.join_text(None, None);
            for (p, row) in rows.iter().enumerate() {
                if let Some(y) = row.window {
                    self.store_joins(term, (y, &lines_of(p)), &joined, |c| c, cell_of);
                }
            }
            return None;
        }
        // **A table row**: a paragraph holding a column rule keeps its
        // columns. Each cell is a paragraph of its own, at the level its
        // own first strong letter gives it (UAX #9 section 3: "the text in
        // two different cells of a table will be in different paragraphs"),
        // each is ordered inside its own columns, and the row runs left to
        // right, every rule where it was written. A path SCP or SPD set is
        // each cell's level, HL1 on each cell's paragraph.
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
            // **The row's runs in column order**, each ordered in its own
            // columns: its paragraph's, and on a row confined to side
            // margins the runs outside them, each at its own paragraph's
            // level -- `(text, from, its text's end, to, level)`.
            let (a, b) = columns[p];
            let inside = (
                ranges[p].0.clone(),
                a.min(width),
                ranges[p].1.index().min(b).min(width),
                b.min(width),
                level,
            );
            let alone = |o: &Alone| {
                let at = self
                    .bidi
                    .paragraph_level(o.text.start)
                    .filter(|_| !o.text.is_empty())
                    .unwrap_or(BidiLevel::LTR);
                (
                    o.text.clone(),
                    o.from.index().min(width),
                    o.end.index().min(width),
                    o.to.index().min(width),
                    at,
                )
            };
            let mut runs: Vec<_> = outside
                .iter()
                .filter(|o| o.row.index() == p && o.from.index() < a)
                .map(alone)
                .collect();
            runs.push(inside);
            runs.extend(
                outside
                    .iter()
                    .filter(|o| o.row.index() == p && o.from.index() >= b)
                    .map(alone),
            );
            let mut out = RowOrder {
                from: Vec::with_capacity(cols),
                to: vec![Col::ZERO; cols],
                level: Vec::with_capacity(cols),
                swaps: Vec::new(),
                mirrors: Vec::new(),
                slants: Vec::new(),
                drawn: Cols::of_index(width),
            };
            let mut reorders = false;
            let mut ordered = true;
            for (line, _, end, to, at) in runs {
                // The erased cells past a run's text go at its end in its
                // paragraph's direction: before the text where that runs
                // right to left, nearest the text last.
                let rtl = at.direction() == baud_domain::BidiDirection::Rtl;
                reorders |= rtl;
                if rtl {
                    for c in (end..to).rev() {
                        out.from.push(Col::at_index(c));
                        out.level.push(at);
                    }
                }
                let begin = line.start;
                if self.order_line(line).is_none() {
                    ordered = false;
                    break;
                }
                let levels = core::mem::take(&mut self.levels);
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
                        let span = crate::cellmap::CellSpan::at(row.cells, own.col);
                        match mirror(term, cell, (path.box_mirror(), span)) {
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
                self.levels = levels;
                if !rtl {
                    for c in end..to {
                        out.from.push(Col::at_index(c));
                        out.level.push(at);
                    }
                }
            }
            if !ordered {
                continue;
            }
            for c in width..cols {
                out.from.push(Col::at_index(c));
                out.level.push(level);
            }
            if !reorders || out.from.len() != cols {
                self.store_joins(term, (y, &lines_of(p)), &joined, |c| c, cell_of);
                continue;
            }
            for (v, l) in out.from.iter().enumerate() {
                if let Some(slot) = out.to.get_mut(l.index()) {
                    *slot = Col::at_index(v);
                }
            }
            let to = out.to.clone();
            self.store_joins(
                term,
                (y, &lines_of(p)),
                &joined,
                |c| to.get(c.index()).copied().unwrap_or(c),
                cell_of,
            );
            self.orders.maps.set_row_grown(y, Some(out));
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
    /// (`self.text`, `self.owner`): rules R1-R7 and Syriac alaph's S1-S3
    /// over each level run in logical order (UAX #9 3.5's "limited to
    /// characters within the same level run", read from `levels`, the
    /// levels after I2, or one run with none), a character X9 removes kept
    /// in the run it sits in, and the lam-alef ligatures L1-L3 among them.
    /// Each text position's form, each pair's lam and alef -- every pair
    /// the rules form, whatever erased cells or soft wrap lie between its
    /// two letters -- and the runs.
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
        let mut runs: Vec<core::ops::Range<usize>> = Vec::new();
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
            alaphs(&cps[start..end], &types[start..end], &mut forms[start..end]);
            runs.push(start..end);
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
                    .map(|form| Joined {
                        form,
                        pair: None,
                        around: Around::default(),
                    })
            })
            .collect();
        JoinedText {
            forms,
            pairs,
            runs,
            reading,
        }
    }

    /// Stores the joins of window row `y`, whose text is `line` of
    /// `self.text`, each cell's at the visual column `to` maps its logical
    /// column to, and each pair's part: a pair on this row with the cells
    /// it spans as drawn, and a pair a soft wrap parts with the other
    /// letter's cell, which `cell_of` reads with whether its row is drawn.
    /// An alef whose lam is on a row of context, which is not drawn, keeps
    /// its final form, so its letter is not lost with the ligature. With a
    /// reach, the cells of each joined cell's word around it, as many
    /// either side as the reach, within its run: read through `cell_of` on
    /// whichever row of the paragraph holds them, an erased cell between
    /// letters, which joining passes through, left out, and a column rule
    /// ending the word. A letter of the reading joined that SAPV 5-8 asks
    /// a form of is stored in that form, joined or not, so it keeps its
    /// word too. A row with none stores nothing.
    fn store_joins<'w>(
        &mut self,
        term: &Term<'_>,
        (y, lines): (Row, &[core::ops::Range<usize>]),
        joined: &JoinedText,
        to: impl Fn(Col) -> Col,
        cell_of: impl Fn(Owner) -> (PackedCell, Drawn, Words<'w>),
    ) {
        let cols = self.orders.cols.index();
        let asks = term.variants_seen();
        let mut row: Option<RowJoins> = None;
        for t in lines.iter().flat_map(Clone::clone) {
            let Some(&own) = self.owner.get(t) else {
                continue;
            };
            if own.lead != Lead::First {
                continue;
            }
            let j = match joined.forms.get(t).copied().flatten() {
                Some(j) => j,
                None if asks
                    && joined
                        .reading
                        .is_none_or(|r| Reading::of(self.text[t]) == r) =>
                {
                    let (cell, _, words) = cell_of(own);
                    match asked_form(term, words, own.col, cell) {
                        Some(form) => Joined {
                            form,
                            pair: None,
                            around: Around::default(),
                        },
                        None => continue,
                    }
                }
                None => continue,
            };
            let v = to(own.col).index();
            let slots = &mut row
                .get_or_insert_with(|| RowJoins {
                    cells: vec![None; cols],
                    words: Vec::new(),
                })
                .cells;
            if let Some(slot) = slots.get_mut(v) {
                *slot = Some(j);
            }
        }
        if let (Some(r), false) = (row.as_mut(), self.reach.is_zero()) {
            self.store_words(term, r, lines, joined, &to, &cell_of);
        }
        if let Some(slots) = row.as_mut().map(|r| &mut r.cells) {
            for p in &joined.pairs {
                let (lam, alef) = (self.owner[p.lam.0], self.owner[p.alef.0]);
                let here = |t: usize| lines.iter().any(|l| l.contains(&t));
                let (lam_here, alef_here) = (here(p.lam.0), here(p.alef.0));
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
                        (cell, Drawn::Yes, _) => set(alef.col, PairPart::AlefWrapped(cell)),
                        (_, Drawn::No, _) => {}
                    },
                    (false, false) => {}
                }
            }
        }
        if let Some(r) = row {
            if self.orders.maps.set_joins(y, r) {
                self.orders.joined_any = Any::Yes;
            }
        }
    }

    /// **The words around a row's joined cells**, into `row`: for each run
    /// holding a cell of the row's text `lines` that `row` holds a form of,
    /// joined or asked, the run's cells in
    /// logical order -- the cells whose letters are given, erased cells
    /// left out, a column rule cutting the run -- from `reach` before the
    /// row's first joined cell in it to `reach` past its last, and each
    /// joined cell's place among them.
    fn store_words<'w>(
        &self,
        term: &Term<'_>,
        row: &mut RowJoins,
        lines: &[core::ops::Range<usize>],
        joined: &JoinedText,
        to: &impl Fn(Col) -> Col,
        cell_of: &impl Fn(Owner) -> (PackedCell, Drawn, Words<'w>),
    ) {
        let reach = usize::try_from(self.reach.value()).unwrap_or(usize::MAX);
        let varies = term.variants_seen();
        let here = |t: usize| lines.iter().any(|l| l.contains(&t));
        for run in &joined.runs {
            if !lines.iter().any(|l| run.start < l.end && l.start < run.end) {
                continue;
            }
            // The run's cells, cut at a column rule into the pieces a
            // word cannot cross.
            let mut cells: Vec<usize> = Vec::new();
            let mut pieces: Vec<core::ops::Range<usize>> = Vec::new();
            let mut from = 0;
            for t in run.clone() {
                let Some(own) = self.owner.get(t) else {
                    continue;
                };
                if own.lead != Lead::First || self.text[t] == ERASED {
                    continue;
                }
                if self.text[t] == PARAGRAPH_SEPARATOR {
                    pieces.push(from..cells.len());
                    from = cells.len();
                    continue;
                }
                cells.push(t);
            }
            pieces.push(from..cells.len());
            for piece in pieces {
                let mine: Vec<usize> = piece
                    .clone()
                    .filter(|&i| {
                        here(cells[i])
                            && row
                                .cells
                                .get(to(self.owner[cells[i]].col).index())
                                .is_some_and(Option::is_some)
                    })
                    .collect();
                let (Some(&first), Some(&last)) = (mine.first(), mine.last()) else {
                    continue;
                };
                let lo = first.saturating_sub(reach).max(piece.start);
                let hi = last.saturating_add(reach.saturating_add(1)).min(piece.end);
                let base = row.words.len();
                for &t in &cells[lo..hi] {
                    let o = self.owner[t];
                    let cell = match varies {
                        true => word_cell_drawn(term, o, cell_of),
                        false => cell_of(o).0,
                    };
                    // A frozen row's cell keeps its word: its id names its
                    // generation's, not the frame's table.
                    let words = cell_of(o).2;
                    row.words.push(WordCell {
                        cell,
                        form: joined.forms[t].map(|j| j.form),
                        word: (!words.is_live()).then(|| words.word(term, o.col, cell)),
                    });
                }
                for i in mine {
                    let v = to(self.owner[cells[i]].col).index();
                    if let Some(Some(j)) = row.cells.get_mut(v) {
                        j.around = Around {
                            lo: WordIx(base + i.saturating_sub(reach).max(lo) - lo),
                            at: WordIx(base + i - lo),
                            hi: WordIx(
                                base + i.saturating_add(reach.saturating_add(1)).min(hi) - lo,
                            ),
                        };
                    }
                }
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
    fn join_displayed(
        &mut self,
        term: &Term<'_>,
        (r, words): (Row, Words<'_>),
        cells: &[PackedCell],
    ) {
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
            (cell.unwrap_or(PackedCell::EMPTY), Drawn::Yes, words)
        };
        self.store_joins(
            term,
            (r, core::slice::from_ref(&line)),
            &joined,
            |c| c,
            cell_of,
        );
        if !ltr {
            return;
        }
        let right_to_left = self.orders.maps.take_joins(r);
        self.displayed_text(term, cells, 0..end, Reading::LeftToRight);
        let joined = self.join_text(None, Some(Reading::LeftToRight));
        let line = 0..self.text.len();
        self.store_joins(
            term,
            (r, core::slice::from_ref(&line)),
            &joined,
            |c| c,
            cell_of,
        );
        // Each cell's letter is one reading's, so the two never both name
        // a form for one cell.
        let Some(before) = right_to_left else {
            return;
        };
        match self.orders.maps.joins_mut(r) {
            Some(now) => {
                // The other reading's words follow this one's, each of its
                // cells moved to find them there.
                let moved = now.words.len();
                now.words.extend_from_slice(&before.words);
                for (n, b) in now.cells.iter_mut().zip(before.cells) {
                    if n.is_none() {
                        *n = b.map(|mut j| {
                            let Around { lo, at, hi } = j.around;
                            j.around = match hi > lo {
                                true => Around {
                                    lo: WordIx(lo.0 + moved),
                                    at: WordIx(at.0 + moved),
                                    hi: WordIx(hi.0 + moved),
                                },
                                false => j.around,
                            };
                            j
                        });
                    }
                }
            }
            None => {
                self.orders.maps.set_joins(r, before);
            }
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

/// A paragraph's text joined: each position's form, the pairs, the runs
/// joining was limited to, each its text positions, and the reading whose
/// letters it joined, if one.
struct JoinedText {
    forms: Vec<Option<Joined>>,
    pairs: Vec<Pair>,
    runs: Vec<core::ops::Range<usize>>,
    reading: Option<Reading>,
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

fn mirror(
    term: &Term<'_>,
    cell: PackedCell,
    (boxes, span): (BoxMirror, crate::cellmap::CellSpan),
) -> Mirror {
    let Some(c) = term
        .base_of(cell)
        .drawn()
        .as_codepoint()
        .and_then(|c| char::from_u32(c.value()))
    else {
        return Mirror::Not;
    };
    // **`?2500` on a box drawing character two cells wide mirrors its
    // whole mask** (`crate::boxes::Facing`): its counterpart with its
    // verticals in its last cell, the first the line is read in, so a
    // character with no side in its name -- `│`, `┼` -- moves its
    // vertical there too.
    if boxes == BoxMirror::On
        && span == crate::cellmap::CellSpan::Two
        && box_counterpart(c).is_some()
    {
        return Mirror::Flipped(None);
    }
    // **`?2500`: a Box Drawing character drawn right to left is drawn as
    // its counterpart** (`[[source: terminal-wg-bidi-misc]]`, "mirror the
    // U+2500 .. U+257F glyphs in RTL"; UAX #9 HL6), under its marks as a
    // mirrored character is. Only one drawn right to left: L4's resolved R,
    // with HL6 widening which characters, so a table row's rules, kept
    // where they were written at the left-to-right level, are drawn as
    // written.
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

/// **The SDS or SRS string `cell`, at column `col` of a row whose ids name
/// `words`, was written in** ([`StringId`]).
pub(crate) fn string_of(term: &Term<'_>, words: Words<'_>, col: Col, cell: PackedCell) -> StringId {
    words.word(term, col, cell).string()
}

/// **The SAPV variants cell `cell` was written under**, read as
/// [`string_of`] reads its string: the default presentation where its
/// rendition names none.
fn variants_of(term: &Term<'_>, words: Words<'_>, col: Col, cell: PackedCell) -> Variants {
    words.word(term, col, cell).variants()
}

/// **The form SAPV 5-8 asks of `cell`'s letter**, as [`forced_form`]
/// gives it: none where its variant asks none, or under 18 and 19, which
/// join nothing.
fn asked_form(
    term: &Term<'_>,
    words: Words<'_>,
    col: Col,
    cell: PackedCell,
) -> Option<JoiningForm> {
    let var = variants_of(term, words, col, cell);
    if var.is_default() || !var.joins() {
        return None;
    }
    let base = term.base_of(cell).drawn().as_codepoint()?;
    var.form()
        .and_then(|f| forced_form(f, JoiningType::of(base.value())))
}

/// **Whether SAPV 15 left a right-to-left path's mirrored character as
/// stored**, with BDSM reset: the device does no bidi there, and the
/// application's SAPV says whether mirrorable glyphs are mirrored, value 3
/// the characters with a Bidi_Mirroring_Glyph ("presented as the other
/// member of the pair") and 4 those with none ("mirrored about that
/// vertical axis"), `[[source: ecma48]]` 8.3.108; TR/53's worked example 1
/// pairs SAPV 3 with EXPLICIT. With BDSM set, L4 governs: "mirroring must
/// be done according to rule L4" (`[[source: uax9-bidi]]` section 7).
fn unmirrored(term: &Term<'_>, words: Words<'_>, col: Col, cell: PackedCell) -> bool {
    if !term.variants_seen() {
        return false;
    }
    let v = variants_of(term, words, col, cell);
    if v.is_default() {
        return false;
    }
    let Some(c) = term
        .base_of(cell)
        .drawn()
        .as_codepoint()
        .and_then(|c| char::from_u32(c.value()))
    else {
        return false;
    };
    if BidiProps::of(c).mirrored != BidiMirrored::Yes {
        return false;
    }
    match mirroring_glyph(c) {
        Some(_) => v.paired() == Mirroring::Cancelled,
        None => v.mathematical() == Mirroring::Cancelled,
    }
}

/// **Whether `cell` holds a character written italic under SAPV 17**: a
/// wide character's tail holds none, its lead drawing it.
pub(crate) fn slanted(term: &Term<'_>, words: Words<'_>, col: Col, cell: PackedCell) -> bool {
    if cell.content.is_empty() || cell.content.is_wide_tail() {
        return false;
    }
    let w = words.word(term, col, cell);
    w.variants().slant() == Slant::ByDirection && w.attrs().has(Attrs::ITALIC)
}

/// An ASCII digit, for a decimal number's mark.
fn is_digit(cell: Option<&PackedCell>) -> bool {
    cell.and_then(|c| c.content.as_codepoint())
        .is_some_and(|c| (u32::from('0')..=u32::from('9')).contains(&c.value()))
}

/// **The character a variant draws `cell` as, where it is another**
/// ([`Reorder::vary_swaps`]), the cells stored either side of it in its
/// row `(before, after)`: a decimal digit as its Arabic-Indic counterpart
/// (value 2, unless 18), and a FULL STOP between two digits as a COMMA
/// (value 10). A cell carrying a mark is drawn as stored.
fn varied(
    term: &Term<'_>,
    (words, col): (Words<'_>, Col),
    cell: PackedCell,
    (before, after): (Option<&PackedCell>, Option<&PackedCell>),
) -> Option<Content> {
    if cell.flags.is_marked() {
        return None;
    }
    let v = variants_of(term, words, col, cell);
    if v.is_default() {
        return None;
    }
    let ch = cell
        .content
        .as_codepoint()
        .and_then(|c| char::from_u32(c.value()))?;
    let to = match ch {
        '0'..='9' => v.digit(ch),
        '.' if v.decimal_mark() == DecimalMark::Comma && is_digit(before) && is_digit(after) => ',',
        _ => return None,
    };
    (to != ch)
        .then(|| Codepoint::new(u32::from(to)).map(Content::codepoint))
        .flatten()
}

/// **A cell of a joined letter's word as it is drawn**: the character a
/// variant draws it as ([`varied`]), read with the cells stored either side
/// of it in its row, through `cell_of`, so the word a letter is shaped
/// with holds what its neighbours show (`[[source: ecma48]]` 8.3.108).
/// Out of line, as [`Reorder::vary_rows`] is: a program that never sends
/// SAPV never calls it.
#[inline(never)]
fn word_cell_drawn<'w>(
    term: &Term<'_>,
    o: Owner,
    cell_of: &impl Fn(Owner) -> (PackedCell, Drawn, Words<'w>),
) -> PackedCell {
    let (cell, _, words) = cell_of(o);
    let at = |c: usize| {
        cell_of(Owner {
            col: Col::at_index(c),
            ..o
        })
        .0
    };
    let before = o.col.index().checked_sub(1).map(at);
    let after = at(o.col.index() + 1);
    match varied(term, (words, o.col), cell, (before.as_ref(), Some(&after))) {
        Some(content) => PackedCell { content, ..cell },
        None => cell,
    }
}

impl Reorder {
    /// **SAPV's variants on the rows of `units` above row `n` of `vp`**
    /// (`[[source: ecma48]]` 8.3.108): the substitutions, the forms and
    /// the slants, on the rows a layout laid out again alone -- a kept
    /// row's are its own already, and a substitution made twice is drawn
    /// twice. Only once a program has selected one
    /// ([`Term::variants_seen`]). Out of line, as a program that never
    /// sends SAPV never runs it: inlined into the layouts with the rows as
    /// written, btop's frames retired 0.08% more instructions with none
    /// sent.
    #[inline(never)]
    fn vary_rows(
        &mut self,
        term: &Term<'_>,
        vp: &Viewport<'_>,
        n: Rows,
        units: &[kept::Para],
        written: &dyn Over,
    ) {
        for y in units.iter().flat_map(kept::Para::rows) {
            if y >= n.index() {
                continue;
            }
            let r = Row::from_index(y);
            self.vary_swaps(term, vp, r, written);
            self.vary_joins(term, vp, r, written);
            self.vary_slants(term, vp, r, written);
        }
    }

    /// SAPV's substitutions and slants on the rows of `units`, as written,
    /// for [`Reorder::keep_displayed`]; out of line, as
    /// [`Reorder::vary_rows`] is.
    #[inline(never)]
    fn vary_kept(
        &mut self,
        term: &Term<'_>,
        vp: &Viewport<'_>,
        units: &[kept::Para],
        written: &dyn Over,
    ) {
        for y in units.iter().flat_map(kept::Para::rows) {
            self.vary_swaps(term, vp, Row::from_index(y), written);
            self.vary_slants(term, vp, Row::from_index(y), written);
        }
    }

    /// **The characters a variant draws as another**: a decimal digit as
    /// its Arabic-Indic counterpart (value 2, unless 18), and a FULL STOP
    /// between two digits as a COMMA (value 10, "where the bit combination
    /// 02/14 is intended to represent a decimal mark in a decimal number"),
    /// drawn as a mirrored character is, in the row's maps: every path
    /// reads the drawn cell.
    fn vary_swaps(&mut self, term: &Term<'_>, vp: &Viewport<'_>, r: Row, written: &dyn Over) {
        let cells = written.row(r).unwrap_or_else(|| vp.row(r));
        let src = self.row_source(term, vp, WindowLine::of(r));
        let mut found: Vec<(Col, Content)> = Vec::new();
        for (x, &cell) in cells.iter().enumerate() {
            let around = (
                x.checked_sub(1).and_then(|b| cells.get(b)),
                cells.get(x + 1),
            );
            if let Some(content) = varied(term, (src.words(), Col::at_index(x)), cell, around) {
                found.push((Col::at_index(x), content));
            }
        }
        if found.is_empty() {
            return;
        }
        let cols = vp.cols().index();
        let width = vp.rendition(r).end_of_line(vp.cols()).index().min(cols);
        let order = self.orders.maps.row_made(r, || RowOrder {
            from: (0..cols).map(Col::at_index).collect(),
            to: (0..cols).map(Col::at_index).collect(),
            level: vec![BidiLevel::LTR; cols],
            swaps: Vec::new(),
            mirrors: Vec::new(),
            slants: Vec::new(),
            drawn: Cols::of_index(width),
        });
        for (l, content) in found {
            if let Some(&v) = order.to.get(l.index()) {
                order.swaps.push((v, content));
            }
        }
        self.orders.any = Any::Yes;
    }

    /// **The italic cells SAPV 17 slants left**: on a row drawn in another
    /// order, each visual cell at an odd level -- the string direction right
    /// to left: under IMPLICIT the level UAX #9 resolves, and with BDSM
    /// reset every cell of a right-to-left path -- that holds a character,
    /// not a wide one's tail, written italic under 17. At an even level its
    /// italic leans right, as the italic face draws it.
    fn vary_slants(&mut self, term: &Term<'_>, vp: &Viewport<'_>, r: Row, written: &dyn Over) {
        let cells = written.row(r).unwrap_or_else(|| vp.row(r));
        let src = self.row_source(term, vp, WindowLine::of(r));
        let words = src.words();
        let Some(order) = self.orders.maps.row_mut(r) else {
            return;
        };
        order.slants.clear();
        for (v, &l) in order.from.iter().enumerate() {
            let odd = order.level.get(v).is_some_and(|lv| lv.number() % 2 == 1);
            if odd
                && cells
                    .get(l.index())
                    .is_some_and(|&c| slanted(term, words, l, c))
            {
                order.slants.push(Col::at_index(v));
            }
        }
    }

    /// **The forms a variant draws a letter in**: none under 18 and 19,
    /// "presented in the form they are stored"; no lam-alef ligature under
    /// 14, each letter in its joined form; and a form 5 to 8 asked, as
    /// [`forced_form`] gives it. A pair one of whose letters is drawn apart
    /// is drawn apart. A letter's word keeps it: a letter whose form was
    /// asked keeps its word, and each cell of a word, whichever row it is
    /// on, is read by the shaping around it in the form its own variant
    /// draws it in.
    fn vary_joins(&mut self, term: &Term<'_>, vp: &Viewport<'_>, r: Row, written: &dyn Over) {
        let cells = written.row(r).unwrap_or_else(|| vp.row(r));
        let src = self.row_source(term, vp, WindowLine::of(r));
        let words = src.words();
        let cols = cells.len();
        // Read, not written: the row is stamped below only where a
        // variant changed its joins.
        let mut row: RowJoins = self
            .orders
            .maps
            .joins()
            .get(r.index())
            .cloned()
            .flatten()
            .unwrap_or_else(|| RowJoins {
                cells: vec![None; cols],
                words: Vec::new(),
            });
        let mut changed = false;
        for w in &mut row.words {
            let var = variants_of(term, w.words(), Col::ZERO, w.cell);
            if var.is_default() {
                continue;
            }
            let form = match var.joins() {
                false => None,
                true => asked_form(term, w.words(), Col::ZERO, w.cell).or(w.form),
            };
            if form != w.form {
                w.form = form;
                changed = true;
            }
        }
        for (v, slot) in row.cells.iter_mut().enumerate() {
            let l = self.orders.logical_col(r, Col::at_index(v)).index();
            let Some(&cell) = cells.get(l) else {
                continue;
            };
            let var = variants_of(term, words, Col::at_index(l), cell);
            if var.is_default() {
                continue;
            }
            if !var.joins() {
                changed |= slot.take().is_some();
                continue;
            }
            if !var.ligates() {
                if let Some(j) = slot.as_mut().filter(|j| j.pair.is_some()) {
                    j.pair = None;
                    changed = true;
                }
            }
            if let Some(form) = asked_form(term, words, Col::at_index(l), cell) {
                let around = slot.map_or_else(Around::default, |j| j.around);
                *slot = Some(Joined {
                    form,
                    pair: None,
                    around,
                });
                changed = true;
            }
        }
        if !changed {
            return;
        }
        // A pair whose other letter lost it is drawn apart.
        let kept: Vec<bool> = (0..cols)
            .map(|v| match row.cells[v].and_then(|j| j.pair) {
                Some((PairPart::Lam(n, end) | PairPart::Alef(n, end), _)) => {
                    let k = n.index().saturating_sub(1);
                    let other = match end {
                        PairEnd::Left => v.checked_add(k),
                        PairEnd::Right => v.checked_sub(k),
                    };
                    other
                        .and_then(|o| row.cells.get(o).copied().flatten())
                        .is_some_and(|j| j.pair.is_some())
                }
                _ => true,
            })
            .collect();
        for (slot, keep) in row.cells.iter_mut().zip(kept) {
            if let (Some(j), false) = (slot.as_mut(), keep) {
                j.pair = None;
            }
        }
        let any = row.cells.iter().any(Option::is_some);
        self.orders.maps.set_joins_grown(r, any.then_some(row));
        if any {
            self.orders.joined_any = Any::Yes;
        }
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
/// exit and one load a cell from [`baud_domain::ROW_BLOCKS`]
/// ([`RowBlocks`]). That pass answers whether the row may hold a syllable
/// or a word (`crate::syllable::may_hold`), reading a cluster's base and
/// marks only for a row that holds one. Each other answer's test of the
/// cells runs only for a row the pass holds back: one with a character in
/// a block that holds a right-to-left character or a joining letter, or one
/// with a cluster. A row of Latin, of Han or of box drawing holds nothing.
fn row_holds(term: &Term<'_>, cells: &[PackedCell]) -> Holds {
    let (blocks, marked) = cells.iter().fold((RowBlocks::NONE, false), |(b, m), c| {
        (b | RowBlocks::of(c.content.bits()), m | c.flags.is_marked())
    });
    let yes = |b: bool| match b {
        true => Opens::Yes,
        false => Opens::No,
    };
    let syllables =
        yes(blocks.syllables() || (marked && crate::syllable::clusters_may_hold(term, cells)));
    if !(blocks.layout() || marked) {
        return Holds {
            syllables,
            ..Holds::NOTHING
        };
    }
    Holds {
        opens: yes(opens_in(term, cells)),
        joins: yes(joins_in(term, cells)),
        // [`Reorder::test_row`] asks it of every row, a layout block or not.
        strung: Opens::No,
        syllables,
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

/// **The last layout of the window, kept a unit at a time**: a paragraph,
/// or with BDSM reset a row, kept while no row it read was written since
/// the seal it was read under (`Term::seal_writes`), so a frame on which
/// nothing was written lays out nothing and one on which a row was lays out
/// again, whole, each unit that read it. Its fields are private, so a
/// layout reaches a kept unit only through [`Laid::answer`], which reads
/// the generations: an answer from the key alone cannot be written in
/// `Reorder::lay_out_held`.
mod kept {
    use std::rc::Rc;

    use baud_core::{BidiSupport, Term};
    use baud_domain::{ClusterEpoch, Cols, Row, Rows, Seal, Viewport, WindowAt, Wrap};

    use super::{Changes, Display, Extent, Joins};
    use crate::units::Tally;

    /// Which layout a key is for: the window's, over its whole height or
    /// its first paragraph alone, or `Reorder::keep_displayed`'s
    /// for a caller with a compare of its own.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Pass {
        Window(Extent),
        Kept(Changes),
    }

    /// Whether a program has begun a string, or selected a variant.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Begun {
        No,
        Yes,
    }

    impl Begun {
        /// Whether `term` has begun an SDS or SRS string.
        pub(super) fn strings(term: &Term<'_>) -> Begun {
            match term.strings_seen() {
                true => Begun::Yes,
                false => Begun::No,
            }
        }

        /// Whether `term` has selected an SAPV variant.
        pub(super) fn variants(term: &Term<'_>) -> Begun {
            match term.variants_seen() {
                true => Begun::Yes,
                false => Begun::No,
            }
        }
    }

    /// **Whether joins are read, as a layout reads it**: with BDSM set a
    /// paragraph is joined in its resolution whatever the renderer has
    /// shaped, so the answer is the same either way and a first joining
    /// letter shaped lays nothing out again; with BDSM reset it decides
    /// whether a row is read for its joins at all.
    pub(super) fn read_joins(term: &Term<'_>, joins: Joins) -> Joins {
        match term.bidi_support() {
            BidiSupport::Explicit => joins,
            BidiSupport::Implicit => Joins::Seen,
        }
    }

    /// **The rows a layout shows**: stored rows, which their generations
    /// name, or a display's lines, which none does: the display held, and
    /// equal only to itself (`Rc::ptr_eq`), so a freed address can never
    /// meet a new display. A display is made again whenever a row it read
    /// was written ([`crate::rebreak`]).
    #[derive(Clone, Debug)]
    pub(super) enum Shown {
        Stored,
        Display(Rc<Display>),
    }

    impl PartialEq for Shown {
        fn eq(&self, other: &Shown) -> bool {
            match (self, other) {
                (Shown::Stored, Shown::Stored) => true,
                (Shown::Display(a), Shown::Display(b)) => Rc::ptr_eq(a, b),
                _ => false,
            }
        }
    }

    impl Eq for Shown {}

    /// **What a layout depended on beyond the cells and marks of the rows
    /// it read**: the pass, which physical rows the window shows (a scroll
    /// under a window held back moves it one ring row and writes none of
    /// the rows it then shows), how far it looks back, its size, BDSM,
    /// whether joins are read, the join reach, the cluster slots its cells
    /// name, whether a string or a variant was ever begun, and what it
    /// shows.
    #[derive(Clone, PartialEq, Eq, Debug)]
    pub(super) struct Key {
        pub(super) pass: Pass,
        pub(super) at: WindowAt,
        pub(super) back: Rows,
        pub(super) rows: Rows,
        pub(super) cols: Cols,
        pub(super) support: BidiSupport,
        pub(super) joins: Joins,
        pub(super) reach: baud_font::bytes::Count,
        pub(super) epoch: ClusterEpoch,
        pub(super) strings: Begun,
        pub(super) variants: Begun,
        pub(super) shown: Shown,
    }

    /// **How many stored rows a unit's context read** past the window:
    /// above it and below it, the row that ended each walk included.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct Reach {
        pub(super) up: Rows,
        pub(super) down: Rows,
    }

    impl Reach {
        pub(super) const NONE: Reach = Reach {
            up: Rows::ZERO,
            down: Rows::ZERO,
        };
    }

    /// **One unit a layout lays out whole**: a paragraph's window rows
    /// `first..=last`, one row with BDSM reset, and the context it read.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct Para {
        pub(super) first: Row,
        pub(super) last: Row,
        pub(super) reach: Reach,
    }

    impl Para {
        /// Row `r` alone.
        pub(super) fn one(r: Row) -> Para {
            Para {
                first: r,
                last: r,
                reach: Reach::NONE,
            }
        }

        /// The unit's window rows, by index.
        pub(super) fn rows(&self) -> core::ops::Range<usize> {
            self.first.index()..self.last.index() + 1
        }

        fn covers(&self, r: Row) -> bool {
            self.rows().contains(&r.index())
        }

        fn extent(&self) -> (Row, Row) {
            (self.first, self.last)
        }
    }

    /// How a window is cut into units: a row each (BDSM reset), its
    /// paragraphs, or its first paragraph alone ([`Extent::Top`]).
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Cut {
        Rows,
        Paragraphs,
        First,
    }

    /// **The units of `vp`, cut `how`, into `out`**: a paragraph runs to
    /// the first row that does not continue, as the layout reads it. One
    /// read of each row's wrap mark, and no cell.
    pub(super) fn cut(vp: &Viewport<'_>, how: Cut, out: &mut Vec<Para>) {
        out.clear();
        let nrows = vp.rows().index();
        let mut y = 0;
        while y < nrows {
            let first = y;
            if how != Cut::Rows {
                while y + 1 < nrows && vp.wrapped(Row::from_index(y)) == Wrap::Continues {
                    y += 1;
                }
            }
            out.push(Para {
                first: Row::from_index(first),
                last: Row::from_index(y),
                reach: Reach::NONE,
            });
            if how == Cut::First {
                return;
            }
            y += 1;
        }
    }

    /// What an ask of the last layout found.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) enum Answer {
        /// Every unit holds: nothing is laid out.
        Holds,
        /// The units left in the caller's buffer are laid out again, and
        /// the rest are kept.
        Units,
        /// It was laid out for another key: the window is laid out again.
        Again,
    }

    /// **The last layout**: its key and the seal its rows were read under,
    /// its units, and the rows it read as written over. `now` is scratch,
    /// kept so an ask allocates nothing.
    #[derive(Default)]
    pub(super) struct Laid {
        kept: Option<(Key, Seal)>,
        paras: Vec<Para>,
        written: Vec<Row>,
        now: Vec<Para>,
    }

    impl Laid {
        /// Nothing kept: the next ask lays out again.
        pub(super) fn forget(&mut self) {
            self.kept = None;
        }

        /// **What the last layout keeps for `key` now**, and how many rows'
        /// generations the answer read. Another key lays out again. Then,
        /// while no surface was written since the seal the layout is kept
        /// under and no row is written over now or was at the last layout,
        /// it holds, and no row is read. Otherwise the window is cut into
        /// its units now, and each is kept where the last layout laid out a
        /// unit of the same rows, no row of it is written over now or was
        /// then, and -- on stored rows -- no row it read was written after
        /// the seal: its window rows, then the rows its context read above
        /// the window, then those below, until the first that was. A row
        /// above that the history no longer holds reads as written. The
        /// units not kept are left in `out`.
        ///
        /// A unit is matched to the last layout's rather than judged by its
        /// own rows because a paragraph split by a row that ends lays out
        /// the lower part in its own order though none of its rows was
        /// written. Where every unit is kept, the layout is kept under the
        /// seal a reader names now, `term.sealed()`, so the next ask reads
        /// no row: every row it read carries a generation at or below the
        /// old seal's, which is at or below the new one's, and a store to
        /// any of them after this carries a later one.
        pub(super) fn answer(
            &mut self,
            term: &Term<'_>,
            key: &Key,
            vp: &Viewport<'_>,
            written: &[Row],
            how: Cut,
            out: &mut Vec<Para>,
        ) -> (Answer, Tally) {
            out.clear();
            let under = match &self.kept {
                Some((k, under)) if k == key => *under,
                _ => return (Answer::Again, Tally::ZERO),
            };
            if !term.wrote_since(&under) && written.is_empty() && self.written.is_empty() {
                return (Answer::Holds, Tally::ZERO);
            }
            cut(vp, how, &mut self.now);
            if how == Cut::First
                && self.now.first().map(Para::extent) != self.paras.first().map(Para::extent)
            {
                return (Answer::Again, Tally::ZERO);
            }
            let stored = key.shown == Shown::Stored;
            let mut read = 0;
            let mut j = 0;
            for u in self.now.iter_mut() {
                while self
                    .paras
                    .get(j)
                    .is_some_and(|p| p.first.index() < u.first.index())
                {
                    j += 1;
                }
                let was = self
                    .paras
                    .get(j)
                    .copied()
                    .filter(|p| p.extent() == u.extent());
                let keeps = was.is_some_and(|p| {
                    u.reach = p.reach;
                    !written.iter().chain(&self.written).any(|&r| p.covers(r))
                        && (!stored || unwritten(term, key, &under, &p, &mut read))
                });
                if !keeps {
                    out.push(*u);
                }
            }
            let read = Tally::of(read);
            if !out.is_empty() {
                return (Answer::Units, read);
            }
            self.written.clear();
            self.written.extend_from_slice(written);
            self.kept = Some((key.clone(), term.sealed()));
            (Answer::Holds, read)
        }

        /// **The units just laid out, `laid`**, kept for `key` under
        /// `term.sealed()` with the rows written over at them: in place of
        /// every unit after [`Answer::Again`], in place of those they
        /// replace after [`Answer::Units`], the rest as the ask found them.
        /// Every unit kept was read clean by the ask, so the seal covers
        /// it.
        pub(super) fn settle(
            &mut self,
            term: &Term<'_>,
            key: &Key,
            laid: &[Para],
            written: &[Row],
            answer: Answer,
        ) {
            match answer {
                Answer::Holds => return,
                Answer::Again => {
                    self.paras.clear();
                    self.paras.extend_from_slice(laid);
                }
                Answer::Units => {
                    let mut i = 0;
                    for u in self.now.iter_mut() {
                        while laid
                            .get(i)
                            .is_some_and(|l| l.first.index() < u.first.index())
                        {
                            i += 1;
                        }
                        if let Some(l) = laid.get(i).filter(|l| l.first == u.first) {
                            *u = *l;
                        }
                    }
                    core::mem::swap(&mut self.paras, &mut self.now);
                }
            }
            self.written.clear();
            self.written.extend_from_slice(written);
            self.kept = Some((key.clone(), term.sealed()));
        }
    }

    /// **Whether no row unit `p` read was written after `under`**, each
    /// row asked once and counted in `read`, stopping at the first that
    /// was.
    fn unwritten(term: &Term<'_>, key: &Key, under: &Seal, p: &Para, read: &mut usize) -> bool {
        let window = term.row_gens(key.back);
        for y in p.rows() {
            *read += 1;
            if window.after(Row::from_index(y), under) {
                return false;
            }
        }
        for k in 1..=p.reach.up.index() {
            let want = Rows::of_index(key.back.index() + k);
            if term.looking_back(want) != want {
                return false;
            }
            *read += 1;
            if term.row_gens(want).after(Row::ZERO, under) {
                return false;
            }
        }
        let last = Row::from_index(key.rows.index().saturating_sub(1));
        for k in 1..=p.reach.down.index() {
            let Some(below) = key.back.index().checked_sub(k) else {
                return false;
            };
            *read += 1;
            if term.row_gens(Rows::of_count(below)).after(last, under) {
                return false;
            }
        }
        true
    }

    /// What the status line's layout depended on beyond its row: the
    /// window's height, where its order sits in the maps, its width, BDSM,
    /// whether joins are read, the reach, the cluster slots, and whether a
    /// string or a variant was ever begun.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    pub(super) struct LineKey {
        pub(super) at: Rows,
        pub(super) cols: Cols,
        pub(super) support: BidiSupport,
        pub(super) joins: Joins,
        pub(super) reach: baud_font::bytes::Count,
        pub(super) epoch: ClusterEpoch,
        pub(super) strings: Begun,
        pub(super) variants: Begun,
    }

    /// **The status line's last layout**, kept while its plane was not
    /// written since the seal it was read under (`Term::status_gen`).
    #[derive(Default)]
    pub(super) struct Line {
        kept: Option<(LineKey, Seal)>,
    }

    impl Line {
        /// Nothing kept: the window's layout cleared the status line's
        /// order with its own.
        pub(super) fn forget(&mut self) {
            self.kept = None;
        }

        /// Whether the last layout holds for `key` now, and how many
        /// generations the answer read: none while no surface was written,
        /// else the status line's one, the layout kept under the seal now
        /// where it was not written.
        pub(super) fn answer(&mut self, term: &Term<'_>, key: &LineKey) -> (Answer, Tally) {
            let Some((k, under)) = self.kept.as_mut() else {
                return (Answer::Again, Tally::ZERO);
            };
            if *k != *key {
                return (Answer::Again, Tally::ZERO);
            }
            if !term.wrote_since(under) {
                return (Answer::Holds, Tally::ZERO);
            }
            match term.status_gen() {
                Some(g) if !g.after(under) => {
                    *under = term.sealed();
                    (Answer::Holds, Tally::of(1))
                }
                _ => (Answer::Again, Tally::of(1)),
            }
        }

        /// The status line just laid out for `key`, kept under
        /// `term.sealed()`.
        pub(super) fn settle(&mut self, term: &Term<'_>, key: LineKey) {
            self.kept = Some((key, term.sealed()));
        }
    }
}
