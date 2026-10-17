use crate::*;

/// An `ESC Fe` sequence, resolved -- the two-byte escapes that carry no
/// parameters.
///
/// The counterpart to [`Csi`], and it exists for the same reason. This dispatch
/// used to be a `match` on `b'7'`, `b'8'`, `b'M'`, `b'D'`, `b'E'`, `b'c'` and
/// `b'H'`, which are not the names of anything -- they are the bytes those
/// operations happen to be spelled with. Every one has a name in the standard,
/// and a reader should not have to remember that `ESC M` scrolls *down*
/// (RI, reverse index) while `ESC D` scrolls up.
///
/// `Unsupported` is a variant rather than a `_ => {}` arm, on the same
/// principle as [`Csi`]: what is not implemented stays enumerable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Esc {
    /// DECSC — Save Cursor, `ESC 7`.
    ///
    /// `[[source: vt510-decsc]]`: saves the cursor position, the SGR
    /// attributes, the character sets, the wrap flag, the state of DECOM, and
    /// the selective erase attribute.
    SaveCursor,
    /// DECRC — Restore Cursor, `ESC 8`. The inverse of [`Esc::SaveCursor`].
    RestoreCursor,
    /// RI — Reverse Index, `ESC M`. Up one line, scrolling the region *down* at
    /// the top margin.
    ReverseIndex,
    /// IND — Index, `ESC D`. Down one line, keeping the column.
    Index,
    /// NEL — Next Line, `ESC E`. Down one line *and* to column 1, which is what
    /// distinguishes it from [`Esc::Index`].
    NextLine,
    /// RIS — Reset to Initial State, `ESC c`. A full reset, not a soft one.
    Reset,
    /// DECKPAM — Application Keypad, `ESC =`. `[[source: ctlseqs]]`:
    /// "ESC = Application Keypad (DECKPAM)."
    KeypadApplication,
    /// DECKPNM — Normal Keypad, `ESC >`, VT100.
    KeypadNumeric,
    /// HTS — Horizontal Tab Set, `ESC H`. Sets a tab stop at the cursor column.
    TabSet,
    /// ST — String Terminator, `ESC \`, with no control string open.
    ///
    /// `[[source: ecma48]]` § 8.3.143: "ST is used as the closing delimiter of
    /// a control string opened by" APC, DCS, OSC, PM or SOS, and that is all it
    /// is. The ST that closes one never arrives here, because the string's own
    /// node takes the `\` after its `ESC`, or the `9C`. One that arrives here
    /// closes nothing, so it has no function to perform and none is missing:
    /// `[[source: williams]]` draws `9C / (no action)` (line 268). Named, so it
    /// does not fall to [`Esc::Unsupported`] and get counted.
    StringTerminator,
    /// DECID -- Identify Terminal, `ESC Z` (`9A` as a C1).
    ///
    /// `[[source: dec-std-070]]` 070-4 (PDF 232): "This control is
    /// identical to a primary DA control with an omitted parameter or a
    /// parameter value of zero (0)", and "DECID is not recognized at Level
    /// 3 or above"; baud's DA names a VT102, which STD 070 places at Level 1,
    /// so note b's Level 3 bar is not reached.
    /// Note a of the same entry records the conflict with ECMA-48: "THIS
    /// CONTROL IS RESERVED FOR FUTURE STANDARDIZATION BY ISO AND ANSI.
    /// Conforming software shall use the primary Device Attributes (DA)
    /// control". In `[[source: ecma48]]` ESC Z / 09/10 is 8.3.109 SCI, whose
    /// use "is reserved for future standardization", so DECID takes a
    /// representation ECMA-48 defines no behaviour for.
    /// VT52 mode never reaches this table: its `ESC Z` is VT52's identify.
    Identify,
    /// Recognised as an `ESC Fe` and deliberately not acted on.
    Unsupported,
}

impl Esc {
    pub fn parse(b: StreamByte) -> Esc {
        match b.raw_byte() {
            b'7' => Esc::SaveCursor,
            b'8' => Esc::RestoreCursor,
            b'M' => Esc::ReverseIndex,
            b'D' => Esc::Index,
            b'E' => Esc::NextLine,
            b'c' => Esc::Reset,
            b'=' => Esc::KeypadApplication,
            b'>' => Esc::KeypadNumeric,
            b'H' => Esc::TabSet,
            b'\\' => Esc::StringTerminator,
            b'Z' => Esc::Identify,
            _ => Esc::Unsupported,
        }
    }
}
