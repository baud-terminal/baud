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
}

impl<'b> Storage<'b> {
    /// # Panics
    /// If any slice is shorter than the geometry needs. Checked once, here,
    /// rather than on every access -- the parser addresses this memory
    /// millions of times a second and cannot re-establish the invariant.
    pub fn new(
        arena: &'b mut [u8],
        vp: &'b mut [PackedCell],
        used: &'b mut [RowExtent],
    ) -> Storage<'b> {
        assert!(arena.len() >= READ_CHUNK, "arena is smaller than one read");
        // `RowBlock` computes the span so the two operands cannot be the
        // wrong way round -- a transposition here is a plausible number and a
        // silently wrong slice.
        let span = RowBlock::leading(Rows::of_index(used.len()), Cols::from_stride(MAX_COLS));
        assert!(span.fits(vp), "viewport is short of its rows");
        Storage { arena, vp, used }
    }
}
