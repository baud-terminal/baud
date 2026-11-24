//! The cells the cell shader must not draw from its map, and what draws
//! them instead.
//!
//! The cell path draws one glyph a cell from a map keyed on the codepoint,
//! and a cell whose rendering is not a function of its codepoint alone --
//! today, a cell with attached codepoints: a family, a base with a mark --
//! is drawn wrong by it. This is the exception structure beside the grid:
//! one byte a screen cell, zero for "the map is right", one for "the
//! records draw this cell", and the records, the same twenty-byte
//! [`Instance`] the instance path emits, drawn between the cell glyph pass
//! and the decoration pass, and their parts above their rows after it. The
//! shaded glyphs a cluster resolves to are a
//! function of the cluster's content, so they are resolved once per
//! distinct cluster and kept -- the glyph map's shape, one level up.
//!
//! Nothing here is per frame except the walk, which the glyph map already
//! makes over every cell: a byte test a cell, a lookup a marked cell.

use crate::atlas::{Code, Flip, Stretch};
use crate::cellmap::CellSpan;
use crate::cellmap::{
    BlockCursor, BlockPaint, Candidate, CursorLook, GlyphMap, Hilite, Line, Resolved, Selected,
    Swaps,
};
use crate::color::Argb;
use crate::counters::{Counter, Site};
use crate::frozen::Words;
use crate::instance::{Instance, Screen};
use crate::metrics::Metrics;
use crate::picture::{Pictured, Pictures};
use crate::renderer::{BlinkPhases, Blinking, ClusterGlyph, Framed, Renderer};
use crate::seeded::Map;
use crate::select::Highlight;
use crate::units::Tally;
use baud_core::Term;
use baud_domain::{
    AmbiguousWidth, Attrs, Chan, Classified, ClusterBreak, ClusterCells, Codepoint, Col, ColorWord,
    Cols, Content, CursorStyle, GraphemeClass, LineRendition, PackedCell, Row, Rows, StyleWord,
    UnicodeWidths, Viewport, Vs16Width, Width, WidthPolicy, is_default_ignorable, vs16,
};
use baud_font::Px;

/// What one frame's walk found and what the store holds.
#[derive(Clone, Copy, Debug, Default)]
pub struct OverlayStats {
    /// Windows shaped this frame around the admitted pairs.
    pub windows: Tally,
    /// Windows shaped because their content had not been seen.
    pub window_misses: Tally,
    /// Cells the windows gathered this frame.
    pub window_cells: Tally,
    /// Cells the records draw this frame.
    pub cells: Tally,
    /// Records emitted this frame.
    pub records: Tally,
    /// Clusters shaped this frame because they had not been seen.
    pub misses: Tally,
    /// Distinct clusters resolved and kept.
    pub clusters: Tally,
    /// Cells of a picture drawn in text, whose backgrounds the shader draws
    /// solid under the dim.
    pub pictured: Tally,
    /// Rows walked this frame for their syllables and words: a row kept
    /// since an earlier frame (`crate::kept`) is not walked again.
    pub walked: Tally,
    /// Rows this frame tested for whether they may hold a syllable by the
    /// pass of a block table, where neither the layout nor the glyph map's
    /// walk of the view answered.
    pub folded: Tally,
}

/// The overlay mask's bits, one byte a screen cell. The shader reads the
/// same two, so they are spelled once here and once in `shaders.metal`.
///
/// `RECORD`: the records draw this cell's glyph and the cell pass must not.
/// `PICTURE`: the cell belongs to a picture drawn in text, and the
/// background pass draws it solid -- [`crate::Pictures`].
pub const MASK_RECORD: u8 = 1;
pub const MASK_PICTURE: u8 = 2;
/// `BARE`: the cell holds a cluster that takes no stress mark (`SGR 64`),
/// one whose base the web leaves bare ([`crate::takes_stress_mark`]); the
/// cell shader sees only the cluster's slot, not its base, and reads this.
/// Set for every such cell on screen, whether or not a record draws it.
/// `MASK_BARE` in the cell shader, pinned by
/// `the_bare_mask_bit_matches_the_shader`.
pub const MASK_BARE: u8 = 16;
/// `SIZED`: the cell is on a line that is not single-width
/// (`[[source: dec-std-070]]` § 5.6.1). The records draw the whole line
/// ([`Renderer::sized_lines`]) and the cell carries `RECORD` too; the
/// background pass draws the column from the cell under half of it, a
/// character's background under both its columns, and the decoration pass
/// draws nothing there. `MASK_SIZED` in the cell shader.
pub const MASK_SIZED: u8 = 4;

/// The word a syllable's [`ClusterKey`] ends with, its cell count in the
/// low bits: a syllable drawn across its cells is not the same codepoints
/// drawn as a cluster in one, nor a lam-alef pair's span (`0x8000_0000`)
/// or its lam cell (`0xC000_0000`).
const SYLLABLE: u32 = 0x4000_0000;

/// `BIDI`: the cell is on a row drawn in another order (`bidi.rs`), so
/// whether it is selected is not the selection's column range but
/// `SELECTED`, which the walk sets from the logical cell it shows -- the
/// instance path's `Highlight::selects` on that cell. `MASK_BIDI` and
/// `MASK_SELECTED` in the cell shader.
pub const MASK_BIDI: u8 = 32;
pub const MASK_SELECTED: u8 = 64;

/// **Who draws the block cursor**: the cell shader on its cell, or the
/// records, over the span of the Devanagari syllable that holds its cell
/// (`crate::word`), the shader then drawing none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorDrawn {
    Shader,
    Records,
}

/// What one frame's walk reads: the core for the marks, the view it is
/// drawing, the resolved rendition table the shader reads, and the cursor.
pub struct Scene<'s, 't> {
    pub term: &'s Term<'t>,
    pub view: Viewport<'s>,
    pub styles: crate::RowTables<'s>,
    pub cursor: Option<(Row, Col, CursorStyle)>,
    /// The selection on this view, or none: the same swap the cell passes
    /// apply, so a record over a selected cell is drawn in the swapped
    /// colour.
    pub highlight: Option<Highlight>,
    /// The glyph map the shader draws from, for what a cell draws alone.
    pub map: &'s GlyphMap<'s>,
    /// The cells that end an admitted pair, from the map's walk.
    pub candidates: &'s [Candidate],
    /// Smooth scroll: the pixels the cell-path grid is shifted down this
    /// frame, folded into every record's vertical offset so a mark or a
    /// cross-cell ligature slides with the glyph beneath it rather than
    /// holding its un-slid position for the length of the slide. `Px::of(0)`
    /// when not sliding; the instance path builds its own records in
    /// `Renderer::convert_row` and never reads this, so a draw-time pan on
    /// the shared record pipeline -- the coupling the first smooth-scroll
    /// withdrawal was about -- is not reintroduced.
    pub pan: Px,
    /// An input method's composition on this view, or none. The walk reads
    /// the cells it covers as empty, as the instance path's convert blanks
    /// them before it shapes: a ligature it covers is not drawn, and the
    /// half of one it leaves draws alone. [`Overlay::composed`] then masks
    /// them and draws the composition's own records.
    pub composition: Option<&'s Composition>,
}

use crate::renderer::PairGlyphs;

/// A window's cells, each with the range of the codepoints it draws.
type WindowCells = Vec<(usize, PackedCell, core::ops::Range<usize>)>;

/// **The cells a ligature window gathers, and the codepoints each draws**:
/// `piece` the window's cells, each with the range of `cps` that holds its
/// codepoints, and `cell` the cell being read, before the window takes it.
/// Emptied for each window, never freed.
#[derive(Default)]
struct Gather {
    piece: WindowCells,
    cps: Vec<Codepoint>,
    cell: Vec<Codepoint>,
}

/// The cluster map and the walk that fills the overlay from it.
pub struct Overlay {
    clusters: Map<ClusterKey, Vec<ClusterGlyph>>,
    /// A cluster's ink measured against its cell, by the key its glyphs
    /// are kept under: `None` where it is not over 1.1 cells wide
    /// (`crate::fit`), so a letter of a joining script is measured once
    /// and not on every frame.
    wides: Map<ClusterKey, Option<crate::fit::Overwide>>,
    /// A lam-alef pair's content to the glyphs its lam's cell and its
    /// alef's cell draw, or `None` where the two take different faces.
    pairs: Map<ClusterKey, Option<PairGlyphs>>,
    /// A window's content to what each of its cells draws: `None` where
    /// the map already draws it, the glyphs where it does not.
    windows: Map<ClusterKey, Vec<Option<Vec<ClusterGlyph>>>>,
    /// **A window's cells as [`Overlay::window`] gathers them**, reused
    /// from window to window and frame to frame, so gathering one
    /// allocates nothing once they have grown to the longest.
    gather: Gather,
    /// The atlas generation the kept slots belong to; a grow moves them.
    grown: Tally,
    stats: OverlayStats,
    /// The frame's pictures, found over the same view the walk reads.
    pictures: Pictures,
    /// The parts of the walk's glyphs above their own row's top, which the
    /// cell path draws after its decoration pass: each row's glyphs and
    /// then its decorations, rows top to bottom, is the instance path's
    /// order, so a glyph reaching up from the row below lies over the
    /// decorations of the row it reaches.
    over: Vec<Instance>,
    /// How many of the walk's records lie inside their rows, and are drawn
    /// between the cell path's glyph pass and its decoration pass.
    under: Tally,
    /// The cells the walk drew by records, in the order it drew them, with
    /// where each one's records lie in the two lists.
    emitted: Vec<Emitted>,
    /// Where [`Overlay::order`] puts the records while it reorders them.
    staged: Vec<Instance>,
    /// The lines that are not single-width, as the instance path converts
    /// them: their rows, kept, so a frame reuses the buffer; the renderer
    /// keeps their records ([`Renderer::sized_records`]).
    sized: Vec<Row>,
    /// The hovered link's fields the walk reads as empty ([`Covered`]),
    /// kept so a frame reuses the buffer.
    hovered: Vec<(Row, Col, Cols)>,
    /// The leaving row's own walk, made on the first slide
    /// ([`Overlay::leaving`]).
    slide: Option<Box<Slide>>,
    /// Who draws the block cursor this frame: the cell shader, or the
    /// records, where it sits on a Devanagari word's cell and lights its
    /// syllable's span (`crate::word`).
    cursor: CursorDrawn,
    /// Each window row's words and syllables as the last frame drew them
    /// ([`Painted`]).
    painted: Vec<Option<Painted>>,
}

/// **The walk of a smooth scroll's leaving row**, apart from the window's:
/// its own caches, and the pair candidates and the cells of several glyphs
/// the map's walk over that row found ([`Overlay::ensure_leaving`]), which
/// the window's walk replaces.
struct Slide {
    walk: Overlay,
    candidates: Vec<Candidate>,
    several: Vec<(Row, Col)>,
}

/// **A row's words and syllables as a frame drew them**, kept by window
/// row for the next frame to draw again while nothing they are drawn from
/// moved: the kept row (`crate::kept`), so its cells and every word's
/// layout; the slide's pan, the blink phases and the selection's colours;
/// each rendition the row's cells name, resolved; the pictures found on
/// it; and its row of the mask as the passes before left it. A row the
/// block cursor or the selection is on is painted again from its kept
/// layout each frame, never from these. What it drew -- its records, the
/// parts of them above the row, each cell drawn and its records' places
/// from the row's first, and the mask after -- is appended as drawn.
struct Painted {
    kept: crate::kept::KeptIx,
    pan: Px,
    blink: BlinkPhases,
    hilite: Hilite,
    styles: Vec<(baud_domain::StyleId, Resolved)>,
    /// The row's cells in a picture, where a frame found any.
    pictured: Option<Vec<Pictured>>,
    before: Vec<u8>,
    after: Vec<u8>,
    inside: Vec<Instance>,
    above: Vec<Instance>,
    emitted: Vec<Emitted>,
    cells: Tally,
}

/// What a row's [`Painted`] must match to be drawn again, but its styles,
/// pictures and mask, read where it is kept.
#[derive(Clone, Copy, PartialEq)]
struct PaintKey {
    kept: crate::kept::KeptIx,
    pan: Px,
    blink: BlinkPhases,
    hilite: Hilite,
}

impl Painted {
    /// Whether it was drawn under `key`, over `styles`, the row's
    /// `pictured` cells and its row of the mask `before`.
    fn matches(
        &self,
        key: PaintKey,
        styles: &[Resolved],
        pictured: Option<&[Pictured]>,
        before: &[u8],
    ) -> bool {
        key == (PaintKey {
            kept: self.kept,
            pan: self.pan,
            blink: self.blink,
            hilite: self.hilite,
        }) && self
            .styles
            .iter()
            .all(|&(id, res)| styles.get(id.index()).copied().unwrap_or_default() == res)
            && self.pictured.as_deref() == pictured
            && self.before == before
    }
}

/// One row's words for [`Overlay::words`]: its index, its cells as the walk
/// reads them, its words and the row kept that holds them laid
/// (`crate::kept`), and the composition's letters beside it.
type RowWords<'w> = (
    usize,
    &'w [PackedCell],
    (&'w [crate::word::Word], Option<crate::kept::KeptIx>),
    &'w Taken,
);

/// Whether the glyph map's last walk was over the view an overlay walk
/// reads: the window's walk follows the map's over the same view, and the
/// leaving row's follows the window's.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Walked {
    This,
    Elsewhere,
}

/// Whether a walk finds the pictures drawn in text on its view: the
/// window's does, and the leaving row's, which the cell path never takes
/// for one, does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Find {
    Pictures,
    None,
}

/// What a walk is given beyond its scene: the cells of several glyphs,
/// where it reads them from other than the map's last walk, and the window
/// a frame's build keyed (`Framed`), where one did.
type Given<'a> = (Option<&'a [(Row, Col)]>, Option<Framed>);

/// One cell the walk drew by records: where it is, its records' places in
/// the frame's records and in the parts above their rows, and the box
/// around their ink.
#[derive(Clone, PartialEq, Debug)]
struct Emitted {
    at: (Row, Col),
    inside: core::ops::Range<usize>,
    above: core::ops::Range<usize>,
    ink: Option<InkBox>,
}

impl Emitted {
    /// The cell at `at`, whose records are `records[inside]` and
    /// `over[above]`.
    fn of(
        at: (Row, Col),
        records: &[Instance],
        over: &[Instance],
        inside: core::ops::Range<usize>,
        above: core::ops::Range<usize>,
    ) -> Emitted {
        let ink = records[inside.clone()]
            .iter()
            .chain(&over[above.clone()])
            .map(InkBox::of)
            .reduce(InkBox::with);
        Emitted {
            at,
            inside,
            above,
            ink,
        }
    }

    /// The cell's place in the instance path's order: rows top to bottom,
    /// each left to right.
    fn order(&self) -> (Row, Col) {
        self.at
    }
}

/// A box of screen pixels, `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, PartialEq, Debug)]
struct InkBox {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}

impl InkBox {
    fn of(i: &Instance) -> InkBox {
        let (x, y) = (i32::from(i.x.0), i32::from(i.y.0));
        InkBox {
            x0: x,
            y0: y,
            x1: x + i32::from(i.w.0),
            y1: y + i32::from(i.h.0),
        }
    }

    fn with(self, o: InkBox) -> InkBox {
        InkBox {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    fn meets(self, o: InkBox) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

/// A cluster's content, folded: the base, its marks and its width.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ClusterKey(u64);

/// The cell geometry a record needs, and the screen's width.
struct Geom {
    m: Metrics,
    cols: Cols,
    /// The downward pixel shift a smooth scroll applies to every record on
    /// the cell path, matching the grid passes' `u.pan`.
    pan: Px,
}

/// **What the walk reads as empty**: the cells a composition draws on the
/// view, its row and the visual runs it is drawn in, or none; and the
/// fields of the link under the pointer ([`crate::HoverPaint`]), each a row
/// and a visual run, as the instance path blanks their cells. `hovered` is
/// the overlay's own buffer, lent for the frame, so a frame allocates
/// nothing for it, and with no link hovered every test is one empty check.
#[derive(Clone, Default)]
struct Covered {
    composition: Option<(usize, Vec<(Col, Cols)>)>,
    hovered: Vec<(Row, Col, Cols)>,
}

impl Covered {
    /// The visual cells `c` covers on a screen `cols` wide, laid as
    /// written ([`Laid::AsWritten`]) through the row's order -- one run on
    /// a row drawn in its own order, more where the order splits it -- and
    /// the letters written beside it that its words take in (`taken`);
    /// with the hovered link's fields, `hovered`.
    fn of(
        c: Option<&Composition>,
        cols: Cols,
        orders: &crate::bidi::Orders,
        taken: &Taken,
        hovered: Vec<(Row, Col, Cols)>,
    ) -> Covered {
        let composition = c.and_then(|c| {
            let (start, end) = c.columns(cols);
            (end > start).then(|| {
                let mut runs = orders.runs(c.row, Col::at_index(start), Col::at_index(end - 1));
                runs.extend(
                    taken
                        .beside
                        .iter()
                        .flatten()
                        .filter(|_| taken.row == Some(c.row))
                        .map(|&(at, n)| (Col::at_index(at), Cols::of_index(n))),
                );
                (c.row.index(), runs)
            })
        });
        Covered {
            composition,
            hovered,
        }
    }

    /// Whether it covers a cell of row `y`.
    fn on(&self, y: Row) -> bool {
        self.composition
            .as_ref()
            .is_some_and(|(row, _)| y.index() == *row)
            || (!self.hovered.is_empty() && self.hovered.iter().any(|&(row, _, _)| row == y))
    }

    fn holds(&self, y: Row, x: Col) -> bool {
        self.composes(y, x)
            || (!self.hovered.is_empty()
                && self.hovered.iter().any(|&(row, v, n)| {
                    row == y && (v.index()..v.index() + n.index()).contains(&x.index())
                }))
    }

    /// Whether the composition, and not a hovered field, covers cell `x` of
    /// row `y`: what a pair's partner is drawn as written from
    /// (`Renderer::written_cell`).
    fn composes(&self, y: Row, x: Col) -> bool {
        self.composition.as_ref().is_some_and(|(row, runs)| {
            y.index() == *row
                && runs
                    .iter()
                    .any(|&(v, n)| (v.index()..v.index() + n.index()).contains(&x.index()))
        })
    }
}

/// **Where a composition's cells are drawn**: as the same text written in
/// its row would be, through the order the row is laid out in with it
/// written there ([`crate::bidi::Written`]) -- an input method's -- or at
/// the screen columns it names, baud's own fields drawn over the text: the
/// find field, the link preview's bar and the config's notice.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Laid {
    AsWritten,
    OnScreen,
}

impl Laid {
    /// The visual column logical column `x` of row `r` is drawn at: the
    /// row's order's, as written, on a line of any rendition -- one that
    /// is not single-width is laid out over its half -- else `x`.
    fn column(self, orders: &crate::bidi::Orders, r: Row, x: Col) -> Col {
        match self {
            Laid::AsWritten => orders.visual_col(r, x),
            Laid::OnScreen => x,
        }
    }
}

/// What colours a record: the resolved rendition table, the selection and
/// the block cursor, each of which swaps its cells' colours.
struct Paint<'p> {
    styles: crate::RowTables<'p>,
    block: Option<(Row, Col, CursorStyle)>,
    highlight: Option<Highlight>,
    /// The frame's `SGR 5` and `SGR 6` phases: a record for a hidden cell is
    /// not drawn.
    blink: BlinkPhases,
    /// How much of the block cursor this frame draws
    /// ([`Renderer::cursor_opacity`]): a fade's records are its glyph part
    /// way to the swapped colour, as the cell under it is.
    cursor: Chan,
    /// The frame's pictures drawn in text, where the records are the
    /// screen's: a record in one takes the solid text, as the cell under it
    /// does on every arm ([`Resolved::drawn`]).
    pictures: Option<&'p Pictures>,
    /// The colours the selection draws in ([`Renderer::hilite`]).
    hilite: Hilite,
    /// The order the window's rows are drawn in, where the records are the
    /// screen's: a cell's selection is its logical cell's.
    orders: Option<&'p crate::bidi::Orders>,
    /// What the block cursor is drawn in ([`Renderer::block_paint`]): a
    /// record's glyph takes its text colour.
    block_paint: BlockPaint,
}

/// **How far the cell at visual `(x, y)` leans left, in `style`**: SAPV
/// 17's slant where the layout found it (`Orders::slants`), and what the
/// cell is then drawn in, its upright style.
fn slant_at(
    r: &Renderer<'_, '_>,
    paint: &Paint<'_>,
    (x, y): (usize, usize),
    style: crate::stack::TextFace,
) -> (Option<crate::stack::Lean>, crate::stack::TextFace) {
    let lean = paint
        .orders
        .filter(|o| o.slanted(Row::from_index(y), Col::at_index(x)))
        .and_then(|_| r.lean(style));
    (lean, lean.map_or(style, |_| style.upright()))
}

/// The opacity a record under `block` is drawn with: its style's, or all
/// of it where there is no cursor.
fn opacity_of(r: &Renderer<'_, '_>, block: Option<(Row, Col, CursorStyle)>) -> Chan {
    block.map_or(Chan::from_u8(0xFF), |(_, _, s)| r.cursor_opacity(s))
}

/// Distinct window contents kept before the store starts over.
const WINDOWS_MAX: usize = 8192;

impl ClusterKey {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn start() -> ClusterKey {
        ClusterKey(ClusterKey::OFFSET)
    }

    fn fold(self, c: Codepoint) -> ClusterKey {
        self.word(Code(c.value()))
    }

    /// Any word folded in: the face a window is shaped on, or the style a
    /// marked cluster is drawn in, so the same codepoints on two faces are
    /// two entries.
    fn word(self, w: Code) -> ClusterKey {
        let mut h = self.0;
        for b in w.0.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(ClusterKey::PRIME);
        }
        ClusterKey(h)
    }

    /// A joining form: a cell drawn in one is a different cluster from the
    /// same codepoints drawn alone.
    fn joined(self, form: baud_domain::JoiningForm) -> ClusterKey {
        let code: u64 = match form {
            baud_domain::JoiningForm::Isolated => 0xf0,
            baud_domain::JoiningForm::Final => 0xf1,
            baud_domain::JoiningForm::Initial => 0xf2,
            baud_domain::JoiningForm::Medial => 0xf3,
            baud_domain::JoiningForm::Final2 => 0xf4,
            baud_domain::JoiningForm::Final3 => 0xf5,
            baud_domain::JoiningForm::Medial2 => 0xf6,
        };
        ClusterKey((self.0 ^ code).wrapping_mul(ClusterKey::PRIME))
    }

    /// The cells of a joined cell's word around it, each its codepoints and
    /// its form, each side counted: a cell shaped with one word is a
    /// different cluster from it shaped with another, or alone.
    fn around(self, word: &crate::renderer::Word) -> ClusterKey {
        if word.is_empty() {
            return self;
        }
        let mut key = self.word(Code(
            0xA000_0000 | u32::try_from(word.before.len()).unwrap_or(0),
        ));
        for (cps, form) in word.before.iter().chain(&word.after) {
            key = key.next_cell();
            for &c in cps {
                key = key.fold(c);
            }
            if let Some(f) = form {
                key = key.joined(*f);
            }
        }
        key.word(Code(
            0xB000_0000 | u32::try_from(word.after.len()).unwrap_or(0),
        ))
    }

    /// A cell fitted to its one cell by rule D (`crate::fit`): a different
    /// cluster from the same codepoints drawn full size.
    fn fitted(self) -> ClusterKey {
        ClusterKey((self.0 ^ 0xfa).wrapping_mul(ClusterKey::PRIME))
    }

    /// A cell drawn flipped left to right (UAX #9 L4): a different cluster
    /// from the same codepoints drawn as they are.
    fn flipped(self) -> ClusterKey {
        ClusterKey((self.0 ^ 0xf8).wrapping_mul(ClusterKey::PRIME))
    }

    /// A cell SAPV 17 slants left, by `lean`: a different cluster from the
    /// same codepoints drawn upright.
    fn sheared(self, lean: crate::stack::Lean) -> ClusterKey {
        let bits = u64::from(lean.shear().to_bits()) << 8;
        ClusterKey((self.0 ^ 0xf9 ^ bits).wrapping_mul(ClusterKey::PRIME))
    }

    /// A lam-alef pair, its lam in `form`: two cells drawn together.
    fn paired(self, form: baud_domain::JoiningForm) -> ClusterKey {
        ClusterKey((self.joined(form).0 ^ 0x08).wrapping_mul(ClusterKey::PRIME))
    }

    /// A wide base is a different cluster from a narrow one with the
    /// same codepoints: one byte past any codepoint's.
    fn wide(self) -> ClusterKey {
        ClusterKey((self.0 ^ 0xff).wrapping_mul(ClusterKey::PRIME))
    }

    /// The boundary between two cells of a window, so `[a b] [c]` and
    /// `[a] [b c]` are different windows.
    fn next_cell(self) -> ClusterKey {
        ClusterKey((self.0 ^ 0xfe).wrapping_mul(ClusterKey::PRIME))
    }
}

impl Default for Overlay {
    fn default() -> Overlay {
        Overlay::new()
    }
}

impl Overlay {
    pub fn new() -> Overlay {
        Overlay {
            clusters: Map::default(),
            wides: Map::default(),
            pairs: Map::default(),
            windows: Map::default(),
            gather: Gather::default(),
            grown: Tally::default(),
            stats: OverlayStats::default(),
            pictures: Pictures::new(),
            over: Vec::new(),
            under: Tally::default(),
            emitted: Vec::new(),
            staged: Vec::new(),
            sized: Vec::new(),
            hovered: Vec::new(),
            slide: None,
            cursor: CursorDrawn::Shader,
            painted: Vec::new(),
        }
    }

    /// **Who draws the block cursor** the last [`Overlay::ensure`] walked:
    /// the records, where it sits on a Devanagari word's cell -- its
    /// syllable's span lit by them, so the cell shader must draw no block
    /// -- else the shader.
    pub fn cursor_drawn(&self) -> CursorDrawn {
        self.cursor
    }

    pub fn stats(&self) -> OverlayStats {
        self.stats
    }

    /// How many of the records [`Overlay::ensure`] last wrote lie inside
    /// their own rows: the cell path draws those between its glyph pass
    /// and its decoration pass, and the rest -- the parts of glyphs above
    /// their rows, and whatever the caller appends -- after it.
    pub fn under(&self) -> Tally {
        self.under
    }

    /// One frame: `mask` is one byte a screen cell in screen order, at
    /// least `cols * rows` of them, and `records` is refilled with what
    /// draws the masked cells. `styles` is the resolved rendition table
    /// the shader reads, so a record's colour is the one the shader would
    /// have used, with the block cursor's swap.
    pub fn ensure(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        self.ensure_with(scene, (None, None), Find::Pictures, r, mask, records);
    }

    /// **[`Overlay::ensure`] for the frame a build keyed** (`framed`,
    /// FS-C5): its sized lines are keyed by where their rows are stored and
    /// the build's seal (`Renderer::sized_framed`), where `ensure`, which
    /// may be handed a window of an older copy, compares their cells.
    pub(crate) fn ensure_framed(
        &mut self,
        scene: &Scene<'_, '_>,
        framed: Framed,
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        self.ensure_with(
            scene,
            (None, Some(framed)),
            Find::Pictures,
            r,
            mask,
            records,
        );
    }

    /// [`Overlay::ensure`], reading the cells of several glyphs from
    /// `several` where given rather than from the map's last walk, the
    /// pictures as `find` says, and its sized lines keyed as `framed` says.
    fn ensure_with(
        &mut self,
        scene: &Scene<'_, '_>,
        (several, framed): Given<'_>,
        find: Find,
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        // The pictures first, from the term, the view and the video alone:
        // a record in one is coloured as the cell under it is.
        let view = &scene.view;
        let (cols, rows) = (view.cols(), view.rows());
        self.stats.pictured = if find == Find::Pictures && r.theme().finds_pictures() {
            // The frame's copy, read through its lent slots' words where it
            // lent any, else each row through its own table.
            let frame = r.frame_held();
            let lent = !frame.words().is_live();
            let words = |y: Row| match lent {
                true => frame.words(),
                false => Words::Row(view.styles(y)),
            };
            self.pictures.find_in(scene.term, view, words, r.video())
        } else {
            self.pictures.clear(cols, rows);
            Tally::default()
        };
        let pictures = core::mem::take(&mut self.pictures);
        // Twice at most: a cluster resolved on this walk can grow the
        // atlas, which moves every slot resolved before it.
        for _ in 0..2 {
            if r.atlas_stats().grown.value() != self.grown.value() {
                self.grown = r.atlas_stats().grown;
                self.clusters.clear();
                self.wides.clear();
                self.pairs.clear();
                self.windows.clear();
                self.painted.clear();
            }
            // The map's last walk was over this view where the caller gives
            // no cells of several glyphs of its own: the window's.
            let walked = match several {
                Some(_) => Walked::Elsewhere,
                None => Walked::This,
            };
            let several = several.unwrap_or_else(|| scene.map.several());
            self.walk(
                scene,
                (several, walked, framed),
                r,
                mask,
                records,
                &pictures,
            );
            if r.atlas_stats().grown.value() == self.grown.value() {
                break;
            }
        }
        self.pictures = pictures;
        self.stats.records = Tally::of(records.len());
        self.stats.clusters = Tally::of(self.clusters.len());
        // Last, over the mask the walk just filled: the picture bit rides
        // beside the record bit, and the walk's own tests read the record
        // bit alone.
        if self.stats.pictured.value() > 0 {
            let w = cols.index();
            crate::counters::add(
                Counter::PictureCellsScanned,
                Site::Pictures,
                Tally::of(w * rows.index()),
            );
            for y in 0..rows.index() {
                for x in 0..w {
                    if self.pictures.at(Row::from_index(y), Col::at_index(x)) != Pictured::Yes {
                        continue;
                    }
                    if let Some(b) = mask.get_mut(y * w + x) {
                        *b |= MASK_PICTURE;
                    }
                }
            }
        }
    }

    fn walk(
        &mut self,
        scene: &Scene<'_, '_>,
        (several, walked, framed): (&[(Row, Col)], Walked, Option<Framed>),
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        pictures: &Pictures,
    ) {
        let (term, view, styles, cursor) = (scene.term, scene.view, scene.styles, scene.cursor);
        // The words the frame's lent slots name (`crate::frozen`).
        let frame = r.frame_held();
        self.over.clear();
        self.emitted.clear();
        records.clear();
        self.stats.cells = Tally::default();
        self.stats.misses = Tally::default();
        self.stats.windows = Tally::default();
        self.stats.window_misses = Tally::default();
        self.stats.window_cells = Tally::default();
        let cols = view.cols().index();
        let rows = view.rows().index();
        // The row after the screen's too, where present: the leaving row's
        // ([`Overlay::leaving`]), set again on each frame that slides.
        if let Some(leaving) =
            mask.get_mut(cols.saturating_mul(rows)..cols.saturating_mul(rows + 1))
        {
            leaving.fill(0);
        }
        let Some(mask) = mask.get_mut(..cols.saturating_mul(rows)) else {
            return;
        };
        mask.fill(0);
        let geom = Geom {
            m: r.metrics(),
            cols: view.cols(),
            pan: scene.pan,
        };
        // **The order the rows are drawn in**, this frame's: the view is the
        // window in that order (`Orders::window_kept`), and the cursor and the
        // selection, kept by logical cells, are mapped through it. A copy,
        // so the walk may borrow the renderer mutably; none while no row
        // is reordered.
        let orders = match r.bidi().any() || r.bidi().any_joined() || r.bidi().display().is_some() {
            true => r.bidi().clone(),
            false => crate::bidi::Orders::default(),
        };
        let block = cursor
            .filter(|(_, _, s)| {
                r.cursor_look() == CursorLook::Shaped
                    && matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock)
            })
            .map(|(row, c, s)| (row, orders.visual_col(row, c), s));
        // A reordered row's selection, cell by cell, for the shader: the
        // logical cell each visual cell shows, asked as the instance path
        // asks it.
        // On a display line ([`crate::rebreak`]) every cell, by the stored
        // cell it shows, which the selection is kept by.
        let displayed = orders.display().is_some();
        if orders.any() || displayed {
            for y in 0..rows {
                let row = Row::from_index(y);
                let same = orders.display().is_some_and(|d| d.same(row) == Some(row));
                if !orders.reordered(row) && (!displayed || same) {
                    continue;
                }
                crate::counters::add(Counter::OverlayCellsBuilt, Site::Overlay, Tally::of(cols));
                for (x, cell) in view.row(row).iter().enumerate().take(cols) {
                    let l = orders.logical_col(row, Col::at_index(x));
                    let sel = scene
                        .highlight
                        .is_some_and(|h| orders.selects(&h, row, l, cell.content));
                    mask[y * cols + x] |= MASK_BIDI | if sel { MASK_SELECTED } else { 0 };
                }
            }
        }
        // The letters written beside the composition that its words take
        // in ([`Taken`]): read as empty with its cells, and what its cells
        // will hold read by the row's other words.
        let mut taken = scene
            .composition
            .filter(|c| c.row.index() < view.rows().index())
            .map_or_else(Taken::default, |c| {
                c.taken(term, &orders, c.width_on(&view), view.rendition(c.row))
            });
        // The hovered link's fields, read as empty too, on the window's walk
        // alone -- a leaving row walked elsewhere is not their row: in the
        // overlay's own buffer, given back after the walk.
        let mut hovered = core::mem::take(&mut self.hovered);
        hovered.clear();
        if walked == Walked::This {
            hovered.extend(r.hover_spans(view.cols()));
        }
        let covered = Covered::of(
            scene.composition,
            scene.composition.map_or(view.cols(), |c| c.width_on(&view)),
            &orders,
            &taken,
            hovered,
        );
        // Its words as it lays them, which the row's word after one closes
        // on where that fits at no width: laid here, before the row's.
        if let Some(c) = scene
            .composition
            .filter(|c| taken.on(c.row) && c.text.iter().any(|&cp| crate::word::is_letter(cp)))
        {
            let under = view
                .row(c.row)
                .get(r.bidi().visual_col(c.row, c.col).index())
                .copied()
                .unwrap_or(PackedCell::EMPTY);
            let laid = composition_parts(
                r,
                term,
                (c, Laid::AsWritten),
                under,
                styles.at(c.row),
                cursor,
                view.cols(),
                view.rendition(c.row),
                scene.pan,
            );
            taken.set_inks(laid.inks);
        }
        if self.windows.len() > WINDOWS_MAX {
            self.windows.clear();
        }
        // **The lines that are not single-width**, drawn whole by the records
        // the instance path converts them to -- glyphs at their doubled size,
        // decorations and the cursor through the line's geometry -- with
        // every cell masked and marked sized, so the shader draws their
        // backgrounds from half the column and nothing else of them.
        let mut sized = core::mem::take(&mut self.sized);
        r.sized_framed(
            term,
            (&view, scene.styles),
            framed,
            cursor,
            scene.composition,
            &mut sized,
        );
        let (cell_h, pan) = (
            i32::try_from(geom.m.cell_h.value()).unwrap_or(0),
            i32::try_from(geom.pan.value()).unwrap_or(0),
        );
        for &row in &sized {
            let insts = r.sized_records(row);
            let y = row.index();
            for b in &mut mask[y * cols..(y + 1) * cols] {
                *b |= MASK_RECORD | MASK_SIZED;
            }
            let top = Screen(
                i16::try_from(i32::try_from(y).unwrap_or(0) * cell_h + pan).unwrap_or(i16::MAX),
            );
            let (inside, above) = (records.len(), self.over.len());
            for inst in insts {
                let inst = Instance {
                    y: Screen(i16::try_from(i32::from(inst.y.0) + pan).unwrap_or(i16::MAX)),
                    ..*inst
                };
                let (a, i) = inst.cut_at(top);
                self.over.extend(a);
                records.extend(i);
            }
            self.emitted.push(Emitted::of(
                (row, Col::ZERO),
                records,
                &self.over,
                inside..records.len(),
                above..self.over.len(),
            ));
        }
        self.sized = sized;
        // The windows: around each admitted pair, as wide as the face's
        // longest rule, merged where they touch, shaped as one run and
        // cached by content.
        // `ceiling-no-windows` prices the ligature half by not walking it.
        let mut i = if cfg!(feature = "ceiling-no-windows") {
            scene.candidates.len()
        } else {
            0
        };
        while i < scene.candidates.len() {
            let y = scene.candidates[i].row.index();
            let mut ranges: Vec<(usize, usize)> = Vec::new();
            // A sized line's records are the instance path's own, its
            // ligatures among them.
            let sized_row = view.rendition(Row::from_index(y)).is_double();
            while i < scene.candidates.len() && scene.candidates[i].row.index() == y {
                let c = scene.candidates[i].col.index();
                let face = scene.candidates[i].face;
                i += 1;
                // **No window crosses a level run** (UAX #9 3.5), nor forms
                // inside a right-to-left one: a pair across a seam is none,
                // and a window stops at the seams either side of its pair.
                let row = Row::from_index(y);
                if orders.seam(row, Col::at_index(c)) {
                    continue;
                }
                let (from, to) = match orders.reordered(row) {
                    true => {
                        let from = (0..=c)
                            .rev()
                            .find(|&x| orders.seam(row, Col::at_index(x)))
                            .unwrap_or(0);
                        let to = (c + 1..cols)
                            .find(|&x| orders.seam(row, Col::at_index(x)))
                            .map_or(cols.saturating_sub(1), |x| x - 1);
                        (from, to)
                    }
                    false => (0, cols.saturating_sub(1)),
                };
                let ext = r
                    .extent(face)
                    .and_then(|e| usize::try_from(e.value()).ok())
                    .unwrap_or(1)
                    .max(1);
                let lo = c.saturating_sub(ext).max(from);
                let hi = (c + ext - 1).min(cols.saturating_sub(1)).min(to);
                match ranges.last_mut() {
                    Some(last) if lo <= last.1 + 1 => last.1 = last.1.max(hi),
                    _ => ranges.push((lo, hi)),
                }
            }
            if sized_row {
                continue;
            }
            for (lo, hi) in ranges {
                self.window(
                    scene,
                    r,
                    Row::from_index(y),
                    (lo, hi),
                    &geom,
                    block,
                    &covered,
                    mask,
                    records,
                    (pictures, &orders),
                );
            }
        }
        // The marked cells the windows did not cover; and whether a cell on
        // screen blinks, which is `Renderer::blink_seen`'s answer on this
        // path. Tested only while the table holds a blinking slot at all,
        // so a screen with none pays one compare a frame.
        let blinks = r.blink_slots() != Blinking::UNSEEN;
        let mut seen = Blinking::UNSEEN;
        let paint = Paint {
            styles,
            block,
            highlight: scene.highlight,
            blink: r.blink(),
            cursor: opacity_of(r, block),
            pictures: Some(pictures),
            hilite: r.hilite(),
            block_paint: r.block_paint(),
            orders: Some(&orders),
        };
        // **The mirrored cells L4 draws other than by their content**,
        // before the marked ones, so a mirrored base under its marks is
        // drawn with its mirror: each by a record, flipped or with the
        // mirror for its base (`Orders::mirrored`). A list a reordered row,
        // so a screen with none pays one test a row.
        if orders.any() {
            for y in 0..rows {
                let row = Row::from_index(y);
                if view.rendition(row).is_double() {
                    continue;
                }
                for &(v, m) in orders.mirrors(row) {
                    let x = v.index();
                    if mask.get(y * cols + x).is_none_or(|b| b & MASK_RECORD != 0)
                        || covered.holds(row, v)
                    {
                        continue;
                    }
                    self.whole_mirrored(scene, r, (x, y), m, mask, records, &paint, &geom);
                }
            }
        }
        // **The joined cells**, before the marked ones, so a letter that
        // carries marks is drawn in its form too: each by a record of its
        // form, a lam-alef pair as one (`bidi.rs`). A list of rows, so a
        // screen with none pays one test.
        if orders.any_joined() {
            for y in 0..rows {
                let row = Row::from_index(y);
                if orders.joins(row).is_none() || view.rendition(row).is_double() {
                    continue;
                }
                let mut x = 0;
                while x < cols {
                    let held = |x: usize| {
                        mask.get(y * cols + x).is_none_or(|m| m & MASK_RECORD != 0)
                            || covered.holds(row, Col::at_index(x))
                    };
                    let Some(j) = orders.joined(row, Col::at_index(x)).filter(|_| !held(x)) else {
                        x += 1;
                        continue;
                    };
                    // A pair's partner on this row is masked when the pair
                    // is drawn, and so passed over as held.
                    let partner_held = match j.pair {
                        Some((
                            crate::bidi::PairPart::Lam(n, _) | crate::bidi::PairPart::Alef(n, _),
                            _,
                        )) => held(x + n.index().saturating_sub(1)),
                        _ => false,
                    };
                    if j.pair.is_some()
                        && self.pair(
                            scene,
                            r,
                            (x, y),
                            (&orders, partner_held, &covered),
                            mask,
                            records,
                            &paint,
                            &geom,
                        )
                    {
                        x += 1;
                        continue;
                    }
                    let around = orders.around(row, &j);
                    self.whole_joined(
                        scene,
                        r,
                        (x, y),
                        (j.form, around),
                        mask,
                        records,
                        &paint,
                        &geom,
                    );
                    x += 1;
                }
            }
        }
        // **The cells SAPV 17 slants left** that neither list above drew,
        // before the marked ones: each by a record of its upright glyph
        // sheared ([`Renderer::shear_glyphs`]). A list a reordered row, so a
        // screen with none pays one test a row.
        if orders.any() {
            for y in 0..rows {
                let row = Row::from_index(y);
                if view.rendition(row).is_double() {
                    continue;
                }
                for &v in orders.slants(row) {
                    let x = v.index();
                    if mask.get(y * cols + x).is_none_or(|b| b & MASK_RECORD != 0)
                        || covered.holds(row, v)
                    {
                        continue;
                    }
                    self.whole(scene, r, (x, y), mask, records, &paint, &geom);
                }
            }
        }
        // **The Devanagari words** (`crate::word`), then the syllables no
        // word holds (`crate::syllable`), before the marked cells: each
        // drawn whole by records, every one of its cells masked, as the
        // instance path draws it from the same glyphs. A composition's
        // cells are read as empty, as the instance path blanks them.
        // A row that holds no character a syllable or a word is made of is
        // let go before it is copied: as this frame's layout found it in its
        // own pass over the row, where the view is the window it laid out --
        // no display re-breaks it -- else as the glyph map's walk of the view
        // found it on the entries it probed (`GlyphMap::row_parts`), the
        // marks of a row that holds a cluster read here, else by that pass
        // of a block table (`crate::syllable::may_hold`). Read with a
        // composition's cells empty, it holds no more.
        self.cursor = CursorDrawn::Shader;
        self.stats.walked = Tally::default();
        self.stats.folded = Tally::default();
        let laid = r.bidi().display().is_none();
        let walked_as = (view.cols(), view.rows());
        for y in 0..rows {
            let at = Row::from_index(y);
            if view.rendition(at).is_double() {
                continue;
            }
            let cells = view.row(at);
            let held = match r.bidi().syllables(at) {
                crate::bidi::Opens::Yes if laid => true,
                crate::bidi::Opens::No if laid => false,
                _ => {
                    let cells = &cells[..cols.min(cells.len())];
                    let parts = match walked {
                        Walked::This => scene.map.row_parts(at, walked_as),
                        Walked::Elsewhere => None,
                    };
                    match parts {
                        Some(crate::cellmap::RowParts::Neither) => false,
                        Some(crate::cellmap::RowParts::Blocks) => true,
                        Some(crate::cellmap::RowParts::Clusters) => {
                            crate::syllable::clusters_may_hold(term, cells)
                        }
                        None => {
                            self.stats.folded.bump();
                            crate::syllable::may_hold(term, cells)
                        }
                    }
                }
            };
            if !held {
                continue;
            }
            // The row as the walk reads it: copied only where a composition
            // covers some of it.
            let blanked: Vec<PackedCell>;
            let row: &[PackedCell] = match covered.on(at) {
                true => {
                    blanked = cells
                        .iter()
                        .take(cols)
                        .enumerate()
                        .map(|(x, c)| match covered.holds(at, Col::at_index(x)) {
                            true => PackedCell::EMPTY,
                            false => *c,
                        })
                        .collect();
                    &blanked
                }
                false => &cells[..cols.min(cells.len())],
            };
            let logical = |v: Col| orders.logical_col(at, v);
            let find = || {
                let (parted, letters) = crate::syllable::walk((term, frame.words()), row, logical);
                let words = match letters {
                    crate::word::Letters::Seen => {
                        crate::word::of_row(term, row, logical, &parted, |x| {
                            taken.held(at, Col::at_index(x))
                        })
                    }
                    crate::word::Letters::None => Vec::new(),
                };
                crate::kept::Found { parted, words }
            };
            // **Kept while the row is the same** (`crate::kept`): a row
            // which no display re-breaks and no composition covers is walked
            // and its words laid only where no row kept holds its cells in
            // the order they are drawn in.
            let keep = laid && !covered.on(at) && !taken.on(at);
            let order = orders.logical_cols(at).unwrap_or(&[]);
            let (kept, found, asked) = match keep {
                true => {
                    let (k, f, asked) = r.kept_row(term, (at, view.rows()), (row, order), find);
                    (Some(k), f, asked)
                }
                false => (None, std::rc::Rc::new(find()), crate::kept::Asked::Found),
            };
            if asked == crate::kept::Asked::Found {
                self.stats.walked.bump();
            }
            // **Drawn again as the last frame drew it** ([`Painted`]), where
            // the row is kept and was on this window row the last frame, no
            // light is on it and nothing it is drawn from moved; kept for
            // the next frame from the second frame a row stays. A debug
            // build draws it, and requires what it draws to be what was
            // kept.
            let span = y * cols..(y + 1) * cols;
            let unlit = paint.block.is_none_or(|(cr, _, _)| cr != at)
                && paint.highlight.is_none_or(|h| !h.covers_row(at));
            let here = asked == crate::kept::Asked::Here;
            let key = kept.filter(|_| unlit && here).map(|k| PaintKey {
                kept: k,
                pan: geom.pan,
                blink: paint.blink,
                hilite: paint.hilite,
            });
            if self.painted.len() < rows {
                self.painted.resize_with(rows, || None);
            }
            let pictured = (self.stats.pictured.value() > 0).then(|| pictures.row(at));
            let again = key.and_then(|k| {
                self.painted[y]
                    .take()
                    .filter(|p| p.matches(k, styles.at(at), pictured, &mask[span.clone()]))
            });
            if let Some(p) = again.as_ref().filter(|_| !cfg!(debug_assertions)) {
                let (inside, above) = (records.len(), self.over.len());
                records.extend_from_slice(&p.inside);
                self.over.extend_from_slice(&p.above);
                self.emitted.extend(p.emitted.iter().map(|e| Emitted {
                    at: e.at,
                    inside: e.inside.start + inside..e.inside.end + inside,
                    above: e.above.start + above..e.above.end + above,
                    ink: e.ink,
                }));
                mask[span].copy_from_slice(&p.after);
                self.stats.cells.add(p.cells);
                self.painted[y] = again;
                continue;
            }
            let before = key.map(|_| mask[span.clone()].to_vec());
            let start = (records.len(), self.over.len(), self.emitted.len());
            let cells_before = self.stats.cells;
            if !found.words.is_empty() {
                self.words(
                    scene,
                    r,
                    (y, row, (&found.words, kept), &taken),
                    mask,
                    records,
                    &paint,
                    &geom,
                );
            }
            for &s in &found.parted {
                let from = s.at.index();
                let held = found.words.iter().any(|w| w.holds(s.at))
                    || (from..from + s.cells.index()).any(|x| {
                        mask.get(y * cols + x).is_none_or(|m| m & MASK_RECORD != 0)
                            || covered.holds(at, Col::at_index(x))
                    });
                if !held {
                    self.syllable(scene, r, (from, y), s.cells, mask, records, &paint, &geom);
                }
            }
            let (Some(key), Some(before)) = (key, before) else {
                continue;
            };
            let (inside, above) = (start.0, start.1);
            let mut styles_of: Vec<(baud_domain::StyleId, Resolved)> = Vec::new();
            for (i, c) in row.iter().enumerate() {
                let run = i > 0 && row[i - 1].style == c.style;
                if !run && !styles_of.iter().any(|&(id, _)| id == c.style) {
                    let res = styles
                        .at(at)
                        .get(c.style.index())
                        .copied()
                        .unwrap_or_default();
                    styles_of.push((c.style, res));
                }
            }
            let painted = Painted {
                kept: key.kept,
                pan: key.pan,
                blink: key.blink,
                hilite: key.hilite,
                styles: styles_of,
                pictured: pictured.map(<[Pictured]>::to_vec),
                before,
                after: mask[span].to_vec(),
                inside: records[inside..].to_vec(),
                above: self.over[above..].to_vec(),
                emitted: self.emitted[start.2..]
                    .iter()
                    .map(|e| Emitted {
                        at: e.at,
                        inside: e.inside.start - inside..e.inside.end - inside,
                        above: e.above.start - above..e.above.end - above,
                        ink: e.ink,
                    })
                    .collect(),
                cells: Tally::of(
                    usize::try_from(self.stats.cells.value() - cells_before.value()).unwrap_or(0),
                ),
            };
            if let Some(p) = &again {
                debug_assert!(
                    (&p.inside, &p.above, &p.emitted, &p.after, p.cells)
                        == (
                            &painted.inside,
                            &painted.above,
                            &painted.emitted,
                            &painted.after,
                            painted.cells
                        ),
                    "a row drawn again draws what was kept"
                );
            }
            self.painted[y] = Some(painted);
        }
        crate::counters::add(
            Counter::OverlayCellsBuilt,
            Site::Overlay,
            Tally::of(rows * cols),
        );
        for y in 0..rows {
            let row = view.row(Row::from_index(y));
            for (x, cell) in row.iter().enumerate().take(cols) {
                if blinks {
                    if let Some(s) = styles.at(Row::from_index(y)).get(cell.style.index()) {
                        seen = seen.with(Blinking::of(s.shapes));
                    }
                }
                if cell.content.cluster_id().is_some()
                    && !crate::takes_stress_mark(term.base_of(*cell), term.cluster_marks(*cell))
                {
                    mask[y * cols + x] |= MASK_BARE;
                }
                if !cell.flags.is_marked()
                    || mask[y * cols + x] & MASK_RECORD != 0
                    || covered.holds(Row::from_index(y), Col::at_index(x))
                {
                    continue;
                }
                self.whole(scene, r, (x, y), mask, records, &paint, &geom);
            }
        }
        // Then the cells whose codepoint shapes to several glyphs alone --
        // a precomposed letter its face lacks, drawn decomposed -- which
        // the map's walk listed (`GlyphMap::several`): an entry draws one
        // glyph, so they are drawn whole, as a marked cell is. A list, so
        // a screen without one pays nothing a cell.
        for &(y, x) in several {
            let (y, x) = (y.index(), x.index());
            let Some(cell) = view.row(Row::from_index(y)).get(x) else {
                continue;
            };
            if cell.flags.is_marked()
                || mask.get(y * cols + x).is_none_or(|m| m & MASK_RECORD != 0)
                || covered.holds(Row::from_index(y), Col::at_index(x))
            {
                continue;
            }
            self.whole(scene, r, (x, y), mask, records, &paint, &geom);
        }
        if !self.emitted.is_empty() {
            self.reach(scene, r, mask, records, &paint, &geom, &covered);
            self.order(records);
        }
        #[cfg(feature = "counters")]
        crate::counters::add(Counter::OverlayCellsBuilt, Site::Overlay, {
            let mut built = self.stats.cells;
            built.add(self.stats.window_cells);
            built
        });
        self.under = Tally::of(records.len());
        records.append(&mut self.over);
        r.set_blink_seen(seen);
        self.hovered = covered.hovered;
    }

    /// **The map's glyphs a record's ink meets that the instance path draws
    /// after it, drawn by records too.** The cell path draws every map
    /// glyph before any record, and the instance path each row's glyphs
    /// left to right, rows top to bottom, so a map glyph later in that
    /// order -- to the record's right, or in a row below -- whose ink
    /// meets the record's would lie under it on the one path and over it
    /// on the other. Such a cell is drawn by a record of its own, and is
    /// met in turn, so a run of glyphs that each meet the next goes whole.
    /// A record draws what the map would have, so a box that meets but
    /// whose ink does not costs a record and no pixel. Looked for among
    /// the cells within the map's reach of each record's ink
    /// ([`GlyphMap::reach`]).
    #[allow(clippy::too_many_arguments)]
    fn reach(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
        covered: &Covered,
    ) {
        let px = |p: Px| i32::try_from(p.value()).unwrap_or(0);
        let (cw, ch) = (px(geom.m.cell_w).max(1), px(geom.m.cell_h).max(1));
        let (cols, rows) = (geom.cols.index(), scene.view.rows().index());
        let origin = px(geom.m.baseline) + px(geom.pan);
        // A map glyph lies within the map's reach of its cell's origin, so
        // only the cells within that reach of a record's ink are probed:
        // none, around a record inside its cell, on a screen of glyphs
        // that keep to theirs.
        let reach = scene.map.reach();
        let (left, right) = (i32::from(reach.left.0), i32::from(reach.right.0));
        let (top, bottom) = (i32::from(reach.top.0), i32::from(reach.bottom.0));
        let index = |v: i32, last: usize| usize::try_from(v.max(0)).unwrap_or(0).min(last);
        // Every record once, the cells it draws included: a cell drawn
        // here is met in turn.
        let mut k = 0;
        while let Some(e) = self.emitted.get(k) {
            k += 1;
            let ((y, x), Some(ink)) = (e.at, e.ink) else {
                continue;
            };
            // Most records keep clear of every later cell's reach: the
            // nearest such glyph starts at the next cell's left plus the
            // reach, or at the next row's top plus it.
            let (ox, oy) = (
                i32::try_from(x.index() + 1).unwrap_or(i32::MAX),
                i32::try_from(y.index() + 1).unwrap_or(i32::MAX),
            );
            if ink.x1 <= ox.saturating_mul(cw).saturating_add(left)
                && ink.y1 <= oy.saturating_mul(ch).saturating_add(origin + top)
            {
                continue;
            }
            let first_row = (ink.y0 - origin - bottom).div_euclid(ch) + 1;
            let last_row = (ink.y1 - 1 - origin - top).div_euclid(ch);
            let first_col = (ink.x0 - right).div_euclid(cw) + 1;
            let last_col = (ink.x1 - 1 - left).div_euclid(cw);
            if last_row < i32::try_from(y.index()).unwrap_or(i32::MAX) || last_col < 0 {
                continue;
            }
            let rows_in = index(first_row, rows - 1).max(y.index())..=index(last_row, rows - 1);
            let cols_in = index(first_col, cols - 1)..=index(last_col, cols - 1);
            for yy in rows_in {
                let row = scene.view.row(Row::from_index(yy));
                for xx in cols_in.clone() {
                    if yy == y.index() && xx <= x.index() {
                        continue;
                    }
                    let Some(cell) = row.get(xx) else {
                        continue;
                    };
                    if cell.content == Content::WIDE_TAIL
                        || mask[yy * cols + xx] & MASK_RECORD != 0
                        || covered.holds(Row::from_index(yy), Col::at_index(xx))
                    {
                        continue;
                    }
                    let Some(cp) = crate::renderer::drawn_base(scene.term, *cell) else {
                        continue;
                    };
                    let style =
                        crate::cellmap::face_of(scene.styles.at(Row::from_index(yy)), *cell);
                    let Some(e) = scene
                        .map
                        .probe(cp, style, CellSpan::at(row, Col::at_index(xx)))
                        .filter(|e| e.kind().is_some())
                    else {
                        continue;
                    };
                    let left = i32::try_from(xx).unwrap_or(0) * cw + i32::from(e.left.0);
                    let top = i32::try_from(yy).unwrap_or(0) * ch + origin + i32::from(e.top.0);
                    let glyph = InkBox {
                        x0: left,
                        y0: top,
                        x1: left + i32::from(e.w.0),
                        y1: top + i32::from(e.h.0),
                    };
                    if glyph.meets(ink) {
                        self.whole(scene, r, (xx, yy), mask, records, paint, geom);
                    }
                }
            }
        }
    }

    /// The walk's records put in the instance path's order, cell by cell:
    /// rows top to bottom, each left to right, a cell's own glyphs in the
    /// order it drew them. The windows are drawn before the marked cells
    /// and [`Overlay::reach`]'s cells after both, so two records whose ink
    /// meets would otherwise lie the other way round.
    fn order(&mut self, records: &mut Vec<Instance>) {
        if self.emitted.is_sorted_by_key(Emitted::order) {
            return;
        }
        // A cell is drawn by records once, so no two keys are equal.
        self.emitted.sort_unstable_by_key(Emitted::order);
        // Through one kept buffer, so a frame allocates nothing here.
        self.staged.clear();
        for e in &self.emitted {
            self.staged.extend_from_slice(&records[e.inside.clone()]);
        }
        records.clear();
        records.extend_from_slice(&self.staged);
        self.staged.clear();
        for e in &self.emitted {
            self.staged.extend_from_slice(&self.over[e.above.clone()]);
        }
        self.over.clear();
        self.over.extend_from_slice(&self.staged);
    }

    /// **The cell at `at`'s glyphs as rule D draws them** (`crate::fit`):
    /// `glyphs`, kept under `key`, unless its `base` is a letter of a
    /// joining script whose ink is over 1.1 cells wide and a cell the ink
    /// reaches is not blank or not on the row, when they are fitted to the
    /// cell -- kept under a key of their own, so the cell fitted and the
    /// same cell full size are two entries. The cells beside it are read
    /// from the view, in the order the row is drawn in.
    fn fit(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        (x, y): (usize, usize),
        (base, key): (Codepoint, ClusterKey),
        glyphs: Vec<ClusterGlyph>,
    ) -> Vec<ClusterGlyph> {
        let drawn = self.fitted_key(scene, r, (x, y), (base, key));
        match (drawn == key, self.clusters.get(&drawn)) {
            (false, Some(g)) => g.clone(),
            _ => glyphs,
        }
    }

    /// **The key the cluster kept under `key` is drawn by at `(x, y)`**:
    /// the fitted one where its `base` takes forms, its glyphs overhang the
    /// cell and the blank cells after it take them in, the fitted glyphs
    /// shaped from the kept ones and kept the first time; `key` otherwise.
    /// Nothing is copied once both are kept: a caret frame draws the kept
    /// glyphs by borrow.
    fn fitted_key(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        (x, y): (usize, usize),
        (base, key): (Codepoint, ClusterKey),
    ) -> ClusterKey {
        if !crate::fit::takes_forms(base) {
            return key;
        }
        let wide = match self.wides.get(&key) {
            Some(&w) => w,
            None => {
                let w = self
                    .clusters
                    .get(&key)
                    .and_then(|g| r.overwide(g, Stretch::One));
                self.wides.insert(key, w);
                w
            }
        };
        let Some(wide) = wide else {
            return key;
        };
        let row = scene.view.row(Row::from_index(y));
        let end = Col::at_index(row.len().min(scene.view.cols().index()));
        let blank = |c: Col| {
            row.get(c.index()).map_or(crate::fit::Blank::No, |cell| {
                crate::fit::Blank::of(scene.term, *cell)
            })
        };
        let crate::fit::Fitting::Fitted(n) = wide.fitting(Col::at_index(x), end, blank) else {
            return key;
        };
        let fitted = key.fitted();
        if !self.clusters.contains_key(&fitted) {
            let mut g = self.clusters.get(&key).cloned().unwrap_or_default();
            r.fit_glyphs(&mut g, n, Stretch::One);
            self.clusters.insert(fitted, g);
        }
        fitted
    }

    /// The cell at `at` drawn whole by records: its base and every
    /// codepoint attached to it, shaped once as a cluster and kept, the
    /// cell masked off the map.
    #[allow(clippy::too_many_arguments)]
    fn whole(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        at: (usize, usize),
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let (x, y) = at;
        let (term, styles) = (scene.term, scene.styles.at(Row::from_index(y)));
        let row = scene.view.row(Row::from_index(y));
        let cell = row[x];
        let Some(cp) = crate::renderer::drawn_base(term, cell) else {
            return;
        };
        let (lean, style) = slant_at(r, paint, at, crate::cellmap::face_of(styles, cell));
        // The cell's codepoints, its base and every mark drawn on it: folded
        // into the key as they are read, and gathered only to shape a
        // cluster not yet kept.
        let marks = || {
            crate::renderer::drawn_marks(term, r.frame_held().words(), cell)
                .filter_map(Codepoint::new)
        };
        let mut key = marks().fold(ClusterKey::start().word(style.code()).fold(cp), |k, c| {
            k.fold(c)
        });
        let span = CellSpan::at(row, Col::at_index(x));
        if span == CellSpan::Two {
            key = key.wide();
        }
        if let Some(l) = lean {
            key = key.sheared(l);
        }
        if !self.clusters.contains_key(&key) {
            self.stats.misses.bump();
            let cps: Vec<Codepoint> = core::iter::once(cp).chain(marks()).collect();
            let mut g = r.cluster(&cps, style, span);
            if let Some(l) = lean {
                r.shear_glyphs(&mut g, l, Stretch::One, Flip::Kept);
            }
            self.clusters.insert(key, g);
        }
        let key = match lean {
            None => self.fitted_key(scene, r, at, (cp, key)),
            Some(_) => key,
        };
        self.stats.cells.bump();
        mask[y * geom.cols.index() + x] |= MASK_RECORD;
        let (inside, above) = (records.len(), self.over.len());
        let glyphs = self.clusters.get(&key).map_or(&[][..], Vec::as_slice);
        Overlay::emit(records, &mut self.over, (x, y), glyphs, cell, paint, geom);
        self.emitted.push(Emitted::of(
            (Row::from_index(y), Col::at_index(x)),
            records,
            &self.over,
            inside..records.len(),
            above..self.over.len(),
        ));
    }

    /// **Row `y`'s Devanagari words drawn by records** (`crate::word`),
    /// through [`Renderer::word_at`], which the instance path draws them by:
    /// each cell's light read as the instance path reads it, its colours as
    /// the cell shader draws them -- a word's own cells unlit, which
    /// `MASK_BIDI` without `MASK_SELECTED` tells the shader, and the block
    /// cursor off the shader where it sits on one ([`Overlay::cursor_drawn`])
    /// -- every cell of a word masked.
    #[allow(clippy::too_many_arguments)]
    fn words(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        (y, row, (words, kept), taken): RowWords<'_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let at = Row::from_index(y);
        let cols = geom.cols.index();
        let in_word = |x: usize| words.iter().any(|w| w.holds(Col::at_index(x)));
        let light = |x: usize, cell: PackedCell| {
            let selected = paint.highlight.is_some_and(|h| {
                let c = Col::at_index(x);
                match paint.orders {
                    Some(o) => o.selects(&h, at, o.logical_col(at, c), cell.content),
                    None => h.selects(at, c, cell.content),
                }
            });
            let block = paint
                .block
                .is_some_and(|(cr, cc, _)| cr == at && cc.index() == x);
            crate::word::Light {
                selected: match selected {
                    true => Selected::Yes,
                    false => Selected::No,
                },
                block: match block {
                    true => BlockCursor::Yes(paint.block_paint),
                    false => BlockCursor::No,
                },
                cursor: paint.cursor,
            }
        };
        let drawn = |x: usize, cell: PackedCell, l: crate::word::Light| {
            let resolved = paint
                .styles
                .at(at)
                .get(cell.style.index())
                .copied()
                .unwrap_or_default();
            resolved.drawn(Swaps {
                pictured: paint
                    .pictures
                    .map_or(Pictured::No, |p| p.at(at, Col::at_index(x))),
                line: Line::of(cell.content),
                selected: l.selected,
                hilite: paint.hilite,
                block: l.block,
                blink: paint.blink,
                cursor: l.cursor,
            })
        };
        let colours = |d: crate::cellmap::Drawn| crate::word::Colours {
            fg: d.fg,
            bg: d.bg,
            curve: d.curve,
        };
        let lights: Vec<crate::word::Light> =
            row.iter().enumerate().map(|(x, c)| light(x, *c)).collect();
        let under: Vec<crate::word::Under> = row
            .iter()
            .enumerate()
            .map(|(x, c)| {
                let l = match in_word(x) {
                    true => crate::word::Light::NONE,
                    false => lights[x],
                };
                crate::word::Under {
                    colours: colours(drawn(x, *c, l)),
                    light: lights[x],
                }
            })
            .collect();
        let pan = i32::try_from(geom.pan.value()).unwrap_or(0);
        let top = Screen(
            i16::try_from(
                i32::try_from(y).unwrap_or(0) * i32::try_from(geom.m.cell_h.value()).unwrap_or(0)
                    + pan,
            )
            .unwrap_or(i16::MAX),
        );
        // The word before each, laid already -- the row's own or the
        // composition's ([`Taken::ink_before`]): its ink's right edge, which
        // a residue closes on, as the instance path lays them.
        let mut prev: Option<(Col, f32)> = None;
        for (i, w) in words.iter().enumerate() {
            let first = w.at.index();
            if (first..w.end().index())
                .any(|x| mask.get(y * cols + x).is_none_or(|m| m & MASK_RECORD != 0))
            {
                continue;
            }
            let wp = crate::word::WordPaint {
                syllables: w
                    .syllables
                    .iter()
                    .map(|s| {
                        let l = (s.at.index()..s.at.index() + s.cells.index())
                            .fold(crate::word::Light::NONE, |l, x| l.with(lights[x]));
                        let d = drawn(s.at.index(), row[s.at.index()], l);
                        (colours(d), l, d.text)
                    })
                    .collect(),
            };
            let mut drawn_word = Vec::new();
            let before = taken.ink_before(at, w.at, prev);
            // Its syllables' faces from the frame's table, which the
            // renderer's own is not kept to on this path.
            let styles = crate::word::faces_of(scene.styles.at(at), row, w);
            let laid = r.word_at(
                (scene.term, &styles),
                (at, row, kept.map(|k| (k, crate::kept::WordIx(i)))),
                (w, &wp, before),
                &under,
                &mut drawn_word,
            );
            prev = laid.map(|l| (l.at, l.ink.1)).or(prev);
            let (inside, above) = (records.len(), self.over.len());
            for inst in drawn_word {
                let moved = Instance {
                    y: Screen(i16::try_from(i32::from(inst.y.0) + pan).unwrap_or(i16::MAX)),
                    ..inst
                };
                let (up, inner) = moved.cut_at(top);
                self.over.extend(up);
                records.extend(inner);
            }
            for (x, l) in lights.iter().enumerate().skip(first).take(w.cells.index()) {
                if let Some(m) = mask.get_mut(y * cols + x) {
                    *m = (*m | MASK_RECORD | MASK_BIDI) & !MASK_SELECTED;
                }
                if l.block != BlockCursor::No {
                    self.cursor = CursorDrawn::Records;
                }
            }
            self.stats.cells.add(Tally::of(w.cells.index()));
            self.emitted.push(Emitted::of(
                (at, w.at),
                records,
                &self.over,
                inside..records.len(),
                above..self.over.len(),
            ));
        }
    }

    /// **The syllable of `cells` cells from `at`** drawn whole by records
    /// (`crate::syllable`): every cell's base and marks shaped once as one
    /// cluster and kept, drawn across the cells ([`Renderer::syllable`]) in
    /// the first cell's paint, every cell masked off the map.
    #[allow(clippy::too_many_arguments)]
    fn syllable(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        at: (usize, usize),
        cells: Cols,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let (x, y) = at;
        let (term, styles) = (scene.term, scene.styles.at(Row::from_index(y)));
        let row = scene.view.row(Row::from_index(y));
        let Some(&cell) = row.get(x) else {
            return;
        };
        let style = crate::cellmap::face_of(styles, cell);
        let mut key = ClusterKey::start().word(style.code());
        let mut cps = Vec::new();
        for c in row.iter().skip(x).take(cells.index()) {
            if c.content.is_wide_tail() {
                continue;
            }
            for cp in crate::renderer::cell_cps(term, r.frame_held().words(), *c) {
                key = key.fold(cp);
                cps.push(cp);
            }
        }
        let key = key.word(Code(SYLLABLE | u32::try_from(cells.index()).unwrap_or(0)));
        let glyphs = match self.clusters.get(&key) {
            Some(g) => g.clone(),
            None => {
                self.stats.misses.bump();
                let g = r.syllable(&cps, style, cells, Stretch::One);
                self.clusters.insert(key, g.clone());
                g
            }
        };
        self.stats.cells.add(Tally::of(cells.index()));
        for m in mask
            .iter_mut()
            .skip(y * geom.cols.index() + x)
            .take(cells.index())
        {
            *m |= MASK_RECORD;
        }
        let (inside, above) = (records.len(), self.over.len());
        Overlay::emit(records, &mut self.over, (x, y), &glyphs, cell, paint, geom);
        self.emitted.push(Emitted::of(
            (Row::from_index(y), Col::at_index(x)),
            records,
            &self.over,
            inside..records.len(),
            above..self.over.len(),
        ));
    }

    /// The cell at `at`, which L4 mirrors as `m`, drawn by records:
    /// [`Overlay::whole`]'s, flipped through [`Renderer::cluster_flipped`],
    /// or with the mirror for its base under its marks.
    #[allow(clippy::too_many_arguments)]
    fn whole_mirrored(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        at: (usize, usize),
        m: crate::bidi::Mirrored,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let (x, y) = at;
        let row = scene.view.row(Row::from_index(y));
        let cell = row[x];
        let mut cps = crate::renderer::cell_cps(scene.term, r.frame_held().words(), cell);
        if let (crate::bidi::Mirrored::Under(base), Some(first)) = (m, cps.first_mut()) {
            *first = base;
        }
        if cps.is_empty() {
            return;
        }
        let (lean, style) = slant_at(
            r,
            paint,
            at,
            crate::cellmap::face_of(scene.styles.at(Row::from_index(y)), cell),
        );
        let mut key = ClusterKey::start().word(style.code());
        for &c in &cps {
            key = key.fold(c);
        }
        let span = CellSpan::at(row, Col::at_index(x));
        if span == CellSpan::Two {
            key = key.wide();
        }
        let flipped = m == crate::bidi::Mirrored::Flipped;
        if flipped {
            key = key.flipped();
        }
        if let Some(l) = lean {
            key = key.sheared(l);
        }
        let glyphs = match self.clusters.get(&key) {
            Some(g) => g.clone(),
            None => {
                self.stats.misses.bump();
                let mut g = match flipped {
                    true => r.cluster_flipped(&cps, style, span),
                    false => r.cluster(&cps, style, span),
                };
                if let Some(l) = lean {
                    let flip = match flipped {
                        true => Flip::Flipped,
                        false => Flip::Kept,
                    };
                    r.shear_glyphs(&mut g, l, Stretch::One, flip);
                }
                self.clusters.insert(key, g.clone());
                g
            }
        };
        self.stats.cells.bump();
        mask[y * geom.cols.index() + x] |= MASK_RECORD;
        let (inside, above) = (records.len(), self.over.len());
        Overlay::emit(records, &mut self.over, (x, y), &glyphs, cell, paint, geom);
        self.emitted.push(Emitted::of(
            (Row::from_index(y), Col::at_index(x)),
            records,
            &self.over,
            inside..records.len(),
            above..self.over.len(),
        ));
    }

    /// The cell at `at`, whose letter the layout joined, drawn by records in
    /// `form` with the cells of its word `around` it: [`Overlay::whole`]'s,
    /// through [`Renderer::cluster_joined`].
    #[allow(clippy::too_many_arguments)]
    fn whole_joined(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        at: (usize, usize),
        (form, around): (
            baud_domain::JoiningForm,
            (&[crate::bidi::WordCell], &[crate::bidi::WordCell]),
        ),
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let (x, y) = at;
        let cell = scene.view.row(Row::from_index(y))[x];
        let frame = r.frame_held();
        let cps = crate::renderer::cell_cps(scene.term, frame.words(), cell);
        if cps.is_empty() {
            return;
        }
        let (lean, style) = slant_at(
            r,
            paint,
            at,
            crate::cellmap::face_of(scene.styles.at(Row::from_index(y)), cell),
        );
        let mut key = ClusterKey::start().word(style.code());
        for &c in &cps {
            key = key.fold(c);
        }
        let word = r.joined_word(
            (
                scene.term,
                scene.styles.at(Row::from_index(y)),
                frame.words(),
            ),
            &cps,
            style,
            (around, (Row::from_index(y), Col::at_index(x))),
        );
        let mut key = key.joined(form).around(&word);
        if let Some(l) = lean {
            key = key.sheared(l);
        }
        let glyphs = match self.clusters.get(&key) {
            Some(g) => g.clone(),
            None => {
                self.stats.misses.bump();
                let mut g = r.cluster_joined(&cps, style, form, &word);
                if let Some(l) = lean {
                    r.shear_glyphs(&mut g, l, Stretch::One, Flip::Kept);
                }
                self.clusters.insert(key, g.clone());
                g
            }
        };
        let glyphs = match (lean, cps.first()) {
            (None, Some(&base)) => self.fit(scene, r, at, (base, key), glyphs),
            _ => glyphs,
        };
        self.stats.cells.bump();
        mask[y * geom.cols.index() + x] |= MASK_RECORD;
        let (inside, above) = (records.len(), self.over.len());
        Overlay::emit(records, &mut self.over, (x, y), &glyphs, cell, paint, geom);
        self.emitted.push(Emitted::of(
            (Row::from_index(y), Col::at_index(x)),
            records,
            &self.over,
            inside..records.len(),
            above..self.over.len(),
        ));
    }

    /// **The lam-alef pair one of whose cells is `at`**, the left one of a
    /// pair on one row, drawn together by records through
    /// [`Renderer::lam_alef`], its cells masked: over the cells from its lam
    /// to its alef, or, for a pair a soft wrap parts, the ligature in the
    /// lam's cell and nothing in the alef's. False, and nothing drawn, where
    /// the two take different faces, the partner is not where the part
    /// says or is held, or a face forms no ligature for a parted pair
    /// ([`Renderer::wrapped_ligates`]).
    #[allow(clippy::too_many_arguments)]
    fn pair(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        at: (usize, usize),
        (orders, partner_held, covered): (&crate::bidi::Orders, bool, &Covered),
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        paint: &Paint<'_>,
        geom: &Geom,
    ) -> bool {
        use crate::bidi::{PairEnd, PairPart};
        use crate::renderer::{LamAt, PairSpan};
        let (x, y) = at;
        let window_row = Row::from_index(y);
        let row = scene.view.row(window_row);
        let Some(j) = orders.joined(window_row, Col::at_index(x)) else {
            return false;
        };
        let Some((part, kind)) = j.pair else {
            return false;
        };
        let Some(&here) = row.get(x) else {
            return false;
        };
        let stretch = crate::atlas::Stretch::One;
        // The cells drawn, each with its glyphs, and the key and the
        // arguments the glyphs are found by.
        // **A partner a composition holds** is drawn as written there
        // (`Renderer::shown_cell`), by this cell, whichever end it is.
        let composed = |o: usize| covered.composes(window_row, Col::at_index(o));
        let shown = |r: &Renderer<'_, '_>, o: usize| {
            let cell = row.get(o).copied().unwrap_or(PackedCell::EMPTY);
            match composed(o) {
                true => r.written_cell(window_row, Col::at_index(o)).unwrap_or(cell),
                false => cell,
            }
        };
        let (lam_x, alef_x, lam_form, lam_cell, alef_cell, span) = match part {
            // The right end is drawn by the left one, which masks it: one
            // reached here was not, and is drawn alone -- unless the left
            // one is composed, and this one draws both.
            PairPart::Lam(cells, PairEnd::Right) | PairPart::Alef(cells, PairEnd::Right) => {
                let Some(o) = x
                    .checked_sub(cells.index().saturating_sub(1))
                    .filter(|&o| o < x && composed(o))
                else {
                    return false;
                };
                let partner = orders
                    .joined(window_row, Col::at_index(o))
                    .and_then(|p| p.pair);
                let there = shown(r, o);
                match (part, partner) {
                    (PairPart::Lam(..), Some((PairPart::Alef(h, PairEnd::Left), _)))
                        if h == cells =>
                    {
                        (
                            x,
                            o,
                            j.form,
                            here,
                            there,
                            PairSpan::Cells(LamAt::Right, cells),
                        )
                    }
                    (PairPart::Alef(..), Some((PairPart::Lam(h, PairEnd::Left), _)))
                        if h == cells =>
                    {
                        let lam_form = orders
                            .joined(window_row, Col::at_index(o))
                            .map_or(j.form, |l| l.form);
                        (
                            o,
                            x,
                            lam_form,
                            there,
                            here,
                            PairSpan::Cells(LamAt::Left, cells),
                        )
                    }
                    _ => return false,
                }
            }
            PairPart::Lam(cells, PairEnd::Left) | PairPart::Alef(cells, PairEnd::Left) => {
                let o = x + cells.index().saturating_sub(1);
                let partner = orders
                    .joined(window_row, Col::at_index(o))
                    .and_then(|p| p.pair);
                let held = partner_held && !composed(o);
                let there = shown(r, o);
                let (true, false, true) = (o < row.len(), held, o > x) else {
                    return false;
                };
                match (part, partner) {
                    (PairPart::Lam(..), Some((PairPart::Alef(h, PairEnd::Right), _)))
                        if h == cells =>
                    {
                        (
                            x,
                            o,
                            j.form,
                            here,
                            there,
                            PairSpan::Cells(LamAt::Left, cells),
                        )
                    }
                    (PairPart::Alef(..), Some((PairPart::Lam(h, PairEnd::Right), _)))
                        if h == cells =>
                    {
                        let lam_form = orders
                            .joined(window_row, Col::at_index(o))
                            .map_or(j.form, |l| l.form);
                        (
                            o,
                            x,
                            lam_form,
                            there,
                            here,
                            PairSpan::Cells(LamAt::Right, cells),
                        )
                    }
                    _ => return false,
                }
            }
            PairPart::LamWrapped(alef) => {
                let style = crate::cellmap::face_of(scene.styles.at(window_row), here);
                if !r.wrapped_ligates(scene.term, (here, alef), kind, style, stretch) {
                    return false;
                }
                (x, x, j.form, here, alef, PairSpan::LamCell)
            }
            PairPart::AlefWrapped(lam) => {
                // The lam is on the row before, its id in that row's table.
                let before = Row::from_index(y.saturating_sub(1));
                let style = crate::cellmap::face_of(scene.styles.at(before), lam);
                if !r.wrapped_ligates(scene.term, (lam, here), kind, style, stretch) {
                    return false;
                }
                // Its letter is in the lam's ligature, on the row before:
                // the cell draws no glyph.
                self.stats.cells.bump();
                mask[y * geom.cols.index() + x] |= MASK_RECORD;
                let (inside, above) = (records.len(), self.over.len());
                self.emitted.push(Emitted::of(
                    (window_row, Col::at_index(x)),
                    records,
                    &self.over,
                    inside..records.len(),
                    above..self.over.len(),
                ));
                return true;
            }
        };
        let frame = r.frame_held();
        let lam = crate::renderer::cell_cps(scene.term, frame.words(), lam_cell);
        let alef = crate::renderer::cell_cps(scene.term, frame.words(), alef_cell);
        let (lean, style) = slant_at(
            r,
            paint,
            (lam_x, y),
            crate::cellmap::face_of(scene.styles.at(window_row), lam_cell),
        );
        let face = r.face_for(&lam, style);
        let alef_at = match span {
            PairSpan::LamCell => None,
            PairSpan::Cells(..) => Some(alef_x),
        };
        let word = r.pair_word(
            (scene.term, scene.styles.at(window_row), frame.words()),
            window_row,
            (lam_x, alef_at),
            face,
        );
        let mut key = ClusterKey::start().word(style.code());
        for &c in &lam {
            key = key.fold(c);
        }
        key = key.next_cell();
        for &c in &alef {
            key = key.fold(c);
        }
        let key = match span {
            PairSpan::Cells(LamAt::Right, _) => key.wide(),
            _ => key,
        }
        .paired(lam_form)
        .around(&word);
        // A span wider than the pair's two cells, or the lam's cell alone,
        // places the same glyphs elsewhere: one word past any codepoint's.
        let key = match span {
            PairSpan::Cells(_, n) if n.index() == 2 => key,
            PairSpan::Cells(_, n) => {
                key.word(Code(0x8000_0000 | u32::try_from(n.index()).unwrap_or(0)))
            }
            PairSpan::LamCell => key.word(Code(0xC000_0000)),
        };
        let key = match lean {
            Some(l) => key.sheared(l),
            None => key,
        };
        let drawn = match self.pairs.get(&key) {
            Some(d) => d.clone(),
            None => {
                self.stats.misses.bump();
                let mut d = r.lam_alef((&lam, &alef), lam_form, style, (span, &word), stretch);
                if let (Some(l), Some(g)) = (lean, d.as_mut()) {
                    r.shear_glyphs(&mut g.lams, l, stretch, Flip::Kept);
                    r.shear_glyphs(&mut g.alefs, l, stretch, Flip::Kept);
                }
                self.pairs.insert(key, d.clone());
                d
            }
        };
        let Some(g) = drawn else {
            return false;
        };
        let cells: Vec<(usize, Vec<ClusterGlyph>, PackedCell)> = match span {
            PairSpan::LamCell => {
                let mut both = g.lams;
                both.extend(g.alefs);
                vec![(lam_x, both, lam_cell)]
            }
            PairSpan::Cells(..) => vec![(lam_x, g.lams, lam_cell), (alef_x, g.alefs, alef_cell)],
        };
        for (cx, glyphs, cell) in &cells {
            self.stats.cells.bump();
            mask[y * geom.cols.index() + cx] |= MASK_RECORD;
            let (inside, above) = (records.len(), self.over.len());
            Overlay::emit(
                records,
                &mut self.over,
                (*cx, y),
                glyphs,
                *cell,
                paint,
                geom,
            );
            self.emitted.push(Emitted::of(
                (window_row, Col::at_index(*cx)),
                records,
                &self.over,
                inside..records.len(),
                above..self.over.len(),
            ));
        }
        true
    }

    /// One window of a row: its cells' clusters gathered, cut where the
    /// face changes or a cell holds no codepoint, each piece shaped or
    /// found, and the cells whose glyphs are not the map's masked and
    /// drawn by records.
    #[allow(clippy::too_many_arguments)]
    fn window(
        &mut self,
        scene: &Scene<'_, '_>,
        r: &mut Renderer<'_, '_>,
        y: Row,
        range: (usize, usize),
        geom: &Geom,
        block: Option<(Row, Col, CursorStyle)>,
        covered: &Covered,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        (pictures, orders): (&Pictures, &crate::bidi::Orders),
    ) {
        let row = scene.view.row(y);
        // The window's cells and their codepoints, in buffers the overlay
        // keeps, given back at the end.
        let Gather {
            mut piece,
            cps: mut all,
            cell: mut cps,
        } = core::mem::take(&mut self.gather);
        let flush = |(piece, all): (&mut WindowCells, &mut Vec<Codepoint>),
                     face: Code,
                     this: &mut Overlay,
                     r: &mut Renderer<'_, '_>,
                     mask: &mut [u8],
                     records: &mut Vec<Instance>| {
            if piece.len() < 2 {
                piece.clear();
                all.clear();
                return;
            }
            this.stats.windows.bump();
            for _ in piece.iter() {
                this.stats.window_cells.bump();
            }
            let mut key = ClusterKey::start().word(face);
            for (x, _, at) in piece.iter() {
                for c in &all[at.clone()] {
                    key = key.fold(*c);
                }
                if crate::cellmap::CellSpan::at(row, Col::at_index(*x))
                    == crate::cellmap::CellSpan::Two
                {
                    key = key.wide();
                }
                key = key.next_cell();
            }
            let paint = Paint {
                styles: scene.styles,
                block,
                highlight: scene.highlight,
                blink: r.blink(),
                cursor: opacity_of(r, block),
                pictures: Some(pictures),
                hilite: r.hilite(),
                block_paint: r.block_paint(),
                orders: Some(orders),
            };
            // `ceiling-no-window-cache` prices the miss: nothing is found.
            let hit = if cfg!(feature = "ceiling-no-window-cache") {
                None
            } else {
                this.windows.get(&key)
            };
            if let Some(drawn) = hit {
                Overlay::paint_window(
                    piece,
                    drawn,
                    y,
                    geom,
                    &paint,
                    &mut this.stats,
                    mask,
                    records,
                    &mut this.over,
                    &mut this.emitted,
                );
            } else {
                this.stats.window_misses.bump();
                let cells: Vec<(Vec<Codepoint>, CellSpan)> = piece
                    .iter()
                    .map(|(x, _, at)| {
                        (
                            all[at.clone()].to_vec(),
                            CellSpan::at(row, Col::at_index(*x)),
                        )
                    })
                    .collect();
                let shaped = r.window(face, &cells);
                let drawn: Vec<Option<Vec<ClusterGlyph>>> = piece
                    .iter()
                    .zip(shaped)
                    .map(|((x, cell, at), glyphs)| {
                        let style = crate::cellmap::face_of(scene.styles.at(y), *cell);
                        let alone = scene
                            .map
                            .probe(all[at.start], style, CellSpan::at(row, Col::at_index(*x)))
                            .map(|e| e.glyph());
                        let same = !cell.flags.is_marked()
                            && glyphs.len() == 1
                            && Some(glyphs[0].id) == alone
                            && glyphs[0].dx == 0
                            && glyphs[0].dy == 0;
                        (!same).then_some(glyphs)
                    })
                    .collect();
                Overlay::paint_window(
                    piece,
                    &drawn,
                    y,
                    geom,
                    &paint,
                    &mut this.stats,
                    mask,
                    records,
                    &mut this.over,
                    &mut this.emitted,
                );
                this.windows.insert(key, drawn);
            }
            piece.clear();
            all.clear();
        };
        // The face of the piece being gathered, from the map entries: a
        // cell on another face, a cell with no codepoint, and a cell
        // whose glyph is in no rule at any position -- which cannot be
        // inside a match -- all cut the window, so what is left is the
        // punctuation the rules are about, which repeats and caches.
        let mut first: Option<Code> = None;
        for x in range.0..=range.1 {
            let Some(cell) = row.get(x) else {
                break;
            };
            if cell.content == Content::WIDE_TAIL {
                continue;
            }
            // A cell under the composition is empty to the walk, and cuts
            // the window as an empty cell does.
            let drawn = match covered.holds(y, Col::at_index(x)) {
                true => None,
                false => crate::renderer::drawn_base(scene.term, *cell),
            };
            let mut entry = drawn.and_then(|cp| {
                scene
                    .map
                    .probe(
                        cp,
                        crate::cellmap::face_of(scene.styles.at(y), *cell),
                        CellSpan::at(row, Col::at_index(x)),
                    )
                    // A letter reaching past its cell cuts the window: the
                    // pass that judges it by the cells beside it draws it
                    // (`crate::fit`).
                    .filter(|e| !e.reaches_past())
                    .map(|e| (cp, e.face(), e.starts_pair() || e.ends_pair()))
            });
            // A marked cell is drawn from the face its whole cluster takes
            // (`Renderer::face_for`), which the map's entry for its base
            // alone does not know. One its marks move off the window's face
            // is cut out of the window and left to the marked cells' pass,
            // which draws it through `Renderer::cluster` on that face -- as
            // the run path draws it, in a run of its own. Only a marked cell
            // reads its style a second time, its marks and `face_for`; a
            // cell with none pays what it paid before. Not on the leaving
            // row of a slide, which the shader draws from the map alone
            // (`cell_glyph_vertex` reads no mask there): a marked cell there
            // draws its base, from its base's face, for that one row.
            cps.clear();
            if let Some((cp, face, _)) = entry {
                if cell.flags.is_marked() {
                    let style = crate::cellmap::face_of(scene.styles.at(y), *cell);
                    cps.push(cp);
                    cps.extend(
                        crate::renderer::drawn_marks(scene.term, r.frame_held().words(), *cell)
                            .filter_map(Codepoint::new),
                    );
                    if u32::try_from(r.face_for(&cps, style).index()).ok() != Some(face.0) {
                        entry = None;
                    }
                }
            }
            // A joined cell is drawn alone in its form, by the joined
            // cells' pass, as the run path draws it.
            if orders.any_joined() && orders.joined(y, Col::at_index(x)).is_some() {
                entry = None;
            }
            let cut = match entry {
                None => true,
                Some((_, _, in_rules)) => !in_rules,
            } || matches!((first, entry), (Some(f), Some((_, face, _))) if f != face);
            if cut {
                if let Some(f) = first {
                    flush((&mut piece, &mut all), f, self, r, mask, records);
                }
                first = None;
            }
            let Some((cp, face, true)) = entry else {
                continue;
            };
            if first.is_none() {
                first = Some(face);
            }
            if cps.is_empty() {
                cps.push(cp);
            }
            let at = all.len()..all.len() + cps.len();
            all.extend_from_slice(&cps);
            piece.push((x, *cell, at));
        }
        if let Some(f) = first {
            flush((&mut piece, &mut all), f, self, r, mask, records);
        }
        self.gather = Gather {
            piece,
            cps: all,
            cell: cps,
        };
    }

    /// A predicted echo: one cell the app draws before
    /// the pty answers, as a record over a masked cell, in the rendition
    /// `cell` names -- the same paint the windows and the marks use, so a
    /// prediction is coloured as the shader would colour the cell once the
    /// echo lands, with `block` the cursor the frame will draw. Shapes the
    /// codepoint alone through the same cluster path a marked cell takes.
    #[allow(clippy::too_many_arguments)]
    pub fn predicted(
        &mut self,
        r: &mut Renderer<'_, '_>,
        at: (Row, Col),
        cell: PackedCell,
        styles: crate::RowTables<'_>,
        block: Option<(Row, Col, CursorStyle)>,
        highlight: Option<Highlight>,
        cols: Cols,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let Some(cp) = cell.content.as_codepoint() else {
            return;
        };
        // **Drawn where the echo will be, as the echo draws it**: `at` is
        // the logical cell, and the row was laid out with the prediction
        // written in it ([`crate::bidi::Written`]), so its visual cell and
        // the block's are the echo's, and so is the mirror UAX #9 L4 gives
        // the character there, a `(` resolved right to left drawn `)`.
        let orders = r.bidi();
        let (y, x) = (at.0.index(), orders.visual_col(at.0, at.1).index());
        let block = block.map(|(br, bc, s)| (br, orders.visual_col(br, bc), s));
        let cp = orders
            .swapped(at.0, Col::at_index(x))
            .and_then(|s| s.as_codepoint())
            .unwrap_or(cp);
        let cell = PackedCell {
            content: Content::codepoint(cp),
            ..cell
        };
        let Some(slot) = mask.get_mut(y * cols.index() + x) else {
            return;
        };
        // The walk already drew this cell with a record of its own -- a
        // mark, or a glyph inside a ligature window. `emit` appends, it
        // does not replace, so drawing here would put two glyphs in one
        // cell; the prediction gives way and the echo draws it a frame
        // later.
        if *slot & MASK_RECORD != 0 {
            return;
        }
        *slot |= MASK_RECORD;
        let glyphs = r.cluster(
            &[cp],
            crate::cellmap::face_of(styles.at(at.0), cell),
            // A prediction is one printable ASCII character.
            CellSpan::One,
        );
        // A prediction is a local echo at the live cursor; the ring rotation
        // that starts a slide drops any pending prediction first, so a
        // predicted record never coexists with a non-zero pan.
        let geom = Geom {
            m: r.metrics(),
            cols,
            pan: Px::of(0),
        };
        let mut over = Vec::new();
        Overlay::emit(
            records,
            &mut over,
            (x, y),
            &glyphs,
            cell,
            &Paint {
                styles,
                block,
                highlight,
                blink: r.blink(),
                cursor: opacity_of(r, block),
                pictures: None,
                hilite: r.hilite(),
                block_paint: r.block_paint(),
                orders: None,
            },
            &geom,
        );
        records.append(&mut over);
    }

    /// An input method's composition, for the cell path: the cells it covers
    /// masked off the shader, and its records -- glyphs and the underline
    /// -- appended, the same records the instance and CPU paths draw
    /// ([`composition_records`]). `block` is the cursor the frame draws,
    /// which [`Composition::cursor`] put after it; `size` is the rendition
    /// of the line it is on, whose row the walk has masked whole when it is
    /// not single-width.
    #[allow(clippy::too_many_arguments)]
    pub fn composed(
        &mut self,
        r: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        (c, laid): (&Composition, Laid),
        under: PackedCell,
        styles: &[Resolved],
        block: Option<(Row, Col, CursorStyle)>,
        cols: Cols,
        size: LineRendition,
        pan: Px,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let (start, end) = c.columns(size.end_of_line(cols));
        for x in start..end {
            let v = laid.column(r.bidi(), c.row, Col::at_index(x));
            if let Some(slot) = mask.get_mut(c.row.index() * cols.index() + v.index()) {
                *slot |= MASK_RECORD;
            }
        }
        // **The letters beside it its words take in** ([`Taken`]): masked
        // as a word's cells are, unlit, the block cursor off the shader
        // where it sits on one -- its words light their syllables' spans.
        if laid == Laid::AsWritten {
            let taken = c.taken(term, r.bidi(), cols, size);
            let block_at = block
                .filter(|(_, _, s)| {
                    r.cursor_look() == CursorLook::Shaped
                        && matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock)
                })
                .map(|(br, bc, _)| (br, r.bidi().visual_col(br, bc).index()));
            for x in taken.beside(c.row) {
                if let Some(m) = mask.get_mut(c.row.index() * cols.index() + x) {
                    *m = (*m | MASK_RECORD | MASK_BIDI) & !MASK_SELECTED;
                }
                if block_at == Some((c.row, x)) {
                    self.cursor = CursorDrawn::Records;
                }
            }
        }
        let mut parts =
            composition_parts(r, term, (c, laid), under, styles, block, cols, size, pan);
        // **On a row the layout joined, drawn in the row's order**, as the
        // instance path draws it ([`crate::renderer::Splice`]): on a line
        // that is not single-width the walk's records of the line already
        // hold its pages and glyphs ([`Renderer::sized_lines`]); on one that
        // is, they go among the walk's records.
        if laid == Laid::AsWritten {
            if let Some(s) = parts.splice(r.bidi(), c.row) {
                if !size.is_double() {
                    self.splice_into(s, records);
                }
            }
        }
        parts.append_to(records);
    }

    /// **A composition's pages and glyphs among the walk's records**, in
    /// the instance path's order: its pages under every record, and each
    /// cluster's glyphs after the records of the cells before its own in
    /// that order -- rows top to bottom, each left to right
    /// ([`Overlay::order`]) -- and before the rest. The walk's records are
    /// the first [`Overlay::under`] of `records`, each of its cells' in
    /// turn; were they not, the glyphs go after them. Both lie inside their
    /// row, so [`Overlay::under`] counts them: the cell path draws them
    /// before its decorations, as the instance path draws them before the
    /// row's.
    fn splice_into(&mut self, s: crate::renderer::Splice, records: &mut Vec<Instance>) {
        let walked = usize::try_from(self.under.value())
            .unwrap_or(usize::MAX)
            .min(records.len());
        let tracked: usize = self.emitted.iter().map(|e| e.inside.len()).sum();
        let mut out = Vec::with_capacity(records.len() + s.pages.len() + s.glyphs.len());
        out.extend(s.pages);
        let (mut k, mut pos) = (0, 0);
        for (x, glyphs) in s.glyphs {
            let at = (s.row, Col::at_index(x));
            while tracked == walked && self.emitted.get(k).is_some_and(|e| e.order() < at) {
                let end = pos + self.emitted[k].inside.len();
                out.extend_from_slice(&records[pos..end]);
                (k, pos) = (k + 1, end);
            }
            if tracked != walked {
                out.extend_from_slice(&records[pos..walked]);
                pos = walked;
            }
            out.extend(glyphs);
        }
        let inserted = out.len() - pos;
        out.extend_from_slice(&records[pos..]);
        *records = out;
        self.under = Tally::of(walked + inserted);
    }

    /// **The leaving row of a smooth scroll, as the shader reads it,
    /// walked into the map**: `leaving`, the row [`Renderer::copy_leaving`]
    /// copied, its entries made present, and the pair candidates and the
    /// cells of several glyphs that walk finds kept for
    /// [`Overlay::leaving`]. Before the window's own walk of the map, which
    /// replaces both.
    pub fn ensure_leaving(
        &mut self,
        map: &mut GlyphMap<'_>,
        term: &Term<'_>,
        leaving: Viewport<'_>,
        styles: &[Resolved],
        r: &mut Renderer<'_, '_>,
    ) {
        let slide = self.slide.get_or_insert_with(|| {
            Box::new(Slide {
                walk: Overlay::new(),
                candidates: Vec::new(),
                several: Vec::new(),
            })
        });
        map.ensure(term, leaving, styles, r, &mut slide.candidates);
        slide.several.clear();
        slide.several.extend_from_slice(map.several());
    }

    /// **The leaving row of a smooth scroll, for the cell path**: the line
    /// just above the window `back` lines into the history, in the order it
    /// was drawn in as that window's first, slid by `pan`, its row of the
    /// mask the one after the screen's `rows`, which the shader reads for
    /// draw row -1.
    ///
    /// On a single-width line, what the map does not draw is drawn by
    /// records as on a row of the window: the walk [`Overlay::ensure`]
    /// makes, over that row alone and through its layout -- its joined
    /// letters in their forms, its mirrored characters, its marked cells
    /// and its ligatures -- lifted one row, and the cells it draws marked
    /// `RECORD`, so the shader draws their backgrounds and decorations and
    /// not their glyphs. The records inside the row are drawn first, with
    /// the window's between the glyph and decoration passes, as the row is
    /// the first the instance path draws. Read after
    /// [`Overlay::ensure_leaving`].
    ///
    /// On a line that is not single-width, the records the instance path
    /// converts it to ([`Renderer::sized_leaving`]), and its row of the
    /// mask marked `RECORD` and `SIZED`, so the shader draws its
    /// backgrounds from half the column and nothing else of it, as on a
    /// sized screen row. Nothing where the mask has no such row.
    #[allow(clippy::too_many_arguments)]
    pub fn leaving(
        &mut self,
        r: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        (map, styles): (&GlyphMap<'_>, crate::RowTables<'_>),
        back: Rows,
        (cols, rows): (Cols, Rows),
        pan: Px,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let at = rows.index().saturating_mul(cols.index());
        if mask.len() < at.saturating_add(cols.index()) {
            return;
        }
        // The size of the line the row draws, as the display lays it out:
        // a re-broken paragraph's line where the stored row is double.
        if !r.leaving_rendition(term, back).is_double() {
            return self.slide_single(r, term, (map, styles), (cols, rows), pan, mask, records);
        }
        let Some(row) = mask.get_mut(at..at.saturating_add(cols.index())) else {
            return;
        };
        for b in row {
            *b |= MASK_RECORD | MASK_SIZED;
        }
        let mut lifted = Vec::new();
        r.sized_leaving(term, back, &mut lifted);
        let pan = i16::try_from(pan.value()).unwrap_or(0);
        records.extend(lifted.into_iter().map(|i| Instance {
            y: Screen(i.y.0.saturating_add(pan)),
            ..i
        }));
    }

    /// [`Overlay::leaving`] on a single-width line: the leaving row walked
    /// as a window one row high, through the layout it was drawn in.
    #[allow(clippy::too_many_arguments)]
    fn slide_single(
        &mut self,
        r: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        (map, styles): (&GlyphMap<'_>, crate::RowTables<'_>),
        (cols, rows): (Cols, Rows),
        pan: Px,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let (Some(slide), Some(cells)) = (self.slide.as_deref_mut(), r.leaving_cells()) else {
            return;
        };
        let at = rows.index().saturating_mul(cols.index());
        if cells.len() != cols.index() {
            return;
        }
        let used = [baud_domain::RowExtent::of(cols.index())];
        let view = Viewport::new(
            &cells,
            &used,
            baud_domain::Stride::of_cells(cols.index()),
            cols,
            Rows::of_count(1),
            Rows::of_count(1),
            baud_domain::RowRot::NONE,
            r.leaving_table(term),
        );
        let scene = Scene {
            term,
            view,
            // The leaving row's table: the row after the window's.
            styles: crate::RowTables::one(styles.at(Row::from_index(rows.index()))),
            cursor: None,
            highlight: None,
            map,
            candidates: &slide.candidates,
            pan,
            composition: None,
        };
        // The walk sets what the frame's cells blink by; the leaving row's
        // are not the window's.
        let blinks = r.blink_seen();
        let mut row_mask = vec![0u8; cols.index()];
        let mut drawn = Vec::new();
        r.swap_leaving_layout();
        slide.walk.ensure_with(
            &scene,
            (Some(&slide.several), None),
            Find::None,
            r,
            &mut row_mask,
            &mut drawn,
        );
        r.swap_leaving_layout();
        r.set_blink_seen(blinks);
        for (b, m) in mask[at..at + cols.index()].iter_mut().zip(&row_mask) {
            *b |= m & MASK_RECORD;
        }
        let ch = i16::try_from(r.metrics().cell_h.value()).unwrap_or(i16::MAX);
        let lift = |i: &Instance| Instance {
            y: Screen(i.y.0.saturating_sub(ch)),
            ..*i
        };
        let count = |t: Tally| usize::try_from(t.value()).unwrap_or(usize::MAX);
        let inside = count(slide.walk.under).min(drawn.len());
        records.splice(0..0, drawn[..inside].iter().map(lift));
        let under = (count(self.under) + inside).min(records.len());
        records.splice(under..under, drawn[inside..].iter().map(lift));
        self.under = Tally::of(under);
    }

    /// **The status line's row, for the cell path**: drawn by the records
    /// the instance path converts it to ([`Renderer::status_records`]), and
    /// its row of the mask -- the one after the main display's `rows`,
    /// which the window copy holds the line's cells in -- marked `RECORD`,
    /// and `SIZED` on a line that is not single-width, so the shader draws
    /// its backgrounds and nothing else of it. Nothing while no status line
    /// is shown. Called after [`Overlay::ensure`], which clears that row.
    #[allow(clippy::too_many_arguments)]
    pub fn status(
        &mut self,
        r: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
        (cols, rows): (Cols, Rows),
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let Some(line) = term.status_line() else {
            return;
        };
        let at = rows.index().saturating_mul(cols.index());
        let Some(row) = mask.get_mut(at..at.saturating_add(cols.index())) else {
            return;
        };
        let bits = match line.rendition.is_double() {
            true => MASK_RECORD | MASK_SIZED,
            false => MASK_RECORD,
        };
        for b in row {
            *b |= bits;
        }
        r.status_records(term, cursor, records);
    }

    /// A window's cells that the map does not draw: masked, counted and
    /// drawn by records.
    #[allow(clippy::too_many_arguments)]
    fn paint_window(
        piece: &[(usize, PackedCell, core::ops::Range<usize>)],
        drawn: &[Option<Vec<ClusterGlyph>>],
        y: Row,
        geom: &Geom,
        paint: &Paint<'_>,
        stats: &mut OverlayStats,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        over: &mut Vec<Instance>,
        emitted: &mut Vec<Emitted>,
    ) {
        for ((x, cell, _), glyphs) in piece.iter().zip(drawn) {
            if let Some(glyphs) = glyphs {
                stats.cells.bump();
                mask[y.index() * geom.cols.index() + x] |= MASK_RECORD;
                let (inside, above) = (records.len(), over.len());
                Overlay::emit(records, over, (*x, y.index()), glyphs, *cell, paint, geom);
                emitted.push(Emitted::of(
                    (y, Col::at_index(*x)),
                    records,
                    over,
                    inside..records.len(),
                    above..over.len(),
                ));
            }
        }
    }

    /// The records for one cell's glyphs, coloured as the shader would
    /// colour the cell, and bent by the contrast curve that colour takes:
    /// the part of each inside its row and below into `records`, and the
    /// part above its row's top into `over`. The cell path draws the first
    /// before its decoration pass and the second after it, so a record
    /// lies under the decorations of its own row and of the row above it
    /// reaches into -- the instance path's order, each row's glyphs and
    /// then its decorations, rows top to bottom.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        records: &mut Vec<Instance>,
        over: &mut Vec<Instance>,
        at: (usize, usize),
        glyphs: &[ClusterGlyph],
        cell: PackedCell,
        paint: &Paint<'_>,
        geom: &Geom,
    ) {
        let (x, y) = at;
        let resolved = paint
            .styles
            .at(Row::from_index(y))
            .get(cell.style.index())
            .copied()
            .unwrap_or_default();
        let under_cursor = paint
            .block
            .is_some_and(|(cr, cc, _)| cr.index() == y && cc.index() == x);
        let selected = paint.highlight.is_some_and(|h| {
            let at = Col::at_index(x);
            match paint.orders {
                Some(o) => {
                    let row = Row::from_index(y);
                    o.selects(&h, row, o.logical_col(row, at), cell.content)
                }
                None => h.selects(Row::from_index(y), at, cell.content),
            }
        });
        // The selection swaps, then the cursor swaps again, and a hidden
        // cell draws nothing of its own: `Resolved::drawn`, the one spelling
        // `Renderer::convert_row` and the cell shader mirror, which the pixel
        // gate compares at a translucent alpha. A record draws a glyph, so
        // only the foreground is taken.
        let pictured = paint
            .pictures
            .map_or(Pictured::No, |p| p.at(Row::from_index(y), Col::at_index(x)));
        let d = resolved.drawn(Swaps {
            pictured,
            line: Line::of(cell.content),
            selected: if selected {
                Selected::Yes
            } else {
                Selected::No
            },
            hilite: paint.hilite,
            block: if under_cursor {
                BlockCursor::Yes(paint.block_paint)
            } else {
                BlockCursor::No
            },
            blink: paint.blink,
            cursor: paint.cursor,
        });
        if !d.text.draws() {
            return;
        }
        let color = d.fg;
        let cell_w = i32::try_from(geom.m.cell_w.value()).unwrap_or(0);
        let cell_h = i32::try_from(geom.m.cell_h.value()).unwrap_or(0);
        let baseline = i32::try_from(geom.m.baseline.value()).unwrap_or(0);
        let ox = i32::try_from(x).unwrap_or(0) * cell_w;
        let oy = i32::try_from(y).unwrap_or(0) * cell_h
            + baseline
            + i32::try_from(geom.pan.value()).unwrap_or(0);
        let curve = d.curve;
        // The row's top, where a glyph reaching up is cut in two.
        let top = Screen(i16::try_from(oy - baseline).unwrap_or(i16::MAX));
        for g in glyphs {
            let Some(slot) = g.slot else {
                continue;
            };
            let inst = Instance {
                x: Screen(i16::try_from(ox + g.dx + slot.left).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(oy - g.dy + slot.top).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                curve,
            };
            let (above, inside) = inst.cut_at(top);
            over.extend(above);
            records.extend(inside);
        }
    }
}

/// An input method's composition -- `NSTextInputClient`'s marked text: a
/// dead key's accent, an IME's unconverted kana -- and the cell it starts
/// at, which is the cursor's.
///
/// **Drawn, never written.** The core never sees it: the child has been sent
/// nothing until the input manager commits, so it lives here, over the grid,
/// for as long as the input manager holds it. `[[source:
/// apple-nstextinputclient]]`'s `setMarkedText` expects marked text drawn
/// "with distinguishing appearance", and `NSTextView`'s
/// `markedTextAttributes` names the three it supports: text colour,
/// background colour and underline. baud's is the underline, in the text's
/// own colour -- what the same text written under `SGR 4` would draw -- with
/// the cursor after it.
///
/// **An input method's hints** (`text-input-v3`'s `preedit_hint`,
/// `[[source: wp-text-input-unstable-v3]]`) are drawn as GTK's theme draws
/// them (`[[source: gtk-3b438f05-common-scss]]`), each in a rendition the
/// terminal already draws and resolved as the same text written in it:
/// `whole` as `SGR 4`, `selection` as selected text, `prediction` as
/// `SGR 3`, `prefix` and `suffix` as `SGR 2`, a `spelling_error` as
/// `SGR 4:3` under `SGR 58:5:1`, the palette's red, a `compose_error` as
/// `SGR 9`, in the text's colour where GTK takes its error colour. Text
/// no hint covers is "unhinted" and drawn plain; with no hints at all the
/// whole composition is underlined, which is all the macOS app is given.
///
/// **The input method's cursor** ([`Caret`]) is drawn where it puts it:
/// the terminal's cursor, in its own style, on the character it is before;
/// a range drawn as selected text with no cursor; none at all; or, where
/// it says nothing, after the composition.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Composition {
    pub row: Row,
    pub col: Col,
    pub text: Vec<Codepoint>,
    /// How the core measures what it writes ([`Measure`]), so the
    /// overlay and a write of the same text agree.
    pub measure: Measure,
    /// The hints on each codepoint of `text`, or none at all.
    pub hints: Vec<Hints>,
    /// Where the input method puts the cursor in it.
    pub caret: Caret,
}

/// **How the core measures the text it writes**, which a composition is
/// laid out by: the columns an ambiguous character takes
/// (`Term::ambiguous_width`; Private Use stays one column either way),
/// the Unicode version's widths (`Term::unicode_widths`), whether a
/// `U+FE0F` widens the base before it (`Term::vs16_width`, read with the
/// cells, `Vs16Width::widens_in`) and how much of a cluster a cell holds
/// (`Term::cluster_cells`). The default is the core's at power-on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Measure {
    pub ambiguous: AmbiguousWidth,
    pub widths: UnicodeWidths,
    pub vs16: Vs16Width,
    pub cells: ClusterCells,
}

impl Default for Measure {
    fn default() -> Measure {
        Measure {
            ambiguous: AmbiguousWidth::Narrow,
            widths: UnicodeWidths::Current,
            vs16: Vs16Width::Two,
            cells: ClusterCells::Legacy,
        }
    }
}

impl Measure {
    /// What `term` measures the next character it writes by.
    pub fn of(term: &Term<'_>) -> Measure {
        Measure {
            ambiguous: term.ambiguous_width(),
            widths: term.unicode_widths(),
            vs16: term.vs16_width(),
            cells: term.cluster_cells(),
        }
    }
}

/// **One cluster of a text as the core writes it**: the indices of the
/// text's codepoints it holds, those it draws, and its cells.
pub(crate) struct Cluster {
    pub(crate) codes: core::ops::Range<usize>,
    pub(crate) cps: Vec<Codepoint>,
    pub(crate) span: CellSpan,
}

impl Cluster {
    /// `cp`, the text's next codepoint, continuing the cluster: a `U+FE0F`
    /// after a listed base makes a one-cell cluster two cells while the
    /// core widens it (`baud_domain::vs16`), the codepoint before it
    /// deciding; under Unicode 8 the second regional indicator of a flag
    /// does (`[[source: eaw-data-8]]`), as the core's `widen_for` does.
    fn join(&mut self, cp: Codepoint, m: Measure) {
        let before = self.cps.last().copied();
        let alone = self.cps.len() == 1;
        self.cps.push(cp);
        self.codes.end += 1;
        if self.span == CellSpan::Two {
            return;
        }
        let vs16 =
            cp.value() == 0xFE0F && m.vs16.widens_in(m.cells) && before.is_some_and(vs16::widens);
        let flag = cp.is_regional_indicator()
            && alone
            && m.widths == UnicodeWidths::Eight
            && before.is_some_and(Codepoint::is_regional_indicator);
        if vs16 || flag {
            self.span = CellSpan::Two;
        }
    }
}

/// **`text` in clusters, as the core writes it after a character**, by
/// `m`: `[[source: uax29]]`'s boundaries from the core's own machine,
/// each cluster as wide as its first character and widened as the core
/// widens it ([`Cluster::join`]), and parted where the cells in force hold
/// less than an extended cluster (`baud_domain::ClusterCells`), as the
/// core parts it; a character of no width that is not
/// Extend or ZWJ joins the cluster before it wherever the boundaries
/// fall, an ignorable one at a boundary is dropped and ends what may
/// join, and a mark at a boundary after that is a defective sequence
/// (`[[source: unicode18-ch3]]` D57) in a cell of its own, drawn on a
/// no-break space as the core draws one. Printable ASCII starts a cluster
/// whatever is before it, as the core's writer does -- a departure from
/// GB9b after a Prepend the core pins (`Why::AsciiAfterPrepend` in its
/// suite), followed so the composition draws what its commit writes. The
/// machine starts as after a
/// letter, so what continues the character before the text -- the
/// count returned first -- joins it, as a write of the text does, and
/// is in no cluster of its own: [`Composition::at_cursor`] leads the
/// text with that character's cluster, so it is drawn there.
pub(crate) fn clusters(text: &[Codepoint], m: Measure) -> (usize, Vec<Cluster>) {
    let policy = WidthPolicy::of(m.ambiguous, m.widths);
    let mut machine = ClusterBreak::after_ascii_run();
    let mut lead = 0;
    let mut out: Vec<Cluster> = Vec::new();
    for (i, &cp) in text.iter().enumerate() {
        // Printable ASCII is written without asking the machine, and tells
        // it a letter went past: a cluster of its own, whatever is before.
        if char::from_u32(cp.value()).is_some_and(|c| c.is_ascii() && !c.is_ascii_control()) {
            machine = ClusterBreak::after_ascii_run();
            out.push(Cluster {
                codes: i..i + 1,
                cps: vec![cp],
                span: CellSpan::One,
            });
            continue;
        }
        let cl = Classified::of(cp);
        let w = cl.width(policy);
        let before = machine;
        let mut joins = !machine.is_break_classified(cl.class, cl.lookback);
        // A join the cells in force part takes a cell of its own, as the
        // core's writer gives it one, the machine started again at it.
        if joins && before.parts_at(m.cells, cl.class, cl.lookback, cp) {
            machine = ClusterBreak::restart(cl.class, cl.lookback);
            joins = false;
        }
        let marks = matches!(cl.class, GraphemeClass::Extend | GraphemeClass::Zwj);
        if joins || (w == Width::ZERO && !marks) {
            match out.last_mut() {
                Some(last) => last.join(cp, m),
                None => lead = i + 1,
            }
            continue;
        }
        if w == Width::ZERO && is_default_ignorable(cp) {
            machine = ClusterBreak::after_control();
            match out.last_mut() {
                Some(last) => last.codes.end = i + 1,
                None => lead = i + 1,
            }
            continue;
        }
        let (cps, span) = match w {
            w if w == Width::ZERO => (vec![Codepoint::from_char('\u{a0}'), cp], CellSpan::One),
            w if w == Width::WIDE => (vec![cp], CellSpan::Two),
            _ => (vec![cp], CellSpan::One),
        };
        out.push(Cluster {
            codes: i..i + 1,
            cps,
            span,
        });
    }
    (lead, out)
}

/// **Where an input method puts the cursor in its composition**, by
/// codepoint of `Composition::text`: `text-input-v3`'s `cursor_begin` and
/// `cursor_end`, which "could be represented by the client as a line if
/// both values are the same, or as a text highlight otherwise", the cursor
/// "hidden when both are equal to -1" (`[[source:
/// wp-text-input-unstable-v3]]`), and AppKit's selected range, "the range
/// to set as the selection" (`[[source: apple-nstextinputclient-setmarkedtext]]`).
/// GTK draws its own cursor at the position (`[[source: gtk-3b438f05-gtktext]]`),
/// so the terminal's cursor is drawn there in the style the program set.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Caret {
    /// Nothing said: after the composition, where a write of the same text
    /// would leave the cursor -- all the macOS app is handed today.
    #[default]
    End,
    /// Before the codepoint at this index; at the count, after the last.
    At(usize),
    /// The codepoints `start..end` drawn as selected text, and no cursor.
    Range(usize, usize),
    /// No cursor.
    Hidden,
}

/// The preedit hints on one codepoint of a composition: any of
/// `text-input-v3`'s seven at once, since their extents "may overlap".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub struct Hints(u16);

impl Hints {
    pub const NONE: Hints = Hints(0);
    pub const WHOLE: Hints = Hints(1);
    pub const SELECTION: Hints = Hints(2);
    pub const PREDICTION: Hints = Hints(4);
    pub const PREFIX: Hints = Hints(8);
    pub const SUFFIX: Hints = Hints(16);
    pub const SPELLING_ERROR: Hints = Hints(32);
    pub const COMPOSE_ERROR: Hints = Hints(64);
    /// **Drawn in baud's message pair** ([`crate::message`]), an opaque box
    /// and text a program cannot change: not one of `text-input-v3`'s, and
    /// never carried by an input method's composition.
    pub const MESSAGE: Hints = Hints(128);
    /// **The link under the pointer as `hover = "text"` draws it**
    /// ([`crate::HoverPaint::Text`]): its own ground, its text in the message
    /// pair's box colour where that reads. Not one of `text-input-v3`'s either.
    pub const LINK_TEXT: Hints = Hints(256);
    /// **Drawn in the window's default rendition**, as text a program
    /// writes after a soft reset (`DECSTR`) is: the default colours and no
    /// attribute, whatever the cell under it was left in -- a ground of
    /// `SGR 41` or the concealing of `SGR 8` -- and its ground painted. Not
    /// one of `text-input-v3`'s either.
    pub const DEFAULT: Hints = Hints(512);

    pub fn with(self, h: Hints) -> Hints {
        Hints(self.0 | h.0)
    }

    pub fn has(self, h: Hints) -> bool {
        self.0 & h.0 == h.0 && h.0 != 0
    }
}

/// One cluster of a composition: the column it starts at, its codepoints,
/// how many cells it takes and the hints on its first -- with the
/// selection where the input method's range covers any of its codepoints
/// -- and the indices of the text's codepoints it holds.
struct Piece {
    col: Col,
    cps: Vec<Codepoint>,
    span: CellSpan,
    hints: Hints,
    codes: core::ops::Range<usize>,
}

/// The line a cluster's hints draw under or through it. Later rules in
/// GTK's theme replace an earlier `text-decoration`, so an error's line
/// takes the place of `whole`'s.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    None,
    Under,
    Curly,
    Strike,
}

/// **How one cluster is drawn**, from its hints: selected or not, the
/// attributes its rendition adds to the cell under it -- `SGR 3` for a
/// prediction, `SGR 2` for a prefix or suffix -- and its line.
#[derive(Clone, Copy, Debug)]
struct Drawing {
    selected: Selected,
    pair: Paired,
    adds: Attrs,
    rule: Rule,
}

/// Whether a cluster is drawn in the message pair ([`Hints::MESSAGE`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paired {
    No,
    Yes,
    /// [`Hints::LINK_TEXT`]: the pair's box colour as the text, no page.
    Text,
    /// [`Hints::DEFAULT`]: the default rendition, its page painted.
    Default,
}

/// The palette entry an error's curly line takes, `SGR 58:5:1`'s: red.
const ERROR: u8 = 1;

impl Drawing {
    fn of(h: Hints) -> Drawing {
        let rule = if h.has(Hints::COMPOSE_ERROR) {
            Rule::Strike
        } else if h.has(Hints::SPELLING_ERROR) {
            Rule::Curly
        } else if h.has(Hints::WHOLE) {
            Rule::Under
        } else {
            Rule::None
        };
        Drawing {
            selected: match h.has(Hints::SELECTION) {
                true => Selected::Yes,
                false => Selected::No,
            },
            pair: match (
                h.has(Hints::MESSAGE),
                h.has(Hints::LINK_TEXT),
                h.has(Hints::DEFAULT),
            ) {
                (true, ..) => Paired::Yes,
                (false, true, _) => Paired::Text,
                (false, false, true) => Paired::Default,
                (false, false, false) => Paired::No,
            },
            adds: match h.has(Hints::PREDICTION) {
                true => Attrs::ITALIC,
                false => Attrs::NONE,
            }
            .with(match h.has(Hints::PREFIX) || h.has(Hints::SUFFIX) {
                true => Attrs::DIM,
                false => Attrs::NONE,
            }),
            rule,
        }
    }

    /// The rendition of the cell under the composition, `under`, as this
    /// draws it: the attributes its hints name added, and an error's
    /// line colour. Resolved by the renderer as any written cell is, so a
    /// hint draws exactly as the same text written in its rendition.
    fn word(self, under: StyleWord) -> StyleWord {
        let w = under.with_attrs(under.attrs().with(self.adds));
        match self.rule {
            Rule::Curly => w.with_underline(ColorWord::indexed(Chan::from_u8(ERROR))),
            _ => w,
        }
    }
}

impl Composition {
    /// **An input method's composition at `term`'s cursor**: `text`, with
    /// its `hints` and `caret`, measured as `term` measures
    /// ([`Measure::of`]).
    ///
    /// **What continues the character before the cursor is drawn on that
    /// character's cell**, as the commit will leave it, in the marked-text
    /// look: where `text` starts with what joins the cluster the core would
    /// attach it to -- the cell before the cursor, a wide one's lead, or
    /// the cursor's own past a pending wrap -- the composition starts on
    /// that cell, its text led by the cell's cluster, so a vowel sign is
    /// drawn on its consonant and a skin tone on its emoji, and what a
    /// commit widens takes the cells it will. The head takes the hints on
    /// `text`'s first codepoint, and `caret` is moved past it. At column 0,
    /// after a blank cell, and where the cell's cluster and what follows
    /// would not form one cluster, the composition starts at the cursor and
    /// what continues is drawn at commit.
    pub fn at_cursor(
        term: &Term<'_>,
        text: &[Codepoint],
        hints: &[Hints],
        caret: Caret,
    ) -> Composition {
        let (row, col) = term.cursor();
        let plain = Composition {
            row,
            col,
            text: text.to_vec(),
            measure: Measure::of(term),
            hints: hints.to_vec(),
            caret,
        };
        plain.headed(term).unwrap_or(plain)
    }

    /// The composition led by the cluster of the cell its first codepoints
    /// continue, if they continue one ([`Composition::at_cursor`]).
    fn headed(&self, term: &Term<'_>) -> Option<Composition> {
        let cells = term.row(self.row)?;
        let pending = term.wrap_pending() == baud_core::WrapPending::Yes;
        let mut at = match pending {
            true => self.col.index(),
            false => self.col.index().checked_sub(1)?,
        };
        if at > 0 && cells.get(at)?.content.is_wide_tail() {
            at -= 1;
        }
        let cell = *cells.get(at)?;
        let base = term.base_of(cell).drawn().as_codepoint()?;
        let mut text: Vec<Codepoint> = core::iter::once(base)
            .chain(term.cluster_marks(cell).filter_map(Codepoint::new))
            .collect();
        let k = text.len();
        text.extend_from_slice(&self.text);
        // The cell's cluster continued by what `text` starts with -- the
        // machine run over the cluster itself, so a flag's second half
        // joins its first -- as wide as the cell or as the commit widens it.
        let (_, laid) = clusters(&text, self.measure);
        let wide = cells.get(at + 1).is_some_and(|c| c.content.is_wide_tail());
        let first = laid.first()?;
        // Or the cell's cluster whole, where what the text starts with takes
        // a cell of its own and belongs to the syllable the cell ends
        // (`crate::syllable`): the composition is drawn from the cell, the
        // syllable across both, as the commit's will be.
        let syllable = first.codes == (0..k)
            && laid
                .get(1)
                .is_some_and(|n| crate::syllable::continues(&text[..k], n.cps[0]));
        if first.codes.start != 0
            || (first.codes.end <= k && !syllable)
            || (wide && first.span == CellSpan::One)
        {
            return None;
        }
        let hints = match self.hints.first() {
            Some(&h) => core::iter::repeat_n(h, k)
                .chain(self.hints.iter().copied())
                .collect(),
            None => Vec::new(),
        };
        Some(Composition {
            row: self.row,
            col: Col::at_index(at),
            text,
            measure: self.measure,
            hints,
            caret: match self.caret {
                Caret::At(i) => Caret::At(i + k),
                Caret::Range(s, e) => Caret::Range(s + k, e + k),
                c @ (Caret::End | Caret::Hidden) => c,
            },
        })
    }

    /// Whether it holds a letter that takes a joining form (Joining_Type
    /// D, R, L or C), which its row's layout joins.
    pub fn joins(&self) -> bool {
        self.text.iter().any(|c| baud_domain::takes_form(c.value()))
    }

    /// Its clusters laid on a screen `cols` wide from its cell, as the core
    /// writes them ([`clusters`]): one that would cross the right edge is
    /// not drawn, nor anything after it -- a composition is shown on its
    /// line, as `NSTextView` shows marked text at the insertion point.
    fn pieces(&self, cols: Cols) -> Vec<Piece> {
        self.laid(cols).1
    }

    /// [`Composition::pieces`], after how many of its codepoints at its
    /// head join the character before it and so are in none.
    fn laid(&self, cols: Cols) -> (usize, Vec<Piece>) {
        let ranged = |i: usize| match self.caret {
            Caret::Range(start, end) => (start..end).contains(&i),
            Caret::End | Caret::At(_) | Caret::Hidden => false,
        };
        let hints: Vec<Hints> = self.hints_each().take(self.text.len()).collect();
        let (lead, clusters) = clusters(&self.text, self.measure);
        let mut out: Vec<Piece> = Vec::new();
        let mut x = self.col.index();
        for c in clusters {
            let n = c.span.cells().index();
            if x + n > cols.index() {
                break;
            }
            let h = hints.get(c.codes.start).copied().unwrap_or(Hints::NONE);
            out.push(Piece {
                col: Col::at_index(x),
                hints: match c.codes.clone().any(ranged) {
                    true => h.with(Hints::SELECTION),
                    false => h,
                },
                cps: c.cps,
                span: c.span,
                codes: c.codes,
            });
            x += n;
        }
        (lead, out)
    }

    /// **The width its line gives it on `view`**: the screen's on a
    /// single-width line, and on a line that is not, the line's half
    /// (`[[source: dec-std-070]]` § 5.6.1), where its clusters are laid in
    /// the line's own character positions as a write of the same text
    /// would lay them. What [`Composition::columns`] and
    /// [`Composition::cursor`] take.
    pub fn width_on(&self, view: &baud_domain::Viewport<'_>) -> Cols {
        match self.row.index() < view.rows().index() {
            true => view.rendition(self.row).end_of_line(view.cols()),
            false => view.cols(),
        }
    }

    /// The hints on each codepoint in turn: `whole` on every one when the
    /// composition carries none, and none past the last it carries.
    fn hints_each(&self) -> impl Iterator<Item = Hints> + '_ {
        let rest = match self.hints.is_empty() {
            true => Hints::WHOLE,
            false => Hints::NONE,
        };
        self.hints.iter().copied().chain(core::iter::repeat(rest))
    }

    /// **Its cells as a write of its text leaves them**, from its column on
    /// a screen `cols` wide, in rendition `style`: each cluster's first
    /// character in its cell, a wide one's tail after it. What its row is
    /// laid out with ([`crate::bidi::Written`]); the marks it carries ride
    /// on their cell there and move with it.
    #[inline(never)]
    pub fn written(&self, cols: Cols, style: baud_domain::StyleId) -> Vec<PackedCell> {
        let mut out = Vec::new();
        for p in self.pieces(cols) {
            out.push(PackedCell {
                content: Content::codepoint(p.cps[0]),
                style,
                flags: baud_domain::CellFlags::NONE,
            });
            if p.span == CellSpan::Two {
                out.push(PackedCell {
                    content: Content::WIDE_TAIL,
                    style,
                    flags: baud_domain::CellFlags::NONE,
                });
            }
        }
        out
    }

    /// The columns it covers, `start..end`.
    pub fn columns(&self, cols: Cols) -> (usize, usize) {
        let start = self.col.index();
        // Printable ASCII is a cluster and a column a character, in every
        // `Measure`: no clusters are built to count them.
        if self.text.iter().all(|c| c.is_printable_ascii()) {
            let room = cols.index().saturating_sub(start);
            return (start, start + self.text.len().min(room));
        }
        self.columns_of_pieces(cols)
    }

    /// [`Composition::columns`], from its clusters laid out.
    fn columns_of_pieces(&self, cols: Cols) -> (usize, usize) {
        let start = self.col.index();
        let end = self.pieces(cols).last().map_or(start, |p| {
            p.col.index() + if p.span == CellSpan::Two { 2 } else { 1 }
        });
        (start, end)
    }

    /// **Where the cursor is drawn while it is held**, if anywhere: with
    /// nothing said, the cell after its last, or the last column when it
    /// runs to the edge, where a write of the same text would leave the
    /// cursor; at an index, the cell of the cluster that codepoint is in,
    /// and past the clusters the edge cut, the same last column; with a
    /// range or hidden, none.
    pub fn cursor(&self, cols: Cols) -> Option<(Row, Col)> {
        let last = cols.index().saturating_sub(1);
        let after = |(_, end): (usize, usize)| (self.row, Col::at_index(end.min(last)));
        match self.caret {
            Caret::End => Some(after(self.columns(cols))),
            Caret::At(i) => {
                // What joins the character before it is in no cluster: the
                // clusters' codepoints start after those.
                let (lead, pieces) = self.laid(cols);
                if i < lead {
                    return Some((self.row, self.col));
                }
                if let Some(p) = pieces.iter().find(|p| i < p.codes.end) {
                    return Some((self.row, p.col));
                }
                // At the count, or past the clusters the edge cut.
                Some(after(self.columns(cols)))
            }
            Caret::Range(..) | Caret::Hidden => None,
        }
    }

    /// **The cells the input method's cursor names**, for the rectangle
    /// its candidate window opens beside: a caret's cell
    /// ([`Composition::cursor`]), both cells of a wide character there,
    /// one past the clusters; a range's cells, from its first cluster to
    /// the end of its last, as foot sends them (`[[source:
    /// foot-2705e36-render]]` 1858-1870); none while the cursor is hidden
    /// or a range holds no cluster drawn.
    pub fn caret_cells(&self, cols: Cols) -> Option<(Row, Col, Cols)> {
        let width = |p: &Piece| Cols::of_index(if p.span == CellSpan::Two { 2 } else { 1 });
        let pieces = self.pieces(cols);
        match self.caret {
            Caret::End | Caret::At(_) => {
                let (row, col) = self.cursor(cols)?;
                let w = pieces
                    .iter()
                    .find(|p| p.col == col)
                    .map_or(Cols::of_index(1), width);
                Some((row, col, w))
            }
            Caret::Range(start, end) => {
                let mut cells: Option<(Col, usize)> = None;
                for p in &pieces {
                    if p.codes.start < end && start < p.codes.end {
                        let right = p.col.index() + width(p).index();
                        cells = Some(cells.map_or((p.col, right), |(l, _)| (l, right)));
                    }
                }
                cells.map(|(l, r)| (self.row, l, Cols::of_index(r - l.index())))
            }
            Caret::Hidden => None,
        }
    }
}

/// **The syllables among clusters laid along a row** (`crate::syllable`),
/// each cluster its codepoints, its visual column and its cells: each the
/// first cluster of it, how many it holds and the cells they take -- one of
/// two or more clusters only where its cells lie left to right on the
/// screen, as the grid finds a syllable along the row as drawn; on a line
/// drawn in reverse each cluster is alone, as written there.
fn units_of(clusters: &[(&[Codepoint], usize, usize)]) -> Vec<(usize, usize, usize)> {
    let laid_right = |first: usize, n: usize| {
        clusters[first..first + n]
            .windows(2)
            .all(|w| w[1].1 == w[0].1 + w[0].2)
    };
    let grouped = crate::syllable::of_clusters(clusters.iter().map(|&(cps, _, n)| (cps, n)));
    let mut units = Vec::new();
    let mut i = 0;
    while i < clusters.len() {
        match grouped.iter().find(|g| g.0 == i && laid_right(g.0, g.1)) {
            Some(&g) => {
                units.push(g);
                i += g.1;
            }
            None => {
                units.push((i, 1, clusters[i].2));
                i += 1;
            }
        }
    }
    units
}

/// **The cells an input method's composition draws on its row**, laid as
/// written ([`Laid::AsWritten`]), in visual columns: its own, and the
/// letters of a Devanagari word written against it that its own letters
/// continue (`crate::word`) -- one word with them once the commit is
/// written, so the composition draws that word whole, as the commit's
/// will be drawn, and the grid draws none of it -- each with what it will
/// hold, which the row's other words are laid by.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Taken {
    row: Option<Row>,
    /// Each visual column it draws and what it will hold.
    held: Vec<(usize, crate::word::Held)>,
    /// The letters written against it that its words take in, left and
    /// right: their first visual column and how many.
    beside: [Option<(usize, usize)>; 2],
    /// Its words as it lays them ([`Composed::inks`]).
    inks: Vec<(usize, f32)>,
}

impl Taken {
    /// Whether the composition is on row `r`.
    pub(crate) fn on(&self, r: Row) -> bool {
        self.row == Some(r)
    }

    /// Its words as it lays them this frame ([`Composed::inks`]).
    pub(crate) fn set_inks(&mut self, inks: Vec<(usize, f32)>) {
        self.inks = inks;
    }

    /// **The word before visual column `x` of row `r` that a word there
    /// closes on** (`crate::word::lay`): of `grid`, the row's own word laid
    /// last before it -- its first column and its ink's right edge -- and
    /// the composition's last word before it, the one nearer, as the
    /// commit's row will lay them in turn.
    pub(crate) fn ink_before(&self, r: Row, x: Col, grid: Option<(Col, f32)>) -> Option<f32> {
        let own = match self.row == Some(r) {
            true => self
                .inks
                .iter()
                .rev()
                .find(|&&(at, _)| at < x.index())
                .copied(),
            false => None,
        };
        match (grid, own) {
            (Some((g, gi)), Some((o, oi))) => Some(if o > g.index() { oi } else { gi }),
            (Some((_, gi)), None) => Some(gi),
            (None, Some((_, oi))) => Some(oi),
            (None, None) => None,
        }
    }

    /// What visual column `x` of row `r` will hold, where the composition
    /// draws it.
    pub(crate) fn held(&self, r: Row, x: Col) -> Option<crate::word::Held> {
        match self.row == Some(r) {
            true => self.held.iter().find(|h| h.0 == x.index()).map(|h| h.1),
            false => None,
        }
    }

    /// The visual columns of row `r` written beside the composition that
    /// its words take in: what the grid converts blank and draws unlit,
    /// their light drawn over their syllables' spans by the composition.
    pub(crate) fn beside(&self, r: Row) -> impl Iterator<Item = usize> + '_ {
        self.beside
            .iter()
            .flatten()
            .filter(move |_| self.row == Some(r))
            .flat_map(|&(at, n)| at..at + n)
    }
}

impl Composition {
    /// **The cells it draws on its row** ([`Taken`]) on a line `size`
    /// drawn in `orders`, `cols` wide: the row read with its own cells blank,
    /// as the grid reads it, and a word of that row's written against its
    /// first or last cluster taken in where that cluster leads a syllable
    /// with a letter, the row's text running on across it. None on a line
    /// that is not single-width, where no word is laid.
    pub(crate) fn taken(
        &self,
        term: &Term<'_>,
        orders: &crate::bidi::Orders,
        cols: Cols,
        size: LineRendition,
    ) -> Taken {
        let pieces = self.pieces(cols);
        if size.is_double()
            || pieces.is_empty()
            || self.row.index() >= term.viewport().rows().index()
        {
            return Taken::default();
        }
        let row = self.row;
        let placed: Vec<(&Piece, usize)> = pieces
            .iter()
            .map(|p| {
                let v = Laid::AsWritten.column(orders, row, p.col);
                let v = match p.span {
                    CellSpan::Two => v.min(Laid::AsWritten.column(orders, row, p.col.after())),
                    CellSpan::One => v,
                };
                (p, v.index())
            })
            .collect();
        let mut held = Vec::new();
        for &(p, v) in &placed {
            for k in 0..p.span.cells().index() {
                let h = match p.cps.first() {
                    Some(&cp) if cp == Codepoint::from_char(' ') => crate::word::Held::Blank,
                    Some(&cp) if crate::word::is_letter(cp) && k == 0 => crate::word::Held::Letter,
                    _ => crate::word::Held::Other,
                };
                held.push((v + k, h));
            }
        }
        let clusters: Vec<(&[Codepoint], usize, usize)> = placed
            .iter()
            .map(|&(p, v)| (p.cps.as_slice(), v, p.span.cells().index()))
            .collect();
        let units = units_of(&clusters);
        let lettered = |i: usize| {
            clusters[i]
                .0
                .first()
                .is_some_and(|&c| crate::word::is_letter(c))
        };
        let width = cols.index();
        let screen = term.viewport();
        let row_cells = screen.row(row);
        let logical = |v: Col| orders.logical_col(row, v);
        let mut drawn: Vec<PackedCell> = (0..width)
            .map(|v| {
                row_cells
                    .get(logical(Col::at_index(v)).index())
                    .copied()
                    .unwrap_or(PackedCell::EMPTY)
            })
            .collect();
        for &(x, _) in &held {
            if let Some(cell) = drawn.get_mut(x) {
                *cell = PackedCell::EMPTY;
            }
        }
        // The live screen's row, whose ids name the live table.
        let (parted, letters) = crate::syllable::of_row((term, Words::LIVE), &drawn, logical);
        let words = match letters {
            crate::word::Letters::Seen => {
                crate::word::of_row(term, &drawn, logical, &parted, |_| None)
            }
            crate::word::Letters::None => Vec::new(),
        };
        // The row's text runs on from the cell before `v` into `v`.
        let follows = |v: usize| {
            v > 0
                && v < width
                && logical(Col::at_index(v)).index() == logical(Col::at_index(v - 1)).index() + 1
        };
        let mut beside = [None, None];
        // A paragraph the layout re-broke draws other cells on the row
        // than the core's: there the row's letters stay the grid's.
        let units = match orders.display() {
            Some(_) => Vec::new(),
            None => units,
        };
        if let (Some(first), Some(last)) = (units.first(), units.last()) {
            let left = clusters[first.0].1;
            if lettered(first.0) && placed.iter().all(|&(_, v)| v >= left) && follows(left) {
                beside[0] = words
                    .iter()
                    .find(|w| w.end().index() == left)
                    .map(|w| (w.at.index(), w.cells.index()));
            }
            let right = clusters[last.0].1 + last.2;
            let ends = placed
                .iter()
                .all(|&(p, v)| v + p.span.cells().index() <= right);
            if lettered(last.0) && ends && follows(right) {
                beside[1] = words
                    .iter()
                    .find(|w| w.at.index() == right)
                    .map(|w| (w.at.index(), w.cells.index()));
            }
        }
        for &(at, n) in beside.iter().flatten() {
            held.extend((at..at + n).map(|x| (x, crate::word::Held::Letter)));
        }
        Taken {
            row: Some(row),
            held,
            beside,
            inks: Vec::new(),
        }
    }
}

/// Whether a composition's cluster is written where SAPV 17 slants an
/// italic left ([`crate::bidi::Orders::slants`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Leans {
    Left,
    No,
}

impl Leans {
    /// How far a cluster drawn in `style` leans, and the style it is
    /// drawn in: its upright one, sheared, where it leans and the stack
    /// has an italic face for it, as the same text written is drawn.
    fn lean(
        self,
        r: &Renderer<'_, '_>,
        style: crate::stack::TextFace,
    ) -> (Option<crate::stack::Lean>, crate::stack::TextFace) {
        let lean = match self {
            Leans::Left => r.lean(style),
            Leans::No => None,
        };
        (lean, lean.map_or(style, |_| style.upright()))
    }
}

impl Composition {
    /// **Whether a cluster at visual column `v`, `span` wide, leans left**:
    /// laid as written, where the row's order slants the cell written
    /// there -- either of a wide one's two, its lead's -- as the same text
    /// written is slanted; on screen, never.
    fn leans(&self, orders: &crate::bidi::Orders, laid: Laid, v: Col, span: CellSpan) -> Leans {
        let at = |v: Col| laid == Laid::AsWritten && orders.slanted(self.row, v);
        match at(v) || (span == CellSpan::Two && at(v.after())) {
            true => Leans::Left,
            false => Leans::No,
        }
    }

    /// **The character a cluster at visual column `v` is drawn as, and
    /// whether flipped**: laid as written, the mirror L4 gives the cell
    /// there -- its Bidi_Mirroring_Glyph, alone or under its marks, or its
    /// own glyph flipped, a box line two cells wide under `?2500` mirrored
    /// whole -- as the same text written is drawn; on screen, `base` as it
    /// is.
    fn mirror(
        &self,
        orders: &crate::bidi::Orders,
        laid: Laid,
        v: Col,
        base: Codepoint,
    ) -> (Codepoint, Flip) {
        if laid == Laid::OnScreen {
            return (base, Flip::Kept);
        }
        let swapped = orders.swapped(self.row, v).and_then(|s| s.as_codepoint());
        match orders.mirrored(self.row, v) {
            Some(crate::bidi::Mirrored::Flipped) => (swapped.unwrap_or(base), Flip::Flipped),
            Some(crate::bidi::Mirrored::Under(m)) => (m, Flip::Kept),
            None => (swapped.unwrap_or(base), Flip::Kept),
        }
    }
}

/// The records that draw a composition: each cluster's glyphs in the
/// rendition of `under`, the cell at the composition's start, with what
/// its hints add ([`Composition`]), coloured as `Overlay::emit` colours
/// any record -- the selection's and the block cursor's swaps included --
/// a selected cluster's page under them, and each cell's line over them,
/// at the face's positions and thickness. The instance and CPU paths draw
/// these after their rows; the cell path after its passes.
///
/// **On a line that is not single-width** (`size`) every glyph, page and
/// line is drawn at the box a character there takes, as the line's
/// own text is (`[[source: dec-std-070]]` § 5.6.1): rasterised at the
/// doubled size ([`Renderer::cluster_at`]), and on a double-height line
/// each row's half of it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composition_records(
    r: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    (c, laid): (&Composition, Laid),
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    size: LineRendition,
    pan: Px,
    records: &mut Vec<Instance>,
) {
    composition_parts(r, term, (c, laid), under, styles, block, cols, size, pan).append_to(records);
}

/// **A composition's records in their parts**: the selection's pages under
/// its glyphs, each cluster's glyphs by the visual column it is drawn at,
/// left to right -- as `Renderer::convert_row` draws a row's -- and each
/// cell's line over them.
#[derive(Default)]
pub(crate) struct Composed {
    pub(crate) pages: Vec<Instance>,
    /// Each cluster's visual column, its glyphs' records inside its row,
    /// and their parts above it.
    pub(crate) glyphs: Vec<(usize, Vec<Instance>, Vec<Instance>)>,
    pub(crate) rules: Vec<Instance>,
    /// Each Devanagari word it draws ([`Taken::ink_before`]): its first
    /// visual column and its ink's right edge, in pixels from the row's
    /// left edge.
    pub(crate) inks: Vec<(usize, f32)>,
}

impl Composed {
    /// **Its pages and glyphs, taken to be drawn in the order of row
    /// `row`**, on a row `orders` joined, where its letters' ink meets that
    /// of the letters beside them ([`crate::renderer::Splice`]); none, and
    /// nothing taken, on any other.
    pub(crate) fn splice(
        &mut self,
        orders: &crate::bidi::Orders,
        row: Row,
    ) -> Option<crate::renderer::Splice> {
        orders.joins(row)?;
        Some(crate::renderer::Splice {
            row,
            pages: core::mem::take(&mut self.pages),
            glyphs: core::mem::take(&mut self.glyphs)
                .into_iter()
                .map(|(x, mut inside, above)| {
                    inside.extend(above);
                    (x, inside)
                })
                .collect(),
        })
    }

    /// The pages, every cluster's glyphs left to right, then the rules.
    pub(crate) fn append_to(self, records: &mut Vec<Instance>) {
        records.extend(self.pages);
        for (_, inside, above) in self.glyphs {
            records.extend(inside);
            records.extend(above);
        }
        records.extend(self.rules);
    }
}

/// [`composition_records`], in its parts ([`Composed`]).
#[allow(clippy::too_many_arguments)]
pub(crate) fn composition_parts(
    r: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    (c, laid): (&Composition, Laid),
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    size: LineRendition,
    pan: Px,
) -> Composed {
    // Only a block swaps a cell's colours; a bar or an underline the input
    // method put inside the composition is drawn over its row as anywhere.
    let block = block.filter(|(_, _, s)| {
        r.cursor_look() == CursorLook::Shaped
            && matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock)
    });
    if size.is_double() {
        return sized_composition(r, term, (c, laid), under, styles, block, cols, size, pan);
    }
    let geom = Geom {
        m: r.metrics(),
        cols,
        pan,
    };
    let (blink, cursor, hilite) = (r.blink(), opacity_of(r, block), r.field_hilite());
    let block_paint = r.block_paint();
    let frame = r.frame_held();
    let base = hinted_base((term, frame.words()), styles, under);
    let y = c.row.index();
    let m = geom.m;
    // **Each cluster's visual cell, and the character drawn there**: laid
    // as written, the cell the row's order draws its logical cell at (a
    // wide one's two cells kept together, its lead on the left) and the
    // mirror L4 gives it there, flipped where the written cell is
    // ([`Composition::mirror`]); on screen, its own column and character.
    let orders = r.bidi();
    let placed: Vec<(Piece, usize, (Codepoint, Flip, Leans))> = c
        .pieces(cols)
        .into_iter()
        .map(|p| {
            let v = laid.column(orders, c.row, p.col);
            let v = match p.span {
                CellSpan::Two => v.min(laid.column(orders, c.row, p.col.after())),
                CellSpan::One => v,
            };
            let (shown, flip) = c.mirror(orders, laid, v, p.cps[0]);
            let leans = c.leans(orders, laid, v, p.span);
            (p, v.index(), (shown, flip, leans))
        })
        .collect();
    let block = block.map(|(br, bc, s)| (br, laid.column(orders, br, bc), s));
    let px = |p: Px| i32::try_from(p.value()).unwrap_or(0);
    let at = |v: i32| Screen(i16::try_from(v).unwrap_or(i16::MAX));
    let (cell_w, cell_h) = (px(m.cell_w), px(m.cell_h));
    let row_top = i32::try_from(y).unwrap_or(0) * cell_h + px(pan);
    // The selection's pages under every glyph, the glyphs, then each
    // cell's line over them, as `Renderer::convert_row` orders a row.
    let (mut pages, mut glyphs_out, mut rules) = (Vec::new(), Vec::new(), Vec::new());
    let styled: Vec<(
        Drawing,
        Resolved,
        (Option<crate::stack::Lean>, crate::stack::TextFace),
    )> = placed
        .iter()
        .map(|(p, _, (_, _, leans))| {
            let look = Drawing::of(p.hints);
            let one = base.of(r, term, look);
            (look, one, leans.lean(r, one.shapes.text_face()))
        })
        .collect();
    let mut joined = joined_pieces(
        r,
        (term, styles),
        (c.row, laid),
        &placed
            .iter()
            .zip(&styled)
            .map(|((p, x0, _), (_, _, (_, style)))| (*x0, p.cps.as_slice(), p.span, *style))
            .collect::<Vec<_>>(),
        Stretch::One,
    );
    // **The letters written beside it that its words take in** ([`Taken`]),
    // which the grid leaves to it, on a row laid as written.
    let taken = match laid {
        Laid::AsWritten => c.taken(term, r.bidi(), cols, size),
        Laid::OnScreen => Taken::default(),
    };
    let screen = term.viewport();
    let row_cells = screen.row(c.row);
    // The cell each visual column shows, as the row's order draws it.
    let shows: Vec<usize> = (0..cols.index())
        .map(|v| r.bidi().logical_col(c.row, Col::at_index(v)).index())
        .collect();
    let cell_at = |v: usize| shows.get(v).and_then(|&l| row_cells.get(l)).copied();
    // **Every cluster in the order the row draws them**: the letters taken
    // in on its left, its own, then those on its right -- each its
    // codepoints, its visual column, its cells, and the piece it is.
    let mut all: Vec<(Vec<Codepoint>, usize, usize, Option<usize>)> = Vec::new();
    let take = |all: &mut Vec<(Vec<Codepoint>, usize, usize, Option<usize>)>,
                side: Option<(usize, usize)>| {
        for v in side.into_iter().flat_map(|(at, n)| at..at + n) {
            if let Some(cell) = cell_at(v).filter(|c| !c.content.is_wide_tail()) {
                all.push((
                    crate::renderer::cell_cps(term, frame.words(), cell),
                    v,
                    1,
                    None,
                ));
            }
        }
    };
    take(&mut all, taken.beside[0]);
    all.extend(
        placed
            .iter()
            .enumerate()
            .map(|(i, (p, v, _))| (p.cps.clone(), *v, p.span.cells().index(), Some(i))),
    );
    take(&mut all, taken.beside[1]);
    // **The syllables among them** (`crate::syllable`), across the letters
    // taken in, as the commit's row will part them.
    let units = units_of(
        &all.iter()
            .map(|(cps, v, n, _)| (cps.as_slice(), *v, *n))
            .collect::<Vec<_>>(),
    );
    // What a cell of the row holds once the commit is written: the
    // composition's cluster over the cells it covers, the row's elsewhere.
    use crate::word::Held;
    let held = |x: usize| {
        if let Some((p, v, _)) = placed
            .iter()
            .find(|(p, v, _)| x >= *v && x < v + p.span.cells().index())
        {
            return match p.cps.first() {
                Some(&cp) if cp == Codepoint::from_char(' ') => Held::Blank,
                Some(&cp) if crate::word::is_letter(cp) && x == *v => Held::Letter,
                _ => Held::Other,
            };
        }
        match cell_at(x) {
            Some(cell) if crate::fit::Blank::of(term, cell) == crate::fit::Blank::Yes => {
                Held::Blank
            }
            Some(cell)
                if crate::renderer::drawn_base(term, cell).is_some_and(crate::word::is_letter) =>
            {
                Held::Letter
            }
            _ => Held::Other,
        }
    };
    let side = |x: Option<usize>| match x.filter(|&x| x < cols.index()).map(held) {
        None => crate::word::Side::Edge,
        Some(Held::Letter) => crate::word::Side::Word,
        Some(_) => crate::word::Side::Wall,
    };
    // **What lights each cell of the row** -- the selection, and the block
    // cursor, which `block` puts at its visual cell -- and the colours a
    // cell outside the composition draws: a letter taken in unlit, its
    // light drawn over its syllable's span, as the grid draws a word's.
    let highlight = r.highlight();
    let lights: Vec<crate::word::Light> = (0..cols.index())
        .map(|x| {
            let cell = cell_at(x).unwrap_or(PackedCell::EMPTY);
            let l = Col::at_index(shows.get(x).copied().unwrap_or(x));
            crate::word::Light {
                selected: match highlight
                    .is_some_and(|h| r.bidi().selects(&h, c.row, l, cell.content))
                {
                    true => Selected::Yes,
                    false => Selected::No,
                },
                block: match block.is_some_and(|(br, bc, _)| br == c.row && bc.index() == x) {
                    true => BlockCursor::Yes(block_paint),
                    false => BlockCursor::No,
                },
                cursor,
            }
        })
        .collect();
    let beside: Vec<usize> = taken.beside(c.row).collect();
    let row_drawn = |cell: PackedCell, l: crate::word::Light| {
        styles
            .get(cell.style.index())
            .copied()
            .unwrap_or_default()
            .drawn(Swaps {
                pictured: Pictured::No,
                line: Line::of(cell.content),
                selected: l.selected,
                hilite,
                block: l.block,
                blink,
                cursor: l.cursor,
            })
    };
    // A cluster of the composition drawn under `l` and its look's own
    // selection.
    let piece_drawn = |i: usize, l: crate::word::Light| {
        let (look, one, _) = &styled[i];
        one.drawn(Swaps {
            pictured: Pictured::No,
            line: Line::No,
            selected: match (look.selected, l.selected) {
                (Selected::No, Selected::No) => Selected::No,
                _ => Selected::Yes,
            },
            hilite,
            block: l.block,
            blink,
            cursor: l.cursor,
        })
    };
    let colours = |d: crate::cellmap::Drawn| crate::word::Colours {
        fg: d.fg,
        bg: d.bg,
        curve: d.curve,
    };
    // **A Devanagari word among them** (`crate::word`) is drawn whole by the
    // word rule ([`Renderer::word_drawn`]), in the room the row leaves it,
    // as the commit's text will be: its glyphs from its syllables' pens,
    // the light on a letter taken in over its syllable's span, the ink over
    // each cell's ground in that ground's text colour -- the block on a
    // cell of its own, which the grid draws, among them. Its clusters draw
    // no glyph of their own. A syllable no word holds is drawn whole from
    // its first cluster's cell across its cells.
    let letter = |u: &(usize, usize, usize)| {
        all[u.0]
            .0
            .first()
            .is_some_and(|&c| crate::word::is_letter(c))
    };
    // **The row's own words, as the grid lays them**: a word that fits at
    // no width closes on the word before it by that word's ink
    // (`crate::word::lay`), so each of its words is laid after the row's
    // words before it, as the commit's row will lay them in turn, and the
    // row's word after it closes on its ink ([`Taken::ink_before`]).
    let grid_row: Vec<PackedCell> = (0..cols.index())
        .map(|v| match taken.held(c.row, Col::at_index(v)) {
            Some(_) => PackedCell::EMPTY,
            None => cell_at(v).unwrap_or(PackedCell::EMPTY),
        })
        .collect();
    let logical_of = |v: Col| Col::at_index(shows.get(v.index()).copied().unwrap_or(v.index()));
    let grid_words = match laid {
        Laid::OnScreen => Vec::new(),
        Laid::AsWritten => {
            match crate::syllable::of_row((term, frame.words()), &grid_row, logical_of) {
                (parted, crate::word::Letters::Seen) => {
                    crate::word::of_row(term, &grid_row, logical_of, &parted, |x| {
                        taken.held(c.row, Col::at_index(x))
                    })
                }
                (_, crate::word::Letters::None) => Vec::new(),
            }
        }
    };
    let mut next_grid = 0usize;
    let mut last: Option<f32> = None;
    let mut inks: Vec<(usize, f32)> = Vec::new();
    let mut worded: Vec<(usize, Vec<Instance>, Vec<Instance>)> = Vec::new();
    let mut k = 0;
    while k < units.len() {
        if !letter(&units[k]) {
            let (first, n, cells) = units[k];
            if let (true, Some(piece)) = (n > 1, all[first].3) {
                let cps: Vec<Codepoint> = all[first..first + n]
                    .iter()
                    .flat_map(|cl| cl.0.iter().copied())
                    .collect();
                let style = styled[piece].2.1;
                if let Some(slot) = joined.get_mut(piece) {
                    *slot = Some(r.syllable(&cps, style, Cols::of_index(cells), Stretch::One));
                }
                for cl in &all[first + 1..first + n] {
                    if let Some(slot) = cl.3.and_then(|i| joined.get_mut(i)) {
                        *slot = Some(Vec::new());
                    }
                }
            }
            k += 1;
            continue;
        }
        let mut m = k + 1;
        while m < units.len()
            && letter(&units[m])
            && all[units[m].0].1 == all[units[m - 1].0].1 + units[m - 1].2
        {
            m += 1;
        }
        let word = &units[k..m];
        let at = all[word[0].0].1;
        let cells: usize = word.iter().map(|u| u.2).sum();
        let syllables: Vec<crate::syllable::Syllable> = word
            .iter()
            .map(|u| crate::syllable::Syllable {
                at: Col::at_index(all[u.0].1),
                cells: Cols::of_index(u.2),
            })
            .collect();
        let parts: Vec<Vec<Codepoint>> = word
            .iter()
            .map(|u| {
                all[u.0..u.0 + u.1]
                    .iter()
                    .flat_map(|cl| cl.0.iter().copied())
                    .collect()
            })
            .collect();
        // Each cell's characters: a cluster's at its first.
        let mut counts = vec![0usize; cells];
        for cl in word.iter().flat_map(|u| &all[u.0..u.0 + u.1]) {
            if let Some(h) = cl.1.checked_sub(at).and_then(|x| counts.get_mut(x)) {
                *h = cl.0.len();
            }
        }
        let text = crate::word::WordText {
            parts,
            held: counts,
        };
        let (mut l, mut rr) = (at, at + cells);
        while l > 0 && held(l - 1) == Held::Blank {
            l -= 1;
        }
        while rr < cols.index() && held(rr) == Held::Blank {
            rr += 1;
        }
        let room = crate::word::Room {
            left: (crate::word::Blanks(at - l), side(l.checked_sub(1))),
            right: (crate::word::Blanks(rr - at - cells), side(Some(rr))),
        };
        // Each cell's face is its cluster's, as the grid's word takes it.
        let mut faces = vec![crate::TextFace::default(); cells];
        for cl in word.iter().flat_map(|u| &all[u.0..u.0 + u.1]) {
            let face = match cl.3 {
                Some(i) => styled[i].2.1,
                None => crate::cellmap::face_of(styles, cell_at(cl.1).unwrap_or(PackedCell::EMPTY)),
            };
            for x in cl.1..cl.1 + cl.2 {
                if let Some(f) = x.checked_sub(at).and_then(|x| faces.get_mut(x)) {
                    *f = face;
                }
            }
        }
        // Each syllable in its first cluster's colours under the light of
        // the letters taken in that it holds; the composition's own cells
        // are lit by the grid, as the cells under it are.
        let paint = crate::word::WordPaint {
            syllables: word
                .iter()
                .map(|u| {
                    let light = all[u.0..u.0 + u.1]
                        .iter()
                        .filter(|cl| cl.3.is_none())
                        .flat_map(|cl| cl.1..cl.1 + cl.2)
                        .fold(crate::word::Light::NONE, |l, x| {
                            l.with(lights.get(x).copied().unwrap_or(crate::word::Light::NONE))
                        });
                    let d = match all[u.0].3 {
                        Some(i) => piece_drawn(i, light),
                        None => row_drawn(cell_at(all[u.0].1).unwrap_or(PackedCell::EMPTY), light),
                    };
                    (colours(d), light, d.text)
                })
                .collect(),
        };
        // Every cell of the row as the word's ink sees it: the
        // composition's in its look under the block where it sits, a letter
        // taken in unlit, any other as the grid draws it.
        let under: Vec<crate::word::Under> = (0..cols.index())
            .map(|x| {
                if let Some(i) = placed
                    .iter()
                    .position(|(p, v, _)| x >= *v && x < v + p.span.cells().index())
                {
                    let l = crate::word::Light {
                        selected: Selected::No,
                        block: lights.get(x).map_or(BlockCursor::No, |l| l.block),
                        cursor,
                    };
                    return crate::word::Under {
                        colours: colours(piece_drawn(i, l)),
                        light: crate::word::Light::NONE,
                    };
                }
                let lit = lights.get(x).copied().unwrap_or(crate::word::Light::NONE);
                let drawn_as = match beside.contains(&x) {
                    true => crate::word::Light::NONE,
                    false => lit,
                };
                crate::word::Under {
                    colours: colours(row_drawn(cell_at(x).unwrap_or(PackedCell::EMPTY), drawn_as)),
                    light: lit,
                }
            })
            .collect();
        let w = crate::word::Word {
            at: Col::at_index(at),
            cells: Cols::of_index(cells),
            syllables,
            room,
        };
        while let Some(g) = grid_words.get(next_grid).filter(|g| g.at.index() < at) {
            let cut = crate::renderer::word_text(term, frame.words(), &grid_row, g);
            let faces = crate::word::faces_of(styles, &grid_row, g);
            last = r
                .word_ink(
                    (&cut, &g.syllables),
                    &faces,
                    (g.at, g.cells, g.room),
                    (cols, last),
                )
                .or(last);
            next_grid += 1;
        }
        let mut drawn_word = Vec::new();
        if let Some(l) = r.word_drawn(
            c.row,
            (&text, &faces),
            (&w, &paint, last),
            (&under, cols),
            &mut drawn_word,
        ) {
            last = Some(l.ink.1);
            inks.push((at, l.ink.1));
        }
        let top = Screen(i16::try_from(row_top).unwrap_or(i16::MAX));
        let (mut inside, mut over) = (Vec::new(), Vec::new());
        for inst in drawn_word {
            let moved = Instance {
                y: Screen(i16::try_from(i32::from(inst.y.0) + px(pan)).unwrap_or(i16::MAX)),
                ..inst
            };
            let (up, inner) = moved.cut_at(top);
            over.extend(up);
            inside.extend(inner);
        }
        worded.push((at, inside, over));
        for cl in word.iter().flat_map(|u| &all[u.0..u.0 + u.1]) {
            if let Some(slot) = cl.3.and_then(|i| joined.get_mut(i)) {
                *slot = Some(Vec::new());
            }
        }
        k = m;
    }
    for (i, ((p, x0, (shown, flip, _)), (look, one, (lean, style)))) in
        placed.into_iter().zip(styled).enumerate()
    {
        let one = [one];
        let n = if p.span == CellSpan::Two { 2 } else { 1 };
        let mut cps = p.cps.clone();
        cps[0] = shown;
        let drawn = |x: usize| {
            let under_cursor = block.is_some_and(|(br, bc, _)| br == c.row && bc.index() == x);
            one[0].drawn(Swaps {
                pictured: Pictured::No,
                line: Line::No,
                selected: look.selected,
                hilite,
                block: if under_cursor {
                    BlockCursor::Yes(block_paint)
                } else {
                    BlockCursor::No
                },
                blink,
                cursor,
            })
        };
        let (mut glyphs, flip) = match (joined.get_mut(i).and_then(Option::take), flip) {
            (Some(g), _) => (g, Flip::Kept),
            (None, Flip::Flipped) => (r.cluster_flipped(&cps, style, p.span), flip),
            (None, Flip::Kept) => (r.cluster(&cps, style, p.span), flip),
        };
        if let Some(l) = lean {
            r.shear_glyphs(&mut glyphs, l, Stretch::One, flip);
        }
        let cell = PackedCell {
            content: Content::codepoint(cps[0]),
            style: baud_domain::StyleId::DEFAULT,
            flags: baud_domain::CellFlags::NONE,
        };
        let paint = Paint {
            styles: crate::RowTables::one(&one),
            block,
            highlight: (look.selected == Selected::Yes).then(|| {
                Highlight::new(
                    (c.row, Col::at_index(x0)),
                    (c.row, Col::at_index(x0 + n - 1)),
                    crate::select::Region::Stream,
                )
            }),
            blink,
            cursor,
            pictures: None,
            hilite,
            block_paint,
            orders: None,
        };
        let (mut inside, mut over) = (Vec::new(), Vec::new());
        Overlay::emit(
            &mut inside,
            &mut over,
            (x0, y),
            &glyphs,
            cell,
            &paint,
            &geom,
        );
        glyphs_out.push((x0, inside, over));
        for x in x0..x0 + n {
            let d = drawn(x);
            let left = i32::try_from(x).unwrap_or(0) * cell_w;
            if look.selected == Selected::Yes || matches!(look.pair, Paired::Yes | Paired::Default)
            {
                pages.push(bar(at(left), at(row_top), m.cell_w, m.cell_h, d.bg));
            }
            if !d.text.draws() {
                continue;
            }
            let place = RuleAt {
                left,
                top: row_top,
                width: cell_w,
                height: cell_h,
                thick: px(m.thickness),
                under: px(m.underline),
                strike: px(m.strike),
            };
            place.push(look.rule, d.fg, d.rule(one[0].underline), &mut |i| {
                rules.push(i)
            });
        }
    }
    glyphs_out.extend(worded);
    glyphs_out.sort_by_key(|&(x, _, _)| x);
    Composed {
        pages,
        glyphs: glyphs_out,
        rules,
        inks,
    }
}

/// **What the row's joining draws a composition's clusters as**, laid as
/// written ([`Laid::AsWritten`]): the row was laid out with the
/// composition's text written in it ([`crate::bidi::Written`]), so each
/// cluster at a visual column the layout joined takes its form there, with
/// the cells of its word around it, as the same text written is drawn
/// (`[[source: unicode-ch9]]` 9.2.2), and a lam and an alef it holds as a
/// pair draw their ligature over their two cells (9.2.3). A pair whose
/// other letter is written beside it is drawn by that letter's cell, and
/// one a face forms no ligature for letter by letter in its forms. Each
/// of `pieces` is a cluster's
/// visual column, its codepoints, its span and the style it is drawn in,
/// and the answer is its glyphs from its own cell's origin, on a line drawn
/// at `st`, or none where joining gives it nothing. On screen
/// ([`Laid::OnScreen`]), or on a row with no joined cell, none at all.
///
/// Out of line: it runs only while a composition is shown on a row that
/// joins.
#[inline(never)]
fn joined_pieces(
    r: &mut Renderer<'_, '_>,
    (term, styles): (&Term<'_>, &[Resolved]),
    (row, laid): (Row, Laid),
    pieces: &[(usize, &[Codepoint], CellSpan, crate::stack::TextFace)],
    st: Stretch,
) -> Vec<Option<Vec<ClusterGlyph>>> {
    use crate::bidi::{PairEnd, PairPart};
    use crate::renderer::{LamAt, PairSpan};
    let mut out: Vec<Option<Vec<ClusterGlyph>>> = vec![None; pieces.len()];
    if laid == Laid::OnScreen || r.bidi().joins(row).is_none() {
        return out;
    }
    let joined: Vec<Option<crate::bidi::Joined>> = pieces
        .iter()
        .map(|&(v, _, span, _)| match span {
            CellSpan::One => r.bidi().joined(row, Col::at_index(v)),
            CellSpan::Two => None,
        })
        .collect();
    // Each pair whose two letters are composed, from its left cell.
    for (i, j) in joined.iter().enumerate() {
        let Some((part, _)) = j.and_then(|j| j.pair) else {
            continue;
        };
        let (PairPart::Lam(n, PairEnd::Left) | PairPart::Alef(n, PairEnd::Left)) = part else {
            continue;
        };
        let o = pieces[i].0 + n.index().saturating_sub(1);
        let Some(k) = pieces.iter().position(|p| p.0 == o).filter(|&k| k != i) else {
            continue;
        };
        let (lam, alef, lam_at) = match (part, joined[k].and_then(|j| j.pair)) {
            (PairPart::Lam(..), Some((PairPart::Alef(h, PairEnd::Right), _))) if h == n => {
                (i, k, LamAt::Left)
            }
            (PairPart::Alef(..), Some((PairPart::Lam(h, PairEnd::Right), _))) if h == n => {
                (k, i, LamAt::Right)
            }
            _ => continue,
        };
        let Some(lam_form) = joined[lam].map(|j| j.form) else {
            continue;
        };
        let (lam_v, lam_cps, _, style) = pieces[lam];
        let face = r.face_for(lam_cps, style);
        let frame = r.frame_held();
        let word = r.pair_word(
            (term, styles, frame.words()),
            row,
            (lam_v, Some(pieces[alef].0)),
            face,
        );
        if let Some(g) = r.lam_alef(
            (lam_cps, pieces[alef].1),
            lam_form,
            style,
            (PairSpan::Cells(lam_at, n), &word),
            st,
        ) {
            out[lam] = Some(g.lams);
            out[alef] = Some(g.alefs);
        }
    }
    // **A pair whose other letter is written beside the composition** is
    // drawn whole by that letter's cell, which reads this one as written
    // (`Renderer::shown_cell`), where the face forms it: the cluster here
    // draws nothing then.
    for (i, j) in joined.iter().enumerate() {
        let Some((part, _)) = j.and_then(|j| j.pair).filter(|_| out[i].is_none()) else {
            continue;
        };
        let (n, end) = match part {
            PairPart::Lam(n, e) | PairPart::Alef(n, e) => (n, e),
            PairPart::LamWrapped(_) | PairPart::AlefWrapped(_) => continue,
        };
        let v = pieces[i].0;
        let far = n.index().saturating_sub(1);
        let Some(o) = (match end {
            PairEnd::Left => v.checked_add(far),
            PairEnd::Right => v.checked_sub(far),
        }) else {
            continue;
        };
        if o == v || pieces.iter().any(|p| p.0 == o) {
            continue;
        }
        let (Some(here), Some(there)) = (
            r.written_cell(row, Col::at_index(v)),
            r.written_cell(row, Col::at_index(o)),
        ) else {
            continue;
        };
        let partner = r.bidi().joined(row, Col::at_index(o));
        let (lam_cell, alef_cell, lam_v, alef_v, lam_at, lam_form) =
            match (part, partner.and_then(|p| p.pair)) {
                (PairPart::Lam(..), Some((PairPart::Alef(h, e), _))) if h == n && e != end => {
                    (here, there, v, o, o > v, j.map(|j| j.form))
                }
                (PairPart::Alef(..), Some((PairPart::Lam(h, e), _))) if h == n && e != end => {
                    (there, here, o, v, v > o, partner.map(|p| p.form))
                }
                _ => continue,
            };
        let Some(lam_form) = lam_form else {
            continue;
        };
        let lam_at = match lam_at {
            true => LamAt::Left,
            false => LamAt::Right,
        };
        // The lam's style, upright where SAPV 17 slants its cell, as that
        // cell's conversion reads it.
        let style = crate::cellmap::face_of(styles, lam_cell);
        let style = match r.bidi().slanted(row, Col::at_index(lam_v)) && r.lean(style).is_some() {
            true => style.upright(),
            false => style,
        };
        let frame = r.frame_held();
        let lam_cps = crate::renderer::cell_cps(term, frame.words(), lam_cell);
        let alef_cps = crate::renderer::cell_cps(term, frame.words(), alef_cell);
        let face = r.face_for(&lam_cps, style);
        let word = r.pair_word(
            (term, styles, frame.words()),
            row,
            (lam_v, Some(alef_v)),
            face,
        );
        if r.lam_alef(
            (&lam_cps, &alef_cps),
            lam_form,
            style,
            (PairSpan::Cells(lam_at, n), &word),
            st,
        )
        .is_some()
        {
            out[i] = Some(Vec::new());
        }
    }
    // Every other joined cluster in its form, with its word.
    for (i, j) in joined.iter().enumerate() {
        let Some(j) = j.filter(|_| out[i].is_none()) else {
            continue;
        };
        let (v, cps, _, style) = pieces[i];
        let (before, after) = {
            let (b, a) = r.bidi().around(row, &j);
            (b.to_vec(), a.to_vec())
        };
        let frame = r.frame_held();
        let word = r.joined_word(
            (term, styles, frame.words()),
            cps,
            style,
            ((&before, &after), (row, Col::at_index(v))),
        );
        out[i] = Some(r.cluster_joined_at(cps, style, j.form, &word, st));
    }
    out
}

/// **The rendition a composition's clusters start from**: the cell
/// under it, as the frame's table resolved it, and its word, which a
/// hinted cluster resolves again with the attributes its hints add.
struct HintedBase {
    resolved: Resolved,
    word: StyleWord,
}

fn hinted_base(
    (term, words): (&Term<'_>, Words<'_>),
    styles: &[Resolved],
    under: PackedCell,
) -> HintedBase {
    HintedBase {
        resolved: styles.get(under.style.index()).copied().unwrap_or_default(),
        // Its word where its id names it: a cell of the frame's copy may
        // name a slot lent a frozen row (`crate::frozen`).
        word: words.word(term, Col::ZERO, under),
    }
}

impl HintedBase {
    /// The rendition a cluster drawn `look` takes: the table's own where
    /// its hints add nothing to it, else resolved afresh.
    fn of(&self, r: &Renderer<'_, '_>, term: &Term<'_>, look: Drawing) -> Resolved {
        match look.pair {
            Paired::Yes => return r.message_rendition(self.resolved),
            Paired::Text => return r.link_text_rendition(self.resolved),
            Paired::Default => return r.resolve_slot(term, StyleWord::DEFAULT),
            Paired::No => {}
        }
        let word = look.word(self.word);
        match word == self.word {
            true => self.resolved,
            false => r.resolve_slot(term, word),
        }
    }
}

/// A filled rectangle `w` by `h` at `(x, y)`.
fn bar(x: Screen, y: Screen, w: Px, h: Px, color: Argb) -> Instance {
    Instance {
        x,
        y,
        w: crate::atlas::Tex(u16::try_from(w.value()).unwrap_or(u16::MAX)),
        h: crate::atlas::Tex(u16::try_from(h.value()).unwrap_or(u16::MAX)),
        u: crate::atlas::Tex(0),
        v: crate::atlas::Tex(0),
        color,
        kind: crate::atlas::Kind::Solid,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

/// **Where one cell's line goes**, in pixels, already in its line's
/// geometry: the cell's box and the face's thickness, underline and
/// strike positions from its top.
struct RuleAt {
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    thick: i32,
    under: i32,
    strike: i32,
}

impl RuleAt {
    /// `rule`'s records: the underline and the strike in the text's colour
    /// `fg`, as `SGR 4` and `SGR 9` draw them; the curly line in `error`,
    /// as `SGR 4:3` under `SGR 58` draws it -- a zigzag one thickness in
    /// amplitude, in segments a thickness wide, clamped inside the cell.
    fn push(&self, rule: Rule, fg: Argb, error: Argb, out: &mut dyn FnMut(Instance)) {
        let px = |v: i32| Px::of(u32::try_from(v).unwrap_or(0));
        let at = |v: i32| Screen(i16::try_from(v).unwrap_or(i16::MAX));
        let th = self.thick.max(1);
        match rule {
            Rule::None => {}
            Rule::Under => out(bar(
                at(self.left),
                at(self.top + self.under),
                px(self.width),
                px(self.thick),
                fg,
            )),
            Rule::Strike => out(bar(
                at(self.left),
                at(self.top + self.strike),
                px(self.width),
                px(self.thick),
                fg,
            )),
            Rule::Curly => {
                let low = self.height - th;
                let (mut dx, mut k) = (0, 0);
                while dx < self.width {
                    let dy = if k % 2 == 0 { 0 } else { th };
                    let y = (self.under + dy).min(low.max(self.under));
                    out(bar(
                        at(self.left + dx),
                        at(self.top + y),
                        px(th),
                        px(self.thick),
                        error,
                    ));
                    dx += th;
                    k += 1;
                }
            }
        }
    }
}

/// [`composition_records`] on a line that is not single-width: each
/// cluster laid in the line's own character positions, its glyphs at the
/// doubled size from the box its character takes there, and the underline
/// stretched with the line, each on a double-height line cut to the row's
/// half, as `Renderer::convert_row` draws the line's text.
#[allow(clippy::too_many_arguments)]
fn sized_composition(
    r: &mut Renderer<'_, '_>,
    term: &Term<'_>,
    (c, laid): (&Composition, Laid),
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    size: LineRendition,
    pan: Px,
) -> Composed {
    use crate::atlas::Stretch;
    use crate::renderer::{box_top, clip_to, stretched};
    let st = Stretch::of(size);
    let line = size.end_of_line(cols);
    let m = r.metrics();
    let px = |p: Px| i32::try_from(p.value()).unwrap_or(0);
    let at = |v: i32| Screen(i16::try_from(v).unwrap_or(i16::MAX));
    let (cell_w, cell_h) = (px(m.cell_w), px(m.cell_h));
    let row_top = i32::try_from(c.row.index()).unwrap_or(0) * cell_h + px(pan);
    let band = crate::units::Band {
        top: Px::of(u32::try_from(row_top).unwrap_or(0)),
        bottom: Px::of(u32::try_from(row_top + cell_h).unwrap_or(0)),
    };
    let top = box_top(size, band);
    let (blink, cursor, hilite) = (r.blink(), opacity_of(r, block), r.field_hilite());
    let block_paint = r.block_paint();
    let frame = r.frame_held();
    let base = hinted_base((term, frame.words()), styles, under);
    let fit = |inst: Instance, out: &mut Vec<Instance>, over: &mut Vec<Instance>| match st.y() {
        1 => {
            let (a, i) = inst.cut_at(Screen(i16::try_from(row_top).unwrap_or(i16::MAX)));
            over.extend(a);
            out.extend(i);
        }
        _ => out.extend(clip_to(inst, band)),
    };
    let (mut pages, mut glyphs_out, mut rules) = (Vec::new(), Vec::new(), Vec::new());
    // Each cluster at its visual cell, with the mirror L4 gives it there,
    // as `composition_records` places it on a single-width line: the line
    // is laid out over its half with the composition written in it.
    let block = block.map(|(br, bc, s)| (br, laid.column(r.bidi(), br, bc), s));
    let placed: Vec<(Piece, Flip, Leans)> = c
        .pieces(line)
        .into_iter()
        .map(|p| {
            let orders = r.bidi();
            let v = laid.column(orders, c.row, p.col);
            let v = match p.span {
                CellSpan::Two => v.min(laid.column(orders, c.row, p.col.after())),
                CellSpan::One => v,
            };
            let mut cps = p.cps.clone();
            let (shown, flip) = c.mirror(orders, laid, v, p.cps[0]);
            let leans = c.leans(orders, laid, v, p.span);
            cps[0] = shown;
            (Piece { col: v, cps, ..p }, flip, leans)
        })
        .collect();
    let styled: Vec<(
        Drawing,
        Resolved,
        (Option<crate::stack::Lean>, crate::stack::TextFace),
    )> = placed
        .iter()
        .map(|(p, _, leans)| {
            let look = Drawing::of(p.hints);
            let one = base.of(r, term, look);
            (look, one, leans.lean(r, one.shapes.text_face()))
        })
        .collect();
    let mut joined = joined_pieces(
        r,
        (term, styles),
        (c.row, laid),
        &placed
            .iter()
            .zip(&styled)
            .map(|((p, _, _), (_, _, (_, style)))| {
                (p.col.index(), p.cps.as_slice(), p.span, *style)
            })
            .collect::<Vec<_>>(),
        st,
    );
    for (i, ((p, flip, _), (look, rendition, (lean, style)))) in
        placed.into_iter().zip(styled).enumerate()
    {
        let drawn = |x: usize, line: Line| {
            let under_cursor = block.is_some_and(|(br, bc, _)| br == c.row && bc.index() == x);
            rendition.drawn(Swaps {
                pictured: Pictured::No,
                line,
                selected: look.selected,
                hilite,
                block: if under_cursor {
                    BlockCursor::Yes(block_paint)
                } else {
                    BlockCursor::No
                },
                blink,
                cursor,
            })
        };
        let n = if p.span == CellSpan::Two { 2 } else { 1 };
        // The page and the line in a single-width cell's coordinates,
        // moved onto the line as `Renderer::convert_row` moves its shapes.
        for x in p.col.index()..p.col.index() + n {
            let d = drawn(x, Line::No);
            let left = i32::try_from(x).unwrap_or(0) * cell_w;
            if look.selected == Selected::Yes || look.pair == Paired::Default {
                pages.extend(stretched(
                    bar(at(left), at(row_top), m.cell_w, m.cell_h, d.bg),
                    size,
                    band,
                ));
            }
            if !d.text.draws() {
                continue;
            }
            let place = RuleAt {
                left,
                top: row_top,
                width: cell_w,
                height: cell_h,
                thick: px(m.thickness),
                under: px(m.underline),
                strike: px(m.strike),
            };
            place.push(look.rule, d.fg, d.rule(rendition.underline), &mut |i| {
                rules.extend(stretched(i, size, band));
            });
        }
        let d = drawn(p.col.index(), Line::of(Content::codepoint(p.cps[0])));
        if !d.text.draws() {
            continue;
        }
        let (mut glyphs, flip) = match (joined.get_mut(i).and_then(Option::take), flip) {
            (Some(g), _) => (g, Flip::Kept),
            (None, Flip::Flipped) => (r.cluster_flipped_at(&p.cps, style, p.span, st), flip),
            (None, Flip::Kept) => (r.cluster_at(&p.cps, style, p.span, st), flip),
        };
        if let Some(l) = lean {
            r.shear_glyphs(&mut glyphs, l, st, flip);
        }
        let ox = i32::try_from(p.col.index()).unwrap_or(0) * cell_w * st.x();
        let oy = top + px(m.baseline) * st.y();
        let (mut inside, mut over) = (Vec::new(), Vec::new());
        for g in glyphs {
            let Some(slot) = g.slot else {
                continue;
            };
            let inst = Instance {
                x: Screen(i16::try_from(ox + g.dx + slot.left).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(oy - g.dy + slot.top).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color: d.fg,
                kind: slot.kind,
                curve: d.curve,
            };
            fit(inst, &mut inside, &mut over);
        }
        glyphs_out.push((p.col.index(), inside, over));
    }
    glyphs_out.sort_by_key(|&(x, _, _)| x);
    Composed {
        pages,
        glyphs: glyphs_out,
        rules,
        inks: Vec::new(),
    }
}

#[cfg(test)]
mod columns_tests {
    use super::*;

    fn at(col: Col, text: &str, measure: Measure) -> Composition {
        Composition {
            row: Row::ZERO,
            col,
            text: text.chars().map(Codepoint::from_char).collect(),
            measure,
            hints: Vec::new(),
            caret: Caret::Hidden,
        }
    }

    /// **`columns` counts printable ASCII as its clusters would lay it**, under
    /// every [`Measure`], for text running past the edge and a start past it,
    /// and the characters either side of the range go by their clusters.
    /// Perturbations: drop `.min(room)`; take `start + self.text.len()` with no
    /// `saturating_sub`; widen the range to `0x20..=0x7f`, which `DEL`, a control
    /// that takes no column, fails; narrow it to `0x21..0x7f`, which the literal
    /// cases still pass, since the clusters agree, so it costs only the path.
    #[test]
    fn printable_ascii_covers_what_its_clusters_cover() {
        let every: String = (0x20u8..0x7f).map(char::from).collect();
        let texts = [
            "",
            " ",
            "a",
            "  two  ",
            "https://a.example/x",
            every.as_str(),
            "a\u{7f}",
            "a\u{1f}",
            "\u{7f}",
            "a\u{e9}",
            "\u{4e16}a",
        ];
        for ambiguous in [AmbiguousWidth::Narrow, AmbiguousWidth::Wide] {
            for widths in [UnicodeWidths::Eight, UnicodeWidths::Current] {
                for vs16 in [Vs16Width::Two, Vs16Width::One] {
                    for cells in [
                        ClusterCells::Legacy,
                        ClusterCells::LegacyMarksJoined,
                        ClusterCells::Extended,
                    ] {
                        let m = Measure {
                            ambiguous,
                            widths,
                            vs16,
                            cells,
                        };
                        for text in texts {
                            for cols in [0usize, 1, 5, 19, 20, 40, 200] {
                                for col in [0, 1, cols.saturating_sub(1), cols, cols + 1, cols + 5]
                                {
                                    let c = at(Col::at_index(col), text, m);
                                    let n = Cols::of_index(cols);
                                    assert_eq!(
                                        c.columns(n),
                                        c.columns_of_pieces(n),
                                        "{text:?} at {col} of {cols}, {m:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        // The same answers as numbers, so both ways cannot be wrong together.
        let m = Measure::default();
        let of = |col, text, cols| at(Col::at_index(col), text, m).columns(Cols::of_index(cols));
        assert_eq!(of(3, "ab", 10), (3, 5), "inside");
        assert_eq!(of(3, "abcdef", 5), (3, 5), "past the edge");
        assert_eq!(of(5, "abc", 5), (5, 5), "starting at the edge");
        assert_eq!(of(9, "abc", 5), (9, 9), "starting past it");
        assert_eq!(of(2, "", 5), (2, 2), "nothing");
        assert_eq!(
            of(0, "\u{4e16}", 10),
            (0, 2),
            "the control: a wide one's two"
        );
    }
}

#[cfg(test)]
mod covered_tests {
    use super::*;

    /// **A hovered field is read as empty and its neighbours are not**: the
    /// walk draws no record for a cell `Covered` holds, so one under the
    /// span of `hover = "text"` -- a field with no page of its own -- is
    /// dropped, and the cells beside it, and the same columns on another
    /// row, are kept. With nothing hovered and no composition it holds no
    /// cell. Perturbation: leave the hovered spans out of `holds`, and the
    /// cell under the span is kept.
    #[test]
    fn a_cell_under_a_hovered_field_is_empty_and_one_beside_is_kept() {
        let orders = crate::bidi::Orders::default();
        let none = Covered::of(
            None,
            Cols::of_index(80),
            &orders,
            &Taken::default(),
            Vec::new(),
        );
        assert!(!none.on(Row::ZERO) && !none.holds(Row::ZERO, Col::at_index(6)));
        let row = Row::from_index(2);
        let span = (row, Col::at_index(4), Cols::of_index(20));
        let c = Covered::of(
            None,
            Cols::of_index(80),
            &orders,
            &Taken::default(),
            vec![span],
        );
        assert!(c.on(row), "the field's row");
        assert!(!c.on(Row::ZERO), "another row");
        for x in 4..24 {
            assert!(c.holds(row, Col::at_index(x)), "under the field at {x}");
            assert!(!c.composes(row, Col::at_index(x)), "and no composition's");
        }
        for x in [3, 24] {
            assert!(!c.holds(row, Col::at_index(x)), "beside the field at {x}");
        }
        assert!(
            !c.holds(Row::ZERO, Col::at_index(6)),
            "the columns on another row"
        );
    }
}
