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

mod arrows;
mod clip;
mod colour;
mod cwd;
mod dcs;
mod feed;
mod file;
mod font;
mod graph;
mod hardcopy;
mod iterm;
#[cfg(feature = "diag")]
pub use graph::Hist;
mod kitty;
mod locator;
mod logging;
mod margin;
mod notice;
mod ops;
mod osc;
mod paint;
mod path;
mod placed;
mod presentation;
mod print;
mod prompt;
mod query;
mod rect;
mod regis;
mod search;
mod shift;
mod sixel;
mod stack;
mod status;
mod storage;
mod term;
mod tsr;
mod variants;
mod vt52;
mod window;
mod write;

pub use baud_domain::{
    ByteLen, ByteLen as DomByteLen, BytePos, Charset, Codepoint, Col, Cols, Content, FinalByte,
    InputEncoding, KeypadMode, LineRendition, PackedCell, PastEdge, ReadLen, Row, RowBlock,
    RowExtent, Rows, StyleWord, Width, WinSize,
};
// **The key API's own vocabulary.** `Term::key` takes a `KeyEvent`, so a
// caller that cannot name one cannot call it -- an entry point whose argument
// type is unreachable is the same failure as a mode recorded and never read.
pub use baud_domain::FileHost;
/// Focus reporting's vocabulary, for the same reason: an embedder that cannot
/// name a [`Focus`] cannot call [`Term::focus`].
pub use baud_domain::Focus;
/// What [`Term::pointer_shape`] returns: `OSC 22`'s shape.
pub use baud_domain::PointerShape;
/// What [`Term::style_reclaims`] returns: the rendition table's evictions,
/// sweeps and the slots they freed.
pub use baud_domain::Reclaims;
/// The reply hold's switch: an embedder that cannot name a [`ReplyHold`]
/// cannot call [`Term::set_reply_hold`].
pub use baud_domain::ReplyHold;
/// The resources `Term::resource` reads.
pub use baud_domain::Resource;
/// What [`Term::set_term_name`] takes: the `TERM` XTGETTCAP's `TN` names.
pub use baud_domain::TermName;
/// VT52 is a protocol an embedder can be in, so it can be asked about.
pub use baud_domain::TerminalMode;
/// Notifications and progress from programs (`OSC 9`, `777`, `99`,
/// `9 ; 4`, `1337 ; RequestAttention`), what an embedder delivers and
/// tells the core.
pub use baud_domain::{
    Activation, Attention, ButtonSupport, CloseEvents, CloseReport, Expiry, ExpirySupport,
    IconSupport, Notice, NoticeAction, NoticeBudget, NoticeButton, NoticeFrom, NoticeRefusals,
    NoticeSupport, NoticeToken, Occasion, Occasions, Percent, Progress, ProgressState, Sounds,
    Urgencies, Urgency,
};
/// The status line's vocabulary: what [`Term::status_type`] and
/// [`Term::active_display`] answer, and the row an embedder lends it
/// ([`Storage::with_status_line`]).
pub use baud_domain::{ActiveDisplay, StatusType};
/// Mouse reporting's vocabulary: an embedder that cannot name a
/// `MouseEvent` cannot call [`Term::mouse_report`] -- and since `?1016` an
/// event carries its pixel position, a `PointerPx` of two `ImagePx`. And
/// `?1001`'s: what [`Term::highlight`] says to draw, in `CellAt`s.
pub use baud_domain::{
    AlternateScroll, CellAt, Highlighted, ImagePx, MouseButton, MouseEncoding, MouseEvent,
    MouseKind, MouseMods, MouseTracking, PointerHides, PointerMode, PointerPx, ShiftEscape,
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
/// `?1042` and `?1043`, which an embedder's bell reads, and the bells'
/// volumes and `?44`.
pub use baud_domain::{BellIsUrgent, BellVolume, BellsRung, MarginBell, PopOnBell, ScrollbarShown};
/// What [`Term::bidi_support`] returns: BDSM's state.
pub use baud_domain::{BidiDirection, BidiSupport};
/// Control strings' vocabulary: `Chan` names a palette entry and `LinkId`
/// an `OSC 8` hyperlink, and both are what the accessors return; `Video` is
/// the video a frame is drawn in, which `Term::cursor_color_in` takes.
pub use baud_domain::{
    Chan, ClipboardBudget, ClipboardContent, ClipboardRefusals, ClipboardWrite, ColorWord,
    DynamicColor, LinkId, Osc, Selection, Selections, StringTerm, Video, XColor,
};
/// `OSC 133`'s vocabulary: how a command ended, what a click at a prompt
/// sends ([`Term::prompt_click`]), and what a prompt entry holds.
pub use baud_domain::{
    ClickEvents, ExitCode, ExitStatus, MoveKeys, PromptCapacity, PromptEntry, PromptsForgotten,
};
/// The cluster store's vocabulary: what [`Term::cluster_epoch`] and
/// [`Term::cluster_sweeps`] return, and the store itself, which a test
/// drives to read where a cluster lands.
pub use baud_domain::{ClusterEpoch, ClusterLive, ClusterStore, ClusterSweeps, Refused};
/// xterm's special colours and their modes: what [`Term::special_color`]
/// and [`Term::color_modes`] take and return.
pub use baud_domain::{ColorMode, ColorModes, SpecialColor};
/// `OSC 50`'s vocabulary, `baud-domain`'s: what an embedder tells the core
/// of its font and reads off a queued change.
pub use baud_domain::{FontChanges, FontSize, SizeStep};
/// The window requests `Term::window_requests` holds, and the setting
/// `Term::set_window_ops` takes.
pub use baud_domain::{
    FullScreen, LogMode, Logging, Maximize, Osc52Writes, PasteControls, SizeAsk, WindowAbilities,
    WindowManipulation, WindowOps, WindowRequest,
};
pub use baud_domain::{KeyCode, KeyEvent, KeyEventKind, KeypadKey, KittyFlags, Modifiers, PuaKey};
/// DEC's text locator's vocabulary: what [`Term::locator`] and
/// [`Term::set_locator_device`] take.
pub use baud_domain::{
    LocatorAt, LocatorButton, LocatorButtons, LocatorDevice, LocatorInput, LocatorKind,
};
pub use baud_domain::{NEEDLE_MAX, Needle, NeedleRefused};
/// Printing's vocabulary: what [`Term::set_printer`] takes and what
/// [`Term::print_jobs`], [`Term::print_run`] and [`Term::print_refusals`]
/// answer.
pub use baud_domain::{
    PRINT_QUEUE, PrintBudget, PrintCut, PrintJob, PrintRefusals, PrintRun, PrinterAsked,
    PrinterHold, PrinterPort,
};
/// The page-width request's vocabulary: an embedder that cannot name a
/// [`WidthHold`] cannot call [`Term::set_width_hold`], nor read what
/// [`Term::requested_width`] answers without a [`WidthRequest`].
pub use baud_domain::{PageWidth, WidthHold, WidthRequest};
/// The window facts `Term::set_window` takes.
pub use baud_domain::{PixelSize, ScreenCoord, ScreenPoint, WindowFacts, WindowState};
/// ReGIS's delay: an embedder that cannot name a [`RegisHold`] cannot
/// call [`Term::set_regis_hold`], nor read [`Term::take_regis_wait`]'s
/// ticks without [`RegisTicks`]; nor route its keys and pointer by
/// [`Term::regis_input`] without [`RegisInput`].
pub use baud_domain::{RegisHold, RegisInput, RegisTicks};
/// A reading of how many `OSC 52` writes have been published.
pub use clip::ClipboardWrites;
pub use cwd::{DirectoriesRefused, WorkingDirectory};
pub use dcs::TcapSetsRefused;
pub use font::{FONT_NAME_MAX, FONT_QUEUE, FontChange, FontFamily, FontsRefused};
pub use graph::Node;
pub use iterm::ItermCounts;
pub use logging::LogsRefused;
pub use notice::ReportCount;
pub use search::{Found, SearchLines, Spot};
pub use status::StatusBuf;
pub use term::StatusLine;
pub use term::{ClickHow, ClickKeys, PromptClick, PromptJump, PromptLine, PromptMark};
pub use variants::VariantsRefused;
pub use window::{PropertiesDropped, WINDOW_QUEUE, WindowOpsRefused, WindowRequestsDropped};

/// What `Owned::with_history` takes beside a depth. Re-exported so a caller
/// that chooses its scrollback -- which `multibench` must, to hand every core
/// the same number -- can name the argument without a second crate.
pub use baud_domain::CacheGeometry;
/// Scrollback: the lines behind the screen's window onto the ring.
pub use baud_domain::{Back, Depth, Scrollback, Wrap};
/// kitty's colour control in flight, which a caller lending its own storage
/// provisions with [`StrBuf::new`].
pub use colour::KittyColours;
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
    NSTYLE_MAX, OSC_MAX, PALETTE_LEN, READ_CHUNK, REPLY, REPLY_MOST, Storage, StrBuf, StyleBook,
    StyleTable, TITLE_SLOTS, TITLE_STACK, grid_stride,
};
/// Which surface is live, for a caller asking `Term::screen`.
pub use term::{
    ColumnRefusals, Erasure, HistoryErasures, HistoryMark, HistoryWipes, Looking, Moved, Overrides,
    PaintRole, ProgramPaint, RegionScrolls, Resets, Resized, Rest, ReverseFlips, RowMoves, Screen,
    ScreenEpoch, SelectionColors, SurfaceLine, Term, TextMoved, ThemeColors, TitleReportsRefused,
    UpdatesOpened, WatchedLines, WidthRequests, WrapPending, Written, Zone,
};
