//! Image storage for Sixel and Kitty graphics: a caller-provided ARGB arena, a
//! bounded image-slot table and a bounded placement table, all
//! **refuse-and-count**. Pure, `no_std`, allocation-free -- the caller owns the
//! pixels, the slots and the placements exactly as it owns the grid and the
//! arena, and `docs/research/notes/t-images.md` is the design.
//!
//! An image is decoded **once, into the arena in place** (a [`SixelCanvas`] over
//! the slice [`ImageStore::store`] returns), which is the zero-extra-copy path
//! the register describes: the pixels land where a renderer will read them
//! rather than in a scratch that is then copied. The storage carries ARGB
//! [`ColorWord`]s and never palette indices, decided in the register on the
//! shader-sampling ground -- a palette index in the cell shader is a second
//! dependent texture fetch.
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
//! spans, recomputed per allocation ([`ImageStore::allocate`]), so a free is a
//! slot cleared and nothing else -- baud's "compute the gaps rather than keep a
//! free list" instinct, and correct for every free order rather than only a
//! stack.

use crate::pos::LineIdx;
use crate::quantity::{Col, Count, ReadLen, Rows};
use crate::style::ColorWord;

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
    To(Rows),
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
        staged: Staged::Idle,
    };

    fn is_empty(self) -> bool {
        self.image.is_none()
    }
}

/// How much caller memory an image store is given: the ARGB pixel arena in
/// `ColorWord`s and the Sixel-body scratch in bytes. A typed pair rather than
/// two loose sizes, so a caller provisioning a store names which is which -- and
/// so `baud-core`'s provisioning method takes one struct rather than two raw
/// `usize`s, which its signature budget forbids.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageBudget {
    /// `ColorWord`s of ARGB pixel arena.
    pub pixels: usize,
    /// Bytes of Sixel-body scratch, holding one raw body before it decodes.
    pub body: usize,
}

impl ImageBudget {
    /// A budget of `pixels` arena words and `body` scratch bytes.
    pub fn of(pixels: usize, body: usize) -> ImageBudget {
        ImageBudget { pixels, body }
    }
}

/// Which limit refused an image, with the size that hit it -- what
/// [`ImageStore::refuse`] is told, and what [`ImageRefusals`] counts. Each
/// variant is a different fix: the first two are `[images] body_kb` and
/// `pixels_mb` in the app, the slot table is a provisioning constant, and an
/// image with no pixels is no limit at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageRefusal {
    /// A Sixel's raw body ran past the body scratch: its true length, which
    /// keeps counting after the scratch stops holding bytes. An unprovisioned
    /// store's empty scratch is this, against a limit of zero.
    Body(ReadLen),
    /// An image with no pixels: an empty Sixel, one whose data paints
    /// nothing, or a zero dimension handed to [`ImageStore::store`]. No limit
    /// refused it and none raised would draw it.
    Empty,
    /// Every image slot held a live image.
    Slot,
    /// No gap in the pixel arena held a `width` x `height` image -- larger
    /// than the whole arena, or larger than what the live images leave free.
    Arena(ImagePx, ImagePx),
}

/// The image refusals split by the limit each hit, with the size the last one
/// of each carried. [`ImageStore::images_refused`] is their total, and stays
/// the one number for a caller that only asks how many: it mixed a body over
/// the scratch with an image over the arena, and the two are different knobs
/// -- yazi's previews went blank on the first (a 600x375 screenshot is a
/// 334 KiB body against the app's 256 KiB scratch) while the arena held them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImageRefusals {
    /// Sixels refused because the raw body ran past the body scratch.
    pub body: usize,
    /// The true length of the last body refused for it, so a reader can see
    /// how far to raise the scratch. Zero until one is.
    pub body_len: ReadLen,
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
}

impl ImageRefusals {
    /// Nothing refused.
    pub const NONE: ImageRefusals = ImageRefusals {
        body: 0,
        body_len: ReadLen::ZERO,
        empty: 0,
        slot: 0,
        arena: 0,
        arena_width: ImagePx::ZERO,
        arena_height: ImagePx::ZERO,
    };

    /// Every refusal, whatever its cause: what
    /// [`ImageStore::images_refused`] equals.
    pub fn total(&self) -> usize {
        self.body + self.empty + self.slot + self.arena
    }
}

/// A read view of a stored image: its pixels and its size.
pub struct ImageView<'a> {
    /// The ARGB pixels, `width * height` of them, row-major.
    pub pixels: &'a [ColorWord],
    /// The image width in pixels.
    pub width: ImagePx,
    /// The image height in pixels.
    pub height: ImagePx,
}

/// A bounded, caller-owned store of images and their placements.
///
/// The three caller buffers are the arena (ARGB pixels), the image-slot table
/// and the placement table; all three are refuse-and-count, and the counters are
/// public and never silent, the [`ClusterStore`] contract.
pub struct ImageStore<'b> {
    pixels: &'b mut [ColorWord],
    slots: &'b mut [ImageSlot],
    placements: &'b mut [PlacementSlot],
    /// Images refused, whatever the cause. Never silent: a refused image is
    /// not drawn, and how often is a number. [`ImageStore::refusals`] says
    /// which limit each hit, and this is their total.
    pub images_refused: usize,
    /// The refusals by cause: the body scratch, the slot table, the arena, or
    /// an image with no pixels. Every refusal in the tree goes through
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
    /// A store over caller memory: `pixels` the ARGB arena, `slots` the
    /// image-slot table, `placements` the placement table.
    pub fn new(
        pixels: &'b mut [ColorWord],
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
    /// Refused, counted, and `None`, if the dimensions are zero, if no image
    /// slot is free, or if no gap in the arena holds the span. Never truncated:
    /// half an image is a wrong image.
    pub fn store(
        &mut self,
        width: ImagePx,
        height: ImagePx,
    ) -> Option<(ImageId, &mut [ColorWord])> {
        let area = match width.index().checked_mul(height.index()) {
            Some(a) if a > 0 => a,
            _ => {
                self.refuse(ImageRefusal::Empty);
                return None;
            }
        };
        let slot = match self.free_slot() {
            Some(i) => i,
            None => {
                self.refuse(ImageRefusal::Slot);
                return None;
            }
        };
        let offset = match self.allocate(area) {
            Some(o) => o,
            None => {
                self.refuse(ImageRefusal::Arena(width, height));
                return None;
            }
        };
        self.slots[slot] = ImageSlot {
            offset,
            width,
            height,
            placements: Count::ZERO,
        };
        self.tallest = self.tallest.max(height);
        self.epoch = self.epoch.bumped();
        let start = offset as usize;
        Some((
            ImageId::from_index(slot),
            &mut self.pixels[start..start + area],
        ))
    }

    /// Count a refused image: the total and its cause in one place, so the
    /// two cannot disagree. The store calls it for its own three refusals;
    /// a decoder calls it for the ones it finds before the store is asked --
    /// a Sixel body past the scratch, or one that paints nothing.
    pub fn refuse(&mut self, why: ImageRefusal) {
        self.images_refused += 1;
        let r = &mut self.refusals;
        match why {
            ImageRefusal::Body(len) => {
                r.body += 1;
                r.body_len = len;
            }
            ImageRefusal::Empty => r.empty += 1,
            ImageRefusal::Slot => r.slot += 1,
            ImageRefusal::Arena(width, height) => {
                r.arena += 1;
                r.arena_width = width;
                r.arena_height = height;
            }
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
    /// `rows` rows from `from`, and `land`, where a row of it lands. Called
    /// before the line is copied, while its cells can still be read.
    pub fn follow_line(&mut self, from: Rows, rows: Rows, land: &dyn Fn(Rows) -> Rows) {
        let end = from.index() + rows.index();
        for p in self.placements.iter_mut() {
            match p.staged {
                Staged::From(r) if (from.index()..end).contains(&r.index()) => {
                    p.staged = Staged::To(land(r));
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
                Staged::To(r) => {
                    self.placements[p].line = floor.plus(r);
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
    /// or `None` if the arena has no such gap. Free space is the gaps between
    /// live slots, so this reclaims every freed span with no free list to keep:
    /// a candidate offset is bumped past any span it overlaps until a full pass
    /// finds none, which converges because the candidate only rises and is
    /// bounded by the arena length.
    fn allocate(&self, area: usize) -> Option<u32> {
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
                return u32::try_from(cand).ok();
            }
        }
    }
}

fn pid_index(pid: PlacementId) -> Option<usize> {
    usize::try_from(pid.0).ok()?.checked_sub(1)
}
