//! The terminal core, as a block-driven graph.
//!
//! **Why this crate replaced `baud-core2`.** The previous core reached
//! 456 MB/s on `cat-cjk.raw` and could not be made materially faster by
//! optimising anything it *did*: ceiling arms priced every decode, every table
//! lookup, every cluster decision, every cell write and every scroll at **45%
//! between them**, so removing all of it left the recording at ~660. What
//! remained was the ingest loop, and the corpus said its cost is per
//! *iteration*: `cat`, which iterates once per printable run, floors at 2830,
//! and `cat-cjk`, which iterates once per 2.23-byte character, floors at 670.
//!
//! Iterating sixteen bytes at a time instead measured **14,100** on the same
//! nothing -- 21x. That is the one mechanism in this project's history whose
//! arithmetic reaches 1000 MB/s on `cat-cjk`, and this crate is built around
//! it rather than around a faster version of the per-character path.
//!
//! `docs/record.md` carries the measurements;
//! `docs/research/notes/b-parser-as-a-graph.md` carries the literature on
//! storing the automaton.
//!
//! **What is reused and what is not.** The foundation is `baud-domain`: the
//! grid, the cell, the generated Unicode tables and the VT vocabulary, all of
//! which are already compile-time lookups and were worth +17%. The vector
//! primitives come from there too, through `core::simd`, except the run scans
//! that `neon-scan` and `avx2-scan` route to `baud-isa-aarch64` and
//! `baud-isa-x86-64`.
//!
//! This crate once declared `baud-isa`, `baud-isa-x86-64` and
//! `baud-isa-aarch64` and referenced none of them in a single line of source,
//! while this paragraph said it used them for the vectors. Today they are
//! optional and target-gated, pulled in by `neon-scan` (the default) and
//! `avx2-scan`, and called only through safe wrappers. The one unconditional
//! dependency is `baud-domain`, which brings `zerocopy` from crates.io; there
//! is no `alloc` outside the `alloc` feature, and no `std`.

#![no_std]
// **Unconditional, including with `neon-scan` on**, and that took an API fix
// rather than an exemption. The first version of the NEON arm dropped this
// attribute under the feature, because `Scan`'s methods are `unsafe fn` and
// `forbid` cannot be relaxed by an `allow`. But that contract is about `Avx2`,
// where AVX2 is not baseline and a caller can genuinely invoke what the machine
// cannot execute; on aarch64 NEON *is* baseline, so the obligation is satisfied
// by the target and there is nothing for a caller to uphold. Propagating an
// `unsafe` block outward for a guarantee the compiler already made is noise, so
// `baud-isa-aarch64` grew a safe inherent surface over its own trait impl and
// this crate calls that. The raw load lives where it is checked.
#![forbid(unsafe_code)]

// **Only for [`Owned`], and only behind the feature.** The default build and
// the `x86_64-unknown-none` target still see nothing but `core`.
#[cfg(feature = "alloc")]
extern crate alloc;

mod clip;
mod dcs;
mod feed;
mod graph;
#[cfg(feature = "diag")]
pub use graph::Hist;
mod ops;
mod osc;
mod paint;
mod placed;
mod prompt;
mod sixel;
mod storage;
mod term;
mod vt52;
mod write;

pub use baud_domain::{
    ByteLen, ByteLen as DomByteLen, BytePos, Charset, Codepoint, Col, Cols, Content, FinalByte,
    InputEncoding, KeypadMode, PackedCell, ReadLen, Row, RowBlock, RowExtent, Rows, StyleWord,
    Width, WinSize,
};
// **The key API's own vocabulary.** `Term::key` takes a `KeyEvent`, so a
// caller that cannot name one cannot call it -- an entry point whose argument
// type is unreachable is the same failure as a mode recorded and never read.
/// Focus reporting's vocabulary, for the same reason: an embedder that cannot
/// name a [`Focus`] cannot call [`Term::focus`].
pub use baud_domain::Focus;
/// VT52 is a protocol an embedder can be in, so it can be asked about.
pub use baud_domain::TerminalMode;
/// Mouse reporting's vocabulary: an embedder that cannot name a
/// `MouseEvent` cannot call [`Term::mouse_report`] -- and since `?1016` an
/// event carries its pixel position, a `PointerPx` of two `ImagePx`.
pub use baud_domain::{
    AlternateScroll, HighlightRegion, HighlightTracking, ImagePx, MouseButton, MouseEncoding,
    MouseEvent, MouseKind, MouseMods, MouseTracking, PointerPx,
};
/// The clustering and width vocabulary the encoding suites name. All of it is
/// `baud-domain`'s -- this crate owns neither UAX #29 nor East Asian Width.
pub use baud_domain::{
    AmbiguousWidth, ClusterBreak, ClusterId, ClusterSlot, GraphemeClass, StreamByte,
};
/// The vocabulary the conformance suites name: a national replacement
/// character set, a private-mode number, and the mode state a `DECRQM`
/// reports. All three are `baud-domain`'s, not this crate's.
pub use baud_domain::{
    Attrs, C1Transmit, Csi, CursorBlink, CursorPick, CursorShape, CursorStyle, DecSet, FnKey,
    GSlot, Level, Marker, ModeState, Nrcs, Params,
};
/// Control strings' vocabulary: `Chan` names a palette entry and `LinkId`
/// an `OSC 8` hyperlink, and both are what the accessors return.
pub use baud_domain::{
    Chan, ClipboardBudget, ClipboardContent, ClipboardRefusals, ClipboardWrite, ColorWord,
    DynamicColor, LinkId, Osc, Selection, Selections, StringTerm, XColor,
};
pub use baud_domain::{KeyCode, KeyEvent, KeyEventKind, KeypadKey, KittyFlags, Modifiers, PuaKey};
/// A reading of how many `OSC 52` writes have been published.
pub use clip::ClipboardWrites;
pub use graph::Node;

/// What `Owned::with_history` takes beside a depth. Re-exported so a caller
/// that chooses its scrollback -- which `multibench` must, to hand every core
/// the same number -- can name the argument without a second crate.
pub use baud_domain::CacheGeometry;
/// Scrollback: the lines behind the screen's window onto the ring.
pub use baud_domain::{Back, Depth, Scrollback, Wrap};
/// The longest single reply the core writes, a DECRQSS rendition string, for
/// a caller sizing what it drains into.
pub use dcs::DECRQSS_MAX;
/// The sixteen allocations a `Storage` borrows, for a caller that has an
/// allocator. Not `Storage::boxed`: this `Storage` holds borrows, so there is
/// nothing for a `Box` to own.
#[cfg(feature = "alloc")]
pub use storage::Owned;
pub use storage::{
    ARENA, CELLS3, CELLS3_PREFIXES, GRID, LINK_MAX, MAX_COLS, MAX_ROWS, NCLUSTER, NLINK, NSTYLE,
    NSTYLE_MAX, OSC_MAX, PALETTE_LEN, READ_CHUNK, REPLY, Storage, StrBuf, StyleBook, StyleTable,
    grid_stride,
};
/// Which surface is live, for a caller asking `Term::screen`.
pub use term::{
    HistoryErasures, HistoryMark, Moved, Overrides, RegionScrolls, Resized, ReverseFlips, RowMoves,
    Screen, ScreenEpoch, Term, ThemeColors, UpdatesOpened, Written, Zone,
};
