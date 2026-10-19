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
    /// A 96-character set, designated by `ESC - . /` ([`Iso96`]).
    Iso96(Iso96),
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
    /// It numbers the set a cell was written under, in
    /// [`CellFlags::national`], and the DECRQCRA index that reads it back.
    pub const fn to_u8(self) -> u8 {
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

    /// The set [`Nrcs::to_u8`] numbers `i`.
    pub const fn from_u8(i: u8) -> Option<Nrcs> {
        Some(match i {
            0 => Nrcs::Finnish,
            1 => Nrcs::Swedish,
            2 => Nrcs::German,
            3 => Nrcs::FrenchCanadian,
            4 => Nrcs::French,
            5 => Nrcs::Italian,
            6 => Nrcs::Spanish,
            7 => Nrcs::Dutch,
            8 => Nrcs::Swiss,
            9 => Nrcs::NorwegianDanish,
            10 => Nrcs::Portuguese,
            11 => Nrcs::Greek,
            12 => Nrcs::Turkish,
            13 => Nrcs::Hebrew,
            _ => return None,
        })
    }
}

/// The set a GL code was received under, where the cell holds a character
/// the code cannot be read back from: what DECRQCRA reads to count the code
/// received ([`CellFlags::received`]).
///
/// **Only the sets that replace a code with another character are named.**
/// A set that draws its byte (ASCII, DEC Technical and the other untranslated
/// DEC sets, a national set's unreplaced positions) leaves the code in the
/// cell, and the UK set's `#` is drawn `£` and counted 0xA3, its codepoint's
/// low byte, as xterm counts it (Jev's lean of 2026-10-19).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Received {
    /// DEC Special Graphics, `0`.
    DecGraphics,
    /// A national replacement set.
    National(Nrcs),
    /// DEC Supplemental, `<` and `% 5`, which share a table.
    DecSupplemental,
    /// JIS-Katakana, `I`.
    JisKatakana,
    /// JIS-Roman, `J`.
    JisRoman,
    /// A 96-character set: every one replaces each code it is sent, SP's
    /// and DEL's among them.
    Iso96(Iso96),
}

impl Received {
    /// The set GL holds as a tag, if it replaces any code.
    pub const fn of(set: Charset) -> Option<Received> {
        match set {
            Charset::DecGraphics => Some(Received::DecGraphics),
            Charset::National(n) => Some(Received::National(n)),
            Charset::Dec(DecSet::Supplemental | DecSet::SupplementalGraphics) => {
                Some(Received::DecSupplemental)
            }
            Charset::Dec(DecSet::JisKatakana) => Some(Received::JisKatakana),
            Charset::Dec(DecSet::JisRoman) => Some(Received::JisRoman),
            Charset::Iso96(i) => Some(Received::Iso96(i)),
            _ => None,
        }
    }

    /// A dense index, 1-24, for the cell's five tag bits; 0 is none.
    pub const fn tag(self) -> u8 {
        match self {
            Received::National(n) => n.to_u8() + 1,
            Received::DecGraphics => 15,
            Received::DecSupplemental => 16,
            Received::JisKatakana => 17,
            Received::JisRoman => 18,
            Received::Iso96(i) => 19 + i.to_u8(),
        }
    }

    /// The set [`Received::tag`] numbers `t`.
    pub const fn from_tag(t: u8) -> Option<Received> {
        match t {
            15 => Some(Received::DecGraphics),
            16 => Some(Received::DecSupplemental),
            17 => Some(Received::JisKatakana),
            18 => Some(Received::JisRoman),
            19..=24 => match Iso96::from_u8(t - 19) {
                Some(i) => Some(Received::Iso96(i)),
                None => None,
            },
            _ => match t.checked_sub(1) {
                Some(i) => match Nrcs::from_u8(i) {
                    Some(n) => Some(Received::National(n)),
                    None => None,
                },
                None => None,
            },
        }
    }
}

/// DEC-defined sets other than the line-drawing one.
///
/// **DEC Supplemental, JIS-Katakana and JIS-Roman are translated**, through
/// the tables [`DecSet::table`] returns, generated by `scripts/gen-dec-sets.py`
/// from the DEC texts that are committed and the constants read from the ones
/// that are not, with `[[source: xterm-charsets]]`'s macros checked as
/// practice. **The rest are designated and draw their byte**, each for the
/// reason on its variant; `docs/vt-coverage.md` has the rows. This said every
/// set here was "designated and recorded, not translated".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecSet {
    /// `<` — DEC Supplemental (VT200), and the User Preference Supplemental
    /// Set (UPSS, VT300; `[[source: dec-std-070]]` 070-3 p. 3-63), which share
    /// a final byte. Which of the two `<` should designate is note AE's Q14;
    /// both readings draw DEC Supplemental while baud has no DECAUPSS, the
    /// UPSS's "Default: DEC Supplemental Graphic" (`[[source: vt510-decaupss]]`).
    Supplemental,
    /// `>` — DEC Technical, VT300. Not translated: which DEC layout (VT420
    /// Figure 2-10 against three other DEC texts at five cells), which hooks
    /// at 2/11-2/14 and what the seven summation pieces 3/1-3/7 store are note
    /// AF's Q-Technical, open.
    Technical,
    /// `I` — JIS-Katakana, VT382. Translated.
    JisKatakana,
    /// `J` — JIS-Roman, VT382. Translated but for 7/14, which draws `~` until
    /// note AF's Q-JIS-Roman says whether it is U+203E or U+00AF.
    JisRoman,
    /// `" 4` — DEC Hebrew Supplemental, VT500. Not translated: DEC charts it
    /// only in `[[source: dec-pplv2-pm]]` Figure A-22, which is `external`,
    /// and its glyphs have not been read from the figure cell by cell against
    /// xterm's `map_DEC_Hebrew_Supp` (note AF §3.5, §6).
    Hebrew,
    /// `" ?` — DEC Greek Supplemental, VT500. Not translated, for Hebrew's
    /// reason, Figure A-24.
    Greek,
    /// `% 0` — DEC Turkish Supplemental, VT500. Not translated, for Hebrew's
    /// reason, Figure A-27; and its seven hatched cells have no
    /// error-character sentence in §A.10 (note AF §3).
    Turkish,
    /// `% 5` — DEC Supplemental Graphics, VT300: DEC Supplemental, under the
    /// final `[[source: dec-std-070]]` gives it (`DEC_SUPPLEMENTAL_G %5`, p.
    /// 5-117). The same table as [`DecSet::Supplemental`].
    SupplementalGraphics,
    /// `& 4` — DEC Cyrillic, VT500. Not translated: no DEC chart of it has been
    /// found (note AF §3.6), and xterm's `map_DEC_Cyrillic` is "from
    /// screenshot".
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

impl DecSet {
    /// The set's 94 positions, indexed by `byte - 0x21`, or `None` for a set
    /// baud does not translate.
    ///
    /// **An index, not a search**: a designated set puts this on the
    /// per-character path, and every position of a translated set is in its
    /// table, a reserved one as DEC's error character.
    pub fn table(self) -> Option<&'static [Codepoint; 94]> {
        match self {
            DecSet::Supplemental | DecSet::SupplementalGraphics => {
                Some(&crate::dec_sets_data::DEC_SUPPLEMENTAL)
            }
            DecSet::JisKatakana => Some(&crate::dec_sets_data::JIS_KATAKANA),
            DecSet::JisRoman => Some(&crate::dec_sets_data::JIS_ROMAN),
            DecSet::Technical
            | DecSet::Hebrew
            | DecSet::Greek
            | DecSet::Turkish
            | DecSet::Cyrillic
            | DecSet::Undocumented => None,
        }
    }
}

/// The 96-character sets `ESC - . /` designate: `[[source: ctlseqs]]`
/// lists the finals `A`, `B`, `F`, `H`, `L` and `M` and "These controls apply
/// only to 96-character sets. Unlike the 94-character sets, these can have
/// different values than ASCII space and DEL for the mapping of 0x20 and
/// 0x7f." Each is the right half of an ISO 8859 part, the set its final is
/// registered as, translated through [`Iso96::table`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iso96 {
    /// `A` -- ISO Latin-1 Supplemental, VT300 (ISO-IR 100).
    Latin1,
    /// `B` -- ISO Latin-2 Supplemental, VT500 (ISO-IR 101).
    Latin2,
    /// `F` -- ISO Greek Supplemental, VT500 (ISO-IR 126).
    Greek,
    /// `H` -- ISO Hebrew Supplemental, VT500 (ISO-IR 138).
    Hebrew,
    /// `L` -- ISO Latin-Cyrillic, VT500 (ISO-IR 144).
    LatinCyrillic,
    /// `M` -- ISO Latin-5 Supplemental, VT500 (ISO-IR 148).
    Latin5,
}

impl Iso96 {
    /// The set a 96-series final names, if `[[source: ctlseqs]]` lists it.
    /// The 94-character and 96-character finals are separate series
    /// (`[[source: ecma35]]` NOTE 44), so `A` here is not the UK set.
    pub fn from_final(b: StreamByte) -> Option<Iso96> {
        Some(match b.raw_byte() {
            b'A' => Iso96::Latin1,
            b'B' => Iso96::Latin2,
            b'F' => Iso96::Greek,
            b'H' => Iso96::Hebrew,
            b'L' => Iso96::LatinCyrillic,
            b'M' => Iso96::Latin5,
            _ => return None,
        })
    }

    /// A dense index, 0-5, for the cell's set tag.
    pub const fn to_u8(self) -> u8 {
        match self {
            Iso96::Latin1 => 0,
            Iso96::Latin2 => 1,
            Iso96::Greek => 2,
            Iso96::Hebrew => 3,
            Iso96::LatinCyrillic => 4,
            Iso96::Latin5 => 5,
        }
    }

    /// The set [`Iso96::to_u8`] numbers `i`.
    pub const fn from_u8(i: u8) -> Option<Iso96> {
        Some(match i {
            0 => Iso96::Latin1,
            1 => Iso96::Latin2,
            2 => Iso96::Greek,
            3 => Iso96::Hebrew,
            4 => Iso96::LatinCyrillic,
            5 => Iso96::Latin5,
            _ => return None,
        })
    }

    /// The set's 96 positions, indexed by `code - 0x20` for the 7-bit code
    /// 2/0 through 7/15, an unassigned one DEC's error character; generated
    /// by `scripts/gen-iso96-sets.py`. **An index, not a search.**
    pub const fn table(self) -> &'static [Codepoint; 96] {
        match self {
            Iso96::Latin1 => &crate::iso96_data::LATIN_1,
            Iso96::Latin2 => &crate::iso96_data::LATIN_2,
            Iso96::Greek => &crate::iso96_data::GREEK,
            Iso96::Hebrew => &crate::iso96_data::HEBREW,
            Iso96::LatinCyrillic => &crate::iso96_data::LATIN_CYRILLIC,
            Iso96::Latin5 => &crate::iso96_data::LATIN_5,
        }
    }
}

/// Whether a designation names a 94-character or a 96-character set: which
/// series its final is from (`[[source: ecma35]]` NOTE 44), and the bit
/// DECCIR's `Scss` reports for the slot (`[[source: dec-std-070]]` p. 5-202).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SetSize {
    Chars94,
    Chars96,
}

/// **How a G slot was designated, as sent**: the size series and the
/// designating tail, a second intermediate if there was one and the final.
///
/// DECCIR's `Sdesig` reports "the intermediate and final characters of the
/// Select Character Set (SCS) designating escape sequence", and "the User
/// Preference Supplemental Set (UPSS) designator if that is how a character
/// set was designated" (`[[source: dec-std-070]]` p. 5-202): a property of
/// the sequence, which [`Charset`] does not keep. Finnish has two finals and
/// `<` and `% 5` name the same table, so the set alone cannot say which was
/// sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Designation {
    size: SetSize,
    /// The second intermediate (0 when there is none) and the final.
    tail: [u8; 2],
}

impl Designation {
    /// ASCII, `ESC ( B`: G0 and G1 at power-up.
    pub const ASCII: Designation = Designation {
        size: SetSize::Chars94,
        tail: [0, b'B'],
    };
    /// The user-preference supplemental set, `<`: G2 and G3 at power-up.
    pub const UPSS: Designation = Designation {
        size: SetSize::Chars94,
        tail: [0, b'<'],
    };

    /// A 94-character designation: `ESC ( F` or `ESC ( I F`.
    pub fn of_94(i2: Option<StreamByte>, f: StreamByte) -> Designation {
        Designation {
            size: SetSize::Chars94,
            tail: [i2.map_or(0, |i| i.raw_byte()), f.raw_byte()],
        }
    }

    /// A 96-character designation: `ESC - F`.
    pub fn of_96(f: StreamByte) -> Designation {
        Designation {
            size: SetSize::Chars96,
            tail: [0, f.raw_byte()],
        }
    }

    pub fn size(self) -> SetSize {
        self.size
    }

    /// The designating tail, one byte or two.
    pub fn tail(&self) -> &[u8] {
        match self.tail[0] {
            0 => &self.tail[1..],
            _ => &self.tail,
        }
    }

    /// The set this designation names.
    pub fn charset(self) -> Charset {
        let f = StreamByte::from_stream(self.tail[1]);
        match (self.size, self.tail[0]) {
            (SetSize::Chars96, _) => Iso96::from_final(f).map_or(Charset::Other, Charset::Iso96),
            (SetSize::Chars94, 0) => Charset::from_final(f),
            (SetSize::Chars94, i) => Charset::from_multi(StreamByte::from_stream(i), f),
        }
    }
}

impl Charset {
    /// Recover the set a cell was written under from its flags.
    ///
    /// The line-drawing set has a flag of its own, and every set that
    /// replaces a code a tag ([`CellFlags::received`], for DECRQCRA), so
    /// this answers `DecGraphics`, `National`, DEC Supplemental, JIS-Katakana,
    /// JIS-Roman, a 96-character set or `Ascii`; `% 5` reads as `<`, whose
    /// table it shares. The
    /// UK set and the sets that draw their byte are not recorded and read as
    /// `Ascii`.
    pub fn from_flags(f: CellFlags) -> Charset {
        // **The tag before the line-drawing flag**: the flag names the set
        // GL held (note AE, Q16(a)), and a character written through GR or
        // a single shift while GL held line drawing carries it beside the
        // tag of the set it was received under, which is the one written.
        match (f.received(), f.is_graphics()) {
            (Some(Received::DecGraphics), _) | (None, true) => Charset::DecGraphics,
            (Some(Received::National(n)), _) => Charset::National(n),
            (Some(Received::DecSupplemental), _) => Charset::Dec(DecSet::Supplemental),
            (Some(Received::JisKatakana), _) => Charset::Dec(DecSet::JisKatakana),
            (Some(Received::JisRoman), _) => Charset::Dec(DecSet::JisRoman),
            (Some(Received::Iso96(i)), _) => Charset::Iso96(i),
            (None, false) => Charset::Ascii,
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
    /// Whether this is a 96-character set, whose 2/0 and 7/15 are characters
    /// of the set when it is invoked into GL (`[[source: ecma35]]` 8.3.1),
    /// and 10/0 and 15/15 in GR (8.3.2).
    pub fn is_96(self) -> bool {
        matches!(self, Charset::Iso96(_))
    }

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

    /// The intermediate that designates a 96-character set into the slot:
    /// `-` is G1, `.` G2, `/` G3. G0 cannot hold one (`[[source: ecma35]]`
    /// 14.3.2: "Any type of graphic character set may be designated as a G1,
    /// G2, or G3 code element").
    pub fn from_intermediate96(b: StreamByte) -> Option<GSlot> {
        Some(match b.raw_byte() {
            b'-' => GSlot::G1,
            b'.' => GSlot::G2,
            b'/' => GSlot::G3,
            _ => return None,
        })
    }

    /// The slot numbered `i`, 0-3, as DECCIR's `Pgl` and `Pgr` number them.
    pub fn from_index(i: usize) -> Option<GSlot> {
        Some(match i {
            0 => GSlot::G0,
            1 => GSlot::G1,
            2 => GSlot::G2,
            3 => GSlot::G3,
            _ => return None,
        })
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
