use crate::*;

/// Which presentation state report DECRQPSR asks for, and which format a
/// DECRSPS restores from.
///
/// `[[source: dec-std-070]]` p. 5-198: "0 ignored, no report sent / 1 Cursor
/// Information Report (DECCIR) / 2 Tabulation Stop Report (DECTABSR)"; and
/// p. 5-205 for DECRSPS, "0 error, restore ignored". `[[source:
/// vt510-decrqpsr]]` and `[[source: vt510-decrsps]]` list the same three.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentationState {
    /// DECCIR, `Ps = 1`.
    CursorInformation,
    /// DECTABSR, `Ps = 2`.
    TabStops,
}

impl PresentationState {
    /// The report `Ps` names, or `None` for 0 and for a value neither text
    /// defines, which both ignore.
    pub fn from_param(ps: u16) -> Option<PresentationState> {
        match ps {
            1 => Some(PresentationState::CursorInformation),
            2 => Some(PresentationState::TabStops),
            _ => None,
        }
    }
}

/// One of DECCIR's bit-encoded bytes, the bits below the `0x40` that every
/// one carries: "bit 8: always reset (0) bit 7: always set (1) bit 6:
/// extension indicator" (`[[source: dec-std-070]]` pp. 5-200 to 5-202).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CirBits(u8);

impl CirBits {
    pub const NONE: CirBits = CirBits(0);
    /// Srend bit 1, bold; Satt bit 1, the selectively erasable attribute;
    /// Sflag bit 1, origin mode.
    pub const BIT1: CirBits = CirBits(1);
    /// Srend bit 2, underline; Sflag bit 2, an SS2 pending.
    pub const BIT2: CirBits = CirBits(2);
    /// Srend bit 3, blink; Sflag bit 3, an SS3 pending.
    pub const BIT3: CirBits = CirBits(4);
    /// Srend bit 4, reverse video; Sflag bit 4, a pending wrap.
    pub const BIT4: CirBits = CirBits(8);

    pub fn with(self, b: CirBits, on: ModeChange) -> CirBits {
        match on {
            ModeChange::Set => CirBits(self.0 | b.0),
            ModeChange::Reset => CirBits(self.0 & !b.0),
        }
    }
    pub fn has(self, b: CirBits) -> bool {
        self.0 & b.0 == b.0
    }
    /// The byte on the wire.
    pub fn wire(self) -> u8 {
        0x40 | self.0
    }
}

/// DECCIR's data, `Pr;Pc;Pp;Srend;Satt;Sflag;Pgl;Pgr;Scss;Sdesig`.
///
/// `[[source: dec-std-070]]` p. 5-200: "a string of parameters containing
/// the information saved through a Save Cursor (DECSC) command"; and
/// `[[source: vt510-deccir]]`: "DECCIR reports the status of the cursor
/// position, including visual attributes and character protection
/// attributes ... the status of origin mode (DECOM) and the current active
/// character sets", so the current state, of the kinds DECSC saves. `Pp` is
/// always 1, "in a single page configuration, the Pp parameter would be 1",
/// and `Scss` is read off the designations' sizes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorInformation {
    pub row: OneBased,
    pub col: OneBased,
    pub rendition: CirBits,
    pub attribute: CirBits,
    pub flags: CirBits,
    pub gl: GSlot,
    pub gr: GSlot,
    pub designations: [Designation; 4],
}

/// The longest DECCIR data string: two five-digit positions, `1`, three
/// flag bytes, two slot digits, the sizes byte, nine separators and four
/// two-byte tails.
pub const CIR_DATA_MAX: usize = 5 + 5 + 1 + 3 + 2 + 1 + 9 + 4 * 2;

impl CursorInformation {
    /// `Scss`: bit n+1 set when Gn holds a 96-character set.
    pub fn sizes(&self) -> CirBits {
        let mut b = 0;
        let mut i = 0;
        while i < 4 {
            if self.designations[i].size() == SetSize::Chars96 {
                b |= 1 << i;
            }
            i += 1;
        }
        CirBits(b)
    }

    /// The data string, into `out`; the bytes written. `out` holds
    /// [`CIR_DATA_MAX`].
    pub fn encode(&self, out: &mut [u8; CIR_DATA_MAX]) -> usize {
        let mut n = 0;
        let mut push = |b: u8| {
            out[n] = b;
            n += 1;
        };
        let num = |v: u16, push: &mut dyn FnMut(u8)| {
            let mut d = [0u8; 5];
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
                push(d[k]);
            }
        };
        num(self.row.get(), &mut push);
        push(b';');
        num(self.col.get(), &mut push);
        push(b';');
        push(b'1');
        push(b';');
        push(self.rendition.wire());
        push(b';');
        push(self.attribute.wire());
        push(b';');
        push(self.flags.wire());
        push(b';');
        push(b'0' + self.gl.index() as u8);
        push(b';');
        push(b'0' + self.gr.index() as u8);
        push(b';');
        push(self.sizes().wire());
        push(b';');
        for d in &self.designations {
            for &b in d.tail() {
                push(b);
            }
        }
        n
    }
}

/// What a DECRSPS in DECCIR's format restores: each field up to the first
/// invalid one, `None` from there on.
///
/// `[[source: vt510-decrsps]]`: "If there is an invalid value in the DECRSPS
/// sequence, then the terminal ignores the rest of the sequence. This action
/// may leave the terminal in a partially restored state." `[[source:
/// dec-std-070]]` p. 5-205 has the terminal "ignore the error, if possible,
/// and if not ... ignore the remainder of the data"; the published manual
/// and xterm's `restore_DECCIR` stop at the first.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CirRestore {
    pub row: Option<OneBased>,
    pub col: Option<OneBased>,
    pub rendition: Option<CirBits>,
    pub attribute: Option<CirBits>,
    pub flags: Option<CirBits>,
    pub gl: Option<GSlot>,
    pub gr: Option<GSlot>,
    pub designations: [Option<Designation>; 4],
}

impl CirRestore {
    /// Read DECCIR's data string for a screen `rows` by `cols`.
    ///
    /// A position must name a cell of the screen and the page must be 1. A
    /// flag byte is `0x40` plus its bits, with the extension bit clear (the
    /// VT420 "always reset" it, and this core writes none) and the reserved
    /// bits clear. `Scss` decides which series each final of `Sdesig` is
    /// from, since `A` is the UK set among the 94-character finals and ISO
    /// Latin-1 Supplemental among the 96 (`[[source: ecma35]]` NOTE 44); a
    /// 96-character set cannot be G0 (14.3.2).
    pub fn parse(data: &[u8], rows: Rows, cols: Cols) -> CirRestore {
        let mut r = CirRestore::default();
        let mut it = Fields { data, at: 0 };
        let Some(v) = it
            .number()
            .filter(|&v| v >= 1 && usize::from(v) <= rows.index())
        else {
            return r;
        };
        r.row = Some(OneBased(v));
        let Some(v) = it
            .number()
            .filter(|&v| v >= 1 && usize::from(v) <= cols.index())
        else {
            return r;
        };
        r.col = Some(OneBased(v));
        if it.number() != Some(1) {
            return r;
        }
        let Some(b) = it.byte().filter(|b| b & 0xf0 == 0x40) else {
            return r;
        };
        r.rendition = Some(CirBits(b & 0x0f));
        let Some(b) = it.byte().filter(|b| b & 0xfe == 0x40) else {
            return r;
        };
        r.attribute = Some(CirBits(b & 0x01));
        let Some(b) = it.byte().filter(|b| b & 0xf0 == 0x40) else {
            return r;
        };
        r.flags = Some(CirBits(b & 0x0f));
        let Some(g) = it.slot() else { return r };
        r.gl = Some(g);
        let Some(g) = it.slot() else { return r };
        r.gr = Some(g);
        let Some(sizes) = it.byte().filter(|b| b & 0xf0 == 0x40) else {
            return r;
        };
        for i in 0..4 {
            let i2 = match it.peek() {
                Some(b @ 0x20..=0x2f) => {
                    it.at += 1;
                    Some(StreamByte::from_stream(b))
                }
                _ => None,
            };
            let Some(f) = it.peek().filter(|b| (0x30..=0x7e).contains(b)) else {
                return r;
            };
            it.at += 1;
            let f = StreamByte::from_stream(f);
            let d = match (sizes >> i) & 1 {
                1 if i == 0 || i2.is_some() => return r,
                1 => Designation::of_96(f),
                _ => Designation::of_94(i2, f),
            };
            r.designations[i] = Some(d);
        }
        r
    }
}

/// A cursor over DECCIR's `;`-separated fields.
struct Fields<'a> {
    data: &'a [u8],
    at: usize,
}

impl Fields<'_> {
    fn peek(&self) -> Option<u8> {
        self.data.get(self.at).copied()
    }
    /// The separator after a field: required, and consumed.
    fn end(&mut self) -> Option<()> {
        match self.peek() {
            Some(b';') => {
                self.at += 1;
                Some(())
            }
            _ => None,
        }
    }
    fn number(&mut self) -> Option<u16> {
        let mut v: u16 = 0;
        let start = self.at;
        while let Some(b @ b'0'..=b'9') = self.peek() {
            v = v.checked_mul(10)?.checked_add(u16::from(b - b'0'))?;
            self.at += 1;
        }
        if self.at == start {
            return None;
        }
        self.end().map(|()| v)
    }
    fn byte(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.at += 1;
        self.end().map(|()| b)
    }
    fn slot(&mut self) -> Option<GSlot> {
        let b = self.byte()?;
        GSlot::from_index(usize::from(b.checked_sub(b'0')?))
    }
}

/// The longest DECTABSR a screen `cols` wide can send: a stop at every
/// column, `/` between them, inside the longer seven-bit frame (`ESC P 2 $ u`
/// and `ESC \`). The core sizes its reply buffer to it.
pub const fn tab_report_max(cols: usize) -> usize {
    let mut n = 2 + 3 + 2;
    let mut c = 1;
    while c <= cols {
        n += digits(c) + 1;
        c += 1;
    }
    n - 1
}

/// How many decimal digits `v` takes.
const fn digits(v: usize) -> usize {
    let mut n = 1;
    let mut v = v / 10;
    while v > 0 {
        n += 1;
        v /= 10;
    }
    n
}

/// DECTABSR, `DCS 2 $ u D...D ST`, into `out`: "a string of numeric
/// parameters specifying where tabulation stops are currently set in the
/// terminal, separated by slash ("/") characters" (`[[source: dec-std-070]]`
/// p. 5-204), each a column from 1, `tabs[i]` the stop at column `i + 1`.
/// **Whole or nothing**: `None`, and nothing written, when `out` cannot hold
/// it all, since a truncated reply is a wrong answer.
pub fn encode_tab_report(tabs: &[bool], c1: C1Transmit, out: &mut [u8]) -> Option<ByteLen> {
    let mut len = c1.dcs().len() + 3 + c1.st().len();
    for (k, (i, _)) in tabs.iter().enumerate().filter(|(_, t)| **t).enumerate() {
        len += digits(i + 1) + usize::from(k > 0);
    }
    if len > out.len() {
        return None;
    }
    let mut w = 0;
    let mut put = |bytes: &[u8]| {
        out[w..w + bytes.len()].copy_from_slice(bytes);
        w += bytes.len();
    };
    put(c1.dcs());
    put(b"2$u");
    for (k, (i, _)) in tabs.iter().enumerate().filter(|(_, t)| **t).enumerate() {
        if k > 0 {
            put(b"/");
        }
        let mut d = [0u8; 20];
        let mut n = 0;
        let mut v = i + 1;
        loop {
            d[19 - n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        put(&d[20 - n..]);
    }
    put(c1.st());
    Some(ByteLen::from_index(len))
}

/// Where a DECRSPS in DECTABSR's format stands as its body arrives: reading
/// a column, a column just ended by `/`, or stopped at an invalid value
/// (`[[source: vt510-decrsps]]`: "If there is an invalid value in the
/// DECRSPS sequence, then the terminal ignores the rest of the sequence").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabRestore {
    Column(u16),
    /// A column ended by `/`, for the caller to set or refuse.
    Ended(OneBased),
    Stopped,
}

impl TabRestore {
    /// A body's first byte is the first digit of a column.
    pub const START: TabRestore = TabRestore::Column(0);

    /// The state after body byte `b`: a digit extends the column, `/` ends
    /// it, and anything else, or a column of 0 or past `u16`, is an invalid
    /// value.
    pub fn put(self, b: StreamByte) -> TabRestore {
        let TabRestore::Column(v) = self else {
            return TabRestore::Stopped;
        };
        match b.raw_byte() {
            d @ b'0'..=b'9' => match v
                .checked_mul(10)
                .and_then(|v| v.checked_add(u16::from(d - b'0')))
            {
                Some(v) => TabRestore::Column(v),
                None => TabRestore::Stopped,
            },
            b'/' if v > 0 => TabRestore::Ended(OneBased(v)),
            _ => TabRestore::Stopped,
        }
    }

    /// The column the body's last byte ended, at its terminator, if any.
    pub fn last(self) -> Option<OneBased> {
        match self {
            TabRestore::Column(v) if v > 0 => Some(OneBased(v)),
            _ => None,
        }
    }
}
