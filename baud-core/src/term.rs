//! The terminal's state, and the one borrow the parser holds.

use crate::graph::Node;
use crate::osc::StrKind;
use crate::storage::{LINK_MAX, MAX_COLS, MAX_ROWS, Storage};
use baud_domain::*;

/// A terminal: a cursor, a rendition, a grid, and where the parser stands.
///
/// **All of it behind one `&mut self`.** A C prototype of this loop used
/// file-scope globals and ran 16-34% slower; `__restrict` recovered only 2.5%
/// of that. The presumed mechanism is LLVM's `noalias`, and the rule that
/// falls out is that state does not migrate to statics.
/// Which screen buffer is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Normal,
    Alternate,
}

/// What `DECSC` saves and `DECRC` restores.
#[derive(Clone, Copy)]
pub struct Saved {
    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) style: StyleId,
    pub(crate) style_raw: StyleWord,
    pub(crate) flags: CellFlags,
    /// **The four slots and which one `GL` invokes, not the resolved set.**
    /// `[[source: vt510-decsc]]` lists "character sets G0-G3" among what DECSC
    /// saves, and a saved *resolved* set restores into slots that no longer
    /// agree with it -- the next `SI` would recompute from `g[gl]` and throw
    /// the restore away.
    pub(crate) g: [Charset; 4],
    pub(crate) gl: GSlot,
    /// **`DECSCA`'s protection, which is not the same word as `flags`.**
    /// `[[source: vt510-decsc]]` lists "selective erase attribute" among what
    /// `DECSC` saves. `flags` is what the write path stamps into a cell and
    /// `protect` is the bit `DECSCA` sets for the *next* write; saving only
    /// the first meant a `DECRC` restored the rendition and left the
    /// protection wherever the stream had since moved it, so a selective erase
    /// after a restore spared the wrong cells.
    pub(crate) protect: CellFlags,
    /// DECAWM, because `[[source: vt510-decsc]]` lists "Wrap flag (autowrap or
    /// no autowrap)" among the seven things DECSC saves.
    pub(crate) autowrap: ModeChange,
    /// DECOM, likewise: "State of origin mode (DECOM)".
    ///
    /// **Restoring it is not cosmetic.** Origin mode decides where home *is*,
    /// so an application that sets it, saves, resets it and restores must get
    /// its own coordinate system back -- and until this was read off the
    /// manual rather than assumed, neither core restored either flag.
    pub(crate) origin: ModeChange,
}

/// How many `SGR` sequences the byte-keyed table holds.
///
/// **Measured against the corpus, not chosen.** The distinct plain-`SGR`
/// sequences are: `btop.raw` **168**, `asciiquarium.raw` **13**,
/// `git-log.raw` 7, `vim-unicode.raw` 6, `less-cjk.raw` 3 -- so 256 slots
/// hold every recording's whole working set and the table is exact rather
/// than lossy in practice. It still **evicts on collision**, the same
/// contract as the style and `OSC 8` link tables, because the key is
/// attacker-supplied content and a growable table is the content-addressing
/// failure mode this repo already records.
#[cfg(feature = "sgr-table")]
pub const NSGR: usize = 256;

/// Which half of the screen's life the terminal is in, from `OSC 133`.
///
/// **The one thing a resize needs to know.** A shell redraws its prompt
/// after a resize by counting the rows it occupies, moving the cursor up
/// by that many and erasing from there -- so re-cutting those rows to a
/// new width changes the count behind its back, the erase starts below
/// the prompt's first line, and that line survives. Every resize leaves
/// another. Re-cutting *output* has no such reader: nothing will redraw
/// it, which is exactly why re-cutting it is worth doing.
///
/// So the boundary is not a row, it is a state: the shell owns the
/// screen from prompt start until the command's output begins, and the
/// application owns it after. `docs/research/notes/m-resize-and-reflow.md`
/// carries the measurement -- four copies against one on a capture of a
/// real shell replayed with real resizes -- and what the alternatives
/// cost.
///
/// A terminal that has seen no marks is in [`Zone::Output`], which is
/// the behaviour that shipped before them: a stream with no shell
/// integration re-cuts as it always did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Zone {
    /// From `OSC 133 ; A` or `; P` until `; C` or `; D`: the shell is
    /// drawing or the user is typing, and the shell will redraw this on a
    /// resize. `D` closes it as `C` does, because a command that ran
    /// nothing -- Enter on an empty line -- sends no `C`.
    Prompt,
    /// Everywhere else, and where a stream sends no marks at all.
    #[default]
    Output,
}

/// How the window moved between two rotations; see [`Term::moved_since`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moved {
    Retired(Rows),
    Restored(Rows),
}

/// The state a control sequence changes that the write loop does not read.
#[derive(Clone, Copy)]
pub struct Cold {
    /// Repeated `SGR` sequences, answered from their bytes.
    ///
    /// **`SGR` is 63% of `btop.raw`'s control sequences, 86% of
    /// `asciiquarium.raw`'s and 100% of `git-log.raw`'s**, and a ceiling arm
    /// that makes the whole route free measures **+34% on btop and +29% on
    /// asciiquarium**. Parsing one is a pure function of its bytes, and the
    /// result is a masked replace of the rendition word, so the whole route --
    /// the parameter walk, `SgrAttr::parse`, and the fold -- collapses to one
    /// compare and one `StyleFold::apply`.
    #[cfg(feature = "sgr-table")]
    pub(crate) sgr: [SgrSlot; NSGR],
    /// DECSCA protection, kept apart from the cell flags so the charset and
    /// the protection each change without recomputing the other wrongly.
    /// DECKPAM/DECKPNM, `ESC =` and `ESC >`.
    ///
    /// **Not a mode bit.** It has no `SM`/`RM` wire number to live under, and
    /// it is input-side: it decides what the keypad sends, which is why the
    /// core's part is to record it and expose it.
    /// **The four G slots, and which one `GL` reads through.**
    ///
    /// Here rather than beside `charset` on the hot path, and that split is
    /// the same one `style`/`style_raw` already makes: designation and the
    /// locking shifts are control sequences, and what the per-character loop
    /// needs is the *resolved* set. `Paint::charset` stays one byte and
    /// `Paint::invoke` re-derives it whenever a slot or `gl` moves, so
    /// `put_ascii_run` reads exactly what it read before.
    ///
    /// `baud-core` had only the resolved byte, which made designation and
    /// invocation the same act: `ESC ) 0` changed what G0 rendered and
    /// `SO`/`SI` did nothing at all.
    pub(crate) g: [Charset; 4],
    pub(crate) gl: GSlot,
    /// **Mouse reporting, all of it in `Cold`.** Every field here is written
    /// by a control sequence and read by an embedder calling
    /// [`Term::mouse_report`] -- never by the per-byte loop -- so it costs the
    /// ingest path nothing at all.
    pub(crate) mouse: MouseTracking,
    /// `?1006`. Resetting it returns to X10, which is the protocol's default
    /// rather than "off": there is no off.
    pub(crate) mouse_encoding: MouseEncoding,
    /// `?1001`. Recorded and exposed, because a core with no selection model
    /// has no other way to act on it -- and answering `DECRQM` truthfully
    /// requires knowing.
    pub(crate) highlight: HighlightTracking,
    pub(crate) highlight_region: HighlightRegion,
    /// **The dynamic colours, `OSC 10` and `OSC 11`, as two pairs.** `defaults`
    /// is what the embedder's theme says the foreground and background are,
    /// pushed in through [`Term::set_default_colors`]; `dynamic` is an
    /// application's override from `OSC 10`/`11`, `DEFAULT` meaning none.
    /// A query answers the override if there is one, else the default if the
    /// embedder supplied one, else nothing -- the same refusal `OSC 4` makes,
    /// because this core holds no palette of its own and naming a colour the
    /// renderer does not use is a wrong-shaped answer. `OSC 110`/`111` clear
    /// the override and leave the default, which is what "reset" means. Here
    /// rather than in `StrBuf` so no caller-provided buffer grows a field.
    pub(crate) defaults: [ColorWord; 2],
    pub(crate) dynamic: [ColorWord; 2],
    /// Which control string is being accumulated, and how much of it.
    ///
    /// In `Cold` because a control string is not the write path: `str_kind` is
    /// never read while text is being written. The write path does dereference
    /// `Cold` per glyph -- for the margins, `modes` and `ambiguous` -- so the
    /// accurate claim is narrower than it once read: a control string adds
    /// nothing to the loads the loop already makes. They are state that outlives a `feed`,
    /// because a string spans reads.
    pub(crate) str_kind: StrKind,
    pub(crate) str_len: ByteLen,
    /// ANSI or VT52, and the row byte a half-finished `ESC Y` is holding.
    ///
    /// **Both land in existing padding**: `size_of::<Cold>()` does not change.
    /// And both are in `Cold` rather than on `Paint` because the per-byte loop
    /// must never read them -- the protocol is in the *node*, so the mode is
    /// only ever consulted when something asks which one is live.
    pub(crate) terminal: TerminalMode,
    pub(crate) vt52_row: Vt52Coord,
    /// `DECSCUSR`. Which cursor a renderer should draw, and whether it
    /// blinks. Recorded and exposed for the reason every renderer-side mode
    /// here is: an embedder cannot honour it without being told, and a shape
    /// parsed and dropped is worse than one not implemented.
    /// How East Asian Ambiguous characters are measured.
    ///
    /// **A policy the embedder owns, which is why the property table stores a
    /// width *code* rather than a resolved `Width`.** Narrow is the default
    /// and is xterm's `cjkWidth=false`; a terminal configured for CJK text
    /// sets Wide, and the same codepoint then takes two columns. Nothing in
    /// the protocol changes it -- there is no escape sequence for it -- so it
    /// arrives through the API and lives in `Cold` with the rest of the state
    /// a control sequence does not touch.
    /// `DECSLRM`'s left and right margins.
    ///
    /// **Cold, because a control sequence moves them and the write path reads
    /// a resolved answer.** The same split as `g`/`gl` and `style_raw`: the
    /// hot loop asks `Paint::right` once a run rather than re-deriving the
    /// margin per character.
    pub(crate) left: Col,
    pub(crate) right: Col,
    /// How incoming bytes are decoded. `ESC % @` / `ESC % G` select it.
    ///
    /// **In `Cold` rather than on `Term` because `Paint` has to write it.**
    /// The loop carries a live copy so the table selection is a register
    /// rather than a field read per byte; this is where it persists between
    /// reads, and `esc_final` sets both.
    /// `S7C1T` / `S8C1T`. How wide a `CSI` this core *writes*.
    ///
    /// Reading is a different question and is settled by `encoding`: an
    /// 8-bit `9B` can only be a control under Latin-1. Transmitting is the
    /// application's choice and it makes it with `ESC SP F` / `ESC SP G`.
    pub(crate) c1: C1Transmit,
    /// XTMODKEYS `modifyOtherKeys`, and XTFMTKEYS `formatOtherKeys`.
    ///
    /// **In `Cold` because they are written once per control sequence and read
    /// once per key press, which is the split this struct exists for.** Neither
    /// is on the ingest path at all: no byte of `feed` reads them.
    pub(crate) other_keys: OtherKeys,
    pub(crate) other_keys_format: OtherKeysFormat,
    pub(crate) encoding: InputEncoding,
    pub(crate) ambiguous: AmbiguousWidth,
    pub(crate) cursor_style: CursorStyle,
    pub(crate) keypad: KeypadMode,
    pub(crate) protect: CellFlags,
    pub(crate) screen: Screen,
    /// The scrolling region, inclusive at both ends.
    pub(crate) top: Row,
    pub(crate) bot: Row,
    pub(crate) saved: Option<Saved>,
    pub(crate) alt_saved: Option<Saved>,
    pub(crate) modes: Modes,
    /// **The kitty keyboard flag stacks, one per screen.** `kitty` is the
    /// active screen's and `kitty_alt` the parked one; the `?1049` transition
    /// swaps them, which is the specification's separate main/alternate stacks
    /// -- an editor changes the mode in the alternate screen without touching
    /// the shell's. Written by the negotiation forms and, until the encoder
    /// honours them, read by nothing but the tests: the protocol's support
    /// detection is whether `CSI ? u` is answered, so it stays refused and no
    /// support is claimed while the flags are merely tracked.
    pub(crate) kitty: KittyStack,
    pub(crate) kitty_alt: KittyStack,
    /// Which half of the screen's life this is, from `OSC 133`; see
    /// [`Zone`]. Read by [`Term::resize`] and by nothing else, which is
    /// the whole of what these marks are acted on for here.
    pub(crate) zone: Zone,
    /// The prompt block the shell is at; see `prompt.rs`.
    pub(crate) prompt: crate::prompt::PromptBlock,
}

// **The size is gated because the prose drifted.** A doc comment called `Cold`
// 120 bytes after it had grown to 192 (charset slots, input modes); nothing
// forced the number to be acknowledged. This does, and it grew to 256 when the
// two per-screen kitty stacks landed -- which is free on the write path,
// because `Cold` is carried by reference and the loop never reads them.
// And to 264 for `prompt`, the prompt block's ring row: written once per
// `OSC 133 ; P` and read by `Term::prompt_row`, which is a resize-time and
// embedder-side question the per-byte loop never asks.
// And to 280 for the dynamic colours: two `ColorWord` pairs, written once
// per `OSC 10`/`11` and by the embedder's theme, read by a query and by the
// renderer once a frame. Measured on landing rather than assumed free --
// vim and btop interleaved with an A/A copy, every range overlapping.
// Guarded off `sgr-table`, which adds `sgr: [SgrSlot; NSGR]` and grows it more.
#[cfg(not(feature = "sgr-table"))]
const _: () = assert!(core::mem::size_of::<Cold>() == 280);

impl Cold {
    pub(crate) fn new(rows: Rows, cols: Cols) -> Cold {
        Cold {
            #[cfg(feature = "sgr-table")]
            sgr: [SgrSlot::EMPTY; NSGR],
            g: [Charset::Ascii; 4],
            gl: GSlot::G0,
            zone: Zone::Output,
            prompt: crate::prompt::PromptBlock::NONE,
            mouse: MouseTracking::Off,
            mouse_encoding: MouseEncoding::X10,
            defaults: [ColorWord::DEFAULT; 2],
            dynamic: [ColorWord::DEFAULT; 2],
            highlight: HighlightTracking::Off,
            highlight_region: HighlightRegion::default(),
            terminal: TerminalMode::Ansi,
            vt52_row: Vt52Coord::ZERO,
            str_kind: StrKind::Other,
            str_len: ByteLen::ZERO,
            c1: C1Transmit::SevenBit,
            // The base layer, which is what an application that has not asked
            // for an enhancement must receive.
            other_keys: OtherKeys::Off,
            other_keys_format: OtherKeysFormat::Xterm,
            encoding: InputEncoding::Utf8,
            left: Col::ZERO,
            right: Col::at_index(cols.index() - 1),
            ambiguous: AmbiguousWidth::Narrow,
            cursor_style: CursorStyle::BlinkingBlock,
            keypad: KeypadMode::Numeric,
            protect: CellFlags::NONE,
            screen: Screen::Normal,
            top: Row::ZERO,
            bot: Row::from_index(rows.index() - 1),
            saved: None,
            alt_saved: None,
            kitty: KittyStack::new(),
            kitty_alt: KittyStack::new(),
            // **DECAWM defaults on**, which is xterm's behaviour rather than
            // the VT510 manual's; `docs/vt-coverage.md` records the choice and
            // `baud-core2` makes the same one, so the differential compares
            // like with like.
            modes: Modes::default(),
        }
    }
}

pub struct Term<'b> {
    /// Segment hashes, for the `ceiling-segment-cache` arm only.
    #[cfg(feature = "ceiling-segment-cache")]
    pub(crate) seen: [SegHash; crate::feed::SEGS],
    pub(crate) store: Storage<'b>,
    /// Where the parser stands between reads.
    pub(crate) node: Node,
    /// What `REP` repeats, carried across reads.
    ///
    /// On `Term` rather than in `Cold` because `Cold` cost **16% of
    /// `cat-scroll`** when this lived there: `Paint` writes it once a run and
    /// `feed` copies it back once a read, which is the same shape `cx` and
    /// `style` already have.
    pub(crate) last_cp: Option<(Codepoint, Width)>,

    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    /// The distance between rows in the grid: [`Storage::stride`], fixed for
    /// the life of the storage.
    ///
    /// **Neither the live column count nor `MAX_COLS`.** Striding by the
    /// maximum put consecutive rows 16 KB apart, and on a 128 KB eight-way L1
    /// the set index spans exactly 16 KB -- so every row of a 200-column
    /// screen mapped to the same sets, and any operation touching several
    /// rows thrashed an eight-way set with fifty of them; 78 KB of live cells
    /// were smeared over 800 KB of address space. Striding by the live width
    /// instead was **+16% on an ASCII payload that scrolls and +12% on one
    /// with newlines**, flat on one that never leaves a row -- the split a
    /// set conflict predicts -- and it made the pitch follow the geometry, so
    /// a resize re-packed every row. [`store_stride`] keeps the win by
    /// construction -- an odd number of lines aliases no way, and the
    /// searched step spreads the rows' starts over the sets, which the first
    /// fixed pitch did not and cost 10% for -- and it does not move when the
    /// width does.
    pub(crate) stride: Stride,
    pub(crate) rot: RowRot,

    pub(crate) style: StyleId,
    /// The rendition being *built*. SGR folds into this and interns the
    /// result: a fold reads the previous value, and an index cannot be read
    /// back through a table that evicts on collision.
    ///
    /// **Beside `style` rather than inside `cold`.** `asciiquarium.raw` folds
    /// into it 876,840 times a rep, which is nearly once a character.
    pub(crate) style_raw: StyleWord,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,
    /// The UAX #29 cluster machine, carried between reads.
    ///
    /// **Beside `charset` on the hot path rather than in `Cold`**, because it
    /// is read once per non-ASCII character where `Cold` is read once per
    /// control sequence. It is the whole state: one table id and four
    /// lookback fields, which `CLUSTER_NEXT` collapses to a single indexed
    /// load.
    pub(crate) cluster: ClusterBreak,
    /// Everything a control sequence changes that the per-character loop does
    /// not read. **One struct so `Paint::new` does not grow an argument per
    /// operation**, which is the shape that gets two of them swapped at a call
    /// site -- and so the hot fields above stay where they are.
    pub(crate) cold: Cold,

    pub(crate) utf8: Utf8Acc,
    /// The shortest-form floor for the lead byte's length. Without it every
    /// overlong decodes to a real character -- `E0 80 AF` renders `/`.
    pub(crate) utf8_min: Utf8Min,

    /// The parameters of the `CSI` being parsed, beside the node for the same
    /// reason `utf8` is.
    pub(crate) csi: crate::write::Csi,
    /// Bytes ingested, the denominator of every amplification figure.
    pub(crate) bytes_in: usize,
    /// Cells written, the numerator. Two arms of an experiment that disagree
    /// here are not doing the same work, whatever their timings say.
    pub(crate) cells_written: usize,
    /// Run-length and encoded-length histograms; see `Paint::runs`.
    ///
    /// **The block-shaped counters went with the block walk.** `blocks`,
    /// `block_runs` and `miss_first` counted iterations per sixteen-byte
    /// block, and there are no blocks -- leaving them would have printed
    /// `NaN%` from counters nothing feeds, which is the failure this repo
    /// records as "instrumentation outlived what fed it".
    #[cfg(feature = "diag")]
    pub runs: [usize; 8],
    /// **How often each stride's *vector* scan is entered**, indexed by encoded
    /// length.
    ///
    /// Separate from `run_len`, which counts runs the scan *produced*. This
    /// counts entries into the bulk path, because those are different numbers
    /// and the gap between them is where a missing bulk path hides: a run of
    /// four-byte characters is found either way, by the vector scan or by the
    /// scalar loop under it, and only an entry counter can tell which.
    ///
    /// This repo already states the rule -- "do not read a bulk path as
    /// covering a character class, count how often it is entered" -- and then
    /// had no counter for this one.
    #[cfg(feature = "diag")]
    pub stride_scan: [usize; 5],
    #[cfg(feature = "diag")]
    pub run_len: [usize; 5],
    /// **Iterations of the ingest loop.** The unit this core is designed
    /// against: `CLAUDE.md` records that the floor tracks *bytes an iteration*
    /// and not what the actions do, and nothing here reported it. `bytes_in /
    /// iters` is that quantity, measured rather than reasoned from run
    /// histograms.
    #[cfg(feature = "diag")]
    pub iters: usize,
    /// Characters served by the three-byte permutation table.
    #[cfg(feature = "diag")]
    pub table_hits: usize,
    #[cfg(feature = "diag")]
    pub hist: crate::graph::Hist,
    /// Which finals arrived unimplemented, rather than how many did.
    ///
    /// **Renamed because it collided.** `Term::unsupported` is the shipped
    /// counter -- "control sequences recognised and not acted on" -- and this
    /// is its per-final breakdown, so the two want the same word and only one
    /// can have it. They were both `unsupported` and `diag` had not been built
    /// since; a feature nothing compiles is the same absent-rather-than-red
    /// failure this repo keeps recording, and it was found by reaching for the
    /// instrument rather than by anything reporting it.
    #[cfg(feature = "diag")]
    pub unsupported_by_final: [usize; FinalByte::COUNT],
    #[cfg(feature = "diag")]
    pub scroll_rot: usize,
    #[cfg(feature = "diag")]
    pub scroll_copy: usize,
    /// How many bytes of `store.reply` are pending.
    pub(crate) reply_len: ByteLen,
    /// Replies dropped because the buffer was full. Counted rather than
    /// silently lost: a truncated reply is a wrong answer, and this file's
    /// rule is that a wrong answer is worse than silence.
    pub reply_overflows: usize,
    /// How much of the title and icon buffers is live, and the open link.
    pub(crate) title_len: ByteLen,
    pub(crate) icon_len: ByteLen,
    pub(crate) link: LinkId,
    /// **Refused, and counted.** A control-string body clipped at `OSC_MAX`, a
    /// string recognised and not acted on, and the two halves of `OSC 52`.
    /// Silence is the right answer to a clipboard read -- any reply, empty
    /// included, tells a remote program the feature works and it will keep
    /// asking, and what it would read back is whatever the user last copied.
    /// Being unable to tell that one arrived is a different thing, and is what
    /// these turn from a bare `=> {}` into a number.
    pub vt52_unsupported: usize,
    /// How many `BEL`s arrived.
    ///
    /// A count rather than a callback, because the core cannot make a sound
    /// and an embedder can -- and a **field** rather than an accessor, which
    /// is the convention its neighbours already set: `reply_overflows` and
    /// `string_truncations` are read the same way. The audit is what settled
    /// it, refusing a `-> usize` return where a public field is not a
    /// signature at all.
    pub bells: usize,
    /// Character-set designations this core does not name. **Counted rather
    /// than recorded**: a name means the designation was understood, so
    /// storing an unlisted final as a set would make it look designated.
    pub charset_unsupported: usize,
    /// Control sequences recognised and not acted on. See `Paint`.
    pub unsupported: usize,
    pub unsupported_param: usize,
    pub replacements: usize,
    pub string_truncations: usize,
    pub strings_ignored: usize,
    pub clipboard_reads_refused: usize,
    pub clipboard_writes_dropped: usize,
    /// `OSC 133` marks seen. An embedder can tell whether the shell on the
    /// other end sends them at all, which decides whether a resize can be
    /// scoped to what the application will redraw -- see [`Zone`].
    pub semantic_marks: usize,
    /// How many redraw marks erased the prompt block they replace -- the
    /// stale-width redraw a burst of resizes produces. Exposed so an
    /// embedder can see the terminal acting on the shell's behalf, and so
    /// a test can require it to have happened rather than infer it.
    pub prompt_erases: usize,
    /// Zero-width codepoints received and **not stored**: a continuation with
    /// no base on the line, a cluster past `ClusterSlot::CAPACITY`, or a
    /// [`ClusterStore`] with no free slot for it.
    ///
    /// **Counted, because the glyph differential structurally cannot see the
    /// difference**: it compares cell content and a continuation writes no
    /// cell, so a core that stores them and one that discards them are the
    /// same to that gate. Same shape as `clipboard_reads_refused` -- baud
    /// refuses, and says how often.
    ///
    /// It is no longer "every mark": most now land in [`ClusterStore`]. What
    /// remains is that store's bound, stated as a number rather than implied.
    pub marks_dropped: usize,
    /// Mouse reports refused because the position had no encoding.
    ///
    /// X10 caps each ordinate at 223 against a 2048-column grid, so a report
    /// from column 400 has no byte -- and the one it would produce names a
    /// *different cell* rather than a truncated one, which is the wrong-shaped
    /// answer `docs/vt-coverage.md` rules out. Refused and counted, like
    /// `clipboard_reads_refused`.
    pub mouse_unencodable: usize,
    pub fast_try: usize,
    #[cfg(feature = "diag")]
    pub fast_hit: usize,
    #[cfg(all(feature = "diag", feature = "sgr-table"))]
    pub sgr_hit: usize,
}

/// Outcome of a [`Term::resize`].
///
/// `#[must_use]` because the two answers are different facts about the
/// terminal, and a caller that drops the value renders a grid that disagrees
/// with the application about its own size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub enum Resized {
    Ok,
    /// The window is larger than this core can hold. `cols`/`rows` are what it
    /// was clamped to.
    ///
    /// **Two different ceilings, and the caller can tell them apart from the
    /// numbers.** `MAX_COLS`/`MAX_ROWS` are the core's, and are fixed; the row
    /// capacity is the *caller's*, because the grid is caller-provided storage
    /// -- an embedder that wants to grow provisions
    /// [`Storage::row_capacity`] rows up front.
    Truncated {
        cols: Cols,
        rows: Rows,
    },
}

impl<'b> Term<'b> {
    /// Geometry comes from a [`WinSize`], which is the one place `ws_col` and
    /// `ws_row` are destructured -- so they cannot be transposed here.
    /// **Fill the three-byte cell table**: every `(b0,b1,b2)` a character can
    /// be, and the cell it becomes. 65,536 entries, once, at construction.
    ///
    /// This is the compile step of a compiled database that happens to run at
    /// startup, because 2 MiB is more than a `const fn` should evaluate and
    /// nothing in it depends on the terminal. `the_cell_table_agrees_with_the_
    /// decoder` walks all 65,536 against the path it replaces.
    ///
    /// **An empty slice is how a caller declines it, and that is the whole
    /// switch.** The only read is in `put_run` behind `cell-table`, so a
    /// default build that passes 2 MiB here spends 262,144 property-table
    /// lookups and 2 MiB of stores at construction on a table nothing will
    /// consult. `ingest2` and `multibench` were doing exactly that. Gating
    /// *this function* on the feature instead is wrong -- the table's own
    /// equivalence test builds it without the feature and reads the slice
    /// directly, which is how that mistake was caught.
    fn fill_cells3(store: &mut Storage<'b>) {
        if store.cells3.is_empty() {
            return;
        }
        for b0 in 0xE0usize..=0xEF {
            for b1 in 0x80usize..=0xBF {
                let row = ((b0 - 0xE0) * 64 + (b1 - 0x80)) * 256;
                // Table 3-7: `E0` needs `A0..BF` and `ED` needs `80..9F`, or the
                // sequence is overlong or a surrogate.
                let ok = !(b0 == 0xE0 && b1 < 0xA0 || b0 == 0xED && b1 > 0x9F);
                for b2 in 0x80usize..=0xBF {
                    let raw = Codepoint::from_three_bytes(b0, b1, b2);
                    let cp = match ok {
                        true => Codepoint::decoded(raw),
                        false => Codepoint::REPLACEMENT,
                    };
                    // **The entry carries everything the write needs**: the
                    // cell, how wide it is, and how far the cursor moves. Both are
                    // bounded functions of the three bytes, so both are settled
                    // here rather than at ingest.
                    let w = Classified::of(cp).width(AmbiguousWidth::Narrow);
                    store.cells3[row + b2] =
                        PackedCell::glyph(cp, StyleId::DEFAULT, CellFlags::of_entry(w));
                }
            }
        }
    }

    /// Where row `y` stops being uniform.
    ///
    /// **Not "where the glyphs are", which is the reading that produced a
    /// bug.** `ECH` at the last column writes a *styled* blank: it holds no
    /// glyph and it does not match the default blanks in front of it, so the
    /// row is non-uniform past its last character. Blanking stops here, and a
    /// mark set past it raises it -- both directions are needed, and the
    /// property suite caught the version that only lowered.
    ///
    /// Takes `&self`: the rotation is resolved here rather than undone, so
    /// it needs no mutation to answer.
    pub fn row_extent(&self, y: Row) -> RowExtent {
        // The rotation resolved here rather than through a `View`, which would
        // need the cells this does not read. Both operands are below the
        // ring, so one conditional subtract does what `%` would -- the same
        // shape `View::phys` uses, and worth ~4% there.
        let (_, used, ring, rot) = self.surface();
        let ring = ring.index();
        let p = y.index() + rot.index();
        used[if p >= ring { p - ring } else { p }]
    }

    /// The live surface: its cells, its marks, its ring and its rotation, by
    /// which screen is up. The primary is the whole ring, scrollback and all;
    /// the alternate is a ring of the screen's capacity.
    fn surface(&self) -> (&[PackedCell], &[RowExtent], Rows, RowRot) {
        match self.cold.screen {
            Screen::Normal => (
                self.store.vp,
                self.store.used,
                self.store.ring_rows(),
                self.rot,
            ),
            Screen::Alternate => (
                self.store.alt.cells,
                self.store.alt.used,
                self.store.row_capacity(),
                self.store.alt.rot,
            ),
        }
    }

    /// The two rotations back from a read: the view's is the live screen's,
    /// the other surface's is the other screen's, whichever way `?1049` left
    /// them.
    pub(crate) fn set_rots(&mut self, (live, other): (RowRot, RowRot)) {
        match self.cold.screen {
            Screen::Normal => {
                self.rot = live;
                self.store.alt.rot = other;
            }
            Screen::Alternate => {
                self.store.alt.rot = live;
                self.rot = other;
            }
        }
    }

    /// How many rows the caller provisioned. See [`Storage::row_capacity`].
    ///
    /// Exposed on `Term` because that is where an embedder asks: it holds the
    /// `Storage` and cannot reach past it, and this is the number that decides
    /// whether a [`Term::resize`] can grow.
    pub fn row_capacity(&self) -> Rows {
        self.store.row_capacity()
    }

    /// Take a new window size.
    ///
    /// **Geometry only: the allocation is the caller's and is untouched.** The
    /// grid is caller-provided, so this can shrink freely and can only grow
    /// into rows somebody provisioned -- [`Storage::row_capacity`] is that
    /// number, and asking for more returns `Truncated` rather than clamping in
    /// silence. Columns are free to the pitch, which is `MAX_COLS` or more by
    /// construction.
    ///
    /// **The content survives, hidden; nothing reflows and nothing is
    /// blanked.** The pitch is fixed, so a cell keeps its address whatever
    /// the geometry: a narrower or shorter screen stops addressing what lies
    /// past its edge, and a wider or taller one addresses it again -- the
    /// window is a view onto a grid that is as large as it was ever made.
    /// What keeps that sound is that a row's mark bounds its content over the
    /// whole pitch, and a clear runs through the mark, so a line the
    /// application erased while the screen was narrow does not come back
    /// with a tail. Reflow -- deciding where a wrapped line breaks now -- is a
    /// renderer's problem and has no right answer at this layer; what this
    /// layer owes is that a resize costs what it touches, which is nothing
    /// but the rotation. [`View::widened`] is the whole of the width's half,
    /// run over both surfaces, because both are laid out at the one pitch;
    /// the height's half is the rotation moving over the ring, below.
    ///
    /// Three things happen besides, in an order that is not free:
    ///
    /// * **A change of height moves the window, not the rows.** The
    ///   rotation is an index into the ring, whose depth a resize never
    ///   changes, so nothing is un-rotated and nothing can outrun a slice --
    ///   the `mid <= len` panic a shrink under a live rotation once produced
    ///   has no operation left to live in.
    /// * **DECSTBM and the side margins are reset.** xterm does, a stale
    ///   bottom margin points off the grid, and a stale *side* margin is an
    ///   addressing error rather than a region: `cold.right` is what `room()`
    ///   and every wrap test read, so a resize from 200 columns to 8 once left
    ///   the write path believing it had 192 more than the grid has. Found by
    ///   `autowrap_off_leaves_the_cursor_on_the_last_column`, after the
    ///   margins work reset them on `RIS` and `DECSTR` and missed the third
    ///   place that changes the geometry.
    /// * **The cursor is clamped, not homed.** There is content to be
    ///   relative to now, and a pending wrap at the old right edge is
    ///   forgotten with the edge.
    ///
    /// **One thing core2 does here and this does not, deliberately.** It
    /// rebuilds the tab table, which is `MAX_COLS` long and stamped every
    /// eighth column at construction, so no width change can invalidate it.
    pub fn resize(&mut self, ws: &WinSize) -> Resized {
        let (want_cols, want_rows) = ws.geometry();
        let cap = self.store.row_capacity();
        let cols = want_cols.clamp(MAX_COLS).clamp(self.stride.index());
        let rows = want_rows.clamp(MAX_ROWS).clamp(cap.index());
        let over = cols.index() != want_cols.index() || rows.index() != want_rows.index();
        let (old_rows, old_cols) = (self.rows, self.cols);

        // **The primary's height changes the way xterm's does, and it is
        // rotation arithmetic over the ring.** A shorter screen keeps the
        // cursor's rows: the rows below the cursor are cut from the bottom
        // and stay where they are past the live height, and only what the
        // cursor needs is retired -- the window advances over the top rows,
        // which become lines behind it without moving. A taller screen shows
        // the cut rows first, then retreats the window over the retired
        // ones, newest nearest the old top, and blanks whatever rows are
        // left: they hold lines already behind the window, which would show
        // twice. Applied to the primary whichever screen is up, around the
        // cursor the primary has -- its own, or the one it was saved with --
        // so an exit from the alternate screen has nothing to catch up on.
        if rows.index() != old_rows.index() {
            let ring = self.store.ring_rows();
            let spare = ring.index() - rows.index();
            let (from, to) = (old_rows.index(), rows.index());
            let live = self.cold.screen == Screen::Normal;
            let mut cy = match live {
                true => self.cy.index(),
                false => self.cold.alt_saved.map_or(0, |s| s.cy.index()),
            }
            .min(from - 1);
            let (mut hist, mut hidden) =
                (self.store.book.hist.index(), self.store.book.hidden.index());
            if to < from {
                let k = from - to;
                // **The anchor is the bottom, and rows come off the top.**
                //
                // Taking them off the bottom instead kept the first line
                // still -- which is what it was for -- and moved the
                // cursor to a row that was not the one it was on. The
                // shell's next redraw then landed *there*, over output,
                // and no later growth can restore a cell that was
                // overwritten rather than hidden: the owner's `ls`
                // deleted, ten of twelve lines, and
                // `a_shrink_does_not_let_the_prompt_overwrite_output`
                // reproduces it.
                //
                // A shrink means the content no longer fits, and content
                // that no longer fits pushes into the history -- which
                // is the owner's own refinement of the invariant. So the
                // cursor keeps its row, the prompt stays the last line,
                // and the growth below brings the rows back.
                let cut = k.min(from - 1 - cy);
                let retire = k - cut;
                self.rot = self.rot.advance(retire, ring);
                hist = (hist + retire).min(spare);
                hidden = (hidden + cut).min(spare - hist);
                cy -= retire;
                cy = cy.min(to - 1);
            } else {
                let k = to - from;
                // **Content first, blanks last.** xterm reveals the rows a
                // shrink cut below the cursor before it pulls anything
                // back from above, whatever those rows hold; here a cut
                // row that is blank is not content, and a growth that
                // showed blank rows under the prompt while the line the
                // shrink pushed into the history stayed there changed the
                // first line for no reason -- the owner's rule is the
                // other way: pull down to the bottom, and blank-fill only
                // when nothing is left to pull. So the cut rows come back
                // as far as the last one that holds anything, the history
                // is pulled next, and the blank cut rows last -- which are
                // what a fresh blank would be anyway.
                let ring_i = ring.index();
                let used = &self.store.used;
                let content = (0..hidden)
                    .rev()
                    .find(|i| used[(self.rot.index() + from + i) % ring_i].index() != 0)
                    .map_or(0, |i| i + 1);
                let reveal = k.min(content);
                hidden -= reveal;
                // **And a growth brings them back**, which is the other
                // half of a bottom anchor: the window sits at the end of
                // the content, so a taller one shows more of it above.
                //
                // Refusing to here was tried, to stop a drag through a
                // very short window revealing the prompts the shell drew
                // and scrolled away at that size. It cost the `ls`: rows
                // a shrink retired are the reader's content far more
                // often than they are a stray prompt, and leaving them
                // behind is a loss where showing them is at worst a
                // duplicate. The duplicate is pinned in
                // `resize_recorded.rs` as the open bug it is.
                let pull = (k - reveal).min(hist);
                self.rot = self.rot.retreat(pull, ring);
                hist -= pull;
                cy += pull;
                // The blank cut rows, after the history: revealed rather
                // than counted as hidden, and blanked below all the same.
                let rest = (k - reveal - pull).min(hidden);
                hidden -= rest;
                let mut v = View::new(
                    self.store.vp,
                    self.store.used,
                    self.stride,
                    Cols::of_index(old_cols.index().max(cols.index())),
                    rows,
                    ring,
                    self.rot,
                );
                let mut n = 0;
                for y in from + reveal + pull..to {
                    n += v.fill_row(Row::from_index(y), PackedCell::EMPTY).index();
                }
                self.cells_written += n;
                hist = hist.min(spare);
                hidden = hidden.min(spare - hist);
            }
            self.store.book.hist = Rows::of_count(hist);
            self.store.book.hidden = Rows::of_count(hidden);
            match live {
                true => self.cy = Row::from_index(cy),
                false => {
                    if let Some(s) = self.cold.alt_saved.as_mut() {
                        s.cy = Row::from_index(cy.min(to - 1));
                    }
                }
            }
        }

        // **A change of width reflows the primary**: its lines are re-cut
        // to the new width in place, through the history, and the window
        // and the cursor follow their cells (`View::reflow`). The
        // alternate screen is cell-addressed rather than reflowed -- a
        // full-screen program is told and redraws -- and a wider one blanks
        // the columns it reveals (`View::widened`): the program owns the
        // screen and repaints it a frame later, so showing the frame it drew
        // when it was last this wide, under the incoming one, is two images
        // at once. Blanking also raises the mark, so a fill taken while
        // narrow is not trusted past the edge it stopped at.
        // **The shell's half of the screen is not re-cut.** See [`Zone`]:
        // between a prompt mark and the output that follows it, the rows
        // on screen are ones the shell will redraw, and it redraws them by
        // counting how many they are. Re-cutting them to a new width
        // changes that count behind its back and leaves the top of the old
        // prompt behind -- once per resize. Output is re-cut as it always
        // was, because nothing will redraw it and a long line is worth
        // re-wrapping.
        //
        // A stream that sends no marks is `Zone::Output` and reaches this
        // exactly as before.
        if cols.index() != old_cols.index() && self.cold.zone == Zone::Output {
            let live = self.cold.screen == Screen::Normal;
            let cursor = match live {
                true => (self.cy, self.cx),
                false => self
                    .cold
                    .alt_saved
                    .map_or((Row::ZERO, Col::ZERO), |s| (s.cy, s.cx)),
            };
            let r = View::new(
                self.store.vp,
                self.store.used,
                self.stride,
                Cols::of_index(old_cols.index().max(cols.index())),
                rows,
                self.store.ring_rows(),
                self.rot,
            )
            .reflow(
                old_cols,
                cols,
                self.store.book.hist,
                self.store.book.hidden,
                cursor,
            );
            self.rot = r.rot;
            self.store.book.hist = r.hist;
            self.store.book.hidden = r.hidden;
            match live {
                true => (self.cy, self.cx) = r.cursor,
                false => {
                    if let Some(s) = self.cold.alt_saved.as_mut() {
                        (s.cy, s.cx) = r.cursor;
                    }
                }
            }
            if cols.index() > old_cols.index() {
                View::new(
                    self.store.alt.cells,
                    self.store.alt.used,
                    self.stride,
                    cols,
                    rows,
                    self.store.row_capacity(),
                    self.store.alt.rot,
                )
                .widened(old_cols);
            }
        }

        self.rows = rows;
        self.cols = cols;
        self.cy = Row::from_index(self.cy.index().min(rows.index() - 1));
        self.cx = Col::at_index(self.cx.index().min(cols.index() - 1));
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(rows.index().saturating_sub(1));
        self.cold.left = Col::ZERO;
        self.cold.right = Col::at_index(cols.index().saturating_sub(1));
        // **The in-band report, after the resize is complete.** The
        // specification is explicit that a terminal must not send one
        // before its own resize has finished, which is why this is the
        // last thing here: everything above has settled the geometry, the
        // window and the cursor.
        //
        // It reports what the terminal *has*, not what was asked for --
        // the clamps above are real -- for the same reason an embedder
        // must send the child the settled size rather than the requested
        // one.
        if self.cold.modes.is_set(ModeBit::InbandResize) {
            self.queue(Report::InbandResize { rows, cols });
        }
        match over {
            true => Resized::Truncated { cols, rows },
            false => Resized::Ok,
        }
    }

    pub fn new(store: Storage<'b>, ws: &WinSize) -> Term<'b> {
        let mut store = store;
        Term::fill_cells3(&mut store);
        // **Every eighth column, and not column zero.** A tab stop at 0 makes
        // `CBT` from column 3 land where the cursor already was.
        for (i, stop) in store.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
        let (cols, rows) = ws.geometry();
        // The pitch admits `MAX_COLS` by construction; a bench arm that lays
        // the grid out narrower bounds the width with it.
        let stride = store.stride;
        let (cols, rows) = (
            cols.clamp(MAX_COLS).clamp(stride.index()),
            rows.clamp(MAX_ROWS),
        );
        Term {
            #[cfg(feature = "ceiling-segment-cache")]
            seen: [SegHash::ZERO; crate::feed::SEGS],
            store,
            node: Node::Ground,
            last_cp: None,
            cy: Row::ZERO,
            cx: Col::ZERO,
            rows,
            cols,
            stride,
            rot: RowRot::NONE,
            style: StyleId::DEFAULT,
            style_raw: StyleWord::DEFAULT,
            flags: CellFlags::NONE,
            charset: Charset::Ascii,
            cluster: ClusterBreak::new(),
            cold: Cold::new(rows, cols),
            utf8: Utf8Acc::lead(0),
            utf8_min: Utf8Min::Two,
            csi: crate::write::Csi::NEW,
            bytes_in: 0,
            cells_written: 0,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            #[cfg(feature = "diag")]
            stride_scan: [0; 5],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
            #[cfg(feature = "diag")]
            iters: 0,
            #[cfg(feature = "diag")]
            table_hits: 0,
            #[cfg(feature = "diag")]
            hist: crate::graph::Hist::ZERO,
            #[cfg(feature = "diag")]
            unsupported_by_final: [0; FinalByte::COUNT],
            #[cfg(feature = "diag")]
            scroll_rot: 0,
            #[cfg(feature = "diag")]
            scroll_copy: 0,
            reply_len: ByteLen::ZERO,
            reply_overflows: 0,
            title_len: ByteLen::ZERO,
            icon_len: ByteLen::ZERO,
            link: LinkId::NONE,
            vt52_unsupported: 0,
            bells: 0,
            charset_unsupported: 0,
            unsupported: 0,
            unsupported_param: 0,
            replacements: 0,
            string_truncations: 0,
            strings_ignored: 0,
            clipboard_reads_refused: 0,
            semantic_marks: 0,
            prompt_erases: 0,
            clipboard_writes_dropped: 0,
            marks_dropped: 0,
            mouse_unencodable: 0,
            fast_try: 0,
            #[cfg(feature = "diag")]
            fast_hit: 0,
            #[cfg(all(feature = "diag", feature = "sgr-table"))]
            sgr_hit: 0,
        }
    }

    /// One row of the grid, resolved through the rotation.
    ///
    /// The screen, read where it is: a [`Viewport`] over the live surface's
    /// ring, the same pointers the core writes through, with a row a slice
    /// of the ring and an index `y * stride + x` resolving to the cell in
    /// place. No copy and no turn, whichever screen is up. A reader that
    /// wants one slice of the whole screen asks [`Viewport::contiguous`],
    /// which the window is for all but `rows` of every `ring` positions,
    /// and takes [`Viewport::parts`] otherwise.
    pub fn viewport(&self) -> Viewport<'_> {
        let (cells, used, ring, rot) = self.surface();
        Viewport::new(cells, used, self.stride, self.cols, self.rows, ring, rot)
    }

    /// The screen scrolled `back` lines into the history: the window that
    /// starts `back` lines above the live screen, bounded by what is
    /// retained, and the live screen itself on the alternate screen, which
    /// keeps none. What a renderer draws while the user is looking back --
    /// the same read as [`Term::viewport`], at a rotation `back` further
    /// round the ring, so it costs a frame nothing more.
    pub fn view(&self, back: Rows) -> Viewport<'_> {
        let n = match self.cold.screen {
            Screen::Normal => back.index().min(self.store.book.hist.index()),
            Screen::Alternate => 0,
        };
        self.viewport().back(n)
    }

    /// One screen row, `cols` cells: [`Viewport::row`].
    pub fn row(&self, y: Row) -> &[PackedCell] {
        self.viewport().row(y)
    }

    /// One row, or `None` if `y` is not on the grid.
    ///
    /// The checked form, for a caller holding a row number from somewhere
    /// other than its own loop bound.
    pub fn get_row(&self, y: Row) -> Option<&[PackedCell]> {
        (y.index() < self.rows.index()).then(|| self.row(y))
    }

    /// Which surface is live. `baud-core` parks the primary rather than
    /// keeping two buffers, so this says which one the single grid holds.
    pub fn screen(&self) -> Screen {
        self.cold.screen
    }

    /// `?2026`. Whether the application asked for frames to be held.
    ///
    /// Recorded and exposed rather than acted on: holding a frame is a
    /// renderer's job and this core has no renderer, so telling the embedder
    /// is the whole of the terminal's part.
    pub fn synchronized_output(&self) -> bool {
        self.cold.modes.is_set(ModeBit::SynchronizedOutput)
    }

    /// Which half of the screen's life this is, from `OSC 133`.
    ///
    /// [`Zone::Output`] until a mark says otherwise, so a stream with no
    /// shell integration reads as output -- which is what it is. Read by
    /// `Term::resize` to decide whether a width change may re-cut, and
    /// exposed so an embedder can see which rule is in force rather than
    /// having to infer it from what the screen did.
    pub fn zone(&self) -> Zone {
        self.cold.zone
    }

    /// The screen row the current prompt block begins on, while the shell
    /// is at a prompt; `None` in [`Zone::Output`] or if the block's first
    /// row has scrolled off the screen.
    ///
    /// **This is the row a resize must keep at a fixed distance below the
    /// cursor.** zsh redraws its prompt by counting rows from where it
    /// started to where the cursor is, moving up that many and erasing
    /// from there (Schaefer, zsh-workers); a terminal that changes that
    /// distance behind its back orphans the prompt's top. The block start
    /// is kept as a ring row on `Cold`, so this answers under the live
    /// rotation rather than from a number that went stale at the first
    /// scroll.
    pub fn prompt_row(&self) -> Option<Row> {
        if self.cold.zone != Zone::Prompt {
            return None;
        }
        let y = self
            .cold
            .prompt
            .start()
            .retreat(self.rotation().index(), self.ring())
            .index();
        (y < self.rows.index()).then(|| Row::from_index(y))
    }

    /// `?2004`. Whether pasted text should be bracketed.
    pub fn bracketed_paste(&self) -> bool {
        self.cold.modes.is_set(ModeBit::BracketedPaste)
    }

    /// The lines that scrolled off the top of the primary screen, newest
    /// first, each to its mark: what a scrolled-back window shows above the
    /// screen. They are rows of the ring behind the window, so the read is a
    /// view and the scroll that made them wrote nothing extra; empty on a
    /// storage built with [`Depth::NONE`], whose ring is the screen.
    pub fn scrollback(&self) -> Scrollback<'_> {
        Scrollback::new(
            self.store.vp,
            self.store.used,
            self.stride.index(),
            self.store.ring_rows(),
            self.rot,
            self.store.book.hist,
        )
    }

    /// The live geometry, which a resize can change.
    pub fn geometry(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }

    /// How incoming bytes are decoded. `ESC % @` / `ESC % G` select it.
    ///
    /// **This core never changes it**, which is the gap
    /// `the_known_encoding_gaps` pins: the selectors are not routed, so the
    /// answer is always UTF-8. The accessor is real and the switch is not.
    pub fn input_encoding(&self) -> InputEncoding {
        self.cold.encoding
    }

    /// How wide a `CSI` this core writes, as `S7C1T` / `S8C1T` last set it.
    pub fn c1_transmit(&self) -> C1Transmit {
        self.cold.c1
    }

    /// What a G-slot holds, as distinct from which slot is invoked.
    ///
    /// `ESC ) 0` loads G1 and `SO` selects it; [`Term::charset`] answers the
    /// second question and this one answers the first, which is what a
    /// conformance check on designation has to ask.
    pub fn charset_in(&self, slot: GSlot) -> Charset {
        self.cold.g[slot.index()]
    }

    /// The character set in force -- the resolved one, `g[gl]`.
    ///
    /// Designation and invocation are different things: `ESC ) 0` loads G1 and
    /// `SO` selects it. This is what the write path reads, which is neither
    /// slot nor shift but their resolution.
    pub fn charset(&self) -> Charset {
        self.charset
    }

    /// Measure East Asian Ambiguous characters as one column or two.
    ///
    /// Takes effect on the **next** character: cells already written keep the
    /// width they were measured at, because the grid stores cells rather than
    /// codepoints and re-measuring would mean re-laying-out the screen.
    pub fn set_ambiguous_width(&mut self, a: AmbiguousWidth) {
        self.cold.ambiguous = a;
    }

    /// What [`Term::set_ambiguous_width`] last selected.
    pub fn ambiguous_width(&self) -> AmbiguousWidth {
        self.cold.ambiguous
    }

    /// `DECSCUSR`. The cursor shape an application asked for.
    pub fn cursor_style(&self) -> CursorStyle {
        self.cold.cursor_style
    }

    /// The `[[source: kitty-keyboard]]` enhancement flags in force on the
    /// active screen.
    ///
    /// **Tracked, not yet honoured.** The negotiation forms maintain this, but
    /// the encoder does not read it and `CSI ? u` is refused, so baud claims no
    /// support: the protocol detects support by whether the query is answered,
    /// and answering before every enhancement is honoured would be a false
    /// claim. Exposed so the negotiation state machine can be tested before the
    /// encoder lands.
    pub fn kitty_flags(&self) -> KittyFlags {
        self.cold.kitty.current()
    }

    /// `?25` DECTCEM. Whether the cursor should be drawn.
    pub fn cursor_visible(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorVisible)
    }

    /// `?4` DECSCLM. Smooth scroll -- renderer policy, recorded and exposed.
    pub fn smooth_scroll(&self) -> bool {
        self.cold.modes.is_set(ModeBit::SmoothScroll)
    }

    /// `?8` DECARM. Auto-repeat -- keyboard policy, recorded and exposed.
    pub fn auto_repeat(&self) -> bool {
        self.cold.modes.is_set(ModeBit::AutoRepeat)
    }

    /// `?3` DECCOLM. Whether the application asked for 132 columns.
    ///
    /// **The width is not changed here.** This core's geometry is the
    /// embedder's -- it comes from a `WinSize` and moves through
    /// [`Term::resize`] -- so DECCOLM records the request and performs the
    /// side effects the specification attaches to it: the region resets, the
    /// cursor homes and the screen clears.
    pub fn column_mode(&self) -> bool {
        self.cold.modes.is_set(ModeBit::Column132)
    }

    /// `?5` DECSCNM, reverse video. Recorded and exposed, not drawn: which
    /// pixels invert is the renderer's.
    pub fn reverse_video(&self) -> bool {
        self.cold.modes.is_set(ModeBit::ReverseVideo)
    }

    /// Bytes ingested and cells written, for the amplification ratio rule #1
    /// is about.
    pub fn amplification(&self) -> (usize, usize) {
        (self.bytes_in, self.cells_written)
    }

    /// The distance between rows in [`Term::cells`].
    ///
    /// **A renderer indexes by this, not by `MAX_COLS` and not by the
    /// width.** The pitch is fixed for the life of the storage at
    /// [`store_stride`]'s choice, which is what keeps consecutive rows off
    /// the same L1 sets, and it survives a resize.
    pub fn stride(&self) -> Stride {
        self.stride
    }

    /// The rendition a cell's [`StyleId`] stands for.
    ///
    /// **Read the slot back rather than trusting the id.** The table evicts on
    /// collision, so an id can outlive the rendition it was minted for; a
    /// renderer that trusted it would paint a stale colour.
    pub fn style_of(&self, cell: PackedCell) -> StyleWord {
        self.store.tmpl[cell.style.index()].unwrap_or(StyleWord::DEFAULT)
    }

    /// The whole rendition table, indexed by [`StyleId`]: what a renderer
    /// that resolves every slot once a frame reads, rather than asking
    /// [`Term::style_of`] once a cell. `None` is a slot nothing was minted
    /// in, and reads as [`StyleWord::DEFAULT`], which is what `style_of`
    /// answers for it.
    pub fn styles(&self) -> &[Option<StyleWord>] {
        self.store.tmpl
    }

    /// The live surface's ring as the core holds it: [`Term::ring`] rows of
    /// [`Term::stride`] cells, under the rotation -- screen row `y` is
    /// physical row `(y + rotation) % ring`, [`Term::rotation`]. What a
    /// reader that can apply the rotation itself -- a shader, one multiply
    /// and one modulo a cell -- takes, so the grid is read in place and
    /// nothing is turned or copied for a frame. On the primary screen the
    /// rows behind the window are the scrollback, so the same reader can
    /// show a window scrolled back by offsetting the rotation.
    pub fn cells(&self) -> &[PackedCell] {
        let (cells, _, ring, _) = self.surface();
        RowBlock::leading(ring, self.stride).of(cells)
    }

    /// How the window moved since a rotation a caller noted: rows retired
    /// off the top, or rows restored -- a resize pulling history back, or
    /// a redraw mark moving the window over its block. A rotation is
    /// modular, so the short way round is the one that happened; an
    /// embedder anchoring a scrolled-back window needs this and must not
    /// guess it from two rotations.
    pub fn moved_since(&self, before: RowRot) -> Moved {
        let ring = self.ring().index();
        let fwd = (self.rotation().index() + ring - before.index()) % ring;
        match fwd * 2 <= ring {
            true => Moved::Retired(Rows::of_count(fwd)),
            false => Moved::Restored(Rows::of_count(ring - fwd)),
        }
    }

    /// The rotation [`Term::cells`] is under.
    pub fn rotation(&self) -> RowRot {
        self.surface().3
    }

    /// Rows a shorter screen cut from below the cursor and a taller one
    /// reveals first, before it pulls anything back from the history. Zero
    /// unless a shrink left some; a redraw of a prompt block taller than
    /// the screen adds the blank rows the window gave up moving back over
    /// the block. Exposed so a test can hold the bookkeeping to its
    /// contract rather than infer it from what a growth shows.
    pub fn hidden(&self) -> Rows {
        self.store.book.hidden
    }

    /// The modulus of that rotation: physical rows in the live surface's
    /// ring. The screen's capacity plus the scrollback on the primary, the
    /// capacity alone on the alternate.
    pub fn ring(&self) -> Rows {
        self.surface().2
    }

    /// Place bytes in the arena at `base`, the way a `read(2)` would.
    ///
    /// The core never copies on the parse path: a read writes here and the
    /// parser reads here. This is the seam, and it exists so a caller can hand
    /// the kernel `&mut arena[base..]` directly.
    pub fn write_at(&mut self, base: BytePos, src: &[u8]) {
        let from = base.index();
        self.store.arena[from..from + src.len()].copy_from_slice(src);
    }

    /// Where a renderer should draw the cursor.
    ///
    /// **Clamped to the last column, and that is the contract rather than a
    /// safety net.** After a character lands in the last column the cursor is
    /// in *pending wrap*: `cx` is `cols`, one past the margin, so the next
    /// graphic character wraps to the next row instead of overwriting what is
    /// there. That is the right internal state and it is not a position --
    /// there is no column `cols` to draw in, and a renderer indexing a row
    /// with it reads past the end of the row.
    ///
    /// So the accessor answers the question a renderer is asking. `baud-core2`
    /// takes `min(cols - 1)` here for the same reason, and
    /// The cursor differential against `baud-core2` is what found that this
    /// core did not: on a 200-column screen it returned **column 200** after
    /// `HT` ran to the right margin. Both cores' *internal* `cx` was 200 and
    /// they rendered identically, so neither pinned gate could see it -- the
    /// glyph gate reads content and the rendition gate reads the style word,
    /// and the cursor is in neither.
    pub fn cursor(&self) -> (Row, Col) {
        // **The sentinel is one past the last *writable* column**, which is
        // the right margin rather than the screen edge once `DECSLRM` has
        // moved it. Clamping to `cols - 1` reported column 10 for a cursor
        // pinned at a margin of 9 -- a position no write can reach, which an
        // application counting columns then disagrees with.
        let last = self.cold.right;
        (self.cy, Col::at_index(self.cx.index().min(last.index())))
    }
}

impl<'b> Term<'b> {
    /// Bytes the terminal owes the host, e.g. the answer to a `DA` or `DSR`.
    ///
    /// Write these back to wherever the input came from, then call
    /// [`Term::clear_reply`]. Empty when there is nothing to say.
    pub fn reply(&self) -> &[u8] {
        &self.store.reply[..self.reply_len.index()]
    }

    /// Drop the pending reply after sending it.
    pub fn clear_reply(&mut self) {
        self.reply_len = ByteLen::ZERO;
    }

    /// The codepoints attached to the cell at `(r, c)`, after its base.
    ///
    /// **Resolved through the grid, not from a position key.** The cell holds
    /// the cluster index, so this is one load and a bounds check, and it is
    /// correct after a scroll and on either screen -- the
    /// index moved when the cell moved. `baud-core2` answers the same question
    /// from a table keyed on surface, physical row, column and generation, and
    /// an adversarial review reproduced what that costs: one un-rotating read
    /// and the answer is empty, then a stranger's.
    ///
    /// Empty when the cell has no continuations, and empty rather than wrong
    /// when the store refused the cluster -- the count is
    /// [`Term::marks_dropped`].
    pub fn marks(&self, r: Row, c: Col) -> impl Iterator<Item = u32> + '_ {
        let (cells, _, ring, rot) = self.surface();
        let v = View::new_ref(cells, self.stride, self.cols, self.rows, ring, rot);
        // **Either half of a wide character answers, and the lead is what
        // holds them.** A renderer draws the glyph at the lead and a selection
        // hands back whichever column the pointer landed on -- so a caller
        // that asks the tail is asking about the same character, and getting
        // nothing back is a wrong answer rather than a missing feature.
        //
        // `ViewRef::lead` is the one constructor of `LeadCol`, which exists
        // because "one column back" and "the cell that owns this position" are
        // different questions that used to be the same type. `attach` already
        // goes through it; this is the read side of the same rule.
        let cell = v.cell_at(r, v.lead(r, c));
        self.store
            .clusters
            .get(cell.flags.cluster(), cell.content)
            .into_iter()
            .flat_map(|s| s.marks())
    }

    /// What tracking the application asked for: `?1000`, `?1002`, `?1003`.
    /// The attached codepoints of `cell` itself, wherever on the ring it
    /// was read: the store is keyed by the cell's cluster id and content,
    /// not by a screen position, so a reader walking a view scrolled back
    /// gets that cell's marks. `marks` resolves a position on the live
    /// screen and answers the wrong cell for any other view.
    pub fn cluster_marks(&self, cell: PackedCell) -> impl Iterator<Item = u32> + '_ {
        self.store
            .clusters
            .get(cell.flags.cluster(), cell.content)
            .into_iter()
            .flat_map(|s| s.marks())
    }

    pub fn mouse_tracking(&self) -> MouseTracking {
        self.cold.mouse
    }

    /// Whether `?1001` highlight tracking is on.
    ///
    /// An embedder with a selection model can implement it; this core records
    /// the request and does nothing else. `[[source: ctlseqs]]`'s warning that
    /// it "requires a cooperating program, else xterm will hang" describes an
    /// event loop blocking on a reply, and this core has none -- so whether to
    /// wait is a decision it does not make.
    pub fn highlight_tracking(&self) -> HighlightTracking {
        self.cold.highlight
    }

    /// The region from the last `CSI Ps ; Ps ; Ps ; Ps ; Ps T`.
    pub fn highlight_region(&self) -> HighlightRegion {
        self.cold.highlight_region
    }

    /// Queue a mouse report, if the application asked for this kind of event.
    ///
    /// Returns whether anything was queued, so an embedder can tell "the
    /// application does not want this" from "it was sent".
    ///
    /// **Two gates, and both are refusals rather than omissions.** The
    /// tracking level decides which *kinds* are reported: motion is dropped
    /// under `?1000`, and motion with no button held under anything below
    /// `?1003`, because sending it anyway floods an application that asked
    /// only for clicks. And the *position* has to be encodable -- X10 caps
    /// each ordinate at 223 against a 2048-column grid, so a report from
    /// column 400 has no byte, and the one it would produce names a
    /// **different cell** rather than a truncated one. Those are refused and
    /// counted in [`Term::mouse_unencodable`]; everything that fits is sent,
    /// which is every position on a terminal anyone actually runs.
    pub fn mouse_report(&mut self, e: MouseEvent) -> bool {
        let wanted = match (self.cold.mouse, e.kind) {
            (MouseTracking::Off, _) => false,
            (MouseTracking::Normal, MouseKind::Motion) => false,
            (MouseTracking::ButtonEvent, MouseKind::Motion) => e.button != MouseButton::None,
            _ => true,
        };
        if !wanted {
            return false;
        }
        if let Some(max) = self.cold.mouse_encoding.max_ordinate() {
            let (r, c) = (e.row.one_based().get(), e.col.one_based().get());
            if r > max || c > max {
                self.mouse_unencodable += 1;
                return false;
            }
            self.queue(Report::MouseX10(e));
            return true;
        }
        self.queue(Report::MouseSgr(e));
        true
    }

    /// Queue a focus report, if the application asked for them with `?1004`.
    ///
    /// Returns whether anything was queued, the way [`Term::mouse_report`]
    /// does, so an embedder can tell "the application does not want this"
    /// from "it was sent". `CSI I` on gain, `CSI O` on loss
    /// (`[[source: ctlseqs]]`, "FocusIn/FocusOut"); nothing when the mode is
    /// reset, because an application that did not ask reads `CSI I` as a
    /// keypress.
    pub fn focus(&mut self, f: Focus) -> bool {
        if !self.cold.modes.is_set(ModeBit::FocusReporting) {
            return false;
        }
        self.queue(Report::Focus(f));
        true
    }

    /// Put a report in the reply buffer.
    ///
    /// The same rule `Paint::want_reply` follows: a truncated reply is a wrong
    /// answer, so it is dropped whole and counted rather than half-sent.
    fn queue(&mut self, r: Report) {
        let bytes = r.encode(self.cold.c1);
        let b = bytes.as_bytes();
        let at = self.reply_len.index();
        match at + b.len() <= self.store.reply.len() {
            true => {
                self.store.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
            }
            false => self.reply_overflows += 1,
        }
    }

    /// ANSI or VT52.
    ///
    /// **The protocol lives in the parser node and this is the read-out.** An
    /// embedder needs it because the two answer keys differently -- the VT52
    /// keyboard sends `ESC A` where ANSI sends `CSI A` -- and `Term::key`
    /// reads the same field for the same reason.
    pub fn terminal_mode(&self) -> TerminalMode {
        self.cold.terminal
    }

    /// The window title, as the application set it. Bounded by `OSC_MAX`.
    pub fn title(&self) -> &[u8] {
        &self.store.strs.title[..self.title_len.index()]
    }

    /// The icon name. `OSC 1` sets this, and `OSC 0` sets both.
    pub fn icon_name(&self) -> &[u8] {
        &self.store.strs.icon[..self.icon_len.index()]
    }

    /// A palette override from `OSC 4`, if the application set one.
    ///
    /// `None` means "use your own colour": this core deliberately holds no
    /// default palette, because the mapping from index to pixels belongs to
    /// the renderer and a second copy here would be one to keep in step. It is
    /// also why a query for an entry nothing overrode gets no reply -- naming
    /// a colour the renderer does not use is a wrong-shaped answer.
    pub fn palette(&self, index: Chan) -> Option<(u8, u8, u8)> {
        self.store.strs.palette[index.index()].as_rgb()
    }

    /// The default foreground or background an application set with `OSC
    /// 10`/`11`, if any. `None` means "use your theme's": the renderer
    /// consults this before its own `Theme`, the way it consults
    /// [`Term::palette`] before its palette table.
    pub fn dynamic_color(&self, which: DynamicColor) -> Option<(u8, u8, u8)> {
        self.cold.dynamic[which.index()].as_rgb()
    }

    /// Tell the core what the embedder's theme draws the default foreground
    /// and background as, so an `OSC 10 ; ?` or `OSC 11 ; ?` on a fresh
    /// terminal can be answered. Every theme-aware program asks `OSC 11` to
    /// decide whether it is on a light or a dark background; without this
    /// call the query gets no reply, which is the truthful answer of a core
    /// that has not been told, and not a wrong one. Call it again when the
    /// theme changes.
    pub fn set_default_colors(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let word = |(r, g, b): (u8, u8, u8)| {
            ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        };
        self.cold.defaults = [word(fg), word(bg)];
    }

    /// The URI a cell's link index resolves to, if any.
    ///
    /// The index comes off the cell -- `CellFlags::link` -- rather than from a
    /// position, which is what makes it survive a scroll: the grid rotates and
    /// the cell carries its link with it. The table evicts on collision, so a
    /// live index can resolve to nothing; that renders as an ordinary cell,
    /// the same tolerable degradation an evicted style has.
    pub fn link_uri(&self, l: LinkId) -> Option<&[u8]> {
        if l.is_none() {
            return None;
        }
        let n = self.store.strs.link_len[l.index()].index();
        (n > 0).then(|| &self.store.strs.links[l.index() * LINK_MAX..][..n])
    }

    /// Encode a key press into the reply buffer.
    ///
    /// Returns whether anything was written; `false` is "this key sends
    /// nothing", which is a real answer rather than a failure -- ctlseqs gives
    /// no modified form for `SS3 j`, and a codepoint above `U+00FF` has no
    /// ISO 8859-1 byte. Inventing a sequence for either would be answering
    /// with a shape no application asked for, which this core already refuses
    /// to do on the output side.
    ///
    /// **The encoding is `baud_domain::encode_key`, shared with
    /// `baud-core2`.** What is here is the part that belongs to a terminal
    /// rather than to the protocol: reading the modes off `Term` and putting
    /// the bytes where the replies already go. The alternative was porting 485
    /// lines and having two spellings of the input protocol, which is what a
    /// hand-written CSI recogniser beside the table cost this repo once.
    ///
    /// **One of the five inputs is a constant here, and it used to be two.**
    ///
    /// * **`C1Transmit::SevenBit`**, because this core does not implement
    ///   `S8C1T` -- the same constant `want_reply` already passes, so a key
    ///   and a `DA` cannot disagree about how wide a `CSI` is.
    /// * `terminal` was `TerminalMode::Ansi` on the grounds that "this core
    ///   has no VT52 sub-graph and no DECANM to leave ANSI with". It has both
    ///   now, so it reads the mode. Leaving the constant would have made every
    ///   arrow key send `CSI A` inside VT52, where the keyboard sends
    ///   `ESC A` -- a gap that opened the moment the *parser* gained a feature
    ///   the *encoder* had carried all along.
    pub fn key(&mut self, e: KeyEvent) -> bool {
        let s = KeyState {
            modes: self.cold.modes,
            keypad: self.cold.keypad,
            c1: C1Transmit::SevenBit,
            terminal: self.cold.terminal,
            encoding: self.cold.encoding,
            other_keys: self.cold.other_keys,
            other_keys_format: self.cold.other_keys_format,
            kitty: self.cold.kitty.current(),
        };
        let Some(k) = encode_key(e, s) else {
            return false;
        };
        let b = k.as_bytes();
        let at = self.reply_len.index();
        // A truncated key is a wrong answer, so it is dropped whole and
        // counted rather than half-sent -- the same rule `want_reply` follows,
        // and for the same reason: an application reads the first byte as the
        // start of a sequence that never arrives.
        match at + b.len() <= self.store.reply.len() {
            true => {
                self.store.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
                true
            }
            false => {
                self.reply_overflows += 1;
                false
            }
        }
    }
}

impl Term<'_> {
    /// DECCKM `?1`. Whether the arrow keys send `SS3 A` rather than `CSI A`.
    ///
    /// **Exposed rather than acted on internally, and that is the acting.** An
    /// embedder cannot encode a key press without knowing this, and being told
    /// is the whole of the terminal's part -- the same contract
    /// `bracketed_paste` already has.
    pub fn cursor_keys_application(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorKeys)
    }

    /// DECKPAM/DECKPNM. Whether the keypad sends `SS3`-prefixed sequences.
    pub fn keypad(&self) -> KeypadMode {
        self.cold.keypad
    }

    /// `?1036` metaSendsEscape. Whether Meta prefixes `ESC`.
    pub fn meta_sends_escape(&self) -> bool {
        self.cold.modes.is_set(ModeBit::MetaSendsEscape)
    }

    /// `?1039` altSendsEscape. Whether Alt prefixes `ESC`.
    pub fn alt_sends_escape(&self) -> bool {
        self.cold.modes.is_set(ModeBit::AltSendsEscape)
    }

    /// `?1034` eightBitInput. Whether Alt/Meta add 128 instead of prefixing.
    pub fn eight_bit_input(&self) -> bool {
        self.cold.modes.is_set(ModeBit::EightBitInput)
    }
}
