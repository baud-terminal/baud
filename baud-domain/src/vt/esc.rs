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
    /// VTS — Line Tabulation Set, `ESC J` (`8A` as a C1). `[[source:
    /// ecma48]]` 8.3.162: "VTS causes a line tabulation stop to be set at
    /// the active line (the line that contains the active presentation
    /// position)". VT52 mode never reaches this table: its `ESC J` erases.
    LineTabSet,
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
    /// 3 or above"; baud's DA names a VT100 by default, and a VT102 under
    /// the Sixel claim, both of which STD 070 places at Level 1, so note
    /// b's Level 3 bar is not reached.
    /// Note a of the same entry records the conflict with ECMA-48: "THIS
    /// CONTROL IS RESERVED FOR FUTURE STANDARDIZATION BY ISO AND ANSI.
    /// Conforming software shall use the primary Device Attributes (DA)
    /// control". In `[[source: ecma48]]` ESC Z / 09/10 is 8.3.109 SCI, whose
    /// use "is reserved for future standardization", so DECID takes a
    /// representation ECMA-48 defines no behaviour for.
    /// VT52 mode never reaches this table: its `ESC Z` is VT52's identify.
    Identify,
    /// SPA -- Start of Guarded Area, `ESC V` (`96` as a C1).
    ///
    /// `[[source: ecma48]]` 8.3.129: "SPA is used to indicate that the active
    /// presentation position is the first of a string of character positions
    /// in the presentation component, the contents of which are protected
    /// against manual alteration, are guarded against transmission or
    /// transfer, depending on the setting of the GUARDED AREA TRANSFER MODE
    /// (GATM) and may be protected against erasure, depending on the setting
    /// of the ERASURE MODE (ERM). The end of this string is indicated by END
    /// OF GUARDED AREA (EPA)."
    StartGuardedArea,
    /// EPA -- End of Guarded Area, `ESC W` (`97` as a C1).
    ///
    /// `[[source: ecma48]]` 8.3.46: "EPA is used to indicate that the active
    /// presentation position is the last of a string of character positions
    /// in the presentation component" whose beginning SPA indicates.
    EndGuardedArea,
    /// SSA -- Start of Selected Area, `ESC F` (`86` as a C1).
    ///
    /// `[[source: ecma48]]` 8.3.138: "SSA is used to indicate that the
    /// active presentation position is the first of a string of character
    /// positions in the presentation component, the contents of which are
    /// eligible to be transmitted in the form of a data stream or
    /// transferred to an auxiliary input/output device. The end of this
    /// string is indicated by END OF SELECTED AREA (ESA)." Only ECMA-48's
    /// meaning: HP's cursor to the lower left corner, which xterm keeps
    /// behind its `hpLowerleftBugCompat` resource (`[[source: ctlseqs]]`),
    /// is not this. VT52 mode never reaches this table: its `ESC F` enters
    /// graphics mode.
    StartSelectedArea,
    /// ESA -- End of Selected Area, `ESC G` (`87` as a C1).
    ///
    /// 8.3.47: "ESA is used to indicate that the active presentation
    /// position is the last of a string of character positions" whose
    /// beginning SSA indicates.
    EndSelectedArea,
    /// HTJ -- Character Tabulation with Justification, `ESC I` (`89` as a
    /// C1; `[[source: ecma48]]` 8.3.61): "the contents of the active field
    /// ... shifted forward so that it ends at the character position
    /// preceding the following character tabulation stop", and the active
    /// position "moved to that following character tabulation stop".
    TabJustify,

    /// DECBI -- Back Index, `ESC 6`. Back one column, and at the left margin
    /// the rectangle the margins bound moves right one column instead
    /// (`[[source: dec-std-070]]` p. 5-39).
    BackIndex,
    /// DECFI -- Forward Index, `ESC 9`. Forward one column, and at the right
    /// margin the rectangle moves left one column instead (p. 5-37).
    ForwardIndex,
    /// SS2 -- Single Shift Two, `ESC N` (`8E` as a C1): the next graphic
    /// character from G2. `[[source: ecma35]]` 8.4: "the immediately
    /// following bit combination ... shall represent the corresponding
    /// character from the set that is designated as the code element G2";
    /// `[[source: dec-std-070]]` p. 3-80 gives the receiver.
    SingleShift2,
    /// SS3 -- Single Shift Three, `ESC O` (`8F`), the same from G3.
    SingleShift3,
    /// LS2 -- Locking Shift Two, `ESC n`: G2 into GL (`[[source: ecma35]]`
    /// 8.3.1; `[[source: ctlseqs]]`, "Invoke the G2 Character Set as GL").
    LockingShift2,
    /// LS3 -- Locking Shift Three, `ESC o`: G3 into GL.
    LockingShift3,
    /// LS1R -- Locking Shift One Right, `ESC ~`: G1 into GR
    /// (`[[source: ecma35]]` 8.3.2).
    LockingShift1Right,
    /// LS2R -- Locking Shift Two Right, `ESC }`: G2 into GR.
    LockingShift2Right,
    /// LS3R -- Locking Shift Three Right, `ESC |`: G3 into GR.
    LockingShift3Right,
    /// HP's memory lock, `ESC l`, done as xterm does it: the top margin
    /// moves to the cursor's line, so the lines above it stay while later
    /// output scrolls beneath them. `[[source: ctlseqs]]`: "Memory Lock
    /// (per HP terminals). Locks memory above the cursor"; xterm's
    /// `CASE_HP_MEM_LOCK` calls `set_tb_margins(screen, screen->cur_row,
    /// screen->bot_marg)` (`[[source: xterm-reset]]` 4759-4770).
    MemoryLock,
    /// HP's memory unlock, `ESC m`: the top margin back to the first line,
    /// the bottom margin kept, xterm's `set_tb_margins(screen, 0,
    /// screen->bot_marg)`.
    MemoryUnlock,
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
            b'J' => Esc::LineTabSet,
            b'\\' => Esc::StringTerminator,
            b'Z' => Esc::Identify,
            b'V' => Esc::StartGuardedArea,
            b'W' => Esc::EndGuardedArea,
            b'F' => Esc::StartSelectedArea,
            b'G' => Esc::EndSelectedArea,
            b'I' => Esc::TabJustify,

            b'6' => Esc::BackIndex,
            b'9' => Esc::ForwardIndex,
            b'N' => Esc::SingleShift2,
            b'O' => Esc::SingleShift3,
            b'n' => Esc::LockingShift2,
            b'o' => Esc::LockingShift3,
            b'~' => Esc::LockingShift1Right,
            b'}' => Esc::LockingShift2Right,
            b'|' => Esc::LockingShift3Right,
            b'l' => Esc::MemoryLock,
            b'm' => Esc::MemoryUnlock,
            _ => Esc::Unsupported,
        }
    }
}
