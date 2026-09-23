//! Caller-provided storage: the arena bytes come in through, and the cells
//! they become.
//!
//! **Nothing here allocates.** The core is `no_std` and `alloc`-free: a caller
//! without an allocator hands in `&'static mut` slices from a linker section.
//!
//! **A caller *with* one gets [`Owned`], behind the `alloc` feature, and it is
//! not `Storage::boxed`.** This file used to claim it was, which was a
//! sentence carried over from `baud-core2` and false here: core2's `Storage`
//! *owns* `[PackedCell; GRID]` and can be boxed zeroed, while this one holds
//! **borrows**, so there is nothing for a `Box<Storage>` to own. [`Owned`]
//! holds the nine allocations and lends them, which is the same convenience in
//! the shape this design admits.

use baud_domain::*;

/// The read buffer, sized to what one `read(2)` returns.
///
/// `N_TTY_BUF_SIZE` caps a tty read at 4095 bytes on Linux
/// (`[[source: linux-n-tty]]`) and Darwin hands back 1024, the capacity of the
/// replica's output clist (`TTYCLSIZE`, `[[source: xnu-tty-h]]`) -- so a larger
/// buffer receives nothing extra. Smaller is not safe
/// either: a control sequence that straddles a read has to be re-parsed from
/// its first byte, and the parser carries no unbounded pushback.
pub const READ_CHUNK: usize = 4096;

/// The ring the reads land in.
///
/// 1 MiB, because a sweep peaked there against a 2 MiB L2 and went *negative*
/// by 64 MiB. Bigger is not better here; it is worse.
pub const ARENA: usize = 1 << 20;

/// The widest and tallest grid addressable. A `CSI 8 ; r ; c t` past these is
/// clamped rather than refused.
pub const MAX_COLS: usize = 2048;
pub const MAX_ROWS: usize = 640;

/// How many characters one run may hold.
///
/// **Not the lane count.** A block holds at most eight two-byte characters,
/// and capping the run there caps every vectored write at the same number --
/// the run-length histogram showed 25% of runs sitting exactly at the block's
/// limit, which is the hardware talking rather than the text. Runs extend past
/// the block, and this is where the decode buffer stops instead.
pub const STRIDE_MAX: usize = 32;

/// Cells in the widest, tallest viewport: the ceiling on the *live* grid.
///
/// Not the allocation behind it, which is `rows * stride` -- and the pitch
/// [`grid_stride`] chooses exceeds `MAX_COLS` by about 10 KB of cells a row,
/// so a caller sizing memory asks [`RowBlock::leading`] with the pitch rather
/// than multiplying these two.
pub const GRID: usize = MAX_ROWS * MAX_COLS;

/// The row pitch a terminal's grid is laid out at, for the cache hierarchy
/// `geom` describes and the width a caller provisions: an odd number of
/// lines so it is a multiple of no power-of-two way at any level and no
/// page, at a total a search chose so rows spread over the sets. The width
/// rather than `MAX_COLS`, because with the grid as the scrollback ring every
/// line costs one row of the pitch. See [`store_stride`].
///
/// One function, so every allocator of a grid -- [`Owned`], a bench, an
/// embedder placing the viewport in GPU memory -- lays it out at the same
/// pitch the core will address it by.
pub fn grid_stride(geom: &CacheGeometry, cols: Cols) -> Stride {
    store_stride(geom, cols)
}

/// Everything the core writes into, **placed by the caller**.
///
/// Three borrows rather than three arrays, and that is the contract rather
/// than a convenience: the grid is 24 MB at full size, which is too much to
/// return by value and too much to put on a stack. A caller with an allocator
/// hands in `Vec`s, one without hands in `&'static mut` from a linker section,
/// and the core never learns the difference.
///
/// `Term` takes this by value and holds the borrows, so there is one lifetime
/// on the parser rather than a borrow of a borrow.
pub struct Storage<'b> {
    /// Bytes as they arrived. A read writes here and the parser reads here;
    /// nothing copies between.
    pub(crate) arena: &'b mut [u8],
    /// The primary screen's rows and the lines behind them: `used.len()`
    /// physical rows at the pitch, of which the screen is a window.
    pub(crate) vp: &'b mut [PackedCell],
    /// How many of `vp`'s rows are scrollback: the ring is the screen's
    /// capacity plus this.
    pub(crate) depth: Depth,
    /// The pitch `vp` and `alt` are laid out at: row `p` begins at
    /// `p * stride` cells. **Fixed for the life of the storage** and at least
    /// `MAX_COLS`, so a resize is a change of how many cells in each row are
    /// live and moves none of them. Chosen by [`grid_stride`] from the cache
    /// geometry a host detected -- an odd number of lines, so it aliases no
    /// way, at a step that spreads the rows over every set -- or by
    /// [`Stride::of_cells`] in a bench arm reproducing another layout.
    pub(crate) stride: Stride,
    /// How far each physical row stops being uniform. Blanking stops here, so
    /// a mostly-empty row costs a fraction of its width.
    pub(crate) used: &'b mut [RowExtent],
    /// **The cells a three-byte character becomes, indexed by its bytes.**
    ///
    /// `table[b0][b1][b2]` -- 1024 prefixes x 256 entries -- so the write is a
    /// store of a value already in hand rather than a decode, a property
    /// lookup and a construction. Built once by [`Term::new`], which is why it
    /// is caller-provided memory rather than rodata: 2 MiB is more than a
    /// `const fn` should evaluate, and nothing about it is per-terminal.
    ///
    /// Nothing dedupes here and that is the point: a row carrying the codepoint
    /// is unique by construction, because the codepoint *is* the concatenation
    /// of the index bytes. Storing it is how you stop computing it.
    pub(crate) cells3: &'b mut [PackedCell],
    /// The rendition table. A cell holds a [`StyleId`] *index* into this;
    /// **bounded by construction and evicting on collision**, which is the
    /// contract every content-keyed table in this project carries -- a
    /// growable one is the content-addressing failure mode this repo records.
    ///
    /// `Option` rather than a parallel validity array: absent and occupied are
    /// two states of one thing, and a `bool` beside it is the missing enum the
    /// audit exists to catch.
    pub(crate) tmpl: &'b mut [Option<StyleWord>],
    /// Tab stops, one per column.
    pub(crate) tabs: &'b mut [bool],
    /// **Where the primary screen waits while the alternate screen is up.**
    ///
    /// One buffer and not a second live surface, and the difference is worth
    /// stating because `baud-core2` made the other choice. `[[source:
    /// ctlseqs]]`'s `?1049` *clears* the alternate screen on entry, so its
    /// content never has to survive an exit -- which is what makes "two
    /// surfaces, pick one per operation" and "park the primary, hand the grid
    /// over" render the same screen.
    ///
    /// **`?1049` is the only alternate screen this core implements, and that
    /// is load-bearing rather than incidental.** `?47` is the pre-1049 form
    /// and it *switches buffers without clearing*, so its alternate content
    /// does have to survive an exit -- which parking cannot express, because
    /// the one grid is handed over and the parked copy holds the primary.
    /// `baud-core` routes `PrivateMode::AltScreenLegacy` nowhere, so `?47` is
    /// unimplemented here rather than wrong; `baud-core2` implements it with
    /// two live surfaces, which is the shape it needs. Implementing `?47` here
    /// means giving up parking, and this note is so that trade is made
    /// deliberately.
    ///
    /// Parking is what costs nothing on the path that matters. core2 branches
    /// on `screen` at every site that builds a `View`; here `View` is built
    /// **once per read** and always addresses `vp`, so the per-character loop
    /// never learns the alternate screen exists. What it costs instead is one
    /// copy of the grid per transition -- ~78 KB at the corpus geometry,
    /// microseconds, on a path a session takes a handful of times. The whole
    /// corpus contains **one** alt-screen exit.
    /// Interned grapheme clusters, indexed **from the cell**.
    ///
    /// See [`ClusterStore`]. The index rides in `CellFlags`, so it moves when
    /// the cell moves -- which is what makes it survive `RowRot`, a frame's read
    /// and the alternate screen without a parallel structure to keep in step,
    /// and is exactly what `baud-core2`'s position key does not.
    pub(crate) clusters: ClusterStore<'b>,
    /// Everything a control string writes. See [`StrBuf`].
    pub(crate) strs: StrBuf<'b>,
    pub(crate) alt: AltBuf<'b>,
    /// The primary ring's bookkeeping; see [`Book`].
    pub(crate) book: Book,
    /// Bytes the terminal owes the host: the answer to a `DA`, a `DSR`, a
    /// `DECRQM` or an `XTVERSION`.
    ///
    /// **Caller-provided like everything else.** This core owns nothing, which
    /// is the property `multibench`'s manifest calls "the point of it" -- so
    /// the reply buffer is a slice the embedder hands over, not an array the
    /// core keeps. Without one the queries were recognised, counted and
    /// deliberately unanswered, which `docs/vt-coverage.md` rates as better
    /// than answering in the wrong shape but worse than answering.
    pub(crate) reply: &'b mut [u8],
    /// Stored images and their placements; see [`ImageStore`]. Bounded,
    /// refuse-and-count, caller-owned like everything else.
    ///
    /// **The pixel arena defaults absent**, the `cells3` opt-in shape: a
    /// caller that wants images provisions it with [`Owned::with_images`], and
    /// without it every image is refused and counted -- a terminal with no
    /// image memory draws none and says how many it dropped, which is the
    /// refuse-and-count contract rather than a silent nothing.
    pub(crate) images: ImageStore<'b>,
    /// Where a Sixel body accumulates across reads before it is decoded, the
    /// way an OSC body accumulates in `pending`. It has its own buffer because
    /// a Sixel body is far larger than `OSC_MAX` -- over 5 KB for a 64x48
    /// image -- and it also defaults absent, so a body arriving with no
    /// scratch is counted rather than decoded from nothing.
    pub(crate) sixel_body: &'b mut [u8],
}

/// The parked primary screen: its cells, its extents and its rotation.
///
/// **One struct rather than three fields, so they cannot be parked
/// separately.** `RowExtent` is indexed by *physical* row and `RowRot` is what
/// makes a physical row out of a screen row, so the three are one value; a
/// restore missing either gives a grid whose rows are in the wrong order or
/// whose blanking stops at the other surface's marks. `View::save_to` and
/// `View::restore_from` take all three in one call for the same reason.
///
/// It is also why `Paint` carries **one** word for this rather than two
/// slices: this file records a thin reference recovering most of the cost of
/// carrying a slice through the inlined loop, and the grouping was chosen for
/// correctness first and gets that for free.
pub struct AltBuf<'b> {
    pub(crate) cells: &'b mut [PackedCell],
    pub(crate) used: &'b mut [RowExtent],
    /// The alternate screen's rotation, into a ring of `used.len()` rows.
    /// `RowRot::NONE` when it is not up: it is cleared on entry, so a
    /// rotation left over from last time describes nothing.
    pub(crate) rot: RowRot,
}

/// What the scroll paths keep about the primary ring, a word `Paint`
/// carries: how many rows behind the window are lines that scrolled off,
/// and how many after it were cut by a shorter window and can come back.
/// Grid bookkeeping, so it lives here rather than in `Cold`, whose size is
/// pinned.
#[derive(Clone, Copy)]
pub struct Book {
    pub(crate) hist: Rows,
    pub(crate) hidden: Rows,
    /// **The absolute line at the top of the primary window.** It moves with
    /// the rotation both ways -- up one for every row the window retires,
    /// down one for every row it takes back -- and a reflow renumbers it with
    /// the rows. So screen row `y` is line `top + y`, a line `n` rows behind
    /// the window is `top - 1 - n`, and the oldest retained is `top - hist`.
    /// An image placement anchors to `top + cursor row`, so it follows its row
    /// through every move the window makes. Unbounded, where `hist` is capped
    /// at the depth, so two images past a full ring get distinct lines.
    pub(crate) top: LineIdx,
    /// The same for the alternate screen, which keeps no history: its line at
    /// the top of the window. Kept only while an image is placed -- nothing
    /// else reads it -- and restarted each time the screen is entered.
    pub(crate) alt_top: LineIdx,
}

/// Everything a control string writes, in one struct.
///
/// **Six slices rather than six arguments, for the reason `AltBuf` is one
/// argument.** `Storage::new` already takes ten `&mut` slices in a fixed order,
/// which is ten chances to swap two of the same type; six more would be
/// sixteen, and four of these six are `&mut [u8]`. Grouping them means a caller
/// names each one and the compiler checks the name.
///
/// They belong together for a second reason: they are the *result* of a control
/// string, and `pending` is the string itself. A title that outlived its
/// terminator, or a link table lent from one place while the payload it was
/// parsed from was lent from another, is the borrow this shape makes
/// unspellable.
pub struct StrBuf<'b> {
    /// The body being accumulated, reset at the **introducer**.
    ///
    /// Not at the terminator, which is the difference that matters: a string
    /// spans reads, so this outlives a `feed`, and a cancelled string never
    /// reaches a terminator at all. Resetting where the string *starts* is what
    /// makes both cases right, and it is the same failure this repo records for
    /// `pre: Option<Classified>` -- a value carried between two functions,
    /// consumed by a later and different one because the path has more exits
    /// than it looks like it has.
    pub(crate) pending: &'b mut [u8],
    /// `OSC 2`, and `OSC 0` sets this and `icon` together.
    pub(crate) title: &'b mut [u8],
    /// `OSC 1`.
    pub(crate) icon: &'b mut [u8],
    /// Palette overrides from `OSC 4`. `ColorWord::DEFAULT` means "not
    /// overridden", so a zeroed buffer starts on the renderer's own palette
    /// with no separate validity array to keep in step.
    pub(crate) palette: &'b mut [ColorWord],
    /// Hyperlink URIs, `NLINK` of them flat at a stride of `LINK_MAX`.
    ///
    /// Flat rather than `[[u8; LINK_MAX]; NLINK]` because the storage is
    /// caller-provided and a slice cannot carry the inner dimension. The stride
    /// is asserted in `Storage::new` rather than assumed -- a two-dimensional
    /// array flattened into a slice is exactly where a silent stride bug lives.
    pub(crate) links: &'b mut [u8],
    pub(crate) link_len: &'b mut [ByteLen],
}

impl<'b> StrBuf<'b> {
    /// Lend the six, in the one order that is correct.
    ///
    /// A constructor rather than public fields, because `Storage::new` is
    /// public and takes a `StrBuf`: without this an embedder outside the crate
    /// could name the type and not build one, which is an entry point whose
    /// argument is unreachable -- the same shape as a mode recorded and never
    /// read.
    pub fn new(
        pending: &'b mut [u8],
        title: &'b mut [u8],
        icon: &'b mut [u8],
        palette: &'b mut [ColorWord],
        links: &'b mut [u8],
        link_len: &'b mut [ByteLen],
    ) -> StrBuf<'b> {
        StrBuf {
            pending,
            title,
            icon,
            palette,
            links,
            link_len,
        }
    }
}

/// The sixteen allocations a `Storage` borrows, owned in one place.
///
/// **A convenience for tests and embedders with an allocator, and nothing the
/// core itself uses.** Every test file that drives this crate otherwise
/// repeats the same twenty-five lines of `vec!` and hands ten `&mut`s to
/// `Storage::new` in the right order -- which is ten chances to swap two
/// arguments of the same type, and the reason the property suite could not be
/// written cheaply before this existed.
///
/// Behind `alloc`, so the default build is still allocation-free and the
/// `no_std` target still compiles with nothing but `core`.
#[cfg(feature = "alloc")]
pub struct Owned {
    stride: Stride,
    depth: Depth,
    arena: alloc::vec::Vec<u8>,
    vp: alloc::vec::Vec<PackedCell>,
    used: alloc::vec::Vec<RowExtent>,
    cells3: alloc::vec::Vec<PackedCell>,
    tmpl: alloc::vec::Vec<Option<StyleWord>>,
    tabs: alloc::vec::Vec<bool>,
    reply: alloc::vec::Vec<u8>,
    alt: alloc::vec::Vec<PackedCell>,
    alt_used: alloc::vec::Vec<RowExtent>,
    clusters: alloc::vec::Vec<ClusterSlot>,
    pending: alloc::vec::Vec<u8>,
    title: alloc::vec::Vec<u8>,
    icon: alloc::vec::Vec<u8>,
    palette: alloc::vec::Vec<ColorWord>,
    links: alloc::vec::Vec<u8>,
    link_len: alloc::vec::Vec<ByteLen>,
    img_pixels: alloc::vec::Vec<ColorWord>,
    img_slots: alloc::vec::Vec<ImageSlot>,
    img_places: alloc::vec::Vec<PlacementSlot>,
    sixel_body: alloc::vec::Vec<u8>,
}

#[cfg(feature = "alloc")]
impl Owned {
    /// Sized to `ws`, with the three-byte cell table **absent**.
    ///
    /// An empty `cells3` is how a caller declines it, and declining is the
    /// default: its only reader is behind `cell-table`, and passing the full
    /// 2 MiB otherwise costs 262,144 property-table lookups and 2 MiB of
    /// stores at construction for a table no read will consult.
    pub fn new(ws: &WinSize) -> Owned {
        Owned::with_geometry(ws, &CacheGeometry::UNKNOWN)
    }

    /// Sized to `ws`, at the pitch [`grid_stride`] derives from `geom`.
    ///
    /// [`Owned::new`] passes [`CacheGeometry::UNKNOWN`], which is safe: an
    /// odd number of the smallest line any cache uses, so it aliases no way
    /// on any machine, at the step the search chose for that line. An app
    /// hands in what its host detected -- `baud-isa`'s per-arch crates -- so
    /// the pitch is the one searched for *this* machine's line.
    pub fn with_geometry(ws: &WinSize, geom: &CacheGeometry) -> Owned {
        Owned::with_history(ws, geom, Depth::DEFAULT)
    }

    /// Sized to `ws` at the pitch `geom` and `ws`'s width choose, with
    /// `depth` lines of scrollback behind the screen: [`Depth::NONE`] for a
    /// terminal that keeps none. The width is provisioned as well as the
    /// height -- a resize past it answers `Truncated` -- because every line
    /// of scrollback costs a row of the pitch.
    pub fn with_history(ws: &WinSize, geom: &CacheGeometry, depth: Depth) -> Owned {
        use alloc::vec;
        let (cols, rows) = ws.geometry();
        let stride = grid_stride(geom, cols);
        let span = RowBlock::leading(rows, stride);
        let ring = RowBlock::leading(Rows::of_index(rows.index() + depth.lines()), stride);
        Owned {
            stride,
            depth,
            arena: vec![0u8; ARENA],
            vp: vec![PackedCell::EMPTY; ring.len()],
            used: vec![RowExtent::ZERO; rows.index() + depth.lines()],
            cells3: vec::Vec::new(),
            // Sized to the whole primary screen (`span` is `rows * stride`), so
            // a text-drawn image preview -- one distinct rendition per cell --
            // never evicts a rendition a visible cell still references. A resize
            // clamps `cols` to `stride` and `rows` to the provisioned capacity,
            // so the largest screen this storage can display is `span.len()`
            // cells, and the table holds it. The next power of two at or above
            // it (so open addressing's `& (len - 1)` wraps), between the NSTYLE
            // floor and the NSTYLE_MAX ceiling; the ceiling clamp precedes
            // `next_power_of_two()`, which panics past 2^63.
            tmpl: vec![None; span.len().min(NSTYLE_MAX).next_power_of_two().max(NSTYLE)],
            tabs: vec![false; MAX_COLS],
            reply: vec![0u8; REPLY],
            alt: vec![PackedCell::EMPTY; span.len()],
            alt_used: vec![RowExtent::ZERO; rows.index()],
            clusters: vec![ClusterSlot::EMPTY; NCLUSTER],
            pending: vec![0u8; OSC_MAX],
            title: vec![0u8; OSC_MAX],
            icon: vec![0u8; OSC_MAX],
            palette: vec![ColorWord::DEFAULT; 256],
            links: vec![0u8; NLINK * LINK_MAX],
            link_len: vec![ByteLen::ZERO; NLINK],
            // The slot and placement tables are tiny and always allocated; the
            // pixel arena and the Sixel-body scratch are the large buffers and
            // default absent, provisioned by [`Owned::with_images`].
            img_pixels: vec::Vec::new(),
            img_slots: vec![ImageSlot::EMPTY; NIMAGE],
            img_places: vec![PlacementSlot::EMPTY; NPLACEMENT],
            sixel_body: vec::Vec::new(),
        }
    }

    /// Provision image storage: `pixels` `ColorWord`s of ARGB arena and `body`
    /// bytes of Sixel-body scratch. [`Owned::new`] provisions neither -- the
    /// `with_cell_table` opt-in shape -- so a terminal draws no images until a
    /// caller that wants them sizes these.
    ///
    /// **The size is the owner's call, between one image and a screenful.** A
    /// 64x48 image is 3072 `ColorWord`s (12 KiB); a full 200x50 screen at an
    /// 8x18 cell is ~1.4M (5.6 MiB). The body scratch holds one raw Sixel
    /// before it decodes, a few KB per small image and proportionally more.
    ///
    /// **A zero-length arena is valid and refuses every image**, which the
    /// gate depends on -- do not add a non-empty assert here. Refuse-and-count
    /// is the contract: `store` returns `None` and increments `images_refused`
    /// when no gap holds the pixels, and an empty arena is that with no gap at
    /// all, which is the correct nothing for a caller that wants images
    /// recognised and dropped rather than drawn.
    pub fn with_images(mut self, budget: ImageBudget) -> Owned {
        self.img_pixels = alloc::vec![ColorWord::DEFAULT; budget.pixels];
        self.sixel_body = alloc::vec![0u8; budget.body];
        self
    }

    /// Sized to `ws` with **neither surface of its own**, for an embedder
    /// that places both where a GPU can read them in place and lends them
    /// through [`Owned::storage_over`]: the primary ring at [`Owned::ring`]
    /// rows and the alternate screen at [`Owned::rows`], both at
    /// [`Owned::stride`]. Everything else is allocated here.
    pub fn without_surfaces(ws: &WinSize, geom: &CacheGeometry, depth: Depth) -> Owned {
        let mut owned = Owned::with_history(ws, geom, depth);
        owned.vp = alloc::vec::Vec::new();
        owned.alt = alloc::vec::Vec::new();
        owned
    }

    /// The pitch this storage lays its grids out at.
    pub fn stride(&self) -> Stride {
        self.stride
    }

    /// Physical rows in the primary ring: the screen's capacity plus the
    /// scrollback.
    pub fn ring(&self) -> Rows {
        Rows::of_index(self.used.len())
    }

    /// The screen's capacity in rows, which is the alternate screen's ring.
    pub fn rows(&self) -> Rows {
        Rows::of_index(self.used.len() - self.depth.lines())
    }

    /// Lend them all to a `Storage` with `vp` and `alt` standing in for the
    /// two surfaces, in memory the caller chose -- shared GPU buffers, so the
    /// grid the core writes is the grid the GPU reads.
    pub fn storage_over<'v>(
        &'v mut self,
        vp: &'v mut [PackedCell],
        alt: &'v mut [PackedCell],
    ) -> Storage<'v> {
        Storage::new(
            &mut self.arena,
            self.stride,
            self.depth,
            vp,
            &mut self.used,
            &mut self.cells3,
            &mut self.tmpl,
            &mut self.tabs,
            &mut self.reply,
            alt,
            &mut self.alt_used,
            &mut self.clusters,
            StrBuf {
                pending: &mut self.pending,
                title: &mut self.title,
                icon: &mut self.icon,
                palette: &mut self.palette,
                links: &mut self.links,
                link_len: &mut self.link_len,
            },
            ImageStore::new(
                &mut self.img_pixels,
                &mut self.img_slots,
                &mut self.img_places,
            ),
            &mut self.sixel_body,
        )
    }

    /// Lend them all to a `Storage`.
    pub fn storage(&mut self) -> Storage<'_> {
        Storage::new(
            &mut self.arena,
            self.stride,
            self.depth,
            &mut self.vp,
            &mut self.used,
            &mut self.cells3,
            &mut self.tmpl,
            &mut self.tabs,
            &mut self.reply,
            &mut self.alt,
            &mut self.alt_used,
            &mut self.clusters,
            StrBuf {
                pending: &mut self.pending,
                title: &mut self.title,
                icon: &mut self.icon,
                palette: &mut self.palette,
                links: &mut self.links,
                link_len: &mut self.link_len,
            },
            ImageStore::new(
                &mut self.img_pixels,
                &mut self.img_slots,
                &mut self.img_places,
            ),
            &mut self.sixel_body,
        )
    }
}

#[cfg(feature = "alloc")]
impl Owned {
    /// Sized to `ws`, with the three-byte cell table **present**.
    ///
    /// The one caller that wants it is the table's own equivalence test, which
    /// builds it *without* `cell-table` and reads the slice directly -- so the
    /// write path is the computing one and the table is the thing under test.
    /// `Owned::new` declines it because nothing else reads it: the only read is
    /// in `put_run` behind the feature, and passing 2 MiB otherwise buys
    /// 262,144 property-table lookups and 2 MiB of stores at construction for
    /// a table no read will consult.
    pub fn with_cell_table(ws: &WinSize) -> Owned {
        let mut owned = Owned::new(ws);
        owned.cells3 = alloc::vec![PackedCell::EMPTY; CELLS3];
        owned
    }

    /// The table as `Term::new` built it.
    ///
    /// Borrowed after the `Term` is dropped, which is what lets a test compare
    /// the table against the cells the write path produced from the same bytes.
    pub fn cell_table(&self) -> &[PackedCell] {
        &self.cells3
    }
}

/// How many distinct grapheme clusters can be live at once.
///
/// **63, and the bound comes from the cell rather than from a sweep.** The
/// index rides in `CellFlags`, `PackedCell` is eight bytes because a ninth
/// rounds it to twelve, three bits are flags and `NLINK = 128` takes seven --
/// which leaves six. `ClusterStore` refuses beyond that and counts, so the
/// answer is never wrong and how often it is missing is a number.
pub const NCLUSTER: usize = 64;

/// The **floor** on the rendition table: the smallest it is ever sized to, for
/// a screen too small to need more. The table is sized to the screen by
/// [`style_slots`] -- a text-drawn image preview mints one distinct truecolour
/// rendition per cell, so a table smaller than the screen would evict a
/// rendition an on-screen cell still points at, and that cell would resolve to
/// another's colour. Below capacity the mapping is exact, so a table that holds
/// one full screen never scrambles a preview; the floor keeps a small screen
/// from paying for slots it cannot fill.
pub const NSTYLE: usize = 1024;

/// The **ceiling**, from the cell rather than from a sweep: a [`StyleId`] is a
/// `u16`, so a slot index must fit sixteen bits and the table cannot exceed
/// `2^16` slots however large the screen. A screen with more than this many
/// distinct renditions -- 65,536 cells, a window no real terminal reaches --
/// can still evict, which is recorded rather than paid for. At 20 bytes a
/// `StyleWord` the ceiling is a 1.5 MB table, so only a window that large pays
/// it; a 200x50 screen sizes to 16,384 slots, ~384 KB.
pub const NSTYLE_MAX: usize = 1 << 16;

/// How many `(lead, continuation)` prefixes a three-byte character has.
pub const CELLS3_PREFIXES: usize = 16 * 64;
/// Entries in the three-byte cell table.
pub const CELLS3: usize = CELLS3_PREFIXES * 256;

/// The longest control-string body kept, and the longest title.
///
/// **A truncation is counted, not silent.** A title past this is clipped and
/// `Term::string_truncations` says how often -- the same contract every
/// bounded table here carries. `baud-core2` uses the same 512.
pub const OSC_MAX: usize = 512;

/// Distinct `OSC 8` hyperlinks live at once.
///
/// **128 because the cell says so.** The link index rides in `CellFlags`,
/// which has seven bits for it, and seven bits is 128 -- 127 usable, since
/// zero is "no link". Bounded because a URI is attacker-supplied content used
/// as a key, and a growable table keyed on that is the content-addressing
/// failure mode this repo already records. **It refuses on collision rather
/// than evicting**, unlike the style table: an evicted style is a wrong colour
/// on the next frame, an evicted link is a wrong *destination* under a cell
/// whose text did not change -- and a later `OSC 8` run that writes no cell
/// at all could have retargeted it. A refused link is counted
/// (`Term::links_refused`) and its cells are plain text.
pub const NLINK: usize = 128;

/// The longest URI kept, and the stride of the flat `links` slice.
pub const LINK_MAX: usize = 256;

/// How many bytes of reply the core will hold before it starts counting
/// losses. `baud-core2` uses the same number and for the same reason: the
/// longest thing this can queue is a `DECRQSS` rendition string, and 256
/// bounds it with room while staying far below anything that matters.
pub const REPLY: usize = 256;

/// How many images can be live at once. Small: a screen shows a handful, the
/// table is scanned linearly, and it is tiny (a slot is a span and a refcount),
/// so it is always allocated -- unlike the pixel arena, which is the large
/// buffer and defaults absent. The store works with any slot count, so this is
/// [`Owned`]'s provisioning size rather than an invariant, hence `alloc`-gated.
#[cfg(feature = "alloc")]
pub const NIMAGE: usize = 32;

/// How many placements can be live at once. An image can be placed more than
/// once, so this is larger than [`NIMAGE`], and also small and always
/// allocated.
#[cfg(feature = "alloc")]
pub const NPLACEMENT: usize = 64;

impl<'b> Storage<'b> {
    /// How many rows the caller provisioned, which is not the live row count.
    ///
    /// **The number is already in the struct**, so this is a read rather than
    /// a field: `used` has one entry per physical row, and `Storage::new`
    /// asserts `vp` holds that many rows' worth of cells. It matters because
    /// the storage is caller-provided: a `Term` can be resized *down* freely
    /// and can only grow into rows somebody allocated. `Owned::new` sizes to
    /// the geometry it is given, so growing past it is `Resized::Truncated`
    /// rather than a silent clamp -- an embedder that wants headroom
    /// provisions for it, and this is the number it provisions.
    pub fn row_capacity(&self) -> Rows {
        Rows::of_index(self.used.len() - self.depth.lines())
    }

    /// Physical rows in the primary ring: the screen's capacity plus the
    /// scrollback.
    pub fn ring_rows(&self) -> Rows {
        Rows::of_index(self.used.len())
    }

    /// # Panics
    /// If any slice is shorter than the geometry needs. Checked once, here,
    /// rather than on every access -- the parser addresses this memory
    /// millions of times a second and cannot re-establish the invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        arena: &'b mut [u8],
        stride: Stride,
        depth: Depth,
        vp: &'b mut [PackedCell],
        used: &'b mut [RowExtent],
        cells3: &'b mut [PackedCell],
        tmpl: &'b mut [Option<StyleWord>],
        tabs: &'b mut [bool],
        reply: &'b mut [u8],
        alt: &'b mut [PackedCell],
        alt_used: &'b mut [RowExtent],
        clusters: &'b mut [ClusterSlot],
        strs: StrBuf<'b>,
        images: ImageStore<'b>,
        sixel_body: &'b mut [u8],
    ) -> Storage<'b> {
        assert!(arena.len() >= READ_CHUNK, "arena is smaller than one read");
        // **The stride is checked, not assumed.** `links` is `NLINK` URIs flat
        // at `LINK_MAX` each, because caller-provided storage cannot carry the
        // inner dimension of a two-dimensional array. A slice one stride short
        // does not fail: it returns a plausible URI belonging to the next slot.
        assert!(
            strs.links.len() >= NLINK * LINK_MAX,
            "links is {} bytes, needs {} -- {NLINK} URIs at a stride of {LINK_MAX}",
            strs.links.len(),
            NLINK * LINK_MAX
        );
        assert!(
            strs.link_len.len() >= NLINK,
            "link_len is shorter than NLINK"
        );
        assert!(
            strs.pending.len() >= OSC_MAX,
            "pending is shorter than OSC_MAX"
        );
        assert!(strs.title.len() >= OSC_MAX, "title is shorter than OSC_MAX");
        assert!(strs.icon.len() >= OSC_MAX, "icon is shorter than OSC_MAX");
        assert!(strs.palette.len() >= 256, "palette is not 256 entries");
        // `RowBlock` computes the span so the two operands cannot be the
        // wrong way round -- a transposition here is a plausible number and a
        // silently wrong slice.
        assert!(
            used.len() > depth.lines(),
            "the ring holds no screen: {} rows against {} of scrollback",
            used.len(),
            depth.lines()
        );
        let ring = RowBlock::leading(Rows::of_index(used.len()), stride);
        assert!(ring.fits(vp), "the ring is short of its rows");
        let screen = RowBlock::leading(Rows::of_index(used.len() - depth.lines()), stride);
        // **Required, not optional.** An absent buffer would mean the
        // alternate screen is recognised, entered, and then quietly destroys
        // the primary on the way out -- which `docs/vt-coverage.md` rates as
        // worse than not implementing it, because the application is given no
        // way to find out.
        assert!(
            screen.fits(alt),
            "the alternate screen is short of its rows"
        );
        assert!(
            alt_used.len() >= used.len() - depth.lines(),
            "the alternate screen's extents are one per row"
        );
        assert!(
            cells3.len() == CELLS3 || cells3.is_empty(),
            "the three-byte cell table is 1024 x 256 entries, or absent"
        );
        assert!(
            tmpl.len().is_power_of_two() && tmpl.len() >= NSTYLE && tmpl.len() <= NSTYLE_MAX,
            "the rendition table is a power of two between NSTYLE and NSTYLE_MAX slots"
        );
        assert!(tabs.len() >= MAX_COLS, "tab stops are one per column");
        assert!(reply.len() >= REPLY, "the reply buffer is REPLY bytes");
        assert!(
            clusters.len() == NCLUSTER,
            "the cluster store is NCLUSTER slots"
        );
        Storage {
            arena,
            stride,
            depth,
            vp,
            used,
            cells3,
            tmpl,
            tabs,
            reply,
            clusters: ClusterStore::new(clusters),
            alt: AltBuf {
                cells: alt,
                used: alt_used,
                rot: RowRot::NONE,
            },
            book: Book {
                hist: Rows::ZERO,
                hidden: Rows::ZERO,
                top: LineIdx::FIRST,
                alt_top: LineIdx::FIRST,
            },
            strs,
            images,
            sixel_body,
        }
    }
}
