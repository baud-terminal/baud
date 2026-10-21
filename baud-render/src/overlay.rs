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

use crate::atlas::Code;
use crate::cellmap::CellSpan;
use crate::cellmap::{
    BlockCursor, Candidate, GlyphMap, Hilite, Line, Resolved, Selected, Swaps, Text,
};
use crate::instance::{Instance, Screen};
use crate::metrics::Metrics;
use crate::picture::{Pictured, Pictures};
use crate::renderer::{BlinkPhases, Blinking, ClusterGlyph, Renderer};
use crate::select::Highlight;
use crate::units::Tally;
use baud_core::Term;
use baud_domain::{
    AmbiguousWidth, Chan, Classified, Codepoint, Col, Cols, Content, CursorStyle, LineRendition,
    PackedCell, Row, Rows, Viewport, Width,
};
use baud_font::Px;
use std::collections::HashMap;

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

/// What one frame's walk reads: the core for the marks, the view it is
/// drawing, the resolved rendition table the shader reads, and the cursor.
pub struct Scene<'s, 't> {
    pub term: &'s Term<'t>,
    pub view: Viewport<'s>,
    pub styles: &'s [Resolved],
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

/// The cluster map and the walk that fills the overlay from it.
pub struct Overlay {
    clusters: HashMap<ClusterKey, Vec<ClusterGlyph>>,
    /// A window's content to what each of its cells draws: `None` where
    /// the map already draws it, the glyphs where it does not.
    windows: HashMap<ClusterKey, Vec<Option<Vec<ClusterGlyph>>>>,
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
    /// them: kept, so a frame reuses the buffers.
    sized: Vec<(Row, Vec<Instance>)>,
}

/// One cell the walk drew by records: where it is, its records' places in
/// the frame's records and in the parts above their rows, and the box
/// around their ink.
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
#[derive(Clone, Copy)]
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

/// The cells a composition covers on the view, `row` and `start..end`, or
/// none: what the walk reads as empty.
#[derive(Clone, Copy)]
struct Covered(Option<(usize, usize, usize)>);

impl Covered {
    fn of(c: Option<&Composition>, cols: Cols) -> Covered {
        Covered(c.map(|c| {
            let (start, end) = c.columns(cols);
            (c.row.index(), start, end)
        }))
    }

    fn holds(self, y: Row, x: Col) -> bool {
        self.0
            .is_some_and(|(row, start, end)| y.index() == row && (start..end).contains(&x.index()))
    }
}

/// What colours a record: the resolved rendition table, the selection and
/// the block cursor, each of which swaps its cells' colours.
struct Paint<'p> {
    styles: &'p [Resolved],
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
            clusters: HashMap::new(),
            windows: HashMap::new(),
            grown: Tally::default(),
            stats: OverlayStats::default(),
            pictures: Pictures::new(),
            over: Vec::new(),
            under: Tally::default(),
            emitted: Vec::new(),
            staged: Vec::new(),
            sized: Vec::new(),
        }
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
        // The pictures first, from the term, the view and the video alone:
        // a record in one is coloured as the cell under it is.
        let view = &scene.view;
        let (cols, rows) = (view.cols(), view.rows());
        self.stats.pictured = if r.theme().finds_pictures() {
            self.pictures.find(scene.term, view, r.video())
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
                self.windows.clear();
            }
            self.walk(scene, r, mask, records, &pictures);
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
        r: &mut Renderer<'_, '_>,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        pictures: &Pictures,
    ) {
        let (term, view, styles, cursor) = (scene.term, scene.view, scene.styles, scene.cursor);
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
        let block = cursor
            .filter(|(_, _, s)| matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock));
        let covered = Covered::of(
            scene.composition,
            scene.composition.map_or(view.cols(), |c| c.width_on(&view)),
        );
        if self.windows.len() > WINDOWS_MAX {
            self.windows.clear();
        }
        // **The lines that are not single-width**, drawn whole by the records
        // the instance path converts them to -- glyphs at their doubled size,
        // decorations and the cursor through the line's geometry -- with
        // every cell masked and marked sized, so the shader draws their
        // backgrounds from half the column and nothing else of them.
        let mut sized = core::mem::take(&mut self.sized);
        r.sized_lines(term, &view, cursor, scene.composition, &mut sized);
        let (cell_h, pan) = (
            i32::try_from(geom.m.cell_h.value()).unwrap_or(0),
            i32::try_from(geom.pan.value()).unwrap_or(0),
        );
        for (row, insts) in &sized {
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
                (*row, Col::ZERO),
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
                let ext = r
                    .extent(face)
                    .and_then(|e| usize::try_from(e.value()).ok())
                    .unwrap_or(1)
                    .max(1);
                let lo = c.saturating_sub(ext);
                let hi = (c + ext - 1).min(cols.saturating_sub(1));
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
                    covered,
                    mask,
                    records,
                    pictures,
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
        };
        for y in 0..rows {
            let row = view.row(Row::from_index(y));
            for (x, cell) in row.iter().enumerate().take(cols) {
                if blinks {
                    if let Some(s) = styles.get(cell.style.index()) {
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
        for &(y, x) in scene.map.several() {
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
            self.reach(scene, r, mask, records, &paint, &geom, covered);
            self.order(records);
        }
        self.under = Tally::of(records.len());
        records.append(&mut self.over);
        r.set_blink_seen(seen);
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
        covered: Covered,
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
                    let style = crate::cellmap::face_of(scene.styles, *cell);
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
        let (term, styles) = (scene.term, scene.styles);
        let row = scene.view.row(Row::from_index(y));
        let cell = row[x];
        let Some(cp) = crate::renderer::drawn_base(term, cell) else {
            return;
        };
        let mut cps = vec![cp];
        let style = crate::cellmap::face_of(styles, cell);
        let mut key = ClusterKey::start().word(style.code()).fold(cp);
        for mk in term.cluster_marks(cell) {
            if let Some(c) = Codepoint::new(mk) {
                cps.push(c);
                key = key.fold(c);
            }
        }
        let span = CellSpan::at(row, Col::at_index(x));
        if span == CellSpan::Two {
            key = key.wide();
        }
        let glyphs = match self.clusters.get(&key) {
            Some(g) => g.clone(),
            None => {
                self.stats.misses.bump();
                let g = r.cluster(&cps, style, span);
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
        covered: Covered,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
        pictures: &Pictures,
    ) {
        let row = scene.view.row(y);
        let mut piece: Vec<(usize, PackedCell, Vec<Codepoint>)> = Vec::new();
        let flush = |piece: &mut Vec<(usize, PackedCell, Vec<Codepoint>)>,
                     face: Code,
                     this: &mut Overlay,
                     r: &mut Renderer<'_, '_>,
                     mask: &mut [u8],
                     records: &mut Vec<Instance>| {
            if piece.len() < 2 {
                piece.clear();
                return;
            }
            this.stats.windows.bump();
            for _ in piece.iter() {
                this.stats.window_cells.bump();
            }
            let mut key = ClusterKey::start().word(face);
            for (x, _, cps) in piece.iter() {
                for c in cps {
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
                    .map(|(x, _, cps)| (cps.clone(), CellSpan::at(row, Col::at_index(*x))))
                    .collect();
                let shaped = r.window(face, &cells);
                let drawn: Vec<Option<Vec<ClusterGlyph>>> = piece
                    .iter()
                    .zip(shaped)
                    .map(|((x, cell, cps), glyphs)| {
                        let style = crate::cellmap::face_of(scene.styles, *cell);
                        let alone = scene
                            .map
                            .probe(cps[0], style, CellSpan::at(row, Col::at_index(*x)))
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
                        crate::cellmap::face_of(scene.styles, *cell),
                        CellSpan::at(row, Col::at_index(x)),
                    )
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
            let mut cps: Vec<Codepoint> = Vec::new();
            if let Some((cp, face, _)) = entry {
                if cell.flags.is_marked() {
                    let style = crate::cellmap::face_of(scene.styles, *cell);
                    cps.push(cp);
                    cps.extend(scene.term.cluster_marks(*cell).filter_map(Codepoint::new));
                    if u32::try_from(r.face_for(&cps, style).index()).ok() != Some(face.0) {
                        entry = None;
                    }
                }
            }
            let cut = match entry {
                None => true,
                Some((_, _, in_rules)) => !in_rules,
            } || matches!((first, entry), (Some(f), Some((_, face, _))) if f != face);
            if cut {
                if let Some(f) = first {
                    flush(&mut piece, f, self, r, mask, records);
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
            piece.push((x, *cell, cps));
        }
        if let Some(f) = first {
            flush(&mut piece, f, self, r, mask, records);
        }
    }

    /// A predicted echo (row 9 of note N): one cell the app draws before
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
        styles: &[Resolved],
        block: Option<(Row, Col, CursorStyle)>,
        highlight: Option<Highlight>,
        cols: Cols,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let Some(cp) = cell.content.as_codepoint() else {
            return;
        };
        let (y, x) = (at.0.index(), at.1.index());
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
            crate::cellmap::face_of(styles, cell),
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
        c: &Composition,
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
            if let Some(slot) = mask.get_mut(c.row.index() * cols.index() + x) {
                *slot |= MASK_RECORD;
            }
        }
        composition_records(r, c, under, styles, block, cols, size, pan, records);
    }

    /// **The leaving row of a smooth scroll, for the cell path, when its
    /// line is not single-width**: `cells` and `size`, the line just above
    /// the window, drawn by the records the instance path converts it to
    /// ([`Renderer::sized_leaving`]) slid by `pan`, and its row of the mask
    /// -- the one after the screen's `rows`, which the shader reads for
    /// draw row -1 -- marked `RECORD` and `SIZED`, so the shader draws its
    /// backgrounds from half the column and nothing else of it, as on a
    /// sized screen row. Nothing for a single-width line, which the shader
    /// draws from the map, or where the mask has no such row.
    #[allow(clippy::too_many_arguments)]
    pub fn leaving(
        &mut self,
        r: &mut Renderer<'_, '_>,
        term: &Term<'_>,
        cells: &[PackedCell],
        size: LineRendition,
        (cols, rows): (Cols, Rows),
        pan: Px,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        if !size.is_double() {
            return;
        }
        let at = rows.index().saturating_mul(cols.index());
        let Some(row) = mask.get_mut(at..at.saturating_add(cols.index())) else {
            return;
        };
        for b in row {
            *b |= MASK_RECORD | MASK_SIZED;
        }
        let mut lifted = Vec::new();
        r.sized_leaving(term, cells, size, &mut lifted);
        let pan = i16::try_from(pan.value()).unwrap_or(0);
        records.extend(lifted.into_iter().map(|i| Instance {
            y: Screen(i.y.0.saturating_add(pan)),
            ..i
        }));
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
        let mut drawn = Vec::new();
        r.status_records(term, cursor, &mut drawn);
        records.extend(drawn);
    }

    /// A window's cells that the map does not draw: masked, counted and
    /// drawn by records.
    #[allow(clippy::too_many_arguments)]
    fn paint_window(
        piece: &[(usize, PackedCell, Vec<Codepoint>)],
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
            .get(cell.style.index())
            .copied()
            .unwrap_or_default();
        let under_cursor = paint
            .block
            .is_some_and(|(cr, cc, _)| cr.index() == y && cc.index() == x);
        let selected = paint
            .highlight
            .is_some_and(|h| h.selects(Row::from_index(y), Col::at_index(x), cell.content));
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
                BlockCursor::Yes(None)
            } else {
                BlockCursor::No
            },
            blink: paint.blink,
            cursor: paint.cursor,
        });
        if d.text == Text::Hidden {
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
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Composition {
    pub row: Row,
    pub col: Col,
    pub text: Vec<Codepoint>,
    /// How many columns an ambiguous character takes: the core's own
    /// policy (`Term::ambiguous_width`), so the overlay and a write of
    /// the same text agree. Private Use stays one column either way.
    pub ambiguous: AmbiguousWidth,
}

/// One cluster of a composition: the column it starts at, its codepoints
/// and how many cells it takes.
struct Piece {
    col: Col,
    cps: Vec<Codepoint>,
    span: CellSpan,
}

impl Composition {
    /// Its clusters laid on a screen `cols` wide from its cell, by the
    /// width tables the core writes by: a character of no width joins the
    /// cluster before it, a wide one takes two cells, and one that would
    /// cross the right edge is not drawn, nor anything after it -- a
    /// composition is shown on its line, as `NSTextView` shows marked text
    /// at the insertion point. Ambiguous characters take `self.ambiguous`.
    fn pieces(&self, cols: Cols) -> Vec<Piece> {
        let mut out: Vec<Piece> = Vec::new();
        let mut x = self.col.index();
        for &cp in &self.text {
            let w = Classified::of(cp).width(self.ambiguous);
            if w == Width::ZERO {
                if let Some(last) = out.last_mut() {
                    last.cps.push(cp);
                }
                continue;
            }
            let span = if w == Width::WIDE {
                CellSpan::Two
            } else {
                CellSpan::One
            };
            let n = if span == CellSpan::Two { 2 } else { 1 };
            if x + n > cols.index() {
                break;
            }
            out.push(Piece {
                col: Col::at_index(x),
                cps: vec![cp],
                span,
            });
            x += n;
        }
        out
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

    /// The columns it covers, `start..end`.
    pub fn columns(&self, cols: Cols) -> (usize, usize) {
        let start = self.col.index();
        let end = self.pieces(cols).last().map_or(start, |p| {
            p.col.index() + if p.span == CellSpan::Two { 2 } else { 1 }
        });
        (start, end)
    }

    /// Where the cursor is drawn while it is held: the cell after its last,
    /// or the last column when it runs to the edge, where a write of the
    /// same text would leave the cursor.
    pub fn cursor(&self, cols: Cols) -> (Row, Col) {
        let (_, end) = self.columns(cols);
        (
            self.row,
            Col::at_index(end.min(cols.index().saturating_sub(1))),
        )
    }
}

/// The records that draw a composition: each cluster's glyphs in the
/// rendition of `under`, the cell at the composition's start, coloured as
/// `Overlay::emit` colours any record -- the block cursor's swap included --
/// and one underline across the cells, at the face's underline position and
/// thickness, in the colour the text takes. The instance and CPU paths draw
/// these after their rows; the cell path after its passes.
///
/// **On a line that is not single-width** (`size`) every glyph and the
/// underline are drawn at the box a character there takes, as the line's
/// own text is (`[[source: dec-std-070]]` § 5.6.1): rasterised at the
/// doubled size ([`Renderer::cluster_at`]), and on a double-height line
/// each row's half of it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn composition_records(
    r: &mut Renderer<'_, '_>,
    c: &Composition,
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    size: LineRendition,
    pan: Px,
    records: &mut Vec<Instance>,
) {
    if size.is_double() {
        return sized_composition(r, c, under, styles, block, cols, size, pan, records);
    }
    let geom = Geom {
        m: r.metrics(),
        cols,
        pan,
    };
    let paint = Paint {
        styles,
        block,
        highlight: None,
        blink: r.blink(),
        cursor: opacity_of(r, block),
        pictures: None,
        hilite: r.hilite(),
    };
    let face = crate::cellmap::face_of(styles, under);
    let y = c.row.index();
    for p in c.pieces(cols) {
        let glyphs = r.cluster(&p.cps, face, p.span);
        let cell = PackedCell {
            content: Content::codepoint(p.cps[0]),
            style: under.style,
            flags: baud_domain::CellFlags::NONE,
        };
        let mut over = Vec::new();
        Overlay::emit(
            records,
            &mut over,
            (p.col.index(), y),
            &glyphs,
            cell,
            &paint,
            &geom,
        );
        records.append(&mut over);
    }
    // One underline a cell, as `Renderer::convert_row` draws `SGR 4`: each
    // in its cell's colour after the swaps, so the cell under a block cursor
    // -- the last column, when the composition runs to the edge -- takes the
    // swapped one there as the written text would.
    let resolved = styles.get(under.style.index()).copied().unwrap_or_default();
    let m = geom.m;
    let cell_w = i32::try_from(m.cell_w.value()).unwrap_or(0);
    let cell_h = i32::try_from(m.cell_h.value()).unwrap_or(0);
    let top = i32::try_from(y).unwrap_or(0) * cell_h
        + i32::try_from(m.underline.value()).unwrap_or(0)
        + i32::try_from(pan.value()).unwrap_or(0);
    let (start, end) = c.columns(cols);
    for x in start..end {
        let under_cursor = block.is_some_and(|(br, bc, _)| br == c.row && bc.index() == x);
        let d = resolved.drawn(Swaps {
            pictured: Pictured::No,
            line: Line::No,
            selected: Selected::No,
            hilite: paint.hilite,
            block: if under_cursor {
                BlockCursor::Yes(None)
            } else {
                BlockCursor::No
            },
            blink: r.blink(),
            cursor: opacity_of(r, block),
        });
        if d.text == Text::Hidden {
            continue;
        }
        let left = i32::try_from(x).unwrap_or(0) * cell_w;
        records.push(Instance {
            x: Screen(i16::try_from(left).unwrap_or(i16::MAX)),
            y: Screen(i16::try_from(top).unwrap_or(i16::MAX)),
            w: crate::atlas::Tex(u16::try_from(m.cell_w.value()).unwrap_or(u16::MAX)),
            h: crate::atlas::Tex(u16::try_from(m.thickness.value()).unwrap_or(u16::MAX)),
            u: crate::atlas::Tex(0),
            v: crate::atlas::Tex(0),
            color: d.fg,
            kind: crate::atlas::Kind::Solid,
            curve: baud_domain::contrast::CurveRow::default(),
        });
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
    c: &Composition,
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    size: LineRendition,
    pan: Px,
    records: &mut Vec<Instance>,
) {
    use crate::atlas::Stretch;
    use crate::renderer::{box_top, clip_to};
    let st = Stretch::of(size);
    let line = size.end_of_line(cols);
    let m = r.metrics();
    let px = |p: Px| i32::try_from(p.value()).unwrap_or(0);
    let (cell_w, cell_h) = (px(m.cell_w), px(m.cell_h));
    let row_top = i32::try_from(c.row.index()).unwrap_or(0) * cell_h + px(pan);
    let band = crate::units::Band {
        top: Px::of(u32::try_from(row_top).unwrap_or(0)),
        bottom: Px::of(u32::try_from(row_top + cell_h).unwrap_or(0)),
    };
    let top = box_top(size, band);
    let (blink, cursor, hilite) = (r.blink(), opacity_of(r, block), r.hilite());
    let resolved = styles.get(under.style.index()).copied().unwrap_or_default();
    let drawn = |x: usize, line: Line| {
        let under_cursor = block.is_some_and(|(br, bc, _)| br == c.row && bc.index() == x);
        resolved.drawn(Swaps {
            pictured: Pictured::No,
            line,
            selected: Selected::No,
            hilite,
            block: if under_cursor {
                BlockCursor::Yes(None)
            } else {
                BlockCursor::No
            },
            blink,
            cursor,
        })
    };
    let fit = |inst: Instance, out: &mut Vec<Instance>, over: &mut Vec<Instance>| match st.y() {
        1 => {
            let (a, i) = inst.cut_at(Screen(i16::try_from(row_top).unwrap_or(i16::MAX)));
            over.extend(a);
            out.extend(i);
        }
        _ => out.extend(clip_to(inst, band)),
    };
    let face = crate::cellmap::face_of(styles, under);
    let mut over = Vec::new();
    for p in c.pieces(line) {
        let d = drawn(p.col.index(), Line::of(Content::codepoint(p.cps[0])));
        if d.text == Text::Hidden {
            continue;
        }
        let glyphs = r.cluster_at(&p.cps, face, p.span, st);
        let ox = i32::try_from(p.col.index()).unwrap_or(0) * cell_w * st.x();
        let oy = top + px(m.baseline) * st.y();
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
            fit(inst, records, &mut over);
        }
    }
    records.append(&mut over);
    let (start, end) = c.columns(line);
    let wide =
        u16::try_from(m.cell_w.value() * u32::try_from(st.x()).unwrap_or(1)).unwrap_or(u16::MAX);
    let thick =
        u16::try_from(m.thickness.value() * u32::try_from(st.y()).unwrap_or(1)).unwrap_or(u16::MAX);
    for x in start..end {
        let d = drawn(x, Line::No);
        if d.text == Text::Hidden {
            continue;
        }
        let left = i32::try_from(x).unwrap_or(0) * cell_w * st.x();
        let y = top + px(m.underline) * st.y();
        let inst = Instance {
            x: Screen(i16::try_from(left).unwrap_or(i16::MAX)),
            y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
            w: crate::atlas::Tex(wide),
            h: crate::atlas::Tex(thick),
            u: crate::atlas::Tex(0),
            v: crate::atlas::Tex(0),
            color: d.fg,
            kind: crate::atlas::Kind::Solid,
            curve: baud_domain::contrast::CurveRow::default(),
        };
        let mut none = Vec::new();
        fit(inst, records, &mut none);
        records.append(&mut none);
    }
}
