//! DECTSR's data: baud's terminal state report, `DCS 1 $ s D...D ST`, and
//! the colour table report DECCTR, `DCS 2 $ s D...D ST`.
//!
//! `[[source: dec-std-070]]` pp. 5-206 to 5-208: the report is "a Sixel
//! encoded, string of 8-bit bytes which contains the state information for
//! the device", "guaranteed not to be longer than 256 characters, including
//! the DCS control character, introducer sequence, and ST control
//! character", "the same as the state information available through the
//! other discrete TSI controls" -- DECRQM, DECRQSS and DECRQPSR -- and its
//! format "is firmware revision dependent". So the layout is baud's own,
//! the VT420's guideline its alphabet: every data byte is `4/0` to `4/15`
//! and carries one nibble, "system parameters are contained in low nibble
//! of each byte", and two checksum bytes follow, the 2's complement of the
//! 8-bit sum of the data bytes, a nibble each, high first.
//!
//! **The layout is fixed**, every field at a known nibble, so a field with
//! a value baud cannot take is stepped over and the next one read, STD
//! 070's "the terminal will ignore the error if possible"; a first nibble
//! that is not [`TSR_VERSION`] restores nothing. In order:
//!
//! | nibbles | field |
//! |---|---|
//! | 1 | the version, [`TSR_VERSION`] |
//! | 1 | the conformance level 1 to 4 (bits 0-1), eight-bit C1 (bit 2), ISO 8859-1 input (bit 3) |
//! | 4 | the eight ANSI modes [`TSR_ANSI_MODES`], two bits each |
//! | 37 | the seventy-four private modes XTSAVE keeps, two bits each, by slot |
//! | 28 | the rendition: 23 attribute bits in `Attrs::WIRE`'s order (6), the font (1), the foreground, background and underline colours (7 each) |
//! | 12 | DECSTBM's top and bottom, DECSLRM's left and right, from 1 |
//! | 1 | DECSCUSR's `Ps` |
//! | 1 | DECSWBV's and DECSMBV's bands, off, low and high, two bits each |
//! | 1 | DECSACE (bit 0), DECSSDT (bits 1-2), DECSASD (bit 3) |
//! | 3 | DECSLPP's lines (bits 0-10), DECSCPP's 132 (bit 11) |
//! | 1 | XTSMTITLE's four modes |
//! | 18 | XTMODKEYS for the seven resources, -1 to 15 as a byte (14); XTFMTKEYS's seven formats (2); modifyOtherKeys' mask (2) |
//! | 27 | DECCIR's fields: the row and column (3 each), `Srend`, `Satt`, `Sflag` (1 each), GL and GR (1), the four designations ([`TSR_DESIGNATIONS`]) |
//! | 53 | the shown screen's `DECSC` save: its row from 1, 0 for none (3), column from 0 (3), rendition (28), protection (bit 0), a wrap waiting at the left margin under DECRLM (bit 1), DECOM (bit 2) and pending wrap (bit 3) (1), GL and GR (1), the four designations |
//! | 4 | the tab stops: carried (bit 0) or every eighth column (bit 1); the columns (3) |
//! | 0-58 | the tab stops' bitmap, a bit a column, when carried, [`TSR_TAB_COLS`] columns at most |
//!
//! **The `DECSC` save is the shown screen's**: no other control reads it
//! back, and the VT420 manual's DECTSR has "the entire state of the
//! terminal, except for user-defined key definitions and the current soft
//! character set". The other screen's, the primary's `?1049` save while
//! the alternate screen is up, is not carried.
//!
//! A tab bitmap is carried only where it fits the 256-character bound, a
//! screen up to [`TSR_TAB_COLS`] columns; a wider screen carries the stops
//! as one flag when they are every eighth column, and else not at all, and
//! DECTABSR reports them at any width.

use crate::*;

/// The version nibble: a report a later layout reads as foreign. 2 since
/// the private modes took a sixty-first slot, `?1040`; 3 since they took
/// terminal-wg's `?2500`, `?2501` and `?1243`; 4 since they took DEC's
/// `?96` and `?36`; 5 since the ANSI modes took DCSM; 6 since the private
/// modes took `?34`; 7 since they took `?35`; 8 since the ANSI modes took
/// HEM; 9 since the private modes took xterm's six keyboards, `?1050` to
/// `?1053`, `?1060` and `?1061`; 10 since the `DECSC` save's bit 1 took
/// DECRLM's wait at the left margin.
pub const TSR_VERSION: u8 = 10;

/// The data nibbles 256 characters leave: less `ESC P`, `1 $ s`, the two
/// checksum bytes and `ESC \`.
pub const TSR_DATA_MAX: usize = 256 - 2 - 3 - 2 - 2;

/// The nibbles before the tab bitmap.
pub const TSR_FIXED: usize = 1
    + 1
    + TSR_ANSI_NIBBLES
    + TSR_PRIVATE_MODES.div_ceil(2)
    + TSR_RENDITION
    + 12
    + 1
    + 1
    + 1
    + 3
    + 1
    + 18
    + TSR_CURSOR
    + TSR_SAVED
    + 4;

/// The nibbles of a rendition: 23 attribute bits (6), the font (1), three
/// colours (7 each).
pub const TSR_RENDITION: usize = 6 + 1 + 3 * 7;

/// The nibbles of four designations: which are 96-character sets (1), and
/// each one's intermediate and final, a byte each (4).
pub const TSR_DESIGNATIONS: usize = 1 + 4 * 4;

/// The nibbles of DECCIR's fields.
pub const TSR_CURSOR: usize = 3 + 3 + 1 + 1 + 1 + 1 + TSR_DESIGNATIONS;

/// The nibbles of the `DECSC` save.
pub const TSR_SAVED: usize = 3 + 3 + TSR_RENDITION + 1 + 1 + TSR_DESIGNATIONS;

/// The widest screen whose tab stops are carried as a bitmap.
pub const TSR_TAB_COLS: usize = (TSR_DATA_MAX - TSR_FIXED) * 4;

/// The bytes of a tab bitmap, a bit a column.
pub const TSR_TAB_BYTES: usize = TSR_TAB_COLS.div_ceil(8);

/// The longest DECTSR, the whole of STD 070's bound.
pub const TSR_REPLY_MAX: usize = 2 + 3 + TSR_DATA_MAX + 2 + 2;
const _: () = assert!(TSR_REPLY_MAX == 256);

/// The ANSI modes DECRQM answers, in the report's order: IRM, LNM, ERM, KAM,
/// SRM, BDSM, DCSM and HEM. TSM is permanently reset and carries nothing.
/// BDSM took the two bits the five left clear; DCSM took a fourth nibble
/// and the report's version 5, and HEM that nibble's low two bits and
/// version 8. DCSM is carried as it is set, which DECRQM answers only once
/// BDSM is reset.
pub const TSR_ANSI_MODES: [u16; 8] = [4, 20, 6, 2, 12, 8, 9, 10];

/// The nibbles the ANSI modes take, two bits each.
pub const TSR_ANSI_NIBBLES: usize = TSR_ANSI_MODES.len().div_ceil(2);

/// How many private modes the report carries: XTSAVE's slots.
pub const TSR_PRIVATE_MODES: usize = SLOT_WIRE.len();

/// One mode's state as the report carries it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsrMode {
    /// Not recognised now, or permanent: nothing to restore.
    Absent,
    Set,
    Reset,
}

impl TsrMode {
    fn code(self) -> u32 {
        match self {
            TsrMode::Absent => 0,
            TsrMode::Set => 1,
            TsrMode::Reset => 2,
        }
    }
    fn of(code: u32) -> Option<TsrMode> {
        match code {
            0 => Some(TsrMode::Absent),
            1 => Some(TsrMode::Set),
            2 => Some(TsrMode::Reset),
            _ => None,
        }
    }
}

/// The tab stops as the report carries them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsrTabs {
    /// Not carried: a screen too wide whose stops are not every eighth.
    Absent,
    /// Every eighth column, 9, 17, 25 and on, across the screen.
    EveryEighth,
    /// A stop wherever `bits` has one, for the first `cols` columns.
    Stops {
        cols: u16,
        bits: [u8; TSR_TAB_BYTES],
    },
}

/// A `DECSC` save as the report carries it: what `DECRC` puts back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TsrSaved {
    /// The row and column, from 0; the column may be one past the right
    /// margin, the pending-wrap sentinel, which `pending` tells apart.
    pub row: u16,
    pub col: u16,
    pub rendition: StyleWord,
    /// DECSCA's protection, DECOM, whether the column is one past a
    /// pending wrap's, and whether a wrap waits on the column at the left
    /// margin, DECRLM's. **Bit 1 of their nibble is the last**: a `DECSC`
    /// save holds no DECAWM, and the layout of version 10 gives it the wait.
    pub protect: ModeChange,
    pub origin: ModeChange,
    pub pending: ModeChange,
    pub left_edge: ModeChange,
    pub gl: GSlot,
    pub gr: GSlot,
    pub designations: [Designation; 4],
}

/// What DECTSR reports, as the controls that set it take it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TerminalState {
    pub level: ConformanceLevel,
    pub c1: C1Transmit,
    /// `ESC % @` or `ESC % G`, which `?1020` reports.
    pub encoding: InputEncoding,
    pub ansi: [TsrMode; TSR_ANSI_MODES.len()],
    pub private: [TsrMode; TSR_PRIVATE_MODES],
    pub rendition: StyleWord,
    /// DECSTBM's and DECSLRM's parameters.
    pub top: u16,
    pub bottom: u16,
    pub left: u16,
    pub right: u16,
    /// DECSCUSR's `Ps`, 0 to 6.
    pub cursor_style: u8,
    /// DECSWBV's and DECSMBV's `Ps`, 1, 2 or 5.
    pub warning: u8,
    pub margin: u8,
    /// DECSACE's `Ps`, 0 or 2; DECSSDT's, 0 to 2; DECSASD's, 0 or 1.
    pub extent: u8,
    pub status_type: u8,
    pub active: u8,
    /// DECSCPP's columns, 80 or 132, and DECSLPP's lines, 24 and up.
    pub columns: u16,
    pub lines: u16,
    /// XTSMTITLE's modes, bit n for mode n.
    pub title_modes: u8,
    /// XTMODKEYS' values, -1 to 15, and XTFMTKEYS' formats, 0 or 1, in
    /// `KeyResource::ALL`'s order; modifyOtherKeys' mask as it is sent,
    /// 0 for none.
    pub modify: [i8; 7],
    pub format: [u8; 7],
    pub mask: u8,
    /// What DECCIR reports.
    pub cursor: CursorInformation,
    /// The shown screen's `DECSC` save, which `?1048` reports is there.
    pub saved: Option<TsrSaved>,
    pub tabs: TsrTabs,
}

/// What a DECRSTS body restores: each field, or `None` where its value is
/// not one baud can take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TerminalRestore {
    pub level: Option<(ConformanceLevel, C1Transmit)>,
    pub encoding: Option<InputEncoding>,
    pub ansi: [Option<TsrMode>; TSR_ANSI_MODES.len()],
    pub private: [Option<TsrMode>; TSR_PRIVATE_MODES],
    pub rendition: Option<StyleWord>,
    pub top: Option<u16>,
    pub bottom: Option<u16>,
    pub left: Option<u16>,
    pub right: Option<u16>,
    pub cursor_style: Option<u8>,
    pub warning: Option<u8>,
    pub margin: Option<u8>,
    pub extent: Option<u8>,
    pub status_type: Option<u8>,
    pub active: Option<u8>,
    pub columns: Option<u16>,
    pub lines: Option<u16>,
    pub title_modes: Option<u8>,
    pub modify: [Option<i8>; 7],
    pub format: [Option<u8>; 7],
    pub mask: Option<u8>,
    /// DECCIR's data string, rebuilt from the fields, and its length.
    pub cursor: Option<([u8; CIR_DATA_MAX], u8)>,
    /// The save: `Some(None)` when the report says there was none.
    pub saved: Option<Option<TsrSaved>>,
    pub tabs: Option<TsrTabs>,
}

/// Nibbles written into a report.
struct Put<'a> {
    out: &'a mut [u8; TSR_DATA_MAX],
    n: usize,
}

impl Put<'_> {
    /// `v`'s low `k` nibbles, high first.
    fn nibbles(&mut self, v: u32, k: usize) {
        let mut i = k;
        while i > 0 {
            i -= 1;
            self.out[self.n] = 0x40 | ((v >> (4 * i)) & 15) as u8;
            self.n += 1;
        }
    }
}

/// Nibbles read from a report.
struct Get<'a> {
    data: &'a [u8],
    n: usize,
}

impl Get<'_> {
    /// The next `k` nibbles as one value, high first, or `None` past the
    /// end. The checksum pass has already held every byte to `4/0..4/15`.
    fn nibbles(&mut self, k: usize) -> Option<u32> {
        let bytes = self.data.get(self.n..self.n + k)?;
        self.n += k;
        Some(bytes.iter().fold(0, |v, b| (v << 4) | u32::from(b & 15)))
    }
}

/// A band's code: off 0, low 1, high 2, from DECSWBV's `Ps`.
fn band(ps: u8) -> u32 {
    match ps {
        1 => 0,
        2 => 1,
        _ => 2,
    }
}

/// The `Ps` of a band's code.
fn band_ps(code: u32) -> Option<u8> {
    match code {
        0 => Some(1),
        1 => Some(2),
        2 => Some(5),
        _ => None,
    }
}

fn level_code(l: ConformanceLevel) -> u32 {
    match l {
        ConformanceLevel::One => 0,
        ConformanceLevel::Two => 1,
        ConformanceLevel::Three => 2,
        ConformanceLevel::Four => 3,
    }
}

fn colour(c: ColorWord) -> u32 {
    c.bits() & 0x03ff_ffff
}

/// A colour from its seven nibbles, or `None` for a tag no colour has.
fn colour_of(v: u32) -> Option<ColorWord> {
    let value = v & 0x00ff_ffff;
    match v >> 24 {
        0 if value == 0 => Some(ColorWord::DEFAULT),
        1 if value <= 0xff => Some(ColorWord::indexed(Chan::from_u8(value as u8))),
        2 => Some(ColorWord::rgb(
            Chan::from_u8((value >> 16) as u8),
            Chan::from_u8((value >> 8) as u8),
            Chan::from_u8(value as u8),
        )),
        _ => None,
    }
}

/// A slot from its two bits.
fn slot(v: u32) -> GSlot {
    match v & 3 {
        0 => GSlot::G0,
        1 => GSlot::G1,
        2 => GSlot::G2,
        _ => GSlot::G3,
    }
}

/// A DECCIR flag byte's four bits as a nibble.
fn cir_nibble(b: CirBits) -> u32 {
    [CirBits::BIT1, CirBits::BIT2, CirBits::BIT3, CirBits::BIT4]
        .iter()
        .enumerate()
        .fold(0, |v, (i, bit)| v | (u32::from(b.has(*bit)) << i))
}

/// The DECCIR flag byte a nibble's bits make.
fn cir_bits(v: u32) -> CirBits {
    [CirBits::BIT1, CirBits::BIT2, CirBits::BIT3, CirBits::BIT4]
        .iter()
        .enumerate()
        .fold(CirBits::NONE, |b, (i, bit)| {
            b.with(*bit, ModeChange::of(v & (1 << i) != 0))
        })
}

impl Put<'_> {
    /// A rendition, [`TSR_RENDITION`] nibbles.
    fn rendition(&mut self, w: StyleWord) {
        let attrs = w.attrs();
        let mut bits = 0u32;
        for (i, (bit, _)) in Attrs::WIRE.iter().enumerate() {
            if attrs.has(*bit) {
                bits |= 1 << i;
            }
        }
        self.nibbles(bits, 6);
        self.nibbles(u32::from(w.font().index()), 1);
        self.nibbles(colour(w.fg()), 7);
        self.nibbles(colour(w.bg()), 7);
        self.nibbles(colour(w.underline()), 7);
    }

    /// Four designations, [`TSR_DESIGNATIONS`] nibbles.
    fn designations(&mut self, d: &[Designation; 4]) {
        let sizes = d.iter().enumerate().fold(0, |v, (i, d)| {
            v | (u32::from(d.size() == SetSize::Chars96) << i)
        });
        self.nibbles(sizes, 1);
        for d in d {
            let (i2, f) = match d.tail() {
                [f] => (0, *f),
                [i2, f] => (*i2, *f),
                _ => (0, 0),
            };
            self.nibbles((u32::from(i2) << 8) | u32::from(f), 4);
        }
    }
}

impl Get<'_> {
    /// A rendition, or `None` for one baud cannot take; its nibbles are
    /// read either way.
    fn rendition(&mut self) -> Option<StyleWord> {
        let bits = self.nibbles(6);
        let font = self.nibbles(1);
        let fg = self.nibbles(7);
        let bg = self.nibbles(7);
        let ul = self.nibbles(7);
        let bits = bits?;
        if bits >> Attrs::WIRE.len() != 0 {
            return None;
        }
        let mut a = Attrs::NONE;
        for (i, (bit, _)) in Attrs::WIRE.iter().enumerate() {
            if bits & (1 << i) != 0 {
                a = a.with(*bit);
            }
        }
        let font = FontId::from_sgr(10 + font? as u16)?;
        Some(
            StyleWord::DEFAULT
                .with_attrs(a)
                .with_font(font)
                .with_fg(colour_of(fg?)?)
                .with_bg(colour_of(bg?)?)
                .with_underline(colour_of(ul?)?),
        )
    }

    /// Four designations, or `None` when one is no designation: a final
    /// outside `3/0..7/14`, an intermediate outside `2/0..2/15`, a
    /// 96-character set with an intermediate or in G0 (`[[source: ecma35]]`
    /// 14.3.2). Its nibbles are read either way.
    fn designations(&mut self) -> Option<[Designation; 4]> {
        let sizes = self.nibbles(1);
        let mut tails = [None; 4];
        for t in tails.iter_mut() {
            *t = self.nibbles(4);
        }
        let sizes = sizes?;
        let mut out = [Designation::ASCII; 4];
        for (i, (d, t)) in out.iter_mut().zip(tails).enumerate() {
            let t = t?;
            let (i2, f) = ((t >> 8) as u8, t as u8);
            if !(0x30..=0x7e).contains(&f) || (i2 != 0 && !(0x20..=0x2f).contains(&i2)) {
                return None;
            }
            let f = StreamByte::from_stream(f);
            *d = match (sizes >> i) & 1 {
                1 if i2 == 0 && i > 0 => Designation::of_96(f),
                1 => return None,
                _ => Designation::of_94((i2 != 0).then(|| StreamByte::from_stream(i2)), f),
            };
        }
        Some(out)
    }
}

impl TerminalState {
    /// The data and checksum, `D1 ... Dn <checksum1> <checksum2>`, into
    /// `out`; the bytes written.
    pub fn encode(&self, out: &mut [u8; TSR_DATA_MAX + 2]) -> usize {
        let mut data = [0u8; TSR_DATA_MAX];
        let mut p = Put {
            out: &mut data,
            n: 0,
        };
        p.nibbles(u32::from(TSR_VERSION), 1);
        let eight = match self.c1 {
            C1Transmit::SevenBit => 0,
            C1Transmit::EightBit => 4,
        };
        let latin1 = match self.encoding {
            InputEncoding::Utf8 => 0,
            InputEncoding::Latin1 => 8,
        };
        p.nibbles(level_code(self.level) | eight | latin1, 1);
        let ansi = (0..2 * TSR_ANSI_NIBBLES).fold(0, |v, i| {
            (v << 2) | self.ansi.get(i).map_or(0, |m| m.code())
        });
        p.nibbles(ansi, TSR_ANSI_NIBBLES);
        for pair in self.private.chunks(2) {
            let low = pair.get(1).map_or(0, |m| m.code());
            p.nibbles((pair[0].code() << 2) | low, 1);
        }
        p.rendition(self.rendition);
        for m in [self.top, self.bottom, self.left, self.right] {
            p.nibbles(u32::from(m), 3);
        }
        p.nibbles(u32::from(self.cursor_style), 1);
        p.nibbles(band(self.warning) | (band(self.margin) << 2), 1);
        let ext = match self.extent {
            2 => 1,
            _ => 0,
        };
        p.nibbles(
            ext | (u32::from(self.status_type) << 1) | (u32::from(self.active) << 3),
            1,
        );
        let wide = match self.columns {
            132 => 1 << 11,
            _ => 0,
        };
        p.nibbles(wide | u32::from(self.lines.min(0x7ff)), 3);
        p.nibbles(u32::from(self.title_modes), 1);
        for v in self.modify {
            p.nibbles(u32::from(v as u8), 2);
        }
        let formats = self
            .format
            .iter()
            .enumerate()
            .fold(0, |v, (i, f)| v | (u32::from(*f & 1) << i));
        p.nibbles(formats, 2);
        p.nibbles(u32::from(self.mask), 2);
        let c = &self.cursor;
        p.nibbles(u32::from(c.row.get()), 3);
        p.nibbles(u32::from(c.col.get()), 3);
        p.nibbles(cir_nibble(c.rendition), 1);
        p.nibbles(cir_nibble(c.attribute), 1);
        p.nibbles(cir_nibble(c.flags), 1);
        p.nibbles((c.gl.index() | (c.gr.index() << 2)) as u32, 1);
        p.designations(&c.designations);
        match self.saved {
            None => {
                for _ in 0..TSR_SAVED {
                    p.nibbles(0, 1);
                }
            }
            Some(s) => {
                p.nibbles(u32::from(s.row) + 1, 3);
                p.nibbles(u32::from(s.col), 3);
                p.rendition(s.rendition);
                let flags = [s.protect, s.left_edge, s.origin, s.pending]
                    .iter()
                    .enumerate()
                    .fold(0, |v, (i, m)| v | (u32::from(*m == ModeChange::Set) << i));
                p.nibbles(flags, 1);
                p.nibbles((s.gl.index() | (s.gr.index() << 2)) as u32, 1);
                p.designations(&s.designations);
            }
        }
        match self.tabs {
            TsrTabs::Absent => p.nibbles(0, 4),
            TsrTabs::EveryEighth => p.nibbles(2 << 12, 4),
            TsrTabs::Stops { cols, bits } => {
                p.nibbles((1 << 12) | u32::from(cols), 4);
                for i in 0..usize::from(cols).div_ceil(4) {
                    let byte = bits[i / 2];
                    let half = match i % 2 {
                        0 => byte & 15,
                        _ => byte >> 4,
                    };
                    p.nibbles(u32::from(half), 1);
                }
            }
        }
        let n = p.n;
        let sum = data[..n].iter().fold(0u8, |s, b| s.wrapping_add(*b));
        let check = 0u8.wrapping_sub(sum);
        out[..n].copy_from_slice(&data[..n]);
        out[n] = 0x40 | (check >> 4);
        out[n + 1] = 0x40 | (check & 15);
        n + 2
    }
}

/// The tab bitmap for `stops`, a screen's columns: `Stops` where it fits,
/// else `EveryEighth` when the stops are every eighth column, else `Absent`.
pub fn tsr_tabs(stops: &[bool]) -> TsrTabs {
    if stops.len() <= TSR_TAB_COLS {
        let mut bits = [0u8; TSR_TAB_BYTES];
        for (i, s) in stops.iter().enumerate() {
            if *s {
                bits[i / 8] |= 1 << (i % 8);
            }
        }
        return TsrTabs::Stops {
            cols: stops.len() as u16,
            bits,
        };
    }
    match stops
        .iter()
        .enumerate()
        .all(|(i, s)| *s == (i > 0 && i % 8 == 0))
    {
        true => TsrTabs::EveryEighth,
        false => TsrTabs::Absent,
    }
}

impl TerminalRestore {
    /// A DECRSTS body read: `None`, restoring nothing, when a byte is not
    /// `4/0..4/15`, the checksum fails, or the version is not baud's --
    /// "If the checksum fails, the entire sequence is ignored" (p. 5-208).
    pub fn parse(body: &[u8]) -> Option<TerminalRestore> {
        if body.len() < 3 || body.iter().any(|b| b & 0xf0 != 0x40) {
            return None;
        }
        let (data, check) = body.split_at(body.len() - 2);
        let sum = data.iter().fold(0u8, |s, b| s.wrapping_add(*b));
        let check = ((check[0] & 15) << 4) | (check[1] & 15);
        if sum.wrapping_add(check) != 0 {
            return None;
        }
        let mut g = Get { data, n: 0 };
        if g.nibbles(1)? != u32::from(TSR_VERSION) {
            return None;
        }
        let lc = g.nibbles(1);
        let encoding = lc.map(|v| match v & 8 {
            0 => InputEncoding::Utf8,
            _ => InputEncoding::Latin1,
        });
        let level = lc.map(|v| {
            let level = match v & 3 {
                0 => ConformanceLevel::One,
                1 => ConformanceLevel::Two,
                2 => ConformanceLevel::Three,
                _ => ConformanceLevel::Four,
            };
            let c1 = match v & 4 {
                0 => C1Transmit::SevenBit,
                _ => C1Transmit::EightBit,
            };
            (level, c1)
        });
        let mut ansi = [None; TSR_ANSI_MODES.len()];
        if let Some(v) = g.nibbles(TSR_ANSI_NIBBLES) {
            for (i, m) in ansi.iter_mut().enumerate() {
                *m = TsrMode::of((v >> (2 * (2 * TSR_ANSI_NIBBLES - 1 - i))) & 3);
            }
        }
        let mut private = [None; TSR_PRIVATE_MODES];
        for pair in private.chunks_mut(2) {
            if let Some(v) = g.nibbles(1) {
                pair[0] = TsrMode::of(v >> 2);
                if let Some(m) = pair.get_mut(1) {
                    *m = TsrMode::of(v & 3);
                }
            }
        }
        let rendition = g.rendition();
        let mut margin = || g.nibbles(3).map(|v| v as u16).filter(|v| *v > 0);
        let (top, bottom, left, right) = (margin(), margin(), margin(), margin());
        let cursor_style = g.nibbles(1).map(|v| v as u8).filter(|v| *v <= 6);
        let bands = g.nibbles(1);
        let warning = bands.and_then(|v| band_ps(v & 3));
        let margin = bands.and_then(|v| band_ps(v >> 2));
        let st = g.nibbles(1);
        let extent = st.map(|v| match v & 1 {
            0 => 0,
            _ => 2,
        });
        let status_type = st.map(|v| ((v >> 1) & 3) as u8).filter(|v| *v <= 2);
        let active = st.map(|v| (v >> 3) as u8);
        let page = g.nibbles(3);
        let columns = page.map(|v| match v >> 11 {
            0 => 80,
            _ => 132,
        });
        let lines = page.map(|v| (v & 0x7ff) as u16).filter(|v| *v >= 24);
        let title_modes = g.nibbles(1).map(|v| v as u8);
        let mut modify = [None; 7];
        for m in modify.iter_mut() {
            *m = g
                .nibbles(2)
                .map(|v| v as u8 as i8)
                .filter(|v| (-1..=15).contains(v));
        }
        let mut format = [None; 7];
        if let Some(v) = g.nibbles(2).filter(|v| v >> 7 == 0) {
            for (i, f) in format.iter_mut().enumerate() {
                *f = Some(((v >> i) & 1) as u8);
            }
        }
        let mask = g.nibbles(2).map(|v| v as u8);
        let cursor = (|| {
            let row = g.nibbles(3);
            let col = g.nibbles(3);
            let rendition = g.nibbles(1);
            let attribute = g.nibbles(1);
            let flags = g.nibbles(1);
            let slots = g.nibbles(1);
            let designations = g.designations()?;
            let (row, col, slots) = (row? as u16, col? as u16, slots?);
            if row == 0 || col == 0 || attribute? > 1 {
                return None;
            }
            let info = CursorInformation {
                row: OneBased(row),
                col: OneBased(col),
                rendition: cir_bits(rendition?),
                attribute: cir_bits(attribute?),
                flags: cir_bits(flags?),
                gl: slot(slots),
                gr: slot(slots >> 2),
                designations,
            };
            let mut bytes = [0u8; CIR_DATA_MAX];
            let len = info.encode(&mut bytes) as u8;
            Some((bytes, len))
        })();
        let saved = (|| {
            let row = g.nibbles(3);
            let col = g.nibbles(3);
            let rendition = g.rendition();
            let flags = g.nibbles(1);
            let slots = g.nibbles(1);
            let designations = g.designations();
            let row = row? as u16;
            if row == 0 {
                return Some(None);
            }
            let (flags, slots) = (flags?, slots?);
            let bit = |i: u32| ModeChange::of(flags & (1 << i) != 0);
            Some(Some(TsrSaved {
                row: row - 1,
                col: col? as u16,
                rendition: rendition?,
                protect: bit(0),
                origin: bit(2),
                pending: bit(3),
                left_edge: bit(1),
                gl: slot(slots),
                gr: slot(slots >> 2),
                designations: designations?,
            }))
        })();
        let tabs = (|| {
            let head = g.nibbles(4)?;
            let cols = (head & 0xfff) as u16;
            match head >> 12 {
                0 => Some(TsrTabs::Absent),
                2 => Some(TsrTabs::EveryEighth),
                1 if usize::from(cols) <= TSR_TAB_COLS => {
                    let mut bits = [0u8; TSR_TAB_BYTES];
                    for i in 0..usize::from(cols).div_ceil(4) {
                        let half = g.nibbles(1)? as u8;
                        bits[i / 2] |= match i % 2 {
                            0 => half,
                            _ => half << 4,
                        };
                    }
                    (g.n == g.data.len()).then_some(TsrTabs::Stops { cols, bits })
                }
                _ => None,
            }
        })();
        Some(TerminalRestore {
            level,
            encoding,
            ansi,
            private,
            rendition,
            top,
            bottom,
            left,
            right,
            cursor_style,
            warning,
            margin,
            extent,
            status_type,
            active,
            columns,
            lines,
            title_modes,
            modify,
            format,
            mask,
            cursor,
            saved,
            tabs,
        })
    }
}

/// The colour coordinate system of a DECCTR, its `Pu`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSpace {
    /// 1: hue 0-360, lightness and saturation 0-100, DEC's hue wheel with
    /// blue at 0.
    Hls,
    /// 2: red, green and blue, 0-100.
    Rgb,
}

impl ColorSpace {
    /// DECCTR's `Pu`: 0 or omitted HLS, the VT340's default
    /// (`[[source: vt3xx-tp-002]]` p. 206, "0 or none HLS (default)"),
    /// 1 HLS, 2 RGB, and any other no system the text names.
    pub fn of_request(pu: u16) -> Option<ColorSpace> {
        match pu {
            0 | 1 => Some(ColorSpace::Hls),
            2 => Some(ColorSpace::Rgb),
            _ => None,
        }
    }
}

/// An 8-bit channel as a percentage, rounded: the value read back sets
/// `round(p * 255 / 100)`, which reports the same percentage again.
fn percent(c: u8) -> u16 {
    (u16::from(c) * 100 + 127) / 255
}

/// A colour in DEC's HLS, rounded: hue with blue at 0, red at 120 and green
/// at 240 (`[[source: vt3xx-screen-control]]`), the inverse of the
/// double-hexcone conversion the Sixel decoder uses, a grey at hue 0.
pub fn rgb_to_hls(r: u8, g: u8, b: u8) -> (u16, u16, u16) {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let sum = max + min;
    let l = (sum * 100 + 255) / 510;
    let d = max - min;
    if d == 0 {
        return (0, l as u16, 0);
    }
    let s = match sum <= 255 {
        true => (d * 100 * 2 + sum) / (2 * sum),
        false => (d * 100 * 2 + (510 - sum)) / (2 * (510 - sum)),
    };
    // The standard hue, red at 0, in tenths of a degree, then DEC's.
    let h = if max == r {
        (600 * (g - b) / d).rem_euclid(3600)
    } else if max == g {
        600 * (b - r) / d + 1200
    } else {
        600 * (r - g) / d + 2400
    };
    let h = ((h + 5) / 10 + 120) % 360;
    (h as u16, l as u16, s.min(100) as u16)
}

/// One DECCTR group, `Pc;Pu;Px;Py;Pz`, for colour `pc`, into `out`; the
/// bytes written.
pub fn color_group(pc: u8, (r, g, b): (u8, u8, u8), space: ColorSpace, out: &mut [u8]) -> usize {
    let (pu, x, y, z) = match space {
        ColorSpace::Hls => {
            let (h, l, s) = rgb_to_hls(r, g, b);
            (1, h, l, s)
        }
        ColorSpace::Rgb => (2, percent(r), percent(g), percent(b)),
    };
    let mut n = 0;
    for (i, v) in [u16::from(pc), pu, x, y, z].into_iter().enumerate() {
        if i > 0 {
            out[n] = b';';
            n += 1;
        }
        let mut d = [0u8; 3];
        let mut k = 0;
        let mut v = v;
        loop {
            d[k] = b'0' + (v % 10) as u8;
            k += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        while k > 0 {
            k -= 1;
            out[n] = d[k];
            n += 1;
        }
    }
    n
}

/// The longest DECCTR group, `255;1;360;100;100`, and its `/`.
pub const COLOR_GROUP_MAX: usize = 17 + 1;

/// The longest DECCTR, every colour known.
pub const CTR_REPLY_MAX: usize = 2 + 3 + 256 * COLOR_GROUP_MAX + 2;

/// A DECRSTS 2 body as it arrives: the five parameters of the group being
/// read, and whether an invalid group has stopped the restore.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorRestore {
    /// Reading a group: `n` parameters ended, `cur` the one being read.
    Group {
        p: [u16; 5],
        n: u8,
        cur: Option<u16>,
    },
    /// An invalid group was met: the rest is ignored (the VT340's, VT420's
    /// and VT510's "ignores the rest of the sequence").
    Stopped,
}

/// A colour a DECRSTS 2 group sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorEntry {
    pub index: u8,
    pub rgb: (u8, u8, u8),
}

impl ColorRestore {
    pub const START: ColorRestore = ColorRestore::Group {
        p: [0; 5],
        n: 0,
        cur: None,
    };

    /// One body byte: the next state, and the colour a `/` completed.
    pub fn put(self, b: u8) -> (ColorRestore, Option<ColorEntry>) {
        let ColorRestore::Group { mut p, mut n, cur } = self else {
            return (self, None);
        };
        match b {
            b'0'..=b'9' => {
                let v = cur
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(u16::from(b - b'0'));
                (ColorRestore::Group { p, n, cur: Some(v) }, None)
            }
            b';' if usize::from(n) < 4 => {
                let Some(v) = cur else {
                    return (ColorRestore::Stopped, None);
                };
                p[usize::from(n)] = v;
                n += 1;
                (ColorRestore::Group { p, n, cur: None }, None)
            }
            b'/' => match (ColorRestore::Group { p, n, cur }).end() {
                Some(e) => (ColorRestore::START, Some(e)),
                None => (ColorRestore::Stopped, None),
            },
            _ => (ColorRestore::Stopped, None),
        }
    }

    /// The group ended by a `/` or the terminator, if it is a whole and
    /// valid one: `Pc` 0-255, `Pu` 1 or 2, each value in its range.
    pub fn end(self) -> Option<ColorEntry> {
        let ColorRestore::Group { mut p, n, cur } = self else {
            return None;
        };
        if n != 4 {
            return None;
        }
        p[4] = cur?;
        let index = u8::try_from(p[0]).ok()?;
        let px = match p[1] {
            1 => crate::sixel::hls(p[2], p[3], p[4])?,
            2 if p[2] <= 100 && p[3] <= 100 && p[4] <= 100 => crate::sixel::rgb(p[2], p[3], p[4]),
            _ => return None,
        };
        let (r, g, b) = px.rgb();
        Some(ColorEntry {
            index,
            rgb: (r.get(), g.get(), b.get()),
        })
    }

    /// Whether a group is open with nothing read yet: the body ended on a
    /// `/` or is empty.
    pub fn is_fresh(self) -> bool {
        self == ColorRestore::START
    }
}
