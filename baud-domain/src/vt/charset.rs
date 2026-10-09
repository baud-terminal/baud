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
    /// And it substitutes: `#` is `£`, through `[[source: xterm-charsets]]`'s
    /// table, cited as xterm's judgement rather than as DEC's (`with_nrcs`).
    /// This said baud "does not substitute" and that "the cell keeps the
    /// byte"; neither has been true since the tables landed.
    Uk,
    /// `0` — "DEC Special Character and Line Drawing Set, VT100."
    ///
    /// Translated through `[[source: foot-vt100-0]]`'s 31 entries, `0x60..0x7E`,
    /// and the cell keeps `CellFlags::GRAPHICS`. DEC's set is 32 entries and
    /// begins at `0x5F`, "Blank" (`[[source: vt100-ug-ch3]]` Table 3-9), which
    /// baud leaves as `_`: an open departure, the codepoint being the owner's
    /// call. See docs/record.md, "DEC Special Graphics begins at 0x5F, and what
    /// it stores there is the owner's call".
    DecGraphics,
    /// A national replacement set, named. See [`Nrcs`].
    National(Nrcs),
    /// `0` is [`Charset::DecGraphics`]; these are the other DEC sets reachable
    /// through a single final byte.
    Dec(DecSet),
    /// A final `[[source: ctlseqs]]` does not list, one byte or two
    /// (`an_unlisted_final_is_still_other`,
    /// `an_unlisted_multi_byte_final_is_counted`). Designated, so the slot is
    /// not mistaken for ASCII, and not translated, there being no set to
    /// translate by.
    ///
    /// This also named the **multi-byte** finals — `% 6` Portuguese, `" >`
    /// Greek, `& 4` DEC Cyrillic and the rest — as reaching here because
    /// `designate` did not parse a two-byte final. It does now, and
    /// [`Charset::from_multi`] names each of them.
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
/// Named and translated. 163 of the fourteen sets' 164 replacements come from
/// `[[source: xterm-charsets]]`, cited as xterm's judgement rather than as
/// DEC's; the other is Turkish 2/1, U+0131, which xterm's table lacks and
/// `[[source: dec-pplv2-pm]]` Figure A-26 draws, typed into the generator with
/// its citation. `each_vt220_national_table_is_decs` holds the ten VT220 sets
/// to DEC's own tables, and `every_national_replacement_renders_as_its_table_says`
/// drives every one through `Term`. This said "recorded and not translated"
/// and "the cell keeps the byte" after the tables landed.
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
    /// `% 6` — Portuguese, VT300.
    Portuguese,
    /// `" >` — Greek, VT500.
    Greek,
    /// `% 2` — Turkish, VT500.
    Turkish,
    /// `% =` — Hebrew, VT500.
    Hebrew,
}

impl Nrcs {
    /// A dense index, 0-13.
    ///
    /// It was for storing a [`Charset`] in one byte, through `Charset::to_u8`,
    /// in a `Run` field nothing reads. `Charset::to_u8` is gone, and the one
    /// caller left is `every_national_replacement_renders_as_its_table_says`,
    /// which counts the distinct sets its designator discovery reaches.
    pub fn to_u8(self) -> u8 {
        match self {
            Nrcs::Finnish => 0,
            Nrcs::Swedish => 1,
            Nrcs::German => 2,
            Nrcs::FrenchCanadian => 3,
            Nrcs::French => 4,
            Nrcs::Italian => 5,
            Nrcs::Spanish => 6,
            Nrcs::Dutch => 7,
            Nrcs::Swiss => 8,
            Nrcs::NorwegianDanish => 9,
            Nrcs::Portuguese => 10,
            Nrcs::Greek => 11,
            Nrcs::Turkish => 12,
            Nrcs::Hebrew => 13,
        }
    }
}

/// DEC-defined sets other than the line-drawing one. Designated and recorded,
/// not translated. This said "same deferral", the national sets' reason, which
/// was that no machine-readable source existed; `[[source: xterm-charsets]]`
/// carries xterm's tables for DEC Technical, Supplemental Graphics, Cyrillic,
/// Greek, Hebrew and Turkish, so for those what is missing is the translation,
/// not a source.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecSet {
    /// `<` — DEC Supplemental (VT200), and the User Preference Supplemental
    /// Set (UPSS, VT300; `[[source: dec-std-070]]` 070-3 p. 3-63), which share
    /// a final byte. Which of the two `<` should designate is note AE's Q14.
    Supplemental,
    /// `>` — DEC Technical, VT300.
    Technical,
    /// `I` — JIS-Katakana, VT382.
    JisKatakana,
    /// `J` — JIS-Roman, VT382.
    JisRoman,
    /// `" 4` — DEC Hebrew, VT500.
    Hebrew,
    /// `" ?` — DEC Greek, VT500.
    Greek,
    /// `% 0` — DEC Turkish, VT500.
    Turkish,
    /// `% 5` — DEC Supplemental Graphics, VT300.
    SupplementalGraphics,
    /// `& 4` — DEC Cyrillic, VT500.
    Cyrillic,
    /// `% 3` — SCS NRCS, and `& 5` — Russian NRCS, VT500: the names the
    /// pinned DEC texts give (`[[source: vt510-scs]]`; `[[source: vt520-rm]]`
    /// Table 5-14). "DEC Russian", which this said, is `[[source: ctlseqs]]`'s
    /// name (line 368).
    ///
    /// `[[source: ctlseqs]]`: "The VT520 reference manual lists a few more, but
    /// **no documentation has been found for the mappings**." xterm 411
    /// translates both all the same, by its own choices -- ISO-IR-141 for SCS,
    /// and for Russian the KOI-7 dialect Kermit used
    /// (`[[source: xterm-charsets]]`'s `map_NRCS_Serbo_Croatian` and
    /// `map_NRCS_Russian`) -- and designates them only at level 5
    /// (`[[source: xterm-reset]]` `charproc.c:1861-1862`). No DEC chart for
    /// either has been read, so both are designated and draw ASCII; what they
    /// should draw is note AF's Q-SCS-Russian. This also said
    /// they were "recognised as designations, which is all baud does with any
    /// set", stale since the national tables landed.
    Undocumented,
}

impl Charset {
    /// Recover the set a cell was written under from its flags.
    ///
    /// Only the line-drawing set has a flag of its own -- `CellFlags` has no
    /// spare bits, the link index having taken them -- so this answers
    /// `DecGraphics` or `Ascii`. A national set cannot be recovered from a
    /// cell at all: the cell holds the codepoint the set translated the byte
    /// to when it was written (`PackedCell::with_charset`), and nothing
    /// records the set. This said the national sets "reach the cell through
    /// `Run::charset`"; nothing in this core builds a `Run`, and nothing reads
    /// that field.
    pub fn from_flags(f: CellFlags) -> Charset {
        if f.is_graphics() {
            Charset::DecGraphics
        } else {
            Charset::Ascii
        }
    }

    /// A designation whose final is preceded by a second intermediate:
    /// `ESC ( % 6` and friends.
    ///
    /// Unreachable until the parser kept two intermediates —
    /// `[[source: williams]]`'s `escape_intermediate` collects them and baud
    /// overwrote, so the `(` was gone before the final arrived.
    pub fn from_multi(inter: StreamByte, f: StreamByte) -> Charset {
        match (inter.raw_byte(), f.raw_byte()) {
            (b'"', b'>') => Charset::National(Nrcs::Greek),
            (b'%', b'2') => Charset::National(Nrcs::Turkish),
            (b'%', b'6') => Charset::National(Nrcs::Portuguese),
            (b'%', b'=') => Charset::National(Nrcs::Hebrew),
            (b'"', b'4') => Charset::Dec(DecSet::Hebrew),
            (b'"', b'?') => Charset::Dec(DecSet::Greek),
            (b'%', b'0') => Charset::Dec(DecSet::Turkish),
            (b'%', b'5') => Charset::Dec(DecSet::SupplementalGraphics),
            (b'&', b'4') => Charset::Dec(DecSet::Cyrillic),
            (b'%', b'3') | (b'&', b'5') => Charset::Dec(DecSet::Undocumented),
            _ => Charset::Other,
        }
    }
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

/// What a DECSCUSR asks for: one style, or the terminal's initial one.
///
/// **`Ps = 0` is read as the initial style, which is foot's reading and not
/// xterm's.** `[[source: ctlseqs]]` makes 0 a blinking block and only 7
/// "initial resources"; xterm's `charproc.c` does the same, resolving
/// `INITIAL_CURSOR` to `screen->initial_cursor` and 0 to `BLINK_BLOCK`
/// (`[[source: xterm-reset]]`'s file, `CASE_DECSCUSR`). foot's `csi.c` makes 0
/// reset "to configured default" (`[[source: foot-csi]]`'s file, 1751). The two
/// agree while the initial style is a blinking block, which is baud's default,
/// and differ only for a reader who configured another -- and an application
/// that resets the cursor with 0 should land that reader on their choice. `1`
/// stays the blinking block the spec names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorPick {
    Style(CursorStyle),
    Initial,
}

/// A cursor's shape, the axis of [`CursorStyle`] a configuration names apart
/// from its blink.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Bar,
}

/// Whether a cursor blinks, the other axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorBlink {
    #[default]
    Blinking,
    Steady,
}

impl CursorStyle {
    /// The style `Ps` selects, or [`CursorPick::Initial`] for 0 and 7; `None`
    /// for a `Ps` no specification names.
    pub fn from_param(p: Option<Param>) -> Option<CursorPick> {
        Some(CursorPick::Style(match p.map_or(0, |x| x.0) {
            0 | 7 => return Some(CursorPick::Initial),
            1 => CursorStyle::BlinkingBlock,
            2 => CursorStyle::SteadyBlock,
            3 => CursorStyle::BlinkingUnderline,
            4 => CursorStyle::SteadyUnderline,
            5 => CursorStyle::BlinkingBar,
            6 => CursorStyle::SteadyBar,
            _ => return None,
        }))
    }
    /// The style a configuration names: a shape, blinking or steady.
    pub fn of(shape: CursorShape, blink: CursorBlink) -> CursorStyle {
        let steady = match shape {
            CursorShape::Block => CursorStyle::SteadyBlock,
            CursorShape::Underline => CursorStyle::SteadyUnderline,
            CursorShape::Bar => CursorStyle::SteadyBar,
        };
        steady.with_blink(blink == CursorBlink::Blinking)
    }
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
    /// Does this style blink? A renderer needs the two axes even though the
    /// wire encodes them together.
    pub fn blinks(self) -> bool {
        matches!(
            self,
            CursorStyle::BlinkingBlock | CursorStyle::BlinkingUnderline | CursorStyle::BlinkingBar
        )
    }
}
