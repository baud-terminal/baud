use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout};

/// Geometry as `TIOCGWINSZ` reports it. Destructured once, here, so `ws_col`
/// and `ws_row` cannot be transposed at any other call site.
pub struct WinSize {
    pub ws_row: u16,
    pub ws_col: u16,
}

impl WinSize {
    /// The single public source of grid geometry. `ws_col` and `ws_row` are
    /// destructured here and nowhere else, so they cannot be transposed.
    pub fn geometry(&self) -> (Cols, Rows) {
        (Cols::from_ws(self.ws_col), Rows::from_ws(self.ws_row))
    }
}

/// One read's worth of arena. Fixed, not a slice with a runtime length.
///
/// `read(2)` on a PTY returns **at most 4095 bytes** no matter what buffer you
/// offer, because `N_TTY_BUF_SIZE` is 4096 and it is a fixed array embedded in
/// `struct n_tty_data`. Measured: offering 4 KiB, 16 KiB, 64 KiB and 256 KiB
/// through a PTY gave 245.7 / 246.9 / 240.6 / 248.8 MB/s — flat. The length was
/// never variable; it was only ever untyped.
pub const SLOT_BYTES: usize = 4096;

/// The arena is exactly `SLOTS * SLOT_BYTES`, so there is no partial slot at
/// the end and the wrap is exact.
pub const SLOTS: usize = 256;

const _: () = assert!(SLOTS * SLOT_BYTES == 1 << 20);

/// How much of a slot a source actually filled. Provably `<= SLOT_BYTES`, so
/// it can never describe a region outside the slot it came from.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, FromBytes, IntoBytes, Immutable, KnownLayout,
)]
#[repr(transparent)]
pub struct Filled(pub(crate) u16);

impl Filled {
    pub const EMPTY: Filled = Filled(0);
    /// Clamps, so a source cannot report more than it was given.
    pub fn from_read(n: usize) -> Filled {
        Filled(n.min(SLOT_BYTES) as u16)
    }
    pub fn len(self) -> ByteLen {
        ByteLen::new(self.0)
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A writable slot of the arena, handed out by the store.
pub struct ArenaSlot<'a> {
    pub(crate) buf: &'a mut [u8; SLOT_BYTES],
    pub(crate) base: BytePos,
    pub(crate) idx: SlotIdx,
}

impl<'a> ArenaSlot<'a> {
    /// Not a runtime length — a constant. Kept as a method so call sites read
    /// the same whether or not the constant later changes.
    pub const fn capacity() -> ByteLen {
        ByteLen::new(SLOT_BYTES as u16)
    }
    pub fn base(&self) -> BytePos {
        self.base
    }
    pub fn idx(&self) -> SlotIdx {
        self.idx
    }
    /// For a source to write into. The one raw handle, named.
    ///
    /// Not an `AsMut` impl: that would make the raw handle reachable generically
    /// by anything with a `T: AsMut<_>` bound, which is the opposite of "named".
    #[allow(clippy::should_implement_trait)]
    pub fn as_mut(&mut self) -> &mut [u8; SLOT_BYTES] {
        self.buf
    }
}

/// Anything that can deliver bytes into the arena.
///
/// A PTY master, a pipe, a shared-memory ring, or a fixture replaying captured
/// `vim` output — the domain does not care which, and the differential test and
/// the PTY-bypass ring are both just further implementations.
pub trait Source {
    type Error;
    /// Fill the slot and report how much arrived.
    ///
    /// Returning [`Filled`] rather than `usize` does two things: a read length
    /// can never be mistaken for a column count, and it can never exceed the
    /// slot it describes.
    ///
    /// One slot per call. A pipe source gives up ~4% by not batching (measured
    /// 492.9 vs 513.6 MB/s at 4 KiB vs 16 KiB), which is the price of a single
    /// fixed shape for every transport.
    fn fill(&mut self, slot: ArenaSlot<'_>) -> Result<Filled, Self::Error>;
}

/// A writable slot handed to a [`Source`]. Not a `&mut [u8]`: the length is
/// [`SLOT_BYTES`], not whatever the caller feels like, and `commit` is the only
/// way to say how much arrived.
pub struct Slot<'a> {
    buf: &'a mut [u8],
    base: BytePos,
}

impl<'a> Slot<'a> {
    /// Only the store constructs one.
    pub fn new(buf: &'a mut [u8], base: BytePos) -> Slot<'a> {
        debug_assert_eq!(buf.len(), SLOT_BYTES);
        Slot { buf, base }
    }
    pub fn base(&self) -> BytePos {
        self.base
    }
    pub const fn capacity() -> ByteLen {
        ByteLen::new(SLOT_BYTES as u16)
    }
    /// Copy from a typed byte stream.
    ///
    /// Indexed rather than iterated for the same reason as `put_ascii_run`: this
    /// runs once per `read(2)` on the ingest path, so the loop's codegen is a
    /// measured quantity and not clippy's to choose.
    #[allow(clippy::needless_range_loop)]
    pub fn write(&mut self, src: &[StreamByte]) -> Filled {
        let n = src.len().min(SLOT_BYTES);
        for i in 0..n {
            self.buf[i] = src[i].raw();
        }
        Filled::from_read(n)
    }
    /// The raw handle a syscall needs. Named, and the only one.
    pub fn as_syscall_buf(&mut self) -> (*mut u8, usize) {
        (self.buf.as_mut_ptr(), SLOT_BYTES)
    }
}
