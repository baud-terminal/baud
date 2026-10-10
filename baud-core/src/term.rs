//! The terminal's state, and the one borrow the parser holds.

use crate::graph::Node;
use crate::osc::StrKind;
use crate::storage::{LINK_MAX, MAX_COLS, MAX_ROWS, Storage};
use baud_domain::*;

/// Which screen buffer is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Normal,
    Alternate,
}

/// Which cursor a resize anchors the primary on; see
/// [`Term::primary_cursor`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum PrimaryCursor {
    /// The primary is up, and the cursor is its own.
    Live,
    /// `?1049`'s alternate screen is up, and the primary's cursor is saved.
    Saved,
    /// `?47`'s alternate screen is up, and the primary has the one shared
    /// cursor, which the step moves nothing of.
    Shared,
}

/// The rendition table's slots written since a count: up to two slices of
/// the log, oldest first, or every slot. See [`Term::styles_written`].
#[derive(Clone, Copy, Debug)]
pub enum Written<'t> {
    Slots([&'t [StyleId]; 2]),
    Every,
}

/// What `?1049 h` saves for the primary screen and `?1049 l` restores: the
/// cursor `DECSC` saves, and the cursor's style and `?12` with it.
///
/// **One record, so the three cannot drift.** A resize rewrites the saved
/// cursor while the alternate screen is up, and a reset takes the save on its
/// way off the alternate screen; kept in separate fields, a style could
/// outlive the cursor saved beside it. `DECSC` does not save the style --
/// `[[source: vt510-decsc]]` lists seven things and the cursor's style is not
/// one of them, and xterm, foot, ghostty and alacritty agree; kitty and
/// wezterm save it with the cursor.
#[derive(Clone, Copy)]
pub(crate) struct AltSaved {
    pub(crate) cursor: Saved,
    pub(crate) style: CursorStyle,
    pub(crate) blink: ModeChange,
}

/// What `DECSC` saves and `DECRC` restores.
#[derive(Clone, Copy)]
pub struct Saved {
    pub(crate) cy: Row,
    pub(crate) cx: Col,
    /// The rendition, and not the id it was interned at: an eviction between
    /// the save and the restore can give that id to another rendition, so
    /// `restore` interns this again.
    pub(crate) style_raw: StyleWord,
    pub(crate) flags: CellFlags,
    /// **The four slots and which one `GL` invokes, not the resolved set.**
    /// `[[source: vt510-decsc]]` lists "Character sets (G0, G1, G2, or G3)
    /// currently in GL and GR" among what DECSC saves, and a saved *resolved*
    /// set restores into slots that no longer agree with it -- the next `SI`
    /// would recompute from `g[gl]` and throw the restore away. `GR` is not
    /// saved because this core does not model it.
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

/// How the window moved between two rotations; see [`Term::moved_since`]
/// and [`Term::history_moved_since`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Moved {
    Retired(Rows),
    Restored(Rows),
}

/// Where the primary screen's window stood, noted by [`Term::history_mark`]:
/// what a window scrolled back into the history is anchored against. It is
/// the absolute line at the primary window's top -- which the primary holds
/// whatever screen is up, and which does not move while the alternate screen
/// does -- rather than a rotation, so it is not read modulo a ring: a flood
/// that retires more than half the ring in one read is still that many rows,
/// where the short way round a ring would read it as rows restored. Its own
/// type, so the live [`RowRot`] [`Term::rotation`] answers -- the other
/// ring's while the alternate screen is up -- cannot be passed here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryMark(LineIdx);

/// Which surface is up, and which time it went up: it moves on every switch
/// between the primary and the alternate screen, in either direction. Two
/// equal readings saw one surface throughout -- where [`Term::screen`] read
/// at both ends of a read cannot tell one that left the alternate screen and
/// entered it again, which clears it and resets its ring, from one that
/// never left.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenEpoch(u32);

impl ScreenEpoch {
    pub(crate) const FIRST: ScreenEpoch = ScreenEpoch(0);

    pub(crate) fn next(self) -> ScreenEpoch {
        ScreenEpoch(self.0.wrapping_add(1))
    }
}

/// Which scroll of a region the primary retired a line through: it moves on
/// every scroll of a region smaller than the screen that sends its top line
/// to the history. Two equal readings mean every line a read retired left
/// with the whole screen -- where [`Term::history_moved_since`] counts a
/// region's retired lines and the whole screen's alike, though a region's
/// leave the rows under it where they were. So an embedder that moves the
/// whole picture with the history -- a smooth scroll, a selection numbered
/// by lines -- asks this before it does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegionScrolls(u32);

impl RegionScrolls {
    pub(crate) const FIRST: RegionScrolls = RegionScrolls(0);

    pub(crate) fn next(self) -> RegionScrolls {
        RegionScrolls(self.0.wrapping_add(1))
    }
}

/// Which erasure of the primary's history the terminal is at: it moves on
/// every `CSI 3 J` and every `RIS`, the two operations that empty the saved
/// lines, and on nothing else -- not `CSI ? 3 J`, whose lines stay. A window
/// over the history read at both ends of a read cannot see one: the erase
/// scrolls nothing, so [`Term::history_moved_since`] reads no movement, and
/// output after it in the same read refills the history to a count that
/// says nothing of what went. An embedder holding a window into the history
/// asks this, and finds the lines it showed are gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistoryErasures(u32);

impl HistoryErasures {
    pub(crate) const FIRST: HistoryErasures = HistoryErasures(0);

    pub(crate) fn next(self) -> HistoryErasures {
        HistoryErasures(self.0.wrapping_add(1))
    }
}

/// A count of `?5` DECSCNM changes, as a reading to compare. The core keeps
/// two: the changes made where a frame could show them
/// ([`Term::reverse_flips`]), and the changes made inside a synchronised
/// update ([`Term::reverse_flips_held`]), which the application asked not to
/// be seen. A driver that reads a count at two frames, finds it moved, and
/// finds the mode reading the same at both knows the other video came and
/// went with no frame drawing it -- a visual bell sent in one read, the way
/// `tput flash` sends it. The held count is for a driver that has stopped
/// honouring an update left open, which then shows what it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReverseFlips(u32);

impl ReverseFlips {
    pub(crate) const FIRST: ReverseFlips = ReverseFlips(0);

    pub(crate) fn next(self) -> ReverseFlips {
        ReverseFlips(self.0.wrapping_add(1))
    }
}

/// Which opening of a synchronised update (`?2026`) the terminal is at: it
/// moves when the mode goes from reset to set, and on nothing else. A driver
/// that bounds how long it honours an update times each one from its
/// opening, and a close and a reopen in one read would otherwise leave it
/// timing the new update from the old one's start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UpdatesOpened(u32);

impl UpdatesOpened {
    pub(crate) const FIRST: UpdatesOpened = UpdatesOpened(0);

    pub(crate) fn next(self) -> UpdatesOpened {
        UpdatesOpened(self.0.wrapping_add(1))
    }
}

/// A Sixel's requested background, from its `Pb`/`P2` parameter. `Opaque`
/// (`P2` = 0 or 2) asks for unpainted pixels to be filled rather than left
/// transparent; baud renders them transparent and counts the request, so this
/// says which Sixels to count. Not a `bool`, so the two-state meaning is named
/// and the field cannot be confused with a flag.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SixelBg {
    /// `P2` = 1, or absent: unpainted pixels are transparent, which baud does.
    Transparent,
    /// `P2` = 0 or 2: an opaque background baud does not fill; counted.
    Opaque,
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
    /// `?1005`, `?1006`, `?1015`, `?1016`. Setting one replaces the one in
    /// force and resetting one returns to X10 only if it is the one in force
    /// ([`MouseEncoding::after`]); X10 is the protocol's default rather than
    /// "off" -- there is no off.
    pub(crate) mouse_encoding: MouseEncoding,
    /// **The cell of the last report the application was sent**, which
    /// button-event and any-event tracking measure motion from:
    /// `[[source: ctlseqs]]`, "Motion events are reported only if the mouse
    /// pointer has moved to a different character cell." Set by every report
    /// that went into the reply buffer -- a press and a release as well as a
    /// move, because after a press in a cell a drag inside it has not moved
    /// to a different one, and xterm's `EditorButton` sets its `mouse_row`
    /// and `mouse_col` on every event it reports (`[[source: xterm-button]]`)
    /// -- and by nothing that was refused, so a position the application was
    /// never told about cannot suppress the move that tells it. `None` since
    /// power-on, a tracking or encoding change, or a reset: a report sent
    /// before any of those is not one the application now listening was
    /// given. A cell and not a pixel under `?1016` too -- the text's gate,
    /// where xterm's compares pixels (`Term::mouse_report`).
    pub(crate) mouse_at: Option<(Col, Row)>,
    /// `?1007`, on at power-on and **kept by both resets**, as xterm keeps
    /// `alternateScroll` (no `ReallyReset` path names it, `[[source:
    /// xterm-reset]]`'s file). Read by the embedder's wheel, never here.
    pub(crate) alternate_scroll: AlternateScroll,
    /// `?1001`. Recorded and exposed, because a core with no selection model
    /// has no other way to act on it -- and answering `DECRQM` truthfully
    /// requires knowing.
    pub(crate) highlight: HighlightTracking,
    pub(crate) highlight_region: HighlightRegion,
    /// **The dynamic colours -- `OSC 10`, `11` and `12` -- as three pairs**,
    /// indexed by [`DynamicColor::index`] (foreground, background, cursor).
    /// `defaults` is what the embedder's theme says the foreground and
    /// background are, pushed in through [`Term::set_default_colors`]; the
    /// cursor has no embedder default because the renderer draws it
    /// reverse-video until an application sets one. `dynamic` is an
    /// application's override from `OSC 10`/`11`/`12`, `DEFAULT` meaning none.
    /// A query answers the override if there is one, else the default if the
    /// embedder supplied one, else nothing -- the same rule `OSC 4` answers by,
    /// whose embedder table sits in `StrBuf`'s palette slice; naming a colour
    /// the renderer does not use is a wrong-shaped answer. `OSC 110`/`111`/`112`
    /// clear the override and leave the default, which is what "reset" means.
    /// Here rather than in `StrBuf` so no caller-provided buffer grows a field.
    pub(crate) defaults: [ColorWord; 3],
    pub(crate) dynamic: [ColorWord; 3],
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
    /// A Sixel body's length, `ReadLen` rather than `ByteLen` because a Sixel
    /// body runs to tens of kilobytes where `str_len` is a `u16`. It counts the
    /// **true** body length and keeps rising past the scratch buffer, so a body
    /// that overflowed is `sixel_len > sixel_body.len()` at the terminator and
    /// is refused whole rather than decoded truncated.
    pub(crate) sixel_len: ReadLen,
    /// A Sixel's `Pb`/`P2` background selector, read from the DCS parameters at
    /// `SixelStart` and consulted at the terminator so a Sixel that is *refused*
    /// (no arena, an overflowed body) is not counted as an ignored request --
    /// the counter is a decision input, and a refused image renders nothing for
    /// a fill to have helped.
    pub(crate) sixel_bg: SixelBg,
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
    /// Whether the Primary DA may claim Sixel. `No` until the embedder opts
    /// in, whatever the store holds: an application that reads the claim stops
    /// drawing its text fallback, so the claim is the embedder's to make.
    pub(crate) sixel_claim: SixelClaim,
    /// The style `DECSCUSR` set, or the configuration: its shape, and its own
    /// blink. What `DECRQSS SP q` answers.
    pub(crate) cursor_style: CursorStyle,
    /// `?12`, which blinks the cursor *or*-ed with the style's own blink --
    /// foot's rule -- and never stops it: [`Term::cursor_style`] draws a
    /// blink when either says so. What `DECRQM ?12` answers.
    pub(crate) cursor_blink: ModeChange,
    /// The style `DECSCUSR 0` and `7`, `DECSTR` and `RIS` restore: the
    /// embedder's ([`Term::set_initial_cursor_style`]), a blinking block
    /// until it says otherwise.
    pub(crate) initial_cursor: CursorStyle,
    /// Whether a `U+FE0F` widens its base ([`Term::set_vs16_width`]).
    pub(crate) vs16: Vs16Width,
    pub(crate) keypad: KeypadMode,
    pub(crate) protect: CellFlags,
    pub(crate) screen: Screen,
    /// Moves on every switch of `screen`: [`Term::screen_epoch`].
    pub(crate) epoch: ScreenEpoch,
    /// The scrolling region, inclusive at both ends.
    pub(crate) top: Row,
    pub(crate) bot: Row,
    /// Moves on every region scroll that advances a ring, on either screen:
    /// [`Term::region_scrolls`].
    pub(crate) region_scrolls: RegionScrolls,
    /// Moves on every erasure of the primary's history:
    /// [`Term::history_erasures`].
    pub(crate) history_erasures: HistoryErasures,
    /// Moves on every change of `?5` outside a synchronised update, and
    /// inside one: [`Term::reverse_flips`], [`Term::reverse_flips_held`].
    pub(crate) flips: ReverseFlips,
    pub(crate) flips_held: ReverseFlips,
    /// Moves on every opening of `?2026`: [`Term::updates_opened`].
    pub(crate) updates_opened: UpdatesOpened,
    pub(crate) saved: Option<Saved>,
    pub(crate) alt_saved: Option<AltSaved>,
    /// Which mode put the alternate screen up, while it is up: `?1049` saved
    /// the primary's cursor in `alt_saved`, `?47` saved nothing and shares
    /// the one cursor. Read by a resize, which anchors the primary on
    /// whichever cursor the primary will have when the alternate screen goes.
    pub(crate) alt_entry: crate::ops::AltSwitch,
    pub(crate) modes: Modes,
    /// XTSAVE's one-level cache of the private modes, which XTRESTORE reads
    /// back: written and read once per `CSI ? Pm s` and `r`, emptied by
    /// `RIS`, never on the write path.
    pub(crate) xtsave: SavedModes,
    /// **The kitty keyboard flag stacks, one per screen.** `kitty` is the
    /// active screen's and `kitty_alt` the parked one; the `?1049` transition
    /// swaps them, which is the specification's separate main/alternate stacks
    /// -- an editor changes the mode in the alternate screen without touching
    /// the shell's. Written by the negotiation forms, and read by `Term::key`,
    /// which hands the active flags to the encoder, and by `CSI ? u`, which
    /// replies with them -- `[[source: kitty-keyboard]]`: "The terminal will
    /// reply with: CSI ? flags u".
    pub(crate) kitty: KittyStack,
    pub(crate) kitty_alt: KittyStack,
    /// What `OSC 133` has said about the live screen: which half of its
    /// life this is (see [`Zone`]) and where the prompt block the shell is
    /// at begins (`prompt.rs`). `shell_other` is the parked screen's, and
    /// the `?1049` transition swaps them -- the kitty stacks' shape, for the
    /// reason `prompt::Shell` gives.
    pub(crate) shell: crate::prompt::Shell,
    pub(crate) shell_other: crate::prompt::Shell,
    /// Where a program put the cursor before it hid it (`placed.rs`):
    /// decided on every `?25l` by looking back at the bytes before it, with
    /// the stream's last 31 bytes before the read kept for that, and read by
    /// [`Term::placed_cursor`] for an embedder's cursor trail.
    pub(crate) placement: crate::placed::Placement,
}

// **The size is gated because the prose drifted.** A doc comment called `Cold`
// 120 bytes after it had grown to 192 (charset slots, input modes); nothing
// forced the number to be acknowledged. This does, and it grew to 256 when the
// two per-screen kitty stacks landed -- which is free on the write path,
// because `Cold` is carried by reference and the loop never reads them.
// And to 264 for `prompt`, the prompt block's ring row: written once per
// `OSC 133 ; P` and read by `Term::prompt_row`, which is a resize-time and
// embedder-side question the per-byte loop never asks.
// And to 288 for the dynamic colours: three `ColorWord` pairs, written once
// per `OSC 10`/`11`/`12` and by the embedder's theme, read by a query and by
// the renderer once a frame. Measured on landing rather than assumed free --
// vim and btop interleaved with an A/A copy, every range overlapping. The
// cursor colour took this from 280 to 288, one word each on `defaults` and
// `dynamic`, on `Cold` which the per-byte loop carries by reference. The Sixel
// body length (`sixel_len: ReadLen`) took it from 288 to 296 -- a `usize`,
// because a Sixel body runs to tens of kilobytes past `str_len`'s `u16`; it is
// read only while a Sixel is parsing, not per glyph. The prompt state went
// per screen at 320: two `Shell`s, a zone and a block start each, where one
// zone and one block were -- written once per `OSC 133` mark and swapped once
// per `?1049` transition, never on the write path. `region_scrolls` took it
// to 328: a `u32` bumped once per region scroll that retires a line, which the
// per-byte loop reaches only through a scroll. The two `?5` counts took it to
// 336 -- the first `u32` landed in padding and the second did not -- and the
// count of `?2026` openings landed in padding again. Each `?5` change bumps
// one of the two, and each opening the third, all on control sequences the
// per-byte loop reaches only through its dispatch. The initial cursor style
// took it to 344: one byte, set by the embedder and read by `DECSCUSR 0` and
// `7` and by both resets, none of them on the write path. The placement took
// it to 408: a placed position, the stream's last 31 bytes before the read --
// copied once a read -- and where the read starts, all read on a `?25l` and
// by the embedder, and none on the write path. `?12` as a bit of its own, and
// the style and that bit saved with `?1049`'s cursor, took it to 416: written
// by `?12`, `?1049` and the resets, and read by the embedder once a frame,
// none of them on the write path. The last reported mouse cell took it to
// 440, none of its 24 bytes in padding: `Option<(Col, Row)>`, written by a
// pointer event that reports, a mouse mode change and the resets, and read by
// `Term::mouse_report` alone -- input-side, an embedder's call per pointer
// event, which the per-byte loop never makes. The count of history erasures
// landed in padding beside `region_scrolls`, so 440 held: a `u32` bumped by
// `CSI 3 J` and `RIS` and read by the embedder after a read. XTSAVE's cache
// took it to 456, none of its 16 bytes in padding: two `u64` words, which
// slots were saved and the value each had, written by `CSI ? Pm s` and
// `RIS` and read by `CSI ? Pm r` -- arms of the dispatch, never the write
// path.
// Guarded off `sgr-table`, which adds `sgr: [SgrSlot; NSGR]` and grows it more.
#[cfg(not(feature = "sgr-table"))]
const _: () = assert!(core::mem::size_of::<Cold>() == 456);

impl Cold {
    /// What `OSC 10`/`11`/`12 ; ?` answers for `which`: the application's
    /// override, else the embedder's default -- `ColorWord::DEFAULT` when
    /// neither exists. The one spelling of it, so `CSI ? 996 n` and the
    /// `OSC 11` query cannot disagree.
    pub(crate) fn shown(&self, which: DynamicColor) -> ColorWord {
        let w = self.dynamic[which.index()];
        if w.is_default() {
            self.defaults[which.index()]
        } else {
            w
        }
    }

    /// The dark/light class of the colours `OSC 10` and `11` would answer, or
    /// `None` where either query would be silent.
    pub(crate) fn color_scheme(&self) -> Option<ColorScheme> {
        ColorScheme::of(
            self.shown(DynamicColor::Foreground),
            self.shown(DynamicColor::Background),
        )
    }

    pub(crate) fn new(rows: Rows, cols: Cols) -> Cold {
        Cold {
            #[cfg(feature = "sgr-table")]
            sgr: [SgrSlot::EMPTY; NSGR],
            g: [Charset::Ascii; 4],
            gl: GSlot::G0,
            shell: crate::prompt::Shell::FRESH,
            shell_other: crate::prompt::Shell::FRESH,
            placement: crate::placed::Placement::NONE,
            mouse: MouseTracking::Off,
            mouse_encoding: MouseEncoding::X10,
            mouse_at: None,
            alternate_scroll: AlternateScroll::On,
            defaults: [ColorWord::DEFAULT; 3],
            dynamic: [ColorWord::DEFAULT; 3],
            highlight: HighlightTracking::Off,
            highlight_region: HighlightRegion::default(),
            terminal: TerminalMode::Ansi,
            vt52_row: Vt52Coord::ZERO,
            str_kind: StrKind::Other,
            str_len: ByteLen::ZERO,
            sixel_len: ReadLen::ZERO,
            sixel_bg: SixelBg::Transparent,
            c1: C1Transmit::SevenBit,
            // The base layer, which is what an application that has not asked
            // for an enhancement must receive.
            other_keys: OtherKeys::Off,
            other_keys_format: OtherKeysFormat::Xterm,
            encoding: InputEncoding::Utf8,
            left: Col::ZERO,
            right: Col::at_index(cols.index() - 1),
            ambiguous: AmbiguousWidth::Narrow,
            sixel_claim: SixelClaim::No,
            cursor_style: CursorStyle::BlinkingBlock,
            cursor_blink: ModeChange::Reset,
            initial_cursor: CursorStyle::BlinkingBlock,
            vs16: Vs16Width::Two,
            keypad: KeypadMode::Numeric,
            protect: CellFlags::NONE,
            screen: Screen::Normal,
            epoch: ScreenEpoch::FIRST,
            top: Row::ZERO,
            bot: Row::from_index(rows.index() - 1),
            region_scrolls: RegionScrolls::FIRST,
            history_erasures: HistoryErasures::FIRST,
            flips: ReverseFlips::FIRST,
            flips_held: ReverseFlips::FIRST,
            updates_opened: UpdatesOpened::FIRST,
            saved: None,
            alt_saved: None,
            alt_entry: crate::ops::AltSwitch::SaveAndClear,
            kitty: KittyStack::new(),
            kitty_alt: KittyStack::new(),
            // **DECAWM defaults on**, which is xterm's behaviour rather than
            // the VT510 manual's; `docs/vt-coverage.md` records the choice.
            // `baud-core2` made the same one, so the golden capture of what it
            // rendered (`tests/golden.rs`) compares like with like.
            modes: Modes::default(),
            xtsave: SavedModes::NONE,
        }
    }
}

/// The embedder's colours, handed over whole by [`Term::set_theme`]: the
/// default foreground and background (what `OSC 10`/`11 ; ?` answer when no
/// application overrode them) and the 256-entry table (what `OSC 4 ; c ; ?`
/// answers).
pub struct ThemeColors<'a> {
    pub fg: (u8, u8, u8),
    pub bg: (u8, u8, u8),
    pub palette: &'a [(u8, u8, u8); 256],
}

/// Whether [`Term::set_theme`] drops the application's `OSC 4`/`10`/`11`/`12`
/// overrides with it. `Drop` is an OS light/dark flip under `theme = "auto"`;
/// `Keep` is everything else, launch included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overrides {
    Keep,
    Drop,
}

/// A terminal: a cursor, a rendition, a grid, and where the parser stands.
///
/// **All of it behind one `&mut self`, and state does not migrate to
/// statics.** A C prototype from before the import is said to have run slower
/// with file-scope globals (`git show f5399fb:CLAUDE.md`, line 39); no harness,
/// data or record survives, so no figure is quoted here, and the mechanism,
/// LLVM's `noalias`, is presumed rather than shown.
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
    /// The height of a cell in device pixels, or [`ImagePx::ZERO`] when the
    /// embedder has not said. It is the one number a headless core lacks to
    /// advance the cursor past a Sixel -- `ceil(image_pixel_height / cell_h)`
    /// rows -- so it is set by the renderer's metrics through
    /// [`Term::set_cell_size`], not by any control sequence, and it survives
    /// `RIS`: a reset does not change the physical cell size. Zero means "do
    /// not advance", which keeps a headless test or bench at today's
    /// behaviour.
    pub(crate) cell_height: ImagePx,
    /// The width of a cell in device pixels, or [`ImagePx::ZERO`] when the
    /// embedder has not said. Paired with [`Term::cell_height`], it is what
    /// lets the core free a Sixel when text is written over its cells: an
    /// image spans `ceil(w / cell_width)` columns, and a previewer replacing
    /// one preview with the next relies on the overwrite removing the old
    /// image. Zero (a headless caller) frees nothing.
    pub(crate) cell_width: ImagePx,
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
    /// back through a table that evicts once it is full.
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
    /// `OSC 8` links refused because their slot already held a different
    /// URI. A slot is taken once for the storage's life, so a cell never
    /// resolves to a target other than the one written under its index;
    /// past 127 distinct targets every new link is refused and this says so.
    pub links_refused: usize,
    /// `OSC 8` links refused because the URI may have been cut -- past
    /// `LINK_MAX`, or a body that filled `OSC_MAX` -- and a cut URL is a
    /// different URL.
    pub links_clipped: usize,
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
    /// silence. Columns are free to the pitch, which `store_stride` derives
    /// from the provisioned width and the cache line -- 208 cells for the
    /// corpus and 400 for the app on a 128-byte line -- so a width past the
    /// pitch or past `MAX_COLS` is `Truncated` too, and one between the
    /// provisioned width and both is not.
    ///
    /// **A change of width re-cuts the primary and not the alternate
    /// screen.** Between output and output the primary's lines are re-cut to
    /// the new width in place, through the history (`View::reflow`); at a
    /// prompt nothing is re-cut, because the shell redraws those rows by
    /// counting them (see [`Zone`]). The alternate screen is cell-addressed:
    /// the pitch is fixed, so a cell keeps its address whatever the width, a
    /// narrower screen stops addressing what lies past its edge, and a wider
    /// one blanks the columns it reveals ([`View::widened`]) because the
    /// program owns them and redraws. What keeps the fixed pitch sound is that
    /// a clear runs through a mark that lies past the live edge, so a line
    /// erased while the screen was narrow does not come back with a tail --
    /// and a mark is true to the live edge and no further, so a growth has to
    /// raise it where the columns it reveals are not the row's uniform cell
    /// (`View::settle_growth` on the primary, whatever the zone). The
    /// height's half is the rotation moving over
    /// the ring, below. [`Term::resize_holding`] is this with a window over
    /// the history held on its line.
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
    /// **One thing core2 did here and this does not, deliberately.** It
    /// rebuilt the tab table; this one is at least `MAX_COLS` long and stamped
    /// every eighth column at construction, so no width change can invalidate
    /// it.
    pub fn resize(&mut self, ws: &WinSize) -> Resized {
        self.resize_holding(ws, Rows::ZERO).0
    }

    /// Which cursor the primary has while a resize runs: its own on the
    /// primary; the one `?1049` saved while that mode's alternate screen is
    /// up; and under `?47`, which saves none, the one cursor both screens
    /// share -- which is the primary's again when `?47 l` leaves, since it
    /// restores nothing. Anchoring the primary on the save while `?47` was up
    /// read row 0, or a row an earlier `?1049` left behind, and a shrink then
    /// cut the primary's bottom rows where it should have retired its top
    /// ones.
    fn primary_cursor(&self) -> PrimaryCursor {
        match (self.cold.screen, self.cold.alt_entry) {
            (Screen::Normal, _) => PrimaryCursor::Live,
            (Screen::Alternate, crate::ops::AltSwitch::SaveAndClear) => PrimaryCursor::Saved,
            (Screen::Alternate, crate::ops::AltSwitch::Bare) => PrimaryCursor::Shared,
        }
    }

    /// [`Term::resize`], holding a window over the primary's history on the
    /// line it shows: `back` is how many lines above the primary window's
    /// top it starts, and the answer is where that line is afterwards, in
    /// the same unit and bounded by the history.
    ///
    /// **Only the terminal can say, because a change of width re-cuts the
    /// lines.** Anchoring the window by how far the window's top line moved
    /// -- [`Term::history_moved_since`] -- counts the growth of every line
    /// above the window, including the lines above the one the window shows,
    /// which do not move it at all; long lines there put the window on
    /// another line. So the window's first row is mapped through the re-cut
    /// the way an image's row is, landing where its first cell lands, and a
    /// line the re-cut dropped leaves the window on the oldest line kept.
    /// **A live window stays live**: `back` of zero is the live screen,
    /// whatever the re-cut did, and so is a line that lands on it.
    pub fn resize_holding(&mut self, ws: &WinSize, back: Rows) -> (Resized, Rows) {
        let (want_cols, want_rows) = ws.geometry();
        let cap = self.store.row_capacity();
        let cols = want_cols.clamp(MAX_COLS).clamp(self.stride.index());
        let rows = want_rows.clamp(MAX_ROWS).clamp(cap.index());
        let over = cols.index() != want_cols.index() || rows.index() != want_rows.index();
        let (old_rows, old_cols) = (self.rows, self.cols);
        // A change of geometry moves or re-cuts the cells a placement names
        // (`placed.rs`); the same geometry leaves it.
        if (rows, cols) != (old_rows, old_cols) {
            self.cold.placement.forget();
        }
        // The window over the history, in lines from the window's top, as it
        // moves with the steps below; `None` is the live screen.
        let mut held = (back.index() > 0).then_some(back);

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
        let top_before = self.store.book.top;
        if rows.index() != old_rows.index() {
            let ring = self.store.ring_rows();
            let spare = ring.index() - rows.index();
            let (from, to) = (old_rows.index(), rows.index());
            let at = self.primary_cursor();
            let mut cy = match at {
                PrimaryCursor::Live | PrimaryCursor::Shared => self.cy.index(),
                PrimaryCursor::Saved => self.cold.alt_saved.map_or(0, |s| s.cursor.cy.index()),
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
                // The window's top line moves on with it, so an image on a
                // retired row keeps its row -- it is behind the window now.
                self.store.book.top = self.store.book.top.plus(Rows::of_count(retire));
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
                // Back with the window; `pull` is within the history, and the
                // top line is never below the history's depth.
                self.store.book.top = self
                    .store
                    .book
                    .top
                    .minus(Rows::of_count(pull))
                    .unwrap_or(LineIdx::FIRST);
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
            // A change of height moves lines past the window's top one for
            // one: the window follows them, as it does output.
            let now = self.store.book.top;
            held = held
                .map(|b| {
                    let at = match now.since(top_before) {
                        Some(n) => b.index().saturating_add(n.index()),
                        None => b
                            .index()
                            .saturating_sub(top_before.since(now).map_or(0, |n| n.index())),
                    };
                    Rows::of_count(at.min(hist))
                })
                .filter(|b| b.index() > 0);
            self.store.book.hist = Rows::of_count(hist);
            self.store.book.hidden = Rows::of_count(hidden);
            match at {
                PrimaryCursor::Live => self.cy = Row::from_index(cy),
                PrimaryCursor::Saved => {
                    if let Some(s) = self.cold.alt_saved.as_mut() {
                        s.cursor.cy = Row::from_index(cy.min(to - 1));
                    }
                }
                // The alternate screen's cursor too, and this step does not
                // move the alternate screen's rows.
                PrimaryCursor::Shared => {}
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
        if cols.index() != old_cols.index() && self.primary_zone() == Zone::Output {
            let at = self.primary_cursor();
            let cursor = match at {
                PrimaryCursor::Live | PrimaryCursor::Shared => (self.cy, self.cx),
                PrimaryCursor::Saved => self
                    .cold
                    .alt_saved
                    .map_or((Row::ZERO, Col::ZERO), |s| (s.cursor.cy, s.cursor.cx)),
            };
            // **The primary's images follow their rows through the re-cut.**
            // Each is staged at its row counted from the oldest retained line,
            // mapped as the reflow visits its line -- before the line is
            // copied, while its cells can still be read -- and written back
            // against the same origin, which a reflow keeps. A line the reflow
            // drops takes its image with it, counted.
            let ring = self.store.ring_rows();
            let floor = self
                .store
                .book
                .top
                .minus(self.store.book.hist)
                .unwrap_or(LineIdx::FIRST);
            let images = &mut self.store.images;
            let staged = match images.any_placed() {
                true => images.follow_begin(ImageScreen::Primary, floor),
                false => 0,
            };
            // Whether the reflow visited a line: one that returned before
            // the re-cut visited none, and staged placements it never mapped
            // are still where they were, not gone.
            let mut visited = false;
            // The window's first row, counted from the oldest retained row as
            // the hook counts, and where the re-cut put it.
            let hist = self.store.book.hist;
            let view = held.map(|b| hist.index().saturating_sub(b.index()));
            let mut landed: Option<Rows> = None;
            let mut hook = |from: Rows, n: Rows, land: &dyn Fn(Rows) -> Rows| {
                visited = true;
                if staged > 0 {
                    images.follow_line(from, n, land);
                }
                if let Some(v) = view {
                    if (from.index()..from.index() + n.index()).contains(&v) {
                        landed = Some(land(Rows::of_count(v)));
                    }
                }
            };
            let follow: Option<Follow<'_>> = match (staged, view) {
                (0, None) => None,
                _ => Some(&mut hook),
            };
            let r = View::new(
                self.store.vp,
                self.store.used,
                self.stride,
                Cols::of_index(old_cols.index().max(cols.index())),
                rows,
                ring,
                self.rot,
            )
            .reflow(
                old_cols,
                cols,
                self.store.book.hist,
                self.store.book.hidden,
                cursor,
                follow,
            );
            match (staged, visited) {
                (0, _) => {}
                (_, true) => {
                    self.store.images.follow_end(floor);
                }
                (_, false) => self.store.images.follow_cancel(),
            }
            // The window over the history, from where its first row landed:
            // lines from the new window top, which is the history's depth past
            // what the re-cut dropped. A line that landed on the live screen
            // or below it is the live screen; one the re-cut dropped is the
            // oldest line kept.
            held = held.map(|_| match landed {
                Some(l) => {
                    Rows::of_count((r.dropped.index() + r.hist.index()).saturating_sub(l.index()))
                }
                None => r.hist,
            });
            self.rot = r.rot;
            self.store.book.hist = r.hist;
            self.store.book.hidden = r.hidden;
            // The oldest retained line moved on by what the reflow dropped,
            // and the window's top is the history's depth after it.
            self.store.book.top = floor.plus(r.dropped).plus(r.hist);
            match at {
                PrimaryCursor::Live => (self.cy, self.cx) = r.cursor,
                PrimaryCursor::Saved => {
                    if let Some(s) = self.cold.alt_saved.as_mut() {
                        (s.cursor.cy, s.cursor.cx) = r.cursor;
                    }
                }
                PrimaryCursor::Shared => {}
            }
        }
        // **A growth keeps both screens' marks honest, whatever the zone.**
        // Only the primary's *re-cut* waits on its zone. The alternate
        // screen's `widened` sat inside the same gate and nothing raised a
        // primary mark outside it, so at a shell prompt a growth raised no
        // mark on the primary -- a band erased in a colour while narrow
        // survived the next default erase -- and left the alternate screen's
        // revealed columns and marks as they were, so the next program's
        // entry clear skipped them and the frame drawn when the screen was
        // last that wide showed under the new one.
        if cols.index() > old_cols.index() {
            View::new(
                self.store.vp,
                self.store.used,
                self.stride,
                cols,
                rows,
                self.store.ring_rows(),
                self.rot,
            )
            .settle_growth(old_cols);
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
        // **And a taller alternate screen blanks the rows it reveals**, the
        // height half of `widened`. Its ring is the provisioned height, so a
        // line that scrolled off its top, or a row a shorter screen cut off
        // its bottom, is still in the ring past the window; a growth moved
        // the window's bottom edge over them, and the lines a program had
        // scrolled away showed under its next frame until it repainted
        // (`a_taller_alternate_screen_reveals_blank_rows_not_scrolled_ones`).
        // A rule of baud's own, stated as one: what xterm's resize does to
        // its alternate buffer is in `screen.c`, which is not pinned. Blanked
        // to the wider of the two widths, as the primary's revealed rows
        // are, and trusting marks that `widened` keeps true over the whole
        // ring, so a tail past a narrower edge goes with them.
        if rows.index() > old_rows.index() {
            let mut v = View::new(
                self.store.alt.cells,
                self.store.alt.used,
                self.stride,
                Cols::of_index(old_cols.index().max(cols.index())),
                rows,
                self.store.row_capacity(),
                self.store.alt.rot,
            );
            let mut n = 0;
            for y in old_rows.index()..rows.index() {
                n += v.fill_row(Row::from_index(y), PackedCell::EMPTY).index();
            }
            self.cells_written += n;
        }

        self.rows = rows;
        self.cols = cols;
        self.cy = Row::from_index(self.cy.index().min(rows.index() - 1));
        self.cx = Col::at_index(self.cx.index().min(cols.index() - 1));
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(rows.index().saturating_sub(1));
        self.cold.left = Col::ZERO;
        self.cold.right = Col::at_index(cols.index().saturating_sub(1));
        // **An image whose row the resize took away goes with it**: a row
        // past what the primary's ring now retains, or past the bottom of a
        // shorter alternate screen, which keeps nothing below it.
        if self.store.images.any_placed() {
            self.expire_images();
        }
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
            self.queue(Report::inband(
                rows,
                cols,
                self.cell_width,
                self.cell_height,
            ));
        }
        let resized = match over {
            true => Resized::Truncated { cols, rows },
            false => Resized::Ok,
        };
        let held = held.unwrap_or(Rows::ZERO);
        // Both steps bound it by the history they leave: the height by its
        // own clamp, the re-cut because a kept row lands no further back
        // than the oldest kept.
        debug_assert!(held.index() <= self.store.book.hist.index());
        (resized, held)
    }

    pub fn new(store: Storage<'b>, ws: &WinSize) -> Term<'b> {
        let mut store = store;
        Term::fill_cells3(&mut store);
        // **Slot 0 is the default rendition, reserved.** A cell carries
        // `StyleId::DEFAULT` without anything being interned -- an empty
        // cell, an erased one, every cell written before the first `SGR` --
        // so the slot has to hold the default for the table's whole life.
        // Left empty, the first rendition to probe into it took it and every
        // default cell took that rendition's colours. The default's key is 0,
        // so this is the slot it would take anyway; `intern_style` never
        // evicts it.
        //
        // Written and logged once, when the table is new: a storage lent again
        // has slot 0 already, and its log already holds the write.
        if store.style_log.book.writes == StyleWrites::NEVER {
            store.tmpl[StyleId::DEFAULT.index()] = Some(StyleWord::DEFAULT);
            store.style_log.note(StyleId::DEFAULT);
        }
        // The dynamic colours restart with `Cold`, and the palette does not:
        // what a rendition resolves through may have changed.
        store.style_log.colors_moved();
        // **Every eighth column, and not column zero.** A tab stop at 0 makes
        // `CBT` from column 3 land where the cursor already was.
        for (i, stop) in store.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
        let (cols, rows) = ws.geometry();
        // The pitch admits the provisioned width by construction and not
        // `MAX_COLS`, so the width is bounded by both.
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
            cell_height: ImagePx::ZERO,
            cell_width: ImagePx::ZERO,
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
            links_refused: 0,
            links_clipped: 0,
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
        self.viewport().back(self.looking_back(back).index())
    }

    /// How far back a view asked for `back` lines up really is: bounded by
    /// the history on the primary, and none on the alternate screen. The one
    /// clamp [`Term::view`] and [`Term::image_origin`] share, so the row a
    /// view draws and the row an image draws on cannot disagree -- and
    /// public, because an embedder's scrollbar, cursor and selection have to
    /// agree with that row too: [`Term::scrollback`] still reports the
    /// primary's history while the alternate screen is up, and a window read
    /// against it draws a thumb over a screen with nothing behind it.
    pub fn looking_back(&self, back: Rows) -> Rows {
        match self.cold.screen {
            Screen::Normal => back.min(self.store.book.hist),
            Screen::Alternate => Rows::ZERO,
        }
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

    /// Which surface is live. Each screen is a surface of its own, swapped
    /// into a read's `View` rather than parked, and this says which one is up.
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

    /// Which half of the **live screen's** life this is, from `OSC 133`.
    ///
    /// [`Zone::Output`] until a mark says otherwise, so a stream with no
    /// shell integration reads as output -- which is what it is. Each screen
    /// has its own, so on the alternate screen this is the program's, and a
    /// fresh one on each entry. What decides whether a width change re-cuts
    /// is the primary's: [`Term::primary_zone`].
    pub fn zone(&self) -> Zone {
        self.cold.shell.zone
    }

    /// The **primary's** zone, whichever screen is up: what decides whether
    /// a change of width re-cuts the primary, exposed so an embedder or an
    /// instrument can see which rule a resize is in. It was the live
    /// screen's, so a mark on the alternate screen stopped the primary
    /// re-cutting.
    pub fn primary_zone(&self) -> Zone {
        match self.cold.screen {
            Screen::Normal => self.cold.shell.zone,
            Screen::Alternate => self.cold.shell_other.zone,
        }
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
        if self.cold.shell.zone != Zone::Prompt {
            return None;
        }
        let y = self
            .cold
            .shell
            .block
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

    /// Let the Primary DA advertise Sixel (attribute `4`). Off by default: the
    /// claim is made only when this is `Yes` **and** the image store is
    /// provisioned, because an application that reads it switches from its text
    /// fallback to Sixel, and an embedder may want the fallback.
    pub fn set_sixel_claim(&mut self, c: SixelClaim) {
        self.cold.sixel_claim = c;
    }

    /// The cursor to draw: the style `DECSCUSR` (or the configuration) set,
    /// blinking if that style blinks **or** `?12` is set -- foot's rule, so
    /// `?12l`, which vim sends at start and every `cnorm` sends, cannot stop a
    /// cursor the style blinks. `DECRQSS` answers the style alone and
    /// `DECRQM` the `?12` bit, so each query reports what was asked.
    pub fn cursor_style(&self) -> CursorStyle {
        match self.cold.cursor_blink {
            ModeChange::Set => self.cold.cursor_style.with_blink(true),
            ModeChange::Reset => self.cold.cursor_style,
        }
    }

    /// The cursor style this terminal powers on in, and the one `DECSCUSR 0`
    /// and `7`, `DECSTR` and `RIS` restore -- xterm's resources, which
    /// `DECSCUSR 7` names "initial resources" (`[[source: ctlseqs]]`). Sets the
    /// style in force too, so an embedder calls it once, before the child
    /// writes anything. A blinking block until it is called.
    pub fn set_initial_cursor_style(&mut self, s: CursorStyle) {
        self.cold.initial_cursor = s;
        self.cold.cursor_style = s;
    }

    /// What [`Term::set_initial_cursor_style`] last set.
    pub fn initial_cursor_style(&self) -> CursorStyle {
        self.cold.initial_cursor
    }

    /// Whether a `U+FE0F` after a base `[[source: emoji-variation-sequences]]`
    /// lists makes it two cells wide. The embedder's power-on choice, called
    /// once before the child writes anything; `DECRQM ?2027` answers 3 for
    /// `Two`, the default, and 4 for `One`, and no sequence changes it.
    pub fn set_vs16_width(&mut self, w: Vs16Width) {
        self.cold.vs16 = w;
    }

    /// What [`Term::set_vs16_width`] last set.
    pub fn vs16_width(&self) -> Vs16Width {
        self.cold.vs16
    }

    /// The `[[source: kitty-keyboard]]` enhancement flags in force on the
    /// active screen.
    ///
    /// The negotiation forms maintain this; [`Term::key`] encodes through it
    /// and `CSI ? u` replies with it, `CSI ? flags u`. The protocol detects
    /// support by whether that query is answered, so the reply is the support
    /// claim, made since the encoder honours every enhancement. Exposed so an
    /// embedder and the tests can read the negotiation's state.
    pub fn kitty_flags(&self) -> KittyFlags {
        self.cold.kitty.current()
    }

    /// `?25` DECTCEM. Whether the cursor should be drawn.
    pub fn cursor_visible(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorVisible)
    }

    /// **Where a program put the cursor before it hid it**, while it is
    /// hidden: the position of a hide that is exactly `ESC [ ? 2 5 l` with a
    /// complete `CUP` or `HVP` straight before it. Helix ends every frame so,
    /// drawing its own block cursor at the selection and hiding the
    /// terminal's, and an embedder's cursor trail follows this where the
    /// cursor itself is not shown. It is not a cursor to draw: the program hid
    /// it. The rules, and what forgets it, are `placed.rs`'s.
    ///
    /// **It is the position the move put the cursor at**, which a `CUP` may
    /// put right of a `DECSLRM` right margin -- where [`Term::cursor`], whose
    /// clamp is for the pending-wrap column, reports the margin. A program
    /// that draws its own cursor draws it where the move put it.
    pub fn placed_cursor(&self) -> Option<(Row, Col)> {
        let placed = self.cold.placement.placed();
        debug_assert!(
            placed.is_none() || !self.cursor_visible(),
            "a placement outlived the cursor being shown"
        );
        placed
    }

    /// `?4` DECSCLM. Smooth scroll -- renderer policy, recorded and exposed.
    pub fn smooth_scroll(&self) -> bool {
        self.cold.modes.is_set(ModeBit::SmoothScroll)
    }

    /// `?8` DECARM. Whether a held key repeats: set at power-on, as
    /// `[[source: vt510-decarm]]` gives it, and [`Term::key`] drops a repeat
    /// while it is reset.
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

    /// How many times `?5` has changed outside a synchronised update, as a
    /// reading to compare: see [`ReverseFlips`].
    pub fn reverse_flips(&self) -> ReverseFlips {
        self.cold.flips
    }

    /// How many times `?5` has changed inside a synchronised update. A driver
    /// honouring the update leaves these unseen; one that has stopped
    /// honouring it, because it stayed open too long, draws what they left.
    pub fn reverse_flips_held(&self) -> ReverseFlips {
        self.cold.flips_held
    }

    /// How many times a synchronised update has opened, as a reading to
    /// compare: see [`UpdatesOpened`].
    pub fn updates_opened(&self) -> UpdatesOpened {
        self.cold.updates_opened
    }

    /// `?5` DECSCNM, reverse video. The core records it; `baud-render` draws
    /// it, by xterm's `ReverseVideo` rule (`Video` in its `color.rs`).
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
    /// **Read the slot back rather than trusting the id.** A full table
    /// evicts, so an id can outlive the rendition it was minted for; a
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

    /// Which rendition table this terminal writes: a renderer that caches
    /// what it resolved keeps the serial it resolved against.
    pub fn style_table(&self) -> TableSerial {
        self.store.style_log.book.table
    }

    /// How many writes the rendition table has taken, for a reader to ask
    /// later what changed since ([`Term::styles_written`]).
    pub fn style_writes(&self) -> StyleWrites {
        self.store.style_log.book.writes
    }

    /// **The slots written since a count**, oldest first, as up to two slices
    /// of the log: or [`Written::Every`] when the reader is more than the
    /// table's length behind, and the ring has overwritten what it missed. A
    /// renderer resolves these and nothing else, so a frame in which no
    /// rendition was minted resolves none. From [`StyleWrites::NEVER`] it is
    /// every live slot until the table has filled.
    pub fn styles_written(&self, since: StyleWrites) -> Written<'_> {
        let log = &self.store.style_log;
        match log.book.writes.replay(since, log.order.len()) {
            Replay::Every => Written::Every,
            Replay::Entries([a, b]) => Written::Slots([&log.order[a], &log.order[b]]),
        }
    }

    /// Moves when a colour a rendition resolves through changes: a palette
    /// entry, or the default foreground or background. A renderer resolves
    /// the table again under a new epoch.
    pub fn color_epoch(&self) -> ColorEpoch {
        self.store.style_log.book.colors
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

    /// How the live surface's window moved since a rotation a caller noted:
    /// rows retired off the top, or rows restored -- a resize pulling
    /// history back, or a redraw mark moving the window over its block. A
    /// rotation is modular, so the short way round is the one that
    /// happened. It is the *live* ring's, so across a read that switches
    /// screens it compares two rings: a count that means nothing, and a
    /// subtraction past zero -- a panic in a debug build -- when the
    /// rotation noted is the primary's and the ring now live is the
    /// alternate screen's shorter one. [`Term::screen_epoch`] says whether a
    /// read switched; a window over the history is anchored with
    /// [`Term::history_moved_since`] instead.
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

    /// Where the primary's window stands, whatever screen is up: what a
    /// window over the history is anchored from. [`Term::moved_since`]
    /// measures the *live* surface, which is the alternate screen's ring
    /// while it is up -- so its output would push a scrolled-back window
    /// further up the primary's history, and a read that switches screens
    /// would compare two rings' rotations.
    pub fn history_mark(&self) -> HistoryMark {
        HistoryMark(self.store.book.top)
    }

    /// How the primary's window moved since `before`: rows retired into the
    /// history, or rows taken back from it. Nothing while only the alternate
    /// screen wrote, and what the primary scrolled across a read that
    /// entered or left it -- however many rows, because the line at the
    /// window's top is counted rather than read round a ring. A reflow
    /// renumbers that line with the rows it re-cuts, so across a change of
    /// width this is the renumbering of the window's top, not a count of
    /// rows that scrolled -- which is why a resize anchors a window over the
    /// history with [`Term::resize_holding`] rather than with this.
    pub fn history_moved_since(&self, before: HistoryMark) -> Moved {
        let now = self.store.book.top;
        match now.since(before.0) {
            Some(n) => Moved::Retired(n),
            None => Moved::Restored(before.0.since(now).unwrap_or(Rows::ZERO)),
        }
    }

    /// Which surface is up, and which time: it moves on every switch between
    /// the screens. A read whose two ends carry one epoch saw one surface,
    /// so its live rotations are one ring's; any other read did not.
    pub fn screen_epoch(&self) -> ScreenEpoch {
        self.cold.epoch
    }

    /// Which region scroll last advanced the live ring: it moves on every
    /// scroll of a region smaller than the screen that rotates the ring --
    /// on the primary, sending a line to the history; on the alternate
    /// screen, which keeps none -- and on nothing else. A read whose two ends
    /// carry one reading advanced the ring only with the whole screen, so
    /// every row on the screen moved with it; a read that moved it advanced
    /// through a region, and the rows under that region did not move.
    pub fn region_scrolls(&self) -> RegionScrolls {
        self.cold.region_scrolls
    }

    /// Which erasure of the primary's history this is: it moves on every
    /// `CSI 3 J` and `RIS`, from either screen, and on nothing else. A read
    /// whose two ends carry one reading erased no saved line; a window over
    /// the history anchored across one that moved has no line left to stay
    /// on, and belongs on the live screen.
    pub fn history_erasures(&self) -> HistoryErasures {
        self.cold.history_erasures
    }

    /// The scrolling region DECSTBM set, as its first and last screen row:
    /// the whole screen when none is set. An embedder that holds screen rows
    /// across a read needs it to know which rows a region scroll left where
    /// they were.
    pub fn scroll_region(&self) -> (Row, Row) {
        (self.cold.top, self.cold.bot)
    }

    /// Drop every placement whose row is gone, on either screen: the
    /// primary's outside the rows its ring retains, the alternate's outside
    /// its window. Counted in `ImageStore::placements_expired`.
    fn expire_images(&mut self) {
        let b = &self.store.book;
        let lo = b.top.minus(b.hist).unwrap_or(LineIdx::FIRST);
        let hi = b.top.plus(self.rows).plus(b.hidden);
        let (alt, rows) = (b.alt_top, self.rows);
        let images = &mut self.store.images;
        images.expire(ImageScreen::Primary, lo, hi, None);
        images.expire(ImageScreen::Alternate, alt, alt.plus(rows), None);
    }

    /// **Where a view `back` lines up starts, in the live screen's image
    /// numbering**: the screen, and the absolute line at the view's top row.
    /// A placement on that screen draws at screen row `line - top`; a
    /// placement on the other screen is not drawn -- the shell's image is not
    /// the editor's. `back` is clamped exactly as [`Term::view`] clamps it, so
    /// an image and the grid row it sits on are drawn from one answer, and
    /// the renderer re-derives nothing.
    pub fn image_origin(&self, back: Rows) -> ImageOrigin {
        let book = &self.store.book;
        match self.cold.screen {
            Screen::Normal => ImageOrigin {
                screen: ImageScreen::Primary,
                top: book
                    .top
                    .minus(self.looking_back(back))
                    .unwrap_or(LineIdx::FIRST),
            },
            Screen::Alternate => ImageOrigin {
                screen: ImageScreen::Alternate,
                top: book.alt_top,
            },
        }
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
    /// took `min(cols - 1)` for the same reason, and the cursor differential
    /// against it is what found that this core did not: on a 200-column screen
    /// it returned **column 200** after `HT` ran to the right margin. Both
    /// cores' *internal* `cx` was 200 and they rendered identically, so neither
    /// pinned gate could see it -- the glyph gate read content and the
    /// rendition gate the style word, and the cursor was in neither. This core
    /// clamps to the right margin rather than to `cols - 1`, for the reason the
    /// body gives.
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
    /// index moved when the cell moved. `baud-core2` answered the same question
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

    /// The stored images and their placements, for a renderer to draw and a
    /// test to inspect. A placement carries an image id, an absolute line, a
    /// column and a `z`; the image carries its pixel size and its ARGB pixels.
    /// How many images and placements were refused or dropped are counters on
    /// the store ([`ImageStore::images_refused`] and the rest), and which limit
    /// each image refusal hit is [`ImageStore::refusals`]: the body scratch with
    /// the body's true length, the arena with the image's size, the slot table,
    /// or an image with no pixels. An embedder compares it across a read to say
    /// which limit bit, which the total alone cannot.
    ///
    /// A Sixel arriving with no image memory provisioned ([`Owned::with_images`]
    /// is how a caller provisions it) is refused and counted here, so an empty
    /// store with a non-zero `images_refused` is the honest state of a terminal
    /// that was sent an image it had nowhere to put -- counted against the
    /// body, whose scratch is empty.
    pub fn images(&self) -> &ImageStore<'b> {
        &self.store.images
    }

    /// Tell the core the cell size in device pixels, so it can advance the
    /// cursor past a Sixel by `ceil(image_pixel_height / cell_h)` rows and free
    /// a Sixel when text is written over its `ceil(w / cell_w)` by
    /// `ceil(h / cell_h)` cells. The embedder sets it from the renderer's
    /// metrics and re-sets it whenever the cell size changes -- a zoom, a font
    /// change, a backing-scale change -- because a stale size mis-advances and
    /// mis-frees every image from then on. Zero (the default, and a headless
    /// caller that never sets it) means "do not advance and do not free",
    /// leaving a Sixel where it landed.
    ///
    /// **It also sizes the text area in pixels**, which `CSI 14 t`, `CSI 16 t`
    /// and the `?2048` report answer with -- and a change of it with `?2048`
    /// set owes a report whether or not the grid moved.
    /// `[[source: inband-resize]]`: "Font size changes can also affect the
    /// text area size", and a zoom that keeps the column count still changes
    /// the pixels. An embedder changing both the cell and the grid -- a zoom
    /// usually does -- sends two reports, one for each change; each is the
    /// size the terminal has at that moment, and the specification limits
    /// nothing about how often one is sent.
    pub fn set_cell_size(&mut self, width: ImagePx, height: ImagePx) {
        let before = Report::inband(self.rows, self.cols, self.cell_width, self.cell_height);
        self.cell_width = width;
        self.cell_height = height;
        let after = Report::inband(self.rows, self.cols, width, height);
        if after != before && self.cold.modes.is_set(ModeBit::InbandResize) {
            self.queue(after);
        }
    }

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

    /// What tracking the application asked for: `?9`, `?1000`, `?1002` or
    /// `?1003`, one level at a time, and off after a reset of any of them.
    /// It decides which events `Term::mouse_report` lets through; the
    /// encoding a report is written in has no accessor, because
    /// `mouse_report` reads it.
    pub fn mouse_tracking(&self) -> MouseTracking {
        self.cold.mouse
    }

    /// `?1007`: whether the wheel on the alternate screen, with no tracking
    /// to take it, should send the cursor keys.
    ///
    /// The wheel is the embedder's, so acting on the mode is exposing it --
    /// the argument `?1` and `?2004` make. `[[source: ctlseqs]]`: "if
    /// Alternate Scroll mode is set, then cursor up/down controls are sent
    /// when the terminal is displaying the Alternate Screen Buffer." On at
    /// power-on, which is what baud did before it read the mode.
    pub fn alternate_scroll(&self) -> AlternateScroll {
        self.cold.alternate_scroll
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
    /// Returns whether the report is in the reply buffer, so an embedder can
    /// tell "it was queued" from both kinds of "it was not": the application
    /// does not want it, or the buffer is full. **The second was answered
    /// `true` until 2026-10-09**: `queue` dropped the report whole and counted
    /// it, and this returned `true` anyway -- a 40-line wheel fling under
    /// herdr queued 21 reports, dropped 19, and said all 40 were sent. A
    /// caller with many reports to queue drains [`Term::reply`] between them;
    /// the `false` is how one that did not finds out.
    ///
    /// **Three gates, and each is a refusal rather than an omission.** The
    /// tracking level decides which *kinds* are reported: motion is dropped
    /// under `?1000`, and motion with no button held under anything below
    /// `?1003`, because sending it anyway floods an application that asked
    /// only for clicks; `?9` reports a press of button 1, 2 or 3 and nothing
    /// else; and no level reports a wheel release, which `[[source:
    /// ctlseqs]]` says "are not reported". A move is reported only into a
    /// cell other than the last report's -- "Motion events are reported only
    /// if the mouse pointer has moved to a different character cell" -- so a
    /// `false` also means "the application already has this cell". And the
    /// *position* has to be encodable -- X10 caps each ordinate at 223
    /// against a 2048-column grid and `?1005` at 2015, so a report from
    /// column 400 under X10 has no byte, and the one it would produce names
    /// a **different cell** rather than a truncated one; `?1016` needs the
    /// pixel the embedder may not have given. Those are refused and counted
    /// in [`Term::mouse_unencodable`]; everything that fits is sent, which is
    /// every position on a terminal anyone actually runs.
    ///
    /// **Input-side and cold**: an embedder calls this per pointer event;
    /// the per-byte loop never does.
    pub fn mouse_report(&mut self, e: MouseEvent) -> bool {
        use MouseButton::{Left, Middle, Right, WheelDown, WheelUp};
        let wanted = match (self.cold.mouse, e.kind, e.button) {
            (MouseTracking::Off, ..) => false,
            // "only on button press ... Cb is button-1, where button is 1,
            // 2 or 3": no release, no motion, and no wheel, which xterm drops
            // under `X10_MOUSE` (`EditorButton`, `[[source: xterm-button]]`).
            (MouseTracking::X10Compat, MouseKind::Press, Left | Middle | Right) => true,
            (MouseTracking::X10Compat, ..) => false,
            // "Release events for the wheel buttons are not reported."
            (_, MouseKind::Release, WheelUp | WheelDown) => false,
            (MouseTracking::Normal, MouseKind::Motion, _) => false,
            (MouseTracking::ButtonEvent, MouseKind::Motion, b) => b != MouseButton::None,
            _ => true,
        };
        if !wanted {
            return false;
        }
        // **Motion only into a different cell.** `[[source: ctlseqs]]`,
        // button-event tracking: "Motion events are reported only if the
        // mouse pointer has moved to a different character cell", and any-event
        // is "the same ... except that all motion events are reported, even
        // if no mouse button is down". The app hands this every AppKit move and
        // drag, so a pointer moving inside one cell sent one identical report
        // per event until 2026-10-09. The position alone is compared, not the
        // button or the modifiers, as xterm's `EditorButton` compares its row
        // and column (`[[source: xterm-button]]`): a modifier pressed inside a
        // cell is not a move to another. A press or a release is never held
        // back -- the text gates motion only.
        //
        // **Gated by the cell under `?1016` too**, where xterm gates by the
        // pixel: its row and column hold pixels in `SET_PIXEL_POSITION_MOUSE`
        // and its same-cell test compares those. The sentence above names no
        // encoding, and SGR-Pixels changes only the format -- "Use the same
        // mouse response format as the 1006 control, but report position in
        // pixels rather than character cells" -- so the text's cell stands
        // and xterm's per-pixel motion is a practice baud departs from, left
        // to the owner.
        let at = (e.col, e.row);
        if e.kind == MouseKind::Motion && self.cold.mouse_at == Some(at) {
            return false;
        }
        // X10 compatibility sends the button and no modifiers: normal
        // tracking is the first mode whose text says "Modifier key (shift,
        // ctrl, meta) information is also sent", and xterm writes `' ' +
        // button` for `X10_MOUSE` (`EmitButtonCode`). The encoding still
        // applies, so `?9` with `?1006` is `CSI < 0 ; x ; y M`.
        let e = match self.cold.mouse {
            MouseTracking::X10Compat => MouseEvent {
                mods: MouseMods::NONE,
                ..e
            },
            _ => e,
        };
        let encoding = self.cold.mouse_encoding;
        if let Some(max) = encoding.max_ordinate() {
            let (r, c) = (e.row.one_based().get(), e.col.one_based().get());
            if r > max || c > max {
                self.mouse_unencodable += 1;
                return false;
            }
        }
        let report = match encoding {
            MouseEncoding::X10 => Report::MouseX10(e),
            MouseEncoding::Utf8 => Report::MouseUtf8(e),
            MouseEncoding::Sgr => Report::MouseSgr(e),
            MouseEncoding::Urxvt => Report::MouseUrxvt(e),
            // No pixel, or one at `u16::MAX` -- where `ImagePx` saturates,
            // "at least this far", and whose one-based form the field cannot
            // carry -- is not made up: refused and counted.
            MouseEncoding::SgrPixels => match e.px {
                Some(px) if px.x < ImagePx::of(u16::MAX) && px.y < ImagePx::of(u16::MAX) => {
                    Report::MouseSgrPixels(e, px)
                }
                _ => {
                    self.mouse_unencodable += 1;
                    return false;
                }
            },
        };
        // The cell moves only with a report that went out: a refusal above,
        // or a buffer too full to take this one, told the application
        // nothing, and recording the cell anyway would suppress the next move
        // into it -- the one that would tell it.
        let queued = self.queue(report);
        if queued {
            self.cold.mouse_at = Some(at);
        }
        queued
    }

    /// Queue a focus report, if the application asked for them with `?1004`.
    ///
    /// Returns whether the report is in the reply buffer, the way
    /// [`Term::mouse_report`] does: `false` when the application did not ask,
    /// and `false` when the buffer is full -- which answered `true` until
    /// 2026-10-09. `CSI I` on gain, `CSI O` on loss
    /// (`[[source: ctlseqs]]`, "FocusIn/FocusOut"); nothing when the mode is
    /// reset, because an application that did not ask reads `CSI I` as a
    /// keypress.
    pub fn focus(&mut self, f: Focus) -> bool {
        if !self.cold.modes.is_set(ModeBit::FocusReporting) {
            return false;
        }
        self.queue(Report::Focus(f))
    }

    /// Put a report in the reply buffer, and say whether it went in.
    ///
    /// The same rule `Paint::want_reply` follows: a truncated reply is a wrong
    /// answer, so it is dropped whole and counted rather than half-sent. The
    /// answer is the caller's to pass on: `mouse_report` and `focus` return
    /// it, and the reports that ride on another call -- `?2048` after a
    /// resize or a cell-size change, `?2031` after a theme change -- are
    /// counted and nothing more.
    fn queue(&mut self, r: Report) -> bool {
        let bytes = r.encode(self.cold.c1);
        let b = bytes.as_bytes();
        let at = self.reply_len.index();
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
    /// `None` means "use your own colour": the renderer draws from its own
    /// table, and the copy an embedder hands over with
    /// [`Term::set_default_palette`] exists only to answer a query, so it is
    /// never reported here.
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
    ///
    /// **A change of dark/light class reports**, like [`Term::set_theme`]: one
    /// `CSI ? 997 ; 1|2 n` while `?2031` is set.
    pub fn set_default_colors(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        self.reporting_scheme(|t| t.write_defaults(fg, bg));
    }

    /// Indices 0 and 1 only: the cursor default (index 2) has no theme source
    /// and stays `DEFAULT`, so `OSC 12 ; ?` on a fresh terminal answers
    /// nothing until an application sets one. Reports nothing.
    fn write_defaults(&mut self, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        let word = |(r, g, b): (u8, u8, u8)| {
            ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
        };
        self.cold.defaults[DynamicColor::Foreground.index()] = word(fg);
        self.cold.defaults[DynamicColor::Background.index()] = word(bg);
    }

    /// Run an embedder change and send **at most one** `CSI ? 997 ; 1|2 n`:
    /// the class of what `OSC 10`/`11` answer is taken before and after the
    /// whole change, and a report is queued only when `?2031` is set and the
    /// class after is known and differs. `[[source:
    /// contour-palette-notifications]]` sends the notification "when the
    /// palette has been updated"; comparing across the whole change is what
    /// keeps a drop-then-set from reporting a class nobody could see, or
    /// missing one. Nothing is stored between calls.
    ///
    /// An application's own colour escapes never come through here: they
    /// change the next `CSI ? 996 n` answer and send nothing.
    fn reporting_scheme(&mut self, change: impl FnOnce(&mut Self)) {
        let before = self.cold.color_scheme();
        change(self);
        let after = self.cold.color_scheme();
        if let Some(s) = after {
            if after != before && self.cold.modes.is_set(ModeBit::ColorPaletteUpdates) {
                self.queue(Report::ColorScheme(s));
            }
        }
    }

    /// The embedder's whole colour change in one call: the default foreground
    /// and background, the 256-entry table, and whether the application's
    /// `OSC 4`/`10`/`11`/`12` overrides are dropped with it -- which is what an
    /// OS light/dark flip does under `theme = "auto"` (note Y, decision 5).
    /// **One call, one report at most**; see [`Term::reporting_scheme`].
    pub fn set_theme(&mut self, theme: &ThemeColors<'_>, overrides: Overrides) {
        self.reporting_scheme(|t| {
            t.write_defaults(theme.fg, theme.bg);
            t.set_default_palette(theme.palette);
            if overrides == Overrides::Drop {
                crate::osc::clear_palette(t.store.strs.palette, &mut t.store.style_log);
                crate::osc::clear_dynamic(&mut t.cold, &mut t.store.style_log);
            }
        });
    }

    /// Tell the core the embedder's 256-entry palette, so an `OSC 4 ; c ; ?`
    /// for an entry no application overrode is answered with the colour the
    /// renderer draws, as xterm answers every entry. Call it beside
    /// [`Term::set_default_colors`], and again when the theme changes.
    ///
    /// **It moves no colour epoch**: nothing resolves a rendition through
    /// this copy -- the renderer reads its own table and asks
    /// [`Term::palette`] only for overrides -- so a renderer that re-resolved
    /// on it would repaint for nothing.
    pub fn set_default_palette(&mut self, table: &[(u8, u8, u8); 256]) {
        let own = &mut self.store.strs.palette[crate::storage::EMBEDDER_PALETTE..];
        for (slot, &(r, g, b)) in own.iter_mut().zip(table) {
            *slot = ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b));
        }
    }

    /// The URI a cell's link index resolves to, if any.
    ///
    /// The index comes off the cell -- `CellFlags::link` -- rather than from a
    /// position, which is what makes it survive a scroll: the grid rotates and
    /// the cell carries its link with it. **A cell never resolves to a URI
    /// other than the one written under its index**: a slot is taken once for
    /// the storage's life and a colliding link is refused rather than written
    /// over it (`links_refused`), because a click opens what the slot holds
    /// and a wrong destination is not a wrong colour. `LinkId::NONE` and a
    /// slot nothing took resolve to `None`.
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
    /// **The encoding is `baud_domain::encode_key`, kept in the domain crate
    /// rather than ported in.** What is here is the part that belongs to a terminal
    /// rather than to the protocol: reading the modes off `Term` and putting
    /// the bytes where the replies already go. The alternative was porting 485
    /// lines and having two spellings of the input protocol, which is what a
    /// hand-written CSI recogniser beside the table cost this repo once.
    ///
    /// **One of `KeyState`'s eight fields is a constant here, and it used to be
    /// two.**
    ///
    /// * **`C1Transmit::SevenBit`.** This said the core does not implement
    ///   `S8C1T` and that `want_reply` passes the same constant, so a key and
    ///   a `DA` could not disagree about how wide a `CSI` is. Both stopped
    ///   being true when `S8C1T` landed: every reply follows `cold.c1` and a
    ///   key does not, so after `ESC SP G` a `DA` answers with `9B` and an
    ///   arrow key still sends `ESC [`. `[[source: ctlseqs]]` says xterm
    ///   "generates 7-bit controls (by default) or 8-bit if S8C1T is
    ///   enabled", which reads as covering keys too; whether to follow it is
    ///   an open conformance call (note AG, section 1.3), and the constant
    ///   stays until it is made.
    /// * `terminal` was `TerminalMode::Ansi` on the grounds that "this core
    ///   has no VT52 sub-graph and no DECANM to leave ANSI with". It has both
    ///   now, so it reads the mode. Leaving the constant would have made every
    ///   arrow key send `CSI A` inside VT52, where the keyboard sends
    ///   `ESC A` -- a gap that opened the moment the *parser* gained a feature
    ///   the *encoder* had carried all along.
    pub fn key(&mut self, e: KeyEvent) -> bool {
        // **DECARM reset: a held key does not repeat.** `[[source: vt510-decarm]]`:
        // "Reset: keys do not autorepeat". The embedder delivers what its
        // keyboard does, and the terminal decides what a repeat is worth --
        // nothing here, on every layer, kitty's event types included.
        if e.kind == KeyEventKind::Repeat && !self.cold.modes.is_set(ModeBit::AutoRepeat) {
            return false;
        }
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
