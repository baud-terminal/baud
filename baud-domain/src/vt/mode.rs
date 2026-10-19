use crate::*;

/// Whether a mode is being turned on or off.
///
/// SM/RM and DECSET/DECRST all carry this, and `set: bool` at those call sites
/// reads as `if set` -- which says nothing about what the other branch is. The
/// same argument [`Marker`] makes for itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeChange {
    Set,
    Reset,
}

impl ModeChange {
    /// From a flag that is already known, for saving and restoring one.
    pub fn of(on: bool) -> ModeChange {
        match on {
            true => ModeChange::Set,
            false => ModeChange::Reset,
        }
    }

    /// `h` sets, `l` resets. The final byte is the only thing that decides.
    pub fn from_final(f: FinalByte) -> Option<ModeChange> {
        Some(match f.raw() {
            b'h' => ModeChange::Set,
            b'l' => ModeChange::Reset,
            _ => return None,
        })
    }
    pub fn is_set(self) -> bool {
        self == ModeChange::Set
    }
}

/// DECSCA's two states, `[[source: vt510-decsca]]`.
///
/// Named rather than `bool` because the wire has *three* parameter values for
/// two states -- 0 and 2 both mean erasable -- so "the parameter" and "the
/// state" are different things and a `bool` would blur them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Protection {
    /// `Ps = 0` or `Ps = 2`. DECSED and DECSEL may erase this.
    #[default]
    Erasable,
    /// `Ps = 1`. They may not.
    Protected,
}

impl Protection {
    pub fn from_param(p: Option<Param>) -> Protection {
        match p.map_or(0, |x| x.0) {
            1 => Protection::Protected,
            _ => Protection::Erasable,
        }
    }
    pub fn is_protected(self) -> bool {
        self == Protection::Protected
    }
}

/// What DECRQM answers about one mode.
///
/// `[[source: ctlseqs]]`: "0 - not recognized, 1 - set, 2 - reset,
/// 3 - permanently set, 4 - permanently reset".
///
/// `NotRecognized` is the important one and it is not a failure case: an
/// application uses DECRQM precisely to find out whether a mode exists, so
/// answering 0 for a mode baud does not implement is the *correct* answer.
/// Answering 2 ("reset") instead would tell it the mode exists and is off,
/// and it would then set the mode and believe it took effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeState {
    NotRecognized,
    Set,
    Reset,
    PermanentlySet,
    PermanentlyReset,
}

impl ModeState {
    pub(crate) fn wire(self) -> u16 {
        match self {
            ModeState::NotRecognized => 0,
            ModeState::Set => 1,
            ModeState::Reset => 2,
            ModeState::PermanentlySet => 3,
            ModeState::PermanentlyReset => 4,
        }
    }
}

/// ANSI modes, the unprefixed `CSI Pm h` / `CSI Pm l` namespace.
///
/// A different namespace from [`PrivateMode`]: `CSI 4 h` sets insert mode and
/// `CSI ? 4 h` sets DECSCLM (smooth scroll). Conflating them is a classic bug,
/// which is why they are separate types rather than a shared integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnsiMode {
    /// IRM, mode 4. Printing inserts rather than overwrites.
    InsertReplace,
    /// LNM, mode 20. When set, LF also performs a carriage return.
    LineFeedNewLine,
    /// ERM, mode 6, `[[source: ecma48]]` 7.2.4. Reset, PROTECT, the initial
    /// state: "Only the contents of unprotected areas are affected by an
    /// erasure control function". Set, ALL: "The contents of protected as
    /// well as of unprotected areas are affected".
    Erasure,
    /// TSM, mode 18, `[[source: ecma48]]` 7.2.17. baud's character stops are
    /// the same in every line, MULTIPLE, the reset state, and cannot be made
    /// per-line: one state only, which 7.1 allows ("some or all of the modes
    /// may have one state only"). `DECRQM` answers it permanently reset and
    /// SM and RM are counted (Jev 2026-10-19, PERMANENTLY_RESET, 0.98/0.98).
    TabulationStop,
    Other(Param),
}

impl AnsiMode {
    pub fn from_wire(p: Option<Param>) -> AnsiMode {
        match p.map_or(0, |x| x.0) {
            4 => AnsiMode::InsertReplace,
            20 => AnsiMode::LineFeedNewLine,
            6 => AnsiMode::Erasure,
            18 => AnsiMode::TabulationStop,
            n => AnsiMode::Other(Param(n)),
        }
    }
}

/// What ED, EL and ECH do to a guarded position: ERM's two states
/// (`[[source: ecma48]]` 7.2.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardedErasure {
    /// Reset, the initial state: a guarded position is not erased.
    Protect,
    /// Set: it is erased, and stays guarded.
    All,
}

/// One bit of terminal mode state.
///
/// Only the modes baud actually acts on. A mode it parses but does not
/// implement must NOT appear here: a bit that is set and never read is exactly
/// the "accepted, did nothing, no way to find out" failure that
/// `docs/vt-coverage.md` calls `parsed`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeBit {
    /// LNM. Off by default: `[[source: ctlseqs]]` lists mode 20 as reset, so
    /// LF moves down without returning to column 1.
    LineFeedNewLine,
    /// IRM.
    InsertReplace,
    /// DECOM, origin mode: cursor addressing is relative to the scroll region.
    Origin,
    /// DECAWM, autowrap. Set by default.
    AutoWrap,
    /// DECTCEM. Set by default.
    CursorVisible,
    /// DECLRMM, `?69`. When set, `CSI s` is DECSLRM rather than SCOSC.
    SideMargins,
    /// DECSET ?2004. Read by the embedder to decide whether to wrap a paste in
    /// `ESC [ 200 ~` / `ESC [ 201 ~`.
    ///
    /// The core has no input path, so "acting on it" means exposing it --
    /// `Term::bracketed_paste()`. That is not the "set and never read" failure:
    /// an embedder cannot implement the mode without being told, and without
    /// the bit a pasted newline executes as typed input, which is the
    /// paste-jacking class rather than a cosmetic gap.
    BracketedPaste,
    /// DECCKM ?1. Read by an embedder to decide what the arrow keys send.
    CursorKeys,
    /// DECSCLM ?4. A rendering hint: scroll smoothly rather than by a line.
    SmoothScroll,
    /// DECSCNM ?5. Reverse video across the whole screen.
    ReverseVideo,
    /// DECARM ?8. Keyboard auto-repeat.
    AutoRepeat,
    /// ?45. Reverse wraparound: backspace at the left margin moves up a line.
    ReverseWrap,
    /// `?1004` focus reporting. When set, the terminal sends `CSI I` on
    /// gaining focus and `CSI O` on losing it (`[[source: ctlseqs]]`,
    /// "FocusIn/FocusOut"). Input-side, acted on by `Term::focus`; before it
    /// existed the request was dropped **uncounted**, and `vim-unicode.raw`
    /// sends it on its first line.
    FocusReporting,
    /// `?1036` metaSendsEscape. Whether the Meta modifier prefixes `ESC`.
    /// Input-side, read by `Term::key`.
    MetaSendsEscape,
    /// `?1034` eightBitInput. Whether Alt/Meta set the eighth bit instead.
    ///
    /// The other branch of `[[source: ctlseqs]]` line 2337, and the one
    /// terminfo reaches: `smm` and `rmm` are `CSI ? 1034 h` / `l` in every
    /// `xterm*` entry. Acted on only under ISO 8859-1, because adding 128 to a
    /// byte under UTF-8 produces a continuation byte rather than a character --
    /// which is why the `ESC` prefix is the default rather than this.
    EightBitInput,
    /// `?1039` altSendsEscape. Whether the Alt modifier prefixes `ESC`.
    ///
    /// A separate bit from [`ModeBit::MetaSendsEscape`] because
    /// `[[source: ctlseqs]]` line 2337 gives them separate columns and four of
    /// its sixteen rows disagree between them. Collapsing the two would make
    /// `?1039 l` silently turn Meta off as well.
    AltSendsEscape,
    /// `?3` DECCOLM: 132-column mode (`column_mode = ONE_THIRTY_TWO`,
    /// `[[source: dec-std-070]]` p. 5-71). Moved only while
    /// [`ModeBit::AllowColumnMode`] is set; `DECRQM` answers it.
    Column132,
    /// DECSET ?2026. Read by the renderer to hold the frame.
    ///
    /// A *hint*: ignoring it is correct-but-worse, so an embedder that never
    /// reads it still renders correctly, just with tearing.
    SynchronizedOutput,
    /// `?2048` — in-band resize notifications.
    ///
    /// **The point is that it is in band.** `SIGWINCH` arrives from the
    /// kernel on its own schedule, on another thread, with no defined
    /// position in the application's input -- so an application cannot
    /// tell which of its own reads the new size belongs to, and
    /// intermediate sizes coalesce. A report written into the stream has
    /// a position, and the race is gone for anything that reads it.
    /// Neovim asks for this and stops handling `SIGWINCH` once it is on.
    ///
    /// It is not the answer to a *shell's* prompt duplicating on resize,
    /// because no shell consumes it; that is [`Zone`] and `OSC 133`.
    /// `docs/research/notes/m-resize-and-reflow.md` keeps the two apart.
    ///
    /// [`Zone`]: ../../baud_core/struct.Term.html
    InbandResize,
    /// `?2031` -- colour palette update notifications,
    /// `[[source: contour-palette-notifications]]`. While set, a change the
    /// *embedder* makes to the colours that moves their dark/light class sends
    /// `CSI ? 997 ; 1 n` or `; 2 n` unsolicited. Nothing is sent on setting it,
    /// and an application's own colour escapes send nothing: they change the
    /// next `CSI ? 996 n` answer only. Acted on by `Term::set_theme`.
    ColorPaletteUpdates,
    /// `?1042` -- xterm's `bellIsUrgent` ([`BellIsUrgent`]). Off at power-on
    /// and after `RIS`, kept by `DECSTR`. Read by the embedder's bell through
    /// `Term::bell_is_urgent`.
    BellIsUrgent,
    /// `?1043` -- xterm's `popOnBell` ([`PopOnBell`]). Off at power-on and
    /// after `RIS`, kept by `DECSTR`; read through `Term::pop_on_bell`.
    PopOnBell,
    /// `?40` -- xterm's "Allow 80 -> 132 mode" (`[[source: ctlseqs]]`), the
    /// `c132` resource: while reset, `?3` is ignored. Off at power-on, as
    /// `Bres(XtNc132, ..., False)` gives it (`[[source: xterm-reset]]`'s
    /// file); off after `RIS`, kept by `DECSTR`.
    AllowColumnMode,
    /// `?95` DECNCSM, "Set/Reset No Clearing Screen On Column Change"
    /// (`[[source: vt510-decncsm]]`): while set, `?3` does not clear the
    /// screen. Off at power-on and after `RIS`, kept by `DECSTR`.
    NoClearOnColumn,
    /// `?80` DECSDM, Sixel display mode: while set, Sixel scrolling is off --
    /// the image starts at the window's upper-left cell, nothing scrolls,
    /// and the cursor stays where it was ([`PrivateMode::SixelDisplay`]).
    /// Off at power-on and after `RIS`, kept by `DECSTR`, as xterm's
    /// `ReallyReset` restores it only on a full reset
    /// (`[[source: xterm-reset]]`).
    SixelDisplay,
    /// `?1070`, "use private color registers for each graphic"
    /// (`[[source: ctlseqs]]`). **Set at power-on**, xterm's
    /// `privateColorRegisters` resource default (`[[source: xterm-reset]]`);
    /// while reset, the registers a Sixel defines carry into the next one.
    /// Restored by `RIS`, kept by `DECSTR`.
    SixelPrivateRegisters,
    /// `?8452`, "Sixel scrolling leaves cursor to right of graphic"
    /// (`[[source: ctlseqs]]`). Off at power-on and after `RIS`, kept by
    /// `DECSTR`.
    SixelScrollsRight,
    /// ERM, `SM 6`, set: ED, EL and ECH erase a guarded position too
    /// ([`AnsiMode::Erasure`]). Reset at power-on and by `RIS`, as
    /// `[[source: ecma48]]` 7.1 recommends and 8.3.105 has RIS "set the
    /// modes into the reset state"; kept by `DECSTR`, whose DEC table and
    /// xterm's soft reset name no ERM.
    EraseGuarded,
    /// DECECM, `?117`, set: an erase, and a line scrolled or opened onto the
    /// screen, takes the screen background, the default rendition, where
    /// reset it takes the current rendition (`[[source: vt520-rm]]`
    /// p. 5-48, "Set: erase to screen background (VT) ... Reset: erase to
    /// text background (PC)"). Reset at power-on and by `RIS`, the
    /// factory default (Set-Up 2.9.7, p. 2-32); kept by `DECSTR`, whose
    /// table 5-6 does not name it.
    EraseScreenBackground,
}

impl ModeBit {
    const fn bit(self) -> u32 {
        1 << (self as u32)
    }
}

/// `Modes` is a `u32`, so a 33rd mode bit would shift off the end: `1u32 << 32`
/// panics in debug and yields nothing useful in release, and the mode would
/// simply stop working with no diagnostic. Widen `Modes` before adding one.
///
/// It was a `u16` and the seventeenth bit is what widened it -- `?1034`, the
/// mode terminfo's `smm`/`rmm` send. The instruction cost is the same: a bit
/// test against a register-width immediate either way.
///
/// **The bound counts the variants rather than naming the last one.** It
/// named one, `LAST`, for its whole life, and named the wrong one twice:
/// `Column132` while `SynchronizedOutput` followed it, then `AltSendsEscape`
/// with three after it (bit 15 guarded while `InbandResize` sat at 18). A
/// name has to be edited when a variant is appended and nothing says so;
/// `variant_count` cannot go stale, because the variants carry no explicit
/// discriminants and so number `0..count` in declaration order.
const _: () = assert!(
    core::mem::variant_count::<ModeBit>() <= 32,
    "ModeBit has outgrown the u32 bitset in Modes -- widen Modes to u64"
);

/// Terminal mode state as a bitset.
///
/// A struct of `bool` fields would be the obvious representation and is what
/// the audit gate exists to prevent: each would be an untyped flag, and
/// `self.lnm = self.decom` would compile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modes(pub(crate) u32);

impl Default for Modes {
    /// DECAWM and DECTCEM are set on a fresh terminal, and so are both
    /// `ESC`-prefix modes.
    ///
    /// **The prefix defaults are baud's choice, not the specification's.**
    /// `[[source: ctlseqs]]` documents `?1036` and `?1039` and gives the full
    /// truth table at line 2337, but states no default for either resource.
    /// `[[source: foot-meta-defaults]]` does: `meta.esc_prefix = true` in both the
    /// constructor and the reset path. baud follows it, because the
    /// alternative the table offers -- adding 128 -- cannot work under UTF-8,
    /// where a byte above 127 is a continuation byte rather than a character.
    ///
    /// **`?8` DECARM is set at power-on**, as `[[source: vt510-decarm]]`
    /// gives it: "Default: Repeat (set)". It was left reset, so DECRQM told
    /// an application keys do not repeat while the keyboard repeated them.
    ///
    /// **`?1070` is set at power-on**: each Sixel gets registers of its own,
    /// which is also what `[[source: dec-std-070]]` 070-9 §10 describes, "No
    /// Sixel state is preserved from one entering of Sixel mode to the next".
    fn default() -> Modes {
        Modes(
            ModeBit::AutoWrap.bit()
                | ModeBit::CursorVisible.bit()
                | ModeBit::MetaSendsEscape.bit()
                | ModeBit::AltSendsEscape.bit()
                | ModeBit::AutoRepeat.bit()
                | ModeBit::SixelPrivateRegisters.bit(),
        )
    }
}

impl Modes {
    pub fn is_set(self, m: ModeBit) -> bool {
        self.0 & m.bit() != 0
    }
    pub fn set(&mut self, m: ModeBit) {
        self.0 |= m.bit();
    }
    pub fn reset(&mut self, m: ModeBit) {
        self.0 &= !m.bit();
    }
    /// `CSI Pm h` and `CSI Pm l` differ only in this bit, so they share a path.
    pub fn apply(&mut self, m: ModeBit, on: bool) {
        if on { self.set(m) } else { self.reset(m) }
    }
}

/// DEC private modes, named. `?1049` and `?25` are not interchangeable
/// integers — one swaps the screen buffer, the other hides the cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivateMode {
    /// DECOM, ?6. Cursor addressing relative to the margins, and confined
    /// to them.
    Origin,
    /// DECAWM, ?7.
    AutoWrap,
    /// `?1001` — highlight mouse tracking.
    HighlightTracking,
    /// `?1020` — xterm's `utf8` resource, which `[[source: ctlseqs]]` marks
    /// "**(read-only)**".
    ///
    /// Read-only is the whole point: it reports how the terminal decodes and
    /// DECSET cannot change it. `DECRQM` answers it from the live input
    /// encoding: `1` under UTF-8 and `2` under ISO 8859-1, which `ESC % G`
    /// and `ESC % @` select.
    Utf8Resource,
    /// `?1021` — xterm's `cjkWidth` resource, read-only in
    /// `[[source: ctlseqs]]`: whether East Asian Ambiguous characters take
    /// two columns. `DECRQM` answers it `1` or `2` from the embedder's
    /// policy (`Term::set_ambiguous_width`), as xterm answers it with
    /// `MdBool` (`[[source: xterm-misc]]` 5845-5853).
    CjkWidthResource,
    /// `?1022` — xterm's `emojiWidth` resource. baud has no such switch, so
    /// `DECRQM` answers it `0`, not recognized (Jev 2026-10-15): its
    /// `Vs16Width` neither narrows on `U+FE0E` nor widens a bare listed
    /// emoji, which is what the resource reports.
    EmojiWidthResource,
    /// `?1023` — xterm's `privateWidth` resource: whether Private Use Area
    /// codes take one column whatever `?1021` says. baud always keeps them
    /// one column (the owner, 2026-10-15), so `DECRQM` answers it `1`.
    PrivateWidthResource,
    /// `?2027` — grapheme clustering. baud always clusters and has no mode
    /// that stops it, so `DECRQM` answers it permanently: `3` while a
    /// `U+FE0F` widens its base and `4` when the embedder configures one cell.
    /// Not a switch, where [`PrivateMode::Utf8Resource`] answers `1` or `2`
    /// from the live input encoding.
    GraphemeClustering,
    CursorVisible,      // ?25
    BracketedPaste,     // ?2004
    AltScreen,          // ?1049
    SynchronizedOutput, // ?2026
    InbandResize,       // ?2048
    /// `?2031` colour palette update notifications.
    ColorPaletteUpdates,
    /// DECLRMM, `?69`. Left/right margin mode.
    LeftRightMargin,
    /// DECANM, `?2`. **Reset** enters VT52 mode; set returns to ANSI.
    ///
    /// The polarity is the trap: `CSI ? 2 l` is the sequence that *enters* a
    /// different protocol.
    AnsiMode,
    /// DECCKM, `?1`. Cursor keys send application sequences. Input-side.
    CursorKeys,
    /// DECCOLM, `?3`. 132-column mode.
    Column132,
    /// DECSCLM, `?4`. Smooth (slow) scroll. A rendering hint.
    SmoothScroll,
    /// DECSCNM, `?5`. Reverse video across the whole screen.
    ReverseVideo,
    /// DECARM, `?8`. Keyboard auto-repeat. Input-side.
    AutoRepeat,
    /// `?12`. Cursor blink -- the other axis of DECSCUSR.
    CursorBlink,
    /// `?45`. Reverse wraparound: backspace at the left margin wraps up.
    ReverseWrap,
    /// `?47`. The pre-1049 alternate screen: switches buffers but does **not**
    /// save or restore the cursor, which `?1049` added precisely because
    /// applications kept getting it wrong.
    AltScreenLegacy,
    /// `?1036` metaSendsEscape: Meta prefixes `ESC`.
    ///
    /// **Not `?1034`, which this was.** `[[source: ctlseqs]]` line 995 gives
    /// `?1034` as `eightBitInput` -- *set the eighth bit* -- and line 1000
    /// gives `?1036` as `metaSendsEscape`. They are opposite halves of the
    /// same truth table, and baud implemented the second under the first one's
    /// number. Nothing read the bit until `Term::key` did, so it was the
    /// `parsed` failure until an encoder made it a wrong answer.
    MetaSendsEscape,
    /// `?1034` eightBitInput: Alt/Meta add 128 rather than prefixing `ESC`.
    EightBitInput,
    /// `?1039` altSendsEscape: Alt prefixes `ESC`.
    AltSendsEscape,
    /// `?1000` normal tracking.
    MouseNormal,
    /// `?1002` button-event tracking (adds motion while a button is down).
    MouseButtonEvent,
    /// `?1003` any-event tracking (adds motion with no button down).
    MouseAnyEvent,
    /// `?1006` SGR encoding.
    MouseSgr,
    /// `?9` X10 compatibility: a press, its button, and nothing else.
    /// `[[source: ctlseqs]]`: "Send Mouse X & Y on button press ... This is
    /// the X10 xterm mouse protocol." A tracking level, beside `?1000`.
    MouseX10Compat,
    /// `?1005` UTF-8 encoding: X10's bytes, each written as UTF-8 past 127.
    MouseUtf8,
    /// `?1015` urxvt encoding: X10's button value and the position, in
    /// decimal, `CSI Cb ; Cx ; Cy M`.
    MouseUrxvt,
    /// `?1016` SGR-Pixels: `?1006`'s shape with the position in pixels.
    MouseSgrPixels,
    /// `?1007` alternate scroll: the wheel on the alternate screen sends the
    /// cursor keys. Input-side, like `?1`.
    AlternateScroll,
    /// `?1004` FocusIn/FocusOut reporting.
    FocusReporting,
    /// `?1042` -- xterm's `bellIsUrgent`: a `BEL` asks for the user's
    /// attention while the window is not focused ([`BellIsUrgent`]).
    BellIsUrgent,
    /// `?1043` -- xterm's `popOnBell`: a `BEL` raises the window
    /// ([`PopOnBell`]).
    PopOnBell,
    /// `?40` -- "Allow 80 -> 132 mode, xterm" (`[[source: ctlseqs]]`): the
    /// gate on `?3` ([`ModeBit::AllowColumnMode`]).
    AllowColumnMode,
    /// `?95` DECNCSM -- "Do not clear screen when DECCOLM is set/reset"
    /// (`[[source: ctlseqs]]`, `[[source: vt510-decncsm]]`).
    NoClearOnColumn,
    /// `?80` DECSDM, Sixel display mode. **Set turns Sixel scrolling off**:
    /// `[[source: ctlseqs]]` "Enable Sixel Display Mode" for `h` and "Turns
    /// on "Sixel Scrolling"" for `l`, as the VT382 manuals it quotes have
    /// it. `[[source: vt3xx-sixel]]` says the reverse, "When sixel display
    /// mode is set, the Sixel Scrolling feature is enabled"; Jev decided
    /// xterm's numbering on 2026-10-11 (0.90/0.90), a departure from that
    /// manual on the owner's veto list.
    SixelDisplay,
    /// `?1070`, private colour registers for each Sixel ([`ModeBit`]'s
    /// `SixelPrivateRegisters`).
    SixelPrivateRegisters,
    /// `?8452`, the cursor to the right of a Sixel rather than at its
    /// column.
    SixelScrollsRight,
    /// `?117` DECECM, erase colour mode ([`ModeBit`]'s
    /// `EraseScreenBackground`).
    EraseColor,
    Other(Param),
}

/// `?1042`, xterm's `bellIsUrgent` resource. `[[source: ctlseqs]]`: "Enable
/// Urgency window manager hint when Control-G is received, xterm. This
/// enables the bellIsUrgent resource." **Off at power-on**, the resource's
/// default (`Bres(XtNbellIsUrgent, ..., False)`, `[[source: xterm-reset]]`'s
/// file; `[[source: xterm-man]]`, "The default is \"false\"").
///
/// A two-state enum for the reason `baud-domain` is gated at zero `bool`
/// fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BellIsUrgent {
    #[default]
    Off,
    On,
}

/// `?1043`, xterm's `popOnBell` resource. `[[source: ctlseqs]]`: "Enable
/// raising of the window when Control-G is received, xterm. This enables
/// the popOnBell resource." **Off at power-on**, the resource's default
/// (`Bres(XtNpopOnBell, ..., False)`, `[[source: xterm-reset]]`'s file).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PopOnBell {
    #[default]
    Off,
    On,
}

/// The two page widths a program asks for: DEC STD 070's `column_mode`,
/// "Eighty (reset) and One Thirty Two (set)", and the `columns_per_page`
/// each sets (`set_column_mode` and `reset_column_mode`,
/// `[[source: dec-std-070]]` p. 5-72). `?3` asks for one, while `?40`
/// allows it, and DECSCPP asks for one whatever `?40` says.
///
/// **A request, not a geometry.** The window is the embedder's, so the core
/// asks (`Term::requested_width`) and the embedder resizes, as it does for
/// a drag; the core does not change its own width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PageWidth {
    Eighty,
    OneThirtyTwo,
}

impl PageWidth {
    /// The width `?3` asks for: set is 132, reset is 80.
    pub const fn of_column_mode(change: ModeChange) -> PageWidth {
        match change {
            ModeChange::Set => PageWidth::OneThirtyTwo,
            ModeChange::Reset => PageWidth::Eighty,
        }
    }

    /// DECSCPP's `Pn`, by `set_columns_per_page` (`[[source: dec-std-070]]`
    /// p. 5-74): `if (parv[0] <= 80)` the page is 80 columns, else 132. An
    /// omitted parameter is 0, "the page width is defaulted to 80" (p. 5-73),
    /// and a value between is "the next higher supported number", 132.
    /// xterm takes 0, 80 and 132 only and ignores the rest (`CASE_DECSCPP`,
    /// `[[source: xterm-reset]]`'s file), a departure not followed.
    pub fn per_page(p: Option<Param>) -> PageWidth {
        match p.map_or(0, |x| x.0) {
            0..=80 => PageWidth::Eighty,
            _ => PageWidth::OneThirtyTwo,
        }
    }

    /// The page a grid `cols` wide is, by the same rule: up to 80 columns
    /// is 80, and wider "the next higher supported number", 132. What
    /// `DECRQSS` answers for DECSCPP.
    pub fn page_of(cols: Cols) -> PageWidth {
        match cols.0 {
            0..=80 => PageWidth::Eighty,
            _ => PageWidth::OneThirtyTwo,
        }
    }

    /// The columns, as a `WinSize` counts them.
    pub const fn ws_col(self) -> u16 {
        match self {
            PageWidth::Eighty => 80,
            PageWidth::OneThirtyTwo => 132,
        }
    }
}

/// A width the core asks the embedder for (`Term::requested_width`): one of
/// the two pages, or the window's own width, which `RIS` asks back for --
/// the width the user gave it, as STD 070's `host_state_init` takes the
/// page width from NVM. **Kept apart from [`PageWidth`]**, which DECSCPP's
/// `Csi` carries: a `Cols` there would widen every `Csi` the dispatch
/// builds, from 12 bytes to 24.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WidthRequest {
    Page(PageWidth),
    /// A width that is neither page: the window's before a program asked.
    Window(Cols),
}

impl WidthRequest {
    /// A window `cols` wide, as a request: 80 and 132 are the two pages, so
    /// that a request compares equal whichever control made it.
    pub fn of_window(cols: Cols) -> WidthRequest {
        match cols.0 {
            80 => WidthRequest::Page(PageWidth::Eighty),
            132 => WidthRequest::Page(PageWidth::OneThirtyTwo),
            _ => WidthRequest::Window(cols),
        }
    }

    /// The columns, as a `WinSize` counts them.
    pub fn ws_col(self) -> u16 {
        match self {
            WidthRequest::Page(w) => w.ws_col(),
            WidthRequest::Window(c) => u16::try_from(c.0).unwrap_or(u16::MAX),
        }
    }
}

/// Whether a read stops at a page-width request, so the embedder can resize
/// before the bytes after it are parsed (`Term::set_width_hold`). **Off by
/// default**: an embedder that never resizes, or never resumes, is fed every
/// byte of a read as before, at the width it has.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WidthHold {
    #[default]
    Off,
    On,
}

/// Where XTSAVE keeps one mode's value in [`SavedModes`]: one per mode
/// DECSET acts on. A slot, not the wire number, so the cache is two words
/// rather than a table over every value a parameter can carry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeSlot(u8);

impl ModeSlot {
    const fn bit(self) -> u64 {
        1 << self.0
    }
}

/// XTSAVE's one-level cache, `CSI ? Pm s`, which XTRESTORE `CSI ? Pm r` reads
/// back (`[[source: ctlseqs]]`): "Like Save Cursor (DECSC), this uses a
/// one-level cache. Unlike Save Cursor, specific settings can be saved and
/// restored independently."
///
/// **Which slots were saved, and the value each had**, two words rather than
/// one: "The value of Ps previously saved is restored" presupposes a save, so
/// a slot never saved must read as nothing to restore rather than as reset.
/// xterm keeps one array that `VTInitialize` zeroes (`memset(screen, 0, ...)`,
/// `[[source: xterm-reset]]`), so its restore of a never-saved mode resets it
/// -- `?25` hides the cursor -- which is a difference from the text, recorded
/// as the owner's question rather than followed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SavedModes {
    saved: u64,
    set: u64,
}

impl SavedModes {
    /// Nothing saved: the power-on cache, and what `RIS` returns it to.
    pub const NONE: SavedModes = SavedModes { saved: 0, set: 0 };

    /// Keep `value` for `slot`, replacing whatever was kept: one level.
    pub fn save(&mut self, slot: ModeSlot, value: ModeChange) {
        self.saved |= slot.bit();
        match value {
            ModeChange::Set => self.set |= slot.bit(),
            ModeChange::Reset => self.set &= !slot.bit(),
        }
    }

    /// The value kept for `slot`, or `None` if it was never saved. Reading it
    /// leaves it: a restore does not consume the save, as DECRC does not.
    pub fn saved(self, slot: ModeSlot) -> Option<ModeChange> {
        match (self.saved & slot.bit() != 0, self.set & slot.bit() != 0) {
            (false, _) => None,
            (true, on) => Some(ModeChange::of(on)),
        }
    }
}

impl PrivateMode {
    /// The slot XTSAVE keeps this mode in: **every mode DECSET acts on, and
    /// no other** -- "Ps values are the same as for DECSET". The two named
    /// modes without one are the five DECSET cannot change: `?1020` to
    /// `?1023` are read-only (`[[source: ctlseqs]]` marks them so, and
    /// xterm's `savemodes` keeps none of them) and `?2027` is permanent
    /// here. An unnamed mode has none, and DECSET counts it, as a
    /// save and a restore of it are counted.
    ///
    /// xterm saves a different set (`savemodes`, `[[source: xterm-reset]]`):
    /// not `?2` or `?8`, and `?3` only under the `c132` resource `?40` sets,
    /// where baud saves `?3` whatever `?40` says, decided 2026-10-10 (Jev,
    /// XT-3) -- a restore of it acts only under `?40`, as DECSET does; and one
    /// slot for all five tracking levels and one for all four encodings,
    /// still the owner's question (XT-4, XT-5).
    pub const fn slot(self) -> Option<ModeSlot> {
        let n = match self {
            PrivateMode::CursorKeys => 0,
            PrivateMode::AnsiMode => 1,
            PrivateMode::Column132 => 2,
            PrivateMode::SmoothScroll => 3,
            PrivateMode::ReverseVideo => 4,
            PrivateMode::Origin => 5,
            PrivateMode::AutoWrap => 6,
            PrivateMode::AutoRepeat => 7,
            PrivateMode::MouseX10Compat => 8,
            PrivateMode::CursorBlink => 9,
            PrivateMode::CursorVisible => 10,
            PrivateMode::ReverseWrap => 11,
            PrivateMode::AltScreenLegacy => 12,
            PrivateMode::LeftRightMargin => 13,
            PrivateMode::MouseNormal => 14,
            PrivateMode::HighlightTracking => 15,
            PrivateMode::MouseButtonEvent => 16,
            PrivateMode::MouseAnyEvent => 17,
            PrivateMode::FocusReporting => 18,
            PrivateMode::MouseUtf8 => 19,
            PrivateMode::MouseSgr => 20,
            PrivateMode::AlternateScroll => 21,
            PrivateMode::MouseUrxvt => 22,
            PrivateMode::MouseSgrPixels => 23,
            PrivateMode::EightBitInput => 24,
            PrivateMode::MetaSendsEscape => 25,
            PrivateMode::AltSendsEscape => 26,
            PrivateMode::AltScreen => 27,
            PrivateMode::BracketedPaste => 28,
            PrivateMode::SynchronizedOutput => 29,
            PrivateMode::ColorPaletteUpdates => 30,
            PrivateMode::InbandResize => 31,
            PrivateMode::BellIsUrgent => 32,
            PrivateMode::PopOnBell => 33,
            PrivateMode::AllowColumnMode => 34,
            PrivateMode::NoClearOnColumn => 35,
            PrivateMode::SixelDisplay => 36,
            PrivateMode::SixelPrivateRegisters => 37,
            PrivateMode::SixelScrollsRight => 38,
            PrivateMode::EraseColor => 39,
            PrivateMode::Utf8Resource
            | PrivateMode::CjkWidthResource
            | PrivateMode::EmojiWidthResource
            | PrivateMode::PrivateWidthResource
            | PrivateMode::GraphemeClustering
            | PrivateMode::Other(_) => {
                return None;
            }
        };
        Some(ModeSlot(n))
    }

    pub fn from_wire(p: Option<Param>) -> PrivateMode {
        match p.map_or(0, |x| x.0) {
            6 => PrivateMode::Origin,
            7 => PrivateMode::AutoWrap,
            25 => PrivateMode::CursorVisible,
            1 => PrivateMode::CursorKeys,
            2 => PrivateMode::AnsiMode,
            3 => PrivateMode::Column132,
            4 => PrivateMode::SmoothScroll,
            5 => PrivateMode::ReverseVideo,
            8 => PrivateMode::AutoRepeat,
            12 => PrivateMode::CursorBlink,
            40 => PrivateMode::AllowColumnMode,
            45 => PrivateMode::ReverseWrap,
            47 => PrivateMode::AltScreenLegacy,
            69 => PrivateMode::LeftRightMargin,
            80 => PrivateMode::SixelDisplay,
            95 => PrivateMode::NoClearOnColumn,
            1070 => PrivateMode::SixelPrivateRegisters,
            8452 => PrivateMode::SixelScrollsRight,
            117 => PrivateMode::EraseColor,
            1034 => PrivateMode::EightBitInput,
            1036 => PrivateMode::MetaSendsEscape,
            1039 => PrivateMode::AltSendsEscape,
            1000 => PrivateMode::MouseNormal,
            1002 => PrivateMode::MouseButtonEvent,
            1003 => PrivateMode::MouseAnyEvent,
            1006 => PrivateMode::MouseSgr,
            9 => PrivateMode::MouseX10Compat,
            1005 => PrivateMode::MouseUtf8,
            1015 => PrivateMode::MouseUrxvt,
            1016 => PrivateMode::MouseSgrPixels,
            1007 => PrivateMode::AlternateScroll,
            1004 => PrivateMode::FocusReporting,
            1042 => PrivateMode::BellIsUrgent,
            1043 => PrivateMode::PopOnBell,
            1049 => PrivateMode::AltScreen,
            1001 => PrivateMode::HighlightTracking,
            1020 => PrivateMode::Utf8Resource,
            1021 => PrivateMode::CjkWidthResource,
            1022 => PrivateMode::EmojiWidthResource,
            1023 => PrivateMode::PrivateWidthResource,
            2027 => PrivateMode::GraphemeClustering,
            2004 => PrivateMode::BracketedPaste,
            2026 => PrivateMode::SynchronizedOutput,
            2048 => PrivateMode::InbandResize,
            2031 => PrivateMode::ColorPaletteUpdates,
            n => PrivateMode::Other(Param(n)),
        }
    }
}
