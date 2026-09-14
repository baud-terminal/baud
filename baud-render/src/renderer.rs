//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Kind, Tex};
use crate::cache::{CacheStats, CellIx, Key, Placed, RUN_CELLS, RunCache, Shaped};
use crate::cellmap::{Resolved, Shapes};
use crate::color::{Argb, Theme};
use crate::composite::{Blend, Target};
use crate::instance::{Instance, Screen};
use crate::metrics::{Metrics, PxSize};
use crate::stack::{FaceIx, Stack};
use crate::units::{Band, Digest, Tally};
use baud_core::Term;
use baud_domain::{Attrs, Chan, Codepoint, Col, Cols, CursorStyle, GlyphId, PackedCell, Row, Rows};
use baud_font::{
    Cluster, Digest as LookupDigest, Face, Features, Glyph, Ignorable, MAX_LOOKUPS, Px, Scale,
    Shaper, Walked,
};
use std::time::{Duration, Instant};

/// Whether to trust the previous frame or repaint everything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Damage {
    /// Repaint the rows that differ from the last frame.
    Incremental,
    /// Repaint every row.
    Full,
}

/// The blink phase of a frame -- one half of the cycle a driver toggles off
/// its display link, the hardware vsync clock. A cell with `Attrs::BLINK`
/// draws its foreground on [`Blink::Shown`] and hides it (foreground set to
/// its own background) on [`Blink::Hidden`]. Blink is a *frame* property, not
/// part of the interned rendition, so it is resolved each frame here rather
/// than stored in a cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blink {
    #[default]
    Shown,
    Hidden,
}

/// Whether the last convert or resolve saw a cell with `Attrs::BLINK`, so a
/// driver repaints on a phase flip only when something blinks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Blinking {
    #[default]
    Unseen,
    Seen,
}

/// What [`Renderer::convert`] did, handed to [`Renderer::paint`] or to a
/// GPU path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Converted {
    pub rows_dirty: Tally,
    pub rows: Tally,
    pub convert: Duration,
    pub exceptions: Exceptions,
}

/// What the converted rows held that one glyph a cell cannot draw, and
/// what finding it would cost a path that shapes only where it must. A
/// count, taken on the path that shapes every run, of the cells the cell
/// shader gets wrong and of the cells a per-face prefilter would send to
/// the shaper to find them:
///
/// * `differs`: cells whose shaped glyphs are not what the cell shader's
///   map gives for the codepoint alone -- a different glyph, a different
///   offset, more than one glyph or none. This is the cells the shader
///   draws wrong, and it is counted only under the `exceptions` feature,
///   because it keeps a memo of the single-codepoint answer per codepoint.
/// * `spanning`: cells covered by a glyph that spans more than its own
///   cluster -- a ligature drawn as one wide glyph.
/// * `marked`: cells carrying attached codepoints in the cluster store.
/// * `starters`: cells whose glyph some `GSUB` lookup the grid features
///   name can start on -- the face's own first-position coverage, so a
///   font with unusual lookups is covered by construction.
/// * `shaped`: cells of the runs holding at least one starter, which is
///   what a prefiltered path would shape each frame.
/// * `morx`: cells on a face with a `morx` table and no `GSUB`, which
///   has no coverage to prefilter on.
#[derive(Clone, Copy, Debug, Default)]
pub struct Exceptions {
    pub differs: Tally,
    pub spanning: Tally,
    pub marked: Tally,
    pub starters: Tally,
    pub shaped: Tally,
    pub morx: Tally,
    /// Cells of the runs holding an adjacent glyph pair the face's rules
    /// can begin on -- what a pair-prefiltered path would shape a frame.
    pub bigram: Tally,
}

/// The glyph pairs a face's grid features can begin a substitution on:
/// for a first glyph, a digest of what may follow it, derived from the
/// tables by `Gsub::pairs`. `admits` says whether two adjacent glyphs could
/// start a match; a run with no admitted pair shapes to what its cells
/// shape to alone.
#[derive(Clone, Default)]
pub struct Pairs {
    after: std::collections::HashMap<usize, LookupDigest>,
    walked: Option<Walked>,
}

impl Pairs {
    fn of(face: &Face<'_>) -> Pairs {
        let mut after: std::collections::HashMap<usize, LookupDigest> =
            std::collections::HashMap::new();
        let walked = face.gsub().map(|gsub| {
            gsub.pairs(Features::GRID, &mut |first, d| {
                let slot = after.entry(first.index()).or_insert(LookupDigest::EMPTY);
                *slot = slot.union(d);
            })
        });
        Pairs { after, walked }
    }

    pub fn admits(&self, a: GlyphId, b: GlyphId) -> bool {
        match self.walked {
            None => false,
            Some(Walked::Unreadable) => true,
            Some(Walked::Derived) => self
                .after
                .get(&a.index())
                .is_some_and(|d| d.may_have(LookupDigest::of(b))),
        }
    }

    /// How many first glyphs the tables name.
    pub fn firsts(&self) -> Tally {
        Tally::of(self.after.len())
    }
}

/// How a face's substitutions can be prefiltered.
#[derive(Clone, Copy)]
enum Starter {
    /// The union of the face's lookup digests: a glyph outside it starts
    /// no substitution.
    Gsub(LookupDigest),
    /// A `morx` face: no coverage to ask.
    Morx,
    /// No substitution table at all.
    Plain,
}

/// One glyph of a shaped cluster: its atlas slot and the offsets, in
/// pixels, the shaper placed it at relative to the cell's origin.
#[derive(Clone, Copy, Debug)]
pub struct ClusterGlyph {
    pub slot: crate::atlas::Slot,
    pub dx: i32,
    pub dy: i32,
}

/// What one frame cost and did.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub rows_dirty: Tally,
    pub rows: Tally,
    pub instances: Tally,
    /// Of the instances, those on the colour texture and those solid.
    pub colored: Tally,
    pub solids: Tally,
    pub cache: CacheStats,
    pub atlas: AtlasStats,
    pub convert: Duration,
    pub composite: Duration,
    pub exceptions: Exceptions,
}

/// An FNV-1a fold over the 256 palette entries the core carries.
fn palette_digest(term: &Term<'_>) -> Digest {
    let mut d = Digest::start();
    for i in 0..=u8::MAX {
        let (r, g, b) = term.palette(Chan::from_u8(i)).unwrap_or((0, 0, 0));
        d = d.fold_bytes(&[r, g, b, 1]);
    }
    d
}

fn px_i32(p: Px) -> i32 {
    i32::try_from(p.value()).unwrap_or(i32::MAX)
}

/// A screen position from pixel arithmetic, pinned to the record's range.
fn screen(v: i32) -> Screen {
    Screen(i16::try_from(v).unwrap_or(i16::MAX))
}

/// A filled rectangle `w` by `h` at `(x, y)` in `color`.
fn solid(x: Screen, y: Screen, w: Px, h: Px, color: Argb) -> Instance {
    Instance {
        x,
        y,
        w: Tex(u16::try_from(w.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(h.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color,
        kind: Kind::Solid,
    }
}

/// One cell's input to the shaper: its base and attached codepoints.
struct ClusterIn {
    cps: Vec<Codepoint>,
    /// Offset of the cell within the run.
    at: CellIx,
}

/// Where the cursor is, when it is shown, and its shape.
type Cursor = Option<(Row, Col, CursorStyle)>;

/// Whether the previous frame's copy describes what is on screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    /// Nothing painted yet, or the geometry changed: the next frame is full.
    Unknown,
    Known,
}

pub struct Renderer<'f, 'a> {
    stack: Stack<'f, 'a>,
    scales: Vec<Scale>,
    /// Per face, a digest per `GSUB` lookup the grid features name, so a
    /// cache miss opens only the lookups that can touch the run.
    digests: Vec<Vec<LookupDigest>>,
    /// Per face, the prefilter the counts in [`Exceptions`] ask.
    starters: Vec<Starter>,
    /// Per face, the glyph pairs its rules can begin on.
    pairs: Vec<Pairs>,
    /// The single-codepoint resolution per codepoint, memoised for the
    /// `differs` count: the face and glyph the cell shader's map would
    /// draw, with its offsets -- not an atlas slot, which a repack moves.
    #[cfg(feature = "exceptions")]
    singles: std::collections::HashMap<u32, Option<(FaceIx, Placed)>>,
    metrics: Metrics,
    theme: Theme,
    blend: Blend,
    cache: RunCache,
    atlas: Atlas,
    cols: Cols,
    rows: Rows,
    prev: Vec<PackedCell>,
    known: Prev,
    cur: Vec<PackedCell>,
    /// Per row, a fold of the renditions its cells resolve to. A `StyleId`
    /// indexes an evicting table, so an unchanged cell word can mean a
    /// changed colour; the word comparison alone would miss it.
    prev_styles: Vec<Digest>,
    cur_styles: Vec<Digest>,
    bg: Vec<Argb>,
    fg: Vec<Argb>,
    per_row: Vec<Vec<Instance>>,
    last_cursor: Cursor,
    /// This frame's blink phase, set by [`Renderer::set_blink`] before a
    /// convert or resolve, and whether any blinking cell was seen -- so the
    /// driver repaints on a phase flip only when something actually blinks.
    blink: Blink,
    blink_seen: Blinking,
    /// Which rows the last `convert` found changed; what `paint` repaints.
    dirty: Vec<bool>,
    /// The palette the last frame resolved against: `OSC 4` repaints
    /// cells whose words never change, so a different palette is a full
    /// frame.
    last_palette: Digest,
    storage: Vec<Glyph>,
}

impl<'f, 'a> Renderer<'f, 'a> {
    /// A renderer for `cols` by `rows` cells; `None` when the primary face
    /// yields no cell geometry at `size`.
    pub fn new(
        stack: Stack<'f, 'a>,
        size: PxSize,
        theme: Theme,
        blend: Blend,
        cols: Cols,
        rows: Rows,
    ) -> Option<Renderer<'f, 'a>> {
        let metrics = Metrics::of(stack.primary(), size)?;
        let scales = stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        let digests = stack
            .faces()
            .iter()
            .map(|f| {
                let mut d = Vec::new();
                if let Some(gsub) = f.gsub() {
                    d.resize(MAX_LOOKUPS, LookupDigest::EMPTY);
                    gsub.digests(Features::GRID, &mut d);
                }
                d
            })
            .collect();
        let starters = stack
            .faces()
            .iter()
            .map(|f| match (f.gsub(), f.morx()) {
                (Some(gsub), _) => {
                    let mut d = vec![LookupDigest::EMPTY; MAX_LOOKUPS];
                    gsub.digests(Features::GRID, &mut d);
                    Starter::Gsub(d.into_iter().fold(LookupDigest::EMPTY, LookupDigest::union))
                }
                (None, Some(_)) => Starter::Morx,
                (None, None) => Starter::Plain,
            })
            .collect();
        let pairs = stack.faces().iter().map(Pairs::of).collect();
        let mut r = Renderer {
            stack,
            scales,
            starters,
            pairs,
            #[cfg(feature = "exceptions")]
            singles: std::collections::HashMap::new(),
            digests,
            metrics,
            theme,
            blend,
            cache: RunCache::new(),
            atlas: Atlas::new(),
            cols: Cols::of_index(0),
            rows: Rows::of_index(0),
            prev: Vec::new(),
            known: Prev::Unknown,
            cur: Vec::new(),
            prev_styles: Vec::new(),
            cur_styles: Vec::new(),
            bg: Vec::new(),
            fg: Vec::new(),
            per_row: Vec::new(),
            last_cursor: None,
            blink: Blink::default(),
            blink_seen: Blinking::default(),
            dirty: Vec::new(),
            last_palette: Digest::default(),
            storage: vec![Glyph::new(GlyphId::NOTDEF, Cluster(0), Ignorable::No); 256],
        };
        r.resize(cols, rows);
        Some(r)
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// Sets the blink phase the next convert or resolve draws with.
    pub fn set_blink(&mut self, blink: Blink) {
        self.blink = blink;
    }

    /// Whether the last convert or resolve saw a cell with `Attrs::BLINK`,
    /// so a driver repaints on a phase flip only when something blinks.
    pub fn blink_seen(&self) -> Blinking {
        self.blink_seen
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.cache.stats()
    }

    pub fn atlas_stats(&self) -> AtlasStats {
        self.atlas.stats()
    }

    /// The pixel size of the whole grid.
    pub fn pixel_size(&self) -> (Px, Px) {
        (
            Px::of(
                u32::try_from(self.cols.index())
                    .unwrap_or(0)
                    .saturating_mul(self.metrics.cell_w.value()),
            ),
            Px::of(
                u32::try_from(self.rows.index())
                    .unwrap_or(0)
                    .saturating_mul(self.metrics.cell_h.value()),
            ),
        )
    }

    fn resize(&mut self, cols: Cols, rows: Rows) {
        self.cols = cols;
        self.rows = rows;
        let cells = cols.index() * rows.index();
        self.prev = vec![PackedCell::EMPTY; cells];
        self.cur = vec![PackedCell::EMPTY; cells];
        self.prev_styles = vec![Digest::default(); rows.index()];
        self.cur_styles = vec![Digest::default(); rows.index()];
        self.bg = vec![Argb(0); cells];
        self.fg = vec![Argb(0); cells];
        self.per_row = (0..rows.index()).map(|_| Vec::new()).collect();
        self.dirty = vec![false; rows.index()];
        self.known = Prev::Unknown;
    }

    fn row_cells(&self, r: Row) -> core::ops::Range<usize> {
        let cols = self.cols.index();
        r.index() * cols..(r.index() + 1) * cols
    }

    fn band(&self, r: Row) -> Band {
        let h = self.metrics.cell_h.value();
        let top = u32::try_from(r.index()).unwrap_or(0).saturating_mul(h);
        Band {
            top: Px::of(top),
            bottom: Px::of(top.saturating_add(h)),
        }
    }

    /// Reads the terminal, converts the rows that changed and paints them
    /// into `target`: [`Renderer::convert`] then [`Renderer::paint`].
    pub fn render(
        &mut self,
        term: &mut Term<'_>,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> Frame {
        let converted = self.convert(term, Rows::ZERO, damage);
        self.paint(target, converted)
    }

    /// Reads the terminal and converts the rows that changed since the
    /// last frame into instances and background words, without painting.
    /// A GPU path takes [`Renderer::instances`], [`Renderer::background`]
    /// and [`Renderer::atlas`] from here; the CPU path calls
    /// [`Renderer::paint`].
    /// Sizes the renderer to the terminal's grid, when it differs. Every
    /// frame goes through it -- [`Renderer::convert`] calls it, and a path
    /// that never converts, the cell path, must call it before it asks
    /// [`Renderer::pixel_size`] for a surface: the app once built its
    /// surface for the 80x24 grid the renderer started with, and the layer
    /// stretched those cells over a 200x50 window.
    pub fn fit(&mut self, term: &Term<'_>) {
        let (cols, rows) = term.geometry();
        if cols.index() != self.cols.index() || rows.index() != self.rows.index() {
            self.resize(cols, rows);
        }
    }

    pub fn convert(&mut self, term: &Term<'_>, back: Rows, damage: Damage) -> Converted {
        let t0 = Instant::now();
        self.blink_seen = Blinking::Unseen;
        self.fit(term);
        let (cols, rows) = term.geometry();
        let ncols = cols.index();
        // Each row read where the core holds it, through the rotation and
        // the ring, `back` lines into the history when the user is looking
        // back; the copy into `cur` is this path's own frame-to-frame damage
        // record, not the core's, and a change of `back` dirties the rows it
        // moves the way any other change does.
        let vp = term.view(back);
        for r in 0..rows.index() {
            self.cur[r * ncols..(r + 1) * ncols].copy_from_slice(vp.row(Row::from_index(r)));
        }
        for r in 0..rows.index() {
            let range = self.row_cells(Row::from_index(r));
            let mut d = Digest::start();
            for cell in &self.cur[range] {
                d = d.fold(term.style_of(*cell).key());
            }
            self.cur_styles[r] = d;
        }
        // No cursor while looking back: it belongs to the live screen, which
        // is `back` lines below the window.
        let cursor: Cursor = (term.cursor_visible() && back.index() == 0).then(|| {
            let (r, c) = term.cursor();
            (r, c, term.cursor_style())
        });
        let palette = palette_digest(term);
        let full =
            damage == Damage::Full || self.known == Prev::Unknown || palette != self.last_palette;
        self.last_palette = palette;
        let cursor_row = |c: Cursor| c.map(|(r, _, _)| r.index());
        let mut rows_dirty = Tally::default();
        let mut exceptions = Exceptions::default();
        for r in 0..rows.index() {
            let range = self.row_cells(Row::from_index(r));
            let changed = self.cur[range.clone()] != self.prev[range]
                || self.cur_styles[r] != self.prev_styles[r];
            let cursor_here = cursor_row(cursor) == Some(r);
            let cursor_was = cursor_row(self.last_cursor) == Some(r);
            let cursor_moved =
                cursor_here != cursor_was || (cursor_here && cursor != self.last_cursor);
            self.dirty[r] = full || changed || cursor_moved;
        }
        for r in 0..rows.index() {
            if self.dirty[r] {
                self.convert_row(term, Row::from_index(r), cursor, &mut exceptions);
                rows_dirty.bump();
            }
        }
        core::mem::swap(&mut self.prev, &mut self.cur);
        core::mem::swap(&mut self.prev_styles, &mut self.cur_styles);
        self.known = Prev::Known;
        self.last_cursor = cursor;
        Converted {
            rows_dirty,
            rows: Tally::of(rows.index()),
            convert: t0.elapsed(),
            exceptions,
        }
    }

    /// Paints the rows the last [`Renderer::convert`] found dirty into
    /// `target`, which must still hold the frame before.
    pub fn paint(&mut self, target: &mut Target<'_>, converted: Converted) -> Frame {
        let t1 = Instant::now();
        let mut frame = Frame {
            rows_dirty: converted.rows_dirty,
            rows: converted.rows,
            convert: converted.convert,
            exceptions: converted.exceptions,
            ..Frame::default()
        };
        for r in 0..self.rows.index() {
            let row = Row::from_index(r);
            frame.instances.add(Tally::of(self.per_row[r].len()));
            for inst in &self.per_row[r] {
                match inst.kind {
                    Kind::Color => frame.colored.bump(),
                    Kind::Solid => frame.solids.bump(),
                    Kind::Mask => {}
                }
            }
            if !self.dirty[r] {
                continue;
            }
            let band = self.band(row);
            target.fill_band(band, &self.bg[self.row_cells(row)], self.metrics.cell_w);
            for inst in &self.per_row[r] {
                target.over(inst, &self.atlas, self.blend, band);
            }
        }
        frame.composite = t1.elapsed();
        frame.cache = self.cache.stats();
        frame.atlas = self.atlas.stats();
        frame
    }

    /// Every instance of the current frame, row by row: what a GPU path
    /// uploads whole (D2: nobody uploads partial ranges).
    pub fn instances(&self) -> impl Iterator<Item = &Instance> + '_ {
        self.per_row.iter().flatten()
    }

    /// The background word of every cell, row-major, `cols` by `rows`.
    pub fn background(&self) -> &[Argb] {
        &self.bg
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// The atlas, for a copy that takes its dirty rectangles.
    pub fn atlas_mut(&mut self) -> &mut Atlas {
        &mut self.atlas
    }

    /// The grid the last frame was converted for.
    pub fn grid(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }

    /// The glyph a codepoint draws as a cluster of one: the face the stack
    /// selects, shaped alone so a single substitution still applies, its
    /// atlas slot with the shaper's offsets folded into the placement.
    /// `None` when it draws nothing. What the cell path's map holds.
    /// A whole cluster -- a base and its attached codepoints -- shaped by
    /// itself: every glyph with its atlas slot and its offsets in pixels.
    /// What the overlay draws for a marked cell; the shaper is the one
    /// definition of what a family or a base with a mark looks like.
    pub fn cluster(&mut self, cps: &[Codepoint]) -> Vec<ClusterGlyph> {
        let Some(&base) = cps.first() else {
            return Vec::new();
        };
        let face = self.stack.select(base);
        let clusters = [ClusterIn {
            cps: cps.to_vec(),
            at: CellIx(0),
        }];
        let shaped = self.shape(face, &clusters);
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let mut out = Vec::new();
        for p in shaped.glyphs() {
            let Some(slot) = self.atlas.slot(f, face, p.id, scale) else {
                continue;
            };
            out.push(ClusterGlyph {
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32,
                dy: scale.px(f32::from(p.dy)).round() as i32,
            });
        }
        out
    }

    pub fn single(&mut self, cp: Codepoint) -> Option<crate::atlas::Slot> {
        let face = self.stack.select(cp);
        let clusters = [ClusterIn {
            cps: vec![cp],
            at: CellIx(0),
        }];
        let shaped = self.shape(face, &clusters);
        let p = *shaped.glyphs().first()?;
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let mut slot = self.atlas.slot(f, face, p.id, scale)?;
        slot.left += scale.px(f32::from(p.dx)).round() as i32;
        slot.top -= scale.px(f32::from(p.dy)).round() as i32;
        Some(slot)
    }

    /// The terminal's rendition table resolved through the theme and the
    /// live palette into `out`, one entry a slot, by the same rules
    /// `convert` applies a cell at a time -- so the cell path draws a
    /// rendition the way the instance path does, without a second spelling
    /// of them. `out` is the memory a shader reads, so nothing is copied
    /// after this. Slots past `out` are not resolved; the count written is
    /// returned.
    pub fn resolve_styles_into(&mut self, term: &Term<'_>, out: &mut [Resolved]) -> Tally {
        self.blink_seen = Blinking::Unseen;
        let table = term.styles();
        let mut n = Tally::default();
        for (slot, dst) in table.iter().zip(out.iter_mut()) {
            let style = slot.unwrap_or(baud_domain::StyleWord::DEFAULT);
            let (mut fg, mut bg) = self.theme.resolve(style, |ch| term.palette(ch));
            if style.bg().is_default() && !style.attrs().has(Attrs::REVERSE) {
                bg = bg.with_alpha(self.theme.bg_alpha);
            }
            let attrs = style.attrs();
            let blink_hidden = if attrs.has(Attrs::BLINK) {
                self.blink_seen = Blinking::Seen;
                self.blink == Blink::Hidden
            } else {
                false
            };
            if blink_hidden {
                fg = bg;
            }
            let mut shapes = Shapes::default();
            if !blink_hidden && (attrs.has(Attrs::UNDERLINE) || attrs.has(Attrs::DOUBLE_UNDERLINE))
            {
                shapes = shapes.with(Shapes::UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::DOUBLE_UNDERLINE) {
                shapes = shapes.with(Shapes::DOUBLE_UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::STRIKE) {
                shapes = shapes.with(Shapes::STRIKE);
            }
            let underline = self.theme.underline(style, fg, |ch| term.palette(ch));
            if underline == fg && style.underline().is_default() {
                shapes = shapes.with(Shapes::UNDERLINE_FOLLOWS_FG);
            }
            *dst = Resolved {
                fg,
                bg,
                underline,
                shapes,
            };
            n.bump();
        }
        n
    }

    /// Resolves one row's colours and glyphs.
    fn convert_row(&mut self, term: &Term<'_>, r: Row, cursor: Cursor, exc: &mut Exceptions) {
        let range = self.row_cells(r);
        // A copy of the row, so shaping may borrow the renderer mutably.
        let row: Vec<PackedCell> = self.cur[range.clone()].to_vec();
        let mut instances = core::mem::take(&mut self.per_row[r.index()]);
        instances.clear();
        // Colours first, cell by cell, with the shapes a rendition asks
        // for: an underline, a strike, an underline or bar cursor. A block
        // cursor is the cell's colours swapped. Shapes go on after the
        // glyphs, so a strike crosses its letter rather than hiding under
        // an opaque colour glyph.
        let mut shapes: Vec<Instance> = Vec::new();
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        for (c, cell) in row.iter().enumerate() {
            let style = term.style_of(*cell);
            let (mut fg, mut bg) = self.theme.resolve(style, |ch| term.palette(ch));
            // Only the default background lets the glass through (D14).
            if style.bg().is_default() && !style.attrs().has(Attrs::REVERSE) {
                bg = bg.with_alpha(self.theme.bg_alpha);
            }
            let here = cursor.filter(|&(cr, cc, _)| cr == r && cc == Col::at_index(c));
            if matches!(
                here,
                Some((_, _, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock))
            ) {
                core::mem::swap(&mut fg, &mut bg);
                bg = bg.with_alpha(Chan::from_u8(0xFF));
            }
            let attrs = style.attrs();
            // Blink hides the whole foreground -- glyph, underline, strike --
            // on the off phase, by drawing it in the cell's own background.
            let blink_hidden = if attrs.has(Attrs::BLINK) {
                self.blink_seen = Blinking::Seen;
                self.blink == Blink::Hidden
            } else {
                false
            };
            if blink_hidden {
                fg = bg;
            }
            self.fg[range.start + c] = fg;
            self.bg[range.start + c] = bg;
            let x = i32::try_from(c).unwrap_or(0) * cell_w;
            let double = attrs.has(Attrs::DOUBLE_UNDERLINE);
            if !blink_hidden && (attrs.has(Attrs::UNDERLINE) || double) {
                let color = self.theme.underline(style, fg, |ch| term.palette(ch));
                let ul = px_i32(self.metrics.underline);
                shapes.push(solid(
                    screen(x),
                    screen(top + ul),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    color,
                ));
                if double {
                    // A second line below, one thickness of gap between them,
                    // clamped so it stays inside the cell.
                    let gap = i32::try_from(self.metrics.thickness.value().max(1)).unwrap_or(1) + 1;
                    let low = px_i32(self.metrics.cell_h) - px_i32(self.metrics.thickness);
                    let y = (ul + gap).min(low.max(ul));
                    shapes.push(solid(
                        screen(x),
                        screen(top + y),
                        self.metrics.cell_w,
                        self.metrics.thickness,
                        color,
                    ));
                }
            }
            if !blink_hidden && attrs.has(Attrs::STRIKE) {
                shapes.push(solid(
                    screen(x),
                    screen(top + px_i32(self.metrics.strike)),
                    self.metrics.cell_w,
                    self.metrics.thickness,
                    fg,
                ));
            }
            match here {
                Some((_, _, CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline)) => {
                    let h = Px::of(self.metrics.thickness.value().max(2));
                    let y = top + px_i32(self.metrics.cell_h) - px_i32(h);
                    shapes.push(solid(screen(x), screen(y), self.metrics.cell_w, h, fg));
                }
                Some((_, _, CursorStyle::BlinkingBar | CursorStyle::SteadyBar)) => {
                    let w = Px::of(self.metrics.thickness.value().max(2));
                    shapes.push(solid(screen(x), screen(top), w, self.metrics.cell_h, fg));
                }
                _ => {}
            }
        }
        // Then runs of cells shaped by one face.
        let mut c = 0usize;
        while c < row.len() {
            let Some(base) = row[c].content.as_codepoint() else {
                c += 1;
                continue;
            };
            let face = self.stack.select(base);
            // Gather the run: consecutive drawn cells on this face.
            let start = c;
            let mut clusters: Vec<ClusterIn> = Vec::new();
            let mut key = Key::start().fold(u32::try_from(face.index()).unwrap_or(0));
            let prefilter = self.starters[face.index()];
            let mut starters_here = Tally::default();
            let mut prev_glyph: Option<GlyphId> = None;
            let mut pair_here = Tally::default();
            while c < row.len() && c - start < RUN_CELLS {
                let cell = row[c];
                let Some(cp) = cell.content.as_codepoint() else {
                    break;
                };
                if self.stack.select(cp) != face {
                    break;
                }
                let width = if cell.flags.is_wide() { 2usize } else { 1usize };
                let glyph = self.stack.face(face).glyph(cp);
                if let (Some(p), Some(g)) = (prev_glyph, glyph) {
                    if self.pairs[face.index()].admits(p, g) {
                        pair_here.bump();
                    }
                }
                prev_glyph = glyph;
                match prefilter {
                    Starter::Gsub(d) => {
                        if let Some(g) = glyph {
                            let mut one = LookupDigest::EMPTY;
                            one.add(g);
                            if d.may_have(one) {
                                starters_here.bump();
                            }
                        }
                    }
                    Starter::Morx => exc.morx.bump(),
                    Starter::Plain => {}
                }
                let mut cps = vec![cp];
                key = key.fold(cp.value());
                if cell.flags.is_marked() {
                    exc.marked.bump();
                    for m in term.cluster_marks(cell) {
                        if let Some(mc) = Codepoint::new(m) {
                            cps.push(mc);
                            key = key.fold(m);
                        }
                    }
                }
                key = key.fold(u32::try_from(width).unwrap_or(0) << 24);
                clusters.push(ClusterIn {
                    cps,
                    at: CellIx(u8::try_from(c - start).unwrap_or(u8::MAX)),
                });
                c += width;
            }
            let shaped = match self.cache.get(key) {
                Some(s) => s,
                None => {
                    let s = self.shape(face, &clusters);
                    self.cache.put(key, s);
                    s
                }
            };
            if starters_here.value() > 0 {
                exc.starters.add(starters_here);
                exc.shaped.add(Tally::of(c - start));
            }
            if pair_here.value() > 0 {
                exc.bigram.add(Tally::of(c - start));
            }
            for p in shaped.glyphs() {
                if p.last != p.cell {
                    exc.spanning
                        .add(Tally::of(usize::from(p.last.0) - usize::from(p.cell.0) + 1));
                }
            }
            #[cfg(feature = "exceptions")]
            self.count_differs(r, Col::at_index(start), &clusters, &shaped, exc);
            self.emit(face, r, Col::at_index(start), &shaped, &mut instances);
        }
        instances.extend(shapes);
        self.per_row[r.index()] = instances;
    }

    fn shape(&mut self, face: FaceIx, clusters: &[ClusterIn]) -> Shaped {
        let f = self.stack.face(face);
        let mut shaper = Shaper::new(f, &mut self.storage);
        shaper.accelerate(&self.digests[face.index()]);
        for cl in clusters {
            if shaper
                .push_cluster(&cl.cps, Cluster(u16::from(cl.at.0)))
                .is_none()
            {
                break;
            }
        }
        let (buf, _) = shaper.finish(Features::GRID);
        let mut out = Shaped::EMPTY;
        for g in buf.as_slice() {
            if g.ignorable == Ignorable::Yes {
                continue;
            }
            let p = Placed {
                id: g.id,
                cell: CellIx(u8::try_from(g.cluster.0).unwrap_or(u8::MAX)),
                last: CellIx(u8::try_from(g.last.0).unwrap_or(u8::MAX)),
                dx: g.dx,
                dy: g.dy,
            };
            if out.push(p).is_none() {
                break;
            }
        }
        out
    }

    /// Instances for a shaped run starting at column `start` of row `r`.
    /// The `differs` count: for each cluster of the run, whether the
    /// shaped glyphs are what the cell shader's map would draw for its
    /// base codepoint -- one glyph, the same atlas slot, the same offset.
    #[cfg(feature = "exceptions")]
    fn count_differs(
        &mut self,
        r: Row,
        start: Col,
        clusters: &[ClusterIn],
        shaped: &Shaped,
        exc: &mut Exceptions,
    ) {
        let range = self.row_cells(r);
        for cl in clusters {
            let col = start.index() + usize::from(cl.at.0);
            let Some(cp) = self
                .cur
                .get(range.start + col)
                .and_then(|c| c.content.as_codepoint())
            else {
                continue;
            };
            let alone = match self.singles.get(&cp.value()) {
                Some(s) => *s,
                None => {
                    let face = self.stack.select(cp);
                    let one = [ClusterIn {
                        cps: vec![cp],
                        at: CellIx(0),
                    }];
                    let s = self.shape(face, &one).glyphs().first().map(|p| (face, *p));
                    self.singles.insert(cp.value(), s);
                    s
                }
            };
            let face = self.stack.select(cp);
            let mine: Vec<Placed> = shaped
                .glyphs()
                .iter()
                .copied()
                .filter(|p| p.cell == cl.at)
                .collect();
            let same = match (alone, mine.as_slice()) {
                (None, []) => true,
                (Some((af, a)), [p]) => af == face && a.id == p.id && a.dx == p.dx && a.dy == p.dy,
                _ => false,
            };
            if !same {
                exc.differs.bump();
            }
        }
    }

    fn emit(&mut self, face: FaceIx, r: Row, start: Col, shaped: &Shaped, out: &mut Vec<Instance>) {
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        let range = self.row_cells(r);
        for p in shaped.glyphs() {
            let Some(slot) = self.atlas.slot(f, face, p.id, scale) else {
                continue;
            };
            let col = start.index() + usize::from(p.cell.0);
            let color = self
                .fg
                .get(range.start + col)
                .copied()
                .unwrap_or(self.theme.fg);
            let dx = scale.px(f32::from(p.dx)).round() as i32;
            let dy = scale.px(f32::from(p.dy)).round() as i32;
            let x = i32::try_from(col).unwrap_or(0) * cell_w + dx + slot.left;
            let y = top + baseline - dy + slot.top;
            out.push(Instance {
                x: Screen(i16::try_from(x).unwrap_or(i16::MAX)),
                y: Screen(i16::try_from(y).unwrap_or(i16::MAX)),
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
