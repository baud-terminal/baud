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
use crate::quantity::{Col, Cols, Count, Rows};
use crate::style::Chan;
use zerocopy::{FromBytes, FromZeros, Immutable, IntoBytes, KnownLayout};

/// **One image pixel, straight ARGB**: `0xAARRGGBB`, the colour not
/// multiplied by its alpha (`[[source: w3c-png-2003]]` 6.2). Zero is
/// [`Pixel::CLEAR`], which a Sixel leaves where its data sets no bit, so a
/// zeroed arena is a clear one. The word is the one a renderer's `Argb`
/// and a `BGRA8Unorm` texture read, so an image goes to either unconverted.
///
/// **Bytes too, through zerocopy**, so a decoder whose output is a byte
/// stream -- inflate, whose output buffer is also its window, and PNG's
/// RGBA8 rows -- writes the reserved span in place, and
/// [`Pixel::from_memory_rgba`] turns each four bytes into the word where they lie.
#[repr(transparent)]
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, FromBytes, IntoBytes, KnownLayout, Immutable,
)]
pub struct Pixel(u32);

impl Pixel {
    /// Fully transparent: drawn as nothing.
    pub const CLEAR: Pixel = Pixel(0);

    /// **`n` pixels, every one [`Pixel::CLEAR`], from zeroed memory**: what
    /// `vec![Pixel::CLEAR; n]` holds, without writing it. `vec!` takes the
    /// allocator's zeroed path only for the primitive types it knows to be
    /// zero, and wrote all `n` words of a newtype, so an arena committed
    /// every page at construction whether or not an image came; zeroed
    /// pages stay uncommitted until an image writes them. Allocation failure
    /// is `vec!`'s: the allocator's error handler, or a capacity overflow.
    #[cfg(feature = "alloc")]
    pub fn zeroed_vec(n: usize) -> alloc::vec::Vec<Pixel> {
        match Pixel::new_vec_zeroed(n) {
            Ok(v) => v,
            Err(_) => match core::alloc::Layout::array::<Pixel>(n) {
                Ok(layout) => alloc::alloc::handle_alloc_error(layout),
                Err(_) => panic!("capacity overflow"),
            },
        }
    }

    /// A fully opaque pixel of three bytes, for a table built at compile
    /// time, where [`Pixel::opaque`]'s channels cannot be made.
    pub(crate) const fn opaque_bytes(r: u8, g: u8, b: u8) -> Pixel {
        Pixel(0xFF00_0000 | (r as u32) << 16 | (g as u32) << 8 | b as u32)
    }

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

    /// The pixel whose four bytes in memory are `r, g, b, a` in that order,
    /// as PNG's RGBA8 and kitty's `f=32` lay them out: the word a byte
    /// decoder left in place, read back as the straight ARGB it means.
    /// `to_ne_bytes` reads the bytes in memory order on either endianness.
    pub fn from_memory_rgba(self) -> Pixel {
        let [r, g, b, a] = self.0.to_ne_bytes();
        Pixel(u32::from_be_bytes([a, r, g, b]))
    }
}

/// **How an image's pixels blend**, marked once at decode: what decides
/// whether a renderer that keeps its last frame may blit the image again
/// over its own last blit.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
    /// The mark of `self`'s pixels and `p` together: partial once any is.
    pub fn with(self, p: Pixel) -> ImageAlpha {
        match (self, p.alpha().get()) {
            (ImageAlpha::OpaqueOrClear, 0 | 0xFF) => ImageAlpha::OpaqueOrClear,
            _ => ImageAlpha::Partial,
        }
    }

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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromZeros)]
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromZeros)]
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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, FromZeros)]
pub struct ImageId(u32);

impl ImageId {
    /// Names no image.
    pub const NONE: ImageId = ImageId(0);

    /// The store's query probe ([`ImageStore::reserve_probe`]): past every
    /// slot, so no slot lookup finds it.
    const PROBE: ImageId = ImageId(u32::MAX);

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
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
///
/// **It is also an entry of the placement index**, which finds a kitty
/// placement by its image and placement id: an open-addressed table of
/// handles, `NONE` an empty entry, lent to [`ImageStore::new`] beside the
/// placement table at [`PlacementId::index_len`] entries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromZeros)]
#[repr(transparent)]
pub struct PlacementId(u32);

impl PlacementId {
    /// Names no placement.
    pub const NONE: PlacementId = PlacementId(0);

    /// **The index length for a table of `slots` placements**: a power of
    /// two at least twice as many, so the index is never more than half
    /// full and a lookup ends at an empty entry in a step or two. A table
    /// sized to a 400x150 screen, 60,000 slots, has 131,072 entries, 512
    /// KiB of address space beside the table's 7.2 MB.
    pub fn index_len(slots: usize) -> usize {
        slots.saturating_mul(2).max(2).next_power_of_two()
    }

    /// **An empty index for a table of `slots` placements**, from zeroed
    /// memory as [`PlacementSlot::zeroed_vec`]'s table is, so a page is
    /// committed only when an entry first lands on it.
    #[cfg(feature = "alloc")]
    pub fn zeroed_index(slots: usize) -> alloc::vec::Vec<PlacementId> {
        let n = PlacementId::index_len(slots);
        match PlacementId::new_vec_zeroed(n) {
            Ok(v) => v,
            Err(_) => match core::alloc::Layout::array::<PlacementId>(n) {
                Ok(layout) => alloc::alloc::handle_alloc_error(layout),
                Err(_) => panic!("capacity overflow"),
            },
        }
    }

    fn from_index(i: usize) -> PlacementId {
        PlacementId((i as u32).wrapping_add(1))
    }

    /// Whether this handle names no placement.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A kitty image id, the `i` key (`[[source: kitty-graphics]]`, "Control
/// data reference"): "an arbitrary positive integer up to 4294967295", zero
/// meaning none was given. The client's name for an image, not the store's
/// [`ImageId`], which is a slot.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct KittyId(u32);

impl KittyId {
    /// No id: an image sent without `i` or `I`.
    pub const NONE: KittyId = KittyId(0);

    /// The id `n`.
    pub fn of(n: u32) -> KittyId {
        KittyId(n)
    }

    /// The raw id, for a reply.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Whether this names no image.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A kitty image number, the `I` key: not unique, and a command naming one
/// acts on "the newest image with that number" (`[[source: kitty-graphics]]`,
/// "Requesting image ids from the terminal").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct KittyNumber(u32);

impl KittyNumber {
    /// No number.
    pub const NONE: KittyNumber = KittyNumber(0);

    /// The number `n`.
    pub fn of(n: u32) -> KittyNumber {
        KittyNumber(n)
    }

    /// The raw number, for a reply.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Whether no number was given.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// A kitty placement id, the `p` key: with the image id, it names one
/// placement, and a second put with the same pair replaces the first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, FromZeros)]
pub struct KittyPlacement(u32);

impl KittyPlacement {
    /// No placement id.
    pub const NONE: KittyPlacement = KittyPlacement(0);

    /// The placement id `n`.
    pub fn of(n: u32) -> KittyPlacement {
        KittyPlacement(n)
    }

    /// The raw id, for a reply.
    pub fn raw(self) -> u32 {
        self.0
    }

    /// Whether no placement id was given.
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

/// **Which protocol stored an image**, because they free it differently.
/// A Sixel or an embedder's image is freed with its last placement, and text
/// written over it erases it. A kitty image outlives its placements -- `a=t`
/// stores one with none, and a lowercase delete removes placements "without
/// necessarily freeing up the stored image data" (`[[source: kitty-graphics]]`,
/// "Deleting images") -- and "The other commands to erase text must have no
/// effect on graphics" ("Interaction with other terminal actions").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageOwner {
    /// A Sixel, or [`ImageStore::store`]'s caller.
    Local,
    /// A kitty transmission, with the client's id and number. An image with
    /// neither can never be named again, so it goes with its last placement
    /// as a local one does (kitty's own rule, `[[source: kitty-graphics-c]]`
    /// `filter_refs` and `modify_refs`).
    Kitty(KittyId, KittyNumber),
    /// A ReGIS page ([`ImageStore::store_page`]): drawn on after it is
    /// stored, and written over by text a cell at a time rather than freed
    /// whole, as a Sixel is. It goes with its last placement as a local
    /// image does.
    Regis,
}

/// **Whether a stored image's pixels are all there.** A kitty image is
/// reserved and placed when its size is known and decoded afterwards, in
/// steps; "terminals must not display anything, until the entire sequence is
/// received and validated" (`[[source: kitty-graphics]]`, "Remote client"),
/// so [`ImageStore::image`] does not show it until it is
/// [`ImageState::Ready`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImageState {
    /// Reserved; its pixels are still being written.
    Decoding,
    /// Every pixel written.
    Ready,
}

/// Where a kitty placement draws from and over what: the source rectangle
/// in image pixels (`x`, `y`, `w`, `h`; a zero width or height is the rest
/// of the image), the offset inside the first cell (`X`, `Y`), the cells it
/// covers, from `c` and `r` or from the image and the cell size, and which
/// of the two counts the client gave. What the deletes that name a cell, a
/// row or a column intersect, what the cursor moves by, and what
/// [`KittyPlace::draw`] turns into the pixels the renderer draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromZeros)]
pub struct KittyPlace {
    /// The client's placement id, or none.
    pub id: KittyPlacement,
    /// The source rectangle's left edge, in image pixels.
    pub src_x: ImagePx,
    /// Its top edge.
    pub src_y: ImagePx,
    /// Its width; zero is the image's width past `src_x`.
    pub src_w: ImagePx,
    /// Its height; zero is the image's height past `src_y`.
    pub src_h: ImagePx,
    /// The offset inside the anchor cell, `X`.
    pub cell_x: ImagePx,
    /// The offset inside the anchor cell, `Y`.
    pub cell_y: ImagePx,
    /// The cells it covers across.
    pub cols: Cols,
    /// The rows it covers down.
    pub rows: Rows,
    /// Which of [`KittyPlace::cols`] and [`KittyPlace::rows`] the client
    /// gave, and so how the image is scaled into them.
    pub fit: KittyFit,
    /// The rows of [`KittyPlace::rows`] clipped off its top by a margin
    /// scroll: the placement's line is its first shown row, these rows
    /// above it.
    pub cut_top: Rows,
    /// The rows clipped off its bottom by a margin scroll.
    pub cut_bottom: Rows,
}

/// **Which of `c` and `r` a kitty placement gave**, because the text scales
/// each case its own way (`[[source: kitty-graphics]]`, "Controlling
/// displayed image layout"): the counts a placement stores are the cells
/// it covers whichever it was, and from them alone a derived count and a
/// given one cannot be told apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromZeros)]
#[repr(u8)]
pub enum KittyFit {
    /// Neither: "the entire image will be displayed", one image pixel a
    /// device pixel.
    Natural,
    /// `c` alone: the image as wide as the columns less `X`, and "the other
    /// one is computed based on the source image aspect ratio".
    Cols,
    /// `r` alone: as tall as the rows less `Y`, the width from the aspect.
    Rows,
    /// Both: "the image is letterboxed/pillarboxed to prevent distortion",
    /// fitted into the box with its aspect kept and centred in it (Jev,
    /// CENTRED, 0.94 and 0.97; the text does not say where in the box).
    Box,
}

/// A rectangle in pixels, `x` and `y` its top-left from an origin the user
/// names, `w` and `h` its size. Four fields of one unit, so a `u32` each:
/// the source rectangle is in image pixels and the drawn one in device
/// pixels, and [`ImageDraw`] keeps them apart by name.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PxRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl PxRect {
    /// Whether it covers no pixel.
    pub fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }
}

/// **What one placement draws**: `src`, the part of the image shown, in
/// image pixels, and `dest`, where it lands, in device pixels from the
/// anchor cell's top-left. The one description every drawing path reads
/// ([`KittyPlace::draw`]), so the paths cannot disagree about a placement's
/// geometry.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ImageDraw {
    pub src: PxRect,
    pub dest: PxRect,
}

impl ImageDraw {
    /// **The source pixel shown at drawn pixel `d`** of a span `dest` long
    /// mapping `src` source pixels from `off`: the one under the drawn
    /// pixel's centre, `off + floor((d + 1/2) src / dest)`, nearest
    /// neighbour. The text names no filter; Jev picked NEAREST on all four
    /// asks (0.88 and 0.52, then with kitty's `GL_LINEAR`, `[[source:
    /// kitty-shaders-c]]`, and ghostty's `filter::linear`, `[[source:
    /// ghostty-shaders-metal]]`, given, 0.98 and 0.72), under the bar on one of each
    /// pair, so it is the interim choice and an open question
    /// (`docs/record.md`, "kitty placements draw what they store"). At one
    /// drawn pixel a source pixel it is `off + d`, so an unscaled image is
    /// copied. Integer arithmetic, so the CPU and the GPU, which reads a
    /// table of these, cannot round apart.
    pub fn nearest(d: u32, dest: u32, off: u32, src: u32) -> u32 {
        let q = match dest {
            0 => 0,
            n => (2 * u64::from(d) + 1) * u64::from(src) / (2 * u64::from(n)),
        };
        off.saturating_add(u32::try_from(q).unwrap_or(u32::MAX))
    }
}

/// `a * b`, saturating at `u32::MAX`.
fn mul_px(a: u32, b: u32) -> u32 {
    u32::try_from(u64::from(a) * u64::from(b)).unwrap_or(u32::MAX)
}

/// `a * b / c` to the nearest integer, a half up; zero when `c` is.
fn scale_px(a: u32, b: u32, c: u32) -> u32 {
    match c {
        0 => 0,
        c => {
            let (a, b, c) = (u64::from(a), u64::from(b), u64::from(c));
            u32::try_from((2 * a * b + c) / (2 * c)).unwrap_or(u32::MAX)
        }
    }
}

impl KittyPlace {
    /// **What this placement draws** of an `iw` by `ih` image at a
    /// `cell_w` by `cell_h` cell, from the text's "Controlling displayed
    /// image layout" (`[[source: kitty-graphics]]`):
    ///
    /// * the source is "the intersection of the specified rectangle with
    ///   the source image rectangle", a zero width or height the rest of
    ///   the image;
    /// * the image is drawn "from the upper left corner of the current
    ///   cell", offset by `X` and `Y`;
    /// * with neither count, at its own size; with `c` alone, as wide as
    ///   the columns less `X` -- the offset "is not added to the number of
    ///   rows/columns" -- and as tall as the source's aspect makes that,
    ///   to the nearest pixel; `r` alone the same way round; both, fitted
    ///   into the box the counts less the offset make, aspect kept, and
    ///   centred on the axis with room to spare.
    ///
    /// Kitty's practice stretches a placement with both counts to the box
    /// (`[[source: kitty-graphics-c]]` `grman_update_layers`), where the
    /// text letterboxes it; the text is followed. With the cell size
    /// unknown, a box is empty and nothing is drawn.
    pub fn draw(&self, iw: ImagePx, ih: ImagePx, cell_w: ImagePx, cell_h: ImagePx) -> ImageDraw {
        let (iw, ih) = (u32::from(iw.raw()), u32::from(ih.raw()));
        let sx = u32::from(self.src_x.raw()).min(iw);
        let sy = u32::from(self.src_y.raw()).min(ih);
        let rest = |want: ImagePx, all: u32, from: u32| match want.raw() {
            0 => all - from,
            w => u32::from(w).min(all - from),
        };
        let (sw, sh) = (rest(self.src_w, iw, sx), rest(self.src_h, ih, sy));
        let (ox, oy) = (u32::from(self.cell_x.raw()), u32::from(self.cell_y.raw()));
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        let box_w = mul_px(count(self.cols.index()), u32::from(cell_w.raw())).saturating_sub(ox);
        let box_h = mul_px(count(self.rows.index()), u32::from(cell_h.raw())).saturating_sub(oy);
        let (mut x, mut y) = (ox, oy);
        let (w, h) = match self.fit {
            KittyFit::Natural => (sw, sh),
            KittyFit::Cols => (box_w, scale_px(box_w, sh, sw)),
            KittyFit::Rows => (scale_px(box_h, sw, sh), box_h),
            KittyFit::Box => {
                // Wider for its height than the box: fit the width.
                if u64::from(box_w) * u64::from(sh) < u64::from(box_h) * u64::from(sw) {
                    let h = scale_px(box_w, sh, sw);
                    y = y.saturating_add((box_h - h.min(box_h)) / 2);
                    (box_w, h)
                } else {
                    let w = scale_px(box_h, sw, sh);
                    x = x.saturating_add((box_w - w.min(box_w)) / 2);
                    (w, box_h)
                }
            }
        };
        let src = PxRect {
            x: sx,
            y: sy,
            w: sw,
            h: sh,
        };
        // A source of no pixels draws none, whatever box it was given.
        let (w, h) = match src.is_empty() {
            true => (0, 0),
            false => (w, h),
        };
        ImageDraw {
            src,
            dest: PxRect { x, y, w, h },
        }
    }
}

impl KittyPlace {
    /// The rows it shows: [`KittyPlace::rows`] less what a margin scroll
    /// clipped, and at least one, as a delete counts a placement's rows.
    pub fn shown(&self) -> Rows {
        let cut = self.cut_top.index().saturating_add(self.cut_bottom.index());
        Rows::of_count(self.rows.index().saturating_sub(cut).max(1))
    }
}

impl KittyPlace {
    /// No kitty geometry: a Sixel's or an embedder's placement.
    pub const NONE: KittyPlace = KittyPlace {
        id: KittyPlacement::NONE,
        src_x: ImagePx::ZERO,
        src_y: ImagePx::ZERO,
        src_w: ImagePx::ZERO,
        src_h: ImagePx::ZERO,
        cell_x: ImagePx::ZERO,
        cell_y: ImagePx::ZERO,
        cols: Cols(0),
        rows: Rows(0),
        fit: KittyFit::Natural,
        cut_top: Rows(0),
        cut_bottom: Rows(0),
    };
}

/// Whether a kitty delete frees the images it leaves with no placement: the
/// uppercase specifiers "delete the image data as well", the lowercase ones
/// only an image no id names (`[[source: kitty-graphics]]`, "Deleting
/// images").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KittyFree {
    /// Lowercase: placements go, named images stay.
    Placements,
    /// Uppercase: an image left with no placement goes too.
    Data,
}

/// [`ImageStore::shift_rows`] for a plane of byte indices, `stride` bytes
/// a row.
fn shift_byte_rows(
    bytes: &mut [u8],
    stride: usize,
    w: usize,
    (y0, y1): (usize, usize),
    k: usize,
    way: BandMove,
) {
    let len = bytes.len();
    let fits = |y: usize| y * stride + w <= len;
    match way {
        BandMove::Up => {
            for y in y0..y1 {
                match y + k < y1 && fits(y + k) {
                    true => bytes.copy_within((y + k) * stride..(y + k) * stride + w, y * stride),
                    false if fits(y) => bytes[y * stride..y * stride + w].fill(0),
                    false => {}
                }
            }
        }
        BandMove::Down => {
            for y in (y0..y1).rev() {
                match y >= y0 + k && fits(y) {
                    true => bytes.copy_within((y - k) * stride..(y - k) * stride + w, y * stride),
                    false if fits(y) => bytes[y * stride..y * stride + w].fill(0),
                    false => {}
                }
            }
        }
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
    /// Which protocol stored it, and the client's names for a kitty image.
    owner: ImageOwner,
    /// Whether its pixels are all written.
    state: ImageState,
    /// The store's epoch when it was stored: which of two images with one
    /// kitty number is the newer.
    born: ImageGen,
    /// How many of a Sixel's or an embedder's pixels are not clear, counted
    /// the first time an erase reaches it and lowered by what each erase
    /// clears, so freeing the image once none is left reads no pixel twice
    /// ([`ImageStore::erase`]). `None` until counted. **A ReGIS page's is
    /// kept from the start**, zero when stored and moved by every write to
    /// its pixels by what that write changed in the pixels it touched: a
    /// repaint ([`ImageStore::repaint_page`]), text over it, an erase, a
    /// band's move. So a page with nothing left lit is freed without a
    /// pixel read twice.
    lit: Option<u32>,
}

impl ImageSlot {
    /// A free slot.
    pub const EMPTY: ImageSlot = ImageSlot {
        offset: 0,
        width: ImagePx::ZERO,
        height: ImagePx::ZERO,
        alpha: ImageAlpha::OpaqueOrClear,
        placements: Count::ZERO,
        owner: ImageOwner::Local,
        state: ImageState::Ready,
        born: ImageGen::ZERO,
        lit: None,
    };

    fn is_empty(self) -> bool {
        self.width.is_zero()
    }

    /// Whether this image goes with its last placement: a local one, or a
    /// kitty one no id names.
    fn goes_with_placements(self) -> bool {
        match self.owner {
            ImageOwner::Local | ImageOwner::Regis => true,
            ImageOwner::Kitty(id, _) => id.is_none(),
        }
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
#[derive(Clone, Copy, PartialEq, Eq, Debug, FromZeros)]
#[repr(u8)]
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
    /// The earliest line past a placement's last row: no placement's
    /// rows all lie before it, so a scroll whose oldest retained line is
    /// not past it drops nothing. A new placement widens it by its
    /// anchor's row alone, which only lowers it, and a scan sets it from
    /// the rows each is drawn over ([`ImageStore::rows_over`]).
    ends: LineIdx,
}

/// What a band move does to a kitty placement: leaves it, moves it with
/// what is left of it, or takes it.
enum Clip {
    Stays,
    Moves(LineIdx, KittyPlace),
    Goes,
}

/// [`ImageStore::scroll_band`]'s kitty rule for placement `s` in the band
/// `[lo, hi)` moving `n` rows `way`: moved only if its shown rows are all
/// in the band, and clipped to it.
fn clipped(s: PlacementSlot, lo: LineIdx, hi: LineIdx, n: Rows, way: BandMove) -> Clip {
    let shown = u64::try_from(s.kitty.shown().index()).unwrap_or(u64::MAX);
    let (top, lo, hi) = (s.line.value(), lo.value(), hi.value());
    let bottom = top.saturating_add(shown);
    if top < lo || bottom > hi {
        return Clip::Stays;
    }
    let n = u64::try_from(n.index()).unwrap_or(u64::MAX);
    let rows = |v: u64| Rows::of_count(usize::try_from(v).unwrap_or(usize::MAX));
    let mut k = s.kitty;
    match way {
        BandMove::Up => {
            if bottom.saturating_sub(n) <= lo {
                return Clip::Goes;
            }
            let to = top.saturating_sub(n);
            let cut = lo.saturating_sub(to);
            k.cut_top = rows(u64::try_from(k.cut_top.index()).unwrap_or(0) + cut);
            Clip::Moves(LineIdx::of(to.max(lo)), k)
        }
        BandMove::Down => {
            let to = top.saturating_add(n);
            if to >= hi {
                return Clip::Goes;
            }
            let cut = to.saturating_add(shown).saturating_sub(hi);
            k.cut_bottom = rows(u64::try_from(k.cut_bottom.index()).unwrap_or(0) + cut);
            Clip::Moves(LineIdx::of(to), k)
        }
    }
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
#[derive(Clone, Copy, PartialEq, Eq, FromZeros)]
#[repr(u8)]
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
#[derive(Clone, Copy, FromZeros)]
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
    /// A kitty placement's geometry and id; [`KittyPlace::NONE`] for any
    /// other.
    pub kitty: KittyPlace,
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
        kitty: KittyPlace::NONE,
    };

    fn is_empty(self) -> bool {
        self.image.is_none()
    }

    /// **`n` free slots from zeroed memory**: [`PlacementSlot::EMPTY`] is
    /// all zero bytes, so the table comes from the allocator's zeroed pages
    /// and a page is committed only when a placement first lands on it --
    /// a table sized to a 400x150 screen, 60,000 slots, is 7.2 MB of
    /// address space and none of it resident until used, where writing
    /// `EMPTY` into each slot would make it all resident at construction.
    /// Allocation failure is `vec!`'s, as [`Pixel::zeroed_vec`]'s is.
    #[cfg(feature = "alloc")]
    pub fn zeroed_vec(n: usize) -> alloc::vec::Vec<PlacementSlot> {
        match PlacementSlot::new_vec_zeroed(n) {
            Ok(v) => v,
            Err(_) => match core::alloc::Layout::array::<PlacementSlot>(n) {
                Ok(layout) => alloc::alloc::handle_alloc_error(layout),
                Err(_) => panic!("capacity overflow"),
            },
        }
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

    /// **The pixels of the kitty query probe that go with it**
    /// ([`ImageStore::with_probe`]): [`KITTY_PROBE`] when there is an arena
    /// at all, none when there is not, so a terminal that draws no images
    /// answers a query as it answers a transmission.
    pub fn probe(self) -> usize {
        match self.pixels {
            0 => 0,
            _ => KITTY_PROBE,
        }
    }

    /// **The bytes of kitty PNG scratch that go with it: eight a pixel.** A
    /// PNG waits whole in the scratch with its inflated scanlines after it
    /// (`[[source: w3c-png-2003]]`'s filtered rows, a byte more each): an
    /// 8-bit RGBA image the size of the whole arena is four bytes a pixel of
    /// scanlines, and its compressed file is smaller than that in all but a
    /// stored stream, so eight holds the largest picture the arena does.
    /// A 16-bit RGBA one that large needs more and is refused for it,
    /// counted and answered.
    pub fn scratch(self) -> usize {
        self.pixels.saturating_mul(8)
    }
}

/// **The kitty query probe, 64 x 64 pixels (16 KiB)**: where a query
/// (`a=q`) decodes, outside the arena and its quota, because the text says
/// a query "will try to load the image and respond with either OK or an
/// error ... but it will not replace an existing image with the same id,
/// nor will it store the image" (`[[source: kitty-graphics]]`, "Querying
/// support and available transmission mediums"). Kitten's detection sends a
/// 1 x 1 query a medium (`[[source: kitty-icat-detect]]`); 64 x 64 holds
/// that and a dummy the size of an icon, at a sixty-fourth of the app's
/// smallest arena. A larger query decodes in the arena if a gap holds it,
/// deleting nothing, and is answered ENOSPC if none does (Jev,
/// QUERY_SMALL_SCRATCH, 0.98 and 0.97).
pub const KITTY_PROBE: usize = 64 * 64;

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

/// **How an embedder gives freed pages back to the operating system**: its
/// page size and a function handed whole, page-aligned pages of memory no
/// live image holds, which it may unmap or advise away. The store calls it
/// when an image's span is freed and the kitty stream when a PNG's use of
/// its scratch ends ([`ImageStore::set_release`]); the core stays `no_std`
/// and free of `unsafe`, and the system call is the embedder's.
///
/// **What the pages hold afterwards is unspecified, and the function
/// may fail.** Linux's `MADV_DONTNEED` reads zero "after a successful"
/// call on a private anonymous mapping and says nothing of a failed one
/// (`[[source: linux-madvise-2]]`); Darwin's `mmap.2` says what a
/// successful `MAP_FIXED` replaces and nothing of what a failed one
/// leaves (`[[source: darwin-mmap-2]]`); `MADV_FREE_REUSABLE` left every
/// page reading its old bytes, measured on this Mac, and is not what
/// baud-app calls. Nothing in the store reads a byte of a freed span
/// before writing it -- a reserve "holds whatever the arena held", a
/// Sixel clears each row as it reaches it, a kitty load short of its
/// pixels is freed -- so the store assumes nothing and re-clears nothing,
/// and tests lend a function that fills what it is handed with a pattern
/// no image wrote and one that fails and leaves the old picture.
///
/// **Which pages are handed: those that just became wholly free.** When a
/// span is freed, the whole pages that touch it and that no live span and
/// no open Sixel claim touches; so a page two neighbours share goes back
/// with the second of them, a page may be handed again without an image
/// writing it in between, when a span reserved and never written is freed
/// (a kitty load refused at once), and a page past the arena's ends is
/// never handed. The page size is the embedder's because a page smaller
/// than the system's would have the call round outward on Linux, where
/// `madvise` rounds its length up, and zero a neighbour.
#[derive(Clone, Copy)]
pub struct PageRelease {
    page: usize,
    give_back: fn(&mut [u8]) -> PagesBack,
}

/// **What a [`PageRelease`]'s function did with the pages it was handed**:
/// gave them back, or kept them because the system call failed. Only
/// pages given back count in [`ImageStore::bytes_released`]; kept ones
/// hold what they held, which the store never reads before writing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PagesBack {
    /// The system took them.
    Released,
    /// The call failed: the pages are as they were, or as the system left
    /// them.
    Kept,
}

impl PageRelease {
    /// Pages of `page` bytes, given back through `give_back`; `None`
    /// unless `page` is a power of two.
    pub fn new(page: usize, give_back: fn(&mut [u8]) -> PagesBack) -> Option<PageRelease> {
        page.is_power_of_two()
            .then_some(PageRelease { page, give_back })
    }

    /// The page size this release rounds to.
    pub fn page(self) -> usize {
        self.page
    }

    /// Give back the whole pages strictly inside `span`, and say how many
    /// bytes the system took. Off the per-byte path: once per freed span.
    #[cold]
    #[inline(never)]
    fn release(self, span: &mut [u8]) -> usize {
        // `align_offset` may answer `usize::MAX` where alignment cannot be
        // computed; `get_mut` then hands nothing.
        let lo = span.as_ptr().align_offset(self.page);
        let Some(rest) = span.get_mut(lo..) else {
            return 0;
        };
        let n = rest.len() - rest.len() % self.page;
        match n {
            0 => 0,
            _ => match (self.give_back)(&mut rest[..n]) {
                PagesBack::Released => n,
                PagesBack::Kept => 0,
            },
        }
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
/// and the placement table, with a fourth, the kitty query probe, outside
/// the arena ([`ImageStore::with_probe`]); all are refuse-and-count, and the counters are
/// public and never silent, the [`ClusterStore`] contract. The one exception is
/// the kitty quota: a new kitty image deletes older kitty images without
/// placements to make space before it is refused
/// ([`ImageStore::reserve_evicting`], counted in `images_evicted`).
pub struct ImageStore<'b> {
    pixels: &'b mut [Pixel],
    /// The kitty query probe ([`ImageStore::with_probe`]): outside the
    /// arena, so a query is answered on a full one.
    probe: &'b mut [Pixel],
    /// The size of the query decoding in the probe, `None` while it is free.
    probe_size: Option<(ImagePx, ImagePx)>,
    slots: &'b mut [ImageSlot],
    placements: &'b mut [PlacementSlot],
    /// **The kitty placements by image and placement id**
    /// ([`ImageStore::find_kitty`]): open addressing over the lent index's
    /// largest power-of-two prefix, linear probing, and a removal that
    /// shifts the entries after it back, so no tombstone lengthens a probe
    /// as a client erases and places again.
    index: &'b mut [PlacementId],
    /// The index's length less one, a power of two less one; zero with no
    /// usable index.
    index_mask: usize,
    /// How many placements the store holds: the table's length, or half
    /// the index's usable length if that is fewer, so the index is never
    /// more than half full.
    place_cap: usize,
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
    /// Bytes of freed spans and spent PNG scratch the embedder's
    /// [`PageRelease`] gave back to the operating system: whole pages it
    /// reported [`PagesBack::Released`], so a page it kept on a failed call
    /// adds nothing. An address range, not a resident size: a page handed
    /// that no image had written since it last went back counts too. Zero
    /// while none is lent.
    pub bytes_released: usize,
    /// Kitty images deleted to make space for a new one: the oldest with no
    /// placement first, "to make space for the new one"
    /// (`[[source: kitty-graphics]]`, "Image persistence and storage
    /// quotas"). Each is in `images_dropped` too.
    pub images_evicted: usize,
    /// Placements removed because their rows no longer exist: the last of
    /// them left the retained history or the alternate screen's top, a
    /// scroll blanked the anchor's as it came in, a reflow dropped its
    /// line, or its screen was the alternate one and was left. Never
    /// silent -- an image that vanished has a count.
    pub placements_expired: usize,
    /// **Index entries examined finding a kitty placement by its image
    /// and placement id**, for a put that replaces and a delete that names
    /// one: a step or two each, where a walk of the table was one a
    /// placement already made.
    pub key_probes: usize,
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
    /// **One past the highest placement slot that can be live**: every
    /// slot at or above it is free, so every walk of the table stops here
    /// and costs the placements a client made, not the table's length. A
    /// table sized to the screen (60,000 slots for a 400x150 one) walked
    /// whole on every put, delete, scroll and frame would cost a
    /// per-cell picture's 660 puts 40 million slot visits. Raised by a
    /// placement past it, lowered past the free slots below it whenever a
    /// slot is freed ([`ImageStore::trim`]), zero with nothing placed.
    live: usize,
    /// **No slot below it is free**: where the search for a free slot
    /// starts, so filling the table one put after another is a step a put
    /// rather than a walk over every slot already taken. Raised past a
    /// slot a placement takes, lowered to any slot freed below it.
    free_from: usize,
    /// **How many live placements show a local image**, a Sixel's or the
    /// embedder's, or a ReGIS page: the ones text written over erases, a
    /// local image whole and a page a cell at a time. The gate for
    /// [`ImageStore::overwrite`], so a screen of kitty placements -- yazi's
    /// preview puts 660, and redraws its file list in the same rows --
    /// pays one compare a write, not a walk of them all.
    local_placed: usize,
    /// The ReGIS page being drawn and the plane of indices it is drawn
    /// from ([`ImageStore::link_page`]), so text written over the page
    /// clears both.
    page_plane: Option<(ImageId, ImageId)>,
    /// Each screen's [`Reach`], `None` when nothing was placed on it since
    /// the last scan found it empty.
    reach: [Option<Reach>; 2],
    /// The tallest image stored since the store was last reset, in pixels:
    /// with the cell height, how far below its anchor any image can reach.
    /// Widened by every store and never narrowed -- a bound, like `reach`.
    tallest: ImagePx,
    /// The cell height in device pixels, which a Sixel's or an
    /// embedder's image is drawn over `ceil(h / cell_h)` rows of; zero
    /// until the embedder says, and then one row.
    cell_h: ImagePx,
    /// The embedder's way to give freed pages back, `None` until lent.
    release: Option<PageRelease>,
    /// What the open Sixel claim has written, words `[start, end)` of the
    /// arena: a gap the slot table does not record, so a page it touches
    /// is not given back when a span beside it is freed ([`SixelStream`]).
    /// Only the written words: a page past them that a free makes wholly
    /// free goes back then, and the Sixel writes it again if it grows.
    claim: Option<(usize, usize)>,
}

impl<'b> ImageStore<'b> {
    /// A store over caller memory: `pixels` the arena, `slots` the
    /// image-slot table, `placements` the placement table and `index` its
    /// index, [`PlacementId::index_len`] of the table's length, all
    /// [`PlacementId::NONE`]. A shorter index holds fewer placements: half
    /// its largest power-of-two prefix.
    pub fn new(
        pixels: &'b mut [Pixel],
        slots: &'b mut [ImageSlot],
        placements: &'b mut [PlacementSlot],
        index: &'b mut [PlacementId],
    ) -> ImageStore<'b> {
        let usable = match index.len() {
            0 | 1 => 0,
            n => 1 << n.ilog2(),
        };
        let place_cap = placements.len().min(usable / 2);
        ImageStore {
            pixels,
            probe: Default::default(),
            probe_size: None,
            slots,
            placements,
            index,
            index_mask: usable.saturating_sub(1),
            place_cap,
            images_refused: 0,
            refusals: ImageRefusals::NONE,
            placements_refused: 0,
            images_dropped: 0,
            bytes_released: 0,
            images_evicted: 0,
            placements_expired: 0,
            key_probes: 0,
            sixel_p2_ignored: 0,
            epoch: ImageGen::ZERO,
            placed: 0,
            live: 0,
            free_from: 0,
            local_placed: 0,
            page_plane: None,
            reach: [None; 2],
            tallest: ImagePx::ZERO,
            cell_h: ImagePx::ZERO,
            release: None,
            claim: None,
        }
    }

    /// **Lend the way freed pages go back to the operating system**: from
    /// here every freed image's span and every spent PNG scratch hands its
    /// whole pages to `release` ([`PageRelease`]). An embedder setting,
    /// which `RIS` keeps.
    pub fn set_release(&mut self, release: PageRelease) {
        self.release = Some(release);
    }

    /// Hand the whole pages inside `span`, memory no live image holds, to
    /// the lent release, and count those it gave back: the kitty PNG
    /// scratch, outside the arena.
    pub(crate) fn release_span(&mut self, span: &mut [u8]) {
        if let Some(r) = self.release {
            self.bytes_released += r.release(span);
        }
    }

    /// **Free slot `i`**: the slot emptied and the pages that became
    /// wholly free given back ([`ImageStore::release_free`]). The one place
    /// a slot is emptied, so no free keeps its pages. The caller counts and
    /// moves the epoch.
    fn forget(&mut self, i: usize) {
        let Some(s) = self.slots.get_mut(i) else {
            return;
        };
        let (start, end) = (s.start(), s.end());
        *s = ImageSlot::EMPTY;
        // A slot reused must not be taken for the page being drawn or its
        // plane, whose indices text written over the page clears.
        if self
            .page_plane
            .is_some_and(|(page, plane)| page.index() == Some(i) || plane.index() == Some(i))
        {
            self.page_plane = None;
        }
        self.release_free(start, end);
    }

    /// Record what the open Sixel claim has written, words `[start, end)`,
    /// or none.
    pub(crate) fn set_claim(&mut self, claim: Option<(usize, usize)>) {
        self.claim = claim;
    }

    /// **Give back the whole pages that touch words `[lo, hi)` of the
    /// arena and that no live span and no open claim touches**: called
    /// once those words stop being used, so each page handed is one that
    /// just became wholly free. A page partly outside the arena is never
    /// handed; it is the allocator's neighbour's too. Off the per-byte
    /// path: once per freed span, with one call to the embedder for each
    /// run of pages between what is still live.
    #[cold]
    #[inline(never)]
    pub(crate) fn release_free(&mut self, lo: usize, hi: usize) {
        let Some(r) = self.release else {
            return;
        };
        let page = r.page();
        let px = core::mem::size_of::<Pixel>();
        let len = core::mem::size_of_val(self.pixels);
        // Byte `lead + k * page` of the arena starts a page.
        let lead = self.pixels.as_ptr().cast::<u8>().align_offset(page);
        if lead >= len || hi <= lo {
            return;
        }
        let top = lead + (len - lead) / page * page;
        let down = |b: usize| match b < lead {
            true => 0,
            false => lead + (b - lead) / page * page,
        };
        let up = |b: usize| match b <= lead {
            true => lead,
            false => lead + (b - lead).div_ceil(page) * page,
        };
        let first = down(lo * px).max(lead);
        let last = up(hi * px).min(top);
        let mut at = first;
        while at < last {
            // What is still in use, each rounded out to its pages: the one
            // overlapping `[at, last)` that starts lowest.
            let next = self
                .slots
                .iter()
                .filter(|s| !s.is_empty())
                .map(|s| (s.start(), s.end()))
                .chain(self.claim)
                // A claim that has written nothing keeps no page.
                .filter(|&(s, e)| e > s)
                .map(|(s, e)| (down(s * px), up(e * px)))
                .filter(|&(s, e)| e > at && s < last)
                .min_by_key(|&(s, _)| s);
            let to = next.map_or(last, |(s, _)| s);
            if to > at {
                if let Some(span) = self.pixels.as_mut_bytes().get_mut(at..to) {
                    self.bytes_released += r.release(span);
                }
            }
            at = match next {
                Some((_, e)) => e.max(at),
                None => last,
            };
        }
    }

    /// **Give the store a kitty query probe** ([`KITTY_PROBE`] pixels,
    /// [`ImageBudget::probe`]): caller memory beside the arena, never counted
    /// against it, never shown, never placed. Without one every query
    /// decodes in the arena.
    pub fn with_probe(mut self, probe: &'b mut [Pixel]) -> ImageStore<'b> {
        self.probe = probe;
        self
    }

    /// **Reserve the probe for a `width` x `height` query**: an id that
    /// [`ImageStore::decoding_mut`] lends the probe's first `width * height`
    /// pixels under, [`ImageStore::size_of`] sizes and [`ImageStore::free`]
    /// releases. It takes no slot, moves no epoch and frees nothing: the
    /// store is as it was once the query is answered. `None` if the query
    /// is larger than the probe or the probe is taken.
    pub fn reserve_probe(&mut self, width: ImagePx, height: ImagePx) -> Option<ImageId> {
        let area = width.index().checked_mul(height.index())?;
        if area == 0 || area > self.probe.len() || self.probe_size.is_some() {
            return None;
        }
        self.probe_size = Some((width, height));
        Some(ImageId::PROBE)
    }

    /// Widen `screen`'s reach to hold a placement anchored at `line` whose
    /// last shown row ends before `ends`.
    fn widen(&mut self, screen: ImageScreen, line: LineIdx, ends: LineIdx) {
        let r = &mut self.reach[screen.slot()];
        *r = Some(match *r {
            None => Reach {
                first: line,
                last: line,
                ends,
            },
            Some(Reach {
                first,
                last,
                ends: e,
            }) => Reach {
                first: first.min(line),
                last: last.max(line),
                ends: e.min(ends),
            },
        });
    }

    /// Recompute `screen`'s reach from its live placements, exactly.
    fn rescan(&mut self, screen: ImageScreen) {
        self.reach[screen.slot()] = None;
        for i in 0..self.live {
            let p = self.placements[i];
            if !p.is_empty() && p.screen == screen {
                self.widen(screen, p.line, p.line.plus(self.rows_over(p)));
            }
        }
    }

    /// **The rows placement `p` is drawn over, from its anchor down**: a
    /// kitty placement's shown rows ([`KittyPlace::shown`]), and a
    /// Sixel's or an embedder's `ceil(h / cell_h)` at the cell height
    /// [`ImageStore::set_cell_height`] last gave, the rows text written
    /// over it counts ([`ImageStore::overwrite`]); one while no height
    /// was given.
    fn rows_over(&self, p: PlacementSlot) -> Rows {
        let ch = self.cell_h.index();
        match p.image.index().and_then(|i| self.slots.get(i)) {
            Some(img) if matches!(img.owner, ImageOwner::Local | ImageOwner::Regis) && ch != 0 => {
                Rows::of_count(img.height.index().div_ceil(ch).max(1))
            }
            _ => p.kitty.shown(),
        }
    }

    /// **Tell the store the cell height in device pixels**, which sizes
    /// the rows a Sixel's or an embedder's image is drawn over
    /// ([`ImageStore::rows_over`]). Each screen's reach is recounted, so
    /// a taller cell, fewer rows, is seen by the next
    /// [`ImageStore::expire`]. An embedder setting, which `RIS` keeps.
    pub fn set_cell_height(&mut self, h: ImagePx) {
        self.cell_h = h;
        self.rescan(ImageScreen::Primary);
        self.rescan(ImageScreen::Alternate);
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
        self.epoch = self.epoch.bumped();
        self.slots[slot] = ImageSlot {
            offset,
            width,
            height,
            alpha,
            placements: Count::ZERO,
            owner: ImageOwner::Local,
            state: ImageState::Ready,
            born: self.epoch,
            lit: None,
        };
        self.tallest = self.tallest.max(height);
        Some(ImageId::from_index(slot))
    }

    /// **Reserve a `width` x `height` image for a decoder that writes it
    /// later**, as `owner`: recorded at once, so nothing else is stored over
    /// its span, and [`ImageState::Decoding`], so [`ImageStore::image`] does
    /// not show it until [`ImageStore::complete`]. Refused and counted as
    /// [`ImageStore::store`] refuses. The span holds whatever the arena held;
    /// [`ImageStore::decoding_mut`] lends it.
    pub fn reserve(
        &mut self,
        width: ImagePx,
        height: ImagePx,
        owner: ImageOwner,
    ) -> Option<ImageId> {
        let area = match width.index().checked_mul(height.index()) {
            Some(a) if a > 0 => a,
            _ => {
                self.refuse(ImageRefusal::Empty);
                return None;
            }
        };
        let at = self.lowest_gap(area);
        let id = self.store_at(at, width, height, ImageAlpha::OpaqueOrClear)?;
        let s = &mut self.slots[id.index()?];
        s.owner = owner;
        s.state = ImageState::Decoding;
        Some(id)
    }

    /// **Reserve a kitty image, deleting older ones to make space**:
    /// "When adding a new image, if the total size exceeds the quota, the
    /// terminal emulator should delete older images to make space for the
    /// new one", and "existing images without placements will be
    /// preferentially deleted" (`[[source: kitty-graphics]]`, "Image
    /// persistence and storage quotas" and "Deleting images"). While no
    /// slot is free or no gap holds `width` x `height`, the oldest kitty
    /// image with no placement, on screen or in the history, and whole
    /// goes, each counted in `images_evicted`; only if deleting all of them
    /// would not make space does none go, and the reserve is refused and
    /// counted once, as [`ImageStore::reserve`] refuses. An image with a
    /// placement, one still decoding, and a Sixel or embedder image are
    /// never deleted for space.
    pub fn reserve_evicting(
        &mut self,
        width: ImagePx,
        height: ImagePx,
        owner: ImageOwner,
    ) -> Option<ImageId> {
        let area = width.index().saturating_mul(height.index());
        let fits = |st: &ImageStore, live: &dyn Fn(&ImageSlot) -> bool| {
            st.slots.iter().any(|s| !live(s)) && st.lowest_gap_among(area, live).is_some()
        };
        let evictable = |s: &ImageSlot| {
            !s.is_empty()
                && s.placements.index() == 0
                && s.state == ImageState::Ready
                && matches!(s.owner, ImageOwner::Kitty(..))
        };
        let taken = |s: &ImageSlot| !s.is_empty();
        if area > 0 && !fits(self, &taken) && fits(self, &|s| taken(s) && !evictable(s)) {
            while !fits(self, &taken) {
                let Some(old) = self
                    .slots
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| evictable(s))
                    .min_by_key(|(_, s)| s.born.raw())
                    .map(|(i, _)| ImageId::from_index(i))
                else {
                    break;
                };
                self.free(old);
                self.images_evicted += 1;
            }
        }
        self.reserve(width, height, owner)
    }

    /// The pixels of an image still being decoded, to write in place;
    /// `None` once it is complete or if `id` names none.
    pub fn decoding_mut(&mut self, id: ImageId) -> Option<&mut [Pixel]> {
        if id == ImageId::PROBE {
            let (w, h) = self.probe_size?;
            return self.probe.get_mut(..w.index() * h.index());
        }
        let s = *self.slots.get(id.index()?)?;
        match !s.is_empty() && s.state == ImageState::Decoding {
            true => Some(&mut self.pixels[s.start()..s.end()]),
            false => None,
        }
    }

    /// **Mark an image's pixels complete, with the mark they earn**: a
    /// decoding image starts showing, and an embedder's image is held to its
    /// pixels rather than its caller's word. The epoch moves, so a texture
    /// cache keyed on it uploads the pixels now there.
    pub fn complete(&mut self, id: ImageId, alpha: ImageAlpha) {
        // The probe is never shown, so completing it changes nothing.
        let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) else {
            return;
        };
        if s.is_empty() {
            return;
        }
        s.state = ImageState::Ready;
        s.alpha = alpha;
        self.epoch = self.epoch.bumped();
    }

    /// The size of a live image, decoding or not.
    pub fn size_of(&self, id: ImageId) -> Option<(ImagePx, ImagePx)> {
        if id == ImageId::PROBE {
            return self.probe_size;
        }
        let s = *self.slots.get(id.index()?)?;
        (!s.is_empty()).then_some((s.width, s.height))
    }

    /// The live image a kitty id names.
    pub fn kitty_image(&self, id: KittyId) -> Option<ImageId> {
        if id.is_none() {
            return None;
        }
        self.slots
            .iter()
            .position(|s| !s.is_empty() && matches!(s.owner, ImageOwner::Kitty(i, _) if i == id))
            .map(ImageId::from_index)
    }

    /// The newest live image a kitty number names.
    pub fn kitty_newest(&self, number: KittyNumber) -> Option<ImageId> {
        if number.is_none() {
            return None;
        }
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| {
                !s.is_empty() && matches!(s.owner, ImageOwner::Kitty(_, n) if n == number)
            })
            .max_by_key(|(_, s)| s.born.raw())
            .map(|(i, _)| ImageId::from_index(i))
    }

    /// The kitty id and number of a live image; `None` for one no kitty
    /// transmission stored.
    pub fn kitty_names(&self, id: ImageId) -> Option<(KittyId, KittyNumber)> {
        let s = *self.slots.get(id.index()?)?;
        match (s.is_empty(), s.owner) {
            (false, ImageOwner::Kitty(i, n)) => Some((i, n)),
            _ => None,
        }
    }

    /// **Place a kitty image**, replacing the placement of the same image
    /// with the same non-zero placement id: "If you send two placements with
    /// the same image id and placement id the second one will replace the
    /// first" (`[[source: kitty-graphics]]`, "Display images on screen"). The
    /// old one goes first, so the image is not freed between them.
    pub fn place_kitty(
        &mut self,
        id: ImageId,
        screen: ImageScreen,
        line: LineIdx,
        col: Col,
        z: ZIndex,
        place: KittyPlace,
    ) -> PlacementId {
        if let Some(p) = self.find_kitty(id, place.id) {
            self.vacate(p);
            if let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) {
                s.placements = Count::of(s.placements.index().saturating_sub(1));
            }
        }
        let pid = self.place(id, screen, line, col, z);
        if let Some(p) = pid_index(pid) {
            self.placements[p].kitty = place;
            if !place.id.is_none() {
                self.index_insert(p);
            }
        }
        pid
    }

    /// **Delete the kitty placement of `image` with placement id `pid`**,
    /// found by the pair, and the image with it as
    /// [`ImageStore::kitty_delete`] would: a delete with `i` and `p`, or
    /// `I` and `p`, takes "only the placement with the specified image id
    /// and placement id" (`[[source: kitty-graphics]]`, "Deleting
    /// images"). Returns how many placements went, one or none.
    pub fn kitty_delete_placement(
        &mut self,
        free: KittyFree,
        image: ImageId,
        pid: KittyPlacement,
    ) -> usize {
        let Some(p) = self.find_kitty(image, pid) else {
            return 0;
        };
        let Some(i) = image.index() else {
            return 0;
        };
        self.vacate(p);
        self.drop_ref(i, free);
        self.trim();
        1
    }

    /// One placement of image `i` gone: its count lowered, and the image
    /// freed with its last as `free` or its namelessness says, kitty's
    /// `filter_refs` (`[[source: kitty-graphics-c]]`).
    fn drop_ref(&mut self, i: usize, free: KittyFree) {
        let s = &mut self.slots[i];
        s.placements = Count::of(s.placements.index().saturating_sub(1));
        if s.placements.index() == 0 && (free == KittyFree::Data || s.goes_with_placements()) {
            self.forget(i);
            self.images_dropped += 1;
            self.epoch = self.epoch.bumped();
        }
    }

    /// The index entry a kitty placement's pair starts its probe at: the
    /// high bits of a multiplicative hash of the image and placement ids,
    /// as `StyleId::bucket` takes them, since a client's
    /// ids run in sequence and low bits would collide them.
    fn bucket(&self, image: ImageId, pid: KittyPlacement) -> usize {
        let key = u64::from(image.0) << 32 | u64::from(pid.raw());
        let bits = self.index_mask.count_ones();
        match bits {
            0 => 0,
            b => usize::try_from(key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - b)).unwrap_or(0),
        }
    }

    /// The pair a placement slot is indexed by.
    fn key_of(&self, p: usize) -> (ImageId, KittyPlacement) {
        let s = &self.placements[p];
        (s.image, s.kitty.id)
    }

    /// **The slot of the kitty placement of `image` with id `pid`**, from
    /// the index: the probe from the pair's bucket ends at it or at an
    /// empty entry, each entry examined counted in
    /// [`ImageStore::key_probes`]. `None` for a placement id of none,
    /// which names no placement.
    fn find_kitty(&mut self, image: ImageId, pid: KittyPlacement) -> Option<usize> {
        if pid.is_none() || self.index_mask == 0 {
            return None;
        }
        let mut j = self.bucket(image, pid);
        // Bounded by the index, which is never more than half full: the
        // probe meets an empty entry long before the bound.
        for _ in 0..=self.index_mask {
            self.key_probes += 1;
            let p = pid_index(self.index[j])?;
            if self.key_of(p) == (image, pid) {
                return Some(p);
            }
            j = (j + 1) & self.index_mask;
        }
        None
    }

    /// Index slot `p`, a kitty placement with a placement id that no other
    /// live placement of its image has.
    fn index_insert(&mut self, p: usize) {
        let (image, pid) = self.key_of(p);
        let mut j = self.bucket(image, pid);
        for _ in 0..=self.index_mask {
            if self.index[j].is_none() {
                self.index[j] = PlacementId::from_index(p);
                return;
            }
            j = (j + 1) & self.index_mask;
        }
    }

    /// **Take slot `p` out of the index** while it still holds its pair,
    /// shifting back each entry after it that its probe passed through, so
    /// every remaining probe still ends where it did.
    fn index_remove(&mut self, p: usize) {
        let (image, pid) = self.key_of(p);
        if pid.is_none() || self.index_mask == 0 {
            return;
        }
        let me = PlacementId::from_index(p);
        let mask = self.index_mask;
        let mut hole = self.bucket(image, pid);
        let mut seen = 0;
        while self.index[hole] != me {
            if self.index[hole].is_none() || seen == mask {
                return;
            }
            hole = (hole + 1) & mask;
            seen += 1;
        }
        let mut k = hole;
        for _ in 0..mask {
            k = (k + 1) & mask;
            let Some(q) = pid_index(self.index[k]) else {
                break;
            };
            let (qi, qp) = self.key_of(q);
            let home = self.bucket(qi, qp);
            // It stays unless its home is no nearer `k` than the hole is.
            if (k.wrapping_sub(home) & mask) >= (k.wrapping_sub(hole) & mask) {
                self.index[hole] = self.index[k];
                hole = k;
            }
        }
        self.index[hole] = PlacementId::NONE;
    }

    /// **Delete the kitty placements `hit` picks**, and with them each
    /// image they leave with no placement that `free` or its namelessness
    /// says goes -- kitty's `filter_refs` (`[[source: kitty-graphics-c]]`):
    /// an image is freed only if a placement of it matched. `hit` sees the
    /// placement and its image's kitty names. Returns how many placements
    /// went. Sixel and embedder placements are never touched: the delete
    /// command is kitty's.
    pub fn kitty_delete(
        &mut self,
        free: KittyFree,
        hit: impl Fn(&PlacementSlot, KittyId, KittyNumber) -> bool,
    ) -> usize {
        let mut n = 0;
        for p in 0..self.live {
            let slot = self.placements[p];
            if slot.is_empty() {
                continue;
            }
            let Some(i) = slot.image.index() else {
                continue;
            };
            let ImageOwner::Kitty(kid, num) = self.slots[i].owner else {
                continue;
            };
            if !hit(&slot, kid, num) {
                continue;
            }
            self.vacate(p);
            n += 1;
            self.drop_ref(i, free);
        }
        self.trim();
        n
    }

    /// **Hold a reference on a live image without a placement on screen**:
    /// a kitty virtual placement (`U=1`), which is kept in the kitty
    /// stream's own table, counts as a placement of its image so the quota
    /// does not delete an image placeholders show, and an uppercase delete
    /// frees it only when it was the last. `false` when `id` names no live
    /// image.
    pub fn hold(&mut self, id: ImageId) -> bool {
        match id.index().and_then(|i| self.slots.get_mut(i)) {
            Some(s) if !s.is_empty() => {
                s.placements = Count::of(s.placements.index() + 1);
                true
            }
            _ => false,
        }
    }

    /// **Let go of a reference [`ImageStore::hold`] took**, freeing the
    /// image when it was its last and `free` or its namelessness says it
    /// goes, as [`ImageStore::kitty_delete`] frees one.
    pub fn release(&mut self, id: ImageId, free: KittyFree) {
        let Some(i) = id.index() else {
            return;
        };
        let Some(s) = self.slots.get_mut(i) else {
            return;
        };
        if s.is_empty() {
            return;
        }
        s.placements = Count::of(s.placements.index().saturating_sub(1));
        if s.placements.index() == 0 && (free == KittyFree::Data || s.goes_with_placements()) {
            // Through `forget`, the one place a slot is emptied, so its
            // pages go back as every other free's do.
            self.forget(i);
            self.images_dropped += 1;
            self.epoch = self.epoch.bumped();
        }
    }

    /// **Free every kitty image `hit` picks that has no placement**: an
    /// `a=d,d=I` or `d=N` naming an image only stored, and `d=R`'s range
    /// (`[[source: kitty-graphics-c]]` `handle_delete_command`, whose
    /// special case frees an unplaced image by id or number). Returns how
    /// many.
    pub fn kitty_free_unplaced(&mut self, hit: impl Fn(KittyId, KittyNumber) -> bool) -> usize {
        let mut n = 0;
        for i in 0..self.slots.len() {
            let s = self.slots[i];
            if s.is_empty() || s.placements.index() != 0 {
                continue;
            }
            let ImageOwner::Kitty(kid, num) = s.owner else {
                continue;
            };
            if hit(kid, num) {
                self.forget(i);
                self.images_dropped += 1;
                n += 1;
            }
        }
        if n > 0 {
            self.epoch = self.epoch.bumped();
        }
        n
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
        if s.state == ImageState::Decoding {
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
            kitty: KittyPlace::NONE,
        };
        self.slots[i].placements = Count::of(self.slots[i].placements.index() + 1);
        self.placed += 1;
        self.live = self.live.max(p + 1);
        self.free_from = p + 1;
        if matches!(self.slots[i].owner, ImageOwner::Local | ImageOwner::Regis) {
            self.local_placed += 1;
        }
        self.widen(screen, line, line.plus(Rows::of_count(1)));
        PlacementId::from_index(p)
    }

    /// **Store a ReGIS page**: a `width` x `height` image, every pixel
    /// [`Pixel::CLEAR`], owned by ReGIS ([`ImageOwner::Regis`]) and drawn on
    /// after it is stored, through [`ImageStore::page_planes`] and
    /// [`ImageStore::repaint_page`]. Refused and counted as
    /// [`ImageStore::store`] refuses.
    pub fn store_page(&mut self, width: ImagePx, height: ImagePx) -> Option<ImageId> {
        let (id, px) = self.store(width, height, ImageAlpha::OpaqueOrClear)?;
        px.fill(Pixel::CLEAR);
        if let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) {
            s.owner = ImageOwner::Regis;
            s.lit = Some(0);
        }
        Some(id)
    }

    /// **A page's pixels and its plane's, both lent at once**: two ReGIS
    /// images, the one shown and the one holding the indices it is drawn
    /// from. `None` unless both are live ReGIS images.
    pub fn page_planes(
        &mut self,
        page: ImageId,
        plane: ImageId,
    ) -> Option<(&mut [Pixel], &mut [u8])> {
        let a = *self.slots.get(page.index()?)?;
        let b = *self.slots.get(plane.index()?)?;
        let regis = |s: &ImageSlot| !s.is_empty() && s.owner == ImageOwner::Regis;
        if !regis(&a) || !regis(&b) || page == plane {
            return None;
        }
        let (ra, rb) = ((a.start(), a.end()), (b.start(), b.end()));
        let (lo, hi, page_first) = match ra.0 < rb.0 {
            true => (ra, rb, true),
            false => (rb, ra, false),
        };
        let (left, right) = self.pixels.split_at_mut(hi.0);
        let first = &mut left[lo.0..lo.1];
        let second = &mut right[..hi.1 - hi.0];
        let (p, q) = match page_first {
            true => (first, second),
            false => (second, first),
        };
        Some((p, q.as_mut_bytes()))
    }

    /// **Repaint a ReGIS page from its plane**: `paint` is lent the page's
    /// pixels and its plane's ([`ImageStore::page_planes`]) and writes the
    /// page's pixels in `rect`, `(x0, y0, x1, y1)`, and nowhere else. The
    /// page's lit count moves by what changed in `rect`, counted before and
    /// after over the pixels the repaint writes anyway, and the epoch moves,
    /// so a renderer's texture cache keyed on it uploads the pixels now
    /// there. Nothing unless both are live ReGIS images.
    pub fn repaint_page(
        &mut self,
        page: ImageId,
        plane: ImageId,
        rect: (usize, usize, usize, usize),
        paint: impl FnOnce(&mut [Pixel], &mut [u8]),
    ) {
        let Some(s) = page.index().and_then(|i| self.slots.get(i)).copied() else {
            return;
        };
        let before = self.lit_in(s, rect);
        let Some((px, pl)) = self.page_planes(page, plane) else {
            return;
        };
        paint(px, pl);
        let after = self.lit_in(s, rect);
        if let Some(slot) = page.index().and_then(|i| self.slots.get_mut(i)) {
            slot.lit = slot
                .lit
                .map(|n| n.saturating_sub(before).saturating_add(after));
        }
        self.epoch = self.epoch.bumped();
    }

    /// **Carry a page's lines still on the screen onto the page that
    /// takes over the screen**: `old`, the page being drawn, whose top line
    /// has scrolled `dy` pixel rows into the history, and `new`, a page of
    /// the same size placed at the window's top. Rows `[dy, h)` of `old`
    /// move to rows `[0, h - dy)` of `new`, and the plane of indices goes
    /// with them, its rows moved up in place and linked to `new`, so a
    /// complement drawing reads the pixels shown where it draws -- "complement
    /// the contents of the pixels present in the image" (070-8
    /// § 8.7.2.4.1.1) -- and the screen's scrolled contents move away
    /// beneath the screen-relative coordinates (§ 8.8.1, the scrolling
    /// model). `old` keeps only its rows in the history, freed if nothing
    /// is left lit there. Both counts are kept over the rows moved. Nothing
    /// unless `old` is the page being drawn and the two are live pages of
    /// one size, and then it returns `false`.
    pub fn carry_page(&mut self, old: ImageId, new: ImageId, dy: usize) -> bool {
        let Some((page, plane)) = self.page_plane else {
            return false;
        };
        let slot = |id: ImageId| id.index().and_then(|i| self.slots.get(i)).copied();
        let (Some(o), Some(n), Some(pl)) = (slot(old), slot(new), slot(plane)) else {
            return false;
        };
        let page_of = |s: ImageSlot| !s.is_empty() && s.owner == ImageOwner::Regis;
        let same = o.width == n.width && o.height == n.height;
        if page != old || !page_of(o) || !page_of(n) || !page_of(pl) || !same || old == new {
            return false;
        }
        let (w, h) = (o.width.index(), o.height.index());
        if dy == 0 || dy >= h {
            return false;
        }
        let stride = w.div_ceil(4) * 4;
        if let Some(bytes) = self.pixels[pl.start()..pl.end()]
            .as_mut_bytes()
            .get_mut(..h * stride)
        {
            bytes.copy_within(dy * stride.., 0);
            bytes[(h - dy) * stride..].fill(0);
        }
        let moved = self.lit_in(o, (0, dy, w, h));
        self.pixels
            .copy_within(o.start() + dy * w..o.end(), n.start());
        self.pixels[o.start() + dy * w..o.end()].fill(Pixel::CLEAR);
        let below = n.start() + (h - dy) * w;
        self.pixels[below..n.end()].fill(Pixel::CLEAR);
        if let Some(i) = new.index() {
            self.slots[i].lit = Some(moved);
        }
        self.unlit(old, moved);
        self.link_page(new, plane);
        self.free_if_unlit(old);
        self.epoch = self.epoch.bumped();
        true
    }

    /// How many pixels of image `s` in `(x0, y0, x1, y1)` are not clear.
    fn lit_in(&self, s: ImageSlot, (x0, y0, x1, y1): (usize, usize, usize, usize)) -> u32 {
        let (w, h) = (s.width.index(), s.height.index());
        let (x0, x1, y1) = (x0.min(w), x1.min(w), y1.min(h));
        let mut n = 0usize;
        for y in y0.min(y1)..y1 {
            let row = s.start() + y * w;
            n += self.pixels[row + x0.min(x1)..row + x1]
                .iter()
                .filter(|p| p.alpha().get() != 0)
                .count();
        }
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    /// **How many pixels of image `id` are lit**, not clear, as the store
    /// keeps it: a ReGIS page's always, a Sixel's once an erase has reached
    /// it. `None` for no live image or one not counted yet.
    pub fn lit(&self, id: ImageId) -> Option<u32> {
        let s = id.index().and_then(|i| self.slots.get(i))?;
        match s.is_empty() {
            true => None,
            false => s.lit,
        }
    }

    /// **Free a ReGIS page with nothing left lit**, and its plane if it is
    /// the page being drawn: the memory both held goes back to the arena,
    /// and the next drawing makes a new page. Returns whether it was freed.
    fn free_if_unlit(&mut self, id: ImageId) -> bool {
        let Some(s) = id.index().and_then(|i| self.slots.get(i)).copied() else {
            return false;
        };
        if s.is_empty() || s.owner != ImageOwner::Regis || s.lit != Some(0) {
            return false;
        }
        if let Some((page, plane)) = self.page_plane {
            if page == id {
                self.free(plane);
            }
        }
        self.free(id);
        true
    }

    /// Lower image `id`'s lit count by `cleared`, where it is kept.
    fn unlit(&mut self, id: ImageId, cleared: u32) {
        if let Some(s) = id.index().and_then(|i| self.slots.get_mut(i)) {
            s.lit = s.lit.map(|n| n.saturating_sub(cleared));
        }
    }

    /// The page being drawn is `page`, drawn from the indices in `plane`.
    pub fn link_page(&mut self, page: ImageId, plane: ImageId) {
        self.page_plane = Some((page, plane));
    }

    /// **Text written over a ReGIS page clears the cells it covers**: row
    /// `row` of the page's cells, columns `[c0, c1)`, back to clear, and the
    /// indices under them if it is the page being drawn. The VT330 and
    /// VT340 draw text and graphics into one bitmap, so a character written
    /// replaces what was drawn in its cell; the rest of the page stays.
    /// Returns how many of the pixels it cleared were not clear before.
    fn clear_page_cells(
        &mut self,
        id: ImageId,
        row: usize,
        c0: usize,
        c1: usize,
        cw: usize,
        ch: usize,
    ) -> u32 {
        let Some(s) = id.index().and_then(|i| self.slots.get(i)).copied() else {
            return 0;
        };
        let (w, h) = (s.width.index(), s.height.index());
        let (x0, x1) = ((c0 * cw).min(w), (c1 * cw).min(w));
        let (y0, y1) = ((row * ch).min(h), ((row + 1) * ch).min(h));
        if x0 >= x1 || y0 >= y1 {
            return 0;
        }
        let at = s.start();
        let mut was_lit = 0u32;
        for y in y0..y1 {
            let span = &mut self.pixels[at + y * w + x0..at + y * w + x1];
            let n = span.iter().filter(|p| p.alpha().get() != 0).count();
            was_lit = was_lit.saturating_add(u32::try_from(n).unwrap_or(u32::MAX));
            span.fill(Pixel::CLEAR);
        }
        if let Some((page, plane)) = self.page_plane {
            if page == id {
                let stride = w.div_ceil(4) * 4;
                if let Some(p) = plane.index().and_then(|i| self.slots.get(i)).copied() {
                    let bytes = self.pixels[p.start()..p.end()].as_mut_bytes();
                    for y in y0..y1 {
                        if let Some(row) = bytes.get_mut(y * stride + x0..y * stride + x1) {
                            row.fill(0);
                        }
                    }
                }
            }
        }
        self.epoch = self.epoch.bumped();
        was_lit
    }

    /// Place a ReGIS page as kitty geometry `place` scales it, at an
    /// absolute line of `screen` and a column.
    pub fn place_page(
        &mut self,
        id: ImageId,
        screen: ImageScreen,
        line: LineIdx,
        col: Col,
        place: KittyPlace,
    ) -> PlacementId {
        let pid = self.place(id, screen, line, col, ZIndex::ZERO);
        if let Some(p) = pid_index(pid) {
            self.placements[p].kitty = place;
        }
        pid
    }

    /// Whether `id` is a live ReGIS image of `width` by `height`.
    pub fn is_page(&self, id: ImageId, width: ImagePx, height: ImagePx) -> bool {
        id.index().and_then(|i| self.slots.get(i)).is_some_and(|s| {
            !s.is_empty() && s.owner == ImageOwner::Regis && s.width == width && s.height == height
        })
    }

    /// Whether image `id` has a placement.
    pub fn is_placed(&self, id: ImageId) -> bool {
        id.index()
            .and_then(|i| self.slots.get(i))
            .is_some_and(|s| !s.is_empty() && s.placements.index() > 0)
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
        self.vacate(p);
        self.trim();
        let Some(i) = image.index() else { return };
        let Some(s) = self.slots.get_mut(i) else {
            return;
        };
        let left = s.placements.index().saturating_sub(1);
        s.placements = Count::of(left);
        if left == 0 && s.goes_with_placements() {
            self.forget(i);
            self.images_dropped += 1;
            self.epoch = self.epoch.bumped();
        }
    }

    /// [`ImageStore::free`] an image only while it is still decoding: what a
    /// decoder that reserved it frees on failure. A slot that was cleared
    /// meanwhile -- `ED 2`, a placement expiring -- may hold another
    /// protocol's image by now, which this leaves alone.
    pub fn free_decoding(&mut self, id: ImageId) {
        if id == ImageId::PROBE {
            self.probe_size = None;
            return;
        }
        let decoding = id
            .index()
            .and_then(|i| self.slots.get(i))
            .is_some_and(|s| !s.is_empty() && s.state == ImageState::Decoding);
        if decoding {
            self.free(id);
        }
    }

    /// Delete an image and every placement of it (the Kitty uppercase delete).
    pub fn free(&mut self, id: ImageId) {
        if id == ImageId::PROBE {
            // Released, and nothing else: no slot, no count, no epoch.
            self.probe_size = None;
            return;
        }
        let Some(i) = id.index() else { return };
        match self.slots.get(i) {
            Some(s) if !s.is_empty() => {}
            _ => return,
        }
        // The placements first, while the slot still says whose image it
        // is, then the slot.
        for p in 0..self.live {
            if self.placements[p].image == id {
                self.vacate(p);
            }
        }
        self.forget(i);
        self.trim();
        self.images_dropped += 1;
        self.epoch = self.epoch.bumped();
    }

    /// Every live placement, for the renderer to draw. Order is slot order, not
    /// `z` order; the renderer sorts by `z`.
    pub fn placements(&self) -> impl Iterator<Item = (PlacementId, &PlacementSlot)> + '_ {
        self.placements[..self.live]
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
        for i in 0..self.slots.len() {
            if !self.slots[i].is_empty() {
                self.forget(i);
            }
        }
        for p in 0..self.live {
            self.index_remove(p);
            self.placements[p] = PlacementSlot::EMPTY;
        }
        self.placed = 0;
        self.live = 0;
        self.free_from = 0;
        self.local_placed = 0;
        self.page_plane = None;
        self.reach = [None; 2];
        self.tallest = ImagePx::ZERO;
        self.probe_size = None;
        self.epoch = self.epoch.bumped();
    }

    /// Whether any placement is live -- the O(1) gate for the write path's
    /// overwrite check, so a stream with no image on screen pays one compare.
    pub fn any_placed(&self) -> bool {
        self.placed > 0
    }

    /// The whole arena, in pixels: the most any one image can hold, which
    /// a declared size is checked against before anything is reserved.
    pub fn capacity(&self) -> usize {
        self.pixels.len()
    }

    /// The query probe, in pixels: the largest query answered without the
    /// arena ([`ImageStore::with_probe`]).
    pub fn probe_capacity(&self) -> usize {
        self.probe.len()
    }

    /// Whether the store has any pixel arena at all, so a Sixel can be drawn
    /// rather than refused for want of memory. What the Primary DA's Sixel
    /// advertisement is gated on: an unprovisioned store (the caller passed no
    /// image budget) refuses every Sixel, and claiming Sixel then would make an
    /// application send images baud draws as nothing.
    pub fn provisioned(&self) -> bool {
        !self.pixels.is_empty()
    }

    /// Drop every placement on `screen` whose rows are gone: one whose last
    /// shown row is before `lo`, one anchored at or past `hi` -- `[lo, hi)`
    /// the rows the screen retains -- or one anchored on `blanked`, a row a
    /// scroll blanked as it came in. Each is counted in
    /// [`ImageStore::placements_expired`]. Returns how many.
    ///
    /// **A kitty placement goes when its last row leaves**, not its anchor:
    /// images "must be scrolled along with text" (`[[source:
    /// kitty-graphics]]`, "Interaction with other terminal actions"), and
    /// the rows below a gone anchor are over text the history still holds.
    /// It is kitty's `start_row + effective_num_rows <= limit`
    /// (`[[source: kitty-graphics-c]]` `scroll_filter_func`, the limit the
    /// history's far end or the alternate screen's top, `[[source:
    /// kitty-screen-c]]` `INDEX_GRAPHICS`), the rows a margin scroll cut
    /// not counted, as [`ImageStore::scroll_band`] keeps one until its last
    /// shown row leaves the band. Its anchor stays where it was, uncut, as
    /// kitty's does: no view reaches above `lo`. Jev decided it at 1.00
    /// both ways (2026-10-18). **A Sixel or an embedder's image goes with
    /// its last row too**, the `ceil(h / cell_h)` rows it is drawn over
    /// ([`ImageStore::rows_over`]): the Sixel texts describe no history,
    /// but they describe the screen's top -- "The top line of the sixel
    /// image may scroll off the screen" (`[[source: vt3xx-sixel]]`,
    /// "Sixel Scrolling Enabled"), the rest still shown -- and xterm
    /// moves a graphic up with each scroll and drops none (`[[source:
    /// xterm-graphics-c]]` 1787-1808). The history's far end takes the
    /// same rule, as kitty's does; foot drops a Sixel with its start row
    /// (`[[source: foot-1280-sixel-c]]` 417-441), where its ring
    /// overwrites the row, which baud's retains.
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
        if r.ends > lo && r.last < hi && clear {
            return 0;
        }
        let mut n = 0;
        for p in 0..self.live {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen {
                continue;
            }
            if s.line.plus(self.rows_over(s)) <= lo || s.line >= hi || Some(s.line) == blanked {
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
    /// without moving the window. A Sixel's or an embedder's placement
    /// whose anchor leaves the band went with its row -- deleted off one
    /// end, the region's text with it -- and is dropped and counted in
    /// [`ImageStore::placements_expired`]. Outside the band nothing moves.
    /// Returns how many were dropped.
    ///
    /// **A kitty placement takes the text's rule**: "When page margins are
    /// defined and the index commands are used, only images that are
    /// entirely within the page area (between the margins) must be
    /// scrolled. When scrolling them would cause them to extend outside the
    /// page area, they must be clipped" (`[[source: kitty-graphics]]`,
    /// "Interaction with other terminal actions"). One whose shown rows
    /// cross a margin stays where it is; one inside moves, and the rows it
    /// would take past the margin are cut from it ([`KittyPlace::cut_top`],
    /// [`KittyPlace::cut_bottom`]), the drawn image clipped at the margin's
    /// row edge; one moved wholly past it goes. The text names the index
    /// commands; `IL` and `DL` move a band the same way here, baud's
    /// choice. A top-anchored region moves the window instead, and
    /// [`ImageStore::put_back_band`] keeps the rule there.
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

    /// **Put back the placements a top-anchored region's window move took
    /// up with it**: the rows `[lo, hi)` under the region went back down
    /// `n`, and their images with them by their anchors, as text rows are
    /// put back whatever an image's height -- an image under a region was
    /// never in its page area, so the text's margin rule does not touch
    /// it. And a kitty placement across the region's bottom margin, from
    /// above `lo` into it, goes back down too: "only images that are
    /// entirely within the page area ... must be scrolled"
    /// (`[[source: kitty-graphics]]`, "Interaction with other terminal
    /// actions"), so it stays on its screen rows. Returns how many were
    /// dropped off the bottom.
    pub fn put_back_band(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        n: Rows,
    ) -> usize {
        self.shift_pages(screen, lo, hi, None, n, BandMove::Down);
        let mut n_gone = 0;
        for p in 0..self.live {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen || s.line >= hi || self.is_regis(s) {
                continue;
            }
            let kitty = s
                .image
                .index()
                .and_then(|i| self.slots.get(i))
                .is_some_and(|i| matches!(i.owner, ImageOwner::Kitty(..)));
            let across = kitty && s.line < lo && s.line.plus(s.kitty.shown()) > lo;
            if s.line < lo && !across {
                continue;
            }
            let to = s.line.plus(n);
            match to < hi || across {
                true => self.placements[p].line = to,
                false => {
                    self.unplace(PlacementId::from_index(p));
                    self.placements_expired += 1;
                    n_gone += 1;
                }
            }
        }
        self.rescan(screen);
        n_gone
    }

    /// [`ImageStore::scroll_band`] for a band that is a rectangle, the
    /// columns `cols`, inclusive, of `cell_w`-pixel cells: a scroll under
    /// side margins, which moves only the character positions between them
    /// (DECSLRM, `[[source: dec-std-070]]` p. 5-24). **The pictures there
    /// move and the rest stay**, as the one bitmap of the VT330 and VT340
    /// would: the ReGIS page's pixels in the rectangle move
    /// ([`ImageStore::shift_pages`]); a Sixel or an embedder's image wholly
    /// inside moves by its anchor, as in a band; one across a side margin
    /// whose anchor row the band moves is split, the part inside the
    /// margins copied into an image of its own that moves and the rest
    /// staying, its pixels inside cleared (Jev's lean, SPLIT, 0.71 and
    /// 0.71). The part moves its whole height by its anchor, as a whole
    /// image in a band does. Where the arena cannot hold the part, refused
    /// and counted as [`ImageStore::store`] refuses, the image's own pixels
    /// inside the margins move within it instead, and only its rows inside
    /// the band: pixels a move takes past the image's top, moving up, or
    /// past the band's bottom or the image's, whichever comes first,
    /// moving down, are lost. The two paths move different rows. A kitty placement keeps the
    /// text's rule (`ImageStore::scroll_band`).
    #[allow(clippy::too_many_arguments)]
    pub fn scroll_rect(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        cols: (Col, Col),
        cell_w: ImagePx,
        n: Rows,
        way: BandMove,
    ) -> usize {
        self.scroll_band_within(screen, lo, hi, Some((cols, cell_w.index())), n, way)
    }

    fn scroll_band_within(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        rect: Option<((Col, Col), usize)>,
        n: Rows,
        way: BandMove,
    ) -> usize {
        let Some(r) = self.reach[screen.slot()] else {
            return 0;
        };
        let cols = rect.map(|(c, _)| c);
        let cells = rect.map(|((l, r), cw)| ((l.index(), r.index() + 1), cw));
        self.shift_pages(screen, lo, hi, cells, n, way);
        if r.last < lo || r.first >= hi {
            return 0;
        }
        if let Some(((l, r), cw)) = rect {
            self.split_across(screen, lo, hi, (l.index(), r.index() + 1), cw, n, way);
        }
        let mut n_gone = 0;
        for p in 0..self.live {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen || s.line < lo || s.line >= hi {
                continue;
            }
            // A ReGIS page's rows moved above (`ImageStore::shift_pages`);
            // under side margins it stays where it is.
            if self.is_regis(s) {
                continue;
            }
            if cols.is_some_and(|(l, r)| s.col < l || s.col > r) {
                continue;
            }
            // Across a side margin: split above (`ImageStore::split_across`),
            // what stays outside staying.
            if let Some(((l, r), cw)) = rect {
                if self.across(s, (l.index(), r.index() + 1), cw) {
                    continue;
                }
            }
            let kitty = s
                .image
                .index()
                .and_then(|i| self.slots.get(i))
                .is_some_and(|i| matches!(i.owner, ImageOwner::Kitty(..)));
            if kitty {
                // "entirely within the page area": under side margins its
                // columns too, the anchor left of the band skipped above.
                let across = cols.is_some_and(|(_, r)| {
                    s.col.index().saturating_add(s.kitty.cols.index().max(1)) > r.index() + 1
                });
                if across {
                    continue;
                }
                match clipped(s, lo, hi, n, way) {
                    Clip::Stays => {}
                    Clip::Moves(line, k) => {
                        self.placements[p].line = line;
                        self.placements[p].kitty = k;
                    }
                    Clip::Goes => {
                        self.unplace(PlacementId::from_index(p));
                        self.placements_expired += 1;
                        n_gone += 1;
                    }
                }
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
        for p in self.placements[..self.live].iter_mut() {
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
        for p in self.placements[..self.live].iter_mut() {
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
        for p in self.placements[..self.live].iter_mut() {
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
        for p in self.placements[..self.live].iter_mut() {
            p.staged = Staged::Idle;
        }
    }

    /// Write the mapped placements back as lines from `floor` -- the same
    /// oldest retained line the staging counted from, which a reflow keeps as
    /// its origin -- and drop, counted, the ones no line took. Returns how
    /// many were dropped.
    pub fn follow_end(&mut self, floor: LineIdx) -> usize {
        let mut n = 0;
        for p in 0..self.live {
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
        if self.local_placed == 0 || cw == 0 || ch == 0 || col1 <= col0 {
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
        for p in 0..self.live {
            let slot = self.placements[p];
            if slot.is_empty() || slot.screen != screen {
                continue;
            }
            let Some(i) = slot.image.index() else {
                continue;
            };
            let img = self.slots[i];
            // A kitty image is not erased by text: "The other commands to
            // erase text must have no effect on graphics"
            // (`[[source: kitty-graphics]]`), and one under the text, at a
            // negative z, is there to be written over.
            if img.is_empty() || !matches!(img.owner, ImageOwner::Local | ImageOwner::Regis) {
                continue;
            }
            let rows = img.height.index().div_ceil(ch);
            let cols = img.width.index().div_ceil(cw);
            let l0 = usize::try_from(slot.line.value()).unwrap_or(usize::MAX);
            let c0 = slot.col.index();
            // The write's row `wl` lands in the image's rows, and its column
            // span `[col0, col1)` overlaps the image's `[c0, c0 + cols)`.
            if wl >= l0 && wl < l0 + rows && col1 > c0 && col0 < c0 + cols {
                if img.owner == ImageOwner::Regis {
                    let cleared = self.clear_page_cells(
                        slot.image,
                        wl - l0,
                        col0.saturating_sub(c0),
                        col1 - c0,
                        cw,
                        ch,
                    );
                    self.unlit(slot.image, cleared);
                    if self.free_if_unlit(slot.image) {
                        freed += 1;
                    }
                    continue;
                }
                self.unplace(PlacementId::from_index(p));
                freed += 1;
            }
        }
        freed
    }

    /// **Clear the graphic pixels a text erase covers**: an erase, a shift
    /// of characters or a fill at absolute `line`, columns `[col0, col1)`,
    /// over `cell_w` by `cell_h` pixel cells. The VT330 and VT340 keep text
    /// and graphics in one bitmap, so a character position the terminal
    /// erases shows no graphic pixel afterwards; DEC STD 070's B.8.11 leaves
    /// the interaction to the device (Jev, the erase forks of 2026-10-21).
    /// A ReGIS page and a Sixel or an embedder's image are cleared in those
    /// cells, to [`Pixel::CLEAR`]; a Sixel every pixel of which is then clear
    /// is freed, as text written over it frees it, so a previewer that
    /// erases its preview frees the image. A kitty image is not touched:
    /// "The other commands to erase text must have no effect on graphics"
    /// (`[[source: kitty-graphics]]`). Returns how many placements were
    /// freed.
    pub fn erase(
        &mut self,
        screen: ImageScreen,
        line: LineIdx,
        col0: usize,
        col1: usize,
        cell_w: ImagePx,
        cell_h: ImagePx,
    ) -> usize {
        let (cw, ch) = (cell_w.index(), cell_h.index());
        if self.local_placed == 0 || cw == 0 || ch == 0 || col1 <= col0 {
            return 0;
        }
        let Some(r) = self.reach[screen.slot()] else {
            return 0;
        };
        let below = Rows::of_count(self.tallest.index().div_ceil(ch));
        if line < r.first || line >= r.last.plus(below) {
            return 0;
        }
        let wl = usize::try_from(line.value()).unwrap_or(usize::MAX);
        let mut freed = 0;
        for p in 0..self.live {
            let slot = self.placements[p];
            if slot.is_empty() || slot.screen != screen {
                continue;
            }
            let Some(i) = slot.image.index() else {
                continue;
            };
            let img = self.slots[i];
            if img.is_empty() || !matches!(img.owner, ImageOwner::Local | ImageOwner::Regis) {
                continue;
            }
            let rows = img.height.index().div_ceil(ch);
            let cols = img.width.index().div_ceil(cw);
            let l0 = usize::try_from(slot.line.value()).unwrap_or(usize::MAX);
            let c0 = slot.col.index();
            if wl < l0 || wl >= l0 + rows || col1 <= c0 || col0 >= c0 + cols {
                continue;
            }
            let lit = match (img.owner, img.lit) {
                (ImageOwner::Local, None) => {
                    let n = self.pixels[img.start()..img.end()]
                        .iter()
                        .filter(|px| px.alpha().get() != 0)
                        .count();
                    Some(u32::try_from(n).unwrap_or(u32::MAX))
                }
                (_, lit) => lit,
            };
            let cleared = self.clear_page_cells(
                slot.image,
                wl - l0,
                col0.saturating_sub(c0),
                col1 - c0,
                cw,
                ch,
            );
            let left = lit.map(|n| n.saturating_sub(cleared));
            self.slots[i].lit = left;
            if img.owner == ImageOwner::Local && left == Some(0) {
                self.unplace(PlacementId::from_index(p));
                freed += 1;
            }
            if self.free_if_unlit(slot.image) {
                freed += 1;
            }
        }
        freed
    }

    /// **Move a ReGIS page's pixel rows with the lines a band moved**: the
    /// lines `[lo, hi)` of `screen` shifted `n` rows `way` -- IL, DL and a
    /// scroll inside a region -- so the page's rows on those lines go with
    /// them and the rows brought in blank are cleared, as the VT340 moves
    /// its bitmap with the text (vt340test, `[[source:
    /// vt340test-faketextcolor]]`: "Colorized text scrolls down properly
    /// with the screen. Tested with Esc M (reverse index) and with Esc [ M
    /// (delete line)"; Jev, MOVE, 1.00 and 1.00). The page stays anchored
    /// where it is: its rows outside the band do not move.
    fn shift_pages(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        cells: Option<((usize, usize), usize)>,
        n: Rows,
        way: BandMove,
    ) {
        let ch = self.cell_h.index();
        if ch == 0 || hi <= lo {
            return;
        }
        let mut moved = false;
        for p in 0..self.live {
            let slot = self.placements[p];
            if slot.is_empty() || slot.screen != screen {
                continue;
            }
            let Some(img) = slot.image.index().and_then(|i| self.slots.get(i)).copied() else {
                continue;
            };
            if img.is_empty() || img.owner != ImageOwner::Regis {
                continue;
            }
            let (w, h) = (img.width.index(), img.height.index());
            let since = |l: LineIdx| l.since(slot.line).map_or(0, |r| r.index());
            let (y0, y1) = ((since(lo) * ch).min(h), (since(hi) * ch).min(h));
            // The rectangle's columns of `cw`-pixel cells, from the page's
            // anchor column, or all of it.
            let c = slot.col.index();
            let (x0, x1) = match cells {
                None => (0, w),
                Some(((a, b), cw)) => (
                    (a.saturating_sub(c) * cw).min(w),
                    (b.saturating_sub(c) * cw).min(w),
                ),
            };
            if y0 >= y1 || x0 >= x1 {
                continue;
            }
            let k = n.index() * ch;
            let before = self.lit_in(img, (x0, y0, x1, y1));
            self.shift_rows(img.start() + x0, w, x1 - x0, (y0, y1), k, way);
            if let Some((page, plane)) = self.page_plane {
                if page == slot.image {
                    if let Some(pl) = plane.index().and_then(|i| self.slots.get(i)).copied() {
                        let stride = w.div_ceil(4) * 4;
                        let bytes = self.pixels[pl.start()..pl.end()].as_mut_bytes();
                        if let Some(bytes) = bytes.get_mut(x0..) {
                            shift_byte_rows(bytes, stride, x1 - x0, (y0, y1), k, way);
                        }
                    }
                }
            }
            // The rows the band dropped and brought in, lit before and not
            // after: the page's count, over the pixels the move copied.
            let after = self.lit_in(img, (x0, y0, x1, y1));
            self.unlit(slot.image, before.saturating_sub(after));
            self.free_if_unlit(slot.image);
            moved = true;
        }
        if moved {
            self.epoch = self.epoch.bumped();
        }
    }

    /// Rows `[y0, y1)` of the `w`-pixel image at `at` moved `k` rows
    /// `way` within that span, the rows left behind cleared.
    fn shift_rows(
        &mut self,
        at: usize,
        stride: usize,
        w: usize,
        (y0, y1): (usize, usize),
        k: usize,
        way: BandMove,
    ) {
        let row = |y: usize| at + y * stride..at + y * stride + w;
        match way {
            BandMove::Up => {
                for y in y0..y1 {
                    match y + k < y1 {
                        true => self.pixels.copy_within(row(y + k), at + y * stride),
                        false => self.pixels[row(y)].fill(Pixel::CLEAR),
                    }
                }
            }
            BandMove::Down => {
                for y in (y0..y1).rev() {
                    match y >= y0 + k {
                        true => self.pixels.copy_within(row(y - k), at + y * stride),
                        false => self.pixels[row(y)].fill(Pixel::CLEAR),
                    }
                }
            }
        }
    }

    /// Whether placement `s` shows a Sixel or an embedder's image across a
    /// side margin of the columns `[l, r1)` of `cw`-pixel cells: some of its
    /// columns inside and some outside.
    fn across(&self, s: PlacementSlot, (l, r1): (usize, usize), cw: usize) -> bool {
        let Some(img) = s.image.index().and_then(|i| self.slots.get(i)) else {
            return false;
        };
        if img.is_empty() || img.owner != ImageOwner::Local || cw == 0 {
            return false;
        }
        let (c0, c1) = (
            s.col.index(),
            s.col.index() + img.width.index().div_ceil(cw),
        );
        c0 < r1 && c1 > l && (c0 < l || c1 > r1)
    }

    /// **Split each image across a side margin that a rectangle scroll
    /// moves**: one whose anchor row is in the band `[lo, hi)` and whose
    /// columns cross `l` or `r1`. The part inside the margins is copied
    /// into an image of its own, placed where it was drawn, for the band's
    /// move to take; the original keeps the rest, its pixels inside cleared,
    /// and is freed if none is left. A part with nothing lit is not copied.
    /// The part then moves its whole height by its anchor, as a whole
    /// image in a band does. Refused for want of a slot or an arena gap
    /// (counted as [`ImageStore::store`] counts), the original's pixels
    /// inside the margins move `n` rows `way` within it instead, only its
    /// rows inside the band: pixels a move takes past the image's top,
    /// moving up, or past the band's bottom or the image's, whichever
    /// comes first, moving down, are lost.
    #[allow(clippy::too_many_arguments)]
    fn split_across(
        &mut self,
        screen: ImageScreen,
        lo: LineIdx,
        hi: LineIdx,
        (l, r1): (usize, usize),
        cw: usize,
        n: Rows,
        way: BandMove,
    ) {
        let ch = self.cell_h.index();
        if ch == 0 || cw == 0 {
            return;
        }
        for p in 0..self.live {
            let s = self.placements[p];
            if s.is_empty() || s.screen != screen || s.line < lo || s.line >= hi {
                continue;
            }
            if !self.across(s, (l, r1), cw) {
                continue;
            }
            let Some(i) = s.image.index() else {
                continue;
            };
            let img = self.slots[i];
            let (w, h, c0) = (img.width.index(), img.height.index(), s.col.index());
            let (x0, x1) = (((l.max(c0) - c0) * cw).min(w), ((r1 - c0) * cw).min(w));
            if x0 >= x1 {
                continue;
            }
            let moved = self.lit_in(img, (x0, 0, x1, h));
            if moved == 0 {
                continue;
            }
            let pw = x1 - x0;
            let piece = self
                .store(
                    ImagePx::of(u16::try_from(pw).unwrap_or(u16::MAX)),
                    img.height,
                    img.alpha,
                )
                .map(|(id, _)| id);
            let placed = piece.map(|id| {
                (
                    id,
                    self.place(id, screen, s.line, Col::at_index(l.max(c0)), s.z),
                )
            });
            match placed {
                Some((id, pid)) if !pid.is_none() => {
                    let to = id.index().map_or(0, |j| self.slots[j].start());
                    for y in 0..h {
                        let from = img.start() + y * w;
                        self.pixels.copy_within(from + x0..from + x1, to + y * pw);
                        self.pixels[from + x0..from + x1].fill(Pixel::CLEAR);
                    }
                    if let Some(j) = id.index() {
                        self.slots[j].lit = Some(moved);
                    }
                    let left = match img.lit {
                        Some(k) => k.saturating_sub(moved),
                        None => self.lit_in(img, (0, 0, w, h)),
                    };
                    self.slots[i].lit = Some(left);
                    if left == 0 {
                        self.unplace(PlacementId::from_index(p));
                    }
                }
                other => {
                    if let Some((id, _)) = other {
                        self.free(id);
                    }
                    let since = |x: LineIdx| x.since(s.line).map_or(0, |r| r.index());
                    let (y0, y1) = ((since(lo) * ch).min(h), (since(hi) * ch).min(h));
                    self.shift_rows(img.start() + x0, w, pw, (y0, y1), n.index() * ch, way);
                    if img.lit.is_some() {
                        let after = self.lit_in(img, (x0, 0, x1, h));
                        self.unlit(s.image, moved.saturating_sub(after));
                    }
                }
            }
        }
        self.epoch = self.epoch.bumped();
    }

    fn is_regis(&self, s: PlacementSlot) -> bool {
        s.image
            .index()
            .and_then(|i| self.slots.get(i))
            .is_some_and(|i| i.owner == ImageOwner::Regis)
    }

    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_empty())
    }

    /// The lowest free slot: among the slots from
    /// [`ImageStore::free_from`] to [`ImageStore::live`] first, else the
    /// one at `live`, else none -- the table is full.
    fn free_placement(&self) -> Option<usize> {
        let from = self.free_from.min(self.live);
        self.placements[from..self.live]
            .iter()
            .position(|p| p.is_empty())
            .map(|i| from + i)
            .or((self.live < self.place_cap).then_some(self.live))
    }

    /// **Free slot `p`**: emptied, counted out of the live placements, and
    /// the free-slot search moved back to it if it is lower. Its image's
    /// count is the caller's.
    fn vacate(&mut self, p: usize) {
        let local = self.placements[p]
            .image
            .index()
            .and_then(|i| self.slots.get(i))
            .is_some_and(|s| matches!(s.owner, ImageOwner::Local | ImageOwner::Regis));
        if local {
            self.local_placed = self.local_placed.saturating_sub(1);
        }
        self.index_remove(p);
        self.placements[p] = PlacementSlot::EMPTY;
        self.placed = self.placed.saturating_sub(1);
        self.free_from = self.free_from.min(p);
    }

    /// **Lower [`ImageStore::live`] past the free slots below it**, after a
    /// slot is freed: each step is a slot the walks no longer visit, so the
    /// trimming over a table's life costs no more than its placements did.
    fn trim(&mut self) {
        while self.live > 0 && self.placements[self.live - 1].is_empty() {
            self.live -= 1;
        }
    }

    /// How many placements the table can hold: the slots it was lent, or
    /// half the index's usable entries if fewer.
    pub fn placement_slots(&self) -> usize {
        self.place_cap
    }

    /// Whether the placement table has a free slot: what
    /// [`ImageStore::place`] refuses without, for a stream that decides a
    /// placement's outcome before it reserves the image.
    pub(crate) fn placement_free(&self) -> bool {
        self.free_placement().is_some()
    }

    /// The lowest offset at which `area` pixels fit in a gap between live spans,
    /// or `None` if the arena has no such gap: where [`ImageStore::store`]
    /// puts an image, and where a Sixel decoded in its claim moves to. Free space is the gaps between
    /// live slots, so this reclaims every freed span with no free list to keep:
    /// a candidate offset is bumped past any span it overlaps until a full pass
    /// finds none, which converges because the candidate only rises and is
    /// bounded by the arena length.
    pub(crate) fn lowest_gap(&self, area: usize) -> Option<usize> {
        self.lowest_gap_among(area, |s| !s.is_empty())
    }

    /// [`ImageStore::lowest_gap`] with the spans `live` picks taken and the
    /// rest free: what a gap would be were the others deleted.
    fn lowest_gap_among(&self, area: usize, live: impl Fn(&ImageSlot) -> bool) -> Option<usize> {
        let mut cand: usize = 0;
        loop {
            let end = cand.checked_add(area)?;
            if end > self.pixels.len() {
                return None;
            }
            let mut bumped = false;
            for s in self.slots.iter() {
                if !live(s) {
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
