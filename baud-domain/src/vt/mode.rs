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
    Other(Param),
}

impl AnsiMode {
    pub fn from_wire(p: Option<Param>) -> AnsiMode {
        match p.map_or(0, |x| x.0) {
            4 => AnsiMode::InsertReplace,
            20 => AnsiMode::LineFeedNewLine,
            n => AnsiMode::Other(Param(n)),
        }
    }
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
    /// ?1034. Meta sets the high bit / sends ESC. Input-side.
    MetaSendsEscape,
    /// ?3. 132-column mode requested.
    Column132,
    /// DECSET ?2026. Read by the renderer to hold the frame.
    ///
    /// A *hint*: ignoring it is correct-but-worse, so an embedder that never
    /// reads it still renders correctly, just with tearing.
    SynchronizedOutput,
}

impl ModeBit {
    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
    /// The highest-numbered variant. Update when adding one -- the assertion
    /// below is what makes forgetting a compile error.
    const LAST: ModeBit = ModeBit::Column132;
}

/// `Modes` is a `u16`, so a 17th mode bit would shift off the end: `1u16 << 16`
/// panics in debug and yields nothing useful in release, and the mode would
/// simply stop working with no diagnostic. Widen `Modes` before adding one.
const _: () = assert!(
    (ModeBit::LAST as u16) < 16,
    "ModeBit has outgrown the u16 bitset in Modes -- widen Modes to u32"
);

/// Terminal mode state as a bitset.
///
/// A struct of `bool` fields would be the obvious representation and is what
/// the audit gate exists to prevent: each would be an untyped flag, and
/// `self.lnm = self.decom` would compile.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Modes(pub(crate) u16);

impl Default for Modes {
    /// DECAWM and DECTCEM are set on a fresh terminal; everything else reset.
    fn default() -> Modes {
        Modes(ModeBit::AutoWrap.bit() | ModeBit::CursorVisible.bit())
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
    /// `?1020` — xterm's `utf8` resource, which `[[source: ctlseqs]]` marks
    /// "**(read-only)**".
    ///
    /// Read-only is the whole point: it reports how the terminal decodes and
    /// cannot be used to change it. baud decodes UTF-8 and nothing else, by
    /// construction, so the answer is fixed for the life of the process.
    Utf8Resource,
    /// `?2027` — grapheme clustering. baud always clusters, so like
    /// [`PrivateMode::Utf8Resource`] this is permanent rather than a switch.
    GraphemeClustering,
    CursorVisible,      // ?25
    BracketedPaste,     // ?2004
    AltScreen,          // ?1049
    SynchronizedOutput, // ?2026
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
    /// `?1034`. Meta sends escape. Input-side.
    MetaSendsEscape,
    /// `?1000` normal tracking.
    MouseNormal,
    /// `?1002` button-event tracking (adds motion while a button is down).
    MouseButtonEvent,
    /// `?1003` any-event tracking (adds motion with no button down).
    MouseAnyEvent,
    /// `?1006` SGR encoding.
    MouseSgr,
    Other(Param),
}

impl PrivateMode {
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
            45 => PrivateMode::ReverseWrap,
            47 => PrivateMode::AltScreenLegacy,
            69 => PrivateMode::LeftRightMargin,
            1034 => PrivateMode::MetaSendsEscape,
            1000 => PrivateMode::MouseNormal,
            1002 => PrivateMode::MouseButtonEvent,
            1003 => PrivateMode::MouseAnyEvent,
            1006 => PrivateMode::MouseSgr,
            1049 => PrivateMode::AltScreen,
            1020 => PrivateMode::Utf8Resource,
            2027 => PrivateMode::GraphemeClustering,
            2004 => PrivateMode::BracketedPaste,
            2026 => PrivateMode::SynchronizedOutput,
            n => PrivateMode::Other(Param(n)),
        }
    }
}
