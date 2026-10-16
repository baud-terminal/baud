//! Image storage for Sixel and Kitty graphics: a caller-provided ARGB arena, a
//! bounded image-slot table and a bounded placement table, all
//! **refuse-and-count**. Pure, `no_std`, allocation-free -- the caller owns the
//! pixels, the slots and the placements exactly as it owns the grid and the
//! arena, and `docs/research/notes/t-images.md` is the design.
//!
//! An image is decoded **once, into the arena in place** (a Sixel into the
//! gap it claims as its body arrives, [`SixelStream`], or a [`SixelCanvas`]
//! over the slice [`ImageStore::store`] returns), which is the
//! zero-extra-copy path the register describes: the pixels land where a
//! renderer will read them rather than in a scratch that is then copied. A
//! Sixel is moved once, at its terminator, when a gap lower than its claim
//! holds it, so the store lays images out as `store` would. The storage carries
//! [`Pixel`]s, straight ARGB, and never palette indices, decided in the
//! register on the shader-sampling ground -- a palette index in the cell
//! shader is a second dependent texture fetch.
//!
//! **Alpha is stored straight**, the colour not multiplied by its opacity:
//! "The colour values in a pixel are not premultiplied by the alpha value
//! assigned to the pixel" (`[[source: w3c-png-2003]]` 6.2), and kitty's
//! `f=32` is "32-bit RGBA" stored "directly as ... 4 bytes per pixel"
//! (`[[source: kitty-graphics]]`, "RGB and RGBA data"), which says nothing
//! of premultiplying. A renderer premultiplies once, where it blends. Each
//! image is marked at decode as [`ImageAlpha::OpaqueOrClear`] -- every pixel
//! drawn whole or not at all, a Sixel's case -- or [`ImageAlpha::Partial`],
//! because only the first can be blitted again over its own last blit and
//! leave the pixel as it was.
//!
//! Three bounded structures, the [`ClusterStore`] pattern a fourth time --
//! **refuse rather than evict, and count the refusal**, because an evicted image
//! is a wrong picture on screen:
//!
//! * the pixel arena, a bump-or-reuse allocator over caller memory;
//! * the image-slot table, mapping a live [`ImageId`] to its span;
//! * the placement table, `{ image, line, column, z }`, screen-space with its
//!   own storage rather than a per-cell reference -- because `CellFlags` is full
//!   at sixteen bits with zero spare, and Kitty's own model is placements with a
//!   `z` index, which avoids the cell-bits problem by construction.
//!
//! An image is freed when its last placement is gone (or by an explicit
//! [`ImageStore::free`], the Kitty delete), which returns its span to the arena.
//! Reclaim is **inherent, not bookkept**: free space is the gaps between live
//! spans, recomputed per allocation (`ImageStore::lowest_gap`), so a free is a
//! slot cleared and nothing else -- baud's "compute the gaps rather than keep a
//! free list" instinct, and correct for every free order rather than only a
//! stack.

use crate::pos::LineIdx;
use crate::quantity::{Col, Count, Rows};
use crate::style::Chan;

/// **One image pixel, straight ARGB**: `0xAARRGGBB`, the colour not
/// multiplied by its alpha (`[[source: w3c-png-2003]]` 6.2). Zero is
/// [`Pixel::CLEAR`], which a Sixel leaves where its data sets no bit, so a
/// zeroed arena is a clear one. The word is the one a renderer's `Argb`
/// and a `BGRA8Unorm` texture read, so an image goes to either unconverted.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pixel(u32);

impl Pixel {
    /// Fully transparent: drawn as nothing.
    pub const CLEAR: Pixel = Pixel(0);

    /// A fully opaque pixel of `r`, `g`, `b`.
    pub fn opaque(r: Chan, g: Chan, b: Chan) -> Pixel {
        Pixel::straight(Chan::from_u8(0xFF), r, g, b)
    }

    /// A pixel of `r`, `g`, `b` at opacity `a`, the colour not premultiplied.
    pub fn straight(a: Chan, r: Chan, g: Chan, b: Chan) -> Pixel {
        Pixel(
            u32::from(a.get()) << 24
                | u32::from(r.get()) << 16
                | u32::from(g.get()) << 8
                | u32::from(b.get()),
        )
    }

    /// The opacity: 0 clear, 255 opaque.
    pub fn alpha(self) -> Chan {
        Chan::from_u8((self.0 >> 24) as u8)
    }

    /// The straight colour.
    pub fn rgb(self) -> (Chan, Chan, Chan) {
        (
            Chan::from_u8((self.0 >> 16) as u8),
            Chan::from_u8((self.0 >> 8) as u8),
            Chan::from_u8(self.0 as u8),
        )
    }

    /// The word, `0xAARRGGBB`.
    pub fn bits(self) -> u32 {
        self.0
    }
}

/// **How an image's pixels blend**, marked once at decode: what decides
/// whether a renderer that keeps its last frame may blit the image again
/// over its own last blit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageAlpha {
    /// Every pixel opaque or clear: a blit over the pixel the last one left
    /// changes nothing, so the rows under a still image need no repaint. A
    /// Sixel is always this.
    OpaqueOrClear,
    /// Some pixel partly transparent: a second blit over the first blends
    /// it twice, so the rows under it are repainted on every frame it is on.
    Partial,
}

impl ImageAlpha {
    /// What `pixels` hold: [`ImageAlpha::Partial`] when any alpha lies
    /// strictly between clear and opaque. For a decoder that does not know
    /// from its format.
    pub fn of(pixels: &[Pixel]) -> ImageAlpha {
        match pixels.iter().any(|p| !matches!(p.alpha().get(), 0 | 0xFF)) {
            true => ImageAlpha::Partial,
            false => ImageAlpha::OpaqueOrClear,
        }
    }
}

/// A pixel dimension of a stored image, width or height. `u16` covers any image
/// a terminal draws -- 65535 pixels a side is far past a screen -- and the area
/// is a `usize` so the arena index cannot overflow the type it is stored at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ImagePx(u16);

impl ImagePx {
    /// The empty dimension; an image slot at width [`ImagePx::ZERO`] is free.
    pub const ZERO: ImagePx = ImagePx(0);

    /// A dimension of `n` pixels.
    pub fn of(n: u16) -> ImagePx {
        ImagePx(n)
    }

    /// The raw pixel count.
    pub fn raw(self) -> u16 {
        self.0
    }

    /// As an index or a length.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// The extent of `cells` cells of this size, **saturating** at the
    /// widest dimension a report or a `winsize` can carry.
    ///
    /// Saturating rather than wrapping: 2048 columns of a 72-pixel cell is
    /// 147,456 pixels, which a `u16` cannot hold, and a wrapped answer names
    /// a small text area that does not exist where a saturated one says only
    /// "at least this big".
    pub fn span(self, cells: usize) -> ImagePx {
        ImagePx(u16::try_from(self.index().saturating_mul(cells)).unwrap_or(u16::MAX))
    }
}

/// A signed draw-order index, the Kitty `z` (`[[source: kitty-graphics]]`).
/// Negative draws below text; ties are broken by image id elsewhere. It is
/// stored on a placement and read by the renderer, never on the ingest path.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ZIndex(i32);

impl ZIndex {
    /// Above text, the default for a placement that names no `z`.
    pub const ZERO: ZIndex = ZIndex(0);

    /// A draw order of `n`.
    pub fn of(n: i32) -> ZIndex {
        ZIndex(n)
    }

    /// The raw index, for the renderer's ordering.
    pub fn raw(self) -> i32 {
        self.0
    }
}

/// A handle to a stored image. `NONE` is zero, so a zeroed slot names no image;
/// a live handle is [`ImageId::from_index`] of the slot it occupies, the
/// `ClusterId`/`LinkId` shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageId(u32);

impl ImageId {
    /// Names no image.
    pub const NONE: ImageId = ImageId(0);

    fn from_index(i: usize) -> ImageId {
        ImageId((i as u32).wrapping_add(1))
    }

    fn index(self) -> Option<usize> {
        usize::try_from(self.0).ok()?.checked_sub(1)
    }

    /// Whether this handle names no image.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A monotonic epoch bumped whenever the set of stored images changes -- a
/// `store`, a `free`, an `unplace` that drops an image, or a `reset`.
///
/// A GPU texture cache keyed on `(generation, id)` drops stale textures
/// without a per-image generation: an image slot reused after a `reset` (RIS)
/// lands under a new epoch, so the cache cannot return the old texture for a
/// reused id. Per-slot generations -- so adding one image does not invalidate
/// the textures of the others -- are the follow-on if images ever churn per
/// frame; the corpus stores an image a handful of times a session, so the
/// coarse epoch re-uploads nothing that is hot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageGen(u32);

impl ImageGen {
    /// The epoch of a store that has held nothing.
    pub const ZERO: ImageGen = ImageGen(0);

    /// The raw counter, for a cache key.
    pub fn raw(self) -> u32 {
        self.0
    }

    fn bumped(self) -> ImageGen {
        ImageGen(self.0.wrapping_add(1))
    }
}

/// A handle to a placement. `NONE` is zero, the [`ImageId`] shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlacementId(u32);

impl PlacementId {
    /// Names no placement.
    pub const NONE: PlacementId = PlacementId(0);

    fn from_index(i: usize) -> PlacementId {
        PlacementId((i as u32).wrapping_add(1))
    }

    /// Whether this handle names no placement.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// One image-slot: where a live image's pixels are in the arena, its size, and
/// how many placements reference it. Empty when [`ImageSlot::width`] is
/// [`ImagePx::ZERO`] -- a zero-width image is meaningless, the `ClusterSlot`
/// `len == 0` precedent, so no `bool` is needed to say a slot is free.
#[derive(Clone, Copy)]
pub struct ImageSlot {
    offset: u32,
    width: ImagePx,
    height: ImagePx,
    /// How its pixels blend, marked when it was stored.
    alpha: ImageAlpha,
    /// The reference count: how many live placements name this image. An image
    /// is freed when this reaches zero after having been placed.
    placements: Count,
}

impl ImageSlot {
    /// A free slot.
    pub const EMPTY: ImageSlot = ImageSlot {
        offset: 0,
        width: ImagePx::ZERO,
        height: ImagePx::ZERO,
        alpha: ImageAlpha::OpaqueOrClear,
        placements: Count::ZERO,
    };

    fn is_empty(self) -> bool {
        self.width.is_zero()
    }

    fn span_len(self) -> usize {
        self.width.index() * self.height.index()
    }

    fn start(self) -> usize {
        self.offset as usize
    }

    fn end(self) -> usize {
        self.start() + self.span_len()
    }
}

/// **Which screen a placement is on**, because the two number their lines
/// apart. The primary screen's lines run on through its history; the
/// alternate screen keeps no history and its lines are the editor's, gone
/// when the editor leaves. A placement is drawn only while its screen is up
/// -- the shell's image is not drawn over the editor -- and the alternate
/// screen's are deleted when it is left, which is foot's rule and ghostty's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageScreen {
    Primary,
    Alternate,
}

impl ImageScreen {
    fn slot(self) -> usize {
        match self {
            ImageScreen::Primary => 0,
            ImageScreen::Alternate => 1,
        }
    }
}

/// **Which way rows moved inside a band of the screen**: toward its top
/// line, as a region scrolls up or `DL` pulls rows in, or toward its bottom,
/// as a region scrolls down or `IL` pushes rows out. A type rather than a
/// `bool`, so a call site names the direction it means.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandMove {
    Up,
    Down,
}

/// **The lines a screen's placements lie between**, inclusive: a bound no
/// placement is outside, so a check whose rows miss it skips the scan. It is
/// widened by every placement and tightened by every scan; a placement
/// removed between scans leaves it wide, which costs a scan that finds
/// nothing and never a missed one.
///
/// It is what makes an image on screen cheap for the text after it. Every
/// glyph write asks whether it lands on an image, and every scroll asks
/// whether one's row just left: both are a scan of the placement table,
/// and with an image anywhere in the history -- where one stays for two
/// thousand lines -- both would run once a run and once a line. The rows
/// being written and scrolled are past the image, so the bound answers.
#[derive(Clone, Copy)]
struct Reach {
    first: LineIdx,
    last: LineIdx,
}

/// Where a view's top is in a screen's numbering: the screen, and the
/// absolute line at the view's top row. A placement on that screen is drawn
/// at screen row `line - top`; one on the other screen is not drawn. The
/// core answers this, with the same clamp its own view uses, so the grid row
/// and the image row cannot disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageOrigin {
    pub screen: ImageScreen,
    pub top: LineIdx,
}

/// A placement's progress through one reflow: not in one, or staged at its
/// row in the old layout, or mapped to its row in the new one. Rows count
/// from the oldest retained row, which a reflow does not move.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Staged {
    Idle,
    From(Rows),
    To(Rows, Col),
}

/// One placement: an image drawn at an absolute line and column with a draw
/// order. The anchor is an **absolute line** (`[[source: kitty-graphics]]` and
/// rio both anchor to the scrollback row), so output scrolling under it moves it
/// with its line; the scroll and view arithmetic is the renderer's, and this
/// only holds the anchor. Empty when [`PlacementSlot::image`] is
/// [`ImageId::NONE`].
#[derive(Clone, Copy)]
pub struct PlacementSlot {
    /// The image this places, or [`ImageId::NONE`] for a free slot.
    pub image: ImageId,
    /// The screen whose numbering [`PlacementSlot::line`] is in.
    pub screen: ImageScreen,
    /// The absolute line the anchor cell is on.
    pub line: LineIdx,
    /// The column the anchor cell is at.
    pub col: Col,
    /// The draw order; higher draws later, negative below text.
    pub z: ZIndex,
    /// Where in the anchor's row the cell the row began with when the image
    /// was placed now sits: zero until a narrower re-cut lands that cell
    /// part-way along a row that begins earlier in the line. What a reflow
    /// maps the anchor by, so a round trip through a narrower width brings
    /// an anchor on a continuation row back to its row
    /// (`an_anchor_on_a_wrapped_row_comes_back_on_its_row_after_a_round_trip`).
    follows: Col,
    staged: Staged,
}

impl PlacementSlot {
    /// A free slot.
    pub const EMPTY: PlacementSlot = PlacementSlot {
        image: ImageId::NONE,
        screen: ImageScreen::Primary,
        line: LineIdx::FIRST,
        col: Col::ZERO,
        z: ZIndex::ZERO,
        follows: Col::ZERO,
        staged: Staged::Idle,
    };

    fn is_empty(self) -> bool {
        self.image.is_none()
    }
}

/// How much caller memory an image store is given: the pixel arena in
/// [`Pixel`]s. A type rather than a loose size, so `baud-core`'s
/// provisioning method takes one rather than a raw `usize`, which its
/// signature budget forbids.
///
/// **It is the only limit on an image's size**, with the 65535 a coordinate
/// holds and the slot table. Until 2026-10-14 it carried a second, the bytes
/// of Sixel-body scratch a raw body was held in before it decoded; a Sixel
/// is decoded as it arrives since ([`SixelStream`]), and holds no body.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageBudget {
    /// [`Pixel`]s of arena.
    pub pixels: usize,
}

impl ImageBudget {
    /// A budget of `pixels` arena words.
    pub fn of(pixels: usize) -> ImageBudget {
        ImageBudget { pixels }
    }
}

/// Which limit refused an image, with the size that hit it -- what
/// [`ImageStore::refuse`] is told, and what [`ImageRefusals`] counts. Each
/// variant is a different fix: the arena is `[images] pixels_mb` in the
/// app, the slot table is a provisioning constant, and an image with no
/// pixels or past 65535 either way is no limit at all. **No refusal is for
/// a body's bytes**: a Sixel is decoded as it arrives ([`SixelStream`]),
/// and until 2026-10-14 a `Body` refusal counted a raw body past its
/// scratch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageRefusal {
    /// An image with no pixels: an empty Sixel, one whose data paints
    /// nothing, or a zero dimension handed to [`ImageStore::store`]. No limit
    /// refused it and none raised would draw it.
    Empty,
    /// Every image slot held a live image.
    Slot,
    /// No gap in the pixel arena held a `width` x `height` image -- larger
    /// than the whole arena, or larger than what the live images leave free.
    /// An unprovisioned store's empty arena is this for every image.
    Arena(ImagePx, ImagePx),
    /// An image wider or taller than 65535 pixels, the most an image
    /// coordinate holds: no limit refused it and none raised would draw it.
    TooLarge,
}

/// The image refusals split by the limit each hit, with the size the last one
/// of each carried. [`ImageStore::images_refused`] is their total, and stays
/// the one number for a caller that only asks how many: it mixed a body over
/// the scratch with an image over the arena, two different knobs -- yazi's
/// previews went blank on the first (a 600x375 screenshot was a 334 KiB body
/// against the app's 256 KiB scratch of 2026-10-09) while the arena held them.
/// The body scratch is gone since 2026-10-14, and with it that cause.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageRefusals {
    /// Images refused for having no pixels.
    pub empty: usize,
    /// Images refused because every image slot held a live image.
    pub slot: usize,
    /// Images refused because no gap in the pixel arena held them.
    pub arena: usize,
    /// The width of the last image refused for want of an arena gap.
    pub arena_width: ImagePx,
    /// Its height.
    pub arena_height: ImagePx,
    /// Images refused for being wider or taller than 65535 pixels.
    pub too_large: usize,
}

impl ImageRefusals {
    /// Nothing refused.
    pub const NONE: ImageRefusals = ImageRefusals {
        empty: 0,
        slot: 0,
        arena: 0,
        arena_width: ImagePx::ZERO,
        arena_height: ImagePx::ZERO,
        too_large: 0,
    };

    /// Every refusal, whatever its cause: what
    /// [`ImageStore::images_refused`] equals.
    pub fn total(&self) -> usize {
        self.empty + self.slot + self.arena + self.too_large
    }
}

/// A read view of a stored image: its pixels, its size and how it blends.
pub struct ImageView<'a> {
    /// The straight ARGB pixels, `width * height` of them, row-major.
    pub pixels: &'a [Pixel],
    /// The image width in pixels.
    pub width: ImagePx,
    /// The image height in pixels.
    pub height: ImagePx,
    /// Whether any pixel is partly transparent, marked when it was stored.
    pub alpha: ImageAlpha,
}

/// A bounded, caller-owned store of images and their placements.
///
/// The three caller buffers are the arena (straight ARGB pixels), the image-slot table
/// and the placement table; all three are refuse-and-count, and the counters are
/// public and never silent, the [`ClusterStore`] contract.
pub struct ImageStore<'b> {
    pixels: &'b mut [Pixel],
    slots: &'b mut [ImageSlot],
    placements: &'b mut [PlacementSlot],
    /// Images refused, whatever the cause. Never silent: a refused image is
    /// not drawn, and how often is a number. [`ImageStore::refusals`] says
    /// which limit each hit, and this is their total.
    pub images_refused: usize,
    /// The refusals by cause: the slot table, the arena, an image with no
    /// pixels, or one past 65535 either way. Every refusal in the tree goes through
    /// [`ImageStore::refuse`], which bumps `images_refused` with it, so the
    /// total and its causes agree.
    pub refusals: ImageRefusals,
    /// Placements that did not fit -- no free placement slot.
    pub placements_refused: usize,
    /// Images freed, by a last placement leaving or an explicit delete.
    pub images_dropped: usize,
    /// Placements removed because the row their anchor is on no longer
    /// exists: it left the retained history, a scroll blanked it as it came
    /// in, a reflow dropped its line, or its screen was the alternate one
    /// and was left. Never silent -- an image that vanished has a count.
    pub placements_expired: usize,
    /// Sixels that stored *and* explicitly asked for an opaque background
    /// (`P2` = 0 or 2) that baud renders transparent instead. baud draws
    /// unpainted pixels transparent -- the `P2` = 1 behaviour, and the more
    /// useful default, since an image then composites over the text -- and the
    /// opaque fill has no defensible pinned source for *what colour* to fill
    /// with, so the request is honoured as transparent and counted rather than
    /// filled with a guess. Counted at the terminator after the image stores,
    /// not when its parameters parse: a refused image drew nothing, so the fill
    /// would not have helped it, and counting it would inflate the number. How
    /// often a *rendered* image asks for it is what would justify building the
    /// fill.
    pub sixel_p2_ignored: usize,
    /// The epoch, bumped whenever the set of stored images changes. A GPU
    /// texture cache keyed on it drops a texture whose image was freed and
    /// its id reused. See [`ImageGen`].
    epoch: ImageGen,
    /// How many placements are live: the O(1) gate so the per-write
    /// overwrite check (does this write land on an image, so free it) costs
    /// one comparison on the common path where no image is on screen.
    placed: usize,
    /// Each screen's [`Reach`], `None` when nothing was placed on it since
    /// the last scan found it empty.
    reach: [Option<Reach>; 2],
    /// The tallest image stored since the store was last reset, in pixels:
    /// with the cell height, how far below its anchor any image can reach.
    /// Widened by every store and never narrowed -- a bound, like `reach`.
    tallest: ImagePx,
}

impl<'b> ImageStore<'b> {
    /// A store over caller memory: `pixels` the arena, `slots` the
    /// image-slot table, `placements` the placement table.
    pub fn new(
        pixels: &'b mut [Pixel],
        slots: &'b mut [ImageSlot],
        placements: &'b mut [PlacementSlot],
    ) -> ImageStore<'b> {
        ImageStore {
            pixels,
            slots,
            placements,
            images_refused: 0,
            refusals: ImageRefusals::NONE,
            placements_refused: 0,
            images_dropped: 0,
            placements_expired: 0,
            sixel_p2_ignored: 0,
            epoch: ImageGen::ZERO,
            placed: 0,
            reach: [None; 2],
            tallest: ImagePx::ZERO,
        }
    }

    /// Widen `screen`'s reach to hold `line`.
    fn widen(&mut self, screen: ImageScreen, line: LineIdx) {
        let r = &mut self.reach[screen.slot()];
        *r = Some(match *r {
            None => Reach {
                first: line,
                last: line,
            },
            Some(Reach { first, last }) => Reach {
                first: first.min(line),
                last: last.max(line),
            },
        });
    }

    /// Recompute `screen`'s reach from its live placements, exactly.
    fn rescan(&mut self, screen: ImageScreen) {
        self.reach[screen.slot()] = None;
        for i in 0..self.placements.len() {
            let p = self.placements[i];
            if !p.is_empty() && p.screen == screen {
                self.widen(screen, p.line);
            }
        }
    }

    /// The store's epoch: bumped on every `store`, `free`, `reset`, and every
    /// `unplace` that drops an image. A GPU texture cache keyed on it and the
    /// [`ImageId`] survives an image being freed and its slot reused.
    pub fn generation(&self) -> ImageGen {
        self.epoch
    }

    /// Reserve a `width` x `height` span and return its id and the slice to
    /// decode into. The caller paints the slice in place -- a [`SixelCanvas`]
    /// over it, or a memcpy of raw Kitty pixels -- so nothing is copied
    /// afterwards.
    ///
    /// **`alpha` is the decoder's mark** for what it writes: a format with no
    /// alpha, or one whose pixels are each opaque or clear, is
    /// [`ImageAlpha::OpaqueOrClear`]; one that may hold a partly transparent
    /// pixel is [`ImageAlpha::Partial`], or [`ImageAlpha::of`] the pixels
    /// once written. A `Partial` mark on opaque pixels costs a repaint of
    /// their rows each frame; an `OpaqueOrClear` mark on partial ones
    /// sends them through a renderer's opaque blit, which stores or draws
    /// each drawn pixel whole without blending it.
    ///
    /// **The slice holds whatever the arena held there**, the pixels of the
    /// last image freed from that span included: it is not cleared here, so a
    /// caller that writes every word (the memcpy) writes each once. A caller
    /// that leaves any word unwritten clears the slice first. A Sixel does not
    /// come through here: it decodes as it arrives into the gap it claimed
    /// ([`SixelStream`]), clearing each row as the image reaches it, because
    /// before `sixel_end` cleared its span a Sixel showed a freed preview
    /// through its unset bits.
    ///
    /// Refused, counted, and `None`, if the dimensions are zero, if no image
    /// slot is free, or if no gap in the arena holds the span. Never truncated:
    /// half an image is a wrong image.
    pub fn store(
        &mut self,
        width: ImagePx,
        height: ImagePx,
        alpha: ImageAlpha,
    ) -> Option<(ImageId, &mut [Pixel])> {
        let area = match width.index().checked_mul(height.index()) {
            Some(a) if a > 0 => a,
            _ => {
                self.refuse(ImageRefusal::Empty);
                return None;
            }
        };
        let at = self.lowest_gap(area);
        let id = self.store_at(at, width, height, alpha)?;
        let start = at?;
        Some((id, &mut self.pixels[start..start + area]))
    }

    /// Record a `width` x `height` image whose pixels are already at word
    /// `at` of the arena -- `None` when no gap held it. Refused and counted
    /// in [`ImageStore::store`]'s order: no free slot, then the arena, which
    /// is also what a span past the arena's end or over a live image is, so
    /// a caller that named a wrong offset gets a refusal rather than a
    /// corrupted image. `width * height` is not zero here.
    pub(crate) fn store_at(
        &mut self,
        at: Option<usize>,
        width: ImagePx,
        height: ImagePx,
        alpha: ImageAlpha,
    ) -> Option<ImageId> {
        let Some(slot) = self.free_slot() else {
            self.refuse(ImageRefusal::Slot);
            return None;
        };
        let area = width.index() * height.index();
        let span = at.and_then(|a| {
            let end = a.checked_add(area)?;
            let offset = u32::try_from(a).ok()?;
            let clear = end <= self.pixels.len()
                && !self
                    .slots
                    .iter()
                    .any(|s| !s.is_empty() && a < s.end() && s.start() < end);
            clear.then_some(offset)
        });
        let Some(offset) = span else {
            self.refuse(ImageRefusal::Arena(width, height));
            return None;
        };
        self.slots[slot] = ImageSlot {
            offset,
            width,
            height,
            alpha,
            placements: Count::ZERO,
        };
        self.tallest = self.tallest.max(height);
        self.epoch = self.epoch.bumped();
        Some(ImageId::from_index(slot))
    }

    /// Whether an image slot is free, which a Sixel decoded as it arrives
    /// asks at its terminator, to refuse for the slot before the arena as
    /// [`ImageStore::store`] does.
    pub(crate) fn has_free_slot(&self) -> bool {
        self.free_slot().is_some()
    }

    /// The largest gap between live spans, as its start and length in words,
    /// the lowest start of two as large: what a Sixel claims at its `q`,
    /// before its size is known. No gap holds more at that moment; a Sixel
    /// asks again when its claim runs out, since a free during its body can
    /// open a larger one.
    pub(crate) fn largest_gap(&self) -> (usize, usize) {
        let n = self.pixels.len();
        let live = || self.slots.iter().filter(|s| !s.is_empty());
        // A gap starts at the arena's start or where a live span ends, and
        // runs to the next live start or the arena's end.
        core::iter::once(0)
            .chain(live().map(|s| s.end()))
            .filter(|&c| c < n && !live().any(|s| s.start() <= c && c < s.end()))
            .map(|c| {
                let end = live()
                    .map(|s| s.start())
                    .filter(|&s| s >= c)
                    .min()
                    .unwrap_or(n);
                (c, end - c)
            })
            .fold((0, 0), |best, g| {
                match g.1 > best.1 || (g.1 == best.1 && g.0 < best.0) {
                    true => g,
                    false => best,
                }
            })
    }

    /// The arena, for a Sixel painting the gap it claimed. Nothing but the
    /// claim is written through it: the claim is a gap, so no live image is
    /// in it.
    pub(crate) fn arena_mut(&mut self) -> &mut [Pixel] {
        self.pixels
    }

    /// Count a refused image: the total and its cause in one place, so the
    /// two cannot disagree. The store calls it for its own three refusals;
    /// a decoder calls it for the ones it finds before the store is asked --
    /// a Sixel that paints nothing, or one past 65535 either way.
    pub fn refuse(&mut self, why: ImageRefusal) {
        self.images_refused += 1;
        let r = &mut self.refusals;
        match why {
            ImageRefusal::Empty => r.empty += 1,
            ImageRefusal::Slot => r.slot += 1,
            ImageRefusal::Arena(width, height) => {
                r.arena += 1;
                r.arena_width = width;
                r.arena_height = height;
            }
            ImageRefusal::TooLarge => r.too_large += 1,
        }
    }

    /// A read view of a live image, or `None` if `id` names none.
    pub fn image(&self, id: ImageId) -> Option<ImageView<'_>> {
        let i = id.index()?;
        let s = *self.slots.get(i)?;
        if s.is_empty() {
            return None;
        }
        Some(ImageView {
            pixels: &self.pixels[s.start()..s.end()],
            width: s.width,
            height: s.height,
            alpha: s.alpha,
        })
    }

    /// Place a live image at an absolute line of `screen`, a column and a draw
    /// order, returning the placement id. Refused, counted, and `None` if the
    /// image is not live or no placement slot is free.
    pub fn place(
        &mut self,
        id: ImageId,
        screen: ImageScreen,
        line: LineIdx,
        col: Col,
        z: ZIndex,
    ) -> PlacementId {
        let Some(i) = id.index() else {
            self.placements_refused += 1;
            return PlacementId::NONE;
        };
        match self.slots.get(i) {
            Some(s) if !s.is_empty() => {}
            _ => {
                self.placements_refused += 1;
                return PlacementId::NONE;
            }
        }
        let Some(p) = self.free_placement() else {
            self.placements_refused += 1;
            return PlacementId::NONE;
        };
        self.placements[p] = PlacementSlot {
            image: id,
            screen,
            line,
            col,
            z,
            follows: Col::ZERO,
            staged: Staged::Idle,
        };
        self.slots[i].placements = Count::of(self.slots[i].placements.index() + 1);
        self.placed += 1;
        self.widen(screen, line);
        PlacementId::from_index(p)
    }

    /// Remove a placement. If it was the image's last, the image is freed and
    /// its span returns to the arena.
    pub fn unplace(&mut self, pid: PlacementId) {
        let Some(p) = pid_index(pid) else { return };
        let Some(slot) = self.placements.get(p) else {
            return;
        };
        if slot.is_empty() {
            return;
        }
        let image = slot.image;
        self.placements[p] = PlacementSlot::EMPTY;
        self.placed = self.placed.saturating_sub(1);
        let Some(i) = image.index() else { return };
        let Some(s) = self.slots.get_mut(i) else {
            return;
        };
        let left = s.placements.index().saturating_sub(1);
        s.placements = Count::of(left);
        if left == 0 {
            *s = ImageSlot::EMPTY;
            self.images_dropped += 1;
            self.epoch = self.epoch.bumped();
        }
    }

    /// Delete an image and every placement of it (the Kitty uppercase delete).
    pub fn free(&mut self, id: ImageId) {
        let Some(i) = id.index() else { return };
        match self.slots.get_mut(i) {
            Some(s) if !s.is_empty() => *s = ImageSlot::EMPTY,
            _ => return,
        }
        for p in self.placements.iter_mut() {
            if p.image == id {
                *p = PlacementSlot::EMPTY;
                self.placed = self.placed.saturating_sub(1);
            }
        }
        self.images_dropped += 1;
        self.epoch = self.epoch.bumped();
    }

    /// Every live placement, for the renderer to draw. Order is slot order, not
    /// `z` order; the renderer sorts by `z`.
    pub fn placements(&self) -> impl Iterator<Item = (PlacementId, &PlacementSlot)> + '_ {
        self.placements
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.is_empty())
            .map(|(i, p)| (PlacementId::from_index(i), p))
    }

    /// Forget every image and placement. `RIS`, not a clear -- an image is
    /// content a program may redraw over, and a placement outliving one erase
    /// is the correct behaviour, the link-table call. Leaving the alternate
    /// screen drops only that screen's, through
    /// [`ImageStore::expire_screen`].
    pub fn reset(&mut self) {
        for s in self.slots.iter_mut() {
            *s = ImageSlot::EMPTY;
        }
        for p in self.placements.iter_mut() {
            *p = PlacementSlot::EMPTY;
        }
        self.placed = 0;
        self.reach = [None; 2];
        self.tallest = ImagePx::ZERO;
        self.epoch = self.epoch.bumped();
    }

    /// Whether any placement is live -- the O(1) gate for the write path's
    /// overwrite check, so a stream with no image on screen pays one compare.
    pub fn any_placed(&self) -> bool {
        self.placed > 0
    }

    /// Whether the store has any pixel arena at all, so a Sixel can be drawn
    /// rather than refused for want of memory. What the Primary DA's Sixel
    /// advertisement is gated on: an unprovisioned store (the caller passed no
    /// image budget) refuses every Sixel, and claiming Sixel then would make an
    /// application send images baud draws as nothing.
    pub fn provisioned(&self) -> bool {
        !self.pixels.is_empty()
    }

    /// Drop every placement on `screen` whose anchor row is gone: one before
    /// `lo` or at or past `hi`, the rows the screen retains, or one on
    /// `blanked`, a row a scroll blanked as it came in. Each is counted in
    /// [`ImageStore::placements_expired`]. Returns how many.
    ///
    /// An image goes whole when its **anchor** row goes, foot's rule: a
    /// placement names one row, and an image half off the far end of the
    /// history is a picture of nothing a reader is looking at.
    pub fn expire(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        blanked: Option<LineIdx>,
    ) -> usize {
        let Some(r) = self.reach[screen.slot()] else {
            return 0;
        };
        let clear = blanked.is_none_or(|b| b < r.first || b > r.last);
        if r.first >= lo && r.last < hi && clear {
            return 0;
        }
        let mut n = 0;
        for p in 0..self.placements.len() {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen {
                continue;
            }
            if s.line < lo || s.line >= hi || Some(s.line) == blanked {
                self.unplace(PlacementId::from_index(p));
                self.placements_expired += 1;
                n += 1;
            }
        }
        self.rescan(screen);
        n
    }

    /// Drop every placement on `screen`, counted as expired: the alternate
    /// screen being left, whose rows go with it.
    pub fn expire_screen(&mut self, screen: ImageScreen) -> usize {
        self.rescan(screen);
        self.expire(screen, LineIdx::FIRST, LineIdx::FIRST, None)
    }

    /// **Move `screen`'s placements with rows that moved inside a band**:
    /// the lines `[lo, hi)` shifted `n` rows `way`, which is what a
    /// scrolling region, `IL` and `DL` do to the rows between their bounds
    /// without moving the window. A placement whose anchor leaves the band
    /// went with its row -- deleted off one end, the region's text with it
    /// -- and is dropped and counted in [`ImageStore::placements_expired`].
    /// Outside the band nothing moves. Returns how many were dropped.
    pub fn scroll_band(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        n: Rows,
        way: BandMove,
    ) -> usize {
        self.scroll_band_within(screen, lo, hi, None, n, way)
    }

    /// [`ImageStore::scroll_band`] for a band that is a rectangle: only a
    /// placement whose anchor column lies in `cols`, inclusive, moved with
    /// the text, because only the text there moved -- a scroll under side
    /// margins. **The anchor's column decides, as the anchor's row does for
    /// a band**: no text says what an image does under a rectangle scroll,
    /// and ghostty moves only a placement wholly inside the rectangle
    /// (`[[source: ghostty-d9840f3-graphics-storage]]`, 513-522), a choice
    /// listed for Jev in the record ("Rectangle scrolling").
    pub fn scroll_rect(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        cols: (Col, Col),
        n: Rows,
        way: BandMove,
    ) -> usize {
        self.scroll_band_within(screen, lo, hi, Some(cols), n, way)
    }

    fn scroll_band_within(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        cols: Option<(Col, Col)>,
        n: Rows,
        way: BandMove,
    ) -> usize {
        let Some(r) = self.reach[screen.slot()] else {
            return 0;
        };
        if r.last < lo || r.first >= hi {
            return 0;
        }
        let mut n_gone = 0;
        for p in 0..self.placements.len() {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen || s.line < lo || s.line >= hi {
                continue;
            }
            if cols.is_some_and(|(l, r)| s.col < l || s.col > r) {
                continue;
            }
            let to = match way {
                BandMove::Up => s.line.minus(n).filter(|l| *l >= lo),
                BandMove::Down => Some(s.line.plus(n)).filter(|l| *l < hi),
            };
            match to {
                Some(line) => self.placements[p].line = line,
                None => {
                    self.unplace(PlacementId::from_index(p));
                    self.placements_expired += 1;
                    n_gone += 1;
                }
            }
        }
        self.rescan(screen);
        n_gone
    }

    /// Move every placement on `screen` `n` rows later. For a window moved
    /// back over rows its numbering has no line for -- a reverse scroll at the
    /// top of a screen with no history -- so its text moved down and the
    /// numbering, which cannot go below zero, moves the images instead.
    pub fn shift(&mut self, screen: ImageScreen, n: Rows) {
        for p in self.placements.iter_mut() {
            if !p.is_empty() && p.screen == screen {
                p.line = p.line.plus(n);
            }
        }
        self.rescan(screen);
    }

    /// **Follow `screen`'s placements through a reflow**, in three steps
    /// around it. This stages each placement at its row counted from
    /// `floor`, the oldest retained line; [`ImageStore::follow_line`] maps
    /// the ones on each line as the reflow visits it; and
    /// [`ImageStore::follow_end`] writes them back and drops the ones whose
    /// line the reflow dropped. Staging is what lets each placement be mapped
    /// exactly once, though the old and new layouts share one numbering: a
    /// row mapped into a later line's old range is not mapped again.
    /// Returns how many are staged, so a caller with none skips the hook.
    pub fn follow_begin(&mut self, screen: ImageScreen, floor: LineIdx) -> usize {
        let mut n = 0;
        for p in self.placements.iter_mut() {
            if p.is_empty() || p.screen != screen {
                continue;
            }
            p.staged = match p.line.since(floor) {
                Some(r) => {
                    n += 1;
                    Staged::From(r)
                }
                None => Staged::Idle,
            };
        }
        n
    }

    /// Map the staged placements on one line of a reflow: the line's first
    /// `rows` rows from `from`, and `land`, where a cell of it -- a row and a
    /// column in it -- lands. Each placement asks by the cell its row began
    /// with when it was placed, and keeps where that cell landed. Called
    /// before the line is copied, while its cells can still be read.
    pub fn follow_line(&mut self, from: Rows, rows: Rows, land: &dyn Fn(Rows, Col) -> (Rows, Col)) {
        let end = from.index() + rows.index();
        for p in self.placements.iter_mut() {
            match p.staged {
                Staged::From(r) if (from.index()..end).contains(&r.index()) => {
                    let (row, at) = land(r, p.follows);
                    p.staged = Staged::To(row, at);
                }
                _ => {}
            }
        }
    }

    /// Leave a follow the reflow never ran: every staged placement stays
    /// where it was. For a reflow that returned before re-cutting anything,
    /// which [`ImageStore::follow_end`] would read as every line dropped.
    pub fn follow_cancel(&mut self) {
        for p in self.placements.iter_mut() {
            p.staged = Staged::Idle;
        }
    }

    /// Write the mapped placements back as lines from `floor` -- the same
    /// oldest retained line the staging counted from, which a reflow keeps as
    /// its origin -- and drop, counted, the ones no line took. Returns how
    /// many were dropped.
    pub fn follow_end(&mut self, floor: LineIdx) -> usize {
        let mut n = 0;
        for p in 0..self.placements.len() {
            match self.placements[p].staged {
                Staged::Idle => continue,
                Staged::To(r, at) => {
                    self.placements[p].line = floor.plus(r);
                    self.placements[p].follows = at;
                    self.placements[p].staged = Staged::Idle;
                }
                Staged::From(_) => {
                    self.placements[p].staged = Staged::Idle;
                    self.unplace(PlacementId::from_index(p));
                    self.placements_expired += 1;
                    n += 1;
                }
            }
        }
        self.rescan(ImageScreen::Primary);
        self.rescan(ImageScreen::Alternate);
        n
    }

    /// Free every image a write covers: a glyph or erase at absolute `line`,
    /// columns `[col0, col1)`, over `cell_w` by `cell_h` pixel cells. An image
    /// spans `ceil(h / cell_h)` rows by `ceil(w / cell_w)` cells from its
    /// anchor, and writing new content over any of them removes it -- the
    /// standard "text over a Sixel erases it", and what a previewer relies on
    /// to replace one preview with the next. Without it a previewer's images
    /// pile up at one cell until the store fills and every later preview is
    /// refused, drawn stacked and transparent-blended into garbage in the
    /// meantime. Zero cell dimensions (a headless caller that set none) free
    /// nothing. Returns how many placements were freed.
    pub fn overwrite(
        &mut self,
        screen: ImageScreen,
        line: LineIdx,
        col0: usize,
        col1: usize,
        cell_w: ImagePx,
        cell_h: ImagePx,
    ) -> usize {
        let (cw, ch) = (cell_w.index(), cell_h.index());
        if self.placed == 0 || cw == 0 || ch == 0 || col1 <= col0 {
            return 0;
        }
        // The write's row against the rows any image on this screen can
        // cover: from the first anchor down to the tallest image below the
        // last. Text written past them -- the common case, output after an
        // image -- is answered here and never scans.
        let Some(r) = self.reach[screen.slot()] else {
            return 0;
        };
        let below = Rows::of_count(self.tallest.index().div_ceil(ch));
        if line < r.first || line >= r.last.plus(below) {
            return 0;
        }
        let wl = usize::try_from(line.value()).unwrap_or(usize::MAX);
        let mut freed = 0;
        for p in 0..self.placements.len() {
            let slot = self.placements[p];
            if slot.is_empty() || slot.screen != screen {
                continue;
            }
            let Some(i) = slot.image.index() else {
                continue;
            };
            let img = self.slots[i];
            if img.is_empty() {
                continue;
            }
            let rows = img.height.index().div_ceil(ch);
            let cols = img.width.index().div_ceil(cw);
            let l0 = usize::try_from(slot.line.value()).unwrap_or(usize::MAX);
            let c0 = slot.col.index();
            // The write's row `wl` lands in the image's rows, and its column
            // span `[col0, col1)` overlaps the image's `[c0, c0 + cols)`.
            if wl >= l0 && wl < l0 + rows && col1 > c0 && col0 < c0 + cols {
                self.unplace(PlacementId::from_index(p));
                freed += 1;
            }
        }
        freed
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_empty())
    }

    fn free_placement(&self) -> Option<usize> {
        self.placements.iter().position(|p| p.is_empty())
    }

    /// The lowest offset at which `area` pixels fit in a gap between live spans,
    /// or `None` if the arena has no such gap: where [`ImageStore::store`]
    /// puts an image, and where a Sixel decoded in its claim moves to. Free space is the gaps between
    /// live slots, so this reclaims every freed span with no free list to keep:
    /// a candidate offset is bumped past any span it overlaps until a full pass
    /// finds none, which converges because the candidate only rises and is
    /// bounded by the arena length.
    pub(crate) fn lowest_gap(&self, area: usize) -> Option<usize> {
        let mut cand: usize = 0;
        loop {
            let end = cand.checked_add(area)?;
            if end > self.pixels.len() {
                return None;
            }
            let mut bumped = false;
            for s in self.slots.iter() {
                if s.is_empty() {
                    continue;
                }
                // Overlap of [cand, end) with the live span [start, end).
                if cand < s.end() && s.start() < end && s.end() > cand {
                    cand = s.end();
                    bumped = true;
                }
            }
            if !bumped {
                return Some(cand);
            }
        }
    }
}

fn pid_index(pid: PlacementId) -> Option<usize> {
    usize::try_from(pid.0).ok()?.checked_sub(1)
}
