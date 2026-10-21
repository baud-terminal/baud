//! The instructions: what the keyletters and their arguments do to the
//! device state, and what they draw.

use super::color::{Comp, REGIS_COLORS, REGIS_DEFAULT_MAP, RegisColor, regis_closest};
use super::cursor::{
    Cursors, Input, Pending, REGIS_CURSOR_CELL, RegisCursor, RegisCursorPair, RegisCursorStyle,
    RegisInputMode, Shown, tenths,
};
use super::num::RegisNum;
use super::trig;
use super::*;
use crate::image::{ImagePx, Pixel};
use crate::vt::{Charset, Designation, Iso96};

/// What the interpreter refused or could not do, each its own count.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegisCounts {
    /// `DCS p` envelopes entered.
    pub entered: u32,
    /// Built-in characters not drawn because no font is set
    /// ([`Regis::set_font`]).
    pub unfonted: u32,
    /// A macrograph definition inside a definition or a macrograph, `@.` in
    /// a macrograph, a definition named by no letter, a stray `@;`.
    pub macro_errors: u32,
    /// Invocations refused because the macrograph was already playing.
    pub macro_recursion: u32,
    /// Invocations refused past [`MACRO_NESTING`].
    pub macro_nesting: u32,
    /// Host characters whose macrographs were cut at [`MACRO_PLAYED`].
    pub macro_cut: u32,
    /// Definitions cut short by a full store.
    pub macro_overflow: u32,
    /// Options opened past [`OPTION_LEVELS`], each read as a `;`.
    pub nesting_overflow: u32,
    /// Saves refused by a full position stack (error 7).
    pub stack_overflow: u32,
    /// Restores from an empty position stack (error 8).
    pub stack_underflow: u32,
    /// Fill vertices dropped past [`FILL_VERTICES`].
    pub fill_overflow: u32,
    /// Fills a `;` or a new keyletter abandoned before their `)`.
    pub fill_abandoned: u32,
    /// Characters loaded past an alphabet's extent, into alphabet 0, or of
    /// a cell size past [`CELL_WIDTH`] by [`CELL_HEIGHT`].
    pub load_refused: u32,
    /// `S(A)` with a missing, negative or empty extent: "the command is
    /// ignored and the address space previously in effect shall be
    /// maintained" (§ 8.5.1.4.1).
    pub space_refused: u32,
    /// Open Extensions (§ 8.8) this interpreter parses and does not act
    /// on: data movement and drawing surfaces (`P(P)`, the VT340's
    /// `S(P)`); and `S(N)`, which no DEC text here defines.
    pub unbuilt: u32,
    /// Characters no grammar position takes, and keyletters no instruction
    /// names (error 1).
    pub ignored: u32,
    /// Drawings with no page to draw on, the image memory absent or full.
    pub unpaged: u32,
    /// `S(T)` delays passed over because the embedder holds no read
    /// ([`RegisHold::Off`]).
    pub delays_unheld: u32,
    /// One-shot `R(P(I))` answered as a non-interactive device answers,
    /// because the embedder holds no read ([`RegisHold::Off`]).
    pub inputs_unheld: u32,
    /// One-shot inputs ended with the empty report because the host wrote
    /// more than its embedder holds while they waited
    /// ([`super::Regis::input_abandon`]).
    pub inputs_abandoned: u32,
}

/// The imposed coordinate space, `S(A[x1,y1][x2,y2])`: the user's
/// coordinates of the upper-left and lower-right corners (§ 8.5.1.4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegisSpace {
    pub x1: RegisNum,
    pub y1: RegisNum,
    pub x2: RegisNum,
    pub y2: RegisNum,
}

impl RegisSpace {
    /// `S(A[0,0][799,479])`, the VT300's (`[[source: vt3xx-regis-intro]]` Table 1-2).
    pub const DEFAULT: RegisSpace = RegisSpace {
        x1: RegisNum::ZERO,
        y1: RegisNum::ZERO,
        x2: RegisNum::int(REGIS_WIDTH - 1),
        y2: RegisNum::int(REGIS_HEIGHT - 1),
    };

    /// Device pixels a user unit spans, as a fraction `(num, den)`, the same
    /// on both axes: § 8.5.1.4.1.2's "The shape (aspect ratio) implied by
    /// S(A[][]) must be maintained" and "All the area implied by the
    /// S(A[][]) must be visible", so the scale is the smaller of the two
    /// axes'. A coordinate's pixel is its distance from the first corner in
    /// these units: the default maps `[799,479]` to pixel `[799,479]`.
    fn scale(&self) -> (i128, i128) {
        let ux = i128::from(self.x2.minus(self.x1).abs().raw());
        let uy = i128::from(self.y2.minus(self.y1).abs().raw());
        let (dw, dh) = (i128::from(REGIS_WIDTH - 1), i128::from(REGIS_HEIGHT - 1));
        // dw / ux against dh / uy, without dividing.
        if dw * uy <= dh * ux {
            (dw << 16, ux.max(1))
        } else {
            (dh << 16, uy.max(1))
        }
    }

    /// `+1` where the coordinate grows rightward (downward), `-1` where it
    /// grows the other way: § 8.3.1's `SXINC` and `SYINC`.
    fn signs(&self) -> (i64, i64) {
        let sx = if self.x2 >= self.x1 { 1 } else { -1 };
        let sy = if self.y2 >= self.y1 { 1 } else { -1 };
        (sx, sy)
    }

    /// A user position on the device, in units of 1/65536 pixel.
    pub(crate) fn fine(&self, x: RegisNum, y: RegisNum) -> (i64, i64) {
        let (num, den) = self.scale();
        let (sx, sy) = self.signs();
        let dx = i128::from(x.minus(self.x1).raw()) * i128::from(sx);
        let dy = i128::from(y.minus(self.y1).raw()) * i128::from(sy);
        let f = |d: i128| -> i64 {
            let v = d * num / den;
            v.clamp(-(1i128 << 50), 1i128 << 50) as i64
        };
        (f(dx), f(dy))
    }

    /// A user position's device pixel.
    pub fn device(&self, x: RegisNum, y: RegisNum) -> RegisPoint {
        let (fx, fy) = self.fine(x, y);
        RegisPoint {
            x: round16(fx),
            y: round16(fy),
        }
    }

    /// A step of `dx`, `dy` device pixels (in 1/65536) as a step of user
    /// units: what a pixel vector and a standard character size are.
    fn user_step(&self, dx: i64, dy: i64) -> (RegisNum, RegisNum) {
        let (num, den) = self.scale();
        let (sx, sy) = self.signs();
        let f = |d: i64, s: i64| RegisNum::from_raw(((i128::from(d) * den / num) as i64) * s);
        (f(dx, sx), f(dy, sy))
    }

    /// A user length on the device, in 1/65536 pixel.
    fn device_len(&self, v: RegisNum) -> i64 {
        let (num, den) = self.scale();
        (i128::from(v.raw()) * num / den) as i64
    }
}

/// **A line's width in page pixels**: `W(L<n>)` times the nominal width,
/// one page pixel, today's line, to the nearest pixel as a coordinate is
/// -- "Devices that do not support fractions may truncate to the next
/// lower integer or round to the nearest integer, just as they do for
/// other numerics" -- at least one, "W(L0) selects the minimum imageable
/// line width", and at most [`REGIS_LINE_WIDTH`], the width field's
/// largest.
fn line_width(n: RegisNum) -> u16 {
    let most = RegisNum::int(i32::from(REGIS_LINE_WIDTH));
    match n >= most {
        true => REGIS_LINE_WIDTH,
        false => n.round().clamp(1, i32::from(REGIS_LINE_WIDTH)) as u16,
    }
}

/// A value in 1/65536 to the nearest whole, a half away from zero.
fn round16(v: i64) -> i32 {
    let r = if v >= 0 {
        (v + 0x8000) >> 16
    } else {
        -((-v + 0x8000) >> 16)
    };
    r.clamp(-(1 << 24), 1 << 24) as i32
}

/// Fill vertices kept. § 8.5.8.4.1 (p. 8-91) reads "A conforming ReGIS
/// device must provide for a vertex list with a capacity of less than 256
/// entries, but note that certain other implementation dependencies ...
/// could result in many fewer than 256 vertices available to software" --
/// "less than" in the page itself, where the rest of the sentence reads as
/// a floor; 4096, since curves and circles make many vertices a point.
pub const FILL_VERTICES: usize = 4096;
/// Contours a fill keeps: the outline and its `F(F ...)` sub-polygons.
pub const FILL_CONTOURS: usize = 64;
/// Loadable alphabets, numbered 1 to 3: "If loadable alphabets are
/// provided, at least three must be provided, and they must be numbered 1,
/// 2, and 3" (§ 8.7.5.4.1).
pub const ALPHABETS: usize = 3;
/// An alphabet's most characters, `L(E)`'s 192 (§ 8.7.5.4.3).
pub const ALPHABET_CHARS: usize = 192;
/// The widest storage cell: two of the VT300's 8.
pub const CELL_WIDTH: u8 = 16;
/// The tallest storage cell.
pub const CELL_HEIGHT: u8 = 32;
/// The longest alphabet name: "up to ten characters" (§ 8.7.5.4.2).
pub const NAME_MAX: usize = 10;

/// A user position.
type Pt = (RegisNum, RegisNum);
/// A device position in 1/65536 pixel.
type Fine = (i64, i64);
/// Four points of a spline, the segment between the middle two.
type Seg = (Fine, Fine, Fine, Fine);

/// Which side of the clip rectangle drawing is limited to, `S(R<n>)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ClipMode {
    /// `n` zero: clipping disabled.
    Off,
    /// `n` above zero: the interior, its boundary included.
    Interior,
    /// `n` below zero: the exterior, "shielding".
    Shield,
}

/// The clip rectangle (§ 8.8.1.4.6): two opposite corners, user
/// coordinates, which "maintains its position in the coordinate space",
/// and which side drawing is limited to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Clip {
    a: Pt,
    b: Pt,
    mode: ClipMode,
}

impl Clip {
    /// "The default clip area is the extent defined by the last valid S(A)
    /// or the entire display surface ... The default clip state is
    /// disabled" (§ 8.8.1.4.6 g).
    const fn of(space: RegisSpace) -> Clip {
        Clip {
            a: (space.x1, space.y1),
            b: (space.x2, space.y2),
            mode: ClipMode::Off,
        }
    }
}

/// Whether a thing is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum On {
    No,
    Yes,
}

/// Where shading goes (§ 8.8.3.3's suboptions), before it is placed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShadeDim {
    Y,
    X,
    P,
}

/// What shading is textured with, before it is resolved to a [`Texture`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShadeWith {
    Line(LinePattern, u16),
    Char {
        c: u8,
        alphabet: u8,
        unit: (i32, i32),
    },
}

/// The writing controls (§ 8.5.3, § 8.7.2).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Write {
    mode: WriteMode,
    negate: Negate,
    fg: RegisIndex,
    planes: RegisPlanes,
    pattern: LinePattern,
    pattern_mul: u16,
    pv: RegisNum,
    shade: On,
    shade_to: ShadeTo,
    /// The snapshot shading takes when enabled, and polygon fill uses
    /// whether or not shading is on (§ 8.7.2.4.2.3).
    shade_with: Option<ShadeWith>,
    /// `W(L<n>)`, the line width in nominal widths (§ 8.8.3.2).
    width: RegisNum,
    /// `W(W)`'s microcoded entries (§ 8.8.3.4).
    micro: RegisMicro,
    /// `W(A)`, blink (§ 8.8.3.1).
    blink: On,
}

impl Write {
    /// The defaults a valid `S(A)` restores: `W(M1)`, `W(P1)`,
    /// `W(P(M2))`, `W(V,N0)`, the VT340's `W(I7)`, `W(F15)`, `W(S0)`, and
    /// "Line width is set to nominal", `W(L1)` (§ 8.8.1.4.1 b).
    const DEFAULT: Write = Write {
        mode: WriteMode::Overlay,
        negate: Negate::Off,
        fg: RegisIndex(7),
        planes: RegisPlanes::ALL,
        pattern: LinePattern::SOLID,
        pattern_mul: 2,
        pv: RegisNum::ONE,
        shade: On::No,
        shade_to: ShadeTo::Row(0),
        shade_with: None,
        width: RegisNum::ONE,
        micro: RegisMicro::NONE,
        blink: On::No,
    };
}

/// The text controls (§ 8.5.6, § 8.7.3).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Text {
    /// 0 the built-in alphabet, 1 to 3 the loadable ones.
    alphabet: u8,
    gl: Charset,
    gr: Charset,
    /// The display cell and the unit cell, device pixels.
    display: (i32, i32),
    unit: (i32, i32),
    /// The escapement, user units, always relative.
    esc: Pt,
    /// The string's baseline and the character's, degrees.
    string_dir: i32,
    char_dir: i32,
    slant: i32,
}

impl Text {
    /// Standard size 1 and the rest of the VT300's defaults, `T(A0)`,
    /// `T(S1)`, `T(D0)`, `T(I0)`, with escapement one display cell
    /// rightward, which `escapement` sets in the imposed space.
    const DEFAULT: Text = Text {
        alphabet: 0,
        gl: Charset::Ascii,
        gr: Charset::Iso96(Iso96::Latin1),
        display: (9, 20),
        unit: (8, 20),
        esc: (RegisNum::int(9), RegisNum::ZERO),
        string_dir: 0,
        char_dir: 0,
        slant: 0,
    };
}

/// The VT300's seventeen standard sizes, `[[source: vt3xx-text]]` Table 7-3:
/// the display cell and the unit cell, device pixels.
fn standard_size(n: i32) -> Option<((i32, i32), (i32, i32))> {
    match n {
        0 => Some(((9, 10), (8, 10))),
        1 => Some(((9, 20), (8, 20))),
        2..=16 => Some(((9 * n, 15 * n), (8 * n, 15 * n))),
        _ => None,
    }
}

/// An entry of the position stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Saved {
    At(Pt),
    /// `P(S)` and `V(S)`: "a dummy entry ... implies NO ELEMENT".
    Dummy,
}

/// A loadable alphabet (§ 8.7.5).
#[derive(Clone)]
struct Alphabet {
    extent: u16,
    w: u8,
    h: u8,
    name: [u8; NAME_MAX],
    name_len: u8,
    defined: [u32; ALPHABET_CHARS / 32],
    rows: [[u16; CELL_HEIGHT as usize]; ALPHABET_CHARS],
}

impl Alphabet {
    /// "The default extent for each loadable alphabet is 96 characters",
    /// and the VT300's cell, 8 by 10 (`[[source: vt3xx-load]]`).
    const NEW: Alphabet = Alphabet {
        extent: 96,
        w: 8,
        h: 10,
        name: [0; NAME_MAX],
        name_len: 0,
        defined: [0; ALPHABET_CHARS / 32],
        rows: [[0; CELL_HEIGHT as usize]; ALPHABET_CHARS],
    };

    fn clear(&mut self) {
        self.defined = [0; ALPHABET_CHARS / 32];
        self.rows = [[0; CELL_HEIGHT as usize]; ALPHABET_CHARS];
    }
}

/// Where a character's code is in an alphabet: "The range of the extent
/// begins with the space character (code value 2/0) ... The ninety-fifth
/// character position is code 7/14, the ninety-sixth is 7/15, the
/// ninety-seventh is 10/0" (§ 8.7.5.4.3).
fn char_slot(c: u8) -> Option<usize> {
    match c {
        0x20..=0x7f => Some(usize::from(c - 0x20)),
        0xa0..=0xff => Some(usize::from(c - 0xa0) + 96),
        _ => None,
    }
}

/// A curve interpolation in progress (§ 8.5.5.4.2.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CurveKind {
    None,
    Open,
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Curve {
    kind: CurveKind,
    /// The last four points, device pixels in 1/65536.
    last: [(i64, i64); 4],
    /// The first three, which a closed curve returns through.
    first: [(i64, i64); 3],
    n: usize,
    start: Pt,
}

impl Curve {
    const NONE: Curve = Curve {
        kind: CurveKind::None,
        last: [(0, 0); 4],
        first: [(0, 0); 3],
        n: 0,
        start: (RegisNum::ZERO, RegisNum::ZERO),
    };
}

/// A fill being defined (§ 8.5.8).
#[derive(Clone)]
struct Fill {
    points: [RegisPoint; FILL_VERTICES],
    n: usize,
    ends: [u16; FILL_CONTOURS],
    nends: usize,
    full: On,
    saved: Pt,
}

impl Fill {
    const NEW: Fill = Fill {
        points: [RegisPoint { x: 0, y: 0 }; FILL_VERTICES],
        n: 0,
        ends: [0; FILL_CONTOURS],
        nends: 0,
        full: On::No,
        saved: (RegisNum::ZERO, RegisNum::ZERO),
    };

    fn push(&mut self, p: RegisPoint) -> bool {
        if self.full == On::Yes {
            return false;
        }
        let contour = self
            .nends
            .checked_sub(1)
            .map_or(0, |i| usize::from(self.ends[i]));
        if self.n > contour && self.points[self.n - 1] == p {
            return true;
        }
        if self.n == FILL_VERTICES {
            self.full = On::Yes;
            return false;
        }
        self.points[self.n] = p;
        self.n += 1;
        true
    }

    /// End the contour in progress, if it has a point.
    fn close(&mut self) {
        let from = self
            .nends
            .checked_sub(1)
            .map_or(0, |i| usize::from(self.ends[i]));
        if self.n > from && self.nends < FILL_CONTOURS {
            self.ends[self.nends] = self.n as u16;
            self.nends += 1;
        }
    }
}

/// One level of the option nesting: its keyletter and the arguments it has
/// collected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Frame {
    key: u8,
    num: Option<RegisNum>,
    digits: [u8; 16],
    nd: u8,
    ext: [Extent; 2],
    next: u8,
    text: [u8; 12],
    nt: u8,
    /// A quoted string arrived before the number: `L(A"name"2)`.
    text_first: On,
    /// What a colour group under this frame named (§ 8.6.2.2.2).
    letter: u8,
    h: Comp,
    l: Comp,
    s: Comp,
    /// A suboption seen: a letter, for `W(S(X))`, `R(P(I))`, `R(M(x))`.
    sub: u8,
}

impl Frame {
    const EMPTY: Frame = Frame {
        key: 0,
        num: None,
        digits: [0; 16],
        nd: 0,
        ext: [Extent {
            x: Part::Absent,
            y: Part::Absent,
        }; 2],
        next: 0,
        text: [0; 12],
        nt: 0,
        text_first: On::No,
        letter: 0,
        h: Comp::Absent,
        l: Comp::Absent,
        s: Comp::Absent,
        sub: 0,
    };

    fn keyed(k: u8) -> Frame {
        Frame {
            key: k,
            ..Frame::EMPTY
        }
    }

    fn color(&self) -> Option<RegisColor> {
        match self.letter {
            0 => match (self.h, self.l, self.s) {
                (Comp::Absent, Comp::Absent, Comp::Absent) => None,
                (h, l, s) => Some(RegisColor::Hls { h, l, s }),
            },
            c => Some(RegisColor::Letter(c)),
        }
    }

    fn int(&self) -> Option<i64> {
        self.num.map(|v| i64::from(v.trunc()))
    }
}

/// Which cursor an `S(C ...)` option sets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CursorWhich {
    Output,
    Input,
    Both,
}

/// Which text tilt the `D` and `S` options of one text command have set,
/// `[[source: vt3xx-text]]`: `T(D<a>)` alone the character's,
/// `T(D<a>,S<n>)` the string's, `T(D<a>,S<n>,D<b>)` both.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tilt {
    Ready,
    /// A `D`: the characters turned.
    Chars,
    /// A `D` then an `S`: the string turned with them.
    String,
    /// A `D` after those: the characters turned again.
    Both,
}

/// The semantic layer: the device state and the instruction in progress.
#[derive(Clone)]
pub(crate) struct Sem {
    pub(crate) counts: RegisCounts,
    pub(crate) pos: Pt,
    pub(crate) space: RegisSpace,
    pub(crate) map: [Pixel; REGIS_COLORS],
    pub(crate) bg: RegisIndex,
    write: Write,
    /// The writing controls in force for the instruction in progress: the
    /// persistent ones, and its temporary `W` option's (§ 8.4.3.4).
    tw: Write,
    text: Text,
    saved_text: Option<Text>,
    stack: [Saved; POSITION_STACK],
    nstack: usize,
    alphabets: [Alphabet; ALPHABETS],
    load: u8,
    /// The loading character and the row being filled.
    load_char: Option<usize>,
    load_row: u8,
    load_digits: u8,
    load_value: u32,
    error: (u8, u8),
    frames: [Frame; OPTION_LEVELS + 1],
    depth: usize,
    // The instruction in progress.
    phase: u32,
    arc: Option<RegisNum>,
    circumferential: On,
    curve: Curve,
    fill: Option<Fill>,
    tilt: Tilt,
    quote_start: Pt,
    /// Characters free in the macrograph store, for `R(M(=))`: the layer
    /// below sets it before each character it hands the lexer.
    pub(crate) macro_free: usize,
    /// The embedder's font, which draws the built-in alphabet.
    pub(crate) font: Option<RegisFont>,
    /// Whether a delay stops the interpreter ([`Regis::set_hold`]).
    pub(crate) hold: RegisHold,
    /// The ticks of the delays met since the caller last took them, under
    /// [`RegisHold::On`].
    pub(crate) wait: u32,
    /// Where a built-in character is drawn by the font before it is put on
    /// the page.
    glyph_bits: [u64; GLYPH_SIDE * GLYPH_SIDE / 64],
    /// The command display's next column.
    pub(crate) display_col: u8,
    /// The command display's characters since its line was last erased,
    /// a byte a column, zero for none: drawn over the page shown while it
    /// is magnified ([`Sem::display_glyphs`]).
    display_line: [u8; 80],
    /// Moved on whenever the command display's line changes.
    display_gen: u32,
    /// Page pixels a device pixel spans, `(num, den)`: the 800 by 480
    /// device fitted to the page with its shape kept (§ 8.5.1.4.1.2), one
    /// to one on an 800 by 480 page.
    pub(crate) scale: (i64, i64),
    /// Where the last line drawn ended, so the next one from there draws
    /// the joint once.
    last_end: Option<RegisPoint>,
    /// The cursors' settings, `S(C ...)`.
    pub(crate) cursors: Cursors,
    /// Graphics input, `R(I<n>)` and `R(P(I))`.
    pub(crate) input: Input,
    /// The clip rectangle, `S(R)`.
    clip: Clip,
    /// The page pixels the wide stroke in progress has marked, to clear
    /// when it ends ([`RegisOut::unmark`]).
    stroke: Option<RegisRect>,
    /// The pen that drew the last step of the stroke's last line, if that
    /// line drew it ([`LineSpan::Chained`]).
    stroke_end: Option<RegisPen>,
    /// How the plane is shown, `S(S)`.
    pub(crate) zoom: RegisZoom,
    /// `S(H(P[x,y]))`'s offset, kept until the next or a valid `S(A)`.
    pub(crate) hard_offset: RegisOffset,
    /// What bounds a flood, `F(B)` (§ 8.8.6.3.1.1).
    bound: RegisBound,
    /// The blink phase pages are shown in ([`super::Regis::set_blink_phase`]).
    pub(crate) blink_phase: RegisPhase,
    /// The instruction's last flood seed, which a relative seed after it
    /// is measured from.
    seed: Option<Pt>,
}

impl Sem {
    pub(crate) const NEW: Sem = Sem {
        counts: RegisCounts {
            entered: 0,
            unfonted: 0,
            macro_errors: 0,
            macro_recursion: 0,
            macro_nesting: 0,
            macro_cut: 0,
            macro_overflow: 0,
            nesting_overflow: 0,
            stack_overflow: 0,
            stack_underflow: 0,
            fill_overflow: 0,
            fill_abandoned: 0,
            load_refused: 0,
            space_refused: 0,
            unbuilt: 0,
            ignored: 0,
            unpaged: 0,
            delays_unheld: 0,
            inputs_unheld: 0,
            inputs_abandoned: 0,
        },
        pos: (RegisNum::ZERO, RegisNum::ZERO),
        space: RegisSpace::DEFAULT,
        map: REGIS_DEFAULT_MAP,
        bg: RegisIndex::ZERO,
        write: Write::DEFAULT,
        tw: Write::DEFAULT,
        text: Text::DEFAULT,
        saved_text: None,
        stack: [Saved::Dummy; POSITION_STACK],
        nstack: 0,
        alphabets: [Alphabet::NEW, Alphabet::NEW, Alphabet::NEW],
        load: 1,
        load_char: None,
        load_row: 0,
        load_digits: 0,
        load_value: 0,
        error: (0, 0),
        frames: [Frame::EMPTY; OPTION_LEVELS + 1],
        depth: 0,
        phase: 0,
        arc: None,
        circumferential: On::No,
        curve: Curve::NONE,
        fill: None,
        tilt: Tilt::Ready,
        quote_start: (RegisNum::ZERO, RegisNum::ZERO),
        macro_free: MACRO_STORE,
        font: None,
        hold: RegisHold::Off,
        wait: 0,
        glyph_bits: [0; GLYPH_SIDE * GLYPH_SIDE / 64],
        display_col: 0,
        display_line: [0; 80],
        display_gen: 0,
        scale: (1, 1),
        last_end: None,
        cursors: Cursors::DEFAULT,
        input: Input::NEW,
        clip: Clip::of(RegisSpace::DEFAULT),
        stroke: None,
        stroke_end: None,
        zoom: RegisZoom::ONE,
        hard_offset: RegisOffset::ZERO,
        bound: RegisBound::Seed,
        seed: None,
        blink_phase: RegisPhase::Shown,
    };

    /// Record an error for `R(E)`: "Only the most recent error code is kept"
    /// (§ 8.8.5.1).
    pub(crate) fn error(&mut self, code: u8, c: u8) {
        if code == 1 {
            self.counts.ignored += 1;
        }
        self.error = (code, c);
    }

    fn pen(&self) -> RegisPen {
        let w = &self.tw;
        RegisPen {
            mode: w.mode,
            negate: w.negate,
            fg: w.fg,
            bg: self.bg,
            planes: w.planes,
            pattern: w.pattern,
            // A pattern element is a device pixel long times its
            // multiplier, so a dash keeps its length on a larger page.
            pattern_mul: self.page_len(i32::from(w.pattern_mul)) as u16,
            shade: match w.shade {
                On::Yes => Some((w.shade_to, self.texture(w.shade_with))),
                On::No => None,
            },
            clip: self.page_clip(),
            width: line_width(w.width),
            micro: w.micro,
            blink: match w.blink {
                On::Yes => RegisBlink::On,
                On::No => RegisBlink::Off,
            },
        }
    }

    /// The clip rectangle on the page: its corners' pixels, the boundary
    /// between them inside it.
    fn page_clip(&self) -> RegisClip {
        let (a, b) = (self.device(self.clip.a), self.device(self.clip.b));
        let r = RegisRect {
            x0: a.x.min(b.x),
            y0: a.y.min(b.y),
            x1: a.x.max(b.x).saturating_add(1),
            y1: a.y.max(b.y).saturating_add(1),
        };
        match self.clip.mode {
            ClipMode::Off => RegisClip::Off,
            ClipMode::Interior => RegisClip::Inside(r),
            ClipMode::Shield => RegisClip::Shield(r),
        }
    }

    /// The texture a shading snapshot is drawn with.
    fn texture(&self, with: Option<ShadeWith>) -> Texture {
        match with {
            None => Texture::Solid,
            Some(ShadeWith::Line(p, m)) => Texture::Line(p, self.page_len(i32::from(m)) as u16),
            Some(ShadeWith::Char { c, alphabet, unit }) => {
                // "only the top square part of the storage cell is applied
                // in a shading context", at the unit cell's granularity.
                let mut rows = [0u16; 16];
                let (bits, w) = self.cell_bits(alphabet, c);
                let side = w.min(16);
                for (r, row) in rows.iter_mut().enumerate().take(usize::from(side)) {
                    *row = bits.get(r).copied().unwrap_or(0);
                }
                let scale = (self.page_len(unit.0) / i32::from(w.max(1))).clamp(1, 64) as u16;
                Texture::Cell { rows, side, scale }
            }
        }
    }

    /// A character's stored cell: a loaded one's rows, or none.
    fn cell_bits(&self, alphabet: u8, c: u8) -> (&[u16], u8) {
        match alphabet {
            1..=3 => {
                let a = &self.alphabets[usize::from(alphabet - 1)];
                match char_slot(c) {
                    Some(i)
                        if i < usize::from(a.extent)
                            && a.defined[i / 32] & (1 << (i % 32)) != 0 =>
                    {
                        (&a.rows[i][..usize::from(a.h)], a.w)
                    }
                    _ => (&[], a.w),
                }
            }
            _ => (&[], 8),
        }
    }

    /// The user step a pixel vector `d` takes at multiplier `m`: "pixel
    /// vector 0 is always to the right, even if the +X direction has been
    /// defined to be to the left" (§ 8.4.5.3).
    fn pv_step(&self, d: u8, m: RegisNum) -> Pt {
        let (dx, dy): (i64, i64) = match d {
            0 => (1, 0),
            1 => (1, -1),
            2 => (0, -1),
            3 => (-1, -1),
            4 => (-1, 0),
            5 => (-1, 1),
            6 => (0, 1),
            _ => (1, 1),
        };
        let (sx, sy) = self.space.signs();
        let m = if m == RegisNum::ZERO {
            RegisNum::ONE
        } else {
            m
        };
        (
            RegisNum::from_raw(m.raw() * dx * sx),
            RegisNum::from_raw(m.raw() * dy * sy),
        )
    }

    /// A user position's page pixel.
    fn device(&self, p: Pt) -> RegisPoint {
        self.page(self.space.fine(p.0, p.1))
    }

    /// A position on the 800 by 480 device, in 1/65536, as a page pixel:
    /// the device scaled to the page by [`Sem::scale`], its shape kept.
    fn page(&self, p: Fine) -> RegisPoint {
        let (n, d) = (i128::from(self.scale.0), i128::from(self.scale.1));
        let f = |v: i64| round16((i128::from(v) * n / d).clamp(-(1i128 << 50), 1i128 << 50) as i64);
        RegisPoint {
            x: f(p.0),
            y: f(p.1),
        }
    }

    /// A length on the 800 by 480 device as page pixels, at least one.
    fn page_len(&self, v: i32) -> i32 {
        (i64::from(v) * self.scale.0 / self.scale.1).clamp(1, 1 << 16) as i32
    }

    /// Draw a line on the page: a joint with the line before it drawn once
    /// ([`LineSpan`]), and the pattern stepping on.
    ///
    /// **A wide line is a stroke while the lines chain**: its pixels carry
    /// a mark so the pen writes each once, and the marks go when the next
    /// line does not begin where it ended, or anything else is drawn
    /// ([`Sem::end_stroke`]).
    fn draw(&mut self, a: RegisPoint, b: RegisPoint, close: Close, out: &mut dyn RegisOut) {
        let chained = self.last_end == Some(a);
        let pen = self.pen();
        if pen.width <= 1 || !chained {
            self.end_stroke(out);
        }
        let span = match (chained, close) {
            // A wide stroke's first line draws its first pen whole.
            _ if pen.width > 1 && self.stroke.is_none() => LineSpan::Whole,
            // The line before drew this line's first step with this pen.
            (true, Close::Loop) if pen.width > 1 && self.stroke_end == Some(pen) => {
                LineSpan::ChainedInner
            }
            (true, Close::Open) if pen.width > 1 && self.stroke_end == Some(pen) => {
                LineSpan::Chained
            }
            (true, Close::Loop) => LineSpan::Inner,
            (true, Close::Open) => LineSpan::Rest,
            (false, _) => LineSpan::Whole,
        };
        if pen.width > 1 {
            let r = i32::from(pen.width) / 2 + 1;
            let mine = RegisRect {
                x0: a.x.min(b.x).saturating_sub(r),
                y0: a.y.min(b.y).saturating_sub(r),
                x1: a.x.max(b.x).saturating_add(r + 1),
                y1: a.y.max(b.y).saturating_add(r + 1),
            };
            self.stroke = Some(match self.stroke {
                Some(s) => RegisRect {
                    x0: s.x0.min(mine.x0),
                    y0: s.y0.min(mine.y0),
                    x1: s.x1.max(mine.x1),
                    y1: s.y1.max(mine.y1),
                },
                None => mine,
            });
        }
        out.line(a, b, &pen, &mut self.phase, span);
        // A line of no length draws no step, the line before's last
        // standing; a loop's closing line does not draw its last.
        self.stroke_end = match span {
            _ if span != LineSpan::Whole && a == b => self.stroke_end,
            LineSpan::Inner | LineSpan::ChainedInner => None,
            LineSpan::Whole | LineSpan::Rest | LineSpan::Chained => Some(pen),
        };
        self.last_end = Some(b);
    }

    /// The wide stroke in progress, if any, ends: its marks go.
    fn end_stroke(&mut self, out: &mut dyn RegisOut) {
        if let Some(r) = self.stroke.take() {
            out.unmark(r);
        }
    }

    // ---------------------------------------------------------------
    // The grammar's events.
    // ---------------------------------------------------------------

    /// How a digit outside a bracket reads here.
    pub(crate) fn digit_mode(&self) -> DigitMode {
        let d = self.depth;
        let (own, base) = self.owner(d);
        if d == base {
            return match own {
                b'P' | b'V' | b'C' | b'T' | b'S' => DigitMode::Vector,
                b'F' if base == 0 => DigitMode::Vector,
                b'L' if base == 0 => DigitMode::Hex,
                _ => DigitMode::Numeric,
            };
        }
        DigitMode::Numeric
    }

    /// The instruction whose arguments an event at depth `d` belongs to,
    /// and the depth its own arguments are at: inside a fill, `F(V ...)`,
    /// `F(P ...)` and `F(C ...)` take "whatever subarguments that would apply
    /// at the command level" (§ 8.5.8.4.1), and `F(F(V ...))` does too.
    fn owner(&self, d: usize) -> (u8, usize) {
        let k = |i: usize| self.frames[i].key;
        let vpc = |c: u8| matches!(c, b'V' | b'P' | b'C');
        if k(0) == b'F' {
            if d >= 1 && vpc(k(1)) {
                return (k(1), 1);
            }
            if d >= 2 && k(1) == b'F' && vpc(k(2)) {
                return (k(2), 2);
            }
        }
        (k(0), 0)
    }

    /// A keyletter at the current level.
    pub(crate) fn key(&mut self, k: u8, out: &mut dyn RegisOut) {
        let d = self.depth;
        if d == 0 {
            self.end_command(out);
            self.begin_command(k);
            return;
        }
        self.end_key(d, out);
        self.frames[d] = Frame::keyed(k);
    }

    pub(crate) fn open(&mut self, out: &mut dyn RegisOut) {
        if self.depth == OPTION_LEVELS {
            // "Recovery from an attempt to exceed allowable recursion depth
            // is to the highest command level, just as if a semicolon
            // character had been encountered" (§ 8.4.3.4).
            self.counts.nesting_overflow += 1;
            return self.sync(out);
        }
        // A fill's option list opens: the position saved. An `F(F(`
        // sub-polygon opens: the contour before it ends.
        match (self.depth, self.frames[0].key, self.frames[self.depth].key) {
            (0, b'F', _) => self.fill_begin(),
            (1, b'F', b'F') => {
                if let Some(f) = self.fill.as_mut() {
                    f.close();
                }
            }
            _ => {}
        }
        self.depth += 1;
        self.frames[self.depth] = Frame::EMPTY;
    }

    pub(crate) fn close(&mut self, out: &mut dyn RegisOut) {
        let d = self.depth;
        if d == 0 {
            return self.error(1, b')');
        }
        self.end_key(d, out);
        self.end_group(d, out);
        self.frames[d] = Frame::EMPTY;
        self.depth = d - 1;
    }

    /// The synchronising `;`: "the entire parser nesting is to be popped
    /// ... There is no command keyletter in force after this action"
    /// (§ 8.4.2 c). The options open settle their state, as "any
    /// state-saving actions are performed as necessary"; a fill is
    /// abandoned, "the F command is aborted and no drawing takes place"
    /// (§ 8.5.8.4.1).
    pub(crate) fn sync(&mut self, out: &mut dyn RegisOut) {
        if self.fill.is_some() {
            self.fill = None;
            self.counts.fill_abandoned += 1;
        }
        while self.depth > 0 {
            let d = self.depth;
            self.end_key(d, out);
            self.frames[d] = Frame::EMPTY;
            self.depth = d - 1;
        }
        self.end_command(out);
        self.frames[0] = Frame::EMPTY;
        // "The error report register is cleared when ReGIS is synchronized
        // by a ";"" (§ 8.8.5.1).
        self.error = (0, 0);
    }

    pub(crate) fn comma(&mut self, out: &mut dyn RegisOut) {
        // In the Load instruction's data a comma ends a row (§ 8.7.5.2).
        if self.depth == 0 && self.frames[0].key == b'L' {
            self.load_row_end();
        }
        let _ = out;
    }

    pub(crate) fn number(&mut self, v: RegisNum, digits: &[u8], out: &mut dyn RegisOut) {
        let d = self.depth;
        let f = &mut self.frames[d];
        f.num = Some(v);
        let n = digits.len().min(16);
        f.digits[..n].copy_from_slice(&digits[..n]);
        f.nd = n as u8;
        // A number between colour groups of `S(M)` names the next entry.
        let _ = out;
    }

    pub(crate) fn extent(&mut self, e: Extent, out: &mut dyn RegisOut) {
        let d = self.depth;
        let (own, base) = self.owner(d);
        if d == base {
            return self.command_extent(own, Dest::of(base), e, out);
        }
        if d == 1 && self.frames[0].key == b'S' && self.frames[1].key == b'R' {
            return self.clip_corner(e);
        }
        let f = &mut self.frames[d];
        if usize::from(f.next) < f.ext.len() {
            f.ext[usize::from(f.next)] = e;
        } else if f.next == 2 {
            // "contained more than two coordinate pairs" -- error 2.
            self.error = (2, 0);
        }
        let f = &mut self.frames[d];
        f.next = f.next.saturating_add(1);
    }

    pub(crate) fn vector(&mut self, v: u8, out: &mut dyn RegisOut) {
        if v > 7 {
            return self.error(1, b'0' + v);
        }
        let d = self.depth;
        let (own, base) = self.owner(d);
        if d != base {
            return;
        }
        match own {
            b'P' | b'V' | b'C' => {
                let step = self.pv_step(v, self.tw.pv);
                let to = (self.pos.0.plus(step.0), self.pos.1.plus(step.1));
                self.command_point(own, Dest::of(base), to, out);
            }
            b'T' => self.text_vector(v),
            // "Numeric arguments to the Screen control command are treated
            // as pixel vector specifiers ... executed as a relative
            // bracketed position argument" (§ 8.8.1.2), at the multiplier
            // `S(W(M))` sets.
            b'S' => {
                let o = self.origin();
                let step = self.pv_step(v, self.tw.pv);
                self.scroll_to((o.0.plus(step.0), o.1.plus(step.1)), out);
            }
            // `F<pv>`: a seed a pixel vector step from the last.
            _ => {
                let from = self.seed.unwrap_or(self.pos);
                let step = self.pv_step(v, self.tw.pv);
                self.flood_seed((from.0.plus(step.0), from.1.plus(step.1)), out);
            }
        }
    }

    pub(crate) fn hex(&mut self, v: u8, out: &mut dyn RegisOut) {
        self.load_digit(v);
        let _ = out;
    }

    pub(crate) fn quote_begin(&mut self, out: &mut dyn RegisOut) {
        let d = self.depth;
        if d == 0 && self.frames[0].key == b'T' {
            self.quote_start = self.pos;
        }
        if d == 0 && self.frames[0].key == b'L' {
            self.load_row_end();
            self.load_char = None;
            self.frames[0].nt = 0;
        }
        let f = &mut self.frames[d];
        f.nt = 0;
        if f.num.is_none() {
            f.text_first = On::Yes;
        }
        let _ = out;
    }

    /// One character of a quoted string, as its code in the in-use table,
    /// or [`NO_CODE`].
    pub(crate) fn quote_char(&mut self, c: u16, out: &mut dyn RegisOut) {
        let d = self.depth;
        if d == 0 && self.frames[0].key == b'T' {
            return self.text_char(c, out);
        }
        let Ok(c) = u8::try_from(c) else {
            return self.error(1, 0);
        };
        if d == 0 && self.frames[0].key == b'L' {
            // "If more than one character is present in a string, the first
            // is used, and the rest are discarded" (§ 8.7.5.3).
            if self.frames[0].nt == 0 {
                self.frames[0].nt = 1;
                self.load_begin(c);
            }
            return;
        }
        let f = &mut self.frames[d];
        if usize::from(f.nt) < f.text.len() {
            f.text[usize::from(f.nt)] = c;
            f.nt += 1;
        }
    }

    pub(crate) fn quote_end(&mut self, out: &mut dyn RegisOut) {
        let _ = out;
    }

    // ---------------------------------------------------------------
    // Instructions.
    // ---------------------------------------------------------------

    fn begin_command(&mut self, k: u8) {
        self.frames[0] = Frame::keyed(k);
        self.tw = self.write;
        self.seed = None;
        self.phase = 0;
        self.arc = None;
        self.circumferential = On::No;
        self.curve = Curve::NONE;
        self.tilt = Tilt::Ready;
        match k {
            b'S' | b'W' | b'P' | b'V' | b'C' | b'T' | b'R' | b'F' | b'L' => {}
            _ => self.error(1, k),
        }
    }

    /// The instruction in force ends: at a new keyletter or a `;`.
    /// "Temporary write options revert to their previous state upon the
    /// occurrence of a new command keyletter" (§ 8.5.2.4.2), a curve "is
    /// lost" (§ 8.5.5.4.2.1), and a fill not closed by its `)` draws
    /// nothing.
    fn end_command(&mut self, out: &mut dyn RegisOut) {
        if self.frames[0].key == b'L' {
            self.load_row_end();
            self.load_char = None;
        }
        if self.fill.is_some() {
            self.fill = None;
            self.counts.fill_abandoned += 1;
        }
        self.curve = Curve::NONE;
        self.tw = self.write;
        self.frames[0] = Frame::EMPTY;
        let _ = out;
    }

    /// A bracketed extent given to an instruction at its own level.
    fn command_extent(&mut self, own: u8, dest: Dest, e: Extent, out: &mut dyn RegisOut) {
        match own {
            b'P' | b'V' | b'C' => {
                let from = match (own, self.curve.kind) {
                    (b'C', CurveKind::Open | CurveKind::Closed) => self.curve_from(),
                    _ => self.pos,
                };
                let to = (e.x.from(from.0), e.y.from(from.1));
                self.command_point(own, dest, to, out);
            }
            // "Character escapement ... always interpreted as relative to the
            // current position, even if the provided values are unsigned.
            // Omitted values are interpreted as spacing of +0" (§ 8.4.5.2).
            b'T' => {
                let v = |p: Part| p.value().unwrap_or(RegisNum::ZERO);
                self.text.esc = (v(e.x), v(e.y));
            }
            b'L' => self.load_size(e),
            b'S' => {
                let o = self.origin();
                self.scroll_to((e.x.from(o.0), e.y.from(o.1)), out);
            }
            b'F' => {
                let from = self.seed.unwrap_or(self.pos);
                self.flood_seed((e.x.from(from.0), e.y.from(from.1)), out);
            }
            _ => {}
        }
    }

    /// The display's origin, the address at the screen's upper-left: under
    /// the scrolling model "display addressing and the display" are coupled
    /// (§ 8.8.1 b), so it is `S(A)`'s first corner whatever has scrolled.
    fn origin(&self) -> Pt {
        (self.space.x1, self.space.y1)
    }

    /// **`S[]` and `S<pv>`, the scrolling model** (§ 8.8.1.1): "position
    /// arguments specify an address in the display address space which will
    /// become the new origin", the data moving and the addresses not --
    /// "Only the data is offset, not the coordinate system"
    /// (`[[source: vt3xx-screen-control]]`). The page moves by the origin's
    /// pixel less the address's, rounded as drawing rounds. A relative or
    /// omitted part is measured from the origin, not the drawing position
    /// (Jev's lean, FROM_ORIGIN, 0.86 and 0.88 on a second ask): the
    /// VT340's "The image moves relative to the screen origin", and
    /// § 8.8.1.4.8's multiplier setting "the granularity of the display
    /// motion". The drawing position stays; the line before no longer
    /// joins the next, its end having moved.
    fn scroll_to(&mut self, to: Pt, out: &mut dyn RegisOut) {
        let (o, t) = (self.device(self.origin()), self.device(to));
        let (dx, dy) = (o.x.saturating_sub(t.x), o.y.saturating_sub(t.y));
        if dx == 0 && dy == 0 {
            return;
        }
        self.end_stroke(out);
        self.last_end = None;
        out.scroll(RegisShift { dx, dy }, self.bg);
    }

    /// A corner of `S(R[][])`, resolved as it comes: "positional
    /// specifiers, and may use relative addressing", each from the corner
    /// before it -- "multiple specifiers have cumulative action" (§ 8.4.5.1)
    /// -- and the first from the drawing position, which `S(R)` does not
    /// move: "S(R[ ][+200,+200]) sets the clip rectangle to an extent 200
    /// square, starting at the current drawing position" (§ 8.8.1.4.6 d).
    /// "If more than two are present, only the last two are used": the
    /// frame keeps the last two, as absolute positions. Jev's lean,
    /// CUMULATIVE, 0.52 and 0.48 on a second ask.
    fn clip_corner(&mut self, e: Extent) {
        let f = &mut self.frames[1];
        let from = match f.next {
            0 => self.pos,
            n => {
                let last = f.ext[usize::from(n.min(2)) - 1];
                (
                    last.x.value().unwrap_or(RegisNum::ZERO),
                    last.y.value().unwrap_or(RegisNum::ZERO),
                )
            }
        };
        let at = Extent {
            x: Part::Abs(e.x.from(from.0)),
            y: Part::Abs(e.y.from(from.1)),
        };
        if f.next >= 2 {
            f.ext[0] = f.ext[1];
            f.ext[1] = at;
        } else {
            f.ext[usize::from(f.next)] = at;
        }
        f.next = f.next.saturating_add(1);
    }

    /// Where the next point of a curve is relative to: the last point.
    fn curve_from(&self) -> Pt {
        self.pos
    }

    /// A position given to `P`, `V` or `C` by a bracket or a pixel vector.
    fn command_point(&mut self, own: u8, dest: Dest, to: Pt, out: &mut dyn RegisOut) {
        match own {
            b'P' => self.pos = to,
            b'V' => self.line_to(to, dest, out),
            _ => self.curve_point(to, dest, out),
        }
    }

    /// Draw from the position to `to`, which becomes the position; inside
    /// a fill, the end is a vertex: "These edges are determined by what
    /// would be drawn by the V and C options were they executed at command
    /// level" (§ 8.5.8.4.1).
    fn line_to(&mut self, to: Pt, dest: Dest, out: &mut dyn RegisOut) {
        let (a, b) = (self.device(self.pos), self.device(to));
        if dest == Dest::Fill {
            self.vertex(b);
        } else {
            self.draw(a, b, Close::Open, out);
        }
        self.pos = to;
    }

    fn vertex(&mut self, p: RegisPoint) {
        if let Some(f) = self.fill.as_mut() {
            if !f.push(p) && f.full == On::Yes {
                self.counts.fill_overflow += 1;
            }
        }
    }

    fn push(&mut self, s: Saved) {
        if self.nstack == POSITION_STACK {
            // "Should stack capacity be exceeded during a save, the current
            // element ... is not saved, but discarded" -- error 7.
            self.counts.stack_overflow += 1;
            self.error = (7, 0);
            return;
        }
        self.stack[self.nstack] = s;
        self.nstack += 1;
    }

    fn pop(&mut self) -> Option<Saved> {
        if self.nstack == 0 {
            // "Stack underflow will cause NO ELEMENT to be returned" --
            // error 8.
            self.counts.stack_underflow += 1;
            self.error = (8, 0);
            return None;
        }
        self.nstack -= 1;
        Some(self.stack[self.nstack])
    }

    // ---------------------------------------------------------------
    // Curves and circles (§ 8.5.5).
    // ---------------------------------------------------------------

    fn curve_point(&mut self, to: Pt, dest: Dest, out: &mut dyn RegisOut) {
        if self.curve.kind != CurveKind::None {
            return self.curve_add(to, dest, out);
        }
        // "The instruction form C[<position>] draws a circle with the
        // current drawing position as the center and [<position>] a point on
        // the circumference ... The (C) option inverts the relationship."
        let (center, edge) = match self.circumferential {
            On::No => (self.pos, to),
            On::Yes => (to, self.pos),
        };
        let sweep = self.arc.unwrap_or(RegisNum::int(360));
        let end = self.arc_points(center, edge, sweep, dest, out);
        // "The drawing position is left at the end point of the arc if the
        // (C) option was used. Otherwise, the drawing position is left at
        // the center" (§ 8.5.5.4.1.1).
        if self.circumferential == On::Yes {
            self.pos = end;
        }
    }

    /// Draw the arc about `center` from `edge` through `sweep` degrees,
    /// counterclockwise as the viewer sees it for a positive sweep, and
    /// return its end in user units.
    fn arc_points(
        &mut self,
        center: Pt,
        edge: Pt,
        sweep: RegisNum,
        dest: Dest,
        out: &mut dyn RegisOut,
    ) -> Pt {
        // "Should the absolute value of the specified angle exceed 360
        // degrees, the value executed will be plus or minus 360 degrees".
        let lim = RegisNum::int(360).raw();
        let sweep = sweep.raw().clamp(-lim, lim);
        let c = self.space.fine(center.0, center.1);
        let e = self.space.fine(edge.0, edge.1);
        let (vx, vy) = (e.0 - c.0, e.1 - c.1);
        let r2 = (vx as i128 * vx as i128 + vy as i128 * vy as i128) >> 32;
        let r = trig::isqrt(r2.min(u64::MAX as i128) as u64) as i64;
        // Segments about two page pixels long, each at most a few degrees.
        let r = r * self.scale.0 / self.scale.1;
        let len = r * 355 * sweep.abs() / 113 / (360 * 65536) / 2;
        let n = len.clamp(4, 4096);
        let whole = sweep.abs() == lim;
        let mut prev = (c.0 + vx, c.1 + vy);
        if dest == Dest::Fill {
            self.vertex(self.page(prev));
        }
        for k in 1..=n {
            let th = sweep * k / n;
            let (co, si) = (trig::cos(th), trig::sin(th));
            // Counterclockwise on a device whose `y` grows downward.
            let x = c.0 + ((vx as i128 * co as i128 + vy as i128 * si as i128) >> 30) as i64;
            let y = c.1 + ((-(vx as i128) * si as i128 + vy as i128 * co as i128) >> 30) as i64;
            if dest == Dest::Fill {
                self.vertex(self.page((x, y)));
            } else {
                let close = match whole && k == n {
                    true => Close::Loop,
                    false => Close::Open,
                };
                self.draw(self.page(prev), self.page((x, y)), close, out);
            }
            prev = (x, y);
        }
        let (ux, uy) = self.space.user_step(prev.0 - c.0, prev.1 - c.1);
        (center.0.plus(ux), center.1.plus(uy))
    }

    fn curve_begin(&mut self, kind: CurveKind) {
        let p = self.space.fine(self.pos.0, self.pos.1);
        self.curve = Curve {
            kind,
            last: [p; 4],
            first: [p; 3],
            n: 1,
            start: self.pos,
        };
    }

    fn curve_add(&mut self, to: Pt, dest: Dest, out: &mut dyn RegisOut) {
        let p = self.space.fine(to.0, to.1);
        let c = &mut self.curve;
        c.last.copy_within(1.., 0);
        c.last[3] = p;
        if c.n < 3 {
            c.first[c.n] = p;
        }
        c.n += 1;
        let ready = c.n >= 4;
        let seg = (c.last[0], c.last[1], c.last[2], c.last[3]);
        if c.kind == CurveKind::Open {
            self.pos = to;
        }
        if ready {
            self.spline(seg, dest, out);
        }
    }

    /// End a curve sequence: an open one has drawn all it draws -- "The
    /// curve segments from the start to the second point and from the
    /// penultimate to the last point are not drawn" -- and a closed one
    /// returns through its first points "so that the closed curve will have
    /// a continuous first derivative at the end point" (§ 8.5.5.4.2.1).
    fn curve_end(&mut self, dest: Dest, out: &mut dyn RegisOut) {
        let c = self.curve;
        self.curve = Curve::NONE;
        if c.kind != CurveKind::Closed {
            return;
        }
        // The cyclic sequence's points in order, the ones kept.
        let n = c.n;
        if n < 2 {
            return;
        }
        let mut ring = [(0i64, 0i64); 7];
        let mut m = 0;
        if n <= 4 {
            // Every point is in `last`'s tail.
            for &p in &c.last[4 - n..] {
                ring[m] = p;
                m += 1;
            }
            for k in 0..n {
                let a = ring[(k + n - 1) % n];
                let b = ring[k];
                let cc = ring[(k + 1) % n];
                let d = ring[(k + 2) % n];
                // Segments b -> cc: the ones streaming did not draw, which
                // for four points is every segment but the second.
                if n == 4 && k == 1 {
                    continue;
                }
                self.spline((a, b, cc, d), dest, out);
            }
        } else {
            // P(n-3), P(n-2), P(n-1) then P0, P1, P2.
            let tail = [c.last[1], c.last[2], c.last[3]];
            ring[..3].copy_from_slice(&tail);
            ring[3..6].copy_from_slice(&c.first);
            let segs = [(0, 1, 2, 3), (1, 2, 3, 4), (2, 3, 4, 5)];
            for (a, b, cc, d) in segs {
                self.spline((ring[a], ring[b], ring[cc], ring[d]), dest, out);
            }
        }
        self.pos = c.start;
    }

    /// The segment from `p1` to `p2` of a uniform Catmull-Rom spline through
    /// `p0` to `p3`: it passes through every point, its slope at a point is
    /// parallel to the chord of the points either side, four collinear
    /// points give a straight segment between the middle two, and it is the
    /// same whichever way the points are given -- § 8.5.5.4.2.1's four
    /// constraints. The text names no algorithm, and xterm, which keeps
    /// sixteen points of a curve, departs from its "unbounded number of
    /// points"; this keeps four and streams.
    fn spline(&mut self, seg: Seg, dest: Dest, out: &mut dyn RegisOut) {
        let (p0, p1, p2, p3) = seg;
        let chord = {
            let (dx, dy) = ((p2.0 - p1.0) >> 16, (p2.1 - p1.1) >> 16);
            trig::isqrt((dx * dx + dy * dy).unsigned_abs()) as i64
        };
        let n = (chord * self.scale.0 / self.scale.1 / 2).clamp(2, 1024);
        let at = |t: i64, a: i64, b: i64, c: i64, d: i64| -> i64 {
            // 0.5 * (2b + (-a + c) t + (2a - 5b + 4c - d) t^2 + (-a + 3b - 3c + d) t^3),
            // t in 1/n.
            let (a, b, c, d) = (a as i128, b as i128, c as i128, d as i128);
            let (t, n) = (t as i128, n as i128);
            let v = 2 * b * n * n * n
                + (c - a) * t * n * n
                + (2 * a - 5 * b + 4 * c - d) * t * t * n
                + (3 * b - a - 3 * c + d) * t * t * t;
            (v / (2 * n * n * n)) as i64
        };
        let mut prev = p1;
        for k in 1..=n {
            let p = (at(k, p0.0, p1.0, p2.0, p3.0), at(k, p0.1, p1.1, p2.1, p3.1));
            if dest == Dest::Fill {
                self.vertex(self.page(p));
            } else {
                self.draw(self.page(prev), self.page(p), Close::Open, out);
            }
            prev = p;
        }
    }

    // ---------------------------------------------------------------
    // Text (§ 8.5.6, § 8.7.3).
    // ---------------------------------------------------------------

    /// The display cell's width and height as user steps along the
    /// baseline and across it, turned by the string's direction.
    fn turned(&self, along: i64, across: i64) -> Pt {
        let a = i64::from(self.text.string_dir) * 65536;
        let (co, si) = (trig::cos(a), trig::sin(a));
        // Along the baseline is the direction's unit vector; across it,
        // downward, is that turned clockwise a right angle.
        let dx = (along as i128 * co as i128 + across as i128 * si as i128) >> 30;
        let dy = (-(along as i128) * si as i128 + across as i128 * co as i128) >> 30;
        self.space.user_step(dx as i64, dy as i64)
    }

    /// The escapement a size option sets: one display cell along the
    /// string's baseline (§ 8.5.6.1, "If character size options are
    /// supported, they also set character escapement as a side effect").
    fn size_escapement(&mut self) {
        self.text.esc = self.turned(i64::from(self.text.display.0) << 16, 0);
    }

    /// A text pixel vector: half the display cell, in the direction the
    /// digit names against the baseline (§ 8.5.6.2, § 8.7.3.2).
    fn text_vector(&mut self, v: u8) {
        let (w, h) = (
            i64::from(self.text.display.0) << 15,
            i64::from(self.text.display.1) << 15,
        );
        let (a, b): (i64, i64) = match v {
            0 => (w, 0),
            1 => (w, -h),
            2 => (0, -h),
            3 => (-w, -h),
            4 => (-w, 0),
            5 => (-w, h),
            6 => (0, h),
            _ => (w, h),
        };
        let s = self.turned(a, b);
        self.pos = (self.pos.0.plus(s.0), self.pos.1.plus(s.1));
    }

    fn text_char(&mut self, c: u16, out: &mut dyn RegisOut) {
        let esc = self.text.esc;
        match c {
            // "The carriage return code resets the drawing position ... to
            // the value it had at the start of the current quoted text
            // extent", or "that saved at the most recent LF".
            0x0d => self.pos = self.quote_start,
            // "moves the drawing position down one character height unit
            // ... and saves this new position".
            0x0a => {
                let s = self.turned(0, i64::from(self.text.display.1) << 16);
                self.pos = (self.pos.0.plus(s.0), self.pos.1.plus(s.1));
                self.quote_start = self.pos;
            }
            0x08 => self.pos = (self.pos.0.minus(esc.0), self.pos.1.minus(esc.1)),
            0x09 => self.pos = (self.pos.0.plus(esc.0), self.pos.1.plus(esc.1)),
            _ if c < 0x20 || c == 0x7f => {}
            _ => {
                self.glyph(c, out);
                self.pos = (self.pos.0.plus(esc.0), self.pos.1.plus(esc.1));
            }
        }
    }

    fn glyph(&mut self, c: u16, out: &mut dyn RegisOut) {
        let at = self.device(self.pos);
        let t = self.text;
        let pen = self.pen();
        let unit = (self.page_len(t.unit.0), self.page_len(t.unit.1));
        let display = (self.page_len(t.display.0), self.page_len(t.display.1));
        self.draw_char(
            c,
            t.alphabet,
            (t.gl, t.gr),
            (at, unit, display),
            (t.char_dir, t.slant),
            &pen,
            out,
        );
        self.last_end = None;
    }

    /// Draw character code `c` of `alphabet`: a built-in one through the
    /// embedder's font at the unit cell, a loaded one from its stored cell,
    /// and § 8.7.3.3's error character for "characters from a nonexistent or
    /// undefined character set", "from outside the defined extent", and
    /// "not-yet-defined characters".
    #[allow(clippy::too_many_arguments)]
    fn draw_char(
        &mut self,
        c: u16,
        alphabet: u8,
        sets: (Charset, Charset),
        cell: (RegisPoint, (i32, i32), (i32, i32)),
        turn: (i32, i32),
        pen: &RegisPen,
        out: &mut dyn RegisOut,
    ) {
        self.end_stroke(out);
        let g = self.char_glyph(c, alphabet, sets, cell, turn, Tally::Yes);
        out.glyph(&g, pen);
    }

    /// Character `c`'s glyph at `cell`, turned by `turn`: a built-in one
    /// drawn by the font, a built-in one with no font counted if `tally`
    /// says so.
    fn char_glyph(
        &mut self,
        c: u16,
        alphabet: u8,
        sets: (Charset, Charset),
        cell: (RegisPoint, (i32, i32), (i32, i32)),
        turn: (i32, i32),
        tally: Tally,
    ) -> RegisGlyph<'_> {
        let (at, unit, display) = cell;
        let code = u8::try_from(c).ok();
        let builtin_ch = match (alphabet, code) {
            (0, Some(b)) => builtin(sets.0, sets.1, b),
            _ => None,
        };
        let (rw, rh) = (
            unit.0.clamp(1, GLYPH_SIDE as i32),
            unit.1.clamp(1, GLYPH_SIDE as i32),
        );
        let mut drawn = Drawn::No;
        if let (Some(ch), Some(font)) = (builtin_ch, self.font) {
            let n = (rw * rh) as usize;
            self.glyph_bits[..n.div_ceil(64)].fill(0);
            let bits = &mut self.glyph_bits;
            font(
                ch,
                ImagePx::of(rw as u16),
                ImagePx::of(rh as u16),
                &mut |x, y| {
                    let (x, y) = (x.index(), y.index());
                    if x < rw as usize && y < rh as usize {
                        let i = y * rw as usize + x;
                        bits[i / 64] |= 1 << (i % 64);
                    }
                },
            );
            drawn = Drawn::Yes;
        }
        if builtin_ch.is_some() && drawn == Drawn::No && tally == Tally::Yes {
            self.counts.unfonted += 1;
        }
        let source = match (alphabet, builtin_ch) {
            (0, Some(ch)) => GlyphSource::Builtin {
                ch,
                cell: match drawn {
                    Drawn::Yes => Some(GlyphBits {
                        bits: &self.glyph_bits[..((rw * rh) as usize).div_ceil(64)],
                        w: rw as u16,
                        h: rh as u16,
                    }),
                    Drawn::No => None,
                },
            },
            (0, None) => GlyphSource::Error,
            (a, _) => {
                let (rows, w) = match code {
                    Some(b) => self.cell_bits(a, b),
                    None => (&[][..], 8),
                };
                match rows.is_empty() {
                    true => GlyphSource::Error,
                    false => GlyphSource::Loaded { rows, w },
                }
            }
        };
        RegisGlyph {
            source,
            at,
            unit,
            display,
            direction: turn.0,
            slant: turn.1,
        }
    }

    // ---------------------------------------------------------------
    // Command display (§ 8.9.1).
    // ---------------------------------------------------------------

    /// **Command display**: a character received is shown on the page's
    /// bottom line as it arrives -- `[[source: vt3xx-regis-intro]]`, "The
    /// last line of ReGIS commands received appears on the bottom line of
    /// the screen", "A line feed character makes the terminal erase the
    /// ReGIS command display line, along with any graphic image previously
    /// drawn there", "The terminal does not display characters that are not
    /// part of the ReGIS character set (8-bit characters and control
    /// characters)", "You can display up to 80 characters on the command
    /// line. If the terminal receives more than 80 characters without a line
    /// feed character, the terminal erases the text line before displaying
    /// the next character", and "The appearance of the characters is based
    /// only on the output map settings and the image that the characters are
    /// drawn over. Other ReGIS-settable states have no effect": the text
    /// foreground, index 7, drawn over the page, in the VT300's 80 columns
    /// of ten device pixels. Jev's lean, 2026-10-20, BUILD (0.38 and 0.28).
    ///
    /// **Magnified, the line stays on the screen's bottom line**, its
    /// characters the size they are unmagnified -- "Other ReGIS-settable
    /// states have no effect" -- drawn over the page shown and not into
    /// the plane, as the cursors are ([`Sem::display_glyphs`]); the line
    /// feed's erase clears the plane's pixels shown on the line. Jev's
    /// lean, OVERLAY, 0.88 and 0.76 on a second ask.
    pub(crate) fn display_char(&mut self, b: u8, out: &mut dyn RegisOut) {
        if b == 0x0a {
            return self.display_erase(out);
        }
        if !(0x20..0x7f).contains(&b) {
            return;
        }
        if self.display_col == 80 {
            self.display_erase(out);
        }
        let col = usize::from(self.display_col);
        if let Some(c) = self.display_line.get_mut(col) {
            *c = b;
        }
        self.display_gen = self.display_gen.wrapping_add(1);
        let (at, unit) = self.display_cell(col);
        let ascii = (Charset::Ascii, Charset::Iso96(Iso96::Latin1));
        match self.zoom == RegisZoom::ONE {
            true => {
                let pen = self.display_pen(WriteMode::Overlay);
                self.draw_char(u16::from(b), 0, ascii, (at, unit, unit), (0, 0), &pen, out);
            }
            // Drawn over the page shown; a character with no font counted
            // here, once.
            false => {
                let _ =
                    self.char_glyph(u16::from(b), 0, ascii, (at, unit, unit), (0, 0), Tally::Yes);
            }
        }
        self.display_col += 1;
    }

    /// The command display's column `col`: its cell's upper-left on the
    /// page and its size, ten device pixels by twenty.
    fn display_cell(&self, col: usize) -> (RegisPoint, (i32, i32)) {
        let x = i64::try_from(col).unwrap_or(0) * 10 * 65536;
        let at = self.page((x, i64::from(REGIS_HEIGHT - 20) * 65536));
        (at, (self.page_len(10), self.page_len(20)))
    }

    /// The command display's pen: the text foreground, index 7, written
    /// `mode` over every plane, the clip off -- "The appearance of the
    /// characters is based only on the output map settings and the image
    /// that the characters are drawn over".
    fn display_pen(&self, mode: WriteMode) -> RegisPen {
        RegisPen {
            mode,
            negate: Negate::Off,
            fg: RegisIndex(7),
            bg: self.bg,
            planes: RegisPlanes::ALL,
            pattern: LinePattern::SOLID,
            pattern_mul: 1,
            shade: None,
            // The terminal's own line, no drawing of the program's.
            clip: RegisClip::Off,
            width: 1,
            micro: RegisMicro::NONE,
            blink: RegisBlink::Off,
        }
    }

    /// **The command display's line on the page**, the device's bottom
    /// twenty rows across its width, in page pixels.
    pub(crate) fn display_band(&self) -> RegisRect {
        let a = self.page((0, i64::from(REGIS_HEIGHT - 20) * 65536));
        let b = self.page((
            i64::from(REGIS_WIDTH) * 65536,
            i64::from(REGIS_HEIGHT) * 65536,
        ));
        RegisRect {
            x0: a.x,
            y0: a.y,
            x1: b.x,
            y1: b.y,
        }
    }

    /// The command display's line erased to the background, "along with
    /// any graphic image previously drawn there": magnified, the plane's
    /// pixels shown on the line, a whole block each.
    fn display_erase(&mut self, out: &mut dyn RegisOut) {
        let band = self.display_band();
        let (zx, zy) = (i32::from(self.zoom.x.max(1)), i32::from(self.zoom.y.max(1)));
        let (x0, y0, x1, y1) = (
            band.x0.div_euclid(zx),
            band.y0.div_euclid(zy),
            band.x1.saturating_add(zx - 1).div_euclid(zx),
            band.y1.saturating_add(zy - 1).div_euclid(zy),
        );
        let points = [
            RegisPoint { x: x0, y: y0 },
            RegisPoint { x: x1, y: y0 },
            RegisPoint { x: x1, y: y1 },
            RegisPoint { x: x0, y: y1 },
        ];
        let pen = self.display_pen(WriteMode::Erase);
        out.fill(&points, &[4], &pen);
        self.display_col = 0;
        self.display_line = [0; 80];
        self.display_gen = self.display_gen.wrapping_add(1);
    }

    /// **The command display's characters, magnified**: each as a glyph at
    /// its unscaled page cell, with the pen it is drawn with, to `f`, for
    /// the page shown; nothing unmagnified, where the plane holds them.
    /// A character with no font is counted where it was received.
    pub(crate) fn display_glyphs(&mut self, f: &mut dyn FnMut(&RegisGlyph<'_>, &RegisPen)) {
        if self.zoom == RegisZoom::ONE {
            return;
        }
        let pen = self.display_pen(WriteMode::Overlay);
        let ascii = (Charset::Ascii, Charset::Iso96(Iso96::Latin1));
        for col in 0..self.display_line.len() {
            let b = self.display_line[col];
            if b == 0 {
                continue;
            }
            let (at, unit) = self.display_cell(col);
            let g = self.char_glyph(u16::from(b), 0, ascii, (at, unit, unit), (0, 0), Tally::No);
            f(&g, &pen);
        }
    }

    /// What [`Sem::display_glyphs`] would draw changes when this does.
    pub(crate) fn display_gen(&self) -> u32 {
        self.display_gen
    }

    // ---------------------------------------------------------------
    // Load (§ 8.7.5).
    // ---------------------------------------------------------------

    fn loading(&mut self) -> Option<&mut Alphabet> {
        match self.load {
            1..=3 => Some(&mut self.alphabets[usize::from(self.load - 1)]),
            _ => None,
        }
    }

    fn load_begin(&mut self, c: u8) {
        let Some(a) = self.loading() else {
            // "The attributes of the default alphabet (alphabet 0) may not
            // be modified" -- error 5.
            self.counts.load_refused += 1;
            self.error = (5, 0);
            return;
        };
        let ext = usize::from(a.extent);
        match char_slot(c) {
            Some(i) if i < ext => {
                a.rows[i] = [0; CELL_HEIGHT as usize];
                a.defined[i / 32] |= 1 << (i % 32);
                self.load_char = Some(i);
                self.load_row = 0;
                self.load_digits = 0;
                self.load_value = 0;
            }
            _ => {
                self.counts.load_refused += 1;
                self.load_char = None;
            }
        }
    }

    /// A hexadecimal digit of a cell: "Groups of hex digits, each of which
    /// specify the contents of one scan line ... are separated by commas, or
    /// by exceeding the maximum number of hex digits needed to specify one
    /// scan line" (§ 8.7.5.2).
    fn load_digit(&mut self, v: u8) {
        if self.load_char.is_none() {
            return;
        }
        let w = self.loading().map_or(8, |a| a.w);
        let per_row = w.div_ceil(4);
        self.load_value = (self.load_value << 4) | u32::from(v);
        self.load_digits += 1;
        if self.load_digits == per_row {
            // "if the width of the character cell is not a multiple of four,
            // any unneeded, low order bits of the last hex digit applied to
            // that row are discarded".
            let row = self.load_value >> (u32::from(per_row) * 4 - u32::from(w));
            self.store_row(row);
        }
    }

    /// A row ended by a comma or the cell's end: "If too few digits are
    /// specified to fill a row, they fill the low order portion of the
    /// mask, setting the high order portion to zero".
    fn load_row_end(&mut self) {
        if self.load_digits > 0 {
            let w = self.loading().map_or(8, |a| a.w);
            let row = self.load_value & ((1u32 << w) - 1);
            self.store_row(row);
        }
    }

    fn store_row(&mut self, row: u32) {
        let (Some(i), r) = (self.load_char, self.load_row) else {
            return;
        };
        if let Some(a) = self.loading() {
            // "If more lines are specified than will fit in the storage
            // cell, these extra lines are ignored".
            if r < a.h {
                a.rows[i][usize::from(r)] = row as u16;
            }
        }
        self.load_row = r.saturating_add(1);
        self.load_digits = 0;
        self.load_value = 0;
    }

    /// `L[h,w]`: "A bracketed coordinate specifier defines the storage
    /// size, in bits per character ... The selected alphabet is cleared when
    /// its storage size is specified" (§ 8.7.5.1); "missing values are
    /// interpreted as 0" (§ 8.4.5.2), and a size with a zero side is none.
    /// **Height first**, as § 8.8.5.1's error list writes it, "In the syntax
    /// L[h,w], the height "h" exceeded the implementation limit", the one
    /// place the text gives the order: decided by Jev 2026-10-20
    /// (HEIGHT_FIRST, 0.90 and 0.90).
    fn load_size(&mut self, e: Extent) {
        let v = |p: Part| p.value().map_or(0, |v| v.trunc());
        let (h, w) = (v(e.x), v(e.y));
        if !(1..=i32::from(CELL_WIDTH)).contains(&w) || !(1..=i32::from(CELL_HEIGHT)).contains(&h) {
            self.counts.load_refused += 1;
            self.error = (5, 0);
            return;
        }
        match self.loading() {
            Some(a) => {
                a.w = w as u8;
                a.h = h as u8;
                a.clear();
            }
            None => {
                self.counts.load_refused += 1;
                self.error = (5, 0);
            }
        }
    }

    // ---------------------------------------------------------------
    // Options.
    // ---------------------------------------------------------------

    /// The key at depth `d` closes, by the next key at its level, its
    /// group's `)`, or a `;`.
    fn end_key(&mut self, d: usize, out: &mut dyn RegisOut) {
        let f = self.frames[d];
        if f.key == 0 {
            return;
        }
        let (own, base) = self.owner(d);
        let r = d - base;
        if r == 0 {
            return;
        }
        let parent = self.frames[d - 1];
        // The last suboption a key had, which `W(S(X))`, `R(P(I))` and
        // `R(M(x))` read where their key closes.
        if r >= 2 {
            self.frames[d - 1].sub = f.key;
        }
        // A colour group's letters: `I(R)`, `I(H120L50S100)`, the groups of
        // `S(M)`.
        if self.is_color_group(d) {
            let p = &mut self.frames[d - 1];
            let comp = match f.num {
                Some(v) => Comp::Value(v.trunc()),
                None => Comp::Named,
            };
            match f.key {
                b'H' => p.h = comp,
                b'L' => p.l = comp,
                b'S' => p.s = comp,
                b'R' | b'G' | b'B' | b'C' | b'Y' | b'M' | b'W' | b'D' => p.letter = f.key,
                // `S(M1(AR))`: the VT340's "only the color value" -- one
                // map serves both, so it changes nothing.
                b'A' => {}
                _ => self.error(1, f.key),
            }
            return;
        }
        match own {
            b'W' => self.write_option(&f, &parent, r, d, Target::Persistent, out),
            _ if r >= 2 && self.frames[base + 1].key == b'W' => {
                self.write_option(&f, &parent, r - 1, d, Target::Temporary, out)
            }
            b'S' => self.screen_option(&f, &parent, r, out),
            b'P' | b'V' => self.stack_option(own, &f, r, Dest::of(base), out),
            b'C' => self.curve_option(&f, r, Dest::of(base), out),
            b'T' => self.text_option(&f, &parent, r),
            b'R' => self.report_option(&f, &parent, r, out),
            b'L' => self.load_option(&f, r),
            b'F' => self.fill_option(&f, r, d, out),
            _ => {}
        }
    }

    /// Whether the group at `d` is a colour specifier: under `W(I)`,
    /// `S(I)`, or `S(M)`.
    fn is_color_group(&self, d: usize) -> bool {
        if d < 2 {
            return false;
        }
        let parent = self.frames[d - 1].key;
        let (own, base) = self.owner(d - 1);
        let r = d - 1 - base;
        let under_w = own == b'W' || (r >= 2 && self.frames[base + 1].key == b'W');
        let flood = own == b'F' && r == 2 && self.frames[base + 1].key == b'B';
        match parent {
            b'I' => under_w || (own == b'S' && r == 1) || flood,
            b'M' => own == b'S' && r == 1,
            _ => false,
        }
    }

    /// A group's `)`: a fill's option list ends and is drawn, an `F(F ...)`
    /// sub-polygon's contour ends, a colour group under `S(M)` sets its
    /// entry.
    fn end_group(&mut self, d: usize, out: &mut dyn RegisOut) {
        let parent = self.frames[d - 1].key;
        if d == 1 && self.frames[0].key == b'F' {
            return self.fill_draw(out);
        }
        if d == 2 && self.frames[0].key == b'F' && parent == b'F' {
            if let Some(f) = self.fill.as_mut() {
                f.close();
            }
            return;
        }
        let (own, base) = self.owner(d - 1);
        if parent == b'M' && own == b'S' && d - 1 - base == 1 {
            let p = self.frames[d - 1];
            if let (Some(i), Some(c)) = (p.int(), p.color()) {
                if let Some(px) = c.pixel() {
                    self.map[RegisIndex::wrap(i).slot()] = px;
                    out.recolor();
                }
            }
            let p = &mut self.frames[d - 1];
            p.letter = 0;
            p.h = Comp::Absent;
            p.l = Comp::Absent;
            p.s = Comp::Absent;
        }
    }

    fn write_option(
        &mut self,
        f: &Frame,
        parent: &Frame,
        r: usize,
        d: usize,
        t: Target,
        out: &mut dyn RegisOut,
    ) {
        let _ = out;
        let mut w = match t {
            Target::Persistent => self.write,
            Target::Temporary => self.tw,
        };
        let pk = parent.key;
        // "Any writing mode command overrides all detailed settings"
        // (§ 8.8.3.4): the five of § 8.7.2.4.1.
        if r == 1 && matches!(f.key, b'V' | b'R' | b'C' | b'E' | b'N') {
            w.micro = RegisMicro::NONE;
        }
        match (r, f.key) {
            (1, b'V') => w.mode = WriteMode::Overlay,
            (1, b'R') => w.mode = WriteMode::Replace,
            (1, b'C') => w.mode = WriteMode::Complement,
            (1, b'E') => w.mode = WriteMode::Erase,
            (1, b'N') => {
                w.negate = match f.int() {
                    Some(0) | None => Negate::Off,
                    _ => Negate::On,
                }
            }
            // `W(W<i>(P<j>N<k>))`: the entries for old index `i`, an index
            // past the device's mapped back as every index is (§ 8.6.2.2.1).
            (1, b'W') => {}
            (2, b'P' | b'N') if pk == b'W' => {
                if let (Some(i), Some(v)) = (parent.int(), f.int()) {
                    let bit = match f.key {
                        b'P' => PatternBit::One,
                        _ => PatternBit::Zero,
                    };
                    w.micro.load(RegisIndex::wrap(i), bit, RegisIndex::wrap(v));
                }
            }
            (1, b'F') => {
                if let Some(n) = f.int() {
                    w.planes = RegisPlanes::of(n);
                }
            }
            (1, b'I') => {
                if let Some(c) = f.color() {
                    if let Some(px) = c.pixel() {
                        w.fg = regis_closest(&self.map, px);
                    }
                } else if let Some(n) = f.int() {
                    w.fg = RegisIndex::wrap(n);
                }
            }
            // "A value of 0 is taken to be 1" (§ 8.5.3.4.1).
            (1, b'M') => {
                if let Some(v) = f.num {
                    w.pv = if v == RegisNum::ZERO {
                        RegisNum::ONE
                    } else {
                        v.abs()
                    };
                }
            }
            (1, b'P') => {
                let digits = &f.digits[..usize::from(f.nd)];
                if !digits.is_empty() {
                    w.pattern = pattern_of(digits);
                }
            }
            // `W(P(M<m>))`: "Implementations must provide for multipliers as
            // large as 16" -- the VT300's "The minimum value is 1, the
            // maximum value is 16".
            (2, b'M') if pk == b'P' => {
                if let Some(n) = f.int() {
                    w.pattern_mul = n.clamp(1, 16) as u16;
                }
            }
            (1, b'S') => self.shade_option(&mut w, f, d),
            (2, b'X' | b'Y' | b'P') if pk == b'S' => {
                // Volatile suboptions, read by `shade_option` from `sub`.
                self.frames[d - 1].sub = f.key;
                return;
            }
            // "Fractional line widths are allowed"; a negative one is read
            // as its size, as `W(M)` reads one.
            (1, b'L') => {
                if let Some(v) = f.num {
                    w.width = v.abs();
                }
            }
            // "For values of <n> greater than zero, set the blink attribute
            // on all foreground data drawn until blink is disabled"; `W(A0)`
            // disables it, and so does a negative value, not greater than
            // zero. Without a value nothing changes.
            (1, b'A') => match f.int() {
                Some(n) if n > 0 => w.blink = On::Yes,
                Some(_) => w.blink = On::No,
                None => {}
            },
            _ if r >= 2 => {}
            _ => self.error(1, f.key),
        }
        match t {
            Target::Persistent => {
                self.write = w;
                self.tw = w;
            }
            Target::Temporary => self.tw = w,
        }
    }

    /// `W(S)`: "A numeric argument to the S option is interpreted as a
    /// boolean switch ... A single quoted character to the S option causes
    /// the shading pattern to be taken from the ... cell for that character
    /// ... A bracketed position specifier defines the location to which
    /// shading will be executed ... If this argument is not specified, the
    /// current drawing position is assumed" (§ 8.7.2.4.2.3).
    fn shade_option(&mut self, w: &mut Write, f: &Frame, d: usize) {
        let dim = match f.sub {
            b'X' => ShadeDim::X,
            b'P' => ShadeDim::P,
            _ => ShadeDim::Y,
        };
        let _ = d;
        let reference = match f.next {
            0 => self.pos,
            _ => (f.ext[0].x.from(self.pos.0), f.ext[0].y.from(self.pos.1)),
        };
        let at = self.device(reference);
        let to = match dim {
            ShadeDim::Y => ShadeTo::Row(at.y),
            ShadeDim::X => ShadeTo::Column(at.x),
            ShadeDim::P => ShadeTo::Point(at),
        };
        let enable = |w: &mut Write, with: ShadeWith| {
            w.shade = On::Yes;
            w.shade_with = Some(with);
            w.shade_to = to;
        };
        if f.nt > 0 {
            let with = ShadeWith::Char {
                c: f.text[0],
                alphabet: self.text.alphabet,
                unit: self.text.unit,
            };
            enable(w, with);
        } else if let Some(n) = f.int() {
            match n {
                0 => w.shade = On::No,
                _ => enable(w, ShadeWith::Line(w.pattern, w.pattern_mul)),
            }
        } else if f.next > 0 {
            w.shade_to = to;
        }
    }

    fn screen_option(&mut self, f: &Frame, parent: &Frame, r: usize, out: &mut dyn RegisOut) {
        match (r, f.key) {
            (1, b'A') => {
                let before = self.zoom;
                self.address(f);
                if self.zoom != before {
                    out.recolor();
                }
            }
            (1, b'E') => {
                // "S(E) clears the position stack ... Shading state ... is
                // disabled on S(E)"; the VT300 "Stops any curve
                // interpolation".
                // Every pixel written, every mark with it, and the command
                // display's characters drawn over a magnified page.
                self.stroke = None;
                out.erase(self.bg);
                self.display_line = [0; 80];
                self.display_gen = self.display_gen.wrapping_add(1);
                self.last_end = None;
                self.nstack = 0;
                self.write.shade = On::No;
                self.tw.shade = On::No;
                // "Application of the blink attribute is disabled by
                // W(A0), S(E), or valid S(A)" (§ 8.8.3.1).
                self.write.blink = On::No;
                self.tw.blink = On::No;
                self.curve = Curve::NONE;
            }
            (1, b'I') => {
                if let Some(c) = f.color() {
                    if let Some(px) = c.pixel() {
                        self.bg = regis_closest(&self.map, px);
                    }
                } else if let Some(n) = f.int() {
                    self.bg = RegisIndex::wrap(n);
                }
            }
            // `S(M)` sets its entries as each group closes.
            (1, b'M') => {}
            // "Devices must accept and properly execute values of <n> as
            // large as 2000"; the VT300 takes 32,767.
            // **Interpretation is suspended** for the ticks: under
            // `RegisHold::On` the caller stops at the character that ends
            // the option and goes on once they have passed; a cascade,
            // "S(T60,T60,T60)", stops at each.
            (1, b'T') => {
                if let Some(n) = f.int() {
                    let t = n.clamp(0, 32_767) as u32;
                    match self.hold {
                        RegisHold::On => self.wait = self.wait.saturating_add(t),
                        RegisHold::Off => self.counts.delays_unheld += 1,
                    }
                    out.delay(RegisTicks::of(t));
                }
            }
            // `S(F)`, "image complete": nothing to feed on a screen.
            (1, b'F') => {}
            (1, b'W') => {}
            (1, b'C') => self.cursor_option(f, CursorWhich::Both),
            (2, b'H') if parent.key == b'C' => self.cursor_option(f, CursorWhich::Output),
            (2, b'I') if parent.key == b'C' => self.cursor_option(f, CursorWhich::Input),
            (1, b'R') => self.clip_option(f),
            // `S(S<n>)`: isotropic; its `(X<n>)` and `(Y<n>)` one way each.
            (1, b'S') => self.scale(f, true, true, out),
            (2, b'X') if parent.key == b'S' => self.scale(f, true, false, out),
            (2, b'Y') if parent.key == b'S' => self.scale(f, false, true, out),
            (1, b'H') => self.hard_copy(f, out),
            (2, b'P') if parent.key == b'H' => {
                let part = |p: Part| match p.value() {
                    Some(v) => u16::try_from(v.trunc().max(0)).unwrap_or(u16::MAX),
                    None => 0,
                };
                self.hard_offset = RegisOffset {
                    x: part(f.ext[0].x),
                    y: part(f.ext[0].y),
                };
            }
            // `S(P)` the display page, `S(D)` data movement: Open
            // Extensions (§ 8.8.1). `S(N)`: no DEC text here defines it.
            (1, b'P' | b'D' | b'N') => self.counts.unbuilt += 1,
            (2, _) if parent.key == b'H' || parent.key == b'C' || parent.key == b'S' => {}
            _ if r >= 2 => {}
            _ => self.error(1, f.key),
        }
    }

    /// **`S(S<n>)`, the scale the display is shown at** (§ 8.8.1.4.7):
    /// whole factors, pixel replication being whole pixels, a fraction
    /// truncated; "Negative values for scaling factors are not allowed",
    /// and neither is zero, no factor at all, so either changes nothing; "A
    /// value which exceeds the implementation limit in a device is mapped
    /// to the implementation limit", [`REGIS_SCALE`]. A change repaints the
    /// page.
    fn scale(&mut self, f: &Frame, x: bool, y: bool, out: &mut dyn RegisOut) {
        let Some(v) = f.num else {
            return;
        };
        let n = v.trunc();
        if n < 1 {
            return;
        }
        let n = n.min(i32::from(REGIS_SCALE)) as u8;
        let before = self.zoom;
        if x {
            self.zoom.x = n;
        }
        if y {
            self.zoom.y = n;
        }
        if self.zoom != before {
            out.recolor();
        }
    }

    /// `S(R<n>[][])`, the clip rectangle (§ 8.8.1.4.6): "Both corners must
    /// be specified. If only one specifier is present, the extent of the
    /// clip rectangle is not changed"; "The extent of the clip rectangle
    /// can be set by an instance of the command without changing the
    /// enabled state", and the number alone switches it, "If <n> is equal
    /// to zero, then clipping is disabled", above zero the interior, below
    /// it the exterior.
    fn clip_option(&mut self, f: &Frame) {
        if f.next >= 2 {
            let p = |e: Extent| {
                (
                    e.x.value().unwrap_or(RegisNum::ZERO),
                    e.y.value().unwrap_or(RegisNum::ZERO),
                )
            };
            self.clip.a = p(f.ext[0]);
            self.clip.b = p(f.ext[1]);
        }
        if let Some(v) = f.num {
            self.clip.mode = match v {
                v if v == RegisNum::ZERO => ClipMode::Off,
                v if v.is_negative() => ClipMode::Shield,
                _ => ClipMode::Interior,
            };
        }
    }

    /// **`S(H)`, hard copy** (070-8 § 8.8.1.4.4): the whole visible screen,
    /// or the rectangle between two positions, or between one and the
    /// drawing position. The positions are positional, "relate to the
    /// imposed coordinate space, and may be expressed in terms of the
    /// current position" (§ 8.4.5.2 item 6), the second relative to the
    /// first as "multiple specifiers have cumulative action" (§ 8.4.5.1;
    /// Jev, CUMULATIVE, 0.97/0.97); "Numeric arguments to the H option are
    /// NOT interpreted as pixel vector specifiers". The texts disagree, and
    /// two readings are departures: the VT330/VT340 manual's, relative
    /// values "relative to the cursor position, not to any other position
    /// specified" (`[[source: vt3xx-gp-002]]` p. 29), and § 8.8.1.4.4's,
    /// "S(H[][]) is a non-positional use of bracketed extents ... always
    /// interpreted absolutely".
    fn hard_copy(&mut self, f: &Frame, out: &mut dyn RegisOut) {
        let area = match f.next {
            0 => RegisHardCopy::Screen,
            n => {
                let a = (f.ext[0].x.from(self.pos.0), f.ext[0].y.from(self.pos.1));
                let b = match n {
                    1 => self.pos,
                    _ => (f.ext[1].x.from(a.0), f.ext[1].y.from(a.1)),
                };
                RegisHardCopy::Rect(self.device(a), self.device(b))
            }
        };
        out.hard_copy(area, self.hard_offset);
    }

    /// `S(A[][])`: valid with two extents whose four parts are present and
    /// none negative -- "Missing elements inside the brackets cause the
    /// command to be ignored" and "if the specified address space would
    /// require absolute negative coordinate values ... the command is
    /// ignored" -- and an extent of some size each way, which the scale
    /// divides by. Then the defaults of § 8.5.1.4.1.4 and § 8.7.1.4.1.
    fn address(&mut self, f: &Frame) {
        let parts = [f.ext[0].x, f.ext[0].y, f.ext[1].x, f.ext[1].y];
        let mut v = [RegisNum::ZERO; 4];
        for (i, p) in parts.iter().enumerate() {
            match p.value() {
                Some(n) if !n.is_negative() && f.next >= 2 => v[i] = n,
                _ => {
                    self.counts.space_refused += 1;
                    return;
                }
            }
        }
        if f.next > 2 {
            self.error = (2, 0);
        }
        if v[0] == v[2] || v[1] == v[3] {
            self.counts.space_refused += 1;
            return;
        }
        self.space = RegisSpace {
            x1: v[0],
            y1: v[1],
            x2: v[2],
            y2: v[3],
        };
        self.defaults();
    }

    /// What a valid `S(A)` resets.
    fn defaults(&mut self) {
        self.pos = (RegisNum::ZERO, RegisNum::ZERO);
        self.hard_offset = RegisOffset::ZERO;
        self.last_end = None;
        self.write = Write::DEFAULT;
        self.tw = Write::DEFAULT;
        self.bg = RegisIndex::ZERO;
        self.nstack = 0;
        let alphabet_names = self.text;
        self.text = Text::DEFAULT;
        let _ = alphabet_names;
        self.saved_text = None;
        self.size_escapement();
        self.load = 1;
        // "Cursor visibility is set to ON, and the default cursors are
        // selected" (§ 8.8.1.4.1 g), and one-shot input "is set by
        // executing a valid S(A)"; "R(I0) or S(A) exit multiple input mode"
        // (§ 8.8.5.2).
        self.cursors = Cursors::DEFAULT;
        self.input.mode = RegisInputMode::OneShot;
        // "The clip rectangle is set to the range indicated by S(A), and
        // clipping is disabled" (§ 8.8.1.4.1 d); "Screen scaling is set to
        // 1" (h).
        self.clip = Clip::of(self.space);
        self.zoom = RegisZoom::ONE;
        // "This default is restored upon execution of a valid S(A)"
        // (§ 8.8.6.3.1.1).
        self.bound = RegisBound::Seed;
    }

    /// `S(C ...)` (§ 8.8.1.4.2): at the `C` itself a number turns the
    /// output cursor on or off -- "S(C1) enables the cursor display, S(C0)
    /// disables it", the VT340's "0 turns the output cursor off" -- and a
    /// quoted pair with its offset defines both cursors, "for all possible
    /// cursors at once"; under `H` and `I` a number picks the output's or
    /// the input's style and a quoted pair defines that one. A number no
    /// style has changes nothing, error 1.
    fn cursor_option(&mut self, f: &Frame, which: CursorWhich) {
        if f.nt > 0 {
            let pair = self.cursor_pair(f);
            if matches!(which, CursorWhich::Output | CursorWhich::Both) {
                self.cursors.output = RegisCursorStyle::Pair;
                self.cursors.output_pair = pair;
            }
            if matches!(which, CursorWhich::Input | CursorWhich::Both) {
                self.cursors.input = RegisCursorStyle::Pair;
                self.cursors.input_pair = pair;
            }
        }
        let set = match which {
            CursorWhich::Both => {
                if let Some(n) = f.int() {
                    self.cursors.shown = match n {
                        0 => Shown::No,
                        _ => Shown::Yes,
                    };
                }
                Some(())
            }
            CursorWhich::Output if f.nt == 0 => self.cursors.set_output(f.int()),
            CursorWhich::Input if f.nt == 0 => self.cursors.set_input(f.int()),
            _ => Some(()),
        };
        if set.is_none() {
            self.error(1, f.key);
        }
    }

    /// The two characters of a quoted string, from "the alphabet currently
    /// selected for drawing by T(A)", as device-pixel masks clipped to
    /// [`REGIS_CURSOR_CELL`], and the extent's offset in cell pixels.
    fn cursor_pair(&mut self, f: &Frame) -> RegisCursorPair {
        let off = (
            f.ext[0].x.value().map_or(0, |v| v.trunc()),
            f.ext[0].y.value().map_or(0, |v| v.trunc()),
        );
        let mut pair = RegisCursorPair {
            fg: [0; 24],
            bg: [0; 24],
            w: 0,
            h: 0,
            off,
        };
        let chars = [f.text[0], f.text[1]];
        for (i, &c) in chars[..usize::from(f.nt.min(2))].iter().enumerate() {
            let (mask, w, h) = self.cursor_mask(c);
            pair.w = pair.w.max(w);
            pair.h = pair.h.max(h);
            match i {
                0 => pair.fg = mask,
                _ => pair.bg = mask,
            }
        }
        pair
    }

    /// One character of the drawing alphabet as a mask: a loaded cell's
    /// rows, a built-in one rasterised by the embedder's font at the unit
    /// cell, or the solid error character for one not defined (§ 8.7.3.3).
    fn cursor_mask(&mut self, c: u8) -> ([u16; 24], i32, i32) {
        let (cw, ch) = REGIS_CURSOR_CELL;
        let mut mask = [0u16; 24];
        let t = self.text;
        if t.alphabet == 0 {
            let (w, h) = (t.unit.0.clamp(1, cw), t.unit.1.clamp(1, ch));
            match (builtin(t.gl, t.gr, c), self.font) {
                (Some(chr), Some(font)) => {
                    font(
                        chr,
                        ImagePx::of(w as u16),
                        ImagePx::of(h as u16),
                        &mut |x, y| {
                            let (x, y) = (x.index(), y.index());
                            if x < w as usize && y < h as usize {
                                mask[y] |= 0x8000 >> x;
                            }
                        },
                    );
                }
                (Some(_), None) => self.counts.unfonted += 1,
                (None, _) => mask[..h as usize].fill(solid_row(w)),
            }
            return (mask, w, h);
        }
        let (rows, w) = self.cell_bits(t.alphabet, c);
        let w = i32::from(w).clamp(1, cw);
        if rows.is_empty() {
            let h = i32::from(self.alphabets[usize::from(t.alphabet - 1)].h).clamp(1, ch);
            mask[..h as usize].fill(solid_row(w));
            return (mask, w, h);
        }
        let aw = i32::from(self.alphabets[usize::from(t.alphabet - 1)].w.max(1));
        let h = (rows.len() as i32).min(ch);
        for (y, &row) in rows.iter().take(h as usize).enumerate() {
            for x in 0..w {
                if row & (1 << (aw - 1 - x)) != 0 {
                    mask[y] |= 0x8000 >> x;
                }
            }
        }
        (mask, w, h)
    }

    /// The output cursor where the drawing position is, if `S(C)` shows it.
    pub(crate) fn output_cursor(&self) -> Option<RegisCursor> {
        if self.cursors.shown == Shown::No {
            return None;
        }
        let at = self.space.fine(self.pos.0, self.pos.1);
        Some(self.cursor_at(self.cursors.output, &self.cursors.output_pair, at, at))
    }

    /// The input cursor where graphics input has it.
    pub(crate) fn input_cursor(&self) -> RegisCursor {
        let i = self.input;
        self.cursor_at(self.cursors.input, &self.cursors.input_pair, i.at, i.anchor)
    }

    fn cursor_at(
        &self,
        style: RegisCursorStyle,
        pair: &RegisCursorPair,
        at: Fine,
        anchor: Fine,
    ) -> RegisCursor {
        let p = self.shown(at);
        match style {
            RegisCursorStyle::Crosshair => RegisCursor::crosshair(
                p,
                self.shown((
                    i64::from(REGIS_WIDTH - 1) << 16,
                    i64::from(REGIS_HEIGHT - 1) << 16,
                )),
            ),
            RegisCursorStyle::Diamond => RegisCursor::diamond(p, self.page_len(10)),
            RegisCursorStyle::RubberLine => RegisCursor::line(self.shown(anchor), p),
            RegisCursorStyle::RubberRect => RegisCursor::rect(self.shown(anchor), p),
            RegisCursorStyle::Pair => {
                let o = (
                    at.0 - (i64::from(pair.off.0) << 16),
                    at.1 - (i64::from(pair.off.1) << 16),
                );
                RegisCursor::pair(
                    self.shown(o),
                    (self.page_len(pair.w), self.page_len(pair.h)),
                    *pair,
                )
            }
        }
    }

    /// Where a position on the device is shown: its page pixel through the
    /// scale `S(S)` shows the page at. A cursor marks the display, not the
    /// bitmap, and keeps its size.
    fn shown(&self, p: Fine) -> RegisPoint {
        let q = self.page(p);
        RegisPoint {
            x: q.x.saturating_mul(i32::from(self.zoom.x)),
            y: q.y.saturating_mul(i32::from(self.zoom.y)),
        }
    }

    /// A page pixel as a position on the device, held to the device: the
    /// plane's pixel the scale shows there.
    pub(crate) fn device_of_page(&self, x: i32, y: i32) -> Fine {
        let (x, y) = (
            x.div_euclid(i32::from(self.zoom.x.max(1))),
            y.div_euclid(i32::from(self.zoom.y.max(1))),
        );
        let (n, d) = (i128::from(self.scale.0.max(1)), i128::from(self.scale.1));
        let f = |v: i32, max: i32| -> i64 {
            let v = (i128::from(v) << 16) * d / n;
            v.clamp(0, i128::from(max - 1) << 16) as i64
        };
        (f(x, REGIS_WIDTH), f(y, REGIS_HEIGHT))
    }

    /// The input position as a report's bracketed extent in user
    /// coordinates, "an absolute bracketed extent in user coordinates"
    /// (070-13 § 5.1), to a tenth ([`tenths`]).
    pub(crate) fn input_position(&self, out: &mut [u8; 56]) -> usize {
        let (ux, uy) = self.space.user_step(self.input.at.0, self.input.at.1);
        let (x, y) = (self.space.x1.plus(ux), self.space.y1.plus(uy));
        let mut n = 0;
        let mut put = |s: &[u8], n: &mut usize| {
            out[*n..*n + s.len()].copy_from_slice(s);
            *n += s.len();
        };
        let mut b = [0u8; 24];
        put(b"[", &mut n);
        let k = tenths(x.raw(), &mut b);
        put(&b[..k], &mut n);
        put(b",", &mut n);
        let k = tenths(y.raw(), &mut b);
        put(&b[..k], &mut n);
        put(b"]", &mut n);
        n
    }

    fn stack_option(&mut self, own: u8, f: &Frame, r: usize, dest: Dest, out: &mut dyn RegisOut) {
        if r != 1 {
            return;
        }
        match f.key {
            b'B' => self.push(Saved::At(self.pos)),
            b'S' => self.push(Saved::Dummy),
            b'E' => match self.pop() {
                Some(Saved::At(p)) => match own {
                    b'V' => self.line_to(p, dest, out),
                    _ => self.pos = p,
                },
                Some(Saved::Dummy) | None => {}
            },
            b'W' => {}
            // `P(P<n>)`, drawing surfaces: an Open Extension.
            b'P' if own == b'P' => self.counts.unbuilt += 1,
            k => self.error(1, k),
        }
    }

    fn curve_option(&mut self, f: &Frame, r: usize, dest: Dest, out: &mut dyn RegisOut) {
        if r != 1 {
            return;
        }
        match f.key {
            b'A' => self.arc = f.num,
            b'C' => self.circumferential = On::Yes,
            b'S' => self.curve_begin(CurveKind::Open),
            b'B' => self.curve_begin(CurveKind::Closed),
            b'E' => self.curve_end(dest, out),
            b'W' => {}
            k => self.error(1, k),
        }
    }

    fn text_option(&mut self, f: &Frame, parent: &Frame, r: usize) {
        if r == 2 && parent.key == b'A' {
            // `T(A0(L"tail"))` and `T(A0(R"tail"))`, which select only
            // into alphabet 0: "If the <n> in the last T(A<n>) was not 0,
            // then the suboption is ignored" (§ 8.7.3.4.1).
            if self.text.alphabet != 0 || parent.int().is_some_and(|n| n != 0) {
                return;
            }
            let tail = &f.text[..usize::from(f.nt)];
            if let Some(cs) = designated(tail) {
                match f.key {
                    b'L' => self.text.gl = cs,
                    b'R' => self.text.gr = cs,
                    _ => {}
                }
            }
            return;
        }
        if r != 1 {
            return;
        }
        let n = f.int();
        match f.key {
            b'A' => match n {
                // "If a received value of <integer> exceeds the range
                // implemented in a device, or <integer> is not specified,
                // then the option is ignored" -- error 4.
                Some(n @ 0..=3) => self.text.alphabet = n as u8,
                Some(_) => self.error = (4, 0),
                None => {}
            },
            b'B' => self.saved_text = Some(self.text),
            // "If there was no previous B option specified, the default
            // values for all states are restored" (§ 8.7.3.4.4).
            b'E' => match self.saved_text {
                Some(t) => self.text = t,
                None => {
                    self.text = Text::DEFAULT;
                    self.size_escapement();
                }
            },
            // "Orient the baseline of each character at <ang> degrees to the
            // horizontal ... When used in conjunction with the S option,
            // string baseline is established" (§ 8.7.3.4.3): `D` alone turns
            // the characters, `D` then `S` the string with them, and a `D`
            // after those the characters again, as xterm reads the VT300's
            // `T(D<a>,S<n>,D<b>)`.
            b'D' => {
                self.text.char_dir = n.unwrap_or(0) as i32;
                self.tilt = match self.tilt {
                    Tilt::Ready | Tilt::Chars => Tilt::Chars,
                    Tilt::String | Tilt::Both => Tilt::Both,
                };
            }
            b'S' => {
                if self.tilt == Tilt::Chars {
                    self.tilt = Tilt::String;
                    self.text.string_dir = self.text.char_dir;
                }
                if f.next > 0 {
                    // "Set display cell size in the current screen addressing
                    // coordinates"; "If either <width> or <height> is
                    // missing, the missing values default to the values that
                    // correspond to the ... standard size 1".
                    let d = self.cell_size(f.ext[0], (9, 20));
                    self.text.display = d;
                    self.size_escapement();
                } else if let Some(n) = f.num {
                    match standard_size(n.trunc()) {
                        Some((d, u)) => {
                            self.text.display = d;
                            self.text.unit = u;
                            self.size_escapement();
                        }
                        // The VT300's error 9, "Text standard size error".
                        None => self.error = (9, 0),
                    }
                }
            }
            b'U' => {
                if f.next > 0 {
                    self.text.unit = self.cell_size(f.ext[0], (8, 20));
                }
            }
            // "Set the character height to be half of <number> times the
            // standard-size-1 cell height, leaving the character width
            // unchanged" -- ten pixels a unit, the VT300's "option value ×
            // 10".
            b'H' => {
                if let Some(v) = f.num {
                    let h = ((v.raw() * 10) >> 16).clamp(1, 2560) as i32;
                    self.text.display.1 = h;
                    self.text.unit.1 = h;
                }
            }
            b'I' => self.text.slant = n.unwrap_or(0).clamp(-45, 45) as i32,
            // "Multiply each pixel from the stored character image width by
            // hint, and each pixel from the stored character height by
            // vint", the stored cell the VT300's 8 by 10, and the display
            // cell with it.
            b'M' => {
                if f.next > 0 {
                    let v = |p: Part, d: i32| p.value().map_or(d, |v| v.trunc().clamp(1, 256));
                    let (hm, vm) = (v(f.ext[0].x, 1).min(16), v(f.ext[0].y, 2));
                    self.text.unit = (8 * hm, 10 * vm);
                    self.text.display = (9 * hm, 10 * vm);
                    self.size_escapement();
                }
            }
            b'W' => {}
            k => self.error(1, k),
        }
    }

    /// A cell size from user units, a missing part the default.
    fn cell_size(&self, e: Extent, default: (i32, i32)) -> (i32, i32) {
        let side = |p: Part, d: i32| match p.value() {
            Some(v) => round16(self.space.device_len(v.abs())).clamp(1, 1 << 14),
            None => d,
        };
        (side(e.x, default.0), side(e.y, default.1))
    }

    fn report_option(&mut self, f: &Frame, parent: &Frame, r: usize, out: &mut dyn RegisOut) {
        // "Some forms of the report command define a report for every
        // suboption letter as well, as is the case with macrograph reports"
        // (§ 8.5.7.5): `R(M(A)(B))` and `R(M(AB))` answer twice.
        if r == 2 && parent.key == b'M' {
            let mut m = *parent;
            m.sub = f.key;
            m.key = b'M';
            self.report(&m, out);
            return;
        }
        if r != 1 {
            return;
        }
        // A macrograph report whose suboptions each answered.
        if f.key == b'M' && f.sub != 0 {
            return;
        }
        self.report(f, out);
    }

    fn report(&mut self, f: &Frame, out: &mut dyn RegisOut) {
        let mut buf = [0u8; 48];
        let mut n = 0;
        let put = |s: &[u8], buf: &mut [u8; 48], n: &mut usize| {
            for &c in s {
                if *n < buf.len() {
                    buf[*n] = c;
                    *n += 1;
                }
            }
        };
        match (f.key, f.sub) {
            // **`R(P(I))`**, as graphics input runs (§ 8.8.5.3): in multiple
            // mode the report goes at once, behind the null button's string
            // ("If report position interactive is received, the report will
            // be prefixed by the null button because no button transition
            // initiated the report", 070-13 § 5.2), which the caller writes;
            // in one-shot mode the input cursor appears and the report waits
            // for the user's key or button, the interpretation suspended
            // meanwhile -- under [`RegisHold::On`], where the caller holds
            // what follows. An embedder that holds nothing has § 8.8.5.3's
            // first response, "The normal bracketed coordinate specifier,
            // just as from the normal R(P) command. This is the result from
            // a non-interactive device", counted.
            (b'P', b'I') if self.input.mode == RegisInputMode::Multiple => {
                let mut pos = [0u8; 56];
                let k = self.input_position(&mut pos);
                return out.interactive(&pos[..k]);
            }
            (b'P', b'I') if self.hold == RegisHold::On => {
                let at = self.space.fine(self.pos.0, self.pos.1);
                self.input.at = at;
                self.input.anchor = at;
                self.input.pending = Pending::Yes;
                return out.input_shown();
            }
            (b'P', b'I') => {
                self.counts.inputs_unheld += 1;
                put(b"[", &mut buf, &mut n);
                put(decimal(self.pos.0.round()).as_bytes(), &mut buf, &mut n);
                put(b",", &mut buf, &mut n);
                put(decimal(self.pos.1.round()).as_bytes(), &mut buf, &mut n);
                put(b"]", &mut buf, &mut n);
            }
            // "The returned report is a bracketed position specifier of the
            // form [x,y] representing the current drawing position in the
            // imposed coordinate system".
            (b'P', _) => {
                put(b"[", &mut buf, &mut n);
                put(decimal(self.pos.0.round()).as_bytes(), &mut buf, &mut n);
                put(b",", &mut buf, &mut n);
                put(decimal(self.pos.1.round()).as_bytes(), &mut buf, &mut n);
                put(b"]", &mut buf, &mut n);
            }
            // "R(M(=)) which returns a quoted string of the form
            // "<free>,<total>"".
            (b'M', b'=') => {
                put(b"\"", &mut buf, &mut n);
                put(decimal(self.macro_free as i32).as_bytes(), &mut buf, &mut n);
                put(b",", &mut buf, &mut n);
                put(decimal(MACRO_STORE as i32).as_bytes(), &mut buf, &mut n);
                put(b"\"", &mut buf, &mut n);
            }
            // A macrograph's contents: "Devices are therefore allowed the
            // freedom to restrict macrograph reports. The preferred fallback
            // is the empty report, that is, just the terminating CR
            // character" (§ 8.5.7.4.1), the owner's choice, 2026-10-20.
            (b'M', _) => {}
            // "The format of the error report is "n,m"" (§ 8.8.5.1); the
            // VT300's `"<N>,<M>"`.
            (b'E', _) => {
                put(b"\"", &mut buf, &mut n);
                put(
                    decimal(i32::from(self.error.0)).as_bytes(),
                    &mut buf,
                    &mut n,
                );
                put(b",", &mut buf, &mut n);
                put(
                    decimal(i32::from(self.error.1)).as_bytes(),
                    &mut buf,
                    &mut n,
                );
                put(b"\"", &mut buf, &mut n);
            }
            // "The format of the report is (A'<name>')" (§ 8.7.4.1), the
            // parentheses sent: Jev's lean, 2026-10-20 (PAREN_FORM, 0.34 and
            // 0.16), the VT330/VT340 manual printing `A'<name>'` and xterm
            // sending `A<n>'<name>'`.
            (b'L', _) => {
                let (name, len) = match self.load {
                    1..=3 => {
                        let a = &self.alphabets[usize::from(self.load - 1)];
                        (a.name, usize::from(a.name_len))
                    }
                    _ => ([0; NAME_MAX], 0),
                };
                let q: &[u8] = if name[..len].contains(&b'\'') {
                    b"\""
                } else {
                    b"'"
                };
                put(b"(A", &mut buf, &mut n);
                put(q, &mut buf, &mut n);
                put(&name[..len], &mut buf, &mut n);
                put(q, &mut buf, &mut n);
                put(b")", &mut buf, &mut n);
            }
            // `R(I<n>)`: "When the terminal receives R(I), it returns a
            // carriage return" (`[[source: vt3xx-report]]`). `R(I0)` selects
            // one-shot mode and leaves multiple mode; `R(I1)` selects
            // multiple mode, and "the input cursor appears on the screen"
            // where the drawing position is. A number neither names
            // changes nothing.
            (b'I', _) => match f.int() {
                Some(0) => self.input.mode = RegisInputMode::OneShot,
                Some(1) => {
                    if self.input.mode != RegisInputMode::Multiple {
                        let at = self.space.fine(self.pos.0, self.pos.1);
                        self.input.at = at;
                        self.input.anchor = at;
                    }
                    self.input.mode = RegisInputMode::Multiple;
                    out.input_shown();
                }
                _ => {}
            },
            // "Requests for unimplemented reports are acknowledged by the
            // device's returning just a carriage return".
            _ => {}
        }
        // "All reports are terminated by a single carriage return
        // character" (§ 8.5.7.5).
        put(b"\r", &mut buf, &mut n);
        out.reply(&buf[..n]);
    }

    fn load_option(&mut self, f: &Frame, r: usize) {
        if r != 1 {
            return;
        }
        match f.key {
            b'A' => {
                let name = |s: &mut Sem| {
                    if f.nt > 0 || f.text_first == On::Yes {
                        match s.loading() {
                            Some(a) => {
                                let n = usize::from(f.nt).min(NAME_MAX);
                                a.name = [0; NAME_MAX];
                                a.name[..n].copy_from_slice(&f.text[..n]);
                                a.name_len = n as u8;
                            }
                            None => s.counts.load_refused += 1,
                        }
                    }
                };
                // "if <name> is specified first, it is associated with the
                // currently selected set, not necessarily the one specified
                // by <integer>" (§ 8.7.5.4.2).
                if f.text_first == On::Yes {
                    name(self);
                }
                match f.int() {
                    Some(n @ 0..=3) => self.load = n as u8,
                    // "If a specified value for <integer> exceeds the
                    // implementation limit, the value is ignored" -- error 4.
                    Some(_) => self.error = (4, 0),
                    None => {}
                }
                if f.text_first == On::No {
                    name(self);
                }
            }
            b'E' => match (f.int(), self.loading()) {
                (Some(n @ 1..=192), Some(a)) => {
                    a.extent = n as u16;
                    a.clear();
                }
                _ => {
                    self.counts.load_refused += 1;
                    self.error = (5, 0);
                }
            },
            k => self.error(1, k),
        }
    }

    fn fill_option(&mut self, f: &Frame, r: usize, d: usize, out: &mut dyn RegisOut) {
        let _ = out;
        let parent = self.frames[d - 1].key;
        match (r, f.key) {
            // The edges' keys end their contour; the vertices came as they
            // were given.
            (1, b'V' | b'P' | b'C') => {}
            (1, b'F') => {}
            (1, b'W') => {}
            // `F(B ...)`, flood's boundary, kept until the next or a valid
            // `S(A)`: "The border of the flooding operation is as
            // determined by the last previous F(B()) encountered".
            (1, b'B') => {}
            (2, b'F') if parent == b'B' => match f.int() {
                Some(0) => self.bound = RegisBound::Background,
                Some(1) | None => self.bound = RegisBound::Seed,
                Some(_) => self.error(1, f.key),
            },
            (2, b'I') if parent == b'B' => {
                if let Some(c) = f.color() {
                    if let Some(px) = c.pixel() {
                        self.bound = RegisBound::Index(regis_closest(&self.map, px));
                    }
                } else if let Some(n) = f.int() {
                    self.bound = RegisBound::Index(RegisIndex::wrap(n));
                }
            }
            (1, k) => self.error(1, k),
            _ => {}
        }
    }

    /// **A flood's seed** (§ 8.8.6.1): "If more than one specifier is
    /// given, each is executed in turn", a relative part measured from the
    /// seed before it and the first from the drawing position, as
    /// "multiple specifiers have cumulative action" (§ 8.4.5.1; Jev's lean,
    /// CUMULATIVE, 0.42 and 0.46 on a second ask); "The current position
    /// does not change as the result of any flood operations". The pen is
    /// a fill's, the texture the last shading snapshot, "even if shading
    /// has been subsequently disabled by W(S0)" (§ 8.8.6.4).
    fn flood_seed(&mut self, at: Pt, out: &mut dyn RegisOut) {
        self.seed = Some(at);
        self.end_stroke(out);
        let pen = self.fill_pen();
        out.flood(self.device(at), self.bound, &pen);
        self.last_end = None;
    }

    /// The `(` after `F` opens a fill: the position saved, "and restored at
    /// the end of an F command, whether any drawing takes place or not"
    /// (§ 8.5.8.4.1).
    fn fill_begin(&mut self) {
        let mut f = Fill::NEW;
        f.saved = self.pos;
        self.fill = Some(f);
    }

    fn fill_draw(&mut self, out: &mut dyn RegisOut) {
        let Some(mut f) = self.fill.take() else {
            return;
        };
        f.close();
        if f.nends > 0 {
            self.end_stroke(out);
            let pen = self.fill_pen();
            out.fill(&f.points[..f.n], &f.ends[..f.nends], &pen);
            self.last_end = None;
        }
        self.pos = f.saved;
    }

    /// A fill's pen: the area texture is the shading snapshot, "An area
    /// texture is established by invoking shading and disabling it"
    /// (§ 8.5.8.4.1), solid with none.
    fn fill_pen(&self) -> RegisPen {
        let mut p = self.pen();
        p.shade = Some((ShadeTo::Row(0), self.texture(self.tw.shade_with)));
        p
    }
}

/// Where what a `V` or a `C` makes goes: drawn on the page, or into a
/// fill's vertex list (§ 8.5.8.4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dest {
    Page,
    Fill,
}

impl Dest {
    /// A command at depth 0 draws; one inside a fill's option list fills.
    fn of(base: usize) -> Dest {
        match base {
            0 => Dest::Page,
            _ => Dest::Fill,
        }
    }
}

/// The code a quoted character has when it has none in the in-use table:
/// a UTF-8 character past U+00FF, or a malformed one.
pub(crate) const NO_CODE: u16 = 0x100;

/// Whether a font drew a character.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Drawn {
    No,
    Yes,
}

/// Whether a built-in character with no font is counted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tally {
    No,
    Yes,
}

/// Whose writing controls a `W` changes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    Persistent,
    Temporary,
}

/// Whether a line closes a loop, so its end is the first line's start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Close {
    Open,
    Loop,
}

/// `W(P)`'s digits: "If a digit string consists entirely of ones and zeros,
/// then it is interpreted as a binary pattern specification. Any other
/// single digit specifies one of the line patterns 2 through 9 ... The last
/// such digit in a sequence of digits selects the appropriate pattern"
/// (§ 8.5.3.4.2).
fn pattern_of(digits: &[u8]) -> LinePattern {
    if digits.iter().all(|&d| d == b'0' || d == b'1') {
        return LinePattern::binary(digits);
    }
    digits
        .iter()
        .rev()
        .find_map(|&d| match d {
            b'2'..=b'9' => LinePattern::standard(d - b'0'),
            _ => None,
        })
        .unwrap_or(LinePattern::SOLID)
}

/// A designating tail (§ 8.5.6.4.1): `0B` and `(B` ASCII, `1A` and `-A`
/// Latin-1, `0%5` and `)%5` DEC Supplemental; the column-three
/// intermediates `0` (a 94-set) and `1` (a 96-set), the column-two ones `(`
/// to `+` and `-` to `/`. A multi-byte, version or revision tail, or an
/// incomplete one, selects nothing: "An incomplete or illegal tail will be
/// ignored".
fn designated(tail: &[u8]) -> Option<Charset> {
    let (&i, rest) = tail.split_first()?;
    let set = |rest: &[u8]| -> Option<(Option<StreamByte>, StreamByte)> {
        match rest {
            [f] => Some((None, StreamByte::from_stream(*f))),
            [i2, f] if (0x20..=0x2f).contains(i2) => Some((
                Some(StreamByte::from_stream(*i2)),
                StreamByte::from_stream(*f),
            )),
            _ => None,
        }
    };
    match i {
        b'0' | b'(' | b')' | b'*' | b'+' => {
            let (i2, f) = set(rest)?;
            Some(Designation::of_94(i2, f).charset())
        }
        b'1' | b'-' | b'.' | b'/' => {
            let (i2, f) = set(rest)?;
            if i2.is_some() {
                return None;
            }
            Some(Designation::of_96(f).charset())
        }
        _ => None,
    }
}

/// The character a code of alphabet 0 is: GL through the left set, GR
/// through the right, by the sets' own tables.
fn builtin(gl: Charset, gr: Charset, c: u8) -> Option<char> {
    let (set, b) = match c {
        0x20..=0x7f => (gl, c),
        0xa0..=0xff => (gr, c - 0x80),
        _ => return None,
    };
    // A 94-character set has no character at 7/15, nor at 15/15 in GR.
    if b == 0x7f && !set.is_96() {
        return None;
    }
    let cp = crate::PackedCell::translate(StreamByte::from_stream(b), set);
    char::from_u32(cp.0)
}

/// A mask row of `w` ones from the left.
fn solid_row(w: i32) -> u16 {
    !(0xffffu16 >> w.clamp(0, 16))
}

/// `n` in decimal, a sign before a negative.
fn decimal(n: i32) -> Decimal {
    let mut d = Decimal {
        buf: [0; 12],
        len: 0,
    };
    let mut v = i64::from(n).unsigned_abs();
    let mut tmp = [0u8; 12];
    let mut k = 0;
    loop {
        tmp[k] = b'0' + (v % 10) as u8;
        k += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if n < 0 {
        d.buf[0] = b'-';
        d.len = 1;
    }
    for i in (0..k).rev() {
        d.buf[d.len] = tmp[i];
        d.len += 1;
    }
    d
}

struct Decimal {
    buf: [u8; 12],
    len: usize,
}

impl Decimal {
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}
