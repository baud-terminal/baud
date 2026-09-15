//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Code, Kind, Tex};
use crate::cache::{CacheStats, CellIx, Key, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
use crate::cellmap::{Resolved, Shapes};
use crate::color::{Argb, Theme};
use crate::composite::{Blend, Target};
use crate::instance::{Instance, Screen};
use crate::metrics::{Metrics, PxSize};
use crate::scrollbar::{self, Thumb, Window};
use crate::select::Highlight;
use crate::stack::{FaceIx, Stack};
use crate::units::{Band, Digest, Tally};
use baud_core::Term;
use baud_domain::{Attrs, Chan, Codepoint, Col, Cols, CursorStyle, GlyphId, PackedCell, Row, Rows};
use baud_font::bytes::Count;
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
    /// By first glyph: the digest of what may follow it, `None` for a
    /// glyph no rule begins on. Indexed rather than hashed: the walk asks
    /// once per adjacent pair whose entries carry both bits.
    after: Vec<Option<LookupDigest>>,
    /// Every glyph that may follow some first: the second bit's test.
    seconds: LookupDigest,
    walked: Option<Walked>,
}

/// Whether a glyph can begin a pair, end one, both or neither -- the two
/// bits the glyph map carries per entry so the walk consults the digests
/// only where both are set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairRole {
    Neither,
    Starts,
    Ends,
    Both,
}

impl Pairs {
    fn of(face: &Face<'_>) -> Pairs {
        let mut after: Vec<Option<LookupDigest>> = Vec::new();
        let mut seconds = LookupDigest::EMPTY;
        let walked = face.gsub().map(|gsub| {
            gsub.pairs(Features::GRID, &mut |first, d| {
                let i = first.index();
                if after.len() <= i {
                    after.resize(i + 1, None);
                }
                let slot = after[i].get_or_insert(LookupDigest::EMPTY);
                *slot = slot.union(d);
                seconds = seconds.union(d);
            })
        });
        Pairs {
            after,
            seconds,
            walked,
        }
    }

    pub fn role(&self, g: GlyphId) -> PairRole {
        let (starts, ends) = match self.walked {
            None => (false, false),
            Some(Walked::Unreadable) => (true, true),
            Some(Walked::Derived) => (
                self.after.get(g.index()).is_some_and(Option::is_some),
                self.seconds.may_have(LookupDigest::of(g)),
            ),
        };
        match (starts, ends) {
            (false, false) => PairRole::Neither,
            (true, false) => PairRole::Starts,
            (false, true) => PairRole::Ends,
            (true, true) => PairRole::Both,
        }
    }

    pub fn admits(&self, a: GlyphId, b: GlyphId) -> bool {
        match self.walked {
            None => false,
            Some(Walked::Unreadable) => true,
            Some(Walked::Derived) => self
                .after
                .get(a.index())
                .and_then(Option::as_ref)
                .is_some_and(|d| d.may_have(LookupDigest::of(b))),
        }
    }

    /// How many first glyphs the tables name.
    pub fn firsts(&self) -> Tally {
        Tally::of(self.after.iter().filter(|d| d.is_some()).count())
    }
}

/// How a face's substitutions can be prefiltered: a count behind the
/// `exceptions` feature, kept because the bench reports it.
#[cfg(feature = "exceptions")]
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
    pub id: GlyphId,
    /// `None` for a glyph with no ink -- a space, a spacer a ligature
    /// leaves behind -- which is still the glyph the cell draws.
    pub slot: Option<crate::atlas::Slot>,
    pub dx: i32,
    pub dy: i32,
}

/// What a codepoint shapes to alone: the glyph the cell shader draws for
/// it, its atlas slot when it has ink, and its pair role.
#[derive(Clone, Copy, Debug)]
pub struct Single {
    pub slot: Option<crate::atlas::Slot>,
    pub glyph: GlyphId,
    pub role: PairRole,
    pub face: FaceIx,
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
    #[cfg(feature = "exceptions")]
    starters: Vec<Starter>,
    /// Per face, the glyph pairs its rules can begin on.
    pairs: Vec<Pairs>,
    /// Per face, the longest sequence a rule can match, in glyphs; `None`
    /// for a face with no substitution rules or one whose tables could
    /// not be walked.
    extents: Vec<Option<Count>>,
    /// The faces by the code a map entry carries.
    faces: Vec<FaceIx>,
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
    /// The selection on this frame's view, set by [`Renderer::set_highlight`]
    /// before a convert, and the one the last convert drew -- a row whose
    /// share of it changed is dirty, the cursor's own rule.
    highlight: Option<Highlight>,
    last_highlight: Option<Highlight>,
    /// The scrollbar's thumb on this frame and the last, and its one solid
    /// instance: a row the thumb covered or covers is dirty, the same rule.
    bar: Option<Thumb>,
    last_bar: Option<Thumb>,
    bar_inst: Vec<Instance>,
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
        #[cfg(feature = "exceptions")]
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
        let extents = stack
            .faces()
            .iter()
            .map(|f| f.gsub().and_then(|g| g.extent(Features::GRID)))
            .collect();
        let faces = stack.each().collect();
        let mut r = Renderer {
            stack,
            scales,
            #[cfg(feature = "exceptions")]
            starters,
            pairs,
            extents,
            faces,
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
            highlight: None,
            last_highlight: None,
            bar: None,
            last_bar: None,
            bar_inst: Vec::new(),
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
    /// A new pixel size for the same faces -- the backing scale changed, or
    /// the font size -- so the metrics, the per-face scales and the atlas are
    /// rebuilt and the next convert repaints every row. `None` leaves the
    /// renderer as it was, when the primary face cannot be measured at that
    /// size.
    ///
    /// **The run cache is kept.** A `Shaped` holds glyph ids, cell spans and
    /// offsets *in font units* (`Placed`), and the current `scale` is applied
    /// at composite -- none of it depends on the pixel size, so shaping a run
    /// at the old size still names the right glyphs at the new one. Only the
    /// atlas holds pixels, so only the atlas resets; clearing the cache too
    /// re-shaped every visible run for nothing, which was ~1.4 ms of the cold
    /// frame a font change forces (btop 14->24 px: convert 2.5 ms to 1.1 ms,
    /// cache misses 163 to 0). The rasterising that remains is the size
    /// change itself and cannot be skipped.
    pub fn rescale(&mut self, size: PxSize) -> Option<()> {
        let metrics = Metrics::of(self.stack.primary(), size)?;
        self.scales = self
            .stack
            .faces()
            .iter()
            .map(|f| Scale::of(size.value(), f.units_per_em()))
            .collect();
        self.metrics = metrics;
        self.atlas.reset();
        #[cfg(feature = "exceptions")]
        self.singles.clear();
        self.known = Prev::Unknown;
        Some(())
    }

    /// The selection to draw on the next frame, in the view's screen
    /// coordinates, or none. One rule on every path: a selected cell's
    /// resolved colours swap, before the block cursor's own swap.
    pub fn set_highlight(&mut self, highlight: Option<Highlight>) {
        self.highlight = highlight;
    }

    /// The scrollbar for the next frame, from where the window sits, or
    /// none: the thumb's geometry is the spec's, its instance is one solid
    /// in the theme's foreground, and every path draws it -- the CPU and
    /// instance paths from the instance list, the cell path from
    /// [`Renderer::scrollbar`] appended to its records.
    pub fn set_scrollbar(&mut self, win: Option<Window>) {
        let size = self.pixel_size();
        self.bar =
            win.and_then(|w| scrollbar::thumb(w, size, self.metrics.cell_w, self.metrics.cell_h));
        self.bar_inst.clear();
        if let Some(t) = self.bar {
            self.bar_inst.push(scrollbar::instance(t, self.theme.fg));
        }
    }

    /// The scrollbar's thumb, if one is drawn: what a press is tested
    /// against.
    pub fn thumb(&self) -> Option<Thumb> {
        self.bar
    }

    /// The scrollbar's instances, for a path that draws records of its own.
    pub fn scrollbar(&self) -> &[Instance] {
        &self.bar_inst
    }

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

    /// The default-background colour, straight (not premultiplied), for a
    /// presenter to clear the margin around the grid to -- so the gutter is
    /// the terminal's own background rather than a see-through edge. It is
    /// what a default cell resolves to: the theme background at its alpha,
    /// the one colour the glass shows through (D14).
    pub fn clear_color(&self) -> Argb {
        self.theme.bg.with_alpha(self.theme.bg_alpha)
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
            let row = Row::from_index(r);
            let selected = |h: Option<Highlight>| h.filter(|h| h.covers_row(row));
            let selection_moved = selected(self.highlight) != selected(self.last_highlight);
            let band = self.band(row);
            let under_bar = |b: Option<Thumb>| b.is_some_and(|t| t.overlaps(band.top, band.bottom));
            let bar_moved = under_bar(self.bar) != under_bar(self.last_bar)
                || (under_bar(self.bar) && self.bar != self.last_bar);
            self.dirty[r] = full || changed || cursor_moved || selection_moved || bar_moved;
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
        self.last_highlight = self.highlight;
        self.last_bar = self.bar;
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
            for inst in &self.bar_inst {
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
        self.per_row.iter().flatten().chain(self.bar_inst.iter())
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
            let slot = self.atlas.slot(f, face, p.id, scale);
            out.push(ClusterGlyph {
                id: p.id,
                slot,
                dx: scale.px(f32::from(p.dx)).round() as i32,
                dy: scale.px(f32::from(p.dy)).round() as i32,
            });
        }
        out
    }

    pub fn single(&mut self, cp: Codepoint) -> Option<Single> {
        let face = self.stack.select(cp);
        let clusters = [ClusterIn {
            cps: vec![cp],
            at: CellIx(0),
        }];
        let shaped = self.shape(face, &clusters);
        let p = *shaped.glyphs().first()?;
        let scale = self.scales[face.index()];
        let f = self.stack.face(face);
        let role = self.pairs[face.index()].role(p.id);
        let slot = self.atlas.slot(f, face, p.id, scale).map(|mut slot| {
            slot.left += scale.px(f32::from(p.dx)).round() as i32;
            slot.top -= scale.px(f32::from(p.dy)).round() as i32;
            slot
        });
        Some(Single {
            slot,
            glyph: p.id,
            role,
            face,
        })
    }

    /// Whether two adjacent glyphs on the face a map entry names can
    /// begin a substitution there.
    pub fn admits(&self, face: Code, a: GlyphId, b: GlyphId) -> bool {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.pairs.get(i))
            .is_some_and(|p| p.admits(a, b))
    }

    /// The longest rule the face a map entry names can match, in glyphs.
    pub fn extent(&self, face: Code) -> Option<Count> {
        usize::try_from(face.0)
            .ok()
            .and_then(|i| self.extents.get(i))
            .copied()
            .flatten()
    }

    /// A window of cells -- each a base with its attached codepoints --
    /// shaped together as one run on the face named, answered per
    /// cell: the glyphs the shaper placed at that cell, with slots and
    /// offsets. A cell with no glyph of its own -- swallowed into a
    /// ligature -- answers an empty list.
    pub fn window(&mut self, face: Code, cells: &[Vec<Codepoint>]) -> Vec<Vec<ClusterGlyph>> {
        // **Shaped in chunks, because `Shaped` holds `PLACED_MAX` glyphs
        // and a longer run used to lose its tail in silence.** `shape`
        // stops pushing once the buffer is full and breaks, so a window of
        // 120 cells was shaped for 96 of them and the rest fell back to
        // the glyph the codepoint has alone. On a band that runs the width
        // of the screen -- asciiquarium's waterline is exactly that -- the
        // right-hand quarter drew unshaped while the left drew shaped, and
        // nothing said so: the pixel gate's scene was forty columns wide,
        // which is narrower than the bug.
        //
        // Each chunk is shaped with `ext` cells of context on either side
        // -- the face's longest rule, the same span the window itself is
        // built from -- and only the middle is kept, so a substitution
        // that straddles a chunk boundary still sees what it needs.
        let ext = self
            .extent(face)
            .and_then(|e| usize::try_from(e.value()).ok())
            .unwrap_or(1)
            .max(1);
        let Some(&facex) = usize::try_from(face.0).ok().and_then(|i| self.faces.get(i)) else {
            return Vec::new();
        };
        let scale = self.scales[facex.index()];
        let f = self.stack.face(facex);
        let mut out: Vec<Vec<ClusterGlyph>> = vec![Vec::new(); cells.len()];
        // Half the glyph budget in cells, so a cell that shapes to two
        // glyphs still fits, and the context on both sides fits with it.
        let step = (PLACED_MAX / 2).saturating_sub(2 * ext).max(1);
        let mut at = 0;
        while at < cells.len() {
            let end = (at + step).min(cells.len());
            let lo = at.saturating_sub(ext);
            let hi = (end + ext).min(cells.len());
            let clusters: Vec<ClusterIn> = cells[lo..hi]
                .iter()
                .enumerate()
                .map(|(i, cps)| ClusterIn {
                    cps: cps.clone(),
                    at: CellIx(u8::try_from(i).unwrap_or(u8::MAX)),
                })
                .collect();
            let shaped = self.shape(facex, &clusters);
            for p in shaped.glyphs() {
                let cell = lo + usize::from(p.cell.0);
                // Only this chunk's own cells; the context either side
                // belongs to the chunk that owns it.
                if cell < at || cell >= end {
                    continue;
                }
                let slot = self.atlas.slot(f, facex, p.id, scale);
                if let Some(o) = out.get_mut(cell) {
                    o.push(ClusterGlyph {
                        id: p.id,
                        slot,
                        dx: scale.px(f32::from(p.dx)).round() as i32,
                        dy: scale.px(f32::from(p.dy)).round() as i32,
                    });
                }
            }
            at = end;
        }
        out
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
            if self
                .highlight
                .is_some_and(|h| h.contains(r, Col::at_index(c)))
            {
                core::mem::swap(&mut fg, &mut bg);
                bg = bg.with_alpha(Chan::from_u8(0xFF));
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
            // The counts behind the `exceptions` feature: a cmap lookup, a
            // digest test and a pair test per cell, which the bench reads
            // and the app must not pay for.
            #[cfg(feature = "exceptions")]
            let prefilter = self.starters[face.index()];
            #[cfg(feature = "exceptions")]
            let mut starters_here = Tally::default();
            #[cfg(feature = "exceptions")]
            let mut prev_glyph: Option<GlyphId> = None;
            #[cfg(feature = "exceptions")]
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
                #[cfg(feature = "exceptions")]
                {
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
            #[cfg(feature = "exceptions")]
            {
                if starters_here.value() > 0 {
                    exc.starters.add(starters_here);
                    exc.shaped.add(Tally::of(c - start));
                }
                if pair_here.value() > 0 {
                    exc.bigram.add(Tally::of(c - start));
                }
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
