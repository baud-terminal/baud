use crate::*;
use zerocopy::{FromBytes, Immutable, IntoBytes, KnownLayout, TryFromBytes};

/// Bytes borrowed from the [`Store`].
///
/// Tied to `&Store`, so it cannot be held across the `&mut` drain that would
/// recycle them. The invariant "no run outlives its tier" is the borrow checker
/// rather than a generation counter.
pub struct Resident<'store> {
    /// Unproven on arrival: what the cold tier holds is whatever the
    /// application wrote, which may be binary. Callers scan it to earn a
    /// stronger [`Kind`] before treating it as text.
    bytes: Bytes<'store, Raw>,
}

impl<'store> Resident<'store> {
    pub fn bytes(&self) -> Bytes<'store, Raw> {
        Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        }
    }
}

/// Two-tier byte store.
///
/// The arena is 1 MiB because a size sweep peaked there against a 2 MiB L2 and
/// went *negative* by 64 MiB. At 280 MB/s it laps every 3.57 ms, so anything
/// retained must reach the cold ring first.
#[allow(dead_code)] // model only: the two-tier store is designed, not implemented
pub struct Store<'buf> {
    arena: &'buf mut [u8],
    cold: &'buf mut [u8],
    head: BytePos,
    drained: BytePos,
    cold_floor: BytePos,
}

impl<'buf> Store<'buf> {
    pub fn head(&self) -> BytePos {
        self.head
    }

    /// `None` means evicted: the bytes are gone and the line cannot be
    /// rendered, searched, or reflowed. Callers must handle it — there is no
    /// panicking accessor.
    pub fn resolve(&self, _pos: BytePos, _len: ByteLen) -> Option<Resident<'_>> {
        None // model only
    }

    /// Ingest may not lap the drain thread.
    ///
    /// Returns [`Lapped`] rather than silently discarding history. This is the
    /// one invariant the type system cannot express, so it is a `Result` the
    /// caller cannot ignore under `#[must_use]`.
    #[must_use = "a Lapped error means ingest overran the drain thread; dropping it silently discards history"]
    pub fn advance_head(&mut self, n: ByteLen) -> Result<BytePos, Lapped> {
        let next = self.head.advance(n);
        if next.gap(self.drained).exceeds(self.arena.len()) {
            return Err(Lapped);
        }
        self.head = next;
        Ok(next)
    }
}

/// Ingest outran the cold-tier writer. History would be lost.
#[derive(Debug)]
pub struct Lapped;

/// A run of bytes sharing one style, as stored.
///
/// Crosses the mmap boundary: the cold tier is a byte-mapped file reinterpreted
/// as `&[Run]` with no copy and no parse. That is what forces `_pad` to be
/// written out — zerocopy rejects implicit padding, because padding is
/// uninitialised memory and writing it to disk or shared memory leaks whatever
/// was there.
#[repr(C)]
#[derive(Clone, Copy, FromBytes, IntoBytes, Immutable, KnownLayout)]
pub struct Run {
    pub pos: BytePos,
    pub len: ByteLen,
    pub style: StyleId,
    /// [`Encoding`] as a byte. Stored rather than re-derived, so materialisation
    /// branches once per run instead of re-classifying what the scanner knew.
    /// Held as `u8` because not every bit pattern is a valid variant — recover
    /// it with [`Run::encoding`], which is checked.
    pub encoding: u8,
    /// [`CellFlags`] as a byte, for the same reason `encoding` is one: the
    /// struct crosses the mmap boundary and zerocopy will not take a type whose
    /// bit patterns are not all valid. Recover it with [`Run::flags`].
    ///
    /// A run carries the protection so that promoting it to cells does not lose
    /// it -- the run *is* the storage in `OpMode::Runs`, and a promotion that
    /// dropped the flag would make protection depend on when the line happened
    /// to materialise.
    pub flags: u8,
    pub _pad: [u8; 2],
}

impl Run {
    /// Checked: a byte off disk is not necessarily a valid [`Encoding`].
    pub fn encoding(&self) -> Option<Encoding> {
        Encoding::try_read_from_bytes(&[self.encoding]).ok()
    }
    /// Unchecked, unlike [`Run::encoding`]: every bit pattern is a valid flag
    /// set, so there is nothing to reject.
    pub fn flags(&self) -> CellFlags {
        CellFlags::from_bits(self.flags)
    }
}

/// Half-open range into the shared run pool.
#[derive(Clone, Copy)]
pub struct RunRange {
    pub start: RunIdx,
    pub count: RunCount,
}

/// A number of runs.
#[derive(
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    FromBytes,
    IntoBytes,
    Immutable,
    KnownLayout,
)]
#[repr(transparent)]
pub struct RunCount(pub(crate) u16);

impl RunCount {
    pub const ZERO: RunCount = RunCount(0);
    pub fn from_index(n: usize) -> RunCount {
        RunCount(n as u16)
    }
    pub fn inc(self) -> RunCount {
        RunCount(self.0 + 1)
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

mod kind {
    pub trait Sealed {}
    impl Sealed for super::Raw {}
    impl Sealed for super::Printable {}
    impl Sealed for super::Utf8 {}
    impl Sealed for super::DecGraphics {}
}

/// How a run of bytes is to be interpreted.
///
/// Not future-proofing: the VT protocol switches this mid-stream. `ESC ( 0`
/// selects DEC Special Graphics for G0, after which `q` is `─` and `x` is `│`
/// — that is how ncurses draws boxes, and it applies to subsequent bytes until
/// switched back. A per-run tag is what the protocol actually requires.
///
/// Stored on [`Run`] so materialisation branches once per run instead of
/// re-deriving what the scanner already knew. One byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug, TryFromBytes, IntoBytes, Immutable, KnownLayout)]
#[repr(u8)]
pub enum Encoding {
    /// Printable ASCII, `0x20..=0x7e`. One byte, one column, no table lookup.
    /// The ~94% path.
    Ascii,
    /// Well-formed UTF-8. Decode to graphemes for width.
    Utf8,
    /// DEC Special Graphics (`ESC ( 0`). Bytes map through a 96-entry table to
    /// box-drawing codepoints; width is always 1.
    DecGraphics,
    /// Uninterpretable. Render U+FFFD per byte, width 1 each. `cat /bin/ls`
    /// lands here and must render rather than be rejected.
    Raw,
}

impl Encoding {
    /// Whether width can be had without decoding. True for every fixed-width
    /// single-byte encoding, which is the property that matters — not
    /// "is it ASCII".
    pub fn is_byte_per_column(self) -> bool {
        matches!(
            self,
            Encoding::Ascii | Encoding::DecGraphics | Encoding::Raw
        )
    }
}

/// What a [`Bytes`] has been *proven* to hold. A `&[u8]` carries no such proof,
/// which is why one may not appear in a domain signature.
///
/// The static counterpart of [`Encoding`]: same information, no runtime tag,
/// used while the bytes are in flight. [`Kind::ENCODING`] is the bridge.
pub trait Kind: kind::Sealed {
    const ENCODING: Encoding;
}

/// Straight off the wire. No guarantees: a PTY delivers `cat /bin/ls` as
/// readily as text.
pub enum Raw {}

/// Verified printable ASCII, `0x20..=0x7e`. Width is exactly one column per
/// byte — the only case where a byte count legitimately *is* a column count.
pub enum Printable {}

/// Verified well-formed UTF-8. Width still unknown until measured.
pub enum Utf8 {}

/// DEC Special Graphics, in flight.
pub enum DecGraphics {}

impl Kind for Raw {
    const ENCODING: Encoding = Encoding::Raw;
}

impl Kind for Printable {
    const ENCODING: Encoding = Encoding::Ascii;
}

impl Kind for Utf8 {
    const ENCODING: Encoding = Encoding::Utf8;
}

impl Kind for DecGraphics {
    const ENCODING: Encoding = Encoding::DecGraphics;
}

/// A byte view carrying a proof of its contents.
#[derive(Clone, Copy)]
pub struct Bytes<'a, K: Kind> {
    raw: &'a [u8],
    _k: core::marker::PhantomData<K>,
}

impl<'a> Bytes<'a, Raw> {
    /// The single entry point for wire data. Everything downstream must earn a
    /// stronger [`Kind`] by passing a scanner.
    pub fn from_source(raw: &'a [u8]) -> Bytes<'a, Raw> {
        Bytes {
            raw,
            _k: core::marker::PhantomData,
        }
    }
    /// Split off the leading run of printable ASCII. This is the SIMD scan, and
    /// the returned `Printable` view is the proof it produced.
    pub fn split_printable(self) -> (Bytes<'a, Printable>, Bytes<'a, Raw>) {
        let n = self
            .raw
            .iter()
            .take_while(|&&b| (0x20..0x7f).contains(&b))
            .count();
        (
            Bytes {
                raw: &self.raw[..n],
                _k: core::marker::PhantomData,
            },
            Bytes {
                raw: &self.raw[n..],
                _k: core::marker::PhantomData,
            },
        )
    }
    /// Promote to UTF-8, or hand the bytes back unproven.
    pub fn validate_utf8(self) -> Result<Bytes<'a, Utf8>, Bytes<'a, Raw>> {
        match core::str::from_utf8(self.raw) {
            Ok(_) => Ok(Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            }),
            Err(_) => Err(self),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
}

impl<'a> Bytes<'a, Printable> {
    /// Free, and needs no [`WidthTable`] — the proof that these are printable
    /// ASCII is exactly the proof that one byte is one column. This is the
    /// ~94% fast path, and the type is what makes skipping the table sound.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

impl<'a> Bytes<'a, Utf8> {
    // `measure` lived here and returned `from_cells(0)` behind a `// model
    // only` comment, taking a `WidthTable` it discarded. Nothing called it. It
    // is deleted rather than fixed because measuring a stored UTF-8 run is for
    // reflow, selection and search, none of which exist -- CLAUDE.md lists all
    // three under "unmeasured, do not assert". An API that answers zero looks
    // implemented; its absence does not.
    pub fn len(&self) -> ByteLen {
        ByteLen::new(self.raw.len() as u16)
    }
}

impl<'a> Bytes<'a, Raw> {
    /// Reinterpret under the active character set. `ESC ( 0` makes subsequent
    /// bytes graphics; this is where that decision lands.
    pub fn as_dec_graphics(self) -> Bytes<'a, DecGraphics> {
        Bytes {
            raw: self.raw,
            _k: core::marker::PhantomData,
        }
    }
}

impl<'a> Bytes<'a, DecGraphics> {
    /// Fixed-width single-byte: one column each, no table.
    pub fn width(&self) -> DisplayWidth {
        DisplayWidth::from_len(ByteLen::of(self.raw))
    }
    pub fn len(&self) -> ByteLen {
        ByteLen::of(self.raw)
    }
}

/// The dynamic form: a run whose encoding is carried rather than proven.
/// This is what gets stored, and what a `match` over stored runs sees.
pub struct Encoded<'a> {
    bytes: Bytes<'a, Raw>,
    encoding: Encoding,
}

impl<'a> Encoded<'a> {
    pub fn encoding(&self) -> Encoding {
        self.encoding
    }
    /// Width without decoding, where the encoding permits it. `None` means the
    /// caller must decode — which is exactly the branch materialisation wants
    /// to take once per run.
    pub fn width_fast(&self) -> Option<DisplayWidth> {
        self.encoding
            .is_byte_per_column()
            .then(|| DisplayWidth::from_len(ByteLen::of(self.bytes.raw)))
    }
    /// Checked downcast back to a static proof.
    pub fn as_utf8(self) -> Option<Bytes<'a, Utf8>> {
        (self.encoding == Encoding::Utf8).then_some(Bytes {
            raw: self.bytes.raw,
            _k: core::marker::PhantomData,
        })
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Static proof to dynamic tag. Free — `K::ENCODING` is a constant.
    pub fn erase(self) -> Encoded<'a> {
        Encoded {
            bytes: Bytes {
                raw: self.raw,
                _k: core::marker::PhantomData,
            },
            encoding: K::ENCODING,
        }
    }
}

impl<'a, K: Kind> Bytes<'a, K> {
    /// Named so every crossing back to an untyped slice is visible. Used by the
    /// renderer and by `memcpy` into the cold tier, and nowhere else.
    pub fn as_slice(&self) -> &'a [u8] {
        self.raw
    }
}

impl Run {
    /// Set the encoding from the typed enum rather than casting at the call
    /// site. The field is `u8` because a fieldless enum cannot be `FromBytes`
    /// — a byte off disk is not necessarily a valid variant — so recovery goes
    /// through the checked [`Run::encoding`].
    pub fn with_encoding(mut self, e: Encoding) -> Run {
        self.encoding = e as u8;
        self
    }
    pub fn with_flags(mut self, f: CellFlags) -> Run {
        self.flags = f.bits();
        self
    }
}
