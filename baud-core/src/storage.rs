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
/// `geom` describes: at least `MAX_COLS` cells, an odd number of lines so it
/// is a multiple of no power-of-two way at any level and no page, padded by
/// the step a search chose so rows spread over the sets. See
/// [`store_stride`].
///
/// One function, so every allocator of a grid -- [`Owned`], a bench, an
/// embedder placing the viewport in GPU memory -- lays it out at the same
/// pitch the core will address it by.
pub fn grid_stride(geom: &CacheGeometry) -> Stride {
    store_stride(geom, Cols::of_index(MAX_COLS))
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
    /// The materialised viewport.
    pub(crate) vp: &'b mut [PackedCell],
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
    /// the cell moves -- which is what makes it survive `RowRot`, `viewport`
    /// and the alternate screen without a parallel structure to keep in step,
    /// and is exactly what `baud-core2`'s position key does not.
    pub(crate) clusters: ClusterStore<'b>,
    /// Everything a control string writes. See [`StrBuf`].
    pub(crate) strs: StrBuf<'b>,
    pub(crate) alt: AltBuf<'b>,
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
    /// The rotation the parked cells are in. `RowRot::NONE` when nothing is
    /// parked, which is also what the alternate screen starts at -- it is
    /// cleared on entry, so a rotation left over from last time describes
    /// nothing.
    pub(crate) rot: RowRot,
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
        use alloc::vec;
        let (_, rows) = ws.geometry();
        let stride = grid_stride(geom);
        let span = RowBlock::leading(rows, stride);
        Owned {
            stride,
            arena: vec![0u8; ARENA],
            vp: vec![PackedCell::EMPTY; span.len()],
            used: vec![RowExtent::ZERO; rows.index()],
            cells3: vec::Vec::new(),
            tmpl: vec![None; NSTYLE],
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
        }
    }

    /// Sized to `ws` with **no viewport of its own**, for an embedder that
    /// places the grid where a GPU can read it in place and lends it
    /// through [`Owned::storage_over`]. Everything else is allocated here,
    /// and the pitch the viewport must be laid out at is [`Owned::stride`].
    pub fn without_viewport(ws: &WinSize, geom: &CacheGeometry) -> Owned {
        let mut owned = Owned::with_geometry(ws, geom);
        owned.vp = alloc::vec::Vec::new();
        owned
    }

    /// The pitch this storage lays its grid out at, for a caller placing the
    /// viewport somewhere else -- a GPU buffer of `rows * stride` cells --
    /// before lending it through [`Owned::storage_over`].
    pub fn stride(&self) -> Stride {
        self.stride
    }

    /// Lend them all to a `Storage` with `vp` standing in for the viewport:
    /// `rows` rows at [`Owned::stride`], as [`Owned::with_geometry`] would
    /// have allocated, in memory the caller chose -- a shared GPU buffer, so
    /// the grid the core writes is the grid the GPU reads.
    pub fn storage_over<'v>(&'v mut self, vp: &'v mut [PackedCell]) -> Storage<'v> {
        Storage::new(
            &mut self.arena,
            self.stride,
            vp,
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
        )
    }

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

    /// Lend them all to a `Storage`, in the one order that is correct.
    pub fn storage(&mut self) -> Storage<'_> {
        Storage::new(
            &mut self.arena,
            self.stride,
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
        )
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

/// How many renditions the template table holds. Bounded so it cannot walk out
/// of cache under truecolour key pressure.
pub const NSTYLE: usize = 1024;

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
/// which has seven bits for it, and seven bits is 128. The table evicts on
/// collision like the style table: a URI is attacker-supplied content used as
/// a key, and a growable table keyed on that is the content-addressing failure
/// mode this repo already records. An evicted link renders as an ordinary
/// cell.
pub const NLINK: usize = 128;

/// The longest URI kept, and the stride of the flat `links` slice.
pub const LINK_MAX: usize = 256;

/// How many bytes of reply the core will hold before it starts counting
/// losses. `baud-core2` uses the same number and for the same reason: the
/// longest thing this can queue is a `DECRQSS` rendition string, and 256
/// bounds it with room while staying far below anything that matters.
pub const REPLY: usize = 256;

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
        let span = RowBlock::leading(Rows::of_index(used.len()), stride);
        assert!(span.fits(vp), "viewport is short of its rows");
        // **Required, not optional.** An absent buffer would mean the
        // alternate screen is recognised, entered, and then quietly destroys
        // the primary on the way out -- which `docs/vt-coverage.md` rates as
        // worse than not implementing it, because the application is given no
        // way to find out.
        assert!(span.fits(alt), "the parked primary is short of its rows");
        assert!(
            alt_used.len() >= used.len(),
            "the parked extents are one per row"
        );
        assert!(
            cells3.len() == CELLS3 || cells3.is_empty(),
            "the three-byte cell table is 1024 x 256 entries, or absent"
        );
        assert!(tmpl.len() == NSTYLE, "the rendition table is NSTYLE slots");
        assert!(tabs.len() >= MAX_COLS, "tab stops are one per column");
        assert!(reply.len() >= REPLY, "the reply buffer is REPLY bytes");
        assert!(
            clusters.len() == NCLUSTER,
            "the cluster store is NCLUSTER slots"
        );
        Storage {
            arena,
            stride,
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
            strs,
        }
    }
}
