//! The frame: rows that changed, converted and painted.

use crate::atlas::{Atlas, AtlasStats, Code, Kind, Tex};
use crate::cache::{CacheStats, CellIx, Key, PLACED_MAX, Placed, RUN_CELLS, RunCache, Shaped};
use crate::cellmap::{Resolved, Shapes};
use crate::color::{Argb, Theme, Video};
use crate::composite::{Blend, Target};
use crate::instance::{Instance, Screen};
use crate::metrics::{Metrics, PxSize};
use crate::picture::{Pictured, Pictures};
use crate::scrollbar::{self, Bar, Scroll};
use crate::select::Highlight;
use crate::stack::{FaceIx, Stack};
use crate::trail::{self, Ghost, Trail};
use crate::transition::{self, Transition, Veil};
use crate::units::{Band, Digest, Tally};
use baud_core::Term;
use baud_domain::{
    Attrs, Chan, Codepoint, Col, Cols, CursorStyle, DynamicColor, GlyphId, ImageGen, ImageId,
    PackedCell, Row, Rows,
};
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
    // The dynamic colours ride in the same digest: an `OSC 11` moves no cell
    // word and no style slot, so without this a changed default background
    // would repaint nothing, the way a theme swap did before `set_theme`
    // dropped the damage cache.
    for which in [DynamicColor::Foreground, DynamicColor::Background] {
        let (r, g, b) = term.dynamic_color(which).unwrap_or((0, 0, 0));
        let set = u8::from(term.dynamic_color(which).is_some());
        d = d.fold_bytes(&[r, g, b, set]);
    }
    // And so does `?5`: a flip moves no cell word and no style slot either,
    // and a program flashes it as a visual bell.
    d.fold_bytes(&[u8::from(term.reverse_video())])
}

/// The video the screen's `?5` asks for.
fn video_of(term: &Term<'_>) -> Video {
    if term.reverse_video() {
        Video::Reverse
    } else {
        Video::Normal
    }
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

/// Whether a convert repaints the rows the frame's image placements cover.
/// The CPU path blits images over the rows it painted, so it needs them
/// repainted; a GPU path draws images in their own pass and does not.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImageRows {
    Ignored,
    Repainted,
}

/// Whether a placement's pixel rows meet the band's.
fn covers(p: &ImagePlacement, band: Band) -> bool {
    let top = i64::from(p.y);
    top < i64::from(band.bottom.value()) && top + i64::from(p.h) > i64::from(band.top.value())
}

/// Whether the last CPU frame blitted a placement over the band that this
/// frame does not: an image left or moved from there, or another took its
/// rect. Whole placements are compared -- image, epoch and rect -- so a
/// different image in the same rect still counts. One that *arrives* needs no
/// repaint: its blit lands on the cells, which is what a repaint would give.
fn left_from(now: &[ImagePlacement], last: &[ImagePlacement], band: Band) -> bool {
    last.iter().any(|p| covers(p, band) && !now.contains(p))
}

/// Whether [`Renderer::set_images`] keeps a copy of each image's pixels for a
/// GPU to upload. The CPU path blits from the terminal's own pixels, so it
/// asks for placements alone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Snapshots {
    Kept,
    Skipped,
}

/// One visible image to draw: which image (and the store epoch it was seen
/// at, the GPU texture-cache key), and where on screen at what size, in device
/// pixels. The origin is signed, so a partly-scrolled image hangs off the top.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ImagePlacement {
    /// Which stored image this draws.
    pub id: ImageId,
    /// The store epoch when it was snapshotted -- the cache key with `id`.
    pub epoch: ImageGen,
    /// The screen x of the top-left, device pixels, signed.
    pub x: i16,
    /// The screen y of the top-left, device pixels, signed.
    pub y: i16,
    /// The image width in device pixels.
    pub w: u16,
    /// The image height in device pixels.
    pub h: u16,
}

/// One image's pixels to upload once: keyed by `id` and `epoch`, premultiplied
/// `0xAARRGGBB` words, `w * h` of them, row-major. A transparent (unpainted
/// Sixel) pixel is `0x00000000`.
pub struct ImageUpload {
    /// Which stored image these pixels are.
    pub id: ImageId,
    /// The store epoch -- the cache key with `id`.
    pub epoch: ImageGen,
    /// The image width in pixels.
    pub w: u16,
    /// The image height in pixels.
    pub h: u16,
    /// The premultiplied pixels, `w * h`, row-major.
    pub pixels: Vec<u32>,
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
    /// The screen's `?5`, read once a frame by `refresh_screen`.
    video: Video,
    /// The default foreground and background the screen draws this frame --
    /// the theme's, moved by `OSC 10`/`11` and swapped by `?5`. What is drawn
    /// "in the foreground" or "in the background" takes these: the margin,
    /// the scrollbar, a cursor trail and a screen fade.
    defaults: (Argb, Argb),
    blend: Blend,
    cache: RunCache,
    atlas: Atlas,
    cols: Cols,
    rows: Rows,
    prev: Vec<PackedCell>,
    known: Prev,
    cur: Vec<PackedCell>,
    /// The cells of the frame being converted that belong to a picture
    /// drawn in text, whose backgrounds are drawn solid under the dim --
    /// [`Theme::finds_pictures`]. Found over the whole screen, because a
    /// picture is a property of a block of cells rather than of one.
    pictures: Pictures,
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
    /// The scrollbar on this frame and the last -- the knob, the track when
    /// widened, and how much of both is seen -- and its solids: a row whose
    /// slice of the bar changed is dirty (`Bar::slice`).
    bar: Option<Bar>,
    last_bar: Option<Bar>,
    bar_inst: Vec<Instance>,
    /// The pixels past the grid the frame keeps for a legacy scroller.
    gutter: Px,
    /// The cursor trail's ghost on this frame and the last, and its one
    /// solid instance: a row the ghost is over or was over is dirty, the
    /// scrollbar's rule, so the trail's damage stays bounded to the rows it
    /// sweeps rather than wiping the frame the way sfte's does.
    trail: Option<Ghost>,
    last_trail: Option<Ghost>,
    trail_inst: Vec<Instance>,
    /// The alternate-screen fade's veil on this frame and the last, and its
    /// one full-screen solid: while it fades every row is dirty, because the
    /// veil covers the whole screen and the whole screen is what changed.
    fade: Option<Veil>,
    last_fade: Option<Veil>,
    fade_inst: Vec<Instance>,
    /// This frame's blink phase, set by [`Renderer::set_blink`] before a
    /// convert or resolve, and whether any blinking cell was seen -- so the
    /// driver repaints on a phase flip only when something actually blinks.
    blink: Blink,
    blink_seen: Blinking,
    /// The visible image placements this frame, screen rects in device
    /// pixels, computed once by [`Renderer::set_images`] and consumed by
    /// both the CPU `paint_images` and the two GPU arms -- one description
    /// of where an image goes, not three.
    image_places: Vec<ImagePlacement>,
    /// The placements the last CPU frame drew, so the next one repaints the
    /// rows an image left as well as the rows it is on.
    last_image_places: Vec<ImagePlacement>,
    /// Each referenced image's pixels, premultiplied, for a GPU to upload
    /// once. Keyed by id and store epoch; the CPU path reads the term's
    /// pixels directly instead. **Kept across frames** and refreshed only for a
    /// new `(id, epoch)`, so a static image is snapshotted once rather than
    /// rebuilt every frame -- a large image's per-frame allocation and
    /// per-pixel conversion, which the frame path would otherwise pay while it
    /// is on screen. Cleared whole when the store epoch moves. See
    /// [`Renderer::set_images`].
    image_uploads: Vec<ImageUpload>,
    /// The store epoch [`Renderer::image_uploads`] is snapshotted at; a change
    /// drops the whole snapshot cache, the [`baud_domain::ImageGen`] rule.
    image_epoch: ImageGen,
    /// How many image pixel-snapshots have been built, cumulative -- a static
    /// image bumps it once, not once a frame, which is what the gate checks.
    image_snapshots: Tally,
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
        let defaults = (theme.fg, theme.bg);
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
            video: Video::Normal,
            defaults,
            blend,
            cache: RunCache::new(),
            atlas: Atlas::new(),
            cols: Cols::of_index(0),
            rows: Rows::of_index(0),
            prev: Vec::new(),
            known: Prev::Unknown,
            cur: Vec::new(),
            pictures: Pictures::new(),
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
            gutter: Px::of(0),
            bar_inst: Vec::new(),
            trail: None,
            last_trail: None,
            trail_inst: Vec::new(),
            fade: None,
            last_fade: None,
            fade_inst: Vec::new(),
            blink: Blink::default(),
            blink_seen: Blinking::default(),
            image_places: Vec::new(),
            last_image_places: Vec::new(),
            image_uploads: Vec::new(),
            image_epoch: ImageGen::ZERO,
            image_snapshots: Tally::of(0),
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

    /// The scrollbar for the next frame, or none: where the window sits,
    /// how much of the thumb is seen and whether the pointer widened it,
    /// each the driver's. Its solids are the knob and, widened, the track,
    /// in the default foreground the screen draws -- the theme's, or
    /// `OSC 10`'s, or under `?5` the background -- and every path draws them -- the CPU and
    /// instance paths from the instance list, the cell path from
    /// [`Renderer::scrollbar`] appended to its records.
    pub fn set_scrollbar(&mut self, scroll: Option<Scroll>) {
        let size = self.frame_size();
        self.bar = scroll.and_then(|s| scrollbar::bar(s, size, self.metrics.cell_h));
        self.bar_inst.clear();
        if let Some(b) = self.bar {
            self.bar_inst
                .extend(scrollbar::instances(b, self.defaults.0));
        }
    }

    /// The scrollbar's instances, for a path that draws records of its own.
    pub fn scrollbar(&self) -> &[Instance] {
        &self.bar_inst
    }

    /// The cursor trail for the next frame, or none: a fading ghost
    /// sweeping from where the cursor was to where it is. Like the
    /// scrollbar its instance is one translucent solid in the default
    /// foreground the screen draws, and every path draws it -- the CPU and instance paths from
    /// the instance list, the cell path from [`Renderer::trail`] appended
    /// to its records. The driver hands a `progress` it computed from the
    /// display link's timestamp and this only interpolates, so the frame is
    /// a pure function of the value and the pixel oracle can compare two
    /// GPU arms on it.
    pub fn set_trail(&mut self, trail: Option<Trail>) {
        self.trail = trail.map(|t| trail::ghost(t, self.metrics.cell_w, self.metrics.cell_h));
        self.trail_inst.clear();
        if let Some(g) = self.trail {
            self.trail_inst.push(trail::instance(g, self.defaults.0));
        }
    }

    /// The cursor trail's instances, for a path that draws records of its
    /// own -- drawn after the cells and under the scrollbar.
    pub fn trail(&self) -> &[Instance] {
        &self.trail_inst
    }

    /// The alternate-screen fade for the next frame, or none: one
    /// full-screen translucent solid in the default background the screen
    /// draws -- `?5` and `OSC 11` included -- that every
    /// path draws on top of everything else, so the new screen appears out
    /// of the background. The driver hands a `progress` it computed from the
    /// display link's timestamp; this only interpolates, so the pixel oracle
    /// can compare two GPU arms on it.
    pub fn set_screen_fade(&mut self, fade: Option<Transition>) {
        let (w, h) = self.pixel_size();
        self.fade = fade.map(|t| transition::veil(t, w, h));
        self.fade_inst.clear();
        if let Some(v) = self.fade {
            self.fade_inst
                .push(transition::instance(v, self.defaults.1));
        }
    }

    /// The screen fade's instances, for a path that draws records of its
    /// own -- drawn last, over the cells, the trail and the scrollbar.
    pub fn screen_fade(&self) -> &[Instance] {
        &self.fade_inst
    }

    pub fn set_blink(&mut self, blink: Blink) {
        self.blink = blink;
    }

    /// Replace the palette every rendition resolves against.
    ///
    /// **Every cell's colour changes, so the damage cache has to go**: row
    /// damage compares a row's cell words and the digest of the renditions
    /// they resolve to, and a theme swap moves neither -- the words are the
    /// same and a `StyleId` still indexes the same slot. Without
    /// [`Prev::Unknown`] a light/dark flip would repaint nothing at all,
    /// which is the same shape as the blink phase needing a forced repaint
    /// because a blink changes no cell content either.
    ///
    /// The scrollbar, the trail and the fade cache the defaults at the
    /// moment they were set, so they are rebuilt here rather than left
    /// drawing the old colours until they are next set. The `OSC 10`/`11`
    /// half of the defaults is the terminal's, which this cannot read, so
    /// the next convert or resolve puts it back (`refresh_screen`).
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.known = Prev::Unknown;
        self.defaults = self.theme.defaults(self.video, |_| None);
        self.rebuild_overlays();
    }

    /// Reads the screen's own colours for this frame -- `?5` and the
    /// `OSC 10`/`11` defaults -- and, when they moved, rebuilds the solids
    /// already built in the old ones. **They are set before a convert**, so
    /// without the rebuild the frame a program flips `?5` on would draw its
    /// scrollbar, trail and fade in the colours of the frame before.
    fn refresh_screen(&mut self, term: &Term<'_>) {
        self.video = video_of(term);
        let defaults = self.theme.defaults(self.video, |d| term.dynamic_color(d));
        if defaults != self.defaults {
            self.defaults = defaults;
            self.rebuild_overlays();
        }
    }

    /// The scrollbar, the trail and the fade, rebuilt in the defaults in
    /// force from the geometry each was last set with.
    fn rebuild_overlays(&mut self) {
        self.bar_inst.clear();
        if let Some(b) = self.bar {
            self.bar_inst
                .extend(scrollbar::instances(b, self.defaults.0));
        }
        self.trail_inst.clear();
        if let Some(g) = self.trail {
            self.trail_inst.push(trail::instance(g, self.defaults.0));
        }
        self.fade_inst.clear();
        if let Some(v) = self.fade {
            self.fade_inst
                .push(transition::instance(v, self.defaults.1));
        }
    }

    /// The theme in force, so a driver can tell the core what the defaults
    /// draw as (`Term::set_default_colors`) from the one place that knows.
    pub fn theme(&self) -> &Theme {
        &self.theme
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
    /// what a default cell resolves to, as of the last convert or resolve:
    /// the theme's background, or `OSC 11`'s, or under `?5` the foreground,
    /// at the window's alpha. It was the theme's whatever `OSC 11` said, so a
    /// program that moved the background got a frame of the old colour.
    pub fn clear_color(&self) -> Argb {
        self.defaults.1.with_alpha(self.theme.bg_alpha)
    }

    /// [`Renderer::clear_color`] as the next convert of `term` will answer
    /// it, for a driver that fills a margin the renderer never paints
    /// before it renders -- the Linux frame is the window's size, and the
    /// strips past the grid are filled once, so a `?5` or an `OSC 11` would
    /// leave them the old colour.
    pub fn clear_color_of(&self, term: &Term<'_>) -> Argb {
        let video = video_of(term);
        let (_, bg) = self.theme.defaults(video, |d| term.dynamic_color(d));
        bg.with_alpha(self.theme.bg_alpha)
    }

    /// The pixels past the grid's right edge the frame keeps for the
    /// scroller: [`crate::gutter`] of the style in force, which the driver
    /// sets where it lays the grid out. A change repaints every row, since
    /// the frame is a different width.
    pub fn set_gutter(&mut self, gutter: Px) {
        if gutter != self.gutter {
            self.gutter = gutter;
            self.known = Prev::Unknown;
        }
    }

    /// The pixel size of the frame: the grid, and the scroller's gutter
    /// beside it. What a presenter letterboxes, what a CPU target is, and
    /// what the scrollbar is drawn and hit-tested against; the cells are
    /// [`Renderer::pixel_size`].
    pub fn frame_size(&self) -> (Px, Px) {
        let (w, h) = self.pixel_size();
        (Px::of(w.value().saturating_add(self.gutter.value())), h)
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
        self.render_at(term, Rows::ZERO, target, damage)
    }

    /// [`Renderer::render`] of a window `back` lines up the history: the
    /// rows [`Term::view`] shows, and the images on them. What a driver
    /// that scrolls the CPU path back calls, so the text moves with the
    /// thumb, the selection and the link under a click.
    pub fn render_at(
        &mut self,
        term: &mut Term<'_>,
        back: Rows,
        target: &mut Target<'_>,
        damage: Damage,
    ) -> Frame {
        // **Images draw on every frame, and the rows one left are repainted.**
        // They drew only on a full frame, and a live terminal renders
        // incrementally after its first, so on the Linux driver, which is
        // CPU-only, and on the macOS CPU path, a Sixel a program printed did
        // not appear until a resize. Blitting every frame is sound because
        // the blit is idempotent -- an image pixel is opaque or clear, so
        // blitting it again over the pixel it left changes nothing -- and the
        // one hazard is an image never erased where it moved from. So the
        // placements are computed first, and the rows under a placement the
        // last CPU frame blitted and this one does not are repainted; a still
        // image's rows are not, and a new one's need not be, since its blit
        // lands on the cells. A format with partial alpha would need its rows
        // repainted every frame, and the core produces none.
        self.fit(term);
        self.place_images(term, back, Snapshots::Skipped);
        let converted = self.convert_with(term, back, damage, ImageRows::Repainted);
        let frame = self.paint(target, converted);
        self.paint_images(term, target);
        frame
    }

    /// Computes where every visible image placement lands this frame, into
    /// [`Renderer::image_placements`], and snapshots each referenced image's
    /// pixels into [`Renderer::image_uploads`] for a GPU to upload once.
    ///
    /// A placement anchors to an absolute line of its screen; its screen row
    /// is `line - top`, where [`Term::image_origin`] gives `top` for a view
    /// `back` lines up with the same clamp [`Term::view`] uses -- so as output
    /// scrolls the image moves up with its line, a scrollback view shows the
    /// images anchored there, and an image and the grid row under it are drawn
    /// from one answer. A placement on the screen that is not up is not
    /// drawn: the shell's image is not the editor's. The pixel origin is the cell `(row, col)` at the cell
    /// metrics, and the image draws one image-pixel per device pixel, its
    /// natural size. Placements fully off the screen are skipped; a
    /// partly-scrolled one keeps a signed screen origin and is clipped by the
    /// blit (CPU) or the rasteriser (GPU).
    ///
    /// This is the one place the placement-to-screen arithmetic is written:
    /// the CPU `paint_images` and the two GPU arms all read the list, so three
    /// paths cannot disagree about where an image goes -- which the pixel
    /// oracle could not otherwise tell from a blit bug.
    ///
    /// Order is placement order, drawn over the text. Z-ordering -- a negative
    /// `z` below the glyphs, ties broken by image id -- is the follow-on;
    /// nothing in the corpus places two overlapping images.
    pub fn set_images(&mut self, term: &Term<'_>, back: Rows) {
        self.place_images(term, back, Snapshots::Kept);
    }

    /// [`Renderer::set_images`], with or without the upload snapshots: the
    /// CPU path blits from the terminal's pixels and skips them, which kept a
    /// second converted copy of every visible image -- two megabytes for a
    /// 600x800 preview -- for a GPU that path does not have.
    fn place_images(&mut self, term: &Term<'_>, back: Rows, snap: Snapshots) {
        self.image_places.clear();
        let imgs = term.images();
        let epoch = imgs.generation();
        // The snapshot cache is kept across frames and dropped whole only when
        // the store epoch moves -- a `store`, `free` or `reset` -- so a static
        // image on screen is not re-snapshotted every frame. Within an epoch,
        // an id names the same pixels, so the `any(id)` check below is the
        // never-rebuild gate.
        if self.image_epoch != epoch {
            self.image_uploads.clear();
            self.image_epoch = epoch;
        }
        let origin = term.image_origin(back);
        let top = origin.top.value() as i64;
        let cw = self.metrics.cell_w.value() as i64;
        let ch = self.metrics.cell_h.value() as i64;
        let screen_h = self.rows.index() as i64 * ch;
        let clamp16 = |v: i64| v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16;
        for (_, p) in imgs.placements() {
            if p.screen != origin.screen {
                continue;
            }
            let Some(view) = imgs.image(p.image) else {
                continue;
            };
            let y0 = (p.line.value() as i64 - top) * ch;
            let x0 = p.col.index() as i64 * cw;
            // Fully above the top, or fully below the bottom: nothing to draw.
            // "The top" is one row above the window, not the window's edge:
            // under smooth scroll the cell path draws the leaving row there
            // and `draw_images` adds the same pan, so an image wholly inside
            // that row is on screen for the length of the slide. Culling at
            // zero dropped it, and it popped off at the slide's start rather
            // than sliding out. A frame that is not sliding draws it at a
            // negative y, which every path clips.
            if y0 + view.height.index() as i64 <= -ch || y0 >= screen_h {
                continue;
            }
            self.image_places.push(ImagePlacement {
                id: p.image,
                epoch,
                x: clamp16(x0),
                y: clamp16(y0),
                w: view.width.raw(),
                h: view.height.raw(),
            });
            // Snapshot the pixels once per referenced `(id, epoch)`: the GPU
            // reads this to upload, the CPU reads the term itself. A pixel that
            // resolves to RGB is opaque; a `DEFAULT` (unpainted Sixel) pixel is
            // `0x00000000`, transparent, which the fragment's premultiplied OVER
            // leaves the cell showing through.
            if snap == Snapshots::Kept
                && !self
                    .image_uploads
                    .iter()
                    .any(|u| u.id == p.image && u.epoch == epoch)
            {
                let pixels = view
                    .pixels
                    .iter()
                    .map(|cw| {
                        cw.as_rgb().map_or(0u32, |(r, g, b)| {
                            0xFF00_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
                        })
                    })
                    .collect();
                self.image_uploads.push(ImageUpload {
                    id: p.image,
                    epoch,
                    w: view.width.raw(),
                    h: view.height.raw(),
                    pixels,
                });
                self.image_snapshots = self.image_snapshots.inc();
            }
        }
        // Drop snapshots of images no longer on screen, so an image scrolled off
        // the top does not hold its pixels until the next epoch change -- which
        // on a terminal showing one image is forever. The clear-every-frame
        // first version avoided this by accident; keeping the cache across
        // frames reintroduces it unless the leavers are pruned. An image that
        // oscillates on and off screen re-snapshots on each re-entry, which is
        // the price of not holding it, and far cheaper than a rebuild a frame.
        if snap == Snapshots::Kept {
            let places = &self.image_places;
            self.image_uploads
                .retain(|u| places.iter().any(|p| p.id == u.id));
        }
    }

    /// The visible image placements this frame, in device-pixel screen rects.
    pub fn image_placements(&self) -> &[ImagePlacement] {
        &self.image_places
    }

    /// Each referenced image's premultiplied pixels, for a GPU to upload once.
    pub fn image_uploads(&self) -> &[ImageUpload] {
        &self.image_uploads
    }

    /// How many image pixel-snapshots have been built over this renderer's
    /// life. A static image on screen bumps it once, not once a frame, which is
    /// how the snapshot-cache gate distinguishes "built once" from "rebuilt".
    pub fn image_snapshots(&self) -> Tally {
        self.image_snapshots
    }

    /// Blits every image placement over the frame, after the glyphs, reading
    /// the list [`Renderer::set_images`] built and the term's own pixels.
    fn paint_images(&self, term: &Term<'_>, target: &mut Target<'_>) {
        let imgs = term.images();
        for p in &self.image_places {
            let Some(view) = imgs.image(p.id) else {
                continue;
            };
            target.blit_image((i32::from(p.x), i32::from(p.y)), &view, self.blend);
        }
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
        self.convert_with(term, back, damage, ImageRows::Ignored)
    }

    fn convert_with(
        &mut self,
        term: &Term<'_>,
        back: Rows,
        damage: Damage,
        images: ImageRows,
    ) -> Converted {
        let t0 = Instant::now();
        self.blink_seen = Blinking::Unseen;
        self.fit(term);
        self.refresh_screen(term);
        let (cols, rows) = term.geometry();
        let ncols = cols.index();
        // Each row read where the core holds it, through the rotation and
        // the ring, `back` lines into the history when the user is looking
        // back; the copy into `cur` is this path's own frame-to-frame damage
        // record, not the core's, and a change of `back` dirties the rows it
        // moves the way any other change does. The window is the core's
        // clamp of `back` -- what `Term::view` draws -- so the cursor below
        // agrees with it: on the alternate screen, which keeps no history, a
        // window left scrolled back on the primary is the live screen, and
        // the program that owns it owns the cursor.
        let back = term.looking_back(back);
        let vp = term.view(back);
        for r in 0..rows.index() {
            self.cur[r * ncols..(r + 1) * ncols].copy_from_slice(vp.row(Row::from_index(r)));
        }
        if self.theme.finds_pictures() {
            self.pictures.find(term, &vp);
        } else {
            self.pictures.clear(cols, rows);
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
        // The screen fade veils every row, so a change to it -- spawned,
        // stepped or cleared -- makes the whole frame dirty. It is not a
        // per-row predicate like the trail's band overlap.
        let full = damage == Damage::Full
            || self.known == Prev::Unknown
            || palette != self.last_palette
            || self.fade != self.last_fade;
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
            let slice = |b: Option<Bar>| b.and_then(|b| b.slice(band.top, band.bottom));
            let bar_moved = slice(self.bar) != slice(self.last_bar);
            let under_trail =
                |g: Option<Ghost>| g.is_some_and(|g| g.overlaps(band.top, band.bottom));
            let trail_moved = under_trail(self.trail) != under_trail(self.last_trail)
                || (under_trail(self.trail) && self.trail != self.last_trail);
            // A picture is the block's, so a row can move in or out of one
            // without a cell of its own changing.
            let pictured = self.pictures.row_changed(row);
            // An image the CPU path blitted over this row last frame is not
            // there now: see `render_at`.
            let imaged = images == ImageRows::Repainted
                && left_from(&self.image_places, &self.last_image_places, band);
            self.dirty[r] = full
                || changed
                || cursor_moved
                || selection_moved
                || bar_moved
                || trail_moved
                || pictured
                || imaged;
        }
        if images == ImageRows::Repainted {
            self.last_image_places.clone_from(&self.image_places);
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
        self.last_trail = self.trail;
        self.last_fade = self.fade;
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
            let margin = self.clear_color();
            target.fill_band(
                band,
                &self.bg[self.row_cells(row)],
                self.metrics.cell_w,
                margin,
            );
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
        self.per_row
            .iter()
            .flatten()
            .chain(self.trail_inst.iter())
            .chain(self.bar_inst.iter())
            .chain(self.fade_inst.iter())
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
        if crate::boxes::covers(base) {
            // A box, block or braille cell is baud's own mask, whatever is
            // attached to it: the shaper is not consulted.
            return vec![ClusterGlyph {
                id: GlyphId::from_shaper(0),
                slot: self.procedural(base),
                dx: 0,
                dy: 0,
            }];
        }
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
        if crate::boxes::covers(cp) {
            // `PairRole::Neither` is load-bearing: with both pair bits clear
            // the cell is never a ligature candidate, never gathered into a
            // window and never re-shaped by a font, so the face field is
            // never read for it.
            return Some(Single {
                slot: self.procedural(cp),
                glyph: GlyphId::from_shaper(0),
                role: PairRole::Neither,
                face: FaceIx::PRIMARY,
            });
        }
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
        self.refresh_screen(term);
        let table = term.styles();
        let mut n = Tally::default();
        for (slot, dst) in table.iter().zip(out.iter_mut()) {
            let style = slot.unwrap_or(baud_domain::StyleWord::DEFAULT);
            let (mut fg, mut bg) = self.theme.resolve(
                style,
                self.video,
                |ch| term.palette(ch),
                |d| term.dynamic_color(d),
            );
            bg = bg.with_alpha(self.theme.bg_alpha);
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
            if self.theme.opaque_bg(style) {
                shapes = shapes.with(Shapes::OPAQUE_BG);
            }
            // Any underline style asks for the base line's colour plumbing;
            // which line is drawn is the style's own bit below. The three colon
            // styles *replace* the straight line rather than adding to it, so
            // the straight-line bit is set only for the two that draw one.
            let styled = attrs.has(Attrs::UNDERLINES);
            let straight = attrs.has(Attrs::UNDERLINE) || attrs.has(Attrs::DOUBLE_UNDERLINE);
            if !blink_hidden && straight {
                shapes = shapes.with(Shapes::UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::DOUBLE_UNDERLINE) {
                shapes = shapes.with(Shapes::DOUBLE_UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::CURLY_UNDERLINE) {
                shapes = shapes.with(Shapes::CURLY_UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::DOTTED_UNDERLINE) {
                shapes = shapes.with(Shapes::DOTTED_UNDERLINE);
            }
            if !blink_hidden && attrs.has(Attrs::DASHED_UNDERLINE) {
                shapes = shapes.with(Shapes::DASHED_UNDERLINE);
            }
            let _ = styled;
            if !blink_hidden && attrs.has(Attrs::STRIKE) {
                shapes = shapes.with(Shapes::STRIKE);
            }
            let underline = self
                .theme
                .underline(style, self.video, fg, |ch| term.palette(ch));
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
        // `OSC 12` cursor colour, if the application set one. Read once for the
        // row -- it is terminal-global -- and used where the cursor draws: the
        // block cursor fills with it (rather than the reverse-video swap) and
        // the bar or underline cursor's shape takes it.
        let cursor_color = term
            .dynamic_color(DynamicColor::Cursor)
            .map(|(r, g, b)| Argb::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b)));
        for (c, cell) in row.iter().enumerate() {
            let style = term.style_of(*cell);
            let (mut fg, mut bg) = self.theme.resolve(
                style,
                self.video,
                |ch| term.palette(ch),
                |d| term.dynamic_color(d),
            );
            // **Every background is translucent, not only the default
            // one.** The rendering register's item 6 asked whether alpha is
            // a *window* property (the default background only) or a
            // *rendition* property (any SGR background too), and the owner
            // chose the second on 2026-09-20. It is visible rather than
            // theoretical: 72% of `btop.raw`'s final screen and 100% of
            // `asciiquarium.raw`'s carry a background the application set,
            // so under the other answer those screens were opaque where a
            // shell was not. Every other recording in the corpus is 0%.
            //
            // The margin keeps the same alpha through `clear_color`, and
            // both background passes *replace* rather than blend -- the CPU
            // one fills, the GPU one has blending disabled -- so a cell's
            // alpha reaches the compositor once rather than compounding
            // with the clear under it.
            bg = bg.with_alpha(self.theme.bg_alpha);
            // A picture drawn in text keeps its backgrounds solid under the
            // dim: its glyphs are solid, and a dimmed background beside
            // each one splits the picture into slivers and holes. Must match
            // the cell shader's `cell_bg_vertex`.
            if self.theme.opaque_bg(style) || self.pictures.at(r, Col::at_index(c)) == Pictured::Yes
            {
                bg = bg.with_alpha(Chan::from_u8(0xFF));
            }
            // A wide tail follows its lead, so a block's edge never cuts a
            // character in two: `Highlight::selects`.
            if self
                .highlight
                .is_some_and(|h| h.selects(r, Col::at_index(c), cell.content))
            {
                // Opaque on both sides of the swap. `bg` becomes the old
                // foreground, which is the block a reader has to see; `fg`
                // becomes the old *background*, which now carries the dim,
                // and a glyph drawn at 0.22 alpha over its own selection
                // block is barely there. A foreground is never translucent.
                core::mem::swap(&mut fg, &mut bg);
                bg = bg.with_alpha(Chan::from_u8(0xFF));
                fg = fg.with_alpha(Chan::from_u8(0xFF));
            }
            let here = cursor.filter(|&(cr, cc, _)| cr == r && cc == Col::at_index(c));
            if matches!(
                here,
                Some((_, _, CursorStyle::BlinkingBlock | CursorStyle::SteadyBlock))
            ) {
                core::mem::swap(&mut fg, &mut bg);
                bg = bg.with_alpha(Chan::from_u8(0xFF));
                fg = fg.with_alpha(Chan::from_u8(0xFF));
                // `OSC 12`: the block fills with the application's cursor
                // colour instead of the swapped foreground. The glyph under
                // it stays `fg` (the cell's own background), so it reads
                // against whatever colour was chosen.
                if let Some(cc) = cursor_color {
                    bg = cc;
                }
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
            // The three colon styles, `[[source: kitty-underlines]]`, drawn as
            // runs of solid quads so they need no texel and composite exactly
            // as the straight line does. Amplitude and period are the line's
            // thickness, which is what keeps them legible at every size.
            if !blink_hidden
                && (attrs.has(Attrs::CURLY_UNDERLINE)
                    || attrs.has(Attrs::DOTTED_UNDERLINE)
                    || attrs.has(Attrs::DASHED_UNDERLINE))
            {
                let color = self
                    .theme
                    .underline(style, self.video, fg, |ch| term.palette(ch));
                let ul = px_i32(self.metrics.underline);
                let th = px_i32(self.metrics.thickness).max(1);
                let w = px_i32(self.metrics.cell_w);
                let low = px_i32(self.metrics.cell_h) - th;
                let seg = Px::of(u32::try_from(th).unwrap_or(1));
                let mut dx = 0;
                let mut k = 0;
                while dx < w {
                    let (on, dy) = if attrs.has(Attrs::CURLY_UNDERLINE) {
                        // A zigzag: up for a thickness, down for a thickness.
                        (true, if k % 2 == 0 { 0 } else { th })
                    } else if attrs.has(Attrs::DOTTED_UNDERLINE) {
                        (k % 2 == 0, 0)
                    } else {
                        (k % 3 != 2, 0)
                    };
                    if on {
                        let y = (ul + dy).min(low.max(ul));
                        shapes.push(solid(
                            screen(x + dx),
                            screen(top + y),
                            seg,
                            self.metrics.thickness,
                            color,
                        ));
                    }
                    dx += th;
                    k += 1;
                }
            }
            if !blink_hidden && (attrs.has(Attrs::UNDERLINE) || double) {
                let color = self
                    .theme
                    .underline(style, self.video, fg, |ch| term.palette(ch));
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
            // The bar or underline cursor draws in the application's cursor
            // colour (`OSC 12`) if it set one, else the cell's foreground.
            let cur = cursor_color.unwrap_or(fg);
            match here {
                Some((_, _, CursorStyle::BlinkingUnderline | CursorStyle::SteadyUnderline)) => {
                    let h = Px::of(self.metrics.thickness.value().max(2));
                    let y = top + px_i32(self.metrics.cell_h) - px_i32(h);
                    shapes.push(solid(screen(x), screen(y), self.metrics.cell_w, h, cur));
                }
                Some((_, _, CursorStyle::BlinkingBar | CursorStyle::SteadyBar)) => {
                    let w = Px::of(self.metrics.thickness.value().max(2));
                    shapes.push(solid(screen(x), screen(top), w, self.metrics.cell_h, cur));
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
            if crate::boxes::covers(base) {
                // Box drawing, a block element or braille: baud's own mask
                // at the cell origin, never shaped, never in a run.
                let width = if row[c].flags.is_wide() {
                    2usize
                } else {
                    1usize
                };
                if let Some(slot) = self.procedural(base) {
                    self.emit_at(r, Col::at_index(c), slot, &mut instances);
                }
                c += width;
                continue;
            }
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
                if crate::boxes::covers(cp) || self.stack.select(cp) != face {
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

    /// The atlas mask for a procedural codepoint at this cell size.
    fn procedural(&mut self, cp: Codepoint) -> Option<crate::atlas::Slot> {
        let cell = crate::boxes::Cell {
            w: self.metrics.cell_w,
            h: self.metrics.cell_h,
        };
        self.atlas.synthetic(cp, cell, self.metrics.baseline)
    }

    /// One instance for a slot placed at the origin of cell `col` of row
    /// `r`, in the cell's foreground: the procedural glyphs' emitter.
    fn emit_at(&mut self, r: Row, col: Col, slot: crate::atlas::Slot, out: &mut Vec<Instance>) {
        let cell_w = i32::try_from(self.metrics.cell_w.value()).unwrap_or(0);
        let baseline = i32::try_from(self.metrics.baseline.value()).unwrap_or(0);
        let top = i32::try_from(self.band(r).top.value()).unwrap_or(0);
        let range = self.row_cells(r);
        let color = self
            .fg
            .get(range.start + col.index())
            .copied()
            .unwrap_or(self.defaults.0);
        let x = i32::try_from(col.index()).unwrap_or(0) * cell_w + slot.left;
        let y = top + baseline + slot.top;
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
                .unwrap_or(self.defaults.0);
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
