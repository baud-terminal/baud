use crate::*;

/// What the numeric keypad sends. DECKPAM / DECKPNM.
///
/// `[[source: ctlseqs]]`: "ESC = Application Keypad (DECKPAM)" and
/// "ESC > Normal Keypad (DECKPNM), VT100."
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeypadMode {
    /// Digits and operators, as printed on the keys.
    #[default]
    Numeric,
    /// `SS3`-prefixed sequences, so an application can tell keypad `5` from the
    /// `5` on the top row.
    Application,
}

/// A character set designated into one of the G0-G3 slots.
///
/// `[[source: ctlseqs]]` lists the final characters for `ESC ( C` and friends.
/// baud distinguishes only the two that change behaviour here -- ASCII and the
/// line-drawing set -- and keeps the rest as `Other` so that designating one is
/// *recorded* rather than mistaken for ASCII.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Charset {
    /// `B` (USASCII), and `A` (British Latin-1), whose difference is one glyph
    /// baud does not special-case.
    #[default]
    Ascii,
    /// `0` — "DEC Special Character and Line Drawing Set, VT100."
    DecGraphics,
    /// Designated, recognised, and not translated. See [`CellFlags::GRAPHICS`]
    /// for why translation needs a source baud does not have.
    Other,
}

impl Charset {
    pub fn from_final(b: StreamByte) -> Charset {
        match b.raw_byte() {
            b'0' => Charset::DecGraphics,
            b'B' | b'A' => Charset::Ascii,
            _ => Charset::Other,
        }
    }
}

/// Which G-slot an `ESC ( ) * +` designates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GSlot {
    G0,
    G1,
    G2,
    G3,
}

impl GSlot {
    /// The intermediate byte that selects the slot: `(` is G0, `)` G1, `*` G2,
    /// `+` G3.
    pub fn from_intermediate(b: StreamByte) -> Option<GSlot> {
        Some(match b.raw_byte() {
            b'(' => GSlot::G0,
            b')' => GSlot::G1,
            b'*' => GSlot::G2,
            b'+' => GSlot::G3,
            _ => return None,
        })
    }
    pub fn index(self) -> usize {
        self as usize
    }
}

/// DECSCUSR cursor shapes, `CSI Ps SP q`.
///
/// `[[source: ctlseqs]]`: "0 -> blinking block. 1 -> blinking block (default).
/// 2 -> steady block. 3 -> blinking underline. 4 -> steady underline.
/// 5 -> blinking bar, xterm. 6 -> steady bar, xterm."
///
/// Shape and blink are kept as one value rather than split into a shape plus a
/// flag, because the wire has seven states and only six are a clean product --
/// 0 and 1 are the same shape, so a split would have to invent a rule for it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorStyle {
    #[default]
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

impl CursorStyle {
    /// `Ps = 7` ("initial resources") means the power-on style, which for baud
    /// is the default -- there are no resources to restore from.
    pub fn from_param(p: Option<Param>) -> Option<CursorStyle> {
        Some(match p.map_or(0, |x| x.0) {
            0 | 1 | 7 => CursorStyle::BlinkingBlock,
            2 => CursorStyle::SteadyBlock,
            3 => CursorStyle::BlinkingUnderline,
            4 => CursorStyle::SteadyUnderline,
            5 => CursorStyle::BlinkingBar,
            6 => CursorStyle::SteadyBar,
            _ => return None,
        })
    }
    /// Does this style blink? A renderer needs the two axes even though the
    /// wire encodes them together.
    /// The `Ps` that selects this style, for reporting it back via DECRQSS.
    pub fn wire(self) -> u8 {
        match self {
            CursorStyle::BlinkingBlock => 1,
            CursorStyle::SteadyBlock => 2,
            CursorStyle::BlinkingUnderline => 3,
            CursorStyle::SteadyUnderline => 4,
            CursorStyle::BlinkingBar => 5,
            CursorStyle::SteadyBar => 6,
        }
    }
    /// The same shape with blinking turned on or off.
    ///
    /// `?12` reaches the blink axis by a different route from DECSCUSR, and
    /// must not change the *shape* on the way — which a plain assignment would.
    pub fn with_blink(self, on: bool) -> CursorStyle {
        use CursorStyle::*;
        match (self, on) {
            (BlinkingBlock | SteadyBlock, true) => BlinkingBlock,
            (BlinkingBlock | SteadyBlock, false) => SteadyBlock,
            (BlinkingUnderline | SteadyUnderline, true) => BlinkingUnderline,
            (BlinkingUnderline | SteadyUnderline, false) => SteadyUnderline,
            (BlinkingBar | SteadyBar, true) => BlinkingBar,
            (BlinkingBar | SteadyBar, false) => SteadyBar,
        }
    }
    pub fn blinks(self) -> bool {
        matches!(
            self,
            CursorStyle::BlinkingBlock | CursorStyle::BlinkingUnderline | CursorStyle::BlinkingBar
        )
    }
}
