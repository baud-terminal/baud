use crate::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Ground,
    Escape,
    /// After `ESC` and an intermediate byte. `[[source: williams]]`'s
    /// `escape_intermediate`. Without it `ESC # 8` dropped the `#` and printed
    /// the `8`.
    EscapeIntermediate,
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
