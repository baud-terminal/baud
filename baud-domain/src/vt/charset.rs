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
    /// `B` — USASCII, and the default.
    #[default]
    Ascii,
    /// `A` — "United Kingdom (UK), VT100" (`[[source: ctlseqs]]`).
    ///
    /// **Not an alias for [`Charset::Ascii`], which is what it used to be.**
    /// The two differ in one position, and aliasing them meant `Term::charset`
    /// reported ASCII to an embedder that had just been told UK — the same
    /// silent conflation `CSI g` is careful to avoid by leaving unhandled
    /// parameters inert rather than aliasing them to `0`.
    ///
    /// baud records the designation and does not substitute, for the reason
    /// [`Charset::DecGraphics`] does not: the replacement is a *glyph* mapping,
    /// and `MANIFEST.toml` carries no machine-readable DEC source for one. The
    /// cell keeps the byte, so a renderer holding the table loses nothing and
    /// nothing is guessed.
    Uk,
    /// `0` — "DEC Special Character and Line Drawing Set, VT100."
    DecGraphics,
    /// A national replacement set, named. See [`Nrcs`].
    National(Nrcs),
    /// `0` is [`Charset::DecGraphics`]; these are the other DEC sets reachable
    /// through a single final byte.
    Dec(DecSet),
    /// Designated, recognised, and not translated. See [`CellFlags::GRAPHICS`]
    /// for why translation needs a source baud does not have.
    ///
    /// What reaches here now is a final byte `[[source: ctlseqs]]` does not
    /// list, or one of the **multi-byte** finals — `% 6` Portuguese, `" >`
    /// Greek, `& 4` DEC Cyrillic and the rest — which need a two-byte final
    /// that `designate` does not parse. Those are named in
    /// `docs/vt-coverage.md` rather than silently lumped in here.
    Other,
}

/// National Replacement Character Sets, from `[[source: ctlseqs]]`'s list of
/// valid final characters for `ESC ( C`.
///
/// **Named rather than folded into [`Charset::Other`].** "The term
/// *replacement* says that the character set is formed by replacing some of the
/// characters in a set with more useful ones for a given language" — so which
/// one it is decides which characters change, and an embedder that asked could
/// not previously tell Finnish from a byte baud did not recognise.
///
/// Recorded and not translated, for the reason `Charset::DecGraphics` is not:
/// the replacements are a glyph mapping and `MANIFEST.toml` carries no
/// machine-readable DEC source for one. The cell keeps the byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nrcs {
    /// `C` or `5`
    Finnish,
    /// `H` or `7`
    Swedish,
    /// `K`
    German,
    /// `Q` or `9`
    FrenchCanadian,
    /// `R` or `f`
    French,
    /// `Y`
    Italian,
    /// `Z`
    Spanish,
    /// `4`
    Dutch,
    /// `=`
    Swiss,
    /// `` ` ``, `E` or `6`
    NorwegianDanish,
}

/// DEC-defined sets other than the line-drawing one, reachable through a single
/// final byte. Same deferral: designated, recorded, not translated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecSet {
    /// `<` — DEC Supplemental (VT200), and User Preferred Selection Set
    /// (VT300), which share a final byte.
    Supplemental,
    /// `>` — DEC Technical, VT300.
    Technical,
    /// `I` — JIS-Katakana, VT382.
    JisKatakana,
    /// `J` — JIS-Roman, VT382.
    JisRoman,
}

impl Charset {
    pub fn from_final(b: StreamByte) -> Charset {
        match b.raw_byte() {
            b'0' => Charset::DecGraphics,
            b'B' => Charset::Ascii,
            b'A' => Charset::Uk,
            b'C' | b'5' => Charset::National(Nrcs::Finnish),
            b'H' | b'7' => Charset::National(Nrcs::Swedish),
            b'K' => Charset::National(Nrcs::German),
            b'Q' | b'9' => Charset::National(Nrcs::FrenchCanadian),
            b'R' | b'f' => Charset::National(Nrcs::French),
            b'Y' => Charset::National(Nrcs::Italian),
            b'Z' => Charset::National(Nrcs::Spanish),
            b'4' => Charset::National(Nrcs::Dutch),
            b'=' => Charset::National(Nrcs::Swiss),
            b'`' | b'E' | b'6' => Charset::National(Nrcs::NorwegianDanish),
            b'<' => Charset::Dec(DecSet::Supplemental),
            b'>' => Charset::Dec(DecSet::Technical),
            b'I' => Charset::Dec(DecSet::JisKatakana),
            b'J' => Charset::Dec(DecSet::JisRoman),
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
