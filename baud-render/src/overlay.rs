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
use crate::cellmap::{Candidate, GlyphMap, Resolved};
use crate::instance::{Instance, Screen};
use crate::metrics::Metrics;
use crate::picture::{Pictured, Pictures};
use crate::renderer::{ClusterGlyph, Renderer};
use crate::select::Highlight;
use crate::units::Tally;
use baud_core::Term;
use baud_domain::{Chan, Codepoint, Col, Cols, Content, CursorStyle, PackedCell, Row, Viewport};
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
}

/// What colours a record: the resolved rendition table, the selection and
/// the block cursor, each of which swaps its cells' colours.
struct Paint<'p> {
    styles: &'p [Resolved],
    block: Option<(Row, Col, CursorStyle)>,
    highlight: Option<Highlight>,
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
        let mut h = self.0;
        for b in c.value().to_le_bytes() {
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
        // Twice at most: a cluster resolved on this walk can grow the
        // atlas, which moves every slot resolved before it.
        for _ in 0..2 {
            if r.atlas_stats().grown.value() != self.grown.value() {
                self.grown = r.atlas_stats().grown;
                self.clusters.clear();
                self.windows.clear();
            }
            self.walk(scene, r, mask, records);
            if r.atlas_stats().grown.value() == self.grown.value() {
                break;
            }
        }
        self.stats.records = Tally::of(records.len());
        self.stats.clusters = Tally::of(self.clusters.len());
        // Last, over the mask the walk just filled: the picture bit rides
        // beside the record bit, and the walk's own tests read the record
        // bit alone.
        let view = &scene.view;
        let (cols, rows) = (view.cols(), view.rows());
        self.stats.pictured = if r.theme().finds_pictures() {
            self.pictures.find(scene.term, view)
        } else {
            self.pictures.clear(cols, rows);
            Tally::default()
        };
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
        };
        let block = cursor
            .filter(|(_, _, s)| matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock));
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
                    mask,
                    records,
                );
            }
        }
        // The marked cells the windows did not cover.
        for y in 0..rows {
            let row = view.row(Row::from_index(y));
            for (x, cell) in row.iter().enumerate().take(cols) {
                if !cell.flags.is_marked() || mask[y * cols + x] & MASK_RECORD != 0 {
                    continue;
                }
                let Some(cp) = cell.content.as_codepoint() else {
                    continue;
                };
                let mut cps = vec![cp];
                let mut key = ClusterKey::start().fold(cp);
                for mk in term.cluster_marks(*cell) {
                    if let Some(c) = Codepoint::new(mk) {
                        cps.push(c);
                        key = key.fold(c);
                    }
                }
                if cell.flags.is_wide() {
                    key = key.wide();
                }
                let glyphs = match self.clusters.get(&key) {
                    Some(g) => g.clone(),
                    None => {
                        self.stats.misses.bump();
                        let g = r.cluster(&cps);
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
                    },
                    &geom,
                );
            }
        }
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
        mask: &mut [u8],
        records: &mut Vec<Instance>,
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
            let mut key = ClusterKey::start();
            for (_, cell, cps) in piece.iter() {
                for c in cps {
                    key = key.fold(*c);
                }
                if cell.flags.is_wide() {
                    key = key.wide();
                }
                key = key.next_cell();
            }
            let paint = Paint {
                styles: scene.styles,
                block,
                highlight: scene.highlight,
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
                let cells: Vec<Vec<Codepoint>> =
                    piece.iter().map(|(_, _, cps)| cps.clone()).collect();
                let shaped = r.window(face, &cells);
                let drawn: Vec<Option<Vec<ClusterGlyph>>> = piece
                    .iter()
                    .zip(shaped)
                    .map(|((_, cell, cps), glyphs)| {
                        let alone = scene.map.probe(cps[0]).map(|e| e.glyph());
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
            let entry = cell.content.as_codepoint().and_then(|cp| {
                scene
                    .map
                    .probe(cp)
                    .map(|e| (cp, e.face(), e.starts_pair() || e.ends_pair()))
            });
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
            let mut cps = vec![cp];
            for mk in scene.term.cluster_marks(*cell) {
                if let Some(c) = Codepoint::new(mk) {
                    cps.push(c);
                }
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
        let glyphs = r.cluster(&[cp]);
        // A prediction is a local echo at the live cursor; the ring rotation
        // that starts a slide drops any pending prediction first, so a
        // predicted record never coexists with a non-zero pan.
        let geom = Geom {
            m: r.metrics(),
            cols,
            pan: Px::of(0),
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
            },
            &geom,
        );
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
    /// colour the cell.
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
        // The selection swaps, then the cursor swaps again: both is
        // neither. **Opaque either way**: `resolved.bg` carries the
        // window's dim, and a mark or a ligature drawn at 0.22 over its
        // own selection block is barely there -- the same rule
        // `Renderer::convert_row` and the cell shader's `colors()` apply
        // to a swapped foreground, and the pixel gate compares the two
        // arms at a translucent alpha to say all three agree.
        let color = if selected != under_cursor {
            resolved.bg.with_alpha(Chan::from_u8(0xFF))
        } else {
            resolved.fg
        };
        let cell_w = i32::try_from(geom.m.cell_w.value()).unwrap_or(0);
        let cell_h = i32::try_from(geom.m.cell_h.value()).unwrap_or(0);
        let baseline = i32::try_from(geom.m.baseline.value()).unwrap_or(0);
        let ox = i32::try_from(x).unwrap_or(0) * cell_w;
        let oy = i32::try_from(y).unwrap_or(0) * cell_h
            + baseline
            + i32::try_from(geom.pan.value()).unwrap_or(0);
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
            });
        }
    }
}
