//! Caller-provided storage: the arena bytes come in through, and the cells
//! they become.
//!
//! **Nothing here allocates.** The core is `no_std` and `alloc`-free; a caller
//! with an allocator gets `Storage::boxed` behind the `alloc` feature, and one
//! without hands in a `&'static mut`.

use baud_domain::*;

/// The read buffer, sized to what one `read(2)` returns.
///
/// `N_TTY_BUF_SIZE` caps a tty read at 4095 bytes on Linux and Darwin hands
/// back 1024, so a larger buffer receives nothing extra. Smaller is not safe
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

/// Cells in the materialised viewport.
pub const GRID: usize = MAX_ROWS * MAX_COLS;

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
    /// ctlseqs]]`'s `?1049` *clears* the alternate screen on entry, and so
    /// does `?47` as this core implements it -- so the alternate's content
    /// never has to survive an exit, and "two surfaces, pick one per
    /// operation" and "park the primary, hand the grid over" render the same
    /// screen.
    ///
    /// Parking is what costs nothing on the path that matters. core2 branches
    /// on `screen` at every site that builds a `View`; here `View` is built
    /// **once per read** and always addresses `vp`, so the per-character loop
    /// never learns the alternate screen exists. What it costs instead is one
    /// copy of the grid per transition -- ~78 KB at the corpus geometry,
    /// microseconds, on a path a session takes a handful of times. The whole
    /// corpus contains **one** alt-screen exit.
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

/// How many renditions the template table holds. Bounded so it cannot walk out
/// of cache under truecolour key pressure.
pub const NSTYLE: usize = 1024;

/// How many `(lead, continuation)` prefixes a three-byte character has.
pub const CELLS3_PREFIXES: usize = 16 * 64;
/// Entries in the three-byte cell table.
pub const CELLS3: usize = CELLS3_PREFIXES * 256;

/// How many bytes of reply the core will hold before it starts counting
/// losses. `baud-core2` uses the same number and for the same reason: the
/// longest thing this can queue is a `DECRQSS` rendition string, and 256
/// bounds it with room while staying far below anything that matters.
pub const REPLY: usize = 256;

impl<'b> Storage<'b> {
    /// # Panics
    /// If any slice is shorter than the geometry needs. Checked once, here,
    /// rather than on every access -- the parser addresses this memory
    /// millions of times a second and cannot re-establish the invariant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        arena: &'b mut [u8],
        vp: &'b mut [PackedCell],
        used: &'b mut [RowExtent],
        cells3: &'b mut [PackedCell],
        tmpl: &'b mut [Option<StyleWord>],
        tabs: &'b mut [bool],
        reply: &'b mut [u8],
        alt: &'b mut [PackedCell],
        alt_used: &'b mut [RowExtent],
    ) -> Storage<'b> {
        assert!(arena.len() >= READ_CHUNK, "arena is smaller than one read");
        // `RowBlock` computes the span so the two operands cannot be the
        // wrong way round -- a transposition here is a plausible number and a
        // silently wrong slice.
        let span = RowBlock::leading(Rows::of_index(used.len()), Cols::from_stride(MAX_COLS));
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
        Storage {
            arena,
            vp,
            used,
            cells3,
            tmpl,
            tabs,
            reply,
            alt: AltBuf {
                cells: alt,
                used: alt_used,
                rot: RowRot::NONE,
            },
        }
    }
}
