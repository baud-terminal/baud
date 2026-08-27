use crate::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Ground,
    Escape,
    /// After `ESC` and an intermediate byte. `[[source: williams]]`'s
    /// `escape_intermediate`. Without it `ESC # 8` dropped the `#` and printed
    /// the `8`.
    EscapeIntermediate,
    /// `[[source: williams]]`'s `csi_entry`: the position *immediately* after
    /// `CSI`, and the only position where a private marker (`3C-3F`) is legal.
    ///
    /// This was folded into [`State::CsiParams`], and the fold is exactly what
    /// the two states disagree about: `csi_entry` collects `3C-3F`, while
    /// `csi_param` sends those same bytes to `csi_ignore`. With one state,
    /// `CSI 2 ? l` was accepted as DECRST and **switched the terminal into VT52
    /// mode** — a protocol change out of a sequence the spec says to discard —
    /// and `CSI 1 ? J` erased the screen as DECSED. Executing what should be
    /// ignored is the dangerous direction, so this is a separate state.
    CsiEntry,
    CsiParams,
    /// `[[source: williams]]`'s `csi_intermediate`. The intermediate is kept,
    /// so `CSI ! p` (DECSTR) is distinguishable from `CSI p`.
    CsiIntermediate,
    /// `[[source: williams]]`'s `csi_ignore`: a malformed sequence is consumed
    /// to its final byte and discarded. Returning to Ground mid-sequence
    /// instead would print the remainder as text.
    CsiIgnore,
    OscString,
    /// An `ESC` arrived inside an OSC payload.
    ///
    /// Its own state because the two continuations differ and both matter:
    /// `ESC \\` is ST and must **dispatch** the payload, while
    /// `[[source: williams]]` says a bare `ESC` *cancels* the control string,
    /// so anything else must discard it and start a fresh sequence. Folding
    /// this into `Escape` loses the payload either way -- which is what the
    /// terminator would have been for.
    OscEsc,
    /// `[[source: williams]]`'s `dcs_entry`: after `ESC P`, collecting
    /// parameters and intermediates before the final byte selects a handler.
    DcsEntry,
    /// `dcs_param` — inside the parameter string.
    DcsParam,
    /// `dcs_intermediate`.
    DcsIntermediate,
    /// `dcs_passthrough` — the final byte named something baud handles, so the
    /// data between it and ST is being collected.
    DcsPassthrough,
    /// `dcs_ignore` — a malformed or unhandled DCS. The payload is consumed to
    /// the terminator and discarded, which is the part that keeps it *safe*:
    /// falling back to Ground mid-string would execute the payload as commands.
    DcsIgnore,
    /// An `ESC` arrived inside a DCS payload. Same two continuations as
    /// [`State::OscEsc`].
    DcsEsc,
    /// Ground, but in VT52.
    ///
    /// A separate variant rather than a flag consulted from `Ground`, and the
    /// reason is measured: the flag was read in the byte loop and cost **4% of
    /// the cells path** (baud/ghostty 0.970 -> 0.931 across three runs,
    /// normalised in-run because the box was thermally drifting). As a state,
    /// the existing `st == Ground` gates exclude VT52 from the ANSI fast paths
    /// for free, and the ANSI path pays nothing at all.
    ///
    /// The trade is that VT52 text takes the byte-at-a-time path. VT52 is a
    /// 1975 terminal reached only by an explicit mode switch; the protocol
    /// everyone actually uses is the one that should be fast.
    Vt52Ground,
    /// `ESC Y` in VT52 mode: waiting for the row byte.
    Vt52Row,
    /// `ESC Y <row>`: waiting for the column byte.
    Vt52Col(Vt52Coord),
    /// The payload of SOS, PM and APC.
    ///
    /// Consumed to the string terminator and executed as nothing. This state
    /// not existing was a **correctness and security** bug, not a missing
    /// feature: those introducers fell through to Ground, so
    /// `ESC _ ... ESC \` had its payload interpreted as commands. A stream
    /// could hide `CSI 2 J` inside an APC string and clear the screen.
    StringIgnore,
    /// Mid multi-byte sequence: continuation bytes remaining, and bits so far.
    Utf8 {
        remaining: Pending,
        acc: Utf8Acc,
    },
}
