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

use crate::cellmap::Resolved;
use crate::instance::{Instance, Screen};
use crate::renderer::{ClusterGlyph, Renderer};
use crate::units::Tally;
use baud_core::Term;
use baud_domain::{Codepoint, Col, CursorStyle, Row, Viewport};
use std::collections::HashMap;

/// What one frame's walk found and what the store holds.
#[derive(Clone, Copy, Debug, Default)]
pub struct OverlayStats {
    /// Cells the records draw this frame.
    pub cells: Tally,
    /// Records emitted this frame.
    pub records: Tally,
    /// Clusters shaped this frame because they had not been seen.
    pub misses: Tally,
    /// Distinct clusters resolved and kept.
    pub clusters: Tally,
}

/// What one frame's walk reads: the core for the marks, the view it is
/// drawing, the resolved rendition table the shader reads, and the cursor.
pub struct Scene<'s, 't> {
    pub term: &'s Term<'t>,
    pub view: Viewport<'s>,
    pub styles: &'s [Resolved],
    pub cursor: Option<(Row, Col, CursorStyle)>,
}

/// The cluster map and the walk that fills the overlay from it.
pub struct Overlay {
    clusters: HashMap<ClusterKey, Vec<ClusterGlyph>>,
    /// The atlas generation the kept slots belong to; a grow moves them.
    grown: Tally,
    stats: OverlayStats,
}

/// A cluster's content, folded: the base, its marks and its width.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ClusterKey(u64);

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
            grown: Tally::default(),
            stats: OverlayStats::default(),
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
            }
            self.walk(scene, r, mask, records);
            if r.atlas_stats().grown.value() == self.grown.value() {
                break;
            }
        }
        self.stats.records = Tally::of(records.len());
        self.stats.clusters = Tally::of(self.clusters.len());
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
        let cols = view.cols().index();
        let rows = view.rows().index();
        let Some(mask) = mask.get_mut(..cols.saturating_mul(rows)) else {
            return;
        };
        mask.fill(0);
        let m = r.metrics();
        let cell_w = i32::try_from(m.cell_w.value()).unwrap_or(0);
        let cell_h = i32::try_from(m.cell_h.value()).unwrap_or(0);
        let baseline = i32::try_from(m.baseline.value()).unwrap_or(0);
        let block = cursor
            .filter(|(_, _, s)| matches!(s, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock));
        for y in 0..rows {
            let row = view.row(Row::from_index(y));
            for (x, cell) in row.iter().enumerate().take(cols) {
                if !cell.flags.is_marked() {
                    continue;
                }
                let Some(cp) = cell.content.as_codepoint() else {
                    continue;
                };
                self.stats.cells.bump();
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
                mask[y * cols + x] = 1;
                let resolved = styles.get(cell.style.index()).copied().unwrap_or_default();
                let under_cursor =
                    block.is_some_and(|(cr, cc, _)| cr.index() == y && cc.index() == x);
                let color = if under_cursor {
                    resolved.bg
                } else {
                    resolved.fg
                };
                let ox = i32::try_from(x).unwrap_or(0) * cell_w;
                let oy = i32::try_from(y).unwrap_or(0) * cell_h + baseline;
                for g in glyphs {
                    records.push(Instance {
                        x: Screen(i16::try_from(ox + g.dx + g.slot.left).unwrap_or(i16::MAX)),
                        y: Screen(i16::try_from(oy - g.dy + g.slot.top).unwrap_or(i16::MAX)),
                        w: g.slot.w,
                        h: g.slot.h,
                        u: g.slot.x,
                        v: g.slot.y,
                        color,
                        kind: g.slot.kind,
                    });
                }
            }
        }
    }
}
