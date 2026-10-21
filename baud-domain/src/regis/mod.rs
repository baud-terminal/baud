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

mod canvas;
mod color;
mod cursor;
mod num;
mod sem;
mod trig;

pub use canvas::*;
pub use color::*;
pub use cursor::{
    REGIS_CURSOR_CELL, RegisCursor, RegisCursorPair, RegisCursorStyle, RegisCursorTone, RegisDrawn,
    RegisInput, RegisInputMode,
};
pub use num::RegisNum;
pub use sem::{RegisCounts, RegisSpace};

use crate::decode::{InputEncoding, StreamByte};
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

/// The largest page side: a text area past this is drawn on a page of
/// this size, the 800 by 480 device fitted into it.
pub const REGIS_PAGE_MAX: u16 = 8192;

/// **The page ReGIS draws on, in pixels: the text area's own**, `cols` cells
/// of `cell_w` by `rows` of `cell_h`, each side at most
/// [`REGIS_PAGE_MAX`]; the VT300's 800 by 480 while no cell size is known.
/// What the core draws on and what XTSMGRAPHICS's `Pi = 3` reads.
pub fn regis_page_size(
    cols: crate::Cols,
    rows: crate::Rows,
    cell_w: crate::ImagePx,
    cell_h: crate::ImagePx,
) -> (crate::ImagePx, crate::ImagePx) {
    use crate::ImagePx;
    let side = |cells: usize, px: ImagePx| {
        let n = cells
            .saturating_mul(px.index())
            .clamp(2, usize::from(REGIS_PAGE_MAX));
        ImagePx::of(u16::try_from(n).unwrap_or(REGIS_PAGE_MAX))
    };
    match (cell_w.raw(), cell_h.raw()) {
        (0, _) | (_, 0) => (ImagePx::of(800), ImagePx::of(480)),
        _ => (side(cols.index(), cell_w), side(rows.index(), cell_h)),
    }
}

/// **Whether an `S(T)` delay stops the interpreter.** 070-8 § 8.7.1.4.3:
/// "the number of device dependent timer "ticks" for which ReGIS
/// interpretation should be suspended ... measured from the time the
/// S(T<n>) is interpreted by the device". `On` is for a caller that holds
/// what follows and goes on once the ticks have passed, scheduled rather
/// than waited on; `Off`, the default, passes over each delay and counts
/// it (`RegisCounts::delays_unheld`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisHold {
    Off,
    On,
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
    /// Where it may write (§ 8.8.1.4.6).
    pub clip: RegisClip,
    /// A line's width in page pixels, `W(L)` (§ 8.8.3.2): one, the
    /// nominal width, to [`REGIS_LINE_WIDTH`].
    pub width: u16,
}

/// How far a scroll moves the page's pixels, in page pixels: right and
/// down for positive values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisShift {
    pub dx: i32,
    pub dy: i32,
}

/// **The widest line, in page pixels**: the width field's largest.
/// § 8.8.3.2 names no limit -- "W(L2) sets a width twice the nominal
/// width, W(L3) three times, and so on" -- and none is set short of the
/// page (decided by Jev, UNBOUNDED, 0.96 and 0.96): a pen wider than twice
/// a page's diagonal covers all of it from any pixel of it, so on a page
/// up to 23,170 pixels a side a line through the page draws the same
/// pixels as with any wider pen. The pen costs a step its rows on the
/// page, and a line at most one whole pen held to the page
/// ([`RegisCanvas::wide_line`]).
pub const REGIS_LINE_WIDTH: u16 = u16::MAX;

/// **Where a drawing may write**, `S(R<n>[][])` (070-8 § 8.8.1.4.6): the
/// whole page, the clip rectangle's interior, "defined to include what
/// would be drawn as the boundary", or its exterior, "shielding", where
/// "the boundary is NOT part of the exterior". The rectangle is in page
/// pixels, its boundary inside it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisClip {
    /// Clipping disabled, `S(R0)`: "drawing actions can take place
    /// everywhere in the display".
    Off,
    /// `S(R<n>)`, `n` above zero.
    Inside(RegisRect),
    /// `S(R<n>)`, `n` below zero.
    Shield(RegisRect),
}

impl RegisClip {
    /// Whether page pixel `(x, y)` may be written.
    pub fn allows(self, x: i32, y: i32) -> bool {
        let inside = |r: RegisRect| x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1;
        match self {
            RegisClip::Off => true,
            RegisClip::Inside(r) => inside(r),
            RegisClip::Shield(r) => !inside(r),
        }
    }
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
    /// A character of the built-in alphabet 0, by Unicode scalar, drawn by
    /// the embedder's font at the unit cell ([`RegisFont`]); `None` with no
    /// font set.
    Builtin {
        ch: char,
        cell: Option<GlyphBits<'a>>,
    },
    /// A loaded cell: its rows, most significant bit leftmost, `w` wide.
    Loaded { rows: &'a [u16], w: u8 },
    /// The error character § 8.7.3.3 asks for: "Solid block characters or
    /// checkerboard characters are acceptable", and the VT330/VT340
    /// "displays a solid block error character" (`[[source: vt3xx-text]]`).
    Error,
}

/// A character drawn at its unit cell: `w` by `h` bits, a row after
/// another, the first row's first bit the low bit of the first word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GlyphBits<'a> {
    pub bits: &'a [u64],
    pub w: u16,
    pub h: u16,
}

impl GlyphBits<'_> {
    /// Whether the bit at `(x, y)` is set.
    pub fn bit(&self, x: usize, y: usize) -> bool {
        let i = y * usize::from(self.w) + x;
        self.bits
            .get(i / 64)
            .is_some_and(|w| w & (1 << (i % 64)) != 0)
    }
}

/// **The embedder's font, for the built-in alphabet**: draw character `c`
/// in a `w` by `h` cell, calling `set(x, y)` for each pixel it covers. Each
/// ReGIS device "must have a built in character definition and writing
/// capability" (070-8 § 8.5.6); baud's is the embedder's own face, drawn by
/// its own rasteriser at the size the character is shown.
pub type RegisFont = fn(
    c: char,
    w: crate::image::ImagePx,
    h: crate::image::ImagePx,
    set: &mut dyn FnMut(crate::image::ImagePx, crate::image::ImagePx),
);

/// The largest cell a built-in character is drawn at, each way; a larger
/// unit cell samples it.
pub const GLYPH_SIDE: usize = 256;

/// What the interpreter does to the world: draw, answer, wait.
///
/// The drawing primitives are in device pixels, already mapped from the
/// imposed coordinate space and clipped by nothing; a page clips them.
pub trait RegisOut {
    /// A line from `a` to `b` through `pen`, the pattern stepping on from
    /// `phase`, its ends drawn as `span` says.
    fn line(
        &mut self,
        a: RegisPoint,
        b: RegisPoint,
        pen: &RegisPen,
        phase: &mut u32,
        span: LineSpan,
    );
    /// A filled area: `points` in contours ending at each of `ends`, the
    /// interior by the even-odd rule (§ 8.5.8.4.1).
    fn fill(&mut self, points: &[RegisPoint], ends: &[u16], pen: &RegisPen);
    /// A character.
    fn glyph(&mut self, g: &RegisGlyph<'_>, pen: &RegisPen);
    /// `S(E)`: the whole page to `bg`.
    fn erase(&mut self, bg: RegisIndex);
    /// `S[]` or `S<pv>`, the scrolling model (§ 8.8.1.1): the page's
    /// pixels moved `by`, what they reveal `bg`.
    fn scroll(&mut self, by: RegisShift, bg: RegisIndex);
    /// An output map entry changed: every pixel holding it shows the new
    /// colour.
    fn recolor(&mut self);
    /// Bytes for the host, a report whole.
    fn reply(&mut self, bytes: &[u8]);
    /// `S(T<n>)`: suspend interpretation for `ticks`.
    fn delay(&mut self, ticks: RegisTicks);
    /// `R(P(I))` in multiple mode: the report `pos`, a bracketed extent,
    /// to go at once behind the null button's string and before the
    /// carriage return, whole (070-13 § 5.2).
    fn interactive(&mut self, pos: &[u8]);
    /// Graphics input's cursor appears: `R(I1)`, "the input cursor appears
    /// on the screen", or a one-shot `R(P(I))`. The caller makes room for
    /// it to be drawn.
    fn input_shown(&mut self);
    /// A wide stroke has ended: the marks its pixels carry in `r` go
    /// ([`RegisCanvas::unmark`]), on the page drawn now, if there is one.
    fn unmark(&mut self, r: RegisRect);
}

/// Whether the parser is inside a ReGIS envelope, between its `DCS p` and
/// the `ST` or cancel that ends it: multiple mode's cursor and buttons work
/// only inside one, "the ReGIS locator will be disabled until ReGIS is
/// re-entered, at which time multiple input mode resumes with the input
/// cursor at its last known position" (070-13 § 5.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegisEnvelope {
    Open,
    Closed,
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
    /// Macrograph characters played for the host character that invoked
    /// them, across the resumes a delay makes: [`MACRO_PLAYED`] bounds
    /// the whole, held or not.
    played: usize,
    sem: Sem,
    /// The encoding the envelope began under ([`Regis::set_encoding`]).
    enc: InputEncoding,
    /// A UTF-8 character being read inside a quoted string: its value so
    /// far and the continuation bytes still owed.
    utf8: (u32, u8),
    /// Command display (`DCS 2p`, `DCS 3p`): on for the envelope that asked.
    display: Display,
    /// The cursors the caller drew, to take away ([`Regis::take_drawn`]).
    drawn: RegisDrawn,
    /// DECLKD's locator key definitions, the strings graphics input sends
    /// for the pointer's buttons.
    keys: crate::LocatorKeys,
}

/// Whether the envelope asked for command display.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Display {
    Off,
    On,
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
        played: 0,
        sem: Sem::NEW,
        enc: InputEncoding::Utf8,
        utf8: (0, 0),
        display: Display::Off,
        drawn: RegisDrawn::NONE,
        keys: crate::LocatorKeys::DEFAULT,
    };

    /// A reset to power-up: `RIS`. The page is the caller's to clear.
    ///
    /// **The embedder's font and hold, and the counts, survive it**: they
    /// are the caller's settings and readings, not the device's state.
    pub fn reset(&mut self) {
        self.macros.clear_all();
        self.at = At::Idle;
        self.depth = 0;
        self.lex = Lex::Ground;
        self.nagain = 0;
        let (font, hold, counts) = (self.sem.font, self.sem.hold, self.sem.counts);
        self.sem = Sem::NEW;
        self.sem.font = font;
        self.sem.hold = hold;
        self.sem.counts = counts;
        // The page they were drawn on goes with the reset's images.
        self.drawn = RegisDrawn::NONE;
        // "A power-up restart or Reset to Initial State (RIS) will cause the
        // LKDs to be set to their default value" (070-13 § 5.4).
        self.keys.reset();
    }

    /// A `DCS Pn p`: the low bit of `Pn` resynchronises, "regardless of the
    /// previous state of the interpreter, and returns the interpreter to the
    /// highest command level with no quoted strings or macrograph
    /// definitions in process" (§ 8.9.1); without it ReGIS resumes where it
    /// was left.
    pub fn enter(&mut self, e: RegisEntry, out: &mut dyn RegisOut) {
        self.sem.counts.entered += 1;
        self.display = match e.display() {
            true => Display::On,
            false => Display::Off,
        };
        self.sem.display_col = 0;
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
        // "Characters appear as the terminal receives them (no case
        // conversion, syntax checking, or macrograph expansion)"
        // (`[[source: vt3xx-regis-intro]]`).
        if self.display == Display::On {
            self.sem.display_char(b.raw_byte(), out);
        }
        if self.depth == 0 {
            self.played = 0;
        }
        self.layer(b.raw_byte(), Source::Host, out);
        self.drain(out);
    }

    /// **Go on with the macrographs a delay stopped**, before the next host
    /// character: under [`RegisHold::On`] a delay inside a macrograph stops
    /// it there, its place kept.
    pub fn resume(&mut self, out: &mut dyn RegisOut) {
        self.drain(out);
    }

    /// Whether a macrograph a delay stopped is still to play.
    pub fn playing(&self) -> bool {
        self.depth > 0
    }

    /// Whether the next character is held: a macrograph a delay stopped is
    /// still to play, or a one-shot `R(P(I))` waits.
    pub fn held(&self) -> bool {
        self.playing() || self.sem.input.pending == cursor::Pending::Yes
    }

    /// Whether a delay stops the interpreter: see [`RegisHold`].
    pub fn set_hold(&mut self, hold: RegisHold) {
        self.sem.hold = hold;
    }

    /// The ticks of the delays met since the last call, under
    /// [`RegisHold::On`]: `None` when none was.
    pub fn take_wait(&mut self) -> Option<RegisTicks> {
        let t = core::mem::take(&mut self.sem.wait);
        (t > 0).then_some(RegisTicks::of(t))
    }

    /// Play the macrographs invoked, a character at a time, until none is,
    /// a delay stops them, or [`MACRO_PLAYED`] have played.
    fn drain(&mut self, out: &mut dyn RegisOut) {
        while self.depth > 0 && self.sem.wait == 0 && self.sem.input.pending == cursor::Pending::No
        {
            if self.played == MACRO_PLAYED {
                self.depth = 0;
                self.sem.counts.macro_cut += 1;
                return;
            }
            let top = self.play[self.depth - 1];
            match self.macros.get(usize::from(top.m), usize::from(top.i)) {
                Some(c) => {
                    self.play[self.depth - 1].i += 1;
                    self.played += 1;
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

    /// `n` drawings had no page to draw on: the caller's image memory is
    /// absent or full.
    pub fn unpaged(&mut self, n: usize) {
        self.sem.counts.unpaged = self.sem.counts.unpaged.saturating_add(n as u32);
    }

    /// The embedder's font, which draws the built-in alphabet.
    pub fn set_font(&mut self, f: RegisFont) {
        self.sem.font = Some(f);
    }

    /// The input encoding the envelope began under: a quoted string's
    /// bytes are a UTF-8 character's under UTF-8.
    pub fn set_encoding(&mut self, enc: InputEncoding) {
        self.enc = enc;
    }

    /// The current drawing position, in the imposed space.
    pub fn position(&self) -> (RegisNum, RegisNum) {
        (self.sem.pos.0, self.sem.pos.1)
    }

    /// The imposed coordinate space (`S(A)`).
    pub fn space(&self) -> RegisSpace {
        self.sem.space
    }

    /// The page drawn on is `w` by `h` pixels: the 800 by 480 device is
    /// fitted into it with its shape kept, as § 8.5.1.4.1.2 fits an imposed
    /// space into a device -- "The shape (aspect ratio) implied by S(A[][])
    /// must be maintained" and "All the area implied by the S(A[][]) must be
    /// visible" -- at its upper-left, "The placement of any subset range
    /// within the device space is left to the device implementor".
    pub fn set_page(&mut self, w: i32, h: i32) {
        let (dw, dh) = (i64::from(REGIS_WIDTH - 1), i64::from(REGIS_HEIGHT - 1));
        let (pw, ph) = (i64::from(w.max(2) - 1), i64::from(h.max(2) - 1));
        self.sem.scale = match pw * dh <= ph * dw {
            true => (pw, dw),
            false => (ph, dh),
        };
    }

    /// The background index `S(I)` names.
    pub fn background(&self) -> RegisIndex {
        self.sem.bg
    }

    /// The characters free in the macrograph store.
    pub fn macro_free(&self) -> usize {
        self.macros.free()
    }

    /// **What graphics input is doing**: a one-shot `R(P(I))` waiting, or
    /// multiple mode inside an envelope (`inside`), or neither.
    pub fn input(&self, inside: RegisEnvelope) -> RegisInput {
        match (self.sem.input.pending, self.sem.input.mode, inside) {
            (cursor::Pending::Yes, _, _) => RegisInput::OneShot,
            (_, RegisInputMode::Multiple, RegisEnvelope::Open) => RegisInput::Multiple,
            _ => RegisInput::Off,
        }
    }

    /// The input mode `R(I<n>)` selected.
    pub fn input_mode(&self) -> RegisInputMode {
        self.sem.input.mode
    }

    /// Move the input cursor `dx`, `dy` device pixels, wrapping at the
    /// screen's edge: an arrow key's step, "one pixel", Shift's "10 pixels"
    /// (`[[source: vt3xx-report]]`), a pixel of the VT300's device that the
    /// page is fitted from (Jev, DEVICE_PIXEL, 0.92 and 0.96).
    pub fn input_step(&mut self, dx: i32, dy: i32) {
        self.sem.input.step(dx, dy);
    }

    /// Put the input cursor under the pointer, at page pixel `x`, `y`: the
    /// pointer is absolute, as the VT340's tablet is ("the tablet is an
    /// absolute positioning device"), held to the device.
    pub fn input_point(&mut self, x: i32, y: i32) {
        self.sem.input.at = self.sem.device_of_page(x, y);
    }

    /// The device pixel the input cursor is on, from the device's upper-left:
    /// what the status line's Graphics Input Position Indicator shows, "the
    /// pixel row and column of the input position" (070-14 p. 14-6).
    pub fn input_pixel(&self) -> crate::PointerPx {
        let (x, y) = self.sem.input.at;
        let px = |v: i64| crate::image::ImagePx::of(u16::try_from(v >> 16).unwrap_or(0));
        crate::PointerPx { x: px(x), y: px(y) }
    }

    /// The input position as a report writes it, `[x,y]` in user
    /// coordinates, into `out`; its length.
    pub fn input_position(&self, out: &mut [u8; 56]) -> usize {
        self.sem.input_position(out)
    }

    /// A one-shot report went: the input cursor goes and the interpreter
    /// goes on (070-13 § 5.1, "The graphics input cursor disappears from
    /// the screen and the terminal exits graphics input mode").
    pub fn input_end(&mut self) {
        self.sem.input.pending = cursor::Pending::No;
    }

    /// **A one-shot input given up** because the host wrote more than the
    /// embedder holds while it waited: ended as [`Regis::input_end`] ends
    /// it, counted; the caller sends 8.8.5.3's empty report, "Only the
    /// terminating CR is sent ... indicating that NO INPUT was to be
    /// returned". Jev's lean: EMPTY_REPORT at 0.38, POSITION_REPORT at
    /// 0.22 in the other order, EMPTY_REPORT having the higher probability
    /// averaged over the two orders, 0.495 against 0.37.
    pub fn input_abandon(&mut self) {
        self.input_end();
        self.sem.counts.inputs_abandoned += 1;
    }

    /// DECLKD's locator key definitions.
    pub fn locator_keys(&self) -> &crate::LocatorKeys {
        &self.keys
    }

    /// DECLKD's locator key definitions, to load.
    pub fn locator_keys_mut(&mut self) -> &mut crate::LocatorKeys {
        &mut self.keys
    }

    /// What the caller drew of the cursors, and nothing kept after.
    pub fn take_drawn(&mut self) -> RegisDrawn {
        core::mem::replace(&mut self.drawn, RegisDrawn::NONE)
    }

    /// Whether any cursor is drawn.
    pub fn is_drawn(&self) -> bool {
        self.drawn != RegisDrawn::NONE
    }

    /// The caller drew `d`.
    pub fn set_drawn(&mut self, d: RegisDrawn) {
        self.drawn = d;
    }

    /// The output cursor at the drawing position, if `S(C)` shows it.
    pub fn output_cursor(&self) -> Option<RegisCursor> {
        self.sem.output_cursor()
    }

    /// The input cursor where graphics input has it.
    pub fn input_cursor(&self) -> RegisCursor {
        self.sem.input_cursor()
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
                if c == q && self.utf8.1 == 0 {
                    self.lex = Lex::QuoteEnd { q };
                } else {
                    self.quoted(c, out);
                }
            }
            Lex::QuoteEnd { q } => {
                if c == q {
                    // "the target quote character must be doubled".
                    self.lex = Lex::Quote { q };
                    self.sem.quote_char(u16::from(q), out);
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

    /// One byte of a quoted string. **Under UTF-8 a character past ASCII
    /// arrives as its bytes**, and is the code it is in the 8-bit in-use
    /// table -- U+0020 to U+007E in GL, U+00A0 to U+00FF in GR, § 8.4.3.2's
    /// "2/0 through 7/15, and 10/0 through 15/15" -- or the error character
    /// for any other (Jev's lean, 2026-10-20, DECODE_TO_TABLE, 0.79 and
    /// 0.74); under ISO 8859-1 each byte is its code.
    fn quoted(&mut self, c: u8, out: &mut dyn RegisOut) {
        if self.enc != InputEncoding::Utf8 || c < 0x80 {
            if self.utf8.1 != 0 {
                // A character cut short by an ASCII byte.
                self.utf8 = (0, 0);
                self.sem.quote_char(sem::NO_CODE, out);
            }
            return self.sem.quote_char(u16::from(c), out);
        }
        match (self.utf8.1, c) {
            (0, 0xc2..=0xdf) => self.utf8 = (u32::from(c & 0x1f), 1),
            (0, 0xe0..=0xef) => self.utf8 = (u32::from(c & 0x0f), 2),
            (0, 0xf0..=0xf4) => self.utf8 = (u32::from(c & 0x07), 3),
            (0, _) => self.sem.quote_char(sem::NO_CODE, out),
            (n, 0x80..=0xbf) => {
                let v = (self.utf8.0 << 6) | u32::from(c & 0x3f);
                self.utf8 = (v, n - 1);
                if n == 1 {
                    self.utf8 = (0, 0);
                    let code = match v {
                        0xa0..=0xff => v as u16,
                        _ => sem::NO_CODE,
                    };
                    self.sem.quote_char(code, out);
                }
            }
            (_, _) => {
                self.utf8 = (0, 0);
                self.sem.quote_char(sem::NO_CODE, out);
                self.quoted(c, out);
            }
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
                self.utf8 = (0, 0);
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
