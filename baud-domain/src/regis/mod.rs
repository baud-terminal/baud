//! ReGIS, the REmote Graphics Instruction Set: `[[source: dec-std-070]]`
//! 070-8, the ReGIS Graphics Extension, read a character at a time.
//!
//! **Three layers, as the text has them.** Below everything, the macrograph
//! layer: § 8.4.4 calls macrographs "extra-syntactic", defined and invoked
//! "anywhere in a ReGIS command stream without affecting the state of the
//! ReGIS parser", so it sits under the lexer and sees every character but
//! those inside a quoted string. Then the lexer, § 8.4's general grammar --
//! keyletters, the four argument types (bracketed extents, quoted strings,
//! numerics, options) and the synchronising `;` -- which every device "must
//! completely interpret ... whether or not those character sequences have
//! semantic meaning" (§ 8.2.1.2). Above it the instructions of § 8.5 (Base),
//! § 8.7 (the Raster Extensions) and the parts of § 8.8 (the Open Extensions)
//! that are built, acting on the device state and drawing through a
//! [`RegisOut`].
//!
//! **State persists between envelopes.** Unlike a Sixel, whose state "is
//! not preserved from one entering of Sixel mode to the next" (070-9 § 10),
//! ReGIS resumes: `[[source: vt3xx-regis-intro]]`, "Enters ReGIS at the
//! point where ReGIS was last exited" for `DCS p` and `DCS 0p`, and 070-8
//! § 8.9.1 makes the low bit of the envelope's parameter an "environmental
//! resynchronization" -- the one thing an envelope may reset.
//!
//! **Every table is bounded, and every refusal counted** ([`RegisCounts`]):
//! the macrograph store, the macrograph nesting, the option nesting, the
//! position stack, the fill's vertex list and the loadable alphabets are
//! all attacker-supplied.

mod color;
mod num;
mod sem;
mod trig;

pub use color::*;
pub use num::RegisNum;
pub use sem::{RegisCounts, RegisSpace};

use crate::decode::StreamByte;
use num::{NumAcc, NumStep, Signed};
use sem::Sem;

/// The device a ReGIS picture is drawn for: 800 by 480 pixels, the VT330
/// and VT340 screen. `[[source: vt3xx-regis-intro]]`: "The VT300 screen
/// has 384,000 pixels arranged in 800 vertical columns by 480 horizontal
/// rows", and its default addressing `S(A[0,0][799,479])`, so a coordinate
/// is a pixel until a program imposes another space.
pub const REGIS_WIDTH: i32 = 800;
/// The device's height in pixels.
pub const REGIS_HEIGHT: i32 = 480;

/// Macrograph storage, in characters: `[[source: vt3xx-macrographs]]`,
/// "The VT300 can store at least 10,000 bytes of macrograph data", above
/// 070-8 § 8.4.4's "no less than 4000 characters".
pub const MACRO_STORE: usize = 10_000;
/// How deep one macrograph may invoke another:
/// `[[source: vt3xx-macrographs]]`, "You can nest macrographs up to 16
/// levels deep".
pub const MACRO_NESTING: usize = 16;
/// Option levels below a command: § 8.4.3.4, "ReGIS interpreters must be
/// prepared to accept recursion of options to at least six levels. A
/// conforming ReGIS generator must not exceed this level of recursion."
pub const OPTION_LEVELS: usize = 6;
/// Characters one host character may play through macrographs. The text
/// bounds neither the invocations a macrograph makes nor how often, and
/// sixteen levels of strings that each invoke the next many times expand
/// past any limit; 65,536 is more than a minute of input at 9600 baud, and
/// what is cut is counted.
pub const MACRO_PLAYED: usize = 65_536;
/// The position stack: `[[source: vt3xx-position]]`, "You can save up to
/// 16 positions in a stack", above § 8.4.5.4's "at least eight positions
/// deep".
pub const POSITION_STACK: usize = 16;

/// What a `DCS Pn p` asks of the interpreter: § 8.9.1's two flags in `Pn`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisEntry(u16);

impl RegisEntry {
    /// From the envelope's first parameter, absent read as 0.
    pub fn of(p: Option<u16>) -> RegisEntry {
        RegisEntry(p.unwrap_or(0))
    }

    /// The low bit: "the interpreter shall force an environmental
    /// resynchronization before interpreting any following ReGIS
    /// instructions" -- `DCS 1p` and `DCS 3p`, "Enters ReGIS at the command
    /// level. ReGIS begins at a new command" (`[[source: vt3xx-regis-intro]]`).
    pub fn resync(self) -> bool {
        self.0 & 1 != 0
    }

    /// The next bit: "should also pass along the source ReGIS stream to a
    /// text display process for display as text", "not mandatory for ReGIS
    /// interpreters" (§ 8.9.1) -- the VT340's command display on the bottom
    /// line (`DCS 2p`, `DCS 3p`).
    pub fn display(self) -> bool {
        self.0 & 2 != 0
    }
}

/// A time delay's length: `S(T<n>)`'s ticks, "60 ticks equal 1 second"
/// (`[[source: vt3xx-screen-control]]`), 070-8 § 8.7.1.4.3's "20ms +/- 20%".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct RegisTicks(u32);

impl RegisTicks {
    /// `n` ticks.
    pub fn of(n: u32) -> RegisTicks {
        RegisTicks(n)
    }

    /// The count of ticks.
    pub fn get(self) -> u32 {
        self.0
    }

    /// How long the ticks last, in microseconds: a sixtieth of a second
    /// each.
    pub fn micros(self) -> u64 {
        u64::from(self.0) * 1_000_000 / 60
    }
}

/// A device pixel of the page, `x` rightward and `y` downward from its
/// upper-left. Off the page is allowed: what is drawn there is clipped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, PartialOrd, Ord)]
pub struct RegisPoint {
    pub x: i32,
    pub y: i32,
}

/// How a drawing primitive writes (§ 8.7.2.4.1): "Four of the writing modes
/// (C = complement, E = erase, R = replace, and V = overlay) are mutually
/// exclusive, each overriding all others".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WriteMode {
    /// `W(V)`: "overlay ("OR") input onto current image. Blanks ... have no
    /// effect" -- the default.
    Overlay,
    /// `W(R)`: ones write the foreground, zeros the background.
    Replace,
    /// `W(C)`: "use ones in the pattern register or cell to complement the
    /// contents of the pixels present in the image".
    Complement,
    /// `W(E)`: "ignore the contents of the pattern register, and Erase each
    /// pixel to the background specifier if W(N0) is in force, or write each
    /// pixel with the foreground specifier if W(N1) is in force".
    Erase,
}

/// `W(N)`: whether the pattern's sense is inverted (§ 8.7.2.4.1.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Negate {
    Off,
    On,
}

/// An index into the output map: what a pixel of the page holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash, PartialOrd, Ord)]
pub struct RegisIndex(u8);

impl RegisIndex {
    /// The background a page starts with and `S(I0)` names.
    pub const ZERO: RegisIndex = RegisIndex(0);

    /// § 8.6.2.2.1: "when an index exceeds the bounds of the device, the
    /// unimplemented index must be mapped back into the supported range ...
    /// a modulus scheme should be used".
    pub fn wrap(n: i64) -> RegisIndex {
        RegisIndex(n.rem_euclid(REGIS_COLORS as i64) as u8)
    }

    /// The index as a map slot.
    pub fn slot(self) -> usize {
        usize::from(self.0)
    }

    /// The raw index, for a plane of bytes.
    pub fn raw(self) -> u8 {
        self.0
    }

    /// An index from a plane's byte, its low bits.
    pub fn of_raw(b: u8) -> RegisIndex {
        RegisIndex(b % REGIS_COLORS as u8)
    }
}

/// `W(F)`: the planes a write may change (§ 8.7.2.4.2.1), a bit each.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisPlanes(u8);

impl RegisPlanes {
    /// Every plane: the default, "W(F<n>) with n=(2**m)-1".
    pub const ALL: RegisPlanes = RegisPlanes(REGIS_COLORS as u8 - 1);

    /// From `W(F<n>)`, held to the planes there are.
    pub fn of(n: i64) -> RegisPlanes {
        RegisPlanes((n.rem_euclid(256) as u8) & RegisPlanes::ALL.0)
    }

    /// The mask.
    pub fn bits(self) -> u8 {
        self.0
    }
}

/// A line pattern: the VT300's "8-bit pattern memory", first bit first
/// (`[[source: vt3xx-write-control]]`; 070-8 § 8.5.3.4.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LinePattern(u8);

impl LinePattern {
    /// `P1`, a solid line, the default.
    pub const SOLID: LinePattern = LinePattern(0xff);
    /// `P0`, "no visible foreground image specified".
    pub const NONE: LinePattern = LinePattern(0);

    /// The standard pattern `n`, 2 to 9: "represented in an eight bit
    /// pattern length" (§ 8.5.3.4.2).
    pub fn standard(n: u8) -> Option<LinePattern> {
        Some(LinePattern(match n {
            0 => 0,
            1 => 0xff,
            2 => 0b1111_0000,
            3 => 0b1110_0100,
            4 => 0b1010_1010,
            5 => 0b1110_1010,
            6 => 0b1000_1000,
            7 => 0b1000_0100,
            8 => 0b1100_1000,
            9 => 0b1000_0110,
            _ => return None,
        }))
    }

    /// A pattern from its binary digits, first first, as the VT300 loads
    /// its pattern memory: "If you specify more than 8 bits, the terminal
    /// uses only the last 8 bits of your pattern. If you specify less than 8
    /// bits, the terminal repeats as much of your pattern as it can in the
    /// remaining bits" -- of the two choices 070-8 allows, "either the first
    /// <k> bits specified or the last <k> bits", the device's.
    pub fn binary(digits: &[u8]) -> LinePattern {
        if digits.is_empty() {
            return LinePattern::SOLID;
        }
        let tail = &digits[digits.len().saturating_sub(8)..];
        let mut bits = 0u8;
        for k in 0..8 {
            bits = (bits << 1) | u8::from(tail[k % tail.len()] == b'1');
        }
        LinePattern(bits)
    }

    /// The bit at step `i` of the pattern, each element `mul` steps long.
    pub fn bit(self, i: u32, mul: u32) -> bool {
        let k = (i / mul.max(1)) % 8;
        self.0 & (0x80 >> k) != 0
    }

    /// The eight bits, first most significant.
    pub fn bits(self) -> u8 {
        self.0
    }
}

/// Where shading is drawn to (§ 8.7.2.4.2.3 and § 8.8.3.3): a horizontal
/// line, a vertical line, or a point, in device pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShadeTo {
    /// `W(S(Y))`, the default: to the row `y`.
    Row(i32),
    /// `W(S(X))`: to the column `x`.
    Column(i32),
    /// `W(S(P))`: to the point.
    Point(RegisPoint),
}

/// What an area is textured with: the shading "snapshot" of § 8.7.2.4.2.3,
/// a linear pattern turned through a right angle or a character cell's
/// pattern, which polygon fill uses too (§ 8.5.8.4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Texture {
    /// Solid foreground.
    Solid,
    /// A line pattern and its element multiplier.
    Line(LinePattern, u16),
    /// The top square part of a character's cell, at the unit cell's
    /// granularity: the cell's rows as bits, most significant leftmost.
    Cell {
        rows: [u16; 16],
        /// The cell's side in stored pixels.
        side: u8,
        /// Device pixels a stored pixel spans.
        scale: u16,
    },
}

/// Everything a primitive draws with: the effective writing controls of
/// the command, resolved to the device.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisPen {
    pub mode: WriteMode,
    pub negate: Negate,
    pub fg: RegisIndex,
    pub bg: RegisIndex,
    pub planes: RegisPlanes,
    pub pattern: LinePattern,
    pub pattern_mul: u16,
    /// Shading, when enabled: where to, and with what.
    pub shade: Option<(ShadeTo, Texture)>,
}

/// A character to draw (§ 8.5.6 and § 8.7.3): its stored cell, where its
/// cell's upper-left is, and how large and at what angles it is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisGlyph<'a> {
    /// What the character is.
    pub source: GlyphSource<'a>,
    /// The upper-left of its display cell, before any turn.
    pub at: RegisPoint,
    /// The unit cell, where the image is drawn, in device pixels.
    pub unit: (i32, i32),
    /// The display cell, the area written, in device pixels.
    pub display: (i32, i32),
    /// The baseline's direction, degrees counterclockwise from the right.
    pub direction: i32,
    /// The italic slant, degrees (§ 8.7.3.4.6).
    pub slant: i32,
}

/// Where a character's image comes from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GlyphSource<'a> {
    /// A character of the built-in alphabet 0, by Unicode scalar.
    Builtin(char),
    /// A loaded cell: its rows, most significant bit leftmost, `w` wide.
    Loaded { rows: &'a [u16], w: u8 },
    /// The error character § 8.7.3.3 asks for: "Solid block characters or
    /// checkerboard characters are acceptable".
    Error,
}

/// What the interpreter does to the world: draw, answer, wait.
///
/// The drawing primitives are in device pixels, already mapped from the
/// imposed coordinate space and clipped by nothing; a page clips them.
pub trait RegisOut {
    /// A line from `a` to `b`, both ends included, through `pen`, the
    /// pattern stepping on from `phase`.
    fn line(&mut self, a: RegisPoint, b: RegisPoint, pen: &RegisPen, phase: &mut u32);
    /// A filled area: `points` in contours ending at each of `ends`, the
    /// interior by the even-odd rule (§ 8.5.8.4.1).
    fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen);
    /// A character.
    fn glyph(&mut self, g: &RegisGlyph<'_>, pen: &RegisPen);
    /// `S(E)`: the whole page to `bg`.
    fn erase(&mut self, bg: RegisIndex);
    /// An output map entry changed: every pixel holding it shows the new
    /// colour.
    fn recolor(&mut self);
    /// Bytes for the host, a report whole.
    fn reply(&mut self, bytes: &[u8]);
    /// `S(T<n>)`: suspend interpretation for `ticks`.
    fn delay(&mut self, ticks: RegisTicks);
}

/// The macrograph store: 26 strings in one pool of [`MACRO_STORE`]
/// characters, each "of any length from no characters up to the defined
/// storage limit" (§ 8.4.4).
#[derive(Clone)]
struct Macros {
    pool: [u8; MACRO_STORE],
    used: usize,
    /// Where each letter's string starts in `pool`, and how long it is.
    at: [(u16, u16); 26],
}

impl Macros {
    const NEW: Macros = Macros {
        pool: [0; MACRO_STORE],
        used: 0,
        at: [(0, 0); 26],
    };

    fn get(&self, m: usize, i: usize) -> Option<u8> {
        let (s, n) = self.at[m];
        (i < usize::from(n)).then(|| self.pool[usize::from(s) + i])
    }

    /// Empty one string, closing the gap it leaves.
    fn clear(&mut self, m: usize) {
        let (s, n) = self.at[m];
        let (s, n) = (usize::from(s), usize::from(n));
        if n == 0 {
            self.at[m] = (0, 0);
            return;
        }
        self.pool.copy_within(s + n..self.used, s);
        self.used -= n;
        for e in self.at.iter_mut() {
            if usize::from(e.0) > s {
                e.0 -= n as u16;
            }
        }
        self.at[m] = (0, 0);
    }

    /// Begin defining `m`: "The first action taken upon the occurrence of a
    /// macrograph definition initiator must be to clear the contents of the
    /// specified macrograph" (§ 8.4.4). It goes at the pool's end, where the
    /// one string being defined always is.
    fn begin(&mut self, m: usize) {
        self.clear(m);
        self.at[m] = (self.used as u16, 0);
    }

    /// Append to `m`, the string being defined; `false` once the pool is
    /// full.
    fn push(&mut self, m: usize, c: u8) -> bool {
        if self.used >= MACRO_STORE {
            return false;
        }
        self.pool[self.used] = c;
        self.used += 1;
        self.at[m].1 += 1;
        true
    }

    fn clear_all(&mut self) {
        self.used = 0;
        self.at = [(0, 0); 26];
    }

    fn free(&self) -> usize {
        MACRO_STORE - self.used
    }
}

/// Where the macrograph layer is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum At {
    /// No `@` pending.
    Idle,
    /// After an `@`, outside a quoted string and a definition.
    Pending,
    /// After `@:`, waiting for the name.
    Name,
    /// In a definition's body.
    Body { m: u8, at: Seen, full: Seen },
    /// In a definition whose name was no letter: kept nowhere, to its `@;`.
    Nameless { at: Seen },
}

/// Whether a thing was seen: an `@` in a body, or the pool filling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Seen {
    No,
    Yes,
}

/// A macrograph being played: which, and how far.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Play {
    m: u8,
    i: u16,
}

/// Where the lexer is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lex {
    Ground,
    /// A number in a numeric context, its digits kept as they came for
    /// `W(P)`, whose digits are each an argument (§ 8.5.3.4.2).
    Number {
        acc: NumAcc,
        digits: [u8; 16],
        n: u8,
    },
    /// Inside a quoted string opened by `q`.
    Quote {
        q: u8,
    },
    /// After a quote that may close the string or, doubled, be in it.
    QuoteEnd {
        q: u8,
    },
    /// Inside a bracketed extent.
    Bracket(Bracket),
}

/// One part of a bracketed extent (§ 8.4.5.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Part {
    /// Missing: "interpreted to mean +0" where the extent is a position.
    #[default]
    Absent,
    /// Unsigned: "the value of the coordinate part is the new number".
    Abs(RegisNum),
    /// Signed: "evaluated relative to the current drawing position".
    Rel(RegisNum),
}

impl Part {
    /// The value whatever its sign, absent as `None`: what a non-positional
    /// extent reads (§ 8.4.5.2).
    pub fn value(self) -> Option<RegisNum> {
        match self {
            Part::Absent => None,
            Part::Abs(v) | Part::Rel(v) => Some(v),
        }
    }

    /// The coordinate this part names from `cur`.
    pub fn from(self, cur: RegisNum) -> RegisNum {
        match self {
            Part::Absent => cur,
            Part::Abs(v) => v,
            Part::Rel(v) => cur.plus(v),
        }
    }
}

/// A bracketed extent being read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Bracket {
    x: Part,
    y: Part,
    acc: NumAcc,
    /// Which part is being read: 0 the x, 1 the y, 2 and on ignored.
    part: u8,
    /// The part met a character no number takes, and is not present.
    spoiled: Seen,
}

/// A whole bracketed extent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Extent {
    pub x: Part,
    pub y: Part,
}

/// What a digit outside a bracket means where it is (§ 8.4.3.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DigitMode {
    /// A number.
    Numeric,
    /// "Where digit strings are used as pixel vectors ... the sign
    /// characters and the decimal point are ignored without effect", and
    /// the `E` is a keyletter.
    Vector,
    /// The Load command's cell data, hexadecimal (§ 8.4.3.3.2), where `A`
    /// to `F` are digits.
    Hex,
}

/// The whole of ReGIS: the macrograph layer, the lexer and the device
/// state. One per terminal, persisting between envelopes.
#[derive(Clone)]
pub struct Regis {
    macros: Macros,
    at: At,
    play: [Play; MACRO_NESTING],
    depth: usize,
    lex: Lex,
    /// Characters a finished token gave back, read before the next.
    again: [u8; 3],
    nagain: usize,
    sem: Sem,
}

impl Regis {
    /// Power-up: the defaults of `[[source: vt3xx-regis-intro]]` Table 1-2, no
    /// macrograph and nothing loaded.
    pub const NEW: Regis = Regis {
        macros: Macros::NEW,
        at: At::Idle,
        play: [Play { m: 0, i: 0 }; MACRO_NESTING],
        depth: 0,
        lex: Lex::Ground,
        again: [0; 3],
        nagain: 0,
        sem: Sem::NEW,
    };

    /// A reset to power-up: `RIS`. The page is the caller's to clear.
    pub fn reset(&mut self) {
        self.macros.clear_all();
        self.at = At::Idle;
        self.depth = 0;
        self.lex = Lex::Ground;
        self.nagain = 0;
        self.sem = Sem::NEW;
    }

    /// A `DCS Pn p`: the low bit of `Pn` resynchronises, "regardless of the
    /// previous state of the interpreter, and returns the interpreter to the
    /// highest command level with no quoted strings or macrograph
    /// definitions in process" (§ 8.9.1); without it ReGIS resumes where it
    /// was left.
    pub fn enter(&mut self, e: RegisEntry, out: &mut dyn RegisOut) {
        self.sem.counts.entered += 1;
        if e.display() {
            self.sem.counts.display_unbuilt += 1;
        }
        if e.resync() {
            self.resync(out);
        }
    }

    /// "Environmental resynchronization will force resynchronization even
    /// from within a quoted string argument or a macrograph definition.
    /// Semicolon resynchronization can do neither of these" (§ 8.4.2).
    fn resync(&mut self, out: &mut dyn RegisOut) {
        self.at = At::Idle;
        self.depth = 0;
        self.lex = Lex::Ground;
        self.nagain = 0;
        self.sem.sync(out);
    }

    /// One character of a ReGIS body, and any macrograph it invokes.
    pub fn put(&mut self, b: StreamByte, out: &mut dyn RegisOut) {
        self.layer(b.raw_byte(), Source::Host, out);
        self.drain(out);
    }

    /// Play the macrographs invoked, a character at a time, until none is,
    /// or [`MACRO_PLAYED`] have played.
    fn drain(&mut self, out: &mut dyn RegisOut) {
        let mut played = 0;
        while self.depth > 0 {
            if played == MACRO_PLAYED {
                self.depth = 0;
                self.sem.counts.macro_cut += 1;
                return;
            }
            let top = self.play[self.depth - 1];
            match self.macros.get(usize::from(top.m), usize::from(top.i)) {
                Some(c) => {
                    self.play[self.depth - 1].i += 1;
                    played += 1;
                    self.layer(c, Source::Macro, out);
                }
                None => self.depth -= 1,
            }
        }
    }

    /// The device state, for a report or a caller drawing the page.
    pub fn colors(&self) -> &[crate::image::Pixel; REGIS_COLORS] {
        &self.sem.map
    }

    /// What was refused and how often.
    pub fn counts(&self) -> RegisCounts {
        self.sem.counts
    }

    /// The current drawing position, in the imposed space.
    pub fn position(&self) -> (RegisNum, RegisNum) {
        (self.sem.pos.0, self.sem.pos.1)
    }

    /// The imposed coordinate space (`S(A)`).
    pub fn space(&self) -> RegisSpace {
        self.sem.space
    }

    /// The background index `S(I)` names.
    pub fn background(&self) -> RegisIndex {
        self.sem.bg
    }

    /// The characters free in the macrograph store.
    pub fn macro_free(&self) -> usize {
        self.macros.free()
    }

    /// The macrograph layer: one character from the host or a playing
    /// macrograph.
    fn layer(&mut self, c: u8, src: Source, out: &mut dyn RegisOut) {
        match self.at {
            At::Body { m, at, full } => return self.define(m, at, full, c),
            At::Nameless { at } => {
                self.at = match (at, c) {
                    (Seen::Yes, b';') => At::Idle,
                    (_, b'@') => At::Nameless { at: Seen::Yes },
                    _ => At::Nameless { at: Seen::No },
                };
                return;
            }
            At::Name => {
                self.at = match c {
                    b'A'..=b'Z' | b'a'..=b'z' => {
                        let m = c.to_ascii_uppercase() - b'A';
                        self.macros.begin(usize::from(m));
                        At::Body {
                            m,
                            at: Seen::No,
                            full: Seen::No,
                        }
                    }
                    _ => {
                        self.sem.counts.macro_errors += 1;
                        At::Nameless { at: Seen::No }
                    }
                };
                return;
            }
            At::Pending => {
                self.at = At::Idle;
                return self.after_at(c, src, out);
            }
            At::Idle => {}
        }
        // **`@` is the macrograph layer's outside a quoted string**, and
        // inside one "the "@" character is treated as a literal character"
        // (§ 8.4.1 f). A quote that may be closing is closed first: "Were
        // <> to be replaced by a macrograph definition ... the quotes are NOT
        // doubled" (§ 8.4.3.2).
        if c == b'@' && !matches!(self.lex, Lex::Quote { .. }) {
            if let Lex::QuoteEnd { .. } = self.lex {
                self.lex = Lex::Ground;
                self.sem.quote_end(out);
            }
            self.at = At::Pending;
            return;
        }
        self.lexer(c, out);
    }

    /// The character after an `@`.
    fn after_at(&mut self, c: u8, src: Source, out: &mut dyn RegisOut) {
        match (c, src) {
            // "A macrograph string cannot contain the definition of another
            // macrograph string", and "An attempt to execute the global
            // clear (@.) from within a macrograph is an error"; recovery from
            // either "is not specified in this standard", and here it is to
            // do nothing and count it.
            (b':' | b'.', Source::Macro) => self.sem.counts.macro_errors += 1,
            (b':', Source::Host) => self.at = At::Name,
            (b'.', Source::Host) => self.macros.clear_all(),
            (b'A'..=b'Z' | b'a'..=b'z', _) => self.invoke(c.to_ascii_uppercase() - b'A'),
            (b'@', _) => self.at = At::Pending,
            // A lone `@`, or `@;` with no definition open: nothing.
            (b';', _) => self.sem.counts.macro_errors += 1,
            _ => {
                self.sem.counts.macro_errors += 1;
                self.layer(c, src, out);
            }
        }
    }

    /// `@<letter>`: play it, unless it is already playing -- "a macrograph
    /// string must not invoke itself either directly or indirectly. There
    /// are no conditional execution controls in ReGIS, therefore there is no
    /// termination mechanism for recursive invocation" (§ 8.4.4) -- or the
    /// nesting is full.
    fn invoke(&mut self, m: u8) {
        if self.play[..self.depth].iter().any(|p| p.m == m) {
            self.sem.counts.macro_recursion += 1;
            return;
        }
        if self.depth == MACRO_NESTING {
            self.sem.counts.macro_nesting += 1;
            return;
        }
        self.play[self.depth] = Play { m, i: 0 };
        self.depth += 1;
    }

    /// One character of a definition's body. "The macrograph definition
    /// terminator "@;" may not be divided by any character sequence ...
    /// There is no instruction parsing during a macrograph storage"
    /// (§ 8.4.1 i), so every character is kept, controls included, as the
    /// VT300 keeps "the four control characters BS, HT, CR, and LF".
    fn define(&mut self, m: u8, at: Seen, full: Seen, c: u8) {
        let mut full = full;
        let keep = |macros: &mut Macros, c: u8, full: &mut Seen| {
            if *full == Seen::No && !macros.push(usize::from(m), c) {
                *full = Seen::Yes;
            }
        };
        match (at, c) {
            (Seen::Yes, b';') => {
                // "fail soft": the part already stored stays, the rest is
                // discarded, "but the definition will terminate "normally""
                // (§ 8.4.4 a-c), and counted once a definition.
                if full == Seen::Yes {
                    self.sem.counts.macro_overflow += 1;
                }
                self.at = At::Idle;
            }
            (Seen::Yes, b':') => {
                // A definition inside a definition: "Determination of and
                // recovery from this error is not specified"; the two
                // characters are dropped and the definition goes on.
                self.sem.counts.macro_errors += 1;
                self.at = At::Body {
                    m,
                    at: Seen::No,
                    full,
                };
            }
            (Seen::Yes, b'@') => {
                keep(&mut self.macros, b'@', &mut full);
                self.at = At::Body {
                    m,
                    at: Seen::Yes,
                    full,
                };
            }
            (Seen::Yes, _) => {
                keep(&mut self.macros, b'@', &mut full);
                keep(&mut self.macros, c, &mut full);
                self.at = At::Body {
                    m,
                    at: Seen::No,
                    full,
                };
            }
            (Seen::No, b'@') => {
                self.at = At::Body {
                    m,
                    at: Seen::Yes,
                    full,
                }
            }
            (Seen::No, _) => {
                keep(&mut self.macros, c, &mut full);
                self.at = At::Body {
                    m,
                    at: Seen::No,
                    full,
                };
            }
        }
    }

    /// The lexer: one character, and any a finished token gave back.
    fn lexer(&mut self, c: u8, out: &mut dyn RegisOut) {
        self.sem.macro_free = self.macros.free();
        self.lex_one(c, out);
        while self.nagain > 0 {
            let c = self.again[0];
            self.again.copy_within(1.., 0);
            self.nagain -= 1;
            self.lex_one(c, out);
        }
    }

    fn give_back(&mut self, s: &[u8]) {
        for &c in s {
            if self.nagain < self.again.len() {
                self.again[self.nagain] = c;
                self.nagain += 1;
            }
        }
    }

    fn lex_one(&mut self, c: u8, out: &mut dyn RegisOut) {
        match self.lex {
            Lex::Quote { q } => {
                if c == q {
                    self.lex = Lex::QuoteEnd { q };
                } else {
                    self.sem.quote_char(c, out);
                }
            }
            Lex::QuoteEnd { q } => {
                if c == q {
                    // "the target quote character must be doubled".
                    self.lex = Lex::Quote { q };
                    self.sem.quote_char(q, out);
                } else if matches!(c, 0x08 | 0x09 | 0x0a | 0x0d | b' ') || !is_control(c) {
                    // "Should paired-quotes ... be separated by BS, CR, LF,
                    // HT, or SP, then they are NOT considered doubled".
                    self.lex = Lex::Ground;
                    self.sem.quote_end(out);
                    self.lex_one(c, out);
                }
                // "When separated by other controls, these controls are
                // interpreted as "no character"": still deciding.
            }
            Lex::Number {
                mut acc,
                mut digits,
                mut n,
            } => {
                if acc.step(c) == NumStep::Took {
                    // The last sixteen digits, which `W(P)` reads.
                    if c.is_ascii_digit() {
                        if usize::from(n) == digits.len() {
                            digits.copy_within(1.., 0);
                            n -= 1;
                        }
                        digits[usize::from(n)] = c;
                        n += 1;
                    }
                    self.lex = Lex::Number { acc, digits, n };
                    return;
                }
                if is_ignored(c) && c != b'.' {
                    // A space or a control inside a number: "Spaces are
                    // treated as control characters and may be included
                    // anywhere within the data stream without effect"
                    // (§ 8.4.1 h).
                    return;
                }
                self.lex = Lex::Ground;
                let back = acc.unread();
                if let Some(v) = acc.value() {
                    self.sem.number(v, &digits[..usize::from(n)], out);
                }
                self.give_back(back);
                self.give_back(&[c]);
            }
            Lex::Bracket(mut br) => self.bracket(&mut br, c, out),
            Lex::Ground => self.ground(c, out),
        }
    }

    fn bracket(&mut self, br: &mut Bracket, c: u8, out: &mut dyn RegisOut) {
        let finish = |br: &mut Bracket| {
            let part = match (br.spoiled, br.acc.value()) {
                (Seen::No, Some(v)) => match br.acc.sign {
                    Signed::None => Part::Abs(v),
                    _ => Part::Rel(v),
                },
                // A sign with no digit is a displacement of nothing.
                (Seen::No, None) if br.acc.sign != Signed::None => Part::Rel(RegisNum::ZERO),
                _ => Part::Absent,
            };
            match br.part {
                0 => br.x = part,
                1 => br.y = part,
                _ => {}
            }
        };
        match c {
            b']' => {
                finish(br);
                self.lex = Lex::Ground;
                self.sem.extent(Extent { x: br.x, y: br.y }, out);
            }
            b',' => {
                finish(br);
                br.part = br.part.saturating_add(1);
                if br.part == 2 {
                    // "only the first two parts are accepted; the third and
                    // any subsequent parts are ignored" -- error 3.
                    self.sem.error(3, 0);
                }
                br.acc = NumAcc::NEW;
                br.spoiled = Seen::No;
                self.lex = Lex::Bracket(*br);
            }
            b';' => {
                self.lex = Lex::Ground;
                self.sem.sync(out);
            }
            _ if is_ignored(c) => self.lex = Lex::Bracket(*br),
            _ => {
                if br.spoiled == Seen::No && br.acc.step(c) == NumStep::Ends {
                    // "Any non-numeric character other than a comma ...
                    // causes the part ... to be terminated and interpreted
                    // as not present" (§ 8.4.3.1).
                    br.spoiled = Seen::Yes;
                    self.sem.error(1, c);
                }
                self.lex = Lex::Bracket(*br);
            }
        }
    }

    fn ground(&mut self, c: u8, out: &mut dyn RegisOut) {
        match c {
            b';' => self.sem.sync(out),
            b'(' => self.sem.open(out),
            b')' => self.sem.close(out),
            b'[' => {
                self.lex = Lex::Bracket(Bracket {
                    x: Part::Absent,
                    y: Part::Absent,
                    acc: NumAcc::NEW,
                    part: 0,
                    spoiled: Seen::No,
                });
            }
            b'\'' | b'"' => {
                self.lex = Lex::Quote { q: c };
                self.sem.quote_begin(out);
            }
            b',' => self.sem.comma(out),
            _ if is_ignored(c) => {}
            _ => match self.sem.digit_mode() {
                DigitMode::Hex if c.is_ascii_hexdigit() => {
                    let v = (c as char).to_digit(16).unwrap_or(0) as u8;
                    self.sem.hex(v, out);
                }
                DigitMode::Vector if matches!(c, b'0'..=b'9' | b'+' | b'-' | b'.') => {
                    // "the sign characters and the decimal point are ignored
                    // without effect, and the interpretation of the digits
                    // takes place normally".
                    if c.is_ascii_digit() {
                        self.sem.vector(c - b'0', out);
                    }
                }
                DigitMode::Numeric | DigitMode::Hex if NumAcc::starts(c) => {
                    let mut digits = [0u8; 16];
                    let mut n = 0u8;
                    if c.is_ascii_digit() {
                        digits[0] = c;
                        n = 1;
                    }
                    self.lex = Lex::Number {
                        acc: NumAcc::begin(c),
                        digits,
                        n,
                    };
                }
                _ if c.is_ascii_alphabetic() || c == b'=' => {
                    self.sem.key(c.to_ascii_uppercase(), out);
                }
                _ => self.sem.error(1, c),
            },
        }
    }
}

/// Where a character came from: the host, or a macrograph playing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Source {
    Host,
    Macro,
}

/// A C0 control or `DEL`.
fn is_control(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}

/// What the lexer passes over outside a quoted string: "The blank (space)
/// character is ignored outside of quoted strings" and "ASCII control
/// characters ... are ignored in ReGIS" (§ 8.4.1 h, i). A byte past `7F`
/// is no character of ReGIS's alphabet, "Columns 2 through 5 of the ASCII
/// Code Table" (§ 8.4.1), and is passed over too.
fn is_ignored(c: u8) -> bool {
    is_control(c) || c == b' ' || c >= 0x80
}
