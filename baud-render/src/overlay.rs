//! The cells the cell shader must not draw from its map, and what draws
//! them instead.
//!
//! The cell path draws one glyph a cell from a map keyed on the codepoint,
//! and a cell whose rendering is not a function of its codepoint alone --
//! today, a cell with attached codepoints: a family, a base with a mark --
//! is drawn wrong by it. This is the exception structure beside the grid:
//! one byte a screen cell, zero for "the map is right", one for "the
//! records draw this cell", and the records, the same twenty-byte
//! [`Instance`] the instance path emits, drawn by one more instanced draw
//! after the cell passes. The shaded glyphs a cluster resolves to are a
//! function of the cluster's content, so they are resolved once per
//! distinct cluster and kept -- the glyph map's shape, one level up.
//!
//! Nothing here is per frame except the walk, which the glyph map already
//! makes over every cell: a byte test a cell, a lookup a marked cell.

use crate::atlas::Code;
use crate::cellmap::CellSpan;
use crate::cellmap::{BlockCursor, Candidate, GlyphMap, Line, Resolved, Selected, Swaps, Text};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use crate::metrics::Metrics;
use crate::picture::{Pictured, Pictures};
use crate::renderer::{BlinkPhases, Blinking, ClusterGlyph, Renderer};
use crate::select::Highlight;
use crate::units::Tally;
use baud_core::Term;
use baud_domain::{
    AmbiguousWidth, Chan, Classified, Codepoint, Col, Cols, Content, CursorStyle, PackedCell, Row,
    Viewport, Width,
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
/// `CURSOR`: a record draws the bar or underline cursor over this cell, after
/// the glyph's, and the shape pass must not draw it under them.
pub const MASK_CURSOR: u8 = 4;
/// `BARE`: the cell holds a cluster that takes no stress mark (`SGR 64`),
/// one whose base the web leaves bare ([`crate::takes_stress_mark`]); the
/// cell shader sees only the cluster's slot, not its base, and reads this.
/// Set for every such cell on screen, whether or not a record draws it.
/// `MASK_BARE` in the cell shader, pinned by
/// `the_bare_mask_bit_matches_the_shader`.
pub const MASK_BARE: u8 = 16;

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
    /// The bar or underline cursor, when one is drawn this frame: over a
    /// cell the records draw, its shape is a record after the glyph's, as
    /// the instance path draws it after the row's glyphs, and the shader's
    /// shape pass leaves it out there.
    bar: Option<BarCursor>,
}

/// A bar or underline cursor as a record draws it: where, which shape, the
/// `OSC 12` colour if one is set, and how much of it a blink draws.
#[derive(Clone, Copy)]
struct BarCursor {
    row: Row,
    col: Col,
    style: CursorStyle,
    color: Option<Argb>,
    opacity: Chan,
}

impl BarCursor {
    /// The frame's bar or underline cursor, from the cursor it draws.
    fn of(
        r: &Renderer<'_, '_>,
        term: &Term<'_>,
        cursor: Option<(Row, Col, CursorStyle)>,
    ) -> Option<BarCursor> {
        let (row, col, style) = cursor?;
        let bar = matches!(
            style,
            CursorStyle::BlinkingBar
                | CursorStyle::SteadyBar
                | CursorStyle::BlinkingUnderline
                | CursorStyle::SteadyUnderline
        );
        bar.then(|| BarCursor {
            row,
            col,
            style,
            color: term
                .cursor_color_in(r.video())
                .map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))),
            opacity: r.cursor_opacity(style),
        })
    }

    /// Its solid over cell `(x, y)` whose glyph is drawn in `fg`, or none
    /// when it is not there: `Renderer::cursor_px`'s rectangle and the
    /// shader's `which == 3` colour, `OSC 12` or the foreground, scaled.
    fn over(self, (x, y): (usize, usize), fg: Argb, geom: &Geom) -> Option<Instance> {
        if self.row.index() != y || self.col.index() != x {
            return None;
        }
        let m = geom.m;
        let (cw, ch) = (m.cell_w.value(), m.cell_h.value());
        let line = m.thickness.value().max(2);
        let (dy, w, h) = match self.style {
            CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline => {
                (ch.saturating_sub(line), cw, line)
            }
            _ => (0, line, ch),
        };
        let at = |v: u64| Screen(i16::try_from(v).unwrap_or(i16::MAX));
        let len = |v: u32| crate::atlas::Tex(u16::try_from(v).unwrap_or(u16::MAX));
        let left = u64::try_from(x).unwrap_or(0) * u64::from(cw);
        let top = u64::try_from(y).unwrap_or(0) * u64::from(ch)
            + u64::from(dy)
            + u64::from(geom.pan.value());
        Some(Instance {
            x: at(left),
            y: at(top),
            w: len(w),
            h: len(h),
            u: crate::atlas::Tex(0),
            v: crate::atlas::Tex(0),
            color: self.color.unwrap_or(fg).scaled(self.opacity),
            kind: crate::atlas::Kind::Solid,
            curve: baud_domain::contrast::CurveRow::default(),
        })
    }
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
        }
    }

    pub fn stats(&self) -> OverlayStats {
        self.stats
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
        records.clear();
        self.stats.cells = Tally::default();
        self.stats.misses = Tally::default();
        self.stats.windows = Tally::default();
        self.stats.window_misses = Tally::default();
        self.stats.window_cells = Tally::default();
        let cols = view.cols().index();
        let rows = view.rows().index();
        let Some(mask) = mask.get_mut(..cols.saturating_mul(rows)) else {
            return;
        };
        mask.fill(0);
        let geom = Geom {
            m: r.metrics(),
            cols: view.cols(),
            pan: scene.pan,
            bar: BarCursor::of(r, scene.term, cursor),
        };
        let block = cursor
            .filter(|(_, _, s)| matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock));
        let covered = Covered::of(scene.composition, view.cols());
        if self.windows.len() > WINDOWS_MAX {
            self.windows.clear();
        }
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
                let Some(cp) = crate::renderer::drawn_base(term, *cell) else {
                    continue;
                };
                let mut cps = vec![cp];
                let style = crate::cellmap::face_of(styles, *cell);
                let mut key = ClusterKey::start().word(style.code()).fold(cp);
                for mk in term.cluster_marks(*cell) {
                    if let Some(c) = Codepoint::new(mk) {
                        cps.push(c);
                        key = key.fold(c);
                    }
                }
                if crate::cellmap::CellSpan::at(row, Col::at_index(x))
                    == crate::cellmap::CellSpan::Two
                {
                    key = key.wide();
                }
                let glyphs = match self.clusters.get(&key) {
                    Some(g) => g.clone(),
                    None => {
                        self.stats.misses.bump();
                        let g = r.cluster(&cps, style, CellSpan::at(row, Col::at_index(x)));
                        self.clusters.insert(key, g.clone());
                        g
                    }
                };
                self.stats.cells.bump();
                mask[y * cols + x] |= MASK_RECORD;
                Overlay::emit(
                    records,
                    (x, y),
                    &glyphs,
                    *cell,
                    &Paint {
                        styles,
                        block,
                        highlight: scene.highlight,
                        blink: r.blink(),
                        cursor: opacity_of(r, block),
                        pictures: Some(pictures),
                    },
                    &geom,
                );
            }
        }
        // The cursor's shape over a cell the records draw is theirs.
        if let Some(m) = geom
            .bar
            .and_then(|b| mask.get_mut(b.row.index() * cols + b.col.index()))
        {
            if *m & MASK_RECORD != 0 {
                *m |= MASK_CURSOR;
            }
        }
        r.set_blink_seen(seen);
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
            bar: None,
        };
        Overlay::emit(
            records,
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
            },
            &geom,
        );
    }

    /// An input method's composition, for the cell path: the cells it covers
    /// masked off the shader, and its records -- glyphs and the underline
    /// -- appended, the same records the instance and CPU paths draw
    /// ([`composition_records`]). `block` is the cursor the frame draws,
    /// which [`Composition::cursor`] put after it.
    #[allow(clippy::too_many_arguments)]
    pub fn composed(
        &mut self,
        r: &mut Renderer<'_, '_>,
        c: &Composition,
        under: PackedCell,
        styles: &[Resolved],
        block: Option<(Row, Col, CursorStyle)>,
        cols: Cols,
        pan: Px,
        mask: &mut [u8],
        records: &mut Vec<Instance>,
    ) {
        let (start, end) = c.columns(cols);
        for x in start..end {
            if let Some(slot) = mask.get_mut(c.row.index() * cols.index() + x) {
                *slot |= MASK_RECORD;
            }
        }
        composition_records(r, c, under, styles, block, cols, pan, records);
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
    ) {
        for ((x, cell, _), glyphs) in piece.iter().zip(drawn) {
            if let Some(glyphs) = glyphs {
                stats.cells.bump();
                mask[y.index() * geom.cols.index() + x] |= MASK_RECORD;
                Overlay::emit(records, (*x, y.index()), glyphs, *cell, paint, geom);
            }
        }
    }

    /// The records for one cell's glyphs, coloured as the shader would
    /// colour the cell, and bent by the contrast curve that colour takes.
    #[allow(clippy::too_many_arguments)]
    fn emit(
        records: &mut Vec<Instance>,
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
            block: if under_cursor {
                BlockCursor::Yes(None)
            } else {
                BlockCursor::No
            },
            blink: paint.blink,
            cursor: paint.cursor,
        });
        let bar = geom.bar.and_then(|b| b.over(at, d.cursor_fg, geom));
        if d.text == Text::Hidden {
            records.extend(bar);
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
        for g in glyphs {
            let Some(slot) = g.slot else {
                continue;
            };
            records.push(Instance {
                x: Screen(i16::try_from(ox + g.dx + slot.left).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(oy - g.dy + slot.top).unwrap_or(i16::MAX)),
                w: slot.w,
                h: slot.h,
                u: slot.x,
                v: slot.y,
                color,
                kind: slot.kind,
                curve,
            });
        }
        records.extend(bar);
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
#[allow(clippy::too_many_arguments)]
pub(crate) fn composition_records(
    r: &mut Renderer<'_, '_>,
    c: &Composition,
    under: PackedCell,
    styles: &[Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    cols: Cols,
    pan: Px,
    records: &mut Vec<Instance>,
) {
    let geom = Geom {
        m: r.metrics(),
        cols,
        pan,
        bar: None,
    };
    let paint = Paint {
        styles,
        block,
        highlight: None,
        blink: r.blink(),
        cursor: opacity_of(r, block),
        pictures: None,
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
        Overlay::emit(records, (p.col.index(), y), &glyphs, cell, &paint, &geom);
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
